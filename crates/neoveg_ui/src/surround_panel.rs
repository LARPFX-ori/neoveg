//! Painel acoplável *Surround Panner* 5.1: escolhe o modo do Master (estéreo ou
//! 5.1) e posiciona trilhas de áudio e barramentos num pad 2D da sala, com os
//! alto-falantes nos ângulos da recomendação ITU-R BS.775. Arte vetorial
//! original desenhada com `egui::Painter`.

use super::App;
use crate::model::{ChannelLayout, Id, SurroundPan, TrackKind};
use eframe::egui;
use egui::{pos2, vec2, Align2, FontId, Pos2, Rect, Sense, Stroke, StrokeKind};
use neoveg_common::{tr, tr_args};

/// Origem do sinal posicionada no pad.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SurroundTarget {
    Track(Id),
    Bus(Id),
}

/// Rótulo e ângulo (graus, 0 = frente, positivo à direita) de cada alto-falante.
const SPEAKERS: [(&str, f32); 5] = [
    ("L", -30.0),
    ("C", 0.0),
    ("R", 30.0),
    ("Ls", -110.0),
    ("Rs", 110.0),
];

const PAD_SIDE: f32 = 180.0;

/// Converte um ponto do pad (`center`, raio `radius`) em posição da sala,
/// limitada ao círculo unitário. Raio nulo devolve o centro da sala.
pub(crate) fn pad_to_pan(p: Pos2, center: Pos2, radius: f32, base: SurroundPan) -> SurroundPan {
    if radius.is_nan() || radius <= 0.0 {
        return base;
    }
    let mut x = (p.x - center.x) / radius;
    let mut y = (p.y - center.y) / radius;
    if !x.is_finite() || !y.is_finite() {
        return base;
    }
    let r = (x * x + y * y).sqrt();
    if r > 1.0 {
        x /= r;
        y /= r;
    }
    SurroundPan { x, y, ..base }.sanitized()
}

impl App {
    pub(crate) fn surround_panner_tab(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let layout = self.project.settings.channel_layout;
        ui.horizontal(|ui| {
            ui.label(tr(lang, "Modo do Master:"));
            let mut sel = layout;
            egui::ComboBox::from_id_salt("surround_layout")
                .selected_text(sel.name_in(lang))
                .show_ui(ui, |ui| {
                    for l in ChannelLayout::ALL {
                        ui.selectable_value(&mut sel, l, l.name_in(lang));
                    }
                });
            if sel != layout {
                self.checkpoint();
                self.project.settings.channel_layout = sel;
                self.changed();
            }
        });
        if self.project.settings.channel_layout != ChannelLayout::Surround51 {
            ui.label(
                egui::RichText::new(tr(
                    lang,
                    "O projeto está em estéreo. Escolha 5.1 Surround no modo do Master \
                     para posicionar trilhas e barramentos na sala.",
                ))
                .small()
                .color(self.theme.text_muted),
            );
            return;
        }

        let mut choices: Vec<(SurroundTarget, String)> = self
            .project
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Audio)
            .map(|t| (SurroundTarget::Track(t.id), t.name.clone()))
            .collect();
        choices.extend(
            self.project
                .audio_buses
                .iter()
                .filter(|b| b.output_bus.is_none())
                .map(|b| {
                    (
                        SurroundTarget::Bus(b.id),
                        tr_args(lang, "Barramento: {name}", &[("name", &b.name)]),
                    )
                }),
        );
        let Some(&(first, _)) = choices.first() else {
            ui.label(tr(lang, "Não há trilhas de áudio para posicionar."));
            return;
        };
        let key = egui::Id::new("surround_panner_target");
        let stored: Option<SurroundTarget> = ui.data(|d| d.get_temp(key));
        let mut target = stored
            .filter(|t| choices.iter().any(|(c, _)| c == t))
            .unwrap_or(first);
        let sel_name = choices
            .iter()
            .find(|(c, _)| *c == target)
            .map(|(_, n)| n.clone())
            .unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(tr(lang, "Origem:"));
            egui::ComboBox::from_id_salt("surround_target")
                .selected_text(sel_name)
                .show_ui(ui, |ui| {
                    for (c, name) in &choices {
                        ui.selectable_value(&mut target, *c, name);
                    }
                });
        });
        ui.data_mut(|d| d.insert_temp(key, target));

        let Some(before) = self.surround_of(target) else {
            return;
        };
        let mut pan = before;
        let mut want_checkpoint = false;
        ui.horizontal(|ui| {
            let (resp, painter) =
                ui.allocate_painter(vec2(PAD_SIDE, PAD_SIDE), Sense::click_and_drag());
            let rect = resp.rect;
            let center = rect.center();
            let radius = PAD_SIDE * 0.5 - 14.0;
            self.paint_surround_pad(&painter, rect, center, radius, pan);
            if resp.drag_started() || resp.clicked() {
                want_checkpoint = true;
            }
            if resp.dragged() || resp.clicked() {
                if let Some(p) = resp.interact_pointer_pos() {
                    pan = pad_to_pan(p, center, radius, pan);
                }
            }
            resp.on_hover_text(tr(
                lang,
                "Arraste para posicionar a origem na sala (frente no topo)",
            ));

            ui.vertical(|ui| {
                if ui
                    .checkbox(&mut pan.use_center, tr(lang, "Usar canal central"))
                    .on_hover_text(tr(
                        lang,
                        "Sem o central, a frente vira uma imagem fantasma entre L e R",
                    ))
                    .changed()
                {
                    want_checkpoint = true;
                }
                ui.label(tr(lang, "LFE:"));
                let lfe = ui.add(egui::Slider::new(&mut pan.lfe, 0.0..=1.0).fixed_decimals(2));
                if lfe.drag_started() || (lfe.changed() && !lfe.dragged()) {
                    want_checkpoint = true;
                }
                ui.label(
                    egui::RichText::new(format!("X {:+.2}   Y {:+.2}", pan.x, pan.y))
                        .small()
                        .monospace()
                        .color(self.theme.text_muted),
                );
                if ui.button(tr(lang, "Frente central")).clicked() {
                    want_checkpoint = true;
                    pan = SurroundPan {
                        use_center: pan.use_center,
                        lfe: pan.lfe,
                        ..SurroundPan::default()
                    };
                }
            });
        });
        if pan != before {
            if want_checkpoint {
                self.checkpoint();
            }
            self.set_surround(target, pan);
            self.changed();
        }
    }

    fn paint_surround_pad(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        center: Pos2,
        radius: f32,
        pan: SurroundPan,
    ) {
        let th = &self.theme;
        painter.rect_filled(rect, 4.0, th.extreme_bg);
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0_f32, th.border),
            StrokeKind::Inside,
        );
        painter.circle_stroke(center, radius, Stroke::new(1.0_f32, th.border));
        painter.line_segment(
            [
                pos2(center.x - radius, center.y),
                pos2(center.x + radius, center.y),
            ],
            Stroke::new(1.0_f32, th.border.gamma_multiply(0.5)),
        );
        painter.line_segment(
            [
                pos2(center.x, center.y - radius),
                pos2(center.x, center.y + radius),
            ],
            Stroke::new(1.0_f32, th.border.gamma_multiply(0.5)),
        );
        for (label, deg) in SPEAKERS {
            if label == "C" && !pan.use_center {
                continue;
            }
            let a = deg.to_radians();
            let p = pos2(center.x + radius * a.sin(), center.y - radius * a.cos());
            let r = Rect::from_center_size(p, vec2(16.0, 12.0));
            painter.rect_filled(r, 2.0, th.selection_bg);
            painter.rect_stroke(
                r,
                2.0,
                Stroke::new(1.0_f32, th.text_muted),
                StrokeKind::Inside,
            );
            painter.text(
                p,
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(9.0),
                th.text_primary,
            );
        }
        let p = pan.sanitized();
        let dot = pos2(center.x + radius * p.x, center.y + radius * p.y);
        painter.line_segment(
            [center, dot],
            Stroke::new(1.0_f32, th.accent.gamma_multiply(0.5)),
        );
        painter.circle_filled(dot, 6.0, th.accent);
        painter.circle_stroke(dot, 6.0, Stroke::new(1.0_f32, th.text_primary));
    }

    pub(crate) fn surround_of(&self, t: SurroundTarget) -> Option<SurroundPan> {
        match t {
            SurroundTarget::Track(id) => self
                .project
                .tracks
                .iter()
                .find(|x| x.id == id)
                .map(|x| x.surround),
            SurroundTarget::Bus(id) => self
                .project
                .audio_buses
                .iter()
                .find(|b| b.id == id)
                .map(|b| b.surround),
        }
    }

    fn set_surround(&mut self, t: SurroundTarget, pan: SurroundPan) {
        let pan = pan.sanitized();
        match t {
            SurroundTarget::Track(id) => {
                if let Some(x) = self.project.tracks.iter_mut().find(|x| x.id == id) {
                    x.surround = pan;
                }
            }
            SurroundTarget::Bus(id) => {
                if let Some(b) = self.project.audio_buses.iter_mut().find(|b| b.id == id) {
                    b.surround = pan;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_maps_top_to_front_and_clamps_outside() {
        let c = pos2(100.0, 100.0);
        let p = pad_to_pan(pos2(100.0, 20.0), c, 80.0, SurroundPan::default());
        assert!((p.y + 1.0).abs() < 1e-5 && p.x.abs() < 1e-5);
        let far = pad_to_pan(pos2(500.0, 100.0), c, 80.0, SurroundPan::default());
        assert!((far.x - 1.0).abs() < 1e-5 && far.y.abs() < 1e-5);
    }

    #[test]
    fn pad_hostile_input_keeps_base() {
        let base = SurroundPan::default();
        let c = pos2(0.0, 0.0);
        assert_eq!(pad_to_pan(pos2(5.0, 5.0), c, 0.0, base), base);
        assert_eq!(pad_to_pan(pos2(f32::NAN, 5.0), c, 10.0, base), base);
        assert_eq!(pad_to_pan(pos2(5.0, 5.0), c, f32::NAN, base), base);
    }
}
