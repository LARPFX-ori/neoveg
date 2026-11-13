//! Interface gráfica (`eframe` + `wgpu`) no estilo do VEGAS Pro: timeline com
//! lista de trilhas em cima, área de janelas acopláveis embaixo e prévia.

use crate::icons::{self, Icon};
use crate::layout::{self, WindowLayout};
use crate::theme::{self, ThemeColors, ThemeId};
use eframe::egui;
use egui::{Key, TextureHandle};
pub use neoveg_common::prefs::{EditLayoutPrefs, LayoutType, Preferences, PreviewQuality};
use neoveg_engine::commands::{
    self, is_command_enabled, menu_label, resolved_shortcut, run_command, CommandHost, DockPanel,
    ShortcutKey, UiAction, COMMANDS,
};
use neoveg_engine::media::{self, MediaCache};
use neoveg_engine::model::*;
use neoveg_engine::playback::{AudioOut, Preview, PreviewRequest};
use neoveg_engine::recovery;
use neoveg_engine::render::{RenderProgress, RenderSettings};
pub use neoveg_engine::session::{ClipItem, EditorOptions as Options, EditorSession};
use neoveg_engine::util;
use std::collections::{HashMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

pub use crate::dialogs::Dialogs;
pub use crate::timeline::TimelineState;
pub use neoveg_engine::commands::DockPanel as DockTab;

/// Itens arrastáveis entre janelas (drag and drop).
#[derive(Clone, Debug)]
pub enum DndPayload {
    Media(Id),
    Fx(FxKind),
    Transition(TransitionKind),
    Generator(GenSpec),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FxTarget {
    Event(Id),
    Track(Id),
    MasterAudio,
    MasterVideo,
}

#[derive(Clone, Copy)]
pub enum PlaceTarget {
    At(Ticks),
    EndOfTrack,
}

pub enum LoadMsg {
    Imported(media::Imported, Option<PlaceTarget>),
    Reloaded(Id, Result<media::Imported, String>),
    ProxyBuilt(Id, Result<PathBuf, String>),
    PrerenderDone(Result<PathBuf, String>),
    Error(String),
}

pub struct RenderJob {
    pub progress: Arc<RenderProgress>,
    pub settings: RenderSettings,
}

pub struct App {
    pub session: EditorSession,
    pub prefs: Preferences,
    pub media: Arc<MediaCache>,
    pub audio: AudioOut,
    pub(crate) preview: Preview,
    pub(crate) preview_tex: Option<TextureHandle>,
    pub(crate) preview_inflight: bool,
    pub(crate) preview_inflight_since: Instant,
    pub(crate) last_req: (Ticks, u64, usize, usize, bool, bool),
    pub(crate) preview_ms: f64,
    pub(crate) scopes_tex: Option<[TextureHandle; 4]>,
    pub tl: TimelineState,
    pub dock_tab: DockTab,
    pub detached_tabs: HashSet<DockTab>,
    pub dock_visible: bool,
    pub dock_height: f32,
    pub preview_ratio: f32,
    pub saved_layouts: Vec<(String, PathBuf)>,
    pub theme_id: ThemeId,
    pub theme: ThemeColors,
    pub playback_rate: f64,
    pub dlg: Dialogs,
    pub status_at: Instant,
    pub thumbs: HashMap<Id, TextureHandle>,
    pub(crate) loader_tx: Sender<LoadMsg>,
    pub(crate) loader_rx: Receiver<LoadMsg>,
    pub loading: usize,
    pub explorer_dir: PathBuf,
    pub explorer_filter: String,
    pub render_job: Option<RenderJob>,
    pub(crate) last_frame: Instant,
    pub(crate) meters_hold: HashMap<Id, [f32; 2]>,
    pub(crate) master_hold: [f32; 2],
    pub(crate) confirm_close: bool,
    pub(crate) allow_close: bool,
    pub shuttle: f64,
    pub gen_fonts: Vec<String>,
    pub(crate) last_autosave: Instant,
    pub(crate) recovery_offer: bool,
}

impl Deref for App {
    type Target = EditorSession;
    fn deref(&self) -> &Self::Target {
        &self.session
    }
}

impl DerefMut for App {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.session
    }
}

/// Intervalo do salvamento automático de recuperação.
const AUTOSAVE_SECS: u64 = 60;

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>) -> Self {
        Self::with_context(&cc.egui_ctx, open)
    }

    pub fn with_context(egui_ctx: &egui::Context, open: Option<PathBuf>) -> Self {
        let mut prefs = Preferences::load_or_default();
        let theme_id = ThemeId::from_pref(&prefs.theme);
        let theme = theme::apply_theme(egui_ctx, theme_id);
        let loaded_layout = prefs
            .window_layout
            .as_deref()
            .and_then(|p| layout::load_layout_file(std::path::Path::new(p)).ok());
        if let Some(el) = loaded_layout.as_ref().and_then(|l| l.edit_layout.clone()) {
            prefs.edit_layout = el;
        }
        let initial_layout = loaded_layout.unwrap_or_else(|| layout::WindowLayout {
            dock_height: prefs.edit_layout.layout_type.default_dock_height(),
            preview_ratio: prefs.edit_layout.layout_type.default_preview_ratio(),
            ..Default::default()
        });
        let saved_layouts = layout::list_saved_layouts();
        let session = EditorSession::new(Project::new_default());
        let media = Arc::new(MediaCache::default());
        media.configure_limits(prefs.frame_cache_cap, prefs.ram_preview_mb);
        let audio = AudioOut::new(session.snapshot.clone());
        let repaint_ctx = egui_ctx.clone();
        let preview = Preview::new(
            media.clone(),
            Box::new(move || repaint_ctx.request_repaint()),
        );
        let (loader_tx, loader_rx) = channel();
        let explorer_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut app = App {
            session,
            prefs,
            media,
            audio,
            preview,
            preview_tex: None,
            preview_inflight: false,
            preview_inflight_since: Instant::now(),
            last_req: (Ticks::from_raw(-1), 0, 0, 0, false, false),
            detached_tabs: initial_layout.detached_panels(),
            dock_visible: initial_layout.dock_visible,
            dock_height: initial_layout.dock_height,
            preview_ratio: initial_layout.preview_ratio,
            saved_layouts,
            theme_id,
            theme,
            playback_rate: 1.0,
            preview_ms: 0.0,
            scopes_tex: None,
            tl: TimelineState::default(),
            dock_tab: initial_layout.active_panel(),
            dlg: Dialogs::default(),
            status_at: Instant::now(),
            thumbs: HashMap::new(),
            loader_tx,
            loader_rx,
            loading: 0,
            explorer_dir,
            explorer_filter: String::new(),
            render_job: None,
            last_frame: Instant::now(),
            meters_hold: HashMap::new(),
            master_hold: [0.0; 2],
            confirm_close: false,
            allow_close: false,
            shuttle: 0.0,
            gen_fonts: vec![],
            last_autosave: Instant::now(),
            recovery_offer: recovery::find_existing_recovery().is_some(),
        };
        if let Some(e) = app.preview.error.clone() {
            app.set_status(e);
        }
        if let Some(e) = &app.audio.error {
            app.set_status(format!("Áudio: {e} (reprodução sem som)"));
        }
        if let Some(p) = open {
            if is_project_file(&p) {
                app.open_project_path(p);
            } else {
                app.import_paths(vec![p], Some(Ticks::ZERO));
            }
        }
        app
    }

    // -----------------------------------------------------------------
    // Estado / desfazer
    // -----------------------------------------------------------------

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.session.set_status(s);
        self.status_at = Instant::now();
        if self.session.status.starts_with("Erro") || self.session.status.contains("falh") {
            util::log_error(&self.session.status);
        }
    }

    pub fn sync_status_from_session(&mut self) {
        self.status_at = Instant::now();
    }

    pub fn sync_after_session(&mut self) {
        self.sync_status_from_session();
        let removed = std::mem::take(&mut self.session.removed_media_ids);
        for id in removed {
            self.thumbs.remove(&id);
        }
        for m in &self.session.project.media {
            if let MediaSource::Generated(_) = m.source {
                if !self.media.contains(m.id) {
                    self.media.insert(
                        m.id,
                        media::MediaEntry {
                            video: media::VideoSource::Generated,
                            audio: None,
                        },
                    );
                }
            }
        }
    }

    /// Salvamento automático para recuperação após falha (§0.7).
    fn autosave(&mut self) {
        if !self.dirty || self.last_autosave.elapsed().as_secs() < AUTOSAVE_SECS {
            return;
        }
        self.last_autosave = Instant::now();
        if let Err(e) = recovery::write_recovery(&self.project) {
            self.set_status(format!("Falha no salvamento automático: {e}"));
        }
    }

    fn recovery_window(&mut self, ctx: &egui::Context) {
        if !self.recovery_offer {
            return;
        }
        let mut choice = None;
        egui::Window::new("Recuperar projeto")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Foi encontrado um projeto salvo automaticamente de uma sessão anterior que não terminou normalmente.");
                ui.horizontal(|ui| {
                    if ui.button("Recuperar").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Descartar").clicked() {
                        choice = Some(false);
                    }
                });
            });
        match choice {
            Some(true) => {
                self.recovery_offer = false;
                if let Some(p) = recovery::find_existing_recovery() {
                    self.open_project_path(p);
                    self.path = None;
                    self.dirty = true;
                    self.set_status("Projeto recuperado. Salve-o com Salvar como...");
                }
            }
            Some(false) => {
                self.recovery_offer = false;
                recovery::clear_recovery();
            }
            None => {}
        }
    }

    /// Salva um ponto de desfazer antes de uma modificação.
    pub fn checkpoint(&mut self) {
        self.session.checkpoint();
    }

    /// Marca o projeto como modificado (atualiza prévia e áudio).
    pub fn changed(&mut self) {
        self.session.changed();
    }

    pub fn undo(&mut self) {
        if self.session.undo() {
            self.after_history();
        }
    }

    pub fn redo(&mut self) {
        if self.session.redo() {
            self.after_history();
        }
    }

    fn after_history(&mut self) {
        for m in &self.session.project.media {
            if let MediaSource::Generated(_) = m.source {
                self.media.invalidate_generated(m.id);
                self.thumbs.remove(&m.id);
            }
        }
        self.sync_after_session();
    }

    fn sync_snapshot(&mut self) {
        if self.session.sent_generation == self.session.generation {
            return;
        }
        self.session.project.sort_events();
        self.session.refresh_snapshot();
        let audio_map = self.media.audio_map();
        let mut s = util::lock(&self.audio.shared);
        s.project = self.session.snapshot.clone();
        s.audio = audio_map;
    }

    // -----------------------------------------------------------------
    // Transporte
    // -----------------------------------------------------------------

    pub fn is_playing(&self) -> bool {
        self.audio
            .shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .playing
    }

    pub fn play(&mut self) {
        self.sync_snapshot();
        let sr = self.audio.sr;
        let lr = if self.opts.loop_playback {
            self.project
                .loop_region
                .map(|(a, b)| (a.to_samples_round(sr), b.to_samples_round(sr)))
        } else {
            None
        };
        let mut start = self.cursor;
        if let Some((a, b)) = self.project.loop_region.filter(|_| self.opts.loop_playback) {
            if start < a || start >= b {
                start = a;
            }
        }
        self.play_start = self.cursor;
        let mut s = self
            .audio
            .shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        s.loop_range = lr.filter(|(a, b)| b > a);
        s.pos = start.to_samples_round(sr);
        let rate = if self.playback_rate.is_finite() && self.playback_rate >= 0.25 {
            self.playback_rate.clamp(0.25, 4.0)
        } else {
            1.0
        };
        s.speed = rate;
        s.mixer.reset();
        s.playing = true;
        self.shuttle = rate;
    }

    pub fn shuttle_play(&mut self, speed: f64) {
        self.sync_snapshot();
        let sr = self.audio.sr;
        let playing = self.is_playing();
        let mut s = self
            .audio
            .shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !playing {
            s.pos = self.cursor.to_samples_round(sr);
        }
        s.loop_range = None;
        s.speed = speed;
        s.playing = true;
        s.mixer.reset();
        self.shuttle = speed;
    }

    /// Para a reprodução. `keep` = manter o cursor onde parou (pausa).
    pub fn stop(&mut self, keep: bool) {
        let pos = {
            let mut s = self
                .audio
                .shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            s.playing = false;
            s.speed = 1.0;
            s.pos
        };
        self.shuttle = 0.0;
        if keep {
            self.cursor = Ticks::from_samples(pos.max(0), self.audio.sr);
        } else {
            self.cursor = self.play_start;
        }
    }

    pub fn toggle_play(&mut self) {
        if self.is_playing() {
            self.stop(false);
        } else {
            self.play();
        }
    }

    pub fn play_pos(&self) -> Ticks {
        let s = self
            .audio
            .shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ticks::from_samples(s.pos.max(0), self.audio.sr)
    }

    pub fn set_cursor(&mut self, t: Ticks) {
        let t = t.max(Ticks::ZERO);
        self.cursor = t;
        if self.is_playing() {
            let sr = self.audio.sr;
            let mut s = self
                .audio
                .shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            s.pos = t.to_samples_round(sr);
            s.mixer.reset();
            self.session.play_start = t;
        }
    }

    /// Tempo exibido (posição de reprodução durante o play, senão o cursor).
    pub fn view_time(&self) -> Ticks {
        if self.is_playing() {
            self.play_pos()
        } else {
            self.cursor
        }
    }

    pub fn fmt_time(&self, t: Ticks) -> String {
        let s = &self.project.settings;
        let fmt = s.effective_time_format(self.opts.time_format);
        format_ticks(t, s.fps, s.sample_rate, fmt)
    }

    // -----------------------------------------------------------------
    // Arquivos
    // -----------------------------------------------------------------

    pub fn new_project(&mut self) {
        self.stop(false);
        self.session.reset_project(Project::new_default(), None);
        self.thumbs.clear();
        self.set_status("Novo projeto");
    }

    pub fn open_project_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Projeto Neoveg", &["neoveg", "nvp"])
            .pick_file()
        {
            self.open_project_path(p);
        }
    }

    pub fn open_project_path(&mut self, path: PathBuf) {
        // o arquivo de projeto é entrada hostil: é validado e corrigido (§0.4)
        let res = Project::load_file(&path);
        match res {
            Ok((p, fixes)) => {
                self.stop(false);
                for f in &fixes {
                    util::log_warn(&format!("projeto corrigido ao abrir: {f}"));
                }
                self.session.reset_project(p, Some(path.clone()));
                self.thumbs.clear();
                // recarrega as mídias em segundo plano
                let items = self.project.media.clone();
                for m in items {
                    self.media.remove(m.id);
                    let tx = self.loader_tx.clone();
                    self.loading += 1;
                    let spawned = util::spawn("recarregar-midia", move || {
                        let mut m = m;
                        let r = media::load_entry(&mut m).map_err(|e| format!("{e:#}"));
                        let _ = tx.send(LoadMsg::Reloaded(m.id, r));
                    });
                    if let Err(e) = spawned {
                        self.loading = self.loading.saturating_sub(1);
                        self.set_status(e);
                    }
                }
                if fixes.is_empty() {
                    self.set_status(format!("Projeto aberto: {}", path.display()));
                } else {
                    self.set_status(format!("Projeto aberto com {} correção(ões) de dados inválidos (veja logs/neoveg.log)", fixes.len()));
                }
            }
            Err(e) => self.set_status(format!("Erro ao abrir projeto: {e}")),
        }
    }

    pub fn save_project(&mut self, save_as: bool) -> bool {
        let path = if save_as || self.path.is_none() {
            match rfd::FileDialog::new()
                .add_filter("Projeto Neoveg", &["neoveg"])
                .add_filter("Projeto legado (NVP)", &["nvp"])
                .set_file_name("projeto.neoveg")
                .save_file()
            {
                Some(p) => p,
                None => return false,
            }
        } else {
            match self.path.clone() {
                Some(p) => p,
                None => return false,
            }
        };
        self.write_project(&path)
    }

    pub(crate) fn write_project(&mut self, path: &std::path::Path) -> bool {
        self.session.project.sort_events();
        match self.session.project.save_file(path) {
            Ok(()) => {
                self.path = Some(path.to_path_buf());
                self.dirty = false;
                recovery::clear_recovery();
                self.set_status(format!("Projeto salvo: {}", path.display()));
                true
            }
            Err(e) => {
                self.set_status(format!("Erro ao salvar: {e}"));
                false
            }
        }
    }

    /// Salvar versão (incremental): projeto_01.neoveg, projeto_02.neoveg...
    pub fn save_version(&mut self) {
        let Some(p) = self.path.clone() else {
            self.save_project(true);
            return;
        };
        let stem = p
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let base = match stem.rsplit_once('_') {
            Some((b, n)) if n.chars().all(|c| c.is_ascii_digit()) => b.to_string(),
            _ => stem.clone(),
        };
        let dir = p.parent().map(|d| d.to_path_buf()).unwrap_or_default();
        for i in 1..1000 {
            let cand = dir.join(format!("{base}_{i:02}.neoveg"));
            if !cand.exists() && !dir.join(format!("{base}_{i:02}.nvp")).exists() {
                self.write_project(&cand);
                return;
            }
        }
    }

    pub fn import_dialog(&mut self) {
        if let Some(files) = rfd::FileDialog::new()
            .add_filter(
                "Mídia suportada",
                &[
                    "png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp", "avi", "y4m", "mp4",
                    "mov", "mkv", "webm", "m4v", "wmv", "asf", "wav", "aif", "aiff", "aifc",
                    "flac", "ogg", "mp3", "m4a", "aac",
                ],
            )
            .add_filter("Todos os arquivos", &["*"])
            .pick_files()
        {
            self.import_paths(files, None);
        }
    }

    pub fn import_sequence_dialog(&mut self) {
        if let Some(f) = rfd::FileDialog::new()
            .add_filter("Imagens", &["png", "jpg", "jpeg", "bmp", "tif", "tiff"])
            .pick_file()
        {
            let fps = self.project.settings.fps;
            let tx = self.loader_tx.clone();
            self.loading += 1;
            let spawned = crate::util::spawn("importar-sequencia", move || {
                match media::import_sequence(&f, fps) {
                    Ok(i) => {
                        let _ = tx.send(LoadMsg::Imported(i, None));
                    }
                    Err(e) => {
                        let _ = tx.send(LoadMsg::Error(format!("{}: {e:#}", f.display())));
                    }
                }
            });
            if let Err(e) = spawned {
                self.loading = self.loading.saturating_sub(1);
                self.set_status(e);
            }
        }
    }

    /// Importa arquivos em segundo plano. `place_at` = adiciona à timeline.
    pub fn import_paths(&mut self, paths: Vec<PathBuf>, place_at: Option<Ticks>) {
        let mut t = place_at.map(PlaceTarget::At);
        for p in paths {
            if !media::is_supported(&p) {
                self.set_status(format!("Formato não suportado: {}", p.display()));
                continue;
            }
            let tx = self.loader_tx.clone();
            self.loading += 1;
            let place = t;
            let spawned = crate::util::spawn("importar", move || match media::import_file(&p) {
                Ok(i) => {
                    let _ = tx.send(LoadMsg::Imported(i, place));
                }
                Err(e) => {
                    let _ = tx.send(LoadMsg::Error(format!("{}: {e:#}", p.display())));
                }
            });
            if let Err(e) = spawned {
                self.loading = self.loading.saturating_sub(1);
                self.set_status(e);
            }
            // arquivos múltiplos são colocados em sequência quando chegarem
            if t.is_some() {
                t = Some(PlaceTarget::EndOfTrack);
            }
        }
    }

    fn poll_loader(&mut self) {
        while let Ok(msg) = self.loader_rx.try_recv() {
            self.loading = self.loading.saturating_sub(1);
            match msg {
                LoadMsg::Imported(imp, place) => {
                    let id = imp.item.id;
                    if let Some(w) = &imp.warning {
                        self.set_status(w.clone());
                    } else {
                        self.set_status(format!("Importado: {}", imp.item.name));
                    }
                    let sr = imp.entry.audio.as_ref().map(|a| a.sample_rate);
                    self.media.insert(id, imp.entry);
                    self.checkpoint();
                    self.session.register_imported_media(imp.item, sr);
                    if let Some(place) = place {
                        let t = match place {
                            PlaceTarget::At(t) => t,
                            PlaceTarget::EndOfTrack => self.end_of_selected_track(),
                        };
                        self.add_media_to_timeline(id, t, self.sel_track);
                    }
                    self.changed();
                }
                LoadMsg::Reloaded(id, r) => match r {
                    Ok(imp) => {
                        self.media.insert(id, imp.entry);
                        if let Some(m) = self.project.media.iter_mut().find(|m| m.id == id) {
                            m.duration = imp.item.duration;
                            m.width = imp.item.width;
                            m.height = imp.item.height;
                            m.fps = imp.item.fps;
                            m.has_audio = imp.item.has_audio;
                            m.has_video = imp.item.has_video;
                            m.info = imp.item.info.clone();
                        }
                        self.generation += 1;
                    }
                    Err(e) => self.set_status(format!("Mídia offline: {e}")),
                },
                LoadMsg::ProxyBuilt(id, res) => match res {
                    Ok(p) => {
                        if let Some(m) = self.project.media.iter_mut().find(|m| m.id == id) {
                            m.proxy_path = Some(p.to_string_lossy().to_string());
                        }
                        self.dirty = true;
                        self.generation += 1;
                        self.set_status(format!("Proxy pronto: {}", p.display()));
                    }
                    Err(e) => self.set_status(format!("Erro ao gerar proxy: {e}")),
                },
                LoadMsg::PrerenderDone(res) => match res {
                    Ok(p) => {
                        self.set_status(format!("Pré-renderização concluída: {}", p.display()))
                    }
                    Err(e) => self.set_status(format!("Erro na pré-renderização: {e}")),
                },
                LoadMsg::Error(e) => self.set_status(format!("Erro de importação: {e}")),
            }
        }
    }

    fn end_of_selected_track(&self) -> Ticks {
        self.sel_track
            .and_then(|i| self.project.tracks.get(i))
            .map(|t| {
                t.events
                    .iter()
                    .map(|e| e.end())
                    .fold(Ticks::ZERO, Ticks::max)
            })
            .unwrap_or(self.cursor)
    }

    /// Adiciona uma mídia à timeline no tempo `t` (vídeo e áudio agrupados).
    pub fn add_media_to_timeline(
        &mut self,
        media_id: Id,
        t: Ticks,
        track_hint: Option<usize>,
    ) -> Vec<Id> {
        let Some(m) = self.project.media(media_id).cloned() else {
            return vec![];
        };
        let len = m
            .duration
            .unwrap_or(Ticks::from_secs_i64(5))
            .max(self.project.settings.frame_dur());
        let mut created = vec![];
        let group = if m.has_video && m.has_audio {
            Some(new_id())
        } else {
            None
        };
        let mut video_track_idx = None;
        if m.has_video {
            let ti = self.find_or_create_track(TrackKind::Video, track_hint);
            let mut e = Event::new(&m, t, len);
            e.group = group;
            if m.duration.is_none() {
                e.looped = false;
            }
            created.push(e.id);
            self.project.tracks[ti].events.push(e);
            video_track_idx = Some(ti);
        }
        if m.has_audio {
            let hint = match video_track_idx {
                Some(v) => Some(v + 1),
                None => track_hint,
            };
            let ti = self.find_or_create_track(TrackKind::Audio, hint);
            let mut e = Event::new(&m, t, len);
            e.group = group;
            created.push(e.id);
            self.project.tracks[ti].events.push(e);
        }
        self.sel_events = created.iter().cloned().collect();
        self.changed();
        created
    }

    /// Procura uma trilha do tipo a partir do índice sugerido; cria se não houver.
    pub fn find_or_create_track(&mut self, kind: TrackKind, hint: Option<usize>) -> usize {
        if let Some(h) = hint {
            if let Some(t) = self.project.tracks.get(h) {
                if t.kind == kind {
                    return h;
                }
            }
            if let Some(i) =
                (h..self.project.tracks.len()).find(|&i| self.project.tracks[i].kind == kind)
            {
                return i;
            }
        }
        if let Some(i) = self.project.tracks.iter().position(|t| t.kind == kind) {
            return i;
        }
        self.insert_track(kind)
    }

    pub fn insert_track(&mut self, kind: TrackKind) -> usize {
        self.session.insert_track(kind)
    }

    pub fn thumb(&mut self, ctx: &egui::Context, id: Id) -> Option<TextureHandle> {
        if let Some(t) = self.thumbs.get(&id) {
            return Some(t.clone());
        }
        let m = self.project.media(id)?.clone();
        if !m.has_video
            || !self.media.contains(id) && !matches!(m.source, MediaSource::Generated(_))
        {
            return None;
        }
        let img = self.media.thumbnail(&m, 160)?;
        let ci = rgba_to_color_image(&img);
        let tex = ctx.load_texture(format!("thumb{id}"), ci, egui::TextureOptions::LINEAR);
        self.thumbs.insert(id, tex.clone());
        Some(tex)
    }

    // -----------------------------------------------------------------
    // Prévia
    // -----------------------------------------------------------------

    fn update_preview(&mut self, ctx: &egui::Context) {
        let mut got = false;
        while let Ok(res) = self.preview.rx.try_recv() {
            got = true;
            self.preview_ms = res.render_ms;
            if let Some(e) = res.error {
                self.set_status(format!("Prévia: {e}"));
            }
            let image = rgba_to_color_image(&res.image);
            match &mut self.preview_tex {
                Some(t) => t.set(image, egui::TextureOptions::LINEAR),
                None => {
                    self.preview_tex =
                        Some(ctx.load_texture("preview", image, egui::TextureOptions::LINEAR))
                }
            }
            if let Some(sc) = res.scopes {
                let opts = egui::TextureOptions::NEAREST;
                let hist = rgba_to_color_image(&sc.histogram);
                let wave = rgba_to_color_image(&sc.waveform);
                let vec = rgba_to_color_image(&sc.vectorscope);
                let par = rgba_to_color_image(&sc.parade);
                match &mut self.scopes_tex {
                    Some(t) => {
                        t[0].set(hist, opts);
                        t[1].set(wave, opts);
                        t[2].set(vec, opts);
                        t[3].set(par, opts);
                    }
                    None => {
                        self.scopes_tex = Some([
                            ctx.load_texture("sc_hist", hist, opts),
                            ctx.load_texture("sc_wave", wave, opts),
                            ctx.load_texture("sc_vec", vec, opts),
                            ctx.load_texture("sc_par", par, opts),
                        ]);
                    }
                }
            }
        }
        if got {
            self.preview_inflight = false;
        }
        if self.preview_inflight && self.preview_inflight_since.elapsed().as_secs_f32() < 2.0 {
            return;
        }
        let t = self.project.settings.quantize(self.view_time());
        let s = self.opts.preview_quality.scale();
        let w = ((self.project.settings.width as f32 * s) as usize).max(16);
        let h = ((self.project.settings.height as f32 * s) as usize).max(16);
        let scopes = self.is_panel_open(DockTab::Scopes);
        let prefer_proxy = self.opts.preview_quality.prefer_proxy();
        let key = (t, self.generation, w, h, scopes, prefer_proxy);
        if key != self.last_req {
            self.sync_snapshot();
            self.preview.request(PreviewRequest {
                project: self.snapshot.clone(),
                t,
                w,
                h,
                scopes,
                prefer_proxy,
            });
            self.last_req = key;
            self.preview_inflight = true;
            self.preview_inflight_since = Instant::now();
        }
    }

    // -----------------------------------------------------------------
    // Despacho pela Tabela Única de Comandos (§3.3)
    // -----------------------------------------------------------------

    pub fn cmd_enabled(&self, id: &str) -> bool {
        let host = GuiQueryHost {
            playing: self.is_playing(),
            shuttle: self.shuttle,
            media: &self.media,
        };
        is_command_enabled(id, &self.session, &host)
    }

    pub fn exec_cmd(&mut self, ctx: Option<&egui::Context>, id: &str) {
        let mut host = GuiHost {
            playing: self.is_playing(),
            shuttle: self.shuttle,
            media: &self.media,
            actions: Vec::new(),
            removed_media: Vec::new(),
            pending_generated: Vec::new(),
        };
        let res = run_command(id, &mut self.session, &mut host);
        let GuiHost {
            actions,
            removed_media,
            pending_generated,
            ..
        } = host;
        for removed_id in removed_media {
            self.thumbs.remove(&removed_id);
        }
        for spec in pending_generated {
            self.insert_generated(spec);
        }
        self.sync_after_session();
        match res {
            Ok(_) => {
                for act in actions {
                    self.dispatch_ui_action(ctx, act);
                }
            }
            Err(msg) => self.set_status(msg),
        }
    }

    fn dispatch_ui_action(&mut self, ctx: Option<&egui::Context>, action: UiAction) {
        match action {
            UiAction::NewProject => self.new_project(),
            UiAction::OpenProjectDialog => self.open_project_dialog(),
            UiAction::SaveProject { save_as } => {
                self.save_project(save_as);
            }
            UiAction::SaveVersion => self.save_version(),
            UiAction::ImportMediaDialog => self.import_dialog(),
            UiAction::ImportSequenceDialog => self.import_sequence_dialog(),
            UiAction::OpenRenderDialog => self
                .dlg
                .open_render(&self.session.project, self.session.path.as_deref()),
            UiAction::SnapshotDialog => self.snapshot_dialog(),
            UiAction::OpenProjectProps => self.dlg.project_props = true,
            UiAction::OpenEventProps => {
                self.dlg.event_props = self.sel_events.iter().next().copied();
            }
            UiAction::OpenPanCrop => {
                self.dlg.pan_crop = self.sel_events.iter().next().copied();
            }
            UiAction::OpenFxEvent => {
                self.dlg.fx_target = self.sel_events.iter().next().map(|i| FxTarget::Event(*i));
            }
            UiAction::OpenFxMasterAudio => self.dlg.fx_target = Some(FxTarget::MasterAudio),
            UiAction::OpenFxMasterVideo => self.dlg.fx_target = Some(FxTarget::MasterVideo),
            UiAction::OpenGoto => self.dlg.goto_open = true,
            UiAction::OpenEditLayoutProps => {
                self.dlg
                    .open_edit_layout(&self.prefs, self.dock_height, self.preview_ratio);
            }
            UiAction::SetLayoutType(lt) => self.select_layout_type(ctx, lt, true),
            UiAction::OpenShortcuts => self.dlg.shortcuts = true,
            UiAction::OpenPrefs => self.dlg.open_prefs(&self.prefs),
            UiAction::OpenAbout => self.dlg.about = true,
            UiAction::BuildProxySelected => self.build_proxy_selected(),
            UiAction::PrerenderRegion => self.prerender_region(),
            UiAction::ExitApp => {
                if let Some(c) = ctx {
                    c.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            UiAction::SelectDock(panel) => {
                self.dock_visible = true;
                self.detached_tabs.remove(&panel);
                self.dock_tab = panel;
            }
            UiAction::TogglePlay => self.toggle_play(),
            UiAction::PlayFromStart => {
                self.set_cursor(Ticks::ZERO);
                self.play();
            }
            UiAction::Play => self.play(),
            UiAction::Stop { restore_cursor } => self.stop(restore_cursor),
            UiAction::Shuttle(s) => self.shuttle_play(s),
            UiAction::SetCursor(t) => self.set_cursor(t),
            UiAction::CenterTimeline => {
                let c = self.cursor;
                self.tl.center_on(c);
            }
            UiAction::ZoomBy(f) => self.tl.zoom_by(f, None),
            UiAction::JumpMarker(forward) => self.jump_marker(forward),
            UiAction::JumpMarkerIndex(idx) => {
                if let Some(t) = self.session.jump_to_marker_index(idx) {
                    self.set_cursor(t);
                }
            }
        }
    }

    // -----------------------------------------------------------------
    // Atalhos de teclado (despachados via Tabela Única de Comandos)
    // -----------------------------------------------------------------

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let (events, mods) = ctx.input(|i| (i.events.clone(), i.modifiers));
        for ev in &events {
            match ev {
                egui::Event::Copy => self.exec_cmd(Some(ctx), "edit.copy"),
                egui::Event::Cut => self.exec_cmd(Some(ctx), "edit.cut"),
                egui::Event::Paste(_) => self.exec_cmd(
                    Some(ctx),
                    if mods.shift {
                        "edit.paste_insert"
                    } else {
                        "edit.paste"
                    },
                ),
                _ => {}
            }
        }
        if self.dlg.capturing_shortcut.is_some() {
            return;
        }
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        let ctrl = mods.command;
        let shift = mods.shift;
        let alt = mods.alt;

        // Alias clássico Ctrl+Shift+Z para Refazer além do Ctrl+Y da tabela
        if ctrl && shift && !alt && pressed(Key::Z) {
            self.exec_cmd(Some(ctx), "edit.redo");
            return;
        }

        // Passo fino de 1 pixel quando ArrowLeft/Right são usadas sem modificadores
        if !ctrl && !shift && !alt {
            if pressed(Key::ArrowLeft) {
                let dt = Ticks::from_secs_f64_clamped(1.0 / self.tl.px_per_sec.max(1.0) as f64)
                    .max(Ticks::from_raw(1));
                let c = self.cursor;
                self.set_cursor(c - dt);
                return;
            }
            if pressed(Key::ArrowRight) {
                let dt = Ticks::from_secs_f64_clamped(1.0 / self.tl.px_per_sec.max(1.0) as f64)
                    .max(Ticks::from_raw(1));
                let c = self.cursor;
                self.set_cursor(c + dt);
                return;
            }
        }

        // Despacho unificado na Tabela Única de Comandos (com atalhos regraváveis)
        let mut to_run: Option<&'static str> = None;
        for cmd in COMMANDS {
            let Some(sc) = resolved_shortcut(cmd, &self.prefs.shortcuts) else {
                continue;
            };
            if sc.ctrl == ctrl && sc.shift == shift && sc.alt == alt && pressed(to_egui_key(sc.key))
            {
                to_run = Some(cmd.id);
                break;
            }
        }
        if let Some(id) = to_run {
            if self.cmd_enabled(id) {
                self.exec_cmd(Some(ctx), id);
            }
            return;
        }

        // Teclas numéricas 0-9: ir para o marcador correspondente
        if !ctrl && !alt {
            let nums = [
                Key::Num0,
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
                Key::Num7,
                Key::Num8,
                Key::Num9,
            ];
            for (i, k) in nums.iter().enumerate() {
                if pressed(*k) {
                    let idx = if i == 0 { 9 } else { i - 1 };
                    if let Some(t) = self.session.jump_to_marker_index(idx) {
                        self.set_cursor(t);
                    }
                }
            }
        }
    }

    fn jump_marker(&mut self, forward: bool) {
        if let Some(t) = self.session.jump_marker_time(forward) {
            self.set_cursor(t);
        }
    }

    // -----------------------------------------------------------------
    // Barras de menu e ferramentas (despachadas pela Tabela Única)
    // -----------------------------------------------------------------

    fn cmd_menu_btn(&mut self, ui: &mut egui::Ui, id: &str) {
        let label = menu_label(id, &self.prefs.shortcuts);
        if ui
            .add_enabled(self.cmd_enabled(id), egui::Button::new(label))
            .clicked()
        {
            self.exec_cmd(Some(ui.ctx()), id);
            ui.close_menu();
        }
    }

    fn cmd_menu_chk(&mut self, ui: &mut egui::Ui, mut value: bool, id: &str) {
        let label = menu_label(id, &self.prefs.shortcuts);
        if ui.checkbox(&mut value, label).changed() {
            self.exec_cmd(Some(ui.ctx()), id);
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("Arquivo", |ui| {
                self.cmd_menu_btn(ui, "file.new");
                self.cmd_menu_btn(ui, "file.open");
                self.cmd_menu_btn(ui, "file.save");
                self.cmd_menu_btn(ui, "file.save_as");
                self.cmd_menu_btn(ui, "file.save_version");
                ui.separator();
                self.cmd_menu_btn(ui, "file.import_media");
                self.cmd_menu_btn(ui, "file.import_sequence");
                ui.separator();
                self.cmd_menu_btn(ui, "file.render");
                self.cmd_menu_btn(ui, "file.snapshot");
                ui.separator();
                self.cmd_menu_btn(ui, "file.project_props");
                ui.separator();
                self.cmd_menu_btn(ui, "file.exit");
            });
            ui.menu_button("Editar", |ui| {
                self.cmd_menu_btn(ui, "edit.undo");
                self.cmd_menu_btn(ui, "edit.redo");
                ui.separator();
                self.cmd_menu_btn(ui, "edit.cut");
                self.cmd_menu_btn(ui, "edit.copy");
                self.cmd_menu_btn(ui, "edit.paste");
                self.cmd_menu_btn(ui, "edit.paste_insert");
                self.cmd_menu_btn(ui, "edit.delete");
                ui.separator();
                self.cmd_menu_btn(ui, "edit.split");
                self.cmd_menu_btn(ui, "edit.trim_to_selection");
                self.cmd_menu_btn(ui, "edit.delete_range_ripple");
                self.cmd_menu_btn(ui, "edit.insert_time");
                self.cmd_menu_btn(ui, "edit.close_gaps");
                ui.separator();
                self.cmd_menu_btn(ui, "edit.group");
                self.cmd_menu_btn(ui, "edit.ungroup");
                self.cmd_menu_btn(ui, "edit.select_all");
                ui.separator();
                self.cmd_menu_btn(ui, "edit.goto");
                ui.separator();
                ui.menu_button("Propriedades", |ui| {
                    ui.menu_button("Layout", |ui| {
                        self.cmd_menu_btn(ui, "edit.properties.layout");
                        ui.separator();
                        ui.label(egui::RichText::new("Tipos de layout").small().weak());
                        for (cmd_id, lt) in [
                            (
                                "edit.properties.layout.sonic_foundry",
                                LayoutType::SonicFoundry,
                            ),
                            ("edit.properties.layout.sony", LayoutType::Sony),
                            ("edit.properties.layout.magix", LayoutType::Magix),
                        ] {
                            let selected = self.prefs.edit_layout.layout_type == lt;
                            let label = menu_label(cmd_id, &self.prefs.shortcuts);
                            if ui.selectable_label(selected, label).clicked() {
                                self.exec_cmd(Some(ui.ctx()), cmd_id);
                                ui.close_menu();
                            }
                        }
                    });
                });
            });
            ui.menu_button("Exibir", |ui| {
                for tab in DockPanel::ALL {
                    let open = self.is_panel_open(tab);
                    if ui.selectable_label(open, tab.name()).clicked() {
                        self.exec_cmd(Some(ui.ctx()), tab.command_id());
                        ui.close_menu();
                    }
                }
                if ui
                    .selectable_label(self.dock_visible, "Área inferior acoplável")
                    .clicked()
                {
                    self.dock_visible = !self.dock_visible;
                    ui.close_menu();
                }
                ui.menu_button("Layouts de janela", |ui| {
                    for preset in layout::builtin_presets() {
                        let label = preset.name.clone();
                        if ui.button(label).clicked() {
                            self.apply_window_layout(preset);
                            ui.close_menu();
                        }
                    }
                    if !self.saved_layouts.is_empty() {
                        ui.separator();
                        let saved = self.saved_layouts.clone();
                        for (name, path) in saved {
                            if ui.button(&name).clicked() {
                                match layout::load_layout_file(&path) {
                                    Ok(l) => {
                                        self.apply_window_layout(l);
                                        self.prefs.window_layout =
                                            Some(path.to_string_lossy().to_string());
                                        let _ = self.prefs.save();
                                    }
                                    Err(e) => self.set_status(format!("Erro no layout: {e}")),
                                }
                                ui.close_menu();
                            }
                        }
                    }
                    ui.separator();
                    if ui
                        .button("Salvar layout atual (.neoveglayout)...")
                        .clicked()
                    {
                        self.dlg.save_layout_name = "Meu Layout".into();
                        self.dlg.save_layout_open = true;
                        ui.close_menu();
                    }
                    if ui.button("Carregar arquivo .neoveglayout...").clicked() {
                        if let Some(p) = rfd::FileDialog::new()
                            .add_filter("Layout Neoveg", &["neoveglayout", "json"])
                            .pick_file()
                        {
                            match layout::load_layout_file(&p) {
                                Ok(l) => {
                                    self.apply_window_layout(l);
                                    self.prefs.window_layout =
                                        Some(p.to_string_lossy().to_string());
                                    let _ = self.prefs.save();
                                }
                                Err(e) => self.set_status(format!("Erro no layout: {e}")),
                            }
                        }
                        ui.close_menu();
                    }
                });
                ui.menu_button("Tema", |ui| {
                    for t in ThemeId::ALL {
                        if ui.selectable_label(self.theme_id == t, t.label()).clicked() {
                            self.set_theme(ui.ctx(), t, true);
                            ui.close_menu();
                        }
                    }
                });
                ui.separator();
                self.cmd_menu_chk(ui, self.opts.show_envelopes, "view.envelopes");
                self.cmd_menu_chk(ui, self.opts.show_waveforms, "view.waveforms");
                self.cmd_menu_chk(ui, self.opts.show_thumbs, "view.thumbs");
                self.cmd_menu_chk(ui, self.opts.safe_areas, "view.safe_areas");
                ui.separator();
                ui.label("Formato de tempo:");
                for fmt in TimeFormat::ALL {
                    ui.radio_value(&mut self.opts.time_format, fmt, fmt.name());
                }
            });
            ui.menu_button("Inserir", |ui| {
                self.cmd_menu_btn(ui, "insert.track_video");
                self.cmd_menu_btn(ui, "insert.track_audio");
                ui.separator();
                self.cmd_menu_btn(ui, "insert.marker");
                self.cmd_menu_btn(ui, "insert.region");
                ui.separator();
                self.cmd_menu_btn(ui, "insert.gen_text");
                self.cmd_menu_btn(ui, "insert.gen_solid");
                ui.separator();
                ui.label("Envelopes da trilha selecionada:");
                self.cmd_menu_btn(ui, "insert.env_volume");
                self.cmd_menu_btn(ui, "insert.env_pan");
            });
            ui.menu_button("Ferramentas", |ui| {
                self.cmd_menu_btn(ui, "tools.fx_master_audio");
                self.cmd_menu_btn(ui, "tools.fx_master_video");
                ui.separator();
                self.cmd_menu_btn(ui, "tools.event_props");
                self.cmd_menu_btn(ui, "tools.pan_crop");
                self.cmd_menu_btn(ui, "tools.fx_event");
                self.cmd_menu_btn(ui, "tools.reverse");
                self.cmd_menu_btn(ui, "tools.freeze_frame");
                ui.separator();
                self.cmd_menu_btn(ui, "tools.build_proxy");
                self.cmd_menu_btn(ui, "tools.prerender");
            });
            ui.menu_button("Opções", |ui| {
                self.cmd_menu_chk(ui, self.opts.snapping, "options.snapping");
                self.cmd_menu_chk(ui, self.opts.quantize, "options.quantize");
                self.cmd_menu_chk(ui, self.opts.auto_ripple, "options.auto_ripple");
                self.cmd_menu_chk(ui, self.opts.ignore_grouping, "options.ignore_grouping");
                self.cmd_menu_chk(ui, self.opts.loop_playback, "options.loop_playback");
                ui.separator();
                ui.label(format!(
                    "Dispositivo de áudio: {} ({} Hz)",
                    self.audio.device_name, self.audio.sr
                ));
                ui.separator();
                self.cmd_menu_btn(ui, "options.prefs");
            });
            ui.menu_button("Ajuda", |ui| {
                self.cmd_menu_btn(ui, "help.shortcuts");
                self.cmd_menu_btn(ui, "help.about");
            });
        });
    }

    fn cmd_hover_text(&self, id: &str, fallback: Option<&str>) -> String {
        commands::find_command(id)
            .map(|c| match resolved_shortcut(c, &self.prefs.shortcuts) {
                Some(sc) => format!("{} ({})", c.label, sc.format()),
                None => c.label.to_string(),
            })
            .unwrap_or_else(|| fallback.unwrap_or(id).to_string())
    }

    fn cmd_tool_btn(&mut self, ui: &mut egui::Ui, icon: Icon, text: Option<&str>, id: &str) {
        let hover = self.cmd_hover_text(id, text);
        let enabled = self.cmd_enabled(id);
        if icons::icon_button(ui, icon, text, false, enabled, &self.theme)
            .on_hover_text(hover)
            .clicked()
        {
            self.exec_cmd(Some(ui.ctx()), id);
        }
    }

    fn cmd_tool_toggle(
        &mut self,
        ui: &mut egui::Ui,
        active: bool,
        icon: Icon,
        text: Option<&str>,
        id: &str,
    ) {
        let hover = self.cmd_hover_text(id, text);
        let enabled = self.cmd_enabled(id);
        if icons::icon_button(ui, icon, text, active, enabled, &self.theme)
            .on_hover_text(hover)
            .clicked()
        {
            self.exec_cmd(Some(ui.ctx()), id);
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            self.cmd_tool_btn(ui, Icon::NewFile, None, "file.new");
            self.cmd_tool_btn(ui, Icon::OpenFolder, None, "file.open");
            self.cmd_tool_btn(ui, Icon::SaveDisk, None, "file.save");
            self.cmd_tool_btn(ui, Icon::RenderClapper, Some("Renderizar"), "file.render");
            self.cmd_tool_btn(ui, Icon::GearProps, None, "file.project_props");
            ui.separator();
            self.cmd_tool_btn(ui, Icon::ImportMedia, Some("Importar"), "file.import_media");
            ui.separator();
            self.cmd_tool_btn(ui, Icon::Cut, None, "edit.cut");
            self.cmd_tool_btn(ui, Icon::Copy, None, "edit.copy");
            self.cmd_tool_btn(ui, Icon::Paste, None, "edit.paste");
            ui.separator();
            self.cmd_tool_btn(ui, Icon::Undo, None, "edit.undo");
            self.cmd_tool_btn(ui, Icon::Redo, None, "edit.redo");
            ui.separator();
            self.cmd_tool_btn(ui, Icon::Split, Some("Dividir"), "edit.split");
            self.cmd_tool_btn(ui, Icon::DeleteTrash, None, "edit.delete");
            ui.separator();
            self.cmd_tool_toggle(
                ui,
                self.opts.snapping,
                Icon::MagnetSnap,
                Some("Encaixe"),
                "options.snapping",
            );
            self.cmd_tool_toggle(
                ui,
                self.opts.auto_ripple,
                Icon::AutoRipple,
                Some("Ripple"),
                "options.auto_ripple",
            );
            self.cmd_tool_toggle(
                ui,
                self.opts.ignore_grouping,
                Icon::IgnoreGrouping,
                Some("Ignorar grupos"),
                "options.ignore_grouping",
            );
            self.cmd_tool_toggle(
                ui,
                self.opts.show_envelopes,
                Icon::Envelopes,
                Some("Envelopes"),
                "view.envelopes",
            );
            ui.separator();
            self.cmd_tool_btn(ui, Icon::LayoutWindow, None, "edit.properties.layout");
            if self.loading > 0 {
                ui.separator();
                ui.spinner();
                ui.label(format!("carregando {}...", self.loading));
            }
        });
    }

    fn transport_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let playing = self.is_playing();
            if icons::icon_button(ui, Icon::GoStart, None, false, true, &self.theme)
                .on_hover_text("Início do projeto (Ctrl+Home)")
                .clicked()
            {
                self.set_cursor(Ticks::ZERO);
            }
            if icons::icon_button(ui, Icon::PrevFrame, None, false, true, &self.theme)
                .on_hover_text("Quadro anterior (Alt+←)")
                .clicked()
            {
                let t = self
                    .project
                    .settings
                    .quantize((self.cursor - self.project.settings.frame_dur()).max(Ticks::ZERO));
                self.set_cursor(t);
            }
            if icons::icon_button(
                ui,
                if playing { Icon::Pause } else { Icon::Play },
                None,
                playing,
                true,
                &self.theme,
            )
            .on_hover_text("Reproduzir/Pausar (Enter)")
            .clicked()
            {
                if playing {
                    self.stop(true);
                } else {
                    self.play();
                }
            }
            if icons::icon_button(ui, Icon::Stop, None, false, true, &self.theme)
                .on_hover_text("Parar (Espaço/Esc)")
                .clicked()
            {
                self.stop(false);
            }
            if icons::icon_button(ui, Icon::NextFrame, None, false, true, &self.theme)
                .on_hover_text("Próximo quadro (Alt+→)")
                .clicked()
            {
                let t = self
                    .project
                    .settings
                    .quantize(self.cursor + self.project.settings.frame_dur());
                self.set_cursor(t);
            }
            if icons::icon_button(ui, Icon::GoEnd, None, false, true, &self.theme)
                .on_hover_text("Fim do projeto (Ctrl+End)")
                .clicked()
            {
                let d = self.project.duration();
                self.set_cursor(d);
            }
            if icons::icon_button(
                ui,
                Icon::LoopPlayback,
                None,
                self.opts.loop_playback,
                true,
                &self.theme,
            )
            .on_hover_text("Reprodução em loop (Q)")
            .clicked()
            {
                self.opts.loop_playback = !self.opts.loop_playback;
            }
            ui.separator();
            let t = self.view_time();
            ui.label(
                egui::RichText::new(self.fmt_time(t))
                    .monospace()
                    .size(20.0)
                    .color(self.theme.timecode_fg),
            );
            if let Some((a, b)) = self.time_sel {
                ui.label(
                    egui::RichText::new(format!(
                        "Seleção: {} → {} ({})",
                        self.fmt_time(a),
                        self.fmt_time(b),
                        self.fmt_time(b - a)
                    ))
                    .monospace()
                    .small(),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut z = self.tl.px_per_sec.ln();
                if ui
                    .add(
                        egui::Slider::new(&mut z, (2.0f32).ln()..=(2000.0f32).ln())
                            .show_value(false),
                    )
                    .on_hover_text("Zoom horizontal (↑/↓, roda do mouse)")
                    .changed()
                {
                    self.tl.px_per_sec = z.exp();
                }
                icons::icon_label(ui, Icon::ZoomGlass, 14.0, "Zoom", &self.theme);
                ui.separator();
                let mut rate_changed = false;
                if ui
                    .small_button(format!("{:.2}×", self.playback_rate))
                    .on_hover_text("Clique para restaurar velocidade normal (1,00×)")
                    .clicked()
                {
                    self.playback_rate = 1.0;
                    rate_changed = true;
                }
                rate_changed |= ui
                    .add(
                        egui::Slider::new(&mut self.playback_rate, 0.25..=4.0)
                            .logarithmic(true)
                            .show_value(false),
                    )
                    .on_hover_text("Taxa de reprodução contínua (Playback Rate)")
                    .changed();
                icons::icon_label(ui, Icon::SpeedGauge, 14.0, "Taxa:", &self.theme);
                if rate_changed && playing {
                    self.shuttle = self.playback_rate;
                    let mut s = self
                        .audio
                        .shared
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    s.speed = self.playback_rate;
                }
                if self.shuttle != 0.0 && (self.shuttle - 1.0).abs() > 1e-6 && playing {
                    ui.label(format!("Shuttle {:+.2}x", self.shuttle));
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if self.status_at.elapsed().as_secs() < 20 {
                ui.label(&self.status);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let s = &self.project.settings;
                ui.label(format!(
                    "{}x{} {} qps | {} Hz | prévia {:.0} ms | {}",
                    s.width,
                    s.height,
                    s.fps.label(),
                    s.sample_rate,
                    self.preview_ms,
                    self.path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "(sem título)".into())
                ));
                if let Some(job) = &self.render_job {
                    ui.add(
                        egui::ProgressBar::new(job.progress.fraction())
                            .desired_width(140.0)
                            .show_percentage(),
                    );
                    ui.label("Renderizando:");
                }
            });
        });
    }

    pub(crate) fn snapshot_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .set_file_name("quadro.png")
            .save_file()
        {
            match crate::render::save_snapshot(&self.project, &self.media, self.cursor, &p) {
                Ok(_) => self.set_status(format!("Quadro salvo em {}", p.display())),
                Err(e) => self.set_status(format!("Erro: {e:#}")),
            }
        }
    }

    pub fn insert_generated(&mut self, spec: GenSpec) {
        let imp = media::import_generated(spec);
        let id = imp.item.id;
        self.media.insert(id, imp.entry);
        self.checkpoint();
        self.project.media.push(imp.item);
        let hint = self
            .sel_track
            .filter(|&i| self.project.tracks.get(i).map(|t| t.kind) == Some(TrackKind::Video));
        let ids = self.add_media_to_timeline(id, self.cursor, hint);
        self.dlg.gen_editor = Some(id);
        let _ = ids;
        self.changed();
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let files: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if files.is_empty() {
            return;
        }
        let t = self.cursor;
        let projects: Vec<PathBuf> = files
            .iter()
            .filter(|p| is_project_file(p))
            .cloned()
            .collect();
        if let Some(p) = projects.into_iter().next() {
            self.open_project_path(p);
            return;
        }
        self.import_paths(files, Some(t));
    }

    fn update_title(&self, ctx: &egui::Context) {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Sem título".into());
        let title = format!("{}{} - Neoveg", name, if self.dirty { "*" } else { "" });
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }

    fn update_meters(&mut self, dt: f32) {
        let m = self
            .audio
            .shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .meters
            .clone();
        let decay = (-dt * 3.0).exp();
        for (id, v) in m.tracks {
            let h = self.meters_hold.entry(id).or_insert([0.0; 2]);
            for k in 0..2 {
                h[k] = (h[k] * decay).max(v[k]);
            }
        }
        for k in 0..2 {
            self.master_hold[k] = (self.master_hold[k] * decay).max(m.master[k]);
        }
    }

    fn poll_render(&mut self) {
        if let Some(job) = &self.render_job {
            if job
                .progress
                .finished
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                let err = job
                    .progress
                    .error
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                let secs = *job
                    .progress
                    .elapsed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                match err {
                    Some(e) => self.set_status(format!("Renderização falhou: {e}")),
                    None => self.set_status(format!(
                        "Renderização concluída em {:.1} s: {}",
                        secs,
                        job.settings.path.display()
                    )),
                }
                self.dlg.render_done = Some(self.status.clone());
                self.render_job = None;
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame_ui(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // saída normal: a cópia de recuperação não é mais necessária
        recovery::clear_recovery();
        util::log_info("Neoveg encerrado normalmente");
    }
}

impl App {
    /// Um quadro completo da interface (separado de `eframe::App` para testes).
    pub fn frame_ui(&mut self, ctx: &egui::Context) {
        let dt = self.last_frame.elapsed().as_secs_f64();
        self.last_frame = Instant::now();
        self.audio.tick_without_device(dt);
        self.poll_loader();
        self.poll_render();
        self.handle_dropped_files(ctx);
        self.handle_shortcuts(ctx);
        self.update_meters(dt as f32);

        let playing = self.is_playing();
        if playing {
            let t = self.play_pos();
            self.tl.follow(t);
            // fim do projeto: para
            if !self.opts.loop_playback
                && t > self.project.duration() + Ticks::from_millis_i64(500)
                && self.shuttle >= 0.0
            {
                self.stop(true);
            }
            ctx.request_repaint();
        }
        if self.render_job.is_some() || self.loading > 0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            self.menu_bar(ui);
            self.toolbar(ui);
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| self.status_bar(ui));

        let tl_bottom = self.prefs.edit_layout.timeline_at_bottom;
        let tr_bottom = self.prefs.edit_layout.transport_at_bottom;
        if tr_bottom {
            egui::TopBottomPanel::bottom("transport_bot").show(ctx, |ui| self.transport_bar(ui));
        }
        self.show_dock_panel(ctx, tl_bottom);
        if !tr_bottom {
            let tr_panel = if tl_bottom {
                egui::TopBottomPanel::top("transport_mid_top")
            } else {
                egui::TopBottomPanel::bottom("transport_mid_bot")
            };
            tr_panel.show(ctx, |ui| self.transport_bar(ui));
        }
        egui::CentralPanel::default().show(ctx, |ui| self.timeline_ui(ui));

        self.floating_panel_windows(ctx);
        self.show_dialogs(ctx);
        self.recovery_window(ctx);
        self.autosave();
        let audio_err = crate::util::lock(&self.audio.shared).error.take();
        if let Some(e) = audio_err {
            self.set_status(e);
        }
        self.update_preview(ctx);
        self.sync_snapshot();
        self.update_title(ctx);

        // confirmação ao fechar com alterações não salvas
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        if self.confirm_close {
            egui::Window::new("Alterações não salvas")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("O projeto tem alterações não salvas. Deseja salvar antes de sair?");
                    ui.horizontal(|ui| {
                        if ui.button("Salvar").clicked() && self.save_project(false) {
                            self.allow_close = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui.button("Descartar").clicked() {
                            self.allow_close = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui.button("Cancelar").clicked() {
                            self.confirm_close = false;
                        }
                    });
                });
        }
    }
}

impl App {
    fn show_dock_panel(&mut self, ctx: &egui::Context, at_top: bool) {
        if !self.dock_visible {
            return;
        }
        let panel = if at_top {
            egui::TopBottomPanel::top("dock_top")
        } else {
            egui::TopBottomPanel::bottom("dock_bot")
        };
        let resp = panel
            .resizable(true)
            .default_height(self.dock_height)
            .min_height(160.0)
            .show(ctx, |ui| self.dock_area(ui));
        let h = resp.response.rect.height();
        if h.is_finite() && h >= 160.0 {
            self.dock_height = h.clamp(160.0, 720.0);
        }
    }

    pub fn apply_edit_layout(
        &mut self,
        ctx: Option<&egui::Context>,
        next: EditLayoutPrefs,
        apply_metrics: bool,
        save_prefs: bool,
    ) {
        let lt = next.layout_type;
        self.prefs.edit_layout = next;
        if apply_metrics {
            self.dock_height = lt.default_dock_height();
            self.preview_ratio = lt.default_preview_ratio();
        }
        if self.prefs.edit_layout.sync_theme_with_layout && !self.theme_id.is_high_contrast() {
            let target_theme = ThemeId::from_pref(lt.default_theme_id());
            if let Some(c) = ctx {
                self.set_theme(c, target_theme, false);
            } else {
                self.theme_id = target_theme;
                self.prefs.theme = target_theme.id().to_string();
            }
        }
        if save_prefs {
            if let Err(e) = self.prefs.save() {
                self.set_status(format!("Aviso ao salvar layout: {e}"));
            }
        }
    }

    pub fn select_layout_type(
        &mut self,
        ctx: Option<&egui::Context>,
        lt: LayoutType,
        save_prefs: bool,
    ) {
        let mut next = self.prefs.edit_layout.clone();
        next.apply_type(lt);
        self.apply_edit_layout(ctx, next, true, save_prefs);
        self.set_status(format!("Layout {} aplicado ({})", lt.label(), lt.era()));
    }

    pub fn set_theme(&mut self, ctx: &egui::Context, id: ThemeId, save_prefs: bool) {
        self.theme_id = id;
        self.theme = theme::apply_theme(ctx, id);
        self.prefs.theme = id.id().to_string();
        if save_prefs {
            if let Err(e) = self.prefs.save() {
                self.set_status(format!("Aviso ao salvar tema: {e}"));
            }
        }
    }

    pub fn is_panel_open(&self, tab: DockTab) -> bool {
        (self.dock_visible && self.dock_tab == tab && !self.detached_tabs.contains(&tab))
            || self.detached_tabs.contains(&tab)
    }

    pub fn current_window_layout(&self, name: &str) -> WindowLayout {
        let mut detached: Vec<String> = self
            .detached_tabs
            .iter()
            .map(|t| layout::panel_key(*t).to_string())
            .collect();
        detached.sort();
        let mut l = WindowLayout {
            name: name.to_string(),
            dock_height: self.dock_height,
            preview_ratio: self.preview_ratio,
            dock_visible: self.dock_visible,
            active_tab: layout::panel_key(self.dock_tab).to_string(),
            detached_tabs: detached,
            px_per_sec: Some(self.tl.px_per_sec),
            edit_layout: Some(self.prefs.edit_layout.clone()),
        };
        l.sanitize();
        l
    }

    pub fn apply_window_layout(&mut self, mut l: WindowLayout) {
        l.sanitize();
        self.dock_height = l.dock_height;
        self.preview_ratio = l.preview_ratio;
        self.dock_visible = l.dock_visible;
        self.dock_tab = l.active_panel();
        self.detached_tabs = l.detached_panels();
        if self.detached_tabs.contains(&self.dock_tab) {
            if let Some(next) = DockTab::ALL
                .into_iter()
                .find(|t| !self.detached_tabs.contains(t))
            {
                self.dock_tab = next;
            }
        }
        if let Some(pps) = l.px_per_sec {
            self.tl.px_per_sec = pps;
        }
        if let Some(el) = l.edit_layout {
            self.prefs.edit_layout = el;
        }
        self.set_status(format!("Layout aplicado: {}", l.name));
    }

    pub fn save_named_layout(&mut self, name: &str) {
        let layout = self.current_window_layout(name);
        let fname = format!("{}.neoveglayout", layout::sanitize_layout_filename(name));
        let path = Preferences::layouts_dir().join(fname);
        match layout::save_layout_file(&layout, &path) {
            Ok(()) => {
                self.prefs.window_layout = Some(path.to_string_lossy().to_string());
                let _ = self.prefs.save();
                self.saved_layouts = layout::list_saved_layouts();
                self.set_status(format!("Layout salvo em {}", path.display()));
            }
            Err(e) => self.set_status(format!("Erro ao salvar layout: {e}")),
        }
    }

    pub fn build_proxy_for_media(&mut self, media_id: Id) {
        let Some(item) = self.project.media(media_id).cloned() else {
            self.set_status("Mídia não encontrada no projeto.");
            return;
        };
        if !item.has_video {
            self.set_status("Proxies aplicam-se apenas a clipes de vídeo.");
            return;
        }
        if !self.media.contains(media_id) {
            self.set_status("Mídia offline: não foi possível gerar proxy.");
            return;
        }
        let dir = neoveg_common::util::base_dir().join("proxies");
        let out = dir.join(format!("proxy_{media_id}.avi"));
        let media = self.media.clone();
        let tx = self.loader_tx.clone();
        self.loading += 1;
        self.set_status(format!("Gerando proxy de '{}'...", item.name));
        if let Err(e) = util::spawn("gerar-proxy", move || {
            let res = media
                .create_proxy_avi(&item, &out)
                .map(|()| out)
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(LoadMsg::ProxyBuilt(media_id, res));
        }) {
            self.loading = self.loading.saturating_sub(1);
            self.set_status(e);
        }
    }

    pub fn build_proxy_selected(&mut self) {
        let mut media_ids: Vec<Id> = Vec::new();
        for eid in &self.sel_events {
            if let Some(ev) = self.project.event(*eid) {
                if self
                    .project
                    .media(ev.media_id)
                    .map(|m| m.has_video)
                    .unwrap_or(false)
                    && !media_ids.contains(&ev.media_id)
                {
                    media_ids.push(ev.media_id);
                }
            }
        }
        if media_ids.is_empty() {
            self.set_status("Selecione pelo menos um evento de vídeo para gerar proxy.");
            return;
        }
        for mid in media_ids {
            self.build_proxy_for_media(mid);
        }
    }

    pub fn prerender_region(&mut self) {
        let (start, end) = if let Some((a, b)) = self.time_sel.filter(|(a, b)| b > a) {
            (a, b)
        } else if let Some((a, b)) = self.project.loop_region.filter(|(a, b)| b > a) {
            (a, b)
        } else {
            let a = self.cursor;
            let dur = self.project.duration();
            let b = if dur > a {
                (a + Ticks::from_secs_i64(5)).min(dur)
            } else {
                a + Ticks::from_secs_i64(5)
            };
            (a, b)
        };
        if end <= start {
            self.set_status("Região vazia para pré-renderizar.");
            return;
        }
        self.sync_snapshot();
        let proj = Arc::new((*self.session.snapshot).clone());
        let media = self.media.clone();
        let scale = self.opts.preview_quality.resolution().scale();
        let w = (((proj.settings.width as f32 * scale) as u32 / 2).max(32)) * 2;
        let h = (((proj.settings.height as f32 * scale) as u32 / 2).max(18)) * 2;
        let dir = neoveg_common::util::base_dir().join("prerender");
        let _ = std::fs::create_dir_all(&dir);
        let out = dir.join(format!("prerender_{}_{}.avi", start.0, end.0));
        let settings = RenderSettings {
            format: crate::render::RenderFormat::AviMjpeg,
            path: out.clone(),
            width: w,
            height: h,
            start,
            end,
            jpeg_quality: 80,
            include_audio: true,
            wav_bits: crate::render::WavBits::Pcm16,
        };
        let tx = self.loader_tx.clone();
        self.loading += 1;
        self.set_status("Pré-renderizando região selecionada...");
        if let Err(e) = util::spawn("prerender-regiao", move || {
            let prog = RenderProgress::default();
            let res = crate::render::run(&proj, &media, &settings, &prog)
                .map(|()| out)
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(LoadMsg::PrerenderDone(res));
        }) {
            self.loading = self.loading.saturating_sub(1);
            self.set_status(e);
        }
    }
}

pub(crate) fn rgba_to_color_image(img: &image::RgbaImage) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied(
        [img.width() as usize, img.height() as usize],
        img.as_raw(),
    )
}

struct GuiQueryHost<'a> {
    playing: bool,
    shuttle: f64,
    media: &'a MediaCache,
}

impl CommandHost for GuiQueryHost<'_> {
    fn supports_ui(&self) -> bool {
        true
    }
    fn is_playing(&self) -> bool {
        self.playing
    }
    fn shuttle_speed(&self) -> f64 {
        self.shuttle
    }
    fn media_cache(&self) -> Option<&MediaCache> {
        Some(self.media)
    }
    fn dispatch_ui(&mut self, _action: UiAction) -> Result<(), String> {
        Ok(())
    }
    fn insert_generated(&mut self, _s: &mut EditorSession, _spec: GenSpec) -> Result<(), String> {
        Ok(())
    }
}

struct GuiHost<'a> {
    playing: bool,
    shuttle: f64,
    media: &'a MediaCache,
    actions: Vec<UiAction>,
    removed_media: Vec<Id>,
    pending_generated: Vec<GenSpec>,
}

impl CommandHost for GuiHost<'_> {
    fn supports_ui(&self) -> bool {
        true
    }
    fn is_playing(&self) -> bool {
        self.playing
    }
    fn shuttle_speed(&self) -> f64 {
        self.shuttle
    }
    fn media_cache(&self) -> Option<&MediaCache> {
        Some(self.media)
    }
    fn dispatch_ui(&mut self, action: UiAction) -> Result<(), String> {
        self.actions.push(action);
        Ok(())
    }
    fn insert_generated(&mut self, _s: &mut EditorSession, spec: GenSpec) -> Result<(), String> {
        self.pending_generated.push(spec);
        Ok(())
    }
    fn on_media_removed(&mut self, ids: &[Id]) {
        self.removed_media.extend_from_slice(ids);
    }
}

fn to_egui_key(k: ShortcutKey) -> Key {
    match k {
        ShortcutKey::A => Key::A,
        ShortcutKey::B => Key::B,
        ShortcutKey::C => Key::C,
        ShortcutKey::D => Key::D,
        ShortcutKey::E => Key::E,
        ShortcutKey::F => Key::F,
        ShortcutKey::G => Key::G,
        ShortcutKey::H => Key::H,
        ShortcutKey::I => Key::I,
        ShortcutKey::J => Key::J,
        ShortcutKey::K => Key::K,
        ShortcutKey::L => Key::L,
        ShortcutKey::M => Key::M,
        ShortcutKey::N => Key::N,
        ShortcutKey::O => Key::O,
        ShortcutKey::P => Key::P,
        ShortcutKey::Q => Key::Q,
        ShortcutKey::R => Key::R,
        ShortcutKey::S => Key::S,
        ShortcutKey::T => Key::T,
        ShortcutKey::U => Key::U,
        ShortcutKey::V => Key::V,
        ShortcutKey::W => Key::W,
        ShortcutKey::X => Key::X,
        ShortcutKey::Y => Key::Y,
        ShortcutKey::Z => Key::Z,
        ShortcutKey::Num0 => Key::Num0,
        ShortcutKey::Num1 => Key::Num1,
        ShortcutKey::Num2 => Key::Num2,
        ShortcutKey::Num3 => Key::Num3,
        ShortcutKey::Num4 => Key::Num4,
        ShortcutKey::Num5 => Key::Num5,
        ShortcutKey::Num6 => Key::Num6,
        ShortcutKey::Num7 => Key::Num7,
        ShortcutKey::Num8 => Key::Num8,
        ShortcutKey::Num9 => Key::Num9,
        ShortcutKey::Space => Key::Space,
        ShortcutKey::Enter => Key::Enter,
        ShortcutKey::Escape => Key::Escape,
        ShortcutKey::Delete => Key::Delete,
        ShortcutKey::F2 => Key::F2,
        ShortcutKey::F8 => Key::F8,
        ShortcutKey::ArrowLeft => Key::ArrowLeft,
        ShortcutKey::ArrowRight => Key::ArrowRight,
        ShortcutKey::ArrowUp => Key::ArrowUp,
        ShortcutKey::ArrowDown => Key::ArrowDown,
        ShortcutKey::Home => Key::Home,
        ShortcutKey::End => Key::End,
        ShortcutKey::OpenBracket => Key::OpenBracket,
        ShortcutKey::CloseBracket => Key::CloseBracket,
        ShortcutKey::Backslash => Key::Backslash,
    }
}
