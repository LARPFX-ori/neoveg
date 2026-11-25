//! Faixa de status de renderização sob a régua da timeline.
//!
//! Estima, trecho a trecho, se a reprodução vai acompanhar o tempo real ou se
//! o trecho precisa ser pré-renderizado (*Pré-renderizar seletivamente*,
//! `Shift+M`). A convenção de cores é a das faixas de renderização descritas
//! na documentação pública de editores de vídeo:
//!
//! - **verde**: já pré-renderizado (`EditorSession::prerendered_ranges`);
//! - **amarelo**: tem processamento, mas deve tocar em tempo real;
//! - **vermelho**: processamento pesado, deve precisar de pré-renderização;
//! - sem cor: vídeo simples, que toca direto.
//!
//! É uma estimativa barata (não decodifica nada): soma um custo por evento
//! de vídeo ativo no trecho, pelas camadas sobrepostas, efeitos, transições,
//! velocidade, Pan/Crop e Movimento da trilha. Os pesos ficam nas constantes
//! abaixo.

use crate::model::{Event, FxKind, Project, Rate, Ticks, Track, TrackKind};

/// Necessidade de renderização de um trecho.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RenderNeed {
    /// Vídeo simples: toca direto (sem cor na faixa).
    Realtime,
    /// Tem processamento, mas deve tocar em tempo real (amarelo).
    MayPlay,
    /// Processamento pesado: deve precisar pré-renderizar (vermelho).
    NeedsRender,
    /// Já pré-renderizado (verde).
    Rendered,
}

/// Custo até o qual o trecho conta como simples (uma camada sem efeitos).
const COST_REALTIME: u32 = 1;
/// Custo até o qual o trecho ainda deve tocar em tempo real.
const COST_MAY_PLAY: u32 = 7;
/// Limite de trechos devolvidos (entrada hostil, §0.4).
const MAX_SEGMENTS: usize = 20_000;

fn fx_cost(kind: FxKind) -> u32 {
    match kind {
        // plug-ins de terceiros e efeitos que olham vários quadros ou pixels
        FxKind::Ofx | FxKind::Stabilization | FxKind::Denoise | FxKind::MotionBlur => 6,
        FxKind::GaussianBlur
        | FxKind::LinearBlur
        | FxKind::RadialBlur
        | FxKind::Median
        | FxKind::LensFlare
        | FxKind::LightRays
        | FxKind::BumpMap
        | FxKind::ConvolutionKernel
        | FxKind::Deform
        | FxKind::TinyPlanet
        | FxKind::ChromaKeyer
        | FxKind::BezierMasking
        | FxKind::LensCorrection
        | FxKind::FilmEffects => 3,
        _ => 1,
    }
}

fn effects_cost(fx: &[crate::model::FxInstance]) -> u32 {
    fx.iter()
        .filter(|f| f.enabled)
        .map(|f| fx_cost(f.kind))
        .fold(0u32, u32::saturating_add)
}

/// Custo de um evento de vídeo ativo, sem contar a camada base.
fn event_cost(e: &Event) -> u32 {
    let mut c = effects_cost(&e.fx);
    if e.rate != Rate::NORMAL || e.reverse {
        c = c.saturating_add(2);
    }
    if e.transition.is_some() {
        c = c.saturating_add(2);
    }
    if !e.pan_crop.is_identity() {
        c = c.saturating_add(1);
    }
    if e.reduce_flicker {
        c = c.saturating_add(1);
    }
    c
}

/// Custo fixo de uma trilha com camada ativa (efeitos e movimento da trilha).
fn track_cost(t: &Track) -> u32 {
    let mut c = effects_cost(&t.fx);
    if !t.motion.is_identity() {
        c = c.saturating_add(1);
    }
    c
}

/// Custo do instante `t`: cada camada de vídeo ativa vale 1, mais os efeitos
/// do evento e da trilha.
fn cost_at(project: &Project, t: Ticks) -> u32 {
    let mut total = 0u32;
    for tr in project
        .tracks
        .iter()
        .filter(|tr| tr.kind == TrackKind::Video && !tr.mute)
    {
        let mut active = false;
        for e in tr
            .events
            .iter()
            .filter(|e| !e.mute && e.start <= t && t < e.end())
        {
            active = true;
            total = total.saturating_add(1).saturating_add(event_cost(e));
        }
        if active {
            total = total.saturating_add(track_cost(tr));
        }
    }
    total.saturating_add(effects_cost(&project.master.video_fx))
}

fn need_for_cost(cost: u32) -> RenderNeed {
    if cost <= COST_REALTIME {
        RenderNeed::Realtime
    } else if cost <= COST_MAY_PLAY {
        RenderNeed::MayPlay
    } else {
        RenderNeed::NeedsRender
    }
}

/// Trechos `(início, fim, necessidade)` de `[0, fim do projeto)`, em ordem e
/// sem buracos entre os eventos de vídeo. Trechos vizinhos com a mesma
/// necessidade são unidos; o que está em `prerendered` vira `Rendered`.
pub fn render_status(
    project: &Project,
    prerendered: &[(Ticks, Ticks)],
) -> Vec<(Ticks, Ticks, RenderNeed)> {
    let mut cuts: Vec<Ticks> = Vec::new();
    for tr in project
        .tracks
        .iter()
        .filter(|tr| tr.kind == TrackKind::Video)
    {
        for e in &tr.events {
            cuts.push(e.start.max(Ticks::ZERO));
            cuts.push(e.end().max(Ticks::ZERO));
        }
    }
    for &(a, b) in prerendered {
        cuts.push(a.max(Ticks::ZERO));
        cuts.push(b.max(Ticks::ZERO));
    }
    cuts.sort();
    cuts.dedup();
    let mut out: Vec<(Ticks, Ticks, RenderNeed)> = Vec::new();
    for w in cuts.windows(2) {
        let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else {
            continue;
        };
        if b <= a {
            continue;
        }
        let mid = Ticks::from_raw(a.0.saturating_add((b.0 - a.0) / 2));
        let need = if prerendered.iter().any(|&(pa, pb)| pa <= mid && mid < pb) {
            RenderNeed::Rendered
        } else {
            need_for_cost(cost_at(project, mid))
        };
        match out.last_mut() {
            Some(last) if last.2 == need && last.1 == a => last.1 = b,
            _ => {
                if out.len() >= MAX_SEGMENTS {
                    break;
                }
                out.push((a, b, need));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FxInstance, GenSpec, MediaItem, MediaSource, Track};

    fn media() -> MediaItem {
        MediaItem {
            id: 1,
            name: "m".into(),
            source: MediaSource::Generated(GenSpec::SolidColor {
                color: [0.0, 0.0, 0.0, 1.0],
            }),
            duration: Some(Ticks::from_secs_i64(60)),
            has_video: true,
            has_audio: false,
            width: 1920,
            height: 1080,
            fps: None,
            info: String::new(),
            proxy_path: None,
        }
    }

    fn s(n: i64) -> Ticks {
        Ticks::from_secs_i64(n)
    }

    #[test]
    fn simples_amarelo_vermelho_e_verde() {
        let m = media();
        let mut p = Project::default();
        let mut v1 = Track::new(TrackKind::Video, "V1");
        // 0–4 s: evento simples
        v1.events.push(Event::new(&m, s(0), s(4)));
        // 4–8 s: com dois efeitos leves
        let mut e2 = Event::new(&m, s(4), s(4));
        e2.fx.push(FxInstance::new(FxKind::BrightnessContrast));
        e2.fx.push(FxInstance::new(FxKind::Sepia));
        v1.events.push(e2);
        // 8–12 s: estabilização + desfoque (pesado)
        let mut e3 = Event::new(&m, s(8), s(4));
        e3.fx.push(FxInstance::new(FxKind::Stabilization));
        e3.fx.push(FxInstance::new(FxKind::GaussianBlur));
        v1.events.push(e3);
        p.tracks.push(v1);
        let st = render_status(&p, &[]);
        assert_eq!(
            st,
            vec![
                (s(0), s(4), RenderNeed::Realtime),
                (s(4), s(8), RenderNeed::MayPlay),
                (s(8), s(12), RenderNeed::NeedsRender),
            ]
        );
        // pré-renderizar 9–11 s deixa esse pedaço verde
        let st = render_status(&p, &[(s(9), s(11))]);
        assert!(st.contains(&(s(9), s(11), RenderNeed::Rendered)));
        assert!(st.contains(&(s(8), s(9), RenderNeed::NeedsRender)));
    }

    #[test]
    fn camadas_sobrepostas_somam_e_audio_nao_conta() {
        let m = media();
        let mut p = Project::default();
        for _ in 0..3 {
            let mut t = Track::new(TrackKind::Video, "V");
            t.events.push(Event::new(&m, s(0), s(2)));
            p.tracks.push(t);
        }
        let mut a = Track::new(TrackKind::Audio, "A");
        a.events.push(Event::new(&m, s(0), s(10)));
        p.tracks.push(a);
        // três camadas simples: tempo real com processamento (amarelo)
        assert_eq!(
            render_status(&p, &[]),
            vec![(s(0), s(2), RenderNeed::MayPlay)]
        );
    }

    #[test]
    fn entrada_hostil_nao_trava() {
        let p = Project::default();
        assert!(render_status(&p, &[]).is_empty());
        assert!(render_status(&p, &[(s(5), s(1)), (Ticks::MIN_NEGATIVE, s(2))]).len() <= 2);
    }
}
