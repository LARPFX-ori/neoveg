//! Ferramentas avançadas de áudio (paridade VEGAS Pro 21/2026):
//! - **Auto Normalize** (*Normalizar áudio automaticamente* para alvo em dBFS);
//! - **Auto Ducking** (análise RMS da trilha gatilho e geração de envelope de volume nas trilhas alvo);
//! - **Detecção de Batidas e Andamento (BPM)** (fluxo espectral/energia + autocorrelação em Rust puro);
//! - **Detecção e Reparo de Dessincronia A/V** (*Synchronize by Moving / Slipping*).
//!
//! Política zero pânico (§0) e tempo exato inteiro (`Ticks`, Regra Rust 5).

// Laços indexam buffers estéreo intercalados e janelas de autocorrelação DSP.
#![allow(clippy::needless_range_loop)]

use crate::media::AudioData;
use crate::model::{
    db_to_lin, lin_to_db, Curve, EnvKind, EnvPoint, Envelope, Event, Id, Marker, MarkerKind,
    Project, Ticks, TrackKind,
};
use crate::session::EditorSession;
use std::collections::HashMap;
use std::sync::Arc;

/// Limite máximo de marcadores inseridos automaticamente pela detecção de batidas (§0.4).
pub const MAX_BEAT_MARKERS: usize = 256;
/// Limite máximo de pontos de envelope gerados pelo Auto Ducking por trilha (§0.4).
const MAX_DUCKING_POINTS: usize = 2048;

/// Calcula o pico absoluto linear (L/R) do trecho ativo de um evento de áudio em `audio`.
pub fn event_audio_peak(ev: &Event, audio: &AudioData) -> f32 {
    if audio.sample_rate == 0 || audio.data.len() < 2 {
        return 0.0;
    }
    let total_frames = audio.data.len() / 2;
    let start_f = ev.offset.to_samples_floor(audio.sample_rate).max(0) as usize;
    let scaled_len = ev.rate.scale_ticks(ev.length).max(Ticks::ZERO);
    let len_f = scaled_len.to_samples_round(audio.sample_rate).max(0) as usize;
    if start_f >= total_frames || len_f == 0 {
        return 0.0;
    }
    let end_f = start_f.saturating_add(len_f).min(total_frames);
    let mut peak = 0.0_f32;
    for f in start_f..end_f {
        let l = audio.data[f * 2].abs();
        let r = audio.data[f * 2 + 1].abs();
        if l.is_finite() {
            peak = peak.max(l);
        }
        if r.is_finite() {
            peak = peak.max(r);
        }
    }
    peak
}

/// Calcula o ganho em dB necessário para normalizar `ev` ao pico alvo `target_dbfs`
/// (ex.: `-0.1 dBFS`), limitado com segurança a `[-24.0, +24.0]` dB.
pub fn compute_normalize_gain_db(ev: &Event, audio: &AudioData, target_dbfs: f32) -> Option<f32> {
    let peak = event_audio_peak(ev, audio);
    if !peak.is_finite() || peak < 1e-6 {
        return None;
    }
    let target = if target_dbfs.is_finite() {
        target_dbfs.clamp(-36.0, 0.0)
    } else {
        -0.1
    };
    let peak_db = lin_to_db(peak);
    if !peak_db.is_finite() {
        return None;
    }
    Some((target - peak_db).clamp(-24.0, 24.0))
}

/// Normaliza todos os eventos de áudio selecionados em `session` (passo único de Desfazer).
/// Retorna quantos eventos tiveram `gain_db` ajustado.
pub fn auto_normalize_selected(
    session: &mut EditorSession,
    audio_map: &HashMap<Id, Arc<AudioData>>,
    target_dbfs: f32,
) -> usize {
    let targets: Vec<(usize, usize, f32)> = session
        .project
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, tr)| tr.kind == TrackKind::Audio && !tr.locked)
        .flat_map(|(ti, tr)| {
            tr.events.iter().enumerate().filter_map(move |(ei, ev)| {
                if !session.sel_events.contains(&ev.id) || ev.locked {
                    return None;
                }
                let data = audio_map.get(&ev.media_id)?;
                let gain = compute_normalize_gain_db(ev, data, target_dbfs)?;
                Some((ti, ei, gain))
            })
        })
        .collect();

    if targets.is_empty() {
        return 0;
    }
    session.checkpoint();
    for (ti, ei, gain) in &targets {
        if let Some(ev) = session
            .project
            .tracks
            .get_mut(*ti)
            .and_then(|t| t.events.get_mut(*ei))
        {
            ev.gain_db = *gain;
        }
    }
    session.changed();
    targets.len()
}

/// Configuração do *Auto Ducking* (VEGAS Pro 2026: atenuação automática de música durante fala).
#[derive(Clone, Debug)]
pub struct AutoDuckingSpec {
    /// Índice da trilha de áudio gatilho (ex.: narração / diálogo).
    pub trigger_track: usize,
    /// Índices das trilhas de áudio alvo que receberão envelope de volume (ex.: trilha musical).
    pub target_tracks: Vec<usize>,
    /// Limiar de disparo em dBFS (`-60.0..=-3.0`, padrão `-24.0`).
    pub threshold_db: f32,
    /// Atenuação aplicada durante o disparo em dB (`-48.0..=-1.0`, padrão `-12.0`).
    pub attenuation_db: f32,
    /// Tempo de ataque em ms (`10..=2000`, padrão `80`).
    pub attack_ms: u32,
    /// Tempo de liberação em ms (`20..=4000`, padrão `350`).
    pub release_ms: u32,
    /// Tempo mínimo de retenção para unir pausas curtas em ms (`0..=2000`, padrão `180`).
    pub hold_ms: u32,
}

impl Default for AutoDuckingSpec {
    fn default() -> Self {
        Self {
            trigger_track: 0,
            target_tracks: Vec::new(),
            threshold_db: -24.0,
            attenuation_db: -12.0,
            attack_ms: 80,
            release_ms: 350,
            hold_ms: 180,
        }
    }
}

/// Detecta intervalos `[start, end]` em que a trilha gatilho ultrapassa `threshold_db`.
pub fn detect_trigger_intervals(
    project: &Project,
    audio_map: &HashMap<Id, Arc<AudioData>>,
    trigger_track: usize,
    threshold_db: f32,
    hold_ms: u32,
) -> Vec<(Ticks, Ticks)> {
    let Some(track) = project.tracks.get(trigger_track) else {
        return Vec::new();
    };
    if track.kind != TrackKind::Audio || track.events.is_empty() {
        return Vec::new();
    }
    let thresh_lin = db_to_lin(if threshold_db.is_finite() {
        threshold_db.clamp(-60.0, -1.0)
    } else {
        -24.0
    });
    let win_ms: i64 = 20;
    let win_ticks = Ticks::from_millis_i64(win_ms);
    let hold_ticks = Ticks::from_millis_i64(i64::from(hold_ms.min(5000)));
    let mut raw: Vec<(Ticks, Ticks)> = Vec::new();

    for ev in &track.events {
        if ev.mute || ev.length <= Ticks::ZERO {
            continue;
        }
        let Some(audio) = audio_map.get(&ev.media_id) else {
            continue;
        };
        if audio.sample_rate == 0 || audio.data.len() < 2 {
            continue;
        }
        let sr = audio.sample_rate;
        let total_frames = audio.data.len() / 2;
        let ev_gain = db_to_lin(ev.gain_db);
        let steps = ((ev.length.0 / win_ticks.0).max(1) as usize).min(60_000);
        let mut active_start: Option<Ticks> = None;

        for s in 0..steps {
            let t_tl = ev.start + Ticks::from_raw(win_ticks.0.saturating_mul(s as i64));
            let t_end = (t_tl + win_ticks).min(ev.end());
            let Some(mt0) = ev.media_time(t_tl, Some(Ticks::from_samples(total_frames as i64, sr)))
            else {
                if let Some(st) = active_start.take() {
                    raw.push((st, t_tl));
                }
                continue;
            };
            let f0 = mt0.to_samples_floor(sr).max(0) as usize;
            let n_win = ((sr as u64 * win_ms as u64) / 1000).max(1) as usize;
            let f1 = f0.saturating_add(n_win).min(total_frames);
            if f1 <= f0 {
                continue;
            }
            let mut sum_sq = 0.0_f64;
            for f in f0..f1 {
                let l = f64::from(audio.data[f * 2]);
                let r = f64::from(audio.data[f * 2 + 1]);
                let mono = 0.5 * (l + r);
                sum_sq += mono * mono;
            }
            let rms = ((sum_sq / (f1 - f0) as f64).sqrt() as f32) * ev_gain;
            if rms >= thresh_lin {
                if active_start.is_none() {
                    active_start = Some(t_tl);
                }
            } else if let Some(st) = active_start.take() {
                raw.push((st, t_tl));
            }
            if s + 1 == steps {
                if let Some(st) = active_start.take() {
                    raw.push((st, t_end));
                }
            }
        }
    }

    if raw.is_empty() {
        return raw;
    }
    raw.sort_by_key(|&(a, _)| a);
    let mut merged: Vec<(Ticks, Ticks)> = Vec::with_capacity(raw.len());
    for (a, b) in raw {
        if let Some(last) = merged.last_mut() {
            if a <= last.1 + hold_ticks {
                last.1 = last.1.max(b);
                continue;
            }
        }
        merged.push((a, b));
    }
    merged
}

/// Aplica *Auto Ducking* nas trilhas alvo da sessão em 1 passo de Desfazer, escrevendo um
/// envelope `EnvKind::Volume` suave. Retorna `(trilhas_atualizadas, intervalos_detectados)`.
pub fn apply_auto_ducking(
    session: &mut EditorSession,
    audio_map: &HashMap<Id, Arc<AudioData>>,
    spec: &AutoDuckingSpec,
) -> (usize, usize) {
    let intervals = detect_trigger_intervals(
        &session.project,
        audio_map,
        spec.trigger_track,
        spec.threshold_db,
        spec.hold_ms,
    );
    if intervals.is_empty() {
        return (0, 0);
    }

    let att_db = if spec.attenuation_db.is_finite() {
        spec.attenuation_db.clamp(-48.0, -0.5)
    } else {
        -12.0
    };
    let duck_lin = db_to_lin(att_db).clamp(0.0, 1.0);
    let attack = Ticks::from_millis_i64(i64::from(spec.attack_ms.clamp(10, 2000)));
    let release = Ticks::from_millis_i64(i64::from(spec.release_ms.clamp(20, 4000)));

    let mut pts: Vec<EnvPoint> =
        Vec::with_capacity((intervals.len() * 4 + 1).min(MAX_DUCKING_POINTS));
    pts.push(EnvPoint {
        t: Ticks::ZERO,
        v: 1.0,
        curve: Curve::Smooth,
        smoothness: 0.0,
    });
    for &(start, end) in &intervals {
        if pts.len() + 4 > MAX_DUCKING_POINTS {
            break;
        }
        let pre = (start - attack).max(Ticks::ZERO);
        let post = end + release;
        pts.push(EnvPoint {
            t: pre,
            v: 1.0,
            curve: Curve::Smooth,
            smoothness: 0.0,
        });
        pts.push(EnvPoint {
            t: start,
            v: duck_lin,
            curve: Curve::Linear,
            smoothness: 0.0,
        });
        pts.push(EnvPoint {
            t: end,
            v: duck_lin,
            curve: Curve::Smooth,
            smoothness: 0.0,
        });
        pts.push(EnvPoint {
            t: post,
            v: 1.0,
            curve: Curve::Linear,
            smoothness: 0.0,
        });
    }
    pts.sort_by_key(|p| p.t);
    pts.dedup_by(|b, a| b.t == a.t);

    let valid_targets: Vec<usize> = spec
        .target_tracks
        .iter()
        .copied()
        .filter(|&ti| {
            ti != spec.trigger_track
                && session
                    .project
                    .tracks
                    .get(ti)
                    .is_some_and(|t| t.kind == TrackKind::Audio && !t.locked)
        })
        .collect();
    if valid_targets.is_empty() {
        return (0, intervals.len());
    }

    session.checkpoint();
    for ti in &valid_targets {
        if let Some(track) = session.project.tracks.get_mut(*ti) {
            if let Some(env) = track
                .envelopes
                .iter_mut()
                .find(|e| e.kind == EnvKind::Volume)
            {
                env.points = pts.clone();
                env.visible = true;
            } else {
                track.envelopes.push(Envelope {
                    kind: EnvKind::Volume,
                    points: pts.clone(),
                    visible: true,
                });
            }
        }
    }
    session.opts.show_envelopes = true;
    session.changed();
    (valid_targets.len(), intervals.len())
}

/// Resultado da detecção de batidas e andamento (BPM).
#[derive(Clone, Debug, PartialEq)]
pub struct TempoDetectionResult {
    pub bpm: f64,
    /// Deslocamento da primeira batida dentro do evento (em ticks).
    pub first_beat_offset: Ticks,
    /// Confiança relativa (`0.0..=1.0`).
    pub confidence: f32,
}

/// Estima o andamento (BPM em `[60.0, 180.0]`) e a fase da primeira batida de um evento de áudio
/// usando fluxo de energia positiva em janelas de 10 ms + autocorrelação (Rust puro, §0).
pub fn detect_tempo_for_event(ev: &Event, audio: &AudioData) -> Option<TempoDetectionResult> {
    if audio.sample_rate == 0 || audio.data.len() < 2 || ev.length <= Ticks::ZERO {
        return None;
    }
    let sr = audio.sample_rate;
    let total_frames = audio.data.len() / 2;
    let start_f = ev.offset.to_samples_floor(sr).max(0) as usize;
    let scaled_len = ev.rate.scale_ticks(ev.length).max(Ticks::ZERO);
    // Analisa no máximo 90 segundos para manter resposta instantânea (§0.4)
    let max_frames = (sr as usize).saturating_mul(90);
    let len_f = (scaled_len.to_samples_round(sr).max(0) as usize).min(max_frames);
    if start_f >= total_frames || len_f < (sr as usize).saturating_mul(2) {
        return None;
    }
    let end_f = start_f.saturating_add(len_f).min(total_frames);
    let hop = ((sr as usize) / 100).max(1); // ~10 ms por passo -> 100 Hz
    let num_hops = (end_f - start_f) / hop;
    if num_hops < 120 {
        return None;
    }

    let mut energy = Vec::with_capacity(num_hops);
    for h in 0..num_hops {
        let base = start_f + h * hop;
        let stop = (base + hop).min(end_f);
        let mut e = 0.0_f64;
        for f in base..stop {
            let m = 0.5 * (f64::from(audio.data[f * 2]) + f64::from(audio.data[f * 2 + 1]));
            e += m * m;
        }
        energy.push((e / (stop - base).max(1) as f64).sqrt());
    }

    // Envelope de onset: derivada retificada positiva da energia
    let mut onset = vec![0.0_f64; num_hops];
    for i in 1..num_hops {
        let d = energy[i] - energy[i - 1];
        if d > 0.0 {
            onset[i] = d;
        }
    }
    let mean = onset.iter().sum::<f64>() / num_hops as f64;
    if mean <= 1e-7 {
        return None;
    }
    for v in &mut onset {
        *v = (*v - mean * 0.5).max(0.0);
    }

    // Autocorrelação para lags correspondentes a 60..=180 BPM
    let hop_hz = sr as f64 / hop as f64;
    let min_lag = ((hop_hz * 60.0 / 180.0).floor() as usize).max(10);
    let max_lag = ((hop_hz * 60.0 / 60.0).ceil() as usize).min(num_hops / 2);
    if max_lag <= min_lag {
        return None;
    }

    let mut best_lag = min_lag;
    let mut best_score = 0.0_f64;
    let mut total_score = 0.0_f64;
    let mut lag_count = 0usize;

    for lag in min_lag..=max_lag {
        let n = num_hops - lag;
        let mut s = 0.0_f64;
        for i in 0..n {
            s += onset[i] * onset[i + lag];
        }
        let norm = s / n as f64;
        // Reforço harmônico leve em 2*lag (evita erro de oitava)
        let harm = if lag * 2 < num_hops {
            let n2 = num_hops - lag * 2;
            let mut s2 = 0.0_f64;
            for i in 0..n2 {
                s2 += onset[i] * onset[i + lag * 2];
            }
            s2 / n2 as f64
        } else {
            0.0
        };
        let score = norm + 0.35 * harm;
        total_score += score;
        lag_count += 1;
        if score > best_score {
            best_score = score;
            best_lag = lag;
        }
    }

    if best_score <= 0.0 || lag_count == 0 {
        return None;
    }

    let raw_bpm = (60.0 * hop_hz / best_lag as f64).clamp(60.0, 180.0);
    let bpm = (raw_bpm * 2.0).round() / 2.0; // passo de 0.5 BPM para leitura limpa
    let avg_score = (total_score / lag_count as f64).max(1e-12);
    let confidence = ((best_score / (avg_score * 4.0)).clamp(0.0, 1.0)) as f32;

    // Fase da primeira batida: soma do envelope em cada fase `0..best_lag`
    let mut best_phase = 0usize;
    let mut best_phase_val = -1.0_f64;
    for ph in 0..best_lag {
        let mut acc = 0.0_f64;
        let mut idx = ph;
        while idx < num_hops {
            acc += onset[idx];
            idx += best_lag;
        }
        if acc > best_phase_val {
            best_phase_val = acc;
            best_phase = ph;
        }
    }
    let first_beat_samples = (best_phase * hop) as i64;
    let first_beat_offset = Ticks::from_samples(first_beat_samples, sr);

    Some(TempoDetectionResult {
        bpm,
        first_beat_offset,
        confidence,
    })
}

/// Aplica o resultado de detecção de andamento ao projeto: atualiza `settings.bpm` e, se
/// `insert_markers`, insere marcadores de batida ao longo de `ev` em 1 passo de Desfazer.
/// Retorna quantos marcadores foram inseridos.
pub fn apply_tempo_detection(
    session: &mut EditorSession,
    ev: &Event,
    result: &TempoDetectionResult,
    update_project_bpm: bool,
    insert_markers: bool,
    every_nth_beat: usize,
) -> usize {
    if !result.bpm.is_finite() || result.bpm < 20.0 || result.bpm > 300.0 {
        return 0;
    }
    session.checkpoint();
    if update_project_bpm {
        session.project.settings.bpm = result.bpm;
    }
    let mut added = 0usize;
    if insert_markers {
        let beat_ticks =
            Ticks::from_secs_f64_clamped(60.0 / result.bpm).max(Ticks::from_millis_i64(50));
        let step_n = every_nth_beat.clamp(1, 16);
        let step = beat_ticks.saturating_mul(step_n as i64);
        let mut pos = ev.start + result.first_beat_offset;
        let end = ev.end();
        let mut beat_idx = 1usize;
        while pos < end && added < MAX_BEAT_MARKERS {
            let label = if step_n > 1 {
                format!("Compasso {beat_idx}")
            } else {
                format!("Batida {beat_idx}")
            };
            session
                .project
                .markers
                .push(Marker::new(pos, MarkerKind::Standard, label));
            pos += step;
            beat_idx += 1;
            added += 1;
        }
        session.project.markers.sort_by_key(|m| m.t);
    }
    session.changed();
    added
}

/// Informação de dessincronia A/V entre eventos agrupados que compartilham o mesmo arquivo de mídia.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncOffsetInfo {
    pub event_id: Id,
    pub partner_id: Id,
    /// Desvio em ticks (`ev.start - expected_start`): positivo = adiantado/atrasado à direita.
    pub delta_ticks: Ticks,
    /// Desvio aproximado em quadros do projeto.
    pub delta_frames: i64,
}

/// Verifica se o evento `ev` (na trilha de tipo `kind`) está dessincronizado em relação ao seu par
/// A/V agrupado (mesmo `group` e mesmo `media_id`, tipo de trilha oposto).
pub fn event_sync_offset(project: &Project, kind: TrackKind, ev: &Event) -> Option<SyncOffsetInfo> {
    let group_id = ev.group?;
    if ev.media_id == 0 {
        return None;
    }
    let other_kind = match kind {
        TrackKind::Video => TrackKind::Audio,
        TrackKind::Audio => TrackKind::Video,
    };
    let partner = project
        .tracks
        .iter()
        .filter(|t| t.kind == other_kind)
        .flat_map(|t| t.events.iter())
        .find(|other| {
            other.id != ev.id && other.group == Some(group_id) && other.media_id == ev.media_id
        })?;

    // Se ambos tocam na mesma taxa, `ev.start - partner.start` deve igualar
    // `(ev.offset - partner.offset) / rate` (quando rate == 1, `ev.offset - partner.offset`).
    let rate = ev.rate.as_f64();
    let off_diff_raw = ev.offset.0.saturating_sub(partner.offset.0);
    let expected_shift = if rate.is_finite() && rate > 1e-4 {
        (off_diff_raw as f64 / rate).round() as i64
    } else {
        off_diff_raw
    };
    let expected_start = Ticks::from_raw(partner.start.0.saturating_add(expected_shift));
    let delta = ev.start - expected_start;
    let frame_dur = project.settings.frame_dur().max(Ticks::from_millis_i64(1));
    if delta.abs() < frame_dur.div_i64(2).max(Ticks::from_millis_i64(2)) {
        return None;
    }
    let delta_frames = if frame_dur.0 > 0 {
        (delta.0
            + if delta.0 >= 0 {
                frame_dur.0 / 2
            } else {
                -frame_dur.0 / 2
            })
            / frame_dur.0
    } else {
        0
    };
    Some(SyncOffsetInfo {
        event_id: ev.id,
        partner_id: partner.id,
        delta_ticks: delta,
        delta_frames,
    })
}

/// Repara a dessincronia A/V de `event_id` movendo sua posição na timeline para alinhar ao par.
pub fn repair_sync_move(session: &mut EditorSession, event_id: Id) -> bool {
    let Some((ti, ei)) = session.project.find_event(event_id) else {
        return false;
    };
    let Some(track) = session.project.tracks.get(ti) else {
        return false;
    };
    if track.locked {
        return false;
    }
    let kind = track.kind;
    let Some(ev) = track.events.get(ei) else {
        return false;
    };
    if ev.locked {
        return false;
    }
    let Some(info) = event_sync_offset(&session.project, kind, ev) else {
        return false;
    };
    let new_start = (ev.start - info.delta_ticks).max(Ticks::ZERO);
    session.checkpoint();
    if let Some(ev_mut) = session
        .project
        .tracks
        .get_mut(ti)
        .and_then(|t| t.events.get_mut(ei))
    {
        ev_mut.start = new_start;
    }
    session.project.sort_events();
    session.changed();
    true
}

/// Repara a dessincronia A/V de `event_id` deslizando (`offset`) o conteúdo interno do evento
/// sem alterar seus pontos de entrada/saída na timeline (*Synchronize by Slipping*).
pub fn repair_sync_slip(session: &mut EditorSession, event_id: Id) -> bool {
    let Some((ti, ei)) = session.project.find_event(event_id) else {
        return false;
    };
    let Some(track) = session.project.tracks.get(ti) else {
        return false;
    };
    if track.locked {
        return false;
    }
    let kind = track.kind;
    let Some(ev) = track.events.get(ei) else {
        return false;
    };
    if ev.locked {
        return false;
    }
    let Some(info) = event_sync_offset(&session.project, kind, ev) else {
        return false;
    };
    let offset_shift = ev.rate.scale_ticks(info.delta_ticks);
    let new_offset = (ev.offset + offset_shift).max(Ticks::ZERO);
    session.checkpoint();
    if let Some(ev_mut) = session
        .project
        .tracks
        .get_mut(ti)
        .and_then(|t| t.events.get_mut(ei))
    {
        ev_mut.offset = new_offset;
    }
    session.changed();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{new_id, AudioPanMode, MediaItem, MediaSource, Track};

    fn make_audio_media(id: Id, name: &str, secs: i64) -> MediaItem {
        MediaItem {
            id,
            name: name.into(),
            source: MediaSource::File {
                path: format!("{name}.wav"),
            },
            duration: Some(Ticks::from_secs_i64(secs)),
            has_video: false,
            has_audio: true,
            width: 0,
            height: 0,
            fps: None,
            info: String::new(),
            proxy_path: None,
        }
    }

    #[test]
    fn auto_normalize_ajusta_ganho_para_pico_alvo() {
        let sr = 48_000u32;
        let frames = sr as usize;
        let mut data = vec![0.0_f32; frames * 2];
        for i in 0..frames {
            let s = 0.25 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sr as f32).sin();
            data[i * 2] = s;
            data[i * 2 + 1] = s;
        }
        let audio = Arc::new(AudioData::from_stereo(sr, data));
        let mid = new_id();
        let m = make_audio_media(mid, "tom", 1);
        let mut p = Project::new_default();
        p.tracks.clear();
        let mut tr = Track::new(TrackKind::Audio, "A1");
        let ev = Event::new(&m, Ticks::ZERO, Ticks::from_secs_i64(1));
        let ev_id = ev.id;
        tr.events.push(ev);
        p.media.push(m);
        p.tracks.push(tr);

        let mut sess = EditorSession::new(p);
        sess.sel_events.insert(ev_id);
        let mut map = HashMap::new();
        map.insert(mid, audio);

        let n = auto_normalize_selected(&mut sess, &map, -0.1);
        assert_eq!(n, 1);
        let g = sess.project.tracks[0].events[0].gain_db;
        // 0.25 = -12.04 dBFS -> ganho esperado ~ +11.94 dB
        assert!((g - 11.94).abs() < 0.25, "ganho inesperado: {g}");
    }

    #[test]
    fn auto_ducking_gera_envelope_de_volume_durante_fala() {
        let sr = 48_000u32;
        let frames = sr as usize * 3;
        let mut data = vec![0.0_f32; frames * 2];
        // Sinal forte apenas entre 1.0s e 2.0s
        for i in (sr as usize)..(sr as usize * 2) {
            let s = 0.5 * (2.0 * std::f32::consts::PI * 300.0 * i as f32 / sr as f32).sin();
            data[i * 2] = s;
            data[i * 2 + 1] = s;
        }
        let audio = Arc::new(AudioData::from_stereo(sr, data));
        let mid = new_id();
        let m = make_audio_media(mid, "voz", 3);
        let mut p = Project::new_default();
        p.tracks.clear();
        let mut voz = Track::new(TrackKind::Audio, "Voz");
        voz.events
            .push(Event::new(&m, Ticks::ZERO, Ticks::from_secs_i64(3)));
        let musica = Track::new(TrackKind::Audio, "Música");
        p.media.push(m);
        p.tracks.push(voz);
        p.tracks.push(musica);

        let mut sess = EditorSession::new(p);
        let mut map = HashMap::new();
        map.insert(mid, audio);

        let spec = AutoDuckingSpec {
            trigger_track: 0,
            target_tracks: vec![1],
            threshold_db: -20.0,
            attenuation_db: -12.0,
            attack_ms: 50,
            release_ms: 150,
            hold_ms: 100,
        };
        let (tracks_n, iv_n) = apply_auto_ducking(&mut sess, &map, &spec);
        assert_eq!(tracks_n, 1);
        assert_eq!(iv_n, 1);
        let env = sess.project.tracks[1]
            .envelope(EnvKind::Volume)
            .expect("envelope volume criado");
        let v_before = env.eval(Ticks::from_millis_i64(300));
        let v_during = env.eval(Ticks::from_millis_i64(1500));
        let v_after = env.eval(Ticks::from_millis_i64(2500));
        assert!((v_before - 1.0).abs() < 0.05, "antes: {v_before}");
        assert!(v_during < 0.35, "durante fala deve atenuar: {v_during}");
        assert!((v_after - 1.0).abs() < 0.05, "depois: {v_after}");
    }

    #[test]
    fn deteccao_de_andamento_recupera_120_bpm_e_insere_marcadores() {
        let sr = 48_000u32;
        let secs = 6usize;
        let frames = sr as usize * secs;
        let mut data = vec![0.0_f32; frames * 2];
        // Cliques a cada 0.5s (120 BPM)
        let beat_frames = (sr / 2) as usize;
        for b in 0..(secs * 2) {
            let start = b * beat_frames;
            for k in 0..600 {
                if start + k < frames {
                    let env = 1.0 - (k as f32 / 600.0);
                    data[(start + k) * 2] = env * 0.9;
                    data[(start + k) * 2 + 1] = env * 0.9;
                }
            }
        }
        let audio = Arc::new(AudioData::from_stereo(sr, data));
        let mid = new_id();
        let m = make_audio_media(mid, "click120", secs as i64);
        let ev = Event::new(&m, Ticks::ZERO, Ticks::from_secs_i64(secs as i64));
        let res = detect_tempo_for_event(&ev, &audio).expect("bpm detectado");
        assert!(
            (res.bpm - 120.0).abs() <= 2.0,
            "esperado ~120 BPM, obtido {}",
            res.bpm
        );

        let mut sess = EditorSession::new(Project::new_default());
        let added = apply_tempo_detection(&mut sess, &ev, &res, true, true, 1);
        assert!(added >= 8);
        assert!((sess.project.settings.bpm - res.bpm).abs() < 0.01);
    }

    #[test]
    fn dessincronia_av_detecta_e_repara_por_move_e_slip() {
        let mid = new_id();
        let mut m = make_audio_media(mid, "clipe_av", 5);
        m.has_video = true;
        let mut p = Project::new_default();
        p.tracks.clear();
        let mut tv = Track::new(TrackKind::Video, "V1");
        let mut ta = Track::new(TrackKind::Audio, "A1");
        let gid = new_id();
        let mut ev_v = Event::new(&m, Ticks::from_secs_i64(2), Ticks::from_secs_i64(3));
        ev_v.group = Some(gid);
        let mut ev_a = Event::new(&m, Ticks::from_secs_i64(2), Ticks::from_secs_i64(3));
        ev_a.group = Some(gid);
        // Desloca o áudio em +500 ms sem ajustar offset
        ev_a.start = Ticks::from_millis_i64(2500);
        let aid = ev_a.id;
        tv.events.push(ev_v);
        ta.events.push(ev_a);
        p.media.push(m);
        p.tracks.push(tv);
        p.tracks.push(ta);

        let info = event_sync_offset(&p, TrackKind::Audio, &p.tracks[1].events[0])
            .expect("deve detectar dessincronia");
        assert_eq!(info.delta_ticks, Ticks::from_millis_i64(500));

        // Testa reparo por Slip
        let mut sess_slip = EditorSession::new(p.clone());
        assert!(repair_sync_slip(&mut sess_slip, aid));
        assert!(event_sync_offset(
            &sess_slip.project,
            TrackKind::Audio,
            &sess_slip.project.tracks[1].events[0]
        )
        .is_none());

        // Testa reparo por Move
        let mut sess_move = EditorSession::new(p);
        assert!(repair_sync_move(&mut sess_move, aid));
        assert_eq!(
            sess_move.project.tracks[1].events[0].start,
            Ticks::from_secs_i64(2)
        );
        assert!(event_sync_offset(
            &sess_move.project,
            TrackKind::Audio,
            &sess_move.project.tracks[1].events[0]
        )
        .is_none());
    }

    #[test]
    fn modos_de_panoramica_estereo_aplicam_curvas_esperadas() {
        let (l, r) = AudioPanMode::Balance0dB.apply_stereo(1.0, 1.0, 0.0);
        assert!((l - 1.0).abs() < 1e-5 && (r - 1.0).abs() < 1e-5);
        let (l, r) = AudioPanMode::BalanceMinus6dB.apply_stereo(1.0, 1.0, 0.0);
        assert!((l - 0.5).abs() < 1e-5 && (r - 0.5).abs() < 1e-5);
        let (l, r) = AudioPanMode::ConstantPower.apply_stereo(1.0, 1.0, 0.0);
        assert!((l * l + r * r - 1.0).abs() < 1e-4);
        let (l, r) = AudioPanMode::AddChannels.apply_stereo(0.5, 0.5, -1.0);
        assert!((l - 1.0).abs() < 1e-5 && r.abs() < 1e-5);
        let (l, r) = AudioPanMode::Film.apply_stereo(1.0, 1.0, 0.0);
        assert!((l - 0.5946).abs() < 0.01 && (r - 0.5946).abs() < 0.01);
    }
}
