//! Máscara Bézier (Bézier Masking).
//!
//! Todas as formas de máscara (elipse, retângulo, retângulo arredondado,
//! polígono e forma livre) são representadas internamente pela mesma estrutura
//! unificada: uma curva Bézier cúbica fechada (`MaskPath`), compartilhando o
//! mesmo modelo de nós de controle e os mesmos quadros-chave de interpolação.

#![allow(clippy::needless_range_loop)]

use crate::engine::Frame;
use crate::model::Ticks;
use serde::{Deserialize, Serialize};

/// Constante matemática para aproximação de quarto de círculo por curva Bézier cúbica:
/// `4 * (sqrt(2) - 1) / 3 ~= 0.55228475`.
pub const KAPPA_BEZIER: f32 = 0.552_284_75;

/// Vértice de uma curva Bézier com alças de controle tangentes de entrada e saída.
///
/// As tangentes `handle_in` e `handle_out` são vetores relativos ao ponto principal
/// `point`. Se ambas forem `[0.0, 0.0]`, o vértice é um canto vivo (linear).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaskVertex {
    /// Coordenada principal do vértice (normalizada 0.0..=1.0 em relação ao quadro).
    pub point: [f32; 2],
    /// Vetor de tangente de entrada, relativo a `point`.
    pub handle_in: [f32; 2],
    /// Vetor de tangente de saída, relativo a `point`.
    pub handle_out: [f32; 2],
}

impl MaskVertex {
    /// Cria um canto afiado/linear sem curvatura.
    pub const fn corner(point: [f32; 2]) -> Self {
        Self {
            point,
            handle_in: [0.0, 0.0],
            handle_out: [0.0, 0.0],
        }
    }

    /// Cria um nó suave com tangentes de entrada e saída relativas.
    pub const fn smooth(point: [f32; 2], handle_in: [f32; 2], handle_out: [f32; 2]) -> Self {
        Self {
            point,
            handle_in,
            handle_out,
        }
    }

    /// Interpola linearmente entre dois vértices.
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let omt = 1.0 - t;
        Self {
            point: [
                self.point[0] * omt + other.point[0] * t,
                self.point[1] * omt + other.point[1] * t,
            ],
            handle_in: [
                self.handle_in[0] * omt + other.handle_in[0] * t,
                self.handle_in[1] * omt + other.handle_in[1] * t,
            ],
            handle_out: [
                self.handle_out[0] * omt + other.handle_out[0] * t,
                self.handle_out[1] * omt + other.handle_out[1] * t,
            ],
        }
    }
}

/// Modo de mesclagem / aplicação da máscara sobre a imagem.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MaskBlendMode {
    /// Mantém apenas o interior da máscara (corta o exterior).
    #[default]
    Add,
    /// Subtrai o interior da máscara (cria um buraco / recorte).
    Subtract,
    /// Multiplica o canal alfa da imagem pelo valor da máscara.
    Multiply,
    /// Inverte o mapa de máscara antes da aplicação.
    Invert,
}

impl MaskBlendMode {
    pub const ALL: [MaskBlendMode; 4] = [
        MaskBlendMode::Add,
        MaskBlendMode::Subtract,
        MaskBlendMode::Multiply,
        MaskBlendMode::Invert,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MaskBlendMode::Add => "Adicionar (Normal)",
            MaskBlendMode::Subtract => "Subtrair (Recorte)",
            MaskBlendMode::Multiply => "Multiplicar",
            MaskBlendMode::Invert => "Inverter",
        }
    }
}

/// Caminho de máscara Bézier fechado unificado.
///
/// Todas as formas geométricas fundamentais (elipses, retângulos, cantos arredondados,
/// polígonos e curvas livres) são expressas diretamente como uma sequência fechada de nós
/// Bézier cúbicos.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaskPath {
    pub vertices: Vec<MaskVertex>,
    #[serde(default = "default_closed")]
    pub closed: bool,
}

fn default_closed() -> bool {
    true
}

impl Default for MaskPath {
    fn default() -> Self {
        Self::ellipse([0.5, 0.5], [0.35, 0.25])
    }
}

impl MaskPath {
    /// Cria uma elipse centrada em `center` com raios `radius` usando 4 curvas Bézier cúbicas
    /// ponderadas pelo fator $\kappa \approx 0.55228475$.
    pub fn ellipse(center: [f32; 2], radius: [f32; 2]) -> Self {
        let (cx, cy) = (center[0], center[1]);
        let (rx, ry) = (radius[0].abs().max(1e-5), radius[1].abs().max(1e-5));
        let kx = rx * KAPPA_BEZIER;
        let ky = ry * KAPPA_BEZIER;

        let vertices = vec![
            // Topo
            MaskVertex::smooth([cx, cy - ry], [-kx, 0.0], [kx, 0.0]),
            // Direita
            MaskVertex::smooth([cx + rx, cy], [0.0, -ky], [0.0, ky]),
            // Fundo
            MaskVertex::smooth([cx, cy + ry], [kx, 0.0], [-kx, 0.0]),
            // Esquerda
            MaskVertex::smooth([cx - rx, cy], [0.0, ky], [0.0, -ky]),
        ];

        Self {
            vertices,
            closed: true,
        }
    }

    /// Cria um retângulo orientado com 4 cantos vivos.
    pub fn rect(p0: [f32; 2], p1: [f32; 2]) -> Self {
        let x_min = p0[0].min(p1[0]);
        let x_max = p0[0].max(p1[0]);
        let y_min = p0[1].min(p1[1]);
        let y_max = p0[1].max(p1[1]);

        let vertices = vec![
            MaskVertex::corner([x_min, y_min]),
            MaskVertex::corner([x_max, y_min]),
            MaskVertex::corner([x_max, y_max]),
            MaskVertex::corner([x_min, y_max]),
        ];

        Self {
            vertices,
            closed: true,
        }
    }

    /// Cria um retângulo com cantos arredondados usando 8 vértices Bézier cúbicos.
    pub fn rounded_rect(p0: [f32; 2], p1: [f32; 2], corner_radius: f32) -> Self {
        let x_min = p0[0].min(p1[0]);
        let x_max = p0[0].max(p1[0]);
        let y_min = p0[1].min(p1[1]);
        let y_max = p0[1].max(p1[1]);

        let max_r = ((x_max - x_min) * 0.5).min((y_max - y_min) * 0.5);
        let r = corner_radius.clamp(0.0, max_r.max(0.0));
        if r <= 1e-5 {
            return Self::rect(p0, p1);
        }

        let kr = r * KAPPA_BEZIER;

        let vertices = vec![
            // Topo (início canto superior direito)
            MaskVertex::smooth([x_max - r, y_min], [0.0, 0.0], [kr, 0.0]),
            // Direita (fim canto superior direito)
            MaskVertex::smooth([x_max, y_min + r], [0.0, -kr], [0.0, 0.0]),
            // Direita (início canto inferior direito)
            MaskVertex::smooth([x_max, y_max - r], [0.0, 0.0], [0.0, kr]),
            // Fundo (fim canto inferior direito)
            MaskVertex::smooth([x_max - r, y_max], [kr, 0.0], [0.0, 0.0]),
            // Fundo (início canto inferior esquerdo)
            MaskVertex::smooth([x_min + r, y_max], [0.0, 0.0], [-kr, 0.0]),
            // Esquerda (fim canto inferior esquerdo)
            MaskVertex::smooth([x_min, y_max - r], [0.0, kr], [0.0, 0.0]),
            // Esquerda (início canto superior esquerdo)
            MaskVertex::smooth([x_min, y_min + r], [0.0, 0.0], [0.0, -kr]),
            // Topo (fim canto superior esquerdo)
            MaskVertex::smooth([x_min + r, y_min], [-kr, 0.0], [0.0, 0.0]),
        ];

        Self {
            vertices,
            closed: true,
        }
    }

    /// Cria um polígono com $N$ vértices lineares.
    pub fn polygon(points: &[[f32; 2]]) -> Self {
        let vertices = points
            .iter()
            .copied()
            .map(MaskVertex::corner)
            .collect::<Vec<_>>();
        Self {
            vertices,
            closed: true,
        }
    }

    /// Cria uma máscara livre a partir de nós customizados.
    pub fn freeform(vertices: Vec<MaskVertex>, closed: bool) -> Self {
        Self { vertices, closed }
    }

    /// Avalia a curva Bézier cúbica entre dois nós e devolve os pontos amostrados.
    fn sample_segment(v0: &MaskVertex, v1: &MaskVertex, samples: usize, out: &mut Vec<[f32; 2]>) {
        let p0 = v0.point;
        let p1 = [
            v0.point[0] + v0.handle_out[0],
            v0.point[1] + v0.handle_out[1],
        ];
        let p2 = [v1.point[0] + v1.handle_in[0], v1.point[1] + v1.handle_in[1]];
        let p3 = v1.point;

        // Se for linear, apenas insere o ponto de partida
        let is_linear = (v0.handle_out[0].abs() < 1e-5 && v0.handle_out[1].abs() < 1e-5)
            && (v1.handle_in[0].abs() < 1e-5 && v1.handle_in[1].abs() < 1e-5);

        if is_linear || samples <= 1 {
            out.push(p0);
            return;
        }

        let step = 1.0 / samples as f32;
        for i in 0..samples {
            let t = i as f32 * step;
            let omt = 1.0 - t;
            let omt2 = omt * omt;
            let omt3 = omt2 * omt;
            let t2 = t * t;
            let t3 = t2 * t;

            let x = omt3 * p0[0] + 3.0 * omt2 * t * p1[0] + 3.0 * omt * t2 * p2[0] + t3 * p3[0];
            let y = omt3 * p0[1] + 3.0 * omt2 * t * p1[1] + 3.0 * omt * t2 * p2[1] + t3 * p3[1];
            out.push([x, y]);
        }
    }

    /// Converte a curva Bézier em uma lista de vértices poligonais densos
    /// para rasterização.
    pub fn flatten(&self, samples_per_curve: usize) -> Vec<[f32; 2]> {
        let n = self.vertices.len();
        if n < 2 {
            return self.vertices.iter().map(|v| v.point).collect();
        }

        let samples = samples_per_curve.clamp(4, 64);
        let mut poly = Vec::with_capacity(n * samples);

        for i in 0..n {
            let next_idx = if i + 1 < n {
                i + 1
            } else if self.closed {
                0
            } else {
                break;
            };

            let v0 = &self.vertices[i];
            let v1 = &self.vertices[next_idx];
            Self::sample_segment(v0, v1, samples, &mut poly);
        }

        poly
    }

    /// Redimensiona e reamostra o caminho para uma quantidade fixa de nós de controle,
    /// permitindo interpolação suave entre formas com contagens de nós distintas.
    pub fn resample_nodes(&self, target_nodes: usize) -> Self {
        let target = target_nodes.clamp(3, 64);
        let poly = self.flatten(16);
        if poly.is_empty() {
            return Self::default();
        }

        let mut resampled = Vec::with_capacity(target);
        let total_pts = poly.len();
        for i in 0..target {
            let idx = (i * total_pts) / target;
            let p = poly[idx.min(total_pts.saturating_sub(1))];
            resampled.push(MaskVertex::corner(p));
        }

        Self {
            vertices: resampled,
            closed: self.closed,
        }
    }

    /// Interpola entre dois caminhos Bézier (morfa de uma forma para outra).
    pub fn interpolate(&self, other: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        if self.vertices.is_empty() {
            return other.clone();
        }
        if other.vertices.is_empty() {
            return self.clone();
        }

        // Se possuem o mesmo número de nós, interpolação ponto a ponto direta
        if self.vertices.len() == other.vertices.len() {
            let mut v = Vec::with_capacity(self.vertices.len());
            for (a, b) in self.vertices.iter().zip(other.vertices.iter()) {
                v.push(a.lerp(b, t));
            }
            Self {
                vertices: v,
                closed: self.closed || other.closed,
            }
        } else {
            // Reamostra ambas para a contagem máxima para evitar saltos ou erros
            let max_nodes = self.vertices.len().max(other.vertices.len());
            let r1 = self.resample_nodes(max_nodes);
            let r2 = other.resample_nodes(max_nodes);
            r1.interpolate(&r2, t)
        }
    }
}

/// Quadro-chave de animação de máscara.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaskKeyframe {
    pub time: Ticks,
    pub path: MaskPath,
    #[serde(default)]
    pub feather: f32,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub expansion: f32,
}

fn default_opacity() -> f32 {
    1.0
}

/// Dados completos do efeito de Máscara Bézier.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BezierMaskData {
    pub default_path: MaskPath,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keyframes: Vec<MaskKeyframe>,
    #[serde(default)]
    pub mode: MaskBlendMode,
    #[serde(default)]
    pub feather: f32,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub expansion: f32,
    #[serde(default)]
    pub invert: bool,
}

impl Default for BezierMaskData {
    fn default() -> Self {
        Self {
            default_path: MaskPath::default(),
            keyframes: Vec::new(),
            mode: MaskBlendMode::Add,
            feather: 0.0,
            opacity: 1.0,
            expansion: 0.0,
            invert: false,
        }
    }
}

impl BezierMaskData {
    /// Avalia a máscara no instante de tempo `t`.
    pub fn evaluate_at(&self, t: Ticks) -> (MaskPath, f32, f32, f32) {
        if self.keyframes.is_empty() {
            return (
                self.default_path.clone(),
                self.feather,
                self.opacity,
                self.expansion,
            );
        }

        // Antes do primeiro quadro-chave
        if t <= self.keyframes[0].time {
            let k = &self.keyframes[0];
            return (k.path.clone(), k.feather, k.opacity, k.expansion);
        }

        // Depois do último quadro-chave
        let last_idx = self.keyframes.len() - 1;
        if t >= self.keyframes[last_idx].time {
            let k = &self.keyframes[last_idx];
            return (k.path.clone(), k.feather, k.opacity, k.expansion);
        }

        // Busca binária / linear entre dois quadros-chave
        for i in 0..last_idx {
            let k0 = &self.keyframes[i];
            let k1 = &self.keyframes[i + 1];
            if t >= k0.time && t <= k1.time {
                let span = (k1.time.0 - k0.time.0).max(1) as f32;
                let progress = ((t.0 - k0.time.0) as f32 / span).clamp(0.0, 1.0);
                let path = k0.path.interpolate(&k1.path, progress);
                let feather = k0.feather + (k1.feather - k0.feather) * progress;
                let opacity = k0.opacity + (k1.opacity - k0.opacity) * progress;
                let expansion = k0.expansion + (k1.expansion - k0.expansion) * progress;
                return (path, feather, opacity, expansion);
            }
        }

        (
            self.default_path.clone(),
            self.feather,
            self.opacity,
            self.expansion,
        )
    }
}

// ---------------------------------------------------------------------------
// Rasterização, Suavização (Feathering) e Aplicação
// ---------------------------------------------------------------------------

/// Rasteriza um polígono normalizado (0.0..1.0) em uma máscara de cobertura com anti-aliasing.
pub fn rasterize_mask(
    polygon: &[[f32; 2]],
    width: usize,
    height: usize,
    expansion_px: f32,
) -> Vec<f32> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    if polygon.len() < 3 {
        return vec![0.0; width * height];
    }

    let w = width as f32;
    let h = height as f32;

    // Converte para coordenadas de pixel e aplica expansão simples se solicitada
    let mut poly_px = Vec::with_capacity(polygon.len());
    let (mut cx, mut cy) = (0.0f32, 0.0f32);
    for p in polygon {
        let px = p[0] * w;
        let py = p[1] * h;
        cx += px;
        cy += py;
        poly_px.push([px, py]);
    }
    let inv_len = 1.0 / polygon.len() as f32;
    cx *= inv_len;
    cy *= inv_len;

    if expansion_px.abs() > 1e-3 {
        for p in &mut poly_px {
            let dx = p[0] - cx;
            let dy = p[1] - cy;
            let len = (dx * dx + dy * dy).sqrt().max(1e-4);
            p[0] += (dx / len) * expansion_px;
            p[1] += (dy / len) * expansion_px;
        }
    }

    let mut mask = vec![0.0f32; width * height];
    let n = poly_px.len();
    let mut intersections = Vec::with_capacity(16);

    for y in 0..height {
        let y_scan = y as f32 + 0.5;
        intersections.clear();

        for i in 0..n {
            let j = if i + 1 < n { i + 1 } else { 0 };
            let p0 = poly_px[i];
            let p1 = poly_px[j];

            if (p0[1] <= y_scan && p1[1] > y_scan) || (p1[1] <= y_scan && p0[1] > y_scan) {
                let dy = p1[1] - p0[1];
                if dy.abs() > 1e-5 {
                    let t = (y_scan - p0[1]) / dy;
                    let x = p0[0] + t * (p1[0] - p0[0]);
                    intersections.push(x);
                }
            }
        }

        intersections.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let row_start = y * width;
        let mut idx = 0;
        while idx + 1 < intersections.len() {
            let x0 = intersections[idx].clamp(0.0, w);
            let x1 = intersections[idx + 1].clamp(0.0, w);

            let i0 = x0.floor() as usize;
            let i1 = (x1.ceil() as usize).min(width);

            for x in i0..i1 {
                let pixel_left = x as f32;
                let pixel_right = pixel_left + 1.0;
                let overlap_start = x0.max(pixel_left);
                let overlap_end = x1.min(pixel_right);
                let coverage = (overlap_end - overlap_start).max(0.0);

                if let Some(target) = mask.get_mut(row_start + x) {
                    *target = (*target + coverage).clamp(0.0, 1.0);
                }
            }
            idx += 2;
        }
    }

    mask
}

/// Aplica suavização (feathering) na máscara via filtro de desfoque separável em dois passos.
pub fn apply_feather(mask: &mut [f32], width: usize, height: usize, radius_px: f32) {
    if width == 0 || height == 0 || radius_px <= 0.5 {
        return;
    }
    let r = (radius_px.round() as usize).clamp(1, 128);
    let mut temp = vec![0.0f32; width * height];

    // Passo 1: Desfoque horizontal
    let div_h = (2 * r + 1) as f32;
    for y in 0..height {
        let row_offset = y * width;
        let mut sum = 0.0f32;

        // Inicializa a janela com extensão de borda
        let first_val = mask[row_offset];
        sum += first_val * r as f32;
        for k in 0..=r.min(width.saturating_sub(1)) {
            sum += mask[row_offset + k];
        }

        for x in 0..width {
            temp[row_offset + x] = sum / div_h;

            let left = if x >= r {
                mask[row_offset + x - r]
            } else {
                first_val
            };

            let right = if x + r + 1 < width {
                mask[row_offset + x + r + 1]
            } else {
                mask[row_offset + width - 1]
            };

            sum += right - left;
        }
    }

    // Passo 2: Desfoque vertical
    let div_v = (2 * r + 1) as f32;
    for x in 0..width {
        let mut sum = 0.0f32;
        let first_val = temp[x];
        sum += first_val * r as f32;
        for k in 0..=r.min(height.saturating_sub(1)) {
            sum += temp[k * width + x];
        }

        for y in 0..height {
            mask[y * width + x] = (sum / div_v).clamp(0.0, 1.0);

            let top = if y >= r {
                temp[(y - r) * width + x]
            } else {
                first_val
            };

            let bottom = if y + r + 1 < height {
                temp[(y + r + 1) * width + x]
            } else {
                temp[(height - 1) * width + x]
            };

            sum += bottom - top;
        }
    }
}

/// Aplica o buffer de máscara sobre os pixels RGBA pré-multiplicados de um `Frame`.
pub fn apply_mask_to_frame(
    f: &mut Frame,
    mask: &[f32],
    mode: MaskBlendMode,
    opacity: f32,
    invert: bool,
) {
    if f.px.len() != mask.len() {
        return;
    }

    let op = opacity.clamp(0.0, 1.0);

    for (p, &raw_m) in f.px.iter_mut().zip(mask.iter()) {
        let mut m = raw_m.clamp(0.0, 1.0);
        if invert {
            m = 1.0 - m;
        }

        let factor = match mode {
            MaskBlendMode::Add => 1.0 - op * (1.0 - m),
            MaskBlendMode::Subtract => 1.0 - op * m,
            MaskBlendMode::Multiply => m * op + (1.0 - op),
            MaskBlendMode::Invert => 1.0 - op * m,
        };

        let factor = factor.clamp(0.0, 1.0);
        p[0] *= factor;
        p[1] *= factor;
        p[2] *= factor;
        p[3] *= factor;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unificacao_das_formas_de_mascara_bezier() {
        // 1. Elipse com 4 vértices cúbicos e tangentes de kappa
        let el = MaskPath::ellipse([0.5, 0.5], [0.2, 0.2]);
        assert_eq!(el.vertices.len(), 4);
        assert!(el.closed);
        assert!(el.vertices[0].handle_out[0] > 0.0);

        // 2. Retângulo com 4 cantos vivos
        let rc = MaskPath::rect([0.1, 0.1], [0.9, 0.9]);
        assert_eq!(rc.vertices.len(), 4);
        assert_eq!(rc.vertices[0].handle_in, [0.0, 0.0]);
        assert_eq!(rc.vertices[0].handle_out, [0.0, 0.0]);

        // 3. Retângulo arredondado com 8 nós
        let rrc = MaskPath::rounded_rect([0.1, 0.1], [0.9, 0.9], 0.05);
        assert_eq!(rrc.vertices.len(), 8);

        // 4. Polígono com 5 vértices
        let poly = MaskPath::polygon(&[[0.5, 0.1], [0.9, 0.4], [0.7, 0.9], [0.3, 0.9], [0.1, 0.4]]);
        assert_eq!(poly.vertices.len(), 5);

        // 5. Interpolação / morfing sem pânico mesmo com contagens diferentes de vértices
        let morphed = el.interpolate(&rrc, 0.5);
        assert!(!morphed.vertices.is_empty());
    }

    #[test]
    fn rasterizacao_feather_e_modos_de_mesclagem() {
        let rect = MaskPath::rect([0.25, 0.25], [0.75, 0.75]);
        let poly = rect.flatten(4);
        let (w, h) = (100, 100);

        let mut mask = rasterize_mask(&poly, w, h, 0.0);
        assert_eq!(mask.len(), w * h);

        // Centro (50, 50) deve estar coberto (1.0)
        assert!(mask[50 * w + 50] > 0.99);
        // Canto exterior (5, 5) deve ser 0.0
        assert_eq!(mask[5 * w + 5], 0.0);

        // Aplicação de feathering
        apply_feather(&mut mask, w, h, 3.0);
        // O centro permanece alto e as bordas suavizadas
        assert!(mask[50 * w + 50] > 0.9);
        assert!(mask[25 * w + 25] > 0.1 && mask[25 * w + 25] < 0.9);

        // Teste em Frame
        let mut frame = Frame::new(w, h);
        for p in &mut frame.px {
            *p = [1.0, 1.0, 1.0, 1.0];
        }

        apply_mask_to_frame(&mut frame, &mask, MaskBlendMode::Add, 1.0, false);
        // Centro mantém cor/alfa quase intacto
        assert!(frame.px[50 * w + 50][3] > 0.9);
        // Canto exterior foi cortado para 0
        assert!(frame.px[5 * w + 5][3] < 0.01);
    }
}
