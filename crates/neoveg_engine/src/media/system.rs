//! Contrato multiplataforma para leitura de vídeo e áudio via APIs do
//! sistema operacional e fallback em Rust puro.
//!
//! O módulo seleciona o backend por plataforma em tempo de compilação
//! (`mf` no Windows, `vaapi` no Linux sem GStreamer/LGPL, `videotoolbox` no
//! macOS) com recuo automático para [`crate::media::fallback`] (Rust puro
//! sobre [`crate::media::demux`]) quando o backend de hardware do SO não
//! estiver disponível ou não suportar o codec (ex.: MKV FFV1/MJPEG/PNG/FLAC).

use crate::media::audio::AudioData;
use crate::model::{FrameRate, Ticks};
use anyhow::Result;
use image::RgbaImage;
use std::path::Path;
use std::sync::Arc;

/// Contrato comum de decodificação de vídeo em streaming servido por uma
/// API nativa do sistema operacional ou pelo decodificador fallback em Rust.
pub trait SystemVideoReader: Send + Sync {
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn fps(&self) -> FrameRate;
    fn decode_at(&self, t: Ticks) -> Result<RgbaImage>;
    fn decode_frame_at(&self, t: Ticks) -> Result<(RgbaImage, bool)> {
        Ok((self.decode_at(t)?, true))
    }
}

/// Resultado unificado de abertura de um contêiner via backend do sistema.
pub struct SystemLoaded {
    pub backend: &'static str,
    pub duration: Option<Ticks>,
    pub video: Option<Arc<dyn SystemVideoReader>>,
    pub audio: Option<AudioData>,
    /// O áudio foi cortado pelo limite de memória (`MAX_SAMPLES`).
    pub audio_truncated: bool,
}

/// Abre um arquivo de mídia usando o backend nativo da plataforma atual,
/// recorrendo ao decodificador fallback em Rust puro quando aplicável.
#[cfg(windows)]
pub fn open_media(path: &Path) -> Result<SystemLoaded> {
    match super::mf::open_media(path) {
        Ok(loaded) => Ok(SystemLoaded {
            backend: "Media Foundation",
            duration: loaded.duration,
            video: loaded
                .video
                .map(|vr| Arc::new(vr) as Arc<dyn SystemVideoReader>),
            audio: loaded.audio,
            audio_truncated: loaded.audio_truncated,
        }),
        Err(mf_err) => super::fallback::open_media(path).map_err(|fb_err| {
            anyhow::anyhow!("{mf_err}; fallback Rust puro também não abriu o contêiner: {fb_err}")
        }),
    }
}

/// Linux: tenta hardware VA-API (`libva` + `libva-drm` dinâmicos) e recua para Rust puro.
#[cfg(target_os = "linux")]
pub fn open_media(path: &Path) -> Result<SystemLoaded> {
    match super::vaapi::open_media(path) {
        Ok(loaded) => Ok(loaded),
        Err(va_err) => super::fallback::open_media(path).map_err(|fb_err| {
            anyhow::anyhow!("VA-API indisponível ({va_err}); fallback Rust puro: {fb_err}")
        }),
    }
}

/// macOS: tenta hardware VideoToolbox / AudioToolbox e recua para Rust puro.
#[cfg(target_os = "macos")]
pub fn open_media(path: &Path) -> Result<SystemLoaded> {
    match super::videotoolbox::open_media(path) {
        Ok(loaded) => Ok(loaded),
        Err(vt_err) => super::fallback::open_media(path).map_err(|fb_err| {
            anyhow::anyhow!("VideoToolbox indisponível ({vt_err}); fallback Rust puro: {fb_err}")
        }),
    }
}

/// Demais plataformas: utiliza o decodificador fallback em Rust puro.
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn open_media(path: &Path) -> Result<SystemLoaded> {
    super::fallback::open_media(path)
}
