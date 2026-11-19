//! Ponteiros vetoriais originais para edição na linha do tempo, prévia,
//! Pan/Crop e Movimento da Trilha (AGENTS.md §1.1: desenho original em código,
//! sem cópia de bitmaps externos; §0: sem pânico com coordenadas inválidas).

use crate::theme::ThemeColors;
use eframe::egui::{self, pos2, vec2, Color32, CursorIcon, LayerId, Order, Pos2, Shape, Stroke};

/// Ponteiro ativo no quadro atual da interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NeovegCursor {
    /// Delega diretamente ao ponteiro nativo do sistema operacional.
    System(CursorIcon),
    /// Aparo da borda esquerda de um evento (`[↔`).
    TrimLeft,
    /// Aparo da borda direita de um evento (`↔]`).
    TrimRight,
    /// Aparo simultâneo entre eventos adjacentes / Roll (`]↔[`).
    TrimAdjacent,
    /// Esticar ou comprimir o tempo na borda do evento (`[↔~`).
    TimeStretch,
    /// Aparo com divisão / Slip Trim (`|↔|`).
    SplitTrim,
    /// Ajuste de fade de entrada/saída nos cantos superiores do evento (arco + `↔`).
    FadeOffset,
    /// Deslizar conteúdo interno do evento (Slip).
    Slip,
    /// Deslizar o ponto de crossfade entre eventos sobrepostos.
    SlipCrossfade,
    /// Deslocar evento entre vizinhos (Slide).
    Slide,
    /// Esfregar (scrub) na régua de tempo ou arrastar o playhead.
    Scrub,
    /// Rotação em Pan/Crop e Movimento da Trilha.
    Rotate,
    /// Mover livremente (cruz de 4 setas).
    Move,
    /// Mover restrito ao eixo horizontal (X).
    MoveX,
    /// Mover restrito ao eixo vertical (Y).
    MoveY,
    /// Manipular ponto ou segmento de envelope / ganho de evento.
    EnvelopeHand,
    /// Ampliar vista (lupa com `+`).
    ZoomIn,
    /// Reduzir vista (lupa com `−`).
    ZoomOut,
    /// Ferramenta Seleção por caixa (mira + retângulo).
    SelectBox,
    /// Caneta Bézier para desenhar máscara.
    MaskPen,
    /// Caneta Bézier para adicionar vértice na máscara (`+`).
    MaskPenAdd,
    /// Caneta Bézier para remover vértice da máscara (`−`).
    MaskPenDel,
    /// Mover vértice ou alça tangente de máscara Bézier.
    MaskVertex,
}

impl Default for NeovegCursor {
    fn default() -> Self {
        Self::System(CursorIcon::Default)
    }
}

impl NeovegCursor {
    /// Todas as variantes personalizadas (usadas na validação e testes).
    pub const CUSTOM_ALL: [NeovegCursor; 22] = [
        Self::TrimLeft,
        Self::TrimRight,
        Self::TrimAdjacent,
        Self::TimeStretch,
        Self::SplitTrim,
        Self::FadeOffset,
        Self::Slip,
        Self::SlipCrossfade,
        Self::Slide,
        Self::Scrub,
        Self::Rotate,
        Self::Move,
        Self::MoveX,
        Self::MoveY,
        Self::EnvelopeHand,
        Self::ZoomIn,
        Self::ZoomOut,
        Self::SelectBox,
        Self::MaskPen,
        Self::MaskPenAdd,
        Self::MaskPenDel,
        Self::MaskVertex,
    ];

    /// Ponteiro do sistema equivalente quando os ponteiros personalizados estão
    /// desligados nas Preferências ou fora da janela.
    pub fn fallback_icon(self) -> CursorIcon {
        match self {
            Self::System(icon) => icon,
            Self::TrimLeft
            | Self::TrimRight
            | Self::TrimAdjacent
            | Self::TimeStretch
            | Self::SplitTrim
            | Self::Slip
            | Self::SlipCrossfade
            | Self::Slide
            | Self::Scrub => CursorIcon::ResizeHorizontal,
            Self::FadeOffset => CursorIcon::ResizeNeSw,
            Self::Rotate => CursorIcon::Alias,
            Self::Move => CursorIcon::Move,
            Self::MoveX => CursorIcon::ResizeHorizontal,
            Self::MoveY => CursorIcon::ResizeVertical,
            Self::EnvelopeHand => CursorIcon::PointingHand,
            Self::ZoomIn => CursorIcon::ZoomIn,
            Self::ZoomOut => CursorIcon::ZoomOut,
            Self::SelectBox
            | Self::MaskPen
            | Self::MaskPenAdd
            | Self::MaskPenDel
            | Self::MaskVertex => CursorIcon::Crosshair,
        }
    }

    pub fn is_custom(self) -> bool {
        !matches!(self, Self::System(_))
    }
}

/// Aplica o ponteiro escolhido ao `egui::Context` no fim do quadro.
pub fn render_cursor(
    ctx: &egui::Context,
    cursor: NeovegCursor,
    theme: &ThemeColors,
    use_custom: bool,
) {
    if !use_custom || !cursor.is_custom() {
        ctx.set_cursor_icon(cursor.fallback_icon());
        return;
    }
    let hover = ctx.input(|i| i.pointer.hover_pos());
    let Some(pos) = hover.filter(|p| p.x.is_finite() && p.y.is_finite()) else {
        ctx.set_cursor_icon(cursor.fallback_icon());
        return;
    };
    ctx.set_cursor_icon(CursorIcon::None);
    let painter = ctx.layer_painter(LayerId::new(
        Order::Tooltip,
        egui::Id::new("neoveg_custom_cursor_layer"),
    ));
    paint_cursor(&painter, pos, cursor, theme);
}

/// Desenha um ponteiro vetorial original centrado no ponto de ação (`pos`).
pub fn paint_cursor(painter: &egui::Painter, pos: Pos2, cursor: NeovegCursor, theme: &ThemeColors) {
    if !pos.x.is_finite() || !pos.y.is_finite() {
        return;
    }
    // Duplo traço (sombra/contorno escuro + corpo claro, invertido no Alto
    // Contraste claro) garante legibilidade sobre qualquer cor de evento/vídeo.
    let (fg, bg) = if theme.high_contrast && !theme.dark_mode {
        (Color32::BLACK, Color32::WHITE)
    } else if theme.high_contrast {
        (Color32::from_rgb(255, 255, 0), Color32::BLACK)
    } else {
        (Color32::WHITE, Color32::from_rgb(16, 16, 20))
    };
    let accent = if theme.high_contrast {
        fg
    } else {
        Color32::from_rgb(120, 205, 255)
    };
    let ow = if theme.high_contrast {
        3.6_f32
    } else {
        3.0_f32
    };
    let iw = if theme.high_contrast {
        1.8_f32
    } else {
        1.4_f32
    };

    let line = |a: Pos2, b: Pos2| {
        painter.line_segment([a, b], Stroke::new(ow, bg));
        painter.line_segment([a, b], Stroke::new(iw, fg));
    };
    let line_accent = |a: Pos2, b: Pos2| {
        painter.line_segment([a, b], Stroke::new(ow, bg));
        painter.line_segment([a, b], Stroke::new(iw, accent));
    };
    let h_arrows = |center: Pos2, half: f32| {
        let l = pos2(center.x - half, center.y);
        let r = pos2(center.x + half, center.y);
        line(l, r);
        // ponta esquerda
        line(l, pos2(l.x + 3.5, l.y - 3.0));
        line(l, pos2(l.x + 3.5, l.y + 3.0));
        // ponta direita
        line(r, pos2(r.x - 3.5, r.y - 3.0));
        line(r, pos2(r.x - 3.5, r.y + 3.0));
    };
    let v_arrows = |center: Pos2, half: f32| {
        let t = pos2(center.x, center.y - half);
        let b = pos2(center.x, center.y + half);
        line(t, b);
        line(t, pos2(t.x - 3.0, t.y + 3.5));
        line(t, pos2(t.x + 3.0, t.y + 3.5));
        line(b, pos2(b.x - 3.0, b.y - 3.5));
        line(b, pos2(b.x + 3.0, b.y - 3.5));
    };
    let bracket_left = |x: f32, y: f32, h: f32| {
        let top = pos2(x, y - h);
        let bot = pos2(x, y + h);
        line(top, bot);
        line(top, pos2(x + 4.0, y - h));
        line(bot, pos2(x + 4.0, y + h));
    };
    let bracket_right = |x: f32, y: f32, h: f32| {
        let top = pos2(x, y - h);
        let bot = pos2(x, y + h);
        line(top, bot);
        line(top, pos2(x - 4.0, y - h));
        line(bot, pos2(x - 4.0, y + h));
    };

    match cursor {
        NeovegCursor::System(_) => {}
        NeovegCursor::TrimLeft => {
            bracket_left(pos.x, pos.y, 7.0);
            h_arrows(pos2(pos.x + 2.0, pos.y), 9.0);
        }
        NeovegCursor::TrimRight => {
            bracket_right(pos.x, pos.y, 7.0);
            h_arrows(pos2(pos.x - 2.0, pos.y), 9.0);
        }
        NeovegCursor::TrimAdjacent => {
            bracket_right(pos.x - 2.0, pos.y, 7.0);
            bracket_left(pos.x + 2.0, pos.y, 7.0);
            h_arrows(pos, 10.5);
        }
        NeovegCursor::TimeStretch => {
            bracket_left(pos.x, pos.y - 1.5, 6.5);
            h_arrows(pos2(pos.x + 1.5, pos.y - 1.5), 8.5);
            // onda senoidal indicando compressão/alongamento temporal
            let mut pts = Vec::with_capacity(11);
            for i in 0..=10 {
                let u = i as f32 / 10.0;
                let wx = pos.x - 7.5 + u * 15.0;
                let wy = pos.y + 8.5 + (u * std::f32::consts::TAU * 1.5).sin() * 2.2;
                pts.push(pos2(wx, wy));
            }
            painter.add(Shape::line(pts.clone(), Stroke::new(ow, bg)));
            painter.add(Shape::line(pts, Stroke::new(iw, accent)));
        }
        NeovegCursor::SplitTrim => {
            line(pos2(pos.x, pos.y - 8.5), pos2(pos.x, pos.y + 8.5));
            h_arrows(pos, 9.5);
            painter.circle_filled(pos, 2.2, bg);
            painter.circle_filled(pos, 1.3, accent);
        }
        NeovegCursor::FadeOffset => {
            // quadrante de arco suave de fade
            let mut arc = Vec::with_capacity(9);
            for i in 0..=8 {
                let u = i as f32 / 8.0;
                let ax = pos.x - 6.0 + u * 12.0;
                let ay = pos.y + 5.0 - (1.0 - (1.0 - u) * (1.0 - u)) * 10.0;
                arc.push(pos2(ax, ay));
            }
            painter.add(Shape::line(arc.clone(), Stroke::new(ow, bg)));
            painter.add(Shape::line(arc, Stroke::new(iw, accent)));
            h_arrows(pos2(pos.x, pos.y + 7.0), 8.0);
        }
        NeovegCursor::Slip => {
            let box_r = egui::Rect::from_center_size(pos, vec2(15.0, 10.0));
            painter.rect_stroke(box_r, 1.0, Stroke::new(ow, bg), egui::StrokeKind::Middle);
            painter.rect_stroke(box_r, 1.0, Stroke::new(iw, fg), egui::StrokeKind::Middle);
            h_arrows(pos, 6.0);
        }
        NeovegCursor::SlipCrossfade => {
            line_accent(
                pos2(pos.x - 6.0, pos.y - 5.0),
                pos2(pos.x + 6.0, pos.y + 5.0),
            );
            line_accent(
                pos2(pos.x - 6.0, pos.y + 5.0),
                pos2(pos.x + 6.0, pos.y - 5.0),
            );
            h_arrows(pos2(pos.x, pos.y + 8.5), 8.0);
        }
        NeovegCursor::Slide => {
            line(
                pos2(pos.x - 9.0, pos.y - 6.0),
                pos2(pos.x - 9.0, pos.y + 6.0),
            );
            line(
                pos2(pos.x + 9.0, pos.y - 6.0),
                pos2(pos.x + 9.0, pos.y + 6.0),
            );
            let inner = egui::Rect::from_center_size(pos, vec2(9.0, 8.0));
            painter.rect_filled(inner, 1.0, bg);
            painter.rect_stroke(
                inner,
                1.0,
                Stroke::new(iw, accent),
                egui::StrokeKind::Inside,
            );
            h_arrows(pos2(pos.x, pos.y + 8.5), 8.0);
        }
        NeovegCursor::Scrub => {
            line_accent(pos2(pos.x, pos.y - 8.5), pos2(pos.x, pos.y + 8.5));
            let head = vec![
                pos2(pos.x - 4.0, pos.y - 9.5),
                pos2(pos.x + 4.0, pos.y - 9.5),
                pos2(pos.x, pos.y - 5.0),
            ];
            painter.add(Shape::convex_polygon(
                head,
                accent,
                Stroke::new(1.0_f32, bg),
            ));
            h_arrows(pos2(pos.x, pos.y + 2.0), 8.5);
        }
        NeovegCursor::Rotate => {
            let r = 7.5_f32;
            let mut pts = Vec::with_capacity(17);
            for i in 0..=16 {
                let ang =
                    -std::f32::consts::FRAC_PI_4 + (i as f32 / 16.0) * std::f32::consts::PI * 1.5;
                pts.push(pos2(pos.x + ang.cos() * r, pos.y + ang.sin() * r));
            }
            painter.add(Shape::line(pts.clone(), Stroke::new(ow, bg)));
            painter.add(Shape::line(pts, Stroke::new(iw, fg)));
            if let (Some(&p0), Some(&p1)) = (
                Some(&pos2(pos.x + r * 0.707, pos.y - r * 0.707)),
                Some(&pos2(pos.x + r * 0.707, pos.y + r * 0.707)),
            ) {
                line(p0, pos2(p0.x - 3.5, p0.y - 1.0));
                line(p0, pos2(p0.x + 0.5, p0.y + 3.5));
                line(p1, pos2(p1.x - 3.5, p1.y + 1.0));
                line(p1, pos2(p1.x + 0.5, p1.y - 3.5));
            }
        }
        NeovegCursor::Move => {
            h_arrows(pos, 9.0);
            v_arrows(pos, 9.0);
        }
        NeovegCursor::MoveX => {
            h_arrows(pos, 10.0);
            line_accent(pos2(pos.x, pos.y - 3.5), pos2(pos.x, pos.y + 3.5));
        }
        NeovegCursor::MoveY => {
            v_arrows(pos, 10.0);
            line_accent(pos2(pos.x - 3.5, pos.y), pos2(pos.x + 3.5, pos.y));
        }
        NeovegCursor::EnvelopeHand => {
            // mira fina com nó losango de envelope e setas verticais
            v_arrows(pos2(pos.x, pos.y), 8.5);
            let d = vec![
                pos2(pos.x, pos.y - 3.5),
                pos2(pos.x + 3.5, pos.y),
                pos2(pos.x, pos.y + 3.5),
                pos2(pos.x - 3.5, pos.y),
            ];
            painter.add(Shape::convex_polygon(d, accent, Stroke::new(1.2_f32, bg)));
        }
        NeovegCursor::ZoomIn | NeovegCursor::ZoomOut => {
            let c = pos2(pos.x - 2.0, pos.y - 2.0);
            painter.circle_stroke(c, 6.0, Stroke::new(ow, bg));
            painter.circle_stroke(c, 6.0, Stroke::new(iw, fg));
            line(pos2(c.x + 4.3, c.y + 4.3), pos2(c.x + 9.5, c.y + 9.5));
            line_accent(pos2(c.x - 3.0, c.y), pos2(c.x + 3.0, c.y));
            if matches!(cursor, NeovegCursor::ZoomIn) {
                line_accent(pos2(c.x, c.y - 3.0), pos2(c.x, c.y + 3.0));
            }
        }
        NeovegCursor::SelectBox => {
            line(pos2(pos.x - 8.0, pos.y), pos2(pos.x + 8.0, pos.y));
            line(pos2(pos.x, pos.y - 8.0), pos2(pos.x, pos.y + 8.0));
            let mini = egui::Rect::from_min_size(pos2(pos.x + 3.0, pos.y + 3.0), vec2(7.0, 6.0));
            painter.rect_stroke(mini, 0.0, Stroke::new(ow, bg), egui::StrokeKind::Middle);
            painter.rect_stroke(mini, 0.0, Stroke::new(iw, accent), egui::StrokeKind::Middle);
        }
        NeovegCursor::MaskPen | NeovegCursor::MaskPenAdd | NeovegCursor::MaskPenDel => {
            // bico de caneta Bézier apontando exatamente para `pos`
            let nib = vec![
                pos,
                pos2(pos.x + 11.0, pos.y + 5.0),
                pos2(pos.x + 8.5, pos.y + 8.5),
                pos2(pos.x + 5.0, pos.y + 11.0),
            ];
            painter.add(Shape::convex_polygon(nib, fg, Stroke::new(1.4_f32, bg)));
            line(pos, pos2(pos.x + 6.5, pos.y + 6.5));
            if matches!(cursor, NeovegCursor::MaskPenAdd | NeovegCursor::MaskPenDel) {
                let sc = pos2(pos.x + 13.0, pos.y + 13.0);
                line_accent(pos2(sc.x - 3.0, sc.y), pos2(sc.x + 3.0, sc.y));
                if matches!(cursor, NeovegCursor::MaskPenAdd) {
                    line_accent(pos2(sc.x, sc.y - 3.0), pos2(sc.x, sc.y + 3.0));
                }
            }
        }
        NeovegCursor::MaskVertex => {
            let sq = egui::Rect::from_center_size(pos, vec2(7.0, 7.0));
            painter.rect_filled(sq, 0.0, bg);
            painter.rect_stroke(sq, 0.0, Stroke::new(iw, accent), egui::StrokeKind::Inside);
            line(pos2(pos.x - 9.0, pos.y), pos2(pos.x - 4.0, pos.y));
            line(pos2(pos.x + 4.0, pos.y), pos2(pos.x + 9.0, pos.y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{palette, ThemeId};

    #[test]
    fn todos_os_ponteiros_desenham_sem_panico_inclusive_entradas_hostis() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, egui::Id::new("t")));
            for theme_id in ThemeId::ALL {
                let th = palette(theme_id);
                for c in NeovegCursor::CUSTOM_ALL {
                    assert!(c.is_custom());
                    paint_cursor(&painter, pos2(120.0, 80.0), c, &th);
                    // Entradas hostis (§0.4): NaN e infinito nunca causam pânico.
                    paint_cursor(&painter, pos2(f32::NAN, 10.0), c, &th);
                    paint_cursor(&painter, pos2(f32::INFINITY, f32::NEG_INFINITY), c, &th);
                }
            }
        });
    }

    #[test]
    fn fallback_de_cada_ponteiro_e_valido() {
        assert!(!NeovegCursor::default().is_custom());
        for c in NeovegCursor::CUSTOM_ALL {
            assert_ne!(c.fallback_icon(), CursorIcon::None);
        }
    }
}
