//! Gerenciamento de mídia: importação, decodificação e cache de quadros.

pub mod audio;
pub mod avi;
pub mod generate;
pub mod y4m;

use crate::model::{new_id, GenSpec, Id, MediaItem, MediaSource};
use anyhow::{bail, Result};
pub use audio::AudioData;
use image::RgbaImage;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const IMAGE_EXTS: [&str; 8] = ["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp", "gif"];
pub const AUDIO_EXTS: [&str; 12] = ["wav", "mp3", "flac", "ogg", "oga", "m4a", "aac", "aif", "aiff", "caf", "mka", "wv"];
pub const VIDEO_EXTS: [&str; 2] = ["avi", "y4m"];
/// Contêineres cujo vídeo (H.264/H.265/VP9...) não possui decodificador maduro
/// em Rust puro: importamos apenas o áudio.
pub const AUDIO_ONLY_CONTAINERS: [&str; 5] = ["mp4", "mov", "mkv", "webm", "m4v"];

pub fn is_supported(path: &Path) -> bool {
    let ext = ext_of(path);
    IMAGE_EXTS.contains(&ext.as_str())
        || AUDIO_EXTS.contains(&ext.as_str())
        || VIDEO_EXTS.contains(&ext.as_str())
        || AUDIO_ONLY_CONTAINERS.contains(&ext.as_str())
}

fn ext_of(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

pub struct FrameCache {
    items: VecDeque<(usize, Arc<RgbaImage>)>,
    cap: usize,
}

impl FrameCache {
    fn new(cap: usize) -> Self {
        FrameCache {
            items: VecDeque::new(),
            cap,
        }
    }
    fn get(&mut self, k: usize) -> Option<Arc<RgbaImage>> {
        let pos = self.items.iter().position(|(i, _)| *i == k)?;
        let item = self.items.remove(pos)?;
        let img = item.1.clone();
        self.items.push_front(item);
        Some(img)
    }
    fn put(&mut self, k: usize, img: Arc<RgbaImage>) {
        self.items.push_front((k, img));
        while self.items.len() > self.cap {
            self.items.pop_back();
        }
    }
}

pub enum VideoSource {
    None,
    Still(Arc<RgbaImage>),
    /// Quadros pré-decodificados (GIF animado) com tempos de início.
    Frames(Vec<Arc<RgbaImage>>, Vec<f64>, f64),
    Avi(Mutex<avi::AviReader>, Mutex<FrameCache>),
    Y4m(Mutex<y4m::Y4mReader>, Mutex<FrameCache>),
    Sequence(Vec<String>, f64, Mutex<FrameCache>),
    Generated,
}

pub struct MediaEntry {
    pub video: VideoSource,
    pub audio: Option<Arc<AudioData>>,
}

/// Cache compartilhado entre interface, prévia, reprodução e renderização.
#[derive(Default)]
pub struct MediaCache {
    entries: Mutex<HashMap<Id, Arc<MediaEntry>>>,
    generated: Mutex<HashMap<(Id, u32, u32, u64), Arc<RgbaImage>>>,
    pub fonts: generate::Fonts,
}

impl MediaCache {
    pub fn insert(&self, id: Id, e: MediaEntry) {
        self.entries.lock().unwrap().insert(id, Arc::new(e));
    }

    pub fn remove(&self, id: Id) {
        self.entries.lock().unwrap().remove(&id);
    }

    pub fn contains(&self, id: Id) -> bool {
        self.entries.lock().unwrap().contains_key(&id)
    }

    fn entry(&self, id: Id) -> Option<Arc<MediaEntry>> {
        self.entries.lock().unwrap().get(&id).cloned()
    }

    pub fn audio(&self, id: Id) -> Option<Arc<AudioData>> {
        self.entry(id).and_then(|e| e.audio.clone())
    }

    pub fn audio_map(&self) -> HashMap<Id, Arc<AudioData>> {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(k, e)| e.audio.clone().map(|a| (*k, a)))
            .collect()
    }

    /// Limpa o cache de mídia gerada de um item (após edição).
    pub fn invalidate_generated(&self, id: Id) {
        self.generated.lock().unwrap().retain(|k, _| k.0 != id);
    }

    /// Quadro da mídia no tempo de mídia `t`. `w`,`h` são usados por mídia
    /// gerada (que é produzida diretamente na resolução de saída).
    pub fn video_frame(&self, item: &MediaItem, t: f64, w: u32, h: u32) -> Option<Arc<RgbaImage>> {
        if let MediaSource::Generated(spec) = &item.source {
            return Some(self.generated_frame(item.id, spec, t, w, h));
        }
        let e = self.entry(item.id)?;
        match &e.video {
            VideoSource::None | VideoSource::Generated => None,
            VideoSource::Still(img) => Some(img.clone()),
            VideoSource::Frames(frames, times, _) => {
                let i = match times.binary_search_by(|x| x.partial_cmp(&t).unwrap()) {
                    Ok(i) => i,
                    Err(i) => i.saturating_sub(1),
                };
                frames.get(i.min(frames.len() - 1)).cloned()
            }
            VideoSource::Avi(r, cache) => {
                let mut rd = r.lock().unwrap();
                let idx = ((t * rd.fps + 1e-6) as usize).min(rd.frame_count().saturating_sub(1));
                if let Some(img) = cache.lock().unwrap().get(idx) {
                    return Some(img);
                }
                let img = Arc::new(rd.decode_frame(idx).ok()?);
                cache.lock().unwrap().put(idx, img.clone());
                Some(img)
            }
            VideoSource::Y4m(r, cache) => {
                let mut rd = r.lock().unwrap();
                let idx = ((t * rd.fps + 1e-6) as usize).min(rd.frames.saturating_sub(1));
                if let Some(img) = cache.lock().unwrap().get(idx) {
                    return Some(img);
                }
                let img = Arc::new(rd.decode_frame(idx).ok()?);
                cache.lock().unwrap().put(idx, img.clone());
                Some(img)
            }
            VideoSource::Sequence(paths, fps, cache) => {
                let idx = ((t * fps + 1e-6) as usize).min(paths.len().saturating_sub(1));
                if let Some(img) = cache.lock().unwrap().get(idx) {
                    return Some(img);
                }
                let img = Arc::new(image::open(&paths[idx]).ok()?.to_rgba8());
                cache.lock().unwrap().put(idx, img.clone());
                Some(img)
            }
        }
    }

    fn generated_frame(&self, id: Id, spec: &GenSpec, t: f64, w: u32, h: u32) -> Arc<RgbaImage> {
        let tk = if spec.is_animated() { (t * 1000.0).round() as u64 } else { 0 };
        let key = (id, w, h, tk);
        if let Some(img) = self.generated.lock().unwrap().get(&key) {
            return img.clone();
        }
        let img = Arc::new(generate::render(spec, t, w, h, &self.fonts));
        let mut g = self.generated.lock().unwrap();
        if g.len() > 64 {
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
        let (tw, th) = (((w as f32 * s) as u32).max(1), ((h as f32 * s) as u32).max(1));
        let t = item.duration.map(|d| (d * 0.1).min(1.0)).unwrap_or(0.0);
        let f = self.video_frame(item, t, tw, th)?;
        if f.width() == tw && f.height() == th {
            return Some((*f).clone());
        }
        Some(image::imageops::resize(&*f, tw, th, image::imageops::FilterType::Triangle))
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
        fps: 0.0,
        info: String::new(),
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
    let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let mut item = base_item(name, MediaSource::File {
        path: path.to_string_lossy().to_string(),
    });
    let mut imp = load_entry(&mut item)?;
    imp.item = item;
    Ok(imp)
}

/// Carrega (ou recarrega, ao abrir um projeto) os dados de um item de mídia.
/// Atualiza as informações do item (duração, dimensões...).
pub fn load_entry(item: &mut MediaItem) -> Result<Imported> {
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
            let first = image::open(&paths[0])?;
            item.width = first.width();
            item.height = first.height();
            item.fps = fps;
            item.has_video = true;
            item.duration = Some(paths.len() as f64 / fps);
            item.info = format!("Sequência de {} imagens, {}x{}, {:.3} qps", paths.len(), item.width, item.height, fps);
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
                    Some((sr, ch, s)) if !s.is_empty() => Some(Arc::new(AudioData::from_interleaved(sr, ch as usize, &s))),
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
                item.fps = rd.fps;
                let vd = if item.has_video { Some(rd.duration()) } else { None };
                let ad = audio.as_ref().map(|a| a.duration());
                item.duration = vd.or(ad);
                if !item.has_video && !item.has_audio {
                    bail!("{}", warning.take().unwrap_or_else(|| "AVI sem fluxos suportados".into()));
                }
                item.info = format!(
                    "AVI {} {}x{} {:.3} qps, {} quadros{}",
                    rd.video_fourcc,
                    rd.width,
                    rd.height,
                    rd.fps,
                    rd.frame_count(),
                    audio.as_ref().map(|a| format!(", áudio {} Hz", a.sample_rate)).unwrap_or_default()
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
                item.fps = rd.fps;
                item.duration = Some(rd.duration());
                item.info = format!("Y4M {}x{} {:.3} qps, {} quadros", rd.width, rd.height, rd.fps, rd.frames);
                MediaEntry {
                    video: VideoSource::Y4m(Mutex::new(rd), Mutex::new(FrameCache::new(12))),
                    audio: None,
                }
            } else {
                // áudio (ou contêiner do qual só extraímos o áudio)
                let a = audio::decode_file(&p)?;
                if AUDIO_ONLY_CONTAINERS.contains(&ext.as_str()) {
                    warning = Some(format!(
                        "{}: apenas o áudio foi importado. Não há decodificador H.264/H.265 em Rust puro; \
                         converta o vídeo para AVI MJPEG (ex.: ffmpeg -i entrada.mp4 -c:v mjpeg -q:v 3 -c:a pcm_s16le saida.avi)",
                        item.name
                    ));
                }
                item.has_audio = true;
                item.duration = Some(a.duration());
                item.info = format!("Áudio {} Hz, {:.2} s", a.sample_rate, a.duration());
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
    let dec = image::codecs::gif::GifDecoder::new(f)?;
    let frames = dec.into_frames().collect_frames()?;
    if frames.is_empty() {
        bail!("GIF sem quadros");
    }
    let mut imgs = vec![];
    let mut times = vec![];
    let mut t = 0.0;
    for fr in frames {
        let (n, d) = fr.delay().numer_denom_ms();
        let ms = if d == 0 { 100.0 } else { n as f64 / d as f64 };
        let ms = if ms < 10.0 { 100.0 } else { ms };
        times.push(t / 1000.0);
        t += ms;
        imgs.push(Arc::new(fr.into_buffer()));
    }
    item.width = imgs[0].width();
    item.height = imgs[0].height();
    item.has_video = true;
    if imgs.len() == 1 {
        item.duration = None;
        item.info = format!("GIF {}x{}", item.width, item.height);
        return Ok(MediaEntry {
            video: VideoSource::Still(imgs.remove(0)),
            audio: None,
        });
    }
    let total = t / 1000.0;
    item.duration = Some(total);
    item.fps = imgs.len() as f64 / total;
    item.info = format!("GIF animado {}x{}, {} quadros, {:.2} s", item.width, item.height, imgs.len(), total);
    Ok(MediaEntry {
        video: VideoSource::Frames(imgs, times, total),
        audio: None,
    })
}

/// Detecta uma sequência numerada a partir do primeiro arquivo
/// (ex.: quadro_0001.png, quadro_0002.png, ...).
pub fn detect_sequence(first: &Path) -> Vec<String> {
    let Some(stem) = first.file_stem().and_then(|s| s.to_str()) else { return vec![] };
    let ext = first.extension().and_then(|s| s.to_str()).unwrap_or("");
    let digits: String = stem.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
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

pub fn import_sequence(first: &Path, fps: f64) -> Result<Imported> {
    let paths = detect_sequence(first);
    if paths.is_empty() {
        bail!("nenhuma imagem encontrada");
    }
    let name = format!(
        "{} (sequência)",
        first.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
    );
    let mut item = base_item(name, MediaSource::ImageSequence { paths, fps });
    let mut imp = load_entry(&mut item)?;
    imp.item = item;
    Ok(imp)
}
