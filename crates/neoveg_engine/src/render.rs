//! Renderização final (Render As) multi-formato: MP4/MOV/3GP/WMV via Media
//! Foundation (Windows), AVI (MJPEG / não comprimido RGB24), GIF animado, Y4M,
//! sequências de imagem (PNG, JPEG, TIFF, TGA, BMP, WebP, PNM, QOI, DPX, EXR) e
//! formatos de áudio (WAV, BWF, RF64, Wave64, AIFF, AU, CAF, QOA, AAC/M4A, WMA, MP3).

use crate::engine::audio::Mixer;
use crate::engine::video::render_frame;
use crate::engine::Frame;
use crate::media::audio_extra::{
    AiffWriter, AuWriter, CafWriter, ExtWaveWriter, QoaWriter, W64Writer,
};
use crate::media::avi::{AviVideoCodec, AviWriter};
use crate::media::ffv1::Ffv1Encoder;
use crate::media::flac::FlacWriter;
use crate::media::img_extra::{encode_image_for_ext, encode_seq_frame, ImageSeqFormat};
use crate::media::mkv::{MkvAudioCodec, MkvVideoCodec, MkvWriter};
use crate::media::mov::{MovVideoCodec, MovWriter};
use crate::media::y4m::Y4mWriter;
use crate::media::MediaCache;
use crate::model::{FrameRate, Project, Ticks, TICKS_PER_SEC};
use crate::util::lock;
use anyhow::{bail, Context, Result};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RenderFormat {
    Mp4H264,
    Mp4Hevc,
    Mp4Av1,
    MovH264,
    MovHevc,
    MovAv1,
    MovMpeg4,
    MkvH264,
    MkvHevc,
    MkvAv1,
    MkvVp9,
    ThreeGp,
    Wmv,
    AviMjpeg,
    AviUncompressed,
    AviUncompressedBgra,
    AviUncompressedYuv422,
    AviUncompressedRgb10,
    MovUncompressedRgb,
    MovUncompressedYuv422,
    MovMjpeg,
    MovPng,
    MkvFfv1,
    MkvMjpeg,
    MkvPng,
    AnimatedGif,
    Y4m,
    PngSequence,
    JpegSequence,
    TiffSequence,
    TgaSequence,
    BmpSequence,
    WebpSequence,
    PnmSequence,
    QoiSequence,
    DpxSequence,
    Dpx10Sequence,
    CineonSequence,
    ExrSequence,
    Wav,
    Bwf,
    Rf64,
    Wave64,
    Aiff,
    Au,
    Caf,
    Qoa,
    Flac,
    AacM4a,
    AacAdts,
    Wma,
    Mp3,
}

impl RenderFormat {
    pub const ALL: [RenderFormat; 52] = [
        RenderFormat::Mp4H264,
        RenderFormat::Mp4Hevc,
        RenderFormat::Mp4Av1,
        RenderFormat::MovH264,
        RenderFormat::MovHevc,
        RenderFormat::MovAv1,
        RenderFormat::MovMpeg4,
        RenderFormat::MkvH264,
        RenderFormat::MkvHevc,
        RenderFormat::MkvAv1,
        RenderFormat::MkvVp9,
        RenderFormat::ThreeGp,
        RenderFormat::Wmv,
        RenderFormat::AviMjpeg,
        RenderFormat::AviUncompressed,
        RenderFormat::AviUncompressedBgra,
        RenderFormat::AviUncompressedYuv422,
        RenderFormat::AviUncompressedRgb10,
        RenderFormat::MovUncompressedRgb,
        RenderFormat::MovUncompressedYuv422,
        RenderFormat::MovMjpeg,
        RenderFormat::MovPng,
        RenderFormat::MkvFfv1,
        RenderFormat::MkvMjpeg,
        RenderFormat::MkvPng,
        RenderFormat::AnimatedGif,
        RenderFormat::Y4m,
        RenderFormat::PngSequence,
        RenderFormat::JpegSequence,
        RenderFormat::TiffSequence,
        RenderFormat::TgaSequence,
        RenderFormat::BmpSequence,
        RenderFormat::WebpSequence,
        RenderFormat::PnmSequence,
        RenderFormat::QoiSequence,
        RenderFormat::DpxSequence,
        RenderFormat::Dpx10Sequence,
        RenderFormat::CineonSequence,
        RenderFormat::ExrSequence,
        RenderFormat::Wav,
        RenderFormat::Bwf,
        RenderFormat::Rf64,
        RenderFormat::Wave64,
        RenderFormat::Aiff,
        RenderFormat::Au,
        RenderFormat::Caf,
        RenderFormat::Qoa,
        RenderFormat::Flac,
        RenderFormat::AacM4a,
        RenderFormat::AacAdts,
        RenderFormat::Wma,
        RenderFormat::Mp3,
    ];
    pub fn name_in(self, lang: neoveg_common::Language) -> &'static str {
        let pt = match self {
            Self::Mp4H264 => "MP4 H.264 / AVC (+ AAC)",
            Self::Mp4Hevc => "MP4 H.265 / HEVC (+ AAC)",
            Self::Mp4Av1 => "MP4 AV1 (+ AAC)",
            Self::MovH264 => "QuickTime MOV H.264 (+ AAC)",
            Self::MovHevc => "QuickTime MOV H.265 / HEVC (+ AAC)",
            Self::MovAv1 => "QuickTime MOV AV1 (+ AAC)",
            Self::MovMpeg4 => "QuickTime MOV MPEG-4 Part 2 (+ AAC)",
            Self::MkvH264 => "Matroska MKV H.264 / AVC (+ AAC)",
            Self::MkvHevc => "Matroska MKV H.265 / HEVC (+ AAC)",
            Self::MkvAv1 => "Matroska MKV AV1 (+ AAC)",
            Self::MkvVp9 => "Matroska MKV VP9 (+ AAC)",
            Self::ThreeGp => "3GPP H.264 (+ AAC)",
            Self::Wmv => "Windows Media Video (.wmv)",
            Self::AviMjpeg => "AVI (Motion-JPEG + PCM)",
            Self::AviUncompressed => "AVI não comprimido RGB24 (+ PCM)",
            Self::AviUncompressedBgra => "AVI não comprimido BGRA 32-bit (+ PCM)",
            Self::AviUncompressedYuv422 => "AVI não comprimido YUV 4:2:2 UYVY (+ PCM)",
            Self::AviUncompressedRgb10 => "AVI não comprimido 10-bit RGB r210 (+ PCM)",
            Self::MovUncompressedRgb => "QuickTime MOV não comprimido BGRA (+ PCM)",
            Self::MovUncompressedYuv422 => "QuickTime MOV não comprimido YUV 4:2:2 2vuy (+ PCM)",
            Self::MovMjpeg => "QuickTime MOV Photo-JPEG (+ PCM)",
            Self::MovPng => "QuickTime MOV PNG com Alfa (+ PCM)",
            Self::MkvFfv1 => "Matroska MKV FFV1 sem perdas (+ PCM)",
            Self::MkvMjpeg => "Matroska MKV Motion-JPEG (+ PCM)",
            Self::MkvPng => "Matroska MKV PNG sem perdas (+ PCM)",
            Self::AnimatedGif => "GIF animado (+ WAV opcional)",
            Self::Y4m => "YUV4MPEG2 .y4m não comprimido (+ WAV)",
            Self::PngSequence => "Sequência de imagens PNG (+ WAV)",
            Self::JpegSequence => "Sequência de imagens JPEG (+ WAV)",
            Self::TiffSequence => "Sequência de imagens TIFF (+ WAV)",
            Self::TgaSequence => "Sequência de imagens Targa TGA (+ WAV)",
            Self::BmpSequence => "Sequência de imagens BMP (+ WAV)",
            Self::WebpSequence => "Sequência de imagens WebP (+ WAV)",
            Self::PnmSequence => "Sequência de imagens PNM/PPM (+ WAV)",
            Self::QoiSequence => "Sequência de imagens QOI (+ WAV)",
            Self::DpxSequence => "Sequência DPX SMPTE 268M 8-bit (+ WAV)",
            Self::Dpx10Sequence => "Sequência DPX SMPTE 268M 10-bit (+ WAV)",
            Self::CineonSequence => "Sequência Kodak Cineon 10-bit (.cin + WAV)",
            Self::ExrSequence => "Sequência OpenEXR HDR (+ WAV)",
            Self::Wav => "Somente áudio WAV",
            Self::Bwf => "Broadcast Wave BWF (EBU bext)",
            Self::Rf64 => "Áudio RF64 (WAV 64 bits >4 GB)",
            Self::Wave64 => "Sony Wave64 (.w64)",
            Self::Aiff => "Áudio AIFF / AIFF-C",
            Self::Au => "Sun / NeXT AU (.au)",
            Self::Caf => "Apple Core Audio Format (.caf)",
            Self::Qoa => "Áudio QOA (Quite OK Audio)",
            Self::Flac => "Áudio FLAC sem perdas (.flac)",
            Self::AacM4a => "Áudio AAC (.m4a)",
            Self::AacAdts => "Áudio AAC ADTS (.aac)",
            Self::Wma => "Windows Media Audio (.wma)",
            Self::Mp3 => "Áudio MP3 (.mp3)",
        };
        neoveg_common::tr(lang, pt)
    }

    pub fn name(self) -> &'static str {
        self.name_in(neoveg_common::Language::PtBr)
    }

    pub fn ext(self) -> &'static str {
        match self {
            Self::Mp4H264 | Self::Mp4Hevc | Self::Mp4Av1 => "mp4",
            Self::MovH264
            | Self::MovHevc
            | Self::MovAv1
            | Self::MovMpeg4
            | Self::MovUncompressedRgb
            | Self::MovUncompressedYuv422
            | Self::MovMjpeg
            | Self::MovPng => "mov",
            Self::ThreeGp => "3gp",
            Self::Wmv => "wmv",
            Self::AviMjpeg
            | Self::AviUncompressed
            | Self::AviUncompressedBgra
            | Self::AviUncompressedYuv422
            | Self::AviUncompressedRgb10 => "avi",
            Self::MkvFfv1
            | Self::MkvMjpeg
            | Self::MkvPng
            | Self::MkvH264
            | Self::MkvHevc
            | Self::MkvAv1
            | Self::MkvVp9 => "mkv",
            Self::AnimatedGif => "gif",
            Self::Y4m => "y4m",
            Self::PngSequence => "png",
            Self::JpegSequence => "jpg",
            Self::TiffSequence => "tif",
            Self::TgaSequence => "tga",
            Self::BmpSequence => "bmp",
            Self::WebpSequence => "webp",
            Self::PnmSequence => "ppm",
            Self::QoiSequence => "qoi",
            Self::DpxSequence | Self::Dpx10Sequence => "dpx",
            Self::CineonSequence => "cin",
            Self::ExrSequence => "exr",
            Self::Wav => "wav",
            Self::Bwf => "bwf",
            Self::Rf64 => "rf64",
            Self::Wave64 => "w64",
            Self::Aiff => "aiff",
            Self::Au => "au",
            Self::Caf => "caf",
            Self::Qoa => "qoa",
            Self::Flac => "flac",
            Self::AacM4a => "m4a",
            Self::AacAdts => "aac",
            Self::Wma => "wma",
            Self::Mp3 => "mp3",
        }
    }

    pub fn image_seq(self) -> Option<ImageSeqFormat> {
        match self {
            Self::PngSequence => Some(ImageSeqFormat::Png),
            Self::JpegSequence => Some(ImageSeqFormat::Jpeg),
            Self::TiffSequence => Some(ImageSeqFormat::Tiff),
            Self::TgaSequence => Some(ImageSeqFormat::Tga),
            Self::BmpSequence => Some(ImageSeqFormat::Bmp),
            Self::WebpSequence => Some(ImageSeqFormat::Webp),
            Self::PnmSequence => Some(ImageSeqFormat::Pnm),
            Self::QoiSequence => Some(ImageSeqFormat::Qoi),
            Self::DpxSequence => Some(ImageSeqFormat::Dpx),
            Self::Dpx10Sequence => Some(ImageSeqFormat::Dpx10),
            Self::CineonSequence => Some(ImageSeqFormat::Cineon),
            Self::ExrSequence => Some(ImageSeqFormat::Exr),
            _ => None,
        }
    }

    pub fn is_audio_only(self) -> bool {
        matches!(
            self,
            Self::Wav
                | Self::Bwf
                | Self::Rf64
                | Self::Wave64
                | Self::Aiff
                | Self::Au
                | Self::Caf
                | Self::Qoa
                | Self::Flac
                | Self::AacM4a
                | Self::AacAdts
                | Self::Wma
                | Self::Mp3
        )
    }

    pub fn has_embedded_audio(self) -> bool {
        self.is_audio_only()
            || matches!(
                self,
                Self::AviMjpeg
                    | Self::AviUncompressed
                    | Self::AviUncompressedBgra
                    | Self::AviUncompressedYuv422
                    | Self::AviUncompressedRgb10
                    | Self::MovUncompressedRgb
                    | Self::MovUncompressedYuv422
                    | Self::MovMjpeg
                    | Self::MovPng
                    | Self::MkvFfv1
                    | Self::MkvMjpeg
                    | Self::MkvPng
                    | Self::Mp4H264
                    | Self::Mp4Hevc
                    | Self::Mp4Av1
                    | Self::MovH264
                    | Self::MovHevc
                    | Self::MovAv1
                    | Self::MovMpeg4
                    | Self::MkvH264
                    | Self::MkvHevc
                    | Self::MkvAv1
                    | Self::MkvVp9
                    | Self::ThreeGp
                    | Self::Wmv
            )
    }

    pub fn uses_jpeg_quality(self) -> bool {
        matches!(
            self,
            Self::AviMjpeg | Self::MovMjpeg | Self::MkvMjpeg | Self::JpegSequence
        )
    }

    pub fn uses_video_bitrate(self) -> bool {
        matches!(
            self,
            Self::Mp4H264
                | Self::Mp4Hevc
                | Self::Mp4Av1
                | Self::MovH264
                | Self::MovHevc
                | Self::MovAv1
                | Self::MovMpeg4
                | Self::MkvH264
                | Self::MkvHevc
                | Self::MkvAv1
                | Self::MkvVp9
                | Self::ThreeGp
                | Self::Wmv
        )
    }

    pub fn uses_audio_bitrate(self) -> bool {
        self.uses_video_bitrate()
            || matches!(self, Self::AacM4a | Self::AacAdts | Self::Wma | Self::Mp3)
    }

    pub fn uses_wav_bits(self) -> bool {
        matches!(
            self,
            Self::Wav
                | Self::Bwf
                | Self::Rf64
                | Self::Wave64
                | Self::Aiff
                | Self::Au
                | Self::Caf
                | Self::Flac
        ) || (!self.has_embedded_audio())
    }

    pub fn uses_windows_mf(self) -> bool {
        self.uses_video_bitrate()
            || matches!(self, Self::AacM4a | Self::AacAdts | Self::Wma | Self::Mp3)
    }

    pub fn from_path(path: &Path) -> Self {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "mp4" | "m4v" => Self::Mp4H264,
            "mov" => Self::MovH264,
            "mkv" => Self::MkvFfv1,
            "3gp" => Self::ThreeGp,
            "wmv" => Self::Wmv,
            "gif" => Self::AnimatedGif,
            "y4m" => Self::Y4m,
            "png" => Self::PngSequence,
            "jpg" | "jpeg" => Self::JpegSequence,
            "tif" | "tiff" => Self::TiffSequence,
            "tga" => Self::TgaSequence,
            "bmp" => Self::BmpSequence,
            "webp" => Self::WebpSequence,
            "ppm" | "pnm" => Self::PnmSequence,
            "qoi" => Self::QoiSequence,
            "dpx" => Self::DpxSequence,
            "cin" => Self::CineonSequence,
            "exr" => Self::ExrSequence,
            "wav" | "wave" => Self::Wav,
            "bwf" => Self::Bwf,
            "rf64" => Self::Rf64,
            "w64" => Self::Wave64,
            "aif" | "aiff" | "aifc" => Self::Aiff,
            "au" | "snd" => Self::Au,
            "caf" => Self::Caf,
            "qoa" => Self::Qoa,
            "flac" => Self::Flac,
            "m4a" => Self::AacM4a,
            "aac" => Self::AacAdts,
            "wma" => Self::Wma,
            "mp3" => Self::Mp3,
            _ => Self::AviMjpeg,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WavBits {
    Pcm16,
    Pcm24,
    Float32,
}

pub type FrameRenderHook = Arc<dyn Fn(u64, Ticks, Frame) -> Frame + Send + Sync>;
pub type FramePreviewHook = Arc<dyn Fn(u64, &Frame) + Send + Sync>;
pub type TimeMapHook = Arc<dyn Fn(u64, Ticks) -> Ticks + Send + Sync>;

/// Opções avançadas de pipeline usadas pelo Neoveg Encoder (taxa de quadros
/// alternativa, resolução de origem antes do corte/escala, efeitos pós-composição,
/// mapeamento temporal, miniatura ao vivo e pausa cooperativa).
#[derive(Clone)]
pub struct RenderOptions {
    pub fps_override: Option<FrameRate>,
    pub source_size: Option<(u32, u32)>,
    pub frame_hook: Option<FrameRenderHook>,
    pub preview_hook: Option<FramePreviewHook>,
    pub time_map: Option<TimeMapHook>,
    pub pause: Option<Arc<AtomicBool>>,
    pub include_video: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            fps_override: None,
            source_size: None,
            frame_hook: None,
            preview_hook: None,
            time_map: None,
            pause: None,
            include_video: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RenderSettings {
    pub format: RenderFormat,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub start: Ticks,
    pub end: Ticks,
    pub jpeg_quality: u8,
    pub include_audio: bool,
    pub wav_bits: WavBits,
    pub video_bitrate_kbps: u32,
    pub audio_bitrate_kbps: u32,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            format: RenderFormat::AviMjpeg,
            path: PathBuf::from("render.avi"),
            width: 1920,
            height: 1080,
            start: Ticks::ZERO,
            end: Ticks::ZERO,
            jpeg_quality: 90,
            include_audio: true,
            wav_bits: WavBits::Pcm16,
            video_bitrate_kbps: 12_000,
            audio_bitrate_kbps: 192,
        }
    }
}

#[derive(Default)]
pub struct RenderProgress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
    pub finished: AtomicBool,
    pub error: Mutex<Option<String>>,
    pub started: Mutex<Option<Instant>>,
    pub elapsed: Mutex<f64>,
}

impl RenderProgress {
    pub fn fraction(&self) -> f32 {
        let t = self.total.load(Ordering::Relaxed).max(1);
        self.done.load(Ordering::Relaxed) as f32 / t as f32
    }
}

pub fn spawn(project: Project, media: Arc<MediaCache>, s: RenderSettings) -> Arc<RenderProgress> {
    let prog = Arc::new(RenderProgress::default());
    *prog
        .started
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Instant::now());
    let pr = prog.clone();
    let spawned = std::thread::Builder::new()
        .name("render".into())
        .spawn(move || {
            let t0 = Instant::now();
            // §0.6: um pânico no render vira erro e o job termina (nunca fica "em execução")
            let r = match crate::util::guard("renderização", || run(&project, &media, &s, &pr)) {
                Ok(r) => r.map_err(|e| format!("{e:#}")),
                Err(e) => Err(e),
            };
            *lock(&pr.elapsed) = t0.elapsed().as_secs_f64();
            if let Err(e) = r {
                crate::util::log_error(&format!("renderização falhou: {e}"));
                *lock(&pr.error) = Some(e);
            }
            pr.finished.store(true, Ordering::SeqCst);
        });
    if let Err(e) = spawned {
        *lock(&prog.error) = Some(format!("não foi possível iniciar a renderização: {e}"));
        prog.finished.store(true, Ordering::SeqCst);
    }
    prog
}

/// Gravador WAV simples (PCM 16/24 bits ou float 32), estéreo ou 5.1.
pub struct WavWriter {
    out: BufWriter<File>,
    bits: WavBits,
    data_bytes: u64,
    /// Tamanho do bloco `fmt ` (16 no PCM clássico, 40 no WAVE_FORMAT_EXTENSIBLE).
    fmt_len: u32,
}

/// Máscara de canais 5.1 do WAVE_FORMAT_EXTENSIBLE: FL, FR, FC, LFE, BL, BR.
const WAV_MASK_5_1: u32 = 0x3F;

impl WavWriter {
    pub fn create(path: &Path, sr: u32, bits: WavBits) -> Result<Self> {
        Self::create_with_channels(path, sr, bits, 2)
    }

    /// Cria um WAV com `channels` canais (1, 2 ou 6). Acima de 2 canais usa
    /// WAVE_FORMAT_EXTENSIBLE com máscara 5.1, como pede a especificação
    /// pública do formato para mais de dois canais.
    pub fn create_with_channels(
        path: &Path,
        sr: u32,
        bits: WavBits,
        channels: u16,
    ) -> Result<Self> {
        if channels != 1 && channels != 2 && channels != 6 {
            bail!("WAV com {channels} canais não suportado (use 1, 2 ou 6)");
        }
        let mut out = BufWriter::new(
            File::create(path).with_context(|| format!("criando {}", path.display()))?,
        );
        let (fmt, bps): (u16, u16) = match bits {
            WavBits::Pcm16 => (1, 16),
            WavBits::Pcm24 => (1, 24),
            WavBits::Float32 => (3, 32),
        };
        let block = channels * bps / 8;
        let extensible = channels > 2;
        let fmt_len: u32 = if extensible { 40 } else { 16 };
        out.write_all(b"RIFF")?;
        out.write_all(&0u32.to_le_bytes())?;
        out.write_all(b"WAVEfmt ")?;
        out.write_all(&fmt_len.to_le_bytes())?;
        let tag: u16 = if extensible { 0xFFFE } else { fmt };
        out.write_all(&tag.to_le_bytes())?;
        out.write_all(&channels.to_le_bytes())?;
        out.write_all(&sr.to_le_bytes())?;
        out.write_all(&sr.saturating_mul(block as u32).to_le_bytes())?;
        out.write_all(&block.to_le_bytes())?;
        out.write_all(&bps.to_le_bytes())?;
        if extensible {
            out.write_all(&22u16.to_le_bytes())?; // cbSize
            out.write_all(&bps.to_le_bytes())?; // wValidBitsPerSample
            out.write_all(&WAV_MASK_5_1.to_le_bytes())?;
            // SubFormat GUID {0000000X-0000-0010-8000-00AA00389B71}
            out.write_all(&u32::from(fmt).to_le_bytes())?;
            out.write_all(&[
                0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
            ])?;
        }
        out.write_all(b"data")?;
        out.write_all(&0u32.to_le_bytes())?;
        Ok(WavWriter {
            out,
            bits,
            data_bytes: 0,
            fmt_len,
        })
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        for &s in samples {
            let s = if s.is_finite() { s } else { 0.0 };
            match self.bits {
                WavBits::Pcm16 => {
                    let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                    self.out.write_all(&v.to_le_bytes())?;
                    self.data_bytes += 2;
                }
                WavBits::Pcm24 => {
                    let v = (s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32;
                    self.out.write_all(&v.to_le_bytes()[..3])?;
                    self.data_bytes += 3;
                }
                WavBits::Float32 => {
                    self.out.write_all(&s.to_le_bytes())?;
                    self.data_bytes += 4;
                }
            }
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        self.out.flush()?;
        let mut f = self.out.into_inner().map_err(|e| anyhow::anyhow!("{e}"))?;
        // RIFF = "WAVE" + bloco fmt (8 + fmt_len) + cabeçalho data (8) + dados
        let header = 4 + 8 + u64::from(self.fmt_len) + 8;
        let riff = u32::try_from(header + self.data_bytes)
            .map_err(|_| anyhow::anyhow!("WAV acima de 4 GiB não é suportado"))?;
        f.seek(SeekFrom::Start(4))?;
        f.write_all(&riff.to_le_bytes())?;
        f.seek(SeekFrom::Start(12 + 8 + u64::from(self.fmt_len) + 4))?;
        f.write_all(&(self.data_bytes as u32).to_le_bytes())?;
        Ok(())
    }
}

pub(crate) fn to_i16(s: &[f32]) -> Vec<i16> {
    s.iter()
        .map(|v| (v.clamp(-1.0, 1.0) * 32767.0).round() as i16)
        .collect()
}

#[cfg(windows)]
fn mf_output_format(fmt: RenderFormat) -> Option<crate::media::mf::encode::MfOutputFormat> {
    use crate::media::mf::encode::MfOutputFormat;
    match fmt {
        RenderFormat::Mp4H264 => Some(MfOutputFormat::Mp4H264),
        RenderFormat::Mp4Hevc => Some(MfOutputFormat::Mp4Hevc),
        RenderFormat::Mp4Av1 => Some(MfOutputFormat::Mp4Av1),
        RenderFormat::MovH264 => Some(MfOutputFormat::MovH264),
        RenderFormat::MovHevc => Some(MfOutputFormat::MovHevc),
        RenderFormat::MovAv1 => Some(MfOutputFormat::MovAv1),
        RenderFormat::MovMpeg4 => Some(MfOutputFormat::MovMpeg4),
        RenderFormat::MkvH264 => Some(MfOutputFormat::MkvH264),
        RenderFormat::MkvHevc => Some(MfOutputFormat::MkvHevc),
        RenderFormat::MkvAv1 => Some(MfOutputFormat::MkvAv1),
        RenderFormat::MkvVp9 => Some(MfOutputFormat::MkvVp9),
        RenderFormat::ThreeGp => Some(MfOutputFormat::ThreeGp),
        RenderFormat::Wmv => Some(MfOutputFormat::Wmv),
        RenderFormat::AacM4a => Some(MfOutputFormat::M4aAac),
        RenderFormat::AacAdts => Some(MfOutputFormat::AdtsAac),
        RenderFormat::Wma => Some(MfOutputFormat::Wma),
        RenderFormat::Mp3 => Some(MfOutputFormat::Mp3),
        _ => None,
    }
}

fn wait_if_paused(prog: &RenderProgress, pause: Option<&AtomicBool>) -> Result<()> {
    let Some(p) = pause else {
        return Ok(());
    };
    while p.load(Ordering::Relaxed) {
        if prog.cancel.load(Ordering::Relaxed) {
            bail!("cancelado");
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    Ok(())
}

fn encode_jpeg_rgb(f: &Frame, w: usize, h: usize, quality: u8) -> Result<Vec<u8>> {
    let rgb = f.to_rgb8_opaque();
    let mut jpg = Vec::with_capacity((w * h / 4).max(512));
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, quality).encode(
        &rgb,
        w as u32,
        h as u32,
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(jpg)
}

fn encode_png_rgba(f: &Frame, w: usize, h: usize) -> Result<Vec<u8>> {
    use image::ImageEncoder;
    let rgba = f.to_rgba8();
    let mut png = Vec::with_capacity((w * h / 2).max(512));
    image::codecs::png::PngEncoder::new(&mut png).write_image(
        &rgba,
        w as u32,
        h as u32,
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(png)
}

#[allow(clippy::too_many_arguments)]
fn run_audio_only_with_pause(
    p: &Project,
    media: &MediaCache,
    s: &RenderSettings,
    prog: &RenderProgress,
    pause: Option<&AtomicBool>,
    sr: u32,
    start_sample: i64,
    end_sample: i64,
) -> Result<()> {
    let total = (end_sample - start_sample).max(0) as u64;
    prog.total.store(total, Ordering::Relaxed);

    #[cfg(windows)]
    type MfAudioEnc = crate::media::mf::encode::MfSinkEncoder;

    enum AudioSink {
        Wav(WavWriter),
        ExtWav(ExtWaveWriter),
        W64(W64Writer),
        Aiff(AiffWriter),
        Au(AuWriter),
        Caf(CafWriter),
        Qoa(Box<QoaWriter>),
        Flac(FlacWriter),
        #[cfg(windows)]
        Mf(MfAudioEnc),
    }

    let proj_ch = p.settings.channel_layout.channels();
    let (mut sink, ch) = match s.format {
        RenderFormat::Wav => (
            AudioSink::Wav(WavWriter::create_with_channels(
                &s.path,
                sr,
                s.wav_bits,
                proj_ch as u16,
            )?),
            proj_ch,
        ),
        RenderFormat::Bwf => (
            AudioSink::ExtWav(ExtWaveWriter::create_bwf(
                &s.path,
                sr,
                s.wav_bits,
                proj_ch as u16,
            )?),
            proj_ch,
        ),
        RenderFormat::Rf64 => (
            AudioSink::ExtWav(ExtWaveWriter::create_rf64(
                &s.path,
                sr,
                s.wav_bits,
                proj_ch as u16,
            )?),
            proj_ch,
        ),
        RenderFormat::Wave64 => (
            AudioSink::W64(W64Writer::create_with_channels(
                &s.path,
                sr,
                s.wav_bits,
                proj_ch as u16,
            )?),
            proj_ch,
        ),
        RenderFormat::Aiff => (
            AudioSink::Aiff(AiffWriter::create_with_channels(
                &s.path,
                sr,
                s.wav_bits,
                proj_ch as u16,
            )?),
            proj_ch,
        ),
        RenderFormat::Au => (
            AudioSink::Au(AuWriter::create(&s.path, sr, s.wav_bits, proj_ch as u16)?),
            proj_ch,
        ),
        RenderFormat::Caf => (
            AudioSink::Caf(CafWriter::create(&s.path, sr, s.wav_bits, proj_ch as u16)?),
            proj_ch,
        ),
        RenderFormat::Qoa => (
            AudioSink::Qoa(Box::new(QoaWriter::create_with_channels(
                &s.path,
                sr,
                proj_ch as u16,
            )?)),
            proj_ch,
        ),
        RenderFormat::Flac => (
            AudioSink::Flac(FlacWriter::create(&s.path, sr, s.wav_bits, proj_ch as u16)?),
            proj_ch,
        ),
        RenderFormat::AacM4a | RenderFormat::AacAdts | RenderFormat::Wma | RenderFormat::Mp3 => {
            #[cfg(windows)]
            {
                let Some(fmt) = mf_output_format(s.format) else {
                    bail!("formato MF inválido");
                };
                let enc = crate::media::mf::encode::MfSinkEncoder::create(
                    crate::media::mf::encode::MfEncodeConfig {
                        format: fmt,
                        path: s.path.clone(),
                        width: 2,
                        height: 2,
                        fps: p.settings.fps,
                        video_bitrate_bps: 0,
                        audio_sr: Some(sr),
                        audio_bitrate_bps: s.audio_bitrate_kbps.clamp(64, 512) * 1000,
                    },
                )?;
                (AudioSink::Mf(enc), 2)
            }
            #[cfg(not(windows))]
            {
                bail!(
                    "a codificação de áudio {} usa Windows Media Foundation e não está disponível nesta plataforma",
                    s.format.name()
                );
            }
        }
        _ => bail!("formato de áudio esperado"),
    };

    let mut mixer = Mixer::new_offline(sr);
    let am = media.audio_map();
    let mut buf = vec![0f32; 8192 * ch];
    // Compensação de latência dos plug-ins (PDC): enche a linha de atraso e
    // lê o projeto adiantado em `pdc` amostras.
    let pdc = mixer.total_latency(p) as i64;
    let mut pre = 0_i64;
    while pre < pdc {
        if prog.cancel.load(Ordering::Relaxed) {
            bail!("cancelado");
        }
        let n = ((pdc - pre) as usize).min(8192);
        let slice = &mut buf[..n * ch];
        if ch == 2 {
            mixer.process(p, &am, start_sample.saturating_add(pre), slice);
        } else {
            mixer.process_multichannel(p, &am, start_sample.saturating_add(pre), slice);
        }
        pre += n as i64;
    }
    let mut pos = start_sample;
    while pos < end_sample {
        if prog.cancel.load(Ordering::Relaxed) {
            bail!("cancelado");
        }
        wait_if_paused(prog, pause)?;
        let n = ((end_sample - pos) as usize).min(8192);
        let slice = &mut buf[..n * ch];
        if ch == 2 {
            mixer.process(p, &am, pos.saturating_add(pdc), slice);
        } else {
            mixer.process_multichannel(p, &am, pos.saturating_add(pdc), slice);
        }
        match &mut sink {
            AudioSink::Wav(w) => w.write(slice)?,
            AudioSink::ExtWav(w) => w.write(slice)?,
            AudioSink::W64(w) => w.write(slice)?,
            AudioSink::Aiff(w) => w.write(slice)?,
            AudioSink::Au(w) => w.write(slice)?,
            AudioSink::Caf(w) => w.write(slice)?,
            AudioSink::Qoa(w) => w.write(slice)?,
            AudioSink::Flac(w) => w.write(slice)?,
            #[cfg(windows)]
            AudioSink::Mf(w) => w.write_audio_f32(slice)?,
        }
        pos += n as i64;
        prog.done
            .store((pos - start_sample) as u64, Ordering::Relaxed);
    }
    match sink {
        AudioSink::Wav(w) => w.finish(),
        AudioSink::ExtWav(w) => w.finish(),
        AudioSink::W64(w) => w.finish(),
        AudioSink::Aiff(w) => w.finish(),
        AudioSink::Au(w) => w.finish(),
        AudioSink::Caf(w) => w.finish(),
        AudioSink::Qoa(w) => w.finish(),
        AudioSink::Flac(w) => w.finish(),
        #[cfg(windows)]
        AudioSink::Mf(w) => w.finish(),
    }
}

pub fn run(
    p: &Project,
    media: &MediaCache,
    s: &RenderSettings,
    prog: &RenderProgress,
) -> Result<()> {
    run_with_options(p, media, s, prog, &RenderOptions::default())
}

pub fn run_with_options(
    p: &Project,
    media: &MediaCache,
    s: &RenderSettings,
    prog: &RenderProgress,
    opts: &RenderOptions,
) -> Result<()> {
    if s.end <= s.start {
        bail!("intervalo de renderização vazio");
    }
    let fps = opts.fps_override.unwrap_or(p.settings.fps);
    let sr = p.settings.sample_rate;
    let dur = (s.end - s.start).0 as i128;
    let den = TICKS_PER_SEC as i128 * fps.den.max(1) as i128;
    let n_frames = ((dur * fps.num.max(1) as i128 + den / 2) / den).max(1) as u64;
    let (w, h) = (s.width.max(2) as usize, s.height.max(2) as usize);
    let sample_at = |i: u64| (s.start + fps.frame_to_ticks(i as i64)).to_samples_round(sr);

    if s.format.is_audio_only() {
        return run_audio_only_with_pause(
            p,
            media,
            s,
            prog,
            opts.pause.as_deref(),
            sr,
            sample_at(0),
            sample_at(n_frames),
        );
    }

    prog.total.store(n_frames, Ordering::Relaxed);
    let audio_map = media.audio_map();
    let has_audio = s.include_audio && !audio_map.is_empty();
    let mut mixer = Mixer::new_offline(sr);
    // Compensação de latência dos plug-ins (PDC): o áudio é lido adiantado
    // em `pdc` amostras para alinhar com o vídeo.
    let pdc = mixer.total_latency(p) as i64;

    #[cfg(windows)]
    type MfVideoEnc = crate::media::mf::encode::MfSinkEncoder;

    enum Sink {
        // encaixotado: o gravador OpenDML guarda índices e é bem maior que as outras variantes
        Avi(Box<AviWriter>),
        Mov(Box<MovWriter>),
        Mkv {
            writer: Box<MkvWriter>,
            ffv1: Option<Arc<Ffv1Encoder>>,
            audio_samples: u64,
        },
        Gif(
            Box<image::codecs::gif::GifEncoder<BufWriter<File>>>,
            FrameRate,
        ),
        Seq(PathBuf, String, ImageSeqFormat),
        Y4m(Y4mWriter),
        #[cfg(windows)]
        Mf(Box<MfVideoEnc>),
    }

    if let Some(parent) = s.path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let (w_even, h_even) = ((w / 2).max(1) * 2, (h / 2).max(1) * 2);
    let (render_w, render_h) = if s.format.uses_windows_mf() {
        (w_even, h_even)
    } else if matches!(
        s.format,
        RenderFormat::AviUncompressedYuv422 | RenderFormat::MovUncompressedYuv422
    ) {
        (w_even, h)
    } else {
        (w, h)
    };
    let avi_codec = |codec| {
        Ok::<_, anyhow::Error>(Sink::Avi(Box::new(AviWriter::create_with_codec(
            &s.path,
            render_w as u32,
            render_h as u32,
            fps,
            if has_audio { Some(sr) } else { None },
            codec,
        )?)))
    };
    let mov_codec = |codec| {
        Ok::<_, anyhow::Error>(Sink::Mov(Box::new(MovWriter::create(
            &s.path,
            render_w as u32,
            render_h as u32,
            fps,
            if has_audio { Some(sr) } else { None },
            codec,
        )?)))
    };
    let mkv_audio = if has_audio {
        Some(MkvAudioCodec::Pcm16Le {
            sample_rate: sr,
            channels: 2,
        })
    } else {
        None
    };

    let mut sink = match s.format {
        RenderFormat::AviMjpeg => avi_codec(AviVideoCodec::Mjpeg)?,
        RenderFormat::AviUncompressed => avi_codec(AviVideoCodec::UncompressedRgb24)?,
        RenderFormat::AviUncompressedBgra => avi_codec(AviVideoCodec::UncompressedBgra)?,
        RenderFormat::AviUncompressedYuv422 => avi_codec(AviVideoCodec::UncompressedUyvy)?,
        RenderFormat::AviUncompressedRgb10 => avi_codec(AviVideoCodec::UncompressedR210)?,
        RenderFormat::MovUncompressedRgb => mov_codec(MovVideoCodec::UncompressedBgra)?,
        RenderFormat::MovUncompressedYuv422 => mov_codec(MovVideoCodec::UncompressedYuv422)?,
        RenderFormat::MovMjpeg => mov_codec(MovVideoCodec::PhotoJpeg)?,
        RenderFormat::MovPng => mov_codec(MovVideoCodec::Png)?,
        RenderFormat::MkvFfv1 => {
            let ffv1 = Arc::new(Ffv1Encoder::new(render_w as u32, render_h as u32, true)?);
            let writer = MkvWriter::create(
                &s.path,
                render_w as u32,
                render_h as u32,
                fps,
                MkvVideoCodec::Ffv1 {
                    codec_private: ffv1.configuration_record().to_vec(),
                    has_alpha: ffv1.has_alpha(),
                },
                mkv_audio,
            )?;
            Sink::Mkv {
                writer: Box::new(writer),
                ffv1: Some(ffv1),
                audio_samples: 0,
            }
        }
        RenderFormat::MkvMjpeg | RenderFormat::MkvPng => {
            let vcodec = if s.format == RenderFormat::MkvMjpeg {
                MkvVideoCodec::Mjpeg
            } else {
                MkvVideoCodec::Png
            };
            let writer = MkvWriter::create(
                &s.path,
                render_w as u32,
                render_h as u32,
                fps,
                vcodec,
                mkv_audio,
            )?;
            Sink::Mkv {
                writer: Box::new(writer),
                ffv1: None,
                audio_samples: 0,
            }
        }
        RenderFormat::AnimatedGif => {
            let bw = BufWriter::new(
                File::create(&s.path).with_context(|| format!("criando {}", s.path.display()))?,
            );
            let mut enc = image::codecs::gif::GifEncoder::new_with_speed(bw, 10);
            enc.set_repeat(image::codecs::gif::Repeat::Infinite)?;
            Sink::Gif(Box::new(enc), fps)
        }
        RenderFormat::Y4m => Sink::Y4m(Y4mWriter::create(
            &s.path,
            render_w as u32,
            render_h as u32,
            fps,
        )?),
        other if other.uses_windows_mf() => {
            #[cfg(windows)]
            {
                let Some(fmt) = mf_output_format(other) else {
                    bail!("formato MF inválido");
                };
                let enc = crate::media::mf::encode::MfSinkEncoder::create(
                    crate::media::mf::encode::MfEncodeConfig {
                        format: fmt,
                        path: s.path.clone(),
                        width: render_w as u32,
                        height: render_h as u32,
                        fps,
                        video_bitrate_bps: s.video_bitrate_kbps.clamp(300, 200_000) * 1000,
                        audio_sr: if has_audio { Some(sr) } else { None },
                        audio_bitrate_bps: s.audio_bitrate_kbps.clamp(64, 512) * 1000,
                    },
                )?;
                Sink::Mf(Box::new(enc))
            }
            #[cfg(not(windows))]
            {
                let _ = (w_even, h_even, other);
                bail!(
                    "exportação {} via Media Foundation disponível apenas no Windows até os backends nativos VideoToolbox/VA-API",
                    s.format.name()
                );
            }
        }
        other => {
            let Some(seq) = other.image_seq() else {
                bail!("formato de renderização inesperado: {}", other.name());
            };
            let dir = s
                .path
                .parent()
                .filter(|d| !d.as_os_str().is_empty())
                .map(|d| d.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            let stem = s
                .path
                .file_stem()
                .map(|x| x.to_string_lossy().to_string())
                .unwrap_or_else(|| "quadro".into());
            Sink::Seq(dir, stem, seq)
        }
    };
    let ffv1_enc = match &sink {
        Sink::Mkv { ffv1, .. } => ffv1.clone(),
        _ => None,
    };
    let proj_ch = p.settings.channel_layout.channels();
    // WAV separado para formatos sem áudio embutido
    let mut side_wav = if has_audio && !s.format.has_embedded_audio() {
        Some(WavWriter::create_with_channels(
            &s.path.with_extension("wav"),
            sr,
            s.wav_bits,
            proj_ch as u16,
        )?)
    } else {
        None
    };
    let audio_ch = if side_wav.is_some() { proj_ch } else { 2 };

    let batch = rayon::current_num_threads().clamp(1, 8) as u64;
    if has_audio && pdc > 0 {
        let mut pre = 0_i64;
        let base = sample_at(0);
        let mut prime_buf = vec![0f32; 8192 * audio_ch];
        while pre < pdc {
            let n = ((pdc - pre) as usize).min(8192);
            if audio_ch == 2 {
                mixer.process(
                    p,
                    &audio_map,
                    base.saturating_add(pre),
                    &mut prime_buf[..n * 2],
                );
            } else {
                mixer.process_multichannel(
                    p,
                    &audio_map,
                    base.saturating_add(pre),
                    &mut prime_buf[..n * audio_ch],
                );
            }
            pre += n as i64;
        }
    }
    let mut i = 0u64;
    let quality = s.jpeg_quality.clamp(10, 100);
    let (src_w, src_h) = opts
        .source_size
        .map(|(sw, sh)| (sw.max(2) as usize, sh.max(2) as usize))
        .unwrap_or((render_w, render_h));
    while i < n_frames {
        if prog.cancel.load(Ordering::Relaxed) {
            bail!("cancelado");
        }
        wait_if_paused(prog, opts.pause.as_deref())?;
        let b_end = (i + batch).min(n_frames);
        // renderiza e codifica um lote de quadros em paralelo
        let encoded: Vec<Result<Vec<u8>>> = (i..b_end)
            .into_par_iter()
            .map(|fi| {
                let out_t = s.start + fps.frame_to_ticks(fi as i64);
                let mut f = if !opts.include_video {
                    Frame::filled(render_w, render_h, [0.0, 0.0, 0.0, 1.0])
                } else {
                    let t = opts
                        .time_map
                        .as_ref()
                        .map(|tm| tm(fi, out_t))
                        .unwrap_or(out_t);
                    let mut raw_f = render_frame(p, media, t, src_w, src_h);
                    if let Some(hook) = &opts.frame_hook {
                        raw_f = hook(fi, out_t, raw_f);
                    }
                    if raw_f.w != render_w || raw_f.h != render_h {
                        crate::engine::video::scale_frame_fit(&raw_f, render_w, render_h)
                    } else {
                        raw_f
                    }
                };
                if let Some(pv) = &opts.preview_hook {
                    if fi == 0 || fi.is_multiple_of(3) {
                        pv(fi, &f);
                    }
                }
                let _ = &mut f;
                match s.format {
                    RenderFormat::AviMjpeg | RenderFormat::MovMjpeg | RenderFormat::MkvMjpeg => {
                        encode_jpeg_rgb(&f, render_w, render_h, quality)
                    }
                    RenderFormat::MovPng | RenderFormat::MkvPng => {
                        encode_png_rgba(&f, render_w, render_h)
                    }
                    RenderFormat::AviUncompressed => Ok(AviWriter::rgba_to_dib_bgr24(
                        &f.to_rgba8(),
                        render_w as u32,
                        render_h as u32,
                    )),
                    RenderFormat::AviUncompressedBgra => Ok(AviWriter::rgba_to_dib_bgra32(
                        &f.to_rgba8(),
                        render_w as u32,
                        render_h as u32,
                    )),
                    RenderFormat::AviUncompressedYuv422 => Ok(AviWriter::rgba_to_uyvy(
                        &f.to_rgba8(),
                        render_w as u32,
                        render_h as u32,
                    )),
                    RenderFormat::AviUncompressedRgb10 => Ok(AviWriter::rgba_to_r210(
                        &f.to_rgba8(),
                        render_w as u32,
                        render_h as u32,
                    )),
                    RenderFormat::MovUncompressedRgb => Ok(MovWriter::rgba_to_bgra_topdown(
                        &f.to_rgba8(),
                        render_w as u32,
                        render_h as u32,
                    )),
                    RenderFormat::MovUncompressedYuv422 => Ok(MovWriter::rgba_to_2vuy(
                        &f.to_rgba8(),
                        render_w as u32,
                        render_h as u32,
                    )),
                    RenderFormat::MkvFfv1 => {
                        let Some(enc) = ffv1_enc.as_ref() else {
                            bail!("codificador FFV1 não inicializado");
                        };
                        enc.encode_frame(&f.to_rgba8())
                    }
                    other if other.uses_windows_mf() => {
                        #[cfg(windows)]
                        {
                            Ok(crate::media::mf::encode::rgba_to_nv12(
                                &f.to_rgba8(),
                                render_w as u32,
                                render_h as u32,
                            ))
                        }
                        #[cfg(not(windows))]
                        {
                            Ok(Vec::new())
                        }
                    }
                    RenderFormat::AnimatedGif | RenderFormat::Y4m => Ok(f.to_rgba8()),
                    other => {
                        let Some(seq) = other.image_seq() else {
                            bail!("formato inesperado");
                        };
                        let Some(img) = f.to_image() else {
                            bail!("quadro com tamanho inválido");
                        };
                        encode_seq_frame(&img, seq, quality)
                    }
                }
            })
            .collect();
        for (k, data) in encoded.into_iter().enumerate() {
            let fi = i + k as u64;
            let data = data?;
            match &mut sink {
                Sink::Avi(wr) => wr.write_video_jpeg(&data)?,
                Sink::Mov(wr) => wr.write_video_frame(&data)?,
                Sink::Mkv { writer, .. } => {
                    let vts = writer.frame_pts_ms(fi);
                    writer.write_video_packet(vts, true, data)?;
                }
                Sink::Gif(enc, gif_fps) => {
                    let Some(rgba_img) =
                        image::RgbaImage::from_raw(render_w as u32, render_h as u32, data)
                    else {
                        bail!("quadro GIF inválido");
                    };
                    let delay = image::Delay::from_numer_denom_ms(
                        u32::from(gif_frame_delay_cs(fi, *gif_fps)) * 10,
                        1,
                    );
                    enc.encode_frame(image::Frame::from_parts(rgba_img, 0, 0, delay))?;
                }
                Sink::Seq(dir, stem, seq) => {
                    std::fs::write(dir.join(format!("{stem}_{:06}.{}", fi, seq.ext())), &data)?;
                }
                Sink::Y4m(wr) => wr.write_frame(&data)?,
                #[cfg(windows)]
                Sink::Mf(wr) => wr.write_video_nv12(fi, &data)?,
            }
            if has_audio {
                let a0 = sample_at(fi);
                let a1 = sample_at(fi + 1);
                let n = (a1 - a0).max(0) as usize;
                let mut buf = vec![0f32; n * audio_ch];
                if audio_ch == 2 {
                    mixer.process(p, &audio_map, a0.saturating_add(pdc), &mut buf);
                } else {
                    mixer.process_multichannel(p, &audio_map, a0.saturating_add(pdc), &mut buf);
                }
                match (&mut sink, &mut side_wav) {
                    (Sink::Avi(wr), _) => wr.write_audio(&to_i16(&buf))?,
                    (Sink::Mov(wr), _) => wr.write_audio(&to_i16(&buf))?,
                    (
                        Sink::Mkv {
                            writer,
                            audio_samples,
                            ..
                        },
                        _,
                    ) => {
                        let pts_ms = MkvWriter::samples_pts_ms(*audio_samples, sr);
                        writer.write_audio_pcm_i16(pts_ms, &to_i16(&buf))?;
                        *audio_samples = audio_samples.saturating_add(n as u64);
                    }
                    #[cfg(windows)]
                    (Sink::Mf(wr), _) => wr.write_audio_f32(&buf)?,
                    (_, Some(wav)) => wav.write(&buf)?,
                    _ => {}
                }
            }
            prog.done.store(fi + 1, Ordering::Relaxed);
        }
        i = b_end;
    }
    match sink {
        Sink::Avi(wr) => (*wr).finish()?,
        Sink::Mov(wr) => (*wr).finish()?,
        Sink::Mkv { writer, .. } => (*writer).finish()?,
        Sink::Y4m(wr) => wr.finish()?,
        #[cfg(windows)]
        Sink::Mf(wr) => (*wr).finish()?,
        Sink::Gif(..) | Sink::Seq(..) => {}
    }
    if let Some(w) = side_wav {
        w.finish()?;
    }
    Ok(())
}

/// Atraso do quadro `fi` de um GIF animado, em centésimos de segundo.
///
/// O GIF guarda o atraso em centésimos inteiros. Um atraso fixo arredondado
/// (24 qps → 4 cs = 25 qps) acelera a animação e encurta o clipe; aqui o
/// resto é distribuído pelos quadros (4, 4, 4, 5, ...), de modo que o fim do
/// quadro `fi` cai em `floor((fi + 1) · 100 · den / num)` e a duração total
/// bate com a do projeto. O mínimo é 2 cs: muitos tocadores trocam atrasos
/// menores por 10 cs, o que deixaria a animação lenta.
pub(crate) fn gif_frame_delay_cs(fi: u64, fps: FrameRate) -> u16 {
    let num = u128::from(fps.num.max(1));
    let den = u128::from(fps.den.max(1));
    let end_cs = |n: u64| u128::from(n).saturating_mul(100).saturating_mul(den) / num;
    let cs = end_cs(fi.saturating_add(1)).saturating_sub(end_cs(fi));
    u16::try_from(cs.max(2)).unwrap_or(u16::MAX)
}

/// Salva uma imagem RGBA arbitrária em arquivo (reutilizado pelo Trimmer e pela Prévia).
pub fn save_rgba_snapshot(img: &image::RgbaImage, path: &Path) -> Result<()> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("png");
    let bytes = encode_image_for_ext(img, ext, 95)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

/// Salva o quadro atual em arquivo de imagem (PNG, JPEG, TIFF, BMP, TGA, WebP, DPX, EXR, QOI, PNM, etc.).
pub fn save_snapshot(p: &Project, media: &MediaCache, t: Ticks, path: &Path) -> Result<()> {
    let f = render_frame(
        p,
        media,
        t,
        p.settings.width as usize,
        p.settings.height as usize,
    );
    let Some(img) = f.to_image() else {
        bail!("quadro com tamanho inválido")
    };
    save_rgba_snapshot(&img, path)
}

#[cfg(test)]
mod gif_delay_tests {
    use super::*;

    fn total_cs(frames: u64, fps: FrameRate) -> u64 {
        (0..frames)
            .map(|i| u64::from(gif_frame_delay_cs(i, fps)))
            .sum()
    }

    #[test]
    fn gif_24fps_keeps_project_duration() {
        // 24 quadros a 24 qps = 1 s; o atraso fixo antigo (4 cs) dava 0,96 s.
        let fps = FrameRate { num: 24, den: 1 };
        assert_eq!(total_cs(24, fps), 100);
        assert_eq!(total_cs(181, fps), 754);
        assert!((0..181).all(|i| matches!(gif_frame_delay_cs(i, fps), 4 | 5)));
    }

    #[test]
    fn gif_ntsc_and_integer_rates_are_exact() {
        let ntsc = FrameRate {
            num: 30_000,
            den: 1001,
        };
        // 30000 quadros a 29,97 qps = 1001 s.
        assert_eq!(total_cs(30_000, ntsc), 100_100);
        assert_eq!(total_cs(25, FrameRate { num: 25, den: 1 }), 100);
        assert_eq!(total_cs(10, FrameRate { num: 10, den: 1 }), 100);
    }

    #[test]
    fn gif_delay_hostile_rates_do_not_panic() {
        assert_eq!(gif_frame_delay_cs(0, FrameRate { num: 60, den: 1 }), 2);
        assert_eq!(
            gif_frame_delay_cs(u64::MAX, FrameRate { num: 0, den: 0 }),
            2
        );
        assert_eq!(
            gif_frame_delay_cs(
                0,
                FrameRate {
                    num: 1,
                    den: u32::MAX
                }
            ),
            u16::MAX
        );
    }
}

#[cfg(test)]
mod wav_tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("neoveg_{}_{name}", std::process::id()))
    }

    fn u16_at(b: &[u8], i: usize) -> u16 {
        u16::from_le_bytes([b[i], b[i + 1]])
    }

    fn u32_at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    }

    #[test]
    fn wav_5_1_uses_extensible_header() {
        let path = tmp("51.wav");
        let mut w =
            WavWriter::create_with_channels(&path, 48000, WavBits::Float32, 6).expect("cria wav");
        w.write(&[0.25; 12]).expect("grava");
        w.finish().expect("fecha");
        let b = std::fs::read(&path).expect("lê");
        let _ = std::fs::remove_file(&path);
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(u32_at(&b, 4) as usize, b.len() - 8);
        assert_eq!(u32_at(&b, 16), 40);
        assert_eq!(u16_at(&b, 20), 0xFFFE);
        assert_eq!(u16_at(&b, 22), 6);
        assert_eq!(u16_at(&b, 32), 24); // bloco = 6 canais × 4 bytes
        assert_eq!(u32_at(&b, 40), WAV_MASK_5_1);
        assert_eq!(u32_at(&b, 44), 3); // SubFormat IEEE float
        assert_eq!(&b[60..64], b"data");
        assert_eq!(u32_at(&b, 64), 48);
        assert_eq!(b.len(), 68 + 48);
    }

    #[test]
    fn wav_stereo_header_unchanged() {
        let path = tmp("st.wav");
        let mut w = WavWriter::create(&path, 44100, WavBits::Pcm16).expect("cria wav");
        w.write(&[0.0; 4]).expect("grava");
        w.finish().expect("fecha");
        let b = std::fs::read(&path).expect("lê");
        let _ = std::fs::remove_file(&path);
        assert_eq!(u32_at(&b, 4) as usize, b.len() - 8);
        assert_eq!(u32_at(&b, 16), 16);
        assert_eq!(u16_at(&b, 20), 1);
        assert_eq!(u16_at(&b, 22), 2);
        assert_eq!(&b[36..40], b"data");
        assert_eq!(u32_at(&b, 40), 8);
    }

    /// Oráculo externo (§2.3): o `ffprobe` lê o WAV 5.1 como 6 canais em
    /// layout 5.1. Pula quando o `ffprobe` não está instalado.
    #[test]
    fn wav_5_1_oracle_ffprobe() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("target")
            .join("fixtures");
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let path = dir.join("oracle_5_1.wav");
        let mut w =
            WavWriter::create_with_channels(&path, 48000, WavBits::Pcm24, 6).expect("cria wav");
        w.write(&vec![0.1; 48000 * 6 / 10]).expect("grava");
        w.finish().expect("fecha");
        let Ok(out) = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "a:0",
                "-show_entries",
                "stream=channels,channel_layout",
                "-of",
                "default=noprint_wrappers=1",
            ])
            .arg(&path)
            .output()
        else {
            return;
        };
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "ffprobe falhou: {text}");
        assert!(text.contains("channels=6"), "{text}");
        assert!(text.contains("channel_layout=5.1"), "{text}");
    }

    #[test]
    fn wav_rejects_unsupported_channel_count() {
        let path = tmp("bad.wav");
        assert!(WavWriter::create_with_channels(&path, 48000, WavBits::Pcm16, 3).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn render_run_exports_new_video_image_and_audio_formats() {
        let dir = std::env::temp_dir().join(format!("neoveg_render_multi_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut p = Project::new_default();
        p.settings.width = 64;
        p.settings.height = 48;
        p.settings.fps = crate::model::FrameRate::FPS_25;
        let g = crate::media::import_generated(crate::model::GenSpec::ColorBars);
        p.media.push(g.item.clone());
        let cache = Arc::new(MediaCache::default());
        cache.insert(g.item.id, g.entry);
        let dur = p.settings.fps.frame_to_ticks(5);
        p.tracks[0]
            .events
            .push(crate::model::Event::new(&g.item, Ticks::ZERO, dur));

        let multi_fmts = [
            RenderFormat::AviUncompressed,
            RenderFormat::AviUncompressedBgra,
            RenderFormat::AviUncompressedYuv422,
            RenderFormat::AviUncompressedRgb10,
            RenderFormat::AnimatedGif,
            RenderFormat::PngSequence,
            RenderFormat::JpegSequence,
            RenderFormat::TiffSequence,
            RenderFormat::TgaSequence,
            RenderFormat::BmpSequence,
            RenderFormat::WebpSequence,
            RenderFormat::PnmSequence,
            RenderFormat::QoiSequence,
            RenderFormat::DpxSequence,
            RenderFormat::Dpx10Sequence,
            RenderFormat::CineonSequence,
            RenderFormat::ExrSequence,
            RenderFormat::Bwf,
            RenderFormat::Rf64,
            RenderFormat::Wave64,
            RenderFormat::Aiff,
            RenderFormat::Au,
            RenderFormat::Caf,
            RenderFormat::Qoa,
            RenderFormat::Flac,
        ];
        for fmt in multi_fmts {
            let out = dir.join(format!("clip_{fmt:?}.{}", fmt.ext()));
            let s = RenderSettings {
                format: fmt,
                path: out.clone(),
                width: 64,
                height: 48,
                start: Ticks::ZERO,
                end: dur,
                include_audio: false,
                ..RenderSettings::default()
            };
            let prog = RenderProgress::default();
            run(&p, &cache, &s, &prog)
                .unwrap_or_else(|e| panic!("falha ao renderizar {}: {e}", fmt.name()));
            if let Some(seq) = fmt.image_seq() {
                let first = dir.join(format!("clip_{fmt:?}_000000.{}", seq.ext()));
                assert!(first.exists(), "quadro inicial ausente para {}", fmt.name());
                let decoded = crate::media::img_extra::open_image_file(&first).unwrap();
                assert_eq!(decoded.dimensions(), (64, 48));
            } else {
                assert!(out.exists(), "saída ausente para {}", fmt.name());
                let imp = crate::media::import_file(&out)
                    .unwrap_or_else(|e| panic!("reimportação falhou para {}: {e}", fmt.name()));
                if fmt.is_audio_only() {
                    assert!(imp.item.has_audio);
                } else {
                    assert!(imp.item.has_video);
                }
            }
        }

        // Novos contêineres em Rust puro (.mov / .mkv) validados por demux próprio e ffmpeg
        for fmt in [
            RenderFormat::MovUncompressedRgb,
            RenderFormat::MovUncompressedYuv422,
            RenderFormat::MovMjpeg,
            RenderFormat::MovPng,
            RenderFormat::MkvFfv1,
            RenderFormat::MkvMjpeg,
            RenderFormat::MkvPng,
        ] {
            let out = dir.join(format!("pure_{fmt:?}.{}", fmt.ext()));
            let s = RenderSettings {
                format: fmt,
                path: out.clone(),
                width: 64,
                height: 48,
                start: Ticks::ZERO,
                end: dur,
                include_audio: true,
                ..RenderSettings::default()
            };
            run(&p, &cache, &s, &RenderProgress::default())
                .unwrap_or_else(|e| panic!("falha ao renderizar {}: {e}", fmt.name()));
            let idx = crate::media::demux::demux_file(&out)
                .unwrap_or_else(|e| panic!("demux falhou para {fmt:?}: {e}"));
            assert!(idx
                .tracks
                .iter()
                .any(|t| t.kind == crate::media::demux::TrackKind::Video));
        }

        // Paridade 5.1 surround em FLAC / AU / CAF
        p.settings.channel_layout = crate::model::ChannelLayout::Surround51;
        for fmt in [RenderFormat::Flac, RenderFormat::Au, RenderFormat::Caf] {
            let out = dir.join(format!("sur51_{fmt:?}.{}", fmt.ext()));
            let s = RenderSettings {
                format: fmt,
                path: out.clone(),
                width: 64,
                height: 48,
                start: Ticks::ZERO,
                end: dur,
                ..RenderSettings::default()
            };
            run(&p, &cache, &s, &RenderProgress::default()).unwrap();
            let raw = std::fs::read(&out).unwrap();
            match fmt {
                RenderFormat::Flac => {
                    assert_eq!(
                        claxon::FlacReader::open(&out)
                            .unwrap()
                            .streaminfo()
                            .channels,
                        6
                    );
                }
                RenderFormat::Au => {
                    assert_eq!(u32::from_be_bytes([raw[20], raw[21], raw[22], raw[23]]), 6);
                }
                RenderFormat::Caf => {
                    // caff(8) + desc(4) + size(8) + sr(8) + id(4) + flags(4) + bpp(4) + fpp(4) + ch_per_frame(4)
                    assert_eq!(u32::from_be_bytes([raw[44], raw[45], raw[46], raw[47]]), 6);
                }
                _ => {}
            }
        }
        p.settings.channel_layout = crate::model::ChannelLayout::Stereo;

        // Snapshots nos formatos novos
        for ext in [
            "png", "jpg", "tga", "dpx", "exr", "qoi", "ppm", "cin", "wbmp", "xbm", "xpm",
        ] {
            let snap = dir.join(format!("snap.{ext}"));
            save_snapshot(&p, &cache, Ticks::ZERO, &snap)
                .unwrap_or_else(|e| panic!("save_snapshot .{ext}: {e}"));
            let reopened = crate::media::img_extra::open_image_file(&snap)
                .unwrap_or_else(|e| panic!("open_image_file .{ext}: {e}"));
            assert_eq!(reopened.dimensions(), (64, 48), ".{ext}");
        }

        #[cfg(windows)]
        {
            for fmt in [
                RenderFormat::Mp4H264,
                RenderFormat::MovH264,
                RenderFormat::ThreeGp,
                RenderFormat::Wmv,
                RenderFormat::AacM4a,
                RenderFormat::Wma,
            ] {
                let out = dir.join(format!("mf_{:?}.{}", fmt, fmt.ext()));
                let s = RenderSettings {
                    format: fmt,
                    path: out.clone(),
                    width: 64,
                    height: 48,
                    start: Ticks::ZERO,
                    end: dur,
                    include_audio: true,
                    ..RenderSettings::default()
                };
                let prog = RenderProgress::default();
                run(&p, &cache, &s, &prog)
                    .unwrap_or_else(|e| panic!("falha ao renderizar {} via MF: {e}", fmt.name()));
                assert!(out.exists());
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
