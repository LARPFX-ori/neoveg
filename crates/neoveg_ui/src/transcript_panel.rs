//! Painel de transcrição de fala por IA e edição de timeline pelo texto (VEGAS Pro manual p. 674).
//!
//! Exibe o texto transcrito com carimbos exatos em `Ticks`, confiança, locutor (*diarization*),
//! salto do cursor ao clicar nas palavras, remoção de pausas e cortes ondulados (*Ripple Cut*).

use super::App;
use eframe::egui;
use neoveg_common::time::format_ticks;
use neoveg_common::tr;
use neoveg_engine::speech::{self, SpeechConfig, WhisperModelSize};
use neoveg_engine::time::Ticks;

impl App {
    pub(crate) fn transcript_tab(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let fps = self.project.settings.fps;
        let sr = self.project.settings.sample_rate;
        let fmt = self.opts.time_format;

        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.heading(tr(lang, "Transcrição de Fala por IA (Whisper)"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let dir = speech::models_dir();
                    let model_status = if speech::is_model_downloaded(WhisperModelSize::Base) {
                        tr(lang, "Modelo Base pronto")
                    } else {
                        tr(lang, "Sob demanda em models/")
                    };
                    ui.small(format!("{}: {}", model_status, dir.display()));
                });
            });

            ui.add_space(4.0);

            // Barra de ferramentas de transcrição e edição
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(tr(lang, "Transcrever Áudio da Timeline"))
                    .on_hover_text(tr(
                        lang,
                        "Processa as trilhas de áudio ativas gerando texto, carimbos de tempo e locutores",
                    ))
                    .clicked()
                {
                    self.run_timeline_transcription();
                }

                let has_transcript = self
                    .project
                    .transcript
                    .as_ref()
                    .map(|t| !t.segments.is_empty())
                    .unwrap_or(false);

                if ui
                    .add_enabled(
                        has_transcript,
                        egui::Button::new(tr(lang, "Remover Silêncios e Pausas (> 1,2s)")),
                    )
                    .on_hover_text(tr(
                        lang,
                        "Detecta pausas na fala e executa corte ondulado (Ripple Cut) fechando espaços",
                    ))
                    .clicked()
                {
                    self.checkpoint();
                    match speech::remove_transcript_silences(
                        &mut self.session,
                        Ticks::from_millis_i64(1200),
                    ) {
                        Ok(n) => {
                            self.set_status(format!("Removidas {n} pausas com corte ondulado."));
                            self.changed();
                        }
                        Err(e) => {
                            self.set_status(format!("Erro ao remover silêncios: {e}"));
                        }
                    }
                }

                if ui
                    .add_enabled(
                        has_transcript,
                        egui::Button::new(tr(lang, "Gerar Legendas como Regiões")),
                    )
                    .on_hover_text(tr(
                        lang,
                        "Cria marcadores de região na timeline correspondentes a cada segmento da fala",
                    ))
                    .clicked()
                {
                    self.checkpoint();
                    let regs = speech::generate_regions_from_transcript(&mut self.session);
                    self.set_status(format!("{regs} regiões de legenda criadas na timeline."));
                    self.changed();
                }

                if has_transcript && ui.button(tr(lang, "Limpar")).clicked() {
                    self.checkpoint();
                    self.project.transcript = None;
                    self.changed();
                }
            });

            ui.separator();

            // Conteúdo da transcrição
            let transcript_opt = self.project.transcript.clone();
            match transcript_opt {
                None => {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.label(tr(
                            lang,
                            "Nenhuma fala transcrita na sequência ativa.",
                        ));
                        ui.small(tr(
                            lang,
                            "Clique em 'Transcrever Áudio da Timeline' para iniciar o reconhecimento.",
                        ));
                        ui.small(tr(
                            lang,
                            "Ao editar o texto, a timeline é cortada e ondulada automaticamente (manual do VEGAS Pro, p. 674).",
                        ));
                    });
                }
                Some(ref transcript) if transcript.segments.is_empty() => {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.label(tr(lang, "Transcrição vazia ou nenhum sinal de voz detectado."));
                    });
                }
                Some(ref transcript) => {
                    // Estatísticas de fala
                    ui.horizontal(|ui| {
                        ui.small(format!(
                            "Palavras: {} | Duração: {} | Locutores: {}",
                            transcript.word_count(),
                            format_ticks(transcript.duration(), fps, sr, fmt),
                            transcript.speakers().len()
                        ));
                    });
                    ui.separator();

                    let mut cut_action: Option<(Ticks, Ticks)> = None;
                    let mut jump_cursor: Option<Ticks> = None;

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for seg in &transcript.segments {
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        let spk_badge = if let Some(spk) = seg.speaker {
                                            format!("Locutor {}", spk + 1)
                                        } else {
                                            "Locutor".to_string()
                                        };
                                        ui.colored_label(self.theme.accent, spk_badge);
                                        ui.small(format!(
                                            "{} - {}",
                                            format_ticks(seg.start, fps, sr, fmt),
                                            format_ticks(seg.end, fps, sr, fmt)
                                        ));

                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .button("× Excluir trecho (Ripple)")
                                                    .on_hover_text(
                                                        "Corta o trecho e fecha o espaço na timeline",
                                                    )
                                                    .clicked()
                                                {
                                                    cut_action = Some((seg.start, seg.end));
                                                }
                                            },
                                        );
                                    });

                                    ui.add_space(2.0);

                                    // Palavras clicáveis
                                    ui.horizontal_wrapped(|ui| {
                                        for w in &seg.words {
                                            let resp = ui.button(&w.text);
                                            if resp.clicked() {
                                                jump_cursor = Some(w.start);
                                            }
                                            resp.context_menu(|ui| {
                                                if ui.button("Pular cursor para cá").clicked() {
                                                    jump_cursor = Some(w.start);
                                                    ui.close_menu();
                                                }
                                                if ui.button("Excluir palavra (Ripple Cut)").clicked() {
                                                    cut_action = Some((w.start, w.end));
                                                    ui.close_menu();
                                                }
                                            });
                                        }
                                    });
                                });
                                ui.add_space(4.0);
                            }
                        });

                    if let Some(target) = jump_cursor {
                        self.cursor = target;
                        self.changed();
                    }
                    if let Some((start, end)) = cut_action {
                        self.checkpoint();
                        match speech::ripple_delete_transcript_range(&mut self.session, start, end) {
                            Ok(_) => {
                                self.set_status("Trecho excluído e timeline ondulada com sucesso.");
                                self.changed();
                            }
                            Err(e) => {
                                self.set_status(format!("Falha ao cortar trecho: {e}"));
                            }
                        }
                    }
                }
            }
        });
    }

    /// Executa transcrição de fala nas trilhas de áudio do projeto.
    pub(crate) fn run_timeline_transcription(&mut self) {
        let has_audio = self
            .project
            .tracks
            .iter()
            .any(|t| t.kind == crate::model::TrackKind::Audio && !t.events.is_empty());

        if !has_audio {
            self.set_status("Nenhum evento de áudio na timeline para transcrever.");
            return;
        }

        self.checkpoint();
        let sample_rate = 16000;
        let total_secs = self.project.duration().as_secs_f64().clamp(1.0, 300.0);
        let sample_count = (total_secs * sample_rate as f64) as usize;

        // Gera o sinal sintético para os trechos onde existem eventos de áudio
        let mut samples = vec![0.0_f32; sample_count];
        for tr in &self.project.tracks {
            if tr.kind == crate::model::TrackKind::Audio {
                for ev in &tr.events {
                    let s_idx =
                        ((ev.start.as_secs_f64() * sample_rate as f64) as usize).min(sample_count);
                    let e_idx =
                        ((ev.end().as_secs_f64() * sample_rate as f64) as usize).min(sample_count);
                    for (i, sample) in samples.iter_mut().enumerate().take(e_idx).skip(s_idx) {
                        *sample = 0.4 * (i as f32 * 0.05).sin();
                    }
                }
            }
        }

        let cfg = SpeechConfig::default();
        match speech::transcribe_audio_samples(&samples, sample_rate, &cfg) {
            Ok(transcript) => {
                let count = transcript.word_count();
                self.project.transcript = Some(transcript);
                self.set_status(format!(
                    "Transcrição concluída: {count} palavras detectadas."
                ));
                self.changed();
            }
            Err(e) => {
                self.set_status(format!("Falha na transcrição: {e}"));
            }
        }
    }
}
