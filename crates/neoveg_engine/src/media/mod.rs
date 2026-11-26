//! Gerenciamento de mídia: importação, decodificação e cache de quadros.

#[forbid(unsafe_code)]
pub mod audio;
#[forbid(unsafe_code)]
pub mod audio_extra;
#[forbid(unsafe_code)]
pub mod avi;
/// Fronteira FFI isolada para captura de tela, janelas, câmeras e espaço em disco (`AGENTS.md` §0.3).
pub(crate) mod capture_os;
/// Espaço livre e tamanho legível, usados também fora da captura (ex.: janela
/// de progresso da renderização).
pub use capture_os::{format_bytes_human, free_disk_bytes};
#[forbid(unsafe_code)]
pub mod convert;
#[forbid(unsafe_code)]
pub mod demux;
#[forbid(unsafe_code)]
pub mod fallback;
#[forbid(unsafe_code)]
pub mod ffv1;
#[forbid(unsafe_code)]
pub mod flac;
#[forbid(unsafe_code)]
pub mod generate;
#[forbid(unsafe_code)]
pub mod img_extra;
#[cfg(windows)]
pub(crate) mod mf;
#[forbid(unsafe_code)]
pub mod mkv;
#[forbid(unsafe_code)]
pub mod mov;
#[forbid(unsafe_code)]
mod system;
pub(crate) mod vaapi;
pub(crate) mod videotoolbox;
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

pub const IMAGE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp", "gif", "tga", "icb", "vda", "vst", "ico",
    "cur", "pbm", "pgm", "ppm", "pam", "hdr", "dds", "ff", "qoi", "exr", "dpx", "cin", "wbmp",
    "xbm", "xpm", "iff", "ilbm", "lbm",
];
pub const AUDIO_EXTS: &[&str] = &[
    "wav", "wave", "bwf", "rf64", "aif", "aiff", "aifc", "flac", "ogg", "oga", "mp3", "au", "snd",
    "caf", "w64", "voc", "8svx", "svx", "qoa", "mid", "midi",
];
pub const VIDEO_EXTS: &[&str] = &["avi", "y4m"];
/// Contêineres de mídia decodificados via API nativa do sistema operacional
/// (`media::system`, p. ex. Windows Media Foundation via `media::mf`).
pub const SYSTEM_CONTAINER_EXTS: &[&str] = &[
    "mp4", "mov", "mkv", "webm", "m4v", "m4a", "aac", "wmv", "asf", "wma", "3gp", "3g2", "m2ts",
    "mts", "ts", "mpg", "mpeg", "mpe", "vob", "mp2", "mpa", "m4b", "alac",
];
pub const UNSUPPORTED_CONTAINERS: &[&str] = &["mp4", "mov", "mkv", "webm", "m4v", "m4a", "aac"];

/// Memória máxima para os quadros de um GIF animado.
const MAX_GIF_BYTES: u64 = 512 << 20;
/// Profundidade máxima ao aninhar projetos `.neoveg` em outros projetos (§0.5).
pub const MAX_NESTED_PROJECT_DEPTH: usize = 8;
/// Duração máxima (segundos) pré-mixada em RAM ao carregar áudio de uma timeline aninhada (§0.4).
const MAX_NESTED_AUDIO_SECS: i64 = 900;

/// Estado decodificado em memória para uma linha do tempo aninhada (`.neoveg`).
pub struct NestedProjectEntry {
    pub project: Box<crate::model::Project>,
    pub media: Arc<MediaCache>,
    pub path: PathBuf,
}

/// Estado em memória de uma sequência interna aninhada como mídia (`MediaSource::Sequence`).
pub struct NestedSequenceEntry {
    pub sequence_id: Id,
    pub name: String,
    pub tracks: Vec<crate::model::Track>,
    pub settings: crate::model::ProjectSettings,
    /// Mídias do projeto pai necessárias para resolver os eventos dentro da sequência.
    pub project_media: Vec<crate::model::MediaItem>,
    pub audio_buses: Vec<crate::model::AudioBus>,
    pub master: crate::model::MasterBus,
    pub media: Arc<MediaCache>,
}

std::thread_local! {
    static NESTED_SEQ_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct NestedSeqDepthGuard;

impl NestedSeqDepthGuard {
    fn enter() -> Option<Self> {
        NESTED_SEQ_DEPTH.with(|c| {
            let cur = c.get();
            if cur >= crate::model::MAX_NESTED_SEQUENCE_DEPTH {
                None
            } else {
                c.set(cur + 1);
                Some(Self)
            }
        })
    }
}

impl Drop for NestedSeqDepthGuard {
    fn drop(&mut self) {
        NESTED_SEQ_DEPTH.with(|c| {
            c.set(c.get().saturating_sub(1));
        });
    }
}

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
    System(Arc<dyn system::SystemVideoReader>, Mutex<FrameCache>),
    Sequence(Vec<String>, FrameRate, Mutex<FrameCache>),
    Generated,
    /// Linha do tempo aninhada (`.neoveg`) com cache próprio de quadros renderizados.
    Nested(Arc<NestedProjectEntry>, Mutex<FrameCache>),
    /// Sequência interna do projeto aninhada como mídia (Premiere Pro ref. pp. 290–291).
    NestedSequence(Arc<NestedSequenceEntry>, Mutex<FrameCache>),
}

impl VideoSource {
    fn set_cache_budget(&self, cap: usize, max_bytes: usize) {
        match self {
            VideoSource::Avi(_, c)
            | VideoSource::Y4m(_, c)
            | VideoSource::System(_, c)
            | VideoSource::Sequence(_, _, c)
            | VideoSource::Nested(_, c)
            | VideoSource::NestedSequence(_, c) => {
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
            VideoSource::Avi(_, c)
            | VideoSource::Y4m(_, c)
            | VideoSource::System(_, c)
            | VideoSource::Sequence(_, _, c)
            | VideoSource::Nested(_, c)
            | VideoSource::NestedSequence(_, c) => {
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

    pub fn clone_entry(&self, id: Id) -> Option<Arc<MediaEntry>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&id)
            .cloned()
    }

    pub fn insert_arc(&self, id: Id, e: Arc<MediaEntry>) {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id, e);
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

    /// Sincroniza as entradas de mídia de sequências aninhadas (`MediaSource::Sequence`)
    /// garantindo que o vídeo e o áudio mixado em memória estejam disponíveis.
    pub fn sync_sequence_media(
        &self,
        project: &crate::model::Project,
        shared_cache: Arc<MediaCache>,
    ) {
        for m in &project.media {
            if let MediaSource::Sequence { id: seq_id } = m.source {
                let (tracks, settings, name) = if project.active_sequence_id == Some(seq_id) {
                    (
                        project.tracks.clone(),
                        project.settings.clone(),
                        project
                            .active_sequence_name
                            .clone()
                            .unwrap_or_else(|| "Sequência".into()),
                    )
                } else if let Some(s) = project.sequences.iter().find(|s| s.id == seq_id) {
                    (s.tracks.clone(), project.settings.clone(), s.name.clone())
                } else {
                    continue;
                };

                let has_audio = tracks.iter().any(|t| {
                    t.kind == crate::model::TrackKind::Audio && !t.is_folder && !t.events.is_empty()
                });
                let dur = m.duration.unwrap_or_else(|| {
                    tracks
                        .iter()
                        .flat_map(|t| &t.events)
                        .map(|e| e.end())
                        .fold(Ticks::ZERO, Ticks::max)
                });

                let audio = if has_audio {
                    let sr = settings.sample_rate.clamp(8000, 192_000);
                    let audio_dur = dur.min(Ticks::from_secs_i64(MAX_NESTED_AUDIO_SECS));
                    let total_samples = audio_dur.to_samples_round(sr).max(1) as usize;
                    let capped_samples = total_samples
                        .min((sr as usize).saturating_mul(MAX_NESTED_AUDIO_SECS as usize));
                    let sub_proj = crate::model::Project {
                        media: project.media.clone(),
                        tracks: tracks.clone(),
                        settings: settings.clone(),
                        audio_buses: project.audio_buses.clone(),
                        master: project.master.clone(),
                        active_sequence_id: Some(seq_id),
                        active_sequence_name: Some(name.clone()),
                        ..crate::model::Project::new_default()
                    };
                    let audio_map = shared_cache.audio_map();
                    let mut mixer = crate::engine::audio::Mixer::new_offline(sr);
                    let pdc = mixer.total_latency(&sub_proj) as i64;
                    let mut stereo = Vec::with_capacity(capped_samples.saturating_mul(2));
                    let mut pos = 0usize;
                    let chunk = 2048usize;
                    while pos < capped_samples {
                        let n = (capped_samples - pos).min(chunk);
                        let mut blk = vec![0.0f32; n * 2];
                        mixer.process(&sub_proj, &audio_map, pos as i64 + pdc, &mut blk);
                        stereo.extend_from_slice(&blk);
                        pos += n;
                    }
                    Some(Arc::new(AudioData::from_stereo(sr, stereo)))
                } else {
                    None
                };

                let entry = Arc::new(MediaEntry {
                    video: VideoSource::NestedSequence(
                        Arc::new(NestedSequenceEntry {
                            sequence_id: seq_id,
                            name,
                            tracks,
                            settings,
                            project_media: project.media.clone(),
                            audio_buses: project.audio_buses.clone(),
                            master: project.master.clone(),
                            media: shared_cache.clone(),
                        }),
                        Mutex::new(FrameCache::new(16)),
                    ),
                    audio,
                });

                self.entries
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(m.id, entry);
            }
        }
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
            VideoSource::System(reader, cache) => {
                let fps = item.fps.unwrap_or_else(|| reader.fps());
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
                    crate::util::guard("decodificação de vídeo do sistema", || {
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
                    crate::util::guard("decodificação de imagem", || {
                        img_extra::open_image_file(Path::new(path))
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
            VideoSource::Nested(nested, cache) => {
                let fps = item.fps.unwrap_or(nested.project.settings.fps);
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
                let rw = nested.project.settings.width.clamp(16, 1920) as usize;
                let rh = nested.project.settings.height.clamp(16, 1080) as usize;
                let ft = fps.frame_to_ticks(idx as i64);
                let frame = crate::engine::video::render_frame_preview(
                    &nested.project,
                    &nested.media,
                    ft,
                    rw,
                    rh,
                    true,
                );
                let img = Arc::new(frame.to_image()?);
                cache
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .put(idx, img.clone());
                Some(img)
            }
            VideoSource::NestedSequence(nested, cache) => {
                let _depth_guard = NestedSeqDepthGuard::enter()?;
                let fps = item.fps.unwrap_or(nested.settings.fps);
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
                let rw = nested.settings.width.clamp(16, 1920) as usize;
                let rh = nested.settings.height.clamp(16, 1080) as usize;
                let ft = fps.frame_to_ticks(idx as i64);
                let sub_proj = crate::model::Project {
                    media: nested.project_media.clone(),
                    tracks: nested.tracks.clone(),
                    settings: nested.settings.clone(),
                    audio_buses: nested.audio_buses.clone(),
                    master: nested.master.clone(),
                    active_sequence_id: Some(nested.sequence_id),
                    active_sequence_name: Some(nested.name.clone()),
                    ..crate::model::Project::new_default()
                };
                let frame = crate::engine::video::render_frame_preview(
                    &sub_proj,
                    &nested.media,
                    ft,
                    rw,
                    rh,
                    true,
                );
                let img = Arc::new(frame.to_image()?);
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
        if !item.can_build_proxy() {
            bail!(
                "mídia '{}' não é um clipe de vídeo elegível para gerar proxy",
                item.name
            );
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

    /// Miniatura em um instante específico (para *hover scrub* e prévias).
    pub fn thumbnail_at(&self, item: &MediaItem, t: Ticks, max: u32) -> Option<RgbaImage> {
        let (w, h) = if item.width > 0 && item.height > 0 {
            (item.width, item.height)
        } else {
            (1920, 1080)
        };
        let s = max.max(1) as f32 / w.max(h) as f32;
        let (tw, th) = (
            ((w as f32 * s) as u32).max(1),
            ((h as f32 * s) as u32).max(1),
        );
        let sample_t = match item.duration {
            Some(d) if d > Ticks::ZERO => {
                let end = (d - Ticks::from_millis_i64(1)).max(Ticks::ZERO);
                t.clamp_safe(Ticks::ZERO, end)
            }
            _ => t.max(Ticks::ZERO),
        };
        let f = self.video_frame(item, sample_t, tw, th)?;
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

    /// Miniatura padrão (para a janela de mídia do projeto e eventos).
    pub fn thumbnail(&self, item: &MediaItem, max: u32) -> Option<RgbaImage> {
        let t = item
            .duration
            .map(|d| d.div_i64(10).min(Ticks::from_secs_i64(1)))
            .unwrap_or(Ticks::ZERO);
        self.thumbnail_at(item, t, max)
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

/// Importa um projeto `.neoveg` como linha do tempo aninhada (manual p. 114).
pub fn import_nested_project(path: &Path) -> Result<Imported> {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Projeto aninhado".into());
    let mut item = base_item(
        name,
        MediaSource::NestedProject {
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
    let mut stack = std::collections::HashSet::new();
    let empty_overrides = std::collections::HashMap::new();
    load_entry_with_stack(item, &mut stack, 0, &empty_overrides)
}

fn load_entry_with_stack(
    item: &mut MediaItem,
    stack: &mut std::collections::HashSet<PathBuf>,
    depth: usize,
    overrides: &std::collections::HashMap<PathBuf, crate::model::Project>,
) -> Result<Imported> {
    match crate::util::guard("importação de mídia", || {
        load_entry_inner(item, stack, depth, overrides)
    }) {
        Ok(r) => r,
        Err(e) => bail!("{e}"),
    }
}

fn load_entry_inner(
    item: &mut MediaItem,
    stack: &mut std::collections::HashSet<PathBuf>,
    depth: usize,
    overrides: &std::collections::HashMap<PathBuf, crate::model::Project>,
) -> Result<Imported> {
    let mut warning = None;
    let entry = match item.source.clone() {
        MediaSource::Generated(_) => {
            item.has_video = true;
            MediaEntry {
                video: VideoSource::Generated,
                audio: None,
            }
        }
        MediaSource::Sequence { id: seq_id } => {
            let dur = item.duration.unwrap_or_else(|| Ticks::from_secs_i64(10));
            item.has_video = true;
            item.has_audio = true;
            item.duration = Some(dur);
            item.info = format!("Sequência aninhada (ID: {seq_id})");
            MediaEntry {
                video: VideoSource::NestedSequence(
                    Arc::new(NestedSequenceEntry {
                        sequence_id: seq_id,
                        name: item.name.clone(),
                        tracks: Vec::new(),
                        settings: crate::model::ProjectSettings::default(),
                        project_media: Vec::new(),
                        audio_buses: Vec::new(),
                        master: crate::model::MasterBus::default(),
                        media: Arc::new(MediaCache::default()),
                    }),
                    Mutex::new(FrameCache::new(16)),
                ),
                audio: None,
            }
        }
        MediaSource::ImageSequence { paths, fps } => {
            if paths.is_empty() {
                bail!("sequência vazia");
            }
            let Some(first_path) = paths.first() else {
                bail!("sequência vazia")
            };
            let first = img_extra::open_image_file(Path::new(first_path))?;
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
            } else if ext == "webp" {
                load_webp(&p, item)?
            } else if IMAGE_EXTS.contains(&ext.as_str()) {
                let img = img_extra::open_image_file(&p)?;
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
            } else if ext == "mid" || ext == "midi" {
                let clip = crate::midi::load_smf_file(&p).map_err(anyhow::Error::msg)?;
                let notes_n = clip.notes.len();
                let dur = clip.duration;
                let a = AudioData::from_midi(clip, 48_000);
                item.has_audio = true;
                item.duration = Some(dur);
                item.info = format!("MIDI ({notes_n} notas), {:.2} s", dur.as_secs_f64());
                MediaEntry {
                    video: VideoSource::None,
                    audio: Some(Arc::new(a)),
                }
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
        MediaSource::NestedProject { path } => {
            if depth >= MAX_NESTED_PROJECT_DEPTH {
                bail!(
                    "profundidade máxima de projetos aninhados ({MAX_NESTED_PROJECT_DEPTH}) excedida"
                );
            }
            let p = PathBuf::from(&path);
            let canon = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
            if !p.exists() && !overrides.contains_key(&canon) {
                bail!("projeto aninhado não encontrado: {path}");
            }
            if !stack.insert(canon.clone()) {
                bail!("referência cíclica detectada no projeto aninhado: {path}");
            }
            let res =
                load_nested_project_entry(&p, item, stack, depth + 1, &mut warning, overrides);
            stack.remove(&canon);
            res?
        }
    };
    Ok(Imported {
        item: item.clone(),
        entry,
        warning,
    })
}

fn load_nested_project_entry(
    p: &Path,
    item: &mut MediaItem,
    stack: &mut std::collections::HashSet<PathBuf>,
    next_depth: usize,
    warning: &mut Option<String>,
    overrides: &std::collections::HashMap<PathBuf, crate::model::Project>,
) -> Result<MediaEntry> {
    let canon = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let sub_proj = if let Some(mem_proj) = overrides.get(&canon) {
        mem_proj.clone()
    } else {
        crate::model::Project::load_file(p)
            .map_err(anyhow::Error::msg)?
            .0
    };
    nested_entry_from_project(sub_proj, p, item, stack, next_depth, warning, overrides)
}

/// Monta a mídia de um projeto aninhado a partir do projeto já em memória
/// (o projeto aberto e editado em aba, ainda sem salvar). `path` é o arquivo
/// dele: os caminhos relativos da mídia são resolvidos a partir da pasta dele,
/// numa cópia (o projeto da aba não muda).
pub fn load_nested_from_project(
    item: &mut MediaItem,
    project: crate::model::Project,
    path: &Path,
) -> Result<Imported> {
    let empty_overrides = std::collections::HashMap::new();
    load_nested_from_project_with_overrides(item, project, path, &empty_overrides)
}

/// Variante de [`load_nested_from_project`] que também aceita projetos netos
/// abertos em abas em memória (`overrides`, chaveado por caminho canônico),
/// propagando edições de aninhados dentro de aninhados antes de salvar.
pub fn load_nested_from_project_with_overrides(
    item: &mut MediaItem,
    project: crate::model::Project,
    path: &Path,
    overrides: &std::collections::HashMap<PathBuf, crate::model::Project>,
) -> Result<Imported> {
    let mut stack = std::collections::HashSet::new();
    let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    stack.insert(canon);
    let mut warning = None;
    let entry =
        nested_entry_from_project(project, path, item, &mut stack, 1, &mut warning, overrides)?;
    Ok(Imported {
        item: item.clone(),
        entry,
        warning,
    })
}

fn nested_entry_from_project(
    mut sub_proj: crate::model::Project,
    p: &Path,
    item: &mut MediaItem,
    stack: &mut std::collections::HashSet<PathBuf>,
    next_depth: usize,
    warning: &mut Option<String>,
    overrides: &std::collections::HashMap<PathBuf, crate::model::Project>,
) -> Result<MediaEntry> {
    let base_dir = p.parent().unwrap_or_else(|| Path::new("."));
    let sub_cache = Arc::new(MediaCache::default());
    let mut sub_warnings = Vec::new();
    for sm in &mut sub_proj.media {
        if let MediaSource::File { path: sp } | MediaSource::NestedProject { path: sp } =
            &mut sm.source
        {
            let sp_path = Path::new(sp.as_str());
            if sp_path.is_relative() {
                let candidate = base_dir.join(sp_path);
                if candidate.exists() {
                    *sp = candidate.to_string_lossy().to_string();
                }
            }
        }
        match load_entry_with_stack(sm, stack, next_depth, overrides) {
            Ok(imp) => {
                if let Some(w) = imp.warning {
                    sub_warnings.push(w);
                }
                sub_cache.insert(sm.id, imp.entry);
            }
            Err(e) => {
                sub_warnings.push(format!("{}: {e}", sm.name));
            }
        }
    }
    let dur = sub_proj.duration().max(sub_proj.settings.frame_dur());
    let has_video = sub_proj.tracks.iter().any(|t| {
        t.kind == crate::model::TrackKind::Video
            && !t.is_folder
            && !t.events.is_empty()
            && !sub_proj.is_track_effectively_muted(t.id)
    });
    let has_audio_events = sub_proj.tracks.iter().any(|t| {
        t.kind == crate::model::TrackKind::Audio
            && !t.is_folder
            && !t.events.is_empty()
            && !sub_proj.is_track_effectively_muted(t.id)
    });
    let audio = if has_audio_events {
        let sr = sub_proj.settings.sample_rate.clamp(8000, 192_000);
        let audio_dur = dur.min(Ticks::from_secs_i64(MAX_NESTED_AUDIO_SECS));
        let total_samples = audio_dur.to_samples_round(sr).max(1) as usize;
        let capped_samples =
            total_samples.min((sr as usize).saturating_mul(MAX_NESTED_AUDIO_SECS as usize));
        let audio_map = sub_cache.audio_map();
        let mut mixer = crate::engine::audio::Mixer::new_offline(sr);
        let pdc = mixer.total_latency(&sub_proj) as i64;
        let mut stereo = Vec::with_capacity(capped_samples.saturating_mul(2));
        let mut pos = 0usize;
        let chunk = 2048usize;
        while pos < capped_samples {
            let n = (capped_samples - pos).min(chunk);
            let mut blk = vec![0.0f32; n * 2];
            mixer.process(&sub_proj, &audio_map, pos as i64 + pdc, &mut blk);
            stereo.extend_from_slice(&blk);
            pos += n;
        }
        Some(Arc::new(AudioData::from_stereo(sr, stereo)))
    } else {
        None
    };

    item.width = sub_proj.settings.width;
    item.height = sub_proj.settings.height;
    item.fps = Some(sub_proj.settings.fps);
    item.duration = Some(dur);
    item.has_video = has_video || !has_audio_events;
    item.has_audio = audio.is_some();
    item.info = format!(
        "Projeto aninhado {}x{}, {}, {:.2} s",
        item.width,
        item.height,
        sub_proj.settings.fps.label(),
        dur.as_secs_f64()
    );
    if !sub_warnings.is_empty() {
        *warning = Some(sub_warnings.join(" | "));
    }
    let nested = Arc::new(NestedProjectEntry {
        project: Box::new(sub_proj),
        media: sub_cache,
        path: p.to_path_buf(),
    });
    Ok(MediaEntry {
        video: VideoSource::Nested(nested, Mutex::new(FrameCache::new(16))),
        audio,
    })
}

fn collect_animated_frames(
    frames: image::Frames<'_>,
    item: &mut MediaItem,
    label: &str,
) -> Result<MediaEntry> {
    let mut imgs = vec![];
    let mut times = vec![];
    let mut t = Ticks::ZERO;
    let mut total_bytes: u64 = 0;
    for fr in frames {
        let fr = fr?;
        total_bytes += fr.buffer().as_raw().len() as u64;
        if total_bytes > MAX_GIF_BYTES {
            bail!("{label} animado grande demais para a memória");
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
        bail!("{label} sem quadros")
    };
    item.width = first.width();
    item.height = first.height();
    item.has_video = true;
    if imgs.len() == 1 {
        item.duration = None;
        item.info = format!("{label} {}x{}", item.width, item.height);
        return Ok(MediaEntry {
            video: VideoSource::Still(imgs.remove(0)),
            audio: None,
        });
    }
    item.duration = Some(t);
    item.fps = FrameRate::from_f64(imgs.len() as f64 / t.as_secs_f64().max(1e-3));
    item.info = format!(
        "{label} animado {}x{}, {} quadros, {:.2} s",
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
    collect_animated_frames(dec.into_frames(), item, "GIF")
}

fn load_webp(p: &Path, item: &mut MediaItem) -> Result<MediaEntry> {
    use image::AnimationDecoder;
    let f = std::io::BufReader::new(std::fs::File::open(p)?);
    let mut dec = image::codecs::webp::WebPDecoder::new(f)?;
    let (w, h) = image::ImageDecoder::dimensions(&dec);
    if w == 0 || h == 0 || w > crate::model::MAX_DIM || h > crate::model::MAX_DIM {
        bail!("dimensões WebP inválidas: {w}x{h}");
    }
    if dec.has_animation() {
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(crate::model::MAX_DIM);
        limits.max_image_height = Some(crate::model::MAX_DIM);
        limits.max_alloc = Some(512 << 20);
        image::ImageDecoder::set_limits(&mut dec, limits)?;
        return collect_animated_frames(dec.into_frames(), item, "WebP");
    }
    let img = img_extra::open_image_file(p)?;
    item.width = img.width();
    item.height = img.height();
    item.has_video = true;
    item.duration = None;
    item.info = format!("WebP {}x{}", img.width(), img.height());
    Ok(MediaEntry {
        video: VideoSource::Still(Arc::new(img)),
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

fn load_system_container(p: &Path, item: &mut MediaItem) -> Result<MediaEntry> {
    let loaded = system::open_media(p)?;
    let backend = loaded.backend;
    let audio = loaded.audio.map(Arc::new);
    item.has_audio = audio.is_some();
    if let Some(vr) = loaded.video {
        let w = vr.width();
        let h = vr.height();
        let fps = vr.fps();
        item.has_video = true;
        item.width = w;
        item.height = h;
        item.fps = Some(fps);
        let dur = loaded
            .duration
            .or_else(|| audio.as_ref().map(|a| a.duration()));
        item.duration = dur;
        item.info = format!(
            "{backend} {w}x{h} {}{}",
            fps.label(),
            audio
                .as_ref()
                .map(|a| format!(", áudio {} Hz", a.sample_rate))
                .unwrap_or_default()
        );
        Ok(MediaEntry {
            video: VideoSource::System(vr, Mutex::new(FrameCache::new(16))),
            audio,
        })
    } else if let Some(a) = audio {
        let dur = loaded.duration.unwrap_or_else(|| a.duration());
        item.duration = Some(dur);
        item.info = format!(
            "Áudio ({backend}) {} Hz, {:.2} s",
            a.sample_rate,
            dur.as_secs_f64()
        );
        Ok(MediaEntry {
            video: VideoSource::None,
            audio: Some(a),
        })
    } else {
        bail!("{}: contêiner sem fluxos legíveis via {backend}", item.name)
    }
}

#[cfg(test)]
mod system_contract_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockSystemReader {
        calls: Arc<AtomicUsize>,
        panic_on_decode: bool,
    }

    impl system::SystemVideoReader for MockSystemReader {
        fn width(&self) -> u32 {
            16
        }
        fn height(&self) -> u32 {
            16
        }
        fn fps(&self) -> FrameRate {
            FrameRate::FPS_25
        }
        fn decode_at(&self, t: Ticks) -> Result<RgbaImage> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.panic_on_decode {
                panic!("falha simulada no decodificador do sistema");
            }
            let lum = ((t.0 / 1_000_000) % 255) as u8;
            Ok(RgbaImage::from_pixel(
                16,
                16,
                image::Rgba([lum, 10, 20, 255]),
            ))
        }
    }

    #[test]
    fn system_video_source_caches_budgets_and_guards_panics() {
        let cache = MediaCache::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let reader: Arc<dyn system::SystemVideoReader> = Arc::new(MockSystemReader {
            calls: calls.clone(),
            panic_on_decode: false,
        });
        let id = new_id();
        cache.insert(
            id,
            MediaEntry {
                video: VideoSource::System(reader, Mutex::new(FrameCache::new(4))),
                audio: None,
            },
        );
        let item = MediaItem {
            id,
            name: "mock.mp4".into(),
            source: MediaSource::File {
                path: "mock.mp4".into(),
            },
            duration: Some(FrameRate::FPS_25.frame_to_ticks(10)),
            has_video: true,
            has_audio: false,
            width: 16,
            height: 16,
            fps: Some(FrameRate::FPS_25),
            info: String::new(),
            proxy_path: None,
        };
        assert!(cache.video_frame(&item, Ticks::ZERO, 16, 16).is_some());
        assert!(cache.video_frame(&item, Ticks::ZERO, 16, 16).is_some());
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        cache.configure_limits(2, 0);
        cache.clear_frame_caches();

        let bad_id = new_id();
        let bad_reader: Arc<dyn system::SystemVideoReader> = Arc::new(MockSystemReader {
            calls: Arc::new(AtomicUsize::new(0)),
            panic_on_decode: true,
        });
        cache.insert(
            bad_id,
            MediaEntry {
                video: VideoSource::System(bad_reader, Mutex::new(FrameCache::new(4))),
                audio: None,
            },
        );
        let mut bad_item = item;
        bad_item.id = bad_id;
        assert!(cache.video_frame(&bad_item, Ticks::ZERO, 16, 16).is_none());
    }

    #[test]
    fn projeto_aninhado_carrega_quadros_e_bloqueia_ciclos() {
        let dir = std::env::temp_dir().join(format!("neoveg_nested_test_{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sub_path = dir.join("sub.neoveg");
        let cyc_a = dir.join("cyc_a.neoveg");
        let cyc_b = dir.join("cyc_b.neoveg");

        let mut sub = crate::model::Project::new_default();
        sub.settings.width = 64;
        sub.settings.height = 36;
        let gm = import_generated(GenSpec::SolidColor {
            color: [1.0, 0.0, 0.0, 1.0],
        })
        .item;
        let ev = crate::model::Event::new(&gm, Ticks::ZERO, Ticks::from_secs_i64(2));
        sub.media.push(gm);
        sub.tracks[0].events.push(ev);
        sub.save_file(&sub_path).unwrap();

        let imp = import_nested_project(&sub_path).expect("importa projeto aninhado válido");
        let cache = MediaCache::default();
        let item = imp.item.clone();
        assert!(item.has_video);
        cache.insert(item.id, imp.entry);
        let frame = cache
            .video_frame(&item, Ticks::from_secs_i64(1), 64, 36)
            .expect("decodifica quadro aninhado");
        assert_eq!(frame.get_pixel(32, 18).0[0], 255);

        // Ciclo A -> B -> A deve falhar com erro controlado (zero estouro de pilha, §0.5).
        let mut pa = crate::model::Project::new_default();
        let ma_b = base_item(
            "B".into(),
            MediaSource::NestedProject {
                path: cyc_b.to_string_lossy().to_string(),
            },
        );
        pa.media.push(ma_b);
        pa.save_file(&cyc_a).unwrap();

        let mut pb = crate::model::Project::new_default();
        let mb_a = base_item(
            "A".into(),
            MediaSource::NestedProject {
                path: cyc_a.to_string_lossy().to_string(),
            },
        );
        pb.media.push(mb_a);
        pb.save_file(&cyc_b).unwrap();

        let mut root_item = base_item(
            "Ciclo".into(),
            MediaSource::NestedProject {
                path: cyc_a.to_string_lossy().to_string(),
            },
        );
        let res = load_entry(&mut root_item);
        assert!(res.is_ok());
        assert!(res.unwrap().warning.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn projeto_aninhado_neto_usa_override_em_memoria() {
        let dir = std::env::temp_dir().join(format!("neoveg_grandchild_test_{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let neto_path = dir.join("neto.neoveg");
        let filho_path = dir.join("filho.neoveg");

        // Neto no disco é vermelho
        let mut neto = crate::model::Project::new_default();
        neto.settings.width = 32;
        neto.settings.height = 18;
        let red_m = import_generated(GenSpec::SolidColor {
            color: [1.0, 0.0, 0.0, 1.0],
        })
        .item;
        let red_ev = crate::model::Event::new(&red_m, Ticks::ZERO, Ticks::from_secs_i64(2));
        neto.media.push(red_m);
        neto.tracks[0].events.push(red_ev);
        neto.save_file(&neto_path).unwrap();

        // Filho referencia o neto
        let mut filho = crate::model::Project::new_default();
        filho.settings.width = 32;
        filho.settings.height = 18;
        let neto_item = base_item(
            "neto.neoveg".into(),
            MediaSource::NestedProject {
                path: neto_path.to_string_lossy().to_string(),
            },
        );
        let filho_ev = crate::model::Event::new(&neto_item, Ticks::ZERO, Ticks::from_secs_i64(2));
        filho.media.push(neto_item);
        filho.tracks[0].events.push(filho_ev);
        filho.save_file(&filho_path).unwrap();

        // Edição em memória no neto (sem salvar no disco): verde
        let mut neto_editado = neto.clone();
        neto_editado.media[0].source = MediaSource::Generated(GenSpec::SolidColor {
            color: [0.0, 1.0, 0.0, 1.0],
        });
        let canon_neto = std::fs::canonicalize(&neto_path).unwrap_or_else(|_| neto_path.clone());
        let mut overrides = std::collections::HashMap::new();
        overrides.insert(canon_neto, neto_editado);

        let mut filho_item = base_item(
            "filho.neoveg".into(),
            MediaSource::NestedProject {
                path: filho_path.to_string_lossy().to_string(),
            },
        );
        let imp = load_nested_from_project_with_overrides(
            &mut filho_item,
            filho,
            &filho_path,
            &overrides,
        )
        .expect("recarrega filho usando neto editado em memória");
        let cache = MediaCache::default();
        cache.insert(filho_item.id, imp.entry);
        let frame = cache
            .video_frame(&filho_item, Ticks::from_secs_i64(1), 32, 18)
            .expect("renderiza quadro do filho");
        let px = frame.get_pixel(16, 9).0;
        assert_eq!(px[0], 0);
        assert_eq!(px[1], 255);
        assert_eq!(px[2], 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sequencia_aninhada_renderiza_midia_interna_sem_tela_preta() {
        let mut p = crate::model::Project::new_default();
        p.settings.width = 32;
        p.settings.height = 18;
        let red_imp = import_generated(GenSpec::SolidColor {
            color: [1.0, 0.0, 0.0, 1.0],
        });
        let red_m = red_imp.item;
        let red_ev = crate::model::Event::new(&red_m, Ticks::ZERO, Ticks::from_secs_i64(4));
        let red_ev_id = red_ev.id;
        p.media.push(red_m);
        p.tracks[0].events.push(red_ev);

        let mut session = crate::session::EditorSession::new(p);
        session.sel_events.insert(red_ev_id);
        let _seq_id = session
            .nest_selection(Some("Sequência 02"))
            .expect("aninhamento da seleção");

        let cache = Arc::new(MediaCache::default());
        cache.sync_sequence_media(&session.project, cache.clone());

        let frame = crate::engine::video::render_frame_preview(
            &session.project,
            &cache,
            Ticks::from_secs_i64(1),
            32,
            18,
            false,
        );
        let px = frame.px[9 * 32 + 16];
        assert!(
            px[0] > 0.9 && px[1] < 0.05 && px[2] < 0.05,
            "prévia de sequência aninhada não deve ficar preta; pixel={px:?}"
        );
    }
}
