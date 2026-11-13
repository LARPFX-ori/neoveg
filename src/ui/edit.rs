//! Operações de edição da timeline (dividir, recortar, colar, ripple,
//! agrupamento, marcadores, regiões, envelopes...).

use super::{App, ClipItem};
use crate::model::*;
use std::collections::{HashMap, HashSet};

impl App {
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

    /// Cola no cursor, na trilha selecionada. `insert` = desloca eventos
    /// posteriores (paste insert).
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
                for e in &mut self.project.tracks[ti].events {
                    if e.start >= t0 {
                        e.start += span;
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
            self.project.tracks[ti].events.push(e);
        }
        self.sel_events = new_sel;
        self.cursor = t0 + span;
        self.changed();
    }

    fn target_track(&self, idx: usize, kind: TrackKind) -> Option<usize> {
        (idx..self.project.tracks.len()).find(|&i| self.project.tracks[i].kind == kind)
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
                // processa do último para o primeiro
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
    /// estiver selecionado).
    pub fn split_at_cursor(&mut self) {
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
            return;
        }
        self.checkpoint();
        let mut group_map: HashMap<Id, Id> = HashMap::new();
        let mut new_sel = HashSet::new();
        for id in targets {
            let Some((ti, ei)) = self.project.find_event(id) else {
                continue;
            };
            let e = &mut self.project.tracks[ti].events[ei];
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
            // pan/crop: desloca keyframes para o tempo local do novo evento
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
            self.project.tracks[ti].events.push(right);
        }
        self.sel_events = new_sel;
        self.changed();
    }

    pub fn trim_to_selection(&mut self) {
        let Some((a, b)) = self.time_sel else { return };
        if self.sel_events.is_empty() {
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
                    // divide: parte esquerda + parte direita deslocada
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
            self.project.tracks[ti].events.push(e);
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
        self.checkpoint();
        let t = &mut self.project.tracks[ti];
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
        let Some(track) = self.project.tracks.get_mut(ti) else {
            return;
        };
        let kind = if track.kind == TrackKind::Audio {
            audio_kind
        } else {
            video_kind
        };
        if let Some(e) = track.envelopes.iter_mut().find(|e| e.kind == kind) {
            e.visible = !e.visible;
        } else {
            self.checkpoint();
            let track = &mut self.project.tracks[ti];
            track.envelopes.push(Envelope::new(kind));
            self.opts.show_envelopes = true;
        }
        self.changed();
    }

    pub fn remove_envelope(&mut self, kind: EnvKind) {
        let Some(ti) = self.sel_track else { return };
        self.checkpoint();
        let track = &mut self.project.tracks[ti];
        let k = match (track.kind, kind) {
            (TrackKind::Video, EnvKind::Volume) => EnvKind::Opacity,
            (TrackKind::Video, EnvKind::Pan) => EnvKind::FadeToColor,
            _ => kind,
        };
        track.envelopes.retain(|e| e.kind != k);
        self.changed();
    }

    pub fn toggle_reverse(&mut self) {
        self.checkpoint();
        for id in self.sel_events.clone() {
            if let Some(e) = self.project.event_mut(id) {
                e.reverse = !e.reverse;
            }
        }
        self.changed();
    }

    /// Congela o quadro sob o cursor: divide e cria um evento de imagem
    /// parada (velocidade 0) de 2 segundos.
    pub fn freeze_frame(&mut self) {
        let Some(id) = self.sel_events.iter().next().cloned() else {
            return;
        };
        let Some((ti, ei)) = self.project.find_event(id) else {
            return;
        };
        if self.project.tracks[ti].kind != TrackKind::Video {
            return;
        }
        let c = self.cursor;
        let e = self.project.tracks[ti].events[ei].clone();
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
        // empurra o restante da trilha
        for ev in &mut self.project.tracks[ti].events {
            if ev.start >= c {
                ev.start += two_s;
            } else if ev.end() > c && ev.id == e.id {
                // divide o evento original no cursor
            }
        }
        let original = &mut self.project.tracks[ti].events[ei];
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
            self.project.tracks[ti].events.push(right);
        }
        self.project.tracks[ti].events.push(fr);
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
        self.checkpoint();
        let mut t = self.project.tracks[ti].clone();
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

    /// Remove mídias do projeto que não são usadas por nenhum evento.
    pub fn remove_unused_media(&mut self) {
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
        for id in removed {
            self.media.remove(id);
            self.thumbs.remove(&id);
        }
        self.set_status(format!(
            "{} mídia(s) removida(s)",
            n0 - self.project.media.len()
        ));
        self.changed();
    }
}
