//! Geradores de mídia (Media Generators): cor sólida, gradientes, xadrez,
//! barras de cor, ruído e títulos/texto com animação.

use crate::model::{GenSpec, TextAlign, TextAnim, TextSpec};
use ab_glyph::{point, Font, FontArc, PxScale, ScaleFont};
use image::RgbaImage;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Default)]
pub struct Fonts {
    cache: Mutex<HashMap<String, Option<FontArc>>>,
}

const SYSTEM_FONTS: [&str; 4] = [
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "C:\\Windows\\Fonts\\arial.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
];

impl Fonts {
    pub fn get(&self, path: &str) -> Option<FontArc> {
        let mut c = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(f) = c.get(path) {
            return f.clone();
        }
        let f = if path.is_empty() {
            default_font()
        } else {
            std::fs::read(path)
                .ok()
                .and_then(|b| FontArc::try_from_vec(b).ok())
                .or_else(default_font)
        };
        c.insert(path.to_string(), f.clone());
        f
    }
}

fn default_font() -> Option<FontArc> {
    for p in SYSTEM_FONTS {
        if let Ok(b) = std::fs::read(p) {
            if let Ok(f) = FontArc::try_from_vec(b) {
                return Some(f);
            }
        }
    }
    // fonte embutida do egui (Ubuntu-Light)
    let defs = eframe::egui::FontDefinitions::default();
    let fd = defs.font_data.get("Ubuntu-Light")?;
    FontArc::try_from_vec(fd.font.to_vec()).ok()
}

/// Lista fontes TTF/OTF instaladas no sistema (para o seletor de fonte).
pub fn list_system_fonts() -> Vec<String> {
    let mut out = vec![];
    for dir in [
        "C:\\Windows\\Fonts",
        "/usr/share/fonts/truetype",
        "/Library/Fonts",
    ] {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                let ext = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if ext == "ttf" || ext == "otf" {
                    out.push(p.to_string_lossy().to_string());
                }
            }
        }
    }
    out.sort();
    out
}

fn to8(c: f32) -> u8 {
    (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn rgba8(c: [f32; 4]) -> [u8; 4] {
    [to8(c[0]), to8(c[1]), to8(c[2]), to8(c[3])]
}

fn hash32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// Gera a imagem da mídia no tamanho pedido, no tempo local `t` (segundos).
pub fn render(spec: &GenSpec, t: f64, w: u32, h: u32, fonts: &Fonts) -> RgbaImage {
    let (w, h) = (w.max(1), h.max(1));
    match spec {
        GenSpec::SolidColor { color } => RgbaImage::from_pixel(w, h, image::Rgba(rgba8(*color))),
        GenSpec::Gradient {
            a,
            b,
            angle,
            radial,
        } => {
            let mut img = RgbaImage::new(w, h);
            let (ca, sa) = (angle.to_radians().cos(), angle.to_radians().sin());
            for (x, y, p) in img.enumerate_pixels_mut() {
                let u = x as f32 / w as f32 - 0.5;
                let v = y as f32 / h as f32 - 0.5;
                let k = if *radial {
                    ((u * u + v * v).sqrt() / std::f32::consts::FRAC_1_SQRT_2).clamp(0.0, 1.0)
                } else {
                    (u * ca + v * sa + 0.5).clamp(0.0, 1.0)
                };
                let c = [
                    a[0] + (b[0] - a[0]) * k,
                    a[1] + (b[1] - a[1]) * k,
                    a[2] + (b[2] - a[2]) * k,
                    a[3] + (b[3] - a[3]) * k,
                ];
                *p = image::Rgba(rgba8(c));
            }
            img
        }
        GenSpec::Checkerboard { a, b, cells } => {
            let cells = (*cells).max(1);
            let cs = (h as f32 / cells as f32).max(1.0);
            let (pa, pb) = (rgba8(*a), rgba8(*b));
            RgbaImage::from_fn(w, h, |x, y| {
                let cx = (x as f32 / cs) as u32;
                let cy = (y as f32 / cs) as u32;
                image::Rgba(if (cx + cy).is_multiple_of(2) { pa } else { pb })
            })
        }
        GenSpec::ColorBars => {
            // barras SMPTE simplificadas (75%)
            let bars: [[u8; 3]; 7] = [
                [191, 191, 191],
                [191, 191, 0],
                [0, 191, 191],
                [0, 191, 0],
                [191, 0, 191],
                [191, 0, 0],
                [0, 0, 191],
            ];
            let low: [[u8; 3]; 7] = [
                [0, 0, 191],
                [19, 19, 19],
                [191, 0, 191],
                [19, 19, 19],
                [0, 191, 191],
                [19, 19, 19],
                [191, 191, 191],
            ];
            RgbaImage::from_fn(w, h, |x, y| {
                let i = ((x as usize * 7) / w as usize).min(6);
                let c = if y < h * 2 / 3 {
                    bars[i]
                } else if y < h * 3 / 4 {
                    low[i]
                } else {
                    let j = (x * 6 / w) as usize;
                    match j {
                        0 => [0, 33, 76],
                        1 => [255, 255, 255],
                        2 => [50, 0, 106],
                        _ => [19, 19, 19],
                    }
                };
                image::Rgba([c[0], c[1], c[2], 255])
            })
        }
        GenSpec::Noise { mono, animated } => {
            let seed = if *animated { (t * 30.0) as u32 } else { 7 };
            RgbaImage::from_fn(w, h, |x, y| {
                let base = hash32(
                    x.wrapping_mul(73_856_093)
                        ^ y.wrapping_mul(19_349_663)
                        ^ seed.wrapping_mul(83_492_791),
                );
                if *mono {
                    let v = (base & 0xff) as u8;
                    image::Rgba([v, v, v, 255])
                } else {
                    image::Rgba([
                        (base & 0xff) as u8,
                        (base >> 8) as u8,
                        (base >> 16) as u8,
                        255,
                    ])
                }
            })
        }
        GenSpec::Text(ts) => render_text(ts, t, w, h, fonts),
    }
}

struct Laid {
    mask: Vec<f32>,
    w: usize,
    h: usize,
}

/// Rasteriza linhas de texto numa máscara de cobertura do tamanho do quadro.
fn layout_mask(
    ts: &TextSpec,
    text: &str,
    font: &FontArc,
    w: u32,
    h: u32,
    offset: (f32, f32),
) -> Laid {
    let (wu, hu) = (w as usize, h as usize);
    let mut mask = vec![0f32; wu * hu];
    let px = (ts.size / 100.0 * h as f32).max(4.0);
    let scale = PxScale::from(px);
    let sf = font.as_scaled(scale);
    let line_h = (sf.ascent() - sf.descent() + sf.line_gap()) * ts.line_spacing.max(0.5);
    let lines: Vec<&str> = text.split('\n').collect();
    let widths: Vec<f32> = lines
        .iter()
        .map(|l| {
            let mut x = 0.0;
            let mut prev = None;
            for c in l.chars() {
                let g = font.glyph_id(c);
                if let Some(p) = prev {
                    x += sf.kern(p, g);
                }
                x += sf.h_advance(g);
                prev = Some(g);
            }
            x
        })
        .collect();
    let block_w = widths.iter().cloned().fold(0.0, f32::max);
    let block_h = line_h * lines.len() as f32;
    let cx = ts.x * w as f32 + offset.0;
    let cy = ts.y * h as f32 + offset.1;
    let top = cy - block_h / 2.0;
    for (li, line) in lines.iter().enumerate() {
        let lw = widths[li];
        let x0 = match ts.align {
            TextAlign::Left => cx - block_w / 2.0,
            TextAlign::Center => cx - lw / 2.0,
            TextAlign::Right => cx + block_w / 2.0 - lw,
        };
        let baseline = top + li as f32 * line_h + sf.ascent();
        if baseline + line_h < 0.0 || baseline - line_h > h as f32 {
            continue;
        }
        let mut x = x0;
        let mut prev = None;
        for c in line.chars() {
            let gid = font.glyph_id(c);
            if let Some(p) = prev {
                x += sf.kern(p, gid);
            }
            let glyph = gid.with_scale_and_position(scale, point(x, baseline));
            x += sf.h_advance(gid);
            prev = Some(gid);
            if let Some(og) = font.outline_glyph(glyph) {
                let b = og.px_bounds();
                og.draw(|gx, gy, cov| {
                    let px = b.min.x as i32 + gx as i32;
                    let py = b.min.y as i32 + gy as i32;
                    if px >= 0 && py >= 0 && (px as usize) < wu && (py as usize) < hu {
                        let m = &mut mask[py as usize * wu + px as usize];
                        *m = (*m + cov).min(1.0);
                    }
                });
            }
        }
    }
    Laid { mask, w: wu, h: hu }
}

/// Dilatação (max) separável com raio r.
fn dilate(m: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    if r == 0 {
        return m.to_vec();
    }
    let mut tmp = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let a = x.saturating_sub(r);
            let b = (x + r).min(w - 1);
            let mut v = 0f32;
            for xx in a..=b {
                v = v.max(m[y * w + xx]);
            }
            tmp[y * w + x] = v;
        }
    }
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        let a = y.saturating_sub(r);
        let b = (y + r).min(h - 1);
        for x in 0..w {
            let mut v = 0f32;
            for yy in a..=b {
                v = v.max(tmp[yy * w + x]);
            }
            out[y * w + x] = v;
        }
    }
    out
}

pub fn box_blur_mask(m: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    if r == 0 {
        return m.to_vec();
    }
    let norm = 1.0 / (2 * r + 1) as f32;
    let mut tmp = vec![0f32; w * h];
    for y in 0..h {
        let row = &m[y * w..(y + 1) * w];
        let mut acc = 0f32;
        acc += row.iter().take(r.min(w - 1) + 1).sum::<f32>();
        acc += row[0] * r as f32;
        for x in 0..w {
            tmp[y * w + x] = acc * norm;
            let add = row[(x + r + 1).min(w - 1)];
            let sub = row[x.saturating_sub(r)];
            acc += add - sub;
        }
    }
    let mut out = vec![0f32; w * h];
    for x in 0..w {
        let mut acc = 0f32;
        for i in 0..=r.min(h - 1) {
            acc += tmp[i * w + x];
        }
        acc += tmp[x] * r as f32;
        for y in 0..h {
            out[y * w + x] = acc * norm;
            let add = tmp[(y + r + 1).min(h - 1) * w + x];
            let sub = tmp[y.saturating_sub(r) * w + x];
            acc += add - sub;
        }
    }
    out
}

fn render_text(ts: &TextSpec, t: f64, w: u32, h: u32, fonts: &Fonts) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(w, h, image::Rgba(rgba8(ts.background)));
    let Some(font) = fonts.get(&ts.font_path) else {
        return img;
    };
    let dur = ts.anim_duration.max(0.1) as f64;
    let prog = (t / dur).clamp(0.0, 1.0) as f32;
    let mut text = ts.text.clone();
    let mut offset = (0.0f32, 0.0f32);
    let mut alpha = 1.0f32;
    match ts.anim {
        TextAnim::None => {}
        TextAnim::ScrollUp => {
            let lines = ts.text.split('\n').count() as f32;
            let block = lines * ts.size / 100.0 * h as f32 * ts.line_spacing * 1.2;
            let travel = h as f32 + block;
            // começa abaixo do quadro e termina acima
            let start = h as f32 * (1.0 - ts.y) + block / 2.0;
            offset.1 = start - travel * prog;
        }
        TextAnim::CrawlLeft => {
            let approx_w = ts.text.chars().count() as f32 * ts.size / 100.0 * h as f32 * 0.55;
            let start = w as f32 * (1.0 - ts.x) + approx_w / 2.0;
            offset.0 = start - (w as f32 + approx_w) * prog;
        }
        TextAnim::Typewriter => {
            let n = ts.text.chars().count();
            let shown = ((prog / 0.8).min(1.0) * n as f32).round() as usize;
            text = ts.text.chars().take(shown).collect();
        }
        TextAnim::FadeInOut => {
            let edge = 0.2;
            alpha = if prog < edge {
                prog / edge
            } else if prog > 1.0 - edge {
                (1.0 - prog) / edge
            } else {
                1.0
            };
        }
    }
    let laid = layout_mask(ts, &text, &font, w, h, offset);
    let (wu, hu) = (laid.w, laid.h);
    let outline_px = (ts.outline / 100.0 * h as f32).round() as usize;
    let outline = if outline_px > 0 {
        Some(box_blur_mask(
            &dilate(&laid.mask, wu, hu, outline_px),
            wu,
            hu,
            1,
        ))
    } else {
        None
    };
    let shadow = if ts.shadow {
        let base = outline.as_ref().unwrap_or(&laid.mask);
        let r = ((h as f32) * 0.006).max(1.0) as usize;
        let blurred = box_blur_mask(base, wu, hu, r);
        let off = ((h as f32) * 0.006).max(1.0) as usize;
        let mut s = vec![0f32; wu * hu];
        for y in off..hu {
            for x in off..wu {
                s[y * wu + x] = blurred[(y - off) * wu + (x - off)];
            }
        }
        Some(s)
    } else {
        None
    };
    let fill = ts.color;
    let oc = ts.outline_color;
    for (i, px) in img.as_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let mut c = [
            px[0] as f32 / 255.0,
            px[1] as f32 / 255.0,
            px[2] as f32 / 255.0,
            px[3] as f32 / 255.0,
        ];
        let mut over = |col: [f32; 3], a: f32| {
            let a = a.clamp(0.0, 1.0) * alpha;
            if a <= 0.0 {
                return;
            }
            let out_a = a + c[3] * (1.0 - a);
            for k in 0..3 {
                c[k] = if out_a > 0.0 {
                    (col[k] * a + c[k] * c[3] * (1.0 - a)) / out_a
                } else {
                    0.0
                };
            }
            c[3] = out_a;
        };
        if let Some(s) = &shadow {
            over([0.0, 0.0, 0.0], s[i] * 0.65);
        }
        if let Some(o) = &outline {
            over([oc[0], oc[1], oc[2]], o[i] * oc[3]);
        }
        over([fill[0], fill[1], fill[2]], laid.mask[i] * fill[3]);
        px.copy_from_slice(&[to8(c[0]), to8(c[1]), to8(c[2]), to8(c[3])]);
    }
    img
}
