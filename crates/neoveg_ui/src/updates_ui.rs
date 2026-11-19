//! Atualizações na interface: verificação automática no programa instalado
//! (uma vez por dia, desligável nas Preferências), Ajuda > Verificar
//! atualizações, o aviso com as novidades e o download do instalador.
//!
//! A instalação só começa quando o Neoveg fecha de verdade (`on_exit`): se o
//! usuário cancelar o fechamento (projeto não salvo), nada é instalado. O
//! instalador espera este processo terminar, instala por cima na mesma
//! pasta e reabre o Neoveg.

use super::App;
use eframe::egui;
use neoveg_common::{tr, tr_args, util};
use neoveg_engine::updates::{self, UpdateInfo};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// Estado das atualizações entre os quadros.
#[derive(Default)]
pub(crate) struct UpdateState {
    checking: Option<Receiver<Result<Option<UpdateInfo>, String>>>,
    /// A verificação foi pedida pelo menu: avisa também "nada novo" e erros.
    manual: bool,
    /// Verificação automática já considerada nesta sessão.
    auto_done: bool,
    found: Option<UpdateInfo>,
    open: bool,
    downloading: Option<Receiver<Result<PathBuf, String>>>,
    /// Instalador baixado, executado quando o Neoveg fechar.
    install_on_exit: Option<PathBuf>,
}

/// Como o programa foi instalado (lido do `instalado.json`): a atualização
/// repete as mesmas escolhas.
struct Installed {
    dir: PathBuf,
    all_users: bool,
    desktop_shortcut: bool,
    associate: bool,
}

/// O programa instalado (`instalado.json` ao lado do executável). `None` no
/// pacote portátil.
fn installed_info() -> Option<Installed> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let manifest = dir.join(util::INSTALL_MARKER);
    let meta = std::fs::metadata(&manifest).ok()?;
    if !meta.is_file() || meta.len() > (8 << 20) {
        return None;
    }
    let raw = std::fs::read_to_string(&manifest).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let all_users = v.get("escopo").and_then(|e| e.as_str()) == Some("AllUsers");
    let desktop_shortcut = v
        .get("atalhos")
        .and_then(|a| a.as_array())
        .is_some_and(|a| {
            a.iter()
                .filter_map(|p| p.as_str())
                .any(|p| p.to_ascii_lowercase().contains("desktop"))
        });
    let associate = v
        .get("associacoes")
        .and_then(|a| a.as_bool())
        .unwrap_or(false);
    Some(Installed {
        dir,
        all_users,
        desktop_shortcut,
        associate,
    })
}

/// Abre um endereço `https://` no navegador padrão.
fn open_in_browser(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("endereço inválido".into());
    }
    #[cfg(windows)]
    let r = std::process::Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    r.map(|_| ()).map_err(|e| e.to_string())
}

impl App {
    /// Começa a verificação em segundo plano. `manual`: pedida pelo menu.
    pub(crate) fn start_update_check(&mut self, manual: bool) {
        if self.update.checking.is_some() {
            return;
        }
        let prereleases = self.prefs.update_prereleases;
        let (tx, rx) = mpsc::channel();
        let spawned = util::spawn("atualizacoes", move || {
            let r = util::guard("verificação de atualizações", || {
                updates::check(prereleases)
            })
            .and_then(|r| r);
            let _ = tx.send(r);
        });
        match spawned {
            Ok(()) => {
                self.update.checking = Some(rx);
                self.update.manual = manual;
                if manual {
                    let lang = self.lang();
                    self.set_status(tr(lang, "Verificando atualizações..."));
                }
            }
            Err(e) => util::log_warn(&e),
        }
    }

    /// Verificação automática: só no programa instalado, uma vez por dia e
    /// se estiver ligada nas Preferências.
    pub(crate) fn maybe_auto_update_check(&mut self) {
        if self.update.auto_done || self.startup_window || self.splash.active {
            return;
        }
        self.update.auto_done = true;
        if !self.prefs.check_updates || installed_info().is_none() {
            return;
        }
        let (today, _) = util::local_date_time_strings();
        if self.prefs.update_last_check == today {
            return;
        }
        self.prefs.update_last_check = today;
        let _ = self.prefs.save();
        self.start_update_check(false);
    }

    /// Lê o resultado da verificação e do download.
    pub(crate) fn poll_updates(&mut self, ctx: &egui::Context) {
        let lang = self.lang();
        if let Some(rx) = &self.update.checking {
            match rx.try_recv() {
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(250));
                }
                Err(TryRecvError::Disconnected) => self.update.checking = None,
                Ok(r) => {
                    self.update.checking = None;
                    let manual = self.update.manual;
                    match r {
                        Ok(Some(info)) => {
                            let skipped =
                                info.version.to_string() == self.prefs.update_skip_version;
                            if manual || !skipped {
                                self.update.found = Some(info);
                                self.update.open = true;
                            }
                        }
                        Ok(None) => {
                            if manual {
                                self.set_status(tr_args(
                                    lang,
                                    "O Neoveg está atualizado (versão {ver}).",
                                    &[("ver", &updates::current_version())],
                                ));
                            }
                        }
                        Err(e) => {
                            util::log_warn(&format!("verificação de atualizações: {e}"));
                            if manual {
                                self.set_status(tr_args(
                                    lang,
                                    "Não foi possível verificar atualizações: {err}",
                                    &[("err", &e)],
                                ));
                            }
                        }
                    }
                }
            }
        }
        if let Some(rx) = &self.update.downloading {
            match rx.try_recv() {
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(250));
                }
                Err(TryRecvError::Disconnected) => self.update.downloading = None,
                Ok(Ok(path)) => {
                    self.update.downloading = None;
                    self.update.install_on_exit = Some(path);
                    self.update.open = false;
                    self.set_status(tr(
                        lang,
                        "Atualização baixada: o Neoveg fecha, instala a versão nova e abre de novo.",
                    ));
                    // o fechamento normal pergunta sobre projetos não salvos
                    ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
                }
                Ok(Err(e)) => {
                    self.update.downloading = None;
                    self.set_status(tr_args(
                        lang,
                        "Não foi possível baixar a atualização: {err}",
                        &[("err", &e)],
                    ));
                }
            }
        }
    }

    fn start_update_download(&mut self, info: UpdateInfo) {
        let (tx, rx) = mpsc::channel();
        let spawned = util::spawn("download-atualizacao", move || {
            let r = util::guard("download da atualização", || {
                updates::download_installer(&info)
            })
            .and_then(|r| r);
            let _ = tx.send(r);
        });
        match spawned {
            Ok(()) => self.update.downloading = Some(rx),
            Err(e) => self.set_status(e),
        }
    }

    /// Aviso "Atualização disponível".
    pub(crate) fn show_update_dialog(&mut self, ctx: &egui::Context) {
        if !self.update.open {
            return;
        }
        let Some(info) = self.update.found.clone() else {
            self.update.open = false;
            return;
        };
        let lang = self.lang();
        let installed = installed_info().is_some();
        let downloading = self.update.downloading.is_some();
        let mut open = true;
        let mut action: Option<&'static str> = None;
        crate::dialog_window::DialogWindow::new(tr(lang, "Atualização disponível"))
            .id(egui::Id::new("update_available_win"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.heading(tr_args(
                    lang,
                    "Neoveg {ver} está disponível",
                    &[("ver", &info.version)],
                ));
                ui.label(tr_args(
                    lang,
                    "Você usa a versão {cur}.",
                    &[("cur", &updates::current_version())],
                ));
                if info.prerelease {
                    ui.label(
                        egui::RichText::new(tr(lang, "Pré-lançamento: pode ter instabilidades."))
                            .color(ui.visuals().warn_fg_color),
                    );
                }
                ui.separator();
                ui.label(egui::RichText::new(tr(lang, "Novidades:")).strong());
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let notes = if info.notes.trim().is_empty() {
                            tr(lang, "(sem descrição)").to_string()
                        } else {
                            info.notes.clone()
                        };
                        ui.label(notes);
                    });
                ui.separator();
                if downloading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(tr(lang, "Baixando a atualização..."));
                    });
                } else if !installed {
                    ui.label(
                        egui::RichText::new(tr(
                            lang,
                            "Esta cópia é portátil: baixe a versão nova pela página do lançamento.",
                        ))
                        .weak(),
                    );
                } else if info.installer_url.is_none() {
                    ui.label(
                        egui::RichText::new(tr(
                            lang,
                            "Este lançamento ainda não tem instalador: veja a página do lançamento.",
                        ))
                        .weak(),
                    );
                }
                ui.horizontal(|ui| {
                    let can_install = installed && info.installer_url.is_some() && !downloading;
                    if ui
                        .add_enabled(can_install, egui::Button::new(tr(lang, "Baixar e instalar")))
                        .clicked()
                    {
                        action = Some("install");
                    }
                    if ui.button(tr(lang, "Ver página")).clicked() {
                        action = Some("page");
                    }
                    if ui.button(tr(lang, "Lembrar depois")).clicked() {
                        action = Some("later");
                    }
                    if ui.button(tr(lang, "Ignorar esta versão")).clicked() {
                        action = Some("skip");
                    }
                });
            });
        match action {
            Some("install") => self.start_update_download(info),
            Some("page") => {
                if let Err(e) = open_in_browser(&info.page_url) {
                    self.set_status(e);
                }
            }
            Some("later") => self.update.open = false,
            Some("skip") => {
                self.prefs.update_skip_version = info.version.to_string();
                let _ = self.prefs.save();
                self.update.open = false;
            }
            _ => {}
        }
        if !open {
            self.update.open = false;
        }
    }

    /// Chamado ao fechar: com uma atualização baixada, abre o instalador em
    /// modo silencioso. Ele espera este processo terminar, instala na mesma
    /// pasta e reabre o Neoveg.
    pub(crate) fn launch_pending_update(&mut self) {
        let Some(setup) = self.update.install_on_exit.take() else {
            return;
        };
        let Some(inst) = installed_info() else {
            return;
        };
        let mut cmd = std::process::Command::new(&setup);
        cmd.arg("--silent")
            .arg(if inst.all_users {
                "--all-users"
            } else {
                "--user"
            })
            .arg("--dir")
            .arg(&inst.dir)
            .arg("--wait-pid")
            .arg(std::process::id().to_string())
            .arg("--launch");
        if inst.desktop_shortcut {
            cmd.arg("--desktop");
        }
        if !inst.associate {
            cmd.arg("--no-assoc");
        }
        let r = cmd.spawn();
        match r {
            Ok(_) => util::log_info("atualização: instalador iniciado"),
            Err(e) => util::log_warn(&format!("atualização: o instalador não abriu: {e}")),
        }
    }
}
