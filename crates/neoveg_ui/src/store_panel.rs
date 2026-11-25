//! Aba Loja de scripts: catálogo do repositório `neoveg-loja`, instalação,
//! atualização, remoção e execução dos scripts instalados.
//!
//! Download e instalação rodam fora da thread da interface
//! (`neoveg_engine::store`); a aba só lê os resultados a cada quadro.

use super::App;
use eframe::egui;
use neoveg_common::{tr, tr_args, util};
use neoveg_engine::store::{self, InstalledPackage, PackageKind, StoreIndex, StorePackage};
use std::cmp::Ordering;
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// Filtro da lista.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum StoreFilter {
    #[default]
    All,
    Installed,
    Updates,
}

/// Resultado de uma instalação ou remoção.
enum StoreOp {
    Installed(Box<InstalledPackage>),
    Removed(String),
}

/// Estado da aba entre os quadros.
#[derive(Default)]
pub(crate) struct StoreState {
    requested: bool,
    loading: Option<Receiver<Result<StoreIndex, String>>>,
    index: Option<StoreIndex>,
    error: Option<String>,
    installed: Vec<InstalledPackage>,
    installed_loaded: bool,
    /// Pacote em instalação ou remoção.
    busy: Option<(String, Receiver<Result<StoreOp, String>>)>,
    search: String,
    filter: StoreFilter,
}

/// O que o usuário pediu nesta passagem.
enum StoreAction {
    Refresh,
    Install(Box<StorePackage>),
    Remove(String),
    Run(std::path::PathBuf),
    OpenPage(String),
    OpenFolder,
}

/// Uma linha da lista: o pacote do catálogo e/ou o instalado.
struct Row {
    catalog: Option<StorePackage>,
    installed: Option<InstalledPackage>,
}

impl Row {
    fn package(&self) -> Option<&StorePackage> {
        self.catalog
            .as_ref()
            .or(self.installed.as_ref().map(|i| &i.package))
    }

    fn has_update(&self) -> bool {
        match (&self.catalog, &self.installed) {
            (Some(c), Some(i)) => {
                store::compare_versions(&c.versao, &i.package.versao) == Ordering::Greater
            }
            _ => false,
        }
    }
}

/// Abre um endereço `https://` no navegador padrão.
fn open_url(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("endereço inválido".into());
    }
    let mut cmd = if cfg!(windows) {
        let mut c = std::process::Command::new("rundll32.exe");
        c.arg("url.dll,FileProtocolHandler");
        c
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else {
        std::process::Command::new("xdg-open")
    };
    cmd.arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

fn matches_search(p: &StorePackage, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty()
        || p.nome.to_lowercase().contains(&q)
        || p.descricao.to_lowercase().contains(&q)
        || p.autor.to_lowercase().contains(&q)
        || p.tags.iter().any(|t| t.contains(&q))
}

impl App {
    fn store_start_fetch(&mut self) {
        if self.store.loading.is_some() {
            return;
        }
        self.store.requested = true;
        self.store.error = None;
        let (tx, rx) = mpsc::channel();
        let spawned = util::spawn("loja-catalogo", move || {
            let r = util::guard("catálogo da loja", store::fetch_index)
                .and_then(|r| r.map_err(|e| e.to_string()));
            let _ = tx.send(r);
        });
        match spawned {
            Ok(()) => self.store.loading = Some(rx),
            Err(e) => self.store.error = Some(e),
        }
    }

    fn store_start_op(&mut self, id: String, install: Option<StorePackage>) {
        if self.store.busy.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let op_id = id.clone();
        let spawned = util::spawn("loja-pacote", move || {
            let r = util::guard("pacote da loja", || match install {
                Some(pkg) => store::install(&pkg)
                    .map(|i| StoreOp::Installed(Box::new(i)))
                    .map_err(|e| e.to_string()),
                None => store::uninstall(&op_id)
                    .map(|()| StoreOp::Removed(op_id.clone()))
                    .map_err(|e| e.to_string()),
            })
            .and_then(|r| r);
            let _ = tx.send(r);
        });
        match spawned {
            Ok(()) => self.store.busy = Some((id, rx)),
            Err(e) => self.set_status(e),
        }
    }

    fn store_poll(&mut self, ctx: &egui::Context) {
        let lang = self.lang();
        if !self.store.installed_loaded {
            self.store.installed = store::list_installed();
            self.store.installed_loaded = true;
        }
        if let Some(rx) = &self.store.loading {
            match rx.try_recv() {
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
                Err(TryRecvError::Disconnected) => self.store.loading = None,
                Ok(r) => {
                    self.store.loading = None;
                    match r {
                        Ok(index) => {
                            for s in &index.skipped {
                                util::log_warn(&format!("loja: pacote descartado: {s}"));
                            }
                            self.store.index = Some(index);
                        }
                        Err(e) => {
                            util::log_warn(&format!("loja: catálogo: {e}"));
                            self.store.error = Some(e);
                        }
                    }
                }
            }
        }
        let mut done = None;
        if let Some((_, rx)) = &self.store.busy {
            match rx.try_recv() {
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
                Err(TryRecvError::Disconnected) => {
                    done = Some(Err(tr(lang, "a operação foi interrompida").to_string()));
                }
                Ok(r) => done = Some(r),
            }
        }
        if let Some(r) = done {
            self.store.busy = None;
            self.store.installed = store::list_installed();
            match r {
                Ok(StoreOp::Installed(p)) => {
                    let name = p.package.nome.clone();
                    let ver = p.package.versao.clone();
                    self.set_status(tr_args(
                        lang,
                        "Instalado: {name} {ver}",
                        &[("name", &name), ("ver", &ver)],
                    ));
                }
                Ok(StoreOp::Removed(id)) => {
                    self.set_status(tr_args(lang, "Removido: {name}", &[("name", &id)]));
                }
                Err(e) => {
                    util::log_warn(&format!("loja: {e}"));
                    self.set_status(tr_args(lang, "Loja de scripts: {err}", &[("err", &e)]));
                }
            }
        }
    }

    fn store_rows(&self) -> Vec<Row> {
        let mut rows: Vec<Row> = Vec::new();
        if let Some(index) = &self.store.index {
            for p in &index.packages {
                rows.push(Row {
                    catalog: Some(p.clone()),
                    installed: self
                        .store
                        .installed
                        .iter()
                        .find(|i| i.package.id == p.id)
                        .cloned(),
                });
            }
        }
        // instalados que saíram do catálogo (ou catálogo ainda não baixado)
        for i in &self.store.installed {
            if !rows
                .iter()
                .any(|r| r.catalog.as_ref().map(|c| &c.id) == Some(&i.package.id))
            {
                rows.push(Row {
                    catalog: None,
                    installed: Some(i.clone()),
                });
            }
        }
        let query = self.store.search.clone();
        let filter = self.store.filter;
        rows.retain(|r| {
            let Some(p) = r.package() else {
                return false;
            };
            let keep = match filter {
                StoreFilter::All => true,
                StoreFilter::Installed => r.installed.is_some(),
                StoreFilter::Updates => r.has_update(),
            };
            keep && matches_search(p, &query)
        });
        rows
    }

    pub(crate) fn store_tab(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let ctx = ui.ctx().clone();
        if !self.store.requested {
            self.store_start_fetch();
        }
        self.store_poll(&ctx);

        let current = env!("CARGO_PKG_VERSION");
        let loading = self.store.loading.is_some();
        let busy_id = self.store.busy.as_ref().map(|(id, _)| id.clone());
        let updates = self.store_rows_unfiltered_updates().to_string();
        let mut action: Option<StoreAction> = None;

        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!loading, egui::Button::new(tr(lang, "Atualizar catálogo")))
                .clicked()
            {
                action = Some(StoreAction::Refresh);
            }
            if loading {
                ui.spinner();
            }
            ui.separator();
            ui.add(
                egui::TextEdit::singleline(&mut self.store.search)
                    .hint_text(tr(lang, "Buscar scripts"))
                    .desired_width(180.0),
            );
            ui.selectable_value(&mut self.store.filter, StoreFilter::All, tr(lang, "Todos"));
            ui.selectable_value(
                &mut self.store.filter,
                StoreFilter::Installed,
                tr(lang, "Instalados"),
            );
            ui.selectable_value(
                &mut self.store.filter,
                StoreFilter::Updates,
                tr_args(lang, "Atualizações ({n})", &[("n", &updates)]),
            );
            ui.separator();
            if ui.button(tr(lang, "Abrir pasta da loja")).clicked() {
                action = Some(StoreAction::OpenFolder);
            }
        });
        if let Some(e) = &self.store.error {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                tr_args(
                    lang,
                    "Não foi possível baixar o catálogo: {err}",
                    &[("err", e)],
                ),
            );
        }
        ui.separator();

        let rows = self.store_rows();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if rows.is_empty() {
                    let msg = if loading {
                        tr(lang, "Baixando o catálogo...")
                    } else {
                        tr(lang, "Nenhum pacote encontrado.")
                    };
                    ui.weak(msg);
                }
                for row in &rows {
                    let Some(p) = row.package() else {
                        continue;
                    };
                    ui.push_id(&p.id, |ui| {
                        ui.group(|ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                ui.strong(&p.nome);
                                ui.weak(format!("{} · {}", p.versao, tr(lang, p.tipo.label())));
                                if let Some(i) = &row.installed {
                                    ui.label(
                                        egui::RichText::new(tr_args(
                                            lang,
                                            "instalado {ver}",
                                            &[("ver", &i.package.versao)],
                                        ))
                                        .color(ui.visuals().selection.bg_fill),
                                    );
                                }
                            });
                            ui.label(&p.descricao);
                            ui.small(tr_args(
                                lang,
                                "Autor: {author} · Licença: {license}",
                                &[("author", &p.autor), ("license", &p.licenca)],
                            ));
                            if !p.tags.is_empty() {
                                ui.small(p.tags.join(", "));
                            }
                            ui.horizontal_wrapped(|ui| {
                                let busy = busy_id.is_some();
                                if busy_id.as_deref() == Some(p.id.as_str()) {
                                    ui.spinner();
                                }
                                let compatible = p.compatible_with(current);
                                match (&row.catalog, &row.installed) {
                                    (Some(c), None) => {
                                        if ui
                                            .add_enabled(
                                                compatible && !busy,
                                                egui::Button::new(tr(lang, "Instalar")),
                                            )
                                            .clicked()
                                        {
                                            action = Some(StoreAction::Install(Box::new(c.clone())));
                                        }
                                    }
                                    (Some(c), Some(_)) if row.has_update() => {
                                        if ui
                                            .add_enabled(
                                                compatible && !busy,
                                                egui::Button::new(tr_args(
                                                    lang,
                                                    "Atualizar para {ver}",
                                                    &[("ver", &c.versao)],
                                                )),
                                            )
                                            .clicked()
                                        {
                                            action = Some(StoreAction::Install(Box::new(c.clone())));
                                        }
                                    }
                                    _ => {}
                                }
                                if !compatible {
                                    ui.weak(tr_args(
                                        lang,
                                        "Requer Neoveg {ver}",
                                        &[("ver", &p.neoveg_min)],
                                    ));
                                }
                                if let Some(i) = &row.installed {
                                    let runnable = i.package.tipo == PackageKind::ScriptCs;
                                    let run = ui
                                        .add_enabled(
                                            runnable && !busy,
                                            egui::Button::new(tr(lang, "Executar")),
                                        )
                                        .on_disabled_hover_text(tr(
                                            lang,
                                            "Este tipo de pacote ainda não pode ser executado pela Loja.",
                                        ));
                                    if run.clicked() {
                                        action = Some(StoreAction::Run(i.entry_path()));
                                    }
                                    if ui
                                        .add_enabled(!busy, egui::Button::new(tr(lang, "Remover")))
                                        .clicked()
                                    {
                                        action = Some(StoreAction::Remove(i.package.id.clone()));
                                    }
                                }
                                if ui.button(tr(lang, "Página")).clicked() {
                                    action = Some(StoreAction::OpenPage(p.page_url()));
                                }
                            });
                        });
                    });
                }
            });

        match action {
            Some(StoreAction::Refresh) => self.store_start_fetch(),
            Some(StoreAction::Install(p)) => self.store_start_op(p.id.clone(), Some(*p)),
            Some(StoreAction::Remove(id)) => self.store_start_op(id, None),
            Some(StoreAction::Run(path)) => self.run_script_path(path),
            Some(StoreAction::OpenPage(url)) => {
                if let Err(e) = open_url(&url) {
                    self.set_status(e);
                }
            }
            Some(StoreAction::OpenFolder) => {
                let dir = store::store_dir();
                let _ = std::fs::create_dir_all(&dir);
                if let Err(e) = crate::crash_report::open_folder(&dir) {
                    self.set_status(e);
                }
            }
            None => {}
        }
    }

    /// Quantos pacotes instalados têm versão nova no catálogo.
    fn store_rows_unfiltered_updates(&self) -> usize {
        let Some(index) = &self.store.index else {
            return 0;
        };
        self.store
            .installed
            .iter()
            .filter(|i| {
                index.packages.iter().any(|c| {
                    c.id == i.package.id
                        && store::compare_versions(&c.versao, &i.package.versao)
                            == Ordering::Greater
                })
            })
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(id: &str, versao: &str) -> StorePackage {
        StorePackage {
            id: id.into(),
            nome: format!("Pacote {id}"),
            descricao: "Faz algo útil".into(),
            versao: versao.into(),
            tipo: PackageKind::ScriptCs,
            autor: "Neoveg".into(),
            licenca: "MIT".into(),
            fonte: "original".into(),
            neoveg_min: "0.1.0".into(),
            entrada: "S.cs".into(),
            arquivos: Vec::new(),
            tags: vec!["marcadores".into()],
        }
    }

    #[test]
    fn busca_e_atualizacao_da_linha() {
        let p = pkg("a", "1.0.0");
        assert!(matches_search(&p, ""));
        assert!(matches_search(&p, "ÚTIL"));
        assert!(matches_search(&p, "marcador"));
        assert!(!matches_search(&p, "inexistente"));

        let row = Row {
            catalog: Some(pkg("a", "1.2.0")),
            installed: Some(InstalledPackage {
                package: pkg("a", "1.0.0"),
                dir: std::path::PathBuf::from("x"),
            }),
        };
        assert!(row.has_update());
        let row = Row {
            catalog: Some(pkg("a", "1.0.0")),
            installed: Some(InstalledPackage {
                package: pkg("a", "1.0.0"),
                dir: std::path::PathBuf::from("x"),
            }),
        };
        assert!(!row.has_update());
        assert!(open_url("file:///c:/windows").is_err());
    }
}
