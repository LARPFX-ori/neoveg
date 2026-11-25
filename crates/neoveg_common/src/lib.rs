//! Tipos fundamentais, representação exata de tempo (ticks/amostras), curvas
//! de fade e keyframes, tipos de erro, preferências e utilitários de robustez.
//!
//! Este crate não possui dependências de interface gráfica, GPU ou áudio de SO
//! (AGENTS.md §3.2).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod curve;
pub mod ease;
pub mod error;
pub mod i18n;
pub mod prefs;
pub mod session_marker;
pub mod time;
pub mod util;

pub use curve::{bump_ids, new_id, peek_next_id, Curve, Fade, Id, Keyframe, Keyframes, Lerp};
pub use error::{NeovegError, Result};
pub use i18n::{active_language, set_active_language, tr, tr_args, tr_menu_path, Language};
pub use prefs::{
    recommended_worker_threads, AdvancedSavePrefs, BackupLocation, CutToOverlapAlign,
    EditLayoutPrefs, IconStyle, LayoutType, Preferences, PreviewQuality, WorkflowMode,
};
pub use time::{
    format_ticks, parse_time, smpte_display_frame_to_ticks, smpte_drop_params,
    ticks_to_smpte_display_frame, FrameRate, Rate, Ticks, TimeFormat, TICKS_PER_MS, TICKS_PER_SEC,
};
