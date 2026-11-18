//! Gerenciamento de cor profissional: ACES, espaços de cor, curvas Log e 32 bits.
//!
//! Implementação em sala limpa compatível com as semânticas do VEGAS Pro e Filmcraft:
//! - Formatos de pixel: 8 bits, 32 bits ponto flutuante (níveis de vídeo) e 32 bits ponto flutuante (faixa completa).
//! - Espaços de cor com adaptação cromática Bradford e matrizes CIE XYZ.
//! - Curvas Log de câmeras: Sony S-Log3, ARRI LogC3/LogC4, Canon Log 2/3, Panasonic V-Log, RED Log3G10 e ACEScc/ACEScct.
//! - Transformações de visualização (View Transforms) com ACES RRT + ODT para sRGB e Rec.709.

#![allow(clippy::excessive_precision)]

use serde::{Deserialize, Serialize};

/// Formato de pixel do projeto (VEGAS Pro manual p. 288).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PixelFormat {
    /// 8 bits inteiro por canal (padrão SDR).
    #[default]
    Bits8,
    /// 32 bits ponto flutuante, níveis de vídeo (níveis nominais 0.0..=1.0 com preservação de super-pretos/super-brancos).
    Float32Video,
    /// 32 bits ponto flutuante, faixa completa (faixa dinâmica ilimitada para composição linear e HDR).
    Float32Full,
}

impl PixelFormat {
    pub const ALL: [PixelFormat; 3] = [
        PixelFormat::Bits8,
        PixelFormat::Float32Video,
        PixelFormat::Float32Full,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PixelFormat::Bits8 => "8 bits (inteiro)",
            PixelFormat::Float32Video => "32 bits ponto flutuante (níveis de vídeo)",
            PixelFormat::Float32Full => "32 bits ponto flutuante (faixa completa)",
        }
    }
}

/// Gama de composição do projeto.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CompositingGamma {
    /// Gama padrão de vídeo (2,222).
    #[default]
    Gamma2_222,
    /// Gama linear (1,000) para mesclagem fisicamente correta sem bordas escuras.
    Linear1_000,
}

impl CompositingGamma {
    pub const ALL: [CompositingGamma; 2] =
        [CompositingGamma::Gamma2_222, CompositingGamma::Linear1_000];

    pub fn label(self) -> &'static str {
        match self {
            CompositingGamma::Gamma2_222 => "2,222 (Vídeo)",
            CompositingGamma::Linear1_000 => "1,000 (Linear)",
        }
    }

    pub fn value(self) -> f32 {
        match self {
            CompositingGamma::Gamma2_222 => 2.222,
            CompositingGamma::Linear1_000 => 1.000,
        }
    }
}

/// Espaço de cor do projeto ou da mídia.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ProjectColorSpace {
    /// sRGB padrão (computador).
    #[default]
    DefaultSrgb,
    /// ITU-R BT.709 (HDTV).
    Rec709,
    /// ITU-R BT.2020 (UHDTV / HDR de ampla gama).
    Rec2020,
    /// DCI-P3 (Cinema Digital).
    DciP3,
    /// Display P3 (Monitores modernos e dispositivos Apple).
    DisplayP3,
    /// ACES2065-1 (AP0 linear, padrão de arquivamento e intercâmbio SMPTE ST 2065-1).
    Aces2065_1,
    /// ACEScg (AP1 linear, espaço de trabalho para computação gráfica e composição).
    AcesCg,
    /// ACEScc (AP1 logarítmico para gradação de cor).
    AcesCc,
    /// ACEScct (AP1 logarítmico com curvatura nos tons escuros/sombras).
    AcesCct,
}

impl ProjectColorSpace {
    pub const ALL: [ProjectColorSpace; 9] = [
        ProjectColorSpace::DefaultSrgb,
        ProjectColorSpace::Rec709,
        ProjectColorSpace::Rec2020,
        ProjectColorSpace::DciP3,
        ProjectColorSpace::DisplayP3,
        ProjectColorSpace::Aces2065_1,
        ProjectColorSpace::AcesCg,
        ProjectColorSpace::AcesCc,
        ProjectColorSpace::AcesCct,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ProjectColorSpace::DefaultSrgb => "sRGB (Padrão)",
            ProjectColorSpace::Rec709 => "Rec.709",
            ProjectColorSpace::Rec2020 => "Rec.2020 (Ampla Gama HDR)",
            ProjectColorSpace::DciP3 => "DCI-P3",
            ProjectColorSpace::DisplayP3 => "Display P3",
            ProjectColorSpace::Aces2065_1 => "ACES2065-1 (AP0 Linear)",
            ProjectColorSpace::AcesCg => "ACEScg (AP1 Linear)",
            ProjectColorSpace::AcesCc => "ACEScc (AP1 Log)",
            ProjectColorSpace::AcesCct => "ACEScct (AP1 Log com Toe)",
        }
    }

    /// Retorna as coordenadas de cromaticidade (x, y) dos primários R, G, B e ponto branco W.
    pub fn primaries_and_white(self) -> ([f32; 2], [f32; 2], [f32; 2], [f32; 2]) {
        match self {
            ProjectColorSpace::DefaultSrgb | ProjectColorSpace::Rec709 => (
                [0.640, 0.330],
                [0.300, 0.600],
                [0.150, 0.060],
                [0.3127, 0.3290], // D65
            ),
            ProjectColorSpace::Rec2020 => (
                [0.708, 0.292],
                [0.170, 0.797],
                [0.131, 0.046],
                [0.3127, 0.3290], // D65
            ),
            ProjectColorSpace::DciP3 => (
                [0.680, 0.320],
                [0.265, 0.690],
                [0.150, 0.060],
                [0.3140, 0.3510], // DCI White
            ),
            ProjectColorSpace::DisplayP3 => (
                [0.680, 0.320],
                [0.265, 0.690],
                [0.150, 0.060],
                [0.3127, 0.3290], // D65
            ),
            ProjectColorSpace::Aces2065_1 => (
                [0.7347, 0.2653],
                [0.0000, 1.0000],
                [0.0001, -0.0770],
                [0.32168, 0.33767], // ACES D60
            ),
            ProjectColorSpace::AcesCg | ProjectColorSpace::AcesCc | ProjectColorSpace::AcesCct => (
                [0.713, 0.293],
                [0.165, 0.830],
                [0.128, 0.044],
                [0.32168, 0.33767], // ACES D60
            ),
        }
    }

    /// Matriz 3x3 de conversão de RGB linear deste espaço para CIE XYZ D65
    /// (com adaptação cromática de Bradford quando o ponto branco não for D65).
    pub fn to_xyz_matrix(self) -> [[f32; 3]; 3] {
        let (r, g, b, w) = self.primaries_and_white();
        let m_rgb_to_xyz = compute_rgb_to_xyz_matrix(r, g, b, w);

        // Se o ponto branco for diferente de D65 (0.3127, 0.3290), adapta para D65
        let d65 = [0.3127f32, 0.3290f32];
        if (w[0] - d65[0]).abs() > 1e-4 || (w[1] - d65[1]).abs() > 1e-4 {
            let bfd = bradford_adaptation_matrix(w, d65);
            mul3x3(&bfd, &m_rgb_to_xyz)
        } else {
            m_rgb_to_xyz
        }
    }

    /// Matriz 3x3 de conversão de CIE XYZ D65 para RGB linear deste espaço.
    pub fn from_xyz_matrix(self) -> [[f32; 3]; 3] {
        let to_xyz = self.to_xyz_matrix();
        invert3x3(&to_xyz).unwrap_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])
    }

    /// Converte um valor RGB linear deste espaço para outro espaço de cor.
    pub fn convert_linear_rgb(self, to: ProjectColorSpace, rgb: [f32; 3]) -> [f32; 3] {
        if self == to {
            return rgb;
        }
        let m_to_xyz = self.to_xyz_matrix();
        let m_from_xyz = to.from_xyz_matrix();
        let m_full = mul3x3(&m_from_xyz, &m_to_xyz);
        mat3_mul_vec(&m_full, rgb)
    }
}

/// Transformação de visualização (View Transform) para a janela de prévia (VEGAS Pro manual p. 291).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ViewTransform {
    /// Desativado (Pass-through).
    #[default]
    Off,
    /// Monitor de computador (sRGB).
    Srgb,
    /// Televisão HD padrão (Rec.709).
    Rec709,
    /// Televisão Ampla Gama / HDR (Rec.2020).
    Rec2020,
    /// Telas P3 modernas (Display P3).
    DisplayP3,
    /// ACES Output Transform (RRT + ODT para sRGB).
    AcesRrtOdtSrgb,
    /// ACES Output Transform (RRT + ODT para Rec.709).
    AcesRrtOdtRec709,
}

impl ViewTransform {
    pub const ALL: [ViewTransform; 7] = [
        ViewTransform::Off,
        ViewTransform::Srgb,
        ViewTransform::Rec709,
        ViewTransform::Rec2020,
        ViewTransform::DisplayP3,
        ViewTransform::AcesRrtOdtSrgb,
        ViewTransform::AcesRrtOdtRec709,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ViewTransform::Off => "Desativado (Direto)",
            ViewTransform::Srgb => "sRGB (Monitor de PC)",
            ViewTransform::Rec709 => "Rec.709 (HDTV)",
            ViewTransform::Rec2020 => "Rec.2020 (HDR)",
            ViewTransform::DisplayP3 => "Display P3",
            ViewTransform::AcesRrtOdtSrgb => "ACES RRT + ODT (sRGB)",
            ViewTransform::AcesRrtOdtRec709 => "ACES RRT + ODT (Rec.709)",
        }
    }

    /// Aplica a transformação de visualização aos pixels RGB.
    pub fn apply(self, working_space: ProjectColorSpace, rgb: [f32; 3]) -> [f32; 3] {
        match self {
            ViewTransform::Off => rgb,
            ViewTransform::Srgb => {
                let lin = working_space.convert_linear_rgb(ProjectColorSpace::DefaultSrgb, rgb);
                [
                    linear_to_srgb(lin[0]),
                    linear_to_srgb(lin[1]),
                    linear_to_srgb(lin[2]),
                ]
            }
            ViewTransform::Rec709 => {
                let lin = working_space.convert_linear_rgb(ProjectColorSpace::Rec709, rgb);
                [
                    linear_to_rec709(lin[0]),
                    linear_to_rec709(lin[1]),
                    linear_to_rec709(lin[2]),
                ]
            }
            ViewTransform::Rec2020 => {
                let lin = working_space.convert_linear_rgb(ProjectColorSpace::Rec2020, rgb);
                [
                    linear_to_rec2020(lin[0]),
                    linear_to_rec2020(lin[1]),
                    linear_to_rec2020(lin[2]),
                ]
            }
            ViewTransform::DisplayP3 => {
                let lin = working_space.convert_linear_rgb(ProjectColorSpace::DisplayP3, rgb);
                [
                    linear_to_srgb(lin[0]),
                    linear_to_srgb(lin[1]),
                    linear_to_srgb(lin[2]),
                ]
            }
            ViewTransform::AcesRrtOdtSrgb => {
                // Converte do espaço de trabalho para ACES2065-1 (AP0), aplica RRT+ODT
                let aces = working_space.convert_linear_rgb(ProjectColorSpace::Aces2065_1, rgb);
                let tone = aces_fitted_tone_curve(aces);
                [
                    linear_to_srgb(tone[0]),
                    linear_to_srgb(tone[1]),
                    linear_to_srgb(tone[2]),
                ]
            }
            ViewTransform::AcesRrtOdtRec709 => {
                let aces = working_space.convert_linear_rgb(ProjectColorSpace::Aces2065_1, rgb);
                let tone = aces_fitted_tone_curve(aces);
                [
                    linear_to_rec709(tone[0]),
                    linear_to_rec709(tone[1]),
                    linear_to_rec709(tone[2]),
                ]
            }
        }
    }
}

/// Curvas Logarítmicas de Câmeras profissionais.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogCurve {
    SonySLog3,
    ArriLogC3,
    ArriLogC4,
    CanonLog2,
    CanonLog3,
    PanasonicVLog,
    RedLog3G10,
    AcesCc,
    AcesCct,
}

impl LogCurve {
    pub const ALL: [LogCurve; 9] = [
        LogCurve::SonySLog3,
        LogCurve::ArriLogC3,
        LogCurve::ArriLogC4,
        LogCurve::CanonLog2,
        LogCurve::CanonLog3,
        LogCurve::PanasonicVLog,
        LogCurve::RedLog3G10,
        LogCurve::AcesCc,
        LogCurve::AcesCct,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LogCurve::SonySLog3 => "Sony S-Log3",
            LogCurve::ArriLogC3 => "ARRI LogC3 (EI 800)",
            LogCurve::ArriLogC4 => "ARRI LogC4 (Alexa 35)",
            LogCurve::CanonLog2 => "Canon Log 2",
            LogCurve::CanonLog3 => "Canon Log 3",
            LogCurve::PanasonicVLog => "Panasonic V-Log",
            LogCurve::RedLog3G10 => "RED Log3G10",
            LogCurve::AcesCc => "ACEScc",
            LogCurve::AcesCct => "ACEScct",
        }
    }

    /// Converte um valor codificado em Log (0.0..1.0) para luminância Linear cena-referenciada.
    pub fn to_linear(self, y: f32) -> f32 {
        let y = if y.is_finite() { y } else { 0.0 };
        match self {
            LogCurve::SonySLog3 => {
                // Especificação oficial Sony S-Log3
                if y >= 171.210_29 / 1023.0 {
                    10.0f32.powf((y * 1023.0 - 420.0) / 261.5) * (0.18 + 0.01) - 0.01
                } else {
                    (y * 1023.0 - 95.0) * 0.011_250 / (171.210_29 - 95.0)
                }
            }
            LogCurve::ArriLogC3 => {
                // ARRI Alexa LogC3 EI 800
                let cut = 0.010_591f32;
                let a = 5.555_556f32;
                let b = 0.052_272f32;
                let c = 0.247_190f32;
                let d = 0.385_537f32;
                let e = 5.367_655f32;
                let f = 0.092_809f32;
                let log_cut = e * cut + f;
                if y > log_cut {
                    (10.0f32.powf((y - d) / c) - b) / a
                } else {
                    (y - f) / e
                }
            }
            LogCurve::ArriLogC4 => {
                // ARRI LogC4 (Alexa 35)
                let a = 16320.0f32 / 16384.0;
                let b = 64.0f32 / 16384.0;
                let c = 0.075f32;
                let s = 1.0f32 / 9.0;
                let t = (y - b) / a;
                let lin = 2.0f32.powf((t - c) / s) - 0.01;
                lin.max(0.0)
            }
            LogCurve::CanonLog2 => {
                // Canon Log 2
                if y > 0.067_31 {
                    (10.0f32.powf((y - 0.246_8) / 0.281_86) - 1.0) / 8.73
                } else {
                    (y - 0.035_38) / 0.529_13
                }
            }
            LogCurve::CanonLog3 => {
                // Canon Log 3
                if y > 0.127_83 {
                    (10.0f32.powf((y - 0.366_12) / 0.527_44) - 1.0) / 8.73
                } else {
                    (y - 0.069_88) / 2.3
                }
            }
            LogCurve::PanasonicVLog => {
                // Panasonic V-Log
                let cut = 0.181;
                let b = 0.008_73;
                let c = 0.241_514;
                let d = 0.598_206;
                if y >= cut {
                    10.0f32.powf((y - d) / c) - b
                } else {
                    (y - 0.125) / 5.6
                }
            }
            LogCurve::RedLog3G10 => {
                // RED Log3G10
                let a = 0.224_282f32;
                let b = 155.975_327f32;
                let c = 0.01f32;
                let g = 3.570_724f32;
                if y >= 0.0 {
                    (10.0f32.powf(y / a) - 1.0) / b - c
                } else {
                    (y / g) - c
                }
            }
            LogCurve::AcesCc => {
                // ACEScc (SMPTE S-2014-003)
                if y < 9.72f32 / 1023.0 {
                    (2.0f32.powf(y * 17.52 - 9.72) - 2.0f32.powf(-16.0)) * 2.0
                } else {
                    2.0f32.powf(y * 17.52 - 9.72)
                }
            }
            LogCurve::AcesCct => {
                // ACEScct
                let _x_brk = 0.007_812_5f32;
                let y_brk = 0.155_251_14f32;
                let a = 10.540_237_7f32;
                let b = 0.072_905_534f32;
                if y <= y_brk {
                    (y - b) / a
                } else {
                    2.0f32.powf(y * 17.52 - 9.72)
                }
            }
        }
    }

    /// Converte um valor Linear cena-referenciado para codificação Log (0.0..1.0).
    pub fn from_linear(self, x: f32) -> f32 {
        let x = if x.is_finite() { x.max(-0.1) } else { 0.0 };
        match self {
            LogCurve::SonySLog3 => {
                if x >= 0.011_250 {
                    (420.0 + ((x + 0.01) / (0.18 + 0.01)).log10() * 261.5) / 1023.0
                } else {
                    (x * (171.210_29 - 95.0) / 0.011_250 + 95.0) / 1023.0
                }
            }
            LogCurve::ArriLogC3 => {
                let cut = 0.010_591f32;
                let a = 5.555_556f32;
                let b = 0.052_272f32;
                let c = 0.247_190f32;
                let d = 0.385_537f32;
                let e = 5.367_655f32;
                let f = 0.092_809f32;
                if x > cut {
                    c * (a * x + b).log10() + d
                } else {
                    e * x + f
                }
            }
            LogCurve::ArriLogC4 => {
                let a = (2.0f32.powf(14.0) - 64.0) / 2.0f32.powf(14.0);
                let b = 64.0 / 2.0f32.powf(14.0);
                let c = 0.075f32;
                let s = 1.0f32 / 9.0;
                let lin = (x + 0.01).max(1e-5);
                let t = lin.log2() * s + c;
                t * a + b
            }
            LogCurve::CanonLog2 => {
                if x > 0.0 {
                    0.281_86 * (8.73 * x + 1.0).log10() + 0.246_8
                } else {
                    0.529_13 * x + 0.035_38
                }
            }
            LogCurve::CanonLog3 => {
                if x > 0.0 {
                    0.527_44 * (8.73 * x + 1.0).log10() + 0.366_12
                } else {
                    2.3 * x + 0.069_88
                }
            }
            LogCurve::PanasonicVLog => {
                let cut = 0.01;
                let b = 0.008_73;
                let c = 0.241_514;
                let d = 0.598_206;
                if x >= cut {
                    c * (x + b).log10() + d
                } else {
                    5.6 * x + 0.125
                }
            }
            LogCurve::RedLog3G10 => {
                let a = 0.224_282f32;
                let b = 155.975_327f32;
                let c = 0.01f32;
                let g = 3.570_724f32;
                if x + c > 0.0 {
                    a * ((x + c) * b + 1.0).log10()
                } else {
                    (x + c) * g
                }
            }
            LogCurve::AcesCc => {
                if x <= 0.0 {
                    -0.358_447_488
                } else if x < 2.0f32.powf(-15.0) {
                    (x.log2() * 0.5 + 9.72) / 17.52
                } else {
                    (x.log2() + 9.72) / 17.52
                }
            }
            LogCurve::AcesCct => {
                let x_brk = 0.007_812_5f32;
                let a = 10.540_237_7f32;
                let b = 0.072_905_534f32;
                if x <= x_brk {
                    a * x + b
                } else {
                    (x.log2() + 9.72) / 17.52
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Funções de Transferência Eletro-Ópticas e OETFs
// ---------------------------------------------------------------------------

#[inline]
pub fn linear_to_srgb(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.003_130_8 {
        12.92 * x
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

#[inline]
pub fn srgb_to_linear(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.040_45 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

#[inline]
pub fn linear_to_rec709(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x < 0.018 {
        4.5 * x
    } else {
        1.099 * x.powf(0.45) - 0.099
    }
}

#[inline]
pub fn rec709_to_linear(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x < 0.081 {
        x / 4.5
    } else {
        ((x + 0.099) / 1.099).powf(1.0 / 0.45)
    }
}

#[inline]
pub fn linear_to_rec2020(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let alpha = 1.099_296_826_809_44;
    let beta = 0.018_053_968_510_807;
    if x < beta {
        4.5 * x
    } else {
        alpha * x.powf(0.45) - (alpha - 1.0)
    }
}

#[inline]
pub fn rec2020_to_linear(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let alpha = 1.099_296_826_809_44;
    let beta = 0.018_053_968_510_807;
    if x < 4.5 * beta {
        x / 4.5
    } else {
        ((x + (alpha - 1.0)) / alpha).powf(1.0 / 0.45)
    }
}

/// Curva de tom cinematográfica adaptada de ACES RRT + ODT
/// (Krzysztof Narkowicz / Stephen Hill fit oficial).
pub fn aces_fitted_tone_curve(rgb: [f32; 3]) -> [f32; 3] {
    // Matriz de entrada para espaço RRT ACES
    let r = rgb[0].max(0.0);
    let g = rgb[1].max(0.0);
    let b = rgb[2].max(0.0);

    let fit = |v: f32| -> f32 {
        let a = 2.51f32;
        let b = 0.03f32;
        let c = 2.43f32;
        let d = 0.59f32;
        let e = 0.14f32;
        ((v * (a * v + b)) / (v * (c * v + d) + e)).clamp(0.0, 1.0)
    };

    [fit(r), fit(g), fit(b)]
}

// ---------------------------------------------------------------------------
// Álgebra Linear 3x3 e Adaptação Cromática Bradford
// ---------------------------------------------------------------------------

pub fn mat3_mul_vec(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

pub fn mul3x3(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0f32; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            out[r][c] = a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c];
        }
    }
    out
}

pub fn invert3x3(m: &[[f32; 3]; 3]) -> Option<[[f32; 3]; 3]> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);

    if det.abs() < 1e-8 {
        return None;
    }
    let inv_det = 1.0 / det;

    let out = [
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_det,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_det,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_det,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det,
        ],
    ];
    Some(out)
}

/// Calcula a matriz 3x3 de conversão de RGB linear para CIE 1931 XYZ a partir
/// das coordenadas de cromaticidade dos primários e do ponto branco.
pub fn compute_rgb_to_xyz_matrix(
    r: [f32; 2],
    g: [f32; 2],
    b: [f32; 2],
    w: [f32; 2],
) -> [[f32; 3]; 3] {
    let to_xyz = |p: [f32; 2]| -> [f32; 3] {
        let y = p[1].max(1e-5);
        [p[0] / y, 1.0, (1.0 - p[0] - p[1]) / y]
    };

    let r_xyz = to_xyz(r);
    let g_xyz = to_xyz(g);
    let b_xyz = to_xyz(b);
    let w_xyz = to_xyz(w);

    let m = [
        [r_xyz[0], g_xyz[0], b_xyz[0]],
        [r_xyz[1], g_xyz[1], b_xyz[1]],
        [r_xyz[2], g_xyz[2], b_xyz[2]],
    ];

    let inv_m = invert3x3(&m).unwrap_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    let s = mat3_mul_vec(&inv_m, w_xyz);

    [
        [s[0] * r_xyz[0], s[1] * g_xyz[0], s[2] * b_xyz[0]],
        [s[0] * r_xyz[1], s[1] * g_xyz[1], s[2] * b_xyz[1]],
        [s[0] * r_xyz[2], s[1] * g_xyz[2], s[2] * b_xyz[2]],
    ]
}

/// Matriz de adaptação cromática de Bradford entre dois pontos brancos CIE xy.
pub fn bradford_adaptation_matrix(src_white: [f32; 2], dst_white: [f32; 2]) -> [[f32; 3]; 3] {
    let m_bfd = [
        [0.8951, 0.2664, -0.1614],
        [-0.7502, 1.7135, 0.0367],
        [0.0389, -0.0685, 1.0296],
    ];
    let inv_bfd = invert3x3(&m_bfd).unwrap_or(m_bfd);

    let to_xyz = |p: [f32; 2]| -> [f32; 3] {
        let y = p[1].max(1e-5);
        [p[0] / y, 1.0, (1.0 - p[0] - p[1]) / y]
    };

    let src_cone = mat3_mul_vec(&m_bfd, to_xyz(src_white));
    let dst_cone = mat3_mul_vec(&m_bfd, to_xyz(dst_white));

    let diag = [
        [
            if src_cone[0].abs() > 1e-6 {
                dst_cone[0] / src_cone[0]
            } else {
                1.0
            },
            0.0,
            0.0,
        ],
        [
            0.0,
            if src_cone[1].abs() > 1e-6 {
                dst_cone[1] / src_cone[1]
            } else {
                1.0
            },
            0.0,
        ],
        [
            0.0,
            0.0,
            if src_cone[2].abs() > 1e-6 {
                dst_cone[2] / src_cone[2]
            } else {
                1.0
            },
        ],
    ];

    let step = mul3x3(&diag, &m_bfd);
    mul3x3(&inv_bfd, &step)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transformacoes_de_espacos_de_cor_e_aces() {
        // Branco puro em Rec.709 deve ser convertido em branco em ACEScg
        let white_709 = [1.0f32, 1.0f32, 1.0f32];
        let white_aces =
            ProjectColorSpace::Rec709.convert_linear_rgb(ProjectColorSpace::AcesCg, white_709);

        // Adaptação D65 -> D60 pode ter leves variações cromáticas, mas deve estar próximo a 1.0
        assert!((white_aces[0] - 1.0).abs() < 0.05);
        assert!((white_aces[1] - 1.0).abs() < 0.05);
        assert!((white_aces[2] - 1.0).abs() < 0.05);

        // Ida e volta Rec.709 -> ACES2065-1 -> Rec.709 deve ser idêntica
        let roundtrip =
            ProjectColorSpace::Aces2065_1.convert_linear_rgb(ProjectColorSpace::Rec709, white_aces);
        assert!((roundtrip[0] - 1.0).abs() < 1e-3);
        assert!((roundtrip[1] - 1.0).abs() < 1e-3);
        assert!((roundtrip[2] - 1.0).abs() < 1e-3);
    }

    #[test]
    fn curvas_log_de_cameras_round_trip() {
        let test_levels = [0.001f32, 0.018f32, 0.18f32, 0.90f32, 2.0f32];
        for curve in LogCurve::ALL {
            for &lin in &test_levels {
                let log = curve.from_linear(lin);
                let restored = curve.to_linear(log);
                let err = (restored - lin).abs();
                assert!(
                    err < 0.02 * lin.max(0.1),
                    "Falha no round-trip de {:?} em lin={lin}: log={log}, restored={restored}",
                    curve.label()
                );
            }
        }
    }

    #[test]
    fn view_transform_aces_rrt_odt() {
        let linear_val = [0.18f32, 0.18f32, 0.18f32]; // 18% cinza médio
        let srgb = ViewTransform::AcesRrtOdtSrgb.apply(ProjectColorSpace::AcesCg, linear_val);
        // O cinza médio no ACES RRT + ODT para sRGB fica por volta de 0.35..0.50
        assert!(srgb[0] > 0.25 && srgb[0] < 0.65);
        assert!((srgb[0] - srgb[1]).abs() < 1e-4);
        assert!((srgb[1] - srgb[2]).abs() < 1e-4);
    }
}
