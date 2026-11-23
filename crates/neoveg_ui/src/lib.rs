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
pub mod cursors;
mod dialog_window;
mod dialogs;
mod dlg_work_folders;
mod doc_tabs;
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
mod preview_toolbar;
mod project_media;
mod script_forms;
mod scrollbars;
mod splash;
mod surround_panel;
pub mod theme;
mod timeline;
mod track_motion_window;
mod transcript_panel;
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
pub use crash_report::run_crash_dialog;
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
        // Linux: classe da janela (Wayland `app_id`, X11 `WM_CLASS`) igual ao
        // atalho `neoveg.desktop` criado pelo instalador, para o painel mostrar
        // o ícone e o nome certos
        .with_app_id("neoveg")
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
/// Ícone das janelas do Neoveg (desenhado em código), também usado pelo
/// instalador.
pub fn window_icon() -> egui::IconData {
    crate::splash::window_icon_data()
}

/// O ícone do Neoveg com `side` x `side` pixels (de 16 a 1024), para atalhos
/// e menus de aplicativos, que mostram o ícone maior que o da janela.
pub fn app_icon(side: u32) -> egui::IconData {
    crate::splash::icon_data_sized(side)
}

pub fn run_gui(
    open: Option<PathBuf>,
    crash_reports: Vec<neoveg_engine::session_marker::CrashReport>,
) -> eframe::Result<()> {
    run_gui_in(open, crash_reports, None)
}

struct GuiActiveGuard;

impl GuiActiveGuard {
    fn enter() -> Self {
        util::set_gui_active(true);
        Self
    }
}

impl Drop for GuiActiveGuard {
    fn drop(&mut self) {
        util::set_gui_active(false);
    }
}

/// Versão de [`run_gui`] com idioma inicial explícito (por exemplo `--lang` na CLI).
pub fn run_gui_in(
    open: Option<PathBuf>,
    crash_reports: Vec<neoveg_engine::session_marker::CrashReport>,
    initial_lang: Option<neoveg_common::Language>,
) -> eframe::Result<()> {
    let _gui_guard = GuiActiveGuard::enter();
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
