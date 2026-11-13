//! Estado editável de uma sessão de edição da timeline: projeto atual,
//! histórico de Desfazer/Refazer, área de transferência, seleção, cursor,
//! opções e regras de edição puras sem dependência de UI (AGENTS.md §3.2).

use crate::media::MediaCache;
use crate::model::*;
use crate::prefs::PreviewQuality;
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
    pub time_format: TimeFormat,
    pub preview_quality: PreviewQuality,
    pub safe_areas: bool,
    pub loop_playback: bool,
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
            time_format: TimeFormat::SmpteDrop,
            preview_quality: PreviewQuality::Preview,
            safe_areas: false,
            loop_playback: false,
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
        }
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
                .map(|t| t.kind == kind)
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
                right.offset += right.rate.scale_ticks(left_len);
            }
            if e.reverse {
                e.offset += e.rate.scale_ticks(right.length);
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
                        e.offset += e.rate.scale_ticks(d);
                    }
                    e.start = a;
                    e.length -= d;
                }
                if e.end() > b {
                    let d = e.end() - b;
                    if e.reverse {
                        e.offset += e.rate.scale_ticks(d);
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
                        e.offset += e.rate.scale_ticks(d);
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
                        e.offset += e.rate.scale_ticks(d);
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
                    right.offset += right.rate.scale_ticks(cut);
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
                    e.offset += e.rate.scale_ticks(d);
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
                    right.offset += right.rate.scale_ticks(cut);
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
                    right.offset += right.rate.scale_ticks(left);
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
            .flat_map(|t| t.events.iter().map(|e| e.media_id))
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
}
