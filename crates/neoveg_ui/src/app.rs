//! Interface gráfica (`eframe` + `wgpu`) no estilo do VEGAS Pro: timeline com
//! lista de trilhas em cima, área de janelas acopláveis embaixo e prévia.

use crate::icons::{self, Icon};
use crate::layout::{self, WindowLayout};
use crate::theme::{self, ThemeColors, ThemeId};
use eframe::egui;
use egui::{Key, TextureHandle};
pub use neoveg_common::prefs::{
    EditLayoutPrefs, LayoutType, PlaybackScrollMode, Preferences, PreviewQuality,
    TimelineMouseScroll,
};
use neoveg_common::{tr, tr_args, Language};
use neoveg_engine::commands::{
    self, is_command_enabled, menu_label_in, resolved_shortcut, run_command, CommandHost,
    DockPanel, ShortcutKey, UiAction, COMMANDS,
};
use neoveg_engine::engine::loudness::{LoudnessReport, LoudnessSnapshot, LoudnessTarget};
use neoveg_engine::media::{self, MediaCache};
use neoveg_engine::model::*;
use neoveg_engine::otio;
use neoveg_engine::playback::{AudioOut, Preview, PreviewRequest};
use neoveg_engine::recovery;
use neoveg_engine::render::{RenderProgress, RenderSettings};
pub use neoveg_engine::session::{ClipItem, EditorOptions as Options, EditorSession};
use neoveg_engine::util;
use std::collections::{HashMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

pub use crate::dialogs::Dialogs;
pub use crate::timeline::TimelineState;
pub use neoveg_engine::commands::DockPanel as DockTab;

/// Comandos do menu `More (...)` da barra de transporte, em grupos separados
/// por divisória. Só ids da Tabela Única (§3.3); o teste abaixo garante isso.
pub(crate) const MORE_TOOLS_COMMANDS: &[&[&str]] = &[
    &[
        "edit.paste_attributes",
        "edit.paste_attributes_selective",
        "edit.smart_split",
        "edit.heal_events",
        "edit.detect_scenes",
        "edit.expanded_edit_mode",
        "edit.trim_to_selection",
        "edit.trim_start",
        "edit.trim_end",
        "edit.delete_range_ripple",
        "edit.insert_time",
        "edit.close_gaps",
    ],
    &["edit.group", "edit.ungroup", "edit.select_group"],
    &[
        "event.sync_link_create",
        "event.sync_link_select",
        "event.sync_link_unlink",
    ],
    &[
        "event.take_chooser",
        "event.pitch_up",
        "event.pitch_down",
        "tools.reverse",
        "tools.freeze_frame",
        "tools.event_props",
    ],
    &[
        "insert.empty_event",
        "insert.track_folder",
        "track.toggle_folder",
        "track.make_child",
        "tools.multicam_mode",
        "view.stereo_3d",
        "track.record_arm",
        "transport.record",
        "media.select_events",
        "preview.copy_frame",
        "preview.ram_build",
        "preview.external_monitor",
        "insert.marker",
        "insert.region",
        "transport.jump_prev_marker",
        "transport.jump_next_marker",
    ],
];

#[cfg(test)]
mod more_tools_tests {
    use super::MORE_TOOLS_COMMANDS;

    #[test]
    fn more_tools_ids_exist_in_command_table() {
        for id in MORE_TOOLS_COMMANDS.iter().flat_map(|g| g.iter()) {
            assert!(
                neoveg_engine::commands::find_command(id).is_some(),
                "id ausente na Tabela Única: {id}"
            );
        }
    }

    #[test]
    fn more_tools_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for id in MORE_TOOLS_COMMANDS.iter().flat_map(|g| g.iter()) {
            assert!(seen.insert(*id), "id repetido no menu More: {id}");
        }
    }
}

/// Itens arrastáveis entre janelas (drag and drop).
#[derive(Clone, Debug)]
pub enum DndPayload {
    Media(Id),
    Fx(FxKind),
    FxPreset(FxInstance),
    Transition(TransitionKind),
    Generator(GenSpec),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FxTarget {
    Event(Id),
    Track(Id),
    AudioBus(Id),
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
    Replaced(Id, Result<media::Imported, String>),
    ProxyBuilt(Id, Result<PathBuf, String>),
    RamPreviewFrame {
        gen: u64,
        frame_idx: i64,
        image: image::RgbaImage,
    },
    RamPreviewDone {
        gen: u64,
    },
    PrerenderDone {
        start: Ticks,
        end: Ticks,
        result: Result<PathBuf, String>,
    },
    LoudnessAnalyzed(Result<LoudnessReport, String>),
    /// A importação de um arquivo falhou (pode ser oferecida a conversão).
    ImportFailed {
        path: PathBuf,
        error: String,
        place: Option<PlaceTarget>,
    },
    /// Um arquivo da fila de conversão terminou (com sucesso ou não).
    Converted {
        src: PathBuf,
        result: Result<media::convert::Converted, String>,
        place: Option<PlaceTarget>,
    },
    Error(String),
}

pub struct LoudnessJob {
    pub cancel: Arc<AtomicBool>,
    pub done: Arc<AtomicU64>,
    pub total: Arc<AtomicU64>,
}

pub struct RenderJob {
    pub progress: Arc<RenderProgress>,
    pub settings: RenderSettings,
}

/// Resultado de um script .NET que rodou em segundo plano.
enum ScriptOutcome {
    Done(Result<Vec<String>, String>),
    /// Pânico isolado na ponte (§0.6); a sessão da interface não mudou.
    Panicked,
}

/// Script .NET em execução. Ele roda numa thread própria, sobre uma cópia da
/// sessão: a interface continua viva (e atende as janelas do WinForms que o
/// script abre, que travariam com a interface parada), mas a edição fica
/// bloqueada até o fim, quando a cópia vira a sessão (§0, nada pela metade).
pub(crate) struct ScriptJob {
    path: PathBuf,
    started: std::time::Instant,
    media_before: std::collections::HashSet<u64>,
    rx: std::sync::mpsc::Receiver<(ScriptOutcome, EditorSession)>,
    /// Pedidos de janela da camada de Forms própria (Linux, macOS).
    ui_rx: std::sync::mpsc::Receiver<crate::script_forms::UiCall>,
    /// Janela do script aberta agora.
    active: Option<crate::script_forms::Active>,
}

#[cfg(test)]
impl ScriptJob {
    /// Script "em andamento" para testes: termina com sucesso, devolvendo a
    /// sessão dada, quando o teste manda `()` pelo canal devolvido.
    pub(crate) fn fake(session: EditorSession) -> (Self, std::sync::mpsc::Sender<()>) {
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if done_rx.recv().is_ok() {
                let _ = tx.send((ScriptOutcome::Done(Ok(Vec::new())), session));
            }
        });
        let job = Self {
            path: PathBuf::from("teste.cs"),
            started: std::time::Instant::now(),
            media_before: std::collections::HashSet::new(),
            rx,
            // Sem remetente: o script de teste não abre janelas.
            ui_rx: std::sync::mpsc::channel().1,
            active: None,
        };
        (job, done_tx)
    }
}

pub struct RamPreviewJob {
    pub cancel: Arc<AtomicBool>,
    pub done: Arc<AtomicU64>,
    pub total: Arc<AtomicU64>,
    pub gen: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct PreviewReqKey {
    t: Ticks,
    gen: u64,
    w: usize,
    h: usize,
    scopes: bool,
    prefer_proxy: bool,
    split_mode: neoveg_engine::playback::PreviewSplitMode,
    mute_video: bool,
    expanded_ab: Option<(Option<Id>, Ticks, Option<Id>, Ticks)>,
}

pub struct App {
    pub session: EditorSession,
    pub prefs: Preferences,
    pub media: Arc<MediaCache>,
    pub audio: AudioOut,
    pub(crate) preview: Preview,
    pub(crate) preview_tex: Option<TextureHandle>,
    pub(crate) preview_split_tex: Option<TextureHandle>,
    pub(crate) preview_clipboard: Option<(TextureHandle, image::RgbaImage)>,
    pub(crate) ram_preview: neoveg_engine::playback::RamPreviewCache,
    pub(crate) ram_preview_job: Option<RamPreviewJob>,
    pub(crate) preview_zoom: f32,
    pub(crate) preview_pan: egui::Vec2,
    pub external_monitor: bool,
    pub(crate) external_monitor_fullscreen: bool,
    pub(crate) expanded_ab_tex: Option<(TextureHandle, TextureHandle)>,
    pub(crate) multicam_thumbs: HashMap<(Id, usize, i64), TextureHandle>,
    pub(crate) preview_inflight: bool,
    pub(crate) preview_inflight_since: Instant,
    pub(crate) last_req: PreviewReqKey,
    pub(crate) preview_ms: f64,
    pub(crate) scopes_tex: Option<[TextureHandle; 4]>,
    pub tl: TimelineState,
    pub dock_tab: DockTab,
    pub detached_tabs: HashSet<DockTab>,
    /// Painéis fechados (fora da área de janelas); voltam pelo menu Exibir.
    pub closed_tabs: HashSet<DockTab>,
    pub preview_detached: bool,
    pub preview_visible: bool,
    pub master_bus_visible: bool,
    pub master_bus_detached: bool,
    pub master_dim: bool,
    pub master_mono: bool,
    pub dock_visible: bool,
    pub dock_height: f32,
    pub preview_ratio: f32,
    pub dock_tree: egui_tiles::Tree<crate::dock::DockPane>,
    /// Painel que preenche a área de janelas (botão Maximizar do VEGAS).
    pub dock_maximized: Option<crate::dock::DockPane>,
    /// Estado de antes do `Ctrl+F11` (timeline maximizada).
    pub(crate) timeline_restore: Option<crate::dock::TimelineRestore>,
    pub(crate) pending_dock_focus: Option<DockTab>,
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
    pub(crate) script_job: Option<ScriptJob>,
    /// Janela Efeitos de Vídeo (categorias, busca e miniaturas).
    pub(crate) fx_browser: crate::fx_browser::FxBrowser,
    /// Janela Geradores de Mídia (categorias, busca e miniaturas).
    pub(crate) gen_browser: crate::gen_browser::GenBrowser,
    pub(crate) prerender_job: Option<(Ticks, Ticks, Arc<RenderProgress>)>,
    pub(crate) window_icon: Arc<egui::IconData>,
    pub(crate) last_frame: Instant,
    pub(crate) meters_hold: HashMap<Id, [f32; 2]>,
    pub(crate) master_hold: [f32; 2],
    pub loudness_target: LoudnessTarget,
    pub loudness_live: LoudnessSnapshot,
    pub loudness_report: Option<LoudnessReport>,
    pub loudness_job: Option<LoudnessJob>,
    pub trimmer: crate::trimmer_panel::TrimmerState,
    pub edit_details: crate::edit_details::EditDetailsState,
    pub(crate) confirm_close: bool,
    pub(crate) allow_close: bool,
    pub shuttle: f64,
    pub gen_fonts: Vec<String>,
    pub(crate) last_autosave: Instant,
    pub(crate) recovery_offer: bool,
    /// Sessões anteriores que não fecharam normalmente (janela de relatório).
    pub(crate) crash_reports: Vec<neoveg_engine::session_marker::CrashReport>,
    pub(crate) splash: crate::splash::SplashState,
    /// A janela ainda está no modo tela de abertura (sem bordas, tamanho da abertura).
    pub(crate) startup_window: bool,
    pub(crate) welcome: crate::welcome::WelcomeState,
    pub(crate) welcome_open: bool,
    pub(crate) pending_welcome_after_splash: bool,
    pub(crate) integrations: crate::integrations_ui::IntegrationsState,
    pub(crate) encoder: crate::encoder_panel::EncoderPanelState,
    pub(crate) recorder: Option<neoveg_engine::playback::AudioRecorder>,
    pub(crate) recorder_tracks: Vec<Id>,
    pub(crate) recording_peaks: HashMap<Id, [f32; 2]>,
    pub project_media: crate::project_media::ProjectMediaState,
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

/// Aplica os padrões globais de edição/prévia das Preferências às opções da sessão.
pub(crate) fn apply_pref_defaults(opts: &mut Options, prefs: &Preferences) {
    opts.snapping = prefs.default_snapping;
    opts.quantize = prefs.default_quantize;
    opts.show_waveforms = prefs.default_show_waveforms;
    opts.show_thumbs = prefs.default_show_thumbs;
    opts.show_envelopes = prefs.default_show_envelopes;
    opts.loop_playback = prefs.default_loop_playback;
    opts.preview_quality = prefs.default_preview_quality;
    opts.time_format = prefs.default_time_format;
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>) -> Self {
        Self::with_context(&cc.egui_ctx, open)
    }

    pub fn with_context(egui_ctx: &egui::Context, open: Option<PathBuf>) -> Self {
        Self::with_context_in(egui_ctx, open, None)
    }

    pub fn with_context_in(
        egui_ctx: &egui::Context,
        open: Option<PathBuf>,
        lang_override: Option<Language>,
    ) -> Self {
        let mut prefs = Preferences::load_or_default();
        if let Some(ov) = lang_override {
            prefs.set_lang(ov);
        }
        let lang = prefs.lang();
        neoveg_common::set_active_language(lang);
        // Ctrl+= / Ctrl+- / Ctrl+0 são atalhos do VEGAS (tom em oitavas);
        // o zoom da interface do egui não pode tomar essas teclas.
        egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        // nos testes fica a fonte embutida, para as medidas de layout serem
        // as mesmas em qualquer máquina
        crate::fonts::configure_fonts(egui_ctx, prefs.system_ui_font && !cfg!(test));
        let theme_id = ThemeId::from_pref(&prefs.theme);
        let theme = theme::apply_theme(egui_ctx, theme_id);
        let loaded_layout = prefs
            .window_layout
            .as_deref()
            .and_then(|p| layout::load_layout_file(std::path::Path::new(p)).ok());
        if let Some(el) = loaded_layout.as_ref().and_then(|l| l.edit_layout.clone()) {
            prefs.edit_layout = el;
        }
        let master_bus_visible = loaded_layout
            .as_ref()
            .map(|l| l.master_bus_visible)
            .unwrap_or(prefs.edit_layout.master_bus_docked);
        let initial_layout = loaded_layout.unwrap_or_else(|| layout::WindowLayout {
            dock_height: prefs.edit_layout.layout_type.default_dock_height(),
            preview_ratio: prefs.edit_layout.layout_type.default_preview_ratio(),
            master_bus_visible,
            ..Default::default()
        });
        let initial_tree = initial_layout.tile_tree.clone();
        let initial_tab = initial_layout.active_panel();
        let initial_pv_left = prefs.edit_layout.preview_on_left;
        let saved_layouts = layout::list_saved_layouts();
        let mut initial_project = Project::new_default();
        initial_project.settings.sample_rate = prefs.default_sample_rate;
        let mut session = EditorSession::new_in(initial_project, lang);
        apply_pref_defaults(&mut session.opts, &prefs);
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
        let recovery_offer = recovery::find_existing_recovery().is_some()
            && !neoveg_engine::session_marker::other_instance_active();
        let splash = crate::splash::SplashState::startup(prefs.show_splash && !recovery_offer);
        let want_welcome = prefs.show_welcome && open.is_none() && !recovery_offer;
        let welcome_open = want_welcome && !splash.active;
        let pending_welcome_after_splash = want_welcome && splash.active;
        let mut app = App {
            session,
            prefs,
            media,
            audio,
            preview,
            preview_tex: None,
            preview_split_tex: None,
            preview_clipboard: None,
            ram_preview: neoveg_engine::playback::RamPreviewCache::default(),
            ram_preview_job: None,
            preview_zoom: 1.0,
            preview_pan: egui::Vec2::ZERO,
            external_monitor: false,
            external_monitor_fullscreen: false,
            expanded_ab_tex: None,
            multicam_thumbs: HashMap::new(),
            preview_inflight: false,
            preview_inflight_since: Instant::now(),
            last_req: PreviewReqKey {
                t: Ticks::from_raw(-1),
                ..Default::default()
            },
            detached_tabs: initial_layout.detached_panels(),
            closed_tabs: initial_layout.closed_panels(),
            preview_detached: initial_layout.preview_detached,
            preview_visible: initial_layout.preview_visible,
            master_bus_visible,
            master_bus_detached: initial_layout.master_bus_detached,
            master_dim: false,
            master_mono: false,
            dock_visible: initial_layout.dock_visible,
            dock_maximized: None,
            timeline_restore: None,
            dock_height: initial_layout.dock_height,
            preview_ratio: initial_layout.preview_ratio,
            dock_tree: initial_tree.unwrap_or_else(|| {
                crate::dock::build_dock_tree(
                    initial_layout.preview_ratio,
                    initial_tab,
                    initial_pv_left,
                )
            }),
            pending_dock_focus: None,
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
            script_job: None,
            fx_browser: Default::default(),
            gen_browser: Default::default(),
            prerender_job: None,
            window_icon: Arc::new(crate::splash::window_icon_data()),
            last_frame: Instant::now(),
            meters_hold: HashMap::new(),
            master_hold: [0.0; 2],
            loudness_target: LoudnessTarget::default(),
            loudness_live: LoudnessSnapshot::default(),
            loudness_report: None,
            loudness_job: None,
            trimmer: crate::trimmer_panel::TrimmerState::default(),
            edit_details: crate::edit_details::EditDetailsState::default(),
            confirm_close: false,
            allow_close: false,
            shuttle: 0.0,
            gen_fonts: vec![],
            last_autosave: Instant::now(),
            recovery_offer,
            crash_reports: Vec::new(),
            splash,
            startup_window: false,
            welcome: crate::welcome::WelcomeState::default(),
            welcome_open,
            pending_welcome_after_splash,
            integrations: crate::integrations_ui::IntegrationsState::default(),
            encoder: crate::encoder_panel::EncoderPanelState::load_from_disk(),
            recorder: None,
            recorder_tracks: Vec::new(),
            recording_peaks: HashMap::new(),
            project_media: crate::project_media::ProjectMediaState::default(),
        };
        app.ram_preview =
            neoveg_engine::playback::RamPreviewCache::new(app.prefs.ram_preview_mb as usize);
        if let Some(e) = app.preview.error.clone() {
            app.set_status(e);
        }
        if let Some(e) = &app.audio.error {
            let err_s = e.to_string();
            app.set_status(tr_args(
                lang,
                "Áudio: {err} (reprodução sem som)",
                &[("err", &err_s)],
            ));
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

    /// Idioma ativo na interface e na sessão de edição.
    pub fn lang(&self) -> Language {
        self.prefs.lang()
    }

    /// Altera o idioma da interface em tempo real e opcionalmente grava nas preferências.
    pub(crate) fn apply_language(
        &mut self,
        ctx: Option<&egui::Context>,
        lang: Language,
        save_prefs: bool,
    ) {
        self.prefs.set_lang(lang);
        self.session.lang = lang;
        neoveg_common::set_active_language(lang);
        if save_prefs {
            if let Err(e) = self.prefs.save() {
                let err_s = e.to_string();
                self.set_status(tr_args(
                    lang,
                    "Aviso ao salvar preferência: {err}",
                    &[("err", &err_s)],
                ));
            }
        }
        if let Some(c) = ctx {
            c.request_repaint();
        }
    }

    // -----------------------------------------------------------------
    // Estado / desfazer
    // -----------------------------------------------------------------

    pub fn is_splash_open(&self) -> bool {
        self.splash.active
    }

    pub fn open_splash(&mut self) {
        self.splash.open_manual();
    }

    pub fn dismiss_splash(&mut self) {
        self.splash.dismiss();
        if self.pending_welcome_after_splash && !self.recovery_offer {
            self.pending_welcome_after_splash = false;
            self.welcome_open = true;
        }
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.session.set_status(s);
        self.status_at = Instant::now();
        let st = &self.session.status;
        let is_err = st.starts_with("Erro")
            || st.starts_with("Error")
            || st.starts_with("Erreur")
            || st.starts_with("Fehler")
            || st.starts_with("エラー")
            || st.contains("falh")
            || st.contains("fail")
            || st.contains("失敗");
        if is_err {
            util::log_error(&self.session.status);
        }
    }

    pub fn sync_status_from_session(&mut self) {
        self.status_at = Instant::now();
    }

    pub fn sync_after_session(&mut self) {
        self.sync_status_from_session();
        self.sync_master_monitor_audio();
        let removed = std::mem::take(&mut self.session.removed_media_ids);
        for id in removed {
            self.thumbs.remove(&id);
            self.project_media.invalidate_media(id);
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

    pub fn open_motion_track_dialog(&mut self) {
        let candidate = self
            .sel_events
            .iter()
            .copied()
            .find(|&eid| {
                self.project.find_event(eid).is_some_and(|(ti, ei)| {
                    self.project.tracks[ti].kind == TrackKind::Video
                        && self
                            .project
                            .media(self.project.tracks[ti].events[ei].media_id)
                            .is_some_and(|m| m.has_video)
                })
            })
            .or_else(|| {
                self.sel_track
                    .and_then(|ti| self.project.tracks.get(ti))
                    .filter(|t| t.kind == TrackKind::Video && !t.is_folder)
                    .and_then(|t| {
                        t.events.iter().find(|e| {
                            e.contains(self.cursor)
                                && self.project.media(e.media_id).is_some_and(|m| m.has_video)
                        })
                    })
                    .map(|e| e.id)
            });
        let Some(ev_id) = candidate else {
            self.set_status("Selecione um evento de vídeo para rastrear o movimento.");
            return;
        };
        let track_id = self
            .project
            .find_event(ev_id)
            .map(|(ti, _)| self.project.tracks[ti].id);
        self.dlg.motion_track_src = Some(ev_id);
        self.dlg.motion_track_target_event = Some(ev_id);
        if self.dlg.motion_track_target_track.is_none() {
            self.dlg.motion_track_target_track = track_id;
        }
        self.dlg.motion_track_step = self.dlg.motion_track_step.max(1);
    }

    pub fn apply_audio_fx_non_realtime(&mut self) {
        let dir = std::env::temp_dir().join("neoveg_audio_fx_bounce");
        match self.session.apply_audio_fx_non_realtime(&self.media, &dir) {
            Ok(_) => self.sync_after_session(),
            Err(e) => self.set_status(format!("Aplicar efeitos de áudio: {e}")),
        }
    }

    /// Salvamento automático para recuperação após falha (§0.7).
    fn autosave(&mut self) {
        if self.recovery_offer
            || !self.prefs.autosave_enabled
            || !self.dirty
            || self.last_autosave.elapsed().as_secs() < self.prefs.autosave_secs.clamp(15, 600)
        {
            return;
        }
        self.last_autosave = Instant::now();
        if let Err(e) = recovery::write_recovery(&self.project) {
            let err_s = e.to_string();
            self.set_status(tr_args(
                self.lang(),
                "Falha no salvamento automático: {err}",
                &[("err", &err_s)],
            ));
        }
    }

    fn recovery_window(&mut self, ctx: &egui::Context) {
        // com relatório de travamento, a recuperação é oferecida na janela dele
        if !self.recovery_offer || !self.crash_reports.is_empty() {
            return;
        }
        let lang = self.lang();
        let mut choice = None;
        egui::Window::new(tr(lang, "Recuperar projeto"))
            .id(egui::Id::new("recovery_offer_win"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(tr(
                    lang,
                    "Foi encontrado um projeto salvo automaticamente de uma sessão anterior que não terminou normalmente.",
                ));
                ui.horizontal(|ui| {
                    if ui.button(tr(lang, "Recuperar")).clicked() {
                        choice = Some(true);
                    }
                    if ui.button(tr(lang, "Descartar")).clicked() {
                        choice = Some(false);
                    }
                });
            });
        if let Some(recover) = choice {
            self.resolve_recovery(recover);
        }
    }

    /// Recupera (`true`) ou descarta (`false`) a cópia automática da sessão
    /// anterior.
    pub(crate) fn resolve_recovery(&mut self, recover: bool) {
        let lang = self.lang();
        self.recovery_offer = false;
        if recover {
            if let Some(p) = recovery::find_existing_recovery() {
                self.open_project_path(p);
                self.path = None;
                self.dirty = true;
                self.set_status(tr(lang, "Projeto recuperado. Salve-o com Salvar como..."));
            }
        } else {
            recovery::clear_recovery();
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
        s.reset_dsp();
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
        s.reset_dsp();
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
            s.reset_dsp();
            s.pos
        };
        self.shuttle = 0.0;
        if keep {
            self.cursor = Ticks::from_samples(pos.max(0), self.audio.sr);
        } else {
            self.cursor = self.play_start;
        }
    }

    /// `Ctrl+F11`: maximiza a timeline escondendo a área de janelas e a lista
    /// de trilhas; de novo, volta ao que estava antes (manual do VEGAS Pro 21).
    pub fn toggle_maximize_timeline(&mut self) {
        match self.timeline_restore.take() {
            Some(r) => {
                self.dock_visible = r.dock_visible;
                if r.header_expanded && self.tl.header_collapsed() {
                    self.tl.toggle_header_collapse();
                }
            }
            None => {
                self.timeline_restore = Some(crate::dock::TimelineRestore {
                    dock_visible: self.dock_visible,
                    header_expanded: !self.tl.header_collapsed(),
                });
                self.dock_visible = false;
                if !self.tl.header_collapsed() {
                    self.tl.toggle_header_collapse();
                }
            }
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

    /// Destino do cursor para uma seta (`dir` < 0: esquerda). Anda um pixel
    /// da régua; com Quantizar para quadros ligado, para na grade de quadros e
    /// anda pelo menos um quadro, para que a agulha nunca fique parada no
    /// mesmo quadro com o zoom aberto.
    pub(crate) fn arrow_step_target(&self, dir: i64) -> Ticks {
        let px = Ticks::from_secs_f64_clamped(
            1.0 / f64::from(
                self.tl
                    .px_per_sec
                    .max(crate::timeline::TimelineState::MIN_PX_PER_SEC),
            ),
        )
        .max(Ticks::from_raw(1));
        let cur = self.cursor;
        let raw = if dir < 0 { cur - px } else { cur + px };
        if !self.opts.quantize {
            return raw.max(Ticks::ZERO);
        }
        let fps = self.project.settings.fps;
        let cur_f = fps.ticks_to_frame_round(cur);
        let raw_f = fps.ticks_to_frame_round(raw);
        let f = if dir < 0 {
            raw_f.min(cur_f.saturating_sub(1))
        } else {
            raw_f.max(cur_f.saturating_add(1))
        };
        fps.frame_to_ticks(f.max(0)).max(Ticks::ZERO)
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
            s.reset_dsp();
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
        let mut proj = Project::new_default();
        proj.settings.sample_rate = self.prefs.default_sample_rate;
        self.session.reset_project(proj, None);
        apply_pref_defaults(&mut self.session.opts, &self.prefs);
        self.thumbs.clear();
        self.project_media.clear_thumbs();
        self.set_status(tr(self.lang(), "Novo projeto"));
    }

    pub fn open_project_dialog(&mut self) {
        let lang = self.lang();
        if let Some(p) = rfd::FileDialog::new()
            .add_filter(tr(lang, "Projeto Neoveg"), &["neoveg", "nvp"])
            .pick_file()
        {
            self.open_project_path(p);
        }
    }

    fn reload_all_media(&mut self) {
        self.thumbs.clear();
        self.project_media.clear_thumbs();
        let items = self.project.media.clone();
        for m in items {
            self.media.remove(m.id);
            self.spawn_media_load(m);
        }
    }

    /// Abre em segundo plano a mídia de um item do projeto; o resultado
    /// volta como `LoadMsg::Reloaded`.
    fn spawn_media_load(&mut self, m: MediaItem) {
        {
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
    }

    pub fn open_project_path(&mut self, path: PathBuf) {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_low = ext.to_ascii_lowercase();
            match ext_low.as_str() {
                "otio" => {
                    self.import_otio_path(&path);
                    return;
                }
                "edl" => {
                    self.import_edl_path(&path);
                    return;
                }
                "fcpxml" | "xml" => {
                    self.import_fcpxml_path(&path);
                    return;
                }
                "aaf" => {
                    self.import_aaf_path(&path);
                    return;
                }
                "srt" | "vtt" => {
                    self.import_subtitles_path(&path);
                    return;
                }
                _ => {}
            }
        }
        // o arquivo de projeto é entrada hostil: é validado e corrigido (§0.4)
        let lang = self.lang();
        let res = Project::load_file(&path);
        match res {
            Ok((p, fixes)) => {
                self.stop(false);
                for f in &fixes {
                    util::log_warn(&format!("projeto corrigido ao abrir: {f}"));
                }
                self.session.reset_project(p, Some(path.clone()));
                self.reload_all_media();
                let path_s = path.display().to_string();
                if fixes.is_empty() {
                    self.set_status(tr_args(
                        lang,
                        "Projeto aberto: {path}",
                        &[("path", &path_s)],
                    ));
                } else {
                    let fixes_s = fixes.len().to_string();
                    self.set_status(tr_args(
                        lang,
                        "Projeto aberto com {n} correção(ões) de dados inválidos (veja logs/neoveg.log)",
                        &[("n", &fixes_s)],
                    ));
                }
                if path != recovery::recovery_path() && path != recovery::legacy_recovery_path() {
                    self.prefs.record_recent_project(&path);
                    let _ = self.prefs.save();
                }
            }
            Err(e) => {
                let err_s = e.to_string();
                self.set_status(tr_args(
                    lang,
                    "Erro ao abrir projeto: {err}",
                    &[("err", &err_s)],
                ));
            }
        }
    }

    fn apply_imported_project(
        &mut self,
        label: &str,
        path: &std::path::Path,
        res: Result<(Project, Vec<String>), neoveg_common::error::NeovegError>,
    ) {
        let lang = self.lang();
        let path_s = path.display().to_string();
        match res {
            Ok((p, warnings)) => {
                self.stop(false);
                for w in &warnings {
                    util::log_warn(&format!("{label}: {w}"));
                }
                self.session.reset_project(p, None);
                self.reload_all_media();
                if warnings.is_empty() {
                    self.set_status(tr_args(
                        lang,
                        "{label} importado: {path}",
                        &[("label", &label), ("path", &path_s)],
                    ));
                } else {
                    let n_s = warnings.len().to_string();
                    self.set_status(tr_args(
                        lang,
                        "{label} importado com {n} aviso(s): {path}",
                        &[("label", &label), ("n", &n_s), ("path", &path_s)],
                    ));
                }
            }
            Err(e) => {
                let err_s = e.to_string();
                self.set_status(tr_args(
                    lang,
                    "Erro ao importar {label}: {err}",
                    &[("label", &label), ("err", &err_s)],
                ));
            }
        }
    }

    fn export_interchange_path(&self, ext: &str) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .map(|s| format!("{s}.{ext}"))
            .unwrap_or_else(|| format!("projeto.{ext}"))
    }

    pub fn import_otio_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("OpenTimelineIO", &["otio", "json"])
            .pick_file()
        {
            self.import_otio_path(&p);
        }
    }

    pub fn import_otio_path(&mut self, path: &std::path::Path) {
        let res = otio::import_otio_file(path);
        self.apply_imported_project("OTIO", path, res);
    }

    pub fn export_otio_dialog(&mut self) {
        let lang = self.lang();
        let default_name = self.export_interchange_path("otio");
        let Some(p) = rfd::FileDialog::new()
            .add_filter("OpenTimelineIO", &["otio"])
            .set_file_name(&default_name)
            .save_file()
        else {
            return;
        };
        self.session.project.sort_events();
        match otio::export_otio_file(&self.session.project, &p) {
            Ok(()) => {
                let p_s = p.display().to_string();
                self.set_status(tr_args(lang, "OTIO exportado: {path}", &[("path", &p_s)]));
            }
            Err(e) => {
                let err_s = e.to_string();
                self.set_status(tr_args(
                    lang,
                    "Erro ao exportar OTIO: {err}",
                    &[("err", &err_s)],
                ));
            }
        }
    }

    pub fn import_subtitles_dialog(&mut self) {
        let lang = self.lang();
        if let Some(p) = rfd::FileDialog::new()
            .add_filter(tr(lang, "Legendas"), &["srt", "vtt"])
            .pick_file()
        {
            self.import_subtitles_path(&p);
        }
    }

    pub fn import_subtitles_path(&mut self, path: &std::path::Path) {
        let lang = self.lang();
        let path_s = path.display().to_string();
        match neoveg_engine::subtitles::import_subtitles_file(path) {
            Ok((fmt, cues, warnings)) => {
                for w in &warnings {
                    util::log_warn(&format!("Legendas: {w}"));
                }
                if cues.is_empty() {
                    self.set_status(tr_args(
                        lang,
                        "Nenhuma legenda encontrada em {path}",
                        &[("path", &path_s)],
                    ));
                    return;
                }
                self.checkpoint();
                let label = match fmt {
                    neoveg_engine::subtitles::SubtitleFormat::Srt => tr(lang, "Legendas (SRT)"),
                    neoveg_engine::subtitles::SubtitleFormat::Vtt => tr(lang, "Legendas (WebVTT)"),
                };
                let added = neoveg_engine::subtitles::import_subtitles_into_project(
                    &mut self.session.project,
                    &cues,
                    label,
                );
                self.changed();
                let added_s = added.to_string();
                self.set_status(tr_args(
                    lang,
                    "{added} legenda(s) importada(s) de {path}",
                    &[("added", &added_s), ("path", &path_s)],
                ));
            }
            Err(e) => {
                let err_s = e.to_string();
                self.set_status(tr_args(
                    lang,
                    "Erro ao importar legendas: {err}",
                    &[("err", &err_s)],
                ));
            }
        }
    }

    pub fn export_subtitles_dialog(&mut self, fmt: neoveg_engine::subtitles::SubtitleFormat) {
        let lang = self.lang();
        let (label_key, ext) = match fmt {
            neoveg_engine::subtitles::SubtitleFormat::Srt => ("Legendas SubRip", "srt"),
            neoveg_engine::subtitles::SubtitleFormat::Vtt => ("Legendas WebVTT", "vtt"),
        };
        let label = tr(lang, label_key);
        let default_name = self.export_interchange_path(ext);
        let Some(p) = rfd::FileDialog::new()
            .add_filter(label, &[ext])
            .set_file_name(&default_name)
            .save_file()
        else {
            return;
        };
        self.session.project.sort_events();
        let res = match fmt {
            neoveg_engine::subtitles::SubtitleFormat::Srt => {
                neoveg_engine::subtitles::export_srt_file(&self.session.project, &p)
            }
            neoveg_engine::subtitles::SubtitleFormat::Vtt => {
                neoveg_engine::subtitles::export_vtt_file(&self.session.project, &p)
            }
        };
        match res {
            Ok(n) => {
                let n_s = n.to_string();
                let p_s = p.display().to_string();
                self.set_status(tr_args(
                    lang,
                    "{label} exportado ({n} legenda(s)): {path}",
                    &[("label", &label), ("n", &n_s), ("path", &p_s)],
                ));
            }
            Err(e) => {
                let err_s = e.to_string();
                self.set_status(tr_args(
                    lang,
                    "Erro ao exportar {label}: {err}",
                    &[("label", &label), ("err", &err_s)],
                ));
            }
        }
    }

    pub fn import_edl_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("CMX 3600 EDL", &["edl"])
            .pick_file()
        {
            self.import_edl_path(&p);
        }
    }

    pub fn import_edl_path(&mut self, path: &std::path::Path) {
        let fps = self.session.project.settings.fps;
        let res = neoveg_engine::edl::import_edl_file(path, fps);
        self.apply_imported_project("EDL CMX 3600", path, res);
    }

    pub fn export_edl_dialog(&mut self) {
        let lang = self.lang();
        let default_name = self.export_interchange_path("edl");
        let Some(p) = rfd::FileDialog::new()
            .add_filter("CMX 3600 EDL", &["edl"])
            .set_file_name(&default_name)
            .save_file()
        else {
            return;
        };
        self.session.project.sort_events();
        match neoveg_engine::edl::export_edl_file(&self.session.project, &p, None) {
            Ok(()) => {
                let p_s = p.display().to_string();
                self.set_status(tr_args(
                    lang,
                    "EDL CMX 3600 exportado: {path}",
                    &[("path", &p_s)],
                ));
            }
            Err(e) => {
                let err_s = e.to_string();
                self.set_status(tr_args(
                    lang,
                    "Erro ao exportar EDL: {err}",
                    &[("err", &err_s)],
                ));
            }
        }
    }

    pub fn import_fcpxml_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Final Cut Pro XML", &["fcpxml", "xml"])
            .pick_file()
        {
            self.import_fcpxml_path(&p);
        }
    }

    pub fn import_fcpxml_path(&mut self, path: &std::path::Path) {
        // `.xml` pode ser FCPXML ou FCP7 (`xmeml`): decide pelo elemento raiz.
        if Self::is_fcp7_file(path) {
            self.import_fcp7_path(path);
            return;
        }
        let res = neoveg_engine::fcpxml::import_fcpxml_file(path);
        self.apply_imported_project("FCPXML", path, res);
    }

    /// Lê só o começo do arquivo para reconhecer um documento `xmeml`.
    fn is_fcp7_file(path: &std::path::Path) -> bool {
        use std::io::Read;
        let Ok(f) = std::fs::File::open(path) else {
            return false;
        };
        let mut head = Vec::with_capacity(4096);
        if f.take(4096).read_to_end(&mut head).is_err() {
            return false;
        }
        neoveg_engine::fcp7xml::looks_like_fcp7(&String::from_utf8_lossy(&head))
    }

    pub fn import_aaf_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Advanced Authoring Format", &["aaf"])
            .pick_file()
        {
            self.import_aaf_path(&p);
        }
    }

    pub fn import_aaf_path(&mut self, path: &std::path::Path) {
        let res = neoveg_engine::aaf::import_aaf_file(path);
        self.apply_imported_project("AAF", path, res);
    }

    pub fn export_aaf_dialog(&mut self) {
        let default_name = self.export_interchange_path("aaf");
        let Some(p) = rfd::FileDialog::new()
            .add_filter("Advanced Authoring Format", &["aaf"])
            .set_file_name(&default_name)
            .save_file()
        else {
            return;
        };
        self.session.project.sort_events();
        match neoveg_engine::aaf::export_aaf_file(&self.session.project, &p) {
            Ok(()) => self.set_status(format!("AAF exportado: {}", p.display())),
            Err(e) => self.set_status(format!("Erro ao exportar AAF: {e}")),
        }
    }

    pub fn import_fcp7_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Final Cut Pro 7 XML", &["xml"])
            .pick_file()
        {
            self.import_fcp7_path(&p);
        }
    }

    pub fn import_fcp7_path(&mut self, path: &std::path::Path) {
        let res = neoveg_engine::fcp7xml::import_fcp7_file(path);
        self.apply_imported_project("FCP7 XML", path, res);
    }

    pub fn export_fcp7_dialog(&mut self) {
        let lang = self.lang();
        let default_name = self.export_interchange_path("xml");
        let Some(p) = rfd::FileDialog::new()
            .add_filter("Final Cut Pro 7 XML", &["xml"])
            .set_file_name(&default_name)
            .save_file()
        else {
            return;
        };
        self.session.project.sort_events();
        match neoveg_engine::fcp7xml::export_fcp7_file(&self.session.project, &p, None) {
            Ok(()) => {
                let p_s = p.display().to_string();
                self.set_status(tr_args(
                    lang,
                    "FCP7 XML exportado: {path}",
                    &[("path", &p_s)],
                ));
            }
            Err(e) => {
                let err_s = e.to_string();
                self.set_status(tr_args(
                    lang,
                    "Erro ao exportar FCP7 XML: {err}",
                    &[("err", &err_s)],
                ));
            }
        }
    }

    pub fn export_fcpxml_dialog(&mut self) {
        let lang = self.lang();
        let default_name = self.export_interchange_path("fcpxml");
        let Some(p) = rfd::FileDialog::new()
            .add_filter("Final Cut Pro XML", &["fcpxml"])
            .set_file_name(&default_name)
            .save_file()
        else {
            return;
        };
        self.session.project.sort_events();
        match neoveg_engine::fcpxml::export_fcpxml_file(&self.session.project, &p, None) {
            Ok(()) => {
                let p_s = p.display().to_string();
                self.set_status(tr_args(lang, "FCPXML exportado: {path}", &[("path", &p_s)]));
            }
            Err(e) => {
                let err_s = e.to_string();
                self.set_status(tr_args(
                    lang,
                    "Erro ao exportar FCPXML: {err}",
                    &[("err", &err_s)],
                ));
            }
        }
    }

    pub fn save_project(&mut self, save_as: bool) -> bool {
        let lang = self.lang();
        let path = if save_as || self.path.is_none() {
            match rfd::FileDialog::new()
                .add_filter(tr(lang, "Projeto Neoveg"), &["neoveg"])
                .add_filter(tr(lang, "Projeto legado (NVP)"), &["nvp"])
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
        let lang = self.lang();
        self.session.project.sort_events();
        match self.session.project.save_file(path) {
            Ok(()) => {
                self.path = Some(path.to_path_buf());
                self.dirty = false;
                recovery::clear_recovery();
                self.prefs.record_recent_project(path);
                let _ = self.prefs.save();
                let p_s = path.display().to_string();
                self.set_status(tr_args(lang, "Projeto salvo: {path}", &[("path", &p_s)]));
                true
            }
            Err(e) => {
                let err_s = e.to_string();
                self.set_status(tr_args(lang, "Erro ao salvar: {err}", &[("err", &err_s)]));
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
        let lang = self.lang();
        let mut all: Vec<&str> = Vec::new();
        all.extend_from_slice(media::IMAGE_EXTS);
        all.extend_from_slice(media::VIDEO_EXTS);
        all.extend_from_slice(media::AUDIO_EXTS);
        all.extend_from_slice(media::SYSTEM_CONTAINER_EXTS);
        if let Some(files) = rfd::FileDialog::new()
            .add_filter(tr(lang, "Mídia suportada"), &all)
            .add_filter(tr(lang, "Imagens"), media::IMAGE_EXTS)
            .add_filter(tr(lang, "Áudio"), media::AUDIO_EXTS)
            .add_filter(tr(lang, "Todos os arquivos"), &["*"])
            .pick_files()
        {
            self.import_paths(files, None);
        }
    }

    pub fn import_sequence_dialog(&mut self) {
        let lang = self.lang();
        if let Some(f) = rfd::FileDialog::new()
            .add_filter(tr(lang, "Imagens"), media::IMAGE_EXTS)
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
        let lang = self.lang();
        let mut t = place_at.map(PlaceTarget::At);
        for p in paths {
            if !media::is_supported(&p) {
                // o formato não é aberto diretamente: oferece converter
                let p_s = p.display().to_string();
                self.set_status(tr_args(
                    lang,
                    "Formato não suportado diretamente: {name}",
                    &[("name", &p_s)],
                ));
                self.offer_conversion(p, t, tr(lang, "formato não suportado diretamente"));
                continue;
            }
            self.spawn_import(p, t);
            // arquivos múltiplos são colocados em sequência quando chegarem
            if t.is_some() {
                t = Some(PlaceTarget::EndOfTrack);
            }
        }
    }

    /// Importa um arquivo em segundo plano; a falha volta como `ImportFailed`.
    pub(crate) fn spawn_import(&mut self, p: PathBuf, place: Option<PlaceTarget>) {
        let tx = self.loader_tx.clone();
        self.loading += 1;
        let spawned = crate::util::spawn("importar", move || match media::import_file(&p) {
            Ok(i) => {
                let _ = tx.send(LoadMsg::Imported(i, place));
            }
            Err(e) => {
                let _ = tx.send(LoadMsg::ImportFailed {
                    error: format!("{e:#}"),
                    path: p,
                    place,
                });
            }
        });
        if let Err(e) = spawned {
            self.loading = self.loading.saturating_sub(1);
            self.set_status(e);
        }
    }

    fn poll_loader(&mut self) {
        // Com um script rodando, a sessão que vale no fim é a do script
        // (`poll_script_job` a coloca no lugar desta): o que chegar agora
        // (importação, mídia recarregada, proxy, pré-renderização) fica no
        // canal e é aplicado depois, sobre a sessão do script, sem se perder.
        if self.script_job.is_some() {
            return;
        }
        while let Ok(msg) = self.loader_rx.try_recv() {
            self.loading = self.loading.saturating_sub(1);
            match msg {
                LoadMsg::Imported(imp, place) => {
                    let lang = self.lang();
                    let id = imp.item.id;
                    if let Some(w) = &imp.warning {
                        self.set_status(w.clone());
                    } else {
                        self.set_status(tr_args(
                            lang,
                            "Importado: {name}",
                            &[("name", &imp.item.name)],
                        ));
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
                        self.thumbs.remove(&id);
                        self.project_media.invalidate_media(id);
                        self.generation += 1;
                    }
                    Err(e) => {
                        let lang = self.lang();
                        self.set_status(tr_args(lang, "Mídia offline: {err}", &[("err", &e)]));
                    }
                },
                LoadMsg::Replaced(id, r) => match r {
                    Ok(imp) => self.replace_media_with_imported(id, imp),
                    Err(e) => {
                        let lang = self.lang();
                        self.set_status(tr_args(
                            lang,
                            "Erro ao substituir mídia: {err}",
                            &[("err", &e)],
                        ));
                    }
                },
                LoadMsg::ProxyBuilt(id, res) => {
                    let lang = self.lang();
                    match res {
                        Ok(p) => {
                            if let Some(m) = self.project.media.iter_mut().find(|m| m.id == id) {
                                m.proxy_path = Some(p.to_string_lossy().to_string());
                            }
                            self.dirty = true;
                            self.generation += 1;
                            let p_s = p.display().to_string();
                            self.set_status(tr_args(
                                lang,
                                "Proxy pronto: {path}",
                                &[("path", &p_s)],
                            ));
                        }
                        Err(e) => self.set_status(tr_args(
                            lang,
                            "Erro ao gerar proxy: {err}",
                            &[("err", &e)],
                        )),
                    }
                }
                LoadMsg::RamPreviewFrame {
                    gen,
                    frame_idx,
                    image,
                } => {
                    if gen == self.generation {
                        let _ = self.ram_preview.insert_frame(frame_idx, image);
                    }
                }
                LoadMsg::RamPreviewDone { gen } => {
                    if self.ram_preview_job.as_ref().is_some_and(|j| j.gen == gen) {
                        self.ram_preview_job = None;
                        if gen == self.generation {
                            let n = self.ram_preview.frames.len();
                            let mb = format!("{:.1}", self.ram_preview.mb_used());
                            self.set_status(tr_args(
                                self.lang(),
                                "Prévia em RAM pronta: {n} quadros ({mb} MB).",
                                &[("n", &n), ("mb", &mb)],
                            ));
                        }
                    }
                }
                LoadMsg::PrerenderDone { start, end, result } => {
                    let lang = self.lang();
                    self.prerender_job = None;
                    match result {
                        Ok(p) => {
                            self.session.record_prerendered_range(start, end);
                            let p_s = p.display().to_string();
                            self.set_status(tr_args(
                                lang,
                                "Pré-renderização concluída: {path}",
                                &[("path", &p_s)],
                            ));
                        }
                        Err(e) => self.set_status(tr_args(
                            lang,
                            "Erro na pré-renderização: {err}",
                            &[("err", &e)],
                        )),
                    }
                }
                LoadMsg::LoudnessAnalyzed(result) => {
                    let lang = self.lang();
                    self.loudness_job = None;
                    match result {
                        Ok(rep) => {
                            let lufs_s = neoveg_engine::engine::loudness::format_lufs(
                                rep.snapshot.integrated_lufs,
                            );
                            let lra_s = format!("{:.1}", rep.snapshot.loudness_range_lu);
                            let summary = tr_args(
                                lang,
                                "Loudness analisado: {lufs} (LRA {lra} LU)",
                                &[("lufs", &lufs_s), ("lra", &lra_s)],
                            );
                            self.loudness_report = Some(rep);
                            self.set_status(summary);
                        }
                        Err(e) => self.set_status(tr_args(
                            lang,
                            "Análise de loudness: {err}",
                            &[("err", &e)],
                        )),
                    }
                }
                LoadMsg::ImportFailed { path, error, place } => {
                    let lang = self.lang();
                    let p_s = path.display().to_string();
                    self.set_status(tr_args(
                        lang,
                        "Erro de importação: {name}: {err}",
                        &[("name", &p_s), ("err", &error)],
                    ));
                    // um arquivo que existe mas não abriu pode ter um decodificador no sistema
                    if path.is_file() {
                        let reason = tr_args(lang, "não abriu: {err}", &[("err", &error)]);
                        self.offer_conversion(path, place, reason);
                    }
                }
                LoadMsg::Converted { src, result, place } => {
                    self.on_converted(&src, result, place);
                }
                LoadMsg::Error(e) => {
                    let lang = self.lang();
                    self.set_status(tr_args(lang, "Erro de importação: {err}", &[("err", &e)]));
                }
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
        let still_len = Ticks::from_secs_f64_clamped(self.prefs.still_image_secs as f64);
        let len = self
            .project
            .media(media_id)
            .and_then(|m| m.duration)
            .unwrap_or(still_len)
            .max(self.project.settings.frame_dur());
        self.add_trimmed_media_to_timeline(media_id, t, Ticks::ZERO, len, true, true, track_hint)
    }

    /// Procura uma trilha do tipo a partir do índice sugerido; cria se não houver.
    pub fn find_or_create_track(&mut self, kind: TrackKind, hint: Option<usize>) -> usize {
        if let Some(h) = hint {
            if let Some(t) = self.project.tracks.get(h) {
                if t.accepts(kind) {
                    return h;
                }
            }
            if let Some(i) =
                (h..self.project.tracks.len()).find(|&i| self.project.tracks[i].accepts(kind))
            {
                return i;
            }
        }
        if let Some(i) = self.project.tracks.iter().position(|t| t.accepts(kind)) {
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
            if let Some(alt_img) = res.split_alt {
                let alt_ci = rgba_to_color_image(&alt_img);
                match &mut self.preview_split_tex {
                    Some(t) => t.set(alt_ci, egui::TextureOptions::LINEAR),
                    None => {
                        self.preview_split_tex = Some(ctx.load_texture(
                            "preview_split_alt",
                            alt_ci,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                }
            } else if self.opts.preview_split_mode
                != neoveg_engine::playback::PreviewSplitMode::FxBypass
            {
                self.preview_split_tex = None;
            }
            if let Some((la, rb)) = res.expanded_ab {
                let ia = rgba_to_color_image(&la);
                let ib = rgba_to_color_image(&rb);
                match &mut self.expanded_ab_tex {
                    Some((ta, tb)) => {
                        ta.set(ia, egui::TextureOptions::LINEAR);
                        tb.set(ib, egui::TextureOptions::LINEAR);
                    }
                    None => {
                        self.expanded_ab_tex = Some((
                            ctx.load_texture("expanded_a", ia, egui::TextureOptions::LINEAR),
                            ctx.load_texture("expanded_b", ib, egui::TextureOptions::LINEAR),
                        ));
                    }
                }
            } else if self.session.expanded_edit.is_none() {
                self.expanded_ab_tex = None;
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
        let scopes =
            self.is_panel_open(DockTab::Scopes) || self.is_panel_open(DockTab::ColorGrading);
        let prefer_proxy = self.opts.preview_quality.prefer_proxy();
        let split_mode = self.opts.preview_split_mode;
        let mute_video = self.opts.preview_mute_video;
        if let Some(job) = &self.ram_preview_job {
            if job.gen != self.generation {
                job.cancel.store(true, Ordering::Relaxed);
                self.ram_preview_job = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
        }
        self.sync_master_monitor_audio();
        let expanded_ab = self.session.expanded_edit.as_ref().map(|exp| {
            let fd = self.project.settings.frame_dur().max(Ticks::from_raw(1));
            let (lid, lt) = match exp.left_event_id.and_then(|id| {
                self.project
                    .event(id)
                    .map(|e| (id, (e.end() - fd).max(e.start)))
            }) {
                Some((id, ts)) => (Some(id), ts),
                None => (None, Ticks::ZERO),
            };
            let (rid, rt) = match exp
                .right_event_id
                .and_then(|id| self.project.event(id).map(|e| (id, e.start)))
            {
                Some((id, ts)) => (Some(id), ts),
                None => (None, Ticks::ZERO),
            };
            (lid, lt, rid, rt)
        });
        let key = PreviewReqKey {
            t,
            gen: self.generation,
            w,
            h,
            scopes,
            prefer_proxy,
            split_mode,
            mute_video,
            expanded_ab,
        };
        if key != self.last_req {
            if !scopes
                && !mute_video
                && split_mode == neoveg_engine::playback::PreviewSplitMode::Off
                && expanded_ab.is_none()
            {
                let frame_idx = self.project.settings.fps.ticks_to_frame_round(t);
                if let Some(cached) = self.ram_preview.get(self.generation, frame_idx, w, h) {
                    let ci = rgba_to_color_image(cached);
                    match &mut self.preview_tex {
                        Some(tex) => tex.set(ci, egui::TextureOptions::LINEAR),
                        None => {
                            self.preview_tex =
                                Some(ctx.load_texture("preview", ci, egui::TextureOptions::LINEAR));
                        }
                    }
                    self.last_req = key;
                    self.preview_inflight = false;
                    return;
                }
            }
            self.sync_snapshot();
            self.preview.request(PreviewRequest {
                project: self.snapshot.clone(),
                t,
                w,
                h,
                scopes,
                prefer_proxy,
                split_mode,
                mute_video,
                expanded_ab,
            });
            self.last_req = key;
            self.preview_inflight = true;
            self.preview_inflight_since = Instant::now();
        }
    }

    pub fn copy_preview_frame_to_clipboard(&mut self, ctx: Option<&egui::Context>) {
        let t = self.project.settings.quantize(self.view_time());
        let w = (self.project.settings.width as usize).clamp(16, 4096);
        let h = (self.project.settings.height as usize).clamp(16, 4096);
        let frame = neoveg_engine::engine::video::render_frame_preview(
            &self.project,
            &self.media,
            t,
            w,
            h,
            self.opts.preview_quality.prefer_proxy(),
        );
        let Some(rgba) = image::RgbaImage::from_raw(w as u32, h as u32, frame.to_rgba8()) else {
            return;
        };
        let ci = rgba_to_color_image(&rgba);
        if let Some(c) = ctx {
            c.copy_image(ci.clone());
            let tex = c.load_texture("preview_clipboard", ci, egui::TextureOptions::LINEAR);
            self.preview_clipboard = Some((tex, rgba));
        } else if let Some((tex, buf)) = &mut self.preview_clipboard {
            tex.set(ci, egui::TextureOptions::LINEAR);
            *buf = rgba;
        }
        self.set_status(format!(
            "Quadro ({w}x{h}) copiado para a área de transferência."
        ));
    }

    pub fn start_ram_preview(&mut self) {
        if let Some(prev) = self.ram_preview_job.take() {
            prev.cancel.store(true, Ordering::Relaxed);
        }
        let fps = self.project.settings.fps;
        let fd = self.project.settings.frame_dur().max(Ticks::from_raw(1));
        let (start, end) = match self.time_sel.filter(|(a, b)| b > a) {
            Some((a, b)) => (a, b),
            None => {
                let a = self.cursor.max(Ticks::ZERO);
                let proj_end = self.project.duration().max(a + Ticks::from_secs_i64(3));
                let b = (a + Ticks::from_secs_i64(4)).min(proj_end);
                (a, b)
            }
        };
        let s = self.opts.preview_quality.scale();
        let w = ((self.project.settings.width as f32 * s) as usize).max(16);
        let h = ((self.project.settings.height as f32 * s) as usize).max(16);
        let max_mb = self.prefs.ram_preview_mb as usize;
        let gen = self.generation;
        self.ram_preview
            .reset(gen, (w, h), (start, end), fps, max_mb);

        let bytes_per_frame = w.saturating_mul(h).saturating_mul(4).max(1);
        let budget_frames = (self.ram_preview.max_bytes / bytes_per_frame).max(1) as i64;
        let start_idx = fps.ticks_to_frame_round(start);
        let raw_end_idx = fps.ticks_to_frame_round(end).max(start_idx);
        let end_idx = raw_end_idx.min(start_idx.saturating_add(budget_frames - 1));
        let total = (end_idx - start_idx + 1).max(1) as u64;

        self.sync_snapshot();
        let project = self.snapshot.clone();
        let media = self.media.clone();
        let prefer_proxy = self.opts.preview_quality.prefer_proxy();
        let tx = self.loader_tx.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicU64::new(0));
        let job = RamPreviewJob {
            cancel: cancel.clone(),
            done: done.clone(),
            total: Arc::new(AtomicU64::new(total)),
            gen,
        };
        self.ram_preview_job = Some(job);
        let spawned = util::spawn("ram-preview", move || {
            for idx in start_idx..=end_idx {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let ft = Ticks::from_raw(fd.0.saturating_mul(idx));
                let fr = neoveg_engine::engine::video::render_frame_preview(
                    &project,
                    &media,
                    ft,
                    w,
                    h,
                    prefer_proxy,
                );
                if let Some(rgba) = image::RgbaImage::from_raw(w as u32, h as u32, fr.to_rgba8()) {
                    if tx
                        .send(LoadMsg::RamPreviewFrame {
                            gen,
                            frame_idx: idx,
                            image: rgba,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                done.fetch_add(1, Ordering::Relaxed);
            }
            let _ = tx.send(LoadMsg::RamPreviewDone { gen });
        });
        if let Err(e) = spawned {
            self.ram_preview_job = None;
            self.set_status(e);
        } else {
            self.set_status(format!(
                "Gerando prévia dinâmica em RAM ({total} quadros)..."
            ));
        }
    }

    pub fn clear_ram_preview(&mut self) {
        if let Some(job) = self.ram_preview_job.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.ram_preview.clear();
        self.set_status("Cache da prévia dinâmica em RAM limpo.");
    }

    pub fn apply_preview_zoom(&mut self, val: f32) {
        if val == 0.0 {
            self.preview_zoom = 1.0;
            self.preview_pan = egui::Vec2::ZERO;
        } else if val < 0.0 {
            self.preview_zoom = (self.preview_zoom * (-val)).clamp(0.25, 8.0);
        } else {
            self.preview_zoom = val.clamp(0.25, 8.0);
        }
        if (self.preview_zoom - 1.0).abs() < 0.01 {
            self.preview_pan = egui::Vec2::ZERO;
        }
    }

    /// Inicia ou finaliza a gravação de áudio nas faixas armadas (manual p. 821).
    pub fn toggle_record(&mut self) {
        if let Some(rec) = self.recorder.take() {
            self.recorder_tracks.clear();
            self.recording_peaks.clear();
            match rec.stop() {
                Ok(clip) => {
                    if clip.frames_written == 0 {
                        let _ = std::fs::remove_file(&clip.path);
                        self.set_status("Gravação encerrada sem quadros capturados.");
                        return;
                    }
                    match media::import_file(&clip.path) {
                        Ok(imp) => {
                            let id = imp.item.id;
                            self.media.insert(id, imp.entry);
                            let dur = Ticks::from_secs_f64(
                                clip.frames_written as f64 / f64::from(clip.sample_rate.max(1)),
                            )
                            .unwrap_or_else(|| self.project.settings.frame_dur());
                            self.session.commit_recorded_clip(
                                imp.item,
                                &clip.target_tracks,
                                clip.start_cursor,
                                dur,
                            );
                            self.sync_after_session();
                        }
                        Err(e) => {
                            self.set_status(format!("Erro ao importar gravação WAV: {e}"));
                        }
                    }
                }
                Err(e) => self.set_status(e),
            }
            return;
        }
        let mut armed = self.session.armed_audio_tracks();
        if armed.is_empty() {
            if let Some(ti) = self.sel_track {
                if self.session.toggle_record_arm(ti) {
                    armed = self.session.armed_audio_tracks();
                }
            }
        }
        if armed.is_empty() {
            self.set_status("Arme uma faixa de áudio (●) antes de iniciar a gravação.");
            return;
        }
        let target_ids: Vec<Id> = armed.iter().map(|(_, id)| *id).collect();
        let dir = if !self.prefs.audio_device.default_record_dir.trim().is_empty() {
            PathBuf::from(self.prefs.audio_device.default_record_dir.trim())
        } else {
            self.path
                .as_ref()
                .and_then(|p| p.parent())
                .map(|p| p.join("Neoveg Recordings"))
                .unwrap_or_else(|| std::env::temp_dir().join("neoveg_recordings"))
        };
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let out_wav = dir.join(format!("gravacao_{stamp}.wav"));
        match neoveg_engine::playback::AudioRecorder::start(
            target_ids.clone(),
            self.cursor,
            out_wav,
        ) {
            Ok(rec) => {
                let msg = format!(
                    "Gravando em {} faixa(s) via {}...",
                    target_ids.len(),
                    rec.device_name
                );
                self.recorder_tracks = target_ids;
                self.recorder = Some(rec);
                self.set_status(msg);
            }
            Err(e) => self.set_status(e),
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
        if res.is_ok() && matches!(id, "insert.track_video" | "insert.track_audio") {
            if let Some(ti) = self.session.sel_track {
                self.session
                    .apply_default_track_props(ti, &self.prefs.default_track);
            }
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
            UiAction::ConvertMediaDialog => self.convert_media_dialog(),
            UiAction::ImportOtioDialog => self.import_otio_dialog(),
            UiAction::ExportOtioDialog => self.export_otio_dialog(),
            UiAction::ImportSubtitlesDialog => self.import_subtitles_dialog(),
            UiAction::ExportSrtDialog => {
                self.export_subtitles_dialog(neoveg_engine::subtitles::SubtitleFormat::Srt)
            }
            UiAction::ExportVttDialog => {
                self.export_subtitles_dialog(neoveg_engine::subtitles::SubtitleFormat::Vtt)
            }
            UiAction::ImportEdlDialog => self.import_edl_dialog(),
            UiAction::ExportEdlDialog => self.export_edl_dialog(),
            UiAction::ImportFcpxmlDialog => self.import_fcpxml_dialog(),
            UiAction::ExportFcpxmlDialog => self.export_fcpxml_dialog(),
            UiAction::ImportFcp7Dialog => self.import_fcp7_dialog(),
            UiAction::ExportFcp7Dialog => self.export_fcp7_dialog(),
            UiAction::ImportAafDialog => self.import_aaf_dialog(),
            UiAction::ExportAafDialog => self.export_aaf_dialog(),
            UiAction::OpenRenderDialog => self
                .dlg
                .open_render(&self.session.project, self.session.path.as_deref()),
            UiAction::SendToEncoder => self.send_timeline_to_encoder(),
            UiAction::EncoderStartQueue => {
                let lang = self.lang();
                self.encoder.start_queue(&self.media);
                self.set_status(tr(lang, "Fila do Neoveg Encoder iniciada"));
            }
            UiAction::EncoderPauseQueue => {
                self.encoder.runtime.toggle_pause();
            }
            UiAction::EncoderStopCurrent => {
                self.encoder.runtime.stop_current();
            }
            UiAction::EncoderStopQueue => {
                self.encoder.runtime.stop_queue();
            }
            UiAction::SnapshotDialog => self.snapshot_dialog(),
            UiAction::OpenProjectProps => self.dlg.project_props = true,
            UiAction::OpenEventProps => {
                self.dlg.event_props = self.sel_events.iter().next().copied();
            }
            UiAction::OpenPasteAttributesDialog => {
                self.dlg.paste_attrs_open = true;
            }
            UiAction::OpenTakeChooser => {
                self.dlg.take_chooser = self.sel_events.iter().next().copied();
            }
            UiAction::OpenSceneDetectDialog => {
                if let Some(&id) = self.sel_events.iter().find(|&&eid| {
                    self.project.find_event(eid).is_some_and(|(ti, ei)| {
                        self.project.tracks[ti].kind == TrackKind::Video
                            && self
                                .project
                                .media(self.project.tracks[ti].events[ei].media_id)
                                .is_some_and(|m| m.has_video)
                    })
                }) {
                    self.open_scene_detect_for(id);
                }
            }
            UiAction::OpenPanCrop => {
                self.dlg.pan_crop = self.sel_events.iter().next().copied();
            }
            UiAction::OpenFxEvent => {
                self.dlg.fx_target = self.sel_events.iter().next().map(|i| FxTarget::Event(*i));
            }
            UiAction::OpenFxMasterAudio => self.dlg.fx_target = Some(FxTarget::MasterAudio),
            UiAction::OpenFxMasterVideo => self.dlg.fx_target = Some(FxTarget::MasterVideo),
            UiAction::OpenMotionTrackDialog => self.open_motion_track_dialog(),
            UiAction::ApplyAudioFxNonRealtime => self.apply_audio_fx_non_realtime(),
            UiAction::OpenCustomizeToolbar => self.open_customize_toolbar(),
            UiAction::OpenCommandMarkerDialog => {
                self.dlg.cmd_marker_open = true;
                if self.dlg.cmd_marker_type.trim().is_empty() {
                    self.dlg.cmd_marker_type = "TEXT".into();
                }
            }
            UiAction::SetRulerGrid(g) => {
                self.opts.ruler_grid = g;
                self.set_status(format!("Grade da régua: {}", g.label()));
            }
            UiAction::OpenGoto => self.dlg.goto_open = true,
            UiAction::OpenEditLayoutProps => {
                self.dlg
                    .open_edit_layout(&self.prefs, self.dock_height, self.preview_ratio);
            }
            UiAction::SetLayoutType(lt) => self.select_layout_type(ctx, lt, true),
            UiAction::OpenShortcuts => self.dlg.shortcuts = true,
            UiAction::OpenPrefs => self.dlg.open_prefs(&self.prefs),
            UiAction::OpenSplash => self.open_splash(),
            UiAction::OpenWelcome => self.open_welcome(),
            UiAction::OpenAbout => self.dlg.about = true,
            UiAction::BuildProxySelected => self.build_proxy_selected(),
            UiAction::PrerenderRegion => self.prerender_region(),
            UiAction::RunScriptDialog => self.run_script_dialog(),
            UiAction::OpenIntegrationsDialog => self.open_integrations_dialog(),
            UiAction::SendToExternalApp(ext_app) => self.send_to_external_app(ext_app, None, None),
            UiAction::BlenderRenderSceneDialog => self.blender_render_scene_dialog(),
            UiAction::BlenderExportScriptDialog => self.workflow_blender_export_script(None),
            UiAction::BlenderImportCameraDialog => self.blender_import_camera_dialog(),
            UiAction::ExitApp => {
                if let Some(c) = ctx {
                    c.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            UiAction::SelectDock(panel) if crate::layout::window_only(panel) => {
                self.open_window_panel(ctx, panel);
            }
            UiAction::SelectDock(panel) => {
                self.dock_visible = true;
                self.closed_tabs.remove(&panel);
                self.detached_tabs.remove(&panel);
                self.dock_tab = panel;
                self.pending_dock_focus = Some(panel);
            }
            UiAction::TogglePreview => {
                if self.preview_detached {
                    self.preview_detached = false;
                    self.preview_visible = true;
                } else {
                    self.preview_visible = !self.preview_visible;
                }
                if self.preview_visible {
                    self.dock_visible = true;
                }
            }
            UiAction::ToggleMasterBus => {
                if self.master_bus_detached {
                    self.master_bus_detached = false;
                    self.master_bus_visible = true;
                } else {
                    self.master_bus_visible = !self.master_bus_visible;
                }
                if self.master_bus_visible {
                    self.dock_visible = true;
                }
            }
            UiAction::ToggleDockArea => {
                if self.timeline_restore.is_some() {
                    self.toggle_maximize_timeline();
                } else {
                    self.dock_visible = !self.dock_visible;
                }
            }
            UiAction::ToggleTrackList => self.tl.toggle_header_collapse(),
            UiAction::ToggleMaximizeTimeline => self.toggle_maximize_timeline(),
            UiAction::TogglePlay => self.toggle_play(),
            UiAction::ToggleRecord => self.toggle_record(),
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
            UiAction::ZoomBy(f) => {
                let c = self.cursor;
                self.tl.zoom_around_playhead(f, c);
            }
            UiAction::JumpMarker(forward) => self.jump_marker(forward),
            UiAction::JumpMarkerIndex(idx) => {
                if let Some(t) = self.session.jump_to_marker_index(idx) {
                    self.set_cursor(t);
                }
            }
            UiAction::ProjectMediaSetView(mode) => {
                self.dock_visible = true;
                self.dock_tab = DockTab::ProjectMedia;
                self.pending_dock_focus = Some(DockTab::ProjectMedia);
                self.project_media.view_mode = match mode {
                    1 => crate::project_media::MediaViewMode::List,
                    2 => crate::project_media::MediaViewMode::Details,
                    3 => {
                        if self.project.storyboards.is_empty() {
                            self.session.create_storyboard("Storyboard 1");
                        }
                        crate::project_media::MediaViewMode::Storyboard
                    }
                    _ => crate::project_media::MediaViewMode::Thumbnails,
                };
            }
            UiAction::ProjectMediaNewBin => {
                self.dock_visible = true;
                self.dock_tab = DockTab::ProjectMedia;
                self.pending_dock_focus = Some(DockTab::ProjectMedia);
                if let Some(bid) = self.session.create_media_bin("", None) {
                    self.project_media.filter_target =
                        crate::project_media::MediaFilterTarget::Bin(bid);
                    self.sync_after_session();
                }
            }
            UiAction::ProjectMediaNewSmartBin => {
                self.dock_visible = true;
                self.dock_tab = DockTab::ProjectMedia;
                self.pending_dock_focus = Some(DockTab::ProjectMedia);
                self.project_media.editing_smart_id = None;
                self.project_media.smart_dialog_open = true;
            }
            UiAction::ProjectMediaNewStoryboard => {
                self.dock_visible = true;
                self.dock_tab = DockTab::ProjectMedia;
                self.pending_dock_focus = Some(DockTab::ProjectMedia);
                if let Some(sb_id) = self.session.create_storyboard("") {
                    self.project_media.view_mode = crate::project_media::MediaViewMode::Storyboard;
                    self.project_media.filter_target =
                        crate::project_media::MediaFilterTarget::Storyboard(sb_id);
                    self.sync_after_session();
                }
            }
            UiAction::ProjectMediaReplaceDialog => {
                if let Some(mid) = self.session.active_media_id {
                    self.replace_media_dialog(mid);
                }
            }
            UiAction::ProjectMediaInsertStoryboard => {
                if let Some(sb_id) = self.session.active_storyboard_id {
                    let cur = self.cursor;
                    self.session.insert_storyboard_at_cursor(
                        sb_id,
                        cur,
                        self.prefs.still_image_secs,
                    );
                    self.sync_after_session();
                }
            }
            UiAction::PreviewToggleSplit => {
                self.opts.preview_split_mode = match self.opts.preview_split_mode {
                    neoveg_engine::playback::PreviewSplitMode::Off => {
                        neoveg_engine::playback::PreviewSplitMode::FxBypass
                    }
                    _ => neoveg_engine::playback::PreviewSplitMode::Off,
                };
            }
            UiAction::PreviewSplitMode(mode) => {
                self.opts.preview_split_mode = mode;
            }
            UiAction::PreviewCopyFrame => {
                self.copy_preview_frame_to_clipboard(ctx);
            }
            UiAction::PreviewRamBuild => {
                self.start_ram_preview();
            }
            UiAction::PreviewRamClear => {
                self.clear_ram_preview();
            }
            UiAction::PreviewZoom(z) => {
                self.apply_preview_zoom(z);
            }
            UiAction::PreviewToggleExternalMonitor => {
                self.external_monitor = !self.external_monitor;
                self.set_status(if self.external_monitor {
                    "Monitor externo ativado."
                } else {
                    "Monitor externo fechado."
                });
            }
            UiAction::InsertNestedTimelineDialog => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("Projetos Neoveg", &["neoveg", "nvp"])
                    .pick_file()
                {
                    match media::import_nested_project(&p) {
                        Ok(imp) => {
                            if let Some(w) = &imp.warning {
                                self.set_status(w.clone());
                            }
                            self.session
                                .insert_nested_project_event(imp, &self.media, self.cursor);
                            self.sync_after_session();
                        }
                        Err(e) => self.set_status(format!("Projeto aninhado: {e:#}")),
                    }
                }
            }
            UiAction::RenderToNewTrack => {
                let dir = std::env::temp_dir().join("neoveg_track_renders");
                match self.session.render_to_new_track(&self.media, None, &dir) {
                    Ok(_) => self.sync_after_session(),
                    Err(e) => self.set_status(format!("Renderizar para nova trilha: {e}")),
                }
            }
            UiAction::SetTrackDefaultProps => {
                if let Some(ti) = self.session.sel_track {
                    if let Some(defs) = self
                        .session
                        .capture_default_track_props(ti, &self.prefs.default_track)
                    {
                        self.prefs.default_track = defs;
                        let _ = self.prefs.save();
                        self.set_status("Propriedades padrão da trilha salvas nas Preferências.");
                    }
                }
            }
            UiAction::CycleStereo3dMode => {
                let modes = Stereo3dMode::ALL;
                let cur = self.project.settings.stereo_3d;
                let next_i = modes
                    .iter()
                    .position(|m| *m == cur)
                    .map(|p| (p + 1) % modes.len())
                    .unwrap_or(0);
                self.checkpoint();
                self.project.settings.stereo_3d = modes[next_i];
                self.changed();
                self.set_status(format!("Modo 3D: {}", modes[next_i].label()));
            }
        }
    }

    // -----------------------------------------------------------------
    // Atalhos de teclado (despachados via Tabela Única de Comandos)
    // -----------------------------------------------------------------

    pub(crate) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // com um script rodando, a sessão é dele até o fim
        if self.splash.active
            || self.welcome_open
            || self.script_job.is_some()
            || ctx.wants_keyboard_input()
        {
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

        // Esquerda/Direita sem modificadores (lista de atalhos do VEGAS Pro):
        // um pixel da régua; com Quantizar para quadros, no mínimo um quadro
        if !ctrl && !shift && !alt {
            if pressed(Key::ArrowLeft) {
                let t = self.arrow_step_target(-1);
                self.set_cursor(t);
                return;
            }
            if pressed(Key::ArrowRight) {
                let t = self.arrow_step_target(1);
                self.set_cursor(t);
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

        // Teclas numéricas 0-9: ângulo multicâmera (1..9 quando ativo) ou marcador
        if !ctrl && !alt && !shift {
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
                    if self.opts.multicam_mode
                        && i > 0
                        && self.session.switch_multicam_angle(self.cursor, i - 1)
                    {
                        self.sync_after_session();
                        return;
                    }
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

    pub(crate) fn cmd_menu_btn(&mut self, ui: &mut egui::Ui, id: &str) {
        let label = menu_label_in(id, &self.prefs.shortcuts, self.lang());
        if ui
            .add_enabled(self.cmd_enabled(id), egui::Button::new(label))
            .clicked()
        {
            self.exec_cmd(Some(ui.ctx()), id);
            ui.close_menu();
        }
    }

    /// Conteúdo do menu `More (...)` da barra de transporte. Em barras estreitas,
    /// mostra também as caixas In/Fim/Tam que foram ocultadas por falta de espaço.
    fn more_tools_menu(
        &mut self,
        ui: &mut egui::Ui,
        (sel_start, sel_end, sel_len): (Ticks, Ticks, Ticks),
        avail_w: f32,
    ) {
        let lang = self.lang();
        if avail_w < 430.0 {
            let label = if self.time_sel.is_some() {
                tr(lang, "In:")
            } else {
                tr(lang, "Pos:")
            };
            ui.label(format!("{label} {}", self.fmt_time(sel_start)));
            ui.label(format!("{} {}", tr(lang, "Fim:"), self.fmt_time(sel_end)));
            ui.label(format!("{} {}", tr(lang, "Tam:"), self.fmt_time(sel_len)));
            ui.separator();
        }
        for group in MORE_TOOLS_COMMANDS {
            for id in *group {
                self.cmd_menu_btn(ui, id);
            }
            ui.separator();
        }
    }

    pub(crate) fn cmd_menu_chk(&mut self, ui: &mut egui::Ui, mut value: bool, id: &str) {
        let label = menu_label_in(id, &self.prefs.shortcuts, self.lang());
        if ui.checkbox(&mut value, label).changed() {
            self.exec_cmd(Some(ui.ctx()), id);
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let mut pick_lang: Option<Language> = None;
        egui::menu::bar(ui, |ui| {
            ui.menu_button(tr(lang, "Arquivo"), |ui| {
                self.cmd_menu_btn(ui, "file.new");
                self.cmd_menu_btn(ui, "file.open");
                self.cmd_menu_btn(ui, "file.save");
                self.cmd_menu_btn(ui, "file.save_as");
                self.cmd_menu_btn(ui, "file.save_version");
                ui.separator();
                self.cmd_menu_btn(ui, "file.import_media");
                self.cmd_menu_btn(ui, "file.import_sequence");
                self.cmd_menu_btn(ui, "file.convert_media");
                self.cmd_menu_btn(ui, "file.import_otio");
                self.cmd_menu_btn(ui, "file.export_otio");
                ui.menu_button(tr(lang, "Intercâmbio & Legendas"), |ui| {
                    self.cmd_menu_btn(ui, "file.import_fcpxml");
                    self.cmd_menu_btn(ui, "file.export_fcpxml");
                    self.cmd_menu_btn(ui, "file.import_fcp7");
                    self.cmd_menu_btn(ui, "file.export_fcp7");
                    self.cmd_menu_btn(ui, "file.import_aaf");
                    self.cmd_menu_btn(ui, "file.export_aaf");
                    ui.separator();
                    self.cmd_menu_btn(ui, "file.import_edl");
                    self.cmd_menu_btn(ui, "file.export_edl");
                    ui.separator();
                    self.cmd_menu_btn(ui, "file.import_subtitles");
                    self.cmd_menu_btn(ui, "file.export_srt");
                    self.cmd_menu_btn(ui, "file.export_vtt");
                });
                ui.separator();
                self.cmd_menu_btn(ui, "file.render");
                self.cmd_menu_btn(ui, "file.send_to_encoder");
                self.cmd_menu_btn(ui, "file.snapshot");
                ui.separator();
                self.cmd_menu_btn(ui, "file.project_props");
                ui.separator();
                self.cmd_menu_btn(ui, "file.exit");
            });
            ui.menu_button(tr(lang, "Editar"), |ui| {
                self.cmd_menu_btn(ui, "edit.undo");
                self.cmd_menu_btn(ui, "edit.redo");
                ui.separator();
                self.cmd_menu_btn(ui, "edit.cut");
                self.cmd_menu_btn(ui, "edit.copy");
                self.cmd_menu_btn(ui, "preview.copy_frame");
                self.cmd_menu_btn(ui, "edit.paste");
                self.cmd_menu_btn(ui, "edit.paste_insert");
                self.cmd_menu_btn(ui, "edit.paste_attributes");
                self.cmd_menu_btn(ui, "edit.paste_attributes_selective");
                self.cmd_menu_btn(ui, "edit.delete");
                ui.separator();
                self.cmd_menu_btn(ui, "edit.split");
                self.cmd_menu_btn(ui, "edit.smart_split");
                self.cmd_menu_btn(ui, "edit.heal_events");
                self.cmd_menu_btn(ui, "edit.detect_scenes");
                self.cmd_menu_btn(ui, "edit.expanded_edit_mode");
                if self.session.expanded_edit.is_some() {
                    self.cmd_menu_btn(ui, "edit.expanded_nudge_left");
                    self.cmd_menu_btn(ui, "edit.expanded_nudge_right");
                }
                ui.menu_button(tr(lang, "Take"), |ui| {
                    self.cmd_menu_btn(ui, "event.take_chooser");
                    ui.separator();
                    self.cmd_menu_btn(ui, "event.next_take");
                    self.cmd_menu_btn(ui, "event.prev_take");
                    self.cmd_menu_btn(ui, "event.delete_active_take");
                });
                ui.menu_button("Tom (Pitch)", |ui| {
                    self.cmd_menu_btn(ui, "event.pitch_up");
                    self.cmd_menu_btn(ui, "event.pitch_down");
                    self.cmd_menu_btn(ui, "event.pitch_up_oct");
                    self.cmd_menu_btn(ui, "event.pitch_down_oct");
                    ui.separator();
                    self.cmd_menu_btn(ui, "event.toggle_pitch_lock");
                    self.cmd_menu_btn(ui, "event.pitch_reset");
                });
                ui.menu_button("Sync Link", |ui| {
                    self.cmd_menu_btn(ui, "event.sync_link_create");
                    self.cmd_menu_btn(ui, "event.sync_link_select");
                    self.cmd_menu_btn(ui, "event.sync_link_unlink");
                });
                ui.menu_button("Ferramenta de edição", |ui| {
                    for id in [
                        "edit.tool.normal",
                        "edit.tool.envelope",
                        "edit.tool.selection",
                        "edit.tool.zoom",
                    ] {
                        self.cmd_menu_btn(ui, id);
                    }
                    ui.separator();
                    for mode in neoveg_engine::session::EditMode::NORMAL_MODES {
                        self.cmd_menu_btn(ui, mode.command_id());
                    }
                    ui.separator();
                    for id in [
                        "edit.tool.axis.both",
                        "edit.tool.axis.horizontal",
                        "edit.tool.axis.vertical",
                    ] {
                        self.cmd_menu_btn(ui, id);
                    }
                    ui.separator();
                    self.cmd_menu_btn(ui, "edit.tool.next");
                    self.cmd_menu_btn(ui, "edit.tool.prev");
                    self.cmd_menu_btn(ui, "edit.tool.mode.next");
                });
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
                ui.menu_button(tr(lang, "Propriedades"), |ui| {
                    ui.menu_button(tr(lang, "Layout"), |ui| {
                        self.cmd_menu_btn(ui, "edit.properties.layout");
                        ui.separator();
                        ui.label(
                            egui::RichText::new(tr(lang, "Tipos de layout"))
                                .small()
                                .weak(),
                        );
                        for (cmd_id, lt) in [
                            (
                                "edit.properties.layout.sonic_foundry",
                                LayoutType::SonicFoundry,
                            ),
                            ("edit.properties.layout.sony", LayoutType::Sony),
                            ("edit.properties.layout.magix", LayoutType::Magix),
                        ] {
                            let selected = self.prefs.edit_layout.layout_type == lt;
                            let label = menu_label_in(cmd_id, &self.prefs.shortcuts, lang);
                            if ui.selectable_label(selected, label).clicked() {
                                self.exec_cmd(Some(ui.ctx()), cmd_id);
                                ui.close_menu();
                            }
                        }
                    });
                });
            });
            ui.menu_button(tr(lang, "Exibir"), |ui| {
                for tab in DockPanel::ALL {
                    let open = self.is_panel_open(tab);
                    let suffix = if self.detached_tabs.contains(&tab) {
                        tr_args(
                            lang,
                            "{panel} (flutuante)",
                            &[("panel", &tab.name_in(lang))],
                        )
                    } else {
                        tab.name_in(lang).to_string()
                    };
                    if ui.selectable_label(open, suffix).clicked() {
                        self.exec_cmd(Some(ui.ctx()), tab.command_id());
                        ui.close_menu();
                    }
                }
                ui.separator();
                let pv_open = (self.dock_visible && self.preview_visible && !self.preview_detached)
                    || self.preview_detached;
                let pv_label = if self.preview_detached {
                    tr(lang, "Prévia de Vídeo (flutuante)")
                } else {
                    tr(lang, "Prévia de Vídeo")
                };
                if ui.selectable_label(pv_open, pv_label).clicked() {
                    self.exec_cmd(Some(ui.ctx()), "view.preview");
                    ui.close_menu();
                }
                let mb_open =
                    (self.dock_visible && self.master_bus_visible && !self.master_bus_detached)
                        || self.master_bus_detached;
                let mb_label = if self.master_bus_detached {
                    tr(lang, "Master Bus (flutuante)")
                } else {
                    tr(lang, "Master Bus")
                };
                if ui.selectable_label(mb_open, mb_label).clicked() {
                    self.exec_cmd(Some(ui.ctx()), "view.master_bus");
                    ui.close_menu();
                }
                if ui
                    .selectable_label(
                        self.preview_detached,
                        tr(lang, "Prévia de Vídeo em janela separada"),
                    )
                    .clicked()
                {
                    self.preview_detached = !self.preview_detached;
                    if !self.preview_detached {
                        self.preview_visible = true;
                    }
                    ui.close_menu();
                }
                if ui
                    .selectable_label(
                        self.master_bus_detached,
                        tr(lang, "Master Bus em janela separada"),
                    )
                    .clicked()
                {
                    self.master_bus_detached = !self.master_bus_detached;
                    if !self.master_bus_detached {
                        self.master_bus_visible = true;
                    }
                    ui.close_menu();
                }
                // F11, Shift+F11 e Ctrl+F11 do VEGAS Pro, pela Tabela de Comandos.
                for (cmd_id, on) in [
                    ("view.toggle_dock_area", self.dock_visible),
                    ("view.toggle_track_list", !self.tl.header_collapsed()),
                    ("view.maximize_timeline", self.timeline_restore.is_some()),
                ] {
                    let label = menu_label_in(cmd_id, &self.prefs.shortcuts, lang);
                    if ui.selectable_label(on, label).clicked() {
                        self.exec_cmd(Some(ui.ctx()), cmd_id);
                        ui.close_menu();
                    }
                }
                if self.dock_visible
                    && !self.detached_tabs.contains(&self.dock_tab)
                    && ui
                        .button(tr(lang, "Destacar janela da aba atual"))
                        .clicked()
                {
                    self.detach_dock_tab(self.dock_tab);
                    ui.close_menu();
                }
                if (!self.detached_tabs.is_empty()
                    || self.preview_detached
                    || self.master_bus_detached)
                    && ui
                        .button(tr(lang, "Acoplar todas as janelas flutuantes"))
                        .clicked()
                {
                    if self.preview_detached {
                        self.preview_visible = true;
                    }
                    if self.master_bus_detached {
                        self.master_bus_visible = true;
                    }
                    self.detached_tabs.clear();
                    self.preview_detached = false;
                    self.master_bus_detached = false;
                    self.dock_visible = true;
                    ui.close_menu();
                }
                ui.menu_button(tr(lang, "Layouts de janela"), |ui| {
                    for preset in layout::builtin_presets() {
                        let label = layout::localized_preset_name(&preset.name, lang);
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
                                    Err(e) => {
                                        let err_s = e.to_string();
                                        self.set_status(tr_args(
                                            lang,
                                            "Erro no layout: {err}",
                                            &[("err", &err_s)],
                                        ));
                                    }
                                }
                                ui.close_menu();
                            }
                        }
                    }
                    ui.separator();
                    if ui
                        .button(tr(lang, "Salvar layout atual (.neoveglayout)..."))
                        .clicked()
                    {
                        self.dlg.save_layout_name = tr(lang, "Meu Layout").into();
                        self.dlg.save_layout_open = true;
                        ui.close_menu();
                    }
                    if ui
                        .button(tr(lang, "Carregar arquivo .neoveglayout..."))
                        .clicked()
                    {
                        if let Some(p) = rfd::FileDialog::new()
                            .add_filter(tr(lang, "Layout Neoveg"), &["neoveglayout", "json"])
                            .pick_file()
                        {
                            match layout::load_layout_file(&p) {
                                Ok(l) => {
                                    self.apply_window_layout(l);
                                    self.prefs.window_layout =
                                        Some(p.to_string_lossy().to_string());
                                    let _ = self.prefs.save();
                                }
                                Err(e) => {
                                    let err_s = e.to_string();
                                    self.set_status(tr_args(
                                        lang,
                                        "Erro no layout: {err}",
                                        &[("err", &err_s)],
                                    ));
                                }
                            }
                        }
                        ui.close_menu();
                    }
                });
                ui.menu_button(tr(lang, "Tema"), |ui| {
                    for t in ThemeId::ALL {
                        if ui
                            .selectable_label(self.theme_id == t, t.label_in(lang))
                            .clicked()
                        {
                            self.set_theme(ui.ctx(), t, true);
                            ui.close_menu();
                        }
                    }
                });
                ui.separator();
                self.cmd_menu_chk(ui, self.opts.show_envelopes, "view.envelopes");
                self.cmd_menu_chk(ui, self.opts.show_waveforms, "view.waveforms");
                self.cmd_menu_btn(ui, "view.waveform_zoom_in");
                self.cmd_menu_btn(ui, "view.waveform_zoom_out");
                self.cmd_menu_chk(ui, self.opts.show_thumbs, "view.thumbs");
                self.cmd_menu_chk(ui, self.opts.safe_areas, "view.safe_areas");
                self.cmd_menu_chk(ui, self.opts.show_bus_tracks, "view.bus_tracks");
                ui.menu_button("Modo de exibição (Mídia do Projeto)", |ui| {
                    self.cmd_menu_btn(ui, "media.view.thumbnails");
                    self.cmd_menu_btn(ui, "media.view.list");
                    self.cmd_menu_btn(ui, "media.view.details");
                    self.cmd_menu_btn(ui, "media.view.storyboard");
                });
                ui.menu_button("Prévia de Vídeo", |ui| {
                    self.cmd_menu_btn(ui, "preview.split_toggle");
                    self.cmd_menu_btn(ui, "preview.split_bypass");
                    self.cmd_menu_btn(ui, "preview.split_clipboard");
                    ui.separator();
                    self.cmd_menu_btn(ui, "preview.zoom_fit");
                    self.cmd_menu_btn(ui, "preview.zoom_100");
                    self.cmd_menu_btn(ui, "preview.zoom_in");
                    self.cmd_menu_btn(ui, "preview.zoom_out");
                    ui.separator();
                    self.cmd_menu_chk(ui, self.external_monitor, "preview.external_monitor");
                });
                ui.separator();
                ui.label(tr(lang, "Formato de tempo:"));
                for fmt in TimeFormat::ALL {
                    ui.radio_value(&mut self.opts.time_format, fmt, fmt.name_in(lang));
                }
            });
            ui.menu_button(tr(lang, "Inserir"), |ui| {
                self.cmd_menu_btn(ui, "insert.track_video");
                self.cmd_menu_btn(ui, "insert.track_audio");
                self.cmd_menu_btn(ui, "insert.track_adjustment");
                self.cmd_menu_btn(ui, "insert.track_folder");
                self.cmd_menu_btn(ui, "insert.audio_bus");
                ui.separator();
                self.cmd_menu_btn(ui, "insert.nested_timeline");
                ui.separator();
                self.cmd_menu_btn(ui, "insert.marker");
                self.cmd_menu_btn(ui, "insert.marker_cd_track");
                self.cmd_menu_btn(ui, "insert.marker_cd_index");
                self.cmd_menu_btn(ui, "insert.marker_command");
                self.cmd_menu_btn(ui, "insert.region");
                self.cmd_menu_btn(ui, "insert.subtitles_from_regions");
                ui.separator();
                self.cmd_menu_btn(ui, "insert.gen_text");
                self.cmd_menu_btn(ui, "insert.gen_solid");
                self.cmd_menu_btn(ui, "insert.empty_event");
                self.cmd_menu_btn(ui, "media.insert_storyboard");
                ui.separator();
                ui.label(tr(lang, "Envelopes da trilha selecionada:"));
                self.cmd_menu_btn(ui, "insert.env_volume");
                self.cmd_menu_btn(ui, "insert.env_pan");
            });
            ui.menu_button(tr(lang, "Ferramentas"), |ui| {
                self.cmd_menu_btn(ui, "tools.fx_master_audio");
                self.cmd_menu_btn(ui, "tools.fx_master_video");
                ui.menu_button("Mídia do Projeto", |ui| {
                    self.cmd_menu_btn(ui, "media.new_bin");
                    self.cmd_menu_btn(ui, "media.new_smart_bin");
                    self.cmd_menu_btn(ui, "media.new_storyboard");
                    ui.separator();
                    self.cmd_menu_btn(ui, "media.select_events");
                    self.cmd_menu_btn(ui, "media.replace");
                    self.cmd_menu_btn(ui, "tools.remove_unused_media");
                });
                ui.separator();
                self.cmd_menu_btn(ui, "tools.event_props");
                self.cmd_menu_btn(ui, "tools.pan_crop");
                self.cmd_menu_btn(ui, "tools.fx_event");
                self.cmd_menu_btn(ui, "tools.motion_track");
                self.cmd_menu_btn(ui, "tools.apply_audio_fx");
                self.cmd_menu_btn(ui, "tools.reverse");
                self.cmd_menu_btn(ui, "tools.freeze_frame");
                ui.separator();
                self.cmd_menu_btn(ui, "tools.build_proxy");
                self.cmd_menu_btn(ui, "tools.prerender");
                self.cmd_menu_btn(ui, "track.render_to_new_track");
                self.cmd_menu_btn(ui, "track.set_defaults");
                ui.menu_button(tr(lang, "Multicâmera"), |ui| {
                    self.cmd_menu_btn(ui, "tools.multicam_create");
                    self.cmd_menu_chk(ui, self.opts.multicam_mode, "tools.multicam_mode");
                    ui.separator();
                    self.cmd_menu_btn(ui, "tools.multicam_cut_1");
                    self.cmd_menu_btn(ui, "tools.multicam_cut_2");
                    self.cmd_menu_btn(ui, "tools.multicam_cut_3");
                    self.cmd_menu_btn(ui, "tools.multicam_cut_4");
                });
                ui.menu_button(tr(lang, "Composição de trilhas (Pai/Filho)"), |ui| {
                    self.cmd_menu_btn(ui, "track.toggle_folder");
                    self.cmd_menu_btn(ui, "track.make_child");
                    self.cmd_menu_btn(ui, "track.unmake_child");
                    self.cmd_menu_btn(ui, "track.mask_cycle");
                });
                self.cmd_menu_btn(ui, "preview.ram_build");
                self.cmd_menu_btn(ui, "preview.ram_clear");
                ui.menu_button(tr(lang, "Neoveg Encoder"), |ui| {
                    self.cmd_menu_btn(ui, "view.tab.encoder");
                    self.cmd_menu_btn(ui, "file.send_to_encoder");
                    ui.separator();
                    self.cmd_menu_btn(ui, "tools.encoder_start");
                    self.cmd_menu_btn(ui, "tools.encoder_pause");
                    self.cmd_menu_btn(ui, "tools.encoder_stop_current");
                    self.cmd_menu_btn(ui, "tools.encoder_stop_queue");
                });
                ui.separator();
                self.cmd_menu_btn(ui, "tools.run_script");
                ui.separator();
                self.cmd_menu_btn(ui, "tools.integrations");
                ui.menu_button("Aplicativos externos (ArtCraft & Blender)", |ui| {
                    self.cmd_menu_btn(ui, "tools.send_photocraft");
                    self.cmd_menu_btn(ui, "tools.send_vectorcraft");
                    self.cmd_menu_btn(ui, "tools.send_soundcraft");
                    self.cmd_menu_btn(ui, "tools.send_effectcraft");
                    self.cmd_menu_btn(ui, "tools.send_anicraft");
                    self.cmd_menu_btn(ui, "tools.send_filmcraft");
                    ui.separator();
                    self.cmd_menu_btn(ui, "tools.blender_render");
                    self.cmd_menu_btn(ui, "tools.blender_export_script");
                    self.cmd_menu_btn(ui, "tools.blender_import_camera");
                });
            });
            ui.menu_button(tr(lang, "Opções"), |ui| {
                self.cmd_menu_chk(ui, self.opts.snapping, "options.snapping");
                self.cmd_menu_chk(ui, self.opts.snap_to_grid, "options.snap_to_grid");
                ui.menu_button("Grade da régua", |ui| {
                    for g in neoveg_engine::session::RulerGridSpacing::ALL {
                        if ui.radio(self.opts.ruler_grid == g, g.label()).clicked() {
                            self.opts.ruler_grid = g;
                            self.set_status(format!("Grade da régua: {}", g.label()));
                            ui.close_menu();
                        }
                    }
                });
                self.cmd_menu_chk(ui, self.opts.quantize, "options.quantize");
                self.cmd_menu_chk(ui, self.opts.auto_ripple, "options.auto_ripple");
                self.cmd_menu_chk(ui, self.opts.ignore_grouping, "options.ignore_grouping");
                self.cmd_menu_chk(ui, self.opts.slip_all_takes, "options.slip_all_takes");
                self.cmd_menu_chk(ui, self.opts.loop_playback, "options.loop_playback");
                self.cmd_menu_chk(ui, self.opts.preview_mute_audio, "preview.mute_audio");
                self.cmd_menu_chk(ui, self.opts.preview_mute_video, "preview.mute_video");
                self.cmd_menu_chk(
                    ui,
                    self.opts.bypass_all_audio_fx,
                    "options.bypass_all_audio_fx",
                );
                ui.separator();
                self.cmd_menu_btn(ui, "options.customize_toolbar");
                ui.separator();
                let sr_s = self.audio.sr.to_string();
                ui.label(tr_args(
                    lang,
                    "Dispositivo de áudio: {dev} ({sr} Hz)",
                    &[("dev", &self.audio.device_name), ("sr", &sr_s)],
                ));
                ui.separator();
                ui.menu_button(tr(lang, "Idioma"), |ui| {
                    for candidate in Language::ALL {
                        if ui
                            .selectable_label(lang == candidate, candidate.display_option(lang))
                            .clicked()
                        {
                            pick_lang = Some(candidate);
                            ui.close_menu();
                        }
                    }
                });
                ui.separator();
                self.cmd_menu_btn(ui, "options.prefs");
            });
            ui.menu_button(tr(lang, "Ajuda"), |ui| {
                self.cmd_menu_btn(ui, "help.welcome");
                self.cmd_menu_btn(ui, "help.splash");
                self.cmd_menu_btn(ui, "help.shortcuts");
                self.cmd_menu_btn(ui, "help.about");
            });
        });
        if let Some(new_lang) = pick_lang {
            self.apply_language(Some(ui.ctx()), new_lang, true);
        }
    }

    fn cmd_hover_text(&self, id: &str, fallback: Option<&str>) -> String {
        let lang = self.lang();
        commands::find_command(id)
            .map(|c| {
                let lbl = c.label_in(lang);
                match resolved_shortcut(c, &self.prefs.shortcuts) {
                    Some(sc) => format!("{lbl} ({})", sc.format_in(lang)),
                    None => lbl.to_string(),
                }
            })
            .unwrap_or_else(|| fallback.unwrap_or(id).to_string())
    }

    pub(crate) fn cmd_tool_btn(
        &mut self,
        ui: &mut egui::Ui,
        icon: Icon,
        text: Option<&str>,
        id: &str,
    ) {
        let hover = self.cmd_hover_text(id, text);
        let enabled = self.cmd_enabled(id);
        if icons::icon_button(ui, icon, text, false, enabled, &self.theme)
            .on_hover_text(hover)
            .clicked()
        {
            self.exec_cmd(Some(ui.ctx()), id);
        }
    }

    pub(crate) fn cmd_tool_toggle(
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

    pub(crate) fn open_customize_toolbar(&mut self) {
        self.dlg.customize_toolbar_open = true;
        self.dlg.customize_toolbar_draft = Some(self.prefs.toolbar_buttons.clone());
    }

    fn toolbar_button_visual(&self, id: &str) -> (Icon, Option<&'static str>, Option<bool>) {
        match id {
            "file.new" => (Icon::NewFile, None, None),
            "file.open" => (Icon::OpenFolder, None, None),
            "file.save" => (Icon::SaveDisk, None, None),
            "file.render" => (Icon::RenderClapper, None, None),
            "view.tab.encoder" => (Icon::EncoderQueue, Some("Encoder"), None),
            "file.project_props" => (Icon::GearProps, None, None),
            "file.import_media" => (Icon::ImportMedia, None, None),
            "edit.cut" => (Icon::Cut, None, None),
            "edit.copy" => (Icon::Copy, None, None),
            "edit.paste" => (Icon::Paste, None, None),
            "edit.undo" => (Icon::Undo, None, None),
            "edit.redo" => (Icon::Redo, None, None),
            "options.snapping" => (Icon::MagnetSnap, None, Some(self.opts.snapping)),
            "options.snap_to_grid" => (Icon::TimelineGrid, None, Some(self.opts.snap_to_grid)),
            "options.auto_ripple" => (Icon::AutoRipple, None, Some(self.opts.auto_ripple)),
            "view.envelopes" => (Icon::Envelopes, None, Some(self.opts.show_envelopes)),
            "options.ignore_grouping" => {
                (Icon::IgnoreGrouping, None, Some(self.opts.ignore_grouping))
            }
            "options.loop_playback" => (Icon::LoopPlayback, None, Some(self.opts.loop_playback)),
            "options.bypass_all_audio_fx" => (
                Icon::FxAudio,
                Some("Bypass"),
                Some(self.opts.bypass_all_audio_fx),
            ),
            "preview.mute_audio" => (Icon::MuteAudio, None, Some(self.opts.preview_mute_audio)),
            "preview.mute_video" => (Icon::MuteVideo, None, Some(self.opts.preview_mute_video)),
            "preview.split_toggle" => (
                Icon::SplitPreview,
                None,
                Some(!matches!(
                    self.opts.preview_split_mode,
                    neoveg_engine::playback::PreviewSplitMode::Off
                )),
            ),
            "edit.properties.layout" => (Icon::LayoutWindow, None, None),
            "tools.integrations" => (Icon::ExternalApps, None, None),
            "insert.marker" => (Icon::InsertMarker, Some("M"), None),
            "insert.marker_cd_track" => (Icon::AudioTrack, Some("CD"), None),
            "insert.marker_command" => (Icon::FxSparkle, Some("CMD"), None),
            "track.cycle_automation" => (Icon::Envelopes, Some("Auto"), None),
            other => {
                let lbl = commands::find_command(other).map(|c| c.label);
                (Icon::GearProps, lbl, None)
            }
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let items = self.prefs.toolbar_buttons.clone();
        let resp = ui.horizontal_wrapped(|ui| {
            for id in &items {
                if id == "|" {
                    ui.separator();
                    continue;
                }
                let (icon, text, toggle) = self.toolbar_button_visual(id);
                if let Some(active) = toggle {
                    self.cmd_tool_toggle(ui, active, icon, text, id);
                } else {
                    self.cmd_tool_btn(ui, icon, text, id);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("⋮")
                    .on_hover_text(tr(lang, "Personalizar barra de ferramentas..."))
                    .clicked()
                {
                    self.open_customize_toolbar();
                }
            });
            if self.loading > 0 {
                let n_s = self.loading.to_string();
                ui.separator();
                crate::splash::spinner_inline(ui, 15.0, ui.ctx().input(|i| i.time), &self.theme);
                ui.label(tr_args(lang, "carregando {n}...", &[("n", &n_s)]));
            }
        });
        resp.response.context_menu(|ui| {
            if ui.button("Personalizar barra de ferramentas...").clicked() {
                self.open_customize_toolbar();
                ui.close_menu();
            }
            if ui.button("Restaurar barra padrão").clicked() {
                self.prefs.toolbar_buttons = neoveg_common::prefs::default_toolbar_buttons();
                let _ = self.prefs.save();
                ui.close_menu();
            }
        });
    }

    fn time_readout_chip(&self, ui: &mut egui::Ui, label: &str, t: Ticks, tooltip: &str) {
        let txt = self.fmt_time(t);
        egui::Frame::new()
            .fill(self.theme.extreme_bg)
            .stroke(egui::Stroke::new(1.0_f32, self.theme.border))
            .corner_radius(2.0)
            .inner_margin(egui::Margin::symmetric(5, 1))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    let name = egui::RichText::new(label)
                        .small()
                        .color(self.theme.text_muted);
                    let value = egui::RichText::new(txt)
                        .monospace()
                        .small()
                        .color(self.theme.text_primary);
                    // a barra é desenhada da direita para a esquerda e o
                    // `horizontal` herda isso: aí o valor entra primeiro, para
                    // o nome ficar à esquerda dele na tela
                    if ui.layout().prefer_right_to_left() {
                        ui.label(value);
                        ui.label(name);
                    } else {
                        ui.label(name);
                        ui.label(value);
                    }
                });
            })
            .response
            .on_hover_text(tooltip);
    }

    fn transport_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let playing = self.is_playing();
            // Bloco 1: Transporte (Manual VEGAS Pro 21 §6)
            self.cmd_tool_toggle(
                ui,
                self.opts.loop_playback,
                Icon::LoopPlayback,
                None,
                "options.loop_playback",
            );
            self.cmd_tool_btn(ui, Icon::PlayFromStart, None, "transport.play_from_start");
            self.cmd_tool_toggle(
                ui,
                playing,
                if playing { Icon::Pause } else { Icon::Play },
                None,
                "transport.play_toggle",
            );
            self.cmd_tool_btn(ui, Icon::Stop, None, "transport.stop");
            self.cmd_tool_toggle(
                ui,
                self.recorder.is_some(),
                Icon::RecordDot,
                None,
                "transport.record",
            );
            self.cmd_tool_btn(ui, Icon::GoStart, None, "transport.project_start");
            self.cmd_tool_btn(ui, Icon::GoEnd, None, "transport.project_end");
            self.cmd_tool_btn(ui, Icon::PrevFrame, None, "transport.frame_back");
            self.cmd_tool_btn(ui, Icon::NextFrame, None, "transport.frame_fwd");
            ui.separator();

            // Ferramentas de edição (manual do VEGAS Pro 21, pp. 75–76 e 236):
            // Normal (com seletor de modos: Normal/Shuffle/Slip/Slide/Stretch/SplitTrim/Roll),
            // Envelope, Seleção e Zoom.
            let tool = self.session.edit_tool;
            let active_mode = self.session.edit_mode;
            self.cmd_tool_toggle(
                ui,
                tool == neoveg_engine::session::EditTool::Normal,
                icons::normal_mode_icon(active_mode),
                None,
                "edit.tool.normal",
            );
            // A seta é desenhada em vetor: a fonte da interface não tem `▼`.
            let arrow_btn = egui::Button::new("")
                .min_size(egui::vec2(12.0, ui.spacing().interact_size.y))
                .frame(false);
            let modes = egui::menu::menu_custom_button(ui, arrow_btn, |ui| {
                for mode in neoveg_engine::session::EditMode::NORMAL_MODES {
                    let id = mode.command_id();
                    let on =
                        tool == neoveg_engine::session::EditTool::Normal && active_mode == mode;
                    let label = menu_label_in(id, &self.prefs.shortcuts, self.lang());
                    if ui.selectable_label(on, label).clicked() {
                        self.exec_cmd(Some(ui.ctx()), id);
                        ui.close_menu();
                    }
                }
            });
            crate::scrollbars::paint_arrow_glyph(
                ui.painter(),
                modes.response.rect,
                crate::scrollbars::ArrowDir::Down,
                ui.visuals().text_color(),
            );
            modes.response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    "Selecionar modo da ferramenta Normal",
                )
            });
            modes
                .response
                .on_hover_text(format!("Modo da ferramenta Normal: {}", active_mode.name()));
            self.cmd_tool_toggle(
                ui,
                tool == neoveg_engine::session::EditTool::Envelope,
                Icon::EnvelopeTool,
                None,
                "edit.tool.envelope",
            );
            self.cmd_tool_toggle(
                ui,
                tool == neoveg_engine::session::EditTool::Selection,
                Icon::SelectionTool,
                None,
                "edit.tool.selection",
            );
            self.cmd_tool_toggle(
                ui,
                tool == neoveg_engine::session::EditTool::Zoom,
                Icon::ZoomGlass,
                None,
                "edit.tool.zoom",
            );
            if matches!(
                tool,
                neoveg_engine::session::EditTool::Selection
                    | neoveg_engine::session::EditTool::Zoom
            ) {
                let axis = self.session.box_axis;
                let axis_btn = egui::Button::new(axis.short_name())
                    .min_size(egui::vec2(38.0, ui.spacing().interact_size.y));
                let axis_menu = egui::menu::menu_custom_button(ui, axis_btn, |ui| {
                    for (mode, cmd_id) in [
                        (
                            neoveg_engine::session::BoxAxisMode::Both,
                            "edit.tool.axis.both",
                        ),
                        (
                            neoveg_engine::session::BoxAxisMode::Horizontal,
                            "edit.tool.axis.horizontal",
                        ),
                        (
                            neoveg_engine::session::BoxAxisMode::Vertical,
                            "edit.tool.axis.vertical",
                        ),
                    ] {
                        if ui.selectable_label(axis == mode, mode.name()).clicked() {
                            self.exec_cmd(Some(ui.ctx()), cmd_id);
                            ui.close_menu();
                        }
                    }
                });
                axis_menu.response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        "Selecionar eixo de Seleção/Zoom",
                    )
                });
                axis_menu
                    .response
                    .on_hover_text(format!("Eixo da caixa: {}", axis.name()));
            }
            ui.separator();

            // Bloco 2: edição, na ordem da tabela do manual do VEGAS Pro 21 (pp. 75–78):
            // Excluir, Aparar, Aparar início, Aparar fim, Dividir, Smart Split,
            // Juntar eventos, Travar, Inserir marcador, Inserir região, Encaixe,
            // Ripple automático, Ignorar agrupamento e Color Grading.
            for (icon, id) in [
                (Icon::DeleteTrash, "edit.delete"),
                (Icon::TrimSelection, "edit.trim_to_selection"),
                (Icon::TrimStart, "edit.trim_start"),
                (Icon::TrimEnd, "edit.trim_end"),
                (Icon::Split, "edit.split"),
                (Icon::SmartSplit, "edit.smart_split"),
                (Icon::HealEvents, "edit.heal_events"),
            ] {
                self.cmd_tool_btn(ui, icon, None, id);
            }
            let all_locked = self.session.selection_all_locked();
            self.cmd_tool_toggle(ui, all_locked, Icon::LockClosed, None, "event.lock");
            self.cmd_tool_btn(ui, Icon::InsertMarker, None, "insert.marker");
            self.cmd_tool_btn(ui, Icon::InsertRegion, None, "insert.region");
            self.cmd_tool_toggle(
                ui,
                self.opts.snapping,
                Icon::MagnetSnap,
                None,
                "options.snapping",
            );
            self.cmd_tool_toggle(
                ui,
                self.opts.auto_ripple,
                Icon::AutoRipple,
                None,
                "options.auto_ripple",
            );
            self.cmd_tool_toggle(
                ui,
                self.opts.ignore_grouping,
                Icon::IgnoreGrouping,
                None,
                "options.ignore_grouping",
            );
            self.cmd_tool_toggle(
                ui,
                self.is_panel_open(DockTab::ColorGrading),
                Icon::ColorWheels,
                None,
                "view.tab.color_grading",
            );
            // do Neoveg, depois dos botões do manual
            ui.separator();
            self.cmd_tool_toggle(
                ui,
                self.opts.show_envelopes,
                Icon::Envelopes,
                None,
                "view.envelopes",
            );
            self.cmd_tool_btn(ui, Icon::PanCrop, None, "tools.pan_crop");

            let lang = self.lang();
            if self.shuttle != 0.0 && (self.shuttle - 1.0).abs() > 1e-6 && playing {
                let x_s = format!("{:+.2}", self.shuttle);
                ui.label(
                    egui::RichText::new(tr_args(lang, "Shuttle {x}x", &[("x", &x_s)])).small(),
                );
            }

            // Bloco 3 (direita): Zoom horizontal + Selection Start/End/Length boxes (Manual p. 73)
            let avail_w = ui.available_width();
            if avail_w >= 135.0 {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (sel_start, sel_end, sel_len) = match self.time_sel {
                        Some((a, b)) if b >= a => (a, b, b - a),
                        _ => (self.view_time(), Ticks::ZERO, Ticks::ZERO),
                    };
                    // Botão `More (...)`: ferramentas de edição que não cabem na
                    // barra, despachadas pelo id da Tabela Única (§3.3).
                    ui.menu_button("…", |ui| {
                        self.more_tools_menu(ui, (sel_start, sel_end, sel_len), avail_w);
                    })
                    .response
                    .on_hover_text(tr(lang, "Mais ferramentas de edição (More)"));
                    let tam_lbl = tr(lang, "Tam:");
                    let fim_lbl = tr(lang, "Fim:");
                    let pos_lbl = if self.time_sel.is_some() {
                        tr(lang, "In:")
                    } else {
                        tr(lang, "Pos:")
                    };
                    if avail_w >= 430.0 {
                        self.time_readout_chip(
                            ui,
                            tam_lbl,
                            sel_len,
                            tr(lang, "Duração da seleção (Selection Length)"),
                        );
                        self.time_readout_chip(
                            ui,
                            fim_lbl,
                            sel_end,
                            tr(lang, "Fim da seleção (Selection End)"),
                        );
                        self.time_readout_chip(
                            ui,
                            pos_lbl,
                            sel_start,
                            tr(lang, "Início da seleção / posição do cursor"),
                        );
                        ui.separator();
                    } else if avail_w >= 260.0 {
                        self.time_readout_chip(
                            ui,
                            tam_lbl,
                            sel_len,
                            tr(lang, "Duração da seleção (Selection Length)"),
                        );
                        ui.separator();
                    }
                    if ui
                        .small_button("+")
                        .on_hover_text(
                            self.cmd_hover_text(
                                "timeline.zoom_in",
                                Some(tr(lang, "Aproximar zoom")),
                            ),
                        )
                        .clicked()
                    {
                        self.exec_cmd(Some(ui.ctx()), "timeline.zoom_in");
                    }
                    let mut z = self.tl.px_per_sec.ln();
                    if ui
                        .add_sized(
                            [72.0, 16.0],
                            egui::Slider::new(
                                &mut z,
                                crate::timeline::TimelineState::MIN_PX_PER_SEC.ln()
                                    ..=crate::timeline::TimelineState::MAX_PX_PER_SEC.ln(),
                            )
                            .show_value(false),
                        )
                        .on_hover_text(tr(lang, "Zoom horizontal (↑/↓, roda do mouse)"))
                        .changed()
                    {
                        self.tl.px_per_sec = crate::timeline::TimelineState::clamp_pps(z.exp());
                    }
                    if ui
                        .small_button("-")
                        .on_hover_text(
                            self.cmd_hover_text(
                                "timeline.zoom_out",
                                Some(tr(lang, "Afastar zoom")),
                            ),
                        )
                        .clicked()
                    {
                        self.exec_cmd(Some(ui.ctx()), "timeline.zoom_out");
                    }
                    icons::paint_inline(ui, Icon::ZoomGlass, 13.0, &self.theme);
                });
            }
        });
    }

    /// Controle compartilhado de velocidade de reprodução (Playback Rate) usado na barra de transporte
    /// e no rodapé da lista de trilhas.
    pub(crate) fn playback_rate_control(&mut self, ui: &mut egui::Ui, compact: bool) {
        let lang = self.lang();
        let mut rate_changed = false;
        let playing = self.is_playing();
        if compact {
            ui.label(
                egui::RichText::new(tr(lang, "Taxa:"))
                    .small()
                    .color(self.theme.text_muted),
            );
        } else {
            icons::icon_label(ui, Icon::SpeedGauge, 14.0, tr(lang, "Taxa:"), &self.theme);
        }
        if ui
            .small_button(format!("{:.2}×", self.playback_rate))
            .on_hover_text(tr(
                lang,
                "Taxa de reprodução contínua — clique para restaurar 1,00×",
            ))
            .clicked()
        {
            self.playback_rate = 1.0;
            rate_changed = true;
        }
        let slider_w = if compact {
            ui.available_width().clamp(40.0, 120.0)
        } else {
            92.0
        };
        rate_changed |= ui
            .add_sized(
                [slider_w, 18.0],
                egui::Slider::new(&mut self.playback_rate, 0.25..=4.0)
                    .logarithmic(true)
                    .show_value(false),
            )
            .on_hover_text(tr(lang, "Taxa de reprodução contínua (Playback Rate)"))
            .changed();
        if rate_changed && playing {
            self.shuttle = self.playback_rate;
            let mut s = self
                .audio
                .shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            s.speed = self.playback_rate;
        }
        if !compact && self.shuttle != 0.0 && (self.shuttle - 1.0).abs() > 1e-6 && playing {
            let x_s = format!("{:+.2}", self.shuttle);
            ui.label(tr_args(lang, "Shuttle {x}x", &[("x", &x_s)]));
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        ui.horizontal(|ui| {
            if self.status_at.elapsed().as_secs() < 20 && !self.status.is_empty() {
                ui.label(&self.status);
            } else {
                ui.label(
                    egui::RichText::new(tr(lang, "Pronto"))
                        .small()
                        .color(self.theme.text_muted),
                );
            }
            let dur_str = self.fmt_time(self.project.duration());
            let wide = ui.available_width() >= 680.0;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let s = &self.project.settings;
                let file_name = self
                    .path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| tr(lang, "(sem título)").into());
                let ch_label = if self.master_mono {
                    tr(lang, "Mono (1 ch)")
                } else {
                    tr(lang, "Estéreo (2 ch)")
                };
                let sr_s = s.sample_rate.to_string();
                let w_s = s.width.to_string();
                let h_s = s.height.to_string();
                let fps_s = s.fps.label_in(lang);
                let ms_s = format!("{:.0}", self.preview_ms);
                let info_text = if wide {
                    tr_args(
                        lang,
                        "{ch} | {sr} Hz | {w}x{h} {fps} | prévia {ms} ms | Duração: {dur} | {file}",
                        &[
                            ("ch", &ch_label),
                            ("sr", &sr_s),
                            ("w", &w_s),
                            ("h", &h_s),
                            ("fps", &fps_s),
                            ("ms", &ms_s),
                            ("dur", &dur_str),
                            ("file", &file_name),
                        ],
                    )
                } else {
                    tr_args(
                        lang,
                        "{ch} | {w}x{h} {fps} | {sr} Hz | {file}",
                        &[
                            ("ch", &ch_label),
                            ("sr", &sr_s),
                            ("w", &w_s),
                            ("h", &h_s),
                            ("fps", &fps_s),
                            ("file", &file_name),
                        ],
                    )
                };
                ui.label(
                    egui::RichText::new(info_text)
                        .small()
                        .color(self.theme.text_muted),
                );
                if let Some(job) = &self.render_job {
                    crate::splash::spinner_inline(
                        ui,
                        14.0,
                        ui.ctx().input(|i| i.time),
                        &self.theme,
                    );
                    ui.add(
                        egui::ProgressBar::new(job.progress.fraction())
                            .desired_width(140.0)
                            .show_percentage(),
                    );
                    ui.label(tr(lang, "Renderizando:"));
                } else if let Some((_, _, prog)) = &self.prerender_job {
                    crate::splash::spinner_inline(
                        ui,
                        14.0,
                        ui.ctx().input(|i| i.time),
                        &self.theme,
                    );
                    ui.add(
                        egui::ProgressBar::new(prog.fraction())
                            .desired_width(130.0)
                            .show_percentage(),
                    );
                    ui.label(tr(lang, "Pré-renderizando:"));
                } else if self.loading > 0 {
                    crate::splash::spinner_inline(
                        ui,
                        14.0,
                        ui.ctx().input(|i| i.time),
                        &self.theme,
                    );
                    let n_s = self.loading.to_string();
                    ui.label(tr_args(lang, "Carregando ({n})...", &[("n", &n_s)]));
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(40));
                } else if !self.session.prerendered_ranges.is_empty() {
                    let n_s = self.session.prerendered_ranges.len().to_string();
                    ui.label(
                        egui::RichText::new(tr_args(
                            lang,
                            "Pré-render: {n} trecho(s)",
                            &[("n", &n_s)],
                        ))
                        .small()
                        .color(self.theme.tl_marker),
                    );
                }
            });
        });
    }

    pub(crate) fn snapshot_dialog(&mut self) {
        let lang = self.lang();
        if let Some(p) = rfd::FileDialog::new()
            .add_filter(
                tr(lang, "Imagem"),
                &[
                    "png", "jpg", "tiff", "tga", "bmp", "webp", "dpx", "exr", "qoi", "ppm", "cin",
                    "wbmp", "xbm", "xpm", "ico",
                ],
            )
            .set_file_name("quadro.png")
            .save_file()
        {
            match crate::render::save_snapshot(&self.project, &self.media, self.cursor, &p) {
                Ok(_) => {
                    let p_s = p.display().to_string();
                    self.set_status(tr_args(lang, "Quadro salvo em {path}", &[("path", &p_s)]));
                }
                Err(e) => {
                    let err_s = format!("{e:#}");
                    self.set_status(tr_args(lang, "Erro: {err}", &[("err", &err_s)]));
                }
            }
        }
    }

    pub(crate) fn run_script_dialog(&mut self) {
        let lang = self.lang();
        if self.script_job.is_some() {
            self.set_status(tr(lang, "Já há um script em execução."));
            return;
        }
        let Some(p) = rfd::FileDialog::new()
            .add_filter(tr(lang, "Scripts VEGAS / .NET"), &["cs", "dll"])
            .pick_file()
        else {
            return;
        };
        // o script trabalha sobre uma cópia: a reprodução não pode mexer no cursor
        if self.is_playing() {
            self.stop(true);
        }
        let media_before: std::collections::HashSet<u64> =
            self.session.project.media.iter().map(|m| m.id).collect();
        let mut work = self.session.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let script = p.clone();
        // janelas da camada de Forms própria são desenhadas por esta interface
        let (ui, ui_rx) = crate::script_forms::bridge();
        let options = neoveg_scripting::ScriptRunOptions {
            ui: Some(ui),
            // só para testar a camada própria no Windows
            force_forms_shim: std::env::var_os("NEOVEG_SCRIPTS_FORMS_PROPRIA")
                .is_some_and(|v| v == "1"),
        };
        let spawned = crate::util::spawn("script-net", move || {
            let outcome = match crate::util::catch_isolated(|| {
                neoveg_scripting::run_cs_script_on_session_with(&script, &mut work, &options)
            }) {
                Ok(Ok(msgs)) => ScriptOutcome::Done(Ok(msgs)),
                Ok(Err(e)) => ScriptOutcome::Done(Err(e.to_string())),
                Err(_) => ScriptOutcome::Panicked,
            };
            let _ = tx.send((outcome, work));
        });
        match spawned {
            Ok(()) => {
                let p_s = p.display().to_string();
                self.set_status(tr_args(
                    lang,
                    "Executando script: {name}",
                    &[("name", &p_s)],
                ));
                self.script_job = Some(ScriptJob {
                    path: p,
                    started: std::time::Instant::now(),
                    media_before,
                    rx,
                    ui_rx,
                    active: None,
                });
            }
            Err(e) => {
                let err_s = e.to_string();
                self.set_status(tr_args(
                    lang,
                    "Erro ao iniciar o script: {err}",
                    &[("err", &err_s)],
                ));
            }
        }
    }

    /// Acompanha o script em segundo plano: enquanto roda, mostra o aviso
    /// modal; ao terminar, aplica a sessão do script (ou só o erro).
    fn poll_script_job(&mut self, ctx: &egui::Context) {
        let lang = self.lang();
        let theme = self.theme.clone();
        let Some(job) = &mut self.script_job else {
            return;
        };
        let (outcome, done) = match job.rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                // janela pedida pelo script (camada de Forms própria)
                if job.active.is_none() {
                    if let Ok(call) = job.ui_rx.try_recv() {
                        job.active = crate::script_forms::Active::start(call);
                    }
                }
                if let Some(active) = &mut job.active {
                    if active.show(ctx) {
                        job.active = None;
                    }
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                    return;
                }
                let el = job.started.elapsed().as_secs_f64();
                let path = job.path.display().to_string();
                egui::Modal::new(egui::Id::new("script_em_execucao")).show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        crate::splash::spinner_inline(ui, 20.0, el, &theme);
                        ui.strong(tr(lang, "Executando script..."));
                    });
                    ui.label(path);
                    ui.label(
                        egui::RichText::new(tr(
                            lang,
                            "A edição volta quando o script terminar. Se o script abriu uma \
                             janela, ela pode estar atrás do Neoveg.",
                        ))
                        .small()
                        .weak(),
                    );
                });
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.script_job = None;
                self.set_status(tr(lang, "Erro interno: o script terminou sem resposta."));
                return;
            }
        };
        let Some(job) = self.script_job.take() else {
            return;
        };
        match outcome {
            ScriptOutcome::Done(Ok(msgs)) => {
                self.session = done;
                self.sync_after_session();
                // o que o carregador guardou durante o script entra no próximo quadro
                ctx.request_repaint();
                // mídia importada pelo script (MediaPool.AddMedia) ainda não foi aberta
                let novas: Vec<MediaItem> = self
                    .session
                    .project
                    .media
                    .iter()
                    .filter(|m| !job.media_before.contains(&m.id) && !self.media.contains(m.id))
                    .cloned()
                    .collect();
                for m in novas {
                    self.spawn_media_load(m);
                }
                if let Some(last) = msgs.last() {
                    self.set_status(tr_args(lang, "Script executado: {val}", &[("val", last)]));
                } else {
                    let p_s = job.path.display().to_string();
                    self.set_status(tr_args(lang, "Script executado: {val}", &[("val", &p_s)]));
                }
            }
            // a sessão da interface não foi tocada: nada a desfazer
            ScriptOutcome::Done(Err(e)) => {
                self.set_status(tr_args(lang, "Erro no script: {err}", &[("err", &e)]))
            }
            ScriptOutcome::Panicked => {
                self.set_status(tr(lang, "Erro interno isolado ao executar script .NET/C#."));
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
        if self.script_job.is_some() {
            self.set_status("Aguarde o script terminar para soltar arquivos.");
            return;
        }
        let t = self.cursor;
        let projects: Vec<PathBuf> = files
            .iter()
            .filter(|p| {
                is_project_file(p)
                    || p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                        matches!(
                            e.to_ascii_lowercase().as_str(),
                            "otio" | "edl" | "fcpxml" | "xml" | "aaf" | "srt" | "vtt"
                        )
                    })
            })
            .cloned()
            .collect();
        if let Some(p) = projects.into_iter().next() {
            self.open_project_path(p);
            return;
        }
        self.import_paths(files, Some(t));
    }

    fn update_title(&self, ctx: &egui::Context) {
        let lang = self.lang();
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| tr(lang, "Sem título").into());
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
        for (id, v) in m.tracks.into_iter().chain(m.buses) {
            let h = self.meters_hold.entry(id).or_insert([0.0; 2]);
            for k in 0..2 {
                h[k] = (h[k] * decay).max(v[k]);
            }
        }
        for k in 0..2 {
            self.master_hold[k] = (self.master_hold[k] * decay).max(m.master[k]);
        }
        self.loudness_live = self.audio.loudness_snapshot();
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
                let lang = self.lang();
                let p_s = job.settings.path.display().to_string();
                let secs_s = format!("{secs:.1}");
                match err {
                    Some(e) => {
                        self.set_status(tr_args(lang, "Renderização falhou: {err}", &[("err", &e)]))
                    }
                    None => self.set_status(tr_args(
                        lang,
                        "Renderização concluída em {secs} s: {path}",
                        &[("secs", &secs_s), ("path", &p_s)],
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
        if let Some(rec) = self.recorder.take() {
            let _ = rec.stop();
        }
        self.encoder.save_to_disk();
        recovery::clear_recovery();
        util::log_info("Neoveg encerrado normalmente");
    }
}

impl App {
    /// Um quadro completo da interface (separado de `eframe::App` para testes).
    /// A janela abriu do tamanho da tela de abertura (ver `native_options`):
    /// enquanto a abertura estiver ativa, só ela é desenhada. Vale mesmo com a
    /// abertura inativa (ex.: oferta de recuperação): aí o primeiro quadro já
    /// devolve as bordas e o tamanho normal, e a janela nunca fica presa pequena.
    pub fn set_startup_splash_window(&mut self, on: bool) {
        self.startup_window = on;
    }

    /// Fim da tela de abertura na inicialização: a janela ganha bordas e o
    /// tamanho normal, centralizada no monitor.
    fn leave_startup_window(&mut self, ctx: &egui::Context) {
        self.startup_window = false;
        let [w, h] = crate::MAIN_WINDOW_SIZE;
        let [min_w, min_h] = crate::MAIN_WINDOW_MIN;
        let monitor = ctx
            .input(|i| i.viewport().monitor_size)
            .filter(|m| m.x.is_finite() && m.y.is_finite() && m.x > 0.0 && m.y > 0.0);
        let size = match monitor {
            Some(m) => egui::vec2(w.min(m.x - 40.0).max(min_w), h.min(m.y - 80.0).max(min_h)),
            None => egui::vec2(w, h),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
            min_w, min_h,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        if let Some(m) = monitor {
            let x = ((m.x - size.x) * 0.5).max(0.0);
            let y = ((m.y - size.y) * 0.5).max(0.0);
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    pub fn frame_ui(&mut self, ctx: &egui::Context) {
        let want_delay = if self.prefs.show_tooltips {
            0.5
        } else {
            3600.0
        };
        if (ctx.style().interaction.tooltip_delay - want_delay).abs() > 0.01 {
            ctx.style_mut(|s| s.interaction.tooltip_delay = want_delay);
        }
        let dt = self.last_frame.elapsed().as_secs_f64();
        self.last_frame = Instant::now();
        self.audio.tick_without_device(dt);
        self.poll_loader();
        self.poll_integrations();
        self.poll_script_job(ctx);
        self.poll_render();
        let lang = self.lang();
        if let Some(msg) = self.encoder.tick(ctx, dt, &self.media, lang) {
            self.set_status(msg);
        }
        if self.startup_window {
            // tela de abertura na própria janela: a interface só aparece depois
            self.show_splash_overlay(ctx);
            if self.splash.active {
                return;
            }
            self.leave_startup_window(ctx);
        }
        self.handle_dropped_files(ctx);
        self.handle_shortcuts(ctx);
        self.update_meters(dt as f32);
        if let Some(rec) = &self.recorder {
            let pk = rec.take_peaks();
            let err = rec.last_error();
            let tids = self.recorder_tracks.clone();
            for tid in tids {
                self.recording_peaks.insert(tid, pk);
            }
            if let Some(e) = err {
                self.set_status(e);
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }

        let playing = self.is_playing();
        if playing {
            let t = self.play_pos();
            let continuous = self.prefs.playback_scroll == PlaybackScrollMode::Continuous;
            self.tl.follow_with(t, continuous);
            // fim do projeto: para
            if !self.opts.loop_playback
                && t > self.project.duration() + Ticks::from_millis_i64(500)
                && self.shuttle >= 0.0
            {
                self.stop(true);
            }
            ctx.request_repaint();
        }
        if self.render_job.is_some()
            || self.prerender_job.is_some()
            || self.loudness_job.is_some()
            || self.integrations.probing
            || self.integrations.blender_job.is_some()
            || self.loading > 0
        {
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
        self.show_integrations_window(ctx);
        self.show_encoder_dialogs(ctx);
        self.crash_report_window(ctx);
        self.recovery_window(ctx);
        self.show_welcome_overlay(ctx);
        self.show_splash_overlay(ctx);
        self.autosave();
        let audio_err = crate::util::lock(&self.audio.shared).error.take();
        if let Some(e) = audio_err {
            self.set_status(e);
        }
        self.update_preview(ctx);
        self.sync_snapshot();
        self.update_title(ctx);

        // confirmação ao fechar com alterações não salvas
        let lang = self.lang();
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        if self.confirm_close {
            egui::Window::new(tr(lang, "Alterações não salvas"))
                .id(egui::Id::new("confirm_close_win"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(tr(
                        lang,
                        "O projeto tem alterações não salvas. Deseja salvar antes de sair?",
                    ));
                    ui.horizontal(|ui| {
                        if ui.button(tr(lang, "Salvar")).clicked() && self.save_project(false) {
                            self.allow_close = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui.button(tr(lang, "Descartar")).clicked() {
                            self.allow_close = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui.button(tr(lang, "Cancelar")).clicked() {
                            self.confirm_close = false;
                        }
                    });
                });
        }
    }
}

impl App {
    fn show_splash_overlay(&mut self, ctx: &egui::Context) {
        let was_active = self.splash.active;
        if !self.splash.update_tick(self.loading) {
            if was_active && self.pending_welcome_after_splash && !self.recovery_offer {
                self.pending_welcome_after_splash = false;
                self.welcome_open = true;
            }
            return;
        }
        let lang = self.lang();
        let workers = self.prefs.effective_worker_threads();
        let notice = self
            .preview
            .error
            .as_deref()
            .or(self.audio.error.as_deref());
        let snap = crate::splash::SplashSnapshot {
            lang,
            theme_id: self.theme_id,
            theme: &self.theme,
            layout_type: self.prefs.edit_layout.layout_type,
            audio_device: &self.audio.device_name,
            audio_sr: self.audio.sr,
            workers,
            ram_preview_mb: self.prefs.ram_preview_mb,
            frame_cache_cap: self.prefs.frame_cache_cap,
            loading: self.loading,
            commands_count: COMMANDS.len(),
            fallback_notice: notice,
            own_window: self.startup_window,
        };
        let out = crate::splash::show_splash_screen(
            ctx,
            &mut self.splash,
            &mut self.prefs.show_splash,
            &snap,
        );
        if out.pref_changed {
            if let Err(e) = self.prefs.save() {
                let err_s = e.to_string();
                self.set_status(tr_args(
                    lang,
                    "Aviso ao salvar preferência da tela de abertura: {err}",
                    &[("err", &err_s)],
                ));
            }
        }
        if out.dismissed && self.pending_welcome_after_splash && !self.recovery_offer {
            self.pending_welcome_after_splash = false;
            self.welcome_open = true;
        }
        if self.splash.active {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
    }

    fn show_dock_panel(&mut self, ctx: &egui::Context, at_top: bool) {
        if !self.dock_visible {
            // Um arraste de aba não sobrevive à área de janelas escondida.
            crate::dock_drop::end(ctx);
            return;
        }
        let panel = if at_top {
            egui::TopBottomPanel::top("dock_top")
        } else {
            egui::TopBottomPanel::bottom("dock_bot")
        };
        let screen_h = ctx.screen_rect().height();
        let max_h = (screen_h * 0.56).max(160.0);
        let eff_h = self.dock_height.min(max_h);
        let resp = panel
            .resizable(true)
            .default_height(eff_h)
            .min_height(150.0)
            .max_height(max_h)
            .show(ctx, |ui| self.dock_area(ui));
        let h = resp.response.rect.height();
        if resp.response.dragged() && h.is_finite() && h >= 150.0 {
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
        let prev_pv_left = self.prefs.edit_layout.preview_on_left;
        let lt = next.layout_type;
        self.prefs.edit_layout = next;
        if apply_metrics {
            self.dock_height = lt.default_dock_height();
            self.preview_ratio = lt.default_preview_ratio();
        }
        if apply_metrics || prev_pv_left != self.prefs.edit_layout.preview_on_left {
            self.dock_tree = crate::dock::build_dock_tree(
                self.preview_ratio,
                self.dock_tab,
                self.prefs.edit_layout.preview_on_left,
            );
        }
        self.master_bus_visible = self.prefs.edit_layout.master_bus_docked;
        if !self.master_bus_visible {
            self.master_bus_detached = false;
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
                let err_s = e.to_string();
                self.set_status(tr_args(
                    self.lang(),
                    "Aviso ao salvar layout: {err}",
                    &[("err", &err_s)],
                ));
            }
        }
    }

    pub fn select_layout_type(
        &mut self,
        ctx: Option<&egui::Context>,
        lt: LayoutType,
        save_prefs: bool,
    ) {
        let lang = self.lang();
        let mut next = self.prefs.edit_layout.clone();
        next.apply_type(lt);
        self.apply_edit_layout(ctx, next, true, save_prefs);
        self.set_status(tr_args(
            lang,
            "Layout {name} aplicado ({era})",
            &[("name", &lt.label_in(lang)), ("era", &lt.era_in(lang))],
        ));
    }

    pub fn set_theme(&mut self, ctx: &egui::Context, id: ThemeId, save_prefs: bool) {
        self.theme_id = id;
        self.theme = theme::apply_theme(ctx, id);
        self.prefs.theme = id.id().to_string();
        if save_prefs {
            if let Err(e) = self.prefs.save() {
                let err_s = e.to_string();
                self.set_status(tr_args(
                    self.lang(),
                    "Aviso ao salvar tema: {err}",
                    &[("err", &err_s)],
                ));
            }
        }
    }

    pub fn is_panel_open(&self, tab: DockTab) -> bool {
        (self.dock_visible
            && self.dock_tab == tab
            && !self.detached_tabs.contains(&tab)
            && !self.closed_tabs.contains(&tab))
            || self.detached_tabs.contains(&tab)
    }

    pub fn current_window_layout(&self, name: &str) -> WindowLayout {
        let mut detached: Vec<String> = self
            .detached_tabs
            .iter()
            .map(|t| layout::panel_key(*t).to_string())
            .collect();
        detached.sort();
        let mut closed: Vec<String> = self
            .closed_tabs
            .iter()
            .map(|t| layout::panel_key(*t).to_string())
            .collect();
        closed.sort();
        let mut l = WindowLayout {
            name: name.to_string(),
            dock_height: self.dock_height,
            preview_ratio: self.preview_ratio,
            dock_visible: self.dock_visible,
            active_tab: layout::panel_key(self.dock_tab).to_string(),
            detached_tabs: detached,
            preview_detached: self.preview_detached,
            preview_visible: self.preview_visible,
            master_bus_visible: self.master_bus_visible,
            master_bus_detached: self.master_bus_detached,
            px_per_sec: Some(self.tl.px_per_sec),
            edit_layout: Some(self.prefs.edit_layout.clone()),
            tile_tree: Some(self.dock_tree.clone()),
            closed_tabs: Some(closed),
        };
        l.sanitize();
        l
    }

    pub fn apply_window_layout(&mut self, mut l: WindowLayout) {
        l.sanitize();
        self.dock_maximized = None;
        self.timeline_restore = None;
        self.dock_height = l.dock_height;
        self.preview_ratio = l.preview_ratio;
        self.dock_visible = l.dock_visible;
        self.dock_tab = l.active_panel();
        self.detached_tabs = l.detached_panels();
        self.closed_tabs = l.closed_panels();
        self.preview_detached = l.preview_detached;
        self.preview_visible = l.preview_visible;
        self.master_bus_visible = l.master_bus_visible;
        self.master_bus_detached = l.master_bus_detached;
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
        self.dock_tree = l.tile_tree.unwrap_or_else(|| {
            crate::dock::build_dock_tree(
                self.preview_ratio,
                self.dock_tab,
                self.prefs.edit_layout.preview_on_left,
            )
        });
        let lang = self.lang();
        self.pending_dock_focus = Some(self.dock_tab);
        let lname = layout::localized_preset_name(&l.name, lang);
        self.set_status(tr_args(
            lang,
            "Layout aplicado: {name}",
            &[("name", &lname)],
        ));
    }

    pub fn save_named_layout(&mut self, name: &str) {
        let lang = self.lang();
        let layout = self.current_window_layout(name);
        let fname = format!("{}.neoveglayout", layout::sanitize_layout_filename(name));
        let path = Preferences::layouts_dir().join(fname);
        match layout::save_layout_file(&layout, &path) {
            Ok(()) => {
                self.prefs.window_layout = Some(path.to_string_lossy().to_string());
                let _ = self.prefs.save();
                self.saved_layouts = layout::list_saved_layouts();
                let p_s = path.display().to_string();
                self.set_status(tr_args(lang, "Layout salvo em {path}", &[("path", &p_s)]));
            }
            Err(e) => {
                let err_s = e.to_string();
                self.set_status(tr_args(
                    lang,
                    "Erro ao salvar layout: {err}",
                    &[("err", &err_s)],
                ));
            }
        }
    }

    pub fn build_proxy_for_media(&mut self, media_id: Id) {
        let lang = self.lang();
        let Some(item) = self.project.media(media_id).cloned() else {
            self.set_status(tr(lang, "Mídia não encontrada no projeto."));
            return;
        };
        if !item.has_video {
            self.set_status(tr(lang, "Proxies aplicam-se apenas a clipes de vídeo."));
            return;
        }
        if !self.media.contains(media_id) {
            self.set_status(tr(lang, "Mídia offline: não foi possível gerar proxy."));
            return;
        }
        let dir = neoveg_common::util::base_dir().join("proxies");
        let out = dir.join(format!("proxy_{media_id}.avi"));
        let media = self.media.clone();
        let tx = self.loader_tx.clone();
        self.loading += 1;
        self.set_status(tr_args(
            lang,
            "Gerando proxy de '{name}'...",
            &[("name", &item.name)],
        ));
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
        let lang = self.lang();
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
            self.set_status(tr(
                lang,
                "Selecione pelo menos um evento de vídeo para gerar proxy.",
            ));
            return;
        }
        for mid in media_ids {
            self.build_proxy_for_media(mid);
        }
    }

    pub fn prerender_region(&mut self) {
        let lang = self.lang();
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
            self.set_status(tr(lang, "Região vazia para pré-renderizar."));
            return;
        }
        self.sync_snapshot();
        let proj = Arc::new((*self.session.snapshot).clone());
        let media = self.media.clone();
        let scale = self.opts.preview_quality.scale();
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
            ..RenderSettings::default()
        };
        let tx = self.loader_tx.clone();
        let prog = Arc::new(RenderProgress::default());
        let prog_bg = prog.clone();
        self.loading += 1;
        self.prerender_job = Some((start, end, prog));
        self.set_status(tr(lang, "Pré-renderizando região selecionada..."));
        if let Err(e) = util::spawn("prerender-regiao", move || {
            let result = crate::render::run(&proj, &media, &settings, &prog_bg)
                .map(|()| out)
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(LoadMsg::PrerenderDone { start, end, result });
        }) {
            self.loading = self.loading.saturating_sub(1);
            self.prerender_job = None;
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
        ShortcutKey::F11 => Key::F11,
        ShortcutKey::Backtick => Key::Backtick,
        ShortcutKey::ArrowLeft => Key::ArrowLeft,
        ShortcutKey::ArrowRight => Key::ArrowRight,
        ShortcutKey::ArrowUp => Key::ArrowUp,
        ShortcutKey::ArrowDown => Key::ArrowDown,
        ShortcutKey::Home => Key::Home,
        ShortcutKey::End => Key::End,
        ShortcutKey::OpenBracket => Key::OpenBracket,
        ShortcutKey::CloseBracket => Key::CloseBracket,
        ShortcutKey::Backslash => Key::Backslash,
        ShortcutKey::Equals => Key::Equals,
        ShortcutKey::Minus => Key::Minus,
    }
}
