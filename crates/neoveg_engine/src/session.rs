//! Estado editável de uma sessão de edição da timeline: projeto atual,
//! histórico de Desfazer/Refazer, área de transferência, seleção, cursor,
//! opções e regras de edição puras sem dependência de UI (AGENTS.md §3.2).

use crate::media::MediaCache;
use crate::model::*;
use crate::prefs::PreviewQuality;
use neoveg_common::{NeovegError, Result};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

pub const MAX_UNDO: usize = 200;

/// Modos de edição suportados pelo motor (§3.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EditMode {
    #[default]
    Normal,
    Slip,
    Slide,
    Stretch,
    Roll,
    Shuffle,
    Ripple,
}

impl EditMode {
    pub const ALL: [EditMode; 7] = [
        EditMode::Normal,
        EditMode::Slip,
        EditMode::Slide,
        EditMode::Stretch,
        EditMode::Roll,
        EditMode::Shuffle,
        EditMode::Ripple,
    ];

    pub fn name(self) -> &'static str {
        match self {
            EditMode::Normal => "Normal",
            EditMode::Slip => "Slip (deslizar conteúdo)",
            EditMode::Slide => "Slide (deslizar evento)",
            EditMode::Stretch => "Stretch (esticamento)",
            EditMode::Roll => "Roll (ajuste conjunto de corte)",
            EditMode::Shuffle => "Shuffle (reordenar)",
            EditMode::Ripple => "Ripple (ondulação)",
        }
    }
}

#[derive(Clone, Debug)]
pub struct EditorOptions {
    pub snapping: bool,
    pub quantize: bool,
    pub auto_ripple: bool,
    pub ignore_grouping: bool,
    pub show_envelopes: bool,
    pub show_waveforms: bool,
    pub show_thumbs: bool,
    pub show_bus_tracks: bool,
    pub time_format: TimeFormat,
    pub preview_quality: PreviewQuality,
    pub safe_areas: bool,
    pub loop_playback: bool,
    /// "Slip All Takes" (menu Opções, manual do VEGAS Pro 21 p. 346):
    /// o slip desloca todos os takes do evento, não só o ativo.
    pub slip_all_takes: bool,
    /// Zoom vertical das formas de onda de áudio, em passos de √2 (0 = 1×):
    /// Shift+↑/↓ (lista de atalhos do VEGAS Pro: "Zoom waveforms in/out
    /// vertically (audio only)"). Passos inteiros: subir e descer volta
    /// exatamente a 1×.
    pub waveform_zoom_steps: u8,
}

/// Maior zoom vertical das formas de onda: 12 passos de √2 = 64× (+36 dB).
pub const MAX_WAVEFORM_ZOOM_STEPS: u8 = 12;

impl EditorOptions {
    /// Fator do zoom vertical das formas de onda (1× a 64×).
    pub fn waveform_zoom(&self) -> f32 {
        let steps = self.waveform_zoom_steps.min(MAX_WAVEFORM_ZOOM_STEPS);
        2f32.powf(f32::from(steps) * 0.5)
    }
}

impl Default for EditorOptions {
    fn default() -> Self {
        Self {
            snapping: true,
            quantize: true,
            auto_ripple: false,
            ignore_grouping: false,
            show_envelopes: true,
            show_waveforms: true,
            show_thumbs: true,
            show_bus_tracks: false,
            time_format: TimeFormat::SmpteDrop,
            preview_quality: PreviewQuality::Preview,
            safe_areas: false,
            loop_playback: false,
            slip_all_takes: false,
            waveform_zoom_steps: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ClipItem {
    pub track_rel: usize,
    pub kind: TrackKind,
    pub ev: Event,
}

/// Estado puro da timeline e do projeto compartilhado entre GUI, CLI e scripting.
#[derive(Clone)]
pub struct EditorSession {
    pub project: Project,
    pub snapshot: Arc<Project>,
    pub generation: u64,
    pub sent_generation: u64,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub undo: Vec<Project>,
    pub redo: Vec<Project>,
    pub cursor: Ticks,
    pub play_start: Ticks,
    pub sel_events: HashSet<Id>,
    pub sel_track: Option<usize>,
    pub time_sel: Option<(Ticks, Ticks)>,
    pub clipboard: Vec<ClipItem>,
    pub clip_base: Ticks,
    pub opts: EditorOptions,
    pub edit_mode: EditMode,
    pub status: String,
    pub removed_media_ids: Vec<Id>,
    /// Intervalos da timeline pré-renderizados (`Shift+M`), limpos ao editar o projeto.
    pub prerendered_ranges: Vec<(Ticks, Ticks)>,
}

impl EditorSession {
    pub fn new(project: Project) -> Self {
        let snapshot = Arc::new(project.clone());
        crate::recovery::remember_snapshot(snapshot.clone());
        let sel_track = if project.tracks.is_empty() {
            None
        } else {
            Some(0)
        };
        Self {
            project,
            snapshot,
            generation: 1,
            sent_generation: 0,
            path: None,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            cursor: Ticks::ZERO,
            play_start: Ticks::ZERO,
            sel_events: HashSet::new(),
            sel_track,
            time_sel: None,
            clipboard: Vec::new(),
            clip_base: Ticks::ZERO,
            opts: EditorOptions::default(),
            edit_mode: EditMode::Normal,
            status: "Pronto. Arraste arquivos de mídia para a timeline ou use Ctrl+I.".into(),
            removed_media_ids: Vec::new(),
            prerendered_ranges: Vec::new(),
        }
    }

    /// Registra um trecho pré-renderizado e mescla intervalos adjacentes/sobrepostos.
    pub fn record_prerendered_range(&mut self, start: Ticks, end: Ticks) {
        let a = start.max(Ticks::ZERO);
        if end <= a {
            return;
        }
        self.prerendered_ranges.push((a, end));
        self.prerendered_ranges.sort_by_key(|(s, _)| *s);
        let mut merged: Vec<(Ticks, Ticks)> = Vec::with_capacity(self.prerendered_ranges.len());
        for (s, e) in self.prerendered_ranges.drain(..) {
            if let Some(last) = merged.last_mut() {
                if s <= last.1 {
                    last.1 = last.1.max(e);
                    continue;
                }
            }
            merged.push((s, e));
        }
        if merged.len() > 256 {
            merged.drain(..merged.len() - 256);
        }
        self.prerendered_ranges = merged;
    }

    pub fn clear_prerendered(&mut self) {
        self.prerendered_ranges.clear();
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
    }

    /// Salva um ponto de desfazer antes de uma modificação.
    pub fn checkpoint(&mut self) {
        self.undo.push(self.project.clone());
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Marca o projeto como modificado (incrementa geração para prévia e áudio).
    pub fn changed(&mut self) {
        self.generation += 1;
        self.dirty = true;
        self.prerendered_ranges.clear();
    }

    /// Atualiza o snapshot imutável quando `generation != sent_generation`.
    pub fn refresh_snapshot(&mut self) -> bool {
        if self.generation != self.sent_generation {
            self.snapshot = Arc::new(self.project.clone());
            self.sent_generation = self.generation;
            crate::recovery::remember_snapshot(self.snapshot.clone());
            true
        } else {
            false
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(p) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(&mut self.project, p));
        self.after_history();
        self.set_status("Desfazer");
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(p) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.project, p));
        self.after_history();
        self.set_status("Refazer");
        true
    }

    fn after_history(&mut self) {
        self.sel_events
            .retain(|id| self.project.find_event(*id).is_some());
        if let Some(t) = self.sel_track {
            if t >= self.project.tracks.len() {
                self.sel_track = if self.project.tracks.is_empty() {
                    None
                } else {
                    Some(0)
                };
            }
        }
        self.generation += 1;
        self.dirty = true;
        self.prerendered_ranges.clear();
    }

    pub fn reset_project(&mut self, project: Project, path: Option<PathBuf>) {
        self.project = project;
        self.path = path;
        self.dirty = false;
        self.undo.clear();
        self.redo.clear();
        self.cursor = Ticks::ZERO;
        self.play_start = Ticks::ZERO;
        self.sel_events.clear();
        self.time_sel = None;
        self.prerendered_ranges.clear();
        self.sel_track = if self.project.tracks.is_empty() {
            None
        } else {
            Some(0)
        };
        self.generation += 1;
        self.sent_generation = 0;
        self.refresh_snapshot();
    }

    pub fn insert_track(&mut self, kind: TrackKind) -> usize {
        let n = self
            .project
            .tracks
            .iter()
            .filter(|t| t.kind == kind)
            .count()
            + 1;
        let name = match kind {
            TrackKind::Video => format!("Vídeo {n}"),
            TrackKind::Audio => format!("Áudio {n}"),
        };
        let t = Track::new(kind, &name);
        // vídeo: insere acima das trilhas de áudio; áudio: no final
        let idx = match kind {
            TrackKind::Video => match self.sel_track {
                Some(i) if self.project.tracks.get(i).map(|t| t.kind) == Some(TrackKind::Video) => {
                    i
                }
                _ => self
                    .project
                    .tracks
                    .iter()
                    .position(|t| t.kind == TrackKind::Audio)
                    .unwrap_or(self.project.tracks.len()),
            },
            TrackKind::Audio => self.project.tracks.len(),
        };
        self.project.tracks.insert(idx, t);
        self.sel_track = Some(idx);
        self.changed();
        idx
    }

    /// Insere uma trilha de ajuste (*Adjustment Track*) de vídeo, na mesma
    /// posição em que `insert_track(TrackKind::Video)` colocaria uma trilha.
    pub fn insert_adjustment_track(&mut self) -> usize {
        let n = self
            .project
            .tracks
            .iter()
            .filter(|t| t.is_adjustment())
            .count()
            + 1;
        let idx = self.insert_track(TrackKind::Video);
        if let Some(t) = self.project.tracks.get_mut(idx) {
            t.adjustment = true;
            t.name = format!("Ajuste {n}");
        }
        self.changed();
        idx
    }

    pub fn add_audio_bus(&mut self) -> Option<Id> {
        if self.project.audio_buses.len() >= MAX_AUDIO_BUSES {
            return None;
        }
        self.checkpoint();
        let id = self.project.add_audio_bus(None)?;
        self.changed();
        Some(id)
    }

    pub fn remove_audio_bus(&mut self, bus_id: Id) -> bool {
        let Some(pos) = self.project.audio_buses.iter().position(|b| b.id == bus_id) else {
            return false;
        };
        self.checkpoint();
        self.project.audio_buses.remove(pos);
        for t in &mut self.project.tracks {
            if t.output_bus == Some(bus_id) {
                t.output_bus = None;
            }
            t.sends.retain(|s| s.bus_id != bus_id);
        }
        for b in &mut self.project.audio_buses {
            if b.output_bus == Some(bus_id) {
                b.output_bus = None;
            }
        }
        self.changed();
        true
    }

    pub fn rename_audio_bus(&mut self, bus_id: Id, name: String) -> bool {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return false;
        }
        let Some(bus) = self.project.audio_bus_mut(bus_id) else {
            return false;
        };
        if bus.name == trimmed {
            return false;
        }
        let new_name = trimmed.to_string();
        self.checkpoint();
        if let Some(bus) = self.project.audio_bus_mut(bus_id) {
            bus.name = new_name;
        }
        self.changed();
        true
    }

    pub fn toggle_bus_envelope(&mut self, bus_id: Id, kind: EnvKind) -> bool {
        if !matches!(kind, EnvKind::Volume | EnvKind::Pan) {
            return false;
        }
        let Some(pos) = self.project.audio_buses.iter().position(|b| b.id == bus_id) else {
            return false;
        };
        self.checkpoint();
        let bus = &mut self.project.audio_buses[pos];
        if bus.envelope(kind).is_some() {
            bus.envelopes.retain(|e| e.kind != kind);
        } else {
            bus.envelopes.push(Envelope::new(kind));
            self.opts.show_envelopes = true;
            self.opts.show_bus_tracks = true;
        }
        self.changed();
        true
    }

    /// Ids dos eventos selecionados expandidos pelos grupos (a menos que o
    /// agrupamento esteja sendo ignorado).
    pub fn expand_groups(&self, ids: &HashSet<Id>) -> HashSet<Id> {
        if self.opts.ignore_grouping {
            return ids.clone();
        }
        let groups: HashSet<Id> = ids
            .iter()
            .filter_map(|id| self.project.event(*id).and_then(|e| e.group))
            .collect();
        let mut out = ids.clone();
        if groups.is_empty() {
            return out;
        }
        for t in &self.project.tracks {
            for e in &t.events {
                if e.group.map(|g| groups.contains(&g)).unwrap_or(false) {
                    out.insert(e.id);
                }
            }
        }
        out
    }

    pub fn copy_selection(&mut self) {
        if self.sel_events.is_empty() {
            return;
        }
        let mut items = vec![];
        let mut min_track = usize::MAX;
        let mut min_t = Ticks::MAX;
        let mut any = false;
        for (ti, t) in self.project.tracks.iter().enumerate() {
            for e in &t.events {
                if self.sel_events.contains(&e.id) {
                    min_track = min_track.min(ti);
                    min_t = min_t.min(e.start);
                    any = true;
                }
            }
        }
        if !any {
            return;
        }
        for (ti, t) in self.project.tracks.iter().enumerate() {
            for e in &t.events {
                if self.sel_events.contains(&e.id) {
                    items.push(ClipItem {
                        track_rel: ti - min_track,
                        kind: t.kind,
                        ev: e.clone(),
                    });
                }
            }
        }
        self.clip_base = min_t;
        self.set_status(format!("{} evento(s) copiado(s)", items.len()));
        self.clipboard = items;
    }

    pub fn cut_selection(&mut self) {
        self.copy_selection();
        self.delete_selection();
    }

    /// Cola no cursor, na trilha selecionada. `insert` = desloca eventos posteriores (paste insert).
    pub fn paste(&mut self, insert: bool) {
        if self.clipboard.is_empty() {
            return;
        }
        self.checkpoint();
        let base_track = self.sel_track.unwrap_or(0);
        let t0 = self.cursor;
        let max_end = self
            .clipboard
            .iter()
            .map(|c| c.ev.end())
            .fold(self.clip_base, Ticks::max);
        let span = max_end - self.clip_base;
        let mut group_map: HashMap<Id, Id> = HashMap::new();
        let mut new_sel = HashSet::new();
        let items = self.clipboard.clone();
        if insert {
            let tracks: HashSet<usize> = items
                .iter()
                .filter_map(|c| self.target_track(base_track + c.track_rel, c.kind))
                .collect();
            for ti in tracks {
                if let Some(track) = self.project.tracks.get_mut(ti) {
                    for e in &mut track.events {
                        if e.start >= t0 {
                            e.start += span;
                        }
                    }
                }
            }
        }
        for c in items {
            let ti = match self.target_track(base_track + c.track_rel, c.kind) {
                Some(t) => t,
                None => self.insert_track(c.kind),
            };
            let mut e = c.ev.clone();
            e.id = new_id();
            e.start = t0 + (c.ev.start - self.clip_base);
            e.group = e.group.map(|g| *group_map.entry(g).or_insert_with(new_id));
            e.fx = e.fx.iter().map(|f| f.duplicate()).collect();
            new_sel.insert(e.id);
            if let Some(track) = self.project.tracks.get_mut(ti) {
                track.events.push(e);
            }
        }
        self.sel_events = new_sel;
        self.cursor = t0 + span;
        self.changed();
    }

    fn target_track(&self, idx: usize, kind: TrackKind) -> Option<usize> {
        (idx..self.project.tracks.len()).find(|&i| {
            self.project
                .tracks
                .get(i)
                .map(|t| t.accepts(kind))
                .unwrap_or(false)
        })
    }

    pub fn delete_selection(&mut self) {
        if self.sel_events.is_empty() {
            if self.time_sel.is_some() && self.opts.auto_ripple {
                self.delete_time_range();
            }
            return;
        }
        self.checkpoint();
        let ids = self.expand_groups(&self.sel_events.clone());
        let ripple = self.opts.auto_ripple;
        for t in &mut self.project.tracks {
            let removed: Vec<(Ticks, Ticks)> = t
                .events
                .iter()
                .filter(|e| ids.contains(&e.id))
                .map(|e| (e.start, e.length))
                .collect();
            t.events.retain(|e| !ids.contains(&e.id));
            if ripple {
                let mut removed = removed;
                removed.sort_by_key(|a| std::cmp::Reverse(a.0));
                for (start, len) in removed {
                    for e in &mut t.events {
                        if e.start >= start + len {
                            e.start -= len;
                        }
                    }
                }
            }
        }
        self.sel_events.clear();
        self.changed();
    }

    /// Divide eventos no cursor (selecionados, ou todos sob o cursor se nada
    /// estiver selecionado). Retorna o número de eventos divididos.
    /// Aplica `f` a cada evento selecionado (sem expandir grupos: os takes
    /// são de cada evento). Um passo de Desfazer se algum evento mudou.
    fn edit_selected_takes(&mut self, f: impl Fn(&mut Event) -> bool) -> usize {
        let ids: Vec<Id> = self.sel_events.iter().copied().collect();
        let mut probe = self.project.clone();
        let mut n = 0;
        for id in ids {
            if let Some(e) = probe.event_mut(id) {
                if !e.locked && f(e) {
                    n += 1;
                }
            }
        }
        if n > 0 {
            self.checkpoint();
            self.project = probe;
            self.changed();
        }
        n
    }

    /// Próximo take dos eventos selecionados (T).
    pub fn next_take_selected(&mut self) -> usize {
        self.edit_selected_takes(crate::takes::next_take)
    }

    /// Take anterior dos eventos selecionados (Shift+T).
    pub fn prev_take_selected(&mut self) -> usize {
        self.edit_selected_takes(crate::takes::prev_take)
    }

    /// Apaga o take ativo dos eventos selecionados que têm mais de um take.
    pub fn delete_active_take_selected(&mut self) -> usize {
        self.edit_selected_takes(crate::takes::delete_active)
    }

    /// Algum evento selecionado tem mais de um take?
    pub fn selection_has_takes(&self) -> bool {
        self.sel_events.iter().any(|id| {
            self.project
                .event(*id)
                .is_some_and(|e| crate::takes::take_count(e) > 1)
        })
    }

    /// "Add As Takes" (p. 346): adiciona a mídia como take ativo de cada
    /// evento selecionado do mesmo tipo (vídeo/áudio); o take anterior fica.
    pub fn add_media_as_take_to_selected(&mut self, media_id: Id) -> usize {
        let Some(m) = self.project.media(media_id).cloned() else {
            return 0;
        };
        let ids: Vec<Id> = self.sel_events.iter().copied().collect();
        let mut probe = self.project.clone();
        let mut n = 0;
        for t in &mut probe.tracks {
            let fits = match t.kind {
                TrackKind::Video => m.has_video,
                TrackKind::Audio => m.has_audio,
            };
            if !fits {
                continue;
            }
            for e in &mut t.events {
                if ids.contains(&e.id) && !e.locked {
                    let take = crate::model::Take {
                        media_id,
                        offset: Ticks::ZERO,
                        name: String::new(),
                    };
                    if crate::takes::add_take(e, take, true) {
                        n += 1;
                    }
                }
            }
        }
        if n > 0 {
            self.checkpoint();
            self.project = probe;
            self.changed();
        }
        n
    }

    /// Torna ativo o take `k` do evento `id` (escolha no submenu Take).
    pub fn set_active_take(&mut self, id: Id, k: usize) -> bool {
        let mut probe = self.project.clone();
        let changed = probe
            .event_mut(id)
            .is_some_and(|e| !e.locked && crate::takes::set_active(e, k));
        if changed {
            self.checkpoint();
            self.project = probe;
            self.changed();
        }
        changed
    }

    pub fn split_at_cursor(&mut self) -> usize {
        let t = self.cursor;
        let targets: HashSet<Id> = if self.sel_events.is_empty() {
            self.project
                .tracks
                .iter()
                .flat_map(|tr| tr.events.iter())
                .filter(|e| e.start < t && e.end() > t)
                .map(|e| e.id)
                .collect()
        } else {
            self.expand_groups(&self.sel_events.clone())
        };
        let targets: Vec<Id> = targets
            .into_iter()
            .filter(|id| {
                self.project
                    .event(*id)
                    .map(|e| e.start < t && e.end() > t && !e.locked)
                    .unwrap_or(false)
            })
            .collect();
        if targets.is_empty() {
            return 0;
        }
        self.checkpoint();
        let mut group_map: HashMap<Id, Id> = HashMap::new();
        let mut new_sel = HashSet::new();
        let mut count = 0usize;
        for id in targets {
            let Some((ti, ei)) = self.project.find_event(id) else {
                continue;
            };
            let Some(track) = self.project.tracks.get_mut(ti) else {
                continue;
            };
            let Some(e) = track.events.get_mut(ei) else {
                continue;
            };
            let mut right = e.clone();
            let left_len = t - e.start;
            e.length = left_len;
            e.fade_out = Fade::default();
            right.id = new_id();
            right.start = t;
            right.length -= left_len;
            right.fade_in = Fade::default();
            right.transition = None;
            if !right.reverse {
                right.shift_offsets(right.rate.scale_ticks(left_len));
            }
            if e.reverse {
                e.shift_offsets(e.rate.scale_ticks(right.length));
            }
            for k in &mut right.pan_crop.keys {
                k.t -= left_len;
            }
            right.group = right
                .group
                .map(|g| *group_map.entry(g).or_insert_with(new_id));
            right.fx = right
                .fx
                .iter()
                .map(|f| {
                    let mut f = f.duplicate();
                    for p in &mut f.params {
                        for k in &mut p.keys {
                            k.0 -= left_len;
                        }
                    }
                    f
                })
                .collect();
            new_sel.insert(right.id);
            track.events.push(right);
            count += 1;
        }
        self.sel_events = new_sel;
        self.changed();
        count
    }

    pub fn trim_to_selection(&mut self) {
        let Some((a, b)) = self.time_sel else { return };
        if self.sel_events.is_empty() || b <= a {
            return;
        }
        self.checkpoint();
        let ids = self.expand_groups(&self.sel_events.clone());
        for t in &mut self.project.tracks {
            t.events.retain_mut(|e| {
                if !ids.contains(&e.id) {
                    return true;
                }
                if e.end() <= a || e.start >= b {
                    return false;
                }
                if e.start < a {
                    let d = a - e.start;
                    if !e.reverse {
                        e.shift_offsets(e.rate.scale_ticks(d));
                    }
                    e.start = a;
                    e.length -= d;
                }
                if e.end() > b {
                    let d = e.end() - b;
                    if e.reverse {
                        e.shift_offsets(e.rate.scale_ticks(d));
                    }
                    e.length -= d;
                }
                true
            });
        }
        self.changed();
    }

    pub fn trim_selected_start_to_cursor(&mut self) {
        if self.sel_events.is_empty() {
            return;
        }
        let c = self.cursor;
        self.checkpoint();
        let ids = self.expand_groups(&self.sel_events.clone());
        for id in ids {
            if let Some(e) = self.project.event_mut(id) {
                if c > e.start && c < e.end() {
                    let d = c - e.start;
                    if !e.reverse {
                        e.shift_offsets(e.rate.scale_ticks(d));
                    }
                    e.start = c;
                    e.length -= d;
                }
            }
        }
        self.changed();
    }

    pub fn trim_selected_end_to_cursor(&mut self) {
        if self.sel_events.is_empty() {
            return;
        }
        let c = self.cursor;
        self.checkpoint();
        let ids = self.expand_groups(&self.sel_events.clone());
        for id in ids {
            if let Some(e) = self.project.event_mut(id) {
                if c > e.start && c < e.end() {
                    let d = e.end() - c;
                    if e.reverse {
                        e.shift_offsets(e.rate.scale_ticks(d));
                    }
                    e.length = c - e.start;
                }
            }
        }
        self.changed();
    }

    /// Remove o intervalo da seleção de tempo de todas as trilhas (ripple).
    pub fn delete_time_range(&mut self) {
        let Some((a, b)) = self.time_sel else { return };
        if b <= a {
            return;
        }
        self.checkpoint();
        let len = b - a;
        let mut new_events: Vec<(usize, Event)> = vec![];
        for (ti, t) in self.project.tracks.iter_mut().enumerate() {
            let mut keep = vec![];
            for mut e in t.events.drain(..) {
                if e.end() <= a {
                    keep.push(e);
                } else if e.start >= b {
                    e.start -= len;
                    keep.push(e);
                } else if e.start >= a && e.end() <= b {
                    // removido
                } else if e.start < a && e.end() > b {
                    let mut right = e.clone();
                    right.id = new_id();
                    let cut = b - e.start;
                    right.start = a;
                    right.length = e.end() - b;
                    right.shift_offsets(right.rate.scale_ticks(cut));
                    right.fade_in = Fade::default();
                    e.length = a - e.start;
                    e.fade_out = Fade::default();
                    keep.push(e);
                    new_events.push((ti, right));
                } else if e.start < a {
                    e.length = a - e.start;
                    keep.push(e);
                } else {
                    let d = b - e.start;
                    e.shift_offsets(e.rate.scale_ticks(d));
                    e.length -= d;
                    e.start = a;
                    keep.push(e);
                }
            }
            t.events = keep;
        }
        for (ti, e) in new_events {
            if let Some(track) = self.project.tracks.get_mut(ti) {
                track.events.push(e);
            }
        }
        self.project.markers.retain(|m| m.t < a || m.t >= b);
        for m in &mut self.project.markers {
            if m.t >= b {
                m.t -= len;
            }
        }
        self.time_sel = None;
        self.cursor = a;
        self.changed();
    }

    /// Insere tempo vazio do tamanho da seleção (Insert Time).
    pub fn insert_time(&mut self) {
        let Some((a, b)) = self.time_sel else {
            self.set_status("Crie uma seleção de tempo para inserir tempo");
            return;
        };
        if b <= a {
            return;
        }
        self.checkpoint();
        let len = b - a;
        for t in &mut self.project.tracks {
            let mut extra = vec![];
            for e in &mut t.events {
                if e.start >= a {
                    e.start += len;
                } else if e.end() > a {
                    let mut right = e.clone();
                    right.id = new_id();
                    let cut = a - e.start;
                    right.start = b;
                    right.length = e.length - cut;
                    right.shift_offsets(right.rate.scale_ticks(cut));
                    right.fade_in = Fade::default();
                    e.length = cut;
                    e.fade_out = Fade::default();
                    extra.push(right);
                }
            }
            t.events.extend(extra);
        }
        for m in &mut self.project.markers {
            if m.t >= a {
                m.t += len;
            }
        }
        self.changed();
    }

    /// Fecha lacunas entre eventos da trilha selecionada.
    pub fn close_gaps(&mut self) {
        let Some(ti) = self.sel_track else { return };
        if ti >= self.project.tracks.len() {
            return;
        }
        self.checkpoint();
        let Some(t) = self.project.tracks.get_mut(ti) else {
            return;
        };
        t.events.sort_by_key(|e| e.start);
        let mut pos = t.events.first().map(|e| e.start).unwrap_or(Ticks::ZERO);
        for e in &mut t.events {
            if e.start > pos {
                e.start = pos;
            }
            pos = pos.max(e.end());
        }
        self.changed();
    }

    pub fn group_selected(&mut self) {
        if self.sel_events.len() < 2 {
            return;
        }
        self.checkpoint();
        let g = new_id();
        for id in self.sel_events.clone() {
            if let Some(e) = self.project.event_mut(id) {
                e.group = Some(g);
            }
        }
        self.set_status("Eventos agrupados");
        self.changed();
    }

    pub fn ungroup_selected(&mut self) {
        if self.sel_events.is_empty() {
            return;
        }
        self.checkpoint();
        for id in self.sel_events.clone() {
            if let Some(e) = self.project.event_mut(id) {
                e.group = None;
            }
        }
        self.set_status("Eventos desagrupados");
        self.changed();
    }

    pub fn select_group(&mut self) {
        let saved = self.opts.ignore_grouping;
        self.opts.ignore_grouping = false;
        self.sel_events = self.expand_groups(&self.sel_events.clone());
        self.opts.ignore_grouping = saved;
    }

    pub fn select_all(&mut self) {
        self.sel_events = self
            .project
            .tracks
            .iter()
            .flat_map(|t| t.events.iter().map(|e| e.id))
            .collect();
    }

    pub fn deselect_all(&mut self) {
        self.sel_events.clear();
    }

    pub fn set_selection_in(&mut self) {
        let c = self.cursor;
        let end = self
            .time_sel
            .map(|s| s.1)
            .filter(|e| *e > c)
            .unwrap_or(c + Ticks::from_secs_i64(5));
        self.time_sel = Some((c, end));
        self.project.loop_region = Some((c, end));
    }

    pub fn set_selection_out(&mut self) {
        let c = self.cursor;
        let start = self
            .time_sel
            .map(|s| s.0)
            .filter(|s| *s < c)
            .unwrap_or(Ticks::ZERO);
        self.time_sel = Some((start, c));
        self.project.loop_region = Some((start, c));
    }

    pub fn select_loop_region(&mut self) {
        if let Some(r) = self.project.loop_region {
            self.time_sel = Some(r);
        }
    }

    pub fn insert_marker(&mut self) {
        self.checkpoint();
        let n = self.project.markers.len() + 1;
        self.project.markers.push(Marker {
            id: new_id(),
            t: self.cursor,
            label: format!("Marcador {n}"),
        });
        self.changed();
    }

    pub fn insert_region(&mut self) {
        let (a, b) = match self.time_sel.or(self.project.loop_region) {
            Some(r) if r.1 > r.0 => r,
            _ => {
                self.set_status("Crie uma seleção de tempo para inserir uma região");
                return;
            }
        };
        self.checkpoint();
        let n = self.project.regions.len() + 1;
        self.project.regions.push(Region {
            id: new_id(),
            start: a,
            end: b,
            label: format!("Região {n}"),
        });
        self.changed();
    }

    /// Mostra/oculta (ou cria) o envelope da trilha selecionada; o tipo
    /// depende da trilha ser de áudio ou vídeo.
    pub fn toggle_envelope_for_selected(&mut self, audio_kind: EnvKind, video_kind: EnvKind) {
        let Some(ti) = self.sel_track else { return };
        let Some(track) = self.project.tracks.get(ti) else {
            return;
        };
        let kind = if track.kind == TrackKind::Audio {
            audio_kind
        } else {
            video_kind
        };
        if track.envelopes.iter().any(|e| e.kind == kind) {
            if let Some(e) = self
                .project
                .tracks
                .get_mut(ti)
                .and_then(|t| t.envelopes.iter_mut().find(|e| e.kind == kind))
            {
                e.visible = !e.visible;
            }
        } else {
            self.checkpoint();
            if let Some(track) = self.project.tracks.get_mut(ti) {
                track.envelopes.push(Envelope::new(kind));
                self.opts.show_envelopes = true;
            }
        }
        self.changed();
    }

    pub fn remove_envelope(&mut self, kind: EnvKind) {
        let Some(ti) = self.sel_track else { return };
        if ti >= self.project.tracks.len() {
            return;
        }
        self.checkpoint();
        let Some(track) = self.project.tracks.get_mut(ti) else {
            return;
        };
        let k = match (track.kind, kind) {
            (TrackKind::Video, EnvKind::Volume) => EnvKind::Opacity,
            (TrackKind::Video, EnvKind::Pan) => EnvKind::FadeToColor,
            _ => kind,
        };
        track.envelopes.retain(|e| e.kind != k);
        self.changed();
    }

    pub fn toggle_reverse(&mut self) {
        if self.sel_events.is_empty() {
            return;
        }
        self.checkpoint();
        for id in self.sel_events.clone() {
            if let Some(e) = self.project.event_mut(id) {
                e.reverse = !e.reverse;
            }
        }
        self.changed();
    }

    /// Congela o quadro sob o cursor: divide e cria um evento de imagem parada de 2 segundos.
    pub fn freeze_frame(&mut self) {
        let Some(id) = self.sel_events.iter().next().cloned() else {
            return;
        };
        let Some((ti, ei)) = self.project.find_event(id) else {
            return;
        };
        let Some(track) = self.project.tracks.get(ti) else {
            return;
        };
        if track.kind != TrackKind::Video {
            return;
        }
        let Some(e) = track.events.get(ei).cloned() else {
            return;
        };
        let c = self.cursor;
        if !e.contains(c) {
            self.set_status("Posicione o cursor sobre o evento");
            return;
        }
        let m = self.project.media(e.media_id).cloned();
        let mt = e
            .media_time(c, m.and_then(|m| m.duration))
            .unwrap_or(Ticks::ZERO);
        self.checkpoint();
        let two_s = Ticks::from_secs_i64(2);
        let mut fr = e.clone();
        fr.id = new_id();
        fr.start = c;
        fr.length = two_s;
        fr.offset = mt;
        // um quadro congelado é da mídia ativa; os outros takes não se aplicam
        fr.alt_takes.clear();
        fr.take_index = 0;
        fr.rate = Rate::FROZEN;
        fr.looped = false;
        fr.reverse = false;
        fr.group = None;
        fr.fade_in = Fade::default();
        fr.fade_out = Fade::default();
        fr.name = format!("{} (congelado)", e.name);
        if let Some(track) = self.project.tracks.get_mut(ti) {
            for ev in &mut track.events {
                if ev.start >= c {
                    ev.start += two_s;
                }
            }
            if let Some(original) = track.events.get_mut(ei) {
                if original.start < c && original.end() > c {
                    let left = c - original.start;
                    let mut right = original.clone();
                    right.id = new_id();
                    right.start = c + two_s;
                    right.length = original.length - left;
                    right.shift_offsets(right.rate.scale_ticks(left));
                    right.fade_in = Fade::default();
                    original.length = left;
                    original.fade_out = Fade::default();
                    track.events.push(right);
                }
            }
            track.events.push(fr);
        }
        self.changed();
    }

    pub fn delete_track(&mut self, ti: usize) {
        if ti >= self.project.tracks.len() {
            return;
        }
        self.checkpoint();
        self.project.tracks.remove(ti);
        self.sel_track = if self.project.tracks.is_empty() {
            None
        } else {
            Some(ti.min(self.project.tracks.len() - 1))
        };
        self.changed();
    }

    pub fn duplicate_track(&mut self, ti: usize) {
        let Some(orig) = self.project.tracks.get(ti).cloned() else {
            return;
        };
        self.checkpoint();
        let mut t = orig;
        t.id = new_id();
        t.name = format!("{} (cópia)", t.name);
        let mut gm: HashMap<Id, Id> = HashMap::new();
        for e in &mut t.events {
            e.id = new_id();
            e.group = e.group.map(|g| *gm.entry(g).or_insert_with(new_id));
            e.fx = e.fx.iter().map(|f| f.duplicate()).collect();
        }
        t.fx = t.fx.iter().map(|f| f.duplicate()).collect();
        self.project.tracks.insert(ti + 1, t);
        self.changed();
    }

    pub fn move_track(&mut self, ti: usize, up: bool) {
        let n = self.project.tracks.len();
        if ti >= n {
            return;
        }
        let to = if up {
            ti.checked_sub(1)
        } else {
            (ti + 1 < n).then_some(ti + 1)
        };
        if let Some(to) = to {
            self.checkpoint();
            self.project.tracks.swap(ti, to);
            self.sel_track = Some(to);
            self.changed();
        }
    }

    pub fn toggle_selected_track_mute(&mut self) -> bool {
        let Some(ti) = self.sel_track else {
            return false;
        };
        if ti >= self.project.tracks.len() {
            return false;
        }
        self.checkpoint();
        if let Some(t) = self.project.tracks.get_mut(ti) {
            t.mute = !t.mute;
        }
        self.changed();
        true
    }

    pub fn toggle_selected_track_solo(&mut self) -> bool {
        let Some(ti) = self.sel_track else {
            return false;
        };
        if ti >= self.project.tracks.len() {
            return false;
        }
        self.checkpoint();
        if let Some(t) = self.project.tracks.get_mut(ti) {
            t.solo = !t.solo;
        }
        self.changed();
        true
    }

    pub fn jump_marker_time(&self, forward: bool) -> Option<Ticks> {
        let mut pts: Vec<Ticks> = self.project.markers.iter().map(|m| m.t).collect();
        pts.extend(self.project.regions.iter().flat_map(|r| [r.start, r.end]));
        pts.push(Ticks::ZERO);
        pts.push(self.project.duration());
        pts.sort();
        let c = self.cursor;
        if forward {
            pts.into_iter().find(|t| *t > c)
        } else {
            pts.into_iter().rev().find(|t| *t < c)
        }
    }

    pub fn jump_to_marker_index(&self, zero_based: usize) -> Option<Ticks> {
        let mut ms: Vec<Ticks> = self.project.markers.iter().map(|m| m.t).collect();
        ms.sort();
        ms.get(zero_based).copied()
    }

    /// Remove mídias do projeto que não são usadas por nenhum evento.
    /// Retorna os IDs removidos para o chamador limpar caches visuais.
    pub fn remove_unused_media(&mut self, media: Option<&MediaCache>) -> Vec<Id> {
        let used: HashSet<Id> = self
            .project
            .tracks
            .iter()
            .flat_map(|t| t.events.iter().flat_map(crate::takes::media_ids))
            .collect();
        let n0 = self.project.media.len();
        self.checkpoint();
        let removed: Vec<Id> = self
            .project
            .media
            .iter()
            .filter(|m| !used.contains(&m.id))
            .map(|m| m.id)
            .collect();
        self.project.media.retain(|m| used.contains(&m.id));
        if let Some(cache) = media {
            for &id in &removed {
                cache.remove(id);
            }
        }
        self.removed_media_ids.extend_from_slice(&removed);
        self.set_status(format!(
            "{} mídia(s) removida(s)",
            n0 - self.project.media.len()
        ));
        self.changed();
        removed
    }

    /// Configura manualmente as propriedades do projeto, impedindo que a
    /// primeira mídia importada sobrescreva o formato escolhido (§3.1).
    pub fn configure_project_settings(&mut self, mut settings: ProjectSettings) {
        self.checkpoint();
        settings.width = (settings.width / 2).max(8) * 2;
        settings.height = (settings.height / 2).max(8) * 2;
        settings.manually_configured = true;
        self.project.settings = settings;
        self.project.sanitize();
        self.changed();
    }

    /// Registra uma mídia importada aplicando a invariante §3.1:
    /// a primeira mídia importada só define o formato do projeto se o usuário
    /// ainda não tiver configurado as propriedades manualmente.
    pub fn register_imported_media(
        &mut self,
        item: MediaItem,
        entry_sample_rate: Option<u32>,
    ) -> Id {
        let id = item.id;
        let is_first = self.project.media.is_empty();
        if is_first && !self.project.settings.manually_configured {
            if item.has_video && item.width >= 16 && item.height >= 16 {
                self.project.settings.width = (item.width / 2).max(8) * 2;
                self.project.settings.height = (item.height / 2).max(8) * 2;
                if let Some(fps) = item.fps {
                    if (1.0..=240.0).contains(&fps.as_f64()) {
                        self.project.settings.fps = fps;
                        self.project.settings.drop_frame = fps.is_ntsc_drop_eligible();
                    }
                }
            }
            if let Some(sr) = entry_sample_rate {
                if (8000..=192_000).contains(&sr) {
                    self.project.settings.sample_rate = sr;
                }
            }
            self.project.sanitize();
        }
        self.project.media.push(item);
        id
    }

    /// Aplica atomicamente um plano de movimentação ou cópia de eventos (§3.1):
    /// - Em movimentações (`copy == false`), solicitações com IDs duplicados são
    ///   rejeitadas atomicamente antes de mutar a timeline, alocar novos IDs ou
    ///   tocar o histórico (`undo`/`redo`).
    /// - Em cópias (`copy == true`), cada entrada recebe um novo `Id` após a
    ///   validação completa e mantém o destino exato solicitado.
    /// - Criação de novas faixas de destino e edição são confirmadas juntas.
    pub fn apply_move_plan(&mut self, plan: &[MoveEntry], copy: bool) -> Result<Vec<Id>> {
        if plan.is_empty() {
            return Ok(Vec::new());
        }

        // Fase 1: validação integral e pura (nenhuma mutação, novo ID ou undo).
        let mut seen_ids = HashSet::new();
        let mut planned_tracks: HashMap<u64, TrackKind> = HashMap::new();
        let mut resolved_origins = Vec::with_capacity(plan.len());

        for entry in plan {
            if !copy && !seen_ids.insert(entry.id) {
                return Err(NeovegError::InvalidInput(format!(
                    "plano de movimentação inválido: identificador {} duplicado",
                    entry.id
                )));
            }
            if entry.new_start < Ticks::ZERO {
                return Err(NeovegError::InvalidInput(format!(
                    "posição inicial negativa para evento {}",
                    entry.id
                )));
            }
            let Some((ti, ei)) = self.project.find_event(entry.id) else {
                return Err(NeovegError::InvalidInput(format!(
                    "evento {} não encontrado na timeline",
                    entry.id
                )));
            };
            let orig_track = &self.project.tracks[ti];
            let orig_ev = &orig_track.events[ei];
            if !copy && orig_ev.locked {
                return Err(NeovegError::InvalidInput(format!(
                    "evento {} está travado para movimentação",
                    entry.id
                )));
            }
            match &entry.target_track {
                MoveTrackTarget::Existing(dst) => {
                    let Some(dst_track) = self.project.tracks.get(*dst) else {
                        return Err(NeovegError::InvalidInput(format!(
                            "faixa de destino {dst} inexistente"
                        )));
                    };
                    if dst_track.kind != orig_track.kind {
                        return Err(NeovegError::InvalidInput(format!(
                            "tipo incompatível ao colocar evento {} na faixa {dst}",
                            entry.id
                        )));
                    }
                }
                MoveTrackTarget::CreateNew { key, kind, name } => {
                    if *kind != orig_track.kind {
                        return Err(NeovegError::InvalidInput(format!(
                            "tipo incompatível para nova faixa '{name}' do evento {}",
                            entry.id
                        )));
                    }
                    if name.trim().is_empty() {
                        return Err(NeovegError::InvalidInput(
                            "nome da nova faixa de destino vazio".into(),
                        ));
                    }
                    if let Some(prev_kind) = planned_tracks.insert(*key, *kind) {
                        if prev_kind != *kind {
                            return Err(NeovegError::InvalidInput(format!(
                                "chave de criação de faixa {key} usada com tipos conflitantes"
                            )));
                        }
                    }
                }
            }
            resolved_origins.push((ti, ei));
        }

        // Fase 2: snapshot dos eventos originais antes de mutar ou alocar IDs.
        let source_clones: Vec<Event> = resolved_origins
            .iter()
            .map(|&(ti, ei)| self.project.tracks[ti].events[ei].clone())
            .collect();

        // Fase 3: efetivação atômica em 1 passo de histórico.
        self.checkpoint();

        let mut created_map: HashMap<u64, usize> = HashMap::new();
        let mut target_indices = Vec::with_capacity(plan.len());
        for entry in plan {
            let dst = match &entry.target_track {
                MoveTrackTarget::Existing(ti) => *ti,
                MoveTrackTarget::CreateNew { key, kind, name } => {
                    if let Some(&idx) = created_map.get(key) {
                        idx
                    } else {
                        let idx = self.project.tracks.len();
                        self.project.tracks.push(Track::new(*kind, name));
                        created_map.insert(*key, idx);
                        idx
                    }
                }
            };
            target_indices.push(dst);
        }

        let mut result_ids = Vec::with_capacity(plan.len());
        if !copy {
            for (idx, entry) in plan.iter().enumerate() {
                let dst = target_indices[idx];
                if let Some((cur_t, cur_e)) = self.project.find_event(entry.id) {
                    if cur_t != dst {
                        let mut ev = self.project.tracks[cur_t].events.remove(cur_e);
                        ev.start = entry.new_start;
                        self.project.tracks[dst].events.push(ev);
                    } else {
                        self.project.tracks[cur_t].events[cur_e].start = entry.new_start;
                    }
                    result_ids.push(entry.id);
                }
            }
            self.sel_events = result_ids.iter().copied().collect();
        } else {
            let mut occurrence_count: HashMap<Id, usize> = HashMap::new();
            let mut group_remap: HashMap<(Id, usize), Id> = HashMap::new();
            for (idx, entry) in plan.iter().enumerate() {
                let dst = target_indices[idx];
                let occ = occurrence_count.entry(entry.id).or_insert(0);
                let occ_idx = *occ;
                *occ += 1;

                let mut ev = source_clones[idx].clone();
                ev.id = new_id();
                ev.start = entry.new_start;
                ev.group = ev
                    .group
                    .map(|g| *group_remap.entry((g, occ_idx)).or_insert_with(new_id));
                ev.fx = ev.fx.iter().map(|f| f.duplicate()).collect();
                result_ids.push(ev.id);
                self.project.tracks[dst].events.push(ev);
            }
            self.sel_events = result_ids.iter().copied().collect();
        }

        self.project.sort_events();
        self.changed();
        Ok(result_ids)
    }
}

/// Destino de faixa para cada item em um plano de movimentação/cópia (§3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MoveTrackTarget {
    Existing(usize),
    CreateNew {
        key: u64,
        kind: TrackKind,
        name: String,
    },
}

/// Entrada individual de um plano de movimentação/cópia (§3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveEntry {
    pub id: Id,
    pub target_track: MoveTrackTarget,
    pub new_start: Ticks,
}

/// Constrói um plano de movimentação a partir de uma lista ordenada de seleção
/// (preservando a ordem da primeira ocorrência e mantendo cada identidade uma
/// única vez antes de expandir eventos agrupados — §3.1).
pub fn build_move_plan_from_selection(
    project: &Project,
    seed_ids: &[Id],
    expand_groups: bool,
    dt: Ticks,
    dtrack: isize,
) -> Vec<MoveEntry> {
    let mut ordered_ids: Vec<Id> = Vec::new();
    let mut seen: HashSet<Id> = HashSet::new();

    for &id in seed_ids {
        if project.find_event(id).is_some() && seen.insert(id) {
            ordered_ids.push(id);
        }
    }

    if expand_groups {
        let mut groups: Vec<Id> = Vec::new();
        let mut seen_groups: HashSet<Id> = HashSet::new();
        for &id in &ordered_ids {
            if let Some(g) = project.event(id).and_then(|e| e.group) {
                if seen_groups.insert(g) {
                    groups.push(g);
                }
            }
        }
        if !seen_groups.is_empty() {
            for t in &project.tracks {
                for e in &t.events {
                    if let Some(g) = e.group {
                        if seen_groups.contains(&g) && seen.insert(e.id) {
                            ordered_ids.push(e.id);
                        }
                    }
                }
            }
        }
    }

    let n = project.tracks.len() as isize;
    let valid_track_shift = dtrack != 0
        && ordered_ids.iter().all(|&id| {
            let Some((ti, _)) = project.find_event(id) else {
                return false;
            };
            let nt = ti as isize + dtrack;
            nt >= 0 && nt < n && project.tracks[nt as usize].kind == project.tracks[ti].kind
        });
    let effective_dtrack = if valid_track_shift { dtrack } else { 0 };

    ordered_ids
        .into_iter()
        .filter_map(|id| {
            let (ti, ei) = project.find_event(id)?;
            let ev = &project.tracks[ti].events[ei];
            let target_ti = (ti as isize + effective_dtrack) as usize;
            let new_start = (ev.start + dt).max(Ticks::ZERO);
            Some(MoveEntry {
                id,
                target_track: MoveTrackTarget::Existing(target_ti),
                new_start,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::peek_next_id;

    fn sample_media(id: Id, w: u32, h: u32, fps: Option<FrameRate>) -> MediaItem {
        MediaItem {
            id,
            name: format!("m_{id}"),
            source: MediaSource::Generated(GenSpec::SolidColor {
                color: [1.0, 0.0, 0.0, 1.0],
            }),
            duration: Some(Ticks::from_secs_i64(5)),
            has_video: true,
            has_audio: true,
            width: w,
            height: h,
            fps,
            info: String::new(),
            proxy_path: None,
        }
    }

    #[test]
    fn colar_pula_trilha_de_ajuste() {
        let mut p = Project::default();
        let mut adj = Track::new(TrackKind::Video, "Ajuste 1");
        adj.adjustment = true;
        p.tracks.push(adj);
        p.tracks.push(Track::new(TrackKind::Video, "Vídeo 1"));
        let m = sample_media(new_id(), 1920, 1080, Some(FrameRate::FPS_30));
        let ev = Event::new(&m, Ticks::ZERO, Ticks::from_secs_i64(1));
        let ev_id = ev.id;
        p.media.push(m);
        p.tracks[1].events.push(ev);
        let mut s = EditorSession::new(p);
        s.sel_events.insert(ev_id);
        s.copy_selection();
        // colar com a trilha de ajuste selecionada cai na próxima trilha de vídeo
        s.sel_track = Some(0);
        s.paste(false);
        assert!(s.project.tracks[0].events.is_empty());
        assert_eq!(s.project.tracks[1].events.len(), 2);
    }

    #[test]
    fn invariante_movimentacao_rejeita_ids_duplicados_atomicamente() {
        let mut p = Project::new_default();
        p.tracks.push(Track::new(TrackKind::Video, "Vídeo 2"));
        let m = sample_media(new_id(), 1920, 1080, Some(FrameRate::FPS_30));
        let ev1 = Event::new(&m, Ticks::from_secs_i64(1), Ticks::from_secs_i64(2));
        let ev2 = Event::new(&m, Ticks::from_secs_i64(4), Ticks::from_secs_i64(2));
        let id1 = ev1.id;
        let id2 = ev2.id;
        p.media.push(m);
        p.tracks[0].events.push(ev1);
        p.tracks[0].events.push(ev2);

        let mut sess = EditorSession::new(p);
        let gen_before = sess.generation;
        let undo_before = sess.undo.len();
        let redo_before = sess.redo.len();
        let next_id_before = peek_next_id();

        let bad_plan = vec![
            MoveEntry {
                id: id1,
                target_track: MoveTrackTarget::Existing(0),
                new_start: Ticks::from_secs_i64(3),
            },
            MoveEntry {
                id: id2,
                target_track: MoveTrackTarget::CreateNew {
                    key: 1,
                    kind: TrackKind::Video,
                    name: "Nova Vídeo".into(),
                },
                new_start: Ticks::from_secs_i64(5),
            },
            MoveEntry {
                id: id1,
                target_track: MoveTrackTarget::Existing(2),
                new_start: Ticks::from_secs_i64(7),
            },
        ];

        let err = sess.apply_move_plan(&bad_plan, false);
        assert!(err.is_err());
        // Nenhuma faixa criada, nenhum evento movido, nenhum novo ID nem histórico tocado.
        assert_eq!(sess.project.tracks.len(), 3);
        assert_eq!(sess.project.tracks[0].events.len(), 2);
        assert_eq!(
            sess.project.tracks[0].events[0].start,
            Ticks::from_secs_i64(1)
        );
        assert_eq!(sess.project.tracks[2].events.len(), 0);
        assert_eq!(sess.generation, gen_before);
        assert_eq!(sess.undo.len(), undo_before);
        assert_eq!(sess.redo.len(), redo_before);
        assert_eq!(peek_next_id(), next_id_before);
    }

    #[test]
    fn invariante_copia_recebe_novos_ids_e_cria_faixa_destino_junto() {
        let mut p = Project::new_default();
        let m = sample_media(new_id(), 1920, 1080, Some(FrameRate::FPS_30));
        let ev = Event::new(&m, Ticks::from_secs_i64(1), Ticks::from_secs_i64(2));
        let src_id = ev.id;
        p.media.push(m);
        p.tracks[0].events.push(ev);

        let mut sess = EditorSession::new(p);
        let plan = vec![
            MoveEntry {
                id: src_id,
                target_track: MoveTrackTarget::Existing(0),
                new_start: Ticks::from_secs_i64(6),
            },
            MoveEntry {
                id: src_id,
                target_track: MoveTrackTarget::CreateNew {
                    key: 99,
                    kind: TrackKind::Video,
                    name: "Vídeo Cópia".into(),
                },
                new_start: Ticks::from_secs_i64(8),
            },
        ];
        let copies = sess.apply_move_plan(&plan, true).unwrap();
        assert_eq!(copies.len(), 2);
        assert_ne!(copies[0], src_id);
        assert_ne!(copies[1], src_id);
        assert_ne!(copies[0], copies[1]);
        assert_eq!(sess.project.tracks.len(), 3);
        assert_eq!(sess.project.tracks[0].events.len(), 2);
        assert_eq!(sess.project.tracks[2].events.len(), 1);
        assert_eq!(sess.project.tracks[2].events[0].id, copies[1]);
        assert_eq!(
            sess.project.tracks[2].events[0].start,
            Ticks::from_secs_i64(8)
        );
        // Desfazer desfaz faixa criada + ambas as cópias atomicamente.
        assert!(sess.undo());
        assert_eq!(sess.project.tracks.len(), 2);
        assert_eq!(sess.project.tracks[0].events.len(), 1);
    }

    #[test]
    fn invariante_plano_selecao_deduplica_antes_de_expandir_grupos() {
        let mut p = Project::new_default();
        let m = sample_media(new_id(), 1280, 720, Some(FrameRate::FPS_24));
        let gid = new_id();
        let mut v_ev = Event::new(&m, Ticks::from_secs_i64(2), Ticks::from_secs_i64(3));
        v_ev.group = Some(gid);
        let mut a_ev = Event::new(&m, Ticks::from_secs_i64(2), Ticks::from_secs_i64(3));
        a_ev.group = Some(gid);
        let standalone = Event::new(&m, Ticks::from_secs_i64(7), Ticks::from_secs_i64(1));
        let (vid, aid, sid) = (v_ev.id, a_ev.id, standalone.id);
        p.media.push(m);
        p.tracks[0].events.push(v_ev);
        p.tracks[0].events.push(standalone);
        p.tracks[1].events.push(a_ev);

        // Seleção contém IDs repetidos antes de expandir o grupo.
        let plan = build_move_plan_from_selection(
            &p,
            &[sid, vid, sid, vid],
            true,
            Ticks::from_secs_i64(1),
            0,
        );
        let plan_ids: Vec<Id> = plan.iter().map(|e| e.id).collect();
        // Ordem da primeira ocorrência das sementes deduplicadas, seguida da expansão de grupo sem duplicar.
        assert_eq!(plan_ids, vec![sid, vid, aid]);
    }

    #[test]
    fn invariante_primeiro_arquivo_define_formato_apenas_se_nao_configurado() {
        // Caso 1: projeto padrão não configurado manualmente adota a 1ª mídia e ignora a 2ª.
        let mut sess = EditorSession::new(Project::new_default());
        assert!(!sess.project.settings.manually_configured);
        let m1 = sample_media(new_id(), 1280, 720, Some(FrameRate::FPS_24));
        sess.register_imported_media(m1, Some(44100));
        assert_eq!(sess.project.settings.width, 1280);
        assert_eq!(sess.project.settings.height, 720);
        assert_eq!(sess.project.settings.fps, FrameRate::FPS_24);
        assert_eq!(sess.project.settings.sample_rate, 44100);

        let m2 = sample_media(new_id(), 3840, 2160, Some(FrameRate::FPS_60));
        sess.register_imported_media(m2, Some(96000));
        assert_eq!(sess.project.settings.width, 1280);
        assert_eq!(sess.project.settings.fps, FrameRate::FPS_24);

        // Caso 2: projeto configurado manualmente nunca é sobrescrito nem pela 1ª mídia.
        let mut sess_manual = EditorSession::new(Project::new_default());
        let custom = ProjectSettings {
            width: 1920,
            height: 1080,
            fps: FrameRate::FPS_60,
            ..Default::default()
        };
        sess_manual.configure_project_settings(custom);
        assert!(sess_manual.project.settings.manually_configured);
        let m_first = sample_media(new_id(), 640, 360, Some(FrameRate::FPS_25));
        sess_manual.register_imported_media(m_first, Some(44100));
        assert_eq!(sess_manual.project.settings.width, 1920);
        assert_eq!(sess_manual.project.settings.height, 1080);
        assert_eq!(sess_manual.project.settings.fps, FrameRate::FPS_60);
        assert_eq!(sess_manual.project.settings.sample_rate, 48000);
    }

    #[test]
    fn trechos_prerenderizados_mesclam_e_invalidam_ao_editar() {
        let mut sess = EditorSession::new(Project::new_default());
        sess.record_prerendered_range(Ticks::from_secs_i64(5), Ticks::from_secs_i64(3));
        assert!(sess.prerendered_ranges.is_empty());

        sess.record_prerendered_range(Ticks::from_secs_i64(2), Ticks::from_secs_i64(6));
        sess.record_prerendered_range(Ticks::from_secs_i64(5), Ticks::from_secs_i64(9));
        sess.record_prerendered_range(Ticks::from_secs_i64(12), Ticks::from_secs_i64(14));
        assert_eq!(
            sess.prerendered_ranges,
            vec![
                (Ticks::from_secs_i64(2), Ticks::from_secs_i64(9)),
                (Ticks::from_secs_i64(12), Ticks::from_secs_i64(14)),
            ]
        );
        sess.changed();
        assert!(sess.prerendered_ranges.is_empty());
    }
}
