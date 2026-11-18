//! Núcleo do **Neoveg Encoder**: fila persistente com múltiplas saídas por origem,
//! predefinições e grupos, pastas monitoradas, efeitos de pré-codificação e
//! execução paralela protegida (`AGENTS.md` §0, §2.2, §3.3).

pub mod effects;
pub mod presets;
pub mod watch;
pub mod worker;

use crate::model::{new_id, FrameRate, Project};
use crate::render::{RenderFormat, WavBits};
use crate::time::Ticks;
use anyhow::{bail, Result};
use effects::{CropRect, PreEncodeEffects, SourceScalingMode};
use presets::PresetCatalog;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use watch::WatchFolder;

const MAX_STATE_BYTES: u64 = 16 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SourceRangeMode {
    #[default]
    EntireClip,
    InOut,
    Custom,
}

impl SourceRangeMode {
    pub const ALL: [Self; 3] = [Self::EntireClip, Self::InOut, Self::Custom];

    pub fn name_in(self, lang: neoveg_common::Language) -> &'static str {
        let pt = match self {
            Self::EntireClip => "Clipe / Sequência inteira",
            Self::InOut => "Intervalo Entrada / Saída",
            Self::Custom => "Personalizado",
        };
        neoveg_common::tr(lang, pt)
    }

    pub fn name(self) -> &'static str {
        self.name_in(neoveg_common::Language::PtBr)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AlphaInterpret {
    #[default]
    Auto,
    Straight,
    Premultiplied,
    Ignore,
}

impl AlphaInterpret {
    pub const ALL: [Self; 4] = [
        Self::Auto,
        Self::Straight,
        Self::Premultiplied,
        Self::Ignore,
    ];

    pub fn name_in(self, lang: neoveg_common::Language) -> &'static str {
        let pt = match self {
            Self::Auto => "Detectar automaticamente",
            Self::Straight => "Alfa direto (não pré-multiplicado)",
            Self::Premultiplied => "Alfa pré-multiplicado",
            Self::Ignore => "Ignorar canal alfa",
        };
        neoveg_common::tr(lang, pt)
    }

    pub fn name(self) -> &'static str {
        self.name_in(neoveg_common::Language::PtBr)
    }
}

/// Interpretação explícita da gravação de origem (§Interpretar gravação).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MediaInterpretation {
    pub fps_override: Option<FrameRate>,
    pub pixel_aspect: f32,
    pub alpha: AlphaInterpret,
}

impl Default for MediaInterpretation {
    fn default() -> Self {
        Self {
            fps_override: None,
            pixel_aspect: 1.0,
            alpha: AlphaInterpret::Auto,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum JobStatus {
    #[default]
    Ready,
    Encoding,
    Done,
    DoneWithWarnings,
    Stopped,
    Failed,
    Skipped,
}

impl JobStatus {
    pub fn name_in(self, lang: neoveg_common::Language) -> &'static str {
        let pt = match self {
            Self::Ready => "Pronto",
            Self::Encoding => "Codificando",
            Self::Done => "Concluído",
            Self::DoneWithWarnings => "Concluído com avisos",
            Self::Stopped => "Parado",
            Self::Failed => "Falha",
            Self::Skipped => "Ignorar",
        };
        neoveg_common::tr(lang, pt)
    }

    pub fn name(self) -> &'static str {
        self.name_in(neoveg_common::Language::PtBr)
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Done | Self::DoneWithWarnings | Self::Stopped | Self::Failed | Self::Skipped
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum QueueSource {
    ProjectSnapshot {
        title: String,
        project_path: Option<PathBuf>,
        project: Box<Project>,
        in_point: Option<Ticks>,
        out_point: Option<Ticks>,
    },
    ProjectFile {
        path: PathBuf,
    },
    MediaFile {
        path: PathBuf,
        #[serde(default)]
        is_sequence: bool,
        #[serde(default)]
        sequence_fps: Option<FrameRate>,
        #[serde(default)]
        interpret: MediaInterpretation,
    },
}

impl QueueSource {
    pub fn display_name(&self) -> String {
        match self {
            Self::ProjectSnapshot { title, .. } => title.clone(),
            Self::ProjectFile { path } | Self::MediaFile { path, .. } => path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| path.display().to_string()),
        }
    }

    pub fn source_dir(&self) -> PathBuf {
        match self {
            Self::ProjectSnapshot { project_path, .. } => project_path
                .as_deref()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
                .unwrap_or_else(default_renders_dir),
            Self::ProjectFile { path } | Self::MediaFile { path, .. } => path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(Path::to_path_buf)
                .unwrap_or_else(default_renders_dir),
        }
    }

    pub fn stem(&self) -> String {
        match self {
            Self::ProjectSnapshot {
                title,
                project_path,
                ..
            } => project_path
                .as_deref()
                .and_then(Path::file_stem)
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| sanitize_stem(title)),
            Self::ProjectFile { path } | Self::MediaFile { path, .. } => path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "saida".to_string()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OutputJob {
    pub id: u64,
    pub preset_id: String,
    pub preset_name: String,
    pub format: RenderFormat,
    pub output_path: PathBuf,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub match_source_dims: bool,
    #[serde(default)]
    pub fps_override: Option<FrameRate>,
    #[serde(default = "yes")]
    pub include_video: bool,
    #[serde(default = "yes")]
    pub include_audio: bool,
    #[serde(default = "default_vkbps")]
    pub video_bitrate_kbps: u32,
    #[serde(default = "default_akbps")]
    pub audio_bitrate_kbps: u32,
    #[serde(default = "default_jq")]
    pub jpeg_quality: u8,
    #[serde(default = "default_wb")]
    pub wav_bits: WavBits,
    #[serde(default)]
    pub range: SourceRangeMode,
    #[serde(default)]
    pub custom_in: Ticks,
    #[serde(default)]
    pub custom_out: Ticks,
    #[serde(default)]
    pub crop: CropRect,
    #[serde(default)]
    pub scaling: SourceScalingMode,
    #[serde(default)]
    pub effects: PreEncodeEffects,
    #[serde(default)]
    pub status: JobStatus,
    #[serde(default)]
    pub progress: f32,
    #[serde(default)]
    pub elapsed_secs: f64,
    #[serde(default)]
    pub eta_secs: Option<f64>,
    #[serde(default)]
    pub last_message: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

fn yes() -> bool {
    true
}
fn default_vkbps() -> u32 {
    12_000
}
fn default_akbps() -> u32 {
    192
}
fn default_jq() -> u8 {
    90
}
fn default_wb() -> WavBits {
    WavBits::Pcm16
}

impl OutputJob {
    pub fn from_preset(
        preset: &presets::EncoderPreset,
        source: &QueueSource,
        out_dir_override: Option<&Path>,
        src_hint_dims: (u32, u32),
    ) -> Self {
        let dir = out_dir_override
            .map(Path::to_path_buf)
            .unwrap_or_else(|| source.source_dir());
        let stem = source.stem();
        let file = if preset.system {
            format!("{stem}.{}", preset.format.ext())
        } else {
            format!(
                "{stem}_{}.{}",
                sanitize_stem(&preset.name),
                preset.format.ext()
            )
        };
        let (w, h) = (
            preset.width.unwrap_or(src_hint_dims.0).max(2),
            preset.height.unwrap_or(src_hint_dims.1).max(2),
        );
        Self {
            id: new_id(),
            preset_id: preset.id.clone(),
            preset_name: preset.name.clone(),
            format: preset.format,
            output_path: dir.join(file),
            width: w,
            height: h,
            match_source_dims: preset.width.is_none() || preset.height.is_none(),
            fps_override: preset.fps,
            include_video: preset.include_video,
            include_audio: preset.include_audio,
            video_bitrate_kbps: preset.video_bitrate_kbps,
            audio_bitrate_kbps: preset.audio_bitrate_kbps,
            jpeg_quality: preset.jpeg_quality,
            wav_bits: preset.wav_bits,
            range: SourceRangeMode::EntireClip,
            custom_in: Ticks::ZERO,
            custom_out: Ticks::ZERO,
            crop: CropRect::default(),
            scaling: preset.scaling,
            effects: preset.effects.clone(),
            status: JobStatus::Ready,
            progress: 0.0,
            elapsed_secs: 0.0,
            eta_secs: None,
            last_message: None,
            warnings: Vec::new(),
        }
    }

    pub fn apply_preset(&mut self, preset: &presets::EncoderPreset, src_hint_dims: (u32, u32)) {
        self.preset_id = preset.id.clone();
        self.preset_name = preset.name.clone();
        self.format = preset.format;
        self.match_source_dims = preset.width.is_none() || preset.height.is_none();
        self.width = preset.width.unwrap_or(src_hint_dims.0).max(2);
        self.height = preset.height.unwrap_or(src_hint_dims.1).max(2);
        self.fps_override = preset.fps;
        self.include_video = preset.include_video;
        self.include_audio = preset.include_audio;
        self.video_bitrate_kbps = preset.video_bitrate_kbps;
        self.audio_bitrate_kbps = preset.audio_bitrate_kbps;
        self.jpeg_quality = preset.jpeg_quality;
        self.wav_bits = preset.wav_bits;
        self.scaling = preset.scaling;
        self.effects = preset.effects.clone();
        self.output_path = self.output_path.with_extension(preset.format.ext());
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueueItem {
    pub id: u64,
    pub source: QueueSource,
    pub outputs: Vec<OutputJob>,
    #[serde(default)]
    pub expanded: bool,
    #[serde(default)]
    pub watch_folder_id: Option<u64>,
}

impl QueueItem {
    pub fn aggregate_status(&self) -> JobStatus {
        if self.outputs.is_empty() {
            return JobStatus::Skipped;
        }
        if self.outputs.iter().any(|o| o.status == JobStatus::Encoding) {
            return JobStatus::Encoding;
        }
        if self.outputs.iter().any(|o| o.status == JobStatus::Ready) {
            return JobStatus::Ready;
        }
        if self.outputs.iter().any(|o| o.status == JobStatus::Failed) {
            return JobStatus::Failed;
        }
        if self.outputs.iter().any(|o| o.status == JobStatus::Stopped) {
            return JobStatus::Stopped;
        }
        if self
            .outputs
            .iter()
            .any(|o| o.status == JobStatus::DoneWithWarnings)
        {
            return JobStatus::DoneWithWarnings;
        }
        if self.outputs.iter().all(|o| o.status == JobStatus::Skipped) {
            return JobStatus::Skipped;
        }
        JobStatus::Done
    }
}

/// Preferências dedicadas do Neoveg Encoder (§Preferências do manual).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EncoderPreferences {
    #[serde(default)]
    pub auto_start_idle_secs: Option<u32>,
    #[serde(default = "yes")]
    pub show_elapsed_time: bool,
    #[serde(default = "yes")]
    pub preview_during_encoding: bool,
    #[serde(default)]
    pub play_chime_on_finish: bool,
    #[serde(default)]
    pub remove_completed_on_exit: bool,
    #[serde(default = "yes")]
    pub increment_existing_output: bool,
    #[serde(default)]
    pub default_output_dir: Option<PathBuf>,
    #[serde(default = "default_seq_fps")]
    pub undefined_media_fps: FrameRate,
    #[serde(default = "default_preset_id")]
    pub default_preset_id: String,
    #[serde(default = "yes")]
    pub auto_encode_watch_folders: bool,
}

fn default_seq_fps() -> FrameRate {
    FrameRate::FPS_25
}
fn default_preset_id() -> String {
    "sys.web_1080p_hq".to_string()
}

impl Default for EncoderPreferences {
    fn default() -> Self {
        Self {
            auto_start_idle_secs: None,
            show_elapsed_time: true,
            preview_during_encoding: true,
            play_chime_on_finish: false,
            remove_completed_on_exit: false,
            increment_existing_output: true,
            default_output_dir: None,
            undefined_media_fps: FrameRate::FPS_25,
            default_preset_id: default_preset_id(),
            auto_encode_watch_folders: true,
        }
    }
}

impl EncoderPreferences {
    pub fn sanitize(&mut self) {
        if let Some(s) = self.auto_start_idle_secs {
            self.auto_start_idle_secs = Some(s.clamp(5, 3600));
        }
        if self.undefined_media_fps.num == 0 || self.undefined_media_fps.den == 0 {
            self.undefined_media_fps = FrameRate::FPS_25;
        }
        if self.default_preset_id.trim().is_empty() {
            self.default_preset_id = default_preset_id();
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EncoderState {
    #[serde(default)]
    pub items: Vec<QueueItem>,
    #[serde(default)]
    pub watch_folders: Vec<WatchFolder>,
    #[serde(default)]
    pub prefs: EncoderPreferences,
}

impl EncoderState {
    pub fn sanitize(&mut self) {
        self.prefs.sanitize();
        for item in &mut self.items {
            if item.id == 0 {
                item.id = new_id();
            }
            for out in &mut item.outputs {
                if out.id == 0 {
                    out.id = new_id();
                }
                if out.status == JobStatus::Encoding {
                    // Sessão interrompida anteriormente volta a Pronto (§0).
                    out.status = JobStatus::Ready;
                    out.progress = 0.0;
                }
                out.width = out.width.clamp(2, crate::model::MAX_DIM);
                out.height = out.height.clamp(2, crate::model::MAX_DIM);
            }
        }
        for wf in &mut self.watch_folders {
            if wf.id == 0 {
                wf.id = new_id();
            }
        }
    }

    pub fn add_media_file(&mut self, path: PathBuf, catalog: &PresetCatalog) -> Result<u64> {
        if !path.is_file() {
            bail!("arquivo não encontrado: {}", path.display());
        }
        let preset = catalog
            .resolve(&self.prefs.default_preset_id)
            .cloned()
            .unwrap_or_else(|| presets::system_presets()[0].clone());
        let src = QueueSource::MediaFile {
            path,
            is_sequence: false,
            sequence_fps: None,
            interpret: MediaInterpretation::default(),
        };
        let out = OutputJob::from_preset(
            &preset,
            &src,
            self.prefs.default_output_dir.as_deref(),
            (1920, 1080),
        );
        let id = new_id();
        self.items.push(QueueItem {
            id,
            source: src,
            outputs: vec![out],
            expanded: true,
            watch_folder_id: None,
        });
        Ok(id)
    }

    pub fn add_image_sequence(
        &mut self,
        first_path: PathBuf,
        fps: FrameRate,
        catalog: &PresetCatalog,
    ) -> Result<u64> {
        if !first_path.is_file() {
            bail!("sequência não encontrada: {}", first_path.display());
        }
        let preset = catalog
            .resolve(&self.prefs.default_preset_id)
            .cloned()
            .unwrap_or_else(|| presets::system_presets()[0].clone());
        let src = QueueSource::MediaFile {
            path: first_path,
            is_sequence: true,
            sequence_fps: Some(fps),
            interpret: MediaInterpretation::default(),
        };
        let mut out = OutputJob::from_preset(
            &preset,
            &src,
            self.prefs.default_output_dir.as_deref(),
            (1920, 1080),
        );
        out.fps_override = Some(fps);
        let id = new_id();
        self.items.push(QueueItem {
            id,
            source: src,
            outputs: vec![out],
            expanded: true,
            watch_folder_id: None,
        });
        Ok(id)
    }

    pub fn add_project_snapshot(
        &mut self,
        title: String,
        project_path: Option<PathBuf>,
        project: Project,
        in_out: Option<(Ticks, Ticks)>,
        catalog: &PresetCatalog,
    ) -> u64 {
        let dims = (
            project.settings.width.max(2),
            project.settings.height.max(2),
        );
        let preset = catalog
            .resolve(&self.prefs.default_preset_id)
            .cloned()
            .unwrap_or_else(|| presets::system_presets()[0].clone());
        let src = QueueSource::ProjectSnapshot {
            title,
            project_path,
            project: Box::new(project),
            in_point: in_out.map(|x| x.0),
            out_point: in_out.map(|x| x.1),
        };
        let mut out = OutputJob::from_preset(
            &preset,
            &src,
            self.prefs.default_output_dir.as_deref(),
            dims,
        );
        if in_out.is_some() {
            out.range = SourceRangeMode::InOut;
        }
        let id = new_id();
        self.items.push(QueueItem {
            id,
            source: src,
            outputs: vec![out],
            expanded: true,
            watch_folder_id: None,
        });
        id
    }

    pub fn add_output(&mut self, item_id: u64, preset: &presets::EncoderPreset) -> Option<u64> {
        let item = self.items.iter_mut().find(|i| i.id == item_id)?;
        let hint = match &item.source {
            QueueSource::ProjectSnapshot { project, .. } => (
                project.settings.width.max(2),
                project.settings.height.max(2),
            ),
            _ => (1920, 1080),
        };
        let out = OutputJob::from_preset(
            preset,
            &item.source,
            self.prefs.default_output_dir.as_deref(),
            hint,
        );
        let oid = out.id;
        item.outputs.push(out);
        item.expanded = true;
        Some(oid)
    }

    pub fn apply_preset_or_group(
        &mut self,
        item_id: u64,
        catalog: &PresetCatalog,
        preset_or_group_id: &str,
        add_as_new_outputs: bool,
    ) -> usize {
        let resolved = catalog.resolve_group_or_preset(preset_or_group_id);
        if resolved.is_empty() {
            return 0;
        }
        let Some(item) = self.items.iter_mut().find(|i| i.id == item_id) else {
            return 0;
        };
        let hint = match &item.source {
            QueueSource::ProjectSnapshot { project, .. } => (
                project.settings.width.max(2),
                project.settings.height.max(2),
            ),
            _ => (1920, 1080),
        };
        if !add_as_new_outputs && resolved.len() == 1 {
            if let Some(first) = item.outputs.first_mut() {
                if first.status != JobStatus::Encoding {
                    first.apply_preset(&resolved[0], hint);
                    first.status = JobStatus::Ready;
                    first.progress = 0.0;
                    return 1;
                }
            }
        }
        let mut added = 0;
        for preset in resolved {
            let out = OutputJob::from_preset(
                &preset,
                &item.source,
                self.prefs.default_output_dir.as_deref(),
                hint,
            );
            item.outputs.push(out);
            added += 1;
        }
        item.expanded = true;
        added
    }

    pub fn duplicate_item(&mut self, item_id: u64) -> Option<u64> {
        let idx = self.items.iter().position(|i| i.id == item_id)?;
        let mut copy = self.items[idx].clone();
        copy.id = new_id();
        for o in &mut copy.outputs {
            o.id = new_id();
            o.status = JobStatus::Ready;
            o.progress = 0.0;
            o.elapsed_secs = 0.0;
            o.eta_secs = None;
            o.last_message = None;
            o.warnings.clear();
        }
        let cid = copy.id;
        self.items.insert(idx + 1, copy);
        Some(cid)
    }

    pub fn duplicate_output(&mut self, item_id: u64, out_id: u64) -> Option<u64> {
        let item = self.items.iter_mut().find(|i| i.id == item_id)?;
        let idx = item.outputs.iter().position(|o| o.id == out_id)?;
        let mut copy = item.outputs[idx].clone();
        copy.id = new_id();
        copy.status = JobStatus::Ready;
        copy.progress = 0.0;
        copy.elapsed_secs = 0.0;
        copy.eta_secs = None;
        copy.last_message = None;
        copy.warnings.clear();
        let oid = copy.id;
        item.outputs.insert(idx + 1, copy);
        Some(oid)
    }

    pub fn remove_item(&mut self, item_id: u64) -> bool {
        let before = self.items.len();
        self.items
            .retain(|i| i.id != item_id || i.aggregate_status() == JobStatus::Encoding);
        self.items.len() != before
    }

    pub fn remove_output(&mut self, item_id: u64, out_id: u64) -> bool {
        let Some(item) = self.items.iter_mut().find(|i| i.id == item_id) else {
            return false;
        };
        let before = item.outputs.len();
        item.outputs
            .retain(|o| o.id != out_id || o.status == JobStatus::Encoding);
        let changed = item.outputs.len() != before;
        let empty = item.outputs.is_empty();
        if empty {
            self.items.retain(|i| i.id != item_id);
        }
        changed
    }

    pub fn move_item(&mut self, item_id: u64, delta: isize) -> bool {
        let Some(idx) = self.items.iter().position(|i| i.id == item_id) else {
            return false;
        };
        let target = idx.saturating_add_signed(delta).min(self.items.len() - 1);
        if target == idx {
            return false;
        }
        self.items.swap(idx, target);
        true
    }

    pub fn set_skipped(&mut self, item_id: u64, out_id: Option<u64>, skip: bool) {
        let Some(item) = self.items.iter_mut().find(|i| i.id == item_id) else {
            return;
        };
        for o in &mut item.outputs {
            if out_id.is_some_and(|id| o.id != id) {
                continue;
            }
            if o.status == JobStatus::Encoding {
                continue;
            }
            if skip {
                o.status = JobStatus::Skipped;
            } else if o.status == JobStatus::Skipped {
                o.status = JobStatus::Ready;
            }
        }
    }

    pub fn reset_status(&mut self, item_id: u64, out_id: Option<u64>) {
        let Some(item) = self.items.iter_mut().find(|i| i.id == item_id) else {
            return;
        };
        for o in &mut item.outputs {
            if out_id.is_some_and(|id| o.id != id) {
                continue;
            }
            if o.status != JobStatus::Encoding {
                o.status = JobStatus::Ready;
                o.progress = 0.0;
                o.elapsed_secs = 0.0;
                o.eta_secs = None;
                o.last_message = None;
                o.warnings.clear();
            }
        }
    }

    pub fn clear_completed(&mut self) {
        self.items.retain(|i| {
            i.outputs
                .iter()
                .any(|o| !matches!(o.status, JobStatus::Done | JobStatus::DoneWithWarnings))
        });
    }
}

pub fn encoder_dir() -> PathBuf {
    crate::util::base_dir().join("encoder")
}

pub fn queue_store_path() -> PathBuf {
    encoder_dir().join("queue.json")
}

pub fn presets_store_path() -> PathBuf {
    encoder_dir().join("presets.json")
}

pub fn presets_user_dir() -> PathBuf {
    encoder_dir().join("presets")
}

pub fn encoder_log_paths() -> (PathBuf, PathBuf) {
    let logs = crate::util::base_dir().join("logs");
    (
        logs.join("neoveg_encoder.log"),
        logs.join("neoveg_encoder_errors.log"),
    )
}

pub fn load_state(path: &Path) -> EncoderState {
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() <= MAX_STATE_BYTES {
            if let Ok(txt) = std::fs::read_to_string(path) {
                if let Ok(mut st) = serde_json::from_str::<EncoderState>(&txt) {
                    st.sanitize();
                    return st;
                }
            }
        }
    }
    EncoderState::default()
}

pub fn save_state(state: &EncoderState, path: &Path) -> Result<()> {
    let mut to_save = state.clone();
    if to_save.prefs.remove_completed_on_exit {
        to_save.clear_completed();
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(&to_save)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Garante um caminho único (`nome_1.ext`, `nome_2.ext`) quando anti-sobrescrita
/// está ativado ou duas saídas paralelas apontam para o mesmo destino.
pub fn resolve_unique_output_path(
    base: &Path,
    increment_existing: bool,
    reserved: &mut std::collections::HashSet<PathBuf>,
) -> PathBuf {
    let mut candidate = base.to_path_buf();
    let needs_check = |p: &Path, res: &std::collections::HashSet<PathBuf>| {
        res.contains(p) || (increment_existing && p.exists())
    };
    if !needs_check(&candidate, reserved) {
        reserved.insert(candidate.clone());
        return candidate;
    }
    let dir = base
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let stem = base
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "saida".to_string());
    let ext = base
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    for idx in 1..=9999 {
        let fname = if ext.is_empty() {
            format!("{stem}_{idx}")
        } else {
            format!("{stem}_{idx}.{ext}")
        };
        candidate = dir.join(fname);
        if !needs_check(&candidate, reserved) {
            reserved.insert(candidate.clone());
            return candidate;
        }
    }
    reserved.insert(candidate.clone());
    candidate
}

fn default_renders_dir() -> PathBuf {
    crate::util::base_dir().join("renders")
}

fn sanitize_stem(raw: &str) -> String {
    let s: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = s.trim_matches('_');
    if trimmed.is_empty() {
        "saida".to_string()
    } else {
        trimmed.chars().take(48).collect()
    }
}
