//! Estado editável de uma sessão de edição da timeline: projeto atual,
//! histórico de Desfazer/Refazer, área de transferência, seleção, cursor,
//! opções e regras de edição puras sem dependência de UI (AGENTS.md §3.2).

use crate::media::MediaCache;
use crate::model::*;
use crate::prefs::PreviewQuality;
use neoveg_common::{tr, tr_args, Language, NeovegError, Result};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

pub const MAX_UNDO: usize = 200;

/// Modos de edição suportados pelo motor (§3.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EditMode {
    #[default]
    Normal,
    Shuffle,
    Slip,
    Slide,
    Stretch,
    SplitTrim,
    Roll,
    Ripple,
}

impl EditMode {
    pub const ALL: [EditMode; 8] = [
        EditMode::Normal,
        EditMode::Shuffle,
        EditMode::Slip,
        EditMode::Slide,
        EditMode::Stretch,
        EditMode::SplitTrim,
        EditMode::Roll,
        EditMode::Ripple,
    ];

    /// Modos da ferramenta Normal exibidos no seletor da barra de ferramentas (p. 236).
    pub const NORMAL_MODES: [EditMode; 7] = [
        EditMode::Normal,
        EditMode::Shuffle,
        EditMode::Slip,
        EditMode::Slide,
        EditMode::Stretch,
        EditMode::SplitTrim,
        EditMode::Roll,
    ];

    pub fn name_in(self, lang: Language) -> &'static str {
        let pt = match self {
            EditMode::Normal => "Normal",
            EditMode::Shuffle => "Shuffle (reordenar)",
            EditMode::Slip => "Slip (deslizar conteúdo)",
            EditMode::Slide => "Slide (deslizar evento)",
            EditMode::Stretch => "Stretch (esticamento de tempo)",
            EditMode::SplitTrim => "Split Trim (dividir e aparar)",
            EditMode::Roll => "Roll (ajuste conjunto de corte)",
            EditMode::Ripple => "Ripple (ondulação)",
        };
        tr(lang, pt)
    }

    pub fn name(self) -> &'static str {
        self.name_in(Language::PtBr)
    }

    pub fn short_name(self) -> &'static str {
        match self {
            EditMode::Normal => "Normal",
            EditMode::Shuffle => "Shuffle",
            EditMode::Slip => "Slip",
            EditMode::Slide => "Slide",
            EditMode::Stretch => "Stretch",
            EditMode::SplitTrim => "Split Trim",
            EditMode::Roll => "Roll",
            EditMode::Ripple => "Ripple",
        }
    }

    pub fn command_id(self) -> &'static str {
        match self {
            EditMode::Normal => "edit.tool.mode.normal",
            EditMode::Shuffle => "edit.tool.mode.shuffle",
            EditMode::Slip => "edit.tool.mode.slip",
            EditMode::Slide => "edit.tool.mode.slide",
            EditMode::Stretch => "edit.tool.mode.stretch",
            EditMode::SplitTrim => "edit.tool.mode.split_trim",
            EditMode::Roll => "edit.tool.mode.roll",
            EditMode::Ripple => "edit.tool.mode.normal",
        }
    }
}

/// Eixo das ferramentas de seleção e zoom em caixa (manual do VEGAS Pro 21, p. 239).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BoxAxisMode {
    /// Livre: seleciona nos dois eixos (largura e altura da caixa).
    #[default]
    Both,
    /// Horizontal: seleciona por intervalo de tempo em todas as trilhas visíveis.
    Horizontal,
    /// Vertical: seleciona eventos das trilhas cruzadas em toda a largura visível.
    Vertical,
}

impl BoxAxisMode {
    pub const ALL: [BoxAxisMode; 3] = [
        BoxAxisMode::Both,
        BoxAxisMode::Horizontal,
        BoxAxisMode::Vertical,
    ];

    pub fn name(self) -> &'static str {
        match self {
            BoxAxisMode::Both => "Livre (horizontal e vertical)",
            BoxAxisMode::Horizontal => "Apenas horizontal",
            BoxAxisMode::Vertical => "Apenas vertical",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            BoxAxisMode::Both => "Livre",
            BoxAxisMode::Horizontal => "Horiz",
            BoxAxisMode::Vertical => "Vert",
        }
    }

    pub fn next(self) -> Self {
        match self {
            BoxAxisMode::Both => BoxAxisMode::Horizontal,
            BoxAxisMode::Horizontal => BoxAxisMode::Vertical,
            BoxAxisMode::Vertical => BoxAxisMode::Both,
        }
    }
}

/// Foco de aparo no Modo de Edição Expandido (manual do VEGAS Pro 21, p. 329).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ExpandedEdgeFocus {
    /// Ajusta ambos os lados do corte ao mesmo tempo (Roll).
    #[default]
    Both,
    /// Ajusta a saída do evento esquerdo deslocando os seguintes (ripple Out).
    LeftOut,
    /// Ajusta a entrada do evento direito.
    RightIn,
}

impl ExpandedEdgeFocus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Both => "Ambos (Roll)",
            Self::LeftOut => "Saída esquerda (Out)",
            Self::RightIn => "Entrada direita (In)",
        }
    }
}

/// Estado do Modo de Edição Expandido (estado de sessão, fora do Desfazer).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpandedEditState {
    pub track_idx: usize,
    pub left_event_id: Option<Id>,
    pub right_event_id: Option<Id>,
    pub edit_point: Ticks,
    pub focus: ExpandedEdgeFocus,
}

/// Máscara de seleção para "Colar atributos do evento" (Paste Event Attributes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PasteAttributesMask {
    pub fx: bool,
    pub pan_crop: bool,
    pub gain_db: bool,
    pub rate: bool,
    pub pitch: bool,
    pub fades: bool,
    pub switches: bool,
    pub transition: bool,
}

impl PasteAttributesMask {
    pub const ALL: Self = Self {
        fx: true,
        pan_crop: true,
        gain_db: true,
        rate: true,
        pitch: true,
        fades: true,
        switches: true,
        transition: true,
    };

    pub fn any(self) -> bool {
        self.fx
            || self.pan_crop
            || self.gain_db
            || self.rate
            || self.pitch
            || self.fades
            || self.switches
            || self.transition
    }
}

impl Default for PasteAttributesMask {
    fn default() -> Self {
        Self::ALL
    }
}

/// Ferramenta de edição ativa na linha do tempo (manual do VEGAS Pro 21,
/// p. 236). É estado da sessão: não vai para o projeto nem para o Desfazer.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EditTool {
    /// Selecionar, mover e aparar eventos (com o modo em `edit_mode`).
    #[default]
    Normal,
    /// Só pontos de envelope; eventos não se movem nem são aparados.
    Envelope,
    /// Caixa de seleção: seleciona os eventos que ela toca.
    Selection,
    /// Caixa de zoom; um clique ajusta o projeto à linha do tempo.
    Zoom,
}

impl EditTool {
    pub const ALL: [EditTool; 4] = [
        EditTool::Normal,
        EditTool::Envelope,
        EditTool::Selection,
        EditTool::Zoom,
    ];

    pub fn name(self) -> &'static str {
        match self {
            EditTool::Normal => "Normal",
            EditTool::Envelope => "Envelope",
            EditTool::Selection => "Seleção",
            EditTool::Zoom => "Zoom",
        }
    }

    /// Próxima ferramenta da lista (D), voltando à primeira depois da última.
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    /// Ferramenta anterior (Shift+D).
    pub fn prev(self) -> Self {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Debug)]
pub struct EditorOptions {
    pub snapping: bool,
    pub quantize: bool,
    pub auto_ripple: bool,
    pub ignore_grouping: bool,
    pub show_envelopes: bool,
    pub show_waveforms: bool,
    pub show_thumbs: bool,
    pub show_bus_tracks: bool,
    pub time_format: TimeFormat,
    pub preview_quality: PreviewQuality,
    pub safe_areas: bool,
    pub loop_playback: bool,
    /// "Slip All Takes" (menu Opções, manual do VEGAS Pro 21 p. 346):
    /// o slip desloca todos os takes do evento, não só o ativo.
    pub slip_all_takes: bool,
    /// Zoom vertical das formas de onda de áudio, em passos de √2 (0 = 1×):
    /// Shift+↑/↓ (lista de atalhos do VEGAS Pro: "Zoom waveforms in/out
    /// vertically (audio only)"). Passos inteiros: subir e descer volta
    /// exatamente a 1×.
    pub waveform_zoom_steps: u8,
    /// Prévia em tela dividida (Desligada, FX ignorados ou Área de transferência).
    pub preview_split_mode: crate::playback::PreviewSplitMode,
    /// Posição da divisória da prévia dividida (`0.08..=0.92`).
    pub preview_split_pos: f32,
    /// Divisão horizontal (cima/baixo) quando `true`, ou vertical (esquerda/direita).
    pub preview_split_horizontal: bool,
    /// Silencia todo o áudio apenas na monitoração da prévia (p. 200).
    pub preview_mute_audio: bool,
    /// Silencia toda a imagem na monitoração da prévia (p. 200).
    pub preview_mute_video: bool,
    /// Modo de edição multicâmera (grade de ângulos na Prévia + corte por tecla 1..9, p. 416).
    pub multicam_mode: bool,
    /// Ignora globalmente todos os efeitos de áudio (eventos, trilhas, barramentos e Master, p. 946).
    pub bypass_all_audio_fx: bool,
    /// Espaçamento da grade da régua da timeline (p. 232).
    pub ruler_grid: RulerGridSpacing,
    /// Encaixar edições na grade da régua (Snap to Grid, Ctrl+F8).
    pub snap_to_grid: bool,
    /// Travar envelopes nos eventos (menu Opções, p. 1014): mover um evento
    /// leva junto os pontos de envelope e os quadros-chave da trilha no
    /// trecho dele.
    pub lock_envelopes: bool,
}

/// Espaçamento configurável da grade vertical da régua da timeline (pp. 86, 232).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum RulerGridSpacing {
    #[default]
    Auto,
    Frame,
    HalfSecond,
    OneSecond,
    TwoSeconds,
    FiveSeconds,
    TenSeconds,
    Beats,
}

impl RulerGridSpacing {
    pub const ALL: [RulerGridSpacing; 8] = [
        RulerGridSpacing::Auto,
        RulerGridSpacing::Frame,
        RulerGridSpacing::HalfSecond,
        RulerGridSpacing::OneSecond,
        RulerGridSpacing::TwoSeconds,
        RulerGridSpacing::FiveSeconds,
        RulerGridSpacing::TenSeconds,
        RulerGridSpacing::Beats,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RulerGridSpacing::Auto => "Automático (zoom)",
            RulerGridSpacing::Frame => "Por quadro",
            RulerGridSpacing::HalfSecond => "0,5 segundo",
            RulerGridSpacing::OneSecond => "1 segundo",
            RulerGridSpacing::TwoSeconds => "2 segundos",
            RulerGridSpacing::FiveSeconds => "5 segundos",
            RulerGridSpacing::TenSeconds => "10 segundos",
            RulerGridSpacing::Beats => "Batidas musicais (BPM)",
        }
    }

    pub fn step_ticks(self, settings: &ProjectSettings) -> Option<Ticks> {
        match self {
            RulerGridSpacing::Auto => None,
            RulerGridSpacing::Frame => Some(settings.frame_dur().max(Ticks::from_raw(1))),
            RulerGridSpacing::HalfSecond => Some(Ticks::from_millis_i64(500)),
            RulerGridSpacing::OneSecond => Some(Ticks::from_secs_i64(1)),
            RulerGridSpacing::TwoSeconds => Some(Ticks::from_secs_i64(2)),
            RulerGridSpacing::FiveSeconds => Some(Ticks::from_secs_i64(5)),
            RulerGridSpacing::TenSeconds => Some(Ticks::from_secs_i64(10)),
            RulerGridSpacing::Beats => {
                let bpm = if settings.bpm.is_finite() {
                    settings.bpm.clamp(20.0, 300.0)
                } else {
                    120.0
                };
                Some(Ticks::from_secs_f64_clamped(60.0 / bpm).max(Ticks::from_raw(1)))
            }
        }
    }
}

/// Maior zoom vertical das formas de onda: 12 passos de √2 = 64× (+36 dB).
pub const MAX_WAVEFORM_ZOOM_STEPS: u8 = 12;

impl EditorOptions {
    /// Fator do zoom vertical das formas de onda (1× a 64×).
    pub fn waveform_zoom(&self) -> f32 {
        let steps = self.waveform_zoom_steps.min(MAX_WAVEFORM_ZOOM_STEPS);
        2f32.powf(f32::from(steps) * 0.5)
    }
}

impl Default for EditorOptions {
    fn default() -> Self {
        Self {
            snapping: true,
            quantize: true,
            auto_ripple: false,
            ignore_grouping: false,
            show_envelopes: true,
            show_waveforms: true,
            show_thumbs: true,
            show_bus_tracks: false,
            time_format: TimeFormat::SmpteDrop,
            preview_quality: PreviewQuality::Preview,
            safe_areas: false,
            loop_playback: false,
            slip_all_takes: false,
            waveform_zoom_steps: 0,
            preview_split_mode: crate::playback::PreviewSplitMode::Off,
            preview_split_pos: 0.5,
            preview_split_horizontal: false,
            preview_mute_audio: false,
            preview_mute_video: false,
            multicam_mode: false,
            bypass_all_audio_fx: false,
            ruler_grid: RulerGridSpacing::Auto,
            snap_to_grid: false,
            lock_envelopes: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ClipItem {
    pub track_rel: usize,
    pub kind: TrackKind,
    pub ev: Event,
}

/// Estado puro da timeline e do projeto compartilhado entre GUI, CLI e scripting.
#[derive(Clone)]
pub struct EditorSession {
    pub project: Project,
    pub snapshot: Arc<Project>,
    pub generation: u64,
    pub sent_generation: u64,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub undo: Vec<Project>,
    pub redo: Vec<Project>,
    pub cursor: Ticks,
    pub play_start: Ticks,
    pub sel_events: HashSet<Id>,
    pub sel_track: Option<usize>,
    pub time_sel: Option<(Ticks, Ticks)>,
    pub clipboard: Vec<ClipItem>,
    pub clip_base: Ticks,
    pub opts: EditorOptions,
    /// Modo da ferramenta Normal (Normal, Shuffle, Slip, Slide, Stretch, SplitTrim, Roll).
    pub edit_mode: EditMode,
    pub lang: Language,
    /// Ferramenta de edição ativa (Normal, Envelope, Seleção, Zoom).
    pub edit_tool: EditTool,
    /// Modo de eixo das ferramentas Seleção e Zoom em caixa (Livre, Horizontal, Vertical).
    pub box_axis: BoxAxisMode,
    /// Modo de edição expandido (prévia dupla A/B e ajuste preciso de corte, p. 329).
    pub expanded_edit: Option<ExpandedEditState>,
    /// Mídia atualmente focada no painel Mídia do Projeto (para comandos de Substituir / Selecionar).
    pub active_media_id: Option<Id>,
    /// Storyboard atualmente selecionado na aba Storyboard.
    pub active_storyboard_id: Option<Id>,
    pub status: String,
    pub removed_media_ids: Vec<Id>,
    /// Intervalos da timeline pré-renderizados (`Shift+M`), limpos ao editar o projeto.
    pub prerendered_ranges: Vec<(Ticks, Ticks)>,
    /// Conta só as edições do projeto (`changed`, desfazer e refazer), não
    /// trocas de aba nem mídia recarregada: decide se um projeto aninhado
    /// editado em aba precisa ser reconstruído no projeto principal.
    pub edit_count: u64,
    /// Esta sessão alimenta o arquivo de recuperação em caso de travamento.
    /// Falso nas sessões de projetos aninhados abertos em aba: a recuperação
    /// guarda sempre o projeto principal.
    pub tracks_recovery: bool,
}

impl EditorSession {
    pub fn new(project: Project) -> Self {
        Self::new_in(project, neoveg_common::active_language())
    }

    pub fn new_in(project: Project, lang: Language) -> Self {
        Self::build(project, lang, true)
    }

    /// Sessão de um projeto aninhado aberto em aba: não substitui o projeto
    /// principal no arquivo de recuperação.
    pub fn new_nested(project: Project, lang: Language) -> Self {
        Self::build(project, lang, false)
    }

    fn build(mut project: Project, lang: Language, tracks_recovery: bool) -> Self {
        project.ensure_sequences();
        let snapshot = Arc::new(project.clone());
        if tracks_recovery {
            crate::recovery::remember_snapshot(snapshot.clone());
        }
        let sel_track = if project.tracks.is_empty() {
            None
        } else {
            Some(0)
        };
        let active_storyboard_id = project.storyboards.first().map(|s| s.id);
        Self {
            project,
            snapshot,
            generation: 1,
            sent_generation: 0,
            path: None,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            cursor: Ticks::ZERO,
            play_start: Ticks::ZERO,
            sel_events: HashSet::new(),
            sel_track,
            time_sel: None,
            clipboard: Vec::new(),
            clip_base: Ticks::ZERO,
            opts: EditorOptions::default(),
            edit_mode: EditMode::Normal,
            lang,
            edit_tool: EditTool::Normal,
            box_axis: BoxAxisMode::Both,
            expanded_edit: None,
            active_media_id: None,
            active_storyboard_id,
            status: tr(
                lang,
                "Pronto. Arraste arquivos de mídia para a timeline ou use Ctrl+I.",
            )
            .into(),
            removed_media_ids: Vec::new(),
            prerendered_ranges: Vec::new(),
            edit_count: 0,
            tracks_recovery,
        }
    }

    /// Registra um trecho pré-renderizado e mescla intervalos adjacentes/sobrepostos.
    pub fn record_prerendered_range(&mut self, start: Ticks, end: Ticks) {
        let a = start.max(Ticks::ZERO);
        if end <= a {
            return;
        }
        self.prerendered_ranges.push((a, end));
        self.prerendered_ranges.sort_by_key(|(s, _)| *s);
        let mut merged: Vec<(Ticks, Ticks)> = Vec::with_capacity(self.prerendered_ranges.len());
        for (s, e) in self.prerendered_ranges.drain(..) {
            if let Some(last) = merged.last_mut() {
                if s <= last.1 {
                    last.1 = last.1.max(e);
                    continue;
                }
            }
            merged.push((s, e));
        }
        if merged.len() > 256 {
            merged.drain(..merged.len() - 256);
        }
        self.prerendered_ranges = merged;
    }

    pub fn clear_prerendered(&mut self) {
        self.prerendered_ranges.clear();
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
    }

    /// Salva um ponto de desfazer antes de uma modificação.
    pub fn checkpoint(&mut self) {
        self.undo.push(self.project.clone());
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Marca o projeto como modificado (incrementa geração para prévia e áudio).
    pub fn changed(&mut self) {
        self.generation += 1;
        self.edit_count = self.edit_count.wrapping_add(1);
        self.dirty = true;
        self.prerendered_ranges.clear();
    }

    /// Atualiza o snapshot imutável quando `generation != sent_generation`.
    pub fn refresh_snapshot(&mut self) -> bool {
        if self.generation != self.sent_generation {
            self.snapshot = Arc::new(self.project.clone());
            self.sent_generation = self.generation;
            if self.tracks_recovery {
                crate::recovery::remember_snapshot(self.snapshot.clone());
            }
            true
        } else {
            false
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(p) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(&mut self.project, p));
        self.edit_count = self.edit_count.wrapping_add(1);
        self.after_history();
        self.set_status(tr(self.lang, "Desfazer"));
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(p) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.project, p));
        self.edit_count = self.edit_count.wrapping_add(1);
        self.after_history();
        self.set_status(tr(self.lang, "Refazer"));
        true
    }

    fn after_history(&mut self) {
        self.sel_events
            .retain(|id| self.project.find_event(*id).is_some());
        if let Some(t) = self.sel_track {
            if t >= self.project.tracks.len() {
                self.sel_track = if self.project.tracks.is_empty() {
                    None
                } else {
                    Some(0)
                };
            }
        }
        self.generation += 1;
        self.dirty = true;
        self.prerendered_ranges.clear();
    }

    pub fn reset_project(&mut self, mut project: Project, path: Option<PathBuf>) {
        project.ensure_sequences();
        self.project = project;
        self.path = path;
        self.dirty = false;
        self.undo.clear();
        self.redo.clear();
        self.cursor = Ticks::ZERO;
        self.play_start = Ticks::ZERO;
        self.sel_events.clear();
        self.time_sel = None;
        self.prerendered_ranges.clear();
        self.sel_track = if self.project.tracks.is_empty() {
            None
        } else {
            Some(0)
        };
        self.generation += 1;
        self.sent_generation = 0;
        self.refresh_snapshot();
    }

    /// Alterna para outra sequência pelo ID, sincronizando o estado atual e
    /// reiniciando cursor e seleções da timeline.
    pub fn switch_sequence(&mut self, seq_id: Id) -> bool {
        if self.project.switch_sequence(seq_id) {
            self.cursor = Ticks::ZERO;
            self.sel_events.clear();
            self.sel_track = if self.project.tracks.is_empty() {
                None
            } else {
                Some(0)
            };
            self.time_sel = None;
            self.refresh_snapshot();
            self.changed();
            true
        } else {
            false
        }
    }

    /// Alterna para outra sequência pelo ID preservando parâmetros de visualização (cursor, scroll_x, px_per_sec).
    pub fn switch_sequence_with_view(
        &mut self,
        seq_id: Id,
        scroll_x: Ticks,
        px_per_sec: f32,
    ) -> Option<(Ticks, Ticks, f32)> {
        let view =
            self.project
                .switch_sequence_with_view(seq_id, self.cursor, scroll_x, px_per_sec)?;
        self.cursor = view.0;
        self.sel_events.clear();
        self.sel_track = if self.project.tracks.is_empty() {
            None
        } else {
            Some(0)
        };
        self.time_sel = None;
        self.refresh_snapshot();
        self.changed();
        Some(view)
    }

    /// Cria uma nova sequência no projeto e a torna ativa.
    pub fn create_sequence(&mut self, name: Option<&str>) -> Id {
        self.checkpoint();
        let id = self.project.create_sequence(name);
        self.cursor = Ticks::ZERO;
        self.sel_events.clear();
        self.sel_track = if self.project.tracks.is_empty() {
            None
        } else {
            Some(0)
        };
        self.time_sel = None;
        self.refresh_snapshot();
        self.changed();
        id
    }

    /// Duplica uma sequência existente com todo o seu conteúdo.
    pub fn duplicate_sequence(&mut self, seq_id: Id) -> Option<Id> {
        self.checkpoint();
        let id = self.project.duplicate_sequence(seq_id);
        if id.is_some() {
            self.refresh_snapshot();
            self.changed();
        }
        id
    }

    /// Remove uma sequência se houver mais de uma no projeto.
    pub fn delete_sequence(&mut self, seq_id: Id) -> bool {
        if self.project.sequences.is_empty() {
            return false;
        }
        self.checkpoint();
        if self.project.delete_sequence(seq_id) {
            self.cursor = Ticks::ZERO;
            self.sel_events.clear();
            self.sel_track = if self.project.tracks.is_empty() {
                None
            } else {
                Some(0)
            };
            self.time_sel = None;
            self.refresh_snapshot();
            self.changed();
            true
        } else {
            false
        }
    }

    /// Renomeia uma sequência pelo ID.
    pub fn rename_sequence(&mut self, seq_id: Id, new_name: &str) -> bool {
        self.checkpoint();
        if self.project.rename_sequence(seq_id, new_name) {
            self.refresh_snapshot();
            self.changed();
            true
        } else {
            false
        }
    }

    /// Renomeia a sequência ativa atual.
    pub fn rename_active_sequence(&mut self, new_name: &str) -> bool {
        let Some(id) = self.project.active_sequence_id else {
            return false;
        };
        self.rename_sequence(id, new_name)
    }

    /// Aninha a seleção atual de eventos numa nova sequência (Premiere Pro ref. pp. 290–291).
    ///
    /// Remove os eventos selecionados da timeline ativa, cria uma nova [`Sequence`],
    /// coloca nela os eventos selecionados (com offset ajustado para iniciarem em Ticks::ZERO),
    /// e insere na timeline ativa um clipe apontando para a nova sequência no intervalo exato.
    pub fn nest_selection(&mut self, custom_name: Option<&str>) -> Result<Id> {
        if self.sel_events.is_empty() {
            return Err(NeovegError::InvalidInput(
                tr(self.lang, "Nenhum evento selecionado para aninhar").to_string(),
            ));
        }

        self.project.ensure_sequences();
        if self.project.sequences.len() >= MAX_SEQUENCES {
            return Err(NeovegError::InvalidInput(
                tr(self.lang, "Limite máximo de sequências atingido").to_string(),
            ));
        }

        // 1. Localiza todos os eventos selecionados nas trilhas ativas
        let mut selected_events = Vec::new();
        let mut min_start = Ticks::MAX;
        let mut max_end = Ticks::ZERO;
        let mut has_video = false;
        let mut has_audio = false;

        for (ti, t) in self.project.tracks.iter().enumerate() {
            for (ei, e) in t.events.iter().enumerate() {
                if self.sel_events.contains(&e.id) {
                    selected_events.push((ti, ei, e.clone(), t.kind));
                    min_start = min_start.min(e.start);
                    max_end = max_end.max(e.end());
                    if t.kind == TrackKind::Video {
                        has_video = true;
                    } else if t.kind == TrackKind::Audio {
                        has_audio = true;
                    }
                }
            }
        }

        if selected_events.is_empty() || min_start >= max_end {
            return Err(NeovegError::InvalidInput(
                tr(self.lang, "Seleção inválida para aninhar").to_string(),
            ));
        }

        let nest_dur = max_end - min_start;
        self.checkpoint();

        // 2. Cria a nova sequência com id estável
        let new_seq_id = new_id();
        let seq_num = self.project.sequences.len() + 2;
        let seq_name = custom_name
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("Sequência Aninhada {seq_num:02}"));

        // 3. Monta as trilhas da nova sequência e remapeia IDs (§3.1)
        let mut new_tracks = Vec::new();
        let mut track_idx_map: HashMap<usize, usize> = HashMap::new();
        let mut event_id_map: HashMap<Id, Id> = HashMap::new();

        for &(orig_ti, _, ref ev, _) in &selected_events {
            event_id_map.insert(ev.id, new_id());
            if let std::collections::hash_map::Entry::Vacant(e) = track_idx_map.entry(orig_ti) {
                let orig_track = &self.project.tracks[orig_ti];
                let mut new_track = Track::new(orig_track.kind, &orig_track.name);
                new_track.id = new_id();
                e.insert(new_tracks.len());
                new_tracks.push(new_track);
            }
        }

        for &(orig_ti, _, ref ev, _) in &selected_events {
            let target_ti = track_idx_map[&orig_ti];
            let mut new_ev = ev.clone();
            new_ev.id = event_id_map[&ev.id];
            new_ev.start = if ev.start >= min_start {
                ev.start - min_start
            } else {
                Ticks::ZERO
            };
            if let Some(orig_master) = ev.sync_link_master {
                new_ev.sync_link_master = event_id_map.get(&orig_master).copied();
            }
            new_tracks[target_ti].events.push(new_ev);
        }

        // 4. Remove os eventos selecionados das trilhas ativas
        for t in &mut self.project.tracks {
            t.events.retain(|e| !self.sel_events.contains(&e.id));
        }

        // 5. Cria o MediaItem correspondente à sequência aninhada no projeto
        let media_id = new_id();
        let media_item = MediaItem {
            id: media_id,
            name: seq_name.clone(),
            source: MediaSource::Sequence { id: new_seq_id },
            duration: Some(nest_dur),
            has_video,
            has_audio,
            width: self.project.settings.width,
            height: self.project.settings.height,
            fps: Some(self.project.settings.fps),
            info: format!("Sequência aninhada {seq_name}"),
            proxy_path: None,
        };
        self.project.media.push(media_item.clone());

        // 6. Insere o clipe aninhado na timeline ativa na posição min_start com duração nest_dur
        let mut new_sel = HashSet::new();
        let mut created_video_ev: Option<Id> = None;
        let mut created_audio_ev: Option<Id> = None;

        if has_video {
            let v_track_idx = selected_events
                .iter()
                .find(|(_, _, _, k)| *k == TrackKind::Video)
                .map(|(ti, _, _, _)| *ti)
                .or_else(|| {
                    self.project
                        .tracks
                        .iter()
                        .position(|t| t.kind == TrackKind::Video)
                });

            if let Some(ti) = v_track_idx {
                let mut ev = Event::new(&media_item, min_start, nest_dur);
                ev.name = seq_name.clone();
                let ev_id = ev.id;
                created_video_ev = Some(ev_id);
                self.project.tracks[ti].events.push(ev);
                new_sel.insert(ev_id);
            }
        }

        if has_audio {
            let a_track_idx = selected_events
                .iter()
                .find(|(_, _, _, k)| *k == TrackKind::Audio)
                .map(|(ti, _, _, _)| *ti)
                .or_else(|| {
                    self.project
                        .tracks
                        .iter()
                        .position(|t| t.kind == TrackKind::Audio)
                });

            if let Some(ti) = a_track_idx {
                let mut ev = Event::new(&media_item, min_start, nest_dur);
                ev.name = seq_name.clone();
                let ev_id = ev.id;
                created_audio_ev = Some(ev_id);
                self.project.tracks[ti].events.push(ev);
                new_sel.insert(ev_id);
            }
        }

        if let (Some(vid_id), Some(aud_id)) = (created_video_ev, created_audio_ev) {
            for t in &mut self.project.tracks {
                for e in &mut t.events {
                    if e.id == aud_id {
                        e.sync_link_master = Some(vid_id);
                    }
                }
            }
        }

        // 7. Salva a nova sequência em self.project.sequences
        let new_sequence = Sequence {
            id: new_seq_id,
            name: seq_name,
            tracks: new_tracks,
            markers: Vec::new(),
            regions: Vec::new(),
            loop_region: None,
            cursor: Ticks::ZERO,
            scroll_x: Ticks::ZERO,
            px_per_sec: 100.0,
            transcript: None,
        };
        self.project.sequences.push(new_sequence);
        self.project.sequence_order.push(new_seq_id);
        self.project.format_version = 2;

        self.sel_events = new_sel;
        self.refresh_snapshot();
        self.changed();

        Ok(new_seq_id)
    }

    /// Retorna as pastas de trabalho configuradas para o projeto.
    pub fn work_folders(&self) -> &crate::model::WorkFolders {
        &self.project.work_folders
    }

    /// Atualiza as pastas de trabalho do projeto em um passo de Desfazer (§3.3).
    pub fn set_work_folders(&mut self, mut folders: crate::model::WorkFolders) {
        folders.sanitize();
        if self.project.work_folders == folders {
            return;
        }
        self.checkpoint();
        self.project.work_folders = folders;
        self.refresh_snapshot();
        self.changed();
    }

    pub fn insert_track(&mut self, kind: TrackKind) -> usize {
        let n = self
            .project
            .tracks
            .iter()
            .filter(|t| t.kind == kind)
            .count()
            + 1;
        let name = match kind {
            TrackKind::Video => tr_args(self.lang, "Vídeo {n}", &[("n", &n)]),
            TrackKind::Audio => tr_args(self.lang, "Áudio {n}", &[("n", &n)]),
        };
        let t = Track::new(kind, &name);
        // vídeo: insere acima das trilhas de áudio; áudio: no final
        let idx = match kind {
            TrackKind::Video => match self.sel_track {
                Some(i) if self.project.tracks.get(i).map(|t| t.kind) == Some(TrackKind::Video) => {
                    i
                }
                _ => self
                    .project
                    .tracks
                    .iter()
                    .position(|t| t.kind == TrackKind::Audio)
                    .unwrap_or(self.project.tracks.len()),
            },
            TrackKind::Audio => self.project.tracks.len(),
        };
        self.project.tracks.insert(idx, t);
        self.sel_track = Some(idx);
        self.changed();
        idx
    }

    /// Insere uma trilha de ajuste (*Adjustment Track*) de vídeo, na mesma
    /// posição em que `insert_track(TrackKind::Video)` colocaria uma trilha.
    pub fn insert_adjustment_track(&mut self) -> usize {
        let n = self
            .project
            .tracks
            .iter()
            .filter(|t| t.is_adjustment())
            .count()
            + 1;
        let idx = self.insert_track(TrackKind::Video);
        if let Some(t) = self.project.tracks.get_mut(idx) {
            t.adjustment = true;
            t.name = tr_args(self.lang, "Ajuste {n}", &[("n", &n)]);
        }
        self.changed();
        idx
    }

    pub fn add_audio_bus(&mut self) -> Option<Id> {
        if self.project.audio_buses.len() >= MAX_AUDIO_BUSES {
            return None;
        }
        self.checkpoint();
        let id = self.project.add_audio_bus(None)?;
        self.changed();
        Some(id)
    }

    pub fn remove_audio_bus(&mut self, bus_id: Id) -> bool {
        let Some(pos) = self.project.audio_buses.iter().position(|b| b.id == bus_id) else {
            return false;
        };
        self.checkpoint();
        self.project.audio_buses.remove(pos);
        for t in &mut self.project.tracks {
            if t.output_bus == Some(bus_id) {
                t.output_bus = None;
            }
            t.sends.retain(|s| s.bus_id != bus_id);
        }
        for b in &mut self.project.audio_buses {
            if b.output_bus == Some(bus_id) {
                b.output_bus = None;
            }
        }
        self.changed();
        true
    }

    pub fn rename_audio_bus(&mut self, bus_id: Id, name: String) -> bool {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return false;
        }
        let Some(bus) = self.project.audio_bus_mut(bus_id) else {
            return false;
        };
        if bus.name == trimmed {
            return false;
        }
        let new_name = trimmed.to_string();
        self.checkpoint();
        if let Some(bus) = self.project.audio_bus_mut(bus_id) {
            bus.name = new_name;
        }
        self.changed();
        true
    }

    pub fn toggle_bus_envelope(&mut self, bus_id: Id, kind: EnvKind) -> bool {
        if !matches!(kind, EnvKind::Volume | EnvKind::Pan) {
            return false;
        }
        let Some(pos) = self.project.audio_buses.iter().position(|b| b.id == bus_id) else {
            return false;
        };
        self.checkpoint();
        let bus = &mut self.project.audio_buses[pos];
        if bus.envelope(kind).is_some() {
            bus.envelopes.retain(|e| e.kind != kind);
        } else {
            bus.envelopes.push(Envelope::new(kind));
            self.opts.show_envelopes = true;
            self.opts.show_bus_tracks = true;
        }
        self.changed();
        true
    }

    /// Ids dos eventos selecionados expandidos pelos grupos (a menos que o
    /// agrupamento esteja sendo ignorado).
    pub fn expand_groups(&self, ids: &HashSet<Id>) -> HashSet<Id> {
        if self.opts.ignore_grouping {
            return ids.clone();
        }
        let groups: HashSet<Id> = ids
            .iter()
            .filter_map(|id| self.project.event(*id).and_then(|e| e.group))
            .collect();
        let mut out = ids.clone();
        if groups.is_empty() {
            return out;
        }
        for t in &self.project.tracks {
            for e in &t.events {
                if e.group.map(|g| groups.contains(&g)).unwrap_or(false) {
                    out.insert(e.id);
                }
            }
        }
        out
    }

    pub fn copy_selection(&mut self) {
        if self.sel_events.is_empty() {
            return;
        }
        let mut items = vec![];
        let mut min_track = usize::MAX;
        let mut min_t = Ticks::MAX;
        let mut any = false;
        for (ti, t) in self.project.tracks.iter().enumerate() {
            for e in &t.events {
                if self.sel_events.contains(&e.id) {
                    min_track = min_track.min(ti);
                    min_t = min_t.min(e.start);
                    any = true;
                }
            }
        }
        if !any {
            return;
        }
        for (ti, t) in self.project.tracks.iter().enumerate() {
            for e in &t.events {
                if self.sel_events.contains(&e.id) {
                    items.push(ClipItem {
                        track_rel: ti - min_track,
                        kind: t.kind,
                        ev: e.clone(),
                    });
                }
            }
        }
        self.clip_base = min_t;
        let count = items.len();
        self.set_status(tr_args(
            self.lang,
            "{n} evento(s) copiado(s)",
            &[("n", &count)],
        ));
        self.clipboard = items;
    }

    pub fn cut_selection(&mut self) {
        self.copy_selection();
        self.delete_selection();
    }

    /// Cola no cursor, na trilha selecionada. `insert` = desloca eventos posteriores (paste insert).
    pub fn paste(&mut self, insert: bool) {
        if self.clipboard.is_empty() {
            return;
        }
        self.checkpoint();
        let base_track = self.sel_track.unwrap_or(0);
        let t0 = self.cursor;
        let max_end = self
            .clipboard
            .iter()
            .map(|c| c.ev.end())
            .fold(self.clip_base, Ticks::max);
        let span = max_end - self.clip_base;
        let mut group_map: HashMap<Id, Id> = HashMap::new();
        let mut new_sel = HashSet::new();
        let items = self.clipboard.clone();
        if insert {
            let tracks: HashSet<usize> = items
                .iter()
                .filter_map(|c| self.target_track(base_track + c.track_rel, c.kind))
                .collect();
            for ti in tracks {
                if let Some(track) = self.project.tracks.get_mut(ti) {
                    for e in &mut track.events {
                        if e.start >= t0 {
                            e.start += span;
                        }
                    }
                }
            }
        }
        for c in items {
            let ti = match self.target_track(base_track + c.track_rel, c.kind) {
                Some(t) => t,
                None => self.insert_track(c.kind),
            };
            let mut e = c.ev.clone();
            e.id = new_id();
            e.start = t0 + (c.ev.start - self.clip_base);
            e.group = e.group.map(|g| *group_map.entry(g).or_insert_with(new_id));
            e.fx = e.fx.iter().map(|f| f.duplicate()).collect();
            new_sel.insert(e.id);
            if let Some(track) = self.project.tracks.get_mut(ti) {
                track.events.push(e);
            }
        }
        self.sel_events = new_sel;
        self.cursor = t0 + span;
        self.changed();
    }

    fn target_track(&self, idx: usize, kind: TrackKind) -> Option<usize> {
        (idx..self.project.tracks.len()).find(|&i| {
            self.project
                .tracks
                .get(i)
                .map(|t| t.accepts(kind))
                .unwrap_or(false)
        })
    }

    pub fn delete_selection(&mut self) {
        if self.sel_events.is_empty() {
            if self.time_sel.is_some() && self.opts.auto_ripple {
                self.delete_time_range();
            }
            return;
        }
        self.checkpoint();
        let ids = self.expand_groups(&self.sel_events.clone());
        let ripple = self.opts.auto_ripple;
        for t in &mut self.project.tracks {
            let removed: Vec<(Ticks, Ticks)> = t
                .events
                .iter()
                .filter(|e| ids.contains(&e.id))
                .map(|e| (e.start, e.length))
                .collect();
            t.events.retain(|e| !ids.contains(&e.id));
            if ripple {
                let mut removed = removed;
                removed.sort_by_key(|a| std::cmp::Reverse(a.0));
                for (start, len) in removed {
                    for e in &mut t.events {
                        if e.start >= start + len {
                            e.start -= len;
                        }
                    }
                }
            }
        }
        self.sel_events.clear();
        self.changed();
    }

    /// Divide eventos no cursor (selecionados, ou todos sob o cursor se nada
    /// estiver selecionado). Retorna o número de eventos divididos.
    /// Aplica `f` a cada evento selecionado (sem expandir grupos: os takes
    /// são de cada evento). Um passo de Desfazer se algum evento mudou.
    fn edit_selected_takes(&mut self, f: impl Fn(&mut Event) -> bool) -> usize {
        let ids: Vec<Id> = self.sel_events.iter().copied().collect();
        let mut probe = self.project.clone();
        let mut n = 0;
        for id in ids {
            if let Some(e) = probe.event_mut(id) {
                if !e.locked && f(e) {
                    n += 1;
                }
            }
        }
        if n > 0 {
            self.checkpoint();
            self.project = probe;
            self.changed();
        }
        n
    }

    /// Próximo take dos eventos selecionados (T).
    pub fn next_take_selected(&mut self) -> usize {
        self.edit_selected_takes(crate::takes::next_take)
    }

    /// Take anterior dos eventos selecionados (Shift+T).
    pub fn prev_take_selected(&mut self) -> usize {
        self.edit_selected_takes(crate::takes::prev_take)
    }

    /// Apaga o take ativo dos eventos selecionados que têm mais de um take.
    pub fn delete_active_take_selected(&mut self) -> usize {
        self.edit_selected_takes(crate::takes::delete_active)
    }

    /// Todos os eventos selecionados (pelo menos um) estão travados?
    pub fn selection_all_locked(&self) -> bool {
        !self.sel_events.is_empty()
            && self
                .sel_events
                .iter()
                .all(|id| self.project.event(*id).is_some_and(|e| e.locked))
    }

    /// Botão Travar (Lock) da barra de transporte e edição (manual do VEGAS
    /// Pro 21, p. 77): trava os eventos selecionados; se todos já estão
    /// travados, destrava. Um passo de Desfazer; devolve quantos mudaram.
    pub fn toggle_lock_selected(&mut self) -> usize {
        let lock = !self.selection_all_locked();
        let ids: Vec<Id> = self.sel_events.iter().copied().collect();
        let mut probe = self.project.clone();
        let mut n = 0;
        for id in ids {
            if let Some(e) = probe.event_mut(id) {
                if e.locked != lock {
                    e.locked = lock;
                    n += 1;
                }
            }
        }
        if n > 0 {
            self.checkpoint();
            self.project = probe;
            self.changed();
        }
        n
    }

    /// Algum evento selecionado tem mais de um take?
    pub fn selection_has_takes(&self) -> bool {
        self.sel_events.iter().any(|id| {
            self.project
                .event(*id)
                .is_some_and(|e| crate::takes::take_count(e) > 1)
        })
    }

    /// "Add As Takes" (p. 346): adiciona a mídia como take ativo de cada
    /// evento selecionado do mesmo tipo (vídeo/áudio); o take anterior fica.
    pub fn add_media_as_take_to_selected(&mut self, media_id: Id) -> usize {
        let Some(m) = self.project.media(media_id).cloned() else {
            return 0;
        };
        let ids: Vec<Id> = self.sel_events.iter().copied().collect();
        let mut probe = self.project.clone();
        let mut n = 0;
        for t in &mut probe.tracks {
            let fits = match t.kind {
                TrackKind::Video => m.has_video,
                TrackKind::Audio => m.has_audio,
            };
            if !fits {
                continue;
            }
            for e in &mut t.events {
                if ids.contains(&e.id) && !e.locked {
                    let take = crate::model::Take {
                        media_id,
                        offset: Ticks::ZERO,
                        name: String::new(),
                    };
                    if crate::takes::add_take(e, take, true) {
                        n += 1;
                    }
                }
            }
        }
        if n > 0 {
            self.checkpoint();
            self.project = probe;
            self.changed();
        }
        n
    }

    /// Adiciona a mídia como take ativo do evento indicado. O take anterior continua.
    pub fn add_media_as_take(&mut self, event_id: Id, media_id: Id) -> bool {
        let Some(m) = self.project.media(media_id).cloned() else {
            return false;
        };
        let mut probe = self.project.clone();
        let mut ok = false;
        for t in &mut probe.tracks {
            let fits = match t.kind {
                TrackKind::Video => m.has_video,
                TrackKind::Audio => m.has_audio,
            };
            if !fits {
                continue;
            }
            for e in &mut t.events {
                if e.id == event_id && !e.locked {
                    let take = crate::model::Take {
                        media_id,
                        offset: Ticks::ZERO,
                        name: String::new(),
                    };
                    if crate::takes::add_take(e, take, true) {
                        ok = true;
                        break;
                    }
                }
            }
            if ok {
                break;
            }
        }
        if ok {
            self.checkpoint();
            self.project = probe;
            self.changed();
        }
        ok
    }

    /// Torna ativo o take `k` do evento `id` (escolha no submenu Take).
    pub fn set_active_take(&mut self, id: Id, k: usize) -> bool {
        let mut probe = self.project.clone();
        let changed = probe
            .event_mut(id)
            .is_some_and(|e| !e.locked && crate::takes::set_active(e, k));
        if changed {
            self.checkpoint();
            self.project = probe;
            self.changed();
        }
        changed
    }

    pub fn split_at_cursor(&mut self) -> usize {
        let t = self.cursor;
        let targets: HashSet<Id> = if self.sel_events.is_empty() {
            self.project
                .tracks
                .iter()
                .flat_map(|tr| tr.events.iter())
                .filter(|e| e.start < t && e.end() > t)
                .map(|e| e.id)
                .collect()
        } else {
            self.expand_groups(&self.sel_events.clone())
        };
        let targets: Vec<Id> = targets
            .into_iter()
            .filter(|id| {
                self.project
                    .event(*id)
                    .map(|e| e.start < t && e.end() > t && !e.locked)
                    .unwrap_or(false)
            })
            .collect();
        if targets.is_empty() {
            return 0;
        }
        self.checkpoint();
        let mut group_map: HashMap<Id, Id> = HashMap::new();
        let mut new_sel = HashSet::new();
        let mut count = 0usize;
        for id in targets {
            let Some((ti, ei)) = self.project.find_event(id) else {
                continue;
            };
            let Some(track) = self.project.tracks.get_mut(ti) else {
                continue;
            };
            if let Some(right_id) = split_event_in_track(track, ei, t, &mut group_map) {
                new_sel.insert(right_id);
                count += 1;
            }
        }
        self.sel_events = new_sel;
        self.changed();
        count
    }

    pub fn trim_to_selection(&mut self) {
        let Some((a, b)) = self.time_sel else { return };
        if self.sel_events.is_empty() || b <= a {
            return;
        }
        self.checkpoint();
        let ids = self.expand_groups(&self.sel_events.clone());
        for t in &mut self.project.tracks {
            t.events.retain_mut(|e| {
                if !ids.contains(&e.id) {
                    return true;
                }
                if e.end() <= a || e.start >= b {
                    return false;
                }
                if e.start < a {
                    let d = a - e.start;
                    if !e.reverse {
                        e.shift_offsets(e.rate.scale_ticks(d));
                    }
                    e.start = a;
                    e.length -= d;
                }
                if e.end() > b {
                    let d = e.end() - b;
                    if e.reverse {
                        e.shift_offsets(e.rate.scale_ticks(d));
                    }
                    e.length -= d;
                }
                true
            });
        }
        self.changed();
    }

    pub fn trim_selected_start_to_cursor(&mut self) {
        if self.sel_events.is_empty() {
            return;
        }
        let c = self.cursor;
        self.checkpoint();
        let ids = self.expand_groups(&self.sel_events.clone());
        for id in ids {
            if let Some(e) = self.project.event_mut(id) {
                if c > e.start && c < e.end() {
                    let d = c - e.start;
                    if !e.reverse {
                        e.shift_offsets(e.rate.scale_ticks(d));
                    }
                    e.start = c;
                    e.length -= d;
                }
            }
        }
        self.changed();
    }

    pub fn trim_selected_end_to_cursor(&mut self) {
        if self.sel_events.is_empty() {
            return;
        }
        let c = self.cursor;
        self.checkpoint();
        let ids = self.expand_groups(&self.sel_events.clone());
        for id in ids {
            if let Some(e) = self.project.event_mut(id) {
                if c > e.start && c < e.end() {
                    let d = e.end() - c;
                    if e.reverse {
                        e.shift_offsets(e.rate.scale_ticks(d));
                    }
                    e.length = c - e.start;
                }
            }
        }
        self.changed();
    }

    /// Remove o intervalo da seleção de tempo de todas as trilhas (ripple).
    pub fn delete_time_range(&mut self) {
        let Some((a, b)) = self.time_sel else { return };
        if b <= a {
            return;
        }
        self.checkpoint();
        let len = b - a;
        let mut new_events: Vec<(usize, Event)> = vec![];
        for (ti, t) in self.project.tracks.iter_mut().enumerate() {
            let mut keep = vec![];
            for mut e in t.events.drain(..) {
                if e.end() <= a {
                    keep.push(e);
                } else if e.start >= b {
                    e.start -= len;
                    keep.push(e);
                } else if e.start >= a && e.end() <= b {
                    // removido
                } else if e.start < a && e.end() > b {
                    let mut right = e.clone();
                    right.id = new_id();
                    let cut = b - e.start;
                    right.start = a;
                    right.length = e.end() - b;
                    right.shift_offsets(right.rate.scale_ticks(cut));
                    right.fade_in = Fade::default();
                    e.length = a - e.start;
                    e.fade_out = Fade::default();
                    keep.push(e);
                    new_events.push((ti, right));
                } else if e.start < a {
                    e.length = a - e.start;
                    keep.push(e);
                } else {
                    let d = b - e.start;
                    e.shift_offsets(e.rate.scale_ticks(d));
                    e.length -= d;
                    e.start = a;
                    keep.push(e);
                }
            }
            t.events = keep;
        }
        for (ti, e) in new_events {
            if let Some(track) = self.project.tracks.get_mut(ti) {
                track.events.push(e);
            }
        }
        self.project.markers.retain(|m| m.t < a || m.t >= b);
        for m in &mut self.project.markers {
            if m.t >= b {
                m.t -= len;
            }
        }
        self.time_sel = None;
        self.cursor = a;
        self.changed();
    }

    /// Insere tempo vazio do tamanho da seleção (Insert Time).
    pub fn insert_time(&mut self) {
        let Some((a, b)) = self.time_sel else {
            self.set_status(tr(
                self.lang,
                "Crie uma seleção de tempo para inserir tempo",
            ));
            return;
        };
        if b <= a {
            return;
        }
        self.checkpoint();
        let len = b - a;
        for t in &mut self.project.tracks {
            let mut extra = vec![];
            for e in &mut t.events {
                if e.start >= a {
                    e.start += len;
                } else if e.end() > a {
                    let mut right = e.clone();
                    right.id = new_id();
                    let cut = a - e.start;
                    right.start = b;
                    right.length = e.length - cut;
                    right.shift_offsets(right.rate.scale_ticks(cut));
                    right.fade_in = Fade::default();
                    e.length = cut;
                    e.fade_out = Fade::default();
                    extra.push(right);
                }
            }
            t.events.extend(extra);
        }
        for m in &mut self.project.markers {
            if m.t >= a {
                m.t += len;
            }
        }
        self.changed();
    }

    /// Fecha lacunas entre eventos da trilha selecionada.
    pub fn close_gaps(&mut self) {
        let Some(ti) = self.sel_track else { return };
        if ti >= self.project.tracks.len() {
            return;
        }
        self.checkpoint();
        let Some(t) = self.project.tracks.get_mut(ti) else {
            return;
        };
        t.events.sort_by_key(|e| e.start);
        let mut pos = t.events.first().map(|e| e.start).unwrap_or(Ticks::ZERO);
        for e in &mut t.events {
            if e.start > pos {
                e.start = pos;
            }
            pos = pos.max(e.end());
        }
        self.changed();
    }

    pub fn group_selected(&mut self) {
        if self.sel_events.len() < 2 {
            return;
        }
        self.checkpoint();
        let g = new_id();
        for id in self.sel_events.clone() {
            if let Some(e) = self.project.event_mut(id) {
                e.group = Some(g);
            }
        }
        self.set_status(tr(self.lang, "Eventos agrupados"));
        self.changed();
    }

    pub fn ungroup_selected(&mut self) {
        if self.sel_events.is_empty() {
            return;
        }
        self.checkpoint();
        for id in self.sel_events.clone() {
            if let Some(e) = self.project.event_mut(id) {
                e.group = None;
            }
        }
        self.set_status(tr(self.lang, "Eventos desagrupados"));
        self.changed();
    }

    pub fn select_group(&mut self) {
        let saved = self.opts.ignore_grouping;
        self.opts.ignore_grouping = false;
        self.sel_events = self.expand_groups(&self.sel_events.clone());
        self.opts.ignore_grouping = saved;
    }

    pub fn select_all(&mut self) {
        self.sel_events = self
            .project
            .tracks
            .iter()
            .flat_map(|t| t.events.iter().map(|e| e.id))
            .collect();
    }

    pub fn deselect_all(&mut self) {
        self.sel_events.clear();
    }

    pub fn set_selection_in(&mut self) {
        let c = self.cursor;
        let end = self
            .time_sel
            .map(|s| s.1)
            .filter(|e| *e > c)
            .unwrap_or(c + Ticks::from_secs_i64(5));
        self.time_sel = Some((c, end));
        self.project.loop_region = Some((c, end));
    }

    pub fn set_selection_out(&mut self) {
        let c = self.cursor;
        let start = self
            .time_sel
            .map(|s| s.0)
            .filter(|s| *s < c)
            .unwrap_or(Ticks::ZERO);
        self.time_sel = Some((start, c));
        self.project.loop_region = Some((start, c));
    }

    pub fn select_loop_region(&mut self) {
        if let Some(r) = self.project.loop_region {
            self.time_sel = Some(r);
        }
    }

    pub fn insert_marker(&mut self) {
        self.insert_marker_kind(MarkerKind::Standard, "", "", "");
    }

    pub fn insert_marker_kind(
        &mut self,
        kind: MarkerKind,
        label: &str,
        command_type: &str,
        command_arg: &str,
    ) {
        self.checkpoint();
        let same_kind = self
            .project
            .markers
            .iter()
            .filter(|m| m.kind == kind)
            .count()
            + 1;
        let resolved_label = if label.trim().is_empty() {
            let n2 = format!("{same_kind:02}");
            match kind {
                MarkerKind::Standard => tr_args(self.lang, "Marcador {n}", &[("n", &same_kind)]),
                MarkerKind::CdTrack => tr_args(self.lang, "Faixa CD {n}", &[("n", &n2)]),
                MarkerKind::CdIndex => tr_args(self.lang, "Índice {n}", &[("n", &n2)]),
                MarkerKind::Command => tr_args(self.lang, "Comando {n}", &[("n", &same_kind)]),
            }
        } else {
            label.trim().chars().take(256).collect()
        };
        let mut mk = Marker::new(
            self.cursor.clamp_safe(Ticks::ZERO, Ticks::MAX),
            kind,
            resolved_label,
        );
        if kind == MarkerKind::Command {
            mk.command_type = command_type
                .trim()
                .chars()
                .filter(|&c| c != '\0')
                .take(64)
                .collect();
            if mk.command_type.is_empty() {
                mk.command_type = "TEXT".to_string();
            }
            mk.command_arg = command_arg
                .trim()
                .chars()
                .filter(|&c| c != '\0')
                .take(512)
                .collect();
        }
        self.project.markers.push(mk);
        self.project.markers.sort_by_key(|m| m.t);
        self.changed();
        self.set_status(format!("{} inserido.", kind.label()));
    }

    /// Gera um arquivo Red Book CUE Sheet (75 quadros/s) a partir dos marcadores de CD do projeto.
    pub fn export_cd_cue_sheet(&self, audio_filename: &str) -> String {
        let mut cd_marks: Vec<&Marker> = self
            .project
            .markers
            .iter()
            .filter(|m| matches!(m.kind, MarkerKind::CdTrack | MarkerKind::CdIndex))
            .collect();
        cd_marks.sort_by_key(|m| m.t);
        let safe_file = if audio_filename.trim().is_empty() {
            "audio.wav"
        } else {
            audio_filename.trim()
        };
        let mut out = format!("FILE \"{safe_file}\" WAVE\n");
        let mut track_num = 0usize;
        let mut index_num = 1usize;
        for m in cd_marks {
            let secs_total = m.t.as_secs_f64().max(0.0);
            let total_cd_frames = (secs_total * 75.0).round() as u64;
            let mm = total_cd_frames / (60 * 75);
            let ss = (total_cd_frames / 75) % 60;
            let ff = total_cd_frames % 75;
            match m.kind {
                MarkerKind::CdTrack => {
                    track_num += 1;
                    index_num = 1;
                    out.push_str(&format!("  TRACK {track_num:02} AUDIO\n"));
                    out.push_str(&format!("    TITLE \"{}\"\n", m.label.replace('"', "'")));
                    out.push_str(&format!("    INDEX 01 {mm:02}:{ss:02}:{ff:02}\n"));
                }
                MarkerKind::CdIndex => {
                    if track_num == 0 {
                        track_num = 1;
                        out.push_str("  TRACK 01 AUDIO\n");
                    }
                    index_num = (index_num + 1).min(99);
                    out.push_str(&format!(
                        "    INDEX {index_num:02} {mm:02}:{ss:02}:{ff:02}\n"
                    ));
                }
                MarkerKind::Standard | MarkerKind::Command => {}
            }
        }
        out
    }

    pub fn set_track_automation_mode(&mut self, ti: usize, mode: AutomationMode) -> bool {
        let Some(cur) = self.project.tracks.get(ti).map(|t| t.automation_mode) else {
            return false;
        };
        if cur == mode {
            return true;
        }
        self.checkpoint();
        if let Some(tr) = self.project.tracks.get_mut(ti) {
            tr.automation_mode = mode;
        }
        self.changed();
        self.set_status(mode.label());
        true
    }

    pub fn cycle_track_automation_mode(&mut self, ti: usize) -> Option<AutomationMode> {
        let cur = self.project.tracks.get(ti)?.automation_mode;
        let next = match cur {
            AutomationMode::Off => AutomationMode::Read,
            AutomationMode::Read => AutomationMode::Touch,
            AutomationMode::Touch => AutomationMode::Latch,
            AutomationMode::Latch => AutomationMode::Off,
        };
        self.set_track_automation_mode(ti, next);
        Some(next)
    }

    /// Grava ou atualiza ponto de automação ao vivo (modos Touch/Latch, p. 1008).
    pub fn record_automation_point(
        &mut self,
        ti: usize,
        env_kind: EnvKind,
        t: Ticks,
        v: f32,
        tol: Ticks,
    ) -> bool {
        let Some(tr) = self.project.tracks.get_mut(ti) else {
            return false;
        };
        if !tr.automation_mode.is_write() {
            return false;
        }
        let env = match tr.envelopes.iter_mut().find(|e| e.kind == env_kind) {
            Some(e) => {
                e.visible = true;
                e
            }
            None => {
                let mut e = Envelope::new(env_kind);
                e.visible = true;
                tr.envelopes.push(e);
                let last_idx = tr.envelopes.len() - 1;
                &mut tr.envelopes[last_idx]
            }
        };
        env.set_point(t, v, tol);
        self.changed();
        true
    }

    pub fn cycle_ruler_grid(&mut self) -> RulerGridSpacing {
        let cur = self.opts.ruler_grid;
        let idx = RulerGridSpacing::ALL
            .iter()
            .position(|g| *g == cur)
            .map(|p| (p + 1) % RulerGridSpacing::ALL.len())
            .unwrap_or(0);
        let next = RulerGridSpacing::ALL[idx];
        self.opts.ruler_grid = next;
        self.set_status(format!("Grade da régua: {}", next.label()));
        next
    }

    pub fn insert_region(&mut self) {
        let (a, b) = match self.time_sel.or(self.project.loop_region) {
            Some(r) if r.1 > r.0 => r,
            _ => {
                self.set_status(tr(
                    self.lang,
                    "Crie uma seleção de tempo para inserir uma região",
                ));
                return;
            }
        };
        self.checkpoint();
        let n = self.project.regions.len() + 1;
        self.project.regions.push(Region {
            id: new_id(),
            start: a,
            end: b,
            label: tr_args(self.lang, "Região {n}", &[("n", &n)]),
        });
        self.changed();
    }

    /// Mostra/oculta (ou cria) o envelope da trilha selecionada; o tipo
    /// depende da trilha ser de áudio ou vídeo.
    pub fn toggle_envelope_for_selected(&mut self, audio_kind: EnvKind, video_kind: EnvKind) {
        let Some(ti) = self.sel_track else { return };
        let Some(track) = self.project.tracks.get(ti) else {
            return;
        };
        let kind = if track.kind == TrackKind::Audio {
            audio_kind
        } else {
            video_kind
        };
        if track.envelopes.iter().any(|e| e.kind == kind) {
            if let Some(e) = self
                .project
                .tracks
                .get_mut(ti)
                .and_then(|t| t.envelopes.iter_mut().find(|e| e.kind == kind))
            {
                e.visible = !e.visible;
            }
        } else {
            self.checkpoint();
            if let Some(track) = self.project.tracks.get_mut(ti) {
                track.envelopes.push(Envelope::new(kind));
                self.opts.show_envelopes = true;
            }
        }
        self.changed();
    }

    pub fn remove_envelope(&mut self, kind: EnvKind) {
        let Some(ti) = self.sel_track else { return };
        if ti >= self.project.tracks.len() {
            return;
        }
        self.checkpoint();
        let Some(track) = self.project.tracks.get_mut(ti) else {
            return;
        };
        let k = match (track.kind, kind) {
            (TrackKind::Video, EnvKind::Volume) => EnvKind::Opacity,
            (TrackKind::Video, EnvKind::Pan) => EnvKind::FadeToColor,
            _ => kind,
        };
        track.envelopes.retain(|e| e.kind != k);
        self.changed();
    }

    pub fn toggle_reverse(&mut self) {
        if self.sel_events.is_empty() {
            return;
        }
        self.checkpoint();
        for id in self.sel_events.clone() {
            if let Some(e) = self.project.event_mut(id) {
                e.reverse = !e.reverse;
            }
        }
        self.changed();
    }

    /// Congela o quadro sob o cursor: divide e cria um evento de imagem parada de 2 segundos.
    pub fn freeze_frame(&mut self) {
        let Some(id) = self.sel_events.iter().next().cloned() else {
            return;
        };
        let Some((ti, ei)) = self.project.find_event(id) else {
            return;
        };
        let Some(track) = self.project.tracks.get(ti) else {
            return;
        };
        if track.kind != TrackKind::Video {
            return;
        }
        let Some(e) = track.events.get(ei).cloned() else {
            return;
        };
        let c = self.cursor;
        if !e.contains(c) {
            self.set_status(tr(self.lang, "Posicione o cursor sobre o evento"));
            return;
        }
        let m = self.project.media(e.media_id).cloned();
        let mt = e
            .media_time(c, m.and_then(|m| m.duration))
            .unwrap_or(Ticks::ZERO);
        self.checkpoint();
        let two_s = Ticks::from_secs_i64(2);
        let mut fr = e.clone();
        fr.id = new_id();
        fr.start = c;
        fr.length = two_s;
        fr.offset = mt;
        // um quadro congelado é da mídia ativa; os outros takes não se aplicam
        fr.alt_takes.clear();
        fr.take_index = 0;
        fr.rate = Rate::FROZEN;
        fr.looped = false;
        fr.reverse = false;
        fr.group = None;
        fr.fade_in = Fade::default();
        fr.fade_out = Fade::default();
        fr.name = tr_args(self.lang, "{name} (congelado)", &[("name", &e.name)]);
        if let Some(track) = self.project.tracks.get_mut(ti) {
            for ev in &mut track.events {
                if ev.start >= c {
                    ev.start += two_s;
                }
            }
            if let Some(original) = track.events.get_mut(ei) {
                if original.start < c && original.end() > c {
                    let left = c - original.start;
                    let mut right = original.clone();
                    right.id = new_id();
                    right.start = c + two_s;
                    right.length = original.length - left;
                    right.shift_offsets(right.rate.scale_ticks(left));
                    right.fade_in = Fade::default();
                    original.length = left;
                    original.fade_out = Fade::default();
                    track.events.push(right);
                }
            }
            track.events.push(fr);
        }
        self.changed();
    }

    pub fn delete_track(&mut self, ti: usize) {
        if ti >= self.project.tracks.len() {
            return;
        }
        self.checkpoint();
        self.project.tracks.remove(ti);
        self.sel_track = if self.project.tracks.is_empty() {
            None
        } else {
            Some(ti.min(self.project.tracks.len() - 1))
        };
        self.changed();
    }

    pub fn duplicate_track(&mut self, ti: usize) {
        let Some(orig) = self.project.tracks.get(ti).cloned() else {
            return;
        };
        self.checkpoint();
        let mut t = orig;
        t.id = new_id();
        t.name = tr_args(self.lang, "{name} (cópia)", &[("name", &t.name)]);
        let mut gm: HashMap<Id, Id> = HashMap::new();
        for e in &mut t.events {
            e.id = new_id();
            e.group = e.group.map(|g| *gm.entry(g).or_insert_with(new_id));
            e.fx = e.fx.iter().map(|f| f.duplicate()).collect();
        }
        t.fx = t.fx.iter().map(|f| f.duplicate()).collect();
        self.project.tracks.insert(ti + 1, t);
        self.changed();
    }

    pub fn move_track(&mut self, ti: usize, up: bool) {
        let n = self.project.tracks.len();
        if ti >= n {
            return;
        }
        let to = if up {
            ti.checked_sub(1)
        } else {
            (ti + 1 < n).then_some(ti + 1)
        };
        if let Some(to) = to {
            self.checkpoint();
            self.project.tracks.swap(ti, to);
            self.sel_track = Some(to);
            self.changed();
        }
    }

    pub fn toggle_selected_track_mute(&mut self) -> bool {
        let Some(ti) = self.sel_track else {
            return false;
        };
        if ti >= self.project.tracks.len() {
            return false;
        }
        self.checkpoint();
        if let Some(t) = self.project.tracks.get_mut(ti) {
            t.mute = !t.mute;
        }
        self.changed();
        true
    }

    pub fn toggle_selected_track_solo(&mut self) -> bool {
        let Some(ti) = self.sel_track else {
            return false;
        };
        if ti >= self.project.tracks.len() {
            return false;
        }
        self.checkpoint();
        if let Some(t) = self.project.tracks.get_mut(ti) {
            t.solo = !t.solo;
        }
        self.changed();
        true
    }

    pub fn jump_marker_time(&self, forward: bool) -> Option<Ticks> {
        let mut pts: Vec<Ticks> = self.project.markers.iter().map(|m| m.t).collect();
        pts.extend(self.project.regions.iter().flat_map(|r| [r.start, r.end]));
        pts.push(Ticks::ZERO);
        pts.push(self.project.duration());
        pts.sort();
        let c = self.cursor;
        if forward {
            pts.into_iter().find(|t| *t > c)
        } else {
            pts.into_iter().rev().find(|t| *t < c)
        }
    }

    pub fn jump_to_marker_index(&self, zero_based: usize) -> Option<Ticks> {
        let mut ms: Vec<Ticks> = self.project.markers.iter().map(|m| m.t).collect();
        ms.sort();
        ms.get(zero_based).copied()
    }

    /// Remove mídias do projeto que não são usadas por nenhum evento (incluindo sequências inativas).
    /// Retorna os IDs removidos para o chamador limpar caches visuais.
    pub fn remove_unused_media(&mut self, media: Option<&MediaCache>) -> Vec<Id> {
        let used: HashSet<Id> = self.project.used_media_ids();
        let n0 = self.project.media.len();
        self.checkpoint();
        let removed: Vec<Id> = self
            .project
            .media
            .iter()
            .filter(|m| !used.contains(&m.id))
            .map(|m| m.id)
            .collect();
        self.project.media.retain(|m| used.contains(&m.id));
        if !removed.is_empty() {
            let rem_set: HashSet<Id> = removed.iter().copied().collect();
            for bin in &mut self.project.media_bins {
                bin.media_ids.retain(|id| !rem_set.contains(id));
            }
            for st in &mut self.project.storyboards {
                st.items.retain(|it| !rem_set.contains(&it.media_id));
            }
            if self.active_media_id.is_some_and(|id| rem_set.contains(&id)) {
                self.active_media_id = None;
            }
        }
        if let Some(cache) = media {
            for &id in &removed {
                cache.remove(id);
            }
        }
        self.removed_media_ids.extend_from_slice(&removed);
        let removed_count = n0 - self.project.media.len();
        self.set_status(tr_args(
            self.lang,
            "{n} mídia(s) removida(s)",
            &[("n", &removed_count)],
        ));
        self.changed();
        removed
    }

    /// Configura manualmente as propriedades do projeto, impedindo que a
    /// primeira mídia importada sobrescreva o formato escolhido (§3.1).
    pub fn configure_project_settings(&mut self, mut settings: ProjectSettings) {
        self.checkpoint();
        settings.width = (settings.width / 2).max(8) * 2;
        settings.height = (settings.height / 2).max(8) * 2;
        settings.manually_configured = true;
        self.project.settings = settings;
        self.project.sanitize();
        self.changed();
    }

    /// Registra uma mídia importada aplicando a invariante §3.1:
    /// a primeira mídia importada só define o formato do projeto se o usuário
    /// ainda não tiver configurado as propriedades manualmente.
    pub fn register_imported_media(
        &mut self,
        item: MediaItem,
        entry_sample_rate: Option<u32>,
    ) -> Id {
        let id = item.id;
        let is_first = self.project.media.is_empty();
        if is_first && !self.project.settings.manually_configured {
            if item.has_video && item.width >= 16 && item.height >= 16 {
                self.project.settings.width = (item.width / 2).max(8) * 2;
                self.project.settings.height = (item.height / 2).max(8) * 2;
                if let Some(fps) = item.fps {
                    if (1.0..=240.0).contains(&fps.as_f64()) {
                        self.project.settings.fps = fps;
                        self.project.settings.drop_frame = fps.is_ntsc_drop_eligible();
                    }
                }
            }
            if let Some(sr) = entry_sample_rate {
                if (8000..=192_000).contains(&sr) {
                    self.project.settings.sample_rate = sr;
                }
            }
            self.project.sanitize();
        }
        self.project.media.push(item);
        id
    }

    /// Aplica atomicamente um plano de movimentação ou cópia de eventos (§3.1):
    /// - Em movimentações (`copy == false`), solicitações com IDs duplicados são
    ///   rejeitadas atomicamente antes de mutar a timeline, alocar novos IDs ou
    ///   tocar o histórico (`undo`/`redo`).
    /// - Em cópias (`copy == true`), cada entrada recebe um novo `Id` após a
    ///   validação completa e mantém o destino exato solicitado.
    /// - Criação de novas faixas de destino e edição são confirmadas juntas.
    pub fn apply_move_plan(&mut self, plan: &[MoveEntry], copy: bool) -> Result<Vec<Id>> {
        if plan.is_empty() {
            return Ok(Vec::new());
        }

        // Fase 1: validação integral e pura (nenhuma mutação, novo ID ou undo).
        let mut seen_ids = HashSet::new();
        let mut planned_tracks: HashMap<u64, TrackKind> = HashMap::new();
        let mut resolved_origins = Vec::with_capacity(plan.len());

        for entry in plan {
            if !copy && !seen_ids.insert(entry.id) {
                return Err(NeovegError::InvalidInput(format!(
                    "plano de movimentação inválido: identificador {} duplicado",
                    entry.id
                )));
            }
            if entry.new_start < Ticks::ZERO {
                return Err(NeovegError::InvalidInput(format!(
                    "posição inicial negativa para evento {}",
                    entry.id
                )));
            }
            let Some((ti, ei)) = self.project.find_event(entry.id) else {
                return Err(NeovegError::InvalidInput(format!(
                    "evento {} não encontrado na timeline",
                    entry.id
                )));
            };
            let orig_track = &self.project.tracks[ti];
            let orig_ev = &orig_track.events[ei];
            if !copy && orig_ev.locked {
                return Err(NeovegError::InvalidInput(format!(
                    "evento {} está travado para movimentação",
                    entry.id
                )));
            }
            match &entry.target_track {
                MoveTrackTarget::Existing(dst) => {
                    let Some(dst_track) = self.project.tracks.get(*dst) else {
                        return Err(NeovegError::InvalidInput(format!(
                            "faixa de destino {dst} inexistente"
                        )));
                    };
                    if dst_track.kind != orig_track.kind {
                        return Err(NeovegError::InvalidInput(format!(
                            "tipo incompatível ao colocar evento {} na faixa {dst}",
                            entry.id
                        )));
                    }
                }
                MoveTrackTarget::CreateNew { key, kind, name } => {
                    if *kind != orig_track.kind {
                        return Err(NeovegError::InvalidInput(format!(
                            "tipo incompatível para nova faixa '{name}' do evento {}",
                            entry.id
                        )));
                    }
                    if name.trim().is_empty() {
                        return Err(NeovegError::InvalidInput(
                            "nome da nova faixa de destino vazio".into(),
                        ));
                    }
                    if let Some(prev_kind) = planned_tracks.insert(*key, *kind) {
                        if prev_kind != *kind {
                            return Err(NeovegError::InvalidInput(format!(
                                "chave de criação de faixa {key} usada com tipos conflitantes"
                            )));
                        }
                    }
                }
            }
            resolved_origins.push((ti, ei));
        }

        // Fase 2: snapshot dos eventos originais antes de mutar ou alocar IDs.
        let source_clones: Vec<Event> = resolved_origins
            .iter()
            .map(|&(ti, ei)| self.project.tracks[ti].events[ei].clone())
            .collect();

        // Fase 3: efetivação atômica em 1 passo de histórico.
        self.checkpoint();

        let mut created_map: HashMap<u64, usize> = HashMap::new();
        let mut target_indices = Vec::with_capacity(plan.len());
        for entry in plan {
            let dst = match &entry.target_track {
                MoveTrackTarget::Existing(ti) => *ti,
                MoveTrackTarget::CreateNew { key, kind, name } => {
                    if let Some(&idx) = created_map.get(key) {
                        idx
                    } else {
                        let idx = self.project.tracks.len();
                        self.project.tracks.push(Track::new(*kind, name));
                        created_map.insert(*key, idx);
                        idx
                    }
                }
            };
            target_indices.push(dst);
        }

        let mut result_ids = Vec::with_capacity(plan.len());
        if !copy {
            for (idx, entry) in plan.iter().enumerate() {
                let dst = target_indices[idx];
                if let Some((cur_t, cur_e)) = self.project.find_event(entry.id) {
                    if cur_t != dst {
                        let mut ev = self.project.tracks[cur_t].events.remove(cur_e);
                        ev.start = entry.new_start;
                        self.project.tracks[dst].events.push(ev);
                    } else {
                        self.project.tracks[cur_t].events[cur_e].start = entry.new_start;
                    }
                    result_ids.push(entry.id);
                }
            }
            self.sel_events = result_ids.iter().copied().collect();
        } else {
            let mut occurrence_count: HashMap<Id, usize> = HashMap::new();
            let mut group_remap: HashMap<(Id, usize), Id> = HashMap::new();
            for (idx, entry) in plan.iter().enumerate() {
                let dst = target_indices[idx];
                let occ = occurrence_count.entry(entry.id).or_insert(0);
                let occ_idx = *occ;
                *occ += 1;

                let mut ev = source_clones[idx].clone();
                ev.id = new_id();
                ev.start = entry.new_start;
                ev.group = ev
                    .group
                    .map(|g| *group_remap.entry((g, occ_idx)).or_insert_with(new_id));
                ev.fx = ev.fx.iter().map(|f| f.duplicate()).collect();
                result_ids.push(ev.id);
                self.project.tracks[dst].events.push(ev);
            }
            self.sel_events = result_ids.iter().copied().collect();
        }

        self.project.sort_events();
        self.changed();
        Ok(result_ids)
    }
}

/// Instantâneo de um evento numa trilha para edição live (Shuffle / Slide / Roll).
#[derive(Clone, Debug)]
pub struct TrackEventSnapshot {
    pub id: Id,
    pub track_idx: usize,
    pub start: Ticks,
    pub length: Ticks,
    pub offset: Ticks,
    pub rate: Rate,
    pub reverse: bool,
    pub looped: bool,
    pub media_dur: Option<Ticks>,
}

/// Instantâneo de um evento para esticamento de tempo (Time Stretch / Compress).
#[derive(Clone, Debug)]
pub struct StretchEventSnapshot {
    pub id: Id,
    pub start: Ticks,
    pub length: Ticks,
    pub offset: Ticks,
    pub rate: Rate,
    pub fade_in: Ticks,
    pub fade_out: Ticks,
    pub pan_crop_times: Vec<Ticks>,
    pub fx_key_times: Vec<Vec<Vec<Ticks>>>,
}

impl StretchEventSnapshot {
    pub fn capture(ev: &Event) -> Self {
        Self {
            id: ev.id,
            start: ev.start,
            length: ev.length,
            offset: ev.offset,
            rate: ev.rate,
            fade_in: ev.fade_in.len,
            fade_out: ev.fade_out.len,
            pan_crop_times: ev.pan_crop.keys.iter().map(|k| k.t).collect(),
            fx_key_times: ev
                .fx
                .iter()
                .map(|f| {
                    f.params
                        .iter()
                        .map(|p| p.keys.iter().map(|k| k.0).collect())
                        .collect()
                })
                .collect(),
        }
    }
}

/// Reordena os eventos de uma trilha com base no instantâneo `snap` movendo `anchor_id`
/// para a posição relativa de `target_time` sem criar lacunas nem sobreposições (modo Shuffle).
/// Eventos agrupados em outras trilhas acompanham o deslocamento do evento âncora.
pub fn apply_shuffle_from_snapshot(
    project: &mut Project,
    track_idx: usize,
    snap: &[TrackEventSnapshot],
    anchor_id: Id,
    target_time: Ticks,
    partners: &[(Id, Ticks)],
) -> bool {
    let Some(track) = project.tracks.get_mut(track_idx) else {
        return false;
    };
    if snap.len() < 2 {
        return false;
    }
    let Some(anchor_snap) = snap.iter().find(|s| s.id == anchor_id) else {
        return false;
    };
    let mut others: Vec<&TrackEventSnapshot> = snap.iter().filter(|s| s.id != anchor_id).collect();
    others.sort_by_key(|s| s.start);
    let insert_pos = others
        .iter()
        .position(|s| {
            let mid = s.start + s.length.div_i64(2);
            target_time < mid
        })
        .unwrap_or(others.len());
    let mut ordered: Vec<&TrackEventSnapshot> = Vec::with_capacity(snap.len());
    for (i, item) in others.iter().enumerate() {
        if i == insert_pos {
            ordered.push(anchor_snap);
        }
        ordered.push(item);
    }
    if insert_pos >= others.len() {
        ordered.push(anchor_snap);
    }

    let base_start = snap
        .iter()
        .map(|s| s.start)
        .min()
        .unwrap_or(Ticks::ZERO)
        .max(Ticks::ZERO);
    let mut cur = base_start;
    let mut anchor_new_start = anchor_snap.start;
    for s in ordered {
        if let Some(ev) = track.events.iter_mut().find(|e| e.id == s.id) {
            ev.start = cur;
        }
        if s.id == anchor_id {
            anchor_new_start = cur;
        }
        cur += s.length;
    }
    let anchor_dt = anchor_new_start - anchor_snap.start;
    for &(pid, orig_start) in partners {
        if pid == anchor_id {
            continue;
        }
        if let Some(pe) = project.event_mut(pid) {
            pe.start = (orig_start + anchor_dt).max(Ticks::ZERO);
        }
    }
    true
}

/// Desliza `anchor_id` (modo Slide) ajustando inversamente vizinhos imediatamente adjacentes
/// na mesma trilha, mantendo fixos os limites externos sem alterar conteúdo do evento âncora.
pub fn apply_slide_from_snapshot(
    project: &mut Project,
    track_idx: usize,
    snap: &[TrackEventSnapshot],
    anchor_id: Id,
    raw_dt: Ticks,
    fd: Ticks,
    partners: &[(Id, Ticks)],
) -> bool {
    let min_len = fd.max(Ticks::from_millis_i64(1));
    let tol = Ticks::from_millis_i64(2);
    let mut sorted: Vec<&TrackEventSnapshot> = snap.iter().collect();
    sorted.sort_by_key(|s| s.start);
    let Some(pos) = sorted.iter().position(|s| s.id == anchor_id) else {
        return false;
    };
    let anchor = sorted[pos];
    let left = pos
        .checked_sub(1)
        .and_then(|i| sorted.get(i).copied())
        .filter(|l| ((l.start + l.length) - anchor.start).abs() <= tol);
    let right = sorted
        .get(pos + 1)
        .copied()
        .filter(|r| (r.start - (anchor.start + anchor.length)).abs() <= tol);

    let mut min_dt = -anchor.start;
    let mut max_dt = Ticks::MAX;

    if let Some(l) = left {
        // Esquerda diminui quando dt < 0, aumenta quando dt > 0.
        min_dt = min_dt.max(-(l.length - min_len));
        if let (Some(d), false, false) = (l.media_dur, l.looped, l.reverse) {
            if let Some(avail) = l.rate.unscale_ticks((d - l.offset).max(Ticks::ZERO)) {
                max_dt = max_dt.min((avail - l.length).max(Ticks::ZERO));
            }
        }
    } else if let Some(prev) = pos.checked_sub(1).and_then(|i| sorted.get(i).copied()) {
        // Não atravessa vizinho não adjacente quando não há toque inicial.
        min_dt = min_dt.max((prev.start + prev.length) - anchor.start);
    }

    if let Some(r) = right {
        // Direita aumenta quando dt < 0 (anda à esquerda), diminui quando dt > 0.
        max_dt = max_dt.min(r.length - min_len);
        if !r.looped && !r.reverse {
            if let Some(max_back) = r.rate.unscale_ticks(r.offset) {
                min_dt = min_dt.max(-max_back);
            }
        }
    } else if let Some(next) = sorted.get(pos + 1).copied() {
        max_dt = max_dt.min(next.start - (anchor.start + anchor.length));
    }

    if min_dt > max_dt {
        min_dt = Ticks::ZERO;
        max_dt = Ticks::ZERO;
    }
    let dt = raw_dt.clamp_safe(min_dt, max_dt);

    let Some(track) = project.tracks.get_mut(track_idx) else {
        return false;
    };
    if let Some(ev) = track.events.iter_mut().find(|e| e.id == anchor_id) {
        ev.start = (anchor.start + dt).max(Ticks::ZERO);
    }
    if let Some(l) = left {
        if let Some(le) = track.events.iter_mut().find(|e| e.id == l.id) {
            let orig_end = l.start + l.length;
            le.length = (l.length + dt).max(min_len);
            if l.reverse {
                let d_end = (l.start + le.length) - orig_end;
                let off_delta = l.offset - l.rate.scale_ticks(d_end) - le.offset;
                le.shift_offsets(off_delta);
            }
            le.fade_in.len = le.fade_in.len.min(le.length);
            le.fade_out.len = le.fade_out.len.min(le.length - le.fade_in.len);
        }
    }
    if let Some(r) = right {
        if let Some(re) = track.events.iter_mut().find(|e| e.id == r.id) {
            re.start = (r.start + dt).max(Ticks::ZERO);
            re.length = (r.length - dt).max(min_len);
            if !r.reverse {
                let target_off = (r.offset + r.rate.scale_ticks(dt)).max(Ticks::ZERO);
                let delta = target_off - re.offset;
                re.shift_offsets(delta);
            }
            re.fade_in.len = re.fade_in.len.min(re.length);
            re.fade_out.len = re.fade_out.len.min(re.length - re.fade_in.len);
        }
    }
    for &(pid, orig_start) in partners {
        if pid == anchor_id {
            continue;
        }
        if let Some(pe) = project.event_mut(pid) {
            pe.start = (orig_start + dt).max(Ticks::ZERO);
        }
    }
    true
}

/// Estica ou comprime eventos pelo arrasto de borda, ajustando `Rate` e escalando keyframes/fades.
pub fn apply_stretch_from_snapshot(
    project: &mut Project,
    snaps: &[StretchEventSnapshot],
    anchor_id: Id,
    left_edge: bool,
    raw_dt: Ticks,
    fd: Ticks,
) -> bool {
    let min_dur = fd.max(Ticks::from_millis_i64(10));
    let Some(_anchor) = snaps.iter().find(|s| s.id == anchor_id) else {
        return false;
    };
    // Limites seguros de taxa (0.25x .. 4.00x de taxa efetiva final).
    let mut dt = raw_dt;
    for s in snaps {
        let orig_media = s.rate.scale_ticks(s.length).max(min_dur);
        let min_len = (Ticks::from_raw((orig_media.0 / 4).max(min_dur.0)))
            .max(min_dur)
            .min(s.length);
        let max_len = Ticks::from_raw(orig_media.0.saturating_mul(4)).max(s.length);
        if left_edge {
            let orig_end = s.start + s.length;
            let lo_len = min_len;
            let hi_len = max_len.min(orig_end);
            let min_dt = s.length - hi_len.max(lo_len);
            let max_dt = s.length - lo_len;
            dt = dt.clamp_safe(min_dt, max_dt);
        } else {
            let min_dt = min_len - s.length;
            let max_dt = max_len - s.length;
            dt = dt.clamp_safe(min_dt, max_dt);
        }
    }

    for s in snaps {
        let Some(e) = project.event_mut(s.id) else {
            continue;
        };
        if s.length <= Ticks::ZERO {
            continue;
        }
        let new_len = if left_edge {
            (s.length - dt).max(min_dur)
        } else {
            (s.length + dt).max(min_dur)
        };
        let orig_media = s.rate.scale_ticks(s.length);
        if orig_media > Ticks::ZERO {
            if let Some(new_rate) = Rate::from_ratio(orig_media.0 as u64, new_len.0 as u64) {
                e.rate = new_rate;
            }
        }
        if left_edge {
            e.start = (s.start + s.length - new_len).max(Ticks::ZERO);
        }
        e.length = new_len;
        let scale_tick = |t: Ticks| -> Ticks {
            let num = t.0 as i128 * new_len.0 as i128;
            let den = (s.length.0 as i128).max(1);
            Ticks::from_raw((num / den) as i64)
        };
        e.fade_in.len = scale_tick(s.fade_in).clamp_safe(Ticks::ZERO, new_len);
        e.fade_out.len = scale_tick(s.fade_out).clamp_safe(Ticks::ZERO, new_len - e.fade_in.len);
        for (k, orig_t) in e.pan_crop.keys.iter_mut().zip(s.pan_crop_times.iter()) {
            k.t = scale_tick(*orig_t).clamp_safe(Ticks::ZERO, new_len);
        }
        for (fx, orig_fx) in e.fx.iter_mut().zip(s.fx_key_times.iter()) {
            for (p, orig_p) in fx.params.iter_mut().zip(orig_fx.iter()) {
                for (k, orig_t) in p.keys.iter_mut().zip(orig_p.iter()) {
                    k.0 = scale_tick(*orig_t).clamp_safe(Ticks::ZERO, new_len);
                }
            }
        }
    }
    true
}

/// Ajuste conjunto de corte (Roll): ao arrastar a borda de corte entre dois eventos
/// contíguos na mesma trilha, ambos têm o ponto de edição movido juntos sem alterar
/// o comprimento total nem deixar lacunas.
pub fn apply_roll_from_snapshot(
    project: &mut Project,
    track_idx: usize,
    snap: &[TrackEventSnapshot],
    anchor_id: Id,
    left_edge: bool,
    raw_dt: Ticks,
    fd: Ticks,
) -> bool {
    let min_len = fd.max(Ticks::from_millis_i64(1));
    let tol = Ticks::from_millis_i64(2);
    let mut sorted: Vec<&TrackEventSnapshot> = snap.iter().collect();
    sorted.sort_by_key(|s| s.start);
    let Some(pos) = sorted.iter().position(|s| s.id == anchor_id) else {
        return false;
    };
    let anchor = sorted[pos];
    let cut_pos = if left_edge {
        anchor.start
    } else {
        anchor.start + anchor.length
    };
    let (left_ev, right_ev) = if left_edge {
        let Some(prev) = pos
            .checked_sub(1)
            .and_then(|i| sorted.get(i).copied())
            .filter(|l| ((l.start + l.length) - cut_pos).abs() <= tol)
        else {
            return false;
        };
        (prev, anchor)
    } else {
        let Some(next) = sorted
            .get(pos + 1)
            .copied()
            .filter(|r| (r.start - cut_pos).abs() <= tol)
        else {
            return false;
        };
        (anchor, next)
    };

    let mut min_dt = -(left_ev.length - min_len);
    let mut max_dt = right_ev.length - min_len;
    if let (Some(d), false, false) = (left_ev.media_dur, left_ev.looped, left_ev.reverse) {
        if let Some(avail) = left_ev
            .rate
            .unscale_ticks((d - left_ev.offset).max(Ticks::ZERO))
        {
            max_dt = max_dt.min((avail - left_ev.length).max(Ticks::ZERO));
        }
    }
    if !right_ev.looped && !right_ev.reverse {
        if let Some(max_back) = right_ev.rate.unscale_ticks(right_ev.offset) {
            min_dt = min_dt.max(-max_back);
        }
    }
    if min_dt > max_dt {
        min_dt = Ticks::ZERO;
        max_dt = Ticks::ZERO;
    }
    let dt = raw_dt.clamp_safe(min_dt, max_dt);

    let Some(track) = project.tracks.get_mut(track_idx) else {
        return false;
    };
    if let Some(le) = track.events.iter_mut().find(|e| e.id == left_ev.id) {
        let orig_end = left_ev.start + left_ev.length;
        le.length = (left_ev.length + dt).max(min_len);
        if left_ev.reverse {
            let d_end = (left_ev.start + le.length) - orig_end;
            let off_delta = left_ev.offset - left_ev.rate.scale_ticks(d_end) - le.offset;
            le.shift_offsets(off_delta);
        }
        le.fade_in.len = le.fade_in.len.min(le.length);
        le.fade_out.len = le.fade_out.len.min(le.length - le.fade_in.len);
    }
    if let Some(re) = track.events.iter_mut().find(|e| e.id == right_ev.id) {
        re.start = (right_ev.start + dt).max(Ticks::ZERO);
        re.length = (right_ev.length - dt).max(min_len);
        if !right_ev.reverse {
            let target_off = (right_ev.offset + right_ev.rate.scale_ticks(dt)).max(Ticks::ZERO);
            let delta = target_off - re.offset;
            re.shift_offsets(delta);
        }
        re.fade_in.len = re.fade_in.len.min(re.length);
        re.fade_out.len = re.fade_out.len.min(re.length - re.fade_in.len);
    }
    true
}

/// Verifica se dois eventos sequenciais na mesma trilha são contínuos para juntar (Heal).
pub fn can_heal_pair(a: &Event, b: &Event) -> bool {
    if a.locked
        || b.locked
        || a.media_id != b.media_id
        || a.reverse != b.reverse
        || a.rate != b.rate
        || a.take_index != b.take_index
        || a.alt_takes != b.alt_takes
    {
        return false;
    }
    let tol = Ticks::from_millis_i64(2);
    if (b.start - a.end()).abs() > tol {
        return false;
    }
    if !a.reverse {
        let expected_b = a.offset + a.rate.scale_ticks(b.start - a.start);
        (b.offset - expected_b).abs() <= tol
    } else {
        let expected_a = b.offset + b.rate.scale_ticks(b.end() - a.end());
        (a.offset - expected_a).abs() <= tol
    }
}

impl EditorSession {
    /// Verifica se há ao menos um par contíguo juntável na seleção atual (Heal).
    pub fn can_heal_selection(&self) -> bool {
        if self.sel_events.len() < 2 {
            return false;
        }
        let ids = self.expand_groups(&self.sel_events);
        for tr in &self.project.tracks {
            if tr.locked {
                continue;
            }
            let mut evs: Vec<&Event> = tr.events.iter().filter(|e| ids.contains(&e.id)).collect();
            if evs.len() < 2 {
                continue;
            }
            evs.sort_by_key(|e| e.start);
            for pair in evs.windows(2) {
                if can_heal_pair(pair[0], pair[1]) {
                    return true;
                }
            }
        }
        false
    }

    /// Junta eventos contíguos da mesma mídia cortados na timeline (Heal).
    pub fn heal_events(&mut self) -> usize {
        if self.sel_events.len() < 2 {
            return 0;
        }
        if !self.can_heal_selection() {
            self.set_status("Nada para juntar: selecione dois eventos contínuos da mesma mídia.");
            return 0;
        }
        self.checkpoint();
        let ids = self.expand_groups(&self.sel_events.clone());
        let mut healed_count = 0usize;
        let mut new_sel = self.sel_events.clone();
        for tr in &mut self.project.tracks {
            if tr.locked {
                continue;
            }
            tr.events.sort_by_key(|e| e.start);
            let mut i = 0usize;
            while i + 1 < tr.events.len() {
                if !ids.contains(&tr.events[i].id) {
                    i += 1;
                    continue;
                }
                let mut next_idx = i + 1;
                while next_idx < tr.events.len() && !ids.contains(&tr.events[next_idx].id) {
                    next_idx += 1;
                }
                if next_idx != i + 1 {
                    i += 1;
                    continue;
                }
                if can_heal_pair(&tr.events[i], &tr.events[next_idx]) {
                    let right = tr.events.remove(next_idx);
                    new_sel.remove(&right.id);
                    let left = &mut tr.events[i];
                    let shift = right.start - left.start;
                    let total_end = left.end().max(right.end());
                    left.length = total_end - left.start;
                    if left.reverse {
                        left.offset = right.offset;
                    }
                    left.fade_out = right.fade_out;
                    left.fade_in.len = left.fade_in.len.min(left.length);
                    left.fade_out.len = left.fade_out.len.min(left.length - left.fade_in.len);
                    for mut k in right.pan_crop.keys {
                        k.t += shift;
                        if !left.pan_crop.keys.iter().any(|ek| ek.t == k.t) {
                            left.pan_crop.keys.push(k);
                        }
                    }
                    left.pan_crop.keys.sort_by_key(|k| k.t);
                    new_sel.insert(left.id);
                    healed_count += 1;
                } else {
                    i += 1;
                }
            }
        }
        self.sel_events = new_sel;
        if healed_count > 0 {
            self.changed();
            self.set_status(format!("{healed_count} corte(s) reunido(s) (Heal)."));
        }
        healed_count
    }

    /// Smart Split (`Alt+S`): remove o trecho `time_sel`, aplica crossfade curto com Warp Flow
    /// (~5 quadros), agrupa as duas partes e desloca o restante da trilha.
    pub fn smart_split(&mut self) -> usize {
        let Some((a, b)) = self.time_sel else {
            return 0;
        };
        if b <= a {
            return 0;
        }
        let fd = self
            .project
            .settings
            .frame_dur()
            .max(Ticks::from_millis_i64(1));
        let explicit_targets: Option<HashSet<Id>> = if self.sel_events.is_empty() {
            None
        } else {
            Some(self.expand_groups(&self.sel_events))
        };
        let has_hit = self.project.tracks.iter().any(|tr| {
            !tr.locked
                && tr.events.iter().any(|e| {
                    !e.locked
                        && e.end() > a
                        && e.start < b
                        && explicit_targets
                            .as_ref()
                            .is_none_or(|ids| ids.contains(&e.id))
                })
        });
        if !has_hit {
            return 0;
        }
        self.checkpoint();
        let span = b - a;
        let overlap_cap = Ticks::from_raw(fd.0.saturating_mul(5)).min(span.div_i64(2));
        let mut group_map: HashMap<Option<Id>, Id> = HashMap::new();
        let mut count = 0usize;
        let mut new_sel = HashSet::new();

        for tr in &mut self.project.tracks {
            if tr.locked {
                continue;
            }
            let mut next_events: Vec<Event> = Vec::with_capacity(tr.events.len() + 2);
            let mut track_shifted = false;
            for mut ev in std::mem::take(&mut tr.events) {
                let targeted = !ev.locked
                    && explicit_targets
                        .as_ref()
                        .is_none_or(|ids| ids.contains(&ev.id));
                if ev.end() <= a {
                    next_events.push(ev);
                    continue;
                }
                if ev.start >= b {
                    if track_shifted || explicit_targets.is_none() {
                        ev.start = (ev.start - span).max(Ticks::ZERO);
                    }
                    next_events.push(ev);
                    continue;
                }
                if !targeted {
                    if ev.start >= b && track_shifted {
                        ev.start = (ev.start - span).max(Ticks::ZERO);
                    }
                    next_events.push(ev);
                    continue;
                }
                track_shifted = true;
                if ev.start < a && ev.end() > b {
                    let orig_start = ev.start;
                    let orig_len = ev.length;
                    let orig_end = ev.end();
                    let orig_off = ev.offset;
                    let left_base = a - orig_start;
                    let right_len = orig_end - b;
                    let ov = overlap_cap
                        .min(left_base.div_i64(2))
                        .min(right_len.div_i64(2));

                    let mut right = ev.clone();
                    let mut left = ev;
                    left.length = left_base + ov;
                    left.fade_out = Fade {
                        len: ov,
                        curve: Curve::Smooth,
                    };
                    if left.reverse {
                        left.offset = orig_off + left.rate.scale_ticks(orig_end - (a + ov));
                    }

                    right.id = new_id();
                    right.start = a;
                    right.length = right_len;
                    right.fade_in = Fade {
                        len: ov,
                        curve: Curve::Smooth,
                    };
                    right.transition = if ov > Ticks::ZERO {
                        Some(TransitionSpec {
                            kind: TransitionKind::WarpFlow,
                            softness: 0.05,
                        })
                    } else {
                        None
                    };
                    if !right.reverse {
                        right.shift_offsets(right.rate.scale_ticks(b - orig_start));
                    } else {
                        right.offset = orig_off;
                    }
                    for k in &mut right.pan_crop.keys {
                        k.t -= b - orig_start;
                    }
                    for f in &mut right.fx {
                        for p in &mut f.params {
                            for k in &mut p.keys {
                                k.0 -= b - orig_start;
                            }
                        }
                    }
                    let _ = orig_len;
                    let gid = *group_map.entry(left.group).or_insert_with(new_id);
                    left.group = Some(gid);
                    right.group = Some(gid);
                    new_sel.insert(left.id);
                    new_sel.insert(right.id);
                    next_events.push(left);
                    next_events.push(right);
                    count += 1;
                } else if ev.start < a && ev.end() > a {
                    let cut_d = ev.end() - a;
                    if ev.reverse {
                        ev.shift_offsets(ev.rate.scale_ticks(cut_d));
                    }
                    ev.length = (a - ev.start).max(fd);
                    ev.fade_out.len = ev.fade_out.len.min(ev.length);
                    next_events.push(ev);
                    count += 1;
                } else if ev.start >= a && ev.end() > b {
                    let d = b - ev.start;
                    if !ev.reverse {
                        ev.shift_offsets(ev.rate.scale_ticks(d));
                    }
                    ev.length = (ev.end() - b).max(fd);
                    ev.start = a;
                    ev.fade_in.len = ev.fade_in.len.min(ev.length);
                    next_events.push(ev);
                    count += 1;
                } else {
                    // Evento totalmente dentro da seleção [a, b]: removido.
                    count += 1;
                }
            }
            tr.events = next_events;
        }
        self.project.sort_events();
        self.sel_events = new_sel;
        self.time_sel = None;
        self.project.loop_region = None;
        self.cursor = a;
        self.changed();
        self.set_status(format!(
            "Smart Split aplicado ({count} corte(s) com Warp Flow)."
        ));
        count
    }

    /// Cola atributos dos eventos na área de transferência para os eventos selecionados.
    pub fn paste_attributes(&mut self, mask: PasteAttributesMask) -> usize {
        if self.clipboard.is_empty() || self.sel_events.is_empty() || !mask.any() {
            return 0;
        }
        let ids: Vec<Id> = self.sel_events.iter().copied().collect();
        let has_target = ids.iter().any(|&id| {
            self.project.find_event(id).is_some_and(|(ti, ei)| {
                !self.project.tracks[ti].locked && !self.project.tracks[ti].events[ei].locked
            })
        });
        if !has_target {
            return 0;
        }
        self.checkpoint();
        let mut applied = 0usize;
        for id in ids {
            let Some((ti, _)) = self.project.find_event(id) else {
                continue;
            };
            if self.project.tracks[ti].locked {
                continue;
            }
            let track_kind = self.project.tracks[ti].kind;
            let src = self
                .clipboard
                .iter()
                .find(|c| c.kind == track_kind)
                .or_else(|| self.clipboard.first());
            let Some(src) = src else {
                continue;
            };
            let src_ev = &src.ev;
            let Some(dst) = self.project.event_mut(id) else {
                continue;
            };
            if dst.locked {
                continue;
            }
            if mask.fx {
                dst.fx = src_ev.fx.iter().map(|f| f.duplicate()).collect();
            }
            if mask.pan_crop && track_kind == TrackKind::Video {
                dst.pan_crop = src_ev.pan_crop.clone();
            }
            if mask.gain_db && track_kind == TrackKind::Audio {
                dst.gain_db = src_ev.gain_db;
            }
            if mask.rate {
                dst.rate = src_ev.rate;
            }
            if mask.pitch && track_kind == TrackKind::Audio {
                dst.pitch_semitones = if src_ev.pitch_semitones.is_finite() {
                    src_ev.pitch_semitones.clamp(-24.0, 24.0)
                } else {
                    0.0
                };
                dst.lock_pitch_to_stretch = src_ev.lock_pitch_to_stretch;
            }
            if mask.fades {
                dst.fade_in = src_ev.fade_in.clone();
                dst.fade_out = src_ev.fade_out.clone();
                dst.fade_in.len = dst.fade_in.len.min(dst.length);
                dst.fade_out.len = dst.fade_out.len.min(dst.length - dst.fade_in.len);
            }
            if mask.switches {
                dst.mute = src_ev.mute;
                dst.looped = src_ev.looped;
                dst.reverse = src_ev.reverse;
                if track_kind == TrackKind::Video {
                    dst.maintain_aspect = src_ev.maintain_aspect;
                }
            }
            if mask.transition && track_kind == TrackKind::Video {
                dst.transition = src_ev.transition.clone();
            }
            applied += 1;
        }
        if applied > 0 {
            self.changed();
            self.set_status(format!("Atributos colados em {applied} evento(s)."));
        }
        applied
    }

    /// Reordena um evento na sua trilha no modo Shuffle para `target_time` em 1 passo de Desfazer.
    pub fn shuffle_event(&mut self, event_id: Id, target_time: Ticks) -> bool {
        let Some((ti, _)) = self.project.find_event(event_id) else {
            return false;
        };
        if self.project.tracks[ti].locked {
            return false;
        }
        let Some(ev) = self.project.tracks[ti]
            .events
            .iter()
            .find(|e| e.id == event_id)
        else {
            return false;
        };
        if ev.locked {
            return false;
        }
        let snap: Vec<TrackEventSnapshot> = self.project.tracks[ti]
            .events
            .iter()
            .filter(|e| !e.locked)
            .map(|e| TrackEventSnapshot {
                id: e.id,
                track_idx: ti,
                start: e.start,
                length: e.length,
                offset: e.offset,
                rate: e.rate,
                reverse: e.reverse,
                looped: e.looped,
                media_dur: self.project.media(e.media_id).and_then(|m| m.duration),
            })
            .collect();
        if snap.len() < 2 {
            return false;
        }
        let mut single = HashSet::new();
        single.insert(event_id);
        let group_ids = self.expand_groups(&single);
        let partners: Vec<(Id, Ticks)> = group_ids
            .into_iter()
            .filter(|&gid| !snap.iter().any(|s| s.id == gid))
            .filter_map(|gid| self.project.event(gid).map(|e| (gid, e.start)))
            .collect();
        self.checkpoint();
        let ok = apply_shuffle_from_snapshot(
            &mut self.project,
            ti,
            &snap,
            event_id,
            target_time,
            &partners,
        );
        if ok {
            self.project.sort_events();
            self.changed();
        }
        ok
    }

    /// Desliza um evento na sua trilha em `delta` no modo Slide em 1 passo de Desfazer.
    pub fn slide_event(&mut self, event_id: Id, delta: Ticks) -> bool {
        let Some((ti, _)) = self.project.find_event(event_id) else {
            return false;
        };
        if self.project.tracks[ti].locked {
            return false;
        }
        let snap: Vec<TrackEventSnapshot> = self.project.tracks[ti]
            .events
            .iter()
            .filter(|e| !e.locked)
            .map(|e| TrackEventSnapshot {
                id: e.id,
                track_idx: ti,
                start: e.start,
                length: e.length,
                offset: e.offset,
                rate: e.rate,
                reverse: e.reverse,
                looped: e.looped,
                media_dur: self.project.media(e.media_id).and_then(|m| m.duration),
            })
            .collect();
        let fd = self.project.settings.frame_dur();
        let mut single = HashSet::new();
        single.insert(event_id);
        let group_ids = self.expand_groups(&single);
        let partners: Vec<(Id, Ticks)> = group_ids
            .into_iter()
            .filter(|&gid| !snap.iter().any(|s| s.id == gid))
            .filter_map(|gid| self.project.event(gid).map(|e| (gid, e.start)))
            .collect();
        self.checkpoint();
        let ok =
            apply_slide_from_snapshot(&mut self.project, ti, &snap, event_id, delta, fd, &partners);
        if ok {
            self.project.sort_events();
            self.changed();
        }
        ok
    }

    /// Estica/comprime um evento (e parceiros agrupados) pela borda escolhida para `new_len`.
    pub fn stretch_event_edge(&mut self, event_id: Id, left_edge: bool, new_len: Ticks) -> bool {
        let Some(anchor) = self.project.event(event_id) else {
            return false;
        };
        if anchor.locked {
            return false;
        }
        let orig_len = anchor.length;
        let orig_edge = if left_edge {
            anchor.start
        } else {
            anchor.end()
        };
        let mut single = HashSet::new();
        single.insert(event_id);
        let group_ids = self.expand_groups(&single);
        let snaps: Vec<StretchEventSnapshot> = group_ids
            .into_iter()
            .filter_map(|gid| {
                let ge = self.project.event(gid)?;
                let ge_edge = if left_edge { ge.start } else { ge.end() };
                if ge.locked || ge_edge.abs_diff(orig_edge) >= Ticks::from_millis_i64(2) {
                    return None;
                }
                Some(StretchEventSnapshot::capture(ge))
            })
            .collect();
        if snaps.is_empty() {
            return false;
        }
        let dt = if left_edge {
            orig_len - new_len
        } else {
            new_len - orig_len
        };
        let fd = self.project.settings.frame_dur();
        self.checkpoint();
        let ok =
            apply_stretch_from_snapshot(&mut self.project, &snaps, event_id, left_edge, dt, fd);
        if ok {
            self.project.sort_events();
            self.changed();
        }
        ok
    }

    /// Divide um evento em `a` e apara o trecho arrastado `[a..b]` (modo Split Trim).
    pub fn split_trim(&mut self, event_id: Id, a: Ticks, b: Ticks) -> bool {
        let Some((ti, _)) = self.project.find_event(event_id) else {
            return false;
        };
        if self.project.tracks[ti].locked {
            return false;
        }
        let fd = self
            .project
            .settings
            .frame_dur()
            .max(Ticks::from_millis_i64(1));
        let lo = a.min(b);
        let hi = a.max(b);
        // Sem arrasto significativo: apenas divide no ponto clicado.
        if hi - lo <= fd {
            let saved_cursor = self.cursor;
            let saved_sel = self.sel_events.clone();
            self.cursor = a;
            self.sel_events.clear();
            self.sel_events.insert(event_id);
            let n = self.split_at_cursor();
            if n == 0 {
                self.cursor = saved_cursor;
                self.sel_events = saved_sel;
                return false;
            }
            return true;
        }
        let mut single = HashSet::new();
        single.insert(event_id);
        let ids = self.expand_groups(&single);
        self.checkpoint();
        let mut touched = 0usize;
        for tr in &mut self.project.tracks {
            if tr.locked {
                continue;
            }
            let mut next_events = Vec::with_capacity(tr.events.len() + 1);
            for mut ev in std::mem::take(&mut tr.events) {
                if !ids.contains(&ev.id) || ev.locked || ev.end() <= lo || ev.start >= hi {
                    next_events.push(ev);
                    continue;
                }
                touched += 1;
                if ev.start < lo && ev.end() > hi {
                    let orig_start = ev.start;
                    let orig_end = ev.end();
                    let orig_off = ev.offset;
                    let mut right = ev.clone();
                    ev.length = (lo - orig_start).max(fd);
                    ev.fade_out = Fade::default();
                    if ev.reverse {
                        ev.offset = orig_off + ev.rate.scale_ticks(orig_end - lo);
                    }
                    right.id = new_id();
                    right.start = hi;
                    right.length = (orig_end - hi).max(fd);
                    right.fade_in = Fade::default();
                    right.transition = None;
                    if !right.reverse {
                        right.shift_offsets(right.rate.scale_ticks(hi - orig_start));
                    }
                    next_events.push(ev);
                    next_events.push(right);
                } else if ev.start < lo {
                    let cut = ev.end() - lo;
                    if ev.reverse {
                        ev.shift_offsets(ev.rate.scale_ticks(cut));
                    }
                    ev.length = (lo - ev.start).max(fd);
                    ev.fade_out.len = ev.fade_out.len.min(ev.length);
                    next_events.push(ev);
                } else if ev.end() > hi {
                    let orig_end = ev.end();
                    let cut = hi - ev.start;
                    if !ev.reverse {
                        ev.shift_offsets(ev.rate.scale_ticks(cut));
                    }
                    ev.start = hi;
                    ev.length = (orig_end - hi).max(fd);
                    ev.fade_in.len = ev.fade_in.len.min(ev.length);
                    next_events.push(ev);
                }
            }
            tr.events = next_events;
        }
        if touched > 0 {
            self.project.sort_events();
            self.changed();
            true
        } else {
            false
        }
    }

    /// Ajusta a transposição de tom (`pitch_semitones`, em `-24.0..=24.0`) nos
    /// eventos de áudio selecionados (manual do VEGAS Pro 21: teclas `=` / `-`).
    pub fn shift_pitch_selected(&mut self, semitones: f32) -> usize {
        if !semitones.is_finite() || semitones == 0.0 || self.sel_events.is_empty() {
            return 0;
        }
        let ids: Vec<Id> = self.sel_events.iter().copied().collect();
        let has_target = ids.iter().any(|&id| {
            self.project.find_event(id).is_some_and(|(ti, ei)| {
                let tr = &self.project.tracks[ti];
                tr.kind == TrackKind::Audio && !tr.locked && !tr.events[ei].locked
            })
        });
        if !has_target {
            return 0;
        }
        self.checkpoint();
        let mut count = 0usize;
        for id in ids {
            let Some((ti, _)) = self.project.find_event(id) else {
                continue;
            };
            let tr = &self.project.tracks[ti];
            if tr.kind != TrackKind::Audio || tr.locked {
                continue;
            }
            if let Some(ev) = self.project.event_mut(id) {
                if ev.locked {
                    continue;
                }
                let next = (ev.pitch_semitones + semitones).clamp(-24.0, 24.0);
                let snapped = (next * 100.0).round() / 100.0;
                if (ev.pitch_semitones - snapped).abs() > 1e-4 {
                    ev.pitch_semitones = snapped;
                    count += 1;
                }
            }
        }
        if count > 0 {
            self.changed();
            self.set_status(format!(
                "Tom ajustado ({:+.1} st) em {count} evento(s).",
                semitones
            ));
        } else {
            self.undo.pop();
        }
        count
    }

    /// Redefine `pitch_semitones` para `0.0` nos eventos de áudio selecionados.
    pub fn reset_pitch_selected(&mut self) -> usize {
        let ids: Vec<Id> = self.sel_events.iter().copied().collect();
        let has_target = ids.iter().any(|&id| {
            self.project.find_event(id).is_some_and(|(ti, ei)| {
                let tr = &self.project.tracks[ti];
                tr.kind == TrackKind::Audio
                    && !tr.locked
                    && !tr.events[ei].locked
                    && tr.events[ei].pitch_semitones.abs() > 1e-4
            })
        });
        if !has_target {
            return 0;
        }
        self.checkpoint();
        let mut count = 0usize;
        for id in ids {
            let Some((ti, _)) = self.project.find_event(id) else {
                continue;
            };
            let tr = &self.project.tracks[ti];
            if tr.kind != TrackKind::Audio || tr.locked {
                continue;
            }
            if let Some(ev) = self.project.event_mut(id) {
                if !ev.locked && ev.pitch_semitones.abs() > 1e-4 {
                    ev.pitch_semitones = 0.0;
                    count += 1;
                }
            }
        }
        if count > 0 {
            self.changed();
            self.set_status(format!("Tom redefinido em {count} evento(s)."));
        } else {
            self.undo.pop();
        }
        count
    }

    /// Alterna "Preservar tom ao esticar" (`lock_pitch_to_stretch`) nos eventos
    /// de áudio selecionados e retorna o novo estado aplicado.
    pub fn toggle_lock_pitch_to_stretch(&mut self) -> Option<bool> {
        let ids: Vec<Id> = self.sel_events.iter().copied().collect();
        let mut targets = Vec::new();
        let mut any_unlocked_unpres = false;
        for id in ids {
            let Some((ti, ei)) = self.project.find_event(id) else {
                continue;
            };
            let tr = &self.project.tracks[ti];
            let ev = &tr.events[ei];
            if tr.kind == TrackKind::Audio && !tr.locked && !ev.locked {
                if !ev.lock_pitch_to_stretch {
                    any_unlocked_unpres = true;
                }
                targets.push(id);
            }
        }
        if targets.is_empty() {
            return None;
        }
        let target_state = any_unlocked_unpres;
        self.checkpoint();
        for id in targets {
            if let Some(ev) = self.project.event_mut(id) {
                ev.lock_pitch_to_stretch = target_state;
            }
        }
        self.changed();
        self.set_status(if target_state {
            "Preservar tom ao esticar ativado na seleção."
        } else {
            "Preservar tom ao esticar desativado na seleção."
        });
        Some(target_state)
    }

    /// Informa se algum evento do projeto segue `master_id` via Sync Link.
    pub fn has_sync_followers(&self, master_id: Id) -> bool {
        self.project
            .tracks
            .iter()
            .flat_map(|t| t.events.iter())
            .any(|e| e.sync_link_master == Some(master_id))
    }

    /// Lista todos os seguidores transitivos de `masters` (id e início atual),
    /// excluindo os próprios elementos de `masters` e eventos/trilhas travadas.
    pub fn sync_followers_for(&self, masters: &HashSet<Id>) -> Vec<(Id, Ticks)> {
        let mut active = masters.clone();
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for _ in 0..32 {
            let mut next_layer = HashSet::new();
            for tr in &self.project.tracks {
                if tr.locked {
                    continue;
                }
                for ev in &tr.events {
                    if ev.locked || masters.contains(&ev.id) {
                        continue;
                    }
                    if ev.sync_link_master.is_some_and(|m| active.contains(&m))
                        && seen.insert(ev.id)
                    {
                        out.push((ev.id, ev.start));
                        next_layer.insert(ev.id);
                    }
                }
            }
            if next_layer.is_empty() {
                break;
            }
            active = next_layer;
        }
        out
    }

    /// Cria vínculos unidirecionais de sincronização (Sync Links): os demais eventos
    /// destravados da seleção passam a acompanhar o deslocamento temporal do mestre.
    pub fn create_sync_links(&mut self, explicit_master: Option<Id>) -> usize {
        let mut candidates: Vec<(Ticks, usize, Id)> = Vec::new();
        for &id in &self.sel_events {
            if let Some((ti, ei)) = self.project.find_event(id) {
                let tr = &self.project.tracks[ti];
                let ev = &tr.events[ei];
                if !tr.locked && !ev.locked {
                    candidates.push((ev.start, ti, ev.id));
                }
            }
        }
        if candidates.len() < 2 {
            return 0;
        }
        candidates.sort_by_key(|(start, ti, id)| (*start, *ti, *id));
        let master_id = explicit_master
            .filter(|id| candidates.iter().any(|(_, _, cid)| cid == id))
            .unwrap_or(candidates[0].2);
        let followers: Vec<Id> = candidates
            .into_iter()
            .filter_map(|(_, _, id)| (id != master_id).then_some(id))
            .collect();
        if followers.is_empty() {
            return 0;
        }
        self.checkpoint();
        let mut linked = 0usize;
        for fid in followers {
            if would_create_sync_cycle(&self.project, fid, master_id) {
                continue;
            }
            if let Some(ev) = self.project.event_mut(fid) {
                if ev.sync_link_master != Some(master_id) {
                    ev.sync_link_master = Some(master_id);
                    linked += 1;
                }
            }
        }
        if linked > 0 {
            self.changed();
            self.set_status(format!(
                "Sync Link criado: {linked} evento(s) seguem o evento mestre."
            ));
        } else {
            self.undo.pop();
        }
        linked
    }

    /// Desfaz Sync Links envolvendo a seleção: remove vínculos dos seguidores
    /// selecionados ou, se nenhum for seguidor, libera os seguidores dos mestres
    /// selecionados.
    pub fn unlink_sync_links(&mut self) -> usize {
        if self.sel_events.is_empty() {
            return 0;
        }
        let sel_ids: Vec<Id> = self.sel_events.iter().copied().collect();
        let selected_followers: Vec<Id> = sel_ids
            .iter()
            .copied()
            .filter(|&id| {
                self.project.find_event(id).is_some_and(|(ti, ei)| {
                    !self.project.tracks[ti].locked
                        && !self.project.tracks[ti].events[ei].locked
                        && self.project.tracks[ti].events[ei]
                            .sync_link_master
                            .is_some()
                })
            })
            .collect();
        let targets: Vec<Id> = if !selected_followers.is_empty() {
            selected_followers
        } else {
            let masters: HashSet<Id> = sel_ids.iter().copied().collect();
            self.project
                .tracks
                .iter()
                .filter(|t| !t.locked)
                .flat_map(|t| t.events.iter())
                .filter(|e| {
                    !e.locked && e.sync_link_master.is_some_and(|mid| masters.contains(&mid))
                })
                .map(|e| e.id)
                .collect()
        };
        if targets.is_empty() {
            return 0;
        }
        self.checkpoint();
        let mut count = 0usize;
        for id in targets {
            if let Some(ev) = self.project.event_mut(id) {
                if ev.sync_link_master.take().is_some() {
                    count += 1;
                }
            }
        }
        if count > 0 {
            self.changed();
            self.set_status(format!("Sync Link removido de {count} evento(s)."));
        } else {
            self.undo.pop();
        }
        count
    }

    /// Seleciona toda a rede conectada via Sync Link a partir da seleção atual.
    pub fn select_sync_linked(&mut self) -> usize {
        let mut acc: HashSet<Id> = self.sel_events.clone();
        for _ in 0..32 {
            let mut changed = false;
            let mut next_add = Vec::new();
            for tr in &self.project.tracks {
                for ev in &tr.events {
                    if let Some(mid) = ev.sync_link_master {
                        if acc.contains(&ev.id) && !acc.contains(&mid) {
                            next_add.push(mid);
                        } else if acc.contains(&mid) && !acc.contains(&ev.id) {
                            next_add.push(ev.id);
                        }
                    }
                }
            }
            for id in next_add {
                changed |= acc.insert(id);
            }
            if !changed {
                break;
            }
        }
        let added = acc.len().saturating_sub(self.sel_events.len());
        self.sel_events = acc;
        if added > 0 {
            self.set_status(format!("{added} evento(s) vinculado(s) selecionado(s)."));
        }
        self.sel_events.len()
    }

    /// Insere um evento vazio no cursor (ou na seleção de tempo) na trilha selecionada
    /// (manual do VEGAS Pro 21: `Insert > Empty Event`).
    pub fn insert_empty_event(&mut self) -> Id {
        self.checkpoint();
        let target_track = self
            .sel_track
            .filter(|&i| {
                self.project
                    .tracks
                    .get(i)
                    .is_some_and(|t| !t.locked && !t.is_adjustment())
            })
            .or_else(|| {
                self.project
                    .tracks
                    .iter()
                    .position(|t| !t.locked && !t.is_adjustment())
            })
            .unwrap_or_else(|| self.insert_track(TrackKind::Video));

        let empty_item = if let Some(existing) = self.project.media.iter().find(|m| {
            matches!(
                &m.source,
                MediaSource::Generated(GenSpec::SolidColor { color })
                if color[3] == 0.0 && m.info == "Evento vazio"
            )
        }) {
            existing.clone()
        } else {
            let item = MediaItem {
                id: new_id(),
                name: "Evento vazio".into(),
                source: MediaSource::Generated(GenSpec::SolidColor {
                    color: [0.0, 0.0, 0.0, 0.0],
                }),
                duration: None,
                has_video: false,
                has_audio: false,
                width: 0,
                height: 0,
                fps: None,
                info: "Evento vazio".into(),
                proxy_path: None,
            };
            self.project.media.push(item.clone());
            item
        };

        let fd = self
            .project
            .settings
            .frame_dur()
            .max(Ticks::from_millis_i64(1));
        let (start, len) = match self.time_sel {
            Some((a, b)) if b > a => (a.max(Ticks::ZERO), (b - a).max(fd)),
            _ => (self.cursor.max(Ticks::ZERO), Ticks::from_secs_i64(5)),
        };
        let mut ev = Event::new(&empty_item, start, len);
        ev.name = "Evento vazio".into();
        let id = ev.id;
        if let Some(tr) = self.project.tracks.get_mut(target_track) {
            tr.events.push(ev);
        }
        self.project.sort_events();
        self.sel_track = Some(target_track);
        self.sel_events.clear();
        self.sel_events.insert(id);
        self.changed();
        self.set_status("Evento vazio inserido na timeline.");
        id
    }

    /// Aplica cortes detectados na mídia (`media_cuts` em tempo da mídia) dividindo
    /// o evento ou criando marcadores na timeline em um único passo de Desfazer.
    pub fn apply_scene_cuts(
        &mut self,
        event_id: Id,
        media_cuts: &[Ticks],
        create_markers: bool,
    ) -> usize {
        let Some((ti, ei)) = self.project.find_event(event_id) else {
            return 0;
        };
        if self.project.tracks[ti].locked {
            return 0;
        }
        let ev = self.project.tracks[ti].events[ei].clone();
        if ev.locked {
            return 0;
        }
        let fd = self
            .project
            .settings
            .frame_dur()
            .max(Ticks::from_millis_i64(1));
        let mut cuts_timeline: Vec<Ticks> = Vec::new();
        for &mc in media_cuts {
            let rel_media = mc - ev.offset;
            if rel_media <= Ticks::ZERO {
                continue;
            }
            let Some(rel_tl) = ev.rate.unscale_ticks(rel_media) else {
                continue;
            };
            let t = if ev.reverse {
                ev.end() - rel_tl
            } else {
                ev.start + rel_tl
            };
            if t > ev.start + fd && t < ev.end() - fd {
                cuts_timeline.push(t);
            }
        }
        cuts_timeline.sort();
        cuts_timeline.dedup();
        if cuts_timeline.is_empty() {
            return 0;
        }

        if create_markers {
            self.checkpoint();
            let mut count = 0usize;
            for (idx, t) in cuts_timeline.into_iter().enumerate() {
                if !self.project.markers.iter().any(|m| m.t.abs_diff(t) < fd) {
                    self.project.markers.push(Marker {
                        id: new_id(),
                        t,
                        label: format!("Cena {}", idx + 1),
                        ..Default::default()
                    });
                    count += 1;
                }
            }
            if count > 0 {
                self.project.markers.sort_by_key(|m| m.t);
                self.changed();
                self.set_status(format!("{count} marcador(es) de cena adicionado(s)."));
            } else {
                self.undo.pop();
            }
            return count;
        }

        self.checkpoint();
        let mut group_map: HashMap<Id, Id> = HashMap::new();
        let mut current_id = event_id;
        let mut splits = 0usize;
        for t in cuts_timeline {
            let Some((cur_ti, cur_ei)) = self.project.find_event(current_id) else {
                break;
            };
            let Some(track) = self.project.tracks.get_mut(cur_ti) else {
                break;
            };
            let cur_ev = &track.events[cur_ei];
            if t <= cur_ev.start + fd || t >= cur_ev.end() - fd {
                continue;
            }
            if let Some(right_id) = split_event_in_track(track, cur_ei, t, &mut group_map) {
                current_id = right_id;
                splits += 1;
            }
        }
        if splits > 0 {
            self.project.sort_events();
            self.changed();
            self.set_status(format!(
                "Detecção de cenas: evento dividido em {} parte(s).",
                splits + 1
            ));
        } else {
            self.undo.pop();
        }
        splits
    }

    /// Entra ou sai do Modo de Edição Expandido (`p. 329`), procurando a junção de
    /// corte mais próxima do cursor (ou da borda direita da seleção).
    pub fn toggle_expanded_edit(&mut self) -> bool {
        if self.expanded_edit.is_some() {
            self.expanded_edit = None;
            self.set_status("Modo de edição expandido desativado.");
            return false;
        }
        let tol = Ticks::from_millis_i64(2);
        let mut candidate: Option<(usize, Id, bool, Ticks)> = None;
        if let Some(&id) = self.sel_events.iter().next() {
            if let Some((ti, ei)) = self.project.find_event(id) {
                let tr = &self.project.tracks[ti];
                if tr.kind == TrackKind::Video && !tr.locked && !tr.events[ei].locked {
                    let ev = &tr.events[ei];
                    let use_left = (self.cursor - ev.start).abs() <= (ev.end() - self.cursor).abs();
                    candidate = Some((
                        ti,
                        ev.id,
                        !use_left,
                        if use_left { ev.start } else { ev.end() },
                    ));
                }
            }
        }
        if candidate.is_none() {
            let mut best_dist = Ticks::MAX;
            for (ti, tr) in self.project.tracks.iter().enumerate() {
                if tr.kind != TrackKind::Video || tr.locked || tr.is_adjustment() {
                    continue;
                }
                for ev in &tr.events {
                    if ev.locked {
                        continue;
                    }
                    let d_end = (ev.end() - self.cursor).abs();
                    if d_end < best_dist {
                        best_dist = d_end;
                        candidate = Some((ti, ev.id, true, ev.end()));
                    }
                    let d_start = (ev.start - self.cursor).abs();
                    if d_start < best_dist {
                        best_dist = d_start;
                        candidate = Some((ti, ev.id, false, ev.start));
                    }
                }
            }
        }
        let Some((ti, eid, right_edge, _)) = candidate else {
            self.set_status("Nenhum evento de vídeo destravado para abrir o modo expandido.");
            return false;
        };
        let _ = tol;
        self.enter_expanded_edit_at(ti, eid, right_edge)
    }

    /// Abre o Modo de Edição Expandido focado na borda (`right_edge`) de `event_id` em `track_idx`.
    pub fn enter_expanded_edit_at(
        &mut self,
        track_idx: usize,
        event_id: Id,
        right_edge: bool,
    ) -> bool {
        let Some(tr) = self.project.tracks.get(track_idx) else {
            return false;
        };
        if tr.locked {
            return false;
        }
        let Some(ev) = tr.events.iter().find(|e| e.id == event_id && !e.locked) else {
            return false;
        };
        let tol = Ticks::from_millis_i64(2);
        let edit_point = if right_edge { ev.end() } else { ev.start };
        let (left_event_id, right_event_id) = if right_edge {
            let neighbor = tr
                .events
                .iter()
                .filter(|other| {
                    other.id != ev.id && !other.locked && other.start.abs_diff(edit_point) <= tol
                })
                .min_by_key(|other| other.start.abs_diff(edit_point))
                .map(|o| o.id);
            (Some(ev.id), neighbor)
        } else {
            let neighbor = tr
                .events
                .iter()
                .filter(|other| {
                    other.id != ev.id && !other.locked && other.end().abs_diff(edit_point) <= tol
                })
                .min_by_key(|other| other.end().abs_diff(edit_point))
                .map(|o| o.id);
            (neighbor, Some(ev.id))
        };
        let focus = if left_event_id.is_some() && right_event_id.is_some() {
            ExpandedEdgeFocus::Both
        } else if left_event_id.is_some() {
            ExpandedEdgeFocus::LeftOut
        } else {
            ExpandedEdgeFocus::RightIn
        };
        self.expanded_edit = Some(ExpandedEditState {
            track_idx,
            left_event_id,
            right_event_id,
            edit_point,
            focus,
        });
        self.cursor = edit_point;
        self.sel_track = Some(track_idx);
        self.set_status("Modo de edição expandido ativo (A/B).");
        true
    }

    pub fn set_expanded_focus(&mut self, focus: ExpandedEdgeFocus) -> bool {
        let Some(exp) = self.expanded_edit.as_mut() else {
            return false;
        };
        exp.focus = focus;
        true
    }

    /// Desloca a junção ativa no Modo de Edição Expandido em `delta` ticks em 1 passo de Desfazer.
    pub fn nudge_expanded_edit(&mut self, delta: Ticks) -> bool {
        let Some(exp) = self.expanded_edit.clone() else {
            return false;
        };
        let Some(tr) = self.project.tracks.get(exp.track_idx) else {
            self.expanded_edit = None;
            return false;
        };
        if tr.locked || delta == Ticks::ZERO {
            return false;
        }
        let fd = self
            .project
            .settings
            .frame_dur()
            .max(Ticks::from_millis_i64(1));
        let snap: Vec<TrackEventSnapshot> = tr
            .events
            .iter()
            .filter(|e| !e.locked)
            .map(|e| TrackEventSnapshot {
                id: e.id,
                track_idx: exp.track_idx,
                start: e.start,
                length: e.length,
                offset: e.offset,
                rate: e.rate,
                reverse: e.reverse,
                looped: e.looped,
                media_dur: self.project.media(e.media_id).and_then(|m| m.duration),
            })
            .collect();

        match (exp.focus, exp.left_event_id, exp.right_event_id) {
            (ExpandedEdgeFocus::Both, Some(_lid), Some(rid)) => {
                let mut probe = self.project.clone();
                if !apply_roll_from_snapshot(&mut probe, exp.track_idx, &snap, rid, true, delta, fd)
                {
                    return false;
                }
                self.checkpoint();
                self.project = probe;
                let new_pt = self
                    .project
                    .event(rid)
                    .map(|e| e.start)
                    .unwrap_or(exp.edit_point);
                if let Some(st) = self.expanded_edit.as_mut() {
                    st.edit_point = new_pt;
                }
                self.cursor = new_pt;
                self.changed();
                true
            }
            (ExpandedEdgeFocus::LeftOut, Some(lid), _)
            | (ExpandedEdgeFocus::Both, Some(lid), None) => {
                let Some(left_snap) = snap.iter().find(|s| s.id == lid) else {
                    return false;
                };
                let min_dt = fd - left_snap.length;
                let mut clamped_dt = delta.max(min_dt);
                if !left_snap.looped && !left_snap.reverse {
                    if let (Some(md), Some(avail)) = (
                        left_snap.media_dur,
                        left_snap.rate.unscale_ticks(left_snap.offset),
                    ) {
                        if let Some(full) = left_snap.rate.unscale_ticks(md) {
                            let max_len = (full - avail).max(fd);
                            clamped_dt = clamped_dt.min(max_len - left_snap.length);
                        }
                    }
                }
                if clamped_dt == Ticks::ZERO {
                    return false;
                }
                self.checkpoint();
                if let Some(track) = self.project.tracks.get_mut(exp.track_idx) {
                    let boundary = left_snap.start + left_snap.length;
                    for ev in &mut track.events {
                        if ev.id == lid {
                            ev.length = (left_snap.length + clamped_dt).max(fd);
                            ev.fade_in.len = ev.fade_in.len.min(ev.length);
                            ev.fade_out.len = ev.fade_out.len.min(ev.length - ev.fade_in.len);
                        } else if !ev.locked && ev.start >= boundary - Ticks::from_millis_i64(2) {
                            ev.start = (ev.start + clamped_dt).max(Ticks::ZERO);
                        }
                    }
                }
                let new_pt = self
                    .project
                    .event(lid)
                    .map(|e| e.end())
                    .unwrap_or(exp.edit_point);
                if let Some(st) = self.expanded_edit.as_mut() {
                    st.edit_point = new_pt;
                }
                self.cursor = new_pt;
                self.changed();
                true
            }
            (ExpandedEdgeFocus::RightIn, _, Some(rid))
            | (ExpandedEdgeFocus::Both, None, Some(rid)) => {
                let Some(right_snap) = snap.iter().find(|s| s.id == rid) else {
                    return false;
                };
                let max_dt = right_snap.length - fd;
                let mut clamped_dt = delta.min(max_dt);
                if right_snap.start + clamped_dt < Ticks::ZERO {
                    clamped_dt = -right_snap.start;
                }
                if !right_snap.looped && !right_snap.reverse {
                    if let Some(back) = right_snap.rate.unscale_ticks(right_snap.offset) {
                        clamped_dt = clamped_dt.max(-back);
                    }
                }
                if clamped_dt == Ticks::ZERO {
                    return false;
                }
                self.checkpoint();
                if let Some(ev) = self.project.event_mut(rid) {
                    ev.start = right_snap.start + clamped_dt;
                    ev.length = (right_snap.length - clamped_dt).max(fd);
                    if !ev.reverse {
                        let new_off =
                            (right_snap.offset + ev.rate.scale_ticks(clamped_dt)).max(Ticks::ZERO);
                        let shift = new_off - ev.offset;
                        ev.shift_offsets(shift);
                    }
                    ev.fade_in.len = ev.fade_in.len.min(ev.length);
                    ev.fade_out.len = ev.fade_out.len.min(ev.length - ev.fade_in.len);
                }
                let new_pt = self
                    .project
                    .event(rid)
                    .map(|e| e.start)
                    .unwrap_or(exp.edit_point);
                if let Some(st) = self.expanded_edit.as_mut() {
                    st.edit_point = new_pt;
                }
                self.cursor = new_pt;
                self.changed();
                true
            }
            _ => false,
        }
    }

    /// Alterna armar faixa de áudio para gravação (`Rec`).
    pub fn toggle_record_arm(&mut self, track_idx: usize) -> bool {
        let Some(tr) = self.project.tracks.get_mut(track_idx) else {
            return false;
        };
        if tr.kind != TrackKind::Audio || tr.locked {
            return false;
        }
        tr.record_armed = !tr.record_armed;
        let armed = tr.record_armed;
        let name = tr.name.clone();
        self.changed();
        self.set_status(if armed {
            format!("Faixa '{name}' armada para gravação.")
        } else {
            format!("Gravação desarmada na faixa '{name}'.")
        });
        armed
    }

    /// Lista `(índice_da_trilha, track_id)` de todas as faixas de áudio armadas e destravadas.
    pub fn armed_audio_tracks(&self) -> Vec<(usize, Id)> {
        self.project
            .tracks
            .iter()
            .enumerate()
            .filter_map(|(i, tr)| {
                (tr.kind == TrackKind::Audio && tr.record_armed && !tr.locked).then_some((i, tr.id))
            })
            .collect()
    }

    /// Adiciona a mídia gravada ao projeto e cria eventos nas faixas de áudio armadas
    /// em um único passo de Desfazer (§3.3).
    pub fn commit_recorded_clip(
        &mut self,
        item: MediaItem,
        track_targets: &[Id],
        start: Ticks,
        len: Ticks,
    ) -> Vec<Id> {
        let valid_targets: Vec<usize> = track_targets
            .iter()
            .filter_map(|tid| {
                self.project
                    .tracks
                    .iter()
                    .position(|tr| tr.id == *tid && tr.kind == TrackKind::Audio && !tr.locked)
            })
            .collect();
        if valid_targets.is_empty() {
            return Vec::new();
        }
        self.checkpoint();
        let dur = len
            .max(self.project.settings.frame_dur())
            .max(Ticks::from_millis_i64(10));
        let start = start.max(Ticks::ZERO);
        let mut created = Vec::new();
        self.project.media.push(item.clone());
        for ti in valid_targets {
            let mut ev = Event::new(&item, start, dur);
            ev.name = item.name.clone();
            let id = ev.id;
            self.project.tracks[ti].events.push(ev);
            created.push(id);
        }
        self.project.sort_events();
        self.sel_events.clear();
        for &id in &created {
            self.sel_events.insert(id);
        }
        self.cursor = start + dur;
        self.changed();
        self.set_status(format!(
            "Gravação concluída: {} evento(s) adicionado(s).",
            created.len()
        ));
        created
    }

    // =========================================================================
    // Mídia do Projeto: Bins, Pastas Inteligentes, Substituição e Storyboards
    // =========================================================================

    /// Cria uma pasta manual (*bin*) na Mídia do Projeto respeitando aciclicidade.
    pub fn create_media_bin(&mut self, name: &str, parent_id: Option<Id>) -> Option<Id> {
        if self.project.media_bins.len() >= MAX_MEDIA_BINS {
            self.set_status("Limite máximo de pastas de mídia atingido.");
            return None;
        }
        if let Some(pid) = parent_id {
            if !self.project.media_bins.iter().any(|b| b.id == pid) {
                return None;
            }
        }
        let clean = name.trim();
        let label = if clean.is_empty() {
            format!("Pasta {}", self.project.media_bins.len() + 1)
        } else {
            clean.chars().take(128).collect()
        };
        let id = new_id();
        if !self.project.validate_bin_parent(id, parent_id) {
            return None;
        }
        self.checkpoint();
        self.project.media_bins.push(MediaBin {
            id,
            name: label.clone(),
            parent_id,
            media_ids: Vec::new(),
        });
        self.changed();
        self.set_status(format!("Pasta \"{label}\" criada."));
        Some(id)
    }

    pub fn rename_media_bin(&mut self, bin_id: Id, name: &str) -> bool {
        let clean: String = name.trim().chars().take(128).collect();
        if clean.is_empty() {
            return false;
        }
        let Some(pos) = self.project.media_bins.iter().position(|b| b.id == bin_id) else {
            return false;
        };
        if self.project.media_bins[pos].name == clean {
            return false;
        }
        self.checkpoint();
        self.project.media_bins[pos].name = clean;
        self.changed();
        true
    }

    pub fn delete_media_bin(&mut self, bin_id: Id) -> bool {
        let Some(pos) = self.project.media_bins.iter().position(|b| b.id == bin_id) else {
            return false;
        };
        self.checkpoint();
        let parent_of_deleted = self.project.media_bins[pos].parent_id;
        self.project.media_bins.remove(pos);
        for bin in &mut self.project.media_bins {
            if bin.parent_id == Some(bin_id) {
                bin.parent_id = parent_of_deleted;
            }
        }
        self.changed();
        self.set_status("Pasta de mídia removida.");
        true
    }

    /// Adiciona `media_id` à pasta `bin_id` (ou remove de todas as pastas quando `None`).
    pub fn assign_media_to_bin(&mut self, media_id: Id, bin_id: Option<Id>) -> bool {
        if self.project.media(media_id).is_none() {
            return false;
        }
        match bin_id {
            Some(bid) => {
                let Some(pos) = self.project.media_bins.iter().position(|b| b.id == bid) else {
                    return false;
                };
                if self.project.media_bins[pos].media_ids.contains(&media_id) {
                    return false;
                }
                self.checkpoint();
                self.project.media_bins[pos].media_ids.push(media_id);
                let name = self.project.media_bins[pos].name.clone();
                self.changed();
                self.set_status(format!("Mídia adicionada à pasta \"{name}\"."));
                true
            }
            None => {
                let had_any = self
                    .project
                    .media_bins
                    .iter()
                    .any(|b| b.media_ids.contains(&media_id));
                if !had_any {
                    return false;
                }
                self.checkpoint();
                for bin in &mut self.project.media_bins {
                    bin.media_ids.retain(|&id| id != media_id);
                }
                self.changed();
                self.set_status("Mídia removida das pastas.");
                true
            }
        }
    }

    pub fn remove_media_from_bin(&mut self, media_id: Id, bin_id: Id) -> bool {
        let Some(pos) = self.project.media_bins.iter().position(|b| b.id == bin_id) else {
            return false;
        };
        if !self.project.media_bins[pos].media_ids.contains(&media_id) {
            return false;
        }
        self.checkpoint();
        self.project.media_bins[pos]
            .media_ids
            .retain(|&id| id != media_id);
        self.changed();
        true
    }

    pub fn create_smart_bin(&mut self, name: &str, rule: SmartBinRule, query: &str) -> Option<Id> {
        if self.project.smart_bins.len() >= MAX_MEDIA_BINS {
            self.set_status("Limite máximo de pastas inteligentes atingido.");
            return None;
        }
        let clean = name.trim();
        let label = if clean.is_empty() {
            rule.label().to_string()
        } else {
            clean.chars().take(128).collect()
        };
        let q: String = query.trim().chars().take(128).collect();
        self.checkpoint();
        let id = new_id();
        self.project.smart_bins.push(SmartBin {
            id,
            name: label.clone(),
            rule,
            query: q,
        });
        self.changed();
        self.set_status(format!("Pasta inteligente \"{label}\" criada."));
        Some(id)
    }

    pub fn update_smart_bin(
        &mut self,
        id: Id,
        name: &str,
        rule: SmartBinRule,
        query: &str,
    ) -> bool {
        let Some(pos) = self.project.smart_bins.iter().position(|s| s.id == id) else {
            return false;
        };
        let clean: String = name.trim().chars().take(128).collect();
        if clean.is_empty() {
            return false;
        }
        self.checkpoint();
        let sb = &mut self.project.smart_bins[pos];
        sb.name = clean;
        sb.rule = rule;
        sb.query = query.trim().chars().take(128).collect();
        self.changed();
        true
    }

    pub fn delete_smart_bin(&mut self, id: Id) -> bool {
        let Some(pos) = self.project.smart_bins.iter().position(|s| s.id == id) else {
            return false;
        };
        self.checkpoint();
        self.project.smart_bins.remove(pos);
        self.changed();
        self.set_status("Pasta inteligente removida.");
        true
    }

    /// Seleciona todos os eventos da timeline que utilizam `media_id` (p. 144).
    pub fn select_events_using_media(&mut self, media_id: Id) -> usize {
        self.sel_events.clear();
        let mut first_track = None;
        for (ti, t) in self.project.tracks.iter().enumerate() {
            for e in &t.events {
                if crate::takes::uses_media(e, media_id) {
                    self.sel_events.insert(e.id);
                    if first_track.is_none() {
                        first_track = Some(ti);
                    }
                }
            }
        }
        if first_track.is_some() {
            self.sel_track = first_track;
        }
        self.active_media_id = Some(media_id);
        let n = self.sel_events.len();
        let label = self
            .project
            .media(media_id)
            .map(|m| m.name.as_str())
            .unwrap_or("mídia");
        self.set_status(format!("{n} evento(s) selecionado(s) para \"{label}\"."));
        n
    }

    /// Remove uma mídia específica do projeto e seus takes/eventos, em um passo de Desfazer.
    pub fn remove_media_by_id(&mut self, media_id: Id, cache: Option<&MediaCache>) -> bool {
        if self.project.media(media_id).is_none() {
            return false;
        }
        self.checkpoint();
        for t in &mut self.project.tracks {
            t.events.retain_mut(|e| {
                !crate::takes::uses_media(e, media_id) || crate::takes::remove_media(e, media_id)
            });
        }
        for s in &mut self.project.sequences {
            for t in &mut s.tracks {
                t.events.retain_mut(|e| {
                    !crate::takes::uses_media(e, media_id)
                        || crate::takes::remove_media(e, media_id)
                });
            }
        }
        self.project.media.retain(|m| m.id != media_id);
        for bin in &mut self.project.media_bins {
            bin.media_ids.retain(|&id| id != media_id);
        }
        for st in &mut self.project.storyboards {
            st.items.retain(|it| it.media_id != media_id);
        }
        self.sel_events
            .retain(|eid| self.project.event(*eid).is_some());
        if self.active_media_id == Some(media_id) {
            self.active_media_id = None;
        }
        self.removed_media_ids.push(media_id);
        if let Some(c) = cache {
            c.remove(media_id);
        }
        self.changed();
        self.set_status("Mídia removida do projeto.");
        true
    }

    /// Substitui a mídia `target_id` in-loco por `replacement` mantendo IDs e propriedades dos eventos.
    pub fn replace_media_item(&mut self, target_id: Id, replacement: MediaItem) -> bool {
        let Some(pos) = self.project.media.iter().position(|m| m.id == target_id) else {
            return false;
        };
        self.checkpoint();
        let old_name = self.project.media[pos].name.clone();
        let mut updated = replacement;
        updated.id = target_id;
        updated.proxy_path = None;
        let new_name = updated.name.clone();
        let new_dur = updated.duration;
        self.project.media[pos] = updated;

        for t in &mut self.project.tracks {
            for e in &mut t.events {
                if e.media_id == target_id {
                    if e.name == old_name {
                        e.name = new_name.clone();
                    }
                    if e.take_name == old_name {
                        e.take_name = new_name.clone();
                    }
                    if !e.looped {
                        if let Some(d) = new_dur.filter(|d| *d > Ticks::ZERO) {
                            e.offset = e.offset.clamp_safe(Ticks::ZERO, d);
                        }
                    }
                }
                for alt in &mut e.alt_takes {
                    if alt.media_id == target_id && alt.name == old_name {
                        alt.name = new_name.clone();
                    }
                    if !e.looped {
                        if let Some(d) = new_dur.filter(|d| *d > Ticks::ZERO) {
                            alt.offset = alt.offset.clamp_safe(Ticks::ZERO, d);
                        }
                    }
                }
            }
        }
        for s in &mut self.project.sequences {
            for t in &mut s.tracks {
                for e in &mut t.events {
                    if e.media_id == target_id {
                        if e.name == old_name {
                            e.name = new_name.clone();
                        }
                        if e.take_name == old_name {
                            e.take_name = new_name.clone();
                        }
                        if !e.looped {
                            if let Some(d) = new_dur.filter(|d| *d > Ticks::ZERO) {
                                e.offset = e.offset.clamp_safe(Ticks::ZERO, d);
                            }
                        }
                    }
                    for alt in &mut e.alt_takes {
                        if alt.media_id == target_id && alt.name == old_name {
                            alt.name = new_name.clone();
                        }
                        if !e.looped {
                            if let Some(d) = new_dur.filter(|d| *d > Ticks::ZERO) {
                                alt.offset = alt.offset.clamp_safe(Ticks::ZERO, d);
                            }
                        }
                    }
                }
            }
        }
        self.active_media_id = Some(target_id);
        self.changed();
        self.set_status(format!("Mídia substituída por \"{new_name}\"."));
        true
    }

    /// Redireciona eventos, takes e storyboards que usam `old_id` para `new_id` (p. 137/144).
    pub fn swap_media_references(&mut self, old_id: Id, new_id: Id) -> usize {
        if old_id == new_id {
            return 0;
        }
        let Some(old_name) = self.project.media(old_id).map(|m| m.name.clone()) else {
            return 0;
        };
        let Some(new_item) = self.project.media(new_id).cloned() else {
            return 0;
        };
        self.checkpoint();
        let mut touched = 0usize;
        for t in &mut self.project.tracks {
            for e in &mut t.events {
                if crate::takes::replace_media_id(e, old_id, new_id, &old_name, &new_item.name) {
                    if !e.looped {
                        if let Some(d) = new_item.duration.filter(|d| *d > Ticks::ZERO) {
                            e.offset = e.offset.clamp_safe(Ticks::ZERO, d);
                        }
                    }
                    touched += 1;
                }
            }
        }
        for s in &mut self.project.sequences {
            for t in &mut s.tracks {
                for e in &mut t.events {
                    if crate::takes::replace_media_id(e, old_id, new_id, &old_name, &new_item.name)
                    {
                        if !e.looped {
                            if let Some(d) = new_item.duration.filter(|d| *d > Ticks::ZERO) {
                                e.offset = e.offset.clamp_safe(Ticks::ZERO, d);
                            }
                        }
                        touched += 1;
                    }
                }
            }
        }
        for st in &mut self.project.storyboards {
            for it in &mut st.items {
                if it.media_id == old_id {
                    it.media_id = new_id;
                    if it.label == old_name {
                        it.label = new_item.name.clone();
                    }
                }
            }
        }
        self.active_media_id = Some(new_id);
        self.changed();
        self.set_status(format!(
            "{touched} evento(s) atualizado(s) para \"{}\".",
            new_item.name
        ));
        touched
    }

    pub fn create_storyboard(&mut self, name: &str) -> Option<Id> {
        if self.project.storyboards.len() >= MAX_STORYBOARDS {
            self.set_status("Limite máximo de storyboards atingido.");
            return None;
        }
        let clean = name.trim();
        let label = if clean.is_empty() {
            format!("Storyboard {}", self.project.storyboards.len() + 1)
        } else {
            clean.chars().take(128).collect()
        };
        self.checkpoint();
        let id = new_id();
        self.project.storyboards.push(Storyboard {
            id,
            name: label.clone(),
            items: Vec::new(),
        });
        self.active_storyboard_id = Some(id);
        self.changed();
        self.set_status(format!("Storyboard \"{label}\" criado."));
        Some(id)
    }

    pub fn rename_storyboard(&mut self, id: Id, name: &str) -> bool {
        let clean: String = name.trim().chars().take(128).collect();
        if clean.is_empty() {
            return false;
        }
        let Some(pos) = self.project.storyboards.iter().position(|s| s.id == id) else {
            return false;
        };
        if self.project.storyboards[pos].name == clean {
            return false;
        }
        self.checkpoint();
        self.project.storyboards[pos].name = clean;
        self.changed();
        true
    }

    pub fn delete_storyboard(&mut self, id: Id) -> bool {
        let Some(pos) = self.project.storyboards.iter().position(|s| s.id == id) else {
            return false;
        };
        self.checkpoint();
        self.project.storyboards.remove(pos);
        if self.active_storyboard_id == Some(id) {
            self.active_storyboard_id = self.project.storyboards.first().map(|s| s.id);
        }
        self.changed();
        self.set_status("Storyboard removido.");
        true
    }

    pub fn add_media_to_storyboard(&mut self, sb_id: Id, media_id: Id) -> bool {
        let Some(m_name) = self.project.media(media_id).map(|m| m.name.clone()) else {
            return false;
        };
        let Some(pos) = self.project.storyboards.iter().position(|s| s.id == sb_id) else {
            return false;
        };
        if self.project.storyboards[pos].items.len() >= MAX_STORYBOARD_ITEMS {
            return false;
        }
        self.checkpoint();
        self.project.storyboards[pos].items.push(StoryboardItem {
            media_id,
            label: m_name,
            trim_in: None,
            duration_override: None,
        });
        self.changed();
        true
    }

    pub fn remove_storyboard_item(&mut self, sb_id: Id, idx: usize) -> bool {
        let Some(pos) = self.project.storyboards.iter().position(|s| s.id == sb_id) else {
            return false;
        };
        if idx >= self.project.storyboards[pos].items.len() {
            return false;
        }
        self.checkpoint();
        self.project.storyboards[pos].items.remove(idx);
        self.changed();
        true
    }

    pub fn move_storyboard_item(&mut self, sb_id: Id, from: usize, to: usize) -> bool {
        let Some(pos) = self.project.storyboards.iter().position(|s| s.id == sb_id) else {
            return false;
        };
        let n = self.project.storyboards[pos].items.len();
        if from >= n || to >= n || from == to {
            return false;
        }
        self.checkpoint();
        let item = self.project.storyboards[pos].items.remove(from);
        self.project.storyboards[pos].items.insert(to, item);
        self.changed();
        true
    }

    /// Calcula a duração total prevista de um storyboard.
    pub fn storyboard_duration(&self, sb_id: Id, still_secs: f32) -> Ticks {
        let Some(sb) = self.project.storyboards.iter().find(|s| s.id == sb_id) else {
            return Ticks::ZERO;
        };
        let still_len = Ticks::from_secs_f64_clamped(f64::from(still_secs.clamp(0.1, 3600.0)));
        let min_len = self.project.settings.frame_dur().max(Ticks::from_raw(1));
        sb.items
            .iter()
            .filter_map(|it| {
                let m = self.project.media(it.media_id)?;
                Some(
                    it.duration_override
                        .or(m.duration)
                        .unwrap_or(still_len)
                        .max(min_len),
                )
            })
            .fold(Ticks::ZERO, |acc, d| {
                Ticks::from_raw(acc.0.saturating_add(d.0))
            })
    }

    /// Insere todos os itens de um storyboard em sequência a partir de `start`,
    /// em um único passo atômico de Desfazer.
    pub fn insert_storyboard_at_cursor(
        &mut self,
        sb_id: Id,
        start: Ticks,
        still_secs: f32,
    ) -> Vec<Id> {
        let Some(sb) = self
            .project
            .storyboards
            .iter()
            .find(|s| s.id == sb_id)
            .cloned()
        else {
            return Vec::new();
        };
        let still_len = Ticks::from_secs_f64_clamped(f64::from(still_secs.clamp(0.1, 3600.0)));
        let min_len = self.project.settings.frame_dur().max(Ticks::from_raw(1));
        let mut plan = Vec::new();
        for it in &sb.items {
            if let Some(m) = self.project.media(it.media_id).cloned() {
                if !m.has_video && !m.has_audio {
                    continue;
                }
                let dur = it
                    .duration_override
                    .or(m.duration)
                    .unwrap_or(still_len)
                    .max(min_len);
                let trim_in = it.trim_in.unwrap_or(Ticks::ZERO).max(Ticks::ZERO);
                plan.push((m, trim_in, dur));
            }
        }
        if plan.is_empty() {
            self.set_status("Storyboard vazio ou sem mídias válidas.");
            return Vec::new();
        }
        self.checkpoint();
        let mut video_ti: Option<usize> = None;
        let mut audio_ti: Option<usize> = None;
        let mut pos = start.max(Ticks::ZERO);
        let mut created = Vec::new();
        for (m, trim_in, dur) in plan {
            let group = if m.has_video && m.has_audio {
                Some(new_id())
            } else {
                None
            };
            if m.has_video {
                let ti = match video_ti {
                    Some(i)
                        if self
                            .project
                            .tracks
                            .get(i)
                            .is_some_and(|t| t.kind == TrackKind::Video) =>
                    {
                        i
                    }
                    _ => {
                        let found = self
                            .sel_track
                            .filter(|&i| {
                                self.project
                                    .tracks
                                    .get(i)
                                    .is_some_and(|t| t.kind == TrackKind::Video)
                            })
                            .or_else(|| {
                                self.project
                                    .tracks
                                    .iter()
                                    .position(|t| t.kind == TrackKind::Video)
                            })
                            .unwrap_or_else(|| self.insert_track(TrackKind::Video));
                        video_ti = Some(found);
                        found
                    }
                };
                let mut e = Event::new(&m, pos, dur);
                e.offset = trim_in;
                e.group = group;
                if m.duration.is_none() {
                    e.looped = false;
                }
                created.push(e.id);
                self.project.tracks[ti].events.push(e);
            }
            if m.has_audio {
                let ti = match audio_ti {
                    Some(i)
                        if self
                            .project
                            .tracks
                            .get(i)
                            .is_some_and(|t| t.kind == TrackKind::Audio) =>
                    {
                        i
                    }
                    _ => {
                        let found = self
                            .project
                            .tracks
                            .iter()
                            .position(|t| t.kind == TrackKind::Audio)
                            .unwrap_or_else(|| self.insert_track(TrackKind::Audio));
                        audio_ti = Some(found);
                        found
                    }
                };
                let mut e = Event::new(&m, pos, dur);
                e.offset = trim_in;
                e.group = group;
                created.push(e.id);
                self.project.tracks[ti].events.push(e);
            }
            pos = Ticks::from_raw(pos.0.saturating_add(dur.0));
        }
        self.project.sort_events();
        self.sel_events = created.iter().copied().collect();
        self.cursor = pos;
        self.changed();
        self.set_status(format!(
            "Storyboard \"{}\" inserido: {} evento(s).",
            sb.name,
            created.len()
        ));
        created
    }

    /// Aplica um efeito de vídeo à saída mestre de vídeo (Video Output FX,
    /// ao soltar um efeito diretamente na janela Prévia de Vídeo, p. 55).
    pub fn apply_video_fx_to_output(&mut self, kind: FxKind) -> bool {
        if kind.is_audio() {
            self.set_status("Efeitos de áudio devem ser aplicados em trilhas ou eventos de áudio.");
            return false;
        }
        self.checkpoint();
        self.project.master.video_fx.push(FxInstance::new(kind));
        self.changed();
        self.set_status(format!(
            "Efeito \"{}\" aplicado à saída de vídeo (Prévia).",
            kind.name()
        ));
        true
    }

    // -----------------------------------------------------------------
    // Trilhas: pastas, composição Pai/Filho, máscaras, aninhamento,
    // multicâmera, padrões e renderizar para nova trilha (p. 382–426, 740)
    // -----------------------------------------------------------------

    /// Insere uma nova trilha aplicando propriedades padrão configuradas pelo usuário (p. 414).
    pub fn insert_track_with_defaults(
        &mut self,
        kind: TrackKind,
        defaults: &crate::prefs::DefaultTrackPrefs,
    ) -> usize {
        let idx = self.insert_track(kind);
        self.apply_default_track_props(idx, defaults);
        idx
    }

    /// Aplica `defaults` à trilha `ti`.
    pub fn apply_default_track_props(
        &mut self,
        ti: usize,
        defaults: &crate::prefs::DefaultTrackPrefs,
    ) -> bool {
        let Some(t) = self.project.tracks.get_mut(ti) else {
            return false;
        };
        t.mute = defaults.mute;
        t.solo = defaults.solo;
        match t.kind {
            TrackKind::Video => {
                t.height = defaults.video_height.clamp(28.0, 400.0);
                t.opacity = defaults.video_opacity.clamp(0.0, 1.0);
                t.blend = BlendMode::from_id(&defaults.video_blend);
            }
            TrackKind::Audio => {
                t.height = defaults.audio_height.clamp(28.0, 400.0);
                t.volume_db = defaults.audio_volume_db.clamp(-120.0, 24.0);
                t.pan = defaults.audio_pan.clamp(-1.0, 1.0);
            }
        }
        self.changed();
        true
    }

    /// Captura as propriedades da trilha `ti` como padrão para futuras trilhas (p. 414).
    pub fn capture_default_track_props(
        &self,
        ti: usize,
        base: &crate::prefs::DefaultTrackPrefs,
    ) -> Option<crate::prefs::DefaultTrackPrefs> {
        let t = self.project.tracks.get(ti)?;
        let mut out = base.clone();
        out.mute = t.mute;
        out.solo = t.solo;
        match t.kind {
            TrackKind::Video => {
                out.video_height = t.height.clamp(28.0, 400.0);
                out.video_opacity = t.opacity.clamp(0.0, 1.0);
                out.video_blend = t.blend.id().to_string();
            }
            TrackKind::Audio => {
                out.audio_height = t.height.clamp(28.0, 400.0);
                out.audio_volume_db = t.volume_db.clamp(-120.0, 24.0);
                out.audio_pan = t.pan.clamp(-1.0, 1.0);
            }
        }
        out.sanitize();
        Some(out)
    }

    /// Insere uma pasta recolhível de trilhas (Track Group, p. 382), envolvendo
    /// a trilha selecionada caso ela ainda não tenha pai.
    pub fn insert_track_folder(&mut self, kind: TrackKind) -> usize {
        self.checkpoint();
        let n = self.project.tracks.iter().filter(|t| t.is_folder).count() + 1;
        let sel_info = self.sel_track.and_then(|i| {
            self.project
                .tracks
                .get(i)
                .map(|t| (i, t.id, t.kind, t.parent_track_id, t.is_folder))
        });
        let insert_idx = sel_info
            .map(|(i, _, _, _, _)| i)
            .unwrap_or_else(|| match kind {
                TrackKind::Video => self
                    .project
                    .tracks
                    .iter()
                    .position(|t| t.kind == TrackKind::Audio)
                    .unwrap_or(self.project.tracks.len()),
                TrackKind::Audio => self.project.tracks.len(),
            });
        let folder_kind = sel_info.map(|(_, _, k, _, _)| k).unwrap_or(kind);
        let mut folder = Track::new(folder_kind, &format!("Pasta {n}"));
        folder.is_folder = true;
        folder.height = 44.0;
        if let Some((_, _, _, cur_parent, _)) = sel_info {
            folder.parent_track_id = cur_parent;
        }
        let folder_id = folder.id;
        self.project.tracks.insert(insert_idx, folder);
        if let Some((_, child_id, _, cur_parent, is_folder)) = sel_info {
            if !is_folder && cur_parent.is_none() {
                if let Some(child_idx) = self.project.track_index(child_id) {
                    if self
                        .project
                        .validate_track_parent(child_id, Some(folder_id))
                        .is_ok()
                    {
                        self.project.tracks[child_idx].parent_track_id = Some(folder_id);
                    }
                }
            }
        }
        self.sel_track = Some(insert_idx);
        self.changed();
        self.set_status(format!("Pasta de trilhas \"Pasta {n}\" criada."));
        insert_idx
    }

    /// Alterna o estado recolhido/expandido da pasta `ti` (ou da pasta pai, se `ti` estiver dentro dela).
    pub fn toggle_track_folder(&mut self, ti: usize) -> bool {
        let Some(t) = self.project.tracks.get(ti) else {
            return false;
        };
        let target_id = if t.is_folder {
            t.id
        } else if let Some(pid) = t.parent_track_id.filter(|pid| {
            self.project
                .tracks
                .iter()
                .any(|p| p.id == *pid && p.is_folder)
        }) {
            pid
        } else {
            return false;
        };
        let Some(fi) = self.project.track_index(target_id) else {
            return false;
        };
        let collapsed = !self.project.tracks[fi].collapsed;
        self.project.tracks[fi].collapsed = collapsed;
        self.changed();
        self.set_status(if collapsed {
            format!("Pasta \"{}\" recolhida.", self.project.tracks[fi].name)
        } else {
            format!("Pasta \"{}\" expandida.", self.project.tracks[fi].name)
        });
        true
    }

    /// Vincula a trilha `ti` como filha da trilha/pasta acima mais próxima ou `parent_id` explícito.
    pub fn set_track_parent(&mut self, ti: usize, parent_id: Option<Id>) -> Result<()> {
        let Some(child_id) = self.project.tracks.get(ti).map(|t| t.id) else {
            return Err(NeovegError::InvalidInput("trilha inválida".into()));
        };
        self.project
            .validate_track_parent(child_id, parent_id)
            .map_err(NeovegError::InvalidInput)?;
        self.checkpoint();
        self.project.tracks[ti].parent_track_id = parent_id;
        self.changed();
        Ok(())
    }

    /// Torna a trilha `ti` filha de composição da trilha acima compatível mais próxima (p. 740).
    pub fn make_compositing_child(&mut self, ti: usize) -> Result<()> {
        let Some(child) = self.project.tracks.get(ti) else {
            return Err(NeovegError::InvalidInput("trilha inválida".into()));
        };
        let child_id = child.id;
        let child_kind = child.kind;
        let cur_parent = child.parent_track_id;
        let mut candidate: Option<Id> = None;
        for up in (0..ti).rev() {
            let t = &self.project.tracks[up];
            if t.id == child_id || Some(t.id) == cur_parent {
                continue;
            }
            if !t.is_folder && t.kind != child_kind {
                continue;
            }
            if t.parent_track_id == cur_parent && !t.is_adjustment() {
                candidate = Some(t.id);
                break;
            }
        }
        let pid = candidate.ok_or_else(|| {
            NeovegError::InvalidInput(
                "não há trilha superior compatível para composição pai/filho".into(),
            )
        })?;
        self.set_track_parent(ti, Some(pid))?;
        self.set_status(format!(
            "Trilha \"{}\" definida como filha de composição.",
            self.project.tracks[ti].name
        ));
        Ok(())
    }

    /// Remove o vínculo Pai/Filho da trilha `ti`, promovendo-a a trilha raiz.
    pub fn unmake_compositing_child(&mut self, ti: usize) -> bool {
        let Some(t) = self.project.tracks.get(ti) else {
            return false;
        };
        if t.parent_track_id.is_none() {
            return false;
        }
        self.checkpoint();
        self.project.tracks[ti].parent_track_id = None;
        self.changed();
        self.set_status(format!(
            "Trilha \"{}\" promovida para o nível raiz.",
            self.project.tracks[ti].name
        ));
        true
    }

    /// Define o modo de máscara de composição da trilha `ti`.
    pub fn set_track_mask_mode(&mut self, ti: usize, mode: TrackMaskMode) -> bool {
        let Some(t) = self.project.tracks.get(ti) else {
            return false;
        };
        if t.mask_mode == mode {
            return false;
        }
        self.checkpoint();
        self.project.tracks[ti].mask_mode = mode;
        self.changed();
        true
    }

    /// Insere um projeto `.neoveg` importado como evento de linha do tempo aninhada (p. 114).
    pub fn insert_nested_project_event(
        &mut self,
        imported: crate::media::Imported,
        cache: &MediaCache,
        at: Ticks,
    ) -> Vec<Id> {
        self.checkpoint();
        let item = imported.item.clone();
        let dur = item.duration.unwrap_or_else(|| {
            self.project
                .settings
                .frame_dur()
                .max(Ticks::from_secs_i64(5))
        });
        let media_id = item.id;
        cache.insert(media_id, imported.entry);
        let registered_id = self.register_imported_media(item, None);

        let mut created = Vec::new();
        let group = (self
            .project
            .media(registered_id)
            .is_some_and(|m| m.has_video && m.has_audio))
        .then(new_id);
        let m_clone = self.project.media(registered_id).cloned();
        if let Some(m) = m_clone {
            if m.has_video {
                let v_ti = self
                    .sel_track
                    .filter(|&i| {
                        self.project
                            .tracks
                            .get(i)
                            .is_some_and(|t| t.accepts(TrackKind::Video))
                    })
                    .or_else(|| {
                        self.project
                            .tracks
                            .iter()
                            .position(|t| t.accepts(TrackKind::Video))
                    })
                    .unwrap_or_else(|| self.insert_track(TrackKind::Video));
                let mut ev = Event::new(&m, at, dur);
                ev.group = group;
                created.push(ev.id);
                self.project.tracks[v_ti].events.push(ev);
            }
            if m.has_audio {
                let a_ti = self
                    .project
                    .tracks
                    .iter()
                    .position(|t| t.accepts(TrackKind::Audio))
                    .unwrap_or_else(|| self.insert_track(TrackKind::Audio));
                let mut ev = Event::new(&m, at, dur);
                ev.group = group;
                created.push(ev.id);
                self.project.tracks[a_ti].events.push(ev);
            }
        }
        self.project.sort_events();
        self.sel_events = created.iter().copied().collect();
        self.changed();
        self.set_status(format!(
            "Projeto aninhado inserido na timeline ({} evento(s)).",
            created.len()
        ));
        created
    }

    /// Renderiza o intervalo selecionado (ou o projeto inteiro) e insere em uma nova trilha (p. 415).
    pub fn render_to_new_track(
        &mut self,
        cache: &MediaCache,
        range: Option<(Ticks, Ticks)>,
        out_dir: &std::path::Path,
    ) -> Result<Vec<Id>> {
        let (start, end) = range
            .or(self.time_sel)
            .filter(|(a, b)| b > a)
            .unwrap_or((Ticks::ZERO, self.project.duration()));
        if end <= start {
            return Err(NeovegError::InvalidInput(
                "projeto ou seleção vazia para renderizar em nova trilha".into(),
            ));
        }
        std::fs::create_dir_all(out_dir).map_err(|e| {
            NeovegError::InvalidInput(format!("erro criando pasta temporária: {e}"))
        })?;
        let any_video_solo = self.project.any_solo(TrackKind::Video);
        let has_video = self.project.tracks.iter().any(|t| {
            t.kind == TrackKind::Video
                && !t.is_folder
                && !t.is_adjustment()
                && !self.project.is_track_effectively_muted(t.id)
                && (!any_video_solo || t.solo)
                && t.events
                    .iter()
                    .any(|e| !e.mute && e.end() > start && e.start < end)
        });
        let any_audio_solo = self.project.any_solo(TrackKind::Audio);
        let has_audio = self.project.tracks.iter().any(|t| {
            t.kind == TrackKind::Audio
                && !t.is_folder
                && !self.project.is_track_effectively_muted(t.id)
                && (!any_audio_solo || t.solo)
                && t.events
                    .iter()
                    .any(|e| !e.mute && e.end() > start && e.start < end)
        });
        let ext = if has_video { "avi" } else { "wav" };
        let path = out_dir.join(format!("render_nova_trilha_{}.{}", new_id(), ext));
        let rs = crate::render::RenderSettings {
            path: path.clone(),
            format: if has_video {
                crate::render::RenderFormat::AviMjpeg
            } else {
                crate::render::RenderFormat::Wav
            },
            width: self.project.settings.width.clamp(16, 1920),
            height: self.project.settings.height.clamp(16, 1080),
            start,
            end,
            include_audio: has_audio,
            ..Default::default()
        };
        let prog = crate::render::RenderProgress::default();
        crate::render::run(&self.project, cache, &rs, &prog).map_err(|e| {
            NeovegError::InvalidInput(format!("falha ao renderizar nova trilha: {e}"))
        })?;

        let imported = crate::media::import_file(&path)
            .map_err(|e| NeovegError::InvalidInput(format!("falha ao reimportar render: {e}")))?;
        self.checkpoint();
        let media_id = imported.item.id;
        cache.insert(media_id, imported.entry);
        let reg_id = self.register_imported_media(imported.item, None);
        let Some(m) = self.project.media(reg_id).cloned() else {
            return Err(NeovegError::InvalidInput(
                "mídia renderizada indisponível".into(),
            ));
        };
        let dur = end - start;
        let group = (m.has_video && m.has_audio).then(new_id);
        let mut created = Vec::new();
        if m.has_video {
            let mut tr = Track::new(TrackKind::Video, "Trilha Renderizada");
            let mut ev = Event::new(&m, start, dur);
            ev.group = group;
            created.push(ev.id);
            tr.events.push(ev);
            self.project.tracks.insert(0, tr);
            self.sel_track = Some(0);
        }
        if m.has_audio {
            let mut tr = Track::new(TrackKind::Audio, "Áudio Renderizado");
            let mut ev = Event::new(&m, start, dur);
            ev.group = group;
            created.push(ev.id);
            tr.events.push(ev);
            self.project.tracks.push(tr);
            if created.len() == 1 {
                self.sel_track = Some(self.project.tracks.len().saturating_sub(1));
            }
        }
        self.project.sort_events();
        self.sel_events = created.iter().copied().collect();
        self.changed();
        self.set_status(format!(
            "Trecho renderizado para nova trilha ({} evento(s)).",
            created.len()
        ));
        Ok(created)
    }

    /// Agrupa trilhas de vídeo em uma nova trilha multicâmera com múltiplos *takes* sincronizados (p. 416).
    pub fn create_multicam_track(&mut self) -> Result<usize> {
        let mut sources: Vec<(usize, Event)> = Vec::new();
        if !self.sel_events.is_empty() {
            for (ti, t) in self.project.tracks.iter().enumerate() {
                if t.kind != TrackKind::Video || t.is_folder || t.is_adjustment() {
                    continue;
                }
                if let Some(ev) = t.events.iter().find(|e| self.sel_events.contains(&e.id)) {
                    sources.push((ti, ev.clone()));
                }
            }
        }
        if sources.len() < 2 {
            sources.clear();
            for (ti, t) in self.project.tracks.iter().enumerate() {
                if t.kind != TrackKind::Video || t.is_folder || t.is_adjustment() {
                    continue;
                }
                if let Some(ev) = t
                    .events
                    .iter()
                    .find(|e| !e.mute && e.contains(self.cursor))
                    .or_else(|| t.events.first())
                {
                    sources.push((ti, ev.clone()));
                }
            }
        }
        if sources.len() < 2 {
            return Err(NeovegError::InvalidInput(
                "são necessárias pelo menos 2 trilhas/eventos de vídeo para criar trilha multicâmera"
                    .into(),
            ));
        }
        sources.truncate(crate::takes::MAX_TAKES.min(16));
        let start = sources
            .iter()
            .map(|(_, e)| e.start)
            .min()
            .unwrap_or(Ticks::ZERO);
        let end = sources
            .iter()
            .map(|(_, e)| e.end())
            .max()
            .unwrap_or(start + self.project.settings.frame_dur());
        let len = (end - start).max(self.project.settings.frame_dur());

        self.checkpoint();
        let mut mc_track = Track::new(TrackKind::Video, "Multicâmera");
        mc_track.multicam = true;
        let first = &sources[0].1;
        let first_media = self
            .project
            .media(first.media_id)
            .cloned()
            .unwrap_or_else(|| MediaItem {
                id: first.media_id,
                name: first.name.clone(),
                source: MediaSource::Generated(GenSpec::ColorBars),
                duration: Some(len),
                width: self.project.settings.width,
                height: self.project.settings.height,
                fps: Some(self.project.settings.fps),
                has_video: true,
                has_audio: false,
                info: String::new(),
                proxy_path: None,
            });
        let mut mc_event = Event::new(&first_media, start, len);
        mc_event.offset = if first.start <= start {
            first.offset + first.rate.scale_ticks(start - first.start)
        } else {
            first.offset
        };
        mc_event.rate = first.rate;
        mc_event.take_name = format!("Câm 1 — {}", first.name);
        mc_event.name = mc_event.take_name.clone();
        for (idx, (_, src_ev)) in sources.iter().enumerate().skip(1) {
            let take_off = if src_ev.start <= start {
                src_ev.offset + src_ev.rate.scale_ticks(start - src_ev.start)
            } else {
                src_ev.offset
            };
            let _ = crate::takes::add_take(
                &mut mc_event,
                Take {
                    media_id: src_ev.media_id,
                    offset: take_off,
                    name: format!("Câm {} — {}", idx + 1, src_ev.name),
                },
                false,
            );
        }
        let mc_ev_id = mc_event.id;
        mc_track.events.push(mc_event);
        for (src_ti, _) in &sources {
            if let Some(t) = self.project.tracks.get_mut(*src_ti) {
                t.mute = true;
            }
        }
        self.project.tracks.insert(0, mc_track);
        self.sel_track = Some(0);
        self.sel_events.clear();
        self.sel_events.insert(mc_ev_id);
        self.opts.multicam_mode = true;
        self.changed();
        self.set_status(format!(
            "Trilha Multicâmera criada com {} câmeras.",
            sources.len()
        ));
        Ok(0)
    }

    /// Retorna `(track_idx, event_idx, event_id, active_take, take_count)` para o evento multicâmera sob `t`.
    pub fn active_multicam_event_at(&self, t: Ticks) -> Option<(usize, usize, Id, usize, usize)> {
        let mut best: Option<(usize, usize, Id, usize, usize)> = None;
        for (ti, tr) in self.project.tracks.iter().enumerate() {
            if tr.kind != TrackKind::Video || tr.is_folder || tr.mute {
                continue;
            }
            for (ei, ev) in tr.events.iter().enumerate() {
                let count = crate::takes::take_count(ev);
                if count > 1 && ev.contains(t) {
                    let item = (ti, ei, ev.id, crate::takes::active_index(ev), count);
                    if tr.multicam || self.sel_track == Some(ti) {
                        return Some(item);
                    }
                    if best.is_none() {
                        best = Some(item);
                    }
                }
            }
        }
        best
    }

    /// Alterna o ângulo multicâmera em `t` dividindo o evento no cursor se necessário e ativando `angle_idx`.
    pub fn switch_multicam_angle(&mut self, t: Ticks, angle_idx: usize) -> bool {
        let Some((ti, ei, ev_id, cur_take, count)) = self.active_multicam_event_at(t) else {
            return false;
        };
        if angle_idx >= count {
            return false;
        }
        let fd = self.project.settings.frame_dur();
        let (ev_start, ev_end) = {
            let e = &self.project.tracks[ti].events[ei];
            (e.start, e.end())
        };
        if t <= ev_start + fd && cur_take == angle_idx {
            return false;
        }
        self.checkpoint();
        let mut target_id = ev_id;
        if t > ev_start + fd && t < ev_end - fd {
            let mut gm = HashMap::new();
            if let Some(new_id_val) =
                split_event_in_track(&mut self.project.tracks[ti], ei, t, &mut gm)
            {
                target_id = new_id_val;
            }
        }
        if let Some(ev) = self.project.event_mut(target_id) {
            if crate::takes::set_active(ev, angle_idx) {
                ev.name = if ev.take_name.is_empty() {
                    format!("Câm {}", angle_idx + 1)
                } else {
                    ev.take_name.clone()
                };
            }
        }
        self.sel_track = Some(ti);
        self.sel_events.clear();
        self.sel_events.insert(target_id);
        self.changed();
        self.set_status(format!("Multicâmera: Câmera {}", angle_idx + 1));
        true
    }

    // -----------------------------------------------------------------
    // Efeitos, cadeias salvas, rastreamento, legendas por região e
    // áudio fora de tempo real (pp. 54–56, 514, 713, 722, 940, 946)
    // -----------------------------------------------------------------

    /// Cria uma instância de `FxInstance` preenchida com os valores salvos de um preset.
    pub fn instantiate_fx_with_preset(kind: FxKind, values: &[f32]) -> FxInstance {
        let mut fx = FxInstance::new(kind);
        let specs = kind.params();
        for (p, (spec, &val)) in fx.params.iter_mut().zip(specs.iter().zip(values.iter())) {
            if val.is_finite() {
                p.value = val.clamp(spec.min, spec.max);
            }
        }
        fx
    }

    /// Converte uma lista de efeitos de uma cadeia para formato serializável salvo em Preferências.
    pub fn capture_saved_fx_chain(
        name: &str,
        audio: bool,
        fxs: &[FxInstance],
    ) -> Option<crate::prefs::SavedFxChainPref> {
        let trimmed = name.trim();
        if trimmed.is_empty() || fxs.is_empty() {
            return None;
        }
        let mut items = Vec::new();
        for fx in fxs.iter().take(crate::prefs::MAX_FX_CHAIN_ITEMS) {
            if matches!(fx.kind, FxKind::Ofx | FxKind::Vst3) {
                continue;
            }
            items.push(crate::prefs::SavedFxChainItemPref {
                fx_kind: fx.kind.id().to_string(),
                enabled: fx.enabled,
                values: fx.params.iter().map(|p| p.value).collect(),
            });
        }
        let mut chain = crate::prefs::SavedFxChainPref {
            name: trimmed.to_string(),
            audio,
            items,
        };
        chain.sanitize().then_some(chain)
    }

    /// Constrói novas instâncias independentes a partir de uma cadeia salva.
    pub fn instantiate_saved_fx_chain(chain: &crate::prefs::SavedFxChainPref) -> Vec<FxInstance> {
        chain
            .items
            .iter()
            .filter_map(|it| {
                let kind = FxKind::from_id(&it.fx_kind)?;
                if kind.is_audio() != chain.audio {
                    return None;
                }
                let mut fx = Self::instantiate_fx_with_preset(kind, &it.values);
                fx.enabled = it.enabled;
                Some(fx)
            })
            .collect()
    }

    /// Converte todas as regiões do projeto em legendas de texto na timeline (p. 722).
    pub fn insert_subtitles_from_regions(&mut self) -> usize {
        if !self.project.regions.iter().any(|r| r.end > r.start) {
            self.set_status("Não há regiões válidas para gerar legendas.");
            return 0;
        }
        self.checkpoint();
        let count = crate::subtitles::regions_to_subtitles(&mut self.project, "Legendas");
        self.changed();
        self.set_status(format!(
            "{count} legenda(s) gerada(s) a partir das regiões."
        ));
        count
    }

    /// Rastreia o deslocamento planar quadro a quadro de `src_ev_id` e grava quadros-chave
    /// de movimento em um evento (`PanCrop`) ou trilha (`TrackMotion`, p. 713).
    pub fn track_event_motion(
        &mut self,
        media: &MediaCache,
        src_ev_id: Id,
        target_event_id: Option<Id>,
        target_track_id: Option<Id>,
        invert_motion: bool,
        step_frames: usize,
    ) -> Result<usize> {
        let Some((src_ti, src_ei)) = self.project.find_event(src_ev_id) else {
            return Err(NeovegError::InvalidInput(
                "evento de origem inválido para rastreamento".into(),
            ));
        };
        let src_ev = self.project.tracks[src_ti].events[src_ei].clone();
        let Some(src_media) = self.project.media(src_ev.media_id).cloned() else {
            return Err(NeovegError::InvalidInput(
                "mídia de origem indisponível".into(),
            ));
        };
        if !src_media.has_video {
            return Err(NeovegError::InvalidInput(
                "rastreamento de movimento requer evento com vídeo".into(),
            ));
        }

        let fd = self
            .project
            .settings
            .frame_dur()
            .max(Ticks::from_millis_i64(1));
        let total_frames = ((src_ev.length.0 / fd.0).max(1)) as usize;
        let auto_step = total_frames.div_ceil(180).max(1);
        let step = step_frames.max(auto_step).min(total_frames.max(1));

        let mut samples: Vec<(Ticks, f32, f32)> = Vec::new();
        let mut prev_img = src_ev
            .media_time(src_ev.start, src_media.duration)
            .and_then(|mt| media.video_frame_for_quality(&src_media, mt, 96, 54, true));
        let mut acc_dx = 0.0f32;
        let mut acc_dy = 0.0f32;
        let sign = if invert_motion { -1.0 } else { 1.0 };
        samples.push((Ticks::ZERO, 0.0, 0.0));

        let mut frame_i = step;
        while frame_i <= total_frames && samples.len() < 240 {
            let local_t = Ticks::from_raw((frame_i as i64).saturating_mul(fd.0))
                .min(Ticks::from_raw(src_ev.length.0.saturating_sub(1)).max(Ticks::ZERO));
            let abs_t = src_ev.start + local_t;
            if let Some(mt) = src_ev.media_time(abs_t, src_media.duration) {
                if let Some(cur_img) = media.video_frame_for_quality(&src_media, mt, 96, 54, true) {
                    if let Some(ref prev) = prev_img {
                        let (dx, dy, _) =
                            crate::engine::vfx::estimate_global_motion(prev, &cur_img);
                        acc_dx = (acc_dx + dx * sign).clamp(-0.45, 0.45);
                        acc_dy = (acc_dy + dy * sign).clamp(-0.45, 0.45);
                    }
                    samples.push((local_t, acc_dx, acc_dy));
                    prev_img = Some(cur_img);
                }
            }
            frame_i += step;
        }
        if samples.len() < 2 {
            samples.push((src_ev.length, acc_dx, acc_dy));
        }

        let tol = fd.div_i64(2);
        self.checkpoint();
        if let Some(tr_id) = target_track_id {
            let Some(ti) = self.project.track_index(tr_id) else {
                return Err(NeovegError::InvalidInput(
                    "trilha de destino inexistente".into(),
                ));
            };
            let base = self.project.tracks[ti].motion.eval(src_ev.start);
            self.project.tracks[ti].motion.keys.clear();
            for &(lt, dx, dy) in &samples {
                self.project.tracks[ti].motion.set(
                    src_ev.start + lt,
                    MotionKey {
                        x: (base.x + dx).clamp(-1.5, 1.5),
                        y: (base.y + dy).clamp(-1.5, 1.5),
                        ..base.clone()
                    },
                    tol,
                );
            }
        } else {
            let dst_id = target_event_id.unwrap_or(src_ev_id);
            let Some(dst_ev) = self.project.event_mut(dst_id) else {
                return Err(NeovegError::InvalidInput(
                    "evento de destino inexistente".into(),
                ));
            };
            let base = dst_ev.pan_crop.eval(Ticks::ZERO);
            dst_ev.pan_crop.keys.clear();
            for &(lt, dx, dy) in &samples {
                dst_ev.pan_crop.set(
                    lt,
                    PanCropKey {
                        cx: (base.cx + dx).clamp(-1.0, 2.0),
                        cy: (base.cy + dy).clamp(-1.0, 2.0),
                        ..base.clone()
                    },
                    tol,
                );
            }
        }
        let n_keys = samples.len();
        self.changed();
        self.set_status(format!(
            "Rastreamento de movimento aplicado ({n_keys} quadros-chave)."
        ));
        Ok(n_keys)
    }

    /// Aplica os efeitos de áudio de eventos selecionados fora de tempo real (WAV),
    /// adicionando o resultado como *take* ativo e desativando os plug-ins em tempo real (p. 940).
    pub fn apply_audio_fx_non_realtime(
        &mut self,
        media: &MediaCache,
        out_dir: &std::path::Path,
    ) -> Result<usize> {
        let mut targets: Vec<(usize, usize, Event)> = Vec::new();
        for (ti, tr) in self.project.tracks.iter().enumerate() {
            if tr.kind != TrackKind::Audio || tr.is_folder {
                continue;
            }
            for (ei, ev) in tr.events.iter().enumerate() {
                if self.sel_events.contains(&ev.id) && ev.fx.iter().any(|f| f.enabled) {
                    targets.push((ti, ei, ev.clone()));
                }
            }
        }
        if targets.is_empty() {
            if let Some(ti) = self.sel_track {
                if let Some(tr) = self
                    .project
                    .tracks
                    .get(ti)
                    .filter(|t| t.kind == TrackKind::Audio)
                {
                    if let Some((ei, ev)) =
                        tr.events.iter().enumerate().find(|(_, e)| {
                            e.contains(self.cursor) && e.fx.iter().any(|f| f.enabled)
                        })
                    {
                        targets.push((ti, ei, ev.clone()));
                    }
                }
            }
        }
        if targets.is_empty() {
            return Err(NeovegError::InvalidInput(
                "selecione evento(s) de áudio com efeitos ativos para aplicar fora de tempo real"
                    .into(),
            ));
        }

        std::fs::create_dir_all(out_dir)
            .map_err(|e| NeovegError::InvalidInput(format!("erro criando pasta de bounce: {e}")))?;
        let sr = self.project.settings.sample_rate.clamp(8000, 192_000);
        let mut rendered: Vec<(Id, crate::media::Imported)> = Vec::new();
        for (_, _, ev) in &targets {
            let Some(pcm) = crate::engine::audio::render_event_fx_stereo(&self.project, media, ev)
            else {
                return Err(NeovegError::InvalidInput(format!(
                    "áudio do evento \"{}\" indisponível",
                    ev.name
                )));
            };
            let wav_path = out_dir.join(format!("fx_aplicado_{}.wav", new_id()));
            let mut wr =
                crate::render::WavWriter::create(&wav_path, sr, crate::render::WavBits::Pcm24)
                    .map_err(|e| NeovegError::InvalidInput(format!("erro criando WAV: {e}")))?;
            wr.write(&pcm)
                .map_err(|e| NeovegError::InvalidInput(format!("erro gravando WAV: {e}")))?;
            wr.finish()
                .map_err(|e| NeovegError::InvalidInput(format!("erro finalizando WAV: {e}")))?;
            let imp = crate::media::import_file(&wav_path)
                .map_err(|e| NeovegError::InvalidInput(format!("erro reimportando WAV: {e}")))?;
            rendered.push((ev.id, imp));
        }

        self.checkpoint();
        let count = rendered.len();
        for (ev_id, imp) in rendered {
            let mid = imp.item.id;
            media.insert(mid, imp.entry);
            let reg_id = self.register_imported_media(imp.item, None);
            if let Some(ev) = self.project.event_mut(ev_id) {
                let take_name = format!("FX aplicado — {}", ev.name);
                let _ = crate::takes::add_take(
                    ev,
                    Take {
                        media_id: reg_id,
                        offset: Ticks::ZERO,
                        name: take_name.clone(),
                    },
                    true,
                );
                ev.rate = Rate::NORMAL;
                ev.pitch_semitones = 0.0;
                ev.take_name = take_name;
                for fx in &mut ev.fx {
                    fx.enabled = false;
                }
            }
        }
        self.changed();
        self.set_status(format!(
            "Efeitos de áudio aplicados fora de tempo real em {count} evento(s)."
        ));
        Ok(count)
    }
}

fn split_event_in_track(
    track: &mut Track,
    ei: usize,
    t: Ticks,
    group_map: &mut HashMap<Id, Id>,
) -> Option<Id> {
    let e = track.events.get_mut(ei)?;
    if t <= e.start || t >= e.end() || e.locked {
        return None;
    }
    let mut right = e.clone();
    let left_len = t - e.start;
    e.length = left_len;
    e.fade_out = Fade::default();
    right.id = new_id();
    right.start = t;
    right.length -= left_len;
    right.fade_in = Fade::default();
    right.transition = None;
    if !right.reverse {
        right.shift_offsets(right.rate.scale_ticks(left_len));
    }
    if e.reverse {
        e.shift_offsets(e.rate.scale_ticks(right.length));
    }
    for k in &mut right.pan_crop.keys {
        k.t -= left_len;
    }
    right.group = right
        .group
        .map(|g| *group_map.entry(g).or_insert_with(new_id));
    right.fx = right
        .fx
        .iter()
        .map(|f| {
            let mut f = f.duplicate();
            for p in &mut f.params {
                for k in &mut p.keys {
                    k.0 -= left_len;
                }
            }
            f
        })
        .collect();
    for env in &mut right.envelopes {
        for p in &mut env.points {
            p.t -= left_len;
        }
    }
    let right_id = right.id;
    track.events.push(right);
    Some(right_id)
}

fn would_create_sync_cycle(project: &Project, follower_id: Id, master_id: Id) -> bool {
    if follower_id == master_id {
        return true;
    }
    let mut visited = HashSet::new();
    let mut cur = master_id;
    for _ in 0..32 {
        if !visited.insert(cur) {
            return true;
        }
        let Some(parent) = project.event(cur).and_then(|e| e.sync_link_master) else {
            return false;
        };
        if parent == follower_id {
            return true;
        }
        cur = parent;
    }
    true
}

/// Destino de faixa para cada item em um plano de movimentação/cópia (§3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MoveTrackTarget {
    Existing(usize),
    CreateNew {
        key: u64,
        kind: TrackKind,
        name: String,
    },
}

/// Entrada individual de um plano de movimentação/cópia (§3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveEntry {
    pub id: Id,
    pub target_track: MoveTrackTarget,
    pub new_start: Ticks,
}

/// Constrói um plano de movimentação a partir de uma lista ordenada de seleção
/// (preservando a ordem da primeira ocorrência e mantendo cada identidade uma
/// única vez antes de expandir eventos agrupados — §3.1).
pub fn build_move_plan_from_selection(
    project: &Project,
    seed_ids: &[Id],
    expand_groups: bool,
    dt: Ticks,
    dtrack: isize,
) -> Vec<MoveEntry> {
    let mut ordered_ids: Vec<Id> = Vec::new();
    let mut seen: HashSet<Id> = HashSet::new();

    for &id in seed_ids {
        if project.find_event(id).is_some() && seen.insert(id) {
            ordered_ids.push(id);
        }
    }

    if expand_groups {
        let mut groups: Vec<Id> = Vec::new();
        let mut seen_groups: HashSet<Id> = HashSet::new();
        for &id in &ordered_ids {
            if let Some(g) = project.event(id).and_then(|e| e.group) {
                if seen_groups.insert(g) {
                    groups.push(g);
                }
            }
        }
        if !seen_groups.is_empty() {
            for t in &project.tracks {
                for e in &t.events {
                    if let Some(g) = e.group {
                        if seen_groups.contains(&g) && seen.insert(e.id) {
                            ordered_ids.push(e.id);
                        }
                    }
                }
            }
        }
    }

    let n = project.tracks.len() as isize;
    let valid_track_shift = dtrack != 0
        && ordered_ids.iter().all(|&id| {
            let Some((ti, _)) = project.find_event(id) else {
                return false;
            };
            let nt = ti as isize + dtrack;
            nt >= 0 && nt < n && project.tracks[nt as usize].kind == project.tracks[ti].kind
        });
    let effective_dtrack = if valid_track_shift { dtrack } else { 0 };

    ordered_ids
        .into_iter()
        .filter_map(|id| {
            let (ti, ei) = project.find_event(id)?;
            let ev = &project.tracks[ti].events[ei];
            let target_ti = (ti as isize + effective_dtrack) as usize;
            let new_start = (ev.start + dt).max(Ticks::ZERO);
            Some(MoveEntry {
                id,
                target_track: MoveTrackTarget::Existing(target_ti),
                new_start,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::peek_next_id;

    fn sample_media(id: Id, w: u32, h: u32, fps: Option<FrameRate>) -> MediaItem {
        MediaItem {
            id,
            name: format!("m_{id}"),
            source: MediaSource::Generated(GenSpec::SolidColor {
                color: [1.0, 0.0, 0.0, 1.0],
            }),
            duration: Some(Ticks::from_secs_i64(5)),
            has_video: true,
            has_audio: true,
            width: w,
            height: h,
            fps,
            info: String::new(),
            proxy_path: None,
        }
    }

    #[test]
    fn travar_selecao_alterna_em_um_passo_de_desfazer() {
        let mut p = Project::default();
        p.tracks.push(Track::new(TrackKind::Video, "Vídeo 1"));
        let m = sample_media(new_id(), 1920, 1080, Some(FrameRate::FPS_30));
        let a = Event::new(&m, Ticks::ZERO, Ticks::from_secs_i64(1));
        let mut b = Event::new(&m, Ticks::from_secs_i64(2), Ticks::from_secs_i64(1));
        b.locked = true;
        let (ida, idb) = (a.id, b.id);
        p.media.push(m);
        p.tracks[0].events = vec![a, b];
        let mut s = EditorSession::new(p);
        let locked = |s: &EditorSession, id| s.project.event(id).map(|e| e.locked);
        // sem seleção, nada muda
        assert_eq!(s.toggle_lock_selected(), 0);
        assert!(s.undo.is_empty());
        s.sel_events.insert(ida);
        s.sel_events.insert(idb);
        // um travado e outro não: trava os dois
        assert!(!s.selection_all_locked());
        assert_eq!(s.toggle_lock_selected(), 1);
        assert_eq!((locked(&s, ida), locked(&s, idb)), (Some(true), Some(true)));
        assert_eq!(s.undo.len(), 1);
        // todos travados: destrava
        assert!(s.selection_all_locked());
        assert_eq!(s.toggle_lock_selected(), 2);
        assert_eq!(
            (locked(&s, ida), locked(&s, idb)),
            (Some(false), Some(false))
        );
        assert_eq!(s.undo.len(), 2);
    }

    #[test]
    fn colar_pula_trilha_de_ajuste() {
        let mut p = Project::default();
        let mut adj = Track::new(TrackKind::Video, "Ajuste 1");
        adj.adjustment = true;
        p.tracks.push(adj);
        p.tracks.push(Track::new(TrackKind::Video, "Vídeo 1"));
        let m = sample_media(new_id(), 1920, 1080, Some(FrameRate::FPS_30));
        let ev = Event::new(&m, Ticks::ZERO, Ticks::from_secs_i64(1));
        let ev_id = ev.id;
        p.media.push(m);
        p.tracks[1].events.push(ev);
        let mut s = EditorSession::new(p);
        s.sel_events.insert(ev_id);
        s.copy_selection();
        // colar com a trilha de ajuste selecionada cai na próxima trilha de vídeo
        s.sel_track = Some(0);
        s.paste(false);
        assert!(s.project.tracks[0].events.is_empty());
        assert_eq!(s.project.tracks[1].events.len(), 2);
    }

    #[test]
    fn invariante_movimentacao_rejeita_ids_duplicados_atomicamente() {
        let mut p = Project::new_default();
        p.tracks.push(Track::new(TrackKind::Video, "Vídeo 2"));
        let m = sample_media(new_id(), 1920, 1080, Some(FrameRate::FPS_30));
        let ev1 = Event::new(&m, Ticks::from_secs_i64(1), Ticks::from_secs_i64(2));
        let ev2 = Event::new(&m, Ticks::from_secs_i64(4), Ticks::from_secs_i64(2));
        let id1 = ev1.id;
        let id2 = ev2.id;
        p.media.push(m);
        p.tracks[0].events.push(ev1);
        p.tracks[0].events.push(ev2);

        let mut sess = EditorSession::new(p);
        let gen_before = sess.generation;
        let undo_before = sess.undo.len();
        let redo_before = sess.redo.len();
        let next_id_before = peek_next_id();

        let bad_plan = vec![
            MoveEntry {
                id: id1,
                target_track: MoveTrackTarget::Existing(0),
                new_start: Ticks::from_secs_i64(3),
            },
            MoveEntry {
                id: id2,
                target_track: MoveTrackTarget::CreateNew {
                    key: 1,
                    kind: TrackKind::Video,
                    name: "Nova Vídeo".into(),
                },
                new_start: Ticks::from_secs_i64(5),
            },
            MoveEntry {
                id: id1,
                target_track: MoveTrackTarget::Existing(2),
                new_start: Ticks::from_secs_i64(7),
            },
        ];

        let err = sess.apply_move_plan(&bad_plan, false);
        assert!(err.is_err());
        // Nenhuma faixa criada, nenhum evento movido, nenhum novo ID nem histórico tocado.
        assert_eq!(sess.project.tracks.len(), 3);
        assert_eq!(sess.project.tracks[0].events.len(), 2);
        assert_eq!(
            sess.project.tracks[0].events[0].start,
            Ticks::from_secs_i64(1)
        );
        assert_eq!(sess.project.tracks[2].events.len(), 0);
        assert_eq!(sess.generation, gen_before);
        assert_eq!(sess.undo.len(), undo_before);
        assert_eq!(sess.redo.len(), redo_before);
        assert_eq!(peek_next_id(), next_id_before);
    }

    #[test]
    fn invariante_copia_recebe_novos_ids_e_cria_faixa_destino_junto() {
        let mut p = Project::new_default();
        let m = sample_media(new_id(), 1920, 1080, Some(FrameRate::FPS_30));
        let ev = Event::new(&m, Ticks::from_secs_i64(1), Ticks::from_secs_i64(2));
        let src_id = ev.id;
        p.media.push(m);
        p.tracks[0].events.push(ev);

        let mut sess = EditorSession::new(p);
        let plan = vec![
            MoveEntry {
                id: src_id,
                target_track: MoveTrackTarget::Existing(0),
                new_start: Ticks::from_secs_i64(6),
            },
            MoveEntry {
                id: src_id,
                target_track: MoveTrackTarget::CreateNew {
                    key: 99,
                    kind: TrackKind::Video,
                    name: "Vídeo Cópia".into(),
                },
                new_start: Ticks::from_secs_i64(8),
            },
        ];
        let copies = sess.apply_move_plan(&plan, true).unwrap();
        assert_eq!(copies.len(), 2);
        assert_ne!(copies[0], src_id);
        assert_ne!(copies[1], src_id);
        assert_ne!(copies[0], copies[1]);
        assert_eq!(sess.project.tracks.len(), 3);
        assert_eq!(sess.project.tracks[0].events.len(), 2);
        assert_eq!(sess.project.tracks[2].events.len(), 1);
        assert_eq!(sess.project.tracks[2].events[0].id, copies[1]);
        assert_eq!(
            sess.project.tracks[2].events[0].start,
            Ticks::from_secs_i64(8)
        );
        // Desfazer desfaz faixa criada + ambas as cópias atomicamente.
        assert!(sess.undo());
        assert_eq!(sess.project.tracks.len(), 2);
        assert_eq!(sess.project.tracks[0].events.len(), 1);
    }

    #[test]
    fn invariante_plano_selecao_deduplica_antes_de_expandir_grupos() {
        let mut p = Project::new_default();
        let m = sample_media(new_id(), 1280, 720, Some(FrameRate::FPS_24));
        let gid = new_id();
        let mut v_ev = Event::new(&m, Ticks::from_secs_i64(2), Ticks::from_secs_i64(3));
        v_ev.group = Some(gid);
        let mut a_ev = Event::new(&m, Ticks::from_secs_i64(2), Ticks::from_secs_i64(3));
        a_ev.group = Some(gid);
        let standalone = Event::new(&m, Ticks::from_secs_i64(7), Ticks::from_secs_i64(1));
        let (vid, aid, sid) = (v_ev.id, a_ev.id, standalone.id);
        p.media.push(m);
        p.tracks[0].events.push(v_ev);
        p.tracks[0].events.push(standalone);
        p.tracks[1].events.push(a_ev);

        // Seleção contém IDs repetidos antes de expandir o grupo.
        let plan = build_move_plan_from_selection(
            &p,
            &[sid, vid, sid, vid],
            true,
            Ticks::from_secs_i64(1),
            0,
        );
        let plan_ids: Vec<Id> = plan.iter().map(|e| e.id).collect();
        // Ordem da primeira ocorrência das sementes deduplicadas, seguida da expansão de grupo sem duplicar.
        assert_eq!(plan_ids, vec![sid, vid, aid]);
    }

    #[test]
    fn invariante_primeiro_arquivo_define_formato_apenas_se_nao_configurado() {
        // Caso 1: projeto padrão não configurado manualmente adota a 1ª mídia e ignora a 2ª.
        let mut sess = EditorSession::new(Project::new_default());
        assert!(!sess.project.settings.manually_configured);
        let m1 = sample_media(new_id(), 1280, 720, Some(FrameRate::FPS_24));
        sess.register_imported_media(m1, Some(44100));
        assert_eq!(sess.project.settings.width, 1280);
        assert_eq!(sess.project.settings.height, 720);
        assert_eq!(sess.project.settings.fps, FrameRate::FPS_24);
        assert_eq!(sess.project.settings.sample_rate, 44100);

        let m2 = sample_media(new_id(), 3840, 2160, Some(FrameRate::FPS_60));
        sess.register_imported_media(m2, Some(96000));
        assert_eq!(sess.project.settings.width, 1280);
        assert_eq!(sess.project.settings.fps, FrameRate::FPS_24);

        // Caso 2: projeto configurado manualmente nunca é sobrescrito nem pela 1ª mídia.
        let mut sess_manual = EditorSession::new(Project::new_default());
        let custom = ProjectSettings {
            width: 1920,
            height: 1080,
            fps: FrameRate::FPS_60,
            ..Default::default()
        };
        sess_manual.configure_project_settings(custom);
        assert!(sess_manual.project.settings.manually_configured);
        let m_first = sample_media(new_id(), 640, 360, Some(FrameRate::FPS_25));
        sess_manual.register_imported_media(m_first, Some(44100));
        assert_eq!(sess_manual.project.settings.width, 1920);
        assert_eq!(sess_manual.project.settings.height, 1080);
        assert_eq!(sess_manual.project.settings.fps, FrameRate::FPS_60);
        assert_eq!(sess_manual.project.settings.sample_rate, 48000);
    }

    #[test]
    fn trechos_prerenderizados_mesclam_e_invalidam_ao_editar() {
        let mut sess = EditorSession::new(Project::new_default());
        sess.record_prerendered_range(Ticks::from_secs_i64(5), Ticks::from_secs_i64(3));
        assert!(sess.prerendered_ranges.is_empty());

        sess.record_prerendered_range(Ticks::from_secs_i64(2), Ticks::from_secs_i64(6));
        sess.record_prerendered_range(Ticks::from_secs_i64(5), Ticks::from_secs_i64(9));
        sess.record_prerendered_range(Ticks::from_secs_i64(12), Ticks::from_secs_i64(14));
        assert_eq!(
            sess.prerendered_ranges,
            vec![
                (Ticks::from_secs_i64(2), Ticks::from_secs_i64(9)),
                (Ticks::from_secs_i64(12), Ticks::from_secs_i64(14)),
            ]
        );
        sess.changed();
        assert!(sess.prerendered_ranges.is_empty());
    }

    #[test]
    fn shuffle_reordena_eventos_na_trilha_sem_lacunas_e_desfaz() {
        let mut p = Project::new_default();
        let m = sample_media(new_id(), 1280, 720, Some(FrameRate::FPS_30));
        let mid = m.id;
        p.media.push(m.clone());
        let a = Event::new(&m, Ticks::ZERO, Ticks::from_secs_i64(2));
        let b = Event::new(&m, Ticks::from_secs_i64(2), Ticks::from_secs_i64(3));
        let mut c = Event::new(&m, Ticks::from_secs_i64(5), Ticks::from_secs_i64(4));
        let mut audio_c = Event::new(&m, Ticks::from_secs_i64(5), Ticks::from_secs_i64(4));
        let g = new_id();
        c.group = Some(g);
        audio_c.group = Some(g);
        let (aid, bid, cid, ac_id) = (a.id, b.id, c.id, audio_c.id);
        let _ = mid;
        p.tracks[0].events.extend([a, b, c]);
        p.tracks[1].events.push(audio_c);

        let mut sess = EditorSession::new(p);
        assert!(sess.shuffle_event(cid, Ticks::ZERO));
        assert_eq!(sess.project.event(cid).map(|e| e.start), Some(Ticks::ZERO));
        assert_eq!(
            sess.project.event(aid).map(|e| e.start),
            Some(Ticks::from_secs_i64(4))
        );
        assert_eq!(
            sess.project.event(bid).map(|e| e.start),
            Some(Ticks::from_secs_i64(6))
        );
        assert_eq!(
            sess.project.event(ac_id).map(|e| e.start),
            Some(Ticks::ZERO)
        );
        assert!(sess.undo());
        assert_eq!(
            sess.project.event(cid).map(|e| e.start),
            Some(Ticks::from_secs_i64(5))
        );
    }

    #[test]
    fn slide_desliza_evento_ajustando_vizinhos_adjacentes_e_respeita_limites() {
        let mut p = Project::new_default();
        let mut m = sample_media(new_id(), 1280, 720, Some(FrameRate::FPS_30));
        m.duration = Some(Ticks::from_secs_i64(20));
        p.media.push(m.clone());
        let a = Event::new(&m, Ticks::ZERO, Ticks::from_secs_i64(5));
        let mut b = Event::new(&m, Ticks::from_secs_i64(5), Ticks::from_secs_i64(4));
        b.offset = Ticks::from_secs_i64(1);
        let mut c = Event::new(&m, Ticks::from_secs_i64(9), Ticks::from_secs_i64(5));
        c.offset = Ticks::from_secs_i64(4);
        let (aid, bid, cid) = (a.id, b.id, c.id);
        p.tracks[0].events.extend([a, b, c]);

        let mut sess = EditorSession::new(p);
        assert!(sess.slide_event(bid, Ticks::from_secs_i64(2)));
        assert_eq!(
            sess.project.event(aid).map(|e| e.length),
            Some(Ticks::from_secs_i64(7))
        );
        let eb = sess.project.event(bid).unwrap();
        assert_eq!(eb.start, Ticks::from_secs_i64(7));
        assert_eq!(eb.length, Ticks::from_secs_i64(4));
        assert_eq!(eb.offset, Ticks::from_secs_i64(1));
        let ec = sess.project.event(cid).unwrap();
        assert_eq!(ec.start, Ticks::from_secs_i64(11));
        assert_eq!(ec.length, Ticks::from_secs_i64(3));
        assert_eq!(ec.offset, Ticks::from_secs_i64(6));

        // Entrada hostil gigante não encolhe vizinho abaixo de 1 quadro nem panica (§0.4).
        assert!(sess.slide_event(bid, Ticks::from_secs_i64(500)));
        assert!(sess.project.event(cid).unwrap().length >= sess.project.settings.frame_dur());
    }

    #[test]
    fn stretch_escala_rate_fades_e_keyframes_por_aritmetica_exata() {
        let mut p = Project::new_default();
        let m = sample_media(new_id(), 1280, 720, Some(FrameRate::FPS_30));
        p.media.push(m.clone());
        let mut ev = Event::new(&m, Ticks::from_secs_i64(2), Ticks::from_secs_i64(4));
        ev.fade_in.len = Ticks::from_secs_i64(1);
        ev.pan_crop.keys.push(Keyframe {
            t: Ticks::from_secs_i64(2),
            value: PanCropKey::default(),
            curve: Curve::Linear,
        });
        let id = ev.id;
        p.tracks[0].events.push(ev);

        let mut sess = EditorSession::new(p);
        assert!(sess.stretch_event_edge(id, false, Ticks::from_secs_i64(8)));
        let stretched = sess.project.event(id).unwrap();
        assert_eq!(stretched.start, Ticks::from_secs_i64(2));
        assert_eq!(stretched.length, Ticks::from_secs_i64(8));
        assert_eq!(stretched.rate, Rate::from_ratio(1, 2).unwrap());
        assert_eq!(stretched.fade_in.len, Ticks::from_secs_i64(2));
        assert_eq!(stretched.pan_crop.keys[1].t, Ticks::from_secs_i64(4));

        // Borda esquerda mantém o fim fixo.
        assert!(sess.stretch_event_edge(id, true, Ticks::from_secs_i64(4)));
        let restored = sess.project.event(id).unwrap();
        assert_eq!(restored.end(), Ticks::from_secs_i64(10));
        assert_eq!(restored.rate, Rate::NORMAL);
    }

    #[test]
    fn smart_split_e_heal_e_colar_atributos() {
        let mut p = Project::new_default();
        let mut m = sample_media(new_id(), 1280, 720, Some(FrameRate::FPS_30));
        m.duration = Some(Ticks::from_secs_i64(20));
        p.media.push(m.clone());
        let mut ev = Event::new(&m, Ticks::ZERO, Ticks::from_secs_i64(10));
        ev.fx.push(FxInstance::new(FxKind::GaussianBlur));
        ev.fade_in.len = Ticks::from_secs_i64(1);
        let tail = Event::new(&m, Ticks::from_secs_i64(10), Ticks::from_secs_i64(4));
        let mut other = Event::new(&m, Ticks::from_secs_i64(15), Ticks::from_secs_i64(3));
        let (ev_id, tail_id, other_id) = (ev.id, tail.id, other.id);
        other.reverse = false;
        p.tracks[0].events.extend([ev, tail, other]);

        let mut sess = EditorSession::new(p);
        sess.sel_events.insert(ev_id);
        sess.copy_selection();

        // Cola atributos no evento `other`.
        sess.sel_events.clear();
        sess.sel_events.insert(other_id);
        assert_eq!(sess.paste_attributes(PasteAttributesMask::ALL), 1);
        assert_eq!(sess.project.event(other_id).unwrap().fx.len(), 1);
        assert_eq!(
            sess.project.event(other_id).unwrap().fade_in.len,
            Ticks::from_secs_i64(1)
        );
        assert!(sess.undo());
        assert!(sess.project.event(other_id).unwrap().fx.is_empty());

        // Smart Split em [3s .. 5s] no primeiro evento + ripple no evento seguinte.
        sess.sel_events.clear();
        sess.time_sel = Some((Ticks::from_secs_i64(3), Ticks::from_secs_i64(5)));
        let n = sess.smart_split();
        assert!(n >= 1);
        assert_eq!(
            sess.project.event(tail_id).map(|e| e.start),
            Some(Ticks::from_secs_i64(8))
        );
        let tr0 = &sess.project.tracks[0];
        let right = tr0
            .events
            .iter()
            .find(|e| e.start == Ticks::from_secs_i64(3))
            .expect("metade direita criada");
        assert_eq!(
            right.transition.as_ref().map(|t| t.kind),
            Some(TransitionKind::WarpFlow)
        );
        assert!(sess.undo());

        // Dividir + Juntar eventos (Heal).
        sess.cursor = Ticks::from_secs_i64(4);
        sess.sel_events.clear();
        sess.sel_events.insert(ev_id);
        assert_eq!(sess.split_at_cursor(), 1);
        let right_split = *sess.sel_events.iter().next().unwrap();
        sess.sel_events.insert(ev_id);
        sess.sel_events.insert(right_split);
        assert!(sess.can_heal_selection());
        assert_eq!(sess.heal_events(), 1);
        assert_eq!(
            sess.project.event(ev_id).map(|e| e.length),
            Some(Ticks::from_secs_i64(10))
        );
    }

    #[test]
    fn split_trim_e_roll_cortam_e_deslizam_fronteira_com_desfazer() {
        let mut p = Project::new_default();
        let mut m = sample_media(new_id(), 1280, 720, Some(FrameRate::FPS_30));
        m.duration = Some(Ticks::from_secs_i64(20));
        p.media.push(m.clone());
        let e1 = Event::new(&m, Ticks::ZERO, Ticks::from_secs_i64(10));
        let id1 = e1.id;
        p.tracks[0].events.push(e1);

        let mut sess = EditorSession::new(p);
        assert!(sess.split_trim(id1, Ticks::from_secs_i64(2), Ticks::from_secs_i64(4)));
        assert_eq!(sess.project.tracks[0].events.len(), 2);
        let left = &sess.project.tracks[0].events[0];
        let right = &sess.project.tracks[0].events[1];
        assert_eq!(left.end(), Ticks::from_secs_i64(2));
        assert_eq!(right.start, Ticks::from_secs_i64(4));
        assert_eq!(right.length, Ticks::from_secs_i64(6));
        assert_eq!(right.offset, Ticks::from_secs_i64(4));

        // Roll entre eventos contíguos.
        assert!(sess.undo());
        let mut left_ev = Event::new(&m, Ticks::ZERO, Ticks::from_secs_i64(5));
        left_ev.offset = Ticks::ZERO;
        let mut right_ev = Event::new(&m, Ticks::from_secs_i64(5), Ticks::from_secs_i64(5));
        right_ev.offset = Ticks::from_secs_i64(5);
        let rid = right_ev.id;
        sess.project.tracks[0].events = vec![left_ev, right_ev];
        let snap = vec![
            TrackEventSnapshot {
                id: sess.project.tracks[0].events[0].id,
                track_idx: 0,
                start: Ticks::ZERO,
                length: Ticks::from_secs_i64(5),
                offset: Ticks::ZERO,
                rate: Rate::NORMAL,
                reverse: false,
                looped: true,
                media_dur: Some(Ticks::from_secs_i64(20)),
            },
            TrackEventSnapshot {
                id: rid,
                track_idx: 0,
                start: Ticks::from_secs_i64(5),
                length: Ticks::from_secs_i64(5),
                offset: Ticks::from_secs_i64(5),
                rate: Rate::NORMAL,
                reverse: false,
                looped: true,
                media_dur: Some(Ticks::from_secs_i64(20)),
            },
        ];
        let fd = sess.project.settings.frame_dur();
        assert!(apply_roll_from_snapshot(
            &mut sess.project,
            0,
            &snap,
            rid,
            true,
            Ticks::from_secs_i64(2),
            fd,
        ));
        assert_eq!(
            sess.project.tracks[0].events[0].length,
            Ticks::from_secs_i64(7)
        );
        assert_eq!(
            sess.project.tracks[0].events[1].start,
            Ticks::from_secs_i64(7)
        );
        assert_eq!(
            sess.project.tracks[0].events[1].length,
            Ticks::from_secs_i64(3)
        );
        assert_eq!(
            sess.project.tracks[0].events[1].offset,
            Ticks::from_secs_i64(7)
        );
    }

    #[test]
    fn pitch_shift_lock_e_colar_atributos_de_tom() {
        let mut p = Project::new_default();
        let mut ma = sample_media(new_id(), 0, 0, None);
        ma.has_video = false;
        ma.has_audio = true;
        ma.duration = Some(Ticks::from_secs_i64(10));
        p.media.push(ma.clone());

        let e1 = Event::new(&ma, Ticks::ZERO, Ticks::from_secs_i64(4));
        let mut e2 = Event::new(&ma, Ticks::from_secs_i64(5), Ticks::from_secs_i64(4));
        e2.rate = Rate::from_f64(2.0).unwrap();
        let id1 = e1.id;
        let id2 = e2.id;
        p.tracks[1].events = vec![e1, e2];

        let mut sess = EditorSession::new(p);
        sess.sel_events.insert(id1);
        assert_eq!(sess.shift_pitch_selected(5.0), 1);
        assert_eq!(sess.shift_pitch_selected(30.0), 1); // satura em +24 st
        assert_eq!(sess.project.event(id1).unwrap().pitch_semitones, 24.0);
        // NaN ignorado sem pânico
        assert_eq!(sess.shift_pitch_selected(f32::NAN), 0);

        sess.copy_selection();
        sess.sel_events.clear();
        sess.sel_events.insert(id2);
        let mask = PasteAttributesMask {
            fx: false,
            pan_crop: false,
            gain_db: false,
            rate: false,
            pitch: true,
            fades: false,
            switches: false,
            transition: false,
        };
        assert_eq!(sess.paste_attributes(mask), 1);
        let ev2 = sess.project.event(id2).unwrap();
        assert_eq!(ev2.pitch_semitones, 24.0);
        assert_eq!(sess.toggle_lock_pitch_to_stretch(), Some(true));
        let factor = sess.project.event(id2).unwrap().effective_pitch_factor();
        // lock=true preserva os +24 st solicitados (4.0x) independentemente de rate=2.0
        assert!((factor - 4.0).abs() < 1e-6);
        assert_eq!(sess.reset_pitch_selected(), 1);
        assert_eq!(sess.project.event(id2).unwrap().pitch_semitones, 0.0);
    }

    #[test]
    fn sync_links_transitivos_desvincular_e_rejeicao_de_ciclos_na_sanitizacao() {
        let mut p = Project::new_default();
        let m = sample_media(new_id(), 1280, 720, Some(FrameRate::FPS_30));
        p.media.push(m.clone());
        let e1 = Event::new(&m, Ticks::from_secs_i64(1), Ticks::from_secs_i64(2));
        let e2 = Event::new(&m, Ticks::from_secs_i64(4), Ticks::from_secs_i64(2));
        let e3 = Event::new(&m, Ticks::from_secs_i64(7), Ticks::from_secs_i64(2));
        let (id1, id2, id3) = (e1.id, e2.id, e3.id);
        p.tracks[0].events = vec![e1, e2, e3];

        let mut sess = EditorSession::new(p);
        sess.sel_events = [id1, id2].into_iter().collect();
        assert_eq!(sess.create_sync_links(Some(id1)), 1);
        // cria cadeia id1 -> id2 -> id3
        sess.sel_events = [id2, id3].into_iter().collect();
        assert_eq!(sess.create_sync_links(Some(id2)), 1);
        let followers = sess.sync_followers_for(&HashSet::from([id1]));
        assert_eq!(followers.len(), 2);
        assert!(sess.has_sync_followers(id1));

        // tentar vincular id1 seguindo id3 (que depende de id1) não cria ciclo
        sess.sel_events = [id3, id1].into_iter().collect();
        assert_eq!(sess.create_sync_links(Some(id3)), 0);

        // selecionar todos conectados
        sess.sel_events = HashSet::from([id3]);
        assert_eq!(sess.select_sync_linked(), 3);

        // projeto hostil com ciclo manual é limpo no sanitize
        sess.project.event_mut(id1).unwrap().sync_link_master = Some(id3);
        sess.project.sanitize();
        assert!(sess.project.event(id1).unwrap().sync_link_master.is_none());
        assert!(sess.project.event(id2).unwrap().sync_link_master.is_none());
        assert!(sess.project.event(id3).unwrap().sync_link_master.is_none());

        // recria vínculo simples id1 -> id2 e testa desvincular
        sess.sel_events = [id1, id2].into_iter().collect();
        assert_eq!(sess.create_sync_links(Some(id1)), 1);
        sess.sel_events = HashSet::from([id2]);
        assert_eq!(sess.unlink_sync_links(), 1);
        assert!(sess.project.event(id2).unwrap().sync_link_master.is_none());
    }

    #[test]
    fn inserir_evento_vazio_usa_time_sel_e_desfaz() {
        let p = Project::new_default();
        let mut sess = EditorSession::new(p.clone());
        sess.time_sel = Some((Ticks::from_secs_i64(3), Ticks::from_secs_i64(9)));
        let ev_id = sess.insert_empty_event();
        let ev = sess.project.event(ev_id).unwrap();
        assert_eq!(ev.start, Ticks::from_secs_i64(3));
        assert_eq!(ev.length, Ticks::from_secs_i64(6));
        assert_eq!(ev.name, "Evento vazio");
        // segundo evento vazio reutiliza o item de mídia vazio existente
        sess.time_sel = None;
        sess.cursor = Ticks::from_secs_i64(12);
        let ev2_id = sess.insert_empty_event();
        assert_eq!(sess.project.media.len(), 1);
        assert_eq!(
            sess.project.event(ev2_id).unwrap().length,
            Ticks::from_secs_i64(5)
        );
        assert!(sess.undo());
        assert!(sess.project.event(ev2_id).is_none());
    }

    #[test]
    fn deteccao_cenas_edicao_expandida_e_gravacao_com_desfazer() {
        let mut p = Project::new_default();
        let mut mv = sample_media(new_id(), 1280, 720, Some(FrameRate::FPS_30));
        mv.duration = Some(Ticks::from_secs_i64(20));
        let mut ma = sample_media(new_id(), 0, 0, None);
        ma.has_video = false;
        ma.has_audio = true;
        ma.duration = Some(Ticks::from_secs_i64(5));
        p.media.push(mv.clone());
        p.media.push(ma.clone());

        let ev = Event::new(&mv, Ticks::ZERO, Ticks::from_secs_i64(12));
        let ev_id = ev.id;
        p.tracks[0].events.push(ev);

        let mut sess = EditorSession::new(p);
        // 1. Detecção de cenas em modo split e marcadores
        let cuts = [Ticks::from_secs_i64(4), Ticks::from_secs_i64(8)];
        assert_eq!(sess.apply_scene_cuts(ev_id, &cuts, true), 2);
        assert_eq!(sess.project.markers.len(), 2);
        assert!(sess.undo());
        assert_eq!(sess.apply_scene_cuts(ev_id, &cuts, false), 2);
        assert_eq!(sess.project.tracks[0].events.len(), 3);
        assert_eq!(
            sess.project.tracks[0].events[0].end(),
            Ticks::from_secs_i64(4)
        );
        assert_eq!(
            sess.project.tracks[0].events[1].start,
            Ticks::from_secs_i64(4)
        );
        assert_eq!(
            sess.project.tracks[0].events[2].start,
            Ticks::from_secs_i64(8)
        );

        // 2. Modo de edição expandido + nudge A/B
        sess.cursor = Ticks::from_secs_i64(4);
        assert!(sess.toggle_expanded_edit());
        let fd = sess.project.settings.frame_dur();
        assert!(sess.nudge_expanded_edit(fd));
        assert_eq!(
            sess.project.tracks[0].events[0].end(),
            Ticks::from_secs_i64(4) + fd
        );
        assert_eq!(
            sess.project.tracks[0].events[1].start,
            Ticks::from_secs_i64(4) + fd
        );
        assert!(sess.set_expanded_focus(ExpandedEdgeFocus::LeftOut));
        assert!(sess.nudge_expanded_edit(fd));
        assert_eq!(
            sess.project.tracks[0].events[1].start,
            Ticks::from_secs_i64(4) + fd + fd
        );
        assert!(sess.undo());
        assert!(!sess.toggle_expanded_edit());

        // 3. Armar faixa de áudio e gravar clipe
        assert!(sess.toggle_record_arm(1));
        assert!(!sess.toggle_record_arm(0)); // vídeo não arma gravação de áudio
        let armed = sess.armed_audio_tracks();
        assert_eq!(armed.len(), 1);
        let created = sess.commit_recorded_clip(
            ma,
            &[armed[0].1],
            Ticks::from_secs_i64(2),
            Ticks::from_secs_i64(3),
        );
        assert_eq!(created.len(), 1);
        assert_eq!(sess.project.tracks[1].events.len(), 1);
        assert!(sess.undo());
        assert!(sess.project.tracks[1].events.is_empty());
    }

    #[test]
    fn midia_do_projeto_bins_smart_bins_substituicao_selecao_e_storyboard() {
        let mut p = Project::new_default();
        let mut mv1 = sample_media(10, 1920, 1080, Some(FrameRate::FPS_30));
        mv1.has_audio = false;
        mv1.duration = Some(Ticks::from_secs_i64(5));
        let mut ma = sample_media(11, 0, 0, None);
        ma.has_video = false;
        ma.has_audio = true;
        ma.duration = Some(Ticks::from_secs_i64(4));
        ma.name = "locucao.wav".into();
        let mut mstill = sample_media(12, 1280, 720, None);
        mstill.has_audio = false;
        mstill.duration = None;
        mstill.name = "foto.png".into();
        p.media = vec![mv1.clone(), ma.clone(), mstill.clone()];
        let ev1 = Event::new(&mv1, Ticks::ZERO, Ticks::from_secs_i64(3));
        let ev1_id = ev1.id;
        p.tracks[0].events.push(ev1);

        let mut sess = EditorSession::new(p);
        let root_bin = sess.create_media_bin("Cenas", None).unwrap();
        let sub_bin = sess.create_media_bin("Externa", Some(root_bin)).unwrap();
        assert!(sess.assign_media_to_bin(mv1.id, Some(sub_bin)));
        assert!(sess
            .project
            .collect_bin_media(root_bin, true)
            .contains(&mv1.id));
        assert!(!sess.project.validate_bin_parent(root_bin, Some(sub_bin)));

        let sb = sess
            .create_smart_bin("Sem uso", SmartBinRule::Unused, "")
            .unwrap();
        let used = sess.project.used_media_ids();
        assert!(!sess.project.smart_bins[0].rule.matches("", &mv1, &used));
        assert!(sess.project.smart_bins[0].rule.matches("", &ma, &used));
        assert!(sess.delete_smart_bin(sb));

        assert_eq!(sess.select_events_using_media(mv1.id), 1);
        assert!(sess.sel_events.contains(&ev1_id));

        let mut mv2 = sample_media(99, 3840, 2160, Some(FrameRate::FPS_60));
        mv2.name = "cena_4k.mp4".into();
        assert!(sess.replace_media_item(mv1.id, mv2));
        assert_eq!(sess.project.media(mv1.id).unwrap().width, 3840);
        assert!(sess.undo());
        assert_eq!(sess.project.media(mv1.id).unwrap().width, 1920);

        let story = sess.create_storyboard("Abertura").unwrap();
        assert!(sess.add_media_to_storyboard(story, mv1.id));
        assert!(sess.add_media_to_storyboard(story, mstill.id));
        assert!(sess.move_storyboard_item(story, 1, 0));
        let inserted = sess.insert_storyboard_at_cursor(story, Ticks::from_secs_i64(10), 2.0);
        assert_eq!(inserted.len(), 2);
        assert_eq!(sess.cursor, Ticks::from_secs_i64(17));
        assert!(sess.undo());
        assert_eq!(sess.project.tracks[0].events.len(), 1);
    }

    #[test]
    fn trilhas_pastas_pai_filho_padroes_multicam_e_render_nova_trilha() {
        let mut p = Project::new_default();
        let mut m1 = sample_media(101, 64, 36, Some(FrameRate::FPS_30));
        m1.source = MediaSource::Generated(GenSpec::SolidColor {
            color: [1.0, 0.0, 0.0, 1.0],
        });
        m1.has_audio = false;
        let mut m2 = sample_media(102, 64, 36, Some(FrameRate::FPS_30));
        m2.source = MediaSource::Generated(GenSpec::SolidColor {
            color: [0.0, 0.0, 1.0, 1.0],
        });
        m2.has_audio = false;
        p.media = vec![m1.clone(), m2.clone()];

        let ev1 = Event::new(&m1, Ticks::ZERO, Ticks::from_secs_i64(4));
        p.tracks[0].events.push(ev1);
        let mut v2 = Track::new(TrackKind::Video, "Vídeo 2");
        v2.events
            .push(Event::new(&m2, Ticks::ZERO, Ticks::from_secs_i64(4)));
        p.tracks.insert(1, v2);

        let mut sess = EditorSession::new(p);
        sess.sel_track = Some(0);

        // 1. Pasta de trilhas + recolher/expandir + validação anti-ciclo
        let fi = sess.insert_track_folder(TrackKind::Video);
        assert!(sess.project.tracks[fi].is_folder);
        let folder_id = sess.project.tracks[fi].id;
        let child_id = sess.project.tracks[fi + 1].id;
        assert_eq!(sess.project.tracks[fi + 1].parent_track_id, Some(folder_id));
        assert!(sess
            .project
            .validate_track_parent(folder_id, Some(child_id))
            .is_err());
        assert!(sess.toggle_track_folder(fi));
        assert!(sess.project.is_track_hidden_by_folder(child_id));
        assert!(sess.toggle_track_folder(fi));
        assert!(!sess.project.is_track_hidden_by_folder(child_id));

        // 2. Pai/Filho composição e promoção
        sess.sel_track = Some(2);
        assert!(sess.make_compositing_child(2).is_ok());
        assert!(sess.project.tracks[2].parent_track_id.is_some());
        assert!(sess.unmake_compositing_child(2));
        assert!(sess.project.tracks[2].parent_track_id.is_none());

        // 3. Propriedades padrão da trilha
        sess.project.tracks[1].height = 120.0;
        let defs = sess
            .capture_default_track_props(1, &crate::prefs::DefaultTrackPrefs::default())
            .unwrap();
        let nt = sess.insert_track_with_defaults(TrackKind::Video, &defs);
        assert!((sess.project.tracks[nt].height - 120.0).abs() < 0.01);

        // 4. Multicâmera: cria trilha multicâmera e corta para Câmera 2 aos 2 s
        sess.sel_events.clear();
        let mc_ti = sess.create_multicam_track().unwrap();
        assert!(sess.project.tracks[mc_ti].multicam);
        assert!(sess.opts.multicam_mode);
        let mid = Ticks::from_secs_i64(2);
        assert!(sess.switch_multicam_angle(mid, 1));
        let (_, _, _, active, count) = sess.active_multicam_event_at(mid).unwrap();
        assert_eq!(active, 1);
        assert!(count >= 2);

        // 5. Renderizar para nova trilha (AVI + Undo em 1 passo)
        let out_dir = std::env::temp_dir().join(format!("neoveg_rtn_{}", new_id()));
        let cache = MediaCache::default();
        let before_tracks = sess.project.tracks.len();
        let created = sess
            .render_to_new_track(
                &cache,
                Some((Ticks::ZERO, Ticks::from_secs_i64(1))),
                &out_dir,
            )
            .unwrap();
        assert!(!created.is_empty());
        assert_eq!(sess.project.tracks.len(), before_tracks + 1);
        assert!(sess.undo());
        assert_eq!(sess.project.tracks.len(), before_tracks);
        let _ = std::fs::remove_dir_all(&out_dir);
    }

    #[test]
    fn efeitos_cadeias_rastreamento_legendas_e_audio_nao_realtime() {
        // 1. Cadeia de efeitos salva e instanciada com segurança.
        let mut fx_sepia = FxInstance::new(FxKind::Sepia);
        fx_sepia.params[0].value = 0.75;
        let chain_pref = EditorSession::capture_saved_fx_chain(
            "Look Quente",
            false,
            &[fx_sepia, FxInstance::new(FxKind::Vignette)],
        )
        .expect("cadeia válida");
        let built = EditorSession::instantiate_saved_fx_chain(&chain_pref);
        assert_eq!(built.len(), 2);
        assert!((built[0].params[0].value - 0.75).abs() < 1e-4);
        assert!(EditorSession::capture_saved_fx_chain("", false, &built).is_none());

        // 2. Legendas a partir de regiões + Undo em passo único.
        let mut p = Project::new_default();
        p.regions.push(Region {
            id: 1,
            start: Ticks::ZERO,
            end: Ticks::from_secs_i64(2),
            label: "Título Inicial".into(),
        });
        let imp_vid = crate::media::import_generated(GenSpec::Text(TextSpec {
            text: "Alvo".into(),
            anim: TextAnim::CrawlLeft,
            anim_duration: 2.0,
            ..TextSpec::default()
        }));
        let ev_vid = Event::new(&imp_vid.item, Ticks::ZERO, Ticks::from_secs_i64(2));
        let ev_vid_id = ev_vid.id;
        p.media.push(imp_vid.item);
        p.tracks[0].events.push(ev_vid);

        let mut sess = EditorSession::new(p);
        let cache = MediaCache::default();
        cache.insert(p_media_id(&sess.project), imp_vid.entry);

        assert_eq!(sess.insert_subtitles_from_regions(), 1);
        assert!(sess
            .project
            .tracks
            .iter()
            .any(|t| t.name.starts_with("Legendas") && !t.events.is_empty()));
        assert!(sess.undo());

        // 3. Rastreamento planar de movimento para Pan/Crop + Undo.
        let keys_n = sess
            .track_event_motion(&cache, ev_vid_id, Some(ev_vid_id), None, true, 6)
            .expect("rastreamento");
        assert!(keys_n >= 2);
        assert!(sess.project.event(ev_vid_id).unwrap().pan_crop.keys.len() >= 2);
        assert!(sess.undo());

        // 4. Bypass global no Mixer e aplicar efeitos de áudio fora de tempo real.
        let dir = std::env::temp_dir().join(format!("neoveg_afxbounce_{}", new_id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let wav_path = dir.join("tone.wav");
        let sr = sess.project.settings.sample_rate;
        let samples = vec![0.25f32; (sr as usize / 2) * 2];
        let mut wr = crate::render::WavWriter::create(&wav_path, sr, crate::render::WavBits::Pcm16)
            .expect("wav create");
        wr.write(&samples).expect("wav write");
        wr.finish().expect("wav finish");
        let imp_aud = crate::media::import_file(&wav_path).expect("import wav");
        let mid_aud = imp_aud.item.id;
        let mut ev_aud = Event::new(&imp_aud.item, Ticks::ZERO, Ticks::from_millis_i64(500));
        let ev_aud_id = ev_aud.id;
        let mut gain = FxInstance::new(FxKind::Gain);
        gain.params[0].value = 6.0;
        ev_aud.fx.push(gain);
        cache.insert(mid_aud, imp_aud.entry);
        sess.project.media.push(imp_aud.item);
        sess.project.tracks[1].events.push(ev_aud);
        sess.sel_events.insert(ev_aud_id);

        // Verifica bypass global no Mixer.
        let audio_map = cache.audio_map();
        let mut mixer = crate::engine::audio::Mixer::new_offline(sr);
        let mut wet = vec![0.0f32; 512 * 2];
        let mut bypassed = vec![0.0f32; 512 * 2];
        mixer.bypass_fx = false;
        mixer.process(&sess.project, &audio_map, 0, &mut wet);
        mixer.reset();
        mixer.bypass_fx = true;
        mixer.process(&sess.project, &audio_map, 0, &mut bypassed);
        let max_wet = wet.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        let max_byp = bypassed.iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(max_wet > max_byp * 1.4, "wet={max_wet} bypassed={max_byp}");

        // Aplica fora de tempo real (adiciona take e desativa o FX em tempo real).
        let applied = sess
            .apply_audio_fx_non_realtime(&cache, &dir)
            .expect("bounce");
        assert_eq!(applied, 1);
        let ev_after = sess.project.event(ev_aud_id).unwrap();
        assert!(!ev_after.alt_takes.is_empty());
        assert!(!ev_after.fx[0].enabled);
        assert!(sess.undo());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn grade_marcadores_cd_comando_e_modos_de_automacao() {
        let mut p = Project::new_default();
        p.settings.bpm = 120.0;
        assert!(RulerGridSpacing::Beats
            .step_ticks(&p.settings)
            .is_some_and(|t| (t.as_secs_f64() - 0.5).abs() < 1e-4));

        // Entrada hostil no BPM não entra em pânico na grade musical.
        p.settings.bpm = f64::NAN;
        assert!(RulerGridSpacing::Beats.step_ticks(&p.settings).is_some());
        p.settings.bpm = -999.0;
        p.sanitize();
        assert!((p.settings.bpm - 120.0).abs() < 1e-6);

        let mut sess = EditorSession::new(p);
        assert_eq!(sess.cycle_ruler_grid(), RulerGridSpacing::Frame);
        sess.cursor = Ticks::from_secs_i64(2);
        sess.insert_marker_kind(MarkerKind::CdTrack, "Abertura", "", "");
        sess.cursor = Ticks::from_secs_i64(4);
        sess.insert_marker_kind(MarkerKind::CdIndex, "Refrão", "", "");
        sess.cursor = Ticks::from_secs_i64(5);
        sess.insert_marker_kind(MarkerKind::Command, "Legenda", "TEXT", "Olá");

        assert_eq!(sess.project.markers.len(), 3);
        let cue = sess.export_cd_cue_sheet("album.wav");
        assert!(cue.contains("FILE \"album.wav\" WAVE"));
        assert!(cue.contains("TRACK 01 AUDIO"));
        assert!(cue.contains("INDEX 01 00:02:00"));
        assert!(cue.contains("INDEX 02 00:04:00"));

        // Automação: Touch grava ponto visível; Off desativa leitura sem apagar pontos.
        assert_eq!(
            sess.cycle_track_automation_mode(0),
            Some(AutomationMode::Touch)
        );
        assert!(sess.record_automation_point(
            0,
            EnvKind::Opacity,
            Ticks::from_secs_i64(1),
            0.35,
            Ticks::from_millis_i64(10),
        ));
        assert!(
            (sess.project.tracks[0].env_value(EnvKind::Opacity, Ticks::from_secs_i64(1)) - 0.35)
                .abs()
                < 1e-3
        );
        sess.set_track_automation_mode(0, AutomationMode::Off);
        assert!(
            (sess.project.tracks[0].env_value(EnvKind::Opacity, Ticks::from_secs_i64(1)) - 1.0)
                .abs()
                < 1e-5
        );
    }

    fn p_media_id(p: &Project) -> Id {
        p.media[0].id
    }

    #[test]
    fn teste_item13_trocar_editar_desfazer_duas_vezes() {
        let mut s = EditorSession::new(Project::new_default());
        let seq1_id = s.project.active_sequence_id.unwrap();
        s.project.tracks[0].name = "Trilha Seq 1".into();

        // Cria seq 2
        let seq2_id = s.create_sequence(Some("Seq 2"));
        assert_eq!(s.project.active_sequence_id, Some(seq2_id));
        s.project.tracks[0].name = "Trilha Seq 2".into();

        // Alterna de volta para seq 1 (troca NÃO é passo de desfazer)
        assert!(s.switch_sequence(seq1_id));
        assert_eq!(s.project.tracks[0].name, "Trilha Seq 1");

        // Edita seq 1
        s.checkpoint();
        s.project.tracks[0].name = "Trilha Seq 1 Editada".into();
        s.changed();

        // Alterna para seq 2
        assert!(s.switch_sequence(seq2_id));
        assert_eq!(s.project.tracks[0].name, "Trilha Seq 2");

        // Edita seq 2
        s.checkpoint();
        s.project.tracks[0].name = "Trilha Seq 2 Editada".into();
        s.changed();

        // Desfaz a edição da seq 2
        assert!(s.undo());
        assert_eq!(s.project.tracks[0].name, "Trilha Seq 2");

        // Desfaz a edição da seq 1
        assert!(s.undo());
        // Alterna para seq 1 e confere que a edição foi desfeita
        assert!(s.switch_sequence(seq1_id));
        assert_eq!(s.project.tracks[0].name, "Trilha Seq 1");
    }

    #[test]
    fn teste_item13_midia_usada_so_em_sequencia_inativa_nao_e_removida() {
        let mut s = EditorSession::new(Project::new_default());
        let mut m1 = sample_media(new_id(), 64, 36, None);
        m1.name = "media_ativa.avi".into();
        let m1_id = m1.id;
        let mut m2 = sample_media(new_id(), 64, 36, None);
        m2.name = "media_inativa.avi".into();
        let m2_id = m2.id;
        let mut m3 = sample_media(new_id(), 64, 36, None);
        m3.name = "media_desperdicada.avi".into();
        let m3_id = m3.id;

        s.project.media.push(m1.clone());
        s.project.media.push(m2.clone());
        s.project.media.push(m3.clone());

        // Sequência ativa (seq 1) usa m1
        let ev1 = Event::new(&m1, Ticks::ZERO, Ticks::from_secs_i64(5));
        s.project.tracks[0].events.push(ev1);

        // Cria sequência 2 (que vira ativa) e nela coloca m2
        s.create_sequence(Some("Seq Inativa"));
        let ev2 = Event::new(&m2, Ticks::ZERO, Ticks::from_secs_i64(5));
        s.project.tracks[0].events.push(ev2);

        // Alterna de volta para seq 1: seq 2 agora é inativa e contém m2
        let seq1_id = s.project.sequences[0].id;
        assert!(s.switch_sequence(seq1_id));

        // Executa remoção de mídia não utilizada
        let removed = s.remove_unused_media(None);

        // Apenas m3 deve ser removida; m1 (na seq ativa) e m2 (na seq inativa) DEVEM ser preservadas!
        assert_eq!(removed, vec![m3_id]);
        assert!(s.project.media.iter().any(|m| m.id == m1_id));
        assert!(s.project.media.iter().any(|m| m.id == m2_id));
        assert!(!s.project.media.iter().any(|m| m.id == m3_id));
    }
}
