//! Peças comuns das janelas de quadros-chave (Pan/Crop do evento e, depois,
//! Movimento da Trilha), seguindo a disposição do VEGAS Pro descrita em
//! `docs_obsidian/ui_references/referencia_visual_janelas.md`:
//!
//! - grade de propriedades com seções recolhíveis;
//! - área de trabalho clara com grade de pontos, zoom e deslocamento;
//! - linha do tempo de quadros-chave com a barra de navegação.

use crate::icons::{self, Icon};
use crate::theme::ThemeColors;
use crate::velocity_graph::{self, GraphKey};
use eframe::egui::{
    self, pos2, vec2, Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};
use neoveg_common::curve::Curve;
use neoveg_common::ease::{EaseMode, KeyEase};
use neoveg_common::i18n::{tr, Language};
use neoveg_common::time::Ticks;

/// Largura da coluna de nomes na grade de propriedades (comporta rótulos como
/// "Espaçamento da grade:" sem truncamento).
const PROP_LABEL_W: f32 = 152.0;

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
    if theme.high_contrast {
        p.rect_stroke(
            rect,
            0.0,
            Stroke::new(1.5_f32, theme.border),
            StrokeKind::Inside,
        );
    }
    let bx = Rect::from_center_size(pos2(rect.left() + 8.0, rect.center().y), vec2(9.0, 9.0));
    p.rect_filled(bx, 0.0, theme.window_fill);
    p.rect_stroke(
        bx,
        0.0,
        Stroke::new(
            if theme.high_contrast {
                1.5_f32
            } else {
                1.0_f32
            },
            theme.border,
        ),
        StrokeKind::Inside,
    );
    let mid = Stroke::new(
        if theme.high_contrast {
            1.5_f32
        } else {
            1.0_f32
        },
        theme.text_primary,
    );
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
                ui.add(egui::Label::new(egui::RichText::new(label).size(12.0)).truncate())
                    .on_hover_text(label);
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
        if tl.x.is_finite()
            && tl.y.is_finite()
            && br.x.is_finite()
            && br.y.is_finite()
            && step > 0.0
        {
            let x0 = (tl.x / step).floor() as i64;
            let x1 = (br.x / step).ceil() as i64;
            let y0 = (tl.y / step).floor() as i64;
            let y1 = (br.y / step).ceil() as i64;
            if x1 >= x0 && y1 >= y0 && x1.saturating_sub(x0) < 400 && y1.saturating_sub(y0) < 400 {
                for gy in y0..=y1 {
                    for gx in x0..=x1 {
                        let s =
                            self.to_screen(area, frame, vec2(gx as f32 * step, gy as f32 * step));
                        if area.contains(s) {
                            painter.rect_filled(Rect::from_min_size(s, vec2(1.0, 1.0)), 0.0, dot);
                        }
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

/// Processa roda do mouse, pinça/Ctrl+scroll, atalhos `+`/`-` (quando em hover)
/// e arrasto com botão do meio para zoom e deslocamento da área de trabalho.
pub(crate) fn handle_workspace_wheel_and_zoom(
    ui: &Ui,
    resp: &egui::Response,
    area: Rect,
    frame: Vec2,
    ws: &mut Workspace,
) {
    if resp.hovered() {
        // Lê `wants_keyboard_input()` FORA de `ui.input(...)`: `ui.input` mantém
        // um read-lock em `ContextImpl` (`parking_lot::RwLock`) e um segundo
        // read-lock aninhado causa deadlock quando a thread de prévia chama
        // `ctx.request_repaint()` concorrentemente (AGENTS.md §0).
        let wants_kb = !ui.ctx().wants_keyboard_input();
        let (scroll, pinch, plus, minus) = ui.input(|i| {
            let sy = if i.raw_scroll_delta.y.abs() > 0.0 {
                i.raw_scroll_delta.y
            } else {
                i.smooth_scroll_delta.y
            };
            let p = i.zoom_delta();
            let plus =
                wants_kb && (i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals));
            let minus = wants_kb && i.key_pressed(egui::Key::Minus);
            (sy, p, plus, minus)
        });
        let anchor = resp.hover_pos().unwrap_or_else(|| area.center());
        if (pinch - 1.0).abs() > 1e-3 {
            ws.zoom_at(area, frame, anchor, pinch.clamp(0.5, 2.0));
        } else if scroll != 0.0 {
            let f = (1.0 + scroll * 0.002).clamp(0.5, 2.0);
            ws.zoom_at(area, frame, anchor, f);
        }
        if plus {
            ws.zoom_at(area, frame, anchor, 1.25);
        }
        if minus {
            ws.zoom_at(area, frame, anchor, 0.8);
        }
    }
    if resp.dragged_by(egui::PointerButton::Middle) {
        let d = resp.drag_delta();
        ws.offset -= vec2(ws.frame_len(d.x), ws.frame_len(d.y));
    }
}

/// Ferramenta mão da área de trabalho (Pan/Crop, Movimento da Trilha): com a
/// ferramenta escolhida ou com Espaço segurado, arrastar com o botão esquerdo
/// desloca a vista. Devolve `true` quando a mão está ativa, para a área não
/// tratar o arrasto como edição.
pub(crate) fn workspace_hand(
    ui: &Ui,
    resp: &egui::Response,
    ws: &mut Workspace,
    tool: bool,
    theme: &ThemeColors,
    custom_cursors: bool,
) -> bool {
    if !crate::hand::hand_active(ui, resp, tool) {
        return false;
    }
    let grabbing = resp.dragged_by(egui::PointerButton::Primary);
    if grabbing {
        let d = resp.drag_delta();
        ws.offset -= vec2(ws.frame_len(d.x), ws.frame_len(d.y));
        crate::hand::note_drag(ui.ctx());
    }
    if resp.hovered() || grabbing {
        let icon = if grabbing {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::Grab
        };
        crate::cursors::render_cursor(
            ui.ctx(),
            crate::cursors::NeovegCursor::System(icon),
            theme,
            custom_cursors,
        );
    }
    true
}

/// Barra flutuante de zoom no canto inferior direito da área de trabalho
/// (`-`, `XX%`, `+`, `Ajustar`). Devolve o retângulo ocupado pela barra.
pub(crate) fn workspace_zoom_bar(
    ui: &mut Ui,
    id: egui::Id,
    area: Rect,
    frame: Vec2,
    ws: &mut Workspace,
    theme: &ThemeColors,
    lang: Language,
) -> Rect {
    let bar_w = 164.0_f32.min((area.width() - 8.0).max(96.0));
    let bar_h = 22.0;
    let bar_rect = Rect::from_min_size(
        pos2(area.right() - bar_w - 6.0, area.bottom() - bar_h - 6.0),
        vec2(bar_w, bar_h),
    );
    let bg = if theme.high_contrast {
        theme.window_fill
    } else {
        Color32::from_rgba_unmultiplied(
            theme.panel_fill.r(),
            theme.panel_fill.g(),
            theme.panel_fill.b(),
            232,
        )
    };
    ui.painter().rect_filled(bar_rect, 3.0, bg);
    ui.painter().rect_stroke(
        bar_rect,
        3.0,
        Stroke::new(1.0_f32, theme.border),
        StrokeKind::Inside,
    );
    let inner = bar_rect.shrink2(vec2(3.0, 1.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(id)
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 2.0;
    if child
        .small_button("−")
        .on_hover_text(tr(lang, "Reduzir zoom"))
        .clicked()
    {
        ws.zoom_at(area, frame, area.center(), 0.8);
    }
    let pct_lbl = format!("{:.0}%", ws.zoom_pct);
    if child
        .small_button(pct_lbl)
        .on_hover_text(tr(lang, "Redefinir zoom para 100%"))
        .clicked()
    {
        ws.zoom_pct = 100.0;
        ws.offset = Vec2::ZERO;
    }
    if child
        .small_button("+")
        .on_hover_text(tr(lang, "Ampliar zoom"))
        .clicked()
    {
        ws.zoom_at(area, frame, area.center(), 1.25);
    }
    if child
        .small_button(tr(lang, "Ajustar"))
        .on_hover_text(tr(lang, "Ajustar a área de trabalho ao quadro"))
        .clicked()
    {
        ws.fit(area, frame);
    }
    bar_rect
}

/// Prévia gráfica compacta da curva de suavização (`Curve` + `Suavidade 0..100`).
pub(crate) fn paint_curve_preview(ui: &mut Ui, curve: Curve, smoothness: f32, theme: &ThemeColors) {
    let (r, _) = ui.allocate_exact_size(
        vec2(ui.available_width().clamp(80.0, 180.0), 34.0),
        Sense::hover(),
    );
    let p = ui.painter_at(r);
    p.rect_filled(r, 2.0, theme.extreme_bg);
    p.rect_stroke(
        r,
        2.0,
        Stroke::new(1.0_f32, theme.border),
        StrokeKind::Inside,
    );
    let inner = r.shrink(4.0);
    // linha de referência linear
    p.line_segment(
        [inner.left_bottom(), inner.right_top()],
        Stroke::new(1.0_f32, theme.border),
    );
    let steps = 28usize;
    let mut pts = Vec::with_capacity(steps + 1);
    let sm = if smoothness.is_finite() {
        (smoothness * 0.01).clamp(0.0, 1.0)
    } else {
        0.0
    };
    for i in 0..=steps {
        let lin = i as f32 / steps as f32;
        let mut u = curve.shape(lin);
        if sm > 0.0 && curve != Curve::Hold {
            let hermite = u * u * (3.0 - 2.0 * u);
            let quintic = u * u * u * (u * (u * 6.0 - 15.0) + 10.0);
            let target = hermite * (1.0 - sm) + quintic * sm;
            u = u + (target - u) * sm;
        }
        let x = inner.left() + lin * inner.width();
        let y = inner.bottom() - u.clamp(0.0, 1.0) * inner.height();
        pts.push(pos2(x, y));
    }
    p.add(Shape::line(pts, Stroke::new(2.0_f32, theme.accent)));
}

/// Escolha no combo "Tipo" de interpolação.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum InterpChoice {
    /// Curva do VEGAS (desliga o Bézier).
    Curve(Curve),
    /// Interpolação temporal Bézier.
    Ease(KeyEase),
}

/// Combo "Tipo" de interpolação do quadro-chave: as curvas do VEGAS, os
/// três tipos de Bézier e as suavizações de entrada e saída.
pub(crate) fn interp_combo(
    ui: &mut Ui,
    id_salt: &str,
    curve: Option<Curve>,
    ease: Option<KeyEase>,
    lang: Language,
) -> Option<InterpChoice> {
    let selected = match (ease, curve) {
        (Some(e), _) => e.mode.name_in(lang),
        (None, Some(c)) => c.name_in(lang),
        (None, None) => Curve::default().name_in(lang),
    };
    let mut choice = None;
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for k in Curve::ALL.iter().take(6) {
                let on = ease.is_none() && curve == Some(*k);
                if ui.selectable_label(on, k.name_in(lang)).clicked() {
                    choice = Some(InterpChoice::Curve(*k));
                }
            }
            ui.separator();
            for m in EaseMode::ALL {
                let on = ease.is_some_and(|e| e.mode == m);
                if ui.selectable_label(on, m.name_in(lang)).clicked() {
                    let mut e = ease.unwrap_or_else(|| KeyEase::new(m));
                    e.mode = m;
                    choice = Some(InterpChoice::Ease(e));
                }
            }
            ui.separator();
            if ui
                .selectable_label(false, tr(lang, "Suavizar entrada"))
                .clicked()
            {
                choice = Some(InterpChoice::Ease(KeyEase::ease_in(ease)));
            }
            if ui
                .selectable_label(false, tr(lang, "Suavizar saída"))
                .clicked()
            {
                choice = Some(InterpChoice::Ease(KeyEase::ease_out(ease)));
            }
            if ui
                .selectable_label(false, tr(lang, "Suavizar entrada e saída"))
                .clicked()
            {
                choice = Some(InterpChoice::Ease(KeyEase::easy_ease()));
            }
        });
    choice
}

/// Campos de velocidade e influência das alças Bézier (em %). Devolve o
/// Bézier editado e se a alça mexida foi a de saída. O automático vira
/// contínuo quando um campo é editado.
pub(crate) fn ease_fields(ui: &mut Ui, ease: KeyEase, lang: Language) -> Option<(KeyEase, bool)> {
    let mut e = ease;
    let mut edited = None;
    let rows: [(&str, bool, bool); 4] = [
        ("Velocidade de entrada (%):", false, true),
        ("Influência de entrada (%):", false, false),
        ("Velocidade de saída (%):", true, true),
        ("Influência de saída (%):", true, false),
    ];
    for (label, out_side, is_speed) in rows {
        let side = if out_side {
            &mut e.outgoing
        } else {
            &mut e.incoming
        };
        let (mut v, range) = if is_speed {
            (
                side.speed * 100.0,
                -neoveg_common::ease::MAX_SPEED * 100.0..=neoveg_common::ease::MAX_SPEED * 100.0,
            )
        } else {
            (
                side.influence * 100.0,
                neoveg_common::ease::MIN_INFLUENCE * 100.0..=100.0,
            )
        };
        if prop_num(ui, tr(lang, label), &mut v, 1.0, 1, range) {
            if is_speed {
                side.speed = v / 100.0;
            } else {
                side.influence = v / 100.0;
            }
            edited = Some(out_side);
        }
    }
    let out_side = edited?;
    if e.mode == EaseMode::Auto {
        e.mode = EaseMode::Continuous;
    }
    Some((e.sanitized(), out_side))
}

/// Prévia compacta da interpolação de um trecho dada por `progress(u)`.
pub(crate) fn paint_progress_preview(
    ui: &mut Ui,
    theme: &ThemeColors,
    progress: impl Fn(f32) -> f32,
) {
    let (r, _) = ui.allocate_exact_size(
        vec2(ui.available_width().clamp(80.0, 180.0), 34.0),
        Sense::hover(),
    );
    let p = ui.painter_at(r);
    p.rect_filled(r, 2.0, theme.extreme_bg);
    p.rect_stroke(
        r,
        2.0,
        Stroke::new(1.0_f32, theme.border),
        StrokeKind::Inside,
    );
    let inner = r.shrink(4.0);
    p.line_segment(
        [inner.left_bottom(), inner.right_top()],
        Stroke::new(1.0_f32, theme.border),
    );
    let steps = 32usize;
    let pts: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let u = i as f32 / steps as f32;
            let y = progress(u);
            let y = if y.is_finite() {
                y.clamp(-0.25, 1.25)
            } else {
                0.0
            };
            pos2(
                inner.left() + u * inner.width(),
                inner.bottom() - y * inner.height(),
            )
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new(2.0_f32, theme.accent)));
}

/// Uma trilha de animação na linha do tempo de quadros-chave.
pub(crate) struct KfRow<'a> {
    pub label: &'a str,
    pub keys: Vec<(Ticks, Curve)>,
    /// Interpolação Bézier de cada quadro-chave (paralelo a `keys`).
    pub eases: Vec<Option<KeyEase>>,
    /// A linha aceita interpolação temporal Bézier e gráfico de velocidade.
    pub bezier: bool,
    /// Caixa de seleção opcional à esquerda do nome (ex.: Máscara, Sombra 2D, Brilho 2D).
    pub checked: Option<bool>,
    /// Destaca a linha ativa no cabeçalho da trilha de quadros-chave.
    pub selected: bool,
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
    /// Bézier escolhido no menu ou alça arrastada no gráfico de velocidade:
    /// (trilha, índice, Bézier, alça de saída foi a mexida).
    pub set_ease: Option<(usize, usize, KeyEase, bool)>,
    /// "Excluir" no menu de contexto: (trilha, índice).
    pub delete_key: Option<(usize, usize)>,
    /// Clique no nome da trilha de quadros-chave: seleciona a trilha ativa.
    pub select_row: Option<usize>,
    /// Clique na caixa de seleção da trilha: alterna ligado/desligado.
    pub toggle_row: Option<usize>,
}

const KF_LABEL_W: f32 = 150.0;
const KF_RULER_H: f32 = 20.0;
const KF_ROW_H: f32 = 16.0;
const KF_HIT: f32 = 6.0;
/// Altura mínima do gráfico de velocidade abaixo das trilhas.
const KF_GRAPH_MIN_H: f32 = 110.0;

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
/// Cor de preenchimento do quadro-chave conforme a interpolação até o
/// próximo, como no VEGAS Pro: linear cinza, rápida verde, lenta amarela,
/// suave azul, aguda rosa e manter (hold) vermelha.
pub(crate) fn keyframe_fill(curve: Curve) -> Color32 {
    match curve {
        Curve::Linear => Color32::from_rgb(200, 198, 200),
        Curve::Fast => Color32::from_rgb(150, 236, 150),
        Curve::Slow => Color32::from_rgb(214, 206, 130),
        Curve::Smooth => Color32::from_rgb(160, 164, 250),
        Curve::Sharp => Color32::from_rgb(240, 140, 200),
        Curve::Hold => Color32::from_rgb(176, 24, 24),
    }
}

/// Desenha um quadro-chave (losango de meia-largura `half`) com a cor da
/// interpolação e contorno escuro; no alto contraste, contorno grosso na cor
/// do texto. Devolve a cor do contorno, usada por marcas desenhadas por cima.
pub(crate) fn paint_keyframe_diamond(
    p: &egui::Painter,
    c: Pos2,
    half: f32,
    curve: Curve,
    theme: &ThemeColors,
) -> Color32 {
    let (stroke_col, stroke_w) = if theme.high_contrast {
        (theme.text_primary, 1.5_f32)
    } else {
        (Color32::from_rgb(28, 28, 30), 1.0_f32)
    };
    p.add(Shape::convex_polygon(
        vec![
            pos2(c.x, c.y - half),
            pos2(c.x + half, c.y),
            pos2(c.x, c.y + half),
            pos2(c.x - half, c.y),
        ],
        keyframe_fill(curve),
        Stroke::new(stroke_w, stroke_col),
    ));
    stroke_col
}

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
    // linha cujo gráfico de velocidade aparece: a ativa, se aceitar Bézier
    let graph_id = id.with("kf_graph_on");
    let graph_on: bool = ui.data(|d| d.get_temp(graph_id)).unwrap_or(false);
    let graph_row = rows
        .iter()
        .position(|r| r.selected)
        .or(if rows.is_empty() { None } else { Some(0) })
        .filter(|ri| rows.get(*ri).is_some_and(|r| r.bezier));
    let rows_h = KF_RULER_H + KF_ROW_H * rows.len() as f32;
    let min_h = if graph_on && graph_row.is_some() {
        rows_h + KF_GRAPH_MIN_H
    } else {
        rows_h
    };
    let area_h = (ui.available_height() - 28.0).max(min_h);
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
    if empty.is_positive() && empty.width().is_finite() && empty.height().is_finite() {
        let mut yy = empty.top();
        let mut odd_row = false;
        let mut row_guard = 0usize;
        while yy < empty.bottom() && row_guard < 400 {
            let mut xx = empty.left() + if odd_row { cell } else { 0.0 };
            let mut col_guard = 0usize;
            while xx < empty.right() && col_guard < 800 {
                let r = Rect::from_min_size(pos2(xx, yy), vec2(cell, cell)).intersect(empty);
                p.rect_filled(r, 0.0, c1);
                xx += cell * 2.0;
                col_guard += 1;
            }
            yy += cell;
            odd_row = !odd_row;
            row_guard += 1;
        }
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
        let name_bg = if row.selected {
            theme.selection_bg
        } else {
            theme.widget_bg
        };
        let name_fg = if row.selected {
            crate::theme::contrast_on(name_bg)
        } else {
            theme.text_primary
        };
        p.rect_filled(name.shrink2(vec2(0.0, 1.0)), 0.0, name_bg);
        let label_x = if let Some(checked) = row.checked {
            let cb =
                Rect::from_center_size(pos2(name.left() + 9.0, name.center().y), vec2(10.0, 10.0));
            p.rect_filled(cb, 1.0, theme.window_fill);
            p.rect_stroke(
                cb,
                1.0,
                Stroke::new(1.0_f32, theme.border),
                StrokeKind::Inside,
            );
            if checked {
                p.line_segment(
                    [
                        pos2(cb.left() + 2.0, cb.center().y),
                        pos2(cb.center().x, cb.bottom() - 2.0),
                    ],
                    Stroke::new(1.5_f32, theme.accent),
                );
                p.line_segment(
                    [
                        pos2(cb.center().x, cb.bottom() - 2.0),
                        pos2(cb.right() - 2.0, cb.top() + 2.0),
                    ],
                    Stroke::new(1.5_f32, theme.accent),
                );
            }
            name.left() + 18.0
        } else {
            name.left() + 4.0
        };
        p.text(
            pos2(label_x, name.center().y),
            egui::Align2::LEFT_CENTER,
            row.label,
            egui::FontId::proportional(11.0),
            name_fg,
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
        for (ki, &(t, _)) in row.keys.iter().enumerate() {
            let x = x_of(t);
            let cy = lane.center().y;
            let is_bezier = row.eases.get(ki).copied().flatten().is_some();
            let curve = row.keys.get(ki).map_or(Curve::Linear, |k| k.1);
            let stroke_col = paint_keyframe_diamond(&p, pos2(x, cy), 5.0, curve, theme);
            if is_bezier {
                velocity_graph::paint_bezier_mark(&p, pos2(x, cy), stroke_col);
            }
        }
    }
    // cursor
    let cx = x_of(cursor.clamp_safe(Ticks::ZERO, len));
    p.line_segment(
        [pos2(cx, lanes.top()), pos2(cx, lanes.bottom())],
        Stroke::new(
            if theme.high_contrast {
                1.5_f32
            } else {
                1.0_f32
            },
            theme.tl_playhead_edit,
        ),
    );
    p.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0_f32, theme.border),
        StrokeKind::Inside,
    );

    // gráfico de velocidade da linha ativa, abaixo das trilhas
    if let (true, Some(gr)) = (graph_on, graph_row) {
        let graph_rect = Some(Rect::from_min_max(
            pos2(rect.left(), rows_bottom + 2.0),
            rect.max,
        ))
        .filter(|r| r.height() >= 40.0);
        if let (Some(r), Some(row)) = (graph_rect, rows.get(gr)) {
            let gkeys: Vec<GraphKey> = row
                .keys
                .iter()
                .enumerate()
                .map(|(ki, &(t, curve))| GraphKey {
                    t,
                    curve,
                    ease: row.eases.get(ki).copied().flatten(),
                    scalar: None,
                })
                .collect();
            if let Some(edit) = velocity_graph::graph_in_rect(
                ui,
                id.with("kf_vgraph"),
                r,
                KF_LABEL_W,
                &gkeys,
                len,
                cursor,
                theme,
                lang,
            ) {
                out.set_ease = Some((gr, edit.key, edit.ease, edit.edited_out));
            }
        }
    }

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
            None if resp.clicked() && names.contains(pos) => {
                let ri = ((pos.y - ruler.bottom()) / KF_ROW_H).floor();
                if (0.0..rows.len() as f32).contains(&ri) {
                    let ri = ri as usize;
                    out.select_row = Some(ri);
                    if rows.get(ri).and_then(|r| r.checked).is_some()
                        && pos.x <= names.left() + 16.0
                    {
                        out.toggle_row = Some(ri);
                    }
                }
            }
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
        let current_ease = rows
            .get(ri)
            .and_then(|r| r.eases.get(ki))
            .copied()
            .flatten();
        let current = rows
            .get(ri)
            .and_then(|r| r.keys.get(ki))
            .map(|k| k.1)
            .filter(|_| current_ease.is_none());
        for c in Curve::ALL.iter().take(6) {
            if ui.radio(current == Some(*c), c.name_in(lang)).clicked() {
                out.set_curve = Some((ri, ki, *c));
                ui.close_menu();
            }
        }
        if rows.get(ri).is_some_and(|r| r.bezier) {
            ui.separator();
            if let Some(e) = velocity_graph::bezier_menu_items(ui, current_ease, lang) {
                out.set_ease = Some((ri, ki, e, true));
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
        ui.add_space(6.0);
        let can_graph = graph_row.is_some();
        if icons::icon_button(ui, Icon::VelocityEnvelope, None, graph_on, can_graph, theme)
            .on_hover_text(tr(
                lang,
                "Gráfico de velocidade: arraste as alças para ajustar a interpolação Bézier",
            ))
            .on_disabled_hover_text(tr(
                lang,
                "Esta linha de quadros-chave não suporta interpolação Bézier.",
            ))
            .clicked()
        {
            ui.data_mut(|d| d.insert_temp(graph_id, !graph_on));
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

    /// Cada interpolação tem a sua cor (antes todos os quadros-chave tinham a
    /// mesma cor do envelope, qualquer que fosse a curva).
    #[test]
    fn quadro_chave_tem_cor_por_interpolacao() {
        let cores: Vec<Color32> = Curve::ALL.iter().map(|c| keyframe_fill(*c)).collect();
        for (i, a) in cores.iter().enumerate() {
            for b in cores.iter().skip(i + 1) {
                assert_ne!(a, b, "duas interpolações com a mesma cor");
            }
        }
        let f = keyframe_fill(Curve::Fast);
        assert!(f.g() > f.r() && f.g() > f.b(), "rápida é verde");
        let h = keyframe_fill(Curve::Hold);
        assert!(h.r() > h.g() && h.r() > h.b(), "manter é vermelha");
        let s = keyframe_fill(Curve::Smooth);
        assert!(s.b() > s.r() && s.b() > s.g(), "suave é azul");
    }

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

    #[test]
    fn handle_workspace_wheel_and_zoom_nao_trava_com_repaint_concorrente() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc,
        };
        use std::time::Duration;

        let ctx = egui::Context::default();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_bg = stop.clone();
        let ctx_bg = ctx.clone();
        // Simula o worker da prévia de vídeo chamando `request_repaint()` enquanto
        // a janela Movimento da Trilha / Pan/Crop está sob o ponteiro do mouse.
        let bg = std::thread::spawn(move || {
            while !stop_bg.load(Ordering::Relaxed) {
                ctx_bg.request_repaint();
                std::thread::yield_now();
            }
        });

        let (done_tx, done_rx) = mpsc::channel();
        let ctx_ui = ctx.clone();
        let ui_thread = std::thread::spawn(move || {
            let mut ws = Workspace::default();
            let frame = vec2(1920.0, 1080.0);
            for _ in 0..80 {
                let mut input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
                    ..Default::default()
                };
                input
                    .events
                    .push(egui::Event::PointerMoved(pos2(200.0, 200.0)));
                let _ = ctx_ui.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let (area, resp) =
                            ui.allocate_exact_size(vec2(500.0, 400.0), Sense::click_and_drag());
                        handle_workspace_wheel_and_zoom(ui, &resp, area, frame, &mut ws);
                    });
                });
            }
            let _ = done_tx.send(());
        });

        let finished = done_rx.recv_timeout(Duration::from_secs(5)).is_ok();
        stop.store(true, Ordering::Relaxed);
        let _ = bg.join();
        assert!(
            finished,
            "handle_workspace_wheel_and_zoom entrou em deadlock sob request_repaint concorrente"
        );
        let _ = ui_thread.join();
    }
}
