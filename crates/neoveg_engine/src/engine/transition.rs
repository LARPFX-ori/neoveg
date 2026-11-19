//! Transições entre eventos sobrepostos na mesma trilha.

// Laços indexam vários buffers paralelos (canais/amostras) ao mesmo tempo;
// iteradores encadeados deixariam o DSP menos legível.
#![allow(clippy::needless_range_loop)]

use super::{smoothstep, Frame};
use crate::model::TransitionKind;
use rayon::prelude::*;
use std::f32::consts::PI;

/// Estimativa de fluxo óptico por blocos (Sala Limpa) e deformação bidirecional
/// para a transição Warp Flow (usada também no Smart Split).
fn warp_flow(a: &Frame, b: &Frame, p: f32) -> Frame {
    let (w, h) = (a.w, a.h);
    let mut out = Frame::new(w, h);
    if w == 0 || h == 0 {
        return out;
    }
    if w < 16 || h < 16 || a.px.len() != w * h || b.px.len() != w * h {
        out.px.par_iter_mut().enumerate().for_each(|(i, o)| {
            for k in 0..4 {
                o[k] = a.px[i][k] * (1.0 - p) + b.px[i][k] * p;
            }
        });
        return out;
    }
    let luma = |px: [f32; 4]| 0.2126 * px[0] + 0.7152 * px[1] + 0.0722 * px[2];
    const BLK: usize = 8;
    const SEARCH: isize = 6;
    let bw = w.div_ceil(BLK);
    let bh = h.div_ceil(BLK);
    // Calcula vetor de deslocamento (dx, dy) de B em relação a A por bloco 8x8.
    let flow: Vec<(f32, f32)> = (0..bw * bh)
        .into_par_iter()
        .map(|bi| {
            let bx = (bi % bw) * BLK;
            let by = (bi / bw) * BLK;
            let mut best_sad = f32::MAX;
            let mut best = (0isize, 0isize);
            for dy in -SEARCH..=SEARCH {
                for dx in -SEARCH..=SEARCH {
                    let mut sad = 0.0f32;
                    for sy in (0..BLK).step_by(2) {
                        let ay = (by + sy).min(h - 1) as isize;
                        let by2 = (ay + dy).clamp(0, h as isize - 1);
                        for sx in (0..BLK).step_by(2) {
                            let ax = (bx + sx).min(w - 1) as isize;
                            let bx2 = (ax + dx).clamp(0, w as isize - 1);
                            let la = luma(a.get_clamped(ax, ay));
                            let lb = luma(b.get_clamped(bx2, by2));
                            sad += (la - lb).abs();
                        }
                    }
                    // Leve regularização favorece deslocamentos menores quando o bloco é liso.
                    sad += ((dx.abs() + dy.abs()) as f32) * 0.015;
                    if sad < best_sad {
                        best_sad = sad;
                        best = (dx, dy);
                    }
                }
            }
            (best.0 as f32, best.1 as f32)
        })
        .collect();

    let w_max = w.saturating_sub(1) as f32;
    let h_max = h.saturating_sub(1) as f32;
    out.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let fy = ((y as f32 + 0.5) / BLK as f32 - 0.5).clamp(0.0, bh.saturating_sub(1) as f32);
        let y0 = (fy.floor() as usize).min(bh - 1);
        let y1 = (y0 + 1).min(bh - 1);
        let ty = fy - y0 as f32;
        for (x, o) in row.iter_mut().enumerate() {
            let fx = ((x as f32 + 0.5) / BLK as f32 - 0.5).clamp(0.0, bw.saturating_sub(1) as f32);
            let x0 = (fx.floor() as usize).min(bw - 1);
            let x1 = (x0 + 1).min(bw - 1);
            let tx = fx - x0 as f32;
            let v00 = flow.get(y0 * bw + x0).copied().unwrap_or((0.0, 0.0));
            let v10 = flow.get(y0 * bw + x1).copied().unwrap_or((0.0, 0.0));
            let v01 = flow.get(y1 * bw + x0).copied().unwrap_or((0.0, 0.0));
            let v11 = flow.get(y1 * bw + x1).copied().unwrap_or((0.0, 0.0));
            let dx0 = v00.0 + (v10.0 - v00.0) * tx;
            let dx1 = v01.0 + (v11.0 - v01.0) * tx;
            let dy0 = v00.1 + (v10.1 - v00.1) * tx;
            let dy1 = v01.1 + (v11.1 - v01.1) * tx;
            let dx = dx0 + (dx1 - dx0) * ty;
            let dy = dy0 + (dy1 - dy0) * ty;

            let ax = (x as f32 + 0.5 - dx * p).clamp(0.5, w_max + 0.5);
            let ay = (y as f32 + 0.5 - dy * p).clamp(0.5, h_max + 0.5);
            let bx = (x as f32 + 0.5 + dx * (1.0 - p)).clamp(0.5, w_max + 0.5);
            let by = (y as f32 + 0.5 + dy * (1.0 - p)).clamp(0.5, h_max + 0.5);
            let pa = a.sample(ax, ay);
            let pb = b.sample(bx, by);
            for k in 0..4 {
                o[k] = pa[k] * (1.0 - p) + pb[k] * p;
            }
        }
    });
    out
}

/// Combina o quadro de saída `a` com o de entrada `b` no progresso `p` (0..1).
pub fn apply(kind: TransitionKind, softness: f32, a: &Frame, b: &Frame, p: f32) -> Frame {
    let (w, h) = (a.w, a.h);
    let p = if p.is_finite() {
        p.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if w == 0 || h == 0 || a.px.len() != w * h {
        return Frame::new(w.min(a.px.len()), h.min(a.px.len()));
    }
    if b.w != w || b.h != h || b.px.len() != a.px.len() {
        // quadros de tamanhos diferentes: troca seca no meio, sem indexar fora
        return if p < 0.5 { a.clone() } else { b.clone() };
    }
    if kind == TransitionKind::WarpFlow {
        return warp_flow(a, b, p);
    }
    let mut out = Frame::new(w, h);
    match kind {
        TransitionKind::Dissolve => {
            out.px.par_iter_mut().enumerate().for_each(|(i, o)| {
                for k in 0..4 {
                    o[k] = a.px[i][k] * (1.0 - p) + b.px[i][k] * p;
                }
            });
        }
        TransitionKind::FadeThroughBlack => {
            out.px.par_iter_mut().enumerate().for_each(|(i, o)| {
                if p < 0.5 {
                    let k = 1.0 - p * 2.0;
                    let s = a.px[i];
                    *o = [s[0] * k, s[1] * k, s[2] * k, s[3]];
                } else {
                    let k = p * 2.0 - 1.0;
                    let s = b.px[i];
                    // mantém fundo preto opaco onde não há imagem
                    *o = [
                        s[0] * k,
                        s[1] * k,
                        s[2] * k,
                        s[3].max(a.px[i][3] * (1.0 - k)),
                    ];
                }
            });
        }
        TransitionKind::PushLeft | TransitionKind::PushRight | TransitionKind::SlideOver => {
            let shift = (p * w as f32) as isize;
            out.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                for (x, o) in row.iter_mut().enumerate() {
                    let xi = x as isize;
                    *o = match kind {
                        TransitionKind::PushLeft => {
                            let ax = xi + shift;
                            if ax < w as isize {
                                a.get(ax, y as isize)
                            } else {
                                b.get(ax - w as isize, y as isize)
                            }
                        }
                        TransitionKind::PushRight => {
                            let ax = xi - shift;
                            if ax >= 0 {
                                a.get(ax, y as isize)
                            } else {
                                b.get(ax + w as isize, y as isize)
                            }
                        }
                        _ => {
                            // B desliza da direita por cima de A
                            let bx = xi - (w as isize - shift);
                            if bx >= 0 {
                                let s = b.get(bx, y as isize);
                                let d = a.get(xi, y as isize);
                                let ia = 1.0 - s[3];
                                [
                                    s[0] + d[0] * ia,
                                    s[1] + d[1] * ia,
                                    s[2] + d[2] * ia,
                                    s[3] + d[3] * ia,
                                ]
                            } else {
                                a.get(xi, y as isize)
                            }
                        }
                    };
                }
            });
        }
        TransitionKind::Zoom => {
            let s = p.max(1e-3);
            let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
            out.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                for (x, o) in row.iter_mut().enumerate() {
                    let bx = (x as f32 + 0.5 - cx) / s + cx;
                    let by = (y as f32 + 0.5 - cy) / s + cy;
                    let sb = if bx >= 0.0 && by >= 0.0 && bx < w as f32 && by < h as f32 {
                        b.sample(bx, by)
                    } else {
                        [0.0; 4]
                    };
                    let d = a.px[y * w + x];
                    let ia = 1.0 - sb[3];
                    *o = [
                        sb[0] + d[0] * ia,
                        sb[1] + d[1] * ia,
                        sb[2] + d[2] * ia,
                        sb[3] + d[3] * ia,
                    ];
                }
            });
        }
        TransitionKind::PushUp
        | TransitionKind::PushDown
        | TransitionKind::Squeeze
        | TransitionKind::Swap
        | TransitionKind::FlashWhite
        | TransitionKind::FlyIn
        | TransitionKind::PagePeel => geometric(kind, a, b, p, &mut out),
        _ => {
            // transições por máscara: B aparece onde v(x,y) < progresso
            let s = softness.clamp(0.0, 0.5).max(0.002);
            let aspect = w as f32 / h as f32;
            out.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                let ny = (y as f32 + 0.5) / h as f32;
                for (x, o) in row.iter_mut().enumerate() {
                    let nx = (x as f32 + 0.5) / w as f32;
                    let v = match kind {
                        TransitionKind::WipeLeft => 1.0 - nx,
                        TransitionKind::WipeRight => nx,
                        TransitionKind::WipeUp => 1.0 - ny,
                        TransitionKind::WipeDown => ny,
                        TransitionKind::IrisOpen | TransitionKind::IrisClose => {
                            let dx = (nx - 0.5) * aspect;
                            let dy = ny - 0.5;
                            let d =
                                (dx * dx + dy * dy).sqrt() / (0.5 * (aspect * aspect + 1.0).sqrt());
                            if kind == TransitionKind::IrisOpen {
                                d
                            } else {
                                1.0 - d
                            }
                        }
                        TransitionKind::ClockWipe => {
                            let ang = (nx - 0.5).atan2(-(ny - 0.5));
                            (ang + PI) / (2.0 * PI)
                        }
                        TransitionKind::BarnDoor => (nx - 0.5).abs() * 2.0,
                        TransitionKind::Blinds => (ny * 8.0).fract(),
                        TransitionKind::Checker => {
                            let cx = (nx * 8.0).floor() as i32;
                            let cy = (ny * 6.0).floor() as i32;
                            let par = ((cx + cy) & 1) as f32;
                            (par + (nx * 8.0).fract()) * 0.5
                        }
                        TransitionKind::DiagonalWipe => (nx * aspect + ny) / (aspect + 1.0),
                        TransitionKind::SplitHorizontal => (ny - 0.5).abs() * 2.0,
                        TransitionKind::VenetianBlinds => (nx * 10.0).fract(),
                        TransitionKind::StarWipe => {
                            let dx = (nx - 0.5) * aspect;
                            let dy = ny - 0.5;
                            let ang = dy.atan2(dx) + PI / 2.0;
                            let k = 0.55 + 0.45 * (2.5 * ang).cos().abs().powi(3);
                            let half_diag = 0.5 * (aspect * aspect + 1.0).sqrt();
                            ((dx * dx + dy * dy).sqrt() / k / (half_diag / 0.55)).min(1.0)
                        }
                        TransitionKind::Spiral => {
                            let dx = (nx - 0.5) * aspect;
                            let dy = ny - 0.5;
                            let half_diag = 0.5 * (aspect * aspect + 1.0).sqrt();
                            let d = (dx * dx + dy * dy).sqrt() / half_diag;
                            let ang = (dy.atan2(dx) + PI) / (2.0 * PI);
                            (d * 3.0 + ang) / 4.0
                        }
                        TransitionKind::Portals => {
                            let (cx, cy) = ((nx * 6.0).floor(), (ny * 4.0).floor());
                            let delay = (super::vfx::hash(
                                (cx as u32).wrapping_mul(73) ^ (cy as u32).wrapping_mul(9_151),
                            ) & 0xffff) as f32
                                / 65_536.0
                                * 0.5;
                            let (fx, fy) = ((nx * 6.0).fract() - 0.5, (ny * 4.0).fract() - 0.5);
                            let d =
                                ((fx * fx + fy * fy).sqrt() * std::f32::consts::SQRT_2).min(1.0);
                            delay + d * 0.5
                        }
                        TransitionKind::RandomDissolve => {
                            (super::vfx::hash(
                                (x as u32 / 2).wrapping_mul(7_919)
                                    ^ (y as u32 / 2).wrapping_mul(104_729),
                            ) & 0xffff) as f32
                                / 65_536.0
                        }
                        TransitionKind::Cascade => {
                            let (cx, cy) = ((nx * 8.0).floor(), (ny * 6.0).floor());
                            let local = ((nx * 8.0).fract() + (ny * 6.0).fract()) * 0.5;
                            (cx + cy) / 12.0 * 0.7 + local * 0.3
                        }
                        TransitionKind::GradientWipe => {
                            let q = a.px.get(y * w + x).copied().unwrap_or([0.0; 4]);
                            let al = q[3].max(1e-6);
                            super::luma([q[0] / al, q[1] / al, q[2] / al]).clamp(0.0, 1.0)
                        }
                        _ => nx,
                    };
                    let pp = p * (1.0 + 2.0 * s) - s;
                    let m = 1.0 - smoothstep(pp - s, pp + s, v);
                    let (pa, pb) = (a.px[y * w + x], b.px[y * w + x]);
                    for k in 0..4 {
                        o[k] = pa[k] * (1.0 - m) + pb[k] * m;
                    }
                }
            });
        }
    }
    out
}

/// `top` sobre `bottom` (RGBA pré-multiplicado).
#[inline]
fn over(top: [f32; 4], bottom: [f32; 4]) -> [f32; 4] {
    let ia = 1.0 - top[3];
    [
        top[0] + bottom[0] * ia,
        top[1] + bottom[1] * ia,
        top[2] + bottom[2] * ia,
        top[3] + bottom[3] * ia,
    ]
}

#[inline]
fn scaled(px: [f32; 4], k: f32) -> [f32; 4] {
    [px[0] * k, px[1] * k, px[2] * k, px[3] * k]
}

/// Amostra `f` em (x, y) somente dentro do quadro; fora devolve transparente.
#[inline]
fn sample_in(f: &Frame, x: f32, y: f32) -> [f32; 4] {
    if x >= 0.0 && y >= 0.0 && x < f.w as f32 && y < f.h as f32 {
        f.sample(x, y)
    } else {
        [0.0; 4]
    }
}

/// Transições que movem ou deformam os quadros (sem máscara).
fn geometric(kind: TransitionKind, a: &Frame, b: &Frame, p: f32, out: &mut Frame) {
    let (w, h) = (a.w, a.h);
    if w == 0 || h == 0 {
        return;
    }
    let (wf, hf) = (w as f32, h as f32);
    let (cx, cy) = (wf * 0.5, hf * 0.5);
    out.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let fy = y as f32 + 0.5;
        for (x, o) in row.iter_mut().enumerate() {
            let fx = x as f32 + 0.5;
            let pa = a.px.get(y * w + x).copied().unwrap_or([0.0; 4]);
            let pb = b.px.get(y * w + x).copied().unwrap_or([0.0; 4]);
            *o = match kind {
                TransitionKind::PushUp => {
                    let sy = fy + p * hf;
                    if sy < hf {
                        sample_in(a, fx, sy)
                    } else {
                        sample_in(b, fx, sy - hf)
                    }
                }
                TransitionKind::PushDown => {
                    let sy = fy - p * hf;
                    if sy >= 0.0 {
                        sample_in(a, fx, sy)
                    } else {
                        sample_in(b, fx, sy + hf)
                    }
                }
                TransitionKind::Squeeze => {
                    // A se comprime para a esquerda enquanto B se expande pela direita
                    let split = (1.0 - p) * wf;
                    if fx < split {
                        sample_in(a, fx / (1.0 - p).max(1e-4), fy)
                    } else {
                        sample_in(b, (fx - split) / p.max(1e-4), fy)
                    }
                }
                TransitionKind::Swap => {
                    // os dois saem para lados opostos e voltam trocando a ordem
                    let off = (p * PI).sin() * wf * 0.55;
                    let sa = sample_in(a, fx + off, fy);
                    let sb = sample_in(b, fx - off, fy);
                    if p < 0.5 {
                        over(sa, sb)
                    } else {
                        over(sb, sa)
                    }
                }
                TransitionKind::FlashWhite => {
                    let white = (1.0 - (p - 0.5).abs() * 2.0).powf(0.7);
                    let base = if p < 0.5 { pa } else { pb };
                    let al = base[3].max(white);
                    [
                        base[0] + (al - base[0]) * white,
                        base[1] + (al - base[1]) * white,
                        base[2] + (al - base[2]) * white,
                        al,
                    ]
                }
                TransitionKind::FlyIn => {
                    // A voa em direção à câmera e some; B chega do fundo
                    let ka = 1.0 + p * 0.8;
                    let kb = (0.2 + 0.8 * p).max(1e-3);
                    let sa = scaled(
                        sample_in(a, cx + (fx - cx) / ka, cy + (fy - cy) / ka),
                        1.0 - p,
                    );
                    let sb = scaled(sample_in(b, cx + (fx - cx) / kb, cy + (fy - cy) / kb), p);
                    over(sa, sb)
                }
                TransitionKind::PagePeel => {
                    // dobra plana a partir do canto inferior direito
                    let span = wf + hf;
                    let s = ((wf - fx) + (hf - fy)) / span;
                    let fold = p * 1.02;
                    if s >= fold {
                        pa
                    } else {
                        // verso da página: reflexo do ponto em relação à linha da dobra
                        let s2 = 2.0 * fold - s;
                        let d = (s - s2) * span * 0.5;
                        let (rx, ry) = (fx + d, fy + d);
                        if s2 <= 1.0 && rx >= 0.0 && ry >= 0.0 && rx < wf && ry < hf {
                            let q = a.sample(rx, ry);
                            let al = q[3];
                            [
                                q[0] * 0.35 + al * 0.6,
                                q[1] * 0.35 + al * 0.6,
                                q[2] * 0.35 + al * 0.6,
                                al,
                            ]
                        } else {
                            pb
                        }
                    }
                }
                _ => pa,
            };
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Toda transição começa em A, termina em B e não trava com quadros
    /// vazios, minúsculos ou de tamanhos diferentes.
    #[test]
    fn todas_as_transicoes_vao_de_a_para_b_sem_panico() {
        let (w, h) = (24, 16);
        let a = Frame::filled(w, h, [1.0, 0.0, 0.0, 1.0]);
        let b = Frame::filled(w, h, [0.0, 0.0, 1.0, 1.0]);
        for kind in TransitionKind::ALL {
            let start = apply(kind, 0.05, &a, &b, 0.0);
            let end = apply(kind, 0.05, &a, &b, 1.0);
            let mid = apply(kind, 0.05, &a, &b, 0.5);
            assert!(mid.px.iter().flatten().all(|c| c.is_finite()), "{kind:?}");
            let red = start.px.iter().filter(|q| q[0] > 0.9).count();
            let blue = end.px.iter().filter(|q| q[2] > 0.9).count();
            assert!(red * 10 >= w * h * 9, "{kind:?} não começa em A ({red})");
            assert!(blue * 10 >= w * h * 9, "{kind:?} não termina em B ({blue})");
            let e = Frame::new(0, 0);
            let z = apply(kind, 0.05, &e, &e, 0.5);
            assert_eq!(z.px.len(), 0, "{kind:?}");
            let one = Frame::filled(1, 1, [0.5; 4]);
            apply(kind, 0.05, &one, &one, 0.3);
            apply(kind, f32::NAN, &a, &b, f32::NAN);
        }
    }

    #[test]
    fn warp_flow_interpola_movimento_sem_panico_e_resiste_dimensoes_extremas() {
        let tiny_a = Frame::filled(4, 4, [1.0, 0.0, 0.0, 1.0]);
        let tiny_b = Frame::filled(4, 4, [0.0, 1.0, 0.0, 1.0]);
        let tiny_mid = apply(TransitionKind::WarpFlow, 0.05, &tiny_a, &tiny_b, 0.5);
        assert_eq!((tiny_mid.w, tiny_mid.h), (4, 4));
        assert!((tiny_mid.px[0][0] - 0.5).abs() < 0.01);

        let empty = Frame::new(0, 0);
        let zero = apply(TransitionKind::WarpFlow, 0.05, &empty, &empty, 0.5);
        assert_eq!((zero.w, zero.h), (0, 0));

        let mut a = Frame::new(32, 24);
        let mut b = Frame::new(32, 24);
        for y in 6..14 {
            for x in 6..14 {
                a.px[y * 32 + x] = [1.0, 1.0, 1.0, 1.0];
                b.px[y * 32 + (x + 4)] = [1.0, 1.0, 1.0, 1.0];
            }
        }
        let mid = apply(TransitionKind::WarpFlow, 0.05, &a, &b, 0.5);
        assert_eq!(mid.px.len(), 32 * 24);
        assert!(mid.px[10 * 32 + 12][0] > 0.7);
        assert!(mid.px.iter().flatten().all(|c| c.is_finite()));
    }
}
