//! Movimento da trilha em 3D (*Alfa da origem 3D*, manual do VEGAS Pro 21,
//! pp. 748–756): a camada vira um plano no espaço, com posição X/Y/Z,
//! tamanho, pivô de rotação, orientação separada da rotação, composição
//! Pai/Filho 3D por matriz 4×4 e câmera estereoscópica fora-de-eixo.
//!
//! Convenções:
//! - origem no centro do quadro, X para a direita, Y para baixo e Z na
//!   direção de quem assiste (Z positivo = mais perto, a imagem cresce);
//! - câmera em `(cam_x, 0, D)` olhando para −Z, com `D` = largura do quadro ×
//!   [`CAMERA_DIST`] (campo de visão horizontal de ~45°); no plano Z = 0 um
//!   pixel da camada é um pixel da tela, como no 2D;
//! - rotação aplicada como `R_orient * R_rot` (cada uma na ordem X → Y → Z)
//!   em torno do pivô `(pivot_x, pivot_y, pivot_z)`.
//!
//! O render mapeia cada pixel da saída para a camada pela inversa da
//! homografia do plano, sem passar por triângulos.

use super::Frame;
use crate::model::MotionKey;
use rayon::prelude::*;

/// Distância da câmera em larguras de quadro: 0,5 / tan(22,5°).
pub const CAMERA_DIST: f32 = 1.207_106_8;

/// Fração mínima da distância da câmera que um ponto precisa manter para
/// ser projetado (evita divisões perto de zero e pontos atrás da câmera).
const NEAR: f32 = 0.05;

/// Matriz homogênea 4×4 por linhas usada para compor transformações 3D e Pai/Filho.
pub type Mat4 = [[f32; 4]; 4];

fn fin(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

/// `true` se o quadro-chave não move, não gira nem orienta a camada (pula o render).
pub fn is_identity(m: &MotionKey) -> bool {
    m.x == 0.0
        && m.y == 0.0
        && m.z == 0.0
        && m.scale_x == 1.0
        && m.scale_y == 1.0
        && m.rot == 0.0
        && m.rot_x == 0.0
        && m.rot_y == 0.0
        && m.orient_x == 0.0
        && m.orient_y == 0.0
        && m.orient_z == 0.0
}

pub fn mat4_identity() -> Mat4 {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

pub fn mat4_mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [[0.0f32; 4]; 4];
    for r in 0..4 {
        for c in 0..4 {
            out[r][c] =
                a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c] + a[r][3] * b[3][c];
        }
    }
    out
}

fn mat3_mul(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0f32; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            out[r][c] = a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c];
        }
    }
    out
}

/// Matriz de rotação X → Y → Z (graus), por linhas.
fn euler_xyz(rx_deg: f32, ry_deg: f32, rz_deg: f32) -> [[f32; 3]; 3] {
    let (sx, cx) = fin(rx_deg, 0.0).to_radians().sin_cos();
    let (sy, cy) = fin(ry_deg, 0.0).to_radians().sin_cos();
    let (sz, cz) = fin(rz_deg, 0.0).to_radians().sin_cos();
    [
        [cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx],
        [sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx],
        [-sy, cy * sx, cy * cx],
    ]
}

/// Matriz de rotação composta `R_orient * R_rot`.
fn combined_rotation(m: &MotionKey) -> [[f32; 3]; 3] {
    let r_rot = euler_xyz(m.rot_x, m.rot_y, m.rot);
    if m.orient_x == 0.0 && m.orient_y == 0.0 && m.orient_z == 0.0 {
        r_rot
    } else {
        let r_orient = euler_xyz(m.orient_x, m.orient_y, m.orient_z);
        mat3_mul(r_orient, r_rot)
    }
}

/// Constrói a matriz 4×4 mundo da camada em pixels:
/// `T(pos + pivot) * R_orient * R_rot * T(-pivot) * S(sx, sy, 1)`.
pub fn motion_to_mat4(m: &MotionKey, w: f32, h: f32) -> Mat4 {
    let r = combined_rotation(m);
    let sx = fin(m.scale_x, 1.0).clamp(0.001, 100.0);
    let sy = fin(m.scale_y, 1.0).clamp(0.001, 100.0);
    let px = fin(m.pivot_x, 0.0) * w;
    let py = fin(m.pivot_y, 0.0) * h;
    let pz = fin(m.pivot_z, 0.0) * w;
    let tx = fin(m.x, 0.0) * w;
    let ty = fin(m.y, 0.0) * h;
    let tz = fin(m.z, 0.0) * w;

    // t_total = pos + pivot - R * pivot
    let rpx = r[0][0] * px + r[0][1] * py + r[0][2] * pz;
    let rpy = r[1][0] * px + r[1][1] * py + r[1][2] * pz;
    let rpz = r[2][0] * px + r[2][1] * py + r[2][2] * pz;

    [
        [r[0][0] * sx, r[0][1] * sy, r[0][2], tx + px - rpx],
        [r[1][0] * sx, r[1][1] * sy, r[1][2], ty + py - rpy],
        [r[2][0] * sx, r[2][1] * sy, r[2][2], tz + pz - rpz],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// Profundidade Z do centro da camada no mundo (para ordenação de trilhas 3D).
pub fn world_center_z(mat: &Mat4) -> f32 {
    fin(mat[2][3], 0.0)
}

/// Ponto da camada (pixels a partir do centro dela) no espaço do mundo,
/// para um quadro `w`×`h`.
pub fn layer_to_world(m: &MotionKey, w: f32, h: f32, u: f32, v: f32) -> [f32; 3] {
    let mat = motion_to_mat4(m, w, h);
    [
        mat[0][0] * u + mat[0][1] * v + mat[0][3],
        mat[1][0] * u + mat[1][1] * v + mat[1][3],
        mat[2][0] * u + mat[2][1] * v + mat[2][3],
    ]
}

/// Projeta um ponto do mundo na tela (pixels a partir do canto superior
/// esquerdo). `None` se estiver atrás da câmera ou colado nela.
pub fn project(p: [f32; 3], w: f32, h: f32) -> Option<(f32, f32)> {
    project_stereo(p, w, h, 0.0, 0.0)
}

/// Projeta um ponto do mundo com deslocamento horizontal de câmera (`cam_x_px`)
/// e plano de convergência (`conv_z_px`) para câmera estereoscópica 3D.
pub fn project_stereo(
    p: [f32; 3],
    w: f32,
    h: f32,
    cam_x_px: f32,
    conv_z_px: f32,
) -> Option<(f32, f32)> {
    let d = w.max(1.0) * CAMERA_DIST;
    let den = d - p[2];
    if !den.is_finite() || den < d * NEAR {
        return None;
    }
    let conv_den = (d - fin(conv_z_px, 0.0)).max(d * NEAR);
    let cam_x = fin(cam_x_px, 0.0);
    let shift = cam_x * d / conv_den;
    let s = d / den;
    Some((w * 0.5 + shift + (p[0] - cam_x) * s, h * 0.5 + p[1] * s))
}

/// Os quatro cantos da camada no mundo (sup. esq., sup. dir., inf. dir.,
/// inf. esq.).
pub fn corners_world(m: &MotionKey, w: f32, h: f32) -> [[f32; 3]; 4] {
    let (hw, hh) = (w * 0.5, h * 0.5);
    [
        layer_to_world(m, w, h, -hw, -hh),
        layer_to_world(m, w, h, hw, -hh),
        layer_to_world(m, w, h, hw, hh),
        layer_to_world(m, w, h, -hw, hh),
    ]
}

/// Homografia camada → tela (por linhas) a partir de uma matriz mundo `Mat4`
/// e deslocamento opcional de câmera estereoscópica.
fn homography_from_mat4(
    mat: &Mat4,
    w: f32,
    h: f32,
    cam_x_px: f32,
    conv_z_px: f32,
) -> [[f32; 3]; 3] {
    let d = w.max(1.0) * CAMERA_DIST;
    let conv_den = (d - fin(conv_z_px, 0.0)).max(d * NEAR);
    let cam_x = fin(cam_x_px, 0.0);
    let cx = w * 0.5 + cam_x * d / conv_den;
    let cy = h * 0.5;
    let cu = [mat[0][0], mat[1][0], mat[2][0]];
    let cv = [mat[0][1], mat[1][1], mat[2][1]];
    let ct = [mat[0][3] - cam_x, mat[1][3], mat[2][3]];
    let col = |k: [f32; 3], is_t: bool| -> [f32; 3] {
        let dz = if is_t { d - k[2] } else { -k[2] };
        [d * k[0] + cx * dz, d * k[1] + cy * dz, dz]
    };
    let (a, b, c) = (col(cu, false), col(cv, false), col(ct, true));
    [[a[0], b[0], c[0]], [a[1], b[1], c[1]], [a[2], b[2], c[2]]]
}

fn invert3(m: [[f32; 3]; 3]) -> Option<[[f32; 3]; 3]> {
    let [[a, b, c], [d, e, f], [g, h, i]] = m;
    let co_a = e * i - f * h;
    let co_b = -(d * i - f * g);
    let co_c = d * h - e * g;
    let det = a * co_a + b * co_b + c * co_c;
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    let k = 1.0 / det;
    Some([
        [co_a * k, -(b * i - c * h) * k, (b * f - c * e) * k],
        [co_b * k, (a * i - c * g) * k, -(a * f - c * d) * k],
        [co_c * k, -(a * h - b * g) * k, (a * e - b * d) * k],
    ])
}

/// Aplica uma matriz 3D arbitrária (incluindo composição Pai/Filho 3D e câmera
/// estereoscópica) a uma camada do tamanho do quadro.
pub fn apply_motion_3d_mat(f: &Frame, mat: &Mat4, cam_x_px: f32, conv_z_px: f32) -> Frame {
    let (w, h) = (f.w, f.h);
    let mut out = Frame::new(w, h);
    let (wf, hf) = (w as f32, h as f32);
    let Some(inv) = invert3(homography_from_mat4(mat, wf, hf, cam_x_px, conv_z_px)) else {
        return out;
    };
    out.px
        .par_chunks_mut(w.max(1))
        .enumerate()
        .for_each(|(y, row)| {
            let py = y as f32 + 0.5;
            for (x, p) in row.iter_mut().enumerate() {
                let px = x as f32 + 0.5;
                let a = inv[0][0] * px + inv[0][1] * py + inv[0][2];
                let b = inv[1][0] * px + inv[1][1] * py + inv[1][2];
                let c = inv[2][0] * px + inv[2][1] * py + inv[2][2];
                if c.is_nan() || c <= 1e-9 {
                    continue;
                }
                let (u, v) = (a / c, b / c);
                let (lx, ly) = (u + wf * 0.5, v + hf * 0.5);
                if lx < -0.5 || ly < -0.5 || lx > wf + 0.5 || ly > hf + 0.5 {
                    continue;
                }
                *p = f.sample(lx, ly);
            }
        });
    out
}

/// Aplica o movimento 3D a uma camada do tamanho do quadro.
pub fn apply_motion_3d(f: &Frame, m: &MotionKey) -> Frame {
    let mat = motion_to_mat4(m, f.w as f32, f.h as f32);
    apply_motion_3d_mat(f, &mat, 0.0, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(w: usize, h: usize) -> Frame {
        let mut f = Frame::new(w, h);
        for (i, p) in f.px.iter_mut().enumerate() {
            let x = i % w;
            *p = if x < w / 2 {
                [1.0, 0.0, 0.0, 1.0]
            } else {
                [0.0, 0.0, 1.0, 1.0]
            };
        }
        f
    }

    fn alpha_sum(f: &Frame) -> f32 {
        f.px.iter().map(|p| p[3]).sum()
    }

    #[test]
    fn sem_movimento_a_projecao_nao_muda_o_ponto() {
        let m = MotionKey::default();
        let p = layer_to_world(&m, 1920.0, 1080.0, -960.0, -540.0);
        let s = project(p, 1920.0, 1080.0).unwrap();
        assert!((s.0).abs() < 1e-3 && (s.1).abs() < 1e-3, "{s:?}");
        let f = apply_motion_3d(&layer(16, 8), &m);
        assert!(f.px[0][0] > 0.9 && f.px[15][2] > 0.9);
    }

    #[test]
    fn z_positivo_aproxima_e_aumenta() {
        let m = MotionKey {
            z: 0.3,
            ..MotionKey::default()
        };
        let c = corners_world(&m, 1920.0, 1080.0);
        let tl = project(c[0], 1920.0, 1080.0).unwrap();
        assert!(tl.0 < 0.0 && tl.1 < 0.0, "canto sai do quadro: {tl:?}");
        let longe = MotionKey {
            z: -1.0,
            ..MotionKey::default()
        };
        let f = apply_motion_3d(&layer(32, 16), &longe);
        assert!(alpha_sum(&f) < 32.0 * 16.0 * 0.6, "longe fica menor");
    }

    #[test]
    fn girar_90_em_y_deixa_a_camada_de_perfil() {
        let m = MotionKey {
            rot_y: 90.0,
            ..MotionKey::default()
        };
        let f = apply_motion_3d(&layer(32, 16), &m);
        assert!(alpha_sum(&f) < 32.0, "de perfil quase não aparece");
        let m = MotionKey {
            rot_y: 180.0,
            ..MotionKey::default()
        };
        let f = apply_motion_3d(&layer(32, 16), &m);
        let row = 8 * 32;
        assert!(f.px[row + 2][2] > 0.9, "{:?}", f.px[row + 2]);
        assert!(f.px[row + 29][0] > 0.9, "{:?}", f.px[row + 29]);
    }

    #[test]
    fn pivo_orientacao_pai_filho_e_camera_estereoscopica_3d() {
        // Pivô permanece imóvel durante rotação
        let m = MotionKey {
            rot: 90.0,
            pivot_x: 0.25,
            pivot_y: 0.0,
            ..MotionKey::default()
        };
        let p = layer_to_world(&m, 100.0, 100.0, 25.0, 0.0);
        assert!((p[0] - 25.0).abs() < 1e-3 && p[1].abs() < 1e-3, "{p:?}");

        // Orientação separada gira independentemente de rot
        let mo = MotionKey {
            orient_y: 180.0,
            ..MotionKey::default()
        };
        assert!(!is_identity(&mo));
        let fo = apply_motion_3d(&layer(32, 16), &mo);
        assert!(fo.px[8 * 32 + 2][2] > 0.9);

        // Pai/Filho 3D: translação Z do pai soma com a do filho
        let parent = motion_to_mat4(
            &MotionKey {
                z: 0.2,
                ..MotionKey::default()
            },
            100.0,
            100.0,
        );
        let child = motion_to_mat4(
            &MotionKey {
                z: -0.05,
                ..MotionKey::default()
            },
            100.0,
            100.0,
        );
        let combined = mat4_mul(&parent, &child);
        assert!((world_center_z(&combined) - 15.0).abs() < 1e-3);

        // Câmera estereoscópica 3D: zero paralaxe na convergência; paralaxe oposta fora dela
        let at_conv = project_stereo([0.0, 0.0, 10.0], 100.0, 100.0, -5.0, 10.0).unwrap();
        let at_conv_r = project_stereo([0.0, 0.0, 10.0], 100.0, 100.0, 5.0, 10.0).unwrap();
        assert!((at_conv.0 - at_conv_r.0).abs() < 1e-3);
        let closer_l = project_stereo([0.0, 0.0, 30.0], 100.0, 100.0, -5.0, 0.0).unwrap();
        let closer_r = project_stereo([0.0, 0.0, 30.0], 100.0, 100.0, 5.0, 0.0).unwrap();
        assert!(
            closer_l.0 > closer_r.0,
            "objeto próximo deve ter disparidade cruzada"
        );
    }

    #[test]
    fn atras_da_camera_nao_projeta_e_valores_hostis_nao_travam() {
        assert!(project([0.0, 0.0, 1e9], 1920.0, 1080.0).is_none());
        assert!(project([0.0, 0.0, f32::NAN], 1920.0, 1080.0).is_none());
        let hostis = [
            MotionKey {
                z: 50.0,
                ..MotionKey::default()
            },
            MotionKey {
                x: f32::NAN,
                y: f32::INFINITY,
                z: f32::NEG_INFINITY,
                scale_x: 0.0,
                scale_y: -5.0,
                rot: f32::NAN,
                rot_x: f32::INFINITY,
                rot_y: f32::NAN,
                pivot_x: f32::NAN,
                pivot_y: f32::INFINITY,
                pivot_z: f32::NEG_INFINITY,
                orient_x: f32::NAN,
                orient_y: f32::INFINITY,
                orient_z: f32::NAN,
            },
        ];
        let src = layer(8, 8);
        for h in hostis {
            let out = apply_motion_3d(&src, &h);
            assert_eq!((out.w, out.h), (8, 8));
        }
    }
}
