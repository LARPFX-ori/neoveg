//! Painel estruturado de *Notas do Projeto* (paridade com VEGAS Pro 21/2026):
//! notas vinculadas ao timecode do cursor com etiqueta colorida, data/hora de
//! criação, estado Resolvida/Não resolvida, salto rápido para o timecode,
//! atualização do timecode para o cursor e preservação das anotações livres legadas.

use crate::icons::{self, Icon};
use eframe::egui;
use egui::{Color32, Stroke};
use neoveg_common::{tr, tr_args};
use neoveg_engine::model::{ProjectNote, Ticks};

const DEFAULT_NOTE_COLOR: [u8; 3] = [66, 138, 204];

#[derive(Clone, Debug)]
pub struct ProjectNotesUiState {
    pub hide_resolved: bool,
    pub filter: String,
    pub default_label: String,
    pub default_color: [u8; 3],
    pub show_legacy_notes: bool,
}

impl Default for ProjectNotesUiState {
    fn default() -> Self {
        Self {
            hide_resolved: false,
            filter: String::new(),
            default_label: String::new(),
            default_color: DEFAULT_NOTE_COLOR,
            show_legacy_notes: false,
        }
    }
}

impl crate::App {
    /// Adiciona uma nova nota estruturada ancorada no cursor atual e abre o painel Notas.
    pub(crate) fn add_project_note_at_cursor(&mut self) {
        let lang = self.lang();
        let label = {
            let trimmed = self.dlg.notes_ui.default_label.trim();
            if trimmed.is_empty() {
                tr(lang, "Sem título").to_string()
            } else {
                trimmed.to_string()
            }
        };
        let note = ProjectNote::new(
            label,
            String::new(),
            Some(self.cursor),
            self.dlg.notes_ui.default_color,
        );
        self.checkpoint();
        self.project.project_notes.push(note);
        self.changed();
        self.set_status(tr(lang, "Nota do projeto adicionada no cursor."));
    }

    pub(crate) fn project_notes_tab(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let theme = self.theme.clone();
        let cursor = self.cursor;

        // Barra superior: busca + ocultar resolvidas
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.dlg.notes_ui.filter)
                    .desired_width(140.0)
                    .hint_text(tr(lang, "Filtrar notas...")),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.checkbox(
                    &mut self.dlg.notes_ui.hide_resolved,
                    tr(lang, "Ocultar notas resolvidas"),
                );
            });
        });
        ui.separator();

        // Rodapé reservado para criação rápida de nota
        let footer_h = 34.0;
        let list_h = (ui.available_height() - footer_h - 6.0).max(80.0);

        let filter_low = self.dlg.notes_ui.filter.trim().to_ascii_lowercase();
        let hide_resolved = self.dlg.notes_ui.hide_resolved;

        let mut jump_to: Option<Ticks> = None;
        let mut remove_idx: Option<usize> = None;
        let mut mutated = false;

        egui::ScrollArea::vertical()
            .id_salt("project_notes_scroll")
            .max_height(list_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.project.project_notes.is_empty() {
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(tr(
                            lang,
                            "Nenhuma nota no projeto. Clique em \"Adicionar nova nota\" abaixo para criar uma anotação vinculada ao timecode atual.",
                        ))
                        .color(theme.text_muted),
                    );
                } else {
                    let count = self.project.project_notes.len();
                    for idx in 0..count {
                        let (resolved, matches_filter) = {
                            let n = &self.project.project_notes[idx];
                            let m = filter_low.is_empty()
                                || n.label.to_ascii_lowercase().contains(&filter_low)
                                || n.text.to_ascii_lowercase().contains(&filter_low);
                            (n.resolved, m)
                        };
                        if (hide_resolved && resolved) || !matches_filter {
                            continue;
                        }

                        let tc_opt = self.project.project_notes[idx].timecode;
                        let tc_text = tc_opt
                            .map(|t| self.fmt_time(t))
                            .unwrap_or_else(|| "--:--:--.--".to_string());
                        let note_col = {
                            let c = self.project.project_notes[idx].color;
                            Color32::from_rgb(c[0], c[1], c[2])
                        };
                        let pill_fg = crate::theme::contrast_on(note_col);
                        let created = self.project.project_notes[idx].created_at.clone();

                        egui::Frame::new()
                            .fill(if resolved {
                                theme.panel_fill
                            } else {
                                theme.widget_bg
                            })
                            .stroke(Stroke::new(
                                1.0_f32,
                                if resolved {
                                    theme.border.gamma_multiply(0.6)
                                } else {
                                    theme.border
                                },
                            ))
                            .corner_radius(2.0)
                            .inner_margin(egui::Margin::same(5))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Etiqueta colorida + data/hora
                                    egui::Frame::new()
                                        .fill(note_col)
                                        .stroke(Stroke::new(1.0_f32, theme.border))
                                        .corner_radius(2.0)
                                        .inner_margin(egui::Margin::symmetric(5, 1))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                let lbl_resp = ui.add(
                                                    egui::TextEdit::singleline(
                                                        &mut self.project.project_notes[idx].label,
                                                    )
                                                    .frame(false)
                                                    .desired_width(110.0)
                                                    .text_color(pill_fg),
                                                );
                                                if lbl_resp.changed() {
                                                    mutated = true;
                                                }
                                                if !created.is_empty() {
                                                    ui.label(
                                                        egui::RichText::new(&created)
                                                            .small()
                                                            .color(pill_fg.gamma_multiply(0.85)),
                                                    );
                                                }
                                            });
                                        });

                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if icons::small_icon_button(
                                                ui,
                                                Icon::CloseX,
                                                None,
                                                false,
                                                &theme,
                                            )
                                            .on_hover_text(tr(lang, "Excluir nota"))
                                            .clicked()
                                            {
                                                remove_idx = Some(idx);
                                            }
                                            let res_tip = if resolved {
                                                tr(lang, "Marcar nota como não resolvida")
                                            } else {
                                                tr(lang, "Marcar nota como resolvida")
                                            };
                                            if icons::small_icon_button(
                                                ui,
                                                Icon::NoteResolved,
                                                None,
                                                resolved,
                                                &theme,
                                            )
                                            .on_hover_text(res_tip)
                                            .clicked()
                                            {
                                                self.project.project_notes[idx].resolved =
                                                    !resolved;
                                                mutated = true;
                                            }
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        egui::RichText::new(&tc_text)
                                                            .monospace()
                                                            .small(),
                                                    )
                                                    .frame(false),
                                                )
                                                .on_hover_text(tr(
                                                    lang,
                                                    "Ir para o timecode desta nota",
                                                ))
                                                .clicked()
                                            {
                                                if let Some(t) = tc_opt {
                                                    jump_to = Some(t);
                                                }
                                            }
                                            if icons::small_icon_button(
                                                ui,
                                                Icon::NoteJump,
                                                None,
                                                false,
                                                &theme,
                                            )
                                            .on_hover_text(tr(
                                                lang,
                                                "Ir para o timecode desta nota",
                                            ))
                                            .clicked()
                                            {
                                                if let Some(t) = tc_opt {
                                                    jump_to = Some(t);
                                                }
                                            }
                                            if icons::small_icon_button(
                                                ui,
                                                Icon::InsertMarker,
                                                None,
                                                false,
                                                &theme,
                                            )
                                            .on_hover_text(tr(
                                                lang,
                                                "Atualizar timecode para a posição atual do cursor",
                                            ))
                                            .clicked()
                                            {
                                                self.project.project_notes[idx].timecode =
                                                    Some(cursor);
                                                mutated = true;
                                            }
                                        },
                                    );
                                });
                                ui.add_space(3.0);
                                let body_resp = ui.add(
                                    egui::TextEdit::multiline(
                                        &mut self.project.project_notes[idx].text,
                                    )
                                    .desired_rows(2)
                                    .desired_width(f32::INFINITY)
                                    .hint_text(tr(lang, "Digite o lembrete ou observação...")),
                                );
                                if body_resp.changed() {
                                    mutated = true;
                                }
                            });
                        ui.add_space(4.0);
                    }
                }

                // Compatibilidade com anotações livres de projetos anteriores
                if !self.project.notes.is_empty() || self.dlg.notes_ui.show_legacy_notes {
                    ui.add_space(4.0);
                    egui::CollapsingHeader::new(tr(lang, "Anotações livres do projeto"))
                        .default_open(!self.project.notes.is_empty())
                        .show(ui, |ui| {
                            if ui
                                .add(
                                    egui::TextEdit::multiline(&mut self.project.notes)
                                        .desired_rows(4)
                                        .desired_width(f32::INFINITY),
                                )
                                .changed()
                            {
                                mutated = true;
                            }
                        });
                }
            });

        if let Some(idx) = remove_idx {
            if idx < self.project.project_notes.len() {
                self.checkpoint();
                self.project.project_notes.remove(idx);
                self.changed();
            }
        } else if mutated {
            self.dirty = true;
        }
        if let Some(t) = jump_to {
            self.set_cursor(t);
            let tc_s = self.fmt_time(t);
            self.set_status(tr_args(
                lang,
                "Cursor movido para a nota ({tc})",
                &[("tc", &tc_s)],
            ));
        }

        ui.separator();
        // Barra inferior: seletor de cor + etiqueta padrão + botão Adicionar nova nota
        ui.horizontal(|ui| {
            ui.color_edit_button_srgb(&mut self.dlg.notes_ui.default_color)
                .on_hover_text(tr(lang, "Cor da nova nota"));

            ui.add(
                egui::TextEdit::singleline(&mut self.dlg.notes_ui.default_label)
                    .desired_width(150.0)
                    .hint_text(tr(lang, "Etiqueta padrão")),
            );

            if icons::icon_button(
                ui,
                Icon::NoteAdd,
                Some(tr(lang, "Adicionar nova nota")),
                false,
                true,
                &theme,
            )
            .clicked()
            {
                self.add_project_note_at_cursor();
            }
        });
    }
}
