//! Motor do **Neoveg Capture** (paridade limpa com o *VEGAS Capture* dos
//! manuais do VEGAS Pro 21–26): captura simultânea de múltiplas fontes de
//! vídeo (telas, janelas de aplicativos e câmeras) e áudio (microfones e áudio
//! do sistema via WASAPI loopback), configurações por dispositivo, estimativa
//! de uso de disco, manifestos de sessão `.nvcap` / `export.json` e importação
//! direta em trilhas dedicadas na linha do tempo do projeto (`AGENTS.md` §0,
//! §2.2 e §3.6).

pub mod manifest;
pub mod runtime;

pub use crate::media::capture_os::{
    format_bytes_human, free_disk_bytes, OsCameraInfo, OsMonitorInfo, OsWindowInfo, MAX_CAPTURE_DIM,
};
pub use manifest::{
    apply_manifest_to_session, load_manifest, save_manifest, CaptureImportReport,
    CaptureProjectManifest, CapturedTrackEntry,
};
pub use runtime::{CaptureRuntime, CaptureSessionResult, SourceLiveSnapshot, SourceRuntimeStatus};

use crate::model::{FrameRate, WorkFolders};
use neoveg_common::{tr, Language};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Tamanho máximo aceito para o arquivo de configurações do Neoveg Capture (§0.4).
const MAX_SETTINGS_BYTES: u64 = 128 * 1024;
/// Comprimento máximo de nomes/caminhos sanitizados nas configurações.
const MAX_PATH_CHARS: usize = 1024;
/// Versão exibida na tela de configurações do Neoveg Capture.
pub const CAPTURE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Codec de vídeo para gravação em tempo real no Neoveg Capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CaptureVideoCodec {
    H264Mp4,
    HevcMp4,
    MjpegAvi,
    Ffv1Mkv,
}

impl CaptureVideoCodec {
    pub const ALL: [Self; 4] = [Self::H264Mp4, Self::HevcMp4, Self::MjpegAvi, Self::Ffv1Mkv];

    pub fn default_for_os() -> Self {
        #[cfg(windows)]
        {
            Self::H264Mp4
        }
        #[cfg(not(windows))]
        {
            Self::MjpegAvi
        }
    }

    pub fn available_on_os(self) -> bool {
        match self {
            Self::H264Mp4 | Self::HevcMp4 => cfg!(windows),
            Self::MjpegAvi | Self::Ffv1Mkv => true,
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Self::H264Mp4 | Self::HevcMp4 => "mp4",
            Self::MjpegAvi => "avi",
            Self::Ffv1Mkv => "mkv",
        }
    }

    pub fn label_in(self, lang: Language) -> &'static str {
        let pt = match self {
            Self::H264Mp4 => "AVC (H.264 / MP4)",
            Self::HevcMp4 => "HEVC (H.265 / MP4)",
            Self::MjpegAvi => "Motion-JPEG (AVI)",
            Self::Ffv1Mkv => "FFV1 sem perdas (MKV)",
        };
        tr(lang, pt)
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Self::H264Mp4 => "AVC (H.264)",
            Self::HevcMp4 => "HEVC (H.265)",
            Self::MjpegAvi => "MJPEG (AVI)",
            Self::Ffv1Mkv => "FFV1 (MKV)",
        }
    }
}

impl Default for CaptureVideoCodec {
    fn default() -> Self {
        Self::default_for_os()
    }
}

/// Codec de áudio para gravação no Neoveg Capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CaptureAudioCodec {
    WavPcm16,
    WavPcm24,
    Mp3,
    AacM4a,
}

impl CaptureAudioCodec {
    pub const ALL: [Self; 4] = [Self::WavPcm16, Self::WavPcm24, Self::Mp3, Self::AacM4a];

    pub fn default_for_os() -> Self {
        Self::WavPcm16
    }

    pub fn available_on_os(self) -> bool {
        match self {
            Self::WavPcm16 | Self::WavPcm24 => true,
            Self::Mp3 | Self::AacM4a => cfg!(windows),
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Self::WavPcm16 | Self::WavPcm24 => "wav",
            Self::Mp3 => "mp3",
            Self::AacM4a => "m4a",
        }
    }

    pub fn label_in(self, lang: Language) -> &'static str {
        let pt = match self {
            Self::WavPcm16 => "WAV (PCM 16-bit)",
            Self::WavPcm24 => "WAV (PCM 24-bit)",
            Self::Mp3 => "MP3",
            Self::AacM4a => "AAC (M4A)",
        };
        tr(lang, pt)
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Self::WavPcm16 => "WAV 16-bit",
            Self::WavPcm24 => "WAV 24-bit",
            Self::Mp3 => "MP3",
            Self::AacM4a => "AAC",
        }
    }
}

impl Default for CaptureAudioCodec {
    fn default() -> Self {
        Self::default_for_os()
    }
}

/// Qualidade de codificação (mapeada para taxa de bits / nível JPEG).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CaptureQuality {
    Low,
    #[default]
    Medium,
    High,
    Lossless,
}

impl CaptureQuality {
    pub const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::High, Self::Lossless];

    pub fn label_in(self, lang: Language) -> &'static str {
        let pt = match self {
            Self::Low => "Baixa",
            Self::Medium => "Média",
            Self::High => "Alta",
            Self::Lossless => "Sem perdas",
        };
        tr(lang, pt)
    }

    /// Taxa de bits alvo (bits/s) para vídeo 1080p@30fps, escalada pela resolução e FPS.
    pub fn video_bitrate_bps(self, w: u32, h: u32, fps: FrameRate) -> u32 {
        let base_1080p30: f64 = match self {
            Self::Low => 4_000_000.0,
            Self::Medium => 10_000_000.0,
            Self::High => 20_000_000.0,
            Self::Lossless => 40_000_000.0,
        };
        let px_ratio =
            ((f64::from(w.max(16)) * f64::from(h.max(16))) / (1920.0 * 1080.0)).clamp(0.15, 4.0);
        let fps_ratio = (fps.as_f64() / 30.0).clamp(0.35, 2.5);
        (base_1080p30 * px_ratio * fps_ratio)
            .round()
            .clamp(500_000.0, 80_000_000.0) as u32
    }

    /// Qualidade JPEG (1..=100) usada na gravação MJPEG em AVI.
    pub fn jpeg_quality(self) -> u8 {
        match self {
            Self::Low => 72,
            Self::Medium => 84,
            Self::High => 92,
            Self::Lossless => 98,
        }
    }

    /// Taxa de bits de áudio comprimido (MP3/AAC) em bits/s.
    pub fn audio_bitrate_bps(self) -> u32 {
        match self {
            Self::Low => 128_000,
            Self::Medium => 192_000,
            Self::High => 256_000,
            Self::Lossless => 320_000,
        }
    }
}

/// Resolução de saída configurável por fonte de vídeo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CaptureResolution {
    #[default]
    Native,
    R1080p,
    R720p,
    R480p,
}

impl CaptureResolution {
    pub const ALL: [Self; 4] = [Self::Native, Self::R1080p, Self::R720p, Self::R480p];

    pub fn label_in(self, lang: Language) -> &'static str {
        let pt = match self {
            Self::Native => "Nativa",
            Self::R1080p => "1920×1080 (Full HD)",
            Self::R720p => "1280×720 (HD)",
            Self::R480p => "854×480 (SD)",
        };
        tr(lang, pt)
    }

    /// Resolve dimensões pares válidas a partir das dimensões nativas da fonte.
    pub fn resolve(self, native_w: u32, native_h: u32) -> (u32, u32) {
        let (w, h) = match self {
            Self::Native => (native_w.max(16), native_h.max(16)),
            Self::R1080p => (1920, 1080),
            Self::R720p => (1280, 720),
            Self::R480p => (854, 480),
        };
        let ew = ((w.clamp(16, MAX_CAPTURE_DIM)) / 2).max(1) * 2;
        let eh = ((h.clamp(16, MAX_CAPTURE_DIM)) / 2).max(1) * 2;
        (ew, eh)
    }
}

/// Taxas de quadros comuns na janela de configuração de dispositivos de vídeo.
pub const CAPTURE_FPS_OPTIONS: [(&str, FrameRate); 6] = [
    ("15 fps", FrameRate { num: 15, den: 1 }),
    ("24 fps", FrameRate::FPS_24),
    ("25 fps", FrameRate::FPS_25),
    ("29,97 fps", FrameRate::FPS_29_97),
    ("30 fps", FrameRate::FPS_30),
    ("60 fps", FrameRate::FPS_60),
];

/// Taxas de amostragem comuns para fontes de áudio.
pub const CAPTURE_SAMPLE_RATES: [u32; 2] = [44_100, 48_000];

/// Estima tamanho em disco (em bytes) para 10 minutos de vídeo Full HD 30 fps
/// com o codec e qualidade escolhidos (como exibido em *VEGAS Capture — Settings*).
pub fn estimate_video_10min_bytes(codec: CaptureVideoCodec, quality: CaptureQuality) -> u64 {
    let bps: u64 = match codec {
        CaptureVideoCodec::H264Mp4 => {
            u64::from(quality.video_bitrate_bps(1920, 1080, FrameRate::FPS_30))
        }
        CaptureVideoCodec::HevcMp4 => {
            u64::from(quality.video_bitrate_bps(1920, 1080, FrameRate::FPS_30)) * 7 / 10
        }
        CaptureVideoCodec::MjpegAvi => match quality {
            CaptureQuality::Low => 28_000_000,
            CaptureQuality::Medium => 55_000_000,
            CaptureQuality::High => 95_000_000,
            CaptureQuality::Lossless => 140_000_000,
        },
        CaptureVideoCodec::Ffv1Mkv => 320_000_000,
    };
    (bps / 8).saturating_mul(600)
}

/// Estima tamanho em disco (em bytes) para 10 minutos de áudio estéreo 48 kHz.
pub fn estimate_audio_10min_bytes(
    codec: CaptureAudioCodec,
    quality: CaptureQuality,
    channels: u16,
    sample_rate: u32,
) -> u64 {
    let ch = u64::from(channels.clamp(1, 2));
    let sr = u64::from(sample_rate.clamp(8_000, 192_000));
    let bytes_per_sec = match codec {
        CaptureAudioCodec::WavPcm16 => sr.saturating_mul(ch).saturating_mul(2),
        CaptureAudioCodec::WavPcm24 => sr.saturating_mul(ch).saturating_mul(3),
        CaptureAudioCodec::Mp3 | CaptureAudioCodec::AacM4a => {
            u64::from(quality.audio_bitrate_bps()) / 8
        }
    };
    bytes_per_sec.saturating_mul(600)
}

/// Tipo concreto de fonte adicionada ao painel do Neoveg Capture.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureSourceKind {
    Screen {
        monitor_id: String,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    Window {
        window_id: String,
        width: u32,
        height: u32,
    },
    Camera {
        device_id: String,
        width: u32,
        height: u32,
    },
    AudioInput {
        device_name: String,
    },
    AudioOutputLoopback {
        device_name: String,
    },
    /// Fontes sintéticas determinísticas para validação em ambientes sem hardware.
    SyntheticVideo {
        width: u32,
        height: u32,
        seed: u32,
    },
    SyntheticAudio {
        freq_hz: u32,
        sample_rate: u32,
    },
}

impl CaptureSourceKind {
    pub fn is_video(&self) -> bool {
        matches!(
            self,
            Self::Screen { .. }
                | Self::Window { .. }
                | Self::Camera { .. }
                | Self::SyntheticVideo { .. }
        )
    }

    pub fn is_audio(&self) -> bool {
        !self.is_video()
    }

    /// Chave estável para evitar duplicar a mesma fonte física no painel.
    pub fn unique_key(&self) -> String {
        match self {
            Self::Screen { monitor_id, .. } => format!("screen:{monitor_id}"),
            Self::Window { window_id, .. } => format!("window:{window_id}"),
            Self::Camera { device_id, .. } => format!("camera:{device_id}"),
            Self::AudioInput { device_name } => format!("ain:{device_name}"),
            Self::AudioOutputLoopback { device_name } => format!("aloop:{device_name}"),
            Self::SyntheticVideo { seed, .. } => format!("synv:{seed}"),
            Self::SyntheticAudio { freq_hz, .. } => format!("syna:{freq_hz}"),
        }
    }

    pub fn native_size(&self) -> (u32, u32) {
        match self {
            Self::Screen { width, height, .. }
            | Self::Window { width, height, .. }
            | Self::Camera { width, height, .. }
            | Self::SyntheticVideo { width, height, .. } => (*width, *height),
            Self::AudioInput { .. }
            | Self::AudioOutputLoopback { .. }
            | Self::SyntheticAudio { .. } => (0, 0),
        }
    }

    pub fn category_label(&self, lang: Language) -> &'static str {
        let pt = match self {
            Self::Screen { .. } => "Tela",
            Self::Window { .. } => "Janela",
            Self::Camera { .. } => "Câmera",
            Self::AudioInput { .. } => "Microfone",
            Self::AudioOutputLoopback { .. } => "Áudio do sistema",
            Self::SyntheticVideo { .. } => "Vídeo de teste",
            Self::SyntheticAudio { .. } => "Áudio de teste",
        };
        tr(lang, pt)
    }
}

/// Cartão de fonte configurada no painel do Neoveg Capture (cada cartão tem
/// prévia ao vivo ou medidor VU e suas próprias configurações de dispositivo).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptureSourceCard {
    pub id: u64,
    pub name: String,
    pub kind: CaptureSourceKind,
    pub enabled: bool,
    pub muted: bool,
    pub resolution: CaptureResolution,
    pub fps: FrameRate,
    pub video_codec: CaptureVideoCodec,
    pub video_quality: CaptureQuality,
    pub capture_cursor: bool,
    pub audio_codec: CaptureAudioCodec,
    pub audio_quality: CaptureQuality,
    pub audio_channels: u16,
    pub audio_sample_rate: u32,
}

impl CaptureSourceCard {
    pub fn new(id: u64, name: String, kind: CaptureSourceKind, defaults: &CaptureSettings) -> Self {
        let video_codec = if defaults.default_video_codec.available_on_os() {
            defaults.default_video_codec
        } else {
            CaptureVideoCodec::default_for_os()
        };
        let audio_codec = if defaults.default_audio_codec.available_on_os() {
            defaults.default_audio_codec
        } else {
            CaptureAudioCodec::default_for_os()
        };
        Self {
            id,
            name,
            kind,
            enabled: true,
            muted: false,
            resolution: CaptureResolution::Native,
            fps: defaults.default_fps,
            video_codec,
            video_quality: defaults.default_video_quality,
            capture_cursor: defaults.capture_cursor,
            audio_codec,
            audio_quality: defaults.default_audio_quality,
            audio_channels: defaults.default_audio_channels.clamp(1, 2),
            audio_sample_rate: defaults.default_audio_sample_rate.clamp(8_000, 192_000),
        }
    }

    pub fn is_video(&self) -> bool {
        self.kind.is_video()
    }

    pub fn is_audio(&self) -> bool {
        self.kind.is_audio()
    }

    pub fn target_dimensions(&self) -> (u32, u32) {
        let (nw, nh) = self.kind.native_size();
        self.resolution.resolve(nw, nh)
    }

    pub fn video_bitrate_bps(&self) -> u32 {
        let (w, h) = self.target_dimensions();
        self.video_quality.video_bitrate_bps(w, h, self.fps)
    }

    /// Linha de resumo exibida abaixo do título do cartão (ex.:
    /// `AVC (H.264); 10,0 Mbit/s; 30 fps; 1920×1080` ou
    /// `WAV 16-bit; Estéreo; 48000 Hz`).
    pub fn summary_line(&self, lang: Language) -> String {
        if self.is_video() {
            let (w, h) = self.target_dimensions();
            let fps = self.fps.as_f64();
            let fps_str = if (fps - fps.round()).abs() < 0.05 {
                format!("{} fps", fps.round() as u32)
            } else {
                format!("{fps:.2} fps")
            };
            match self.video_codec {
                CaptureVideoCodec::H264Mp4 | CaptureVideoCodec::HevcMp4 => {
                    let mbps = f64::from(self.video_bitrate_bps()) / 1_000_000.0;
                    format!(
                        "{}; {mbps:.1} Mbit/s; {fps_str}; {w}×{h}",
                        self.video_codec.short_name()
                    )
                }
                CaptureVideoCodec::MjpegAvi | CaptureVideoCodec::Ffv1Mkv => {
                    format!(
                        "{}; {}; {fps_str}; {w}×{h}",
                        self.video_codec.short_name(),
                        self.video_quality.label_in(lang)
                    )
                }
            }
        } else {
            let ch = if self.audio_channels <= 1 {
                tr(lang, "Mono")
            } else {
                tr(lang, "Estéreo")
            };
            match self.audio_codec {
                CaptureAudioCodec::Mp3 | CaptureAudioCodec::AacM4a => {
                    let kbps = self.audio_quality.audio_bitrate_bps() / 1000;
                    format!(
                        "{}; {kbps} kbit/s; {ch}; {} Hz",
                        self.audio_codec.short_name(),
                        self.audio_sample_rate
                    )
                }
                CaptureAudioCodec::WavPcm16 | CaptureAudioCodec::WavPcm24 => {
                    format!(
                        "{}; {ch}; {} Hz",
                        self.audio_codec.short_name(),
                        self.audio_sample_rate
                    )
                }
            }
        }
    }
}

/// Configurações globais persistidas do Neoveg Capture (espelho de *VEGAS Capture — Settings*).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureSettings {
    /// Pasta personalizada de gravação (vazia = usa `work_folders.captured_media` do projeto).
    pub recording_folder: String,
    pub default_video_codec: CaptureVideoCodec,
    pub default_video_quality: CaptureQuality,
    pub default_fps: FrameRate,
    pub default_audio_codec: CaptureAudioCodec,
    pub default_audio_quality: CaptureQuality,
    pub default_audio_channels: u16,
    pub default_audio_sample_rate: u32,
    pub capture_cursor: bool,
    /// Contagem regressiva antes de começar a gravar (`0`, `3` ou `5` segundos).
    pub countdown_secs: u8,
    /// Se verdadeiro, importa automaticamente na timeline ao clicar em Parar.
    pub auto_import_on_stop: bool,
}

impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            recording_folder: String::new(),
            default_video_codec: CaptureVideoCodec::default_for_os(),
            default_video_quality: CaptureQuality::Medium,
            default_fps: FrameRate::FPS_30,
            default_audio_codec: CaptureAudioCodec::default_for_os(),
            default_audio_quality: CaptureQuality::Medium,
            default_audio_channels: 2,
            default_audio_sample_rate: 48_000,
            capture_cursor: true,
            countdown_secs: 0,
            auto_import_on_stop: false,
        }
    }
}

impl CaptureSettings {
    pub fn sanitize(&mut self) {
        self.recording_folder = self
            .recording_folder
            .chars()
            .filter(|&c| c != '\0' && !c.is_control())
            .take(MAX_PATH_CHARS)
            .collect::<String>()
            .trim()
            .to_string();
        if !self.default_video_codec.available_on_os() {
            self.default_video_codec = CaptureVideoCodec::default_for_os();
        }
        if !self.default_audio_codec.available_on_os() {
            self.default_audio_codec = CaptureAudioCodec::default_for_os();
        }
        if !(1.0..=120.0).contains(&self.default_fps.as_f64()) {
            self.default_fps = FrameRate::FPS_30;
        }
        self.default_audio_channels = self.default_audio_channels.clamp(1, 2);
        self.default_audio_sample_rate = if self.default_audio_sample_rate == 44_100 {
            44_100
        } else {
            48_000
        };
        self.countdown_secs = match self.countdown_secs {
            0 => 0,
            1..=3 => 3,
            _ => 5,
        };
    }

    pub fn settings_path() -> PathBuf {
        neoveg_common::util::base_dir().join("capture_settings.json")
    }

    pub fn load_from_disk() -> Self {
        Self::load_from(&Self::settings_path())
    }

    pub fn load_from(path: &Path) -> Self {
        let Ok(meta) = std::fs::metadata(path) else {
            return Self::default();
        };
        if meta.len() > MAX_SETTINGS_BYTES {
            return Self::default();
        }
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let mut s: Self = serde_json::from_str(&raw).unwrap_or_default();
        s.sanitize();
        s
    }

    pub fn save_to_disk(&self) -> anyhow::Result<()> {
        self.save_to(&Self::settings_path())
    }

    pub fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        let mut clean = self.clone();
        clean.sanitize();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&clean)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Resolve a pasta efetiva de gravação: preferência explícita ou
    /// `work_folders.effective_captured_media(...)` do projeto.
    pub fn effective_recording_dir(
        &self,
        work_folders: &WorkFolders,
        project_path: Option<&Path>,
    ) -> PathBuf {
        let trimmed = self.recording_folder.trim();
        if !trimmed.is_empty() {
            PathBuf::from(trimmed)
        } else {
            work_folders.effective_captured_media(project_path)
        }
    }
}

/// Dispositivo de áudio enumerado pelo sistema (`cpal`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OsAudioDeviceInfo {
    pub name: String,
    pub is_loopback: bool,
    pub is_default: bool,
    pub channels: u16,
    pub sample_rate: u32,
}

/// Catálogo completo de fontes descobertas no sistema operacional para a janela
/// *Adicionar fontes de captura*.
#[derive(Clone, Debug, Default)]
pub struct DiscoveredSources {
    pub monitors: Vec<OsMonitorInfo>,
    pub windows: Vec<OsWindowInfo>,
    pub cameras: Vec<OsCameraInfo>,
    pub audio_inputs: Vec<OsAudioDeviceInfo>,
    pub audio_outputs: Vec<OsAudioDeviceInfo>,
}

/// Descobre telas, janelas, câmeras e dispositivos de áudio (microfones e, no
/// Windows, saídas WASAPI para loopback de áudio do sistema).
pub fn discover_sources() -> DiscoveredSources {
    use cpal::traits::{DeviceTrait, HostTrait};

    let monitors = crate::media::capture_os::list_monitors();
    let windows = crate::media::capture_os::list_windows();
    let cameras = crate::media::capture_os::list_cameras();

    crate::playback::ensure_audio_com_keeper();
    let host = cpal::default_host();

    let mut audio_inputs = Vec::new();
    let default_in = host.default_input_device().and_then(|d| d.name().ok());
    if let Ok(devs) = host.input_devices() {
        for dev in devs {
            let Ok(name) = dev.name() else {
                continue;
            };
            let (channels, sample_rate) = dev
                .default_input_config()
                .map(|c| {
                    (
                        c.channels().clamp(1, 2),
                        c.sample_rate().0.clamp(8_000, 192_000),
                    )
                })
                .unwrap_or((2, 48_000));
            let is_default = default_in.as_deref() == Some(name.as_str());
            audio_inputs.push(OsAudioDeviceInfo {
                name,
                is_loopback: false,
                is_default,
                channels,
                sample_rate,
            });
        }
    }

    // Loopback de áudio do sistema: no Windows o backend WASAPI do `cpal`
    // habilita automaticamente `AUDCLNT_STREAMFLAGS_LOOPBACK` em dispositivos
    // de saída abertos com `build_input_stream`.
    #[cfg(windows)]
    let mut audio_outputs = Vec::new();
    #[cfg(not(windows))]
    let audio_outputs = Vec::new();
    #[cfg(windows)]
    {
        let default_out = host.default_output_device().and_then(|d| d.name().ok());
        if let Ok(devs) = host.output_devices() {
            for dev in devs {
                let Ok(name) = dev.name() else {
                    continue;
                };
                let (channels, sample_rate) = dev
                    .default_output_config()
                    .map(|c| {
                        (
                            c.channels().clamp(1, 2),
                            c.sample_rate().0.clamp(8_000, 192_000),
                        )
                    })
                    .unwrap_or((2, 48_000));
                let is_default = default_out.as_deref() == Some(name.as_str());
                audio_outputs.push(OsAudioDeviceInfo {
                    name,
                    is_loopback: true,
                    is_default,
                    channels,
                    sample_rate,
                });
            }
        }
    }

    DiscoveredSources {
        monitors,
        windows,
        cameras,
        audio_inputs,
        audio_outputs,
    }
}

/// Lista apenas os nomes dos dispositivos de entrada de áudio disponíveis no SO
/// (leve, sem varrer janelas nem câmeras), para uso nos seletores de entrada
/// das trilhas de áudio e do Console de Mixagem.
pub fn list_audio_input_names() -> Vec<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    crate::playback::ensure_audio_com_keeper();
    let host = cpal::default_host();
    let mut out = Vec::new();
    if let Ok(devs) = host.input_devices() {
        for dev in devs {
            if let Ok(name) = dev.name() {
                if !name.trim().is_empty() && !out.contains(&name) {
                    out.push(name);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuracoes_sanitizam_entradas_hostis_e_persistem() {
        let dir = std::env::temp_dir().join(format!("neoveg_cap_cfg_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("capture_settings.json");

        let mut cfg = CaptureSettings {
            recording_folder: format!("  C:\\grav\0\n{}  ", "a".repeat(2000)),
            default_fps: FrameRate { num: 0, den: 1 },
            default_audio_channels: 99,
            default_audio_sample_rate: 12345,
            countdown_secs: 42,
            ..CaptureSettings::default()
        };
        cfg.sanitize();
        assert!(!cfg.recording_folder.contains('\0'));
        assert!(cfg.recording_folder.len() <= MAX_PATH_CHARS);
        assert_eq!(cfg.default_fps, FrameRate::FPS_30);
        assert_eq!(cfg.default_audio_channels, 2);
        assert_eq!(cfg.default_audio_sample_rate, 48_000);
        assert_eq!(cfg.countdown_secs, 5);

        cfg.save_to(&p).expect("salvar capture_settings.json");
        let loaded = CaptureSettings::load_from(&p);
        assert_eq!(loaded.countdown_secs, 5);
        assert_eq!(loaded.default_audio_sample_rate, 48_000);

        // JSON corrompido cai com segurança no padrão
        std::fs::write(&p, "{quebrado").expect("escrever corrompido");
        let fallback = CaptureSettings::load_from(&p);
        assert_eq!(fallback.default_fps, FrameRate::FPS_30);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resumo_de_cartao_e_estimativas_de_disco_sao_validos() {
        let defs = CaptureSettings::default();
        let v = CaptureSourceCard::new(
            1,
            "Tela 1".into(),
            CaptureSourceKind::Screen {
                monitor_id: "m1".into(),
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            &defs,
        );
        let vs = v.summary_line(Language::PtBr);
        assert!(vs.contains("1920×1080") && vs.contains("30 fps"), "{vs}");

        let a = CaptureSourceCard::new(
            2,
            "Microfone".into(),
            CaptureSourceKind::AudioInput {
                device_name: "Mic".into(),
            },
            &defs,
        );
        let as_line = a.summary_line(Language::PtBr);
        assert!(as_line.contains("48000 Hz"), "{as_line}");

        for vc in CaptureVideoCodec::ALL {
            assert!(estimate_video_10min_bytes(vc, CaptureQuality::Medium) > 0);
        }
        for ac in CaptureAudioCodec::ALL {
            assert!(estimate_audio_10min_bytes(ac, CaptureQuality::Medium, 2, 48_000) > 0);
        }
    }
}
