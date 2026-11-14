//! Janela acoplável Trimmer (pré-marcação de pontos In/Out e envio para a timeline)
//! no estilo clássico do VEGAS Pro (`Alt+2`).

use super::{rgba_to_color_image, App, DockTab};
use crate::model::*;
use eframe::egui;
use egui::{pos2, vec2, Align2, FontId, Rect, Sense, Stroke, StrokeKind, TextureHandle};
use neoveg_common::time::format_ticks;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrimmerStreamMode {
    #[default]
    Both,
    VideoOnly,
    AudioOnly,
}

impl TrimmerStreamMode {
    pub const ALL: [TrimmerStreamMode; 3] = [
        TrimmerStreamMode::Both,
        TrimmerStreamMode::VideoOnly,
        TrimmerStreamMode::AudioOnly,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TrimmerStreamMode::Both => "Áudio + Vídeo",
            TrimmerStreamMode::VideoOnly => "Apenas Vídeo",
            TrimmerStreamMode::AudioOnly => "Apenas Áudio",
        }
    }
}

#[derive(Default)]
pub struct TrimmerState {
    pub media_id: Option<Id>,
    pub cursor: Ticks,
    pub sel_in: Ticks,
    pub sel_out: Ticks,
    pub stream_mode: TrimmerStreamMode,
    pub advance_timeline_cursor: bool,
    pub tex: Option<TextureHandle>,
    pub last_frame: Option<(Id, Ticks)>,
}

impl TrimmerState {
    pub fn load_media(&mut self, id: Id, duration: Ticks) {
        let len = duration.max(Ticks::from_millis_i64(40));
        self.media_id = Some(id);
        self.cursor = Ticks::ZERO;
        self.sel_in = Ticks::ZERO;
        self.sel_out = len;
        self.last_frame = None;
    }

    pub fn clamp_bounds(&mut self, duration: Ticks, min_len: Ticks) {
        let dur = duration.max(min_len.max(Ticks::from_raw(1)));
        self.cursor = self.cursor.clamp_safe(Ticks::ZERO, dur);
        self.sel_in = self.sel_in.clamp_safe(Ticks::ZERO, dur - min_len.min(dur));
        self.sel_out = self
            .sel_out
            .clamp_safe((self.sel_in + min_len).min(dur), dur);
    }
}

impl App {
    /// Abre uma mídia do projeto no painel Trimmer zerando seus limites `In..Out`.
    pub fn open_in_trimmer(&mut self, media_id: Id) {
        let dur = self.media_trimmer_duration(media_id);
        self.trimmer.load_media(media_id, dur);
        self.dock_visible = true;
        self.detached_tabs.remove(&DockTab::Trimmer);
        self.dock_tab = DockTab::Trimmer;
        self.pending_dock_focus = Some(DockTab::Trimmer);
    }

    fn media_trimmer_duration(&self, media_id: Id) -> Ticks {
        let still_len = Ticks::from_secs_f64_clamped(self.prefs.still_image_secs as f64);
        self.project
            .media(media_id)
            .and_then(|m| m.duration)
            .unwrap_or(still_len)
            .max(self.project.settings.frame_dur())
    }

    /// Insere um trecho (`offset`, `length`) de mídia na timeline, filtrando faixas A/V.
    #[allow(clippy::too_many_arguments)]
    pub fn add_trimmed_media_to_timeline(
        &mut self,
        media_id: Id,
        start: Ticks,
        offset: Ticks,
        length: Ticks,
        include_video: bool,
        include_audio: bool,
        track_hint: Option<usize>,
    ) -> Vec<Id> {
        let Some(m) = self.project.media(media_id).cloned() else {
            return vec![];
        };
        let min_len = self.project.settings.frame_dur().max(Ticks::from_raw(1));
        let start = start.max(Ticks::ZERO);
        let offset = offset.max(Ticks::ZERO);
        let length = length.max(min_len);
        let want_video = m.has_video && include_video;
        let want_audio = m.has_audio && include_audio;
        if !want_video && !want_audio {
            return vec![];
        }
        let mut created = vec![];
        let group = if want_video && want_audio {
            Some(new_id())
        } else {
            None
        };
        let mut video_track_idx = None;
        if want_video {
            let ti = self.find_or_create_track(TrackKind::Video, track_hint);
            let mut e = Event::new(&m, start, length);
            e.offset = offset;
            e.group = group;
            if m.duration.is_none() {
                e.looped = false;
            }
            created.push(e.id);
            self.project.tracks[ti].events.push(e);
            video_track_idx = Some(ti);
        }
        if want_audio {
            let hint = match video_track_idx {
                Some(v) => Some(v + 1),
                None => track_hint,
            };
            let ti = self.find_or_create_track(TrackKind::Audio, hint);
            let mut e = Event::new(&m, start, length);
            e.offset = offset;
            e.group = group;
            created.push(e.id);
            self.project.tracks[ti].events.push(e);
        }
        self.sel_events = created.iter().cloned().collect();
        self.changed();
        created
    }

    pub(crate) fn trimmer_tab(&mut self, ui: &mut egui::Ui) {
        if let Some(id) = self.trimmer.media_id {
            if self.project.media(id).is_none() {
                self.trimmer.media_id = None;
                self.trimmer.last_frame = None;
            }
        }
        if self.trimmer.media_id.is_none() {
            if let Some(first) = self.project.media.first() {
                let id = first.id;
                let dur = self.media_trimmer_duration(id);
                self.trimmer.load_media(id, dur);
            }
        }

        let selected_label = self
            .trimmer
            .media_id
            .and_then(|id| self.project.media(id))
            .map(|m| m.name.clone())
            .unwrap_or_else(|| "(Nenhuma mídia carregada)".into());

        let mut picked_id: Option<Id> = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Mídia:").strong().small());
            egui::ComboBox::from_id_salt("trimmer_media_combo")
                .width(180.0)
                .selected_text(&selected_label)
                .show_ui(ui, |ui| {
                    for m in &self.project.media {
                        if ui
                            .selectable_label(self.trimmer.media_id == Some(m.id), &m.name)
                            .clicked()
                        {
                            picked_id = Some(m.id);
                        }
                    }
                });
            if let Some(ev_mid) = self
                .sel_events
                .iter()
                .next()
                .and_then(|eid| self.project.event(*eid))
                .map(|e| e.media_id)
            {
                if ui
                    .small_button("Carregar seleção")
                    .on_hover_text("Abre a mídia do evento selecionado na timeline")
                    .clicked()
                {
                    picked_id = Some(ev_mid);
                }
            }
            egui::ComboBox::from_id_salt("trimmer_stream_mode")
                .width(120.0)
                .selected_text(self.trimmer.stream_mode.label())
                .show_ui(ui, |ui| {
                    for mode in TrimmerStreamMode::ALL {
                        ui.selectable_value(&mut self.trimmer.stream_mode, mode, mode.label());
                    }
                });
        });

        if let Some(id) = picked_id {
            let dur = self.media_trimmer_duration(id);
            self.trimmer.load_media(id, dur);
        }

        let Some(media_id) = self.trimmer.media_id else {
            ui.separator();
            ui.label(
                egui::RichText::new(
                    "Importe ou selecione uma mídia para definir pontos In/Out no Trimmer.",
                )
                .color(self.theme.text_muted),
            );
            return;
        };
        let Some(item) = self.project.media(media_id).cloned() else {
            return;
        };

        let frame_dur = self.project.settings.frame_dur().max(Ticks::from_raw(1));
        let dur = self.media_trimmer_duration(media_id);
        self.trimmer.clamp_bounds(dur, frame_dur);

        // Prévia de quadro no cursor do Trimmer.
        if item.has_video && self.trimmer.last_frame != Some((media_id, self.trimmer.cursor)) {
            if let Some(img) = self.media.video_frame(&item, self.trimmer.cursor, 320, 180) {
                let ci = rgba_to_color_image(&img);
                match &mut self.trimmer.tex {
                    Some(t) => t.set(ci, egui::TextureOptions::LINEAR),
                    None => {
                        self.trimmer.tex = Some(ui.ctx().load_texture(
                            "trimmer_preview",
                            ci,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                }
            }
            self.trimmer.last_frame = Some((media_id, self.trimmer.cursor));
        }

        let fps = self.project.settings.fps;
        let sr = self.project.settings.sample_rate;
        let fmt = self.opts.time_format;

        ui.separator();
        ui.columns(2, |cols| {
            // Coluna esquerda: visor do clipe no Trimmer
            let left = &mut cols[0];
            if item.has_video {
                if let Some(tex) = &self.trimmer.tex {
                    let avail = vec2(left.available_width().max(120.0), 116.0);
                    let [iw, ih] = tex.size();
                    let asp = (iw.max(1) as f32) / (ih.max(1) as f32);
                    let h = avail.y.min(avail.x / asp.max(0.1)).max(48.0);
                    let w = (h * asp).min(avail.x);
                    left.image(egui::load::SizedTexture::new(tex.id(), vec2(w, h)));
                } else {
                    left.label(egui::RichText::new("Mídia offline / sem quadro").small());
                }
            } else {
                left.label(egui::RichText::new("Mídia somente áudio").strong().small());
            }
            left.label(
                egui::RichText::new(&item.info)
                    .small()
                    .color(self.theme.text_muted),
            );

            // Coluna direita: leitores SMPTE, In/Out e inserção na timeline
            let right = &mut cols[1];
            let sel_len = (self.trimmer.sel_out - self.trimmer.sel_in).max(Ticks::ZERO);
            right.label(
                egui::RichText::new(format!(
                    "Cursor: {}  |  Duração: {}",
                    format_ticks(self.trimmer.cursor, fps, sr, fmt),
                    format_ticks(dur, fps, sr, fmt),
                ))
                .monospace()
                .small(),
            );
            right.label(
                egui::RichText::new(format!(
                    "In: {}   Out: {}   Trecho: {}",
                    format_ticks(self.trimmer.sel_in, fps, sr, fmt),
                    format_ticks(self.trimmer.sel_out, fps, sr, fmt),
                    format_ticks(sel_len, fps, sr, fmt),
                ))
                .monospace()
                .small(),
            );
            right.horizontal_wrapped(|ui| {
                if ui.small_button("◀ 1q").clicked() {
                    self.trimmer.cursor = (self.trimmer.cursor - frame_dur).max(Ticks::ZERO);
                }
                if ui.small_button("1q ▶").clicked() {
                    self.trimmer.cursor = (self.trimmer.cursor + frame_dur).min(dur);
                }
                if ui
                    .small_button("Marcar In (I)")
                    .on_hover_text("Define o ponto inicial do trecho no cursor do Trimmer")
                    .clicked()
                {
                    self.trimmer.sel_in = self.trimmer.cursor;
                    if self.trimmer.sel_out <= self.trimmer.sel_in {
                        self.trimmer.sel_out = (self.trimmer.sel_in + frame_dur).min(dur);
                    }
                }
                if ui
                    .small_button("Marcar Out (O)")
                    .on_hover_text("Define o ponto final do trecho no cursor do Trimmer")
                    .clicked()
                {
                    self.trimmer.sel_out = self.trimmer.cursor.max(frame_dur);
                    if self.trimmer.sel_in >= self.trimmer.sel_out {
                        self.trimmer.sel_in = (self.trimmer.sel_out - frame_dur).max(Ticks::ZERO);
                    }
                }
                if ui.small_button("Tudo").clicked() {
                    self.trimmer.sel_in = Ticks::ZERO;
                    self.trimmer.sel_out = dur;
                }
            });
            right.horizontal_wrapped(|ui| {
                if ui
                    .button("Adicionar à Timeline no Cursor")
                    .on_hover_text("Insere o trecho marcado [In..Out] na timeline")
                    .clicked()
                {
                    let (inc_v, inc_a) = match self.trimmer.stream_mode {
                        TrimmerStreamMode::Both => (true, true),
                        TrimmerStreamMode::VideoOnly => (true, false),
                        TrimmerStreamMode::AudioOnly => (false, true),
                    };
                    self.checkpoint();
                    let created = self.add_trimmed_media_to_timeline(
                        media_id,
                        self.cursor,
                        self.trimmer.sel_in,
                        sel_len.max(frame_dur),
                        inc_v,
                        inc_a,
                        self.sel_track,
                    );
                    if !created.is_empty() && self.trimmer.advance_timeline_cursor {
                        let next = self.cursor + sel_len.max(frame_dur);
                        self.set_cursor(next);
                    }
                }
                ui.checkbox(&mut self.trimmer.advance_timeline_cursor, "Avançar cursor");
            });
        });

        ui.add_space(4.0);
        // Faixa interativa com waveform + trecho In..Out + cursor do Trimmer.
        let desired_w = ui.available_width().max(160.0);
        let (strip, resp) = ui.allocate_exact_size(vec2(desired_w, 42.0), Sense::click_and_drag());
        let p = ui.painter_at(strip);
        p.rect_filled(strip, 2.0, self.theme.extreme_bg);
        p.rect_stroke(
            strip,
            2.0,
            Stroke::new(1.0_f32, self.theme.border),
            StrokeKind::Inside,
        );

        let dur_i = dur.0.max(1) as f32;
        let to_x = |t: Ticks| -> f32 {
            strip.left() + ((t.0 as f32 / dur_i).clamp(0.0, 1.0)) * strip.width()
        };
        let x_in = to_x(self.trimmer.sel_in);
        let x_out = to_x(self.trimmer.sel_out);
        let sel_r = Rect::from_min_max(
            pos2(x_in, strip.top() + 1.0),
            pos2(x_out, strip.bottom() - 1.0),
        );
        p.rect_filled(sel_r, 0.0, self.theme.selection_bg.gamma_multiply(0.35));

        if let Some(audio) = self.media.audio_map().get(&media_id) {
            let cols = (strip.width() as usize).clamp(1, 512);
            let mid = strip.center().y;
            let half_h = (strip.height() * 0.42).max(2.0);
            for i in 0..cols {
                let x = strip.left() + (i as f32 + 0.5) * strip.width() / cols as f32;
                let t0 = Ticks::from_raw((dur.0 as i128 * i as i128 / cols as i128) as i64);
                let t1 = Ticks::from_raw((dur.0 as i128 * (i as i128 + 1) / cols as i128) as i64);
                let (lo, hi) = audio.peak_range(t0, t1);
                let y0 = mid - hi.clamp(-1.0, 1.0) * half_h;
                let y1 = mid - lo.clamp(-1.0, 1.0) * half_h;
                p.line_segment(
                    [pos2(x, y0), pos2(x, y1)],
                    Stroke::new(1.0_f32, self.theme.accent),
                );
            }
        }

        // Marcações In, Out e Cursor
        p.line_segment(
            [pos2(x_in, strip.top()), pos2(x_in, strip.bottom())],
            Stroke::new(2.0_f32, self.theme.accent),
        );
        p.line_segment(
            [pos2(x_out, strip.top()), pos2(x_out, strip.bottom())],
            Stroke::new(2.0_f32, self.theme.tl_marker),
        );
        let x_cur = to_x(self.trimmer.cursor);
        p.line_segment(
            [pos2(x_cur, strip.top()), pos2(x_cur, strip.bottom())],
            Stroke::new(2.0_f32, self.theme.tl_playhead_edit),
        );
        p.text(
            pos2(strip.left() + 4.0, strip.top() + 2.0),
            Align2::LEFT_TOP,
            "Trimmer",
            FontId::proportional(9.5),
            self.theme.text_muted,
        );

        if (resp.clicked() || resp.dragged()) && strip.width() > 1.0 {
            if let Some(pos) = resp.interact_pointer_pos() {
                let frac = ((pos.x - strip.left()) / strip.width()).clamp(0.0, 1.0) as f64;
                self.trimmer.cursor = Ticks::from_raw((dur.0 as f64 * frac).round() as i64)
                    .clamp_safe(Ticks::ZERO, dur);
            }
        }
    }
}
