//! Miniaturas da janela Efeitos de Vídeo (manual do VEGAS Pro 21, p. 54: as
//! miniaturas à direita representam os presets do efeito escolhido). Cada
//! uma é o efeito aplicado a um quadro de amostra desenhado aqui em código,
//! sem imagem embutida (AGENTS.md §1).

use super::{vfx, Frame};
use crate::model::{FrameRate, FxInstance, Ticks};

/// Tamanho das miniaturas (pequenas, para hardware modesto, §3.6).
pub const THUMB_W: usize = 96;
pub const THUMB_H: usize = 54;

/// Quadro de amostra: céu em gradiente, uma faixa de cores primárias, um
/// tom de pele e uma área verde (para o Chroma Key mostrar o recorte).
pub fn sample_frame(w: usize, h: usize) -> Frame {
    let mut f = Frame::new(w, h);
    let (wf, hf) = (w.max(1) as f32, h.max(1) as f32);
    const BARS: [[f32; 4]; 4] = [
        [0.90, 0.15, 0.15, 1.0],
        [0.15, 0.80, 0.20, 1.0],
        [0.20, 0.30, 0.90, 1.0],
        [0.95, 0.85, 0.20, 1.0],
    ];
    for y in 0..h {
        for x in 0..w {
            let u = x as f32 / wf;
            let v = y as f32 / hf;
            let px = if v < 0.45 {
                [0.25 + 0.30 * u, 0.45 + 0.25 * u, 0.85, 1.0]
            } else if v < 0.62 {
                let i = ((u * 4.0) as usize).min(BARS.len() - 1);
                BARS[i]
            } else if u < 0.45 {
                [0.87, 0.65, 0.52, 1.0]
            } else {
                [0.10, 0.75, 0.20, 1.0]
            };
            if let Some(p) = f.px.get_mut(y * w + x) {
                *p = px;
            }
        }
    }
    f
}

/// O efeito aplicado ao quadro de amostra, no instante zero.
pub fn render_preview(fx: &FxInstance, w: usize, h: usize) -> Frame {
    render_preview_at(fx, Ticks::ZERO, FrameRate::FPS_30, w, h)
}

/// O efeito aplicado ao quadro de amostra em movimento no tempo `t` (miniatura animada ao passar o mouse).
pub fn render_preview_at(fx: &FxInstance, t: Ticks, fps: FrameRate, w: usize, h: usize) -> Frame {
    let mut f = sample_frame(w, h);
    if w == 0 || h == 0 {
        return f;
    }
    if t > Ticks::ZERO {
        let secs = t.as_secs_f64();
        let shift = ((secs * (w as f64) * 0.35).round() as usize) % w.max(1);
        if shift > 0 {
            let orig = f.px.clone();
            for y in 0..h {
                for x in 0..w {
                    let sx = (x + shift) % w;
                    f.px[y * w + x] = orig[y * w + sx];
                }
            }
        }
    }
    vfx::apply(fx, &mut f, t, fps);
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FxCategory, FxKind};

    #[test]
    fn todo_efeito_de_video_gera_miniatura_do_tamanho_certo() {
        let base = sample_frame(THUMB_W, THUMB_H);
        for k in FxKind::VIDEO {
            let f = render_preview(&FxInstance::new(k), THUMB_W, THUMB_H);
            assert_eq!(
                (f.w, f.h, f.px.len()),
                (THUMB_W, THUMB_H, THUMB_W * THUMB_H),
                "{k:?}"
            );
            assert!(
                f.px.iter().all(|p| p.iter().all(|c| c.is_finite())),
                "{k:?} gerou NaN/infinito"
            );
            assert_eq!(f.to_rgba8().len(), THUMB_W * THUMB_H * 4, "{k:?}");
        }
        // o efeito aparece na miniatura
        let inv = render_preview(&FxInstance::new(FxKind::Invert), THUMB_W, THUMB_H);
        assert_ne!(inv.px, base.px);
    }

    #[test]
    fn categorias_cobrem_os_efeitos_de_video_sem_sobra() {
        for k in FxKind::VIDEO {
            assert!(k.category().is_some(), "{k:?} sem categoria");
        }
        for k in FxKind::AUDIO {
            assert!(k.category().is_none(), "{k:?} é de áudio");
        }
        for c in FxCategory::ALL {
            assert!(
                FxKind::VIDEO.iter().any(|k| k.category() == Some(c)),
                "categoria vazia: {}",
                c.name()
            );
        }
        let mut names: Vec<&str> = FxCategory::ALL.iter().map(|c| c.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), FxCategory::ALL.len(), "nomes repetidos");
    }

    #[test]
    fn quadro_de_tamanho_zero_nao_entra_em_panico() {
        let f = render_preview(&FxInstance::new(FxKind::GaussianBlur), 0, 0);
        assert!(f.px.is_empty());
    }

    #[test]
    fn miniatura_animada_no_tempo_varia_sem_panico() {
        let fx = FxInstance::new(FxKind::ColorCorrector);
        let a = render_preview_at(&fx, Ticks::ZERO, FrameRate::FPS_30, THUMB_W, THUMB_H);
        let b = render_preview_at(
            &fx,
            Ticks::from_secs_i64(1),
            FrameRate::FPS_30,
            THUMB_W,
            THUMB_H,
        );
        assert_eq!(a.px.len(), THUMB_W * THUMB_H);
        assert_eq!(b.px.len(), THUMB_W * THUMB_H);
        assert_ne!(a.px, b.px);
        assert!(b.px.iter().all(|p| p.iter().all(|c| c.is_finite())));
    }
}
