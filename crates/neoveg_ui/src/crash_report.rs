//! Janela "O Neoveg não foi fechado normalmente", mostrada na abertura
//! seguinte a uma sessão que caiu (pedido do usuário; não está nos manuais).
//!
//! A detecção fica em `neoveg_common::session`: cada sessão da interface
//! mantém um marcador travado, e o gancho de pânico grava nele. Aqui só se
//! mostra o que se sabe, oferece a recuperação do projeto (se houver) e
//! permite copiar o relatório ou abrir a pasta de logs.

use crate::app::App;
use eframe::egui;
use neoveg_common::util;
use neoveg_engine::session_marker::CrashReport;
use std::path::Path;

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
        self.splash.dismiss();
        self.welcome_open = false;
        self.pending_welcome_after_splash = false;
    }

    pub(crate) fn crash_report_window(&mut self, ctx: &egui::Context) {
        let Some(first) = self.crash_reports.first().cloned() else {
            return;
        };
        let others = self.crash_reports.len().saturating_sub(1);
        let mut close = false;
        let mut recover = None;
        let mut copy = false;
        let mut open_logs = false;
        let recovery_offer = self.recovery_offer;
        egui::Window::new("O Neoveg não foi fechado normalmente")
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(first.summary()).strong());
                ui.add_space(4.0);
                ui.label(format!(
                    "Sessão iniciada em {} (versão {}).",
                    first.started, first.version
                ));
                if let Some(p) = first.fatal_panic() {
                    ui.label(egui::RichText::new(format!("Local: {}", p.location)).small());
                }
                let isolated: Vec<_> = first.isolated_panics().cloned().collect();
                if !isolated.is_empty() {
                    egui::CollapsingHeader::new(format!(
                        "Erros internos isolados nesta sessão ({})",
                        isolated.len()
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
                    ui.label(
                        egui::RichText::new(format!(
                            "Outras {others} sessão(ões) também não fecharam normalmente \
                             (detalhes em logs/neoveg.log)."
                        ))
                        .small(),
                    );
                }
                ui.separator();
                if recovery_offer {
                    ui.label("Há um projeto salvo automaticamente dessa sessão.");
                    ui.horizontal(|ui| {
                        if ui.button("Recuperar projeto").clicked() {
                            recover = Some(true);
                        }
                        if ui.button("Descartar cópia").clicked() {
                            recover = Some(false);
                        }
                    });
                    ui.separator();
                }
                ui.horizontal(|ui| {
                    if ui
                        .button("Copiar relatório")
                        .on_hover_text("Copia o resumo para colar numa mensagem ou issue")
                        .clicked()
                    {
                        copy = true;
                    }
                    if ui.button("Abrir pasta de logs").clicked() {
                        open_logs = true;
                    }
                    if ui.button("Fechar").clicked() {
                        close = true;
                    }
                });
            });
        if copy {
            ctx.copy_text(first.report_text());
            self.set_status("Relatório copiado para a área de transferência");
        }
        if open_logs {
            let dir = util::base_dir().join("logs");
            if let Err(e) = open_folder(&dir) {
                self.set_status(format!("Não foi possível abrir {}: {e}", dir.display()));
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
        }
    }
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
