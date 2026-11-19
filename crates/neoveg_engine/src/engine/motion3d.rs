//! Movimento da trilha em 3D (*Alfa da origem 3D*, manual do VEGAS Pro 21,
//! pp. 748–756): a camada vira um plano no espaço, com posição X/Y/Z,
//! tamanho e rotação em torno de X, Y e Z, visto por uma câmera em
//! perspectiva.
//!
//! Convenções (escolhas nossas, o manual não fixa a câmera):
//! - origem no centro do quadro, X para a direita, Y para baixo e Z na
//!   direção de quem assiste (Z positivo = mais perto, a imagem cresce);
//! - câmera em `(0, 0, D)` olhando para −Z, com `D` = largura do quadro ×
//!   [`CAMERA_DIST`] (campo de visão horizontal de ~45°); no plano Z = 0 um
//!   pixel da camada é um pixel da tela, como no 2D;
//! - rotação aplicada na ordem X, depois Y, depois Z.
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

fn fin(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

/// `true` se o quadro-chave não move nada (pula o render).
pub fn is_identity(m: &MotionKey) -> bool {
    m.x == 0.0
        && m.y == 0.0
        && m.z == 0.0
        && m.scale_x == 1.0
        && m.scale_y == 1.0
        && m.rot == 0.0
        && m.rot_x == 0.0
        && m.rot_y == 0.0
}

/// Matriz de rotação X → Y → Z (graus), por linhas.
fn rotation(m: &MotionKey) -> [[f32; 3]; 3] {
    let (sx, cx) = fin(m.rot_x, 0.0).to_radians().sin_cos();
    let (sy, cy) = fin(m.rot_y, 0.0).to_radians().sin_cos();
    let (sz, cz) = fin(m.rot, 0.0).to_radians().sin_cos();
    // Rz * Ry * Rx
    [
        [cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx],
        [sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx],
        [-sy, cy * sx, cy * cx],
    ]
}

/// Ponto da camada (pixels a partir do centro dela) no espaço do mundo,
/// para um quadro `w`×`h`.
pub fn layer_to_world(m: &MotionKey, w: f32, h: f32, u: f32, v: f32) -> [f32; 3] {
    let r = rotation(m);
    let su = u * fin(m.scale_x, 1.0).clamp(0.001, 100.0);
    let sv = v * fin(m.scale_y, 1.0).clamp(0.001, 100.0);
    let t = [
        fin(m.x, 0.0) * w,
        fin(m.y, 0.0) * h,
        fin(m.z, 0.0) * w,
    ];
    [
        r[0][0] * su + r[0][1] * sv + t[0],
        r[1][0] * su + r[1][1] * sv + t[1],
        r[2][0] * su + r[2][1] * sv + t[2],
    ]
}

/// Projeta um ponto do mundo na tela (pixels a partir do canto superior
/// esquerdo). `None` se estiver atrás da câmera ou colado nela.
pub fn project(p: [f32; 3], w: f32, h: f32) -> Option<(f32, f32)> {
    let d = w.max(1.0) * CAMERA_DIST;
    let den = d - p[2];
    if !den.is_finite() || den < d * NEAR {
        return None;
    }
    let s = d / den;
    Some((w * 0.5 + p[0] * s, h * 0.5 + p[1] * s))
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

/// Homografia camada → tela (por linhas), em coordenadas homogêneas.
fn homography(m: &MotionKey, w: f32, h: f32) -> [[f32; 3]; 3] {
    let d = w.max(1.0) * CAMERA_DIST;
    let (cx, cy) = (w * 0.5, h * 0.5);
    let r = rotation(m);
    let sxs = fin(m.scale_x, 1.0).clamp(0.001, 100.0);
    let sys = fin(m.scale_y, 1.0).clamp(0.001, 100.0);
    // colunas de mundo: u → (r·0)*sx, v → (r·1)*sy, 1 → t
    let cu = [r[0][0] * sxs, r[1][0] * sxs, r[2][0] * sxs];
    let cv = [r[0][1] * sys, r[1][1] * sys, r[2][1] * sys];
    let ct = [fin(m.x, 0.0) * w, fin(m.y, 0.0) * h, fin(m.z, 0.0) * w];
    // num_x = D·X + cx·(D − Z), num_y = D·Y + cy·(D − Z), den = D − Z
    let row = |a: f32, b: f32, c: f32| -> [f32; 3] { [a, b, c] };
    let col = |k: [f32; 3], is_t: bool| -> [f32; 3] {
        let dz = if is_t { d - k[2] } else { -k[2] };
        [d * k[0] + cx * dz, d * k[1] + cy * dz, dz]
    };
    let (a, b, c) = (col(cu, false), col(cv, false), col(ct, true));
    [
        row(a[0], b[0], c[0]),
        row(a[1], b[1], c[1]),
        row(a[2], b[2], c[2]),
    ]
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

/// Aplica o movimento 3D a uma camada do tamanho do quadro.
pub fn apply_motion_3d(f: &Frame, m: &MotionKey) -> Frame {
    let (w, h) = (f.w, f.h);
    let mut out = Frame::new(w, h);
    let (wf, hf) = (w as f32, h as f32);
    let Some(inv) = invert3(homography(m, wf, hf)) else {
        return out;
    };
    out.px.par_chunks_mut(w.max(1)).enumerate().for_each(|(y, row)| {
        let py = y as f32 + 0.5;
        for (x, p) in row.iter_mut().enumerate() {
            let px = x as f32 + 0.5;
            let a = inv[0][0] * px + inv[0][1] * py + inv[0][2];
            let b = inv[1][0] * px + inv[1][1] * py + inv[1][2];
            let c = inv[2][0] * px + inv[2][1] * py + inv[2][2];
            // c ≤ 0: o raio passa atrás da câmera para este pixel
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
        // 180° em Y espelha: a metade vermelha passa para a direita
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
                rot_x: f32::INFINITY,
                scale_y: 0.0,
                ..MotionKey::default()
            },
        ];
        for m in hostis {
            let f = apply_motion_3d(&layer(8, 4), &m);
            assert_eq!(f.px.len(), 32);
            assert!(f.px.iter().all(|p| p.iter().all(|v| v.is_finite())));
        }
        let vazio = apply_motion_3d(&Frame::new(0, 0), &MotionKey::default());
        assert!(vazio.px.is_empty());
    }

    #[test]
    fn projeto_antigo_sem_campos_3d_abre_igual() {
        let k: MotionKey = serde_json::from_str(
            r#"{"x":0.1,"y":0.0,"scale_x":1.0,"scale_y":1.0,"rot":5.0}"#,
        )
        .unwrap();
        assert_eq!((k.z, k.rot_x, k.rot_y), (0.0, 0.0, 0.0));
        let mut v = serde_json::to_value(crate::model::Track::new(
            crate::model::TrackKind::Video,
            "V",
        ))
        .unwrap();
        v.as_object_mut().unwrap().remove("motion_3d");
        let t: crate::model::Track = serde_json::from_value(v).unwrap();
        assert!(!t.motion_3d);
    }
}
