//! Importação em sala limpa (*Clean Room*, `AGENTS.md` §2.3) de **AAF**
//! (*Advanced Authoring Format*), a partir das especificações públicas da AAF
//! Association / AMWA: *AAF Low-Level Container Specification* (formato de
//! objetos armazenados em Compound File Binary) e *AAF Object Specification
//! v1.1* (modelo de Mobs, MobSlots, Sequence, SourceClip, Filler, Transition).
//!
//! O contêiner Compound File é lido pelo crate `cfb` (MIT). Só os streams de
//! metadados (`properties` e índices) são lidos; a essência embutida é
//! ignorada. Toda a leitura é limitada (§0.4–§0.5) e protegida por
//! `catch_unwind` (§0.6).

use crate::model::{
    new_id, Event, MediaItem, MediaSource, Project, Track, TrackKind, TransitionKind,
    TransitionSpec,
};
use crate::otio::decode_target_url;
use neoveg_common::error::NeovegError;
use neoveg_common::time::{FrameRate, Ticks};
use std::collections::HashMap;
use std::io::{Read, Seek};
use std::path::Path;

/// Tamanho máximo de um stream de metadados lido.
const MAX_STREAM_BYTES: u64 = 4 * 1024 * 1024;
/// Máximo de objetos visitados num arquivo.
const MAX_OBJECTS: usize = 200_000;
/// Profundidade máxima de segmentos aninhados.
const MAX_DEPTH: usize = 32;
/// Máximo de filhos num vetor ou conjunto.
const MAX_CHILDREN: usize = 100_000;
const MAX_AAF_CLIPS: usize = 20_000;
/// Maior posição em unidades de edição aceita (~115 h a 120 fps).
const MAX_UNITS: i64 = 50_000_000;

// Formas de armazenamento (*stored forms*) da especificação de contêiner.
const SF_DATA: u16 = 0x82;
const SF_STRONG_REF: u16 = 0x22;
const SF_STRONG_REF_VECTOR: u16 = 0x32;
const SF_STRONG_REF_SET: u16 = 0x3A;
const SF_WEAK_REF: u16 = 0x02;

// Identificadores de propriedade (*pids*) da especificação de objetos.
const PID_ROOT_HEADER: u16 = 0x0002;
const PID_HEADER_CONTENT: u16 = 0x3B03;
const PID_CONTENT_MOBS: u16 = 0x1901;
const PID_MOB_ID: u16 = 0x4401;
const PID_MOB_NAME: u16 = 0x4402;
const PID_MOB_SLOTS: u16 = 0x4403;
const PID_SLOT_ID: u16 = 0x4801;
const PID_SLOT_SEGMENT: u16 = 0x4803;
const PID_SLOT_EDIT_RATE: u16 = 0x4B01;
const PID_COMPONENT_DATADEF: u16 = 0x0201;
const PID_COMPONENT_LENGTH: u16 = 0x0202;
const PID_SEQUENCE_COMPONENTS: u16 = 0x1001;
const PID_SOURCE_ID: u16 = 0x1101;
const PID_SOURCE_SLOT_ID: u16 = 0x1102;
const PID_CLIP_START: u16 = 0x1201;
const PID_SOURCE_MOB_DESCRIPTOR: u16 = 0x4701;
const PID_DESCRIPTOR_LOCATOR: u16 = 0x2F01;
const PID_LOCATOR_URL: u16 = 0x4001;

// Códigos de classe: byte variável do AUID `0d010101-0101-XX00-060e-2b3402060101`.
const CLASS_FILLER: u8 = 0x09;
const CLASS_SEQUENCE: u8 = 0x0F;
const CLASS_SOURCE_CLIP: u8 = 0x11;
const CLASS_TRANSITION: u8 = 0x17;
const CLASS_COMPOSITION_MOB: u8 = 0x35;
const CLASS_SOURCE_MOB: u8 = 0x37;

/// AUIDs (bytes como gravados, little-endian nos três primeiros campos) das
/// definições de dados *Picture* e *Sound*, nas formas SMPTE e legada.
const DD_PICTURE: [u8; 16] = [
    0x02, 0x02, 0x03, 0x01, 0x00, 0x01, 0x00, 0x00, 0x06, 0x0E, 0x2B, 0x34, 0x04, 0x01, 0x01, 0x01,
];
const DD_SOUND: [u8; 16] = [
    0x02, 0x02, 0x03, 0x01, 0x00, 0x02, 0x00, 0x00, 0x06, 0x0E, 0x2B, 0x34, 0x04, 0x01, 0x01, 0x01,
];
const DD_LEGACY_PICTURE: [u8; 16] = [
    0xE1, 0x8C, 0x3C, 0x6F, 0xEF, 0x6C, 0xD2, 0x11, 0x80, 0x7D, 0x00, 0x60, 0x08, 0x14, 0x3E, 0x6F,
];
const DD_LEGACY_SOUND: [u8; 16] = [
    0xE1, 0xEB, 0xE1, 0x78, 0xEF, 0x6C, 0xD2, 0x11, 0x80, 0x7D, 0x00, 0x60, 0x08, 0x14, 0x3E, 0x6F,
];

fn err(msg: impl Into<String>) -> NeovegError {
    NeovegError::InvalidInput(msg.into())
}

// ---------------------------------------------------------------------------
// Objetos armazenados
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Media {
    Picture,
    Sound,
    Other,
}

struct Obj {
    path: String,
    class: Option<u8>,
    props: Vec<(u16, u16, Vec<u8>)>,
}

impl Obj {
    fn prop(&self, pid: u16) -> Option<(u16, &[u8])> {
        self.props
            .iter()
            .find(|(p, _, _)| *p == pid)
            .map(|(_, sf, v)| (*sf, v.as_slice()))
    }

    fn data(&self, pid: u16) -> Option<&[u8]> {
        self.prop(pid)
            .filter(|(sf, _)| *sf == SF_DATA)
            .map(|(_, v)| v)
    }

    fn i64(&self, pid: u16) -> Option<i64> {
        let b = self.data(pid)?;
        Some(i64::from_le_bytes(b.get(..8)?.try_into().ok()?))
    }

    fn u32(&self, pid: u16) -> Option<u32> {
        let b = self.data(pid)?;
        Some(u32::from_le_bytes(b.get(..4)?.try_into().ok()?))
    }

    fn string(&self, pid: u16) -> Option<String> {
        self.data(pid).map(utf16_string)
    }

    fn media(&self) -> Media {
        let Some((sf, v)) = self.prop(PID_COMPONENT_DATADEF) else {
            return Media::Other;
        };
        if sf != SF_WEAK_REF {
            return Media::Other;
        }
        // tag u16 | pid da chave u16 | tamanho da chave u8 | chave
        let size = v.get(4).copied().unwrap_or(0) as usize;
        let Some(key) = v.get(5..5 + size) else {
            return Media::Other;
        };
        if key == DD_PICTURE || key == DD_LEGACY_PICTURE {
            Media::Picture
        } else if key == DD_SOUND || key == DD_LEGACY_SOUND {
            Media::Sound
        } else {
            Media::Other
        }
    }
}

fn utf16_string(b: &[u8]) -> String {
    let (pairs, _) = b.as_chunks::<2>();
    let units: Vec<u16> = pairs
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|u| *u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// Lê o stream `properties` (cabeçalho: ordem de bytes `'L'`, versão,
/// contagem; entradas `pid, forma, tamanho`; depois os valores).
fn parse_properties(b: &[u8]) -> Result<Vec<(u16, u16, Vec<u8>)>, NeovegError> {
    if b.first() != Some(&0x4C) {
        return Err(err(
            "stream de propriedades AAF fora do formato little-endian",
        ));
    }
    let count = b
        .get(2..4)
        .map(|c| u16::from_le_bytes([c[0], c[1]]) as usize)
        .ok_or_else(|| err("stream de propriedades AAF truncado"))?;
    let mut pos = 4usize.saturating_add(count.saturating_mul(6));
    let mut out = Vec::with_capacity(count.min(256));
    for i in 0..count {
        let e = 4 + i * 6;
        let (Some(p), Some(sf), Some(len)) =
            (b.get(e..e + 2), b.get(e + 2..e + 4), b.get(e + 4..e + 6))
        else {
            return Err(err("entrada de propriedade AAF truncada"));
        };
        let len = u16::from_le_bytes([len[0], len[1]]) as usize;
        let end = pos
            .checked_add(len)
            .ok_or_else(|| err("tamanho de propriedade AAF inválido"))?;
        let value = b
            .get(pos..end)
            .ok_or_else(|| err("valor de propriedade AAF truncado"))?
            .to_vec();
        out.push((
            u16::from_le_bytes([p[0], p[1]]),
            u16::from_le_bytes([sf[0], sf[1]]),
            value,
        ));
        pos = end;
    }
    Ok(out)
}

struct Reader<F> {
    comp: cfb::CompoundFile<F>,
    visited: usize,
}

impl<F: Read + Seek> Reader<F> {
    fn stream(&mut self, path: &str) -> Option<Vec<u8>> {
        let mut s = self.comp.open_stream(path).ok()?;
        let mut buf = Vec::new();
        (&mut s)
            .take(MAX_STREAM_BYTES + 1)
            .read_to_end(&mut buf)
            .ok()?;
        if buf.len() as u64 > MAX_STREAM_BYTES {
            return None;
        }
        Some(buf)
    }

    fn object(&mut self, path: &str) -> Result<Obj, NeovegError> {
        self.visited += 1;
        if self.visited > MAX_OBJECTS {
            return Err(err(format!("limite de {MAX_OBJECTS} objetos AAF atingido")));
        }
        let class = self.comp.entry(path).ok().and_then(|e| {
            let b = e.clsid().as_bytes();
            let aaf = b.get(..6) == Some(&[0x0D, 0x01, 0x01, 0x01, 0x01, 0x01][..])
                && b.get(8..) == Some(&[0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01][..]);
            if aaf {
                b.get(6).copied()
            } else {
                None
            }
        });
        let raw = self
            .stream(&format!("{path}/properties"))
            .ok_or_else(|| err(format!("objeto AAF sem propriedades: {path}")))?;
        Ok(Obj {
            path: path.to_string(),
            class,
            props: parse_properties(&raw)?,
        })
    }

    /// Filho de uma referência forte simples.
    fn strong(&mut self, o: &Obj, pid: u16) -> Option<Obj> {
        let (sf, v) = o.prop(pid)?;
        if sf != SF_STRONG_REF {
            return None;
        }
        let name = utf16_string(v);
        self.object(&format!("{}/{name}", o.path.trim_end_matches('/')))
            .ok()
    }

    /// Filhos de um vetor (na ordem do índice) ou conjunto de referências fortes.
    fn strong_list(&mut self, o: &Obj, pid: u16) -> Vec<Obj> {
        let Some((sf, v)) = o.prop(pid) else {
            return vec![];
        };
        let name = utf16_string(v);
        let Some(idx) = self.stream(&format!("{}/{name} index", o.path.trim_end_matches('/')))
        else {
            return vec![];
        };
        let rd = |i: usize| -> Option<u32> {
            Some(u32::from_le_bytes(idx.get(i..i + 4)?.try_into().ok()?))
        };
        let count = rd(0).unwrap_or(0) as usize;
        let keys: Vec<u32> = match sf {
            SF_STRONG_REF_VECTOR => (0..count.min(MAX_CHILDREN))
                .map_while(|i| rd(12 + i * 4))
                .collect(),
            SF_STRONG_REF_SET => {
                // count | firstFree | lastFree | pid da identificação u16 | tamanho u8
                let id_size = idx.get(14).copied().unwrap_or(0) as usize;
                let entry = 8 + id_size;
                (0..count.min(MAX_CHILDREN))
                    .map_while(|i| rd(15 + i * entry))
                    .collect()
            }
            _ => vec![],
        };
        keys.into_iter()
            .filter_map(|k| {
                self.object(&format!("{}/{name}{{{k:x}}}", o.path.trim_end_matches('/')))
                    .ok()
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Modelo intermediário
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Seg {
    Sequence(Vec<Seg>),
    Clip {
        len: i64,
        start: i64,
        source: [u8; 32],
        slot: u32,
    },
    Filler(i64),
    Transition(i64),
    Other(i64),
}

impl Seg {
    fn first_clip(&self) -> Option<(i64, [u8; 32], u32)> {
        match self {
            Seg::Clip {
                start,
                source,
                slot,
                ..
            } => Some((*start, *source, *slot)),
            Seg::Sequence(v) => v.iter().find_map(Seg::first_clip),
            _ => None,
        }
    }
}

struct Slot {
    id: u32,
    rate: Option<FrameRate>,
    media: Media,
    segment: Seg,
}

struct Mob {
    class: u8,
    name: String,
    slots: Vec<Slot>,
    locator: Option<String>,
}

fn read_segment<F: Read + Seek>(r: &mut Reader<F>, o: &Obj, depth: usize) -> Seg {
    let len = o.i64(PID_COMPONENT_LENGTH).unwrap_or(0).clamp(0, MAX_UNITS);
    if depth > MAX_DEPTH {
        return Seg::Other(len);
    }
    match o.class {
        Some(CLASS_SEQUENCE) => {
            let comps = r.strong_list(o, PID_SEQUENCE_COMPONENTS);
            Seg::Sequence(
                comps
                    .iter()
                    .map(|c| read_segment(r, c, depth + 1))
                    .collect(),
            )
        }
        Some(CLASS_SOURCE_CLIP) => {
            let mut source = [0u8; 32];
            if let Some(b) = o.data(PID_SOURCE_ID).and_then(|b| b.get(..32)) {
                source.copy_from_slice(b);
            }
            Seg::Clip {
                len,
                start: o.i64(PID_CLIP_START).unwrap_or(0).clamp(0, MAX_UNITS),
                source,
                slot: o.u32(PID_SOURCE_SLOT_ID).unwrap_or(0),
            }
        }
        Some(CLASS_FILLER) => Seg::Filler(len),
        Some(CLASS_TRANSITION) => Seg::Transition(len),
        _ => Seg::Other(len),
    }
}

fn read_rate(o: &Obj) -> Option<FrameRate> {
    let b = o.data(PID_SLOT_EDIT_RATE)?;
    let num = i32::from_le_bytes(b.get(..4)?.try_into().ok()?);
    let den = i32::from_le_bytes(b.get(4..8)?.try_into().ok()?);
    if num <= 0 || den <= 0 {
        return None;
    }
    FrameRate::new(num as u32, den as u32)
}

fn read_mob<F: Read + Seek>(r: &mut Reader<F>, o: &Obj) -> Option<([u8; 32], Mob)> {
    let class = o.class?;
    let mut id = [0u8; 32];
    id.copy_from_slice(o.data(PID_MOB_ID)?.get(..32)?);
    let slots = r
        .strong_list(o, PID_MOB_SLOTS)
        .iter()
        .filter_map(|s| {
            let seg_obj = r.strong(s, PID_SLOT_SEGMENT)?;
            Some(Slot {
                id: s.u32(PID_SLOT_ID).unwrap_or(0),
                rate: read_rate(s),
                media: seg_obj.media(),
                segment: read_segment(r, &seg_obj, 0),
            })
        })
        .collect();
    let locator = if class == CLASS_SOURCE_MOB {
        r.strong(o, PID_SOURCE_MOB_DESCRIPTOR).and_then(|d| {
            r.strong_list(&d, PID_DESCRIPTOR_LOCATOR)
                .iter()
                .find_map(|l| l.string(PID_LOCATOR_URL))
        })
    } else {
        None
    };
    Some((
        id,
        Mob {
            class,
            name: o.string(PID_MOB_NAME).unwrap_or_default(),
            slots,
            locator,
        },
    ))
}

// ---------------------------------------------------------------------------
// Conversão para o projeto
// ---------------------------------------------------------------------------

struct Build<'a> {
    mobs: &'a HashMap<[u8; 32], Mob>,
    media: Vec<MediaItem>,
    media_by_mob: HashMap<[u8; 32], usize>,
    warnings: Vec<String>,
    clips: usize,
}

impl Build<'_> {
    /// Segue MasterMob → SourceMob e devolve (mob de origem, deslocamento extra).
    fn resolve(&self, source: [u8; 32], slot: u32) -> Option<([u8; 32], i64)> {
        let mut id = source;
        let mut slot_id = slot;
        let mut extra = 0i64;
        for _ in 0..8 {
            let mob = self.mobs.get(&id)?;
            if mob.class == CLASS_SOURCE_MOB && mob.locator.is_some() {
                return Some((id, extra));
            }
            let s = mob.slots.iter().find(|s| s.id == slot_id)?;
            let (start, next, next_slot) = s.segment.first_clip()?;
            if next == [0u8; 32] {
                return Some((id, extra));
            }
            extra = extra.saturating_add(start);
            id = next;
            slot_id = next_slot;
        }
        None
    }

    fn media_for(
        &mut self,
        source: [u8; 32],
        slot: u32,
        kind: TrackKind,
    ) -> Option<(MediaItem, i64)> {
        let (mob_id, extra) = self.resolve(source, slot)?;
        if let Some(&i) = self.media_by_mob.get(&mob_id) {
            let m = self.media.get_mut(i)?;
            match kind {
                TrackKind::Video => m.has_video = true,
                TrackKind::Audio => m.has_audio = true,
            }
            return Some((m.clone(), extra));
        }
        let mob = self.mobs.get(&mob_id)?;
        let master_name = self
            .mobs
            .get(&source)
            .map(|m| m.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| mob.name.clone());
        let path = mob
            .locator
            .as_deref()
            .map(decode_target_url)
            .unwrap_or_else(|| master_name.clone());
        let dur = mob.slots.iter().find_map(|s| {
            let len = match &s.segment {
                Seg::Filler(l) | Seg::Other(l) | Seg::Clip { len: l, .. } => *l,
                Seg::Sequence(_) | Seg::Transition(_) => return None,
            };
            Some(s.rate?.frame_to_ticks(len))
        });
        let item = MediaItem {
            id: new_id(),
            name: master_name,
            source: MediaSource::File { path },
            duration: dur.filter(|d| *d > Ticks::ZERO),
            has_video: kind == TrackKind::Video,
            has_audio: kind == TrackKind::Audio,
            width: 0,
            height: 0,
            fps: None,
            info: "Importado via AAF".into(),
            proxy_path: None,
        };
        self.media_by_mob.insert(mob_id, self.media.len());
        self.media.push(item.clone());
        Some((item, extra))
    }

    fn track(&mut self, slot: &Slot, kind: TrackKind, name: String) -> Track {
        let mut track = Track::new(kind, &name);
        let Some(rate) = slot.rate else {
            self.warnings
                .push(format!("trilha AAF '{name}' sem taxa de edição ignorada"));
            return track;
        };
        let comps: Vec<&Seg> = match &slot.segment {
            Seg::Sequence(v) => v.iter().collect(),
            other => vec![other],
        };
        let mut cursor = 0i64;
        let mut pending_tr = false;
        for c in comps {
            match c {
                Seg::Filler(l) | Seg::Other(l) => {
                    cursor = cursor.saturating_add(*l).min(MAX_UNITS);
                    pending_tr = false;
                }
                // a transição sobrepõe o fim do clipe anterior e o início do próximo
                Seg::Transition(l) => {
                    cursor = cursor.saturating_sub(*l).max(0);
                    pending_tr = true;
                }
                Seg::Clip {
                    len,
                    start,
                    source,
                    slot: sid,
                } => {
                    if self.clips >= MAX_AAF_CLIPS {
                        self.warnings
                            .push(format!("limite de {MAX_AAF_CLIPS} clipes AAF atingido"));
                        break;
                    }
                    if *len <= 0 {
                        continue;
                    }
                    if *source == [0u8; 32] {
                        cursor = cursor.saturating_add(*len).min(MAX_UNITS);
                        continue;
                    }
                    match self.media_for(*source, *sid, kind) {
                        Some((item, extra)) => {
                            let t0 = rate.frame_to_ticks(cursor);
                            let t1 =
                                rate.frame_to_ticks(cursor.saturating_add(*len).min(MAX_UNITS));
                            let mut ev = Event::new(&item, t0, t1 - t0);
                            ev.name = item.name.clone();
                            ev.offset =
                                rate.frame_to_ticks(start.saturating_add(extra).min(MAX_UNITS));
                            ev.looped = false;
                            if pending_tr {
                                ev.transition = Some(TransitionSpec {
                                    kind: TransitionKind::Dissolve,
                                    softness: 0.0,
                                });
                            }
                            self.clips += 1;
                            track.events.push(ev);
                        }
                        None => self
                            .warnings
                            .push("clipe AAF com origem não encontrada ignorado".into()),
                    }
                    cursor = cursor.saturating_add(*len).min(MAX_UNITS);
                    pending_tr = false;
                }
                Seg::Sequence(_) => {
                    self.warnings.push("sequência AAF aninhada ignorada".into());
                }
            }
        }
        track
    }
}

fn import_from<F: Read + Seek>(
    comp: cfb::CompoundFile<F>,
) -> Result<(Project, Vec<String>), NeovegError> {
    let mut r = Reader { comp, visited: 0 };
    let root = r.object("/")?;
    let header = r
        .strong(&root, PID_ROOT_HEADER)
        .ok_or_else(|| err("arquivo AAF sem Header"))?;
    let content = r
        .strong(&header, PID_HEADER_CONTENT)
        .ok_or_else(|| err("arquivo AAF sem ContentStorage"))?;
    let mut mobs = HashMap::new();
    let mut order = vec![];
    for m in r.strong_list(&content, PID_CONTENT_MOBS) {
        if let Some((id, mob)) = read_mob(&mut r, &m) {
            order.push(id);
            mobs.insert(id, mob);
        }
    }
    let comp_id = order
        .iter()
        .find(|id| {
            mobs.get(*id)
                .is_some_and(|m| m.class == CLASS_COMPOSITION_MOB)
        })
        .copied()
        .ok_or_else(|| err("arquivo AAF sem CompositionMob"))?;
    let Some(comp_mob) = mobs.get(&comp_id) else {
        return Err(err("arquivo AAF sem CompositionMob"));
    };

    let mut b = Build {
        mobs: &mobs,
        media: vec![],
        media_by_mob: HashMap::new(),
        warnings: vec![],
        clips: 0,
    };
    let mut video = vec![];
    let mut audio = vec![];
    let mut fps = None;
    for s in &comp_mob.slots {
        match s.media {
            Media::Picture => {
                fps = fps.or(s.rate.filter(|r| r.as_f64() <= 300.0));
                let n = video.len() + 1;
                video.push(b.track(s, TrackKind::Video, format!("Vídeo {n}")));
            }
            Media::Sound => {
                let n = audio.len() + 1;
                audio.push(b.track(s, TrackKind::Audio, format!("Áudio {n}")));
            }
            Media::Other => {}
        }
    }
    // a primeira trilha de imagem do AAF é a camada de baixo; no Neoveg a de cima vem primeiro
    video.reverse();

    let mut project = Project::new_default();
    project.tracks = video;
    project.tracks.extend(audio);
    if project.tracks.is_empty() {
        project.tracks.push(Track::new(TrackKind::Video, "Vídeo 1"));
    }
    project.media = b.media;
    project.markers.clear();
    project.regions.clear();
    project.notes = comp_mob.name.clone();
    if let Some(f) = fps {
        project.settings.fps = f;
        project.settings.drop_frame = f.is_ntsc_drop_eligible();
    }
    let mut fixes = b.warnings;
    fixes.extend(project.sanitize());
    crate::model::bump_ids(project.max_id());
    Ok((project, fixes))
}

fn guarded<T>(f: impl FnOnce() -> Result<T, NeovegError>) -> Result<T, NeovegError> {
    crate::util::catch_isolated(f)
        .unwrap_or_else(|_| Err(err("falha interna isolada ao ler o AAF")))
}

/// Importa um projeto AAF de bytes em memória.
pub fn import_aaf_bytes(bytes: &[u8]) -> Result<(Project, Vec<String>), NeovegError> {
    guarded(|| {
        let comp = cfb::CompoundFile::open(std::io::Cursor::new(bytes))
            .map_err(|e| err(format!("contêiner AAF inválido: {e}")))?;
        import_from(comp)
    })
}

/// Importa um arquivo AAF (só os metadados são lidos; a essência fica no disco).
pub fn import_aaf_file(path: &Path) -> Result<(Project, Vec<String>), NeovegError> {
    guarded(|| {
        let file = std::fs::File::open(path).map_err(|e| {
            NeovegError::Io(format!("não foi possível abrir {}: {e}", path.display()))
        })?;
        let comp = cfb::CompoundFile::open(file)
            .map_err(|e| err(format!("contêiner AAF inválido: {e}")))?;
        import_from(comp)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> Option<PathBuf> {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("target")
            .join("fixtures")
            .join(name);
        p.is_file().then_some(p)
    }

    #[test]
    fn parse_properties_rejects_truncation() {
        assert!(parse_properties(&[]).is_err());
        assert!(parse_properties(&[0x42, 0x20, 0, 0]).is_err());
        // uma entrada declarada sem valor
        assert!(parse_properties(&[0x4C, 0x20, 1, 0, 1, 0, 0x82, 0, 8, 0]).is_err());
        let ok = parse_properties(&[0x4C, 0x20, 1, 0, 1, 0, 0x82, 0, 2, 0, 7, 0]).expect("ok");
        assert_eq!(ok, vec![(1, 0x82, vec![7, 0])]);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(import_aaf_bytes(b"nada de AAF aqui").is_err());
        assert!(import_aaf_bytes(&[]).is_err());
    }

    /// Fixture gerada por `tools/gen_aaf_fixtures.py` (oráculo externo `pyaaf2`).
    /// Pula quando a fixture não existe.
    #[test]
    fn imports_basic_fixture() {
        let Some(path) = fixture("oracle_basic.aaf") else {
            return;
        };
        let (p, warn) = import_aaf_file(&path).expect("importa");
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!(p.settings.fps, FrameRate::FPS_25);
        assert_eq!(p.notes, "Corte AAF");
        let fps = FrameRate::FPS_25;
        let v = p
            .tracks
            .iter()
            .find(|t| t.kind == TrackKind::Video)
            .expect("vídeo");
        assert_eq!(v.events.len(), 2);
        assert_eq!(v.events[0].start, Ticks::ZERO);
        assert_eq!(v.events[0].length, fps.frame_to_ticks(60));
        assert_eq!(v.events[0].offset, fps.frame_to_ticks(10));
        assert_eq!(v.events[1].start, fps.frame_to_ticks(70));
        assert_eq!(v.events[1].length, fps.frame_to_ticks(70));
        let a = p
            .tracks
            .iter()
            .find(|t| t.kind == TrackKind::Audio)
            .expect("áudio");
        assert_eq!(a.events.len(), 1);
        assert_eq!(p.media.len(), 2);
        let ma = p.media.iter().find(|m| m.name == "A").expect("mídia A");
        assert!(matches!(&ma.source, MediaSource::File { path } if path == "C:/midia/a b.mov"));
        assert!(ma.has_video && ma.has_audio);
        assert_eq!(ma.duration, Some(fps.frame_to_ticks(300)));
    }

    #[test]
    fn imports_transition_fixture_as_overlap() {
        let Some(path) = fixture("oracle_transition.aaf") else {
            return;
        };
        let (p, _) = import_aaf_file(&path).expect("importa");
        let fps = FrameRate::FPS_25;
        let v = p
            .tracks
            .iter()
            .find(|t| t.kind == TrackKind::Video)
            .expect("vídeo");
        assert_eq!(v.events.len(), 2);
        // B começa 20 quadros antes do fim de A
        assert_eq!(v.events[1].start, fps.frame_to_ticks(40));
        assert!(v.events[1].transition.is_some());
        assert!(v.events[0].transition.is_none());
    }

    /// Fuzzing por mutação (§0.8) sobre a fixture: nunca entra em pânico.
    #[test]
    fn fuzz_fixture_mutations_never_panic() {
        let Some(path) = fixture("oracle_basic.aaf") else {
            return;
        };
        let Ok(base) = std::fs::read(&path) else {
            return;
        };
        let mut seed: u64 = 0xAAF0_0AAF;
        for i in 0..120 {
            let mut b = base.clone();
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let pos = (seed as usize) % b.len().max(1);
            match i % 3 {
                0 => b.truncate(pos),
                1 => {
                    let n = b.len().max(1);
                    for k in 0..16 {
                        if let Some(x) = b.get_mut(pos.wrapping_add(k * 4096) % n) {
                            *x ^= 1 << ((seed >> k) % 8);
                        }
                    }
                }
                _ => {
                    if let Some(x) = b.get_mut(pos) {
                        *x = 0xFF;
                    }
                }
            }
            let r = std::panic::catch_unwind(|| {
                let _ = import_aaf_bytes(&b);
            });
            assert!(r.is_ok(), "pânico na mutação {i}");
        }
    }
}
