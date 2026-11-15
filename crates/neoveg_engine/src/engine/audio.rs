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

pub struct Mixer {
    pub sr: u32,
    states: HashMap<Id, FxState>,
    event_resamplers: HashMap<Id, EventResampler>,
    pub meters: Meters,
    track_buf: Vec<f32>,
    ev_buf: Vec<f32>,
    bus_bufs: Vec<Vec<f32>>,
    master_buf: Vec<f32>,
    /// Master 5.1 intercalado (FL, FR, C, LFE, BL, BR), só no modo surround.
    master6: Vec<f32>,
    /// Dono das instâncias VST3 deste mixer no host isolado.
    vst3_owner: u64,
    /// Render/análise offline: espera o plug-in VST3 abrir em vez de passar
    /// o sinal seco nos primeiros blocos.
    offline: bool,
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

fn find_bus_index(buses: &[AudioBus], limit: usize, bus_id: Id) -> Option<usize> {
    buses[..limit].iter().position(|b| b.id == bus_id)
}

impl Mixer {
    pub fn new(sr: u32) -> Self {
        Mixer {
            sr: sr.max(8000),
            states: HashMap::new(),
            event_resamplers: HashMap::new(),
            meters: Meters::default(),
            track_buf: vec![],
            ev_buf: vec![],
            bus_bufs: vec![],
            master_buf: vec![],
            master6: vec![],
            vst3_owner: crate::plugin_host::new_vst3_owner(),
            offline: false,
        }
    }

    /// Mixer para render e análise offline: os efeitos VST3 são abertos de
    /// forma síncrona, então nenhum bloco sai sem o efeito.
    pub fn new_offline(sr: u32) -> Self {
        let mut m = Mixer::new(sr);
        m.offline = true;
        m
    }

    /// Zera o estado dos efeitos e reamostradores (após salto de posição).
    pub fn reset(&mut self) {
        self.states.clear();
        self.event_resamplers.clear();
        crate::plugin_host::vst3_reset_owner(self.vst3_owner);
    }

    /// Efeito VST3 pelo host isolado. Em falha o bloco passa seco e o erro
    /// vai uma vez para o log (por efeito), sem travar o áudio.
    fn run_vst3(&mut self, fx: &FxInstance, buf: &mut [f32]) {
        use std::sync::{Mutex, OnceLock, PoisonError};
        static LOGGED: OnceLock<Mutex<std::collections::HashSet<Id>>> = OnceLock::new();
        let (Some(r), true) = (fx.vst3.as_ref(), fx.enabled) else {
            return;
        };
        let Some(host) = crate::plugin_host::default_host_exe() else {
            return;
        };
        let res = crate::plugin_host::vst3_process(
            &host,
            self.vst3_owner,
            fx.id,
            r,
            self.sr,
            buf,
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
        if fx.kind == FxKind::Vst3 {
            self.run_vst3(fx, buf);
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
        track_buf.resize(n * 2, 0.0);
        ev_buf.resize(n * 2, 0.0);

        for track in &p.tracks {
            if track.kind != TrackKind::Audio {
                continue;
            }
            if !track.is_audible(any_solo) {
                self.meters.tracks.insert(track.id, [0.0, 0.0]);
                continue;
            }
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
                ev_buf.fill(0.0);
                self.sample_event_block(e, dur, a, start, n, &mut ev_buf[..n * 2]);
                for i in 0..n {
                    let t = timeline_sample_time(start.saturating_add(i as i64), sr);
                    let k = if e.contains(t) {
                        g * fade_gain(e, &fi, &fo, t)
                    } else {
                        0.0
                    };
                    ev_buf[i * 2] *= k;
                    ev_buf[i * 2 + 1] *= k;
                }
                for fx in &e.fx {
                    self.run_fx(fx, &mut ev_buf[..n * 2], t0 - e.start);
                }
                for i in 0..n * 2 {
                    track_buf[i] += ev_buf[i];
                }
            }
            for fx in &track.fx {
                self.run_fx(fx, &mut track_buf[..n * 2], t0);
            }

            // Envios auxiliares pré-fader (pre-fader sends)
            for send in &track.sends {
                if send.mute || !send.pre_fader {
                    continue;
                }
                let Some(bi) = find_bus_index(&p.audio_buses, num_buses, send.bus_id) else {
                    continue;
                };
                let sg = db_to_lin(send.level_db);
                let dst = &mut self.bus_bufs[bi];
                for i in 0..n * 2 {
                    dst[i] += track_buf[i] * sg;
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
                track.envelope(EnvKind::Volume),
                if is_surround {
                    None
                } else {
                    track.envelope(EnvKind::Pan)
                },
            );
            self.meters.tracks.insert(track.id, peak);

            // Envios auxiliares pós-fader (post-fader sends)
            for send in &track.sends {
                if send.mute || send.pre_fader {
                    continue;
                }
                let Some(bi) = find_bus_index(&p.audio_buses, num_buses, send.bus_id) else {
                    continue;
                };
                let sg = db_to_lin(send.level_db);
                let dst = &mut self.bus_bufs[bi];
                for i in 0..n * 2 {
                    dst[i] += track_buf[i] * sg;
                }
            }

            // Saída principal da trilha: barramento atribuído ou Master Bus
            if let Some(dst_bi) = track
                .output_bus
                .and_then(|bid| find_bus_index(&p.audio_buses, num_buses, bid))
            {
                let dst = &mut self.bus_bufs[dst_bi];
                for i in 0..n * 2 {
                    dst[i] += track_buf[i];
                }
            } else if is_surround {
                let g = surround::gains(track.surround);
                surround::mix_into(&mut self.master6, &track_buf[..n * 2], n, &g);
            } else {
                for i in 0..n * 2 {
                    self.master_buf[i] += track_buf[i];
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
            let mut bbuf = std::mem::take(&mut self.bus_bufs[bi]);
            if !bus.is_audible(any_bus_solo) {
                self.meters.buses.insert(bus.id, [0.0, 0.0]);
                self.bus_bufs[bi] = bbuf;
                continue;
            }
            for fx in &bus.fx {
                self.run_fx(fx, &mut bbuf[..n * 2], t0);
            }
            let bpeak = apply_volume_pan_envelopes(
                &mut bbuf[..n * 2],
                n,
                sr,
                start,
                bus.volume_db,
                if is_surround { 0.0 } else { bus.pan },
                bus.envelope(EnvKind::Volume),
                if is_surround {
                    None
                } else {
                    bus.envelope(EnvKind::Pan)
                },
            );
            self.meters.buses.insert(bus.id, bpeak);

            let mut sent_to_downstream = false;
            if let Some(dst_id) = bus.output_bus {
                if let Some(dst_bi) = find_bus_index(&p.audio_buses, num_buses, dst_id) {
                    // Evita realimentação se o projeto não foi saneado previamente.
                    let dst_is_later = topo[pos_idx + 1..].contains(&dst_bi);
                    if dst_bi != bi && dst_is_later {
                        let dst = &mut self.bus_bufs[dst_bi];
                        for i in 0..n * 2 {
                            dst[i] += bbuf[i];
                        }
                        sent_to_downstream = true;
                    }
                }
            }
            if !sent_to_downstream {
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
            self.run_fx(fx, &mut master[..n * 2], t0);
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
        is_surround
    }
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
        }];
        let mut env_pan = Envelope::new(EnvKind::Pan);
        env_pan.points = vec![EnvPoint {
            t: Ticks::ZERO,
            v: -1.0,
            curve: Curve::Linear,
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
}
