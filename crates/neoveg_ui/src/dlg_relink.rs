//! Janela *Mídia não encontrada* (Localizar mídia ausente).
//!
//! Abre sozinha quando um projeto aberto aponta para arquivos que não
//! existem e já começa a procurar, de uma vez, todos eles na pasta do
//! projeto e perto dos caminhos originais. A lista mostra o estado de cada
//! arquivo; quando há vários arquivos com o mesmo nome, uma caixa de escolha
//! pede que o usuário fique com só um. *Aplicar* reaponta as mídias num
//! único passo de Desfazer, recarrega-as e (opcionalmente) salva o projeto.
//!
//! A busca roda numa thread própria protegida contra pânico (§0.6, §3.6) e
//! pode ser cancelada; o motor limita profundidade e entradas visitadas.

use eframe::egui;
use neoveg_common::{tr, tr_args};
use neoveg_engine::model::Id;
use neoveg_engine::relink::{self, MissingMedia, SearchResult, SearchState};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

struct Job {
    rx: Receiver<Result<SearchResult, String>>,
    state: Arc<SearchState>,
}

/// Estado da janela (fica em `Dialogs`).
#[derive(Default)]
pub struct RelinkUiState {
    pub open: bool,
    pub missing: Vec<MissingMedia>,
    pub roots: Vec<PathBuf>,
    job: Option<Job>,
    pub result: Option<SearchResult>,
    /// Candidato escolhido por mídia (índice em `result.candidates`).
    pub choice: HashMap<Id, usize>,
    /// Arquivo escolhido à mão por mídia (*Procurar arquivo...*).
    pub manual: HashMap<Id, PathBuf>,
    /// Não salvar o projeto depois de aplicar (por padrão, salva).
    pub dont_save: bool,
    pub error: Option<String>,
}

impl RelinkUiState {
    pub fn searching(&self) -> bool {
        self.job.is_some()
    }

    /// Arquivo que será usado para a mídia `id`, se houver.
    pub fn chosen(&self, id: Id) -> Option<PathBuf> {
        if let Some(p) = self.manual.get(&id) {
            return Some(p.clone());
        }
        let r = self.result.as_ref()?;
        let list = r.candidates.get(&id)?;
        match self.choice.get(&id) {
            Some(i) => list.get(*i).cloned(),
            None => relink::auto_choice(r, id).cloned(),
        }
    }

    /// Pares (mídia, arquivo) prontos para aplicar.
    pub fn choices(&self) -> Vec<(Id, PathBuf)> {
        self.missing
            .iter()
            .filter_map(|m| self.chosen(m.id).map(|p| (m.id, p)))
            .collect()
    }

    fn cancel(&mut self) {
        if let Some(j) = &self.job {
            j.state.cancel();
        }
    }
}

/// Verifica o projeto aberto e, se houver mídia ausente, abre a janela e já
/// começa a procurar. Devolve quantas mídias estão ausentes.
pub fn open_if_missing(app: &mut crate::App) -> usize {
    let missing = relink::find_missing(&app.session.project);
    let n = missing.len();
    if n > 0 {
        open_with(app, missing, true);
    }
    n
}

/// Comando *Arquivo → Localizar mídia ausente...*.
pub fn open_from_command(app: &mut crate::App) {
    let missing = relink::find_missing(&app.session.project);
    if missing.is_empty() {
        let lang = app.lang();
        app.set_status(tr(lang, "Nenhuma mídia ausente no projeto."));
        return;
    }
    open_with(app, missing, true);
}

fn open_with(app: &mut crate::App, missing: Vec<MissingMedia>, start: bool) {
    let st = &mut app.dlg.relink;
    st.cancel();
    st.job = None;
    st.roots = relink::default_roots(app.session.path.as_deref(), &missing);
    st.missing = missing;
    st.result = None;
    st.choice.clear();
    st.manual.clear();
    st.error = None;
    st.open = true;
    if start {
        start_search(app);
    }
}

/// Começa (ou recomeça) a busca de todas as mídias ausentes.
pub fn start_search(app: &mut crate::App) {
    let st = &mut app.dlg.relink;
    st.cancel();
    st.error = None;
    if st.roots.is_empty() || st.missing.is_empty() {
        st.job = None;
        return;
    }
    let state = Arc::new(SearchState::default());
    let (tx, rx) = channel();
    let roots = st.roots.clone();
    let missing = st.missing.clone();
    let worker_state = Arc::clone(&state);
    let spawned = neoveg_common::util::spawn("localizar-midia", move || {
        let r = neoveg_common::util::guard("localizar mídia ausente", || {
            relink::search(&roots, &missing, &worker_state)
        });
        let _ = tx.send(r);
    });
    match spawned {
        Ok(()) => st.job = Some(Job { rx, state }),
        Err(e) => {
            st.job = None;
            st.error = Some(e);
        }
    }
}

fn poll(app: &mut crate::App, ctx: &egui::Context) {
    let st = &mut app.dlg.relink;
    let Some(job) = &st.job else {
        return;
    };
    match job.rx.try_recv() {
        Ok(Ok(mut r)) => {
            // mantém os arquivos escolhidos à mão que a busca também achou
            for list in r.candidates.values_mut() {
                list.dedup();
            }
            st.choice.retain(|id, i| {
                r.candidates
                    .get(id)
                    .is_some_and(|l| *i < l.len() && l.len() > 1)
            });
            st.result = Some(r);
            st.job = None;
        }
        Ok(Err(e)) => {
            st.error = Some(e);
            st.job = None;
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            st.job = None;
        }
    }
}

/// Reaponta as mídias escolhidas, recarrega-as e salva o projeto.
pub fn apply(app: &mut crate::App) {
    let lang = app.lang();
    let choices = app.dlg.relink.choices();
    let n = app.session.relink_media(&choices);
    if n == 0 {
        app.set_status(tr(lang, "Nenhuma mídia foi reapontada."));
        return;
    }
    app.sync_after_session();
    for (id, _) in &choices {
        if let Some(m) = app.session.project.media(*id).cloned() {
            app.media.remove(*id);
            app.thumbs.remove(id);
            app.spawn_media_load(m);
        }
    }
    // o que ainda falta continua na lista
    let still = relink::find_missing(&app.session.project);
    let st = &mut app.dlg.relink;
    st.manual.retain(|id, _| still.iter().any(|m| m.id == *id));
    st.choice.retain(|id, _| still.iter().any(|m| m.id == *id));
    st.missing = still;
    if st.missing.is_empty() {
        st.open = false;
    }
    let save = !st.dont_save;
    let n_s = n.to_string();
    if save && app.save_project(false) {
        app.set_status(tr_args(
            lang,
            "{n} mídia(s) reapontada(s); projeto salvo.",
            &[("n", &n_s)],
        ));
    } else {
        app.set_status(tr_args(lang, "{n} mídia(s) reapontada(s).", &[("n", &n_s)]));
    }
}

pub fn show_relink_window(app: &mut crate::App, ctx: &egui::Context) {
    if !app.dlg.relink.open {
        return;
    }
    poll(app, ctx);
    let lang = app.lang();
    let mut open = true;
    let mut do_search = false;
    let mut do_apply = false;
    let mut do_close = false;
    let mut add_root: Option<PathBuf> = None;
    let mut remove_root: Option<usize> = None;
    let mut pick_for: Option<Id> = None;

    crate::dialog_window::DialogWindow::new(tr(lang, "Mídia não encontrada"))
        .id(egui::Id::new("relink_media_window"))
        .open(&mut open)
        .resizable(true)
        .default_size([760.0, 460.0])
        .min_size([560.0, 320.0])
        .show(ctx, |ui| {
            let st = &mut app.dlg.relink;
            let n_s = st.missing.len().to_string();
            ui.label(tr_args(
                lang,
                "{n} arquivo(s) de mídia do projeto não foram encontrados. O Neoveg procura todos de uma vez nas pastas abaixo.",
                &[("n", &n_s)],
            ));
            ui.add_space(4.0);

            // pastas da busca
            ui.horizontal(|ui| {
                ui.strong(tr(lang, "Procurar em:"));
                if ui
                    .add_enabled(!st.searching(), egui::Button::new(tr(lang, "Adicionar pasta...")))
                    .clicked()
                {
                    add_root = rfd::FileDialog::new().pick_folder();
                }
            });
            if st.roots.is_empty() {
                ui.label(
                    egui::RichText::new(tr(lang, "Nenhuma pasta. Adicione uma pasta para procurar."))
                        .weak(),
                );
            }
            for (i, r) in st.roots.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!st.searching(), egui::Button::new("×").small())
                        .on_hover_text(tr(lang, "Remover pasta"))
                        .clicked()
                    {
                        remove_root = Some(i);
                    }
                    ui.monospace(r.display().to_string());
                });
            }
            ui.separator();

            // progresso
            if let Some(job) = &st.job {
                let v = job.state.visited.load(std::sync::atomic::Ordering::Relaxed);
                let m = job.state.matches.load(std::sync::atomic::Ordering::Relaxed);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr_args(
                        lang,
                        "Procurando... {v} itens verificados, {m} arquivo(s) encontrado(s).",
                        &[("v", &v.to_string()), ("m", &m.to_string())],
                    ));
                    if ui.button(tr(lang, "Cancelar busca")).clicked() {
                        job.state.cancel();
                    }
                });
            } else if let Some(r) = &st.result {
                let note = if r.cancelled {
                    tr(lang, "Busca cancelada.")
                } else if r.truncated {
                    tr(lang, "A busca parou no limite de itens; escolha pastas menores.")
                } else {
                    tr(lang, "Busca concluída.")
                };
                ui.label(format!(
                    "{note} {}",
                    tr_args(
                        lang,
                        "{v} itens verificados.",
                        &[("v", &r.visited.to_string())]
                    )
                ));
            }
            if let Some(e) = &st.error {
                ui.colored_label(egui::Color32::from_rgb(220, 80, 70), e);
            }

            // lista das mídias ausentes
            egui::ScrollArea::vertical()
                .max_height(ui.available_height() - 64.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("relink_grid")
                        .num_columns(3)
                        .striped(true)
                        .spacing([12.0, 6.0])
                        .show(ui, |ui| {
                            ui.strong(tr(lang, "Mídia"));
                            ui.strong(tr(lang, "Caminho original"));
                            ui.strong(tr(lang, "Novo local"));
                            ui.end_row();
                            let missing = st.missing.clone();
                            for m in &missing {
                                ui.label(&m.name).on_hover_text(&m.file_name);
                                ui.label(
                                    egui::RichText::new(short_path(&m.path)).small().weak(),
                                )
                                .on_hover_text(&m.path);
                                ui.horizontal(|ui| {
                                    location_cell(ui, lang, st, m);
                                    if ui
                                        .small_button("…")
                                        .on_hover_text(tr(lang, "Procurar arquivo..."))
                                        .clicked()
                                    {
                                        pick_for = Some(m.id);
                                    }
                                });
                                ui.end_row();
                            }
                        });
                });

            ui.separator();
            let ready = st.choices().len();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !st.searching() && !st.roots.is_empty(),
                        egui::Button::new(tr(lang, "Procurar todos")),
                    )
                    .clicked()
                {
                    do_search = true;
                }
                ui.checkbox(&mut st.dont_save, tr(lang, "Não salvar o projeto"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr(lang, "Fechar")).clicked() {
                        do_close = true;
                    }
                    let label = if st.dont_save {
                        tr_args(lang, "Aplicar ({n})", &[("n", &ready.to_string())])
                    } else {
                        tr_args(lang, "Aplicar e salvar ({n})", &[("n", &ready.to_string())])
                    };
                    if ui
                        .add_enabled(ready > 0 && !st.searching(), egui::Button::new(label))
                        .clicked()
                    {
                        do_apply = true;
                    }
                });
            });
        });

    if let Some(dir) = add_root {
        let st = &mut app.dlg.relink;
        let mut roots = std::mem::take(&mut st.roots);
        roots.push(dir);
        st.roots = relink::normalize_roots(roots);
        do_search = true;
    }
    if let Some(i) = remove_root {
        if i < app.dlg.relink.roots.len() {
            app.dlg.relink.roots.remove(i);
        }
    }
    if let Some(id) = pick_for {
        if let Some(f) = rfd::FileDialog::new()
            .set_title(tr(lang, "Procurar arquivo..."))
            .pick_file()
        {
            app.dlg.relink.manual.insert(id, f);
        }
    }
    if do_search {
        start_search(app);
    }
    if do_apply {
        apply(app);
    }
    if !open || do_close {
        let st = &mut app.dlg.relink;
        st.cancel();
        st.job = None;
        st.open = false;
    }
}

/// Coluna *Novo local*: estado da busca ou a escolha entre homônimos.
fn location_cell(
    ui: &mut egui::Ui,
    lang: neoveg_common::Language,
    st: &mut RelinkUiState,
    m: &MissingMedia,
) {
    if let Some(p) = st.manual.get(&m.id) {
        ui.label(egui::RichText::new(p.display().to_string()).small())
            .on_hover_text(tr(lang, "Escolhido à mão"));
        return;
    }
    let list = st
        .result
        .as_ref()
        .and_then(|r| r.candidates.get(&m.id))
        .cloned()
        .unwrap_or_default();
    match list.len() {
        0 if st.searching() => {
            ui.label(egui::RichText::new(tr(lang, "Procurando...")).weak());
        }
        0 if st.result.is_none() => {
            ui.label(egui::RichText::new(tr(lang, "Ainda não procurado")).weak());
        }
        0 => {
            ui.label(
                egui::RichText::new(tr(lang, "Não encontrado"))
                    .color(egui::Color32::from_rgb(220, 110, 70)),
            );
        }
        1 => {
            let p = list
                .first()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            ui.label(egui::RichText::new(short_path(&p)).small())
                .on_hover_text(&p);
        }
        n => {
            let cur = st.choice.get(&m.id).copied();
            let text = match cur.and_then(|i| list.get(i)) {
                Some(p) => short_path(&p.display().to_string()),
                None => tr_args(
                    lang,
                    "{n} arquivos com este nome: escolha um",
                    &[("n", &n.to_string())],
                ),
            };
            egui::ComboBox::from_id_salt(("relink_pick", m.id))
                .width(ui.available_width().clamp(160.0, 420.0))
                .selected_text(text)
                .show_ui(ui, |ui| {
                    for (i, p) in list.iter().enumerate() {
                        if ui
                            .selectable_label(cur == Some(i), p.display().to_string())
                            .clicked()
                        {
                            st.choice.insert(m.id, i);
                        }
                    }
                });
        }
    }
}

/// Caracteres de um caminho mostrados na janela; o resto vai na dica.
const SHORT_PATH_CHARS: usize = 56;

/// Encurta um caminho longo mantendo o fim (pasta e nome do arquivo), que é o
/// que diferencia arquivos de mesmo nome. Sem isso, um caminho longo alargava
/// a coluna *Novo local* e empurrava *Aplicar e salvar* e *Fechar* para fora
/// da janela.
pub(crate) fn short_path(p: &str) -> String {
    let n = p.chars().count();
    if n <= SHORT_PATH_CHARS {
        return p.to_string();
    }
    let tail: String = p.chars().skip(n - (SHORT_PATH_CHARS - 1)).collect();
    format!("…{tail}")
}

#[cfg(test)]
mod short_path_tests {
    use super::{short_path, SHORT_PATH_CHARS};

    #[test]
    fn caminho_longo_mantem_o_fim_e_cabe_no_limite() {
        let longo = format!("C:/{}/movida/a/qa_teste.mp4", "pasta_comprida/".repeat(12));
        let curto = short_path(&longo);
        assert_eq!(curto.chars().count(), SHORT_PATH_CHARS);
        assert!(curto.starts_with('…'));
        assert!(curto.ends_with("movida/a/qa_teste.mp4"));
        assert_eq!(short_path("C:/a/b.mp4"), "C:/a/b.mp4");
        // limite de caractere, não de byte
        let acentos = "ção/".repeat(40);
        assert_eq!(short_path(&acentos).chars().count(), SHORT_PATH_CHARS);
    }
}
