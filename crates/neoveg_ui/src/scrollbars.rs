//! Barras de rolagem e botões direcionais clássicos do estilo VEGAS Pro
//! (`◀` `▶` `▲` `▼`, faixa deslizante, passo por página, zoom `−`/`+` e
//! invólucro para painéis acoplados com `ScrollArea`).

use crate::theme::ThemeColors;
use eframe::egui;
use egui::{pos2, vec2, Color32, CornerRadius, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

/// Largura padrão da barra de rolagem vertical da timeline (px).
pub const TIMELINE_V_SCROLL_W: f32 = 16.0;
/// Largura padrão das barras de rolagem de painéis acoplados (px).
pub const DOCK_SCROLL_W: f32 = 14.0;
/// Altura padrão da barra de rolagem horizontal das abas do dock (px).
pub const TAB_SCROLL_BAR_H: f32 = 11.0;

const HOLD_INITIAL_DELAY_SECS: f64 = 0.35;
const HOLD_REPEAT_INTERVAL_SECS: f64 = 0.055;
const MIN_THUMB_LEN: f32 = 18.0;

/// Orientação de uma barra de rolagem linear.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// Direção do triângulo desenhado em um botão direcional da barra de rolagem.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArrowDir {
    Left,
    Right,
    Up,
    Down,
}

/// Símbolo vetorial para botões de escala/zoom adjacentes à barra de rolagem.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SymbolGlyph {
    Minus,
    Plus,
    Fit,
}

/// Intervalo normalizado de rolagem ao longo de um eixo (`f32` finito).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarRange {
    pub offset: f32,
    pub content: f32,
    pub viewport: f32,
}

impl ScrollbarRange {
    /// Deslocamento máximo permitido (`max(content - viewport, 0)`).
    pub fn max_offset(self) -> f32 {
        if !self.content.is_finite() || !self.viewport.is_finite() || self.viewport <= 0.0 {
            return 0.0;
        }
        (self.content - self.viewport).max(0.0)
    }

    /// Deslocamento atual higienizado para o intervalo `[0, max_offset]`.
    pub fn sanitized_offset(self) -> f32 {
        let max = self.max_offset();
        if !self.offset.is_finite() {
            return 0.0;
        }
        self.offset.clamp(0.0, max)
    }

    /// Retorna `true` quando há conteúdo além da janela visível.
    pub fn is_scrollable(self) -> bool {
        self.max_offset() > 0.5
    }

    /// Calcula `(inicio_relativo, comprimento)` do indicador móvel (thumb) ao longo da trilha.
    pub fn thumb_span(self, track_len: f32, min_thumb: f32) -> (f32, f32) {
        if !track_len.is_finite() || track_len <= 0.0 {
            return (0.0, 0.0);
        }
        let min_t = if min_thumb.is_finite() {
            min_thumb.clamp(4.0, track_len.max(4.0)).min(track_len)
        } else {
            track_len.min(MIN_THUMB_LEN)
        };
        let max_off = self.max_offset();
        if max_off <= 0.5 || !self.content.is_finite() || self.content <= 0.0 {
            return (0.0, track_len);
        }
        let ratio = (self.viewport / self.content).clamp(0.0, 1.0);
        let thumb_len = (track_len * ratio).clamp(min_t, track_len);
        let travel = (track_len - thumb_len).max(0.0);
        let frac = (self.sanitized_offset() / max_off).clamp(0.0, 1.0);
        (travel * frac, thumb_len)
    }

    /// Converte uma posição relativa do início do indicador na trilha em deslocamento real.
    pub fn offset_from_thumb_pos(self, track_len: f32, thumb_len: f32, thumb_start: f32) -> f32 {
        let max_off = self.max_offset();
        if max_off <= 0.0 || !track_len.is_finite() || !thumb_len.is_finite() {
            return 0.0;
        }
        let travel = (track_len - thumb_len).max(0.0);
        if travel <= 0.001 || !thumb_start.is_finite() {
            return 0.0;
        }
        let frac = (thumb_start / travel).clamp(0.0, 1.0);
        frac * max_off
    }
}

/// Resultado da interação com uma trilha de barra de rolagem.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScrollbarOutcome {
    pub new_offset: Option<f32>,
    pub double_clicked: bool,
}

/// Resultado de interação em um par completo `[◀/▲] [trilha] [▶/▼]`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ArrowScrollbarOutcome {
    pub new_offset: Option<f32>,
    pub double_clicked: bool,
}

#[derive(Clone, Copy, Default)]
struct HoldRepeatState {
    active: bool,
    fired_on_press: bool,
    next_fire: f64,
}

#[derive(Clone, Copy, Default)]
struct ThumbDragAnchor {
    active: bool,
    pointer_axis: f32,
    start_thumb_pos: f32,
}

#[derive(Clone, Copy, Default)]
struct DragZoomState {
    dragging: bool,
    moved_beyond_click: bool,
}

fn hold_repeat_step(ui: &egui::Ui, id: Id, resp: &egui::Response, enabled: bool) -> bool {
    let mut state = ui
        .ctx()
        .data_mut(|d| d.get_temp::<HoldRepeatState>(id).unwrap_or_default());
    if !enabled {
        state = HoldRepeatState::default();
        ui.ctx().data_mut(|d| d.insert_temp(id, state));
        return false;
    }

    let now = ui.input(|i| i.time);
    let mut fired = false;

    if resp.is_pointer_button_down_on() {
        if !state.active {
            state.active = true;
            state.fired_on_press = true;
            state.next_fire = now + HOLD_INITIAL_DELAY_SECS;
            fired = true;
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(HOLD_INITIAL_DELAY_SECS));
        } else if now >= state.next_fire {
            state.next_fire = now + HOLD_REPEAT_INTERVAL_SECS;
            fired = true;
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(
                    HOLD_REPEAT_INTERVAL_SECS,
                ));
        } else {
            let wait = (state.next_fire - now).max(0.016);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
    } else {
        if !state.fired_on_press && resp.clicked() {
            fired = true;
        }
        state.active = false;
        state.fired_on_press = false;
    }

    ui.ctx().data_mut(|d| d.insert_temp(id, state));
    fired
}

fn axis_coord(pos: Pos2, axis: Axis) -> f32 {
    match axis {
        Axis::Horizontal => pos.x,
        Axis::Vertical => pos.y,
    }
}

fn axis_delta(delta: Vec2, axis: Axis) -> f32 {
    match axis {
        Axis::Horizontal => delta.x,
        Axis::Vertical => -delta.y,
    }
}

fn axis_len(rect: Rect, axis: Axis) -> f32 {
    match axis {
        Axis::Horizontal => rect.width(),
        Axis::Vertical => rect.height(),
    }
}

fn button_chrome(
    ui: &egui::Ui,
    rect: Rect,
    resp: &egui::Response,
    enabled: bool,
    theme: &ThemeColors,
) -> Color32 {
    let pressed = enabled && (resp.is_pointer_button_down_on() || resp.dragged());
    let hovered = enabled && resp.hovered();

    let bg = if !enabled {
        theme.panel_fill
    } else if pressed {
        theme.selection_bg
    } else if hovered {
        theme.widget_hover_bg
    } else {
        theme.widget_bg
    };

    let border_col = if theme.high_contrast && hovered {
        theme.accent
    } else {
        theme.border
    };

    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(1), bg);
    p.rect_stroke(
        rect,
        CornerRadius::same(1),
        Stroke::new(1.0_f32, border_col),
        StrokeKind::Inside,
    );

    if !enabled {
        if theme.high_contrast {
            theme.text_muted
        } else {
            theme.text_muted.gamma_multiply(0.45)
        }
    } else if pressed && !theme.dark_mode && !theme.high_contrast {
        Color32::WHITE
    } else {
        theme.text_primary
    }
}

/// Desenha seta vetorial centralizada dentro de `rect`.
pub fn paint_arrow_glyph(painter: &egui::Painter, rect: Rect, dir: ArrowDir, color: Color32) {
    let c = rect.center();
    let rx = (rect.width() * 0.25).clamp(2.2, 4.5);
    let ry = (rect.height() * 0.25).clamp(2.2, 4.5);
    let r = rx.min(ry);
    let pts = match dir {
        ArrowDir::Left => vec![
            pos2(c.x - r * 0.75, c.y),
            pos2(c.x + r * 0.65, c.y - r * 0.95),
            pos2(c.x + r * 0.65, c.y + r * 0.95),
        ],
        ArrowDir::Right => vec![
            pos2(c.x + r * 0.75, c.y),
            pos2(c.x - r * 0.65, c.y - r * 0.95),
            pos2(c.x - r * 0.65, c.y + r * 0.95),
        ],
        ArrowDir::Up => vec![
            pos2(c.x, c.y - r * 0.75),
            pos2(c.x - r * 0.95, c.y + r * 0.65),
            pos2(c.x + r * 0.95, c.y + r * 0.65),
        ],
        ArrowDir::Down => vec![
            pos2(c.x, c.y + r * 0.75),
            pos2(c.x - r * 0.95, c.y - r * 0.65),
            pos2(c.x + r * 0.95, c.y - r * 0.65),
        ],
    };
    painter.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
}

/// Desenha símbolo vetorial (`−`, `+` ou quadrado de ajuste) dentro de `rect`.
pub fn paint_symbol_glyph(painter: &egui::Painter, rect: Rect, glyph: SymbolGlyph, color: Color32) {
    let c = rect.center();
    let arm = (rect.width().min(rect.height()) * 0.26).clamp(2.5, 4.5);
    let stroke = Stroke::new(1.5_f32, color);
    match glyph {
        SymbolGlyph::Minus => {
            painter.line_segment([pos2(c.x - arm, c.y), pos2(c.x + arm, c.y)], stroke);
        }
        SymbolGlyph::Plus => {
            painter.line_segment([pos2(c.x - arm, c.y), pos2(c.x + arm, c.y)], stroke);
            painter.line_segment([pos2(c.x, c.y - arm), pos2(c.x, c.y + arm)], stroke);
        }
        SymbolGlyph::Fit => {
            let box_r = Rect::from_center_size(c, vec2(arm * 1.9, arm * 1.5));
            painter.rect_stroke(
                box_r,
                CornerRadius::ZERO,
                Stroke::new(1.1_f32, color),
                StrokeKind::Inside,
            );
            painter.line_segment(
                [
                    pos2(box_r.left() + 1.5, c.y),
                    pos2(box_r.right() - 1.5, c.y),
                ],
                Stroke::new(1.1_f32, color),
            );
        }
    }
}

/// Botão direcional (`◀`/`▶`/`▲`/`▼`) com clique imediato e repetição contínua ao segurar.
#[allow(clippy::too_many_arguments)]
pub fn arrow_button(
    ui: &mut egui::Ui,
    id: Id,
    rect: Rect,
    dir: ArrowDir,
    enabled: bool,
    label: &str,
    tooltip: &str,
    theme: &ThemeColors,
) -> bool {
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return false;
    }
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let fg = button_chrome(ui, rect, &resp, enabled, theme);
    paint_arrow_glyph(ui.painter(), rect, dir, fg);
    let fired = hold_repeat_step(ui, id.with("hold"), &resp, enabled);
    if !tooltip.is_empty() {
        resp.on_hover_text(tooltip);
    }
    fired
}

/// Botão de zoom/escala (`−`, `+` ou ajuste) com clique contínuo ao segurar
/// e retorno opcional de arrasto suave ao deslizar o ponteiro.
#[allow(clippy::too_many_arguments)]
pub fn zoom_button(
    ui: &mut egui::Ui,
    id: Id,
    rect: Rect,
    glyph: SymbolGlyph,
    axis: Axis,
    enabled: bool,
    label: &str,
    tooltip: &str,
    theme: &ThemeColors,
) -> (bool, f32) {
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return (false, 0.0);
    }
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let fg = button_chrome(ui, rect, &resp, enabled, theme);
    paint_symbol_glyph(ui.painter(), rect, glyph, fg);

    let drag_id = id.with("zoom_drag");
    let mut dz = ui
        .ctx()
        .data_mut(|d| d.get_temp::<DragZoomState>(drag_id).unwrap_or_default());
    let mut drag_step = 0.0_f32;

    if enabled && resp.drag_started() {
        dz.dragging = true;
        dz.moved_beyond_click = false;
    }
    if enabled && dz.dragging && resp.dragged() {
        let delta = axis_delta(resp.drag_delta(), axis);
        if delta.abs() >= 1.5 || dz.moved_beyond_click {
            dz.moved_beyond_click = true;
            drag_step = delta;
        }
    }
    if !resp.dragged() && !resp.is_pointer_button_down_on() {
        dz.dragging = false;
        dz.moved_beyond_click = false;
    }
    ui.ctx().data_mut(|d| d.insert_temp(drag_id, dz));

    let stepped = if dz.moved_beyond_click {
        false
    } else {
        hold_repeat_step(ui, id.with("hold"), &resp, enabled)
    };

    if !tooltip.is_empty() {
        resp.on_hover_text(tooltip);
    }
    (stepped, drag_step)
}

/// Desenha e processa uma trilha de barra de rolagem com indicador deslizante,
/// salto por página ao clicar na trilha e detecção de duplo clique.
pub fn scrollbar_track(
    ui: &mut egui::Ui,
    id: Id,
    track_rect: Rect,
    axis: Axis,
    range: ScrollbarRange,
    theme: &ThemeColors,
) -> ScrollbarOutcome {
    let mut out = ScrollbarOutcome::default();
    if track_rect.width() <= 2.0 || track_rect.height() <= 2.0 {
        return out;
    }

    let track_len = axis_len(track_rect, axis);
    let scrollable = range.is_scrollable();
    let cur_offset = range.sanitized_offset();
    let (thumb_start, thumb_len) = range.thumb_span(track_len, MIN_THUMB_LEN);

    let thumb_rect = if scrollable {
        match axis {
            Axis::Horizontal => Rect::from_min_max(
                pos2(track_rect.left() + thumb_start, track_rect.top() + 1.0),
                pos2(
                    track_rect.left() + thumb_start + thumb_len,
                    track_rect.bottom() - 1.0,
                ),
            ),
            Axis::Vertical => Rect::from_min_max(
                pos2(track_rect.left() + 1.0, track_rect.top() + thumb_start),
                pos2(
                    track_rect.right() - 1.0,
                    track_rect.top() + thumb_start + thumb_len,
                ),
            ),
        }
    } else {
        track_rect.shrink(1.0)
    };

    let resp = ui.interact(track_rect, id, Sense::click_and_drag());
    let anchor_id = id.with("thumb_anchor");
    let mut anchor = ui
        .ctx()
        .data_mut(|d| d.get_temp::<ThumbDragAnchor>(anchor_id).unwrap_or_default());

    if resp.double_clicked() {
        out.double_clicked = true;
    }

    if scrollable {
        let max_off = range.max_offset();

        if resp.drag_started() {
            if let Some(ptr) = ui.ctx().pointer_interact_pos() {
                if thumb_rect.expand(2.0).contains(ptr) {
                    anchor = ThumbDragAnchor {
                        active: true,
                        pointer_axis: axis_coord(ptr, axis),
                        start_thumb_pos: thumb_start,
                    };
                }
            }
        }

        if anchor.active && resp.dragged() {
            if let Some(ptr) = ui.ctx().pointer_interact_pos() {
                let delta_px = axis_coord(ptr, axis) - anchor.pointer_axis;
                out.new_offset = Some(range.offset_from_thumb_pos(
                    track_len,
                    thumb_len,
                    anchor.start_thumb_pos + delta_px,
                ));
            }
        } else if !resp.dragged() {
            anchor.active = false;
        }

        if resp.clicked() && !anchor.active && !out.double_clicked {
            if let Some(ptr) = resp.interact_pointer_pos() {
                let page = (range.viewport * 0.85).max(16.0);
                let p_ax = axis_coord(ptr, axis);
                let t_min = axis_coord(thumb_rect.min, axis);
                let t_max = axis_coord(thumb_rect.max, axis);
                if p_ax < t_min {
                    out.new_offset = Some((cur_offset - page).clamp(0.0, max_off));
                } else if p_ax > t_max {
                    out.new_offset = Some((cur_offset + page).clamp(0.0, max_off));
                }
            }
        }
    } else {
        anchor.active = false;
    }

    ui.ctx().data_mut(|d| d.insert_temp(anchor_id, anchor));

    // Desenho clássico VEGAS Pro: trilha rebaixada + indicador com nervura central
    let p = ui.painter();
    p.rect_filled(track_rect, CornerRadius::ZERO, theme.extreme_bg);
    p.rect_stroke(
        track_rect,
        CornerRadius::ZERO,
        Stroke::new(1.0_f32, theme.border),
        StrokeKind::Inside,
    );

    if scrollable {
        let hovered_thumb = ui
            .ctx()
            .pointer_hover_pos()
            .is_some_and(|pos| thumb_rect.expand(1.0).contains(pos));
        let thumb_bg = if anchor.active {
            theme.selection_bg
        } else if hovered_thumb {
            theme.widget_hover_bg
        } else {
            theme.widget_bg
        };
        let thumb_border = if theme.high_contrast && (anchor.active || hovered_thumb) {
            theme.accent
        } else {
            theme.border
        };

        p.rect_filled(thumb_rect, CornerRadius::same(1), thumb_bg);
        p.rect_stroke(
            thumb_rect,
            CornerRadius::same(1),
            Stroke::new(1.0_f32, thumb_border),
            StrokeKind::Inside,
        );

        if thumb_len >= 22.0 {
            let grip_col = if anchor.active && !theme.dark_mode && !theme.high_contrast {
                Color32::WHITE.gamma_multiply(0.75)
            } else {
                theme.text_muted.gamma_multiply(0.7)
            };
            let c = thumb_rect.center();
            for off in [-3.0_f32, 0.0, 3.0] {
                match axis {
                    Axis::Horizontal => {
                        let half = (thumb_rect.height() * 0.24).clamp(2.0, 4.0);
                        p.line_segment(
                            [pos2(c.x + off, c.y - half), pos2(c.x + off, c.y + half)],
                            Stroke::new(1.0_f32, grip_col),
                        );
                    }
                    Axis::Vertical => {
                        let half = (thumb_rect.width() * 0.24).clamp(2.0, 4.0);
                        p.line_segment(
                            [pos2(c.x - half, c.y + off), pos2(c.x + half, c.y + off)],
                            Stroke::new(1.0_f32, grip_col),
                        );
                    }
                }
            }
        }
    }

    out
}

/// Barra completa `[◀/▲] [trilha] [▶/▼]` no retângulo informado.
#[allow(clippy::too_many_arguments)]
pub fn arrow_scrollbar(
    ui: &mut egui::Ui,
    id: Id,
    rect: Rect,
    axis: Axis,
    range: ScrollbarRange,
    step: f32,
    dec_label: &str,
    inc_label: &str,
    theme: &ThemeColors,
) -> ArrowScrollbarOutcome {
    let mut out = ArrowScrollbarOutcome::default();
    let thickness = match axis {
        Axis::Horizontal => rect.height(),
        Axis::Vertical => rect.width(),
    };
    let len = axis_len(rect, axis);
    if thickness <= 2.0 || len <= thickness * 2.0 + 4.0 {
        let t = scrollbar_track(ui, id.with("track"), rect, axis, range, theme);
        return ArrowScrollbarOutcome {
            new_offset: t.new_offset,
            double_clicked: t.double_clicked,
        };
    }

    let btn_len = thickness.clamp(11.0, 18.0);
    let (dec_rect, track_rect, inc_rect, dec_dir, inc_dir) = match axis {
        Axis::Horizontal => (
            Rect::from_min_max(rect.min, pos2(rect.left() + btn_len, rect.bottom())),
            Rect::from_min_max(
                pos2(rect.left() + btn_len, rect.top()),
                pos2(rect.right() - btn_len, rect.bottom()),
            ),
            Rect::from_min_max(pos2(rect.right() - btn_len, rect.top()), rect.max),
            ArrowDir::Left,
            ArrowDir::Right,
        ),
        Axis::Vertical => (
            Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + btn_len)),
            Rect::from_min_max(
                pos2(rect.left(), rect.top() + btn_len),
                pos2(rect.right(), rect.bottom() - btn_len),
            ),
            Rect::from_min_max(pos2(rect.left(), rect.bottom() - btn_len), rect.max),
            ArrowDir::Up,
            ArrowDir::Down,
        ),
    };

    let cur = range.sanitized_offset();
    let max_off = range.max_offset();
    let can_dec = range.is_scrollable() && cur > 0.5;
    let can_inc = range.is_scrollable() && cur < max_off - 0.5;
    let step_val = step.max(1.0);

    if arrow_button(
        ui,
        id.with("dec"),
        dec_rect,
        dec_dir,
        can_dec,
        dec_label,
        dec_label,
        theme,
    ) {
        out.new_offset = Some((cur - step_val).clamp(0.0, max_off));
    }

    let track_out = scrollbar_track(ui, id.with("track"), track_rect, axis, range, theme);
    if track_out.new_offset.is_some() {
        out.new_offset = track_out.new_offset;
    }
    out.double_clicked = track_out.double_clicked;

    if arrow_button(
        ui,
        id.with("inc"),
        inc_rect,
        inc_dir,
        can_inc,
        inc_label,
        inc_label,
        theme,
    ) {
        out.new_offset = Some((cur + step_val).clamp(0.0, max_off));
    }

    out
}

#[derive(Clone, Copy, Default)]
struct CachedScrollExtent {
    content: Vec2,
    viewport: Vec2,
}

/// Invólucro de `egui::ScrollArea::both()` com barras de rolagem externas estilo
/// VEGAS Pro (`▲`/`▼` na borda direita e `◀`/`▶` na borda inferior quando necessário).
pub fn arrow_scroll_area<R>(
    ui: &mut egui::Ui,
    id: Id,
    theme: &ThemeColors,
    always_show_v: bool,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let avail = ui.available_rect_before_wrap();
    if avail.width() <= DOCK_SCROLL_W + 8.0 || avail.height() <= DOCK_SCROLL_W + 8.0 {
        let out = egui::ScrollArea::both()
            .id_salt(id)
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, add_contents);
        return out.inner;
    }

    let ext_id = id.with("cached_extent");
    let prev = ui
        .ctx()
        .data_mut(|d| d.get_temp::<CachedScrollExtent>(ext_id).unwrap_or_default());
    let need_v = always_show_v || (prev.viewport.y > 0.0 && prev.content.y > prev.viewport.y + 1.0);
    let need_h = prev.viewport.x > 0.0 && prev.content.x > prev.viewport.x + 1.0;

    let bar_w = if need_v { DOCK_SCROLL_W } else { 0.0 };
    let bar_h = if need_h { DOCK_SCROLL_W } else { 0.0 };
    let content_rect = Rect::from_min_max(
        avail.min,
        pos2(avail.right() - bar_w, avail.bottom() - bar_h),
    );

    // Aloca todo o retângulo disponível antes de renderizar o painel interno.
    ui.allocate_rect(avail, Sense::hover());

    let mut content_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let scroll_out = egui::ScrollArea::both()
        .id_salt(id.with("area"))
        .auto_shrink([false, false])
        .max_width(content_rect.width())
        .max_height(content_rect.height())
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(&mut content_ui, add_contents);

    let viewport_size = scroll_out.inner_rect.size();
    let content_size = scroll_out.content_size;
    ui.ctx().data_mut(|d| {
        d.insert_temp(
            ext_id,
            CachedScrollExtent {
                content: content_size,
                viewport: viewport_size,
            },
        )
    });

    let mut next_offset = scroll_out.state.offset;
    let mut changed = false;

    if need_v {
        let v_rect = Rect::from_min_max(
            pos2(content_rect.right(), avail.top()),
            pos2(avail.right(), content_rect.bottom()),
        );
        let v_range = ScrollbarRange {
            offset: scroll_out.state.offset.y,
            content: content_size.y,
            viewport: viewport_size.y,
        };
        let step = (viewport_size.y * 0.14).clamp(24.0, 72.0);
        let res = arrow_scrollbar(
            ui,
            id.with("vbar"),
            v_rect,
            Axis::Vertical,
            v_range,
            step,
            "Rolar painel para cima",
            "Rolar painel para baixo",
            theme,
        );
        if let Some(off) = res.new_offset {
            next_offset.y = off;
            changed = true;
        }
    }

    if need_h {
        let h_rect = Rect::from_min_max(
            pos2(avail.left(), content_rect.bottom()),
            pos2(content_rect.right(), avail.bottom()),
        );
        let h_range = ScrollbarRange {
            offset: scroll_out.state.offset.x,
            content: content_size.x,
            viewport: viewport_size.x,
        };
        let step = (viewport_size.x * 0.14).clamp(24.0, 72.0);
        let res = arrow_scrollbar(
            ui,
            id.with("hbar"),
            h_rect,
            Axis::Horizontal,
            h_range,
            step,
            "Rolar painel para a esquerda",
            "Rolar painel para a direita",
            theme,
        );
        if let Some(off) = res.new_offset {
            next_offset.x = off;
            changed = true;
        }
    }

    if need_v && need_h {
        let corner = Rect::from_min_max(content_rect.max, avail.max);
        let p = ui.painter();
        p.rect_filled(corner, CornerRadius::ZERO, theme.panel_fill);
        p.rect_stroke(
            corner,
            CornerRadius::ZERO,
            Stroke::new(1.0_f32, theme.border),
            StrokeKind::Inside,
        );
    }

    if changed {
        let mut st = scroll_out.state;
        st.offset = next_offset;
        st.store(ui.ctx(), scroll_out.id);
        ui.ctx().request_repaint();
    }

    scroll_out.inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrollbar_range_guards_invalid_and_empty_inputs() {
        let r = ScrollbarRange {
            offset: f32::NAN,
            content: 500.0,
            viewport: 0.0,
        };
        assert_eq!(r.max_offset(), 0.0);
        assert_eq!(r.sanitized_offset(), 0.0);
        assert!(!r.is_scrollable());
        assert_eq!(r.thumb_span(100.0, 18.0), (0.0, 100.0));
        assert_eq!(r.offset_from_thumb_pos(100.0, 100.0, 20.0), 0.0);
    }

    #[test]
    fn scrollbar_range_roundtrips_offset_cleanly() {
        let r = ScrollbarRange {
            offset: 250.0,
            content: 1000.0,
            viewport: 200.0,
        };
        assert!(r.is_scrollable());
        assert_eq!(r.max_offset(), 800.0);
        let (start, len) = r.thumb_span(200.0, 18.0);
        assert!((len - 40.0).abs() < 0.01);
        assert!((start - 50.0).abs() < 0.01);
        let back = r.offset_from_thumb_pos(200.0, len, start);
        assert!((back - 250.0).abs() < 0.1);
    }

    #[test]
    fn scrollbar_range_enforces_min_thumb_and_clamps_bounds() {
        let r = ScrollbarRange {
            offset: 200_000.0,
            content: 100_000.0,
            viewport: 100.0,
        };
        let (start, len) = r.thumb_span(200.0, 18.0);
        assert!((len - 18.0).abs() < 0.01);
        assert!((start + len - 200.0).abs() < 0.01);
        assert_eq!(
            r.sanitized_offset(),
            r.max_offset(),
            "offset deve ser limitado ao maximo permitido"
        );
    }
}
