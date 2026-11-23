//! Geometria do retângulo de enquadramento do Pan/Crop (manual do VEGAS Pro
//! 21, pp. 250–251): alças, mover, redimensionar com ou sem *Travar
//! proporção* e *Dimensionar em torno do centro*, girar e os comandos do
//! menu de contexto da área de trabalho.
//!
//! Tudo em pixels do quadro do projeto (origem no canto superior esquerdo,
//! y para baixo). Sem estado de interface: só contas, para testar sem janela.

use eframe::egui::{vec2, Vec2};
use neoveg_engine::model::PanCropKey;

/// Menor meia-largura/meia-altura aceita, em pixels do quadro.
const MIN_HALF: f32 = 1.0;
/// Maior tamanho relativo ao quadro (o mesmo limite da janela antiga).
const MAX_SIZE: f32 = 10.0;

/// Sinais das 8 alças no espaço do retângulo, a partir do canto superior
/// esquerdo, no sentido horário.
pub(crate) const HANDLE_SIGNS: [(f32, f32); 8] = [
    (-1.0, -1.0),
    (0.0, -1.0),
    (1.0, -1.0),
    (1.0, 0.0),
    (1.0, 1.0),
    (0.0, 1.0),
    (-1.0, 1.0),
    (-1.0, 0.0),
];

/// Modo de movimento da barra de ferramentas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum MoveMode {
    #[default]
    Free,
    XOnly,
    YOnly,
}

/// O que está sob o ponteiro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hit {
    Handle(usize),
    Inside,
    /// Fora do retângulo: arrastar gira.
    Rotate,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BoxGeom {
    pub center: Vec2,
    pub half: Vec2,
    pub rot_deg: f32,
}

fn finite_or(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

impl BoxGeom {
    /// Retângulo de um quadro-chave num quadro `frame` (largura, altura).
    pub(crate) fn from_key(k: &PanCropKey, frame: Vec2) -> Self {
        let size = finite_or(k.size, 1.0).clamp(0.001, MAX_SIZE);
        BoxGeom {
            center: vec2(
                finite_or(k.cx, 0.5) * frame.x,
                finite_or(k.cy, 0.5) * frame.y,
            ),
            half: vec2(frame.x * size * 0.5, frame.y * size * k.ratio_safe() * 0.5),
            rot_deg: finite_or(k.rot, 0.0),
        }
    }

    /// Grava o retângulo em `base`, mantendo o espelhamento dele.
    pub(crate) fn to_key(self, base: &PanCropKey, frame: Vec2) -> PanCropKey {
        let fw = frame.x.max(1.0);
        let fh = frame.y.max(1.0);
        let size = (self.half.x * 2.0 / fw).clamp(0.001, MAX_SIZE);
        let ratio = (self.half.y * 2.0 / (fh * size)).clamp(0.01, 100.0);
        PanCropKey {
            cx: finite_or(self.center.x / fw, 0.5),
            cy: finite_or(self.center.y / fh, 0.5),
            size: finite_or(size, 1.0),
            rot: finite_or(self.rot_deg, 0.0),
            ratio: finite_or(ratio, 1.0),
            ..base.clone()
        }
    }

    fn sin_cos(&self) -> (f32, f32) {
        self.rot_deg.to_radians().sin_cos()
    }

    /// Do espaço do retângulo (centro na origem, sem rotação) para o quadro.
    pub(crate) fn to_frame(self, l: Vec2) -> Vec2 {
        let (sn, cs) = self.sin_cos();
        self.center + vec2(l.x * cs - l.y * sn, l.x * sn + l.y * cs)
    }

    /// Do quadro para o espaço do retângulo.
    pub(crate) fn to_local(self, p: Vec2) -> Vec2 {
        let (sn, cs) = self.sin_cos();
        let d = p - self.center;
        vec2(d.x * cs + d.y * sn, -d.x * sn + d.y * cs)
    }

    pub(crate) fn handle(&self, i: usize) -> Vec2 {
        let (sx, sy) = HANDLE_SIGNS.get(i).copied().unwrap_or((0.0, 0.0));
        self.to_frame(vec2(sx * self.half.x, sy * self.half.y))
    }

    pub(crate) fn corners(&self) -> [Vec2; 4] {
        [
            self.handle(0),
            self.handle(2),
            self.handle(4),
            self.handle(6),
        ]
    }

    /// Raio do círculo de rotação (meia diagonal).
    pub(crate) fn rotate_radius(&self) -> f32 {
        self.half.length()
    }

    /// `tol` é a distância de toque das alças, em pixels do quadro.
    /// Prioriza os 4 cantos (largura+altura), depois as 4 alças laterais e, em seguida,
    /// as próprias bordas do retângulo para facilitar o redimensionamento.
    pub(crate) fn hit(&self, p: Vec2, tol: f32) -> Hit {
        if !p.x.is_finite() || !p.y.is_finite() {
            return Hit::Rotate;
        }
        let tol = finite_or(tol, 0.0).max(0.0);
        let closest_in = |indices: &[usize]| -> Option<usize> {
            indices
                .iter()
                .copied()
                .filter_map(|i| {
                    let d = (self.handle(i) - p).length();
                    (d <= tol).then_some((i, d))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        };
        if let Some(i) = closest_in(&[0, 2, 4, 6]) {
            return Hit::Handle(i);
        }
        if let Some(i) = closest_in(&[1, 3, 5, 7]) {
            return Hit::Handle(i);
        }
        let l = self.to_local(p);
        let edge_tol = (tol * 0.65).min(self.half.x.min(self.half.y) * 0.45);
        if edge_tol > 0.0 {
            let near_x = l.x.abs() <= self.half.x + edge_tol;
            let near_y = l.y.abs() <= self.half.y + edge_tol;
            if near_x && (l.y + self.half.y).abs() <= edge_tol {
                return Hit::Handle(1);
            }
            if near_x && (l.y - self.half.y).abs() <= edge_tol {
                return Hit::Handle(5);
            }
            if near_y && (l.x - self.half.x).abs() <= edge_tol {
                return Hit::Handle(3);
            }
            if near_y && (l.x + self.half.x).abs() <= edge_tol {
                return Hit::Handle(7);
            }
        }
        if l.x.abs() <= self.half.x && l.y.abs() <= self.half.y {
            Hit::Inside
        } else {
            Hit::Rotate
        }
    }

    /// Arrasta a alça `i` até `p`.
    pub(crate) fn resize(&self, i: usize, p: Vec2, lock_aspect: bool, about_center: bool) -> Self {
        let Some(&(sx, sy)) = HANDLE_SIGNS.get(i) else {
            return *self;
        };
        let l = self.to_local(p);
        if !l.x.is_finite() || !l.y.is_finite() {
            return *self;
        }
        // lado oposto ancorado (sem "em torno do centro")
        let anchor = vec2(-sx * self.half.x, -sy * self.half.y);
        let want = |s: f32, lv: f32, av: f32, cur: f32| -> f32 {
            if s == 0.0 {
                cur
            } else if about_center {
                (lv * s).max(MIN_HALF)
            } else {
                ((lv - av) * s * 0.5).max(MIN_HALF)
            }
        };
        let mut half = vec2(
            want(sx, l.x, anchor.x, self.half.x),
            want(sy, l.y, anchor.y, self.half.y),
        );
        if lock_aspect && self.half.x > 0.0 && self.half.y > 0.0 {
            let kx = half.x / self.half.x;
            let ky = half.y / self.half.y;
            let k = match (sx != 0.0, sy != 0.0) {
                (true, true) => kx.max(ky),
                (true, false) => kx,
                _ => ky,
            };
            half = self.half * k;
        }
        let half = vec2(half.x.max(MIN_HALF), half.y.max(MIN_HALF));
        let center_local = if about_center {
            Vec2::ZERO
        } else {
            vec2(
                if sx == 0.0 {
                    0.0
                } else {
                    anchor.x + sx * half.x
                },
                if sy == 0.0 {
                    0.0
                } else {
                    anchor.y + sy * half.y
                },
            )
        };
        BoxGeom {
            center: self.to_frame(center_local),
            half,
            rot_deg: self.rot_deg,
        }
    }

    /// Gira pelo ângulo entre `from` e `to` em torno do centro.
    pub(crate) fn rotate(&self, from: Vec2, to: Vec2) -> Self {
        let a = from - self.center;
        let b = to - self.center;
        if a.length() < 1e-3 || b.length() < 1e-3 {
            return *self;
        }
        let delta = (b.y.atan2(b.x) - a.y.atan2(a.x)).to_degrees();
        let raw = self.rot_deg + delta;
        let rot = if raw.is_finite() {
            let wrapped = (raw + 180.0).rem_euclid(360.0) - 180.0;
            if wrapped <= -180.0 && raw > 0.0 {
                180.0
            } else {
                wrapped
            }
        } else {
            self.rot_deg
        };
        BoxGeom {
            rot_deg: finite_or(rot, self.rot_deg),
            ..*self
        }
    }

    /// Desloca o centro, com o modo de movimento e o encaixe na grade.
    pub(crate) fn moved(&self, delta: Vec2, mode: MoveMode, snap_grid: Option<f32>) -> Self {
        let d = match mode {
            MoveMode::Free => delta,
            MoveMode::XOnly => vec2(delta.x, 0.0),
            MoveMode::YOnly => vec2(0.0, delta.y),
        };
        let mut c = self.center + vec2(finite_or(d.x, 0.0), finite_or(d.y, 0.0));
        if let Some(g) = snap_grid.filter(|g| g.is_finite() && *g >= 1.0) {
            if mode != MoveMode::YOnly {
                c.x = (c.x / g).round() * g;
            }
            if mode != MoveMode::XOnly {
                c.y = (c.y / g).round() * g;
            }
        }
        BoxGeom { center: c, ..*self }
    }
}

/// "Combinar proporção de saída": o retângulo passa a ter a proporção do
/// quadro, mantendo a largura.
pub(crate) fn match_output_aspect(k: &PanCropKey) -> PanCropKey {
    PanCropKey {
        ratio: 1.0,
        ..k.clone()
    }
}

/// "Combinar proporção da origem": o retângulo passa a ter a proporção da
/// mídia (`src` = largura, altura), mantendo a largura.
pub(crate) fn match_source_aspect(k: &PanCropKey, frame: Vec2, src: Vec2) -> PanCropKey {
    if src.x <= 0.0 || src.y <= 0.0 || frame.y <= 0.0 {
        return k.clone();
    }
    let ratio = (frame.x * src.y) / (frame.y * src.x);
    PanCropKey {
        ratio: if ratio.is_finite() {
            ratio.clamp(0.01, 100.0)
        } else {
            1.0
        },
        ..k.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const F: Vec2 = Vec2::new(1920.0, 1080.0);

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 0.01
    }

    #[test]
    fn padrao_cobre_o_quadro_inteiro_e_volta_igual() {
        let k = PanCropKey::default();
        let g = BoxGeom::from_key(&k, F);
        assert!(close(g.center, vec2(960.0, 540.0)));
        assert!(close(g.half, vec2(960.0, 540.0)));
        assert!(close(g.handle(0), Vec2::ZERO));
        assert!(close(g.handle(4), F));
        assert_eq!(g.to_key(&k, F), k);
    }

    #[test]
    fn alca_de_canto_com_centro_fixo_e_proporcao_travada() {
        let g = BoxGeom::from_key(&PanCropKey::default(), F);
        let r = g.resize(4, vec2(960.0 + 480.0, 540.0 + 100.0), true, true);
        assert!(close(r.center, g.center));
        // o maior dos dois fatores manda: 480/960 = 0,5 contra 100/540
        assert!(close(r.half, vec2(480.0, 270.0)), "{:?}", r.half);
    }

    #[test]
    fn alca_de_canto_sem_centro_fixo_ancora_o_canto_oposto() {
        let g = BoxGeom::from_key(&PanCropKey::default(), F);
        let r = g.resize(4, vec2(960.0, 540.0), false, false);
        assert!(close(r.handle(0), Vec2::ZERO), "{:?}", r.handle(0));
        assert!(close(r.handle(4), vec2(960.0, 540.0)));
    }

    #[test]
    fn alca_lateral_sem_travar_muda_so_a_largura() {
        let g = BoxGeom::from_key(&PanCropKey::default(), F);
        let r = g.resize(3, vec2(1500.0, 300.0), false, true);
        assert!(close(r.half, vec2(540.0, 540.0)), "{:?}", r.half);
        let k = r.to_key(&PanCropKey::default(), F);
        // largura 1080 com altura 1080: proporção quadrada
        assert!((k.size - 1080.0 / 1920.0).abs() < 1e-4);
        assert!((k.ratio - 1920.0 / 1080.0).abs() < 1e-3);
    }

    #[test]
    fn redimensionar_girado_segue_os_eixos_do_retangulo() {
        let mut g = BoxGeom::from_key(&PanCropKey::default(), F);
        g.rot_deg = 90.0;
        // girado 90°, a alça da direita (3) fica embaixo do centro
        assert!(close(g.handle(3), vec2(960.0, 540.0 + 960.0)));
        let r = g.resize(3, vec2(960.0, 540.0 + 480.0), false, true);
        assert!(close(r.half, vec2(480.0, 540.0)), "{:?}", r.half);
    }

    #[test]
    fn toque_nas_alcas_dentro_e_fora() {
        let g = BoxGeom::from_key(&PanCropKey::default(), F);
        assert_eq!(g.hit(vec2(2.0, 1.0), 6.0), Hit::Handle(0));
        assert_eq!(g.hit(vec2(960.0, 1078.0), 6.0), Hit::Handle(5));
        assert_eq!(g.hit(vec2(500.0, 500.0), 6.0), Hit::Inside);
        assert_eq!(g.hit(vec2(-200.0, 500.0), 6.0), Hit::Rotate);
        assert_eq!(g.hit(vec2(f32::NAN, 0.0), f32::NAN), Hit::Rotate);
        // tocar diretamente em uma borda longe da alça central também permite redimensionar
        assert_eq!(g.hit(vec2(1919.0, 200.0), 12.0), Hit::Handle(3));
        assert_eq!(g.hit(vec2(300.0, 2.0), 12.0), Hit::Handle(1));
    }

    #[test]
    fn girar_e_mover_com_restricoes() {
        let g = BoxGeom::from_key(&PanCropKey::default(), F);
        let r = g.rotate(vec2(1960.0, 540.0), vec2(960.0, 1540.0));
        assert!((r.rot_deg - 90.0).abs() < 1e-3, "{}", r.rot_deg);
        let r = g.rotate(g.center, vec2(0.0, 0.0));
        assert_eq!(r, g, "a partir do centro não gira");
        let m = g.moved(vec2(13.0, 27.0), MoveMode::XOnly, None);
        assert!(close(m.center, vec2(973.0, 540.0)));
        let m = g.moved(vec2(13.0, 27.0), MoveMode::Free, Some(16.0));
        assert!(close(m.center, vec2(976.0, 560.0)), "{:?}", m.center);
        let m = g.moved(
            vec2(f32::NAN, f32::INFINITY),
            MoveMode::Free,
            Some(f32::NAN),
        );
        assert!(m.center.x.is_finite() && m.center.y.is_finite());
    }

    #[test]
    fn combinar_proporcoes_e_valores_hostis() {
        let k = PanCropKey {
            ratio: 3.0,
            ..PanCropKey::default()
        };
        assert_eq!(match_output_aspect(&k).ratio, 1.0);
        // mídia 4:3 num quadro 16:9: o retângulo fica mais alto que o quadro
        let s = match_source_aspect(&k, F, vec2(640.0, 480.0));
        assert!((s.ratio - (1920.0 * 480.0) / (1080.0 * 640.0)).abs() < 1e-4);
        assert_eq!(match_source_aspect(&k, F, vec2(0.0, 480.0)), k);
        let hostil = PanCropKey {
            cx: f32::NAN,
            size: f32::INFINITY,
            rot: f32::NAN,
            ..PanCropKey::default()
        };
        let g = BoxGeom::from_key(&hostil, F);
        assert!(g.center.x.is_finite() && g.half.x.is_finite() && g.rot_deg.is_finite());
        let back = g.to_key(&hostil, F);
        assert!(back.cx.is_finite() && back.size.is_finite());

        // Ângulo extremo ou não finito não pode travar o laço de normalização (§0.5)
        for bad_rot in [
            1e30_f32,
            -1e30_f32,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
        ] {
            let mut g_bad = BoxGeom::from_key(&PanCropKey::default(), F);
            g_bad.rot_deg = bad_rot;
            let r = g_bad.rotate(vec2(1960.0, 540.0), vec2(960.0, 1540.0));
            if bad_rot.is_finite() {
                assert!(r.rot_deg.is_finite() && (-180.0..=180.0).contains(&r.rot_deg));
            }
        }
    }
}
