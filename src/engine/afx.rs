//! Efeitos de áudio (Audio FX) com estado por instância.

// Laços indexam vários buffers paralelos (canais/amostras) ao mesmo tempo;
// iteradores encadeados deixariam o DSP menos legível.
#![allow(clippy::needless_range_loop)]

use crate::model::{db_to_lin, FxInstance, FxKind};

#[derive(Clone, Copy, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

#[derive(Clone, Copy)]
pub enum BqType {
    LowPass,
    HighPass,
    Peak,
    LowShelf,
    HighShelf,
}

impl Biquad {
    /// Fórmulas do "Audio EQ Cookbook" (RBJ).
    pub fn set(&mut self, ty: BqType, sr: f32, freq: f32, q: f32, gain_db: f32) {
        let f = crate::util::clampf(freq, 10.0, sr * 0.49);
        let w0 = 2.0 * std::f32::consts::PI * f / sr;
        let (sn, cs) = w0.sin_cos();
        let q = q.max(0.05);
        let alpha = sn / (2.0 * q);
        let a = 10f32.powf(gain_db / 40.0);
        let (b0, b1, b2, a0, a1, a2) = match ty {
            BqType::LowPass => (
                (1.0 - cs) / 2.0,
                1.0 - cs,
                (1.0 - cs) / 2.0,
                1.0 + alpha,
                -2.0 * cs,
                1.0 - alpha,
            ),
            BqType::HighPass => (
                (1.0 + cs) / 2.0,
                -(1.0 + cs),
                (1.0 + cs) / 2.0,
                1.0 + alpha,
                -2.0 * cs,
                1.0 - alpha,
            ),
            BqType::Peak => (
                1.0 + alpha * a,
                -2.0 * cs,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cs,
                1.0 - alpha / a,
            ),
            BqType::LowShelf => {
                let sq = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cs + sq),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cs),
                    a * ((a + 1.0) - (a - 1.0) * cs - sq),
                    (a + 1.0) + (a - 1.0) * cs + sq,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cs),
                    (a + 1.0) + (a - 1.0) * cs - sq,
                )
            }
            BqType::HighShelf => {
                let sq = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cs + sq),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cs),
                    a * ((a + 1.0) + (a - 1.0) * cs - sq),
                    (a + 1.0) - (a - 1.0) * cs + sq,
                    2.0 * ((a - 1.0) - (a + 1.0) * cs),
                    (a + 1.0) - (a - 1.0) * cs - sq,
                )
            }
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        // forma direta transposta II
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

struct Comb {
    buf: Vec<f32>,
    pos: usize,
    store: f32,
}

impl Comb {
    fn new(n: usize) -> Self {
        Comb {
            buf: vec![0.0; n.max(1)],
            pos: 0,
            store: 0.0,
        }
    }
    #[inline]
    fn run(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buf[self.pos];
        self.store = out * (1.0 - damp) + self.store * damp;
        self.buf[self.pos] = x + self.store * feedback;
        self.pos = (self.pos + 1) % self.buf.len();
        out
    }
}

struct AllPass {
    buf: Vec<f32>,
    pos: usize,
}

impl AllPass {
    fn new(n: usize) -> Self {
        AllPass {
            buf: vec![0.0; n.max(1)],
            pos: 0,
        }
    }
    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let b = self.buf[self.pos];
        let out = -x + b;
        self.buf[self.pos] = x + b * 0.5;
        self.pos = (self.pos + 1) % self.buf.len();
        out
    }
}

/// Reverb no estilo Freeverb (Schroeder/Moorer).
pub struct Freeverb {
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<AllPass>; 2],
}

impl Freeverb {
    fn new(sr: u32) -> Self {
        let k = sr as f32 / 44100.0;
        let combs_t = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
        let ap_t = [556, 441, 341, 225];
        let spread = 23;
        let mk = |off: usize| {
            (
                combs_t
                    .iter()
                    .map(|&n| Comb::new(((n + off) as f32 * k) as usize))
                    .collect::<Vec<_>>(),
                ap_t.iter()
                    .map(|&n| AllPass::new(((n + off) as f32 * k) as usize))
                    .collect::<Vec<_>>(),
            )
        };
        let (cl, al) = mk(0);
        let (cr, ar) = mk(spread);
        Freeverb {
            combs: [cl, cr],
            allpasses: [al, ar],
        }
    }
}

pub enum FxState {
    None,
    Filters(Vec<[Biquad; 2]>),
    Dynamics {
        env: f32,
        gain: f32,
    },
    Delay {
        buf: Vec<f32>,
        pos: usize,
    },
    Reverb(Box<Freeverb>),
    Chorus {
        buf: Vec<f32>,
        pos: usize,
        phase: f32,
    },
}

impl FxState {
    pub fn new(kind: FxKind, sr: u32) -> Self {
        match kind {
            FxKind::ParametricEq => FxState::Filters(vec![[Biquad::default(); 2]; 3]),
            FxKind::HighPass | FxKind::LowPass => FxState::Filters(vec![[Biquad::default(); 2]; 1]),
            FxKind::Compressor | FxKind::NoiseGate => FxState::Dynamics {
                env: 0.0,
                gain: 1.0,
            },
            FxKind::Delay => FxState::Delay {
                buf: vec![0.0; (sr as usize * 2 + 16) * 2],
                pos: 0,
            },
            FxKind::Reverb => FxState::Reverb(Box::new(Freeverb::new(sr))),
            FxKind::Chorus => FxState::Chorus {
                buf: vec![0.0; (sr as usize / 20 + 16) * 2],
                pos: 0,
                phase: 0.0,
            },
            _ => FxState::None,
        }
    }
}

/// Processa um bloco estéreo intercalado. `t` = tempo local do início do bloco.
pub fn process(fx: &FxInstance, st: &mut FxState, sr: u32, buf: &mut [f32], t: f64) {
    if !fx.enabled || !fx.kind.is_audio() {
        return;
    }
    let v = fx.values(t);
    let srf = sr as f32;
    let frames = buf.len() / 2;
    match (fx.kind, st) {
        (FxKind::Gain, _) => {
            let g = db_to_lin(v[0]);
            buf.iter_mut().for_each(|s| *s *= g);
        }
        (FxKind::ParametricEq, FxState::Filters(f)) => {
            for ch in 0..2 {
                f[0][ch].set(BqType::LowShelf, srf, v[0], 0.707, v[1]);
                f[1][ch].set(BqType::Peak, srf, v[2], v[4], v[3]);
                f[2][ch].set(BqType::HighShelf, srf, v[5], 0.707, v[6]);
            }
            for i in 0..frames {
                for ch in 0..2 {
                    let mut x = buf[i * 2 + ch];
                    for b in f.iter_mut() {
                        x = b[ch].run(x);
                    }
                    buf[i * 2 + ch] = x;
                }
            }
        }
        (FxKind::HighPass | FxKind::LowPass, FxState::Filters(f)) => {
            let ty = if fx.kind == FxKind::HighPass {
                BqType::HighPass
            } else {
                BqType::LowPass
            };
            for ch in 0..2 {
                f[0][ch].set(ty, srf, v[0], v[1], 0.0);
            }
            for i in 0..frames {
                for ch in 0..2 {
                    buf[i * 2 + ch] = f[0][ch].run(buf[i * 2 + ch]);
                }
            }
        }
        (FxKind::Compressor, FxState::Dynamics { env, .. }) => {
            let (thr, ratio, att, rel, makeup) = (v[0], v[1].max(1.0), v[2], v[3], db_to_lin(v[4]));
            let ca = (-1.0 / (att * 0.001 * srf).max(1.0)).exp();
            let cr = (-1.0 / (rel * 0.001 * srf).max(1.0)).exp();
            for i in 0..frames {
                let x = buf[i * 2].abs().max(buf[i * 2 + 1].abs());
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
            let thr = db_to_lin(v[0]);
            let ca = (-1.0 / (v[1] * 0.001 * srf).max(1.0)).exp();
            let cr = (-1.0 / (v[2] * 0.001 * srf).max(1.0)).exp();
            let det = (-1.0 / (0.005 * srf)).exp();
            for i in 0..frames {
                let x = buf[i * 2].abs().max(buf[i * 2 + 1].abs());
                *env = x.max(*env * det);
                let target = if *env > thr { 1.0 } else { 0.0 };
                let c = if target > *gain { ca } else { cr };
                *gain = target + (*gain - target) * c;
                buf[i * 2] *= *gain;
                buf[i * 2 + 1] *= *gain;
            }
        }
        (FxKind::Delay, FxState::Delay { buf: d, pos }) => {
            let len = d.len() / 2;
            let dl = ((v[0] * 0.001 * srf) as usize).clamp(1, len - 1);
            let (fb, mix) = (v[1], v[2]);
            for i in 0..frames {
                let rp = (*pos + len - dl) % len;
                for ch in 0..2 {
                    let x = buf[i * 2 + ch];
                    let y = d[rp * 2 + ch];
                    d[*pos * 2 + ch] = x + y * fb;
                    buf[i * 2 + ch] = x * (1.0 - mix) + y * mix;
                }
                *pos = (*pos + 1) % len;
            }
        }
        (FxKind::Reverb, FxState::Reverb(rv)) => {
            let (room, damp, width, mix) = (v[0], v[1], v[2], v[3]);
            let feedback = 0.7 + room * 0.28;
            let damp = damp * 0.4;
            let wet1 = width / 2.0 + 0.5;
            let wet2 = (1.0 - width) / 2.0;
            for i in 0..frames {
                let input = (buf[i * 2] + buf[i * 2 + 1]) * 0.015;
                let mut outs = [0f32; 2];
                for ch in 0..2 {
                    let mut acc = 0.0;
                    for c in rv.combs[ch].iter_mut() {
                        acc += c.run(input, feedback, damp);
                    }
                    for a in rv.allpasses[ch].iter_mut() {
                        acc = a.run(acc);
                    }
                    outs[ch] = acc;
                }
                let l = outs[0] * wet1 + outs[1] * wet2;
                let r = outs[1] * wet1 + outs[0] * wet2;
                buf[i * 2] = buf[i * 2] * (1.0 - mix) + l * mix * 3.0;
                buf[i * 2 + 1] = buf[i * 2 + 1] * (1.0 - mix) + r * mix * 3.0;
            }
        }
        (FxKind::Chorus, FxState::Chorus { buf: d, pos, phase }) => {
            let len = d.len() / 2;
            let (rate, depth, mix) = (v[0], v[1], v[2]);
            let base = 0.012 * srf;
            for i in 0..frames {
                for ch in 0..2 {
                    d[*pos * 2 + ch] = buf[i * 2 + ch];
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
            let (drive, mix) = (v[0], v[1]);
            let norm = 1.0 / drive.tanh();
            buf.iter_mut().for_each(|s| {
                let y = (*s * drive).tanh() * norm;
                *s = *s * (1.0 - mix) + y * mix;
            });
        }
        _ => {}
    }
}
