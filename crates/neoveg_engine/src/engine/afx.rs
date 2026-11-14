//! Efeitos de áudio (Audio FX) com estado por instância e grafos DSP via `fundsp`.

// Laços indexam vários buffers paralelos (canais/amostras) ao mesmo tempo;
// iteradores encadeados deixariam o DSP menos legível.
#![allow(clippy::needless_range_loop)]

use crate::model::{db_to_lin, FxInstance, FxKind, Ticks};
use fundsp::prelude32::{
    bell_hz, highpass_hz, highshelf_hz, limiter_stereo, lowpass_hz, lowshelf_hz, reverb_stereo,
    AudioUnit,
};

#[inline]
fn san(v: f32, fallback: f32, lo: f32, hi: f32) -> f32 {
    let f = if v.is_finite() { v } else { fallback };
    crate::util::clampf(f, lo, hi)
}

#[derive(Clone)]
pub struct FundspNodeState {
    sig: Vec<i32>,
    sr: u32,
    unit: Option<Box<dyn AudioUnit>>,
}

impl FundspNodeState {
    fn new(sr: u32) -> Self {
        Self {
            sig: Vec::new(),
            sr: sr.max(8000),
            unit: None,
        }
    }

    fn ensure(&mut self, sr: u32, key: &[i32], build: impl FnOnce() -> Box<dyn AudioUnit>) {
        let sr = sr.max(8000);
        if self.unit.is_none() || self.sr != sr || self.sig.as_slice() != key {
            let mut u = build();
            u.set_sample_rate(sr as f64);
            self.unit = Some(u);
            self.sr = sr;
            self.sig.clear();
            self.sig.extend_from_slice(key);
        }
    }
}

fn build_eq_unit(sr: u32, v: &[f32]) -> Box<dyn AudioUnit> {
    let max_f = (sr as f32 * 0.45).max(100.0);
    let ls_f = san(v.first().copied().unwrap_or(100.0), 100.0, 20.0, max_f);
    let ls_g = db_to_lin(san(v.get(1).copied().unwrap_or(0.0), 0.0, -24.0, 24.0)).clamp(0.05, 16.0);
    let pk_f = san(v.get(2).copied().unwrap_or(1000.0), 1000.0, 20.0, max_f);
    let pk_g = db_to_lin(san(v.get(3).copied().unwrap_or(0.0), 0.0, -24.0, 24.0)).clamp(0.05, 16.0);
    let pk_q = san(v.get(4).copied().unwrap_or(1.0), 1.0, 0.1, 20.0);
    let hs_f = san(v.get(5).copied().unwrap_or(8000.0), 8000.0, 20.0, max_f);
    let hs_g = db_to_lin(san(v.get(6).copied().unwrap_or(0.0), 0.0, -24.0, 24.0)).clamp(0.05, 16.0);

    let chain_l = lowshelf_hz(ls_f, 0.707, ls_g)
        >> bell_hz(pk_f, pk_q, pk_g)
        >> highshelf_hz(hs_f, 0.707, hs_g);
    let chain_r = lowshelf_hz(ls_f, 0.707, ls_g)
        >> bell_hz(pk_f, pk_q, pk_g)
        >> highshelf_hz(hs_f, 0.707, hs_g);
    Box::new(chain_l | chain_r)
}

fn build_filter_unit(kind: FxKind, sr: u32, v: &[f32]) -> Box<dyn AudioUnit> {
    let max_f = (sr as f32 * 0.45).max(100.0);
    let f = san(v.first().copied().unwrap_or(1000.0), 1000.0, 15.0, max_f);
    let q = san(v.get(1).copied().unwrap_or(0.707), 0.707, 0.1, 20.0);
    match kind {
        FxKind::HighPass => Box::new(highpass_hz(f, q) | highpass_hz(f, q)),
        _ => Box::new(lowpass_hz(f, q) | lowpass_hz(f, q)),
    }
}

fn build_reverb_unit(v: &[f32]) -> Box<dyn AudioUnit> {
    let room = 8.0 + san(v.first().copied().unwrap_or(0.6), 0.6, 0.0, 1.0) * 22.0;
    let damp = san(v.get(1).copied().unwrap_or(0.5), 0.5, 0.0, 1.0);
    let decay_s = (0.4 + room * 0.12).clamp(0.4, 5.0);
    Box::new(reverb_stereo(room, decay_s, damp))
}

fn build_limiter_unit(v: &[f32]) -> Box<dyn AudioUnit> {
    let att = san(v.first().copied().unwrap_or(5.0), 5.0, 0.5, 50.0) * 0.001;
    let rel = san(v.get(1).copied().unwrap_or(100.0), 100.0, 10.0, 1000.0) * 0.001;
    Box::new(limiter_stereo(att, rel))
}

#[inline]
fn run_stereo_unit_wet_dry(
    unit: &mut dyn AudioUnit,
    buf: &mut [f32],
    dry: f32,
    wet: f32,
    width: f32,
) {
    let frames = buf.len() / 2;
    let mut out = [0.0_f32; 2];
    for i in 0..frames {
        let il = if buf[i * 2].is_finite() {
            buf[i * 2]
        } else {
            0.0
        };
        let ir = if buf[i * 2 + 1].is_finite() {
            buf[i * 2 + 1]
        } else {
            0.0
        };
        unit.tick(&[il, ir], &mut out);
        let wl = if out[0].is_finite() { out[0] } else { 0.0 };
        let wr = if out[1].is_finite() { out[1] } else { 0.0 };
        let mid = 0.5 * (wl + wr);
        let side = 0.5 * (wl - wr) * width;
        let rl = mid + side;
        let rr = mid - side;
        buf[i * 2] = il * dry + rl * wet;
        buf[i * 2 + 1] = ir * dry + rr * wet;
    }
}

pub enum FxState {
    None,
    FundspNode(FundspNodeState),
    Dynamics {
        env: f32,
        gain: f32,
    },
    Delay {
        buf: Vec<f32>,
        pos: usize,
    },
    Chorus {
        buf: Vec<f32>,
        pos: usize,
        phase: f32,
    },
}

impl FxState {
    pub fn new(kind: FxKind, sr: u32) -> Self {
        let sr = sr.max(8000);
        match kind {
            FxKind::ParametricEq
            | FxKind::HighPass
            | FxKind::LowPass
            | FxKind::Reverb
            | FxKind::Limiter => FxState::FundspNode(FundspNodeState::new(sr)),
            FxKind::Compressor | FxKind::NoiseGate => FxState::Dynamics {
                env: 0.0,
                gain: 1.0,
            },
            FxKind::Delay => FxState::Delay {
                buf: vec![0.0; (sr as usize * 2 + 16) * 2],
                pos: 0,
            },
            FxKind::Chorus => FxState::Chorus {
                buf: vec![0.0; (sr as usize / 20 + 16) * 2],
                pos: 0,
                phase: 0.0,
            },
            _ => FxState::None,
        }
    }
}

fn quantize_key(vals: &[f32], scale: f32) -> Vec<i32> {
    vals.iter()
        .map(|&v| {
            if v.is_finite() {
                (v * scale).round().clamp(-1_000_000.0, 1_000_000.0) as i32
            } else {
                0
            }
        })
        .collect()
}

/// Processa um bloco estéreo intercalado. `t` = tempo local do início do bloco (em ticks).
pub fn process(fx: &FxInstance, st: &mut FxState, sr: u32, buf: &mut [f32], t: Ticks) {
    if !fx.enabled || !fx.kind.is_audio() || buf.is_empty() {
        return;
    }
    let sr = sr.max(8000);
    let v = fx.values(t);
    let srf = sr as f32;
    let frames = buf.len() / 2;
    match (fx.kind, st) {
        (FxKind::Gain, _) => {
            let g = db_to_lin(san(v.first().copied().unwrap_or(0.0), 0.0, -120.0, 36.0));
            buf.iter_mut()
                .for_each(|s| *s = if s.is_finite() { *s * g } else { 0.0 });
        }
        (FxKind::ParametricEq, FxState::FundspNode(node)) => {
            let key = quantize_key(&v, 20.0);
            node.ensure(sr, &key, || build_eq_unit(sr, &v));
            if let Some(unit) = node.unit.as_deref_mut() {
                run_stereo_unit_wet_dry(unit, buf, 0.0, 1.0, 1.0);
            }
        }
        (FxKind::HighPass | FxKind::LowPass, FxState::FundspNode(node)) => {
            let key = quantize_key(&v, 20.0);
            let kind = fx.kind;
            node.ensure(sr, &key, || build_filter_unit(kind, sr, &v));
            if let Some(unit) = node.unit.as_deref_mut() {
                run_stereo_unit_wet_dry(unit, buf, 0.0, 1.0, 1.0);
            }
        }
        (FxKind::Reverb, FxState::FundspNode(node)) => {
            let room_damp = [
                v.first().copied().unwrap_or(0.6),
                v.get(1).copied().unwrap_or(0.5),
            ];
            let key = quantize_key(&room_damp, 100.0);
            node.ensure(sr, &key, || build_reverb_unit(&v));
            let width = san(v.get(2).copied().unwrap_or(1.0), 1.0, 0.0, 1.0);
            let mix = san(v.get(3).copied().unwrap_or(0.25), 0.25, 0.0, 1.0);
            if let Some(unit) = node.unit.as_deref_mut() {
                run_stereo_unit_wet_dry(unit, buf, 1.0 - mix, mix, width);
            }
        }
        (FxKind::Limiter, FxState::FundspNode(node)) => {
            let timing = [
                v.first().copied().unwrap_or(5.0),
                v.get(1).copied().unwrap_or(100.0),
            ];
            let key = quantize_key(&timing, 10.0);
            node.ensure(sr, &key, || build_limiter_unit(&v));
            let drive = db_to_lin(san(v.get(2).copied().unwrap_or(0.0), 0.0, -12.0, 24.0));
            let ceil = db_to_lin(san(v.get(3).copied().unwrap_or(-0.3), -0.3, -24.0, 0.0));
            if let Some(unit) = node.unit.as_deref_mut() {
                let mut out = [0.0_f32; 2];
                for i in 0..frames {
                    let il = san(buf[i * 2] * drive, 0.0, -32.0, 32.0);
                    let ir = san(buf[i * 2 + 1] * drive, 0.0, -32.0, 32.0);
                    unit.tick(&[il, ir], &mut out);
                    buf[i * 2] = san(out[0], 0.0, -1.0, 1.0) * ceil;
                    buf[i * 2 + 1] = san(out[1], 0.0, -1.0, 1.0) * ceil;
                }
            }
        }
        (FxKind::Compressor, FxState::Dynamics { env, .. }) => {
            let thr = san(v[0], -18.0, -60.0, 0.0);
            let ratio = san(v[1], 4.0, 1.0, 20.0);
            let att = san(v[2], 10.0, 0.1, 200.0);
            let rel = san(v[3], 120.0, 5.0, 2000.0);
            let makeup = db_to_lin(san(v[4], 0.0, -12.0, 24.0));
            let ca = (-1.0 / (att * 0.001 * srf).max(1.0)).exp();
            let cr = (-1.0 / (rel * 0.001 * srf).max(1.0)).exp();
            for i in 0..frames {
                let x = buf[i * 2].abs().max(buf[i * 2 + 1].abs());
                let x = if x.is_finite() { x } else { 0.0 };
                let c = if x > *env { ca } else { cr };
                *env = x + (*env - x) * c;
                let lvl = crate::model::lin_to_db(*env);
                let gr = if lvl > thr {
                    (thr - lvl) * (1.0 - 1.0 / ratio)
                } else {
                    0.0
                };
                let g = db_to_lin(gr) * makeup;
                buf[i * 2] *= g;
                buf[i * 2 + 1] *= g;
            }
        }
        (FxKind::NoiseGate, FxState::Dynamics { env, gain }) => {
            let thr = db_to_lin(san(v[0], -50.0, -90.0, 0.0));
            let ca = (-1.0 / (san(v[1], 2.0, 0.1, 100.0) * 0.001 * srf).max(1.0)).exp();
            let cr = (-1.0 / (san(v[2], 100.0, 5.0, 2000.0) * 0.001 * srf).max(1.0)).exp();
            let det = (-1.0 / (0.005 * srf)).exp();
            for i in 0..frames {
                let x = buf[i * 2].abs().max(buf[i * 2 + 1].abs());
                let x = if x.is_finite() { x } else { 0.0 };
                *env = x.max(*env * det);
                let target = if *env > thr { 1.0 } else { 0.0 };
                let c = if target > *gain { ca } else { cr };
                *gain = target + (*gain - target) * c;
                buf[i * 2] *= *gain;
                buf[i * 2 + 1] *= *gain;
            }
        }
        (FxKind::Delay, FxState::Delay { buf: d, pos }) => {
            let len = (d.len() / 2).max(2);
            let dl = ((san(v[0], 350.0, 1.0, 2000.0) * 0.001 * srf) as usize).clamp(1, len - 1);
            let fb = san(v[1], 0.35, 0.0, 0.95);
            let mix = san(v[2], 0.3, 0.0, 1.0);
            for i in 0..frames {
                let rp = (*pos + len - dl) % len;
                for ch in 0..2 {
                    let x = san(buf[i * 2 + ch], 0.0, -64.0, 64.0);
                    let y = d[rp * 2 + ch];
                    d[*pos * 2 + ch] = x + y * fb;
                    buf[i * 2 + ch] = x * (1.0 - mix) + y * mix;
                }
                *pos = (*pos + 1) % len;
            }
        }
        (FxKind::Chorus, FxState::Chorus { buf: d, pos, phase }) => {
            let len = (d.len() / 2).max(4);
            let rate = san(v[0], 0.8, 0.05, 5.0);
            let depth = san(v[1], 5.0, 0.0, 20.0);
            let mix = san(v[2], 0.4, 0.0, 1.0);
            let base = 0.012 * srf;
            for i in 0..frames {
                for ch in 0..2 {
                    d[*pos * 2 + ch] = san(buf[i * 2 + ch], 0.0, -64.0, 64.0);
                }
                for ch in 0..2 {
                    let ph = *phase + ch as f32 * 0.25;
                    let lfo = (ph * 2.0 * std::f32::consts::PI).sin();
                    let delay = (base + lfo * depth * 0.001 * srf).clamp(1.0, (len - 2) as f32);
                    let rp = *pos as f32 - delay;
                    let rp = if rp < 0.0 { rp + len as f32 } else { rp };
                    let i0 = rp as usize % len;
                    let i1 = (i0 + 1) % len;
                    let fr = rp.fract();
                    let y = d[i0 * 2 + ch] * (1.0 - fr) + d[i1 * 2 + ch] * fr;
                    buf[i * 2 + ch] = buf[i * 2 + ch] * (1.0 - mix * 0.5) + y * mix;
                }
                *pos = (*pos + 1) % len;
                *phase = (*phase + rate / srf).fract();
            }
        }
        (FxKind::Distortion, _) => {
            let drive = san(v[0], 5.0, 1.0, 50.0);
            let mix = san(v[1], 1.0, 0.0, 1.0);
            let norm = 1.0 / drive.tanh();
            buf.iter_mut().for_each(|s| {
                let x = san(*s, 0.0, -64.0, 64.0);
                let y = (x * drive).tanh() * norm;
                *s = x * (1.0 - mix) + y * mix;
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_buf(sr: u32, freq: f32, frames: usize, amp: f32) -> Vec<f32> {
        let mut out = vec![0.0_f32; frames * 2];
        for i in 0..frames {
            let v = amp * (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin();
            out[i * 2] = v;
            out[i * 2 + 1] = v;
        }
        out
    }

    #[test]
    fn fundsp_reverb_eq_e_limiter_funcionam_e_toleram_entradas_hostis() {
        let sr = 48_000;
        let mut eq = FxInstance::new(FxKind::ParametricEq);
        eq.params[1].value = 6.0;
        let mut eq_st = FxState::new(FxKind::ParametricEq, sr);
        let mut buf = sine_buf(sr, 80.0, 2048, 0.25);
        process(&eq, &mut eq_st, sr, &mut buf, Ticks::ZERO);
        assert!(buf.iter().all(|s| s.is_finite()));

        let mut rv = FxInstance::new(FxKind::Reverb);
        rv.params[3].value = 0.5;
        let mut rv_st = FxState::new(FxKind::Reverb, sr);
        let mut imp = vec![0.0_f32; 4096 * 2];
        imp[0] = 0.9;
        imp[1] = 0.9;
        process(&rv, &mut rv_st, sr, &mut imp, Ticks::ZERO);
        assert!(imp.iter().all(|s| s.is_finite()));
        assert!(imp[100..].iter().any(|&s| s.abs() > 1e-4));

        let mut lim = FxInstance::new(FxKind::Limiter);
        lim.params[2].value = 18.0; // drive alto
        lim.params[3].value = -6.0; // teto ~0.5
        let mut lim_st = FxState::new(FxKind::Limiter, sr);
        let mut hot = sine_buf(sr, 440.0, 2048, 0.8);
        process(&lim, &mut lim_st, sr, &mut hot, Ticks::ZERO);
        assert!(hot.iter().all(|s| s.is_finite()));
        let ceiling = db_to_lin(-6.0) + 0.05;
        assert!(hot[512..].iter().all(|&s| s.abs() <= ceiling));

        // Entradas hostis (NaN/inf) não produzem NaN nem entram em pânico
        let mut hostile_eq = FxInstance::new(FxKind::LowPass);
        hostile_eq.params[0].value = f32::NAN;
        hostile_eq.params[1].value = f32::INFINITY;
        let mut st_h = FxState::new(FxKind::LowPass, sr);
        let mut bad = vec![f32::NAN; 128 * 2];
        process(&hostile_eq, &mut st_h, sr, &mut bad, Ticks::ZERO);
        assert!(bad.iter().all(|s| s.is_finite()));
    }
}
