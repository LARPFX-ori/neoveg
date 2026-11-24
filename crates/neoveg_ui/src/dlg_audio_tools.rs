//! Diálogos das ferramentas avançadas de áudio (VEGAS Pro 2026):
//! - **Auto Ducking** (redução automática de volume de trilhas alvo durante fala);
//! - **Detecção de Batidas e Andamento (BPM)** (análise de evento de áudio + marcadores).

use crate::icons::{self, Icon};
use eframe::egui;
use neoveg_common::{tr, tr_args};
use neoveg_engine::engine::audio_tools::{
    apply_auto_ducking, apply_tempo_detection, detect_tempo_for_event, AutoDuckingSpec,
    TempoDetectionResult,
};
use neoveg_engine::model::TrackKind;

pub struct AudioToolsUiState {
    pub ducking_open: bool,
    pub ducking_trigger: usize,
    pub ducking_targets: Vec<usize>,
    pub ducking_threshold_db: f32,
    pub ducking_attenuation_db: f32,
    pub ducking_attack_ms: u32,
    pub ducking_release_ms: u32,
    pub ducking_hold_ms: u32,

    pub tempo_open: bool,
    pub tempo_result: Option<TempoDetectionResult>,
    pub tempo_update_bpm: bool,
    pub tempo_insert_markers: bool,
    pub tempo_every_nth_beat: usize,
}

impl Default for AudioToolsUiState {
    fn default() -> Self {
        Self {
            ducking_open: false,
            ducking_trigger: 0,
            ducking_targets: Vec::new(),
            ducking_threshold_db: -24.0,
            ducking_attenuation_db: -12.0,
            ducking_attack_ms: 80,
            ducking_release_ms: 350,
            ducking_hold_ms: 180,
            tempo_open: false,
            tempo_result: None,
            tempo_update_bpm: true,
            tempo_insert_markers: true,
            tempo_every_nth_beat: 4,
        }
    }
}

pub fn show_audio_tools_windows(app: &mut crate::App, ctx: &egui::Context) {
    show_auto_ducking_dialog(app, ctx);
    show_beat_detection_dialog(app, ctx);
}

fn show_auto_ducking_dialog(app: &mut crate::App, ctx: &egui::Context) {
    if !app.dlg.audio_tools.ducking_open {
        return;
    }
    let lang = app.lang();
    let theme = app.theme.clone();
    let audio_tracks: Vec<(usize, String)> = app
        .session
        .project
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.kind == TrackKind::Audio && !t.is_folder)
        .map(|(i, t)| (i, t.name.clone()))
        .collect();

    if !audio_tracks
        .iter()
        .any(|(i, _)| *i == app.dlg.audio_tools.ducking_trigger)
    {
        if let Some((first_i, _)) = audio_tracks.first() {
            app.dlg.audio_tools.ducking_trigger = *first_i;
        }
    }

    let mut open = true;
    let mut apply_clicked = false;
    let mut cancel_clicked = false;

    crate::dialog_window::DialogWindow::new(tr(lang, "Auto Ducking (redução automática)"))
        .id(egui::Id::new("auto_ducking_win"))
        .open(&mut open)
        .resizable(true)
        .default_size([460.0, 360.0])
        .min_size([380.0, 300.0])
        .show(ctx, |ui| {
            if audio_tracks.len() < 2 {
                ui.label(
                    egui::RichText::new(tr(
                        lang,
                        "É necessário ter ao menos duas trilhas de áudio (uma de gatilho/fala e outra alvo/música).",
                    ))
                    .color(theme.text_muted),
                );
            } else {
                let trigger_name = audio_tracks
                    .iter()
                    .find(|(i, _)| *i == app.dlg.audio_tools.ducking_trigger)
                    .map(|(_, n)| n.as_str())
                    .unwrap_or("-");
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "Trilha gatilho (fala/narração):"));
                    egui::ComboBox::from_id_salt("ducking_trigger_combo")
                        .selected_text(trigger_name)
                        .show_ui(ui, |ui| {
                            for (i, name) in &audio_tracks {
                                ui.selectable_value(
                                    &mut app.dlg.audio_tools.ducking_trigger,
                                    *i,
                                    name,
                                );
                            }
                        });
                });
                ui.add_space(4.0);
                ui.label(tr(lang, "Trilhas alvo (recebem atenuação de volume):"));
                egui::ScrollArea::vertical()
                    .max_height(88.0)
                    .show(ui, |ui| {
                        for (i, name) in &audio_tracks {
                            if *i == app.dlg.audio_tools.ducking_trigger {
                                continue;
                            }
                            let mut checked = app.dlg.audio_tools.ducking_targets.contains(i);
                            if ui.checkbox(&mut checked, name).changed() {
                                if checked {
                                    app.dlg.audio_tools.ducking_targets.push(*i);
                                } else {
                                    app.dlg.audio_tools.ducking_targets.retain(|x| x != i);
                                }
                            }
                        }
                    });
                ui.separator();
                egui::Grid::new("ducking_params")
                    .num_columns(2)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(tr(lang, "Limiar de detecção (dBFS):"));
                        ui.add(
                            egui::Slider::new(
                                &mut app.dlg.audio_tools.ducking_threshold_db,
                                -60.0..=-3.0,
                            )
                            .suffix(" dB"),
                        );
                        ui.end_row();

                        ui.label(tr(lang, "Atenuação (dB):"));
                        ui.add(
                            egui::Slider::new(
                                &mut app.dlg.audio_tools.ducking_attenuation_db,
                                -36.0..=-1.0,
                            )
                            .suffix(" dB"),
                        );
                        ui.end_row();

                        ui.label(tr(lang, "Ataque (ms):"));
                        ui.add(
                            egui::Slider::new(
                                &mut app.dlg.audio_tools.ducking_attack_ms,
                                10..=1000,
                            )
                            .suffix(" ms"),
                        );
                        ui.end_row();

                        ui.label(tr(lang, "Liberação (ms):"));
                        ui.add(
                            egui::Slider::new(
                                &mut app.dlg.audio_tools.ducking_release_ms,
                                20..=2000,
                            )
                            .suffix(" ms"),
                        );
                        ui.end_row();

                        ui.label(tr(lang, "Retenção (Hold, ms):"));
                        ui.add(
                            egui::Slider::new(&mut app.dlg.audio_tools.ducking_hold_ms, 0..=1500)
                                .suffix(" ms"),
                        );
                        ui.end_row();
                    });
            }
            ui.add_space(8.0);
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr(lang, "Cancelar")).clicked() {
                    cancel_clicked = true;
                }
                let can_apply =
                    audio_tracks.len() >= 2 && !app.dlg.audio_tools.ducking_targets.is_empty();
                if icons::icon_button(
                    ui,
                    Icon::AutoDucking,
                    Some(tr(lang, "Aplicar")),
                    false,
                    can_apply,
                    &theme,
                )
                .clicked()
                {
                    apply_clicked = true;
                }
            });
        });

    if apply_clicked {
        let spec = AutoDuckingSpec {
            trigger_track: app.dlg.audio_tools.ducking_trigger,
            target_tracks: app.dlg.audio_tools.ducking_targets.clone(),
            threshold_db: app.dlg.audio_tools.ducking_threshold_db,
            attenuation_db: app.dlg.audio_tools.ducking_attenuation_db,
            attack_ms: app.dlg.audio_tools.ducking_attack_ms,
            release_ms: app.dlg.audio_tools.ducking_release_ms,
            hold_ms: app.dlg.audio_tools.ducking_hold_ms,
        };
        let audio_map = app.media.audio_map();
        let (tracks_n, iv_n) = apply_auto_ducking(&mut app.session, &audio_map, &spec);
        app.sync_after_session();
        if tracks_n > 0 {
            let t_s = tracks_n.to_string();
            let iv_s = iv_n.to_string();
            app.set_status(tr_args(
                lang,
                "Auto Ducking aplicado em {t} trilha(s) ({iv} trecho(s) detectado(s)).",
                &[("t", &t_s), ("iv", &iv_s)],
            ));
            app.dlg.audio_tools.ducking_open = false;
        } else {
            app.set_status(tr(
                lang,
                "Nenhum trecho acima do limiar foi detectado na trilha gatilho.",
            ));
        }
    } else if cancel_clicked || !open {
        app.dlg.audio_tools.ducking_open = false;
    }
}

fn show_beat_detection_dialog(app: &mut crate::App, ctx: &egui::Context) {
    if !app.dlg.audio_tools.tempo_open {
        return;
    }
    let lang = app.lang();
    let theme = app.theme.clone();
    let selected_audio_ev = app.session.sel_events.iter().copied().find_map(|eid| {
        let (ti, ei) = app.session.project.find_event(eid)?;
        let track = app.session.project.tracks.get(ti)?;
        if track.kind == TrackKind::Audio {
            track.events.get(ei).cloned()
        } else {
            None
        }
    });

    let mut open = true;
    let mut detect_clicked = false;
    let mut apply_clicked = false;
    let mut close_clicked = false;

    crate::dialog_window::DialogWindow::new(tr(lang, "Detecção de batidas e andamento (BPM)"))
        .id(egui::Id::new("beat_detection_win"))
        .open(&mut open)
        .resizable(true)
        .default_size([430.0, 270.0])
        .min_size([360.0, 230.0])
        .show(ctx, |ui| {
            if let Some(ref ev) = selected_audio_ev {
                ui.label(tr_args(
                    lang,
                    "Evento selecionado: {name}",
                    &[("name", &ev.name)],
                ));
            } else {
                ui.label(
                    egui::RichText::new(tr(
                        lang,
                        "Selecione um evento de áudio na timeline para analisar o andamento.",
                    ))
                    .color(theme.text_muted),
                );
            }
            ui.add_space(6.0);
            if icons::icon_button(
                ui,
                Icon::BeatDetect,
                Some(tr(lang, "Analisar andamento")),
                false,
                selected_audio_ev.is_some(),
                &theme,
            )
            .clicked()
            {
                detect_clicked = true;
            }
            ui.add_space(6.0);
            if let Some(ref res) = app.dlg.audio_tools.tempo_result {
                let bpm_s = format!("{:.1}", res.bpm);
                let conf_s = format!("{:.0}%", res.confidence * 100.0);
                ui.label(
                    egui::RichText::new(tr_args(
                        lang,
                        "Andamento estimado: {bpm} BPM (confiança: {conf})",
                        &[("bpm", &bpm_s), ("conf", &conf_s)],
                    ))
                    .strong(),
                );
            }
            ui.separator();
            ui.checkbox(
                &mut app.dlg.audio_tools.tempo_update_bpm,
                tr(lang, "Atualizar BPM nas propriedades do projeto"),
            );
            ui.checkbox(
                &mut app.dlg.audio_tools.tempo_insert_markers,
                tr(lang, "Inserir marcadores de batida na timeline"),
            );
            ui.add_enabled_ui(app.dlg.audio_tools.tempo_insert_markers, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "Inserir marcador a cada:"));
                    egui::ComboBox::from_id_salt("beat_step_combo")
                        .selected_text(format!("{}", app.dlg.audio_tools.tempo_every_nth_beat))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut app.dlg.audio_tools.tempo_every_nth_beat,
                                1,
                                tr(lang, "1 batida"),
                            );
                            ui.selectable_value(
                                &mut app.dlg.audio_tools.tempo_every_nth_beat,
                                2,
                                tr(lang, "2 batidas"),
                            );
                            ui.selectable_value(
                                &mut app.dlg.audio_tools.tempo_every_nth_beat,
                                4,
                                tr(lang, "4 batidas (compasso)"),
                            );
                        });
                });
            });
            ui.add_space(8.0);
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr(lang, "Fechar")).clicked() {
                    close_clicked = true;
                }
                let can_apply =
                    selected_audio_ev.is_some() && app.dlg.audio_tools.tempo_result.is_some();
                if ui
                    .add_enabled(can_apply, egui::Button::new(tr(lang, "Aplicar")))
                    .clicked()
                {
                    apply_clicked = true;
                }
            });
        });

    if detect_clicked {
        if let Some(ref ev) = selected_audio_ev {
            let audio_map = app.media.audio_map();
            if let Some(audio) = audio_map.get(&ev.media_id) {
                app.dlg.audio_tools.tempo_result = detect_tempo_for_event(ev, audio);
                if app.dlg.audio_tools.tempo_result.is_none() {
                    app.set_status(tr(
                        lang,
                        "Áudio curto ou silencioso demais para detectar andamento.",
                    ));
                }
            } else {
                app.set_status(tr(lang, "Aguardando decodificação do áudio do evento."));
            }
        }
    }
    if apply_clicked {
        let res_opt = app.dlg.audio_tools.tempo_result.clone();
        if let (Some(ref ev), Some(ref res)) = (&selected_audio_ev, &res_opt) {
            let added = apply_tempo_detection(
                &mut app.session,
                ev,
                res,
                app.dlg.audio_tools.tempo_update_bpm,
                app.dlg.audio_tools.tempo_insert_markers,
                app.dlg.audio_tools.tempo_every_nth_beat,
            );
            app.sync_after_session();
            let bpm_s = format!("{:.1}", res.bpm);
            let n_s = added.to_string();
            app.set_status(tr_args(
                lang,
                "Andamento {bpm} BPM aplicado ({n} marcador(es) inserido(s)).",
                &[("bpm", &bpm_s), ("n", &n_s)],
            ));
            app.dlg.audio_tools.tempo_open = false;
        }
    } else if close_clicked || !open {
        app.dlg.audio_tools.tempo_open = false;
    }
}
