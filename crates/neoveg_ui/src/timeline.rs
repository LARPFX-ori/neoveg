//! Timeline: régua, barra de marcadores/regiões, lista de trilhas (track
//! headers) e área de eventos com edição por mouse (mover, aparar, fades,
//! slip, envelopes, seleção de tempo, arrastar e soltar).

use super::{App, DndPayload, FxTarget};
use crate::icons::{self, Icon};
use crate::model::*;
use eframe::egui;
use egui::{
    pos2, vec2, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind,
};
use neoveg_common::{tr, tr_args};
use std::collections::{HashMap, HashSet};

mod keyframes;
use keyframes::KfArea;
use neoveg_engine::media::audio::WaveChannel;
use neoveg_engine::session::{
    apply_roll_from_snapshot, apply_shuffle_from_snapshot, apply_slide_from_snapshot,
    apply_stretch_from_snapshot, BoxAxisMode, EditMode, EditTool, StretchEventSnapshot,
    TrackEventSnapshot,
};
use neoveg_engine::track_keyframes::KfRowKind;

/// Altura de cada linha de quadros-chave abaixo de uma trilha de vídeo.
const KF_ROW_H: f32 = 18.0;

const HEADER_W: f32 = 250.0;
const LOOP_H: f32 = 12.0;
const MARKER_H: f32 = 18.0;
const RULER_H: f32 = 22.0;
const TOP_H: f32 = LOOP_H + MARKER_H + RULER_H;
const TRACK_FOOTER_H: f32 = 26.0;
const STATUS_BAR_H: f32 = 22.0;
const BOTTOM_BARS_H: f32 = TRACK_FOOTER_H + STATUS_BAR_H;
const EDGE_PX: f32 = 5.0;
const DOC_TAB_H: f32 = 22.0;
const SEQ_TAB_H: f32 = 22.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EnvOwner {
    Track(usize),
    Bus(usize),
    MasterVideo,
}

struct TimelineRows {
    /// Trilha do Barramento de Vídeo (*Video Bus Track*, `Ctrl+Shift+B`) no topo.
    video_bus_rect: Option<(f32, f32)>,
    /// Área dos eventos de cada trilha (sem as linhas de quadros-chave).
    track_rects: Vec<(f32, f32)>,
    /// Trilha inteira, com as linhas de quadros-chave: para saber em que
    /// trilha o ponteiro está (soltar mídia, mover entre trilhas).
    track_spans: Vec<(f32, f32)>,
    /// Linhas de quadros-chave abaixo de cada trilha, se houver.
    kf_areas: Vec<Option<KfArea>>,
    bus_rects: Vec<(f32, f32)>,
}

impl TimelineRows {
    fn row_rect(&self, owner: EnvOwner) -> Option<(f32, f32)> {
        match owner {
            EnvOwner::Track(ti) => self.track_rects.get(ti).copied(),
            EnvOwner::Bus(bi) => self.bus_rects.get(bi).copied(),
            EnvOwner::MasterVideo => self.video_bus_rect,
        }
    }

    fn owner_at(&self, y: f32) -> Option<EnvOwner> {
        if let Some((y0, h)) = self.video_bus_rect {
            if y >= y0 && y < y0 + h {
                return Some(EnvOwner::MasterVideo);
            }
        }
        if let Some(ti) = self
            .track_rects
            .iter()
            .position(|(y0, h)| y >= *y0 && y < y0 + h)
        {
            return Some(EnvOwner::Track(ti));
        }
        self.bus_rects
            .iter()
            .position(|(y0, h)| y >= *y0 && y < y0 + h)
            .map(EnvOwner::Bus)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Zone {
    Body,
    LeftEdge,
    RightEdge,
    FadeIn,
    FadeOut,
    GainHandle,
    OpacityHandle,
    FxButton,
    PanCropButton,
    MoreButton,
}

#[derive(Clone, Debug, Default)]
enum Drag {
    #[default]
    None,
    Scrub,
    TimeSel {
        anchor: Ticks,
    },
    LoopEdge {
        start: bool,
    },
    LoopMove {
        orig: (Ticks, Ticks),
        t0: Ticks,
    },
    Move {
        anchor_id: Id,
        anchor_start: Ticks,
        origin_t: Ticks,
        origin_track: usize,
        orig: Vec<(Id, Ticks, usize)>,
        sync_followers: Vec<(Id, Ticks)>,
        /// Travar envelopes nos eventos: foto da automação de cada trilha de
        /// origem (id da trilha) no começo do arraste.
        env_lock: Vec<(Id, neoveg_engine::env_lock::AutomationSnapshot)>,
    },
    Trim {
        left: bool,
        slip: bool,
        origin_t: Ticks,
        anchor_id: Id,
        orig: Vec<(Id, Ticks, Ticks, Ticks)>,
    },
    Fade {
        id: Id,
        fade_in: bool,
    },
    Gain {
        id: Id,
        orig_db: f32,
        origin_y: f32,
        event_h: f32,
    },
    Opacity {
        id: Id,
        orig_opacity: f32,
        origin_y: f32,
        event_h: f32,
    },
    Slip {
        origin_t: Ticks,
        orig: Vec<(Id, Ticks)>,
    },
    Shuffle {
        anchor_id: Id,
        track_idx: usize,
        origin_t: Ticks,
        anchor_orig_start: Ticks,
        snap: Vec<TrackEventSnapshot>,
        partners: Vec<(Id, Ticks)>,
        sync_followers: Vec<(Id, Ticks)>,
    },
    Slide {
        anchor_id: Id,
        track_idx: usize,
        origin_t: Ticks,
        anchor_orig_start: Ticks,
        snap: Vec<TrackEventSnapshot>,
        partners: Vec<(Id, Ticks)>,
        sync_followers: Vec<(Id, Ticks)>,
    },
    /// Desliza a janela de crossfade (`Ctrl+Alt+Arrastar` sobre a sobreposição),
    /// mantendo a duração do crossfade e a sincronia da mídia.
    SlideCrossfade {
        origin_t: Ticks,
        anchor_start: Ticks,
        pairs: Vec<neoveg_engine::crossfade::SlideCrossfadePair>,
    },
    Stretch {
        left: bool,
        origin_t: Ticks,
        anchor_id: Id,
        snaps: Vec<StretchEventSnapshot>,
    },
    Roll {
        anchor_id: Id,
        track_idx: usize,
        left_edge: bool,
        origin_t: Ticks,
        orig_edge: Ticks,
        snap: Vec<TrackEventSnapshot>,
    },
    SplitTrim {
        event_id: Id,
        origin_t: Ticks,
        cur_t: Ticks,
    },
    /// Caixa das ferramentas Seleção (`zoom: false`) e Zoom.
    Box {
        zoom: bool,
        start: Pos2,
    },
    EnvPoint {
        owner: EnvOwner,
        kind: EnvKind,
        idx: usize,
    },
    Marker {
        id: Id,
    },
    RegionEdge {
        id: Id,
        start: bool,
    },
    TrackHeight {
        track: usize,
        orig_h: f32,
        y0: f32,
    },
    Pan {
        last: Pos2,
    },
    Keyframe {
        track: Id,
        row: KfRowKind,
        at: Ticks,
        moved: bool,
    },
}

#[derive(Clone, Debug, Default)]
enum CtxTarget {
    #[default]
    None,
    Event(Id),
    EventFade(Id, bool),
    Crossfade {
        track_idx: usize,
        left_id: Id,
        right_id: Id,
        active: bool,
    },
    EnvPoint(EnvOwner, EnvKind, usize),
    EnvLine(EnvOwner, EnvKind, Ticks),
    Lane(usize, Ticks),
    Marker(Id),
    Region(Id),
    Keyframe {
        track: Id,
        row: KfRowKind,
        key: Option<Ticks>,
        t: Ticks,
        has_curve: bool,
    },
}

/// Menu de contexto aberto ao soltar uma mídia com o botão direito na timeline.
#[derive(Clone, Debug)]
pub struct DropContextMenuState {
    pub pos: Pos2,
    pub media_id: Id,
    pub track: Option<usize>,
    pub t: Ticks,
    pub target_event: Option<Id>,
}

pub struct TimelineState {
    pub px_per_sec: f32,
    pub scroll_x: Ticks,
    pub scroll_y: f32,
    pub width: f32,
    drag: Drag,
    ctx_target: CtxTarget,
    rename: Option<(Id, String)>,
    snap_line: Option<Ticks>,
    drop_hint: Option<(usize, Ticks)>,
    /// Menu de contexto aberto ao soltar mídia com botão direito.
    pub drop_context_menu: Option<DropContextMenuState>,
    /// Trilhas com a área de quadros-chave recolhida (estado de exibição).
    pub kf_collapsed: HashSet<Id>,
    /// Trilhas de vídeo com *Expandir camadas da trilha* ligado: rolo A em
    /// cima, rolo de transição no meio e rolo B embaixo (estado de exibição).
    pub layers_expanded: HashSet<Id>,
    /// Rolo de cada evento das trilhas expandidas (0 = A, 1 = B), refeito a
    /// cada quadro ao desenhar as trilhas.
    layer_row: HashMap<Id, u8>,
    /// Ponto atual da caixa das ferramentas Seleção e Zoom.
    box_cur: Option<Pos2>,
    /// Origem e seleção de tempo anterior ao arrastar espaço vazio (permite
    /// clique-esquerdo + clique-direito virar caixa de seleção, VEGAS Pro p. 238).
    time_sel_origin: Option<(Pos2, Option<(Ticks, Ticks)>)>,
    /// Suprime menu de contexto quando o botão direito alternou modo da caixa durante arrasto.
    suppress_ctx_menu: bool,
    /// Retângulos das barras de projetos abertos e de sequências no último quadro.
    pub(crate) last_doc_bar: Rect,
    pub(crate) last_seq_bar: Rect,
    /// Área das faixas e posição de cada trilha no último quadro.
    pub(crate) last_lanes: Rect,
    pub(crate) last_rows: Vec<(f32, f32)>,
    /// Largura atual e anterior do painel esquerdo de cabeçalhos de trilha (p. 69).
    pub header_w: f32,
    pub header_prev_w: f32,
    /// Ajusta automaticamente a largura horizontal do cabeçalho para comportar os botões visíveis.
    pub header_auto_fit: bool,
    /// Valor fixado em automação *Latch* enquanto a reprodução continua.
    latch_hold: Option<(usize, EnvKind, f32)>,
    /// Altura de cada trilha antes de Minimizar/Maximizar (p. 388).
    pub(crate) height_restore: HashMap<Id, f32>,
    /// Pares sobrepostos existentes antes do início de um arraste (para distinguir
    /// sobreposições novas das antigas quando `auto_crossfades` varia).
    drag_prev_overlaps: HashSet<(Id, Id)>,
}

impl Default for TimelineState {
    fn default() -> Self {
        TimelineState {
            px_per_sec: 60.0,
            scroll_x: Ticks::ZERO,
            scroll_y: 0.0,
            width: 800.0,
            drag: Drag::None,
            ctx_target: CtxTarget::None,
            rename: None,
            snap_line: None,
            drop_hint: None,
            drop_context_menu: None,
            kf_collapsed: HashSet::new(),
            layers_expanded: HashSet::new(),
            layer_row: HashMap::new(),
            box_cur: None,
            time_sel_origin: None,
            suppress_ctx_menu: false,
            last_doc_bar: Rect::NOTHING,
            last_seq_bar: Rect::NOTHING,
            last_lanes: Rect::NOTHING,
            last_rows: Vec::new(),
            header_w: HEADER_W,
            header_prev_w: HEADER_W,
            header_auto_fit: true,
            latch_hold: None,
            height_restore: HashMap::new(),
            drag_prev_overlaps: HashSet::new(),
        }
    }
}

impl TimelineState {
    /// Zoom mínimo (afastado): as 48 h de um projeto (`MAX_SECONDS`) cabem em
    /// cerca de 35 px.
    pub const MIN_PX_PER_SEC: f32 = 2.0e-4;
    /// Zoom máximo (aproximado): cerca de 4 ticks por pixel, onde o tempo
    /// exato ainda separa cada pixel; bem além de uma amostra por pixel.
    pub const MAX_PX_PER_SEC: f32 = (neoveg_engine::time::TICKS_PER_SEC / 4) as f32;

    pub const MIN_HEADER_W: f32 = 48.0;
    pub const COLLAPSED_HEADER_W: f32 = 72.0;
    pub const MAX_HEADER_W: f32 = 420.0;

    /// Ajusta a largura dos cabeçalhos de trilha dentro dos limites seguros (§0.4).
    pub fn set_header_w(&mut self, w: f32) {
        if w.is_finite() {
            self.header_w = w.clamp(Self::MIN_HEADER_W, Self::MAX_HEADER_W);
        }
    }

    /// Lista de trilhas recolhida (largura mínima dos cabeçalhos).
    pub fn header_collapsed(&self) -> bool {
        self.header_w <= 104.0
    }

    /// Duplo clique na borda do cabeçalho alterna entre largura recolhida e expandida (p. 69).
    pub fn toggle_header_collapse(&mut self) {
        if self.header_collapsed() {
            self.header_w = self.header_prev_w.clamp(180.0, Self::MAX_HEADER_W);
            self.header_auto_fit = true;
        } else {
            self.header_prev_w = self.header_w;
            self.header_w = Self::COLLAPSED_HEADER_W;
            self.header_auto_fit = false;
        }
    }

    /// Zoom dentro dos limites; valor inválido (NaN, infinito, <= 0) volta ao
    /// padrão (§0.4).
    pub fn clamp_pps(v: f32) -> f32 {
        if v.is_finite() && v > 0.0 {
            v.clamp(Self::MIN_PX_PER_SEC, Self::MAX_PX_PER_SEC)
        } else {
            60.0
        }
    }

    fn dx_to_ticks(&self, dx: f32) -> Ticks {
        Ticks::from_secs_f64_clamped((dx / self.px_per_sec.max(Self::MIN_PX_PER_SEC)) as f64)
    }

    pub fn zoom_by(&mut self, f: f32, anchor: Option<Ticks>) {
        let anchor = anchor.unwrap_or(self.scroll_x + self.dx_to_ticks(self.width / 2.0));
        let ax = (anchor - self.scroll_x).as_secs_f64() as f32 * self.px_per_sec;
        self.px_per_sec = Self::clamp_pps(self.px_per_sec * f);
        self.scroll_x = (anchor - self.dx_to_ticks(ax)).max(Ticks::ZERO);
    }

    /// Zoom a partir da agulha de reprodução: se ela está na tela, fica no
    /// mesmo pixel; se não, o zoom acontece e a agulha vai para o centro.
    pub fn zoom_around_playhead(&mut self, f: f32, playhead: Ticks) {
        let vis_end = self.scroll_x + self.dx_to_ticks(self.width);
        if playhead >= self.scroll_x && playhead <= vis_end {
            self.zoom_by(f, Some(playhead));
        } else {
            self.zoom_by(f, None);
            self.center_on(playhead);
        }
    }

    pub fn center_on(&mut self, t: Ticks) {
        self.scroll_x = (t - self.dx_to_ticks(self.width / 2.0)).max(Ticks::ZERO);
    }

    /// Rolagem automática durante a reprodução (por páginas).
    pub fn follow(&mut self, t: Ticks) {
        self.follow_with(t, false);
    }

    /// Rolagem automática durante a reprodução (por páginas ou contínua).
    pub fn follow_with(&mut self, t: Ticks, continuous: bool) {
        let vis = self.dx_to_ticks(self.width);
        if continuous {
            let half = Ticks::from_secs_f64_clamped(vis.as_secs_f64() * 0.5);
            self.scroll_x = (t - half).max(Ticks::ZERO);
            return;
        }
        let far = self.scroll_x + Ticks::from_secs_f64_clamped(vis.as_secs_f64() * 0.97);
        if t > far || t < self.scroll_x {
            let pad = Ticks::from_secs_f64_clamped(vis.as_secs_f64() * 0.03);
            self.scroll_x = (t - pad).max(Ticks::ZERO);
        }
    }

    fn t2x(&self, t: Ticks, left: f32) -> f32 {
        left + ((t - self.scroll_x).as_secs_f64() * self.px_per_sec as f64) as f32
    }

    fn x2t(&self, x: f32, left: f32) -> Ticks {
        self.scroll_x + self.dx_to_ticks(x - left)
    }
}

fn shade(c: [u8; 3], k: f32) -> Color32 {
    Color32::from_rgb(
        (c[0] as f32 * k).min(255.0) as u8,
        (c[1] as f32 * k).min(255.0) as u8,
        (c[2] as f32 * k).min(255.0) as u8,
    )
}

/// Teto de marcas da régua e linhas da grade por quadro.
const MAX_RULER_MARKS: u32 = 4096;

fn nice_step(pps: f32, fps: FrameRate) -> (Ticks, Ticks) {
    let f1 = fps.ticks_per_frame();
    let steps: [(Ticks, i64); 17] = [
        (f1, 1),
        (f1.saturating_mul(2), 2),
        (f1.saturating_mul(5), 5),
        (Ticks::from_millis_i64(250), 5),
        (Ticks::from_millis_i64(500), 5),
        (Ticks::from_secs_i64(1), 5),
        (Ticks::from_secs_i64(2), 4),
        (Ticks::from_secs_i64(5), 5),
        (Ticks::from_secs_i64(10), 5),
        (Ticks::from_secs_i64(15), 5),
        (Ticks::from_secs_i64(30), 6),
        (Ticks::from_secs_i64(60), 6),
        (Ticks::from_secs_i64(120), 4),
        (Ticks::from_secs_i64(300), 5),
        (Ticks::from_secs_i64(600), 6),
        (Ticks::from_secs_i64(1800), 6),
        (Ticks::from_secs_i64(3600), 6),
    ];
    for (s, divs) in steps {
        if s >= f1 && s.as_secs_f64() * pps as f64 >= 90.0 {
            let minor = s.div_i64(divs).max(f1);
            return (s, minor);
        }
    }
    // acima de 1 h: 2 h, 4 h, 8 h... até caber (o tempo satura em 48 h)
    let mut s = Ticks::from_secs_i64(7200);
    for _ in 0..48 {
        if s.as_secs_f64() * pps as f64 >= 90.0 {
            break;
        }
        s = s.saturating_mul(2);
    }
    (s, s.div_i64(6).max(f1))
}

fn effective_grid_step(
    grid: neoveg_engine::session::RulerGridSpacing,
    pps: f32,
    settings: &ProjectSettings,
    width_px: f32,
) -> (Ticks, Ticks) {
    if let Some(mut step) = grid.step_ticks(settings) {
        let view_secs = (f64::from(width_px.max(200.0))
            / f64::from(pps.max(TimelineState::MIN_PX_PER_SEC)))
        .min(neoveg_engine::time::MAX_SECONDS as f64);
        let max_marks = f64::from(MAX_RULER_MARKS / 2);
        while view_secs / step.as_secs_f64().max(1e-6) > max_marks {
            step = step.saturating_mul(2);
            if step >= Ticks::MAX.div_i64(2) {
                break;
            }
        }
        return (step, step);
    }
    nice_step(pps, settings.fps)
}

const BUS_TRACK_H: f32 = 58.0;
/// Altura mínima de uma trilha (Minimizar, p. 388).
pub(crate) const TRACK_MIN_H: f32 = 28.0;
/// Teto de Maximizar: a área das faixas, limitada para não gerar camadas
/// gigantes em monitores muito altos.
const TRACK_MAX_FILL_H: f32 = 2000.0;

impl App {
    fn timeline_rows(&self, lanes_top: f32) -> TimelineRows {
        let mut y = lanes_top - self.tl.scroll_y;
        let video_bus_rect = if self.opts.show_video_bus_track {
            let r = (y, BUS_TRACK_H);
            y += BUS_TRACK_H;
            Some(r)
        } else {
            None
        };
        let n = self.project.tracks.len();
        let mut track_rects = Vec::with_capacity(n);
        let mut track_spans = Vec::with_capacity(n);
        let mut kf_areas = Vec::with_capacity(n);
        for t in &self.project.tracks {
            if self.project.is_track_hidden_by_folder(t.id) {
                track_rects.push((y, 0.0));
                track_spans.push((y, 0.0));
                kf_areas.push(None);
                continue;
            }
            let kf = self.kf_visible_rows(t);
            let kf_h = kf.len() as f32 * KF_ROW_H;
            track_rects.push((y, t.height));
            track_spans.push((y, t.height + kf_h));
            kf_areas.push(if kf.is_empty() {
                None
            } else {
                Some(KfArea {
                    top: y + t.height,
                    rows: kf,
                })
            });
            y += t.height + kf_h;
        }
        let bus_rects = if self.opts.show_bus_tracks {
            self.project
                .audio_buses
                .iter()
                .map(|_| {
                    let r = (y, BUS_TRACK_H);
                    y += BUS_TRACK_H;
                    r
                })
                .collect()
        } else {
            Vec::new()
        };
        TimelineRows {
            video_bus_rect,
            track_rects,
            track_spans,
            kf_areas,
            bus_rects,
        }
    }

    fn owner_envelopes(&self, owner: EnvOwner) -> Option<&[Envelope]> {
        match owner {
            EnvOwner::Track(ti) => self.project.tracks.get(ti).map(|t| t.envelopes.as_slice()),
            EnvOwner::Bus(bi) => self
                .project
                .audio_buses
                .get(bi)
                .map(|b| b.envelopes.as_slice()),
            EnvOwner::MasterVideo => Some(self.project.master.video_envelopes.as_slice()),
        }
    }

    fn owner_envelopes_mut(&mut self, owner: EnvOwner) -> Option<&mut Vec<Envelope>> {
        match owner {
            EnvOwner::Track(ti) => self.project.tracks.get_mut(ti).map(|t| &mut t.envelopes),
            EnvOwner::Bus(bi) => self
                .project
                .audio_buses
                .get_mut(bi)
                .map(|b| &mut b.envelopes),
            EnvOwner::MasterVideo => Some(&mut self.project.master.video_envelopes),
        }
    }

    fn track_at(&self, y: f32, layout: &[(f32, f32)]) -> Option<usize> {
        layout.iter().position(|(y0, h)| y >= *y0 && y < y0 + h)
    }

    fn env_y(kind: EnvKind, v: f32, y0: f32, h: f32) -> f32 {
        y0 + h - 4.0 - kind.to_norm(v) * (h - 8.0)
    }

    fn event_rect(&self, e: &Event, left: f32, y0: f32, h: f32) -> Rect {
        // camadas expandidas: o evento ocupa o terço do rolo A (em cima) ou
        // do rolo B (embaixo); o terço do meio é o rolo de transição
        let (top, bottom) = match self.tl.layer_row.get(&e.id) {
            Some(0) => (y0 + 1.0, y0 + h / 3.0),
            Some(_) => (y0 + h * 2.0 / 3.0, y0 + h - 1.0),
            None => (y0 + 1.0, y0 + h - 1.0),
        };
        Rect::from_min_max(
            pos2(self.tl.t2x(e.start, left), top),
            pos2(self.tl.t2x(e.end(), left), bottom.max(top + 1.0)),
        )
    }

    fn gain_handle_y(r: Rect, gain_db: f32) -> f32 {
        // Na semântica do VEGAS Pro, 0 dB fica no topo do evento de áudio e
        // arrastar para baixo atenua até -36 dB.
        let t = (-gain_db / 36.0).clamp(0.0, 1.0);
        let span = (r.height() - 10.0).max(0.0);
        (r.top() + 5.0 + t * span).clamp(r.top() + 4.0, (r.bottom() - 4.0).max(r.top() + 4.0))
    }

    fn opacity_handle_y(r: Rect, opacity: f32) -> f32 {
        // Na semântica do VEGAS Pro, 100% de opacidade fica no topo do evento
        // de vídeo e arrastar para baixo reduz até 0%.
        let t = (1.0 - opacity.clamp(0.0, 1.0)).clamp(0.0, 1.0);
        let span = (r.height() - 10.0).max(0.0);
        (r.top() + 5.0 + t * span).clamp(r.top() + 4.0, (r.bottom() - 4.0).max(r.top() + 4.0))
    }

    /// Calcula a largura horizontal necessária para exibir todos os botões ativos
    /// nas faixas de áudio e vídeo sem cortar nenhum controle.
    pub fn required_header_w_for_buttons(&self) -> f32 {
        let btns_width =
            |is_video: bool, has_kf: bool, has_extra_badge: bool, depth: usize| -> f32 {
                let b = self.prefs.track_header.get(is_video);
                // Gutter lateral (26 px) + margem direita (16 px) + recuo de hierarquia + campo de nome mínimo (72 px)
                let mut w = 26.0 + 16.0 + (depth.min(4) as f32) * 8.0 + 72.0;
                if has_extra_badge {
                    w += 26.0;
                }
                if b.mute {
                    w += 24.0;
                }
                if b.solo {
                    w += 24.0;
                }
                if b.fx {
                    w += 30.0;
                }
                // Selo de automação (Off/Read/Touch/Latch)
                w += 42.0;
                if is_video {
                    if b.track_motion {
                        w += 24.0;
                    }
                    if b.compositing_child {
                        w += 24.0;
                    }
                } else if b.record_arm {
                    w += 24.0;
                }
                if b.height {
                    w += 46.0;
                }
                if has_kf {
                    w += 24.0;
                }
                w
            };
        let mut req = 180.0_f32;
        let mut saw_video = false;
        let mut saw_audio = false;
        for t in &self.project.tracks {
            let is_video = t.kind == TrackKind::Video;
            if is_video {
                saw_video = true;
            } else {
                saw_audio = true;
            }
            let depth = self.project.track_depth(t.id);
            let has_kf = is_video
                && !neoveg_engine::track_keyframes::track_rows(t, self.kf_tol()).is_empty();
            let has_badge = t.is_adjustment()
                || (is_video && self.project.has_track_children(t.id))
                || (is_video && t.mask_mode != neoveg_engine::model::TrackMaskMode::None)
                || t.multicam;
            req = req.max(btns_width(is_video, has_kf, has_badge, depth));
        }
        if !saw_video {
            req = req.max(btns_width(true, false, false, 0));
        }
        if !saw_audio {
            req = req.max(btns_width(false, false, false, 0));
        }
        req.clamp(180.0, TimelineState::MAX_HEADER_W)
    }

    /// Ajusta a largura horizontal do cabeçalho das faixas para acomodar os
    /// botões atualmente visíveis.
    pub fn sync_header_width_to_buttons(&mut self) {
        self.tl.header_auto_fit = true;
        if !self.tl.header_collapsed() {
            let req = self.required_header_w_for_buttons();
            self.tl.set_header_w(req);
        }
    }

    fn event_button_hidden(&self, is_video: bool, key: &str) -> bool {
        let list = if is_video {
            &self.prefs.event_hidden_buttons_video
        } else {
            &self.prefs.event_hidden_buttons_audio
        };
        list.iter().any(|k| k == key)
    }

    fn event_buttons(
        &self,
        r: &Rect,
        is_video: bool,
    ) -> (Option<Rect>, Option<Rect>, Option<Rect>) {
        if r.width() < 54.0 {
            return (None, None, None);
        }
        // No layout VEGAS Pro, os botões do evento ficam no canto inferior direito
        // quando a faixa tem altura confortável, recuando para o topo em faixas mínimas.
        let y = if r.height() >= 34.0 {
            (r.bottom() - 16.0).max(r.top() + 2.0)
        } else {
            r.top() + 2.0
        };
        let mut rx = r.right() - 4.0;
        let more = if r.width() >= 54.0 {
            rx -= 14.0;
            let b = Rect::from_min_size(pos2(rx, y), vec2(14.0, 13.0));
            rx -= 2.0;
            Some(b)
        } else {
            None
        };
        let fx = if !self.event_button_hidden(is_video, "fx") && r.width() >= 70.0 {
            rx -= 16.0;
            let b = Rect::from_min_size(pos2(rx, y), vec2(16.0, 13.0));
            rx -= 2.0;
            Some(b)
        } else {
            None
        };
        let pc = if is_video && !self.event_button_hidden(is_video, "pan_crop") && r.width() >= 88.0
        {
            rx -= 16.0;
            Some(Rect::from_min_size(pos2(rx, y), vec2(16.0, 13.0)))
        } else {
            None
        };
        (fx, pc, more)
    }

    fn hit_crossfade(
        &self,
        pos: Pos2,
        left: f32,
        layout: &[(f32, f32)],
    ) -> Option<(usize, neoveg_engine::crossfade::CrossfadeRegion)> {
        let ti = self.track_at(pos.y, layout)?;
        let (y0, h) = layout[ti];
        let track = self.project.tracks.get(ti)?;
        for xf in neoveg_engine::crossfade::track_crossfades(track, true)
            .into_iter()
            .rev()
        {
            let x0 = self.tl.t2x(xf.start, left);
            let x1 = self.tl.t2x(xf.end, left);
            if x1 - x0 < 2.0 {
                continue;
            }
            let xr = Rect::from_min_max(pos2(x0, y0 + 1.0), pos2(x1, y0 + h - 1.0));
            if xr.contains(pos) {
                return Some((ti, xf));
            }
        }
        None
    }

    fn hit_event(
        &self,
        pos: Pos2,
        left: f32,
        layout: &[(f32, f32)],
    ) -> Option<(usize, usize, Zone)> {
        let ti = self.track_at(pos.y, layout)?;
        let (y0, h) = layout[ti];
        let track = &self.project.tracks[ti];
        for strict in [true, false] {
            for (ei, e) in track.events.iter().enumerate().rev() {
                let r = self.event_rect(e, left, y0, h);
                let hit_rect = if strict {
                    r
                } else {
                    r.expand2(vec2(EDGE_PX, 0.0))
                };
                if !hit_rect.contains(pos) {
                    continue;
                }
                let (fxb, pcb, moreb) = self.event_buttons(&r, track.kind == TrackKind::Video);
                if moreb.map(|b| b.contains(pos)).unwrap_or(false) {
                    return Some((ti, ei, Zone::MoreButton));
                }
                if fxb.map(|b| b.contains(pos)).unwrap_or(false) {
                    return Some((ti, ei, Zone::FxButton));
                }
                if pcb.map(|b| b.contains(pos)).unwrap_or(false) {
                    return Some((ti, ei, Zone::PanCropButton));
                }
                let fi_x = r.left() + (e.fade_in.len.as_secs_f64() as f32 * self.tl.px_per_sec);
                let fo_x = r.right() - (e.fade_out.len.as_secs_f64() as f32 * self.tl.px_per_sec);
                if pos.y < r.top() + 10.0 {
                    if (pos.x - fi_x).abs() < 6.0 && pos.x < r.center().x + 6.0 {
                        return Some((ti, ei, Zone::FadeIn));
                    }
                    if (pos.x - fo_x).abs() < 6.0 && pos.x > r.center().x - 6.0 {
                        return Some((ti, ei, Zone::FadeOut));
                    }
                }
                if r.width() > 24.0 && r.height() >= 18.0 {
                    if track.kind == TrackKind::Audio {
                        let gy = Self::gain_handle_y(r, e.gain_db);
                        if (pos.y - gy).abs() <= 7.5 && pos.x > fi_x + 6.0 && pos.x < fo_x - 6.0 {
                            return Some((ti, ei, Zone::GainHandle));
                        }
                    } else if track.kind == TrackKind::Video {
                        let oy = Self::opacity_handle_y(r, e.opacity);
                        if (pos.y - oy).abs() <= 7.5 && pos.x > fi_x + 6.0 && pos.x < fo_x - 6.0 {
                            return Some((ti, ei, Zone::OpacityHandle));
                        }
                    }
                }
                if (pos.x - r.left()).abs() <= EDGE_PX && r.width() > 12.0 {
                    return Some((ti, ei, Zone::LeftEdge));
                }
                if (pos.x - r.right()).abs() <= EDGE_PX {
                    return Some((ti, ei, Zone::RightEdge));
                }
                if r.contains(pos) {
                    return Some((ti, ei, Zone::Body));
                }
            }
        }
        None
    }

    fn hit_envelope(
        &self,
        pos: Pos2,
        left: f32,
        rows: &TimelineRows,
    ) -> Option<(EnvOwner, EnvKind, Option<usize>, Ticks)> {
        if !self.opts.show_envelopes {
            return None;
        }
        let owner = rows.owner_at(pos.y)?;
        let (y0, h) = rows.row_rect(owner)?;
        let t = self.tl.x2t(pos.x, left);
        for env in self.owner_envelopes(owner)?.iter().filter(|e| e.visible) {
            for (i, p) in env.points.iter().enumerate() {
                let px = self.tl.t2x(p.t, left);
                let py = Self::env_y(env.kind, p.v, y0, h);
                if (px - pos.x).abs() < 6.0 && (py - pos.y).abs() < 6.0 {
                    return Some((owner, env.kind, Some(i), t));
                }
            }
            let ly = Self::env_y(env.kind, env.eval(t), y0, h);
            if (ly - pos.y).abs() < 4.0 {
                return Some((owner, env.kind, None, t));
            }
        }
        None
    }

    /// Encaixe (snapping) em cursor, marcadores, regiões e bordas de eventos.
    fn snap(&mut self, t: Ticks, exclude: &HashSet<Id>, shift: bool) -> Ticks {
        let mut t = t.max(Ticks::ZERO);
        self.tl.snap_line = None;
        let enabled = self.opts.snapping != shift;
        if enabled {
            let tol = self.tl.dx_to_ticks(8.0).max(Ticks::from_raw(1));
            let mut best: Option<(Ticks, Ticks)> = None;
            let mut consider = |c: Ticks| {
                let d = c.abs_diff(t);
                if d <= tol && best.map(|b| d < b.0).unwrap_or(true) {
                    best = Some((d, c));
                }
            };
            consider(Ticks::ZERO);
            consider(self.cursor);
            for m in &self.project.markers {
                consider(m.t);
            }
            for r in &self.project.regions {
                consider(r.start);
                consider(r.end);
            }
            if let Some((a, b)) = self.project.loop_region {
                consider(a);
                consider(b);
            }
            for tr in &self.project.tracks {
                for e in &tr.events {
                    if !exclude.contains(&e.id) {
                        consider(e.start);
                        consider(e.end());
                    }
                }
            }
            if self.opts.snap_to_grid {
                let (_, gminor) = effective_grid_step(
                    self.opts.ruler_grid,
                    self.tl.px_per_sec,
                    &self.project.settings,
                    self.tl.width,
                );
                let gstep = gminor.0.max(1);
                let g0 = (t.0 / gstep).saturating_mul(gstep);
                let g1 = g0.saturating_add(gstep);
                consider(Ticks::from_raw(g0));
                consider(Ticks::from_raw(g1));
            }
            if let Some((_, c)) = best {
                self.tl.snap_line = Some(c);
                return c;
            }
        }
        if self.opts.quantize {
            t = self.project.settings.quantize(t);
        }
        t
    }

    pub fn timeline_ui(&mut self, ui: &mut egui::Ui) {
        self.project.ensure_sequences();
        let full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, Sense::hover());
        // Clicar na timeline devolve o foco de teclado do Trimmer para a timeline (VEGAS Pro).
        if ui.input(|i| {
            (i.pointer.primary_pressed() || i.pointer.secondary_pressed())
                && i.pointer.interact_pos().is_some_and(|p| full.contains(p))
        }) {
            self.trimmer.has_focus = false;
        }
        // Faixa de projetos abertos no topo e faixa de sequências logo abaixo dela.
        let show_doc_tabs = full.height() > TOP_H + BOTTOM_BARS_H + DOC_TAB_H + 36.0;
        let doc_bar_h = if show_doc_tabs { DOC_TAB_H } else { 0.0 };
        self.tl.last_doc_bar = if show_doc_tabs {
            let doc_bar_rect = Rect::from_min_size(full.min, vec2(full.width(), DOC_TAB_H));
            self.document_tab_bar_ui(ui, doc_bar_rect);
            doc_bar_rect
        } else {
            Rect::NOTHING
        };
        let classic_workflow = self.prefs.workflow_mode == neoveg_common::WorkflowMode::Classic;
        let show_seq_tabs = !classic_workflow
            && full.height() > TOP_H + BOTTOM_BARS_H + doc_bar_h + SEQ_TAB_H + 36.0;
        let seq_bar_h = if show_seq_tabs { SEQ_TAB_H } else { 0.0 };
        self.tl.last_seq_bar = if show_seq_tabs {
            let seq_bar_rect = Rect::from_min_size(
                pos2(full.left(), full.top() + doc_bar_h),
                vec2(full.width(), SEQ_TAB_H),
            );
            self.sequence_tab_bar_ui(ui, seq_bar_rect);
            seq_bar_rect
        } else {
            Rect::NOTHING
        };
        let top_bars_h = doc_bar_h + seq_bar_h;
        let content_top = full.top() + top_bars_h;

        if self.tl.header_auto_fit && !self.tl.header_collapsed() {
            let req = self.required_header_w_for_buttons();
            if self.tl.header_w < req {
                self.tl.set_header_w(req);
            }
        }
        let max_hdr =
            (full.width() - 96.0).clamp(TimelineState::MIN_HEADER_W, TimelineState::MAX_HEADER_W);
        let hdr_w = self.tl.header_w.clamp(TimelineState::MIN_HEADER_W, max_hdr);
        let left = full.left() + hdr_w;
        let show_h_col = full.width() > hdr_w + crate::scrollbars::TIMELINE_V_SCROLL_W + 72.0;
        let show_footer = full.height() > top_bars_h + TOP_H + BOTTOM_BARS_H + 40.0;
        let bottom_h = if show_footer { BOTTOM_BARS_H } else { 0.0 };
        let show_v_col = show_h_col && full.height() > top_bars_h + TOP_H + bottom_h + 60.0;
        let v_col_w = if show_v_col {
            crate::scrollbars::TIMELINE_V_SCROLL_W
        } else {
            0.0
        };
        let lanes_right = (full.right() - v_col_w).max(left + 10.0);
        let lanes_bottom = (full.bottom() - bottom_h).max(content_top + TOP_H + 10.0);

        let corner = Rect::from_min_max(
            pos2(full.left(), content_top),
            pos2(left, content_top + TOP_H),
        );
        let top = Rect::from_min_max(
            pos2(left, content_top),
            pos2(lanes_right, content_top + TOP_H),
        );
        let headers = Rect::from_min_max(
            pos2(full.left(), content_top + TOP_H),
            pos2(left, lanes_bottom),
        );
        let lanes = Rect::from_min_max(
            pos2(left, content_top + TOP_H),
            pos2(lanes_right, lanes_bottom),
        );
        self.tl.width = lanes.width().max(10.0);

        // limite de rolagem vertical
        let vbus_h: f32 = if self.opts.show_video_bus_track {
            BUS_TRACK_H
        } else {
            0.0
        };
        let bus_h: f32 = vbus_h
            + if self.opts.show_bus_tracks {
                self.project.audio_buses.len() as f32 * BUS_TRACK_H
            } else {
                0.0
            };
        let total_h: f32 = self
            .project
            .tracks
            .iter()
            .map(|t| t.height + self.kf_visible_rows(t).len() as f32 * KF_ROW_H)
            .sum::<f32>()
            + bus_h;
        self.tl.scroll_y = self
            .tl
            .scroll_y
            .clamp(0.0, (total_h - lanes.height() + 40.0).max(0.0));

        self.time_display_ui(ui, corner);
        self.top_strip(ui, top);
        if show_v_col {
            let cap = Rect::from_min_max(
                pos2(lanes_right, content_top),
                pos2(full.right(), lanes.top()),
            );
            let p = ui.painter();
            p.rect_filled(cap, egui::CornerRadius::ZERO, self.theme.tl_ruler_bg);
            p.rect_stroke(
                cap,
                egui::CornerRadius::ZERO,
                Stroke::new(1.0_f32, self.theme.border),
                StrokeKind::Inside,
            );
        }

        let rows = self.timeline_rows(lanes.top());
        self.lanes_interact(ui, lanes, &rows);
        let rows = self.timeline_rows(lanes.top());
        self.draw_lanes(ui, lanes, &rows);
        self.draw_keyframe_rows(ui, lanes, &rows);
        // Desenha o retângulo de seleção/zoom e o destaque de SplitTrim ACIMA das
        // trilhas e eventos para não ser encoberto pelo fundo opaco das faixas.
        self.draw_lane_drag_overlays(ui, lanes, &rows);
        self.headers_ui(ui, headers, &rows);
        self.draw_keyframe_labels(ui, headers, &rows);

        // Divisor interativo da largura dos cabeçalhos de trilha (p. 69):
        // arrastar oculta progressivamente botões; duplo clique alterna largura.
        let edge_r = Rect::from_min_max(
            pos2(left - 3.0, content_top),
            pos2(left + 3.0, lanes.bottom()),
        );
        let edge_resp = ui.interact(
            edge_r,
            ui.id().with("tl_header_edge"),
            Sense::click_and_drag(),
        );
        edge_resp.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                "Divisor de largura dos cabeçalhos",
            )
        });
        if edge_resp.hovered() || edge_resp.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
            ui.painter().line_segment(
                [pos2(left, content_top), pos2(left, lanes.bottom())],
                Stroke::new(2.0_f32, self.theme.accent),
            );
        }
        if edge_resp.dragged() {
            let dx = edge_resp.drag_delta().x;
            self.tl.header_auto_fit = false;
            self.tl.set_header_w(self.tl.header_w + dx);
        }
        if edge_resp.double_clicked() {
            self.tl.toggle_header_collapse();
        }
        edge_resp.on_hover_text(
            "Arraste para redimensionar os cabeçalhos (duplo clique para recolher/expandir)",
        );

        // Automação Latch: sustenta o último valor gravado durante a reprodução.
        if self.is_playing() {
            if let Some((lti, lkind, lval)) = self.tl.latch_hold {
                let is_latch = self
                    .project
                    .tracks
                    .get(lti)
                    .is_some_and(|t| t.automation_mode == AutomationMode::Latch);
                if is_latch {
                    let pt = self.play_pos();
                    let tol = self.project.settings.frame_dur().div_i64(2);
                    self.session
                        .record_automation_point(lti, lkind, pt, lval, tol);
                } else {
                    self.tl.latch_hold = None;
                }
            }
        } else {
            self.tl.latch_hold = None;
        }

        if show_v_col {
            let v_col = Rect::from_min_max(
                pos2(lanes_right, lanes.top()),
                pos2(full.right(), lanes.bottom()),
            );
            self.timeline_v_scrollbar_ui(ui, v_col, total_h, bus_h, lanes.height());
        }

        if show_footer {
            let footer_bottom = (lanes.bottom() + TRACK_FOOTER_H).min(full.bottom());
            let track_footer_r =
                Rect::from_min_max(pos2(full.left(), lanes.bottom()), pos2(left, footer_bottom));
            let h_scroll_r =
                Rect::from_min_max(pos2(left, lanes.bottom()), pos2(lanes_right, footer_bottom));
            let corner_r = show_v_col.then(|| {
                Rect::from_min_max(
                    pos2(lanes_right, lanes.bottom()),
                    pos2(full.right(), footer_bottom),
                )
            });
            let render_bar_r = Rect::from_min_max(pos2(full.left(), footer_bottom), full.max);
            self.track_list_footer_ui(ui, track_footer_r);
            self.timeline_h_scrollbar_ui(ui, h_scroll_r, corner_r, lanes.width());
            self.render_status_bar_ui(ui, render_bar_r);
        }
        self.draw_cursors(ui, top.union(lanes));
        self.rename_popup(ui);
        self.show_drop_context_menu(ui);
    }

    fn effective_box_rect(&self, raw: Rect, lanes: Rect) -> Rect {
        match self.session.box_axis {
            BoxAxisMode::Both => raw,
            BoxAxisMode::Horizontal => Rect::from_min_max(
                pos2(raw.left(), lanes.top()),
                pos2(raw.right(), lanes.bottom()),
            ),
            BoxAxisMode::Vertical => Rect::from_min_max(
                pos2(lanes.left(), raw.top()),
                pos2(lanes.right(), raw.bottom()),
            ),
        }
    }

    /// Fim da caixa das ferramentas Seleção e Zoom (manual do VEGAS Pro 21,
    /// pp. 239–240). Seleção: seleciona os eventos que a caixa toca (Shift ou
    /// Ctrl somam à seleção). Zoom: o trecho da caixa ocupa a largura (ou
    /// altura das trilhas no eixo vertical) da linha do tempo.
    fn finish_box(
        &mut self,
        rect: Rect,
        zoom: bool,
        lanes: Rect,
        layout: &[(f32, f32)],
        mods: egui::Modifiers,
    ) {
        let left = lanes.left();
        let axis = self.session.box_axis;
        let rect = self.effective_box_rect(rect, lanes);
        if zoom {
            if axis == BoxAxisMode::Vertical {
                if rect.height() < 8.0 {
                    return;
                }
                let hit_tracks: Vec<usize> = layout
                    .iter()
                    .enumerate()
                    .filter_map(|(ti, &(y0, h))| {
                        (y0 + h >= rect.top() && y0 <= rect.bottom()).then_some(ti)
                    })
                    .collect();
                if !hit_tracks.is_empty() {
                    let target_h = (lanes.height() / hit_tracks.len() as f32).clamp(32.0, 220.0);
                    for ti in hit_tracks {
                        if let Some(tr) = self.project.tracks.get_mut(ti) {
                            tr.height = target_h;
                        }
                    }
                }
                return;
            }
            // caixa muito estreita é um clique (tratado em `clicked`)
            if rect.width() < 4.0 {
                return;
            }
            let t0 = self.tl.x2t(rect.left().max(left), left);
            let t1 = self.tl.x2t(rect.right().min(lanes.right()), left);
            if t1 <= t0 {
                return;
            }
            let secs = (t1 - t0).as_secs_f64() as f32;
            self.tl.px_per_sec = TimelineState::clamp_pps(lanes.width() / secs);
            self.tl.scroll_x = t0;
            return;
        }
        let mut hits: Vec<Id> = Vec::new();
        for (ti, tr) in self.project.tracks.iter().enumerate() {
            let Some(&(y0, h)) = layout.get(ti) else {
                continue;
            };
            if axis == BoxAxisMode::Vertical && (y0 + h < rect.top() || y0 > rect.bottom()) {
                continue;
            }
            for e in &tr.events {
                if axis == BoxAxisMode::Vertical || self.event_rect(e, left, y0, h).intersects(rect)
                {
                    hits.push(e.id);
                }
            }
        }
        if !(mods.shift || mods.command) {
            self.sel_events.clear();
        }
        self.sel_events.extend(hits);
    }

    /// Retângulo de um evento na tela no último quadro (testes de interface).
    #[cfg(test)]
    pub(crate) fn event_screen_rect(&self, id: Id) -> Option<Rect> {
        let (ti, ei) = self.project.find_event(id)?;
        let &(y0, h) = self.tl.last_rows.get(ti)?;
        let e = self.project.tracks.get(ti)?.events.get(ei)?;
        Some(self.event_rect(e, self.tl.last_lanes.left(), y0, h))
    }

    fn fit_project_to_timeline(&mut self, lanes_width: f32) {
        let mut target_secs = self.project.duration().as_secs_f64() as f32;
        if target_secs <= 0.05 {
            if let Some((a, b)) = self.time_sel {
                if b > a {
                    target_secs = (b - a).as_secs_f64() as f32;
                }
            }
        }
        if target_secs.is_finite() && target_secs > 0.05 {
            let pps = TimelineState::clamp_pps((lanes_width - 24.0).max(40.0) / target_secs);
            self.tl.px_per_sec = pps;
        } else {
            self.tl.px_per_sec = 60.0;
        }
        self.tl.scroll_x = Ticks::ZERO;
    }

    fn fit_tracks_vertically(&mut self, lanes_height: f32, bus_h: f32) {
        if self.project.tracks.is_empty() {
            self.tl.scroll_y = 0.0;
            return;
        }
        let n = self.project.tracks.len() as f32;
        let avail = (lanes_height - bus_h - 4.0).max(28.0 * n);
        let h = (avail / n).clamp(28.0, 120.0);
        for tr in &mut self.project.tracks {
            tr.height = h;
        }
        self.tl.scroll_y = 0.0;
    }

    /// Minimizar (`maximize = false`) ou Maximizar a altura da trilha `ti`
    /// (manual do VEGAS Pro 21, p. 388). Clicar de novo no mesmo botão volta
    /// à altura de antes. Maximizar ocupa a área das faixas do último quadro.
    pub(crate) fn toggle_track_height(&mut self, ti: usize, maximize: bool) {
        let Some((id, cur)) = self.project.tracks.get(ti).map(|t| (t.id, t.height)) else {
            return;
        };
        let lanes_h = self.tl.last_lanes.height();
        let target = if maximize {
            if lanes_h.is_finite() && lanes_h > TRACK_MIN_H {
                lanes_h.min(TRACK_MAX_FILL_H)
            } else {
                400.0
            }
        } else {
            TRACK_MIN_H
        };
        let next = match self.tl.height_restore.get(&id).copied() {
            Some(prev) if (cur - target).abs() < 0.5 => {
                self.tl.height_restore.remove(&id);
                prev.clamp(TRACK_MIN_H, TRACK_MAX_FILL_H)
            }
            _ => {
                // guarda a altura "normal" só na primeira vez (minimizar e
                // depois maximizar ainda volta para a altura original)
                self.tl.height_restore.entry(id).or_insert(cur);
                target
            }
        };
        if let Some(t) = self.project.tracks.get_mut(ti) {
            t.height = next;
        }
    }

    fn adjust_all_track_heights(&mut self, delta: f32) {
        if !delta.is_finite() || delta == 0.0 {
            return;
        }
        for tr in &mut self.project.tracks {
            tr.height = (tr.height + delta).clamp(28.0, 400.0);
        }
    }

    fn timeline_v_scrollbar_ui(
        &mut self,
        ui: &mut egui::Ui,
        col: Rect,
        total_h: f32,
        bus_h: f32,
        lanes_h: f32,
    ) {
        let p = ui.painter();
        p.rect_filled(col, egui::CornerRadius::ZERO, self.theme.panel_fill);
        p.vline(
            col.left(),
            col.top()..=col.bottom(),
            Stroke::new(1.0_f32, self.theme.border),
        );

        let show_track_zoom = !self.project.tracks.is_empty() && col.height() >= 96.0;
        let zoom_pair_h = if show_track_zoom { 34.0 } else { 0.0 };
        let scroll_r = Rect::from_min_max(col.min, pos2(col.right(), col.bottom() - zoom_pair_h));

        let v_content = (total_h + 40.0).max(lanes_h);
        let v_range = crate::scrollbars::ScrollbarRange {
            offset: self.tl.scroll_y,
            content: v_content,
            viewport: lanes_h,
        };
        let lang = self.lang();
        let step = (lanes_h * 0.18).clamp(28.0, 84.0);
        let res = crate::scrollbars::arrow_scrollbar(
            ui,
            egui::Id::new("tl_v_scroll"),
            scroll_r,
            crate::scrollbars::Axis::Vertical,
            v_range,
            step,
            tr(lang, "Rolar trilhas para cima"),
            tr(lang, "Rolar trilhas para baixo"),
            &self.theme,
        );
        if let Some(off) = res.new_offset {
            self.tl.scroll_y = off.clamp(0.0, (total_h - lanes_h + 40.0).max(0.0));
        }
        if res.double_clicked {
            self.fit_tracks_vertically(lanes_h, bus_h);
        }

        if show_track_zoom {
            let zoom_top = scroll_r.bottom() + 2.0;
            let btn_h = ((col.bottom() - zoom_top) * 0.5).max(12.0);
            let inc_r = Rect::from_min_max(
                pos2(col.left(), zoom_top),
                pos2(col.right(), zoom_top + btn_h),
            );
            let dec_r = Rect::from_min_max(
                pos2(col.left(), zoom_top + btn_h),
                pos2(col.right(), col.bottom()),
            );

            let can_inc = self.project.tracks.iter().any(|t| t.height < 399.5);
            let can_dec = self.project.tracks.iter().any(|t| t.height > 28.5);

            let (inc_click, inc_drag) = crate::scrollbars::zoom_button(
                ui,
                egui::Id::new("tl_track_h_plus"),
                inc_r,
                crate::scrollbars::SymbolGlyph::Plus,
                crate::scrollbars::Axis::Vertical,
                can_inc,
                tr(lang, "Aumentar altura das trilhas"),
                tr(
                    lang,
                    "Aumentar altura das trilhas (clique ou arraste verticalmente)",
                ),
                &self.theme,
            );
            let (dec_click, dec_drag) = crate::scrollbars::zoom_button(
                ui,
                egui::Id::new("tl_track_h_minus"),
                dec_r,
                crate::scrollbars::SymbolGlyph::Minus,
                crate::scrollbars::Axis::Vertical,
                can_dec,
                tr(lang, "Diminuir altura das trilhas"),
                tr(
                    lang,
                    "Diminuir altura das trilhas (clique ou arraste verticalmente)",
                ),
                &self.theme,
            );

            if inc_click {
                self.adjust_all_track_heights(10.0);
            }
            if dec_click {
                self.adjust_all_track_heights(-10.0);
            }
            let drag = inc_drag + dec_drag;
            if drag.abs() > 0.01 {
                self.adjust_all_track_heights(drag);
            }
        }
    }

    fn timeline_h_scrollbar_ui(
        &mut self,
        ui: &mut egui::Ui,
        row: Rect,
        corner: Option<Rect>,
        lanes_w: f32,
    ) {
        let p = ui.painter();
        p.rect_filled(row, egui::CornerRadius::ZERO, self.theme.panel_fill);
        p.hline(
            row.left()..=row.right(),
            row.top(),
            Stroke::new(1.0_f32, self.theme.border),
        );

        let bar_h = 18.0_f32.min((row.height() - 2.0).max(12.0));
        let bar_top = (row.center().y - bar_h * 0.5).max(row.top() + 1.0);
        let bar_bottom = (bar_top + bar_h).min(row.bottom() - 1.0);
        let inner = Rect::from_min_max(
            pos2(row.left() + 2.0, bar_top),
            pos2((row.right() - 2.0).max(row.left() + 8.0), bar_bottom),
        );

        let show_zoom = inner.width() >= 96.0;
        let btn_w = bar_h;
        let zoom_w = if show_zoom { btn_w * 2.0 + 3.0 } else { 0.0 };
        let scroll_r = Rect::from_min_max(
            inner.min,
            pos2(
                (inner.right() - zoom_w).max(inner.left() + 12.0),
                inner.bottom(),
            ),
        );

        let view_secs =
            (lanes_w / self.tl.px_per_sec.max(TimelineState::MIN_PX_PER_SEC)).max(1.0e-9);
        let proj_secs = self.project.duration().as_secs_f64() as f32;
        let mut max_end_secs = proj_secs.max(self.cursor.as_secs_f64() as f32);
        if let Some((a, b)) = self.time_sel {
            if b > a {
                max_end_secs = max_end_secs.max(b.as_secs_f64() as f32);
            }
        }
        let cur_scroll_secs = self.tl.scroll_x.as_secs_f64().max(0.0) as f32;
        let content_secs = (max_end_secs + (view_secs * 0.35).max(10.0))
            .max(view_secs * 2.0)
            .max(cur_scroll_secs + view_secs);

        let h_range = crate::scrollbars::ScrollbarRange {
            offset: cur_scroll_secs,
            content: content_secs,
            viewport: view_secs,
        };
        let lang = self.lang();
        let step_secs = (view_secs * 0.15).max(self.tl.dx_to_ticks(48.0).as_secs_f64() as f32);
        let res = crate::scrollbars::arrow_scrollbar(
            ui,
            egui::Id::new("tl_h_scroll"),
            scroll_r,
            crate::scrollbars::Axis::Horizontal,
            h_range,
            step_secs,
            tr(lang, "Rolar timeline para a esquerda"),
            tr(lang, "Rolar timeline para a direita"),
            &self.theme,
        );
        if let Some(secs) = res.new_offset {
            self.tl.scroll_x = Ticks::from_secs_f64_clamped(secs.max(0.0) as f64);
        }
        if res.double_clicked {
            self.fit_project_to_timeline(lanes_w);
        }

        if show_zoom {
            let zoom_left = scroll_r.right() + 3.0;
            let dec_r = Rect::from_min_max(
                pos2(zoom_left, inner.top()),
                pos2(zoom_left + btn_w, inner.bottom()),
            );
            let inc_r = Rect::from_min_max(
                pos2(zoom_left + btn_w, inner.top()),
                pos2(inner.right(), inner.bottom()),
            );
            let playhead = self.cursor;
            let can_out = self.tl.px_per_sec > TimelineState::MIN_PX_PER_SEC * 1.0001;
            let can_in = self.tl.px_per_sec < TimelineState::MAX_PX_PER_SEC * 0.9999;

            let (dec_click, dec_drag) = crate::scrollbars::zoom_button(
                ui,
                egui::Id::new("tl_h_zoom_out"),
                dec_r,
                crate::scrollbars::SymbolGlyph::Minus,
                crate::scrollbars::Axis::Horizontal,
                can_out,
                tr(lang, "Diminuir zoom horizontal"),
                tr(
                    lang,
                    "Afastar zoom horizontal (clique ou arraste horizontalmente)",
                ),
                &self.theme,
            );
            let (inc_click, inc_drag) = crate::scrollbars::zoom_button(
                ui,
                egui::Id::new("tl_h_zoom_in"),
                inc_r,
                crate::scrollbars::SymbolGlyph::Plus,
                crate::scrollbars::Axis::Horizontal,
                can_in,
                tr(lang, "Aumentar zoom horizontal"),
                tr(
                    lang,
                    "Aproximar zoom horizontal (clique ou arraste horizontalmente)",
                ),
                &self.theme,
            );

            if dec_click {
                self.tl.zoom_around_playhead(0.8, playhead);
            }
            if inc_click {
                self.tl.zoom_around_playhead(1.25, playhead);
            }
            let drag = dec_drag + inc_drag;
            if drag.abs() > 0.01 {
                let factor = (1.0 + drag * 0.012).clamp(0.5, 2.0);
                self.tl.zoom_around_playhead(factor, playhead);
            }
        }

        if let Some(c_rect) = corner {
            let cp = ui.painter();
            cp.rect_filled(c_rect, egui::CornerRadius::ZERO, self.theme.panel_fill);
            cp.rect_stroke(
                c_rect,
                egui::CornerRadius::ZERO,
                Stroke::new(1.0_f32, self.theme.border),
                StrokeKind::Inside,
            );
            let fit_btn_r = Rect::from_min_max(
                pos2(c_rect.left() + 1.0, bar_top),
                pos2(c_rect.right() - 1.0, bar_bottom),
            );
            let (fit_click, _) = crate::scrollbars::zoom_button(
                ui,
                egui::Id::new("tl_fit_project"),
                fit_btn_r,
                crate::scrollbars::SymbolGlyph::Fit,
                crate::scrollbars::Axis::Horizontal,
                true,
                tr(lang, "Ajustar projeto à timeline"),
                tr(
                    lang,
                    "Ajustar todo o projeto à largura visível da timeline (ou duplo clique na barra horizontal)",
                ),
                &self.theme,
            );
            if fit_click {
                self.fit_project_to_timeline(lanes_w);
            }
        }
    }

    /// Faixa de projetos abertos (projeto principal e projetos `.neoveg` aninhados
    /// abertos em aba), sempre posicionada acima da faixa de sequências.
    fn document_tab_bar_ui(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let lang = self.lang();
        let p = ui.painter();
        p.rect_filled(rect, egui::CornerRadius::ZERO, self.theme.extreme_bg);
        p.line_segment(
            [
                pos2(rect.left(), rect.bottom() - 0.5),
                pos2(rect.right(), rect.bottom() - 0.5),
            ],
            Stroke::new(1.0_f32, self.theme.border),
        );

        let font_id = FontId::proportional(11.0);
        let tab_h = (rect.height() - 3.0).max(14.0);
        let mut cur_x = rect.left() + 5.0;

        // Rótulo discreto identificando a faixa de projetos abertos
        let hdr_lbl = tr(lang, "Projetos");
        let hdr_galley = p.layout_no_wrap(
            hdr_lbl.to_string(),
            FontId::proportional(10.0),
            self.theme.text_muted,
        );
        let icon_r = Rect::from_center_size(pos2(cur_x + 6.0, rect.center().y), vec2(11.0, 11.0));
        icons::paint(p, icon_r, Icon::OpenFolder, &self.theme, false);
        p.galley(
            pos2(
                icon_r.right() + 3.0,
                rect.center().y - hdr_galley.size().y * 0.5,
            ),
            hdr_galley.clone(),
            self.theme.text_muted,
        );
        cur_x = icon_r.right() + 3.0 + hdr_galley.size().x + 8.0;
        p.line_segment(
            [
                pos2(cur_x - 4.0, rect.top() + 4.0),
                pos2(cur_x - 4.0, rect.bottom() - 4.0),
            ],
            Stroke::new(1.0_f32, self.theme.border),
        );

        let classic_workflow = self.prefs.workflow_mode == neoveg_common::WorkflowMode::Classic;
        let seq_tabs = self.project.sequence_tab_info();
        let primary_seq_id = seq_tabs.first().map(|(id, _, _)| *id);
        let primary_seq_active = seq_tabs.first().is_none_or(|(_, _, active)| *active);

        let mut doc_switch = None;
        let mut doc_close = None;
        let mut seq_switch: Option<Id> = None;
        let mut seq_delete: Option<Id> = None;
        let mut seq_duplicate: Option<Id> = None;
        let mut seq_rename: Option<(Id, String)> = None;
        for (di, dname, dactive, ddirty) in self.doc_bar_entries() {
            let tab_active = if classic_workflow && dactive {
                primary_seq_active
            } else {
                dactive
            };
            let label = if ddirty { format!("{dname}*") } else { dname };
            let text_w = ui
                .painter()
                .layout_no_wrap(label.clone(), font_id.clone(), Color32::WHITE)
                .size()
                .x;
            let close_w = if di > 0 { 16.0 } else { 0.0 };
            let tab_w = (text_w + 22.0 + close_w).clamp(72.0, 220.0);
            if cur_x + tab_w > rect.right() - 28.0 {
                break;
            }
            let tab_rect = Rect::from_min_size(pos2(cur_x, rect.top() + 2.0), vec2(tab_w, tab_h));
            let resp = ui
                .interact(tab_rect, ui.id().with(("doc_tab", di)), Sense::click())
                .on_hover_text(if di == 0 {
                    tr(lang, "Projeto principal")
                } else {
                    tr(lang, "Projeto aninhado aberto em aba (Ctrl+S salva todos)")
                });
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
            let bg = if tab_active {
                self.theme.tl_header_bg
            } else if resp.hovered() {
                self.theme.widget_hover_bg
            } else {
                self.theme.panel_fill
            };
            let radius = egui::CornerRadius {
                nw: 3,
                ne: 3,
                sw: 0,
                se: 0,
            };
            p.rect_filled(tab_rect, radius, bg);
            p.rect_stroke(
                tab_rect,
                radius,
                Stroke::new(
                    1.0_f32,
                    if tab_active {
                        self.theme.accent
                    } else {
                        self.theme.border
                    },
                ),
                StrokeKind::Inside,
            );
            // marca lateral do documento
            p.rect_filled(
                Rect::from_min_size(
                    tab_rect.left_top() + vec2(1.0, 3.0),
                    vec2(3.0, (tab_h - 6.0).max(4.0)),
                ),
                egui::CornerRadius::ZERO,
                self.theme.accent,
            );
            let text_color = if tab_active {
                self.theme.text_primary
            } else {
                self.theme.text_muted
            };
            p.text(
                pos2(tab_rect.left() + 9.0, tab_rect.center().y),
                Align2::LEFT_CENTER,
                &label,
                font_id.clone(),
                text_color,
            );
            let mut closing = false;
            if di > 0 {
                let close_rect = Rect::from_center_size(
                    pos2(tab_rect.right() - 10.0, tab_rect.center().y),
                    vec2(12.0, 12.0),
                );
                let close_resp = ui
                    .interact(close_rect, ui.id().with(("doc_close", di)), Sense::click())
                    .on_hover_text(tr(lang, "Fechar a aba"));
                p.text(
                    close_rect.center(),
                    Align2::CENTER_CENTER,
                    "×",
                    FontId::proportional(12.0),
                    if close_resp.hovered() {
                        self.theme.offline_fg
                    } else {
                        self.theme.text_muted
                    },
                );
                if close_resp.clicked() {
                    doc_close = Some(di);
                    closing = true;
                }
            }
            if resp.clicked() && !closing {
                if !dactive {
                    doc_switch = Some(di);
                } else if classic_workflow && !primary_seq_active {
                    seq_switch = primary_seq_id;
                }
            }
            cur_x += tab_w + 3.0;
        }

        // No fluxo Clássico, sequências adicionais do projeto atual aparecem na
        // própria faixa de projetos abertos como projetos internos aninhados.
        if classic_workflow {
            let seq_suffix = format!(".{}", neoveg_engine::package::SEQUENCE_EXT);
            for (seq_id, seq_name, is_active) in seq_tabs.iter().skip(1) {
                let seq_label = seq_name
                    .strip_suffix(&seq_suffix)
                    .unwrap_or(seq_name)
                    .to_string();
                let text_w = ui
                    .painter()
                    .layout_no_wrap(seq_label.clone(), font_id.clone(), Color32::WHITE)
                    .size()
                    .x;
                let tab_w = (text_w + 38.0).clamp(80.0, 220.0);
                if cur_x + tab_w > rect.right() - 28.0 {
                    break;
                }
                let tab_rect =
                    Rect::from_min_size(pos2(cur_x, rect.top() + 2.0), vec2(tab_w, tab_h));
                let tab_resp = ui
                    .interact(
                        tab_rect,
                        ui.id().with(("classic_seq_tab", *seq_id)),
                        Sense::click(),
                    )
                    .on_hover_text(tr(
                        lang,
                        "Projeto interno (sequência aninhada dentro do projeto atual)",
                    ));
                tab_resp.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &seq_label)
                });
                let bg = if *is_active {
                    self.theme.tl_header_bg
                } else if tab_resp.hovered() {
                    self.theme.widget_hover_bg
                } else {
                    self.theme.panel_fill
                };
                let radius = egui::CornerRadius {
                    nw: 3,
                    ne: 3,
                    sw: 0,
                    se: 0,
                };
                p.rect_filled(tab_rect, radius, bg);
                p.rect_stroke(
                    tab_rect,
                    radius,
                    Stroke::new(
                        1.0_f32,
                        if *is_active {
                            self.theme.accent
                        } else {
                            self.theme.border
                        },
                    ),
                    StrokeKind::Inside,
                );
                let ico_r = Rect::from_center_size(
                    pos2(tab_rect.left() + 10.0, tab_rect.center().y),
                    vec2(10.0, 10.0),
                );
                icons::paint(p, ico_r, Icon::NestedTimeline, &self.theme, *is_active);
                let text_color = if *is_active {
                    self.theme.text_primary
                } else {
                    self.theme.text_muted
                };
                p.text(
                    pos2(ico_r.right() + 4.0, tab_rect.center().y),
                    Align2::LEFT_CENTER,
                    &seq_label,
                    font_id.clone(),
                    text_color,
                );
                let close_rect = Rect::from_center_size(
                    pos2(tab_rect.right() - 10.0, tab_rect.center().y),
                    vec2(12.0, 12.0),
                );
                let close_resp = ui
                    .interact(
                        close_rect,
                        ui.id().with(("classic_seq_close", *seq_id)),
                        Sense::click(),
                    )
                    .on_hover_text(tr(lang, "Excluir sequência"));
                p.text(
                    close_rect.center(),
                    Align2::CENTER_CENTER,
                    "×",
                    FontId::proportional(12.0),
                    if close_resp.hovered() {
                        self.theme.offline_fg
                    } else {
                        self.theme.text_muted
                    },
                );
                let mut closing = false;
                if close_resp.clicked() {
                    seq_delete = Some(*seq_id);
                    closing = true;
                }
                tab_resp.context_menu(|ui| {
                    if ui.button(tr(lang, "Renomear")).clicked() {
                        seq_rename = Some((*seq_id, seq_name.clone()));
                        ui.close_menu();
                    }
                    if ui.button(tr(lang, "Duplicar sequência")).clicked() {
                        seq_duplicate = Some(*seq_id);
                        ui.close_menu();
                    }
                    if ui.button(tr(lang, "Excluir sequência")).clicked() {
                        seq_delete = Some(*seq_id);
                        ui.close_menu();
                    }
                });
                if tab_resp.clicked() && !closing && !*is_active {
                    seq_switch = Some(*seq_id);
                }
                if tab_resp.double_clicked() {
                    seq_rename = Some((*seq_id, seq_name.clone()));
                }
                cur_x += tab_w + 3.0;
            }

            if cur_x + 22.0 <= rect.right() - 4.0 {
                let add_rect =
                    Rect::from_min_size(pos2(cur_x, rect.top() + 3.0), vec2(20.0, tab_h - 2.0));
                let add_resp = ui
                    .interact(
                        add_rect,
                        ui.id().with("classic_seq_add_btn"),
                        Sense::click(),
                    )
                    .on_hover_text(tr(lang, "Novo projeto interno (sequência aninhada)"));
                let add_bg = if add_resp.hovered() {
                    self.theme.widget_hover_bg
                } else {
                    self.theme.widget_bg
                };
                p.rect_filled(add_rect, egui::CornerRadius::same(2), add_bg);
                p.rect_stroke(
                    add_rect,
                    egui::CornerRadius::same(2),
                    Stroke::new(1.0_f32, self.theme.border),
                    StrokeKind::Inside,
                );
                p.text(
                    add_rect.center(),
                    Align2::CENTER_CENTER,
                    "+",
                    font_id,
                    self.theme.text_primary,
                );
                if add_resp.clicked() {
                    self.create_sequence_app(None);
                }
            }
        }

        if let Some(i) = doc_close {
            self.request_close_doc_tab(i);
        } else if let Some(i) = doc_switch {
            self.switch_doc_tab(i);
        } else if let Some(target_id) = seq_switch {
            self.switch_sequence_app(target_id);
        } else if let Some(target_id) = seq_delete {
            self.delete_sequence(target_id);
        } else if let Some(source_id) = seq_duplicate {
            self.duplicate_sequence(source_id);
        } else if let Some((id, name)) = seq_rename {
            self.tl.rename = Some((id, name));
        }
    }

    /// Barra de abas de sequências na timeline, logo abaixo da faixa de projetos abertos.
    fn sequence_tab_bar_ui(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let lang = self.lang();
        let p = ui.painter();
        p.rect_filled(rect, egui::CornerRadius::ZERO, self.theme.panel_fill);
        p.line_segment(
            [
                pos2(rect.left(), rect.bottom() - 0.5),
                pos2(rect.right(), rect.bottom() - 0.5),
            ],
            Stroke::new(1.0_f32, self.theme.border),
        );

        let font_id = FontId::proportional(11.0);
        let tab_h = (rect.height() - 3.0).max(14.0);
        let mut cur_x = rect.left() + 5.0;

        let hdr_lbl = tr(lang, "Sequências");
        let hdr_galley = p.layout_no_wrap(
            hdr_lbl.to_string(),
            FontId::proportional(10.0),
            self.theme.text_muted,
        );
        let icon_r = Rect::from_center_size(pos2(cur_x + 6.0, rect.center().y), vec2(11.0, 11.0));
        icons::paint(p, icon_r, Icon::NestedTimeline, &self.theme, false);
        p.galley(
            pos2(
                icon_r.right() + 3.0,
                rect.center().y - hdr_galley.size().y * 0.5,
            ),
            hdr_galley.clone(),
            self.theme.text_muted,
        );
        cur_x = icon_r.right() + 3.0 + hdr_galley.size().x + 8.0;
        p.line_segment(
            [
                pos2(cur_x - 4.0, rect.top() + 4.0),
                pos2(cur_x - 4.0, rect.bottom() - 4.0),
            ],
            Stroke::new(1.0_f32, self.theme.border),
        );

        let mut action_switch: Option<Id> = None;
        let mut action_delete: Option<Id> = None;
        let mut action_duplicate: Option<Id> = None;
        let mut action_rename: Option<(Id, String)> = None;

        let tabs = self.project.sequence_tab_info();
        let seq_count = tabs.len();

        for (seq_id, seq_name, is_active) in tabs {
            // exibe apenas "Sequência XX" na aba (sem extensão .nvegseq)
            let seq_label = seq_name
                .strip_suffix(&format!(".{}", neoveg_engine::package::SEQUENCE_EXT))
                .unwrap_or(&seq_name)
                .to_string();
            let text_w = ui
                .painter()
                .layout_no_wrap(seq_label.clone(), font_id.clone(), Color32::WHITE)
                .size()
                .x;
            let close_w = if seq_count > 1 { 16.0 } else { 0.0 };
            let tab_w = (text_w + 20.0 + close_w).clamp(70.0, 220.0);

            if cur_x + tab_w > rect.right() - 32.0 {
                break;
            }

            let tab_rect = Rect::from_min_size(pos2(cur_x, rect.top() + 3.0), vec2(tab_w, tab_h));
            let tab_resp = ui
                .interact(tab_rect, ui.id().with(("seq_tab", seq_id)), Sense::click())
                .on_hover_text(&seq_label);

            let bg = if is_active {
                self.theme.tl_header_bg
            } else if tab_resp.hovered() {
                self.theme.widget_hover_bg
            } else {
                self.theme.panel_fill
            };

            p.rect_filled(
                tab_rect,
                egui::CornerRadius {
                    nw: 3,
                    ne: 3,
                    sw: 0,
                    se: 0,
                },
                bg,
            );
            let border_color = if is_active {
                self.theme.accent
            } else {
                self.theme.border
            };
            p.rect_stroke(
                tab_rect,
                egui::CornerRadius {
                    nw: 3,
                    ne: 3,
                    sw: 0,
                    se: 0,
                },
                Stroke::new(1.0_f32, border_color),
                StrokeKind::Inside,
            );

            if is_active {
                p.line_segment(
                    [
                        pos2(tab_rect.left() + 1.0, tab_rect.top() + 1.0),
                        pos2(tab_rect.right() - 1.0, tab_rect.top() + 1.0),
                    ],
                    Stroke::new(2.0_f32, self.theme.accent),
                );
            }

            let text_color = if is_active {
                self.theme.text_primary
            } else {
                self.theme.text_muted
            };
            let max_text_w = tab_w - 12.0 - close_w;
            let text_pos = pos2(tab_rect.left() + 8.0, tab_rect.center().y);
            let label_clipped = if text_w > max_text_w {
                format!("{}…", seq_label.chars().take(18).collect::<String>())
            } else {
                seq_label.clone()
            };
            p.text(
                text_pos,
                Align2::LEFT_CENTER,
                label_clipped,
                font_id.clone(),
                text_color,
            );

            if seq_count > 1 {
                let close_rect = Rect::from_center_size(
                    pos2(tab_rect.right() - 10.0, tab_rect.center().y),
                    vec2(12.0, 12.0),
                );
                let close_resp = ui.interact(
                    close_rect,
                    ui.id().with(("seq_close", seq_id)),
                    Sense::click(),
                );
                let close_color = if close_resp.hovered() {
                    self.theme.offline_fg
                } else {
                    self.theme.text_muted
                };
                p.text(
                    close_rect.center(),
                    Align2::CENTER_CENTER,
                    "×",
                    FontId::proportional(12.0),
                    close_color,
                );
                if close_resp.clicked() {
                    action_delete = Some(seq_id);
                }
            }

            tab_resp.context_menu(|ui| {
                if ui.button(tr(self.lang(), "Renomear")).clicked() {
                    action_rename = Some((seq_id, seq_name.clone()));
                    ui.close_menu();
                }
                if ui.button(tr(self.lang(), "Duplicar sequência")).clicked() {
                    action_duplicate = Some(seq_id);
                    ui.close_menu();
                }
                if seq_count > 1 && ui.button(tr(self.lang(), "Excluir sequência")).clicked() {
                    action_delete = Some(seq_id);
                    ui.close_menu();
                }
            });

            if tab_resp.clicked() && !is_active {
                action_switch = Some(seq_id);
            }
            if tab_resp.double_clicked() {
                action_rename = Some((seq_id, seq_name.clone()));
            }

            cur_x += tab_w + 3.0;
        }

        let add_rect = Rect::from_min_size(pos2(cur_x, rect.top() + 4.0), vec2(20.0, tab_h - 2.0));
        let add_resp = ui.interact(add_rect, ui.id().with("seq_add_btn"), Sense::click());
        let add_bg = if add_resp.hovered() {
            self.theme.widget_hover_bg
        } else {
            self.theme.widget_bg
        };
        p.rect_filled(add_rect, egui::CornerRadius::same(2), add_bg);
        p.rect_stroke(
            add_rect,
            egui::CornerRadius::same(2),
            Stroke::new(1.0_f32, self.theme.border),
            StrokeKind::Inside,
        );
        p.text(
            add_rect.center(),
            Align2::CENTER_CENTER,
            "+",
            font_id,
            self.theme.text_primary,
        );
        if add_resp.clicked() {
            self.create_sequence_app(None);
        }

        if let Some(target_id) = action_switch {
            self.switch_sequence_app(target_id);
        }
        if let Some(target_id) = action_delete {
            self.delete_sequence(target_id);
        }
        if let Some(source_id) = action_duplicate {
            self.duplicate_sequence(source_id);
        }
        if let Some((id, name)) = action_rename {
            self.tl.rename = Some((id, name));
        }
    }

    /// Visor de Tempo dedicado acima da lista de faixas de áudio/vídeo, alinhado à régua e barra de marcadores.
    fn time_display_ui(&mut self, ui: &mut egui::Ui, r: Rect) {
        let painter = ui.painter_at(r);
        painter.rect_filled(r, 0.0, self.theme.tl_header_bg);
        painter.line_segment(
            [r.left_bottom(), r.right_bottom()],
            Stroke::new(1.0_f32, self.theme.border),
        );
        painter.line_segment(
            [r.right_top(), r.right_bottom()],
            Stroke::new(1.0_f32, self.theme.border),
        );

        let box_r = r.shrink2(vec2(5.0, 4.0));
        let layout_type = self.prefs.edit_layout.layout_type;
        let corner =
            if self.theme.high_contrast || layout_type == neoveg_common::LayoutType::SonicFoundry {
                0.0
            } else {
                2.0
            };
        painter.rect_filled(box_r, corner, self.theme.extreme_bg);
        if !self.theme.high_contrast && layout_type == neoveg_common::LayoutType::SonicFoundry {
            // Moldura rebaixada 3D clássica do Visor de Tempo (Vegas 1.0)
            painter.hline(
                box_r.left()..=box_r.right(),
                box_r.top() + 0.5,
                Stroke::new(1.5_f32, self.theme.border),
            );
            painter.vline(
                box_r.left() + 0.5,
                box_r.top()..=box_r.bottom(),
                Stroke::new(1.5_f32, self.theme.border),
            );
            painter.hline(
                box_r.left()..=box_r.right(),
                box_r.bottom() - 0.5,
                Stroke::new(1.0_f32, Color32::from_white_alpha(210)),
            );
            painter.vline(
                box_r.right() - 0.5,
                box_r.top()..=box_r.bottom(),
                Stroke::new(1.0_f32, Color32::from_white_alpha(210)),
            );
        } else {
            painter.rect_stroke(
                box_r,
                corner,
                Stroke::new(1.0_f32, self.theme.border),
                StrokeKind::Inside,
            );
            if !self.theme.high_contrast && layout_type == neoveg_common::LayoutType::Moderno {
                // Friso vertical de destaque discreto (visual renovado v23+)
                let stripe = Rect::from_min_max(
                    pos2(box_r.left() + 1.0, box_r.top() + 2.0),
                    pos2(box_r.left() + 3.5, box_r.bottom() - 2.0),
                );
                painter.rect_filled(stripe, 1.0, self.theme.accent);
            }
        }

        let lang = self.lang();
        painter.text(
            pos2(box_r.left() + 6.0, box_r.top() + 3.0),
            Align2::LEFT_TOP,
            tr(lang, "Visor de tempo"),
            crate::fonts::semibold_font(9.5),
            self.theme.text_muted,
        );
        painter.text(
            pos2(box_r.right() - 6.0, box_r.top() + 3.0),
            Align2::RIGHT_TOP,
            self.opts.time_format.name_in(lang),
            FontId::proportional(9.5),
            self.theme.text_muted,
        );
        crate::fonts::paint_tabular_text(
            &painter,
            pos2(box_r.right() - 8.0, box_r.bottom() - 3.0),
            Align2::RIGHT_BOTTOM,
            &self.fmt_time(self.view_time()),
            crate::fonts::time_font(ui.ctx(), 20.0),
            self.theme.timecode_fg,
        );

        let resp = ui
            .interact(box_r, ui.id().with("timeline_time_display"), Sense::click())
            .on_hover_text(tr(
                lang,
                "Visor de tempo (duplo clique: Ir para tempo • clique direito: formato de tempo)",
            ));
        if resp.double_clicked() {
            self.exec_cmd(Some(ui.ctx()), "edit.goto");
        }
        resp.context_menu(|ui| {
            ui.label(
                egui::RichText::new(tr(lang, "Formato do Visor de Tempo"))
                    .small()
                    .strong(),
            );
            ui.separator();
            for fmt in TimeFormat::ALL {
                if ui
                    .selectable_label(self.opts.time_format == fmt, fmt.name_in(lang))
                    .clicked()
                {
                    self.session.opts.time_format = fmt;
                    ui.close_menu();
                }
            }
            ui.separator();
            if ui.button(tr(lang, "Ir para tempo... (Ctrl+G)")).clicked() {
                self.dlg.goto_open = true;
                ui.close_menu();
            }
        });
    }

    /// Barra de Status de Renderização e Pré-renderização na base da timeline.
    fn render_status_bar_ui(&mut self, ui: &mut egui::Ui, r: Rect) {
        let lang = self.lang();
        let painter = ui.painter_at(r);
        painter.rect_filled(r, 0.0, self.theme.tl_header_bg);
        painter.line_segment(
            [r.left_top(), r.right_top()],
            Stroke::new(1.0_f32, self.theme.border),
        );

        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(r.shrink2(vec2(6.0, 2.0)))
                .id_salt("timeline_render_status_bar"),
        );
        child.set_clip_rect(r);

        let mut cancel_render = false;
        let mut cancel_prerender = false;
        let mut clear_prerender = false;
        let mut trigger_prerender = false;
        let mut trigger_render = false;

        child.horizontal(|ui| {
            if let Some(job) = &self.render_job {
                let p = &job.progress;
                let frac = p.fraction().clamp(0.0, 1.0);
                let done = p.done.load(std::sync::atomic::Ordering::Relaxed);
                let total = p.total.load(std::sync::atomic::Ordering::Relaxed);
                let name = job
                    .settings
                    .path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| job.settings.path.display().to_string());
                crate::splash::spinner_inline(ui, 13.0, ui.ctx().input(|i| i.time), &self.theme);
                let done_s = done.to_string();
                let total_s = total.to_string();
                ui.label(
                    egui::RichText::new(tr_args(
                        lang,
                        "Renderizando {name}: {done}/{total}",
                        &[("name", &name), ("done", &done_s), ("total", &total_s)],
                    ))
                    .small()
                    .strong(),
                );
                ui.add(
                    egui::ProgressBar::new(frac)
                        .desired_width(140.0)
                        .show_percentage(),
                );
                if ui.small_button(tr(lang, "Cancelar")).clicked() {
                    cancel_render = true;
                }
            } else if let Some((a, b, prog)) = &self.prerender_job {
                let frac = prog.fraction().clamp(0.0, 1.0);
                crate::splash::spinner_inline(ui, 13.0, ui.ctx().input(|i| i.time), &self.theme);
                let a_s = self.fmt_time(*a);
                let b_s = self.fmt_time(*b);
                ui.label(
                    egui::RichText::new(tr_args(
                        lang,
                        "Pré-renderizando {a} → {b}",
                        &[("a", &a_s), ("b", &b_s)],
                    ))
                    .small()
                    .strong(),
                );
                ui.add(
                    egui::ProgressBar::new(frac)
                        .desired_width(130.0)
                        .show_percentage(),
                );
                if ui.small_button(tr(lang, "Cancelar")).clicked() {
                    cancel_prerender = true;
                }
            } else {
                let pre_count = self.session.prerendered_ranges.len();
                let pre_text = if pre_count > 0 {
                    let count_s = pre_count.to_string();
                    tr_args(
                        lang,
                        "Status de renderização: {n} trecho(s) pré-renderizado(s)",
                        &[("n", &count_s)],
                    )
                } else {
                    tr(
                        lang,
                        "Status de renderização: Pronto (sem trechos pré-renderizados)",
                    )
                    .to_string()
                };
                let pre_col = if pre_count > 0 {
                    self.theme.tl_marker
                } else {
                    self.theme.text_muted
                };
                ui.label(egui::RichText::new(pre_text).small().color(pre_col));
                if ui
                    .small_button(tr(lang, "Pré-render (Shift+M)"))
                    .on_hover_text(tr(
                        lang,
                        "Pré-renderizar região selecionada e marcar no topo da timeline",
                    ))
                    .clicked()
                {
                    trigger_prerender = true;
                }
                if ui
                    .small_button(tr(lang, "Renderizar..."))
                    .on_hover_text(tr(lang, "Abrir janela Renderizar como"))
                    .clicked()
                {
                    trigger_render = true;
                }
                if pre_count > 0
                    && ui
                        .small_button(tr(lang, "Limpar pré-render"))
                        .on_hover_text(tr(lang, "Limpar indicações de pré-render da timeline"))
                        .clicked()
                {
                    clear_prerender = true;
                }
            }
        });

        if cancel_render {
            if let Some(job) = &self.render_job {
                job.progress
                    .cancel
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        if cancel_prerender {
            if let Some((_, _, prog)) = &self.prerender_job {
                prog.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        if clear_prerender {
            self.session.clear_prerendered();
            self.set_status(tr(lang, "Indicações de pré-renderização limpas."));
        }
        if trigger_prerender {
            self.exec_cmd(Some(ui.ctx()), "tools.prerender");
        }
        if trigger_render {
            self.exec_cmd(Some(ui.ctx()), "file.render");
        }
    }

    /// Rodapé da lista de trilhas: botões rápidos de faixa e controle de Taxa (`Playback Rate`, Manual VEGAS Pro 21, p. 68).
    fn track_list_footer_ui(&mut self, ui: &mut egui::Ui, r: Rect) {
        let lang = self.lang();
        let painter = ui.painter_at(r);
        painter.rect_filled(r, 0.0, self.theme.tl_header_bg);
        painter.line_segment(
            [r.left_top(), r.right_top()],
            Stroke::new(1.0_f32, self.theme.border),
        );
        painter.line_segment(
            [r.right_top(), r.right_bottom()],
            Stroke::new(1.0_f32, self.theme.border),
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(r.shrink2(vec2(5.0, 2.0)))
                .id_salt("timeline_track_list_footer"),
        );
        child.set_clip_rect(r);
        child.horizontal_centered(|ui| {
            if icons::small_icon_button(ui, Icon::VideoTrack, None, false, &self.theme)
                .on_hover_text(tr(lang, "Inserir trilha de vídeo (Ctrl+Shift+Q)"))
                .clicked()
            {
                self.checkpoint();
                self.insert_track(TrackKind::Video);
            }
            if icons::small_icon_button(ui, Icon::AudioTrack, None, false, &self.theme)
                .on_hover_text(tr(lang, "Inserir trilha de áudio (Ctrl+Q)"))
                .clicked()
            {
                self.checkpoint();
                self.insert_track(TrackKind::Audio);
            }
            ui.separator();
            self.playback_rate_control(ui, true);
        });
    }

    // -----------------------------------------------------------------
    // Régua, marcadores e região de loop
    // -----------------------------------------------------------------

    fn top_strip(&mut self, ui: &mut egui::Ui, r: Rect) {
        let painter = ui.painter_at(r);
        let loop_r = Rect::from_min_max(r.min, pos2(r.right(), r.top() + LOOP_H));
        let marker_r = Rect::from_min_max(
            pos2(r.left(), loop_r.bottom()),
            pos2(r.right(), loop_r.bottom() + MARKER_H),
        );
        let ruler_r = Rect::from_min_max(pos2(r.left(), marker_r.bottom()), r.max);
        painter.rect_filled(loop_r, 0.0, self.theme.tl_loop_bar);
        painter.rect_filled(marker_r, 0.0, self.theme.tl_marker_bar);
        painter.rect_filled(ruler_r, 0.0, self.theme.tl_ruler_bg);
        let left = r.left();

        // região de loop
        if let Some((a, b)) = self.project.loop_region {
            let lr = Rect::from_min_max(
                pos2(self.tl.t2x(a, left), loop_r.top() + 2.0),
                pos2(self.tl.t2x(b, left), loop_r.bottom() - 2.0),
            );
            let col = if self.opts.loop_playback {
                self.theme.tl_loop_active
            } else {
                self.theme.tl_loop_inactive
            };
            painter.rect_filled(lr, 2.0, col);
            // setas amarelas nas pontas da região de loop (manual do VEGAS Pro 21, p. 207)
            let h = (loop_r.height() - 2.0).clamp(4.0, 8.0);
            let top = loop_r.top() + 1.0;
            let arrow = Color32::from_rgb(240, 210, 40);
            let outline = Stroke::new(1.0_f32, Color32::from_black_alpha(160));
            for (x, dir) in [(lr.left(), 1.0_f32), (lr.right(), -1.0_f32)] {
                if x.is_finite() && x >= r.left() - h && x <= r.right() + h {
                    painter.add(egui::Shape::convex_polygon(
                        vec![pos2(x, top), pos2(x + dir * h, top), pos2(x, top + h)],
                        arrow,
                        outline,
                    ));
                }
            }
        }
        // barra de status de pré-renderização no topo da timeline (VEGAS Pro 21, p. 215)
        for &(pa, pb) in &self.session.prerendered_ranges {
            let pr = Rect::from_min_max(
                pos2(self.tl.t2x(pa, left), loop_r.bottom() - 4.0),
                pos2(self.tl.t2x(pb, left), loop_r.bottom()),
            );
            painter.rect_filled(pr, 0.0, self.theme.tl_marker);
        }
        if let Some((pa, pb, prog)) = &self.prerender_job {
            let frac = prog.fraction().clamp(0.0, 1.0);
            let xa = self.tl.t2x(*pa, left);
            let xb = self.tl.t2x(*pb, left);
            let xm = xa + (xb - xa) * frac;
            painter.rect_filled(
                Rect::from_min_max(pos2(xa, loop_r.bottom() - 4.0), pos2(xb, loop_r.bottom())),
                0.0,
                self.theme.tl_loop_inactive,
            );
            painter.rect_filled(
                Rect::from_min_max(pos2(xa, loop_r.bottom() - 4.0), pos2(xm, loop_r.bottom())),
                0.0,
                self.theme.accent,
            );
        }
        let (major, minor) = effective_grid_step(
            self.opts.ruler_grid,
            self.tl.px_per_sec,
            &self.project.settings,
            r.width(),
        );
        let step = minor.0.max(1);
        let maj = major.0.max(step);
        let half_minor = step / 4;
        let t_start = self.tl.scroll_x;
        let t_end = self.tl.x2t(r.right(), left);
        let mut idx = t_start.0 / step;
        // teto por quadro: nenhum zoom desenha marcas sem fim (§0.5)
        let mut marks = 0u32;
        loop {
            marks += 1;
            if marks > MAX_RULER_MARKS {
                break;
            }
            let t = Ticks::from_raw(idx.saturating_mul(step));
            if t > t_end {
                break;
            }
            let x = self.tl.t2x(t, left);
            let rem = t.0.rem_euclid(maj);
            let is_major = rem <= half_minor || (maj - rem) <= half_minor;
            let h = if is_major { 10.0 } else { 4.0 };
            painter.line_segment(
                [pos2(x, ruler_r.bottom() - h), pos2(x, ruler_r.bottom())],
                Stroke::new(1.0_f32, self.theme.text_muted),
            );
            if is_major {
                crate::fonts::paint_tabular_text(
                    &painter,
                    pos2(x + 3.0, ruler_r.top() + 1.0),
                    Align2::LEFT_TOP,
                    &self.fmt_time(t),
                    crate::fonts::ruler_font(10.0),
                    self.theme.text_primary,
                );
            }
            idx = idx.saturating_add(1);
        }
        // regiões e marcadores
        for reg in &self.project.regions {
            let xa = self.tl.t2x(reg.start, left);
            let xb = self.tl.t2x(reg.end, left);
            let col = self.theme.tl_marker;
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(xa, marker_r.top() + 1.0),
                    pos2(xb, marker_r.top() + 4.0),
                ),
                0.0,
                col.gamma_multiply(0.55),
            );
            painter.rect_filled(
                Rect::from_min_size(pos2(xa, marker_r.top()), vec2(10.0, MARKER_H)),
                0.0,
                col,
            );
            painter.rect_filled(
                Rect::from_min_size(pos2(xb - 10.0, marker_r.top()), vec2(10.0, MARKER_H)),
                0.0,
                col,
            );
            painter.text(
                pos2(xa + 12.0, marker_r.top() + 3.0),
                Align2::LEFT_TOP,
                &reg.label,
                FontId::proportional(11.0),
                self.theme.text_primary,
            );
        }
        let mut sorted: Vec<&Marker> = self.project.markers.iter().collect();
        sorted.sort_by_key(|a| a.t);
        let mut std_n = 0usize;
        let mut cd_tr_n = 0usize;
        let mut cd_ix_n = 0usize;
        for m in &sorted {
            let x = self.tl.t2x(m.t, left);
            let (col, badge, tag_w) = match m.kind {
                MarkerKind::Standard => {
                    std_n += 1;
                    (self.theme.tl_region, format!("{}", std_n % 10), 14.0)
                }
                MarkerKind::CdTrack => {
                    cd_tr_n += 1;
                    (Color32::from_rgb(56, 152, 226), format!("T{cd_tr_n}"), 22.0)
                }
                MarkerKind::CdIndex => {
                    cd_ix_n += 1;
                    (Color32::from_rgb(76, 176, 108), format!("I{cd_ix_n}"), 20.0)
                }
                MarkerKind::Command => (Color32::from_rgb(168, 92, 212), "CMD".to_string(), 28.0),
            };
            painter.rect_filled(
                Rect::from_min_size(pos2(x, marker_r.top()), vec2(tag_w, MARKER_H - 2.0)),
                1.0,
                col,
            );
            painter.text(
                pos2(x + 2.0, marker_r.top() + 2.0),
                Align2::LEFT_TOP,
                badge,
                FontId::proportional(10.0),
                crate::theme::contrast_on(col),
            );
            let display_lbl = if m.kind == MarkerKind::Command && !m.command_type.is_empty() {
                format!("{} [{}]", m.label, m.command_type)
            } else {
                m.label.clone()
            };
            painter.text(
                pos2(x + tag_w + 3.0, marker_r.top() + 2.0),
                Align2::LEFT_TOP,
                display_lbl,
                FontId::proportional(11.0),
                self.theme.tl_region_band,
            );
        }

        // interação
        let resp = ui.interact(r, ui.id().with("topstrip"), Sense::click_and_drag());
        let shift = ui.input(|i| i.modifiers.shift);
        if let Some(pos) = resp.interact_pointer_pos().or(resp.hover_pos()) {
            let t = self.tl.x2t(pos.x, left);
            let hit_marker = self
                .project
                .markers
                .iter()
                .find(|m| {
                    let x = self.tl.t2x(m.t, left);
                    pos.x >= x - 2.0 && pos.x <= x + 14.0
                })
                .map(|m| m.id);
            let hit_region = self.project.regions.iter().find_map(|reg| {
                let xa = self.tl.t2x(reg.start, left);
                let xb = self.tl.t2x(reg.end, left);
                if pos.x >= xa && pos.x <= xa + 10.0 {
                    Some((reg.id, true))
                } else if pos.x >= xb - 10.0 && pos.x <= xb {
                    Some((reg.id, false))
                } else {
                    None
                }
            });
            let in_loop = loop_r.contains(pos);
            let in_marker = marker_r.contains(pos);
            if matches!(self.tl.drag, Drag::Scrub)
                || (matches!(self.tl.drag, Drag::None) && ruler_r.contains(pos))
            {
                self.set_cursor_shape(crate::cursors::NeovegCursor::Scrub);
            } else if matches!(
                self.tl.drag,
                Drag::LoopEdge { .. } | Drag::RegionEdge { .. }
            ) {
                self.set_cursor_shape(crate::cursors::NeovegCursor::MoveX);
            } else if matches!(self.tl.drag, Drag::LoopMove { .. } | Drag::Marker { .. }) {
                self.set_cursor_shape(crate::cursors::NeovegCursor::Move);
            }
            if resp.drag_started() {
                let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
                let ot = self.tl.x2t(origin.x, left);
                let in_loop0 = loop_r.contains(origin);
                let in_marker0 = marker_r.contains(origin);
                if let (true, Some(mid)) = (in_marker0, hit_marker) {
                    self.checkpoint();
                    self.tl.drag = Drag::Marker { id: mid };
                } else if let (true, Some((id, start))) = (in_marker0, hit_region) {
                    self.checkpoint();
                    self.tl.drag = Drag::RegionEdge { id, start };
                } else if in_loop0 {
                    let tol = self.tl.dx_to_ticks(6.0);
                    match self.project.loop_region {
                        Some((a, _)) if ot.abs_diff(a) < tol => {
                            self.tl.drag = Drag::LoopEdge { start: true }
                        }
                        Some((_, b)) if ot.abs_diff(b) < tol => {
                            self.tl.drag = Drag::LoopEdge { start: false }
                        }
                        Some((a, b)) if ot > a && ot < b => {
                            self.tl.drag = Drag::LoopMove {
                                orig: (a, b),
                                t0: ot,
                            }
                        }
                        _ => {
                            let s = self.snap(ot, &HashSet::new(), shift);
                            self.tl.drag = Drag::TimeSel { anchor: s };
                        }
                    }
                } else {
                    self.tl.drag = Drag::Scrub;
                }
            }
            if resp.dragged() {
                let min_len = Ticks::from_millis_i64(10);
                match self.tl.drag.clone() {
                    Drag::Scrub => {
                        let s = self.snap(t, &HashSet::new(), shift);
                        self.set_cursor(s)
                    }
                    Drag::Marker { id } => {
                        let s = self.snap(t, &HashSet::new(), shift);
                        if let Some(m) = self.project.markers.iter_mut().find(|m| m.id == id) {
                            m.t = s;
                        }
                        self.changed();
                    }
                    Drag::RegionEdge { id, start } => {
                        let s = self.snap(t, &HashSet::new(), shift);
                        if let Some(r) = self.project.regions.iter_mut().find(|r| r.id == id) {
                            if start {
                                r.start = s.min(r.end - min_len);
                            } else {
                                r.end = s.max(r.start + min_len);
                            }
                        }
                        self.changed();
                    }
                    Drag::LoopEdge { start } => {
                        let s = self.snap(t, &HashSet::new(), shift);
                        if let Some((a, b)) = self.project.loop_region {
                            let nr = if start {
                                (s.min(b - min_len), b)
                            } else {
                                (a, s.max(a + min_len))
                            };
                            self.project.loop_region = Some(nr);
                            self.time_sel = Some(nr);
                        }
                    }
                    Drag::LoopMove { orig, t0 } => {
                        let d = (t - t0).max(-orig.0);
                        let a = self.snap(orig.0 + d, &HashSet::new(), shift);
                        let nr = (a, a + (orig.1 - orig.0));
                        self.project.loop_region = Some(nr);
                        self.time_sel = Some(nr);
                    }
                    Drag::TimeSel { anchor } => {
                        let s = self.snap(t, &HashSet::new(), shift);
                        let sel = (anchor.min(s), anchor.max(s));
                        self.time_sel = Some(sel);
                        self.project.loop_region = Some(sel);
                    }
                    _ => {}
                }
            }
            if resp.drag_stopped() {
                self.tl.drag = Drag::None;
                self.tl.snap_line = None;
            }
            if resp.clicked() && !in_loop && !in_marker {
                let s = self.snap(t, &HashSet::new(), shift);
                self.set_cursor(s);
            }
            if resp.clicked() && in_marker {
                if let Some(id) = hit_marker {
                    if let Some(m) = self.project.markers.iter().find(|m| m.id == id) {
                        let mt = m.t;
                        self.set_cursor(mt);
                    }
                } else if let Some((id, _)) = hit_region {
                    if let Some(r) = self.project.regions.iter().find(|r| r.id == id) {
                        let sel = (r.start, r.end);
                        self.time_sel = Some(sel);
                        self.project.loop_region = Some(sel);
                        self.set_cursor(sel.0);
                    }
                }
            }
            if resp.double_clicked() && in_marker {
                if let Some(id) = hit_marker {
                    let label = self
                        .project
                        .markers
                        .iter()
                        .find(|m| m.id == id)
                        .map(|m| m.label.clone())
                        .unwrap_or_default();
                    self.tl.rename = Some((id, label));
                } else if let Some((id, _)) = hit_region {
                    let label = self
                        .project
                        .regions
                        .iter()
                        .find(|r| r.id == id)
                        .map(|r| r.label.clone())
                        .unwrap_or_default();
                    self.tl.rename = Some((id, label));
                } else {
                    self.cursor = t;
                    self.insert_marker();
                }
            }
            if resp.double_clicked() && in_loop {
                self.project.loop_region = None;
                self.time_sel = None;
            }
            if resp.secondary_clicked() {
                self.tl.ctx_target = if let Some(id) = hit_marker {
                    CtxTarget::Marker(id)
                } else if let Some((id, _)) = hit_region {
                    CtxTarget::Region(id)
                } else {
                    CtxTarget::None
                };
            }
        }
        let lang = self.lang();
        resp.context_menu(|ui| match self.tl.ctx_target.clone() {
            CtxTarget::Marker(id) => {
                ui.menu_button(tr(lang, "Tipo de marcador"), |ui| {
                    let cur_kind = self
                        .project
                        .markers
                        .iter()
                        .find(|m| m.id == id)
                        .map(|m| m.kind)
                        .unwrap_or_default();
                    for k in MarkerKind::ALL {
                        if ui.radio(cur_kind == k, k.label()).clicked() {
                            self.checkpoint();
                            if let Some(m) = self.project.markers.iter_mut().find(|m| m.id == id) {
                                m.kind = k;
                            }
                            self.changed();
                            ui.close_menu();
                        }
                    }
                });
                if ui.button(tr(lang, "Renomear marcador")).clicked() {
                    let label = self
                        .project
                        .markers
                        .iter()
                        .find(|m| m.id == id)
                        .map(|m| m.label.clone())
                        .unwrap_or_default();
                    self.tl.rename = Some((id, label));
                    ui.close_menu();
                }
                if ui.button(tr(lang, "Excluir marcador")).clicked() {
                    self.checkpoint();
                    self.project.markers.retain(|m| m.id != id);
                    self.changed();
                    ui.close_menu();
                }
            }
            CtxTarget::Region(id) => {
                if ui.button(tr(lang, "Renomear região")).clicked() {
                    let label = self
                        .project
                        .regions
                        .iter()
                        .find(|m| m.id == id)
                        .map(|m| m.label.clone())
                        .unwrap_or_default();
                    self.tl.rename = Some((id, label));
                    ui.close_menu();
                }
                if ui.button(tr(lang, "Selecionar região")).clicked() {
                    if let Some((start, end)) = self
                        .session
                        .project
                        .regions
                        .iter()
                        .find(|r| r.id == id)
                        .map(|r| (r.start, r.end))
                    {
                        self.session.time_sel = Some((start, end));
                        self.session.project.loop_region = Some((start, end));
                    }
                    ui.close_menu();
                }
                if ui.button(tr(lang, "Renderizar região...")).clicked() {
                    if let Some((start, end)) = self
                        .session
                        .project
                        .regions
                        .iter()
                        .find(|r| r.id == id)
                        .map(|r| (r.start, r.end))
                    {
                        self.session.project.loop_region = Some((start, end));
                    }
                    self.dlg
                        .open_render(&self.session.project, self.session.path.as_deref());
                    self.dlg.render_use_loop = true;
                    ui.close_menu();
                }
                if ui.button(tr(lang, "Excluir região")).clicked() {
                    self.checkpoint();
                    self.project.regions.retain(|m| m.id != id);
                    self.changed();
                    ui.close_menu();
                }
            }
            _ => {
                if ui
                    .button(tr(lang, "Inserir marcador no cursor (M)"))
                    .clicked()
                {
                    self.insert_marker();
                    ui.close_menu();
                }
                if ui
                    .button(tr(lang, "Inserir marcador de faixa de CD (N)"))
                    .clicked()
                {
                    self.exec_cmd(Some(ui.ctx()), "insert.marker_cd_track");
                    ui.close_menu();
                }
                if ui
                    .button(tr(lang, "Inserir marcador de índice de CD"))
                    .clicked()
                {
                    self.exec_cmd(Some(ui.ctx()), "insert.marker_cd_index");
                    ui.close_menu();
                }
                if ui
                    .button(tr(lang, "Inserir marcador de comando..."))
                    .clicked()
                {
                    self.exec_cmd(Some(ui.ctx()), "insert.marker_command");
                    ui.close_menu();
                }
                if ui
                    .button(tr(lang, "Inserir região da seleção (R)"))
                    .clicked()
                {
                    self.insert_region();
                    ui.close_menu();
                }
                ui.separator();
                if ui
                    .checkbox(
                        &mut self.opts.snap_to_grid,
                        tr(lang, "Encaixar na grade (Ctrl+F8)"),
                    )
                    .clicked()
                {
                    ui.close_menu();
                }
                ui.menu_button(tr(lang, "Grade da régua"), |ui| {
                    for g in neoveg_engine::session::RulerGridSpacing::ALL {
                        if ui.radio(self.opts.ruler_grid == g, g.label()).clicked() {
                            self.opts.ruler_grid = g;
                            self.set_status(format!("Grade da régua: {}", g.label()));
                            ui.close_menu();
                        }
                    }
                });
                ui.separator();
                if ui.button(tr(lang, "Limpar região de loop")).clicked() {
                    self.project.loop_region = None;
                    self.time_sel = None;
                    ui.close_menu();
                }
            }
        });
    }

    fn rename_popup(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let Some((id, mut label)) = self.tl.rename.clone() else {
            return;
        };
        let mut open = true;
        let mut done = false;
        crate::dialog_window::DialogWindow::new(tr(lang, "Renomear"))
            .id(egui::Id::new("tl_rename_win"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                let r = ui.text_edit_singleline(&mut label);
                r.request_focus();
                if ui.button(tr(lang, "OK")).clicked()
                    || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    done = true;
                }
            });
        if done {
            self.checkpoint();
            if let Some(m) = self.project.markers.iter_mut().find(|m| m.id == id) {
                m.label = label.clone();
            }
            if let Some(r) = self.project.regions.iter_mut().find(|r| r.id == id) {
                r.label = label.clone();
            }
            if let Some(e) = self.project.event_mut(id) {
                e.name = label.clone();
            }
            self.project.rename_sequence(id, &label);
            self.changed();
            self.tl.rename = None;
        } else if !open {
            self.tl.rename = None;
        } else {
            self.tl.rename = Some((id, label));
        }
    }

    fn show_drop_context_menu(&mut self, ui: &mut egui::Ui) {
        let Some(drop_menu) = self.tl.drop_context_menu.clone() else {
            return;
        };
        let mut close = false;
        let mut handled = false;
        egui::Area::new(egui::Id::new("tl_drop_context_menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(drop_menu.pos)
            .show(ui.ctx(), |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    ui.set_min_width(160.0);
                    let lang = self.lang();
                    if let Some(target_id) = drop_menu.target_event {
                        if ui.button(tr(lang, "Adicionar como take")).clicked() {
                            if self
                                .session
                                .add_media_as_take(target_id, drop_menu.media_id)
                            {
                                self.set_status(tr(lang, "Mídia adicionada como take."));
                            }
                            handled = true;
                            close = true;
                        }
                        ui.separator();
                    }
                    if ui.button(tr(lang, "Copiar aqui")).clicked() {
                        self.checkpoint();
                        self.add_media_to_timeline(
                            drop_menu.media_id,
                            drop_menu.t,
                            drop_menu.track,
                        );
                        handled = true;
                        close = true;
                    }
                    if ui.button(tr(lang, "Mover aqui")).clicked() {
                        self.checkpoint();
                        self.add_media_to_timeline(
                            drop_menu.media_id,
                            drop_menu.t,
                            drop_menu.track,
                        );
                        handled = true;
                        close = true;
                    }
                    ui.separator();
                    if ui.button(tr(lang, "Cancelar")).clicked() {
                        close = true;
                    }
                });
            });

        if close || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.tl.drop_context_menu = None;
        } else if !handled && ui.input(|i| i.pointer.any_pressed()) {
            if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                let r = Rect::from_min_size(drop_menu.pos, egui::vec2(180.0, 160.0));
                if !r.contains(p) {
                    self.tl.drop_context_menu = None;
                }
            }
        }
    }

    // -----------------------------------------------------------------
    // Cabeçalhos de trilha
    // -----------------------------------------------------------------

    // o índice da trilha é usado no layout e no projeto ao mesmo tempo
    #[allow(clippy::needless_range_loop)]
    fn headers_ui(&mut self, ui: &mut egui::Ui, area: Rect, rows: &TimelineRows) {
        let painter = ui.painter_at(area);
        painter.rect_filled(area, 0.0, self.theme.tl_header_bg);
        // rolagem vertical com a roda sobre os cabeçalhos
        if ui.rect_contains_pointer(area) {
            let d = ui.input(|i| i.raw_scroll_delta.y);
            if d != 0.0 {
                self.tl.scroll_y = (self.tl.scroll_y - d).max(0.0);
            }
        }
        let hdr_w = area.width().max(TimelineState::MIN_HEADER_W);
        let show_name = hdr_w >= 100.0;
        let show_btns = hdr_w >= 155.0;
        let show_sliders = hdr_w >= 170.0;
        let show_auto_badge = hdr_w >= 210.0;
        let playing_now = self.is_playing();
        let play_t_now = if playing_now {
            self.play_pos()
        } else {
            Ticks::ZERO
        };
        let auto_tol = self.project.settings.frame_dur().div_i64(2);
        let n = self.project.tracks.len();
        for ti in 0..n {
            let (y0, h) = rows.track_rects[ti];
            if h <= 0.0 {
                continue;
            }
            let r = Rect::from_min_size(pos2(area.left(), y0), vec2(hdr_w, h));
            if r.bottom() < area.top() || r.top() > area.bottom() {
                continue;
            }
            let selected = self.sel_track == Some(ti);
            let track = &self.project.tracks[ti];
            let tid = track.id;
            let depth = self.project.track_depth(tid);
            let indent = (depth.min(4) as f32) * 8.0;
            let is_folder = track.is_folder;
            let folder_collapsed = track.collapsed;
            let has_parent = track.parent_track_id.is_some();
            let has_children = self.project.has_track_children(tid);
            let mask_mode = track.mask_mode;
            let is_multicam = track.multicam;
            let bg = if selected {
                self.theme.tl_header_sel
            } else {
                self.theme.tl_header_idle
            };
            painter.rect_filled(r.shrink2(vec2(0.0, 0.5)), 0.0, bg);
            let use_gutter = h >= 42.0;
            let gutter_col = shade(
                self.track_rgb(track.color),
                if selected { 1.05 } else { 0.88 },
            );
            let gutter_rect = Rect::from_min_size(
                pos2(r.left() + indent, r.top() + 0.5),
                vec2(22.0, (h - 1.0).max(1.0)),
            );
            if use_gutter {
                painter.rect_filled(gutter_rect, 0.0, gutter_col);
                if selected {
                    painter.rect_filled(
                        Rect::from_min_size(gutter_rect.min, vec2(2.5, gutter_rect.height())),
                        0.0,
                        self.theme.accent,
                    );
                }
                if depth > 0 {
                    let gx = r.left() + indent * 0.5;
                    let mid_y = r.center().y;
                    painter.line_segment(
                        [pos2(gx, r.top()), pos2(gx, mid_y)],
                        Stroke::new(1.0_f32, self.theme.accent.gamma_multiply(0.7)),
                    );
                    painter.line_segment(
                        [pos2(gx, mid_y), pos2(r.left() + indent, mid_y)],
                        Stroke::new(1.0_f32, self.theme.accent.gamma_multiply(0.7)),
                    );
                }
            } else {
                painter.rect_filled(
                    Rect::from_min_size(r.min, vec2(6.0, h)),
                    0.0,
                    shade(self.track_rgb(track.color), 1.0),
                );
                if depth > 0 {
                    let gx = r.left() + 6.0 + indent * 0.5;
                    let mid_y = r.center().y;
                    painter.line_segment(
                        [pos2(gx, r.top()), pos2(gx, mid_y)],
                        Stroke::new(1.0_f32, self.theme.accent.gamma_multiply(0.7)),
                    );
                    painter.line_segment(
                        [pos2(gx, mid_y), pos2(r.left() + 9.0 + indent, mid_y)],
                        Stroke::new(1.0_f32, self.theme.accent.gamma_multiply(0.7)),
                    );
                }
            }
            painter.line_segment(
                [pos2(r.left(), r.bottom()), pos2(r.right(), r.bottom())],
                Stroke::new(1.0_f32, self.theme.border),
            );

            // clique no fundo seleciona a trilha
            let bresp = ui.interact(r, ui.id().with(("thdr", tid)), Sense::click());
            if bresp.clicked() {
                self.session.sel_track = Some(ti);
            }
            bresp.context_menu(|ui| self.track_menu(ui, ti));
            // o menu pode ter excluído trilhas: os índices calculados no início
            // do quadro deixam de valer (evita acesso fora dos limites)
            if self.project.tracks.get(ti).map(|t| t.id) != Some(tid) {
                break;
            }

            let kind = self.project.tracks[ti].kind;
            let mut changed_track = false;
            let mut want_checkpoint = false;
            let kf_has = kind == TrackKind::Video
                && self.project.tracks.get(ti).is_some_and(|t| {
                    !neoveg_engine::track_keyframes::track_rows(t, self.kf_tol()).is_empty()
                });
            let kf_open = !self.tl.kf_collapsed.contains(&tid);
            let mut kf_toggle = false;
            let is_video = kind == TrackKind::Video;
            let lang = self.lang();
            let btns = self.prefs.track_header.get(is_video);
            let mut more: Option<HeaderAction> = None;
            let (cur_mute, cur_solo, cur_auto, cur_armed) = {
                let t = &self.project.tracks[ti];
                (t.mute, t.solo, t.automation_mode, t.record_armed)
            };
            let adj = self.project.tracks[ti].is_adjustment();
            let track_icon = if is_folder {
                Icon::TrackFolder
            } else if kind == TrackKind::Video {
                Icon::VideoTrack
            } else {
                Icon::AudioTrack
            };
            let draw_more_menu = |ui: &mut egui::Ui, more_out: &mut Option<HeaderAction>| {
                if ui
                    .selectable_label(cur_mute, tr(lang, "Mudo (Z)"))
                    .clicked()
                {
                    *more_out = Some(HeaderAction::Mute);
                    ui.close_menu();
                }
                if ui
                    .selectable_label(cur_solo, tr(lang, "Solo (X)"))
                    .clicked()
                {
                    *more_out = Some(HeaderAction::Solo);
                    ui.close_menu();
                }
                if ui.button(tr(lang, "Efeitos da trilha...")).clicked() {
                    *more_out = Some(HeaderAction::Fx);
                    ui.close_menu();
                }
                if is_video
                    && !adj
                    && ui
                        .button(tr(lang, "Movimento da trilha (Track Motion)..."))
                        .clicked()
                {
                    *more_out = Some(HeaderAction::TrackMotion);
                    ui.close_menu();
                }
                if !is_folder {
                    if ui
                        .button(tr(lang, "Minimizar a altura da trilha"))
                        .clicked()
                    {
                        *more_out = Some(HeaderAction::Minimize);
                        ui.close_menu();
                    }
                    if ui
                        .button(tr(lang, "Maximizar a altura da trilha"))
                        .clicked()
                    {
                        *more_out = Some(HeaderAction::Maximize);
                        ui.close_menu();
                    }
                }
                if is_video
                    && !is_folder
                    && !adj
                    && ui
                        .selectable_label(
                            has_parent,
                            tr(lang, "Tornar filho de composição (Pai/Filho)"),
                        )
                        .clicked()
                {
                    *more_out = Some(HeaderAction::ToggleCompositingChild);
                    ui.close_menu();
                }
                if !is_video
                    && !is_folder
                    && ui
                        .selectable_label(cur_armed, tr(lang, "Armar para gravação"))
                        .clicked()
                {
                    *more_out = Some(HeaderAction::RecordArm);
                    ui.close_menu();
                }
                if !is_folder {
                    ui.menu_button(format!("Automação ({})", cur_auto.short_label()), |ui| {
                        for m in AutomationMode::ALL {
                            if ui.radio(cur_auto == m, m.label()).clicked() {
                                *more_out = Some(HeaderAction::SetAutomation(m));
                                ui.close_menu();
                            }
                        }
                    });
                }
                ui.separator();
                if ui
                    .button(tr(lang, "Editar conjunto de botões visíveis..."))
                    .clicked()
                {
                    *more_out = Some(HeaderAction::EditButtons);
                    ui.close_menu();
                }
            };

            if use_gutter {
                // Coluna lateral colorida no padrão VEGAS Pro (manual p. 69):
                // botão Mais (`…`) no topo, acima do número e do ícone da faixa.
                let gutter_fg = crate::theme::contrast_on(gutter_col);
                let mut gutter_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(gutter_rect.shrink2(vec2(1.0, 1.0)))
                        .id_salt(("th_gutter", tid)),
                );
                gutter_ui.set_clip_rect(gutter_rect.intersect(area));
                gutter_ui.spacing_mut().item_spacing = vec2(0.0, 1.0);
                gutter_ui.spacing_mut().button_padding = vec2(2.0, 0.0);
                gutter_ui.vertical_centered(|ui| {
                    ui.menu_button(egui::RichText::new("…").small().color(gutter_fg), |ui| {
                        draw_more_menu(ui, &mut more)
                    })
                    .response
                    .on_hover_text(tr(lang, "Mais (botões do cabeçalho, inclusive os ocultos)"));
                    let num_txt = if selected {
                        egui::RichText::new(format!("{}", ti + 1))
                            .size(11.0)
                            .strong()
                            .color(gutter_fg)
                    } else {
                        egui::RichText::new(format!("{}", ti + 1))
                            .size(11.0)
                            .color(gutter_fg)
                    };
                    ui.label(num_txt);
                    if h >= 48.0 {
                        let mut gutter_theme = self.theme.clone();
                        gutter_theme.text_primary = gutter_fg;
                        gutter_theme.text_muted = gutter_fg.gamma_multiply(0.85);
                        icons::paint_inline(ui, track_icon, 12.0, &gutter_theme);
                    }
                });
            }

            let inner = Rect::from_min_max(
                pos2(
                    r.left() + indent + if use_gutter { 26.0 } else { 10.0 },
                    r.top() + 3.0,
                ),
                pos2(r.right() - 14.0, r.bottom() - 3.0),
            );
            let mut child =
                ui.new_child(egui::UiBuilder::new().max_rect(inner).id_salt(("th", tid)));
            child.set_clip_rect(inner.intersect(area));
            child.horizontal(|ui| {
                if !use_gutter {
                    ui.menu_button(egui::RichText::new("…").small(), |ui| {
                        draw_more_menu(ui, &mut more);
                    })
                    .response
                    .on_hover_text(tr(lang, "Mais (botões do cabeçalho, inclusive os ocultos)"));
                    let (num_r, _) = ui.allocate_exact_size(vec2(22.0, 18.0), Sense::hover());
                    let num_bg = if selected {
                        self.theme.selection_bg
                    } else {
                        self.theme.widget_bg
                    };
                    let num_border = if selected {
                        self.theme.accent
                    } else {
                        self.theme.border
                    };
                    ui.painter().rect_filled(num_r, 2.0, num_bg);
                    ui.painter().rect_stroke(
                        num_r,
                        2.0,
                        Stroke::new(1.0_f32, num_border),
                        StrokeKind::Inside,
                    );
                    ui.painter().text(
                        num_r.center(),
                        Align2::CENTER_CENTER,
                        format!("{}", ti + 1),
                        FontId::proportional(11.0),
                        crate::theme::contrast_on(num_bg),
                    );
                    icons::paint_inline(ui, track_icon, 14.0, &self.theme);
                }
                if is_folder {
                    let toggle_lbl = if folder_collapsed { ">" } else { "v" };
                    if ui
                        .small_button(toggle_lbl)
                        .on_hover_text(if folder_collapsed {
                            "Expandir pasta de trilhas"
                        } else {
                            "Recolher pasta de trilhas"
                        })
                        .clicked()
                    {
                        self.session.toggle_track_folder(ti);
                        self.sync_status_from_session();
                    }
                }
                let is_adjustment = self.project.tracks[ti].is_adjustment();
                if is_adjustment {
                    ui.label(
                        egui::RichText::new(tr(lang, "AJ"))
                            .small()
                            .strong()
                            .color(self.theme.text_muted),
                    )
                    .on_hover_text(tr(
                        lang,
                        "Trilha de ajuste (Adjustment Track): os efeitos desta trilha \
                         valem para todas as trilhas de vídeo abaixo dela",
                    ));
                } else if !is_folder && is_video && has_children {
                    ui.label(
                        egui::RichText::new(tr(lang, "PAI"))
                            .small()
                            .strong()
                            .color(self.theme.accent),
                    )
                    .on_hover_text(tr(
                        lang,
                        "Trilha pai em composição Pai/Filho (Track Motion de grupo)",
                    ));
                }
                if !is_folder && is_video && mask_mode != neoveg_engine::model::TrackMaskMode::None
                {
                    ui.label(
                        egui::RichText::new(tr(lang, "MSK"))
                            .small()
                            .strong()
                            .color(self.theme.accent),
                    )
                    .on_hover_text(mask_mode.label());
                }
                if is_multicam {
                    ui.label(
                        egui::RichText::new(tr(lang, "MC"))
                            .small()
                            .strong()
                            .color(self.theme.accent),
                    )
                    .on_hover_text(tr(lang, "Trilha Multicâmera"));
                }
                if has_parent && !is_folder {
                    ui.label(egui::RichText::new("↳").small().weak())
                        .on_hover_text(tr(lang, "Filha vinculada à trilha/pasta pai"));
                }
                let t = &mut self.project.tracks[ti];
                if show_name {
                    let mut btns_w = 0.0_f32;
                    if show_btns {
                        if btns.mute {
                            btns_w += 22.0;
                        }
                        if btns.solo && !is_folder {
                            btns_w += 22.0;
                        }
                        if btns.fx && !is_folder {
                            btns_w += 28.0;
                        }
                        if show_auto_badge && !is_folder {
                            btns_w += 40.0;
                        }
                        if is_video && btns.track_motion && !is_adjustment && !is_folder {
                            btns_w += 22.0;
                        }
                        if !is_video && !is_folder && btns.record_arm {
                            btns_w += 22.0;
                        }
                        if is_video && btns.compositing_child && !is_folder && !is_adjustment {
                            btns_w += 22.0;
                        }
                        if btns.height && !is_folder {
                            btns_w += 44.0;
                        }
                        if kf_has {
                            btns_w += 22.0;
                        }
                    }
                    let avail_for_name = inner.width() - btns_w - 16.0;
                    let name_w = avail_for_name.clamp(42.0, 120.0);
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut t.name)
                            .desired_width(name_w)
                            .font(egui::TextStyle::Small),
                    );
                    if r.changed() {
                        changed_track = true;
                    }
                }
                if show_btns
                    && btns.mute
                    && ui
                        .selectable_label(t.mute, "M")
                        .on_hover_text(tr(lang, "Mudo (Z)"))
                        .clicked()
                {
                    more = Some(HeaderAction::Mute);
                }
                if show_btns
                    && btns.solo
                    && !is_folder
                    && ui
                        .selectable_label(t.solo, "S")
                        .on_hover_text(tr(lang, "Solo (X)"))
                        .clicked()
                {
                    more = Some(HeaderAction::Solo);
                }
                let fxlabel = if t.fx.is_empty() { "fx" } else { "FX" };
                if show_btns
                    && btns.fx
                    && !is_folder
                    && ui
                        .small_button(fxlabel)
                        .on_hover_text(tr(lang, "Efeitos da trilha"))
                        .clicked()
                {
                    more = Some(HeaderAction::Fx);
                }
                if show_auto_badge && !is_folder {
                    let auto_mode = t.automation_mode;
                    let badge_txt = if auto_mode.is_write() {
                        egui::RichText::new(auto_mode.short_label())
                            .small()
                            .strong()
                            .color(self.theme.offline_fg)
                    } else {
                        egui::RichText::new(auto_mode.short_label()).small()
                    };
                    if ui
                        .selectable_label(auto_mode.is_write(), badge_txt)
                        .on_hover_text(format!(
                            "{} — clique para alternar (Off / Read / Touch / Latch)",
                            auto_mode.label()
                        ))
                        .clicked()
                    {
                        more = Some(HeaderAction::CycleAutomation);
                    }
                }
                if show_btns
                    && kind == TrackKind::Video
                    && btns.track_motion
                    && !is_adjustment
                    && !is_folder
                    && icons::small_icon_button(ui, Icon::TrackMotion, None, false, &self.theme)
                        .on_hover_text(tr(lang, "Movimento da trilha (Track Motion)"))
                        .clicked()
                {
                    self.dlg.track_motion = Some(tid);
                }
                if show_btns && kind == TrackKind::Audio && !is_folder && btns.record_arm {
                    let (rec_armed, rec_locked) = {
                        let tk = &self.project.tracks[ti];
                        (tk.record_armed, tk.locked)
                    };
                    let rec_resp =
                        icons::small_icon_button(ui, Icon::RecordDot, None, rec_armed, &self.theme);
                    rec_resp.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            !rec_locked,
                            "Armar gravação",
                        )
                    });
                    if rec_resp
                        .on_hover_text("Armar faixa para gravação (Ctrl+Shift+R)")
                        .clicked()
                    {
                        more = Some(HeaderAction::RecordArm);
                    }
                }
                if show_btns
                    && is_video
                    && btns.compositing_child
                    && !is_folder
                    && !is_adjustment
                    && icons::small_icon_button(
                        ui,
                        Icon::ParentChild,
                        None,
                        has_parent,
                        &self.theme,
                    )
                    .on_hover_text(if has_parent {
                        tr(lang, "Promover filho para trilha raiz")
                    } else {
                        tr(lang, "Tornar filho de composição (Pai/Filho)")
                    })
                    .clicked()
                {
                    more = Some(HeaderAction::ToggleCompositingChild);
                }
                if show_btns && btns.height && !is_folder {
                    // ▲ / ▼ têm cobertura garantida em todas as fontes (fonts.rs)
                    if ui
                        .small_button("▲")
                        .on_hover_text(tr(lang, "Minimizar a altura da trilha"))
                        .clicked()
                    {
                        more = Some(HeaderAction::Minimize);
                    }
                    if ui
                        .small_button("▼")
                        .on_hover_text(tr(lang, "Maximizar a altura da trilha"))
                        .clicked()
                    {
                        more = Some(HeaderAction::Maximize);
                    }
                }
                if kf_has
                    && icons::small_icon_button(
                        ui,
                        if kf_open {
                            Icon::KeyframeFilled
                        } else {
                            Icon::KeyframeEmpty
                        },
                        None,
                        kf_open,
                        &self.theme,
                    )
                    .on_hover_text(tr(lang, "Expandir/recolher os quadros-chave da trilha"))
                    .clicked()
                {
                    kf_toggle = true;
                }
            });
            if kf_toggle && !self.tl.kf_collapsed.remove(&tid) {
                self.tl.kf_collapsed.insert(tid);
            }
            match more {
                Some(HeaderAction::Mute) => {
                    let t = &mut self.project.tracks[ti];
                    t.mute = !t.mute;
                    want_checkpoint = true;
                    changed_track = true;
                }
                Some(HeaderAction::Solo) => {
                    let t = &mut self.project.tracks[ti];
                    t.solo = !t.solo;
                    want_checkpoint = true;
                    changed_track = true;
                }
                Some(HeaderAction::Fx) => self.open_fx_target(FxTarget::Track(tid)),
                Some(HeaderAction::TrackMotion) => self.dlg.track_motion = Some(tid),
                Some(HeaderAction::Minimize) => self.toggle_track_height(ti, false),
                Some(HeaderAction::Maximize) => self.toggle_track_height(ti, true),
                Some(HeaderAction::ToggleCompositingChild) => {
                    if has_parent {
                        self.session.unmake_compositing_child(ti);
                        self.sync_after_session();
                    } else if let Err(e) = self.session.make_compositing_child(ti) {
                        self.set_status(format!("{e}"));
                    } else {
                        self.sync_after_session();
                    }
                }
                Some(HeaderAction::RecordArm) => {
                    self.session.toggle_record_arm(ti);
                    self.sync_status_from_session();
                }
                Some(HeaderAction::SetAutomation(mode)) => {
                    self.session.set_track_automation_mode(ti, mode);
                    self.sync_status_from_session();
                }
                Some(HeaderAction::CycleAutomation) => {
                    self.session.cycle_track_automation_mode(ti);
                    self.sync_status_from_session();
                }
                Some(HeaderAction::EditButtons) => {
                    self.dlg.header_buttons = Some((is_video, btns));
                }
                None => {}
            }
            let mut auto_record: Option<(EnvKind, f32)> = None;
            let mut auto_released = false;
            if h >= 50.0 && show_sliders {
                child.horizontal(|ui| {
                    let t = &mut self.project.tracks[ti];
                    let am = t.automation_mode;
                    if kind == TrackKind::Audio {
                        ui.label(egui::RichText::new(tr(lang, "Vol")).small());
                        let r = ui.add(
                            egui::Slider::new(&mut t.volume_db, -60.0..=12.0)
                                .show_value(true)
                                .suffix(" dB")
                                .fixed_decimals(1)
                                .trailing_fill(true),
                        );
                        if r.drag_started() {
                            want_checkpoint = true;
                        }
                        if r.double_clicked() {
                            t.volume_db = 0.0;
                        }
                        if playing_now && am.is_write() && (r.dragged() || r.changed()) {
                            auto_record = Some((
                                EnvKind::Volume,
                                crate::model::db_to_lin(t.volume_db).clamp(0.0, 2.0),
                            ));
                        }
                        if r.drag_stopped() {
                            auto_released = true;
                        }
                        changed_track |= r.changed() || r.double_clicked();
                    } else {
                        ui.label(egui::RichText::new(tr(lang, "Nível")).small());
                        let mut pct = t.opacity * 100.0;
                        let r = ui.add(
                            egui::Slider::new(&mut pct, 0.0..=100.0)
                                .suffix(" %")
                                .fixed_decimals(0)
                                .trailing_fill(true),
                        );
                        if r.drag_started() {
                            want_checkpoint = true;
                        }
                        if r.changed() {
                            t.opacity = pct / 100.0;
                            changed_track = true;
                        }
                        if playing_now && am.is_write() && (r.dragged() || r.changed()) {
                            auto_record = Some((EnvKind::Opacity, t.opacity.clamp(0.0, 1.0)));
                        }
                        if r.drag_stopped() {
                            auto_released = true;
                        }
                    }
                });
            }
            if h >= 72.0 && show_sliders {
                child.horizontal(|ui| {
                    if kind == TrackKind::Audio {
                        let t = &mut self.project.tracks[ti];
                        let am = t.automation_mode;
                        ui.label(egui::RichText::new(tr(lang, "Pan")).small());
                        let r = ui.add(egui::Slider::new(&mut t.pan, -1.0..=1.0).show_value(false));
                        if r.drag_started() {
                            want_checkpoint = true;
                        }
                        if r.double_clicked() {
                            t.pan = 0.0;
                        }
                        if playing_now && am.is_write() && (r.dragged() || r.changed()) {
                            auto_record = Some((EnvKind::Pan, t.pan.clamp(-1.0, 1.0)));
                        }
                        if r.drag_stopped() {
                            auto_released = true;
                        }
                        changed_track |= r.changed() || r.double_clicked();
                        ui.label(egui::RichText::new(EnvKind::Pan.format(t.pan)).small());
                        if !self.project.audio_buses.is_empty() {
                            let bus_list: Vec<(Id, String)> = self
                                .project
                                .audio_buses
                                .iter()
                                .map(|b| (b.id, b.name.clone()))
                                .collect();
                            let t = &mut self.project.tracks[ti];
                            let out_lbl = t
                                .output_bus
                                .and_then(|bid| {
                                    bus_list
                                        .iter()
                                        .find(|(id, _)| *id == bid)
                                        .map(|(_, nm)| nm.as_str())
                                })
                                .unwrap_or_else(|| tr(lang, "Master"));
                            let before = t.output_bus;
                            egui::ComboBox::from_id_salt(("hdr_out", tid))
                                .width(64.0)
                                .selected_text(out_lbl)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut t.output_bus,
                                        None,
                                        tr(lang, "Master"),
                                    );
                                    for (bid, bname) in &bus_list {
                                        ui.selectable_value(&mut t.output_bus, Some(*bid), bname);
                                    }
                                });
                            if t.output_bus != before {
                                want_checkpoint = true;
                                changed_track = true;
                            }
                        }
                    } else {
                        let t = &mut self.project.tracks[ti];
                        ui.label(egui::RichText::new(tr(lang, "Modo")).small());
                        let before = t.blend;
                        egui::ComboBox::from_id_salt(("blend", tid))
                            .width(120.0)
                            .selected_text(t.blend.name_in(lang))
                            .show_ui(ui, |ui| {
                                for b in BlendMode::ALL {
                                    ui.selectable_value(&mut t.blend, b, b.name_in(lang));
                                }
                            });
                        if t.blend != before {
                            changed_track = true;
                        }
                    }
                });
            }
            if want_checkpoint {
                // checkpoint com o estado anterior aproximado (antes da mudança atual)
                let mut snap = self.project.clone();
                let t = &mut snap.tracks[ti];
                let old = &self.snapshot.tracks.iter().find(|x| x.id == tid);
                if let Some(old) = old {
                    t.mute = old.mute;
                    t.solo = old.solo;
                    t.volume_db = old.volume_db;
                    t.opacity = old.opacity;
                    t.pan = old.pan;
                    t.output_bus = old.output_bus;
                }
                self.undo.push(snap);
                self.redo.clear();
            }
            if changed_track {
                self.changed();
            }
            if let Some((ekind, eval)) = auto_record {
                let am = self.project.tracks[ti].automation_mode;
                self.session
                    .record_automation_point(ti, ekind, play_t_now, eval, auto_tol);
                if am == AutomationMode::Latch {
                    self.tl.latch_hold = Some((ti, ekind, eval));
                }
            } else if auto_released
                && self.project.tracks[ti].automation_mode == AutomationMode::Touch
            {
                self.tl.latch_hold = None;
            }
            // medidor de nível (áudio / entrada armada durante gravação)
            if kind == TrackKind::Audio {
                let rec_lv = if self.project.tracks[ti].record_armed {
                    self.recording_peaks.get(&tid).copied()
                } else {
                    None
                };
                let lv = rec_lv
                    .unwrap_or_else(|| self.meters_hold.get(&tid).cloned().unwrap_or([0.0; 2]));
                let mr = Rect::from_min_max(
                    pos2(r.right() - 11.0, r.top() + 4.0),
                    pos2(r.right() - 3.0, r.bottom() - 4.0),
                );
                draw_meter(&painter, mr, lv);
            }
            // alça de altura da trilha
            let hr = Rect::from_min_max(
                pos2(r.left(), r.bottom() - 3.0),
                pos2(r.right(), r.bottom() + 2.0),
            );
            let hresp = ui.interact(hr, ui.id().with(("theight", tid)), Sense::drag());
            if hresp.hovered() || hresp.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
            }
            if hresp.drag_started() {
                self.tl.drag = Drag::TrackHeight {
                    track: ti,
                    orig_h: h,
                    y0: hresp.interact_pointer_pos().map(|p| p.y).unwrap_or(0.0),
                };
            }
            if hresp.dragged() {
                if let (Drag::TrackHeight { track, orig_h, y0 }, Some(p)) =
                    (self.tl.drag.clone(), hresp.interact_pointer_pos())
                {
                    if track == ti {
                        self.project.tracks[ti].height = (orig_h + p.y - y0).clamp(28.0, 400.0);
                    }
                }
            }
            if hresp.drag_stopped() {
                self.tl.drag = Drag::None;
            }
        }

        // Trilha do Barramento de Vídeo (Master Video Bus Track, Ctrl+Shift+B)
        if let Some((y0, h)) = rows.video_bus_rect {
            let r = Rect::from_min_size(pos2(area.left(), y0), vec2(hdr_w, h));
            if r.bottom() >= area.top() && r.top() <= area.bottom() {
                painter.rect_filled(r.shrink2(vec2(0.0, 0.5)), 0.0, self.theme.tl_header_sel);
                painter.rect_filled(
                    Rect::from_min_size(r.min, vec2(6.0, h)),
                    0.0,
                    Color32::from_rgb(168, 114, 232),
                );
                painter.line_segment(
                    [pos2(r.left(), r.bottom()), pos2(r.right(), r.bottom())],
                    Stroke::new(1.0_f32, self.theme.border),
                );
                let inner = Rect::from_min_max(
                    pos2(r.left() + 10.0, r.top() + 3.0),
                    pos2(r.right() - 10.0, r.bottom() - 3.0),
                );
                let mut child =
                    ui.new_child(egui::UiBuilder::new().max_rect(inner).id_salt("vbus_hdr"));
                child.set_clip_rect(inner.intersect(area));
                let lang = self.lang();
                child.horizontal(|ui| {
                    icons::paint_inline(ui, Icon::VideoBusTrack, 14.0, &self.theme);
                    ui.label(
                        egui::RichText::new(tr(lang, "Barramento de Vídeo"))
                            .small()
                            .strong(),
                    );
                    let fx_lbl = if self.project.master.video_fx.is_empty() {
                        "fx"
                    } else {
                        "FX"
                    };
                    if ui
                        .small_button(fx_lbl)
                        .on_hover_text(tr(lang, "Efeitos de saída de vídeo (Master)"))
                        .clicked()
                    {
                        self.open_fx_target(FxTarget::MasterVideo);
                    }
                });
                child.horizontal(|ui| {
                    let has_op = self
                        .project
                        .master
                        .video_envelope(EnvKind::Opacity)
                        .is_some();
                    let has_ftc = self
                        .project
                        .master
                        .video_envelope(EnvKind::FadeToColor)
                        .is_some();
                    let has_mb = self
                        .project
                        .master
                        .video_envelope(EnvKind::MotionBlur)
                        .is_some();
                    if ui.selectable_label(has_op, tr(lang, "Opacidade")).clicked() {
                        self.session.toggle_master_video_envelope(EnvKind::Opacity);
                        self.sync_after_session();
                    }
                    if ui
                        .selectable_label(has_ftc, tr(lang, "Fade"))
                        .on_hover_text(tr(lang, "Fade para cor"))
                        .clicked()
                    {
                        self.session
                            .toggle_master_video_envelope(EnvKind::FadeToColor);
                        self.sync_after_session();
                    }
                    if ui
                        .selectable_label(has_mb, tr(lang, "Mov."))
                        .on_hover_text(tr(lang, "Desfoque de movimento"))
                        .clicked()
                    {
                        self.session
                            .toggle_master_video_envelope(EnvKind::MotionBlur);
                        self.sync_after_session();
                    }
                });
            }
        }

        // Trilhas de barramentos auxiliares (B)
        for bi in 0..rows.bus_rects.len() {
            if bi >= self.project.audio_buses.len() {
                break;
            }
            let (y0, h) = rows.bus_rects[bi];
            let r = Rect::from_min_size(pos2(area.left(), y0), vec2(hdr_w, h));
            if r.bottom() < area.top() || r.top() > area.bottom() {
                continue;
            }
            let bid = self.project.audio_buses[bi].id;
            painter.rect_filled(r.shrink2(vec2(0.0, 0.5)), 0.0, self.theme.tl_header_idle);
            painter.rect_filled(
                Rect::from_min_size(r.min, vec2(6.0, h)),
                0.0,
                self.theme.accent,
            );
            painter.line_segment(
                [pos2(r.left(), r.bottom()), pos2(r.right(), r.bottom())],
                Stroke::new(1.0_f32, self.theme.border),
            );
            let bresp = ui.interact(r, ui.id().with(("bhdr", bid)), Sense::click());
            bresp.context_menu(|ui| self.bus_menu(ui, bi));
            if self.project.audio_buses.get(bi).map(|b| b.id) != Some(bid) {
                break;
            }
            let inner = Rect::from_min_max(
                pos2(r.left() + 10.0, r.top() + 3.0),
                pos2(r.right() - 14.0, r.bottom() - 3.0),
            );
            let mut child =
                ui.new_child(egui::UiBuilder::new().max_rect(inner).id_salt(("bh", bid)));
            child.set_clip_rect(inner.intersect(area));
            let mut changed_bus = false;
            let mut commit_name: Option<String> = None;
            child.horizontal(|ui| {
                icons::paint_inline(ui, Icon::BusTrack, 14.0, &self.theme);
                let cur_name = self.project.audio_buses[bi].name.clone();
                let buf_id = egui::Id::new(("tl_bus_name", bid));
                let mut draft = ui.data_mut(|d| {
                    d.get_temp::<String>(buf_id)
                        .unwrap_or_else(|| cur_name.clone())
                });
                let nr = ui.add(
                    egui::TextEdit::singleline(&mut draft)
                        .desired_width(90.0)
                        .font(egui::TextStyle::Small),
                );
                if nr.has_focus() {
                    ui.data_mut(|d| d.insert_temp(buf_id, draft.clone()));
                } else if nr.lost_focus() {
                    commit_name = Some(draft.clone());
                    ui.data_mut(|d| d.remove::<String>(buf_id));
                } else if draft != cur_name {
                    ui.data_mut(|d| d.remove::<String>(buf_id));
                }
                let b_mute = self.project.audio_buses[bi].mute;
                let b_solo = self.project.audio_buses[bi].solo;
                let b_fx_empty = self.project.audio_buses[bi].fx.is_empty();
                if ui.selectable_label(b_mute, "M").clicked() {
                    self.checkpoint();
                    self.project.audio_buses[bi].mute = !b_mute;
                    changed_bus = true;
                }
                if ui.selectable_label(b_solo, "S").clicked() {
                    self.checkpoint();
                    self.project.audio_buses[bi].solo = !b_solo;
                    changed_bus = true;
                }
                let lang = self.lang();
                let fxlabel = if b_fx_empty { "fx" } else { "FX" };
                if ui
                    .small_button(fxlabel)
                    .on_hover_text(tr(lang, "Efeitos do barramento"))
                    .clicked()
                {
                    self.open_fx_target(FxTarget::AudioBus(bid));
                }
            });
            let lang = self.lang();
            child.horizontal(|ui| {
                ui.label(egui::RichText::new(tr(lang, "Vol")).small());
                let mut vol = self.project.audio_buses[bi].volume_db;
                let sr = ui.add(
                    egui::Slider::new(&mut vol, -60.0..=12.0)
                        .show_value(true)
                        .suffix(" dB")
                        .fixed_decimals(1)
                        .trailing_fill(true),
                );
                if sr.drag_started() {
                    self.checkpoint();
                }
                if sr.double_clicked() {
                    self.checkpoint();
                    self.project.audio_buses[bi].volume_db = 0.0;
                    changed_bus = true;
                } else if sr.changed() {
                    self.project.audio_buses[bi].volume_db = vol;
                    changed_bus = true;
                }
            });
            if let Some(new_name) = commit_name {
                self.session.rename_audio_bus(bid, new_name);
            }
            if changed_bus {
                self.changed();
            }
            let lv = self.meters_hold.get(&bid).cloned().unwrap_or([0.0; 2]);
            let mr = Rect::from_min_max(
                pos2(r.right() - 11.0, r.top() + 4.0),
                pos2(r.right() - 3.0, r.bottom() - 4.0),
            );
            draw_meter(&painter, mr, lv);
        }
    }

    pub fn track_menu(&mut self, ui: &mut egui::Ui, ti: usize) {
        let lang = self.lang();
        let kind = self.project.tracks[ti].kind;
        let tid = self.project.tracks[ti].id;
        if ui.button(tr(lang, "Efeitos da trilha...")).clicked() {
            self.open_fx_target(FxTarget::Track(tid));
            ui.close_menu();
        }
        if kind == TrackKind::Video && ui.button(tr(lang, "Movimento da trilha...")).clicked() {
            self.dlg.track_motion = Some(tid);
            ui.close_menu();
        }
        if kind == TrackKind::Video {
            let mut on = self.tl.layers_expanded.contains(&tid);
            if ui
                .checkbox(&mut on, tr(lang, "Expandir camadas da trilha"))
                .changed()
            {
                if on {
                    self.tl.layers_expanded.insert(tid);
                } else {
                    self.tl.layers_expanded.remove(&tid);
                }
                ui.close_menu();
            }
        }
        ui.separator();
        let envs: &[EnvKind] = if kind == TrackKind::Audio {
            &[EnvKind::Volume, EnvKind::Pan]
        } else {
            &[EnvKind::Opacity, EnvKind::FadeToColor]
        };
        for &k in envs {
            let has = self.project.tracks[ti].envelope(k).is_some();
            let k_name = k.name_in(lang);
            let label = if has {
                tr_args(lang, "Remover envelope: {kind}", &[("kind", &k_name)])
            } else {
                tr_args(lang, "Inserir envelope: {kind}", &[("kind", &k_name)])
            };
            if ui.button(label).clicked() {
                self.checkpoint();
                let t = &mut self.project.tracks[ti];
                if has {
                    t.envelopes.retain(|e| e.kind != k);
                } else {
                    t.envelopes.push(Envelope::new(k));
                    self.opts.show_envelopes = true;
                }
                self.changed();
                ui.close_menu();
            }
        }
        if kind == TrackKind::Audio {
            let cur_pm = self.project.tracks[ti].pan_mode;
            ui.menu_button(tr(lang, "Modo de panorâmica (Pan)"), |ui| {
                if ui
                    .selectable_label(cur_pm.is_none(), tr(lang, "Padrão do projeto"))
                    .clicked()
                {
                    self.checkpoint();
                    self.project.tracks[ti].pan_mode = None;
                    self.changed();
                    ui.close_menu();
                }
                ui.separator();
                for pm in AudioPanMode::ALL {
                    if ui
                        .selectable_label(cur_pm == Some(pm), pm.name_in(lang))
                        .clicked()
                    {
                        self.checkpoint();
                        self.project.tracks[ti].pan_mode = Some(pm);
                        self.changed();
                        ui.close_menu();
                    }
                }
            });
        }
        // Curvas de suavização da faixa (áudio e vídeo): define a curva padrão da trilha
        // e permite aplicar suavização imediatamente aos envelopes e fades da faixa.
        let cur_def_curve = self.project.tracks[ti].default_env_curve;
        let cur_def_sm = self.project.tracks[ti].default_env_smoothness;
        ui.menu_button(tr(lang, "Curva de suavização da trilha"), |ui| {
            for c in Curve::ALL {
                if ui
                    .selectable_label(cur_def_curve == c, c.name_in(lang))
                    .clicked()
                {
                    self.checkpoint();
                    let t = &mut self.project.tracks[ti];
                    t.default_env_curve = c;
                    for env in &mut t.envelopes {
                        env.set_all_curves(c, None);
                    }
                    if c != Curve::Hold {
                        for ev in &mut t.events {
                            ev.fade_in.curve = c;
                            ev.fade_out.curve = c;
                        }
                    }
                    self.changed();
                    ui.close_menu();
                }
            }
            ui.separator();
            let mut sm_pct = (cur_def_sm * 100.0).clamp(0.0, 100.0);
            ui.label(tr(lang, "Suavização (%)"));
            if ui
                .add(
                    egui::Slider::new(&mut sm_pct, 0.0..=100.0)
                        .suffix(" %")
                        .fixed_decimals(0),
                )
                .changed()
            {
                let sm = (sm_pct / 100.0).clamp(0.0, 1.0);
                let t = &mut self.project.tracks[ti];
                t.default_env_smoothness = sm;
                for env in &mut t.envelopes {
                    for pt in &mut env.points {
                        pt.smoothness = sm;
                    }
                }
                self.changed();
            }
        });
        if kind == TrackKind::Audio && !self.project.audio_buses.is_empty() {
            ui.menu_button(tr(lang, "Roteamento de saída"), |ui| {
                let cur_out = self.project.tracks[ti].output_bus;
                if ui
                    .selectable_label(cur_out.is_none(), tr(lang, "Master"))
                    .clicked()
                {
                    self.checkpoint();
                    self.project.tracks[ti].output_bus = None;
                    self.changed();
                    ui.close_menu();
                }
                let buses: Vec<(Id, String)> = self
                    .project
                    .audio_buses
                    .iter()
                    .map(|b| (b.id, b.name.clone()))
                    .collect();
                for (bid, bname) in buses {
                    if ui.selectable_label(cur_out == Some(bid), bname).clicked() {
                        self.checkpoint();
                        self.project.tracks[ti].output_bus = Some(bid);
                        self.changed();
                        ui.close_menu();
                    }
                }
            });
        }
        ui.separator();
        let is_folder = self.project.tracks[ti].is_folder;
        let folder_collapsed = self.project.tracks[ti].collapsed;
        let has_parent = self.project.tracks[ti].parent_track_id.is_some();
        if is_folder {
            if ui
                .button(tr(
                    lang,
                    if folder_collapsed {
                        "Expandir pasta de trilhas"
                    } else {
                        "Recolher pasta de trilhas"
                    },
                ))
                .clicked()
            {
                self.session.toggle_track_folder(ti);
                self.sync_status_from_session();
                ui.close_menu();
            }
        } else {
            if ti > 0
                && ui
                    .button(tr(lang, "Tornar filho de composição (Pai/Filho)"))
                    .clicked()
            {
                if let Err(e) = self.session.make_compositing_child(ti) {
                    self.set_status(format!("{e}"));
                } else {
                    self.sync_after_session();
                }
                ui.close_menu();
            }
            if has_parent
                && ui
                    .button(tr(lang, "Promover filho para trilha raiz"))
                    .clicked()
            {
                self.session.unmake_compositing_child(ti);
                self.sync_after_session();
                ui.close_menu();
            }
            if ui.button(tr(lang, "Agrupar em pasta de trilhas")).clicked() {
                self.session.sel_track = Some(ti);
                self.session.insert_track_folder(kind);
                self.sync_after_session();
                ui.close_menu();
            }
        }
        if kind == TrackKind::Video && !is_folder {
            ui.menu_button(tr(lang, "Modo de máscara (Pai/Filho)"), |ui| {
                let cur_mask = self.project.tracks[ti].mask_mode;
                for m in neoveg_engine::model::TrackMaskMode::ALL {
                    if ui.selectable_label(cur_mask == m, m.label()).clicked() {
                        self.session.set_track_mask_mode(ti, m);
                        self.sync_after_session();
                        ui.close_menu();
                    }
                }
            });
        }
        if !is_folder {
            ui.menu_button(tr(lang, "Modo de automação"), |ui| {
                let cur_auto = self.project.tracks[ti].automation_mode;
                for m in AutomationMode::ALL {
                    if ui.radio(cur_auto == m, m.label()).clicked() {
                        self.session.set_track_automation_mode(ti, m);
                        self.sync_status_from_session();
                        ui.close_menu();
                    }
                }
            });
        }
        ui.separator();
        if ui
            .button(tr(lang, "Definir propriedades padrão da trilha"))
            .clicked()
        {
            self.session.sel_track = Some(ti);
            self.exec_cmd(Some(ui.ctx()), "track.set_defaults");
            ui.close_menu();
        }
        if ui
            .button(tr(lang, "Renderizar para nova trilha..."))
            .clicked()
        {
            self.session.sel_track = Some(ti);
            self.exec_cmd(Some(ui.ctx()), "track.render_to_new_track");
            ui.close_menu();
        }
        ui.separator();
        if ui.button(tr(lang, "Duplicar trilha")).clicked() {
            self.duplicate_track(ti);
            ui.close_menu();
        }
        if ui.button(tr(lang, "Mover para cima")).clicked() {
            self.move_track(ti, true);
            ui.close_menu();
        }
        if ui.button(tr(lang, "Mover para baixo")).clicked() {
            self.move_track(ti, false);
            ui.close_menu();
        }
        ui.menu_button(tr(lang, "Cor da trilha"), |ui| {
            let mut c = self.project.tracks[ti].color;
            if ui.color_edit_button_srgb(&mut c).changed() {
                self.project.tracks[ti].color = c;
            }
        });
        ui.separator();
        if ui.button(tr(lang, "Excluir trilha")).clicked() {
            self.delete_track(ti);
            ui.close_menu();
        }
    }

    fn bus_menu(&mut self, ui: &mut egui::Ui, bi: usize) {
        let lang = self.lang();
        let Some(bid) = self.project.audio_buses.get(bi).map(|b| b.id) else {
            return;
        };
        if ui.button(tr(lang, "Efeitos do barramento...")).clicked() {
            self.open_fx_target(FxTarget::AudioBus(bid));
            ui.close_menu();
        }
        ui.separator();
        for &k in &[EnvKind::Volume, EnvKind::Pan] {
            let has = self.project.audio_buses[bi].envelope(k).is_some();
            let k_name = k.name_in(lang);
            let label = if has {
                tr_args(lang, "Remover envelope: {kind}", &[("kind", &k_name)])
            } else {
                tr_args(lang, "Inserir envelope: {kind}", &[("kind", &k_name)])
            };
            if ui.button(label).clicked() {
                if self.session.toggle_bus_envelope(bid, k) && !has {
                    self.opts.show_envelopes = true;
                }
                ui.close_menu();
            }
        }
        ui.separator();
        if ui.button(tr(lang, "Excluir barramento")).clicked() {
            self.session.remove_audio_bus(bid);
            ui.close_menu();
        }
    }

    // -----------------------------------------------------------------
    // Interação com eventos
    // -----------------------------------------------------------------

    fn lanes_interact(&mut self, ui: &mut egui::Ui, lanes: Rect, rows: &TimelineRows) {
        let layout = &rows.track_rects;
        let left = lanes.left();
        self.tl.last_lanes = lanes;
        self.tl.last_rows.clone_from(layout);
        let tool = self.session.edit_tool;
        let resp = ui.interact(lanes, ui.id().with("lanes"), Sense::click_and_drag());
        let (mods, scroll, zoom_delta, middle_down, pointer) = ui.input(|i| {
            (
                i.modifiers,
                i.raw_scroll_delta,
                i.zoom_delta(),
                i.pointer.middle_down(),
                i.pointer.hover_pos(),
            )
        });

        // roda do mouse (configurável em Preferências > Edição)
        if let Some(p) = pointer.filter(|p| lanes.contains(*p)) {
            if zoom_delta != 1.0 {
                self.tl.scroll_y = (self.tl.scroll_y - (zoom_delta - 1.0) * 400.0).max(0.0);
            } else if scroll != egui::Vec2::ZERO {
                let d = if scroll.y != 0.0 { scroll.y } else { scroll.x };
                let horiz_mode = self.prefs.timeline_mouse_scroll
                    == neoveg_common::prefs::TimelineMouseScroll::HorizontalScroll;
                if horiz_mode {
                    if mods.command {
                        let c = self.cursor;
                        self.tl
                            .zoom_around_playhead((1.0 + d * 0.0025).clamp(0.5, 2.0), c);
                    } else if mods.shift {
                        self.tl.scroll_y = (self.tl.scroll_y - d).max(0.0);
                    } else {
                        self.tl.scroll_x =
                            (self.tl.scroll_x - self.tl.dx_to_ticks(d)).max(Ticks::ZERO);
                    }
                } else if mods.shift || (scroll.x != 0.0 && scroll.y == 0.0) {
                    self.tl.scroll_x = (self.tl.scroll_x - self.tl.dx_to_ticks(d)).max(Ticks::ZERO);
                } else if mods.command {
                    self.tl.scroll_y = (self.tl.scroll_y - d).max(0.0);
                } else {
                    // como os botões e as setas: a partir da agulha de reprodução
                    let c = self.cursor;
                    self.tl
                        .zoom_around_playhead((1.0 + d * 0.0025).clamp(0.5, 2.0), c);
                }
            }
            // botão do meio: pan
            if middle_down {
                if let Drag::Pan { last } = self.tl.drag {
                    let d = p - last;
                    self.tl.scroll_x =
                        (self.tl.scroll_x - self.tl.dx_to_ticks(d.x)).max(Ticks::ZERO);
                    self.tl.scroll_y = (self.tl.scroll_y - d.y).max(0.0);
                }
                self.tl.drag = Drag::Pan { last: p };
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            } else if matches!(self.tl.drag, Drag::Pan { .. }) {
                self.tl.drag = Drag::None;
            }
        }

        // arrastar e soltar (mídia, efeitos, transições, geradores)
        self.tl.drop_hint = None;
        if let Some(p) = pointer.filter(|p| lanes.contains(*p)) {
            if egui::DragAndDrop::has_any_payload(ui.ctx()) {
                let ti = self
                    .track_at(p.y, &rows.track_spans)
                    .unwrap_or(self.project.tracks.len());
                let t = self.snap(self.tl.x2t(p.x, left), &HashSet::new(), mods.shift);
                self.tl.drop_hint = Some((ti, t));
                let right_release =
                    ui.input(|i| i.pointer.button_released(egui::PointerButton::Secondary));
                let any_release = ui.input(|i| i.pointer.any_released());
                if any_release {
                    if let Some(payload) = egui::DragAndDrop::take_payload::<DndPayload>(ui.ctx()) {
                        let target_event = self
                            .hit_crossfade(p, left, layout)
                            .map(|(_, xf)| xf.right_id)
                            .or_else(|| {
                                self.hit_event(p, left, layout)
                                    .map(|(ti, ei, _)| self.project.tracks[ti].events[ei].id)
                            });
                        if right_release {
                            if let DndPayload::Media(mid) = *payload {
                                self.tl.drop_context_menu = Some(DropContextMenuState {
                                    pos: p,
                                    media_id: mid,
                                    track: self.track_at(p.y, &rows.track_spans),
                                    t,
                                    target_event,
                                });
                            } else {
                                self.handle_drop(
                                    (*payload).clone(),
                                    self.track_at(p.y, &rows.track_spans),
                                    t,
                                    target_event,
                                );
                            }
                        } else {
                            self.handle_drop(
                                (*payload).clone(),
                                self.track_at(p.y, &rows.track_spans),
                                t,
                                target_event,
                            );
                        }
                    }
                }
            }
        }

        let hover = pointer.filter(|p| lanes.contains(*p));
        // Ponteiro vetorial contextual conforme ferramenta, modo e operação ativa.
        match &self.tl.drag {
            Drag::None => {
                if let Some(p) = hover {
                    match tool {
                        EditTool::Selection => {
                            self.set_cursor_shape(crate::cursors::NeovegCursor::SelectBox);
                        }
                        EditTool::Zoom => {
                            self.set_cursor_shape(if mods.shift || mods.alt {
                                crate::cursors::NeovegCursor::ZoomOut
                            } else {
                                crate::cursors::NeovegCursor::ZoomIn
                            });
                        }
                        EditTool::Envelope => {
                            if self.hit_envelope(p, left, rows).is_some() {
                                self.set_cursor_shape(crate::cursors::NeovegCursor::EnvelopeHand);
                            }
                        }
                        EditTool::Normal => {
                            if matches!(self.hit_keyframe(p, left, rows), Some((_, _, Some(_), _)))
                            {
                                self.set_cursor_shape(crate::cursors::NeovegCursor::MoveX);
                            } else if matches!(
                                self.hit_envelope(p, left, rows),
                                Some((_, _, Some(_), _))
                            ) {
                                self.set_cursor_shape(crate::cursors::NeovegCursor::EnvelopeHand);
                            } else if let Some((ti, ei, z)) = self.hit_event(p, left, layout) {
                                let mode = self.session.edit_mode;
                                match z {
                                    Zone::LeftEdge | Zone::RightEdge => {
                                        let cur = if mods.command || mode == EditMode::Stretch {
                                            crate::cursors::NeovegCursor::TimeStretch
                                        } else if mode == EditMode::Roll {
                                            crate::cursors::NeovegCursor::TrimAdjacent
                                        } else if mods.alt || mode == EditMode::Slip {
                                            crate::cursors::NeovegCursor::Slip
                                        } else if z == Zone::LeftEdge {
                                            crate::cursors::NeovegCursor::TrimLeft
                                        } else {
                                            crate::cursors::NeovegCursor::TrimRight
                                        };
                                        self.set_cursor_shape(cur);
                                    }
                                    Zone::FadeIn | Zone::FadeOut => {
                                        self.set_cursor_shape(
                                            crate::cursors::NeovegCursor::FadeOffset,
                                        );
                                    }
                                    Zone::GainHandle | Zone::OpacityHandle => {
                                        self.set_cursor_shape(
                                            crate::cursors::NeovegCursor::EnvelopeHand,
                                        );
                                    }
                                    Zone::FxButton | Zone::PanCropButton | Zone::MoreButton => {
                                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                                    }
                                    Zone::Body => {
                                        if mode == EditMode::SplitTrim
                                            || (mods.command && mods.alt && mods.shift)
                                        {
                                            self.set_cursor_shape(
                                                crate::cursors::NeovegCursor::SplitTrim,
                                            );
                                        } else if mode == EditMode::Slide
                                            || (mods.command && mods.alt)
                                        {
                                            let on_xf =
                                                self.hit_crossfade(p, left, layout).is_some();
                                            self.set_cursor_shape(if on_xf {
                                                crate::cursors::NeovegCursor::SlipCrossfade
                                            } else {
                                                crate::cursors::NeovegCursor::Slide
                                            });
                                        } else if mods.alt || mode == EditMode::Slip {
                                            let pt = self.tl.x2t(p.x, left);
                                            let in_xfade =
                                                self.project.tracks.get(ti).is_some_and(|tr| {
                                                    tr.events.iter().enumerate().any(|(idx, ev)| {
                                                        idx != ei
                                                            && pt >= ev.start
                                                            && pt <= ev.end()
                                                    })
                                                });
                                            self.set_cursor_shape(if in_xfade {
                                                crate::cursors::NeovegCursor::SlipCrossfade
                                            } else {
                                                crate::cursors::NeovegCursor::Slip
                                            });
                                        } else if mode == EditMode::Shuffle {
                                            self.set_cursor_shape(
                                                crate::cursors::NeovegCursor::MoveX,
                                            );
                                        } else {
                                            ui.ctx().set_cursor_icon(CursorIcon::Grab);
                                        }
                                    }
                                }
                            } else if self.hit_envelope(p, left, rows).is_some() {
                                self.set_cursor_shape(crate::cursors::NeovegCursor::EnvelopeHand);
                            }
                        }
                    }
                }
            }
            Drag::Trim { left, slip, .. } => {
                self.set_cursor_shape(if *slip {
                    crate::cursors::NeovegCursor::Slip
                } else if *left {
                    crate::cursors::NeovegCursor::TrimLeft
                } else {
                    crate::cursors::NeovegCursor::TrimRight
                });
            }
            Drag::Stretch { .. } => {
                self.set_cursor_shape(crate::cursors::NeovegCursor::TimeStretch);
            }
            Drag::Roll { .. } => {
                self.set_cursor_shape(crate::cursors::NeovegCursor::TrimAdjacent);
            }
            Drag::SplitTrim { .. } => {
                self.set_cursor_shape(crate::cursors::NeovegCursor::SplitTrim);
            }
            Drag::Fade { .. } => {
                self.set_cursor_shape(crate::cursors::NeovegCursor::FadeOffset);
            }
            Drag::Slip { .. } => {
                self.set_cursor_shape(crate::cursors::NeovegCursor::Slip);
            }
            Drag::Slide { .. } => {
                self.set_cursor_shape(crate::cursors::NeovegCursor::Slide);
            }
            Drag::SlideCrossfade { .. } => {
                self.set_cursor_shape(crate::cursors::NeovegCursor::SlipCrossfade);
            }
            Drag::Gain { .. } | Drag::Opacity { .. } | Drag::EnvPoint { .. } => {
                self.set_cursor_shape(crate::cursors::NeovegCursor::EnvelopeHand);
            }
            Drag::Scrub => {
                self.set_cursor_shape(crate::cursors::NeovegCursor::Scrub);
            }
            Drag::Box { zoom: true, .. } => {
                self.set_cursor_shape(crate::cursors::NeovegCursor::ZoomIn);
            }
            Drag::Box { zoom: false, .. } => {
                self.set_cursor_shape(crate::cursors::NeovegCursor::SelectBox);
            }
            Drag::Move { .. } => {
                self.set_cursor_shape(crate::cursors::NeovegCursor::Move);
            }
            Drag::Shuffle { .. } | Drag::Keyframe { .. } => {
                self.set_cursor_shape(crate::cursors::NeovegCursor::MoveX);
            }
            _ => {}
        }

        if resp.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui
                .input(|i| i.pointer.press_origin())
                .unwrap_or(lanes.center());
            self.start_lane_drag(origin, left, rows, mods);
        }
        if resp.dragged_by(egui::PointerButton::Primary) {
            let sec_pressed =
                ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary));
            if sec_pressed {
                if matches!(self.tl.drag, Drag::TimeSel { .. }) {
                    // Acesso imediato à Ferramenta de Seleção a partir da Normal
                    // (manual do VEGAS Pro 21, p. 238): segurar esquerdo e clicar direito.
                    if let Some((orig_pos, prev_sel)) = self.tl.time_sel_origin.take() {
                        self.time_sel = prev_sel;
                        self.project.loop_region = prev_sel;
                        self.tl.drag = Drag::Box {
                            zoom: false,
                            start: orig_pos,
                        };
                        self.tl.box_cur = resp.interact_pointer_pos().or(Some(orig_pos));
                        self.tl.suppress_ctx_menu = true;
                    }
                } else if matches!(self.tl.drag, Drag::Box { .. }) {
                    // Manual do VEGAS Pro 21, p. 240: clique direito durante a caixa
                    // alterna entre Livre (Both), Horizontal e Vertical.
                    self.session.box_axis = match self.session.box_axis {
                        BoxAxisMode::Both => BoxAxisMode::Horizontal,
                        BoxAxisMode::Horizontal => BoxAxisMode::Vertical,
                        BoxAxisMode::Vertical => BoxAxisMode::Both,
                    };
                    self.tl.suppress_ctx_menu = true;
                }
            }
            if let Some(p) = resp.interact_pointer_pos() {
                self.update_lane_drag(p, left, rows, mods);
                // rolagem automática perto das bordas
                if p.x > lanes.right() - 20.0 {
                    self.tl.scroll_x += self.tl.dx_to_ticks(8.0);
                } else if p.x < lanes.left() + 10.0 && self.tl.scroll_x > Ticks::ZERO {
                    self.tl.scroll_x =
                        (self.tl.scroll_x - self.tl.dx_to_ticks(8.0)).max(Ticks::ZERO);
                }
            }
        }
        if resp.drag_stopped() {
            if let (Drag::Box { zoom, start }, Some(cur)) = (self.tl.drag.clone(), self.tl.box_cur)
            {
                self.finish_box(Rect::from_two_pos(start, cur), zoom, lanes, layout, mods);
            }
            if let Drag::SplitTrim {
                event_id,
                origin_t,
                cur_t,
            } = self.tl.drag.clone()
            {
                self.session.split_trim(event_id, origin_t, cur_t);
                self.sync_after_session();
            }
            self.tl.box_cur = None;
            self.tl.time_sel_origin = None;
            if matches!(
                self.tl.drag,
                Drag::Move { .. }
                    | Drag::Trim { .. }
                    | Drag::Shuffle { .. }
                    | Drag::Slide { .. }
                    | Drag::SlideCrossfade { .. }
                    | Drag::Stretch { .. }
                    | Drag::Roll { .. }
            ) {
                self.project.sort_events();
            }
            self.tl.drag_prev_overlaps.clear();
            if !matches!(self.tl.drag, Drag::Pan { .. }) {
                self.tl.drag = Drag::None;
            }
            self.tl.snap_line = None;
        }

        if resp.clicked() {
            if tool == EditTool::Zoom {
                // ferramenta Zoom: um clique mostra o projeto inteiro
                self.fit_project_to_timeline(lanes.width());
            } else if let Some(p) = resp.interact_pointer_pos() {
                self.lane_click(p, left, rows, mods);
            }
        }
        if resp.double_clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                if let Some((owner, kind, None, t)) = self.hit_envelope(p, left, rows) {
                    if let Some((y0, h)) = rows.row_rect(owner) {
                        self.checkpoint();
                        let v = kind.value_from_norm((y0 + h - 4.0 - p.y) / (h - 8.0));
                        let (def_curve, def_sm) = match owner {
                            EnvOwner::Track(ti) => self
                                .project
                                .tracks
                                .get(ti)
                                .map(|tr| (tr.default_env_curve, tr.default_env_smoothness))
                                .unwrap_or((Curve::Linear, 0.0)),
                            EnvOwner::Bus(bi) => self
                                .project
                                .audio_buses
                                .get(bi)
                                .map(|b| (b.default_env_curve, b.default_env_smoothness))
                                .unwrap_or((Curve::Linear, 0.0)),
                            EnvOwner::MasterVideo => (Curve::Linear, 0.0),
                        };
                        let Some(env) = self
                            .owner_envelopes_mut(owner)
                            .and_then(|list| list.iter_mut().find(|e| e.kind == kind))
                        else {
                            return;
                        };
                        let v = if (Self::env_y(kind, env.eval(t), y0, h) - p.y).abs() < 4.0 {
                            env.eval(t)
                        } else {
                            v
                        };
                        env.points.push(EnvPoint {
                            t,
                            v,
                            curve: def_curve,
                            smoothness: def_sm,
                        });
                        env.sort();
                        self.changed();
                    }
                } else if let Some((ti, ei, zone)) = self.hit_event(p, left, layout) {
                    if zone == Zone::GainHandle {
                        self.checkpoint();
                        self.project.tracks[ti].events[ei].gain_db = 0.0;
                        self.changed();
                    } else if zone == Zone::OpacityHandle {
                        self.checkpoint();
                        self.project.tracks[ti].events[ei].opacity = 1.0;
                        self.changed();
                    } else if matches!(zone, Zone::LeftEdge | Zone::RightEdge)
                        && self.project.tracks[ti].kind == TrackKind::Video
                    {
                        let eid = self.project.tracks[ti].events[ei].id;
                        if self
                            .session
                            .enter_expanded_edit_at(ti, eid, zone == Zone::RightEdge)
                        {
                            self.sync_status_from_session();
                        }
                    } else {
                        let e = &self.project.tracks[ti].events[ei];
                        if let Some(seq_id) = self.nested_sequence_id(e.media_id) {
                            self.switch_sequence_app(seq_id);
                        } else if self.nested_media_path(e.media_id).is_some() {
                            self.open_nested_media_tab(e.media_id);
                        } else {
                            let sel = (e.start, e.end());
                            self.time_sel = Some(sel);
                            self.project.loop_region = Some(sel);
                        }
                    }
                }
            }
        }
        let suppress_ctx = lane_ctx_menu_suppressed(
            resp.context_menu_opened(),
            self.tl.suppress_ctx_menu,
            ui.input(|i| i.pointer.primary_down()),
            matches!(self.tl.drag, Drag::Box { .. }),
        );
        if resp.secondary_clicked() && !suppress_ctx {
            if let Some(p) = resp.interact_pointer_pos() {
                let t = self.tl.x2t(p.x, left);
                self.tl.ctx_target = if let Some(kf) = self.keyframe_ctx(p, left, rows) {
                    kf
                } else if let Some((owner, k, idx, t)) = self.hit_envelope(p, left, rows) {
                    match idx {
                        Some(i) => CtxTarget::EnvPoint(owner, k, i),
                        None => CtxTarget::EnvLine(owner, k, t),
                    }
                } else if let Some((ti, ei, zone)) = self.hit_event(p, left, layout) {
                    let id = self.project.tracks[ti].events[ei].id;
                    let on_ctrl = matches!(
                        zone,
                        Zone::FxButton
                            | Zone::PanCropButton
                            | Zone::MoreButton
                            | Zone::FadeIn
                            | Zone::FadeOut
                    );
                    if !on_ctrl {
                        if let Some((xti, xf)) = self.hit_crossfade(p, left, layout) {
                            if !self.sel_events.contains(&xf.left_id)
                                && !self.sel_events.contains(&xf.right_id)
                            {
                                self.sel_events.clear();
                                self.sel_events.insert(xf.right_id);
                            }
                            self.sel_track = Some(xti);
                            CtxTarget::Crossfade {
                                track_idx: xti,
                                left_id: xf.left_id,
                                right_id: xf.right_id,
                                active: xf.active,
                            }
                        } else {
                            if !self.sel_events.contains(&id) {
                                self.sel_events.clear();
                                self.sel_events.insert(id);
                            }
                            self.sel_track = Some(ti);
                            CtxTarget::Event(id)
                        }
                    } else {
                        if !self.sel_events.contains(&id) {
                            self.sel_events.clear();
                            self.sel_events.insert(id);
                        }
                        self.sel_track = Some(ti);
                        if matches!(zone, Zone::FadeIn | Zone::FadeOut) {
                            CtxTarget::EventFade(id, zone == Zone::FadeIn)
                        } else {
                            CtxTarget::Event(id)
                        }
                    }
                } else if let Some(ti) = self.track_at(p.y, layout) {
                    self.sel_track = Some(ti);
                    CtxTarget::Lane(ti, t)
                } else {
                    CtxTarget::None
                };
            }
        }
        if !suppress_ctx {
            resp.context_menu(|ui| self.lane_context_menu(ui));
        } else if !ui.input(|i| i.pointer.primary_down() || i.pointer.secondary_down()) {
            self.tl.suppress_ctx_menu = false;
        }
    }

    /// Desenha sobreposições de arrasto (`Drag::Box` das ferramentas Seleção/Zoom
    /// e corte `Drag::SplitTrim`) DEPOIS de `draw_lanes` para permanecerem sempre
    /// visíveis acima das faixas e dos eventos (manual do VEGAS Pro 21, pp. 239–241).
    fn draw_lane_drag_overlays(&self, ui: &egui::Ui, lanes: Rect, rows: &TimelineRows) {
        let left = lanes.left();
        let layout = &rows.track_rects;
        if let (Drag::Box { zoom, start }, Some(cur)) = (&self.tl.drag, self.tl.box_cur) {
            let r = self
                .effective_box_rect(Rect::from_two_pos(*start, cur), lanes)
                .intersect(lanes);
            if r.width() > 0.5 || r.height() > 0.5 {
                let p = ui.painter().with_clip_rect(lanes);
                let axis = self.session.box_axis;
                // Destaque ao vivo dos eventos alcançados pelo retângulo de seleção
                if !*zoom {
                    let hit_stroke = Stroke::new(2.0_f32, self.theme.tl_event_sel_border);
                    for (ti, tr) in self.project.tracks.iter().enumerate() {
                        let Some(&(y0, h)) = layout.get(ti) else {
                            continue;
                        };
                        if axis == BoxAxisMode::Vertical && (y0 + h < r.top() || y0 > r.bottom()) {
                            continue;
                        }
                        for e in &tr.events {
                            let er = self.event_rect(e, left, y0, h);
                            if axis == BoxAxisMode::Vertical || er.intersects(r) {
                                p.rect_stroke(er, 2.0, hit_stroke, StrokeKind::Inside);
                            }
                        }
                    }
                }
                let fill = if *zoom {
                    self.theme.accent.gamma_multiply(0.18)
                } else {
                    self.theme.selection_bg.gamma_multiply(0.22)
                };
                p.rect_filled(r, 0.0, fill);
                let corners = [
                    r.left_top(),
                    r.right_top(),
                    r.right_bottom(),
                    r.left_bottom(),
                    r.left_top(),
                ];
                let backing = if self.theme.dark_mode {
                    Color32::from_black_alpha(180)
                } else {
                    Color32::from_white_alpha(200)
                };
                p.rect_stroke(r, 0.0, Stroke::new(1.0_f32, backing), StrokeKind::Inside);
                let fg = if *zoom || self.theme.high_contrast {
                    self.theme.accent
                } else {
                    self.theme.text_primary
                };
                p.extend(Shape::dashed_line(
                    &corners,
                    Stroke::new(1.5_f32, fg),
                    5.0,
                    3.0,
                ));
                if axis != BoxAxisMode::Both {
                    let badge = axis.name();
                    let g = p.layout_no_wrap(
                        badge.to_string(),
                        FontId::proportional(10.0),
                        self.theme.text_primary,
                    );
                    let pos = pos2(
                        (r.left() + 4.0).min((lanes.right() - g.size().x - 10.0).max(lanes.left())),
                        (r.top() + 4.0).min((lanes.bottom() - g.size().y - 6.0).max(lanes.top())),
                    );
                    let bg = Rect::from_min_size(pos, g.size() + vec2(6.0, 3.0));
                    p.rect_filled(bg, 2.0, self.theme.extreme_bg.gamma_multiply(0.9));
                    p.rect_stroke(
                        bg,
                        2.0,
                        Stroke::new(1.0_f32, self.theme.border),
                        StrokeKind::Inside,
                    );
                    p.galley(bg.min + vec2(3.0, 1.5), g, self.theme.text_primary);
                }
            }
        }
        if let Drag::SplitTrim {
            event_id,
            origin_t,
            cur_t,
        } = &self.tl.drag
        {
            if let Some((ti, _)) = self.project.find_event(*event_id) {
                if let Some(&(y0, h)) = layout.get(ti) {
                    let lo = origin_t.min(cur_t);
                    let hi = origin_t.max(cur_t);
                    let x0 = self.tl.t2x(*lo, left).max(lanes.left());
                    let x1 = self.tl.t2x(*hi, left).min(lanes.right());
                    if x1 > x0 {
                        let cut_rect = Rect::from_min_max(pos2(x0, y0), pos2(x1, y0 + h));
                        ui.painter().with_clip_rect(lanes).rect_filled(
                            cut_rect,
                            0.0,
                            self.theme.offline_fg.gamma_multiply(0.35),
                        );
                    }
                }
            }
        }
    }

    fn start_lane_drag(
        &mut self,
        origin: Pos2,
        left: f32,
        rows: &TimelineRows,
        mods: egui::Modifiers,
    ) {
        let layout = &rows.track_rects;
        let ot = self.tl.x2t(origin.x, left);
        let tool = self.session.edit_tool;
        // Seleção e Zoom desenham uma caixa e não mexem em nada (p. 239)
        if matches!(tool, EditTool::Selection | EditTool::Zoom) {
            self.tl.drag = Drag::Box {
                zoom: tool == EditTool::Zoom,
                start: origin,
            };
            self.tl.box_cur = Some(origin);
            return;
        }
        let slip_mode = tool == EditTool::Normal && self.session.edit_mode == EditMode::Slip;
        if let Some((ti, row, Some(key), _)) = self.hit_keyframe(origin, left, rows) {
            if let Some(tr) = self.project.tracks.get(ti).filter(|tr| !tr.locked) {
                self.tl.drag = Drag::Keyframe {
                    track: tr.id,
                    row: row.kind,
                    at: key,
                    moved: false,
                };
            }
            return;
        }
        if let Some((owner, kind, Some(idx), _)) = self.hit_envelope(origin, left, rows) {
            self.checkpoint();
            self.tl.drag = Drag::EnvPoint { owner, kind, idx };
            return;
        }
        if let Some((ti, ei, zone)) = self.hit_event(origin, left, layout) {
            // ferramenta Envelope: eventos não se movem nem são aparados
            // (p. 239); a volta acontece antes de qualquer checkpoint
            if tool == EditTool::Envelope {
                return;
            }
            let id = self.project.tracks[ti].events[ei].id;
            if self.project.tracks[ti].events[ei].locked || self.project.tracks[ti].locked {
                return;
            }
            self.sel_track = Some(ti);
            match zone {
                Zone::FadeIn | Zone::FadeOut => {
                    self.checkpoint();
                    self.tl.drag = Drag::Fade {
                        id,
                        fade_in: zone == Zone::FadeIn,
                    };
                }
                Zone::GainHandle => {
                    let orig_db = self.project.tracks[ti].events[ei].gain_db;
                    let event_h = layout[ti].1.max(18.0);
                    self.checkpoint();
                    self.tl.drag = Drag::Gain {
                        id,
                        orig_db,
                        origin_y: origin.y,
                        event_h,
                    };
                }
                Zone::OpacityHandle => {
                    let orig_opacity = self.project.tracks[ti].events[ei].opacity;
                    let event_h = layout[ti].1.max(18.0);
                    self.checkpoint();
                    self.tl.drag = Drag::Opacity {
                        id,
                        orig_opacity,
                        origin_y: origin.y,
                        event_h,
                    };
                }
                Zone::LeftEdge | Zone::RightEdge => {
                    self.tl.drag_prev_overlaps =
                        neoveg_engine::crossfade::overlap_pairs(&self.project.tracks);
                    self.checkpoint();
                    if !self.sel_events.contains(&id) {
                        self.sel_events.clear();
                        self.sel_events.insert(id);
                    }
                    let e = &self.project.tracks[ti].events[ei];
                    let left_edge = zone == Zone::LeftEdge;
                    let edge_t = if left_edge { e.start } else { e.end() };
                    let mode = self.session.edit_mode;
                    let mut ids: HashSet<Id> = HashSet::new();
                    ids.insert(id);
                    let group_ids = self.expand_groups(&ids);

                    // Ctrl+borda ou modo Stretch esticam a velocidade (p. 236).
                    if mods.command || mode == EditMode::Stretch {
                        let mut snaps = vec![];
                        for gid in group_ids {
                            if let Some(ge) = self.project.event(gid) {
                                let ge_edge = if left_edge { ge.start } else { ge.end() };
                                if !ge.locked
                                    && ge_edge.abs_diff(edge_t) < Ticks::from_millis_i64(2)
                                {
                                    snaps.push(StretchEventSnapshot::capture(ge));
                                }
                            }
                        }
                        self.tl.drag = Drag::Stretch {
                            left: left_edge,
                            origin_t: ot,
                            anchor_id: id,
                            snaps,
                        };
                        return;
                    }

                    // Modo Roll ajusta em conjunto dois eventos contíguos no ponto de corte.
                    if mode == EditMode::Roll {
                        let snap = self.capture_track_snapshot(ti);
                        let tol = Ticks::from_millis_i64(2);
                        let has_neighbor = snap.iter().any(|s| {
                            s.id != id
                                && if left_edge {
                                    ((s.start + s.length) - edge_t).abs() <= tol
                                } else {
                                    (s.start - edge_t).abs() <= tol
                                }
                        });
                        if has_neighbor {
                            self.tl.drag = Drag::Roll {
                                anchor_id: id,
                                track_idx: ti,
                                left_edge,
                                origin_t: ot,
                                orig_edge: edge_t,
                                snap,
                            };
                            return;
                        }
                    }

                    let mut orig = vec![];
                    for gid in group_ids {
                        if let Some(ge) = self.project.event(gid) {
                            let ge_edge = if left_edge { ge.start } else { ge.end() };
                            if ge_edge.abs_diff(edge_t) < Ticks::from_millis_i64(1) {
                                orig.push((gid, ge.start, ge.length, ge.offset));
                            }
                        }
                    }
                    self.tl.drag = Drag::Trim {
                        left: left_edge,
                        slip: mods.alt || slip_mode,
                        origin_t: ot,
                        anchor_id: id,
                        orig,
                    };
                }
                Zone::FxButton | Zone::PanCropButton | Zone::MoreButton => {}
                Zone::Body => {
                    let mode = self.session.edit_mode;
                    if mode == EditMode::SplitTrim || (mods.command && mods.alt && mods.shift) {
                        if !self.sel_events.contains(&id) {
                            self.sel_events.clear();
                            self.sel_events.insert(id);
                        }
                        let snapped = self.snap(ot, &HashSet::new(), mods.shift);
                        self.tl.drag = Drag::SplitTrim {
                            event_id: id,
                            origin_t: snapped,
                            cur_t: snapped,
                        };
                        return;
                    }
                    self.tl.drag_prev_overlaps =
                        neoveg_engine::crossfade::overlap_pairs(&self.project.tracks);
                    self.checkpoint();
                    if !self.sel_events.contains(&id) {
                        if !mods.command && !mods.shift {
                            self.sel_events.clear();
                        }
                        self.sel_events.insert(id);
                    }
                    let ids = self.expand_groups(&self.sel_events.clone());
                    // Ctrl+Alt+arrastar ou modo Slide sobre um crossfade desliza a janela de crossfade.
                    if (mods.command && mods.alt) || mode == EditMode::Slide {
                        if let Some((xti, xf)) = self.hit_crossfade(origin, left, layout) {
                            let pairs = neoveg_engine::crossfade::capture_slide_crossfade(
                                &self.project,
                                xti,
                                xf.left_id,
                                xf.right_id,
                                self.opts.ignore_grouping,
                            );
                            if !pairs.is_empty() {
                                self.tl.drag = Drag::SlideCrossfade {
                                    origin_t: ot,
                                    anchor_start: xf.start,
                                    pairs,
                                };
                                return;
                            }
                        }
                        let snap = self.capture_track_snapshot(ti);
                        let anchor_orig_start = self.project.tracks[ti].events[ei].start;
                        let partners = ids
                            .iter()
                            .filter(|pid| !snap.iter().any(|s| s.id == **pid))
                            .filter_map(|pid| self.project.event(*pid).map(|ev| (*pid, ev.start)))
                            .collect();
                        let masters: HashSet<Id> = snap
                            .iter()
                            .map(|s| s.id)
                            .chain(ids.iter().copied())
                            .collect();
                        let sync_followers = self.session.sync_followers_for(&masters);
                        self.tl.drag = Drag::Slide {
                            anchor_id: id,
                            track_idx: ti,
                            origin_t: ot,
                            anchor_orig_start,
                            snap,
                            partners,
                            sync_followers,
                        };
                        return;
                    }
                    // Alt, ou a ferramenta Normal no modo Slip (p. 236)
                    if mods.alt || slip_mode {
                        let orig = ids
                            .iter()
                            .filter_map(|i| self.project.event(*i).map(|e| (*i, e.offset)))
                            .collect();
                        self.tl.drag = Drag::Slip { origin_t: ot, orig };
                        return;
                    }
                    // Modo Shuffle reordena eventos na trilha mantendo sequência contínua.
                    if mode == EditMode::Shuffle && !mods.command {
                        let snap = self.capture_track_snapshot(ti);
                        if snap.len() >= 2 {
                            let anchor_orig_start = self.project.tracks[ti].events[ei].start;
                            let partners = ids
                                .iter()
                                .filter(|pid| !snap.iter().any(|s| s.id == **pid))
                                .filter_map(|pid| {
                                    self.project.event(*pid).map(|ev| (*pid, ev.start))
                                })
                                .collect();
                            let masters: HashSet<Id> = snap
                                .iter()
                                .map(|s| s.id)
                                .chain(ids.iter().copied())
                                .collect();
                            let sync_followers = self.session.sync_followers_for(&masters);
                            self.tl.drag = Drag::Shuffle {
                                anchor_id: id,
                                track_idx: ti,
                                origin_t: ot,
                                anchor_orig_start,
                                snap,
                                partners,
                                sync_followers,
                            };
                            return;
                        }
                    }
                    let mut anchor = id;
                    if mods.command {
                        // Ctrl+arrastar: copia os eventos
                        let mut gm = std::collections::HashMap::new();
                        let mut new_sel = HashSet::new();
                        for i in &ids {
                            let Some((t, e)) = self.project.find_event(*i) else {
                                continue;
                            };
                            let mut c = self.project.tracks[t].events[e].clone();
                            c.id = new_id();
                            c.group = c.group.map(|g| *gm.entry(g).or_insert_with(new_id));
                            c.sync_link_master = None;
                            c.fx = c.fx.iter().map(|f| f.duplicate()).collect();
                            if *i == id {
                                anchor = c.id;
                            }
                            new_sel.insert(c.id);
                            self.project.tracks[t].events.push(c);
                        }
                        self.sel_events = new_sel.clone();
                        self.changed();
                        let orig = new_sel
                            .iter()
                            .filter_map(|i| {
                                self.project
                                    .find_event(*i)
                                    .map(|(t, e)| (*i, self.project.tracks[t].events[e].start, t))
                            })
                            .collect();
                        let anchor_start = self
                            .project
                            .event(anchor)
                            .map(|e| e.start)
                            .unwrap_or(Ticks::ZERO);
                        self.tl.drag = Drag::Move {
                            anchor_id: anchor,
                            anchor_start,
                            origin_t: ot,
                            origin_track: ti,
                            orig,
                            sync_followers: Vec::new(),
                            // cópia: a automação fica com os originais
                            env_lock: Vec::new(),
                        };
                        return;
                    }
                    let orig = ids
                        .iter()
                        .filter_map(|i| {
                            self.project
                                .find_event(*i)
                                .map(|(t, e)| (*i, self.project.tracks[t].events[e].start, t))
                        })
                        .collect();
                    let sync_followers = self.session.sync_followers_for(&ids);
                    let anchor_start = self.project.tracks[ti].events[ei].start;
                    let env_lock = if self.opts.lock_envelopes {
                        self.capture_env_lock(&ids)
                    } else {
                        Vec::new()
                    };
                    self.tl.drag = Drag::Move {
                        anchor_id: anchor,
                        anchor_start,
                        origin_t: ot,
                        origin_track: ti,
                        orig,
                        sync_followers,
                        env_lock,
                    };
                }
            }
            return;
        }
        // área vazia: seleção de tempo (clique direito enquanto arrasta alterna para
        // caixa de seleção, manual do VEGAS Pro 21, p. 238)
        let s = self.snap(ot, &HashSet::new(), mods.shift);
        if let Some(ti) = self.track_at(origin.y, &rows.track_spans) {
            self.sel_track = Some(ti);
        }
        self.tl.time_sel_origin = Some((origin, self.time_sel));
        self.tl.drag = Drag::TimeSel { anchor: s };
    }

    fn update_lane_drag(&mut self, p: Pos2, left: f32, rows: &TimelineRows, mods: egui::Modifiers) {
        let t = self.tl.x2t(p.x, left);
        if matches!(self.tl.drag, Drag::Box { .. }) {
            self.tl.box_cur = Some(p);
            return;
        }
        match self.tl.drag.clone() {
            Drag::Keyframe {
                track,
                row,
                at,
                moved,
            } => {
                let target = self.snap(t, &HashSet::new(), mods.shift);
                if let Some(new_at) = self.drag_keyframe(track, &row, at, moved, target) {
                    self.tl.drag = Drag::Keyframe {
                        track,
                        row,
                        at: new_at,
                        moved: true,
                    };
                }
            }
            Drag::TimeSel { anchor } => {
                let s = self.snap(t, &HashSet::new(), mods.shift);
                let sel = (anchor.min(s), anchor.max(s));
                if sel.1 - sel.0 > Ticks::ZERO {
                    self.time_sel = Some(sel);
                    self.project.loop_region = Some(sel);
                }
                self.cursor = sel.0;
            }
            Drag::EnvPoint { owner, kind, idx } => {
                let Some((y0, h)) = rows.row_rect(owner) else {
                    return;
                };
                let v = kind.value_from_norm((y0 + h - 4.0 - p.y) / (h - 8.0));
                let snapped = self.snap(t, &HashSet::new(), mods.shift);
                let env = self
                    .owner_envelopes_mut(owner)
                    .and_then(|list| list.iter_mut().find(|e| e.kind == kind));
                if let Some(env) = env {
                    let gap = Ticks::from_raw(TICKS_PER_MS / 10);
                    let lo = if idx > 0 {
                        env.points[idx - 1].t + gap
                    } else {
                        Ticks::ZERO
                    };
                    let hi = env
                        .points
                        .get(idx + 1)
                        .map(|p| p.t - gap)
                        .unwrap_or(Ticks::MAX);
                    if let Some(pt) = env.points.get_mut(idx) {
                        if !mods.command {
                            pt.t = if idx == 0 && env_first_locked() {
                                Ticks::ZERO
                            } else {
                                snapped.clamp_safe(lo, hi)
                            };
                        }
                        if !mods.alt {
                            pt.v = v;
                        }
                    }
                }
                self.changed();
            }
            Drag::Fade { id, fade_in } => {
                if let Some(e) = self.project.event_mut(id) {
                    if fade_in {
                        e.fade_in.len =
                            (t - e.start).clamp_safe(Ticks::ZERO, e.length - e.fade_out.len);
                    } else {
                        e.fade_out.len =
                            (e.end() - t).clamp_safe(Ticks::ZERO, e.length - e.fade_in.len);
                    }
                }
                self.changed();
            }
            Drag::Gain {
                id,
                orig_db,
                origin_y,
                event_h,
            } => {
                let dy = p.y - origin_y;
                let span = (event_h - 10.0).max(20.0);
                let raw = (orig_db - (dy / span) * 36.0).clamp(-36.0, 12.0);
                let snapped = if raw.abs() < 0.4 {
                    0.0
                } else {
                    (raw * 10.0).round() / 10.0
                };
                if let Some(e) = self.project.event_mut(id) {
                    e.gain_db = snapped;
                }
                self.changed();
            }
            Drag::Opacity {
                id,
                orig_opacity,
                origin_y,
                event_h,
            } => {
                let dy = p.y - origin_y;
                let span = (event_h - 10.0).max(20.0);
                let raw = (orig_opacity - dy / span).clamp(0.0, 1.0);
                let snapped = if (1.0 - raw).abs() < 0.015 {
                    1.0
                } else if raw < 0.015 {
                    0.0
                } else {
                    (raw * 100.0).round() / 100.0
                };
                if let Some(e) = self.project.event_mut(id) {
                    e.opacity = snapped;
                }
                self.changed();
            }
            Drag::Slip { origin_t, orig } => {
                let dt = t - origin_t;
                let slip_all_takes = self.opts.slip_all_takes;
                let durations: std::collections::HashMap<Id, Option<Ticks>> = self
                    .project
                    .media
                    .iter()
                    .map(|m| (m.id, m.duration))
                    .collect();
                for (id, off) in orig {
                    let dur = self
                        .project
                        .event(id)
                        .and_then(|e| self.project.media(e.media_id))
                        .and_then(|m| m.duration);
                    if let Some(e) = self.project.event_mut(id) {
                        let before = e.offset;
                        let mut o = off - e.rate.scale_ticks(dt);
                        if let (Some(d), false) = (dur, e.looped) {
                            let max_o = (d - e.rate.scale_ticks(e.length)).max(Ticks::ZERO);
                            o = o.clamp_safe(Ticks::ZERO, max_o);
                        }
                        e.offset = if e.looped { o } else { o.max(Ticks::ZERO) };
                        if e.looped {
                            if let Some(d) = dur {
                                if d > Ticks::ZERO {
                                    e.offset = e.offset.rem_euclid(d);
                                }
                            }
                        }
                        if slip_all_takes {
                            let delta = e.offset - before;
                            neoveg_engine::takes::slip_alt_takes(e, delta, |m| {
                                durations.get(&m).copied().flatten()
                            });
                        }
                    }
                }
                self.changed();
            }
            Drag::Trim {
                left: is_left,
                slip,
                origin_t,
                anchor_id,
                orig,
            } => {
                let exclude: HashSet<Id> = orig.iter().map(|o| o.0).collect();
                let Some(&(_, a_start, a_len, _)) = orig.iter().find(|o| o.0 == anchor_id) else {
                    return;
                };
                let fd = self.project.settings.frame_dur();
                let raw_edge = if is_left { a_start } else { a_start + a_len } + (t - origin_t);
                let edge = self.snap(raw_edge, &exclude, mods.shift);
                let mut dt = edge - if is_left { a_start } else { a_start + a_len };
                // limites para todos os eventos do grupo
                for &(id, st, len, off) in &orig {
                    let Some(e) = self.project.event(id) else {
                        continue;
                    };
                    let dur = self.project.media(e.media_id).and_then(|m| m.duration);
                    let rate = e.rate;
                    if is_left {
                        dt = dt.min(len - fd);
                        dt = dt.max(-st);
                        if !e.looped && !slip && !e.reverse {
                            if let Some(max_back) = rate.unscale_ticks(off) {
                                dt = dt.max(-max_back);
                            }
                        }
                    } else {
                        dt = dt.max(-(len - fd));
                        if let (Some(d), false, false) = (dur, e.looped, slip) {
                            if !e.reverse {
                                if let Some(avail) = rate.unscale_ticks((d - off).max(Ticks::ZERO))
                                {
                                    dt = dt.min((avail - len).max(-(len - fd)));
                                }
                            }
                        }
                    }
                }
                let slip_all_takes = self.opts.slip_all_takes;
                let durations: std::collections::HashMap<Id, Option<Ticks>> = self
                    .project
                    .media
                    .iter()
                    .map(|m| (m.id, m.duration))
                    .collect();
                for &(id, st, len, off) in &orig {
                    if let Some(e) = self.project.event_mut(id) {
                        if is_left {
                            e.start = st + dt;
                            e.length = (len - dt).max(fd);
                            if !slip && !e.reverse {
                                // todos os takes acompanham o início do evento
                                let delta = off + e.rate.scale_ticks(dt) - e.offset;
                                e.shift_offsets(delta);
                            }
                        } else {
                            e.length = (len + dt).max(fd);
                            if slip {
                                let new_off = (off - e.rate.scale_ticks(dt)).max(Ticks::ZERO);
                                let delta = new_off - e.offset;
                                e.offset = new_off;
                                if slip_all_takes {
                                    neoveg_engine::takes::slip_alt_takes(e, delta, |m| {
                                        durations.get(&m).copied().flatten()
                                    });
                                }
                            }
                        }
                        e.fade_in.len = e.fade_in.len.min(e.length);
                        e.fade_out.len = e.fade_out.len.min(e.length - e.fade_in.len);
                    }
                }
                let all_tracks: Vec<usize> = (0..self.project.tracks.len()).collect();
                let prev = self.tl.drag_prev_overlaps.clone();
                self.session.sync_overlap_crossfades(&all_tracks, &prev);
                self.changed();
            }
            Drag::Move {
                anchor_id,
                anchor_start,
                origin_t,
                origin_track,
                orig,
                sync_followers,
                env_lock,
            } => {
                let exclude: HashSet<Id> = orig.iter().map(|o| o.0).collect();
                let raw = anchor_start + (t - origin_t);
                // encaixa o início ou o fim do evento âncora
                let anchor_len = self
                    .project
                    .event(anchor_id)
                    .map(|e| e.length)
                    .unwrap_or(Ticks::ZERO);
                let s1 = self.snap(raw, &exclude, mods.shift);
                let line1 = self.tl.snap_line;
                let s2 = self.snap(raw + anchor_len, &exclude, mods.shift) - anchor_len;
                let line2 = self.tl.snap_line;
                let ns = if line1.is_some()
                    && (line2.is_none() || (s1 - raw).abs() <= (s2 - raw).abs())
                {
                    self.tl.snap_line = line1;
                    s1
                } else if line2.is_some() {
                    self.tl.snap_line = line2;
                    s2
                } else {
                    s1
                };
                let min_start = orig.iter().map(|o| o.1).fold(Ticks::MAX, Ticks::min);
                let dt = (ns - anchor_start).max(Ticks::ZERO - min_start);
                // deslocamento vertical entre trilhas do mesmo tipo
                let target = self
                    .track_at(p.y, &rows.track_spans)
                    .unwrap_or(origin_track);
                let dtrack = target as isize - origin_track as isize;
                let n = self.project.tracks.len() as isize;
                let valid = orig.iter().all(|&(id, _, ti)| {
                    let nt = ti as isize + dtrack;
                    let kind = self
                        .project
                        .find_event(id)
                        .map(|(t, _)| self.project.tracks[t].kind);
                    nt >= 0
                        && nt < n
                        && kind.is_some_and(|k| self.project.tracks[nt as usize].accepts(k))
                });
                let dtrack = if valid && !mods.alt { dtrack } else { 0 };
                for &(id, st, ti) in &orig {
                    let Some((cur_t, cur_e)) = self.project.find_event(id) else {
                        continue;
                    };
                    let new_t = (ti as isize + dtrack) as usize;
                    // grupos áudio/vídeo: só move de trilha os eventos do tipo da âncora
                    let new_t =
                        if self.project.tracks[new_t].accepts(self.project.tracks[cur_t].kind) {
                            new_t
                        } else {
                            cur_t
                        };
                    if new_t != cur_t {
                        let ev = self.project.tracks[cur_t].events.remove(cur_e);
                        self.project.tracks[new_t].events.push(ev);
                    }
                    if let Some(e) = self.project.event_mut(id) {
                        e.start = st + dt;
                    }
                }
                for &(fid, f_orig) in &sync_followers {
                    if let Some(fe) = self.project.event_mut(fid) {
                        fe.start = (f_orig + dt).max(Ticks::ZERO);
                    }
                }
                // a automação só acompanha quando os eventos ficam na mesma
                // trilha; trocando de trilha, ela volta ao lugar
                let env_dt = if dtrack == 0 { dt } else { Ticks::ZERO };
                for (track_id, snap) in &env_lock {
                    if let Some(tr) = self.project.tracks.iter_mut().find(|t| t.id == *track_id) {
                        snap.apply(tr, env_dt);
                    }
                }
                let all_tracks: Vec<usize> = (0..self.project.tracks.len()).collect();
                let prev = self.tl.drag_prev_overlaps.clone();
                self.session.sync_overlap_crossfades(&all_tracks, &prev);
                self.changed();
            }
            Drag::Shuffle {
                anchor_id,
                track_idx,
                origin_t,
                anchor_orig_start,
                snap,
                partners,
                sync_followers,
            } => {
                let target_t = anchor_orig_start + (t - origin_t);
                if apply_shuffle_from_snapshot(
                    &mut self.project,
                    track_idx,
                    &snap,
                    anchor_id,
                    target_t,
                    &partners,
                ) {
                    let actual_dt = self
                        .project
                        .event(anchor_id)
                        .map(|e| e.start - anchor_orig_start)
                        .unwrap_or(Ticks::ZERO);
                    for &(fid, f_orig) in &sync_followers {
                        if let Some(fe) = self.project.event_mut(fid) {
                            fe.start = (f_orig + actual_dt).max(Ticks::ZERO);
                        }
                    }
                    self.changed();
                }
            }
            Drag::Slide {
                anchor_id,
                track_idx,
                origin_t,
                anchor_orig_start,
                snap,
                partners,
                sync_followers,
            } => {
                let exclude: HashSet<Id> = snap.iter().map(|s| s.id).collect();
                let raw = anchor_orig_start + (t - origin_t);
                let snapped = self.snap(raw, &exclude, mods.shift);
                let dt = snapped - anchor_orig_start;
                let fd = self.project.settings.frame_dur();
                if apply_slide_from_snapshot(
                    &mut self.project,
                    track_idx,
                    &snap,
                    anchor_id,
                    dt,
                    fd,
                    &partners,
                ) {
                    let actual_dt = self
                        .project
                        .event(anchor_id)
                        .map(|e| e.start - anchor_orig_start)
                        .unwrap_or(Ticks::ZERO);
                    for &(fid, f_orig) in &sync_followers {
                        if let Some(fe) = self.project.event_mut(fid) {
                            fe.start = (f_orig + actual_dt).max(Ticks::ZERO);
                        }
                    }
                    self.changed();
                }
            }
            Drag::SlideCrossfade {
                origin_t,
                anchor_start,
                pairs,
            } => {
                let mut exclude: HashSet<Id> = HashSet::new();
                for p in &pairs {
                    exclude.insert(p.left_id);
                    exclude.insert(p.right_id);
                }
                let raw = anchor_start + (t - origin_t);
                let snapped = self.snap(raw, &exclude, mods.shift);
                let dt = snapped - anchor_start;
                let fd = self.project.settings.frame_dur();
                if neoveg_engine::crossfade::apply_slide_crossfade(
                    &mut self.project,
                    &pairs,
                    dt,
                    fd,
                ) {
                    self.changed();
                }
            }
            Drag::Stretch {
                left,
                origin_t,
                anchor_id,
                snaps,
            } => {
                let exclude: HashSet<Id> = snaps.iter().map(|s| s.id).collect();
                let Some(a) = snaps.iter().find(|s| s.id == anchor_id) else {
                    return;
                };
                let orig_edge = if left { a.start } else { a.start + a.length };
                let target_edge = self.snap(orig_edge + (t - origin_t), &exclude, mods.shift);
                let dt = target_edge - orig_edge;
                let fd = self.project.settings.frame_dur();
                if apply_stretch_from_snapshot(&mut self.project, &snaps, anchor_id, left, dt, fd) {
                    self.changed();
                }
            }
            Drag::Roll {
                anchor_id,
                track_idx,
                left_edge,
                origin_t,
                orig_edge,
                snap,
            } => {
                let exclude: HashSet<Id> = snap.iter().map(|s| s.id).collect();
                let target_edge = self.snap(orig_edge + (t - origin_t), &exclude, mods.shift);
                let dt = target_edge - orig_edge;
                let fd = self.project.settings.frame_dur();
                if apply_roll_from_snapshot(
                    &mut self.project,
                    track_idx,
                    &snap,
                    anchor_id,
                    left_edge,
                    dt,
                    fd,
                ) {
                    self.changed();
                }
            }
            Drag::SplitTrim {
                event_id, origin_t, ..
            } => {
                let cur_t = self.snap(t, &HashSet::new(), mods.shift);
                self.tl.drag = Drag::SplitTrim {
                    event_id,
                    origin_t,
                    cur_t,
                };
            }
            _ => {}
        }
    }

    /// Foto da automação das trilhas de origem dos eventos `ids`, com o
    /// trecho de cada evento (Travar envelopes nos eventos).
    pub(crate) fn capture_env_lock<'a>(
        &self,
        ids: impl IntoIterator<Item = &'a Id>,
    ) -> Vec<(Id, neoveg_engine::env_lock::AutomationSnapshot)> {
        let mut by_track: Vec<(usize, Vec<(Ticks, Ticks)>)> = Vec::new();
        for id in ids {
            let Some((ti, ei)) = self.project.find_event(*id) else {
                continue;
            };
            let Some(e) = self.project.tracks.get(ti).and_then(|t| t.events.get(ei)) else {
                continue;
            };
            match by_track.iter_mut().find(|(t, _)| *t == ti) {
                Some((_, spans)) => spans.push((e.start, e.length)),
                None => by_track.push((ti, vec![(e.start, e.length)])),
            }
        }
        by_track
            .into_iter()
            .filter_map(|(ti, spans)| {
                let tr = self.project.tracks.get(ti)?;
                Some((
                    tr.id,
                    neoveg_engine::env_lock::AutomationSnapshot::capture(tr, &spans),
                ))
            })
            .collect()
    }

    fn capture_track_snapshot(&self, track_idx: usize) -> Vec<TrackEventSnapshot> {
        self.project
            .tracks
            .get(track_idx)
            .map(|tr| {
                tr.events
                    .iter()
                    .filter(|e| !e.locked)
                    .map(|e| TrackEventSnapshot {
                        id: e.id,
                        track_idx,
                        start: e.start,
                        length: e.length,
                        offset: e.offset,
                        rate: e.rate,
                        reverse: e.reverse,
                        looped: e.looped,
                        media_dur: self.project.media(e.media_id).and_then(|m| m.duration),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn lane_click(&mut self, p: Pos2, left: f32, rows: &TimelineRows, mods: egui::Modifiers) {
        let layout = &rows.track_rects;
        let t = self.tl.x2t(p.x, left);
        if let Some(ti) = self.track_at(p.y, &rows.track_spans) {
            self.sel_track = Some(ti);
        }
        if let Some((ti, ei, zone)) = self.hit_event(p, left, layout) {
            let id = self.project.tracks[ti].events[ei].id;
            if self.session.edit_tool == EditTool::Normal
                && self.session.edit_mode == EditMode::SplitTrim
                && matches!(zone, Zone::Body)
            {
                let snapped = self.snap(t, &HashSet::new(), mods.shift);
                self.session.split_trim(id, snapped, snapped);
                self.sync_after_session();
                return;
            }
            match zone {
                Zone::FxButton => {
                    self.open_fx_target(FxTarget::Event(id));
                    return;
                }
                Zone::PanCropButton => {
                    self.dlg.pan_crop = Some(id);
                    return;
                }
                Zone::MoreButton => {
                    self.event_buttons_dlg_video =
                        Some(self.project.tracks[ti].kind == TrackKind::Video);
                    return;
                }
                _ => {}
            }
            if mods.command {
                if !self.sel_events.remove(&id) {
                    self.sel_events.insert(id);
                }
            } else if mods.shift {
                self.sel_events.insert(id);
            } else {
                self.sel_events.clear();
                self.sel_events.insert(id);
                let ids = self.expand_groups(&self.sel_events.clone());
                self.sel_events = ids;
            }
            let s = self.snap(t, &HashSet::new(), false);
            self.set_cursor(s);
            return;
        }
        if self.hit_envelope(p, left, rows).is_some() {
            return;
        }
        if !mods.command && !mods.shift {
            self.sel_events.clear();
        }
        self.time_sel = None;
        let s = self.snap(t, &HashSet::new(), mods.shift);
        self.set_cursor(s);
    }

    fn handle_drop(
        &mut self,
        payload: DndPayload,
        track: Option<usize>,
        t: Ticks,
        event: Option<Id>,
    ) {
        match payload {
            DndPayload::Media(mid) => {
                if let Some(m) = self.project.media(mid) {
                    if let MediaSource::Sequence { id: seq_id } = m.source {
                        if let Some(active_id) = self.project.active_sequence_id {
                            if let Err(e) =
                                self.project.validate_no_sequence_cycle(active_id, seq_id)
                            {
                                self.set_status(e);
                                return;
                            }
                        }
                    }
                }
                let prev = neoveg_engine::crossfade::overlap_pairs(&self.project.tracks);
                self.checkpoint();
                self.add_media_to_timeline(mid, t, track);
                let all_tracks: Vec<usize> = (0..self.project.tracks.len()).collect();
                self.session.sync_overlap_crossfades(&all_tracks, &prev);
            }
            DndPayload::TrimmedMedia {
                media_id,
                offset,
                length,
                include_video,
                include_audio,
            } => {
                if let Some(m) = self.project.media(media_id) {
                    if let MediaSource::Sequence { id: seq_id } = m.source {
                        if let Some(active_id) = self.project.active_sequence_id {
                            if let Err(e) =
                                self.project.validate_no_sequence_cycle(active_id, seq_id)
                            {
                                self.set_status(e);
                                return;
                            }
                        }
                    }
                }
                let prev = neoveg_engine::crossfade::overlap_pairs(&self.project.tracks);
                self.checkpoint();
                let _ = self.add_trimmed_media_to_timeline(
                    media_id,
                    t,
                    offset,
                    length,
                    include_video,
                    include_audio,
                    track,
                );
                let all_tracks: Vec<usize> = (0..self.project.tracks.len()).collect();
                self.session.sync_overlap_crossfades(&all_tracks, &prev);
            }
            DndPayload::Generator(spec) => {
                let imp = crate::media::import_generated(spec);
                let id = imp.item.id;
                self.media.insert(id, imp.entry);
                let prev = neoveg_engine::crossfade::overlap_pairs(&self.project.tracks);
                self.checkpoint();
                self.project.media.push(imp.item);
                self.add_media_to_timeline(id, t, track);
                let all_tracks: Vec<usize> = (0..self.project.tracks.len()).collect();
                self.session.sync_overlap_crossfades(&all_tracks, &prev);
            }
            DndPayload::Fx(kind) => {
                self.drop_fx_instance(FxInstance::new(kind), event, track);
            }
            DndPayload::FxPreset(mut fx) => {
                fx.id = new_id();
                self.drop_fx_instance(fx, event, track);
            }
            DndPayload::Transition(kind) => {
                if let Some(eid) = event {
                    self.apply_transition(eid, kind);
                }
            }
        }
    }

    fn drop_fx_instance(&mut self, fx: FxInstance, event: Option<Id>, track: Option<usize>) {
        let want_audio = fx.kind.is_audio();
        if let Some(eid) = event {
            let Some((ti, _)) = self.project.find_event(eid) else {
                return;
            };
            if (self.project.tracks[ti].kind == TrackKind::Audio) == want_audio {
                self.checkpoint();
                if let Some(ev) = self.project.event_mut(eid) {
                    ev.fx.push(fx);
                }
                self.dlg.fx_target = Some(FxTarget::Event(eid));
                self.changed();
                return;
            }
        }
        if let Some(ti) = track {
            if (self.project.tracks[ti].kind == TrackKind::Audio) == want_audio {
                self.checkpoint();
                self.project.tracks[ti].fx.push(fx);
                self.dlg.fx_target = Some(FxTarget::Track(self.project.tracks[ti].id));
                self.changed();
                return;
            }
        }
        self.set_status("Solte efeitos de vídeo em eventos/trilhas de vídeo e de áudio em áudio");
    }

    pub fn apply_transition(&mut self, eid: Id, kind: TransitionKind) {
        let lang = self.lang();
        let Some((ti, _)) = self.project.find_event(eid) else {
            return;
        };
        if self.project.tracks[ti].kind != TrackKind::Video {
            self.set_status(tr(lang, "Transições se aplicam a eventos de vídeo"));
            return;
        }
        let has_overlap =
            neoveg_engine::crossfade::track_crossfades(&self.project.tracks[ti], true)
                .iter()
                .any(|xf| xf.right_id == eid);
        self.checkpoint();
        let Some(e) = self.project.event_mut(eid) else {
            return;
        };
        let soft = e.transition.as_ref().map(|t| t.softness).unwrap_or(0.05);
        e.transition = Some(TransitionSpec {
            kind,
            softness: soft,
        });
        e.no_crossfade_in = false;
        if !has_overlap && e.fade_in.len <= Ticks::ZERO {
            e.fade_in.len = Ticks::from_secs_i64(1).min(e.length.div_i64(2));
        }
        self.set_status(tr_args(
            lang,
            "Transição \"{name}\" aplicada",
            &[("name", &kind.name_in(lang))],
        ));
        self.changed();
    }

    fn lane_context_menu(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        match self.tl.ctx_target.clone() {
            CtxTarget::Event(id) => self.event_context_menu(ui, id),
            CtxTarget::EventFade(id, fade_in) => {
                let title = if fade_in {
                    tr(lang, "Curva do fade (entrada)")
                } else {
                    tr(lang, "Curva do fade (saída)")
                };
                ui.label(egui::RichText::new(title).strong());
                let cur = self.project.event(id).map(|e| {
                    if fade_in {
                        e.fade_in.curve
                    } else {
                        e.fade_out.curve
                    }
                });
                let th = self.theme.clone();
                for c in Curve::ALL.iter().take(5) {
                    if fade_curve_menu_item(ui, *c, fade_in, cur == Some(*c), c.name_in(lang), &th)
                        .clicked()
                    {
                        self.checkpoint();
                        if let Some(ev) = self.project.event_mut(id) {
                            if fade_in {
                                ev.fade_in.curve = *c;
                            } else {
                                ev.fade_out.curve = *c;
                            }
                        }
                        self.changed();
                        ui.close_menu();
                    }
                }
                ui.separator();
                self.event_context_menu(ui, id);
            }
            CtxTarget::Crossfade {
                track_idx,
                left_id,
                right_id,
                active,
            } => {
                self.crossfade_context_menu(ui, track_idx, left_id, right_id, active);
            }
            CtxTarget::Keyframe {
                track,
                row,
                key,
                t,
                has_curve,
            } => self.keyframe_context_menu(ui, track, row, key, t, has_curve),
            CtxTarget::EnvPoint(owner, kind, idx) => {
                ui.label(
                    egui::RichText::new(tr_args(
                        lang,
                        "Ponto de envelope: {val}",
                        &[("val", &kind.name_in(lang))],
                    ))
                    .strong(),
                );
                let mut cur_curve = Curve::Linear;
                let mut cur_sm = 0.0_f32;
                if let Some(env) = self
                    .owner_envelopes(owner)
                    .and_then(|list| list.iter().find(|e| e.kind == kind))
                {
                    if let Some(p) = env.points.get(idx) {
                        ui.label(format!("{}  @ {}", kind.format(p.v), self.fmt_time(p.t)));
                        cur_curve = p.curve;
                        cur_sm = p.smoothness;
                    }
                }
                ui.separator();
                for c in Curve::ALL {
                    if ui
                        .selectable_label(
                            cur_curve == c,
                            tr_args(lang, "Curva: {curve}", &[("curve", &c.name_in(lang))]),
                        )
                        .clicked()
                    {
                        self.checkpoint();
                        if let Some(env) = self
                            .owner_envelopes_mut(owner)
                            .and_then(|list| list.iter_mut().find(|e| e.kind == kind))
                        {
                            if let Some(p) = env.points.get_mut(idx) {
                                p.curve = c;
                            }
                        }
                        self.changed();
                        ui.close_menu();
                    }
                }
                let mut sm_pct = (cur_sm * 100.0).clamp(0.0, 100.0);
                ui.label(tr(lang, "Suavização (%)"));
                if ui
                    .add(
                        egui::Slider::new(&mut sm_pct, 0.0..=100.0)
                            .suffix(" %")
                            .fixed_decimals(0),
                    )
                    .changed()
                {
                    if let Some(env) = self
                        .owner_envelopes_mut(owner)
                        .and_then(|list| list.iter_mut().find(|e| e.kind == kind))
                    {
                        if let Some(p) = env.points.get_mut(idx) {
                            p.smoothness = (sm_pct / 100.0).clamp(0.0, 1.0);
                        }
                    }
                    self.changed();
                }
                ui.separator();
                if ui.button(tr(lang, "Redefinir valor")).clicked() {
                    self.checkpoint();
                    if let Some(env) = self
                        .owner_envelopes_mut(owner)
                        .and_then(|list| list.iter_mut().find(|e| e.kind == kind))
                    {
                        if let Some(p) = env.points.get_mut(idx) {
                            p.v = kind.default_value();
                        }
                    }
                    self.changed();
                    ui.close_menu();
                }
                if ui.button(tr(lang, "Excluir ponto")).clicked() {
                    self.checkpoint();
                    if let Some(env) = self
                        .owner_envelopes_mut(owner)
                        .and_then(|list| list.iter_mut().find(|e| e.kind == kind))
                    {
                        if env.points.len() > 1 {
                            env.points.remove(idx);
                        }
                    }
                    self.changed();
                    ui.close_menu();
                }
            }
            CtxTarget::EnvLine(owner, kind, t) => {
                if ui.button(tr(lang, "Adicionar ponto aqui")).clicked() {
                    self.checkpoint();
                    let (def_curve, def_sm) = match owner {
                        EnvOwner::Track(ti) => self
                            .project
                            .tracks
                            .get(ti)
                            .map(|tr| (tr.default_env_curve, tr.default_env_smoothness))
                            .unwrap_or((Curve::Linear, 0.0)),
                        EnvOwner::Bus(bi) => self
                            .project
                            .audio_buses
                            .get(bi)
                            .map(|b| (b.default_env_curve, b.default_env_smoothness))
                            .unwrap_or((Curve::Linear, 0.0)),
                        EnvOwner::MasterVideo => (Curve::Linear, 0.0),
                    };
                    if let Some(env) = self
                        .owner_envelopes_mut(owner)
                        .and_then(|list| list.iter_mut().find(|e| e.kind == kind))
                    {
                        let v = env.eval(t);
                        env.points.push(EnvPoint {
                            t,
                            v,
                            curve: def_curve,
                            smoothness: def_sm,
                        });
                        env.sort();
                    }
                    self.changed();
                    ui.close_menu();
                }
                let seg_info = self
                    .owner_envelopes(owner)
                    .and_then(|list| list.iter().find(|e| e.kind == kind))
                    .and_then(|env| {
                        let seg_i = env.segment_index_at(t)?;
                        let pt = env.points.get(seg_i)?;
                        Some((seg_i, pt.curve, pt.smoothness))
                    });
                if let Some((seg_i, seg_curve, seg_sm)) = seg_info {
                    ui.separator();
                    ui.menu_button(tr(lang, "Curva deste segmento"), |ui| {
                        for c in Curve::ALL {
                            if ui
                                .selectable_label(seg_curve == c, c.name_in(lang))
                                .clicked()
                            {
                                self.checkpoint();
                                if let Some(env) = self
                                    .owner_envelopes_mut(owner)
                                    .and_then(|list| list.iter_mut().find(|e| e.kind == kind))
                                {
                                    if let Some(pt) = env.points.get_mut(seg_i) {
                                        pt.curve = c;
                                    }
                                }
                                self.changed();
                                ui.close_menu();
                            }
                        }
                    });
                    let mut sm_pct = (seg_sm * 100.0).clamp(0.0, 100.0);
                    ui.label(tr(lang, "Suavização (%)"));
                    if ui
                        .add(
                            egui::Slider::new(&mut sm_pct, 0.0..=100.0)
                                .suffix(" %")
                                .fixed_decimals(0),
                        )
                        .changed()
                    {
                        if let Some(env) = self
                            .owner_envelopes_mut(owner)
                            .and_then(|list| list.iter_mut().find(|e| e.kind == kind))
                        {
                            if let Some(pt) = env.points.get_mut(seg_i) {
                                pt.smoothness = (sm_pct / 100.0).clamp(0.0, 1.0);
                            }
                        }
                        self.changed();
                    }
                }
                ui.separator();
                if ui.button(tr(lang, "Redefinir todos os pontos")).clicked() {
                    self.checkpoint();
                    if let Some(env) = self
                        .owner_envelopes_mut(owner)
                        .and_then(|list| list.iter_mut().find(|e| e.kind == kind))
                    {
                        *env = Envelope::new(kind);
                    }
                    self.changed();
                    ui.close_menu();
                }
                if ui.button(tr(lang, "Ocultar envelope")).clicked() {
                    if let Some(env) = self
                        .owner_envelopes_mut(owner)
                        .and_then(|list| list.iter_mut().find(|e| e.kind == kind))
                    {
                        env.visible = false;
                    }
                    ui.close_menu();
                }
                if ui.button(tr(lang, "Remover envelope")).clicked() {
                    self.checkpoint();
                    if let Some(list) = self.owner_envelopes_mut(owner) {
                        list.retain(|e| e.kind != kind);
                    }
                    self.changed();
                    ui.close_menu();
                }
            }
            CtxTarget::Lane(ti, t) => {
                if ui
                    .add_enabled(
                        !self.clipboard.is_empty(),
                        egui::Button::new(tr(lang, "Colar aqui")),
                    )
                    .clicked()
                {
                    self.cursor = t;
                    self.sel_track = Some(ti);
                    self.paste(false);
                    ui.close_menu();
                }
                if ui.button(tr(lang, "Inserir marcador aqui")).clicked() {
                    self.cursor = t;
                    self.insert_marker();
                    ui.close_menu();
                }
                if ui.button(tr(lang, "Inserir texto aqui")).clicked() {
                    self.cursor = t;
                    self.sel_track = Some(ti);
                    self.insert_generated(GenSpec::Text(TextSpec::default()));
                    ui.close_menu();
                }
                if ui.button("Inserir evento vazio aqui").clicked() {
                    self.cursor = t;
                    self.sel_track = Some(ti);
                    self.session.insert_empty_event();
                    self.sync_after_session();
                    ui.close_menu();
                }
                if matches!(self.session.edit_tool, EditTool::Selection | EditTool::Zoom) {
                    ui.separator();
                    ui.menu_button("Eixo da seleção/zoom", |ui| {
                        for axis in BoxAxisMode::ALL {
                            if ui
                                .radio(self.session.box_axis == axis, axis.name())
                                .clicked()
                            {
                                self.session.box_axis = axis;
                                ui.close_menu();
                            }
                        }
                    });
                }
                ui.separator();
                self.track_menu(ui, ti);
            }
            _ => {
                if matches!(self.session.edit_tool, EditTool::Selection | EditTool::Zoom) {
                    ui.menu_button("Eixo da seleção/zoom", |ui| {
                        for axis in BoxAxisMode::ALL {
                            if ui
                                .radio(self.session.box_axis == axis, axis.name())
                                .clicked()
                            {
                                self.session.box_axis = axis;
                                ui.close_menu();
                            }
                        }
                    });
                } else {
                    ui.label("—");
                }
            }
        }
    }

    /// Submenu Take (manual do VEGAS Pro 21, p. 347): escolher o take ativo,
    /// próximo/anterior, Take Chooser e excluir o ativo.
    fn take_submenu(&mut self, ui: &mut egui::Ui, e: &Event) {
        use neoveg_engine::takes;
        let lang = self.lang();
        let list = takes::takes(e);
        let active = takes::active_index(e);
        let id = e.id;
        let idx_s = (active + 1).to_string();
        let total_s = list.len().to_string();
        ui.menu_button(
            tr_args(
                lang,
                "Take ({idx} de {total})",
                &[("idx", &idx_s), ("total", &total_s)],
            ),
            |ui| {
                for (k, t) in list.iter().enumerate() {
                    let name =
                        takes::display_name(t, |m| self.project.media(m).map(|x| x.name.clone()));
                    if ui
                        .radio(k == active, format!("{}. {name}", k + 1))
                        .clicked()
                    {
                        if !self.session.set_active_take(id, k) && k != active {
                            self.set_status(tr(lang, "Evento travado: o take não foi trocado."));
                        }
                        ui.close_menu();
                    }
                }
                ui.separator();
                if !self.sel_events.contains(&id) {
                    self.sel_events.clear();
                    self.sel_events.insert(id);
                }
                self.cmd_menu_btn(ui, "event.take_chooser");
                ui.separator();
                self.cmd_menu_btn(ui, "event.next_take");
                self.cmd_menu_btn(ui, "event.prev_take");
                self.cmd_menu_btn(ui, "event.delete_active_take");
            },
        );
    }

    fn crossfade_context_menu(
        &mut self,
        ui: &mut egui::Ui,
        track_idx: usize,
        left_id: Id,
        right_id: Id,
        active: bool,
    ) {
        let lang = self.lang();
        let Some(track) = self.project.tracks.get(track_idx) else {
            ui.close_menu();
            return;
        };
        let is_video = track.kind == TrackKind::Video;
        let Some(xf) = neoveg_engine::crossfade::track_crossfades(track, true)
            .into_iter()
            .find(|x| x.left_id == left_id && x.right_id == right_id)
        else {
            self.event_context_menu(ui, right_id);
            return;
        };
        let dur_str = self.fmt_time(xf.len());
        let header = if active {
            tr_args(lang, "Crossfade ({dur})", &[("dur", &dur_str)])
        } else {
            tr_args(
                lang,
                "Sobreposição sem crossfade ({dur})",
                &[("dur", &dur_str)],
            )
        };
        ui.label(egui::RichText::new(header).strong());
        ui.separator();

        if active {
            let th = self.theme.clone();
            ui.menu_button(tr(lang, "Tipo de fade (Crossfade)"), |ui| {
                let presets: [(&str, Curve, Curve); 5] = [
                    (
                        "Potência constante (Rápida / Rápida)",
                        Curve::Fast,
                        Curve::Fast,
                    ),
                    ("Linear (Linear / Linear)", Curve::Linear, Curve::Linear),
                    ("Suave (Suave / Suave)", Curve::Smooth, Curve::Smooth),
                    ("Abrupta (Abrupta / Abrupta)", Curve::Sharp, Curve::Sharp),
                    ("Mergulho (Lenta / Lenta)", Curve::Slow, Curve::Slow),
                ];
                for (lbl, oc, ic) in presets {
                    let sel = xf.out_curve == oc && xf.in_curve == ic;
                    if crossfade_curve_menu_item(ui, oc, ic, sel, tr(lang, lbl), &th).clicked() {
                        self.session.set_crossfade_curves(left_id, right_id, oc, ic);
                        self.sync_after_session();
                        ui.close_menu();
                    }
                }
                ui.separator();
                ui.label(
                    egui::RichText::new(tr(lang, "Matriz 5×5 (Saída × Entrada)"))
                        .small()
                        .color(th.text_muted),
                );
                let curves = neoveg_engine::crossfade::CROSSFADE_CURVES;
                egui::Grid::new("xfade_matrix_5x5")
                    .spacing(vec2(3.0, 3.0))
                    .show(ui, |ui| {
                        ui.label("");
                        for ic in curves {
                            ui.label(
                                egui::RichText::new(ic.name_in(lang))
                                    .small()
                                    .color(th.text_muted),
                            );
                        }
                        ui.end_row();
                        for oc in curves {
                            ui.label(
                                egui::RichText::new(oc.name_in(lang))
                                    .small()
                                    .color(th.text_muted),
                            );
                            for ic in curves {
                                let sel = xf.out_curve == oc && xf.in_curve == ic;
                                let (cell_r, resp) =
                                    ui.allocate_exact_size(vec2(32.0, 20.0), Sense::click());
                                let out_lbl = oc.name_in(lang);
                                let in_lbl = ic.name_in(lang);
                                let tip = tr_args(
                                    lang,
                                    "Saída: {out} / Entrada: {inc}",
                                    &[("out", &out_lbl), ("inc", &in_lbl)],
                                );
                                resp.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        true,
                                        tip.clone(),
                                    )
                                });
                                paint_crossfade_curve_icon(
                                    &ui.painter_at(cell_r),
                                    cell_r,
                                    oc,
                                    ic,
                                    sel,
                                    resp.hovered(),
                                    &th,
                                );
                                if resp.on_hover_text(tip).clicked() {
                                    self.session.set_crossfade_curves(left_id, right_id, oc, ic);
                                    self.sync_after_session();
                                    ui.close_menu();
                                }
                            }
                            ui.end_row();
                        }
                    });
            });

            if is_video {
                let cur_tr = self
                    .project
                    .event(right_id)
                    .and_then(|e| e.transition.clone());
                ui.menu_button(tr(lang, "Transição"), |ui| {
                    if ui
                        .selectable_label(cur_tr.is_none(), tr(lang, "Dissolver (crossfade)"))
                        .clicked()
                    {
                        self.checkpoint();
                        if let Some(ev) = self.project.event_mut(right_id) {
                            ev.transition = None;
                        }
                        self.changed();
                        ui.close_menu();
                    }
                    ui.separator();
                    for k in TransitionKind::ALL {
                        let sel = cur_tr.as_ref().is_some_and(|t| t.kind == k);
                        if ui.selectable_label(sel, k.name_in(lang)).clicked() {
                            self.apply_transition(right_id, k);
                            ui.close_menu();
                        }
                    }
                });
            }

            if ui
                .button(tr(lang, "Desativar crossfade (Corte seco)"))
                .clicked()
            {
                self.session.set_crossfade_enabled(right_id, false);
                self.sync_after_session();
                ui.close_menu();
            }
        } else if ui.button(tr(lang, "Ativar crossfade")).clicked() {
            self.session.set_crossfade_enabled(right_id, true);
            self.sync_after_session();
            ui.close_menu();
        }

        ui.menu_button(
            tr(lang, "Converter em corte (Remover sobreposição)"),
            |ui| {
                for (align, lbl) in [
                    (
                        neoveg_common::CutToOverlapAlign::Center,
                        "Centralizado no corte",
                    ),
                    (
                        neoveg_common::CutToOverlapAlign::BeforeCut,
                        "No início da sobreposição",
                    ),
                    (
                        neoveg_common::CutToOverlapAlign::AfterCut,
                        "No fim da sobreposição",
                    ),
                ] {
                    if ui.button(tr(lang, lbl)).clicked() {
                        self.session
                            .convert_crossfade_to_cut(left_id, right_id, align);
                        self.sync_after_session();
                        ui.close_menu();
                    }
                }
            },
        );

        ui.separator();
        let left_name = self
            .project
            .event(left_id)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let right_name = self
            .project
            .event(right_id)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        if ui
            .button(tr_args(
                lang,
                "Propriedades: {name}",
                &[("name", &left_name)],
            ))
            .clicked()
        {
            self.dlg.event_props = Some(left_id);
            ui.close_menu();
        }
        if ui
            .button(tr_args(
                lang,
                "Propriedades: {name}",
                &[("name", &right_name)],
            ))
            .clicked()
        {
            self.dlg.event_props = Some(right_id);
            ui.close_menu();
        }
    }

    fn event_context_menu(&mut self, ui: &mut egui::Ui, id: Id) {
        let lang = self.lang();
        let Some((ti, ei)) = self.project.find_event(id) else {
            ui.close_menu();
            return;
        };
        let is_video = self.project.tracks[ti].kind == TrackKind::Video;
        let e = self.project.tracks[ti].events[ei].clone();
        let media_generated = self
            .project
            .media(e.media_id)
            .map(|m| matches!(m.source, MediaSource::Generated(_)))
            .unwrap_or(false);
        ui.label(egui::RichText::new(&e.name).strong());
        ui.separator();
        if ui.button(tr(lang, "Propriedades...")).clicked() {
            self.dlg.event_props = Some(id);
            ui.close_menu();
        }
        if self.nested_media_path(e.media_id).is_some()
            && ui
                .button(tr(lang, "Abrir projeto aninhado em aba"))
                .clicked()
        {
            self.open_nested_media_tab(e.media_id);
            ui.close_menu();
        }
        if let Some(seq_id) = self.nested_sequence_id(e.media_id) {
            if ui
                .button(tr(lang, "Abrir sequência aninhada em aba"))
                .clicked()
            {
                self.switch_sequence_app(seq_id);
                ui.close_menu();
            }
        }
        if !self.sel_events.is_empty()
            && ui
                .button(tr(lang, "Aninhar seleção em nova sequência"))
                .clicked()
        {
            self.exec_cmd(Some(ui.ctx()), "sequence.nest_selection");
            ui.close_menu();
        }
        if is_video && ui.button(tr(lang, "Pan/Crop do evento...")).clicked() {
            self.dlg.pan_crop = Some(id);
            ui.close_menu();
        }
        if ui.button(tr(lang, "Efeitos do evento...")).clicked() {
            self.open_fx_target(FxTarget::Event(id));
            ui.close_menu();
        }
        if media_generated && ui.button(tr(lang, "Editar mídia gerada...")).clicked() {
            self.dlg.gen_editor = Some(e.media_id);
            ui.close_menu();
        }
        if ui.button(tr(lang, "Renomear (F2)")).clicked() {
            self.tl.rename = Some((id, e.name.clone()));
            ui.close_menu();
        }
        self.take_submenu(ui, &e);
        ui.menu_button("Editar em aplicativo externo", |ui| {
            use neoveg_engine::integrations::ExternalApp;
            if is_video {
                for app in [
                    ExternalApp::PhotoCraft,
                    ExternalApp::VectorCraft,
                    ExternalApp::EffectCraft,
                    ExternalApp::AniCraft,
                    ExternalApp::Blender,
                ] {
                    if ui.button(format!("{}...", app.name())).clicked() {
                        self.send_to_external_app(app, Some(id), Some(e.media_id));
                        ui.close_menu();
                    }
                }
            } else if ui.button("SoundCraft (cópia BWF)...").clicked() {
                self.send_to_external_app(ExternalApp::SoundCraft, Some(id), Some(e.media_id));
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Gerenciar integrações...").clicked() {
                self.open_integrations_dialog();
                ui.close_menu();
            }
        });
        ui.separator();
        if ui.button(tr(lang, "Dividir no cursor (S)")).clicked() {
            self.split_at_cursor();
            ui.close_menu();
        }
        if self.time_sel.is_some() && ui.button(tr(lang, "Smart Split (Alt+S)")).clicked() {
            self.exec_cmd(Some(ui.ctx()), "edit.smart_split");
            ui.close_menu();
        }
        if self.session.can_heal_selection()
            && ui.button(tr(lang, "Juntar eventos (Heal)")).clicked()
        {
            self.exec_cmd(Some(ui.ctx()), "edit.heal_events");
            ui.close_menu();
        }
        if self.session.can_convert_cut_to_overlap()
            && ui
                .button(tr(lang, "Converter corte em crossfade"))
                .clicked()
        {
            self.exec_cmd(Some(ui.ctx()), "edit.cut_to_overlap");
            ui.close_menu();
        }
        if ui.button(tr(lang, "Copiar")).clicked() {
            self.copy_selection();
            ui.close_menu();
        }
        if ui.button(tr(lang, "Recortar")).clicked() {
            self.cut_selection();
            ui.close_menu();
        }
        if !self.clipboard.is_empty() {
            if ui.button(tr(lang, "Colar atributos do evento")).clicked() {
                self.exec_cmd(Some(ui.ctx()), "edit.paste_attributes");
                ui.close_menu();
            }
            if ui
                .button(tr(lang, "Colar atributos seletivamente..."))
                .clicked()
            {
                self.exec_cmd(Some(ui.ctx()), "edit.paste_attributes_selective");
                ui.close_menu();
            }
        }
        if ui.button(tr(lang, "Excluir")).clicked() {
            self.delete_selection();
            ui.close_menu();
        }
        ui.separator();
        let mut flags = (e.mute, e.locked, e.looped, e.reverse, e.maintain_aspect);
        let before = flags;
        ui.checkbox(&mut flags.0, tr(lang, "Mudo"));
        ui.checkbox(&mut flags.1, tr(lang, "Travado"));
        ui.checkbox(&mut flags.2, tr(lang, "Loop"));
        ui.checkbox(&mut flags.3, tr(lang, "Inverter (reverse)"));
        if is_video {
            ui.checkbox(&mut flags.4, tr(lang, "Manter proporção"));
        }
        if flags != before {
            self.checkpoint();
            for sid in self.sel_events.clone() {
                if let Some(ev) = self.project.event_mut(sid) {
                    if flags.0 != before.0 {
                        ev.mute = flags.0;
                    }
                    if flags.1 != before.1 {
                        ev.locked = flags.1;
                    }
                    if flags.2 != before.2 {
                        ev.looped = flags.2;
                    }
                    if flags.3 != before.3 {
                        ev.reverse = flags.3;
                    }
                    if flags.4 != before.4 {
                        ev.maintain_aspect = flags.4;
                    }
                }
            }
            self.changed();
        }
        ui.separator();
        let th = self.theme.clone();
        ui.menu_button(tr(lang, "Curva do fade de entrada"), |ui| {
            for c in Curve::ALL.iter().take(5) {
                if fade_curve_menu_item(ui, *c, true, e.fade_in.curve == *c, c.name_in(lang), &th)
                    .clicked()
                {
                    self.checkpoint();
                    if let Some(ev) = self.project.event_mut(id) {
                        ev.fade_in.curve = *c;
                    }
                    self.changed();
                    ui.close_menu();
                }
            }
        });
        ui.menu_button(tr(lang, "Curva do fade de saída"), |ui| {
            for c in Curve::ALL.iter().take(5) {
                if fade_curve_menu_item(ui, *c, false, e.fade_out.curve == *c, c.name_in(lang), &th)
                    .clicked()
                {
                    self.checkpoint();
                    if let Some(ev) = self.project.event_mut(id) {
                        ev.fade_out.curve = *c;
                    }
                    self.changed();
                    ui.close_menu();
                }
            }
        });
        if is_video {
            ui.menu_button(tr(lang, "Transição de entrada"), |ui| {
                if ui
                    .selectable_label(e.transition.is_none(), tr(lang, "Nenhuma (crossfade)"))
                    .clicked()
                {
                    self.checkpoint();
                    if let Some(ev) = self.project.event_mut(id) {
                        ev.transition = None;
                    }
                    self.changed();
                    ui.close_menu();
                }
                for k in TransitionKind::ALL {
                    let sel = e.transition.as_ref().map(|t| t.kind == k).unwrap_or(false);
                    if ui.selectable_label(sel, k.name_in(lang)).clicked() {
                        self.apply_transition(id, k);
                        ui.close_menu();
                    }
                }
            });
        }
        ui.menu_button(tr(lang, "Velocidade"), |ui| {
            for (num, den, pct) in [
                (1, 4, 25),
                (1, 2, 50),
                (3, 4, 75),
                (1, 1, 100),
                (3, 2, 150),
                (2, 1, 200),
                (4, 1, 400),
            ] {
                let Some(r) = Rate::from_ratio(num, den) else {
                    continue;
                };
                if ui
                    .selectable_label(e.rate == r, format!("{pct}%"))
                    .clicked()
                {
                    self.checkpoint();
                    let min_len = self.project.settings.frame_dur();
                    let Some(ev) = self.project.event_mut(id) else {
                        return;
                    };
                    // mantém o mesmo conteúdo: ajusta a duração
                    let content = ev.rate.scale_ticks(ev.length);
                    ev.rate = r;
                    if let Some(new_len) = r.unscale_ticks(content) {
                        ev.length = new_len.max(min_len);
                    }
                    self.changed();
                    ui.close_menu();
                }
            }
        });
        ui.separator();
        if ui.button(tr(lang, "Agrupar seleção (G)")).clicked() {
            self.group_selected();
            ui.close_menu();
        }
        if e.group.is_some() && ui.button(tr(lang, "Desagrupar (U)")).clicked() {
            self.ungroup_selected();
            ui.close_menu();
        }
        if e.group.is_some() && ui.button(tr(lang, "Selecionar grupo (Shift+G)")).clicked() {
            self.select_group();
            ui.close_menu();
        }
        ui.menu_button(tr(lang, "Sync Link"), |ui| {
            let can_link = self.sel_events.len() >= 2;
            if ui
                .add_enabled(
                    can_link,
                    egui::Button::new(tr(lang, "Vincular seleção a este (Sync Link)")),
                )
                .clicked()
            {
                self.session.create_sync_links(Some(id));
                self.sync_after_session();
                ui.close_menu();
            }
            self.cmd_menu_btn(ui, "event.sync_link_select");
            self.cmd_menu_btn(ui, "event.sync_link_unlink");
        });
        if !is_video {
            ui.menu_button(tr(lang, "Tom (Pitch)"), |ui| {
                self.cmd_menu_btn(ui, "event.pitch_up");
                self.cmd_menu_btn(ui, "event.pitch_down");
                self.cmd_menu_btn(ui, "event.pitch_up_oct");
                self.cmd_menu_btn(ui, "event.pitch_down_oct");
                ui.separator();
                self.cmd_menu_chk(ui, e.lock_pitch_to_stretch, "event.toggle_pitch_lock");
                self.cmd_menu_btn(ui, "event.pitch_reset");
            });
            ui.menu_button(tr(lang, "Ferramentas de áudio"), |ui| {
                self.cmd_menu_btn(ui, "tools.auto_normalize");
                self.cmd_menu_btn(ui, "tools.auto_ducking");
                self.cmd_menu_btn(ui, "tools.beat_detection");
            });
        }
        let track_kind = if is_video {
            TrackKind::Video
        } else {
            TrackKind::Audio
        };
        if neoveg_engine::engine::audio_tools::event_sync_offset(&self.project, track_kind, &e)
            .is_some()
        {
            ui.separator();
            self.cmd_menu_btn(ui, "event.repair_sync_move");
            self.cmd_menu_btn(ui, "event.repair_sync_slip");
        }
        if is_video {
            ui.menu_button(tr(lang, "Envelope de velocidade"), |ui| {
                let has_vel = e.velocity_envelope().is_some();
                let lbl = if has_vel {
                    tr(lang, "Remover envelope de velocidade")
                } else {
                    tr(lang, "Inserir envelope de velocidade")
                };
                if ui.button(lbl).clicked() {
                    self.session.toggle_velocity_envelope_selected();
                    self.sync_after_session();
                    ui.close_menu();
                }
                ui.separator();
                for (lbl_pt, spd) in [
                    ("0% (Congelar quadro)", 0.0_f32),
                    ("50% (Câmera lenta)", 0.5_f32),
                    ("100% (Normal)", 1.0_f32),
                    ("200% (Rápido)", 2.0_f32),
                    ("300% (Máximo)", 3.0_f32),
                    ("-100% (Reverso)", -1.0_f32),
                ] {
                    if ui.button(tr(lang, lbl_pt)).clicked() {
                        self.session.set_velocity_preset_selected(spd);
                        self.sync_after_session();
                        ui.close_menu();
                    }
                }
            });
        }
        if is_video && ui.button(tr(lang, "Congelar quadro no cursor")).clicked() {
            self.freeze_frame();
            ui.close_menu();
        }
        if is_video {
            if ui.button("Detectar cenas e cortes...").clicked() {
                self.exec_cmd(Some(ui.ctx()), "edit.detect_scenes");
                ui.close_menu();
            }
            if ui.button("Modo de edição expandido (A/B)").clicked() {
                self.exec_cmd(Some(ui.ctx()), "edit.expanded_edit_mode");
                ui.close_menu();
            }
        }
    }

    // -----------------------------------------------------------------
    // Desenho
    // -----------------------------------------------------------------

    fn draw_row_envelopes(
        &self,
        painter: &egui::Painter,
        lanes: Rect,
        left: f32,
        y0: f32,
        h: f32,
        envelopes: &[Envelope],
    ) {
        if !self.opts.show_envelopes {
            return;
        }
        for env in envelopes.iter().filter(|e| e.visible) {
            // cores da aba Exibição, na ordem de DISPLAY_ENVELOPES
            let col = match env.kind {
                EnvKind::Volume => self.envelope_color(0, self.theme.tl_env_primary),
                EnvKind::Pan => self.envelope_color(1, self.theme.tl_env_secondary),
                EnvKind::Opacity => self.envelope_color(2, self.theme.tl_env_primary),
                EnvKind::FadeToColor => self.envelope_color(3, self.theme.tl_env_secondary),
                EnvKind::Velocity => Color32::from_rgb(76, 224, 108),
                EnvKind::MotionBlur => Color32::from_rgb(186, 132, 245),
            };
            let mut pts = vec![];
            let mut x = lanes.left();
            while x <= lanes.right() + 3.0 {
                let t = self.tl.x2t(x, left);
                pts.push(pos2(x, Self::env_y(env.kind, env.eval(t), y0, h)));
                x += 3.0;
            }
            painter.add(Shape::line(pts, Stroke::new(1.5_f32, col)));
            for p in &env.points {
                let px = self.tl.t2x(p.t, left);
                if px < lanes.left() - 5.0 || px > lanes.right() + 5.0 {
                    continue;
                }
                let py = Self::env_y(env.kind, p.v, y0, h);
                painter.rect_filled(
                    Rect::from_center_size(pos2(px, py), vec2(7.0, 7.0)),
                    1.0,
                    col,
                );
                painter.rect_stroke(
                    Rect::from_center_size(pos2(px, py), vec2(7.0, 7.0)),
                    1.0,
                    Stroke::new(1.0_f32, crate::theme::contrast_on(col)),
                    StrokeKind::Inside,
                );
            }
            painter.text(
                pos2(lanes.left() + 4.0, y0 + h - 14.0),
                Align2::LEFT_TOP,
                env.kind.name(),
                FontId::proportional(10.0),
                col,
            );
        }
    }

    #[allow(clippy::needless_range_loop)]
    fn draw_lanes(&mut self, ui: &mut egui::Ui, lanes: Rect, rows: &TimelineRows) {
        let layout = &rows.track_rects;
        let painter = ui.painter_at(lanes);
        painter.rect_filled(lanes, 0.0, self.theme.tl_bg);
        let left = lanes.left();
        let pps = self.tl.px_per_sec;
        let ctx = ui.ctx().clone();
        // grade
        let (major, _) = effective_grid_step(
            self.opts.ruler_grid,
            pps,
            &self.project.settings,
            lanes.width(),
        );
        let step = major.0.max(1);
        let t_end = self.tl.x2t(lanes.right(), left);
        let mut idx = self.tl.scroll_x.0 / step;
        let mut marks = 0u32;
        loop {
            marks += 1;
            if marks > MAX_RULER_MARKS {
                break;
            }
            let gt = Ticks::from_raw(idx.saturating_mul(step));
            if gt > t_end {
                break;
            }
            let x = self.tl.t2x(gt, left);
            painter.line_segment(
                [pos2(x, lanes.top()), pos2(x, lanes.bottom())],
                Stroke::new(1.0_f32, self.theme.tl_grid_line),
            );
            idx = idx.saturating_add(1);
        }
        // seleção de tempo
        if let Some((a, b)) = self.time_sel {
            let r = Rect::from_min_max(
                pos2(self.tl.t2x(a, left), lanes.top()),
                pos2(self.tl.t2x(b, left), lanes.bottom()),
            );
            painter.rect_filled(r, 0.0, self.theme.tl_sel_band);
        }
        let audio_map = self.media.audio_map();
        self.tl.layer_row.clear();
        for t in &self.project.tracks {
            if t.kind == TrackKind::Video && self.tl.layers_expanded.contains(&t.id) {
                self.tl.layer_row.extend(ab_rolls(&t.events));
            }
        }
        let sync_masters: HashSet<Id> = self
            .project
            .tracks
            .iter()
            .flat_map(|t| t.events.iter())
            .filter_map(|e| e.sync_link_master)
            .collect();
        for ti in 0..self.project.tracks.len() {
            let (y0, h) = layout[ti];
            if h <= 0.0 || y0 + h < lanes.top() || y0 > lanes.bottom() {
                continue;
            }
            let lane = Rect::from_min_max(pos2(lanes.left(), y0), pos2(lanes.right(), y0 + h));
            let selected = self.sel_track == Some(ti);
            let track_is_folder = self.project.tracks[ti].is_folder;
            painter.rect_filled(
                lane,
                0.0,
                if track_is_folder {
                    shade(self.track_rgb(self.project.tracks[ti].color), 0.32)
                } else if selected {
                    self.theme.tl_lane_even
                } else {
                    self.theme.tl_lane_odd
                },
            );
            painter.line_segment(
                [
                    pos2(lane.left(), lane.bottom()),
                    pos2(lane.right(), lane.bottom()),
                ],
                Stroke::new(1.0_f32, self.theme.border),
            );
            let track = self.project.tracks[ti].clone();
            let muted = track.mute;
            let xfades = neoveg_engine::crossfade::track_crossfades(&track, true);
            if track.kind == TrackKind::Video && self.tl.layers_expanded.contains(&track.id) {
                // rolo de transição: faixa do meio, com as sobreposições
                let roll = Rect::from_min_max(
                    pos2(lane.left(), y0 + h / 3.0),
                    pos2(lane.right(), y0 + h * 2.0 / 3.0),
                );
                painter.rect_filled(roll, 0.0, self.theme.tl_lane_even);
                for xf in &xfades {
                    let r = Rect::from_min_max(
                        pos2(self.tl.t2x(xf.start, left), roll.top() + 1.0),
                        pos2(self.tl.t2x(xf.end, left), roll.bottom() - 1.0),
                    );
                    if r.right() < lanes.left() || r.left() > lanes.right() {
                        continue;
                    }
                    painter.rect_filled(r, 2.0, self.theme.tl_crossfade);
                    painter.line_segment(
                        [r.left_bottom(), r.right_top()],
                        Stroke::new(1.0_f32, self.theme.text_primary),
                    );
                    painter.line_segment(
                        [r.left_top(), r.right_bottom()],
                        Stroke::new(1.0_f32, self.theme.text_primary),
                    );
                }
            }
            for e in &track.events {
                let r = self.event_rect(e, left, y0, h);
                if r.right() < lanes.left() || r.left() > lanes.right() {
                    continue;
                }
                let xf_in = xfades.iter().find(|xf| xf.active && xf.right_id == e.id);
                let xf_out = xfades.iter().find(|xf| xf.active && xf.left_id == e.id);
                let (eff_fi, eff_fo) =
                    neoveg_engine::crossfade::effective_event_fades_from_regions(e, &xfades);
                let sel = self.sel_events.contains(&e.id);
                let k = if sel { 1.0 } else { 0.62 };
                let tc = self.track_rgb(track.color);
                // Inverter a cor de fundo do evento (aba Exibição)
                let reverse = self.prefs.display.reverse_event_bg;
                let body = if e.mute || muted {
                    shade([90, 90, 90], if self.theme.dark_mode { 0.7 } else { 1.45 })
                } else if reverse {
                    shade(tc, k * 1.35)
                } else {
                    shade(tc, k * 0.55)
                };
                let hdr_h = 14.0f32.min(r.height() * 0.3);
                let hdr_col = if e.mute || muted {
                    if self.theme.dark_mode {
                        Color32::from_gray(80)
                    } else {
                        Color32::from_gray(176)
                    }
                } else {
                    shade(tc, k * 0.9)
                };
                let hdr = Rect::from_min_max(r.min, pos2(r.right(), r.top() + hdr_h));
                if let Some(xf) = xf_in {
                    // Na região de crossfade, o fundo do evento que entra fica translúcido
                    // para deixar a forma de onda/miniatura do evento anterior aparecer junto.
                    let split_x = self
                        .tl
                        .t2x(xf.end, left)
                        .clamp(r.left(), r.right().max(r.left()));
                    let ov_r = Rect::from_min_max(r.min, pos2(split_x, r.bottom()));
                    let rem_r = Rect::from_min_max(pos2(split_x, r.top()), r.max);
                    let ov_alpha = if track.kind == TrackKind::Audio {
                        0.42
                    } else {
                        0.55
                    };
                    if ov_r.width() > 0.5 {
                        painter.rect_filled(ov_r, 0.0, body.gamma_multiply(ov_alpha));
                    }
                    if rem_r.width() > 0.5 {
                        painter.rect_filled(rem_r, 2.0, body);
                        let rem_hdr = Rect::from_min_max(
                            pos2(split_x, hdr.top()),
                            pos2(r.right(), hdr.bottom()),
                        );
                        painter.rect_filled(rem_hdr, 2.0, hdr_col);
                    }
                } else {
                    painter.rect_filled(r, 2.0, body);
                    painter.rect_filled(hdr, 2.0, hdr_col);
                }
                let body_light = crate::theme::relative_luminance(body) > 0.35;
                let hdr_fg = crate::theme::contrast_on(hdr_col);
                let mut ev_icon_theme = self.theme.clone();
                ev_icon_theme.dark_mode = !body_light;
                if ev_icon_theme.high_contrast {
                    let fg = crate::theme::contrast_on(body);
                    ev_icon_theme.text_primary = fg;
                    ev_icon_theme.text_muted = fg;
                    ev_icon_theme.extreme_bg = body;
                }
                let content = Rect::from_min_max(pos2(r.left(), hdr.bottom()), r.max);
                let vis = content.intersect(lanes);
                // conteúdo: miniaturas ou forma de onda
                if track.kind == TrackKind::Video
                    && self.opts.show_thumbs
                    && content.height() > 16.0
                {
                    if let Some(tex) = self.thumb(&ctx, e.media_id) {
                        let ts = tex.size_vec2();
                        let th = content.height() - 2.0;
                        let tw = th * ts.x / ts.y.max(1.0);
                        let mut x = content.left() + 1.0;
                        let p2 = painter.with_clip_rect(vis);
                        let mut count = 0;
                        while x < content.right() && count < 64 {
                            if x + tw >= lanes.left() && x <= lanes.right() {
                                p2.image(
                                    tex.id(),
                                    Rect::from_min_size(pos2(x, content.top() + 1.0), vec2(tw, th)),
                                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                    Color32::from_white_alpha(if e.mute { 80 } else { 230 }),
                                );
                            }
                            x += tw.max(8.0)
                                + if content.width() > tw * 3.0 {
                                    tw * 2.0
                                } else {
                                    tw
                                };
                            count += 1;
                        }
                    }
                }
                if track.kind == TrackKind::Audio && self.opts.show_waveforms && vis.width() > 0.0 {
                    if let Some(a) = audio_map.get(&e.media_id) {
                        let dur = self
                            .project
                            .media(e.media_id)
                            .and_then(|m| m.duration)
                            .or(Some(a.duration()));
                        // ganho do evento e zoom vertical das formas de onda (Shift+↑/↓)
                        let gain = db_to_lin(e.gain_db).min(4.0) * self.opts.waveform_zoom();
                        let alpha = if e.mute { 70 } else { 185 };
                        let gray = if body_light {
                            Color32::from_rgba_unmultiplied(22, 30, 42, alpha)
                        } else {
                            Color32::from_rgba_unmultiplied(220, 230, 240, alpha)
                        };
                        // VEGAS: a forma de onda usa a cor da faixa (Track Display
                        // Color); cinza quando mudo, em alto contraste ou sem contraste
                        let tinted = if reverse {
                            shade(tc, 0.28)
                        } else {
                            shade(tc, if body_light { 0.45 } else { 1.35 })
                        };
                        let col = if e.mute
                            || muted
                            || self.theme.high_contrast
                            || crate::theme::contrast_ratio(tinted, body) < 3.0
                        {
                            gray
                        } else {
                            tinted.gamma_multiply(f32::from(alpha) / 255.0)
                        };
                        let center_col = col.gamma_multiply(0.45);
                        // como no VEGAS: fonte estéreo mostra L em cima e R embaixo,
                        // cada um com sua linha central; evento baixo ou mono, um traço só
                        let chans: Vec<(WaveChannel, Rect)> =
                            if !a.mono_source && content.height() >= 30.0 {
                                let my = content.center().y;
                                vec![
                                    (
                                        WaveChannel::Left,
                                        Rect::from_min_max(content.min, pos2(content.right(), my)),
                                    ),
                                    (
                                        WaveChannel::Right,
                                        Rect::from_min_max(pos2(content.left(), my), content.max),
                                    ),
                                ]
                            } else {
                                vec![(WaveChannel::Mix, content)]
                            };
                        let last_t = (e.end() - Ticks::from_raw(1)).max(e.start);
                        let x_first = vis.left().floor();
                        // menos de uma amostra por pixel: curva amostra a amostra
                        let one_sample = Ticks::from_samples(1, a.sample_rate).0.max(1);
                        let sample_mode = match (
                            e.media_time(self.tl.x2t(x_first, left), dur),
                            e.media_time(self.tl.x2t(x_first + 1.0, left).min(last_t), dur),
                        ) {
                            (Some(m0), Some(m1)) => (m1 - m0).0.abs() < one_sample,
                            _ => false,
                        };
                        let stroke = Stroke::new(1.0_f32, col);
                        let curve = Stroke::new(1.2_f32, col);
                        let mut shapes = Vec::with_capacity(vis.width() as usize * chans.len() + 4);
                        for (ch, cr) in &chans {
                            let (ya, yb) = (cr.top().min(cr.bottom()), cr.top().max(cr.bottom()));
                            let mid = cr.center().y;
                            let amp = cr.height() * 0.46 * gain;
                            let y_of = |v: f32| (mid - v * amp).clamp(ya, yb);
                            shapes.push(Shape::line_segment(
                                [pos2(vis.left(), mid), pos2(vis.right(), mid)],
                                Stroke::new(1.0_f32, center_col),
                            ));
                            let mut pts: Vec<Pos2> = Vec::new();
                            let mut x = x_first;
                            while x < vis.right() {
                                let t0 = self.tl.x2t(x, left);
                                let fg = {
                                    let local = t0 - e.start;
                                    let mut g = 1.0_f32;
                                    if eff_fi.len > Ticks::ZERO && local < eff_fi.len {
                                        g *= eff_fi
                                            .curve
                                            .shape(local.fraction_in(Ticks::ZERO, eff_fi.len));
                                    }
                                    let rem = e.end() - t0;
                                    if eff_fo.len > Ticks::ZERO && rem < eff_fo.len {
                                        g *= eff_fo
                                            .curve
                                            .shape(rem.fraction_in(Ticks::ZERO, eff_fo.len));
                                    }
                                    g
                                };
                                if sample_mode {
                                    let tc = self.tl.x2t(x + 0.5, left).min(last_t);
                                    if let Some(m) = e.media_time(tc, dur) {
                                        let v = a.sample_channel(*ch, m) * fg;
                                        pts.push(pos2(x + 0.5, y_of(v)));
                                    } else if pts.len() > 1 {
                                        shapes.push(Shape::line(std::mem::take(&mut pts), curve));
                                    } else {
                                        pts.clear();
                                    }
                                } else {
                                    let t1 = self.tl.x2t(x + 1.0, left);
                                    if let (Some(m0), Some(m1)) =
                                        (e.media_time(t0, dur), e.media_time(t1.min(last_t), dur))
                                    {
                                        let (m0, m1) = if m0 <= m1 { (m0, m1) } else { (m1, m0) };
                                        let (lo, hi) = a.channel_range(*ch, m0, m1);
                                        shapes.push(Shape::line_segment(
                                            [
                                                pos2(x + 0.5, y_of(hi * fg)),
                                                pos2(x + 0.5, y_of(lo * fg) + 0.5),
                                            ],
                                            stroke,
                                        ));
                                    }
                                }
                                x += 1.0;
                            }
                            if pts.len() > 1 {
                                shapes.push(Shape::line(pts, curve));
                            }
                        }
                        painter.with_clip_rect(vis).extend(shapes);
                    }
                }
                // nome e indicadores
                let mut label = e.name.clone();
                if !e.rate.is_normal() {
                    label.push_str(&format!("  [{:.0}%]", e.rate.percent_f64()));
                }
                if e.pitch_semitones.abs() > 0.05 {
                    label.push_str(&format!("  [{:+.1} st]", e.pitch_semitones));
                }
                if e.reverse {
                    label.push_str("  [Rev]");
                }
                if e.looped
                    && self
                        .project
                        .media(e.media_id)
                        .and_then(|m| m.duration)
                        .map(|d| e.rate.scale_ticks(e.length) > d + Ticks::from_millis_i64(1))
                        .unwrap_or(false)
                {
                    label.push_str("  [Loop]");
                }
                if e.group.is_some() {
                    label.push_str("  [Grp]");
                }
                let is_sync = e.sync_link_master.is_some() || sync_masters.contains(&e.id);
                if e.sync_link_master.is_some() {
                    label.push_str("  [Sync←]");
                } else if sync_masters.contains(&e.id) {
                    label.push_str("  [Sync★]");
                }
                if let Some(tr) = &e.transition {
                    label = format!("[{}] {}", tr.kind.name(), label);
                }
                let is_video_track = track.kind == TrackKind::Video;
                let (fxb, pcb, moreb) = self.event_buttons(&r, is_video_track);
                let top_btns_w = if r.height() >= 34.0 {
                    4.0
                } else {
                    let min_x = [fxb, pcb, moreb]
                        .into_iter()
                        .flatten()
                        .map(|b| b.left())
                        .fold(r.right(), f32::min);
                    (r.right() - min_x + 4.0).max(4.0)
                };
                let mut text_right = (r.right() - top_btns_w).min(lanes.right());
                if r.width() >= 150.0 && !self.event_button_hidden(is_video_track, "duration") {
                    let dur_str = self.fmt_time(e.length);
                    let (dur_galley, dur_w) = painter.fonts(|f| {
                        crate::fonts::tabular_galley(
                            f,
                            &dur_str,
                            crate::fonts::visor_font(9.5),
                            hdr_fg.gamma_multiply(0.85),
                        )
                    });
                    if text_right - dur_w - 8.0 > r.left().max(lanes.left()) + 36.0 {
                        let dx = text_right - dur_w - 4.0;
                        painter.with_clip_rect(hdr.intersect(lanes)).galley(
                            pos2(dx, hdr.top() + 1.5),
                            dur_galley,
                            hdr_fg,
                        );
                        text_right = dx - 4.0;
                    }
                }
                let label_left = r.left().max(lanes.left()) + 3.0;
                let label_clip =
                    Rect::from_min_max(hdr.min, pos2(text_right.max(hdr.left()), hdr.bottom()))
                        .intersect(lanes);
                if label_clip.width() > 8.0 {
                    let galley = painter.layout_no_wrap(
                        label,
                        FontId::proportional(11.0),
                        if self.theme.high_contrast {
                            hdr_fg
                        } else {
                            Color32::from_rgb(238, 240, 244)
                        },
                    );
                    if !self.theme.high_contrast {
                        let pill_w =
                            (galley.size().x + 8.0).min((text_right - label_left).max(0.0));
                        if pill_w > 6.0 {
                            let pill = Rect::from_min_size(
                                pos2(label_left, hdr.top() + 1.0),
                                vec2(pill_w, (hdr.height() - 2.0).max(11.0)),
                            );
                            painter.with_clip_rect(label_clip).rect_filled(
                                pill,
                                2.0,
                                Color32::from_rgba_unmultiplied(16, 18, 22, 165),
                            );
                        }
                    }
                    painter.with_clip_rect(label_clip).galley(
                        pos2(label_left + 3.0, hdr.top() + 1.0),
                        galley,
                        hdr_fg,
                    );
                }
                // botões do evento (canto inferior direito no padrão VEGAS Pro)
                let btn_bg = if body_light && self.theme.high_contrast {
                    Color32::from_rgba_unmultiplied(255, 255, 255, 220)
                } else {
                    Color32::from_rgba_unmultiplied(22, 24, 28, 205)
                };
                let btn_muted = crate::theme::contrast_on(btn_bg).gamma_multiply(0.82);
                if let Some(cluster) = [fxb, pcb, moreb].into_iter().flatten().reduce(Rect::union) {
                    let tray = cluster.expand2(vec2(2.0, 1.5));
                    painter.rect_filled(
                        tray,
                        3.0,
                        Color32::from_rgba_unmultiplied(14, 15, 18, 170),
                    );
                    painter.rect_stroke(
                        tray,
                        3.0,
                        Stroke::new(1.0_f32, self.theme.border.gamma_multiply(0.8)),
                        StrokeKind::Inside,
                    );
                }
                if let Some(b) = fxb {
                    let col = if e.fx.is_empty() {
                        btn_muted
                    } else if self.theme.high_contrast {
                        self.theme.accent
                    } else {
                        Color32::from_rgb(120, 230, 130)
                    };
                    painter.rect_filled(b, 2.0, btn_bg);
                    painter.rect_stroke(b, 2.0, Stroke::new(1.0_f32, col), StrokeKind::Inside);
                    painter.text(
                        b.center(),
                        Align2::CENTER_CENTER,
                        "fx",
                        FontId::proportional(9.0),
                        col,
                    );
                }
                if let Some(b) = pcb {
                    let active = !e.pan_crop.is_identity() || e.pan_crop.keys.len() > 1;
                    let col = if !active {
                        btn_muted
                    } else if self.theme.high_contrast {
                        self.theme.accent
                    } else {
                        Color32::from_rgb(115, 200, 255)
                    };
                    painter.rect_filled(b, 2.0, btn_bg);
                    painter.rect_stroke(b, 2.0, Stroke::new(1.0_f32, col), StrokeKind::Inside);
                    let mut pc_theme = ev_icon_theme.clone();
                    pc_theme.text_primary = col;
                    pc_theme.text_muted = col;
                    icons::paint(&painter, b.shrink(1.5), Icon::PanCrop, &pc_theme, active);
                }
                if let Some(b) = moreb {
                    painter.rect_filled(b, 2.0, btn_bg);
                    painter.rect_stroke(
                        b,
                        2.0,
                        Stroke::new(1.0_f32, btn_muted.gamma_multiply(0.65)),
                        StrokeKind::Inside,
                    );
                    painter.text(
                        b.center(),
                        Align2::CENTER_CENTER,
                        "…",
                        FontId::proportional(10.0),
                        btn_muted,
                    );
                }
                // fades
                let fade_col = Stroke::new(1.5_f32, self.theme.tl_fade);
                let draw_fade = |len: Ticks, curve: Curve, fade_in: bool| {
                    if len <= Ticks::ZERO {
                        return;
                    }
                    let w = len.as_secs_f64() as f32 * pps;
                    let n = (w / 3.0).clamp(2.0, 60.0) as usize;
                    let mut pts = vec![];
                    for i in 0..=n {
                        let x = i as f32 / n as f32;
                        let g = curve.shape(x);
                        let (px, py) = if fade_in {
                            (r.left() + x * w, r.bottom() - g * (r.height() - 2.0))
                        } else {
                            (r.right() - x * w, r.bottom() - g * (r.height() - 2.0))
                        };
                        pts.push(pos2(px, py));
                    }
                    painter.add(Shape::line(pts, fade_col));
                };
                if xf_in.is_none() {
                    draw_fade(e.fade_in.len, e.fade_in.curve, true);
                }
                if xf_out.is_none() {
                    draw_fade(e.fade_out.len, e.fade_out.curve, false);
                }
                // alças de fade (ocultas na borda já governada por crossfade ativo)
                let fi_x = r.left() + e.fade_in.len.as_secs_f64() as f32 * pps;
                let fo_x = r.right() - e.fade_out.len.as_secs_f64() as f32 * pps;
                if xf_in.is_none() {
                    painter.rect_filled(
                        Rect::from_center_size(pos2(fi_x, r.top() + 3.0), vec2(6.0, 6.0)),
                        1.0,
                        self.theme.tl_fade,
                    );
                }
                if xf_out.is_none() {
                    painter.rect_filled(
                        Rect::from_center_size(pos2(fo_x, r.top() + 3.0), vec2(6.0, 6.0)),
                        1.0,
                        self.theme.tl_fade,
                    );
                }
                // Triângulos invertidos estilo VEGAS Pro indicando início, fim e
                // pontos de repetição (loop) da mídia subjacente no topo do evento.
                let media_dur = self.project.media(e.media_id).and_then(|m| m.duration);
                let (at_start, at_end, interior_notches) = e.media_boundary_ticks(media_dur);
                let notch_fill = if self.theme.dark_mode {
                    Color32::from_rgb(236, 240, 246)
                } else {
                    Color32::from_rgb(28, 32, 40)
                };
                let notch_stroke = Stroke::new(
                    1.0_f32,
                    if self.theme.dark_mode {
                        Color32::from_rgb(20, 22, 26)
                    } else {
                        Color32::from_rgb(245, 246, 250)
                    },
                );
                let draw_notch = |cx: f32, half_w: f32, nh: f32| {
                    let y_top = r.top() + 0.5;
                    painter.add(Shape::convex_polygon(
                        vec![
                            pos2(cx - half_w, y_top),
                            pos2(cx + half_w, y_top),
                            pos2(cx, y_top + nh),
                        ],
                        notch_fill,
                        notch_stroke,
                    ));
                };
                for t_notch in interior_notches {
                    let nx = self.tl.t2x(t_notch, left);
                    if nx > r.left() + 4.0 && nx < r.right() - 4.0 {
                        draw_notch(nx, 4.5, 6.5);
                    }
                }
                if r.width() >= 22.0 {
                    if at_start && e.fade_in.len <= Ticks::ZERO {
                        let y_top = r.top() + 0.5;
                        painter.add(Shape::convex_polygon(
                            vec![
                                pos2(r.left() + 1.0, y_top),
                                pos2(r.left() + 7.5, y_top),
                                pos2(r.left() + 1.0, y_top + 6.0),
                            ],
                            notch_fill,
                            notch_stroke,
                        ));
                    }
                    if at_end && e.fade_out.len <= Ticks::ZERO {
                        let y_top = r.top() + 0.5;
                        painter.add(Shape::convex_polygon(
                            vec![
                                pos2(r.right() - 7.5, y_top),
                                pos2(r.right() - 1.0, y_top),
                                pos2(r.right() - 1.0, y_top + 6.0),
                            ],
                            notch_fill,
                            notch_stroke,
                        ));
                    }
                }
                // Barra grossa de ganho (áudio) ou opacidade/nível (vídeo) no evento
                let pointer_pos = ui.ctx().pointer_hover_pos();
                if r.width() > 24.0 && r.height() >= 18.0 {
                    let x0 = (fi_x + 5.0).min(r.right() - 5.0);
                    let x1 = (fo_x - 5.0).max(x0);
                    if x1 > x0 + 4.0 {
                        let backing_col = if self.theme.dark_mode {
                            Color32::from_rgba_unmultiplied(10, 12, 16, 200)
                        } else {
                            Color32::from_rgba_unmultiplied(255, 255, 255, 210)
                        };
                        if track.kind == TrackKind::Audio {
                            let gy = Self::gain_handle_y(r, e.gain_db);
                            let dragging_gain =
                                matches!(self.tl.drag, Drag::Gain { id, .. } if id == e.id);
                            let hovering_gain = pointer_pos.is_some_and(|hp| {
                                (hp.y - gy).abs() <= 7.5 && hp.x >= x0 && hp.x <= x1
                            });
                            let thick = if dragging_gain || hovering_gain {
                                4.5_f32
                            } else {
                                3.0_f32
                            };
                            painter.line_segment(
                                [pos2(x0, gy), pos2(x1, gy)],
                                Stroke::new(thick + 2.0, backing_col),
                            );
                            painter.line_segment(
                                [pos2(x0, gy), pos2(x1, gy)],
                                Stroke::new(thick, self.theme.tl_gain_line),
                            );
                            if dragging_gain || hovering_gain || e.gain_db.abs() > 0.05 {
                                painter.rect_filled(
                                    Rect::from_center_size(
                                        pos2((x0 + x1) * 0.5, gy),
                                        vec2(18.0, 5.5),
                                    ),
                                    2.5,
                                    self.theme.tl_gain_line,
                                );
                            }
                            if dragging_gain || hovering_gain {
                                let g = painter.layout_no_wrap(
                                    format!("{:+.1} dB", e.gain_db),
                                    FontId::monospace(10.0),
                                    self.theme.text_primary,
                                );
                                let bg = Rect::from_center_size(
                                    pos2((x0 + x1) * 0.5, (gy + 14.0).min(r.bottom() - 9.0)),
                                    g.size() + vec2(8.0, 4.0),
                                );
                                painter.rect_filled(bg, 2.0, self.theme.extreme_bg);
                                painter.rect_stroke(
                                    bg,
                                    2.0,
                                    Stroke::new(1.0_f32, self.theme.border),
                                    StrokeKind::Inside,
                                );
                                painter.galley(bg.min + vec2(4.0, 2.0), g, self.theme.text_primary);
                            }
                        } else if track.kind == TrackKind::Video {
                            let oy = Self::opacity_handle_y(r, e.opacity);
                            let dragging_op =
                                matches!(self.tl.drag, Drag::Opacity { id, .. } if id == e.id);
                            let hovering_op = pointer_pos.is_some_and(|hp| {
                                (hp.y - oy).abs() <= 7.5 && hp.x >= x0 && hp.x <= x1
                            });
                            let op_col = if self.theme.dark_mode {
                                Color32::from_rgb(95, 185, 255)
                            } else {
                                Color32::from_rgb(20, 105, 205)
                            };
                            let thick = if dragging_op || hovering_op {
                                4.5_f32
                            } else {
                                3.0_f32
                            };
                            painter.line_segment(
                                [pos2(x0, oy), pos2(x1, oy)],
                                Stroke::new(thick + 2.0, backing_col),
                            );
                            painter.line_segment(
                                [pos2(x0, oy), pos2(x1, oy)],
                                Stroke::new(thick, op_col),
                            );
                            if dragging_op || hovering_op || (1.0 - e.opacity).abs() > 0.005 {
                                painter.rect_filled(
                                    Rect::from_center_size(
                                        pos2((x0 + x1) * 0.5, oy),
                                        vec2(18.0, 5.5),
                                    ),
                                    2.5,
                                    op_col,
                                );
                            }
                            if dragging_op || hovering_op {
                                let pct_str =
                                    format!("{:.0}", (e.opacity * 100.0).clamp(0.0, 100.0));
                                let txt =
                                    tr_args(self.lang(), "Opacidade: {pct}%", &[("pct", &pct_str)]);
                                let g = painter.layout_no_wrap(
                                    txt,
                                    FontId::monospace(10.0),
                                    self.theme.text_primary,
                                );
                                let bg = Rect::from_center_size(
                                    pos2((x0 + x1) * 0.5, (oy + 14.0).min(r.bottom() - 9.0)),
                                    g.size() + vec2(8.0, 4.0),
                                );
                                painter.rect_filled(bg, 2.0, self.theme.extreme_bg);
                                painter.rect_stroke(
                                    bg,
                                    2.0,
                                    Stroke::new(1.0_f32, self.theme.border),
                                    StrokeKind::Inside,
                                );
                                painter.galley(bg.min + vec2(4.0, 2.0), g, self.theme.text_primary);
                            }
                        }
                    }
                }
                let border = if sel {
                    let sel_col = if self.opts.auto_ripple {
                        self.theme.offline_fg
                    } else {
                        self.theme.tl_event_sel_border
                    };
                    Stroke::new(2.0_f32, sel_col)
                } else if is_sync {
                    let sync_col = if self.theme.dark_mode {
                        Color32::from_rgb(168, 120, 240)
                    } else {
                        Color32::from_rgb(110, 55, 190)
                    };
                    Stroke::new(1.5_f32, sync_col)
                } else {
                    Stroke::new(1.0_f32, self.theme.tl_event_border)
                };
                painter.rect_stroke(r, 2.0, border, StrokeKind::Inside);
                // Evento de Ajuste (VEGAS Pro 20/2026): faixa violeta + etiqueta FX AJUSTE
                if e.is_adjustment {
                    let ev_clip = painter.with_clip_rect(r.intersect(lanes));
                    let adj_col = Color32::from_rgb(154, 92, 224);
                    ev_clip.rect_filled(
                        Rect::from_min_max(r.min, pos2(r.right(), (r.top() + 5.0).min(r.bottom()))),
                        0.0,
                        adj_col,
                    );
                    if r.width() > 54.0 {
                        let badge_g = ev_clip.layout_no_wrap(
                            tr(self.lang(), "FX AJUSTE").to_string(),
                            FontId::proportional(9.5),
                            Color32::WHITE,
                        );
                        let br = Rect::from_min_size(
                            pos2(r.left() + 4.0, (r.bottom() - 16.0).max(r.top() + 6.0)),
                            badge_g.size() + vec2(6.0, 2.0),
                        );
                        ev_clip.rect_filled(br, 2.0, adj_col);
                        ev_clip.galley(br.min + vec2(3.0, 1.0), badge_g, Color32::WHITE);
                    }
                }
                // Envelope de Velocidade no evento de vídeo (curva verde -100%..+300%)
                if track.kind == TrackKind::Video && self.opts.show_envelopes {
                    if let Some(venv) = e.velocity_envelope().filter(|v| v.visible) {
                        let vis_r = r.intersect(lanes);
                        if vis_r.width() > 4.0 && vis_r.height() > 12.0 {
                            let vp = painter.with_clip_rect(vis_r);
                            let vcol = Color32::from_rgb(76, 224, 108);
                            let mut pts = Vec::new();
                            let mut sx = vis_r.left();
                            while sx <= vis_r.right() + 3.0 {
                                let local_t = (self.tl.x2t(sx, left) - e.start)
                                    .clamp_safe(Ticks::ZERO, e.length);
                                let spd = venv.eval(local_t);
                                pts.push(pos2(
                                    sx,
                                    Self::env_y(EnvKind::Velocity, spd, r.top(), r.height()),
                                ));
                                sx += 3.0;
                            }
                            if pts.len() >= 2 {
                                vp.add(Shape::line(pts, Stroke::new(1.8_f32, vcol)));
                            }
                            for pt in &venv.points {
                                let px = self.tl.t2x(e.start + pt.t, left);
                                if px >= vis_r.left() - 4.0 && px <= vis_r.right() + 4.0 {
                                    let py =
                                        Self::env_y(EnvKind::Velocity, pt.v, r.top(), r.height());
                                    vp.rect_filled(
                                        Rect::from_center_size(pos2(px, py), vec2(6.0, 6.0)),
                                        1.0,
                                        vcol,
                                    );
                                }
                            }
                        }
                    }
                }
                // Alerta de dessincronia A/V entre eventos agrupados (pílula vermelha VEGAS Pro)
                if let Some(sync_off) = neoveg_engine::engine::audio_tools::event_sync_offset(
                    &self.project,
                    track.kind,
                    e,
                ) {
                    let vis_r = r.intersect(lanes);
                    if vis_r.width() > 38.0 {
                        let sp = painter.with_clip_rect(vis_r);
                        let secs = sync_off.delta_ticks.as_secs_f64();
                        let txt = format!("{secs:+.2}s ({:+}q)", sync_off.delta_frames);
                        let g = sp.layout_no_wrap(txt, FontId::monospace(9.5), Color32::WHITE);
                        let pr = Rect::from_min_size(
                            pos2(vis_r.left() + 3.0, (r.bottom() - 16.0).max(r.top() + 2.0)),
                            g.size() + vec2(6.0, 2.0),
                        );
                        sp.rect_filled(pr, 2.0, Color32::from_rgb(198, 42, 42));
                        sp.galley(pr.min + vec2(3.0, 1.0), g, Color32::WHITE);
                    }
                }
                if e.locked {
                    let lock_rect = Rect::from_min_size(
                        pos2(r.right() - 15.0, r.bottom() - 14.0),
                        vec2(11.0, 11.0),
                    );
                    let lock_painter = painter.with_clip_rect(r.intersect(lanes));
                    icons::paint(
                        &lock_painter,
                        lock_rect,
                        Icon::LockClosed,
                        &ev_icon_theme,
                        true,
                    );
                }
            }
            // Crossfades desenhados ACIMA dos eventos (curvas X reais + hachura VEGAS)
            let hover_pos = ui.ctx().pointer_hover_pos();
            for xf in &xfades {
                let x0 = self.tl.t2x(xf.start, left);
                let x1 = self.tl.t2x(xf.end, left);
                if x1 <= lanes.left() || x0 >= lanes.right() || x1 - x0 < 1.0 {
                    continue;
                }
                let xr = Rect::from_min_max(pos2(x0, y0 + 1.0), pos2(x1, y0 + h - 1.0));
                let vis_xr = xr.intersect(lanes);
                if vis_xr.width() <= 0.5 || vis_xr.height() <= 2.0 {
                    continue;
                }
                let xp = painter.with_clip_rect(vis_xr);
                if !xf.active {
                    // Sobreposição sem crossfade (corte seco): apenas divisor vertical nítido.
                    xp.line_segment(
                        [pos2(xr.left(), xr.top()), pos2(xr.left(), xr.bottom())],
                        Stroke::new(1.5_f32, self.theme.tl_event_border),
                    );
                    continue;
                }
                let sliding_this = matches!(
                    &self.tl.drag,
                    Drag::SlideCrossfade { pairs, .. }
                        if pairs.iter().any(|p| p.left_id == xf.left_id && p.right_id == xf.right_id)
                );
                let hovering_this = hover_pos.is_some_and(|hp| vis_xr.contains(hp))
                    && matches!(self.tl.drag, Drag::None);
                let is_hot = sliding_this || hovering_this;

                // Fundo e textura sutil de crossfade (VEGAS Pro)
                let veil = if self.theme.high_contrast {
                    self.theme.tl_crossfade.gamma_multiply(0.14)
                } else if self.theme.dark_mode {
                    Color32::from_rgba_unmultiplied(185, 210, 245, if is_hot { 34 } else { 22 })
                } else {
                    Color32::from_rgba_unmultiplied(35, 65, 125, if is_hot { 34 } else { 22 })
                };
                xp.rect_filled(vis_xr, 0.0, veil);

                // Hachura diagonal sutil na região de sobreposição (limitada a 160 traços, §0.5)
                if !self.theme.high_contrast && vis_xr.width() >= 6.0 {
                    let hatch_col = if self.theme.dark_mode {
                        Color32::from_rgba_unmultiplied(230, 240, 255, 22)
                    } else {
                        Color32::from_rgba_unmultiplied(40, 50, 75, 26)
                    };
                    let hatch_stroke = Stroke::new(1.0_f32, hatch_col);
                    let diag_span = vis_xr.width() + vis_xr.height();
                    let step = (diag_span / 160.0).max(6.0);
                    let mut d = 0.0_f32;
                    let mut segs = 0usize;
                    while d < diag_span && segs < 160 {
                        let p0 = pos2(vis_xr.left() + d, vis_xr.top());
                        let p1 = pos2(vis_xr.left() + d - vis_xr.height(), vis_xr.bottom());
                        xp.line_segment([p0, p1], hatch_stroke);
                        d += step;
                        segs += 1;
                    }
                }

                // Bordas verticais de início e fim do crossfade
                let edge_col = if self.theme.high_contrast {
                    self.theme.tl_crossfade
                } else if self.theme.dark_mode {
                    Color32::from_rgba_unmultiplied(210, 225, 245, 150)
                } else {
                    Color32::from_rgba_unmultiplied(45, 55, 78, 175)
                };
                xp.line_segment(
                    [
                        pos2(xr.left() + 0.5, xr.top()),
                        pos2(xr.left() + 0.5, xr.bottom()),
                    ],
                    Stroke::new(1.0_f32, edge_col),
                );
                xp.line_segment(
                    [
                        pos2(xr.right() - 0.5, xr.top()),
                        pos2(xr.right() - 0.5, xr.bottom()),
                    ],
                    Stroke::new(1.0_f32, edge_col),
                );

                // Pequeno triângulo de sobreposição na base esquerda (estilo VEGAS Pro)
                if xr.width() >= 10.0 && xr.height() >= 18.0 {
                    let tri_fill = if self.theme.dark_mode {
                        Color32::from_rgb(236, 240, 246)
                    } else {
                        Color32::from_rgb(245, 247, 250)
                    };
                    let tri_stroke = Stroke::new(1.0_f32, Color32::from_rgb(20, 24, 32));
                    xp.add(Shape::convex_polygon(
                        vec![
                            pos2(xr.left() + 0.5, xr.bottom() - 6.5),
                            pos2(xr.left() + 6.5, xr.bottom() - 0.5),
                            pos2(xr.left() + 0.5, xr.bottom() - 0.5),
                        ],
                        tri_fill,
                        tri_stroke,
                    ));
                }

                // As duas curvas cruzadas reais: saída (A) descendo + entrada (B) subindo
                let steps = (xr.width() / 3.0).clamp(4.0, 64.0) as usize;
                let mut pts_out = Vec::with_capacity(steps + 1);
                let mut pts_in = Vec::with_capacity(steps + 1);
                let usable_h = (xr.height() - 3.0).max(1.0);
                for s in 0..=steps {
                    let frac = s as f32 / steps as f32;
                    let px = xr.left() + frac * xr.width();
                    let g_out = xf.out_curve.shape(1.0 - frac).clamp(0.0, 1.0);
                    let g_in = xf.in_curve.shape(frac).clamp(0.0, 1.0);
                    pts_out.push(pos2(px, xr.bottom() - 1.5 - g_out * usable_h));
                    pts_in.push(pos2(px, xr.bottom() - 1.5 - g_in * usable_h));
                }
                let backing_col = if self.theme.dark_mode {
                    Color32::from_rgba_unmultiplied(8, 10, 14, 175)
                } else {
                    Color32::from_rgba_unmultiplied(255, 255, 255, 185)
                };
                let backing_w = if is_hot { 4.0_f32 } else { 3.0_f32 };
                let curve_w = if is_hot { 2.25_f32 } else { 1.75_f32 };
                let curve_col = self.theme.tl_crossfade;
                xp.add(Shape::line(
                    pts_out.clone(),
                    Stroke::new(backing_w, backing_col),
                ));
                xp.add(Shape::line(
                    pts_in.clone(),
                    Stroke::new(backing_w, backing_col),
                ));
                xp.add(Shape::line(pts_out, Stroke::new(curve_w, curve_col)));
                xp.add(Shape::line(pts_in, Stroke::new(curve_w, curve_col)));

                // Selo de transição quando há transição de vídeo diferente de Dissolver
                let tr_name = track
                    .events
                    .get(xf.right_idx)
                    .and_then(|ev| ev.transition.as_ref())
                    .filter(|t| t.kind != TransitionKind::Dissolve)
                    .map(|t| t.kind.name_in(self.lang()));
                if let Some(tname) = tr_name {
                    if vis_xr.width() >= 44.0 {
                        let g = painter.layout_no_wrap(
                            tname.to_string(),
                            FontId::proportional(9.5),
                            Color32::WHITE,
                        );
                        let pw = (g.size().x + 8.0).min(vis_xr.width() - 4.0);
                        if pw > 12.0 {
                            let pill = Rect::from_center_size(
                                pos2(vis_xr.center().x, vis_xr.top() + 8.5),
                                vec2(pw, 13.0),
                            );
                            xp.rect_filled(
                                pill,
                                2.0,
                                Color32::from_rgba_unmultiplied(16, 22, 36, 210),
                            );
                            xp.rect_stroke(
                                pill,
                                2.0,
                                Stroke::new(1.0_f32, curve_col),
                                StrokeKind::Inside,
                            );
                            xp.galley(pos2(pill.left() + 4.0, pill.top() + 1.0), g, Color32::WHITE);
                        }
                    }
                }

                // Etiqueta ao passar o mouse ou deslizar o crossfade
                if is_hot {
                    let lang = self.lang();
                    let dur_s = self.fmt_time(xf.len());
                    let info = if let Some(tname) = tr_name {
                        format!(
                            "{tname}  •  {dur_s}  •  {} / {}",
                            xf.out_curve.name_in(lang),
                            xf.in_curve.name_in(lang)
                        )
                    } else {
                        format!(
                            "Crossfade  {dur_s}  •  {} / {}",
                            xf.out_curve.name_in(lang),
                            xf.in_curve.name_in(lang)
                        )
                    };
                    let g = painter.layout_no_wrap(
                        info,
                        FontId::monospace(10.0),
                        self.theme.text_primary,
                    );
                    let bw = g.size().x + 8.0;
                    let bh = g.size().y + 4.0;
                    let bx = (vis_xr.center().x - bw * 0.5).clamp(
                        lanes.left() + 2.0,
                        (lanes.right() - bw - 2.0).max(lanes.left() + 2.0),
                    );
                    let by = (xr.top() - bh - 2.0).max(lanes.top() + 2.0);
                    let badge = Rect::from_min_size(pos2(bx, by), vec2(bw, bh));
                    painter.rect_filled(badge, 2.0, self.theme.extreme_bg);
                    painter.rect_stroke(
                        badge,
                        2.0,
                        Stroke::new(1.0_f32, curve_col),
                        StrokeKind::Inside,
                    );
                    painter.galley(badge.min + vec2(4.0, 2.0), g, self.theme.text_primary);
                }
            }
            // envelopes
            self.draw_row_envelopes(&painter, lanes, left, y0, h, &track.envelopes);
        }
        // Trilha do Barramento de Vídeo (Ctrl+Shift+B)
        if let Some((y0, h)) = rows.video_bus_rect {
            if y0 + h >= lanes.top() && y0 <= lanes.bottom() {
                let lane = Rect::from_min_max(pos2(lanes.left(), y0), pos2(lanes.right(), y0 + h));
                painter.rect_filled(lane, 0.0, self.theme.tl_lane_even);
                painter.line_segment(
                    [
                        pos2(lane.left(), lane.bottom()),
                        pos2(lane.right(), lane.bottom()),
                    ],
                    Stroke::new(1.0_f32, self.theme.border),
                );
                painter.text(
                    pos2(lane.left() + 6.0, lane.top() + 3.0),
                    Align2::LEFT_TOP,
                    tr(self.lang(), "Barramento de Vídeo (Master)"),
                    FontId::proportional(10.0),
                    self.theme.text_muted,
                );
                self.draw_row_envelopes(
                    &painter,
                    lanes,
                    left,
                    y0,
                    h,
                    &self.project.master.video_envelopes,
                );
            }
        }
        // trilhas de barramentos auxiliares (B)
        for bi in 0..rows.bus_rects.len() {
            let Some(bus) = self.project.audio_buses.get(bi) else {
                break;
            };
            let (y0, h) = rows.bus_rects[bi];
            if y0 + h < lanes.top() || y0 > lanes.bottom() {
                continue;
            }
            let lane = Rect::from_min_max(pos2(lanes.left(), y0), pos2(lanes.right(), y0 + h));
            painter.rect_filled(lane, 0.0, self.theme.tl_lane_odd);
            painter.line_segment(
                [
                    pos2(lane.left(), lane.bottom()),
                    pos2(lane.right(), lane.bottom()),
                ],
                Stroke::new(1.0_f32, self.theme.border),
            );
            painter.text(
                pos2(lane.left() + 6.0, lane.top() + 3.0),
                Align2::LEFT_TOP,
                format!("Barramento: {}", bus.name),
                FontId::proportional(10.0),
                self.theme.text_muted,
            );
            self.draw_row_envelopes(&painter, lanes, left, y0, h, &bus.envelopes);
        }
        // valor do ponto de envelope durante o arrasto
        if let Drag::EnvPoint { owner, kind, idx } = self.tl.drag {
            if let (Some((y0, h)), Some(p)) = (
                rows.row_rect(owner),
                self.owner_envelopes(owner)
                    .and_then(|list| list.iter().find(|e| e.kind == kind))
                    .and_then(|e| e.points.get(idx)),
            ) {
                let g = painter.layout_no_wrap(
                    format!("{}  {}", kind.format(p.v), self.fmt_time(p.t)),
                    FontId::proportional(11.0),
                    self.theme.text_primary,
                );
                let px = (self.tl.t2x(p.t, left) + 8.0)
                    .min(lanes.right() - g.size().x - 10.0)
                    .max(lanes.left() + 2.0);
                let py = (Self::env_y(kind, p.v, y0, h) - 20.0).max(lanes.top() + 2.0);
                let bg = Rect::from_min_size(pos2(px, py), g.size() + vec2(8.0, 4.0));
                painter.rect_filled(bg, 2.0, self.theme.extreme_bg);
                painter.rect_stroke(
                    bg,
                    2.0,
                    Stroke::new(1.0_f32, self.theme.border),
                    StrokeKind::Inside,
                );
                painter.galley(bg.min + vec2(4.0, 2.0), g, self.theme.text_primary);
            }
        }
        // colchetes do Modo de Edição Expandido (p. 329)
        if let Some(exp) = &self.session.expanded_edit {
            if let Some(&(y0, h)) = layout.get(exp.track_idx) {
                let x = self.tl.t2x(exp.edit_point, left);
                let col = self.theme.accent;
                let st = Stroke::new(2.0_f32, col);
                painter.line_segment([pos2(x, y0), pos2(x, y0 + h)], st);
                painter.line_segment([pos2(x - 6.0, y0 + 2.0), pos2(x + 6.0, y0 + 2.0)], st);
                painter.line_segment(
                    [pos2(x - 6.0, y0 + h - 2.0), pos2(x + 6.0, y0 + h - 2.0)],
                    st,
                );
            }
        }
        // indicador de soltura
        if let Some((ti, t)) = self.tl.drop_hint {
            let x = self.tl.t2x(t, left);
            let (y0, h) = layout
                .get(ti)
                .cloned()
                .unwrap_or((lanes.bottom() - 30.0, 30.0));
            painter.rect_stroke(
                Rect::from_min_size(pos2(x, y0 + 1.0), vec2(80.0, h - 2.0)),
                2.0,
                Stroke::new(2.0_f32, self.theme.tl_playhead_edit),
                StrokeKind::Inside,
            );
        }
        if let Some(t) = self.tl.snap_line {
            let x = self.tl.t2x(t, left);
            painter.line_segment(
                [pos2(x, lanes.top()), pos2(x, lanes.bottom())],
                Stroke::new(
                    1.0_f32,
                    self.prefs
                        .display
                        .snap_color
                        .map_or(self.theme.tl_playhead_edit, |[r, g, b]| {
                            Color32::from_rgb(r, g, b)
                        }),
                ),
            );
        }
    }

    fn draw_cursors(&mut self, ui: &mut egui::Ui, area: Rect) {
        let painter = ui.painter_at(area);
        let left = area.left();
        // marcadores (linhas pontilhadas conforme Preferências > Aparência)
        if self.prefs.always_draw_marker_lines || self.opts.snapping {
            for m in &self.project.markers {
                let x = self.tl.t2x(m.t, left);
                let mut y = area.top() + TOP_H;
                while y < area.bottom() {
                    painter.line_segment(
                        [pos2(x, y), pos2(x, (y + 4.0).min(area.bottom()))],
                        Stroke::new(1.0_f32, self.theme.tl_region),
                    );
                    y += 8.0;
                }
            }
        }
        let x = self.tl.t2x(self.cursor, left);
        let ph_col = if self.opts.auto_ripple {
            self.theme.offline_fg
        } else {
            self.theme.tl_playhead_edit
        };
        let ph_rim = crate::theme::contrast_on(ph_col);
        painter.line_segment(
            [pos2(x, area.top() + LOOP_H), pos2(x, area.bottom())],
            Stroke::new(1.5_f32, ph_col),
        );
        let head_y = area.top() + LOOP_H + MARKER_H;
        painter.add(Shape::convex_polygon(
            vec![
                pos2(x - 5.5, head_y - 4.0),
                pos2(x + 5.5, head_y - 4.0),
                pos2(x + 5.5, head_y),
                pos2(x, head_y + 7.0),
                pos2(x - 5.5, head_y),
            ],
            ph_col,
            Stroke::new(1.0_f32, ph_rim),
        ));
        if self.is_playing() {
            let px = self.tl.t2x(self.play_pos(), left);
            painter.line_segment(
                [pos2(px, area.top() + LOOP_H), pos2(px, area.bottom())],
                Stroke::new(1.5_f32, self.theme.tl_playhead_live),
            );
        }
    }
}

fn env_first_locked() -> bool {
    false
}

pub fn draw_meter_channel(painter: &egui::Painter, r: Rect, v: f32) {
    painter.rect_filled(r, 1.0, Color32::from_rgb(16, 17, 20));
    let db = lin_to_db(v).clamp(-60.0, 6.0);
    let n = ((db + 60.0) / 66.0).clamp(0.0, 1.0);
    if n <= 0.001 || r.height() < 2.0 {
        return;
    }
    let y_top = r.bottom() - n * r.height();
    // Faixas graduadas estilo console VEGAS Pro: verde (-60..-12 dB),
    // amarelo (-12..-2 dB) e vermelho (-2..+6 dB).
    let y_yellow = r.bottom() - ((-12.0 + 60.0) / 66.0) * r.height();
    let y_red = r.bottom() - ((-2.0 + 60.0) / 66.0) * r.height();
    let bands = [
        (
            y_top.max(y_yellow),
            r.bottom(),
            Color32::from_rgb(62, 194, 88),
        ),
        (y_top.max(y_red), y_yellow, Color32::from_rgb(232, 204, 52)),
        (y_top, y_red, Color32::from_rgb(236, 62, 54)),
    ];
    for (b_top, b_bot, col) in bands {
        if b_bot > b_top + 0.25 {
            painter.rect_filled(
                Rect::from_min_max(pos2(r.left(), b_top), pos2(r.right(), b_bot)),
                0.0,
                col,
            );
        }
    }
}

pub fn draw_meter(painter: &egui::Painter, r: Rect, lv: [f32; 2]) {
    painter.rect_filled(r, 1.0, Color32::from_rgb(16, 17, 20));
    let w = ((r.width() - 1.0) * 0.5).max(1.0);
    for (i, v) in lv.iter().enumerate() {
        let x0 = r.left() + i as f32 * (w + 1.0);
        let ch_r = Rect::from_min_max(pos2(x0, r.top()), pos2((x0 + w).min(r.right()), r.bottom()));
        draw_meter_channel(painter, ch_r, *v);
    }
}

/// Rolos A/B de *Expandir camadas da trilha* (manual do VEGAS Pro 21,
/// "Expanding or collapsing track layers"): os eventos seguidos, em ordem de
/// início, se alternam entre o rolo A (0) e o rolo B (1).
fn ab_rolls(events: &[Event]) -> Vec<(Id, u8)> {
    let mut order: Vec<(Ticks, Id)> = events.iter().map(|e| (e.start, e.id)).collect();
    order.sort();
    order
        .into_iter()
        .enumerate()
        .map(|(i, (_, id))| (id, (i % 2) as u8))
        .collect()
}

/// O menu de contexto das trilhas deixa de ser desenhado neste quadro?
///
/// Segurar o botão esquerdo (ou a caixa de seleção) impede **abrir** o menu,
/// para que esquerdo + direito vire a Ferramenta de Seleção (manual do VEGAS
/// Pro 21, p. 238). Um menu que já está aberto continua sendo desenhado: o
/// clique num item segura o botão esquerdo, e parar de desenhar o menu nesse
/// momento fazia ele sumir, o item não executar e o menu voltar ao soltar.
fn lane_ctx_menu_suppressed(
    menu_open: bool,
    suppress_flag: bool,
    primary_down: bool,
    box_drag: bool,
) -> bool {
    !menu_open && (suppress_flag || primary_down || box_drag)
}

#[cfg(test)]
mod layers_tests {
    use super::*;

    #[test]
    fn rolos_alternam_pela_ordem_de_inicio() {
        let m = MediaItem {
            id: 1,
            name: "m".into(),
            source: MediaSource::Generated(GenSpec::SolidColor {
                color: [0.0, 0.0, 0.0, 1.0],
            }),
            duration: Some(Ticks::from_secs_i64(10)),
            has_video: true,
            has_audio: false,
            width: 16,
            height: 16,
            fps: None,
            info: String::new(),
            proxy_path: None,
        };
        let s = Ticks::from_secs_i64;
        // fora de ordem no vetor: a ordem que vale é a do início
        let c = Event::new(&m, s(4), s(2));
        let a = Event::new(&m, s(0), s(2));
        let b = Event::new(&m, s(1), s(2));
        let rolls: HashMap<Id, u8> = ab_rolls(&[c.clone(), a.clone(), b.clone()])
            .into_iter()
            .collect();
        assert_eq!(rolls.get(&a.id), Some(&0));
        assert_eq!(rolls.get(&b.id), Some(&1));
        assert_eq!(rolls.get(&c.id), Some(&0));
        assert!(ab_rolls(&[]).is_empty());
    }
}

#[cfg(test)]
mod ctx_menu_tests {
    use super::lane_ctx_menu_suppressed;

    #[test]
    fn menu_aberto_continua_desenhado_ao_clicar_num_item() {
        // menu aberto + botão esquerdo pressionado sobre "Inserir texto aqui"
        assert!(!lane_ctx_menu_suppressed(true, false, true, false));
        assert!(!lane_ctx_menu_suppressed(true, true, true, true));
    }

    #[test]
    fn esquerdo_mais_direito_ainda_nao_abre_menu_novo() {
        assert!(lane_ctx_menu_suppressed(false, false, true, false));
        assert!(lane_ctx_menu_suppressed(false, true, false, false));
        assert!(lane_ctx_menu_suppressed(false, false, false, true));
        assert!(!lane_ctx_menu_suppressed(false, false, false, false));
    }
}

#[cfg(test)]
mod zoom_tests {
    use super::*;

    #[test]
    fn zoom_aceita_extremos_e_recusa_valores_hostis() {
        type S = TimelineState;
        assert_eq!(S::clamp_pps(f32::NAN), 60.0);
        assert_eq!(S::clamp_pps(f32::INFINITY), 60.0);
        assert_eq!(S::clamp_pps(-3.0), 60.0);
        assert_eq!(S::clamp_pps(0.0), 60.0);
        assert_eq!(S::clamp_pps(1.0e-30), S::MIN_PX_PER_SEC);
        assert_eq!(S::clamp_pps(1.0e30), S::MAX_PX_PER_SEC);
        // o antigo teto (4000 px/s) e o antigo piso (2 px/s) ficaram para trás
        assert_eq!(S::clamp_pps(1.0e6), 1.0e6);
        assert_eq!(S::clamp_pps(0.01), 0.01);
    }

    // no zoom máximo, uma amostra a 48 kHz ocupa milhares de pixels
    const _: () = assert!(TimelineState::MAX_PX_PER_SEC / 48_000.0 > 1000.0);

    #[test]
    fn regua_cabe_nos_dois_extremos() {
        let fps = FrameRate::FPS_25;
        for pps in [
            TimelineState::MIN_PX_PER_SEC,
            1.0e-3,
            0.01,
            0.5,
            60.0,
            1.0e5,
            TimelineState::MAX_PX_PER_SEC,
        ] {
            let (major, minor) = nice_step(pps, fps);
            assert!(minor.0 > 0 && major >= minor, "{pps}");
            // tela de 4000 px (o tempo para em 48 h): marcas abaixo do teto
            let view_secs = (4000.0 / f64::from(pps)).min(neoveg_engine::time::MAX_SECONDS as f64);
            let marks = view_secs / minor.as_secs_f64();
            assert!(marks <= f64::from(MAX_RULER_MARKS), "{pps}: {marks} marcas");
        }
    }

    #[test]
    fn zoom_ida_e_volta_mantem_a_ancora() {
        let mut tl = TimelineState {
            width: 1000.0,
            ..Default::default()
        };
        let anchor = Ticks::from_secs_i64(90);
        for _ in 0..200 {
            tl.zoom_by(2.0, Some(anchor));
        }
        assert_eq!(tl.px_per_sec, TimelineState::MAX_PX_PER_SEC);
        for _ in 0..400 {
            tl.zoom_by(0.5, Some(anchor));
        }
        assert_eq!(tl.px_per_sec, TimelineState::MIN_PX_PER_SEC);
        assert!(tl.scroll_x >= Ticks::ZERO);
    }

    #[test]
    fn zoom_parte_da_agulha_de_reproducao() {
        let mut tl = TimelineState {
            width: 1000.0,
            px_per_sec: 100.0,
            scroll_x: Ticks::from_secs_i64(10),
            ..Default::default()
        };
        // agulha visível, a 2 s do início da tela (200 px): fica no mesmo pixel
        let agulha = Ticks::from_secs_i64(12);
        for f in [4.0, 0.3, 50.0, 0.01] {
            tl.zoom_around_playhead(f, agulha);
            let px = (agulha - tl.scroll_x).as_secs_f64() * f64::from(tl.px_per_sec);
            assert!((px - 200.0).abs() < 1.0, "zoom {f}: agulha em {px} px");
        }
        // agulha fora da tela: vai para o centro
        tl.px_per_sec = 100.0;
        tl.scroll_x = Ticks::ZERO;
        let longe = Ticks::from_secs_i64(600);
        tl.zoom_around_playhead(2.0, longe);
        let px = (longe - tl.scroll_x).as_secs_f64() * f64::from(tl.px_per_sec);
        assert!((px - 500.0).abs() < 1.0, "agulha em {px} px");
    }

    #[test]
    fn largura_cabecalho_e_grade_personalizada_protegem_limites_e_teto_de_marcas() {
        let mut tl = TimelineState::default();
        tl.set_header_w(f32::NAN);
        assert_eq!(tl.header_w, HEADER_W);
        tl.set_header_w(1.0);
        assert_eq!(tl.header_w, TimelineState::MIN_HEADER_W);
        tl.set_header_w(9999.0);
        assert_eq!(tl.header_w, TimelineState::MAX_HEADER_W);
        tl.set_header_w(260.0);
        tl.toggle_header_collapse();
        assert_eq!(tl.header_w, TimelineState::COLLAPSED_HEADER_W);
        tl.toggle_header_collapse();
        assert!((tl.header_w - 260.0).abs() < 0.01);

        let settings = ProjectSettings::default();
        for g in neoveg_engine::session::RulerGridSpacing::ALL {
            for pps in [TimelineState::MIN_PX_PER_SEC, 0.5, 60.0, 5000.0] {
                let (maj, min) = effective_grid_step(g, pps, &settings, 2000.0);
                assert!(min.0 > 0 && maj >= min);
                let view_secs =
                    (2000.0 / f64::from(pps)).min(neoveg_engine::time::MAX_SECONDS as f64);
                let marks = view_secs / min.as_secs_f64();
                assert!(marks <= f64::from(MAX_RULER_MARKS), "{g:?} {pps}: {marks}");
            }
        }
    }

    #[test]
    fn icones_curvas_de_fade_sobem_na_entrada_e_descem_na_saida() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(28.0, 14.0));
        for c in Curve::ALL.iter().take(5) {
            let pin = fade_curve_points(r, *c, true);
            let pout = fade_curve_points(r, *c, false);
            assert_eq!(pin.len(), 17);
            assert_eq!(pout.len(), 17);
            // Na entrada, começa embaixo (y maior) e termina em cima (y menor)
            assert!(pin[0].y > pin[16].y, "{c:?} fade-in deve subir");
            // Na saída, começa em cima (y menor) e termina embaixo (y maior)
            assert!(pout[0].y < pout[16].y, "{c:?} fade-out deve descer");
        }
        // Fast sobe mais rápido que Slow no ponto médio (y menor na tela)
        let fast_mid = fade_curve_points(r, Curve::Fast, true)[8].y;
        let slow_mid = fade_curve_points(r, Curve::Slow, true)[8].y;
        assert!(fast_mid < slow_mid);
        // No fade-out, Fast também arqueia para cima (y menor que Slow no meio)
        let fast_out_mid = fade_curve_points(r, Curve::Fast, false)[8].y;
        let slow_out_mid = fade_curve_points(r, Curve::Slow, false)[8].y;
        assert!(fast_out_mid < slow_out_mid);

        // Renderiza os itens de menu de fade e crossfade em todos os 7 temas sem pânico
        let ctx = egui::Context::default();
        for tid in crate::theme::ThemeId::ALL {
            let th = crate::theme::palette(tid);
            let _ = ctx.run(egui::RawInput::default(), |c| {
                egui::CentralPanel::default().show(c, |ui| {
                    for &fade_in in &[true, false] {
                        for cur in Curve::ALL.iter().take(5) {
                            let _ = fade_curve_menu_item(
                                ui,
                                *cur,
                                fade_in,
                                *cur == Curve::Smooth,
                                cur.name(),
                                &th,
                            );
                        }
                    }
                    for &oc in &neoveg_engine::crossfade::CROSSFADE_CURVES {
                        for &ic in &neoveg_engine::crossfade::CROSSFADE_CURVES {
                            let _ = crossfade_curve_menu_item(
                                ui,
                                oc,
                                ic,
                                oc == Curve::Fast && ic == Curve::Fast,
                                "Crossfade",
                                &th,
                            );
                        }
                    }
                });
            });
        }
    }
}

/// Pontos da curva de fade normalizados dentro de `rect` (`fade_in`: sobe da
/// esquerda para a direita; `!fade_in`: desce da esquerda para a direita).
fn fade_curve_points(rect: Rect, curve: Curve, fade_in: bool) -> Vec<Pos2> {
    const STEPS: usize = 16;
    let mut pts = Vec::with_capacity(STEPS + 1);
    for i in 0..=STEPS {
        let frac = i as f32 / STEPS as f32;
        let v = if fade_in {
            curve.shape(frac)
        } else {
            curve.shape(1.0 - frac)
        };
        let x = rect.left() + frac * rect.width();
        let y = rect.bottom() - v.clamp(0.0, 1.0) * rect.height();
        pts.push(pos2(x, y));
    }
    pts
}

/// Desenha a miniatura vetorial da curva de fade (entrada ou saída),
/// seguindo o submenu *Fade Type* do VEGAS Pro (desenhado do zero em código).
fn paint_fade_curve_icon(
    painter: &egui::Painter,
    rect: Rect,
    curve: Curve,
    fade_in: bool,
    selected: bool,
    theme: &crate::theme::ThemeColors,
) {
    let bg = if selected && theme.high_contrast {
        theme.window_fill
    } else {
        theme.extreme_bg
    };
    painter.rect_filled(rect, 1.5, bg);
    let inner = rect.shrink2(vec2(2.0, 2.0));
    if inner.width() > 2.0 && inner.height() > 2.0 {
        let pts = fade_curve_points(inner, curve, fade_in);
        let fill_col = if theme.high_contrast {
            theme.accent.gamma_multiply(0.35)
        } else {
            theme.accent.gamma_multiply(0.28)
        };
        // Preenche em faixas verticais sob a curva (evita polígono não-convexo)
        for seg in pts.windows(2) {
            let [a, b] = [seg[0], seg[1]];
            let quad = vec![a, b, pos2(b.x, inner.bottom()), pos2(a.x, inner.bottom())];
            painter.add(Shape::convex_polygon(quad, fill_col, Stroke::NONE));
        }
        let line_col = if selected || theme.high_contrast {
            theme.accent
        } else {
            theme.tl_fade
        };
        painter.add(Shape::line(pts, Stroke::new(1.5_f32, line_col)));
    }
    painter.rect_stroke(
        rect,
        1.5,
        Stroke::new(1.0_f32, if selected { theme.accent } else { theme.border }),
        StrokeKind::Inside,
    );
}

/// Item de menu com representação gráfica da curva de fade ao lado do nome.
fn fade_curve_menu_item(
    ui: &mut egui::Ui,
    curve: Curve,
    fade_in: bool,
    selected: bool,
    label: &str,
    theme: &crate::theme::ThemeColors,
) -> egui::Response {
    let desired_w = ui.available_width().max(138.0);
    let (row_r, resp) = ui.allocate_exact_size(vec2(desired_w, 22.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let p = ui.painter_at(row_r);
    if selected {
        p.rect_filled(row_r, 2.0, theme.selection_bg);
    } else if resp.hovered() {
        p.rect_filled(row_r, 2.0, theme.widget_hover_bg);
    }
    let icon_r = Rect::from_min_size(
        pos2(row_r.left() + 4.0, row_r.center().y - 7.0),
        vec2(28.0, 14.0),
    );
    paint_fade_curve_icon(&p, icon_r, curve, fade_in, selected, theme);
    let fg = if selected {
        crate::theme::contrast_on(theme.selection_bg)
    } else {
        theme.text_primary
    };
    p.text(
        pos2(icon_r.right() + 7.0, row_r.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.0),
        fg,
    );
    if selected {
        p.text(
            pos2(row_r.right() - 6.0, row_r.center().y),
            Align2::RIGHT_CENTER,
            "✓",
            FontId::proportional(11.0),
            fg,
        );
    }
    resp
}

/// Desenha miniatura vetorial das duas curvas cruzadas de um crossfade
/// (`out_curve` descendo + `in_curve` subindo), para o submenu *Fade Type* 5×5.
fn paint_crossfade_curve_icon(
    painter: &egui::Painter,
    rect: Rect,
    out_curve: Curve,
    in_curve: Curve,
    selected: bool,
    hovered: bool,
    theme: &crate::theme::ThemeColors,
) {
    let bg = if selected {
        theme
            .selection_bg
            .gamma_multiply(if theme.high_contrast { 1.0 } else { 0.45 })
    } else if hovered {
        theme.widget_hover_bg
    } else {
        theme.extreme_bg
    };
    painter.rect_filled(rect, 1.5, bg);
    let inner = rect.shrink2(vec2(2.0, 2.0));
    if inner.width() > 2.0 && inner.height() > 2.0 {
        let out_pts = fade_curve_points(inner, out_curve, false);
        let in_pts = fade_curve_points(inner, in_curve, true);
        let out_col = if selected || theme.high_contrast {
            theme.accent
        } else {
            theme.tl_crossfade
        };
        let in_col = if selected || theme.high_contrast {
            theme.tl_fade
        } else {
            theme.tl_crossfade
        };
        painter.add(Shape::line(out_pts, Stroke::new(1.4_f32, out_col)));
        painter.add(Shape::line(in_pts, Stroke::new(1.4_f32, in_col)));
    }
    painter.rect_stroke(
        rect,
        1.5,
        Stroke::new(
            1.0_f32,
            if selected || hovered {
                theme.accent
            } else {
                theme.border
            },
        ),
        StrokeKind::Inside,
    );
}

/// Item de menu com miniatura das duas curvas cruzadas do crossfade ao lado do nome.
fn crossfade_curve_menu_item(
    ui: &mut egui::Ui,
    out_curve: Curve,
    in_curve: Curve,
    selected: bool,
    label: &str,
    theme: &crate::theme::ThemeColors,
) -> egui::Response {
    let desired_w = ui.available_width().max(220.0);
    let (row_r, resp) = ui.allocate_exact_size(vec2(desired_w, 22.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let p = ui.painter_at(row_r);
    if selected {
        p.rect_filled(row_r, 2.0, theme.selection_bg);
    } else if resp.hovered() {
        p.rect_filled(row_r, 2.0, theme.widget_hover_bg);
    }
    let icon_r = Rect::from_min_size(
        pos2(row_r.left() + 4.0, row_r.center().y - 7.0),
        vec2(28.0, 14.0),
    );
    paint_crossfade_curve_icon(
        &p,
        icon_r,
        out_curve,
        in_curve,
        selected,
        resp.hovered(),
        theme,
    );
    let fg = if selected {
        crate::theme::contrast_on(theme.selection_bg)
    } else {
        theme.text_primary
    };
    p.text(
        pos2(icon_r.right() + 7.0, row_r.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.0),
        fg,
    );
    if selected {
        p.text(
            pos2(row_r.right() - 6.0, row_r.center().y),
            Align2::RIGHT_CENTER,
            "✓",
            FontId::proportional(11.0),
            fg,
        );
    }
    resp
}

/// Ação pedida por um botão do cabeçalho de faixa ou pelo menu do botão Mais.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeaderAction {
    Mute,
    Solo,
    Fx,
    TrackMotion,
    Minimize,
    Maximize,
    ToggleCompositingChild,
    RecordArm,
    SetAutomation(AutomationMode),
    CycleAutomation,
    EditButtons,
}
