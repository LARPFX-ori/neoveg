//! Efeitos de pré-codificação, corte (Crop), modos de escala, LUT `.cube` 1D/3D
//! e sobreposições (Imagem, Nome e Timecode) do Neoveg Encoder (Sala Limpa).

use crate::engine::video::resample_rect;
use crate::engine::Frame;
use crate::media::generate::system_font;
use crate::media::img_extra;
use crate::time::{format_ticks, FrameRate, Ticks, TimeFormat};
use ab_glyph::{point, Font, PxScale, ScaleFont};
use anyhow::{anyhow, bail, Result};
use image::RgbaImage;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Limites defensivos para arquivos `.cube` externos (§0.4).
const MAX_CUBE_BYTES: u64 = 16 << 20;
const MAX_LUT_1D_SIZE: usize = 4096;
const MAX_LUT_3D_SIZE: usize = 65;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CropAspect {
    #[default]
    Free,
    Ratio16x9,
    Ratio4x3,
    Ratio9x16,
    Ratio1x1,
    Ratio24x1,
}

impl CropAspect {
    pub const ALL: [Self; 6] = [
        Self::Free,
        Self::Ratio16x9,
        Self::Ratio4x3,
        Self::Ratio9x16,
        Self::Ratio1x1,
        Self::Ratio24x1,
    ];

    pub fn name_in(self, lang: neoveg_common::Language) -> &'static str {
        let pt = match self {
            Self::Free => "Livre",
            Self::Ratio16x9 => "16:9 Widescreen",
            Self::Ratio4x3 => "4:3 Clássico",
            Self::Ratio9x16 => "9:16 Vertical",
            Self::Ratio1x1 => "1:1 Quadrado",
            Self::Ratio24x1 => "2.4:1 Escopo",
        };
        neoveg_common::tr(lang, pt)
    }

    pub fn name(self) -> &'static str {
        self.name_in(neoveg_common::Language::PtBr)
    }

    pub fn ratio(self) -> Option<f32> {
        match self {
            Self::Free => None,
            Self::Ratio16x9 => Some(16.0 / 9.0),
            Self::Ratio4x3 => Some(4.0 / 3.0),
            Self::Ratio9x16 => Some(9.0 / 16.0),
            Self::Ratio1x1 => Some(1.0),
            Self::Ratio24x1 => Some(2.4),
        }
    }
}

/// Corte retangular em pixels na origem antes do escalonamento.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CropRect {
    pub enabled: bool,
    pub left_px: u32,
    pub top_px: u32,
    pub right_px: u32,
    pub bottom_px: u32,
    #[serde(default)]
    pub aspect: CropAspect,
}

impl CropRect {
    /// Calcula `(x, y, w, h)` válidos na origem respeitando o corte e proporção.
    pub fn effective_rect(self, src_w: u32, src_h: u32) -> (u32, u32, u32, u32) {
        let sw = src_w.max(2);
        let sh = src_h.max(2);
        if !self.enabled {
            return (0, 0, sw, sh);
        }
        let lx = self.left_px.min(sw.saturating_sub(2));
        let ty = self.top_px.min(sh.saturating_sub(2));
        let rx = self.right_px.min(sw.saturating_sub(lx + 2));
        let by = self.bottom_px.min(sh.saturating_sub(ty + 2));
        let mut cw = sw.saturating_sub(lx + rx).max(2);
        let mut ch = sh.saturating_sub(ty + by).max(2);
        let mut cx = lx;
        let mut cy = ty;

        if let Some(target_ar) = self.aspect.ratio() {
            let cur_ar = cw as f32 / ch as f32;
            if cur_ar > target_ar {
                let nw = ((ch as f32 * target_ar).round() as u32).clamp(2, cw);
                cx += (cw - nw) / 2;
                cw = nw;
            } else if cur_ar < target_ar {
                let nh = ((cw as f32 / target_ar).round() as u32).clamp(2, ch);
                cy += (ch - nh) / 2;
                ch = nh;
            }
        }
        (cx, cy, cw, ch)
    }
}

/// Os 5 modos de escala de origem (§Referência das Configurações de Exportação).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SourceScalingMode {
    #[default]
    ScaleToFit,
    ScaleToFill,
    StretchToFill,
    ScaleFitBlackBorders,
    MatchSource,
}

impl SourceScalingMode {
    pub const ALL: [Self; 5] = [
        Self::ScaleToFit,
        Self::ScaleToFill,
        Self::StretchToFill,
        Self::ScaleFitBlackBorders,
        Self::MatchSource,
    ];

    pub fn name_in(self, lang: neoveg_common::Language) -> &'static str {
        let pt = match self {
            Self::ScaleToFit => "Dimensionar para ajustar",
            Self::ScaleToFill => "Dimensionar para preencher",
            Self::StretchToFill => "Esticar para preencher",
            Self::ScaleFitBlackBorders => "Ajustar com bordas pretas",
            Self::MatchSource => "Alterar saída para corresponder à origem",
        };
        neoveg_common::tr(lang, pt)
    }

    pub fn name(self) -> &'static str {
        self.name_in(neoveg_common::Language::PtBr)
    }
}

/// Posicionamento relativo em grade 3x3 + deslocamento em pixels para sobreposições.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OverlayPosition {
    TopLeft,
    TopCenter,
    TopRight,
    MidLeft,
    Center,
    MidRight,
    BottomLeft,
    #[default]
    BottomCenter,
    BottomRight,
}

impl OverlayPosition {
    pub const ALL: [Self; 9] = [
        Self::TopLeft,
        Self::TopCenter,
        Self::TopRight,
        Self::MidLeft,
        Self::Center,
        Self::MidRight,
        Self::BottomLeft,
        Self::BottomCenter,
        Self::BottomRight,
    ];

    pub fn name_in(self, lang: neoveg_common::Language) -> &'static str {
        let pt = match self {
            Self::TopLeft => "Superior esquerdo",
            Self::TopCenter => "Superior centro",
            Self::TopRight => "Superior direito",
            Self::MidLeft => "Centro esquerdo",
            Self::Center => "Centro",
            Self::MidRight => "Centro direito",
            Self::BottomLeft => "Inferior esquerdo",
            Self::BottomCenter => "Inferior centro",
            Self::BottomRight => "Inferior direito",
        };
        neoveg_common::tr(lang, pt)
    }

    pub fn name(self) -> &'static str {
        self.name_in(neoveg_common::Language::PtBr)
    }

    fn anchor_factors(self) -> (f32, f32) {
        match self {
            Self::TopLeft => (0.0, 0.0),
            Self::TopCenter => (0.5, 0.0),
            Self::TopRight => (1.0, 0.0),
            Self::MidLeft => (0.0, 0.5),
            Self::Center => (0.5, 0.5),
            Self::MidRight => (1.0, 0.5),
            Self::BottomLeft => (0.0, 1.0),
            Self::BottomCenter => (0.5, 1.0),
            Self::BottomRight => (1.0, 1.0),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub enum GradePreset {
    #[default]
    None,
    CinematicWarm,
    CoolTeal,
    DesaturatedFade,
    HighContrast,
    Monochrome,
    VintageSepia,
    Custom {
        lift: f32,
        gamma: f32,
        gain: f32,
        temp: f32,
        tint: f32,
        saturation: f32,
    },
}

impl GradePreset {
    pub const NAMED: [(&'static str, Self); 7] = [
        ("Nenhum", Self::None),
        ("Cinemático Quente", Self::CinematicWarm),
        ("Estilo Frio (Teal)", Self::CoolTeal),
        ("Remoção de Saturação", Self::DesaturatedFade),
        ("Alto Contraste", Self::HighContrast),
        ("Monocromático", Self::Monochrome),
        ("Sépia Clássico", Self::VintageSepia),
    ];

    pub fn name_in(&self, lang: neoveg_common::Language) -> &'static str {
        let pt = match self {
            Self::None => "Nenhum",
            Self::CinematicWarm => "Cinemático Quente",
            Self::CoolTeal => "Estilo Frio (Teal)",
            Self::DesaturatedFade => "Remoção de Saturação",
            Self::HighContrast => "Alto Contraste",
            Self::Monochrome => "Monocromático",
            Self::VintageSepia => "Sépia Clássico",
            Self::Custom { .. } => "Ajuste Personalizado",
        };
        neoveg_common::tr(lang, pt)
    }

    pub fn name(&self) -> &'static str {
        self.name_in(neoveg_common::Language::PtBr)
    }

    fn apply_rgb(&self, c: [f32; 3]) -> [f32; 3] {
        match *self {
            Self::None => c,
            Self::CinematicWarm => {
                let y = [
                    (c[0] * 1.06 + 0.015).clamp(0.0, 1.0),
                    (c[1] * 1.00 + 0.004).clamp(0.0, 1.0),
                    (c[2] * 0.90).clamp(0.0, 1.0),
                ];
                saturate_rgb(y, 1.08)
            }
            Self::CoolTeal => {
                let y = [
                    (c[0] * 0.90).clamp(0.0, 1.0),
                    (c[1] * 1.02 + 0.01).clamp(0.0, 1.0),
                    (c[2] * 1.09 + 0.02).clamp(0.0, 1.0),
                ];
                saturate_rgb(y, 0.95)
            }
            Self::DesaturatedFade => {
                let f = saturate_rgb(c, 0.45);
                [
                    (f[0] * 0.92 + 0.05).clamp(0.0, 1.0),
                    (f[1] * 0.92 + 0.05).clamp(0.0, 1.0),
                    (f[2] * 0.92 + 0.05).clamp(0.0, 1.0),
                ]
            }
            Self::HighContrast => {
                let f = |x: f32| -> f32 {
                    let t = ((x - 0.5) * 1.28 + 0.5).clamp(0.0, 1.0);
                    t * t * (3.0 - 2.0 * t)
                };
                [f(c[0]), f(c[1]), f(c[2])]
            }
            Self::Monochrome => {
                let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
                [l, l, l]
            }
            Self::VintageSepia => [
                (0.393 * c[0] + 0.769 * c[1] + 0.189 * c[2]).clamp(0.0, 1.0),
                (0.349 * c[0] + 0.686 * c[1] + 0.168 * c[2]).clamp(0.0, 1.0),
                (0.272 * c[0] + 0.534 * c[1] + 0.131 * c[2]).clamp(0.0, 1.0),
            ],
            Self::Custom {
                lift,
                gamma,
                gain,
                temp,
                tint,
                saturation,
            } => {
                let ig = 1.0 / gamma.clamp(0.1, 4.0);
                let mut o = [0.0f32; 3];
                for k in 0..3 {
                    let y = (gain * (c[k] + lift * (1.0 - c[k]))).clamp(0.0, 4.0);
                    o[k] = y.powf(ig);
                }
                o[0] = (o[0] + temp * 0.1).clamp(0.0, 1.0);
                o[2] = (o[2] - temp * 0.1).clamp(0.0, 1.0);
                o[1] = (o[1] + tint * 0.1).clamp(0.0, 1.0);
                saturate_rgb(o, saturation.clamp(0.0, 3.0))
            }
        }
    }
}

fn saturate_rgb(c: [f32; 3], sat: f32) -> [f32; 3] {
    let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    [
        (l + (c[0] - l) * sat).clamp(0.0, 1.0),
        (l + (c[1] - l) * sat).clamp(0.0, 1.0),
        (l + (c[2] - l) * sat).clamp(0.0, 1.0),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TimecodeSource {
    #[default]
    MediaTime,
    CustomStart,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimecodeOverlay {
    pub enabled: bool,
    #[serde(default)]
    pub source: TimecodeSource,
    #[serde(default)]
    pub custom_start_ticks: Ticks,
    #[serde(default)]
    pub drop_frame: bool,
    #[serde(default)]
    pub frame_offset: i64,
    #[serde(default)]
    pub position: OverlayPosition,
    #[serde(default)]
    pub offset_x: i32,
    #[serde(default)]
    pub offset_y: i32,
    #[serde(default = "default_overlay_text_pct")]
    pub size_pct: f32,
    #[serde(default = "default_bg_opacity")]
    pub bg_opacity: f32,
}

impl Default for TimecodeOverlay {
    fn default() -> Self {
        Self {
            enabled: false,
            source: TimecodeSource::MediaTime,
            custom_start_ticks: Ticks::ZERO,
            drop_frame: false,
            frame_offset: 0,
            position: OverlayPosition::BottomCenter,
            offset_x: 0,
            offset_y: 0,
            size_pct: 4.5,
            bg_opacity: 0.6,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum NameFormat {
    PrefixSuffixOnly,
    #[default]
    SourceFilename,
    SourceStem,
    OutputFilename,
    OutputStem,
}

impl NameFormat {
    pub const ALL: [Self; 5] = [
        Self::PrefixSuffixOnly,
        Self::SourceFilename,
        Self::SourceStem,
        Self::OutputFilename,
        Self::OutputStem,
    ];

    pub fn name_in(self, lang: neoveg_common::Language) -> &'static str {
        let pt = match self {
            Self::PrefixSuffixOnly => "Somente prefixo e sufixo",
            Self::SourceFilename => "Nome do arquivo de origem",
            Self::SourceStem => "Nome de origem (sem extensão)",
            Self::OutputFilename => "Nome do arquivo de saída",
            Self::OutputStem => "Nome de saída (sem extensão)",
        };
        neoveg_common::tr(lang, pt)
    }

    pub fn name(self) -> &'static str {
        self.name_in(neoveg_common::Language::PtBr)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NameOverlay {
    pub enabled: bool,
    #[serde(default)]
    pub format: NameFormat,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
    #[serde(default)]
    pub position: OverlayPosition,
    #[serde(default)]
    pub offset_x: i32,
    #[serde(default)]
    pub offset_y: i32,
    #[serde(default = "default_overlay_text_pct")]
    pub size_pct: f32,
    #[serde(default = "default_bg_opacity")]
    pub bg_opacity: f32,
}

impl Default for NameOverlay {
    fn default() -> Self {
        Self {
            enabled: false,
            format: NameFormat::SourceStem,
            prefix: String::new(),
            suffix: String::new(),
            position: OverlayPosition::TopLeft,
            offset_x: 0,
            offset_y: 0,
            size_pct: 4.0,
            bg_opacity: 0.6,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageOverlay {
    pub enabled: bool,
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub position: OverlayPosition,
    #[serde(default)]
    pub offset_x: i32,
    #[serde(default)]
    pub offset_y: i32,
    #[serde(default = "default_img_scale_pct")]
    pub size_pct: f32,
    #[serde(default)]
    pub absolute_size: bool,
    #[serde(default = "default_full_opacity")]
    pub opacity: f32,
}

impl Default for ImageOverlay {
    fn default() -> Self {
        Self {
            enabled: false,
            path: None,
            position: OverlayPosition::TopRight,
            offset_x: 0,
            offset_y: 0,
            size_pct: 15.0,
            absolute_size: false,
            opacity: 1.0,
        }
    }
}

/// Ajustador de tempo: acelera ou estende a duração final da mídia (50%..200%).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimeTuner {
    pub enabled: bool,
    #[serde(default = "default_tuner_pct")]
    pub duration_pct: f32,
}

impl Default for TimeTuner {
    fn default() -> Self {
        Self {
            enabled: false,
            duration_pct: 100.0,
        }
    }
}

fn default_overlay_text_pct() -> f32 {
    4.5
}
fn default_bg_opacity() -> f32 {
    0.6
}
fn default_img_scale_pct() -> f32 {
    15.0
}
fn default_full_opacity() -> f32 {
    1.0
}
fn default_tuner_pct() -> f32 {
    100.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct PreEncodeEffects {
    #[serde(default)]
    pub grade: GradePreset,
    #[serde(default)]
    pub lut_path: Option<PathBuf>,
    #[serde(default)]
    pub image_overlay: ImageOverlay,
    #[serde(default)]
    pub name_overlay: NameOverlay,
    #[serde(default)]
    pub timecode_overlay: TimecodeOverlay,
    #[serde(default)]
    pub time_tuner: TimeTuner,
}

/// Tabela LUT 1D ou 3D carregada de arquivo `.cube` (especificação pública).
#[derive(Clone, Debug)]
pub enum CubeLut {
    Lut1D {
        domain_min: [f32; 3],
        domain_max: [f32; 3],
        entries: Vec<[f32; 3]>,
    },
    Lut3D {
        size: usize,
        domain_min: [f32; 3],
        domain_max: [f32; 3],
        entries: Vec<[f32; 3]>,
    },
}

impl CubeLut {
    pub fn load_file(path: &Path) -> Result<Self> {
        let meta = std::fs::metadata(path)?;
        if meta.len() > MAX_CUBE_BYTES {
            bail!("arquivo .cube grande demais ({} bytes)", meta.len());
        }
        let text = std::fs::read_to_string(path)?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self> {
        if text.len() > MAX_CUBE_BYTES as usize {
            bail!("conteúdo .cube grande demais");
        }
        let mut size_1d: Option<usize> = None;
        let mut size_3d: Option<usize> = None;
        let mut domain_min = [0.0f32, 0.0, 0.0];
        let mut domain_max = [1.0f32, 1.0, 1.0];
        let mut entries = Vec::new();

        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix("TITLE") {
                let _ = rest;
                continue;
            }
            if let Some(rest) = line.strip_prefix("LUT_1D_SIZE") {
                let n: usize = rest
                    .trim()
                    .parse()
                    .map_err(|_| anyhow!("LUT_1D_SIZE inválido"))?;
                if !(2..=MAX_LUT_1D_SIZE).contains(&n) {
                    bail!("LUT_1D_SIZE fora dos limites: {n}");
                }
                size_1d = Some(n);
                continue;
            }
            if let Some(rest) = line.strip_prefix("LUT_3D_SIZE") {
                let n: usize = rest
                    .trim()
                    .parse()
                    .map_err(|_| anyhow!("LUT_3D_SIZE inválido"))?;
                if !(2..=MAX_LUT_3D_SIZE).contains(&n) {
                    bail!("LUT_3D_SIZE fora dos limites: {n}");
                }
                size_3d = Some(n);
                continue;
            }
            if let Some(rest) = line.strip_prefix("DOMAIN_MIN") {
                domain_min = parse_rgb_triplet(rest)?;
                continue;
            }
            if let Some(rest) = line.strip_prefix("DOMAIN_MAX") {
                domain_max = parse_rgb_triplet(rest)?;
                continue;
            }
            if line.starts_with("LUT_") {
                continue;
            }
            let max_expected = match (size_3d, size_1d) {
                (Some(n), _) => n * n * n,
                (None, Some(n)) => n,
                (None, None) => MAX_LUT_3D_SIZE * MAX_LUT_3D_SIZE * MAX_LUT_3D_SIZE,
            };
            if entries.len() >= max_expected {
                bail!("entradas em excesso na tabela .cube");
            }
            entries.push(parse_rgb_triplet(line)?);
        }

        if let Some(n) = size_3d {
            let expected = n * n * n;
            if entries.len() != expected {
                bail!(
                    "LUT 3D esperava {expected} linhas, encontrou {}",
                    entries.len()
                );
            }
            return Ok(Self::Lut3D {
                size: n,
                domain_min,
                domain_max,
                entries,
            });
        }
        if let Some(n) = size_1d {
            if entries.len() != n {
                bail!("LUT 1D esperava {n} linhas, encontrou {}", entries.len());
            }
            return Ok(Self::Lut1D {
                domain_min,
                domain_max,
                entries,
            });
        }
        bail!("arquivo .cube sem LUT_3D_SIZE ou LUT_1D_SIZE")
    }

    pub fn sample(&self, rgb: [f32; 3]) -> [f32; 3] {
        match self {
            Self::Lut1D {
                domain_min,
                domain_max,
                entries,
            } => {
                let n = entries.len().saturating_sub(1).max(1);
                let mut out = [0.0f32; 3];
                for c in 0..3 {
                    let span = (domain_max[c] - domain_min[c]).max(1e-6);
                    let u = ((rgb[c] - domain_min[c]) / span).clamp(0.0, 1.0) * n as f32;
                    let i0 = (u.floor() as usize).min(n);
                    let i1 = (i0 + 1).min(n);
                    let f = u - i0 as f32;
                    let a = entries.get(i0).map(|e| e[c]).unwrap_or(0.0);
                    let b = entries.get(i1).map(|e| e[c]).unwrap_or(a);
                    out[c] = (a + (b - a) * f).clamp(0.0, 1.0);
                }
                out
            }
            Self::Lut3D {
                size,
                domain_min,
                domain_max,
                entries,
            } => {
                let n = *size;
                let max_i = n.saturating_sub(1).max(1);
                let norm = |v: f32, c: usize| -> f32 {
                    let span = (domain_max[c] - domain_min[c]).max(1e-6);
                    ((v - domain_min[c]) / span).clamp(0.0, 1.0) * max_i as f32
                };
                let (rx, gx, bx) = (norm(rgb[0], 0), norm(rgb[1], 1), norm(rgb[2], 2));
                let (r0, g0, b0) = (
                    (rx.floor() as usize).min(max_i),
                    (gx.floor() as usize).min(max_i),
                    (bx.floor() as usize).min(max_i),
                );
                let (r1, g1, b1) = (
                    (r0 + 1).min(max_i),
                    (g0 + 1).min(max_i),
                    (b0 + 1).min(max_i),
                );
                let (fr, fg, fb) = (rx - r0 as f32, gx - g0 as f32, bx - b0 as f32);
                let at = |r: usize, g: usize, b: usize| -> [f32; 3] {
                    // Ordem padrão `.cube` 3D: R varia mais rápido, depois G, depois B.
                    let idx = (b * n + g) * n + r;
                    entries.get(idx).copied().unwrap_or(rgb)
                };
                let c000 = at(r0, g0, b0);
                let c100 = at(r1, g0, b0);
                let c010 = at(r0, g1, b0);
                let c110 = at(r1, g1, b0);
                let c001 = at(r0, g0, b1);
                let c101 = at(r1, g0, b1);
                let c011 = at(r0, g1, b1);
                let c111 = at(r1, g1, b1);
                let mut out = [0.0f32; 3];
                for k in 0..3 {
                    let c00 = c000[k] + (c100[k] - c000[k]) * fr;
                    let c10 = c010[k] + (c110[k] - c010[k]) * fr;
                    let c01 = c001[k] + (c101[k] - c001[k]) * fr;
                    let c11 = c011[k] + (c111[k] - c011[k]) * fr;
                    let c0 = c00 + (c10 - c00) * fg;
                    let c1 = c01 + (c11 - c01) * fg;
                    out[k] = (c0 + (c1 - c0) * fb).clamp(0.0, 1.0);
                }
                out
            }
        }
    }
}

fn parse_rgb_triplet(s: &str) -> Result<[f32; 3]> {
    let mut parts = s.split_whitespace();
    let parse_one = |p: Option<&str>| -> Result<f32> {
        let v: f32 = p
            .ok_or_else(|| anyhow!("linha .cube incompleta"))?
            .parse()
            .map_err(|_| anyhow!("número inválido no arquivo .cube"))?;
        if !v.is_finite() {
            bail!("valor não finito no arquivo .cube");
        }
        Ok(v)
    };
    Ok([
        parse_one(parts.next())?,
        parse_one(parts.next())?,
        parse_one(parts.next())?,
    ])
}

/// Dimensões finais considerando `SourceScalingMode::MatchSource` + `CropRect`.
pub fn effective_output_dims(
    src_w: u32,
    src_h: u32,
    crop: CropRect,
    scaling: SourceScalingMode,
    req_w: u32,
    req_h: u32,
    match_source_dims: bool,
) -> (u32, u32) {
    if match_source_dims || scaling == SourceScalingMode::MatchSource {
        let (_, _, cw, ch) = crop.effective_rect(src_w, src_h);
        let even = |v: u32| (v.max(2) / 2) * 2;
        (even(cw), even(ch))
    } else {
        (req_w.max(2), req_h.max(2))
    }
}

/// Aplica corte (Crop) e qualquer um dos 5 modos de escala ao quadro de origem.
pub fn crop_and_scale_frame(
    src: &Frame,
    crop: CropRect,
    scaling: SourceScalingMode,
    out_w: usize,
    out_h: usize,
) -> Frame {
    let (cx, cy, cw, ch) = crop.effective_rect(src.w as u32, src.h as u32);
    let (sx, sy, sw, sh) = (cx as f32, cy as f32, cw as f32, ch as f32);
    let (ow, oh) = (out_w.max(2), out_h.max(2));
    let (wf, hf) = (ow as f32, oh as f32);

    match scaling {
        SourceScalingMode::MatchSource => {
            if cx == 0
                && cy == 0
                && cw as usize == src.w
                && ch as usize == src.h
                && src.w == ow
                && src.h == oh
            {
                src.clone()
            } else {
                resample_rect(src, sx, sy, sw, sh, ow, oh, 0.0, 0.0, wf, hf)
            }
        }
        SourceScalingMode::StretchToFill => {
            resample_rect(src, sx, sy, sw, sh, ow, oh, 0.0, 0.0, wf, hf)
        }
        SourceScalingMode::ScaleToFit => {
            let s = (wf / sw).min(hf / sh);
            let dw = sw * s;
            let dh = sh * s;
            resample_rect(
                src,
                sx,
                sy,
                sw,
                sh,
                ow,
                oh,
                (wf - dw) * 0.5,
                (hf - dh) * 0.5,
                dw,
                dh,
            )
        }
        SourceScalingMode::ScaleFitBlackBorders => {
            // Aplica margem preta uniforme de 4% mesmo quando cabe na saída.
            let inner_w = wf * 0.92;
            let inner_h = hf * 0.92;
            let s = (inner_w / sw).min(inner_h / sh);
            let dw = sw * s;
            let dh = sh * s;
            resample_rect(
                src,
                sx,
                sy,
                sw,
                sh,
                ow,
                oh,
                (wf - dw) * 0.5,
                (hf - dh) * 0.5,
                dw,
                dh,
            )
        }
        SourceScalingMode::ScaleToFill => {
            let s = (wf / sw).max(hf / sh);
            let dw = sw * s;
            let dh = sh * s;
            resample_rect(
                src,
                sx,
                sy,
                sw,
                sh,
                ow,
                oh,
                (wf - dw) * 0.5,
                (hf - dh) * 0.5,
                dw,
                dh,
            )
        }
    }
}

/// Recursos pré-carregados por trabalho para evitar E/S durante a renderização dos quadros.
#[derive(Clone, Default)]
pub struct PreparedEffects {
    pub lut: Option<CubeLut>,
    pub overlay_img: Option<RgbaImage>,
    pub source_name: String,
    pub source_stem: String,
    pub output_name: String,
    pub output_stem: String,
}

impl PreparedEffects {
    pub fn prepare(
        fx: &PreEncodeEffects,
        source_path_or_name: &str,
        output_path: &Path,
        warnings: &mut Vec<String>,
    ) -> Self {
        let lut = fx
            .lut_path
            .as_ref()
            .and_then(|p| match CubeLut::load_file(p) {
                Ok(l) => Some(l),
                Err(e) => {
                    warnings.push(format!("LUT .cube ignorada ({}): {e}", p.display()));
                    None
                }
            });
        let overlay_img = if fx.image_overlay.enabled {
            fx.image_overlay
                .path
                .as_ref()
                .and_then(|p| match img_extra::open_image_file(p) {
                    Ok(img) => Some(img),
                    Err(e) => {
                        warnings.push(format!(
                            "Sobreposição de imagem ignorada ({}): {e}",
                            p.display()
                        ));
                        None
                    }
                })
        } else {
            None
        };
        let src_p = Path::new(source_path_or_name);
        let source_name = src_p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| source_path_or_name.to_string());
        let source_stem = src_p
            .file_stem()
            .map(|n| n.to_string_lossy().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| source_name.clone());
        let output_name = output_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let output_stem = output_path
            .file_stem()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        Self {
            lut,
            overlay_img,
            source_name,
            source_stem,
            output_name,
            output_stem,
        }
    }
}

/// Aplica todos os efeitos e sobreposições de pré-codificação em um quadro já escalado.
pub fn apply_pre_encode_effects(
    frame: &mut Frame,
    fx: &PreEncodeEffects,
    prep: &PreparedEffects,
    out_frame_index: u64,
    src_ticks: Ticks,
    fps: FrameRate,
    sample_rate: u32,
) {
    // 1. Correção de cor / Look + LUT `.cube`
    if fx.grade != GradePreset::None || prep.lut.is_some() {
        frame.px.par_iter_mut().for_each(|p| {
            let a = p[3];
            let inv = if a > 1e-6 { 1.0 / a } else { 0.0 };
            let mut rgb = [
                (p[0] * inv).clamp(0.0, 1.0),
                (p[1] * inv).clamp(0.0, 1.0),
                (p[2] * inv).clamp(0.0, 1.0),
            ];
            if fx.grade != GradePreset::None {
                rgb = fx.grade.apply_rgb(rgb);
            }
            if let Some(lut) = &prep.lut {
                rgb = lut.sample(rgb);
            }
            p[0] = rgb[0] * a;
            p[1] = rgb[1] * a;
            p[2] = rgb[2] * a;
        });
    }

    // 2. Sobreposição de Imagem
    if fx.image_overlay.enabled {
        if let Some(img) = &prep.overlay_img {
            composite_image_overlay(frame, img, &fx.image_overlay);
        }
    }

    // 3. Sobreposição de Nome
    if fx.name_overlay.enabled {
        let base = match fx.name_overlay.format {
            NameFormat::PrefixSuffixOnly => "",
            NameFormat::SourceFilename => &prep.source_name,
            NameFormat::SourceStem => &prep.source_stem,
            NameFormat::OutputFilename => &prep.output_name,
            NameFormat::OutputStem => &prep.output_stem,
        };
        let txt = format!(
            "{}{}{}",
            fx.name_overlay.prefix, base, fx.name_overlay.suffix
        );
        if !txt.trim().is_empty() {
            draw_text_badge(
                frame,
                &txt,
                fx.name_overlay.position,
                fx.name_overlay.offset_x,
                fx.name_overlay.offset_y,
                fx.name_overlay.size_pct,
                fx.name_overlay.bg_opacity,
            );
        }
    }

    // 4. Sobreposição de Timecode
    if fx.timecode_overlay.enabled {
        let base_ticks = match fx.timecode_overlay.source {
            TimecodeSource::MediaTime => src_ticks,
            TimecodeSource::CustomStart => {
                fx.timecode_overlay.custom_start_ticks + fps.frame_to_ticks(out_frame_index as i64)
            }
        };
        let shifted = base_ticks + fps.frame_to_ticks(fx.timecode_overlay.frame_offset);
        let fmt = if fx.timecode_overlay.drop_frame {
            TimeFormat::SmpteDrop
        } else {
            TimeFormat::SmpteNonDrop
        };
        let label = format_ticks(shifted, fps, sample_rate, fmt);
        draw_text_badge(
            frame,
            &label,
            fx.timecode_overlay.position,
            fx.timecode_overlay.offset_x,
            fx.timecode_overlay.offset_y,
            fx.timecode_overlay.size_pct,
            fx.timecode_overlay.bg_opacity,
        );
    }
}

fn composite_image_overlay(frame: &mut Frame, img: &RgbaImage, cfg: &ImageOverlay) {
    let (sw, sh) = (img.width().max(1), img.height().max(1));
    let (dw, dh) = if cfg.absolute_size {
        (sw as usize, sh as usize)
    } else {
        let pct = (cfg.size_pct / 100.0).clamp(0.02, 1.0);
        let target_h = (frame.h as f32 * pct).round().max(4.0) as usize;
        let target_w = ((target_h as f32 * sw as f32 / sh as f32).round().max(4.0)) as usize;
        (target_w.min(frame.w), target_h.min(frame.h))
    };
    let margin_x = (frame.w as f32 * 0.03).round() as isize;
    let margin_y = (frame.h as f32 * 0.03).round() as isize;
    let (ax, ay) = cfg.position.anchor_factors();
    let avail_w = frame.w as isize - dw as isize - margin_x * 2;
    let avail_h = frame.h as isize - dh as isize - margin_y * 2;
    let x0 = margin_x + (avail_w as f32 * ax).round() as isize + cfg.offset_x as isize;
    let y0 = margin_y + (avail_h as f32 * ay).round() as isize + cfg.offset_y as isize;
    let opacity = cfg.opacity.clamp(0.0, 1.0);
    let raw = img.as_raw();

    for dy in 0..dh {
        let fy = y0 + dy as isize;
        if fy < 0 || fy >= frame.h as isize {
            continue;
        }
        let sy = ((dy * sh as usize) / dh.max(1)).min(sh as usize - 1);
        for dx in 0..dw {
            let fx = x0 + dx as isize;
            if fx < 0 || fx >= frame.w as isize {
                continue;
            }
            let sx = ((dx * sw as usize) / dw.max(1)).min(sw as usize - 1);
            let idx = (sy * sw as usize + sx) * 4;
            let a = (raw.get(idx + 3).copied().unwrap_or(0) as f32 / 255.0) * opacity;
            if a <= 1e-4 {
                continue;
            }
            let sr = raw[idx] as f32 / 255.0 * a;
            let sg = raw[idx + 1] as f32 / 255.0 * a;
            let sb = raw[idx + 2] as f32 / 255.0 * a;
            let dst = &mut frame.px[fy as usize * frame.w + fx as usize];
            let ia = 1.0 - a;
            dst[0] = sr + dst[0] * ia;
            dst[1] = sg + dst[1] * ia;
            dst[2] = sb + dst[2] * ia;
            dst[3] = (a + dst[3] * ia).clamp(0.0, 1.0);
        }
    }
}

fn draw_text_badge(
    frame: &mut Frame,
    text: &str,
    pos: OverlayPosition,
    offset_x: i32,
    offset_y: i32,
    size_pct: f32,
    bg_opacity: f32,
) {
    if frame.w < 16 || frame.h < 16 {
        return;
    }
    let px = ((size_pct / 100.0).clamp(0.015, 0.25) * frame.h as f32).max(10.0);
    let bg_a = bg_opacity.clamp(0.0, 1.0);

    if let Some(font) = system_font() {
        let scale = PxScale::from(px);
        let sf = font.as_scaled(scale);
        let mut tw = 0.0f32;
        let mut prev = None;
        for c in text.chars() {
            let gid = font.glyph_id(c);
            if let Some(p) = prev {
                tw += sf.kern(p, gid);
            }
            tw += sf.h_advance(gid);
            prev = Some(gid);
        }
        let th = (sf.ascent() - sf.descent()).max(px);
        let pad_x = (px * 0.45).round() as isize;
        let pad_y = (px * 0.25).round() as isize;
        let bw = (tw.ceil() as isize + pad_x * 2).max(4);
        let bh = (th.ceil() as isize + pad_y * 2).max(4);
        let (x0, y0) = place_box(frame.w, frame.h, bw, bh, pos, offset_x, offset_y);
        fill_rect_alpha(frame, x0, y0, bw, bh, [0.0, 0.0, 0.0, bg_a]);

        let mut cx = (x0 + pad_x) as f32;
        let baseline = y0 as f32 + pad_y as f32 + sf.ascent();
        let mut prev = None;
        for c in text.chars() {
            let gid = font.glyph_id(c);
            if let Some(p) = prev {
                cx += sf.kern(p, gid);
            }
            let g = gid.with_scale_and_position(scale, point(cx, baseline));
            cx += sf.h_advance(gid);
            prev = Some(gid);
            if let Some(og) = font.outline_glyph(g) {
                let bounds = og.px_bounds();
                og.draw(|gx, gy, cov| {
                    let px_x = bounds.min.x as isize + gx as isize;
                    let px_y = bounds.min.y as isize + gy as isize;
                    if px_x >= 0
                        && px_y >= 0
                        && (px_x as usize) < frame.w
                        && (px_y as usize) < frame.h
                    {
                        let a = cov.clamp(0.0, 1.0);
                        let p = &mut frame.px[px_y as usize * frame.w + px_x as usize];
                        let ia = 1.0 - a;
                        p[0] = a + p[0] * ia;
                        p[1] = a + p[1] * ia;
                        p[2] = a + p[2] * ia;
                        p[3] = (a + p[3] * ia).clamp(0.0, 1.0);
                    }
                });
            }
        }
        return;
    }

    // Fallback determinístico 5x7 quando não houver fonte instalada no ambiente headless.
    let cell = ((px / 8.0).round() as isize).max(1);
    let chars: Vec<char> = text.chars().collect();
    let bw = (chars.len() as isize) * 6 * cell + 4 * cell;
    let bh = 9 * cell;
    let (x0, y0) = place_box(frame.w, frame.h, bw, bh, pos, offset_x, offset_y);
    fill_rect_alpha(frame, x0, y0, bw, bh, [0.0, 0.0, 0.0, bg_a]);
    for (ci, ch) in chars.iter().enumerate() {
        let bits = mini_glyph_5x7(*ch);
        let gx0 = x0 + 2 * cell + (ci as isize) * 6 * cell;
        let gy0 = y0 + cell;
        for (row, &mask) in bits.iter().enumerate() {
            for col in 0..5 {
                if (mask >> (4 - col)) & 1 == 1 {
                    fill_rect_alpha(
                        frame,
                        gx0 + (col as isize) * cell,
                        gy0 + (row as isize) * cell,
                        cell,
                        cell,
                        [1.0, 1.0, 1.0, 1.0],
                    );
                }
            }
        }
    }
}

fn place_box(
    fw: usize,
    fh: usize,
    bw: isize,
    bh: isize,
    pos: OverlayPosition,
    off_x: i32,
    off_y: i32,
) -> (isize, isize) {
    let mx = (fw as f32 * 0.03).round() as isize;
    let my = (fh as f32 * 0.03).round() as isize;
    let (ax, ay) = pos.anchor_factors();
    let x = mx + ((fw as isize - bw - mx * 2) as f32 * ax).round() as isize + off_x as isize;
    let y = my + ((fh as isize - bh - my * 2) as f32 * ay).round() as isize + off_y as isize;
    (x, y)
}

fn fill_rect_alpha(frame: &mut Frame, x0: isize, y0: isize, w: isize, h: isize, rgba: [f32; 4]) {
    let a = rgba[3].clamp(0.0, 1.0);
    if a <= 1e-4 {
        return;
    }
    let x_start = x0.max(0) as usize;
    let y_start = y0.max(0) as usize;
    let x_end = (x0 + w).clamp(0, frame.w as isize) as usize;
    let y_end = (y0 + h).clamp(0, frame.h as isize) as usize;
    let ia = 1.0 - a;
    for y in y_start..y_end {
        for x in x_start..x_end {
            let p = &mut frame.px[y * frame.w + x];
            p[0] = rgba[0] * a + p[0] * ia;
            p[1] = rgba[1] * a + p[1] * ia;
            p[2] = rgba[2] * a + p[2] * ia;
            p[3] = (a + p[3] * ia).clamp(0.0, 1.0);
        }
    }
}

fn mini_glyph_5x7(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        ':' => [0x00, 0x04, 0x04, 0x00, 0x04, 0x04, 0x00],
        ';' => [0x00, 0x04, 0x04, 0x00, 0x04, 0x04, 0x08],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x04],
        '-' | '_' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        'A'..='Z' => {
            let b = (c.to_ascii_uppercase() as u8) - b'A';
            [0x0E | (b & 1), 0x11, 0x1F, 0x11, 0x11, 0x11, 0x11]
        }
        ' ' => [0; 7],
        _ => [0x1F, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1F],
    }
}
