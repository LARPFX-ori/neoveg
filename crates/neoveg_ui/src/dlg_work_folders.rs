//! Janela de configuração de pastas de trabalho do projeto (item 14 do roadmap).
//!
//! Permite ao usuário mapear caminhos específicos ou usar as subpastas padrão
//! relativas ao arquivo `.neoveg` (ou diretório base da aplicação se o projeto ainda não foi salvo).

use eframe::egui;
use neoveg_common::i18n::tr;
use std::path::Path;

pub fn show_work_folders_window(app: &mut crate::App, ctx: &egui::Context) {
    if !app.dlg.work_folders_open {
        return;
    }

    if app.dlg.work_folders_draft.is_none() {
        app.dlg.work_folders_draft = Some(app.session.project.work_folders.clone());
    }

    let Some(mut draft) = app.dlg.work_folders_draft.take() else {
        app.dlg.work_folders_open = false;
        return;
    };

    let lang = app.lang();
    let mut open = true;
    let mut save_close = false;
    let mut cancel_close = false;
    let mut restore_defaults = false;

    let proj_path = app.path.as_deref();
    let def_capturas = neoveg_engine::model::WorkFolders::default_captured_media(proj_path);
    let def_audio = neoveg_engine::model::WorkFolders::default_audio_recordings(
        proj_path,
        Some(&app.prefs.audio_device.default_record_dir),
    );
    let def_prerender = neoveg_engine::model::WorkFolders::default_prerendered_files(proj_path);
    let def_proxies = neoveg_engine::model::WorkFolders::default_proxies(proj_path);
    let def_renders = neoveg_engine::model::WorkFolders::default_render_exports(proj_path);

    egui::Window::new(tr(lang, "Pastas de trabalho"))
        .id(egui::Id::new("work_folders_window"))
        .open(&mut open)
        .resizable(true)
        .default_size([640.0, 420.0])
        .min_size([520.0, 320.0])
        .show(ctx, |ui| {
            ui.add_space(4.0);
            ui.label(tr(
                lang,
                "Defina onde este projeto armazena mídias capturadas, gravações, pré-renderizações, proxies e exportações:",
            ));
            ui.add_space(8.0);

            egui::ScrollArea::vertical().show(ui, |ui| {
                folder_row(
                    ui,
                    lang,
                    tr(lang, "Mídias capturadas / importadas:"),
                    &mut draft.captured_media,
                    &def_capturas.to_string_lossy(),
                );
                ui.add_space(6.0);
                folder_row(
                    ui,
                    lang,
                    tr(lang, "Gravações de áudio:"),
                    &mut draft.audio_recordings,
                    &def_audio.to_string_lossy(),
                );
                ui.add_space(6.0);
                folder_row(
                    ui,
                    lang,
                    tr(lang, "Trechos pré-renderizados:"),
                    &mut draft.prerendered_files,
                    &def_prerender.to_string_lossy(),
                );
                ui.add_space(6.0);
                folder_row(
                    ui,
                    lang,
                    tr(lang, "Proxies de vídeo:"),
                    &mut draft.proxies,
                    &def_proxies.to_string_lossy(),
                );
                ui.add_space(6.0);
                folder_row(
                    ui,
                    lang,
                    tr(lang, "Renderizações e exportações:"),
                    &mut draft.render_exports,
                    &def_renders.to_string_lossy(),
                );
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                if ui.button(tr(lang, "Restaurar padrões")).clicked() {
                    restore_defaults = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr(lang, "Cancelar")).clicked() {
                        cancel_close = true;
                    }
                    if ui.button(tr(lang, "Salvar")).clicked() {
                        save_close = true;
                    }
                });
            });
        });

    if restore_defaults {
        draft = neoveg_engine::model::WorkFolders::default();
    }

    if save_close {
        app.session.set_work_folders(draft);
        app.dlg.work_folders_open = false;
        app.dlg.work_folders_draft = None;
    } else if cancel_close || !open {
        app.dlg.work_folders_open = false;
        app.dlg.work_folders_draft = None;
    } else {
        app.dlg.work_folders_open = true;
        app.dlg.work_folders_draft = Some(draft);
    }
}

fn folder_row(
    ui: &mut egui::Ui,
    lang: neoveg_common::i18n::Language,
    label: &str,
    field: &mut String,
    def_path: &str,
) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(label).strong());
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(field)
                    .hint_text(def_path)
                    .desired_width(ui.available_width() - 90.0),
            );
            if ui.button(tr(lang, "Procurar...")).clicked() {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    *field = folder.to_string_lossy().to_string();
                }
            }
        });

        let trimmed = field.trim();
        let effective = if trimmed.is_empty() {
            def_path
        } else {
            trimmed
        };
        let p = Path::new(effective);
        if p.is_dir() {
            ui.label(
                egui::RichText::new(format!("✔ {}", tr(lang, "Pasta existente")))
                    .color(egui::Color32::from_rgb(80, 200, 100))
                    .small(),
            );
        } else {
            ui.label(
                egui::RichText::new(format!(
                    "ℹ {}",
                    tr(
                        lang,
                        "A pasta não existe no disco e será criada quando necessário"
                    )
                ))
                .color(egui::Color32::from_rgb(220, 180, 70))
                .small(),
            );
        }
    });
}
