//! Miniaturas da janela Efeitos de Vídeo (manual do VEGAS Pro 21, p. 54: as
//! miniaturas à direita representam os presets do efeito escolhido). Cada
//! uma é o efeito aplicado a um quadro de amostra desenhado aqui em código,
//! sem imagem embutida (AGENTS.md §1).

use super::{transition, vfx, Frame};
use crate::model::{FrameRate, FxInstance, OfxFxRef, Ticks, TransitionKind};

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

/// Segundo quadro de amostra (o que entra nas transições): pôr do sol com
/// um disco claro, bem diferente do primeiro para a transição aparecer.
pub fn sample_frame_b(w: usize, h: usize) -> Frame {
    let mut f = Frame::new(w, h);
    let (wf, hf) = (w.max(1) as f32, h.max(1) as f32);
    for y in 0..h {
        for x in 0..w {
            let u = x as f32 / wf;
            let v = y as f32 / hf;
            let (dx, dy) = (u - 0.65, (v - 0.4) * hf / wf);
            let px = if dx * dx + dy * dy < 0.018 {
                [0.98, 0.92, 0.70, 1.0]
            } else if v < 0.7 {
                [0.85 - 0.25 * v, 0.40 - 0.20 * v, 0.30 + 0.25 * v, 1.0]
            } else {
                [0.12, 0.10, 0.22, 1.0]
            };
            if let Some(p) = f.px.get_mut(y * w + x) {
                *p = px;
            }
        }
    }
    f
}

/// Miniatura de uma transição (janela Transições): o quadro de amostra
/// saindo e o segundo entrando, no progresso `p` (0..1).
pub fn render_transition_preview(kind: TransitionKind, p: f32, w: usize, h: usize) -> Frame {
    let a = sample_frame(w, h);
    if w == 0 || h == 0 {
        return a;
    }
    let b = sample_frame_b(w, h);
    let p = if p.is_finite() {
        p.clamp(0.0, 1.0)
    } else {
        0.5
    };
    transition::apply(kind, 0.1, &a, &b, p)
}

/// Miniatura de um efeito OFX de terceiros, renderizada no host isolado de
/// plug-ins com os parâmetros padrão. Erro do plug-in ou sem host volta como
/// erro (a janela mostra "sem prévia", nunca a amostra sem efeito).
pub fn render_ofx_preview(r: &OfxFxRef, w: usize, h: usize) -> Result<Frame, String> {
    let mut f = sample_frame(w, h);
    let (Ok(wu), Ok(hu)) = (u32::try_from(w), u32::try_from(h)) else {
        return Err("tamanho de miniatura inválido".into());
    };
    if w == 0 || h == 0 {
        return Err("tamanho de miniatura inválido".into());
    }
    let host = crate::plugin_host::default_host_exe()
        .ok_or_else(|| "host de plug-ins não encontrado".to_string())?;
    let mut px: Vec<f32> = f.px.iter().flatten().copied().collect();
    crate::plugin_host::ofx_render(&host, r, wu, hu, 0.0, 30.0, &mut px)?;
    for (dst, src) in f.px.iter_mut().zip(px.as_chunks::<4>().0) {
        *dst = *src;
    }
    if !f.px.iter().all(|p| p.iter().all(|c| c.is_finite())) {
        return Err("o plug-in devolveu valores inválidos".into());
    }
    Ok(f)
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
    fn toda_transicao_gera_miniatura_e_o_progresso_vai_de_a_para_b() {
        let a = sample_frame(THUMB_W, THUMB_H);
        let b = sample_frame_b(THUMB_W, THUMB_H);
        assert_ne!(a.px, b.px);
        for k in TransitionKind::ALL {
            let f = render_transition_preview(k, 0.5, THUMB_W, THUMB_H);
            assert_eq!(
                (f.w, f.h, f.px.len()),
                (THUMB_W, THUMB_H, THUMB_W * THUMB_H),
                "{k:?}"
            );
            assert!(
                f.px.iter().all(|p| p.iter().all(|c| c.is_finite())),
                "{k:?} gerou NaN/infinito"
            );
        }
        let ini = render_transition_preview(TransitionKind::default(), 0.0, THUMB_W, THUMB_H);
        let fim = render_transition_preview(TransitionKind::default(), 1.0, THUMB_W, THUMB_H);
        assert_ne!(
            ini.px, fim.px,
            "a transição muda a imagem ao longo do progresso"
        );
        // progresso hostil e tamanho zero
        let _ = render_transition_preview(TransitionKind::default(), f32::NAN, THUMB_W, THUMB_H);
        let _ = render_transition_preview(TransitionKind::default(), 9.0, THUMB_W, THUMB_H);
        assert!(
            render_transition_preview(TransitionKind::default(), 0.5, 0, 0)
                .px
                .is_empty()
        );
    }

    #[test]
    fn previa_ofx_sem_plugin_valido_e_erro_e_nao_trava() {
        let r = OfxFxRef {
            binary: "nao/existe.ofx".into(),
            plugin_id: "x".into(),
            label: String::new(),
            params: Vec::new(),
        };
        assert!(render_ofx_preview(&r, THUMB_W, THUMB_H).is_err());
        assert!(render_ofx_preview(&r, 0, 0).is_err());
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
