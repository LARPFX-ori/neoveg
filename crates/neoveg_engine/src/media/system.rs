//! Contrato multiplataforma para leitura de vídeo e áudio via APIs do
//! sistema operacional (Issue #11 — Etapa 0).
//!
//! O módulo seleciona o backend por plataforma em tempo de compilação
//! (`mf` no Windows; próximos backends: `avf` no macOS e `vaapi` no Linux)
//! sem vazar tipos de FFI para o resto de `media`.

use crate::media::audio::AudioData;
use crate::model::{FrameRate, Ticks};
use anyhow::Result;
use image::RgbaImage;
use std::path::Path;
use std::sync::Arc;

/// Contrato comum de decodificação de vídeo em streaming servido por uma
/// API nativa do sistema operacional.
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

/// Abre um arquivo de mídia usando o backend nativo da plataforma atual.
#[cfg(windows)]
pub fn open_media(path: &Path) -> Result<SystemLoaded> {
    let loaded = super::mf::open_media(path)?;
    Ok(SystemLoaded {
        backend: "Media Foundation",
        duration: loaded.duration,
        video: loaded
            .video
            .map(|vr| Arc::new(vr) as Arc<dyn SystemVideoReader>),
        audio: loaded.audio,
        audio_truncated: loaded.audio_truncated,
    })
}

/// Plataformas sem backend nativo configurado devolvem erro recuperável (§0).
#[cfg(not(windows))]
pub fn open_media(path: &Path) -> Result<SystemLoaded> {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy())
        .unwrap_or_else(|| path.to_string_lossy());
    anyhow::bail!(
        "{name}: contêineres MP4/MOV/MKV/WEBM/M4V/M4A/AAC ainda não contam com backend nativo nesta plataforma; \
         utilize AVI (MJPEG/PCM), Y4M, GIF ou WAV/AIFF/FLAC/OGG/MP3"
    )
}
