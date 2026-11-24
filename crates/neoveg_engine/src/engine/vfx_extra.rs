//! Efeitos de vídeo nativos complementares (cor, desfoque, distorção, luz,
//! estilo e composição), escritos do zero a partir do comportamento descrito
//! nos manuais públicos do VEGAS Pro (capítulo de plug-ins de vídeo).
//!
//! Todos operam em RGBA pré-multiplicado e toleram quadros vazios e
//! parâmetros extremos sem entrar em pânico (§0).

use rayon::prelude::*;

use super::vfx::{gaussian, hash, map_color, mix3, saturate};
use super::{luma, smoothstep, Frame};
use crate::model::{FxKind, Ticks};

/// Valor do parâmetro `i` (os valores já vêm limitados pela especificação).
#[inline]
fn g(v: &[f32], i: usize, d: f32) -> f32 {
    v.get(i).copied().filter(|x| x.is_finite()).unwrap_or(d)
}

/// Reamostra o quadro: para cada pixel de saída, `map(x, y)` devolve a
/// coordenada de origem em pixels (centro em +0,5) ou `None` (transparente).
fn remap<F>(f: &mut Frame, map: F)
where
    F: Fn(f32, f32) -> Option<(f32, f32)> + Sync,
{
    let w = f.w;
    if w == 0 || f.h == 0 {
        return;
    }
    let src = f.clone();
    f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, p) in row.iter_mut().enumerate() {
            *p = match map(x as f32 + 0.5, y as f32 + 0.5) {
                Some((sx, sy)) if sx.is_finite() && sy.is_finite() => src.sample(sx, sy),
                _ => [0.0; 4],
            };
        }
    });
}

/// Ruído determinístico em [0, 1) por pixel e semente.
#[inline]
fn noise01(x: usize, y: usize, seed: u32, salt: u32) -> f32 {
    let h = hash(
        (x as u32).wrapping_mul(7919)
            ^ (y as u32).wrapping_mul(104_729)
            ^ seed.wrapping_mul(1_299_709)
            ^ salt.wrapping_mul(2_654_435_761),
    );
    (h & 0x00ff_ffff) as f32 / 16_777_216.0
}

fn seed_of(t: Ticks) -> u32 {
    t.div_i64(crate::time::TICKS_PER_MS).0 as u32
}

/// Filtro de vizinhança quadrada (mediana, mínimo ou máximo) por canal.
fn rank_filter(f: &mut Frame, r: usize, mode: Rank) {
    let (w, h) = (f.w, f.h);
    if w == 0 || h == 0 || r == 0 {
        return;
    }
    let src = f.clone();
    f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let mut win: Vec<[f32; 4]> = Vec::with_capacity((2 * r + 1) * (2 * r + 1));
        for (x, p) in row.iter_mut().enumerate() {
            win.clear();
            for dy in -(r as isize)..=(r as isize) {
                for dx in -(r as isize)..=(r as isize) {
                    win.push(src.get_clamped(x as isize + dx, y as isize + dy));
                }
            }
            let mut o = [0.0f32; 4];
            for (k, ok) in o.iter_mut().enumerate() {
                *ok = match mode {
                    Rank::Min => win.iter().map(|q| q[k]).fold(f32::INFINITY, f32::min),
                    Rank::Max => win.iter().map(|q| q[k]).fold(f32::NEG_INFINITY, f32::max),
                    Rank::Median => {
                        let mut c: Vec<f32> = win.iter().map(|q| q[k]).collect();
                        c.sort_by(|a, b| a.total_cmp(b));
                        c.get(c.len() / 2).copied().unwrap_or(0.0)
                    }
                };
            }
            *p = o;
        }
    });
}

#[derive(Clone, Copy)]
enum Rank {
    Min,
    Max,
    Median,
}

/// Desfoque direcional: média de `n` amostras ao longo de `(dx, dy)` por passo.
fn directional_blur<F>(f: &mut Frame, steps: usize, offset: F)
where
    F: Fn(f32, f32, f32) -> (f32, f32) + Sync,
{
    let w = f.w;
    if w == 0 || f.h == 0 || steps < 2 {
        return;
    }
    let src = f.clone();
    let n = steps as f32;
    f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, p) in row.iter_mut().enumerate() {
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            let mut acc = [0.0f32; 4];
            for i in 0..steps {
                let s = i as f32 / (n - 1.0) - 0.5;
                let (sx, sy) = offset(cx, cy, s);
                let q = src.sample(sx.clamp(0.0, src.w as f32), sy.clamp(0.0, src.h as f32));
                for k in 0..4 {
                    acc[k] += q[k];
                }
            }
            for k in 0..4 {
                p[k] = acc[k] / n;
            }
        }
    });
}

/// Distorção radial dentro de um círculo de raio `rad` (fração da metade
/// da menor dimensão) centrado no quadro; `warp(d)` devolve a nova distância
/// normalizada de origem (0..1) e `rot(d)` uma rotação em radianos.
fn radial_warp<W, R>(f: &mut Frame, rad: f32, warp: W, rot: R)
where
    W: Fn(f32) -> f32 + Sync,
    R: Fn(f32) -> f32 + Sync,
{
    let (w, h) = (f.w as f32, f.h as f32);
    let (cx, cy) = (w * 0.5, h * 0.5);
    let radius = (w.min(h) * 0.5 * rad).max(1.0);
    remap(f, |x, y| {
        let (dx, dy) = (x - cx, y - cy);
        let d = (dx * dx + dy * dy).sqrt() / radius;
        if d >= 1.0 || d <= 1e-6 {
            return Some((x, y));
        }
        let nd = warp(d);
        let a = rot(d);
        let (s, c) = a.sin_cos();
        let k = nd / d;
        let (ux, uy) = (dx * k, dy * k);
        Some((cx + ux * c - uy * s, cy + ux * s + uy * c))
    });
}

/// Aplica um dos efeitos complementares. Devolve `false` se `kind` não for
/// um deles (o chamador ignora).
pub(crate) fn apply(kind: FxKind, v: &[f32], f: &mut Frame, t: Ticks) -> bool {
    let (w, h) = (f.w, f.h);
    if w == 0 || h == 0 {
        return is_extra(kind);
    }
    let (wf, hf) = (w as f32, h as f32);
    match kind {
        // ---------------------------------------------------------------- cor
        FxKind::WhiteBalance => {
            let neutral = [g(v, 0, 1.0), g(v, 1, 1.0), g(v, 2, 1.0)];
            let a = g(v, 3, 1.0);
            let l = luma(neutral).max(1e-3);
            let gain = [
                l / neutral[0].max(1e-3),
                l / neutral[1].max(1e-3),
                l / neutral[2].max(1e-3),
            ];
            map_color(f, |x| {
                let c = [x[0] * gain[0], x[1] * gain[1], x[2] * gain[2]];
                mix3(x, c, a)
            });
        }
        FxKind::ColorBalance => {
            let (cr, mg, yb) = (g(v, 0, 0.0), g(v, 1, 0.0), g(v, 2, 0.0));
            let keep = g(v, 3, 1.0) > 0.5;
            map_color(f, |x| {
                let mut c = [x[0] + cr * 0.25, x[1] + mg * 0.25, x[2] + yb * 0.25];
                if keep {
                    let d = luma(x) - luma(c);
                    c = [c[0] + d, c[1] + d, c[2] + d];
                }
                c
            });
        }
        FxKind::SaturationAdjust => {
            let (all, lo, hi) = (g(v, 0, 1.0), g(v, 1, 1.0), g(v, 2, 1.0));
            map_color(f, |x| {
                let l = luma(x).clamp(0.0, 1.0);
                let s = all * (lo + (hi - lo) * smoothstep(0.0, 1.0, l));
                saturate(x, s)
            });
        }
        FxKind::ChannelBlend => {
            let m = [
                [g(v, 0, 1.0), g(v, 1, 0.0), g(v, 2, 0.0)],
                [g(v, 3, 0.0), g(v, 4, 1.0), g(v, 5, 0.0)],
                [g(v, 6, 0.0), g(v, 7, 0.0), g(v, 8, 1.0)],
            ];
            map_color(f, |x| {
                [
                    m[0][0] * x[0] + m[0][1] * x[1] + m[0][2] * x[2],
                    m[1][0] * x[0] + m[1][1] * x[1] + m[1][2] * x[2],
                    m[2][0] * x[0] + m[2][1] * x[1] + m[2][2] * x[2],
                ]
            });
        }
        FxKind::GradientMap => {
            let dark = [g(v, 0, 0.0), g(v, 1, 0.0), g(v, 2, 0.3)];
            let light = [g(v, 3, 1.0), g(v, 4, 0.9), g(v, 5, 0.6)];
            let a = g(v, 6, 1.0);
            map_color(f, |x| {
                let l = luma(x).clamp(0.0, 1.0);
                mix3(x, mix3(dark, light, l), a)
            });
        }
        FxKind::Threshold => {
            let (thr, soft) = (g(v, 0, 0.5), g(v, 1, 0.02));
            map_color(f, |x| {
                let k = smoothstep(thr - soft, thr + soft, luma(x));
                [k; 3]
            });
        }
        // ------------------------------------------------- desfoque/nitidez
        FxKind::UnsharpMask => {
            let (amt, rad, thr) = (g(v, 0, 1.0), g(v, 1, 0.5), g(v, 2, 0.0));
            let r = (rad / 100.0 * hf).max(0.5);
            let mut blur = f.clone();
            gaussian(&mut blur, r, r);
            f.px.par_iter_mut()
                .zip(blur.px.par_iter())
                .for_each(|(p, b)| {
                    for k in 0..3 {
                        let d = p[k] - b[k];
                        if d.abs() >= thr * p[3] {
                            p[k] = crate::util::clampf(p[k] + amt * d, 0.0, p[3]);
                        }
                    }
                });
        }
        FxKind::LinearBlur => {
            let dist = g(v, 0, 2.0) / 100.0 * hf;
            let (s, c) = g(v, 1, 0.0).to_radians().sin_cos();
            let steps = (dist.round() as usize).clamp(2, 64);
            if dist >= 0.5 {
                directional_blur(f, steps, |x, y, k| (x + c * dist * k, y + s * dist * k));
            }
        }
        FxKind::RadialBlur => {
            let amt = g(v, 0, 0.2);
            let (cx, cy) = (g(v, 1, 0.5) * wf, g(v, 2, 0.5) * hf);
            let steps = ((amt * wf.max(hf) * 0.25).round() as usize).clamp(2, 48);
            if amt > 1e-3 {
                directional_blur(f, steps, |x, y, k| {
                    let s = 1.0 + amt * k;
                    (cx + (x - cx) * s, cy + (y - cy) * s)
                });
            }
        }
        FxKind::Median => {
            let r = g(v, 0, 1.0).round().clamp(1.0, 5.0) as usize;
            rank_filter(f, r, Rank::Median);
        }
        FxKind::MinMax => {
            let mode = if g(v, 0, 0.0) > 0.5 {
                Rank::Max
            } else {
                Rank::Min
            };
            let r = g(v, 1, 2.0).round().clamp(1.0, 10.0) as usize;
            rank_filter(f, r, mode);
        }
        // -------------------------------------------------------- distorção
        FxKind::Wave => {
            let amp = g(v, 0, 2.0) / 100.0 * hf;
            let len = (g(v, 1, 20.0) / 100.0 * hf).max(1.0);
            let ph = g(v, 2, 0.0).to_radians();
            let horiz = g(v, 3, 1.0) > 0.5;
            let tau = std::f32::consts::TAU;
            remap(f, |x, y| {
                if horiz {
                    Some((x + amp * (y / len * tau + ph).sin(), y))
                } else {
                    Some((x, y + amp * (x / len * tau + ph).sin()))
                }
            });
        }
        FxKind::Swirl => {
            let ang = g(v, 0, 180.0).to_radians();
            let rad = g(v, 1, 0.5);
            radial_warp(f, rad, |d| d, |d| ang * (1.0 - d) * (1.0 - d));
        }
        FxKind::Spherize => {
            let amt = g(v, 0, 0.5);
            let rad = g(v, 1, 0.5);
            radial_warp(
                f,
                rad,
                |d| {
                    // esfera: a origem se aproxima do centro (lupa) quando amt > 0
                    let bulge = (d * std::f32::consts::FRAC_PI_2).sin();
                    let pinch = d * d;
                    if amt >= 0.0 {
                        d + (pinch - d) * amt
                    } else {
                        d + (bulge - d) * -amt
                    }
                },
                |_| 0.0,
            );
        }
        FxKind::PinchPunch => {
            let amt = g(v, 0, 0.5);
            let rad = g(v, 1, 0.5);
            let e = 2f32.powf(amt * 1.5);
            radial_warp(f, rad, |d| d.powf(1.0 / e), |_| 0.0);
        }
        FxKind::OffsetWrap => {
            let (ox, oy) = (g(v, 0, 0.0) / 100.0 * wf, g(v, 1, 0.0) / 100.0 * hf);
            let wrap = g(v, 2, 1.0) > 0.5;
            remap(f, |x, y| {
                let (sx, sy) = (x - ox, y - oy);
                if wrap {
                    Some((sx.rem_euclid(wf), sy.rem_euclid(hf)))
                } else if (0.0..wf).contains(&sx) && (0.0..hf).contains(&sy) {
                    Some((sx, sy))
                } else {
                    None
                }
            });
        }
        // ------------------------------------------------------------- luz
        FxKind::LensFlare => {
            let (px, py) = (g(v, 0, 0.3) * wf, g(v, 1, 0.3) * hf);
            let inten = g(v, 2, 1.0);
            let size = g(v, 3, 0.2) * wf.min(hf);
            let (cx, cy) = (wf * 0.5, hf * 0.5);
            // halos ao longo da linha fonte → centro → lado oposto
            let ghosts: [(f32, f32, [f32; 3]); 4] = [
                (0.5, 0.25, [0.9, 0.6, 0.3]),
                (1.2, 0.15, [0.3, 0.8, 0.5]),
                (1.6, 0.35, [0.4, 0.5, 1.0]),
                (2.0, 0.1, [1.0, 0.4, 0.7]),
            ];
            f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                let fy = y as f32 + 0.5;
                for (x, p) in row.iter_mut().enumerate() {
                    let fx = x as f32 + 0.5;
                    let d = ((fx - px).powi(2) + (fy - py).powi(2)).sqrt() / size.max(1.0);
                    let core = (-d * d * 4.0).exp() + 0.25 * (-d * 0.8).exp();
                    let mut add = [core; 3];
                    for (pos, r, col) in ghosts {
                        let gx = px + (cx - px) * pos;
                        let gy = py + (cy - py) * pos;
                        let gd =
                            ((fx - gx).powi(2) + (fy - gy).powi(2)).sqrt() / (size * r).max(1.0);
                        let ring = (1.0 - smoothstep(0.7, 1.0, gd)) * 0.18;
                        for k in 0..3 {
                            add[k] += ring * col[k];
                        }
                    }
                    for k in 0..3 {
                        p[k] = (p[k] + add[k] * inten).min(1.0);
                    }
                    p[3] = p[3].max(p[0]).max(p[1]).max(p[2]);
                }
            });
        }
        FxKind::LightRays => {
            let (cx, cy) = (g(v, 0, 0.5) * wf, g(v, 1, 0.5) * hf);
            let len = g(v, 2, 0.4);
            let inten = g(v, 3, 0.8);
            let thr = g(v, 4, 0.6);
            let mut bright = f.clone();
            bright.px.par_iter_mut().for_each(|p| {
                let a = p[3].max(1e-6);
                let k = smoothstep(
                    thr,
                    (thr + 0.1).min(1.0),
                    luma([p[0] / a, p[1] / a, p[2] / a]),
                );
                for c in p.iter_mut() {
                    *c *= k;
                }
            });
            let steps = ((len * 40.0).round() as usize).clamp(2, 40);
            if len > 1e-3 {
                // raios saindo do centro: média das amostras em direção a ele
                directional_blur(&mut bright, steps, |x, y, k| {
                    let s = 1.0 - len * (k + 0.5);
                    (cx + (x - cx) * s, cy + (y - cy) * s)
                });
            }
            f.px.par_iter_mut()
                .zip(bright.px.par_iter())
                .for_each(|(p, b)| {
                    for k in 0..3 {
                        p[k] = (p[k] + b[k] * inten * 1.5).min(1.0);
                    }
                    p[3] = p[3].max(p[0]).max(p[1]).max(p[2]);
                });
        }
        // ---------------------------------------------------------- estilo
        FxKind::AddNoise => {
            let amt = g(v, 0, 0.2);
            let mono = g(v, 1, 0.0) > 0.5;
            let seed = if g(v, 2, 1.0) > 0.5 { seed_of(t) } else { 0 };
            f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                for (x, p) in row.iter_mut().enumerate() {
                    // soma de 2 uniformes ≈ distribuição triangular (mais natural)
                    let n = |s: u32| {
                        noise01(x, y, seed, s) + noise01(x, y, seed, s.wrapping_add(97)) - 1.0
                    };
                    let (a, b, c) = if mono {
                        let m = n(1);
                        (m, m, m)
                    } else {
                        (n(1), n(2), n(3))
                    };
                    for (k, d) in [a, b, c].into_iter().enumerate() {
                        p[k] = crate::util::clampf(p[k] + d * amt * p[3], 0.0, p[3]);
                    }
                }
            });
        }
        FxKind::TvSimulator => {
            let (lines, noise, shift, vig) =
                (g(v, 0, 0.5), g(v, 1, 0.1), g(v, 2, 0.2), g(v, 3, 0.3));
            let seed = seed_of(t);
            let dx = shift / 100.0 * wf;
            let src = f.clone();
            f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                let fy = y as f32 + 0.5;
                let scan = 1.0 - lines * 0.5 * (1.0 + (fy * std::f32::consts::PI).cos()) * 0.5;
                let ny = fy / hf - 0.5;
                for (x, p) in row.iter_mut().enumerate() {
                    let fx = x as f32 + 0.5;
                    let r = src.sample((fx + dx).min(wf), fy);
                    let gch = src.sample(fx, fy);
                    let b = src.sample((fx - dx).max(0.0), fy);
                    let nx = fx / wf - 0.5;
                    let vg = 1.0 - vig * smoothstep(0.3, 0.75, (nx * nx + ny * ny).sqrt());
                    let nz = (noise01(x, y, seed, 5) - 0.5) * noise;
                    let a = gch[3].max(r[3]).max(b[3]);
                    let k = scan * vg;
                    p[0] = crate::util::clampf(r[0] * k + nz * a, 0.0, a);
                    p[1] = crate::util::clampf(gch[1] * k + nz * a, 0.0, a);
                    p[2] = crate::util::clampf(b[2] * k + nz * a, 0.0, a);
                    p[3] = a;
                }
            });
        }
        FxKind::FilmEffects => {
            let (age, scratch, flicker, dust) =
                (g(v, 0, 0.5), g(v, 1, 0.3), g(v, 2, 0.2), g(v, 3, 0.2));
            let seed = seed_of(t) / 40; // muda ~25 vezes por segundo
            let fl = 1.0 + (noise01(0, 0, seed, 11) - 0.5) * flicker * 0.6;
            // até 3 arranhões verticais por quadro
            let cols: Vec<f32> = (0..3u32)
                .filter(|i| noise01(0, 0, seed, 20 + i) < scratch)
                .map(|i| noise01(1, 0, seed, 30 + i) * wf)
                .collect();
            f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                for (x, p) in row.iter_mut().enumerate() {
                    let a = p[3];
                    if a <= 1e-6 {
                        continue;
                    }
                    let inv = 1.0 / a;
                    let c = [p[0] * inv, p[1] * inv, p[2] * inv];
                    let sep = [
                        0.393 * c[0] + 0.769 * c[1] + 0.189 * c[2],
                        0.349 * c[0] + 0.686 * c[1] + 0.168 * c[2],
                        0.272 * c[0] + 0.534 * c[1] + 0.131 * c[2],
                    ];
                    let mut o = mix3(c, sep, age);
                    for k in o.iter_mut() {
                        *k *= fl;
                    }
                    let fx = x as f32 + 0.5;
                    if cols.iter().any(|cx| (fx - cx).abs() < 0.8) {
                        o = mix3(o, [0.9; 3], 0.6);
                    }
                    if noise01(x / 2, y / 2, seed, 40) < dust * 0.004 {
                        o = [0.05; 3];
                    }
                    for k in 0..3 {
                        p[k] = o[k].clamp(0.0, 1.0) * a;
                    }
                }
            });
        }
        FxKind::Newsprint => {
            let cell = (g(v, 0, 1.0) / 100.0 * hf).max(2.0);
            let (s, c) = g(v, 1, 45.0).to_radians().sin_cos();
            let color = g(v, 2, 0.0) > 0.5;
            map_frame_xy(f, |x, y, col| {
                // coordenadas na grade girada
                let (u, w2) = ((x * c + y * s) / cell, (-x * s + y * c) / cell);
                let (fu, fw) = (u - u.floor() - 0.5, w2 - w2.floor() - 0.5);
                let d = (fu * fu + fw * fw).sqrt() * std::f32::consts::SQRT_2;
                let dot = |ink: f32| 1.0 - smoothstep(ink - 0.08, ink + 0.08, d);
                if color {
                    [
                        1.0 - dot((1.0 - col[0]).sqrt()),
                        1.0 - dot((1.0 - col[1]).sqrt()),
                        1.0 - dot((1.0 - col[2]).sqrt()),
                    ]
                } else {
                    let k = 1.0 - dot((1.0 - luma(col)).clamp(0.0, 1.0).sqrt());
                    [k; 3]
                }
            });
        }
        // ------------------------------------------------------- composição
        FxKind::Border => {
            let th = g(v, 0, 3.0) / 100.0 * hf;
            let col = [g(v, 1, 1.0), g(v, 2, 1.0), g(v, 3, 1.0)];
            let soft = (g(v, 4, 0.0) / 100.0 * hf).max(1e-3);
            f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                let fy = y as f32 + 0.5;
                for (x, p) in row.iter_mut().enumerate() {
                    let fx = x as f32 + 0.5;
                    let edge = fx.min(wf - fx).min(fy).min(hf - fy);
                    let k = 1.0 - smoothstep(th, th + soft, edge);
                    if k > 0.0 {
                        for i in 0..3 {
                            p[i] = p[i] * (1.0 - k) + col[i] * k;
                        }
                        p[3] = p[3] * (1.0 - k) + k;
                    }
                }
            });
        }
        FxKind::BlackBarFill => {
            let blur = g(v, 0, 3.0) / 100.0 * hf;
            let dim = g(v, 1, 0.8);
            let thr = g(v, 2, 0.03);
            let Some((x0, y0, x1, y1)) = content_box(f, thr) else {
                return true;
            };
            let (bw, bh) = ((x1 - x0) as f32, (y1 - y0) as f32);
            if bw >= wf - 0.5 && bh >= hf - 0.5 {
                return true;
            }
            // fundo: o conteúdo ampliado para cobrir o quadro, desfocado e escurecido
            let s = (wf / bw).max(hf / bh);
            let (ccx, ccy) = ((x0 + x1) as f32 * 0.5, (y0 + y1) as f32 * 0.5);
            let mut bg = f.clone();
            remap(&mut bg, |x, y| {
                Some((
                    (ccx + (x - wf * 0.5) / s).clamp(x0 as f32, x1 as f32),
                    (ccy + (y - hf * 0.5) / s).clamp(y0 as f32, y1 as f32),
                ))
            });
            gaussian(&mut bg, blur, blur);
            let src = f.clone();
            f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                for (x, p) in row.iter_mut().enumerate() {
                    let inside = (x0..x1).contains(&x) && (y0..y1).contains(&y);
                    let b = bg.px.get(y * w + x).copied().unwrap_or([0.0; 4]);
                    let s = src.px.get(y * w + x).copied().unwrap_or([0.0; 4]);
                    *p = if inside {
                        s
                    } else {
                        [b[0] * dim, b[1] * dim, b[2] * dim, b[3]]
                    };
                }
            });
        }
        FxKind::PictureInPicture => {
            let (px, py) = (g(v, 0, 0.75) * wf, g(v, 1, 0.75) * hf);
            let sc = g(v, 2, 0.35).max(0.01);
            let (rs, rc) = g(v, 3, 0.0).to_radians().sin_cos();
            let border = g(v, 4, 0.5) / 100.0 * hf;
            let (hw, hh) = (wf * sc * 0.5, hf * sc * 0.5);
            remap_with_border(f, border, |x, y| {
                let (dx, dy) = (x - px, y - py);
                let (lx, ly) = (dx * rc + dy * rs, -dx * rs + dy * rc);
                (lx, ly, hw, hh, sc)
            });
        }
        FxKind::CookieCutter => {
            let shape = g(v, 0, 0.0).round() as i32;
            let size = g(v, 1, 0.5);
            let (cx, cy) = (g(v, 2, 0.5) * wf, g(v, 3, 0.5) * hf);
            let soft = g(v, 4, 0.01).max(1e-3);
            let inv = g(v, 5, 0.0) > 0.5;
            let r = (hf * 0.5 * size).max(1.0);
            f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                let ny = (y as f32 + 0.5 - cy) / r;
                for (x, p) in row.iter_mut().enumerate() {
                    let nx = (x as f32 + 0.5 - cx) / r;
                    // distância normalizada à borda da forma (1 = borda)
                    let d = match shape {
                        1 => nx.abs().max(ny.abs()),
                        2 => nx.abs() + ny.abs(),
                        3 => {
                            let a = ny.atan2(nx);
                            let k = 0.55 + 0.45 * (2.5 * a).cos().abs().powf(3.0);
                            (nx * nx + ny * ny).sqrt() / k
                        }
                        _ => (nx * nx + ny * ny).sqrt(),
                    };
                    let mut k = 1.0 - smoothstep(1.0 - soft, 1.0 + soft, d);
                    if inv {
                        k = 1.0 - k;
                    }
                    for c in p.iter_mut() {
                        *c *= k;
                    }
                }
            });
        }
        FxKind::ColorCorrectorSecondary => {
            let target_h = g(v, 0, 0.0).rem_euclid(360.0) / 360.0;
            let width_h = (g(v, 1, 45.0).clamp(5.0, 180.0)) / 360.0;
            let shift_h = g(v, 2, 0.0).clamp(-180.0, 180.0) / 360.0;
            let sat_m = g(v, 3, 1.0).clamp(0.0, 3.0);
            let gain_m = g(v, 4, 1.0).clamp(0.0, 2.0);
            let inv = g(v, 5, 0.0) > 0.5;
            map_color(f, |c| {
                let (h, s, l) = rgb_to_hsl_local(c);
                let mut dist = (h - target_h).abs();
                if dist > 0.5 {
                    dist = 1.0 - dist;
                }
                let mut mask =
                    (1.0 - smoothstep(width_h * 0.4, width_h, dist)) * smoothstep(0.04, 0.14, s);
                if inv {
                    mask = 1.0 - mask;
                }
                if mask <= 1e-4 {
                    return c;
                }
                let nh = (h + shift_h).rem_euclid(1.0);
                let ns = (s * sat_m).clamp(0.0, 1.0);
                let nl = (l * gain_m).clamp(0.0, 1.0);
                let out_c = hsl_to_rgb_local(nh, ns, nl);
                mix3(c, out_c, mask)
            });
        }
        FxKind::LensCorrection => {
            let k1 = g(v, 0, 0.0).clamp(-1.0, 1.0);
            let k2 = g(v, 1, 0.0).clamp(-1.0, 1.0);
            let zoom = g(v, 2, 1.0).clamp(0.5, 2.0);
            let cx = g(v, 3, 0.5) * wf;
            let cy = g(v, 4, 0.5) * hf;
            let norm = (wf.hypot(hf) * 0.5).max(1.0);
            remap(f, |x, y| {
                let dx = (x - cx) / (norm * zoom);
                let dy = (y - cy) / (norm * zoom);
                let r2 = dx * dx + dy * dy;
                let factor = 1.0 + k1 * r2 + k2 * r2 * r2;
                let sx = cx + dx * factor * norm;
                let sy = cy + dy * factor * norm;
                if sx >= 0.0 && sy >= 0.0 && sx < wf && sy < hf {
                    Some((sx, sy))
                } else {
                    None
                }
            });
        }
        FxKind::BumpMap => {
            let height = g(v, 0, 1.5).clamp(0.0, 5.0);
            let ang = g(v, 1, 45.0).to_radians();
            let ambient = g(v, 2, 0.5).clamp(0.0, 1.0);
            let mix = g(v, 3, 0.6).clamp(0.0, 1.0);
            let (lx, ly) = (ang.cos(), ang.sin());
            let src = f.clone();
            f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                for (x, p) in row.iter_mut().enumerate() {
                    let a = p[3];
                    if a <= 1e-6 {
                        continue;
                    }
                    let lum_at = |dx: isize, dy: isize| {
                        let q = src.get_clamped(x as isize + dx, y as isize + dy);
                        let qa = q[3].max(1e-6);
                        luma([q[0] / qa, q[1] / qa, q[2] / qa])
                    };
                    let gx = (lum_at(1, 0) - lum_at(-1, 0)) * height * 2.0;
                    let gy = (lum_at(0, 1) - lum_at(0, -1)) * height * 2.0;
                    let nz = 1.0_f32;
                    let inv_len = 1.0 / (gx * gx + gy * gy + nz * nz).sqrt();
                    let dot = ((-gx * lx - gy * ly + nz * 0.7) * inv_len).clamp(0.0, 1.5);
                    let light = (ambient + (1.0 - ambient) * dot).clamp(0.0, 1.8);
                    let inv = 1.0 / a;
                    let base = [p[0] * inv, p[1] * inv, p[2] * inv];
                    let lit = [
                        (base[0] * light).clamp(0.0, 1.0),
                        (base[1] * light).clamp(0.0, 1.0),
                        (base[2] * light).clamp(0.0, 1.0),
                    ];
                    let out_c = mix3(base, lit, mix);
                    p[0] = out_c[0].clamp(0.0, 1.0) * a;
                    p[1] = out_c[1].clamp(0.0, 1.0) * a;
                    p[2] = out_c[2].clamp(0.0, 1.0) * a;
                }
            });
        }
        FxKind::ConvolutionKernel => {
            let preset = g(v, 0, 0.0).round() as i32;
            let strength = g(v, 1, 1.0).clamp(0.0, 2.0);
            let mix = g(v, 2, 1.0).clamp(0.0, 1.0);
            let k: [f32; 9] = match preset {
                1 => [-1.0, -1.0, -1.0, -1.0, 8.0, -1.0, -1.0, -1.0, -1.0], // Bordas
                2 => [-2.0, -1.0, 0.0, -1.0, 1.0, 1.0, 0.0, 1.0, 2.0],      // Relevo
                3 => [0.0, -1.0, 0.0, -1.0, 4.0, -1.0, 0.0, -1.0, 0.0],     // Laplaciano
                4 => [1.0 / 9.0; 9],                                        // Caixa
                _ => [0.0, -1.0, 0.0, -1.0, 5.0, -1.0, 0.0, -1.0, 0.0],     // Nitidez
            };
            let src = f.clone();
            f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                for (x, p) in row.iter_mut().enumerate() {
                    let a = p[3];
                    if a <= 1e-6 {
                        continue;
                    }
                    let mut acc = [0.0_f32; 3];
                    let mut ki = 0usize;
                    for dy in -1..=1_isize {
                        for dx in -1..=1_isize {
                            let q = src.get_clamped(x as isize + dx, y as isize + dy);
                            let qa = q[3].max(1e-6);
                            let w_k = k[ki];
                            acc[0] += (q[0] / qa) * w_k;
                            acc[1] += (q[1] / qa) * w_k;
                            acc[2] += (q[2] / qa) * w_k;
                            ki += 1;
                        }
                    }
                    let inv = 1.0 / a;
                    let base = [p[0] * inv, p[1] * inv, p[2] * inv];
                    let conv = [
                        (base[0] + (acc[0] - base[0]) * strength).clamp(0.0, 1.0),
                        (base[1] + (acc[1] - base[1]) * strength).clamp(0.0, 1.0),
                        (base[2] + (acc[2] - base[2]) * strength).clamp(0.0, 1.0),
                    ];
                    let out_c = mix3(base, conv, mix);
                    p[0] = out_c[0].clamp(0.0, 1.0) * a;
                    p[1] = out_c[1].clamp(0.0, 1.0) * a;
                    p[2] = out_c[2].clamp(0.0, 1.0) * a;
                }
            });
        }
        FxKind::Deform => {
            let bend_x = g(v, 0, 0.0).clamp(-1.0, 1.0);
            let bend_y = g(v, 1, 0.0).clamp(-1.0, 1.0);
            let shear_x = g(v, 2, 0.0).clamp(-1.0, 1.0);
            let shear_y = g(v, 3, 0.0).clamp(-1.0, 1.0);
            let sq_x = g(v, 4, 0.0).clamp(-0.8, 0.8);
            let sq_y = g(v, 5, 0.0).clamp(-0.8, 0.8);
            remap(f, |x, y| {
                let nx = (x / wf) * 2.0 - 1.0;
                let ny = (y / hf) * 2.0 - 1.0;
                let ux = nx + bend_x * (ny * ny - 0.5) * 0.6 + shear_x * ny * 0.5;
                let uy = ny + bend_y * (nx * nx - 0.5) * 0.6 + shear_y * nx * 0.5;
                let sx = ((ux / (1.0 - sq_x * 0.5).max(0.2)) * 0.5 + 0.5) * wf;
                let sy = ((uy / (1.0 - sq_y * 0.5).max(0.2)) * 0.5 + 0.5) * hf;
                if sx >= 0.0 && sy >= 0.0 && sx < wf && sy < hf {
                    Some((sx, sy))
                } else {
                    None
                }
            });
        }
        FxKind::Denoise => {
            let amount = g(v, 0, 0.4).clamp(0.0, 1.0);
            let r = (g(v, 1, 2.0).round() as isize).clamp(1, 5);
            let sigma_r = g(v, 2, 0.12).clamp(0.02, 0.5);
            let inv_2sr2 = 1.0 / (2.0 * sigma_r * sigma_r);
            if amount <= 1e-4 {
                return true;
            }
            let src = f.clone();
            f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                for (x, p) in row.iter_mut().enumerate() {
                    let c0 = *p;
                    let mut sum = [0.0_f32; 4];
                    let mut wsum = 0.0_f32;
                    for dy in -r..=r {
                        for dx in -r..=r {
                            let q = src.get_clamped(x as isize + dx, y as isize + dy);
                            let dr = q[0] - c0[0];
                            let dg = q[1] - c0[1];
                            let db = q[2] - c0[2];
                            let d2 = dr * dr + dg * dg + db * db;
                            let wt = (-d2 * inv_2sr2).exp();
                            for k in 0..4 {
                                sum[k] += q[k] * wt;
                            }
                            wsum += wt;
                        }
                    }
                    if wsum > 1e-6 {
                        let inv = 1.0 / wsum;
                        let out_a = (c0[3] + (sum[3] * inv - c0[3]) * amount).clamp(0.0, 1.0);
                        p[3] = out_a;
                        for k in 0..3 {
                            let v_k = c0[k] + (sum[k] * inv - c0[k]) * amount;
                            p[k] = v_k.clamp(0.0, out_a);
                        }
                    }
                }
            });
        }
        FxKind::QuickBlur => {
            let rx = ((g(v, 0, 1.5).clamp(0.0, 10.0) / 100.0) * wf).round() as usize;
            let ry = ((g(v, 1, 1.5).clamp(0.0, 10.0) / 100.0) * hf).round() as usize;
            box_blur_frame(f, rx.min(48), ry.min(48));
        }
        FxKind::RadialPixelate => {
            let sectors = g(v, 0, 24.0).clamp(4.0, 128.0);
            let rings = g(v, 1, 16.0).clamp(4.0, 128.0);
            let cx = g(v, 2, 0.5) * wf;
            let cy = g(v, 3, 0.5) * hf;
            let max_r = wf.hypot(hf) * 0.5;
            remap(f, |x, y| {
                let dx = x - cx;
                let dy = y - cy;
                let rad = dx.hypot(dy);
                let ang = dy.atan2(dx);
                let q_r = (((rad / max_r.max(1.0)) * rings).floor() + 0.5) / rings * max_r;
                let step_a = std::f32::consts::TAU / sectors;
                let q_a = ((ang / step_a).floor() + 0.5) * step_a;
                let sx = (cx + q_r * q_a.cos()).clamp(0.5, (wf - 0.5).max(0.5));
                let sy = (cy + q_r * q_a.sin()).clamp(0.5, (hf - 0.5).max(0.5));
                Some((sx, sy))
            });
        }
        FxKind::MaskGenerator => {
            let ch = g(v, 0, 0.0).round() as i32;
            let lo = g(v, 1, 0.2).clamp(0.0, 1.0);
            let hi = g(v, 2, 0.8).clamp(0.0, 1.0);
            let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
            let inv = g(v, 3, 0.0) > 0.5;
            f.px.par_iter_mut().for_each(|p| {
                let a = p[3];
                if a <= 1e-6 {
                    *p = [0.0; 4];
                    return;
                }
                let inv_a = 1.0 / a;
                let rgb = [p[0] * inv_a, p[1] * inv_a, p[2] * inv_a];
                let val = match ch {
                    1 => rgb[0],
                    2 => rgb[1],
                    3 => rgb[2],
                    _ => luma(rgb),
                };
                let mut m = if (hi - lo).abs() < 1e-4 {
                    if val >= lo {
                        1.0
                    } else {
                        0.0
                    }
                } else {
                    smoothstep(lo, hi, val)
                };
                if inv {
                    m = 1.0 - m;
                }
                let na = (a * m).clamp(0.0, 1.0);
                p[0] = (rgb[0] * na).clamp(0.0, na);
                p[1] = (rgb[1] * na).clamp(0.0, na);
                p[2] = (rgb[2] * na).clamp(0.0, na);
                p[3] = na;
            });
        }
        FxKind::MotionBlur => {
            let ang = g(v, 0, 0.0).to_radians();
            let len_px = (g(v, 1, 2.5).clamp(0.0, 15.0) / 100.0) * wf.max(hf);
            let samples = (g(v, 2, 9.0).round() as usize).clamp(3, 21);
            if len_px <= 0.25 {
                return true;
            }
            let (dx, dy) = (ang.cos() * len_px, ang.sin() * len_px);
            let src = f.clone();
            let inv_s = 1.0 / samples as f32;
            f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                let fy = y as f32 + 0.5;
                for (x, p) in row.iter_mut().enumerate() {
                    let fx = x as f32 + 0.5;
                    let mut acc = [0.0_f32; 4];
                    for s in 0..samples {
                        let u = (s as f32 / (samples - 1) as f32) - 0.5;
                        let q = src.sample(fx + dx * u, fy + dy * u);
                        for k in 0..4 {
                            acc[k] += q[k];
                        }
                    }
                    let a = (acc[3] * inv_s).clamp(0.0, 1.0);
                    p[3] = a;
                    for k in 0..3 {
                        p[k] = (acc[k] * inv_s).clamp(0.0, a);
                    }
                }
            });
        }
        FxKind::TinyPlanet => {
            let zoom = g(v, 0, 1.0).clamp(0.2, 4.0);
            let rot = g(v, 1, 0.0).to_radians();
            let tilt = g(v, 2, 0.0).clamp(-1.0, 1.0);
            let invert_pole = g(v, 3, 0.0) > 0.5;
            let cx = wf * 0.5;
            let cy = hf * 0.5;
            let scale = (wf.min(hf) * 0.5 * zoom).max(1.0);
            remap(f, |x, y| {
                let nx = (x - cx) / scale;
                let ny = (y - cy) / scale;
                let r = nx.hypot(ny);
                // Projeção estereográfica inversa -> coordenadas equiretangulares (lon, lat)
                let mut lat = (2.0 * r.atan() - std::f32::consts::FRAC_PI_2) + tilt * 0.6;
                if invert_pole {
                    lat = -lat;
                }
                let lon = (ny.atan2(nx) + rot).rem_euclid(std::f32::consts::TAU);
                let u = lon / std::f32::consts::TAU;
                let v_coord = (lat / std::f32::consts::PI + 0.5).clamp(0.0, 1.0);
                Some((
                    (u * wf).clamp(0.5, (wf - 0.5).max(0.5)),
                    (v_coord * hf).clamp(0.5, (hf - 0.5).max(0.5)),
                ))
            });
        }
        FxKind::AutoLooks => {
            let style = g(v, 0, 0.0).round() as i32;
            let amount = g(v, 1, 0.75).clamp(0.0, 1.0);
            if amount <= 1e-4 {
                return true;
            }
            map_color(f, |c| {
                let y = luma(c);
                let target = match style {
                    1 => {
                        // Teal & Orange
                        let sh = 1.0 - smoothstep(0.15, 0.65, y);
                        let hi = smoothstep(0.35, 0.85, y);
                        [
                            (c[0] + hi * 0.14 - sh * 0.08).clamp(0.0, 1.0),
                            (c[1] + hi * 0.03 + sh * 0.04).clamp(0.0, 1.0),
                            (c[2] - hi * 0.12 + sh * 0.16).clamp(0.0, 1.0),
                        ]
                    }
                    2 => {
                        // Vintage (pretos levantados + sépia suave)
                        let s = saturate(c, 0.72);
                        [
                            (s[0] * 0.94 + 0.06).clamp(0.0, 1.0),
                            (s[1] * 0.90 + 0.04).clamp(0.0, 1.0),
                            (s[2] * 0.82 + 0.05).clamp(0.0, 1.0),
                        ]
                    }
                    3 => {
                        // Noir (P&B alto contraste)
                        let g_v = ((y - 0.5) * 1.45 + 0.5).clamp(0.0, 1.0);
                        [g_v, g_v, g_v]
                    }
                    4 => {
                        // Bleach Bypass
                        let desat = saturate(c, 0.45);
                        [
                            ((desat[0] - 0.5) * 1.35 + 0.5).clamp(0.0, 1.0),
                            ((desat[1] - 0.5) * 1.35 + 0.5).clamp(0.0, 1.0),
                            ((desat[2] - 0.5) * 1.35 + 0.5).clamp(0.0, 1.0),
                        ]
                    }
                    5 => {
                        // Pôr do Sol quente
                        [
                            (c[0] * 1.14 + 0.04).clamp(0.0, 1.0),
                            (c[1] * 1.02).clamp(0.0, 1.0),
                            (c[2] * 0.84).clamp(0.0, 1.0),
                        ]
                    }
                    _ => {
                        // Cinema (curva S suave + leve resfriamento nas sombras)
                        let s_curve = |x: f32| {
                            let x = x.clamp(0.0, 1.0);
                            let sc = x * x * (3.0 - 2.0 * x);
                            x * 0.55 + sc * 0.45
                        };
                        let sh = 1.0 - smoothstep(0.0, 0.5, y);
                        [
                            (s_curve(c[0]) * 1.03).clamp(0.0, 1.0),
                            s_curve(c[1]),
                            (s_curve(c[2]) + sh * 0.05).clamp(0.0, 1.0),
                        ]
                    }
                };
                mix3(c, target, amount)
            });
        }
        _ => return false,
    }
    true
}

fn rgb_to_hsl_local(c: [f32; 3]) -> (f32, f32, f32) {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    let l = 0.5 * (max + min);
    let d = max - min;
    if d <= 1e-6 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-6);
    let mut h = if (max - c[0]).abs() <= 1e-6 {
        ((c[1] - c[2]) / d).rem_euclid(6.0)
    } else if (max - c[1]).abs() <= 1e-6 {
        (c[2] - c[0]) / d + 2.0
    } else {
        (c[0] - c[1]) / d + 4.0
    };
    h /= 6.0;
    (h.rem_euclid(1.0), s.clamp(0.0, 1.0), l.clamp(0.0, 1.0))
}

fn hsl_to_rgb_local(h: f32, s: f32, l: f32) -> [f32; 3] {
    if s <= 1e-6 {
        return [l, l, l];
    }
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(1.0) * 6.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match hp.floor() as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - 0.5 * c;
    [
        (r1 + m).clamp(0.0, 1.0),
        (g1 + m).clamp(0.0, 1.0),
        (b1 + m).clamp(0.0, 1.0),
    ]
}

fn box_blur_frame(f: &mut Frame, rx: usize, ry: usize) {
    let (w, h) = (f.w, f.h);
    if w == 0 || h == 0 || (rx == 0 && ry == 0) {
        return;
    }
    if rx > 0 {
        let src = f.clone();
        let inv = 1.0 / (2 * rx + 1) as f32;
        f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
            for (x, p) in row.iter_mut().enumerate() {
                let mut acc = [0.0_f32; 4];
                for dx in -(rx as isize)..=(rx as isize) {
                    let q = src.get_clamped(x as isize + dx, y as isize);
                    for k in 0..4 {
                        acc[k] += q[k];
                    }
                }
                let a = (acc[3] * inv).clamp(0.0, 1.0);
                p[3] = a;
                for k in 0..3 {
                    p[k] = (acc[k] * inv).clamp(0.0, a);
                }
            }
        });
    }
    if ry > 0 {
        let src = f.clone();
        let inv = 1.0 / (2 * ry + 1) as f32;
        f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
            for (x, p) in row.iter_mut().enumerate() {
                let mut acc = [0.0_f32; 4];
                for dy in -(ry as isize)..=(ry as isize) {
                    let q = src.get_clamped(x as isize, y as isize + dy);
                    for k in 0..4 {
                        acc[k] += q[k];
                    }
                }
                let a = (acc[3] * inv).clamp(0.0, 1.0);
                p[3] = a;
                for k in 0..3 {
                    p[k] = (acc[k] * inv).clamp(0.0, a);
                }
            }
        });
    }
}

/// `true` para os efeitos tratados por este módulo.
pub(crate) fn is_extra(kind: FxKind) -> bool {
    FxKind::VIDEO_EXTRA.contains(&kind)
}

/// Como `map_color`, mas a função recebe também a posição do pixel.
fn map_frame_xy<F>(f: &mut Frame, func: F)
where
    F: Fn(f32, f32, [f32; 3]) -> [f32; 3] + Sync,
{
    let w = f.w;
    if w == 0 {
        return;
    }
    f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, p) in row.iter_mut().enumerate() {
            let a = p[3];
            if a <= 1e-6 {
                continue;
            }
            let inv = 1.0 / a;
            let c = func(
                x as f32 + 0.5,
                y as f32 + 0.5,
                [p[0] * inv, p[1] * inv, p[2] * inv],
            );
            for k in 0..3 {
                p[k] = c[k].clamp(0.0, 1.0) * a;
            }
        }
    });
}

/// Retângulo do conteúdo (exclui as faixas quase pretas ou transparentes nas
/// bordas). `None` se o quadro inteiro estiver vazio.
fn content_box(f: &Frame, thr: f32) -> Option<(usize, usize, usize, usize)> {
    let (w, h) = (f.w, f.h);
    let filled = |p: &[f32; 4]| p[3] > 0.02 && luma([p[0], p[1], p[2]]) > thr * p[3];
    let row_has = |y: usize| {
        f.px.get(y * w..(y + 1) * w)
            .is_some_and(|r| r.iter().any(filled))
    };
    let col_has = |x: usize| (0..h).any(|y| f.px.get(y * w + x).is_some_and(filled));
    let y0 = (0..h).find(|&y| row_has(y))?;
    let y1 = (0..h).rev().find(|&y| row_has(y))? + 1;
    let x0 = (0..w).find(|&x| col_has(x))?;
    let x1 = (0..w).rev().find(|&x| col_has(x))? + 1;
    Some((x0, y0, x1, y1))
}

/// Desenha o quadro reduzido (imagem na imagem) com borda branca opcional.
/// `local(x, y)` devolve (lx, ly, meia-largura, meia-altura, escala).
fn remap_with_border<F>(f: &mut Frame, border: f32, local: F)
where
    F: Fn(f32, f32) -> (f32, f32, f32, f32, f32) + Sync,
{
    let w = f.w;
    let (wf, hf) = (f.w as f32, f.h as f32);
    let src = f.clone();
    f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, p) in row.iter_mut().enumerate() {
            let (lx, ly, hw, hh, sc) = local(x as f32 + 0.5, y as f32 + 0.5);
            if lx.abs() <= hw && ly.abs() <= hh {
                *p = src.sample(wf * 0.5 + lx / sc, hf * 0.5 + ly / sc);
            } else if border > 0.0 && lx.abs() <= hw + border && ly.abs() <= hh + border {
                *p = [1.0; 4];
            } else {
                *p = [0.0; 4];
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FxInstance;

    fn pattern(w: usize, h: usize) -> Frame {
        let mut f = Frame::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let c = [x as f32 / w as f32, y as f32 / h as f32, 0.5, 1.0];
                if let Some(p) = f.px.get_mut(y * w + x) {
                    *p = c;
                }
            }
        }
        f
    }

    /// Todo efeito novo roda com os padrões, com os extremos e num quadro
    /// vazio sem entrar em pânico e sem produzir valores inválidos.
    #[test]
    fn efeitos_novos_sao_robustos() {
        for kind in FxKind::VIDEO_EXTRA {
            let specs = kind.params();
            let sets: Vec<Vec<f32>> = vec![
                specs.iter().map(|s| s.default).collect(),
                specs.iter().map(|s| s.min).collect(),
                specs.iter().map(|s| s.max).collect(),
            ];
            for vals in &sets {
                let mut f = pattern(24, 16);
                assert!(apply(kind, vals, &mut f, Ticks(0)), "{kind:?}");
                for p in &f.px {
                    for c in p {
                        assert!(
                            c.is_finite() && (-1e-4..=1.0001).contains(c),
                            "{kind:?} {c}"
                        );
                    }
                    assert!(p[0] <= p[3] + 1e-3 && p[1] <= p[3] + 1e-3 && p[2] <= p[3] + 1e-3);
                }
                let mut empty = Frame::new(0, 0);
                assert!(apply(kind, vals, &mut empty, Ticks(0)));
                let mut one = Frame::filled(1, 1, [0.5, 0.5, 0.5, 1.0]);
                apply(kind, vals, &mut one, Ticks(i64::MAX));
            }
            // vetor de valores truncado (projeto hostil) não trava
            let mut f = pattern(8, 8);
            apply(kind, &[], &mut f, Ticks(0));
            // pelo caminho público também
            let fx = FxInstance::new(kind);
            let mut f = pattern(8, 8);
            super::super::vfx::apply(&fx, &mut f, Ticks(0), crate::time::FrameRate::FPS_30);
        }
    }

    #[test]
    fn limiar_binariza_e_balanco_de_branco_neutraliza() {
        let mut f = pattern(16, 8);
        apply(FxKind::Threshold, &[0.5, 0.0], &mut f, Ticks(0));
        assert!(f.px.iter().all(|p| p[0] < 1e-3 || p[0] > 0.999));

        // um cinza azulado escolhido como neutro vira cinza
        let mut f = Frame::filled(2, 2, [0.4, 0.5, 0.8, 1.0]);
        apply(
            FxKind::WhiteBalance,
            &[0.4, 0.5, 0.8, 1.0],
            &mut f,
            Ticks(0),
        );
        let p = f.px[0];
        assert!(
            (p[0] - p[1]).abs() < 0.02 && (p[1] - p[2]).abs() < 0.02,
            "{p:?}"
        );
    }

    #[test]
    fn deslocar_com_repeticao_preserva_pixels() {
        let mut f = pattern(10, 10);
        let orig = f.clone();
        apply(FxKind::OffsetWrap, &[50.0, 0.0, 1.0], &mut f, Ticks(0));
        // deslocar meia largura: o pixel 0 recebe o da coluna 5
        let a = f.px[0];
        let b = orig.px[5];
        assert!((a[0] - b[0]).abs() < 0.02, "{a:?} vs {b:?}");
    }

    #[test]
    fn preenchimento_de_barras_cobre_as_faixas_pretas() {
        // conteúdo vermelho no meio, faixas pretas à esquerda e à direita
        let (w, h) = (40, 20);
        let mut f = Frame::filled(w, h, [0.0, 0.0, 0.0, 1.0]);
        for y in 0..h {
            for x in 10..30 {
                f.px[y * w + x] = [0.9, 0.1, 0.1, 1.0];
            }
        }
        apply(FxKind::BlackBarFill, &[1.0, 1.0, 0.03], &mut f, Ticks(0));
        assert!(f.px[w * 10 + 2][0] > 0.3, "a faixa preta foi preenchida");
        assert!(
            (f.px[w * 10 + 20][0] - 0.9).abs() < 1e-3,
            "o conteúdo fica igual"
        );
    }

    #[test]
    fn cortador_mantem_centro_e_remove_cantos() {
        let mut f = Frame::filled(20, 20, [1.0; 4]);
        apply(
            FxKind::CookieCutter,
            &[0.0, 0.5, 0.5, 0.5, 0.01, 0.0],
            &mut f,
            Ticks(0),
        );
        assert!(f.px[10 * 20 + 10][3] > 0.99);
        assert!(f.px[0][3] < 0.01);
    }
}
