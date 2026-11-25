//! Motor de edição do Neoveg: estado da timeline, seleção, snapping, regras
//! de edição, mixer multifaixa, DSP nativo, composição de vídeo, áudio,
//! renderização, persistência `.neoveg` e Tabela Única de Comandos.
//!
//! Este crate não depende de `egui`, `eframe` ou `winit` (AGENTS.md §3.2).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub use neoveg_common::{curve, ease, error, prefs, session_marker, time, util};

#[forbid(unsafe_code)]
pub mod aaf;
#[forbid(unsafe_code)]
mod aaf_metadict;
#[forbid(unsafe_code)]
pub mod archive;
#[forbid(unsafe_code)]
pub mod color;
#[forbid(unsafe_code)]
pub mod commands;
#[forbid(unsafe_code)]
pub mod crossfade;
#[forbid(unsafe_code)]
pub mod edl;
#[forbid(unsafe_code)]
pub mod encoder;
#[forbid(unsafe_code)]
pub mod engine;
pub mod env_lock;
#[forbid(unsafe_code)]
pub mod fcp7xml;
#[forbid(unsafe_code)]
pub mod fcpxml;
#[forbid(unsafe_code)]
pub mod integrations;
#[forbid(unsafe_code)]
pub mod mask;
pub mod media;
#[forbid(unsafe_code)]
pub mod midi;
#[forbid(unsafe_code)]
pub mod model;
#[forbid(unsafe_code)]
pub mod otio;
#[forbid(unsafe_code)]
pub mod package;
#[forbid(unsafe_code)]
pub mod playback;
/// Fronteira FFI isolada de plug-ins externos (`AGENTS.md` §0.3).
pub mod plugin_host;
#[forbid(unsafe_code)]
pub mod plugins;
#[forbid(unsafe_code)]
pub mod recovery;
#[forbid(unsafe_code)]
pub mod render;
#[forbid(unsafe_code)]
pub mod render_status;
#[forbid(unsafe_code)]
pub mod scopes;
#[forbid(unsafe_code)]
pub mod session;
#[forbid(unsafe_code)]
pub mod slideshow;
#[forbid(unsafe_code)]
pub mod speech;
#[forbid(unsafe_code)]
pub mod store;
#[forbid(unsafe_code)]
pub mod subtitles;
#[forbid(unsafe_code)]
pub mod takes;
#[forbid(unsafe_code)]
pub mod track_keyframes;

#[cfg(test)]
#[forbid(unsafe_code)]
mod fuzz_tests;
