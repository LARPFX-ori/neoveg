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

pub use neoveg_common::{curve, error, prefs, time, util};

pub mod commands;
pub mod engine;
pub mod media;
pub mod model;
pub mod playback;
pub mod recovery;
pub mod render;
pub mod scopes;
pub mod session;

#[cfg(test)]
mod fuzz_tests;
