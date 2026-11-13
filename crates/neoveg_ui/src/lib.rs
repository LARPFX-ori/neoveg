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
mod dialogs;
mod edit;
mod icons;
pub mod layout;
mod panels;
mod splash;
pub mod theme;
mod timeline;

#[cfg(test)]
mod tests;

pub(crate) use app::rgba_to_color_image;
pub use app::{
    App, DndPayload, DockTab, EditLayoutPrefs, FxTarget, LayoutType, Preferences, PreviewQuality,
    RenderJob, TimelineState,
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
            .with_title("Neoveg"),
        ..Default::default()
    }
}

/// Inicia a janela principal usando `wgpu`; caso não haja adaptador gráfico
/// compatível (§3.6), cai automaticamente para `glow` (OpenGL/software) e
/// exibe aviso na barra de status sem entrar em pânico.
pub fn run_gui(open: Option<PathBuf>) -> eframe::Result<()> {
    let open_wgpu = open.clone();
    match eframe::run_native(
        "Neoveg",
        native_options(eframe::Renderer::Wgpu),
        Box::new(move |cc| Ok(Box::new(App::new(cc, open_wgpu.clone())))),
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
                    app.set_status("Modo gráfico alternativo (OpenGL/software): WGPU indisponível");
                    Ok(Box::new(app))
                }),
            )
        }
    }
}
