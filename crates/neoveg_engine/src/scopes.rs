//! Escopos de vídeo (Video Scopes): histograma, forma de onda (waveform),
//! vectorscope e RGB parade.

// Laços indexam vários buffers paralelos (canais/amostras) ao mesmo tempo;
// iteradores encadeados deixariam o DSP menos legível.
#![allow(clippy::needless_range_loop)]

use crate::engine::Frame;
use image::{Rgba, RgbaImage};

pub struct ScopeImages {
    pub histogram: RgbaImage,
    pub waveform: RgbaImage,
    pub vectorscope: RgbaImage,
    pub parade: RgbaImage,
}

fn img(w: usize, h: usize) -> RgbaImage {
    RgbaImage::from_pixel(w as u32, h as u32, Rgba([12, 12, 14, 255]))
}

fn add(img: &mut RgbaImage, x: usize, y: usize, c: [u8; 3], k: u8) {
    let Some(p) = img.get_pixel_mut_checked(x as u32, y as u32) else {
        return;
    };
    let s = |v: u8| ((v as u16 * k as u16) / 255) as u8;
    p.0 = [
        p.0[0].saturating_add(s(c[0])),
        p.0[1].saturating_add(s(c[1])),
        p.0[2].saturating_add(s(c[2])),
        255,
    ];
}

pub fn compute(f: &Frame) -> ScopeImages {
    // subamostra para manter custo baixo
    let step = ((f.w * f.h) as f32 / 120_000.0).sqrt().max(1.0) as usize;
    let mut hist = [[0u32; 256]; 4];
    let ww = 256usize;
    let wh = 200usize;
    let mut wf = vec![0u32; ww * wh];
    let mut par = vec![[0u32; 3]; ww * wh]; // cada terço da largura = um canal
    let vs = 200usize;
    let mut vec_acc = vec![0u32; vs * vs];
    for y in (0..f.h).step_by(step) {
        for x in (0..f.w).step_by(step) {
            let p = f.px[y * f.w + x];
            let (r, g, b) = (
                p[0].clamp(0.0, 1.0),
                p[1].clamp(0.0, 1.0),
                p[2].clamp(0.0, 1.0),
            );
            let l = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            hist[0][(r * 255.0) as usize] += 1;
            hist[1][(g * 255.0) as usize] += 1;
            hist[2][(b * 255.0) as usize] += 1;
            hist[3][(l * 255.0) as usize] += 1;
            let col = x * ww / f.w;
            let row = |v: f32| ((1.0 - v) * (wh - 1) as f32) as usize;
            wf[row(l) * ww + col] += 1;
            let pc = (x * (ww / 3)) / f.w;
            par[row(r) * ww + pc][0] += 1;
            par[row(g) * ww + pc + ww / 3][1] += 1;
            par[row(b) * ww + pc + 2 * (ww / 3)][2] += 1;
            let cb = (b - l) * 0.5389;
            let cr = (r - l) * 0.6350;
            let vx = ((cb + 0.5) * (vs - 1) as f32).clamp(0.0, (vs - 1) as f32) as usize;
            let vy = ((0.5 - cr) * (vs - 1) as f32).clamp(0.0, (vs - 1) as f32) as usize;
            vec_acc[vy * vs + vx] += 1;
        }
    }
    // histograma
    let (hw, hh) = (256usize, 120usize);
    let mut histogram = img(hw, hh);
    let maxv = hist
        .iter()
        .flat_map(|h| h.iter())
        .cloned()
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    let colors = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]];
    for (ci, h) in hist.iter().enumerate() {
        for x in 0..256 {
            let v = ((h[x] as f32 / maxv).sqrt() * (hh - 1) as f32) as usize;
            for y in (hh - 1 - v)..hh {
                add(
                    &mut histogram,
                    x,
                    y,
                    colors[ci],
                    if ci == 3 { 50 } else { 90 },
                );
            }
        }
    }
    let intensity = |n: u32| -> u8 { ((n as f32).sqrt() * 40.0).min(255.0) as u8 };
    let mut waveform = img(ww, wh);
    for (dst, &count) in waveform
        .as_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(wf.iter())
    {
        let k = intensity(count);
        if k > 0 {
            *dst = [k / 3, k, k / 3, 255];
        }
    }
    let mut parade = img(ww, wh);
    for (dst, &[r, g, b]) in parade
        .as_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(par.iter())
    {
        if r + g + b > 0 {
            *dst = [intensity(r), intensity(g), intensity(b), 255];
        }
    }
    // linhas de referência 0/50/100 IRE
    for y in [0, wh / 2, wh - 1] {
        for x in (0..ww).step_by(2) {
            add(&mut waveform, x, y, [255, 255, 255], 40);
            add(&mut parade, x, y, [255, 255, 255], 40);
        }
    }
    let mut vectorscope = img(vs, vs);
    for (dst, &count) in vectorscope
        .as_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(vec_acc.iter())
    {
        let k = intensity(count);
        if k > 0 {
            *dst = [k / 2, k, k / 2, 255];
        }
    }
    // círculo e alvos das cores primárias (75%)
    let c = vs as f32 / 2.0;
    for a in 0..360 {
        let r = a as f32 * std::f32::consts::PI / 180.0;
        let x = (c + r.cos() * (c - 2.0)) as usize;
        let y = (c + r.sin() * (c - 2.0)) as usize;
        add(
            &mut vectorscope,
            x.min(vs - 1),
            y.min(vs - 1),
            [255, 255, 255],
            60,
        );
    }
    for col in [
        [0.75, 0.0, 0.0],
        [0.0, 0.75, 0.0],
        [0.0, 0.0, 0.75],
        [0.75, 0.75, 0.0],
        [0.0, 0.75, 0.75],
        [0.75, 0.0, 0.75],
    ] {
        let l = 0.2126 * col[0] + 0.7152 * col[1] + 0.0722 * col[2];
        let cb = (col[2] - l) * 0.5389;
        let cr = (col[0] - l) * 0.6350;
        let vx = ((cb + 0.5) * (vs - 1) as f32) as i32;
        let vy = ((0.5 - cr) * (vs - 1) as f32) as i32;
        for d in -3..=3 {
            for (xx, yy) in [
                (vx + d, vy - 3),
                (vx + d, vy + 3),
                (vx - 3, vy + d),
                (vx + 3, vy + d),
            ] {
                if xx >= 0 && yy >= 0 && (xx as usize) < vs && (yy as usize) < vs {
                    add(
                        &mut vectorscope,
                        xx as usize,
                        yy as usize,
                        [255, 200, 80],
                        120,
                    );
                }
            }
        }
    }
    ScopeImages {
        histogram,
        waveform,
        vectorscope,
        parade,
    }
}
