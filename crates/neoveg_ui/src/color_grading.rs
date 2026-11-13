//! Painel acoplável de Color Grading estilo VEGAS Pro, com controles vetoriais
//! originais (Lift / Gamma / Gain / Balanço Temp × Tinta / Saturação) ligados em
//! tempo real a `FxKind::ColorCorrector` no evento, trilha ou saída Master.

use super::{App, DockTab, FxTarget};
use crate::icons::{self, Icon};
use crate::model::{FxInstance, FxKind, Ticks, TrackKind};
use crate::theme::ThemeColors;
use eframe::egui;
use egui::{pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind};

#[derive(Clone, Copy)]
struct GradingPreset {
    label: &'static str,
    /// [lift, gamma, gain, temperatura, tonalidade, saturacao]
    values: [f32; 6],
}

const GRADING_PRESETS: &[GradingPreset] = &[
    GradingPreset {
        label: "Neutro",
        values: [0.0, 1.0, 1.0, 0.0, 0.0, 1.0],
    },
    GradingPreset {
        label: "Cinema Quente",
        values: [-0.04, 1.06, 1.08, 0.28, -0.04, 1.12],
    },
    GradingPreset {
        label: "Frio Noturno",
        values: [-0.06, 0.94, 0.96, -0.35, 0.06, 0.88],
    },
    GradingPreset {
        label: "Contraste Punch",
        values: [-0.10, 1.12, 1.18, 0.05, 0.0, 1.22],
    },
    GradingPreset {
        label: "Vintage Suave",
        values: [0.06, 0.90, 0.94, 0.22, 0.08, 0.78],
    },
];

struct ResolvedTarget {
    target: FxTarget,
    label: String,
    local_t: Ticks,
}

impl App {
    fn resolve_color_grading_target(&self) -> ResolvedTarget {
        if let Some(ev_id) = self.sel_events.iter().copied().next() {
            if let Some((ti, _)) = self.project.find_event(ev_id) {
                if self
                    .project
                    .tracks
                    .get(ti)
                    .is_some_and(|t| t.kind == TrackKind::Video)
                {
                    if let Some(ev) = self.project.event(ev_id) {
                        let local_t = (self.cursor - ev.start).clamp_safe(Ticks::ZERO, ev.length);
                        return ResolvedTarget {
                            target: FxTarget::Event(ev_id),
                            label: format!("Evento: {}", ev.name),
                            local_t,
                        };
                    }
                }
            }
        }
        if let Some(ti) = self.sel_track {
            if let Some(t) = self.project.tracks.get(ti) {
                if t.kind == TrackKind::Video {
                    return ResolvedTarget {
                        target: FxTarget::Track(t.id),
                        label: format!("Trilha: {}", t.name),
                        local_t: self.cursor,
                    };
                }
            }
        }
        ResolvedTarget {
            target: FxTarget::MasterVideo,
            label: "Saída Master de Vídeo".to_string(),
            local_t: self.cursor,
        }
    }

    /// Conteúdo da aba `Color Grading` no painel acoplável / janela flutuante.
    pub(crate) fn color_grading_tab(&mut self, ui: &mut egui::Ui) {
        let resolved = self.resolve_color_grading_target();
        let target = resolved.target;
        let local_t = resolved.local_t;
        let tol = self.project.settings.frame_dur().div_i64(2);

        ui.horizontal_wrapped(|ui| {
            icons::icon_label(ui, Icon::ColorWheels, 15.0, "Color Grading", &self.theme);
            ui.separator();
            ui.label(egui::RichText::new(&resolved.label).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::small_icon_button(
                    ui,
                    Icon::ScopesWave,
                    Some("Escopos"),
                    self.is_panel_open(DockTab::Scopes),
                    &self.theme,
                )
                .on_hover_text("Abrir painel de Escopos")
                .clicked()
                {
                    self.exec_cmd(Some(ui.ctx()), "view.tab.scopes");
                }
                if icons::small_icon_button(
                    ui,
                    Icon::FxSparkle,
                    Some("Cadeia FX..."),
                    self.dlg.fx_target == Some(target),
                    &self.theme,
                )
                .on_hover_text("Abrir cadeia completa de efeitos do alvo")
                .clicked()
                {
                    self.dlg.fx_target = Some(target);
                }
            });
        });
        ui.separator();

        let existing_idx = self.fx_chain_mut(target).and_then(|chain| {
            chain
                .iter()
                .position(|fx| fx.kind == FxKind::ColorCorrector)
        });

        if existing_idx.is_none() {
            self.color_grading_empty_state(ui, target, local_t, tol);
            return;
        }

        let Some(fx_idx) = existing_idx else {
            return;
        };
        let Some(chain) = self.fx_chain_mut(target) else {
            return;
        };
        let Some(fx) = chain.get(fx_idx).cloned() else {
            return;
        };

        let mut fx_work = fx;
        let specs = FxKind::ColorCorrector.params();
        let mut vals = fx_work.values(local_t);
        if vals.len() < 6 {
            return;
        }

        let mut modified = false;
        let mut reset_all = false;
        let mut remove_fx = false;
        let mut checkpoint_op = false;

        ui.horizontal_wrapped(|ui| {
            if ui
                .checkbox(&mut fx_work.enabled, "Ativo (Color Corrector)")
                .changed()
            {
                checkpoint_op = true;
                modified = true;
            }
            ui.separator();
            ui.label(egui::RichText::new("Predefinições:").small().weak());
            for preset in GRADING_PRESETS {
                if ui.small_button(preset.label).clicked() {
                    for (i, &pv) in preset.values.iter().enumerate() {
                        if let Some(slot) = vals.get_mut(i) {
                            *slot = pv;
                        }
                    }
                    checkpoint_op = true;
                    modified = true;
                }
            }
            if icons::small_icon_button(ui, Icon::ResetSpin, Some("Zerar"), false, &self.theme)
                .on_hover_text("Restaurar valores neutros de cor")
                .clicked()
            {
                reset_all = true;
            }
            if icons::small_icon_button(ui, Icon::CloseX, Some("Remover"), false, &self.theme)
                .on_hover_text("Remover Color Corrector da cadeia")
                .clicked()
            {
                remove_fx = true;
            }
        });

        if remove_fx {
            self.checkpoint();
            if let Some(c) = self.fx_chain_mut(target) {
                if fx_idx < c.len() {
                    c.remove(fx_idx);
                }
            }
            self.changed();
            self.set_status("Color Corrector removido.");
            return;
        }

        if reset_all {
            if let Some(first) = GRADING_PRESETS.first() {
                for (i, &pv) in first.values.iter().enumerate() {
                    if let Some(slot) = vals.get_mut(i) {
                        *slot = pv;
                    }
                }
                checkpoint_op = true;
                modified = true;
            }
        }

        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    // 3 rodas/dials tonais + pad cromático 2D (100% vetoriais em egui::Painter)
                    let wheel_d = 96.0_f32;
                    if let Some(spec) = specs.first() {
                        modified |= draw_tone_wheel(
                            ui,
                            "tone_lift",
                            "Lift (Sombras)",
                            &mut vals[0],
                            spec.min,
                            spec.max,
                            spec.default,
                            Color32::from_rgb(90, 145, 225),
                            wheel_d,
                            &self.theme,
                        );
                    }
                    if let Some(spec) = specs.get(1) {
                        modified |= draw_tone_wheel(
                            ui,
                            "tone_gamma",
                            "Gamma (Médios)",
                            &mut vals[1],
                            spec.min,
                            spec.max,
                            spec.default,
                            Color32::from_rgb(95, 200, 145),
                            wheel_d,
                            &self.theme,
                        );
                    }
                    if let Some(spec) = specs.get(2) {
                        modified |= draw_tone_wheel(
                            ui,
                            "tone_gain",
                            "Gain (Realces)",
                            &mut vals[2],
                            spec.min,
                            spec.max,
                            spec.default,
                            Color32::from_rgb(235, 175, 80),
                            wheel_d,
                            &self.theme,
                        );
                    }
                    let mut temp = vals[3];
                    let mut tint = vals[4];
                    if draw_balance_pad(ui, &mut temp, &mut tint, wheel_d, &self.theme) {
                        vals[3] = temp;
                        vals[4] = tint;
                        modified = true;
                    }

                    if let Some(sc_tex) = &self.scopes_tex {
                        ui.group(|ui| {
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new("Histograma").small().weak());
                                let s = sc_tex[0].size_vec2();
                                let w = 132.0_f32;
                                let h = (w * s.y / s.x.max(1.0)).clamp(60.0, 96.0);
                                ui.image((sc_tex[0].id(), vec2(w, h)));
                            });
                        });
                    }
                });

                ui.add_space(6.0);
                egui::Grid::new("cg_sliders")
                    .num_columns(3)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        for (i, spec) in specs.iter().enumerate() {
                            let Some(val) = vals.get_mut(i) else {
                                continue;
                            };
                            ui.label(spec.name);
                            let mut cur = *val;
                            let r = ui.add(
                                egui::Slider::new(&mut cur, spec.min..=spec.max).fixed_decimals(2),
                            );
                            if r.changed() && cur.is_finite() {
                                *val = cur.clamp(spec.min, spec.max);
                                modified = true;
                            }
                            if ui
                                .small_button("Padrão")
                                .on_hover_text(format!("Restaurar para {:.2}", spec.default))
                                .clicked()
                            {
                                *val = spec.default;
                                checkpoint_op = true;
                                modified = true;
                            }
                            ui.end_row();
                        }
                    });
            });

        if modified {
            if checkpoint_op {
                self.checkpoint();
            }
            for (i, spec) in specs.iter().enumerate() {
                let Some(&new_val) = vals.get(i) else {
                    continue;
                };
                if let Some(p) = fx_work.params.get_mut(i) {
                    let clamped = if new_val.is_finite() {
                        new_val.clamp(spec.min, spec.max)
                    } else {
                        spec.default
                    };
                    if p.keys.is_empty() {
                        p.value = clamped;
                    } else {
                        p.set_key(local_t, clamped, tol);
                    }
                }
            }
            if let Some(chain) = self.fx_chain_mut(target) {
                if let Some(slot) = chain.get_mut(fx_idx) {
                    *slot = fx_work;
                }
            }
            if checkpoint_op {
                self.changed();
            } else {
                self.dialog_change();
            }
        }
    }

    fn color_grading_empty_state(
        &mut self,
        ui: &mut egui::Ui,
        target: FxTarget,
        _local_t: Ticks,
        _tol: Ticks,
    ) {
        ui.label(
            egui::RichText::new(
                "O alvo selecionado ainda não possui o efeito Corretor de Cor na cadeia.",
            )
            .weak(),
        );
        ui.add_space(6.0);
        let mut chosen_preset: Option<GradingPreset> = None;
        ui.horizontal_wrapped(|ui| {
            if icons::icon_button(
                ui,
                Icon::ColorWheels,
                Some("Ativar Color Grading"),
                true,
                true,
                &self.theme,
            )
            .clicked()
            {
                if let Some(first) = GRADING_PRESETS.first() {
                    chosen_preset = Some(*first);
                }
            }
            ui.label(
                egui::RichText::new("ou iniciar com predefinição:")
                    .small()
                    .weak(),
            );
            for preset in GRADING_PRESETS.iter().skip(1) {
                if ui.button(preset.label).clicked() {
                    chosen_preset = Some(*preset);
                }
            }
        });
        if let Some(preset) = chosen_preset {
            self.checkpoint();
            let mut inst = FxInstance::new(FxKind::ColorCorrector);
            for (i, &pv) in preset.values.iter().enumerate() {
                if let Some(p) = inst.params.get_mut(i) {
                    p.value = pv;
                }
            }
            if let Some(chain) = self.fx_chain_mut(target) {
                chain.push(inst);
            }
            self.changed();
            self.set_status(format!("Color Grading ativado ({})", preset.label));
        }
    }
}

/// Roda vetorial original para parâmetros tonais (`Lift`, `Gamma`, `Gain`).
/// Arraste vertical ou horizontal ajusta o valor; duplo clique restaura `default`.
#[allow(clippy::too_many_arguments)]
fn draw_tone_wheel(
    ui: &mut egui::Ui,
    id_salt: &str,
    title: &str,
    value: &mut f32,
    min_v: f32,
    max_v: f32,
    default_v: f32,
    accent: Color32,
    diameter: f32,
    theme: &ThemeColors,
) -> bool {
    let mut changed = false;
    let d = if diameter.is_finite() {
        diameter.clamp(48.0, 160.0)
    } else {
        88.0
    };
    let (lo, hi) = if min_v <= max_v && min_v.is_finite() && max_v.is_finite() {
        (min_v, max_v)
    } else {
        (0.0, 1.0)
    };
    let span = (hi - lo).max(1e-4);
    let cur = if value.is_finite() {
        (*value).clamp(lo, hi)
    } else {
        default_v.clamp(lo, hi)
    };

    ui.group(|ui| {
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(title).small().strong());
            let (rect, resp) = ui.allocate_exact_size(vec2(d, d), Sense::click_and_drag());
            let id = ui.id().with(id_salt);
            let _ = id;
            if resp.dragged() {
                let delta = resp.drag_delta();
                let step = (delta.x - delta.y) * (span / 140.0);
                let next = (cur + step).clamp(lo, hi);
                if (next - *value).abs() > 1e-5 {
                    *value = next;
                    changed = true;
                }
            }
            if resp.double_clicked() {
                *value = default_v.clamp(lo, hi);
                changed = true;
            }
            if ui.is_rect_visible(rect) {
                paint_tone_wheel_art(ui.painter(), rect, *value, lo, hi, default_v, accent, theme);
            }
            ui.label(
                egui::RichText::new(format!("{:.2}", *value))
                    .small()
                    .monospace(),
            );
        });
    });
    changed
}

#[allow(clippy::too_many_arguments)]
fn paint_tone_wheel_art(
    painter: &egui::Painter,
    rect: Rect,
    value: f32,
    lo: f32,
    hi: f32,
    default_v: f32,
    accent: Color32,
    theme: &ThemeColors,
) {
    if rect.width() <= 8.0 || rect.height() <= 8.0 {
        return;
    }
    let center = rect.center();
    let r = (rect.width().min(rect.height()) * 0.44).max(6.0);
    let ring_col = if theme.high_contrast {
        theme.accent
    } else {
        accent
    };
    let face_col = if theme.high_contrast {
        theme.extreme_bg
    } else {
        theme.panel_fill
    };
    painter.circle_filled(center, r, face_col);
    painter.circle_stroke(center, r, Stroke::new(1.5_f32, theme.border));

    // Marca de referência padrão no topo (12h) e arco proporcional ao valor atual
    let t = if value.is_finite() {
        ((value - lo) / (hi - lo).max(1e-4)).clamp(0.0, 1.0)
    } else {
        0.5
    };
    let t_def = ((default_v - lo) / (hi - lo).max(1e-4)).clamp(0.0, 1.0);
    let ang_min = std::f32::consts::FRAC_PI_2 + 0.55;
    let ang_span = std::f32::consts::TAU - 1.10;
    let cur_ang = ang_min + t * ang_span;
    let def_ang = ang_min + t_def * ang_span;

    // Pontos de arco vetorial (24 segmentos)
    let segments = 24usize;
    let mut track_pts = Vec::with_capacity(segments + 1);
    for i in 0..=segments {
        let a = ang_min + (i as f32 / segments as f32) * ang_span;
        track_pts.push(center + vec2(a.cos(), a.sin()) * (r * 0.78));
    }
    painter.add(egui::Shape::line(
        track_pts,
        Stroke::new(2.2_f32, theme.border.gamma_multiply(0.7)),
    ));

    let fill_segs = ((t * segments as f32).round() as usize).max(1);
    let mut fill_pts = Vec::with_capacity(fill_segs + 1);
    for i in 0..=fill_segs {
        let f = (i as f32 / segments as f32).min(t);
        let a = ang_min + f * ang_span;
        fill_pts.push(center + vec2(a.cos(), a.sin()) * (r * 0.78));
    }
    if fill_pts.len() >= 2 {
        painter.add(egui::Shape::line(fill_pts, Stroke::new(3.0_f32, ring_col)));
    }

    let def_dir = vec2(def_ang.cos(), def_ang.sin());
    painter.line_segment(
        [center + def_dir * (r * 0.64), center + def_dir * (r * 0.92)],
        Stroke::new(1.2_f32, theme.text_muted),
    );

    let knob = center + vec2(cur_ang.cos(), cur_ang.sin()) * (r * 0.78);
    painter.line_segment([center, knob], Stroke::new(1.8_f32, theme.text_primary));
    painter.circle_filled(knob, 4.0_f32, ring_col);
    painter.circle_stroke(knob, 4.0_f32, Stroke::new(1.0_f32, theme.text_primary));
    painter.circle_filled(center, 3.0_f32, theme.text_primary);
}

/// Pad bidimensional vetorial original para Temperatura (X) × Tonalidade (Y).
fn draw_balance_pad(
    ui: &mut egui::Ui,
    temp: &mut f32,
    tint: &mut f32,
    diameter: f32,
    theme: &ThemeColors,
) -> bool {
    let mut changed = false;
    let d = if diameter.is_finite() {
        diameter.clamp(48.0, 160.0)
    } else {
        88.0
    };
    ui.group(|ui| {
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("Temp × Tinta").small().strong());
            let (rect, resp) = ui.allocate_exact_size(vec2(d, d), Sense::click_and_drag());
            let center = rect.center();
            let radius = (rect.width().min(rect.height()) * 0.44).max(6.0);
            if resp.dragged() || resp.clicked() {
                if let Some(pointer) = resp.interact_pointer_pos() {
                    let dx = ((pointer.x - center.x) / radius).clamp(-1.0, 1.0);
                    let dy = (-(pointer.y - center.y) / radius).clamp(-1.0, 1.0);
                    if dx.is_finite() && dy.is_finite() {
                        *temp = dx;
                        *tint = dy;
                        changed = true;
                    }
                }
            }
            if resp.double_clicked() {
                *temp = 0.0;
                *tint = 0.0;
                changed = true;
            }
            if ui.is_rect_visible(rect) {
                paint_balance_pad_art(ui.painter(), rect, *temp, *tint, theme);
            }
            ui.label(
                egui::RichText::new(format!("{:+.2} / {:+.2}", *temp, *tint))
                    .small()
                    .monospace(),
            );
        });
    });
    changed
}

fn paint_balance_pad_art(
    painter: &egui::Painter,
    rect: Rect,
    temp: f32,
    tint: f32,
    theme: &ThemeColors,
) {
    if rect.width() <= 8.0 || rect.height() <= 8.0 {
        return;
    }
    let center = rect.center();
    let r = (rect.width().min(rect.height()) * 0.44).max(6.0);
    let bg = if theme.high_contrast {
        theme.extreme_bg
    } else {
        theme.panel_fill
    };
    painter.circle_filled(center, r, bg);
    if !theme.high_contrast {
        // Pontos de referência de cor nos 4 polos (Frio, Quente, Verde, Magenta)
        painter.circle_filled(
            pos2(center.x - r * 0.68, center.y),
            r * 0.16,
            Color32::from_rgb(70, 145, 240).gamma_multiply(0.55),
        );
        painter.circle_filled(
            pos2(center.x + r * 0.68, center.y),
            r * 0.16,
            Color32::from_rgb(245, 160, 65).gamma_multiply(0.55),
        );
        painter.circle_filled(
            pos2(center.x, center.y - r * 0.68),
            r * 0.16,
            Color32::from_rgb(80, 210, 120).gamma_multiply(0.55),
        );
        painter.circle_filled(
            pos2(center.x, center.y + r * 0.68),
            r * 0.16,
            Color32::from_rgb(220, 95, 195).gamma_multiply(0.55),
        );
    }
    painter.circle_stroke(center, r, Stroke::new(1.5_f32, theme.border));
    painter.line_segment(
        [pos2(center.x - r, center.y), pos2(center.x + r, center.y)],
        Stroke::new(1.0_f32, theme.border.gamma_multiply(0.65)),
    );
    painter.line_segment(
        [pos2(center.x, center.y - r), pos2(center.x, center.y + r)],
        Stroke::new(1.0_f32, theme.border.gamma_multiply(0.65)),
    );

    let tx = if temp.is_finite() {
        temp.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let ty = if tint.is_finite() {
        tint.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let pos = pos2(center.x + tx * r * 0.86, center.y - ty * r * 0.86);
    painter.line_segment([center, pos], Stroke::new(1.5_f32, theme.accent));
    painter.circle_filled(pos, 4.5_f32, theme.accent);
    painter.circle_stroke(pos, 4.5_f32, Stroke::new(1.2_f32, theme.text_primary));
    let _ = (
        Align2::CENTER_CENTER,
        FontId::proportional(10.0),
        StrokeKind::Inside,
        Pos2::ZERO,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{palette, ThemeId};

    #[test]
    fn widgets_color_grading_desenham_em_todos_os_temas_e_dimensoes_hostis() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                for tid in ThemeId::ALL {
                    let th = palette(tid);
                    let mut v = f32::NAN;
                    let _ = draw_tone_wheel(
                        ui,
                        "test_wheel",
                        "Lift",
                        &mut v,
                        -0.5,
                        0.5,
                        0.0,
                        Color32::BLUE,
                        f32::NAN,
                        &th,
                    );
                    let mut temp = f32::INFINITY;
                    let mut tint = f32::NEG_INFINITY;
                    let _ = draw_balance_pad(ui, &mut temp, &mut tint, -10.0, &th);

                    let tiny = Rect::from_min_size(pos2(0.0, 0.0), vec2(2.0, 2.0));
                    paint_tone_wheel_art(
                        ui.painter(),
                        tiny,
                        f32::NAN,
                        -0.5,
                        0.5,
                        0.0,
                        Color32::WHITE,
                        &th,
                    );
                    paint_balance_pad_art(ui.painter(), tiny, f32::NAN, f32::INFINITY, &th);
                }
            });
        });
    }
}
