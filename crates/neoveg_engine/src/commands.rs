//! Tabela Única de Comandos (AGENTS.md §3.3).
//!
//! Toda ação editável, de menu, de barra de ferramentas, de atalho de teclado,
//! de script `.NET` ou de CLI é registrada aqui com `id`, rótulo localizado,
//! caminho de menu, atalho padrão (regravável), `enabled()` e `run()`.

use crate::media::MediaCache;
use crate::model::{EnvKind, GenSpec, Id, TextSpec, TrackKind};
use crate::session::EditorSession;
use crate::time::Ticks;
use neoveg_common::LayoutType;
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
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    OpenBracket,
    CloseBracket,
    Backslash,
}

impl ShortcutKey {
    pub fn label(self) -> &'static str {
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
            ShortcutKey::ArrowLeft => "Esquerda",
            ShortcutKey::ArrowRight => "Direita",
            ShortcutKey::ArrowUp => "Cima",
            ShortcutKey::ArrowDown => "Baixo",
            ShortcutKey::Home => "Home",
            ShortcutKey::End => "End",
            ShortcutKey::OpenBracket => "[",
            ShortcutKey::CloseBracket => "]",
            ShortcutKey::Backslash => "\\",
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
            "left" | "esquerda" => ShortcutKey::ArrowLeft,
            "right" | "direita" => ShortcutKey::ArrowRight,
            "up" | "cima" => ShortcutKey::ArrowUp,
            "down" | "baixo" => ShortcutKey::ArrowDown,
            "home" => ShortcutKey::Home,
            "end" => ShortcutKey::End,
            "[" | "openbracket" => ShortcutKey::OpenBracket,
            "]" | "closebracket" => ShortcutKey::CloseBracket,
            "\\" | "backslash" => ShortcutKey::Backslash,
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

    pub fn format(self) -> String {
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
        parts.push(self.key.label());
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
}

impl DockPanel {
    pub const ALL: [DockPanel; 16] = [
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
    ];

    pub fn name(self) -> &'static str {
        match self {
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
        }
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
        }
    }
}

/// Ações de interface/transporte delegadas pela tabela de comandos ao host ativo.
#[derive(Clone, Debug, PartialEq)]
pub enum UiAction {
    NewProject,
    OpenProjectDialog,
    SaveProject { save_as: bool },
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
    OpenRenderDialog,
    SnapshotDialog,
    OpenProjectProps,
    OpenEventProps,
    OpenPanCrop,
    OpenFxEvent,
    OpenFxMasterAudio,
    OpenFxMasterVideo,
    OpenGoto,
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
    ExitApp,
    SelectDock(DockPanel),
    TogglePreview,
    ToggleMasterBus,
    TogglePlay,
    PlayFromStart,
    Play,
    Stop { restore_cursor: bool },
    Shuttle(f64),
    SetCursor(Ticks),
    CenterTimeline,
    ZoomBy(f32),
    JumpMarker(bool),
    JumpMarkerIndex(usize),
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
        Err(format!(
            "ação '{action:?}' requer interface gráfica ativa e não pode rodar em modo headless"
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
        Err(format!(
            "projeto ainda não tem caminho definido para exportar {label} sem diálogo"
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

/// Tabela Única de Comandos do Neoveg (§3.3).
pub static COMMANDS: &[CommandDef] = &[
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
                Err("projeto ainda não tem caminho definido para salvar sem diálogo".into())
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
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::M)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::OpenRenderDialog, h),
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
        label: "Sonic Foundry",
        menu_path: "Editar/Propriedades/Layout/Tipos de layout/Sonic Foundry",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SetLayoutType(LayoutType::SonicFoundry), h),
    },
    CommandDef {
        id: "edit.properties.layout.sony",
        label: "Sony",
        menu_path: "Editar/Propriedades/Layout/Tipos de layout/Sony",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SetLayoutType(LayoutType::Sony), h),
    },
    CommandDef {
        id: "edit.properties.layout.magix",
        label: "MAGIX",
        menu_path: "Editar/Propriedades/Layout/Tipos de layout/MAGIX",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SetLayoutType(LayoutType::Magix), h),
    },
    // --- Exibir ---
    CommandDef {
        id: "view.tab.project_media",
        label: "Mídia do Projeto",
        menu_path: "Exibir/Mídia do Projeto",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::ProjectMedia), h),
    },
    CommandDef {
        id: "view.tab.explorer",
        label: "Explorer",
        menu_path: "Exibir/Explorer",
        default_shortcut: None,
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
        default_shortcut: Some(Shortcut::alt(ShortcutKey::Num3)),
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::EditDetails), h),
    },
    CommandDef {
        id: "view.tab.transitions",
        label: "Transições",
        menu_path: "Exibir/Transições",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::Transitions), h),
    },
    CommandDef {
        id: "view.tab.video_fx",
        label: "Efeitos de Vídeo",
        menu_path: "Exibir/Efeitos de Vídeo",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::VideoFx), h),
    },
    CommandDef {
        id: "view.tab.color_grading",
        label: "Color Grading",
        menu_path: "Exibir/Color Grading",
        default_shortcut: None,
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
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::Generators), h),
    },
    CommandDef {
        id: "view.tab.mixer",
        label: "Mixer",
        menu_path: "Exibir/Mixer",
        default_shortcut: None,
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
        default_shortcut: None,
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
        default_shortcut: Some(Shortcut::ctrl(ShortcutKey::B)),
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.show_bus_tracks = !s.opts.show_bus_tracks;
            ok_mutated(s, h)
        },
    },
    CommandDef {
        id: "view.tab.scopes",
        label: "Escopos",
        menu_path: "Exibir/Escopos",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::Scopes), h),
    },
    CommandDef {
        id: "view.tab.notes",
        label: "Notas",
        menu_path: "Exibir/Notas",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::SelectDock(DockPanel::Notes), h),
    },
    CommandDef {
        id: "view.envelopes",
        label: "Mostrar envelopes",
        menu_path: "Exibir/Mostrar envelopes",
        default_shortcut: None,
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
        default_shortcut: None,
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
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::TogglePreview, h),
    },
    CommandDef {
        id: "view.master_bus",
        label: "Master Bus",
        menu_path: "Exibir/Master Bus",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::ToggleMasterBus, h),
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
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.checkpoint();
            s.insert_adjustment_track();
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
                return Err("limite de 26 barramentos auxiliares atingido".into());
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
        default_shortcut: Some(Shortcut::plain(ShortcutKey::F2)),
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
        id: "tools.build_proxy",
        label: "Criar proxy de vídeo para selecionados",
        menu_path: "Ferramentas/Criar proxy de vídeo para selecionados",
        default_shortcut: None,
        needs_ui: true,
        enabled: has_selected_events_ui,
        run: |_s, h| ui_action(UiAction::BuildProxySelected, h),
    },
    CommandDef {
        id: "tools.prerender",
        label: "Pré-renderizar seletivamente região...",
        menu_path: "Ferramentas/Pré-renderizar seletivamente região...",
        default_shortcut: None,
        needs_ui: true,
        enabled: ui_enabled,
        run: |_s, h| ui_action(UiAction::PrerenderRegion, h),
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
        id: "options.quantize",
        label: "Quantizar para quadros",
        menu_path: "Opções/Quantizar para quadros",
        default_shortcut: None,
        needs_ui: false,
        enabled: always_enabled,
        run: |s, h| {
            s.opts.quantize = !s.opts.quantize;
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
        default_shortcut: None,
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
        label: "Ir para o início da seleção (W)",
        menu_path: "Transporte/Ir para início (W)",
        default_shortcut: Some(Shortcut::plain(ShortcutKey::W)),
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
    let cmd = find_command(id).ok_or_else(|| format!("comando desconhecido: '{id}'"))?;
    if !(cmd.enabled)(session, host) {
        return Err(format!(
            "comando '{}' não está habilitado no momento",
            cmd.id
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

pub fn shortcut_label(id: &str, overrides: &HashMap<String, Option<String>>) -> Option<String> {
    let cmd = find_command(id)?;
    resolved_shortcut(cmd, overrides).map(Shortcut::format)
}

/// Rótulo formatado para item de menu com atalho à direita (se configurado).
pub fn menu_label(id: &str, overrides: &HashMap<String, Option<String>>) -> String {
    let Some(cmd) = find_command(id) else {
        return id.to_string();
    };
    match resolved_shortcut(cmd, overrides) {
        Some(sc) => format!("{}   {}", cmd.label, sc.format()),
        None => cmd.label.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Project;
    use std::collections::HashSet;

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
}
