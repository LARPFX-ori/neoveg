//! Regras e matemática de crossfades (sobreposição de eventos na mesma trilha)
//! com semântica do VEGAS Pro, tempo exato em ticks e política zero pânico (§0 / §2.2).
//!
//! Este módulo é a fonte única de verdade compartilhada entre:
//! - composição de vídeo (`engine/video.rs`);
//! - mixagem de áudio (`engine/audio.rs`);
//! - operações de edição (`session.rs` e `commands.rs`);
//! - visualização e interação na linha do tempo (`neoveg_ui/src/timeline.rs`).

use crate::model::{Curve, Event, Fade, Id, Project, Track, TrackKind};
use crate::time::Ticks;
use neoveg_common::CutToOverlapAlign;
use std::collections::{HashMap, HashSet};

/// As 5 curvas de fade disponíveis na matriz 5×5 de *Fade Type* do VEGAS Pro
/// (exclui `Curve::Hold`, que é só para envelopes e keyframes).
pub const CROSSFADE_CURVES: [Curve; 5] = [
    Curve::Fast,
    Curve::Linear,
    Curve::Smooth,
    Curve::Sharp,
    Curve::Slow,
];

/// Região de sobreposição/crossfade entre dois eventos na mesma trilha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossfadeRegion {
    pub left_idx: usize,
    pub right_idx: usize,
    pub left_id: Id,
    pub right_id: Id,
    pub start: Ticks,
    pub end: Ticks,
    pub out_curve: Curve,
    pub in_curve: Curve,
    /// `true` quando o crossfade está ativo; `false` quando a sobreposição foi
    /// criada com *Crossfades automáticos* desligado ou desativada manualmente.
    pub active: bool,
}

impl CrossfadeRegion {
    pub fn len(self) -> Ticks {
        (self.end - self.start).max(Ticks::ZERO)
    }

    pub fn is_empty(self) -> bool {
        self.end <= self.start
    }
}

/// Curva padrão de crossfade conforme o tipo de trilha (VEGAS Pro):
/// - Áudio: `Curve::Fast` (potência constante / *Fast Log*, sem queda de -3 dB no meio);
/// - Vídeo: `Curve::Linear` (dissolução linear clássica).
pub fn default_crossfade_curve(kind: TrackKind) -> Curve {
    match kind {
        TrackKind::Audio => Curve::Fast,
        TrackKind::Video => Curve::Linear,
    }
}

/// Garante que uma curva de crossfade seja uma das 5 curvas suportadas (nunca `Hold`).
pub fn sanitize_crossfade_curve(curve: Curve, kind: TrackKind) -> Curve {
    if curve == Curve::Hold {
        default_crossfade_curve(kind)
    } else {
        curve
    }
}

/// Resolve a curva de saída efetiva de `left` em um crossfade na trilha `kind`.
pub fn resolve_out_curve(kind: TrackKind, left: &Event) -> Curve {
    if let Some(c) = left.crossfade_out_curve {
        return sanitize_crossfade_curve(c, kind);
    }
    if left.fade_out.curve != Curve::Linear && left.fade_out.curve != Curve::Hold {
        return left.fade_out.curve;
    }
    default_crossfade_curve(kind)
}

/// Resolve a curva de entrada efetiva de `right` em um crossfade na trilha `kind`.
pub fn resolve_in_curve(kind: TrackKind, right: &Event) -> Curve {
    if let Some(c) = right.crossfade_in_curve {
        return sanitize_crossfade_curve(c, kind);
    }
    if right.fade_in.curve != Curve::Linear && right.fade_in.curve != Curve::Hold {
        return right.fade_in.curve;
    }
    default_crossfade_curve(kind)
}

/// Lista todas as regiões de sobreposição/crossfade de uma trilha, ordenadas no tempo.
///
/// Um par `(left, right)` forma sobreposição elegível a crossfade quando:
/// `left.start < right.start && right.start < left.end() && left.end() <= right.end()`.
/// Eventos estritamente contidos dentro de um evento mais longo (cutaway/sobreposição)
/// não são classificados como crossfade.
pub fn track_crossfades(track: &Track, include_muted: bool) -> Vec<CrossfadeRegion> {
    let mut order: Vec<usize> = track
        .events
        .iter()
        .enumerate()
        .filter(|(_, e)| include_muted || !e.mute)
        .map(|(i, _)| i)
        .collect();
    order.sort_by_key(|&i| {
        let e = &track.events[i];
        (e.start, e.end(), e.id)
    });

    let mut out = Vec::new();
    for pos in 1..order.len() {
        let ri = order[pos];
        let right = &track.events[ri];
        let mut best_left: Option<usize> = None;
        for &li in order[..pos].iter().rev() {
            let left = &track.events[li];
            if left.start < right.start && left.end() > right.start && left.end() <= right.end() {
                best_left = Some(li);
                break;
            }
        }
        if let Some(li) = best_left {
            let left = &track.events[li];
            let start = right.start;
            let end = left.end();
            if end > start {
                out.push(CrossfadeRegion {
                    left_idx: li,
                    right_idx: ri,
                    left_id: left.id,
                    right_id: right.id,
                    start,
                    end,
                    out_curve: resolve_out_curve(track.kind, left),
                    in_curve: resolve_in_curve(track.kind, right),
                    active: !right.no_crossfade_in,
                });
            }
        }
    }
    out
}

/// Procura o crossfade ativo entre `left_id` e `right_id` na trilha.
pub fn find_active_crossfade_pair(
    track: &Track,
    left_id: Id,
    right_id: Id,
) -> Option<CrossfadeRegion> {
    track_crossfades(track, false)
        .into_iter()
        .find(|xf| xf.active && xf.left_id == left_id && xf.right_id == right_id)
}

/// Calcula os fades efetivos de um evento a partir da lista já computada de crossfades
/// (usado tanto pelo mixer de áudio quanto pelo desenho da forma de onda na timeline).
pub fn effective_event_fades_from_regions(ev: &Event, regions: &[CrossfadeRegion]) -> (Fade, Fade) {
    let mut fi = ev.fade_in.clone();
    let mut fo = ev.fade_out.clone();
    for xf in regions {
        if !xf.active {
            continue;
        }
        let ov = xf.len();
        if ov <= Ticks::ZERO {
            continue;
        }
        if xf.right_id == ev.id && ov >= fi.len {
            fi.len = ov;
            fi.curve = xf.in_curve;
        }
        if xf.left_id == ev.id && ov >= fo.len {
            fo.len = ov;
            fo.curve = xf.out_curve;
        }
    }
    (fi, fo)
}

/// Calcula os fades efetivos de um evento de áudio em `track[idx]`.
pub fn effective_audio_fades(track: &Track, idx: usize) -> (Fade, Fade) {
    let Some(ev) = track.events.get(idx) else {
        return (Fade::default(), Fade::default());
    };
    let regions = track_crossfades(track, false);
    effective_event_fades_from_regions(ev, &regions)
}

/// Captura o conjunto de pares `(left_id, right_id)` atualmente sobrepostos nas trilhas.
pub fn overlap_pairs(tracks: &[Track]) -> HashSet<(Id, Id)> {
    let mut set = HashSet::new();
    for tr in tracks {
        for xf in track_crossfades(tr, true) {
            set.insert((xf.left_id, xf.right_id));
        }
    }
    set
}

/// Atualiza estado de crossfades após mover, aparar, colar ou soltar eventos.
///
/// - Sobreposições **novas** (ausentes de `prev_overlaps`) seguem `auto_crossfades`:
///   quando ligado, habilitam o crossfade e aplicam a curva padrão das Preferências se
///   o evento não tiver curva personalizada; quando desligado, marcam `no_crossfade_in = true`
///   (corte seco, sem alterar crossfades antigos já existentes).
/// - Eventos que deixaram de se sobrepor ao anterior voltam a ter `no_crossfade_in = false`,
///   permitindo que futuras sobreposições com *Crossfades automáticos* ligado funcionem.
pub fn sync_new_overlaps(
    tracks: &mut [Track],
    track_indices: &[usize],
    prev_overlaps: &HashSet<(Id, Id)>,
    auto_crossfades: bool,
    default_audio_curve: Curve,
    default_video_curve: Curve,
) {
    let mut visited = HashSet::new();
    for &ti in track_indices {
        if !visited.insert(ti) {
            continue;
        }
        let Some(tr) = tracks.get_mut(ti) else {
            continue;
        };
        let kind = tr.kind;
        let pref_curve = sanitize_crossfade_curve(
            match kind {
                TrackKind::Audio => default_audio_curve,
                TrackKind::Video => default_video_curve,
            },
            kind,
        );
        let built_in_default = default_crossfade_curve(kind);
        let regions = track_crossfades(tr, true);
        let mut right_with_overlap = HashSet::new();
        for xf in &regions {
            right_with_overlap.insert(xf.right_id);
            let is_new = !prev_overlaps.contains(&(xf.left_id, xf.right_id));
            if !is_new {
                continue;
            }
            if auto_crossfades {
                if let Some(right) = tr.events.get_mut(xf.right_idx) {
                    right.no_crossfade_in = false;
                    if right.crossfade_in_curve.is_none()
                        && right.fade_in.curve == Curve::Linear
                        && pref_curve != built_in_default
                    {
                        right.crossfade_in_curve = Some(pref_curve);
                    }
                }
                if let Some(left) = tr.events.get_mut(xf.left_idx) {
                    if left.crossfade_out_curve.is_none()
                        && left.fade_out.curve == Curve::Linear
                        && pref_curve != built_in_default
                    {
                        left.crossfade_out_curve = Some(pref_curve);
                    }
                }
            } else if let Some(right) = tr.events.get_mut(xf.right_idx) {
                right.no_crossfade_in = true;
            }
        }
        // Eventos que não estão mais sobrepostos à esquerda limpam `no_crossfade_in`.
        for ev in &mut tr.events {
            if !right_with_overlap.contains(&ev.id) && ev.no_crossfade_in {
                ev.no_crossfade_in = false;
            }
        }
    }
}

/// Snapshot de um par de eventos em crossfade para deslizamento atômico (*Slide Crossfade*).
#[derive(Clone, Debug)]
pub struct SlideCrossfadePair {
    pub track_idx: usize,
    pub left_id: Id,
    pub left_start: Ticks,
    pub left_length: Ticks,
    pub left_offset: Ticks,
    pub left_rate: crate::time::Rate,
    pub left_reverse: bool,
    pub left_looped: bool,
    pub left_media_dur: Option<Ticks>,
    pub right_id: Id,
    pub right_start: Ticks,
    pub right_length: Ticks,
    pub right_offset: Ticks,
    pub right_rate: crate::time::Rate,
    pub right_reverse: bool,
    pub right_looped: bool,
    pub right_media_dur: Option<Ticks>,
    pub overlap: Ticks,
}

/// Captura o par principal e quaisquer pares agrupados (ex.: áudio/vídeo vinculados)
/// que compartilham a mesma janela de crossfade para *Slide Crossfade* (`Ctrl+Alt+Arrastar`).
pub fn capture_slide_crossfade(
    project: &Project,
    track_idx: usize,
    left_id: Id,
    right_id: Id,
    ignore_grouping: bool,
) -> Vec<SlideCrossfadePair> {
    let Some(primary_tr) = project.tracks.get(track_idx) else {
        return Vec::new();
    };
    let Some(xf) = track_crossfades(primary_tr, true)
        .into_iter()
        .find(|x| x.left_id == left_id && x.right_id == right_id)
    else {
        return Vec::new();
    };
    let left_ev = &primary_tr.events[xf.left_idx];
    let right_ev = &primary_tr.events[xf.right_idx];
    if left_ev.locked || right_ev.locked || primary_tr.locked {
        return Vec::new();
    }

    let make_pair = |ti: usize, l: &Event, r: &Event, ov: Ticks| SlideCrossfadePair {
        track_idx: ti,
        left_id: l.id,
        left_start: l.start,
        left_length: l.length,
        left_offset: l.offset,
        left_rate: l.rate,
        left_reverse: l.reverse,
        left_looped: l.looped,
        left_media_dur: project.media(l.media_id).and_then(|m| m.duration),
        right_id: r.id,
        right_start: r.start,
        right_length: r.length,
        right_offset: r.offset,
        right_rate: r.rate,
        right_reverse: r.reverse,
        right_looped: r.looped,
        right_media_dur: project.media(r.media_id).and_then(|m| m.duration),
        overlap: ov,
    };

    let mut pairs = vec![make_pair(track_idx, left_ev, right_ev, xf.len())];
    if ignore_grouping {
        return pairs;
    }

    let lg = left_ev.group;
    let rg = right_ev.group;
    let tol = Ticks::from_millis_i64(2);
    for (ti, tr) in project.tracks.iter().enumerate() {
        if ti == track_idx || tr.locked {
            continue;
        }
        for other_xf in track_crossfades(tr, true) {
            let l = &tr.events[other_xf.left_idx];
            let r = &tr.events[other_xf.right_idx];
            if l.locked || r.locked {
                continue;
            }
            let same_groups = (lg.is_some() && l.group == lg) || (rg.is_some() && r.group == rg);
            let same_window =
                other_xf.start.abs_diff(xf.start) <= tol && other_xf.end.abs_diff(xf.end) <= tol;
            if same_groups && same_window {
                pairs.push(make_pair(ti, l, r, other_xf.len()));
            }
        }
    }
    pairs
}

/// Desliza a janela de crossfade por `raw_dt` (em ticks exatos), preservando a duração
/// do crossfade, os limites externos `[left_start, right_end]` e a sincronia da mídia.
pub fn apply_slide_crossfade(
    project: &mut Project,
    pairs: &[SlideCrossfadePair],
    raw_dt: Ticks,
    fd: Ticks,
) -> bool {
    if pairs.is_empty() {
        return false;
    }
    let min_tail = fd.max(Ticks::from_millis_i64(1));
    let mut min_dt = Ticks::MIN_NEGATIVE;
    let mut max_dt = Ticks::MAX;

    for p in pairs {
        // Esquerda deve manter duração >= overlap + min_tail
        let max_left_shrink = (p.left_length - p.overlap - min_tail).max(Ticks::ZERO);
        min_dt = min_dt.max(-max_left_shrink);
        // Direita não pode cruzar t = 0
        min_dt = min_dt.max(-p.right_start);
        // Direita deve manter duração >= overlap + min_tail
        let max_right_shrink = (p.right_length - p.overlap - min_tail).max(Ticks::ZERO);
        max_dt = max_dt.min(max_right_shrink);

        // Limite de mídia disponível na cauda da esquerda (ao estender para direita, dt > 0)
        if let (Some(d), false, false) = (p.left_media_dur, p.left_looped, p.left_reverse) {
            if let Some(avail) = p
                .left_rate
                .unscale_ticks((d - p.left_offset).max(Ticks::ZERO))
            {
                max_dt = max_dt.min((avail - p.left_length).max(Ticks::ZERO));
            }
        }
        // Limite de mídia disponível no início da direita (ao recuar para esquerda, dt < 0)
        if !p.right_looped && !p.right_reverse {
            if let Some(max_back) = p.right_rate.unscale_ticks(p.right_offset) {
                min_dt = min_dt.max(-max_back);
            }
        }
    }

    if min_dt > max_dt {
        min_dt = Ticks::ZERO;
        max_dt = Ticks::ZERO;
    }
    let dt = raw_dt.clamp_safe(min_dt, max_dt);

    for p in pairs {
        let Some(track) = project.tracks.get_mut(p.track_idx) else {
            continue;
        };
        if let Some(le) = track.events.iter_mut().find(|e| e.id == p.left_id) {
            let orig_end = p.left_start + p.left_length;
            le.length = (p.left_length + dt).max(p.overlap + min_tail);
            if p.left_reverse {
                let d_end = (p.left_start + le.length) - orig_end;
                let off_delta = p.left_offset - p.left_rate.scale_ticks(d_end) - le.offset;
                le.shift_offsets(off_delta);
            }
            le.fade_in.len = le.fade_in.len.min(le.length);
            le.fade_out.len = le.fade_out.len.min(le.length - le.fade_in.len);
        }
        if let Some(re) = track.events.iter_mut().find(|e| e.id == p.right_id) {
            re.start = (p.right_start + dt).max(Ticks::ZERO);
            re.length = (p.right_length - dt).max(p.overlap + min_tail);
            if !p.right_reverse {
                let target_off = (p.right_offset + p.right_rate.scale_ticks(dt)).max(Ticks::ZERO);
                let delta = target_off - re.offset;
                re.shift_offsets(delta);
            }
            re.fade_in.len = re.fade_in.len.min(re.length);
            re.fade_out.len = re.fade_out.len.min(re.length - re.fade_in.len);
        }
    }
    true
}

/// Converte um corte seco entre dois eventos contíguos `(left, right)` na mesma trilha
/// em uma sobreposição com crossfade de duração até `amount`, conforme `align`.
///
/// Retorna `true` se uma sobreposição válida foi criada.
#[allow(clippy::too_many_arguments)]
pub fn convert_cut_pair_to_overlap(
    left: &mut Event,
    right: &mut Event,
    left_media_dur: Option<Ticks>,
    track_kind: TrackKind,
    amount: Ticks,
    align: CutToOverlapAlign,
    min_len: Ticks,
    default_curve: Curve,
) -> bool {
    if left.locked || right.locked || amount <= Ticks::ZERO {
        return false;
    }
    let min_keep = min_len.max(Ticks::from_millis_i64(1));
    // Limite máximo de sobreposição sem aniquilar nenhum evento.
    let max_ov = (left.length - min_keep)
        .min(right.length - min_keep)
        .max(Ticks::ZERO);
    let desired = amount.min(max_ov);
    if desired <= Ticks::ZERO {
        return false;
    }

    // Quanto `left` consegue estender para a direita sem ultrapassar a mídia.
    let left_tail_avail = if left.looped || left.reverse {
        desired
    } else if let Some(d) = left_media_dur {
        left.rate
            .unscale_ticks((d - left.offset).max(Ticks::ZERO))
            .map(|avail| (avail - left.length).max(Ticks::ZERO))
            .unwrap_or(Ticks::ZERO)
    } else {
        desired
    };

    // Quanto `right` consegue estender para a esquerda sem ficar negativo na timeline/mídia.
    let right_head_avail = if right.looped || right.reverse {
        right.start
    } else {
        right
            .rate
            .unscale_ticks(right.offset)
            .unwrap_or(Ticks::ZERO)
            .min(right.start)
    };

    let (want_l, want_r) = match align {
        CutToOverlapAlign::Center => {
            let r_half = desired.div_i64(2);
            (desired - r_half, r_half)
        }
        CutToOverlapAlign::BeforeCut => (Ticks::ZERO, desired),
        CutToOverlapAlign::AfterCut => (desired, Ticks::ZERO),
    };

    let mut ext_l = want_l.min(left_tail_avail);
    let mut ext_r = want_r.min(right_head_avail);

    // Se um lado não teve margem de mídia suficiente, tenta compensar no outro lado.
    let deficit = desired - (ext_l + ext_r);
    if deficit > Ticks::ZERO {
        let extra_l = (left_tail_avail - ext_l).max(Ticks::ZERO).min(deficit);
        ext_l += extra_l;
        let rem = deficit - extra_l;
        if rem > Ticks::ZERO {
            let extra_r = (right_head_avail - ext_r).max(Ticks::ZERO).min(rem);
            ext_r += extra_r;
        }
    }

    // Se nenhum dos dois lados possui margem de mídia (ex.: clipes não-loopados inteiros
    // a partir do início), desloca o evento direito para a esquerda para criar o crossfade.
    if ext_l + ext_r <= Ticks::ZERO {
        let shift = desired.min(right.start);
        if shift <= Ticks::ZERO {
            return false;
        }
        right.start -= shift;
    } else {
        if ext_l > Ticks::ZERO {
            left.length += ext_l;
            if left.reverse {
                let d_off = left.rate.scale_ticks(ext_l);
                let new_off = (left.offset - d_off).max(Ticks::ZERO);
                left.shift_offsets(new_off - left.offset);
            }
        }
        if ext_r > Ticks::ZERO {
            right.start -= ext_r;
            right.length += ext_r;
            if !right.reverse {
                let d_off = right.rate.scale_ticks(ext_r);
                let new_off = (right.offset - d_off).max(Ticks::ZERO);
                right.shift_offsets(new_off - right.offset);
            }
        }
    }

    right.no_crossfade_in = false;
    let pref = sanitize_crossfade_curve(default_curve, track_kind);
    let built_in = default_crossfade_curve(track_kind);
    if pref != built_in {
        if left.crossfade_out_curve.is_none() {
            left.crossfade_out_curve = Some(pref);
        }
        if right.crossfade_in_curve.is_none() {
            right.crossfade_in_curve = Some(pref);
        }
    }
    true
}

/// Converte uma sobreposição `(left, right)` de volta em um corte seco, aparando as
/// bordas sobrepostas no ponto escolhido (`Center`, `BeforeCut` = início ou `AfterCut` = fim).
pub fn convert_overlap_pair_to_cut(
    left: &mut Event,
    right: &mut Event,
    cut_align: CutToOverlapAlign,
    min_len: Ticks,
) -> bool {
    if left.locked || right.locked {
        return false;
    }
    if !(left.start < right.start && left.end() > right.start && left.end() <= right.end()) {
        return false;
    }
    let min_keep = min_len.max(Ticks::from_millis_i64(1));
    let ov_start = right.start;
    let ov_end = left.end();
    let cut_t = match cut_align {
        CutToOverlapAlign::Center => ov_start + (ov_end - ov_start).div_i64(2),
        CutToOverlapAlign::BeforeCut => ov_start,
        CutToOverlapAlign::AfterCut => ov_end,
    };
    let min_cut = left.start + min_keep;
    let max_cut = right.end() - min_keep;
    if min_cut > max_cut {
        return false;
    }
    let cut_t = cut_t.clamp_safe(min_cut, max_cut);

    let left_new_len = (cut_t - left.start).max(min_keep);
    let left_shrink = left.length - left_new_len;
    left.length = left_new_len;
    if left.reverse && left_shrink > Ticks::ZERO {
        left.shift_offsets(left.rate.scale_ticks(left_shrink));
    }
    left.fade_in.len = left.fade_in.len.min(left.length);
    left.fade_out.len = Ticks::ZERO;

    let right_delta = cut_t - right.start;
    right.start = cut_t;
    right.length = (right.length - right_delta).max(min_keep);
    if !right.reverse && right_delta > Ticks::ZERO {
        right.shift_offsets(right.rate.scale_ticks(right_delta));
    }
    right.fade_in.len = Ticks::ZERO;
    right.fade_out.len = right.fade_out.len.min(right.length);
    right.no_crossfade_in = false;
    right.transition = None;
    true
}

/// Mapa de metadados de duração de mídia (helper interno opcional).
pub fn media_durations_map(project: &Project) -> HashMap<Id, Option<Ticks>> {
    project.media.iter().map(|m| (m.id, m.duration)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MediaItem, MediaSource, Project};

    fn make_media(dur_secs: i64) -> MediaItem {
        MediaItem {
            id: crate::model::new_id(),
            name: "clip".into(),
            source: MediaSource::Generated(crate::model::GenSpec::SolidColor {
                color: [1.0, 0.0, 0.0, 1.0],
            }),
            duration: Some(Ticks::from_secs_i64(dur_secs)),
            has_video: true,
            has_audio: true,
            width: 320,
            height: 180,
            fps: None,
            info: String::new(),
            proxy_path: None,
        }
    }

    #[test]
    fn audio_usa_fast_e_video_usa_linear_por_padrao() {
        let m = make_media(10);
        let mut at = Track::new(TrackKind::Audio, "A1");
        let a = Event::new(&m, Ticks::from_secs_i64(0), Ticks::from_secs_i64(5));
        let b = Event::new(&m, Ticks::from_secs_i64(3), Ticks::from_secs_i64(5));
        at.events.push(a);
        at.events.push(b);

        let ax = track_crossfades(&at, false);
        assert_eq!(ax.len(), 1);
        assert!(ax[0].active);
        assert_eq!(ax[0].start, Ticks::from_secs_i64(3));
        assert_eq!(ax[0].end, Ticks::from_secs_i64(5));
        assert_eq!(ax[0].out_curve, Curve::Fast);
        assert_eq!(ax[0].in_curve, Curve::Fast);

        let mut vt = Track::new(TrackKind::Video, "V1");
        vt.events.push(Event::new(
            &m,
            Ticks::from_secs_i64(0),
            Ticks::from_secs_i64(5),
        ));
        vt.events.push(Event::new(
            &m,
            Ticks::from_secs_i64(3),
            Ticks::from_secs_i64(5),
        ));
        let vx = track_crossfades(&vt, false);
        assert_eq!(vx.len(), 1);
        assert_eq!(vx[0].out_curve, Curve::Linear);
        assert_eq!(vx[0].in_curve, Curve::Linear);
    }

    #[test]
    fn curva_personalizada_e_no_crossfade_sao_respeitados() {
        let m = make_media(10);
        let mut at = Track::new(TrackKind::Audio, "A1");
        let mut a = Event::new(&m, Ticks::from_secs_i64(0), Ticks::from_secs_i64(5));
        let mut b = Event::new(&m, Ticks::from_secs_i64(4), Ticks::from_secs_i64(5));
        a.crossfade_out_curve = Some(Curve::Slow);
        b.crossfade_in_curve = Some(Curve::Linear);
        at.events.push(a);
        at.events.push(b);

        let xf = track_crossfades(&at, false);
        assert_eq!(xf.len(), 1);
        assert_eq!(xf[0].out_curve, Curve::Slow);
        assert_eq!(xf[0].in_curve, Curve::Linear);
        let (fi_b, _) = effective_audio_fades(&at, 1);
        assert_eq!(fi_b.len, Ticks::from_secs_i64(1));
        assert_eq!(fi_b.curve, Curve::Linear);

        // Desativar crossfade gera corte seco (sem fade automático no áudio).
        at.events[1].no_crossfade_in = true;
        let (fi_b_off, _) = effective_audio_fades(&at, 1);
        assert_eq!(fi_b_off.len, Ticks::ZERO);
    }

    #[test]
    fn sync_new_overlaps_respeita_auto_crossfades_e_preserva_antigos() {
        let m = make_media(10);
        let mut tracks = vec![Track::new(TrackKind::Video, "V1")];
        tracks[0].events.push(Event::new(
            &m,
            Ticks::from_secs_i64(0),
            Ticks::from_secs_i64(4),
        ));
        tracks[0].events.push(Event::new(
            &m,
            Ticks::from_secs_i64(3),
            Ticks::from_secs_i64(4),
        ));
        tracks[0].events.push(Event::new(
            &m,
            Ticks::from_secs_i64(8),
            Ticks::from_secs_i64(4),
        ));

        // (0, 1) já existia; agora movemos 2 sobre 1 com auto_crossfades = false.
        let prev = overlap_pairs(&tracks);
        tracks[0].events[2].start = Ticks::from_secs_i64(6);
        sync_new_overlaps(&mut tracks, &[0], &prev, false, Curve::Fast, Curve::Linear);
        // Crossfade antigo (0, 1) permanece ativo; novo (1, 2) vira corte seco.
        assert!(!tracks[0].events[1].no_crossfade_in);
        assert!(tracks[0].events[2].no_crossfade_in);

        // Afastar 2 limpa `no_crossfade_in`.
        let prev2 = overlap_pairs(&tracks);
        tracks[0].events[2].start = Ticks::from_secs_i64(10);
        sync_new_overlaps(&mut tracks, &[0], &prev2, false, Curve::Fast, Curve::Linear);
        assert!(!tracks[0].events[2].no_crossfade_in);
    }

    #[test]
    fn slide_crossfade_desloca_janela_mantendo_duracao_e_sincronia() {
        let m = make_media(20);
        let mut p = Project::new_default();
        p.media.push(m.clone());
        let mut t = Track::new(TrackKind::Video, "V1");
        let mut a = Event::new(&m, Ticks::from_secs_i64(0), Ticks::from_secs_i64(5));
        a.looped = false;
        let mut b = Event::new(&m, Ticks::from_secs_i64(4), Ticks::from_secs_i64(5));
        b.looped = false;
        b.offset = Ticks::from_secs_i64(3);
        let a_id = a.id;
        let b_id = b.id;
        t.events.push(a);
        t.events.push(b);
        p.tracks = vec![t];

        let pairs = capture_slide_crossfade(&p, 0, a_id, b_id, false);
        assert_eq!(pairs.len(), 1);
        let fd = p.settings.frame_dur();
        assert!(apply_slide_crossfade(
            &mut p,
            &pairs,
            Ticks::from_secs_i64(1),
            fd
        ));

        let a2 = &p.tracks[0].events[0];
        let b2 = &p.tracks[0].events[1];
        assert_eq!(a2.start, Ticks::ZERO);
        assert_eq!(a2.length, Ticks::from_secs_i64(6));
        assert_eq!(b2.start, Ticks::from_secs_i64(5));
        assert_eq!(b2.length, Ticks::from_secs_i64(4));
        assert_eq!(b2.offset, Ticks::from_secs_i64(4));
        assert_eq!(a2.end() - b2.start, Ticks::from_secs_i64(1));
    }

    #[test]
    fn cut_to_overlap_e_overlap_to_cut_preservam_limites_exatos() {
        let m = make_media(10);
        let mut a = Event::new(&m, Ticks::from_secs_i64(0), Ticks::from_secs_i64(4));
        a.looped = false;
        let mut b = Event::new(&m, Ticks::from_secs_i64(4), Ticks::from_secs_i64(4));
        b.looped = false;
        b.offset = Ticks::from_secs_i64(2);

        let min_len = Ticks::from_millis_i64(33);
        assert!(convert_cut_pair_to_overlap(
            &mut a,
            &mut b,
            m.duration,
            TrackKind::Video,
            Ticks::from_secs_i64(1),
            CutToOverlapAlign::Center,
            min_len,
            Curve::Linear,
        ));
        assert_eq!(a.end(), Ticks::from_millis_i64(4500));
        assert_eq!(b.start, Ticks::from_millis_i64(3500));
        assert_eq!(b.offset, Ticks::from_millis_i64(1500));
        assert_eq!(a.end() - b.start, Ticks::from_secs_i64(1));

        assert!(convert_overlap_pair_to_cut(
            &mut a,
            &mut b,
            CutToOverlapAlign::Center,
            min_len,
        ));
        assert_eq!(a.end(), Ticks::from_secs_i64(4));
        assert_eq!(b.start, Ticks::from_secs_i64(4));
        assert_eq!(b.offset, Ticks::from_secs_i64(2));
    }

    #[test]
    fn comandos_sessao_auto_crossfades_e_conversao_corte_overlap() {
        use crate::commands::{run_command, HeadlessHost};
        use crate::session::EditorSession;

        let mut p = Project::new_default();
        p.tracks.clear();
        let m = make_media(10);
        p.media.push(m.clone());
        let mut tr = Track::new(TrackKind::Video, "V1");
        let a = Event::new(&m, Ticks::ZERO, Ticks::from_secs_i64(4));
        let mut b = Event::new(&m, Ticks::from_secs_i64(4), Ticks::from_secs_i64(4));
        b.offset = Ticks::from_secs_i64(2);
        let a_id = a.id;
        let b_id = b.id;
        tr.events.push(a);
        tr.events.push(b);
        p.tracks.push(tr);

        let mut sess = EditorSession::new(p);
        let mut host = HeadlessHost;
        assert!(sess.opts.auto_crossfades);

        // Alternar `options.auto_crossfades` pela Tabela Única
        run_command("options.auto_crossfades", &mut sess, &mut host).expect("toggle off");
        assert!(!sess.opts.auto_crossfades);
        run_command("options.auto_crossfades", &mut sess, &mut host).expect("toggle on");
        assert!(sess.opts.auto_crossfades);

        // Converter corte em crossfade no cursor (t = 4s)
        sess.cursor = Ticks::from_secs_i64(4);
        assert!(sess.can_convert_cut_to_overlap());
        run_command("edit.cut_to_overlap", &mut sess, &mut host).expect("cut to overlap");
        let xfs = track_crossfades(&sess.project.tracks[0], false);
        assert_eq!(xfs.len(), 1);
        assert!(xfs[0].active);
        assert_eq!(xfs[0].len(), Ticks::from_secs_i64(1));

        // Alterar curvas do crossfade com Desfazer
        assert!(sess.set_crossfade_curves(a_id, b_id, Curve::Smooth, Curve::Sharp));
        let xfs2 = track_crossfades(&sess.project.tracks[0], false);
        assert_eq!(xfs2[0].out_curve, Curve::Smooth);
        assert_eq!(xfs2[0].in_curve, Curve::Sharp);

        // Converter crossfade de volta em corte seco
        run_command("edit.crossfade_to_cut", &mut sess, &mut host).expect("crossfade to cut");
        assert!(track_crossfades(&sess.project.tracks[0], false).is_empty());
        assert_eq!(
            sess.project.tracks[0].events[0].end(),
            Ticks::from_secs_i64(4)
        );
        assert_eq!(
            sess.project.tracks[0].events[1].start,
            Ticks::from_secs_i64(4)
        );
    }
}
