//! Escopos de vídeo (Video Scopes): histograma, forma de onda (waveform),
//! vectorscope e RGB parade com opções configuráveis de escala, modos e alvos.

// Laços indexam vários buffers paralelos (canais/amostras) ao mesmo tempo;
// iteradores encadeados deixariam o DSP menos legível.
#![allow(clippy::needless_range_loop)]

use crate::engine::Frame;
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

/// Escala de níveis da Forma de Onda (Waveform Scale).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum WaveformScale {
    /// Escala IRE canônica do VEGAS Pro (-20 a 120 IRE).
    #[default]
    Ire,
    /// Milivolts (-140 mV a 714 mV).
    MilliVolts,
    /// Níveis digitais de 8 bits (0 a 255).
    Digital8Bit,
    /// Ponto flutuante normalizado (0.0 a 1.0).
    Float,
}

impl WaveformScale {
    pub const ALL: [WaveformScale; 4] = [
        WaveformScale::Ire,
        WaveformScale::MilliVolts,
        WaveformScale::Digital8Bit,
        WaveformScale::Float,
    ];

    pub fn label(self) -> &'static str {
        match self {
            WaveformScale::Ire => "IRE (-20 a 120)",
            WaveformScale::MilliVolts => "Milivolts (mV)",
            WaveformScale::Digital8Bit => "Digital (8-bit)",
            WaveformScale::Float => "Float (0.0 a 1.0)",
        }
    }
}

/// Modo de exibição da Forma de Onda.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum WaveformMode {
    /// Apenas luminância (Y).
    #[default]
    Luminance,
    /// Sobreposição aditiva dos canais R, G, B.
    RgbOverlay,
    /// Sinal YC (Y com envelope de crominância).
    YC,
    /// Sinal YC sem modulação de crominância.
    YCNoChroma,
}

impl WaveformMode {
    pub const ALL: [WaveformMode; 4] = [
        WaveformMode::Luminance,
        WaveformMode::RgbOverlay,
        WaveformMode::YC,
        WaveformMode::YCNoChroma,
    ];

    pub fn label(self) -> &'static str {
        match self {
            WaveformMode::Luminance => "Luminância",
            WaveformMode::RgbOverlay => "Sobreposição RGB",
            WaveformMode::YC => "YC (Com Croma)",
            WaveformMode::YCNoChroma => "YC (Sem Croma)",
        }
    }
}

/// Alvos de saturação do Vectorscope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum VectorscopeTarget {
    /// Alvos de 75% (padrão de barras de cores SMPTE / EBU).
    #[default]
    Percent75,
    /// Alvos de 100% de saturação.
    Percent100,
}

impl VectorscopeTarget {
    pub fn amplitude(self) -> f32 {
        match self {
            VectorscopeTarget::Percent75 => 0.75,
            VectorscopeTarget::Percent100 => 1.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            VectorscopeTarget::Percent75 => "75%",
            VectorscopeTarget::Percent100 => "100%",
        }
    }
}

/// Modo de exibição do Parade.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ParadeMode {
    /// Canais R, G e B lado a lado.
    #[default]
    Rgb,
    /// Canais Y, Cb e Cr lado a lado.
    Yuv,
    /// R, G, B e canal Branco (Luminância) como 4ª coluna.
    RgbWhite,
}

impl ParadeMode {
    pub const ALL: [ParadeMode; 3] = [ParadeMode::Rgb, ParadeMode::Yuv, ParadeMode::RgbWhite];

    pub fn label(self) -> &'static str {
        match self {
            ParadeMode::Rgb => "RGB",
            ParadeMode::Yuv => "YUV",
            ParadeMode::RgbWhite => "RGB + Branco",
        }
    }
}

/// Matriz de conversão de cores para cálculo de luma e croma.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ScopeColorMatrix {
    /// Rec. 601 (SD / NTSC / PAL).
    Rec601,
    /// Rec. 709 (HD / SDR padrão).
    #[default]
    Rec709,
    /// Rec. 2020 (UHD / HDR / Wide Color Gamut).
    Rec2020,
}

impl ScopeColorMatrix {
    pub const ALL: [ScopeColorMatrix; 3] = [
        ScopeColorMatrix::Rec709,
        ScopeColorMatrix::Rec601,
        ScopeColorMatrix::Rec2020,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ScopeColorMatrix::Rec601 => "Rec. 601",
            ScopeColorMatrix::Rec709 => "Rec. 709",
            ScopeColorMatrix::Rec2020 => "Rec. 2020",
        }
    }

    pub fn luma_weights(self) -> (f32, f32, f32) {
        match self {
            ScopeColorMatrix::Rec601 => (0.299, 0.587, 0.114),
            ScopeColorMatrix::Rec709 => (0.2126, 0.7152, 0.0722),
            ScopeColorMatrix::Rec2020 => (0.2627, 0.6780, 0.0593),
        }
    }
}

/// Configuração completa dos escopos de vídeo.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScopeConfig {
    pub waveform_scale: WaveformScale,
    pub waveform_mode: WaveformMode,
    pub vectorscope_targets: VectorscopeTarget,
    pub vectorscope_skin_line: bool,
    pub parade_mode: ParadeMode,
    pub matrix: ScopeColorMatrix,
    pub clamp: bool,
}

impl Default for ScopeConfig {
    fn default() -> Self {
        Self {
            waveform_scale: WaveformScale::Ire,
            waveform_mode: WaveformMode::Luminance,
            vectorscope_targets: VectorscopeTarget::Percent75,
            vectorscope_skin_line: true,
            parade_mode: ParadeMode::Rgb,
            matrix: ScopeColorMatrix::Rec709,
            clamp: true,
        }
    }
}

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

/// Computa os escopos de vídeo com a configuração padrão.
pub fn compute(f: &Frame) -> ScopeImages {
    compute_with_config(f, &ScopeConfig::default())
}

/// Computa os escopos de vídeo com parâmetros customizados de escala, modo e alvos.
pub fn compute_with_config(f: &Frame, cfg: &ScopeConfig) -> ScopeImages {
    let ww = 256usize;
    let wh = 200usize;
    let vs = 200usize;
    let (hw, hh) = (256usize, 120usize);

    if f.w == 0 || f.h == 0 || f.px.is_empty() {
        return ScopeImages {
            histogram: img(hw, hh),
            waveform: img(ww, wh),
            vectorscope: img(vs, vs),
            parade: img(ww, wh),
        };
    }

    // Subamostragem para manter o processamento fluido a 60 FPS
    let step = ((f.w * f.h) as f32 / 120_000.0).sqrt().max(1.0) as usize;
    let mut hist = [[0u32; 256]; 4];

    // Acumuladores de waveform
    let mut wf_luma = vec![0u32; ww * wh];
    let mut wf_rgb = vec![[0u32; 3]; ww * wh];
    let mut wf_yc_chroma = vec![0u32; ww * wh];

    // Acumuladores de parade (RGB, YUV ou RGB-White)
    let parade_cols = match cfg.parade_mode {
        ParadeMode::RgbWhite => 4,
        _ => 3,
    };
    let mut par = vec![vec![0u32; parade_cols]; ww * wh];

    // Acumulador de vectorscope
    let mut vec_acc = vec![0u32; vs * vs];

    let (wr, wg, wb) = cfg.matrix.luma_weights();

    for y in (0..f.h).step_by(step) {
        for x in (0..f.w).step_by(step) {
            let p = f.px[y * f.w + x];
            let (r, g, b) = if cfg.clamp {
                (
                    p[0].clamp(0.0, 1.0),
                    p[1].clamp(0.0, 1.0),
                    p[2].clamp(0.0, 1.0),
                )
            } else {
                (p[0].max(0.0), p[1].max(0.0), p[2].max(0.0))
            };

            let l = wr * r + wg * g + wb * b;

            // Histograma
            hist[0][(r.clamp(0.0, 1.0) * 255.0) as usize] += 1;
            hist[1][(g.clamp(0.0, 1.0) * 255.0) as usize] += 1;
            hist[2][(b.clamp(0.0, 1.0) * 255.0) as usize] += 1;
            hist[3][(l.clamp(0.0, 1.0) * 255.0) as usize] += 1;

            let col = (x * ww / f.w).min(ww - 1);
            let row = |v: f32| ((1.0 - v.clamp(0.0, 1.0)) * (wh - 1) as f32) as usize;

            // Waveform
            match cfg.waveform_mode {
                WaveformMode::Luminance | WaveformMode::YCNoChroma => {
                    wf_luma[row(l) * ww + col] += 1;
                }
                WaveformMode::RgbOverlay => {
                    wf_rgb[row(r) * ww + col][0] += 1;
                    wf_rgb[row(g) * ww + col][1] += 1;
                    wf_rgb[row(b) * ww + col][2] += 1;
                }
                WaveformMode::YC => {
                    wf_luma[row(l) * ww + col] += 1;
                    let cb = (b - l) * 0.5389;
                    let cr = (r - l) * 0.6350;
                    let c_amp = (cb * cb + cr * cr).sqrt() * 1.5;
                    wf_yc_chroma[row(l + c_amp) * ww + col] += 1;
                    wf_yc_chroma[row(l - c_amp) * ww + col] += 1;
                }
            }

            // Parade
            let slot_w = ww / parade_cols;
            let pc = ((x * slot_w) / f.w).min(slot_w - 1);
            let cb = (b - l) * 0.5389;
            let cr = (r - l) * 0.6350;

            match cfg.parade_mode {
                ParadeMode::Rgb => {
                    par[row(r) * ww + pc][0] += 1;
                    par[row(g) * ww + pc + slot_w][1] += 1;
                    par[row(b) * ww + pc + 2 * slot_w][2] += 1;
                }
                ParadeMode::Yuv => {
                    par[row(l) * ww + pc][0] += 1;
                    par[row(cb + 0.5) * ww + pc + slot_w][1] += 1;
                    par[row(cr + 0.5) * ww + pc + 2 * slot_w][2] += 1;
                }
                ParadeMode::RgbWhite => {
                    par[row(r) * ww + pc][0] += 1;
                    par[row(g) * ww + pc + slot_w][1] += 1;
                    par[row(b) * ww + pc + 2 * slot_w][2] += 1;
                    par[row(l) * ww + pc + 3 * slot_w][3] += 1;
                }
            }

            // Vectorscope
            let vx = ((cb + 0.5) * (vs - 1) as f32).clamp(0.0, (vs - 1) as f32) as usize;
            let vy = ((0.5 - cr) * (vs - 1) as f32).clamp(0.0, (vs - 1) as f32) as usize;
            vec_acc[vy * vs + vx] += 1;
        }
    }

    // Histograma
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

    // Waveform
    let mut waveform = img(ww, wh);
    match cfg.waveform_mode {
        WaveformMode::RgbOverlay => {
            for (dst, &[r, g, b]) in waveform
                .as_mut()
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(wf_rgb.iter())
            {
                if r + g + b > 0 {
                    *dst = [intensity(r), intensity(g), intensity(b), 255];
                }
            }
        }
        WaveformMode::YC => {
            for (i, dst) in waveform
                .as_mut()
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .enumerate()
            {
                let kl = intensity(wf_luma[i]);
                let kc = intensity(wf_yc_chroma[i]);
                if kl > 0 || kc > 0 {
                    *dst = [
                        kc.saturating_add(kl / 3),
                        kl.saturating_add(kc / 2),
                        (kl / 3).saturating_add(kc / 4),
                        255,
                    ];
                }
            }
        }
        _ => {
            for (dst, &count) in waveform
                .as_mut()
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(wf_luma.iter())
            {
                let k = intensity(count);
                if k > 0 {
                    *dst = [k / 3, k, k / 3, 255];
                }
            }
        }
    }

    // Linhas de referência da escala da Waveform
    let grid_lines: Vec<(usize, [u8; 3])> = match cfg.waveform_scale {
        WaveformScale::Ire => vec![
            (wh - 1, [200, 200, 200]),                               // 0 IRE
            ((wh as f32 * (1.0 - 0.075)) as usize, [120, 180, 240]), // 7.5 IRE (setup pedestal)
            (wh / 2, [180, 180, 180]),                               // 50 IRE
            (0, [220, 220, 220]),                                    // 100 IRE
        ],
        WaveformScale::MilliVolts => vec![
            (wh - 1, [200, 200, 200]), // 0 mV
            (wh / 2, [180, 180, 180]), // 350 mV
            (0, [220, 220, 220]),      // 700 mV
        ],
        WaveformScale::Digital8Bit => vec![
            ((wh as f32 * (1.0 - 16.0 / 255.0)) as usize, [120, 180, 240]), // 16 Studio Black
            (wh / 2, [180, 180, 180]),                                      // 128 Mid
            (
                (wh as f32 * (1.0 - 235.0 / 255.0)) as usize,
                [220, 200, 120],
            ), // 235 Studio White
            (0, [255, 255, 255]),                                           // 255 Peak
        ],
        WaveformScale::Float => vec![
            (wh - 1, [200, 200, 200]), // 0.0
            (wh / 2, [180, 180, 180]), // 0.5
            (0, [220, 220, 220]),      // 1.0
        ],
    };

    for (y, col) in grid_lines {
        let yy = y.min(wh - 1);
        for x in (0..ww).step_by(2) {
            add(&mut waveform, x, yy, col, 45);
        }
    }

    // Parade
    let mut parade = img(ww, wh);
    for (i, dst) in parade
        .as_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .enumerate()
    {
        let sample = &par[i];
        match cfg.parade_mode {
            ParadeMode::Rgb => {
                let (r, g, b) = (sample[0], sample[1], sample[2]);
                if r + g + b > 0 {
                    *dst = [intensity(r), intensity(g), intensity(b), 255];
                }
            }
            ParadeMode::Yuv => {
                let (y_val, u_val, v_val) = (sample[0], sample[1], sample[2]);
                if y_val + u_val + v_val > 0 {
                    *dst = [
                        intensity(v_val).saturating_add(intensity(y_val) / 2),
                        intensity(y_val),
                        intensity(u_val).saturating_add(intensity(y_val) / 2),
                        255,
                    ];
                }
            }
            ParadeMode::RgbWhite => {
                let (r, g, b, w_val) = (sample[0], sample[1], sample[2], sample[3]);
                if r + g + b + w_val > 0 {
                    *dst = [
                        intensity(r).saturating_add(intensity(w_val) / 3),
                        intensity(g).saturating_add(intensity(w_val) / 3),
                        intensity(b).saturating_add(intensity(w_val) / 3),
                        255,
                    ];
                }
            }
        }
    }

    // Linhas de grade do parade
    for y in [0, wh / 2, wh - 1] {
        for x in (0..ww).step_by(2) {
            add(&mut parade, x, y, [255, 255, 255], 40);
        }
    }

    // Vectorscope
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

    // Círculo externo (100% de saturação)
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

    // Linha tom de pele (Skin Tone Line / Eixo I a 123°)
    if cfg.vectorscope_skin_line {
        let skin_rad = 123.0f32 * std::f32::consts::PI / 180.0;
        let cos_a = skin_rad.cos();
        let sin_a = skin_rad.sin();
        let max_len = c - 4.0;
        for step_i in 0..=(max_len as usize) {
            let t = step_i as f32;
            let sx = (c + t * cos_a) as usize;
            let sy = (c - t * sin_a) as usize; // tela Y invertido
            if sx < vs && sy < vs {
                add(&mut vectorscope, sx, sy, [255, 180, 110], 80);
            }
        }
        // Marcadores de referência na linha tom de pele
        for tick_t in [max_len * 0.40, max_len * 0.70] {
            let px = c + tick_t * cos_a;
            let py = c - tick_t * sin_a;
            for offset in -2..=2 {
                let ox = (px - offset as f32 * sin_a) as usize;
                let oy = (py - offset as f32 * cos_a) as usize;
                if ox < vs && oy < vs {
                    add(&mut vectorscope, ox, oy, [255, 210, 140], 120);
                }
            }
        }
    }

    // Alvos das cores primárias e secundárias (75% ou 100%)
    let amp = cfg.vectorscope_targets.amplitude();
    for col in [
        [amp, 0.0, 0.0],
        [0.0, amp, 0.0],
        [0.0, 0.0, amp],
        [amp, amp, 0.0],
        [0.0, amp, amp],
        [amp, 0.0, amp],
    ] {
        let l = wr * col[0] + wg * col[1] + wb * col[2];
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_com_config_padrao_e_variantes() {
        let mut px = vec![[0.0f32; 4]; 1920 * 1080];
        // Preenche com gradiente colorido e tom de pele aproximado
        for y in 0..1080 {
            for x in 0..1920 {
                let r = (x as f32) / 1920.0;
                let g = (y as f32) / 1080.0;
                let b = 0.5;
                px[y * 1920 + x] = [r, g, b, 1.0];
            }
        }
        let frame = Frame {
            w: 1920,
            h: 1080,
            px,
        };

        // 1. Configuração padrão (IRE, Luminância, 75%, Skin line, RGB)
        let sc_def = compute(&frame);
        assert_eq!(sc_def.waveform.width(), 256);
        assert_eq!(sc_def.vectorscope.width(), 200);

        // 2. Waveform RGB Overlay e Digital 8-bit
        let mut cfg = ScopeConfig {
            waveform_scale: WaveformScale::Digital8Bit,
            waveform_mode: WaveformMode::RgbOverlay,
            vectorscope_targets: VectorscopeTarget::Percent100,
            vectorscope_skin_line: false,
            parade_mode: ParadeMode::RgbWhite,
            matrix: ScopeColorMatrix::Rec2020,
            clamp: true,
        };
        let sc_rgb = compute_with_config(&frame, &cfg);
        assert_eq!(sc_rgb.waveform.width(), 256);

        // 3. Waveform YC e mV com Rec. 601
        cfg.waveform_mode = WaveformMode::YC;
        cfg.waveform_scale = WaveformScale::MilliVolts;
        cfg.matrix = ScopeColorMatrix::Rec601;
        cfg.parade_mode = ParadeMode::Yuv;
        cfg.vectorscope_skin_line = true;
        let sc_yc = compute_with_config(&frame, &cfg);
        assert_eq!(sc_yc.waveform.width(), 256);

        // 4. Frame vazio ou hostil não entra em pânico
        let empty_frame = Frame {
            w: 0,
            h: 0,
            px: Vec::new(),
        };
        let sc_empty = compute_with_config(&empty_frame, &cfg);
        assert_eq!(sc_empty.histogram.width(), 256);
    }
}
