//! Janela acoplável Detalhes da Edição (Edit Details — `Alt+3`) em formato tabular
//! para inspeção e edição precisa de Eventos, Marcadores, Regiões, Trilhas e Barramentos.

use super::{App, FxTarget};
use crate::model::*;
use eframe::egui;
use neoveg_common::time::{format_ticks, parse_time, TimeFormat};
use neoveg_common::{tr, Language};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DetailsCategory {
    #[default]
    Events,
    Markers,
    Regions,
    Tracks,
    Buses,
}

impl DetailsCategory {
    pub const ALL: [DetailsCategory; 5] = [
        DetailsCategory::Events,
        DetailsCategory::Markers,
        DetailsCategory::Regions,
        DetailsCategory::Tracks,
        DetailsCategory::Buses,
    ];

    pub fn label_in(self, lang: Language) -> &'static str {
        let key = match self {
            DetailsCategory::Events => "Eventos",
            DetailsCategory::Markers => "Marcadores",
            DetailsCategory::Regions => "Regiões",
            DetailsCategory::Tracks => "Trilhas",
            DetailsCategory::Buses => "Barramentos",
        };
        tr(lang, key)
    }

    pub fn label(self) -> &'static str {
        self.label_in(Language::PtBr)
    }
}

#[derive(Default)]
pub struct EditDetailsState {
    pub category: DetailsCategory,
    pub filter: String,
    pub edit_buf: HashMap<String, String>,
}

/// Editor seguro de timestamp SMPTE/Milissegundos usando buffer temporário até Enter/blur.
fn time_cell_editor(
    ui: &mut egui::Ui,
    bufs: &mut HashMap<String, String>,
    key: String,
    value: Ticks,
    fps: FrameRate,
    sr: u32,
    fmt: TimeFormat,
) -> Option<Ticks> {
    let cur_str = format_ticks(value, fps, sr, fmt);
    let mut text = bufs.get(&key).cloned().unwrap_or_else(|| cur_str.clone());
    let resp = ui.add(
        egui::TextEdit::singleline(&mut text)
            .desired_width(88.0)
            .font(egui::TextStyle::Monospace),
    );
    if resp.has_focus() {
        bufs.insert(key, text);
        None
    } else if resp.lost_focus() {
        bufs.remove(&key);
        parse_time(text.trim(), fps, sr, fmt).filter(|&parsed| parsed != value)
    } else {
        bufs.remove(&key);
        None
    }
}

impl App {
    pub(crate) fn edit_details_tab(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let mut copy_tsv = false;
        let mut add_marker = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(tr(lang, "Exibir:")).strong().small());
            egui::ComboBox::from_id_salt("edit_details_category")
                .selected_text(self.edit_details.category.label_in(lang))
                .show_ui(ui, |ui| {
                    for cat in DetailsCategory::ALL {
                        if ui
                            .selectable_value(
                                &mut self.edit_details.category,
                                cat,
                                cat.label_in(lang),
                            )
                            .changed()
                        {
                            self.edit_details.edit_buf.clear();
                        }
                    }
                });
            ui.label(tr(lang, "Filtro:"));
            ui.add(egui::TextEdit::singleline(&mut self.edit_details.filter).desired_width(110.0));
            if ui
                .button(tr(lang, "Copiar TSV"))
                .on_hover_text(tr(lang, "Copia a tabela visível separada por tabulação"))
                .clicked()
            {
                copy_tsv = true;
            }
            if self.edit_details.category == DetailsCategory::Markers
                && ui.button(tr(lang, "＋ Marcador no Cursor")).clicked()
            {
                add_marker = true;
            }
        });
        if add_marker {
            let ctx = ui.ctx().clone();
            self.exec_cmd(Some(&ctx), "insert.marker");
        }
        if copy_tsv {
            let tsv = self.build_details_tsv();
            ui.ctx().copy_text(tsv);
            self.set_status(tr(
                lang,
                "Tabela copiada para a área de transferência (TSV).",
            ));
        }
        ui.separator();

        let filter = self.edit_details.filter.trim().to_ascii_lowercase();
        egui::ScrollArea::both()
            .id_salt("edit_details_scroll")
            .show(ui, |ui| match self.edit_details.category {
                DetailsCategory::Events => self.details_events_table(ui, &filter),
                DetailsCategory::Markers => self.details_markers_table(ui, &filter),
                DetailsCategory::Regions => self.details_regions_table(ui, &filter),
                DetailsCategory::Tracks => self.details_tracks_table(ui, &filter),
                DetailsCategory::Buses => self.details_buses_table(ui, &filter),
            });
    }

    fn details_events_table(&mut self, ui: &mut egui::Ui, filter: &str) {
        let lang = self.lang();
        let fps = self.project.settings.fps;
        let sr = self.project.settings.sample_rate;
        let fmt = self.opts.time_format;
        let min_len = self.project.settings.frame_dur().max(Ticks::from_raw(1));

        struct Row {
            ti: usize,
            ei: usize,
            id: Id,
            track_name: String,
            kind: TrackKind,
            name: String,
            start: Ticks,
            length: Ticks,
            offset: Ticks,
            rate: f64,
            gain_db: f32,
            mute: bool,
            locked: bool,
        }

        let mut rows = Vec::new();
        for (ti, t) in self.project.tracks.iter().enumerate() {
            for (ei, e) in t.events.iter().enumerate() {
                if !filter.is_empty()
                    && !e.name.to_ascii_lowercase().contains(filter)
                    && !t.name.to_ascii_lowercase().contains(filter)
                {
                    continue;
                }
                rows.push(Row {
                    ti,
                    ei,
                    id: e.id,
                    track_name: format!("#{} {}", ti + 1, t.name),
                    kind: t.kind,
                    name: e.name.clone(),
                    start: e.start,
                    length: e.length,
                    offset: e.offset,
                    rate: e.rate.as_f64(),
                    gain_db: e.gain_db,
                    mute: e.mute,
                    locked: e.locked,
                });
            }
        }

        if rows.is_empty() {
            ui.label(
                egui::RichText::new(tr(lang, "Nenhum evento correspondente."))
                    .color(self.theme.text_muted),
            );
            return;
        }

        let mut goto: Option<(usize, Id, Ticks)> = None;
        let mut edit_name: Option<(usize, usize, String)> = None;
        let mut edit_start: Option<(usize, usize, Ticks)> = None;
        let mut edit_len: Option<(usize, usize, Ticks)> = None;
        let mut edit_off: Option<(usize, usize, Ticks)> = None;
        let mut edit_rate: Option<(usize, usize, Rate)> = None;
        let mut edit_gain: Option<(usize, usize, f32)> = None;
        let mut toggle_mute: Option<(usize, usize)> = None;
        let mut toggle_lock: Option<(usize, usize)> = None;

        egui::Grid::new("ed_events_grid")
            .striped(true)
            .min_col_width(52.0)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                for hdr in [
                    "Trilha",
                    "Evento",
                    "Início",
                    "Fim",
                    "Duração",
                    "Offset",
                    "Taxa",
                    "Ganho",
                    "M",
                    "Bloq",
                    "Ir",
                ] {
                    ui.label(egui::RichText::new(tr(lang, hdr)).strong().small());
                }
                ui.end_row();

                for mut row in rows {
                    ui.label(egui::RichText::new(&row.track_name).small());
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut row.name)
                                .desired_width(110.0)
                                .font(egui::TextStyle::Small),
                        )
                        .changed()
                    {
                        edit_name = Some((row.ti, row.ei, row.name.clone()));
                    }
                    if let Some(new_t) = time_cell_editor(
                        ui,
                        &mut self.edit_details.edit_buf,
                        format!("ev_st_{}", row.id),
                        row.start,
                        fps,
                        sr,
                        fmt,
                    ) {
                        edit_start = Some((
                            row.ti,
                            row.ei,
                            new_t.clamp_safe(Ticks::ZERO, Ticks::from_raw(MAX_TICKS)),
                        ));
                    }
                    ui.label(
                        egui::RichText::new(format_ticks(row.start + row.length, fps, sr, fmt))
                            .monospace()
                            .small(),
                    );
                    if let Some(new_l) = time_cell_editor(
                        ui,
                        &mut self.edit_details.edit_buf,
                        format!("ev_len_{}", row.id),
                        row.length,
                        fps,
                        sr,
                        fmt,
                    ) {
                        edit_len = Some((
                            row.ti,
                            row.ei,
                            new_l.clamp_safe(min_len, Ticks::from_raw(MAX_TICKS)),
                        ));
                    }
                    if let Some(new_o) = time_cell_editor(
                        ui,
                        &mut self.edit_details.edit_buf,
                        format!("ev_off_{}", row.id),
                        row.offset,
                        fps,
                        sr,
                        fmt,
                    ) {
                        edit_off = Some((
                            row.ti,
                            row.ei,
                            new_o.clamp_safe(Ticks::ZERO, Ticks::from_raw(MAX_TICKS)),
                        ));
                    }
                    let mut rate_val = row.rate;
                    if ui
                        .add(
                            egui::DragValue::new(&mut rate_val)
                                .speed(0.01)
                                .range(0.1..=4.0)
                                .fixed_decimals(2)
                                .suffix("×"),
                        )
                        .changed()
                    {
                        if let Some(r) = Rate::from_f64(rate_val) {
                            edit_rate = Some((row.ti, row.ei, r));
                        }
                    }
                    if row.kind == TrackKind::Audio {
                        let mut g = row.gain_db;
                        if ui
                            .add(
                                egui::DragValue::new(&mut g)
                                    .speed(0.1)
                                    .range(-60.0..=12.0)
                                    .fixed_decimals(1)
                                    .suffix(" dB"),
                            )
                            .changed()
                        {
                            edit_gain = Some((row.ti, row.ei, g));
                        }
                    } else {
                        ui.label(egui::RichText::new("—").small().weak());
                    }
                    if ui.selectable_label(row.mute, "M").clicked() {
                        toggle_mute = Some((row.ti, row.ei));
                    }
                    if ui.selectable_label(row.locked, "L").clicked() {
                        toggle_lock = Some((row.ti, row.ei));
                    }
                    if ui.small_button(tr(lang, "Ir")).clicked() {
                        goto = Some((row.ti, row.id, row.start));
                    }
                    ui.end_row();
                }
            });

        let mut dirty = false;
        if let Some((ti, ei, s)) = edit_name {
            if let Some(e) = self
                .project
                .tracks
                .get_mut(ti)
                .and_then(|t| t.events.get_mut(ei))
            {
                e.name = s;
                dirty = true;
            }
        }
        if let Some((ti, ei, v)) = edit_start {
            self.checkpoint();
            if let Some(e) = self
                .project
                .tracks
                .get_mut(ti)
                .and_then(|t| t.events.get_mut(ei))
            {
                e.start = v;
                self.project.sort_events();
                dirty = true;
            }
        }
        if let Some((ti, ei, v)) = edit_len {
            self.checkpoint();
            if let Some(e) = self
                .project
                .tracks
                .get_mut(ti)
                .and_then(|t| t.events.get_mut(ei))
            {
                e.length = v;
                dirty = true;
            }
        }
        if let Some((ti, ei, v)) = edit_off {
            self.checkpoint();
            if let Some(e) = self
                .project
                .tracks
                .get_mut(ti)
                .and_then(|t| t.events.get_mut(ei))
            {
                e.offset = v;
                dirty = true;
            }
        }
        if let Some((ti, ei, r)) = edit_rate {
            self.checkpoint();
            if let Some(e) = self
                .project
                .tracks
                .get_mut(ti)
                .and_then(|t| t.events.get_mut(ei))
            {
                e.rate = r;
                dirty = true;
            }
        }
        if let Some((ti, ei, g)) = edit_gain {
            self.checkpoint();
            if let Some(e) = self
                .project
                .tracks
                .get_mut(ti)
                .and_then(|t| t.events.get_mut(ei))
            {
                e.gain_db = g;
                dirty = true;
            }
        }
        if let Some((ti, ei)) = toggle_mute {
            self.checkpoint();
            if let Some(e) = self
                .project
                .tracks
                .get_mut(ti)
                .and_then(|t| t.events.get_mut(ei))
            {
                e.mute = !e.mute;
                dirty = true;
            }
        }
        if let Some((ti, ei)) = toggle_lock {
            self.checkpoint();
            if let Some(e) = self
                .project
                .tracks
                .get_mut(ti)
                .and_then(|t| t.events.get_mut(ei))
            {
                e.locked = !e.locked;
                dirty = true;
            }
        }
        if let Some((ti, id, t)) = goto {
            self.sel_track = Some(ti);
            self.sel_events.clear();
            self.sel_events.insert(id);
            self.set_cursor(t);
        }
        if dirty {
            self.changed();
        }
    }

    fn details_markers_table(&mut self, ui: &mut egui::Ui, filter: &str) {
        let lang = self.lang();
        let fps = self.project.settings.fps;
        let sr = self.project.settings.sample_rate;
        let fmt = self.opts.time_format;
        let mut items: Vec<(usize, Marker)> = self
            .project
            .markers
            .iter()
            .enumerate()
            .filter(|(_, m)| filter.is_empty() || m.label.to_ascii_lowercase().contains(filter))
            .map(|(i, m)| (i, m.clone()))
            .collect();
        if items.is_empty() {
            ui.label(
                egui::RichText::new(tr(lang, "Nenhum marcador no projeto."))
                    .color(self.theme.text_muted),
            );
            return;
        }
        let mut rename: Option<(usize, String)> = None;
        let mut retime: Option<(usize, Ticks)> = None;
        let mut remove: Option<usize> = None;
        let mut jump: Option<Ticks> = None;

        egui::Grid::new("ed_markers_grid")
            .striped(true)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                for h in ["#", "Rótulo", "Tempo", "Ir", "×"] {
                    let lbl = if matches!(h, "#" | "×") {
                        h
                    } else {
                        tr(lang, h)
                    };
                    ui.label(egui::RichText::new(lbl).strong().small());
                }
                ui.end_row();
                for (row_idx, (i, m)) in items.iter_mut().enumerate() {
                    ui.label(format!("{}", row_idx + 1));
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut m.label)
                                .desired_width(160.0)
                                .font(egui::TextStyle::Small),
                        )
                        .changed()
                    {
                        rename = Some((*i, m.label.clone()));
                    }
                    if let Some(nt) = time_cell_editor(
                        ui,
                        &mut self.edit_details.edit_buf,
                        format!("mk_t_{}", m.id),
                        m.t,
                        fps,
                        sr,
                        fmt,
                    ) {
                        retime = Some((*i, nt.clamp_safe(Ticks::ZERO, Ticks::from_raw(MAX_TICKS))));
                    }
                    if ui.small_button(tr(lang, "Ir")).clicked() {
                        jump = Some(m.t);
                    }
                    if ui.small_button("×").clicked() {
                        remove = Some(*i);
                    }
                    ui.end_row();
                }
            });

        if let Some((i, lbl)) = rename {
            if let Some(m) = self.project.markers.get_mut(i) {
                m.label = lbl;
                self.dirty = true;
            }
        }
        if let Some((i, t)) = retime {
            self.checkpoint();
            if let Some(m) = self.project.markers.get_mut(i) {
                m.t = t;
                self.project.markers.sort_by_key(|x| x.t);
                self.changed();
            }
        }
        if let Some(i) = remove {
            if i < self.project.markers.len() {
                self.checkpoint();
                self.project.markers.remove(i);
                self.changed();
            }
        }
        if let Some(t) = jump {
            self.set_cursor(t);
        }
    }

    fn details_regions_table(&mut self, ui: &mut egui::Ui, filter: &str) {
        let lang = self.lang();
        let fps = self.project.settings.fps;
        let sr = self.project.settings.sample_rate;
        let fmt = self.opts.time_format;
        let min_len = self.project.settings.frame_dur().max(Ticks::from_raw(1));
        let mut items: Vec<(usize, Region)> = self
            .project
            .regions
            .iter()
            .enumerate()
            .filter(|(_, r)| filter.is_empty() || r.label.to_ascii_lowercase().contains(filter))
            .map(|(i, r)| (i, r.clone()))
            .collect();
        if items.is_empty() {
            ui.label(
                egui::RichText::new(tr(lang, "Nenhuma região no projeto."))
                    .color(self.theme.text_muted),
            );
            return;
        }
        let mut rename: Option<(usize, String)> = None;
        let mut restart: Option<(usize, Ticks)> = None;
        let mut reend: Option<(usize, Ticks)> = None;
        let mut select: Option<(Ticks, Ticks)> = None;
        let mut remove: Option<usize> = None;

        egui::Grid::new("ed_regions_grid")
            .striped(true)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                for h in ["#", "Rótulo", "Início", "Fim", "Duração", "Selecionar", "×"] {
                    let lbl = if matches!(h, "#" | "×") {
                        h
                    } else {
                        tr(lang, h)
                    };
                    ui.label(egui::RichText::new(lbl).strong().small());
                }
                ui.end_row();
                for (row_idx, (i, reg)) in items.iter_mut().enumerate() {
                    ui.label(format!("{}", row_idx + 1));
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut reg.label)
                                .desired_width(140.0)
                                .font(egui::TextStyle::Small),
                        )
                        .changed()
                    {
                        rename = Some((*i, reg.label.clone()));
                    }
                    if let Some(ns) = time_cell_editor(
                        ui,
                        &mut self.edit_details.edit_buf,
                        format!("rg_s_{}", reg.id),
                        reg.start,
                        fps,
                        sr,
                        fmt,
                    ) {
                        restart = Some((*i, ns.clamp_safe(Ticks::ZERO, reg.end - min_len)));
                    }
                    if let Some(ne) = time_cell_editor(
                        ui,
                        &mut self.edit_details.edit_buf,
                        format!("rg_e_{}", reg.id),
                        reg.end,
                        fps,
                        sr,
                        fmt,
                    ) {
                        reend = Some((
                            *i,
                            ne.clamp_safe(reg.start + min_len, Ticks::from_raw(MAX_TICKS)),
                        ));
                    }
                    ui.label(
                        egui::RichText::new(format_ticks(
                            (reg.end - reg.start).max(Ticks::ZERO),
                            fps,
                            sr,
                            fmt,
                        ))
                        .monospace()
                        .small(),
                    );
                    if ui.small_button(tr(lang, "Selecionar")).clicked() {
                        select = Some((reg.start, reg.end));
                    }
                    if ui.small_button("×").clicked() {
                        remove = Some(*i);
                    }
                    ui.end_row();
                }
            });

        if let Some((i, lbl)) = rename {
            if let Some(r) = self.project.regions.get_mut(i) {
                r.label = lbl;
                self.dirty = true;
            }
        }
        if let Some((i, s)) = restart {
            self.checkpoint();
            if let Some(r) = self.project.regions.get_mut(i) {
                r.start = s;
                self.changed();
            }
        }
        if let Some((i, e)) = reend {
            self.checkpoint();
            if let Some(r) = self.project.regions.get_mut(i) {
                r.end = e;
                self.changed();
            }
        }
        if let Some((a, b)) = select {
            self.time_sel = Some((a, b));
            self.project.loop_region = Some((a, b));
            self.set_cursor(a);
        }
        if let Some(i) = remove {
            if i < self.project.regions.len() {
                self.checkpoint();
                self.project.regions.remove(i);
                self.changed();
            }
        }
    }

    fn details_tracks_table(&mut self, ui: &mut egui::Ui, filter: &str) {
        let lang = self.lang();
        let bus_choices: Vec<(Id, String)> = self
            .project
            .audio_buses
            .iter()
            .map(|b| (b.id, b.name.clone()))
            .collect();
        let n = self.project.tracks.len();
        egui::Grid::new("ed_tracks_grid")
            .striped(true)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                for h in [
                    "#",
                    "Tipo",
                    "Nome",
                    "Nível",
                    "Pan / Blend",
                    "Saída",
                    "M",
                    "S",
                    "Sel",
                ] {
                    let lbl = if h == "#" { h } else { tr(lang, h) };
                    ui.label(egui::RichText::new(lbl).strong().small());
                }
                ui.end_row();
                for ti in 0..n {
                    let t_kind = self.project.tracks[ti].kind;
                    let t_id = self.project.tracks[ti].id;
                    let mut name = self.project.tracks[ti].name.clone();
                    if !filter.is_empty() && !name.to_ascii_lowercase().contains(filter) {
                        continue;
                    }
                    ui.label(format!("{}", ti + 1));
                    ui.label(if t_kind == TrackKind::Video {
                        tr(lang, "Vídeo")
                    } else {
                        tr(lang, "Áudio")
                    });
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut name)
                                .desired_width(130.0)
                                .font(egui::TextStyle::Small),
                        )
                        .changed()
                    {
                        self.project.tracks[ti].name = name;
                        self.dirty = true;
                    }
                    if t_kind == TrackKind::Audio {
                        let mut vol = self.project.tracks[ti].volume_db;
                        if ui
                            .add(
                                egui::DragValue::new(&mut vol)
                                    .speed(0.2)
                                    .range(-60.0..=12.0)
                                    .fixed_decimals(1)
                                    .suffix(" dB"),
                            )
                            .changed()
                        {
                            self.checkpoint();
                            self.project.tracks[ti].volume_db = vol;
                            self.changed();
                        }
                        let mut pan = self.project.tracks[ti].pan;
                        if ui
                            .add(
                                egui::DragValue::new(&mut pan)
                                    .speed(0.02)
                                    .range(-1.0..=1.0)
                                    .fixed_decimals(2),
                            )
                            .changed()
                        {
                            self.checkpoint();
                            self.project.tracks[ti].pan = pan;
                            self.changed();
                        }
                        let out_lbl = self.project.tracks[ti]
                            .output_bus
                            .and_then(|bid| {
                                bus_choices
                                    .iter()
                                    .find(|(id, _)| *id == bid)
                                    .map(|(_, nm)| nm.as_str())
                            })
                            .unwrap_or_else(|| tr(lang, "Master"));
                        ui.label(egui::RichText::new(out_lbl).small());
                    } else {
                        let mut op = self.project.tracks[ti].opacity * 100.0;
                        if ui
                            .add(
                                egui::DragValue::new(&mut op)
                                    .speed(1.0)
                                    .range(0.0..=100.0)
                                    .fixed_decimals(0)
                                    .suffix(" %"),
                            )
                            .changed()
                        {
                            self.checkpoint();
                            self.project.tracks[ti].opacity = (op / 100.0).clamp(0.0, 1.0);
                            self.changed();
                        }
                        ui.label(
                            egui::RichText::new(self.project.tracks[ti].blend.name_in(lang))
                                .small(),
                        );
                        ui.label(egui::RichText::new(tr(lang, "Master Video")).small().weak());
                    }
                    let mute = self.project.tracks[ti].mute;
                    if ui.selectable_label(mute, "M").clicked() {
                        self.checkpoint();
                        self.project.tracks[ti].mute = !mute;
                        self.changed();
                    }
                    let solo = self.project.tracks[ti].solo;
                    if ui.selectable_label(solo, "S").clicked() {
                        self.checkpoint();
                        self.project.tracks[ti].solo = !solo;
                        self.changed();
                    }
                    if ui
                        .selectable_label(self.sel_track == Some(ti), tr(lang, "Selecionar"))
                        .clicked()
                    {
                        self.sel_track = Some(ti);
                        let _ = t_id;
                    }
                    ui.end_row();
                }
            });
    }

    fn details_buses_table(&mut self, ui: &mut egui::Ui, filter: &str) {
        let lang = self.lang();
        if self.project.audio_buses.is_empty() {
            ui.label(
                egui::RichText::new(tr(
                    lang,
                    "Nenhum barramento auxiliar. Adicione um barramento no Mixer.",
                ))
                .color(self.theme.text_muted),
            );
            return;
        }
        let bus_choices: Vec<(Id, String)> = self
            .project
            .audio_buses
            .iter()
            .map(|b| (b.id, b.name.clone()))
            .collect();
        let nb = self.project.audio_buses.len();
        egui::Grid::new("ed_buses_grid")
            .striped(true)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                for h in ["#", "Nome", "Saída", "Vol", "Pan", "M", "S", "FX"] {
                    let lbl = if h == "#" { h } else { tr(lang, h) };
                    ui.label(egui::RichText::new(lbl).strong().small());
                }
                ui.end_row();
                for bi in 0..nb {
                    let bid = self.project.audio_buses[bi].id;
                    let cur_name = self.project.audio_buses[bi].name.clone();
                    if !filter.is_empty() && !cur_name.to_ascii_lowercase().contains(filter) {
                        continue;
                    }
                    ui.label(format!("{}", bi + 1));
                    let buf_id = egui::Id::new(("ed_bus_name", bid));
                    let mut draft = ui.data_mut(|d| {
                        d.get_temp::<String>(buf_id)
                            .unwrap_or_else(|| cur_name.clone())
                    });
                    let nr = ui.add(
                        egui::TextEdit::singleline(&mut draft)
                            .desired_width(120.0)
                            .font(egui::TextStyle::Small),
                    );
                    if nr.has_focus() {
                        ui.data_mut(|d| d.insert_temp(buf_id, draft.clone()));
                    } else if nr.lost_focus() {
                        ui.data_mut(|d| d.remove::<String>(buf_id));
                        self.session.rename_audio_bus(bid, draft);
                    } else if draft != cur_name {
                        ui.data_mut(|d| d.remove::<String>(buf_id));
                    }

                    let out_lbl = self.project.audio_buses[bi]
                        .output_bus
                        .and_then(|out_id| {
                            bus_choices
                                .iter()
                                .find(|(id, _)| *id == out_id)
                                .map(|(_, nm)| nm.as_str())
                        })
                        .unwrap_or_else(|| tr(lang, "Master"));
                    ui.label(egui::RichText::new(out_lbl).small());

                    let mut vol = self.project.audio_buses[bi].volume_db;
                    if ui
                        .add(
                            egui::DragValue::new(&mut vol)
                                .speed(0.2)
                                .range(-60.0..=12.0)
                                .fixed_decimals(1)
                                .suffix(" dB"),
                        )
                        .changed()
                    {
                        self.checkpoint();
                        self.project.audio_buses[bi].volume_db = vol;
                        self.changed();
                    }
                    let mut pan = self.project.audio_buses[bi].pan;
                    if ui
                        .add(
                            egui::DragValue::new(&mut pan)
                                .speed(0.02)
                                .range(-1.0..=1.0)
                                .fixed_decimals(2),
                        )
                        .changed()
                    {
                        self.checkpoint();
                        self.project.audio_buses[bi].pan = pan;
                        self.changed();
                    }
                    let mute = self.project.audio_buses[bi].mute;
                    if ui.selectable_label(mute, "M").clicked() {
                        self.checkpoint();
                        self.project.audio_buses[bi].mute = !mute;
                        self.changed();
                    }
                    let solo = self.project.audio_buses[bi].solo;
                    if ui.selectable_label(solo, "S").clicked() {
                        self.checkpoint();
                        self.project.audio_buses[bi].solo = !solo;
                        self.changed();
                    }
                    if ui.small_button("FX").clicked() {
                        self.dlg.fx_target = Some(FxTarget::AudioBus(bid));
                    }
                    ui.end_row();
                }
            });
    }

    fn build_details_tsv(&self) -> String {
        let lang = self.lang();
        let fps = self.project.settings.fps;
        let sr = self.project.settings.sample_rate;
        let fmt = self.opts.time_format;
        let mut out = String::new();
        match self.edit_details.category {
            DetailsCategory::Events => {
                out.push_str(&format!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                    tr(lang, "Trilha"),
                    tr(lang, "Evento"),
                    tr(lang, "Início"),
                    tr(lang, "Fim"),
                    tr(lang, "Duração"),
                    tr(lang, "Offset"),
                    tr(lang, "Taxa"),
                    tr(lang, "Ganho"),
                ));
                for (ti, t) in self.project.tracks.iter().enumerate() {
                    for e in &t.events {
                        out.push_str(&format!(
                            "#{}\t{}\t{}\t{}\t{}\t{}\t{:.2}\t{:+.1}\n",
                            ti + 1,
                            e.name,
                            format_ticks(e.start, fps, sr, fmt),
                            format_ticks(e.end(), fps, sr, fmt),
                            format_ticks(e.length, fps, sr, fmt),
                            format_ticks(e.offset, fps, sr, fmt),
                            e.rate.as_f64(),
                            e.gain_db,
                        ));
                    }
                }
            }
            DetailsCategory::Markers => {
                out.push_str(&format!(
                    "#\t{}\t{}\n",
                    tr(lang, "Rótulo"),
                    tr(lang, "Tempo")
                ));
                for (i, m) in self.project.markers.iter().enumerate() {
                    out.push_str(&format!(
                        "{}\t{}\t{}\n",
                        i + 1,
                        m.label,
                        format_ticks(m.t, fps, sr, fmt)
                    ));
                }
            }
            DetailsCategory::Regions => {
                out.push_str(&format!(
                    "#\t{}\t{}\t{}\t{}\n",
                    tr(lang, "Rótulo"),
                    tr(lang, "Início"),
                    tr(lang, "Fim"),
                    tr(lang, "Duração"),
                ));
                for (i, r) in self.project.regions.iter().enumerate() {
                    out.push_str(&format!(
                        "{}\t{}\t{}\t{}\t{}\n",
                        i + 1,
                        r.label,
                        format_ticks(r.start, fps, sr, fmt),
                        format_ticks(r.end, fps, sr, fmt),
                        format_ticks((r.end - r.start).max(Ticks::ZERO), fps, sr, fmt)
                    ));
                }
            }
            DetailsCategory::Tracks => {
                out.push_str(&format!(
                    "#\t{}\t{}\t{}\t{}\n",
                    tr(lang, "Tipo"),
                    tr(lang, "Nome"),
                    tr(lang, "Mute"),
                    tr(lang, "Solo"),
                ));
                for (i, t) in self.project.tracks.iter().enumerate() {
                    out.push_str(&format!(
                        "{}\t{}\t{}\t{}\t{}\n",
                        i + 1,
                        if t.kind == TrackKind::Video {
                            tr(lang, "Vídeo")
                        } else {
                            tr(lang, "Áudio")
                        },
                        t.name,
                        t.mute,
                        t.solo
                    ));
                }
            }
            DetailsCategory::Buses => {
                out.push_str(&format!(
                    "#\t{}\t{}\t{}\t{}\t{}\n",
                    tr(lang, "Nome"),
                    tr(lang, "Vol_dB"),
                    tr(lang, "Pan"),
                    tr(lang, "Mute"),
                    tr(lang, "Solo"),
                ));
                for (i, b) in self.project.audio_buses.iter().enumerate() {
                    out.push_str(&format!(
                        "{}\t{}\t{:+.1}\t{:.2}\t{}\t{}\n",
                        i + 1,
                        b.name,
                        b.volume_db,
                        b.pan,
                        b.mute,
                        b.solo
                    ));
                }
            }
        }
        out
    }
}
