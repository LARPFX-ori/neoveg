//! Travar envelopes nos eventos (*Lock Envelopes to Events*, menu Opções;
//! manual do VEGAS Pro 21, pp. 389, 710 e 1014): com a opção ligada, mover
//! um evento ao longo da linha do tempo leva junto os pontos de envelope da
//! trilha e os quadros-chave de trilha (Track Motion, movimento do pai e
//! efeitos da trilha) que estão dentro do trecho do evento.
//!
//! Só funções puras sobre [`Track`]. O arraste da UI tira uma foto da
//! automação no começo e, a cada quadro, recalcula tudo a partir dela com o
//! deslocamento atual ([`AutomationSnapshot::apply`]); assim voltar o mouse
//! ao ponto de partida devolve exatamente os tempos originais, e pontos que
//! passam por cima de outros nunca deixam a lista fora de ordem.

use crate::model::{AnimParam, Envelope, Id, Keyframes, MotionKey, Ticks, Track};

/// Foto da automação de uma trilha no começo de um movimento.
#[derive(Clone, Debug)]
pub struct AutomationSnapshot {
    envelopes: Vec<Envelope>,
    motion: Keyframes<MotionKey>,
    parent_motion: Keyframes<MotionKey>,
    fx: Vec<(Id, Vec<AnimParam>)>,
    /// Trechos `[início, fim]` (ticks da linha do tempo) dos eventos
    /// movidos, no lugar de onde saíram.
    spans: Vec<(Ticks, Ticks)>,
}

/// Limite de trechos guardados (um por evento movido na trilha).
const MAX_SPANS: usize = 4096;

impl AutomationSnapshot {
    /// Foto da automação de `track` para mover os eventos dos trechos
    /// `spans` (início, duração). Trechos com duração negativa são ignorados.
    pub fn capture(track: &Track, spans: &[(Ticks, Ticks)]) -> Self {
        let spans = spans
            .iter()
            .take(MAX_SPANS)
            .filter(|(_, len)| *len >= Ticks::ZERO)
            .map(|&(s, len)| (s, s + len))
            .collect();
        AutomationSnapshot {
            envelopes: track.envelopes.clone(),
            motion: track.motion.clone(),
            parent_motion: track.parent_motion.clone(),
            fx: track.fx.iter().map(|f| (f.id, f.params.clone())).collect(),
            spans,
        }
    }

    fn inside(&self, t: Ticks) -> bool {
        self.spans.iter().any(|&(a, b)| t >= a && t <= b)
    }

    fn shift(&self, t: Ticks, dt: Ticks) -> Ticks {
        if self.inside(t) {
            (t + dt).clamp(Ticks::ZERO, Ticks::MAX)
        } else {
            t
        }
    }

    /// Recoloca a automação de `track` a partir da foto, deslocando por `dt`
    /// os pontos que estavam dentro dos trechos. `dt == 0` restaura a foto.
    /// Envelopes ou efeitos que mudaram de quantidade desde a foto (outra
    /// edição no meio) não são tocados.
    pub fn apply(&self, track: &mut Track, dt: Ticks) {
        if track.envelopes.len() == self.envelopes.len() {
            for (dst, src) in track.envelopes.iter_mut().zip(&self.envelopes) {
                let mut pts = src.points.clone();
                for p in &mut pts {
                    p.t = self.shift(p.t, dt);
                }
                pts.sort_by_key(|p| p.t);
                dst.points = pts;
            }
        }
        for (dst, src) in [
            (&mut track.motion, &self.motion),
            (&mut track.parent_motion, &self.parent_motion),
        ] {
            let mut keys = src.keys.clone();
            for k in &mut keys {
                k.t = self.shift(k.t, dt);
            }
            keys.sort_by_key(|k| k.t);
            dst.keys = keys;
        }
        for fx in &mut track.fx {
            let Some((_, params)) = self.fx.iter().find(|(id, _)| *id == fx.id) else {
                continue;
            };
            if params.len() != fx.params.len() {
                continue;
            }
            for (dst, src) in fx.params.iter_mut().zip(params) {
                let mut keys = src.keys.clone();
                for k in &mut keys {
                    k.0 = self.shift(k.0, dt);
                }
                keys.sort_by_key(|k| k.0);
                dst.keys = keys;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EnvKind, EnvPoint, TrackKind};

    fn s(v: i64) -> Ticks {
        Ticks::from_secs_i64(v)
    }

    fn track_com_envelope() -> Track {
        let mut t = Track::new(TrackKind::Audio, "A");
        let mut env = Envelope::new(EnvKind::Volume);
        env.points = [0, 2, 3, 6, 9]
            .iter()
            .map(|&x| EnvPoint {
                t: s(x),
                v: x as f32,
                curve: Default::default(),
                smoothness: 0.0,
            })
            .collect();
        t.envelopes.push(env);
        t
    }

    fn tempos(t: &Track) -> Vec<i64> {
        t.envelopes[0]
            .points
            .iter()
            .map(|p| p.t.as_secs_f64().round() as i64)
            .collect()
    }

    #[test]
    fn so_os_pontos_do_trecho_andam_e_voltar_restaura() {
        let mut t = track_com_envelope();
        // evento de 2 s a 4 s
        let snap = AutomationSnapshot::capture(&t, &[(s(2), s(2))]);
        snap.apply(&mut t, s(1));
        assert_eq!(tempos(&t), vec![0, 3, 4, 6, 9]);
        // os valores acompanham os pontos
        assert_eq!(t.envelopes[0].points[1].v, 2.0);
        snap.apply(&mut t, Ticks::ZERO);
        assert_eq!(tempos(&t), vec![0, 2, 3, 6, 9]);
    }

    #[test]
    fn pontos_que_passam_por_cima_de_outros_ficam_em_ordem_e_nao_ficam_negativos() {
        let mut t = track_com_envelope();
        let snap = AutomationSnapshot::capture(&t, &[(s(2), s(2))]);
        snap.apply(&mut t, s(5));
        assert_eq!(tempos(&t), vec![0, 6, 7, 8, 9]);
        snap.apply(&mut t, s(-10));
        assert_eq!(tempos(&t), vec![0, 0, 0, 6, 9]);
        assert!(t.envelopes[0].points.windows(2).all(|w| w[0].t <= w[1].t));
    }

    #[test]
    fn quadros_chave_de_trilha_e_de_efeito_andam_juntos() {
        let mut t = Track::new(TrackKind::Video, "V");
        t.motion.keys[0].t = s(1);
        let mut fx = crate::model::FxInstance::new(crate::model::FxKind::GaussianBlur);
        if let Some(p) = fx.params.first_mut() {
            p.keys = vec![(s(1), 0.0), (s(8), 1.0)];
        }
        t.fx.push(fx);
        let snap = AutomationSnapshot::capture(&t, &[(Ticks::ZERO, s(2))]);
        snap.apply(&mut t, s(3));
        assert_eq!(t.motion.keys[0].t, s(4));
        if let Some(p) = t.fx[0].params.first() {
            assert_eq!(p.keys, vec![(s(4), 0.0), (s(8), 1.0)]);
        }
    }

    #[test]
    fn entrada_hostil_nao_trava() {
        let mut t = track_com_envelope();
        // duração negativa: ignorada
        let snap = AutomationSnapshot::capture(&t, &[(s(2), s(-1))]);
        snap.apply(&mut t, s(1));
        assert_eq!(tempos(&t), vec![0, 2, 3, 6, 9]);
        // extremos: satura sem estourar
        let snap = AutomationSnapshot::capture(&t, &[(Ticks::ZERO, Ticks::MAX)]);
        snap.apply(&mut t, Ticks::MAX);
        snap.apply(&mut t, Ticks::MIN_NEGATIVE);
        assert!(t.envelopes[0]
            .points
            .iter()
            .all(|p| p.t >= Ticks::ZERO && p.t <= Ticks::MAX));
        // envelope apagado no meio do arraste: não mexe
        let snap = AutomationSnapshot::capture(&t, &[(Ticks::ZERO, s(5))]);
        t.envelopes.clear();
        snap.apply(&mut t, s(1));
        assert!(t.envelopes.is_empty());
    }
}
