//! Painel acoplável de Color Grading estilo VEGAS Pro, com controles vetoriais
//! originais (Lift / Gamma / Gain / Balanço Temp × Tinta / Saturação) ligados em
//! tempo real a `FxKind::ColorCorrector` no evento, trilha ou saída Master.

use super::{App, DockTab, FxTarget};
use crate::icons::{self, Icon};
use crate::model::{ColorGradeCurves, FxInstance, FxKind, Ticks, TrackKind};
use crate::theme::ThemeColors;
use eframe::egui;
use egui::{pos2, vec2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind};
use neoveg_common::{tr, tr_args, Language};

/// Seções de trabalho do painel Color Grading (paridade com a barra lateral do VEGAS Pro 21/23).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ColorGradingSection {
    #[default]
    All,
    Wheels,
    Luts,
    Curves,
    Hsl,
}

impl ColorGradingSection {
    pub(crate) const ALL: [Self; 5] =
        [Self::All, Self::Wheels, Self::Luts, Self::Curves, Self::Hsl];

    pub(crate) fn label_in(self, lang: Language) -> &'static str {
        let key = match self {
            Self::All => "Tudo",
            Self::Wheels => "Rodas de Cor",
            Self::Luts => "LUTs",
            Self::Curves => "Curvas",
            Self::Hsl => "HSL",
        };
        tr(lang, key)
    }

    pub(crate) fn show_wheels(self) -> bool {
        matches!(self, Self::All | Self::Wheels)
    }

    pub(crate) fn show_luts(self) -> bool {
        matches!(self, Self::All | Self::Luts)
    }

    pub(crate) fn show_curves(self) -> bool {
        matches!(self, Self::All | Self::Curves)
    }

    pub(crate) fn show_hsl(self) -> bool {
        matches!(self, Self::All | Self::Hsl)
    }
}

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
        let lang = self.lang();
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
                            label: tr_args(lang, "Evento: {name}", &[("name", &ev.name)]),
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
                        label: tr_args(lang, "Trilha: {name}", &[("name", &t.name)]),
                        local_t: self.cursor,
                    };
                }
            }
        }
        ResolvedTarget {
            target: FxTarget::MasterVideo,
            label: tr(lang, "Saída Master de Vídeo").to_string(),
            local_t: self.cursor,
        }
    }

    /// Conteúdo da aba `Color Grading` no painel acoplável / janela flutuante.
    pub(crate) fn color_grading_tab(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let resolved = self.resolve_color_grading_target();
        let target = resolved.target;
        let local_t = resolved.local_t;
        let tol = self.project.settings.frame_dur().div_i64(2);

        ui.horizontal_wrapped(|ui| {
            icons::icon_label(
                ui,
                Icon::ColorWheels,
                15.0,
                tr(lang, "Color Grading"),
                &self.theme,
            );
            ui.separator();
            ui.label(egui::RichText::new(&resolved.label).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::small_icon_button(
                    ui,
                    Icon::ScopesWave,
                    Some(tr(lang, "Escopos")),
                    self.is_panel_open(DockTab::Scopes),
                    &self.theme,
                )
                .on_hover_text(tr(lang, "Abrir painel de Escopos"))
                .clicked()
                {
                    self.exec_cmd(Some(ui.ctx()), "view.tab.scopes");
                }
                if icons::small_icon_button(
                    ui,
                    Icon::FxSparkle,
                    Some(tr(lang, "Cadeia FX...")),
                    self.dlg.fx_target == Some(target),
                    &self.theme,
                )
                .on_hover_text(tr(lang, "Abrir cadeia completa de efeitos do alvo"))
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
                .checkbox(&mut fx_work.enabled, tr(lang, "Ativo (Color Corrector)"))
                .changed()
            {
                checkpoint_op = true;
                modified = true;
            }
            ui.separator();
            ui.label(
                egui::RichText::new(tr(lang, "Predefinições:"))
                    .small()
                    .weak(),
            );
            for preset in GRADING_PRESETS {
                if ui.small_button(tr(lang, preset.label)).clicked() {
                    for (i, &pv) in preset.values.iter().enumerate() {
                        if let Some(slot) = vals.get_mut(i) {
                            *slot = pv;
                        }
                    }
                    checkpoint_op = true;
                    modified = true;
                }
            }
            if icons::small_icon_button(
                ui,
                Icon::ResetSpin,
                Some(tr(lang, "Zerar")),
                false,
                &self.theme,
            )
            .on_hover_text(tr(lang, "Restaurar valores neutros de cor"))
            .clicked()
            {
                reset_all = true;
            }
            if icons::small_icon_button(
                ui,
                Icon::CloseX,
                Some(tr(lang, "Remover")),
                false,
                &self.theme,
            )
            .on_hover_text(tr(lang, "Remover Color Corrector da cadeia"))
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
            self.set_status(tr(lang, "Color Corrector removido."));
            return;
        }

        if reset_all {
            if let Some(first) = GRADING_PRESETS.first() {
                for (i, &pv) in first.values.iter().enumerate() {
                    if let Some(slot) = vals.get_mut(i) {
                        *slot = pv;
                    }
                }
                fx_work.color_grade = None;
                checkpoint_op = true;
                modified = true;
            }
        }

        let sec_id = egui::Id::new("cg_active_section");
        let mut section = ui.ctx().data_mut(|d| {
            d.get_temp::<ColorGradingSection>(sec_id)
                .unwrap_or_default()
        });

        let wheels_active = specs.iter().enumerate().any(|(i, spec)| {
            vals.get(i)
                .is_some_and(|v| v.is_finite() && (*v - spec.default).abs() > 1e-4)
        });
        let lut_active = fx_work
            .color_grade
            .as_ref()
            .is_some_and(|c| c.lut_path.is_some());
        let curves_active = fx_work.color_grade.as_ref().is_some_and(|c| {
            !c.master.is_empty() || !c.red.is_empty() || !c.green.is_empty() || !c.blue.is_empty()
        });
        let hsl_active = fx_work.color_grade.as_ref().is_some_and(|c| {
            c.hsl_bands
                .iter()
                .any(|b| b[0].abs() > 1e-4 || b[1].abs() > 1e-4 || b[2].abs() > 1e-4)
        });

        ui.add_space(4.0);
        let mut render_content = |ui: &mut egui::Ui, active_sec: ColorGradingSection| {
            egui::ScrollArea::vertical()
                .id_salt("cg_content_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if active_sec.show_wheels() {
                        ui.horizontal_wrapped(|ui| {
                            // 3 rodas tonais com anel cromático e barra vertical + pad cromático 2D
                            let wheel_d = 92.0_f32;
                            if let Some(spec) = specs.first() {
                                modified |= draw_tone_wheel(
                                    ui,
                                    lang,
                                    "tone_lift",
                                    tr(lang, "Lift (Sombras)"),
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
                                    lang,
                                    "tone_gamma",
                                    tr(lang, "Gamma (Médios)"),
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
                                    lang,
                                    "tone_gain",
                                    tr(lang, "Gain (Realces)"),
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
                            if draw_balance_pad(
                                ui,
                                lang,
                                &mut temp,
                                &mut tint,
                                wheel_d,
                                &self.theme,
                            ) {
                                vals[3] = temp;
                                vals[4] = tint;
                                modified = true;
                            }

                            if let Some(sc_tex) = &self.scopes_tex {
                                ui.group(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new(tr(lang, "Histograma"))
                                                .small()
                                                .weak(),
                                        );
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
                                    ui.label(spec.name_in(lang));
                                    let mut cur = *val;
                                    let r = ui.add(
                                        egui::Slider::new(&mut cur, spec.min..=spec.max)
                                            .fixed_decimals(2),
                                    );
                                    if r.changed() && cur.is_finite() {
                                        *val = cur.clamp(spec.min, spec.max);
                                        modified = true;
                                    }
                                    let def_str = format!("{:.2}", spec.default);
                                    if ui
                                        .small_button(tr(lang, "Padrão"))
                                        .on_hover_text(tr_args(
                                            lang,
                                            "Restaurar para {val}",
                                            &[("val", &def_str)],
                                        ))
                                        .clicked()
                                    {
                                        *val = spec.default;
                                        checkpoint_op = true;
                                        modified = true;
                                    }
                                    ui.end_row();
                                }
                            });
                    }

                    if active_sec.show_luts() || active_sec.show_curves() || active_sec.show_hsl() {
                        if active_sec.show_wheels() {
                            ui.add_space(6.0);
                            ui.separator();
                        }
                        let mut cg = fx_work.color_grade.take().unwrap_or_default();
                        let (cg_changed, cg_ckpt) =
                            draw_lut_curves_hsl_section(ui, lang, active_sec, &mut cg, &self.theme);
                        if cg_changed {
                            cg.sanitize();
                            modified = true;
                            if cg_ckpt {
                                checkpoint_op = true;
                            }
                        }
                        let cg_empty = cg.lut_path.is_none()
                            && cg.master.is_empty()
                            && cg.red.is_empty()
                            && cg.green.is_empty()
                            && cg.blue.is_empty()
                            && cg
                                .hsl_bands
                                .iter()
                                .all(|b| b[0] == 0.0 && b[1] == 0.0 && b[2] == 0.0);
                        fx_work.color_grade = (!cg_empty).then_some(cg);
                    }
                });
        };

        if ui.available_width() >= 420.0 {
            ui.horizontal_top(|ui| {
                let h = ui.available_height().max(120.0);
                ui.allocate_ui_with_layout(
                    vec2(124.0, h),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        for sec in ColorGradingSection::ALL {
                            let has_edit = match sec {
                                ColorGradingSection::All => {
                                    wheels_active || lut_active || curves_active || hsl_active
                                }
                                ColorGradingSection::Wheels => wheels_active,
                                ColorGradingSection::Luts => lut_active,
                                ColorGradingSection::Curves => curves_active,
                                ColorGradingSection::Hsl => hsl_active,
                            };
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(section == sec, sec.label_in(lang))
                                    .clicked()
                                {
                                    section = sec;
                                }
                                if has_edit {
                                    let (dot_r, _) =
                                        ui.allocate_exact_size(vec2(8.0, 14.0), Sense::hover());
                                    ui.painter().circle_filled(
                                        dot_r.center(),
                                        2.5_f32,
                                        self.theme.accent,
                                    );
                                }
                            });
                        }
                    },
                );
                ui.separator();
                ui.vertical(|ui| {
                    render_content(ui, section);
                });
            });
        } else {
            ui.horizontal_wrapped(|ui| {
                for sec in ColorGradingSection::ALL {
                    if ui
                        .selectable_label(section == sec, sec.label_in(lang))
                        .clicked()
                    {
                        section = sec;
                    }
                }
            });
            ui.separator();
            render_content(ui, section);
        }
        ui.ctx().data_mut(|d| d.insert_temp(sec_id, section));

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
        let lang = self.lang();
        ui.label(
            egui::RichText::new(tr(
                lang,
                "O alvo selecionado ainda não possui o efeito Corretor de Cor na cadeia.",
            ))
            .weak(),
        );
        ui.add_space(6.0);
        let mut chosen_preset: Option<GradingPreset> = None;
        ui.horizontal_wrapped(|ui| {
            if icons::icon_button(
                ui,
                Icon::ColorWheels,
                Some(tr(lang, "Ativar Color Grading")),
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
                egui::RichText::new(tr(lang, "ou iniciar com predefinição:"))
                    .small()
                    .weak(),
            );
            for preset in GRADING_PRESETS.iter().skip(1) {
                if ui.button(tr(lang, preset.label)).clicked() {
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
            let pname = tr(lang, preset.label);
            self.set_status(tr_args(
                lang,
                "Color Grading ativado ({name})",
                &[("name", &pname)],
            ));
        }
    }
}

fn hue_to_rgb(h: f32) -> Color32 {
    let h = if h.is_finite() {
        h.rem_euclid(1.0)
    } else {
        0.0
    };
    let sector = (h * 6.0).floor() as i32;
    let f = h * 6.0 - sector as f32;
    let (r, g, b) = match sector.rem_euclid(6) {
        0 => (1.0, f, 0.0),
        1 => (1.0 - f, 1.0, 0.0),
        2 => (0.0, 1.0, f),
        3 => (0.0, 1.0 - f, 1.0),
        4 => (f, 0.0, 1.0),
        _ => (1.0, 0.0, 1.0 - f),
    };
    Color32::from_rgb(
        (r * 235.0).round() as u8,
        (g * 235.0).round() as u8,
        (b * 235.0).round() as u8,
    )
}

fn paint_chromatic_ring(painter: &egui::Painter, center: Pos2, radius: f32, thickness: f32) {
    const SEGS: usize = 36;
    for i in 0..SEGS {
        let a0 = (i as f32 / SEGS as f32) * std::f32::consts::TAU;
        let a1 = ((i + 1) as f32 / SEGS as f32) * std::f32::consts::TAU;
        let p0 = center + vec2(a0.cos(), a0.sin()) * radius;
        let p1 = center + vec2(a1.cos(), a1.sin()) * radius;
        let col = hue_to_rgb((i as f32 + 0.5) / SEGS as f32);
        painter.line_segment([p0, p1], Stroke::new(thickness, col));
    }
}

/// Roda vetorial original para parâmetros tonais (`Lift`, `Gamma`, `Gain`),
/// com anel cromático, barra vertical de ajuste fino e redefinição individual.
/// Arraste vertical ou horizontal ajusta o valor; duplo clique restaura `default`.
#[allow(clippy::too_many_arguments)]
fn draw_tone_wheel(
    ui: &mut egui::Ui,
    lang: Language,
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
    let (lo, hi) = if min_v < max_v && min_v.is_finite() && max_v.is_finite() {
        (min_v, max_v)
    } else {
        (0.0, 1.0)
    };
    let def = if default_v.is_finite() {
        default_v.clamp(lo, hi)
    } else {
        (lo + hi) * 0.5
    };
    let span = (hi - lo).max(1e-4);
    let cur = if value.is_finite() {
        (*value).clamp(lo, hi)
    } else {
        def
    };

    ui.group(|ui| {
        ui.vertical_centered(|ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(title).small().strong());
                let reset_lbl = tr_args(lang, "Redefinir {name}", &[("name", &title)]);
                let is_modified = (cur - def).abs() > 1e-4;
                if icons::icon_button_named(
                    ui,
                    Icon::ResetSpin,
                    &reset_lbl,
                    false,
                    is_modified,
                    theme,
                )
                .clicked()
                {
                    *value = def;
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                let (rect, resp) = ui.allocate_exact_size(vec2(d, d), Sense::click_and_drag());
                let _ = ui.id().with(id_salt);
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
                    *value = def;
                    changed = true;
                }
                if ui.is_rect_visible(rect) {
                    paint_tone_wheel_art(ui.painter(), rect, *value, lo, hi, def, accent, theme);
                }
                resp.on_hover_text(tr(
                    lang,
                    "Arraste para ajustar; duplo clique restaura o padrão",
                ));
                ui.scope(|ui| {
                    ui.spacing_mut().slider_width = (d - 8.0).max(40.0);
                    let mut sv = if value.is_finite() {
                        (*value).clamp(lo, hi)
                    } else {
                        def
                    };
                    let sr = ui.add(
                        egui::Slider::new(&mut sv, lo..=hi)
                            .vertical()
                            .show_value(false),
                    );
                    if sr.changed() && sv.is_finite() {
                        *value = sv.clamp(lo, hi);
                        changed = true;
                    }
                    if sr.double_clicked() {
                        *value = def;
                        changed = true;
                    }
                });
            });
            let disp = if value.is_finite() {
                (*value).clamp(lo, hi)
            } else {
                def
            };
            ui.label(
                egui::RichText::new(format!("{disp:.2}"))
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
        theme.extreme_bg.gamma_multiply(0.9)
    };
    painter.circle_filled(center, r, face_col);
    if theme.high_contrast {
        painter.circle_stroke(center, r, Stroke::new(1.5_f32, theme.border));
    } else {
        paint_chromatic_ring(painter, center, r, 2.2_f32);
    }

    // Mira central discreta
    let cross_col = theme.border.gamma_multiply(0.5);
    painter.line_segment(
        [
            pos2(center.x - r * 0.56, center.y),
            pos2(center.x + r * 0.56, center.y),
        ],
        Stroke::new(0.8_f32, cross_col),
    );
    painter.line_segment(
        [
            pos2(center.x, center.y - r * 0.56),
            pos2(center.x, center.y + r * 0.56),
        ],
        Stroke::new(0.8_f32, cross_col),
    );

    // Marca de referência padrão e arco proporcional ao valor atual
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
        track_pts.push(center + vec2(a.cos(), a.sin()) * (r * 0.76));
    }
    painter.add(egui::Shape::line(
        track_pts,
        Stroke::new(2.0_f32, theme.border.gamma_multiply(0.7)),
    ));

    let fill_segs = ((t * segments as f32).round() as usize).max(1);
    let mut fill_pts = Vec::with_capacity(fill_segs + 1);
    for i in 0..=fill_segs {
        let f = (i as f32 / segments as f32).min(t);
        let a = ang_min + f * ang_span;
        fill_pts.push(center + vec2(a.cos(), a.sin()) * (r * 0.76));
    }
    if fill_pts.len() >= 2 {
        painter.add(egui::Shape::line(fill_pts, Stroke::new(2.8_f32, ring_col)));
    }

    let def_dir = vec2(def_ang.cos(), def_ang.sin());
    painter.line_segment(
        [center + def_dir * (r * 0.62), center + def_dir * (r * 0.88)],
        Stroke::new(1.2_f32, theme.text_muted),
    );

    let knob = center + vec2(cur_ang.cos(), cur_ang.sin()) * (r * 0.76);
    painter.line_segment([center, knob], Stroke::new(1.8_f32, theme.text_primary));
    painter.circle_filled(knob, 4.0_f32, ring_col);
    painter.circle_stroke(knob, 4.0_f32, Stroke::new(1.0_f32, theme.text_primary));
    painter.circle_filled(center, 2.8_f32, theme.text_primary);
}

/// Pad bidimensional vetorial original para Temperatura (X) × Tonalidade (Y).
fn draw_balance_pad(
    ui: &mut egui::Ui,
    lang: Language,
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
    let cur_t = if temp.is_finite() {
        (*temp).clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let cur_n = if tint.is_finite() {
        (*tint).clamp(-1.0, 1.0)
    } else {
        0.0
    };
    ui.group(|ui| {
        ui.vertical_centered(|ui| {
            let title = tr(lang, "Temp × Tinta");
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(title).small().strong());
                let reset_lbl = tr_args(lang, "Redefinir {name}", &[("name", &title)]);
                let is_modified = cur_t.abs() > 1e-4 || cur_n.abs() > 1e-4;
                if icons::icon_button_named(
                    ui,
                    Icon::ResetSpin,
                    &reset_lbl,
                    false,
                    is_modified,
                    theme,
                )
                .clicked()
                {
                    *temp = 0.0;
                    *tint = 0.0;
                    changed = true;
                }
            });
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
            resp.on_hover_text(tr(
                lang,
                "Arraste para ajustar; duplo clique restaura o padrão",
            ));
            let dt = if temp.is_finite() {
                (*temp).clamp(-1.0, 1.0)
            } else {
                0.0
            };
            let dn = if tint.is_finite() {
                (*tint).clamp(-1.0, 1.0)
            } else {
                0.0
            };
            ui.label(
                egui::RichText::new(format!("{dt:+.2} / {dn:+.2}"))
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
        theme.extreme_bg.gamma_multiply(0.9)
    };
    painter.circle_filled(center, r, bg);
    if theme.high_contrast {
        painter.circle_stroke(center, r, Stroke::new(1.5_f32, theme.border));
    } else {
        // Pontos de referência de cor nos 4 polos (Frio, Quente, Verde, Magenta)
        painter.circle_filled(
            pos2(center.x - r * 0.64, center.y),
            r * 0.15,
            Color32::from_rgb(70, 145, 240).gamma_multiply(0.50),
        );
        painter.circle_filled(
            pos2(center.x + r * 0.64, center.y),
            r * 0.15,
            Color32::from_rgb(245, 160, 65).gamma_multiply(0.50),
        );
        painter.circle_filled(
            pos2(center.x, center.y - r * 0.64),
            r * 0.15,
            Color32::from_rgb(80, 210, 120).gamma_multiply(0.50),
        );
        painter.circle_filled(
            pos2(center.x, center.y + r * 0.64),
            r * 0.15,
            Color32::from_rgb(220, 95, 195).gamma_multiply(0.50),
        );
        paint_chromatic_ring(painter, center, r, 2.2_f32);
    }
    painter.line_segment(
        [
            pos2(center.x - r * 0.92, center.y),
            pos2(center.x + r * 0.92, center.y),
        ],
        Stroke::new(0.9_f32, theme.border.gamma_multiply(0.6)),
    );
    painter.line_segment(
        [
            pos2(center.x, center.y - r * 0.92),
            pos2(center.x, center.y + r * 0.92),
        ],
        Stroke::new(0.9_f32, theme.border.gamma_multiply(0.6)),
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
    let pos = pos2(center.x + tx * r * 0.84, center.y - ty * r * 0.84);
    painter.line_segment([center, pos], Stroke::new(1.5_f32, theme.accent));
    painter.circle_filled(pos, 4.5_f32, theme.accent);
    painter.circle_stroke(pos, 4.5_f32, Stroke::new(1.2_f32, theme.text_primary));
}

fn lut_display_name(lut_path: Option<&str>, lang: Language) -> String {
    let Some(p) = lut_path.filter(|s| !s.trim().is_empty()) else {
        return tr(lang, "Nenhuma").to_string();
    };
    if let Some((_, label)) = ColorGradeCurves::BUILTIN_LUTS
        .iter()
        .find(|(id, _)| *id == p)
    {
        return tr(lang, label).to_string();
    }
    std::path::Path::new(p)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_string())
}

/// Seção de LUT (.cube ou integrada), Curvas RGB e 6 bandas HSL no painel Color Grading.
fn draw_lut_curves_hsl_section(
    ui: &mut egui::Ui,
    lang: Language,
    section: ColorGradingSection,
    cg: &mut ColorGradeCurves,
    theme: &ThemeColors,
) -> (bool, bool) {
    let mut changed = false;
    let mut ckpt = false;
    let mut drew_prev = false;

    if section.show_luts() {
        ui.strong(tr(lang, "LUT de Câmera / Look (.cube)"));
        ui.horizontal_wrapped(|ui| {
            let cur_label = lut_display_name(cg.lut_path.as_deref(), lang);
            egui::ComboBox::from_id_salt("cg_lut_combo")
                .selected_text(cur_label)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(cg.lut_path.is_none(), tr(lang, "Nenhuma"))
                        .clicked()
                    {
                        cg.lut_path = None;
                        changed = true;
                        ckpt = true;
                    }
                    for (id, name) in ColorGradeCurves::BUILTIN_LUTS {
                        let active = cg.lut_path.as_deref() == Some(id);
                        if ui.selectable_label(active, tr(lang, name)).clicked() {
                            cg.lut_path = Some(id.to_string());
                            changed = true;
                            ckpt = true;
                        }
                    }
                });
            if ui
                .button(tr(lang, "Carregar .cube..."))
                .on_hover_text(tr(lang, "Selecionar arquivo de LUT 1D/3D no formato .cube"))
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("LUT Cube", &["cube", "CUBE"])
                    .pick_file()
                {
                    cg.lut_path = Some(path.display().to_string());
                    changed = true;
                    ckpt = true;
                }
            }
            if ui
                .add_enabled(cg.lut_path.is_some(), egui::Button::new(tr(lang, "Limpar")))
                .clicked()
            {
                cg.lut_path = None;
                changed = true;
                ckpt = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label(tr(lang, "Intensidade da LUT"));
            if ui
                .add(egui::Slider::new(&mut cg.lut_strength, 0.0..=1.0).fixed_decimals(2))
                .changed()
            {
                changed = true;
            }
        });
        drew_prev = true;
    }

    if section.show_curves() {
        if drew_prev {
            ui.add_space(4.0);
            ui.separator();
        }
        ui.strong(tr(lang, "Curvas RGB"));
        let ch_id = egui::Id::new("cg_curve_channel_state");
        let mut active_ch = ui
            .ctx()
            .data_mut(|d| d.get_temp::<usize>(ch_id).unwrap_or(0));
        let channels = [
            (0usize, tr(lang, "Master"), theme.text_primary),
            (1, tr(lang, "Vermelho"), Color32::from_rgb(235, 95, 95)),
            (2, tr(lang, "Verde"), Color32::from_rgb(95, 220, 120)),
            (3, tr(lang, "Azul"), Color32::from_rgb(95, 165, 240)),
        ];
        ui.horizontal_wrapped(|ui| {
            for &(ci, name, col) in &channels {
                let text = egui::RichText::new(name).color(if active_ch == ci {
                    theme.accent
                } else {
                    col
                });
                if ui.selectable_label(active_ch == ci, text).clicked() {
                    active_ch = ci;
                }
            }
            if ui.small_button(tr(lang, "Redefinir curva")).clicked() {
                match active_ch {
                    1 => cg.red.clear(),
                    2 => cg.green.clear(),
                    3 => cg.blue.clear(),
                    _ => cg.master.clear(),
                }
                changed = true;
                ckpt = true;
            }
            if ui.small_button(tr(lang, "Zerar curvas")).clicked() {
                cg.master.clear();
                cg.red.clear();
                cg.green.clear();
                cg.blue.clear();
                changed = true;
                ckpt = true;
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(ch_id, active_ch));

        let line_col = channels
            .get(active_ch)
            .map(|c| c.2)
            .unwrap_or(theme.text_primary);
        let curve_pts = match active_ch {
            1 => &mut cg.red,
            2 => &mut cg.green,
            3 => &mut cg.blue,
            _ => &mut cg.master,
        };
        if draw_rgb_curve_editor(ui, lang, curve_pts, line_col, theme) {
            changed = true;
        }
        drew_prev = true;
    }

    if section.show_hsl() {
        if drew_prev {
            ui.add_space(4.0);
            ui.separator();
        }
        ui.strong(tr(lang, "Curvas HSL (6 setores)"));
        let band_id = egui::Id::new("cg_hsl_band_state");
        let mut active_band = ui
            .ctx()
            .data_mut(|d| d.get_temp::<usize>(band_id).unwrap_or(0))
            .min(5);
        let band_colors = [
            Color32::from_rgb(235, 80, 80),
            Color32::from_rgb(235, 210, 75),
            Color32::from_rgb(80, 210, 110),
            Color32::from_rgb(75, 210, 225),
            Color32::from_rgb(85, 135, 235),
            Color32::from_rgb(220, 85, 200),
        ];
        ui.horizontal_wrapped(|ui| {
            for (bi, &name) in ColorGradeCurves::HSL_BAND_NAMES.iter().enumerate() {
                let bcol = band_colors.get(bi).copied().unwrap_or(theme.text_primary);
                let lbl = egui::RichText::new(tr(lang, name)).color(if active_band == bi {
                    theme.accent
                } else {
                    bcol
                });
                if ui.selectable_label(active_band == bi, lbl).clicked() {
                    active_band = bi;
                }
            }
            if ui.small_button(tr(lang, "Zerar HSL")).clicked() {
                cg.hsl_bands.clear();
                changed = true;
                ckpt = true;
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(band_id, active_band));

        let mut band = cg.hsl_bands.get(active_band).copied().unwrap_or([0.0; 3]);
        let mut band_changed = false;
        egui::Grid::new("cg_hsl_sliders")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                ui.label(tr(lang, "Matiz"));
                band_changed |= ui
                    .add(egui::Slider::new(&mut band[0], -0.5..=0.5).fixed_decimals(2))
                    .changed();
                ui.end_row();
                ui.label(tr(lang, "Saturação"));
                band_changed |= ui
                    .add(egui::Slider::new(&mut band[1], -1.0..=1.0).fixed_decimals(2))
                    .changed();
                ui.end_row();
                ui.label(tr(lang, "Luminância"));
                band_changed |= ui
                    .add(egui::Slider::new(&mut band[2], -1.0..=1.0).fixed_decimals(2))
                    .changed();
                ui.end_row();
            });
        if band_changed {
            if cg.hsl_bands.len() < 6 {
                cg.hsl_bands.resize(6, [0.0, 0.0, 0.0]);
            }
            if let Some(slot) = cg.hsl_bands.get_mut(active_band) {
                *slot = band;
            }
            changed = true;
        }
    }

    (changed, ckpt)
}

fn draw_rgb_curve_editor(
    ui: &mut egui::Ui,
    lang: Language,
    pts: &mut Vec<[f32; 2]>,
    line_col: Color32,
    theme: &ThemeColors,
) -> bool {
    let side = ui.available_width().clamp(96.0, 172.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(side, side), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, theme.tl_ruler_bg);
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0_f32, theme.border),
        StrokeKind::Inside,
    );

    for step in 1..4 {
        let f = step as f32 * 0.25;
        let gx = rect.left() + f * rect.width();
        let gy = rect.top() + f * rect.height();
        let grid_stroke = Stroke::new(0.8_f32, theme.border.gamma_multiply(0.45));
        painter.line_segment([pos2(gx, rect.top()), pos2(gx, rect.bottom())], grid_stroke);
        painter.line_segment([pos2(rect.left(), gy), pos2(rect.right(), gy)], grid_stroke);
    }
    painter.line_segment(
        [rect.left_bottom(), rect.right_top()],
        Stroke::new(1.0_f32, theme.border.gamma_multiply(0.7)),
    );

    let drag_id = ui.id().with("cg_curve_drag_idx");
    let mut changed = false;
    let to_screen = |x: f32, y: f32| -> Pos2 {
        let sx = rect.left() + x.clamp(0.0, 1.0) * rect.width();
        let sy = rect.bottom() - y.clamp(0.0, 1.0) * rect.height();
        pos2(sx, sy)
    };
    let to_norm = |p: Pos2| -> [f32; 2] {
        let w = rect.width().max(1.0);
        let h = rect.height().max(1.0);
        let nx = ((p.x - rect.left()) / w).clamp(0.0, 1.0);
        let ny = ((rect.bottom() - p.y) / h).clamp(0.0, 1.0);
        [nx, ny]
    };

    if resp.drag_started() {
        if let Some(pos) = resp.interact_pointer_pos() {
            if pts.is_empty() {
                *pts = ColorGradeCurves::default_curve();
            }
            let hit = pts
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    let d = to_screen(p[0], p[1]).distance(pos);
                    (d <= 14.0).then_some((i, d))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
            let idx = if let Some(i) = hit {
                i
            } else if pts.len() < 32 {
                let np = to_norm(pos);
                pts.push(np);
                pts.len() - 1
            } else {
                0
            };
            ui.ctx().data_mut(|d| d.insert_temp(drag_id, Some(idx)));
        }
    }
    if resp.dragged() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let drag_idx = ui
                .ctx()
                .data_mut(|d| d.get_temp::<Option<usize>>(drag_id).unwrap_or(None));
            if let Some(idx) = drag_idx {
                if let Some(slot) = pts.get_mut(idx) {
                    *slot = to_norm(pos);
                    changed = true;
                }
            }
        }
    } else if resp.drag_stopped() {
        ui.ctx().data_mut(|d| d.insert_temp(drag_id, None::<usize>));
        if !pts.is_empty() {
            pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
            changed = true;
        }
    } else if resp.secondary_clicked() && pts.len() > 2 {
        if let Some(pos) = resp.interact_pointer_pos() {
            if let Some((i, _)) = pts
                .iter()
                .enumerate()
                .filter(|(i, _)| *i > 0 && *i + 1 < pts.len())
                .filter_map(|(i, p)| {
                    let d = to_screen(p[0], p[1]).distance(pos);
                    (d <= 12.0).then_some((i, d))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
            {
                pts.remove(i);
                changed = true;
            }
        }
    }

    let mut sampled = Vec::with_capacity(33);
    for s in 0..=32 {
        let x = s as f32 / 32.0;
        let y = ColorGradeCurves::eval_curve(pts, x);
        sampled.push(to_screen(x, y));
    }
    for w in sampled.windows(2) {
        painter.line_segment([w[0], w[1]], Stroke::new(1.8_f32, line_col));
    }
    let display_pts = if pts.is_empty() {
        vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]]
    } else {
        pts.clone()
    };
    for pt in display_pts {
        let c = to_screen(pt[0], pt[1]);
        painter.circle_filled(c, 4.0, line_col);
        painter.circle_stroke(c, 4.0, Stroke::new(1.0_f32, theme.text_primary));
    }
    resp.on_hover_text(tr(
        lang,
        "Arraste pontos para ajustar (clique em área vazia para adicionar; botão direito remove)",
    ));
    changed
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
                        Language::PtBr,
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
                    let _ = draw_balance_pad(ui, Language::PtBr, &mut temp, &mut tint, -10.0, &th);

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
                    let mut cg = ColorGradeCurves {
                        master: vec![[f32::NAN, f32::INFINITY], [0.5, 0.7]],
                        ..Default::default()
                    };
                    let _ = draw_lut_curves_hsl_section(
                        ui,
                        Language::PtBr,
                        ColorGradingSection::All,
                        &mut cg,
                        &th,
                    );
                }
            });
        });
    }

    #[test]
    fn secoes_color_grading_e_anel_cromatico_resistem_a_entradas_hostis() {
        assert_eq!(ColorGradingSection::ALL.len(), 5);
        assert!(ColorGradingSection::All.show_wheels());
        assert!(ColorGradingSection::All.show_luts());
        assert!(ColorGradingSection::All.show_curves());
        assert!(ColorGradingSection::All.show_hsl());

        assert!(ColorGradingSection::Luts.show_luts());
        assert!(!ColorGradingSection::Luts.show_curves());
        assert!(!ColorGradingSection::Curves.show_wheels());
        assert!(ColorGradingSection::Hsl.show_hsl());

        for sec in ColorGradingSection::ALL {
            assert!(!sec.label_in(Language::PtBr).is_empty());
            assert!(!sec.label_in(Language::En).is_empty());
        }

        // Entradas hostis para hue_to_rgb e lut_display_name não entram em pânico
        let _ = hue_to_rgb(f32::NAN);
        let _ = hue_to_rgb(f32::INFINITY);
        let _ = hue_to_rgb(-3.75);
        assert_eq!(lut_display_name(None, Language::En), "None");
        assert_eq!(
            lut_display_name(Some("builtin:rec709"), Language::En),
            "Camera: Log -> Rec.709"
        );
    }
}
