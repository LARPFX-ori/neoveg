//! Detecção automática de cenas e cortes em eventos de vídeo (Sala Limpa).
//!
//! Compara quadros sucessivos usando diferença média absoluta de luminância
//! BT.601 e distância de histograma L1 normalizado (16 faixas), aplicando
//! duração mínima entre cortes (§0.4).

use crate::media::MediaCache;
use crate::model::{MediaItem, Ticks};
use image::RgbaImage;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Limite máximo de quadros amostrados por análise para evitar laços ilimitados (§0.4).
pub const MAX_SCENE_STEPS: usize = 20_000;

/// Parâmetros da detecção de cortes.
#[derive(Clone, Copy, Debug)]
pub struct SceneDetectConfig {
    /// Limiar normalizado `[0.05..=0.95]` para acusar transição de cena.
    pub threshold: f32,
    /// Intervalo mínimo entre dois cortes consecutivos.
    pub min_scene_ticks: Ticks,
    /// Largura da grade de análise.
    pub sample_w: u32,
    /// Altura da grade de análise.
    pub sample_h: u32,
}

impl Default for SceneDetectConfig {
    fn default() -> Self {
        Self {
            threshold: 0.35,
            min_scene_ticks: Ticks::from_millis_i64(250),
            sample_w: 64,
            sample_h: 36,
        }
    }
}

impl SceneDetectConfig {
    pub fn sanitized(self) -> Self {
        let thr = if self.threshold.is_finite() {
            self.threshold.clamp(0.05, 0.95)
        } else {
            0.35
        };
        Self {
            threshold: thr,
            min_scene_ticks: self.min_scene_ticks.max(Ticks::from_millis_i64(1)),
            sample_w: self.sample_w.clamp(8, 256),
            sample_h: self.sample_h.clamp(8, 256),
        }
    }
}

/// Mede a dissimilaridade visual `[0.0..=1.0]` entre dois quadros RGBA.
/// Retorna `0.0` se qualquer quadro estiver vazio ou com dimensões incompatíveis.
pub fn scene_change_score(prev: &RgbaImage, next: &RgbaImage) -> f32 {
    let (w, h) = (prev.width(), prev.height());
    if w == 0 || h == 0 || w != next.width() || h != next.height() {
        return 0.0;
    }
    let pa = prev.as_raw();
    let pb = next.as_raw();
    let total = (w as usize).saturating_mul(h as usize);
    if total == 0 || pa.len() < total * 4 || pb.len() < total * 4 {
        return 0.0;
    }
    let mut hist_a = [0u32; 16];
    let mut hist_b = [0u32; 16];
    let mut sad = 0.0_f64;
    for i in 0..total {
        let o = i * 4;
        let la = (0.299 * f64::from(pa[o])
            + 0.587 * f64::from(pa[o + 1])
            + 0.114 * f64::from(pa[o + 2]))
            / 255.0;
        let lb = (0.299 * f64::from(pb[o])
            + 0.587 * f64::from(pb[o + 1])
            + 0.114 * f64::from(pb[o + 2]))
            / 255.0;
        sad += (la - lb).abs();
        let ba = ((la * 15.999) as usize).min(15);
        let bb = ((lb * 15.999) as usize).min(15);
        hist_a[ba] = hist_a[ba].saturating_add(1);
        hist_b[bb] = hist_b[bb].saturating_add(1);
    }
    let inv_total = 1.0 / (total as f64);
    let mad = (sad * inv_total).clamp(0.0, 1.0);
    let mut hist_l1 = 0.0_f64;
    for b in 0..16 {
        let ha = f64::from(hist_a[b]) * inv_total;
        let hb = f64::from(hist_b[b]) * inv_total;
        hist_l1 += (ha - hb).abs();
    }
    let hist_dist = (0.5 * hist_l1).clamp(0.0, 1.0);
    let score = (0.55 * mad + 0.45 * hist_dist) as f32;
    if score.is_finite() {
        score.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Detecta instantes de corte usando uma função provedora de quadros para testabilidade e reuso.
pub fn detect_cuts_from_samples<F>(
    range: (Ticks, Ticks),
    step: Ticks,
    cfg: SceneDetectConfig,
    cancel: Option<&AtomicBool>,
    mut fetch: F,
) -> Vec<Ticks>
where
    F: FnMut(Ticks) -> Option<Arc<RgbaImage>>,
{
    let cfg = cfg.sanitized();
    let start = range.0.max(Ticks::ZERO);
    let end = range.1.max(start);
    let step = step.max(Ticks::from_millis_i64(15));
    if end <= start + step {
        return Vec::new();
    }
    let mut cuts = Vec::new();
    let mut prev: Option<(Ticks, Arc<RgbaImage>)> = None;
    let mut last_cut = start;
    let mut t = start;
    for _ in 0..MAX_SCENE_STEPS {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) || t >= end {
            break;
        }
        if let Some(frame) = fetch(t) {
            if let Some((_, ref prev_img)) = prev {
                if t - last_cut >= cfg.min_scene_ticks {
                    let s = scene_change_score(prev_img, &frame);
                    if s >= cfg.threshold {
                        cuts.push(t);
                        last_cut = t;
                    }
                }
            }
            prev = Some((t, frame));
        }
        let next = t + step;
        if next <= t {
            break;
        }
        t = next;
    }
    cuts
}

/// Detecta cortes em um item de mídia de vídeo no intervalo `[range.0..range.1]`.
pub fn detect_cuts_in_media(
    item: &MediaItem,
    cache: &MediaCache,
    range: (Ticks, Ticks),
    step: Ticks,
    cfg: SceneDetectConfig,
    cancel: Option<&AtomicBool>,
) -> Vec<Ticks> {
    if !item.has_video {
        return Vec::new();
    }
    let cfg = cfg.sanitized();
    detect_cuts_from_samples(range, step, cfg, cancel, |mt| {
        cache.video_frame_for_quality(item, mt, cfg.sample_w, cfg.sample_h, true)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_img(w: u32, h: u32, rgb: [u8; 3]) -> RgbaImage {
        let mut img = RgbaImage::new(w, h);
        for p in img.pixels_mut() {
            *p = image::Rgba([rgb[0], rgb[1], rgb[2], 255]);
        }
        img
    }

    #[test]
    fn detect_cuts_identifica_cortes_e_respeita_limiar_e_minimo() {
        let step = Ticks::from_millis_i64(40);
        let cfg = SceneDetectConfig {
            threshold: 0.30,
            min_scene_ticks: Ticks::from_millis_i64(200),
            sample_w: 32,
            sample_h: 18,
        };
        let cut1 = Ticks::from_millis_i64(400);
        let cut2_too_close = Ticks::from_millis_i64(480);
        let cut3 = Ticks::from_millis_i64(800);

        let cuts = detect_cuts_from_samples(
            (Ticks::ZERO, Ticks::from_secs_i64(1)),
            step,
            cfg,
            None,
            |t| {
                let rgb = if t < cut1 {
                    [20, 25, 30]
                } else if t < cut2_too_close {
                    [235, 240, 245]
                } else if t < cut3 {
                    // Volta para médio logo em seguida (< min_scene_ticks do corte anterior)
                    [180, 185, 190]
                } else {
                    [15, 20, 25]
                };
                Some(Arc::new(solid_img(32, 18, rgb)))
            },
        );
        assert_eq!(cuts, vec![cut1, cut3]);
    }

    #[test]
    fn scene_change_score_resiste_dimensoes_hostis_e_vazias() {
        let empty = RgbaImage::new(0, 0);
        let small = solid_img(16, 9, [200, 200, 200]);
        let diff = solid_img(32, 18, [10, 10, 10]);
        assert_eq!(scene_change_score(&empty, &empty), 0.0);
        assert_eq!(scene_change_score(&small, &diff), 0.0);
        let same = solid_img(16, 9, [200, 200, 200]);
        assert_eq!(scene_change_score(&small, &same), 0.0);
    }
}
