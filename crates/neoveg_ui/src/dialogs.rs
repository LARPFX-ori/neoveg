//! Janelas de diálogo: propriedades do projeto e do evento, Pan/Crop, Track
//! Motion, cadeias de efeitos, editor de mídia gerada, Renderizar Como,
//! atalhos e "sobre".

use super::{
    apply_pref_defaults, App, EditLayoutPrefs, FxTarget, LayoutType, PlaybackScrollMode,
    Preferences, PreviewQuality, RenderJob, TimelineMouseScroll,
};
use crate::icons::{self, Icon};
use crate::model::*;
use crate::render::{RenderFormat, RenderSettings, WavBits};
use crate::theme::ThemeId;
use eframe::egui;
use egui::{pos2, vec2, Color32, Key, Rect, Sense, Stroke, StrokeKind};
use neoveg_common::{recommended_worker_threads, tr, tr_args, Language};
use neoveg_engine::commands::{resolved_shortcut, Shortcut, ShortcutKey, COMMANDS};
use neoveg_engine::session::EditorSession;
use std::path::{Path, PathBuf};

pub struct RenderDialogState {
    pub format: RenderFormat,
    pub template_id: String,
    pub filter: String,
    pub only_favorites: bool,
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub quality: u8,
    pub include_audio: bool,
    pub wav_bits: WavBits,
    pub video_bitrate_kbps: u32,
    pub audio_bitrate_kbps: u32,
}

#[derive(Clone, Copy)]
struct RenderTemplate {
    id: &'static str,
    format: RenderFormat,
    title: &'static str,
    summary: &'static str,
    /// `None` mantém a resolução do projeto (ou `true` na tupla indica 1/2 do projeto).
    resolution: Option<(u32, u32)>,
    half_project: bool,
    quality: u8,
    include_audio: bool,
    wav_bits: WavBits,
    video_bitrate_kbps: u32,
    audio_bitrate_kbps: u32,
}

const RENDER_TEMPLATES: &[RenderTemplate] = &[
    RenderTemplate {
        id: "mp4.1080p",
        format: RenderFormat::Mp4H264,
        title: "MP4 H.264 1080p Web (12 Mbps + AAC)",
        summary: "H.264 Main + AAC 192 kbps (Windows Media Foundation)",
        resolution: Some((1920, 1080)),
        half_project: false,
        quality: 90,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 12_000,
        audio_bitrate_kbps: 192,
    },
    RenderTemplate {
        id: "mp4.project",
        format: RenderFormat::Mp4H264,
        title: "MP4 H.264 igual ao projeto (+ AAC)",
        summary: "H.264 na resolução da timeline via Media Foundation",
        resolution: None,
        half_project: false,
        quality: 90,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 15_000,
        audio_bitrate_kbps: 192,
    },
    RenderTemplate {
        id: "hevc.4k",
        format: RenderFormat::Mp4Hevc,
        title: "MP4 HEVC / H.265 4K UHD (20 Mbps)",
        summary: "HEVC de alta eficiência para 4K/UHD + AAC 256 kbps",
        resolution: Some((3840, 2160)),
        half_project: false,
        quality: 92,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 20_000,
        audio_bitrate_kbps: 256,
    },
    RenderTemplate {
        id: "mov.project",
        format: RenderFormat::MovH264,
        title: "QuickTime MOV H.264 igual ao projeto",
        summary: "Contêiner .mov com H.264 + áudio AAC",
        resolution: None,
        half_project: false,
        quality: 92,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 18_000,
        audio_bitrate_kbps: 192,
    },
    RenderTemplate {
        id: "3gp.mobile",
        format: RenderFormat::ThreeGp,
        title: "3GPP 360p (Dispositivos móveis)",
        summary: "Contêiner 3GP compacto 640×360 H.264 + AAC",
        resolution: Some((640, 360)),
        half_project: false,
        quality: 85,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 1_500,
        audio_bitrate_kbps: 128,
    },
    RenderTemplate {
        id: "wmv.1080p",
        format: RenderFormat::Wmv,
        title: "WMV Windows Media 1080p (+ WMA)",
        summary: "Windows Media Video WMV3 + áudio WMA",
        resolution: Some((1920, 1080)),
        half_project: false,
        quality: 90,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 10_000,
        audio_bitrate_kbps: 192,
    },
    RenderTemplate {
        id: "avi.project",
        format: RenderFormat::AviMjpeg,
        title: "Igual ao projeto (Full)",
        summary: "AVI Motion-JPEG 90% + PCM 16 bits na resolução da timeline",
        resolution: None,
        half_project: false,
        quality: 90,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 12_000,
        audio_bitrate_kbps: 192,
    },
    RenderTemplate {
        id: "avi.1080p",
        format: RenderFormat::AviMjpeg,
        title: "Full HD 1080p (1920×1080)",
        summary: "AVI Motion-JPEG 92% • ideal para entrega Full HD em Rust puro",
        resolution: Some((1920, 1080)),
        half_project: false,
        quality: 92,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 12_000,
        audio_bitrate_kbps: 192,
    },
    RenderTemplate {
        id: "avi.720p",
        format: RenderFormat::AviMjpeg,
        title: "HD 720p (1280×720)",
        summary: "AVI Motion-JPEG 88% • arquivos menores e prévia rápida",
        resolution: Some((1280, 720)),
        half_project: false,
        quality: 88,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 8_000,
        audio_bitrate_kbps: 192,
    },
    RenderTemplate {
        id: "avi.4k",
        format: RenderFormat::AviMjpeg,
        title: "4K UHD (3840×2160)",
        summary: "AVI Motion-JPEG 94% • saída de alta resolução",
        resolution: Some((3840, 2160)),
        half_project: false,
        quality: 94,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 24_000,
        audio_bitrate_kbps: 192,
    },
    RenderTemplate {
        id: "avi.vertical",
        format: RenderFormat::AviMjpeg,
        title: "Vertical 9:16 (1080×1920)",
        summary: "AVI Motion-JPEG 90% para vídeos verticais",
        resolution: Some((1080, 1920)),
        half_project: false,
        quality: 90,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 12_000,
        audio_bitrate_kbps: 192,
    },
    RenderTemplate {
        id: "avi.draft",
        format: RenderFormat::AviMjpeg,
        title: "Rascunho leve (1/2 projeto, JPEG 75%)",
        summary: "Metade da resolução do projeto para revisão rápida",
        resolution: None,
        half_project: true,
        quality: 75,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 6_000,
        audio_bitrate_kbps: 128,
    },
    RenderTemplate {
        id: "avi.raw",
        format: RenderFormat::AviUncompressed,
        title: "AVI sem compactação RGB24 (Full)",
        summary: "Quadros DIB RGB24 puros sem perdas + PCM 16 bits",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 192,
    },
    RenderTemplate {
        id: "gif.half",
        format: RenderFormat::AnimatedGif,
        title: "GIF animado Web (1/2 projeto)",
        summary: "Loop infinito em GIF quantizado em metade da resolução",
        resolution: None,
        half_project: true,
        quality: 90,
        include_audio: false,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "png.project",
        format: RenderFormat::PngSequence,
        title: "Sequência PNG sem perdas (Full)",
        summary: "Quadros PNG RGB8 + áudio WAV ao lado na resolução do projeto",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm24,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "jpg.seq",
        format: RenderFormat::JpegSequence,
        title: "Sequência JPEG 90% (Full)",
        summary: "Quadros JPEG enumerados + áudio WAV ao lado",
        resolution: None,
        half_project: false,
        quality: 90,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "tiff.seq",
        format: RenderFormat::TiffSequence,
        title: "Sequência TIFF sem perdas (Full)",
        summary: "Arquivos TIFF RGB8 + WAV para gráfica e pós-produção",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm24,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "tga.seq",
        format: RenderFormat::TgaSequence,
        title: "Sequência Targa TGA com canal Alfa",
        summary: "Quadros TGA RGBA32 + WAV para composição VFX",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm24,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "bmp.seq",
        format: RenderFormat::BmpSequence,
        title: "Sequência BMP Windows (Full)",
        summary: "Bitmap RGB8 puro em sequência",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: false,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "webp.seq",
        format: RenderFormat::WebpSequence,
        title: "Sequência WebP sem perdas (Alfa)",
        summary: "Quadros WebP RGBA sem perdas",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: false,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "ppm.seq",
        format: RenderFormat::PnmSequence,
        title: "Sequência PNM / PPM P6",
        summary: "Portable Pixmap RGB bruto interoperável",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: false,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "qoi.seq",
        format: RenderFormat::QoiSequence,
        title: "Sequência QOI sem perdas (Alfa)",
        summary: "Quite OK Image ultrarrápida em RGBA",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: false,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "dpx.seq",
        format: RenderFormat::DpxSequence,
        title: "Sequência DPX SMPTE 268M (Cinema)",
        summary: "Quadros DPX SDPX v2.0 para cinema e color grading",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm24,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "exr.seq",
        format: RenderFormat::ExrSequence,
        title: "Sequência OpenEXR HDR (VFX)",
        summary: "Quadros OpenEXR com canal alfa para pipelines HDR/VFX",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Float32,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "y4m.master",
        format: RenderFormat::Y4m,
        title: "YUV4MPEG2 Master não comprimido",
        summary: "Fluxo Y4M bruto interoperável com pipelines externos + WAV",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm24,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "wav.16",
        format: RenderFormat::Wav,
        title: "WAV PCM 16 bits (Estéreo)",
        summary: "Mixagem Master sem compressão padrão CD",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "wav.24",
        format: RenderFormat::Wav,
        title: "WAV PCM 24 bits (Estúdio)",
        summary: "Maior faixa dinâmica para masterização de áudio",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm24,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "wav.32f",
        format: RenderFormat::Wav,
        title: "WAV IEEE Float 32 bits",
        summary: "Precisão de ponto flutuante sem ceifamento acima de 0 dB",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Float32,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "bwf.24",
        format: RenderFormat::Bwf,
        title: "Broadcast Wave BWF 24 bits (EBU bext)",
        summary: "WAV profissional de radiodifusão com metadados EBU Tech 3285",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm24,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "rf64.32f",
        format: RenderFormat::Rf64,
        title: "EBU RF64 Float 32 bits (Sem limite 4 GB)",
        summary: "Extensão RIFF 64 bits EBU Tech 3306 para longas metragens",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Float32,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "w64.24",
        format: RenderFormat::Wave64,
        title: "Sony Wave64 PCM 24 bits",
        summary: "Contêiner WAVE de 64 bits tradicional do ecossistema VEGAS",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm24,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "aiff.24",
        format: RenderFormat::Aiff,
        title: "Apple AIFF PCM 24 bits",
        summary: "Intercâmbio sem perdas big-endian para DAWs e macOS",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm24,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "au.16",
        format: RenderFormat::Au,
        title: "Sun / NeXT AU PCM 16 bits",
        summary: "Áudio clássico Unix/NeXT big-endian",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "caf.32f",
        format: RenderFormat::Caf,
        title: "Apple Core Audio Format (.caf)",
        summary: "Contêiner 64 bits Core Audio Format sem limite de tamanho",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Float32,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "qoa.192",
        format: RenderFormat::Qoa,
        title: "QOA (Quite OK Audio)",
        summary: "Compressão temporal de áudio aberta a 3,2 bits por amostra",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "m4a.aac",
        format: RenderFormat::AacM4a,
        title: "AAC Estéreo (.m4a, 256 kbps)",
        summary: "Áudio AAC comprimido via Media Foundation",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 256,
    },
    RenderTemplate {
        id: "wma.192",
        format: RenderFormat::Wma,
        title: "Windows Media Audio (.wma, 192 kbps)",
        summary: "Áudio WMA via Media Foundation",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 192,
    },
    RenderTemplate {
        id: "mp3.192",
        format: RenderFormat::Mp3,
        title: "MP3 Estéreo (Media Foundation)",
        summary: "Áudio MP3 via Media Foundation (quando disponível no SO)",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 192,
    },
    RenderTemplate {
        id: "flac.24",
        format: RenderFormat::Flac,
        title: "FLAC Sem Perdas (.flac)",
        summary: "Áudio comprimido sem perdas RFC 9639 em até 24 bits e 5.1",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm24,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "aac.adts",
        format: RenderFormat::AacAdts,
        title: "AAC ADTS bruto (.aac)",
        summary: "Stream AAC ADTS para broadcast e dispositivos móveis",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 192,
    },
    RenderTemplate {
        id: "dpx10.seq",
        format: RenderFormat::Dpx10Sequence,
        title: "Sequência DPX 10-bit SMPTE 268M",
        summary: "Quadros DPX 10 bits (Method A) + áudio WAV 24-bit auxiliar",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm24,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "cin.seq",
        format: RenderFormat::CineonSequence,
        title: "Sequência Kodak Cineon 10-bit (.cin)",
        summary: "Quadros Kodak Cineon v4.5 + WAV 24-bit auxiliar",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm24,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "avi.bgra",
        format: RenderFormat::AviUncompressedBgra,
        title: "AVI sem compactação BGRA 32-bit",
        summary: "Quadros BI_RGB 32 bits em Rust puro + PCM 16 bits",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "avi.uyvy",
        format: RenderFormat::AviUncompressedYuv422,
        title: "AVI sem compactação YUV 4:2:2 UYVY",
        summary: "Fluxo bruto BT.601 YUV 4:2:2 + PCM 16 bits",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "avi.r210",
        format: RenderFormat::AviUncompressedRgb10,
        title: "AVI sem compactação 10-bit r210",
        summary: "Quadros RGB 10 bits não comprimidos + PCM 16 bits",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "mov.bgra",
        format: RenderFormat::MovUncompressedRgb,
        title: "QuickTime MOV BGRA sem perdas (+ PCM)",
        summary: "Contêiner QuickTime puro BGRA 32-bit com transparência",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "mov.2vuy",
        format: RenderFormat::MovUncompressedYuv422,
        title: "QuickTime MOV YUV 4:2:2 2vuy (+ PCM)",
        summary: "Contêiner QuickTime puro YUV 4:2:2 não comprimido",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "mov.jpeg",
        format: RenderFormat::MovMjpeg,
        title: "QuickTime MOV Photo-JPEG (+ PCM)",
        summary: "Contêiner QuickTime Photo-JPEG quadro a quadro",
        resolution: None,
        half_project: false,
        quality: 92,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "mov.png",
        format: RenderFormat::MovPng,
        title: "QuickTime MOV PNG com Alfa (+ PCM)",
        summary: "Contêiner QuickTime PNG sem perdas com canal alfa",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "mov.hevc",
        format: RenderFormat::MovHevc,
        title: "QuickTime MOV HEVC / H.265 (+ AAC)",
        summary: "MOV HEVC via Media Foundation + áudio AAC",
        resolution: None,
        half_project: false,
        quality: 92,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 24_000,
        audio_bitrate_kbps: 256,
    },
    RenderTemplate {
        id: "mp4.av1",
        format: RenderFormat::Mp4Av1,
        title: "MP4 AV1 (+ AAC)",
        summary: "MP4 com codificador AV1 via Media Foundation",
        resolution: None,
        half_project: false,
        quality: 90,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 10_000,
        audio_bitrate_kbps: 192,
    },
    RenderTemplate {
        id: "mov.av1",
        format: RenderFormat::MovAv1,
        title: "QuickTime MOV AV1 (+ AAC)",
        summary: "MOV com codificador AV1 via Media Foundation",
        resolution: None,
        half_project: false,
        quality: 90,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 14_000,
        audio_bitrate_kbps: 256,
    },
    RenderTemplate {
        id: "mov.mpeg4",
        format: RenderFormat::MovMpeg4,
        title: "QuickTime MOV MPEG-4 Part 2 (+ AAC)",
        summary: "MOV MPEG-4 legado via Media Foundation",
        resolution: None,
        half_project: false,
        quality: 88,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 8_000,
        audio_bitrate_kbps: 192,
    },
    RenderTemplate {
        id: "mkv.ffv1",
        format: RenderFormat::MkvFfv1,
        title: "Matroska MKV FFV1 Sem Perdas (+ PCM)",
        summary: "Master de preservação sem perdas FFV1 v3 em MKV",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "mkv.mjpeg",
        format: RenderFormat::MkvMjpeg,
        title: "Matroska MKV Motion-JPEG (+ PCM)",
        summary: "Contêiner MKV MJPEG em Rust puro",
        resolution: None,
        half_project: false,
        quality: 92,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "mkv.png",
        format: RenderFormat::MkvPng,
        title: "Matroska MKV PNG Sem Perdas (+ PCM)",
        summary: "Contêiner MKV PNG com transparência",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    },
    RenderTemplate {
        id: "mkv.h264",
        format: RenderFormat::MkvH264,
        title: "Matroska MKV H.264 / AVC (+ AAC)",
        summary: "MKV H.264 + AAC com remux nativo via Media Foundation",
        resolution: None,
        half_project: false,
        quality: 90,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 16_000,
        audio_bitrate_kbps: 192,
    },
    RenderTemplate {
        id: "mkv.hevc",
        format: RenderFormat::MkvHevc,
        title: "Matroska MKV HEVC / H.265 (+ AAC)",
        summary: "MKV HEVC + AAC via Media Foundation",
        resolution: None,
        half_project: false,
        quality: 92,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 20_000,
        audio_bitrate_kbps: 256,
    },
    RenderTemplate {
        id: "mkv.av1",
        format: RenderFormat::MkvAv1,
        title: "Matroska MKV AV1 (+ AAC)",
        summary: "MKV AV1 + AAC via Media Foundation",
        resolution: None,
        half_project: false,
        quality: 90,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 10_000,
        audio_bitrate_kbps: 192,
    },
    RenderTemplate {
        id: "mkv.vp9",
        format: RenderFormat::MkvVp9,
        title: "Matroska MKV VP9 (+ AAC)",
        summary: "MKV VP9 + AAC via Media Foundation",
        resolution: None,
        half_project: false,
        quality: 90,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
        video_bitrate_kbps: 12_000,
        audio_bitrate_kbps: 192,
    },
];

/// Categorias exibidas na lista lateral de `Preferências...`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PrefsCategory {
    #[default]
    General,
    Appearance,
    Editing,
    VideoPreview,
    PreviewDevice,
    Audio,
    AudioDevice,
    MidiSyncControl,
    FileIo,
    Memory,
    AutoSave,
    Shortcuts,
    Integrations,
}

impl PrefsCategory {
    pub const ALL: [PrefsCategory; 13] = [
        PrefsCategory::General,
        PrefsCategory::Appearance,
        PrefsCategory::Editing,
        PrefsCategory::VideoPreview,
        PrefsCategory::PreviewDevice,
        PrefsCategory::Audio,
        PrefsCategory::AudioDevice,
        PrefsCategory::MidiSyncControl,
        PrefsCategory::FileIo,
        PrefsCategory::Memory,
        PrefsCategory::AutoSave,
        PrefsCategory::Shortcuts,
        PrefsCategory::Integrations,
    ];

    pub fn label_in(self, lang: Language) -> &'static str {
        let pt = match self {
            PrefsCategory::General => "Geral",
            PrefsCategory::Appearance => "Aparência e Layout",
            PrefsCategory::Editing => "Edição e Timeline",
            PrefsCategory::VideoPreview => "Vídeo e Prévia",
            PrefsCategory::PreviewDevice => "Dispositivo de Prévia",
            PrefsCategory::Audio => "Áudio",
            PrefsCategory::AudioDevice => "Dispositivo de Áudio",
            PrefsCategory::MidiSyncControl => "MIDI / Sync / Controle",
            PrefsCategory::FileIo => "E/S de Arquivos (File I/O)",
            PrefsCategory::Memory => "Memória e Desempenho",
            PrefsCategory::AutoSave => "Salvamento Automático",
            PrefsCategory::Shortcuts => "Atalhos e Controles",
            PrefsCategory::Integrations => "Aplicativos externos",
        };
        tr(lang, pt)
    }

    pub fn label(self) -> &'static str {
        self.label_in(Language::PtBr)
    }
}

struct SceneDetectJob {
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    rx: std::sync::mpsc::Receiver<Vec<Ticks>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum FxChooserItem {
    Native(FxKind),
    SavedChain(usize),
    Ofx {
        binary: String,
        identifier: String,
        label: String,
    },
    Vst3 {
        binary: String,
        class_id: String,
        name: String,
        is_instrument: bool,
    },
}

#[derive(Default)]
pub struct Dialogs {
    pub project_props: bool,
    pub event_props: Option<Id>,
    pub pan_crop: Option<Id>,
    pub track_motion: Option<Id>,
    pub fx_target: Option<FxTarget>,
    pub(crate) fx_chooser_open: bool,
    fx_chooser_cat: usize,
    fx_chooser_filter: String,
    fx_chooser_selected: Option<FxChooserItem>,
    fx_chooser_snapshot: Option<Vec<FxInstance>>,
    fx_selected_slot: Option<usize>,
    pub gen_editor: Option<Id>,
    pub paste_attrs_open: bool,
    pub paste_attrs_mask: neoveg_engine::session::PasteAttributesMask,
    pub take_chooser: Option<Id>,
    take_chooser_selected: Option<usize>,
    take_chooser_tex: Option<(egui::TextureHandle, (Id, usize))>,
    take_chooser_add_media: Option<Id>,
    take_chooser_name_buf: String,
    take_chooser_renamed_idx: Option<(Id, usize)>,
    pub scene_detect: Option<Id>,
    pub scene_detect_threshold: f32,
    pub scene_detect_min_frames: u32,
    pub scene_detect_markers: bool,
    pub scene_detect_cuts: Vec<Ticks>,
    pub scene_detect_analyzed: bool,
    scene_detect_job: Option<SceneDetectJob>,
    pub shortcuts: bool,
    pub prefs_open: bool,
    pub prefs_tab: PrefsCategory,
    /// Aba aberta nas Propriedades do projeto (0 Vídeo … 4 CD de áudio).
    pub project_props_tab: u8,
    prefs_draft: Option<Preferences>,
    prefs_orig: Option<(String, EditLayoutPrefs, String)>,
    prefs_orig_metrics: Option<(f32, f32)>,
    pub edit_layout_open: bool,
    edit_layout_draft: Option<EditLayoutPrefs>,
    edit_layout_orig: Option<(EditLayoutPrefs, String, f32, f32)>,
    shortcut_filter: String,
    pub capturing_shortcut: Option<&'static str>,
    pub save_layout_open: bool,
    pub save_layout_name: String,
    pub about: bool,
    pub goto_open: bool,
    goto_text: String,
    pub render: Option<RenderDialogState>,
    pub render_use_loop: bool,
    pub render_done: Option<String>,
    pub(crate) pc_tex: Option<(egui::TextureHandle, (Id, i64))>,
    /// Estado da janela Pan/Crop (ferramentas, área de trabalho, cursor).
    pub(crate) pc: crate::pan_crop_window::PanCropUi,
    /// Estado da janela Movimento da Trilha.
    pub(crate) tm: crate::track_motion_window::TrackMotionUi,
    edit_in_progress: bool,
    /// Arquivos aguardando confirmação para conversão.
    pub convert_offer: Option<crate::convert_dialog::ConvertOffer>,
    /// Conversão em andamento.
    pub convert_job: Option<crate::convert_dialog::ConvertJob>,
    /// *Editar conjunto de botões visíveis* (botão Mais do cabeçalho de
    /// faixa): `true` = faixas de vídeo, e o rascunho dos botões.
    pub header_buttons: Option<(bool, neoveg_common::prefs::HeaderButtons)>,
    pub motion_track_src: Option<Id>,
    pub motion_track_target_is_track: bool,
    pub motion_track_target_event: Option<Id>,
    pub motion_track_target_track: Option<Id>,
    pub motion_track_invert: bool,
    pub motion_track_step: usize,
    save_preset_for: Option<(FxTarget, usize)>,
    save_preset_name: String,
    save_chain_target: Option<FxTarget>,
    save_chain_name: String,
    pub customize_toolbar_open: bool,
    pub customize_toolbar_draft: Option<Vec<String>>,
    customize_toolbar_filter: String,
    pub cmd_marker_open: bool,
    pub cmd_marker_label: String,
    pub cmd_marker_type: String,
    pub cmd_marker_arg: String,
    pub work_folders_open: bool,
    pub work_folders_draft: Option<neoveg_engine::model::WorkFolders>,
}

const PRESETS: [(&str, u32, u32, FrameRate); 12] = [
    ("HD 1080p 29,97", 1920, 1080, FrameRate::FPS_29_97),
    ("HD 1080p 30", 1920, 1080, FrameRate::FPS_30),
    ("HD 1080p 25 (PAL)", 1920, 1080, FrameRate::FPS_25),
    ("HD 1080p 24", 1920, 1080, FrameRate::FPS_24),
    ("HD 1080p 60", 1920, 1080, FrameRate::FPS_60),
    ("HD 720p 29,97", 1280, 720, FrameRate::FPS_29_97),
    ("HD 720p 30", 1280, 720, FrameRate::FPS_30),
    ("4K UHD 29,97", 3840, 2160, FrameRate::FPS_29_97),
    ("4K UHD 25", 3840, 2160, FrameRate::FPS_25),
    ("Vertical 1080x1920 30", 1080, 1920, FrameRate::FPS_30),
    ("Quadrado 1080x1080 30", 1080, 1080, FrameRate::FPS_30),
    ("SD 640x480 29,97", 640, 480, FrameRate::FPS_29_97),
];

/// Aba *Vídeo* das Propriedades do projeto. Devolve `true` se algo mudou.
fn project_video_tab(
    ui: &mut egui::Ui,
    lang: neoveg_common::Language,
    s: &mut crate::model::ProjectSettings,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_label(tr(lang, "Modelo"))
        .selected_text(tr(lang, "Escolher modelo..."))
        .show_ui(ui, |ui| {
            for (name, w, h, fps) in PRESETS {
                if ui.selectable_label(false, tr(lang, name)).clicked() {
                    s.width = w;
                    s.height = h;
                    s.fps = fps;
                    changed = true;
                }
            }
        });
    egui::Grid::new("pp").num_columns(2).show(ui, |ui| {
        ui.label(tr(lang, "Largura"));
        changed |= ui
            .add(egui::DragValue::new(&mut s.width).range(16..=8192))
            .changed();
        ui.end_row();
        ui.label(tr(lang, "Altura"));
        changed |= ui
            .add(egui::DragValue::new(&mut s.height).range(16..=8192))
            .changed();
        ui.end_row();
        ui.label(tr(lang, "Quadros por segundo"));
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("fps_std")
                .selected_text(s.fps.label_in(lang))
                .show_ui(ui, |ui| {
                    for f in FrameRate::STANDARD {
                        changed |= ui
                            .selectable_value(&mut s.fps, f, f.label_in(lang))
                            .changed();
                    }
                });
            let (mut num, mut den) = (s.fps.num, s.fps.den);
            let c_n = ui
                .add(egui::DragValue::new(&mut num).range(1..=240_000))
                .changed();
            ui.label("/");
            let c_d = ui
                .add(egui::DragValue::new(&mut den).range(1..=1001))
                .changed();
            if c_n || c_d {
                if let Some(fr) = FrameRate::new(num, den) {
                    let fv = fr.as_f64();
                    if (1.0..=240.0).contains(&fv) {
                        s.fps = fr;
                        changed = true;
                    }
                }
            }
            ui.add_enabled_ui(s.fps.is_ntsc_drop_eligible(), |ui| {
                changed |= ui
                    .checkbox(&mut s.drop_frame, tr(lang, "Drop-frame (;)"))
                    .on_hover_text(tr(lang, "SMPTE Drop-Frame para 29,97 / 59,94 qps"))
                    .changed();
            });
        });
        ui.end_row();
        ui.label(tr(lang, "Ordem dos campos"));
        egui::ComboBox::from_id_salt("pp_field_order")
            .selected_text(s.field_order.name_in(lang))
            .show_ui(ui, |ui| {
                for f in FieldOrder::ALL {
                    changed |= ui
                        .selectable_value(&mut s.field_order, f, f.name_in(lang))
                        .changed();
                }
            });
        ui.end_row();
        ui.label(tr(lang, "Proporção do pixel"));
        changed |= ui
            .add(
                egui::DragValue::new(&mut s.pixel_aspect)
                    .range(0.1..=4.0)
                    .speed(0.001)
                    .fixed_decimals(4),
            )
            .changed();
        ui.end_row();
        ui.label(tr(lang, "Formato de pixel"));
        egui::ComboBox::from_id_salt("pixel_format")
            .selected_text(s.pixel_format.label())
            .show_ui(ui, |ui| {
                for pf in neoveg_engine::color::PixelFormat::ALL {
                    changed |= ui
                        .selectable_value(&mut s.pixel_format, pf, pf.label())
                        .changed();
                }
            });
        ui.end_row();
        ui.label(tr(lang, "Gama de composição"));
        egui::ComboBox::from_id_salt("compositing_gamma")
            .selected_text(s.compositing_gamma.label())
            .show_ui(ui, |ui| {
                for cg in neoveg_engine::color::CompositingGamma::ALL {
                    changed |= ui
                        .selectable_value(&mut s.compositing_gamma, cg, cg.label())
                        .changed();
                }
            });
        ui.end_row();
        ui.label(tr(lang, "Espaço de cor (ACES)"));
        egui::ComboBox::from_id_salt("project_color_space")
            .selected_text(s.color_space.label())
            .show_ui(ui, |ui| {
                for cs in neoveg_engine::color::ProjectColorSpace::ALL {
                    changed |= ui
                        .selectable_value(&mut s.color_space, cs, cs.label())
                        .changed();
                }
            });
        ui.end_row();
        ui.label(tr(lang, "Transformação de visualização"));
        egui::ComboBox::from_id_salt("view_transform")
            .selected_text(s.view_transform.label())
            .show_ui(ui, |ui| {
                for vt in neoveg_engine::color::ViewTransform::ALL {
                    changed |= ui
                        .selectable_value(&mut s.view_transform, vt, vt.label())
                        .changed();
                }
            });
        ui.end_row();
        ui.label(tr(lang, "Qualidade de renderização em resolução total"));
        egui::ComboBox::from_id_salt("pp_render_quality")
            .selected_text(s.render_quality.name_in(lang))
            .show_ui(ui, |ui| {
                for q in RenderQuality::ALL {
                    changed |= ui
                        .selectable_value(&mut s.render_quality, q, q.name_in(lang))
                        .changed();
                }
            });
        ui.end_row();
        ui.label(tr(lang, "Método de desentrelaçamento"));
        egui::ComboBox::from_id_salt("pp_deinterlace")
            .selected_text(s.deinterlace.name_in(lang))
            .show_ui(ui, |ui| {
                for d in DeinterlaceMethod::ALL {
                    changed |= ui
                        .selectable_value(&mut s.deinterlace, d, d.name_in(lang))
                        .changed();
                }
            });
        ui.end_row();
        ui.label(tr(lang, "Modo 3D Estereoscópico"));
        egui::ComboBox::from_id_salt("stereo_3d_mode")
            .selected_text(s.stereo_3d.label())
            .show_ui(ui, |ui| {
                for m3d in Stereo3dMode::ALL {
                    changed |= ui
                        .selectable_value(&mut s.stereo_3d, m3d, m3d.label())
                        .changed();
                }
            });
        ui.end_row();
        if s.stereo_3d != Stereo3dMode::Off {
            ui.label(tr(lang, "Distância interocular 3D"));
            changed |= ui
                .add(
                    egui::DragValue::new(&mut s.stereo_3d_interocular)
                        .range(0.0..=0.25)
                        .speed(0.002)
                        .fixed_decimals(3),
                )
                .changed();
            ui.end_row();
            ui.label(tr(lang, "Convergência 3D (Z)"));
            changed |= ui
                .add(
                    egui::DragValue::new(&mut s.stereo_3d_convergence)
                        .range(-1.0..=1.0)
                        .speed(0.01)
                        .fixed_decimals(3),
                )
                .changed();
            ui.end_row();
        }
        ui.label(tr(lang, "Cor de fundo"));
        changed |= ui.color_edit_button_rgb(&mut s.background).changed();
        ui.end_row();
    });
    changed |= ui
        .checkbox(
            &mut s.adapt_source_media,
            tr(
                lang,
                "Ajustar a mídia de origem para combinar com o projeto",
            ),
        )
        .changed();
    s.width = (s.width / 2).max(8) * 2;
    s.height = (s.height / 2).max(8) * 2;
    ui.separator();
    ui.label(
        egui::RichText::new(tr(
            lang,
            "Dimensões pares são exigidas pelos formatos 4:2:0.",
        ))
        .small()
        .weak(),
    );
    changed
}

impl Dialogs {
    pub fn open_prefs(&mut self, current: &Preferences) {
        self.prefs_draft = Some(current.clone());
        self.prefs_orig = Some((
            current.theme.clone(),
            current.edit_layout.clone(),
            current.language.clone(),
        ));
        self.prefs_orig_metrics = None;
        self.prefs_open = true;
    }

    pub fn open_edit_layout(
        &mut self,
        current: &Preferences,
        dock_height: f32,
        preview_ratio: f32,
    ) {
        self.edit_layout_draft = Some(current.edit_layout.clone());
        self.edit_layout_orig = Some((
            current.edit_layout.clone(),
            current.theme.clone(),
            dock_height,
            preview_ratio,
        ));
        self.edit_layout_open = true;
    }

    pub fn open_render(&mut self, p: &Project, proj_path: Option<&Path>) {
        let render_dir = p.work_folders.effective_render_exports(proj_path);
        let base = proj_path
            .and_then(|pp| pp.file_stem())
            .map(|stem| render_dir.join(format!("{}.avi", stem.to_string_lossy())))
            .unwrap_or_else(|| render_dir.join("render.avi"));
        let (
            path,
            format,
            template_id,
            filter,
            only_favorites,
            quality,
            include_audio,
            wav_bits,
            video_bitrate_kbps,
            audio_bitrate_kbps,
        ) = match &self.render {
            Some(r) => (
                r.path.clone(),
                r.format,
                r.template_id.clone(),
                r.filter.clone(),
                r.only_favorites,
                r.quality,
                r.include_audio,
                r.wav_bits,
                r.video_bitrate_kbps,
                r.audio_bitrate_kbps,
            ),
            None => (
                base.to_string_lossy().to_string(),
                RenderFormat::AviMjpeg,
                "avi.project".to_string(),
                String::new(),
                false,
                90,
                true,
                WavBits::Pcm16,
                12_000,
                192,
            ),
        };
        self.render = Some(RenderDialogState {
            format,
            template_id,
            filter,
            only_favorites,
            path,
            width: p.settings.width,
            height: p.settings.height,
            quality,
            include_audio,
            wav_bits,
            video_bitrate_kbps,
            audio_bitrate_kbps,
        });
        self.render_use_loop = p.loop_region.is_some() && self.render_use_loop;
    }
}

fn time_field(
    ui: &mut egui::Ui,
    value: &mut Ticks,
    fps: FrameRate,
    sample_rate: u32,
    fmt: TimeFormat,
    id: impl std::hash::Hash,
) -> bool {
    let key = egui::Id::new(id);
    let mut text = ui
        .data_mut(|d| d.get_temp::<String>(key))
        .unwrap_or_else(|| format_ticks(*value, fps, sample_rate, fmt));
    let r = ui.add(
        egui::TextEdit::singleline(&mut text)
            .desired_width(110.0)
            .font(egui::TextStyle::Monospace),
    );
    let mut changed = false;
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(key, text.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(key));
    }
    if r.lost_focus() {
        if let Some(t) = parse_time(&text, fps, sample_rate, fmt) {
            let t = t.max(Ticks::ZERO);
            if t != *value {
                *value = t;
                changed = true;
            }
        }
        ui.data_mut(|d| d.remove::<String>(key));
    }
    changed
}

fn curve_combo(ui: &mut egui::Ui, c: &mut Curve, id: impl std::hash::Hash, lang: Language) -> bool {
    let before = *c;
    egui::ComboBox::from_id_salt(id)
        .selected_text(c.name_in(lang))
        .show_ui(ui, |ui| {
            for k in Curve::ALL.iter().take(5) {
                ui.selectable_value(c, *k, k.name_in(lang));
            }
        });
    *c != before
}

impl App {
    /// Registra desfazer para edições feitas em diálogos (uma vez por gesto).
    pub(crate) fn dialog_change(&mut self) {
        if !self.dlg.edit_in_progress {
            let snap = (*self.session.snapshot).clone();
            self.session.undo.push(snap);
            self.session.redo.clear();
            self.dlg.edit_in_progress = true;
        }
        self.changed();
    }

    pub fn show_dialogs(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.pointer.any_down()) && !ctx.wants_keyboard_input() {
            self.dlg.edit_in_progress = false;
        }
        self.project_props_window(ctx);
        self.event_props_window(ctx);
        self.pan_crop_window(ctx);
        self.track_motion_window(ctx);
        self.fx_window(ctx);
        self.gen_editor_window(ctx);
        self.paste_attributes_window(ctx);
        self.take_chooser_window(ctx);
        self.scene_detect_window(ctx);
        self.motion_track_window(ctx);
        self.render_window(ctx);
        self.render_progress_window(ctx);
        self.convert_windows(ctx);
        self.header_buttons_window(ctx);
        self.event_buttons_window(ctx);
        self.customize_toolbar_window(ctx);
        self.command_marker_window(ctx);
        crate::dlg_work_folders::show_work_folders_window(self, ctx);
        self.misc_windows(ctx);
    }

    fn customize_toolbar_window(&mut self, ctx: &egui::Context) {
        if !self.dlg.customize_toolbar_open {
            return;
        }
        let Some(mut draft) = self.dlg.customize_toolbar_draft.take() else {
            self.dlg.customize_toolbar_open = false;
            return;
        };
        let mut open = true;
        let mut save_close = false;
        let mut cancel_close = false;
        let mut restore_def = false;
        crate::dialog_window::DialogWindow::new("Personalizar barra de ferramentas")
            .id(egui::Id::new("customize_toolbar_win"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(600.0)
            .default_height(420.0)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.screen_rect().center())
            .show(ctx, |ui| {
                ui.label("Escolha quais comandos da Tabela Única aparecem na barra de ferramentas principal (VEGAS Pro p. 75).");
                ui.horizontal(|ui| {
                    ui.label("Filtrar:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.dlg.customize_toolbar_filter)
                            .desired_width(220.0),
                    );
                    if ui.button("＋ Separador (|)").clicked()
                        && draft.len() < neoveg_common::prefs::MAX_TOOLBAR_BUTTONS
                    {
                        draft.push("|".into());
                    }
                });
                ui.separator();
                let q = self.dlg.customize_toolbar_filter.trim().to_lowercase();
                ui.columns(2, |cols| {
                    cols[0].strong("Comandos disponíveis");
                    egui::ScrollArea::vertical()
                        .id_salt("ctb_avail")
                        .max_height(260.0)
                        .show(&mut cols[0], |ui| {
                            for cmd in COMMANDS {
                                if !q.is_empty()
                                    && !cmd.label.to_lowercase().contains(&q)
                                    && !cmd.id.to_lowercase().contains(&q)
                                {
                                    continue;
                                }
                                ui.horizontal(|ui| {
                                    let can_add = draft.len()
                                        < neoveg_common::prefs::MAX_TOOLBAR_BUTTONS;
                                    if ui
                                        .add_enabled(can_add, egui::Button::new("Adicionar →").small())
                                        .clicked()
                                    {
                                        draft.push(cmd.id.to_string());
                                    }
                                    ui.label(egui::RichText::new(cmd.label).small())
                                        .on_hover_text(cmd.id);
                                });
                            }
                        });

                    cols[1].strong(format!("Barra ativa ({})", draft.len()));
                    let mut move_up: Option<usize> = None;
                    let mut move_down: Option<usize> = None;
                    let mut remove_idx: Option<usize> = None;
                    egui::ScrollArea::vertical()
                        .id_salt("ctb_active")
                        .max_height(260.0)
                        .show(&mut cols[1], |ui| {
                            for (i, id) in draft.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    let lbl = if id == "|" {
                                        "── Separador ──"
                                    } else {
                                        neoveg_engine::commands::find_command(id)
                                            .map(|c| c.label)
                                            .unwrap_or(id.as_str())
                                    };
                                    if i > 0 && ui.small_button("↑").clicked() {
                                        move_up = Some(i);
                                    }
                                    if i + 1 < draft.len() && ui.small_button("↓").clicked() {
                                        move_down = Some(i);
                                    }
                                    if ui.small_button("×").clicked() {
                                        remove_idx = Some(i);
                                    }
                                    ui.label(egui::RichText::new(lbl).small());
                                });
                            }
                        });
                    if let Some(i) = move_up {
                        draft.swap(i, i - 1);
                    }
                    if let Some(i) = move_down {
                        draft.swap(i, i + 1);
                    }
                    if let Some(i) = remove_idx {
                        draft.remove(i);
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Restaurar padrão").clicked() {
                        restore_def = true;
                    }
                    if ui.button("OK").clicked() {
                        save_close = true;
                    }
                    if ui.button("Cancelar").clicked() {
                        cancel_close = true;
                    }
                });
            });

        if restore_def {
            draft = neoveg_common::prefs::default_toolbar_buttons();
        }
        if save_close {
            self.prefs.toolbar_buttons = draft;
            self.prefs.sanitize();
            let _ = self.prefs.save();
            self.dlg.customize_toolbar_open = false;
            self.set_status("Barra de ferramentas atualizada.");
        } else if !open || cancel_close {
            self.dlg.customize_toolbar_open = false;
        } else {
            self.dlg.customize_toolbar_draft = Some(draft);
        }
    }

    fn command_marker_window(&mut self, ctx: &egui::Context) {
        if !self.dlg.cmd_marker_open {
            return;
        }
        let mut open = true;
        let mut insert_now = false;
        let mut cancel_now = false;
        crate::dialog_window::DialogWindow::new("Inserir marcador de comando")
            .id(egui::Id::new("command_marker_win"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.screen_rect().center())
            .show(ctx, |ui| {
                ui.label("Insere um marcador de comando no cursor (TEXT, URL, CHAPTER ou personalizado, p. 725).");
                ui.separator();
                egui::Grid::new("cmd_marker_grid").num_columns(2).show(ui, |ui| {
                    ui.label("Rótulo:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.dlg.cmd_marker_label)
                            .desired_width(210.0),
                    );
                    ui.end_row();
                    ui.label("Comando:");
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("cmd_mk_preset")
                            .selected_text(if self.dlg.cmd_marker_type.is_empty() {
                                "TEXT"
                            } else {
                                self.dlg.cmd_marker_type.as_str()
                            })
                            .show_ui(ui, |ui| {
                                for k in ["TEXT", "URL", "WM_CAPTION", "CHAPTER"] {
                                    ui.selectable_value(
                                        &mut self.dlg.cmd_marker_type,
                                        k.to_string(),
                                        k,
                                    );
                                }
                            });
                        ui.add(
                            egui::TextEdit::singleline(&mut self.dlg.cmd_marker_type)
                                .desired_width(90.0),
                        );
                    });
                    ui.end_row();
                    ui.label("Parâmetro:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.dlg.cmd_marker_arg)
                            .desired_width(210.0),
                    );
                    ui.end_row();
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Inserir marcador").clicked() {
                        insert_now = true;
                    }
                    if ui.button("Cancelar").clicked() {
                        cancel_now = true;
                    }
                });
            });

        if insert_now {
            self.session.insert_marker_kind(
                MarkerKind::Command,
                &self.dlg.cmd_marker_label,
                &self.dlg.cmd_marker_type,
                &self.dlg.cmd_marker_arg,
            );
            self.sync_after_session();
            self.dlg.cmd_marker_open = false;
        } else if !open || cancel_now {
            self.dlg.cmd_marker_open = false;
        }
    }

    /// *Editar conjunto de botões visíveis* (manual do VEGAS Pro 21, p. 69):
    /// quais botões aparecem no cabeçalho das faixas de vídeo ou de áudio.
    fn header_buttons_window(&mut self, ctx: &egui::Context) {
        use neoveg_common::prefs::HeaderButtons;
        let Some((video, mut draft)) = self.dlg.header_buttons else {
            return;
        };
        let lang = self.lang();
        let mut open = true;
        let mut done: Option<bool> = None;
        let title = if video {
            tr(lang, "Botões visíveis: faixas de vídeo")
        } else {
            tr(lang, "Botões visíveis: faixas de áudio")
        };
        crate::dialog_window::DialogWindow::new(title)
            .id(egui::Id::new("header_buttons_win"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.screen_rect().center())
            .show(ctx, |ui| {
                ui.checkbox(&mut draft.mute, tr(lang, "Mudo"));
                ui.checkbox(&mut draft.solo, tr(lang, "Solo"));
                ui.checkbox(&mut draft.fx, tr(lang, "Efeitos da trilha (fx)"));
                if video {
                    ui.checkbox(
                        &mut draft.track_motion,
                        tr(lang, "Movimento da trilha (Track Motion)"),
                    );
                    ui.checkbox(
                        &mut draft.compositing_child,
                        tr(lang, "Tornar filho de composição (Pai/Filho)"),
                    );
                } else {
                    ui.checkbox(&mut draft.record_arm, tr(lang, "Armar para gravação"));
                }
                ui.checkbox(
                    &mut draft.height,
                    tr(lang, "Minimizar / Maximizar a altura"),
                );
                ui.label(
                    egui::RichText::new(tr(lang, "Os botões ocultos continuam no botão Mais (…)."))
                        .small()
                        .weak(),
                );
                ui.horizontal(|ui| {
                    if ui.button(tr(lang, "Mostrar todos")).clicked() {
                        draft = HeaderButtons::all(true);
                    }
                    if ui.button(tr(lang, "Ocultar todos")).clicked() {
                        draft = HeaderButtons::all(false);
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(tr(lang, "OK")).clicked() {
                        done = Some(true);
                    }
                    if ui.button(tr(lang, "Cancelar")).clicked() {
                        done = Some(false);
                    }
                });
            });
        match done {
            Some(true) => {
                self.prefs.track_header.set(video, draft);
                let _ = self.prefs.save();
                self.sync_header_width_to_buttons();
                self.dlg.header_buttons = None;
            }
            Some(false) => self.dlg.header_buttons = None,
            None if !open => self.dlg.header_buttons = None,
            None => self.dlg.header_buttons = Some((video, draft)),
        }
    }

    fn event_buttons_window(&mut self, ctx: &egui::Context) {
        let Some(is_video) = self.event_buttons_dlg_video else {
            return;
        };
        let lang = self.lang();
        let mut open = true;
        let mut close_now = false;
        let list = if is_video {
            &mut self.prefs.event_hidden_buttons_video
        } else {
            &mut self.prefs.event_hidden_buttons_audio
        };
        let mut show_fx = !list.iter().any(|k| k == "fx");
        let mut show_pc = !list.iter().any(|k| k == "pan_crop");
        let mut show_dur = !list.iter().any(|k| k == "duration");
        let title = if is_video {
            tr(lang, "Botões visíveis nos eventos de vídeo")
        } else {
            tr(lang, "Botões visíveis nos eventos de áudio")
        };
        let mut changed = false;
        crate::dialog_window::DialogWindow::new(title)
            .id(egui::Id::new("event_buttons_win"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.screen_rect().center())
            .show(ctx, |ui| {
                if ui
                    .checkbox(&mut show_fx, tr(lang, "Efeitos do evento (fx)"))
                    .changed()
                {
                    changed = true;
                }
                if is_video
                    && ui
                        .checkbox(&mut show_pc, tr(lang, "Pan/Crop do evento"))
                        .changed()
                {
                    changed = true;
                }
                if ui
                    .checkbox(&mut show_dur, tr(lang, "Duração no cabeçalho do evento"))
                    .changed()
                {
                    changed = true;
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(tr(lang, "Mostrar todos")).clicked() {
                        show_fx = true;
                        show_pc = true;
                        show_dur = true;
                        changed = true;
                    }
                    if ui.button(tr(lang, "Fechar")).clicked() {
                        close_now = true;
                    }
                });
            });
        if changed {
            let list = if is_video {
                &mut self.prefs.event_hidden_buttons_video
            } else {
                &mut self.prefs.event_hidden_buttons_audio
            };
            list.clear();
            if !show_fx {
                list.push("fx".into());
            }
            if is_video && !show_pc {
                list.push("pan_crop".into());
            }
            if !show_dur {
                list.push("duration".into());
            }
            self.prefs.sanitize();
            let _ = self.prefs.save();
        }
        if !open || close_now {
            self.event_buttons_dlg_video = None;
        }
    }

    fn paste_attributes_window(&mut self, ctx: &egui::Context) {
        if !self.dlg.paste_attrs_open {
            return;
        }
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        let mut mask = self.dlg.paste_attrs_mask;
        crate::dialog_window::DialogWindow::new("Colar atributos do evento")
            .id(egui::Id::new("paste_attrs_win"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.screen_rect().center())
            .show(ctx, |ui| {
                ui.label("Escolha os atributos da área de transferência para aplicar aos eventos selecionados:");
                ui.separator();
                ui.checkbox(&mut mask.fx, "Cadeia de efeitos (Video FX / Audio FX)");
                ui.checkbox(&mut mask.pan_crop, "Pan/Crop (quadros-chave)");
                ui.checkbox(&mut mask.gain_db, "Ganho de áudio (dB)");
                ui.checkbox(&mut mask.rate, "Taxa de reprodução (Velocidade)");
                ui.checkbox(&mut mask.pitch, "Tom (Pitch Shift / Preservar tom)");
                ui.checkbox(&mut mask.fades, "Fades de entrada e saída");
                ui.checkbox(&mut mask.switches, "Opções (Mudo, Loop, Reverso, Proporção)");
                ui.checkbox(&mut mask.transition, "Transição de entrada");
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Marcar tudo").clicked() {
                        mask = neoveg_engine::session::PasteAttributesMask::ALL;
                    }
                    if ui.button("Desmarcar tudo").clicked() {
                        mask = neoveg_engine::session::PasteAttributesMask {
                            fx: false,
                            pan_crop: false,
                            gain_db: false,
                            rate: false,
                            pitch: false,
                            fades: false,
                            switches: false,
                            transition: false,
                        };
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    let can_apply =
                        mask.any() && !self.clipboard.is_empty() && !self.sel_events.is_empty();
                    if ui
                        .add_enabled(can_apply, egui::Button::new("Aplicar"))
                        .clicked()
                    {
                        apply = true;
                    }
                    if ui.button("Cancelar").clicked() {
                        cancel = true;
                    }
                });
            });
        self.dlg.paste_attrs_mask = mask;
        if apply {
            self.session.paste_attributes(mask);
            self.sync_after_session();
            self.dlg.paste_attrs_open = false;
        } else if !open || cancel {
            self.dlg.paste_attrs_open = false;
        }
    }

    /// Diálogo **Take Chooser** (manual do VEGAS Pro 21, p. 347): lista os takes
    /// do evento selecionado, mostra miniatura de pré-visualização, permite
    /// renomear, reordenar, adicionar mídia como take e definir o take ativo.
    fn take_chooser_window(&mut self, ctx: &egui::Context) {
        use neoveg_engine::{model::Take, takes};

        let Some(id) = self.dlg.take_chooser else {
            return;
        };
        let Some((ti, ei)) = self.project.find_event(id) else {
            self.dlg.take_chooser = None;
            self.dlg.take_chooser_tex = None;
            return;
        };
        let is_video = self.project.tracks[ti].kind == TrackKind::Video;
        let track_locked = self.project.tracks[ti].locked;
        let ev = self.project.tracks[ti].events[ei].clone();
        let list = takes::takes(&ev);
        let active = takes::active_index(&ev);
        let sel_idx = self
            .dlg
            .take_chooser_selected
            .filter(|&i| i < list.len())
            .unwrap_or(active);
        if self.dlg.take_chooser_renamed_idx != Some((id, sel_idx)) {
            self.dlg.take_chooser_name_buf = list
                .get(sel_idx)
                .map(|t| t.name.clone())
                .unwrap_or_default();
            self.dlg.take_chooser_renamed_idx = Some((id, sel_idx));
        }

        // Prévia visual do take selecionado (para faixas de vídeo)
        if is_video {
            let key = (id, sel_idx);
            let need_tex = self
                .dlg
                .take_chooser_tex
                .as_ref()
                .is_none_or(|(_, k)| *k != key);
            if need_tex {
                if let Some(tk) = list.get(sel_idx) {
                    if let Some(m) = self.project.media(tk.media_id).cloned() {
                        let local = if (self.cursor >= ev.start && self.cursor <= ev.end())
                            && ev.length > Ticks::ZERO
                        {
                            self.cursor - ev.start
                        } else {
                            ev.length.div_i64(2)
                        };
                        let raw_t = tk.offset + ev.rate.scale_ticks(local);
                        let mt = match m.duration {
                            Some(d) if d > Ticks::ZERO => raw_t.clamp_safe(Ticks::ZERO, d),
                            _ => raw_t.max(Ticks::ZERO),
                        };
                        if let Some(img) = self.media.video_frame(&m, mt, 288, 162) {
                            let ci = super::rgba_to_color_image(&img);
                            let tex = ctx.load_texture(
                                "take_chooser_preview",
                                ci,
                                egui::TextureOptions::LINEAR,
                            );
                            self.dlg.take_chooser_tex = Some((tex, key));
                        } else {
                            self.dlg.take_chooser_tex = None;
                        }
                    }
                }
            }
        } else {
            self.dlg.take_chooser_tex = None;
        }

        let sel_take_opt = list.get(sel_idx).cloned();
        let sel_media_info = sel_take_opt
            .as_ref()
            .and_then(|tk| self.project.media(tk.media_id))
            .map(|m| (m.name.clone(), m.info.clone()));
        let add_candidates: Vec<(Id, String)> = self
            .project
            .media
            .iter()
            .filter(|m| if is_video { m.has_video } else { m.has_audio })
            .map(|m| (m.id, m.name.clone()))
            .collect();
        if self
            .dlg
            .take_chooser_add_media
            .is_none_or(|mid| !add_candidates.iter().any(|(cid, _)| *cid == mid))
        {
            self.dlg.take_chooser_add_media = add_candidates.first().map(|(cid, _)| *cid);
        }

        let mut open = true;
        let mut new_sel = sel_idx;
        let mut activate_idx: Option<usize> = None;
        let mut move_delta: Option<isize> = None;
        let mut delete_idx: Option<usize> = None;
        let mut rename_apply: Option<(usize, String)> = None;
        let mut add_media: Option<Id> = None;
        let mut close_dlg = false;

        let editable = !track_locked && !ev.locked;
        crate::dialog_window::DialogWindow::new(format!("Escolher take — {}", ev.name))
            .id(egui::Id::new("take_chooser_win"))
            .open(&mut open)
            .resizable(true)
            .default_size([560.0, 360.0])
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.screen_rect().center())
            .show(ctx, |ui| {
                ui.columns(2, |cols| {
                    let left = &mut cols[0];
                    left.label(
                        egui::RichText::new(format!(
                            "Takes do evento ({} de {})",
                            active + 1,
                            list.len()
                        ))
                        .strong(),
                    );
                    left.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .id_salt("take_chooser_list")
                        .max_height(190.0)
                        .show(left, |ui| {
                            for (idx, tk) in list.iter().enumerate() {
                                let name = takes::display_name(tk, |mid| {
                                    self.project.media(mid).map(|m| m.name.clone())
                                });
                                let tag = if idx == active { " [Ativo]" } else { "" };
                                let resp = ui.selectable_label(
                                    idx == sel_idx,
                                    format!("{}. {name}{tag}", idx + 1),
                                );
                                if resp.clicked() {
                                    new_sel = idx;
                                }
                                if resp.double_clicked() && editable {
                                    new_sel = idx;
                                    activate_idx = Some(idx);
                                    close_dlg = true;
                                }
                            }
                        });
                    left.add_space(4.0);
                    left.horizontal_wrapped(|ui| {
                        if ui
                            .add_enabled(editable && sel_idx != active, egui::Button::new("Ativar"))
                            .clicked()
                        {
                            activate_idx = Some(sel_idx);
                        }
                        if ui
                            .add_enabled(editable && sel_idx > 0, egui::Button::new("Subir"))
                            .clicked()
                        {
                            move_delta = Some(-1);
                        }
                        if ui
                            .add_enabled(
                                editable && sel_idx + 1 < list.len(),
                                egui::Button::new("Descer"),
                            )
                            .clicked()
                        {
                            move_delta = Some(1);
                        }
                        if ui
                            .add_enabled(editable && list.len() > 1, egui::Button::new("Excluir"))
                            .clicked()
                        {
                            delete_idx = Some(sel_idx);
                        }
                    });
                    if !add_candidates.is_empty() {
                        left.separator();
                        left.label(egui::RichText::new("Adicionar mídia como take:").small());
                        let sel_add_label = self
                            .dlg
                            .take_chooser_add_media
                            .and_then(|mid| {
                                add_candidates
                                    .iter()
                                    .find(|(cid, _)| *cid == mid)
                                    .map(|(_, n)| n.as_str())
                            })
                            .unwrap_or("Selecionar mídia...");
                        left.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("take_chooser_add_combo")
                                .selected_text(sel_add_label)
                                .width(155.0)
                                .show_ui(ui, |ui| {
                                    for (cid, cname) in &add_candidates {
                                        ui.selectable_value(
                                            &mut self.dlg.take_chooser_add_media,
                                            Some(*cid),
                                            cname,
                                        );
                                    }
                                });
                            if ui
                                .add_enabled(
                                    editable && self.dlg.take_chooser_add_media.is_some(),
                                    egui::Button::new("+ Take"),
                                )
                                .clicked()
                            {
                                add_media = self.dlg.take_chooser_add_media;
                            }
                        });
                    }

                    let right = &mut cols[1];
                    right.label(egui::RichText::new("Prévia e propriedades").strong());
                    right.add_space(4.0);
                    let preview_size = egui::vec2(240.0, 135.0);
                    let (prev_rect, _) =
                        right.allocate_exact_size(preview_size, egui::Sense::hover());
                    right.painter().rect_filled(
                        prev_rect,
                        2.0,
                        egui::Color32::from_rgb(18, 20, 24),
                    );
                    if let Some((tex, key)) = &self.dlg.take_chooser_tex {
                        if *key == (id, sel_idx) {
                            let ts = tex.size_vec2();
                            let scale = (prev_rect.width() / ts.x.max(1.0))
                                .min(prev_rect.height() / ts.y.max(1.0));
                            let draw_sz = ts * scale;
                            let draw_r = egui::Rect::from_center_size(prev_rect.center(), draw_sz);
                            right.painter().image(
                                tex.id(),
                                draw_r,
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                egui::Color32::WHITE,
                            );
                        }
                    } else {
                        let msg = if is_video {
                            "(Sem quadro decodificado)"
                        } else {
                            "Evento de áudio"
                        };
                        right.painter().text(
                            prev_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            msg,
                            egui::FontId::proportional(11.0),
                            self.theme.text_muted,
                        );
                    }
                    right.add_space(6.0);
                    if let Some((mname, minfo)) = &sel_media_info {
                        right.label(egui::RichText::new(format!("Mídia: {mname}")).small());
                        if !minfo.is_empty() {
                            right.label(
                                egui::RichText::new(minfo)
                                    .small()
                                    .color(self.theme.text_muted),
                            );
                        }
                    }
                    if let Some(tk) = &sel_take_opt {
                        right.label(
                            egui::RichText::new(format!("Offset: {}", self.fmt_time(tk.offset)))
                                .small(),
                        );
                    }
                    right.add_space(4.0);
                    right.horizontal(|ui| {
                        ui.label("Nome:");
                        let te = ui.add_enabled(
                            editable,
                            egui::TextEdit::singleline(&mut self.dlg.take_chooser_name_buf)
                                .desired_width(130.0),
                        );
                        let enter_pressed =
                            te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if (enter_pressed
                            || ui
                                .add_enabled(editable, egui::Button::new("Salvar"))
                                .clicked())
                            && editable
                        {
                            rename_apply =
                                Some((sel_idx, self.dlg.take_chooser_name_buf.trim().to_string()));
                        }
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.add_enabled(editable, egui::Button::new("OK")).clicked() {
                        activate_idx = Some(new_sel);
                        close_dlg = true;
                    }
                    if ui.button("Fechar").clicked() {
                        close_dlg = true;
                    }
                });
            });

        self.dlg.take_chooser_selected = Some(new_sel);

        if let Some((ridx, new_name)) = rename_apply {
            let mut updated = list.clone();
            if let Some(tk) = updated.get_mut(ridx) {
                if tk.name != new_name {
                    tk.name = new_name;
                    self.checkpoint();
                    if let Some(ev_mut) = self.project.event_mut(id) {
                        takes::replace_takes(ev_mut, updated, active);
                    }
                    self.changed();
                }
            }
        }
        if let Some(delta) = move_delta {
            let target = sel_idx as isize + delta;
            if target >= 0 && (target as usize) < list.len() {
                let tidx = target as usize;
                let mut reordered = list.clone();
                reordered.swap(sel_idx, tidx);
                let new_active = if active == sel_idx {
                    tidx
                } else if active == tidx {
                    sel_idx
                } else {
                    active
                };
                self.checkpoint();
                if let Some(ev_mut) = self.project.event_mut(id) {
                    takes::replace_takes(ev_mut, reordered, new_active);
                }
                self.dlg.take_chooser_selected = Some(tidx);
                self.dlg.take_chooser_renamed_idx = None;
                self.dlg.take_chooser_tex = None;
                self.changed();
            }
        }
        if let Some(didx) = delete_idx {
            self.checkpoint();
            let removed = self
                .project
                .event_mut(id)
                .is_some_and(|ev_mut| takes::delete_take(ev_mut, didx));
            if removed {
                let next_sel = didx.min(list.len().saturating_sub(2));
                self.dlg.take_chooser_selected = Some(next_sel);
                self.dlg.take_chooser_renamed_idx = None;
                self.dlg.take_chooser_tex = None;
                self.changed();
            } else {
                self.undo.pop();
            }
        }
        if let Some(mid) = add_media {
            if let Some(m) = self.project.media(mid).cloned() {
                let tk = Take {
                    media_id: m.id,
                    offset: Ticks::ZERO,
                    name: m.name,
                };
                self.checkpoint();
                let added = self
                    .project
                    .event_mut(id)
                    .is_some_and(|ev_mut| takes::add_take(ev_mut, tk, true));
                if added {
                    let next_active = list.len();
                    self.dlg.take_chooser_selected = Some(next_active);
                    self.dlg.take_chooser_renamed_idx = None;
                    self.dlg.take_chooser_tex = None;
                    self.changed();
                } else {
                    self.undo.pop();
                }
            }
        }
        if let Some(aidx) = activate_idx {
            if aidx != active {
                self.session.set_active_take(id, aidx);
                self.sync_after_session();
            }
        }
        if !open || close_dlg {
            self.dlg.take_chooser = None;
            self.dlg.take_chooser_tex = None;
            self.dlg.take_chooser_renamed_idx = None;
        }
    }

    pub(crate) fn open_scene_detect_for(&mut self, event_id: Id) {
        self.dlg.scene_detect = Some(event_id);
        if self.dlg.scene_detect_threshold < 5.0 {
            self.dlg.scene_detect_threshold = 35.0;
        }
        if self.dlg.scene_detect_min_frames == 0 {
            self.dlg.scene_detect_min_frames = 6;
        }
        self.dlg.scene_detect_cuts.clear();
        self.dlg.scene_detect_analyzed = false;
        self.dlg.scene_detect_job = None;
    }

    /// Diálogo **Detecção de Cenas e Cortes** (manual do VEGAS Pro 21, p. 300).
    fn scene_detect_window(&mut self, ctx: &egui::Context) {
        use neoveg_engine::engine::scene_detect::{detect_cuts_in_media, SceneDetectConfig};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::mpsc;

        // Consulta resultado de análise assíncrona em andamento.
        let mut finished_cuts: Option<Vec<Ticks>> = None;
        let mut cancel_job = false;
        if let Some(job) = &self.dlg.scene_detect_job {
            match job.rx.try_recv() {
                Ok(cuts) => {
                    finished_cuts = Some(cuts);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    finished_cuts = Some(Vec::new());
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                }
            }
        }
        if let Some(cuts) = finished_cuts {
            self.dlg.scene_detect_cuts = cuts;
            self.dlg.scene_detect_analyzed = true;
            self.dlg.scene_detect_job = None;
        }

        let Some(id) = self.dlg.scene_detect else {
            return;
        };
        let Some((ti, ei)) = self.project.find_event(id) else {
            self.dlg.scene_detect = None;
            return;
        };
        let ev = self.project.tracks[ti].events[ei].clone();
        let Some(media_item) = self.project.media(ev.media_id).cloned() else {
            self.dlg.scene_detect = None;
            return;
        };

        let mut open = true;
        let mut start_scan = false;
        let mut apply = false;
        let mut close = false;
        let running = self.dlg.scene_detect_job.is_some();

        crate::dialog_window::DialogWindow::new(format!("Detectar cenas e cortes — {}", ev.name))
            .id(egui::Id::new("scene_detect_win"))
            .open(&mut open)
            .resizable(false)
            .default_width(380.0)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.screen_rect().center())
            .show(ctx, |ui| {
                ui.label("Analisa diferenças visuais entre quadros consecutivos do evento selecionado para localizar mudanças de plano.");
                ui.separator();
                egui::Grid::new("scene_detect_grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Sensibilidade ao corte (%)");
                        ui.add_enabled(
                            !running,
                            egui::Slider::new(&mut self.dlg.scene_detect_threshold, 10.0..=90.0)
                                .fixed_decimals(0)
                                .suffix(" %"),
                        );
                        ui.end_row();
                        ui.label("Duração mínima da cena");
                        ui.add_enabled(
                            !running,
                            egui::DragValue::new(&mut self.dlg.scene_detect_min_frames)
                                .range(2..=120)
                                .suffix(" quadros"),
                        );
                        ui.end_row();
                        ui.label("Ação");
                        ui.vertical(|ui| {
                            ui.radio_value(
                                &mut self.dlg.scene_detect_markers,
                                false,
                                "Dividir evento nos cortes detectados",
                            );
                            ui.radio_value(
                                &mut self.dlg.scene_detect_markers,
                                true,
                                "Inserir marcadores de cena na timeline",
                            );
                        });
                        ui.end_row();
                    });
                ui.separator();
                if running {
                    ui.horizontal(|ui| {
                        crate::splash::spinner_inline(
                            ui,
                            ui.input(|i| i.time as f32),
                            16.0,
                            &self.theme,
                        );
                        ui.label("Analisando quadros do evento...");
                        if ui.button("Cancelar").clicked() {
                            cancel_job = true;
                        }
                    });
                } else {
                    if self.dlg.scene_detect_analyzed {
                        let msg = if self.dlg.scene_detect_cuts.is_empty() {
                            "Nenhum corte detectado com esse limiar.".to_string()
                        } else {
                            format!(
                                "{} corte(s) de cena detectado(s).",
                                self.dlg.scene_detect_cuts.len()
                            )
                        };
                        ui.label(egui::RichText::new(msg).strong());
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Analisar clipe").clicked() {
                            start_scan = true;
                        }
                        if ui
                            .add_enabled(
                                !self.dlg.scene_detect_cuts.is_empty(),
                                egui::Button::new("Aplicar na timeline"),
                            )
                            .clicked()
                        {
                            apply = true;
                        }
                        if ui.button("Fechar").clicked() {
                            close = true;
                        }
                    });
                }
            });

        if cancel_job {
            if let Some(job) = self.dlg.scene_detect_job.take() {
                job.cancel.store(true, Ordering::Relaxed);
            }
        }
        if start_scan {
            let cancel = std::sync::Arc::new(AtomicBool::new(false));
            let cancel_w = cancel.clone();
            let cache = self.media.clone();
            let fd = self
                .project
                .settings
                .frame_dur()
                .max(Ticks::from_millis_i64(15));
            let content_len = ev.rate.scale_ticks(ev.length).max(fd);
            let range = (ev.offset, ev.offset + content_len);
            let step = Ticks::from_raw(fd.0.saturating_mul(2)).max(Ticks::from_millis_i64(33));
            let cfg = SceneDetectConfig {
                threshold: (self.dlg.scene_detect_threshold / 100.0).clamp(0.1, 0.9),
                min_scene_ticks: Ticks::from_raw(
                    fd.0.saturating_mul(i64::from(self.dlg.scene_detect_min_frames.max(2))),
                ),
                sample_w: 64,
                sample_h: 36,
            };
            let (tx, rx) = mpsc::channel();
            self.dlg.scene_detect_analyzed = false;
            self.dlg.scene_detect_cuts.clear();
            if crate::util::spawn("scene-detect", move || {
                let cuts =
                    detect_cuts_in_media(&media_item, &cache, range, step, cfg, Some(&cancel_w));
                let _ = tx.send(cuts);
            })
            .is_ok()
            {
                self.dlg.scene_detect_job = Some(SceneDetectJob { cancel, rx });
            } else {
                self.set_status("Não foi possível iniciar o worker de detecção de cenas.");
            }
        }
        if apply {
            let cuts = self.dlg.scene_detect_cuts.clone();
            let as_markers = self.dlg.scene_detect_markers;
            self.session.apply_scene_cuts(id, &cuts, as_markers);
            self.sync_after_session();
            self.dlg.scene_detect = None;
        }
        if !open || close {
            if let Some(job) = self.dlg.scene_detect_job.take() {
                job.cancel.store(true, Ordering::Relaxed);
            }
            self.dlg.scene_detect = None;
        }
    }

    fn motion_track_window(&mut self, ctx: &egui::Context) {
        let Some(src_id) = self.dlg.motion_track_src else {
            return;
        };
        let Some((src_ti, src_ei)) = self.project.find_event(src_id) else {
            self.dlg.motion_track_src = None;
            return;
        };
        let src_name = self.project.tracks[src_ti].events[src_ei].name.clone();
        let video_events: Vec<(Id, String)> = self
            .project
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Video && !t.is_folder)
            .flat_map(|t| t.events.iter().map(|e| (e.id, e.name.clone())))
            .collect();
        let video_tracks: Vec<(Id, String)> = self
            .project
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Video && !t.is_folder)
            .map(|t| (t.id, t.name.clone()))
            .collect();

        if self.dlg.motion_track_target_event.is_none() {
            self.dlg.motion_track_target_event = Some(src_id);
        }
        if self.dlg.motion_track_target_track.is_none() {
            self.dlg.motion_track_target_track = video_tracks.first().map(|t| t.0);
        }
        if self.dlg.motion_track_step == 0 {
            self.dlg.motion_track_step = 1;
        }

        let mut open = true;
        let mut run_track = false;
        let mut close = false;
        crate::dialog_window::DialogWindow::new(format!("Rastreamento de Movimento — {src_name}"))
            .id(egui::Id::new("motion_track_win"))
            .open(&mut open)
            .resizable(false)
            .default_width(390.0)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.screen_rect().center())
            .show(ctx, |ui| {
                ui.label("Rastreia o deslocamento planar quadro a quadro no evento de vídeo selecionado e gera quadros-chave (Planar / Pan/Crop / Track Motion).");
                ui.separator();
                ui.label("Destino dos quadros-chave:");
                ui.radio_value(
                    &mut self.dlg.motion_track_target_is_track,
                    false,
                    "Pan/Crop de evento de vídeo",
                );
                if !self.dlg.motion_track_target_is_track {
                    let sel_ev_label = self
                        .dlg
                        .motion_track_target_event
                        .and_then(|id| video_events.iter().find(|e| e.0 == id))
                        .map(|e| e.1.clone())
                        .unwrap_or_else(|| src_name.clone());
                    egui::ComboBox::from_id_salt("mt_target_event")
                        .selected_text(sel_ev_label)
                        .show_ui(ui, |ui| {
                            for (eid, ename) in &video_events {
                                ui.selectable_value(
                                    &mut self.dlg.motion_track_target_event,
                                    Some(*eid),
                                    ename,
                                );
                            }
                        });
                }
                ui.radio_value(
                    &mut self.dlg.motion_track_target_is_track,
                    true,
                    "Movimento da trilha (Track Motion)",
                );
                if self.dlg.motion_track_target_is_track {
                    let sel_tr_label = self
                        .dlg
                        .motion_track_target_track
                        .and_then(|id| video_tracks.iter().find(|t| t.0 == id))
                        .map(|t| t.1.clone())
                        .unwrap_or_else(|| "Selecionar trilha".into());
                    egui::ComboBox::from_id_salt("mt_target_track")
                        .selected_text(sel_tr_label)
                        .show_ui(ui, |ui| {
                            for (tid, tname) in &video_tracks {
                                ui.selectable_value(
                                    &mut self.dlg.motion_track_target_track,
                                    Some(*tid),
                                    tname,
                                );
                            }
                        });
                }
                ui.add_space(4.0);
                ui.checkbox(
                    &mut self.dlg.motion_track_invert,
                    "Inverter movimento (estabilizar / travar ponto)",
                );
                ui.horizontal(|ui| {
                    ui.label("Amostragem (quadros)");
                    ui.add(
                        egui::DragValue::new(&mut self.dlg.motion_track_step)
                            .range(1..=30)
                            .suffix(" q"),
                    );
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Rastrear e aplicar keyframes").clicked() {
                        run_track = true;
                    }
                    if ui.button("Fechar").clicked() {
                        close = true;
                    }
                });
            });

        if run_track {
            let dst_ev = (!self.dlg.motion_track_target_is_track)
                .then_some(self.dlg.motion_track_target_event.unwrap_or(src_id));
            let dst_tr = self
                .dlg
                .motion_track_target_is_track
                .then_some(self.dlg.motion_track_target_track)
                .flatten();
            match self.session.track_event_motion(
                &self.media,
                src_id,
                dst_ev,
                dst_tr,
                self.dlg.motion_track_invert,
                self.dlg.motion_track_step,
            ) {
                Ok(_) => {
                    self.sync_after_session();
                    self.dlg.motion_track_src = None;
                }
                Err(e) => self.set_status(format!("Rastreamento de movimento: {e}")),
            }
        }
        if !open || close {
            self.dlg.motion_track_src = None;
        }
    }

    // -----------------------------------------------------------------

    fn project_props_window(&mut self, ctx: &egui::Context) {
        if !self.dlg.project_props {
            return;
        }
        let lang = self.lang();
        let mut open = true;
        let mut changed = false;
        let mut tab = self.dlg.project_props_tab.min(4);
        let mut time_format = self.opts.time_format;
        let bus_count = self.project.audio_buses.len();
        crate::dialog_window::DialogWindow::new(tr(lang, "Propriedades do projeto"))
            .id(egui::Id::new("project_props_win"))
            .open(&mut open)
            .resizable(true)
            .default_width(540.0)
            .min_width(480.0)
            .show(ctx, |ui| {
                // abas na ordem do VEGAS Pro: Vídeo, Áudio, Régua, Resumo, CD de áudio
                ui.horizontal(|ui| {
                    for (i, name) in ["Vídeo", "Áudio", "Régua", "Resumo", "CD de áudio"]
                        .into_iter()
                        .enumerate()
                    {
                        ui.selectable_value(&mut tab, i as u8, tr(lang, name));
                    }
                });
                ui.separator();
                let s = &mut self.project.settings;
                match tab {
                    0 => changed |= project_video_tab(ui, lang, s),
                    1 => {
                        egui::Grid::new("pp_audio").num_columns(2).show(ui, |ui| {
                            ui.label(tr(lang, "Taxa de amostragem (áudio)"));
                            egui::ComboBox::from_id_salt("sr")
                                .selected_text(format!("{} Hz", s.sample_rate))
                                .show_ui(ui, |ui| {
                                    for r in [44100, 48000, 88200, 96000] {
                                        changed |= ui
                                            .selectable_value(
                                                &mut s.sample_rate,
                                                r,
                                                format!("{r} Hz"),
                                            )
                                            .changed();
                                    }
                                });
                            ui.end_row();
                            ui.label(tr(lang, "Profundidade de bits"));
                            egui::ComboBox::from_id_salt("pp_bits")
                                .selected_text(format!("{}", s.audio_bit_depth))
                                .show_ui(ui, |ui| {
                                    for b in [16u16, 24, 32] {
                                        changed |= ui
                                            .selectable_value(
                                                &mut s.audio_bit_depth,
                                                b,
                                                format!("{b}"),
                                            )
                                            .changed();
                                    }
                                });
                            ui.end_row();
                            ui.label(tr(lang, "Modo do Master (áudio)"));
                            egui::ComboBox::from_id_salt("master_layout")
                                .selected_text(s.channel_layout.name_in(lang))
                                .show_ui(ui, |ui| {
                                    for l in crate::model::ChannelLayout::ALL {
                                        changed |= ui
                                            .selectable_value(
                                                &mut s.channel_layout,
                                                l,
                                                l.name_in(lang),
                                            )
                                            .changed();
                                    }
                                });
                            ui.end_row();
                            ui.label(tr(lang, "Qualidade da reamostragem e do esticamento"));
                            egui::ComboBox::from_id_salt("pp_resample")
                                .selected_text(s.resample_quality.name_in(lang))
                                .show_ui(ui, |ui| {
                                    for q in RenderQuality::ALL {
                                        changed |= ui
                                            .selectable_value(
                                                &mut s.resample_quality,
                                                q,
                                                q.name_in(lang),
                                            )
                                            .changed();
                                    }
                                });
                            ui.end_row();
                            ui.label(tr(lang, "Barramentos de áudio"));
                            ui.label(format!("{bus_count}"));
                            ui.end_row();
                        });
                    }
                    2 => {
                        egui::Grid::new("pp_ruler").num_columns(2).show(ui, |ui| {
                            ui.label(tr(lang, "Formato de tempo da régua"));
                            egui::ComboBox::from_id_salt("pp_time_fmt")
                                .selected_text(time_format.name_in(lang))
                                .show_ui(ui, |ui| {
                                    for f in TimeFormat::ALL {
                                        ui.selectable_value(&mut time_format, f, f.name_in(lang));
                                    }
                                });
                            ui.end_row();
                            ui.label(tr(lang, "Tempo musical (BPM)"));
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut s.bpm)
                                        .range(20.0..=300.0)
                                        .speed(0.5)
                                        .suffix(" BPM"),
                                )
                                .changed();
                            ui.end_row();
                            ui.label(tr(lang, "Batidas por compasso"));
                            changed |= ui
                                .add(egui::DragValue::new(&mut s.beats_per_measure).range(1..=32))
                                .changed();
                            ui.end_row();
                            ui.label(tr(lang, "Figura da batida"));
                            egui::ComboBox::from_id_salt("pp_beat_note")
                                .selected_text(format!("1/{}", s.beat_note))
                                .show_ui(ui, |ui| {
                                    for n in [2u32, 4, 8, 16] {
                                        changed |= ui
                                            .selectable_value(&mut s.beat_note, n, format!("1/{n}"))
                                            .changed();
                                    }
                                });
                            ui.end_row();
                        });
                    }
                    3 => {
                        let sm = &mut s.summary;
                        egui::Grid::new("pp_summary").num_columns(2).show(ui, |ui| {
                            for (label, field) in [
                                ("Título", &mut sm.title),
                                ("Artista", &mut sm.artist),
                                ("Engenheiro", &mut sm.engineer),
                                ("Direitos autorais", &mut sm.copyright),
                            ] {
                                ui.label(tr(lang, label));
                                changed |= ui
                                    .add(
                                        egui::TextEdit::singleline(field)
                                            .char_limit(SUMMARY_FIELD_MAX)
                                            .desired_width(280.0),
                                    )
                                    .changed();
                                ui.end_row();
                            }
                            ui.label(tr(lang, "Comentários"));
                            changed |= ui
                                .add(
                                    egui::TextEdit::multiline(&mut sm.comments)
                                        .char_limit(SUMMARY_FIELD_MAX)
                                        .desired_rows(4)
                                        .desired_width(280.0),
                                )
                                .changed();
                            ui.end_row();
                        });
                    }
                    _ => {
                        let cd = &mut s.audio_cd;
                        egui::Grid::new("pp_cd").num_columns(2).show(ui, |ui| {
                            ui.label(tr(lang, "Código UPC/EAN do disco"));
                            let r = ui.add(
                                egui::TextEdit::singleline(&mut cd.upc)
                                    .char_limit(13)
                                    .desired_width(160.0),
                            );
                            if r.changed() {
                                cd.upc.retain(|c| c.is_ascii_digit());
                                changed = true;
                            }
                            ui.end_row();
                            ui.label(tr(lang, "Primeira faixa"));
                            changed |= ui
                                .add(egui::DragValue::new(&mut cd.first_track).range(1..=99))
                                .changed();
                            ui.end_row();
                        });
                        ui.label(
                            egui::RichText::new(tr(
                                lang,
                                "O código e a numeração vão para a folha .cue exportada.",
                            ))
                            .small()
                            .weak(),
                        );
                    }
                }
            });
        self.dlg.project_props_tab = tab;
        if time_format != self.opts.time_format {
            self.opts.time_format = time_format;
        }
        if changed {
            self.session.project.settings.manually_configured = true;
            self.dialog_change();
        }
        self.dlg.project_props = open;
    }

    fn event_props_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.dlg.event_props else {
            return;
        };
        let Some((ti, _)) = self.project.find_event(id) else {
            self.dlg.event_props = None;
            return;
        };
        let is_video = self.project.tracks[ti].kind == TrackKind::Video;
        let fps = self.project.settings.fps;
        let sr = self.project.settings.sample_rate;
        let fd = self.project.settings.frame_dur();
        let tf = self
            .project
            .settings
            .effective_time_format(self.opts.time_format);
        let lang = self.lang();
        let mut open = true;
        let mut changed = false;
        let (media_info, sync_master_name, has_sync_followers) = {
            let Some(e) = self.project.event(id) else {
                return;
            };
            let info = self
                .project
                .media(e.media_id)
                .map(|m| format!("{}  —  {}", m.name, m.info))
                .unwrap_or_default();
            let master_name = e.sync_link_master.and_then(|mid| {
                self.project
                    .event(mid)
                    .map(|me| format!("{} ({})", me.name, self.fmt_time(me.start)))
            });
            let followers = self.session.has_sync_followers(id);
            (info, master_name, followers)
        };
        let mut unlink_sync = false;
        let mut select_sync = false;
        let mut open_tc = false;
        let mut open_pc = false;
        let mut open_fx = false;
        crate::dialog_window::DialogWindow::new(tr(lang, "Propriedades do evento"))
            .id(egui::Id::new("event_props_win"))
            .open(&mut open)
            .default_width(460.0)
            .min_width(400.0)
            .show(ctx, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(media_info).small()).wrap());
                ui.separator();
                let Some(e) = self.project.event_mut(id) else {
                    return;
                };
                egui::Grid::new(("ep", id)).num_columns(2).show(ui, |ui| {
                    ui.label(tr(lang, "Nome"));
                    changed |= ui.text_edit_singleline(&mut e.name).changed();
                    ui.end_row();
                    ui.label(tr(lang, "Início"));
                    changed |= time_field(ui, &mut e.start, fps, sr, tf, ("ep_start", id));
                    ui.end_row();
                    ui.label(tr(lang, "Duração"));
                    let mut len = e.length;
                    if time_field(ui, &mut len, fps, sr, tf, ("ep_len", id)) {
                        e.length = len.max(fd);
                        e.fade_in.len = e.fade_in.len.min(e.length);
                        e.fade_out.len = e.fade_out.len.min(e.length - e.fade_in.len);
                        changed = true;
                    }
                    ui.end_row();
                    ui.label(tr(lang, "Deslocamento na mídia"));
                    changed |= time_field(ui, &mut e.offset, fps, sr, tf, ("ep_off", id));
                    ui.end_row();
                    ui.label(tr(lang, "Velocidade"));
                    let mut pct = e.rate.percent_f64();
                    if ui
                        .add(
                            egui::DragValue::new(&mut pct)
                                .speed(1.0)
                                .range(0.0..=1000.0)
                                .suffix(" %"),
                        )
                        .changed()
                    {
                        if let Some(r) = Rate::from_f64(pct / 100.0) {
                            e.rate = r;
                            changed = true;
                        }
                    }
                    ui.end_row();
                    if is_video {
                        ui.label(tr(lang, "Opacidade"));
                        let mut pct = (e.opacity * 100.0).clamp(0.0, 100.0);
                        if ui
                            .add(
                                egui::Slider::new(&mut pct, 0.0..=100.0)
                                    .suffix(" %")
                                    .fixed_decimals(0),
                            )
                            .changed()
                        {
                            e.opacity = (pct / 100.0).clamp(0.0, 1.0);
                            changed = true;
                        }
                        ui.end_row();
                    }
                    if !is_video {
                        ui.label(tr(lang, "Ganho"));
                        changed |= ui
                            .add(egui::Slider::new(&mut e.gain_db, -40.0..=20.0).suffix(" dB"))
                            .changed();
                        ui.end_row();
                        ui.label("Tom (Pitch Shift)");
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                changed |= ui
                                    .add(
                                        egui::Slider::new(&mut e.pitch_semitones, -24.0..=24.0)
                                            .step_by(0.1)
                                            .suffix(" st"),
                                    )
                                    .changed();
                                if e.pitch_semitones.abs() > 1e-4
                                    && ui.small_button("0 st").clicked()
                                {
                                    e.pitch_semitones = 0.0;
                                    changed = true;
                                }
                            });
                            changed |= ui
                                .checkbox(&mut e.lock_pitch_to_stretch, "Preservar tom ao esticar")
                                .changed();
                        });
                        ui.end_row();
                    }
                    ui.label(tr(lang, "Fade de entrada"));
                    ui.horizontal(|ui| {
                        let mut fi = e.fade_in.len;
                        if time_field(ui, &mut fi, fps, sr, tf, ("ep_fi", id)) {
                            e.fade_in.len = fi.clamp_safe(Ticks::ZERO, e.length - e.fade_out.len);
                            changed = true;
                        }
                        changed |= curve_combo(ui, &mut e.fade_in.curve, ("fic", id), lang);
                    });
                    ui.end_row();
                    ui.label(tr(lang, "Fade de saída"));
                    ui.horizontal(|ui| {
                        let mut fo = e.fade_out.len;
                        if time_field(ui, &mut fo, fps, sr, tf, ("ep_fo", id)) {
                            e.fade_out.len = fo.clamp_safe(Ticks::ZERO, e.length - e.fade_in.len);
                            changed = true;
                        }
                        changed |= curve_combo(ui, &mut e.fade_out.curve, ("foc", id), lang);
                    });
                    ui.end_row();
                    ui.label(tr(lang, "Opções"));
                    // linhas fixas: `horizontal_wrapped` dentro da Grid quebra
                    // depois de a altura da linha ser medida e invade a linha
                    // de baixo (Transição de entrada)
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            changed |= ui.checkbox(&mut e.mute, tr(lang, "Mudo")).changed();
                            changed |= ui.checkbox(&mut e.locked, tr(lang, "Travado")).changed();
                            changed |= ui.checkbox(&mut e.looped, tr(lang, "Loop")).changed();
                            changed |= ui.checkbox(&mut e.reverse, tr(lang, "Reverso")).changed();
                        });
                        if is_video {
                            changed |= ui
                                .checkbox(&mut e.maintain_aspect, tr(lang, "Manter proporção"))
                                .changed();
                        }
                    });
                    ui.end_row();
                    if is_video {
                        ui.label(tr(lang, "Transição de entrada"));
                        ui.vertical(|ui| {
                            let mut kind = e.transition.as_ref().map(|t| t.kind);
                            let before = kind;
                            egui::ComboBox::from_id_salt(("trk", id))
                                .selected_text(
                                    kind.map(|k| k.name_in(lang))
                                        .unwrap_or_else(|| tr(lang, "Nenhuma (crossfade)")),
                                )
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut kind,
                                        None,
                                        tr(lang, "Nenhuma (crossfade)"),
                                    );
                                    for k in TransitionKind::ALL {
                                        ui.selectable_value(&mut kind, Some(k), k.name_in(lang));
                                    }
                                });
                            if kind != before {
                                e.transition = kind.map(|k| TransitionSpec {
                                    kind: k,
                                    softness: 0.05,
                                });
                                changed = true;
                            }
                            if let Some(tr_spec) = &mut e.transition {
                                changed |= ui
                                    .add(
                                        egui::Slider::new(&mut tr_spec.softness, 0.0..=0.5)
                                            .text(tr(lang, "Suavidade da borda")),
                                    )
                                    .changed();
                            }
                        });
                        ui.end_row();
                        ui.label("3D Estereoscópico");
                        ui.vertical(|ui| {
                            egui::ComboBox::from_id_salt(("ep_s3d", id))
                                .selected_text(e.stereo_3d.layout.label())
                                .show_ui(ui, |ui| {
                                    for l in Stereo3dSourceLayout::ALL {
                                        changed |= ui
                                            .selectable_value(&mut e.stereo_3d.layout, l, l.label())
                                            .changed();
                                    }
                                });
                            changed |= ui
                                .checkbox(
                                    &mut e.stereo_3d.swap_lr,
                                    "Inverter olho esquerdo/direito",
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::Slider::new(
                                        &mut e.stereo_3d.horizontal_offset,
                                        -0.25..=0.25,
                                    )
                                    .step_by(0.002)
                                    .text("Convergência"),
                                )
                                .changed();
                        });
                        ui.end_row();
                    }
                    ui.label("Sync Link");
                    ui.horizontal(|ui| {
                        if let Some(mname) = &sync_master_name {
                            ui.label(format!("Segue: {mname}"));
                            if ui.small_button("Desvincular").clicked() {
                                unlink_sync = true;
                            }
                            if ui.small_button("Selecionar rede").clicked() {
                                select_sync = true;
                            }
                        } else if has_sync_followers {
                            ui.label("Evento mestre com seguidores");
                            if ui.small_button("Selecionar rede").clicked() {
                                select_sync = true;
                            }
                            if ui.small_button("Liberar seguidores").clicked() {
                                unlink_sync = true;
                            }
                        } else {
                            ui.label(egui::RichText::new("Sem vínculo de sincronização").weak());
                        }
                    });
                    ui.end_row();
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if is_video && ui.button(tr(lang, "Pan/Crop...")).clicked() {
                        open_pc = true;
                    }
                    if ui.button(tr(lang, "Efeitos do evento...")).clicked() {
                        open_fx = true;
                    }
                    if ui.button("Escolher take...").clicked() {
                        open_tc = true;
                    }
                });
            });
        if unlink_sync {
            if !self.sel_events.contains(&id) {
                self.sel_events.clear();
                self.sel_events.insert(id);
            }
            self.session.unlink_sync_links();
            self.sync_after_session();
        }
        if select_sync {
            if !self.sel_events.contains(&id) {
                self.sel_events.clear();
                self.sel_events.insert(id);
            }
            self.session.select_sync_linked();
            self.sync_after_session();
        }
        if open_tc {
            self.dlg.take_chooser = Some(id);
        }
        if open_pc {
            self.dlg.pan_crop = Some(id);
        }
        if open_fx {
            self.open_fx_target(FxTarget::Event(id));
        }
        if changed {
            self.dialog_change();
        }
        if !open {
            self.dlg.event_props = None;
        }
    }

    // -----------------------------------------------------------------
    // Cadeia de efeitos
    // -----------------------------------------------------------------

    pub(crate) fn fx_chain_mut(&mut self, target: FxTarget) -> Option<&mut Vec<FxInstance>> {
        match target {
            FxTarget::Event(id) => self.project.event_mut(id).map(|e| &mut e.fx),
            FxTarget::Track(id) => self
                .project
                .tracks
                .iter_mut()
                .find(|t| t.id == id)
                .map(|t| &mut t.fx),
            FxTarget::AudioBus(id) => self.project.audio_bus_mut(id).map(|b| &mut b.fx),
            FxTarget::MasterAudio => Some(&mut self.project.master.fx),
            FxTarget::MasterVideo => Some(&mut self.project.master.video_fx),
        }
    }

    /// Abre a janela de efeitos para `target` seguindo o fluxo do VEGAS Pro
    /// (manual do VEGAS Pro 21, pp. 488–489): quando ainda não há nenhum plug-in
    /// na cadeia, abre automaticamente o *Plug-In Chooser* (Escolha de Plug-in);
    /// quando já há efeitos, abre direto na cadeia de parâmetros com acesso ao seletor.
    pub(crate) fn open_fx_target(&mut self, target: FxTarget) {
        let is_empty = self
            .fx_chain_mut(target)
            .map(|c| c.is_empty())
            .unwrap_or(true);
        self.dlg.fx_target = Some(target);
        if is_empty {
            self.dlg.fx_chooser_open = true;
            self.dlg.fx_chooser_snapshot = Some(Vec::new());
            self.dlg.fx_chooser_selected = None;
            self.dlg.fx_selected_slot = None;
        } else {
            self.dlg.fx_chooser_open = false;
            self.dlg.fx_chooser_snapshot = None;
            self.dlg.fx_selected_slot = Some(0);
        }
    }

    fn fx_window(&mut self, ctx: &egui::Context) {
        let Some(target) = self.dlg.fx_target else {
            return;
        };
        let lang = self.lang();
        let (title, audio, local_t) = match target {
            FxTarget::Event(id) => {
                let Some((ti, _)) = self.project.find_event(id) else {
                    self.dlg.fx_target = None;
                    return;
                };
                let Some(e) = self.project.event(id) else {
                    return;
                };
                (
                    tr_args(lang, "Efeitos do evento — {name}", &[("name", &e.name)]),
                    self.project.tracks[ti].kind == TrackKind::Audio,
                    (self.cursor - e.start).clamp_safe(Ticks::ZERO, e.length),
                )
            }
            FxTarget::Track(id) => {
                let Some(t) = self.project.tracks.iter().find(|t| t.id == id) else {
                    self.dlg.fx_target = None;
                    return;
                };
                (
                    tr_args(lang, "Efeitos da trilha — {name}", &[("name", &t.name)]),
                    t.kind == TrackKind::Audio,
                    self.cursor,
                )
            }
            FxTarget::AudioBus(id) => {
                let Some(b) = self.project.audio_bus(id) else {
                    self.dlg.fx_target = None;
                    return;
                };
                (
                    tr_args(lang, "Efeitos do barramento — {name}", &[("name", &b.name)]),
                    true,
                    self.cursor,
                )
            }
            FxTarget::MasterAudio => (
                tr(lang, "Efeitos do master bus (áudio)").into(),
                true,
                self.cursor,
            ),
            FxTarget::MasterVideo => (
                tr(lang, "Efeitos de saída de vídeo").into(),
                false,
                self.cursor,
            ),
        };
        let tol = self.project.settings.frame_dur().div_i64(2);
        let fmt_local = self.fmt_time(local_t);
        let mut open = true;
        let mut changed = false;
        let mut add_item: Option<FxChooserItem> = None;
        let mut replace_item: Option<(usize, FxChooserItem)> = None;
        let mut op: Option<(usize, &str)> = None;
        let mut apply_saved_chain: Option<usize> = None;
        let mut delete_saved_chain: Option<usize> = None;
        let mut export_chain_file = false;
        let mut import_chain_file = false;
        let mut confirm_save_chain: Option<String> = None;
        let mut confirm_save_preset: Option<(FxInstance, String)> = None;
        let mut open_color_grading = false;
        let mut chooser_ok = false;
        let mut chooser_cancel = false;
        let saved_chains_snapshot = self.prefs.fx_chains.clone();
        let probed_ofx = if !audio {
            crate::plugin_manager::probed_ofx_plugins(ctx)
        } else {
            Vec::new()
        };
        let probed_vst3 = if audio {
            crate::plugin_manager::probed_vst3_plugins(ctx)
        } else {
            Vec::new()
        };
        let Some(chain) = self.fx_chain_mut(target) else {
            return;
        };
        let mut chain_copy = chain.clone();
        let mut chooser_open = self.dlg.fx_chooser_open;
        let mut chooser_cat = self.dlg.fx_chooser_cat;
        let mut chooser_filter = self.dlg.fx_chooser_filter.clone();
        let mut chooser_selected = self.dlg.fx_chooser_selected.clone();
        let mut sel_slot = self.dlg.fx_selected_slot;
        let win_title = if chooser_open {
            tr_args(lang, "Escolha de Plug-in — {title}", &[("title", &title)])
        } else {
            title
        };
        crate::dialog_window::DialogWindow::new(win_title)
            .id(egui::Id::new("fx_chain_win"))
            .open(&mut open)
            .default_size([740.0, 560.0])
            .min_size([560.0, 400.0])
            .resizable(true)
            .show(ctx, |ui| {
                // Faixa superior estilo VEGAS Pro: pílulas da cadeia + botão Cadeia de Plug-ins
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("●").weak());
                    if chain_copy.is_empty() {
                        ui.label(
                            egui::RichText::new(tr(lang, "Nenhum plug-in na cadeia"))
                                .small()
                                .weak(),
                        );
                    } else {
                        for (i, fx) in chain_copy.iter_mut().enumerate() {
                            ui.label(egui::RichText::new("—").weak());
                            let is_sel = sel_slot == Some(i);
                            if ui.checkbox(&mut fx.enabled, "").changed() {
                                changed = true;
                            }
                            let pill_name = if matches!(fx.kind, FxKind::Ofx | FxKind::Vst3) {
                                fx.display_name()
                            } else {
                                fx.kind.name_in(lang).to_string()
                            };
                            if ui.selectable_label(is_sel, pill_name).clicked() {
                                sel_slot = Some(i);
                            }
                        }
                    }
                    ui.label(egui::RichText::new("—●").weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::icon_button(
                            ui,
                            Icon::PluginPlug,
                            Some(tr(lang, "Cadeia de Plug-ins...")),
                            chooser_open,
                            true,
                            &self.theme,
                        )
                        .on_hover_text(tr(
                            lang,
                            "Abrir seletor de plug-ins (Escolha de Plug-in) para adicionar, substituir ou remover efeitos",
                        ))
                        .clicked()
                        {
                            if !chooser_open {
                                self.dlg.fx_chooser_snapshot = Some(chain_copy.clone());
                            }
                            chooser_open = !chooser_open;
                        }
                    });
                });
                ui.separator();

                if chooser_open {
                    // Seletor estilo VEGAS Pro Plug-In Chooser (manual pp. 488-489):
                    // pastas à esquerda, lista + prévia no centro, botões OK/Cancelar/Adicionar/Substituir/Remover à direita.
                    ui.horizontal(|ui| {
                        ui.strong(tr(lang, "Escolha de Plug-in"));
                        ui.add(
                            egui::TextEdit::singleline(&mut chooser_filter)
                                .desired_width((ui.available_width() - 120.0).clamp(160.0, 320.0))
                                .hint_text(tr(lang, "Filtrar plug-ins...")),
                        );
                        if !chooser_filter.is_empty() && ui.small_button("×").clicked() {
                            chooser_filter.clear();
                        }
                    });
                    ui.add_space(4.0);
                    let q = chooser_filter.trim().to_lowercase();
                    let native_list: &[FxKind] = if audio {
                        &FxKind::AUDIO
                    } else {
                        &FxKind::VIDEO
                    };
                    let mut folder_names: Vec<String> = vec![
                        tr(lang, "Todos").to_string(),
                        tr(lang, "Favoritos").to_string(),
                        tr(lang, "Cadeias salvas").to_string(),
                        if audio {
                            tr(lang, "Terceiros (VST3)").to_string()
                        } else {
                            tr(lang, "Terceiros (OFX)").to_string()
                        },
                    ];
                    if !audio {
                        for cat in FxCategory::ALL {
                            folder_names.push(cat.name_in(lang).to_string());
                        }
                    }
                    if chooser_cat >= folder_names.len() {
                        chooser_cat = 0;
                    }
                    let favs = self.prefs.fx_favorites.clone();

                    // Constrói os itens visíveis na pasta selecionada
                    let mut visible_items: Vec<(String, FxChooserItem)> = Vec::new();
                    match chooser_cat {
                        0 => {
                            // Todos: nativos + cadeias salvas + terceiros consultados
                            for &k in native_list {
                                let nm = k.name_in(lang).to_string();
                                if q.is_empty()
                                    || nm.to_lowercase().contains(&q)
                                    || k.name().to_lowercase().contains(&q)
                                {
                                    visible_items.push((nm, FxChooserItem::Native(k)));
                                }
                            }
                            if !audio {
                                for (bin, info) in &probed_ofx {
                                    if q.is_empty() || info.identifier.to_lowercase().contains(&q) {
                                        let label = info
                                            .identifier
                                            .rsplit('.')
                                            .next()
                                            .filter(|s| !s.is_empty())
                                            .unwrap_or(&info.identifier)
                                            .to_string();
                                        visible_items.push((
                                            format!("{label} [OFX]"),
                                            FxChooserItem::Ofx {
                                                binary: bin.to_string_lossy().to_string(),
                                                identifier: info.identifier.clone(),
                                                label,
                                            },
                                        ));
                                    }
                                }
                            } else {
                                for (bin, info) in &probed_vst3 {
                                    if q.is_empty() || info.name.to_lowercase().contains(&q) {
                                        visible_items.push((
                                            format!("{} [VST3]", info.name),
                                            FxChooserItem::Vst3 {
                                                binary: bin.to_string_lossy().to_string(),
                                                class_id: info.class_id.clone(),
                                                name: info.name.clone(),
                                                is_instrument: info.is_instrument(),
                                            },
                                        ));
                                    }
                                }
                            }
                        }
                        1 => {
                            // Favoritos
                            for &k in native_list {
                                if !favs.iter().any(|f| f == &format!("{k:?}")) {
                                    continue;
                                }
                                let nm = k.name_in(lang).to_string();
                                if q.is_empty()
                                    || nm.to_lowercase().contains(&q)
                                    || k.name().to_lowercase().contains(&q)
                                {
                                    visible_items.push((nm, FxChooserItem::Native(k)));
                                }
                            }
                        }
                        2 => {
                            // Cadeias salvas (Filter Packages)
                            for (ci, c) in saved_chains_snapshot.iter().enumerate() {
                                if c.audio != audio {
                                    continue;
                                }
                                if q.is_empty() || c.name.to_lowercase().contains(&q) {
                                    visible_items.push((
                                        format!("{} ({})", c.name, c.items.len()),
                                        FxChooserItem::SavedChain(ci),
                                    ));
                                }
                            }
                        }
                        3 => {
                            // Terceiros (OFX / VST3)
                            if !audio {
                                for (bin, info) in &probed_ofx {
                                    if q.is_empty() || info.identifier.to_lowercase().contains(&q) {
                                        let label = info
                                            .identifier
                                            .rsplit('.')
                                            .next()
                                            .filter(|s| !s.is_empty())
                                            .unwrap_or(&info.identifier)
                                            .to_string();
                                        visible_items.push((
                                            info.identifier.clone(),
                                            FxChooserItem::Ofx {
                                                binary: bin.to_string_lossy().to_string(),
                                                identifier: info.identifier.clone(),
                                                label,
                                            },
                                        ));
                                    }
                                }
                            } else {
                                for (bin, info) in &probed_vst3 {
                                    if q.is_empty() || info.name.to_lowercase().contains(&q) {
                                        visible_items.push((
                                            info.name.clone(),
                                            FxChooserItem::Vst3 {
                                                binary: bin.to_string_lossy().to_string(),
                                                class_id: info.class_id.clone(),
                                                name: info.name.clone(),
                                                is_instrument: info.is_instrument(),
                                            },
                                        ));
                                    }
                                }
                            }
                        }
                        cat_idx => {
                            let video_cat = FxCategory::ALL.get(cat_idx.saturating_sub(4)).copied();
                            for &k in native_list {
                                if k.category() != video_cat {
                                    continue;
                                }
                                let nm = k.name_in(lang).to_string();
                                if q.is_empty()
                                    || nm.to_lowercase().contains(&q)
                                    || k.name().to_lowercase().contains(&q)
                                {
                                    visible_items.push((nm, FxChooserItem::Native(k)));
                                }
                            }
                        }
                    }

                    let avail_w = ui.available_width().max(480.0);
                    let left_w = 155.0_f32;
                    let right_w = 104.0_f32;
                    let mid_w = (avail_w - left_w - right_w - 28.0).max(200.0);
                    let chooser_h = (ui.available_height() - 16.0).clamp(240.0, 420.0);

                    ui.horizontal_top(|ui| {
                        // Coluna 1: Pastas de plug-ins
                        ui.vertical(|ui| {
                            ui.set_width(left_w);
                            ui.strong(tr(lang, "Pastas"));
                            egui::Frame::NONE
                                .fill(self.theme.extreme_bg)
                                .stroke(Stroke::new(1.0_f32, self.theme.border))
                                .inner_margin(4.0)
                                .show(ui, |ui| {
                                    ui.set_width(left_w - 8.0);
                                    egui::ScrollArea::vertical()
                                        .id_salt("fx_chooser_folders")
                                        .max_height(chooser_h - 24.0)
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            for (ci, cname) in folder_names.iter().enumerate() {
                                                ui.horizontal(|ui| {
                                                    icons::paint_inline(
                                                        ui,
                                                        Icon::FolderItem,
                                                        13.0,
                                                        &self.theme,
                                                    );
                                                    if ui
                                                        .selectable_label(chooser_cat == ci, cname)
                                                        .clicked()
                                                    {
                                                        chooser_cat = ci;
                                                    }
                                                });
                                            }
                                        });
                                });
                        });

                        // Coluna 2: Lista de plug-ins + prévia do efeito selecionado
                        ui.vertical(|ui| {
                            ui.set_width(mid_w);
                            ui.strong(tr(lang, "Plug-ins disponíveis"));
                            let preview_h = if !audio { 104.0 } else { 0.0 };
                            let list_h = (chooser_h - preview_h - 28.0).max(120.0);
                            egui::Frame::NONE
                                .fill(self.theme.extreme_bg)
                                .stroke(Stroke::new(1.0_f32, self.theme.border))
                                .inner_margin(4.0)
                                .show(ui, |ui| {
                                    ui.set_width(mid_w - 8.0);
                                    egui::ScrollArea::vertical()
                                        .id_salt("fx_chooser_list")
                                        .max_height(list_h)
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            if visible_items.is_empty() {
                                                ui.label(
                                                    egui::RichText::new(tr(
                                                        lang,
                                                        "Nenhum plug-in disponível nesta pasta.",
                                                    ))
                                                    .small()
                                                    .weak(),
                                                );
                                            }
                                            for (label, item) in &visible_items {
                                                let is_sel =
                                                    chooser_selected.as_ref() == Some(item);
                                                let resp = ui.selectable_label(is_sel, label);
                                                if resp.clicked() {
                                                    chooser_selected = Some(item.clone());
                                                }
                                                if resp.double_clicked() {
                                                    chooser_selected = Some(item.clone());
                                                    add_item = Some(item.clone());
                                                }
                                            }
                                        });
                                });
                            if !audio {
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    let thumb_sz = vec2(148.0, 84.0);
                                    let (rect, _) =
                                        ui.allocate_exact_size(thumb_sz, Sense::hover());
                                    let hovered = ui.rect_contains_pointer(rect);
                                    let painted = match chooser_selected.as_ref() {
                                        Some(FxChooserItem::Native(k)) => {
                                            let ctx2 = ui.ctx().clone();
                                            self.fx_browser
                                                .preview_texture(&ctx2, *k, hovered)
                                                .map(|tex| {
                                                    egui::Image::new((tex.id(), thumb_sz))
                                                        .paint_at(ui, rect);
                                                })
                                                .is_some()
                                        }
                                        Some(FxChooserItem::Ofx {
                                            binary, identifier, ..
                                        }) => {
                                            let ctx2 = ui.ctx().clone();
                                            match self.fx_browser.ofx.get(&ctx2, binary, identifier)
                                            {
                                                crate::fx_browser::OfxThumb::Ready(tex) => {
                                                    egui::Image::new((tex.id(), thumb_sz))
                                                        .paint_at(ui, rect);
                                                    true
                                                }
                                                _ => false,
                                            }
                                        }
                                        _ => false,
                                    };
                                    if !painted {
                                        ui.painter().rect_filled(rect, 2.0, self.theme.widget_bg);
                                    }
                                    ui.painter().rect_stroke(
                                        rect,
                                        2.0,
                                        Stroke::new(1.0_f32, self.theme.border),
                                        StrokeKind::Inside,
                                    );
                                    ui.vertical(|ui| {
                                        let hint = match chooser_selected.as_ref() {
                                            Some(FxChooserItem::Native(k)) => {
                                                k.name_in(lang).to_string()
                                            }
                                            Some(FxChooserItem::Ofx { label, .. }) => label.clone(),
                                            Some(FxChooserItem::SavedChain(ci)) => saved_chains_snapshot
                                                .get(*ci)
                                                .map(|c| c.name.clone())
                                                .unwrap_or_default(),
                                            _ => tr(
                                                lang,
                                                "Clique em um efeito para ver a prévia; duplo clique adiciona à cadeia.",
                                            )
                                            .to_string(),
                                        };
                                        ui.add(egui::Label::new(egui::RichText::new(hint).small()).wrap());
                                    });
                                });
                            }
                        });

                        // Coluna 3: Botões de ação verticais no estilo do VEGAS Pro
                        ui.vertical(|ui| {
                            ui.set_width(right_w);
                            ui.add_space(18.0);
                            let bw = right_w - 4.0;
                            if ui
                                .add_sized([bw, 24.0], egui::Button::new(tr(lang, "OK")))
                                .clicked()
                            {
                                chooser_ok = true;
                            }
                            if ui
                                .add_sized([bw, 24.0], egui::Button::new(tr(lang, "Cancelar")))
                                .clicked()
                            {
                                chooser_cancel = true;
                            }
                            ui.add_space(10.0);
                            let has_pick = chooser_selected.is_some();
                            if ui
                                .add_enabled_ui(has_pick, |ui| {
                                    ui.add_sized(
                                        [bw, 24.0],
                                        egui::Button::new(tr(lang, "Adicionar")),
                                    )
                                })
                                .inner
                                .clicked()
                            {
                                add_item = chooser_selected.clone();
                            }
                            let valid_slot = sel_slot.filter(|&s| s < chain_copy.len());
                            if ui
                                .add_enabled_ui(has_pick && valid_slot.is_some(), |ui| {
                                    ui.add_sized(
                                        [bw, 24.0],
                                        egui::Button::new(tr(lang, "Substituir")),
                                    )
                                })
                                .inner
                                .clicked()
                            {
                                if let (Some(slot), Some(item)) =
                                    (valid_slot, chooser_selected.clone())
                                {
                                    replace_item = Some((slot, item));
                                }
                            }
                            if ui
                                .add_enabled_ui(valid_slot.is_some(), |ui| {
                                    ui.add_sized(
                                        [bw, 24.0],
                                        egui::Button::new(tr(lang, "Remover")),
                                    )
                                })
                                .inner
                                .clicked()
                            {
                                if let Some(slot) = valid_slot {
                                    op = Some((slot, "del"));
                                }
                            }
                            ui.add_space(10.0);
                            let has_saveable = chain_copy
                                .iter()
                                .any(|fx| !matches!(fx.kind, FxKind::Ofx | FxKind::Vst3));
                            if ui
                                .add_enabled_ui(has_saveable, |ui| {
                                    ui.add_sized(
                                        [bw, 24.0],
                                        egui::Button::new(tr(lang, "Salvar cadeia...")),
                                    )
                                })
                                .inner
                                .on_hover_text(tr(
                                    lang,
                                    "Salvar esta sequência de efeitos como pacote reutilizável (Filter Package)",
                                ))
                                .clicked()
                            {
                                self.dlg.save_chain_target = Some(target);
                                if self.dlg.save_chain_name.trim().is_empty() {
                                    self.dlg.save_chain_name = if audio {
                                        tr(lang, "Cadeia de áudio").into()
                                    } else {
                                        tr(lang, "Cadeia de vídeo").into()
                                    };
                                }
                            }
                        });
                    });
                    if self.dlg.save_chain_target == Some(target) {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(tr(lang, "Nome da cadeia:"));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.dlg.save_chain_name)
                                    .desired_width(180.0),
                            );
                            let valid = !self.dlg.save_chain_name.trim().is_empty();
                            if ui
                                .add_enabled(valid, egui::Button::new(tr(lang, "Salvar")))
                                .clicked()
                            {
                                confirm_save_chain =
                                    Some(self.dlg.save_chain_name.trim().to_string());
                            }
                            if ui.button(tr(lang, "Fechar")).clicked() {
                                self.dlg.save_chain_target = None;
                            }
                        });
                    }
                    return;
                }

                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("fx_add")
                        .selected_text(tr(lang, "+ Adicionar efeito..."))
                        .show_ui(ui, |ui| {
                            let list: &[FxKind] = if audio {
                                &FxKind::AUDIO
                            } else {
                                &FxKind::VIDEO
                            };
                            for k in list {
                                if ui.selectable_label(false, k.name_in(lang)).clicked() {
                                    add_item = Some(FxChooserItem::Native(*k));
                                }
                            }
                        });
                    egui::ComboBox::from_id_salt("fx_saved_chains")
                        .selected_text(tr(lang, "Cadeias salvas..."))
                        .show_ui(ui, |ui| {
                            let mut any_match = false;
                            for (ci, c) in saved_chains_snapshot.iter().enumerate() {
                                if c.audio != audio {
                                    continue;
                                }
                                any_match = true;
                                ui.horizontal(|ui| {
                                    if ui
                                        .selectable_label(
                                            false,
                                            format!("{} ({})", c.name, c.items.len()),
                                        )
                                        .clicked()
                                    {
                                        apply_saved_chain = Some(ci);
                                    }
                                    if ui.small_button("×").on_hover_text(tr(lang, "Excluir cadeia")).clicked() {
                                        delete_saved_chain = Some(ci);
                                    }
                                });
                            }
                            if !any_match {
                                ui.label(egui::RichText::new(tr(lang, "Nenhuma cadeia salva ainda.")).small().weak());
                            }
                            ui.separator();
                            if ui.button(tr(lang, "Exportar cadeia (.neovegfxchain)...")).clicked() {
                                export_chain_file = true;
                            }
                            if ui.button(tr(lang, "Importar cadeia (.neovegfxchain)...")).clicked() {
                                import_chain_file = true;
                            }
                        });
                    let has_saveable = chain_copy
                        .iter()
                        .any(|fx| !matches!(fx.kind, FxKind::Ofx | FxKind::Vst3));
                    if ui
                        .add_enabled(has_saveable, egui::Button::new(tr(lang, "Salvar cadeia...")))
                        .on_hover_text(tr(lang, "Salvar esta sequência de efeitos como pacote reutilizável (Filter Package)"))
                        .clicked()
                    {
                        self.dlg.save_chain_target = Some(target);
                        if self.dlg.save_chain_name.trim().is_empty() {
                            self.dlg.save_chain_name = if audio {
                                tr(lang, "Cadeia de áudio").into()
                            } else {
                                tr(lang, "Cadeia de vídeo").into()
                            };
                        }
                    }
                    ui.label(
                        egui::RichText::new(tr_args(
                            lang,
                            "tempo local: {time}",
                            &[("time", &fmt_local)],
                        ))
                        .small(),
                    );
                });
                if self.dlg.save_chain_target == Some(target) {
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "Nome da cadeia:"));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.dlg.save_chain_name)
                                .desired_width(160.0),
                        );
                        let valid = !self.dlg.save_chain_name.trim().is_empty();
                        if ui.add_enabled(valid, egui::Button::new(tr(lang, "Salvar"))).clicked() {
                            confirm_save_chain = Some(self.dlg.save_chain_name.trim().to_string());
                        }
                        if ui.button(tr(lang, "Cancelar")).clicked() {
                            self.dlg.save_chain_target = None;
                        }
                    });
                }
                ui.separator();
                if chain_copy.is_empty() {
                    ui.label(tr(lang, "Nenhum efeito na cadeia."));
                }
                egui::ScrollArea::vertical()
                    .max_height(520.0)
                    .show(ui, |ui| {
                        let n = chain_copy.len();
                        for (i, fx) in chain_copy.iter_mut().enumerate() {
                            ui.push_id(fx.id, |ui| {
                                ui.horizontal(|ui| {
                                    changed |= ui
                                        .checkbox(&mut fx.enabled, "")
                                        .on_hover_text(tr(lang, "Ativar/ignorar (bypass)"))
                                        .changed();
                                    let fx_title = if matches!(fx.kind, FxKind::Ofx | FxKind::Vst3)
                                    {
                                        fx.display_name()
                                    } else {
                                        fx.kind.name_in(lang).to_string()
                                    };
                                    ui.label(egui::RichText::new(fx_title).strong());
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if icons::small_icon_button(
                                                ui,
                                                Icon::CloseX,
                                                None,
                                                false,
                                                &self.theme,
                                            )
                                            .on_hover_text(tr(lang, "Remover"))
                                            .clicked()
                                            {
                                                op = Some((i, "del"));
                                            }
                                            if i + 1 < n
                                                && icons::small_icon_button(
                                                    ui,
                                                    Icon::MoveDown,
                                                    None,
                                                    false,
                                                    &self.theme,
                                                )
                                                .on_hover_text(tr(lang, "Mover para baixo"))
                                                .clicked()
                                            {
                                                op = Some((i, "down"));
                                            }
                                            if i > 0
                                                && icons::small_icon_button(
                                                    ui,
                                                    Icon::MoveUp,
                                                    None,
                                                    false,
                                                    &self.theme,
                                                )
                                                .on_hover_text(tr(lang, "Mover para cima"))
                                                .clicked()
                                            {
                                                op = Some((i, "up"));
                                            }
                                            if icons::small_icon_button(
                                                ui,
                                                Icon::ResetSpin,
                                                None,
                                                false,
                                                &self.theme,
                                            )
                                            .on_hover_text(tr(lang, "Valores padrão"))
                                            .clicked()
                                            {
                                                op = Some((i, "reset"));
                                            }
                                            if !matches!(fx.kind, FxKind::Ofx | FxKind::Vst3)
                                                && ui
                                                    .small_button("Salvar preset")
                                                    .on_hover_text("Salvar parâmetros atuais como preset próprio no navegador")
                                                    .clicked()
                                            {
                                                self.dlg.save_preset_for = Some((target, i));
                                                self.dlg.save_preset_name =
                                                    format!("{} personalizado", fx.kind.name());
                                            }
                                        },
                                    );
                                });
                                if self.dlg.save_preset_for == Some((target, i)) {
                                    ui.horizontal(|ui| {
                                        ui.label("Preset:");
                                        ui.add(
                                            egui::TextEdit::singleline(
                                                &mut self.dlg.save_preset_name,
                                            )
                                            .desired_width(150.0),
                                        );
                                        let valid = !self.dlg.save_preset_name.trim().is_empty();
                                        if ui.add_enabled(valid, egui::Button::new("Salvar")).clicked()
                                        {
                                            confirm_save_preset = Some((
                                                fx.clone(),
                                                self.dlg.save_preset_name.trim().to_string(),
                                            ));
                                        }
                                        if ui.button("Cancelar").clicked() {
                                            self.dlg.save_preset_for = None;
                                        }
                                    });
                                }
                                if fx.kind == FxKind::ColorCorrector
                                    && ui
                                        .small_button(
                                            "Editar LUT (.cube) e Curvas RGB/HSL no Color Grading",
                                        )
                                        .clicked()
                                {
                                    open_color_grading = true;
                                }
                                if fx.kind == FxKind::Ofx {
                                    changed |= crate::plugin_manager::ofx_params_editor(
                                        ui,
                                        lang,
                                        fx,
                                        &self.theme,
                                        local_t,
                                        tol,
                                    );
                                }
                                if fx.kind == FxKind::Vst3 {
                                    changed |= crate::plugin_manager::vst3_params_editor(
                                        ui,
                                        lang,
                                        fx,
                                        &self.theme,
                                        local_t,
                                        tol,
                                    );
                                }
                                let specs = fx.kind.params();
                                egui::Grid::new("params").num_columns(3).show(ui, |ui| {
                                    for (pi, spec) in specs.iter().enumerate() {
                                        let Some(p) = fx.params.get_mut(pi) else {
                                            continue;
                                        };
                                        ui.label(spec.name_in(lang));
                                        let mut v = p.eval(local_t);
                                        let r = if spec.toggle {
                                            let mut b = v > 0.5;
                                            let r = ui.checkbox(&mut b, "");
                                            v = if b { 1.0 } else { 0.0 };
                                            r
                                        } else {
                                            let mut sl =
                                                egui::Slider::new(&mut v, spec.min..=spec.max);
                                            if spec.max > 1000.0
                                                || (spec.min > 0.0
                                                    && spec.max / spec.min.max(1e-3) > 100.0)
                                            {
                                                sl = sl.logarithmic(true);
                                            }
                                            ui.add(sl)
                                        };
                                        if r.changed() {
                                            if p.keys.is_empty() {
                                                p.value = v;
                                            } else {
                                                p.set_key(local_t, v, tol);
                                            }
                                            changed = true;
                                        }
                                        ui.horizontal(|ui| {
                                            let has_key_here =
                                                p.keys.iter().any(|k| k.0.abs_diff(local_t) <= tol);
                                            if icons::small_icon_button(
                                                ui,
                                                if has_key_here {
                                                    Icon::KeyframeFilled
                                                } else {
                                                    Icon::KeyframeEmpty
                                                },
                                                None,
                                                has_key_here,
                                                &self.theme,
                                            )
                                            .on_hover_text(tr(lang, "Criar keyframe neste tempo"))
                                            .clicked()
                                            {
                                                if p.keys.is_empty() && local_t > tol {
                                                    p.keys.push((Ticks::ZERO, p.value));
                                                }
                                                p.set_key(local_t, v, tol);
                                                changed = true;
                                            }
                                            if !p.keys.is_empty() {
                                                ui.label(
                                                    egui::RichText::new(format!(
                                                        "{}",
                                                        p.keys.len()
                                                    ))
                                                    .small(),
                                                );
                                                if icons::small_icon_button(
                                                    ui,
                                                    Icon::Backspace,
                                                    None,
                                                    false,
                                                    &self.theme,
                                                )
                                                .on_hover_text(tr(lang, "Remover keyframes"))
                                                .clicked()
                                                {
                                                    p.value = v;
                                                    p.keys.clear();
                                                    changed = true;
                                                }
                                            }
                                        });
                                        ui.end_row();
                                    }
                                });
                                ui.separator();
                            });
                        }
                    });
            });
        self.dlg.fx_chooser_open = chooser_open;
        self.dlg.fx_chooser_cat = chooser_cat;
        self.dlg.fx_chooser_filter = chooser_filter;
        self.dlg.fx_chooser_selected = chooser_selected;
        self.dlg.fx_selected_slot = sel_slot;
        let make_instances = |item: FxChooserItem| -> Vec<FxInstance> {
            match item {
                FxChooserItem::Native(k) => vec![FxInstance::new(k)],
                FxChooserItem::SavedChain(ci) => self
                    .prefs
                    .fx_chains
                    .get(ci)
                    .map(EditorSession::instantiate_saved_fx_chain)
                    .unwrap_or_default(),
                FxChooserItem::Ofx {
                    binary,
                    identifier,
                    label,
                } => vec![FxInstance::new_ofx(&binary, &identifier, &label)],
                FxChooserItem::Vst3 {
                    binary,
                    class_id,
                    name,
                    is_instrument,
                } => vec![FxInstance::new_vst3_with_kind(
                    &binary,
                    &class_id,
                    &name,
                    is_instrument,
                )],
            }
        };
        if let Some((slot, item)) = replace_item {
            if slot < chain_copy.len() {
                let insts = make_instances(item);
                if let Some(first) = insts.into_iter().next() {
                    chain_copy[slot] = first;
                    self.dlg.fx_selected_slot = Some(slot);
                    changed = true;
                }
            }
        }
        if let Some(item) = add_item {
            let insts = make_instances(item);
            if !insts.is_empty() {
                chain_copy.extend(insts);
                self.dlg.fx_selected_slot = Some(chain_copy.len().saturating_sub(1));
                changed = true;
            }
        }
        if chooser_ok {
            self.dlg.fx_chooser_open = false;
            self.dlg.fx_chooser_snapshot = None;
            if chain_copy.is_empty() {
                open = false;
            }
        }
        if chooser_cancel {
            if let Some(snap) = self.dlg.fx_chooser_snapshot.take() {
                let was_empty = snap.is_empty();
                chain_copy = snap;
                changed = true;
                if was_empty {
                    open = false;
                }
            }
            self.dlg.fx_chooser_open = false;
        }
        if let Some((i, what)) = op {
            match what {
                "del" => {
                    chain_copy.remove(i);
                }
                "up" => chain_copy.swap(i, i - 1),
                "down" => chain_copy.swap(i, i + 1),
                "reset" => {
                    let id = chain_copy[i].id;
                    // OFX/VST3: mantém o plug-in e volta só os valores ao padrão dele
                    let ofx = chain_copy[i].ofx.take().map(|mut o| {
                        o.params.clear();
                        o
                    });
                    let vst3 = chain_copy[i].vst3.take().map(|mut v| {
                        v.params.clear();
                        v.state = None;
                        v
                    });
                    chain_copy[i] = FxInstance::new(chain_copy[i].kind);
                    chain_copy[i].id = id;
                    chain_copy[i].ofx = ofx;
                    chain_copy[i].vst3 = vst3;
                }
                _ => {}
            }
            changed = true;
        }
        if let Some(ci) = apply_saved_chain {
            if let Some(saved) = self.prefs.fx_chains.get(ci).cloned() {
                let items = EditorSession::instantiate_saved_fx_chain(&saved);
                if !items.is_empty() {
                    self.checkpoint();
                    chain_copy.extend(items);
                    changed = true;
                    self.set_status(format!("Cadeia \"{}\" aplicada.", saved.name));
                }
            }
        }
        if let Some(ci) = delete_saved_chain {
            if ci < self.prefs.fx_chains.len() {
                self.prefs.fx_chains.remove(ci);
                let _ = self.prefs.save();
            }
        }
        if let Some(name) = confirm_save_chain {
            if let Some(chain_pref) =
                EditorSession::capture_saved_fx_chain(&name, audio, &chain_copy)
            {
                self.prefs
                    .fx_chains
                    .retain(|c| !(c.audio == audio && c.name == chain_pref.name));
                self.prefs.fx_chains.push(chain_pref);
                self.prefs.sanitize();
                let _ = self.prefs.save();
                self.dlg.save_chain_target = None;
                self.set_status(format!("Cadeia \"{name}\" salva nas Preferências."));
            }
        }
        if let Some((fx, name)) = confirm_save_preset {
            let mut preset = neoveg_common::prefs::UserFxPresetPref {
                name: name.clone(),
                fx_kind: fx.kind.id().to_string(),
                values: fx.values(local_t),
            };
            if preset.sanitize() {
                self.prefs
                    .user_fx_presets
                    .retain(|p| !(p.fx_kind == preset.fx_kind && p.name == preset.name));
                self.prefs.user_fx_presets.push(preset);
                self.prefs.sanitize();
                let _ = self.prefs.save();
                self.dlg.save_preset_for = None;
                self.set_status(format!("Preset \"{name}\" salvo para {}.", fx.kind.name()));
            }
        }
        if export_chain_file {
            if let Some(chain_pref) =
                EditorSession::capture_saved_fx_chain("Cadeia exportada", audio, &chain_copy)
            {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Cadeia Neoveg FX", &["neovegfxchain", "json"])
                    .set_file_name("cadeia.neovegfxchain")
                    .save_file()
                {
                    match serde_json::to_string_pretty(&chain_pref) {
                        Ok(json) => {
                            if let Err(e) = std::fs::write(&path, json) {
                                self.set_status(format!("Erro exportando cadeia: {e}"));
                            } else {
                                self.set_status("Cadeia de efeitos exportada.");
                            }
                        }
                        Err(e) => self.set_status(format!("Erro serializando cadeia: {e}")),
                    }
                }
            }
        }
        if import_chain_file {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Cadeia Neoveg FX", &["neovegfxchain", "json"])
                .pick_file()
            {
                match std::fs::read_to_string(&path).ok().and_then(|s| {
                    serde_json::from_str::<neoveg_common::prefs::SavedFxChainPref>(&s).ok()
                }) {
                    Some(mut pref) => {
                        if pref.sanitize() && pref.audio == audio {
                            let items = EditorSession::instantiate_saved_fx_chain(&pref);
                            if !items.is_empty() {
                                self.checkpoint();
                                chain_copy.extend(items);
                                changed = true;
                                self.prefs
                                    .fx_chains
                                    .retain(|c| !(c.audio == audio && c.name == pref.name));
                                self.prefs.fx_chains.push(pref);
                                self.prefs.sanitize();
                                let _ = self.prefs.save();
                                self.set_status("Cadeia de efeitos importada e aplicada.");
                            }
                        } else {
                            self.set_status(
                                "Arquivo de cadeia incompatível (áudio/vídeo) ou inválido.",
                            );
                        }
                    }
                    None => self.set_status("Não foi possível ler a cadeia .neovegfxchain."),
                }
            }
        }
        if open_color_grading {
            self.exec_cmd(Some(ctx), "view.tab.color_grading");
        }
        if changed {
            if let Some(chain) = self.fx_chain_mut(target) {
                *chain = chain_copy;
            }
            self.dialog_change();
        }
        if !open {
            self.dlg.fx_target = None;
        }
    }

    // -----------------------------------------------------------------
    // Mídia gerada
    // -----------------------------------------------------------------

    fn gen_editor_window(&mut self, ctx: &egui::Context) {
        let Some(mid) = self.dlg.gen_editor else {
            return;
        };
        let Some(mi) = self.project.media.iter().position(|m| m.id == mid) else {
            self.dlg.gen_editor = None;
            return;
        };
        let MediaSource::Generated(mut spec) = self.project.media[mi].source.clone() else {
            self.dlg.gen_editor = None;
            return;
        };
        if self.gen_fonts.is_empty() {
            self.gen_fonts = crate::media::generate::list_system_fonts();
            if self.gen_fonts.is_empty() {
                self.gen_fonts.push(String::new());
            }
        }
        let lang = self.lang();
        let fonts = self.gen_fonts.clone();
        let mut open = true;
        let before = spec.clone();
        // centralizada: sem posição, o egui a punha no primeiro espaço livre,
        // por cima dos cabeçalhos de faixa
        crate::dialog_window::DialogWindow::new(tr(lang, "Mídia gerada"))
            .id(egui::Id::new("gen_editor_win"))
            .open(&mut open)
            .default_width(420.0)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.screen_rect().center())
            .show(ctx, |ui| match &mut spec {
                GenSpec::Text(t) => {
                    ui.label(tr(lang, "Texto:"));
                    ui.add(
                        egui::TextEdit::multiline(&mut t.text)
                            .desired_rows(4)
                            .desired_width(f32::INFINITY),
                    );
                    egui::Grid::new("gen_txt").num_columns(2).show(ui, |ui| {
                        ui.label(tr(lang, "Fonte"));
                        let cur = if t.font_path.is_empty() {
                            tr(lang, "Padrão").to_string()
                        } else {
                            Path::new(&t.font_path)
                                .file_name()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_default()
                        };
                        egui::ComboBox::from_id_salt("font")
                            .selected_text(cur)
                            .height(300.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut t.font_path,
                                    String::new(),
                                    tr(lang, "Padrão"),
                                );
                                for f in &fonts {
                                    let n = Path::new(f)
                                        .file_name()
                                        .map(|s| s.to_string_lossy().to_string())
                                        .unwrap_or_default();
                                    ui.selectable_value(&mut t.font_path, f.clone(), n);
                                }
                            });
                        ui.end_row();
                        ui.label(tr(lang, "Tamanho (% altura)"));
                        ui.add(egui::Slider::new(&mut t.size, 1.0..=40.0));
                        ui.end_row();
                        ui.label(tr(lang, "Cor do texto"));
                        ui.color_edit_button_rgba_unmultiplied(&mut t.color);
                        ui.end_row();
                        ui.label(tr(lang, "Contorno"));
                        ui.horizontal(|ui| {
                            ui.add(egui::Slider::new(&mut t.outline, 0.0..=3.0));
                            ui.color_edit_button_rgba_unmultiplied(&mut t.outline_color);
                        });
                        ui.end_row();
                        ui.label(tr(lang, "Sombra"));
                        ui.checkbox(&mut t.shadow, "");
                        ui.end_row();
                        ui.label(tr(lang, "Fundo"));
                        ui.color_edit_button_rgba_unmultiplied(&mut t.background);
                        ui.end_row();
                        ui.label(tr(lang, "Alinhamento"));
                        ui.horizontal(|ui| {
                            ui.selectable_value(
                                &mut t.align,
                                TextAlign::Left,
                                tr(lang, "Esquerda"),
                            );
                            ui.selectable_value(
                                &mut t.align,
                                TextAlign::Center,
                                tr(lang, "Centro"),
                            );
                            ui.selectable_value(
                                &mut t.align,
                                TextAlign::Right,
                                tr(lang, "Direita"),
                            );
                        });
                        ui.end_row();
                        ui.label(tr(lang, "Posição X / Y"));
                        ui.horizontal(|ui| {
                            ui.add(egui::Slider::new(&mut t.x, 0.0..=1.0));
                            ui.add(egui::Slider::new(&mut t.y, 0.0..=1.0));
                        });
                        ui.end_row();
                        ui.label(tr(lang, "Espaçamento de linha"));
                        ui.add(egui::Slider::new(&mut t.line_spacing, 0.6..=3.0));
                        ui.end_row();
                        ui.label(tr(lang, "Espaçamento de caracteres (Tracking)"));
                        ui.add(egui::Slider::new(&mut t.tracking, -0.2..=1.0).step_by(0.01));
                        ui.end_row();
                        ui.label(tr(lang, "Quebra automática de linha"));
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut t.wrap, "");
                            if t.wrap {
                                ui.add(
                                    egui::Slider::new(&mut t.wrap_width, 10.0..=100.0).suffix("%"),
                                );
                            }
                        });
                        ui.end_row();
                        ui.label(tr(lang, "Animação"));
                        egui::ComboBox::from_id_salt("anim")
                            .selected_text(t.anim.name_in(lang))
                            .show_ui(ui, |ui| {
                                for a in TextAnim::ALL {
                                    ui.selectable_value(&mut t.anim, a, a.name_in(lang));
                                }
                            });
                        ui.end_row();
                        ui.label(tr(lang, "Duração da animação (s)"));
                        ui.add(egui::Slider::new(&mut t.anim_duration, 0.5..=60.0));
                        ui.end_row();
                    });
                }
                GenSpec::SolidColor { color } => {
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "Cor"));
                        ui.color_edit_button_rgba_unmultiplied(color);
                    });
                }
                GenSpec::Gradient {
                    a,
                    b,
                    angle,
                    radial,
                } => {
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "Cor inicial"));
                        ui.color_edit_button_rgba_unmultiplied(a);
                        ui.label(tr(lang, "Cor final"));
                        ui.color_edit_button_rgba_unmultiplied(b);
                    });
                    ui.checkbox(radial, tr(lang, "Radial"));
                    ui.add(egui::Slider::new(angle, -180.0..=180.0).text(tr(lang, "Ângulo")));
                }
                GenSpec::Checkerboard { a, b, cells } => {
                    ui.horizontal(|ui| {
                        ui.color_edit_button_rgba_unmultiplied(a);
                        ui.color_edit_button_rgba_unmultiplied(b);
                    });
                    ui.add(egui::Slider::new(cells, 1..=64).text(tr(lang, "Células (vertical)")));
                }
                GenSpec::ColorBars => {
                    ui.label(tr(lang, "Barras de cor SMPTE (sem parâmetros)."));
                }
                GenSpec::Noise { mono, animated } => {
                    ui.checkbox(mono, tr(lang, "Monocromático"));
                    ui.checkbox(animated, tr(lang, "Animado"));
                }
                GenSpec::TestPattern { pattern } => {
                    let names = [
                        "Grade de convergência",
                        "Rampa de cinza",
                        "Placa de zona",
                        "Grade de pixels",
                    ];
                    let cur = names.get(*pattern as usize).copied().unwrap_or(names[0]);
                    egui::ComboBox::from_label(tr(lang, "Padrão"))
                        .selected_text(tr(lang, cur))
                        .show_ui(ui, |ui| {
                            for (i, n) in names.iter().enumerate() {
                                ui.selectable_value(pattern, i as u32, tr(lang, n));
                            }
                        });
                }
                GenSpec::NoiseTexture {
                    a,
                    b,
                    scale,
                    octaves,
                    animated,
                } => {
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "Cor inicial"));
                        ui.color_edit_button_rgba_unmultiplied(a);
                        ui.label(tr(lang, "Cor final"));
                        ui.color_edit_button_rgba_unmultiplied(b);
                    });
                    ui.add(egui::Slider::new(scale, 0.01..=1.0).text(tr(lang, "Escala")));
                    ui.add(egui::Slider::new(octaves, 1..=8).text(tr(lang, "Detalhe")));
                    ui.checkbox(animated, tr(lang, "Animado"));
                }
                GenSpec::Ofx(ofx_ref) => {
                    let mut tmp_fx = crate::model::FxInstance::new(FxKind::Ofx);
                    tmp_fx.id = mid;
                    tmp_fx.ofx = Some(ofx_ref.clone());
                    let tol = Ticks((self.project.settings.frame_dur().0 / 2).max(1));
                    let local_t = self.cursor;
                    if crate::plugin_manager::ofx_params_editor(
                        ui,
                        lang,
                        &mut tmp_fx,
                        &self.theme,
                        local_t,
                        tol,
                    ) {
                        if let Some(updated) = tmp_fx.ofx {
                            *ofx_ref = updated;
                        }
                    }
                }
            });
        if spec != before {
            self.dlg.edit_in_progress = self.dlg.edit_in_progress && true;
            let label = spec.label_in(lang);
            let m = &mut self.project.media[mi];
            m.source = MediaSource::Generated(spec);
            m.name = label.clone();
            for t in &mut self.project.tracks {
                for e in &mut t.events {
                    if e.media_id == mid {
                        e.name = label.clone();
                    }
                }
            }
            self.media.invalidate_generated(mid);
            self.thumbs.remove(&mid);
            self.dialog_change();
        }
        if !open {
            self.dlg.gen_editor = None;
        }
    }

    // -----------------------------------------------------------------
    // Renderização
    // -----------------------------------------------------------------

    fn render_window(&mut self, ctx: &egui::Context) {
        if self.dlg.render.is_none() {
            return;
        }
        let lang = self.lang();
        let mut open = true;
        let mut start = false;
        let mut send_encoder = false;
        let has_loop = self.project.loop_region.is_some();
        let pw = self.project.settings.width;
        let ph = self.project.settings.height;
        let fps = self.project.settings.fps;
        let sr = self.project.settings.sample_rate;
        let dur = self.project.duration();
        let loop_r = self.project.loop_region;
        let fmt_dur = self.fmt_time(dur);
        let fmt_loop = loop_r.map(|(a, b)| format!("{} → {}", self.fmt_time(a), self.fmt_time(b)));
        let mut use_loop = self.dlg.render_use_loop;
        let mut toggle_fav: Option<String> = None;
        let mut apply_tmpl: Option<RenderTemplate> = None;
        let mut cancel_btn = false;
        let favs = self.prefs.render_favorites.clone();
        let (range_a, range_b) = match (use_loop && has_loop, loop_r) {
            (true, Some(lr)) => lr,
            _ => (Ticks::ZERO, dur),
        };
        let range_ticks = (range_b - range_a).max(Ticks::ZERO);
        let est_frames = fps.ticks_to_frame_round(range_ticks).max(0);
        let est_len_fmt = self.fmt_time(range_ticks);
        let Some(r) = self.dlg.render.as_mut() else {
            return;
        };

        crate::dialog_window::DialogWindow::new(tr(lang, "Renderizar como"))
            .id(egui::Id::new("render_as_win"))
            .open(&mut open)
            .resizable(true)
            .default_size([740.0, 440.0])
            .min_width(560.0)
            .show(ctx, |ui| {
                ui.columns(2, |cols| {
                    // Coluna esquerda: Formatos, pesquisa, favoritos e modelos (templates)
                    let left = &mut cols[0];
                    left.label(egui::RichText::new(tr(lang, "Formatos e modelos")).strong());
                    left.add_space(3.0);
                    left.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut r.filter)
                                .hint_text(tr(lang, "Filtrar formatos ou modelos..."))
                                .desired_width(175.0),
                        );
                        ui.checkbox(&mut r.only_favorites, tr(lang, "★ Favoritos"));
                    });
                    left.separator();
                    let query = r.filter.trim().to_lowercase();
                    egui::ScrollArea::vertical()
                        .id_salt("render_templates_scroll")
                        .max_height(280.0)
                        .show(left, |ui| {
                            let mut any = false;
                            for fmt in RenderFormat::ALL {
                                let matching: Vec<&RenderTemplate> = RENDER_TEMPLATES
                                    .iter()
                                    .filter(|t| t.format == fmt)
                                    .filter(|t| {
                                        if r.only_favorites && !favs.iter().any(|f| f == t.id) {
                                            return false;
                                        }
                                        if query.is_empty() {
                                            return true;
                                        }
                                        tr(lang, t.title).to_lowercase().contains(&query)
                                            || t.title.to_lowercase().contains(&query)
                                            || tr(lang, t.summary).to_lowercase().contains(&query)
                                            || fmt.name_in(lang).to_lowercase().contains(&query)
                                    })
                                    .collect();
                                if matching.is_empty() {
                                    continue;
                                }
                                any = true;
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} (.{})",
                                        fmt.name_in(lang),
                                        fmt.ext()
                                    ))
                                    .small()
                                    .strong()
                                    .color(self.theme.accent),
                                );
                                for t in matching {
                                    let is_fav = favs.iter().any(|f| f == t.id);
                                    let selected = r.template_id == t.id && r.format == t.format;
                                    ui.horizontal(|ui| {
                                        if ui
                                            .small_button(if is_fav { "★" } else { "☆" })
                                            .on_hover_text(tr(lang, "Alternar favorito"))
                                            .clicked()
                                        {
                                            toggle_fav = Some(t.id.to_string());
                                        }
                                        if ui.selectable_label(selected, tr(lang, t.title)).clicked() {
                                            apply_tmpl = Some(*t);
                                        }
                                    });
                                }
                                ui.add_space(4.0);
                            }
                            if !any {
                                ui.label(
                                    egui::RichText::new(tr(
                                        lang,
                                        "Nenhum modelo corresponde ao filtro atual.",
                                    ))
                                    .small()
                                    .weak(),
                                );
                            }
                        });
                    if let Some(active_t) = RENDER_TEMPLATES.iter().find(|t| t.id == r.template_id)
                    {
                        left.separator();
                        left.group(|ui| {
                            ui.label(egui::RichText::new(tr(lang, active_t.title)).small().strong());
                            ui.label(
                                egui::RichText::new(tr(lang, active_t.summary))
                                    .small()
                                    .weak(),
                            );
                        });
                    }

                    // Coluna direita: Arquivo de saída, parâmetros de saída, intervalo e resumo
                    let right = &mut cols[1];
                    right.label(egui::RichText::new(tr(lang, "Destino e configurações")).strong());
                    right.add_space(3.0);

                    if let Some(tmpl) = apply_tmpl {
                        r.format = tmpl.format;
                        r.template_id = tmpl.id.to_string();
                        if tmpl.half_project {
                            r.width = ((pw / 2) / 2).max(8) * 2;
                            r.height = ((ph / 2) / 2).max(8) * 2;
                        } else if let Some((tw, th)) = tmpl.resolution {
                            r.width = tw;
                            r.height = th;
                        } else {
                            r.width = pw;
                            r.height = ph;
                        }
                        r.quality = tmpl.quality;
                        r.include_audio = tmpl.include_audio;
                        r.wav_bits = tmpl.wav_bits;
                        if tmpl.video_bitrate_kbps > 0 {
                            r.video_bitrate_kbps = tmpl.video_bitrate_kbps;
                        }
                        if tmpl.audio_bitrate_kbps > 0 {
                            r.audio_bitrate_kbps = tmpl.audio_bitrate_kbps;
                        }
                        r.path = PathBuf::from(&r.path)
                            .with_extension(r.format.ext())
                            .to_string_lossy()
                            .to_string();
                    }

                    egui::Grid::new("rg_right").num_columns(2).show(right, |ui| {
                        ui.label(tr(lang, "Formato"));
                        let before = r.format;
                        egui::ComboBox::from_id_salt("rfmt")
                            .selected_text(r.format.name_in(lang))
                            .show_ui(ui, |ui| {
                                for f in RenderFormat::ALL {
                                    ui.selectable_value(&mut r.format, f, f.name_in(lang));
                                }
                            });
                        if before != r.format {
                            r.path = PathBuf::from(&r.path)
                                .with_extension(r.format.ext())
                                .to_string_lossy()
                                .to_string();
                            if let Some(first_t) =
                                RENDER_TEMPLATES.iter().find(|t| t.format == r.format)
                            {
                                r.template_id = first_t.id.to_string();
                            }
                        }
                        ui.end_row();

                        ui.label(tr(lang, "Arquivo"));
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut r.path).desired_width(240.0));
                            if ui
                                .button("...")
                                .on_hover_text(tr(lang, "Procurar destino"))
                                .clicked()
                            {
                                let ext = r.format.ext();
                                if let Some(p) = rfd::FileDialog::new()
                                    .add_filter(ext.to_uppercase(), &[ext])
                                    .set_file_name(
                                        PathBuf::from(&r.path)
                                            .file_name()
                                            .map(|s| s.to_string_lossy().to_string())
                                            .unwrap_or_default(),
                                    )
                                    .save_file()
                                {
                                    r.path = p.to_string_lossy().to_string();
                                }
                            }
                        });
                        ui.end_row();

                        if !r.format.is_audio_only() {
                            ui.label(tr(lang, "Resolução"));
                            ui.horizontal(|ui| {
                                ui.add(egui::DragValue::new(&mut r.width).range(16..=8192));
                                ui.label("x");
                                ui.add(egui::DragValue::new(&mut r.height).range(16..=8192));
                                if ui.small_button(tr(lang, "Projeto")).clicked() {
                                    r.width = pw;
                                    r.height = ph;
                                }
                                if ui.small_button("1/2").clicked() {
                                    r.width = pw / 2;
                                    r.height = ph / 2;
                                }
                            });
                            r.width = (r.width / 2).max(8) * 2;
                            r.height = (r.height / 2).max(8) * 2;
                            ui.end_row();
                        }
                        if r.format.uses_jpeg_quality() {
                            ui.label(tr(lang, "Qualidade JPEG"));
                            ui.add(egui::Slider::new(&mut r.quality, 30..=100));
                            ui.end_row();
                        }
                        if r.format.uses_video_bitrate() {
                            ui.label(tr(lang, "Taxa de bits vídeo"));
                            ui.add(
                                egui::DragValue::new(&mut r.video_bitrate_kbps)
                                    .range(500..=100_000)
                                    .suffix(" kbps"),
                            );
                            ui.end_row();
                        }
                        if r.format.uses_audio_bitrate() {
                            ui.label(tr(lang, "Taxa de bits áudio"));
                            ui.add(
                                egui::DragValue::new(&mut r.audio_bitrate_kbps)
                                    .range(64..=512)
                                    .suffix(" kbps"),
                            );
                            ui.end_row();
                        }
                        if !r.format.is_audio_only() {
                            ui.label(tr(lang, "Áudio"));
                            ui.checkbox(
                                &mut r.include_audio,
                                if r.format.has_embedded_audio() {
                                    tr(lang, "Incluir áudio no arquivo")
                                } else {
                                    tr(lang, "Gerar WAV ao lado")
                                },
                            );
                            ui.end_row();
                        }
                        if r.format.uses_wav_bits() {
                            ui.label(tr(lang, "Profundidade PCM"));
                            ui.horizontal(|ui| {
                                ui.selectable_value(
                                    &mut r.wav_bits,
                                    WavBits::Pcm16,
                                    tr(lang, "16 bits"),
                                );
                                ui.selectable_value(
                                    &mut r.wav_bits,
                                    WavBits::Pcm24,
                                    tr(lang, "24 bits"),
                                );
                                ui.selectable_value(
                                    &mut r.wav_bits,
                                    WavBits::Float32,
                                    tr(lang, "32 bits float"),
                                );
                            });
                            ui.end_row();
                        }
                        ui.label(tr(lang, "Intervalo"));
                        ui.vertical(|ui| {
                            ui.radio_value(
                                &mut use_loop,
                                false,
                                tr_args(lang, "Projeto inteiro ({dur})", &[("dur", &fmt_dur)]),
                            );
                            let loop_str = fmt_loop.clone().unwrap_or_default();
                            ui.add_enabled_ui(has_loop, |ui| {
                                ui.radio_value(
                                    &mut use_loop,
                                    true,
                                    tr_args(
                                        lang,
                                        "Somente a região de loop {range}",
                                        &[("range", &loop_str)],
                                    ),
                                );
                            });
                        });
                        ui.end_row();
                    });

                    right.add_space(6.0);
                    right.group(|ui| {
                        ui.label(
                            egui::RichText::new(tr(lang, "Resumo da saída"))
                                .small()
                                .strong(),
                        );
                        let video_line = if r.format.is_audio_only() {
                            tr_args(lang, "Somente áudio • {sr} Hz", &[("sr", &sr)])
                        } else {
                            let fps_l = fps.label_in(lang);
                            tr_args(
                                lang,
                                "{w}×{h} • {fps} • {frames} quadro(s) • {sr} Hz",
                                &[
                                    ("w", &r.width),
                                    ("h", &r.height),
                                    ("fps", &fps_l),
                                    ("frames", &est_frames),
                                    ("sr", &sr),
                                ],
                            )
                        };
                        ui.label(egui::RichText::new(video_line).small());
                        ui.label(
                            egui::RichText::new(tr_args(
                                lang,
                                "Duração selecionada: {dur}",
                                &[("dur", &est_len_fmt)],
                            ))
                            .small()
                            .weak(),
                        );
                        if r.format == RenderFormat::AviMjpeg {
                            ui.label(
                                egui::RichText::new(tr(
                                    lang,
                                    "AVI Motion-JPEG é compatível com VLC e editores externos (acima de 1 GB usa as extensões OpenDML).",
                                ))
                                .small()
                                .weak(),
                            );
                        } else if r.format.uses_windows_mf() {
                            ui.label(
                                egui::RichText::new(tr(
                                    lang,
                                    "Codificação acelerada via Windows Media Foundation (IMFSinkWriter).",
                                ))
                                .small()
                                .weak(),
                            );
                        }
                    });

                    right.add_space(6.0);
                    right.horizontal_wrapped(|ui| {
                        if icons::icon_button(
                            ui,
                            Icon::RenderClapper,
                            Some(tr(lang, "Renderizar")),
                            false,
                            true,
                            &self.theme,
                        )
                        .clicked()
                        {
                            start = true;
                        }
                        if icons::icon_button(
                            ui,
                            Icon::EncoderQueue,
                            Some(tr(lang, "Enviar para o Encoder")),
                            false,
                            true,
                            &self.theme,
                        )
                        .on_hover_text(tr(lang, "Adiciona a timeline à fila em lote do Neoveg Encoder"))
                        .clicked()
                        {
                            send_encoder = true;
                        }
                        if ui.button(tr(lang, "Cancelar")).clicked() {
                            cancel_btn = true;
                        }
                    });
                });
            });

        if cancel_btn {
            open = false;
        }
        if send_encoder {
            self.send_timeline_to_encoder();
            open = false;
        }

        if let Some(fav_id) = toggle_fav {
            if let Some(pos) = self
                .prefs
                .render_favorites
                .iter()
                .position(|x| x == &fav_id)
            {
                self.prefs.render_favorites.remove(pos);
            } else {
                self.prefs.render_favorites.push(fav_id);
            }
            if let Err(e) = self.prefs.save() {
                let err_s = e.to_string();
                self.set_status(tr_args(
                    lang,
                    "Aviso ao salvar favoritos de renderização: {err}",
                    &[("err", &err_s)],
                ));
            }
        }
        self.dlg.render_use_loop = use_loop && has_loop;
        if start {
            let Some(r) = self.dlg.render.as_ref() else {
                return;
            };
            let (a, b) = match (self.dlg.render_use_loop, loop_r) {
                (true, Some(lr)) => lr,
                _ => (Ticks::ZERO, dur),
            };
            if b - a <= Ticks::ZERO {
                self.set_status(tr(lang, "Nada para renderizar (projeto vazio)"));
            } else if self.render_job.is_some() {
                self.set_status(tr(lang, "Já existe uma renderização em andamento"));
            } else {
                let settings = RenderSettings {
                    format: r.format,
                    path: PathBuf::from(&r.path),
                    width: r.width,
                    height: r.height,
                    start: a,
                    end: b,
                    jpeg_quality: r.quality,
                    include_audio: r.include_audio,
                    wav_bits: r.wav_bits,
                    video_bitrate_kbps: r.video_bitrate_kbps,
                    audio_bitrate_kbps: r.audio_bitrate_kbps,
                };
                self.project.sort_events();
                let progress = crate::render::spawn(
                    self.project.clone(),
                    self.media.clone(),
                    settings.clone(),
                );
                self.render_job = Some(RenderJob { progress, settings });
                open = false;
            }
        }
        if !open {
            self.dlg.render = None;
        }
    }

    fn render_progress_window(&mut self, ctx: &egui::Context) {
        let lang = self.lang();
        if let Some(job) = &self.render_job {
            let p = &job.progress;
            let frac = p.fraction();
            let started = p
                .started
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .unwrap_or_else(std::time::Instant::now);
            let el = started.elapsed().as_secs_f64();
            let eta = if frac > 0.001 {
                el / frac as f64 - el
            } else {
                0.0
            };
            let mut cancel = false;
            let theme = self.theme.clone();
            crate::dialog_window::DialogWindow::new(tr(lang, "Renderizando..."))
                .id(egui::Id::new("render_prog_win"))
                .collapsible(true)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        crate::splash::spinner_inline(ui, 20.0, el, &theme);
                        ui.label(job.settings.path.display().to_string());
                    });
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .show_percentage()
                            .desired_width(360.0),
                    );
                    let done_n = p.done.load(std::sync::atomic::Ordering::Relaxed);
                    let total_n = p.total.load(std::sync::atomic::Ordering::Relaxed);
                    let el_s = format!("{el:.0}");
                    let eta_s = format!("{eta:.0}");
                    ui.label(tr_args(
                        lang,
                        "{done} / {total}   •   decorrido {elapsed} s   •   restante ~{rem} s",
                        &[
                            ("done", &done_n),
                            ("total", &total_n),
                            ("elapsed", &el_s),
                            ("rem", &eta_s),
                        ],
                    ));
                    if ui.button(tr(lang, "Cancelar")).clicked() {
                        cancel = true;
                    }
                });
            if cancel {
                p.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
    }

    fn misc_windows(&mut self, ctx: &egui::Context) {
        let lang = self.lang();
        if let Some(msg) = self.dlg.render_done.clone() {
            let mut close = false;
            // o fechar da janela é o mesmo que OK
            let mut keep = true;
            crate::dialog_window::DialogWindow::new(tr(lang, "Renderização"))
                .id(egui::Id::new("render_done_win"))
                .open(&mut keep)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(msg);
                    if ui.button(tr(lang, "OK")).clicked() {
                        close = true;
                    }
                });
            if close || !keep {
                self.dlg.render_done = None;
            }
        }
        if self.dlg.goto_open {
            let mut open = true;
            let mut go = None;
            crate::dialog_window::DialogWindow::new(tr(lang, "Ir para"))
                .id(egui::Id::new("goto_win"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(tr(
                        lang,
                        "Tempo (hh:mm:ss;ff, segundos ou Nf para quadros):",
                    ));
                    let r = ui.text_edit_singleline(&mut self.dlg.goto_text);
                    r.request_focus();
                    if ui.button(tr(lang, "Ir")).clicked()
                        || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    {
                        let s = &self.project.settings;
                        go = parse_time(
                            &self.dlg.goto_text,
                            s.fps,
                            s.sample_rate,
                            s.effective_time_format(self.opts.time_format),
                        );
                    }
                });
            if let Some(t) = go {
                self.set_cursor(t);
                self.tl.center_on(t);
                open = false;
            }
            self.dlg.goto_open = open;
        }
        if self.dlg.shortcuts {
            self.shortcuts_window(ctx);
        }
        if self.dlg.prefs_open {
            self.prefs_window(ctx);
        }
        if self.dlg.edit_layout_open {
            self.edit_layout_window(ctx);
        }
        if self.dlg.save_layout_open {
            self.save_layout_window(ctx);
        }
        if self.dlg.about {
            let mut open = true;
            crate::dialog_window::DialogWindow::new(tr(lang, "Sobre o Neoveg"))
                .id(egui::Id::new("about_win"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.heading("Neoveg");
                    ui.label(tr_args(
                        lang,
                        "Versão {ver}",
                        &[("ver", &env!("CARGO_PKG_VERSION"))],
                    ));
                    ui.label(tr(
                        lang,
                        "Editor de vídeo não-linear escrito em Rust puro (eframe + wgpu, cpal, claxon, lewton, puremp3, fundsp, rubato, netcorehost, image).",
                    ));
                    ui.label(tr(
                        lang,
                        "Fluxo de trabalho e atalhos baseados na semântica do VEGAS Pro.",
                    ));
                    ui.separator();
                    ui.label(
                        egui::RichText::new(tr(
                            lang,
                            "Formatos de vídeo: AVI (MJPEG/RGB), Y4M, GIF, sequências de imagens e MP4/MOV/MKV/WMV no Windows via Media Foundation. Áudio: WAV/AIFF, FLAC, Ogg Vorbis, MP3 e M4A/AAC (via Media Foundation no Windows).",
                        ))
                        .small(),
                    );
                });
            self.dlg.about = open;
        }
    }

    fn shortcuts_window(&mut self, ctx: &egui::Context) {
        let lang = self.lang();
        if let Some(cmd_id) = self.dlg.capturing_shortcut {
            if let Some(captured) = capture_shortcut(ctx) {
                self.dlg.capturing_shortcut = None;
                if let Some(sc) = captured {
                    let formatted = sc.format_canonical();
                    let disp = sc.format_in(lang);
                    self.prefs
                        .shortcuts
                        .insert(cmd_id.to_string(), Some(formatted));
                    if let Err(e) = self.prefs.save() {
                        let err_s = e.to_string();
                        self.set_status(tr_args(
                            lang,
                            "Aviso ao salvar atalho: {err}",
                            &[("err", &err_s)],
                        ));
                    } else {
                        self.set_status(tr_args(
                            lang,
                            "Atalho '{id}' definido para {sc}",
                            &[("id", &cmd_id), ("sc", &disp)],
                        ));
                    }
                }
            }
        }
        let mut open = true;
        crate::dialog_window::DialogWindow::new(tr(lang, "Atalhos de teclado"))
            .id(egui::Id::new("shortcuts_win"))
            .open(&mut open)
            .default_width(640.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "Filtrar:"));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.dlg.shortcut_filter)
                            .hint_text(tr(lang, "comando, menu ou tecla..."))
                            .desired_width(240.0),
                    );
                    if ui.button(tr(lang, "Restaurar padrões")).clicked() {
                        self.prefs.shortcuts.clear();
                        self.dlg.capturing_shortcut = None;
                        let _ = self.prefs.save();
                        self.set_status(tr(lang, "Atalhos restaurados para os padrões."));
                    }
                });
                if let Some(cap) = self.dlg.capturing_shortcut {
                    ui.colored_label(
                        self.theme.accent,
                        tr_args(
                            lang,
                            "Pressione a nova combinação para '{id}' (Esc para cancelar)...",
                            &[("id", &cap)],
                        ),
                    );
                }
                ui.separator();
                let filter = self.dlg.shortcut_filter.trim().to_lowercase();
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .show(ui, |ui| {
                        egui::Grid::new("sc_dyn")
                            .striped(true)
                            .num_columns(4)
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new(tr(lang, "Menu / Comando")).strong());
                                ui.label(egui::RichText::new(tr(lang, "Atalho ativo")).strong());
                                ui.label("");
                                ui.label("");
                                ui.end_row();
                                for cmd in COMMANDS {
                                    let sc_text = resolved_shortcut(cmd, &self.prefs.shortcuts)
                                        .map(|s| s.format_in(lang))
                                        .unwrap_or_else(|| "-".into());
                                    let menu_l = cmd.menu_path_in(lang);
                                    let lbl_l = cmd.label_in(lang);
                                    if !filter.is_empty() {
                                        let m_loc = menu_l.to_lowercase();
                                        let l_loc = lbl_l.to_lowercase();
                                        let m = cmd.menu_path.to_lowercase();
                                        let l = cmd.label.to_lowercase();
                                        let id_l = cmd.id.to_lowercase();
                                        let s = sc_text.to_lowercase();
                                        if !m_loc.contains(&filter)
                                            && !l_loc.contains(&filter)
                                            && !m.contains(&filter)
                                            && !l.contains(&filter)
                                            && !id_l.contains(&filter)
                                            && !s.contains(&filter)
                                        {
                                            continue;
                                        }
                                    }
                                    ui.label(&menu_l)
                                        .on_hover_text(format!("{} ({})", lbl_l, cmd.id));
                                    ui.label(egui::RichText::new(sc_text).monospace());
                                    let waiting = self.dlg.capturing_shortcut == Some(cmd.id);
                                    if ui
                                        .small_button(if waiting {
                                            tr(lang, "Aguardando...")
                                        } else {
                                            tr(lang, "Gravar")
                                        })
                                        .clicked()
                                    {
                                        self.dlg.capturing_shortcut = Some(cmd.id);
                                    }
                                    if ui.small_button(tr(lang, "Limpar")).clicked() {
                                        self.dlg.capturing_shortcut = None;
                                        self.prefs.shortcuts.insert(cmd.id.to_string(), None);
                                        let _ = self.prefs.save();
                                    }
                                    ui.end_row();
                                }
                            });
                        ui.separator();
                        ui.label(
                            egui::RichText::new(tr(lang, "Gestos de mouse na timeline:")).strong(),
                        );
                        egui::Grid::new("sc_mouse")
                            .striped(true)
                            .num_columns(2)
                            .show(ui, |ui| {
                                for (k, d) in MOUSE_HINTS {
                                    ui.label(egui::RichText::new(tr(lang, k)).monospace());
                                    ui.label(tr(lang, d));
                                    ui.end_row();
                                }
                            });
                    });
            });
        if !open {
            self.dlg.capturing_shortcut = None;
        }
        self.dlg.shortcuts = open;
    }

    fn prefs_window(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.dlg.prefs_draft.take() else {
            self.dlg.prefs_open = false;
            return;
        };
        let lang = self.lang();
        let mut open = true;
        let mut apply_and_keep = false;
        let mut save_and_close = false;
        let mut cancel_close = false;
        let mut reset_defaults = false;
        let mut clear_cache_now = false;
        let mut clear_recents_now = false;
        let mut open_shortcuts = false;
        let mut apply_defaults_to_session = false;
        let mut live_layout_preview = false;

        let screen = ctx.screen_rect().size();
        let max_w = if screen.x.is_finite() {
            (screen.x - 36.0).clamp(300.0, 860.0)
        } else {
            720.0
        };
        let max_h = if screen.y.is_finite() {
            (screen.y - 40.0).clamp(240.0, 660.0)
        } else {
            540.0
        };
        let def_w = 690.0_f32.min(max_w);
        let def_h = 500.0_f32.min(max_h);
        let extra_wrap = if def_w < 460.0 { 44.0 } else { 0.0 };
        let body_h = (def_h - 108.0 - extra_wrap).max(110.0);
        let nav_inner_h = (body_h - 10.0).max(90.0);

        self.dlg
            .prefs_orig_metrics
            .get_or_insert((self.dock_height, self.preview_ratio));
        let rec_workers = recommended_worker_threads();
        let active_sr = self.project.settings.sample_rate;
        let audio_device = self.audio.device_name.clone();
        let audio_sr = self.audio.sr;
        let audio_error = self.audio.error.clone();
        let render_favs_count = self.prefs.render_favorites.len();
        let pref_path_text = Preferences::file_path().display().to_string();
        let layout_dir_text = Preferences::layouts_dir().display().to_string();
        let theme = self.theme.clone();
        let sel_bg = theme.selection_bg;
        let sel_fg = crate::theme::contrast_on(sel_bg);
        let mut live_theme: Option<ThemeId> = None;
        let mut live_language: Option<Language> = None;

        let mut win = crate::dialog_window::DialogWindow::new(tr(lang, "Preferências..."))
            .id(egui::Id::new("prefs_win"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .constrain_to(ctx.screen_rect())
            .default_size([def_w, def_h])
            .max_size([max_w, max_h]);
        if screen.x.is_finite() && screen.y.is_finite() {
            win = win.default_pos([
                ((screen.x - def_w) * 0.5).max(8.0),
                ((screen.y - def_h) * 0.5).max(8.0),
            ]);
        }
        win.show(ctx, |ui| {
            let avail_w = ui.available_width().max(260.0);
            let side_by_side = avail_w >= 480.0;

            if side_by_side {
                let side_w = (avail_w * 0.25).clamp(140.0, 185.0);
                let right_w = (avail_w - side_w - 14.0).max(260.0);
                ui.horizontal_top(|ui| {
                    // Painel esquerdo: lista de categorias em moldura escura/contrastada
                    egui::Frame::NONE
                        .fill(theme.extreme_bg)
                        .stroke(Stroke::new(1.0_f32, theme.border))
                        .corner_radius(3.0)
                        .inner_margin(egui::Margin::same(4))
                        .show(ui, |ui| {
                            ui.set_width(side_w);
                            ui.set_min_height(nav_inner_h);
                            egui::ScrollArea::vertical()
                                .id_salt("prefs_nav_scroll")
                                .max_height(nav_inner_h)
                                .show(ui, |ui| {
                                    ui.with_layout(
                                        egui::Layout::top_down_justified(egui::Align::LEFT),
                                        |ui| {
                                            ui.spacing_mut().item_spacing.y = 1.0;
                                            let visuals = ui.visuals_mut();
                                            visuals.selection.bg_fill = sel_bg;
                                            visuals.selection.stroke = Stroke::NONE;
                                            visuals.widgets.hovered.bg_fill = theme.widget_hover_bg;
                                            visuals.widgets.hovered.weak_bg_fill =
                                                theme.widget_hover_bg;
                                            for cat in PrefsCategory::ALL {
                                                let selected = self.dlg.prefs_tab == cat;
                                                let fg = if selected {
                                                    sel_fg
                                                } else {
                                                    theme.text_primary
                                                };
                                                let resp = ui.selectable_label(
                                                    selected,
                                                    egui::RichText::new(cat.label_in(lang))
                                                        .color(fg),
                                                );
                                                if resp.clicked() {
                                                    self.dlg.prefs_tab = cat;
                                                }
                                            }
                                        },
                                    );
                                });
                        });

                    ui.add_space(6.0);

                    // Painel direito: opções da categoria selecionada
                    let inner_right_w = (right_w - 14.0).max(200.0);
                    ui.vertical(|ui| {
                        ui.set_width(right_w);
                        ui.set_max_width(right_w);
                        egui::ScrollArea::vertical()
                            .id_salt(("prefs_body_scroll", self.dlg.prefs_tab as u8))
                            .max_height(body_h)
                            .show(ui, |ui| {
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                                ui.set_min_width(inner_right_w);
                                ui.set_max_width(inner_right_w);
                                let ctx_refs = PrefsFormContext {
                                    lang,
                                    theme: &theme,
                                    tab: self.dlg.prefs_tab,
                                    rec_workers,
                                    active_sr,
                                    audio_device: &audio_device,
                                    audio_sr,
                                    audio_error: audio_error.as_deref(),
                                    render_favs_count,
                                    pref_path: &pref_path_text,
                                    layout_dir: &layout_dir_text,
                                    label_w: (right_w * 0.42).clamp(150.0, 225.0),
                                };
                                render_prefs_category(
                                    ui,
                                    &ctx_refs,
                                    &mut draft,
                                    &mut PrefsActions {
                                        live_theme: &mut live_theme,
                                        live_language: &mut live_language,
                                        live_layout_preview: &mut live_layout_preview,
                                        clear_cache_now: &mut clear_cache_now,
                                        clear_recents_now: &mut clear_recents_now,
                                        open_shortcuts: &mut open_shortcuts,
                                        apply_defaults_to_session: &mut apply_defaults_to_session,
                                    },
                                );
                            });
                    });
                });
            } else {
                egui::ScrollArea::horizontal()
                    .id_salt("prefs_nav_compact")
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for cat in PrefsCategory::ALL {
                                let selected = self.dlg.prefs_tab == cat;
                                if ui.selectable_label(selected, cat.label_in(lang)).clicked() {
                                    self.dlg.prefs_tab = cat;
                                }
                            }
                        });
                    });
                ui.separator();
                let compact_h = (body_h - 34.0).max(80.0);
                egui::ScrollArea::vertical()
                    .id_salt(("prefs_body_scroll_compact", self.dlg.prefs_tab as u8))
                    .max_height(compact_h)
                    .show(ui, |ui| {
                        let ctx_refs = PrefsFormContext {
                            lang,
                            theme: &theme,
                            tab: self.dlg.prefs_tab,
                            rec_workers,
                            active_sr,
                            audio_device: &audio_device,
                            audio_sr,
                            audio_error: audio_error.as_deref(),
                            render_favs_count,
                            pref_path: &pref_path_text,
                            layout_dir: &layout_dir_text,
                            label_w: 0.0,
                        };
                        render_prefs_category(
                            ui,
                            &ctx_refs,
                            &mut draft,
                            &mut PrefsActions {
                                live_theme: &mut live_theme,
                                live_language: &mut live_language,
                                live_layout_preview: &mut live_layout_preview,
                                clear_cache_now: &mut clear_cache_now,
                                clear_recents_now: &mut clear_recents_now,
                                open_shortcuts: &mut open_shortcuts,
                                apply_defaults_to_session: &mut apply_defaults_to_session,
                            },
                        );
                    });
            }

            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(tr(lang, "Restaurar padrões"))
                    .on_hover_text(tr(
                        lang,
                        "Repõe as preferências padrão (preserva projetos recentes e favoritos)",
                    ))
                    .clicked()
                {
                    reset_defaults = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr(lang, "Cancelar")).clicked() {
                        cancel_close = true;
                    }
                    if ui.button(tr(lang, "Aplicar")).clicked() {
                        apply_and_keep = true;
                    }
                    if ui.button(tr(lang, "OK")).clicked() {
                        save_and_close = true;
                    }
                });
            });
        });

        if let Some(tid) = live_theme {
            self.set_theme(ctx, tid, false);
        }
        if let Some(new_lang) = live_language {
            self.apply_language(Some(ctx), new_lang, false);
        }
        if live_layout_preview {
            self.apply_edit_layout(Some(ctx), draft.edit_layout.clone(), true, false);
        }

        if reset_defaults {
            let keep_recents = draft.recent_projects.clone();
            let keep_favs = draft.render_favorites.clone();
            draft = Preferences {
                recent_projects: keep_recents,
                render_favorites: keep_favs,
                ..Default::default()
            };
            let tid = ThemeId::from_pref(&draft.theme);
            self.set_theme(ctx, tid, false);
            let def_lang = draft.lang();
            self.apply_language(Some(ctx), def_lang, false);
            self.apply_edit_layout(Some(ctx), draft.edit_layout.clone(), true, false);
            self.set_status(tr(
                def_lang,
                "Rascunho restaurado para as preferências padrão.",
            ));
        }

        if clear_cache_now {
            self.media.clear_frame_caches();
            self.thumbs.clear();
            self.project_media.clear_thumbs();
            self.set_status(tr(self.lang(), "Cache de quadros e miniaturas liberado."));
        }

        if clear_recents_now {
            let cur_lang = self.lang();
            draft.clear_recent_projects();
            self.prefs.clear_recent_projects();
            if let Err(e) = self.prefs.save() {
                let err_s = e.to_string();
                self.set_status(tr_args(
                    cur_lang,
                    "Aviso ao limpar projetos recentes: {err}",
                    &[("err", &err_s)],
                ));
            } else {
                self.set_status(tr(cur_lang, "Histórico de projetos recentes limpo."));
            }
        }

        if open_shortcuts {
            self.dlg.shortcuts = true;
        }

        if cancel_close || (!open && !save_and_close) {
            if let Some((orig_theme, orig_layout, orig_lang)) = self.dlg.prefs_orig.take() {
                let tid = ThemeId::from_pref(&orig_theme);
                self.set_theme(ctx, tid, false);
                let olang = Language::from_pref(&orig_lang);
                self.apply_language(Some(ctx), olang, false);
                self.apply_edit_layout(Some(ctx), orig_layout, false, false);
            }
            if let Some((orig_dock_h, orig_prev_r)) = self.dlg.prefs_orig_metrics.take() {
                self.dock_height = orig_dock_h;
                self.preview_ratio = orig_prev_r;
            }
            self.dlg.prefs_open = false;
            self.dlg.prefs_draft = None;
            return;
        }

        if apply_defaults_to_session {
            apply_pref_defaults(&mut self.session.opts, &draft);
            self.set_status(tr(self.lang(), "Opções padrão aplicadas à sessão aberta."));
        }

        if apply_and_keep || save_and_close {
            draft.sanitize();
            self.apply_saved_prefs(ctx, &draft);
            let cur_lang = self.lang();
            if save_and_close {
                self.dlg.prefs_orig = None;
                self.dlg.prefs_orig_metrics = None;
                self.dlg.prefs_open = false;
                self.dlg.prefs_draft = None;
                self.set_status(tr(cur_lang, "Preferências salvas."));
            } else {
                self.dlg.prefs_orig = Some((
                    draft.theme.clone(),
                    draft.edit_layout.clone(),
                    draft.language.clone(),
                ));
                self.dlg.prefs_orig_metrics = Some((self.dock_height, self.preview_ratio));
                self.dlg.prefs_draft = Some(draft);
                self.set_status(tr(cur_lang, "Preferências aplicadas."));
            }
        } else {
            self.dlg.prefs_draft = Some(draft);
        }
    }

    fn apply_saved_prefs(&mut self, ctx: &egui::Context, draft: &Preferences) {
        let prev = self.prefs.clone();
        self.media
            .configure_limits(draft.frame_cache_cap, draft.ram_preview_mb);

        let opts = &mut self.session.opts;
        if draft.default_snapping != prev.default_snapping {
            opts.snapping = draft.default_snapping;
        }
        if draft.default_quantize != prev.default_quantize {
            opts.quantize = draft.default_quantize;
        }
        if draft.default_show_waveforms != prev.default_show_waveforms {
            opts.show_waveforms = draft.default_show_waveforms;
        }
        if draft.default_show_thumbs != prev.default_show_thumbs {
            opts.show_thumbs = draft.default_show_thumbs;
        }
        if draft.default_show_envelopes != prev.default_show_envelopes {
            opts.show_envelopes = draft.default_show_envelopes;
        }
        if draft.default_loop_playback != prev.default_loop_playback {
            opts.loop_playback = draft.default_loop_playback;
        }
        if draft.default_preview_quality != prev.default_preview_quality {
            opts.preview_quality = draft.default_preview_quality;
        }
        if draft.default_time_format != prev.default_time_format {
            opts.time_format = draft.default_time_format;
        }

        let next_theme = ThemeId::from_pref(&draft.theme);
        if draft.edit_layout != prev.edit_layout {
            let layout_type_changed = draft.edit_layout.layout_type != prev.edit_layout.layout_type;
            self.apply_edit_layout(
                Some(ctx),
                draft.edit_layout.clone(),
                layout_type_changed,
                false,
            );
        }
        if next_theme != self.theme_id {
            self.set_theme(ctx, next_theme, false);
        }

        let next_lang = draft.lang();
        self.apply_language(Some(ctx), next_lang, false);
        if draft.system_ui_font != prev.system_ui_font {
            crate::fonts::configure_fonts(ctx, draft.system_ui_font && !cfg!(test));
        }

        let mut next = draft.clone();
        next.recent_projects = prev.recent_projects;
        next.render_favorites = prev.render_favorites;
        next.sanitize();
        self.prefs = next;
        if let Err(e) = self.prefs.save() {
            let err_s = e.to_string();
            self.set_status(tr_args(
                next_lang,
                "Aviso ao salvar preferências: {err}",
                &[("err", &err_s)],
            ));
        }
    }

    fn edit_layout_window(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.dlg.edit_layout_draft.take() else {
            self.dlg.edit_layout_open = false;
            return;
        };
        let lang = self.lang();
        let mut open = true;
        let mut apply_type_live: Option<LayoutType> = None;
        let mut live_tweak = false;
        let mut restore_default = false;
        let mut save_and_close = false;
        let mut apply_and_keep = false;
        let mut cancel_close = false;

        let screen = ctx.screen_rect().size();
        let max_w = if screen.x.is_finite() {
            (screen.x - 24.0).clamp(260.0, 760.0)
        } else {
            580.0
        };
        let max_h = if screen.y.is_finite() {
            (screen.y - 36.0).clamp(220.0, 620.0)
        } else {
            460.0
        };
        let def_w = 560.0_f32.min(max_w);
        let def_h = 460.0_f32.min(max_h);
        let extra_wrap = if def_w < 440.0 { 26.0 } else { 0.0 };
        let scroll_max_h = (def_h - 96.0 - extra_wrap).max(90.0);

        crate::dialog_window::DialogWindow::new(tr(lang, "Editar > Propriedades > Layout"))
            .id(egui::Id::new("edit_layout_win"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .constrain_to(ctx.screen_rect())
            .default_size([def_w, def_h])
            .max_size([max_w, max_h])
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("edit_layout_scroll")
                    .max_height(scroll_max_h)
                    .show(ui, |ui| {
                        let avail_w = ui.available_width().max(220.0);
                        let wide = avail_w >= 460.0;
                        let mut render_types = |ui: &mut egui::Ui| {
                            ui.heading(tr(lang, "Tipos de layout"));
                            ui.add_space(4.0);
                            for lt in LayoutType::ALL {
                                let selected = draft.layout_type == lt;
                                if ui
                                    .radio(
                                        selected,
                                        egui::RichText::new(format!(
                                            "{}  ({})",
                                            lt.label_in(lang),
                                            lt.era_in(lang)
                                        ))
                                        .strong(),
                                    )
                                    .clicked()
                                    && !selected
                                {
                                    apply_type_live = Some(lt);
                                }
                            }
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(draft.layout_type.description_in(lang))
                                    .small()
                                    .weak(),
                            );
                        };
                        if wide {
                            let left_w = (avail_w * 0.52).clamp(210.0, 290.0);
                            let schem_w = (avail_w - left_w - 22.0).clamp(170.0, 270.0);
                            ui.horizontal_top(|ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(left_w);
                                    render_types(ui);
                                });
                                ui.separator();
                                ui.vertical(|ui| {
                                    ui.label(
                                        egui::RichText::new(tr(
                                            lang,
                                            "Pré-visualização da janela",
                                        ))
                                        .strong(),
                                    );
                                    ui.add_space(4.0);
                                    draw_layout_schematic(ui, lang, &draft, &self.theme, schem_w);
                                });
                            });
                        } else {
                            render_types(ui);
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(tr(lang, "Pré-visualização da janela"))
                                    .strong(),
                            );
                            ui.add_space(4.0);
                            draw_layout_schematic(ui, lang, &draft, &self.theme, avail_w);
                        }

                        ui.separator();
                        ui.heading(tr(lang, "Opções da área de trabalho"));
                        live_tweak |= edit_layout_checkboxes(ui, lang, &mut draft, None);
                        if self.theme_id.is_high_contrast() {
                            ui.label(
                                egui::RichText::new(tr(
                                    lang,
                                    "Tema Alto Contraste AAA ativo: a paleta de acessibilidade é preservada ao alternar tipos de layout.",
                                ))
                                .small()
                                .color(self.theme.accent),
                            );
                        }
                    });

                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    if ui.button(tr(lang, "OK")).clicked() {
                        save_and_close = true;
                    }
                    if ui.button(tr(lang, "Aplicar")).clicked() {
                        apply_and_keep = true;
                    }
                    if ui.button(tr(lang, "Cancelar")).clicked() {
                        cancel_close = true;
                    }
                    if ui
                        .button(tr(lang, "Restaurar padrão do tipo"))
                        .on_hover_text(tr(
                            lang,
                            "Repõe a disposição clássica padrão do tipo selecionado",
                        ))
                        .clicked()
                    {
                        restore_default = true;
                    }
                });
            });

        if cancel_close || (!open && !save_and_close) {
            if let Some((orig_el, orig_theme, orig_dock_h, orig_prev_r)) =
                self.dlg.edit_layout_orig.take()
            {
                self.prefs.edit_layout = orig_el;
                self.dock_height = orig_dock_h;
                self.preview_ratio = orig_prev_r;
                self.dock_tree = crate::dock::build_dock_tree_with(
                    self.preview_ratio,
                    self.dock_tab,
                    self.prefs.edit_layout.preview_on_left,
                    self.prefs.edit_layout.master_bus_placement,
                );
                let tid = ThemeId::from_pref(&orig_theme);
                self.set_theme(ctx, tid, false);
            }
            self.dlg.edit_layout_open = false;
            self.dlg.edit_layout_draft = None;
            return;
        }

        if let Some(lt) = apply_type_live {
            draft.apply_type(lt);
            self.apply_edit_layout(Some(ctx), draft.clone(), true, false);
        } else if restore_default {
            draft.apply_type(draft.layout_type);
            self.apply_edit_layout(Some(ctx), draft.clone(), true, false);
        } else if live_tweak {
            self.apply_edit_layout(Some(ctx), draft.clone(), false, false);
        }

        if apply_and_keep || save_and_close {
            self.apply_edit_layout(Some(ctx), draft.clone(), false, true);
            if save_and_close {
                self.dlg.edit_layout_orig = None;
                self.dlg.edit_layout_open = false;
                self.dlg.edit_layout_draft = None;
                let lt_name = draft.layout_type.label_in(lang);
                let lt_era = draft.layout_type.era_in(lang);
                self.set_status(tr_args(
                    lang,
                    "Layout {name} ({era}) salvo.",
                    &[("name", &lt_name), ("era", &lt_era)],
                ));
            } else {
                self.dlg.edit_layout_orig = Some((
                    draft.clone(),
                    self.prefs.theme.clone(),
                    self.dock_height,
                    self.preview_ratio,
                ));
                self.dlg.edit_layout_draft = Some(draft);
                self.set_status(tr(lang, "Propriedades de layout aplicadas."));
            }
        } else {
            self.dlg.edit_layout_draft = Some(draft);
        }
    }

    fn save_layout_window(&mut self, ctx: &egui::Context) {
        let lang = self.lang();
        let mut open = true;
        let mut commit = false;
        crate::dialog_window::DialogWindow::new(tr(
            lang,
            "Salvar layout de janela (.neoveglayout)",
        ))
        .id(egui::Id::new("save_layout_win"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(tr(lang, "Nome do layout:"));
            ui.text_edit_singleline(&mut self.dlg.save_layout_name);
            ui.horizontal(|ui| {
                if ui.button(tr(lang, "Salvar")).clicked() {
                    commit = true;
                }
                if ui.button(tr(lang, "Cancelar")).clicked() {
                    self.dlg.save_layout_open = false;
                }
            });
        });
        if commit {
            let name = self.dlg.save_layout_name.clone();
            self.save_named_layout(&name);
            open = false;
        }
        self.dlg.save_layout_open = open;
    }
}

fn capture_shortcut(ctx: &egui::Context) -> Option<Option<Shortcut>> {
    let mods = ctx.input(|i| i.modifiers);
    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        return Some(None);
    }
    const KEYS: &[(Key, ShortcutKey)] = &[
        (Key::A, ShortcutKey::A),
        (Key::B, ShortcutKey::B),
        (Key::C, ShortcutKey::C),
        (Key::D, ShortcutKey::D),
        (Key::E, ShortcutKey::E),
        (Key::F, ShortcutKey::F),
        (Key::G, ShortcutKey::G),
        (Key::H, ShortcutKey::H),
        (Key::I, ShortcutKey::I),
        (Key::J, ShortcutKey::J),
        (Key::K, ShortcutKey::K),
        (Key::L, ShortcutKey::L),
        (Key::M, ShortcutKey::M),
        (Key::N, ShortcutKey::N),
        (Key::O, ShortcutKey::O),
        (Key::P, ShortcutKey::P),
        (Key::Q, ShortcutKey::Q),
        (Key::R, ShortcutKey::R),
        (Key::S, ShortcutKey::S),
        (Key::T, ShortcutKey::T),
        (Key::U, ShortcutKey::U),
        (Key::V, ShortcutKey::V),
        (Key::W, ShortcutKey::W),
        (Key::X, ShortcutKey::X),
        (Key::Y, ShortcutKey::Y),
        (Key::Z, ShortcutKey::Z),
        (Key::Num0, ShortcutKey::Num0),
        (Key::Num1, ShortcutKey::Num1),
        (Key::Num2, ShortcutKey::Num2),
        (Key::Num3, ShortcutKey::Num3),
        (Key::Num4, ShortcutKey::Num4),
        (Key::Num5, ShortcutKey::Num5),
        (Key::Num6, ShortcutKey::Num6),
        (Key::Num7, ShortcutKey::Num7),
        (Key::Num8, ShortcutKey::Num8),
        (Key::Num9, ShortcutKey::Num9),
        (Key::Space, ShortcutKey::Space),
        (Key::Enter, ShortcutKey::Enter),
        (Key::Delete, ShortcutKey::Delete),
        (Key::F2, ShortcutKey::F2),
        (Key::F8, ShortcutKey::F8),
        (Key::F11, ShortcutKey::F11),
        (Key::Backtick, ShortcutKey::Backtick),
        (Key::ArrowLeft, ShortcutKey::ArrowLeft),
        (Key::ArrowRight, ShortcutKey::ArrowRight),
        (Key::ArrowUp, ShortcutKey::ArrowUp),
        (Key::ArrowDown, ShortcutKey::ArrowDown),
        (Key::Home, ShortcutKey::Home),
        (Key::End, ShortcutKey::End),
        (Key::OpenBracket, ShortcutKey::OpenBracket),
        (Key::CloseBracket, ShortcutKey::CloseBracket),
        (Key::Backslash, ShortcutKey::Backslash),
        (Key::Equals, ShortcutKey::Equals),
        (Key::Minus, ShortcutKey::Minus),
    ];
    for &(ek, sk) in KEYS {
        if ctx.input(|i| i.key_pressed(ek)) {
            return Some(Some(Shortcut {
                ctrl: mods.command,
                shift: mods.shift,
                alt: mods.alt,
                key: sk,
            }));
        }
    }
    None
}

const MOUSE_HINTS: &[(&str, &str)] = &[
    (
        "Roda / Shift+roda / Ctrl+roda",
        "Zoom / rolar horizontal / rolar vertical",
    ),
    ("Shift+arrastar", "Ignorar encaixe temporariamente"),
    ("Ctrl+arrastar evento", "Copiar evento"),
    (
        "Alt+arrastar evento",
        "Slip (mover a mídia dentro do evento)",
    ),
    ("Alt+arrastar borda", "Slip-trim"),
    (
        "Arrastar linha superior (áudio)",
        "Ajustar ganho do evento em dB",
    ),
    ("Duplo clique no envelope", "Adicionar ponto"),
    ("Botão do meio", "Mover a vista (pan)"),
];

fn draw_layout_schematic(
    ui: &mut egui::Ui,
    lang: Language,
    cfg: &EditLayoutPrefs,
    theme: &crate::theme::ThemeColors,
    max_width: f32,
) {
    let w = max_width.clamp(170.0, 270.0);
    let h = (w * 0.54).clamp(112.0, 146.0);
    let size = vec2(w, h);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, theme.extreme_bg);
    p.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, theme.border),
        StrokeKind::Outside,
    );

    let inner = rect.shrink(5.0);
    let mut top_y = inner.top();
    let mut bot_y = inner.bottom();
    let bar_h = (h * 0.10).clamp(11.0, 15.0);
    let tr_h = (h * 0.11).clamp(12.0, 16.0);
    let font_sz = (w / 27.0).clamp(8.0, 10.0);

    let box_labeled = |r: Rect, fill: Color32, stroke: Color32, text: &str| {
        p.rect_filled(r, 2.0, fill);
        p.rect_stroke(r, 2.0, Stroke::new(1.0_f32, stroke), StrokeKind::Inside);
        p.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(font_sz),
            crate::theme::contrast_on(fill),
        );
    };

    // Menu + Toolbar no topo
    let menu_r = Rect::from_min_max(
        pos2(inner.left(), top_y),
        pos2(inner.right(), top_y + bar_h),
    );
    top_y += bar_h + 2.5;
    box_labeled(
        menu_r,
        theme.tl_header_bg,
        theme.border,
        tr(lang, "Menu / Toolbar"),
    );

    // Status no rodapé
    let status_r = Rect::from_min_max(
        pos2(inner.left(), bot_y - (bar_h - 1.5)),
        pos2(inner.right(), bot_y),
    );
    bot_y -= bar_h + 1.0;
    box_labeled(
        status_r,
        theme.tl_header_bg,
        theme.border,
        tr(lang, "Barra de estado"),
    );

    let transport_fill = theme.widget_bg;
    if cfg.transport_at_bottom {
        let tr_r = Rect::from_min_max(pos2(inner.left(), bot_y - tr_h), pos2(inner.right(), bot_y));
        bot_y -= tr_h + 2.5;
        box_labeled(
            tr_r,
            transport_fill,
            theme.accent,
            tr(lang, "Barra de transporte"),
        );
    }

    let dock_h = ((bot_y - top_y) * 0.44).clamp(24.0, 56.0);
    let (dock_r, tl_r, tr_r_opt) = if cfg.timeline_at_bottom {
        let dr = Rect::from_min_max(
            pos2(inner.left(), top_y),
            pos2(inner.right(), top_y + dock_h),
        );
        let mut tl_top = dr.bottom() + 2.5;
        let tr_mid = if !cfg.transport_at_bottom {
            let r = Rect::from_min_max(
                pos2(inner.left(), tl_top),
                pos2(inner.right(), tl_top + tr_h),
            );
            tl_top += tr_h + 2.5;
            Some(r)
        } else {
            None
        };
        let tl = Rect::from_min_max(pos2(inner.left(), tl_top), pos2(inner.right(), bot_y));
        (dr, tl, tr_mid)
    } else {
        let dr = Rect::from_min_max(
            pos2(inner.left(), bot_y - dock_h),
            pos2(inner.right(), bot_y),
        );
        let mut tl_bot = dr.top() - 2.5;
        let tr_mid = if !cfg.transport_at_bottom {
            let r = Rect::from_min_max(
                pos2(inner.left(), tl_bot - tr_h),
                pos2(inner.right(), tl_bot),
            );
            tl_bot -= tr_h + 2.5;
            Some(r)
        } else {
            None
        };
        let tl = Rect::from_min_max(pos2(inner.left(), top_y), pos2(inner.right(), tl_bot));
        (dr, tl, tr_mid)
    };

    if let Some(tr_r) = tr_r_opt {
        box_labeled(
            tr_r,
            transport_fill,
            theme.accent,
            tr(lang, "Barra de transporte"),
        );
    }

    // Timeline (track list + faixas)
    box_labeled(
        tl_r,
        theme.tl_lane_even,
        theme.border,
        tr(lang, "Timeline (Track View)"),
    );
    let split_x = tl_r.left() + tl_r.width() * 0.25;
    p.line_segment(
        [pos2(split_x, tl_r.top()), pos2(split_x, tl_r.bottom())],
        Stroke::new(1.0_f32, theme.border),
    );

    // Área acoplável (Painéis + Prévia + Master Bus opcional + barra de abas no topo/base)
    let master_w = if cfg.master_bus_docked {
        (dock_r.width() * 0.14).clamp(26.0, 38.0)
    } else {
        0.0
    };
    let main_dock_w =
        (dock_r.width() - if master_w > 0.0 { master_w + 2.0 } else { 0.0 }).max(40.0);
    let split = main_dock_w * if cfg.preview_on_left { 0.42 } else { 0.58 };
    let left_sub = Rect::from_min_max(
        dock_r.min,
        pos2(dock_r.left() + split - 2.0, dock_r.bottom()),
    );
    let right_sub = Rect::from_min_max(
        pos2(dock_r.left() + split + 1.0, dock_r.top()),
        pos2(dock_r.left() + main_dock_w, dock_r.bottom()),
    );
    let (tabs_sub, prev_sub) = if cfg.preview_on_left {
        (right_sub, left_sub)
    } else {
        (left_sub, right_sub)
    };
    box_labeled(
        tabs_sub,
        theme.panel_fill,
        theme.border,
        tr(lang, "Painéis"),
    );
    box_labeled(
        prev_sub,
        theme.tl_preview_bg,
        theme.accent,
        tr(lang, "Prévia"),
    );
    let strip_span = if master_w > 0.0 {
        let master_sub =
            Rect::from_min_max(pos2(dock_r.right() - master_w, dock_r.top()), dock_r.max);
        box_labeled(
            master_sub,
            theme.panel_fill,
            theme.border,
            tr(lang, "Master"),
        );
        dock_r
    } else {
        tabs_sub
    };
    let strip_r = if cfg.tabs_at_top {
        Rect::from_min_max(
            strip_span.min,
            pos2(strip_span.right(), strip_span.top() + 5.0),
        )
    } else {
        Rect::from_min_max(
            pos2(strip_span.left(), strip_span.bottom() - 5.0),
            strip_span.max,
        )
    };
    p.rect_filled(strip_r, 1.0, theme.accent);
}

fn edit_layout_checkboxes(
    ui: &mut egui::Ui,
    lang: Language,
    cfg: &mut EditLayoutPrefs,
    sync_theme: Option<&mut String>,
) -> bool {
    let mut changed = false;
    // a geração "Moderno" não tem timeline no topo: a opção fica travada
    let top_allowed = cfg.layout_type.allows_timeline_on_top();
    if !top_allowed && !cfg.timeline_at_bottom {
        cfg.timeline_at_bottom = true;
        changed = true;
    }
    changed |= ui
        .add_enabled(
            top_allowed,
            egui::Checkbox::new(
                &mut cfg.timeline_at_bottom,
                tr(
                    lang,
                    "Exibir timeline na parte inferior da janela principal (área acoplável no topo)",
                ),
            ),
        )
        .on_disabled_hover_text(tr(
            lang,
            "No layout Moderno a timeline fica sempre na parte inferior.",
        ))
        .changed();
    changed |= ui
        .checkbox(
            &mut cfg.tabs_at_top,
            tr(lang, "Posicionar abas no topo das janelas acopladas"),
        )
        .changed();
    changed |= ui
        .checkbox(
            &mut cfg.transport_at_bottom,
            tr(lang, "Fixar barra de transporte no rodapé da janela"),
        )
        .changed();
    changed |= ui
        .checkbox(
            &mut cfg.preview_on_left,
            tr(
                lang,
                "Posicionar Prévia de Vídeo à esquerda na área acoplável",
            ),
        )
        .changed();
    changed |= ui
        .checkbox(
            &mut cfg.master_bus_docked,
            tr(lang, "Exibir Master Bus acoplado na área de janelas"),
        )
        .changed();
    ui.add_enabled_ui(cfg.master_bus_docked, |ui| {
        ui.horizontal(|ui| {
            ui.label(tr(lang, "Posição do Master Bus:"));
            egui::ComboBox::from_id_salt("edit_layout_master_place")
                .selected_text(cfg.master_bus_placement.label_in(lang))
                .show_ui(ui, |ui| {
                    for m in neoveg_common::prefs::MasterBusPlacement::ALL {
                        if ui
                            .selectable_value(&mut cfg.master_bus_placement, m, m.label_in(lang))
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
        });
    });
    if ui
        .checkbox(
            &mut cfg.sync_theme_with_layout,
            tr(
                lang,
                "Aplicar tema visual da era ao selecionar tipo de layout",
            ),
        )
        .changed()
    {
        if cfg.sync_theme_with_layout {
            if let Some(theme_pref) = sync_theme {
                if !ThemeId::from_pref(theme_pref).is_high_contrast() {
                    *theme_pref = cfg.layout_type.default_theme_id().to_string();
                }
            }
        }
        changed = true;
    }
    changed
}

struct PrefsFormContext<'a> {
    lang: Language,
    theme: &'a crate::theme::ThemeColors,
    tab: PrefsCategory,
    rec_workers: usize,
    active_sr: u32,
    audio_device: &'a str,
    audio_sr: u32,
    audio_error: Option<&'a str>,
    render_favs_count: usize,
    pref_path: &'a str,
    layout_dir: &'a str,
    label_w: f32,
}

struct PrefsActions<'a> {
    live_theme: &'a mut Option<ThemeId>,
    live_language: &'a mut Option<Language>,
    live_layout_preview: &'a mut bool,
    clear_cache_now: &'a mut bool,
    clear_recents_now: &'a mut bool,
    open_shortcuts: &'a mut bool,
    apply_defaults_to_session: &'a mut bool,
}

fn inner_label_w(label_w: f32) -> f32 {
    if label_w > 1.0 {
        (label_w - 11.0).max(110.0)
    } else {
        0.0
    }
}

fn prefs_row(
    ui: &mut egui::Ui,
    label_w: f32,
    label: &str,
    add_control: impl FnOnce(&mut egui::Ui),
) {
    if label_w > 1.0 {
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                vec2(label_w, 20.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.add(egui::Label::new(label).wrap());
                },
            );
            add_control(ui);
        });
    } else {
        ui.vertical(|ui| {
            ui.add(egui::Label::new(label).wrap());
            ui.horizontal(|ui| {
                add_control(ui);
            });
        });
    }
}

fn prefs_ro_row(
    ui: &mut egui::Ui,
    theme: &crate::theme::ThemeColors,
    label_w: f32,
    label: &str,
    value: &str,
    monospace: bool,
) {
    prefs_row(ui, label_w, label, |ui| {
        let mut rt = egui::RichText::new(value).color(theme.text_muted);
        if monospace {
            rt = rt.monospace().size(11.5);
            ui.add(egui::Label::new(rt).truncate()).on_hover_text(value);
        } else {
            ui.add(egui::Label::new(rt).wrap());
        }
    });
}

fn prefs_indented(ui: &mut egui::Ui, label_w: f32, add_contents: impl FnOnce(&mut egui::Ui)) {
    if label_w > 1.0 {
        let indent = label_w + ui.spacing().item_spacing.x;
        let max_inner = (ui.available_width() - indent).max(120.0);
        ui.horizontal(|ui| {
            ui.add_space(indent);
            ui.vertical(|ui| {
                ui.set_max_width(max_inner);
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                add_contents(ui);
            });
        });
    } else {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        add_contents(ui);
    }
}

fn prefs_group_box(
    ui: &mut egui::Ui,
    theme: &crate::theme::ThemeColors,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    ui.add_space(4.0);
    egui::Frame::NONE
        .fill(theme.panel_fill)
        .stroke(Stroke::new(1.0_f32, theme.border))
        .corner_radius(2.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                egui::RichText::new(title)
                    .strong()
                    .color(theme.text_primary),
            );
            ui.add_space(4.0);
            add_contents(ui);
        });
    ui.add_space(3.0);
}

fn render_prefs_category(
    ui: &mut egui::Ui,
    cx: &PrefsFormContext<'_>,
    draft: &mut Preferences,
    act: &mut PrefsActions<'_>,
) {
    ui.spacing_mut().item_spacing.y = 4.0;
    match cx.tab {
        PrefsCategory::General => prefs_general(ui, cx, draft, act),
        PrefsCategory::Appearance => prefs_appearance(ui, cx, draft, act),
        PrefsCategory::Editing => prefs_editing(ui, cx, draft, act),
        PrefsCategory::VideoPreview => prefs_video_preview(ui, cx, draft, act),
        PrefsCategory::PreviewDevice => prefs_preview_device(ui, cx, draft),
        PrefsCategory::Audio => prefs_audio(ui, cx, draft, act),
        PrefsCategory::AudioDevice => prefs_audio_device(ui, cx, draft),
        PrefsCategory::MidiSyncControl => prefs_midi_sync(ui, cx, draft),
        PrefsCategory::FileIo => prefs_file_io(ui, cx, draft),
        PrefsCategory::Memory => prefs_memory(ui, cx, draft, act),
        PrefsCategory::AutoSave => prefs_autosave(ui, cx, draft),
        PrefsCategory::Shortcuts => prefs_shortcuts(ui, cx, draft, act),
        PrefsCategory::Integrations => prefs_integrations(ui, cx, draft),
    }
}

fn prefs_general(
    ui: &mut egui::Ui,
    cx: &PrefsFormContext<'_>,
    draft: &mut Preferences,
    act: &mut PrefsActions<'_>,
) {
    let lang = cx.lang;
    prefs_row(
        ui,
        cx.label_w,
        tr(lang, "Duração padrão de imagem estática:"),
        |ui| {
            ui.add_sized(
                [74.0, 20.0],
                egui::DragValue::new(&mut draft.still_image_secs)
                    .range(0.5..=60.0)
                    .speed(0.1)
                    .max_decimals(2),
            );
            ui.label(tr(lang, "segundos"));
        },
    );

    prefs_row(
        ui,
        cx.label_w,
        tr(lang, "Rolagem automática na reprodução:"),
        |ui| {
            egui::ComboBox::from_id_salt("prefs_playback_scroll")
                .selected_text(draft.playback_scroll.label_in(lang))
                .show_ui(ui, |ui| {
                    for mode in PlaybackScrollMode::ALL {
                        ui.selectable_value(&mut draft.playback_scroll, mode, mode.label_in(lang));
                    }
                });
        },
    );

    prefs_row(
        ui,
        cx.label_w,
        tr(lang, "Roda do mouse na timeline:"),
        |ui| {
            egui::ComboBox::from_id_salt("prefs_mouse_scroll")
                .selected_text(draft.timeline_mouse_scroll.label_in(lang))
                .show_ui(ui, |ui| {
                    for mode in TimelineMouseScroll::ALL {
                        ui.selectable_value(
                            &mut draft.timeline_mouse_scroll,
                            mode,
                            mode.label_in(lang),
                        );
                    }
                });
        },
    );

    ui.add_space(2.0);
    prefs_indented(ui, cx.label_w, |ui| {
        ui.checkbox(
            &mut draft.show_splash,
            tr(lang, "Exibir tela de abertura (Splash) ao iniciar"),
        );
        ui.horizontal(|ui| {
            ui.label(tr(lang, "Escala da interface:"));
            ui.add(
                egui::DragValue::new(&mut draft.ui_scale_pct)
                    .range(neoveg_common::prefs::UI_SCALE_MIN..=neoveg_common::prefs::UI_SCALE_MAX)
                    .speed(1.0)
                    .suffix(" %"),
            )
            .on_hover_text(tr(
                lang,
                "Aumenta ou diminui textos, botões e painéis (também em Exibir > Escala da interface)",
            ));
            if ui.small_button("100%").clicked() {
                draft.ui_scale_pct = 100;
            }
        });
        ui.checkbox(
            &mut draft.system_ui_font,
            tr(
                lang,
                "Usar as fontes do sistema na interface (Segoe UI no Windows)",
            ),
        )
        .on_hover_text(tr(
            lang,
            "Desligado, a interface usa as fontes embutidas do programa",
        ));
        ui.checkbox(
            &mut draft.show_welcome,
            tr(
                lang,
                "Exibir tela de boas-vindas com projetos recentes ao iniciar",
            ),
        );
        ui.checkbox(
            &mut draft.always_draw_marker_lines,
            tr(lang, "Desenhar sempre as linhas de marcadores na timeline"),
        );
        ui.checkbox(
            &mut draft.show_tooltips,
            tr(lang, "Exibir dicas flutuantes (Tool Tips) nos controles"),
        );
        ui.checkbox(
            &mut draft.custom_cursors,
            tr(lang, "Usar ponteiros personalizados de edição"),
        );
    });

    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(
        ui,
        cx.theme,
        tr(lang, "Histórico e Projetos Recentes"),
        |ui| {
            prefs_row(
                ui,
                inner_lw,
                tr(lang, "Máximo de projetos recentes:"),
                |ui| {
                    ui.add_sized(
                        [64.0, 20.0],
                        egui::DragValue::new(&mut draft.max_recent_projects).range(1..=20),
                    );
                    ui.label(tr(lang, "itens  (1–20)"));
                },
            );
            prefs_indented(ui, inner_lw, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button(tr(lang, "Limpar projetos recentes")).clicked() {
                        *act.clear_recents_now = true;
                    }
                    let recents_n = draft.recent_projects.len();
                    ui.label(
                        egui::RichText::new(tr_args(
                            lang,
                            "{projects} projeto(s) no histórico • {favs} modelo(s) favorito(s)",
                            &[("projects", &recents_n), ("favs", &cx.render_favs_count)],
                        ))
                        .color(cx.theme.text_muted),
                    );
                });
            });
        },
    );

    prefs_group_box(
        ui,
        cx.theme,
        tr(lang, "Idioma e Arquivos de Configuração"),
        |ui| {
            let mut cur_lang = draft.lang();
            prefs_row(ui, inner_lw, tr(lang, "Idioma ativo:"), |ui| {
                egui::ComboBox::from_id_salt("prefs_language_combo")
                    .selected_text(cur_lang.display_option(lang))
                    .show_ui(ui, |ui| {
                        for l in Language::ALL {
                            if ui
                                .selectable_value(&mut cur_lang, l, l.display_option(lang))
                                .changed()
                            {
                                draft.set_lang(l);
                                *act.live_language = Some(l);
                            }
                        }
                    });
            });
            prefs_ro_row(
                ui,
                cx.theme,
                inner_lw,
                tr(lang, "Arquivo de preferências:"),
                cx.pref_path,
                true,
            );
            prefs_ro_row(
                ui,
                cx.theme,
                inner_lw,
                tr(lang, "Pasta de layouts (.neoveglayout):"),
                cx.layout_dir,
                true,
            );
        },
    );
}

fn prefs_appearance(
    ui: &mut egui::Ui,
    cx: &PrefsFormContext<'_>,
    draft: &mut Preferences,
    act: &mut PrefsActions<'_>,
) {
    let lang = cx.lang;
    ui.checkbox(
        &mut draft.allow_floating_dock,
        tr(lang, "Permitir que janelas flutuantes acoplem"),
    )
    .on_hover_text(tr(
        lang,
        "Ligado: arrastar uma janela destacada para a área de janelas a acopla (segure Ctrl para não acoplar). Desligado: só acopla segurando Ctrl.",
    ));
    let mut active_theme = ThemeId::from_pref(&draft.theme);
    prefs_row(ui, cx.label_w, tr(lang, "Tema visual:"), |ui| {
        egui::ComboBox::from_id_salt("prefs_theme_combo")
            .selected_text(active_theme.label_in(lang))
            .show_ui(ui, |ui| {
                for t in ThemeId::ALL {
                    if ui
                        .selectable_value(&mut active_theme, t, t.label_in(lang))
                        .changed()
                    {
                        draft.theme = t.id().to_string();
                        *act.live_theme = Some(t);
                    }
                }
            });
    });

    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(ui, cx.theme, tr(lang, "Fluxo de trabalho"), |ui| {
        for mode in neoveg_common::WorkflowMode::ALL {
            ui.radio_value(&mut draft.workflow_mode, mode, mode.label_in(lang));
            ui.indent(("prefs_workflow_desc", mode as u8), |ui| {
                ui.label(
                    egui::RichText::new(mode.description_in(lang))
                        .small()
                        .color(cx.theme.text_muted),
                );
            });
            ui.add_space(2.0);
        }
    });
    prefs_group_box(
        ui,
        cx.theme,
        tr(lang, "Disposição Clássica da Área de Trabalho (Layout)"),
        |ui| {
            let cur_lt = draft.edit_layout.layout_type;
            prefs_row(ui, inner_lw, tr(lang, "Tipo de layout:"), |ui| {
                egui::ComboBox::from_id_salt("prefs_layout_type_combo")
                    .selected_text(format!(
                        "{} ({})",
                        cur_lt.label_in(lang),
                        cur_lt.era_in(lang)
                    ))
                    .show_ui(ui, |ui| {
                        for lt in LayoutType::ALL {
                            if ui
                                .selectable_label(
                                    cur_lt == lt,
                                    format!("{} — {}", lt.label_in(lang), lt.era_in(lang)),
                                )
                                .clicked()
                            {
                                draft.edit_layout.apply_type(lt);
                                if draft.edit_layout.sync_theme_with_layout
                                    && !ThemeId::from_pref(&draft.theme).is_high_contrast()
                                {
                                    draft.theme = lt.default_theme_id().to_string();
                                }
                                *act.live_layout_preview = true;
                            }
                        }
                    });
            });

            prefs_indented(ui, inner_lw, |ui| {
                if edit_layout_checkboxes(ui, lang, &mut draft.edit_layout, Some(&mut draft.theme))
                {
                    *act.live_layout_preview = true;
                }
            });
        },
    );
    prefs_display_group(ui, cx, draft, inner_lw);
}

fn rgb_of(c: egui::Color32) -> [u8; 3] {
    [c.r(), c.g(), c.b()]
}

/// Botão de cor com "Padrão": `None` volta à cor `def`.
fn display_color_row(ui: &mut egui::Ui, lang: Language, slot: &mut Option<[u8; 3]>, def: [u8; 3]) {
    let mut c = slot.unwrap_or(def);
    if ui.color_edit_button_srgb(&mut c).changed() {
        *slot = Some(c);
    }
    if ui
        .add_enabled(
            slot.is_some(),
            egui::Button::new(tr(lang, "Padrão")).small(),
        )
        .clicked()
    {
        *slot = None;
    }
}

/// Opções da aba Exibição (manual do VEGAS Pro 21, pp. 1101–1103; ajuda do
/// VEGAS Pro 2026, *Display Tab*).
fn prefs_display_group(
    ui: &mut egui::Ui,
    cx: &PrefsFormContext<'_>,
    draft: &mut Preferences,
    inner_lw: f32,
) {
    let lang = cx.lang;
    let theme = cx.theme;
    prefs_group_box(ui, theme, tr(lang, "Cores e exibição"), |ui| {
        let d = &mut draft.display;
        let n_of = |i: usize| tr_args(lang, "Cor {n}", &[("n", &(i + 1).to_string())]);
        prefs_row(ui, inner_lw, tr(lang, "Cores das trilhas:"), |ui| {
            let id = egui::Id::new("prefs_display_track_idx");
            let mut idx: usize = ui.data_mut(|m| *m.get_temp_mut_or(id, 0usize));
            idx = idx.min(neoveg_common::prefs::TRACK_PALETTE_LEN - 1);
            egui::ComboBox::from_id_salt("prefs_display_track")
                .selected_text(n_of(idx))
                .show_ui(ui, |ui| {
                    for i in 0..neoveg_common::prefs::TRACK_PALETTE_LEN {
                        ui.selectable_value(&mut idx, i, n_of(i));
                    }
                });
            ui.data_mut(|m| m.insert_temp(id, idx));
            let def = crate::model::TRACK_COLORS
                .get(idx)
                .copied()
                .unwrap_or([128, 128, 128]);
            let mut slot = d.track_color(idx);
            display_color_row(ui, lang, &mut slot, def);
            d.set_track_color(idx, slot);
            ui.label("(?)").on_hover_text(tr(
                lang,
                "Toda trilha que usa esta cor da paleta passa a ser mostrada com a cor escolhida.",
            ));
        });
        prefs_row(ui, inner_lw, tr(lang, "Cores dos envelopes:"), |ui| {
            let id = egui::Id::new("prefs_display_env_idx");
            let mut idx: usize = ui.data_mut(|m| *m.get_temp_mut_or(id, 0usize));
            idx = idx.min(neoveg_common::prefs::DISPLAY_ENVELOPES.len() - 1);
            let name = |i: usize| {
                tr(
                    lang,
                    neoveg_common::prefs::DISPLAY_ENVELOPES
                        .get(i)
                        .copied()
                        .unwrap_or("Volume"),
                )
            };
            egui::ComboBox::from_id_salt("prefs_display_env")
                .selected_text(name(idx))
                .show_ui(ui, |ui| {
                    for i in 0..neoveg_common::prefs::DISPLAY_ENVELOPES.len() {
                        ui.selectable_value(&mut idx, i, name(i));
                    }
                });
            ui.data_mut(|m| m.insert_temp(id, idx));
            // Volume e Opacidade usam a cor principal; Pan e Fade, a secundária
            let def = if idx == 0 || idx == 2 {
                rgb_of(theme.tl_env_primary)
            } else {
                rgb_of(theme.tl_env_secondary)
            };
            if let Some(slot) = d.envelope_colors.get_mut(idx) {
                display_color_row(ui, lang, slot, def);
            }
        });
        prefs_row(
            ui,
            inner_lw,
            tr(lang, "Cor do indicador de encaixe:"),
            |ui| {
                display_color_row(ui, lang, &mut d.snap_color, rgb_of(theme.tl_playhead_edit));
            },
        );
        prefs_row(
            ui,
            inner_lw,
            tr(lang, "Intensidade da cor dos ícones:"),
            |ui| {
                ui.add(
                egui::Slider::new(
                    &mut d.icon_strength_pct,
                    0..=neoveg_common::prefs::DisplayPrefs::ICON_STRENGTH_MAX,
                )
                    .suffix(" %"),
            )
            .on_hover_text(tr(
                lang,
                "À esquerda os ícones ficam menos saturados; à direita, mais. Não muda os temas de Alto Contraste.",
            ));
            },
        );
        prefs_row(
            ui,
            inner_lw,
            tr(lang, "Cor de destaque dos botões:"),
            |ui| {
                display_color_row(
                    ui,
                    lang,
                    &mut d.button_highlight,
                    rgb_of(theme.selection_bg),
                );
                ui.label("(?)")
                    .on_hover_text(tr(lang, "Fundo dos botões ativos da barra de ferramentas."));
            },
        );
        prefs_indented(ui, inner_lw, |ui| {
            ui.checkbox(
                &mut d.auto_hide_dock,
                tr(lang, "Ocultar automaticamente a área de janelas"),
            )
            .on_hover_text(tr(
                lang,
                "A área de janelas some quando não está em uso; passe o ponteiro na faixa fina para mostrá-la.",
            ));
            ui.checkbox(
                &mut d.reverse_event_bg,
                tr(lang, "Inverter a cor de fundo do evento"),
            )
            .on_hover_text(tr(
                lang,
                "Troca a cor de frente e de fundo dos eventos: no áudio, a cor da trilha vai para o fundo e a forma de onda fica escura.",
            ));
            if ui
                .button(tr(lang, "Restaurar padrões da exibição"))
                .clicked()
            {
                *d = neoveg_common::prefs::DisplayPrefs::default();
            }
        });
    });
}

fn prefs_editing(
    ui: &mut egui::Ui,
    cx: &PrefsFormContext<'_>,
    draft: &mut Preferences,
    act: &mut PrefsActions<'_>,
) {
    let lang = cx.lang;
    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(
        ui,
        cx.theme,
        tr(lang, "Opções Padrão da Timeline (Novos Projetos)"),
        |ui| {
            prefs_row(ui, inner_lw, tr(lang, "Formato padrão de tempo:"), |ui| {
                egui::ComboBox::from_id_salt("prefs_default_time_format")
                    .selected_text(draft.default_time_format.name_in(lang))
                    .show_ui(ui, |ui| {
                        for tf in TimeFormat::ALL {
                            ui.selectable_value(
                                &mut draft.default_time_format,
                                tf,
                                tf.name_in(lang),
                            );
                        }
                    });
            });

            ui.add_space(2.0);
            prefs_indented(ui, inner_lw, |ui| {
                ui.checkbox(
                    &mut draft.default_snapping,
                    tr(lang, "Ativar encaixe magnético (Snapping) por padrão"),
                );
                ui.checkbox(
                    &mut draft.default_quantize,
                    tr(lang, "Quantizar edições nos limites exatos de quadros"),
                );
                ui.checkbox(
                    &mut draft.default_show_waveforms,
                    tr(
                        lang,
                        "Exibir formas de onda (waveforms) nos eventos de áudio",
                    ),
                );
                ui.checkbox(
                    &mut draft.default_show_thumbs,
                    tr(lang, "Exibir miniaturas nos eventos de vídeo"),
                );
                ui.checkbox(
                    &mut draft.default_show_envelopes,
                    tr(lang, "Exibir curvas de envelope nas faixas"),
                );
                ui.checkbox(
                    &mut draft.default_loop_playback,
                    tr(lang, "Ativar reprodução contínua em loop por padrão"),
                );
                ui.add_space(3.0);
                if ui
                    .button(tr(lang, "Aplicar padrões à sessão aberta"))
                    .on_hover_text(tr(
                        lang,
                        "Atualiza imediatamente a timeline atual com os padrões acima",
                    ))
                    .clicked()
                {
                    *act.apply_defaults_to_session = true;
                }
            });
        },
    );

    prefs_group_box(
        ui,
        cx.theme,
        tr(lang, "Comportamento de Edição e Mídia"),
        |ui| {
            ui.add(
            egui::Label::new(
                egui::RichText::new(tr(
                    lang,
                    "Ao importar o primeiro arquivo de mídia, o formato do projeto é ajustado automaticamente somente se as propriedades do projeto ainda não foram personalizadas.",
                ))
                .color(cx.theme.text_muted),
            )
            .wrap(),
        );
        },
    );
}

fn prefs_video_preview(
    ui: &mut egui::Ui,
    cx: &PrefsFormContext<'_>,
    draft: &mut Preferences,
    act: &mut PrefsActions<'_>,
) {
    let lang = cx.lang;
    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(
        ui,
        cx.theme,
        tr(lang, "Prévia de Vídeo Escalonável"),
        |ui| {
            prefs_row(
                ui,
                inner_lw,
                tr(lang, "Qualidade inicial da Prévia:"),
                |ui| {
                    egui::ComboBox::from_id_salt("prefs_default_preview_quality")
                        .selected_text(draft.default_preview_quality.name_in(lang))
                        .show_ui(ui, |ui| {
                            for pq in PreviewQuality::ALL {
                                ui.selectable_value(
                                    &mut draft.default_preview_quality,
                                    pq,
                                    pq.name_in(lang),
                                );
                            }
                        });
                },
            );
            prefs_indented(ui, inner_lw, |ui| {
                if ui
                    .button(tr(lang, "Aplicar qualidade à sessão aberta"))
                    .clicked()
                {
                    *act.apply_defaults_to_session = true;
                }
            });
        },
    );

    prefs_group_box(
        ui,
        cx.theme,
        tr(lang, "Guias de Área Segura na Prévia (Safe Areas)"),
        |ui| {
            prefs_row(
                ui,
                inner_lw,
                tr(lang, "Área segura de ação (Action Safe):"),
                |ui| {
                    if ui
                        .add_sized(
                            [64.0, 20.0],
                            egui::DragValue::new(&mut draft.action_safe_pct).range(70..=98),
                        )
                        .changed()
                        && draft.title_safe_pct > draft.action_safe_pct
                    {
                        draft.title_safe_pct = draft.action_safe_pct;
                    }
                    ui.label("%  (70–98%)");
                },
            );
            prefs_row(
                ui,
                inner_lw,
                tr(lang, "Área segura de título (Title Safe):"),
                |ui| {
                    let max_title = draft.action_safe_pct.max(60);
                    ui.add_sized(
                        [64.0, 20.0],
                        egui::DragValue::new(&mut draft.title_safe_pct).range(60..=max_title),
                    );
                    ui.label(format!("%  (60–{max_title}%)"));
                },
            );
            prefs_indented(ui, inner_lw, |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(tr(
                            lang,
                            "As guias são desenhadas na Prévia de Vídeo quando o botão \"Áreas seguras\" está ativado.",
                        ))
                        .color(cx.theme.text_muted),
                    )
                    .wrap(),
                );
            });
            prefs_row(ui, inner_lw, tr(lang, "Guias extras:"), |ui| {
                ui.vertical(|ui| {
                    let g = &mut draft.safe_guides;
                    ui.checkbox(&mut g.center_cross, tr(lang, "Cruz central"));
                    ui.checkbox(&mut g.thirds, tr(lang, "Grade dos terços"));
                    ui.checkbox(&mut g.cut_4x3, tr(lang, "Recorte central 4:3"));
                    ui.checkbox(&mut g.cut_9x16, tr(lang, "Recorte central 9:16 (vertical)"));
                    ui.checkbox(&mut g.cut_1x1, tr(lang, "Recorte central 1:1 (quadrado)"));
                    ui.checkbox(&mut g.action_title, tr(lang, "Áreas de ação e de título"));
                    ui.checkbox(&mut g.rulers, tr(lang, "Réguas (pixels do projeto)"));
                    ui.checkbox(&mut g.grid, tr(lang, "Grade"));
                    // quebra a linha em janela estreita (Preferências em 760x520)
                    ui.horizontal_wrapped(|ui| {
                        ui.label(tr(lang, "Divisões horizontais:"));
                        ui.add(egui::DragValue::new(&mut g.grid_h).range(2..=64));
                        ui.label(tr(lang, "Verticais:"));
                        ui.add(egui::DragValue::new(&mut g.grid_v).range(2..=64));
                    });
                });
            });
        },
    );
}

fn prefs_audio(
    ui: &mut egui::Ui,
    cx: &PrefsFormContext<'_>,
    draft: &mut Preferences,
    _act: &mut PrefsActions<'_>,
) {
    let lang = cx.lang;
    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(
        ui,
        cx.theme,
        tr(lang, "Configurações de Áudio de Novos Projetos"),
        |ui| {
            let sr_label = match draft.default_sample_rate {
                44100 => tr(lang, "44100 Hz (44,1 kHz — CD)"),
                96000 => tr(lang, "96000 Hz (96 kHz — Estúdio)"),
                _ => tr(lang, "48000 Hz (48 kHz — Vídeo)"),
            };
            prefs_row(ui, inner_lw, tr(lang, "Taxa de amostragem padrão:"), |ui| {
                egui::ComboBox::from_id_salt("prefs_default_sr")
                    .selected_text(sr_label)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut draft.default_sample_rate,
                            44100,
                            tr(lang, "44100 Hz (44,1 kHz — CD)"),
                        );
                        ui.selectable_value(
                            &mut draft.default_sample_rate,
                            48000,
                            tr(lang, "48000 Hz (48 kHz — Vídeo)"),
                        );
                        ui.selectable_value(
                            &mut draft.default_sample_rate,
                            96000,
                            tr(lang, "96000 Hz (96 kHz — Estúdio)"),
                        );
                    });
            });
            let active_sr_desc = tr_args(
                lang,
                "{sr} Hz  (Arquivo > Propriedades)",
                &[("sr", &cx.active_sr)],
            );
            prefs_ro_row(
                ui,
                cx.theme,
                inner_lw,
                tr(lang, "Taxa do projeto aberto:"),
                &active_sr_desc,
                false,
            );
        },
    );

    prefs_group_box(
        ui,
        cx.theme,
        tr(
            lang,
            "Dispositivo de Saída e Motor (cpal / fundsp / rubato)",
        ),
        |ui| {
            prefs_ro_row(
                ui,
                cx.theme,
                inner_lw,
                tr(lang, "Dispositivo ativo:"),
                cx.audio_device,
                false,
            );
            prefs_ro_row(
                ui,
                cx.theme,
                inner_lw,
                tr(lang, "Taxa do hardware de saída:"),
                &format!("{} Hz", cx.audio_sr),
                false,
            );
            if let Some(err) = cx.audio_error {
                prefs_ro_row(
                    ui,
                    cx.theme,
                    inner_lw,
                    tr(lang, "Estado da saída:"),
                    err,
                    false,
                );
            }
        },
    );
}

fn prefs_memory(
    ui: &mut egui::Ui,
    cx: &PrefsFormContext<'_>,
    draft: &mut Preferences,
    act: &mut PrefsActions<'_>,
) {
    let lang = cx.lang;
    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(
        ui,
        cx.theme,
        tr(lang, "Limites de RAM, Cache e Threads"),
        |ui| {
            prefs_row(
                ui,
                inner_lw,
                tr(lang, "Prévia dinâmica em RAM máx.:"),
                |ui| {
                    ui.add_sized(
                        [72.0, 20.0],
                        egui::DragValue::new(&mut draft.ram_preview_mb)
                            .range(32..=8192)
                            .speed(16),
                    );
                    ui.label("MB  (32–8192 MB)");
                },
            );

            prefs_row(
                ui,
                inner_lw,
                tr(lang, "Máximo de quadros em cache:"),
                |ui| {
                    ui.add_sized(
                        [72.0, 20.0],
                        egui::DragValue::new(&mut draft.frame_cache_cap).range(4..=512),
                    );
                    ui.label(tr(lang, "quadros  (4–512)"));
                },
            );

            prefs_row(ui, inner_lw, tr(lang, "Threads de trabalho:"), |ui| {
                ui.add_sized(
                    [64.0, 20.0],
                    egui::DragValue::new(&mut draft.worker_threads).range(0..=32),
                );
                let desc = if draft.worker_threads == 0 {
                    tr_args(
                        lang,
                        "automático ({n} rec. nesta CPU)",
                        &[("n", &cx.rec_workers)],
                    )
                } else {
                    tr(lang, "manuais (0 = auto)").to_string()
                };
                ui.add(egui::Label::new(desc).wrap());
            });

            prefs_indented(ui, inner_lw, |ui| {
                if ui
                    .button(tr(lang, "Limpar cache e miniaturas agora"))
                    .clicked()
                {
                    *act.clear_cache_now = true;
                }
            });
        },
    );

    prefs_group_box(
        ui,
        cx.theme,
        tr(lang, "Adaptabilidade a Hardware Limitado (§3.6)"),
        |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(tr(
                        lang,
                        "Em máquinas com 2 núcleos ou 4 GB de RAM, manter threads em 0 e Prévia RAM moderada evita pressão de memória; mídia é lida em streaming.",
                    ))
                    .color(cx.theme.text_muted),
                )
                .wrap(),
            );
        },
    );
}

fn prefs_autosave(ui: &mut egui::Ui, cx: &PrefsFormContext<'_>, draft: &mut Preferences) {
    let lang = cx.lang;
    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(
        ui,
        cx.theme,
        tr(lang, "Salvamento Automático e Recuperação"),
        |ui| {
            prefs_indented(ui, inner_lw, |ui| {
                ui.checkbox(
                    &mut draft.autosave_enabled,
                    tr(lang, "Ativar salvamento automático periódico do projeto"),
                );
            });

            ui.add_enabled_ui(draft.autosave_enabled, |ui| {
                prefs_row(ui, inner_lw, tr(lang, "Intervalo de gravação:"), |ui| {
                    ui.add_sized(
                        [72.0, 20.0],
                        egui::DragValue::new(&mut draft.autosave_secs)
                            .range(15..=600)
                            .speed(5),
                    );
                    ui.label(tr(lang, "segundos  (15–600 s)"));
                });
                prefs_row(
                    ui,
                    inner_lw,
                    tr(lang, "Histórico de recuperação:"),
                    |ui| {
                        ui.add_sized(
                            [72.0, 20.0],
                            egui::DragValue::new(&mut draft.autosave_history_copies)
                                .range(1..=10)
                                .speed(1),
                        );
                        ui.label(tr(lang, "versões mantidas"));
                    },
                );
            });

            let summary = if draft.autosave_enabled {
                let mins = draft.autosave_secs / 60;
                let secs = draft.autosave_secs % 60;
                if mins > 0 && secs > 0 {
                    tr_args(
                        lang,
                        "ativo — a cada {mins} min {secs} s",
                        &[("mins", &mins), ("secs", &secs)],
                    )
                } else if mins > 0 {
                    tr_args(lang, "ativo — a cada {mins} min", &[("mins", &mins)])
                } else {
                    tr_args(lang, "ativo — a cada {secs} s", &[("secs", &secs)])
                }
            } else {
                tr(lang, "desativado").to_string()
            };
            prefs_ro_row(
                ui,
                cx.theme,
                inner_lw,
                tr(lang, "Estado atual:"),
                &summary,
                false,
            );
            prefs_ro_row(
                ui,
                cx.theme,
                inner_lw,
                tr(lang, "Arquivo de configuração:"),
                cx.pref_path,
                true,
            );
        },
    );

    prefs_group_box(
        ui,
        cx.theme,
        tr(lang, "Cópia de Segurança do Projeto (.bak)"),
        |ui| {
            prefs_indented(ui, inner_lw, |ui| {
                ui.checkbox(
                    &mut draft.project_backup_enabled,
                    tr(
                        lang,
                        "Criar cópia de segurança (.bak) ao salvar manualmente o projeto",
                    ),
                );
            });

            ui.add_enabled_ui(draft.project_backup_enabled, |ui| {
                prefs_row(
                    ui,
                    inner_lw,
                    tr(lang, "Cópias históricas mantidas:"),
                    |ui| {
                        ui.add_sized(
                            [72.0, 20.0],
                            egui::DragValue::new(&mut draft.project_backup_copies)
                                .range(1..=10)
                                .speed(1),
                        );
                        ui.label(tr(lang, "cópias (.bak, .bak1, .bak2...)"));
                    },
                );
            });
        },
    );
}

fn prefs_shortcuts(
    ui: &mut egui::Ui,
    cx: &PrefsFormContext<'_>,
    draft: &Preferences,
    act: &mut PrefsActions<'_>,
) {
    let lang = cx.lang;
    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(
        ui,
        cx.theme,
        tr(lang, "Atalhos de Teclado e Comandos"),
        |ui| {
            let cmds_n = COMMANDS.len();
            let custom_n = draft.shortcuts.len();
            let stats = tr_args(
                lang,
                "{cmds} comando(s) na Tabela Única • {custom} atalho(s) personalizado(s)",
                &[("cmds", &cmds_n), ("custom", &custom_n)],
            );
            prefs_ro_row(
                ui,
                cx.theme,
                inner_lw,
                tr(lang, "Resumo de atalhos:"),
                &stats,
                false,
            );
            prefs_indented(ui, inner_lw, |ui| {
                if ui
                    .button(tr(lang, "Configurar atalhos de teclado..."))
                    .clicked()
                {
                    *act.open_shortcuts = true;
                }
            });
        },
    );

    prefs_group_box(
        ui,
        cx.theme,
        tr(lang, "Mapeamento Clássico do VEGAS Pro Preservado"),
        |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(tr(
                        lang,
                        "S = Dividir • M = Marcador • R = Região • J/K/L = Scrub • G/U = Agrupar/Desagrupar • Z/X = Mudo/Solo • Ctrl+L = Auto Ripple • Alt+[ / Alt+] = Aparo • Ctrl+T = Aparar à seleção",
                    ))
                    .color(cx.theme.text_muted),
                )
                .wrap(),
            );
        },
    );
}

fn prefs_integrations(ui: &mut egui::Ui, cx: &PrefsFormContext<'_>, draft: &mut Preferences) {
    use neoveg_engine::integrations::{discovery::discover_app, ExternalApp};

    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(
        ui,
        cx.theme,
        "Suíte ArtCraft & Blender (Executáveis Manuais / Autodetecção)",
        |ui| {
            ui.label(
                egui::RichText::new(
                    "Deixe vazio para detectar no PATH ou pastas padrão do sistema; defina manualmente se estiver em pasta portátil.",
                )
                .small()
                .color(cx.theme.text_muted),
            );
            for app in ExternalApp::ALL {
                let id = app.id();
                let st = discover_app(app, &draft.external_apps, false);
                let hint = if st.available() {
                    st.desktop_exe
                        .as_ref()
                        .or(st.cli_exe.as_ref())
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "Detectado".into())
                } else {
                    "Automático".into()
                };
                if let Some(buf) = draft.external_apps.path_mut(id) {
                    prefs_row(ui, inner_lw, &format!("{}:", app.name()), |ui| {
                        ui.add(
                            egui::TextEdit::singleline(buf)
                                .desired_width(210.0)
                                .hint_text(&hint),
                        );
                        if ui.small_button("...").clicked() {
                            let mut fd = rfd::FileDialog::new();
                            if cfg!(windows) {
                                fd = fd.add_filter("Executável", &["exe"]);
                            }
                            if let Some(p) = fd.pick_file() {
                                *buf = p.to_string_lossy().to_string();
                            }
                        }
                        if !buf.is_empty() && ui.small_button("Limpar").clicked() {
                            buf.clear();
                        }
                    });
                }
            }
        },
    );

    prefs_group_box(ui, cx.theme, "Comportamento de Controle e Takes", |ui| {
        prefs_row(ui, inner_lw, "Tempo limite RPC / CLI:", |ui| {
            ui.add_sized(
                [72.0, 20.0],
                egui::DragValue::new(&mut draft.external_apps.rpc_timeout_secs)
                    .range(5..=300)
                    .speed(1),
            );
            ui.label("segundos (5–300 s)");
        });
        prefs_indented(ui, inner_lw, |ui| {
            ui.checkbox(
                &mut draft.external_apps.watch_edited_copies,
                "Recarregar automaticamente cópias editadas salvas externamente",
            );
        });
    });
}

fn prefs_preview_device(ui: &mut egui::Ui, cx: &PrefsFormContext<'_>, draft: &mut Preferences) {
    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(
        ui,
        cx.theme,
        "Dispositivo de Prévia Secundário (p. 1061)",
        |ui| {
            let mode_label = match draft.preview_device.device_mode.as_str() {
                "window" => "Janela destacável no editor",
                "external_deck" => "Monitor externo / Saída secundária dedicada",
                _ => "Monitor secundário do Windows (Tela cheia)",
            };
            prefs_row(ui, inner_lw, "Dispositivo de saída:", |ui| {
                egui::ComboBox::from_id_salt("prefs_prev_dev_mode")
                    .selected_text(mode_label)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut draft.preview_device.device_mode,
                            "secondary_monitor".into(),
                            "Monitor secundário do Windows (Tela cheia)",
                        );
                        ui.selectable_value(
                            &mut draft.preview_device.device_mode,
                            "window".into(),
                            "Janela destacável no editor",
                        );
                        ui.selectable_value(
                            &mut draft.preview_device.device_mode,
                            "external_deck".into(),
                            "Monitor externo / Saída secundária dedicada",
                        );
                    });
            });
            prefs_row(ui, inner_lw, "Nível de fundo (cinza):", |ui| {
                ui.add_sized(
                    [64.0, 20.0],
                    egui::DragValue::new(&mut draft.preview_device.bg_gray).range(0..=48),
                );
            });
            prefs_indented(ui, inner_lw, |ui| {
                ui.checkbox(
                    &mut draft.preview_device.scale_to_fit,
                    "Ajustar imagem para caber mantendo a proporção (Scale to Fit)",
                );
                ui.checkbox(
                    &mut draft.preview_device.match_project_fps,
                    "Sincronizar taxa de atualização com os quadros por segundo do projeto",
                );
                ui.checkbox(
                    &mut draft.preview_device.show_alpha_checkerboard,
                    "Exibir quadriculado para áreas transparentes (Canal Alfa)",
                );
            });
        },
    );
}

fn prefs_audio_device(ui: &mut egui::Ui, cx: &PrefsFormContext<'_>, draft: &mut Preferences) {
    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(
        ui,
        cx.theme,
        "Driver e Dispositivo de Áudio (p. 1075)",
        |ui| {
            let drv_label = match draft.audio_device.driver_mode.as_str() {
                "wasapi_shared" => "WASAPI Compartilhado (Windows)",
                "wasapi_exclusive" => "WASAPI Exclusivo (Baixa latência)",
                "low_latency" => "Modo Baixa Latência",
                _ => "Mapper / Padrão do Sistema Operacional (cpal)",
            };
            prefs_row(ui, inner_lw, "Tipo de dispositivo:", |ui| {
                egui::ComboBox::from_id_salt("prefs_aud_dev_drv")
                    .selected_text(drv_label)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut draft.audio_device.driver_mode,
                            "os_default".into(),
                            "Mapper / Padrão do Sistema Operacional (cpal)",
                        );
                        ui.selectable_value(
                            &mut draft.audio_device.driver_mode,
                            "wasapi_shared".into(),
                            "WASAPI Compartilhado (Windows)",
                        );
                        ui.selectable_value(
                            &mut draft.audio_device.driver_mode,
                            "wasapi_exclusive".into(),
                            "WASAPI Exclusivo (Baixa latência)",
                        );
                        ui.selectable_value(
                            &mut draft.audio_device.driver_mode,
                            "low_latency".into(),
                            "Modo Baixa Latência",
                        );
                    });
            });
            prefs_ro_row(
                ui,
                cx.theme,
                inner_lw,
                "Dispositivo ativo no motor:",
                &format!("{} ({} Hz)", cx.audio_device, cx.audio_sr),
                false,
            );
            prefs_row(ui, inner_lw, "Dispositivo preferencial:", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut draft.audio_device.preferred_device)
                        .desired_width(210.0)
                        .hint_text("Automático (padrão do SO)"),
                );
            });
            prefs_row(ui, inner_lw, "Tamanho do buffer (latência):", |ui| {
                ui.add_sized(
                    [68.0, 20.0],
                    egui::DragValue::new(&mut draft.audio_device.buffer_ms)
                        .range(5..=250)
                        .suffix(" ms"),
                );
            });
            prefs_row(ui, inner_lw, "Compensação de gravação:", |ui| {
                ui.add_sized(
                    [68.0, 20.0],
                    egui::DragValue::new(&mut draft.audio_device.record_offset_ms)
                        .range(-500..=500)
                        .suffix(" ms"),
                );
            });
            prefs_row(ui, inner_lw, "Pasta padrão de gravações:", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut draft.audio_device.default_record_dir)
                        .desired_width(200.0)
                        .hint_text("Pasta do projeto / temporária"),
                );
                if ui.small_button("...").clicked() {
                    if let Some(d) = rfd::FileDialog::new().pick_folder() {
                        draft.audio_device.default_record_dir = d.to_string_lossy().into_owned();
                    }
                }
            });
        },
    );
}

fn prefs_midi_sync(ui: &mut egui::Ui, cx: &PrefsFormContext<'_>, draft: &mut Preferences) {
    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(
        ui,
        cx.theme,
        "MIDI e Sincronização MTC / SMPTE (p. 1090)",
        |ui| {
            prefs_indented(ui, inner_lw, |ui| {
                ui.checkbox(
                    &mut draft.midi_sync.mtc_input_enabled,
                    "Receber código de tempo MIDI (MTC / Disparo de transporte)",
                );
                ui.checkbox(
                    &mut draft.midi_sync.mtc_output_enabled,
                    "Enviar código de tempo MIDI (Gerador MTC para dispositivos externos)",
                );
            });
            prefs_row(ui, inner_lw, "Porta de entrada MIDI:", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut draft.midi_sync.midi_in_device)
                        .desired_width(210.0)
                        .hint_text("Nenhuma / Todas"),
                );
            });
            prefs_row(ui, inner_lw, "Porta de saída MIDI:", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut draft.midi_sync.midi_out_device)
                        .desired_width(210.0)
                        .hint_text("Nenhuma"),
                );
            });
            prefs_row(ui, inner_lw, "Taxa de quadros de Sync:", |ui| {
                egui::ComboBox::from_id_salt("prefs_sync_fps")
                    .selected_text(format!("{} qps", draft.midi_sync.sync_fps))
                    .show_ui(ui, |ui| {
                        for f in ["24", "25", "29.97", "30"] {
                            ui.selectable_value(
                                &mut draft.midi_sync.sync_fps,
                                f.to_string(),
                                format!("{f} qps SMPTE"),
                            );
                        }
                    });
            });
            prefs_row(ui, inner_lw, "Deslocamento de Sync:", |ui| {
                ui.add_sized(
                    [76.0, 20.0],
                    egui::DragValue::new(&mut draft.midi_sync.sync_offset_ms)
                        .range(-60_000..=60_000)
                        .suffix(" ms"),
                );
            });
        },
    );

    prefs_group_box(ui, cx.theme, "Dispositivos de Controle Externo", |ui| {
        prefs_indented(ui, inner_lw, |ui| {
            ui.checkbox(
                &mut draft.midi_sync.control_surface_enabled,
                "Ativar superfície de controle externa (faders e transporte)",
            );
        });
        prefs_row(ui, inner_lw, "Protocolo da superfície:", |ui| {
            egui::ComboBox::from_id_salt("prefs_ctrl_proto")
                .selected_text(match draft.midi_sync.control_surface_protocol.as_str() {
                    "generic_cc" => "MIDI CC Genérico",
                    "osc" => "Open Sound Control (OSC)",
                    _ => "Mackie Control Universal",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut draft.midi_sync.control_surface_protocol,
                        "mackie".into(),
                        "Mackie Control Universal",
                    );
                    ui.selectable_value(
                        &mut draft.midi_sync.control_surface_protocol,
                        "generic_cc".into(),
                        "MIDI CC Genérico",
                    );
                    ui.selectable_value(
                        &mut draft.midi_sync.control_surface_protocol,
                        "osc".into(),
                        "Open Sound Control (OSC)",
                    );
                });
        });
        prefs_row(ui, inner_lw, "Sensibilidade dos controles:", |ui| {
            ui.add(
                egui::Slider::new(&mut draft.midi_sync.control_surface_sensitivity, 0.2..=3.0)
                    .fixed_decimals(2),
            );
        });
    });
}

fn prefs_file_io(ui: &mut egui::Ui, cx: &PrefsFormContext<'_>, draft: &mut Preferences) {
    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(
        ui,
        cx.theme,
        "Entrada / Saída e Hardware de Vídeo (p. 1105)",
        |ui| {
            prefs_indented(ui, inner_lw, |ui| {
                ui.checkbox(
                    &mut draft.file_io.hardware_accel_decode,
                    "Ativar decodificação de vídeo acelerada por hardware (GPU/SO)",
                );
                ui.checkbox(
                    &mut draft.file_io.hardware_accel_encode,
                    "Usar aceleração por hardware na renderização quando suportado",
                );
                ui.checkbox(
                    &mut draft.file_io.prefer_system_demuxer,
                    "Priorizar leitores nativos do sistema operacional para MP4/MOV",
                );
                ui.checkbox(
                    &mut draft.file_io.auto_build_proxies_4k,
                    "Sugerir/gerar proxies automaticamente para mídias 4K ou superiores",
                );
            });
            prefs_row(ui, inner_lw, "FPS de sequências de imagem:", |ui| {
                ui.add_sized(
                    [76.0, 20.0],
                    egui::DragValue::new(&mut draft.file_io.image_sequence_fps)
                        .range(1.0..=120.0)
                        .fixed_decimals(2),
                );
            });
            prefs_row(ui, inner_lw, "Pasta padrão de projetos:", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut draft.file_io.default_project_dir)
                        .desired_width(200.0)
                        .hint_text("Padrão do sistema"),
                );
                if ui.small_button("...").clicked() {
                    if let Some(d) = rfd::FileDialog::new().pick_folder() {
                        draft.file_io.default_project_dir = d.to_string_lossy().into_owned();
                    }
                }
            });
            prefs_row(ui, inner_lw, "Pasta de arquivos proxy:", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut draft.file_io.default_proxy_dir)
                        .desired_width(200.0)
                        .hint_text("Mesma pasta da mídia original"),
                );
                if ui.small_button("...").clicked() {
                    if let Some(d) = rfd::FileDialog::new().pick_folder() {
                        draft.file_io.default_proxy_dir = d.to_string_lossy().into_owned();
                    }
                }
            });
        },
    );
}
