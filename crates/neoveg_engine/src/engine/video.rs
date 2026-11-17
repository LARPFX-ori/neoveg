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
    event_layer_opts(p, media, e, t, w, h, false)
}

fn event_layer_opts(
    p: &Project,
    media: &MediaCache,
    e: &Event,
    t: Ticks,
    w: usize,
    h: usize,
    prefer_proxy: bool,
) -> Option<Frame> {
    let m = p.media(e.media_id)?;
    if !m.has_video {
        return None;
    }
    let mt = e.media_time(t, m.duration)?;
    let src = media.video_frame_for_quality(m, mt, w as u32, h as u32, prefer_proxy)?;
    let local = t - e.start;
    let key = e.pan_crop.eval(local);
    let mut f = draw_source(&src, w, h, &key, e.maintain_aspect);
    for fx in &e.fx {
        vfx::apply(fx, &mut f, local, p.settings.fps);
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
    prefer_proxy: bool,
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
        if let Some(mut f) = event_layer_opts(p, media, e, t, w, h, prefer_proxy) {
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
        let fa = event_layer_opts(p, media, a, t, w, h, prefer_proxy).map(|mut f| {
            f.scale_alpha(event_alpha(a, t, false, true));
            f
        });
        let fb = event_layer_opts(p, media, b, t, w, h, prefer_proxy).map(|mut f| {
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
            (Some(tr), true) => {
                event_layer_opts(p, media, e, t, w, h, prefer_proxy).map(|mut f| {
                    f.scale_alpha(event_alpha(e, t, true, false));
                    let prog = e
                        .fade_in
                        .curve
                        .shape(local.fraction_in(Ticks::ZERO, e.fade_in.len));
                    transition::apply(tr.kind, tr.softness, &Frame::new(w, h), &f, prog)
                })
            }
            _ => event_layer_opts(p, media, e, t, w, h, prefer_proxy).map(|mut f| {
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
    let bg = p.settings.background;
    let mut out = Frame::filled(w, h, [bg[0], bg[1], bg[2], 1.0]);
    let any_solo = p.any_solo(TrackKind::Video);
    for track in p.tracks.iter().rev() {
        if track.kind != TrackKind::Video || !track.is_audible(any_solo) {
            continue;
        }
        if track.is_adjustment() {
            apply_adjustment(track, &mut out, t, p.settings.fps);
            continue;
        }
        let Some(mut layer) = render_track(p, media, track, t, w, h, prefer_proxy) else {
            continue;
        };
        for fx in &track.fx {
            vfx::apply(fx, &mut layer, t, p.settings.fps);
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
        vfx::apply(fx, &mut out, t, p.settings.fps);
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
}
