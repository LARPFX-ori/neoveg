//! Linhas de quadros-chave de trilha na linha do tempo.
//!
//! Comportamento do manual do VEGAS Pro 21 (p. 389): os quadros-chave dos
//! efeitos de vídeo da trilha e de Track Motion ficam abaixo da trilha de
//! vídeo; arrastar muda a posição; o menu de contexto adiciona, apaga e muda
//! a interpolação; o botão no cabeçalho expande ou recolhe a área. As regras
//! de edição ficam em `neoveg_engine::track_keyframes`; aqui só desenho,
//! teste de clique e Desfazer.

use super::{App, CtxTarget, TimelineRows, KF_ROW_H};
use crate::icons::{self, Icon};
use crate::model::*;
use eframe::egui;
use egui::{pos2, vec2, Align2, FontId, Pos2, Rect, Stroke};
use neoveg_common::tr;
use neoveg_engine::track_keyframes::{self as tkf, KfRow, KfRowKind};

/// Área de quadros-chave abaixo de uma trilha.
pub(super) struct KfArea {
    pub top: f32,
    pub rows: Vec<KfRow>,
}

/// Quadro-chave sob o ponteiro: trilha, linha, quadro-chave (se perto de
/// um) e tempo do ponteiro.
pub(super) type KfHit = (usize, KfRow, Option<Ticks>, Ticks);

impl App {
    /// Meio quadro do projeto: quadros-chave mais próximos que isso contam
    /// como o mesmo.
    pub(super) fn kf_tol(&self) -> Ticks {
        let f = self.project.settings.fps.ticks_per_frame();
        Ticks::from_raw((f.0 / 2).max(1))
    }

    /// Linhas visíveis de uma trilha (nenhuma se a área está recolhida).
    pub(super) fn kf_visible_rows(&self, t: &Track) -> Vec<KfRow> {
        if self.tl.kf_collapsed.contains(&t.id) {
            return vec![];
        }
        tkf::track_rows_in(t, self.kf_tol(), self.lang())
    }

    pub(super) fn hit_keyframe(&self, pos: Pos2, left: f32, rows: &TimelineRows) -> Option<KfHit> {
        for (ti, area) in rows.kf_areas.iter().enumerate() {
            let Some(area) = area else {
                continue;
            };
            let bottom = area.top + area.rows.len() as f32 * KF_ROW_H;
            if pos.y < area.top || pos.y >= bottom {
                continue;
            }
            let ri = ((pos.y - area.top) / KF_ROW_H).floor().max(0.0) as usize;
            let row = area.rows.get(ri)?.clone();
            let t = self.tl.x2t(pos.x, left).max(Ticks::ZERO);
            let key = row
                .times
                .iter()
                .copied()
                .find(|k| (self.tl.t2x(*k, left) - pos.x).abs() <= 6.0);
            return Some((ti, row, key, t));
        }
        None
    }

    pub(super) fn keyframe_ctx(
        &self,
        pos: Pos2,
        left: f32,
        rows: &TimelineRows,
    ) -> Option<CtxTarget> {
        let (ti, row, key, t) = self.hit_keyframe(pos, left, rows)?;
        let track = self.project.tracks.get(ti)?.id;
        Some(CtxTarget::Keyframe {
            track,
            row: row.kind,
            key,
            t,
            has_curve: row.has_curve,
        })
    }

    /// Aplica `f` numa cópia da trilha e só grava (com Desfazer) se algo
    /// mudou. Trilhas travadas não são editadas.
    fn edit_track_keyframes(&mut self, track: Id, f: impl FnOnce(&mut Track) -> bool) -> bool {
        let Some(i) = self.project.tracks.iter().position(|x| x.id == track) else {
            return false;
        };
        let Some(tr) = self.project.tracks.get(i).filter(|t| !t.locked) else {
            return false;
        };
        let mut probe = tr.clone();
        if !f(&mut probe) {
            return false;
        }
        self.checkpoint();
        if let Some(slot) = self.project.tracks.get_mut(i) {
            *slot = probe;
        }
        self.changed();
        true
    }

    /// Um passo do arraste de um quadro-chave. Grava o Desfazer só no
    /// primeiro movimento de verdade (`moved == false`). Devolve o novo tempo
    /// do quadro-chave, ou `None` se ele não andou.
    pub(super) fn drag_keyframe(
        &mut self,
        track: Id,
        row: &KfRowKind,
        at: Ticks,
        moved: bool,
        target: Ticks,
    ) -> Option<Ticks> {
        if target == at {
            return None;
        }
        let tol = self.kf_tol();
        let i = self.project.tracks.iter().position(|x| x.id == track)?;
        let tr = self.project.tracks.get(i).filter(|t| !t.locked)?;
        let mut probe = tr.clone();
        let new_at = tkf::move_key(&mut probe, row, at, target, tol)?;
        if new_at == at {
            return None;
        }
        if !moved {
            self.checkpoint();
        }
        if let Some(slot) = self.project.tracks.get_mut(i) {
            *slot = probe;
        }
        self.changed();
        Some(new_at)
    }

    pub(super) fn draw_keyframe_rows(&self, ui: &egui::Ui, lanes: Rect, rows: &TimelineRows) {
        let painter = ui.painter_at(lanes);
        let left = lanes.left();
        for area in rows.kf_areas.iter().flatten() {
            for (ri, row) in area.rows.iter().enumerate() {
                let y0 = area.top + ri as f32 * KF_ROW_H;
                let r =
                    Rect::from_min_max(pos2(lanes.left(), y0), pos2(lanes.right(), y0 + KF_ROW_H));
                if r.bottom() < lanes.top() || r.top() > lanes.bottom() {
                    continue;
                }
                painter.rect_filled(r, 0.0, self.theme.tl_header_idle);
                painter.line_segment(
                    [r.left_bottom(), r.right_bottom()],
                    Stroke::new(1.0_f32, self.theme.border),
                );
                let cy = r.center().y;
                if let (Some(a), Some(b)) = (row.times.first(), row.times.last()) {
                    painter.line_segment(
                        [
                            pos2(self.tl.t2x(*a, left), cy),
                            pos2(self.tl.t2x(*b, left), cy),
                        ],
                        Stroke::new(1.0_f32, self.theme.text_muted),
                    );
                }
                for k in &row.times {
                    let x = self.tl.t2x(*k, left);
                    if x < lanes.left() - 8.0 || x > lanes.right() + 8.0 {
                        continue;
                    }
                    let kr = Rect::from_center_size(pos2(x, cy), vec2(11.0, 11.0));
                    icons::paint(&painter, kr, Icon::KeyframeFilled, &self.theme, true);
                }
            }
        }
    }

    /// Nome de cada linha na coluna de cabeçalhos, ao lado da área.
    pub(super) fn draw_keyframe_labels(&self, ui: &egui::Ui, headers: Rect, rows: &TimelineRows) {
        let painter = ui.painter_at(headers);
        for area in rows.kf_areas.iter().flatten() {
            for (ri, row) in area.rows.iter().enumerate() {
                let y0 = area.top + ri as f32 * KF_ROW_H;
                let r = Rect::from_min_max(
                    pos2(headers.left(), y0),
                    pos2(headers.right(), y0 + KF_ROW_H),
                );
                if r.bottom() < headers.top() || r.top() > headers.bottom() {
                    continue;
                }
                painter.rect_filled(r, 0.0, self.theme.tl_header_idle);
                painter.line_segment(
                    [r.left_bottom(), r.right_bottom()],
                    Stroke::new(1.0_f32, self.theme.border),
                );
                let ir =
                    Rect::from_center_size(pos2(r.left() + 18.0, r.center().y), vec2(10.0, 10.0));
                icons::paint(&painter, ir, Icon::KeyframeEmpty, &self.theme, true);
                painter.text(
                    pos2(r.left() + 28.0, r.center().y),
                    Align2::LEFT_CENTER,
                    &row.label,
                    FontId::proportional(10.5),
                    self.theme.text_muted,
                );
            }
        }
    }

    pub(super) fn keyframe_context_menu(
        &mut self,
        ui: &mut egui::Ui,
        track: Id,
        row: KfRowKind,
        key: Option<Ticks>,
        t: Ticks,
        has_curve: bool,
    ) {
        let lang = self.lang();
        let tol = self.kf_tol();
        ui.label(egui::RichText::new(tr(lang, "Quadro-chave da trilha")).strong());
        if let Some(k) = key {
            ui.label(self.fmt_time(k));
        }
        ui.separator();
        if ui.button(tr(lang, "Adicionar quadro-chave")).clicked() {
            let r = row.clone();
            let at = key.unwrap_or(t);
            self.edit_track_keyframes(track, |tr| tkf::add_key(tr, &r, at, tol));
            ui.close_menu();
        }
        let del = ui.add_enabled(
            key.is_some(),
            egui::Button::new(tr(lang, "Excluir quadro-chave")),
        );
        let del = if row == KfRowKind::Motion {
            del.on_disabled_hover_text(tr(lang, "Clique sobre um quadro-chave para excluí-lo."))
                .on_hover_text(tr(
                    lang,
                    "O Movimento da trilha sempre mantém pelo menos um quadro-chave.",
                ))
        } else {
            del.on_disabled_hover_text(tr(lang, "Clique sobre um quadro-chave para excluí-lo."))
        };
        if del.clicked() {
            if let Some(k) = key {
                let r = row.clone();
                self.edit_track_keyframes(track, |tr| tkf::delete_key(tr, &r, k, tol));
            }
            ui.close_menu();
        }
        ui.separator();
        match (has_curve, key) {
            (true, Some(k)) => {
                let track_ref = self.project.tracks.iter().find(|x| x.id == track);
                let current_ease = track_ref.and_then(|tr| tkf::key_ease(tr, &row, k, tol));
                let current = track_ref
                    .and_then(|tr| tkf::key_curve(tr, &row, k, tol))
                    .filter(|_| current_ease.is_none());
                ui.label(egui::RichText::new(tr(lang, "Interpolação até o próximo")).small());
                for c in Curve::ALL {
                    if ui.radio(current == Some(c), c.name_in(lang)).clicked() {
                        let r = row.clone();
                        self.edit_track_keyframes(track, |tr| {
                            tkf::set_key_curve(tr, &r, k, c, tol)
                        });
                        ui.close_menu();
                    }
                }
                ui.separator();
                if let Some(e) = crate::velocity_graph::bezier_menu_items(ui, current_ease, lang) {
                    let r = row.clone();
                    self.edit_track_keyframes(track, |tr| {
                        tkf::set_key_ease(tr, &r, k, Some(e), tol)
                    });
                    ui.close_menu();
                }
            }
            (true, None) => {
                let _ = ui
                    .add_enabled(false, egui::Button::new(tr(lang, "Interpolação")))
                    .on_disabled_hover_text(tr(
                        lang,
                        "Clique sobre um quadro-chave para mudar a curva.",
                    ));
            }
            (false, _) => {
                let _ = ui
                    .add_enabled(false, egui::Button::new(tr(lang, "Interpolação")))
                    .on_disabled_hover_text(tr(
                        lang,
                        "Esta linha de quadros-chave não suporta curvas de interpolação.",
                    ));
            }
        }
    }
}
