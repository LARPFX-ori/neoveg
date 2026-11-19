//! Janela acoplável Trimmer (`Alt+2`) com a disposição do VEGAS Pro (manual
//! do VEGAS Pro 21, "Using the Trimmer"; `docs_obsidian/ui_references/
//! referencia_visual_janelas.md`): histórico de mídias no alto, régua com a
//! seleção, área de vídeo, forma de onda, barra de botões configurável e os
//! tempos do cursor e da seleção embaixo.

use super::{rgba_to_color_image, App, DockTab};
use crate::icons::{self, Icon};
use crate::model::*;
use eframe::egui;
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind, TextureHandle};
use neoveg_common::time::format_ticks;
use neoveg_common::{tr, Language};

/// Quantas mídias o histórico guarda.
pub(crate) const HISTORY_MAX: usize = 20;

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

    pub fn label_in(self, lang: Language) -> &'static str {
        let key = match self {
            TrimmerStreamMode::Both => "Áudio + Vídeo",
            TrimmerStreamMode::VideoOnly => "Apenas Vídeo",
            TrimmerStreamMode::AudioOnly => "Apenas Áudio",
        };
        tr(lang, key)
    }

    pub fn label(self) -> &'static str {
        self.label_in(Language::PtBr)
    }
}

/// Botões da barra do Trimmer, na ordem do VEGAS Pro. O conjunto visível é
/// escolhido em *Editar conjunto de botões visíveis* (botão "…").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrimBtn {
    Loop,
    PlayFromStart,
    Play,
    Pause,
    Stop,
    GoStart,
    GoEnd,
    PrevFrame,
    NextFrame,
    AddFromCursor,
    AddUpToCursor,
    SetIn,
    SetOut,
}

impl TrimBtn {
    pub(crate) const ALL: [TrimBtn; 13] = [
        TrimBtn::Loop,
        TrimBtn::PlayFromStart,
        TrimBtn::Play,
        TrimBtn::Pause,
        TrimBtn::Stop,
        TrimBtn::GoStart,
        TrimBtn::GoEnd,
        TrimBtn::PrevFrame,
        TrimBtn::NextFrame,
        TrimBtn::AddFromCursor,
        TrimBtn::AddUpToCursor,
        TrimBtn::SetIn,
        TrimBtn::SetOut,
    ];

    /// Identificador gravado nas preferências.
    pub(crate) fn id(self) -> &'static str {
        match self {
            TrimBtn::Loop => "loop",
            TrimBtn::PlayFromStart => "play_from_start",
            TrimBtn::Play => "play",
            TrimBtn::Pause => "pause",
            TrimBtn::Stop => "stop",
            TrimBtn::GoStart => "go_start",
            TrimBtn::GoEnd => "go_end",
            TrimBtn::PrevFrame => "prev_frame",
            TrimBtn::NextFrame => "next_frame",
            TrimBtn::AddFromCursor => "add_from_cursor",
            TrimBtn::AddUpToCursor => "add_up_to_cursor",
            TrimBtn::SetIn => "set_in",
            TrimBtn::SetOut => "set_out",
        }
    }

    fn name(self) -> &'static str {
        match self {
            TrimBtn::Loop => "Reprodução em loop",
            TrimBtn::PlayFromStart => "Reproduzir do início",
            TrimBtn::Play => "Reproduzir",
            TrimBtn::Pause => "Pausar",
            TrimBtn::Stop => "Parar",
            TrimBtn::GoStart => "Ir para o início",
            TrimBtn::GoEnd => "Ir para o fim",
            TrimBtn::PrevFrame => "Quadro anterior",
            TrimBtn::NextFrame => "Próximo quadro",
            TrimBtn::AddFromCursor => "Adicionar à timeline a partir do cursor",
            TrimBtn::AddUpToCursor => "Adicionar à timeline até o cursor",
            TrimBtn::SetIn => "Definir ponto de entrada",
            TrimBtn::SetOut => "Definir ponto de saída",
        }
    }

    fn icon(self) -> Icon {
        match self {
            TrimBtn::Loop => Icon::LoopPlayback,
            TrimBtn::PlayFromStart => Icon::PlayFromStart,
            TrimBtn::Play => Icon::Play,
            TrimBtn::Pause => Icon::Pause,
            TrimBtn::Stop => Icon::Stop,
            TrimBtn::GoStart => Icon::GoStart,
            TrimBtn::GoEnd => Icon::GoEnd,
            TrimBtn::PrevFrame => Icon::PrevFrame,
            TrimBtn::NextFrame => Icon::NextFrame,
            TrimBtn::AddFromCursor => Icon::AddFromCursor,
            TrimBtn::AddUpToCursor => Icon::AddUpToCursor,
            TrimBtn::SetIn => Icon::MarkIn,
            TrimBtn::SetOut => Icon::MarkOut,
        }
    }

    /// Começa o grupo de botões próprios do Trimmer (separador antes).
    fn starts_group(self) -> bool {
        self == TrimBtn::AddFromCursor
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
    /// Mídias abertas no Trimmer, a mais recente primeiro.
    pub history: Vec<Id>,
    /// Lista do histórico em ordem alfabética em vez de recente.
    pub sort_alpha: bool,
    /// Reprodução da prévia do Trimmer (só imagem).
    pub playing: bool,
    pub loop_play: bool,
    /// Onde a reprodução começou (Parar volta para cá).
    pub play_origin: Ticks,
    /// Âncora do arrasto que cria a seleção na régua.
    pub drag_anchor: Option<Ticks>,
    /// Rascunho de *Editar conjunto de botões visíveis*: botões ocultos.
    pub buttons_draft: Option<Vec<String>>,
}

/// Põe `id` no topo do histórico, sem repetir e com limite.
pub(crate) fn push_history(history: &mut Vec<Id>, id: Id) {
    history.retain(|h| *h != id);
    history.insert(0, id);
    history.truncate(HISTORY_MAX);
}

/// Avança a reprodução em `dt`. Com loop, volta ao início da seleção ao
/// passar do fim dela; sem loop, para no fim da mídia. Devolve o novo
/// cursor e se a reprodução continua.
pub(crate) fn play_step(
    cursor: Ticks,
    dt: Ticks,
    sel: (Ticks, Ticks),
    looping: bool,
    duration: Ticks,
) -> (Ticks, bool) {
    let next = cursor + dt.max(Ticks::ZERO);
    let (a, b) = (sel.0.min(sel.1), sel.0.max(sel.1));
    if looping && b > a {
        if next >= b || next < a {
            return (a, true);
        }
        return (next, true);
    }
    if next >= duration {
        (duration, false)
    } else {
        (next, true)
    }
}

impl TrimmerState {
    pub fn load_media(&mut self, id: Id, duration: Ticks) {
        let len = duration.max(Ticks::from_millis_i64(40));
        self.media_id = Some(id);
        self.cursor = Ticks::ZERO;
        self.sel_in = Ticks::ZERO;
        self.sel_out = len;
        self.last_frame = None;
        self.playing = false;
        self.drag_anchor = None;
        push_history(&mut self.history, id);
    }

    pub fn clamp_bounds(&mut self, duration: Ticks, min_len: Ticks) {
        let dur = duration.max(min_len.max(Ticks::from_raw(1)));
        self.cursor = self.cursor.clamp_safe(Ticks::ZERO, dur);
        self.sel_in = self.sel_in.clamp_safe(Ticks::ZERO, dur - min_len.min(dur));
        self.sel_out = self
            .sel_out
            .clamp_safe((self.sel_in + min_len).min(dur), dur);
    }

    fn set_in(&mut self, frame_dur: Ticks, dur: Ticks) {
        self.sel_in = self.cursor;
        if self.sel_out <= self.sel_in {
            self.sel_out = (self.sel_in + frame_dur).min(dur);
        }
    }

    fn set_out(&mut self, frame_dur: Ticks) {
        self.sel_out = self.cursor.max(frame_dur);
        if self.sel_in >= self.sel_out {
            self.sel_in = (self.sel_out - frame_dur).max(Ticks::ZERO);
        }
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

    /// Envia a seleção do Trimmer para a timeline. `up_to_cursor` = o
    /// evento termina no cursor da timeline em vez de começar nele.
    fn trimmer_add_to_timeline(&mut self, media_id: Id, up_to_cursor: bool) {
        let frame_dur = self.project.settings.frame_dur().max(Ticks::from_raw(1));
        let len = (self.trimmer.sel_out - self.trimmer.sel_in).max(frame_dur);
        let (inc_v, inc_a) = match self.trimmer.stream_mode {
            TrimmerStreamMode::Both => (true, true),
            TrimmerStreamMode::VideoOnly => (true, false),
            TrimmerStreamMode::AudioOnly => (false, true),
        };
        let start = if up_to_cursor {
            (self.cursor - len).max(Ticks::ZERO)
        } else {
            self.cursor
        };
        self.checkpoint();
        let created = self.add_trimmed_media_to_timeline(
            media_id,
            start,
            self.trimmer.sel_in,
            len,
            inc_v,
            inc_a,
            self.sel_track,
        );
        if !created.is_empty() && self.trimmer.advance_timeline_cursor && !up_to_cursor {
            let next = self.cursor + len;
            self.set_cursor(next);
        }
    }

    fn trimmer_button(&mut self, b: TrimBtn, media_id: Id, dur: Ticks, frame_dur: Ticks) {
        let has_sel = self.trimmer.sel_out > self.trimmer.sel_in
            && (self.trimmer.sel_in > Ticks::ZERO || self.trimmer.sel_out < dur);
        match b {
            TrimBtn::Loop => {
                self.trimmer.loop_play = !self.trimmer.loop_play;
                if self.trimmer.playing {
                    self.trimmer_play();
                }
            }
            TrimBtn::PlayFromStart => {
                self.trimmer.cursor = if has_sel {
                    self.trimmer.sel_in
                } else {
                    Ticks::ZERO
                };
                self.trimmer.play_origin = self.trimmer.cursor;
                self.trimmer_play();
            }
            TrimBtn::Play => {
                if !self.trimmer.playing {
                    if self.trimmer.cursor >= dur {
                        self.trimmer.cursor = Ticks::ZERO;
                    }
                    self.trimmer.play_origin = self.trimmer.cursor;
                    self.trimmer_play();
                }
            }
            TrimBtn::Pause => self.trimmer_pause(),
            TrimBtn::Stop => self.trimmer_stop(),
            TrimBtn::GoStart => {
                self.trimmer.cursor = Ticks::ZERO;
                if self.trimmer.playing {
                    self.trimmer_play();
                }
            }
            TrimBtn::GoEnd => {
                self.trimmer.cursor = dur;
                if self.trimmer.playing {
                    self.trimmer_pause();
                }
            }
            TrimBtn::PrevFrame => {
                self.trimmer.cursor = (self.trimmer.cursor - frame_dur).max(Ticks::ZERO);
                if self.trimmer.playing {
                    self.trimmer_play();
                }
            }
            TrimBtn::NextFrame => {
                self.trimmer.cursor = (self.trimmer.cursor + frame_dur).min(dur);
                if self.trimmer.playing {
                    self.trimmer_play();
                }
            }
            TrimBtn::SetIn => self.trimmer.set_in(frame_dur, dur),
            TrimBtn::SetOut => self.trimmer.set_out(frame_dur),
            TrimBtn::AddFromCursor => self.trimmer_add_to_timeline(media_id, false),
            TrimBtn::AddUpToCursor => self.trimmer_add_to_timeline(media_id, true),
        }
    }

    pub(crate) fn trimmer_tab(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        if let Some(id) = self.trimmer.media_id {
            if self.project.media(id).is_none() {
                self.trimmer.media_id = None;
                self.trimmer.last_frame = None;
                self.trimmer_pause();
            }
        }
        let project_ids: std::collections::HashSet<Id> =
            self.project.media.iter().map(|m| m.id).collect();
        self.trimmer.history.retain(|h| project_ids.contains(h));

        self.trimmer_history_row(ui, lang);

        let Some(media_id) = self.trimmer.media_id else {
            ui.separator();
            ui.label(
                egui::RichText::new(tr(
                    lang,
                    "Abra uma mídia no Trimmer pela Mídia do projeto ou selecione um evento e use o botão acima.",
                ))
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

        // reprodução da prévia no Trimmer (áudio via streaming e vídeo sincronizado)
        if self.trimmer.playing {
            let sr = self.audio.sr;
            let audio_info = {
                let s = crate::util::lock(&self.audio.shared);
                s.trimmer_audio.clone()
            };
            if let Some(trim) = audio_info.filter(|t| t.media_id == media_id && t.playing) {
                self.trimmer.cursor = Ticks::from_samples(trim.pos.max(0), sr);
                self.trimmer.playing = trim.playing;
                ui.ctx().request_repaint();
            } else if !item.has_audio {
                let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.25);
                let (c, go) = play_step(
                    self.trimmer.cursor,
                    Ticks::from_secs_f64_clamped(f64::from(dt)),
                    (self.trimmer.sel_in, self.trimmer.sel_out),
                    self.trimmer.loop_play,
                    dur,
                );
                self.trimmer.cursor = c;
                self.trimmer.playing = go;
                ui.ctx().request_repaint();
            } else {
                self.trimmer.playing = false;
            }
        }

        let ruler_h = 22.0;
        let wave_h = if item.has_audio && item.has_video {
            44.0
        } else {
            0.0
        };
        let bar_h = 30.0;
        let bottom_h = 22.0;
        let avail = ui.available_size();
        let video_h = (avail.y - ruler_h - wave_h - bar_h - bottom_h - 8.0).max(60.0);

        self.trimmer_ruler(ui, media_id, dur, frame_dur, ruler_h);
        self.trimmer_video(ui, &item, media_id, vec2(avail.x, video_h), lang);
        if wave_h > 0.0 {
            self.trimmer_waveform(ui, media_id, dur, frame_dur, wave_h);
        }
        self.trimmer_button_bar(ui, media_id, dur, frame_dur, lang);
        self.trimmer_times(ui, lang);
        self.trimmer_buttons_dialog(ui.ctx(), lang);
    }

    /// Linha do alto: histórico, ordenar, limpar, abrir o evento selecionado
    /// e as trilhas (áudio/vídeo) que vão para a timeline.
    fn trimmer_history_row(&mut self, ui: &mut egui::Ui, lang: Language) {
        let mut picked: Option<Id> = None;
        let mut clear = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let selected = self
                .trimmer
                .media_id
                .and_then(|id| self.project.media(id))
                .map(|m| m.name.clone())
                .unwrap_or_else(|| tr(lang, "(Nenhum)").to_string());
            let mut entries: Vec<(Id, String)> = self
                .trimmer
                .history
                .iter()
                .filter_map(|id| self.project.media(*id).map(|m| (*id, m.name.clone())))
                .collect();
            if self.trimmer.sort_alpha {
                entries.sort_by_key(|e| e.1.to_lowercase());
            }
            let right_w = 4.0 * 26.0 + 130.0;
            egui::ComboBox::from_id_salt("trimmer_history")
                .width((ui.available_width() - right_w).max(120.0))
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    if entries.is_empty() {
                        ui.label(tr(lang, "(Nenhum)"));
                    }
                    for (id, name) in &entries {
                        if ui
                            .selectable_label(self.trimmer.media_id == Some(*id), name)
                            .clicked()
                        {
                            picked = Some(*id);
                        }
                    }
                });
            let th = &self.theme;
            if icons::icon_button_named(
                ui,
                Icon::ViewList,
                tr(lang, "Ordenar o histórico por nome"),
                self.trimmer.sort_alpha,
                true,
                th,
            )
            .clicked()
            {
                self.trimmer.sort_alpha = !self.trimmer.sort_alpha;
            }
            if icons::icon_button_named(
                ui,
                Icon::DeleteTrash,
                tr(lang, "Limpar o histórico"),
                false,
                !self.trimmer.history.is_empty(),
                th,
            )
            .clicked()
            {
                clear = true;
            }
            let sel_media = self
                .sel_events
                .iter()
                .next()
                .and_then(|eid| self.project.event(*eid))
                .map(|e| e.media_id);
            if icons::icon_button_named(
                ui,
                Icon::TrimmerBlade,
                tr(lang, "Abrir o evento selecionado no Trimmer"),
                false,
                sel_media.is_some(),
                th,
            )
            .clicked()
            {
                picked = sel_media;
            }
            egui::ComboBox::from_id_salt("trimmer_stream_mode")
                .width(110.0)
                .selected_text(self.trimmer.stream_mode.label_in(lang))
                .show_ui(ui, |ui| {
                    for mode in TrimmerStreamMode::ALL {
                        ui.selectable_value(
                            &mut self.trimmer.stream_mode,
                            mode,
                            mode.label_in(lang),
                        );
                    }
                });
        });
        if clear {
            self.trimmer.history.clear();
            self.trimmer.media_id = None;
            self.trimmer.last_frame = None;
            self.trimmer_pause();
        }
        if let Some(id) = picked {
            self.trimmer_pause();
            let dur = self.media_trimmer_duration(id);
            self.trimmer.load_media(id, dur);
        }
    }

    /// Régua com a seleção: clique posiciona o cursor, arrasto cria a
    /// seleção e duplo clique seleciona tudo.
    fn trimmer_ruler(
        &mut self,
        ui: &mut egui::Ui,
        media_id: Id,
        dur: Ticks,
        frame_dur: Ticks,
        h: f32,
    ) {
        let w = ui.available_width().max(160.0);
        let (strip, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
        let p = ui.painter_at(strip);
        let th = &self.theme;
        p.rect_filled(strip, 0.0, th.tl_ruler_bg);
        let x_of = |t: Ticks| {
            strip.left() + t.fraction_in(Ticks::ZERO, dur.max(Ticks::from_raw(1))) * strip.width()
        };
        let fps = self.project.settings.fps;
        let sr = self.project.settings.sample_rate;
        let fmt = self.opts.time_format;
        // marcas: ~8 rótulos ao longo da mídia
        for i in 0..=8 {
            let t = Ticks::from_raw((i128::from(dur.0) * i / 8) as i64);
            let x = x_of(t);
            p.line_segment(
                [pos2(x, strip.bottom() - 6.0), pos2(x, strip.bottom())],
                Stroke::new(1.0_f32, th.text_muted),
            );
            if i < 8 {
                p.text(
                    pos2(x + 2.0, strip.top() + 1.0),
                    Align2::LEFT_TOP,
                    format_ticks(t, fps, sr, fmt),
                    FontId::monospace(9.5),
                    th.text_muted,
                );
            }
        }
        let (a, b) = (x_of(self.trimmer.sel_in), x_of(self.trimmer.sel_out));
        let full = self.trimmer.sel_in == Ticks::ZERO && self.trimmer.sel_out >= dur;
        if !full {
            p.rect_filled(
                Rect::from_min_max(pos2(a, strip.bottom() - 7.0), pos2(b, strip.bottom())),
                0.0,
                th.tl_sel_band,
            );
            for x in [a, b] {
                p.line_segment(
                    [pos2(x, strip.top()), pos2(x, strip.bottom())],
                    Stroke::new(1.0_f32, th.accent),
                );
            }
        }
        // cursor: triângulo amarelo no alto e linha fina
        let cx = x_of(self.trimmer.cursor);
        p.add(egui::Shape::convex_polygon(
            vec![
                pos2(cx - 5.0, strip.top()),
                pos2(cx + 5.0, strip.top()),
                pos2(cx, strip.top() + 7.0),
            ],
            Color32::from_rgb(240, 200, 40),
            Stroke::NONE,
        ));
        let stroke_w = if th.high_contrast { 1.5_f32 } else { 1.0_f32 };
        p.line_segment(
            [pos2(cx, strip.top()), pos2(cx, strip.bottom())],
            Stroke::new(stroke_w, th.tl_playhead_edit),
        );
        p.rect_stroke(
            strip,
            0.0,
            Stroke::new(stroke_w, th.border),
            StrokeKind::Inside,
        );
        let _ = media_id;
        self.trimmer_pointer(&resp, strip, dur, frame_dur);
    }

    /// Clique, arrasto e duplo clique na régua ou na forma de onda.
    fn trimmer_pointer(
        &mut self,
        resp: &egui::Response,
        strip: Rect,
        dur: Ticks,
        frame_dur: Ticks,
    ) {
        let t_of = |x: f32| -> Ticks {
            let frac = ((x - strip.left()) / strip.width().max(1.0)).clamp(0.0, 1.0);
            Ticks::from_raw((dur.0 as f64 * f64::from(frac)).round() as i64)
                .clamp_safe(Ticks::ZERO, dur)
        };
        if resp.double_clicked() {
            self.trimmer.sel_in = Ticks::ZERO;
            self.trimmer.sel_out = dur;
            return;
        }
        if resp.drag_started() {
            self.trimmer.drag_anchor = resp.interact_pointer_pos().map(|p| t_of(p.x));
        }
        if let Some(pos) = resp.interact_pointer_pos() {
            let t = t_of(pos.x);
            if resp.dragged() {
                if let Some(a) = self.trimmer.drag_anchor {
                    let (lo, hi) = (a.min(t), a.max(t));
                    if hi - lo >= frame_dur {
                        self.trimmer.sel_in = lo;
                        self.trimmer.sel_out = hi;
                    }
                }
                self.trimmer.cursor = t;
            } else if resp.clicked() {
                self.trimmer.cursor = t;
            }
        }
        if resp.drag_stopped() {
            self.trimmer.drag_anchor = None;
        }
    }

    fn trimmer_video(
        &mut self,
        ui: &mut egui::Ui,
        item: &MediaItem,
        media_id: Id,
        size: egui::Vec2,
        lang: Language,
    ) {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, self.theme.extreme_bg);
        if !item.has_video {
            p.text(
                rect.center(),
                Align2::CENTER_CENTER,
                tr(lang, "Mídia somente áudio"),
                FontId::proportional(13.0),
                self.theme.text_muted,
            );
            return;
        }
        if self.trimmer.last_frame != Some((media_id, self.trimmer.cursor)) {
            let (w, h) = (
                (rect.width() as u32).clamp(64, 960),
                (rect.height() as u32).clamp(36, 540),
            );
            if let Some(img) = self.media.video_frame(item, self.trimmer.cursor, w, h) {
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
        match &self.trimmer.tex {
            Some(tex) => {
                let [iw, ih] = tex.size();
                let asp = (iw.max(1) as f32) / (ih.max(1) as f32);
                let mut w = rect.width();
                let mut h = w / asp.max(0.01);
                if h > rect.height() {
                    h = rect.height();
                    w = h * asp;
                }
                let r = Rect::from_center_size(rect.center(), vec2(w, h));
                p.image(
                    tex.id(),
                    r,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                p.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    tr(lang, "Mídia offline / sem quadro"),
                    FontId::proportional(13.0),
                    self.theme.text_muted,
                );
            }
        }
        let stroke_w = if self.theme.high_contrast {
            1.5_f32
        } else {
            1.0_f32
        };
        p.rect_stroke(
            rect,
            0.0,
            Stroke::new(stroke_w, self.theme.border),
            StrokeKind::Inside,
        );
    }

    fn trimmer_waveform(
        &mut self,
        ui: &mut egui::Ui,
        media_id: Id,
        dur: Ticks,
        frame_dur: Ticks,
        h: f32,
    ) {
        let w = ui.available_width().max(160.0);
        let (strip, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
        let p = ui.painter_at(strip);
        p.rect_filled(strip, 0.0, self.theme.tl_lane_even);
        let x_of = |t: Ticks| {
            strip.left() + t.fraction_in(Ticks::ZERO, dur.max(Ticks::from_raw(1))) * strip.width()
        };
        let full = self.trimmer.sel_in == Ticks::ZERO && self.trimmer.sel_out >= dur;
        if !full {
            p.rect_filled(
                Rect::from_min_max(
                    pos2(x_of(self.trimmer.sel_in), strip.top()),
                    pos2(x_of(self.trimmer.sel_out), strip.bottom()),
                ),
                0.0,
                self.theme.tl_sel_band,
            );
        }
        if let Some(audio) = self.media.audio_map().get(&media_id) {
            let cols = (strip.width() as usize).clamp(1, 1024);
            let mid = strip.center().y;
            let half_h = (strip.height() * 0.45).max(2.0);
            for i in 0..cols {
                let x = strip.left() + (i as f32 + 0.5) * strip.width() / cols as f32;
                let t0 = Ticks::from_raw((i128::from(dur.0) * i as i128 / cols as i128) as i64);
                let t1 =
                    Ticks::from_raw((i128::from(dur.0) * (i as i128 + 1) / cols as i128) as i64);
                let (lo, hi) = audio.peak_range(t0, t1);
                let wave_col = if self.theme.high_contrast {
                    self.theme.accent
                } else {
                    self.theme.tl_gain_line
                };
                p.line_segment(
                    [
                        pos2(x, mid - hi.clamp(-1.0, 1.0) * half_h),
                        pos2(x, mid - lo.clamp(-1.0, 1.0) * half_h),
                    ],
                    Stroke::new(1.0_f32, wave_col),
                );
            }
        }
        let stroke_w = if self.theme.high_contrast {
            1.5_f32
        } else {
            1.0_f32
        };
        let cx = x_of(self.trimmer.cursor);
        p.line_segment(
            [pos2(cx, strip.top()), pos2(cx, strip.bottom())],
            Stroke::new(stroke_w, self.theme.tl_playhead_edit),
        );
        p.rect_stroke(
            strip,
            0.0,
            Stroke::new(stroke_w, self.theme.border),
            StrokeKind::Inside,
        );
        self.trimmer_pointer(&resp, strip, dur, frame_dur);
    }

    /// Barra de botões centralizada, com o "…" que edita o conjunto visível.
    fn trimmer_button_bar(
        &mut self,
        ui: &mut egui::Ui,
        media_id: Id,
        dur: Ticks,
        frame_dur: Ticks,
        lang: Language,
    ) {
        let hidden = self.prefs.trimmer_hidden_buttons.clone();
        let visible: Vec<TrimBtn> = TrimBtn::ALL
            .into_iter()
            .filter(|b| !hidden.iter().any(|h| h == b.id()))
            .collect();
        let mut pressed: Option<TrimBtn> = None;
        let mut more = false;
        let groups = visible.iter().filter(|b| b.starts_group()).count();
        let btn_w = 26.0;
        let total = (visible.len() + 1) as f32 * btn_w + groups as f32 * 10.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add_space(((ui.available_width() - total) * 0.5).max(0.0));
            for b in &visible {
                if b.starts_group() {
                    ui.separator();
                }
                let active = match b {
                    TrimBtn::Loop => self.trimmer.loop_play,
                    TrimBtn::Play => self.trimmer.playing,
                    _ => false,
                };
                if icons::icon_button_named(
                    ui,
                    b.icon(),
                    tr(lang, b.name()),
                    active,
                    true,
                    &self.theme,
                )
                .clicked()
                {
                    pressed = Some(*b);
                }
            }
            if icons::icon_button_named(
                ui,
                Icon::MoreDots,
                tr(lang, "Editar conjunto de botões visíveis..."),
                false,
                true,
                &self.theme,
            )
            .clicked()
            {
                more = true;
            }
        });
        if let Some(b) = pressed {
            self.trimmer_button(b, media_id, dur, frame_dur);
        }
        if more {
            self.trimmer.buttons_draft = Some(hidden);
        }
    }

    /// Linha de baixo: cursor e a seleção (início, fim e duração).
    fn trimmer_times(&mut self, ui: &mut egui::Ui, lang: Language) {
        let fps = self.project.settings.fps;
        let sr = self.project.settings.sample_rate;
        let fmt = self.opts.time_format;
        let f = |t: Ticks| format_ticks(t, fps, sr, fmt);
        let len = (self.trimmer.sel_out - self.trimmer.sel_in).max(Ticks::ZERO);
        let th = self.theme.clone();
        ui.horizontal(|ui| {
            let boxed = |ui: &mut egui::Ui, text: String, tip: &str| {
                egui::Frame::new()
                    .stroke(Stroke::new(1.0_f32, th.border))
                    .inner_margin(egui::Margin::symmetric(4, 1))
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(
                            egui::RichText::new(text).monospace().color(th.timecode_fg),
                        ))
                        .on_hover_text(tip);
                    });
            };
            ui.add_space(((ui.available_width() - 420.0) * 0.5).max(0.0));
            icons::paint_inline(ui, Icon::InsertMarker, 13.0, &th);
            boxed(ui, f(self.trimmer.cursor), tr(lang, "Cursor do Trimmer"));
            ui.add_space(12.0);
            icons::paint_inline(ui, Icon::MarkIn, 13.0, &th);
            boxed(ui, f(self.trimmer.sel_in), tr(lang, "Início da seleção"));
            icons::paint_inline(ui, Icon::MarkOut, 13.0, &th);
            boxed(ui, f(self.trimmer.sel_out), tr(lang, "Fim da seleção"));
            boxed(ui, f(len), tr(lang, "Duração da seleção"));
        });
    }

    /// *Editar conjunto de botões visíveis* da barra do Trimmer.
    fn trimmer_buttons_dialog(&mut self, ctx: &egui::Context, lang: Language) {
        let Some(mut draft) = self.trimmer.buttons_draft.take() else {
            return;
        };
        let mut open = true;
        let mut done: Option<bool> = None;
        crate::dialog_window::DialogWindow::new(tr(lang, "Editar conjunto de botões visíveis..."))
            .id(egui::Id::new("trimmer_buttons_dialog"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(tr(
                    lang,
                    "Escolha os botões que aparecem na barra do Trimmer.",
                ));
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        for b in TrimBtn::ALL {
                            let mut on = !draft.iter().any(|h| h == b.id());
                            if ui.checkbox(&mut on, tr(lang, b.name())).changed() {
                                draft.retain(|h| h != b.id());
                                if !on {
                                    draft.push(b.id().to_string());
                                }
                            }
                        }
                    });
                    ui.vertical(|ui| {
                        if ui.button(tr(lang, "Mostrar todos")).clicked() {
                            draft.clear();
                        }
                        if ui.button(tr(lang, "Ocultar todos")).clicked() {
                            draft = TrimBtn::ALL.iter().map(|b| b.id().to_string()).collect();
                        }
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(tr(lang, "OK")).clicked() {
                        done = Some(true);
                    }
                    if ui.button(tr(lang, "Cancelar")).clicked() {
                        done = Some(false);
                    }
                });
            });
        match done {
            Some(true) => {
                self.prefs.trimmer_hidden_buttons = draft;
                if let Err(e) = self.prefs.save() {
                    self.set_status(format!(
                        "{}: {e}",
                        tr(lang, "Não foi possível salvar as preferências")
                    ));
                }
            }
            Some(false) => {}
            None if open => self.trimmer.buttons_draft = Some(draft),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(n: i64) -> Ticks {
        Ticks::from_secs_i64(n)
    }

    #[test]
    fn historico_sem_repetir_e_com_limite() {
        let mut h = Vec::new();
        push_history(&mut h, 1);
        push_history(&mut h, 2);
        push_history(&mut h, 1);
        assert_eq!(h, vec![1, 2]);
        for i in 10..40 {
            push_history(&mut h, i);
        }
        assert_eq!(h.len(), HISTORY_MAX);
        assert_eq!(h[0], 39);
    }

    #[test]
    fn reproducao_com_e_sem_loop() {
        // sem loop: para no fim da mídia
        assert_eq!(
            play_step(s(9), s(2), (s(0), s(10)), false, s(10)),
            (s(10), false)
        );
        assert_eq!(
            play_step(s(3), s(1), (s(0), s(10)), false, s(10)),
            (s(4), true)
        );
        // com loop: volta ao início da seleção
        assert_eq!(
            play_step(s(4), s(2), (s(2), s(5)), true, s(10)),
            (s(2), true)
        );
        assert_eq!(
            play_step(s(3), s(1), (s(2), s(5)), true, s(10)),
            (s(4), true)
        );
        // cursor fora da seleção entra nela; seleção invertida é aceita
        assert_eq!(
            play_step(s(8), s(0), (s(5), s(2)), true, s(10)),
            (s(2), true)
        );
        // dt negativo não volta
        assert_eq!(
            play_step(s(3), Ticks::from_raw(-5), (s(0), s(10)), false, s(10)),
            (s(3), true)
        );
    }

    #[test]
    fn botoes_tem_identificadores_unicos() {
        let mut ids: Vec<&str> = TrimBtn::ALL.iter().map(|b| b.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), TrimBtn::ALL.len());
    }
}
