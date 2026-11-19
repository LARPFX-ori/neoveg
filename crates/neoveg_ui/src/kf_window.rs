//! Peças comuns das janelas de quadros-chave (Pan/Crop do evento e, depois,
//! Movimento da Trilha), seguindo a disposição do VEGAS Pro descrita em
//! `docs_obsidian/ui_references/referencia_visual_janelas.md`:
//!
//! - grade de propriedades com seções recolhíveis;
//! - área de trabalho clara com grade de pontos, zoom e deslocamento;
//! - linha do tempo de quadros-chave com a barra de navegação.

use crate::icons::{self, Icon};
use crate::theme::ThemeColors;
use eframe::egui::{
    self, pos2, vec2, Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};
use neoveg_common::curve::Curve;
use neoveg_common::i18n::{tr, Language};
use neoveg_common::time::Ticks;

/// Largura da coluna de nomes na grade de propriedades.
const PROP_LABEL_W: f32 = 92.0;

/// Cabeçalho de seção da grade de propriedades, com o quadradinho `+`/`−`.
/// O estado aberto/fechado fica na memória do egui, por `id`.
pub(crate) fn prop_section(
    ui: &mut Ui,
    id: egui::Id,
    title: &str,
    default_open: bool,
    theme: &ThemeColors,
    body: impl FnOnce(&mut Ui),
) {
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(default_open);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 17.0), Sense::click());
    let p = ui.painter();
    let head_bg = if open {
        theme.widget_hover_bg
    } else {
        theme.widget_bg
    };
    p.rect_filled(rect, 0.0, head_bg);
    let bx = Rect::from_center_size(pos2(rect.left() + 8.0, rect.center().y), vec2(9.0, 9.0));
    p.rect_filled(bx, 0.0, theme.window_fill);
    p.rect_stroke(
        bx,
        0.0,
        Stroke::new(1.0_f32, theme.text_muted),
        StrokeKind::Inside,
    );
    let mid = Stroke::new(1.0_f32, theme.text_primary);
    p.line_segment(
        [
            bx.left_center() + vec2(2.0, 0.0),
            bx.right_center() - vec2(2.0, 0.0),
        ],
        mid,
    );
    if !open {
        p.line_segment(
            [
                bx.center_top() + vec2(0.0, 2.0),
                bx.center_bottom() - vec2(0.0, 2.0),
            ],
            mid,
        );
    }
    p.text(
        pos2(rect.left() + 16.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(12.0),
        theme.text_primary,
    );
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, true, open, title)
    });
    if resp.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    if open {
        ui.spacing_mut().item_spacing.y = 1.0;
        body(ui);
    }
}

/// Linha "· nome    valor" da grade de propriedades.
pub(crate) fn prop_row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(12.0, 16.0), Sense::hover());
        let c = ui.visuals().weak_text_color();
        ui.painter()
            .circle_filled(pos2(r.left() + 8.0, r.center().y), 1.0, c);
        // rótulo de verdade (e não só pintado) para leitores de tela
        ui.allocate_ui_with_layout(
            vec2(PROP_LABEL_W - 12.0 - ui.spacing().item_spacing.x, 16.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(PROP_LABEL_W - 12.0 - ui.spacing().item_spacing.x);
                ui.add(egui::Label::new(egui::RichText::new(label).size(12.0)).truncate());
            },
        );
        add(ui)
    })
    .inner
}

/// Linha numérica editável (arrastar ou digitar). Devolve `true` se mudou.
pub(crate) fn prop_num(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    speed: f64,
    decimals: usize,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    prop_row(ui, label, |ui| {
        ui.add(
            egui::DragValue::new(value)
                .speed(speed)
                .range(range)
                .fixed_decimals(decimals),
        )
        .changed()
    })
}

/// Área de trabalho: zoom em porcentagem (pixels de tela por pixel do
/// quadro × 100), deslocamento em pixels do quadro e espaçamento da grade.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Workspace {
    pub zoom_pct: f32,
    pub offset: Vec2,
    pub grid: f32,
    /// Já ajustado ao tamanho da área uma vez (primeira abertura).
    pub fitted: bool,
}

impl Default for Workspace {
    fn default() -> Self {
        Workspace {
            zoom_pct: 50.0,
            offset: Vec2::ZERO,
            grid: 16.0,
            fitted: false,
        }
    }
}

pub(crate) const WS_MIN_ZOOM: f32 = 1.0;
pub(crate) const WS_MAX_ZOOM: f32 = 1600.0;

impl Workspace {
    /// Zoom em que o quadro ocupa uns 60% da área, centralizado.
    pub(crate) fn fit(&mut self, area: Rect, frame: Vec2) {
        if frame.x <= 0.0 || frame.y <= 0.0 || area.width() <= 0.0 || area.height() <= 0.0 {
            return;
        }
        let z = (area.width() / (frame.x * 1.6)).min(area.height() / (frame.y * 1.6)) * 100.0;
        self.zoom_pct = z.clamp(WS_MIN_ZOOM, WS_MAX_ZOOM);
        self.offset = Vec2::ZERO;
        self.fitted = true;
    }

    fn scale(&self) -> f32 {
        if self.zoom_pct.is_finite() {
            self.zoom_pct.clamp(WS_MIN_ZOOM, WS_MAX_ZOOM) / 100.0
        } else {
            0.5
        }
    }

    fn off(&self) -> Vec2 {
        if self.offset.x.is_finite() && self.offset.y.is_finite() {
            self.offset
        } else {
            Vec2::ZERO
        }
    }

    pub(crate) fn to_screen(self, area: Rect, frame: Vec2, p: Vec2) -> Pos2 {
        area.center() + (p - frame * 0.5 - self.off()) * self.scale()
    }

    pub(crate) fn to_frame(self, area: Rect, frame: Vec2, s: Pos2) -> Vec2 {
        (s - area.center()) / self.scale() + frame * 0.5 + self.off()
    }

    /// Pixels do quadro que cabem em `px` pixels de tela.
    pub(crate) fn frame_len(&self, px: f32) -> f32 {
        px / self.scale()
    }

    /// Multiplica o zoom por `factor` mantendo fixo o ponto sob `anchor`.
    pub(crate) fn zoom_at(&mut self, area: Rect, frame: Vec2, anchor: Pos2, factor: f32) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let before = self.to_frame(area, frame, anchor);
        self.zoom_pct = (self.scale() * 100.0 * factor).clamp(WS_MIN_ZOOM, WS_MAX_ZOOM);
        let after = self.to_frame(area, frame, anchor);
        self.offset = self.off() + (before - after);
    }

    /// Fundo claro, grade de pontos e as linhas que prolongam as bordas do
    /// quadro de saída.
    pub(crate) fn paint_background(
        &self,
        painter: &egui::Painter,
        area: Rect,
        frame: Vec2,
        theme: &ThemeColors,
    ) {
        self.paint_background_opt(painter, area, frame, theme, true);
    }

    /// Como [`Self::paint_background`]; `edges = false` omite as linhas das
    /// bordas do quadro (vistas de cima e de lado do 3D).
    pub(crate) fn paint_background_opt(
        &self,
        painter: &egui::Painter,
        area: Rect,
        frame: Vec2,
        theme: &ThemeColors,
        edges: bool,
    ) {
        let (bg, dot, edge) = if theme.high_contrast {
            (theme.extreme_bg, theme.text_muted, theme.border)
        } else {
            (
                Color32::from_gray(234),
                Color32::from_gray(150),
                Color32::from_gray(178),
            )
        };
        painter.rect_filled(area, 0.0, bg);
        // passo da grade em pixels do quadro, dobrado até ficar legível
        let mut step = if self.grid.is_finite() {
            self.grid.clamp(1.0, 4096.0)
        } else {
            16.0
        };
        let sc = self.scale();
        while step * sc < 12.0 && step < 1e6 {
            step *= 2.0;
        }
        let tl = self.to_frame(area, frame, area.left_top());
        let br = self.to_frame(area, frame, area.right_bottom());
        let x0 = (tl.x / step).floor() as i64;
        let x1 = (br.x / step).ceil() as i64;
        let y0 = (tl.y / step).floor() as i64;
        let y1 = (br.y / step).ceil() as i64;
        if (x1 - x0) < 400 && (y1 - y0) < 400 {
            for gy in y0..=y1 {
                for gx in x0..=x1 {
                    let s = self.to_screen(area, frame, vec2(gx as f32 * step, gy as f32 * step));
                    if area.contains(s) {
                        painter.rect_filled(Rect::from_min_size(s, vec2(1.0, 1.0)), 0.0, dot);
                    }
                }
            }
        }
        if !edges {
            return;
        }
        let a = self.to_screen(area, frame, Vec2::ZERO);
        let b = self.to_screen(area, frame, frame);
        let st = Stroke::new(1.0_f32, edge);
        for x in [a.x, b.x] {
            painter.line_segment([pos2(x, area.top()), pos2(x, area.bottom())], st);
        }
        for y in [a.y, b.y] {
            painter.line_segment([pos2(area.left(), y), pos2(area.right(), y)], st);
        }
    }
}

/// Uma trilha de animação na linha do tempo de quadros-chave.
pub(crate) struct KfRow<'a> {
    pub label: &'a str,
    pub keys: Vec<(Ticks, Curve)>,
}

/// Botões da barra de navegação de quadros-chave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KfNav {
    First,
    Prev,
    Next,
    Last,
    Add,
    Delete,
}

#[derive(Default, Debug, PartialEq)]
pub(crate) struct KfStripOut {
    /// Clique ou arrasto na régua/trilhas: novo tempo local.
    pub seek: Option<Ticks>,
    pub nav: Option<KfNav>,
    /// Quadro-chave arrastado: (trilha, índice, novo tempo).
    pub move_key: Option<(usize, usize, Ticks)>,
    /// Interpolação escolhida no menu de contexto: (trilha, índice, curva).
    pub set_curve: Option<(usize, usize, Curve)>,
    /// "Excluir" no menu de contexto: (trilha, índice).
    pub delete_key: Option<(usize, usize)>,
}

const KF_LABEL_W: f32 = 150.0;
const KF_RULER_H: f32 = 20.0;
const KF_ROW_H: f32 = 16.0;
const KF_HIT: f32 = 6.0;

/// Escolhe o passo da régua (em segundos) para dar ~90 px entre rótulos.
fn ruler_step_secs(len_secs: f64, width: f32) -> f64 {
    const STEPS: [f64; 14] = [
        0.04, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0,
    ];
    let per_px = len_secs / f64::from(width.max(1.0));
    STEPS
        .iter()
        .copied()
        .find(|s| s / per_px >= 90.0)
        .unwrap_or(1800.0)
}

/// Linha do tempo de quadros-chave com a barra de navegação embaixo.
/// `cursor` e os tempos são locais (0 = início do evento ou da trilha).
#[allow(clippy::too_many_arguments)]
pub(crate) fn keyframe_strip(
    ui: &mut Ui,
    id: egui::Id,
    rows: &[KfRow<'_>],
    length: Ticks,
    cursor: Ticks,
    sync_cursor: &mut bool,
    fmt: &dyn Fn(Ticks) -> String,
    theme: &ThemeColors,
    lang: Language,
) -> KfStripOut {
    let mut out = KfStripOut::default();
    let len = length.max(Ticks::from_raw(1));
    let full_w = ui.available_width();
    let area_h = (ui.available_height() - 28.0).max(KF_RULER_H + KF_ROW_H * rows.len() as f32);
    let (rect, resp) = ui.allocate_exact_size(vec2(full_w, area_h), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    let names = Rect::from_min_max(rect.min, pos2(rect.left() + KF_LABEL_W, rect.bottom()));
    let lanes = Rect::from_min_max(pos2(names.right(), rect.top()), rect.max);
    p.rect_filled(names, 0.0, theme.panel_fill);
    // fundo xadrez onde não há trilha de animação
    p.rect_filled(lanes, 0.0, theme.tl_bg);
    let rows_bottom = lanes.top() + KF_RULER_H + KF_ROW_H * rows.len() as f32;
    let empty = Rect::from_min_max(pos2(lanes.left(), rows_bottom), lanes.max);
    let cell = 6.0;
    let c1 = theme.widget_bg;
    let mut yy = empty.top();
    let mut odd_row = false;
    while yy < empty.bottom() {
        let mut xx = empty.left() + if odd_row { cell } else { 0.0 };
        while xx < empty.right() {
            let r = Rect::from_min_size(pos2(xx, yy), vec2(cell, cell)).intersect(empty);
            p.rect_filled(r, 0.0, c1);
            xx += cell * 2.0;
        }
        yy += cell;
        odd_row = !odd_row;
    }
    let x_of = |t: Ticks| lanes.left() + t.fraction_in(Ticks::ZERO, len) * lanes.width();
    let t_of = |x: f32| -> Ticks {
        let frac = ((x - lanes.left()) / lanes.width().max(1.0)).clamp(0.0, 1.0);
        Ticks::from_secs_f64_clamped(len.as_secs_f64() * f64::from(frac))
            .clamp_safe(Ticks::ZERO, len)
    };
    // régua
    let ruler = Rect::from_min_max(lanes.min, pos2(lanes.right(), lanes.top() + KF_RULER_H));
    p.rect_filled(ruler, 0.0, theme.tl_ruler_bg);
    let step = ruler_step_secs(len.as_secs_f64(), lanes.width());
    let n = (len.as_secs_f64() / step).floor() as i64;
    let font = egui::FontId::monospace(10.0);
    for i in 0..=n.min(400) {
        let t = Ticks::from_secs_f64_clamped(step * i as f64);
        let x = x_of(t);
        p.line_segment(
            [pos2(x, ruler.bottom() - 7.0), pos2(x, ruler.bottom())],
            Stroke::new(1.0_f32, theme.text_muted),
        );
        for k in 1..5 {
            let xm = x_of(Ticks::from_secs_f64_clamped(
                step * (i as f64 + k as f64 / 5.0),
            ));
            if xm < ruler.right() {
                p.line_segment(
                    [pos2(xm, ruler.bottom() - 3.0), pos2(xm, ruler.bottom())],
                    Stroke::new(1.0_f32, theme.text_muted),
                );
            }
        }
        p.text(
            pos2(x + 2.0, ruler.top() + 1.0),
            egui::Align2::LEFT_TOP,
            fmt(t),
            font.clone(),
            theme.text_muted,
        );
    }
    // trilhas
    for (ri, row) in rows.iter().enumerate() {
        let top = ruler.bottom() + KF_ROW_H * ri as f32;
        let lane = Rect::from_min_max(pos2(lanes.left(), top), pos2(lanes.right(), top + KF_ROW_H));
        let name = Rect::from_min_max(pos2(names.left(), top), pos2(names.right(), top + KF_ROW_H));
        p.rect_filled(
            lane,
            0.0,
            if ri % 2 == 0 {
                theme.tl_lane_even
            } else {
                theme.tl_lane_odd
            },
        );
        p.rect_filled(name.shrink2(vec2(0.0, 1.0)), 0.0, theme.widget_bg);
        p.text(
            pos2(name.left() + 4.0, name.center().y),
            egui::Align2::LEFT_CENTER,
            row.label,
            egui::FontId::proportional(11.0),
            theme.text_primary,
        );
        // seta de início da trilha
        p.add(Shape::convex_polygon(
            vec![
                pos2(lane.left() + 1.0, lane.top() + 3.0),
                pos2(lane.left() + 6.0, lane.center().y),
                pos2(lane.left() + 1.0, lane.bottom() - 3.0),
            ],
            theme.text_muted,
            Stroke::NONE,
        ));
        for &(t, _) in &row.keys {
            let x = x_of(t);
            let cy = lane.center().y;
            p.add(Shape::convex_polygon(
                vec![
                    pos2(x, cy - 5.0),
                    pos2(x + 5.0, cy),
                    pos2(x, cy + 5.0),
                    pos2(x - 5.0, cy),
                ],
                theme.tl_env_primary,
                Stroke::new(1.0_f32, theme.text_primary),
            ));
        }
    }
    // cursor
    let cx = x_of(cursor.clamp_safe(Ticks::ZERO, len));
    p.line_segment(
        [pos2(cx, lanes.top()), pos2(cx, lanes.bottom())],
        Stroke::new(1.0_f32, theme.tl_playhead_edit),
    );
    p.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0_f32, theme.border),
        StrokeKind::Inside,
    );

    // interação: arrastar losango move o quadro-chave; o resto posiciona o cursor
    let key_at = |pos: Pos2| -> Option<(usize, usize)> {
        let ri = ((pos.y - ruler.bottom()) / KF_ROW_H).floor();
        if !(0.0..rows.len() as f32).contains(&ri) {
            return None;
        }
        let ri = ri as usize;
        let row = rows.get(ri)?;
        row.keys
            .iter()
            .enumerate()
            .filter(|(_, (t, _))| (x_of(*t) - pos.x).abs() <= KF_HIT)
            .min_by(|a, b| {
                let da = (x_of(a.1 .0) - pos.x).abs();
                let db = (x_of(b.1 .0) - pos.x).abs();
                da.total_cmp(&db)
            })
            .map(|(ki, _)| (ri, ki))
    };
    let drag_id = id.with("kf_drag");
    if resp.drag_started() {
        let hit = resp
            .interact_pointer_pos()
            .filter(|p| lanes.contains(*p))
            .and_then(key_at);
        ui.data_mut(|d| d.insert_temp(drag_id, hit));
    }
    let dragging: Option<(usize, usize)> = ui.data(|d| d.get_temp(drag_id)).flatten();
    if let Some(pos) = resp
        .interact_pointer_pos()
        .filter(|_| resp.dragged() || resp.clicked())
    {
        match dragging.filter(|_| resp.dragged()) {
            Some((ri, ki)) => out.move_key = Some((ri, ki, t_of(pos.x))),
            None if pos.x >= lanes.left() => out.seek = Some(t_of(pos.x)),
            None => {}
        }
    }
    if resp.drag_stopped() {
        ui.data_mut(|d| d.remove::<Option<(usize, usize)>>(drag_id));
    }
    let menu_key_id = id.with("kf_menu_key");
    if resp.secondary_clicked() {
        let hit = resp.interact_pointer_pos().and_then(key_at);
        ui.data_mut(|d| d.insert_temp(menu_key_id, hit));
    }
    let menu_key: Option<(usize, usize)> = ui.data(|d| d.get_temp(menu_key_id)).flatten();
    resp.context_menu(|ui| {
        let Some((ri, ki)) = menu_key else {
            ui.label(tr(lang, "Clique com o botão direito num quadro-chave"));
            return;
        };
        let current = rows.get(ri).and_then(|r| r.keys.get(ki)).map(|k| k.1);
        for c in Curve::ALL.iter().take(6) {
            if ui.radio(current == Some(*c), c.name_in(lang)).clicked() {
                out.set_curve = Some((ri, ki, *c));
                ui.close_menu();
            }
        }
        ui.separator();
        if ui.button(tr(lang, "Excluir")).clicked() {
            out.delete_key = Some((ri, ki));
            ui.close_menu();
        }
    });

    // barra de navegação
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if icons::icon_button(ui, Icon::SyncCursor, None, *sync_cursor, true, theme)
            .on_hover_text(tr(lang, "Sincronizar o cursor com a linha do tempo"))
            .clicked()
        {
            *sync_cursor = !*sync_cursor;
        }
        let nav = [
            (Icon::GoStart, KfNav::First, "Primeiro quadro-chave"),
            (Icon::PrevFrame, KfNav::Prev, "Quadro-chave anterior"),
            (Icon::NextFrame, KfNav::Next, "Próximo quadro-chave"),
            (Icon::GoEnd, KfNav::Last, "Último quadro-chave"),
            (Icon::KeyframeFilled, KfNav::Add, "Adicionar quadro-chave"),
            (Icon::KeyframeDelete, KfNav::Delete, "Excluir quadro-chave"),
        ];
        for (icon, act, tip) in nav {
            if icons::icon_button(ui, icon, None, false, true, theme)
                .on_hover_text(tr(lang, tip))
                .clicked()
            {
                out.nav = Some(act);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(fmt(cursor))
                    .monospace()
                    .color(theme.timecode_fg),
            );
            icons::paint_inline(ui, Icon::InsertMarker, 13.0, theme);
        });
    });
    out
}

/// Tempo do quadro-chave para onde cada botão da barra leva, a partir de
/// `cursor` (nenhum quando não há para onde ir).
pub(crate) fn nav_target(times: &[Ticks], cursor: Ticks, nav: KfNav) -> Option<Ticks> {
    match nav {
        KfNav::First => times.iter().copied().min(),
        KfNav::Last => times.iter().copied().max(),
        KfNav::Prev => times.iter().copied().filter(|t| *t < cursor).max(),
        KfNav::Next => times.iter().copied().filter(|t| *t > cursor).min(),
        KfNav::Add | KfNav::Delete => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navegacao_entre_quadros_chave() {
        let ts = [
            Ticks::from_secs_i64(0),
            Ticks::from_secs_i64(2),
            Ticks::from_secs_i64(5),
        ];
        let c = Ticks::from_secs_i64(3);
        assert_eq!(nav_target(&ts, c, KfNav::Prev), Some(ts[1]));
        assert_eq!(nav_target(&ts, c, KfNav::Next), Some(ts[2]));
        assert_eq!(nav_target(&ts, c, KfNav::First), Some(ts[0]));
        assert_eq!(nav_target(&ts, c, KfNav::Last), Some(ts[2]));
        assert_eq!(nav_target(&ts, ts[2], KfNav::Next), None);
        assert_eq!(nav_target(&[], c, KfNav::First), None);
    }

    #[test]
    fn area_de_trabalho_ida_e_volta_e_zoom_no_ponteiro() {
        let area = Rect::from_min_size(pos2(10.0, 20.0), vec2(800.0, 500.0));
        let frame = vec2(1920.0, 1080.0);
        let mut ws = Workspace::default();
        ws.fit(area, frame);
        assert!(ws.fitted && ws.zoom_pct > 0.0);
        let p = vec2(300.0, 700.0);
        let s = ws.to_screen(area, frame, p);
        assert!((ws.to_frame(area, frame, s) - p).length() < 0.01);
        let anchor = pos2(600.0, 200.0);
        let before = ws.to_frame(area, frame, anchor);
        ws.zoom_at(area, frame, anchor, 2.0);
        assert!((ws.to_frame(area, frame, anchor) - before).length() < 0.01);
        ws.zoom_at(area, frame, anchor, f32::NAN);
        ws.zoom_at(area, frame, anchor, 1e12);
        assert!(ws.zoom_pct <= WS_MAX_ZOOM);
        ws.offset = vec2(f32::NAN, 0.0);
        assert!(ws.to_screen(area, frame, p).x.is_finite());
    }

    #[test]
    fn passo_da_regua_cresce_com_a_duracao() {
        assert!(ruler_step_secs(10.0, 900.0) <= ruler_step_secs(600.0, 900.0));
        assert!(ruler_step_secs(0.0, 0.0) > 0.0);
        assert!(ruler_step_secs(1e9, 900.0) > 0.0);
    }
}
