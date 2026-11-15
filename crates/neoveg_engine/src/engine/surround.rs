//! Surround Panner 5.1: ganhos por canal a partir de uma posição 2D na sala.
//!
//! Disposição dos alto-falantes segundo a recomendação pública ITU-R BS.775
//! (C a 0°, L/R a ±30°, Ls/Rs a ±110°). O panning entre o par vizinho usa lei
//! de potência constante (seno/cosseno); perto do centro da sala o som se
//! espalha igualmente por todos os alto-falantes do anel. A energia total é
//! normalizada para 1, e o LFE recebe um nível independente.
//!
//! Ordem dos canais (WAVE_FORMAT_EXTENSIBLE, máscara 0x3F):
//! FL, FR, C, LFE, BL, BR.

use crate::model::SurroundPan;

pub const CHANNELS: usize = 6;
pub const FL: usize = 0;
pub const FR: usize = 1;
pub const C: usize = 2;
pub const LFE: usize = 3;
pub const SL: usize = 4;
pub const SR: usize = 5;

/// Anel de alto-falantes em graus (0 = frente, positivo à direita), ordenado.
const RING_WITH_C: [(usize, f32); 5] =
    [(SL, -110.0), (FL, -30.0), (C, 0.0), (FR, 30.0), (SR, 110.0)];
const RING_NO_C: [(usize, f32); 4] = [(SL, -110.0), (FL, -30.0), (FR, 30.0), (SR, 110.0)];

/// Coeficiente de downmix (−3 dB) para centro e surrounds (ITU-R BS.775).
const DOWNMIX_K: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// Ganhos dos 6 canais para a posição `pan` (entradas fora da faixa são saneadas).
pub fn gains(pan: SurroundPan) -> [f32; CHANNELS] {
    let p = pan.sanitized();
    let ring: &[(usize, f32)] = if p.use_center {
        &RING_WITH_C
    } else {
        &RING_NO_C
    };
    let r = (p.x * p.x + p.y * p.y).sqrt().min(1.0);
    let mut g = [0f32; CHANNELS];
    if r > 1e-6 {
        // atan2(x, -y): 0° na frente (y = -1), 180° atrás.
        let theta = p.x.atan2(-p.y).to_degrees();
        pair_gains(ring, theta, &mut g);
    }
    let spread = 1.0 / (ring.len() as f32).sqrt();
    for &(ch, _) in ring {
        g[ch] = r * g[ch] + (1.0 - r) * spread;
    }
    let energy: f32 = ring.iter().map(|&(ch, _)| g[ch] * g[ch]).sum();
    if energy > 1e-12 {
        let k = 1.0 / energy.sqrt();
        for &(ch, _) in ring {
            g[ch] *= k;
        }
    }
    g[LFE] = p.lfe;
    g
}

/// Panning de potência constante entre o par de alto-falantes que contém `theta`.
fn pair_gains(ring: &[(usize, f32)], theta: f32, g: &mut [f32; CHANNELS]) {
    let n = ring.len();
    for i in 0..n {
        let (Some(&(a_ch, a)), Some(&(b_ch, b0))) = (ring.get(i), ring.get((i + 1) % n)) else {
            return;
        };
        // o último par fecha o anel por trás (Rs → Ls)
        let b = if i + 1 == n { b0 + 360.0 } else { b0 };
        let th = if theta < a { theta + 360.0 } else { theta };
        if th >= a && th <= b {
            let span = (b - a).max(1e-3);
            let f = ((th - a) / span).clamp(0.0, 1.0);
            let ang = f * std::f32::consts::FRAC_PI_2;
            g[a_ch] = ang.cos();
            g[b_ch] = ang.sin();
            return;
        }
    }
}

/// Soma `src` (estéreo intercalado, `n` quadros) em `dst` (6 canais intercalados)
/// como fonte mono posicionada com os ganhos `g`.
pub fn mix_into(dst: &mut [f32], src: &[f32], n: usize, g: &[f32; CHANNELS]) {
    for i in 0..n {
        let (Some(&l), Some(&r)) = (src.get(i * 2), src.get(i * 2 + 1)) else {
            return;
        };
        let m = 0.5 * (l + r);
        let Some(frame) = dst.get_mut(i * CHANNELS..(i + 1) * CHANNELS) else {
            return;
        };
        for (s, k) in frame.iter_mut().zip(g.iter()) {
            *s += m * k;
        }
    }
}

/// Downmix ITU-R BS.775 de um quadro 5.1 para estéreo (LFE descartado).
pub fn downmix_frame(f: &[f32]) -> (f32, f32) {
    let at = |i: usize| f.get(i).copied().unwrap_or(0.0);
    let l = at(FL) + DOWNMIX_K * at(C) + DOWNMIX_K * at(SL);
    let r = at(FR) + DOWNMIX_K * at(C) + DOWNMIX_K * at(SR);
    (l, r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pan(x: f32, y: f32) -> SurroundPan {
        SurroundPan {
            x,
            y,
            ..SurroundPan::default()
        }
    }

    fn energy(g: &[f32; CHANNELS]) -> f32 {
        g.iter()
            .enumerate()
            .filter(|(i, _)| *i != LFE)
            .map(|(_, v)| v * v)
            .sum()
    }

    #[test]
    fn front_center_goes_to_center_speaker() {
        let g = gains(pan(0.0, -1.0));
        assert!((g[C] - 1.0).abs() < 1e-4, "{g:?}");
        assert!(g[FL].abs() < 1e-4 && g[FR].abs() < 1e-4);
    }

    #[test]
    fn front_center_without_c_splits_front_pair() {
        let mut p = pan(0.0, -1.0);
        p.use_center = false;
        let g = gains(p);
        assert!(g[C].abs() < 1e-6);
        assert!((g[FL] - g[FR]).abs() < 1e-4);
        assert!(g[FL] > 0.6);
    }

    #[test]
    fn hard_rear_left_goes_to_left_surround() {
        // -110° = posição do Ls
        let a = (-110.0f32).to_radians();
        let g = gains(pan(a.sin(), -a.cos()));
        assert!((g[SL] - 1.0).abs() < 1e-3, "{g:?}");
    }

    #[test]
    fn rear_center_splits_surround_pair() {
        let g = gains(pan(0.0, 1.0));
        assert!((g[SL] - g[SR]).abs() < 1e-4, "{g:?}");
        assert!(g[SL] > 0.6);
    }

    #[test]
    fn room_center_spreads_equally() {
        let g = gains(pan(0.0, 0.0));
        let v = g[FL];
        for ch in [FR, C, SL, SR] {
            assert!((g[ch] - v).abs() < 1e-5);
        }
    }

    #[test]
    fn energy_is_normalized_everywhere() {
        for xi in -4..=4 {
            for yi in -4..=4 {
                let g = gains(pan(xi as f32 / 4.0, yi as f32 / 4.0));
                assert!((energy(&g) - 1.0).abs() < 1e-3, "x={xi} y={yi} {g:?}");
            }
        }
    }

    #[test]
    fn hostile_values_do_not_produce_nan() {
        for (x, y) in [
            (f32::NAN, 0.0),
            (f32::INFINITY, f32::NEG_INFINITY),
            (1e30, -1e30),
        ] {
            let mut p = pan(x, y);
            p.lfe = f32::NAN;
            let g = gains(p);
            assert!(g.iter().all(|v| v.is_finite()), "{g:?}");
        }
    }

    #[test]
    fn lfe_is_independent_level() {
        let mut p = pan(0.0, -1.0);
        p.lfe = 0.5;
        assert!((gains(p)[LFE] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn downmix_keeps_front_and_attenuates_center() {
        let (l, r) = downmix_frame(&[1.0, 0.0, 1.0, 1.0, 0.0, 0.0]);
        assert!((l - (1.0 + DOWNMIX_K)).abs() < 1e-6);
        assert!((r - DOWNMIX_K).abs() < 1e-6);
        // quadro curto não entra em pânico
        assert_eq!(downmix_frame(&[]), (0.0, 0.0));
    }

    #[test]
    fn mix_into_tolerates_short_buffers() {
        let mut dst = vec![0f32; 6];
        mix_into(&mut dst, &[1.0, 1.0, 1.0, 1.0], 4, &gains(pan(0.0, -1.0)));
        assert!((dst[C] - 1.0).abs() < 1e-4);
    }
}
