//! Interface gráfica (egui/eframe) no estilo do VEGAS Pro: timeline com lista
//! de trilhas em cima, área de janelas acopláveis embaixo (mídia do projeto,
//! explorer, transições, efeitos, geradores, mixer, escopos) e prévia de vídeo.

mod dialogs;
mod edit;
mod panels;
#[cfg(test)]
mod tests;
mod timeline;

use crate::media::{self, MediaCache};
use crate::model::*;
use crate::playback::{AudioOut, Preview, PreviewRequest};
use crate::render::{RenderProgress, RenderSettings};
use eframe::egui;
use egui::{Color32, Key, TextureHandle};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

pub use timeline::TimelineState;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockTab {
    ProjectMedia,
    Explorer,
    Transitions,
    VideoFx,
    AudioFx,
    Generators,
    Mixer,
    Scopes,
    Notes,
}

impl DockTab {
    const ALL: [DockTab; 9] = [
        DockTab::ProjectMedia,
        DockTab::Explorer,
        DockTab::Transitions,
        DockTab::VideoFx,
        DockTab::AudioFx,
        DockTab::Generators,
        DockTab::Mixer,
        DockTab::Scopes,
        DockTab::Notes,
    ];
    fn name(self) -> &'static str {
        match self {
            DockTab::ProjectMedia => "Mídia do Projeto",
            DockTab::Explorer => "Explorer",
            DockTab::Transitions => "Transições",
            DockTab::VideoFx => "Efeitos de Vídeo",
            DockTab::AudioFx => "Efeitos de Áudio",
            DockTab::Generators => "Geradores de Mídia",
            DockTab::Mixer => "Mixer",
            DockTab::Scopes => "Escopos",
            DockTab::Notes => "Notas",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PreviewQuality {
    Draft,
    Preview,
    Good,
}

impl PreviewQuality {
    fn name(self) -> &'static str {
        match self {
            PreviewQuality::Draft => "Rascunho (1/4)",
            PreviewQuality::Preview => "Prévia (1/2)",
            PreviewQuality::Good => "Boa (completa)",
        }
    }
    fn scale(self) -> f32 {
        match self {
            PreviewQuality::Draft => 0.25,
            PreviewQuality::Preview => 0.5,
            PreviewQuality::Good => 1.0,
        }
    }
}

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

pub struct Options {
    pub snapping: bool,
    pub quantize: bool,
    pub auto_ripple: bool,
    pub ignore_grouping: bool,
    pub show_envelopes: bool,
    pub show_waveforms: bool,
    pub show_thumbs: bool,
    pub time_format: TimeFormat,
    pub preview_quality: PreviewQuality,
    pub safe_areas: bool,
    pub loop_playback: bool,
}

#[derive(Clone)]
pub struct ClipItem {
    pub track_rel: usize,
    pub kind: TrackKind,
    pub ev: Event,
}

pub enum LoadMsg {
    Imported(media::Imported, Option<f64>),
    Reloaded(Id, Result<media::Imported, String>),
    Error(String),
}

pub struct RenderJob {
    pub progress: Arc<RenderProgress>,
    pub settings: RenderSettings,
}

pub struct App {
    pub project: Project,
    pub snapshot: Arc<Project>,
    generation: u64,
    sent_generation: u64,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    undo: Vec<Project>,
    redo: Vec<Project>,
    pub media: Arc<MediaCache>,
    pub audio: AudioOut,
    preview: Preview,
    preview_tex: Option<TextureHandle>,
    preview_inflight: bool,
    preview_inflight_since: Instant,
    last_req: (f64, u64, usize, usize, bool),
    preview_ms: f64,
    scopes_tex: Option<[TextureHandle; 4]>,
    pub cursor: f64,
    play_start: f64,
    pub sel_events: HashSet<Id>,
    pub sel_track: Option<usize>,
    pub time_sel: Option<(f64, f64)>,
    pub tl: TimelineState,
    pub opts: Options,
    pub clipboard: Vec<ClipItem>,
    pub clip_base: f64,
    pub dock_tab: DockTab,
    pub dlg: dialogs::Dialogs,
    pub status: String,
    status_at: Instant,
    pub thumbs: HashMap<Id, TextureHandle>,
    loader_tx: Sender<LoadMsg>,
    loader_rx: Receiver<LoadMsg>,
    pub loading: usize,
    pub explorer_dir: PathBuf,
    pub explorer_filter: String,
    pub render_job: Option<RenderJob>,
    last_frame: Instant,
    meters_hold: HashMap<Id, [f32; 2]>,
    master_hold: [f32; 2],
    confirm_close: bool,
    allow_close: bool,
    pub shuttle: f64,
    pub gen_fonts: Vec<String>,
}

const MAX_UNDO: usize = 200;

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>) -> Self {
        Self::with_context(&cc.egui_ctx, open)
    }

    pub fn with_context(egui_ctx: &egui::Context, open: Option<PathBuf>) -> Self {
        setup_style(egui_ctx);
        let project = Project::new_default();
        let snapshot = Arc::new(project.clone());
        let media = Arc::new(MediaCache::default());
        let audio = AudioOut::new(snapshot.clone());
        let preview = Preview::new(media.clone(), egui_ctx.clone());
        let (loader_tx, loader_rx) = channel();
        let explorer_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut app = App {
            project,
            snapshot,
            generation: 1,
            sent_generation: 0,
            path: None,
            dirty: false,
            undo: vec![],
            redo: vec![],
            media,
            audio,
            preview,
            preview_tex: None,
            preview_inflight: false,
            preview_inflight_since: Instant::now(),
            last_req: (-1.0, 0, 0, 0, false),
            preview_ms: 0.0,
            scopes_tex: None,
            cursor: 0.0,
            play_start: 0.0,
            sel_events: HashSet::new(),
            sel_track: Some(0),
            time_sel: None,
            tl: TimelineState::default(),
            opts: Options {
                snapping: true,
                quantize: true,
                auto_ripple: false,
                ignore_grouping: false,
                show_envelopes: true,
                show_waveforms: true,
                show_thumbs: true,
                time_format: TimeFormat::Timecode,
                preview_quality: PreviewQuality::Preview,
                safe_areas: false,
                loop_playback: false,
            },
            clipboard: vec![],
            clip_base: 0.0,
            dock_tab: DockTab::ProjectMedia,
            dlg: dialogs::Dialogs::default(),
            status: String::new(),
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
        };
        if let Some(e) = &app.audio.error {
            app.set_status(format!("Áudio: {e} (reprodução sem som)"));
        }
        if let Some(p) = open {
            if p.extension().map(|e| e.eq_ignore_ascii_case("nvp")).unwrap_or(false) {
                app.open_project_path(p);
            } else {
                app.import_paths(vec![p], Some(0.0));
            }
        }
        app
    }

    // -----------------------------------------------------------------
    // Estado / desfazer
    // -----------------------------------------------------------------

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = s.into();
        self.status_at = Instant::now();
    }

    /// Salva um ponto de desfazer antes de uma modificação.
    pub fn checkpoint(&mut self) {
        self.undo.push(self.project.clone());
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Marca o projeto como modificado (atualiza prévia e áudio).
    pub fn changed(&mut self) {
        self.generation += 1;
        self.dirty = true;
    }

    pub fn undo(&mut self) {
        if let Some(p) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.project, p));
            self.after_history();
            self.set_status("Desfazer");
        }
    }

    pub fn redo(&mut self) {
        if let Some(p) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.project, p));
            self.after_history();
            self.set_status("Refazer");
        }
    }

    fn after_history(&mut self) {
        self.sel_events.retain(|id| self.project.find_event(*id).is_some());
        if let Some(t) = self.sel_track {
            if t >= self.project.tracks.len() {
                self.sel_track = if self.project.tracks.is_empty() { None } else { Some(0) };
            }
        }
        for m in &self.project.media {
            if let MediaSource::Generated(_) = m.source {
                self.media.invalidate_generated(m.id);
                self.thumbs.remove(&m.id);
            }
        }
        self.changed();
    }

    fn sync_snapshot(&mut self) {
        if self.sent_generation == self.generation {
            return;
        }
        self.project.sort_events();
        self.snapshot = Arc::new(self.project.clone());
        let audio_map = self.media.audio_map();
        let mut s = self.audio.shared.lock().unwrap();
        s.project = self.snapshot.clone();
        s.audio = audio_map;
        drop(s);
        self.sent_generation = self.generation;
    }

    // -----------------------------------------------------------------
    // Transporte
    // -----------------------------------------------------------------

    pub fn is_playing(&self) -> bool {
        self.audio.shared.lock().unwrap().playing
    }

    pub fn play(&mut self) {
        self.sync_snapshot();
        let sr = self.audio.sr as f64;
        let lr = if self.opts.loop_playback {
            self.project.loop_region.map(|(a, b)| ((a * sr) as i64, (b * sr) as i64))
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
        let mut s = self.audio.shared.lock().unwrap();
        s.loop_range = lr.filter(|(a, b)| b > a);
        s.pos = (start * sr) as i64;
        s.speed = 1.0;
        s.mixer.reset();
        s.playing = true;
        self.shuttle = 1.0;
    }

    pub fn shuttle_play(&mut self, speed: f64) {
        self.sync_snapshot();
        let sr = self.audio.sr as f64;
        let playing = self.is_playing();
        let mut s = self.audio.shared.lock().unwrap();
        if !playing {
            s.pos = (self.cursor * sr) as i64;
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
            let mut s = self.audio.shared.lock().unwrap();
            s.playing = false;
            s.speed = 1.0;
            s.pos
        };
        self.shuttle = 0.0;
        if keep {
            self.cursor = (pos as f64 / self.audio.sr as f64).max(0.0);
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

    pub fn play_pos(&self) -> f64 {
        let s = self.audio.shared.lock().unwrap();
        (s.pos as f64 / self.audio.sr as f64).max(0.0)
    }

    pub fn set_cursor(&mut self, t: f64) {
        let t = t.max(0.0);
        self.cursor = t;
        if self.is_playing() {
            let sr = self.audio.sr as f64;
            let mut s = self.audio.shared.lock().unwrap();
            s.pos = (t * sr) as i64;
            s.mixer.reset();
            self.play_start = t;
        }
    }

    /// Tempo exibido (posição de reprodução durante o play, senão o cursor).
    pub fn view_time(&self) -> f64 {
        if self.is_playing() {
            self.play_pos()
        } else {
            self.cursor
        }
    }

    pub fn fmt_time(&self, t: f64) -> String {
        format_time(t, self.project.settings.fps, self.opts.time_format)
    }

    // -----------------------------------------------------------------
    // Arquivos
    // -----------------------------------------------------------------

    pub fn new_project(&mut self) {
        self.stop(false);
        self.project = Project::new_default();
        self.path = None;
        self.dirty = false;
        self.undo.clear();
        self.redo.clear();
        self.sel_events.clear();
        self.sel_track = Some(0);
        self.cursor = 0.0;
        self.time_sel = None;
        self.thumbs.clear();
        self.changed();
        self.dirty = false;
        self.set_status("Novo projeto");
    }

    pub fn open_project_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Projeto NVOV", &["nvp"])
            .pick_file()
        {
            self.open_project_path(p);
        }
    }

    pub fn open_project_path(&mut self, path: PathBuf) {
        let res = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<Project>(&s).map_err(|e| e.to_string()));
        match res {
            Ok(p) => {
                self.stop(false);
                bump_ids(p.max_id());
                self.project = p;
                self.path = Some(path.clone());
                self.undo.clear();
                self.redo.clear();
                self.sel_events.clear();
                self.sel_track = if self.project.tracks.is_empty() { None } else { Some(0) };
                self.cursor = 0.0;
                self.thumbs.clear();
                // recarrega as mídias em segundo plano
                let items = self.project.media.clone();
                for m in items {
                    self.media.remove(m.id);
                    let tx = self.loader_tx.clone();
                    self.loading += 1;
                    std::thread::spawn(move || {
                        let mut m = m;
                        let r = media::load_entry(&mut m).map_err(|e| format!("{e:#}"));
                        let _ = tx.send(LoadMsg::Reloaded(m.id, r));
                    });
                }
                self.changed();
                self.dirty = false;
                self.set_status(format!("Projeto aberto: {}", path.display()));
            }
            Err(e) => self.set_status(format!("Erro ao abrir projeto: {e}")),
        }
    }

    pub fn save_project(&mut self, save_as: bool) -> bool {
        let path = if save_as || self.path.is_none() {
            match rfd::FileDialog::new()
                .add_filter("Projeto NVOV", &["nvp"])
                .set_file_name("projeto.nvp")
                .save_file()
            {
                Some(p) => p,
                None => return false,
            }
        } else {
            self.path.clone().unwrap()
        };
        self.write_project(&path)
    }

    fn write_project(&mut self, path: &std::path::Path) -> bool {
        self.project.sort_events();
        match serde_json::to_string_pretty(&self.project) {
            Ok(s) => match std::fs::write(path, s) {
                Ok(_) => {
                    self.path = Some(path.to_path_buf());
                    self.dirty = false;
                    self.set_status(format!("Projeto salvo: {}", path.display()));
                    true
                }
                Err(e) => {
                    self.set_status(format!("Erro ao salvar: {e}"));
                    false
                }
            },
            Err(e) => {
                self.set_status(format!("Erro ao serializar: {e}"));
                false
            }
        }
    }

    /// Salvar versão (incremental): projeto_01.nvp, projeto_02.nvp...
    pub fn save_version(&mut self) {
        let Some(p) = self.path.clone() else {
            self.save_project(true);
            return;
        };
        let stem = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let base = match stem.rsplit_once('_') {
            Some((b, n)) if n.chars().all(|c| c.is_ascii_digit()) => b.to_string(),
            _ => stem.clone(),
        };
        let dir = p.parent().map(|d| d.to_path_buf()).unwrap_or_default();
        for i in 1..1000 {
            let cand = dir.join(format!("{base}_{i:02}.nvp"));
            if !cand.exists() {
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
                    "png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp", "avi", "y4m", "wav", "mp3", "flac", "ogg", "m4a", "aac",
                    "aif", "aiff", "caf", "mp4", "mov", "mkv", "webm",
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
            std::thread::spawn(move || match media::import_sequence(&f, fps) {
                Ok(i) => {
                    let _ = tx.send(LoadMsg::Imported(i, None));
                }
                Err(e) => {
                    let _ = tx.send(LoadMsg::Error(format!("{}: {e:#}", f.display())));
                }
            });
        }
    }

    /// Importa arquivos em segundo plano. `place_at` = adiciona à timeline.
    pub fn import_paths(&mut self, paths: Vec<PathBuf>, place_at: Option<f64>) {
        let mut t = place_at;
        for p in paths {
            if !media::is_supported(&p) {
                self.set_status(format!("Formato não suportado: {}", p.display()));
                continue;
            }
            let tx = self.loader_tx.clone();
            self.loading += 1;
            let place = t;
            std::thread::spawn(move || match media::import_file(&p) {
                Ok(i) => {
                    let _ = tx.send(LoadMsg::Imported(i, place));
                }
                Err(e) => {
                    let _ = tx.send(LoadMsg::Error(format!("{}: {e:#}", p.display())));
                }
            });
            // arquivos múltiplos são colocados em sequência quando chegarem
            if t.is_some() {
                t = Some(f64::NAN);
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
                    self.media.insert(id, imp.entry);
                    self.checkpoint();
                    self.project.media.push(imp.item);
                    if let Some(t) = place {
                        let t = if t.is_nan() { self.end_of_selected_track() } else { t };
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
                LoadMsg::Error(e) => self.set_status(format!("Erro de importação: {e}")),
            }
        }
    }

    fn end_of_selected_track(&self) -> f64 {
        self.sel_track
            .and_then(|i| self.project.tracks.get(i))
            .map(|t| t.events.iter().map(|e| e.end()).fold(0.0, f64::max))
            .unwrap_or(self.cursor)
    }

    /// Adiciona uma mídia à timeline no tempo `t` (vídeo e áudio agrupados).
    pub fn add_media_to_timeline(&mut self, media_id: Id, t: f64, track_hint: Option<usize>) -> Vec<Id> {
        let Some(m) = self.project.media(media_id).cloned() else { return vec![] };
        let len = m.duration.unwrap_or(5.0).max(self.project.settings.frame_dur());
        let mut created = vec![];
        let group = if m.has_video && m.has_audio { Some(new_id()) } else { None };
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
            if let Some(i) = (h..self.project.tracks.len()).find(|&i| self.project.tracks[i].kind == kind) {
                return i;
            }
        }
        if let Some(i) = self.project.tracks.iter().position(|t| t.kind == kind) {
            return i;
        }
        self.insert_track(kind)
    }

    pub fn insert_track(&mut self, kind: TrackKind) -> usize {
        let n = self.project.tracks.iter().filter(|t| t.kind == kind).count() + 1;
        let name = match kind {
            TrackKind::Video => format!("Vídeo {n}"),
            TrackKind::Audio => format!("Áudio {n}"),
        };
        let t = Track::new(kind, &name);
        // vídeo: insere acima das trilhas de áudio; áudio: no final
        let idx = match kind {
            TrackKind::Video => match self.sel_track {
                Some(i) if self.project.tracks.get(i).map(|t| t.kind) == Some(TrackKind::Video) => i,
                _ => self.project.tracks.iter().position(|t| t.kind == TrackKind::Audio).unwrap_or(self.project.tracks.len()),
            },
            TrackKind::Audio => self.project.tracks.len(),
        };
        self.project.tracks.insert(idx, t);
        self.sel_track = Some(idx);
        self.changed();
        idx
    }

    pub fn thumb(&mut self, ctx: &egui::Context, id: Id) -> Option<TextureHandle> {
        if let Some(t) = self.thumbs.get(&id) {
            return Some(t.clone());
        }
        let m = self.project.media(id)?.clone();
        if !m.has_video || !self.media.contains(id) && !matches!(m.source, MediaSource::Generated(_)) {
            return None;
        }
        let img = self.media.thumbnail(&m, 160)?;
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
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
            match &mut self.preview_tex {
                Some(t) => t.set(res.image, egui::TextureOptions::LINEAR),
                None => self.preview_tex = Some(ctx.load_texture("preview", res.image, egui::TextureOptions::LINEAR)),
            }
            if let Some(sc) = res.scopes {
                let opts = egui::TextureOptions::NEAREST;
                match &mut self.scopes_tex {
                    Some(t) => {
                        t[0].set(sc.histogram, opts);
                        t[1].set(sc.waveform, opts);
                        t[2].set(sc.vectorscope, opts);
                        t[3].set(sc.parade, opts);
                    }
                    None => {
                        self.scopes_tex = Some([
                            ctx.load_texture("sc_hist", sc.histogram, opts),
                            ctx.load_texture("sc_wave", sc.waveform, opts),
                            ctx.load_texture("sc_vec", sc.vectorscope, opts),
                            ctx.load_texture("sc_par", sc.parade, opts),
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
        let scopes = self.dock_tab == DockTab::Scopes;
        let key = (t, self.generation, w, h, scopes);
        if key != self.last_req {
            self.sync_snapshot();
            self.preview.request(PreviewRequest {
                project: self.snapshot.clone(),
                t,
                w,
                h,
                scopes,
                overlay_safe: self.opts.safe_areas,
            });
            self.last_req = key;
            self.preview_inflight = true;
            self.preview_inflight_since = Instant::now();
        }
    }

    // -----------------------------------------------------------------
    // Atalhos de teclado (baseados nos atalhos padrão do VEGAS Pro)
    // -----------------------------------------------------------------

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let (events, mods) = ctx.input(|i| (i.events.clone(), i.modifiers));
        for ev in &events {
            match ev {
                egui::Event::Copy => self.copy_selection(),
                egui::Event::Cut => self.cut_selection(),
                egui::Event::Paste(_) => self.paste(mods.shift),
                _ => {}
            }
        }
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        let ctrl = mods.command;
        let shift = mods.shift;
        let alt = mods.alt;
        let fd = self.project.settings.frame_dur();

        if pressed(Key::Space) {
            if shift {
                self.set_cursor(0.0);
                self.play();
            } else {
                self.toggle_play();
            }
        }
        if pressed(Key::Enter) && !alt {
            if self.is_playing() {
                self.stop(true);
            } else {
                self.play();
            }
        }
        if pressed(Key::Enter) && alt {
            self.dlg.project_props = true;
        }
        if pressed(Key::Escape) && self.is_playing() {
            self.stop(false);
        }
        if ctrl && pressed(Key::Z) {
            if shift {
                self.redo();
            } else {
                self.undo();
            }
        }
        if ctrl && pressed(Key::Y) {
            self.redo();
        }
        if ctrl && pressed(Key::S) {
            if alt {
                self.save_version();
            } else {
                self.save_project(shift);
            }
        }
        if ctrl && pressed(Key::O) {
            self.open_project_dialog();
        }
        if ctrl && pressed(Key::N) {
            self.new_project();
        }
        if ctrl && pressed(Key::M) {
            self.dlg.open_render(&self.project, self.path.as_deref());
        }
        if ctrl && pressed(Key::I) {
            self.import_dialog();
        }
        if ctrl && pressed(Key::A) {
            if shift {
                self.sel_events.clear();
            } else {
                self.sel_events = self.project.tracks.iter().flat_map(|t| t.events.iter().map(|e| e.id)).collect();
            }
        }
        if ctrl && pressed(Key::Q) {
            self.checkpoint();
            self.insert_track(if shift { TrackKind::Video } else { TrackKind::Audio });
        }
        if ctrl && pressed(Key::T) {
            self.trim_to_selection();
        }
        if ctrl && pressed(Key::G) {
            self.dlg.goto_open = true;
        }
        if ctrl && pressed(Key::L) {
            self.opts.auto_ripple = !self.opts.auto_ripple;
            self.set_status(if self.opts.auto_ripple { "Ripple automático ligado" } else { "Ripple automático desligado" });
        }
        if ctrl {
            if pressed(Key::Home) {
                self.set_cursor(0.0);
            }
            if pressed(Key::End) {
                let d = self.project.duration();
                self.set_cursor(d);
            }
            if pressed(Key::ArrowLeft) {
                self.jump_marker(false);
            }
            if pressed(Key::ArrowRight) {
                self.jump_marker(true);
            }
            if pressed(Key::ArrowUp) {
                self.tl.zoom_by(2.0, None);
            }
            if pressed(Key::ArrowDown) {
                self.tl.zoom_by(0.5, None);
            }
            return;
        }
        if pressed(Key::Delete) {
            self.delete_selection();
        }
        if pressed(Key::S) && !alt {
            self.split_at_cursor();
        }
        if pressed(Key::M) {
            self.insert_marker();
        }
        if pressed(Key::R) {
            self.insert_region();
        }
        if pressed(Key::Q) {
            if shift {
                if let Some(r) = self.project.loop_region {
                    self.time_sel = Some(r);
                }
            } else {
                self.opts.loop_playback = !self.opts.loop_playback;
            }
        }
        if pressed(Key::I) {
            let c = self.cursor;
            let end = self.time_sel.map(|s| s.1).filter(|e| *e > c).unwrap_or(c + 5.0);
            self.time_sel = Some((c, end));
            self.project.loop_region = Some((c, end));
        }
        if pressed(Key::O) {
            let c = self.cursor;
            let start = self.time_sel.map(|s| s.0).filter(|s| *s < c).unwrap_or(0.0);
            self.time_sel = Some((start, c));
            self.project.loop_region = Some((start, c));
        }
        if pressed(Key::Home) || pressed(Key::W) {
            let t = self.time_sel.map(|s| s.0).unwrap_or(0.0);
            self.set_cursor(t);
        }
        if pressed(Key::End) {
            let t = self.time_sel.map(|s| s.1).unwrap_or(self.project.duration());
            self.set_cursor(t);
        }
        if alt && pressed(Key::ArrowLeft) {
            let t = self.project.settings.quantize(self.cursor - fd);
            self.set_cursor(t);
        } else if pressed(Key::ArrowLeft) {
            let t = self.cursor - 1.0 / self.tl.px_per_sec as f64;
            self.set_cursor(t);
        }
        if alt && pressed(Key::ArrowRight) {
            let t = self.project.settings.quantize(self.cursor + fd);
            self.set_cursor(t);
        } else if pressed(Key::ArrowRight) {
            let t = self.cursor + 1.0 / self.tl.px_per_sec as f64;
            self.set_cursor(t);
        }
        if !alt && pressed(Key::ArrowUp) {
            self.tl.zoom_by(1.25, None);
        }
        if !alt && pressed(Key::ArrowDown) {
            self.tl.zoom_by(0.8, None);
        }
        if pressed(Key::J) {
            let s = if self.shuttle < 0.0 { (self.shuttle * 2.0).max(-8.0) } else { -1.0 };
            self.shuttle_play(s);
        }
        if pressed(Key::K) {
            if self.is_playing() {
                self.stop(true);
            }
        }
        if pressed(Key::L) {
            let s = if self.shuttle > 0.0 && self.is_playing() { (self.shuttle * 2.0).min(8.0) } else { 1.0 };
            if s == 1.0 {
                self.play();
            } else {
                self.shuttle_play(s);
            }
        }
        if pressed(Key::F8) {
            self.opts.snapping = !self.opts.snapping;
        }
        if pressed(Key::G) {
            if shift {
                self.select_group();
            } else {
                self.group_selected();
            }
        }
        if pressed(Key::U) {
            self.ungroup_selected();
        }
        if pressed(Key::Z) {
            if let Some(t) = self.sel_track {
                self.checkpoint();
                self.project.tracks[t].mute = !self.project.tracks[t].mute;
                self.changed();
            }
        }
        if pressed(Key::X) {
            if let Some(t) = self.sel_track {
                self.checkpoint();
                self.project.tracks[t].solo = !self.project.tracks[t].solo;
                self.changed();
            }
        }
        if pressed(Key::V) {
            if shift {
                self.remove_envelope(EnvKind::Volume);
            } else {
                self.toggle_envelope_for_selected(EnvKind::Volume, EnvKind::Opacity);
            }
        }
        if pressed(Key::P) {
            if shift {
                self.remove_envelope(EnvKind::Pan);
            } else {
                self.toggle_envelope_for_selected(EnvKind::Pan, EnvKind::FadeToColor);
            }
        }
        if pressed(Key::OpenBracket) {
            if alt {
                self.trim_selected_start_to_cursor();
            }
        }
        if pressed(Key::CloseBracket) {
            if alt {
                self.trim_selected_end_to_cursor();
            }
        }
        if pressed(Key::F2) {
            if let Some(id) = self.sel_events.iter().next().cloned() {
                self.dlg.event_props = Some(id);
            }
        }
        if pressed(Key::Backslash) {
            self.tl.center_on(self.cursor);
        }
        let nums = [Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
        for (i, k) in nums.iter().enumerate() {
            if pressed(*k) {
                let mut ms: Vec<f64> = self.project.markers.iter().map(|m| m.t).collect();
                ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let idx = if i == 0 { 9 } else { i - 1 };
                if let Some(t) = ms.get(idx) {
                    self.set_cursor(*t);
                }
            }
        }
    }

    fn jump_marker(&mut self, forward: bool) {
        let mut pts: Vec<f64> = self.project.markers.iter().map(|m| m.t).collect();
        pts.extend(self.project.regions.iter().flat_map(|r| [r.start, r.end]));
        pts.push(0.0);
        pts.push(self.project.duration());
        pts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let c = self.cursor;
        let t = if forward {
            pts.into_iter().find(|t| *t > c + 1e-6)
        } else {
            pts.into_iter().rev().find(|t| *t < c - 1e-6)
        };
        if let Some(t) = t {
            self.set_cursor(t);
        }
    }

    // -----------------------------------------------------------------
    // Barras de menu e ferramentas
    // -----------------------------------------------------------------

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("Arquivo", |ui| {
                if ui.button("Novo projeto        Ctrl+N").clicked() {
                    self.new_project();
                    ui.close_menu();
                }
                if ui.button("Abrir...            Ctrl+O").clicked() {
                    self.open_project_dialog();
                    ui.close_menu();
                }
                if ui.button("Salvar              Ctrl+S").clicked() {
                    self.save_project(false);
                    ui.close_menu();
                }
                if ui.button("Salvar como...      Ctrl+Shift+S").clicked() {
                    self.save_project(true);
                    ui.close_menu();
                }
                if ui.button("Salvar versão       Ctrl+Alt+S").clicked() {
                    self.save_version();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Importar mídia...   Ctrl+I").clicked() {
                    self.import_dialog();
                    ui.close_menu();
                }
                if ui.button("Importar sequência de imagens...").clicked() {
                    self.import_sequence_dialog();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Renderizar como...  Ctrl+M").clicked() {
                    self.dlg.open_render(&self.project, self.path.as_deref());
                    ui.close_menu();
                }
                if ui.button("Salvar quadro (snapshot) em PNG...").clicked() {
                    self.snapshot_dialog();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Propriedades do projeto...  Alt+Enter").clicked() {
                    self.dlg.project_props = true;
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Sair").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    ui.close_menu();
                }
            });
            ui.menu_button("Editar", |ui| {
                if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Desfazer   Ctrl+Z")).clicked() {
                    self.undo();
                    ui.close_menu();
                }
                if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Refazer    Ctrl+Y")).clicked() {
                    self.redo();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Recortar   Ctrl+X").clicked() {
                    self.cut_selection();
                    ui.close_menu();
                }
                if ui.button("Copiar     Ctrl+C").clicked() {
                    self.copy_selection();
                    ui.close_menu();
                }
                if ui.button("Colar      Ctrl+V").clicked() {
                    self.paste(false);
                    ui.close_menu();
                }
                if ui.button("Colar inserindo (ripple)  Ctrl+Shift+V").clicked() {
                    self.paste(true);
                    ui.close_menu();
                }
                if ui.button("Excluir    Delete").clicked() {
                    self.delete_selection();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Dividir no cursor   S").clicked() {
                    self.split_at_cursor();
                    ui.close_menu();
                }
                if ui.button("Aparar para a seleção   Ctrl+T").clicked() {
                    self.trim_to_selection();
                    ui.close_menu();
                }
                if ui.button("Excluir intervalo selecionado (ripple)").clicked() {
                    self.delete_time_range();
                    ui.close_menu();
                }
                if ui.button("Inserir tempo na seleção").clicked() {
                    self.insert_time();
                    ui.close_menu();
                }
                if ui.button("Fechar lacunas da trilha selecionada").clicked() {
                    self.close_gaps();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Agrupar   G").clicked() {
                    self.group_selected();
                    ui.close_menu();
                }
                if ui.button("Desagrupar   U").clicked() {
                    self.ungroup_selected();
                    ui.close_menu();
                }
                if ui.button("Selecionar todos   Ctrl+A").clicked() {
                    self.sel_events = self.project.tracks.iter().flat_map(|t| t.events.iter().map(|e| e.id)).collect();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Ir para...   Ctrl+G").clicked() {
                    self.dlg.goto_open = true;
                    ui.close_menu();
                }
            });
            ui.menu_button("Exibir", |ui| {
                for tab in DockTab::ALL {
                    if ui.selectable_label(self.dock_tab == tab, tab.name()).clicked() {
                        self.dock_tab = tab;
                        ui.close_menu();
                    }
                }
                ui.separator();
                ui.checkbox(&mut self.opts.show_envelopes, "Mostrar envelopes");
                ui.checkbox(&mut self.opts.show_waveforms, "Mostrar formas de onda");
                ui.checkbox(&mut self.opts.show_thumbs, "Mostrar miniaturas de vídeo");
                ui.checkbox(&mut self.opts.safe_areas, "Áreas de segurança na prévia");
                ui.separator();
                ui.label("Formato de tempo:");
                ui.radio_value(&mut self.opts.time_format, TimeFormat::Timecode, "Timecode SMPTE");
                ui.radio_value(&mut self.opts.time_format, TimeFormat::Seconds, "Tempo (hh:mm:ss.mmm)");
                ui.radio_value(&mut self.opts.time_format, TimeFormat::Frames, "Quadros");
            });
            ui.menu_button("Inserir", |ui| {
                if ui.button("Trilha de vídeo   Ctrl+Shift+Q").clicked() {
                    self.checkpoint();
                    self.insert_track(TrackKind::Video);
                    ui.close_menu();
                }
                if ui.button("Trilha de áudio   Ctrl+Q").clicked() {
                    self.checkpoint();
                    self.insert_track(TrackKind::Audio);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Marcador   M").clicked() {
                    self.insert_marker();
                    ui.close_menu();
                }
                if ui.button("Região   R").clicked() {
                    self.insert_region();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Texto / título...").clicked() {
                    self.insert_generated(GenSpec::Text(TextSpec::default()));
                    ui.close_menu();
                }
                if ui.button("Cor sólida").clicked() {
                    self.insert_generated(GenSpec::SolidColor { color: [0.1, 0.2, 0.5, 1.0] });
                    ui.close_menu();
                }
                ui.separator();
                ui.label("Envelopes da trilha selecionada:");
                if ui.button("Volume / Nível de composição   V").clicked() {
                    self.toggle_envelope_for_selected(EnvKind::Volume, EnvKind::Opacity);
                    ui.close_menu();
                }
                if ui.button("Pan / Fade para cor   P").clicked() {
                    self.toggle_envelope_for_selected(EnvKind::Pan, EnvKind::FadeToColor);
                    ui.close_menu();
                }
            });
            ui.menu_button("Ferramentas", |ui| {
                if ui.button("Efeitos do master bus (áudio)...").clicked() {
                    self.dlg.fx_target = Some(FxTarget::MasterAudio);
                    ui.close_menu();
                }
                if ui.button("Efeitos de saída de vídeo...").clicked() {
                    self.dlg.fx_target = Some(FxTarget::MasterVideo);
                    ui.close_menu();
                }
                ui.separator();
                let has_sel = !self.sel_events.is_empty();
                if ui.add_enabled(has_sel, egui::Button::new("Propriedades do evento...")).clicked() {
                    self.dlg.event_props = self.sel_events.iter().next().cloned();
                    ui.close_menu();
                }
                if ui.add_enabled(has_sel, egui::Button::new("Pan/Crop do evento...")).clicked() {
                    self.dlg.pan_crop = self.sel_events.iter().next().cloned();
                    ui.close_menu();
                }
                if ui.add_enabled(has_sel, egui::Button::new("Efeitos do evento...")).clicked() {
                    self.dlg.fx_target = self.sel_events.iter().next().map(|i| FxTarget::Event(*i));
                    ui.close_menu();
                }
                if ui.add_enabled(has_sel, egui::Button::new("Inverter evento (reverse)")).clicked() {
                    self.toggle_reverse();
                    ui.close_menu();
                }
                if ui.add_enabled(has_sel, egui::Button::new("Congelar quadro no cursor")).clicked() {
                    self.freeze_frame();
                    ui.close_menu();
                }
            });
            ui.menu_button("Opções", |ui| {
                ui.checkbox(&mut self.opts.snapping, "Ativar encaixe (snapping)   F8");
                ui.checkbox(&mut self.opts.quantize, "Quantizar para quadros");
                ui.checkbox(&mut self.opts.auto_ripple, "Ripple automático   Ctrl+L");
                ui.checkbox(&mut self.opts.ignore_grouping, "Ignorar agrupamento de eventos");
                ui.checkbox(&mut self.opts.loop_playback, "Reprodução em loop   Q");
                ui.separator();
                ui.label(format!("Dispositivo de áudio: {} ({} Hz)", self.audio.device_name, self.audio.sr));
            });
            ui.menu_button("Ajuda", |ui| {
                if ui.button("Atalhos de teclado").clicked() {
                    self.dlg.shortcuts = true;
                    ui.close_menu();
                }
                if ui.button("Sobre o NVOV").clicked() {
                    self.dlg.about = true;
                    ui.close_menu();
                }
            });
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("🗋").on_hover_text("Novo projeto (Ctrl+N)").clicked() {
                self.new_project();
            }
            if ui.button("🗁").on_hover_text("Abrir (Ctrl+O)").clicked() {
                self.open_project_dialog();
            }
            if ui.button("💾").on_hover_text("Salvar (Ctrl+S)").clicked() {
                self.save_project(false);
            }
            if ui.button("🎬 Renderizar").on_hover_text("Renderizar como (Ctrl+M)").clicked() {
                self.dlg.open_render(&self.project, self.path.as_deref());
            }
            if ui.button("⚙").on_hover_text("Propriedades do projeto (Alt+Enter)").clicked() {
                self.dlg.project_props = true;
            }
            ui.separator();
            if ui.button("➕ Importar").on_hover_text("Importar mídia (Ctrl+I)").clicked() {
                self.import_dialog();
            }
            ui.separator();
            if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("↶")).on_hover_text("Desfazer (Ctrl+Z)").clicked() {
                self.undo();
            }
            if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("↷")).on_hover_text("Refazer (Ctrl+Y)").clicked() {
                self.redo();
            }
            ui.separator();
            if ui.button("✂ Dividir").on_hover_text("Dividir no cursor (S)").clicked() {
                self.split_at_cursor();
            }
            if ui.button("🗑").on_hover_text("Excluir (Delete)").clicked() {
                self.delete_selection();
            }
            ui.separator();
            ui.toggle_value(&mut self.opts.snapping, "🧲 Encaixe").on_hover_text("Ativar encaixe (F8)");
            ui.toggle_value(&mut self.opts.auto_ripple, "⇥ Ripple").on_hover_text("Ripple automático (Ctrl+L)");
            ui.toggle_value(&mut self.opts.ignore_grouping, "⛓ Ignorar grupos");
            ui.toggle_value(&mut self.opts.show_envelopes, "〰 Envelopes");
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
            if ui.button("⏮").on_hover_text("Início do projeto (Ctrl+Home)").clicked() {
                self.set_cursor(0.0);
            }
            if ui.button("⏪").on_hover_text("Quadro anterior (Alt+←)").clicked() {
                let t = self.project.settings.quantize(self.cursor - self.project.settings.frame_dur());
                self.set_cursor(t);
            }
            if ui.button(if playing { "⏸" } else { "▶" }).on_hover_text("Reproduzir/Pausar (Enter)").clicked() {
                if playing {
                    self.stop(true);
                } else {
                    self.play();
                }
            }
            if ui.button("⏹").on_hover_text("Parar (Espaço/Esc)").clicked() {
                self.stop(false);
            }
            if ui.button("⏩").on_hover_text("Próximo quadro (Alt+→)").clicked() {
                let t = self.project.settings.quantize(self.cursor + self.project.settings.frame_dur());
                self.set_cursor(t);
            }
            if ui.button("⏭").on_hover_text("Fim do projeto (Ctrl+End)").clicked() {
                let d = self.project.duration();
                self.set_cursor(d);
            }
            ui.toggle_value(&mut self.opts.loop_playback, "🔁").on_hover_text("Reprodução em loop (Q)");
            ui.separator();
            let t = self.view_time();
            ui.label(egui::RichText::new(self.fmt_time(t)).monospace().size(20.0).color(Color32::from_rgb(120, 230, 140)));
            if let Some((a, b)) = self.time_sel {
                ui.label(
                    egui::RichText::new(format!("Seleção: {} → {} ({})", self.fmt_time(a), self.fmt_time(b), self.fmt_time(b - a)))
                        .monospace()
                        .small(),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut z = self.tl.px_per_sec.ln();
                if ui
                    .add(egui::Slider::new(&mut z, (2.0f32).ln()..=(2000.0f32).ln()).show_value(false))
                    .on_hover_text("Zoom horizontal (↑/↓, roda do mouse)")
                    .changed()
                {
                    self.tl.px_per_sec = z.exp();
                }
                ui.label("Zoom");
                if self.shuttle != 0.0 && (self.shuttle - 1.0).abs() > 1e-6 && playing {
                    ui.label(format!("Shuttle {:+.0}x", self.shuttle));
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
                    "{}x{} {:.3} qps | {} Hz | prévia {:.0} ms | {}",
                    s.width,
                    s.height,
                    s.fps,
                    s.sample_rate,
                    self.preview_ms,
                    self.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "(sem título)".into())
                ));
                if let Some(job) = &self.render_job {
                    ui.add(egui::ProgressBar::new(job.progress.fraction()).desired_width(140.0).show_percentage());
                    ui.label("Renderizando:");
                }
            });
        });
    }

    fn snapshot_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name("quadro.png").save_file() {
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
        let files: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if files.is_empty() {
            return;
        }
        let t = self.cursor;
        let projects: Vec<PathBuf> = files.iter().filter(|p| p.extension().map(|e| e == "nvp").unwrap_or(false)).cloned().collect();
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
        let title = format!("{}{} - NVOV", name, if self.dirty { "*" } else { "" });
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }

    fn update_meters(&mut self, dt: f32) {
        let m = self.audio.shared.lock().unwrap().meters.clone();
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
            if job.progress.finished.load(std::sync::atomic::Ordering::SeqCst) {
                let err = job.progress.error.lock().unwrap().clone();
                let secs = *job.progress.elapsed.lock().unwrap();
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
            if !self.opts.loop_playback && t > self.project.duration() + 0.5 && self.shuttle >= 0.0 {
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
        egui::TopBottomPanel::bottom("dock")
            .resizable(true)
            .default_height(330.0)
            .min_height(160.0)
            .show(ctx, |ui| self.dock_area(ui));
        egui::TopBottomPanel::bottom("transport").show(ctx, |ui| self.transport_bar(ui));
        egui::CentralPanel::default().show(ctx, |ui| self.timeline_ui(ui));

        self.show_dialogs(ctx);
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

fn setup_style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::from_rgb(38, 38, 42);
    v.window_fill = Color32::from_rgb(44, 44, 48);
    v.extreme_bg_color = Color32::from_rgb(24, 24, 26);
    v.selection.bg_fill = Color32::from_rgb(60, 110, 190);
    ctx.set_visuals(v);
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    ctx.set_style(style);
}
