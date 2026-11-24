//! Criador de Apresentação de Slides (*Inserir → Apresentação de slides...*,
//! paridade com o *Slideshow Creator* do VEGAS Pro 21/2026).
//!
//! Opera em tempo exato (`Ticks`), passo único de Desfazer e política zero pânico (§0).

use crate::model::{
    new_id, Event, Id, Keyframe, Keyframes, MediaItem, MediaSource, PanCropKey, Track, TrackKind,
    TransitionKind, TransitionSpec,
};
use crate::session::EditorSession;
use crate::time::Ticks;
use std::path::PathBuf;

/// Um item de imagem na fila do Criador de Apresentação de Slides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlideshowItem {
    pub media_id: Option<Id>,
    pub path: Option<PathBuf>,
    pub name: String,
    /// Rotação em múltiplos de 90° (`0`, `1` = 90° CW, `2` = 180°, `3` = 270° CW).
    pub rotation_quarters: u8,
    /// Data de modificação do arquivo em segundos UNIX (para ordenação por data).
    pub modified_secs: u64,
}

impl SlideshowItem {
    pub fn rotate_cw(&mut self) {
        self.rotation_quarters = (self.rotation_quarters + 1) % 4;
    }

    pub fn rotate_ccw(&mut self) {
        self.rotation_quarters = (self.rotation_quarters + 3) % 4;
    }

    pub fn rotation_degrees(&self) -> f32 {
        f32::from((self.rotation_quarters % 4) * 90)
    }
}

/// Ordenação suportada na barra de ferramentas da aba *Imagens*.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlideshowSort {
    NameAsc,
    NameDesc,
    NewestFirst,
    OldestFirst,
    Shuffle,
}

/// Modo de transição entre slides consecutivos.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SlideshowTransitionMode {
    /// Cortes secos sem sobreposição.
    None,
    /// Crossfade linear (dissolução).
    #[default]
    Crossfade,
    /// Transição de vídeo fixa escolhida pelo usuário.
    Fixed(TransitionKind),
    /// Transição variada determinística entre slides.
    Random,
}

/// Enquadramento / movimento suave aplicado aos slides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SlideshowPanCropMode {
    /// Mantém aspecto padrão do evento.
    #[default]
    FitAspect,
    /// Preenche o quadro sem barras laterais (zoom automático).
    CropToFill,
    /// Movimento Ken Burns suave (zoom lento de 100% para ~90% do quadro).
    GentleZoomIn,
}

/// Especificação completa da apresentação de slides a ser inserida na timeline.
#[derive(Clone, Debug)]
pub struct SlideshowSpec {
    pub items: Vec<SlideshowItem>,
    pub slide_duration: Ticks,
    pub overlap_duration: Ticks,
    pub transition_mode: SlideshowTransitionMode,
    pub pan_crop_mode: SlideshowPanCropMode,
    pub start_at_cursor: bool,
    pub create_new_track: bool,
}

impl Default for SlideshowSpec {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            slide_duration: Ticks::from_secs_i64(4),
            overlap_duration: Ticks::from_secs_i64(1),
            transition_mode: SlideshowTransitionMode::Crossfade,
            pan_crop_mode: SlideshowPanCropMode::FitAspect,
            start_at_cursor: true,
            create_new_track: false,
        }
    }
}

/// Ordena ou embaralha `items` in-place de forma determinística (sem pânico).
pub fn sort_slideshow_items(items: &mut [SlideshowItem], sort: SlideshowSort, seed: u64) {
    match sort {
        SlideshowSort::NameAsc => {
            items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        }
        SlideshowSort::NameDesc => {
            items.sort_by(|a, b| b.name.to_lowercase().cmp(&a.name.to_lowercase()));
        }
        SlideshowSort::NewestFirst => {
            items.sort_by(|a, b| b.modified_secs.cmp(&a.modified_secs));
        }
        SlideshowSort::OldestFirst => {
            items.sort_by(|a, b| a.modified_secs.cmp(&b.modified_secs));
        }
        SlideshowSort::Shuffle => {
            // Fisher-Yates com LCG determinístico (zero dependência extra).
            let mut state = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
            for i in (1..items.len()).rev() {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let j = ((state >> 32) as usize) % (i + 1);
                items.swap(i, j);
            }
        }
    }
}

/// Duração total exata em `Ticks` da apresentação de slides descrita por `spec`.
pub fn total_slideshow_duration(spec: &SlideshowSpec) -> Ticks {
    let n = spec.items.len();
    if n == 0 {
        return Ticks::ZERO;
    }
    let dur = spec.slide_duration.max(Ticks::from_millis_i64(100));
    let ov = effective_overlap(spec, dur);
    let step = (dur - ov).max(Ticks::from_millis_i64(1));
    let total_raw = dur
        .0
        .saturating_add(step.0.saturating_mul((n.saturating_sub(1)) as i64));
    Ticks::from_raw(total_raw).clamp_safe(Ticks::ZERO, Ticks::MAX)
}

fn effective_overlap(spec: &SlideshowSpec, dur: Ticks) -> Ticks {
    if matches!(spec.transition_mode, SlideshowTransitionMode::None) {
        return Ticks::ZERO;
    }
    let max_ov = dur.div_i64(2).max(Ticks::ZERO);
    spec.overlap_duration.clamp_safe(Ticks::ZERO, max_ov)
}

const RANDOM_SLIDESHOW_TRANSITIONS: [TransitionKind; 10] = [
    TransitionKind::Dissolve,
    TransitionKind::PushLeft,
    TransitionKind::PushRight,
    TransitionKind::SlideOver,
    TransitionKind::WipeLeft,
    TransitionKind::WipeRight,
    TransitionKind::IrisOpen,
    TransitionKind::ClockWipe,
    TransitionKind::BarnDoor,
    TransitionKind::PagePeel,
];

/// Insere os slides de `spec` na timeline de `session` em um único passo de Desfazer.
/// Retorna a quantidade de eventos inseridos e os IDs de mídia novos (para pré-carregar).
pub fn apply_slideshow(session: &mut EditorSession, spec: &SlideshowSpec) -> (usize, Vec<Id>) {
    if spec.items.is_empty() {
        return (0, Vec::new());
    }
    let min_len = session
        .project
        .settings
        .frame_dur()
        .max(Ticks::from_millis_i64(33));
    let slide_dur = spec.slide_duration.max(min_len);
    let ov = effective_overlap(spec, slide_dur);
    let step = (slide_dur - ov).max(min_len);

    session.checkpoint();

    // Garante ou cria a trilha de vídeo de destino.
    let target_ti =
        if spec.create_new_track {
            let n = session
                .project
                .tracks
                .iter()
                .filter(|t| t.kind == TrackKind::Video)
                .count()
                + 1;
            session
                .project
                .tracks
                .insert(0, Track::new(TrackKind::Video, &format!("Slideshow {n}")));
            0
        } else if let Some(ti) = session.project.tracks.iter().position(|t| {
            t.kind == TrackKind::Video && !t.locked && !t.is_folder && !t.is_adjustment()
        }) {
            ti
        } else {
            session
                .project
                .tracks
                .insert(0, Track::new(TrackKind::Video, "Vídeo 1"));
            0
        };

    let mut pos = if spec.start_at_cursor {
        session.cursor.max(Ticks::ZERO)
    } else {
        session.project.duration()
    };
    if session.opts.quantize {
        pos = session.project.settings.quantize(pos);
    }

    let mut new_media_ids = Vec::new();
    let mut created_events = Vec::new();
    let default_curve = session.opts.default_video_crossfade_curve;

    for (idx, item) in spec.items.iter().enumerate() {
        let media_item = if let Some(mid) = item.media_id {
            if let Some(m) = session.project.media(mid).cloned() {
                m
            } else {
                continue;
            }
        } else if let Some(path) = &item.path {
            let path_str = path.to_string_lossy().into_owned();
            if let Some(existing) = session
                .project
                .media
                .iter()
                .find(|m| matches!(&m.source, MediaSource::File { path: p } if p == &path_str))
                .cloned()
            {
                existing
            } else {
                let m = MediaItem {
                    id: new_id(),
                    name: item.name.clone(),
                    source: MediaSource::File { path: path_str },
                    duration: Some(slide_dur),
                    has_video: true,
                    has_audio: false,
                    width: session.project.settings.width,
                    height: session.project.settings.height,
                    fps: None,
                    info: String::new(),
                    proxy_path: None,
                };
                new_media_ids.push(m.id);
                session.project.media.push(m.clone());
                m
            }
        } else {
            continue;
        };

        let mut ev = Event::new(&media_item, pos, slide_dur);
        ev.maintain_aspect = !matches!(spec.pan_crop_mode, SlideshowPanCropMode::CropToFill);
        let rot = item.rotation_degrees();
        match spec.pan_crop_mode {
            SlideshowPanCropMode::FitAspect => {
                if rot != 0.0 {
                    let k = PanCropKey {
                        rot,
                        ..PanCropKey::default()
                    };
                    ev.pan_crop = Keyframes {
                        keys: vec![Keyframe {
                            t: Ticks::ZERO,
                            value: k,
                            curve: crate::model::Curve::Smooth,
                        }],
                    };
                }
            }
            SlideshowPanCropMode::CropToFill => {
                let k = PanCropKey {
                    rot,
                    ..PanCropKey::default()
                };
                ev.pan_crop = Keyframes {
                    keys: vec![Keyframe {
                        t: Ticks::ZERO,
                        value: k,
                        curve: crate::model::Curve::Smooth,
                    }],
                };
            }
            SlideshowPanCropMode::GentleZoomIn => {
                let k0 = PanCropKey {
                    rot,
                    size: 1.0,
                    ..PanCropKey::default()
                };
                let k1 = PanCropKey {
                    rot,
                    size: 0.88,
                    ..PanCropKey::default()
                };
                ev.pan_crop = Keyframes {
                    keys: vec![
                        Keyframe {
                            t: Ticks::ZERO,
                            value: k0,
                            curve: crate::model::Curve::Smooth,
                        },
                        Keyframe {
                            t: slide_dur,
                            value: k1,
                            curve: crate::model::Curve::Smooth,
                        },
                    ],
                };
            }
        }

        if idx > 0 && ov > Ticks::ZERO {
            ev.no_crossfade_in = false;
            if default_curve != crate::model::Curve::Linear {
                ev.crossfade_in_curve = Some(default_curve);
            }
            match spec.transition_mode {
                SlideshowTransitionMode::None | SlideshowTransitionMode::Crossfade => {}
                SlideshowTransitionMode::Fixed(kind) => {
                    if kind != TransitionKind::Dissolve {
                        ev.transition = Some(TransitionSpec {
                            kind,
                            softness: 0.0,
                        });
                    }
                }
                SlideshowTransitionMode::Random => {
                    let kind = RANDOM_SLIDESHOW_TRANSITIONS
                        [(idx - 1) % RANDOM_SLIDESHOW_TRANSITIONS.len()];
                    if kind != TransitionKind::Dissolve {
                        ev.transition = Some(TransitionSpec {
                            kind,
                            softness: 0.0,
                        });
                    }
                }
            }
        }

        created_events.push(ev.id);
        session.project.tracks[target_ti].events.push(ev);
        pos = Ticks::from_raw(pos.0.saturating_add(step.0));
    }

    if !created_events.is_empty() {
        session.project.sort_events();
        session.sel_events = created_events.iter().copied().collect();
        session.sel_track = Some(target_ti);
        session.changed();
    }
    (created_events.len(), new_media_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{GenSpec, Project};

    #[test]
    fn slideshow_insere_eventos_sobrepostos_com_rotacao_e_ken_burns() {
        let mut p = Project::new_default();
        p.tracks.clear();
        let mut items = Vec::new();
        for i in 0..3 {
            let m = MediaItem {
                id: new_id(),
                name: format!("foto_{i}.png"),
                source: MediaSource::Generated(GenSpec::SolidColor {
                    color: [0.2 * (i as f32 + 1.0), 0.3, 0.4, 1.0],
                }),
                duration: Some(Ticks::from_secs_i64(5)),
                has_video: true,
                has_audio: false,
                width: 640,
                height: 360,
                fps: None,
                info: String::new(),
                proxy_path: None,
            };
            items.push(SlideshowItem {
                media_id: Some(m.id),
                path: None,
                name: m.name.clone(),
                rotation_quarters: i as u8,
                modified_secs: 100 + i as u64,
            });
            p.media.push(m);
        }
        let mut sess = EditorSession::new(p);
        let spec = SlideshowSpec {
            items,
            slide_duration: Ticks::from_secs_i64(4),
            overlap_duration: Ticks::from_secs_i64(1),
            transition_mode: SlideshowTransitionMode::Random,
            pan_crop_mode: SlideshowPanCropMode::GentleZoomIn,
            start_at_cursor: true,
            create_new_track: true,
        };
        assert_eq!(total_slideshow_duration(&spec), Ticks::from_secs_i64(10));
        let (count, _) = apply_slideshow(&mut sess, &spec);
        assert_eq!(count, 3);
        let evs = &sess.project.tracks[0].events;
        assert_eq!(evs.len(), 3);
        assert_eq!(evs[0].start, Ticks::ZERO);
        assert_eq!(evs[1].start, Ticks::from_secs_i64(3));
        assert_eq!(evs[2].start, Ticks::from_secs_i64(6));
        assert_eq!(evs[1].pan_crop.keys[0].value.rot, 90.0);
        assert_eq!(evs[0].pan_crop.keys.len(), 2);
        assert_eq!(
            crate::crossfade::track_crossfades(&sess.project.tracks[0], false).len(),
            2
        );
    }
}
