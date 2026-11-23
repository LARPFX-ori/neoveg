//! Janela de relatório de erro ao travar (estilo diálogo clássico "parou de
//! funcionar" com detalhes do problema, descrição opcional do usuário, cópia,
//! salvamento de relatório e reinício), tanto imediata (`--crash-dialog`) quanto
//! na abertura seguinte via marcador de sessão (`neoveg_common::session_marker`).
//!
//! Todos os elementos visuais (incluindo o emblema de erro vetorial) são
//! desenhados originalmente em código (AGENTS.md §1) e todos os campos de log
//! passam por `util::redact` (Regra da Casa 10).

use crate::app::App;
use crate::theme::{self, ThemeColors, ThemeId};
use eframe::egui::{self, pos2, vec2, Color32, Rect, Stroke};
use neoveg_common::util::{self, CrashDialogPayload};
use neoveg_common::{tr, tr_args, Language};
use neoveg_engine::prefs::Preferences;
use neoveg_engine::session_marker::CrashReport;
use std::path::{Path, PathBuf};

/// Desenha o emblema circular de erro em código vetorial original (sem assets externos).
fn paint_crash_badge(ui: &mut egui::Ui, theme: &ThemeColors) {
    let (rect, _) = ui.allocate_exact_size(vec2(38.0, 38.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let center = rect.center();
    let radius = 17.0;
    let fill = if theme.high_contrast {
        theme.accent
    } else {
        Color32::from_rgb(198, 46, 46)
    };
    let fg = theme::contrast_on(fill);
    p.circle_filled(center, radius, fill);
    p.circle_stroke(center, radius, Stroke::new(1.5_f32, theme.border));
    // Ponto de exclamação centralizado
    let bar = Rect::from_center_size(pos2(center.x, center.y - 3.5), vec2(4.0, 12.5));
    p.rect_filled(bar, 1.5, fg);
    p.circle_filled(pos2(center.x, center.y + 7.5), 2.3, fg);
}

/// Abre o diálogo do sistema para salvar o relatório técnico em `.txt`.
fn save_report_dialog(lang: Language, content: &str) -> Option<Result<PathBuf, String>> {
    let path = rfd::FileDialog::new()
        .set_title(tr(lang, "Salvar relatório de problema"))
        .set_file_name("neoveg-crash-report.txt")
        .add_filter(tr(lang, "Relatório de texto"), &["txt", "log"])
        .save_file()?;
    Some(
        std::fs::write(&path, content)
            .map(|()| path)
            .map_err(|e| e.to_string()),
    )
}

/// Inicia uma nova instância do Neoveg (sem `--crash-dialog`).
fn relaunch_neoveg() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Abre uma pasta no gerenciador de arquivos do sistema. O `explorer` do
/// Windows devolve código 1 mesmo quando abre a pasta, então só a falha ao
/// iniciar o processo conta como erro.
fn open_folder(dir: &Path) -> Result<(), String> {
    let cmd = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(cmd)
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Diálogo imediato em processo separado (`neoveg --crash-dialog <json>`)
// ---------------------------------------------------------------------------

struct CrashDialogApp {
    payload: CrashDialogPayload,
    lang: Language,
    theme: ThemeColors,
    user_notes: String,
    details_open: bool,
    status: String,
    has_recovery: bool,
}

impl CrashDialogApp {
    fn from_path(ctx: &egui::Context, payload_path: &Path, initial_lang: Option<Language>) -> Self {
        let mut prefs = Preferences::load_or_default();
        if let Some(ov) = initial_lang {
            prefs.set_lang(ov);
        }
        let lang = prefs.lang();
        neoveg_common::set_active_language(lang);
        crate::fonts::configure_fonts(ctx, prefs.system_ui_font && !cfg!(test));
        let theme_id = ThemeId::from_pref(&prefs.theme);
        let theme = theme::apply_theme(ctx, theme_id);

        let payload = CrashDialogPayload::load_from_file(payload_path).unwrap_or_else(|_| {
            CrashDialogPayload {
                version: env!("CARGO_PKG_VERSION").to_string(),
                os: std::env::consts::OS.to_string(),
                arch: std::env::consts::ARCH.to_string(),
                message: tr(
                    lang,
                    "Nenhum detalhe técnico adicional foi gravado para este encerramento.",
                )
                .to_string(),
                log_dir: util::redact(&util::base_dir().join("logs").display().to_string()),
                ..Default::default()
            }
        });
        let has_recovery = payload.recovery_note.is_some()
            || neoveg_engine::recovery::find_existing_recovery().is_some();

        Self {
            payload,
            lang,
            theme,
            user_notes: String::new(),
            details_open: false,
            status: String::new(),
            has_recovery,
        }
    }

    fn full_report(&self) -> String {
        self.payload.format_problem_details(&self.user_notes)
    }

    fn draw_ui(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let mut toggle_details = false;
        let mut copy_now = false;
        let mut save_now = false;
        let mut open_logs = false;
        let mut restart_now = false;
        let mut close_now = false;

        egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(&ctx.style())
                    .fill(self.theme.window_fill)
                    .inner_margin(egui::Margin::same(14)),
            )
            .show(ctx, |ui| {
                // Cabeçalho com emblema de erro + título principal
                ui.horizontal(|ui| {
                    paint_crash_badge(ui, &self.theme);
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.label(
                            crate::fonts::semibold_text(tr(lang, "O Neoveg parou de funcionar"), 15.0)
                                .color(self.theme.text_primary),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new(tr(
                                lang,
                                "Um problema fez o programa parar de funcionar corretamente. Seus arquivos de projeto originais não foram alterados.",
                            ))
                            .color(self.theme.text_muted),
                        );
                    });
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);

                if self.has_recovery {
                    egui::Frame::new()
                        .fill(self.theme.panel_fill)
                        .stroke(Stroke::new(1.0_f32, self.theme.border))
                        .corner_radius(2.0)
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(tr(
                                    lang,
                                    "Cópia de recuperação automática salva: ao reiniciar, o Neoveg oferecerá restaurar seu projeto.",
                                ))
                                .color(self.theme.text_primary),
                            );
                        });
                    ui.add_space(6.0);
                }

                // Campo opcional clássico de descrição do usuário
                ui.label(
                    egui::RichText::new(tr(
                        lang,
                        "O que você estava fazendo antes do erro? (opcional)",
                    ))
                    .strong(),
                );
                ui.add_space(2.0);
                ui.add(
                    egui::TextEdit::multiline(&mut self.user_notes)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text(tr(
                            lang,
                            "Ex.: movendo uma trilha 3D, aplicando um efeito ou renderizando...",
                        )),
                );

                ui.add_space(8.0);
                let toggle_lbl = if self.details_open {
                    tr(lang, "Ocultar detalhes do problema")
                } else {
                    tr(lang, "Mostrar detalhes do problema")
                };
                if ui.button(toggle_lbl).clicked() {
                    toggle_details = true;
                }

                if self.details_open {
                    ui.add_space(6.0);
                    let mut details = self.full_report();
                    egui::Frame::new()
                        .fill(self.theme.extreme_bg)
                        .stroke(Stroke::new(1.0_f32, self.theme.border))
                        .inner_margin(egui::Margin::same(6))
                        .show(ui, |ui| {
                            egui::ScrollArea::both()
                                .id_salt("crash_dlg_details_scroll")
                                .max_height(210.0)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut details)
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(f32::INFINITY)
                                            .interactive(true),
                                    );
                                });
                        });
                }

                if !self.status.is_empty() {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(&self.status)
                            .small()
                            .color(self.theme.accent),
                    );
                }

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    if ui
                        .button(tr(lang, "Copiar relatório"))
                        .on_hover_text(tr(lang, "Copia o resumo para colar numa mensagem ou issue"))
                        .clicked()
                    {
                        copy_now = true;
                    }
                    if ui.button(tr(lang, "Salvar relatório...")).clicked() {
                        save_now = true;
                    }
                    if ui.button(tr(lang, "Abrir pasta de logs")).clicked() {
                        open_logs = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(tr(lang, "Fechar")).clicked() {
                            close_now = true;
                        }
                        if ui.button(tr(lang, "Reiniciar Neoveg")).clicked() {
                            restart_now = true;
                        }
                    });
                });
            });

        if toggle_details {
            self.details_open = !self.details_open;
            let h = if self.details_open { 600.0 } else { 370.0 };
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(580.0, h)));
        }
        if copy_now {
            ctx.copy_text(self.full_report());
            self.status = tr(lang, "Relatório copiado para a área de transferência").to_string();
        }
        if save_now {
            if let Some(res) = save_report_dialog(lang, &self.full_report()) {
                match res {
                    Ok(p) => {
                        let ps = p.display().to_string();
                        self.status = tr_args(lang, "Relatório salvo em {path}", &[("path", &ps)]);
                    }
                    Err(e) => {
                        self.status = tr_args(
                            lang,
                            "Não foi possível salvar o relatório: {err}",
                            &[("err", &e)],
                        );
                    }
                }
            }
        }
        if open_logs {
            let dir = util::base_dir().join("logs");
            if let Err(e) = open_folder(&dir) {
                let dir_s = dir.display().to_string();
                self.status = tr_args(
                    lang,
                    "Não foi possível abrir {path}: {err}",
                    &[("path", &dir_s), ("err", &e)],
                );
            }
        }
        if restart_now {
            match relaunch_neoveg() {
                Ok(()) => close_now = true,
                Err(e) => {
                    self.status = tr_args(
                        lang,
                        "Não foi possível reiniciar o Neoveg: {err}",
                        &[("err", &e)],
                    );
                }
            }
        }
        if close_now {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for CrashDialogApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.draw_ui(ctx);
    }
}

fn crash_native_options(renderer: eframe::Renderer, lang: Language) -> eframe::NativeOptions {
    let viewport = egui::ViewportBuilder::default()
        .with_title(tr(lang, "O Neoveg parou de funcionar"))
        .with_app_id("neoveg")
        .with_icon(std::sync::Arc::new(crate::splash::window_icon_data()))
        .with_inner_size([580.0, 370.0])
        .with_min_inner_size([480.0, 320.0])
        .with_resizable(true);
    eframe::NativeOptions {
        renderer,
        viewport,
        centered: true,
        ..Default::default()
    }
}

/// Abre a janela autônoma de relatório de problema (`neoveg --crash-dialog <json>`),
/// com fallback automático `Wgpu` -> `Glow`.
pub fn run_crash_dialog(payload_path: &Path, initial_lang: Option<Language>) -> eframe::Result<()> {
    let lang = initial_lang.unwrap_or_else(|| Preferences::load_or_default().lang());
    let path_wgpu = payload_path.to_path_buf();
    let path_glow = payload_path.to_path_buf();
    match eframe::run_native(
        "Neoveg Crash Report",
        crash_native_options(eframe::Renderer::Wgpu, lang),
        Box::new(move |cc| {
            Ok(Box::new(CrashDialogApp::from_path(
                &cc.egui_ctx,
                &path_wgpu,
                initial_lang,
            )))
        }),
    ) {
        Ok(()) => Ok(()),
        Err(_) => eframe::run_native(
            "Neoveg Crash Report",
            crash_native_options(eframe::Renderer::Glow, lang),
            Box::new(move |cc| {
                Ok(Box::new(CrashDialogApp::from_path(
                    &cc.egui_ctx,
                    &path_glow,
                    initial_lang,
                )))
            }),
        ),
    }
}

// ---------------------------------------------------------------------------
// Janela exibida na abertura seguinte (via marcador de sessão)
// ---------------------------------------------------------------------------

fn build_session_report_text(first: &CrashReport, user_notes: &str) -> String {
    let mut text = first.report_text();
    let notes = user_notes.trim();
    if !notes.is_empty() {
        text.push_str("\nDescrição do usuário:\n");
        text.push_str(&util::redact(notes));
        text.push('\n');
    }
    if let Ok(last) = CrashDialogPayload::load_from_file(&util::last_crash_payload_path()) {
        if !last.backtrace.trim().is_empty()
            && first
                .fatal_panic()
                .is_some_and(|p| p.message == last.message || p.location == last.location)
        {
            text.push_str("\nInformações Extras (Stack Trace):\n");
            text.push_str(&last.backtrace);
            if !last.backtrace.ends_with('\n') {
                text.push('\n');
            }
        }
    }
    text
}

impl App {
    /// Relatórios das sessões anteriores que não fecharam normalmente
    /// (chamado por `run_gui` depois de criar a janela). Com relatório, a
    /// tela de abertura e a de boas-vindas não aparecem.
    pub fn set_crash_reports(&mut self, reports: Vec<CrashReport>) {
        if reports.is_empty() {
            return;
        }
        for r in &reports {
            util::log_warn(&format!(
                "sessão anterior (iniciada em {}) não fechou normalmente: {}",
                r.started,
                r.summary()
            ));
        }
        self.crash_reports = reports;
        self.crash_report_notes.clear();
        self.crash_details_open = false;
        if neoveg_engine::session_marker::other_instance_active() {
            // outra instância aberta pode estar gravando em autosave/recuperacao.neoveg
            self.recovery_offer = false;
        }
        self.splash.dismiss();
        self.welcome_open = false;
        self.pending_welcome_after_splash = false;
    }

    pub(crate) fn crash_report_window(&mut self, ctx: &egui::Context) {
        let lang = self.lang();
        let Some(first) = self.crash_reports.first().cloned() else {
            return;
        };
        let others = self.crash_reports.len().saturating_sub(1);
        let mut close = false;
        let mut recover = None;
        let mut copy = false;
        let mut save = false;
        let mut open_logs = false;
        let recovery_offer = self.recovery_offer;
        let theme = self.theme.clone();
        let has_fatal = first.fatal_panic().is_some();
        let title = if has_fatal {
            tr(lang, "O Neoveg parou de funcionar")
        } else {
            tr(lang, "O Neoveg não foi fechado normalmente")
        };

        crate::dialog_window::DialogWindow::new(title)
            .id(egui::Id::new("crash_report_win"))
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    paint_crash_badge(ui, &theme);
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.label(
                            crate::fonts::semibold_text(title, 14.0).color(theme.text_primary),
                        );
                        ui.add_space(2.0);
                        ui.label(egui::RichText::new(first.summary()).strong());
                    });
                });
                ui.add_space(4.0);
                ui.label(tr_args(
                    lang,
                    "Sessão iniciada em {at} (versão {ver}).",
                    &[("at", &first.started), ("ver", &first.version)],
                ));
                if let Some(p) = first.fatal_panic() {
                    ui.label(
                        egui::RichText::new(tr_args(lang, "Local: {loc}", &[("loc", &p.location)]))
                            .small(),
                    );
                }
                let isolated: Vec<_> = first.isolated_panics().cloned().collect();
                if !isolated.is_empty() {
                    let count_s = isolated.len().to_string();
                    egui::CollapsingHeader::new(tr_args(
                        lang,
                        "Erros internos isolados nesta sessão ({n})",
                        &[("n", &count_s)],
                    ))
                    .default_open(false)
                    .show(ui, |ui| {
                        for p in &isolated {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} · {} · {}: {}",
                                    p.time, p.thread, p.location, p.message
                                ))
                                .small(),
                            );
                        }
                    });
                }
                if others > 0 {
                    let others_s = others.to_string();
                    ui.label(
                        egui::RichText::new(tr_args(
                            lang,
                            "Outras {n} sessão(ões) também não fecharam normalmente \
                             (detalhes em logs/neoveg.log).",
                            &[("n", &others_s)],
                        ))
                        .small(),
                    );
                }

                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(tr(
                        lang,
                        "O que você estava fazendo antes do erro? (opcional)",
                    ))
                    .small()
                    .strong(),
                );
                ui.add(
                    egui::TextEdit::multiline(&mut self.crash_report_notes)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .hint_text(tr(
                            lang,
                            "Ex.: movendo uma trilha 3D, aplicando um efeito ou renderizando...",
                        )),
                );

                ui.add_space(4.0);
                let toggle_lbl = if self.crash_details_open {
                    tr(lang, "Ocultar detalhes do problema")
                } else {
                    tr(lang, "Mostrar detalhes do problema")
                };
                if ui.button(toggle_lbl).clicked() {
                    self.crash_details_open = !self.crash_details_open;
                }
                if self.crash_details_open {
                    ui.add_space(4.0);
                    let mut details = build_session_report_text(&first, &self.crash_report_notes);
                    egui::Frame::new()
                        .fill(theme.extreme_bg)
                        .stroke(Stroke::new(1.0_f32, theme.border))
                        .inner_margin(egui::Margin::same(5))
                        .show(ui, |ui| {
                            egui::ScrollArea::both()
                                .id_salt("session_crash_details_scroll")
                                .max_height(160.0)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut details)
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(f32::INFINITY)
                                            .interactive(true),
                                    );
                                });
                        });
                }

                ui.separator();
                if recovery_offer {
                    ui.label(tr(
                        lang,
                        "Há um projeto salvo automaticamente dessa sessão.",
                    ));
                    ui.horizontal(|ui| {
                        if ui.button(tr(lang, "Recuperar projeto")).clicked() {
                            recover = Some(true);
                        }
                        if ui.button(tr(lang, "Descartar cópia")).clicked() {
                            recover = Some(false);
                        }
                    });
                    ui.separator();
                }
                ui.horizontal(|ui| {
                    if ui
                        .button(tr(lang, "Copiar relatório"))
                        .on_hover_text(tr(lang, "Copia o resumo para colar numa mensagem ou issue"))
                        .clicked()
                    {
                        copy = true;
                    }
                    if ui.button(tr(lang, "Salvar relatório...")).clicked() {
                        save = true;
                    }
                    if ui.button(tr(lang, "Abrir pasta de logs")).clicked() {
                        open_logs = true;
                    }
                    if ui.button(tr(lang, "Fechar")).clicked() {
                        close = true;
                    }
                });
            });
        if copy {
            ctx.copy_text(build_session_report_text(&first, &self.crash_report_notes));
            self.set_status(tr(lang, "Relatório copiado para a área de transferência"));
        }
        if save {
            let report = build_session_report_text(&first, &self.crash_report_notes);
            if let Some(res) = save_report_dialog(lang, &report) {
                match res {
                    Ok(p) => {
                        let ps = p.display().to_string();
                        self.set_status(tr_args(
                            lang,
                            "Relatório salvo em {path}",
                            &[("path", &ps)],
                        ));
                    }
                    Err(e) => {
                        self.set_status(tr_args(
                            lang,
                            "Não foi possível salvar o relatório: {err}",
                            &[("err", &e)],
                        ));
                    }
                }
            }
        }
        if open_logs {
            let dir = util::base_dir().join("logs");
            if let Err(e) = open_folder(&dir) {
                let dir_s = dir.display().to_string();
                self.set_status(tr_args(
                    lang,
                    "Não foi possível abrir {path}: {err}",
                    &[("path", &dir_s), ("err", &e)],
                ));
            }
        }
        if let Some(r) = recover {
            self.resolve_recovery(r);
            close = true;
        }
        if close {
            // a cópia de recuperação, se ainda houver, continua oferecida na
            // janela de recuperação: fechar o relatório não a descarta
            self.crash_reports.clear();
            self.crash_report_notes.clear();
            self.crash_details_open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_dialog_app_renderiza_com_payload_valido_e_ausente() {
        let dir = std::env::temp_dir().join(format!("neoveg_crash_dlg_ui_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("crash.json");
        let payload = CrashDialogPayload {
            version: "0.3.0".into(),
            os: "windows".into(),
            arch: "x86_64".into(),
            timestamp: "2026-11-23 15:10:00".into(),
            thread: "main".into(),
            location: "track_motion_window.rs:420".into(),
            message: "erro simulado".into(),
            backtrace: "0: neoveg_ui::update".into(),
            recovery_note: Some("autosave ok".into()),
            log_dir: "~/logs".into(),
        };
        payload.save_to_file(&file).unwrap();

        let ctx = egui::Context::default();
        let mut dlg = CrashDialogApp::from_path(&ctx, &file, Some(Language::PtBr));
        dlg.user_notes = "Arrastando o quadro 3D".into();
        dlg.details_open = true;
        let report = dlg.full_report();
        assert!(report.contains("erro simulado"));
        assert!(report.contains("Arrastando o quadro 3D"));
        assert!(report.contains("0: neoveg_ui::update"));
        let _ = ctx.run(egui::RawInput::default(), |c| dlg.draw_ui(c));

        // Caminho inexistente cai para fallback seguro sem entrar em pânico
        let mut missing =
            CrashDialogApp::from_path(&ctx, &dir.join("nao_existe.json"), Some(Language::En));
        let _ = ctx.run(egui::RawInput::default(), |c| missing.draw_ui(c));
        assert!(!missing.full_report().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
