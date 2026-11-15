//! Efeitos de vídeo (Video FX).

// Laços indexam vários buffers paralelos (canais/amostras) ao mesmo tempo;
// iteradores encadeados deixariam o DSP menos legível.
#![allow(clippy::needless_range_loop)]

use super::{luma, smoothstep, Frame};
use crate::model::{FxInstance, FxKind};
use rayon::prelude::*;

/// Aplica uma função de cor (em RGB não pré-multiplicado) a todos os pixels.
fn map_color<F: Fn([f32; 3]) -> [f32; 3] + Sync>(f: &mut Frame, func: F) {
    f.px.par_iter_mut().for_each(|p| {
        let a = p[3];
        if a <= 1e-6 {
            return;
        }
        let inv = 1.0 / a;
        let c = func([p[0] * inv, p[1] * inv, p[2] * inv]);
        p[0] = c[0].clamp(0.0, 1.0) * a;
        p[1] = c[1].clamp(0.0, 1.0) * a;
        p[2] = c[2].clamp(0.0, 1.0) * a;
    });
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn saturate(c: [f32; 3], s: f32) -> [f32; 3] {
    let l = luma(c);
    [l + (c[0] - l) * s, l + (c[1] - l) * s, l + (c[2] - l) * s]
}

/// Rotação de matiz preservando a luminância.
fn hue_matrix(deg: f32) -> [[f32; 3]; 3] {
    let (s, c) = deg.to_radians().sin_cos();
    let (lr, lg, lb) = (0.213, 0.715, 0.072);
    [
        [
            lr + c * (1.0 - lr) + s * (-lr),
            lg + c * (-lg) + s * (-lg),
            lb + c * (-lb) + s * (1.0 - lb),
        ],
        [
            lr + c * (-lr) + s * 0.143,
            lg + c * (1.0 - lg) + s * 0.140,
            lb + c * (-lb) + s * (-0.283),
        ],
        [
            lr + c * (-lr) + s * (-(1.0 - lr)),
            lg + c * (-lg) + s * lg,
            lb + c * (1.0 - lb) + s * lb,
        ],
    ]
}

fn mat3(m: &[[f32; 3]; 3], c: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * c[0] + m[0][1] * c[1] + m[0][2] * c[2],
        m[1][0] * c[0] + m[1][1] * c[1] + m[1][2] * c[2],
        m[2][0] * c[0] + m[2][1] * c[1] + m[2][2] * c[2],
    ]
}

/// Desfoque por caixa separável sobre os 4 canais (pré-multiplicados).
pub fn box_blur(f: &mut Frame, rx: usize, ry: usize) {
    let (w, h) = (f.w, f.h);
    if w == 0 || h == 0 {
        return;
    }
    if rx > 0 {
        let norm = 1.0 / (2 * rx + 1) as f32;
        f.px.par_chunks_mut(w).for_each(|row| {
            let src = row.to_vec();
            let mut acc = [0f32; 4];
            for i in 0..=rx {
                let p = src[i.min(w - 1)];
                for k in 0..4 {
                    acc[k] += p[k];
                }
            }
            for k in 0..4 {
                acc[k] += src[0][k] * rx as f32;
            }
            for x in 0..w {
                for k in 0..4 {
                    row[x][k] = acc[k] * norm;
                }
                let add = src[(x + rx + 1).min(w - 1)];
                let sub = src[x.saturating_sub(rx)];
                for k in 0..4 {
                    acc[k] += add[k] - sub[k];
                }
            }
        });
    }
    if ry > 0 {
        let norm = 1.0 / (2 * ry + 1) as f32;
        let src = f.px.clone();
        // processa por colunas em blocos para paralelizar
        let cols: Vec<Vec<[f32; 4]>> = (0..w)
            .into_par_iter()
            .map(|x| {
                let mut col = vec![[0f32; 4]; h];
                let mut acc = [0f32; 4];
                for i in 0..=ry {
                    let p = src[i.min(h - 1) * w + x];
                    for k in 0..4 {
                        acc[k] += p[k];
                    }
                }
                for k in 0..4 {
                    acc[k] += src[x][k] * ry as f32;
                }
                for y in 0..h {
                    for k in 0..4 {
                        col[y][k] = acc[k] * norm;
                    }
                    let add = src[(y + ry + 1).min(h - 1) * w + x];
                    let sub = src[y.saturating_sub(ry) * w + x];
                    for k in 0..4 {
                        acc[k] += add[k] - sub[k];
                    }
                }
                col
            })
            .collect();
        for (x, col) in cols.into_iter().enumerate() {
            for (y, p) in col.into_iter().enumerate() {
                f.px[y * w + x] = p;
            }
        }
    }
}

/// Aproximação de desfoque gaussiano com 3 passadas de caixa.
pub fn gaussian(f: &mut Frame, sx: f32, sy: f32) {
    let r = |s: f32| ((s / 3f32.sqrt()).round() as usize).max(if s > 0.3 { 1 } else { 0 });
    let (rx, ry) = (r(sx), r(sy));
    for _ in 0..3 {
        box_blur(f, rx, ry);
    }
}

fn hash(x: u32) -> u32 {
    let mut x = x;
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// Taxa usada para converter o tempo local em quadros OFX.
const OFX_TIME_FPS: f64 = 30.0;

/// Aplica um efeito OFX pelo host isolado. Em erro o quadro fica intacto e a
/// falha vai uma vez para o log (por efeito), sem travar a renderização.
fn apply_ofx(fx: &FxInstance, f: &mut Frame, t: crate::model::Ticks) {
    use std::sync::{Mutex, OnceLock, PoisonError};
    static LOGGED: OnceLock<Mutex<std::collections::HashSet<crate::model::Id>>> = OnceLock::new();
    let Some(r) = fx.ofx.as_ref() else {
        return;
    };
    let (Ok(w), Ok(h)) = (u32::try_from(f.w), u32::try_from(f.h)) else {
        return;
    };
    let Some(host) = crate::plugin_host::default_host_exe() else {
        return;
    };
    let time = t.0 as f64 / crate::model::TICKS_PER_SEC as f64 * OFX_TIME_FPS;
    let mut px: Vec<f32> = f.px.iter().flatten().copied().collect();
    match crate::plugin_host::ofx_render(&host, r, w, h, time, OFX_TIME_FPS, &mut px) {
        Ok(()) => {
            for (dst, src) in f.px.iter_mut().zip(px.as_chunks::<4>().0) {
                *dst = *src;
            }
        }
        Err(e) => {
            let set = LOGGED.get_or_init(|| Mutex::new(Default::default()));
            if set
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(fx.id)
            {
                crate::util::log_error(&format!("efeito OFX '{}' falhou: {e}", r.plugin_id));
            }
        }
    }
}

/// Aplica um efeito de vídeo. `t` é o tempo local (em ticks) usado para os
/// keyframes dos parâmetros.
pub fn apply(fx: &FxInstance, f: &mut Frame, t: crate::model::Ticks) {
    if !fx.enabled || fx.kind.is_audio() {
        return;
    }
    if fx.kind == FxKind::Ofx {
        apply_ofx(fx, f, t);
        return;
    }
    let v = fx.values(t);
    let hpx = f.h as f32;
    match fx.kind {
        FxKind::BrightnessContrast => {
            let (b, c) = (v[0], v[1]);
            let k = if c >= 0.0 { 1.0 + c * 3.0 } else { 1.0 + c };
            map_color(f, |x| {
                let g = |y: f32| (y - 0.5) * k + 0.5 + b;
                [g(x[0]), g(x[1]), g(x[2])]
            });
        }
        FxKind::HslAdjust => {
            let m = hue_matrix(v[0]);
            let (s, l) = (v[1], v[2]);
            map_color(f, |x| {
                let c = if v[0].abs() > 0.01 { mat3(&m, x) } else { x };
                let c = saturate(c, s);
                if l >= 0.0 {
                    mix3(c, [1.0; 3], l)
                } else {
                    mix3(c, [0.0; 3], -l)
                }
            });
        }
        FxKind::ColorCorrector => {
            let (lift, gamma, gain, temp, tint, sat) = (v[0], v[1], v[2], v[3], v[4], v[5]);
            let ig = 1.0 / gamma.max(0.01);
            map_color(f, |x| {
                let mut c = [0.0; 3];
                for k in 0..3 {
                    let y = gain * (x[k] + lift * (1.0 - x[k]));
                    c[k] = y.max(0.0).powf(ig);
                }
                c[0] += temp * 0.1;
                c[2] -= temp * 0.1;
                c[1] += tint * 0.1;
                saturate(c, sat)
            });
        }
        FxKind::Levels => {
            let (il, ih, g, ol, oh) = (v[0], v[1], v[2], v[3], v[4]);
            let span = (ih - il).max(1e-3);
            let ig = 1.0 / g.max(0.01);
            map_color(f, |x| {
                let m = |y: f32| ((y - il) / span).clamp(0.0, 1.0).powf(ig) * (oh - ol) + ol;
                [m(x[0]), m(x[1]), m(x[2])]
            });
        }
        FxKind::BlackWhite => {
            let a = v[0];
            map_color(f, |x| {
                let l = luma(x);
                mix3(x, [l; 3], a)
            });
        }
        FxKind::Sepia => {
            let a = v[0];
            map_color(f, |x| {
                let s = [
                    0.393 * x[0] + 0.769 * x[1] + 0.189 * x[2],
                    0.349 * x[0] + 0.686 * x[1] + 0.168 * x[2],
                    0.272 * x[0] + 0.534 * x[1] + 0.131 * x[2],
                ];
                mix3(x, s, a)
            });
        }
        FxKind::Invert => {
            let a = v[0];
            map_color(f, |x| mix3(x, [1.0 - x[0], 1.0 - x[1], 1.0 - x[2]], a));
        }
        FxKind::GaussianBlur => {
            gaussian(f, v[0] / 100.0 * hpx, v[1] / 100.0 * hpx);
        }
        FxKind::Sharpen => {
            let amt = v[0];
            let mut blur = f.clone();
            gaussian(&mut blur, (hpx * 0.0015).max(1.0), (hpx * 0.0015).max(1.0));
            f.px.par_iter_mut()
                .zip(blur.px.par_iter())
                .for_each(|(p, b)| {
                    for k in 0..3 {
                        p[k] = crate::util::clampf(p[k] + amt * (p[k] - b[k]), 0.0, p[3]);
                    }
                });
        }
        FxKind::Vignette => {
            let (amt, rad, soft) = (v[0], v[1], v[2]);
            let (w, h) = (f.w, f.h);
            let aspect = w as f32 / h as f32;
            f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                let ny = (y as f32 + 0.5) / h as f32 - 0.5;
                for (x, p) in row.iter_mut().enumerate() {
                    let nx = ((x as f32 + 0.5) / w as f32 - 0.5) * aspect;
                    let d =
                        (nx * nx + ny * ny).sqrt() / (0.5 * (aspect * aspect + 1.0).sqrt()) * 1.0;
                    let k = 1.0 - amt * smoothstep(rad * 0.7, rad * 0.7 + soft, d);
                    for c in p.iter_mut().take(3) {
                        *c *= k;
                    }
                }
            });
        }
        FxKind::ChromaKeyer => {
            let key = [v[0], v[1], v[2]];
            let (tol, soft, spill) = (v[3], v[4], v[5]);
            let chroma = |c: [f32; 3]| {
                let y = luma(c);
                ((c[2] - y) * 0.5389, (c[0] - y) * 0.6350)
            };
            let (kcb, kcr) = chroma(key);
            let dominant = if key[1] >= key[0] && key[1] >= key[2] {
                1
            } else if key[2] >= key[0] {
                2
            } else {
                0
            };
            f.px.par_iter_mut().for_each(|p| {
                let a = p[3];
                if a <= 1e-6 {
                    return;
                }
                let inv = 1.0 / a;
                let mut c = [p[0] * inv, p[1] * inv, p[2] * inv];
                let (cb, cr) = chroma(c);
                let d = ((cb - kcb).powi(2) + (cr - kcr).powi(2)).sqrt() * 2.0;
                let alpha = smoothstep(tol, tol + soft.max(1e-3), d);
                // supressão de spill: limita o canal dominante da cor-chave
                let others = match dominant {
                    0 => c[1].max(c[2]),
                    1 => c[0].max(c[2]),
                    _ => c[0].max(c[1]),
                };
                if c[dominant] > others {
                    c[dominant] -= (c[dominant] - others) * spill;
                }
                let na = a * alpha;
                p[0] = c[0] * na;
                p[1] = c[1] * na;
                p[2] = c[2] * na;
                p[3] = na;
            });
        }
        FxKind::Pixelate => {
            let bs = ((v[0] / 100.0 * hpx).round() as usize).max(1);
            if bs > 1 {
                let (w, h) = (f.w, f.h);
                let src = f.px.clone();
                f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                    let by = (y / bs) * bs;
                    for bx in (0..w).step_by(bs) {
                        let mut acc = [0f32; 4];
                        let mut n = 0.0;
                        for yy in by..(by + bs).min(h) {
                            for xx in bx..(bx + bs).min(w) {
                                let p = src[yy * w + xx];
                                for k in 0..4 {
                                    acc[k] += p[k];
                                }
                                n += 1.0;
                            }
                        }
                        for k in 0..4 {
                            acc[k] /= n;
                        }
                        for p in row.iter_mut().take((bx + bs).min(w)).skip(bx) {
                            *p = acc;
                        }
                    }
                });
            }
        }
        FxKind::Mirror => {
            let (w, h) = (f.w, f.h);
            let src = f.px.clone();
            let (hz, vt) = (v[0] > 0.5, v[1] > 0.5);
            f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                let sy = if vt { h - 1 - y } else { y };
                for (x, p) in row.iter_mut().enumerate() {
                    let sx = if hz { w - 1 - x } else { x };
                    *p = src[sy * w + sx];
                }
            });
        }
        FxKind::Glow => {
            let (thr, inten, rad) = (v[0], v[1], v[2]);
            let mut bright = f.clone();
            bright.px.par_iter_mut().for_each(|p| {
                let a = p[3].max(1e-6);
                let l = luma([p[0] / a, p[1] / a, p[2] / a]);
                let k = smoothstep(thr, (thr + 0.1).min(1.0), l);
                for c in p.iter_mut().take(3) {
                    *c *= k;
                }
            });
            let r = rad / 100.0 * hpx;
            gaussian(&mut bright, r, r);
            f.px.par_iter_mut()
                .zip(bright.px.par_iter())
                .for_each(|(p, b)| {
                    for k in 0..3 {
                        p[k] = (p[k] + b[k] * inten).min(1.0);
                    }
                    p[3] = p[3].max(p[0]).max(p[1]).max(p[2]);
                });
        }
        FxKind::FilmGrain => {
            let (amt, mono) = (v[0], v[1] > 0.5);
            let seed = t.div_i64(crate::time::TICKS_PER_MS).0 as u32;
            let w = f.w;
            f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                for (x, p) in row.iter_mut().enumerate() {
                    let hsh = hash(
                        (x as u32).wrapping_mul(7919)
                            ^ (y as u32).wrapping_mul(104_729)
                            ^ seed.wrapping_mul(1_299_709),
                    );
                    let n = |s: u32| ((hsh >> s) & 0xff) as f32 / 255.0 - 0.5;
                    let (a, b, c) = if mono {
                        (n(0), n(0), n(0))
                    } else {
                        (n(0), n(8), n(16))
                    };
                    p[0] = crate::util::clampf(p[0] + a * amt * p[3], 0.0, p[3]);
                    p[1] = crate::util::clampf(p[1] + b * amt * p[3], 0.0, p[3]);
                    p[2] = crate::util::clampf(p[2] + c * amt * p[3], 0.0, p[3]);
                }
            });
        }
        FxKind::Emboss => {
            let amt = v[0];
            let (w, h) = (f.w, f.h);
            let src = f.clone();
            f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                for (x, p) in row.iter_mut().enumerate() {
                    let (xi, yi) = (x as isize, y as isize);
                    let l = |dx: isize, dy: isize| {
                        let q = src.get_clamped(xi + dx, yi + dy);
                        luma([q[0], q[1], q[2]])
                    };
                    let e = 0.5 + (l(1, 1) - l(-1, -1)) * 2.0 + (l(1, 0) - l(-1, 0)) * 0.5;
                    let e = e.clamp(0.0, 1.0) * p[3];
                    for k in 0..3 {
                        p[k] = p[k] + (e - p[k]) * amt;
                    }
                }
            });
            let _ = h;
        }
        FxKind::Posterize => {
            let n = v[0].round().max(2.0);
            map_color(f, |x| {
                let q = |y: f32| (y * (n - 1.0)).round() / (n - 1.0);
                [q(x[0]), q(x[1]), q(x[2])]
            });
        }
        FxKind::Colorize => {
            let col = [v[0], v[1], v[2]];
            let a = v[3];
            map_color(f, |x| {
                let l = luma(x);
                let tinted = [l * col[0] * 1.5, l * col[1] * 1.5, l * col[2] * 1.5];
                mix3(x, tinted, a)
            });
        }
        _ => {}
    }
}
