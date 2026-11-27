//! Tabela Única de Comandos (AGENTS.md §3.3).
//!
//! Toda ação editável, de menu, de barra de ferramentas, de atalho de teclado,
//! de script `.NET` ou de CLI é registrada aqui com `id`, rótulo localizado,
//! caminho de menu, atalho padrão (regravável), `enabled()` e `run()`.

use crate::media::MediaCache;
use crate::model::{EnvKind, GenSpec, Id, MarkerKind, TextSpec, TrackKind};
use crate::session::EditorSession;
use crate::time::Ticks;
use neoveg_common::{tr, tr_args, tr_menu_path, Language, LayoutType};
use std::collections::HashMap;

/// Tecla portátil (independente de biblioteca de UI).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ShortcutKey {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    Space,
    Enter,
    Escape,
    Delete,
    F2,
    F8,
    F11,
    /// Crase (`` ` ``): no ABNT2 é tecla morta e quase nunca chega à UI.
    Backtick,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    OpenBracket,
    CloseBracket,
    Backslash,
    Equals,
    Minus,
    Backspace,
    PageUp,
    PageDown,
    F4,
    F5,
    F12,
}

impl ShortcutKey {
    pub fn label_in(self, lang: Language) -> &'static str {
        let raw = self.canonical_label();
        match self {
            ShortcutKey::Space
            | ShortcutKey::ArrowLeft
            | ShortcutKey::ArrowRight
            | ShortcutKey::ArrowUp
            | ShortcutKey::ArrowDown => tr(lang, raw),
            _ => raw,
        }
    }

    pub fn label(self) -> &'static str {
        self.label_in(Language::PtBr)
    }

    /// Token neutro para salvar atalhos em `preferencias.json` independente do idioma ativo.
    pub const fn canonical_token(self) -> &'static str {
        match self {
            ShortcutKey::Space => "Space",
            ShortcutKey::ArrowLeft => "Left",
            ShortcutKey::ArrowRight => "Right",
            ShortcutKey::ArrowUp => "Up",
            ShortcutKey::ArrowDown => "Down",
            _ => self.canonical_label(),
        }
    }

    const fn canonical_label(self) -> &'static str {
        match self {
            ShortcutKey::A => "A",
            ShortcutKey::B => "B",
            ShortcutKey::C => "C",
            ShortcutKey::D => "D",
            ShortcutKey::E => "E",
            ShortcutKey::F => "F",
            ShortcutKey::G => "G",
            ShortcutKey::H => "H",
            ShortcutKey::I => "I",
            ShortcutKey::J => "J",
            ShortcutKey::K => "K",
            ShortcutKey::L => "L",
            ShortcutKey::M => "M",
            ShortcutKey::N => "N",
            ShortcutKey::O => "O",
            ShortcutKey::P => "P",
            ShortcutKey::Q => "Q",
            ShortcutKey::R => "R",
            ShortcutKey::S => "S",
            ShortcutKey::T => "T",
            ShortcutKey::U => "U",
            ShortcutKey::V => "V",
            ShortcutKey::W => "W",
            ShortcutKey::X => "X",
            ShortcutKey::Y => "Y",
            ShortcutKey::Z => "Z",
            ShortcutKey::Num0 => "0",
            ShortcutKey::Num1 => "1",
            ShortcutKey::Num2 => "2",
            ShortcutKey::Num3 => "3",
            ShortcutKey::Num4 => "4",
            ShortcutKey::Num5 => "5",
            ShortcutKey::Num6 => "6",
            ShortcutKey::Num7 => "7",
            ShortcutKey::Num8 => "8",
            ShortcutKey::Num9 => "9",
            ShortcutKey::Space => "Espaço",
            ShortcutKey::Enter => "Enter",
            ShortcutKey::Escape => "Esc",
            ShortcutKey::Delete => "Delete",
            ShortcutKey::F2 => "F2",
            ShortcutKey::F8 => "F8",
            ShortcutKey::F11 => "F11",
            ShortcutKey::Backtick => "`",
            ShortcutKey::ArrowLeft => "Esquerda",
            ShortcutKey::ArrowRight => "Direita",
            ShortcutKey::ArrowUp => "Cima",
            ShortcutKey::ArrowDown => "Baixo",
            ShortcutKey::Home => "Home",
            ShortcutKey::End => "End",
            ShortcutKey::OpenBracket => "[",
            ShortcutKey::CloseBracket => "]",
            ShortcutKey::Backslash => "\\",
            ShortcutKey::Equals => "=",
            ShortcutKey::Minus => "-",
            ShortcutKey::Backspace => "Backspace",
            ShortcutKey::PageUp => "PageUp",
            ShortcutKey::PageDown => "PageDown",
            ShortcutKey::F4 => "F4",
            ShortcutKey::F5 => "F5",
            ShortcutKey::F12 => "F12",
        }
    }

    pub fn parse(token: &str) -> Option<Self> {
        let t = token.trim().to_ascii_lowercase();
        Some(match t.as_str() {
            "a" => ShortcutKey::A,
            "b" => ShortcutKey::B,
            "c" => ShortcutKey::C,
            "d" => ShortcutKey::D,
            "e" => ShortcutKey::E,
            "f" => ShortcutKey::F,
            "g" => ShortcutKey::G,
            "h" => ShortcutKey::H,
            "i" => ShortcutKey::I,
            "j" => ShortcutKey::J,
            "k" => ShortcutKey::K,
            "l" => ShortcutKey::L,
            "m" => ShortcutKey::M,
            "n" => ShortcutKey::N,
            "o" => ShortcutKey::O,
            "p" => ShortcutKey::P,
            "q" => ShortcutKey::Q,
            "r" => ShortcutKey::R,
            "s" => ShortcutKey::S,
            "t" => ShortcutKey::T,
            "u" => ShortcutKey::U,
            "v" => ShortcutKey::V,
            "w" => ShortcutKey::W,
            "x" => ShortcutKey::X,
            "y" => ShortcutKey::Y,
            "z" => ShortcutKey::Z,
            "0" => ShortcutKey::Num0,
            "1" => ShortcutKey::Num1,
            "2" => ShortcutKey::Num2,
            "3" => ShortcutKey::Num3,
            "4" => ShortcutKey::Num4,
            "5" => ShortcutKey::Num5,
            "6" => ShortcutKey::Num6,
            "7" => ShortcutKey::Num7,
            "8" => ShortcutKey::Num8,
            "9" => ShortcutKey::Num9,
            "space" | "espaco" | "espaço" => ShortcutKey::Space,
            "enter" | "return" => ShortcutKey::Enter,
            "esc" | "escape" => ShortcutKey::Escape,
            "del" | "delete" => ShortcutKey::Delete,
            "f2" => ShortcutKey::F2,
            "f8" => ShortcutKey::F8,
            "f11" => ShortcutKey::F11,
            "`" | "backtick" | "crase" => ShortcutKey::Backtick,
            "left" | "esquerda" => ShortcutKey::ArrowLeft,
            "right" | "direita" => ShortcutKey::ArrowRight,
            "up" | "cima" => ShortcutKey::ArrowUp,
            "down" | "baixo" => ShortcutKey::ArrowDown,
            "home" => ShortcutKey::Home,
            "end" => ShortcutKey::End,
            "[" | "openbracket" => ShortcutKey::OpenBracket,
            "]" | "closebracket" => ShortcutKey::CloseBracket,
            "\\" | "backslash" => ShortcutKey::Backslash,
            "=" | "+" | "equal" | "equals" | "plus" => ShortcutKey::Equals,
            "-" | "minus" => ShortcutKey::Minus,
            "backspace" | "bksp" => ShortcutKey::Backspace,
            "pageup" | "pgup" => ShortcutKey::PageUp,
            "pagedown" | "pgdn" | "pgdown" => ShortcutKey::PageDown,
            "f4" => ShortcutKey::F4,
            "f5" => ShortcutKey::F5,
            "f12" => ShortcutKey::F12,
            _ => return None,
        })
    }
}

/// Combinação de atalho de teclado independente da UI.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Shortcut {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: ShortcutKey,
}

impl Shortcut {
    pub const fn new(ctrl: bool, shift: bool, alt: bool, key: ShortcutKey) -> Self {
        Self {
            ctrl,
            shift,
            alt,
            key,
        }
    }

    pub const fn plain(key: ShortcutKey) -> Self {
        Self::new(false, false, false, key)
    }

    pub const fn ctrl(key: ShortcutKey) -> Self {
        Self::new(true, false, false, key)
    }

    pub const fn ctrl_shift(key: ShortcutKey) -> Self {
        Self::new(true, true, false, key)
    }

    pub const fn ctrl_alt(key: ShortcutKey) -> Self {
        Self::new(true, false, true, key)
    }

    pub const fn shift(key: ShortcutKey) -> Self {
        Self::new(false, true, false, key)
    }

    pub const fn alt(key: ShortcutKey) -> Self {
        Self::new(false, false, true, key)
    }

    pub const fn alt_shift(key: ShortcutKey) -> Self {
        Self::new(false, true, true, key)
    }

    pub fn format_in(self, lang: Language) -> String {
        let mut parts = Vec::with_capacity(4);
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.alt {
            parts.push("Alt");
        }
        parts.push(self.key.label_in(lang));
        parts.join("+")
    }

    pub fn format(self) -> String {
        self.format_in(Language::PtBr)
    }

    /// Representação independente de idioma para gravar em configurações.
    pub fn format_canonical(self) -> String {
        let mut parts = Vec::with_capacity(4);
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.alt {
            parts.push("Alt");
        }
        parts.push(self.key.canonical_token());
        parts.join("+")
    }

    /// Faz parse de uma string como `"Ctrl+Shift+S"` sem nunca entrar em pânico (§0.4).
    pub fn parse(spec: &str) -> Option<Self> {
        if spec.trim().is_empty() || spec.len() > 64 {
            return None;
        }
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut key = None;
        for part in spec.split('+') {
            let p = part.trim();
            if p.eq_ignore_ascii_case("ctrl") || p.eq_ignore_ascii_case("cmd") {
                ctrl = true;
            } else if p.eq_ignore_ascii_case("shift") {
                shift = true;
            } else if p.eq_ignore_ascii_case("alt") {
                alt = true;
            } else {
                key = ShortcutKey::parse(p);
            }
        }
        key.map(|k| Self {
            ctrl,
            shift,
            alt,
            key: k,
        })
    }
}

/// Abas acopláveis da área inferior (espelhadas na UI).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub enum DockPanel {
    ProjectMedia,
    Explorer,
    Trimmer,
    EditDetails,
    Transitions,
    VideoFx,
    ColorGrading,
    AudioFx,
    Generators,
    Mixer,
    Loudness,
    SurroundPanner,
    PluginManager,
    Compositors,
    Scopes,
    Notes,
    Encoder,
    Capture,
    Transcript,
    Store,
}

impl DockPanel {
    pub const ALL: [DockPanel; 20] = [
        DockPanel::ProjectMedia,
        DockPanel::Explorer,
        DockPanel::Trimmer,
        DockPanel::EditDetails,
        DockPanel::Transitions,
        DockPanel::VideoFx,
        DockPanel::ColorGrading,
        DockPanel::AudioFx,
        DockPanel::Generators,
        DockPanel::Mixer,
        DockPanel::Loudness,
        DockPanel::SurroundPanner,
        DockPanel::PluginManager,
        DockPanel::Compositors,
        DockPanel::Scopes,
        DockPanel::Notes,
        DockPanel::Encoder,
        DockPanel::Capture,
        DockPanel::Transcript,
        DockPanel::Store,
    ];

    pub fn name_in(self, lang: Language) -> &'static str {
        let pt = match self {
            DockPanel::ProjectMedia => "Mídia do Projeto",
            DockPanel::Explorer => "Explorer",
            DockPanel::Trimmer => "Trimmer",
            DockPanel::EditDetails => "Detalhes da Edição",
            DockPanel::Transitions => "Transições",
            DockPanel::VideoFx => "Efeitos de Vídeo",
            DockPanel::ColorGrading => "Color Grading",
            DockPanel::AudioFx => "Efeitos de Áudio",
            DockPanel::Generators => "Geradores de Mídia",
            DockPanel::Mixer => "Mixer",
            DockPanel::Loudness => "Medidores de Loudness",
            DockPanel::SurroundPanner => "Surround Panner",
            DockPanel::PluginManager => "Plug-In Manager",
            DockPanel::Compositors => "Compositores",
            DockPanel::Scopes => "Escopos",
            DockPanel::Notes => "Notas",
            DockPanel::Encoder => "Neoveg Encoder",
            DockPanel::Capture => "Neoveg Capture",
            DockPanel::Transcript => "Transcrição de fala",
            DockPanel::Store => "Loja de scripts",
        };
        tr(lang, pt)
    }

    pub fn name(self) -> &'static str {
        self.name_in(Language::PtBr)
    }

    pub fn command_id(self) -> &'static str {
        match self {
            DockPanel::ProjectMedia => "view.tab.project_media",
            DockPanel::Explorer => "view.tab.explorer",
            DockPanel::Trimmer => "view.tab.trimmer",
            DockPanel::EditDetails => "view.tab.edit_details",
            DockPanel::Transitions => "view.tab.transitions",
            DockPanel::VideoFx => "view.tab.video_fx",
            DockPanel::ColorGrading => "view.tab.color_grading",
            DockPanel::AudioFx => "view.tab.audio_fx",
            DockPanel::Generators => "view.tab.generators",
            DockPanel::Mixer => "view.tab.mixer",
            DockPanel::Loudness => "view.tab.loudness",
            DockPanel::SurroundPanner => "view.tab.surround_panner",
            DockPanel::PluginManager => "view.tab.plugin_manager",
            DockPanel::Compositors => "view.tab.compositors",
            DockPanel::Scopes => "view.tab.scopes",
            DockPanel::Notes => "view.tab.notes",
            DockPanel::Encoder => "view.tab.encoder",
            DockPanel::Capture => "view.tab.capture",
            DockPanel::Transcript => "view.tab.transcript",
            DockPanel::Store => "view.tab.store",
        }
    }
}

/// Ações de interface/transporte delegadas pela tabela de comandos ao host ativo.
#[derive(Clone, Debug, PartialEq)]
pub enum UiAction {
    NewProject,
    OpenProjectDialog,
    SaveProject {
        save_as: bool,
    },
    SaveVersion,
    ImportMediaDialog,
    ImportSequenceDialog,
    ConvertMediaDialog,
    ImportOtioDialog,
    ExportOtioDialog,
    ImportSubtitlesDialog,
    ExportSrtDialog,
    ExportVttDialog,
    ImportEdlDialog,
    ExportEdlDialog,
    ImportFcpxmlDialog,
    ExportFcpxmlDialog,
    ImportFcp7Dialog,
    ExportFcp7Dialog,
    ImportAafDialog,
    ExportAafDialog,
    ImportNvegseqDialog,
    ExportNvegseqDialog,
    OpenRenderDialog,
    SendToEncoder,
    EncoderStartQueue,
    EncoderPauseQueue,
    EncoderStopCurrent,
    EncoderStopQueue,
    OpenCapture,
    ImportCaptureProjectDialog,
    CaptureToggleRecord,
    CaptureExportToProject,
    SnapshotDialog,
    OpenWorkFolders,
    /// Arquivo → Localizar mídia ausente...
    FindMissingMedia,
    OpenAdvancedSaveDialog,
    OpenArchiveProjectDialog,
    OpenSlideshowDialog,
    OpenParentTimeline,
    AddProjectNoteAtCursor,
    AutoNormalizeSelected,
    OpenAutoDuckingDialog,
    OpenBeatDetectionDialog,
    OpenProjectProps,
    OpenEventProps,
    OpenPasteAttributesDialog,
    OpenTakeChooser,
    OpenSceneDetectDialog,
    OpenPanCrop,
    OpenFxEvent,
    OpenFxMasterAudio,
    OpenFxMasterVideo,
    OpenGoto,
    /// Abre em aba o projeto `.neoveg` aninhado do evento ou da mídia selecionada.
    OpenNestedProjectTab,
    OpenEditLayoutProps,
    SetLayoutType(LayoutType),
    OpenShortcuts,
    OpenPrefs,
    OpenSplash,
    OpenWelcome,
    OpenAbout,
    BuildProxySelected,
    PrerenderRegion,
    RunScriptDialog,
    OpenIntegrationsDialog,
    SendToExternalApp(crate::integrations::ExternalApp),
    BlenderRenderSceneDialog,
    BlenderExportScriptDialog,
    BlenderImportCameraDialog,
    ExitApp,
    SelectDock(DockPanel),
    TogglePreview,
    ToggleMasterBus,
    /// F11: minimiza/restaura a área de janelas acopláveis (manual do VEGAS Pro 21).
    ToggleDockArea,
    /// Shift+F11: minimiza/restaura a lista de trilhas.
    ToggleTrackList,
    /// Ctrl+F11: maximiza/restaura a timeline (esconde a área de janelas e a lista de trilhas).
    ToggleMaximizeTimeline,
    TogglePlay,
    ToggleRecord,
    PlayFromStart,
    Play,
    Stop {
        restore_cursor: bool,
    },
    Shuttle(f64),
    SetCursor(Ticks),
    CenterTimeline,
    ZoomBy(f32),
    JumpMarker(bool),
    JumpMarkerIndex(usize),
    ProjectMediaSetView(u8),
    ProjectMediaNewBin,
    ProjectMediaNewSmartBin,
    ProjectMediaNewStoryboard,
    ProjectMediaReplaceDialog,
    ProjectMediaInsertStoryboard,
    PreviewToggleSplit,
    PreviewSplitMode(crate::playback::PreviewSplitMode),
    PreviewCopyFrame,
    PreviewRamBuild,
    PreviewRamClear,
    PreviewZoom(f32),
    /// Zoom da prévia em passos de 5% (`n` passos; negativo diminui).
    PreviewZoomStep(i32),
    /// Alterna entre o zoom atual e o zoom de ajuste (*Bypass Zoom*).
    PreviewZoomBypass,
    /// Escala da interface: `Some(n)` muda `n` passos, `None` volta a 100%.
    UiScale(Option<i32>),
    PreviewToggleExternalMonitor,
    InsertNestedTimelineDialog,
    RenderToNewTrack,
    SetTrackDefaultProps,
    CycleStereo3dMode,
    OpenMotionTrackDialog,
    ApplyAudioFxNonRealtime,
    OpenCustomizeToolbar,
    OpenCommandMarkerDialog,
    SetRulerGrid(crate::session::RulerGridSpacing),
    /// PageUp/PageDown: move o cursor para a marca de grade anterior (`-1`)
    /// ou seguinte (`1`), na grade visível da régua.
    MoveByGrid(i64),
    /// Ctrl+Shift+F12: atenua a saída do Master em 20 dB (*Dim*).
    ToggleMasterDim,
    /// Shift+F5: relê do disco o áudio das mídias para refazer as formas de onda.
    RebuildAudioPeaks,
    /// Ctrl+F4: fecha o projeto da aba ativa.
    CloseProject,
    /// F2: renomeia o evento selecionado na timeline.
    RenameSelectedEvent,
    /// Ctrl+B: diálogo *Colar repetindo*.
    OpenPasteRepeatDialog,
}

/// Resultado de execução de um comando.
#[derive(Clone, Debug, PartialEq)]
pub enum CommandOutcome {
    Executed,
    UiDispatched(UiAction),
}

/// Fronteira entre o motor de comandos (`neoveg_engine`) e o ambiente (GUI ou headless).
pub trait CommandHost {
    fn supports_ui(&self) -> bool;
    fn is_playing(&self) -> bool {
        false
    }
    fn shuttle_speed(&self) -> f64 {
        0.0
    }
    fn media_cache(&self) -> Option<&MediaCache> {
        None
    }
    fn dispatch_ui(&mut self, action: UiAction) -> Result<(), String>;
    fn insert_generated(
        &mut self,
        session: &mut EditorSession,
        spec: GenSpec,
    ) -> Result<(), String>;
    fn on_media_removed(&mut self, _ids: &[Id]) {}
    fn after_mutation(&mut self, _session: &mut EditorSession) {}
}

/// Host headless para execução por CLI e `neoveg_scripting` (§3.2 / §3.3).
#[derive(Default)]
pub struct HeadlessHost;

impl CommandHost for HeadlessHost {
    fn supports_ui(&self) -> bool {
        false
    }

    fn dispatch_ui(&mut self, action: UiAction) -> Result<(), String> {
        let act = format!("{action:?}");
        Err(tr_args(
            neoveg_common::active_language(),
            "ação '{action}' requer interface gráfica ativa e não pode rodar em modo headless",
            &[("action", &act)],
        ))
    }

    fn insert_generated(
        &mut self,
        session: &mut EditorSession,
        spec: GenSpec,
    ) -> Result<(), String> {
        session.checkpoint();
        let ti = session
            .sel_track
            .filter(|&i| {
                session
                    .project
                    .tracks
                    .get(i)
                    .map(|t| t.kind == TrackKind::Video)
                    .unwrap_or(false)
            })
            .unwrap_or_else(|| {
                session
                    .project
                    .tracks
                    .iter()
                    .position(|t| t.kind == TrackKind::Video)
                    .unwrap_or_else(|| session.insert_track(TrackKind::Video))
            });
        let imp = crate::media::import_generated(spec);
        let dur = crate::time::Ticks::from_secs_i64(5);
        let ev = crate::model::Event::new(&imp.item, session.cursor, dur);
        let id = ev.id;
        session.project.media.push(imp.item);
        if let Some(track) = session.project.tracks.get_mut(ti) {
            track.events.push(ev);
        }
        session.sel_events.clear();
        session.sel_events.insert(id);
        session.changed();
        Ok(())
    }
}

/// Definição estática de um comando registrado na Tabela Única (§3.3).
pub struct CommandDef {
    pub id: &'static str,
    pub label: &'static str,
    pub menu_path: &'static str,
    pub default_shortcut: Option<Shortcut>,
    pub needs_ui: bool,
    pub enabled: fn(&EditorSession, &dyn CommandHost) -> bool,
    pub run: fn(&mut EditorSession, &mut dyn CommandHost) -> Result<CommandOutcome, String>,
}

impl CommandDef {
    pub fn label_in(&self, lang: Language) -> &'static str {
        tr(lang, self.label)
    }

    pub fn menu_path_in(&self, lang: Language) -> String {
        tr_menu_path(lang, self.menu_path)
    }
}

fn always_enabled(_s: &EditorSession, _h: &dyn CommandHost) -> bool {
    true
}

fn ui_enabled(_s: &EditorSession, h: &dyn CommandHost) -> bool {
    h.supports_ui()
}

fn has_selected_events(s: &EditorSession, _h: &dyn CommandHost) -> bool {
    !s.sel_events.is_empty()
}

fn has_selected_events_ui(s: &EditorSession, h: &dyn CommandHost) -> bool {
    h.supports_ui() && !s.sel_events.is_empty()
}

fn has_selected_audio_events(s: &EditorSession, _h: &dyn CommandHost) -> bool {
    s.sel_events.iter().any(|&id| {
        s.project.find_event(id).is_some_and(|(ti, ei)| {
            let tr = &s.project.tracks[ti];
            tr.kind == TrackKind::Audio && !tr.locked && !tr.events[ei].locked
        })
    })
}

fn has_selected_video_events_ui(s: &EditorSession, h: &dyn CommandHost) -> bool {
    h.supports_ui()
        && s.sel_events.iter().any(|&id| {
            s.project.find_event(id).is_some_and(|(ti, ei)| {
                let tr = &s.project.tracks[ti];
                tr.kind == TrackKind::Video
                    && !tr.locked
                    && !tr.events[ei].locked
                    && s.project
                        .media(tr.events[ei].media_id)
                        .is_some_and(|m| m.has_video)
            })
        })
}

fn has_selected_proxyable_video_events_ui(s: &EditorSession, h: &dyn CommandHost) -> bool {
    h.supports_ui()
        && s.sel_events.iter().any(|&id| {
            s.project.find_event(id).is_some_and(|(ti, ei)| {
                let Some(tr) = s.project.tracks.get(ti) else {
                    return false;
                };
                let Some(ev) = tr.events.get(ei) else {
                    return false;
                };
                tr.kind == TrackKind::Video
                    && s.project
                        .media(ev.media_id)
                        .is_some_and(|m| m.can_build_proxy())
            })
        })
}

fn has_selected_audio_track(s: &EditorSession, _h: &dyn CommandHost) -> bool {
    s.sel_track.is_some_and(|ti| {
        s.project
            .tracks
            .get(ti)
            .is_some_and(|t| t.kind == TrackKind::Audio && !t.locked)
    })
}

fn has_active_media(s: &EditorSession, _h: &dyn CommandHost) -> bool {
    s.active_media_id
        .is_some_and(|id| s.project.media(id).is_some())
}

fn has_active_media_ui(s: &EditorSession, h: &dyn CommandHost) -> bool {
    h.supports_ui() && has_active_media(s, h)
}

fn can_create_sync_link(s: &EditorSession, _h: &dyn CommandHost) -> bool {
    s.sel_events
        .iter()
        .filter(|&&id| {
            s.project.find_event(id).is_some_and(|(ti, ei)| {
                !s.project.tracks[ti].locked && !s.project.tracks[ti].events[ei].locked
            })
        })
        .take(2)
        .count()
        >= 2
}

fn has_sync_link_in_selection(s: &EditorSession, _h: &dyn CommandHost) -> bool {
    s.sel_events.iter().any(|&id| {
        s.project
            .event(id)
            .is_some_and(|e| e.sync_link_master.is_some())
            || s.has_sync_followers(id)
    })
}

fn has_clipboard(s: &EditorSession, _h: &dyn CommandHost) -> bool {
    !s.clipboard.is_empty()
}

fn has_time_selection(s: &EditorSession, _h: &dyn CommandHost) -> bool {
    matches!(s.time_sel, Some((a, b)) if b > a)
}

fn has_selected_track(s: &EditorSession, _h: &dyn CommandHost) -> bool {
    s.sel_track
        .map(|i| i < s.project.tracks.len())
        .unwrap_or(false)
}

fn ui_action(action: UiAction, host: &mut dyn CommandHost) -> Result<CommandOutcome, String> {
    host.dispatch_ui(action.clone())?;
    Ok(CommandOutcome::UiDispatched(action))
}

fn export_or_ui(
    s: &EditorSession,
    h: &mut dyn CommandHost,
    action: UiAction,
    ext: &str,
    label: &str,
    writer: impl FnOnce(
        &EditorSession,
        &std::path::Path,
    ) -> Result<(), neoveg_common::error::NeovegError>,
) -> Result<CommandOutcome, String> {
    if h.supports_ui() {
        ui_action(action, h)
    } else if let Some(p) = s.path.as_ref() {
        let out_path = p.with_extension(ext);
        writer(s, &out_path).map_err(|e| format!("{e}"))?;
        Ok(CommandOutcome::Executed)
    } else {
        Err(tr_args(
            s.lang,
            "projeto ainda não tem caminho definido para exportar {label} sem diálogo",
            &[("label", &label)],
        ))
    }
}

fn ok_mutated(
    session: &mut EditorSession,
    host: &mut dyn CommandHost,
) -> Result<CommandOutcome, String> {
    host.after_mutation(session);
    Ok(CommandOutcome::Executed)
}

/// Leva o cursor a `t`: pela interface quando há uma (a prévia acompanha),
/// direto na sessão no modo headless.
fn move_cursor(
    s: &mut EditorSession,
    h: &mut dyn CommandHost,
    t: Ticks,
) -> Result<CommandOutcome, String> {
    if h.supports_ui() {
        ui_action(UiAction::SetCursor(t), h)
    } else {
        s.cursor = t;
        ok_mutated(s, h)
    }
}

/// PageUp/PageDown: a interface usa a grade visível da régua; sem
/// interface, o espaçamento escolhido (1 segundo na grade automática).
fn move_by_grid(
    s: &mut EditorSession,
    h: &mut dyn CommandHost,
    dir: i64,
) -> Result<CommandOutcome, String> {
    if h.supports_ui() {
        return ui_action(UiAction::MoveByGrid(dir), h);
    }
    let step = s
        .opts
        .ruler_grid
        .step_ticks(&s.project.settings)
        .unwrap_or_else(|| Ticks::from_secs_i64(1));
    s.cursor = crate::session::grid_neighbor(s.cursor, step, dir);
    ok_mutated(s, h)
}

/// Ctrl+[ / Ctrl+]: no modo de edição expandido desloca o corte um quadro;
/// fora dele leva o cursor à borda do evento selecionado anterior/seguinte
/// (lista de atalhos do VEGAS Pro).
fn selected_edge_or_nudge(
    s: &mut EditorSession,
    h: &mut dyn CommandHost,
    forward: bool,
) -> Result<CommandOutcome, String> {
    if s.expanded_edit.is_some() {
        let fd = s.project.settings.frame_dur();
        s.nudge_expanded_edit(if forward { fd } else { -fd });
        return ok_mutated(s, h);
    }
    match s.selected_edge_time(forward) {
        Some(t) => move_cursor(s, h, t),
        None => Ok(CommandOutcome::Executed),
    }
}

fn post_ripple(
    s: &mut EditorSession,
    h: &mut dyn CommandHost,
    scope: crate::session::PostEditRipple,
) -> Result<CommandOutcome, String> {
    let n = s.post_edit_ripple(scope);
    s.set_status(tr_args(
        s.lang,
        "Ripple pós-edição: {n} evento(s) deslocado(s).",
        &[("n", &n)],
    ));
    ok_mutated(s, h)
}

fn can_post_ripple(s: &EditorSession, _h: &dyn CommandHost) -> bool {
    !s.post_edit_ripple_plan().is_empty()
}

fn can_restore_time_selection(s: &EditorSession, _h: &dyn CommandHost) -> bool {
    !s.time_sel_history.is_empty()
        || (s.time_sel != s.time_sel_seen && s.time_sel_seen.is_some_and(|(a, b)| b > a))
}

/// Tabela Única de Comandos do Neoveg (§3.3).
/// Passo de Ctrl+Shift+Cima/Baixo na altura das trilhas (pontos).
const TRACK_HEIGHT_STEP: f32 = 12.0;

pub static COMMANDS: &[CommandDef] = &[
    CommandDef {
        id: "options.snap_to_markers",
        label: "Encaixar em marcadores",
        menu_path: "Opções/Encaixar em marcadores",
        default_shortcut: Some(Shortcut::shift(ShortcutKey::F8)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.snap_to_markers = !s.opts.snap_to_markers;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "options.snap_to_events",
        label: "Encaixar em todos os eventos",
        menu_path: "Opções/Encaixar em todos os eventos",
        default_shortcut: Some(Shortcut::ctrl_shift(ShortcutKey::F8)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.snap_to_all_events = !s.opts.snap_to_all_events;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "view.event_headers",
        label: "Mostrar cabeçalhos dos eventos",
        menu_path: "Exibir/Mostrar cabeçalhos dos eventos",
        default_shortcut: Some(Shortcut::ctrl_alt(ShortcutKey::M)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.show_event_headers = !s.opts.show_event_headers;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.restore_time_selection",
        label: "Restaurar seleção de tempo anterior",
        menu_path: "Editar/Restaurar seleção de tempo anterior",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::Backspace)),
        needs_ui: false,
        enabled: can_restore_time_selection,
        run: |s, h| {
            s.restore_previous_time_selection();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "transport.grid_left",
        label: "Mover à esquerda pela grade",
        menu_path: "Transporte/Mover à esquerda pela grade",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::PageUp)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| move_by_grid(s, h, -1),
    },
    CommandDef {
        id: "transport.grid_right",
        label: "Mover à direita pela grade",
        menu_path: "Transporte/Mover à direita pela grade",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::PageDown)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| move_by_grid(s, h, 1),
    },
    CommandDef {
        id: "options.dim_master",
        label: "Atenuar saída do Master (Dim)",
        menu_path: "Opções/Atenuar saída do Master (Dim)",
        default_shortcut: Some(Shortcut::ctrl_shift(ShortcutKey::F12)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ToggleMasterDim, h),
    },
    CommandDef {
        id: "tools.rebuild_peaks",
        label: "Reconstruir picos de áudio",
        menu_path: "Ferramentas/Reconstruir picos de áudio",
        default_shortcut: Some(Shortcut::shift(ShortcutKey::F5)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::RebuildAudioPeaks, h),
    },
    CommandDef {
        id: "file.close",
        label: "Fechar projeto",
        menu_path: "Arquivo/Fechar projeto",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::F4)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::CloseProject, h),
    },
    CommandDef {
        id: "edit.paste_repeat",
        label: "Colar repetindo...",
        menu_path: "Editar/Colar repetindo...",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::B)),
        needs_ui: true,
        enabled: |s, h| h.supports_ui() && !s.clipboard.is_empty(),
        run: |_s, h| ui_action(UiAction::OpenPasteRepeatDialog, h),
    },
    CommandDef {
        id: "edit.post_ripple_tracks",
        label: "Ripple pós-edição: trilhas afetadas",
        menu_path: "Editar/Ripple pós-edição/Trilhas afetadas",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::F)),
        needs_ui: false,
        enabled: can_post_ripple,
        run: |s, h| post_ripple(s, h, crate::session::PostEditRipple::AffectedTracks),
    },
    CommandDef {
        id: "edit.post_ripple_markers",
        label: "Ripple pós-edição: trilhas afetadas, barramentos, marcadores e regiões",
        menu_path: "Editar/Ripple pós-edição/Trilhas afetadas, barramentos, marcadores e regiões",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::F)),
        needs_ui: false,
        enabled: can_post_ripple,
        run: |s, h| post_ripple(s, h, crate::session::PostEditRipple::AffectedBusesMarkers),
    },
    CommandDef {
        id: "edit.post_ripple_all",
        label: "Ripple pós-edição: todas as trilhas, marcadores e regiões",
        menu_path: "Editar/Ripple pós-edição/Todas as trilhas, marcadores e regiões",
        default_shortcut: Some(Shortcut::ctrl_shift(ShortcutKey::F)),
        needs_ui: false,
        enabled: can_post_ripple,
        run: |s, h| post_ripple(s, h, crate::session::PostEditRipple::All),
    },
    CommandDef {
        id: "edit.rename_event",
        label: "Renomear evento",
        menu_path: "Editar/Renomear evento",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::F2)),
        needs_ui: true,
        enabled: has_selected_events_ui,
        run: |_s, h| ui_action(UiAction::RenameSelectedEvent, h),
    },
    CommandDef {
        id: "edit.cursor_prev_selected_edge",
        label: "Ir para a borda do evento selecionado anterior",
        menu_path: "Editar/Ir para a borda do evento selecionado anterior",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::OpenBracket)),
        needs_ui: false,
        enabled: |s, _| s.expanded_edit.is_some() || !s.sel_events.is_empty(),
        run: |s, h| selected_edge_or_nudge(s, h, false),
    },
    CommandDef {
        id: "edit.cursor_next_selected_edge",
        label: "Ir para a borda do evento selecionado seguinte",
        menu_path: "Editar/Ir para a borda do evento selecionado seguinte",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::CloseBracket)),
        needs_ui: false,
        enabled: |s, _| s.expanded_edit.is_some() || !s.sel_events.is_empty(),
        run: |s, h| selected_edge_or_nudge(s, h, true),
    },
    CommandDef {
        id: "edit.clear_group",
        label: "Limpar grupo",
        menu_path: "Editar/Limpar grupo",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::U)),
        needs_ui: false,
        enabled: has_selected_events,
        run: |s, h| {
            s.clear_group_selected();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "event.pitch_up_cent",
        label: "Subir tom (+1 cent)",
        menu_path: "Editar/Pitch/Subir 1 cent",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::Equals)),
        needs_ui: false,
        enabled: has_selected_audio_events,
        run: |s, h| {
            s.shift_pitch_selected(0.01);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "event.pitch_down_cent",
        label: "Descer tom (-1 cent)",
        menu_path: "Editar/Pitch/Descer 1 cent",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::Minus)),
        needs_ui: false,
        enabled: has_selected_audio_events,
        run: |s, h| {
            s.shift_pitch_selected(-0.01);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "track.mute_exclusive",
        label: "Mudo só na trilha selecionada",
        menu_path: "Trilha/Mudo exclusivo",
        default_shortcut: Some(Shortcut::shift(ShortcutKey::Z)),
        needs_ui: false,
        enabled: has_selected_track,
        run: |s, h| {
            s.set_selected_track_exclusive(false);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "track.solo_exclusive",
        label: "Solo só na trilha selecionada",
        menu_path: "Trilha/Solo exclusivo",
        default_shortcut: Some(Shortcut::shift(ShortcutKey::X)),
        needs_ui: false,
        enabled: has_selected_track,
        run: |s, h| {
            s.set_selected_track_exclusive(true);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "view.tracks_taller",
        label: "Aumentar a altura de todas as trilhas",
        menu_path: "Exibir/Aumentar a altura de todas as trilhas",
        default_shortcut: Some(Shortcut::ctrl_shift(ShortcutKey::ArrowUp)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.adjust_all_track_heights(TRACK_HEIGHT_STEP);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "view.tracks_shorter",
        label: "Diminuir a altura de todas as trilhas",
        menu_path: "Exibir/Diminuir a altura de todas as trilhas",
        default_shortcut: Some(Shortcut::ctrl_shift(ShortcutKey::ArrowDown)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.adjust_all_track_heights(-TRACK_HEIGHT_STEP);
            ok_mutated(s, h)
        },
    },
    // --- Arquivo ---
    CommandDef {
        id: "file.new",
        label: "Novo projeto",
        menu_path: "Arquivo/Novo projeto",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::N)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            if h.supports_ui() {
                ui_action(UiAction::NewProject, h)
            } else {
                s.reset_project(crate::model::Project::new_default(), None);
                ok_mutated(s, h)
            }
        },
    },
    CommandDef {
        id: "file.open",
        label: "Abrir...",
        menu_path: "Arquivo/Abrir...",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::O)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenProjectDialog, h),
    },
    CommandDef {
        id: "file.save",
        label: "Salvar",
        menu_path: "Arquivo/Salvar",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::S)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            if h.supports_ui() {
                ui_action(UiAction::SaveProject { save_as: false }, h)
            } else if let Some(p) = s.path.clone() {
                s.project.save_file(&p)?;
                s.dirty = false;
                Ok(CommandOutcome::Executed)
            } else {
                Err(tr(
                    s.lang,
                    "projeto ainda não tem caminho definido para salvar sem diálogo",
                )
                .into())
            }
        },
    },
    CommandDef {
        id: "file.save_as",
        label: "Salvar como...",
        menu_path: "Arquivo/Salvar como...",
        default_shortcut: Some(Shortcut::ctrl_shift(ShortcutKey::S)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SaveProject { save_as: true }, h),
    },
    CommandDef {
        id: "file.save_version",
        label: "Salvar versão",
        menu_path: "Arquivo/Salvar versão",
        default_shortcut: Some(Shortcut::ctrl_alt(ShortcutKey::S)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SaveVersion, h),
    },
    CommandDef {
        id: "file.import_media",
        label: "Importar mídia...",
        menu_path: "Arquivo/Importar mídia...",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::I)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ImportMediaDialog, h),
    },
    CommandDef {
        id: "file.import_sequence",
        label: "Importar sequência de imagens...",
        menu_path: "Arquivo/Importar sequência de imagens...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ImportSequenceDialog, h),
    },
    CommandDef {
        id: "file.convert_media",
        label: "Converter mídia para formato compatível...",
        menu_path: "Arquivo/Converter mídia para formato compatível...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ConvertMediaDialog, h),
    },
    CommandDef {
        id: "file.import_otio",
        label: "Importar OpenTimelineIO (.otio)...",
        menu_path: "Arquivo/Importar OpenTimelineIO (.otio)...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ImportOtioDialog, h),
    },
    CommandDef {
        id: "file.export_otio",
        label: "Exportar OpenTimelineIO (.otio)...",
        menu_path: "Arquivo/Exportar OpenTimelineIO (.otio)...",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            export_or_ui(
                s,
                h,
                UiAction::ExportOtioDialog,
                "otio",
                "OTIO",
                |sess, p| crate::otio::export_otio_file(&sess.project, p),
            )
        },
    },
    CommandDef {
        id: "file.import_subtitles",
        label: "Importar legendas (SRT/WebVTT)...",
        menu_path: "Arquivo/Importar legendas (SRT/WebVTT)...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ImportSubtitlesDialog, h),
    },
    CommandDef {
        id: "file.export_srt",
        label: "Exportar legendas SRT (.srt)...",
        menu_path: "Arquivo/Exportar legendas SRT (.srt)...",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            export_or_ui(s, h, UiAction::ExportSrtDialog, "srt", "SRT", |sess, p| {
                crate::subtitles::export_srt_file(&sess.project, p).map(|_| ())
            })
        },
    },
    CommandDef {
        id: "file.export_vtt",
        label: "Exportar legendas WebVTT (.vtt)...",
        menu_path: "Arquivo/Exportar legendas WebVTT (.vtt)...",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            export_or_ui(
                s,
                h,
                UiAction::ExportVttDialog,
                "vtt",
                "WebVTT",
                |sess, p| crate::subtitles::export_vtt_file(&sess.project, p).map(|_| ()),
            )
        },
    },
    CommandDef {
        id: "file.import_edl",
        label: "Importar EDL CMX 3600 (.edl)...",
        menu_path: "Arquivo/Importar EDL CMX 3600 (.edl)...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ImportEdlDialog, h),
    },
    CommandDef {
        id: "file.export_edl",
        label: "Exportar EDL CMX 3600 (.edl)...",
        menu_path: "Arquivo/Exportar EDL CMX 3600 (.edl)...",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            export_or_ui(s, h, UiAction::ExportEdlDialog, "edl", "EDL", |sess, p| {
                crate::edl::export_edl_file(&sess.project, p, None)
            })
        },
    },
    CommandDef {
        id: "file.import_fcpxml",
        label: "Importar Final Cut Pro XML (.fcpxml)...",
        menu_path: "Arquivo/Importar Final Cut Pro XML (.fcpxml)...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ImportFcpxmlDialog, h),
    },
    CommandDef {
        id: "file.export_fcpxml",
        label: "Exportar Final Cut Pro XML (.fcpxml)...",
        menu_path: "Arquivo/Exportar Final Cut Pro XML (.fcpxml)...",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            export_or_ui(
                s,
                h,
                UiAction::ExportFcpxmlDialog,
                "fcpxml",
                "FCPXML",
                |sess, p| crate::fcpxml::export_fcpxml_file(&sess.project, p, None),
            )
        },
    },
    CommandDef {
        id: "file.import_aaf",
        label: "Importar AAF (.aaf)...",
        menu_path: "Arquivo/Importar AAF (.aaf)...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ImportAafDialog, h),
    },
    CommandDef {
        id: "file.export_aaf",
        label: "Exportar AAF (.aaf)...",
        menu_path: "Arquivo/Exportar AAF (.aaf)...",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            export_or_ui(s, h, UiAction::ExportAafDialog, "aaf", "AAF", |sess, p| {
                crate::aaf::export_aaf_file(&sess.project, p)
            })
        },
    },
    CommandDef {
        id: "file.import_fcp7",
        label: "Importar Final Cut Pro 7 XML (.xml)...",
        menu_path: "Arquivo/Importar Final Cut Pro 7 XML (.xml)...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ImportFcp7Dialog, h),
    },
    CommandDef {
        id: "file.export_fcp7",
        label: "Exportar Final Cut Pro 7 XML (.xml)...",
        menu_path: "Arquivo/Exportar Final Cut Pro 7 XML (.xml)...",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            export_or_ui(
                s,
                h,
                UiAction::ExportFcp7Dialog,
                "xml",
                "FCP7 XML",
                |sess, p| crate::fcp7xml::export_fcp7_file(&sess.project, p, None),
            )
        },
    },
    CommandDef {
        id: "file.render",
        label: "Renderizar como...",
        menu_path: "Arquivo/Renderizar como...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenRenderDialog, h),
    },
    CommandDef {
        id: "file.send_to_encoder",
        label: "Enviar para o Neoveg Encoder...",
        menu_path: "Arquivo/Enviar para o Neoveg Encoder...",
        default_shortcut: Some(Shortcut::ctrl_shift(ShortcutKey::M)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SendToEncoder, h),
    },
    CommandDef {
        id: "file.capture",
        label: "Neoveg Capture...",
        menu_path: "Arquivo/Neoveg Capture...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenCapture, h),
    },
    CommandDef {
        id: "file.import_nvcap",
        label: "Importar projeto Neoveg Capture (.nvcap)...",
        menu_path: "Arquivo/Importar projeto Neoveg Capture (.nvcap)...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ImportCaptureProjectDialog, h),
    },
    CommandDef {
        id: "file.snapshot",
        label: "Salvar quadro (snapshot) em PNG...",
        menu_path: "Arquivo/Salvar quadro (snapshot) em PNG...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SnapshotDialog, h),
    },
    CommandDef {
        id: "file.project_props",
        label: "Propriedades do projeto...",
        menu_path: "Arquivo/Propriedades do projeto...",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::Enter)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenProjectProps, h),
    },
    CommandDef {
        id: "file.work_folders",
        label: "Pastas de trabalho...",
        menu_path: "Arquivo/Pastas de trabalho...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenWorkFolders, h),
    },
    CommandDef {
        id: "file.find_missing_media",
        label: "Localizar mídia ausente...",
        menu_path: "Arquivo/Localizar mídia ausente...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::FindMissingMedia, h),
    },
    CommandDef {
        id: "file.advanced_save",
        label: "Salvamento avançado e backups...",
        menu_path: "Arquivo/Salvamento avançado e backups...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenAdvancedSaveDialog, h),
    },
    CommandDef {
        id: "file.archive_project",
        label: "Arquivar projeto com mídias...",
        menu_path: "Arquivo/Arquivar projeto com mídias...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenArchiveProjectDialog, h),
    },
    CommandDef {
        id: "file.exit",
        label: "Sair",
        menu_path: "Arquivo/Sair",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ExitApp, h),
    },
    // --- Editar ---
    CommandDef {
        id: "edit.undo",
        label: "Desfazer",
        menu_path: "Editar/Desfazer",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::Z)),
        needs_ui: false,
        enabled: |s, _| s.can_undo(),
        run: |s, h| {
            s.undo();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.redo",
        label: "Refazer",
        menu_path: "Editar/Refazer",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::Y)),
        needs_ui: false,
        enabled: |s, _| s.can_redo(),
        run: |s, h| {
            s.redo();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.cut",
        label: "Recortar",
        menu_path: "Editar/Recortar",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::X)),
        needs_ui: false,
        enabled: has_selected_events,
        run: |s, h| {
            s.cut_selection();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.copy",
        label: "Copiar",
        menu_path: "Editar/Copiar",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::C)),
        needs_ui: false,
        enabled: has_selected_events,
        run: |s, h| {
            s.copy_selection();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.paste",
        label: "Colar",
        menu_path: "Editar/Colar",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::V)),
        needs_ui: false,
        enabled: has_clipboard,
        run: |s, h| {
            s.paste(false);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.paste_insert",
        label: "Colar inserindo (ripple)",
        menu_path: "Editar/Colar inserindo (ripple)",
        default_shortcut: Some(Shortcut::ctrl_shift(ShortcutKey::V)),
        needs_ui: false,
        enabled: has_clipboard,
        run: |s, h| {
            s.paste(true);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.paste_attributes",
        label: "Colar atributos do evento",
        menu_path: "Editar/Colar atributos do evento",
        default_shortcut: Some(Shortcut::ctrl_alt(ShortcutKey::V)),
        needs_ui: false,
        enabled: |s, _| !s.clipboard.is_empty() && !s.sel_events.is_empty(),
        run: |s, h| {
            s.paste_attributes(crate::session::PasteAttributesMask::ALL);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.paste_attributes_selective",
        label: "Colar atributos seletivamente...",
        menu_path: "Editar/Colar atributos seletivamente...",
        default_shortcut: None,
        needs_ui: true,
        enabled: |s, _| !s.clipboard.is_empty() && !s.sel_events.is_empty(),
        run: |_s, h| ui_action(UiAction::OpenPasteAttributesDialog, h),
    },
    CommandDef {
        id: "edit.delete",
        label: "Excluir",
        menu_path: "Editar/Excluir",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::Delete)),
        needs_ui: false,
        enabled: |s, _| !s.sel_events.is_empty() || (s.time_sel.is_some() && s.opts.auto_ripple),
        run: |s, h| {
            s.delete_selection();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.split",
        label: "Dividir no cursor",
        menu_path: "Editar/Dividir no cursor",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::S)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.split_at_cursor();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.smart_split",
        label: "Smart Split (corte com transição)",
        menu_path: "Editar/Smart Split",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::S)),
        needs_ui: false,
        enabled: has_time_selection,
        run: |s, h| {
            s.smart_split();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.heal_events",
        label: "Juntar eventos (Heal)",
        menu_path: "Editar/Juntar eventos (Heal)",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| s.can_heal_selection(),
        run: |s, h| {
            s.heal_events();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.cut_to_overlap",
        label: "Converter corte em crossfade",
        menu_path: "Editar/Converter corte em crossfade",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| s.can_convert_cut_to_overlap(),
        run: |s, h| {
            s.convert_cut_to_overlap();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.crossfade_to_cut",
        label: "Converter crossfade em corte seco",
        menu_path: "Editar/Converter crossfade em corte seco",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| s.has_target_crossfade(),
        run: |s, h| {
            s.convert_target_crossfades_to_cuts();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.detect_scenes",
        label: "Detectar cenas e cortes...",
        menu_path: "Editar/Detectar cenas e cortes...",
        default_shortcut: None,
        needs_ui: true,
        enabled: has_selected_video_events_ui,
        run: |_s, h| ui_action(UiAction::OpenSceneDetectDialog, h),
    },
    CommandDef {
        id: "edit.expanded_edit_mode",
        label: "Modo de edição expandido (A/B)",
        menu_path: "Editar/Modo de edição expandido",
        default_shortcut: Some(Shortcut::ctrl_alt(ShortcutKey::E)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.toggle_expanded_edit();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.expanded_nudge_left",
        label: "Deslocar corte 1 quadro à esquerda",
        menu_path: "Editar/Expandido: -1 quadro",
        // Ctrl+[ passa por `edit.cursor_prev_selected_edge`, que desloca o
        // corte quando o modo expandido está ativo
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| s.expanded_edit.is_some(),
        run: |s, h| {
            let fd = s.project.settings.frame_dur();
            s.nudge_expanded_edit(-fd);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.expanded_nudge_right",
        label: "Deslocar corte 1 quadro à direita",
        menu_path: "Editar/Expandido: +1 quadro",
        // Ctrl+] passa por `edit.cursor_next_selected_edge`
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| s.expanded_edit.is_some(),
        run: |s, h| {
            let fd = s.project.settings.frame_dur();
            s.nudge_expanded_edit(fd);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "event.next_take",
        label: "Próximo take",
        menu_path: "Editar/Take/Próximo take",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::T)),
        needs_ui: false,
        enabled: |s, _| s.selection_has_takes(),
        run: |s, h| {
            s.next_take_selected();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "event.prev_take",
        label: "Take anterior",
        menu_path: "Editar/Take/Take anterior",
        default_shortcut: Some(Shortcut::shift(ShortcutKey::T)),
        needs_ui: false,
        enabled: |s, _| s.selection_has_takes(),
        run: |s, h| {
            s.prev_take_selected();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "event.lock",
        label: "Travar eventos",
        menu_path: "Editar/Travar eventos",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| !s.sel_events.is_empty(),
        run: |s, h| {
            s.toggle_lock_selected();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "event.delete_active_take",
        label: "Excluir take ativo",
        menu_path: "Editar/Take/Excluir take ativo",
        default_shortcut: None,
        needs_ui: false,
        // nunca deixa um evento sem take: só com mais de um
        enabled: |s, _| s.selection_has_takes(),
        run: |s, h| {
            s.delete_active_take_selected();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "event.take_chooser",
        label: "Escolher take...",
        menu_path: "Editar/Take/Escolher take...",
        default_shortcut: None,
        needs_ui: true,
        enabled: has_selected_events_ui,
        run: |_s, h| ui_action(UiAction::OpenTakeChooser, h),
    },
    CommandDef {
        id: "event.pitch_up",
        label: "Subir tom (+1 semitom)",
        menu_path: "Editar/Pitch/Subir 1 semitom",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::Equals)),
        needs_ui: false,
        enabled: has_selected_audio_events,
        run: |s, h| {
            s.shift_pitch_selected(1.0);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "event.pitch_down",
        label: "Descer tom (-1 semitom)",
        menu_path: "Editar/Pitch/Descer 1 semitom",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::Minus)),
        needs_ui: false,
        enabled: has_selected_audio_events,
        run: |s, h| {
            s.shift_pitch_selected(-1.0);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "event.pitch_up_oct",
        label: "Subir tom (+1 oitava)",
        menu_path: "Editar/Pitch/Subir 1 oitava",
        default_shortcut: Some(Shortcut::shift(ShortcutKey::Equals)),
        needs_ui: false,
        enabled: has_selected_audio_events,
        run: |s, h| {
            s.shift_pitch_selected(12.0);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "event.pitch_down_oct",
        label: "Descer tom (-1 oitava)",
        menu_path: "Editar/Pitch/Descer 1 oitava",
        default_shortcut: Some(Shortcut::shift(ShortcutKey::Minus)),
        needs_ui: false,
        enabled: has_selected_audio_events,
        run: |s, h| {
            s.shift_pitch_selected(-12.0);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "event.pitch_reset",
        label: "Redefinir tom (0 st)",
        menu_path: "Editar/Pitch/Redefinir tom",
        default_shortcut: Some(Shortcut::ctrl_shift(ShortcutKey::Equals)),
        needs_ui: false,
        enabled: has_selected_audio_events,
        run: |s, h| {
            s.reset_pitch_selected();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "event.toggle_pitch_lock",
        label: "Preservar tom ao esticar",
        menu_path: "Editar/Pitch/Preservar tom ao esticar",
        default_shortcut: None,
        needs_ui: false,
        enabled: has_selected_audio_events,
        run: |s, h| {
            s.toggle_lock_pitch_to_stretch();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "event.sync_link_create",
        label: "Criar Sync Link (vincular ao primeiro)",
        menu_path: "Editar/Sync Link/Criar vínculo",
        default_shortcut: None,
        needs_ui: false,
        enabled: can_create_sync_link,
        run: |s, h| {
            s.create_sync_links(None);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "event.sync_link_unlink",
        label: "Desvincular Sync Link",
        menu_path: "Editar/Sync Link/Desvincular",
        default_shortcut: None,
        needs_ui: false,
        enabled: has_sync_link_in_selection,
        run: |s, h| {
            s.unlink_sync_links();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "event.sync_link_select",
        label: "Selecionar eventos com Sync Link",
        menu_path: "Editar/Sync Link/Selecionar vinculados",
        default_shortcut: None,
        needs_ui: false,
        enabled: has_sync_link_in_selection,
        run: |s, h| {
            s.select_sync_linked();
            ok_mutated(s, h)
        },
    },
    // Ferramentas de edição (manual do VEGAS Pro 21, p. 236): trocar de
    // ferramenta não muda o projeto nem cria passo de Desfazer.
    CommandDef {
        id: "edit.tool.normal",
        label: "Ferramenta Normal",
        menu_path: "Editar/Ferramenta de edição/Normal",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::D)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, _| {
            s.edit_tool = crate::session::EditTool::Normal;
            Ok(CommandOutcome::Executed)
        },
    },
    CommandDef {
        id: "edit.tool.envelope",
        label: "Ferramenta Envelope",
        menu_path: "Editar/Ferramenta de edição/Envelope",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, _| {
            s.edit_tool = crate::session::EditTool::Envelope;
            Ok(CommandOutcome::Executed)
        },
    },
    CommandDef {
        id: "edit.tool.selection",
        label: "Ferramenta Seleção",
        menu_path: "Editar/Ferramenta de edição/Seleção",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, _| {
            s.edit_tool = crate::session::EditTool::Selection;
            Ok(CommandOutcome::Executed)
        },
    },
    CommandDef {
        id: "edit.tool.zoom",
        label: "Ferramenta Zoom",
        menu_path: "Editar/Ferramenta de edição/Zoom",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, _| {
            s.edit_tool = crate::session::EditTool::Zoom;
            Ok(CommandOutcome::Executed)
        },
    },
    CommandDef {
        id: "edit.tool.hand",
        label: "Ferramenta Mão",
        menu_path: "Editar/Ferramenta de edição/Mão",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, _| {
            s.edit_tool = crate::session::EditTool::Hand;
            Ok(CommandOutcome::Executed)
        },
    },
    CommandDef {
        id: "edit.tool.next",
        label: "Próxima ferramenta",
        menu_path: "Editar/Ferramenta de edição/Próxima ferramenta",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::D)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, _| {
            s.edit_tool = s.edit_tool.next();
            Ok(CommandOutcome::Executed)
        },
    },
    CommandDef {
        id: "edit.tool.prev",
        label: "Ferramenta anterior",
        menu_path: "Editar/Ferramenta de edição/Ferramenta anterior",
        default_shortcut: Some(Shortcut::shift(ShortcutKey::D)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, _| {
            s.edit_tool = s.edit_tool.prev();
            Ok(CommandOutcome::Executed)
        },
    },
    CommandDef {
        id: "edit.tool.mode.normal",
        label: "Modo Normal",
        menu_path: "Editar/Ferramenta de edição/Modo Normal",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, _| {
            s.edit_tool = crate::session::EditTool::Normal;
            s.edit_mode = crate::session::EditMode::Normal;
            Ok(CommandOutcome::Executed)
        },
    },
    CommandDef {
        id: "edit.tool.mode.shuffle",
        label: "Modo Shuffle (reordenar)",
        menu_path: "Editar/Ferramenta de edição/Modo Shuffle",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, _| {
            s.edit_tool = crate::session::EditTool::Normal;
            s.edit_mode = crate::session::EditMode::Shuffle;
            Ok(CommandOutcome::Executed)
        },
    },
    CommandDef {
        id: "edit.tool.mode.slip",
        label: "Modo Slip (deslizar conteúdo)",
        menu_path: "Editar/Ferramenta de edição/Modo Slip",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, _| {
            s.edit_tool = crate::session::EditTool::Normal;
            s.edit_mode = crate::session::EditMode::Slip;
            Ok(CommandOutcome::Executed)
        },
    },
    CommandDef {
        id: "edit.tool.mode.slide",
        label: "Modo Slide (deslizar evento)",
        menu_path: "Editar/Ferramenta de edição/Modo Slide",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, _| {
            s.edit_tool = crate::session::EditTool::Normal;
            s.edit_mode = crate::session::EditMode::Slide;
            Ok(CommandOutcome::Executed)
        },
    },
    CommandDef {
        id: "edit.tool.mode.stretch",
        label: "Modo Stretch (esticamento)",
        menu_path: "Editar/Ferramenta de edição/Modo Stretch",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, _| {
            s.edit_tool = crate::session::EditTool::Normal;
            s.edit_mode = crate::session::EditMode::Stretch;
            Ok(CommandOutcome::Executed)
        },
    },
    CommandDef {
        id: "edit.tool.mode.split_trim",
        label: "Modo Split Trim (dividir e aparar)",
        menu_path: "Editar/Ferramenta de edição/Modo Split Trim",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, _| {
            s.edit_tool = crate::session::EditTool::Normal;
            s.edit_mode = crate::session::EditMode::SplitTrim;
            Ok(CommandOutcome::Executed)
        },
    },
    CommandDef {
        id: "edit.tool.mode.roll",
        label: "Modo Roll (ajuste de corte)",
        menu_path: "Editar/Ferramenta de edição/Modo Roll",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, _| {
            s.edit_tool = crate::session::EditTool::Normal;
            s.edit_mode = crate::session::EditMode::Roll;
            Ok(CommandOutcome::Executed)
        },
    },
    CommandDef {
        id: "edit.tool.mode.next",
        label: "Alternar modo da ferramenta Normal",
        menu_path: "Editar/Ferramenta de edição/Alternar modo",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, _| {
            let modes = crate::session::EditMode::NORMAL_MODES;
            let idx = modes.iter().position(|m| *m == s.edit_mode).unwrap_or(0);
            s.edit_tool = crate::session::EditTool::Normal;
            s.edit_mode = modes[(idx + 1) % modes.len()];
            Ok(CommandOutcome::Executed)
        },
    },
    CommandDef {
        id: "edit.tool.axis.both",
        label: "Eixo de seleção/zoom livre",
        menu_path: "Editar/Ferramenta de edição/Eixo livre",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, _| {
            s.box_axis = crate::session::BoxAxisMode::Both;
            Ok(CommandOutcome::Executed)
        },
    },
    CommandDef {
        id: "edit.tool.axis.horizontal",
        label: "Eixo de seleção/zoom horizontal",
        menu_path: "Editar/Ferramenta de edição/Eixo horizontal",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, _| {
            s.box_axis = crate::session::BoxAxisMode::Horizontal;
            Ok(CommandOutcome::Executed)
        },
    },
    CommandDef {
        id: "edit.tool.axis.vertical",
        label: "Eixo de seleção/zoom vertical",
        menu_path: "Editar/Ferramenta de edição/Eixo vertical",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, _| {
            s.box_axis = crate::session::BoxAxisMode::Vertical;
            Ok(CommandOutcome::Executed)
        },
    },
    CommandDef {
        id: "edit.trim_to_selection",
        label: "Aparar para a seleção",
        menu_path: "Editar/Aparar para a seleção",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::T)),
        needs_ui: false,
        enabled: |s, _| has_time_selection(s, &HeadlessHost) && !s.sel_events.is_empty(),
        run: |s, h| {
            s.trim_to_selection();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.trim_start",
        label: "Aparar início do evento no cursor",
        menu_path: "Editar/Aparar início no cursor",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::OpenBracket)),
        needs_ui: false,
        enabled: has_selected_events,
        run: |s, h| {
            s.trim_selected_start_to_cursor();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.trim_end",
        label: "Aparar fim do evento no cursor",
        menu_path: "Editar/Aparar fim no cursor",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::CloseBracket)),
        needs_ui: false,
        enabled: has_selected_events,
        run: |s, h| {
            s.trim_selected_end_to_cursor();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.delete_range_ripple",
        label: "Excluir intervalo selecionado (ripple)",
        menu_path: "Editar/Excluir intervalo selecionado (ripple)",
        default_shortcut: None,
        needs_ui: false,
        enabled: has_time_selection,
        run: |s, h| {
            s.delete_time_range();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.insert_time",
        label: "Inserir tempo na seleção",
        menu_path: "Editar/Inserir tempo na seleção",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.insert_time();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.close_gaps",
        label: "Fechar lacunas da trilha selecionada",
        menu_path: "Editar/Fechar lacunas da trilha selecionada",
        default_shortcut: None,
        needs_ui: false,
        enabled: has_selected_track,
        run: |s, h| {
            s.close_gaps();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.group",
        label: "Agrupar",
        menu_path: "Editar/Agrupar",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::G)),
        needs_ui: false,
        enabled: |s, _| s.sel_events.len() >= 2,
        run: |s, h| {
            s.group_selected();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.ungroup",
        label: "Desagrupar",
        menu_path: "Editar/Desagrupar",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::U)),
        needs_ui: false,
        enabled: has_selected_events,
        run: |s, h| {
            s.ungroup_selected();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.select_group",
        label: "Selecionar grupo",
        menu_path: "Editar/Selecionar grupo",
        default_shortcut: Some(Shortcut::shift(ShortcutKey::G)),
        needs_ui: false,
        enabled: has_selected_events,
        run: |s, h| {
            s.select_group();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.select_events_to_end",
        label: "Selecionar eventos até o fim",
        menu_path: "Editar/Selecionar eventos até o fim",
        default_shortcut: None,
        needs_ui: false,
        enabled: has_selected_events,
        run: |s, h| {
            s.select_events_to_end();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.select_all",
        label: "Selecionar todos",
        menu_path: "Editar/Selecionar todos",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::A)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.select_all();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.deselect_all",
        label: "Limpar seleção",
        menu_path: "Editar/Limpar seleção",
        default_shortcut: Some(Shortcut::ctrl_shift(ShortcutKey::A)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.deselect_all();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.set_in",
        label: "Definir entrada da seleção (I)",
        menu_path: "Editar/Definir entrada (I)",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::I)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.set_selection_in();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.set_out",
        label: "Definir saída da seleção (O)",
        menu_path: "Editar/Definir saída (O)",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::O)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.set_selection_out();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "edit.select_loop_region",
        label: "Selecionar região de loop",
        menu_path: "Editar/Selecionar região de loop",
        default_shortcut: Some(Shortcut::shift(ShortcutKey::Q)),
        needs_ui: false,
        enabled: |s, _| s.project.loop_region.is_some(),
        run: |s, h| {
            s.select_loop_region();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "file.open_nested_tab",
        label: "Abrir projeto aninhado em aba",
        menu_path: "Arquivo/Abrir projeto aninhado em aba",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::N)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenNestedProjectTab, h),
    },
    CommandDef {
        id: "timeline.open_parent",
        label: "Voltar à linha do tempo pai",
        menu_path: "Editar/Voltar à linha do tempo pai",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::P)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenParentTimeline, h),
    },
    CommandDef {
        id: "edit.goto",
        label: "Ir para...",
        menu_path: "Editar/Ir para...",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::G)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenGoto, h),
    },
    CommandDef {
        id: "edit.properties.layout",
        label: "Configurar Layout...",
        menu_path: "Editar/Propriedades/Layout/Configurar Layout...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenEditLayoutProps, h),
    },
    CommandDef {
        id: "edit.properties.layout.sonic_foundry",
        label: "Clássico",
        menu_path: "Editar/Propriedades/Layout/Tipos de layout/Clássico",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SetLayoutType(LayoutType::SonicFoundry), h),
    },
    CommandDef {
        id: "edit.properties.layout.sony",
        label: "Intermediário",
        menu_path: "Editar/Propriedades/Layout/Tipos de layout/Intermediário",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SetLayoutType(LayoutType::Sony), h),
    },
    CommandDef {
        id: "edit.properties.layout.magix",
        label: "Recente",
        menu_path: "Editar/Propriedades/Layout/Tipos de layout/Recente",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SetLayoutType(LayoutType::Magix), h),
    },
    CommandDef {
        id: "edit.properties.layout.moderno",
        label: "Moderno",
        menu_path: "Editar/Propriedades/Layout/Tipos de layout/Moderno",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SetLayoutType(LayoutType::Moderno), h),
    },
    // --- Exibir ---
    CommandDef {
        id: "view.tab.project_media",
        label: "Mídia do Projeto",
        menu_path: "Exibir/Mídia do Projeto",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::Num5)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::ProjectMedia), h),
    },
    CommandDef {
        id: "view.tab.explorer",
        label: "Explorer",
        menu_path: "Exibir/Explorer",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::Num1)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::Explorer), h),
    },
    CommandDef {
        id: "view.tab.trimmer",
        label: "Trimmer",
        menu_path: "Exibir/Trimmer",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::Num2)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::Trimmer), h),
    },
    CommandDef {
        id: "view.tab.edit_details",
        label: "Detalhes da Edição",
        menu_path: "Exibir/Detalhes da Edição",
        default_shortcut: Some(Shortcut::ctrl_alt(ShortcutKey::Num4)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::EditDetails), h),
    },
    CommandDef {
        id: "view.tab.transitions",
        label: "Transições",
        menu_path: "Exibir/Transições",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::Num7)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::Transitions), h),
    },
    CommandDef {
        id: "view.tab.video_fx",
        label: "Efeitos de Vídeo",
        menu_path: "Exibir/Efeitos de Vídeo",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::Num8)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::VideoFx), h),
    },
    CommandDef {
        id: "view.tab.color_grading",
        label: "Color Grading",
        menu_path: "Exibir/Color Grading",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::G)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::ColorGrading), h),
    },
    CommandDef {
        id: "view.tab.audio_fx",
        label: "Efeitos de Áudio",
        menu_path: "Exibir/Efeitos de Áudio",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::AudioFx), h),
    },
    CommandDef {
        id: "view.tab.generators",
        label: "Geradores de Mídia",
        menu_path: "Exibir/Geradores de Mídia",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::Num9)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::Generators), h),
    },
    CommandDef {
        id: "view.tab.mixer",
        label: "Mixer",
        menu_path: "Exibir/Mixer",
        default_shortcut: Some(Shortcut::ctrl_alt(ShortcutKey::Num5)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::Mixer), h),
    },
    CommandDef {
        id: "view.tab.loudness",
        label: "Medidores de Loudness",
        menu_path: "Exibir/Medidores de Loudness",
        default_shortcut: Some(Shortcut::ctrl_alt(ShortcutKey::Num7)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::Loudness), h),
    },
    CommandDef {
        id: "view.tab.surround_panner",
        label: "Surround Panner",
        menu_path: "Exibir/Surround Panner",
        default_shortcut: Some(Shortcut::ctrl_alt(ShortcutKey::Num3)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::SurroundPanner), h),
    },
    CommandDef {
        id: "view.tab.plugin_manager",
        label: "Plug-In Manager",
        menu_path: "Exibir/Plug-In Manager",
        default_shortcut: Some(Shortcut::ctrl_alt(ShortcutKey::Num1)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::PluginManager), h),
    },
    CommandDef {
        id: "view.tab.compositors",
        label: "Compositores",
        menu_path: "Exibir/Compositores",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::Compositors), h),
    },
    CommandDef {
        id: "view.bus_tracks",
        label: "Mostrar trilhas de barramentos",
        menu_path: "Exibir/Mostrar trilhas de barramentos",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::B)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.show_bus_tracks = !s.opts.show_bus_tracks;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "view.video_bus_track",
        label: "Mostrar trilha do barramento de vídeo",
        menu_path: "Exibir/Mostrar trilha do barramento de vídeo",
        default_shortcut: Some(Shortcut::ctrl_shift(ShortcutKey::B)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.show_video_bus_track = !s.opts.show_video_bus_track;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "view.tab.scopes",
        label: "Escopos",
        menu_path: "Exibir/Escopos",
        default_shortcut: Some(Shortcut::ctrl_alt(ShortcutKey::Num2)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::Scopes), h),
    },
    CommandDef {
        id: "view.tab.notes",
        label: "Notas",
        menu_path: "Exibir/Notas",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::Num6)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::Notes), h),
    },
    CommandDef {
        id: "view.tab.encoder",
        label: "Neoveg Encoder",
        menu_path: "Exibir/Neoveg Encoder",
        default_shortcut: Some(Shortcut::new(true, true, true, ShortcutKey::M)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::Encoder), h),
    },
    CommandDef {
        id: "view.tab.capture",
        label: "Neoveg Capture",
        menu_path: "Exibir/Neoveg Capture",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::Capture), h),
    },
    CommandDef {
        id: "view.tab.transcript",
        label: "Transcrição de fala",
        menu_path: "Exibir/Transcrição de fala",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::Transcript), h),
    },
    CommandDef {
        id: "view.tab.store",
        label: "Loja de scripts",
        menu_path: "Exibir/Loja de scripts",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::Store), h),
    },
    CommandDef {
        id: "speech.remove_silences",
        label: "Remover silêncios e pausas",
        menu_path: "Editar/Fala/Remover silêncios e pausas",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| {
            s.project
                .transcript
                .as_ref()
                .map(|t| !t.segments.is_empty())
                .unwrap_or(false)
        },
        run: |s, h| {
            crate::speech::remove_transcript_silences(
                s,
                crate::time::Ticks::from_millis_i64(1200),
            )?;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "speech.generate_subtitles",
        label: "Legendas a partir da fala",
        menu_path: "Inserir/Legendas a partir da fala",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| {
            s.project
                .transcript
                .as_ref()
                .map(|t| !t.segments.is_empty())
                .unwrap_or(false)
        },
        run: |s, h| {
            crate::speech::generate_regions_from_transcript(s);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "view.envelopes",
        label: "Mostrar envelopes",
        menu_path: "Exibir/Mostrar envelopes",
        default_shortcut: Some(Shortcut::ctrl_shift(ShortcutKey::E)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.show_envelopes = !s.opts.show_envelopes;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "view.waveforms",
        label: "Mostrar formas de onda",
        menu_path: "Exibir/Mostrar formas de onda",
        default_shortcut: Some(Shortcut::ctrl_shift(ShortcutKey::W)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.show_waveforms = !s.opts.show_waveforms;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "view.waveform_zoom_in",
        label: "Ampliar formas de onda (vertical)",
        menu_path: "Exibir/Ampliar formas de onda",
        default_shortcut: Some(Shortcut::shift(ShortcutKey::ArrowUp)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.waveform_zoom_steps = s
                .opts
                .waveform_zoom_steps
                .saturating_add(1)
                .min(crate::session::MAX_WAVEFORM_ZOOM_STEPS);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "view.waveform_zoom_out",
        label: "Reduzir formas de onda (vertical)",
        menu_path: "Exibir/Reduzir formas de onda",
        default_shortcut: Some(Shortcut::shift(ShortcutKey::ArrowDown)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.waveform_zoom_steps = s.opts.waveform_zoom_steps.saturating_sub(1);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "view.thumbs",
        label: "Mostrar miniaturas de vídeo",
        menu_path: "Exibir/Mostrar miniaturas de vídeo",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.show_thumbs = !s.opts.show_thumbs;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "view.safe_areas",
        label: "Áreas de segurança na prévia",
        menu_path: "Exibir/Áreas de segurança na prévia",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.safe_areas = !s.opts.safe_areas;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "view.preview",
        label: "Prévia de Vídeo",
        menu_path: "Exibir/Prévia de Vídeo",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::Num4)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::TogglePreview, h),
    },
    CommandDef {
        id: "view.master_bus",
        label: "Master Bus",
        menu_path: "Exibir/Master Bus",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::Num3)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ToggleMasterBus, h),
    },
    CommandDef {
        id: "view.toggle_dock_area",
        label: "Minimizar/restaurar a área de janelas",
        menu_path: "Exibir/Minimizar ou restaurar a área de janelas",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::F11)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ToggleDockArea, h),
    },
    CommandDef {
        id: "view.toggle_track_list",
        label: "Minimizar/restaurar a lista de trilhas",
        menu_path: "Exibir/Minimizar ou restaurar a lista de trilhas",
        default_shortcut: Some(Shortcut::shift(ShortcutKey::F11)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ToggleTrackList, h),
    },
    CommandDef {
        id: "view.maximize_timeline",
        label: "Maximizar/restaurar a timeline",
        menu_path: "Exibir/Maximizar ou restaurar a timeline",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::F11)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ToggleMaximizeTimeline, h),
    },
    CommandDef {
        id: "preview.split_toggle",
        label: "Prévia: Tela dividida (Antes/Depois)",
        menu_path: "Exibir/Prévia/Tela dividida",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::PreviewToggleSplit, h),
    },
    CommandDef {
        id: "preview.split_bypass",
        label: "Prévia: Comparar sem efeitos (Bypass FX)",
        menu_path: "Exibir/Prévia/Tela dividida: FX ignorados",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| {
            ui_action(
                UiAction::PreviewSplitMode(crate::playback::PreviewSplitMode::FxBypass),
                h,
            )
        },
    },
    CommandDef {
        id: "preview.split_clipboard",
        label: "Prévia: Comparar com área de transferência",
        menu_path: "Exibir/Prévia/Tela dividida: Área de transferência",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| {
            ui_action(
                UiAction::PreviewSplitMode(crate::playback::PreviewSplitMode::Clipboard),
                h,
            )
        },
    },
    CommandDef {
        id: "preview.ram_build",
        label: "Prévia dinâmica em RAM (seleção de tempo)",
        menu_path: "Ferramentas/Prévia dinâmica em RAM",
        default_shortcut: Some(Shortcut::shift(ShortcutKey::B)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::PreviewRamBuild, h),
    },
    CommandDef {
        id: "preview.ram_clear",
        label: "Limpar cache da prévia em RAM",
        menu_path: "Ferramentas/Limpar prévia em RAM",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::PreviewRamClear, h),
    },
    CommandDef {
        id: "view.ui_scale_up",
        label: "Aumentar a escala da interface",
        menu_path: "Exibir/Escala da interface/Aumentar",
        default_shortcut: Some(Shortcut::ctrl_alt(ShortcutKey::Equals)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::UiScale(Some(1)), h),
    },
    CommandDef {
        id: "view.ui_scale_down",
        label: "Diminuir a escala da interface",
        menu_path: "Exibir/Escala da interface/Diminuir",
        default_shortcut: Some(Shortcut::ctrl_alt(ShortcutKey::Minus)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::UiScale(Some(-1)), h),
    },
    CommandDef {
        id: "view.ui_scale_reset",
        label: "Escala da interface em 100%",
        menu_path: "Exibir/Escala da interface/100%",
        default_shortcut: Some(Shortcut::ctrl_alt(ShortcutKey::Num0)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::UiScale(None), h),
    },
    CommandDef {
        id: "preview.zoom_fit",
        label: "Zoom da prévia: Ajustar à janela",
        menu_path: "Exibir/Prévia/Ajustar zoom",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::PreviewZoom(0.0), h),
    },
    CommandDef {
        id: "preview.zoom_100",
        label: "Zoom da prévia: 100%",
        menu_path: "Exibir/Prévia/Zoom 100%",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::PreviewZoom(1.0), h),
    },
    CommandDef {
        id: "preview.zoom_in",
        label: "Ampliar imagem da prévia",
        menu_path: "Exibir/Prévia/Ampliar zoom",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::PreviewZoom(-1.25), h),
    },
    CommandDef {
        id: "preview.zoom_out",
        label: "Reduzir imagem da prévia",
        menu_path: "Exibir/Prévia/Reduzir zoom",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::PreviewZoom(-0.8), h),
    },
    CommandDef {
        id: "preview.zoom_step_in",
        label: "Zoom da prévia: +5%",
        menu_path: "Exibir/Prévia/Zoom +5%",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::PreviewZoomStep(1), h),
    },
    CommandDef {
        id: "preview.zoom_step_out",
        label: "Zoom da prévia: -5%",
        menu_path: "Exibir/Prévia/Zoom -5%",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::PreviewZoomStep(-1), h),
    },
    CommandDef {
        id: "preview.zoom_bypass",
        label: "Zoom da prévia: ignorar zoom",
        menu_path: "Exibir/Prévia/Ignorar zoom",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::PreviewZoomBypass, h),
    },
    CommandDef {
        id: "preview.copy_frame",
        label: "Copiar quadro atual para a área de transferência",
        menu_path: "Editar/Copiar quadro da prévia",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::PreviewCopyFrame, h),
    },
    CommandDef {
        id: "preview.mute_audio",
        label: "Silenciar todo o áudio na prévia",
        menu_path: "Opções/Silenciar áudio da prévia",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.preview_mute_audio = !s.opts.preview_mute_audio;
            s.set_status(if s.opts.preview_mute_audio {
                "Áudio da prévia silenciado."
            } else {
                "Áudio da prévia ativado."
            });
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "preview.mute_video",
        label: "Silenciar todo o vídeo na prévia",
        menu_path: "Opções/Silenciar vídeo da prévia",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.preview_mute_video = !s.opts.preview_mute_video;
            s.set_status(if s.opts.preview_mute_video {
                "Vídeo da prévia silenciado."
            } else {
                "Vídeo da prévia ativado."
            });
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "preview.external_monitor",
        label: "Monitor externo (janela de prévia secundária)",
        menu_path: "Exibir/Monitor externo (Prévia)",
        default_shortcut: Some(Shortcut::alt_shift(ShortcutKey::Num4)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::PreviewToggleExternalMonitor, h),
    },
    // --- Inserir ---
    CommandDef {
        id: "insert.track_video",
        label: "Trilha de vídeo",
        menu_path: "Inserir/Trilha de vídeo",
        default_shortcut: Some(Shortcut::ctrl_shift(ShortcutKey::Q)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.checkpoint();
            s.insert_track(TrackKind::Video);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "insert.track_audio",
        label: "Trilha de áudio",
        menu_path: "Inserir/Trilha de áudio",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::Q)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.checkpoint();
            s.insert_track(TrackKind::Audio);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "insert.track_adjustment",
        label: "Trilha de ajuste",
        menu_path: "Inserir/Trilha de ajuste",
        default_shortcut: Some(Shortcut::new(true, true, true, ShortcutKey::Q)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.checkpoint();
            s.insert_adjustment_track();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "insert.track_folder",
        label: "Pasta de trilhas (Grupo)",
        menu_path: "Inserir/Pasta de trilhas (Grupo)",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            let kind = s
                .sel_track
                .and_then(|i| s.project.tracks.get(i))
                .map(|t| t.kind)
                .unwrap_or(TrackKind::Video);
            s.insert_track_folder(kind);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "insert.nested_timeline",
        label: "Linha do tempo aninhada (.neoveg)...",
        menu_path: "Inserir/Linha do tempo aninhada (.neoveg)...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::InsertNestedTimelineDialog, h),
    },
    CommandDef {
        id: "insert.input_bus",
        label: "Barramento de entrada",
        menu_path: "Inserir/Barramento de entrada",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| s.project.input_buses.len() < crate::model::MAX_INPUT_BUSES,
        run: |s, h| {
            if s.add_input_bus().is_none() {
                return Err(tr(s.lang, "limite de barramentos de entrada atingido").into());
            }
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "insert.assignable_fx",
        label: "FX atribuível",
        menu_path: "Inserir/FX atribuível",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| s.project.audio_buses.len() < crate::model::MAX_AUDIO_BUSES,
        run: |s, h| {
            if s.add_assignable_fx().is_none() {
                return Err(tr(s.lang, "limite de 26 barramentos auxiliares atingido").into());
            }
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "insert.audio_bus",
        label: "Barramento de áudio",
        menu_path: "Inserir/Barramento de áudio",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| s.project.audio_buses.len() < crate::model::MAX_AUDIO_BUSES,
        run: |s, h| {
            if s.add_audio_bus().is_none() {
                return Err(tr(s.lang, "limite de 26 barramentos auxiliares atingido").into());
            }
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "insert.marker",
        label: "Marcador",
        menu_path: "Inserir/Marcador",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::M)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.insert_marker();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "insert.marker_cd_track",
        label: "Marcador de faixa de CD",
        menu_path: "Inserir/Marcador de faixa de CD",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::N)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.insert_marker_kind(MarkerKind::CdTrack, "", "", "");
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "insert.marker_cd_index",
        label: "Marcador de índice de CD",
        menu_path: "Inserir/Marcador de índice de CD",
        default_shortcut: Some(Shortcut::shift(ShortcutKey::N)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.insert_marker_kind(MarkerKind::CdIndex, "", "", "");
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "insert.marker_command",
        label: "Marcador de comando...",
        menu_path: "Inserir/Marcador de comando...",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::C)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenCommandMarkerDialog, h),
    },
    CommandDef {
        id: "insert.region",
        label: "Região",
        menu_path: "Inserir/Região",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::R)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.insert_region();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "insert.gen_text",
        label: "Texto / título...",
        menu_path: "Inserir/Texto / título...",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            h.insert_generated(s, GenSpec::Text(TextSpec::default()))?;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "insert.gen_solid",
        label: "Cor sólida",
        menu_path: "Inserir/Cor sólida",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            h.insert_generated(
                s,
                GenSpec::SolidColor {
                    color: [0.1, 0.2, 0.5, 1.0],
                },
            )?;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "insert.empty_event",
        label: "Evento vazio",
        menu_path: "Inserir/Evento vazio",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.insert_empty_event();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "insert.adjustment_event",
        label: "Evento de ajuste",
        menu_path: "Inserir/Evento de ajuste",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.insert_adjustment_event();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "insert.gen_credit_roll",
        label: "Rolagem de créditos...",
        menu_path: "Inserir/Rolagem de créditos...",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            h.insert_generated(
                s,
                GenSpec::CreditRoll(crate::model::CreditRollSpec::default()),
            )?;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "insert.slideshow",
        label: "Apresentação de slides...",
        menu_path: "Inserir/Apresentação de slides...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenSlideshowDialog, h),
    },
    CommandDef {
        id: "notes.add",
        label: "Adicionar nota do projeto no cursor",
        menu_path: "Inserir/Nota do projeto no cursor",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::AddProjectNoteAtCursor, h),
    },
    CommandDef {
        id: "sequence.new",
        label: "Nova sequência",
        menu_path: "Inserir/Nova sequência",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.checkpoint();
            s.create_sequence(None);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "sequence.duplicate",
        label: "Duplicar sequência ativa",
        menu_path: "Editar/Sequência/Duplicar sequência ativa",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| s.project.active_sequence_id.is_some(),
        run: |s, h| {
            if let Some(id) = s.project.active_sequence_id {
                s.checkpoint();
                s.duplicate_sequence(id);
            }
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "sequence.rename",
        label: "Renomear sequência ativa",
        menu_path: "Editar/Sequência/Renomear sequência ativa",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| s.project.active_sequence_id.is_some(),
        run: |s, h| {
            if let Some(id) = s.project.active_sequence_id {
                let current_name = s
                    .project
                    .sequence_name(id)
                    .unwrap_or_else(|| "Sequência".to_string());
                s.rename_active_sequence(&format!("{current_name} (Renomeada)"));
            }
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "sequence.delete",
        label: "Excluir sequência ativa",
        menu_path: "Editar/Sequência/Excluir sequência ativa",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| !s.project.sequences.is_empty(),
        run: |s, h| {
            if let Some(id) = s.project.active_sequence_id {
                s.checkpoint();
                s.delete_sequence(id);
            }
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "sequence.nest_selection",
        label: "Aninhar seleção em nova sequência",
        menu_path: "Editar/Sequência/Aninhar seleção em nova sequência",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::C)),
        needs_ui: false,
        enabled: |s, _| !s.sel_events.is_empty(),
        run: |s, h| {
            s.nest_selection(None).map_err(|e| e.to_string())?;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "sequence.import_nvegseq",
        label: "Importar sequência (.nvegseq)...",
        menu_path: "Arquivo/Importar sequência (.nvegseq)...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ImportNvegseqDialog, h),
    },
    CommandDef {
        id: "sequence.export_nvegseq",
        label: "Exportar sequência (.nvegseq)...",
        menu_path: "Arquivo/Exportar sequência (.nvegseq)...",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| s.project.active_sequence_id.is_some(),
        run: |s, h| {
            if h.supports_ui() {
                ui_action(UiAction::ExportNvegseqDialog, h)
            } else if let Some(p) = s.path.as_ref() {
                let out_path = p.with_extension("nvegseq");
                crate::package::export_standalone_sequence_file(&s.project, None, &out_path)?;
                Ok(CommandOutcome::Executed)
            } else {
                Err(tr_args(
                    s.lang,
                    "projeto ainda não tem caminho definido para exportar {label} sem diálogo",
                    &[("label", &"NVEGSEQ")],
                ))
            }
        },
    },
    CommandDef {
        id: "insert.env_volume",
        label: "Envelope de Volume / Nível",
        menu_path: "Inserir/Volume / Nível de composição",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::V)),
        needs_ui: false,
        enabled: has_selected_track,
        run: |s, h| {
            s.toggle_envelope_for_selected(EnvKind::Volume, EnvKind::Opacity);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "insert.env_pan",
        label: "Envelope de Pan / Fade para cor",
        menu_path: "Inserir/Pan / Fade para cor",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::P)),
        needs_ui: false,
        enabled: has_selected_track,
        run: |s, h| {
            s.toggle_envelope_for_selected(EnvKind::Pan, EnvKind::FadeToColor);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "insert.remove_env_volume",
        label: "Remover envelope de Volume / Opacidade",
        menu_path: "Inserir/Remover envelope de Volume",
        default_shortcut: Some(Shortcut::shift(ShortcutKey::V)),
        needs_ui: false,
        enabled: has_selected_track,
        run: |s, h| {
            s.remove_envelope(EnvKind::Volume);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "insert.remove_env_pan",
        label: "Remover envelope de Pan / Fade",
        menu_path: "Inserir/Remover envelope de Pan",
        default_shortcut: Some(Shortcut::shift(ShortcutKey::P)),
        needs_ui: false,
        enabled: has_selected_track,
        run: |s, h| {
            s.remove_envelope(EnvKind::Pan);
            ok_mutated(s, h)
        },
    },
    // --- Trilha ---
    CommandDef {
        id: "track.mute",
        label: "Mudo na trilha selecionada",
        menu_path: "Trilha/Mudo",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::Z)),
        needs_ui: false,
        enabled: has_selected_track,
        run: |s, h| {
            s.toggle_selected_track_mute();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "track.solo",
        label: "Solo na trilha selecionada",
        menu_path: "Trilha/Solo",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::X)),
        needs_ui: false,
        enabled: has_selected_track,
        run: |s, h| {
            s.toggle_selected_track_solo();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "track.record_arm",
        label: "Armar trilha para gravação (Rec)",
        menu_path: "Trilha/Armar gravação",
        default_shortcut: Some(Shortcut::ctrl_alt(ShortcutKey::R)),
        needs_ui: false,
        enabled: has_selected_audio_track,
        run: |s, h| {
            if let Some(ti) = s.sel_track {
                s.toggle_record_arm(ti);
            }
            ok_mutated(s, h)
        },
    },
    // --- Ferramentas ---
    CommandDef {
        id: "tools.fx_master_audio",
        label: "Efeitos do master bus (áudio)...",
        menu_path: "Ferramentas/Efeitos do master bus (áudio)...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenFxMasterAudio, h),
    },
    CommandDef {
        id: "tools.fx_master_video",
        label: "Efeitos de saída de vídeo...",
        menu_path: "Ferramentas/Efeitos de saída de vídeo...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenFxMasterVideo, h),
    },
    CommandDef {
        id: "tools.event_props",
        label: "Propriedades do evento...",
        menu_path: "Ferramentas/Propriedades do evento...",
        default_shortcut: None,
        needs_ui: true,
        enabled: has_selected_events_ui,
        run: |_s, h| ui_action(UiAction::OpenEventProps, h),
    },
    CommandDef {
        id: "tools.pan_crop",
        label: "Pan/Crop do evento...",
        menu_path: "Ferramentas/Pan/Crop do evento...",
        default_shortcut: None,
        needs_ui: true,
        enabled: has_selected_events_ui,
        run: |_s, h| ui_action(UiAction::OpenPanCrop, h),
    },
    CommandDef {
        id: "tools.fx_event",
        label: "Efeitos do evento...",
        menu_path: "Ferramentas/Efeitos do evento...",
        default_shortcut: None,
        needs_ui: true,
        enabled: has_selected_events_ui,
        run: |_s, h| ui_action(UiAction::OpenFxEvent, h),
    },
    CommandDef {
        id: "tools.reverse",
        label: "Inverter evento (reverse)",
        menu_path: "Ferramentas/Inverter evento (reverse)",
        default_shortcut: None,
        needs_ui: false,
        enabled: has_selected_events,
        run: |s, h| {
            s.toggle_reverse();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "tools.freeze_frame",
        label: "Congelar quadro no cursor",
        menu_path: "Ferramentas/Congelar quadro no cursor",
        default_shortcut: None,
        needs_ui: false,
        enabled: has_selected_events,
        run: |s, h| {
            s.freeze_frame();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "event.toggle_velocity_envelope",
        label: "Inserir/Remover envelope de velocidade",
        menu_path: "Inserir/Envelopes de evento/Velocidade",
        default_shortcut: None,
        needs_ui: false,
        enabled: has_selected_events,
        run: |s, h| {
            s.toggle_velocity_envelope_selected();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "event.repair_sync_move",
        label: "Sincronizar áudio e vídeo movendo evento",
        menu_path: "Editar/Sincronizar áudio e vídeo movendo evento",
        default_shortcut: None,
        needs_ui: false,
        enabled: has_selected_events,
        run: |s, h| {
            let ids: Vec<crate::model::Id> = s.sel_events.iter().copied().collect();
            let mut repaired = 0usize;
            for id in ids {
                if crate::engine::audio_tools::repair_sync_move(s, id) {
                    repaired += 1;
                }
            }
            if repaired == 0 {
                s.set_status(neoveg_common::tr(
                    s.lang,
                    "Nenhum evento agrupado dessincronizado na seleção.",
                ));
            }
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "event.repair_sync_slip",
        label: "Sincronizar áudio e vídeo deslizando conteúdo (Slip)",
        menu_path: "Editar/Sincronizar áudio e vídeo deslizando conteúdo (Slip)",
        default_shortcut: None,
        needs_ui: false,
        enabled: has_selected_events,
        run: |s, h| {
            let ids: Vec<crate::model::Id> = s.sel_events.iter().copied().collect();
            let mut repaired = 0usize;
            for id in ids {
                if crate::engine::audio_tools::repair_sync_slip(s, id) {
                    repaired += 1;
                }
            }
            if repaired == 0 {
                s.set_status(neoveg_common::tr(
                    s.lang,
                    "Nenhum evento agrupado dessincronizado na seleção.",
                ));
            }
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "tools.auto_normalize",
        label: "Normalizar áudio automaticamente",
        menu_path: "Ferramentas/Áudio/Normalizar áudio automaticamente",
        default_shortcut: None,
        needs_ui: true,
        enabled: has_selected_events_ui,
        run: |_s, h| ui_action(UiAction::AutoNormalizeSelected, h),
    },
    CommandDef {
        id: "tools.auto_ducking",
        label: "Auto Ducking (redução automática)...",
        menu_path: "Ferramentas/Áudio/Auto Ducking (redução automática)...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenAutoDuckingDialog, h),
    },
    CommandDef {
        id: "tools.beat_detection",
        label: "Detectar batidas e andamento (BPM)...",
        menu_path: "Ferramentas/Áudio/Detectar batidas e andamento (BPM)...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenBeatDetectionDialog, h),
    },
    CommandDef {
        id: "tools.build_proxy",
        label: "Criar proxy de vídeo para selecionados",
        menu_path: "Ferramentas/Criar proxy de vídeo para selecionados",
        default_shortcut: None,
        needs_ui: true,
        enabled: has_selected_proxyable_video_events_ui,
        run: |_s, h| ui_action(UiAction::BuildProxySelected, h),
    },
    CommandDef {
        id: "tools.prerender",
        label: "Pré-renderizar seletivamente região...",
        menu_path: "Ferramentas/Pré-renderizar seletivamente região...",
        default_shortcut: Some(Shortcut::shift(ShortcutKey::M)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::PrerenderRegion, h),
    },
    CommandDef {
        id: "tools.encoder_start",
        label: "Encoder: Iniciar / Retomar fila",
        menu_path: "Ferramentas/Neoveg Encoder/Iniciar fila",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::EncoderStartQueue, h),
    },
    CommandDef {
        id: "tools.encoder_pause",
        label: "Encoder: Pausar / Retomar",
        menu_path: "Ferramentas/Neoveg Encoder/Pausar fila",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::EncoderPauseQueue, h),
    },
    CommandDef {
        id: "tools.encoder_stop_current",
        label: "Encoder: Interromper item atual",
        menu_path: "Ferramentas/Neoveg Encoder/Interromper arquivo atual",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::EncoderStopCurrent, h),
    },
    CommandDef {
        id: "tools.encoder_stop_queue",
        label: "Encoder: Interromper fila",
        menu_path: "Ferramentas/Neoveg Encoder/Interromper fila",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::EncoderStopQueue, h),
    },
    CommandDef {
        id: "tools.capture_toggle_record",
        label: "Capture: Iniciar / Parar gravação",
        menu_path: "Ferramentas/Neoveg Capture/Iniciar ou parar gravação",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::CaptureToggleRecord, h),
    },
    CommandDef {
        id: "tools.capture_export",
        label: "Capture: Exportar para a timeline",
        menu_path: "Ferramentas/Neoveg Capture/Exportar para a timeline",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::CaptureExportToProject, h),
    },
    CommandDef {
        id: "tools.run_script",
        label: "Executar script (.cs/.dll)...",
        menu_path: "Ferramentas/Executar script (.cs/.dll)...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::RunScriptDialog, h),
    },
    CommandDef {
        id: "tools.integrations",
        label: "Integrações (ArtCraft & Blender)...",
        menu_path: "Ferramentas/Integrações (ArtCraft & Blender)...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenIntegrationsDialog, h),
    },
    CommandDef {
        id: "tools.send_photocraft",
        label: "Editar no PhotoCraft...",
        menu_path: "Ferramentas/Aplicativos externos/Editar no PhotoCraft...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| {
            ui_action(
                UiAction::SendToExternalApp(crate::integrations::ExternalApp::PhotoCraft),
                h,
            )
        },
    },
    CommandDef {
        id: "tools.send_vectorcraft",
        label: "Editar vetor no VectorCraft...",
        menu_path: "Ferramentas/Aplicativos externos/Editar vetor no VectorCraft...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| {
            ui_action(
                UiAction::SendToExternalApp(crate::integrations::ExternalApp::VectorCraft),
                h,
            )
        },
    },
    CommandDef {
        id: "tools.send_soundcraft",
        label: "Editar áudio no SoundCraft (BWF)...",
        menu_path: "Ferramentas/Aplicativos externos/Editar áudio no SoundCraft (BWF)...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| {
            ui_action(
                UiAction::SendToExternalApp(crate::integrations::ExternalApp::SoundCraft),
                h,
            )
        },
    },
    CommandDef {
        id: "tools.send_effectcraft",
        label: "Enviar para composição no EffectCraft...",
        menu_path: "Ferramentas/Aplicativos externos/Enviar para EffectCraft...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| {
            ui_action(
                UiAction::SendToExternalApp(crate::integrations::ExternalApp::EffectCraft),
                h,
            )
        },
    },
    CommandDef {
        id: "tools.send_anicraft",
        label: "Abrir/Criar animação no AniCraft...",
        menu_path: "Ferramentas/Aplicativos externos/Abrir no AniCraft...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| {
            ui_action(
                UiAction::SendToExternalApp(crate::integrations::ExternalApp::AniCraft),
                h,
            )
        },
    },
    CommandDef {
        id: "tools.send_filmcraft",
        label: "Exportar timeline para FilmCraft (OTIO)...",
        menu_path: "Ferramentas/Aplicativos externos/Exportar timeline para FilmCraft (OTIO)...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| {
            ui_action(
                UiAction::SendToExternalApp(crate::integrations::ExternalApp::FilmCraft),
                h,
            )
        },
    },
    CommandDef {
        id: "tools.blender_render",
        label: "Renderizar cena do Blender para Timeline...",
        menu_path: "Ferramentas/Aplicativos externos/Blender: Renderizar cena (.blend)...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::BlenderRenderSceneDialog, h),
    },
    CommandDef {
        id: "tools.blender_export_script",
        label: "Exportar marcadores e câmera para Blender (.py)...",
        menu_path: "Ferramentas/Aplicativos externos/Blender: Exportar script (.py)...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::BlenderExportScriptDialog, h),
    },
    CommandDef {
        id: "tools.blender_import_camera",
        label: "Importar câmera do Blender (.json)...",
        menu_path: "Ferramentas/Aplicativos externos/Blender: Importar câmera (.json)...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::BlenderImportCameraDialog, h),
    },
    CommandDef {
        id: "tools.remove_unused_media",
        label: "Remover mídias não utilizadas",
        menu_path: "Ferramentas/Remover mídias não utilizadas",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            let cache = h.media_cache();
            let removed = s.remove_unused_media(cache);
            h.on_media_removed(&removed);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "track.toggle_folder",
        label: "Recolher / expandir pasta da trilha",
        menu_path: "Editar/Trilha/Recolher ou expandir pasta",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| {
            s.sel_track
                .and_then(|i| s.project.tracks.get(i))
                .is_some_and(|t| {
                    t.is_folder
                        || t.parent_track_id.is_some_and(|pid| {
                            s.project.tracks.iter().any(|p| p.id == pid && p.is_folder)
                        })
                })
        },
        run: |s, h| {
            if let Some(ti) = s.sel_track {
                s.toggle_track_folder(ti);
            }
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "track.make_child",
        label: "Tornar filho de composição",
        menu_path: "Editar/Trilha/Tornar filho de composição",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| {
            s.sel_track
                .is_some_and(|i| i > 0 && i < s.project.tracks.len())
        },
        run: |s, h| {
            if let Some(ti) = s.sel_track {
                s.make_compositing_child(ti).map_err(|e| e.to_string())?;
            }
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "track.unmake_child",
        label: "Promover filho para trilha raiz",
        menu_path: "Editar/Trilha/Promover filho para trilha raiz",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| {
            s.sel_track
                .and_then(|i| s.project.tracks.get(i))
                .is_some_and(|t| t.parent_track_id.is_some())
        },
        run: |s, h| {
            if let Some(ti) = s.sel_track {
                s.unmake_compositing_child(ti);
            }
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "track.mask_cycle",
        label: "Alternar modo de máscara da trilha",
        menu_path: "Editar/Trilha/Alternar modo de máscara",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| {
            s.sel_track
                .and_then(|i| s.project.tracks.get(i))
                .is_some_and(|t| t.kind == TrackKind::Video && !t.is_folder)
        },
        run: |s, h| {
            if let Some(ti) = s.sel_track {
                let modes = crate::model::TrackMaskMode::ALL;
                let cur = s.project.tracks[ti].mask_mode;
                let next_i = modes
                    .iter()
                    .position(|m| *m == cur)
                    .map(|p| (p + 1) % modes.len())
                    .unwrap_or(0);
                s.set_track_mask_mode(ti, modes[next_i]);
                s.set_status(format!("Máscara da trilha: {}", modes[next_i].label()));
            }
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "track.set_defaults",
        label: "Definir propriedades padrão da trilha",
        menu_path: "Ferramentas/Definir propriedades padrão da trilha",
        default_shortcut: None,
        needs_ui: true,
        enabled: |s, h| h.supports_ui() && s.sel_track.is_some(),
        run: |_s, h| ui_action(UiAction::SetTrackDefaultProps, h),
    },
    CommandDef {
        id: "track.render_to_new_track",
        label: "Renderizar para nova trilha...",
        menu_path: "Ferramentas/Renderizar para nova trilha...",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::M)),
        needs_ui: true,
        enabled: |s, h| {
            h.supports_ui()
                && (s.time_sel.is_some_and(|(a, b)| b > a) || s.project.duration() > Ticks::ZERO)
        },
        run: |_s, h| ui_action(UiAction::RenderToNewTrack, h),
    },
    CommandDef {
        id: "tools.multicam_create",
        label: "Multicâmera: Criar trilha multicâmera",
        menu_path: "Ferramentas/Multicâmera/Criar trilha multicâmera",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| {
            s.project
                .tracks
                .iter()
                .filter(|t| {
                    t.kind == TrackKind::Video
                        && !t.is_folder
                        && !t.is_adjustment()
                        && !t.events.is_empty()
                })
                .count()
                >= 2
        },
        run: |s, h| {
            s.create_multicam_track().map_err(|e| e.to_string())?;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "tools.multicam_mode",
        label: "Multicâmera: Ativar edição multicâmera",
        menu_path: "Ferramentas/Multicâmera/Ativar edição multicâmera",
        default_shortcut: Some(Shortcut::ctrl_shift(ShortcutKey::D)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.multicam_mode = !s.opts.multicam_mode;
            s.set_status(if s.opts.multicam_mode {
                "Edição Multicâmera ativada (clique no ângulo ou pressione 1..9)."
            } else {
                "Edição Multicâmera desativada."
            });
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "tools.multicam_cut_1",
        label: "Multicâmera: Cortar para Câmera 1",
        menu_path: "Ferramentas/Multicâmera/Cortar para Câmera 1",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| s.active_multicam_event_at(s.cursor).is_some(),
        run: |s, h| {
            s.switch_multicam_angle(s.cursor, 0);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "tools.multicam_cut_2",
        label: "Multicâmera: Cortar para Câmera 2",
        menu_path: "Ferramentas/Multicâmera/Cortar para Câmera 2",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| s.active_multicam_event_at(s.cursor).is_some(),
        run: |s, h| {
            s.switch_multicam_angle(s.cursor, 1);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "tools.multicam_cut_3",
        label: "Multicâmera: Cortar para Câmera 3",
        menu_path: "Ferramentas/Multicâmera/Cortar para Câmera 3",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| s.active_multicam_event_at(s.cursor).is_some(),
        run: |s, h| {
            s.switch_multicam_angle(s.cursor, 2);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "tools.multicam_cut_4",
        label: "Multicâmera: Cortar para Câmera 4",
        menu_path: "Ferramentas/Multicâmera/Cortar para Câmera 4",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| s.active_multicam_event_at(s.cursor).is_some(),
        run: |s, h| {
            s.switch_multicam_angle(s.cursor, 3);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "view.stereo_3d",
        label: "Prévia: Alternar modo 3D Estereoscópico",
        menu_path: "Exibir/Prévia/Alternar modo 3D Estereoscópico",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::CycleStereo3dMode, h),
    },
    CommandDef {
        id: "insert.subtitles_from_regions",
        label: "Legendas a partir das regiões",
        menu_path: "Inserir/Legendas a partir das regiões",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| s.project.regions.iter().any(|r| r.end > r.start),
        run: |s, h| {
            s.insert_subtitles_from_regions();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "tools.motion_track",
        label: "Rastrear movimento (Motion Tracking)...",
        menu_path: "Ferramentas/Rastrear movimento...",
        default_shortcut: None,
        needs_ui: true,
        enabled: has_selected_video_events_ui,
        run: |_s, h| ui_action(UiAction::OpenMotionTrackDialog, h),
    },
    CommandDef {
        id: "tools.apply_audio_fx",
        label: "Aplicar efeitos de áudio fora de tempo real",
        menu_path: "Ferramentas/Aplicar efeitos de áudio fora de tempo real",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ApplyAudioFxNonRealtime, h),
    },
    CommandDef {
        id: "options.bypass_all_audio_fx",
        label: "Ignorar todos os efeitos de áudio",
        menu_path: "Opções/Ignorar todos os efeitos de áudio",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.bypass_all_audio_fx = !s.opts.bypass_all_audio_fx;
            s.set_status(if s.opts.bypass_all_audio_fx {
                "Todos os efeitos de áudio ignorados globalmente (Bypass)."
            } else {
                "Efeitos de áudio reativados."
            });
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "media.view.thumbnails",
        label: "Mídia do Projeto: Miniaturas",
        menu_path: "Exibir/Mídia do Projeto/Miniaturas",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ProjectMediaSetView(0), h),
    },
    CommandDef {
        id: "media.view.list",
        label: "Mídia do Projeto: Lista",
        menu_path: "Exibir/Mídia do Projeto/Lista",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ProjectMediaSetView(1), h),
    },
    CommandDef {
        id: "media.view.details",
        label: "Mídia do Projeto: Detalhes",
        menu_path: "Exibir/Mídia do Projeto/Detalhes",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ProjectMediaSetView(2), h),
    },
    CommandDef {
        id: "media.view.storyboard",
        label: "Mídia do Projeto: Storyboard",
        menu_path: "Exibir/Mídia do Projeto/Storyboard",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ProjectMediaSetView(3), h),
    },
    CommandDef {
        id: "media.new_bin",
        label: "Nova pasta de mídia (Bin)",
        menu_path: "Projeto/Nova pasta de mídia (Bin)",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ProjectMediaNewBin, h),
    },
    CommandDef {
        id: "media.new_smart_bin",
        label: "Nova pasta inteligente...",
        menu_path: "Projeto/Nova pasta inteligente...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ProjectMediaNewSmartBin, h),
    },
    CommandDef {
        id: "media.new_storyboard",
        label: "Novo storyboard",
        menu_path: "Projeto/Novo storyboard",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ProjectMediaNewStoryboard, h),
    },
    CommandDef {
        id: "media.select_events",
        label: "Selecionar eventos desta mídia",
        menu_path: "Editar/Selecionar eventos da mídia",
        default_shortcut: None,
        needs_ui: false,
        enabled: has_active_media,
        run: |s, h| {
            if let Some(mid) = s.active_media_id {
                s.select_events_using_media(mid);
            }
            h.after_mutation(s);
            Ok(CommandOutcome::Executed)
        },
    },
    CommandDef {
        id: "media.replace",
        label: "Substituir mídia selecionada...",
        menu_path: "Projeto/Substituir mídia selecionada...",
        default_shortcut: None,
        needs_ui: true,
        enabled: has_active_media_ui,
        run: |_s, h| ui_action(UiAction::ProjectMediaReplaceDialog, h),
    },
    CommandDef {
        id: "media.insert_storyboard",
        label: "Inserir storyboard na timeline",
        menu_path: "Inserir/Storyboard na timeline",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| {
            s.active_storyboard_id.is_some_and(|id| {
                s.project
                    .storyboards
                    .iter()
                    .any(|sb| sb.id == id && !sb.items.is_empty())
            })
        },
        run: |s, h| {
            if let Some(sb_id) = s.active_storyboard_id {
                let c = s.cursor;
                s.insert_storyboard_at_cursor(sb_id, c, 5.0);
            }
            ok_mutated(s, h)
        },
    },
    // --- Opções ---
    CommandDef {
        id: "options.snapping",
        label: "Ativar encaixe (snapping)",
        menu_path: "Opções/Ativar encaixe (snapping)",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::F8)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.snapping = !s.opts.snapping;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "options.auto_crossfades",
        label: "Crossfades automáticos",
        menu_path: "Opções/Crossfades automáticos",
        default_shortcut: Some(Shortcut::ctrl_shift(ShortcutKey::X)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.auto_crossfades = !s.opts.auto_crossfades;
            let msg = if s.opts.auto_crossfades {
                tr(s.lang, "Crossfades automáticos ativados.")
            } else {
                tr(s.lang, "Crossfades automáticos desativados.")
            };
            s.set_status(msg);
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "options.snap_to_grid",
        label: "Encaixar na grade",
        menu_path: "Opções/Encaixar na grade",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::F8)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.snap_to_grid = !s.opts.snap_to_grid;
            s.set_status(if s.opts.snap_to_grid {
                "Encaixe na grade da régua ativado."
            } else {
                "Encaixe na grade da régua desativado."
            });
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "options.grid_cycle",
        label: "Grade da régua: próximo espaçamento",
        menu_path: "Opções/Grade da régua/Próximo espaçamento",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.cycle_ruler_grid();
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "track.cycle_automation",
        label: "Alternar modo de automação da trilha",
        menu_path: "Trilha/Automação (Off / Read / Touch / Latch)",
        default_shortcut: None,
        needs_ui: false,
        enabled: |s, _| s.sel_track.is_some(),
        run: |s, h| {
            if let Some(ti) = s.sel_track {
                s.cycle_track_automation_mode(ti);
            }
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "options.customize_toolbar",
        label: "Personalizar barra de ferramentas...",
        menu_path: "Opções/Personalizar barra de ferramentas...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenCustomizeToolbar, h),
    },
    CommandDef {
        id: "options.quantize",
        label: "Quantizar para quadros",
        menu_path: "Opções/Quantizar para quadros",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::F8)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.quantize = !s.opts.quantize;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "options.lock_envelopes",
        label: "Travar envelopes nos eventos",
        menu_path: "Opções/Travar envelopes nos eventos",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.lock_envelopes = !s.opts.lock_envelopes;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "options.auto_ripple",
        label: "Ripple automático",
        menu_path: "Opções/Ripple automático",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::L)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.auto_ripple = !s.opts.auto_ripple;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "options.ignore_grouping",
        label: "Ignorar agrupamento de eventos",
        menu_path: "Opções/Ignorar agrupamento de eventos",
        default_shortcut: Some(Shortcut::ctrl_shift(ShortcutKey::U)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.ignore_grouping = !s.opts.ignore_grouping;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "options.slip_all_takes",
        label: "Slip em todos os takes",
        menu_path: "Opções/Slip em todos os takes",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.slip_all_takes = !s.opts.slip_all_takes;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "options.loop_playback",
        label: "Reprodução em loop",
        menu_path: "Opções/Reprodução em loop",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::Q)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.loop_playback = !s.opts.loop_playback;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "options.prefs",
        label: "Preferências...",
        menu_path: "Opções/Preferências...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenPrefs, h),
    },
    // --- Transporte e Navegação ---
    CommandDef {
        id: "transport.play_toggle",
        label: "Reproduzir / Pausar",
        menu_path: "Transporte/Reproduzir / Pausar",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::Space)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::TogglePlay, h),
    },
    CommandDef {
        id: "transport.play_from_start",
        label: "Reproduzir do início",
        menu_path: "Transporte/Reproduzir do início",
        default_shortcut: Some(Shortcut::shift(ShortcutKey::Space)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::PlayFromStart, h),
    },
    CommandDef {
        id: "transport.play_stop_restore",
        label: "Reproduzir / Parar restaurando cursor",
        menu_path: "Transporte/Parar restaurando cursor",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::Enter)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| {
            if h.is_playing() {
                ui_action(
                    UiAction::Stop {
                        restore_cursor: true,
                    },
                    h,
                )
            } else {
                ui_action(UiAction::Play, h)
            }
        },
    },
    CommandDef {
        id: "transport.stop",
        label: "Parar reprodução",
        menu_path: "Transporte/Parar",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::Escape)),
        needs_ui: true,
        enabled: |_, h| h.supports_ui() && h.is_playing(),
        run: |_s, h| {
            ui_action(
                UiAction::Stop {
                    restore_cursor: false,
                },
                h,
            )
        },
    },
    CommandDef {
        id: "transport.record",
        label: "Gravar áudio nas trilhas armadas",
        menu_path: "Transporte/Gravar",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::R)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ToggleRecord, h),
    },
    CommandDef {
        id: "transport.shuttle_rev",
        label: "Scrub reverso (J)",
        menu_path: "Transporte/Scrub reverso",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::J)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| {
            let cur = h.shuttle_speed();
            let s = if cur < 0.0 {
                (cur * 2.0).max(-8.0)
            } else {
                -1.0
            };
            ui_action(UiAction::Shuttle(s), h)
        },
    },
    CommandDef {
        id: "transport.shuttle_pause",
        label: "Pausar scrub (K)",
        menu_path: "Transporte/Pausar scrub",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::K)),
        needs_ui: true,
        enabled: |_, h| h.supports_ui() && h.is_playing(),
        run: |_s, h| {
            ui_action(
                UiAction::Stop {
                    restore_cursor: true,
                },
                h,
            )
        },
    },
    CommandDef {
        id: "transport.shuttle_fwd",
        label: "Scrub avanço (L)",
        menu_path: "Transporte/Scrub avanço",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::L)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| {
            let cur = h.shuttle_speed();
            let s = if cur > 0.0 && h.is_playing() {
                (cur * 2.0).min(8.0)
            } else {
                1.0
            };
            if s == 1.0 {
                ui_action(UiAction::Play, h)
            } else {
                ui_action(UiAction::Shuttle(s), h)
            }
        },
    },
    CommandDef {
        id: "transport.frame_back",
        label: "Voltar 1 quadro",
        menu_path: "Transporte/Voltar 1 quadro",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::ArrowLeft)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            let fd = s.project.settings.frame_dur();
            let t = s
                .project
                .settings
                .quantize((s.cursor - fd).max(Ticks::ZERO));
            if h.supports_ui() {
                ui_action(UiAction::SetCursor(t), h)
            } else {
                s.cursor = t;
                ok_mutated(s, h)
            }
        },
    },
    CommandDef {
        id: "transport.frame_fwd",
        label: "Avançar 1 quadro",
        menu_path: "Transporte/Avançar 1 quadro",
        default_shortcut: Some(Shortcut::alt(ShortcutKey::ArrowRight)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            let fd = s.project.settings.frame_dur();
            let t = s.project.settings.quantize(s.cursor + fd);
            if h.supports_ui() {
                ui_action(UiAction::SetCursor(t), h)
            } else {
                s.cursor = t;
                ok_mutated(s, h)
            }
        },
    },
    CommandDef {
        id: "transport.jump_prev_marker",
        label: "Marcador/corte anterior",
        menu_path: "Transporte/Marcador anterior",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::ArrowLeft)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            if let Some(t) = s.jump_marker_time(false) {
                if h.supports_ui() {
                    return ui_action(UiAction::SetCursor(t), h);
                }
                s.cursor = t;
            }
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "transport.jump_next_marker",
        label: "Próximo marcador/corte",
        menu_path: "Transporte/Próximo marcador",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::ArrowRight)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            if let Some(t) = s.jump_marker_time(true) {
                if h.supports_ui() {
                    return ui_action(UiAction::SetCursor(t), h);
                }
                s.cursor = t;
            }
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "transport.project_start",
        label: "Ir para o início do projeto",
        menu_path: "Transporte/Início do projeto",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::Home)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            if h.supports_ui() {
                ui_action(UiAction::SetCursor(Ticks::ZERO), h)
            } else {
                s.cursor = Ticks::ZERO;
                ok_mutated(s, h)
            }
        },
    },
    CommandDef {
        id: "transport.project_end",
        label: "Ir para o fim do projeto",
        menu_path: "Transporte/Fim do projeto",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::End)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            let d = s.project.duration();
            if h.supports_ui() {
                ui_action(UiAction::SetCursor(d), h)
            } else {
                s.cursor = d;
                ok_mutated(s, h)
            }
        },
    },
    CommandDef {
        id: "transport.sel_start",
        label: "Ir para o início da seleção",
        menu_path: "Transporte/Início da seleção",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::Home)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            let t = s.time_sel.map(|sel| sel.0).unwrap_or(Ticks::ZERO);
            if h.supports_ui() {
                ui_action(UiAction::SetCursor(t), h)
            } else {
                s.cursor = t;
                ok_mutated(s, h)
            }
        },
    },
    CommandDef {
        id: "transport.rewind_w",
        label: "Ir para o início do projeto (W)",
        menu_path: "Transporte/Ir para início (W)",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::W)),
        needs_ui: false,
        enabled: always_enabled,
        // lista do VEGAS Pro: "Go to beginning of project: Ctrl+Home or W"
        run: |s, h| move_cursor(s, h, Ticks::ZERO),
    },
    CommandDef {
        id: "transport.sel_end",
        label: "Ir para o fim da seleção",
        menu_path: "Transporte/Fim da seleção",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::End)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            let t = s.time_sel.map(|sel| sel.1).unwrap_or(s.project.duration());
            if h.supports_ui() {
                ui_action(UiAction::SetCursor(t), h)
            } else {
                s.cursor = t;
                ok_mutated(s, h)
            }
        },
    },
    CommandDef {
        id: "transport.center_cursor",
        label: "Centralizar timeline no cursor",
        menu_path: "Exibir/Centralizar no cursor",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::Backslash)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::CenterTimeline, h),
    },
    CommandDef {
        id: "timeline.zoom_in",
        label: "Aproximar zoom da timeline",
        menu_path: "Exibir/Zoom +",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::ArrowUp)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ZoomBy(1.25), h),
    },
    CommandDef {
        id: "timeline.zoom_out",
        label: "Afastar zoom da timeline",
        menu_path: "Exibir/Zoom -",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::ArrowDown)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ZoomBy(0.8), h),
    },
    CommandDef {
        id: "timeline.zoom_in_fast",
        label: "Aproximar zoom (2x)",
        menu_path: "Exibir/Zoom + (2x)",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::ArrowUp)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ZoomBy(2.0), h),
    },
    CommandDef {
        id: "timeline.zoom_out_fast",
        label: "Afastar zoom (2x)",
        menu_path: "Exibir/Zoom - (2x)",
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::ArrowDown)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ZoomBy(0.5), h),
    },
    // --- Ajuda ---
    CommandDef {
        id: "help.welcome",
        label: "Tela de boas-vindas",
        menu_path: "Ajuda/Tela de boas-vindas",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenWelcome, h),
    },
    CommandDef {
        id: "help.splash",
        label: "Tela de abertura",
        menu_path: "Ajuda/Tela de abertura",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenSplash, h),
    },
    CommandDef {
        id: "help.shortcuts",
        label: "Atalhos de teclado",
        menu_path: "Ajuda/Atalhos de teclado",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenShortcuts, h),
    },
    CommandDef {
        id: "help.about",
        label: "Sobre o Neoveg",
        menu_path: "Ajuda/Sobre o Neoveg",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenAbout, h),
    },
];

pub fn find_command(id: &str) -> Option<&'static CommandDef> {
    COMMANDS.iter().find(|c| c.id == id)
}

pub fn is_command_enabled(id: &str, session: &EditorSession, host: &dyn CommandHost) -> bool {
    find_command(id)
        .map(|c| (c.enabled)(session, host))
        .unwrap_or(false)
}

/// Despacha uma ação exclusivamente pelo seu `id` na Tabela Única de Comandos.
pub fn run_command(
    id: &str,
    session: &mut EditorSession,
    host: &mut dyn CommandHost,
) -> Result<CommandOutcome, String> {
    let cmd = find_command(id)
        .ok_or_else(|| tr_args(session.lang, "comando desconhecido: '{id}'", &[("id", &id)]))?;
    // a seleção de tempo de antes do comando entra no histórico do Backspace
    session.note_time_selection();
    if !(cmd.enabled)(session, host) {
        return Err(tr_args(
            session.lang,
            "comando '{id}' não está habilitado no momento",
            &[("id", &cmd.id)],
        ));
    }
    (cmd.run)(session, host)
}

/// Resolve o atalho efetivo de um comando considerando regravações das preferências.
pub fn resolved_shortcut(
    cmd: &CommandDef,
    overrides: &HashMap<String, Option<String>>,
) -> Option<Shortcut> {
    match overrides.get(cmd.id) {
        Some(Some(spec)) => Shortcut::parse(spec).or(cmd.default_shortcut),
        Some(None) => None,
        None => cmd.default_shortcut,
    }
}

pub fn shortcut_label_in(
    id: &str,
    overrides: &HashMap<String, Option<String>>,
    lang: Language,
) -> Option<String> {
    let cmd = find_command(id)?;
    resolved_shortcut(cmd, overrides).map(|sc| sc.format_in(lang))
}

pub fn shortcut_label(id: &str, overrides: &HashMap<String, Option<String>>) -> Option<String> {
    shortcut_label_in(id, overrides, Language::PtBr)
}

/// Rótulo formatado para item de menu com atalho à direita (se configurado).
pub fn menu_label_in(
    id: &str,
    overrides: &HashMap<String, Option<String>>,
    lang: Language,
) -> String {
    let Some(cmd) = find_command(id) else {
        return id.to_string();
    };
    let lbl = cmd.label_in(lang);
    match resolved_shortcut(cmd, overrides) {
        Some(sc) => format!("{lbl}   {}", sc.format_in(lang)),
        None => lbl.to_string(),
    }
}

/// Rótulo formatado em português (compatibilidade com testes legados).
pub fn menu_label(id: &str, overrides: &HashMap<String, Option<String>>) -> String {
    menu_label_in(id, overrides, Language::PtBr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use std::collections::HashSet;

    /// D e Shift+D percorrem as ferramentas; trocar de ferramenta ou de
    /// modo não cria passo de Desfazer.
    #[test]
    fn ferramentas_de_edicao_alternam_sem_desfazer() {
        use crate::session::{EditMode, EditTool};
        let mut s = EditorSession::new(Project::new_default());
        let mut h = HeadlessHost;
        assert_eq!(s.edit_tool, EditTool::Normal);
        for want in [
            EditTool::Envelope,
            EditTool::Selection,
            EditTool::Zoom,
            EditTool::Hand,
            EditTool::Normal,
        ] {
            run_command("edit.tool.next", &mut s, &mut h).unwrap();
            assert_eq!(s.edit_tool, want);
        }
        run_command("edit.tool.prev", &mut s, &mut h).unwrap();
        assert_eq!(s.edit_tool, EditTool::Hand);
        run_command("edit.tool.zoom", &mut s, &mut h).unwrap();
        run_command("edit.tool.mode.slip", &mut s, &mut h).unwrap();
        assert_eq!(
            (s.edit_tool, s.edit_mode),
            (EditTool::Normal, EditMode::Slip)
        );
        run_command("edit.tool.selection", &mut s, &mut h).unwrap();
        run_command("edit.tool.mode.normal", &mut s, &mut h).unwrap();
        assert_eq!(
            (s.edit_tool, s.edit_mode),
            (EditTool::Normal, EditMode::Normal)
        );
        for mode in EditMode::NORMAL_MODES {
            run_command(mode.command_id(), &mut s, &mut h).unwrap();
            assert_eq!((s.edit_tool, s.edit_mode), (EditTool::Normal, mode));
        }
        run_command("edit.tool.mode.normal", &mut s, &mut h).unwrap();
        run_command("edit.tool.mode.next", &mut s, &mut h).unwrap();
        assert_eq!(s.edit_mode, EditMode::Shuffle);
        assert!(s.undo.is_empty(), "ferramenta não entra no Desfazer");
    }

    #[test]
    fn ids_da_tabela_sao_unicos() {
        let mut seen = HashSet::new();
        for cmd in COMMANDS {
            assert!(seen.insert(cmd.id), "id duplicado na tabela: {}", cmd.id);
            assert!(!cmd.label.is_empty());
            assert!(!cmd.menu_path.is_empty());
        }
    }

    #[test]
    fn zoom_vertical_das_formas_de_onda_sobe_desce_e_nao_mexe_no_projeto() {
        let mut s = EditorSession::new(Project::new_default());
        let mut h = HeadlessHost;
        assert_eq!(s.opts.waveform_zoom(), 1.0);
        // no 1× não desce mais
        run_command("view.waveform_zoom_out", &mut s, &mut h).unwrap();
        assert_eq!(s.opts.waveform_zoom_steps, 0);
        run_command("view.waveform_zoom_in", &mut s, &mut h).unwrap();
        run_command("view.waveform_zoom_in", &mut s, &mut h).unwrap();
        assert_eq!(s.opts.waveform_zoom(), 2.0);
        // para no máximo (64×)
        for _ in 0..40 {
            run_command("view.waveform_zoom_in", &mut s, &mut h).unwrap();
        }
        assert_eq!(s.opts.waveform_zoom(), 64.0);
        for _ in 0..40 {
            run_command("view.waveform_zoom_out", &mut s, &mut h).unwrap();
        }
        assert_eq!(s.opts.waveform_zoom(), 1.0, "volta exatamente a 1×");
        assert!(
            s.undo.is_empty(),
            "zoom de exibição não cria passo de Desfazer"
        );
        // valor fora do limite (estado hostil) não passa de 64×
        s.opts.waveform_zoom_steps = u8::MAX;
        assert_eq!(s.opts.waveform_zoom(), 64.0);
    }

    #[test]
    fn comando_headless_inserir_trilha_e_desfazer_refazer() {
        let mut s = EditorSession::new(Project::new_default());
        let mut h = HeadlessHost;
        assert_eq!(s.project.tracks.len(), 2);

        run_command("insert.track_video", &mut s, &mut h).unwrap();
        assert_eq!(s.project.tracks.len(), 3);
        assert!(is_command_enabled("edit.undo", &s, &h));

        run_command("edit.undo", &mut s, &mut h).unwrap();
        assert_eq!(s.project.tracks.len(), 2);
        assert!(is_command_enabled("edit.redo", &s, &h));

        run_command("edit.redo", &mut s, &mut h).unwrap();
        assert_eq!(s.project.tracks.len(), 3);
    }

    #[test]
    fn comando_desconhecido_ou_ui_em_headless_retorna_erro() {
        let mut s = EditorSession::new(Project::new_default());
        let mut h = HeadlessHost;
        assert!(run_command("nao.existe", &mut s, &mut h).is_err());
        assert!(run_command("file.open", &mut s, &mut h).is_err());
    }

    #[test]
    fn atalho_pode_ser_regravado_nas_preferencias() {
        let mut map = HashMap::new();
        map.insert("edit.split".to_string(), Some("Ctrl+Shift+B".to_string()));
        assert_eq!(
            shortcut_label("edit.split", &map).as_deref(),
            Some("Ctrl+Shift+B")
        );
        map.insert("edit.split".to_string(), None);
        assert_eq!(shortcut_label("edit.split", &map), None);
        map.insert("edit.split".to_string(), Some("???".to_string()));
        assert_eq!(shortcut_label("edit.split", &map).as_deref(), Some("S"));
    }

    #[test]
    fn todos_comandos_e_paineis_possuem_traducoes_nos_seis_idiomas() {
        let same_in_ja = [
            "Color Grading",
            "Explorer",
            "Master Bus",
            "Mixer",
            "Plug-In Manager",
            "Surround Panner",
            "Take",
            "Trimmer",
            "Zoom",
        ];
        for cmd in COMMANDS {
            assert!(
                neoveg_common::i18n::has_key(cmd.label) || same_in_ja.contains(&cmd.label),
                "rótulo '{}' do comando '{}' ausente no catálogo i18n",
                cmd.label,
                cmd.id
            );
            for lang in Language::ALL {
                assert!(!cmd.label_in(lang).is_empty());
                assert!(!cmd.menu_path_in(lang).is_empty());
            }
            for seg in neoveg_common::i18n::split_menu_path(cmd.menu_path) {
                assert!(
                    neoveg_common::i18n::has_key(seg) || same_in_ja.contains(&seg),
                    "segmento {:?} de '{}' ({}) ausente no catálogo i18n",
                    seg,
                    cmd.id,
                    cmd.menu_path
                );
            }
        }
        for panel in DockPanel::ALL {
            for lang in Language::ALL {
                assert!(!panel.name_in(lang).is_empty());
            }
        }
        for panel in DockPanel::ALL {
            for lang in Language::ALL {
                assert!(!panel.name_in(lang).is_empty());
            }
        }
        assert_eq!(ShortcutKey::Space.label_in(Language::Ja), "スペース");
        assert_eq!(
            DockPanel::ProjectMedia.name_in(Language::De),
            "Projektmedien"
        );
        assert_eq!(DockPanel::Notes.name_in(Language::Fr), "Notes");
    }

    #[test]
    fn comandos_pitch_sync_link_eixo_e_evento_vazio() {
        use crate::model::{new_id, Event, MediaItem, MediaSource};
        use crate::time::Ticks;

        let mut p = Project::new_default();
        let mut ma = MediaItem {
            id: new_id(),
            name: "voz.wav".into(),
            source: MediaSource::Generated(GenSpec::ColorBars),
            duration: Some(Ticks::from_secs_i64(10)),
            has_video: false,
            has_audio: true,
            width: 0,
            height: 0,
            fps: None,
            info: String::new(),
            proxy_path: None,
        };
        ma.has_video = false;
        p.media.push(ma.clone());
        let a1 = Event::new(&ma, Ticks::ZERO, Ticks::from_secs_i64(4));
        let a2 = Event::new(&ma, Ticks::from_secs_i64(5), Ticks::from_secs_i64(4));
        let (id1, id2) = (a1.id, a2.id);
        p.tracks[1].events = vec![a1, a2];

        let mut s = EditorSession::new(p);
        let mut h = HeadlessHost;
        s.sel_events.insert(id1);
        run_command("event.pitch_up", &mut s, &mut h).unwrap();
        assert_eq!(s.project.event(id1).unwrap().pitch_semitones, 1.0);
        run_command("event.pitch_down_oct", &mut s, &mut h).unwrap();
        assert_eq!(s.project.event(id1).unwrap().pitch_semitones, -11.0);
        run_command("event.toggle_pitch_lock", &mut s, &mut h).unwrap();
        assert!(s.project.event(id1).unwrap().lock_pitch_to_stretch);
        run_command("event.pitch_reset", &mut s, &mut h).unwrap();
        assert_eq!(s.project.event(id1).unwrap().pitch_semitones, 0.0);

        s.sel_events = [id1, id2].into_iter().collect();
        run_command("event.sync_link_create", &mut s, &mut h).unwrap();
        assert_eq!(s.project.event(id2).unwrap().sync_link_master, Some(id1));
        s.sel_events = [id2].into_iter().collect();
        run_command("event.sync_link_select", &mut s, &mut h).unwrap();
        assert_eq!(s.sel_events.len(), 2);
        run_command("event.sync_link_unlink", &mut s, &mut h).unwrap();
        assert!(s.project.event(id2).unwrap().sync_link_master.is_none());

        run_command("edit.tool.axis.horizontal", &mut s, &mut h).unwrap();
        assert_eq!(s.box_axis, crate::session::BoxAxisMode::Horizontal);
        run_command("insert.empty_event", &mut s, &mut h).unwrap();
        assert_eq!(s.sel_events.len(), 1);

        s.sel_track = Some(1);
        run_command("track.record_arm", &mut s, &mut h).unwrap();
        assert!(!s.armed_audio_tracks().is_empty());

        s.active_media_id = Some(ma.id);
        run_command("media.select_events", &mut s, &mut h).unwrap();
        assert_eq!(s.sel_events.len(), 2);
        let sb = s.create_storyboard("Teste").unwrap();
        s.add_media_to_storyboard(sb, ma.id);
        run_command("media.insert_storyboard", &mut s, &mut h).unwrap();
        assert_eq!(s.project.tracks[1].events.len(), 3);

        assert!(s.apply_video_fx_to_output(crate::model::FxKind::Invert));
        assert_eq!(s.project.master.video_fx.len(), 1);
        assert!(!s.apply_video_fx_to_output(crate::model::FxKind::Gain));
        run_command("preview.mute_audio", &mut s, &mut h).unwrap();
        assert!(s.opts.preview_mute_audio);
        run_command("preview.mute_video", &mut s, &mut h).unwrap();
        assert!(s.opts.preview_mute_video);

        // Comandos de sequência
        run_command("sequence.new", &mut s, &mut h).unwrap();
        assert_eq!(s.project.sequences.len(), 1); // 1 inativa + 1 ativa
        run_command("sequence.duplicate", &mut s, &mut h).unwrap();
        assert_eq!(s.project.sequences.len(), 2); // 2 inativas + 1 ativa
        run_command("sequence.delete", &mut s, &mut h).unwrap();
        assert_eq!(s.project.sequences.len(), 1); // 1 inativa + 1 ativa
    }

    #[test]
    fn comandos_sequencias_parametros_hostis() {
        let mut s = EditorSession::new(Project::new_default());
        let mut h = HeadlessHost;

        // Com apenas 1 sequência no projeto, excluir deve ser desabilitado/recusado
        assert!(run_command("sequence.delete", &mut s, &mut h).is_err());

        // sequence.rename com sequência ativa
        run_command("sequence.rename", &mut s, &mut h).unwrap();
        assert!(s
            .project
            .active_sequence_name
            .as_deref()
            .unwrap_or("")
            .contains("(Renomeada)"));

        // sequence.new cria sequência
        run_command("sequence.new", &mut s, &mut h).unwrap();
        assert_eq!(s.project.sequences.len(), 1);

        // sequence.duplicate duplica ativa
        run_command("sequence.duplicate", &mut s, &mut h).unwrap();
        assert_eq!(s.project.sequences.len(), 2);

        // sequence.delete agora tem sucesso
        run_command("sequence.delete", &mut s, &mut h).unwrap();
        assert_eq!(s.project.sequences.len(), 1);

        // Estado hostil: active_sequence_id = None
        s.project.active_sequence_id = None;
        assert!(run_command("sequence.rename", &mut s, &mut h).is_err());
        assert!(run_command("sequence.duplicate", &mut s, &mut h).is_err());

        // sequence.nest_selection sem eventos selecionados deve falhar
        assert!(run_command("sequence.nest_selection", &mut s, &mut h).is_err());

        // Cria projeto com evento e testa aninhamento
        let mut s2 = EditorSession::new(Project::new_default());
        let m = MediaItem {
            id: new_id(),
            name: "Video Teste".into(),
            source: MediaSource::Generated(GenSpec::ColorBars),
            duration: Some(Ticks::from_secs_i64(10)),
            has_video: true,
            has_audio: false,
            width: 1920,
            height: 1080,
            fps: Some(FrameRate::FPS_30),
            info: String::new(),
            proxy_path: None,
        };
        s2.project.media.push(m.clone());
        let ev1 = Event::new(&m, Ticks::from_secs_i64(2), Ticks::from_secs_i64(5));
        let ev1_id = ev1.id;
        s2.project.tracks[0].events.push(ev1);
        s2.sel_events.insert(ev1_id);

        // Aninhar seleção com sucesso
        run_command("sequence.nest_selection", &mut s2, &mut h).unwrap();
        // A timeline ativa agora deve conter 1 evento substituto apontando para a sequência
        assert_eq!(s2.project.tracks[0].events.len(), 1);
        let nested_clip = &s2.project.tracks[0].events[0];
        assert_eq!(nested_clip.start, Ticks::from_secs_i64(2));
        assert_eq!(nested_clip.length, Ticks::from_secs_i64(5));

        // A mídia do clipe substituto deve ser MediaSource::Sequence
        let nested_media = s2.project.media(nested_clip.media_id).unwrap();
        let nested_seq_id = match nested_media.source {
            MediaSource::Sequence { id } => id,
            _ => panic!("deve ser MediaSource::Sequence"),
        };

        // A sequência aninhada deve existir em s2.project.sequences
        let nested_seq = s2
            .project
            .sequences
            .iter()
            .find(|seq| seq.id == nested_seq_id)
            .unwrap();
        assert_eq!(nested_seq.tracks[0].events.len(), 1);
        // O evento na sequência aninhada deve ter sido compensado para iniciar no zero
        assert_eq!(nested_seq.tracks[0].events[0].start, Ticks::ZERO);

        // Validação de ciclo: aninhar a sequência dentro de si mesma deve falhar
        assert!(s2
            .project
            .validate_no_sequence_cycle(nested_seq_id, nested_seq_id)
            .is_err());

        // Comandos .nvegseq em modo headless (parâmetros hostis §0.8)
        assert!(run_command("sequence.import_nvegseq", &mut s2, &mut h).is_err());
        // Sem caminho salvo, exportar sem diálogo retorna erro controlado
        s2.path = None;
        assert!(run_command("sequence.export_nvegseq", &mut s2, &mut h).is_err());
        // Com active_sequence_id = None, exportar fica desabilitado
        let mut s_no_seq = EditorSession::new(Project::new_default());
        s_no_seq.project.active_sequence_id = None;
        assert!(run_command("sequence.export_nvegseq", &mut s_no_seq, &mut h).is_err());
        // Com caminho temporário válido, exporta .nvegseq com sucesso
        let tmp_dir =
            std::env::temp_dir().join(format!("neoveg_cmd_nvegseq_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        s2.path = Some(tmp_dir.join("proj.neoveg"));
        run_command("sequence.export_nvegseq", &mut s2, &mut h).unwrap();
        let exported = tmp_dir.join("proj.nvegseq");
        assert!(exported.exists());
        let mut dst = Project::new_default();
        crate::package::import_standalone_sequence_file(&mut dst, &exported).unwrap();
        assert_eq!(dst.sequences.len(), 1);
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn comando_work_folders_hostil() {
        struct MockUiHost {
            last_action: Option<UiAction>,
        }
        impl CommandHost for MockUiHost {
            fn supports_ui(&self) -> bool {
                true
            }
            fn dispatch_ui(&mut self, action: UiAction) -> Result<(), String> {
                self.last_action = Some(action);
                Ok(())
            }
            fn insert_generated(
                &mut self,
                _session: &mut EditorSession,
                _spec: GenSpec,
            ) -> Result<(), String> {
                Ok(())
            }
        }

        let mut s = EditorSession::new(Project::new_default());
        let mut h = HeadlessHost;

        // Em modo headless, comando que requer UI recusa a execução
        assert!(run_command("file.work_folders", &mut s, &mut h).is_err());

        // Com host de interface gráfica ativo, despacha a ação OpenWorkFolders
        let mut ui_host = MockUiHost { last_action: None };
        assert!(run_command("file.work_folders", &mut s, &mut ui_host).is_ok());
        assert_eq!(ui_host.last_action, Some(UiAction::OpenWorkFolders));

        // Parâmetros hostis em set_work_folders
        let hostil = crate::model::WorkFolders {
            captured_media: "X:\0\0\0//invalido".into(),
            audio_recordings: "b".repeat(5000),
            prerendered_files: "\0\0".into(),
            proxies: "   C:\\Temp\\Proxies   ".into(),
            render_exports: "D:\\Renders".into(),
        };
        s.set_work_folders(hostil);
        assert!(!s.work_folders().captured_media.contains('\0'));
        assert_eq!(s.work_folders().audio_recordings.len(), 1024);
        assert_eq!(s.work_folders().proxies, "C:\\Temp\\Proxies");

        // Desfazer restaura as pastas anteriores
        assert!(s.undo());
        assert!(s.work_folders().is_empty());
    }

    fn midia_teste() -> MediaItem {
        MediaItem {
            id: new_id(),
            name: "v".into(),
            source: MediaSource::Generated(GenSpec::ColorBars),
            duration: Some(Ticks::from_secs_i64(60)),
            has_video: true,
            has_audio: false,
            width: 64,
            height: 64,
            fps: Some(FrameRate::FPS_30),
            info: String::new(),
            proxy_path: None,
        }
    }

    #[test]
    fn teclas_novas_fazem_parse_e_voltam_ao_mesmo_texto() {
        for spec in [
            "Backspace",
            "PageUp",
            "PageDown",
            "Ctrl+F4",
            "Shift+F5",
            "Ctrl+Shift+F12",
        ] {
            let sc = Shortcut::parse(spec).unwrap();
            assert_eq!(sc.format(), spec);
            assert_eq!(Shortcut::parse(&sc.format_canonical()), Some(sc));
        }
        assert_eq!(ShortcutKey::parse("pgdn"), Some(ShortcutKey::PageDown));
    }

    /// Delete sem Ripple automático deixa o buraco; F fecha só nas trilhas
    /// editadas, Ctrl+F leva os marcadores, Ctrl+Shift+F leva tudo. O mesmo
    /// ripple não se aplica duas vezes.
    #[test]
    fn ripple_pos_edicao_fecha_o_buraco_conforme_o_escopo() {
        let secs = Ticks::from_secs_i64;
        let m = midia_teste();
        let mut p = Project::new_default();
        let a = Event::new(&m, secs(0), secs(2));
        let b = Event::new(&m, secs(2), secs(2));
        let c = Event::new(&m, secs(4), secs(2));
        let outra = Event::new(&m, secs(5), secs(1));
        let (ida, idb, idc, id_outra) = (a.id, b.id, c.id, outra.id);
        p.media.push(m);
        p.tracks[0].events = vec![a, b, c];
        p.tracks[1].events = vec![outra];
        p.markers
            .push(Marker::new(secs(5), MarkerKind::default(), "m"));
        let mut s = EditorSession::new(p);
        let mut h = HeadlessHost;
        assert!(!is_command_enabled("edit.post_ripple_tracks", &s, &h));

        s.sel_events.insert(idb);
        run_command("edit.delete", &mut s, &mut h).unwrap();
        assert_eq!(s.project.event(idc).unwrap().start, secs(4));
        assert!(is_command_enabled("edit.post_ripple_tracks", &s, &h));
        run_command("edit.post_ripple_tracks", &mut s, &mut h).unwrap();
        assert_eq!(s.project.event(idc).unwrap().start, secs(2));
        assert_eq!(s.project.event(ida).unwrap().start, secs(0));
        assert_eq!(s.project.event(id_outra).unwrap().start, secs(5));
        assert_eq!(s.project.markers[0].t, secs(5), "F não mexe em marcadores");
        assert!(
            !is_command_enabled("edit.post_ripple_tracks", &s, &h),
            "o mesmo ripple não se repete"
        );

        // desfazer volta ao estado logo depois do Delete
        run_command("edit.undo", &mut s, &mut h).unwrap();
        assert_eq!(s.project.event(idc).unwrap().start, secs(4));
        run_command("edit.post_ripple_markers", &mut s, &mut h).unwrap();
        assert_eq!(s.project.event(idc).unwrap().start, secs(2));
        assert_eq!(s.project.markers[0].t, secs(3));
        assert_eq!(s.project.event(id_outra).unwrap().start, secs(5));

        run_command("edit.undo", &mut s, &mut h).unwrap();
        run_command("edit.post_ripple_all", &mut s, &mut h).unwrap();
        assert_eq!(s.project.event(idc).unwrap().start, secs(2));
        assert_eq!(s.project.event(id_outra).unwrap().start, secs(3));
        assert_eq!(s.project.markers[0].t, secs(3));
    }

    /// Aparar o fim de um evento para frente empurra o que vem depois
    /// pela mesma diferença; dividir não muda fim nenhum.
    #[test]
    fn ripple_pos_edicao_apos_aparar_e_dividir() {
        let secs = Ticks::from_secs_i64;
        let m = midia_teste();
        let mut p = Project::new_default();
        let a = Event::new(&m, secs(0), secs(2));
        let b = Event::new(&m, secs(3), secs(2));
        let (ida, idb) = (a.id, b.id);
        p.media.push(m);
        p.tracks[0].events = vec![a, b];
        let mut s = EditorSession::new(p);
        let mut h = HeadlessHost;
        s.checkpoint();
        if let Some(e) = s.project.event_mut(ida) {
            e.length = secs(4);
        }
        s.changed();
        let plan = s.post_edit_ripple_plan();
        assert_eq!(plan, vec![(0, secs(2), secs(2))]);
        run_command("edit.post_ripple_tracks", &mut s, &mut h).unwrap();
        assert_eq!(s.project.event(idb).unwrap().start, secs(5));
        assert_eq!(s.project.event(ida).unwrap().length, secs(4));

        // dividir não muda o fim de nada: não há o que propagar
        s.sel_events = [ida].into_iter().collect();
        s.cursor = secs(1);
        run_command("edit.split", &mut s, &mut h).unwrap();
        assert!(s.post_edit_ripple_plan().is_empty());
    }

    #[test]
    fn colar_repetindo_lado_a_lado_e_com_espacamento() {
        let secs = Ticks::from_secs_i64;
        let m = midia_teste();
        let mut p = Project::new_default();
        let a = Event::new(&m, secs(0), secs(2));
        let ida = a.id;
        p.media.push(m);
        p.tracks[0].events = vec![a];
        let mut s = EditorSession::new(p);
        let mut h = HeadlessHost;
        s.sel_events.insert(ida);
        run_command("edit.copy", &mut s, &mut h).unwrap();
        // sem interface, o diálogo não abre
        assert!(run_command("edit.paste_repeat", &mut s, &mut h).is_err());

        let starts = |s: &EditorSession| {
            let mut v: Vec<Ticks> = s.project.tracks[0]
                .events
                .iter()
                .filter(|e| e.id != ida)
                .map(|e| e.start)
                .collect();
            v.sort();
            v
        };
        s.cursor = secs(10);
        let undo_before = s.undo.len();
        assert_eq!(s.paste_repeat(3, None), 3);
        assert_eq!(starts(&s), vec![secs(10), secs(12), secs(14)]);
        assert_eq!(s.cursor, secs(16));
        assert_eq!(s.undo.len(), undo_before + 1, "um passo de Desfazer");

        assert!(s.undo());
        s.cursor = secs(10);
        assert_eq!(s.paste_repeat(2, Some(secs(5))), 2);
        assert_eq!(starts(&s), vec![secs(10), secs(15)]);

        // parâmetros hostis
        assert_eq!(s.paste_repeat(0, None), 0);
        assert!(s.undo());
        s.cursor = Ticks::ZERO;
        assert_eq!(
            s.paste_repeat(u32::MAX, Some(Ticks::from_raw(-5))),
            crate::session::MAX_PASTE_REPEAT as usize
        );
    }

    #[test]
    fn backspace_restaura_as_ultimas_cinco_selecoes_de_tempo() {
        let secs = Ticks::from_secs_i64;
        let mut s = EditorSession::new(Project::new_default());
        let mut h = HeadlessHost;
        assert!(!is_command_enabled("edit.restore_time_selection", &s, &h));
        for i in 1..=7 {
            s.time_sel = Some((secs(i), secs(i + 1)));
            s.note_time_selection();
        }
        for want in (2..=6).rev() {
            run_command("edit.restore_time_selection", &mut s, &mut h).unwrap();
            assert_eq!(s.time_sel, Some((secs(want), secs(want + 1))));
        }
        assert!(!is_command_enabled("edit.restore_time_selection", &s, &h));
        assert!(s.undo.is_empty(), "seleção não entra no Desfazer");
    }

    #[test]
    fn pageup_pagedown_andam_pela_grade() {
        use crate::session::grid_neighbor;
        let secs = Ticks::from_secs_i64;
        let one = secs(1);
        assert_eq!(grid_neighbor(Ticks::ZERO, one, 1), one);
        assert_eq!(grid_neighbor(Ticks::from_millis_i64(1500), one, 1), secs(2));
        assert_eq!(grid_neighbor(Ticks::from_millis_i64(1500), one, -1), one);
        assert_eq!(grid_neighbor(secs(2), one, -1), one);
        assert_eq!(grid_neighbor(Ticks::ZERO, one, -1), Ticks::ZERO);
        // passo hostil vira 1 tick; o topo não estoura
        assert_eq!(
            grid_neighbor(Ticks::from_raw(5), Ticks::ZERO, 1),
            Ticks::from_raw(6)
        );
        assert!(grid_neighbor(Ticks::MAX, one, 1) <= Ticks::MAX);

        let mut s = EditorSession::new(Project::new_default());
        let mut h = HeadlessHost;
        run_command("transport.grid_right", &mut s, &mut h).unwrap();
        run_command("transport.grid_right", &mut s, &mut h).unwrap();
        assert_eq!(s.cursor, secs(2));
        run_command("transport.grid_left", &mut s, &mut h).unwrap();
        assert_eq!(s.cursor, one);
    }

    #[test]
    fn ctrl_colchetes_vao_as_bordas_dos_eventos_selecionados_e_w_ao_inicio() {
        let secs = Ticks::from_secs_i64;
        let m = midia_teste();
        let mut p = Project::new_default();
        let a = Event::new(&m, secs(0), secs(2));
        let c = Event::new(&m, secs(4), secs(2));
        let ids = [a.id, c.id];
        p.media.push(m);
        p.tracks[0].events = vec![a, c];
        let mut s = EditorSession::new(p);
        let mut h = HeadlessHost;
        assert!(!is_command_enabled(
            "edit.cursor_prev_selected_edge",
            &s,
            &h
        ));
        s.sel_events = ids.into_iter().collect();
        s.cursor = secs(3);
        run_command("edit.cursor_prev_selected_edge", &mut s, &mut h).unwrap();
        assert_eq!(s.cursor, secs(2));
        run_command("edit.cursor_prev_selected_edge", &mut s, &mut h).unwrap();
        assert_eq!(s.cursor, Ticks::ZERO);
        // sem borda antes: o cursor fica
        run_command("edit.cursor_prev_selected_edge", &mut s, &mut h).unwrap();
        assert_eq!(s.cursor, Ticks::ZERO);
        for want in [2, 4, 6] {
            run_command("edit.cursor_next_selected_edge", &mut s, &mut h).unwrap();
            assert_eq!(s.cursor, secs(want));
        }

        // W vai ao início do projeto mesmo com seleção de tempo
        s.time_sel = Some((secs(4), secs(6)));
        run_command("transport.rewind_w", &mut s, &mut h).unwrap();
        assert_eq!(s.cursor, Ticks::ZERO);
    }

    #[test]
    fn encaixe_e_cabecalhos_alternam_sem_desfazer() {
        let mut s = EditorSession::new(Project::new_default());
        let mut h = HeadlessHost;
        assert!(s.opts.snap_to_markers && s.opts.snap_to_all_events);
        assert!(s.opts.show_event_headers);
        run_command("options.snap_to_markers", &mut s, &mut h).unwrap();
        run_command("options.snap_to_events", &mut s, &mut h).unwrap();
        run_command("view.event_headers", &mut s, &mut h).unwrap();
        assert!(!s.opts.snap_to_markers && !s.opts.snap_to_all_events);
        assert!(!s.opts.show_event_headers);
        assert!(s.undo.is_empty());
        // os comandos de interface recusam o modo headless
        for id in ["options.dim_master", "tools.rebuild_peaks", "file.close"] {
            assert!(run_command(id, &mut s, &mut h).is_err(), "{id}");
        }
    }

    #[test]
    fn criar_proxy_desabilita_para_imagem_estatica_e_habilita_para_video() {
        struct UiHost;
        impl CommandHost for UiHost {
            fn supports_ui(&self) -> bool {
                true
            }
            fn dispatch_ui(&mut self, _action: UiAction) -> Result<(), String> {
                Ok(())
            }
            fn insert_generated(
                &mut self,
                _session: &mut EditorSession,
                _spec: GenSpec,
            ) -> Result<(), String> {
                Ok(())
            }
        }
        let ui_h = UiHost;
        let mut p = Project::new_default();
        let still = MediaItem {
            id: new_id(),
            name: "foto.png".into(),
            source: MediaSource::File {
                path: "foto.png".into(),
            },
            duration: None,
            has_video: true,
            has_audio: false,
            width: 1920,
            height: 1080,
            fps: None,
            info: String::new(),
            proxy_path: None,
        };
        let video = MediaItem {
            id: new_id(),
            name: "clipe.mp4".into(),
            source: MediaSource::File {
                path: "clipe.mp4".into(),
            },
            duration: Some(Ticks::from_secs_i64(10)),
            has_video: true,
            has_audio: true,
            width: 1920,
            height: 1080,
            fps: Some(FrameRate::FPS_30),
            info: String::new(),
            proxy_path: None,
        };
        let ev_still = Event::new(&still, Ticks::ZERO, Ticks::from_secs_i64(5));
        let ev_video = Event::new(&video, Ticks::from_secs_i64(6), Ticks::from_secs_i64(5));
        let (id_still, id_video) = (ev_still.id, ev_video.id);
        p.media.push(still);
        p.media.push(video);
        p.tracks[0].events = vec![ev_still, ev_video];

        let mut s = EditorSession::new(p);
        s.sel_events.insert(id_still);
        assert!(
            !is_command_enabled("tools.build_proxy", &s, &ui_h),
            "imagem estática não deve habilitar Criar proxy"
        );
        s.sel_events.clear();
        s.sel_events.insert(id_video);
        assert!(
            is_command_enabled("tools.build_proxy", &s, &ui_h),
            "vídeo com duração deve habilitar Criar proxy"
        );
    }
}

/// As teclas padrão seguem a lista oficial de atalhos do VEGAS Pro (manual
/// do VEGAS Pro 21 e lista de teclado do VEGAS Pro 18). Quem vem do VEGAS
/// aperta a tecla e espera o mesmo comando; um recurso novo não pode tomar
/// uma tecla do VEGAS (aconteceu com `Ctrl+Alt+M`, `Ctrl+Shift+E`, `Ctrl+B`,
/// `Ctrl+M`, `Alt+3` e `Ctrl+=`/`Ctrl+-`). Este teste diz o que ele previne:
/// mudar uma linha aqui exige conferir o manual antes.
#[cfg(test)]
mod vegas_keymap_tests {
    use super::*;

    /// (tecla do VEGAS, comando do Neoveg que deve usá-la).
    const VEGAS_KEYS: &[(&str, &str)] = &[
        ("Ctrl+N", "file.new"),
        ("Ctrl+O", "file.open"),
        ("Ctrl+S", "file.save"),
        ("Ctrl+Shift+S", "file.save_as"),
        ("Ctrl+Alt+S", "file.save_version"),
        ("Alt+Enter", "file.project_props"),
        ("Alt+2", "view.tab.trimmer"),
        ("Alt+3", "view.master_bus"),
        ("Ctrl+Alt+1", "view.tab.plugin_manager"),
        ("Ctrl+Alt+4", "view.tab.edit_details"),
        ("Ctrl+Alt+7", "view.tab.loudness"),
        ("B", "view.bus_tracks"),
        ("Ctrl+Shift+B", "view.video_bus_track"),
        ("Ctrl+Shift+E", "view.envelopes"),
        ("F11", "view.toggle_dock_area"),
        ("Ctrl+F11", "view.maximize_timeline"),
        ("Shift+F11", "view.toggle_track_list"),
        ("Ctrl+G", "edit.goto"),
        ("Ctrl+Home", "transport.project_start"),
        ("Ctrl+End", "transport.project_end"),
        ("Shift+Q", "edit.select_loop_region"),
        ("Ctrl+X", "edit.cut"),
        ("Ctrl+C", "edit.copy"),
        ("Ctrl+V", "edit.paste"),
        ("Ctrl+Shift+V", "edit.paste_insert"),
        ("Delete", "edit.delete"),
        ("Ctrl+T", "edit.trim_to_selection"),
        ("Ctrl+M", "track.render_to_new_track"),
        ("Ctrl+Z", "edit.undo"),
        ("Ctrl+Y", "edit.redo"),
        ("D", "edit.tool.next"),
        ("Shift+D", "edit.tool.prev"),
        ("Ctrl+Shift+D", "tools.multicam_mode"),
        ("Ctrl+A", "edit.select_all"),
        ("Ctrl+Shift+A", "edit.deselect_all"),
        ("S", "edit.split"),
        ("Alt+S", "edit.smart_split"),
        ("Alt+[", "edit.trim_start"),
        ("Alt+]", "edit.trim_end"),
        ("T", "event.next_take"),
        ("Shift+T", "event.prev_take"),
        ("Shift+=", "event.pitch_up_oct"),
        ("Shift+-", "event.pitch_down_oct"),
        ("Ctrl+Shift+=", "event.pitch_reset"),
        ("Ctrl+R", "transport.record"),
        ("Q", "options.loop_playback"),
        ("Shift+Space", "transport.play_from_start"),
        ("Alt+Left", "transport.frame_back"),
        ("Alt+Right", "transport.frame_fwd"),
        ("Shift+B", "preview.ram_build"),
        ("Ctrl+Q", "insert.track_audio"),
        ("Ctrl+Shift+Q", "insert.track_video"),
        ("Z", "track.mute"),
        ("X", "track.solo"),
        ("F8", "options.snapping"),
        ("Ctrl+F8", "options.snap_to_grid"),
        ("Ctrl+L", "options.auto_ripple"),
        ("Ctrl+Shift+X", "options.auto_crossfades"),
        ("G", "edit.group"),
        ("U", "edit.ungroup"),
        ("Shift+G", "edit.select_group"),
        ("V", "insert.env_volume"),
        ("Shift+V", "insert.remove_env_volume"),
        ("P", "insert.env_pan"),
        ("Shift+P", "insert.remove_env_pan"),
        ("M", "insert.marker"),
        ("R", "insert.region"),
        ("N", "insert.marker_cd_track"),
        ("Alt+F8", "options.quantize"),
        ("Ctrl+Shift+W", "view.waveforms"),
        ("Ctrl+D", "edit.tool.normal"),
        ("Ctrl+Alt+R", "track.record_arm"),
        ("Ctrl+U", "edit.clear_group"),
        ("Ctrl+=", "event.pitch_up_cent"),
        ("Ctrl+-", "event.pitch_down_cent"),
        ("Shift+Z", "track.mute_exclusive"),
        ("Shift+X", "track.solo_exclusive"),
        ("Ctrl+Shift+Up", "view.tracks_taller"),
        ("Ctrl+Shift+Down", "view.tracks_shorter"),
        ("Alt+1", "view.tab.explorer"),
        ("Alt+4", "view.preview"),
        ("Alt+5", "view.tab.project_media"),
        ("Alt+6", "view.tab.notes"),
        ("Alt+7", "view.tab.transitions"),
        ("Alt+8", "view.tab.video_fx"),
        ("Alt+9", "view.tab.generators"),
        ("Ctrl+Alt+2", "view.tab.scopes"),
        ("Ctrl+Alt+3", "view.tab.surround_panner"),
        ("Ctrl+Alt+5", "view.tab.mixer"),
        ("Alt+G", "view.tab.color_grading"),
        ("Shift+M", "tools.prerender"),
        ("Ctrl+Shift+U", "options.ignore_grouping"),
        ("Alt+C", "sequence.nest_selection"),
        ("Alt+N", "file.open_nested_tab"),
        ("Alt+P", "timeline.open_parent"),
        ("Ctrl+Shift+Alt+Q", "insert.track_adjustment"),
        ("Shift+N", "insert.marker_cd_index"),
        ("C", "insert.marker_command"),
        ("Shift+F8", "options.snap_to_markers"),
        ("Ctrl+Shift+F8", "options.snap_to_events"),
        ("Ctrl+Alt+M", "view.event_headers"),
        ("Backspace", "edit.restore_time_selection"),
        ("PageUp", "transport.grid_left"),
        ("PageDown", "transport.grid_right"),
        ("Ctrl+Shift+F12", "options.dim_master"),
        ("Shift+F5", "tools.rebuild_peaks"),
        ("Ctrl+F4", "file.close"),
        ("Ctrl+B", "edit.paste_repeat"),
        ("F", "edit.post_ripple_tracks"),
        ("Ctrl+F", "edit.post_ripple_markers"),
        ("Ctrl+Shift+F", "edit.post_ripple_all"),
        ("F2", "edit.rename_event"),
        ("W", "transport.rewind_w"),
        ("Ctrl+[", "edit.cursor_prev_selected_edge"),
        ("Ctrl+]", "edit.cursor_next_selected_edge"),
    ];

    /// Teclas de comandos do VEGAS que o Neoveg ainda não tem: ficam livres
    /// até o comando existir, para não fazerem outra coisa. Hoje vazia: os
    /// comandos de `Ctrl+Alt+M` e `Ctrl+B` já existem (estão em `VEGAS_KEYS`).
    const VEGAS_RESERVED_UNBOUND: &[(&str, &str)] = &[];

    fn bound_to(sc: Shortcut) -> Vec<&'static str> {
        COMMANDS
            .iter()
            .filter(|c| c.default_shortcut == Some(sc))
            .map(|c| c.id)
            .collect()
    }

    #[test]
    fn teclas_do_vegas_chamam_o_mesmo_comando() {
        for (key, id) in VEGAS_KEYS {
            let Some(sc) = Shortcut::parse(key) else {
                panic!("tecla inválida na tabela do teste: {key}");
            };
            assert!(
                find_command(id).is_some(),
                "comando {id} da tabela não existe"
            );
            assert_eq!(
                bound_to(sc),
                vec![*id],
                "{key} é a tecla do VEGAS para {id}; confira o manual antes de mudar"
            );
        }
    }

    #[test]
    fn teclas_reservadas_do_vegas_ficam_livres() {
        for (key, what) in VEGAS_RESERVED_UNBOUND {
            let Some(sc) = Shortcut::parse(key) else {
                panic!("tecla inválida na tabela do teste: {key}");
            };
            assert!(
                bound_to(sc).is_empty(),
                "{key} é a tecla do VEGAS para {what}; não ligue outro comando a ela"
            );
        }
    }
}
