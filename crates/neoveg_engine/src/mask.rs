//! Máscara Bézier (Bézier Masking).
//!
//! Todas as formas de máscara (elipse, retângulo, retângulo arredondado,
//! polígono e forma livre) são representadas internamente pela mesma estrutura
//! unificada: uma curva Bézier cúbica fechada (`MaskPath`), compartilhando o
//! mesmo modelo de nós de controle e os mesmos quadros-chave de interpolação.

#![allow(clippy::needless_range_loop)]

use crate::engine::Frame;
use crate::model::Ticks;
use neoveg_common::curve::Curve;
use serde::{Deserialize, Serialize};

/// Constante matemática para aproximação de quarto de círculo por curva Bézier cúbica:
/// `4 * (sqrt(2) - 1) / 3 ~= 0.55228475`.
pub const KAPPA_BEZIER: f32 = 0.552_284_8;

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
    /// Desloca todos os vértices da máscara por `(dx, dy)` em coordenadas normalizadas.
    pub fn translate(&mut self, dx: f32, dy: f32) {
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }
        for v in &mut self.vertices {
            v.point[0] = (v.point[0] + dx).clamp(-1.0, 2.0);
            v.point[1] = (v.point[1] + dy).clamp(-1.0, 2.0);
        }
    }

    /// Verifica se o ponto normalizado `p` está no interior da máscara (ray-casting).
    pub fn contains_point(&self, p: [f32; 2]) -> bool {
        if !p[0].is_finite() || !p[1].is_finite() || self.vertices.len() < 3 {
            return false;
        }
        let poly = self.flatten(12);
        let n = poly.len();
        if n < 3 {
            return false;
        }
        let mut inside = false;
        let mut j = n - 1;
        for i in 0..n {
            let pi = poly[i];
            let pj = poly[j];
            let crosses = (pi[1] > p[1]) != (pj[1] > p[1]);
            if crosses {
                let dy = pj[1] - pi[1];
                if dy.abs() > 1e-6 {
                    let x_int = pi[0] + (p[1] - pi[1]) * (pj[0] - pi[0]) / dy;
                    if p[0] < x_int {
                        inside = !inside;
                    }
                }
            }
            j = i;
        }
        inside
    }

    /// Insere um novo vértice no segmento mais próximo de `p` (evitando laços cruzados).
    /// Devolve o índice do vértice inserido.
    pub fn insert_vertex_on_nearest_segment(&mut self, p: [f32; 2]) -> usize {
        let p = [
            if p[0].is_finite() {
                p[0].clamp(-1.0, 2.0)
            } else {
                0.5
            },
            if p[1].is_finite() {
                p[1].clamp(-1.0, 2.0)
            } else {
                0.5
            },
        ];
        if self.vertices.len() >= 128 {
            return self.vertices.len().saturating_sub(1);
        }
        let n = self.vertices.len();
        if n < 2 {
            self.vertices.push(MaskVertex::corner(p));
            return self.vertices.len().saturating_sub(1);
        }
        let seg_count = if self.closed { n } else { n - 1 };
        let mut best_seg = n - 1;
        let mut best_dist2 = f32::INFINITY;
        for i in 0..seg_count {
            let a = self.vertices[i].point;
            let b = self.vertices[(i + 1) % n].point;
            let abx = b[0] - a[0];
            let aby = b[1] - a[1];
            let len2 = abx * abx + aby * aby;
            let t = if len2 > 1e-8 {
                (((p[0] - a[0]) * abx + (p[1] - a[1]) * aby) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let proj = [a[0] + abx * t, a[1] + aby * t];
            let d2 = (p[0] - proj[0]).powi(2) + (p[1] - proj[1]).powi(2);
            if d2 < best_dist2 {
                best_dist2 = d2;
                best_seg = i;
            }
        }
        let insert_idx = (best_seg + 1).min(self.vertices.len());
        self.vertices.insert(insert_idx, MaskVertex::corner(p));
        insert_idx
    }

    /// Alterna o vértice `idx` entre canto vivo (sem tangentes) e nó suave Bézier.
    pub fn toggle_vertex_tangent(&mut self, idx: usize) {
        let n = self.vertices.len();
        if idx >= n || n < 2 {
            return;
        }
        let v = self.vertices[idx];
        let has_handles = v.handle_in[0].abs() > 1e-4
            || v.handle_in[1].abs() > 1e-4
            || v.handle_out[0].abs() > 1e-4
            || v.handle_out[1].abs() > 1e-4;
        if has_handles {
            self.vertices[idx].handle_in = [0.0, 0.0];
            self.vertices[idx].handle_out = [0.0, 0.0];
        } else {
            let prev = self.vertices[(idx + n - 1) % n].point;
            let next = self.vertices[(idx + 1) % n].point;
            let tx = (next[0] - prev[0]) * 0.2;
            let ty = (next[1] - prev[1]) * 0.2;
            self.vertices[idx].handle_in = [-tx, -ty];
            self.vertices[idx].handle_out = [tx, ty];
        }
    }

    /// Saneia vértices contra entradas hostis (NaN/infinito e quantidade excessiva).
    pub fn sanitize(&mut self) {
        if self.vertices.len() > 128 {
            self.vertices.truncate(128);
        }
        for v in &mut self.vertices {
            for c in &mut v.point {
                *c = if c.is_finite() {
                    c.clamp(-2.0, 3.0)
                } else {
                    0.5
                };
            }
            for c in v.handle_in.iter_mut().chain(v.handle_out.iter_mut()) {
                *c = if c.is_finite() {
                    c.clamp(-2.0, 2.0)
                } else {
                    0.0
                };
            }
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
    #[serde(default)]
    pub curve: Curve,
    /// Suavidade da curva de interpolação (`0..=100`, igual à linha de Posição).
    #[serde(default)]
    pub smoothness: f32,
}

impl MaskKeyframe {
    pub fn smoothness_safe(&self) -> f32 {
        if self.smoothness.is_finite() {
            self.smoothness.clamp(0.0, 100.0)
        } else {
            0.0
        }
    }
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
    /// Aplica curva e suavidade (`0..=100`) sobre o progresso linear `[0, 1]`,
    /// com a mesma matemática de `eval_pan_crop_smooth`.
    pub fn shape_progress(curve: Curve, smoothness: f32, lin: f32) -> f32 {
        let mut u = curve.shape(lin.clamp(0.0, 1.0));
        let sm = if smoothness.is_finite() {
            (smoothness * 0.01).clamp(0.0, 1.0)
        } else {
            0.0
        };
        if sm > 0.0 && curve != Curve::Hold {
            let hermite = u * u * (3.0 - 2.0 * u);
            let quintic = u * u * u * (u * (u * 6.0 - 15.0) + 10.0);
            let target = hermite * (1.0 - sm) + quintic * sm;
            u = u + (target - u) * sm;
        }
        u.clamp(0.0, 1.0)
    }

    /// Índice do quadro-chave ativo no tempo `t` (último `<= t + tol`, ou 0).
    pub fn active_key_index(&self, t: Ticks, tol: Ticks) -> Option<usize> {
        if self.keyframes.is_empty() {
            return None;
        }
        Some(
            self.keyframes
                .iter()
                .rposition(|k| k.time <= t + tol)
                .unwrap_or(0),
        )
    }

    /// Atualiza o estado da máscara no tempo `t`: quando não há quadros-chave,
    /// altera o caminho padrão; quando já há quadros-chave, atualiza o quadro
    /// dentro de `tol` (preservando curva e suavidade) ou cria um novo quadro.
    pub fn set_at(
        &mut self,
        t: Ticks,
        tol: Ticks,
        path: MaskPath,
        feather: f32,
        opacity: f32,
        expansion: f32,
    ) {
        let feather = if feather.is_finite() {
            feather.clamp(0.0, 100.0)
        } else {
            0.0
        };
        let opacity = if opacity.is_finite() {
            opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let expansion = if expansion.is_finite() {
            expansion.clamp(-100.0, 100.0)
        } else {
            0.0
        };
        if self.keyframes.is_empty() {
            self.default_path = path;
            self.feather = feather;
            self.opacity = opacity;
            self.expansion = expansion;
            return;
        }
        if let Some(k) = self
            .keyframes
            .iter_mut()
            .find(|k| k.time.abs_diff(t) <= tol)
        {
            k.path = path;
            k.feather = feather;
            k.opacity = opacity;
            k.expansion = expansion;
            return;
        }
        let (curve, smoothness) = self
            .active_key_index(t, tol)
            .and_then(|i| self.keyframes.get(i))
            .map(|k| (k.curve, k.smoothness_safe()))
            .unwrap_or((Curve::Smooth, 0.0));
        self.keyframes.push(MaskKeyframe {
            time: t.max(Ticks::ZERO),
            path,
            feather,
            opacity,
            expansion,
            curve,
            smoothness,
        });
        self.keyframes.sort_by_key(|k| k.time);
    }

    /// Garante que exista um quadro-chave em `t` (dentro de `tol`) e devolve o índice dele.
    pub fn ensure_key_at(&mut self, t: Ticks, tol: Ticks) -> usize {
        if let Some(idx) = self
            .keyframes
            .iter()
            .position(|k| k.time.abs_diff(t) <= tol)
        {
            return idx;
        }
        let (path, feather, opacity, expansion) = self.evaluate_at(t);
        let (curve, smoothness) = self
            .active_key_index(t, tol)
            .and_then(|i| self.keyframes.get(i))
            .map(|k| (k.curve, k.smoothness_safe()))
            .unwrap_or((Curve::Smooth, 0.0));
        let clamped_t = t.max(Ticks::ZERO);
        self.keyframes.push(MaskKeyframe {
            time: clamped_t,
            path,
            feather,
            opacity,
            expansion,
            curve,
            smoothness,
        });
        self.keyframes.sort_by_key(|k| k.time);
        self.keyframes
            .iter()
            .position(|k| k.time == clamped_t)
            .unwrap_or(0)
    }

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

        // Busca entre dois quadros-chave aplicando curva e suavidade
        for i in 0..last_idx {
            let k0 = &self.keyframes[i];
            let k1 = &self.keyframes[i + 1];
            if t >= k0.time && t <= k1.time {
                let lin = t.fraction_in(k0.time, k1.time);
                let progress = Self::shape_progress(k0.curve, k0.smoothness_safe(), lin);
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
        frame.px.fill([1.0, 1.0, 1.0, 1.0]);

        apply_mask_to_frame(&mut frame, &mask, MaskBlendMode::Add, 1.0, false);
        // Centro mantém cor/alfa quase intacto
        assert!(frame.px[50 * w + 50][3] > 0.9);
        // Canto exterior foi cortado para 0
        assert!(frame.px[5 * w + 5][3] < 0.01);
    }

    #[test]
    fn mascara_curva_suavidade_e_edicao_com_keyframes() {
        let mut md = BezierMaskData {
            default_path: MaskPath::rect([0.2, 0.2], [0.6, 0.6]),
            ..BezierMaskData::default()
        };
        let tol = Ticks::from_millis_i64(16);
        md.keyframes = vec![
            MaskKeyframe {
                time: Ticks::ZERO,
                path: MaskPath::rect([0.1, 0.1], [0.3, 0.3]),
                feather: 0.0,
                opacity: 1.0,
                expansion: 0.0,
                curve: Curve::Fast,
                smoothness: 0.0,
            },
            MaskKeyframe {
                time: Ticks::from_secs_i64(2),
                path: MaskPath::rect([0.5, 0.5], [0.9, 0.9]),
                feather: 20.0,
                opacity: 1.0,
                expansion: 0.0,
                curve: Curve::Linear,
                smoothness: 0.0,
            },
        ];
        // Em t=1s (metade), Curve::Fast deve avançar mais que 0.5 (feather > 10)
        let (_, f_fast, _, _) = md.evaluate_at(Ticks::from_secs_i64(1));
        assert!(f_fast > 14.0, "Curve::Fast esperado > 14, veio {f_fast}");

        // Curve::Hold mantém o valor inicial até o próximo quadro-chave
        md.keyframes[0].curve = Curve::Hold;
        let (_, f_hold, _, _) = md.evaluate_at(Ticks::from_secs_i64(1));
        assert!(f_hold < 0.01, "Curve::Hold esperado ~0, veio {f_hold}");

        // Suavidade > 0 muda a curva em t=0.25s em relação a Linear puro
        md.keyframes[0].curve = Curve::Linear;
        let (_, f_lin, _, _) = md.evaluate_at(Ticks::from_millis_i64(500));
        md.keyframes[0].smoothness = 100.0;
        let (_, f_sm, _, _) = md.evaluate_at(Ticks::from_millis_i64(500));
        assert!(
            f_sm < f_lin - 1.0,
            "suavidade 100 em 25% deve desacelerar no início: lin={f_lin}, sm={f_sm}"
        );

        // Editar em t=1s quando existem keyframes cria um keyframe novo e se reflete em evaluate_at
        let mut p_mid = MaskPath::rect([0.2, 0.2], [0.8, 0.8]);
        assert!(p_mid.contains_point([0.5, 0.5]));
        assert!(!p_mid.contains_point([0.05, 0.05]));
        p_mid.translate(0.1, 0.0);
        assert!((p_mid.vertices[0].point[0] - 0.3).abs() < 1e-5);
        let ins = p_mid.insert_vertex_on_nearest_segment([0.6, 0.2]);
        assert_eq!(ins, 1);
        assert_eq!(p_mid.vertices.len(), 5);
        p_mid.toggle_vertex_tangent(ins);
        assert!(p_mid.vertices[ins].handle_out[0].abs() > 1e-3);
        p_mid.toggle_vertex_tangent(ins);
        assert_eq!(p_mid.vertices[ins].handle_out, [0.0, 0.0]);

        md.set_at(Ticks::from_secs_i64(1), tol, p_mid.clone(), 5.0, 0.8, 2.0);
        assert_eq!(md.keyframes.len(), 3);
        let (eval_p, eval_f, eval_o, eval_e) = md.evaluate_at(Ticks::from_secs_i64(1));
        assert_eq!(eval_p.vertices.len(), 5);
        assert!((eval_f - 5.0).abs() < 1e-4);
        assert!((eval_o - 0.8).abs() < 1e-4);
        assert!((eval_e - 2.0).abs() < 1e-4);
    }
}
