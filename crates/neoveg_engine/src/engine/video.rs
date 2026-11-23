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
    draw_source_ex(src, w, h, key, maintain_aspect, true)
}

/// Variante completa de [`draw_source`] suportando *Esticar para preencher o
/// quadro* (`stretch_to_fill = false` mantém o tamanho nativo da imagem de
/// origem centralizada no quadro de saída sem esticar).
pub fn draw_source_ex(
    src: &RgbaImage,
    w: usize,
    h: usize,
    key: &PanCropKey,
    maintain_aspect: bool,
    stretch_to_fill: bool,
) -> Frame {
    let (sw, sh) = (src.width() as f32, src.height() as f32);
    let (wf, hf) = (w as f32, h as f32);
    let ratio = key.ratio_safe();
    let identity = key.size == 1.0
        && key.rot == 0.0
        && key.cx == 0.5
        && key.cy == 0.5
        && ratio == 1.0
        && !key.flip_h
        && !key.flip_v;
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
    let (fw, fh) = if !stretch_to_fill {
        (sw.max(1.0), sh.max(1.0))
    } else if maintain_aspect {
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
    let size_x = if key.flip_h { -size } else { size };
    let size_y = size * ratio * if key.flip_v { -1.0 } else { 1.0 };
    let (ccx, ccy) = (key.cx * wf, key.cy * hf);
    // ao reduzir muito a imagem, usa uma amostragem com área (pré-redução)
    let downscale = (sx_scale * size).max(sy_scale * size * ratio);
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
                    let dx = (x as f32 + ox - wf / 2.0) * size_x;
                    let dy = (y as f32 + oy - hf / 2.0) * size_y;
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

/// No modo 2D os campos de Z/X/Y de 3D não contam; rotação soma `rot + orient_z`.
fn motion_2d_is_identity(m: &MotionKey) -> bool {
    m.x == 0.0 && m.y == 0.0 && m.scale_x == 1.0 && m.scale_y == 1.0 && (m.rot + m.orient_z) == 0.0
}

/// Aplica Track Motion 2D (posição, escala, pivô e rotação + orientação Z) a uma camada.
fn apply_motion(f: &Frame, m: &MotionKey) -> Frame {
    let (w, h) = (f.w, f.h);
    let (wf, hf) = (w as f32, h as f32);
    let angle = if (m.rot + m.orient_z).is_finite() {
        m.rot + m.orient_z
    } else {
        0.0
    };
    let (sn, cs) = (-angle.to_radians()).sin_cos();
    let sx = if m.scale_x.is_finite() {
        m.scale_x.max(0.001)
    } else {
        1.0
    };
    let sy = if m.scale_y.is_finite() {
        m.scale_y.max(0.001)
    } else {
        1.0
    };
    let px = if m.pivot_x.is_finite() {
        m.pivot_x * wf
    } else {
        0.0
    };
    let py = if m.pivot_y.is_finite() {
        m.pivot_y * hf
    } else {
        0.0
    };
    let tx = if m.x.is_finite() { m.x * wf } else { 0.0 };
    let ty = if m.y.is_finite() { m.y * hf } else { 0.0 };
    let mut out = Frame::new(w, h);
    out.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, p) in row.iter_mut().enumerate() {
            // Inversa de T(pos + pivot) * R(angle) * T(-pivot) * S(sx, sy)
            let dx = x as f32 + 0.5 - wf / 2.0 - tx - px;
            let dy = y as f32 + 0.5 - hf / 2.0 - ty - py;
            let rx = dx * cs - dy * sn + px;
            let ry = dx * sn + dy * cs + py;
            let lx = rx / sx + wf / 2.0;
            let ly = ry / sy + hf / 2.0;
            *p = f.sample(lx, ly);
        }
    });
    out
}

/// Aplica *Sombra 2D* ou *Brilho 2D* do Movimento da Trilha (VEGAS Pro p. 412)
/// desfocando o canal alfa da camada, deslocando-o e compondo-o atrás dela.
pub fn apply_shadow_glow_2d(layer: &Frame, key: &ShadowGlowKey) -> Frame {
    let (w, h) = (layer.w, layer.h);
    if w == 0 || h == 0 {
        return layer.clone();
    }
    let intensity = if key.intensity.is_finite() {
        key.intensity.clamp(0.0, 2.0)
    } else {
        0.0
    };
    if intensity <= 1e-4 || key.color[3] <= 1e-4 {
        return layer.clone();
    }
    let ox = if key.offset_x.is_finite() {
        (key.offset_x * w as f32).round() as isize
    } else {
        0
    };
    let oy = if key.offset_y.is_finite() {
        (key.offset_y * h as f32).round() as isize
    } else {
        0
    };
    let mut alpha = vec![0.0f32; w * h];
    for y in 0..h {
        let sy = y as isize - oy;
        if sy < 0 || sy >= h as isize {
            continue;
        }
        let s_row = sy as usize * w;
        let d_row = y * w;
        for x in 0..w {
            let sx = x as isize - ox;
            if sx >= 0 && sx < w as isize {
                alpha[d_row + x] = layer.px[s_row + sx as usize][3];
            }
        }
    }
    let blur_n = if key.blur.is_finite() {
        key.blur.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let radius = ((blur_n * (w.min(h) as f32) * 0.08).round() as usize).min(48);
    if radius > 0 {
        // Duas passadas de desfoque em caixa separável (aproximação gaussiana rápida).
        let mut tmp = vec![0.0f32; w * h];
        for _ in 0..2 {
            // horizontal
            let win = (radius * 2 + 1) as f32;
            for y in 0..h {
                let row = &alpha[y * w..(y + 1) * w];
                let dst = &mut tmp[y * w..(y + 1) * w];
                let mut sum = 0.0f32;
                for dx in -(radius as isize)..=(radius as isize) {
                    let ix = dx.clamp(0, w as isize - 1) as usize;
                    sum += row[ix];
                }
                for (x, d) in dst.iter_mut().enumerate().take(w) {
                    *d = sum / win;
                    let sub_x = (x as isize - radius as isize).clamp(0, w as isize - 1) as usize;
                    let add_x =
                        (x as isize + radius as isize + 1).clamp(0, w as isize - 1) as usize;
                    sum += row[add_x] - row[sub_x];
                }
            }
            // vertical
            for x in 0..w {
                let mut sum = 0.0f32;
                for dy in -(radius as isize)..=(radius as isize) {
                    let iy = dy.clamp(0, h as isize - 1) as usize;
                    sum += tmp[iy * w + x];
                }
                for y in 0..h {
                    alpha[y * w + x] = sum / win;
                    let sub_y = (y as isize - radius as isize).clamp(0, h as isize - 1) as usize;
                    let add_y =
                        (y as isize + radius as isize + 1).clamp(0, h as isize - 1) as usize;
                    sum += tmp[add_y * w + x] - tmp[sub_y * w + x];
                }
            }
        }
    }

    let cr = key.color[0].clamp(0.0, 1.0);
    let cg = key.color[1].clamp(0.0, 1.0);
    let cb = key.color[2].clamp(0.0, 1.0);
    let ca = key.color[3].clamp(0.0, 1.0) * intensity;
    let mut out = Frame::new(w, h);
    out.px.par_iter_mut().enumerate().for_each(|(i, dst)| {
        let sa = (alpha[i] * ca).clamp(0.0, 1.0);
        let bg = [cr * sa, cg * sa, cb * sa, sa];
        let fg = layer.px[i];
        let ia = 1.0 - fg[3];
        *dst = [
            fg[0] + bg[0] * ia,
            fg[1] + bg[1] * ia,
            fg[2] + bg[2] * ia,
            fg[3] + bg[3] * ia,
        ];
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

/// Alfa de um evento considerando opacidade do evento, envelope de opacidade e fades,
/// com opção de ignorar fade de entrada/saída (quando substituído por uma transição).
fn event_alpha(e: &Event, t: Ticks, ignore_in: bool, ignore_out: bool) -> f32 {
    let mut g = if e.opacity.is_finite() {
        e.opacity.clamp(0.0, 1.0)
    } else {
        1.0
    };
    let local = t - e.start;
    if let Some(env) = e.envelope(EnvKind::Opacity) {
        let ev_op = env.eval(local);
        if ev_op.is_finite() {
            g *= ev_op.clamp(0.0, 1.0);
        }
    }
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
    let key = eval_pan_crop_smooth(&e.pan_crop, local);
    let mut f = draw_source_ex(
        src_ref,
        w,
        h,
        &key,
        e.maintain_aspect,
        e.pan_crop_stretch_to_fill,
    );
    if e.pan_crop_mask_enabled {
        if let Some(mask_data) = e.pan_crop_mask.as_deref() {
            let (path, feather, opacity, expansion) = mask_data.evaluate_at(local);
            let poly = path.flatten(16);
            let mut mask = crate::mask::rasterize_mask(&poly, f.w, f.h, expansion);
            if feather > 0.5 {
                crate::mask::apply_feather(&mut mask, f.w, f.h, feather);
            }
            crate::mask::apply_mask_to_frame(
                &mut f,
                &mask,
                mask_data.mode,
                opacity,
                mask_data.invert,
            );
        }
    }
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

fn stereo_camera_params(p: &Project, w: usize, eye: StereoEye) -> (f32, f32) {
    let wf = w as f32;
    let inter = if p.settings.stereo_3d_interocular.is_finite() {
        p.settings.stereo_3d_interocular.clamp(0.0, 0.25)
    } else {
        0.035
    };
    let conv = if p.settings.stereo_3d_convergence.is_finite() {
        p.settings.stereo_3d_convergence.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let cam_x = match eye {
        StereoEye::Center => 0.0,
        StereoEye::Left => -inter * 0.5 * wf,
        StereoEye::Right => inter * 0.5 * wf,
    };
    (cam_x, conv * wf)
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
    parent_3d: Option<&super::motion3d::Mat4>,
    bg: Option<&Frame>,
) -> Option<Frame> {
    let mut layer = render_track_eye(p, media, track, t, w, h, opts, eye)?;
    if !opts.bypass_fx {
        for fx in &track.fx {
            vfx::apply_with_aux(fx, &mut layer, bg, t, p.settings.fps);
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
    let (cam_x, conv_z) = stereo_camera_params(p, w, eye);
    if track.motion_3d || parent_3d.is_some() {
        let local_mat = super::motion3d::motion_to_mat4(&mk, w as f32, h as f32);
        let world_mat = match parent_3d {
            Some(pm) => super::motion3d::mat4_mul(pm, &local_mat),
            None => local_mat,
        };
        let needs_3d =
            parent_3d.is_some() || !super::motion3d::is_identity(&mk) || cam_x.abs() > 1e-5;
        if needs_3d {
            layer = super::motion3d::apply_motion_3d_mat(&layer, &world_mat, cam_x, conv_z);
        }
    } else if !motion_2d_is_identity(&mk) {
        layer = apply_motion(&layer, &mk);
    }
    if track.glow_2d_enabled {
        let gk = track.glow_2d.eval(t);
        layer = apply_shadow_glow_2d(&layer, &gk);
    }
    if track.shadow_2d_enabled {
        let sk = track.shadow_2d.eval(t);
        layer = apply_shadow_glow_2d(&layer, &sk);
    }
    Some(layer)
}

/// Ordena faixas 3D contíguas por profundidade Z do mundo (mais distantes atrás,
/// mais próximas na frente) preservando a ordem relativa de faixas 2D/máscaras.
fn sort_sibling_3d_runs<'a>(
    list: Vec<&'a Track>,
    t: Ticks,
    w: usize,
    h: usize,
    parent_3d: Option<&super::motion3d::Mat4>,
) -> Vec<&'a Track> {
    let mut out = Vec::with_capacity(list.len());
    let mut run: Vec<(&'a Track, f32)> = Vec::new();
    let flush = |run: &mut Vec<(&'a Track, f32)>, out: &mut Vec<&'a Track>| {
        if run.len() > 1 {
            // Z menor (mais negativo) fica atrás e é composto primeiro.
            run.sort_by(|a, b| a.1.total_cmp(&b.1));
        }
        out.extend(run.drain(..).map(|(tr, _)| tr));
    };
    for tr in list {
        if tr.kind == TrackKind::Video
            && tr.motion_3d
            && !tr.is_folder
            && !tr.is_adjustment()
            && tr.mask_mode == TrackMaskMode::None
        {
            let mk = tr.motion.eval(t);
            let lm = super::motion3d::motion_to_mat4(&mk, w as f32, h as f32);
            let wm = match parent_3d {
                Some(pm) => super::motion3d::mat4_mul(pm, &lm),
                None => lm,
            };
            run.push((tr, super::motion3d::world_center_z(&wm)));
        } else {
            flush(&mut run, &mut out);
            out.push(tr);
        }
    }
    flush(&mut run, &mut out);
    out
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
    parent_3d: Option<&super::motion3d::Mat4>,
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
        let ordered = sort_sibling_3d_runs(
            video_children.into_iter().rev().collect(),
            t,
            w,
            h,
            parent_3d,
        );
        for child in ordered {
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
                parent_3d,
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
        let Some(layer) =
            render_track_processed(p, media, track, t, w, h, opts, eye, parent_3d, Some(out))
        else {
            return;
        };
        if track.mask_mode != TrackMaskMode::None {
            apply_track_mask(out, &layer, track.mask_mode);
        } else {
            let op = track.opacity * track.env_value(EnvKind::Opacity, t);
            blend(out, &layer, track.blend_at(t), op);
        }
        return;
    }

    // Subárvore Pai/Filho (composição em grupo com máscara e movimento pai 2D ou 3D, VEGAS Pro pp. 740, 754).
    let pmk = track.parent_motion.eval(t);
    let own_parent_3d = track.parent_motion_3d.then(|| {
        let pm = super::motion3d::motion_to_mat4(&pmk, w as f32, h as f32);
        match parent_3d {
            Some(anc) => super::motion3d::mat4_mul(anc, &pm),
            None => pm,
        }
    });
    let child_parent_3d = own_parent_3d.as_ref().or(parent_3d);

    let mut group = Frame::new(w, h);
    let ordered_children = sort_sibling_3d_runs(
        video_children.into_iter().rev().collect(),
        t,
        w,
        h,
        child_parent_3d,
    );
    for child in ordered_children {
        if !p.is_track_effectively_muted(child.id) && child.mask_mode != TrackMaskMode::None {
            if !any_solo || child.solo {
                if let Some(mask) = render_track_processed(
                    p,
                    media,
                    child,
                    t,
                    w,
                    h,
                    opts,
                    eye,
                    child_parent_3d,
                    Some(&group),
                ) {
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
                child_parent_3d,
            );
        }
    }
    if track_active {
        if let Some(layer) = render_track_processed(
            p,
            media,
            track,
            t,
            w,
            h,
            opts,
            eye,
            child_parent_3d,
            Some(&group),
        ) {
            if track.mask_mode != TrackMaskMode::None {
                apply_track_mask(&mut group, &layer, track.mask_mode);
            } else {
                blend(&mut group, &layer, track.blend_at(t), 1.0);
            }
        }
    }
    if !track.parent_motion_3d && !motion_2d_is_identity(&pmk) {
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
    let top_level: Vec<&Track> = p
        .tracks
        .iter()
        .rev()
        .filter(|track| {
            !track
                .parent_track_id
                .is_some_and(|pid| pid != track.id && track_ids.contains(&pid))
        })
        .collect();
    let ordered = sort_sibling_3d_runs(top_level, t, w, h, None);
    for track in ordered {
        compose_subtree(
            p, media, track, t, w, h, opts, eye, any_solo, &mut out, 0, None,
        );
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
    fn video_event_opacity_scales_rendered_alpha() {
        let mut p = Project::new_default();
        let m = MediaItem {
            id: new_id(),
            name: "branco".into(),
            source: MediaSource::Generated(GenSpec::SolidColor {
                color: [1.0, 1.0, 1.0, 1.0],
            }),
            duration: Some(Ticks::from_secs_i64(10)),
            has_video: true,
            has_audio: false,
            width: 8,
            height: 8,
            fps: Some(FrameRate::FPS_30),
            info: String::new(),
            proxy_path: None,
        };
        let mid = m.id;
        p.media.push(m);
        let mut ev = Event::new(
            p.media(mid).expect("mídia presente"),
            Ticks::ZERO,
            Ticks::from_secs_i64(5),
        );
        ev.opacity = 0.5;
        p.tracks[0].events.push(ev);
        let cache = MediaCache::default();
        let f = render_frame_preview(&p, &cache, Ticks::from_secs_i64(1), 8, 8, false);
        let px = f.px[0];
        assert!(
            px[0] > 0.45 && px[0] < 0.55,
            "opacidade 0.5 do evento deve reduzir a luminância pela metade: {px:?}"
        );
        p.tracks[0].events[0].opacity = 0.0;
        let f0 = render_frame_preview(&p, &cache, Ticks::from_secs_i64(1), 8, 8, false);
        assert!(
            f0.px[0][0] < 0.02,
            "opacidade 0.0 do evento deve ficar transparente: {:?}",
            f0.px[0]
        );
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

#[cfg(test)]
mod pan_crop_tests {
    use super::*;

    /// Imagem 4x4: coluna da esquerda vermelha, linha de cima azul.
    fn src() -> RgbaImage {
        let mut img = RgbaImage::new(4, 4);
        for (x, y, p) in img.enumerate_pixels_mut() {
            let r = if x == 0 { 255 } else { 0 };
            let b = if y == 0 { 255 } else { 0 };
            *p = image::Rgba([r, 0, b, 255]);
        }
        img
    }

    #[test]
    fn espelhar_na_horizontal_traz_a_coluna_da_direita_para_a_esquerda() {
        let key = PanCropKey {
            flip_h: true,
            ..PanCropKey::default()
        };
        let f = draw_source(&src(), 4, 4, &key, false);
        // pixel (0,1): sem espelhar seria vermelho; espelhado vem da coluna 3
        assert!(f.px[4][0] < 0.1, "{:?}", f.px[4]);
        assert!(f.px[4 + 3][0] > 0.9, "{:?}", f.px[4 + 3]);
    }

    #[test]
    fn espelhar_na_vertical_traz_a_linha_de_baixo_para_cima() {
        let key = PanCropKey {
            flip_v: true,
            ..PanCropKey::default()
        };
        let f = draw_source(&src(), 4, 4, &key, false);
        assert!(f.px[1][2] < 0.1, "{:?}", f.px[1]);
        assert!(f.px[3 * 4 + 1][2] > 0.9, "{:?}", f.px[3 * 4 + 1]);
    }

    #[test]
    fn proporcao_maior_mostra_mais_altura_da_origem() {
        // ratio 2: o enquadramento cobre o dobro da altura do quadro, então
        // a imagem aparece achatada na vertical e sobra área vazia em cima
        let key = PanCropKey {
            ratio: 2.0,
            ..PanCropKey::default()
        };
        let f = draw_source(&src(), 4, 4, &key, false);
        assert!(
            f.px[0][3] < 0.1,
            "linha 0 fica fora da origem: {:?}",
            f.px[0]
        );
        assert!(f.px[4 * 2 + 2][3] > 0.9);
    }

    #[test]
    fn proporcao_hostil_nao_trava() {
        for r in [f32::NAN, f32::INFINITY, -5.0, 0.0, 1e30] {
            let key = PanCropKey {
                ratio: r,
                ..PanCropKey::default()
            };
            let f = draw_source(&src(), 4, 4, &key, false);
            assert_eq!(f.px.len(), 16);
        }
    }

    #[test]
    fn projeto_antigo_sem_os_campos_novos_abre_igual() {
        let k: PanCropKey =
            serde_json::from_str(r#"{"cx":0.4,"cy":0.6,"size":0.5,"rot":10.0}"#).unwrap();
        assert_eq!(k.ratio, 1.0);
        assert!(!k.flip_h && !k.flip_v);
        assert_eq!(k.size, 0.5);
        assert_eq!(k.smoothness, 0.0);
    }

    #[test]
    fn stretch_to_fill_false_mantem_pixels_nativos_sem_esticar() {
        let mut small = RgbaImage::new(4, 4);
        for px in small.pixels_mut() {
            *px = image::Rgba([255, 0, 0, 255]);
        }
        let f = draw_source_ex(&small, 16, 16, &PanCropKey::default(), true, false);
        // Centro tem vermelho opaco; borda permanece transparente porque não esticou 4x4 para 16x16
        assert!(f.px[8 * 16 + 8][0] > 0.9);
        assert!(f.px[0][3] < 0.05);
    }

    #[test]
    fn sombra_2d_desenha_halo_fora_do_bloco_central() {
        let mut layer = Frame::new(24, 24);
        for y in 8..16 {
            for x in 8..16 {
                layer.px[y * 24 + x] = [1.0, 1.0, 1.0, 1.0];
            }
        }
        let sk = ShadowGlowKey {
            offset_x: 0.12,
            offset_y: 0.12,
            blur: 0.4,
            intensity: 0.9,
            color: [0.0, 0.0, 1.0, 1.0],
        };
        let out = apply_shadow_glow_2d(&layer, &sk);
        // Pixel deslocado fora do quadrado central deve receber alfa azul da sombra
        assert!(out.px[17 * 24 + 17][3] > 0.05);
        assert!(out.px[17 * 24 + 17][2] > 0.02);
        // Centro continua branco opaco na frente da sombra
        assert!(out.px[12 * 24 + 12][0] > 0.95);
    }

    #[test]
    fn ordenacao_z_entre_trilhas_3d_traz_mais_proxima_a_frente() {
        let mut p = Project::new_default();
        p.tracks.clear();
        let imp_r = crate::media::import_generated(GenSpec::SolidColor {
            color: [1.0, 0.0, 0.0, 1.0],
        });
        let imp_b = crate::media::import_generated(GenSpec::SolidColor {
            color: [0.0, 0.0, 1.0, 1.0],
        });
        let media = MediaCache::default();
        media.insert(imp_r.item.id, imp_r.entry);
        media.insert(imp_b.item.id, imp_b.entry);
        p.media.push(imp_r.item.clone());
        p.media.push(imp_b.item.clone());

        // Trilha superior (índice 0) está mais LONGE (Z = -0.3, vermelha)
        let mut t_top = Track::new(TrackKind::Video, "Longe");
        t_top.motion_3d = true;
        t_top.motion.keys[0].value.z = -0.3;
        t_top.events.push(Event::new(
            &imp_r.item,
            Ticks::ZERO,
            Ticks::from_secs_i64(2),
        ));

        // Trilha inferior (índice 1) está mais PERTO (Z = +0.2, azul)
        let mut t_bot = Track::new(TrackKind::Video, "Perto");
        t_bot.motion_3d = true;
        t_bot.motion.keys[0].value.z = 0.2;
        t_bot.events.push(Event::new(
            &imp_b.item,
            Ticks::ZERO,
            Ticks::from_secs_i64(2),
        ));

        p.tracks.push(t_top);
        p.tracks.push(t_bot);

        let frame = render_frame(&p, &media, Ticks::ZERO, 16, 16);
        let center = frame.px[8 * 16 + 8];
        assert!(
            center[2] > 0.9 && center[0] < 0.1,
            "trilha 3D mais próxima deve ficar à frente: {center:?}"
        );
    }
}
