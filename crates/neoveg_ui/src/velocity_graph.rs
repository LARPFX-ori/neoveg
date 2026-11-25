//! Gráfico de valor e de velocidade dos quadros-chave, com alças Bézier
//! arrastáveis.
//!
//! O comportamento segue a interpolação temporal descrita na documentação
//! pública de editores de vídeo: cada quadro-chave Bézier tem uma alça de
//! entrada e uma de saída; arrastar a alça para cima ou para baixo muda a
//! velocidade naquele lado e para os lados muda a influência (quanto do
//! trecho ela afeta). O desenho usa as cores e a densidade das janelas de
//! quadros-chave do Neoveg (régua, trilhas e cursor de 1 px), sem arte de
//! terceiros.
//!
//! Propriedades de um só número (parâmetros de efeito) mostram o gráfico de
//! valor em cima e o de velocidade embaixo, em unidades por segundo.
//! Propriedades compostas (posição, recorte, movimento) mostram só a
//! velocidade, em porcentagem do trecho por segundo.

use crate::theme::ThemeColors;
use eframe::egui::{self, pos2, vec2, Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui};
use neoveg_common::curve::Curve;
use neoveg_common::ease::{self, EaseKey, EaseMode, KeyEase};
use neoveg_common::i18n::{tr, Language};
use neoveg_common::time::Ticks;

/// Raio de clique das alças, em pixels.
const HANDLE_HIT: f32 = 7.0;
/// Máximo de colunas amostradas (entrada hostil: janela gigante).
const MAX_COLUMNS: usize = 2400;

/// Um quadro-chave como o gráfico o vê.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GraphKey {
    pub t: Ticks,
    /// Curva do VEGAS do trecho que sai deste quadro-chave.
    pub curve: Curve,
    pub ease: Option<KeyEase>,
    /// Valor numérico, quando a propriedade tem um só número.
    pub scalar: Option<f32>,
}

/// Alça arrastada: novo Bézier do quadro-chave `key`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GraphEdit {
    pub key: usize,
    pub ease: KeyEase,
    /// `true` quando a alça mexida é a de saída.
    pub edited_out: bool,
}

fn ease_keys(keys: &[GraphKey]) -> Vec<EaseKey> {
    keys.iter()
        .map(|k| EaseKey {
            t: k.t,
            scalar: k.scalar,
            ease: k.ease,
        })
        .collect()
}

/// Trecho que contém `t` (`None` antes do primeiro ou depois do último).
fn segment_at(keys: &[GraphKey], t: Ticks) -> Option<(usize, f32)> {
    for (i, w) in keys.windows(2).enumerate() {
        let (a, b) = (w.first()?, w.get(1)?);
        if t >= a.t && t < b.t {
            return Some((i, Ticks::lerp_factor(t, a.t, b.t)));
        }
    }
    None
}

/// Posição das alças de um quadro-chave no gráfico de velocidade:
/// `(tempo da ponta, velocidade real por segundo)` para entrada e saída.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct HandlePos {
    pub incoming: Option<(Ticks, f32)>,
    pub outgoing: Option<(Ticks, f32)>,
}

/// Alças de todos os quadros-chave Bézier (calculadas, também no automático).
pub(crate) fn handle_positions(keys: &[GraphKey]) -> Vec<HandlePos> {
    let ek = ease_keys(keys);
    let n = keys.len();
    (0..n)
        .map(|k| {
            let Some(e) = ease::resolved_ease(&ek, k) else {
                return HandlePos::default();
            };
            let Some(key) = keys.get(k) else {
                return HandlePos::default();
            };
            let outgoing = keys.get(k + 1).map(|next| {
                let dt = next.t.0.saturating_sub(key.t.0).max(0);
                let tip = Ticks::from_raw(
                    key.t
                        .0
                        .saturating_add((dt as f64 * f64::from(e.outgoing.influence)) as i64),
                );
                (tip, e.outgoing.speed * ease::segment_avg_per_sec(&ek, k))
            });
            let incoming = k.checked_sub(1).and_then(|p| keys.get(p)).map(|prev| {
                let dt = key.t.0.saturating_sub(prev.t.0).max(0);
                let tip = Ticks::from_raw(
                    key.t
                        .0
                        .saturating_sub((dt as f64 * f64::from(e.incoming.influence)) as i64),
                );
                (
                    tip,
                    e.incoming.speed * ease::segment_avg_per_sec(&ek, k - 1),
                )
            });
            HandlePos { incoming, outgoing }
        })
        .collect()
}

/// Novo Bézier ao soltar a alça `edited_out` do quadro-chave `k` em
/// `(tempo, velocidade real)`. O automático vira contínuo ao ser mexido,
/// como nos editores que usam esse modelo.
pub(crate) fn drag_handle(
    keys: &[GraphKey],
    k: usize,
    edited_out: bool,
    t: Ticks,
    real_speed: f32,
) -> Option<KeyEase> {
    let ek = ease_keys(keys);
    let mut e = ease::resolved_ease(&ek, k).unwrap_or_else(|| KeyEase::new(EaseMode::Bezier));
    if e.mode == EaseMode::Auto {
        e.mode = EaseMode::Continuous;
    }
    let key = keys.get(k)?;
    if edited_out {
        let next = keys.get(k + 1)?;
        let dt = next.t.0.saturating_sub(key.t.0).max(1) as f64;
        let infl = (t.0.saturating_sub(key.t.0) as f64 / dt) as f32;
        e.outgoing.influence = infl;
        e.outgoing.speed = ease::normalize_speed(&ek, k, real_speed);
    } else {
        let prev = keys.get(k.checked_sub(1)?)?;
        let dt = key.t.0.saturating_sub(prev.t.0).max(1) as f64;
        let infl = (key.t.0.saturating_sub(t.0) as f64 / dt) as f32;
        e.incoming.influence = infl;
        e.incoming.speed = ease::normalize_speed(&ek, k - 1, real_speed);
    }
    let e = e.sanitized();
    // no contínuo o outro lado acompanha a mesma velocidade real
    Some(ease::enforce_continuous(&ek, k, e, edited_out))
}

/// Amostras `(valor, velocidade por segundo)` em `cols + 1` tempos igualmente
/// espaçados de `0` a `length`. `None` em tempos fora dos trechos.
fn sample(keys: &[GraphKey], length: Ticks, cols: usize) -> Vec<(Option<f32>, Option<f32>, usize)> {
    let ek = ease_keys(keys);
    let len = length.max(Ticks::from_raw(1));
    (0..=cols)
        .map(|j| {
            let frac = j as f64 / cols.max(1) as f64;
            let t = Ticks::from_raw((len.0 as f64 * frac) as i64);
            match segment_at(keys, t) {
                Some((i, u)) => {
                    let (Some(a), Some(b)) = (keys.get(i), keys.get(i + 1)) else {
                        return (None, None, usize::MAX);
                    };
                    let p = ease::segment_progress(&ek, i, a.curve, u);
                    let value = match (a.scalar, b.scalar) {
                        (Some(va), Some(vb)) => Some(va + (vb - va) * p),
                        _ => None,
                    };
                    let speed = ease::segment_speed_per_sec(&ek, i, a.curve, u);
                    (value, Some(speed), i)
                }
                None => {
                    let edge = if keys.first().is_some_and(|k| t < k.t) {
                        keys.first()
                    } else {
                        keys.last()
                    };
                    (edge.and_then(|k| k.scalar), Some(0.0), usize::MAX)
                }
            }
        })
        .collect()
}

fn fmt_speed(v: f32, scalar: bool) -> String {
    if scalar {
        format!("{v:.2}/s")
    } else {
        format!("{:.0} %/s", v * 100.0)
    }
}

/// Desenha o gráfico em `rect` (coluna de nomes com `label_w` px à
/// esquerda) e trata o arraste das alças. O eixo de tempo vai de `0` a
/// `length`, alinhado com a linha do tempo de quadros-chave acima dele.
#[allow(clippy::too_many_arguments)]
pub(crate) fn graph_in_rect(
    ui: &mut Ui,
    id: egui::Id,
    rect: Rect,
    label_w: f32,
    keys: &[GraphKey],
    length: Ticks,
    cursor: Ticks,
    theme: &ThemeColors,
    lang: Language,
) -> Option<GraphEdit> {
    if !rect.is_positive() || !rect.width().is_finite() || !rect.height().is_finite() {
        return None;
    }
    let p = ui.painter_at(rect);
    let names = Rect::from_min_max(rect.min, pos2(rect.left() + label_w, rect.bottom()));
    let plot =
        Rect::from_min_max(pos2(names.right(), rect.top()), rect.max).shrink2(vec2(0.0, 3.0));
    p.rect_filled(names, 0.0, theme.panel_fill);
    p.rect_filled(plot.expand2(vec2(0.0, 3.0)), 0.0, theme.extreme_bg);
    let scalar = keys.len() >= 2 && keys.iter().all(|k| k.scalar.is_some_and(f32::is_finite));
    let (value_rect, speed_rect) = if scalar {
        let mid = plot.top() + plot.height() * 0.45;
        (
            Some(Rect::from_min_max(plot.min, pos2(plot.right(), mid - 2.0))),
            Rect::from_min_max(pos2(plot.left(), mid + 2.0), plot.max),
        )
    } else {
        (None, plot)
    };
    let len = length.max(Ticks::from_raw(1));
    let x_of = |t: Ticks| plot.left() + t.fraction_in(Ticks::ZERO, len) * plot.width();
    let t_of = |x: f32| -> Ticks {
        let frac = ((x - plot.left()) / plot.width().max(1.0)).clamp(0.0, 1.0);
        Ticks::from_raw((len.0 as f64 * f64::from(frac)) as i64)
    };
    let cols = (plot.width().max(2.0) as usize).min(MAX_COLUMNS);
    let samples = sample(keys, len, cols);
    let handles = handle_positions(keys);
    let small = egui::FontId::proportional(10.0);
    let thin = Stroke::new(1.0_f32, theme.border);
    let curve_w = if theme.high_contrast {
        2.0_f32
    } else {
        1.5_f32
    };

    // gráfico de valor
    if let Some(vr) = value_rect {
        let vals: Vec<f32> = samples
            .iter()
            .filter_map(|s| s.0)
            .filter(|v| v.is_finite())
            .collect();
        let lo = vals.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = vals.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let (lo, hi) = if lo.is_finite() && hi.is_finite() && hi > lo {
            (lo, hi)
        } else {
            let c = if lo.is_finite() { lo } else { 0.0 };
            (c - 1.0, c + 1.0)
        };
        let y_of = |v: f32| vr.bottom() - ((v - lo) / (hi - lo)).clamp(-0.1, 1.1) * vr.height();
        p.line_segment([vr.left_top(), vr.right_top()], thin);
        p.line_segment([vr.left_bottom(), vr.right_bottom()], thin);
        let pts: Vec<Pos2> = samples
            .iter()
            .enumerate()
            .filter_map(|(j, s)| {
                let x = plot.left() + plot.width() * j as f32 / cols.max(1) as f32;
                s.0.map(|v| pos2(x, y_of(v)))
            })
            .collect();
        if pts.len() >= 2 {
            p.add(Shape::line(pts, Stroke::new(curve_w, theme.accent)));
        }
        for k in keys {
            if let Some(v) = k.scalar {
                let c = pos2(x_of(k.t), y_of(v));
                p.add(Shape::convex_polygon(
                    vec![
                        c + vec2(0.0, -4.0),
                        c + vec2(4.0, 0.0),
                        c + vec2(0.0, 4.0),
                        c + vec2(-4.0, 0.0),
                    ],
                    theme.tl_env_primary,
                    Stroke::new(1.0_f32, theme.text_primary),
                ));
            }
        }
        p.text(
            pos2(names.left() + 4.0, vr.top()),
            egui::Align2::LEFT_TOP,
            tr(lang, "Valor"),
            small.clone(),
            theme.text_primary,
        );
        p.text(
            pos2(names.right() - 4.0, vr.top()),
            egui::Align2::RIGHT_TOP,
            format!("{hi:.2}"),
            small.clone(),
            theme.text_muted,
        );
        p.text(
            pos2(names.right() - 4.0, vr.bottom()),
            egui::Align2::RIGHT_BOTTOM,
            format!("{lo:.2}"),
            small.clone(),
            theme.text_muted,
        );
    }

    // gráfico de velocidade
    let sr = speed_rect;
    let mut vmax = samples
        .iter()
        .filter_map(|s| s.1)
        .filter(|v| v.is_finite())
        .fold(0.0_f32, |m, v| m.max(v.abs()));
    for h in &handles {
        for (_, v) in h.incoming.iter().chain(h.outgoing.iter()) {
            if v.is_finite() {
                vmax = vmax.max(v.abs());
            }
        }
    }
    let vmax = if vmax > 1e-9 { vmax * 1.15 } else { 1.0 };
    let signed = samples.iter().filter_map(|s| s.1).any(|v| v < -1e-6)
        || handles
            .iter()
            .flat_map(|h| h.incoming.iter().chain(h.outgoing.iter()))
            .any(|(_, v)| *v < -1e-6);
    let vmin = if signed { -vmax } else { 0.0 };
    let y_speed = |v: f32| {
        let v = if v.is_finite() { v } else { 0.0 };
        sr.bottom() - ((v - vmin) / (vmax - vmin)).clamp(-0.05, 1.05) * sr.height()
    };
    let speed_from_y = |y: f32| vmin + (sr.bottom() - y) / sr.height().max(1.0) * (vmax - vmin);
    p.line_segment([sr.left_top(), sr.right_top()], thin);
    if signed {
        p.line_segment(
            [
                pos2(sr.left(), y_speed(0.0)),
                pos2(sr.right(), y_speed(0.0)),
            ],
            Stroke::new(1.0_f32, theme.tl_grid_line),
        );
    }
    let mut run: Vec<Pos2> = Vec::new();
    let mut run_seg = usize::MAX;
    let flush = |run: &mut Vec<Pos2>| {
        if run.len() >= 2 {
            p.add(Shape::line(
                std::mem::take(run),
                Stroke::new(curve_w, theme.accent),
            ));
        } else {
            run.clear();
        }
    };
    for (j, s) in samples.iter().enumerate() {
        let x = plot.left() + plot.width() * j as f32 / cols.max(1) as f32;
        if s.2 != run_seg {
            flush(&mut run);
            run_seg = s.2;
        }
        if let Some(v) = s.1 {
            run.push(pos2(x, y_speed(v)));
        }
    }
    flush(&mut run);
    p.text(
        pos2(names.left() + 4.0, sr.top()),
        egui::Align2::LEFT_TOP,
        tr(lang, "Velocidade"),
        small.clone(),
        theme.text_primary,
    );
    p.text(
        pos2(names.right() - 4.0, sr.top() + 12.0),
        egui::Align2::RIGHT_TOP,
        fmt_speed(vmax, scalar),
        small.clone(),
        theme.text_muted,
    );
    p.text(
        pos2(names.right() - 4.0, sr.bottom()),
        egui::Align2::RIGHT_BOTTOM,
        fmt_speed(vmin, scalar),
        small.clone(),
        theme.text_muted,
    );

    // alças
    let handle_col = if theme.high_contrast {
        theme.text_primary
    } else {
        theme.tl_env_secondary
    };
    let mut tips: Vec<(usize, bool, Pos2)> = Vec::new();
    for (k, h) in handles.iter().enumerate() {
        let Some(key) = keys.get(k) else {
            continue;
        };
        let kx = x_of(key.t);
        for (side_out, hp) in [(false, h.incoming), (true, h.outgoing)] {
            let Some((tip_t, v)) = hp else {
                continue;
            };
            let y = y_speed(v);
            let anchor = pos2(kx, y);
            let tip = pos2(x_of(tip_t), y);
            p.line_segment([anchor, tip], Stroke::new(1.0_f32, handle_col));
            p.circle_filled(tip, 3.5, handle_col);
            p.rect_filled(
                Rect::from_center_size(anchor, vec2(5.0, 5.0)),
                0.0,
                theme.text_primary,
            );
            tips.push((k, side_out, tip));
        }
    }

    // cursor
    let cx = x_of(cursor.clamp_safe(Ticks::ZERO, len));
    p.line_segment(
        [pos2(cx, plot.top()), pos2(cx, plot.bottom())],
        Stroke::new(1.0_f32, theme.tl_playhead_edit),
    );
    p.rect_stroke(rect, 0.0, thin, StrokeKind::Inside);

    // interação: arrastar a ponta de uma alça
    let resp = ui.interact(plot, id.with("vg_area"), Sense::click_and_drag());
    let nearest = |pos: Pos2| -> Option<(usize, bool)> {
        tips.iter()
            .map(|(k, o, tp)| (*k, *o, tp.distance(pos)))
            .filter(|(_, _, d)| *d <= HANDLE_HIT)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(k, o, _)| (k, o))
    };
    if resp.hovered() && resp.hover_pos().and_then(nearest).is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    let drag_id = id.with("vg_drag");
    if resp.drag_started() {
        let hit = resp.interact_pointer_pos().and_then(nearest);
        ui.data_mut(|d| d.insert_temp(drag_id, hit));
    }
    let dragging: Option<(usize, bool)> = ui.data(|d| d.get_temp(drag_id)).flatten();
    let mut edit = None;
    if let (Some((k, out)), true) = (dragging, resp.dragged()) {
        if let Some(pos) = resp.interact_pointer_pos() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            let real = speed_from_y(pos.y.clamp(sr.top(), sr.bottom()));
            if let Some(e) = drag_handle(keys, k, out, t_of(pos.x), real) {
                let side = if out { e.outgoing } else { e.incoming };
                let _ = egui::show_tooltip_at_pointer(
                    ui.ctx(),
                    ui.layer_id(),
                    id.with("vg_tip"),
                    |ui| {
                        ui.label(format!(
                            "{}: {}   {}: {:.0} %",
                            tr(lang, "Velocidade"),
                            fmt_speed(real, scalar),
                            tr(lang, "Influência"),
                            side.influence * 100.0
                        ));
                    },
                );
                edit = Some(GraphEdit {
                    key: k,
                    ease: e,
                    edited_out: out,
                });
            }
        }
    }
    if resp.drag_stopped() {
        ui.data_mut(|d| d.remove::<Option<(usize, bool)>>(drag_id));
    }
    edit
}

/// Gráfico solto (janela de efeitos): ocupa a largura disponível e `height`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn velocity_graph(
    ui: &mut Ui,
    id: egui::Id,
    keys: &[GraphKey],
    length: Ticks,
    cursor: Ticks,
    height: f32,
    theme: &ThemeColors,
    lang: Language,
) -> Option<GraphEdit> {
    let w = ui.available_width().max(120.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, height.max(60.0)), Sense::hover());
    graph_in_rect(ui, id, rect, 70.0, keys, length, cursor, theme, lang)
}

/// Gráficos de valor e velocidade dos parâmetros de efeito com o botão de
/// gráfico ligado (janela de efeitos), com o combo "Tipo" e as alças do
/// quadro-chave no cursor. Devolve `true` se algum parâmetro mudou.
#[allow(clippy::too_many_arguments)]
pub(crate) fn fx_param_graphs(
    ui: &mut Ui,
    fx: &mut neoveg_engine::model::FxInstance,
    specs: &[neoveg_engine::model::ParamSpec],
    fx_id: neoveg_common::curve::Id,
    local_t: Ticks,
    span: Option<Ticks>,
    tol: Ticks,
    theme: &ThemeColors,
    lang: Language,
) -> bool {
    let mut changed = false;
    for (pi, spec) in specs.iter().enumerate() {
        let gid = egui::Id::new(("fx_vgraph", fx_id, pi));
        let on: bool = ui.data(|d| d.get_temp(gid)).unwrap_or(false);
        let Some(p) = fx.params.get_mut(pi) else {
            continue;
        };
        if !on || p.keys.len() < 2 {
            continue;
        }
        ui.add_space(2.0);
        ui.label(egui::RichText::new(spec.name_in(lang)).strong().small());
        let ki = p.keys.iter().position(|k| k.0.abs_diff(local_t) <= tol);
        match ki {
            Some(ki) => {
                let cur_curve = p.curves.get(ki).copied().unwrap_or(Curve::Linear);
                let cur_ease = p.ease_index(ki);
                let key_t = p.keys.get(ki).map(|k| k.0).unwrap_or(local_t);
                crate::kf_window::prop_row(ui, tr(lang, "Tipo:"), |ui| {
                    match crate::kf_window::interp_combo(
                        ui,
                        &format!("fx_interp_{fx_id}_{pi}"),
                        Some(cur_curve),
                        cur_ease,
                        lang,
                    ) {
                        Some(crate::kf_window::InterpChoice::Curve(c)) => {
                            changed |= p.set_curve(key_t, c, tol);
                        }
                        Some(crate::kf_window::InterpChoice::Ease(e)) => {
                            changed |= p.set_ease_index(ki, Some(e), true);
                        }
                        None => {}
                    }
                });
                if let Some(e) = cur_ease {
                    if let Some((ne, edited_out)) = crate::kf_window::ease_fields(ui, e, lang) {
                        changed |= p.set_ease_index(ki, Some(ne), edited_out);
                    }
                }
            }
            None => {
                ui.label(
                    egui::RichText::new(tr(
                        lang,
                        "Posicione o cursor sobre um quadro-chave para mudar a interpolação.",
                    ))
                    .small()
                    .weak(),
                );
            }
        }
        let gkeys: Vec<GraphKey> = p
            .keys
            .iter()
            .enumerate()
            .map(|(i, k)| GraphKey {
                t: k.0,
                curve: p.curves.get(i).copied().unwrap_or(Curve::Linear),
                ease: p.ease_index(i),
                scalar: Some(k.1),
            })
            .collect();
        let last = p.keys.last().map(|k| k.0).unwrap_or(Ticks::ZERO);
        let len = span.unwrap_or_else(|| {
            let pad = Ticks::from_raw(last.0 / 10).max(Ticks::from_secs_i64(1));
            (last + pad).max(local_t)
        });
        if let Some(edit) = velocity_graph(
            ui,
            egui::Id::new(("fx_vg", fx_id, pi)),
            &gkeys,
            len,
            local_t,
            150.0,
            theme,
            lang,
        ) {
            if p.ease_index(edit.key) != Some(edit.ease) {
                changed |= p.set_ease_index(edit.key, Some(edit.ease), edit.edited_out);
            }
        }
    }
    changed
}

/// Itens Bézier do menu de interpolação de um quadro-chave, depois das
/// curvas do VEGAS. `current` é o Bézier atual (se houver). Devolve o novo
/// Bézier escolhido (`Some(None)` não ocorre: voltar ao VEGAS é escolher uma
/// curva comum).
pub(crate) fn bezier_menu_items(
    ui: &mut Ui,
    current: Option<KeyEase>,
    lang: Language,
) -> Option<KeyEase> {
    let mut chosen = None;
    ui.label(egui::RichText::new(tr(lang, "Interpolação temporal")).small());
    for m in EaseMode::ALL {
        let on = current.is_some_and(|e| e.mode == m);
        if ui.radio(on, m.name_in(lang)).clicked() {
            let mut e = current.unwrap_or_else(|| KeyEase::new(m));
            e.mode = m;
            chosen = Some(e);
        }
    }
    ui.separator();
    if ui.button(tr(lang, "Suavizar entrada")).clicked() {
        chosen = Some(KeyEase::ease_in(current));
    }
    if ui.button(tr(lang, "Suavizar saída")).clicked() {
        chosen = Some(KeyEase::ease_out(current));
    }
    if ui.button(tr(lang, "Suavizar entrada e saída")).clicked() {
        chosen = Some(KeyEase::easy_ease());
    }
    chosen
}

/// Marca de quadro-chave Bézier: uma barrinha no losango, para distinguir
/// das curvas do VEGAS na linha do tempo de quadros-chave.
pub(crate) fn paint_bezier_mark(p: &egui::Painter, center: Pos2, color: Color32) {
    p.line_segment(
        [center + vec2(0.0, -3.0), center + vec2(0.0, 3.0)],
        Stroke::new(1.5_f32, color),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gk(secs: i64, v: Option<f32>, ease: Option<KeyEase>) -> GraphKey {
        GraphKey {
            t: Ticks::from_secs_i64(secs),
            curve: Curve::Linear,
            ease,
            scalar: v,
        }
    }

    #[test]
    fn alcas_ficam_na_velocidade_e_na_influencia_do_trecho() {
        let keys = [
            gk(0, Some(0.0), Some(KeyEase::new(EaseMode::Bezier))),
            gk(2, Some(10.0), Some(KeyEase::new(EaseMode::Bezier))),
        ];
        let h = handle_positions(&keys);
        let (tip, v) = h[0].outgoing.unwrap_or_default();
        // um terço de 2 s, velocidade média 5/s
        assert!((tip.as_secs_f64() - 2.0 / 3.0).abs() < 1e-3);
        assert!((v - 5.0).abs() < 1e-3);
        assert!(h[0].incoming.is_none() && h[1].outgoing.is_none());
    }

    #[test]
    fn arrastar_alca_muda_velocidade_e_influencia() {
        let keys = [
            gk(0, Some(0.0), Some(KeyEase::new(EaseMode::Auto))),
            gk(2, Some(10.0), None),
        ];
        let e = drag_handle(&keys, 0, true, Ticks::from_secs_i64(1), 0.0).unwrap_or_default();
        assert_eq!(e.mode, EaseMode::Continuous, "automático vira contínuo");
        assert!((e.outgoing.influence - 0.5).abs() < 1e-4);
        assert_eq!(e.outgoing.speed, 0.0);
        // alça de entrada do primeiro quadro-chave não existe
        assert!(drag_handle(&keys, 0, false, Ticks::ZERO, 1.0).is_none());
        // índice inválido não entra em pânico
        assert!(drag_handle(&keys, 9, true, Ticks::ZERO, 1.0).is_none());
    }

    #[test]
    fn amostras_sem_quadros_chave_nao_travam() {
        assert_eq!(sample(&[], Ticks::ZERO, 0).len(), 1);
        let one = [gk(1, Some(3.0), None)];
        let s = sample(&one, Ticks::from_secs_i64(2), 4);
        assert!(s.iter().all(|x| x.1 == Some(0.0)));
        assert!(handle_positions(&one)[0] == HandlePos::default());
    }
}
