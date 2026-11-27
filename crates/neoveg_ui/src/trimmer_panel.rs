//! Janela acoplável Trimmer (`Alt+2`) com a disposição e os comandos do VEGAS Pro
//! (manual do VEGAS Pro 21, "Using the Trimmer", pp. 311–323; lista de atalhos do
//! VEGAS Pro, pp. 11–12; `docs_obsidian/ui_references/referencia_visual_janelas.md`):
//! histórico de mídias no alto, régua com zoom/marcadores/regiões/seleção, monitor
//! de vídeo com Hover Scrub, divisória ajustável, forma de onda com zoom horizontal
//! e vertical, barra de rolagem e zoom, menu de contexto completo, atalhos de teclado
//! quando em foco, barra de botões configurável e tempos editáveis embaixo.

use super::{rgba_to_color_image, App, DndPayload, DockTab};
use crate::icons::{self, Icon};
use crate::model::*;
use eframe::egui;
use egui::{
    pos2, vec2, Align2, Color32, FontId, Key, Pos2, Rect, Sense, Shape, Stroke, StrokeKind,
    TextureHandle,
};
use neoveg_common::time::{format_ticks, parse_time};
use neoveg_common::{tr, tr_args, Language};
use neoveg_engine::media::audio::WaveChannel;
use std::collections::HashMap;

/// Quantas mídias o histórico guarda.
pub(crate) const HISTORY_MAX: usize = 20;
const MAX_TRIMMER_MARKERS: usize = 256;
const MAX_TRIMMER_REGIONS: usize = 256;
const WAVE_ZOOM_MIN: f32 = 0.25;
const WAVE_ZOOM_MAX: f32 = 8.0;
const SPLIT_RATIO_MIN: f32 = 0.20;
const SPLIT_RATIO_MAX: f32 = 0.85;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrimmerStreamMode {
    #[default]
    Both,
    VideoOnly,
    AudioOnly,
}

impl TrimmerStreamMode {
    pub const ALL: [TrimmerStreamMode; 3] = [
        TrimmerStreamMode::Both,
        TrimmerStreamMode::VideoOnly,
        TrimmerStreamMode::AudioOnly,
    ];

    pub fn label_in(self, lang: Language) -> &'static str {
        let key = match self {
            TrimmerStreamMode::Both => "Áudio + Vídeo",
            TrimmerStreamMode::VideoOnly => "Apenas Vídeo",
            TrimmerStreamMode::AudioOnly => "Apenas Áudio",
        };
        tr(lang, key)
    }

    pub fn label(self) -> &'static str {
        self.label_in(Language::PtBr)
    }

    /// Alterna o fluxo (`Tab` avança, `Shift+Tab` volta; VEGAS Pro p. 316).
    pub fn cycle(self, reverse: bool) -> Self {
        match (self, reverse) {
            (Self::Both, false) => Self::VideoOnly,
            (Self::VideoOnly, false) => Self::AudioOnly,
            (Self::AudioOnly, false) => Self::Both,
            (Self::Both, true) => Self::AudioOnly,
            (Self::AudioOnly, true) => Self::VideoOnly,
            (Self::VideoOnly, true) => Self::Both,
        }
    }
}

/// Modo de exibição dos canais de áudio na forma de onda do Trimmer (VEGAS Pro p. 312/315).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrimmerAudioView {
    #[default]
    Both,
    LeftOnly,
    RightOnly,
    Combine,
}

impl TrimmerAudioView {
    pub const ALL: [Self; 4] = [Self::Both, Self::LeftOnly, Self::RightOnly, Self::Combine];

    pub fn label_in(self, lang: Language) -> &'static str {
        let key = match self {
            Self::Both => "Ambos os canais (Estéreo)",
            Self::LeftOnly => "Somente canal esquerdo",
            Self::RightOnly => "Somente canal direito",
            Self::Combine => "Combinar (Mono)",
        };
        tr(lang, key)
    }
}

/// Ação ativa durante o arrasto na barra de rolagem horizontal do Trimmer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollbarDragMode {
    Pan { offset_px: f32 },
    LeftEdge,
    RightEdge,
}

/// Modo de arrasto sobre régua, monitor de vídeo ou forma de onda do Trimmer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrimmerDragMode {
    PendingInSelection { origin: Pos2, anchor_t: Ticks },
    Select { anchor_t: Ticks },
    EdgeIn,
    EdgeOut,
    DragToTimeline,
}

/// Campo de tempo sendo editado por duplo clique na barra inferior do Trimmer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrimmerTimeField {
    Cursor,
    SelIn,
    SelOut,
}

/// Rascunho do diálogo *Criar subclip* (manual do VEGAS Pro 21, pp. 322–323).
#[derive(Clone, Debug)]
pub struct SubclipDraft {
    pub media_id: Id,
    pub name: String,
    pub reverse: bool,
}

/// Botões da barra do Trimmer, na ordem do VEGAS Pro. O conjunto visível é
/// escolhido em *Editar conjunto de botões visíveis* (botão "…").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrimBtn {
    Loop,
    PlayFromStart,
    Play,
    Pause,
    Stop,
    GoStart,
    GoEnd,
    PrevFrame,
    NextFrame,
    OverwriteTimeline,
    AddFromCursor,
    AddUpToCursor,
    FitToFill,
    SetIn,
    SetOut,
    InsertMarker,
    InsertRegion,
}

impl TrimBtn {
    pub(crate) const ALL: [TrimBtn; 17] = [
        TrimBtn::Loop,
        TrimBtn::PlayFromStart,
        TrimBtn::Play,
        TrimBtn::Pause,
        TrimBtn::Stop,
        TrimBtn::GoStart,
        TrimBtn::GoEnd,
        TrimBtn::PrevFrame,
        TrimBtn::NextFrame,
        TrimBtn::OverwriteTimeline,
        TrimBtn::AddFromCursor,
        TrimBtn::AddUpToCursor,
        TrimBtn::FitToFill,
        TrimBtn::SetIn,
        TrimBtn::SetOut,
        TrimBtn::InsertMarker,
        TrimBtn::InsertRegion,
    ];

    /// Identificador gravado nas preferências.
    pub(crate) fn id(self) -> &'static str {
        match self {
            TrimBtn::Loop => "loop",
            TrimBtn::PlayFromStart => "play_from_start",
            TrimBtn::Play => "play",
            TrimBtn::Pause => "pause",
            TrimBtn::Stop => "stop",
            TrimBtn::GoStart => "go_start",
            TrimBtn::GoEnd => "go_end",
            TrimBtn::PrevFrame => "prev_frame",
            TrimBtn::NextFrame => "next_frame",
            TrimBtn::OverwriteTimeline => "overwrite_timeline",
            TrimBtn::AddFromCursor => "add_from_cursor",
            TrimBtn::AddUpToCursor => "add_up_to_cursor",
            TrimBtn::FitToFill => "fit_to_fill",
            TrimBtn::SetIn => "set_in",
            TrimBtn::SetOut => "set_out",
            TrimBtn::InsertMarker => "insert_marker",
            TrimBtn::InsertRegion => "insert_region",
        }
    }

    fn name(self) -> &'static str {
        match self {
            TrimBtn::Loop => "Reprodução em loop",
            TrimBtn::PlayFromStart => "Reproduzir do início",
            TrimBtn::Play => "Reproduzir",
            TrimBtn::Pause => "Pausar",
            TrimBtn::Stop => "Parar",
            TrimBtn::GoStart => "Ir para o início",
            TrimBtn::GoEnd => "Ir para o fim",
            TrimBtn::PrevFrame => "Quadro anterior",
            TrimBtn::NextFrame => "Próximo quadro",
            TrimBtn::OverwriteTimeline => "Habilitar sobrescrita na timeline",
            TrimBtn::AddFromCursor => "Adicionar à timeline a partir do cursor",
            TrimBtn::AddUpToCursor => "Adicionar à timeline até o cursor",
            TrimBtn::FitToFill => "Ajustar para preencher",
            TrimBtn::SetIn => "Definir ponto de entrada",
            TrimBtn::SetOut => "Definir ponto de saída",
            TrimBtn::InsertMarker => "Inserir marcador no Trimmer",
            TrimBtn::InsertRegion => "Inserir região no Trimmer",
        }
    }

    fn icon(self) -> Icon {
        match self {
            TrimBtn::Loop => Icon::LoopPlayback,
            TrimBtn::PlayFromStart => Icon::PlayFromStart,
            TrimBtn::Play => Icon::Play,
            TrimBtn::Pause => Icon::Pause,
            TrimBtn::Stop => Icon::Stop,
            TrimBtn::GoStart => Icon::GoStart,
            TrimBtn::GoEnd => Icon::GoEnd,
            TrimBtn::PrevFrame => Icon::PrevFrame,
            TrimBtn::NextFrame => Icon::NextFrame,
            TrimBtn::OverwriteTimeline => Icon::ReplaceMedia,
            TrimBtn::AddFromCursor => Icon::AddFromCursor,
            TrimBtn::AddUpToCursor => Icon::AddUpToCursor,
            TrimBtn::FitToFill => Icon::StretchTool,
            TrimBtn::SetIn => Icon::MarkIn,
            TrimBtn::SetOut => Icon::MarkOut,
            TrimBtn::InsertMarker => Icon::InsertMarker,
            TrimBtn::InsertRegion => Icon::InsertRegion,
        }
    }

    /// Começa um grupo de botões próprios do Trimmer (separador antes).
    fn starts_group(self) -> bool {
        matches!(self, TrimBtn::OverwriteTimeline | TrimBtn::SetIn)
    }
}

pub struct TrimmerState {
    pub media_id: Option<Id>,
    pub cursor: Ticks,
    pub sel_in: Ticks,
    pub sel_out: Ticks,
    pub stream_mode: TrimmerStreamMode,
    pub advance_timeline_cursor: bool,
    pub tex: Option<TextureHandle>,
    pub last_frame: Option<(Id, Ticks)>,
    /// Mídias abertas no Trimmer, a mais recente primeiro.
    pub history: Vec<Id>,
    /// Lista do histórico em ordem alfabética em vez de recente.
    pub sort_alpha: bool,
    /// Reprodução ativa no Trimmer.
    pub playing: bool,
    pub loop_play: bool,
    /// Velocidade do shuttle J/K/L (`0.0` parado, `-8.0..=8.0`).
    pub shuttle: f64,
    /// Onde a reprodução começou (Parar volta para cá).
    pub play_origin: Ticks,
    /// Âncora do arrasto que cria a seleção na régua, vídeo ou forma de onda.
    pub drag_anchor: Option<Ticks>,
    /// Rascunho de *Editar conjunto de botões visíveis*: botões ocultos.
    pub buttons_draft: Option<Vec<String>>,
    /// Indica se o Trimmer tem o foco de teclado (clicado ou ativado via `Alt+2`).
    pub has_focus: bool,
    /// Indica se o Trimmer foi desenhado neste quadro (acoplado ou destacado).
    pub visible_this_frame: bool,
    /// Início da janela visível (zoom horizontal).
    pub view_start: Ticks,
    /// Duração da janela visível (`Ticks::ZERO` ou `>= dur` = mídia inteira).
    pub view_span: Ticks,
    /// Posição/duração interpoladas para animação suave de zoom (igual à timeline).
    pub anim_view_start: Option<f64>,
    pub anim_view_span: Option<f64>,
    /// Zoom vertical da forma de onda de áudio (`0.25..=8.0`).
    pub wave_zoom: f32,
    /// Exibição de canais na forma de onda (Estéreo L/R, L, R ou Mono).
    pub audio_view: TrimmerAudioView,
    /// Proporção de altura reservada ao monitor de vídeo quando vídeo e áudio coexistem.
    pub video_split_ratio: f32,
    /// Exibir monitor de vídeo embutido no Trimmer (VEGAS Pro p. 314).
    pub show_video_monitor: bool,
    /// Enviar vídeo do Trimmer para a janela Prévia de Vídeo (`Ctrl+P`, VEGAS Pro p. 314).
    pub show_in_preview: bool,
    /// Ativar *Hover Scrub* no monitor de vídeo (VEGAS Pro p. 316).
    pub hover_scrub: bool,
    /// Tempo sob o ponteiro durante *Hover Scrub* (não altera `cursor` até arrastar).
    pub hover_time: Option<Ticks>,
    /// Modo de sobrescrita na timeline ao adicionar pelo Trimmer (VEGAS Pro p. 318).
    pub overwrite_timeline: bool,
    /// Marcadores de mídia criados no Trimmer (por `media_id`).
    pub media_markers: HashMap<Id, Vec<Marker>>,
    /// Regiões de mídia criadas no Trimmer (por `media_id`).
    pub media_regions: HashMap<Id, Vec<Region>>,
    /// Diálogo ativo de *Criar subclip*.
    pub subclip_dlg: Option<SubclipDraft>,
    /// Edição inline de timecode na barra inferior (duplo clique, VEGAS Pro p. 316).
    pub time_edit: Option<(TrimmerTimeField, String)>,
    /// Modo ativo de arrasto na barra de rolagem horizontal (pan ou zoom pelas bordas).
    pub scrollbar_drag: Option<ScrollbarDragMode>,
    /// Modo ativo de arrasto sobre régua/vídeo/forma de onda (seleção, bordas In/Out ou arrasto vertical à timeline).
    pub drag_mode: Option<TrimmerDragMode>,
    /// Temporizador para repetição contínua ao segurar Quadro anterior / Próximo quadro (VEGAS Pro p. 315).
    pub step_hold_accum: f32,
}

impl Default for TrimmerState {
    fn default() -> Self {
        Self {
            media_id: None,
            cursor: Ticks::ZERO,
            sel_in: Ticks::ZERO,
            sel_out: Ticks::ZERO,
            stream_mode: TrimmerStreamMode::Both,
            advance_timeline_cursor: false,
            tex: None,
            last_frame: None,
            history: Vec::new(),
            sort_alpha: false,
            playing: false,
            loop_play: false,
            shuttle: 0.0,
            play_origin: Ticks::ZERO,
            drag_anchor: None,
            buttons_draft: None,
            has_focus: false,
            visible_this_frame: false,
            view_start: Ticks::ZERO,
            view_span: Ticks::ZERO,
            anim_view_start: None,
            anim_view_span: None,
            wave_zoom: 1.0,
            audio_view: TrimmerAudioView::Both,
            video_split_ratio: 0.62,
            show_video_monitor: true,
            show_in_preview: false,
            hover_scrub: true,
            hover_time: None,
            overwrite_timeline: false,
            media_markers: HashMap::new(),
            media_regions: HashMap::new(),
            subclip_dlg: None,
            time_edit: None,
            scrollbar_drag: None,
            drag_mode: None,
            step_hold_accum: 0.0,
        }
    }
}

/// Põe `id` no topo do histórico, sem repetir e com limite.
pub(crate) fn push_history(history: &mut Vec<Id>, id: Id) {
    history.retain(|h| *h != id);
    history.insert(0, id);
    history.truncate(HISTORY_MAX);
}

/// Calcula o próximo valor de velocidade do shuttle `J` (`dir = -1`) ou `L` (`dir = 1`).
pub(crate) fn next_shuttle_speed(current: f64, dir: i32) -> f64 {
    let cur = if current.is_finite() { current } else { 0.0 };
    if dir > 0 {
        if cur < -1.5 {
            (cur * 0.5).min(-1.0)
        } else if cur < 0.0 {
            0.0
        } else if cur < 0.5 {
            1.0
        } else {
            (cur * 2.0).min(8.0)
        }
    } else if dir < 0 {
        if cur > 1.5 {
            (cur * 0.5).max(1.0)
        } else if cur > 0.0 {
            0.0
        } else if cur > -0.5 {
            -1.0
        } else {
            (cur * 2.0).max(-8.0)
        }
    } else {
        0.0
    }
}

/// Avança a reprodução em `dt` (compatível com velocidade positiva ou reversa).
pub(crate) fn play_step_with_speed(
    cursor: Ticks,
    dt_secs: f64,
    speed: f64,
    sel: (Ticks, Ticks),
    looping: bool,
    duration: Ticks,
) -> (Ticks, bool) {
    let dur = duration.max(Ticks::ZERO);
    let spd = if speed.is_finite() && speed.abs() >= 0.05 {
        speed.clamp(-8.0, 8.0)
    } else {
        1.0
    };
    let dt_s = if dt_secs.is_finite() && dt_secs > 0.0 {
        dt_secs.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let delta_raw = (dt_s * spd * neoveg_common::time::TICKS_PER_SEC as f64).round() as i64;
    let next = Ticks::from_raw(cursor.0.saturating_add(delta_raw));
    let (a, b) = (sel.0.min(sel.1), sel.0.max(sel.1));
    if looping && b > a {
        if spd >= 0.0 {
            if next >= b || next < a {
                return (a, true);
            }
        } else if next <= a || next > b {
            return (b, true);
        }
        return (next, true);
    }
    if spd >= 0.0 {
        if next >= dur {
            (dur, false)
        } else {
            (next.max(Ticks::ZERO), true)
        }
    } else if next <= Ticks::ZERO {
        (Ticks::ZERO, false)
    } else {
        (next.min(dur), true)
    }
}

/// Avança a reprodução em `dt`. Com loop, volta ao início da seleção ao
/// passar do fim dela; sem loop, para no fim da mídia. Devolve o novo
/// cursor e se a reprodução continua.
#[cfg(test)]
pub(crate) fn play_step(
    cursor: Ticks,
    dt: Ticks,
    sel: (Ticks, Ticks),
    looping: bool,
    duration: Ticks,
) -> (Ticks, bool) {
    let next = cursor + dt.max(Ticks::ZERO);
    let (a, b) = (sel.0.min(sel.1), sel.0.max(sel.1));
    if looping && b > a {
        if next >= b || next < a {
            return (a, true);
        }
        return (next, true);
    }
    if next >= duration {
        (duration, false)
    } else {
        (next, true)
    }
}

impl TrimmerState {
    pub fn load_media(&mut self, id: Id, duration: Ticks) {
        let len = duration.max(Ticks::from_millis_i64(40));
        self.media_id = Some(id);
        self.cursor = Ticks::ZERO;
        self.sel_in = Ticks::ZERO;
        self.sel_out = len;
        self.last_frame = None;
        self.playing = false;
        self.shuttle = 0.0;
        self.drag_anchor = None;
        self.drag_mode = None;
        self.scrollbar_drag = None;
        self.hover_time = None;
        self.view_start = Ticks::ZERO;
        self.view_span = len;
        self.anim_view_start = None;
        self.anim_view_span = None;
        self.time_edit = None;
        push_history(&mut self.history, id);
    }

    pub fn clamp_bounds(&mut self, duration: Ticks, min_len: Ticks) {
        let dur = duration.max(min_len.max(Ticks::from_raw(1)));
        self.cursor = self.cursor.clamp_safe(Ticks::ZERO, dur);
        self.sel_in = self.sel_in.clamp_safe(Ticks::ZERO, dur - min_len.min(dur));
        self.sel_out = self
            .sel_out
            .clamp_safe((self.sel_in + min_len).min(dur), dur);
        self.clamp_view(dur, min_len);
        self.wave_zoom = if self.wave_zoom.is_finite() {
            self.wave_zoom.clamp(WAVE_ZOOM_MIN, WAVE_ZOOM_MAX)
        } else {
            1.0
        };
        self.video_split_ratio = if self.video_split_ratio.is_finite() {
            self.video_split_ratio
                .clamp(SPLIT_RATIO_MIN, SPLIT_RATIO_MAX)
        } else {
            0.62
        };
    }

    pub fn min_view_span(dur: Ticks, frame_dur: Ticks) -> Ticks {
        let floor = Ticks::from_raw(
            frame_dur
                .0
                .max(1)
                .saturating_mul(4)
                .max(Ticks::from_millis_i64(40).0),
        );
        floor.min(dur.max(Ticks::from_raw(1)))
    }

    pub fn effective_view(&self, dur: Ticks) -> (Ticks, Ticks) {
        let d = dur.max(Ticks::from_raw(1));
        if self.view_span <= Ticks::ZERO || self.view_span >= d {
            return (Ticks::ZERO, d);
        }
        let span = self.view_span.clamp_safe(Ticks::from_raw(1), d);
        let max_start = Ticks::from_raw(d.0.saturating_sub(span.0).max(0));
        let start = self.view_start.clamp_safe(Ticks::ZERO, max_start);
        (start, span)
    }

    pub fn clamp_view(&mut self, dur: Ticks, frame_dur: Ticks) {
        let d = dur.max(Ticks::from_raw(1));
        if self.view_span <= Ticks::ZERO || self.view_span >= d {
            self.view_start = Ticks::ZERO;
            self.view_span = d;
            return;
        }
        let min_span = Self::min_view_span(d, frame_dur);
        self.view_span = self.view_span.clamp_safe(min_span, d);
        let max_start = Ticks::from_raw(d.0.saturating_sub(self.view_span.0).max(0));
        self.view_start = self.view_start.clamp_safe(Ticks::ZERO, max_start);
    }

    /// Fator de zoom horizontal atual (`1.0` = mídia inteira visível).
    pub fn zoom_factor(&self, dur: Ticks) -> f64 {
        let d = dur.0.max(1) as f64;
        let (_, span) = self.effective_view(dur);
        (d / span.0.max(1) as f64).clamp(1.0, 10000.0)
    }

    /// Aplica zoom horizontal mantendo o instante `anchor` na mesma posição horizontal.
    /// `factor > 1.0` aproxima; `factor < 1.0` afasta.
    pub fn zoom_around(&mut self, anchor: Ticks, factor: f64, dur: Ticks, frame_dur: Ticks) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let d = dur.max(Ticks::from_raw(1));
        let (cur_start, cur_span) = self.effective_view(d);
        let anchor_clamped = anchor.clamp_safe(Ticks::ZERO, d);
        let rel = if anchor_clamped >= cur_start && anchor_clamped <= cur_start + cur_span {
            (anchor_clamped.0.saturating_sub(cur_start.0) as f64 / cur_span.0.max(1) as f64)
                .clamp(0.0, 1.0)
        } else {
            0.5
        };
        let min_span = Self::min_view_span(d, frame_dur);
        let new_span_raw = (cur_span.0 as f64 / factor.clamp(0.05, 20.0)).round() as i64;
        let new_span = Ticks::from_raw(new_span_raw.clamp(min_span.0, d.0));
        let new_start_raw = anchor_clamped
            .0
            .saturating_sub((new_span.0 as f64 * rel).round() as i64);
        let max_start = d.0.saturating_sub(new_span.0).max(0);
        self.view_span = new_span;
        self.view_start = Ticks::from_raw(new_start_raw.clamp(0, max_start));
    }

    /// Zoom horizontal centrado na agulha de reprodução do Trimmer, exatamente
    /// como `TimelineState::zoom_around_playhead` na linha do tempo: se a agulha
    /// estiver visível, ela permanece no mesmo pixel; se estiver fora da tela,
    /// é centralizada.
    pub fn zoom_around_playhead(&mut self, factor: f64, dur: Ticks, frame_dur: Ticks) {
        self.zoom_around(self.cursor, factor, dur, frame_dur);
    }

    /// Avança a animação suave de zoom/deslocamento da janela visível em direção
    /// ao alvo (`view_start`, `view_span`). Devolve `true` enquanto ainda está animando.
    pub fn step_zoom_animation(&mut self, dur: Ticks, dt: f32) -> bool {
        let (target_s, target_sp) = self.effective_view(dur);
        let ts = target_s.0 as f64;
        let tsp = target_sp.0.max(1) as f64;
        let (Some(cur_s), Some(cur_sp)) = (self.anim_view_start, self.anim_view_span) else {
            self.anim_view_start = Some(ts);
            self.anim_view_span = Some(tsp);
            return false;
        };
        if !cur_s.is_finite() || !cur_sp.is_finite() || cur_sp <= 0.0 {
            self.anim_view_start = Some(ts);
            self.anim_view_span = Some(tsp);
            return false;
        }
        let eps = (tsp * 0.0015).max(1.0);
        if (cur_s - ts).abs() <= eps && (cur_sp - tsp).abs() <= eps {
            self.anim_view_start = Some(ts);
            self.anim_view_span = Some(tsp);
            return false;
        }
        let dt_c = if dt.is_finite() {
            f64::from(dt.clamp(0.001, 0.1))
        } else {
            0.016
        };
        let alpha = (1.0 - (-dt_c * 24.0).exp()).clamp(0.08, 1.0);
        let next_s = cur_s + (ts - cur_s) * alpha;
        let next_sp = (cur_sp + (tsp - cur_sp) * alpha).clamp(1.0, dur.0.max(1) as f64);
        self.anim_view_start = Some(next_s);
        self.anim_view_span = Some(next_sp);
        true
    }

    /// Janela visível usada na renderização (usa o valor suavemente animado quando ativo).
    pub fn display_view(&self, dur: Ticks) -> (Ticks, Ticks) {
        let d = dur.max(Ticks::from_raw(1));
        if let (Some(as_), Some(asp)) = (self.anim_view_start, self.anim_view_span) {
            if as_.is_finite() && asp.is_finite() && asp >= 1.0 {
                let span = Ticks::from_raw((asp.round() as i64).clamp(1, d.0));
                let max_start = d.0.saturating_sub(span.0).max(0);
                let start = Ticks::from_raw((as_.round() as i64).clamp(0, max_start));
                return (start, span);
            }
        }
        self.effective_view(d)
    }

    /// Sincroniza imediatamente a animação com a janela atual (usado durante arrasto de barra de rolagem).
    pub fn snap_zoom_animation(&mut self, dur: Ticks) {
        let (s, sp) = self.effective_view(dur);
        self.anim_view_start = Some(s.0 as f64);
        self.anim_view_span = Some(sp.0.max(1) as f64);
    }

    pub fn zoom_fit(&mut self, dur: Ticks) {
        let d = dur.max(Ticks::from_raw(1));
        self.view_start = Ticks::ZERO;
        self.view_span = d;
    }

    pub fn zoom_to_selection(&mut self, dur: Ticks, frame_dur: Ticks) {
        let d = dur.max(Ticks::from_raw(1));
        let (a, b) = (self.sel_in.min(self.sel_out), self.sel_in.max(self.sel_out));
        let sel_len = Ticks::from_raw(b.0.saturating_sub(a.0));
        if sel_len <= Ticks::ZERO {
            self.zoom_fit(d);
            return;
        }
        let pad = Ticks::from_raw((sel_len.0 / 16).max(frame_dur.0));
        let min_span = Self::min_view_span(d, frame_dur);
        let start = Ticks::from_raw(a.0.saturating_sub(pad.0).max(0));
        let end = Ticks::from_raw(b.0.saturating_add(pad.0).min(d.0));
        let span = Ticks::from_raw(end.0.saturating_sub(start.0)).clamp_safe(min_span, d);
        let max_start = d.0.saturating_sub(span.0).max(0);
        self.view_span = span;
        self.view_start = Ticks::from_raw(start.0.clamp(0, max_start));
    }

    pub fn pan_by(&mut self, delta: Ticks, dur: Ticks) {
        let d = dur.max(Ticks::from_raw(1));
        let (start, span) = self.effective_view(d);
        let max_start = d.0.saturating_sub(span.0).max(0);
        self.view_span = span;
        self.view_start = Ticks::from_raw(start.0.saturating_add(delta.0).clamp(0, max_start));
    }

    pub fn ensure_cursor_visible(&mut self, dur: Ticks) {
        let d = dur.max(Ticks::from_raw(1));
        let (start, span) = self.effective_view(d);
        if span >= d {
            return;
        }
        let end = Ticks::from_raw(start.0.saturating_add(span.0));
        let margin = Ticks::from_raw(span.0 / 10);
        let max_start = d.0.saturating_sub(span.0).max(0);
        if self.cursor < start {
            self.view_start =
                Ticks::from_raw(self.cursor.0.saturating_sub(margin.0).clamp(0, max_start));
        } else if self.cursor > end {
            let target = self
                .cursor
                .0
                .saturating_sub(span.0)
                .saturating_add(margin.0);
            self.view_start = Ticks::from_raw(target.clamp(0, max_start));
        }
    }

    /// Converte um tempo da mídia para coordenada X na faixa visível `[view_start..view_end]`.
    pub fn x_of(&self, t: Ticks, strip: Rect, dur: Ticks) -> f32 {
        let (start, span) = self.display_view(dur);
        let frac = (t.0.saturating_sub(start.0) as f64 / span.0.max(1) as f64) as f32;
        strip.left() + frac * strip.width().max(1.0)
    }

    /// Converte uma coordenada X da faixa visível para tempo exato da mídia.
    pub fn t_of(&self, x: f32, strip: Rect, dur: Ticks) -> Ticks {
        let d = dur.max(Ticks::from_raw(1));
        let (start, span) = self.display_view(d);
        let frac = ((x - strip.left()) / strip.width().max(1.0)).clamp(0.0, 1.0);
        let off = (span.0 as f64 * f64::from(frac)).round() as i64;
        Ticks::from_raw(start.0.saturating_add(off)).clamp_safe(Ticks::ZERO, d)
    }

    pub fn zoom_wave(&mut self, factor: f32) {
        if factor.is_finite() && factor > 0.0 {
            self.wave_zoom = (self.wave_zoom * factor).clamp(WAVE_ZOOM_MIN, WAVE_ZOOM_MAX);
        }
    }

    pub fn set_in(&mut self, frame_dur: Ticks, dur: Ticks) {
        self.sel_in = self.cursor;
        if self.sel_out <= self.sel_in {
            self.sel_out = (self.sel_in + frame_dur).min(dur);
        }
    }

    pub fn set_out(&mut self, frame_dur: Ticks) {
        self.sel_out = self.cursor.max(frame_dur);
        if self.sel_in >= self.sel_out {
            self.sel_in = (self.sel_out - frame_dur).max(Ticks::ZERO);
        }
    }

    pub fn add_media_marker(&mut self, media_id: Id, t: Ticks) {
        let list = self.media_markers.entry(media_id).or_default();
        if list.len() >= MAX_TRIMMER_MARKERS {
            return;
        }
        let n = list.len() + 1;
        list.push(Marker {
            id: new_id(),
            t: t.max(Ticks::ZERO),
            label: format!("{n}"),
            kind: MarkerKind::Standard,
            command_type: String::new(),
            command_arg: String::new(),
        });
        list.sort_by_key(|m| m.t);
    }

    pub fn add_media_region(
        &mut self,
        media_id: Id,
        start: Ticks,
        end: Ticks,
        label: Option<String>,
    ) {
        let (lo, hi) = (start.min(end).max(Ticks::ZERO), start.max(end));
        if hi <= lo {
            return;
        }
        let list = self.media_regions.entry(media_id).or_default();
        if list.len() >= MAX_TRIMMER_REGIONS {
            return;
        }
        let n = list.len() + 1;
        list.push(Region {
            id: new_id(),
            start: lo,
            end: hi,
            label: label.unwrap_or_else(|| format!("R{n}")),
        });
        list.sort_by_key(|r| r.start);
    }

    /// Salta para o marcador ou limite de seleção anterior/seguinte (`Ctrl+←` / `Ctrl+→`).
    pub fn jump_stop(&mut self, media_id: Id, forward: bool, dur: Ticks) {
        let mut pts = vec![Ticks::ZERO, self.sel_in, self.sel_out, dur];
        if let Some(ms) = self.media_markers.get(&media_id) {
            pts.extend(ms.iter().map(|m| m.t));
        }
        if let Some(rs) = self.media_regions.get(&media_id) {
            for r in rs {
                pts.push(r.start);
                pts.push(r.end);
            }
        }
        pts.sort_unstable();
        pts.dedup();
        let target = if forward {
            pts.into_iter().find(|&p| p > self.cursor).unwrap_or(dur)
        } else {
            pts.into_iter()
                .rev()
                .find(|&p| p < self.cursor)
                .unwrap_or(Ticks::ZERO)
        };
        self.cursor = target.clamp_safe(Ticks::ZERO, dur);
        self.ensure_cursor_visible(dur);
    }
}

/// Remove/recorta trechos de eventos da trilha que se sobrepõem a `[start..end]`
/// quando o modo *Sobrescrita na timeline* do Trimmer está ativo (VEGAS Pro p. 318).
pub(crate) fn punch_overwrite_range(track: &mut Track, start: Ticks, end: Ticks, min_len: Ticks) {
    if end <= start {
        return;
    }
    let old = std::mem::take(&mut track.events);
    let mut out = Vec::with_capacity(old.len() + 1);
    for mut ev in old {
        let ev_start = ev.start;
        let ev_end = ev.end();
        if ev_end <= start || ev_start >= end {
            out.push(ev);
        } else if ev_start < start && ev_end > end {
            // Divide em parte esquerda e parte direita
            let left_len = Ticks::from_raw(start.0.saturating_sub(ev_start.0));
            let right_len = Ticks::from_raw(ev_end.0.saturating_sub(end.0));
            let right_shift = Ticks::from_raw(end.0.saturating_sub(ev_start.0));
            let mut right = ev.clone();
            right.id = new_id();
            right.start = end;
            right.length = right_len;
            right.offset = ev.offset + ev.rate.scale_ticks(right_shift);
            right.fade_in.len = Ticks::ZERO;
            if left_len >= min_len {
                ev.length = left_len;
                ev.fade_out.len = ev.fade_out.len.min(left_len);
                out.push(ev);
            }
            if right_len >= min_len {
                right.fade_out.len = right.fade_out.len.min(right_len);
                out.push(right);
            }
        } else if ev_start < start {
            // Mantém apenas a parte à esquerda
            let left_len = Ticks::from_raw(start.0.saturating_sub(ev_start.0));
            if left_len >= min_len {
                ev.length = left_len;
                ev.fade_out.len = ev.fade_out.len.min(left_len);
                out.push(ev);
            }
        } else if ev_end > end {
            // Mantém apenas a parte à direita
            let right_len = Ticks::from_raw(ev_end.0.saturating_sub(end.0));
            let shift = Ticks::from_raw(end.0.saturating_sub(ev_start.0));
            if right_len >= min_len {
                ev.start = end;
                ev.length = right_len;
                ev.offset += ev.rate.scale_ticks(shift);
                ev.fade_in.len = ev.fade_in.len.min(right_len);
                out.push(ev);
            }
        }
        // Completamente coberto por [start..end]: removido
    }
    track.events = out;
}

impl App {
    /// Abre uma mídia do projeto no painel Trimmer zerando seus limites `In..Out`.
    pub fn open_in_trimmer(&mut self, media_id: Id) {
        let dur = self.media_trimmer_duration(media_id);
        self.trimmer.load_media(media_id, dur);
        self.trimmer.has_focus = true;
        self.trimmer.visible_this_frame = true;
        self.dock_visible = true;
        self.closed_tabs.remove(&DockTab::Trimmer);
        self.detached_tabs.remove(&DockTab::Trimmer);
        self.dock_tab = DockTab::Trimmer;
        self.pending_dock_focus = Some(DockTab::Trimmer);
    }

    fn media_trimmer_duration(&self, media_id: Id) -> Ticks {
        let still_len = Ticks::from_secs_f64_clamped(self.prefs.still_image_secs as f64);
        self.project
            .media(media_id)
            .and_then(|m| m.duration)
            .unwrap_or(still_len)
            .max(self.project.settings.frame_dur())
    }

    /// Insere um trecho (`offset`, `length`) de mídia na timeline, filtrando faixas A/V.
    #[allow(clippy::too_many_arguments)]
    pub fn add_trimmed_media_to_timeline(
        &mut self,
        media_id: Id,
        start: Ticks,
        offset: Ticks,
        length: Ticks,
        include_video: bool,
        include_audio: bool,
        track_hint: Option<usize>,
    ) -> Vec<Id> {
        self.add_trimmed_media_with_options(
            media_id,
            start,
            offset,
            length,
            include_video,
            include_audio,
            track_hint,
            Rate::NORMAL,
            false,
            None,
            self.trimmer.overwrite_timeline,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn add_trimmed_media_with_options(
        &mut self,
        media_id: Id,
        start: Ticks,
        offset: Ticks,
        length: Ticks,
        include_video: bool,
        include_audio: bool,
        track_hint: Option<usize>,
        rate: Rate,
        reverse: bool,
        custom_name: Option<String>,
        overwrite: bool,
    ) -> Vec<Id> {
        let Some(m) = self.project.media(media_id).cloned() else {
            return vec![];
        };
        let min_len = self.project.settings.frame_dur().max(Ticks::from_raw(1));
        let start = start.max(Ticks::ZERO);
        let offset = offset.max(Ticks::ZERO);
        let length = length.max(min_len);
        let end = start + length;
        let want_video = m.has_video && include_video;
        let want_audio = m.has_audio && include_audio;
        if !want_video && !want_audio {
            return vec![];
        }
        let mut created = vec![];
        let group = if want_video && want_audio {
            Some(new_id())
        } else {
            None
        };
        let mut video_track_idx = None;
        if want_video {
            let ti = self.find_or_create_track(TrackKind::Video, track_hint);
            if overwrite {
                punch_overwrite_range(&mut self.project.tracks[ti], start, end, min_len);
            }
            let mut e = Event::new(&m, start, length);
            e.offset = offset;
            e.rate = rate;
            e.reverse = reverse;
            if let Some(ref name) = custom_name {
                e.name = name.clone();
            }
            e.group = group;
            if m.duration.is_none() {
                e.looped = false;
            }
            created.push(e.id);
            self.project.tracks[ti].events.push(e);
            video_track_idx = Some(ti);
        }
        if want_audio {
            let hint = match video_track_idx {
                Some(v) => Some(v + 1),
                None => track_hint,
            };
            let ti = self.find_or_create_track(TrackKind::Audio, hint);
            if overwrite {
                punch_overwrite_range(&mut self.project.tracks[ti], start, end, min_len);
            }
            let mut e = Event::new(&m, start, length);
            e.offset = offset;
            e.rate = rate;
            e.reverse = reverse;
            if let Some(ref name) = custom_name {
                e.name = name.clone();
            }
            e.group = group;
            created.push(e.id);
            self.project.tracks[ti].events.push(e);
        }
        self.sel_events = created.iter().cloned().collect();
        self.changed();
        created
    }

    /// Envia a seleção do Trimmer para a timeline. `up_to_cursor` = o
    /// evento termina no cursor da timeline em vez de começar nele.
    fn trimmer_add_to_timeline(&mut self, media_id: Id, up_to_cursor: bool) {
        let frame_dur = self.project.settings.frame_dur().max(Ticks::from_raw(1));
        let len = (self.trimmer.sel_out - self.trimmer.sel_in).max(frame_dur);
        let (inc_v, inc_a) = match self.trimmer.stream_mode {
            TrimmerStreamMode::Both => (true, true),
            TrimmerStreamMode::VideoOnly => (true, false),
            TrimmerStreamMode::AudioOnly => (false, true),
        };
        let start = if up_to_cursor {
            (self.cursor - len).max(Ticks::ZERO)
        } else {
            self.cursor
        };
        self.checkpoint();
        let created = self.add_trimmed_media_to_timeline(
            media_id,
            start,
            self.trimmer.sel_in,
            len,
            inc_v,
            inc_a,
            self.sel_track,
        );
        if !created.is_empty() && self.trimmer.advance_timeline_cursor && !up_to_cursor {
            let next = self.cursor + len;
            self.set_cursor(next);
        }
    }

    /// Edição em 4 pontos (*Fit to Fill*, manual do VEGAS Pro 21, p. 318):
    /// comprime ou estica a seleção do Trimmer para preencher a seleção de tempo da timeline.
    fn trimmer_fit_to_fill(&mut self, media_id: Id) {
        let lang = self.lang();
        let Some((tl_a, tl_b)) = self.time_sel.filter(|(a, b)| b > a) else {
            self.set_status(tr(
                lang,
                "Selecione um intervalo de tempo na timeline antes de usar Ajustar para preencher.",
            ));
            return;
        };
        let frame_dur = self.project.settings.frame_dur().max(Ticks::from_raw(1));
        let trim_len = (self.trimmer.sel_out - self.trimmer.sel_in).max(frame_dur);
        let tl_len = (tl_b - tl_a).max(frame_dur);
        let ratio = (trim_len.0 as f64 / tl_len.0.max(1) as f64).clamp(0.1, 8.0);
        let rate = Rate::from_f64(ratio).unwrap_or(Rate::NORMAL);
        let (inc_v, inc_a) = match self.trimmer.stream_mode {
            TrimmerStreamMode::Both => (true, true),
            TrimmerStreamMode::VideoOnly => (true, false),
            TrimmerStreamMode::AudioOnly => (false, true),
        };
        self.checkpoint();
        let _ = self.add_trimmed_media_with_options(
            media_id,
            tl_a,
            self.trimmer.sel_in,
            tl_len,
            inc_v,
            inc_a,
            self.sel_track,
            rate,
            false,
            None,
            self.trimmer.overwrite_timeline,
        );
    }

    /// Transfere a duração da seleção de tempo da timeline para o Trimmer (`T` após cursor,
    /// `Shift+T` antes do cursor — atalhos do VEGAS Pro, p. 11).
    fn trimmer_transfer_timeline_selection(
        &mut self,
        before_cursor: bool,
        dur: Ticks,
        frame_dur: Ticks,
    ) {
        let lang = self.lang();
        let Some((tl_a, tl_b)) = self.time_sel.filter(|(a, b)| b > a) else {
            self.set_status(tr(lang, "Não há seleção de tempo ativa na timeline."));
            return;
        };
        let len = (tl_b - tl_a).max(frame_dur);
        if before_cursor {
            let out = self.trimmer.cursor.clamp_safe(frame_dur, dur);
            let inn = Ticks::from_raw(out.0.saturating_sub(len.0)).clamp_safe(Ticks::ZERO, out);
            self.trimmer.sel_in = inn;
            self.trimmer.sel_out = out;
        } else {
            let inn = self.trimmer.cursor.clamp_safe(Ticks::ZERO, dur);
            let out = Ticks::from_raw(inn.0.saturating_add(len.0)).clamp_safe(inn, dur);
            self.trimmer.sel_in = inn;
            self.trimmer.sel_out = out;
        }
        self.trimmer.clamp_bounds(dur, frame_dur);
    }

    /// Copia o quadro atual do Trimmer para a área de transferência (VEGAS Pro p. 323).
    fn trimmer_copy_snapshot(&mut self, ctx: &egui::Context, item: &MediaItem) {
        let lang = self.lang();
        if !item.has_video {
            self.set_status(tr(lang, "Mídia somente áudio"));
            return;
        }
        let w = item.width.clamp(64, 3840);
        let h = item.height.clamp(36, 2160);
        let t = self.trimmer.cursor;
        let Some(img) = self.media.video_frame(item, t, w, h) else {
            self.set_status(tr(lang, "Mídia offline / sem quadro"));
            return;
        };
        let rgba = (*img).clone();
        let ci = rgba_to_color_image(&rgba);
        ctx.copy_image(ci.clone());
        let tex = ctx.load_texture("preview_clipboard", ci, egui::TextureOptions::LINEAR);
        self.preview_clipboard = Some((tex, rgba));
        let dims = format!("{}x{}", img.width(), img.height());
        self.set_status(tr_args(
            lang,
            "Quadro do Trimmer ({dims}) copiado para a área de transferência.",
            &[("dims", &dims)],
        ));
    }

    /// Salva o quadro atual do Trimmer em arquivo e o adiciona à Mídia do Projeto (VEGAS Pro p. 323).
    fn trimmer_save_snapshot(&mut self, item: &MediaItem) {
        let lang = self.lang();
        if !item.has_video {
            self.set_status(tr(lang, "Mídia somente áudio"));
            return;
        }
        let w = item.width.clamp(64, 3840);
        let h = item.height.clamp(36, 2160);
        let Some(img) = self.media.video_frame(item, self.trimmer.cursor, w, h) else {
            self.set_status(tr(lang, "Mídia offline / sem quadro"));
            return;
        };
        if let Some(p) = rfd::FileDialog::new()
            .add_filter(
                tr(lang, "Imagem"),
                &[
                    "png", "jpg", "tiff", "tga", "bmp", "webp", "dpx", "exr", "qoi", "ppm",
                ],
            )
            .set_file_name("trimmer_quadro.png")
            .save_file()
        {
            match crate::render::save_rgba_snapshot(&img, &p) {
                Ok(()) => {
                    let p_s = p.display().to_string();
                    // Adiciona automaticamente à Mídia do Projeto (VEGAS Pro p. 323)
                    self.import_paths(vec![p], None);
                    self.set_status(tr_args(lang, "Quadro salvo em {path}", &[("path", &p_s)]));
                }
                Err(e) => {
                    let err_s = format!("{e:#}");
                    self.set_status(tr_args(lang, "Erro: {err}", &[("err", &err_s)]));
                }
            }
        }
    }

    /// Remove apenas a mídia atual do histórico do Trimmer (`Ctrl+H`, VEGAS Pro p. 319).
    fn trimmer_remove_current_from_history(&mut self) {
        let Some(cur) = self.trimmer.media_id else {
            return;
        };
        self.trimmer_pause();
        self.trimmer.history.retain(|id| *id != cur);
        self.trimmer.media_id = None;
        self.trimmer.last_frame = None;
        self.trimmer.hover_time = None;
    }

    /// Shuttle J/K/L no Trimmer (`dir = -1` reverso, `0` pausa, `1` avanço).
    fn trimmer_shuttle(&mut self, dir: i32, dur: Ticks) {
        if dir == 0 {
            self.trimmer_pause();
            return;
        }
        let next = next_shuttle_speed(self.trimmer.shuttle, dir);
        if next.abs() < 0.05 {
            self.trimmer_pause();
            return;
        }
        if !self.trimmer.playing {
            if next > 0.0 && self.trimmer.cursor >= dur {
                self.trimmer.cursor = Ticks::ZERO;
            } else if next < 0.0 && self.trimmer.cursor <= Ticks::ZERO {
                self.trimmer.cursor = dur;
            }
            self.trimmer.play_origin = self.trimmer.cursor;
        }
        self.trimmer_play_at_speed(next);
    }

    /// Intercepta e executa os atalhos de teclado do Trimmer quando ele está visível e em foco.
    /// Retorna `true` se o atalho foi consumido pelo Trimmer.
    pub(crate) fn handle_trimmer_shortcuts(&mut self, ctx: &egui::Context) -> bool {
        if !self.trimmer.has_focus || !self.trimmer.visible_this_frame {
            return false;
        }
        if self.trimmer.buttons_draft.is_some()
            || self.trimmer.subclip_dlg.is_some()
            || self.trimmer.time_edit.is_some()
        {
            return false;
        }
        let (mods, tab_pressed, shift_tab_pressed) = ctx.input_mut(|i| {
            let m = i.modifiers;
            let tab =
                !m.command && !m.alt && !m.shift && i.consume_key(egui::Modifiers::NONE, Key::Tab);
            let stab =
                !m.command && !m.alt && m.shift && i.consume_key(egui::Modifiers::SHIFT, Key::Tab);
            (m, tab, stab)
        });
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        let ctrl = mods.command;
        let shift = mods.shift;
        let alt = mods.alt;

        // Comandos de histórico funcionam mesmo sem mídia ativa (exceto Ctrl+H).
        if !ctrl && !shift && !alt && pressed(Key::H) {
            self.trimmer.sort_alpha = !self.trimmer.sort_alpha;
            return true;
        }
        if ctrl && shift && !alt && pressed(Key::H) {
            self.trimmer.history.clear();
            self.trimmer.media_id = None;
            self.trimmer.last_frame = None;
            self.trimmer_pause();
            return true;
        }
        if ctrl && !shift && !alt && pressed(Key::H) {
            self.trimmer_remove_current_from_history();
            return true;
        }
        if ctrl && !shift && !alt && pressed(Key::P) {
            self.trimmer.show_in_preview = !self.trimmer.show_in_preview;
            return true;
        }

        let Some(media_id) = self.trimmer.media_id else {
            return false;
        };
        let Some(item) = self.project.media(media_id).cloned() else {
            return false;
        };
        let frame_dur = self.project.settings.frame_dur().max(Ticks::from_raw(1));
        let dur = self.media_trimmer_duration(media_id);
        self.trimmer.clamp_bounds(dur, frame_dur);

        // Tab / Shift+Tab: alternar fluxo (Áudio+Vídeo / Vídeo / Áudio)
        if tab_pressed || shift_tab_pressed {
            self.trimmer.stream_mode = self.trimmer.stream_mode.cycle(shift_tab_pressed);
            return true;
        }

        // Ctrl+Shift+Up/Down: ajustar altura de vídeo vs áudio (VEGAS Pro p. 314)
        // Ctrl+Shift+O: limpar seleção personalizada (restaurar clipe inteiro)
        if ctrl && shift && !alt {
            if pressed(Key::O) {
                self.trimmer.sel_in = Ticks::ZERO;
                self.trimmer.sel_out = dur;
                return true;
            }
            if pressed(Key::ArrowUp) {
                self.trimmer.video_split_ratio =
                    (self.trimmer.video_split_ratio + 0.12).clamp(SPLIT_RATIO_MIN, SPLIT_RATIO_MAX);
                return true;
            }
            if pressed(Key::ArrowDown) {
                self.trimmer.video_split_ratio =
                    (self.trimmer.video_split_ratio - 0.12).clamp(SPLIT_RATIO_MIN, SPLIT_RATIO_MAX);
                return true;
            }
        }

        // Ctrl+A / Ctrl+E / Ctrl+Left / Ctrl+Right
        if ctrl && !shift && !alt {
            if pressed(Key::A) {
                self.trimmer.sel_in = Ticks::ZERO;
                self.trimmer.sel_out = dur;
                return true;
            }
            if pressed(Key::E) {
                if item.has_audio {
                    self.send_to_external_app(
                        neoveg_engine::integrations::ExternalApp::SoundCraft,
                        None,
                        Some(media_id),
                    );
                }
                return true;
            }
            if pressed(Key::ArrowLeft) {
                self.trimmer.jump_stop(media_id, false, dur);
                return true;
            }
            if pressed(Key::ArrowRight) {
                self.trimmer.jump_stop(media_id, true, dur);
                return true;
            }
            if pressed(Key::Home) {
                self.trimmer_button(TrimBtn::GoStart, media_id, dur, frame_dur);
                self.trimmer.ensure_cursor_visible(dur);
                return true;
            }
            if pressed(Key::End) {
                self.trimmer_button(TrimBtn::GoEnd, media_id, dur, frame_dur);
                self.trimmer.ensure_cursor_visible(dur);
                return true;
            }
        }

        // Shift+<tecla>
        if !ctrl && shift && !alt {
            if pressed(Key::Space) {
                self.trimmer_button(TrimBtn::PlayFromStart, media_id, dur, frame_dur);
                return true;
            }
            if pressed(Key::A) {
                self.trimmer_button(TrimBtn::AddUpToCursor, media_id, dur, frame_dur);
                return true;
            }
            if pressed(Key::T) {
                self.trimmer_transfer_timeline_selection(true, dur, frame_dur);
                return true;
            }
            if pressed(Key::ArrowUp) {
                self.trimmer.zoom_wave(1.25);
                return true;
            }
            if pressed(Key::ArrowDown) {
                self.trimmer.zoom_wave(0.8);
                return true;
            }
            if pressed(Key::ArrowLeft) {
                let prev = self.trimmer.cursor;
                let next = (prev - frame_dur).max(Ticks::ZERO);
                if self.trimmer.sel_in == prev {
                    self.trimmer.sel_in = next;
                } else if self.trimmer.sel_out == prev && next > self.trimmer.sel_in {
                    self.trimmer.sel_out = next;
                } else {
                    self.trimmer.sel_in = next;
                    self.trimmer.sel_out = prev.max(next + frame_dur).min(dur);
                }
                self.trimmer.cursor = next;
                self.trimmer.clamp_bounds(dur, frame_dur);
                self.trimmer.ensure_cursor_visible(dur);
                return true;
            }
            if pressed(Key::ArrowRight) {
                let prev = self.trimmer.cursor;
                let next = (prev + frame_dur).min(dur);
                if self.trimmer.sel_out == prev {
                    self.trimmer.sel_out = next;
                } else if self.trimmer.sel_in == prev && next < self.trimmer.sel_out {
                    self.trimmer.sel_in = next;
                } else {
                    self.trimmer.sel_in = prev.min(next);
                    self.trimmer.sel_out = next.max(self.trimmer.sel_in + frame_dur).min(dur);
                }
                self.trimmer.cursor = next;
                self.trimmer.clamp_bounds(dur, frame_dur);
                self.trimmer.ensure_cursor_visible(dur);
                return true;
            }
        }

        // Teclas sem modificadores
        if !ctrl && !shift && !alt {
            if pressed(Key::Space) {
                if self.trimmer.playing {
                    self.trimmer_stop();
                } else {
                    self.trimmer_button(TrimBtn::Play, media_id, dur, frame_dur);
                }
                return true;
            }
            if pressed(Key::Enter) {
                if self.trimmer.playing {
                    self.trimmer_pause();
                } else {
                    self.trimmer_button(TrimBtn::Play, media_id, dur, frame_dur);
                }
                return true;
            }
            if pressed(Key::Escape) {
                self.trimmer_stop();
                return true;
            }
            if pressed(Key::Q) {
                self.trimmer_button(TrimBtn::Loop, media_id, dur, frame_dur);
                return true;
            }
            if pressed(Key::J) {
                self.trimmer_shuttle(-1, dur);
                return true;
            }
            if pressed(Key::K) {
                self.trimmer_shuttle(0, dur);
                return true;
            }
            if pressed(Key::L) {
                self.trimmer_shuttle(1, dur);
                return true;
            }
            if pressed(Key::ArrowLeft) {
                self.trimmer_button(TrimBtn::PrevFrame, media_id, dur, frame_dur);
                self.trimmer.ensure_cursor_visible(dur);
                return true;
            }
            if pressed(Key::ArrowRight) {
                self.trimmer_button(TrimBtn::NextFrame, media_id, dur, frame_dur);
                self.trimmer.ensure_cursor_visible(dur);
                return true;
            }
            if pressed(Key::Home) {
                self.trimmer_button(TrimBtn::GoStart, media_id, dur, frame_dur);
                self.trimmer.ensure_cursor_visible(dur);
                return true;
            }
            if pressed(Key::End) {
                self.trimmer_button(TrimBtn::GoEnd, media_id, dur, frame_dur);
                self.trimmer.ensure_cursor_visible(dur);
                return true;
            }
            if pressed(Key::I) || pressed(Key::OpenBracket) {
                self.trimmer_button(TrimBtn::SetIn, media_id, dur, frame_dur);
                return true;
            }
            if pressed(Key::O) || pressed(Key::CloseBracket) {
                self.trimmer_button(TrimBtn::SetOut, media_id, dur, frame_dur);
                return true;
            }
            if pressed(Key::A) {
                self.trimmer_button(TrimBtn::AddFromCursor, media_id, dur, frame_dur);
                return true;
            }
            if pressed(Key::T) {
                self.trimmer_transfer_timeline_selection(false, dur, frame_dur);
                return true;
            }
            if pressed(Key::ArrowUp) || pressed(Key::Equals) || pressed(Key::Plus) {
                self.trimmer.zoom_around_playhead(1.25, dur, frame_dur);
                return true;
            }
            if pressed(Key::ArrowDown) || pressed(Key::Minus) {
                self.trimmer.zoom_around_playhead(0.8, dur, frame_dur);
                return true;
            }
            if pressed(Key::M) {
                self.trimmer_button(TrimBtn::InsertMarker, media_id, dur, frame_dur);
                return true;
            }
            if pressed(Key::R) {
                self.trimmer_button(TrimBtn::InsertRegion, media_id, dur, frame_dur);
                return true;
            }
        }

        false
    }

    fn trimmer_button(&mut self, b: TrimBtn, media_id: Id, dur: Ticks, frame_dur: Ticks) {
        let has_sel = self.trimmer.sel_out > self.trimmer.sel_in
            && (self.trimmer.sel_in > Ticks::ZERO || self.trimmer.sel_out < dur);
        match b {
            TrimBtn::Loop => {
                self.trimmer.loop_play = !self.trimmer.loop_play;
                if self.trimmer.playing {
                    let spd = self.trimmer.shuttle;
                    self.trimmer_play_at_speed(spd);
                }
            }
            TrimBtn::PlayFromStart => {
                self.trimmer.cursor = if has_sel {
                    self.trimmer.sel_in
                } else {
                    Ticks::ZERO
                };
                self.trimmer.play_origin = self.trimmer.cursor;
                self.trimmer_play();
            }
            TrimBtn::Play => {
                if !self.trimmer.playing {
                    if self.trimmer.cursor >= dur {
                        self.trimmer.cursor = Ticks::ZERO;
                    }
                    self.trimmer.play_origin = self.trimmer.cursor;
                    self.trimmer_play();
                }
            }
            TrimBtn::Pause => self.trimmer_pause(),
            TrimBtn::Stop => self.trimmer_stop(),
            TrimBtn::GoStart => {
                self.trimmer.cursor = Ticks::ZERO;
                if self.trimmer.playing {
                    let spd = self.trimmer.shuttle;
                    self.trimmer_play_at_speed(spd);
                }
            }
            TrimBtn::GoEnd => {
                self.trimmer.cursor = dur;
                if self.trimmer.playing {
                    self.trimmer_pause();
                }
            }
            TrimBtn::PrevFrame => {
                self.trimmer.cursor = (self.trimmer.cursor - frame_dur).max(Ticks::ZERO);
                if self.trimmer.playing {
                    let spd = self.trimmer.shuttle;
                    self.trimmer_play_at_speed(spd);
                }
            }
            TrimBtn::NextFrame => {
                self.trimmer.cursor = (self.trimmer.cursor + frame_dur).min(dur);
                if self.trimmer.playing {
                    let spd = self.trimmer.shuttle;
                    self.trimmer_play_at_speed(spd);
                }
            }
            TrimBtn::OverwriteTimeline => {
                self.trimmer.overwrite_timeline = !self.trimmer.overwrite_timeline;
            }
            TrimBtn::SetIn => self.trimmer.set_in(frame_dur, dur),
            TrimBtn::SetOut => self.trimmer.set_out(frame_dur),
            TrimBtn::AddFromCursor => self.trimmer_add_to_timeline(media_id, false),
            TrimBtn::AddUpToCursor => self.trimmer_add_to_timeline(media_id, true),
            TrimBtn::FitToFill => self.trimmer_fit_to_fill(media_id),
            TrimBtn::InsertMarker => self.trimmer.add_media_marker(media_id, self.trimmer.cursor),
            TrimBtn::InsertRegion => {
                self.trimmer.add_media_region(
                    media_id,
                    self.trimmer.sel_in,
                    self.trimmer.sel_out,
                    None,
                );
            }
        }
    }

    pub(crate) fn trimmer_tab(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        self.trimmer.visible_this_frame = true;

        // Se o Trimmer estiver em uma janela nativa destacada, processa seus atalhos
        // também no contexto próprio da janela flutuante.
        if ui.ctx().viewport_id() != egui::ViewportId::ROOT && !ui.ctx().wants_keyboard_input() {
            self.trimmer.has_focus = true;
            let _ = self.handle_trimmer_shortcuts(ui.ctx());
        }

        // Qualquer clique dentro da área do Trimmer transfere o foco de teclado para ele.
        let panel_rect = ui.available_rect_before_wrap();
        if ui.input(|i| {
            (i.pointer.primary_pressed() || i.pointer.secondary_pressed())
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| panel_rect.contains(p))
        }) {
            self.trimmer.has_focus = true;
        }

        // Limpa mídia removida do projeto
        if let Some(id) = self.trimmer.media_id {
            if self.project.media(id).is_none() {
                self.trimmer.media_id = None;
                self.trimmer.last_frame = None;
                self.trimmer.hover_time = None;
                self.trimmer_pause();
            }
        }
        let project_ids: std::collections::HashSet<Id> =
            self.project.media.iter().map(|m| m.id).collect();
        self.trimmer.history.retain(|h| project_ids.contains(h));
        self.trimmer
            .media_markers
            .retain(|id, _| project_ids.contains(id));
        self.trimmer
            .media_regions
            .retain(|id, _| project_ids.contains(id));

        // Área de soltura em todo o painel: arrastar mídia da Mídia do Projeto ou Explorer
        // para o Trimmer (vazio ou já com mídia aberta) carrega a mídia imediatamente (VEGAS Pro p. 313).
        let pointer_pos = ui.input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()));
        let hover_media_drag = pointer_pos.is_some_and(|p| panel_rect.contains(p))
            && egui::DragAndDrop::payload::<DndPayload>(ui.ctx())
                .is_some_and(|pl| matches!(*pl, DndPayload::Media(_)));
        let mut dropped_media: Option<Id> = None;
        if hover_media_drag && ui.input(|i| i.pointer.any_released()) {
            if let Some(payload) = egui::DragAndDrop::take_payload::<DndPayload>(ui.ctx()) {
                if let DndPayload::Media(mid) = *payload {
                    dropped_media = Some(mid);
                }
            }
        }

        self.trimmer_body_ui(ui, lang, hover_media_drag);

        if let Some(mid) = dropped_media {
            self.trimmer_pause();
            let dur = self.media_trimmer_duration(mid);
            self.trimmer.load_media(mid, dur);
            self.trimmer.has_focus = true;
        }

        if hover_media_drag {
            ui.painter()
                .rect_filled(panel_rect, 2.0, self.theme.accent.gamma_multiply(0.14));
            ui.painter().rect_stroke(
                panel_rect,
                2.0,
                Stroke::new(2.0_f32, self.theme.accent),
                StrokeKind::Inside,
            );
        }

        // Contorno discreto de foco quando o Trimmer está recebendo atalhos de teclado.
        if self.trimmer.has_focus {
            ui.painter().rect_stroke(
                panel_rect,
                1.0,
                Stroke::new(1.0_f32, self.theme.accent.gamma_multiply(0.55)),
                StrokeKind::Inside,
            );
        }

        self.trimmer_buttons_dialog(ui.ctx(), lang);
        self.trimmer_subclip_dialog(ui.ctx(), lang);
    }

    fn trimmer_body_ui(&mut self, ui: &mut egui::Ui, lang: Language, hover_media_drag: bool) {
        self.trimmer_history_row(ui, lang);

        let Some(media_id) = self.trimmer.media_id else {
            ui.separator();
            let empty_size = ui.available_size().max(vec2(180.0, 110.0));
            let (empty_rect, _) = ui.allocate_exact_size(empty_size, Sense::hover());
            let p = ui.painter_at(empty_rect);
            let box_rect = empty_rect.shrink2(vec2(14.0, 14.0));
            let stroke_col = if hover_media_drag {
                self.theme.accent
            } else {
                self.theme.border
            };
            p.rect_filled(box_rect, 4.0, self.theme.extreme_bg.gamma_multiply(0.65));
            p.rect_stroke(
                box_rect,
                4.0,
                Stroke::new(if hover_media_drag { 2.0_f32 } else { 1.0_f32 }, stroke_col),
                StrokeKind::Inside,
            );
            let icon_r = Rect::from_center_size(
                pos2(box_rect.center().x, box_rect.center().y - 16.0),
                vec2(26.0, 26.0),
            );
            icons::paint(
                &p,
                icon_r,
                Icon::TrimmerBlade,
                &self.theme,
                hover_media_drag,
            );
            let prompt = if hover_media_drag {
                tr(lang, "Solte aqui para abrir no Trimmer")
            } else {
                tr(
                    lang,
                    "Arraste uma mídia da Mídia do Projeto para cá ou use 'Abrir no Trimmer'.",
                )
            };
            p.text(
                pos2(box_rect.center().x, box_rect.center().y + 14.0),
                Align2::CENTER_CENTER,
                prompt,
                FontId::proportional(11.5),
                if hover_media_drag {
                    self.theme.accent
                } else {
                    self.theme.text_muted
                },
            );
            return;
        };
        let Some(item) = self.project.media(media_id).cloned() else {
            return;
        };
        let frame_dur = self.project.settings.frame_dur().max(Ticks::from_raw(1));
        let dur = self.media_trimmer_duration(media_id);
        self.trimmer.clamp_bounds(dur, frame_dur);
        let dt_anim = ui.input(|i| i.stable_dt);
        if self.trimmer.step_zoom_animation(dur, dt_anim) {
            ui.ctx().request_repaint();
        }

        // Reprodução da prévia no Trimmer (áudio via streaming + shuttle e vídeo sincronizado)
        if self.trimmer.playing {
            let sr = self.audio.sr;
            let audio_info = {
                let s = crate::util::lock(&self.audio.shared);
                s.trimmer_audio.clone()
            };
            if let Some(trim) = audio_info.filter(|t| t.media_id == media_id && t.playing) {
                self.trimmer.cursor =
                    Ticks::from_samples(trim.pos.max(0), sr).clamp_safe(Ticks::ZERO, dur);
                self.trimmer.playing = trim.playing;
                self.trimmer.ensure_cursor_visible(dur);
                ui.ctx().request_repaint();
            } else if !item.has_audio {
                let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.25);
                let spd = if self.trimmer.shuttle.abs() >= 0.05 {
                    self.trimmer.shuttle
                } else {
                    1.0
                };
                let (c, go) = play_step_with_speed(
                    self.trimmer.cursor,
                    f64::from(dt),
                    spd,
                    (self.trimmer.sel_in, self.trimmer.sel_out),
                    self.trimmer.loop_play,
                    dur,
                );
                self.trimmer.cursor = c;
                self.trimmer.playing = go;
                if !go {
                    self.trimmer.shuttle = 0.0;
                }
                self.trimmer.ensure_cursor_visible(dur);
                ui.ctx().request_repaint();
            } else {
                self.trimmer.playing = false;
                self.trimmer.shuttle = 0.0;
            }
        }

        // Atualiza a textura do quadro mesmo se o monitor interno estiver oculto,
        // quando "Mostrar vídeo na Prévia de Vídeo" (Ctrl+P) estiver ligado.
        let preview_t = self.trimmer.hover_time.unwrap_or(self.trimmer.cursor);
        if item.has_video && !self.trimmer.show_video_monitor && self.trimmer.show_in_preview {
            self.refresh_trimmer_texture(ui.ctx(), &item, media_id, preview_t, 640, 360);
        }

        let ruler_h = 26.0;
        let scroll_h = 18.0;
        let bar_h = 30.0;
        let bottom_h = 22.0;
        let avail = ui.available_size();
        let body_h = (avail.y - ruler_h - scroll_h - bar_h - bottom_h - 14.0).max(52.0);
        let show_video = item.has_video && self.trimmer.show_video_monitor;
        let show_wave = item.has_audio;
        let splitter_h = if show_video && show_wave { 5.0 } else { 0.0 };
        let usable_h = (body_h - splitter_h).max(40.0);
        let (video_h, wave_h) = match (show_video, show_wave) {
            (true, true) => {
                let vh = (usable_h * self.trimmer.video_split_ratio)
                    .clamp(36.0, (usable_h - 28.0).max(36.0));
                let wh = (usable_h - vh).max(28.0);
                (vh, wh)
            }
            (true, false) => (usable_h, 0.0),
            (false, true) => (0.0, usable_h),
            (false, false) => (usable_h, 0.0),
        };

        // Ordem do Trimmer do VEGAS Pro: vídeo em cima, régua com marcadores e
        // regiões logo acima da forma de onda e dos controles de transporte.
        if show_video || (!show_wave && !item.has_video) {
            self.trimmer_video(
                ui,
                &item,
                media_id,
                dur,
                frame_dur,
                vec2(avail.x.max(160.0), video_h.max(36.0)),
                lang,
            );
        } else {
            self.trimmer.hover_time = None;
        }

        if show_video && show_wave {
            self.trimmer_splitter(ui, avail.x.max(160.0), splitter_h, usable_h);
        }

        self.trimmer_ruler(ui, &item, media_id, dur, frame_dur, ruler_h, lang);

        if show_wave {
            self.trimmer_waveform(ui, &item, media_id, dur, frame_dur, wave_h, lang);
        }

        self.trimmer_zoom_scrollbar(ui, dur, frame_dur, scroll_h, lang);
        self.trimmer_button_bar(ui, media_id, dur, frame_dur, lang);
        self.trimmer_times(ui, dur, frame_dur, lang);
    }

    fn refresh_trimmer_texture(
        &mut self,
        ctx: &egui::Context,
        item: &MediaItem,
        media_id: Id,
        t: Ticks,
        w: u32,
        h: u32,
    ) {
        if self.trimmer.last_frame == Some((media_id, t)) {
            return;
        }
        if let Some(img) = self
            .media
            .video_frame(item, t, w.clamp(64, 960), h.clamp(36, 540))
        {
            let ci = rgba_to_color_image(&img);
            match &mut self.trimmer.tex {
                Some(tex) => tex.set(ci, egui::TextureOptions::LINEAR),
                None => {
                    self.trimmer.tex =
                        Some(ctx.load_texture("trimmer_preview", ci, egui::TextureOptions::LINEAR));
                }
            }
        }
        self.trimmer.last_frame = Some((media_id, t));
    }

    /// Linha do alto: histórico, ordenar, remover atual, limpar, abrir o evento
    /// selecionado, seletor de fluxo e alça para arrastar seleção à timeline.
    fn trimmer_history_row(&mut self, ui: &mut egui::Ui, lang: Language) {
        let mut picked: Option<Id> = None;
        let mut clear = false;
        let mut remove_cur = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let selected = self
                .trimmer
                .media_id
                .and_then(|id| self.project.media(id))
                .map(format_trimmer_history_entry)
                .unwrap_or_else(|| tr(lang, "(Nenhum)").to_string());
            let mut entries: Vec<(Id, String)> = self
                .trimmer
                .history
                .iter()
                .filter_map(|id| {
                    self.project
                        .media(*id)
                        .map(|m| (*id, format_trimmer_history_entry(m)))
                })
                .collect();
            if self.trimmer.sort_alpha {
                entries.sort_by_key(|e| e.1.to_lowercase());
            }
            let right_w = 5.0 * 26.0 + 115.0 + 26.0;
            egui::ComboBox::from_id_salt("trimmer_history")
                .width((ui.available_width() - right_w).max(96.0))
                .selected_text(selected.as_str())
                .show_ui(ui, |ui| {
                    if entries.is_empty() {
                        ui.label(tr(lang, "(Nenhum)"));
                    }
                    for (id, name) in &entries {
                        if ui
                            .selectable_label(self.trimmer.media_id == Some(*id), name)
                            .clicked()
                        {
                            picked = Some(*id);
                        }
                    }
                })
                .response
                .on_hover_text(selected.as_str());
            let th = &self.theme;
            if icons::icon_button_named(
                ui,
                Icon::ViewList,
                tr(lang, "Ordenar o histórico por nome"),
                self.trimmer.sort_alpha,
                true,
                th,
            )
            .clicked()
            {
                self.trimmer.sort_alpha = !self.trimmer.sort_alpha;
            }
            if icons::icon_button_named(
                ui,
                Icon::CloseX,
                tr(lang, "Remover mídia atual do histórico"),
                false,
                self.trimmer.media_id.is_some(),
                th,
            )
            .clicked()
            {
                remove_cur = true;
            }
            if icons::icon_button_named(
                ui,
                Icon::DeleteTrash,
                tr(lang, "Limpar o histórico"),
                false,
                !self.trimmer.history.is_empty(),
                th,
            )
            .clicked()
            {
                clear = true;
            }
            let sel_media = self
                .sel_events
                .iter()
                .next()
                .and_then(|eid| self.project.event(*eid))
                .map(|e| e.media_id);
            if icons::icon_button_named(
                ui,
                Icon::TrimmerBlade,
                tr(lang, "Abrir o evento selecionado no Trimmer"),
                false,
                sel_media.is_some(),
                th,
            )
            .clicked()
            {
                picked = sel_media;
            }
            egui::ComboBox::from_id_salt("trimmer_stream_mode")
                .width(108.0)
                .selected_text(self.trimmer.stream_mode.label_in(lang))
                .show_ui(ui, |ui| {
                    for mode in TrimmerStreamMode::ALL {
                        ui.selectable_value(
                            &mut self.trimmer.stream_mode,
                            mode,
                            mode.label_in(lang),
                        );
                    }
                });
            // Alça de arrastar seleção diretamente para qualquer trilha da timeline (VEGAS Pro p. 317)
            if let Some(mid) = self.trimmer.media_id {
                let frame_dur = self.project.settings.frame_dur().max(Ticks::from_raw(1));
                let len = (self.trimmer.sel_out - self.trimmer.sel_in).max(frame_dur);
                let (inc_v, inc_a) = match self.trimmer.stream_mode {
                    TrimmerStreamMode::Both => (true, true),
                    TrimmerStreamMode::VideoOnly => (true, false),
                    TrimmerStreamMode::AudioOnly => (false, true),
                };
                let payload = DndPayload::TrimmedMedia {
                    media_id: mid,
                    offset: self.trimmer.sel_in,
                    length: len,
                    include_video: inc_v,
                    include_audio: inc_a,
                };
                ui.dnd_drag_source(egui::Id::new(("trimmer_drag_sel", mid)), payload, |ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(22.0, 20.0), Sense::hover());
                    ui.painter().rect_filled(r, 2.0, th.panel_fill);
                    ui.painter().rect_stroke(
                        r,
                        2.0,
                        Stroke::new(1.0_f32, th.border),
                        StrokeKind::Inside,
                    );
                    icons::paint(ui.painter(), r.shrink(3.0), Icon::MediaItem, th, true);
                })
                .response
                .on_hover_text(tr(lang, "Arrastar seleção do Trimmer para a timeline"));
            }
        });
        if clear {
            self.trimmer.history.clear();
            self.trimmer.media_id = None;
            self.trimmer.last_frame = None;
            self.trimmer.hover_time = None;
            self.trimmer_pause();
        } else if remove_cur {
            self.trimmer_remove_current_from_history();
        }
        if let Some(id) = picked {
            self.trimmer_pause();
            let dur = self.media_trimmer_duration(id);
            self.trimmer.load_media(id, dur);
            self.trimmer.has_focus = true;
        }
    }

    /// Régua com zoom, marcadores/regiões da mídia, seleção e cursor.
    #[allow(clippy::too_many_arguments)]
    fn trimmer_ruler(
        &mut self,
        ui: &mut egui::Ui,
        item: &MediaItem,
        media_id: Id,
        dur: Ticks,
        frame_dur: Ticks,
        h: f32,
        lang: Language,
    ) {
        let w = ui.available_width().max(160.0);
        let (strip, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
        let p = ui.painter_at(strip);
        let th = &self.theme;
        p.rect_filled(strip, 0.0, th.tl_ruler_bg);

        let (v_start, v_span) = self.trimmer.display_view(dur);
        let fps = self.project.settings.fps;
        let sr = self.project.settings.sample_rate;
        let fmt = self.opts.time_format;
        let n_ticks = ((strip.width() / 76.0).floor() as i128).clamp(3, 12);
        for i in 0..=n_ticks {
            let t = Ticks::from_raw(
                v_start
                    .0
                    .saturating_add((i128::from(v_span.0) * i / n_ticks) as i64),
            );
            let x = self.trimmer.x_of(t, strip, dur);
            p.line_segment(
                [pos2(x, strip.bottom() - 6.0), pos2(x, strip.bottom())],
                Stroke::new(1.0_f32, th.text_muted),
            );
            if i < n_ticks {
                // Posiciona o texto abaixo da faixa do triângulo da agulha (top + 7 px)
                // para o cursor amarelo não sobrepor o código de tempo.
                p.text(
                    pos2(x + 2.0, strip.top() + 7.5),
                    Align2::LEFT_TOP,
                    format_ticks(t, fps, sr, fmt),
                    FontId::monospace(9.0),
                    th.text_muted,
                );
            }
        }

        // Regiões da mídia na régua (verde VEGAS Pro)
        if let Some(regions) = self.trimmer.media_regions.get(&media_id) {
            for r in regions {
                let x0 = self
                    .trimmer
                    .x_of(r.start, strip, dur)
                    .clamp(strip.left(), strip.right());
                let x1 = self
                    .trimmer
                    .x_of(r.end, strip, dur)
                    .clamp(strip.left(), strip.right());
                if x1 > x0 + 1.0 {
                    let bar = Rect::from_min_max(
                        pos2(x0, strip.bottom() - 10.0),
                        pos2(x1, strip.bottom() - 6.0),
                    );
                    p.rect_filled(bar, 1.0, Color32::from_rgb(65, 170, 95));
                    p.text(
                        pos2(x0 + 2.0, strip.bottom() - 11.0),
                        Align2::LEFT_BOTTOM,
                        &r.label,
                        FontId::proportional(8.5),
                        Color32::from_rgb(130, 225, 155),
                    );
                }
            }
        }

        // Marcadores da mídia na régua (laranja VEGAS Pro)
        if let Some(markers) = self.trimmer.media_markers.get(&media_id) {
            for m in markers {
                let mx = self.trimmer.x_of(m.t, strip, dur);
                if mx >= strip.left() - 2.0 && mx <= strip.right() + 2.0 {
                    let flag = Rect::from_min_size(pos2(mx, strip.top() + 1.0), vec2(12.0, 10.0));
                    p.rect_filled(flag, 1.0, Color32::from_rgb(225, 130, 40));
                    p.line_segment(
                        [pos2(mx, strip.top()), pos2(mx, strip.bottom())],
                        Stroke::new(1.0_f32, Color32::from_rgb(225, 130, 40)),
                    );
                    p.text(
                        flag.center(),
                        Align2::CENTER_CENTER,
                        &m.label,
                        FontId::proportional(8.0),
                        Color32::BLACK,
                    );
                }
            }
        }

        // Faixa da seleção [sel_in..sel_out]
        let (a, b) = (
            self.trimmer.x_of(self.trimmer.sel_in, strip, dur),
            self.trimmer.x_of(self.trimmer.sel_out, strip, dur),
        );
        let full = self.trimmer.sel_in == Ticks::ZERO && self.trimmer.sel_out >= dur;
        if !full {
            let ax = a.clamp(strip.left(), strip.right());
            let bx = b.clamp(strip.left(), strip.right());
            if bx > ax {
                p.rect_filled(
                    Rect::from_min_max(pos2(ax, strip.bottom() - 7.0), pos2(bx, strip.bottom())),
                    0.0,
                    th.tl_sel_band,
                );
            }
            for (idx, x) in [a, b].into_iter().enumerate() {
                if x >= strip.left() - 1.0 && x <= strip.right() + 1.0 {
                    p.line_segment(
                        [pos2(x, strip.top()), pos2(x, strip.bottom())],
                        Stroke::new(1.5_f32, th.accent),
                    );
                    let dir = if idx == 0 { 1.0_f32 } else { -1.0_f32 };
                    p.add(Shape::convex_polygon(
                        vec![
                            pos2(x, strip.bottom() - 7.0),
                            pos2(x + dir * 5.0, strip.bottom() - 7.0),
                            pos2(x, strip.bottom() - 1.0),
                        ],
                        th.accent,
                        Stroke::NONE,
                    ));
                }
            }
        }

        // Linha guia de Hover Scrub
        if let Some(ht) = self.trimmer.hover_time {
            let hx = self.trimmer.x_of(ht, strip, dur);
            if hx >= strip.left() && hx <= strip.right() {
                p.line_segment(
                    [pos2(hx, strip.top()), pos2(hx, strip.bottom())],
                    Stroke::new(1.0_f32, Color32::from_rgb(130, 200, 255)),
                );
            }
        }

        // Cursor: triângulo amarelo no alto e linha fina
        let cx = self.trimmer.x_of(self.trimmer.cursor, strip, dur);
        let stroke_w = if th.high_contrast { 1.5_f32 } else { 1.0_f32 };
        if cx >= strip.left() - 6.0 && cx <= strip.right() + 6.0 {
            p.add(egui::Shape::convex_polygon(
                vec![
                    pos2(cx - 5.0, strip.top()),
                    pos2(cx + 5.0, strip.top()),
                    pos2(cx, strip.top() + 7.0),
                ],
                Color32::from_rgb(240, 200, 40),
                Stroke::NONE,
            ));
            p.line_segment(
                [pos2(cx, strip.top()), pos2(cx, strip.bottom())],
                Stroke::new(stroke_w, th.tl_playhead_edit),
            );
        }
        p.rect_stroke(
            strip,
            0.0,
            Stroke::new(stroke_w, th.border),
            StrokeKind::Inside,
        );

        self.trimmer_wheel_zoom(ui, &resp, strip, dur, frame_dur, false);
        self.trimmer_pointer(ui, &resp, strip, item, media_id, dur, frame_dur, true);
        resp.context_menu(|ui| {
            self.trimmer_context_menu(ui, item, media_id, dur, frame_dur, lang);
        });
    }

    /// Trata roda do mouse sobre régua, monitor de vídeo ou forma de onda
    /// usando exatamente a mesma fórmula e ancoragem na agulha da linha do tempo:
    /// roda = zoom contínuo em torno da agulha; Shift+roda = rolagem horizontal;
    /// Ctrl+roda / Alt+roda (na forma de onda) = zoom vertical da forma de onda.
    fn trimmer_wheel_zoom(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        strip: Rect,
        dur: Ticks,
        frame_dur: Ticks,
        allow_wave_zoom: bool,
    ) {
        if !resp.hovered() {
            return;
        }
        let (raw_scroll, smooth_scroll, mods) =
            ui.input(|i| (i.raw_scroll_delta, i.smooth_scroll_delta, i.modifiers));
        let scroll = if raw_scroll != egui::Vec2::ZERO {
            raw_scroll
        } else {
            smooth_scroll
        };
        if scroll == egui::Vec2::ZERO {
            return;
        }
        let d = if scroll.y != 0.0 { scroll.y } else { scroll.x };
        if allow_wave_zoom && mods.alt {
            let factor = (1.0 + d * 0.0025).clamp(0.5, 2.0);
            self.trimmer.zoom_wave(factor);
            return;
        }
        let horiz_mode = self.prefs.timeline_mouse_scroll
            == neoveg_common::prefs::TimelineMouseScroll::HorizontalScroll;
        if horiz_mode {
            if mods.command {
                let factor = f64::from((1.0 + d * 0.0025).clamp(0.5, 2.0));
                self.trimmer.zoom_around_playhead(factor, dur, frame_dur);
                ui.ctx().request_repaint();
            } else {
                let (_, span) = self.trimmer.effective_view(dur);
                let frac = f64::from(-d) / f64::from(strip.width().max(1.0));
                let delta = Ticks::from_raw((span.0 as f64 * frac * 1.5).round() as i64);
                self.trimmer.pan_by(delta, dur);
                ui.ctx().request_repaint();
            }
            return;
        }
        if mods.shift || (scroll.x != 0.0 && scroll.y == 0.0) {
            let (_, span) = self.trimmer.effective_view(dur);
            let frac = f64::from(-d) / f64::from(strip.width().max(1.0));
            let delta = Ticks::from_raw((span.0 as f64 * frac * 1.5).round() as i64);
            self.trimmer.pan_by(delta, dur);
            ui.ctx().request_repaint();
            return;
        }
        if allow_wave_zoom && mods.command {
            let factor = (1.0 + d * 0.0025).clamp(0.5, 2.0);
            self.trimmer.zoom_wave(factor);
            return;
        }
        let factor = f64::from((1.0 + d * 0.0025).clamp(0.5, 2.0));
        self.trimmer.zoom_around_playhead(factor, dur, frame_dur);
        ui.ctx().request_repaint();
    }

    /// Clique, arrasto de seleção, ajuste direto das bordas In/Out, arrasto vertical
    /// da seleção para a timeline (VEGAS Pro 21, p. 317) e duplo clique para selecionar tudo.
    #[allow(clippy::too_many_arguments)]
    fn trimmer_pointer(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        strip: Rect,
        item: &MediaItem,
        media_id: Id,
        dur: Ticks,
        frame_dur: Ticks,
        allow_edge_trim: bool,
    ) {
        let full = self.trimmer.sel_in == Ticks::ZERO && self.trimmer.sel_out >= dur;
        let x_in = self.trimmer.x_of(self.trimmer.sel_in, strip, dur);
        let x_out = self.trimmer.x_of(self.trimmer.sel_out, strip, dur);
        if allow_edge_trim && !full && resp.hovered() && !resp.dragged() {
            if let Some(hp) = ui.input(|i| i.pointer.hover_pos()) {
                if (hp.x - x_in).abs() <= 6.0 || (hp.x - x_out).abs() <= 6.0 {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
            }
        }
        if resp.double_clicked() {
            self.trimmer.sel_in = Ticks::ZERO;
            self.trimmer.sel_out = dur;
            self.trimmer.drag_mode = None;
            self.trimmer.drag_anchor = None;
            return;
        }
        if resp.drag_started() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let t = self.trimmer.t_of(pos.x, strip, dur);
                self.trimmer.drag_anchor = Some(t);
                let dist_in = (pos.x - x_in).abs();
                let dist_out = (pos.x - x_out).abs();
                if allow_edge_trim && !full && (dist_in <= 6.0 || dist_out <= 6.0) {
                    self.trimmer.drag_mode = if dist_in <= dist_out {
                        Some(TrimmerDragMode::EdgeIn)
                    } else {
                        Some(TrimmerDragMode::EdgeOut)
                    };
                } else {
                    self.trimmer.drag_mode = Some(TrimmerDragMode::PendingInSelection {
                        origin: pos,
                        anchor_t: t,
                    });
                }
            }
        }
        if let Some(pos) = resp.interact_pointer_pos() {
            let t = self.trimmer.t_of(pos.x, strip, dur);
            if resp.dragged() {
                if let Some(TrimmerDragMode::PendingInSelection { origin, anchor_t }) =
                    self.trimmer.drag_mode
                {
                    let dx = (pos.x - origin.x).abs();
                    let dy = (pos.y - origin.y).abs();
                    if dy > 6.0 && dy > dx * 1.15 {
                        self.trimmer.drag_mode = Some(TrimmerDragMode::DragToTimeline);
                    } else if dx >= 4.0 {
                        self.trimmer.drag_mode = Some(TrimmerDragMode::Select { anchor_t });
                    }
                }
                match self.trimmer.drag_mode {
                    Some(TrimmerDragMode::EdgeIn) => {
                        let max_in = Ticks::from_raw(
                            self.trimmer.sel_out.0.saturating_sub(frame_dur.0).max(0),
                        );
                        self.trimmer.sel_in = t.clamp_safe(Ticks::ZERO, max_in);
                        self.trimmer.cursor = self.trimmer.sel_in;
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    Some(TrimmerDragMode::EdgeOut) => {
                        let min_out = Ticks::from_raw(
                            self.trimmer.sel_in.0.saturating_add(frame_dur.0).min(dur.0),
                        );
                        self.trimmer.sel_out = t.clamp_safe(min_out, dur);
                        self.trimmer.cursor = self.trimmer.sel_out;
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    Some(TrimmerDragMode::DragToTimeline) => {
                        let len = (self.trimmer.sel_out - self.trimmer.sel_in).max(frame_dur);
                        let (inc_v, inc_a) = match self.trimmer.stream_mode {
                            TrimmerStreamMode::Both => (true, true),
                            TrimmerStreamMode::VideoOnly => (true, false),
                            TrimmerStreamMode::AudioOnly => (false, true),
                        };
                        egui::DragAndDrop::set_payload(
                            ui.ctx(),
                            DndPayload::TrimmedMedia {
                                media_id,
                                offset: self.trimmer.sel_in,
                                length: len,
                                include_video: inc_v,
                                include_audio: inc_a,
                            },
                        );
                        let len_s = format_ticks(
                            len,
                            self.project.settings.fps,
                            self.project.settings.sample_rate,
                            self.opts.time_format,
                        );
                        let ghost = format!("{} [{}]", item.name, len_s);
                        crate::project_media::paint_media_drag_ghost(ui.ctx(), &ghost, &self.theme);
                    }
                    Some(TrimmerDragMode::Select { anchor_t }) => {
                        let (lo, hi) = (anchor_t.min(t), anchor_t.max(t));
                        if hi - lo >= frame_dur {
                            self.trimmer.sel_in = lo;
                            self.trimmer.sel_out = hi;
                        }
                        self.trimmer.cursor = t;
                    }
                    Some(TrimmerDragMode::PendingInSelection { .. }) | None => {}
                }
            } else if resp.clicked() {
                self.trimmer.cursor = t;
                if self.trimmer.playing {
                    let spd = self.trimmer.shuttle;
                    self.trimmer_play_at_speed(spd);
                }
            }
        }
        if resp.drag_stopped() {
            self.trimmer.drag_anchor = None;
            self.trimmer.drag_mode = None;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn trimmer_video(
        &mut self,
        ui: &mut egui::Ui,
        item: &MediaItem,
        media_id: Id,
        dur: Ticks,
        frame_dur: Ticks,
        size: egui::Vec2,
        lang: Language,
    ) {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, self.theme.extreme_bg);
        if !item.has_video {
            p.text(
                rect.center(),
                Align2::CENTER_CENTER,
                tr(lang, "Mídia somente áudio"),
                FontId::proportional(13.0),
                self.theme.text_muted,
            );
            resp.context_menu(|ui| {
                self.trimmer_context_menu(ui, item, media_id, dur, frame_dur, lang);
            });
            return;
        }

        // Hover Scrub no monitor de vídeo (manual do VEGAS Pro 21, p. 316)
        if self.trimmer.hover_scrub && resp.hovered() && !resp.dragged() && !self.trimmer.playing {
            if let Some(hp) = ui.input(|i| i.pointer.hover_pos()) {
                let ht = self.trimmer.t_of(hp.x, rect, dur);
                self.trimmer.hover_time = Some(ht);
            }
        } else {
            self.trimmer.hover_time = None;
        }

        let display_t = self.trimmer.hover_time.unwrap_or(self.trimmer.cursor);
        self.refresh_trimmer_texture(
            ui.ctx(),
            item,
            media_id,
            display_t,
            rect.width() as u32,
            rect.height() as u32,
        );

        match &self.trimmer.tex {
            Some(tex) => {
                let [iw, ih] = tex.size();
                let asp = (iw.max(1) as f32) / (ih.max(1) as f32);
                let mut w = rect.width();
                let mut h = w / asp.max(0.01);
                if h > rect.height() {
                    h = rect.height();
                    w = h * asp;
                }
                let r = Rect::from_center_size(rect.center(), vec2(w, h));
                p.image(
                    tex.id(),
                    r,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                p.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    tr(lang, "Mídia offline / sem quadro"),
                    FontId::proportional(13.0),
                    self.theme.text_muted,
                );
            }
        }

        // Barra discreta inferior indicando posição do Hover Scrub
        if let Some(ht) = self.trimmer.hover_time {
            let hx = self
                .trimmer
                .x_of(ht, rect, dur)
                .clamp(rect.left(), rect.right());
            let bar_r = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 4.0), rect.max);
            p.rect_filled(bar_r, 0.0, Color32::from_black_alpha(150));
            p.rect_filled(
                Rect::from_min_max(bar_r.min, pos2(hx, bar_r.max.y)),
                0.0,
                Color32::from_rgb(130, 200, 255),
            );
            let tc = format_ticks(
                ht,
                self.project.settings.fps,
                self.project.settings.sample_rate,
                self.opts.time_format,
            );
            let pill = Rect::from_min_size(
                pos2(rect.left() + 6.0, rect.bottom() - 22.0),
                vec2(84.0, 16.0),
            );
            p.rect_filled(pill, 2.0, Color32::from_black_alpha(185));
            p.text(
                pill.center(),
                Align2::CENTER_CENTER,
                tc,
                FontId::monospace(9.5),
                Color32::from_rgb(175, 225, 255),
            );
        }

        // Indicador de velocidade de shuttle J/K/L quando diferente de 1x
        if self.trimmer.playing && (self.trimmer.shuttle - 1.0).abs() > 0.05 {
            let lbl = if self.trimmer.shuttle < 0.0 {
                format!("◀ {:.1}x", self.trimmer.shuttle.abs())
            } else {
                format!("▶ {:.1}x", self.trimmer.shuttle)
            };
            let badge = Rect::from_min_size(
                pos2(rect.right() - 56.0, rect.top() + 5.0),
                vec2(50.0, 16.0),
            );
            p.rect_filled(badge, 3.0, Color32::from_black_alpha(190));
            p.text(
                badge.center(),
                Align2::CENTER_CENTER,
                lbl,
                FontId::monospace(9.5),
                Color32::from_rgb(240, 200, 40),
            );
        }

        let stroke_w = if self.theme.high_contrast {
            1.5_f32
        } else {
            1.0_f32
        };
        p.rect_stroke(
            rect,
            0.0,
            Stroke::new(stroke_w, self.theme.border),
            StrokeKind::Inside,
        );

        // Arrastar horizontalmente no monitor de vídeo seleciona In..Out, arrastar verticalmente leva à timeline e duplo clique seleciona tudo (VEGAS Pro pp. 316–317)
        self.trimmer_wheel_zoom(ui, &resp, rect, dur, frame_dur, false);
        self.trimmer_pointer(ui, &resp, rect, item, media_id, dur, frame_dur, false);
        resp.context_menu(|ui| {
            self.trimmer_context_menu(ui, item, media_id, dur, frame_dur, lang);
        });
    }

    /// Divisória horizontal arrastável entre o monitor de vídeo e a forma de onda (VEGAS Pro p. 312).
    fn trimmer_splitter(&mut self, ui: &mut egui::Ui, width: f32, h: f32, usable_h: f32) {
        let (rect, resp) = ui.allocate_exact_size(vec2(width, h), Sense::click_and_drag());
        let p = ui.painter_at(rect);
        let col = if resp.hovered() || resp.dragged() {
            self.theme.accent
        } else {
            self.theme.border
        };
        p.rect_filled(rect, 0.0, self.theme.panel_fill);
        let cy = rect.center().y;
        p.line_segment(
            [pos2(rect.left() + 12.0, cy), pos2(rect.right() - 12.0, cy)],
            Stroke::new(1.5_f32, col),
        );
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        if resp.double_clicked() {
            self.trimmer.video_split_ratio = 0.5;
        } else if resp.dragged() {
            let dy = resp.drag_delta().y;
            let dr = dy / usable_h.max(40.0);
            self.trimmer.video_split_ratio =
                (self.trimmer.video_split_ratio + dr).clamp(SPLIT_RATIO_MIN, SPLIT_RATIO_MAX);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn trimmer_waveform(
        &mut self,
        ui: &mut egui::Ui,
        item: &MediaItem,
        media_id: Id,
        dur: Ticks,
        frame_dur: Ticks,
        h: f32,
        lang: Language,
    ) {
        let total_w = ui.available_width().max(160.0);
        let vzoom_w = 20.0;
        let wave_w = (total_w - vzoom_w - 2.0).max(120.0);
        let mut wave_strip_opt: Option<(Rect, egui::Response)> = None;
        let mut vz_delta: f32 = 0.0;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let (strip, resp) = ui.allocate_exact_size(vec2(wave_w, h), Sense::click_and_drag());
            let p = ui.painter_at(strip);
            p.rect_filled(strip, 0.0, self.theme.tl_lane_even);
            let (v_start, v_span) = self.trimmer.display_view(dur);

            let full = self.trimmer.sel_in == Ticks::ZERO && self.trimmer.sel_out >= dur;
            if !full {
                let ax = self
                    .trimmer
                    .x_of(self.trimmer.sel_in, strip, dur)
                    .clamp(strip.left(), strip.right());
                let bx = self
                    .trimmer
                    .x_of(self.trimmer.sel_out, strip, dur)
                    .clamp(strip.left(), strip.right());
                if bx > ax {
                    p.rect_filled(
                        Rect::from_min_max(pos2(ax, strip.top()), pos2(bx, strip.bottom())),
                        0.0,
                        self.theme.tl_sel_band,
                    );
                }
            }

            // Forma de onda fiel às faixas de áudio da linha do tempo (`timeline.rs`):
            // estéreo L/R em duas subfaixas separadas com linha central 0 dB própria,
            // barras de pico por pixel e curva contínua amostra a amostra sob zoom alto.
            let wave_col = if self.theme.high_contrast {
                self.theme.accent
            } else {
                self.theme.tl_gain_line
            };
            let center_col = wave_col.gamma_multiply(0.45);
            if let Some(audio) = self.media.audio_map().get(&media_id) {
                let chans: Vec<(WaveChannel, Rect, Option<&'static str>)> = match (
                    self.trimmer.audio_view,
                    audio.mono_source || strip.height() < 30.0,
                ) {
                    (TrimmerAudioView::Both, false) => {
                        let my = strip.center().y;
                        vec![
                            (
                                WaveChannel::Left,
                                Rect::from_min_max(strip.min, pos2(strip.right(), my)),
                                Some("L"),
                            ),
                            (
                                WaveChannel::Right,
                                Rect::from_min_max(pos2(strip.left(), my), strip.max),
                                Some("R"),
                            ),
                        ]
                    }
                    (TrimmerAudioView::LeftOnly, _) => {
                        vec![(WaveChannel::Left, strip, Some("L"))]
                    }
                    (TrimmerAudioView::RightOnly, _) => {
                        vec![(WaveChannel::Right, strip, Some("R"))]
                    }
                    (TrimmerAudioView::Combine, _) | (TrimmerAudioView::Both, true) => {
                        vec![(WaveChannel::Mix, strip, None)]
                    }
                };
                if chans.len() == 2 {
                    let my = strip.center().y;
                    p.line_segment(
                        [pos2(strip.left(), my), pos2(strip.right(), my)],
                        Stroke::new(1.0_f32, self.theme.border.gamma_multiply(0.55)),
                    );
                }
                let cols = (strip.width().ceil() as usize).clamp(1, 2048);
                let wz = self.trimmer.wave_zoom;
                let one_sample = Ticks::from_samples(1, audio.sample_rate).0.max(1);
                let ticks_per_px = v_span.0 as f64 / cols as f64;
                let sample_mode = ticks_per_px < one_sample as f64;
                let stroke = Stroke::new(1.0_f32, wave_col);
                let curve = Stroke::new(1.2_f32, wave_col);
                let mut shapes = Vec::with_capacity(cols * chans.len() + 4);
                for (ch, cr, tag) in &chans {
                    let ya = cr.top().min(cr.bottom()) + 1.0;
                    let yb = (cr.top().max(cr.bottom()) - 1.0).max(ya);
                    let mid = cr.center().y;
                    let amp = (cr.height() * 0.46 * wz).max(1.0);
                    let y_of = |v: f32| (mid - v * amp).clamp(ya, yb);
                    shapes.push(Shape::line_segment(
                        [pos2(cr.left(), mid), pos2(cr.right(), mid)],
                        Stroke::new(1.0_f32, center_col),
                    ));
                    if sample_mode {
                        let mut pts = Vec::with_capacity(cols);
                        for i in 0..cols {
                            let x = cr.left() + (i as f32 + 0.5) * cr.width() / cols as f32;
                            let tc = Ticks::from_raw(v_start.0.saturating_add(
                                ((i128::from(v_span.0) * (2 * i as i128 + 1)) / (2 * cols as i128))
                                    as i64,
                            ));
                            let v = audio.sample_channel(*ch, tc);
                            pts.push(pos2(x, y_of(v)));
                        }
                        if pts.len() > 1 {
                            shapes.push(Shape::line(pts, curve));
                        }
                    } else {
                        for i in 0..cols {
                            let x = cr.left() + (i as f32 + 0.5) * cr.width() / cols as f32;
                            let t0 = Ticks::from_raw(v_start.0.saturating_add(
                                (i128::from(v_span.0) * i as i128 / cols as i128) as i64,
                            ));
                            let t1 = Ticks::from_raw(v_start.0.saturating_add(
                                (i128::from(v_span.0) * (i as i128 + 1) / cols as i128) as i64,
                            ));
                            let (lo, hi) = audio.channel_range(*ch, t0, t1);
                            shapes.push(Shape::line_segment(
                                [pos2(x, y_of(hi)), pos2(x, y_of(lo) + 0.5)],
                                stroke,
                            ));
                        }
                    }
                    if let Some(lbl) = tag {
                        if cr.height() >= 14.0 {
                            p.text(
                                pos2(cr.left() + 3.0, cr.top() + 2.0),
                                Align2::LEFT_TOP,
                                *lbl,
                                FontId::monospace(8.5),
                                self.theme.text_muted.gamma_multiply(0.8),
                            );
                        }
                    }
                }
                p.extend(shapes);
            } else {
                let mid = strip.center().y;
                p.line_segment(
                    [pos2(strip.left(), mid), pos2(strip.right(), mid)],
                    Stroke::new(1.0_f32, center_col),
                );
            }

            if !full {
                for edge_t in [self.trimmer.sel_in, self.trimmer.sel_out] {
                    let ex = self.trimmer.x_of(edge_t, strip, dur);
                    if ex >= strip.left() - 1.0 && ex <= strip.right() + 1.0 {
                        p.line_segment(
                            [pos2(ex, strip.top()), pos2(ex, strip.bottom())],
                            Stroke::new(1.25_f32, self.theme.accent.gamma_multiply(0.85)),
                        );
                    }
                }
            }

            // Linhas verticais de marcadores da mídia
            if let Some(markers) = self.trimmer.media_markers.get(&media_id) {
                for m in markers {
                    let mx = self.trimmer.x_of(m.t, strip, dur);
                    if mx >= strip.left() && mx <= strip.right() {
                        p.line_segment(
                            [pos2(mx, strip.top()), pos2(mx, strip.bottom())],
                            Stroke::new(
                                1.0_f32,
                                Color32::from_rgb(225, 130, 40).gamma_multiply(0.7),
                            ),
                        );
                    }
                }
            }

            let stroke_w = if self.theme.high_contrast {
                1.5_f32
            } else {
                1.0_f32
            };
            let cx = self.trimmer.x_of(self.trimmer.cursor, strip, dur);
            if cx >= strip.left() && cx <= strip.right() {
                p.line_segment(
                    [pos2(cx, strip.top()), pos2(cx, strip.bottom())],
                    Stroke::new(stroke_w, self.theme.tl_playhead_edit),
                );
            }
            p.rect_stroke(
                strip,
                0.0,
                Stroke::new(stroke_w, self.theme.border),
                StrokeKind::Inside,
            );
            wave_strip_opt = Some((strip, resp));

            // Botões verticais + / - de zoom da forma de onda (estilo VEGAS Pro)
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                let bh = ((h - 1.0) * 0.5).clamp(12.0, 20.0);
                if ui
                    .add_sized(vec2(vzoom_w, bh), egui::Button::new("+").small())
                    .on_hover_text(tr(lang, "Ampliar altura da forma de onda (Shift+↑)"))
                    .clicked()
                {
                    vz_delta = 1.25;
                }
                if ui
                    .add_sized(vec2(vzoom_w, bh), egui::Button::new("−").small())
                    .on_hover_text(tr(lang, "Reduzir altura da forma de onda (Shift+↓)"))
                    .clicked()
                {
                    vz_delta = 0.8;
                }
            });
        });

        if vz_delta > 0.0 {
            self.trimmer.zoom_wave(vz_delta);
        }
        if let Some((strip, resp)) = wave_strip_opt {
            self.trimmer_wheel_zoom(ui, &resp, strip, dur, frame_dur, true);
            self.trimmer_pointer(ui, &resp, strip, item, media_id, dur, frame_dur, true);
            resp.context_menu(|ui| {
                self.trimmer_context_menu(ui, item, media_id, dur, frame_dur, lang);
            });
        }
    }

    /// Barra de rolagem horizontal e controles de zoom do Trimmer (VEGAS Pro p. 312),
    /// incluindo alças arrastáveis nas bordas esquerda/direita para zoom interativo.
    fn trimmer_zoom_scrollbar(
        &mut self,
        ui: &mut egui::Ui,
        dur: Ticks,
        frame_dur: Ticks,
        h: f32,
        lang: Language,
    ) {
        let mut act_zoom_in = false;
        let mut act_zoom_out = false;
        let mut act_zoom_sel = false;
        let mut act_zoom_fit = false;
        let zf = self.trimmer.zoom_factor(dur);

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let controls_w = 136.0;
            let track_w = (ui.available_width() - controls_w).max(60.0);
            let (track_r, resp) =
                ui.allocate_exact_size(vec2(track_w, h - 2.0), Sense::click_and_drag());
            let p = ui.painter_at(track_r);
            p.rect_filled(track_r, 2.0, self.theme.extreme_bg);
            p.rect_stroke(
                track_r,
                2.0,
                Stroke::new(1.0_f32, self.theme.border),
                StrokeKind::Inside,
            );

            let (v_start, v_span) = self.trimmer.display_view(dur);
            let (eff_start, eff_span) = self.trimmer.effective_view(dur);
            let d = dur.0.max(1) as f64;
            let start_frac = (v_start.0 as f64 / d).clamp(0.0, 1.0) as f32;
            let span_frac = (v_span.0 as f64 / d).clamp(0.04, 1.0) as f32;
            let inner = track_r.shrink(2.0);
            let thumb_w = (inner.width() * span_frac).clamp(18.0, inner.width().max(18.0));
            let travel = (inner.width() - thumb_w).max(0.0);
            let max_start_frac = (1.0 - span_frac).max(0.0001);
            let thumb_x = inner.left() + (start_frac / max_start_frac).clamp(0.0, 1.0) * travel;
            let thumb_r =
                Rect::from_min_size(pos2(thumb_x, inner.top()), vec2(thumb_w, inner.height()));
            let thumb_col = if resp.hovered() || resp.dragged() {
                self.theme.accent
            } else {
                self.theme.text_muted.gamma_multiply(0.65)
            };
            p.rect_filled(thumb_r, 2.0, thumb_col);
            // Alças visuais nas bordas esquerda/direita do polegar para zoom (VEGAS Pro p. 312)
            let grip_col = crate::theme::contrast_on(thumb_col).gamma_multiply(0.75);
            if thumb_r.width() >= 20.0 {
                let gy0 = thumb_r.top() + 2.5;
                let gy1 = thumb_r.bottom() - 2.5;
                p.line_segment(
                    [
                        pos2(thumb_r.left() + 3.0, gy0),
                        pos2(thumb_r.left() + 3.0, gy1),
                    ],
                    Stroke::new(1.25_f32, grip_col),
                );
                p.line_segment(
                    [
                        pos2(thumb_r.right() - 3.0, gy0),
                        pos2(thumb_r.right() - 3.0, gy1),
                    ],
                    Stroke::new(1.25_f32, grip_col),
                );
            }

            let edge_hit_px = 6.0_f32;
            if resp.hovered() && !resp.dragged() {
                if let Some(hp) = ui.input(|i| i.pointer.hover_pos()) {
                    if (hp.x - thumb_r.left()).abs() <= edge_hit_px
                        || (hp.x - thumb_r.right()).abs() <= edge_hit_px
                    {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                }
            }

            if resp.drag_started() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let dl = (pos.x - thumb_r.left()).abs();
                    let dr = (pos.x - thumb_r.right()).abs();
                    if dl <= edge_hit_px && dl <= dr {
                        self.trimmer.scrollbar_drag = Some(ScrollbarDragMode::LeftEdge);
                    } else if dr <= edge_hit_px {
                        self.trimmer.scrollbar_drag = Some(ScrollbarDragMode::RightEdge);
                    } else if thumb_r.contains(pos) {
                        self.trimmer.scrollbar_drag = Some(ScrollbarDragMode::Pan {
                            offset_px: pos.x - thumb_r.left(),
                        });
                    } else {
                        self.trimmer.scrollbar_drag = Some(ScrollbarDragMode::Pan {
                            offset_px: thumb_w * 0.5,
                        });
                    }
                }
            }
            if resp.dragged() || resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let min_span = TrimmerState::min_view_span(dur, frame_dur);
                    let mode = self
                        .trimmer
                        .scrollbar_drag
                        .unwrap_or(ScrollbarDragMode::Pan {
                            offset_px: thumb_w * 0.5,
                        });
                    match mode {
                        ScrollbarDragMode::Pan { offset_px } => {
                            if travel > 0.5 {
                                let new_thumb_x =
                                    (pos.x - offset_px - inner.left()).clamp(0.0, travel);
                                let ratio = f64::from(new_thumb_x / travel);
                                let max_start = dur.0.saturating_sub(eff_span.0).max(0);
                                self.trimmer.view_start =
                                    Ticks::from_raw((max_start as f64 * ratio).round() as i64);
                                self.trimmer.snap_zoom_animation(dur);
                            }
                        }
                        ScrollbarDragMode::LeftEdge => {
                            let view_end =
                                Ticks::from_raw(eff_start.0.saturating_add(eff_span.0).min(dur.0));
                            let frac =
                                ((pos.x - inner.left()) / inner.width().max(1.0)).clamp(0.0, 1.0);
                            let max_start =
                                Ticks::from_raw(view_end.0.saturating_sub(min_span.0).max(0));
                            let new_start = Ticks::from_raw((d * f64::from(frac)).round() as i64)
                                .clamp_safe(Ticks::ZERO, max_start);
                            self.trimmer.view_start = new_start;
                            self.trimmer.view_span =
                                Ticks::from_raw(view_end.0.saturating_sub(new_start.0))
                                    .clamp_safe(min_span, dur);
                            self.trimmer.snap_zoom_animation(dur);
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                        }
                        ScrollbarDragMode::RightEdge => {
                            let frac =
                                ((pos.x - inner.left()) / inner.width().max(1.0)).clamp(0.0, 1.0);
                            let min_end =
                                Ticks::from_raw(eff_start.0.saturating_add(min_span.0).min(dur.0));
                            let new_end = Ticks::from_raw((d * f64::from(frac)).round() as i64)
                                .clamp_safe(min_end, dur);
                            self.trimmer.view_start = eff_start;
                            self.trimmer.view_span =
                                Ticks::from_raw(new_end.0.saturating_sub(eff_start.0))
                                    .clamp_safe(min_span, dur);
                            self.trimmer.snap_zoom_animation(dur);
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                        }
                    }
                }
            }
            if resp.drag_stopped() {
                self.trimmer.scrollbar_drag = None;
            }

            if ui
                .add_sized(vec2(20.0, h - 2.0), egui::Button::new("−").small())
                .on_hover_text(tr(lang, "Reduzir zoom no Trimmer (↓)"))
                .clicked()
            {
                act_zoom_out = true;
            }
            if ui
                .add_sized(vec2(20.0, h - 2.0), egui::Button::new("+").small())
                .on_hover_text(tr(lang, "Ampliar zoom no Trimmer (↑)"))
                .clicked()
            {
                act_zoom_in = true;
            }
            if ui
                .add_sized(
                    vec2(34.0, h - 2.0),
                    egui::Button::new(tr(lang, "Sel.")).small(),
                )
                .on_hover_text(tr(lang, "Zoom na seleção do Trimmer"))
                .clicked()
            {
                act_zoom_sel = true;
            }
            let zoom_lbl = if zf <= 1.02 {
                "100%".to_string()
            } else {
                format!("{zf:.1}x")
            };
            if ui
                .add_sized(vec2(48.0, h - 2.0), egui::Button::new(zoom_lbl).small())
                .on_hover_text(tr(lang, "Ajustar toda a mídia no Trimmer"))
                .clicked()
            {
                act_zoom_fit = true;
            }
        });

        if act_zoom_in {
            self.trimmer.zoom_around_playhead(1.25, dur, frame_dur);
            ui.ctx().request_repaint();
        } else if act_zoom_out {
            self.trimmer.zoom_around_playhead(0.8, dur, frame_dur);
            ui.ctx().request_repaint();
        } else if act_zoom_sel {
            self.trimmer.zoom_to_selection(dur, frame_dur);
            ui.ctx().request_repaint();
        } else if act_zoom_fit {
            self.trimmer.zoom_fit(dur);
            ui.ctx().request_repaint();
        }
    }

    /// Menu de contexto completo do Trimmer (manual do VEGAS Pro 21, pp. 312–323).
    #[allow(clippy::too_many_arguments)]
    fn trimmer_context_menu(
        &mut self,
        ui: &mut egui::Ui,
        item: &MediaItem,
        media_id: Id,
        dur: Ticks,
        frame_dur: Ticks,
        lang: Language,
    ) {
        self.trimmer.has_focus = true;

        if ui
            .button(format!(
                "{} (A)",
                tr(lang, "Adicionar à timeline a partir do cursor")
            ))
            .clicked()
        {
            self.trimmer_add_to_timeline(media_id, false);
            ui.close_menu();
        }
        if ui
            .button(format!(
                "{} (Shift+A)",
                tr(lang, "Adicionar à timeline até o cursor")
            ))
            .clicked()
        {
            self.trimmer_add_to_timeline(media_id, true);
            ui.close_menu();
        }
        let has_tl_sel = self.time_sel.is_some_and(|(a, b)| b > a);
        if ui
            .add_enabled(
                has_tl_sel,
                egui::Button::new(tr(lang, "Ajustar para preencher seleção da timeline")),
            )
            .clicked()
        {
            self.trimmer_fit_to_fill(media_id);
            ui.close_menu();
        }
        ui.checkbox(
            &mut self.trimmer.overwrite_timeline,
            tr(lang, "Habilitar sobrescrita na timeline"),
        );
        ui.checkbox(
            &mut self.trimmer.advance_timeline_cursor,
            tr(lang, "Avançar cursor da timeline ao adicionar"),
        );

        ui.separator();

        ui.menu_button(tr(lang, "Fluxo selecionado (Tab)"), |ui| {
            for mode in TrimmerStreamMode::ALL {
                if ui
                    .radio(self.trimmer.stream_mode == mode, mode.label_in(lang))
                    .clicked()
                {
                    self.trimmer.stream_mode = mode;
                    ui.close_menu();
                }
            }
        });
        if item.has_audio {
            ui.menu_button(tr(lang, "Canais de áudio"), |ui| {
                for view in TrimmerAudioView::ALL {
                    if ui
                        .radio(self.trimmer.audio_view == view, view.label_in(lang))
                        .clicked()
                    {
                        self.trimmer.audio_view = view;
                        ui.close_menu();
                    }
                }
            });
        }
        if ui
            .button(format!("{} (I)", tr(lang, "Definir ponto de entrada")))
            .clicked()
        {
            self.trimmer.set_in(frame_dur, dur);
            ui.close_menu();
        }
        if ui
            .button(format!("{} (O)", tr(lang, "Definir ponto de saída")))
            .clicked()
        {
            self.trimmer.set_out(frame_dur);
            ui.close_menu();
        }
        if ui
            .add_enabled(
                has_tl_sel,
                egui::Button::new(format!(
                    "{} (T)",
                    tr(lang, "Transferir seleção da timeline após cursor")
                )),
            )
            .clicked()
        {
            self.trimmer_transfer_timeline_selection(false, dur, frame_dur);
            ui.close_menu();
        }
        if ui
            .add_enabled(
                has_tl_sel,
                egui::Button::new(format!(
                    "{} (Shift+T)",
                    tr(lang, "Transferir seleção da timeline antes do cursor")
                )),
            )
            .clicked()
        {
            self.trimmer_transfer_timeline_selection(true, dur, frame_dur);
            ui.close_menu();
        }
        if ui
            .button(format!("{} (Ctrl+A)", tr(lang, "Selecionar tudo")))
            .clicked()
        {
            self.trimmer.sel_in = Ticks::ZERO;
            self.trimmer.sel_out = dur;
            ui.close_menu();
        }

        ui.separator();

        if ui.button(tr(lang, "Criar subclip...")).clicked() {
            let n = self
                .trimmer
                .media_regions
                .get(&media_id)
                .map_or(1, |v| v.len() + 1);
            self.trimmer.subclip_dlg = Some(SubclipDraft {
                media_id,
                name: format!("{} - Subclip {n:02}", item.name),
                reverse: false,
            });
            ui.close_menu();
        }
        if ui
            .button(format!("{} (M)", tr(lang, "Inserir marcador no Trimmer")))
            .clicked()
        {
            self.trimmer.add_media_marker(media_id, self.trimmer.cursor);
            ui.close_menu();
        }
        if ui
            .button(format!("{} (R)", tr(lang, "Inserir região no Trimmer")))
            .clicked()
        {
            self.trimmer.add_media_region(
                media_id,
                self.trimmer.sel_in,
                self.trimmer.sel_out,
                None,
            );
            ui.close_menu();
        }
        let has_mr = self
            .trimmer
            .media_markers
            .get(&media_id)
            .is_some_and(|v| !v.is_empty())
            || self
                .trimmer
                .media_regions
                .get(&media_id)
                .is_some_and(|v| !v.is_empty());
        if has_mr
            && ui
                .button(tr(lang, "Limpar marcadores e regiões da mídia"))
                .clicked()
        {
            self.trimmer.media_markers.remove(&media_id);
            self.trimmer.media_regions.remove(&media_id);
            ui.close_menu();
        }

        ui.separator();

        ui.menu_button(tr(lang, "Zoom"), |ui| {
            if ui
                .button(format!("{} (↑)", tr(lang, "Ampliar zoom")))
                .clicked()
            {
                self.trimmer.zoom_around_playhead(1.25, dur, frame_dur);
                ui.close_menu();
            }
            if ui
                .button(format!("{} (↓)", tr(lang, "Reduzir zoom")))
                .clicked()
            {
                self.trimmer.zoom_around_playhead(0.8, dur, frame_dur);
                ui.close_menu();
            }
            if ui.button(tr(lang, "Zoom na seleção")).clicked() {
                self.trimmer.zoom_to_selection(dur, frame_dur);
                ui.close_menu();
            }
            if ui.button(tr(lang, "Zoom total (Ajustar)")).clicked() {
                self.trimmer.zoom_fit(dur);
                ui.close_menu();
            }
            if item.has_audio {
                ui.separator();
                if ui
                    .button(format!("{} (Shift+↑)", tr(lang, "Ampliar forma de onda")))
                    .clicked()
                {
                    self.trimmer.zoom_wave(1.25);
                    ui.close_menu();
                }
                if ui
                    .button(format!("{} (Shift+↓)", tr(lang, "Reduzir forma de onda")))
                    .clicked()
                {
                    self.trimmer.zoom_wave(0.8);
                    ui.close_menu();
                }
                if ui
                    .button(tr(lang, "Restaurar altura da forma de onda"))
                    .clicked()
                {
                    self.trimmer.wave_zoom = 1.0;
                    ui.close_menu();
                }
            }
        });

        ui.menu_button(tr(lang, "Opções de exibição"), |ui| {
            ui.checkbox(
                &mut self.trimmer.show_video_monitor,
                tr(lang, "Mostrar monitor de vídeo"),
            );
            ui.checkbox(
                &mut self.trimmer.show_in_preview,
                format!("{} (Ctrl+P)", tr(lang, "Mostrar vídeo na Prévia de Vídeo")),
            );
            ui.checkbox(
                &mut self.trimmer.hover_scrub,
                tr(lang, "Ativar scrub ao passar o mouse (Hover Scrub)"),
            );
            if item.has_video && item.has_audio {
                ui.separator();
                if ui
                    .button(format!(
                        "{} (Ctrl+Shift+↑)",
                        tr(lang, "Altura de vídeo maior")
                    ))
                    .clicked()
                {
                    self.trimmer.video_split_ratio = 0.74;
                    ui.close_menu();
                }
                if ui
                    .button(tr(lang, "Vídeo e áudio com mesma altura"))
                    .clicked()
                {
                    self.trimmer.video_split_ratio = 0.50;
                    ui.close_menu();
                }
                if ui
                    .button(format!(
                        "{} (Ctrl+Shift+↓)",
                        tr(lang, "Altura de áudio maior")
                    ))
                    .clicked()
                {
                    self.trimmer.video_split_ratio = 0.30;
                    ui.close_menu();
                }
            }
        });

        ui.separator();

        if item.has_video {
            if ui
                .button(tr(lang, "Copiar quadro para a área de transferência"))
                .clicked()
            {
                self.trimmer_copy_snapshot(ui.ctx(), item);
                ui.close_menu();
            }
            if ui
                .button(tr(lang, "Salvar quadro como arquivo..."))
                .clicked()
            {
                self.trimmer_save_snapshot(item);
                ui.close_menu();
            }
        }
        if item.has_audio
            && ui
                .button(format!(
                    "{} (Ctrl+E)",
                    tr(lang, "Abrir no editor de áudio externo")
                ))
                .clicked()
        {
            self.send_to_external_app(
                neoveg_engine::integrations::ExternalApp::SoundCraft,
                None,
                Some(media_id),
            );
            ui.close_menu();
        }
        if ui
            .button(tr(lang, "Selecionar na Mídia do Projeto"))
            .clicked()
        {
            self.session.active_media_id = Some(media_id);
            self.dock_visible = true;
            self.closed_tabs.remove(&DockTab::ProjectMedia);
            self.dock_tab = DockTab::ProjectMedia;
            self.pending_dock_focus = Some(DockTab::ProjectMedia);
            ui.close_menu();
        }
        if ui
            .button(format!(
                "{} (Ctrl+H)",
                tr(lang, "Remover mídia atual do histórico")
            ))
            .clicked()
        {
            self.trimmer_remove_current_from_history();
            ui.close_menu();
        }
    }

    /// Barra de botões centralizada (com quebra segura em painéis estreitos)
    /// e o "…" que edita o conjunto visível.
    fn trimmer_button_bar(
        &mut self,
        ui: &mut egui::Ui,
        media_id: Id,
        dur: Ticks,
        frame_dur: Ticks,
        lang: Language,
    ) {
        let hidden = self.prefs.trimmer_hidden_buttons.clone();
        let visible: Vec<TrimBtn> = TrimBtn::ALL
            .into_iter()
            .filter(|b| !hidden.iter().any(|h| h == b.id()))
            .collect();
        let mut pressed: Option<TrimBtn> = None;
        let mut holding_step: Option<TrimBtn> = None;
        let mut more = false;
        let groups = visible.iter().filter(|b| b.starts_group()).count();
        let btn_w = 25.0;
        let total = (visible.len() + 1) as f32 * btn_w + groups as f32 * 8.0;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let pad = ((ui.available_width() - total) * 0.5).max(0.0);
            if pad > 0.0 {
                ui.add_space(pad);
            }
            for b in &visible {
                if b.starts_group() {
                    ui.separator();
                }
                let active = match b {
                    TrimBtn::Loop => self.trimmer.loop_play,
                    TrimBtn::Play => self.trimmer.playing,
                    TrimBtn::OverwriteTimeline => self.trimmer.overwrite_timeline,
                    _ => false,
                };
                let resp = icons::icon_button_named(
                    ui,
                    b.icon(),
                    tr(lang, b.name()),
                    active,
                    true,
                    &self.theme,
                );
                if resp.clicked() {
                    pressed = Some(*b);
                } else if matches!(b, TrimBtn::PrevFrame | TrimBtn::NextFrame)
                    && resp.is_pointer_button_down_on()
                {
                    holding_step = Some(*b);
                }
            }
            if icons::icon_button_named(
                ui,
                Icon::MoreDots,
                tr(lang, "Editar conjunto de botões visíveis..."),
                false,
                true,
                &self.theme,
            )
            .clicked()
            {
                more = true;
            }
        });
        if let Some(b) = pressed {
            self.trimmer.has_focus = true;
            self.trimmer.step_hold_accum = 0.0;
            self.trimmer_button(b, media_id, dur, frame_dur);
        } else if let Some(b) = holding_step {
            self.trimmer.has_focus = true;
            let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.1);
            self.trimmer.step_hold_accum += dt;
            if self.trimmer.step_hold_accum >= 0.30 {
                self.trimmer.step_hold_accum -= 0.06;
                self.trimmer_button(b, media_id, dur, frame_dur);
                self.trimmer.ensure_cursor_visible(dur);
            }
            ui.ctx().request_repaint();
        } else {
            self.trimmer.step_hold_accum = 0.0;
        }
        if more {
            self.trimmer.buttons_draft = Some(hidden);
        }
    }

    /// Linha de baixo: cursor e seleção (início, fim e duração).
    /// Duplo clique em Cursor, In ou Out permite digitar o timecode (VEGAS Pro p. 316).
    fn trimmer_times(&mut self, ui: &mut egui::Ui, dur: Ticks, frame_dur: Ticks, lang: Language) {
        let fps = self.project.settings.fps;
        let sr = self.project.settings.sample_rate;
        let fmt = self.opts.time_format;
        let fmt_t = |t: Ticks| format_ticks(t, fps, sr, fmt);
        let len = (self.trimmer.sel_out - self.trimmer.sel_in).max(Ticks::ZERO);
        let th = self.theme.clone();

        let mut start_edit: Option<(TrimmerTimeField, String)> = None;
        let mut commit_edit: Option<(TrimmerTimeField, String)> = None;
        let mut cancel_edit = false;

        ui.horizontal(|ui| {
            let total_w = 430.0;
            ui.add_space(((ui.available_width() - total_w) * 0.5).max(0.0));

            let mut draw_time_box =
                |ui: &mut egui::Ui, field: Option<TrimmerTimeField>, val: Ticks, tip: &str| {
                    let text = fmt_t(val);
                    let editing = field.is_some()
                        && self
                            .trimmer
                            .time_edit
                            .as_ref()
                            .is_some_and(|(f, _)| Some(*f) == field);
                    if editing {
                        if let Some((f, ref mut buf)) = self.trimmer.time_edit {
                            let resp = ui.add(
                                egui::TextEdit::singleline(buf)
                                    .desired_width(82.0)
                                    .font(egui::TextStyle::Monospace),
                            );
                            resp.request_focus();
                            if ui.input(|i| i.key_pressed(Key::Enter)) {
                                commit_edit = Some((f, buf.clone()));
                            } else if ui.input(|i| i.key_pressed(Key::Escape)) || resp.lost_focus()
                            {
                                cancel_edit = true;
                            }
                        }
                        return;
                    }
                    let frame_resp = egui::Frame::new()
                        .stroke(Stroke::new(1.0_f32, th.border))
                        .inner_margin(egui::Margin::symmetric(4, 1))
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&text).monospace().color(th.timecode_fg),
                                )
                                .sense(Sense::click()),
                            )
                            .on_hover_text(tip)
                        })
                        .inner;
                    if let Some(f) = field {
                        if frame_resp.double_clicked() {
                            start_edit = Some((f, text));
                        }
                    }
                };

            icons::paint_inline(ui, Icon::InsertMarker, 13.0, &th);
            draw_time_box(
                ui,
                Some(TrimmerTimeField::Cursor),
                self.trimmer.cursor,
                tr(lang, "Cursor do Trimmer"),
            );
            ui.add_space(10.0);
            icons::paint_inline(ui, Icon::MarkIn, 13.0, &th);
            draw_time_box(
                ui,
                Some(TrimmerTimeField::SelIn),
                self.trimmer.sel_in,
                tr(lang, "Início da seleção"),
            );
            icons::paint_inline(ui, Icon::MarkOut, 13.0, &th);
            draw_time_box(
                ui,
                Some(TrimmerTimeField::SelOut),
                self.trimmer.sel_out,
                tr(lang, "Fim da seleção"),
            );
            draw_time_box(ui, None, len, tr(lang, "Duração da seleção"));
        });

        if let Some((f, text)) = commit_edit {
            if let Some(parsed) = parse_time(&text, fps, sr, fmt) {
                let t = parsed.clamp_safe(Ticks::ZERO, dur);
                match f {
                    TrimmerTimeField::Cursor => {
                        self.trimmer.cursor = t;
                        self.trimmer.ensure_cursor_visible(dur);
                    }
                    TrimmerTimeField::SelIn => {
                        self.trimmer.sel_in = t;
                        if self.trimmer.sel_out <= self.trimmer.sel_in {
                            self.trimmer.sel_out = (t + frame_dur).min(dur);
                        }
                    }
                    TrimmerTimeField::SelOut => {
                        self.trimmer.sel_out = t.max(frame_dur);
                        if self.trimmer.sel_in >= self.trimmer.sel_out {
                            self.trimmer.sel_in =
                                (self.trimmer.sel_out - frame_dur).max(Ticks::ZERO);
                        }
                    }
                }
                self.trimmer.clamp_bounds(dur, frame_dur);
            }
            self.trimmer.time_edit = None;
        } else if cancel_edit {
            self.trimmer.time_edit = None;
        } else if let Some(ed) = start_edit {
            self.trimmer.time_edit = Some(ed);
        }
    }

    /// Diálogo *Criar subclip* (manual do VEGAS Pro 21, pp. 322–323).
    fn trimmer_subclip_dialog(&mut self, ctx: &egui::Context, lang: Language) {
        let Some(mut draft) = self.trimmer.subclip_dlg.take() else {
            return;
        };
        let mut open = true;
        let mut done: Option<bool> = None;
        crate::dialog_window::DialogWindow::new(tr(lang, "Criar subclip"))
            .id(egui::Id::new("trimmer_subclip_dialog"))
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "Nome:"));
                    ui.add(egui::TextEdit::singleline(&mut draft.name).desired_width(260.0));
                });
                ui.checkbox(
                    &mut draft.reverse,
                    tr(lang, "Reverter reprodução do subclip"),
                );
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(tr(lang, "OK")).clicked() {
                        done = Some(true);
                    }
                    if ui.button(tr(lang, "Cancelar")).clicked() {
                        done = Some(false);
                    }
                });
            });
        match done {
            Some(true) => {
                let frame_dur = self.project.settings.frame_dur().max(Ticks::from_raw(1));
                let len = (self.trimmer.sel_out - self.trimmer.sel_in).max(frame_dur);
                let name = draft.name.trim();
                let label = if name.is_empty() {
                    "Subclip".to_string()
                } else {
                    name.to_string()
                };
                self.trimmer.add_media_region(
                    draft.media_id,
                    self.trimmer.sel_in,
                    self.trimmer.sel_out,
                    Some(label.clone()),
                );
                let (inc_v, inc_a) = match self.trimmer.stream_mode {
                    TrimmerStreamMode::Both => (true, true),
                    TrimmerStreamMode::VideoOnly => (true, false),
                    TrimmerStreamMode::AudioOnly => (false, true),
                };
                self.checkpoint();
                let _ = self.add_trimmed_media_with_options(
                    draft.media_id,
                    self.cursor,
                    self.trimmer.sel_in,
                    len,
                    inc_v,
                    inc_a,
                    self.sel_track,
                    Rate::NORMAL,
                    draft.reverse,
                    Some(label),
                    self.trimmer.overwrite_timeline,
                );
            }
            Some(false) => {}
            None if open => self.trimmer.subclip_dlg = Some(draft),
            None => {}
        }
    }

    /// *Editar conjunto de botões visíveis* da barra do Trimmer.
    fn trimmer_buttons_dialog(&mut self, ctx: &egui::Context, lang: Language) {
        let Some(mut draft) = self.trimmer.buttons_draft.take() else {
            return;
        };
        let mut open = true;
        let mut done: Option<bool> = None;
        crate::dialog_window::DialogWindow::new(tr(lang, "Editar conjunto de botões visíveis"))
            .id(egui::Id::new("trimmer_buttons_dialog"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(tr(
                    lang,
                    "Escolha os botões que aparecem na barra do Trimmer.",
                ));
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        for b in TrimBtn::ALL {
                            let mut on = !draft.iter().any(|h| h == b.id());
                            if ui.checkbox(&mut on, tr(lang, b.name())).changed() {
                                draft.retain(|h| h != b.id());
                                if !on {
                                    draft.push(b.id().to_string());
                                }
                            }
                        }
                    });
                    ui.vertical(|ui| {
                        if ui.button(tr(lang, "Mostrar todos")).clicked() {
                            draft.clear();
                        }
                        if ui.button(tr(lang, "Ocultar todos")).clicked() {
                            draft = TrimBtn::ALL.iter().map(|b| b.id().to_string()).collect();
                        }
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(tr(lang, "OK")).clicked() {
                        done = Some(true);
                    }
                    if ui.button(tr(lang, "Cancelar")).clicked() {
                        done = Some(false);
                    }
                });
            });
        match done {
            Some(true) => {
                self.prefs.trimmer_hidden_buttons = draft;
                if let Err(e) = self.prefs.save() {
                    self.set_status(format!(
                        "{}: {e}",
                        tr(lang, "Não foi possível salvar as preferências")
                    ));
                }
            }
            Some(false) => {}
            None if open => self.trimmer.buttons_draft = Some(draft),
            None => {}
        }
    }
}

fn parent_dir_display(path: &str) -> Option<&str> {
    let trimmed = path.trim_end_matches(['/', '\\']);
    let sep = trimmed.rfind(['/', '\\'])?;
    if sep == 0 {
        return trimmed.get(..1);
    }
    let parent = trimmed.get(..sep)?;
    if parent.is_empty() {
        None
    } else {
        Some(parent)
    }
}

fn compact_history_dir(dir: &str) -> String {
    const MAX_CHARS: usize = 56;
    let total = dir.chars().count();
    if total <= MAX_CHARS {
        return dir.to_string();
    }
    let skip = total.saturating_sub(MAX_CHARS.saturating_sub(3));
    let tail: String = dir.chars().skip(skip).collect();
    format!("...{tail}")
}

/// Formata uma entrada do histórico do Trimmer no padrão `Arquivo [Diretório]`
/// do VEGAS Pro quando a mídia provém de arquivo ou sequência em disco.
pub(crate) fn format_trimmer_history_entry(item: &MediaItem) -> String {
    let raw_path = match &item.source {
        MediaSource::File { path } | MediaSource::NestedProject { path } => Some(path.as_str()),
        MediaSource::ImageSequence { paths, .. } => paths.first().map(String::as_str),
        MediaSource::Generated(_) | MediaSource::Sequence { .. } => None,
    };
    if let Some(path) = raw_path {
        if let Some(dir) = parent_dir_display(path) {
            return format!("{} [{}]", item.name, compact_history_dir(dir));
        }
    }
    item.name.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(n: i64) -> Ticks {
        Ticks::from_secs_i64(n)
    }

    #[test]
    fn historico_sem_repetir_e_com_limite() {
        let mut h = Vec::new();
        push_history(&mut h, 1);
        push_history(&mut h, 2);
        push_history(&mut h, 1);
        assert_eq!(h, vec![1, 2]);
        for i in 10..40 {
            push_history(&mut h, i);
        }
        assert_eq!(h.len(), HISTORY_MAX);
        assert_eq!(h[0], 39);
    }

    #[test]
    fn reproducao_com_e_sem_loop() {
        // sem loop: para no fim da mídia
        assert_eq!(
            play_step(s(9), s(2), (s(0), s(10)), false, s(10)),
            (s(10), false)
        );
        assert_eq!(
            play_step(s(3), s(1), (s(0), s(10)), false, s(10)),
            (s(4), true)
        );
        // com loop: volta ao início da seleção
        assert_eq!(
            play_step(s(4), s(2), (s(2), s(5)), true, s(10)),
            (s(2), true)
        );
        assert_eq!(
            play_step(s(3), s(1), (s(2), s(5)), true, s(10)),
            (s(4), true)
        );
        // cursor fora da seleção entra nela; seleção invertida é aceita
        assert_eq!(
            play_step(s(8), s(0), (s(5), s(2)), true, s(10)),
            (s(2), true)
        );
        // dt negativo não volta
        assert_eq!(
            play_step(s(3), Ticks::from_raw(-5), (s(0), s(10)), false, s(10)),
            (s(3), true)
        );
    }

    #[test]
    fn botoes_tem_identificadores_unicos() {
        let mut ids: Vec<&str> = TrimBtn::ALL.iter().map(|b| b.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), TrimBtn::ALL.len());
    }

    #[test]
    fn zoom_pan_e_mapeamento_viewport_trimmer() {
        let mut st = TrimmerState::default();
        let dur = s(20);
        let fd = Ticks::from_millis_i64(40);
        st.load_media(42, dur);
        assert_eq!(st.effective_view(dur), (Ticks::ZERO, dur));
        assert!((st.zoom_factor(dur) - 1.0).abs() < 1e-6);

        let strip = Rect::from_min_size(pos2(100.0, 0.0), vec2(400.0, 20.0));
        let anchor = s(5); // 25% da faixa
        let x_before = st.x_of(anchor, strip, dur);
        assert!((x_before - 200.0).abs() < 0.5);

        st.zoom_around(anchor, 2.0, dur, fd);
        assert!(st.zoom_factor(dur) > 1.9);
        let x_after = st.x_of(anchor, strip, dur);
        assert!(
            (x_after - x_before).abs() < 1.0,
            "zoom centrado deve manter âncora em X (antes={x_before}, depois={x_after})"
        );
        assert_eq!(st.t_of(x_after, strip, dur), anchor);

        // Pan e limites hostis
        st.pan_by(s(999), dur);
        let (vs, sp) = st.effective_view(dur);
        assert_eq!(vs + sp, dur);
        st.pan_by(Ticks::from_raw(-999_000_000_000), dur);
        assert_eq!(st.effective_view(dur).0, Ticks::ZERO);

        // Zoom na seleção e entradas hostis (NaN, Infinity, negativo) sem pânico
        st.sel_in = s(6);
        st.sel_out = s(10);
        st.zoom_to_selection(dur, fd);
        assert!(st.zoom_factor(dur) > 3.0);
        st.zoom_around(anchor, f64::NAN, dur, fd);
        st.zoom_around(anchor, -3.0, dur, fd);
        st.zoom_wave(f32::NAN);
        st.zoom_wave(100.0);
        assert!((st.wave_zoom - WAVE_ZOOM_MAX).abs() < 1e-6);
        st.zoom_fit(dur);
        assert_eq!(st.effective_view(dur), (Ticks::ZERO, dur));
    }

    #[test]
    fn shuttle_jkl_fluxos_e_sobrescrita_trimmer() {
        assert_eq!(next_shuttle_speed(0.0, 1), 1.0);
        assert_eq!(next_shuttle_speed(1.0, 1), 2.0);
        assert_eq!(next_shuttle_speed(4.0, 1), 8.0);
        assert_eq!(next_shuttle_speed(8.0, 1), 8.0);
        assert_eq!(next_shuttle_speed(4.0, -1), 2.0);
        assert_eq!(next_shuttle_speed(1.0, -1), 0.0);
        assert_eq!(next_shuttle_speed(0.0, -1), -1.0);
        assert_eq!(next_shuttle_speed(-2.0, -1), -4.0);
        assert_eq!(next_shuttle_speed(f64::NAN, 1), 1.0);

        // Ciclo de fluxo Tab / Shift+Tab
        let m = TrimmerStreamMode::Both;
        assert_eq!(m.cycle(false), TrimmerStreamMode::VideoOnly);
        assert_eq!(m.cycle(false).cycle(false), TrimmerStreamMode::AudioOnly);
        assert_eq!(m.cycle(true), TrimmerStreamMode::AudioOnly);

        // Reprodução reversa com e sem loop
        assert_eq!(
            play_step_with_speed(s(1), 1.0, -2.0, (s(0), s(10)), false, s(10)),
            (Ticks::ZERO, false)
        );
        assert_eq!(
            play_step_with_speed(s(3), 1.0, -2.0, (s(2), s(6)), true, s(10)),
            (s(6), true)
        );

        // Sobrescrita na timeline divide evento que engloba o trecho inserido
        let mut trk = Track::new(TrackKind::Video, "V1");
        let media = MediaItem {
            id: 10,
            name: "clip.avi".into(),
            source: MediaSource::Generated(GenSpec::ColorBars),
            duration: Some(s(20)),
            has_video: true,
            has_audio: false,
            width: 320,
            height: 180,
            fps: Some(FrameRate::FPS_25),
            info: String::new(),
            proxy_path: None,
        };
        trk.events.push(Event::new(&media, s(0), s(10)));
        punch_overwrite_range(&mut trk, s(3), s(6), Ticks::from_millis_i64(40));
        assert_eq!(trk.events.len(), 2);
        assert_eq!((trk.events[0].start, trk.events[0].length), (s(0), s(3)));
        assert_eq!((trk.events[1].start, trk.events[1].length), (s(6), s(4)));
        assert_eq!(trk.events[1].offset, s(6));
    }

    #[test]
    fn atalhos_teclado_trimmer_respeitam_foco() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(&ctx, None);
        app.insert_generated(GenSpec::Text(TextSpec::default()));
        let mid = app.project.media[0].id;
        app.project.media[0].duration = Some(s(10));
        app.open_in_trimmer(mid);

        let press_key = |_c: &egui::Context, k: Key, mods: egui::Modifiers| -> egui::RawInput {
            egui::RawInput {
                modifiers: mods,
                events: vec![egui::Event::Key {
                    key: k,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: mods,
                }],
                ..Default::default()
            }
        };

        // Sem foco: não intercepta
        app.trimmer.has_focus = false;
        let _ = ctx.run(press_key(&ctx, Key::I, egui::Modifiers::NONE), |c| {
            assert!(!app.handle_trimmer_shortcuts(c));
        });

        // Com foco e visível: seta direita avança 1 quadro, I marca entrada, Up aplica zoom, Tab alterna fluxo
        app.trimmer.has_focus = true;
        app.trimmer.visible_this_frame = true;
        app.trimmer.cursor = s(2);
        let fd = app.project.settings.frame_dur();
        let _ = ctx.run(
            press_key(&ctx, Key::ArrowRight, egui::Modifiers::NONE),
            |c| {
                assert!(app.handle_trimmer_shortcuts(c));
            },
        );
        assert_eq!(app.trimmer.cursor, s(2) + fd);

        let _ = ctx.run(press_key(&ctx, Key::I, egui::Modifiers::NONE), |c| {
            assert!(app.handle_trimmer_shortcuts(c));
        });
        assert_eq!(app.trimmer.sel_in, s(2) + fd);

        let _ = ctx.run(press_key(&ctx, Key::ArrowUp, egui::Modifiers::NONE), |c| {
            assert!(app.handle_trimmer_shortcuts(c));
        });
        assert!(app.trimmer.zoom_factor(s(10)) > 1.2);

        let _ = ctx.run(press_key(&ctx, Key::Tab, egui::Modifiers::NONE), |c| {
            assert!(app.handle_trimmer_shortcuts(c));
        });
        assert_eq!(app.trimmer.stream_mode, TrimmerStreamMode::VideoOnly);

        let _ = ctx.run(press_key(&ctx, Key::M, egui::Modifiers::NONE), |c| {
            assert!(app.handle_trimmer_shortcuts(c));
        });
        assert_eq!(app.trimmer.media_markers.get(&mid).map(Vec::len), Some(1));

        // Ctrl+Shift+O restaura a seleção para toda a mídia
        let mut ctrl_shift = egui::Modifiers::COMMAND;
        ctrl_shift.shift = true;
        let _ = ctx.run(press_key(&ctx, Key::O, ctrl_shift), |c| {
            assert!(app.handle_trimmer_shortcuts(c));
        });
        assert_eq!(app.trimmer.sel_in, Ticks::ZERO);
        assert_eq!(app.trimmer.sel_out, s(10));
    }

    #[test]
    fn animacao_zoom_suave_agulha_e_canais_trimmer() {
        let mut st = TrimmerState::default();
        let dur = s(20);
        let fd = Ticks::from_millis_i64(40);
        st.load_media(99, dur);
        st.cursor = s(6);
        let strip = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 40.0));
        let cx_before = st.x_of(st.cursor, strip, dur);

        // Inicializa animação no primeiro quadro (sem animação pendente)
        assert!(!st.step_zoom_animation(dur, 0.016));
        st.zoom_around_playhead(2.0, dur, fd);
        assert!(st.step_zoom_animation(dur, 0.016));

        // Converge suavemente para o alvo em número finito de quadros
        let mut still_running = true;
        for _ in 0..80 {
            still_running = st.step_zoom_animation(dur, 0.016);
            if !still_running {
                break;
            }
        }
        assert!(!still_running);
        assert_eq!(st.display_view(dur), st.effective_view(dur));
        let cx_after = st.x_of(st.cursor, strip, dur);
        assert!(
            (cx_after - cx_before).abs() < 1.5,
            "zoom centrado na agulha deve manter posição da agulha (antes={cx_before}, depois={cx_after})"
        );

        // Snap sincroniza imediatamente sem esperar quadros
        st.zoom_fit(dur);
        st.snap_zoom_animation(dur);
        assert_eq!(st.display_view(dur), (Ticks::ZERO, dur));

        // Entradas hostis (dt NaN / negativo) nunca entram em pânico
        assert!(!st.step_zoom_animation(dur, f32::NAN));
        assert!(!st.step_zoom_animation(dur, -1.0));

        // Todos os modos de visualização de canais têm rótulos distintos
        let labels: Vec<&str> = TrimmerAudioView::ALL
            .iter()
            .map(|v| v.label_in(Language::PtBr))
            .collect();
        assert_eq!(labels.len(), 4);
        assert!(labels.iter().all(|l| !l.is_empty()));
    }

    #[test]
    fn historico_trimmer_formata_arquivo_com_diretorio() {
        let make_item = |name: &str, source: MediaSource| MediaItem {
            id: 1,
            name: name.into(),
            source,
            duration: Some(s(5)),
            has_video: true,
            has_audio: true,
            width: 1280,
            height: 720,
            fps: Some(FrameRate::FPS_25),
            info: String::new(),
            proxy_path: None,
        };

        let file_unix = make_item(
            "cena01.mp4",
            MediaSource::File {
                path: "/projetos/filme/cena01.mp4".into(),
            },
        );
        assert_eq!(
            format_trimmer_history_entry(&file_unix),
            "cena01.mp4 [/projetos/filme]"
        );

        let file_win = make_item(
            "cena02.avi",
            MediaSource::File {
                path: r"C:\Videos\Sessao\cena02.avi".into(),
            },
        );
        assert_eq!(
            format_trimmer_history_entry(&file_win),
            r"cena02.avi [C:\Videos\Sessao]"
        );

        let bare = make_item(
            "solto.wav",
            MediaSource::File {
                path: "solto.wav".into(),
            },
        );
        assert_eq!(format_trimmer_history_entry(&bare), "solto.wav");

        let long_dir = "/muito/longo/caminho/com/acentuação/árvore/produção/2026/tomadas/final";
        let long_item = make_item(
            "take.mov",
            MediaSource::File {
                path: format!("{long_dir}/take.mov"),
            },
        );
        let formatted = format_trimmer_history_entry(&long_item);
        assert!(formatted.starts_with("take.mov [..."));
        assert!(formatted.ends_with(']'));

        let generated = make_item("Barras", MediaSource::Generated(GenSpec::ColorBars));
        assert_eq!(format_trimmer_history_entry(&generated), "Barras");

        let seq = make_item("Sequência 02", MediaSource::Sequence { id: 7 });
        assert_eq!(format_trimmer_history_entry(&seq), "Sequência 02");
    }
}
