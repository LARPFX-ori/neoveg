//! Gerenciamento de mídia: importação, decodificação e cache de quadros.

#[forbid(unsafe_code)]
pub mod audio;
#[forbid(unsafe_code)]
pub mod avi;
#[forbid(unsafe_code)]
pub mod convert;
#[forbid(unsafe_code)]
pub mod demux;
#[forbid(unsafe_code)]
pub mod generate;
#[cfg(windows)]
mod mf;
#[forbid(unsafe_code)]
pub mod wav;
#[forbid(unsafe_code)]
pub mod y4m;

use crate::model::{new_id, FrameRate, GenSpec, Id, MediaItem, MediaSource, Ticks, TICKS_PER_MS};
use anyhow::{bail, Result};
pub use audio::AudioData;
use image::RgbaImage;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const IMAGE_EXTS: [&str; 8] = ["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp", "gif"];
pub const AUDIO_EXTS: [&str; 9] = [
    "wav", "wave", "aif", "aiff", "aifc", "flac", "ogg", "oga", "mp3",
];
pub const VIDEO_EXTS: [&str; 2] = ["avi", "y4m"];
/// Contêineres de mídia decodificados via API nativa do sistema operacional
/// (Windows Media Foundation via módulo FFI isolado `media::mf`).
pub const SYSTEM_CONTAINER_EXTS: [&str; 9] = [
    "mp4", "mov", "mkv", "webm", "m4v", "m4a", "aac", "wmv", "asf",
];
pub const UNSUPPORTED_CONTAINERS: [&str; 7] = ["mp4", "mov", "mkv", "webm", "m4v", "m4a", "aac"];

/// Memória máxima para os quadros de um GIF animado.
const MAX_GIF_BYTES: u64 = 512 << 20;

pub fn is_supported(path: &Path) -> bool {
    let ext = ext_of(path);
    IMAGE_EXTS.contains(&ext.as_str())
        || AUDIO_EXTS.contains(&ext.as_str())
        || VIDEO_EXTS.contains(&ext.as_str())
        || SYSTEM_CONTAINER_EXTS.contains(&ext.as_str())
}

fn ext_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

pub struct FrameCache {
    items: VecDeque<(usize, Arc<RgbaImage>, usize)>,
    cap: usize,
    total_bytes: usize,
    max_bytes: usize,
}

impl FrameCache {
    fn new(cap: usize) -> Self {
        Self::with_budget(cap, 64 << 20)
    }

    fn with_budget(cap: usize, max_bytes: usize) -> Self {
        FrameCache {
            items: VecDeque::new(),
            cap: cap.max(1),
            total_bytes: 0,
            max_bytes: max_bytes.max(1 << 20),
        }
    }

    fn set_budget(&mut self, cap: usize, max_bytes: usize) {
        self.cap = cap.max(1);
        self.max_bytes = max_bytes.max(1 << 20);
        self.evict();
    }

    fn clear(&mut self) {
        self.items.clear();
        self.total_bytes = 0;
    }

    fn evict(&mut self) {
        while self.items.len() > self.cap
            || (self.items.len() > 1 && self.total_bytes > self.max_bytes)
        {
            if let Some((_, _, bytes)) = self.items.pop_back() {
                self.total_bytes = self.total_bytes.saturating_sub(bytes);
            } else {
                break;
            }
        }
    }

    fn get(&mut self, k: usize) -> Option<Arc<RgbaImage>> {
        let pos = self.items.iter().position(|(i, _, _)| *i == k)?;
        let item = self.items.remove(pos)?;
        let img = item.1.clone();
        self.items.push_front(item);
        Some(img)
    }

    fn put(&mut self, k: usize, img: Arc<RgbaImage>) {
        let bytes = img.as_raw().len();
        self.total_bytes = self.total_bytes.saturating_add(bytes);
        self.items.push_front((k, img, bytes));
        self.evict();
    }
}

pub enum VideoSource {
    None,
    Still(Arc<RgbaImage>),
    /// Quadros pré-decodificados (GIF animado) com tempos de início em ticks.
    Frames(Vec<Arc<RgbaImage>>, Vec<Ticks>),
    Avi(Mutex<avi::AviReader>, Mutex<FrameCache>),
    Y4m(Mutex<y4m::Y4mReader>, Mutex<FrameCache>),
    #[cfg(windows)]
    Mf(Arc<mf::MfVideoReader>, Mutex<FrameCache>),
    Sequence(Vec<String>, FrameRate, Mutex<FrameCache>),
    Generated,
}

impl VideoSource {
    fn set_cache_budget(&self, cap: usize, max_bytes: usize) {
        match self {
            VideoSource::Avi(_, c) | VideoSource::Y4m(_, c) | VideoSource::Sequence(_, _, c) => {
                c.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .set_budget(cap, max_bytes);
            }
            #[cfg(windows)]
            VideoSource::Mf(_, c) => {
                c.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .set_budget(cap, max_bytes);
            }
            VideoSource::None
            | VideoSource::Still(_)
            | VideoSource::Frames(_, _)
            | VideoSource::Generated => {}
        }
    }

    fn clear_cache(&self) {
        match self {
            VideoSource::Avi(_, c) | VideoSource::Y4m(_, c) | VideoSource::Sequence(_, _, c) => {
                c.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clear();
            }
            #[cfg(windows)]
            VideoSource::Mf(_, c) => {
                c.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clear();
            }
            VideoSource::None
            | VideoSource::Still(_)
            | VideoSource::Frames(_, _)
            | VideoSource::Generated => {}
        }
    }
}

pub struct MediaEntry {
    pub video: VideoSource,
    pub audio: Option<Arc<AudioData>>,
}

/// Chave do cache de mídia gerada: (mídia, largura, altura, tempo em ms).
type GenKey = (Id, u32, u32, u64);

/// Cache compartilhado entre interface, prévia, reprodução e renderização, com
/// orçamento configurável de RAM e número de quadros (§3.6).
pub struct MediaCache {
    entries: Mutex<HashMap<Id, Arc<MediaEntry>>>,
    proxies: Mutex<HashMap<(Id, String), Arc<MediaEntry>>>,
    generated: Mutex<HashMap<GenKey, Arc<RgbaImage>>>,
    frame_cap: Mutex<usize>,
    ram_budget_bytes: Mutex<usize>,
    pub fonts: generate::Fonts,
}

impl Default for MediaCache {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            proxies: Mutex::new(HashMap::new()),
            generated: Mutex::new(HashMap::new()),
            frame_cap: Mutex::new(48),
            ram_budget_bytes: Mutex::new(256 << 20),
            fonts: generate::Fonts::default(),
        }
    }
}

impl MediaCache {
    pub fn insert(&self, id: Id, e: MediaEntry) {
        let cap = *self
            .frame_cap
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let budget = *self
            .ram_budget_bytes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        e.video.set_cache_budget(cap, (budget / 2).max(8 << 20));
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id, Arc::new(e));
    }

    pub fn remove(&self, id: Id) {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);
        self.proxies
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|(k, _), _| *k != id);
    }

    pub fn contains(&self, id: Id) -> bool {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(&id)
    }

    /// Atualiza os limites de cache em execução a partir das Preferências (§3.6).
    pub fn configure_limits(&self, frame_cap: usize, ram_mb: u32) {
        let cap = frame_cap.clamp(4, 512);
        let budget = (ram_mb.clamp(32, 8192) as usize) << 20;
        *self
            .frame_cap
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = cap;
        *self
            .ram_budget_bytes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = budget;

        let per_stream_budget = (budget / 2).max(8 << 20);
        for entry in self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
        {
            entry.video.set_cache_budget(cap, per_stream_budget);
        }
        for entry in self
            .proxies
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
        {
            entry.video.set_cache_budget(cap, per_stream_budget);
        }
        let mut g = self
            .generated
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if g.len() > cap {
            g.clear();
        }
    }

    /// Libera todos os caches de quadros sob pressão de memória (§3.6).
    pub fn clear_frame_caches(&self) {
        for entry in self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
        {
            entry.video.clear_cache();
        }
        for entry in self
            .proxies
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
        {
            entry.video.clear_cache();
        }
        self.generated
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    fn entry(&self, id: Id) -> Option<Arc<MediaEntry>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&id)
            .cloned()
    }

    pub fn audio_map(&self) -> HashMap<Id, Arc<AudioData>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter_map(|(k, e)| e.audio.clone().map(|a| (*k, a)))
            .collect()
    }

    /// Limpa o cache de mídia gerada de um item (após edição).
    pub fn invalidate_generated(&self, id: Id) {
        self.generated
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|k, _| k.0 != id);
    }

    /// Quadro da mídia no tempo exato `t` (em ticks). `w`,`h` são usados por
    /// mídia gerada (que é produzida diretamente na resolução de saída).
    pub fn video_frame(
        &self,
        item: &MediaItem,
        t: Ticks,
        w: u32,
        h: u32,
    ) -> Option<Arc<RgbaImage>> {
        self.video_frame_for_quality(item, t, w, h, false)
    }

    /// Quadro da mídia respeitando a preferência por proxy na prévia (§3.6).
    pub fn video_frame_for_quality(
        &self,
        item: &MediaItem,
        t: Ticks,
        w: u32,
        h: u32,
        prefer_proxy: bool,
    ) -> Option<Arc<RgbaImage>> {
        if let MediaSource::Generated(spec) = &item.source {
            return Some(self.generated_frame(item.id, spec, t, w, h));
        }
        if prefer_proxy {
            if let Some(proxy_path) = &item.proxy_path {
                if let Some(pe) = self.proxy_entry(item.id, proxy_path) {
                    if let Some(img) = self.decode_from_source(&pe.video, item, t) {
                        return Some(img);
                    }
                }
            }
        }
        let e = self.entry(item.id)?;
        self.decode_from_source(&e.video, item, t)
    }

    fn proxy_entry(&self, media_id: Id, path: &str) -> Option<Arc<MediaEntry>> {
        let key = (media_id, path.to_string());
        if let Some(existing) = self
            .proxies
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
            .cloned()
        {
            return Some(existing);
        }
        let p = Path::new(path);
        if !p.exists() {
            return None;
        }
        let mut dummy = base_item("proxy".into(), MediaSource::File { path: path.into() });
        let loaded = load_entry(&mut dummy).ok()?;
        let cap = *self
            .frame_cap
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let budget = *self
            .ram_budget_bytes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loaded
            .entry
            .video
            .set_cache_budget(cap, (budget / 2).max(8 << 20));
        let arc = Arc::new(loaded.entry);
        self.proxies
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key, arc.clone());
        Some(arc)
    }

    fn decode_from_source(
        &self,
        source: &VideoSource,
        item: &MediaItem,
        t: Ticks,
    ) -> Option<Arc<RgbaImage>> {
        // `item` só é lido pelo leitor do Media Foundation (Windows); sem
        // isto, o build fora do Windows falha com `-D warnings`.
        #[cfg(not(windows))]
        let _ = item;
        match source {
            VideoSource::None | VideoSource::Generated => None,
            VideoSource::Still(img) => Some(img.clone()),
            VideoSource::Frames(frames, times) => {
                let i = match times.binary_search(&t) {
                    Ok(i) => i,
                    Err(i) => i.saturating_sub(1),
                };
                frames.get(i.min(frames.len().saturating_sub(1))).cloned()
            }
            VideoSource::Avi(r, cache) => {
                let mut rd = r.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let max_idx = rd.frame_count().saturating_sub(1) as i64;
                let idx = rd.fps.ticks_to_frame_floor(t).clamp(0, max_idx) as usize;
                if let Some(img) = cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(idx)
                {
                    return Some(img);
                }
                let img = Arc::new(
                    crate::util::guard("decodificação AVI", || rd.decode_frame(idx))
                        .ok()?
                        .ok()?,
                );
                cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .put(idx, img.clone());
                Some(img)
            }
            VideoSource::Y4m(r, cache) => {
                let mut rd = r.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let max_idx = rd.frames.saturating_sub(1) as i64;
                let idx = rd.fps.ticks_to_frame_floor(t).clamp(0, max_idx) as usize;
                if let Some(img) = cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(idx)
                {
                    return Some(img);
                }
                let img = Arc::new(
                    crate::util::guard("decodificação Y4M", || rd.decode_frame(idx))
                        .ok()?
                        .ok()?,
                );
                cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .put(idx, img.clone());
                Some(img)
            }
            #[cfg(windows)]
            VideoSource::Mf(reader, cache) => {
                let fps = item.fps.unwrap_or(reader.fps);
                let max_idx = item
                    .duration
                    .map(|d| fps.ticks_to_frame_floor(d).max(0))
                    .unwrap_or(i64::MAX / 2);
                let idx = fps.ticks_to_frame_floor(t).clamp(0, max_idx) as usize;
                if let Some(img) = cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(idx)
                {
                    return Some(img);
                }
                let frame_time = fps.frame_to_ticks(idx as i64);
                let img = Arc::new(
                    crate::util::guard("decodificação Media Foundation", || {
                        reader.decode_at(frame_time)
                    })
                    .ok()?
                    .ok()?,
                );
                cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .put(idx, img.clone());
                Some(img)
            }
            VideoSource::Sequence(paths, fps, cache) => {
                let max_idx = paths.len().saturating_sub(1) as i64;
                let idx = fps.ticks_to_frame_floor(t).clamp(0, max_idx) as usize;
                if let Some(img) = cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(idx)
                {
                    return Some(img);
                }
                let path = paths.get(idx)?;
                let img = Arc::new(
                    crate::util::guard("decodificação de imagem", || image::open(path))
                        .ok()?
                        .ok()?
                        .to_rgba8(),
                );
                cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .put(idx, img.clone());
                Some(img)
            }
        }
    }

    /// Gera um arquivo proxy AVI MJPEG em 1/4 da resolução (até 480x270)
    /// em streaming quadro a quadro para edição fluida em hardware fraco (§3.6).
    pub fn create_proxy_avi(&self, item: &MediaItem, out_path: &Path) -> Result<()> {
        if !item.has_video {
            bail!("mídia '{}' não contém vídeo para gerar proxy", item.name);
        }
        let dur = item.duration.unwrap_or_else(|| Ticks::from_secs_i64(5));
        let fps = item.fps.unwrap_or(FrameRate::FPS_30);
        let src_w = item.width.max(16);
        let src_h = item.height.max(16);
        let mut pw = ((src_w / 4).max(64) / 2) * 2;
        let mut ph = ((src_h / 4).max(64) / 2) * 2;
        if pw > 480 {
            let scale = 480.0 / pw as f32;
            pw = 480;
            ph = (((ph as f32 * scale).round() as u32).max(64) / 2) * 2;
        }
        let n_frames = fps.ticks_to_frame_floor(dur).max(1) as usize;
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut w = avi::AviWriter::create(out_path, pw, ph, fps, None)?;
        for fi in 0..n_frames {
            let t = fps.frame_to_ticks(fi as i64);
            let frame = self
                .video_frame_for_quality(item, t, pw, ph, false)
                .map(|img| {
                    if img.width() == pw && img.height() == ph {
                        (*img).clone()
                    } else {
                        image::imageops::resize(
                            &*img,
                            pw,
                            ph,
                            image::imageops::FilterType::Triangle,
                        )
                    }
                })
                .unwrap_or_else(|| RgbaImage::new(pw, ph));
            w.write_video_jpeg(&convert::encode_jpeg(&frame, 75)?)?;
        }
        w.finish()?;
        Ok(())
    }

    fn generated_frame(&self, id: Id, spec: &GenSpec, t: Ticks, w: u32, h: u32) -> Arc<RgbaImage> {
        let tk = if spec.is_animated() {
            (t.0 / TICKS_PER_MS).max(0) as u64
        } else {
            0
        };
        let key = (id, w, h, tk);
        if let Some(img) = self
            .generated
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
        {
            return img.clone();
        }
        let img = match crate::util::guard("mídia gerada", || {
            generate::render(spec, t.as_secs_f64(), w, h, &self.fonts)
        }) {
            Ok(i) => Arc::new(i),
            Err(_) => Arc::new(RgbaImage::new(w.max(1), h.max(1))),
        };
        let cap = *self
            .frame_cap
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut g = self
            .generated
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if g.len() > cap {
            g.clear();
        }
        g.insert(key, img.clone());
        img
    }

    /// Miniatura (para a janela de mídia do projeto e eventos).
    pub fn thumbnail(&self, item: &MediaItem, max: u32) -> Option<RgbaImage> {
        let (w, h) = if item.width > 0 && item.height > 0 {
            (item.width, item.height)
        } else {
            (1920, 1080)
        };
        let s = max as f32 / w.max(h) as f32;
        let (tw, th) = (
            ((w as f32 * s) as u32).max(1),
            ((h as f32 * s) as u32).max(1),
        );
        let t = item
            .duration
            .map(|d| d.div_i64(10).min(Ticks::from_secs_i64(1)))
            .unwrap_or(Ticks::ZERO);
        let f = self.video_frame(item, t, tw, th)?;
        if f.width() == tw && f.height() == th {
            return Some((*f).clone());
        }
        Some(image::imageops::resize(
            &*f,
            tw,
            th,
            image::imageops::FilterType::Triangle,
        ))
    }
}

/// Resultado da importação: item para o projeto e dados para o cache.
pub struct Imported {
    pub item: MediaItem,
    pub entry: MediaEntry,
    pub warning: Option<String>,
}

fn base_item(name: String, source: MediaSource) -> MediaItem {
    MediaItem {
        id: new_id(),
        name,
        source,
        duration: None,
        has_video: false,
        has_audio: false,
        width: 0,
        height: 0,
        fps: None,
        info: String::new(),
        proxy_path: None,
    }
}

pub fn import_generated(spec: GenSpec) -> Imported {
    let mut item = base_item(spec.label(), MediaSource::Generated(spec));
    item.has_video = true;
    item.info = "Mídia gerada".into();
    Imported {
        item,
        entry: MediaEntry {
            video: VideoSource::Generated,
            audio: None,
        },
        warning: None,
    }
}

/// Importa um arquivo de mídia do disco.
pub fn import_file(path: &Path) -> Result<Imported> {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut item = base_item(
        name,
        MediaSource::File {
            path: path.to_string_lossy().to_string(),
        },
    );
    let mut imp = load_entry(&mut item)?;
    imp.item = item;
    Ok(imp)
}

/// Carrega (ou recarrega, ao abrir um projeto) os dados de um item de mídia.
/// Atualiza as informações do item (duração, dimensões...). Pânicos dos
/// decodificadores são isolados e viram erro (§0.6).
pub fn load_entry(item: &mut MediaItem) -> Result<Imported> {
    match crate::util::guard("importação de mídia", || load_entry_inner(item)) {
        Ok(r) => r,
        Err(e) => bail!("{e}"),
    }
}

fn load_entry_inner(item: &mut MediaItem) -> Result<Imported> {
    let mut warning = None;
    let entry = match item.source.clone() {
        MediaSource::Generated(_) => MediaEntry {
            video: VideoSource::Generated,
            audio: None,
        },
        MediaSource::ImageSequence { paths, fps } => {
            if paths.is_empty() {
                bail!("sequência vazia");
            }
            let Some(first_path) = paths.first() else {
                bail!("sequência vazia")
            };
            let first = image::open(first_path)?;
            item.width = first.width();
            item.height = first.height();
            item.fps = Some(fps);
            item.has_video = true;
            item.duration = Some(fps.frame_to_ticks(paths.len() as i64));
            item.info = format!(
                "Sequência de {} imagens, {}x{}, {}",
                paths.len(),
                item.width,
                item.height,
                fps.label()
            );
            MediaEntry {
                video: VideoSource::Sequence(paths, fps, Mutex::new(FrameCache::new(16))),
                audio: None,
            }
        }
        MediaSource::File { path } => {
            let p = PathBuf::from(&path);
            if !p.exists() {
                bail!("arquivo não encontrado: {path}");
            }
            let ext = ext_of(&p);
            if ext == "gif" {
                load_gif(&p, item)?
            } else if IMAGE_EXTS.contains(&ext.as_str()) {
                let img = image::open(&p)?.to_rgba8();
                item.width = img.width();
                item.height = img.height();
                item.has_video = true;
                item.duration = None;
                item.info = format!("Imagem {}x{}", img.width(), img.height());
                MediaEntry {
                    video: VideoSource::Still(Arc::new(img)),
                    audio: None,
                }
            } else if ext == "avi" {
                let mut rd = avi::AviReader::open(&p)?;
                let audio = match rd.read_audio()? {
                    Some((sr, ch, s)) if !s.is_empty() => {
                        Some(Arc::new(AudioData::from_interleaved(sr, ch as usize, &s)))
                    }
                    _ => None,
                };
                if rd.audio.is_some() && audio.is_none() {
                    warning = Some(format!("{}: áudio do AVI não é PCM; ignorado", item.name));
                }
                if let Some(m) = rd.video_supported_msg() {
                    warning = Some(format!("{}: {m}", item.name));
                }
                item.has_video = rd.has_video();
                item.has_audio = audio.is_some();
                item.width = rd.width;
                item.height = rd.height;
                item.fps = Some(rd.fps);
                let vd = if item.has_video {
                    Some(rd.duration())
                } else {
                    None
                };
                let ad = audio.as_ref().map(|a| a.duration());
                item.duration = vd.or(ad);
                if !item.has_video && !item.has_audio {
                    bail!(
                        "{}",
                        warning
                            .take()
                            .unwrap_or_else(|| "AVI sem fluxos suportados".into())
                    );
                }
                item.info = format!(
                    "AVI {} {}x{} {}, {} quadros{}",
                    rd.video_fourcc,
                    rd.width,
                    rd.height,
                    rd.fps.label(),
                    rd.frame_count(),
                    audio
                        .as_ref()
                        .map(|a| format!(", áudio {} Hz", a.sample_rate))
                        .unwrap_or_default()
                );
                MediaEntry {
                    video: if item.has_video {
                        VideoSource::Avi(Mutex::new(rd), Mutex::new(FrameCache::new(24)))
                    } else {
                        VideoSource::None
                    },
                    audio,
                }
            } else if ext == "y4m" {
                let rd = y4m::Y4mReader::open(&p)?;
                item.has_video = true;
                item.width = rd.width;
                item.height = rd.height;
                item.fps = Some(rd.fps);
                item.duration = Some(rd.duration());
                item.info = format!(
                    "Y4M {}x{} {}, {} quadros",
                    rd.width,
                    rd.height,
                    rd.fps.label(),
                    rd.frames
                );
                MediaEntry {
                    video: VideoSource::Y4m(Mutex::new(rd), Mutex::new(FrameCache::new(12))),
                    audio: None,
                }
            } else if SYSTEM_CONTAINER_EXTS.contains(&ext.as_str()) {
                load_system_container(&p, item)?
            } else {
                let a = audio::decode_file(&p)?;
                item.has_audio = true;
                let dur = a.duration();
                item.duration = Some(dur);
                item.info = format!("Áudio {} Hz, {:.2} s", a.sample_rate, dur.as_secs_f64());
                MediaEntry {
                    video: VideoSource::None,
                    audio: Some(Arc::new(a)),
                }
            }
        }
    };
    Ok(Imported {
        item: item.clone(),
        entry,
        warning,
    })
}

fn load_gif(p: &Path, item: &mut MediaItem) -> Result<MediaEntry> {
    use image::AnimationDecoder;
    let f = std::io::BufReader::new(std::fs::File::open(p)?);
    // o tamanho da tela lógica vem do arquivo: valida antes de decodificar,
    // pois o compositor de quadros aloca largura x altura x 4 bytes (§0.4)
    let mut head = [0u8; 10];
    std::io::Read::read_exact(&mut std::fs::File::open(p)?, &mut head)?;
    if &head[0..3] != b"GIF" {
        bail!("não é um GIF");
    }
    let gw = u16::from_le_bytes([head[6], head[7]]) as u32;
    let gh = u16::from_le_bytes([head[8], head[9]]) as u32;
    if gw == 0 || gh == 0 || gw > crate::model::MAX_DIM || gh > crate::model::MAX_DIM {
        bail!("dimensões de GIF inválidas: {gw}x{gh}");
    }
    let mut dec = image::codecs::gif::GifDecoder::new(f)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(crate::model::MAX_DIM);
    limits.max_image_height = Some(crate::model::MAX_DIM);
    limits.max_alloc = Some(512 << 20);
    image::ImageDecoder::set_limits(&mut dec, limits)?;
    let frames = dec.into_frames();
    let mut imgs = vec![];
    let mut times = vec![];
    let mut t = Ticks::ZERO;
    let mut total_bytes: u64 = 0;
    for fr in frames {
        let fr = fr?;
        total_bytes += fr.buffer().as_raw().len() as u64;
        if total_bytes > MAX_GIF_BYTES {
            bail!("GIF animado grande demais para a memória");
        }
        let (n, d) = fr.delay().numer_denom_ms();
        let dur_ticks = if d == 0 || (n as u64) < (d as u64 * 10) {
            Ticks::from_millis_i64(100)
        } else {
            Ticks::from_millis_ratio(n, d)
        };
        times.push(t);
        t += dur_ticks;
        imgs.push(Arc::new(fr.into_buffer()));
    }
    let Some(first) = imgs.first() else {
        bail!("GIF sem quadros")
    };
    item.width = first.width();
    item.height = first.height();
    item.has_video = true;
    if imgs.len() == 1 {
        item.duration = None;
        item.info = format!("GIF {}x{}", item.width, item.height);
        return Ok(MediaEntry {
            video: VideoSource::Still(imgs.remove(0)),
            audio: None,
        });
    }
    item.duration = Some(t);
    item.fps = FrameRate::from_f64(imgs.len() as f64 / t.as_secs_f64().max(1e-3));
    item.info = format!(
        "GIF animado {}x{}, {} quadros, {:.2} s",
        item.width,
        item.height,
        imgs.len(),
        t.as_secs_f64()
    );
    Ok(MediaEntry {
        video: VideoSource::Frames(imgs, times),
        audio: None,
    })
}

/// Detecta uma sequência numerada a partir do primeiro arquivo
/// (ex.: quadro_0001.png, quadro_0002.png, ...).
pub fn detect_sequence(first: &Path) -> Vec<String> {
    let Some(stem) = first.file_stem().and_then(|s| s.to_str()) else {
        return vec![];
    };
    let ext = first.extension().and_then(|s| s.to_str()).unwrap_or("");
    let digits: String = stem
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if digits.is_empty() {
        return vec![first.to_string_lossy().to_string()];
    }
    let prefix = &stem[..stem.len() - digits.len()];
    let width = digits.len();
    let start: u64 = digits.parse().unwrap_or(0);
    let dir = first.parent().unwrap_or(Path::new("."));
    let mut out = vec![];
    let mut n = start;
    loop {
        let name = format!("{prefix}{:0width$}.{ext}", n, width = width);
        let p = dir.join(&name);
        if !p.exists() {
            break;
        }
        out.push(p.to_string_lossy().to_string());
        n += 1;
    }
    out
}

pub fn import_sequence(first: &Path, fps: FrameRate) -> Result<Imported> {
    let paths = detect_sequence(first);
    if paths.is_empty() {
        bail!("nenhuma imagem encontrada");
    }
    let name = format!(
        "{} (sequência)",
        first
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    );
    let mut item = base_item(name, MediaSource::ImageSequence { paths, fps });
    let mut imp = load_entry(&mut item)?;
    imp.item = item;
    Ok(imp)
}

#[cfg(windows)]
fn load_system_container(p: &Path, item: &mut MediaItem) -> Result<MediaEntry> {
    let loaded = mf::open_media(p)?;
    let audio = loaded.audio.map(Arc::new);
    item.has_audio = audio.is_some();
    if let Some(vr) = loaded.video {
        let vr = Arc::new(vr);
        item.has_video = true;
        item.width = vr.width;
        item.height = vr.height;
        item.fps = Some(vr.fps);
        let dur = loaded
            .duration
            .or_else(|| audio.as_ref().map(|a| a.duration()));
        item.duration = dur;
        item.info = format!(
            "Media Foundation {}x{} {}{}",
            vr.width,
            vr.height,
            vr.fps.label(),
            audio
                .as_ref()
                .map(|a| format!(", áudio {} Hz", a.sample_rate))
                .unwrap_or_default()
        );
        Ok(MediaEntry {
            video: VideoSource::Mf(vr, Mutex::new(FrameCache::new(16))),
            audio,
        })
    } else if let Some(a) = audio {
        let dur = loaded.duration.unwrap_or_else(|| a.duration());
        item.duration = Some(dur);
        item.info = format!(
            "Áudio (Media Foundation) {} Hz, {:.2} s",
            a.sample_rate,
            dur.as_secs_f64()
        );
        Ok(MediaEntry {
            video: VideoSource::None,
            audio: Some(a),
        })
    } else {
        bail!(
            "{}: contêiner sem fluxos legíveis via Media Foundation",
            item.name
        )
    }
}

// TODO(roadmap fase 4): backends nativos fora do Windows. Linux via VA-API
// (sem GStreamer, que é LGPL — AGENTS.md §2.3) e macOS via VideoToolbox/
// AVFoundation, ambos em fronteira FFI isolada, mais decodificador fallback em
// Rust puro. Até lá, este caminho devolve erro recuperável.
#[cfg(not(windows))]
fn load_system_container(_p: &Path, item: &mut MediaItem) -> Result<MediaEntry> {
    bail!(
        "{}: contêineres MP4/MOV/MKV/WEBM/M4V/M4A/AAC usam API nativa do sistema operacional \
         (Media Foundation no Windows); em outras plataformas utilize AVI (MJPEG/PCM), Y4M, GIF ou WAV/AIFF/FLAC/OGG/MP3",
        item.name
    )
}
