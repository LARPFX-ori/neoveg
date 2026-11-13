//! Transições entre eventos sobrepostos na mesma trilha.

use super::{smoothstep, Frame};
use crate::model::TransitionKind;
use rayon::prelude::*;
use std::f32::consts::PI;

/// Combina o quadro de saída `a` com o de entrada `b` no progresso `p` (0..1).
pub fn apply(kind: TransitionKind, softness: f32, a: &Frame, b: &Frame, p: f32) -> Frame {
    let (w, h) = (a.w, a.h);
    let p = p.clamp(0.0, 1.0);
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
                    *o = [s[0] * k, s[1] * k, s[2] * k, s[3].max(a.px[i][3] * (1.0 - k))];
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
                                [s[0] + d[0] * ia, s[1] + d[1] * ia, s[2] + d[2] * ia, s[3] + d[3] * ia]
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
                    *o = [sb[0] + d[0] * ia, sb[1] + d[1] * ia, sb[2] + d[2] * ia, sb[3] + d[3] * ia];
                }
            });
        }
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
                            let d = (dx * dx + dy * dy).sqrt() / (0.5 * (aspect * aspect + 1.0).sqrt());
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
