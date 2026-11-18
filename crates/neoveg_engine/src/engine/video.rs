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
    event_layer_opts(p, media, e, t, w, h, VideoRenderOpts::default())
}

/// Opções de renderização de prévia para um quadro composto.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VideoRenderOpts {
    pub prefer_proxy: bool,
    /// Ignora todos os efeitos de evento, trilha, ajuste e saída mestre (tela dividida "Antes").
    pub bypass_fx: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StereoEye {
    Center,
    Left,
    Right,
}

fn extract_stereo_eye(src: &RgbaImage, s3d: &Stereo3dClip, eye: StereoEye) -> Option<RgbaImage> {
    if eye == StereoEye::Center || s3d.layout == Stereo3dSourceLayout::Mono2D {
        return None;
    }
    let use_right = matches!(
        (eye, s3d.swap_lr),
        (StereoEye::Right, false) | (StereoEye::Left, true)
    );
    let (sw, sh) = (src.width(), src.height());
    match s3d.layout {
        Stereo3dSourceLayout::Mono2D => None,
        Stereo3dSourceLayout::SideBySideHalf | Stereo3dSourceLayout::SideBySideFull => {
            let half_w = (sw / 2).max(1);
            let x0 = if use_right {
                half_w.min(sw.saturating_sub(1))
            } else {
                0
            };
            let cw = if use_right {
                sw.saturating_sub(x0).max(1)
            } else {
                half_w
            };
            let mut out = RgbaImage::new(cw, sh.max(1));
            for y in 0..sh {
                for x in 0..cw {
                    let px = *src.get_pixel((x0 + x).min(sw - 1), y);
                    out.put_pixel(x, y, px);
                }
            }
            Some(out)
        }
        Stereo3dSourceLayout::TopBottomHalf => {
            let half_h = (sh / 2).max(1);
            let y0 = if use_right {
                half_h.min(sh.saturating_sub(1))
            } else {
                0
            };
            let ch = if use_right {
                sh.saturating_sub(y0).max(1)
            } else {
                half_h
            };
            let mut out = RgbaImage::new(sw.max(1), ch);
            for y in 0..ch {
                for x in 0..sw {
                    let px = *src.get_pixel(x, (y0 + y).min(sh - 1));
                    out.put_pixel(x, y, px);
                }
            }
            Some(out)
        }
    }
}

fn shift_frame_horizontal(src: &Frame, dx_px: f32) -> Frame {
    if dx_px.abs() < 0.25 {
        return src.clone();
    }
    let (w, h) = (src.w, src.h);
    let mut out = Frame::new(w, h);
    out.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, p) in row.iter_mut().enumerate() {
            *p = src.sample(x as f32 + 0.5 - dx_px, y as f32 + 0.5);
        }
    });
    out
}

fn apply_track_mask(dst: &mut Frame, mask: &Frame, mode: TrackMaskMode) {
    dst.px
        .par_iter_mut()
        .zip(mask.px.par_iter())
        .for_each(|(d, m)| {
            let luma = (0.2126 * m[0] + 0.7152 * m[1] + 0.0722 * m[2]).clamp(0.0, 1.0);
            let alpha = m[3].clamp(0.0, 1.0);
            let k = match mode {
                TrackMaskMode::None => 1.0,
                TrackMaskMode::MultiplyMask => luma,
                TrackMaskMode::InvertedMultiplyMask => (1.0 - luma).clamp(0.0, 1.0),
                TrackMaskMode::AlphaMask => alpha,
                TrackMaskMode::InvertedAlphaMask => (1.0 - alpha).clamp(0.0, 1.0),
            };
            for c in d.iter_mut() {
                *c *= k;
            }
        });
}

fn compose_stereo_3d(left: &Frame, right: &Frame, mode: Stereo3dMode) -> Frame {
    let (w, h) = (left.w.max(1), left.h.max(1));
    let mut out = Frame::new(w, h);
    match mode {
        Stereo3dMode::Off | Stereo3dMode::LeftOnly => return left.clone(),
        Stereo3dMode::RightOnly => return right.clone(),
        Stereo3dMode::AnaglyphRedCyan => {
            out.px
                .par_iter_mut()
                .zip(left.px.par_iter().zip(right.px.par_iter()))
                .for_each(|(o, (l, r))| {
                    *o = [l[0], r[1], r[2], l[3].max(r[3])];
                });
        }
        Stereo3dMode::SideBySideHalf | Stereo3dMode::SideBySideFull => {
            let half_w = (w / 2).max(1);
            let wf = w as f32;
            out.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                let sy = y as f32 + 0.5;
                for (x, p) in row.iter_mut().enumerate() {
                    if x < half_w {
                        let sx = ((x as f32 + 0.5) / half_w as f32) * wf;
                        *p = left.sample(sx, sy);
                    } else {
                        let rx = (x - half_w) as f32 + 0.5;
                        let rw = (w - half_w).max(1) as f32;
                        let sx = (rx / rw) * wf;
                        *p = right.sample(sx, sy);
                    }
                }
            });
        }
        Stereo3dMode::TopBottomHalf => {
            let half_h = (h / 2).max(1);
            let hf = h as f32;
            out.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                let (src, sy) = if y < half_h {
                    (left, ((y as f32 + 0.5) / half_h as f32) * hf)
                } else {
                    let ry = (y - half_h) as f32 + 0.5;
                    let rh = (h - half_h).max(1) as f32;
                    (right, (ry / rh) * hf)
                };
                for (x, p) in row.iter_mut().enumerate() {
                    *p = src.sample(x as f32 + 0.5, sy);
                }
            });
        }
    }
    out
}

fn event_layer_opts(
    p: &Project,
    media: &MediaCache,
    e: &Event,
    t: Ticks,
    w: usize,
    h: usize,
    opts: VideoRenderOpts,
) -> Option<Frame> {
    event_layer_eye(p, media, e, t, w, h, opts, StereoEye::Center)
}

#[allow(clippy::too_many_arguments)]
fn event_layer_eye(
    p: &Project,
    media: &MediaCache,
    e: &Event,
    t: Ticks,
    w: usize,
    h: usize,
    opts: VideoRenderOpts,
    eye: StereoEye,
) -> Option<Frame> {
    let m = p.media(e.media_id)?;
    if !m.has_video {
        return None;
    }
    let mt = e.media_time(t, m.duration)?;
    let src = media.video_frame_for_quality(m, mt, w as u32, h as u32, opts.prefer_proxy)?;
    let eye_img = extract_stereo_eye(&src, &e.stereo_3d, eye);
    let src_ref: &RgbaImage = eye_img.as_ref().unwrap_or(&src);
    let local = t - e.start;
    let key = e.pan_crop.eval(local);
    let mut f = draw_source(src_ref, w, h, &key, e.maintain_aspect);
    if eye != StereoEye::Center && e.stereo_3d.horizontal_offset.abs() > 1e-6 {
        let sign = if eye == StereoEye::Left { -0.5 } else { 0.5 };
        f = shift_frame_horizontal(&f, e.stereo_3d.horizontal_offset * (w as f32) * sign);
    }
    if opts.bypass_fx {
        return Some(f);
    }
    for fx in &e.fx {
        if fx.enabled && fx.kind == FxKind::Stabilization {
            let vals = fx.values(local);
            let smooth_k = (vals.first().copied().unwrap_or(65.0) / 100.0).clamp(0.0, 1.0);
            let border_zoom = vals.get(1).copied().unwrap_or(1.12);
            let rot_k = vals.get(2).copied().unwrap_or(0.35).clamp(0.0, 1.0);
            let (mut acc_dx, mut acc_dy, mut acc_ang, mut n_nbr) = (0.0f32, 0.0f32, 0.0f32, 0usize);
            if smooth_k > 1e-3 {
                let fd = e
                    .rate
                    .scale_ticks(p.settings.frame_dur())
                    .max(Ticks::from_millis_i64(1));
                if let Some(cur_s) = media.video_frame_for_quality(m, mt, 64, 36, true) {
                    for k in [-2i64, -1, 1, 2] {
                        let nbr_t = t + Ticks::from_raw(fd.0.saturating_mul(k));
                        if let Some(nbr_mt) = e.media_time(nbr_t, m.duration) {
                            if let Some(nbr_s) =
                                media.video_frame_for_quality(m, nbr_mt, 64, 36, true)
                            {
                                let (dx_n, dy_n, ang_n) =
                                    vfx::estimate_global_motion(&nbr_s, &cur_s);
                                acc_dx += dx_n;
                                acc_dy += dy_n;
                                acc_ang += ang_n;
                                n_nbr += 1;
                            }
                        }
                    }
                }
            }
            if n_nbr > 0 {
                let inv = 1.0 / (n_nbr as f32);
                vfx::apply_stabilization_transform(
                    &mut f,
                    -(acc_dx * inv) * (w as f32) * smooth_k,
                    -(acc_dy * inv) * (h as f32) * smooth_k,
                    -(acc_ang * inv) * rot_k * smooth_k,
                    border_zoom,
                );
            } else {
                vfx::apply_stabilization_transform(&mut f, 0.0, 0.0, 0.0, border_zoom);
            }
        } else {
            vfx::apply(fx, &mut f, local, p.settings.fps);
        }
    }
    Some(f)
}

/// Renderiza um único evento isolado no instante `t` (usado na prévia A/B do modo expandido).
pub fn render_event_still(
    p: &Project,
    media: &MediaCache,
    ev_id: Id,
    t: Ticks,
    w: usize,
    h: usize,
    prefer_proxy: bool,
) -> Frame {
    let w = w.max(1);
    let h = h.max(1);
    let Some((ti, ei)) = p.find_event(ev_id) else {
        return Frame::filled(w, h, [0.0, 0.0, 0.0, 1.0]);
    };
    let e = &p.tracks[ti].events[ei];
    let clamped_t = t.clamp_safe(e.start, (e.end() - Ticks::from_raw(1)).max(e.start));
    let opts = VideoRenderOpts {
        prefer_proxy,
        bypass_fx: false,
    };
    event_layer_opts(p, media, e, clamped_t, w, h, opts)
        .unwrap_or_else(|| Frame::filled(w, h, [0.0, 0.0, 0.0, 1.0]))
}

/// Renderiza o quadro de um take específico de evento (usado na grade multicâmera).
#[allow(clippy::too_many_arguments)]
pub fn render_event_take_still(
    p: &Project,
    media: &MediaCache,
    ev_id: Id,
    take_idx: usize,
    t: Ticks,
    w: usize,
    h: usize,
    prefer_proxy: bool,
) -> Frame {
    let w = w.max(1);
    let h = h.max(1);
    let Some((ti, ei)) = p.find_event(ev_id) else {
        return Frame::filled(w, h, [0.0, 0.0, 0.0, 1.0]);
    };
    let mut ev = p.tracks[ti].events[ei].clone();
    let _ = crate::takes::set_active(&mut ev, take_idx);
    let clamped_t = t.clamp_safe(ev.start, (ev.end() - Ticks::from_raw(1)).max(ev.start));
    let opts = VideoRenderOpts {
        prefer_proxy,
        bypass_fx: false,
    };
    event_layer_opts(p, media, &ev, clamped_t, w, h, opts)
        .unwrap_or_else(|| Frame::filled(w, h, [0.0, 0.0, 0.0, 1.0]))
}

#[allow(clippy::too_many_arguments)]
fn render_track_eye(
    p: &Project,
    media: &MediaCache,
    track: &Track,
    t: Ticks,
    w: usize,
    h: usize,
    opts: VideoRenderOpts,
    eye: StereoEye,
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
        if let Some(mut f) = event_layer_eye(p, media, e, t, w, h, opts, eye) {
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
        let fa = event_layer_eye(p, media, a, t, w, h, opts, eye).map(|mut f| {
            f.scale_alpha(event_alpha(a, t, false, true));
            f
        });
        let fb = event_layer_eye(p, media, b, t, w, h, opts, eye).map(|mut f| {
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
            (Some(tr), true) => event_layer_eye(p, media, e, t, w, h, opts, eye).map(|mut f| {
                f.scale_alpha(event_alpha(e, t, true, false));
                let prog = e
                    .fade_in
                    .curve
                    .shape(local.fraction_in(Ticks::ZERO, e.fade_in.len));
                transition::apply(tr.kind, tr.softness, &Frame::new(w, h), &f, prog)
            }),
            _ => event_layer_eye(p, media, e, t, w, h, opts, eye).map(|mut f| {
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

/// Trilha de ajuste: aplica os FX da trilha sobre a composição acumulada das
/// trilhas abaixo e mistura o resultado pela opacidade × envelope (0..1).
fn apply_adjustment(track: &Track, out: &mut Frame, t: Ticks, fps: FrameRate) {
    let op = track.opacity * track.env_value(EnvKind::Opacity, t);
    // NaN ou intensidade nula não aplica nada (§0.4).
    if op.is_nan() || op <= 1e-4 || !track.fx.iter().any(|fx| fx.enabled) {
        return;
    }
    let op = op.min(1.0);
    let mut adj = out.clone();
    for fx in &track.fx {
        vfx::apply(fx, &mut adj, t, fps);
    }
    if op >= 1.0 {
        *out = adj;
        return;
    }
    out.px
        .par_iter_mut()
        .zip(adj.px.par_iter())
        .for_each(|(o, a)| {
            for c in 0..4 {
                o[c] += (a[c] - o[c]) * op;
            }
        });
}

/// Renderiza o quadro do projeto no tempo exato `t` (ticks) na resolução w x h.
pub fn render_frame(p: &Project, media: &MediaCache, t: Ticks, w: usize, h: usize) -> Frame {
    render_frame_preview(p, media, t, w, h, false)
}

/// Renderiza o quadro do projeto com preferência opcional por proxies (§3.6).
pub fn render_frame_preview(
    p: &Project,
    media: &MediaCache,
    t: Ticks,
    w: usize,
    h: usize,
    prefer_proxy: bool,
) -> Frame {
    render_frame_with_opts(
        p,
        media,
        t,
        w,
        h,
        VideoRenderOpts {
            prefer_proxy,
            bypass_fx: false,
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn render_track_processed(
    p: &Project,
    media: &MediaCache,
    track: &Track,
    t: Ticks,
    w: usize,
    h: usize,
    opts: VideoRenderOpts,
    eye: StereoEye,
) -> Option<Frame> {
    let mut layer = render_track_eye(p, media, track, t, w, h, opts, eye)?;
    if !opts.bypass_fx {
        for fx in &track.fx {
            vfx::apply(fx, &mut layer, t, p.settings.fps);
        }
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
    Some(layer)
}

#[allow(clippy::too_many_arguments)]
fn compose_subtree(
    p: &Project,
    media: &MediaCache,
    track: &Track,
    t: Ticks,
    w: usize,
    h: usize,
    opts: VideoRenderOpts,
    eye: StereoEye,
    any_solo: bool,
    out: &mut Frame,
    depth: usize,
) {
    if depth > MAX_TRACK_DEPTH {
        return;
    }
    if p.is_track_effectively_muted(track.id) {
        return;
    }
    let video_children: Vec<&Track> = p
        .tracks
        .iter()
        .filter(|c| {
            c.parent_track_id == Some(track.id)
                && (c.kind == TrackKind::Video || (track.is_folder && c.is_folder))
        })
        .collect();
    if track.is_folder {
        for child in video_children.into_iter().rev() {
            compose_subtree(
                p,
                media,
                child,
                t,
                w,
                h,
                opts,
                eye,
                any_solo,
                out,
                depth + 1,
            );
        }
        return;
    }
    if track.kind != TrackKind::Video {
        return;
    }
    let track_active = !any_solo || track.solo;
    if track.is_adjustment() {
        if track_active && !opts.bypass_fx {
            apply_adjustment(track, out, t, p.settings.fps);
        }
        return;
    }
    if video_children.is_empty() {
        if !track_active {
            return;
        }
        let Some(layer) = render_track_processed(p, media, track, t, w, h, opts, eye) else {
            return;
        };
        if track.mask_mode != TrackMaskMode::None {
            apply_track_mask(out, &layer, track.mask_mode);
        } else {
            let op = track.opacity * track.env_value(EnvKind::Opacity, t);
            blend(out, &layer, track.blend, op);
        }
        return;
    }

    // Subárvore Pai/Filho (composição em grupo com máscara e movimento pai, VEGAS Pro p. 740).
    let mut group = Frame::new(w, h);
    for child in video_children.into_iter().rev() {
        if !p.is_track_effectively_muted(child.id) && child.mask_mode != TrackMaskMode::None {
            if !any_solo || child.solo {
                if let Some(mask) = render_track_processed(p, media, child, t, w, h, opts, eye) {
                    apply_track_mask(&mut group, &mask, child.mask_mode);
                }
            }
        } else {
            compose_subtree(
                p,
                media,
                child,
                t,
                w,
                h,
                opts,
                eye,
                any_solo,
                &mut group,
                depth + 1,
            );
        }
    }
    if track_active {
        if let Some(layer) = render_track_processed(p, media, track, t, w, h, opts, eye) {
            if track.mask_mode != TrackMaskMode::None {
                apply_track_mask(&mut group, &layer, track.mask_mode);
            } else {
                blend(&mut group, &layer, track.blend, 1.0);
            }
        }
    }
    let pmk = track.parent_motion.eval(t);
    if pmk != MotionKey::default() {
        group = apply_motion(&group, &pmk);
    }
    let op = track.opacity * track.env_value(EnvKind::Opacity, t);
    blend(out, &group, BlendMode::SourceAlpha, op);
}

fn render_frame_eye(
    p: &Project,
    media: &MediaCache,
    t: Ticks,
    w: usize,
    h: usize,
    opts: VideoRenderOpts,
    eye: StereoEye,
) -> Frame {
    let bg = p.settings.background;
    let mut out = Frame::filled(w, h, [bg[0], bg[1], bg[2], 1.0]);
    let any_solo = p.any_solo(TrackKind::Video);
    let track_ids: std::collections::HashSet<Id> = p.tracks.iter().map(|t| t.id).collect();
    for track in p.tracks.iter().rev() {
        // Trilhas com pai válido são compostas dentro de `compose_subtree` do ancestral.
        let has_valid_parent = track
            .parent_track_id
            .is_some_and(|pid| pid != track.id && track_ids.contains(&pid));
        if has_valid_parent {
            continue;
        }
        compose_subtree(p, media, track, t, w, h, opts, eye, any_solo, &mut out, 0);
    }
    if !opts.bypass_fx {
        for fx in &p.master.video_fx {
            vfx::apply(fx, &mut out, t, p.settings.fps);
        }
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

/// Renderiza o quadro do projeto com controle explícito de proxies e bypass de efeitos.
pub fn render_frame_with_opts(
    p: &Project,
    media: &MediaCache,
    t: Ticks,
    w: usize,
    h: usize,
    opts: VideoRenderOpts,
) -> Frame {
    let mode = p.settings.stereo_3d;
    match mode {
        Stereo3dMode::Off => render_frame_eye(p, media, t, w, h, opts, StereoEye::Center),
        Stereo3dMode::LeftOnly => render_frame_eye(p, media, t, w, h, opts, StereoEye::Left),
        Stereo3dMode::RightOnly => render_frame_eye(p, media, t, w, h, opts, StereoEye::Right),
        Stereo3dMode::AnaglyphRedCyan
        | Stereo3dMode::SideBySideHalf
        | Stereo3dMode::SideBySideFull
        | Stereo3dMode::TopBottomHalf => {
            let left = render_frame_eye(p, media, t, w, h, opts, StereoEye::Left);
            let right = render_frame_eye(p, media, t, w, h, opts, StereoEye::Right);
            compose_stereo_3d(&left, &right, mode)
        }
    }
}

/// Reamostra o retângulo `(sx, sy, sw, sh)` de `src` para o retângulo
/// `(dx, dy, dw, dh)` dentro de um quadro de dimensões `(w, h)` sobre fundo preto opaco.
#[allow(clippy::too_many_arguments)]
pub fn resample_rect(
    src: &Frame,
    sx: f32,
    sy: f32,
    sw: f32,
    sh: f32,
    w: usize,
    h: usize,
    dx: f32,
    dy: f32,
    dw: f32,
    dh: f32,
) -> Frame {
    let w = w.max(1);
    let h = h.max(1);
    let mut out = Frame::filled(w, h, [0.0, 0.0, 0.0, 1.0]);
    if src.w == 0 || src.h == 0 || sw <= 0.0 || sh <= 0.0 || dw <= 0.0 || dh <= 0.0 {
        return out;
    }
    let rx = sw / dw;
    let ry = sh / dh;
    let d_x0 = dx.floor().max(0.0) as usize;
    let d_y0 = dy.floor().max(0.0) as usize;
    let d_x1 = (dx + dw).ceil().clamp(0.0, w as f32) as usize;
    let d_y1 = (dy + dh).ceil().clamp(0.0, h as f32) as usize;
    out.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        if y < d_y0 || y >= d_y1 {
            return;
        }
        let src_y = sy + (y as f32 + 0.5 - dy) * ry;
        if src_y < 0.0 || src_y > src.h as f32 {
            return;
        }
        for (x, p) in row.iter_mut().enumerate().take(d_x1).skip(d_x0) {
            let src_x = sx + (x as f32 + 0.5 - dx) * rx;
            if src_x < 0.0 || src_x > src.w as f32 {
                continue;
            }
            let c = src.sample(src_x, src_y);
            let ia = (1.0 - c[3]).clamp(0.0, 1.0);
            *p = [c[0], c[1], c[2], (c[3] + ia).clamp(0.0, 1.0)];
        }
    });
    out
}

/// Redimensiona proporcionalmente `src` para caber em `(w, h)` mantendo proporção.
pub fn scale_frame_fit(src: &Frame, w: usize, h: usize) -> Frame {
    let (w, h) = (w.max(1), h.max(1));
    if src.w == w && src.h == h {
        return src.clone();
    }
    if src.w == 0 || src.h == 0 {
        return Frame::filled(w, h, [0.0, 0.0, 0.0, 1.0]);
    }
    let scale = (w as f32 / src.w as f32).min(h as f32 / src.h as f32);
    let dw = src.w as f32 * scale;
    let dh = src.h as f32 * scale;
    resample_rect(
        src,
        0.0,
        0.0,
        src.w as f32,
        src.h as f32,
        w,
        h,
        (w as f32 - dw) * 0.5,
        (h as f32 - dh) * 0.5,
        dw,
        dh,
    )
}

#[cfg(test)]
mod adjustment_tests {
    use super::*;
    use crate::commands::{run_command, HeadlessHost};
    use crate::session::EditorSession;

    fn project_with_adjustment(fx: Option<FxKind>) -> Project {
        let mut p = Project::default();
        let mut t = Track::new(TrackKind::Video, "Ajuste 1");
        t.adjustment = true;
        if let Some(k) = fx {
            t.fx.push(FxInstance::new(k));
        }
        p.tracks.push(t);
        p
    }

    fn first_red(p: &Project) -> f32 {
        let cache = MediaCache::default();
        let f = render_frame(p, &cache, Ticks::ZERO, 4, 4);
        f.px[0][0]
    }

    #[test]
    fn adjustment_track_applies_fx_to_composite_below() {
        // fundo preto invertido vira branco
        let p = project_with_adjustment(Some(FxKind::Invert));
        assert!((first_red(&p) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn adjustment_track_mixes_by_opacity() {
        let mut p = project_with_adjustment(Some(FxKind::Invert));
        p.tracks[0].opacity = 0.5;
        assert!((first_red(&p) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn adjustment_track_muted_or_empty_does_nothing() {
        let mut p = project_with_adjustment(Some(FxKind::Invert));
        p.tracks[0].mute = true;
        assert!(first_red(&p).abs() < 1e-4);
        let p = project_with_adjustment(None);
        assert!(first_red(&p).abs() < 1e-4);
    }

    #[test]
    fn adjustment_track_nan_opacity_is_ignored() {
        let mut p = project_with_adjustment(Some(FxKind::Invert));
        p.tracks[0].opacity = f32::NAN;
        assert!(first_red(&p).abs() < 1e-4);
    }

    #[test]
    fn old_project_without_field_loads_as_normal_track() {
        let json = r#"{"settings": {"width": 8, "height": 8, "fps": 30.0, "sample_rate": 48000},
            "media": [],
            "tracks": [{"id": 1, "name": "V", "kind": "Video", "events": []}]}"#;
        let (p, _) = Project::from_json(json).expect("projeto válido");
        assert!(!p.tracks[0].adjustment);
        assert!(!p.tracks[0].is_adjustment());
    }

    #[test]
    fn insert_adjustment_command_creates_video_adjustment_track() {
        let mut s = EditorSession::new(Project::new_default());
        let before = s.project.tracks.len();
        run_command("insert.track_adjustment", &mut s, &mut HeadlessHost).expect("comando executa");
        assert_eq!(s.project.tracks.len(), before + 1);
        let adj: Vec<_> = s
            .project
            .tracks
            .iter()
            .filter(|t| t.is_adjustment())
            .collect();
        assert_eq!(adj.len(), 1);
        assert_eq!(adj[0].kind, TrackKind::Video);
        assert_eq!(adj[0].name, "Ajuste 1");
    }

    #[test]
    fn adjustment_track_does_not_accept_events() {
        let mut t = Track::new(TrackKind::Video, "A");
        assert!(t.accepts(TrackKind::Video));
        t.adjustment = true;
        assert!(!t.accepts(TrackKind::Video));
        assert!(!t.accepts(TrackKind::Audio));
    }

    #[test]
    fn bypass_fx_ignora_efeitos_de_evento_trilha_e_master() {
        let mut p = Project::default();
        p.settings.background = [0.0, 0.0, 0.0];
        p.master.video_fx.push(FxInstance::new(FxKind::Invert));
        let cache = MediaCache::default();
        let normal = render_frame_with_opts(
            &p,
            &cache,
            Ticks::ZERO,
            4,
            4,
            VideoRenderOpts {
                prefer_proxy: false,
                bypass_fx: false,
            },
        );
        let bypassed = render_frame_with_opts(
            &p,
            &cache,
            Ticks::ZERO,
            4,
            4,
            VideoRenderOpts {
                prefer_proxy: false,
                bypass_fx: true,
            },
        );
        assert!((normal.px[0][0] - 1.0).abs() < 1e-4);
        assert!(bypassed.px[0][0].abs() < 1e-4);
    }

    #[test]
    fn pai_filho_mascara_e_estereoscopico_3d() {
        let mut p = Project::default();
        p.settings.background = [0.0, 0.0, 0.0];
        let mut parent = Track::new(TrackKind::Video, "Pai");
        let mut child = Track::new(TrackKind::Video, "Filho");
        let mut mask_tr = Track::new(TrackKind::Video, "Máscara");

        let green_m = MediaItem {
            id: new_id(),
            name: "Verde".into(),
            source: MediaSource::Generated(GenSpec::SolidColor {
                color: [0.0, 1.0, 0.0, 1.0],
            }),
            duration: None,
            width: 32,
            height: 32,
            fps: None,
            has_video: true,
            has_audio: false,
            info: String::new(),
            proxy_path: None,
        };
        let half_black = MediaItem {
            id: new_id(),
            name: "Escuro".into(),
            source: MediaSource::Generated(GenSpec::SolidColor {
                color: [0.5, 0.5, 0.5, 1.0],
            }),
            duration: None,
            width: 32,
            height: 32,
            fps: None,
            has_video: true,
            has_audio: false,
            info: String::new(),
            proxy_path: None,
        };
        p.media = vec![green_m.clone(), half_black.clone()];
        let ev_c = Event::new(&green_m, Ticks::ZERO, Ticks::from_secs_i64(2));
        child.events.push(ev_c);
        child.parent_track_id = Some(parent.id);

        let ev_m = Event::new(&half_black, Ticks::ZERO, Ticks::from_secs_i64(2));
        mask_tr.events.push(ev_m);
        mask_tr.parent_track_id = Some(parent.id);
        mask_tr.mask_mode = TrackMaskMode::MultiplyMask;

        // Movimento no Pai desloca o grupo para fora da borda esquerda em parte do quadro
        parent.parent_motion.keys[0].value.x = 0.5;
        p.tracks = vec![parent, mask_tr, child];

        let cache = MediaCache::default();
        let f = render_frame(&p, &cache, Ticks::ZERO, 20, 20);
        // Lado esquerdo fica preto pelo deslocamento Pai/Filho, lado direito mascarado em ~0.5
        assert!(f.px[10 * 20 + 2][1] < 0.05);
        assert!((f.px[10 * 20 + 18][1] - 0.5).abs() < 0.08);

        // Estereoscópico 3D: Anáglifo vermelho/ciano + lado a lado com offset horizontal
        let mut p3d = Project::default();
        p3d.settings.background = [0.0, 0.0, 0.0];
        p3d.settings.stereo_3d = Stereo3dMode::AnaglyphRedCyan;
        let white_m = MediaItem {
            id: new_id(),
            name: "Branco".into(),
            source: MediaSource::Generated(GenSpec::SolidColor {
                color: [1.0, 1.0, 1.0, 1.0],
            }),
            duration: None,
            width: 20,
            height: 20,
            fps: None,
            has_video: true,
            has_audio: false,
            info: String::new(),
            proxy_path: None,
        };
        p3d.media = vec![white_m.clone()];
        let mut tr3d = Track::new(TrackKind::Video, "3D");
        tr3d.motion.keys[0].value.scale_x = 0.4;
        let mut ev3d = Event::new(&white_m, Ticks::ZERO, Ticks::from_secs_i64(2));
        ev3d.stereo_3d = Stereo3dClip {
            layout: Stereo3dSourceLayout::Mono2D,
            swap_lr: false,
            horizontal_offset: 0.2,
        };
        tr3d.events.push(ev3d);
        p3d.tracks = vec![tr3d];
        let ana = render_frame(&p3d, &cache, Ticks::ZERO, 40, 20);
        // Faixa escalada 0.4× ao redor de x=20: esquerda cobre ~12..26,4 e direita ~13,6..28
        let left_px = ana.px[10 * 40 + 12];
        let right_px = ana.px[10 * 40 + 27];
        assert!(left_px[0] > 0.5 && left_px[1] < 0.2 && left_px[2] < 0.2);
        assert!(right_px[0] < 0.2 && right_px[1] > 0.5 && right_px[2] > 0.5);

        p3d.settings.stereo_3d = Stereo3dMode::SideBySideHalf;
        let sbs = render_frame(&p3d, &cache, Ticks::ZERO, 40, 20);
        assert_eq!((sbs.w, sbs.h), (40, 20));
    }
}
