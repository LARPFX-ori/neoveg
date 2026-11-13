//! Composição de vídeo: eventos -> trilhas -> saída.
//!
//! A ordem segue o VEGAS: a trilha de cima na lista é composta por cima das
//! de baixo. Cada evento passa por Pan/Crop e Event FX; cada trilha por Track
//! FX, Fade-to-color, Track Motion e nível de composição/modo de mistura.

use super::{transition, vfx, Frame};
use crate::media::MediaCache;
use crate::model::*;
use image::RgbaImage;
use rayon::prelude::*;

/// Desenha uma imagem de origem no quadro de saída aplicando ajuste à tela
/// (letterbox) e o enquadramento do Pan/Crop.
pub fn draw_source(
    src: &RgbaImage,
    w: usize,
    h: usize,
    key: &PanCropKey,
    maintain_aspect: bool,
) -> Frame {
    let (sw, sh) = (src.width() as f32, src.height() as f32);
    let (wf, hf) = (w as f32, h as f32);
    let identity = key.size == 1.0 && key.rot == 0.0 && key.cx == 0.5 && key.cy == 0.5;
    let mut out = Frame::new(w, h);
    let raw = src.as_raw();
    let srcw = src.width() as usize;
    let to_f = |i: usize| -> [f32; 4] {
        let a = raw[i + 3] as f32 / 255.0;
        [
            raw[i] as f32 / 255.0 * a,
            raw[i + 1] as f32 / 255.0 * a,
            raw[i + 2] as f32 / 255.0 * a,
            a,
        ]
    };
    // caminho rápido: mesma resolução e sem pan/crop
    if identity && src.width() as usize == w && src.height() as usize == h {
        out.px
            .par_iter_mut()
            .enumerate()
            .for_each(|(i, p)| *p = to_f(i * 4));
        return out;
    }
    let (fw, fh) = if maintain_aspect {
        let s = (wf / sw).min(hf / sh);
        (sw * s, sh * s)
    } else {
        (wf, hf)
    };
    let fx0 = (wf - fw) / 2.0;
    let fy0 = (hf - fh) / 2.0;
    let (sx_scale, sy_scale) = (sw / fw, sh / fh);
    let (sn, cs) = key.rot.to_radians().sin_cos();
    let size = key.size.max(0.001);
    let (ccx, ccy) = (key.cx * wf, key.cy * hf);
    // ao reduzir muito a imagem, usa uma amostragem com área (pré-redução)
    let downscale = (sx_scale * size).max(sy_scale * size);
    let sample_src = |x: f32, y: f32| -> [f32; 4] {
        // bilinear em u8
        let fx = x - 0.5;
        let fy = y - 0.5;
        let x0 = fx.floor();
        let y0 = fy.floor();
        let ax = fx - x0;
        let ay = fy - y0;
        let (x0, y0) = (x0 as isize, y0 as isize);
        let get = |xx: isize, yy: isize| -> [f32; 4] {
            if xx < 0 || yy < 0 || xx >= srcw as isize || yy >= src.height() as isize {
                [0.0; 4]
            } else {
                to_f((yy as usize * srcw + xx as usize) * 4)
            }
        };
        let (p00, p10, p01, p11) = (
            get(x0, y0),
            get(x0 + 1, y0),
            get(x0, y0 + 1),
            get(x0 + 1, y0 + 1),
        );
        let mut o = [0.0; 4];
        for k in 0..4 {
            let a = p00[k] + (p10[k] - p00[k]) * ax;
            let b = p01[k] + (p11[k] - p01[k]) * ax;
            o[k] = a + (b - a) * ay;
        }
        o
    };
    let taps: i32 = if downscale > 2.5 { 2 } else { 1 };
    out.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, p) in row.iter_mut().enumerate() {
            let mut acc = [0f32; 4];
            for ty in 0..taps {
                for tx in 0..taps {
                    let ox = (tx as f32 + 0.5) / taps as f32;
                    let oy = (ty as f32 + 0.5) / taps as f32;
                    // ponto no espaço "enquadrado"
                    let dx = (x as f32 + ox - wf / 2.0) * size;
                    let dy = (y as f32 + oy - hf / 2.0) * size;
                    let qx = dx * cs - dy * sn + ccx;
                    let qy = dx * sn + dy * cs + ccy;
                    let sx = (qx - fx0) * sx_scale;
                    let sy = (qy - fy0) * sy_scale;
                    if sx < -0.5 || sy < -0.5 || sx > sw + 0.5 || sy > sh + 0.5 {
                        continue;
                    }
                    let s = sample_src(sx, sy);
                    for k in 0..4 {
                        acc[k] += s[k];
                    }
                }
            }
            let n = (taps * taps) as f32;
            *p = [acc[0] / n, acc[1] / n, acc[2] / n, acc[3] / n];
        }
    });
    out
}

/// Aplica Track Motion (posição, escala, rotação) a uma camada.
fn apply_motion(f: &Frame, m: &MotionKey) -> Frame {
    let (w, h) = (f.w, f.h);
    let (wf, hf) = (w as f32, h as f32);
    let (sn, cs) = (-m.rot.to_radians()).sin_cos();
    let (sx, sy) = (m.scale_x.max(0.001), m.scale_y.max(0.001));
    let mut out = Frame::new(w, h);
    out.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, p) in row.iter_mut().enumerate() {
            let dx = x as f32 + 0.5 - wf / 2.0 - m.x * wf;
            let dy = y as f32 + 0.5 - hf / 2.0 - m.y * hf;
            let rx = dx * cs - dy * sn;
            let ry = dx * sn + dy * cs;
            let lx = rx / sx + wf / 2.0;
            let ly = ry / sy + hf / 2.0;
            *p = f.sample(lx, ly);
        }
    });
    out
}

#[inline]
fn blend_channel(mode: BlendMode, cb: f32, cs: f32) -> f32 {
    match mode {
        BlendMode::SourceAlpha => cs,
        BlendMode::Add => (cb + cs).min(1.0),
        BlendMode::Subtract => (cb - cs).max(0.0),
        BlendMode::Multiply => cb * cs,
        BlendMode::Screen => cb + cs - cb * cs,
        BlendMode::Overlay => blend_channel(BlendMode::HardLight, cs, cb),
        BlendMode::HardLight => {
            if cs <= 0.5 {
                cb * 2.0 * cs
            } else {
                let s = 2.0 * cs - 1.0;
                cb + s - cb * s
            }
        }
        BlendMode::Lighten => cb.max(cs),
        BlendMode::Darken => cb.min(cs),
        BlendMode::Difference => (cb - cs).abs(),
    }
}

/// Mistura `top` (pré-multiplicado) sobre `dst` com opacidade e modo.
pub fn blend(dst: &mut Frame, top: &Frame, mode: BlendMode, opacity: f32) {
    let op = opacity.clamp(0.0, 1.0);
    if op <= 0.0 {
        return;
    }
    dst.px
        .par_iter_mut()
        .zip(top.px.par_iter())
        .for_each(|(d, s)| {
            let s = [s[0] * op, s[1] * op, s[2] * op, s[3] * op];
            let sa = s[3];
            if sa <= 1e-6 {
                return;
            }
            if mode == BlendMode::SourceAlpha {
                let ia = 1.0 - sa;
                for k in 0..4 {
                    d[k] = s[k] + d[k] * ia;
                }
                return;
            }
            let da = d[3];
            let mut o = [0f32; 4];
            for k in 0..3 {
                let cs = s[k] / sa;
                let cb = if da > 1e-6 { d[k] / da } else { 0.0 };
                let b = blend_channel(mode, cb, cs);
                o[k] = s[k] * (1.0 - da) + d[k] * (1.0 - sa) + sa * da * b;
            }
            o[3] = sa + da * (1.0 - sa);
            *d = o;
        });
}

/// Alfa de um evento considerando fades, com opção de ignorar fade de
/// entrada/saída (quando substituído por uma transição).
fn event_alpha(e: &Event, t: Ticks, ignore_in: bool, ignore_out: bool) -> f32 {
    let mut g = 1.0;
    let local = t - e.start;
    if !ignore_in && e.fade_in.len > Ticks::ZERO && local < e.fade_in.len {
        g *= e
            .fade_in
            .curve
            .shape(local.fraction_in(Ticks::ZERO, e.fade_in.len));
    }
    let rem = e.end() - t;
    if !ignore_out && e.fade_out.len > Ticks::ZERO && rem < e.fade_out.len {
        g *= e
            .fade_out
            .curve
            .shape(rem.fraction_in(Ticks::ZERO, e.fade_out.len));
    }
    g
}

pub fn event_layer(
    p: &Project,
    media: &MediaCache,
    e: &Event,
    t: Ticks,
    w: usize,
    h: usize,
) -> Option<Frame> {
    let m = p.media(e.media_id)?;
    if !m.has_video {
        return None;
    }
    let mt = e.media_time(t, m.duration)?;
    let src = media.video_frame(m, mt, w as u32, h as u32)?;
    let local = t - e.start;
    let key = e.pan_crop.eval(local);
    let mut f = draw_source(&src, w, h, &key, e.maintain_aspect);
    for fx in &e.fx {
        vfx::apply(fx, &mut f, local);
    }
    Some(f)
}

fn render_track(
    p: &Project,
    media: &MediaCache,
    track: &Track,
    t: Ticks,
    w: usize,
    h: usize,
) -> Option<Frame> {
    let mut active: Vec<&Event> = track
        .events
        .iter()
        .filter(|e| !e.mute && e.contains(t))
        .collect();
    if active.is_empty() {
        return None;
    }
    active.sort_by_key(|e| e.start);
    let n = active.len();
    // camadas mais antigas por baixo (sem transição)
    let mut base: Option<Frame> = None;
    for e in active.iter().take(n.saturating_sub(2)) {
        if let Some(mut f) = event_layer(p, media, e, t, w, h) {
            f.scale_alpha(event_alpha(e, t, false, false));
            match &mut base {
                Some(b) => b.over(&f),
                None => base = Some(f),
            }
        }
    }
    let layer = if n >= 2 {
        let a = active[n - 2];
        let b = active[n - 1];
        let overlap = (a.end() - b.start).max(Ticks::from_raw(1));
        let prog = t.fraction_in(b.start, overlap);
        let fa = event_layer(p, media, a, t, w, h).map(|mut f| {
            f.scale_alpha(event_alpha(a, t, false, true));
            f
        });
        let fb = event_layer(p, media, b, t, w, h).map(|mut f| {
            f.scale_alpha(event_alpha(b, t, true, false));
            f
        });
        let fa = fa.unwrap_or_else(|| Frame::new(w, h));
        let fb = fb.unwrap_or_else(|| Frame::new(w, h));
        let tr = b.transition.clone().unwrap_or(TransitionSpec {
            kind: TransitionKind::Dissolve,
            softness: 0.0,
        });
        // curva da transição segue a curva do fade de entrada de B
        let shaped = b.fade_in.curve.shape(prog);
        Some(transition::apply(tr.kind, tr.softness, &fa, &fb, shaped))
    } else {
        let e = active[0];
        let local = t - e.start;
        match (
            &e.transition,
            e.fade_in.len > Ticks::ZERO && local < e.fade_in.len,
        ) {
            // transição sem evento anterior: entra a partir do transparente
            (Some(tr), true) => event_layer(p, media, e, t, w, h).map(|mut f| {
                f.scale_alpha(event_alpha(e, t, true, false));
                let prog = e
                    .fade_in
                    .curve
                    .shape(local.fraction_in(Ticks::ZERO, e.fade_in.len));
                transition::apply(tr.kind, tr.softness, &Frame::new(w, h), &f, prog)
            }),
            _ => event_layer(p, media, e, t, w, h).map(|mut f| {
                f.scale_alpha(event_alpha(e, t, false, false));
                f
            }),
        }
    };
    match (base, layer) {
        (Some(mut b), Some(l)) => {
            b.over(&l);
            Some(b)
        }
        (b, l) => l.or(b),
    }
}

/// Renderiza o quadro do projeto no tempo exato `t` (ticks) na resolução w x h.
pub fn render_frame(p: &Project, media: &MediaCache, t: Ticks, w: usize, h: usize) -> Frame {
    let bg = p.settings.background;
    let mut out = Frame::filled(w, h, [bg[0], bg[1], bg[2], 1.0]);
    let any_solo = p.any_solo(TrackKind::Video);
    for track in p.tracks.iter().rev() {
        if track.kind != TrackKind::Video || !track.is_audible(any_solo) {
            continue;
        }
        let Some(mut layer) = render_track(p, media, track, t, w, h) else {
            continue;
        };
        for fx in &track.fx {
            vfx::apply(fx, &mut layer, t);
        }
        let ftc = track.env_value(EnvKind::FadeToColor, t);
        if ftc.abs() > 1e-4 {
            let target = if ftc > 0.0 { 1.0 } else { 0.0 };
            let k = ftc.abs().min(1.0);
            layer.px.par_iter_mut().for_each(|px| {
                for c in 0..3 {
                    px[c] += (target * px[3] - px[c]) * k;
                }
            });
        }
        let mk = track.motion.eval(t);
        if mk != MotionKey::default() {
            layer = apply_motion(&layer, &mk);
        }
        let op = track.opacity * track.env_value(EnvKind::Opacity, t);
        blend(&mut out, &layer, track.blend, op);
    }
    for fx in &p.master.video_fx {
        vfx::apply(fx, &mut out, t);
    }
    // garante saída opaca
    out.px.par_iter_mut().for_each(|px| {
        let ia = 1.0 - px[3];
        if ia > 0.0 {
            px[0] += bg[0] * ia;
            px[1] += bg[1] * ia;
            px[2] += bg[2] * ia;
            px[3] = 1.0;
        }
    });
    out
}
