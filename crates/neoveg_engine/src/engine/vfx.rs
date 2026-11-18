//! Efeitos de vídeo (Video FX).

// Laços indexam vários buffers paralelos (canais/amostras) ao mesmo tempo;
// iteradores encadeados deixariam o DSP menos legível.
#![allow(clippy::needless_range_loop)]

use super::{luma, smoothstep, Frame};
use crate::model::{ColorGradeCurves, FrameRate, FxInstance, FxKind};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

fn load_cached_cube(path: &str) -> Option<Arc<crate::encoder::effects::CubeLut>> {
    static CUBE_CACHE: OnceLock<
        Mutex<HashMap<PathBuf, Option<Arc<crate::encoder::effects::CubeLut>>>>,
    > = OnceLock::new();
    let cache = CUBE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = PathBuf::from(path);
    {
        let g = cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(found) = g.get(&key) {
            return found.clone();
        }
    }
    let loaded = crate::encoder::effects::CubeLut::load_file(Path::new(path))
        .ok()
        .map(Arc::new);
    let mut g = cache.lock().unwrap_or_else(PoisonError::into_inner);
    if g.len() >= 32 {
        g.clear();
    }
    g.insert(key, loaded.clone());
    loaded
}

fn rgb_to_hsl(c: [f32; 3]) -> (f32, f32, f32) {
    let r = c[0].clamp(0.0, 1.0);
    let g = c[1].clamp(0.0, 1.0);
    let b = c[2].clamp(0.0, 1.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) * 0.5;
    let d = max - min;
    if d <= 1e-5 {
        return (0.0, 0.0, l);
    }
    let s = if l > 0.5 {
        d / (2.0 - max - min).max(1e-5)
    } else {
        d / (max + min).max(1e-5)
    };
    let mut h = if (max - r).abs() < 1e-6 {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if (max - g).abs() < 1e-6 {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h /= 6.0;
    (h.rem_euclid(1.0), s.clamp(0.0, 1.0), l.clamp(0.0, 1.0))
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    if s <= 1e-5 {
        return [l, l, l];
    }
    let hue2rgb = |p: f32, q: f32, mut t: f32| -> f32 {
        t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    [
        hue2rgb(p, q, h + 1.0 / 3.0),
        hue2rgb(p, q, h),
        hue2rgb(p, q, h - 1.0 / 3.0),
    ]
}

fn eval_builtin_lut(kind: &str, rgb: [f32; 3]) -> [f32; 3] {
    let s_curve = |v: f32, strg: f32| -> f32 {
        let x = v.clamp(0.0, 1.0);
        let s = x * x * (3.0 - 2.0 * x);
        x + (s - x) * strg
    };
    match kind {
        "builtin:rec709" => saturate(
            [
                s_curve((rgb[0] * 1.12 - 0.06).clamp(0.0, 1.0), 0.75),
                s_curve((rgb[1] * 1.12 - 0.06).clamp(0.0, 1.0), 0.75),
                s_curve((rgb[2] * 1.12 - 0.06).clamp(0.0, 1.0), 0.75),
            ],
            1.15,
        ),
        "builtin:teal_orange" => {
            let l = luma(rgb);
            let hi = smoothstep(0.35, 0.85, l);
            let sh = 1.0 - hi;
            [
                s_curve((rgb[0] + hi * 0.08 - sh * 0.07).clamp(0.0, 1.0), 0.45),
                s_curve((rgb[1] + hi * 0.02 + sh * 0.02).clamp(0.0, 1.0), 0.45),
                s_curve((rgb[2] - hi * 0.09 + sh * 0.08).clamp(0.0, 1.0), 0.45),
            ]
        }
        "builtin:film_cool" => [
            (s_curve(rgb[0], 0.35) * 0.94 + 0.02).clamp(0.0, 1.0),
            (s_curve(rgb[1], 0.35) * 0.98 + 0.03).clamp(0.0, 1.0),
            (s_curve(rgb[2], 0.35) * 1.04 + 0.05).clamp(0.0, 1.0),
        ],
        "builtin:mono_film" => {
            let y = s_curve(
                (0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2]).clamp(0.0, 1.0),
                0.65,
            );
            [y, y, y]
        }
        _ => rgb,
    }
}

fn apply_color_grade_curves(
    mut c: [f32; 3],
    cg: &ColorGradeCurves,
    lut: Option<&crate::encoder::effects::CubeLut>,
) -> [f32; 3] {
    if !cg.master.is_empty() {
        c[0] = ColorGradeCurves::eval_curve(&cg.master, c[0]);
        c[1] = ColorGradeCurves::eval_curve(&cg.master, c[1]);
        c[2] = ColorGradeCurves::eval_curve(&cg.master, c[2]);
    }
    if !cg.red.is_empty() {
        c[0] = ColorGradeCurves::eval_curve(&cg.red, c[0]);
    }
    if !cg.green.is_empty() {
        c[1] = ColorGradeCurves::eval_curve(&cg.green, c[1]);
    }
    if !cg.blue.is_empty() {
        c[2] = ColorGradeCurves::eval_curve(&cg.blue, c[2]);
    }
    if !cg.hsl_bands.is_empty() {
        let (mut h, mut s, mut l) = rgb_to_hsl(c);
        if s > 1e-3 {
            let mut dh = 0.0f32;
            let mut ds = 0.0f32;
            let mut dl = 0.0f32;
            for (bi, b) in cg.hsl_bands.iter().take(6).enumerate() {
                let center = bi as f32 / 6.0;
                let mut dist = (h - center).abs();
                if dist > 0.5 {
                    dist = 1.0 - dist;
                }
                let w = (1.0 - dist * 6.0).max(0.0);
                if w > 0.0 {
                    dh += b[0] * w;
                    ds += b[1] * w;
                    dl += b[2] * w;
                }
            }
            if dh.abs() > 1e-4 || ds.abs() > 1e-4 || dl.abs() > 1e-4 {
                h = (h + dh).rem_euclid(1.0);
                s = (s * (1.0 + ds)).clamp(0.0, 1.0);
                l = (l + dl * 0.5).clamp(0.0, 1.0);
                c = hsl_to_rgb(h, s, l);
            }
        }
    }
    let strg = if cg.lut_strength.is_finite() {
        cg.lut_strength.clamp(0.0, 1.0)
    } else {
        1.0
    };
    if strg > 1e-4 {
        if let Some(lut_ref) = lut {
            let mapped = lut_ref.sample(c);
            c = mix3(c, mapped, strg);
        } else if let Some(path) = cg.lut_path.as_deref() {
            if path.starts_with("builtin:") {
                let mapped = eval_builtin_lut(path, c);
                c = mix3(c, mapped, strg);
            }
        }
    }
    [
        c[0].clamp(0.0, 1.0),
        c[1].clamp(0.0, 1.0),
        c[2].clamp(0.0, 1.0),
    ]
}

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

/// Aplica um efeito OFX pelo host isolado. Em erro o quadro fica intacto e a
/// falha vai uma vez para o log (por efeito), sem travar a renderização.
fn apply_ofx(fx: &FxInstance, f: &mut Frame, t: crate::model::Ticks, fps: FrameRate) {
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
    let (time, rate) = ofx_time(t, fps);
    let mut px: Vec<f32> = f.px.iter().flatten().copied().collect();
    match crate::plugin_host::ofx_render(&host, r, w, h, time, rate, &mut px) {
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

/// Tempo local em quadros OFX e a taxa do projeto. A conta é exata em
/// inteiros; só o resultado vira `f64`, que é o tipo da API do OFX.
fn ofx_time(t: crate::model::Ticks, fps: FrameRate) -> (f64, f64) {
    let (num, den) = (i128::from(fps.num.max(1)), i128::from(fps.den.max(1)));
    let tps = i128::from(crate::model::TICKS_PER_SEC);
    let scaled = i128::from(t.0).saturating_mul(num);
    let whole = scaled / (tps * den);
    let rem = scaled % (tps * den);
    let frames = whole as f64 + rem as f64 / (tps * den) as f64;
    (frames, num as f64 / den as f64)
}

/// Aplica um efeito de vídeo. `t` é o tempo local (em ticks) usado para os
/// keyframes dos parâmetros; `fps` é a taxa do projeto (tempo dos efeitos
/// OFX).
pub fn apply(fx: &FxInstance, f: &mut Frame, t: crate::model::Ticks, fps: FrameRate) {
    if !fx.enabled || fx.kind.is_audio() {
        return;
    }
    if fx.kind == FxKind::Ofx {
        apply_ofx(fx, f, t, fps);
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
            let cg = fx.color_grade.as_deref();
            let cube = cg
                .and_then(|g| g.lut_path.as_deref())
                .filter(|p| !p.starts_with("builtin:"))
                .and_then(load_cached_cube);
            map_color(f, |x| {
                let mut c = [0.0; 3];
                for k in 0..3 {
                    let y = gain * (x[k] + lift * (1.0 - x[k]));
                    c[k] = y.max(0.0).powf(ig);
                }
                c[0] += temp * 0.1;
                c[2] -= temp * 0.1;
                c[1] += tint * 0.1;
                c = saturate(c, sat);
                if let Some(g) = cg {
                    c = apply_color_grade_curves(c, g, cube.as_deref());
                }
                c
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
        FxKind::Stabilization => {
            // Sem vizinhança temporal (ex.: prévia isolada de efeito), aplica apenas o zoom de borda.
            let border_zoom = v.get(1).copied().unwrap_or(1.12);
            apply_stabilization_transform(f, 0.0, 0.0, 0.0, border_zoom);
        }
        _ => {}
    }
}

/// Aplica transformação afim de estabilização (deslocamento + micro-rotação + zoom de borda)
/// mantendo as amostras dentro dos limites válidos do quadro sem bordas pretas.
pub fn apply_stabilization_transform(
    f: &mut Frame,
    dx_px: f32,
    dy_px: f32,
    angle_rad: f32,
    border_zoom: f32,
) {
    let (w, h) = (f.w, f.h);
    if w == 0 || h == 0 {
        return;
    }
    let fin = |val: f32, fallback: f32| if val.is_finite() { val } else { fallback };
    let zoom = fin(border_zoom, 1.12).clamp(1.0, 1.5);
    let max_shift_x = (w as f32) * 0.25;
    let max_shift_y = (h as f32) * 0.25;
    let dx = fin(dx_px, 0.0).clamp(-max_shift_x, max_shift_x);
    let dy = fin(dy_px, 0.0).clamp(-max_shift_y, max_shift_y);
    let ang = fin(angle_rad, 0.0).clamp(-0.35, 0.35);
    if (zoom - 1.0).abs() < 1e-4 && dx.abs() < 1e-4 && dy.abs() < 1e-4 && ang.abs() < 1e-4 {
        return;
    }
    let src = f.clone();
    let cx = (w as f32) * 0.5;
    let cy = (h as f32) * 0.5;
    let inv_zoom = 1.0 / zoom;
    let (sn, cs) = ang.sin_cos();
    let max_sx = (w as f32 - 0.5).max(0.5);
    let max_sy = (h as f32 - 0.5).max(0.5);
    f.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let oy = (y as f32 + 0.5 - cy) * inv_zoom;
        for (x, p) in row.iter_mut().enumerate() {
            let ox = (x as f32 + 0.5 - cx) * inv_zoom;
            let sx = (ox * cs - oy * sn + cx - dx).clamp(0.5, max_sx);
            let sy = (ox * sn + oy * cs + cy - dy).clamp(0.5, max_sy);
            *p = src.sample(sx, sy);
        }
    });
}

/// Estima movimento global `(dx_norm, dy_norm, angle_rad)` de `cur` em relação a `prev`
/// usando busca de blocos 8x8 em luminância BT.601 em grade 3x3 (Sala Limpa).
pub fn estimate_global_motion(prev: &image::RgbaImage, cur: &image::RgbaImage) -> (f32, f32, f32) {
    let (w, h) = (prev.width() as usize, prev.height() as usize);
    if w < 24 || h < 24 || w != cur.width() as usize || h != cur.height() as usize {
        return (0.0, 0.0, 0.0);
    }
    let pa = prev.as_raw();
    let pb = cur.as_raw();
    if pa.len() < w * h * 4 || pb.len() < w * h * 4 {
        return (0.0, 0.0, 0.0);
    }
    let luma_at = |buf: &[u8], x: usize, y: usize| -> i32 {
        let i = (y * w + x) * 4;
        (77 * i32::from(buf[i]) + 150 * i32::from(buf[i + 1]) + 29 * i32::from(buf[i + 2])) >> 8
    };
    let rx = (w / 10).clamp(2, 8) as isize;
    let ry = (h / 10).clamp(2, 8) as isize;
    let bw = 8usize;
    let mut dxs = [0f32; 9];
    let mut dys = [0f32; 9];
    let mut count = 0usize;
    for by in 0..3 {
        let cy0 = ((h * (by + 2)) / 6).min(h - bw - 1);
        for bx in 0..3 {
            let cx0 = ((w * (bx + 2)) / 6).min(w - bw - 1);
            let mut best_sad = u32::MAX;
            let mut best_dx = 0isize;
            let mut best_dy = 0isize;
            for dy in (-ry..=ry).step_by(2) {
                for dx in (-rx..=rx).step_by(2) {
                    let mut sad = 0u32;
                    for j in 0..bw {
                        let py = ((cy0 + j) as isize + dy).clamp(0, h as isize - 1) as usize;
                        for i in 0..bw {
                            let px = ((cx0 + i) as isize + dx).clamp(0, w as isize - 1) as usize;
                            let diff = luma_at(pb, cx0 + i, cy0 + j) - luma_at(pa, px, py);
                            sad += diff.unsigned_abs();
                        }
                    }
                    if sad < best_sad {
                        best_sad = sad;
                        best_dx = dx;
                        best_dy = dy;
                    }
                }
            }
            dxs[count] = best_dx as f32;
            dys[count] = best_dy as f32;
            count += 1;
        }
    }
    if count == 0 {
        return (0.0, 0.0, 0.0);
    }
    dxs.sort_by(|a, b| a.total_cmp(b));
    dys.sort_by(|a, b| a.total_cmp(b));
    let med_dx = dxs[4];
    let med_dy = dys[4];
    (med_dx / w as f32, med_dy / h as f32, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Ticks;

    /// O tempo enviado ao OFX segue a taxa do projeto (antes era 30 fixo).
    #[test]
    fn ofx_time_uses_project_rate_exactly() {
        for fps in [FrameRate::FPS_25, FrameRate::FPS_29_97, FrameRate::FPS_60] {
            for n in [0i64, 1, 100, 107_892] {
                let (frames, rate) = ofx_time(fps.frame_to_ticks(n), fps);
                assert!((frames - n as f64).abs() < 1e-6, "{fps:?} {n}: {frames}");
                assert!((rate - fps.as_f64()).abs() < 1e-12);
            }
        }
        // 1 s a 29,97 fps = 30000/1001 quadros
        let (frames, _) = ofx_time(Ticks::from_secs_i64(1), FrameRate::FPS_29_97);
        assert!((frames - 30000.0 / 1001.0).abs() < 1e-9);
        // entrada hostil: taxa zerada e tempo extremo não travam
        let zero = FrameRate { num: 0, den: 0 };
        assert!(ofx_time(Ticks(i64::MAX), zero).0.is_finite());
        assert!(ofx_time(Ticks(i64::MIN), FrameRate::FPS_60).0.is_finite());
    }

    #[test]
    fn estabilizador_reduz_tremor_e_protege_bordas_sem_panico() {
        let mut cur = image::RgbaImage::new(64, 36);
        let mut nbr = image::RgbaImage::new(64, 36);
        for y in 0..36u32 {
            for x in 0..64u32 {
                let block = |px: i32, py: i32| {
                    if (20..36).contains(&px) && (12..24).contains(&py) {
                        image::Rgba([240, 220, 180, 255])
                    } else {
                        image::Rgba([30, 40, 50, 255])
                    }
                };
                cur.put_pixel(x, y, block(x as i32, y as i32));
                nbr.put_pixel(x, y, block(x as i32 - 4, y as i32));
            }
        }
        let (dx, dy, _) = estimate_global_motion(&nbr, &cur);
        assert!(dx.is_finite() && dy.is_finite());
        assert!(dx < 0.0, "deve detectar deslocamento relativo dx={dx}");

        let mut frame = Frame::filled(32, 18, [0.5, 0.4, 0.3, 1.0]);
        apply_stabilization_transform(&mut frame, 3.0, -2.0, 0.02, 1.15);
        assert!(frame
            .px
            .iter()
            .all(|p| p.iter().all(|c| c.is_finite() && *c > 0.0)));
        // Entradas extremas/NaN não entram em pânico
        apply_stabilization_transform(&mut frame, f32::NAN, f32::INFINITY, -99.0, 0.0);
    }

    #[test]
    fn color_grading_curvas_rgb_hsl_e_lut_transformam_quadro_e_ignoram_entrada_hostil() {
        let mut fx = FxInstance::new(FxKind::ColorCorrector);
        let mut hsl = crate::model::ColorGradeCurves::default_hsl_bands();
        hsl[0] = [0.25, -0.5, 0.2];
        let cg = crate::model::ColorGradeCurves {
            red: vec![[0.0, 0.25], [1.0, 1.0]],
            hsl_bands: hsl,
            lut_path: Some("builtin:mono_film".into()),
            lut_strength: 1.0,
            ..Default::default()
        };
        fx.color_grade = Some(Box::new(cg));

        let mut frame = Frame::filled(4, 4, [0.8, 0.2, 0.1, 1.0]);
        apply(&fx, &mut frame, Ticks::ZERO, FrameRate::FPS_30);
        let p = frame.px[0];
        assert!(p.iter().all(|c| c.is_finite()));
        // Mono LUT produz cinza equilibrado com forte proximidade entre R, G e B.
        assert!((p[0] - p[1]).abs() < 1e-4 && (p[1] - p[2]).abs() < 1e-4);

        // Arquivo .cube 3D 2x2x2 que troca vermelho <-> azul.
        let tmp = std::env::temp_dir().join(format!("neoveg_cg_{}.cube", crate::model::new_id()));
        let cube = "LUT_3D_SIZE 2\n0 0 0\n0 0 1\n0 1 0\n0 1 1\n1 0 0\n1 0 1\n1 1 0\n1 1 1\n";
        std::fs::write(&tmp, cube).expect("write cube");
        let mut fx_cube = FxInstance::new(FxKind::ColorCorrector);
        let cg2 = crate::model::ColorGradeCurves {
            lut_path: Some(tmp.display().to_string()),
            lut_strength: 1.0,
            ..Default::default()
        };
        fx_cube.color_grade = Some(Box::new(cg2));
        let mut f2 = Frame::filled(2, 2, [0.9, 0.1, 0.0, 1.0]);
        apply(&fx_cube, &mut f2, Ticks::ZERO, FrameRate::FPS_30);
        assert!(
            f2.px[0][2] > f2.px[0][0],
            "LUT deve inverter R e B: {:?}",
            f2.px[0]
        );
        let _ = std::fs::remove_file(&tmp);

        // Entrada hostil: caminho inexistente + NaN não geram pânico.
        let mut fx_bad = FxInstance::new(FxKind::ColorCorrector);
        let cg_bad = crate::model::ColorGradeCurves {
            lut_path: Some("/caminho/inexistente/nao_existe.cube".into()),
            lut_strength: f32::NAN,
            master: vec![[f32::NAN, f32::INFINITY]],
            ..Default::default()
        };
        fx_bad.color_grade = Some(Box::new(cg_bad));
        let mut f_bad = Frame::filled(2, 2, [0.5, 0.4, 0.3, 1.0]);
        apply(&fx_bad, &mut f_bad, Ticks::ZERO, FrameRate::FPS_30);
        assert!(f_bad.px[0].iter().all(|c| c.is_finite()));
    }
}
