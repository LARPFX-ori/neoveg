//! Mixagem de áudio: eventos (com reamostragem Sinc `rubato`) -> trilhas
//! (FX, volume, pan, envelopes, sends pré/pós-fader) -> barramentos auxiliares
//! (até 26 barramentos acíclicos com FX `fundsp`) -> Master Bus -> `TransportVarispeed`.

// Laços indexam buffers estéreo intercalados em paralelo.
#![allow(clippy::needless_range_loop)]

use super::afx::{self, FxState};
use super::surround;
use crate::media::AudioData;
use crate::model::*;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
    Adjustable, Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

#[derive(Default, Clone)]
pub struct Meters {
    /// Pico por trilha (L, R).
    pub tracks: HashMap<Id, [f32; 2]>,
    /// Pico por barramento auxiliar (L, R).
    pub buses: HashMap<Id, [f32; 2]>,
    pub master: [f32; 2],
}

fn sinc_params() -> SincInterpolationParameters {
    SincInterpolationParameters {
        sinc_len: 64,
        f_cutoff: None,
        oversampling_factor: 64,
        interpolation: SincInterpolationType::Cubic,
        window: WindowFunction::BlackmanHarris2,
    }
}

/// Limite seguro de taxa relativa para `rubato` (evita alocações extremas por entrada hostil).
const MIN_RUBATO_RATIO: f64 = 0.0625;
const MAX_RUBATO_RATIO: f64 = 16.0;
const VARISPEED_CHUNK: usize = 256;
/// Limite de compensação automática de latência VST3 por ramo (10 s @ 48 kHz; §0.4).
const MAX_PDC_FRAMES: usize = 480_000;

/// Linha de atraso estéreo intercalada usada para alinhar caminhos paralelos
/// na compensação automática de latência (PDC) de plug-ins VST3.
#[derive(Default, Clone)]
struct DelayLine {
    ring: Vec<f32>,
    pos: usize,
    frames: usize,
}

impl DelayLine {
    fn process(&mut self, delay_frames: usize, buf: &mut [f32]) {
        let target = delay_frames.min(MAX_PDC_FRAMES);
        if target == 0 {
            self.frames = 0;
            self.pos = 0;
            return;
        }
        if self.frames != target {
            self.ring.resize(target * 2, 0.0);
            self.ring.fill(0.0);
            self.pos = 0;
            self.frames = target;
        }
        for frame in buf.as_chunks_mut::<2>().0 {
            let idx = self.pos * 2;
            let dl = self.ring[idx];
            let dr = self.ring[idx + 1];
            self.ring[idx] = frame[0];
            self.ring[idx + 1] = frame[1];
            frame[0] = dl;
            frame[1] = dr;
            self.pos += 1;
            if self.pos == self.frames {
                self.pos = 0;
            }
        }
    }
}

/// Reamostrador estéreo contínuo baseado em `rubato::Async<f32>` (Sinc) usado
/// tanto para `Playback Rate` contínuo / shuttle J-K-L quanto para clipes com
/// taxa diferente da taxa do projeto.
pub struct TransportVarispeed {
    resampler: Option<Async<f32>>,
    fifo: VecDeque<f32>,
    head_pos: i64,
    active_pos: Option<i64>,
    in_buf: Vec<f32>,
    out_buf: Vec<f32>,
    scratch: Vec<f32>,
    scratch_part: Vec<f32>,
}

impl Default for TransportVarispeed {
    fn default() -> Self {
        Self::new()
    }
}

impl TransportVarispeed {
    pub fn new() -> Self {
        let resampler = Async::<f32>::new_sinc(
            1.0,
            MAX_RUBATO_RATIO,
            &sinc_params(),
            VARISPEED_CHUNK,
            2,
            FixedAsync::Output,
        )
        .ok();
        Self {
            resampler,
            fifo: VecDeque::with_capacity(VARISPEED_CHUNK * 4),
            head_pos: 0,
            active_pos: None,
            in_buf: Vec::new(),
            out_buf: vec![0.0_f32; VARISPEED_CHUNK * 2],
            scratch: Vec::new(),
            scratch_part: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        if let Some(r) = &mut self.resampler {
            r.reset();
        }
        self.fifo.clear();
        self.active_pos = None;
    }

    fn pull_forward_chunk(
        &mut self,
        frames_needed: usize,
        loop_range: Option<(i64, i64)>,
        mix_fn: &mut dyn FnMut(i64, &mut [f32]),
    ) {
        self.scratch.resize(frames_needed * 2, 0.0);
        let mut done = 0usize;
        while done < frames_needed {
            let mut take = frames_needed - done;
            if let Some((a, b)) = loop_range {
                if self.head_pos >= b && b > a {
                    self.head_pos = a;
                }
                if self.head_pos < b {
                    take = take.min((b - self.head_pos).max(1) as usize);
                }
            }
            let part = &mut self.scratch[done * 2..(done + take) * 2];
            mix_fn(self.head_pos, part);
            self.head_pos = self.head_pos.saturating_add(take as i64);
            done += take;
        }
    }

    fn pull_reverse_chunk(
        &mut self,
        frames_needed: usize,
        mix_fn: &mut dyn FnMut(i64, &mut [f32]),
    ) {
        self.scratch.resize(frames_needed * 2, 0.0);
        self.scratch.fill(0.0);
        if self.head_pos <= 0 {
            return;
        }
        let avail = (self.head_pos as usize).min(frames_needed);
        let start = self.head_pos - avail as i64;
        self.scratch_part.resize(avail * 2, 0.0);
        mix_fn(start, &mut self.scratch_part[..avail * 2]);
        for i in 0..avail {
            let src_idx = avail - 1 - i;
            self.scratch[i * 2] = self.scratch_part[src_idx * 2];
            self.scratch[i * 2 + 1] = self.scratch_part[src_idx * 2 + 1];
        }
        self.head_pos = start;
    }

    /// Produz `out.len()/2` quadros na saída de áudio avançando/retrocedendo a posição
    /// de timeline `pos` conforme `speed` (`0.25..=4.0` ou reverso para shuttle).
    /// Retorna `true` se o cursor atingiu o início (<= 0 em reverso) e deve pausar.
    pub fn process(
        &mut self,
        speed: f64,
        pos: &mut i64,
        frac: &mut f64,
        loop_range: Option<(i64, i64)>,
        out: &mut [f32],
        mut mix_fn: impl FnMut(i64, &mut [f32]),
    ) -> bool {
        let out_frames = out.len() / 2;
        if out_frames == 0 || !speed.is_finite() || speed.abs() < 1e-4 {
            out.fill(0.0);
            return false;
        }

        // Caminho direto em 1,0x sem latência adicional.
        if (speed - 1.0).abs() < 1e-6 {
            self.reset();
            let mut done = 0usize;
            while done < out_frames {
                let mut n = out_frames - done;
                if let Some((a, b)) = loop_range {
                    if *pos >= b && b > a {
                        *pos = a;
                    }
                    if *pos < b {
                        n = n.min((b - *pos).max(1) as usize);
                    }
                }
                mix_fn(*pos, &mut out[done * 2..(done + n) * 2]);
                *pos = pos.saturating_add(n as i64);
                done += n;
            }
            *frac = 0.0;
            return false;
        }

        let dir = if speed >= 0.0 { 1.0_f64 } else { -1.0_f64 };
        let mag = speed.abs();

        // Fora do intervalo seguro do Sinc resampler, apenas avança o relógio sem aliasing.
        if !(MIN_RUBATO_RATIO..=MAX_RUBATO_RATIO).contains(&(1.0 / mag)) || self.resampler.is_none()
        {
            out.fill(0.0);
            let adv = out_frames as f64 * speed + *frac;
            *pos = pos.saturating_add(adv.trunc() as i64);
            *frac = adv.fract();
            if *pos <= 0 && dir < 0.0 {
                *pos = 0;
                return true;
            }
            return false;
        }

        if self.active_pos.is_none() {
            self.head_pos = *pos;
            self.active_pos = Some(*pos);
        }

        let target_ratio = (1.0 / mag).clamp(MIN_RUBATO_RATIO, MAX_RUBATO_RATIO);
        let mut hit_start = false;

        while self.fifo.len() < out_frames * 2 {
            let in_needed = {
                let Some(res) = self.resampler.as_mut() else {
                    break;
                };
                if res.set_resample_ratio(target_ratio, true).is_err() {
                    break;
                }
                res.input_frames_next().max(1)
            };
            if dir >= 0.0 {
                self.pull_forward_chunk(in_needed, loop_range, &mut mix_fn);
            } else {
                self.pull_reverse_chunk(in_needed, &mut mix_fn);
                if self.head_pos <= 0 {
                    hit_start = true;
                }
            }
            self.in_buf.resize(in_needed * 2, 0.0);
            self.in_buf.copy_from_slice(&self.scratch[..in_needed * 2]);
            self.out_buf.resize(VARISPEED_CHUNK * 2, 0.0);

            let in_slice = InterleavedSlice::new(&self.in_buf, 2, in_needed);
            let out_slice = InterleavedSlice::new_mut(&mut self.out_buf, 2, VARISPEED_CHUNK);
            if let (Ok(inp), Ok(mut outp), Some(res)) =
                (in_slice, out_slice, self.resampler.as_mut())
            {
                if res.process_into_buffer(&inp, &mut outp, None).is_err() {
                    self.out_buf.fill(0.0);
                }
            } else {
                self.out_buf.fill(0.0);
            }
            self.fifo.extend(self.out_buf.iter().copied());
            if hit_start && self.fifo.len() >= out_frames * 2 {
                break;
            }
        }

        for i in 0..out_frames * 2 {
            out[i] = self.fifo.pop_front().unwrap_or(0.0);
        }

        let exact = *pos as f64 + out_frames as f64 * speed + *frac;
        *pos = exact.trunc() as i64;
        *frac = exact.fract();
        if let Some((a, b)) = loop_range {
            if dir > 0.0 && *pos >= b && b > a {
                *pos = a;
                self.head_pos = a;
                self.fifo.clear();
            }
        }
        if dir < 0.0 && *pos <= 0 {
            *pos = 0;
            *frac = 0.0;
            return true;
        }
        false
    }
}

struct EventResampler {
    src_sr: u32,
    ratio: f64,
    next_timeline_sample: i64,
    in_pos: i64,
    resampler: Async<f32>,
    fifo: VecDeque<f32>,
    in_buf: Vec<f32>,
    out_buf: Vec<f32>,
}

const EV_RESAMP_CHUNK: usize = 256;

impl EventResampler {
    fn new(src_sr: u32, ratio: f64) -> Option<Self> {
        let clamped = ratio.clamp(MIN_RUBATO_RATIO, MAX_RUBATO_RATIO);
        let resampler = Async::<f32>::new_sinc(
            clamped,
            2.0,
            &sinc_params(),
            EV_RESAMP_CHUNK,
            2,
            FixedAsync::Output,
        )
        .ok()?;
        Some(Self {
            src_sr: src_sr.max(1),
            ratio: clamped,
            next_timeline_sample: i64::MIN,
            in_pos: 0,
            resampler,
            fifo: VecDeque::with_capacity(EV_RESAMP_CHUNK * 4),
            in_buf: Vec::new(),
            out_buf: vec![0.0_f32; EV_RESAMP_CHUNK * 2],
        })
    }

    fn reset_at(
        &mut self,
        start_timeline: i64,
        ratio: f64,
        e: &Event,
        a: &AudioData,
        dur: Option<Ticks>,
        sr: u32,
    ) {
        self.resampler.reset();
        self.fifo.clear();
        self.ratio = ratio.clamp(MIN_RUBATO_RATIO, MAX_RUBATO_RATIO);
        let _ = self.resampler.set_resample_ratio(self.ratio, false);
        self.next_timeline_sample = start_timeline;

        let t0 = timeline_sample_time(start_timeline, sr);
        let src_frame0 = e
            .media_time(t0, dur)
            .map(|mt| mt.to_sample_pos(a.sample_rate).0)
            .unwrap_or(0);
        // Alinha o início descontando o atraso do filtro Sinc sem alterar a fase contínua.
        let delay_in = ((self.resampler.output_delay() as f64) / self.ratio).round() as i64;
        self.in_pos = src_frame0.saturating_sub(delay_in);
        let delay_out = self.resampler.output_delay();
        if delay_out > 0 {
            self.pump(delay_out, e, a);
            let drop_samples = (delay_out * 2).min(self.fifo.len());
            self.fifo.drain(..drop_samples);
        }
    }

    fn pump(&mut self, need_frames: usize, e: &Event, a: &AudioData) {
        let src_frames = a.frames();
        let dir: i64 = if e.reverse { -1 } else { 1 };
        while self.fifo.len() < need_frames * 2 {
            let in_n = self.resampler.input_frames_next().max(1);
            self.in_buf.resize(in_n * 2, 0.0);
            for k in 0..in_n {
                let idx = self.in_pos.saturating_add((k as i64).saturating_mul(dir));
                let valid = if src_frames == 0 {
                    None
                } else if e.looped {
                    Some(idx.rem_euclid(src_frames as i64) as usize)
                } else if idx >= 0 && (idx as usize) < src_frames {
                    Some(idx as usize)
                } else {
                    None
                };
                if let Some(f) = valid {
                    self.in_buf[k * 2] = a.data[f * 2];
                    self.in_buf[k * 2 + 1] = a.data[f * 2 + 1];
                } else {
                    self.in_buf[k * 2] = 0.0;
                    self.in_buf[k * 2 + 1] = 0.0;
                }
            }
            self.in_pos = self
                .in_pos
                .saturating_add((in_n as i64).saturating_mul(dir));
            self.out_buf.resize(EV_RESAMP_CHUNK * 2, 0.0);
            if let (Ok(inp), Ok(mut outp)) = (
                InterleavedSlice::new(&self.in_buf, 2, in_n),
                InterleavedSlice::new_mut(&mut self.out_buf, 2, EV_RESAMP_CHUNK),
            ) {
                if self
                    .resampler
                    .process_into_buffer(&inp, &mut outp, None)
                    .is_err()
                {
                    self.out_buf.fill(0.0);
                }
            } else {
                self.out_buf.fill(0.0);
            }
            self.fifo.extend(self.out_buf.iter().copied());
        }
    }
}

#[inline]
fn timeline_sample_time(idx: i64, sr: u32) -> Ticks {
    match Ticks::exact_step_for_sample_rate(sr) {
        Some(k) => Ticks(idx.saturating_mul(k)),
        None => Ticks::from_samples(idx, sr),
    }
}

const WSOLA_WIN: usize = 1536;
const WSOLA_HOP: usize = WSOLA_WIN / 2;
const WSOLA_SEARCH: isize = 192;

fn sample_audio_frac(a: &AudioData, fpos: f64, looped: bool) -> (f32, f32) {
    let frames = a.frames();
    if frames == 0 || !fpos.is_finite() {
        return (0.0, 0.0);
    }
    let len_f = frames as f64;
    let p = if looped {
        fpos.rem_euclid(len_f)
    } else if fpos < 0.0 || fpos >= len_f {
        return (0.0, 0.0);
    } else {
        fpos
    };
    let i0 = p.floor() as isize;
    let frac = (p - i0 as f64) as f32;
    let at = |idx: isize| -> (f32, f32) {
        let fi = if looped {
            idx.rem_euclid(frames as isize) as usize
        } else if idx < 0 || (idx as usize) >= frames {
            return (0.0, 0.0);
        } else {
            idx as usize
        };
        let l = a.data.get(fi * 2).copied().unwrap_or(0.0);
        let r = a.data.get(fi * 2 + 1).copied().unwrap_or(0.0);
        (l, r)
    };
    let (l0, r0) = at(i0);
    let (l1, r1) = at(i0 + 1);
    (l0 + (l1 - l0) * frac, r0 + (r1 - r0) * frac)
}

/// Deslocador de tom / preservador de tom em escala temporal baseado em WSOLA
/// (*Waveform Similarity Overlap-Add*) de janela periódica de Hann em Sala Limpa.
struct WsolaShifter {
    src_sr: u32,
    ev_rate: f64,
    pitch_factor: f64,
    next_timeline_sample: i64,
    hop_timeline_pos: i64,
    ola_tail: Vec<f32>,
    prev_nat_next: Vec<f32>,
    has_prev: bool,
    fifo: VecDeque<f32>,
    hann: Vec<f32>,
    cand: Vec<f32>,
    nat: Vec<f32>,
}

impl WsolaShifter {
    fn new(src_sr: u32, ev_rate: f64, pitch_factor: f64) -> Self {
        let mut hann = Vec::with_capacity(WSOLA_WIN);
        let two_pi = std::f64::consts::TAU;
        for i in 0..WSOLA_WIN {
            let v = 0.5 * (1.0 - (two_pi * i as f64 / WSOLA_WIN as f64).cos());
            hann.push(v as f32);
        }
        Self {
            src_sr: src_sr.max(1),
            ev_rate,
            pitch_factor,
            next_timeline_sample: i64::MIN,
            hop_timeline_pos: 0,
            ola_tail: vec![0.0; WSOLA_HOP * 2],
            prev_nat_next: vec![0.0; WSOLA_HOP * 2],
            has_prev: false,
            fifo: VecDeque::with_capacity(WSOLA_WIN * 4),
            hann,
            cand: vec![0.0; WSOLA_WIN * 2],
            nat: vec![0.0; WSOLA_HOP * 2],
        }
    }

    fn reset_at(
        &mut self,
        start_timeline: i64,
        pitch_factor: f64,
        e: &Event,
        a: &AudioData,
        dur: Option<Ticks>,
        sr: u32,
    ) {
        self.src_sr = a.sample_rate.max(1);
        self.ev_rate = e.rate.as_f64();
        self.pitch_factor = pitch_factor;
        self.next_timeline_sample = start_timeline;
        self.hop_timeline_pos = start_timeline;
        self.ola_tail.fill(0.0);
        self.prev_nat_next.fill(0.0);
        self.has_prev = false;
        self.fifo.clear();

        // Semeia o meio-grão anterior ao cursor para que a amostra 0 já comece em ganho unitário.
        let dir = if e.reverse { -1.0 } else { 1.0 };
        let src0 = Self::source_frame_at(start_timeline, e, a, dur, sr);
        let p_step = self.pitch_factor * (self.src_sr as f64 / sr.max(1) as f64) * dir;
        let pre_start = src0 - (WSOLA_HOP as f64) * p_step;
        for i in 0..WSOLA_HOP {
            let w = self.hann[WSOLA_HOP + i];
            let (l, r) =
                sample_audio_frac(a, pre_start + ((WSOLA_HOP + i) as f64) * p_step, e.looped);
            self.ola_tail[i * 2] = l * w;
            self.ola_tail[i * 2 + 1] = r * w;
            let (nl, nr) =
                sample_audio_frac(a, pre_start + ((WSOLA_HOP + i) as f64) * p_step, e.looped);
            self.prev_nat_next[i * 2] = nl;
            self.prev_nat_next[i * 2 + 1] = nr;
        }
        self.has_prev = true;
    }

    fn source_frame_at(t_idx: i64, e: &Event, a: &AudioData, dur: Option<Ticks>, sr: u32) -> f64 {
        let t = timeline_sample_time(t_idx, sr);
        let rate_f = e.rate.as_f64();
        let off_frames =
            (e.offset.0 as f64 / crate::model::TICKS_PER_SEC as f64) * a.sample_rate.max(1) as f64;
        if let Some(mt) = e.media_time(t, dur) {
            let (idx, frac) = mt.to_sample_pos(a.sample_rate);
            return idx as f64 + f64::from(frac);
        }
        let dt_secs = (t - e.start).0 as f64 / crate::model::TICKS_PER_SEC as f64;
        if e.reverse {
            let len_secs = e.length.0 as f64 / crate::model::TICKS_PER_SEC as f64;
            off_frames + (len_secs - dt_secs) * rate_f * a.sample_rate.max(1) as f64
        } else {
            off_frames + dt_secs * rate_f * a.sample_rate.max(1) as f64
        }
    }

    fn pump(&mut self, need_frames: usize, e: &Event, a: &AudioData, dur: Option<Ticks>, sr: u32) {
        let dir = if e.reverse { -1.0 } else { 1.0 };
        let p_step = self.pitch_factor * (self.src_sr as f64 / sr.max(1) as f64) * dir;
        while self.fifo.len() < need_frames * 2 {
            let nom_src = Self::source_frame_at(self.hop_timeline_pos, e, a, dur, sr);
            let mut best_delta = 0isize;
            if self.has_prev {
                let mut best_score = f32::NEG_INFINITY;
                let mut prev_norm = 0.0_f32;
                for i in (0..WSOLA_HOP).step_by(2) {
                    let m = 0.5 * (self.prev_nat_next[i * 2] + self.prev_nat_next[i * 2 + 1]);
                    prev_norm += m * m;
                }
                if prev_norm > 1e-8 {
                    let score_candidate = |delta: isize| -> f32 {
                        let base = nom_src + (delta as f64) * p_step;
                        let mut dot = 0.0_f32;
                        let mut cand_norm = 0.0_f32;
                        for i in (0..WSOLA_HOP).step_by(2) {
                            let (l, r) = sample_audio_frac(a, base + (i as f64) * p_step, e.looped);
                            let cm = 0.5 * (l + r);
                            let pm =
                                0.5 * (self.prev_nat_next[i * 2] + self.prev_nat_next[i * 2 + 1]);
                            dot += cm * pm;
                            cand_norm += cm * cm;
                        }
                        dot / ((cand_norm * prev_norm).sqrt() + 1e-6)
                    };
                    for delta in (-WSOLA_SEARCH..=WSOLA_SEARCH).step_by(2) {
                        let score = score_candidate(delta);
                        if score > best_score {
                            best_score = score;
                            best_delta = delta;
                        }
                    }
                    // Refinamento fino +-2 amostras.
                    let center_d = best_delta;
                    for delta in (center_d - 2)..=(center_d + 2) {
                        if !(-WSOLA_SEARCH..=WSOLA_SEARCH).contains(&delta) {
                            continue;
                        }
                        let score = score_candidate(delta);
                        if score > best_score {
                            best_score = score;
                            best_delta = delta;
                        }
                    }
                }
            }
            let chosen_base = nom_src + (best_delta as f64) * p_step;
            for i in 0..WSOLA_WIN {
                let (l, r) = sample_audio_frac(a, chosen_base + (i as f64) * p_step, e.looped);
                self.cand[i * 2] = l;
                self.cand[i * 2 + 1] = r;
            }
            for i in 0..WSOLA_HOP {
                let w_in = self.hann[i];
                let out_l = self.ola_tail[i * 2] + self.cand[i * 2] * w_in;
                let out_r = self.ola_tail[i * 2 + 1] + self.cand[i * 2 + 1] * w_in;
                self.fifo.push_back(out_l);
                self.fifo.push_back(out_r);

                let w_out = self.hann[WSOLA_HOP + i];
                self.ola_tail[i * 2] = self.cand[(WSOLA_HOP + i) * 2] * w_out;
                self.ola_tail[i * 2 + 1] = self.cand[(WSOLA_HOP + i) * 2 + 1] * w_out;

                // Guarda continuação natural deste grão escolhida como referência para o próximo hop.
                let (nl, nr) =
                    sample_audio_frac(a, chosen_base + ((WSOLA_HOP + i) as f64) * p_step, e.looped);
                self.nat[i * 2] = nl;
                self.nat[i * 2 + 1] = nr;
            }
            self.prev_nat_next.copy_from_slice(&self.nat);
            self.has_prev = true;
            self.hop_timeline_pos = self.hop_timeline_pos.saturating_add(WSOLA_HOP as i64);
        }
    }
}

pub struct Mixer {
    pub sr: u32,
    states: HashMap<Id, FxState>,
    event_resamplers: HashMap<Id, EventResampler>,
    pitch_shifters: HashMap<Id, WsolaShifter>,
    pub meters: Meters,
    track_buf: Vec<f32>,
    ev_buf: Vec<f32>,
    pad_buf: Vec<f32>,
    bus_bufs: Vec<Vec<f32>>,
    master_buf: Vec<f32>,
    /// Master 5.1 intercalado (FL, FR, C, LFE, BL, BR), só no modo surround.
    master6: Vec<f32>,
    /// Linhas de atraso estéreo para compensação de latência entre ramos paralelos.
    delay_lines: HashMap<(u8, Id, u64), DelayLine>,
    /// Dono das instâncias VST3 deste mixer no host isolado.
    vst3_owner: u64,
    /// Render/análise offline: espera o plug-in VST3 abrir em vez de passar
    /// o sinal seco nos primeiros blocos.
    offline: bool,
    /// Ignora globalmente todos os efeitos de áudio (eventos, trilhas, barramentos e master).
    pub bypass_fx: bool,
    /// Eventos MIDI do bloco atual (evento ou trilha) para instrumentos VST3.
    pending_midi: Vec<crate::midi::Vst3MidiEvent>,
}

impl Drop for Mixer {
    fn drop(&mut self) {
        crate::plugin_host::vst3_release_owner(self.vst3_owner);
    }
}

/// Fades efetivos de um evento de áudio: o maior entre o fade definido e a
/// sobreposição com o evento vizinho (crossfade automático).
fn effective_fades(track: &Track, idx: usize) -> (Fade, Fade) {
    let e = &track.events[idx];
    let mut fi = e.fade_in.clone();
    let mut fo = e.fade_out.clone();
    for (j, o) in track.events.iter().enumerate() {
        if j == idx || o.mute {
            continue;
        }
        if o.start < e.start && o.end() > e.start && o.end() < e.end() {
            let ov = o.end() - e.start;
            if ov > fi.len {
                fi.len = ov;
            }
        }
        if o.start > e.start && o.start < e.end() && o.end() > e.end() {
            let ov = e.end() - o.start;
            if ov > fo.len {
                fo.len = ov;
            }
        }
    }
    (fi, fo)
}

#[inline]
fn fade_gain(e: &Event, fi: &Fade, fo: &Fade, t: Ticks) -> f32 {
    let local = t - e.start;
    let mut g = 1.0;
    if fi.len > Ticks::ZERO && local < fi.len {
        g *= fi.curve.shape(local.fraction_in(Ticks::ZERO, fi.len));
    }
    let rem = e.end() - t;
    if fo.len > Ticks::ZERO && rem < fo.len {
        g *= fo.curve.shape(rem.fraction_in(Ticks::ZERO, fo.len));
    }
    g
}

#[allow(clippy::too_many_arguments)]
fn apply_volume_pan_envelopes(
    buf: &mut [f32],
    n: usize,
    sr: u32,
    start: i64,
    base_vol_db: f32,
    base_pan: f32,
    venv: Option<&Envelope>,
    penv: Option<&Envelope>,
) -> [f32; 2] {
    let vol = db_to_lin(base_vol_db);
    let mut peak = [0f32; 2];
    for i in 0..n {
        let t = timeline_sample_time(start.saturating_add(i as i64), sr);
        let ve = venv.map(|e| e.eval(t)).unwrap_or(1.0);
        let pan = (base_pan + penv.map(|e| e.eval(t)).unwrap_or(0.0)).clamp(-1.0, 1.0);
        let gl = vol * ve * (1.0 - pan).min(1.0);
        let gr = vol * ve * (1.0 + pan).min(1.0);
        let l = buf[i * 2] * gl;
        let r = buf[i * 2 + 1] * gr;
        buf[i * 2] = l;
        buf[i * 2 + 1] = r;
        peak[0] = peak[0].max(l.abs());
        peak[1] = peak[1].max(r.abs());
    }
    peak
}

fn chain_latency_with(fxs: &[FxInstance], lookup: &mut impl FnMut(&FxInstance) -> usize) -> usize {
    let mut sum = 0usize;
    for fx in fxs {
        if fx.enabled && fx.kind == FxKind::Vst3 {
            sum = sum.saturating_add(lookup(fx));
        }
    }
    sum.min(MAX_PDC_FRAMES)
}

struct PdcPlan {
    ev_lat: HashMap<Id, usize>,
    track_ev_lat: Vec<usize>,
    track_lat: Vec<usize>,
    bus_in_lat: Vec<usize>,
    bus_out_lat: Vec<usize>,
    master_in_lat: usize,
    total_latency: usize,
}

fn find_bus_index(buses: &[AudioBus], limit: usize, bus_id: Id) -> Option<usize> {
    buses[..limit].iter().position(|b| b.id == bus_id)
}

impl Mixer {
    pub fn new(sr: u32) -> Self {
        Mixer {
            sr: sr.max(8000),
            states: HashMap::new(),
            event_resamplers: HashMap::new(),
            pitch_shifters: HashMap::new(),
            meters: Meters::default(),
            track_buf: vec![],
            ev_buf: vec![],
            pad_buf: vec![],
            bus_bufs: vec![],
            master_buf: vec![],
            master6: vec![],
            delay_lines: HashMap::new(),
            vst3_owner: crate::plugin_host::new_vst3_owner(),
            offline: false,
            bypass_fx: false,
            pending_midi: Vec::new(),
        }
    }

    /// Mixer para render e análise offline: os efeitos VST3 são abertos de
    /// forma síncrona, então nenhum bloco sai sem o efeito.
    pub fn new_offline(sr: u32) -> Self {
        let mut m = Mixer::new(sr);
        m.offline = true;
        m
    }

    /// Zera o estado dos efeitos, linhas de atraso e reamostradores (após salto de posição).
    pub fn reset(&mut self) {
        self.states.clear();
        self.event_resamplers.clear();
        self.pitch_shifters.clear();
        self.delay_lines.clear();
        self.pending_midi.clear();
        crate::plugin_host::vst3_reset_owner(self.vst3_owner);
    }

    /// Latência total do grafo de áudio (em amostras na taxa deste mixer).
    /// No modo offline, garante a abertura dos efeitos VST3 ativos e reinicia
    /// o estado antes de iniciar o pré-roll de compensação.
    pub fn total_latency(&mut self, p: &Project) -> usize {
        let host = crate::plugin_host::default_host_exe();
        let owner = self.vst3_owner;
        let sr = self.sr;
        let offline = self.offline;
        let mut cache = HashMap::<Id, usize>::new();
        let mut lookup = |fx: &FxInstance| -> usize {
            if let Some(&lat) = cache.get(&fx.id) {
                return lat;
            }
            let lat = match (fx.enabled, fx.kind, fx.vst3.as_ref(), host.as_deref()) {
                (true, FxKind::Vst3, Some(r), Some(h)) => {
                    crate::plugin_host::vst3_cached_latency(h, owner, fx.id, r, sr, offline)
                        as usize
                }
                _ => 0,
            };
            cache.insert(fx.id, lat);
            lat
        };
        Self::compute_pdc_plan(p, &mut lookup).total_latency
    }

    fn compute_pdc_plan(p: &Project, lookup: &mut impl FnMut(&FxInstance) -> usize) -> PdcPlan {
        let num_buses = p.audio_buses.len().min(MAX_AUDIO_BUSES);
        let any_solo = p.any_solo(TrackKind::Audio);
        let mut ev_lat = HashMap::new();
        let mut track_ev_lat = vec![0usize; p.tracks.len()];
        let mut track_lat = vec![0usize; p.tracks.len()];
        let mut bus_in_lat = vec![0usize; num_buses];
        let mut bus_out_lat = vec![0usize; num_buses];
        let mut bus_down = vec![false; num_buses];

        for (ti, track) in p.tracks.iter().enumerate() {
            if track.kind != TrackKind::Audio || !track.is_audible(any_solo) {
                continue;
            }
            let mut max_ev = 0usize;
            for e in &track.events {
                if e.mute {
                    continue;
                }
                let el = chain_latency_with(&e.fx, lookup);
                ev_lat.insert(e.id, el);
                max_ev = max_ev.max(el);
            }
            track_ev_lat[ti] = max_ev;
            let t_lat = max_ev
                .saturating_add(chain_latency_with(&track.fx, lookup))
                .min(MAX_PDC_FRAMES);
            track_lat[ti] = t_lat;

            if let Some(dst_bi) = track
                .output_bus
                .and_then(|bid| find_bus_index(&p.audio_buses, num_buses, bid))
            {
                bus_in_lat[dst_bi] = bus_in_lat[dst_bi].max(t_lat);
            }
            for send in &track.sends {
                if send.mute {
                    continue;
                }
                if let Some(bi) = find_bus_index(&p.audio_buses, num_buses, send.bus_id) {
                    bus_in_lat[bi] = bus_in_lat[bi].max(t_lat);
                }
            }
        }

        let topo = p.topological_bus_order();
        for pos_idx in 0..topo.len() {
            let bi = topo[pos_idx];
            let Some(bus) = p.audio_buses.get(bi) else {
                continue;
            };
            let own = chain_latency_with(&bus.fx, lookup);
            let out_l = bus_in_lat[bi].saturating_add(own).min(MAX_PDC_FRAMES);
            bus_out_lat[bi] = out_l;
            if let Some(dst_id) = bus.output_bus {
                if let Some(dst_bi) = find_bus_index(&p.audio_buses, num_buses, dst_id) {
                    let dst_is_later = topo[pos_idx + 1..].contains(&dst_bi);
                    if dst_bi != bi && dst_is_later {
                        bus_in_lat[dst_bi] = bus_in_lat[dst_bi].max(out_l);
                        bus_down[bi] = true;
                    }
                }
            }
        }

        let mut master_in_lat = 0usize;
        for (ti, track) in p.tracks.iter().enumerate() {
            if track.kind != TrackKind::Audio || !track.is_audible(any_solo) {
                continue;
            }
            let goes_to_bus = track
                .output_bus
                .and_then(|bid| find_bus_index(&p.audio_buses, num_buses, bid))
                .is_some();
            if !goes_to_bus {
                master_in_lat = master_in_lat.max(track_lat[ti]);
            }
        }
        for &bi in &topo {
            if !bus_down[bi] {
                master_in_lat = master_in_lat.max(bus_out_lat[bi]);
            }
        }
        let master_fx = chain_latency_with(&p.master.fx, lookup);
        let total_latency = master_in_lat.saturating_add(master_fx).min(MAX_PDC_FRAMES);
        PdcPlan {
            ev_lat,
            track_ev_lat,
            track_lat,
            bus_in_lat,
            bus_out_lat,
            master_in_lat,
            total_latency,
        }
    }

    fn apply_delay(&mut self, tag: u8, src: Id, dst: u64, delay_frames: usize, buf: &mut [f32]) {
        if delay_frames == 0 {
            return;
        }
        self.delay_lines
            .entry((tag, src, dst))
            .or_default()
            .process(delay_frames, buf);
    }

    /// Amostra os pontos de automação com precisão de amostra para todos os
    /// parâmetros VST3 com keyframes dentro do bloco `[t_start, t_start + dur)`.
    fn sample_vst3_automation(
        r: &Vst3FxRef,
        t_start: Ticks,
        sr: u32,
        frames: usize,
    ) -> Vec<crate::plugin_host::Vst3ParamPoint> {
        let mut pts = Vec::new();
        if frames == 0 || sr == 0 {
            return pts;
        }
        let block_ticks = timeline_sample_time(frames as i64, sr);
        let t_end = Ticks::from_raw(t_start.0.saturating_add(block_ticks.0));
        let max_off = frames.saturating_sub(1).min(i32::MAX as usize) as i32;
        const SUBDIV: usize = 64;
        for p in &r.params {
            if p.keys.is_empty() {
                continue;
            }
            let v0 = p.value_at(t_start);
            let v1 = p.value_at(t_end);
            pts.push(crate::plugin_host::Vst3ParamPoint {
                id: p.id,
                sample_offset: 0,
                value: v0,
            });
            // Adiciona pontos intermediários a cada 64 amostras em rampas ativas
            // mais os keyframes exatos que caem dentro do bloco.
            let mut has_interior_key = false;
            for &(kt, kv) in &p.keys {
                if kt > t_start && kt < t_end {
                    has_interior_key = true;
                    let rel = (kt - t_start).0;
                    let off = if block_ticks.0 > 0 {
                        ((rel as i128).saturating_mul(frames as i128) / (block_ticks.0 as i128))
                            as i32
                    } else {
                        0
                    };
                    pts.push(crate::plugin_host::Vst3ParamPoint {
                        id: p.id,
                        sample_offset: off.clamp(0, max_off),
                        value: kv.clamp(0.0, 1.0),
                    });
                }
            }
            if has_interior_key || (v1 - v0).abs() > 1e-5 {
                let mut f = SUBDIV;
                while f < frames {
                    let tf = Ticks::from_raw(
                        t_start
                            .0
                            .saturating_add(timeline_sample_time(f as i64, sr).0),
                    );
                    pts.push(crate::plugin_host::Vst3ParamPoint {
                        id: p.id,
                        sample_offset: (f as i32).clamp(0, max_off),
                        value: p.value_at(tf),
                    });
                    f = f.saturating_add(SUBDIV);
                }
                if max_off > 0 {
                    pts.push(crate::plugin_host::Vst3ParamPoint {
                        id: p.id,
                        sample_offset: max_off,
                        value: v1,
                    });
                }
            }
        }
        pts
    }

    /// Extrai eventos MIDI de um evento da timeline dentro do bloco `[t0, t1)`,
    /// respeitando `offset`, recorte e velocidade (`rate`).
    fn extract_event_midi(
        e: &Event,
        clip: &crate::midi::MidiClip,
        t0: Ticks,
        t1: Ticks,
        frames: usize,
        out: &mut Vec<crate::midi::Vst3MidiEvent>,
    ) {
        let rate = e.rate.as_f64();
        if frames == 0 || t1 <= t0 || !rate.is_finite() || rate <= 0.0 {
            return;
        }
        let span_tl = (t1 - t0).0;
        if span_tl <= 0 {
            return;
        }
        let win_a = t0.max(e.start);
        let win_b = t1.min(e.end());
        if win_b <= win_a {
            return;
        }
        let clip_a = Ticks::from_raw(
            e.offset
                .0
                .saturating_add((((win_a - e.start).0 as f64) * rate).round() as i64),
        );
        let clip_b = Ticks::from_raw(
            e.offset
                .0
                .saturating_add((((win_b - e.start).0 as f64) * rate).round() as i64),
        );
        let mut local = Vec::new();
        let sub_frames = (((win_b - win_a).0 as i128).saturating_mul(frames as i128)
            / (span_tl as i128))
            .clamp(1, frames as i128) as usize;
        clip.events_in_window(clip_a, clip_b, sub_frames, &mut local);
        let base_off = (((win_a - t0).0 as i128).saturating_mul(frames as i128) / (span_tl as i128))
            .clamp(0, frames.saturating_sub(1) as i128) as i32;
        let max_off = frames.saturating_sub(1).min(i32::MAX as usize) as i32;
        for mut ev in local {
            ev.sample_offset = ev.sample_offset.saturating_add(base_off).clamp(0, max_off);
            out.push(ev);
        }
    }

    /// Efeito/instrumento VST3 pelo host isolado com automação em tempo real e MIDI.
    /// Em falha o bloco passa seco e o erro vai uma vez para o log (por efeito).
    fn run_vst3(&mut self, fx: &FxInstance, buf: &mut [f32], t: Ticks) {
        use std::sync::{Mutex, OnceLock, PoisonError};
        static LOGGED: OnceLock<Mutex<std::collections::HashSet<Id>>> = OnceLock::new();
        let (Some(r), true) = (fx.vst3.as_ref(), fx.enabled) else {
            return;
        };
        let Some(host) = crate::plugin_host::default_host_exe() else {
            return;
        };
        let frames = buf.len() / 2;
        let auto_pts = Self::sample_vst3_automation(r, t, self.sr, frames);
        let res = crate::plugin_host::vst3_process_ex(
            &host,
            self.vst3_owner,
            fx.id,
            r,
            self.sr,
            buf,
            &auto_pts,
            &self.pending_midi,
            self.offline,
        );
        if let Err(e) = res {
            let set = LOGGED.get_or_init(|| Mutex::new(Default::default()));
            if set
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(fx.id)
            {
                crate::util::log_error(&format!("efeito VST3 '{}' falhou: {e}", r.label));
            }
        }
    }

    fn run_fx(&mut self, fx: &FxInstance, buf: &mut [f32], t: Ticks) {
        if self.bypass_fx {
            return;
        }
        if fx.kind == FxKind::Vst3 {
            self.run_vst3(fx, buf, t);
            return;
        }
        let sr = self.sr;
        let st = self
            .states
            .entry(fx.id)
            .or_insert_with(|| FxState::new(fx.kind, sr));
        afx::process(fx, st, sr, buf, t);
    }

    fn sample_event_block(
        &mut self,
        e: &Event,
        media_dur: Option<Ticks>,
        a: &AudioData,
        start: i64,
        n: usize,
        buf: &mut [f32],
    ) {
        let sr = self.sr;
        let ev_rate = e.rate.as_f64();
        let pitch_factor = e.effective_pitch_factor();
        let tsm_ratio = if ev_rate > 0.0 {
            pitch_factor / ev_rate
        } else {
            1.0
        };
        if ev_rate > 0.0 && (tsm_ratio - 1.0).abs() > 1e-4 {
            let entry = match self.pitch_shifters.get_mut(&e.id) {
                Some(ws)
                    if ws.src_sr == a.sample_rate
                        && (ws.ev_rate - ev_rate).abs() < 1e-6
                        && (ws.pitch_factor - pitch_factor).abs() < 1e-6 =>
                {
                    ws
                }
                _ => {
                    self.pitch_shifters.insert(
                        e.id,
                        WsolaShifter::new(a.sample_rate, ev_rate, pitch_factor),
                    );
                    let Some(ws) = self.pitch_shifters.get_mut(&e.id) else {
                        buf.fill(0.0);
                        return;
                    };
                    ws
                }
            };
            if entry.next_timeline_sample != start {
                entry.reset_at(start, pitch_factor, e, a, media_dur, sr);
            }
            entry.pump(n, e, a, media_dur, sr);
            for i in 0..n {
                let l = entry.fifo.pop_front().unwrap_or(0.0);
                let r = entry.fifo.pop_front().unwrap_or(0.0);
                let t = timeline_sample_time(start.saturating_add(i as i64), sr);
                if e.contains(t) && e.media_time(t, media_dur).is_some() {
                    buf[i * 2] = l;
                    buf[i * 2 + 1] = r;
                } else {
                    buf[i * 2] = 0.0;
                    buf[i * 2 + 1] = 0.0;
                }
            }
            entry.next_timeline_sample = start.saturating_add(n as i64);
            return;
        }
        let needs_resamp = (a.sample_rate != sr || !e.rate.is_normal()) && ev_rate > 0.0;
        let ratio = if ev_rate > 0.0 {
            (sr as f64) / (a.sample_rate.max(1) as f64 * ev_rate)
        } else {
            1.0
        };
        if needs_resamp && (MIN_RUBATO_RATIO..=MAX_RUBATO_RATIO).contains(&ratio) {
            let entry = match self.event_resamplers.get_mut(&e.id) {
                Some(er) if er.src_sr == a.sample_rate && (er.ratio - ratio).abs() < 1e-6 => {
                    Some(er)
                }
                _ => {
                    if let Some(fresh) = EventResampler::new(a.sample_rate, ratio) {
                        self.event_resamplers.insert(e.id, fresh);
                    }
                    self.event_resamplers.get_mut(&e.id)
                }
            };
            if let Some(er) = entry {
                if er.next_timeline_sample != start {
                    er.reset_at(start, ratio, e, a, media_dur, sr);
                }
                er.pump(n, e, a);
                for i in 0..n {
                    let l = er.fifo.pop_front().unwrap_or(0.0);
                    let r = er.fifo.pop_front().unwrap_or(0.0);
                    let t = timeline_sample_time(start.saturating_add(i as i64), sr);
                    if e.contains(t) && e.media_time(t, media_dur).is_some() {
                        buf[i * 2] = l;
                        buf[i * 2 + 1] = r;
                    } else {
                        buf[i * 2] = 0.0;
                        buf[i * 2 + 1] = 0.0;
                    }
                }
                er.next_timeline_sample = start.saturating_add(n as i64);
                return;
            }
        }
        for i in 0..n {
            let t = timeline_sample_time(start.saturating_add(i as i64), sr);
            if !e.contains(t) {
                buf[i * 2] = 0.0;
                buf[i * 2 + 1] = 0.0;
                continue;
            }
            let Some(mt) = e.media_time(t, media_dur) else {
                buf[i * 2] = 0.0;
                buf[i * 2 + 1] = 0.0;
                continue;
            };
            let (l, r) = a.sample(mt);
            buf[i * 2] = l;
            buf[i * 2 + 1] = r;
        }
    }

    /// Mixa `out.len()/2` quadros estéreo a partir da amostra `start` (na taxa
    /// `sr`). Em projeto 5.1, devolve o downmix ITU-R BS.775 (monitoração).
    pub fn process(
        &mut self,
        p: &Project,
        audio: &HashMap<Id, Arc<AudioData>>,
        start: i64,
        out: &mut [f32],
    ) {
        let n = out.len() / 2;
        out.fill(0.0);
        if n == 0 {
            return;
        }
        let is_surround = self.mix_block(p, audio, start, n);
        let mv = master_gain(p);
        let mut peak = [0f32; 2];
        for i in 0..n {
            let (l, r) = if is_surround {
                self.master6
                    .get(i * surround::CHANNELS..(i + 1) * surround::CHANNELS)
                    .map(surround::downmix_frame)
                    .unwrap_or((0.0, 0.0))
            } else {
                (
                    self.master_buf.get(i * 2).copied().unwrap_or(0.0),
                    self.master_buf.get(i * 2 + 1).copied().unwrap_or(0.0),
                )
            };
            let (l, r) = (l * mv, r * mv);
            peak[0] = peak[0].max(l.abs());
            peak[1] = peak[1].max(r.abs());
            out[i * 2] = l;
            out[i * 2 + 1] = r;
        }
        self.meters.master = peak;
    }

    /// Mixa no layout de canais do projeto: `out` tem `n * channel_layout.channels()`
    /// amostras intercaladas (5.1: FL, FR, C, LFE, BL, BR). Usado pelo render.
    pub fn process_multichannel(
        &mut self,
        p: &Project,
        audio: &HashMap<Id, Arc<AudioData>>,
        start: i64,
        out: &mut [f32],
    ) {
        if p.settings.channel_layout != ChannelLayout::Surround51 {
            self.process(p, audio, start, out);
            return;
        }
        let ch = surround::CHANNELS;
        let n = out.len() / ch;
        out.fill(0.0);
        if n == 0 {
            return;
        }
        self.mix_block(p, audio, start, n);
        let mv = master_gain(p);
        let mut peak = [0f32; 2];
        for (o, s) in out[..n * ch].iter_mut().zip(self.master6.iter()) {
            *o = s * mv;
        }
        for f in out[..n * ch].chunks_exact(ch) {
            peak[0] = peak[0].max(f[surround::FL].abs().max(f[surround::SL].abs()));
            peak[1] = peak[1].max(f[surround::FR].abs().max(f[surround::SR].abs()));
        }
        self.meters.master = peak;
    }

    /// Mixa `n` quadros em `master_buf` (estéreo) ou `master6` (5.1), sem o
    /// volume do Master. Devolve verdadeiro no modo 5.1.
    fn mix_block(
        &mut self,
        p: &Project,
        audio: &HashMap<Id, Arc<AudioData>>,
        start: i64,
        n: usize,
    ) -> bool {
        let host = crate::plugin_host::default_host_exe();
        let owner = self.vst3_owner;
        let sr = self.sr;
        let offline = self.offline;
        let mut cache = HashMap::<Id, usize>::new();
        let mut lat_lookup = |fx: &FxInstance| -> usize {
            if let Some(&lat) = cache.get(&fx.id) {
                return lat;
            }
            let lat = match (fx.enabled, fx.kind, fx.vst3.as_ref(), host.as_deref()) {
                (true, FxKind::Vst3, Some(r), Some(h)) => {
                    crate::plugin_host::vst3_cached_latency(h, owner, fx.id, r, sr, offline)
                        as usize
                }
                _ => 0,
            };
            cache.insert(fx.id, lat);
            lat
        };
        let pdc = Self::compute_pdc_plan(p, &mut lat_lookup);
        self.mix_block_inner(p, audio, start, n, &pdc, &mut |this, fx, buf, t| {
            this.run_fx(fx, buf, t);
        })
    }

    /// Núcleo da mixagem parametrizado pelo executor de FX e plano PDC (permite
    /// testes determinísticos de alinhamento multi-trilha/barramento).
    fn mix_block_inner(
        &mut self,
        p: &Project,
        audio: &HashMap<Id, Arc<AudioData>>,
        start: i64,
        n: usize,
        pdc: &PdcPlan,
        exec_fx: &mut impl FnMut(&mut Self, &FxInstance, &mut [f32], Ticks),
    ) -> bool {
        let sr = self.sr;
        let t0 = timeline_sample_time(start, sr);
        let t1 = timeline_sample_time(start.saturating_add(n as i64), sr);
        let is_surround = p.settings.channel_layout == ChannelLayout::Surround51;
        self.master_buf.resize(n * 2, 0.0);
        self.master_buf.fill(0.0);
        if is_surround {
            self.master6.resize(n * surround::CHANNELS, 0.0);
            self.master6.fill(0.0);
        }

        let num_buses = p.audio_buses.len().min(MAX_AUDIO_BUSES);
        if self.bus_bufs.len() < num_buses {
            self.bus_bufs.resize_with(num_buses, Vec::new);
        }
        for bi in 0..num_buses {
            self.bus_bufs[bi].resize(n * 2, 0.0);
            self.bus_bufs[bi].fill(0.0);
        }

        let any_solo = p.any_solo(TrackKind::Audio);
        let mut track_buf = std::mem::take(&mut self.track_buf);
        let mut ev_buf = std::mem::take(&mut self.ev_buf);
        let mut pad_buf = std::mem::take(&mut self.pad_buf);
        track_buf.resize(n * 2, 0.0);
        ev_buf.resize(n * 2, 0.0);
        pad_buf.resize(n * 2, 0.0);

        for (ti, track) in p.tracks.iter().enumerate() {
            if track.kind != TrackKind::Audio || track.is_folder {
                continue;
            }
            if p.is_track_effectively_muted(track.id) || (any_solo && !track.solo) {
                self.meters.tracks.insert(track.id, [0.0, 0.0]);
                continue;
            }
            let max_ev_lat = pdc.track_ev_lat.get(ti).copied().unwrap_or(0);
            let t_lat = pdc.track_lat.get(ti).copied().unwrap_or(0);
            let track_has_instrument = !self.bypass_fx
                && track.fx.iter().any(|fx| {
                    fx.enabled
                        && fx.kind == FxKind::Vst3
                        && fx.vst3.as_ref().is_some_and(|r| r.is_instrument)
                });
            let mut track_midi: Vec<crate::midi::Vst3MidiEvent> = Vec::new();
            track_buf.fill(0.0);
            for (ei, e) in track.events.iter().enumerate() {
                if e.mute || e.end() <= t0 || e.start >= t1 {
                    continue;
                }
                let Some(m) = p.media(e.media_id) else {
                    continue;
                };
                let Some(a) = audio.get(&e.media_id) else {
                    continue;
                };
                let dur = m.duration.or_else(|| Some(a.duration()));
                let (fi, fo) = effective_fades(track, ei);
                let g = db_to_lin(e.gain_db);
                let ev_has_instrument = !self.bypass_fx
                    && e.fx.iter().any(|fx| {
                        fx.enabled
                            && fx.kind == FxKind::Vst3
                            && fx.vst3.as_ref().is_some_and(|r| r.is_instrument)
                    });
                let mut ev_midi = Vec::new();
                if let Some(midi_clip) = a.midi.as_ref() {
                    Self::extract_event_midi(e, midi_clip, t0, t1, n, &mut ev_midi);
                }
                ev_buf.fill(0.0);
                // Quando há instrumento VST3 ativo para o clipe MIDI, deixa o
                // instrumento sintetizar em vez do áudio de prévia embutido.
                if a.midi.is_none() || (!ev_has_instrument && !track_has_instrument) {
                    self.sample_event_block(e, dur, a, start, n, &mut ev_buf[..n * 2]);
                    for i in 0..n {
                        let t = timeline_sample_time(start.saturating_add(i as i64), sr);
                        let k = if e.contains(t) {
                            let dt = t - e.start;
                            let ev_vol = e
                                .envelope(EnvKind::Volume)
                                .map(|env| env.eval(dt))
                                .unwrap_or(1.0);
                            g * fade_gain(e, &fi, &fo, t) * ev_vol
                        } else {
                            0.0
                        };
                        ev_buf[i * 2] *= k;
                        ev_buf[i * 2 + 1] *= k;
                    }
                }
                if ev_has_instrument {
                    self.pending_midi = ev_midi;
                } else {
                    if !ev_midi.is_empty() && track_has_instrument {
                        track_midi.extend(ev_midi);
                    }
                    self.pending_midi.clear();
                }
                for fx in &e.fx {
                    exec_fx(self, fx, &mut ev_buf[..n * 2], t0 - e.start);
                }
                self.pending_midi.clear();
                if max_ev_lat > 0 {
                    let ev_l = pdc.ev_lat.get(&e.id).copied().unwrap_or(0);
                    let pad = max_ev_lat.saturating_sub(ev_l);
                    self.apply_delay(1, e.id, 0, pad, &mut ev_buf[..n * 2]);
                }
                for i in 0..n * 2 {
                    track_buf[i] += ev_buf[i];
                }
            }
            if !track_midi.is_empty() {
                track_midi.sort_by_key(|ev| (ev.sample_offset, ev.is_note_on));
                self.pending_midi = track_midi;
            }
            for fx in &track.fx {
                exec_fx(self, fx, &mut track_buf[..n * 2], t0);
            }
            self.pending_midi.clear();

            // Envios auxiliares pré-fader (pre-fader sends) com alinhamento PDC
            for (si, send) in track.sends.iter().enumerate() {
                if send.mute || !send.pre_fader {
                    continue;
                }
                let Some(bi) = find_bus_index(&p.audio_buses, num_buses, send.bus_id) else {
                    continue;
                };
                let sg = db_to_lin(send.level_db);
                let target_lat = pdc.bus_in_lat.get(bi).copied().unwrap_or(0);
                let pad = target_lat.saturating_sub(t_lat);
                if pad > 0 {
                    pad_buf[..n * 2].copy_from_slice(&track_buf[..n * 2]);
                    self.apply_delay(2, track.id, (si as u64) * 2, pad, &mut pad_buf[..n * 2]);
                    let dst = &mut self.bus_bufs[bi];
                    for i in 0..n * 2 {
                        dst[i] += pad_buf[i] * sg;
                    }
                } else {
                    let dst = &mut self.bus_bufs[bi];
                    for i in 0..n * 2 {
                        dst[i] += track_buf[i] * sg;
                    }
                }
            }

            // Volume, envelope de volume e pan (em 5.1 o pan estéreo dá lugar
            // ao Surround Panner, aplicado na saída para o Master)
            let peak = apply_volume_pan_envelopes(
                &mut track_buf[..n * 2],
                n,
                sr,
                start,
                track.volume_db,
                if is_surround { 0.0 } else { track.pan },
                track.active_envelope(EnvKind::Volume),
                if is_surround {
                    None
                } else {
                    track.active_envelope(EnvKind::Pan)
                },
            );
            self.meters.tracks.insert(track.id, peak);

            // Envios auxiliares pós-fader (post-fader sends) com alinhamento PDC
            for (si, send) in track.sends.iter().enumerate() {
                if send.mute || send.pre_fader {
                    continue;
                }
                let Some(bi) = find_bus_index(&p.audio_buses, num_buses, send.bus_id) else {
                    continue;
                };
                let sg = db_to_lin(send.level_db);
                let target_lat = pdc.bus_in_lat.get(bi).copied().unwrap_or(0);
                let pad = target_lat.saturating_sub(t_lat);
                if pad > 0 {
                    pad_buf[..n * 2].copy_from_slice(&track_buf[..n * 2]);
                    self.apply_delay(2, track.id, (si as u64) * 2 + 1, pad, &mut pad_buf[..n * 2]);
                    let dst = &mut self.bus_bufs[bi];
                    for i in 0..n * 2 {
                        dst[i] += pad_buf[i] * sg;
                    }
                } else {
                    let dst = &mut self.bus_bufs[bi];
                    for i in 0..n * 2 {
                        dst[i] += track_buf[i] * sg;
                    }
                }
            }

            // Saída principal da trilha: barramento atribuído ou Master Bus
            if let Some(dst_bi) = track
                .output_bus
                .and_then(|bid| find_bus_index(&p.audio_buses, num_buses, bid))
            {
                let target_lat = pdc.bus_in_lat.get(dst_bi).copied().unwrap_or(0);
                let pad = target_lat.saturating_sub(t_lat);
                if pad > 0 {
                    self.apply_delay(3, track.id, dst_bi as u64, pad, &mut track_buf[..n * 2]);
                }
                let dst = &mut self.bus_bufs[dst_bi];
                for i in 0..n * 2 {
                    dst[i] += track_buf[i];
                }
            } else {
                let pad = pdc.master_in_lat.saturating_sub(t_lat);
                if pad > 0 {
                    self.apply_delay(3, track.id, u64::MAX, pad, &mut track_buf[..n * 2]);
                }
                if is_surround {
                    let g = surround::gains(track.surround);
                    surround::mix_into(&mut self.master6, &track_buf[..n * 2], n, &g);
                } else {
                    for i in 0..n * 2 {
                        self.master_buf[i] += track_buf[i];
                    }
                }
            }
        }

        // Processamento dos barramentos auxiliares em ordem topológica (acíclica)
        let any_bus_solo = p.any_bus_solo();
        let topo = p.topological_bus_order();
        for pos_idx in 0..topo.len() {
            let bi = topo[pos_idx];
            let Some(bus) = p.audio_buses.get(bi) else {
                continue;
            };
            let b_lat = pdc.bus_out_lat.get(bi).copied().unwrap_or(0);
            let mut bbuf = std::mem::take(&mut self.bus_bufs[bi]);
            if !bus.is_audible(any_bus_solo) {
                self.meters.buses.insert(bus.id, [0.0, 0.0]);
                self.bus_bufs[bi] = bbuf;
                continue;
            }
            for fx in &bus.fx {
                exec_fx(self, fx, &mut bbuf[..n * 2], t0);
            }
            let bpeak = apply_volume_pan_envelopes(
                &mut bbuf[..n * 2],
                n,
                sr,
                start,
                bus.volume_db,
                if is_surround { 0.0 } else { bus.pan },
                bus.active_envelope(EnvKind::Volume),
                if is_surround {
                    None
                } else {
                    bus.active_envelope(EnvKind::Pan)
                },
            );
            self.meters.buses.insert(bus.id, bpeak);

            let mut sent_to_downstream = false;
            if let Some(dst_id) = bus.output_bus {
                if let Some(dst_bi) = find_bus_index(&p.audio_buses, num_buses, dst_id) {
                    // Evita realimentação se o projeto não foi saneado previamente.
                    let dst_is_later = topo[pos_idx + 1..].contains(&dst_bi);
                    if dst_bi != bi && dst_is_later {
                        let target_lat = pdc.bus_in_lat.get(dst_bi).copied().unwrap_or(0);
                        let pad = target_lat.saturating_sub(b_lat);
                        if pad > 0 {
                            self.apply_delay(4, bus.id, dst_bi as u64, pad, &mut bbuf[..n * 2]);
                        }
                        let dst = &mut self.bus_bufs[dst_bi];
                        for i in 0..n * 2 {
                            dst[i] += bbuf[i];
                        }
                        sent_to_downstream = true;
                    }
                }
            }
            if !sent_to_downstream {
                let pad = pdc.master_in_lat.saturating_sub(b_lat);
                if pad > 0 {
                    self.apply_delay(4, bus.id, u64::MAX, pad, &mut bbuf[..n * 2]);
                }
                if is_surround {
                    let g = surround::gains(bus.surround);
                    surround::mix_into(&mut self.master6, &bbuf[..n * 2], n, &g);
                } else {
                    for i in 0..n * 2 {
                        self.master_buf[i] += bbuf[i];
                    }
                }
            }
            self.bus_bufs[bi] = bbuf;
        }

        // FX do Master: em 5.1 processam o par frontal FL/FR (os efeitos são
        // estéreo); C, LFE e surrounds passam sem efeito.
        let mut master = std::mem::take(&mut self.master_buf);
        if is_surround && !p.master.fx.is_empty() {
            for i in 0..n {
                master[i * 2] = self.master6[i * surround::CHANNELS + surround::FL];
                master[i * 2 + 1] = self.master6[i * surround::CHANNELS + surround::FR];
            }
        }
        for fx in &p.master.fx {
            exec_fx(self, fx, &mut master[..n * 2], t0);
        }
        if is_surround && !p.master.fx.is_empty() {
            for i in 0..n {
                self.master6[i * surround::CHANNELS + surround::FL] = master[i * 2];
                self.master6[i * surround::CHANNELS + surround::FR] = master[i * 2 + 1];
            }
        }
        self.master_buf = master;
        self.track_buf = track_buf;
        self.ev_buf = ev_buf;
        self.pad_buf = pad_buf;
        is_surround
    }
}

/// Renderiza fora de tempo real (offline) a cadeia `ev.fx` de um evento de áudio em
/// PCM estéreo intercalado na taxa do projeto (`p.settings.sample_rate`), para
/// *Apply Non-Real-Time Audio FX* (manual p. 940).
pub fn render_event_fx_stereo(
    p: &Project,
    media: &crate::media::MediaCache,
    ev: &Event,
) -> Option<Vec<f32>> {
    let audio_map = media.audio_map();
    if !audio_map.contains_key(&ev.media_id) {
        return None;
    }
    let sr = p.settings.sample_rate.clamp(8000, 192_000);
    let mut iso = Project {
        settings: p.settings.clone(),
        ..Default::default()
    };
    iso.settings.channel_layout = ChannelLayout::Stereo;
    if let Some(m) = p.media(ev.media_id) {
        iso.media.push(m.clone());
    }
    let mut iso_ev = ev.clone();
    iso_ev.start = Ticks::ZERO;
    iso_ev.gain_db = 0.0;
    iso_ev.fade_in = Fade::default();
    iso_ev.fade_out = Fade::default();
    iso_ev.mute = false;
    let mut tr = Track::new(TrackKind::Audio, "FXBounce");
    tr.events.push(iso_ev);
    iso.tracks.push(tr);

    let max_samples = (sr as usize).saturating_mul(600);
    let total = (ev.length.to_samples_round(sr).max(1) as usize).min(max_samples);
    let mut mixer = Mixer::new_offline(sr);
    let pdc = mixer.total_latency(&iso) as i64;
    let mut out = Vec::with_capacity(total.saturating_mul(2));
    let mut pos = 0usize;
    let chunk = 2048usize;
    while pos < total {
        let n = (total - pos).min(chunk);
        let mut blk = vec![0.0f32; n * 2];
        mixer.process(&iso, &audio_map, pos as i64 + pdc, &mut blk);
        out.extend_from_slice(&blk);
        pos += n;
    }
    Some(out)
}

fn master_gain(p: &Project) -> f32 {
    if p.master.mute {
        0.0
    } else {
        db_to_lin(p.master.volume_db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_audio_item(mid: Id, dur_secs: i64) -> MediaItem {
        MediaItem {
            id: mid,
            name: "tom".into(),
            source: MediaSource::Generated(GenSpec::SolidColor {
                color: [0.0, 0.0, 0.0, 1.0],
            }),
            duration: Some(Ticks::from_secs_i64(dur_secs)),
            has_video: false,
            has_audio: true,
            width: 0,
            height: 0,
            fps: None,
            info: "test".into(),
            proxy_path: None,
        }
    }

    #[test]
    fn master_mute_silencia_mixagem_e_zera_medidor() {
        let mut p = Project::new_default();
        let mid = new_id();
        let item = test_audio_item(mid, 2);
        let audio_track = p
            .tracks
            .iter_mut()
            .find(|t| t.kind == TrackKind::Audio)
            .expect("projeto padrão contém faixa de áudio");
        audio_track
            .events
            .push(Event::new(&item, Ticks::ZERO, Ticks::from_secs_i64(1)));
        p.media.push(item);
        let mut audio = HashMap::new();
        audio.insert(
            mid,
            Arc::new(AudioData::from_stereo(48000, vec![0.5; 48000 * 2])),
        );

        let mut mixer = Mixer::new(48000);
        let mut out = vec![0.0_f32; 256 * 2];
        mixer.process(&p, &audio, 0, &mut out);
        assert!(out.iter().any(|&v| v.abs() > 0.1));
        assert!(mixer.meters.master[0] > 0.1);

        p.master.mute = true;
        mixer.process(&p, &audio, 0, &mut out);
        assert!(out.iter().all(|&v| v == 0.0));
        assert_eq!(mixer.meters.master, [0.0, 0.0]);
    }

    fn surround_project(pan: SurroundPan) -> (Project, HashMap<Id, Arc<AudioData>>) {
        let mut p = Project::new_default();
        p.settings.channel_layout = ChannelLayout::Surround51;
        let mid = new_id();
        let item = test_audio_item(mid, 2);
        let t = p
            .tracks
            .iter_mut()
            .find(|t| t.kind == TrackKind::Audio)
            .expect("projeto padrão contém faixa de áudio");
        t.surround = pan;
        t.events
            .push(Event::new(&item, Ticks::ZERO, Ticks::from_secs_i64(1)));
        p.media.push(item);
        let mut audio = HashMap::new();
        audio.insert(
            mid,
            Arc::new(AudioData::from_stereo(48000, vec![0.5; 48000 * 2])),
        );
        (p, audio)
    }

    #[test]
    fn surround_front_center_goes_to_center_channel() {
        let (p, audio) = surround_project(SurroundPan::default());
        let mut mixer = Mixer::new(48000);
        let mut out = vec![0.0_f32; 64 * 6];
        mixer.process_multichannel(&p, &audio, 0, &mut out);
        let f = &out[32 * 6..33 * 6];
        assert!((f[surround::C] - 0.5).abs() < 1e-3, "{f:?}");
        assert!(f[surround::FL].abs() < 1e-3 && f[surround::SL].abs() < 1e-3);
        assert!(f[surround::LFE].abs() < 1e-6);
    }

    #[test]
    fn surround_rear_left_goes_to_left_surround() {
        let a = (-110.0f32).to_radians();
        let pan = SurroundPan {
            x: a.sin(),
            y: -a.cos(),
            ..SurroundPan::default()
        };
        let (p, audio) = surround_project(pan);
        let mut mixer = Mixer::new(48000);
        let mut out = vec![0.0_f32; 64 * 6];
        mixer.process_multichannel(&p, &audio, 0, &mut out);
        let f = &out[32 * 6..33 * 6];
        assert!((f[surround::SL] - 0.5).abs() < 1e-3, "{f:?}");
        assert!(f[surround::C].abs() < 1e-3);
    }

    #[test]
    fn surround_stereo_output_is_itu_downmix() {
        let (p, audio) = surround_project(SurroundPan::default());
        let mut mixer = Mixer::new(48000);
        let mut out = vec![0.0_f32; 64 * 2];
        mixer.process(&p, &audio, 0, &mut out);
        let k = std::f32::consts::FRAC_1_SQRT_2;
        assert!((out[64] - 0.5 * k).abs() < 1e-3, "{}", out[64]);
        assert!((out[65] - 0.5 * k).abs() < 1e-3);
    }

    #[test]
    fn multichannel_on_stereo_project_matches_process() {
        let (mut p, audio) = surround_project(SurroundPan::default());
        p.settings.channel_layout = ChannelLayout::Stereo;
        let mut a = vec![0.0_f32; 64 * 2];
        let mut b = vec![0.0_f32; 64 * 2];
        Mixer::new(48000).process(&p, &audio, 0, &mut a);
        Mixer::new(48000).process_multichannel(&p, &audio, 0, &mut b);
        assert_eq!(a, b);
    }

    #[test]
    fn barramentos_encadeados_pre_e_pos_fader_mixam_para_master() {
        let mut p = Project::new_default();
        let bus_a = p.add_audio_bus(Some("Bus A")).expect("bus A");
        let bus_b = p.add_audio_bus(Some("Bus B")).expect("bus B");
        if let Some(a) = p.audio_bus_mut(bus_a) {
            a.output_bus = Some(bus_b);
            a.volume_db = -6.0;
        }
        let mid = new_id();
        let item = test_audio_item(mid, 2);
        let ti = p
            .tracks
            .iter()
            .position(|t| t.kind == TrackKind::Audio)
            .expect("trilha de áudio");
        p.tracks[ti].output_bus = Some(bus_a);
        p.tracks[ti].sends.push(AudioSend {
            bus_id: bus_b,
            level_db: -12.0,
            pre_fader: true,
            mute: false,
        });
        p.tracks[ti]
            .events
            .push(Event::new(&item, Ticks::ZERO, Ticks::from_secs_i64(1)));
        p.media.push(item);

        let mut audio = HashMap::new();
        audio.insert(
            mid,
            Arc::new(AudioData::from_stereo(48000, vec![0.4; 48000 * 2])),
        );

        let mut mixer = Mixer::new(48000);
        let mut out = vec![0.0_f32; 256 * 2];
        mixer.process(&p, &audio, 0, &mut out);
        let m_a = mixer.meters.buses.get(&bus_a).copied().unwrap_or([0.0; 2]);
        let m_b = mixer.meters.buses.get(&bus_b).copied().unwrap_or([0.0; 2]);
        assert!(m_a[0] > 0.05);
        assert!(m_b[0] > m_a[0]);
        assert!(out.iter().any(|&v| v.abs() > 0.1));

        // Mutar Bus B silencia o caminho encaminhado por B -> Master
        p.audio_bus_mut(bus_b).expect("bus B").mute = true;
        mixer.process(&p, &audio, 0, &mut out);
        assert!(out.iter().all(|&v| v.abs() < 1e-6));
    }

    #[test]
    fn rubato_reamostra_clipe_44k1_e_playback_rate_variavel() {
        let mut p = Project::new_default();
        let mid = new_id();
        let item = test_audio_item(mid, 4);
        let mut sine = vec![0.0_f32; 44_100 * 2 * 2];
        for i in 0..44_100 * 2 {
            let v = 0.5 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44_100.0).sin();
            sine[i * 2] = v;
            sine[i * 2 + 1] = v;
        }
        let ti = p
            .tracks
            .iter()
            .position(|t| t.kind == TrackKind::Audio)
            .expect("trilha de áudio");
        let mut ev = Event::new(&item, Ticks::ZERO, Ticks::from_secs_i64(2));
        ev.rate = Rate::from_f64(1.5).expect("taxa 1.5x");
        p.tracks[ti].events.push(ev);
        p.media.push(item);

        let mut audio = HashMap::new();
        audio.insert(mid, Arc::new(AudioData::from_stereo(44_100, sine)));

        let mut mixer = Mixer::new(48_000);
        let mut varispeed = TransportVarispeed::new();
        let mut pos = 0_i64;
        let mut frac = 0.0_f64;
        for speed in [0.25_f64, 0.5, 1.0, 2.0, 4.0] {
            let mut out = vec![0.0_f32; 1024 * 2];
            varispeed.process(speed, &mut pos, &mut frac, None, &mut out, |s_pos, buf| {
                mixer.process(&p, &audio, s_pos, buf);
            });
            assert!(out.iter().all(|v| v.is_finite()));
            assert!(
                out.iter().any(|&v| v.abs() > 0.01),
                "playback rate {speed} deve produzir áudio reamostrado"
            );
        }

        // Shuttle reverso retrocede cursor e não entra em pânico antes de 0
        pos = 1200;
        let mut out_rev = vec![0.0_f32; 2048 * 2];
        let _stopped = varispeed.process(
            -1.5,
            &mut pos,
            &mut frac,
            None,
            &mut out_rev,
            |s_pos, buf| {
                mixer.process(&p, &audio, s_pos, buf);
            },
        );
        assert!(out_rev.iter().all(|v| v.is_finite()));
        assert!(pos >= 0);
    }

    #[test]
    fn barramento_aplica_envelope_de_volume_e_pan() {
        let mut p = Project::new_default();
        let bus_id = p.add_audio_bus(Some("Bus A")).expect("bus A");
        let mid = new_id();
        let item = test_audio_item(mid, 2);
        let ti = p
            .tracks
            .iter()
            .position(|t| t.kind == TrackKind::Audio)
            .expect("trilha de áudio");
        p.tracks[ti].output_bus = Some(bus_id);
        p.tracks[ti]
            .events
            .push(Event::new(&item, Ticks::ZERO, Ticks::from_secs_i64(2)));
        p.media.push(item);
        let mut audio = HashMap::new();
        audio.insert(
            mid,
            Arc::new(AudioData::from_stereo(48_000, vec![0.5; 48_000 * 2])),
        );

        let mut env_vol = Envelope::new(EnvKind::Volume);
        env_vol.points = vec![EnvPoint {
            t: Ticks::ZERO,
            v: 0.25,
            curve: Curve::Linear,
            smoothness: 0.0,
        }];
        let mut env_pan = Envelope::new(EnvKind::Pan);
        env_pan.points = vec![EnvPoint {
            t: Ticks::ZERO,
            v: -1.0,
            curve: Curve::Linear,
            smoothness: 0.0,
        }];
        let bus = p.audio_bus_mut(bus_id).expect("bus A");
        bus.envelopes.push(env_vol);
        bus.envelopes.push(env_pan);

        let mut mixer = Mixer::new(48_000);
        let mut out = vec![0.0_f32; 256 * 2];
        mixer.process(&p, &audio, 0, &mut out);
        let m = mixer.meters.buses.get(&bus_id).copied().unwrap_or([0.0; 2]);
        assert!(
            m[0] > 0.05 && m[0] < 0.20,
            "envelope de volume do barramento deve atenuar L: {}",
            m[0]
        );
        assert!(
            m[1] < 1e-5,
            "envelope de pan em -1.0 deve silenciar canal R do barramento: {}",
            m[1]
        );
    }

    #[test]
    fn delay_line_and_pdc_align_parallel_tracks_and_cap_hostile_latency() {
        let mut p = Project::new_default();
        let mid_impulse = new_id();
        let item = test_audio_item(mid_impulse, 1);
        let t1_idx = p
            .tracks
            .iter()
            .position(|t| t.kind == TrackKind::Audio)
            .expect("trilha 1");
        let t2 = Track::new(TrackKind::Audio, "Trilha 2");
        p.tracks.push(t2);
        let t2_idx = p.tracks.len() - 1;

        let mut fx_delayed = FxInstance::new_vst3("fake.vst3", &"0".repeat(32), "Delayed");
        fx_delayed.id = 9001;
        p.tracks[t1_idx].fx.push(fx_delayed);

        let bus_a_id = p.add_audio_bus(Some("Bus A")).expect("bus A");
        p.tracks[t1_idx].output_bus = Some(bus_a_id);
        let mut fx_bus = FxInstance::new_vst3("fake.vst3", &"0".repeat(32), "BusDelay");
        fx_bus.id = 9002;
        p.audio_bus_mut(bus_a_id).expect("bus").fx.push(fx_bus);

        p.tracks[t1_idx]
            .events
            .push(Event::new(&item, Ticks::ZERO, Ticks::from_secs_i64(1)));
        p.tracks[t2_idx]
            .events
            .push(Event::new(&item, Ticks::ZERO, Ticks::from_secs_i64(1)));
        p.media.push(item);

        // Impulso na amostra 0 (L = +1.0, R = +1.0)
        let mut pcm = vec![0.0_f32; 48_000 * 2];
        pcm[0] = 1.0;
        pcm[1] = 1.0;
        let mut audio = HashMap::new();
        audio.insert(mid_impulse, Arc::new(AudioData::from_stereo(48_000, pcm)));

        // Trilha 1 entra com +6 frames no FX, passa pelo Bus A (+4 frames) => total 10 frames;
        // Trilha 2 vai direto ao Master (0 frames), logo o PDC deve atrasá-la em 10 frames.
        let plan = Mixer::compute_pdc_plan(&p, &mut |fx| match fx.id {
            9001 => 6,
            9002 => 4,
            _ => 0,
        });
        assert_eq!(plan.master_in_lat, 10);
        assert_eq!(plan.total_latency, 10);

        let mut mixer = Mixer::new_offline(48_000);
        let mut sim_delays = HashMap::<Id, DelayLine>::new();
        mixer.mix_block_inner(&p, &audio, 0, 32, &plan, &mut |_, fx, buf, _| {
            let d = match fx.id {
                9001 => 6,
                9002 => 4,
                _ => 0,
            };
            if d > 0 {
                sim_delays.entry(fx.id).or_default().process(d, buf);
            }
        });
        // Ambas as trilhas somam no índice exato 10 (1.0 + 1.0 = 2.0) e zero antes!
        for i in 0..32 {
            let v = mixer.master_buf[i * 2];
            if i == 10 {
                assert!((v - 2.0).abs() < 1e-5, "esperado pico somado em i=10: {v}");
            } else {
                assert!(v.abs() < 1e-5, "desalinhamento em i={i}: {v}");
            }
        }

        // Latência hostil gigantesca é limitada sem OOM/pânico (§0.4)
        let hostile = Mixer::compute_pdc_plan(&p, &mut |_| usize::MAX / 2);
        assert_eq!(hostile.total_latency, MAX_PDC_FRAMES);
    }

    fn estimate_freq_hz(mono: &[f32], sr: u32) -> f64 {
        let mut crossings = 0usize;
        for w in mono.windows(2) {
            if w[0] <= 0.0 && w[1] > 0.0 {
                crossings += 1;
            }
        }
        crossings as f64 * (sr as f64) / (mono.len().max(1) as f64)
    }

    #[test]
    fn wsola_pitch_shift_e_time_stretch_preservam_tom_e_suportam_entrada_hostil() {
        let sr = 48_000u32;
        let frames = 48_000 * 2;
        let freq = 440.0_f64;
        let mut pcm = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let s = (std::f64::consts::TAU * freq * (i as f64) / (sr as f64)).sin() as f32 * 0.6;
            pcm.push(s);
            pcm.push(s);
        }
        let media_id = new_id();
        let item = MediaItem {
            id: media_id,
            name: "sine440.wav".into(),
            source: MediaSource::File {
                path: "sine440.wav".into(),
            },
            duration: Some(Ticks::from_secs_i64(2)),
            has_video: false,
            has_audio: true,
            width: 0,
            height: 0,
            fps: None,
            info: String::new(),
            proxy_path: None,
        };
        let mut audio = HashMap::new();
        audio.insert(media_id, Arc::new(AudioData::from_stereo(sr, pcm)));

        // Caso 1: +12 semitons sem mudar duração -> dobra para ~880 Hz.
        let mut p = Project::new_default();
        p.settings.sample_rate = sr;
        p.media.push(item.clone());
        let mut ev = Event::new(&item, Ticks::ZERO, Ticks::from_secs_i64(1));
        ev.pitch_semitones = 12.0;
        p.tracks[1].events.push(ev);
        let mut mixer = Mixer::new_offline(sr);
        let mut out = vec![0.0_f32; sr as usize * 2];
        mixer.process(&p, &audio, 0, &mut out);
        let mono_up: Vec<f32> = out
            .as_chunks::<2>()
            .0
            .iter()
            .skip(4096)
            .map(|c| c[0])
            .collect();
        let f_up = estimate_freq_hz(&mono_up, sr);
        assert!(
            (f_up - 880.0).abs() < 35.0,
            "esperado ~880 Hz, obtido {f_up}"
        );

        // Caso 2: -12 semitons -> cai para ~220 Hz.
        p.tracks[1].events[0].pitch_semitones = -12.0;
        mixer.reset();
        out.fill(0.0);
        mixer.process(&p, &audio, 0, &mut out);
        let mono_dn: Vec<f32> = out
            .as_chunks::<2>()
            .0
            .iter()
            .skip(4096)
            .map(|c| c[0])
            .collect();
        let f_dn = estimate_freq_hz(&mono_dn, sr);
        assert!(
            (f_dn - 220.0).abs() < 20.0,
            "esperado ~220 Hz, obtido {f_dn}"
        );

        // Caso 3: stretch 0.5x (50%) com lock_pitch_to_stretch preserva ~440 Hz.
        p.tracks[1].events[0].pitch_semitones = 0.0;
        p.tracks[1].events[0].lock_pitch_to_stretch = true;
        p.tracks[1].events[0].rate = Rate::from_ratio(1, 2).unwrap();
        p.tracks[1].events[0].length = Ticks::from_secs_i64(2);
        mixer.reset();
        out.fill(0.0);
        mixer.process(&p, &audio, 0, &mut out);
        let mono_lock: Vec<f32> = out
            .as_chunks::<2>()
            .0
            .iter()
            .skip(4096)
            .map(|c| c[0])
            .collect();
        let f_lock = estimate_freq_hz(&mono_lock, sr);
        assert!(
            (f_lock - 440.0).abs() < 25.0,
            "esperado ~440 Hz preservado, obtido {f_lock}"
        );

        // Caso 4: valores hostis (NaN / infinito) não causam NaN nem pânico (§0.4).
        p.tracks[1].events[0].pitch_semitones = f32::NAN;
        mixer.reset();
        mixer.process(&p, &audio, 0, &mut out);
        assert!(out.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn vst3_automacao_por_amostra_e_extracao_midi_de_evento() {
        let mut r = Vst3FxRef {
            binary: "synth.vst3".into(),
            class_id: "0".repeat(32),
            label: "Synth".into(),
            params: vec![
                Vst3ParamValue {
                    id: 7,
                    value: 0.0,
                    keys: vec![(Ticks::ZERO, 0.0), (Ticks::from_secs_i64(1), 1.0)],
                },
                Vst3ParamValue {
                    id: 8,
                    value: 0.5,
                    keys: vec![],
                },
            ],
            state: None,
            is_instrument: true,
        };
        r.sanitize();
        let pts = Mixer::sample_vst3_automation(&r, Ticks::ZERO, 48_000, 256);
        // Só o parâmetro 7 tem keyframes e deve gerar pontos intermediários na rampa
        assert!(pts.iter().all(|p| p.id == 7));
        assert!(pts.len() >= 4, "{pts:?}");
        assert!((pts[0].value - 0.0).abs() < 1e-6);
        assert!(pts.last().unwrap().value > pts[0].value);

        // Extração de notas MIDI com deslocamento de evento
        let clip = crate::midi::MidiClip {
            tpqn: 480,
            notes: vec![crate::midi::MidiNote {
                start: Ticks::from_millis_i64(100),
                duration: Ticks::from_millis_i64(200),
                channel: 0,
                pitch: 69,
                velocity: 100,
            }],
            duration: Ticks::from_secs_i64(1),
        };
        let media = MediaItem {
            id: new_id(),
            name: "melodia.mid".into(),
            source: MediaSource::File {
                path: "melodia.mid".into(),
            },
            duration: Some(clip.duration),
            has_video: false,
            has_audio: true,
            width: 0,
            height: 0,
            fps: None,
            info: String::new(),
            proxy_path: None,
        };
        let ev = Event::new(&media, Ticks::from_secs_i64(1), Ticks::from_secs_i64(1));
        let mut out_evs = Vec::new();
        Mixer::extract_event_midi(
            &ev,
            &clip,
            Ticks::from_secs_i64(1),
            Ticks::from_millis_i64(1500),
            500,
            &mut out_evs,
        );
        assert_eq!(out_evs.len(), 2);
        assert!(out_evs[0].is_note_on && out_evs[0].pitch == 69);
        assert!((out_evs[0].sample_offset - 100).abs() <= 1);
        assert!(!out_evs[1].is_note_on && out_evs[1].pitch == 69);
        assert!((out_evs[1].sample_offset - 300).abs() <= 1);
    }
}
