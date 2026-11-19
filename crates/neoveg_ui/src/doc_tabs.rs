//! Projetos `.neoveg` aninhados abertos e editados em aba própria, dentro da
//! janela do Neoveg (ao lado das abas de sequência da timeline).
//!
//! Cada aba é um documento com a sua sessão de edição (projeto, desfazer,
//! seleção, caminho e "não salvo"), o seu cache de mídia, o seu canal de
//! carregamento em segundo plano e o seu estado de timeline e de Mídia do
//! Projeto. Só uma aba fica "ao vivo" no `App`; as outras ficam guardadas e
//! são trocadas inteiras ao clicar na aba. Os ids de mídia de projetos
//! diferentes podem coincidir, por isso nada disso é compartilhado.
//!
//! - **Salvar** (`Ctrl+S`) grava o projeto principal e depois todo projeto
//!   aninhado alterado.
//! - Ao voltar para o projeto principal, os eventos dos projetos aninhados
//!   editados são reconstruídos a partir do projeto em memória (num worker),
//!   antes mesmo de salvar.
//! - A recuperação automática guarda sempre o projeto principal.

use crate::app::{App, LoadMsg};
use crate::model::{Id, MediaSource, Project};
use crate::timeline::TimelineState;
use neoveg_common::{tr, tr_args, util};
use neoveg_engine::media::{self, MediaCache};
use neoveg_engine::session::EditorSession;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// Mais abas de documento abertas ao mesmo tempo (a principal incluída):
/// cada uma guarda cache de mídia e desfazer próprios (§3.6).
pub const MAX_DOC_TABS: usize = 8;

/// O que fica guardado de um documento enquanto a aba dele não é a ativa.
pub(crate) struct DocStash {
    session: EditorSession,
    media: Arc<MediaCache>,
    loader_tx: Sender<LoadMsg>,
    loader_rx: Receiver<LoadMsg>,
    loading: usize,
    tl: TimelineState,
    project_media: crate::project_media::ProjectMediaState,
}

/// Uma aba de documento.
pub(crate) struct DocTab {
    /// Caminho canônico do projeto aninhado; `None` é o projeto principal.
    pub key: Option<PathBuf>,
    /// Conteúdo guardado; `None` na aba ativa (o conteúdo está no `App`).
    pub stash: Option<Box<DocStash>>,
    /// `edit_count` do projeto aninhado na última vez que o principal recebeu
    /// a mídia dele.
    pub synced_edits: u64,
}

/// Resultado do worker que remonta um projeto aninhado para o principal.
pub struct NestedRebuilt {
    pub id: Id,
    pub key: PathBuf,
    pub edits: u64,
    pub result: Result<media::Imported, String>,
}

/// Caminho canônico (para comparar arquivos), ou o próprio caminho.
pub fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Nome curto de um arquivo para a aba.
fn file_label(p: Option<&Path>) -> Option<String> {
    p.and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().to_string())
}

impl App {
    fn ensure_doc_tabs(&mut self) {
        if self.doc_tabs.is_empty() {
            self.doc_tabs.push(DocTab {
                key: None,
                stash: None,
                synced_edits: 0,
            });
            self.active_doc = 0;
        }
    }

    /// Há projetos aninhados abertos em aba.
    pub(crate) fn has_doc_tabs(&self) -> bool {
        self.doc_tabs.len() > 1
    }

    /// A sessão do projeto principal (ativa ou guardada).
    pub(crate) fn main_session(&self) -> &EditorSession {
        if self.active_doc == 0 {
            return &self.session;
        }
        match self.doc_tabs.first().and_then(|t| t.stash.as_ref()) {
            Some(s) => &s.session,
            None => &self.session,
        }
    }

    /// Algum documento aberto (principal ou aninhado) tem alterações não salvas.
    pub(crate) fn any_doc_dirty(&self) -> bool {
        self.session.dirty
            || self
                .doc_tabs
                .iter()
                .filter_map(|t| t.stash.as_ref())
                .any(|s| s.session.dirty)
    }

    /// Rótulos das abas: (índice, nome, ativa, não salvo).
    pub(crate) fn doc_tab_labels(&self) -> Vec<(usize, String, bool, bool)> {
        let lang = self.lang();
        self.doc_tabs
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let active = i == self.active_doc;
                let s = match (&t.stash, active) {
                    (Some(st), _) => &st.session,
                    _ => &self.session,
                };
                let name = file_label(s.path.as_deref())
                    .unwrap_or_else(|| tr(lang, "Sem título").to_string());
                (i, name, active, s.dirty)
            })
            .collect()
    }

    fn swap_live(&mut self, st: &mut DocStash) {
        std::mem::swap(&mut self.session, &mut st.session);
        std::mem::swap(&mut self.media, &mut st.media);
        std::mem::swap(&mut self.loader_tx, &mut st.loader_tx);
        std::mem::swap(&mut self.loader_rx, &mut st.loader_rx);
        std::mem::swap(&mut self.loading, &mut st.loading);
        std::mem::swap(&mut self.tl, &mut st.tl);
        std::mem::swap(&mut self.project_media, &mut st.project_media);
    }

    /// Antes de trocar de documento: trabalhos, caches e diálogos que
    /// guardam ids do documento que sai.
    fn leave_document(&mut self) {
        self.stop(false);
        if let Some(job) = self.ram_preview_job.take() {
            job.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        self.ram_preview.clear();
        if let Some((_, _, p)) = self.prerender_job.take() {
            p.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        self.thumbs.clear();
        self.multicam_thumbs.clear();
        self.meters_hold.clear();
        self.project_media.clear_thumbs();
        let d = &mut self.dlg;
        d.event_props = None;
        d.pan_crop = None;
        d.track_motion = None;
        d.gen_editor = None;
        d.take_chooser = None;
        d.scene_detect = None;
        d.motion_track_src = None;
        d.motion_track_target_event = None;
        d.motion_track_target_track = None;
        d.fx_target = None;
        // o documento que sai fica com o snapshot em dia
        self.session.refresh_snapshot();
    }

    /// Mostra a aba de documento `i`.
    pub(crate) fn switch_doc_tab(&mut self, i: usize) {
        self.ensure_doc_tabs();
        // um script em execução também troca a sessão: espere ele terminar
        if i == self.active_doc || i >= self.doc_tabs.len() || self.script_job.is_some() {
            return;
        }
        let Some(mut incoming) = self.doc_tabs.get_mut(i).and_then(|t| t.stash.take()) else {
            return;
        };
        self.leave_document();
        // opções da aplicação (barra de ferramentas) seguem de uma aba para outra
        let opts = self.session.opts.clone();
        let (edit_mode, edit_tool, box_axis, lang) = (
            self.session.edit_mode,
            self.session.edit_tool,
            self.session.box_axis,
            self.session.lang,
        );
        self.swap_live(&mut incoming);
        let prev = self.active_doc;
        if let Some(t) = self.doc_tabs.get_mut(prev) {
            t.stash = Some(incoming);
        }
        self.active_doc = i;
        let s = &mut self.session;
        s.opts = opts;
        s.edit_mode = edit_mode;
        s.edit_tool = edit_tool;
        s.box_axis = box_axis;
        s.lang = lang;
        // a troca não é edição: só força a prévia e o áudio a ressincronizar
        s.generation = s.generation.wrapping_add(1);
        s.sent_generation = 0;
        self.sync_snapshot();
        if i == 0 {
            self.rebuild_edited_nested();
        }
    }

    /// Abre o projeto `.neoveg` aninhado `path` numa aba (ou mostra a aba
    /// dele, se já estiver aberto).
    pub(crate) fn open_nested_project_tab(&mut self, path: &Path) {
        self.ensure_doc_tabs();
        let lang = self.lang();
        let key = canonical(path);
        if let Some(i) = self
            .doc_tabs
            .iter()
            .position(|t| t.key.as_ref() == Some(&key))
        {
            self.switch_doc_tab(i);
            return;
        }
        if self.main_session().path.as_deref().map(canonical).as_ref() == Some(&key) {
            self.set_status(tr(lang, "Este é o projeto principal: ele já está aberto."));
            return;
        }
        if self.doc_tabs.len() >= MAX_DOC_TABS {
            let n = MAX_DOC_TABS.saturating_sub(1).to_string();
            self.set_status(tr_args(
                lang,
                "Feche uma aba: no máximo {n} projetos aninhados abertos.",
                &[("n", &n)],
            ));
            return;
        }
        let (project, fixes) = match Project::load_file(path) {
            Ok(r) => r,
            Err(e) => {
                self.set_status(tr_args(
                    lang,
                    "Não foi possível abrir o projeto aninhado: {err}",
                    &[("err", &e)],
                ));
                return;
            }
        };
        for f in &fixes {
            util::log_warn(&format!("projeto aninhado corrigido ao abrir: {f}"));
        }
        let mut session = EditorSession::new_nested(project, lang);
        session.path = Some(path.to_path_buf());
        let media = Arc::new(MediaCache::default());
        media.configure_limits(self.prefs.frame_cache_cap, self.prefs.ram_preview_mb);
        let (loader_tx, loader_rx) = channel();
        let edits = session.edit_count;
        self.doc_tabs.push(DocTab {
            key: Some(key),
            stash: Some(Box::new(DocStash {
                session,
                media,
                loader_tx,
                loader_rx,
                loading: 0,
                tl: TimelineState::default(),
                project_media: Default::default(),
            })),
            synced_edits: edits,
        });
        let i = self.doc_tabs.len() - 1;
        self.switch_doc_tab(i);
        self.reload_all_media();
        let name = file_label(Some(path)).unwrap_or_default();
        self.set_status(tr_args(
            lang,
            "Projeto aninhado aberto em aba: {name}",
            &[("name", &name)],
        ));
    }

    /// Abre em aba o projeto aninhado da mídia `media_id` do documento ativo.
    pub(crate) fn open_nested_media_tab(&mut self, media_id: Id) {
        let lang = self.lang();
        let Some(path) = self.nested_media_path(media_id) else {
            self.set_status(tr(
                lang,
                "A mídia escolhida não é um projeto .neoveg aninhado.",
            ));
            return;
        };
        self.open_nested_project_tab(&path);
    }

    /// Caminho do projeto aninhado da mídia `media_id` (relativo à pasta do
    /// documento ativo quando o caminho guardado é relativo).
    pub(crate) fn nested_media_path(&self, media_id: Id) -> Option<PathBuf> {
        let m = self.project.media.iter().find(|m| m.id == media_id)?;
        let MediaSource::NestedProject { path } = &m.source else {
            return None;
        };
        let p = PathBuf::from(path);
        if p.is_relative() {
            if let Some(dir) = self.path.as_deref().and_then(Path::parent) {
                let c = dir.join(&p);
                if c.exists() {
                    return Some(c);
                }
            }
        }
        Some(p)
    }

    /// Comando *Abrir projeto aninhado em aba*: o do evento selecionado, ou
    /// a mídia ativa no painel Mídia do Projeto.
    pub(crate) fn open_nested_from_selection(&mut self) {
        let from_event = self
            .sel_events
            .iter()
            .filter_map(|id| self.project.event(*id).map(|e| e.media_id))
            .find(|mid| self.nested_media_path(*mid).is_some());
        match from_event.or(self.session.active_media_id) {
            Some(mid) => self.open_nested_media_tab(mid),
            None => {
                let lang = self.lang();
                self.set_status(tr(
                    lang,
                    "Selecione um evento ou uma mídia de projeto .neoveg aninhado.",
                ));
            }
        }
    }

    /// Pede para fechar a aba `i` (pergunta se ela tem alterações).
    pub(crate) fn request_close_doc_tab(&mut self, i: usize) {
        if i == 0 || i >= self.doc_tabs.len() {
            return;
        }
        let dirty = if i == self.active_doc {
            self.session.dirty
        } else {
            self.doc_tabs
                .get(i)
                .and_then(|t| t.stash.as_ref())
                .is_some_and(|s| s.session.dirty)
        };
        if dirty {
            self.pending_close_doc = Some(i);
        } else {
            self.close_doc_tab(i, false);
        }
    }

    /// Fecha a aba `i`. `discarded`: alterações descartadas, o principal volta
    /// a mostrar o projeto aninhado como está no arquivo.
    pub(crate) fn close_doc_tab(&mut self, i: usize, discarded: bool) {
        if i == 0 || i >= self.doc_tabs.len() {
            return;
        }
        if i == self.active_doc {
            self.switch_doc_tab(0);
        }
        let tab = self.doc_tabs.remove(i);
        if self.active_doc > i {
            self.active_doc -= 1;
        }
        if self.pending_close_doc == Some(i) {
            self.pending_close_doc = None;
        }
        let Some(key) = tab.key else {
            return;
        };
        if discarded
            || tab
                .stash
                .as_ref()
                .is_some_and(|s| s.session.edit_count != tab.synced_edits)
        {
            // o principal volta a ler o projeto aninhado do arquivo
            if self.active_doc == 0 {
                let items: Vec<_> = self
                    .project
                    .media
                    .iter()
                    .filter(|m| self.media_is_nested(m.id, &key))
                    .cloned()
                    .collect();
                for m in items {
                    self.spawn_media_load(m);
                }
            }
        }
    }

    fn media_is_nested(&self, id: Id, key: &Path) -> bool {
        self.nested_media_path(id)
            .is_some_and(|p| canonical(&p) == key)
    }

    /// Ao voltar para o principal: remonta, num worker, a mídia dos projetos
    /// aninhados editados desde a última vez.
    fn rebuild_edited_nested(&mut self) {
        if self.active_doc != 0 {
            return;
        }
        let mut jobs = Vec::new();
        for t in self.doc_tabs.iter_mut().skip(1) {
            let (Some(key), Some(st)) = (&t.key, &t.stash) else {
                continue;
            };
            if st.session.edit_count == t.synced_edits {
                continue;
            }
            t.synced_edits = st.session.edit_count;
            if let Some(path) = st.session.path.clone() {
                jobs.push((
                    key.clone(),
                    path,
                    st.session.project.clone(),
                    t.synced_edits,
                ));
            }
        }
        for (key, path, project, edits) in jobs {
            let items: Vec<_> = self
                .project
                .media
                .iter()
                .filter(|m| self.media_is_nested(m.id, &key))
                .cloned()
                .collect();
            for item in items {
                let tx = self.loader_tx.clone();
                let (project, path, key) = (project.clone(), path.clone(), key.clone());
                self.loading += 1;
                let spawned = util::spawn("aninhado-em-aba", move || {
                    let id = item.id;
                    // §0.6: um pânico ao montar vira erro, sem derrubar a UI
                    let result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                            let mut item = item;
                            media::load_nested_from_project(&mut item, project, &path)
                                .map_err(|e| format!("{e:#}"))
                        }))
                        .unwrap_or_else(|_| Err("falha ao montar o projeto aninhado".to_string()));
                    let _ = tx.send(LoadMsg::NestedRebuilt(Box::new(NestedRebuilt {
                        id,
                        key,
                        edits,
                        result,
                    })));
                });
                if let Err(e) = spawned {
                    self.loading = self.loading.saturating_sub(1);
                    self.set_status(e);
                }
            }
        }
    }

    /// Resultado do worker de [`Self::rebuild_edited_nested`]: aplica só o
    /// mais novo de cada mídia.
    pub(crate) fn apply_nested_rebuilt(&mut self, r: NestedRebuilt) {
        let newest = self.nested_applied.get(&r.id).copied().unwrap_or(0);
        if r.edits < newest {
            return;
        }
        self.nested_applied.insert(r.id, r.edits);
        let _ = r.key;
        match r.result {
            Ok(imp) => self.apply_reloaded(r.id, imp),
            Err(e) => {
                let lang = self.lang();
                self.set_status(tr_args(lang, "Mídia offline: {err}", &[("err", &e)]));
            }
        }
    }

    /// Salva o projeto principal e, depois, todo projeto aninhado alterado.
    /// Devolve `true` se tudo foi salvo.
    pub(crate) fn save_all_documents(&mut self) -> bool {
        self.ensure_doc_tabs();
        let lang = self.lang();
        let mut ok = true;
        // 1. o principal
        let main_needs_dialog = self.main_session().path.is_none();
        if self.active_doc == 0 || main_needs_dialog {
            self.switch_doc_tab(0);
            ok &= self.save_project_single(false);
        } else if let Some(st) = self.doc_tabs.first_mut().and_then(|t| t.stash.as_mut()) {
            if st.session.dirty {
                ok &= save_stashed(&mut st.session, &self.prefs);
                if !st.session.dirty {
                    if let Some(p) = st.session.path.clone() {
                        neoveg_engine::recovery::clear_recovery();
                        self.prefs.record_recent_project(&p);
                        let _ = self.prefs.save();
                    }
                }
            }
        }
        // 2. os aninhados alterados
        let mut failed = Vec::new();
        for i in 1..self.doc_tabs.len() {
            let saved = if i == self.active_doc {
                if self.session.dirty {
                    save_stashed(&mut self.session, &self.prefs)
                } else {
                    true
                }
            } else {
                match self.doc_tabs.get_mut(i).and_then(|t| t.stash.as_mut()) {
                    Some(st) if st.session.dirty => save_stashed(&mut st.session, &self.prefs),
                    _ => true,
                }
            };
            if !saved {
                failed.push(i);
            }
        }
        if !failed.is_empty() {
            ok = false;
            self.set_status(tr(
                lang,
                "Erro ao salvar projetos aninhados: eles continuam com alterações não salvas.",
            ));
        } else if ok {
            self.set_status(tr(lang, "Projeto e projetos aninhados salvos."));
        }
        ok
    }

    /// Antes de abrir ou criar outro projeto: fecha as abas de projetos
    /// aninhados. Recusa (e avisa) se alguma tem alterações não salvas.
    pub(crate) fn close_nested_tabs_for_reset(&mut self) -> bool {
        if !self.has_doc_tabs() {
            return true;
        }
        let nested_dirty = (1..self.doc_tabs.len()).any(|i| {
            if i == self.active_doc {
                self.session.dirty
            } else {
                self.doc_tabs
                    .get(i)
                    .and_then(|t| t.stash.as_ref())
                    .is_some_and(|s| s.session.dirty)
            }
        });
        if nested_dirty {
            let lang = self.lang();
            self.set_status(tr(
                lang,
                "Salve ou feche as abas de projetos aninhados antes de abrir outro projeto.",
            ));
            return false;
        }
        self.switch_doc_tab(0);
        self.doc_tabs.truncate(1);
        self.nested_applied.clear();
        true
    }

    /// Diálogo de fechar uma aba com alterações não salvas.
    pub(crate) fn close_doc_tab_dialog(&mut self, ctx: &eframe::egui::Context) {
        let Some(i) = self.pending_close_doc else {
            return;
        };
        if i == 0 || i >= self.doc_tabs.len() {
            self.pending_close_doc = None;
            return;
        }
        let lang = self.lang();
        let name = self
            .doc_tab_labels()
            .into_iter()
            .find(|t| t.0 == i)
            .map(|t| t.1)
            .unwrap_or_default();
        let mut keep = true;
        let mut choice = None;
        crate::dialog_window::DialogWindow::new(tr(lang, "Alterações não salvas"))
            .id(eframe::egui::Id::new("close_doc_tab_win"))
            .open(&mut keep)
            .collapsible(false)
            .resizable(false)
            .anchor(eframe::egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(tr_args(
                    lang,
                    "O projeto aninhado \"{name}\" tem alterações não salvas. Deseja salvá-lo antes de fechar a aba?",
                    &[("name", &name)],
                ));
                ui.horizontal(|ui| {
                    if ui.button(tr(lang, "Salvar")).clicked() {
                        choice = Some(true);
                    }
                    if ui.button(tr(lang, "Descartar")).clicked() {
                        choice = Some(false);
                    }
                    if ui.button(tr(lang, "Cancelar")).clicked() {
                        self.pending_close_doc = None;
                    }
                });
            });
        if !keep {
            self.pending_close_doc = None;
        }
        match choice {
            Some(true) => {
                let saved = if i == self.active_doc {
                    save_stashed(&mut self.session, &self.prefs)
                } else {
                    match self.doc_tabs.get_mut(i).and_then(|t| t.stash.as_mut()) {
                        Some(st) => save_stashed(&mut st.session, &self.prefs),
                        None => false,
                    }
                };
                if saved {
                    self.close_doc_tab(i, false);
                } else {
                    self.set_status(tr(lang, "Erro ao salvar o projeto aninhado."));
                }
                self.pending_close_doc = None;
            }
            Some(false) => {
                self.close_doc_tab(i, true);
                self.pending_close_doc = None;
            }
            None => {}
        }
    }
}

/// Salva a sessão de um documento no arquivo dela, com a rotação de cópias
/// das Preferências. Em erro, o documento continua "não salvo".
fn save_stashed(s: &mut EditorSession, prefs: &neoveg_common::prefs::Preferences) -> bool {
    let Some(path) = s.path.clone() else {
        return false;
    };
    s.project.sort_events();
    match s.project.save_file_with_backup(
        &path,
        prefs.project_backup_enabled,
        prefs.project_backup_copies,
    ) {
        Ok(()) => {
            s.dirty = false;
            true
        }
        Err(e) => {
            util::log_warn(&format!("erro ao salvar {}: {e}", path.display()));
            false
        }
    }
}
