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
use std::collections::HashSet;

const HEADER_W: f32 = 250.0;
const LOOP_H: f32 = 12.0;
const MARKER_H: f32 = 18.0;
const RULER_H: f32 = 22.0;
const TOP_H: f32 = LOOP_H + MARKER_H + RULER_H;
const TRACK_FOOTER_H: f32 = 26.0;
const STATUS_BAR_H: f32 = 22.0;
const BOTTOM_BARS_H: f32 = TRACK_FOOTER_H + STATUS_BAR_H;
const EDGE_PX: f32 = 5.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EnvOwner {
    Track(usize),
    Bus(usize),
}

struct TimelineRows {
    track_rects: Vec<(f32, f32)>,
    bus_rects: Vec<(f32, f32)>,
}

impl TimelineRows {
    fn row_rect(&self, owner: EnvOwner) -> Option<(f32, f32)> {
        match owner {
            EnvOwner::Track(ti) => self.track_rects.get(ti).copied(),
            EnvOwner::Bus(bi) => self.bus_rects.get(bi).copied(),
        }
    }

    fn owner_at(&self, y: f32) -> Option<EnvOwner> {
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
    FxButton,
    PanCropButton,
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
    },
    Slip {
        origin_t: Ticks,
        orig: Vec<(Id, Ticks)>,
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
}

#[derive(Clone, Debug, Default)]
enum CtxTarget {
    #[default]
    None,
    Event(Id),
    EnvPoint(EnvOwner, EnvKind, usize),
    EnvLine(EnvOwner, EnvKind, Ticks),
    Lane(usize, Ticks),
    Marker(Id),
    Region(Id),
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
        }
    }
}

impl TimelineState {
    fn dx_to_ticks(&self, dx: f32) -> Ticks {
        Ticks::from_secs_f64_clamped((dx / self.px_per_sec.max(1.0)) as f64)
    }

    pub fn zoom_by(&mut self, f: f32, anchor: Option<Ticks>) {
        let anchor = anchor.unwrap_or(self.scroll_x + self.dx_to_ticks(self.width / 2.0));
        let ax = (anchor - self.scroll_x).as_secs_f64() as f32 * self.px_per_sec;
        self.px_per_sec = (self.px_per_sec * f).clamp(2.0, 4000.0);
        self.scroll_x = (anchor - self.dx_to_ticks(ax)).max(Ticks::ZERO);
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
    (
        Ticks::from_secs_i64(7200),
        Ticks::from_secs_i64(600).max(f1),
    )
}

const BUS_TRACK_H: f32 = 58.0;

impl App {
    fn timeline_rows(&self, lanes_top: f32) -> TimelineRows {
        let mut y = lanes_top - self.tl.scroll_y;
        let track_rects = self
            .project
            .tracks
            .iter()
            .map(|t| {
                let r = (y, t.height);
                y += t.height;
                r
            })
            .collect();
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
            track_rects,
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
        }
    }

    fn track_at(&self, y: f32, layout: &[(f32, f32)]) -> Option<usize> {
        layout.iter().position(|(y0, h)| y >= *y0 && y < y0 + h)
    }

    fn env_y(kind: EnvKind, v: f32, y0: f32, h: f32) -> f32 {
        y0 + h - 4.0 - kind.to_norm(v) * (h - 8.0)
    }

    fn event_rect(&self, e: &Event, left: f32, y0: f32, h: f32) -> Rect {
        Rect::from_min_max(
            pos2(self.tl.t2x(e.start, left), y0 + 1.0),
            pos2(self.tl.t2x(e.end(), left), y0 + h - 1.0),
        )
    }

    fn gain_handle_y(r: Rect, gain_db: f32) -> f32 {
        // Na semântica do VEGAS Pro, 0 dB fica no topo do evento de áudio e
        // arrastar para baixo atenua até -36 dB.
        let t = (-gain_db / 36.0).clamp(0.0, 1.0);
        let span = (r.height() - 8.0).max(0.0);
        (r.top() + 3.0 + t * span).clamp(r.top() + 2.0, (r.bottom() - 3.0).max(r.top() + 2.0))
    }

    fn event_buttons(r: &Rect, is_video: bool) -> (Option<Rect>, Option<Rect>) {
        if r.width() < 70.0 {
            return (None, None);
        }
        let fx = Rect::from_min_size(pos2(r.right() - 20.0, r.top() + 2.0), vec2(16.0, 13.0));
        let pc = if is_video {
            Some(Rect::from_min_size(
                pos2(r.right() - 38.0, r.top() + 2.0),
                vec2(16.0, 13.0),
            ))
        } else {
            None
        };
        (Some(fx), pc)
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
        for (ei, e) in track.events.iter().enumerate().rev() {
            let r = self.event_rect(e, left, y0, h);
            if !r.expand2(vec2(EDGE_PX, 0.0)).contains(pos) {
                continue;
            }
            let (fxb, pcb) = Self::event_buttons(&r, track.kind == TrackKind::Video);
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
            if track.kind == TrackKind::Audio && r.width() > 24.0 && r.height() >= 18.0 {
                let gy = Self::gain_handle_y(r, e.gain_db);
                if (pos.y - gy).abs() <= 4.5 && pos.x > fi_x + 5.0 && pos.x < fo_x - 5.0 {
                    return Some((ti, ei, Zone::GainHandle));
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
        let full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, Sense::hover());
        let left = full.left() + HEADER_W;
        let show_h_col = full.width() > HEADER_W + crate::scrollbars::TIMELINE_V_SCROLL_W + 72.0;
        let show_footer = full.height() > TOP_H + BOTTOM_BARS_H + 40.0;
        let bottom_h = if show_footer { BOTTOM_BARS_H } else { 0.0 };
        let show_v_col = show_h_col && full.height() > TOP_H + bottom_h + 60.0;
        let v_col_w = if show_v_col {
            crate::scrollbars::TIMELINE_V_SCROLL_W
        } else {
            0.0
        };
        let lanes_right = (full.right() - v_col_w).max(left + 10.0);
        let lanes_bottom = (full.bottom() - bottom_h).max(full.top() + TOP_H + 10.0);

        let corner = Rect::from_min_max(full.min, pos2(left, full.top() + TOP_H));
        let top = Rect::from_min_max(
            pos2(left, full.top()),
            pos2(lanes_right, full.top() + TOP_H),
        );
        let headers = Rect::from_min_max(
            pos2(full.left(), full.top() + TOP_H),
            pos2(left, lanes_bottom),
        );
        let lanes = Rect::from_min_max(
            pos2(left, full.top() + TOP_H),
            pos2(lanes_right, lanes_bottom),
        );
        self.tl.width = lanes.width().max(10.0);

        // limite de rolagem vertical
        let bus_h: f32 = if self.opts.show_bus_tracks {
            self.project.audio_buses.len() as f32 * BUS_TRACK_H
        } else {
            0.0
        };
        let total_h: f32 = self.project.tracks.iter().map(|t| t.height).sum::<f32>() + bus_h;
        self.tl.scroll_y = self
            .tl
            .scroll_y
            .clamp(0.0, (total_h - lanes.height() + 40.0).max(0.0));

        self.time_display_ui(ui, corner);
        self.top_strip(ui, top);
        if show_v_col {
            let cap = Rect::from_min_max(
                pos2(lanes_right, full.top()),
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
        self.headers_ui(ui, headers, &rows);

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
    }

    fn timeline_zoom_anchor(&self) -> Option<Ticks> {
        let vis_end = self.tl.scroll_x + self.tl.dx_to_ticks(self.tl.width);
        if self.cursor >= self.tl.scroll_x && self.cursor <= vis_end {
            Some(self.cursor)
        } else {
            None
        }
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
            let pps = ((lanes_width - 24.0).max(40.0) / target_secs).clamp(2.0, 4000.0);
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
        let step = (lanes_h * 0.18).clamp(28.0, 84.0);
        let res = crate::scrollbars::arrow_scrollbar(
            ui,
            egui::Id::new("tl_v_scroll"),
            scroll_r,
            crate::scrollbars::Axis::Vertical,
            v_range,
            step,
            "Rolar trilhas para cima",
            "Rolar trilhas para baixo",
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
                "Aumentar altura das trilhas",
                "Aumentar altura das trilhas (clique ou arraste verticalmente)",
                &self.theme,
            );
            let (dec_click, dec_drag) = crate::scrollbars::zoom_button(
                ui,
                egui::Id::new("tl_track_h_minus"),
                dec_r,
                crate::scrollbars::SymbolGlyph::Minus,
                crate::scrollbars::Axis::Vertical,
                can_dec,
                "Diminuir altura das trilhas",
                "Diminuir altura das trilhas (clique ou arraste verticalmente)",
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

        let view_secs = (lanes_w / self.tl.px_per_sec.max(1.0)).max(0.5);
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
        let step_secs = (view_secs * 0.15).max(self.tl.dx_to_ticks(48.0).as_secs_f64() as f32);
        let res = crate::scrollbars::arrow_scrollbar(
            ui,
            egui::Id::new("tl_h_scroll"),
            scroll_r,
            crate::scrollbars::Axis::Horizontal,
            h_range,
            step_secs,
            "Rolar timeline para a esquerda",
            "Rolar timeline para a direita",
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
            let anchor = self.timeline_zoom_anchor();
            let can_out = self.tl.px_per_sec > 2.01;
            let can_in = self.tl.px_per_sec < 3999.0;

            let (dec_click, dec_drag) = crate::scrollbars::zoom_button(
                ui,
                egui::Id::new("tl_h_zoom_out"),
                dec_r,
                crate::scrollbars::SymbolGlyph::Minus,
                crate::scrollbars::Axis::Horizontal,
                can_out,
                "Diminuir zoom horizontal",
                "Afastar zoom horizontal (clique ou arraste horizontalmente)",
                &self.theme,
            );
            let (inc_click, inc_drag) = crate::scrollbars::zoom_button(
                ui,
                egui::Id::new("tl_h_zoom_in"),
                inc_r,
                crate::scrollbars::SymbolGlyph::Plus,
                crate::scrollbars::Axis::Horizontal,
                can_in,
                "Aumentar zoom horizontal",
                "Aproximar zoom horizontal (clique ou arraste horizontalmente)",
                &self.theme,
            );

            if dec_click {
                self.tl.zoom_by(0.8, anchor);
            }
            if inc_click {
                self.tl.zoom_by(1.25, anchor);
            }
            let drag = dec_drag + inc_drag;
            if drag.abs() > 0.01 {
                let factor = (1.0 + drag * 0.012).clamp(0.5, 2.0);
                self.tl.zoom_by(factor, anchor);
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
                "Ajustar projeto à timeline",
                "Ajustar todo o projeto à largura visível da timeline (ou duplo clique na barra horizontal)",
                &self.theme,
            );
            if fit_click {
                self.fit_project_to_timeline(lanes_w);
            }
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
        painter.rect_filled(box_r, 2.0, self.theme.extreme_bg);
        painter.rect_stroke(
            box_r,
            2.0,
            Stroke::new(1.0_f32, self.theme.border),
            StrokeKind::Inside,
        );

        painter.text(
            pos2(box_r.left() + 6.0, box_r.top() + 3.0),
            Align2::LEFT_TOP,
            "Visor de tempo",
            FontId::proportional(9.5),
            self.theme.text_muted,
        );
        painter.text(
            pos2(box_r.right() - 6.0, box_r.top() + 3.0),
            Align2::RIGHT_TOP,
            self.opts.time_format.name(),
            FontId::proportional(9.5),
            self.theme.text_muted,
        );
        painter.text(
            pos2(box_r.right() - 8.0, box_r.bottom() - 3.0),
            Align2::RIGHT_BOTTOM,
            self.fmt_time(self.view_time()),
            FontId::monospace(20.0),
            self.theme.timecode_fg,
        );

        let resp = ui
            .interact(box_r, ui.id().with("timeline_time_display"), Sense::click())
            .on_hover_text(
                "Visor de tempo (duplo clique: Ir para tempo • clique direito: formato de tempo)",
            );
        if resp.double_clicked() {
            self.exec_cmd(Some(ui.ctx()), "edit.goto");
        }
        resp.context_menu(|ui| {
            ui.label(
                egui::RichText::new("Formato do Visor de Tempo")
                    .small()
                    .strong(),
            );
            ui.separator();
            for fmt in TimeFormat::ALL {
                if ui
                    .selectable_label(self.opts.time_format == fmt, fmt.name())
                    .clicked()
                {
                    self.session.opts.time_format = fmt;
                    ui.close_menu();
                }
            }
            ui.separator();
            if ui.button("Ir para tempo... (Ctrl+G)").clicked() {
                self.dlg.goto_open = true;
                ui.close_menu();
            }
        });
    }

    /// Barra de Status de Renderização e Pré-renderização na base da timeline.
    fn render_status_bar_ui(&mut self, ui: &mut egui::Ui, r: Rect) {
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
                ui.label(
                    egui::RichText::new(format!("Renderizando {name}: {done}/{total}"))
                        .small()
                        .strong(),
                );
                ui.add(
                    egui::ProgressBar::new(frac)
                        .desired_width(140.0)
                        .show_percentage(),
                );
                if ui.small_button("Cancelar").clicked() {
                    cancel_render = true;
                }
            } else if let Some((a, b, prog)) = &self.prerender_job {
                let frac = prog.fraction().clamp(0.0, 1.0);
                crate::splash::spinner_inline(ui, 13.0, ui.ctx().input(|i| i.time), &self.theme);
                ui.label(
                    egui::RichText::new(format!(
                        "Pré-renderizando {} → {}",
                        self.fmt_time(*a),
                        self.fmt_time(*b)
                    ))
                    .small()
                    .strong(),
                );
                ui.add(
                    egui::ProgressBar::new(frac)
                        .desired_width(130.0)
                        .show_percentage(),
                );
                if ui.small_button("Cancelar").clicked() {
                    cancel_prerender = true;
                }
            } else {
                let pre_count = self.session.prerendered_ranges.len();
                let pre_text = if pre_count > 0 {
                    format!("Status de renderização: {pre_count} trecho(s) pré-renderizado(s)")
                } else {
                    "Status de renderização: Pronto (sem trechos pré-renderizados)".to_string()
                };
                let pre_col = if pre_count > 0 {
                    self.theme.tl_marker
                } else {
                    self.theme.text_muted
                };
                ui.label(egui::RichText::new(pre_text).small().color(pre_col));
                if ui
                    .small_button("Pré-render (Shift+M)")
                    .on_hover_text("Pré-renderizar região selecionada e marcar no topo da timeline")
                    .clicked()
                {
                    trigger_prerender = true;
                }
                if ui
                    .small_button("Renderizar...")
                    .on_hover_text("Abrir janela Renderizar como (Ctrl+M)")
                    .clicked()
                {
                    trigger_render = true;
                }
                if pre_count > 0
                    && ui
                        .small_button("Limpar pré-render")
                        .on_hover_text("Limpar indicações de pré-render da timeline")
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
            self.set_status("Indicações de pré-renderização limpas.");
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
                .on_hover_text("Inserir trilha de vídeo (Ctrl+Shift+Q)")
                .clicked()
            {
                self.checkpoint();
                self.insert_track(TrackKind::Video);
            }
            if icons::small_icon_button(ui, Icon::AudioTrack, None, false, &self.theme)
                .on_hover_text("Inserir trilha de áudio (Ctrl+Q)")
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
        let fps = self.project.settings.fps;

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
        let (major, minor) = nice_step(self.tl.px_per_sec, fps);
        let step = minor.0.max(1);
        let maj = major.0.max(step);
        let half_minor = step / 4;
        let t_start = self.tl.scroll_x;
        let t_end = self.tl.x2t(r.right(), left);
        let mut idx = t_start.0 / step;
        loop {
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
                painter.text(
                    pos2(x + 3.0, ruler_r.top() + 1.0),
                    Align2::LEFT_TOP,
                    self.fmt_time(t),
                    FontId::monospace(10.0),
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
        for (i, m) in sorted.iter().enumerate() {
            let x = self.tl.t2x(m.t, left);
            let col = self.theme.tl_region;
            painter.rect_filled(
                Rect::from_min_size(pos2(x, marker_r.top()), vec2(14.0, MARKER_H - 2.0)),
                1.0,
                col,
            );
            painter.text(
                pos2(x + 3.0, marker_r.top() + 2.0),
                Align2::LEFT_TOP,
                format!("{}", (i + 1) % 10),
                FontId::proportional(11.0),
                crate::theme::contrast_on(col),
            );
            painter.text(
                pos2(x + 17.0, marker_r.top() + 2.0),
                Align2::LEFT_TOP,
                &m.label,
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
        resp.context_menu(|ui| match self.tl.ctx_target.clone() {
            CtxTarget::Marker(id) => {
                if ui.button("Renomear marcador").clicked() {
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
                if ui.button("Excluir marcador").clicked() {
                    self.checkpoint();
                    self.project.markers.retain(|m| m.id != id);
                    self.changed();
                    ui.close_menu();
                }
            }
            CtxTarget::Region(id) => {
                if ui.button("Renomear região").clicked() {
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
                if ui.button("Selecionar região").clicked() {
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
                if ui.button("Renderizar região...").clicked() {
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
                if ui.button("Excluir região").clicked() {
                    self.checkpoint();
                    self.project.regions.retain(|m| m.id != id);
                    self.changed();
                    ui.close_menu();
                }
            }
            _ => {
                if ui.button("Inserir marcador no cursor (M)").clicked() {
                    self.insert_marker();
                    ui.close_menu();
                }
                if ui.button("Inserir região da seleção (R)").clicked() {
                    self.insert_region();
                    ui.close_menu();
                }
                if ui.button("Limpar região de loop").clicked() {
                    self.project.loop_region = None;
                    self.time_sel = None;
                    ui.close_menu();
                }
            }
        });
    }

    fn rename_popup(&mut self, ui: &mut egui::Ui) {
        let Some((id, mut label)) = self.tl.rename.clone() else {
            return;
        };
        let mut open = true;
        let mut done = false;
        egui::Window::new("Renomear")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                let r = ui.text_edit_singleline(&mut label);
                r.request_focus();
                if ui.button("OK").clicked()
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
            self.changed();
            self.tl.rename = None;
        } else if !open {
            self.tl.rename = None;
        } else {
            self.tl.rename = Some((id, label));
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
        let n = self.project.tracks.len();
        for ti in 0..n {
            let (y0, h) = rows.track_rects[ti];
            let r = Rect::from_min_size(pos2(area.left(), y0), vec2(HEADER_W, h));
            if r.bottom() < area.top() || r.top() > area.bottom() {
                continue;
            }
            let selected = self.sel_track == Some(ti);
            let track = &self.project.tracks[ti];
            let bg = if selected {
                self.theme.tl_header_sel
            } else {
                self.theme.tl_header_idle
            };
            painter.rect_filled(r.shrink2(vec2(0.0, 0.5)), 0.0, bg);
            painter.rect_filled(
                Rect::from_min_size(r.min, vec2(6.0, h)),
                0.0,
                shade(track.color, 1.0),
            );
            painter.line_segment(
                [pos2(r.left(), r.bottom()), pos2(r.right(), r.bottom())],
                Stroke::new(1.0_f32, self.theme.border),
            );

            // clique no fundo seleciona a trilha
            let tid = track.id;
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

            let inner = Rect::from_min_max(
                pos2(r.left() + 10.0, r.top() + 3.0),
                pos2(r.right() - 14.0, r.bottom() - 3.0),
            );
            let mut child =
                ui.new_child(egui::UiBuilder::new().max_rect(inner).id_salt(("th", tid)));
            child.set_clip_rect(inner.intersect(area));
            let kind = self.project.tracks[ti].kind;
            let mut changed_track = false;
            let mut want_checkpoint = false;
            child.horizontal(|ui| {
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
                icons::paint_inline(
                    ui,
                    if kind == TrackKind::Video {
                        Icon::VideoTrack
                    } else {
                        Icon::AudioTrack
                    },
                    14.0,
                    &self.theme,
                );
                let is_adjustment = self.project.tracks[ti].is_adjustment();
                if is_adjustment {
                    ui.label(
                        egui::RichText::new("AJ")
                            .small()
                            .strong()
                            .color(self.theme.text_muted),
                    )
                    .on_hover_text(
                        "Trilha de ajuste (Adjustment Track): os efeitos desta trilha \
                         valem para todas as trilhas de vídeo abaixo dela",
                    );
                }
                let t = &mut self.project.tracks[ti];
                let r = ui.add(
                    egui::TextEdit::singleline(&mut t.name)
                        .desired_width(78.0)
                        .font(egui::TextStyle::Small),
                );
                if r.changed() {
                    changed_track = true;
                }
                let m = ui.selectable_label(t.mute, "M").on_hover_text("Mudo (Z)");
                if m.clicked() {
                    want_checkpoint = true;
                    t.mute = !t.mute;
                    changed_track = true;
                }
                let s = ui.selectable_label(t.solo, "S").on_hover_text("Solo (X)");
                if s.clicked() {
                    want_checkpoint = true;
                    t.solo = !t.solo;
                    changed_track = true;
                }
                let fxlabel = if t.fx.is_empty() { "fx" } else { "FX" };
                if ui
                    .small_button(fxlabel)
                    .on_hover_text("Efeitos da trilha")
                    .clicked()
                {
                    self.dlg.fx_target = Some(FxTarget::Track(tid));
                }
                if kind == TrackKind::Video
                    && !is_adjustment
                    && icons::small_icon_button(ui, Icon::TrackMotion, None, false, &self.theme)
                        .on_hover_text("Movimento da trilha (Track Motion)")
                        .clicked()
                {
                    self.dlg.track_motion = Some(tid);
                }
            });
            if h >= 50.0 {
                child.horizontal(|ui| {
                    let t = &mut self.project.tracks[ti];
                    if kind == TrackKind::Audio {
                        ui.label(egui::RichText::new("Vol").small());
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
                        changed_track |= r.changed() || r.double_clicked();
                    } else {
                        ui.label(egui::RichText::new("Nível").small());
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
                    }
                });
            }
            if h >= 72.0 {
                child.horizontal(|ui| {
                    if kind == TrackKind::Audio {
                        let t = &mut self.project.tracks[ti];
                        ui.label(egui::RichText::new("Pan").small());
                        let r = ui.add(egui::Slider::new(&mut t.pan, -1.0..=1.0).show_value(false));
                        if r.drag_started() {
                            want_checkpoint = true;
                        }
                        if r.double_clicked() {
                            t.pan = 0.0;
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
                                .unwrap_or("Master");
                            let before = t.output_bus;
                            egui::ComboBox::from_id_salt(("hdr_out", tid))
                                .width(64.0)
                                .selected_text(out_lbl)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut t.output_bus, None, "Master");
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
                        ui.label(egui::RichText::new("Modo").small());
                        let before = t.blend;
                        egui::ComboBox::from_id_salt(("blend", tid))
                            .width(120.0)
                            .selected_text(t.blend.name())
                            .show_ui(ui, |ui| {
                                for b in BlendMode::ALL {
                                    ui.selectable_value(&mut t.blend, b, b.name());
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
            // medidor de nível (áudio)
            if kind == TrackKind::Audio {
                let lv = self.meters_hold.get(&tid).cloned().unwrap_or([0.0; 2]);
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

        // Trilhas de barramentos auxiliares (Ctrl+B)
        for bi in 0..rows.bus_rects.len() {
            if bi >= self.project.audio_buses.len() {
                break;
            }
            let (y0, h) = rows.bus_rects[bi];
            let r = Rect::from_min_size(pos2(area.left(), y0), vec2(HEADER_W, h));
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
                let fxlabel = if b_fx_empty { "fx" } else { "FX" };
                if ui
                    .small_button(fxlabel)
                    .on_hover_text("Efeitos do barramento")
                    .clicked()
                {
                    self.dlg.fx_target = Some(FxTarget::AudioBus(bid));
                }
            });
            child.horizontal(|ui| {
                ui.label(egui::RichText::new("Vol").small());
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
        let kind = self.project.tracks[ti].kind;
        let tid = self.project.tracks[ti].id;
        if ui.button("Efeitos da trilha...").clicked() {
            self.dlg.fx_target = Some(FxTarget::Track(tid));
            ui.close_menu();
        }
        if kind == TrackKind::Video && ui.button("Movimento da trilha...").clicked() {
            self.dlg.track_motion = Some(tid);
            ui.close_menu();
        }
        ui.separator();
        let envs: &[EnvKind] = if kind == TrackKind::Audio {
            &[EnvKind::Volume, EnvKind::Pan]
        } else {
            &[EnvKind::Opacity, EnvKind::FadeToColor]
        };
        for &k in envs {
            let has = self.project.tracks[ti].envelope(k).is_some();
            let label = if has {
                format!("Remover envelope: {}", k.name())
            } else {
                format!("Inserir envelope: {}", k.name())
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
        if kind == TrackKind::Audio && !self.project.audio_buses.is_empty() {
            ui.menu_button("Roteamento de saída", |ui| {
                let cur_out = self.project.tracks[ti].output_bus;
                if ui.selectable_label(cur_out.is_none(), "Master").clicked() {
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
        if ui.button("Duplicar trilha").clicked() {
            self.duplicate_track(ti);
            ui.close_menu();
        }
        if ui.button("Mover para cima").clicked() {
            self.move_track(ti, true);
            ui.close_menu();
        }
        if ui.button("Mover para baixo").clicked() {
            self.move_track(ti, false);
            ui.close_menu();
        }
        ui.menu_button("Cor da trilha", |ui| {
            let mut c = self.project.tracks[ti].color;
            if ui.color_edit_button_srgb(&mut c).changed() {
                self.project.tracks[ti].color = c;
            }
        });
        ui.separator();
        if ui.button("Excluir trilha").clicked() {
            self.delete_track(ti);
            ui.close_menu();
        }
    }

    fn bus_menu(&mut self, ui: &mut egui::Ui, bi: usize) {
        let Some(bid) = self.project.audio_buses.get(bi).map(|b| b.id) else {
            return;
        };
        if ui.button("Efeitos do barramento...").clicked() {
            self.dlg.fx_target = Some(FxTarget::AudioBus(bid));
            ui.close_menu();
        }
        ui.separator();
        for &k in &[EnvKind::Volume, EnvKind::Pan] {
            let has = self.project.audio_buses[bi].envelope(k).is_some();
            let label = if has {
                format!("Remover envelope: {}", k.name())
            } else {
                format!("Inserir envelope: {}", k.name())
            };
            if ui.button(label).clicked() {
                if self.session.toggle_bus_envelope(bid, k) && !has {
                    self.opts.show_envelopes = true;
                }
                ui.close_menu();
            }
        }
        ui.separator();
        if ui.button("Excluir barramento").clicked() {
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
                        let t = self.tl.x2t(p.x, left);
                        self.tl.zoom_by((1.0 + d * 0.0025).clamp(0.5, 2.0), Some(t));
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
                    let t = self.tl.x2t(p.x, left);
                    self.tl.zoom_by((1.0 + d * 0.0025).clamp(0.5, 2.0), Some(t));
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
                    .track_at(p.y, layout)
                    .unwrap_or(self.project.tracks.len());
                let t = self.snap(self.tl.x2t(p.x, left), &HashSet::new(), mods.shift);
                self.tl.drop_hint = Some((ti, t));
                if ui.input(|i| i.pointer.any_released()) {
                    if let Some(payload) = egui::DragAndDrop::take_payload::<DndPayload>(ui.ctx()) {
                        let hit = self.hit_event(p, left, layout);
                        self.handle_drop(
                            (*payload).clone(),
                            self.track_at(p.y, layout),
                            t,
                            hit.map(|(ti, ei, _)| self.project.tracks[ti].events[ei].id),
                        );
                    }
                }
            }
        }

        let hover = pointer.filter(|p| lanes.contains(*p));
        // cursor do mouse conforme a zona
        if let (Some(p), Drag::None) = (hover, &self.tl.drag) {
            if let Some((_, _, Some(_), _)) = self.hit_envelope(p, left, rows) {
                ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            } else if let Some((_, _, z)) = self.hit_event(p, left, layout) {
                match z {
                    Zone::LeftEdge | Zone::RightEdge => {
                        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal)
                    }
                    Zone::FadeIn | Zone::FadeOut => {
                        ui.ctx().set_cursor_icon(CursorIcon::ResizeNeSw)
                    }
                    Zone::GainHandle => ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical),
                    Zone::FxButton | Zone::PanCropButton => {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand)
                    }
                    Zone::Body => {
                        if mods.alt {
                            ui.ctx().set_cursor_icon(CursorIcon::AllScroll)
                        } else {
                            ui.ctx().set_cursor_icon(CursorIcon::Grab)
                        }
                    }
                }
            } else if self.hit_envelope(p, left, rows).is_some() {
                ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            }
        }

        if resp.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui
                .input(|i| i.pointer.press_origin())
                .unwrap_or(lanes.center());
            self.start_lane_drag(origin, left, rows, mods);
        }
        if resp.dragged_by(egui::PointerButton::Primary) {
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
            if matches!(self.tl.drag, Drag::Move { .. } | Drag::Trim { .. }) {
                self.project.sort_events();
            }
            if !matches!(self.tl.drag, Drag::Pan { .. }) {
                self.tl.drag = Drag::None;
            }
            self.tl.snap_line = None;
        }

        if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                self.lane_click(p, left, rows, mods);
            }
        }
        if resp.double_clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                if let Some((owner, kind, None, t)) = self.hit_envelope(p, left, rows) {
                    if let Some((y0, h)) = rows.row_rect(owner) {
                        self.checkpoint();
                        let v = kind.value_from_norm((y0 + h - 4.0 - p.y) / (h - 8.0));
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
                            curve: Curve::Linear,
                        });
                        env.sort();
                        self.changed();
                    }
                } else if let Some((ti, ei, _)) = self.hit_event(p, left, layout) {
                    let e = &self.project.tracks[ti].events[ei];
                    let sel = (e.start, e.end());
                    self.time_sel = Some(sel);
                    self.project.loop_region = Some(sel);
                }
            }
        }
        if resp.secondary_clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                let t = self.tl.x2t(p.x, left);
                self.tl.ctx_target =
                    if let Some((owner, k, idx, t)) = self.hit_envelope(p, left, rows) {
                        match idx {
                            Some(i) => CtxTarget::EnvPoint(owner, k, i),
                            None => CtxTarget::EnvLine(owner, k, t),
                        }
                    } else if let Some((ti, ei, _)) = self.hit_event(p, left, layout) {
                        let id = self.project.tracks[ti].events[ei].id;
                        if !self.sel_events.contains(&id) {
                            self.sel_events.clear();
                            self.sel_events.insert(id);
                        }
                        self.sel_track = Some(ti);
                        CtxTarget::Event(id)
                    } else if let Some(ti) = self.track_at(p.y, layout) {
                        self.sel_track = Some(ti);
                        CtxTarget::Lane(ti, t)
                    } else {
                        CtxTarget::None
                    };
            }
        }
        resp.context_menu(|ui| self.lane_context_menu(ui));
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
        if let Some((owner, kind, Some(idx), _)) = self.hit_envelope(origin, left, rows) {
            self.checkpoint();
            self.tl.drag = Drag::EnvPoint { owner, kind, idx };
            return;
        }
        if let Some((ti, ei, zone)) = self.hit_event(origin, left, layout) {
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
                    self.checkpoint();
                    self.tl.drag = Drag::Gain {
                        id,
                        orig_db,
                        origin_y: origin.y,
                    };
                }
                Zone::LeftEdge | Zone::RightEdge => {
                    self.checkpoint();
                    if !self.sel_events.contains(&id) {
                        self.sel_events.clear();
                        self.sel_events.insert(id);
                    }
                    let e = &self.project.tracks[ti].events[ei];
                    let left_edge = zone == Zone::LeftEdge;
                    let edge_t = if left_edge { e.start } else { e.end() };
                    let mut ids: HashSet<Id> = HashSet::new();
                    ids.insert(id);
                    let group_ids = self.expand_groups(&ids);
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
                        slip: mods.alt,
                        origin_t: ot,
                        anchor_id: id,
                        orig,
                    };
                }
                Zone::FxButton | Zone::PanCropButton => {}
                Zone::Body => {
                    self.checkpoint();
                    if !self.sel_events.contains(&id) {
                        if !mods.command && !mods.shift {
                            self.sel_events.clear();
                        }
                        self.sel_events.insert(id);
                    }
                    let ids = self.expand_groups(&self.sel_events.clone());
                    if mods.alt {
                        let orig = ids
                            .iter()
                            .filter_map(|i| self.project.event(*i).map(|e| (*i, e.offset)))
                            .collect();
                        self.tl.drag = Drag::Slip { origin_t: ot, orig };
                        return;
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
                    let anchor_start = self.project.tracks[ti].events[ei].start;
                    self.tl.drag = Drag::Move {
                        anchor_id: anchor,
                        anchor_start,
                        origin_t: ot,
                        origin_track: ti,
                        orig,
                    };
                }
            }
            return;
        }
        // área vazia: seleção de tempo
        let s = self.snap(ot, &HashSet::new(), mods.shift);
        if let Some(ti) = self.track_at(origin.y, layout) {
            self.sel_track = Some(ti);
        }
        self.tl.drag = Drag::TimeSel { anchor: s };
    }

    fn update_lane_drag(&mut self, p: Pos2, left: f32, rows: &TimelineRows, mods: egui::Modifiers) {
        let layout = &rows.track_rects;
        let t = self.tl.x2t(p.x, left);
        match self.tl.drag.clone() {
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
            } => {
                let dy = p.y - origin_y;
                let raw = (orig_db - dy * 0.35).clamp(-36.0, 12.0);
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
            Drag::Slip { origin_t, orig } => {
                let dt = t - origin_t;
                for (id, off) in orig {
                    let dur = self
                        .project
                        .event(id)
                        .and_then(|e| self.project.media(e.media_id))
                        .and_then(|m| m.duration);
                    if let Some(e) = self.project.event_mut(id) {
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
                for &(id, st, len, off) in &orig {
                    if let Some(e) = self.project.event_mut(id) {
                        if is_left {
                            e.start = st + dt;
                            e.length = (len - dt).max(fd);
                            if !slip && !e.reverse {
                                e.offset = off + e.rate.scale_ticks(dt);
                            }
                        } else {
                            e.length = (len + dt).max(fd);
                            if slip {
                                e.offset = (off - e.rate.scale_ticks(dt)).max(Ticks::ZERO);
                            }
                        }
                        e.fade_in.len = e.fade_in.len.min(e.length);
                        e.fade_out.len = e.fade_out.len.min(e.length - e.fade_in.len);
                    }
                }
                self.changed();
            }
            Drag::Move {
                anchor_id,
                anchor_start,
                origin_t,
                origin_track,
                orig,
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
                let target = self.track_at(p.y, layout).unwrap_or(origin_track);
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
                self.changed();
            }
            _ => {}
        }
    }

    fn lane_click(&mut self, p: Pos2, left: f32, rows: &TimelineRows, mods: egui::Modifiers) {
        let layout = &rows.track_rects;
        let t = self.tl.x2t(p.x, left);
        if let Some(ti) = self.track_at(p.y, layout) {
            self.sel_track = Some(ti);
        }
        if let Some((ti, ei, zone)) = self.hit_event(p, left, layout) {
            let id = self.project.tracks[ti].events[ei].id;
            match zone {
                Zone::FxButton => {
                    self.dlg.fx_target = Some(FxTarget::Event(id));
                    return;
                }
                Zone::PanCropButton => {
                    self.dlg.pan_crop = Some(id);
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
                self.checkpoint();
                self.add_media_to_timeline(mid, t, track);
            }
            DndPayload::Generator(spec) => {
                let imp = crate::media::import_generated(spec);
                let id = imp.item.id;
                self.media.insert(id, imp.entry);
                self.checkpoint();
                self.project.media.push(imp.item);
                self.add_media_to_timeline(id, t, track);
            }
            DndPayload::Fx(kind) => {
                let want_audio = kind.is_audio();
                if let Some(eid) = event {
                    let Some((ti, _)) = self.project.find_event(eid) else {
                        return;
                    };
                    if (self.project.tracks[ti].kind == TrackKind::Audio) == want_audio {
                        self.checkpoint();
                        if let Some(ev) = self.project.event_mut(eid) {
                            ev.fx.push(FxInstance::new(kind));
                        }
                        self.dlg.fx_target = Some(FxTarget::Event(eid));
                        self.changed();
                        return;
                    }
                }
                if let Some(ti) = track {
                    if (self.project.tracks[ti].kind == TrackKind::Audio) == want_audio {
                        self.checkpoint();
                        self.project.tracks[ti].fx.push(FxInstance::new(kind));
                        self.dlg.fx_target = Some(FxTarget::Track(self.project.tracks[ti].id));
                        self.changed();
                        return;
                    }
                }
                self.set_status(
                    "Solte efeitos de vídeo em eventos/trilhas de vídeo e de áudio em áudio",
                );
            }
            DndPayload::Transition(kind) => {
                if let Some(eid) = event {
                    self.apply_transition(eid, kind);
                }
            }
        }
    }

    pub fn apply_transition(&mut self, eid: Id, kind: TransitionKind) {
        let Some((ti, _)) = self.project.find_event(eid) else {
            return;
        };
        if self.project.tracks[ti].kind != TrackKind::Video {
            self.set_status("Transições se aplicam a eventos de vídeo");
            return;
        }
        self.checkpoint();
        let Some(e) = self.project.event_mut(eid) else {
            return;
        };
        let soft = e.transition.as_ref().map(|t| t.softness).unwrap_or(0.05);
        e.transition = Some(TransitionSpec {
            kind,
            softness: soft,
        });
        if e.fade_in.len <= Ticks::ZERO {
            e.fade_in.len = Ticks::from_secs_i64(1).min(e.length.div_i64(2));
        }
        self.set_status(format!("Transição \"{}\" aplicada", kind.name()));
        self.changed();
    }

    fn lane_context_menu(&mut self, ui: &mut egui::Ui) {
        match self.tl.ctx_target.clone() {
            CtxTarget::Event(id) => self.event_context_menu(ui, id),
            CtxTarget::EnvPoint(owner, kind, idx) => {
                ui.label(
                    egui::RichText::new(format!("Ponto de envelope: {}", kind.name())).strong(),
                );
                if let Some(env) = self
                    .owner_envelopes(owner)
                    .and_then(|list| list.iter().find(|e| e.kind == kind))
                {
                    if let Some(p) = env.points.get(idx) {
                        ui.label(format!("{}  @ {}", kind.format(p.v), self.fmt_time(p.t)));
                    }
                }
                ui.separator();
                for c in Curve::ALL {
                    if ui.button(format!("Curva: {}", c.name())).clicked() {
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
                ui.separator();
                if ui.button("Redefinir valor").clicked() {
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
                if ui.button("Excluir ponto").clicked() {
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
                if ui.button("Adicionar ponto aqui").clicked() {
                    self.checkpoint();
                    if let Some(env) = self
                        .owner_envelopes_mut(owner)
                        .and_then(|list| list.iter_mut().find(|e| e.kind == kind))
                    {
                        let v = env.eval(t);
                        env.points.push(EnvPoint {
                            t,
                            v,
                            curve: Curve::Linear,
                        });
                        env.sort();
                    }
                    self.changed();
                    ui.close_menu();
                }
                if ui.button("Redefinir todos os pontos").clicked() {
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
                if ui.button("Ocultar envelope").clicked() {
                    if let Some(env) = self
                        .owner_envelopes_mut(owner)
                        .and_then(|list| list.iter_mut().find(|e| e.kind == kind))
                    {
                        env.visible = false;
                    }
                    ui.close_menu();
                }
                if ui.button("Remover envelope").clicked() {
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
                    .add_enabled(!self.clipboard.is_empty(), egui::Button::new("Colar aqui"))
                    .clicked()
                {
                    self.cursor = t;
                    self.sel_track = Some(ti);
                    self.paste(false);
                    ui.close_menu();
                }
                if ui.button("Inserir marcador aqui").clicked() {
                    self.cursor = t;
                    self.insert_marker();
                    ui.close_menu();
                }
                if ui.button("Inserir texto aqui").clicked() {
                    self.cursor = t;
                    self.sel_track = Some(ti);
                    self.insert_generated(GenSpec::Text(TextSpec::default()));
                    ui.close_menu();
                }
                ui.separator();
                self.track_menu(ui, ti);
            }
            _ => {
                ui.label("—");
            }
        }
    }

    fn event_context_menu(&mut self, ui: &mut egui::Ui, id: Id) {
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
        if ui.button("Propriedades...").clicked() {
            self.dlg.event_props = Some(id);
            ui.close_menu();
        }
        if is_video && ui.button("Pan/Crop do evento...").clicked() {
            self.dlg.pan_crop = Some(id);
            ui.close_menu();
        }
        if ui.button("Efeitos do evento...").clicked() {
            self.dlg.fx_target = Some(FxTarget::Event(id));
            ui.close_menu();
        }
        if media_generated && ui.button("Editar mídia gerada...").clicked() {
            self.dlg.gen_editor = Some(e.media_id);
            ui.close_menu();
        }
        if ui.button("Renomear (F2)").clicked() {
            self.tl.rename = Some((id, e.name.clone()));
            ui.close_menu();
        }
        ui.separator();
        if ui.button("Dividir no cursor (S)").clicked() {
            self.split_at_cursor();
            ui.close_menu();
        }
        if ui.button("Copiar").clicked() {
            self.copy_selection();
            ui.close_menu();
        }
        if ui.button("Recortar").clicked() {
            self.cut_selection();
            ui.close_menu();
        }
        if ui.button("Excluir").clicked() {
            self.delete_selection();
            ui.close_menu();
        }
        ui.separator();
        let mut flags = (e.mute, e.locked, e.looped, e.reverse, e.maintain_aspect);
        let before = flags;
        ui.checkbox(&mut flags.0, "Mudo");
        ui.checkbox(&mut flags.1, "Travado");
        ui.checkbox(&mut flags.2, "Loop");
        ui.checkbox(&mut flags.3, "Inverter (reverse)");
        if is_video {
            ui.checkbox(&mut flags.4, "Manter proporção");
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
        ui.menu_button("Curva do fade de entrada", |ui| {
            for c in Curve::ALL.iter().take(5) {
                if ui
                    .selectable_label(e.fade_in.curve == *c, c.name())
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
        ui.menu_button("Curva do fade de saída", |ui| {
            for c in Curve::ALL.iter().take(5) {
                if ui
                    .selectable_label(e.fade_out.curve == *c, c.name())
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
            ui.menu_button("Transição de entrada", |ui| {
                if ui
                    .selectable_label(e.transition.is_none(), "Nenhuma (crossfade)")
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
                    if ui.selectable_label(sel, k.name()).clicked() {
                        self.apply_transition(id, k);
                        ui.close_menu();
                    }
                }
            });
        }
        ui.menu_button("Velocidade", |ui| {
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
        if ui.button("Agrupar seleção (G)").clicked() {
            self.group_selected();
            ui.close_menu();
        }
        if e.group.is_some() && ui.button("Desagrupar (U)").clicked() {
            self.ungroup_selected();
            ui.close_menu();
        }
        if e.group.is_some() && ui.button("Selecionar grupo (Shift+G)").clicked() {
            self.select_group();
            ui.close_menu();
        }
        if is_video && ui.button("Congelar quadro no cursor").clicked() {
            self.freeze_frame();
            ui.close_menu();
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
            let col = match env.kind {
                EnvKind::Volume | EnvKind::Opacity => self.theme.tl_env_primary,
                EnvKind::Pan | EnvKind::FadeToColor => self.theme.tl_env_secondary,
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
        let (major, _) = nice_step(pps, self.project.settings.fps);
        let step = major.0.max(1);
        let t_end = self.tl.x2t(lanes.right(), left);
        let mut idx = self.tl.scroll_x.0 / step;
        loop {
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
        for ti in 0..self.project.tracks.len() {
            let (y0, h) = layout[ti];
            if y0 + h < lanes.top() || y0 > lanes.bottom() {
                continue;
            }
            let lane = Rect::from_min_max(pos2(lanes.left(), y0), pos2(lanes.right(), y0 + h));
            let selected = self.sel_track == Some(ti);
            painter.rect_filled(
                lane,
                0.0,
                if selected {
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
            // sobreposições (crossfades)
            for (i, a) in track.events.iter().enumerate() {
                for b in track.events.iter().skip(i + 1) {
                    let (s, e) = (a.start.max(b.start), a.end().min(b.end()));
                    if e > s {
                        let r = Rect::from_min_max(
                            pos2(self.tl.t2x(s, left), y0 + 1.0),
                            pos2(self.tl.t2x(e, left), y0 + h - 1.0),
                        );
                        painter.rect_filled(r, 0.0, self.theme.text_primary.gamma_multiply(0.08));
                        painter.line_segment(
                            [r.left_top(), r.right_bottom()],
                            Stroke::new(1.0_f32, self.theme.border),
                        );
                        painter.line_segment(
                            [r.left_bottom(), r.right_top()],
                            Stroke::new(1.0_f32, self.theme.border),
                        );
                    }
                }
            }
            for e in &track.events {
                let r = self.event_rect(e, left, y0, h);
                if r.right() < lanes.left() || r.left() > lanes.right() {
                    continue;
                }
                let sel = self.sel_events.contains(&e.id);
                let k = if sel { 1.0 } else { 0.62 };
                let body = if e.mute || muted {
                    shade([90, 90, 90], if self.theme.dark_mode { 0.7 } else { 1.45 })
                } else {
                    shade(track.color, k * 0.55)
                };
                let hdr_h = 14.0f32.min(r.height() * 0.3);
                painter.rect_filled(r, 2.0, body);
                let hdr_col = if e.mute || muted {
                    if self.theme.dark_mode {
                        Color32::from_gray(80)
                    } else {
                        Color32::from_gray(176)
                    }
                } else {
                    shade(track.color, k * 0.9)
                };
                let hdr = Rect::from_min_max(r.min, pos2(r.right(), r.top() + hdr_h));
                painter.rect_filled(hdr, 2.0, hdr_col);
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
                        let mid = content.center().y;
                        let amp = content.height() * 0.48 * db_to_lin(e.gain_db).min(4.0);
                        let alpha = if e.mute { 70 } else { 185 };
                        let col = if body_light {
                            Color32::from_rgba_unmultiplied(22, 30, 42, alpha)
                        } else {
                            Color32::from_rgba_unmultiplied(220, 230, 240, alpha)
                        };
                        let mut x = vis.left().floor();
                        let mut segs = Vec::with_capacity(vis.width() as usize * 2);
                        let min_span = Ticks::from_raw(TICKS_PER_MS / 10);
                        let last_t = (e.end() - Ticks::from_raw(1)).max(e.start);
                        while x < vis.right() {
                            let t0 = self.tl.x2t(x, left);
                            let t1 = self.tl.x2t(x + 1.0, left);
                            if let (Some(m0), Some(m1)) =
                                (e.media_time(t0, dur), e.media_time(t1.min(last_t), dur))
                            {
                                let (m0, m1) = if m0 <= m1 { (m0, m1) } else { (m1, m0) };
                                let (lo, hi) = a.peak_range(m0, m1.max(m0 + min_span));
                                let fg = e.fade_gain(t0);
                                segs.push(Shape::line_segment(
                                    [
                                        pos2(x, mid - hi * amp * fg),
                                        pos2(x, mid - lo * amp * fg + 0.5),
                                    ],
                                    Stroke::new(1.0_f32, col),
                                ));
                            }
                            x += 1.0;
                        }
                        painter.with_clip_rect(vis).extend(segs);
                    }
                }
                // nome e indicadores
                let mut label = e.name.clone();
                if !e.rate.is_normal() {
                    label.push_str(&format!("  [{:.0}%]", e.rate.percent_f64()));
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
                if let Some(tr) = &e.transition {
                    label = format!("[{}] {}", tr.kind.name(), label);
                }
                painter.with_clip_rect(hdr.intersect(lanes)).text(
                    pos2(r.left().max(lanes.left()) + 4.0, hdr.top() + 1.0),
                    Align2::LEFT_TOP,
                    label,
                    FontId::proportional(11.0),
                    hdr_fg,
                );
                // botões do evento
                let (fxb, pcb) = Self::event_buttons(&r, track.kind == TrackKind::Video);
                let btn_bg = if body_light {
                    Color32::from_rgba_unmultiplied(255, 255, 255, 170)
                } else {
                    Color32::from_rgba_unmultiplied(18, 22, 28, 175)
                };
                let btn_muted = crate::theme::contrast_on(btn_bg).gamma_multiply(0.78);
                if let Some(b) = fxb {
                    let col = if e.fx.is_empty() {
                        btn_muted
                    } else if self.theme.high_contrast {
                        self.theme.accent
                    } else if body_light {
                        Color32::from_rgb(16, 130, 56)
                    } else {
                        Color32::from_rgb(120, 230, 120)
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
                    } else if body_light {
                        Color32::from_rgb(16, 102, 198)
                    } else {
                        Color32::from_rgb(120, 200, 255)
                    };
                    painter.rect_filled(b, 2.0, btn_bg);
                    painter.rect_stroke(b, 2.0, Stroke::new(1.0_f32, col), StrokeKind::Inside);
                    icons::paint(
                        &painter,
                        b.shrink(1.5),
                        Icon::PanCrop,
                        &ev_icon_theme,
                        active,
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
                draw_fade(e.fade_in.len, e.fade_in.curve, true);
                draw_fade(e.fade_out.len, e.fade_out.curve, false);
                // alças de fade
                let fi_x = r.left() + e.fade_in.len.as_secs_f64() as f32 * pps;
                let fo_x = r.right() - e.fade_out.len.as_secs_f64() as f32 * pps;
                for x in [fi_x, fo_x] {
                    painter.rect_filled(
                        Rect::from_center_size(pos2(x, r.top() + 3.0), vec2(6.0, 6.0)),
                        1.0,
                        self.theme.tl_fade,
                    );
                }
                // alça de ganho no topo do evento de áudio
                if track.kind == TrackKind::Audio && r.width() > 24.0 && r.height() >= 18.0 {
                    let gy = Self::gain_handle_y(r, e.gain_db);
                    let x0 = (fi_x + 4.0).min(r.right() - 4.0);
                    let x1 = (fo_x - 4.0).max(x0);
                    painter.line_segment(
                        [pos2(x0, gy), pos2(x1, gy)],
                        Stroke::new(1.0_f32, self.theme.tl_gain_line),
                    );
                    let dragging_gain = matches!(self.tl.drag, Drag::Gain { id, .. } if id == e.id);
                    if dragging_gain || e.gain_db.abs() > 0.05 {
                        painter.rect_filled(
                            Rect::from_center_size(pos2((x0 + x1) * 0.5, gy), vec2(5.0, 5.0)),
                            1.0,
                            self.theme.tl_gain_line,
                        );
                    }
                    if dragging_gain {
                        let g = painter.layout_no_wrap(
                            format!("{:+.1} dB", e.gain_db),
                            FontId::monospace(10.0),
                            self.theme.text_primary,
                        );
                        let bg = Rect::from_center_size(
                            pos2((x0 + x1) * 0.5, (gy - 12.0).max(r.top() + 8.0)),
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
                let border = if sel {
                    let sel_col = if self.opts.auto_ripple {
                        self.theme.offline_fg
                    } else {
                        self.theme.tl_event_sel_border
                    };
                    Stroke::new(2.0_f32, sel_col)
                } else {
                    Stroke::new(1.0_f32, self.theme.tl_event_border)
                };
                painter.rect_stroke(r, 2.0, border, StrokeKind::Inside);
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
            // envelopes
            self.draw_row_envelopes(&painter, lanes, left, y0, h, &track.envelopes);
        }
        // trilhas de barramentos auxiliares (Ctrl+B)
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
                Stroke::new(1.0_f32, self.theme.tl_playhead_edit),
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

pub fn draw_meter(painter: &egui::Painter, r: Rect, lv: [f32; 2]) {
    painter.rect_filled(r, 1.0, Color32::from_gray(20));
    let w = (r.width() - 1.0) / 2.0;
    for (i, v) in lv.iter().enumerate() {
        let db = lin_to_db(*v).clamp(-60.0, 6.0);
        let n = ((db + 60.0) / 66.0).clamp(0.0, 1.0);
        let col = if db > -0.1 {
            Color32::from_rgb(240, 60, 50)
        } else if db > -9.0 {
            Color32::from_rgb(230, 210, 60)
        } else {
            Color32::from_rgb(70, 200, 90)
        };
        let x0 = r.left() + i as f32 * (w + 1.0);
        let bar = Rect::from_min_max(
            pos2(x0, r.bottom() - n * r.height()),
            pos2(x0 + w, r.bottom()),
        );
        painter.rect_filled(bar, 0.0, col);
    }
}
