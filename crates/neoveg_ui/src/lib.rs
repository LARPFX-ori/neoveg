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
mod dock_drop;
mod edit;
mod edit_details;
mod encoder_panel;
mod float_dock;
pub mod fonts;
mod fx_browser;
mod gen_browser;
mod icons;
pub mod integrations_ui;
mod kf_window;
pub mod layout;
mod pan_crop_geom;
mod pan_crop_window;
mod panels;
mod plugin_manager;
mod preview_overlays;
mod project_media;
mod script_forms;
mod scrollbars;
mod splash;
mod surround_panel;
pub mod theme;
mod timeline;
mod track_motion_window;
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

/// Tamanho da janela principal depois da tela de abertura.
pub(crate) const MAIN_WINDOW_SIZE: [f32; 2] = [1480.0, 900.0];
pub(crate) const MAIN_WINDOW_MIN: [f32; 2] = [900.0, 600.0];

/// Com `splash_window`, a janela começa do tamanho da tela de abertura, sem
/// bordas e centralizada: a tela de abertura aparece sozinha e a janela do
/// programa só surge depois de carregar, como no VEGAS Pro.
fn native_options(renderer: eframe::Renderer, splash_window: bool) -> eframe::NativeOptions {
    let viewport = egui::ViewportBuilder::default()
        .with_title("Neoveg")
        .with_icon(std::sync::Arc::new(crate::splash::window_icon_data()));
    let viewport = if splash_window {
        viewport
            .with_inner_size(crate::splash::SPLASH_WINDOW_SIZE)
            .with_decorations(false)
            .with_resizable(false)
    } else {
        viewport
            .with_inner_size(MAIN_WINDOW_SIZE)
            .with_min_inner_size(MAIN_WINDOW_MIN)
    };
    eframe::NativeOptions {
        renderer,
        viewport,
        centered: true,
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
    run_gui_in(open, crash_reports, None)
}

/// Versão de [`run_gui`] com idioma inicial explícito (por exemplo `--lang` na CLI).
pub fn run_gui_in(
    open: Option<PathBuf>,
    crash_reports: Vec<neoveg_engine::session_marker::CrashReport>,
    initial_lang: Option<neoveg_common::Language>,
) -> eframe::Result<()> {
    let open_wgpu = open.clone();
    let crash_wgpu = crash_reports.clone();
    let splash_window = Preferences::load_or_default().show_splash;
    match eframe::run_native(
        "Neoveg",
        native_options(eframe::Renderer::Wgpu, splash_window),
        Box::new(move |cc| {
            let mut app = App::with_context_in(&cc.egui_ctx, open_wgpu.clone(), initial_lang);
            app.set_crash_reports(crash_wgpu.clone());
            app.set_startup_splash_window(splash_window);
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
                native_options(eframe::Renderer::Glow, splash_window),
                Box::new(move |cc| {
                    let mut app = App::with_context_in(&cc.egui_ctx, open.clone(), initial_lang);
                    app.set_crash_reports(crash_reports.clone());
                    app.set_startup_splash_window(splash_window);
                    let lang = app.lang();
                    app.set_status(neoveg_common::tr(
                        lang,
                        "Modo gráfico alternativo (OpenGL/software): WGPU indisponível",
                    ));
                    Ok(Box::new(app))
                }),
            )
        }
    }
}
