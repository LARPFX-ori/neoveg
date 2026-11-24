//! Diálogos de *Salvamento Avançado (Live Save & Backups Automáticos)*,
//! *Restaurar Backup* e *Arquivar Projeto com Mídias* (VEGAS Pro 21/2026).

use crate::icons::{self, Icon};
use eframe::egui;
use neoveg_common::{tr, tr_args, AdvancedSavePrefs, BackupLocation};
use neoveg_engine::archive::{archive_project_to_dir, ArchiveOptions};
use neoveg_engine::recovery::{
    advanced_backup_dir, clear_advanced_backups, list_advanced_backups, write_advanced_backup,
    BackupEntry,
};
use std::path::PathBuf;

#[derive(Default)]
pub struct AdvancedSaveUiState {
    pub open: bool,
    pub draft: Option<AdvancedSavePrefs>,
    pub restore_open: bool,
    pub restore_list: Vec<BackupEntry>,
    pub restore_selected: Option<usize>,
    pub archive_open: bool,
    pub archive_dest: String,
    pub archive_only_used: bool,
}

fn fmt_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

pub fn show_advanced_save_windows(app: &mut crate::App, ctx: &egui::Context) {
    show_advanced_save_dialog(app, ctx);
    show_restore_backup_dialog(app, ctx);
    show_archive_project_dialog(app, ctx);
}

fn show_advanced_save_dialog(app: &mut crate::App, ctx: &egui::Context) {
    if !app.dlg.adv_save.open {
        return;
    }
    if app.dlg.adv_save.draft.is_none() {
        app.dlg.adv_save.draft = Some(app.prefs.advanced_save.clone());
    }
    let Some(mut draft) = app.dlg.adv_save.draft.take() else {
        app.dlg.adv_save.open = false;
        return;
    };

    let lang = app.lang();
    let theme = app.theme.clone();
    let backup_dir = advanced_backup_dir(&draft, app.path.as_deref());
    let current_backups = list_advanced_backups(&backup_dir, app.path.as_deref());
    let last_backup_label = current_backups
        .first()
        .map(|b| b.file_name.clone())
        .unwrap_or_else(|| tr(lang, "Nenhum").to_string());

    let mut open = true;
    let mut ok_clicked = false;
    let mut cancel_clicked = false;
    let mut open_restore = false;
    let mut backup_now = false;
    let mut clear_all = false;
    let mut clear_current = false;

    crate::dialog_window::DialogWindow::new(tr(lang, "Salvamento avançado e backups"))
        .id(egui::Id::new("advanced_save_win"))
        .open(&mut open)
        .resizable(true)
        .default_size([560.0, 390.0])
        .min_size([460.0, 340.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut draft.live_save, tr(lang, "Salvamento ao vivo (Live Save)"))
                    .on_hover_text(tr(
                        lang,
                        "Salva o projeto automaticamente poucos segundos após cada edição quando já existe arquivo em disco.",
                    ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr(lang, "Restaurar...")).clicked() {
                        open_restore = true;
                    }
                    if ui.button(tr(lang, "Criar backup agora")).clicked() {
                        backup_now = true;
                    }
                });
            });

            ui.add_space(4.0);
            ui.checkbox(
                &mut draft.use_advanced_backup,
                tr(lang, "Usar backup avançado automático"),
            );
            ui.separator();

            ui.add_enabled_ui(draft.use_advanced_backup, |ui| {
                egui::Grid::new("adv_save_intervals")
                    .num_columns(3)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.checkbox(
                            &mut draft.interval_mins_enabled,
                            tr(lang, "Intervalo de backup (minutos):"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut draft.interval_mins)
                                .range(1..=240)
                                .speed(1),
                        );
                        ui.label(tr(lang, "minutos"));
                        ui.end_row();

                        ui.checkbox(
                            &mut draft.interval_hours_enabled,
                            tr(lang, "Intervalo de backup (horas):"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut draft.interval_hours)
                                .range(1..=24)
                                .speed(1),
                        );
                        ui.label(tr(lang, "horas"));
                        ui.end_row();

                        ui.checkbox(
                            &mut draft.archive_daily_enabled,
                            tr(lang, "Arquivar backups diários:"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut draft.max_daily)
                                .range(1..=50)
                                .speed(1),
                        );
                        ui.label(tr(lang, "máx."));
                        ui.end_row();

                        ui.checkbox(
                            &mut draft.archive_realtime_enabled,
                            tr(lang, "Arquivar backups em tempo real:"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut draft.max_realtime)
                                .range(1..=50)
                                .speed(1),
                        );
                        ui.label(tr(lang, "máx."));
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.label(egui::RichText::new(tr(lang, "Local de salvamento:")).strong());
                ui.horizontal(|ui| {
                    for loc in BackupLocation::ALL {
                        ui.radio_value(&mut draft.location, loc, loc.label_in(lang));
                    }
                });
                ui.horizontal(|ui| {
                    let is_custom = draft.location == BackupLocation::CustomFolder;
                    if is_custom {
                        ui.add(
                            egui::TextEdit::singleline(&mut draft.custom_folder)
                                .desired_width(360.0),
                        );
                        if ui.button(tr(lang, "Procurar...")).clicked() {
                            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                draft.custom_folder = folder.to_string_lossy().into_owned();
                            }
                        }
                    } else {
                        let mut ro = backup_dir.to_string_lossy().into_owned();
                        ui.add_enabled(
                            false,
                            egui::TextEdit::singleline(&mut ro).desired_width(420.0),
                        );
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(tr(lang, "Limpar todos os backups")).clicked() {
                        clear_all = true;
                    }
                    if ui
                        .button(tr(lang, "Limpar backups do projeto atual"))
                        .clicked()
                    {
                        clear_current = true;
                    }
                });
            });

            ui.add_space(8.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(tr_args(
                        lang,
                        "Último backup do projeto atual: {name}",
                        &[("name", &last_backup_label)],
                    ))
                    .small()
                    .color(theme.text_muted),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr(lang, "Cancelar")).clicked() {
                        cancel_clicked = true;
                    }
                    if icons::icon_button(ui, Icon::LiveSave, Some(tr(lang, "OK")), false, true, &theme)
                        .clicked()
                    {
                        ok_clicked = true;
                    }
                });
            });
        });

    if backup_now {
        draft.sanitize();
        let dir = advanced_backup_dir(&draft, app.path.as_deref());
        match write_advanced_backup(
            &app.session.project,
            &dir,
            app.path.as_deref(),
            "auto",
            draft.max_daily,
        ) {
            Ok(p) => {
                let p_s = p.display().to_string();
                app.set_status(tr_args(lang, "Backup criado: {path}", &[("path", &p_s)]));
            }
            Err(e) => {
                app.set_status(tr_args(lang, "Erro ao criar backup: {err}", &[("err", &e)]));
            }
        }
    }
    if clear_all {
        let dir = advanced_backup_dir(&draft, app.path.as_deref());
        let n = clear_advanced_backups(&dir, None);
        let n_s = n.to_string();
        app.set_status(tr_args(lang, "{n} backup(s) removido(s).", &[("n", &n_s)]));
    }
    if clear_current {
        let dir = advanced_backup_dir(&draft, app.path.as_deref());
        let n = clear_advanced_backups(&dir, app.path.as_deref());
        let n_s = n.to_string();
        app.set_status(tr_args(lang, "{n} backup(s) removido(s).", &[("n", &n_s)]));
    }
    if open_restore {
        let dir = advanced_backup_dir(&draft, app.path.as_deref());
        app.dlg.adv_save.restore_list = list_advanced_backups(&dir, None);
        app.dlg.adv_save.restore_selected = if app.dlg.adv_save.restore_list.is_empty() {
            None
        } else {
            Some(0)
        };
        app.dlg.adv_save.restore_open = true;
    }

    if ok_clicked {
        draft.sanitize();
        app.prefs.advanced_save = draft;
        let _ = app.prefs.save();
        app.dlg.adv_save.open = false;
        app.set_status(tr(lang, "Configurações de salvamento avançado salvas."));
    } else if cancel_clicked || !open {
        app.dlg.adv_save.open = false;
    } else {
        app.dlg.adv_save.draft = Some(draft);
    }
}

fn show_restore_backup_dialog(app: &mut crate::App, ctx: &egui::Context) {
    if !app.dlg.adv_save.restore_open {
        return;
    }
    let lang = app.lang();
    let theme = app.theme.clone();
    let mut open = true;
    let mut restore_path: Option<PathBuf> = None;
    let mut close_clicked = false;

    crate::dialog_window::DialogWindow::new(tr(lang, "Restaurar backup"))
        .id(egui::Id::new("restore_backup_win"))
        .open(&mut open)
        .resizable(true)
        .default_size([540.0, 340.0])
        .min_size([420.0, 260.0])
        .show(ctx, |ui| {
            if app.dlg.adv_save.restore_list.is_empty() {
                ui.label(
                    egui::RichText::new(tr(lang, "Nenhum backup encontrado na pasta selecionada."))
                        .color(theme.text_muted),
                );
            } else {
                egui::ScrollArea::vertical()
                    .max_height(240.0)
                    .show(ui, |ui| {
                        for (i, entry) in app.dlg.adv_save.restore_list.iter().enumerate() {
                            let sel = app.dlg.adv_save.restore_selected == Some(i);
                            let line = format!(
                                "{}  [{}]  ({})",
                                entry.file_name,
                                tr(lang, entry.kind),
                                fmt_size(entry.size_bytes)
                            );
                            if ui.selectable_label(sel, line).clicked() {
                                app.dlg.adv_save.restore_selected = Some(i);
                            }
                        }
                    });
            }
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr(lang, "Fechar")).clicked() {
                    close_clicked = true;
                }
                let can_restore = app
                    .dlg
                    .adv_save
                    .restore_selected
                    .and_then(|i| app.dlg.adv_save.restore_list.get(i))
                    .is_some();
                if ui
                    .add_enabled(
                        can_restore,
                        egui::Button::new(tr(lang, "Restaurar selecionado")),
                    )
                    .clicked()
                {
                    restore_path = app
                        .dlg
                        .adv_save
                        .restore_selected
                        .and_then(|i| app.dlg.adv_save.restore_list.get(i))
                        .map(|e| e.path.clone());
                }
            });
        });

    if let Some(p) = restore_path {
        app.open_project_path(p);
        app.dlg.adv_save.restore_open = false;
        app.dlg.adv_save.open = false;
    } else if close_clicked || !open {
        app.dlg.adv_save.restore_open = false;
    }
}

fn show_archive_project_dialog(app: &mut crate::App, ctx: &egui::Context) {
    if !app.dlg.adv_save.archive_open {
        return;
    }
    let lang = app.lang();
    let theme = app.theme.clone();
    let mut open = true;
    let mut run_archive = false;
    let mut cancel = false;

    crate::dialog_window::DialogWindow::new(tr(lang, "Arquivar projeto com mídias"))
        .id(egui::Id::new("archive_project_win"))
        .open(&mut open)
        .resizable(true)
        .default_size([520.0, 210.0])
        .min_size([420.0, 180.0])
        .show(ctx, |ui| {
            ui.label(tr(
                lang,
                "Copia o projeto .neoveg e as mídias referenciadas para uma pasta autocontida com caminhos relativos.",
            ));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(tr(lang, "Pasta de destino:"));
                ui.add(
                    egui::TextEdit::singleline(&mut app.dlg.adv_save.archive_dest)
                        .desired_width(290.0),
                );
                if ui.button(tr(lang, "Procurar...")).clicked() {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        app.dlg.adv_save.archive_dest = folder.to_string_lossy().into_owned();
                    }
                }
            });
            ui.add_space(4.0);
            ui.checkbox(
                &mut app.dlg.adv_save.archive_only_used,
                tr(lang, "Incluir somente mídias usadas na timeline"),
            );
            ui.add_space(8.0);
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr(lang, "Cancelar")).clicked() {
                    cancel = true;
                }
                let can_run = !app.dlg.adv_save.archive_dest.trim().is_empty();
                if icons::icon_button(
                    ui,
                    Icon::ArchiveProject,
                    Some(tr(lang, "Arquivar")),
                    false,
                    can_run,
                    &theme,
                )
                .clicked()
                {
                    run_archive = true;
                }
            });
        });

    if run_archive {
        let dest = PathBuf::from(app.dlg.adv_save.archive_dest.trim());
        let proj_name = app
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("projeto")
            .to_string();
        match archive_project_to_dir(
            &app.session.project,
            &dest,
            &proj_name,
            ArchiveOptions {
                only_used_media: app.dlg.adv_save.archive_only_used,
                overwrite: true,
            },
        ) {
            Ok(rep) => {
                let n_s = rep.copied_files.to_string();
                let p_s = rep.project_file.display().to_string();
                app.set_status(tr_args(
                    lang,
                    "Projeto arquivado ({n} mídia(s)): {path}",
                    &[("n", &n_s), ("path", &p_s)],
                ));
                app.dlg.adv_save.archive_open = false;
            }
            Err(e) => {
                app.set_status(tr_args(
                    lang,
                    "Erro ao arquivar projeto: {err}",
                    &[("err", &e)],
                ));
            }
        }
    } else if cancel || !open {
        app.dlg.adv_save.archive_open = false;
    }
}
