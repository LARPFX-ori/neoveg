//! Interface gráfica do Neoveg (`eframe` + `wgpu` com fallback para `glow`).
//! Mantém layout, semântica, menus e atalhos inspirados no VEGAS Pro,
//! despachando todas as ações pela Tabela Única de Comandos (`neoveg_engine`).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod app;
mod color_grading;
mod convert_dialog;
mod crash_report;
mod dialogs;
mod dock;
mod edit;
mod edit_details;
mod encoder_panel;
mod fx_browser;
mod gen_browser;
mod icons;
pub mod layout;
mod panels;
mod plugin_manager;
mod scrollbars;
mod splash;
mod surround_panel;
pub mod theme;
mod timeline;
mod trimmer_panel;
mod welcome;

#[cfg(test)]
mod kittest_tests;
#[cfg(test)]
mod tests;

pub(crate) use app::{apply_pref_defaults, rgba_to_color_image};
pub use app::{
    App, DndPayload, DockTab, EditLayoutPrefs, FxTarget, LayoutType, PlaybackScrollMode,
    Preferences, PreviewQuality, RenderJob, TimelineMouseScroll, TimelineState,
};
pub use neoveg_engine::{media, model, playback, recovery, render, session, util};

use eframe::egui;
use std::path::PathBuf;

fn native_options(renderer: eframe::Renderer) -> eframe::NativeOptions {
    eframe::NativeOptions {
        renderer,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1480.0, 900.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("Neoveg")
            .with_icon(std::sync::Arc::new(crate::splash::window_icon_data())),
        ..Default::default()
    }
}

/// Inicia a janela principal usando `wgpu`; caso não haja adaptador gráfico
/// compatível (§3.6), cai automaticamente para `glow` (OpenGL/software) e
/// exibe aviso na barra de status sem entrar em pânico.
pub fn run_gui(
    open: Option<PathBuf>,
    crash_reports: Vec<neoveg_engine::session_marker::CrashReport>,
) -> eframe::Result<()> {
    let open_wgpu = open.clone();
    let crash_wgpu = crash_reports.clone();
    match eframe::run_native(
        "Neoveg",
        native_options(eframe::Renderer::Wgpu),
        Box::new(move |cc| {
            let mut app = App::new(cc, open_wgpu.clone());
            app.set_crash_reports(crash_wgpu.clone());
            Ok(Box::new(app))
        }),
    ) {
        Ok(()) => Ok(()),
        Err(wgpu_err) => {
            let warn =
                format!("Adaptador WGPU indisponível ({wgpu_err}); iniciando em OpenGL/software");
            util::log_warn(&warn);
            eframe::run_native(
                "Neoveg",
                native_options(eframe::Renderer::Glow),
                Box::new(move |cc| {
                    let mut app = App::new(cc, open.clone());
                    app.set_crash_reports(crash_reports.clone());
                    app.set_status("Modo gráfico alternativo (OpenGL/software): WGPU indisponível");
                    Ok(Box::new(app))
                }),
            )
        }
    }
}
