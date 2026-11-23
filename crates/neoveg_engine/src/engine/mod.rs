//! Motor de renderização: composição de vídeo e mixagem de áudio.

pub mod afx;
pub mod audio;
pub mod fx_preview;
pub mod loudness;
pub mod motion3d;
pub mod scene_detect;
pub mod surround;
pub mod transition;
pub mod vfx;
mod vfx_extra;
pub mod video;

use image::RgbaImage;
use rayon::prelude::*;

/// Quadro de vídeo RGBA em ponto flutuante com alfa pré-multiplicado.
#[derive(Clone)]
pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[f32; 4]>,
}

impl Frame {
    pub fn new(w: usize, h: usize) -> Self {
        Frame {
            w,
            h,
            px: vec![[0.0; 4]; w * h],
        }
    }

    pub fn filled(w: usize, h: usize, c: [f32; 4]) -> Self {
        let pc = [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]];
        Frame {
            w,
            h,
            px: vec![pc; w * h],
        }
    }

    #[inline]
    pub fn get(&self, x: isize, y: isize) -> [f32; 4] {
        if x < 0 || y < 0 || x >= self.w as isize || y >= self.h as isize {
            return [0.0; 4];
        }
        self.px
            .get(y as usize * self.w + x as usize)
            .copied()
            .unwrap_or([0.0; 4])
    }

    #[inline]
    pub fn get_clamped(&self, x: isize, y: isize) -> [f32; 4] {
        if self.w == 0 || self.h == 0 {
            return [0.0; 4];
        }
        let x = x.max(0).min(self.w as isize - 1) as usize;
        let y = y.max(0).min(self.h as isize - 1) as usize;
        self.px.get(y * self.w + x).copied().unwrap_or([0.0; 4])
    }

    /// Amostragem bilinear (coordenadas em pixels, centro do pixel em +0.5).
    #[inline]
    pub fn sample(&self, x: f32, y: f32) -> [f32; 4] {
        if self.w == 0 || self.h == 0 || !x.is_finite() || !y.is_finite() {
            return [0.0; 4];
        }
        let fx = x - 0.5;
        let fy = y - 0.5;
        let x0 = fx.floor();
        let y0 = fy.floor();
        let ax = fx - x0;
        let ay = fy - y0;
        let (x0, y0) = (x0 as isize, y0 as isize);
        let x1 = x0.saturating_add(1);
        let y1 = y0.saturating_add(1);
        let p00 = self.get(x0, y0);
        let p10 = self.get(x1, y0);
        let p01 = self.get(x0, y1);
        let p11 = self.get(x1, y1);
        let mut o = [0.0; 4];
        for k in 0..4 {
            let a = p00[k] + (p10[k] - p00[k]) * ax;
            let b = p01[k] + (p11[k] - p01[k]) * ax;
            o[k] = a + (b - a) * ay;
        }
        o
    }

    pub fn scale_alpha(&mut self, a: f32) {
        if (a - 1.0).abs() < 1e-6 {
            return;
        }
        self.px.par_iter_mut().for_each(|p| {
            for c in p.iter_mut() {
                *c *= a;
            }
        });
    }

    /// Composição "over" (alfa da origem).
    pub fn over(&mut self, top: &Frame) {
        self.px
            .par_iter_mut()
            .zip(top.px.par_iter())
            .for_each(|(d, s)| {
                let ia = 1.0 - s[3];
                for k in 0..4 {
                    d[k] = s[k] + d[k] * ia;
                }
            });
    }

    /// Converte para RGBA 8 bits (não pré-multiplicado).
    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.w * self.h * 4];
        out.par_chunks_mut(4)
            .zip(self.px.par_iter())
            .for_each(|(o, p)| {
                let a = p[3].clamp(0.0, 1.0);
                let inv = if a > 1e-6 { 1.0 / a } else { 0.0 };
                o[0] = to8(p[0] * inv);
                o[1] = to8(p[1] * inv);
                o[2] = to8(p[2] * inv);
                o[3] = to8(a);
            });
        out
    }

    /// Converte para RGB 8 bits composto sobre preto (saída final).
    pub fn to_rgb8_opaque(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.w * self.h * 3];
        out.par_chunks_mut(3)
            .zip(self.px.par_iter())
            .for_each(|(o, p)| {
                o[0] = to8(p[0]);
                o[1] = to8(p[1]);
                o[2] = to8(p[2]);
            });
        out
    }

    pub fn to_image(&self) -> Option<RgbaImage> {
        RgbaImage::from_raw(self.w as u32, self.h as u32, self.to_rgba8())
    }
}

#[inline]
pub fn to8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

#[inline]
pub fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

#[inline]
pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    if (b - a).abs() < 1e-9 {
        return if x < a { 0.0 } else { 1.0 };
    }
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
