//! Importação e exportação em sala limpa (*Clean Room*, `AGENTS.md` §2.3) de **AAF**
//! (*Advanced Authoring Format*), a partir das especificações e registros públicos:
//! - **AMWA MS-03 v1.0.1** (*AAF Low-Level Container Specification*: Compound File Binary);
//! - **AMWA MS-02 v1.0.1** (*AAF Stored Format Specification*: streams `properties`,
//!   `/referenced properties`, índices de vetores/conjuntos e assinaturas §1.6);
//! - **AMWA MS-01 v1.1** (*AAF Object Specification*: §5/§27 `MetaDictionary`, Header,
//!   Identification, Dictionary, ContentStorage, Mobs, Slots, TimelineMobSlot, Sequence,
//!   SourceClip, Filler, Transition, OperationGroup, ImportDescriptor e NetworkLocator);
//! - **AMWA AS-01** (*AAF Edit Protocol*, 2005-04-08, §12.1 *Video Dissolve* e §15);
//! - **SMPTE Metadata Registers** públicos (`https://registry.smpte-ra.org/`:
//!   SMPTE ST 395 Grupos/Classes, ST 335 Elementos/`LocalIdentification`, ST 2003 Tipos e
//!   ST 400 Rótulos `PictureEssenceTrack`, `SoundEssenceTrack` e `AAFEditProtocol`);
//! - Vetor binário de conformidade em `tools/gen_aaf_fixtures.py` (`OperationDef_VideoDissolve`).
//!
//! O contêiner Compound File é lido e gravado pelo crate `cfb` (MIT). Só os streams de
//! metadados (`properties` e índices) são processados; a essência externa fica
//! referenciada por `NetworkLocator`. Toda a operação é limitada (§0.4–§0.5) e
//! protegida por `catch_unwind` (§0.6).

use crate::model::{
    new_id, Event, Id, MediaItem, MediaSource, Project, Track, TrackKind, TransitionKind,
    TransitionSpec,
};
use crate::otio::{decode_target_url, encode_target_url};
use chrono::{Datelike, Timelike};
use neoveg_common::error::NeovegError;
use neoveg_common::time::{FrameRate, Ticks};
use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read, Seek, Write};
use std::path::Path;
use uuid::Uuid;

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

// Formas de armazenamento (*stored forms*, MS-02 §1.4.1).
pub(crate) const SF_DATA: u16 = 0x82;
pub(crate) const SF_STRONG_REF: u16 = 0x22;
pub(crate) const SF_STRONG_REF_VECTOR: u16 = 0x32;
pub(crate) const SF_STRONG_REF_SET: u16 = 0x3A;
pub(crate) const SF_WEAK_REF: u16 = 0x02;
pub(crate) const SF_WEAK_REF_VECTOR: u16 = 0x12;

// Identificadores de propriedade (*LocalIdentification* / *pid*, SMPTE ST 335).
const PID_ROOT_METADICT: u16 = 0x0001;
const PID_ROOT_HEADER: u16 = 0x0002;
pub(crate) const PID_METADEF_IDENT: u16 = 0x0005;
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

// Códigos de classe: byte variável do AUID `0d010101-0101-XX00-060e-2b3402060101` (SMPTE ST 395).
const CLASS_FILLER: u8 = 0x09;
const CLASS_SEQUENCE: u8 = 0x0F;
const CLASS_SOURCE_CLIP: u8 = 0x11;
const CLASS_TRANSITION: u8 = 0x17;
const CLASS_COMPOSITION_MOB: u8 = 0x35;
const CLASS_SOURCE_MOB: u8 = 0x37;

/// AUIDs em disco (campos LE, meia-troca SMPTE UL) para *DataDefinition* (SMPTE ST 400):
/// - `PictureEssenceTrack`: `urn:smpte:ul:060e2b34.04010101.01030202.01000000`
/// - `SoundEssenceTrack`: `urn:smpte:ul:060e2b34.04010101.01030202.02000000`
///
/// além das formas legadas v1.0 aceitas apenas na importação.
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

// ---------------------------------------------------------------------------
// Exportação AAF (AMWA MS-03 / MS-02 v1.0.1 / MS-01 v1.1 / AS-01 + SMPTE)
// ---------------------------------------------------------------------------

const MAX_EXPORT_UTF16_UNITS: usize = 4096;

// Classes de meta-modelo no AUID `0d010101-XX00-0000-060e-2b3402060101` (SMPTE ST 395).
pub(crate) const CLASS_CLASS_DEFINITION: [u8; 16] = crate::aaf_metadict::auid(
    0x0D01_0101,
    0x0201,
    0x0000,
    [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
);
pub(crate) const CLASS_PROPERTY_DEFINITION: [u8; 16] = crate::aaf_metadict::auid(
    0x0D01_0101,
    0x0202,
    0x0000,
    [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
);
pub(crate) const CLASS_METADICT: [u8; 16] = crate::aaf_metadict::auid(
    0x0D01_0101,
    0x0225,
    0x0000,
    [0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
);

// PIDs adicionais (SMPTE ST 335 Reg-XML `www-smpte-ra-org-reg-335-2012.xml`).
const PID_HEADER_BYTE_ORDER: u16 = 0x3B01;
const PID_HEADER_LAST_MODIFIED: u16 = 0x3B02;
const PID_HEADER_DICTIONARY: u16 = 0x3B04;
const PID_HEADER_VERSION: u16 = 0x3B05;
const PID_HEADER_IDENTIFICATION_LIST: u16 = 0x3B06;
const PID_HEADER_MODEL_VERSION: u16 = 0x3B07;
const PID_HEADER_OP_PATTERN: u16 = 0x3B09;

const PID_IDENT_COMPANY: u16 = 0x3C01;
const PID_IDENT_PRODUCT: u16 = 0x3C02;
const PID_IDENT_VERSION_STR: u16 = 0x3C04;
const PID_IDENT_PRODUCT_ID: u16 = 0x3C05;
const PID_IDENT_DATE: u16 = 0x3C06;
const PID_IDENT_PLATFORM: u16 = 0x3C08;
const PID_IDENT_GENERATION: u16 = 0x3C09;

const PID_DICT_OP_DEFS: u16 = 0x2603;
const PID_DICT_DATA_DEFS: u16 = 0x2605;

const PID_DEF_IDENT: u16 = 0x1B01;
const PID_DEF_NAME: u16 = 0x1B02;
const PID_DEF_DESC: u16 = 0x1B03;

const PID_OPDEF_DATADEF: u16 = 0x1E01;
const PID_OPDEF_NUM_INPUTS: u16 = 0x1E07;

const PID_MOB_LAST_MODIFIED: u16 = 0x4404;
const PID_MOB_CREATION_TIME: u16 = 0x4405;
const PID_SLOT_NAME: u16 = 0x4802;
const PID_SLOT_PHYSICAL_TRACK: u16 = 0x4804;
const PID_SLOT_ORIGIN: u16 = 0x4B02;

const PID_TRANSITION_OP_GROUP: u16 = 0x1801;
const PID_TRANSITION_CUT_POINT: u16 = 0x1802;
const PID_OPGROUP_OP_DEF: u16 = 0x0B01;

// Códigos de classe adicionais (byte 6 de `0d010101-0101-XX00-060e-2b3402060101`, SMPTE ST 395).
const CLASS_OPERATION_GROUP: u8 = 0x0A;
const CLASS_CONTENT_STORAGE: u8 = 0x18;
const CLASS_DATA_DEFINITION: u8 = 0x1B;
const CLASS_OPERATION_DEFINITION: u8 = 0x1C;
const CLASS_DICTIONARY: u8 = 0x22;
const CLASS_HEADER: u8 = 0x2F;
const CLASS_IDENTIFICATION: u8 = 0x30;
const CLASS_NETWORK_LOCATOR: u8 = 0x32;
const CLASS_MASTER_MOB: u8 = 0x36;
const CLASS_TIMELINE_MOB_SLOT: u8 = 0x3B;
const CLASS_IMPORT_DESCRIPTOR: u8 = 0x4A;

/// Assinatura da raiz para setores de 512 bytes (MS-02 §1.6 `aafSignature_Aaf_SSBinary`).
const AAF_SIGNATURE_SSBINARY: Uuid = Uuid::from_fields(
    0x4246_4141,
    0x000D,
    0x4D4F,
    &[0x06, 0x0E, 0x2B, 0x34, 0x01, 0x01, 0x01, 0xFF],
);
/// Assinatura da raiz para setores de 4096 bytes (MS-02 §1.6 `aafSignature_Aaf_SSBin_4K`).
#[allow(dead_code)]
const AAF_SIGNATURE_SSBIN_4K: Uuid = Uuid::from_fields(
    0x0D01_0201,
    0x0200,
    0x0000,
    &[0x06, 0x0E, 0x2B, 0x34, 0x03, 0x02, 0x01, 0x01],
);

/// Rótulo `AAFEditProtocol` (SMPTE ST 400 / AS-01 §15:
/// `urn:smpte:ul:060e2b34.04010105.0d011201.01000000` em ordem AUID de disco).
const OP_PATTERN_EDIT_PROTOCOL: [u8; 16] = crate::aaf_metadict::auid(
    0x0D01_1201,
    0x0100,
    0x0000,
    [0x06, 0x0E, 0x2B, 0x34, 0x04, 0x01, 0x01, 0x05],
);

/// Identificador da operação *Video Dissolve* (`0c3bea40-fc05-11d2-8a29-0050040ef7d2`),
/// documentada em AMWA AS-01 §12.1 (`OperationDef_VideoDissolve`) e verificada pelo
/// vetor de conformidade gerado por `tools/gen_aaf_fixtures.py`.
const OP_VIDEO_DISSOLVE: [u8; 16] = crate::aaf_metadict::auid(
    0x0C3B_EA40,
    0xFC05,
    0x11D2,
    [0x8A, 0x29, 0x00, 0x50, 0x04, 0x0E, 0xF7, 0xD2],
);

pub(crate) const REF_IDX_META_CLASSES: u16 = 0;
pub(crate) const REF_IDX_META_TYPES: u16 = 1;
const REF_IDX_DATA_DEFS: u16 = 2;
const REF_IDX_OP_DEFS: u16 = 3;

pub(crate) struct SetIndexEntry<'a> {
    pub(crate) local_key: u32,
    pub(crate) ref_count: u32,
    pub(crate) key: &'a [u8],
}

fn class_clsid(class: u8) -> Uuid {
    Uuid::from_fields(
        0x0D01_0101,
        0x0101,
        u16::from(class) << 8,
        &[0x06, 0x0E, 0x2B, 0x34, 0x02, 0x06, 0x01, 0x01],
    )
}

pub(crate) fn encode_utf16le_nul(s: &str) -> Vec<u8> {
    let mut units: Vec<u16> = Vec::new();
    for ch in s.chars() {
        if ch == '\0' {
            continue;
        }
        let mut pair = [0u16; 2];
        let slice = ch.encode_utf16(&mut pair);
        if units.len() + slice.len() >= MAX_EXPORT_UTF16_UNITS {
            break;
        }
        units.extend_from_slice(slice);
    }
    units.push(0);
    let mut out = Vec::with_capacity(units.len() * 2);
    for u in units {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out
}

fn aaf_timestamp() -> [u8; 8] {
    let now = chrono::Utc::now();
    let yr = now.year().clamp(1970, 9999) as i16;
    let yr_b = yr.to_le_bytes();
    [
        yr_b[0],
        yr_b[1],
        now.month() as u8,
        now.day() as u8,
        now.hour() as u8,
        now.minute() as u8,
        now.second() as u8,
        0,
    ]
}

fn make_mob_id(tag: &[u8], a: u64, b: u64) -> [u8; 32] {
    // Prefixo SMPTE 330M Material UMID + digest determinístico de 128 bits.
    let mut h1: u64 = 0xCBF2_9CE4_8422_2325;
    let mut h2: u64 = 0xAF63_BD4C_8601_B7DF;
    for &byte in tag
        .iter()
        .chain(a.to_le_bytes().iter())
        .chain(b.to_le_bytes().iter())
    {
        h1 ^= u64::from(byte);
        h1 = h1.wrapping_mul(0x1000_0000_01B3);
        h2 = h2.rotate_left(7) ^ u64::from(byte).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
    let mut out = [0u8; 32];
    out[..16].copy_from_slice(&[
        0x06, 0x0A, 0x2B, 0x34, 0x01, 0x01, 0x01, 0x05, 0x01, 0x01, 0x0F, 0x20, 0x13, 0x00, 0x00,
        0x00,
    ]);
    out[16..24].copy_from_slice(&h1.to_le_bytes());
    out[24..32].copy_from_slice(&h2.to_le_bytes());
    out
}

fn make_uuid_bytes(seed: &str) -> [u8; 16] {
    let mut h1: u64 = 0x517C_C1B7_2722_0A95;
    let mut h2: u64 = 0x9E37_79B9_7F4A_7C15;
    for &b in seed.as_bytes() {
        h1 ^= u64::from(b);
        h1 = h1.wrapping_mul(0x1000_0000_01B3);
        h2 = (h2 ^ u64::from(b)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    }
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&h1.to_le_bytes());
    bytes[8..].copy_from_slice(&h2.to_le_bytes());
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    bytes
}

pub(crate) fn weak_ref_auid(ref_idx: u16, ident_pid: u16, auid: &[u8; 16]) -> Vec<u8> {
    let mut v = Vec::with_capacity(21);
    v.extend_from_slice(&ref_idx.to_le_bytes());
    v.extend_from_slice(&ident_pid.to_le_bytes());
    v.push(16);
    v.extend_from_slice(auid);
    v
}

pub(crate) fn write_stream<F: Read + Seek + Write>(
    comp: &mut cfb::CompoundFile<F>,
    path: &str,
    data: &[u8],
) -> Result<(), NeovegError> {
    let mut s = comp
        .create_stream(path)
        .map_err(|e| err(format!("falha ao criar stream AAF '{path}': {e}")))?;
    s.write_all(data)
        .map_err(|e| err(format!("falha ao gravar stream AAF '{path}': {e}")))?;
    Ok(())
}

pub(crate) fn create_object_clsid<F: Read + Seek + Write>(
    comp: &mut cfb::CompoundFile<F>,
    path: &str,
    clsid: Uuid,
    props: &[(u16, u16, Vec<u8>)],
) -> Result<(), NeovegError> {
    comp.create_storage(path)
        .map_err(|e| err(format!("falha ao criar storage AAF '{path}': {e}")))?;
    comp.set_storage_clsid(path, clsid)
        .map_err(|e| err(format!("falha ao definir CLSID de '{path}': {e}")))?;
    write_properties(comp, path, props)
}

fn create_object<F: Read + Seek + Write>(
    comp: &mut cfb::CompoundFile<F>,
    path: &str,
    class: u8,
    props: &[(u16, u16, Vec<u8>)],
) -> Result<(), NeovegError> {
    create_object_clsid(comp, path, class_clsid(class), props)
}

pub(crate) fn write_properties<F: Read + Seek + Write>(
    comp: &mut cfb::CompoundFile<F>,
    obj_path: &str,
    props: &[(u16, u16, Vec<u8>)],
) -> Result<(), NeovegError> {
    let count = u16::try_from(props.len())
        .map_err(|_| err("objeto AAF excedeu o limite de propriedades"))?;
    let mut buf = Vec::with_capacity(4 + props.len() * 6 + 128);
    buf.push(0x4C); // 'L' little-endian (MS-02 §1.2.1.3.3)
    buf.push(0x20); // _formatVersion 0x20
    buf.extend_from_slice(&count.to_le_bytes());
    for &(pid, sf, ref val) in props {
        let len = u16::try_from(val.len())
            .map_err(|_| err(format!("propriedade AAF 0x{pid:04x} grande demais")))?;
        buf.extend_from_slice(&pid.to_le_bytes());
        buf.extend_from_slice(&sf.to_le_bytes());
        buf.extend_from_slice(&len.to_le_bytes());
    }
    for (_, _, val) in props {
        buf.extend_from_slice(val);
    }
    let prop_path = if obj_path == "/" {
        "/properties".to_string()
    } else {
        format!("{obj_path}/properties")
    };
    write_stream(comp, &prop_path, &buf)
}

fn write_vector_index<F: Read + Seek + Write>(
    comp: &mut cfb::CompoundFile<F>,
    parent_path: &str,
    list_name: &str,
    count: usize,
) -> Result<(), NeovegError> {
    let n = u32::try_from(count).map_err(|_| err("vetor AAF excedeu o limite"))?;
    let mut buf = Vec::with_capacity(12 + count * 4);
    buf.extend_from_slice(&n.to_le_bytes());
    buf.extend_from_slice(&n.to_le_bytes());
    buf.extend_from_slice(&u32::MAX.to_le_bytes());
    for i in 0..n {
        buf.extend_from_slice(&i.to_le_bytes());
    }
    let path = format!("{}/{list_name} index", parent_path.trim_end_matches('/'));
    write_stream(comp, &path, &buf)
}

pub(crate) fn write_set_index<F: Read + Seek + Write>(
    comp: &mut cfb::CompoundFile<F>,
    parent_path: &str,
    set_name: &str,
    ident_pid: u16,
    key_size: u8,
    entries: &mut [SetIndexEntry<'_>],
) -> Result<(), NeovegError> {
    // MS-02 §1.2.1.6.4: entradas aparecem no índice em ordem crescente de chave.
    entries.sort_by(|a, b| a.key.cmp(b.key));
    let n = u32::try_from(entries.len()).map_err(|_| err("conjunto AAF excedeu o limite"))?;
    let mut buf = Vec::with_capacity(15 + entries.len() * (8 + usize::from(key_size)));
    buf.extend_from_slice(&n.to_le_bytes());
    buf.extend_from_slice(&n.to_le_bytes());
    buf.extend_from_slice(&u32::MAX.to_le_bytes());
    buf.extend_from_slice(&ident_pid.to_le_bytes());
    buf.push(key_size);
    for e in entries {
        if e.key.len() != usize::from(key_size) {
            return Err(err("tamanho de chave inválido em índice de conjunto AAF"));
        }
        buf.extend_from_slice(&e.local_key.to_le_bytes());
        buf.extend_from_slice(&e.ref_count.to_le_bytes());
        buf.extend_from_slice(e.key);
    }
    let path = format!("{}/{set_name} index", parent_path.trim_end_matches('/'));
    write_stream(comp, &path, &buf)
}

fn write_referenced_properties<F: Read + Seek + Write>(
    comp: &mut cfb::CompoundFile<F>,
) -> Result<(), NeovegError> {
    let paths: [&[u16]; 4] = [
        &[PID_ROOT_METADICT, 0x0003, 0],
        &[PID_ROOT_METADICT, 0x0004, 0],
        &[
            PID_ROOT_HEADER,
            PID_HEADER_DICTIONARY,
            PID_DICT_DATA_DEFS,
            0,
        ],
        &[PID_ROOT_HEADER, PID_HEADER_DICTIONARY, PID_DICT_OP_DEFS, 0],
    ];
    let count = paths.len() as u16;
    let total_u16s: u32 = paths.iter().map(|p| p.len() as u32).sum();
    let mut buf = Vec::with_capacity(7 + (total_u16s as usize) * 2);
    buf.push(0x4C);
    buf.extend_from_slice(&count.to_le_bytes());
    buf.extend_from_slice(&total_u16s.to_le_bytes());
    for p in paths {
        for pid in p {
            buf.extend_from_slice(&pid.to_le_bytes());
        }
    }
    write_stream(comp, "/referenced properties", &buf)
}

fn export_slot_rate(fps: FrameRate) -> FrameRate {
    let f = fps.as_f64();
    if f.is_finite() && (1.0..=1000.0).contains(&f) && fps.num > 0 && fps.den > 0 {
        fps
    } else {
        FrameRate::FPS_29_97
    }
}

fn export_media_path(m: &MediaItem) -> Option<String> {
    match &m.source {
        MediaSource::File { path } => (!path.is_empty()).then(|| path.clone()),
        MediaSource::ImageSequence { paths, .. } => {
            paths.first().filter(|p| !p.is_empty()).cloned()
        }
        MediaSource::Generated(_) => None,
    }
}

enum ExportSeg {
    Filler(i64),
    VideoDissolve {
        len: i64,
        cut_point: i64,
    },
    Clip {
        len: i64,
        start: i64,
        master_mob: [u8; 32],
        slot_id: u32,
    },
}

struct ExportClip {
    rec_start: i64,
    rec_len: i64,
    src_start: i64,
    master_mob: [u8; 32],
    slot_id: u32,
    has_transition: bool,
}

fn build_track_lanes(
    track: &Track,
    slot_rate: FrameRate,
    master_by_media: &HashMap<Id, ([u8; 32], bool, bool)>,
    remaining_clips: &mut usize,
) -> Vec<Vec<ExportSeg>> {
    let kind_is_picture = track.kind == TrackKind::Video;
    let mut raw_clips: Vec<ExportClip> = Vec::new();
    let mut events: Vec<&Event> = track.events.iter().collect();
    events.sort_by_key(|e| e.start);

    for ev in events {
        if *remaining_clips == 0 {
            break;
        }
        let Some(&(master_mob, has_pic, has_snd)) = master_by_media.get(&ev.media_id) else {
            continue;
        };
        let slot_id = if kind_is_picture {
            if has_pic || !has_snd {
                1
            } else {
                continue;
            }
        } else if has_snd {
            2
        } else {
            continue;
        };
        let rec_start = slot_rate
            .ticks_to_frame_round(ev.start.max(Ticks::ZERO))
            .clamp(0, MAX_UNITS);
        let rec_end = slot_rate
            .ticks_to_frame_round(ev.end().max(Ticks::ZERO))
            .clamp(0, MAX_UNITS);
        let rec_len = (rec_end - rec_start).clamp(0, MAX_UNITS);
        if rec_len <= 0 {
            continue;
        }
        let src_start = slot_rate
            .ticks_to_frame_round(ev.offset.max(Ticks::ZERO))
            .clamp(0, MAX_UNITS);
        *remaining_clips -= 1;
        raw_clips.push(ExportClip {
            rec_start,
            rec_len,
            src_start,
            master_mob,
            slot_id,
            has_transition: ev.transition.is_some(),
        });
    }

    if raw_clips.is_empty() {
        return Vec::new();
    }

    // Vídeo suporta `OperationDef_VideoDissolve` (AS-01 §12.1) quando a sobreposição é
    // estritamente menor que ambos os clipes. Sobreposições de áudio (sem AUID público
    // nos registros SMPTE/MS-01) e demais sobreposições vão para slots paralelos.
    struct Lane {
        cursor: i64,
        last_clip_len: i64,
        segs: Vec<ExportSeg>,
    }

    let mut lanes: Vec<Lane> = Vec::new();
    for clip in raw_clips {
        let mut placed = false;
        for lane in &mut lanes {
            if clip.rec_start >= lane.cursor {
                let gap = clip.rec_start - lane.cursor;
                if gap > 0 {
                    lane.segs.push(ExportSeg::Filler(gap));
                }
                lane.segs.push(ExportSeg::Clip {
                    len: clip.rec_len,
                    start: clip.src_start,
                    master_mob: clip.master_mob,
                    slot_id: clip.slot_id,
                });
                lane.cursor = clip.rec_start.saturating_add(clip.rec_len).min(MAX_UNITS);
                lane.last_clip_len = clip.rec_len;
                placed = true;
                break;
            } else if kind_is_picture && clip.has_transition && !lane.segs.is_empty() {
                let overlap = lane.cursor - clip.rec_start;
                if overlap > 0 && overlap < lane.last_clip_len && overlap < clip.rec_len {
                    lane.segs.push(ExportSeg::VideoDissolve {
                        len: overlap,
                        cut_point: overlap / 2,
                    });
                    lane.segs.push(ExportSeg::Clip {
                        len: clip.rec_len,
                        start: clip.src_start,
                        master_mob: clip.master_mob,
                        slot_id: clip.slot_id,
                    });
                    lane.cursor = clip.rec_start.saturating_add(clip.rec_len).min(MAX_UNITS);
                    lane.last_clip_len = clip.rec_len;
                    placed = true;
                    break;
                }
            }
        }
        if !placed {
            let mut segs = Vec::new();
            if clip.rec_start > 0 {
                segs.push(ExportSeg::Filler(clip.rec_start));
            }
            segs.push(ExportSeg::Clip {
                len: clip.rec_len,
                start: clip.src_start,
                master_mob: clip.master_mob,
                slot_id: clip.slot_id,
            });
            lanes.push(Lane {
                cursor: clip.rec_start.saturating_add(clip.rec_len).min(MAX_UNITS),
                last_clip_len: clip.rec_len,
                segs,
            });
        }
    }
    lanes.into_iter().map(|l| l.segs).collect()
}

struct ClipRef<'a> {
    datadef_auid: &'a [u8; 16],
    len: i64,
    start: i64,
    source_id: &'a [u8; 32],
    source_slot_id: u32,
}

fn write_source_clip<F: Read + Seek + Write>(
    comp: &mut cfb::CompoundFile<F>,
    path: &str,
    clip: &ClipRef<'_>,
) -> Result<(), NeovegError> {
    create_object(
        comp,
        path,
        CLASS_SOURCE_CLIP,
        &[
            (
                PID_COMPONENT_DATADEF,
                SF_WEAK_REF,
                weak_ref_auid(REF_IDX_DATA_DEFS, PID_DEF_IDENT, clip.datadef_auid),
            ),
            (
                PID_COMPONENT_LENGTH,
                SF_DATA,
                clip.len.to_le_bytes().to_vec(),
            ),
            (PID_CLIP_START, SF_DATA, clip.start.to_le_bytes().to_vec()),
            (PID_SOURCE_ID, SF_DATA, clip.source_id.to_vec()),
            (
                PID_SOURCE_SLOT_ID,
                SF_DATA,
                clip.source_slot_id.to_le_bytes().to_vec(),
            ),
        ],
    )
}

struct SlotDesc<'a> {
    slot_id: u32,
    physical_track: u32,
    name: Option<&'a str>,
    edit_rate: FrameRate,
}

fn write_slot<F: Read + Seek + Write>(
    comp: &mut cfb::CompoundFile<F>,
    slot_path: &str,
    desc: &SlotDesc<'_>,
) -> Result<(), NeovegError> {
    let mut rate_bytes = Vec::with_capacity(8);
    rate_bytes.extend_from_slice(&(desc.edit_rate.num as i32).to_le_bytes());
    rate_bytes.extend_from_slice(&(desc.edit_rate.den as i32).to_le_bytes());
    let mut props = vec![
        (PID_SLOT_ID, SF_DATA, desc.slot_id.to_le_bytes().to_vec()),
        (
            PID_SLOT_PHYSICAL_TRACK,
            SF_DATA,
            desc.physical_track.to_le_bytes().to_vec(),
        ),
        (PID_SLOT_ORIGIN, SF_DATA, 0i64.to_le_bytes().to_vec()),
        (PID_SLOT_EDIT_RATE, SF_DATA, rate_bytes),
        (
            PID_SLOT_SEGMENT,
            SF_STRONG_REF,
            encode_utf16le_nul("Segment-4803"),
        ),
    ];
    if let Some(name) = desc.name.filter(|s| !s.trim().is_empty()) {
        props.push((PID_SLOT_NAME, SF_DATA, encode_utf16le_nul(name)));
    }
    create_object(comp, slot_path, CLASS_TIMELINE_MOB_SLOT, &props)
}

struct ExportMediaPlan {
    name: String,
    url: String,
    rate: FrameRate,
    dur_frames: i64,
    has_pic: bool,
    has_snd: bool,
    source_mob_id: [u8; 32],
    master_mob_id: [u8; 32],
}

struct CompSlotPlan {
    is_pic: bool,
    phys: u32,
    name: String,
    segs: Vec<ExportSeg>,
}

fn export_to_bytes(project: &Project) -> Result<Vec<u8>, NeovegError> {
    let ts = aaf_timestamp();
    let seq_rate = export_slot_rate(project.settings.fps);

    let mut used_ids: HashSet<Id> = HashSet::new();
    for tr in &project.tracks {
        for ev in &tr.events {
            used_ids.insert(ev.media_id);
        }
    }

    let mut media_plans: Vec<ExportMediaPlan> = Vec::new();
    let mut master_by_media: HashMap<Id, ([u8; 32], bool, bool)> = HashMap::new();

    for (idx, m) in project.media.iter().enumerate() {
        let Some(raw_path) = export_media_path(m) else {
            continue;
        };
        if !used_ids.contains(&m.id) && project.tracks.iter().any(|t| !t.events.is_empty()) {
            continue;
        }
        let rate = export_slot_rate(m.fps.unwrap_or(seq_rate));
        let mut dur_frames = m
            .duration
            .map(|d| rate.ticks_to_frame_round(d.max(Ticks::ZERO)))
            .unwrap_or(0)
            .clamp(0, MAX_UNITS);
        for tr in &project.tracks {
            for ev in &tr.events {
                if ev.media_id == m.id {
                    let needed = rate
                        .ticks_to_frame_round(
                            ev.offset.max(Ticks::ZERO) + ev.length.max(Ticks::ZERO),
                        )
                        .clamp(0, MAX_UNITS);
                    dur_frames = dur_frames.max(needed);
                }
            }
        }
        dur_frames = dur_frames.max(1);
        let has_pic = m.has_video || !m.has_audio;
        let has_snd = m.has_audio;
        let source_mob_id = make_mob_id(b"SRC", m.id, idx as u64 + 1);
        let master_mob_id = make_mob_id(b"MST", m.id, idx as u64 + 1);
        master_by_media.insert(m.id, (master_mob_id, has_pic, has_snd));
        media_plans.push(ExportMediaPlan {
            name: if m.name.is_empty() {
                format!("Midia {}", idx + 1)
            } else {
                m.name.clone()
            },
            url: encode_target_url(&raw_path),
            rate,
            dur_frames,
            has_pic,
            has_snd,
            source_mob_id,
            master_mob_id,
        });
    }

    // Camadas de vídeo de baixo para cima (`rev()`), seguidas das trilhas de áudio.
    let mut comp_slots: Vec<CompSlotPlan> = Vec::new();
    let mut remaining_clips = MAX_AAF_CLIPS;
    let video_tracks: Vec<&Track> = project
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Video && !t.is_adjustment())
        .collect();
    let mut phys_v: u32 = 0;
    for tr in video_tracks.iter().rev() {
        let lanes = build_track_lanes(tr, seq_rate, &master_by_media, &mut remaining_clips);
        if lanes.is_empty() {
            phys_v = phys_v.saturating_add(1);
            comp_slots.push(CompSlotPlan {
                is_pic: true,
                phys: phys_v,
                name: tr.name.clone(),
                segs: Vec::new(),
            });
        } else {
            for segs in lanes {
                phys_v = phys_v.saturating_add(1);
                comp_slots.push(CompSlotPlan {
                    is_pic: true,
                    phys: phys_v,
                    name: tr.name.clone(),
                    segs,
                });
            }
        }
    }
    let mut phys_a: u32 = 0;
    for tr in project.tracks.iter().filter(|t| t.kind == TrackKind::Audio) {
        let lanes = build_track_lanes(tr, seq_rate, &master_by_media, &mut remaining_clips);
        if lanes.is_empty() {
            phys_a = phys_a.saturating_add(1);
            comp_slots.push(CompSlotPlan {
                is_pic: false,
                phys: phys_a,
                name: tr.name.clone(),
                segs: Vec::new(),
            });
        } else {
            for segs in lanes {
                phys_a = phys_a.saturating_add(1);
                comp_slots.push(CompSlotPlan {
                    is_pic: false,
                    phys: phys_a,
                    name: tr.name.clone(),
                    segs,
                });
            }
        }
    }

    let has_video_dissolve = comp_slots.iter().any(|slot| {
        slot.segs
            .iter()
            .any(|seg| matches!(seg, ExportSeg::VideoDissolve { .. }))
    });

    let comp_mob_id = make_mob_id(
        b"CMP",
        project.tracks.len() as u64,
        media_plans.len() as u64,
    );
    let comp_mob_name = if project.notes.trim().is_empty() {
        "Neoveg Timeline".to_string()
    } else {
        project.notes.clone()
    };

    let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new()))
        .map_err(|e| err(format!("falha ao inicializar contêiner AAF: {e}")))?;
    comp.set_storage_clsid("/", AAF_SIGNATURE_SSBINARY)
        .map_err(|e| err(format!("falha ao gravar CLSID raiz AAF: {e}")))?;
    write_properties(
        &mut comp,
        "/",
        &[
            (
                PID_ROOT_METADICT,
                SF_STRONG_REF,
                encode_utf16le_nul("MetaDictionary-1"),
            ),
            (
                PID_ROOT_HEADER,
                SF_STRONG_REF,
                encode_utf16le_nul("Header-2"),
            ),
        ],
    )?;
    write_referenced_properties(&mut comp)?;
    crate::aaf_metadict::write_meta_dictionary(&mut comp)?;

    // /Header-2
    let hdr = "/Header-2";
    create_object(
        &mut comp,
        hdr,
        CLASS_HEADER,
        &[
            (
                PID_HEADER_DICTIONARY,
                SF_STRONG_REF,
                encode_utf16le_nul("Dictionary-3b04"),
            ),
            (
                PID_HEADER_CONTENT,
                SF_STRONG_REF,
                encode_utf16le_nul("Content-3b03"),
            ),
            (
                PID_HEADER_OP_PATTERN,
                SF_DATA,
                OP_PATTERN_EDIT_PROTOCOL.to_vec(),
            ),
            (
                PID_HEADER_MODEL_VERSION,
                SF_DATA,
                1u32.to_le_bytes().to_vec(),
            ),
            (PID_HEADER_VERSION, SF_DATA, vec![1u8, 2u8]),
            (
                PID_HEADER_IDENTIFICATION_LIST,
                SF_STRONG_REF_VECTOR,
                encode_utf16le_nul("Identifi-ionList-3b06"),
            ),
            (PID_HEADER_LAST_MODIFIED, SF_DATA, ts.to_vec()),
            (
                PID_HEADER_BYTE_ORDER,
                SF_DATA,
                0x4949u16.to_le_bytes().to_vec(),
            ),
        ],
    )?;

    let id_dir = "/Header-2/Identifi-ionList-3b06{0}";
    create_object(
        &mut comp,
        id_dir,
        CLASS_IDENTIFICATION,
        &[
            (PID_IDENT_COMPANY, SF_DATA, encode_utf16le_nul("Neoveg")),
            (PID_IDENT_PRODUCT, SF_DATA, encode_utf16le_nul("Neoveg")),
            (
                PID_IDENT_VERSION_STR,
                SF_DATA,
                encode_utf16le_nul(env!("CARGO_PKG_VERSION")),
            ),
            (
                PID_IDENT_PRODUCT_ID,
                SF_DATA,
                make_uuid_bytes("neoveg-aaf-product").to_vec(),
            ),
            (PID_IDENT_DATE, SF_DATA, ts.to_vec()),
            (
                PID_IDENT_PLATFORM,
                SF_DATA,
                encode_utf16le_nul(std::env::consts::OS),
            ),
            (
                PID_IDENT_GENERATION,
                SF_DATA,
                make_uuid_bytes(&comp_mob_name).to_vec(),
            ),
        ],
    )?;
    write_vector_index(&mut comp, hdr, "Identifi-ionList-3b06", 1)?;

    // /Header-2/Dictionary-3b04
    let dict = "/Header-2/Dictionary-3b04";
    create_object(
        &mut comp,
        dict,
        CLASS_DICTIONARY,
        &[
            (
                PID_DICT_DATA_DEFS,
                SF_STRONG_REF_SET,
                encode_utf16le_nul("DataDefinitions-2605"),
            ),
            (
                PID_DICT_OP_DEFS,
                SF_STRONG_REF_SET,
                encode_utf16le_nul("Operatio-nitions-2603"),
            ),
        ],
    )?;

    let data_defs: [(&[u8; 16], &str, &str); 2] = [
        (&DD_PICTURE, "DataDef_Picture", "Picture data"),
        (&DD_SOUND, "DataDef_Sound", "Sound data"),
    ];
    let mut dd_entries: Vec<SetIndexEntry<'_>> = Vec::with_capacity(data_defs.len());
    for (i, &(auid, name, desc)) in data_defs.iter().enumerate() {
        let p = format!("{dict}/DataDefinitions-2605{{{i:x}}}");
        create_object(
            &mut comp,
            &p,
            CLASS_DATA_DEFINITION,
            &[
                (PID_DEF_IDENT, SF_DATA, auid.to_vec()),
                (PID_DEF_NAME, SF_DATA, encode_utf16le_nul(name)),
                (PID_DEF_DESC, SF_DATA, encode_utf16le_nul(desc)),
            ],
        )?;
        dd_entries.push(SetIndexEntry {
            local_key: i as u32,
            ref_count: 1,
            key: auid,
        });
    }
    write_set_index(
        &mut comp,
        dict,
        "DataDefinitions-2605",
        PID_DEF_IDENT,
        16,
        &mut dd_entries,
    )?;

    let mut op_entries: Vec<SetIndexEntry<'_>> = Vec::new();
    if has_video_dissolve {
        let p = format!("{dict}/Operatio-nitions-2603{{0}}");
        create_object(
            &mut comp,
            &p,
            CLASS_OPERATION_DEFINITION,
            &[
                (PID_DEF_IDENT, SF_DATA, OP_VIDEO_DISSOLVE.to_vec()),
                (PID_DEF_NAME, SF_DATA, encode_utf16le_nul("Video Dissolve")),
                (PID_DEF_DESC, SF_DATA, encode_utf16le_nul("Video Dissolve")),
                (
                    PID_OPDEF_DATADEF,
                    SF_WEAK_REF,
                    weak_ref_auid(REF_IDX_DATA_DEFS, PID_DEF_IDENT, &DD_PICTURE),
                ),
                (PID_OPDEF_NUM_INPUTS, SF_DATA, 2i32.to_le_bytes().to_vec()),
            ],
        )?;
        op_entries.push(SetIndexEntry {
            local_key: 0,
            ref_count: 1,
            key: &OP_VIDEO_DISSOLVE,
        });
    }
    write_set_index(
        &mut comp,
        dict,
        "Operatio-nitions-2603",
        PID_DEF_IDENT,
        16,
        &mut op_entries,
    )?;

    // /Header-2/Content-3b03
    let content = "/Header-2/Content-3b03";
    create_object(
        &mut comp,
        content,
        CLASS_CONTENT_STORAGE,
        &[(
            PID_CONTENT_MOBS,
            SF_STRONG_REF_SET,
            encode_utf16le_nul("Mobs-1901"),
        )],
    )?;

    let mut mob_entries: Vec<(u32, [u8; 32])> = Vec::new();
    let mut mob_idx: u32 = 0;

    for mp in &media_plans {
        let src_lk = mob_idx;
        mob_idx += 1;
        let src_path = format!("{content}/Mobs-1901{{{src_lk:x}}}");
        mob_entries.push((src_lk, mp.source_mob_id));
        create_object(
            &mut comp,
            &src_path,
            CLASS_SOURCE_MOB,
            &[
                (PID_MOB_ID, SF_DATA, mp.source_mob_id.to_vec()),
                (PID_MOB_NAME, SF_DATA, encode_utf16le_nul(&mp.name)),
                (PID_MOB_LAST_MODIFIED, SF_DATA, ts.to_vec()),
                (PID_MOB_CREATION_TIME, SF_DATA, ts.to_vec()),
                (
                    PID_MOB_SLOTS,
                    SF_STRONG_REF_VECTOR,
                    encode_utf16le_nul("Slots-4403"),
                ),
                (
                    PID_SOURCE_MOB_DESCRIPTOR,
                    SF_STRONG_REF,
                    encode_utf16le_nul("EssenceDescription-4701"),
                ),
            ],
        )?;
        let mut src_slots: Vec<(u32, &[u8; 16])> = Vec::new();
        if mp.has_pic {
            src_slots.push((1, &DD_PICTURE));
        }
        if mp.has_snd {
            src_slots.push((2, &DD_SOUND));
        }
        let zero_mob = [0u8; 32];
        for (si, &(slot_id, dd_auid)) in src_slots.iter().enumerate() {
            let sp = format!("{src_path}/Slots-4403{{{si:x}}}");
            write_slot(
                &mut comp,
                &sp,
                &SlotDesc {
                    slot_id,
                    physical_track: 0,
                    name: None,
                    edit_rate: mp.rate,
                },
            )?;
            let segp = format!("{sp}/Segment-4803");
            write_source_clip(
                &mut comp,
                &segp,
                &ClipRef {
                    datadef_auid: dd_auid,
                    len: mp.dur_frames,
                    start: 0,
                    source_id: &zero_mob,
                    source_slot_id: 0,
                },
            )?;
        }
        write_vector_index(&mut comp, &src_path, "Slots-4403", src_slots.len())?;

        let desc_path = format!("{src_path}/EssenceDescription-4701");
        create_object(
            &mut comp,
            &desc_path,
            CLASS_IMPORT_DESCRIPTOR,
            &[(
                PID_DESCRIPTOR_LOCATOR,
                SF_STRONG_REF_VECTOR,
                encode_utf16le_nul("Locator-2f01"),
            )],
        )?;
        let loc_path = format!("{desc_path}/Locator-2f01{{0}}");
        create_object(
            &mut comp,
            &loc_path,
            CLASS_NETWORK_LOCATOR,
            &[(PID_LOCATOR_URL, SF_DATA, encode_utf16le_nul(&mp.url))],
        )?;
        write_vector_index(&mut comp, &desc_path, "Locator-2f01", 1)?;

        let mst_lk = mob_idx;
        mob_idx += 1;
        let mst_path = format!("{content}/Mobs-1901{{{mst_lk:x}}}");
        mob_entries.push((mst_lk, mp.master_mob_id));
        create_object(
            &mut comp,
            &mst_path,
            CLASS_MASTER_MOB,
            &[
                (PID_MOB_ID, SF_DATA, mp.master_mob_id.to_vec()),
                (PID_MOB_NAME, SF_DATA, encode_utf16le_nul(&mp.name)),
                (PID_MOB_LAST_MODIFIED, SF_DATA, ts.to_vec()),
                (PID_MOB_CREATION_TIME, SF_DATA, ts.to_vec()),
                (
                    PID_MOB_SLOTS,
                    SF_STRONG_REF_VECTOR,
                    encode_utf16le_nul("Slots-4403"),
                ),
            ],
        )?;
        for (si, &(slot_id, dd_auid)) in src_slots.iter().enumerate() {
            let sp = format!("{mst_path}/Slots-4403{{{si:x}}}");
            write_slot(
                &mut comp,
                &sp,
                &SlotDesc {
                    slot_id,
                    physical_track: 0,
                    name: None,
                    edit_rate: mp.rate,
                },
            )?;
            let segp = format!("{sp}/Segment-4803");
            write_source_clip(
                &mut comp,
                &segp,
                &ClipRef {
                    datadef_auid: dd_auid,
                    len: mp.dur_frames,
                    start: 0,
                    source_id: &mp.source_mob_id,
                    source_slot_id: slot_id,
                },
            )?;
        }
        write_vector_index(&mut comp, &mst_path, "Slots-4403", src_slots.len())?;
    }

    // CompositionMob
    let cmp_lk = mob_idx;
    let cmp_path = format!("{content}/Mobs-1901{{{cmp_lk:x}}}");
    mob_entries.push((cmp_lk, comp_mob_id));
    create_object(
        &mut comp,
        &cmp_path,
        CLASS_COMPOSITION_MOB,
        &[
            (PID_MOB_ID, SF_DATA, comp_mob_id.to_vec()),
            (PID_MOB_NAME, SF_DATA, encode_utf16le_nul(&comp_mob_name)),
            (PID_MOB_LAST_MODIFIED, SF_DATA, ts.to_vec()),
            (PID_MOB_CREATION_TIME, SF_DATA, ts.to_vec()),
            (
                PID_MOB_SLOTS,
                SF_STRONG_REF_VECTOR,
                encode_utf16le_nul("Slots-4403"),
            ),
        ],
    )?;

    for (si, slot) in comp_slots.iter().enumerate() {
        let slot_id = (si + 1) as u32;
        let dd_auid = if slot.is_pic { &DD_PICTURE } else { &DD_SOUND };
        let sp = format!("{cmp_path}/Slots-4403{{{si:x}}}");
        write_slot(
            &mut comp,
            &sp,
            &SlotDesc {
                slot_id,
                physical_track: slot.phys,
                name: Some(&slot.name),
                edit_rate: seq_rate,
            },
        )?;

        let mut total_len = 0i64;
        for seg in &slot.segs {
            match seg {
                ExportSeg::Filler(l) | ExportSeg::Clip { len: l, .. } => {
                    total_len = total_len.saturating_add(*l).min(MAX_UNITS);
                }
                ExportSeg::VideoDissolve { len: l, .. } => {
                    total_len = total_len.saturating_sub(*l).max(0);
                }
            }
        }
        let seq_path = format!("{sp}/Segment-4803");
        create_object(
            &mut comp,
            &seq_path,
            CLASS_SEQUENCE,
            &[
                (
                    PID_COMPONENT_DATADEF,
                    SF_WEAK_REF,
                    weak_ref_auid(REF_IDX_DATA_DEFS, PID_DEF_IDENT, dd_auid),
                ),
                (
                    PID_COMPONENT_LENGTH,
                    SF_DATA,
                    total_len.to_le_bytes().to_vec(),
                ),
                (
                    PID_SEQUENCE_COMPONENTS,
                    SF_STRONG_REF_VECTOR,
                    encode_utf16le_nul("Components-1001"),
                ),
            ],
        )?;

        for (ci, seg) in slot.segs.iter().enumerate() {
            let cp = format!("{seq_path}/Components-1001{{{ci:x}}}");
            match seg {
                ExportSeg::Filler(len) => {
                    create_object(
                        &mut comp,
                        &cp,
                        CLASS_FILLER,
                        &[
                            (
                                PID_COMPONENT_DATADEF,
                                SF_WEAK_REF,
                                weak_ref_auid(REF_IDX_DATA_DEFS, PID_DEF_IDENT, dd_auid),
                            ),
                            (PID_COMPONENT_LENGTH, SF_DATA, len.to_le_bytes().to_vec()),
                        ],
                    )?;
                }
                ExportSeg::VideoDissolve { len, cut_point } => {
                    create_object(
                        &mut comp,
                        &cp,
                        CLASS_TRANSITION,
                        &[
                            (
                                PID_COMPONENT_DATADEF,
                                SF_WEAK_REF,
                                weak_ref_auid(REF_IDX_DATA_DEFS, PID_DEF_IDENT, dd_auid),
                            ),
                            (PID_COMPONENT_LENGTH, SF_DATA, len.to_le_bytes().to_vec()),
                            (
                                PID_TRANSITION_OP_GROUP,
                                SF_STRONG_REF,
                                encode_utf16le_nul("OperationGroup-1801"),
                            ),
                            (
                                PID_TRANSITION_CUT_POINT,
                                SF_DATA,
                                cut_point.to_le_bytes().to_vec(),
                            ),
                        ],
                    )?;
                    let og_path = format!("{cp}/OperationGroup-1801");
                    create_object(
                        &mut comp,
                        &og_path,
                        CLASS_OPERATION_GROUP,
                        &[
                            (
                                PID_COMPONENT_DATADEF,
                                SF_WEAK_REF,
                                weak_ref_auid(REF_IDX_DATA_DEFS, PID_DEF_IDENT, dd_auid),
                            ),
                            (PID_COMPONENT_LENGTH, SF_DATA, len.to_le_bytes().to_vec()),
                            (
                                PID_OPGROUP_OP_DEF,
                                SF_WEAK_REF,
                                weak_ref_auid(REF_IDX_OP_DEFS, PID_DEF_IDENT, &OP_VIDEO_DISSOLVE),
                            ),
                        ],
                    )?;
                }
                ExportSeg::Clip {
                    len,
                    start,
                    master_mob,
                    slot_id,
                } => {
                    write_source_clip(
                        &mut comp,
                        &cp,
                        &ClipRef {
                            datadef_auid: dd_auid,
                            len: *len,
                            start: *start,
                            source_id: master_mob,
                            source_slot_id: *slot_id,
                        },
                    )?;
                }
            }
        }
        write_vector_index(&mut comp, &seq_path, "Components-1001", slot.segs.len())?;
    }
    write_vector_index(&mut comp, &cmp_path, "Slots-4403", comp_slots.len())?;

    let mut mob_idx_entries: Vec<SetIndexEntry<'_>> = mob_entries
        .iter()
        .map(|(lk, id)| SetIndexEntry {
            local_key: *lk,
            ref_count: 1,
            key: id.as_slice(),
        })
        .collect();
    write_set_index(
        &mut comp,
        content,
        "Mobs-1901",
        PID_MOB_ID,
        32,
        &mut mob_idx_entries,
    )?;

    comp.flush()
        .map_err(|e| err(format!("falha ao finalizar contêiner AAF: {e}")))?;
    Ok(comp.into_inner().into_inner())
}

/// Exporta o projeto para um contêiner AAF em memória.
pub fn export_aaf_bytes(project: &Project) -> Result<Vec<u8>, NeovegError> {
    guarded(|| export_to_bytes(project))
}

/// Exporta o projeto para um arquivo `.aaf` (escrita atômica em `.part`).
pub fn export_aaf_file(project: &Project, path: &Path) -> Result<(), NeovegError> {
    let bytes = export_aaf_bytes(project)?;
    let part = path.with_extension("aaf.part");
    std::fs::write(&part, &bytes)
        .map_err(|e| NeovegError::Io(format!("não foi possível gravar {}: {e}", part.display())))?;
    if let Err(e) = std::fs::rename(&part, path) {
        let _ = std::fs::remove_file(&part);
        return Err(NeovegError::Io(format!(
            "não foi possível concluir {}: {e}",
            path.display()
        )));
    }
    Ok(())
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

    #[test]
    fn ms02_signature_metadict_sorted_indices_and_audio_parallel_slots() {
        let mut p = Project::new_default();
        let fps = FrameRate::FPS_25;
        p.settings.fps = fps;
        let m = MediaItem {
            id: new_id(),
            name: "audio_v".into(),
            source: MediaSource::File {
                path: "C:/midia/a.mov".into(),
            },
            duration: Some(fps.frame_to_ticks(200)),
            has_video: true,
            has_audio: true,
            width: 1920,
            height: 1080,
            fps: Some(fps),
            info: String::new(),
            proxy_path: None,
        };
        p.media.push(m.clone());
        p.tracks[0]
            .events
            .push(Event::new(&m, Ticks::ZERO, fps.frame_to_ticks(40)));
        // Sobreposição de áudio mesmo marcada com transição vai para slot paralelo (sem AUID inventado).
        let mut a2 = Event::new(&m, fps.frame_to_ticks(10), fps.frame_to_ticks(40));
        a2.transition = Some(TransitionSpec {
            kind: TransitionKind::Dissolve,
            softness: 0.5,
        });
        p.tracks[1].events = vec![Event::new(&m, Ticks::ZERO, fps.frame_to_ticks(40)), a2];

        let bytes = export_aaf_bytes(&p).expect("exporta AAF");
        let mut comp = cfb::CompoundFile::open(Cursor::new(&bytes)).expect("abre CFB");
        let root_entry = comp.entry("/").expect("raiz");
        assert_eq!(root_entry.clsid(), &AAF_SIGNATURE_SSBINARY);

        // MetaDictionary populado conforme MS-01 §5 e índices de conjuntos ordenados (MS-02 §1.2.1.6.4).
        for (idx_path, key_len) in [
            ("/MetaDictionary-1/ClassDefinitions-3 index", 16usize),
            ("/MetaDictionary-1/TypeDefinitions-4 index", 16usize),
            (
                "/Header-2/Dictionary-3b04/DataDefinitions-2605 index",
                16usize,
            ),
            ("/Header-2/Content-3b03/Mobs-1901 index", 32usize),
        ] {
            let mut s = comp.open_stream(idx_path).expect(idx_path);
            let mut raw = Vec::new();
            s.read_to_end(&mut raw).expect("lê índice");
            let cnt = u32::from_le_bytes(raw[..4].try_into().expect("cnt")) as usize;
            assert!(cnt > 0, "{idx_path} não pode estar vazio");
            let keys: Vec<&[u8]> = raw[15..]
                .chunks_exact(8 + key_len)
                .map(|c| &c[8..])
                .collect();
            assert_eq!(keys.len(), cnt);
            assert!(
                keys.windows(2).all(|w| w[0] < w[1]),
                "{idx_path} desordenado"
            );
        }

        // Nenhum OperationDefinition inventado quando não há transição de vídeo.
        let mut op_idx = comp
            .open_stream("/Header-2/Dictionary-3b04/Operatio-nitions-2603 index")
            .expect("op index");
        let mut op_raw = Vec::new();
        op_idx.read_to_end(&mut op_raw).expect("lê op index");
        assert_eq!(u32::from_le_bytes(op_raw[..4].try_into().expect("cnt")), 0);

        let (re, warn) = import_aaf_bytes(&bytes).expect("reimporta");
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!(
            re.tracks
                .iter()
                .filter(|t| t.kind == TrackKind::Audio)
                .count(),
            2
        );
    }

    #[test]
    fn roundtrip_video_audio_transitions_and_gaps() {
        let mut p = Project::new_default();
        p.notes = "Projeto AAF Completo".into();
        let fps = FrameRate::FPS_25;
        p.settings.fps = fps;
        p.settings.drop_frame = false;

        let m_av = MediaItem {
            id: new_id(),
            name: "clipe_av".into(),
            source: MediaSource::File {
                path: "C:/midia/cena um.mov".into(),
            },
            duration: Some(fps.frame_to_ticks(300)),
            has_video: true,
            has_audio: true,
            width: 1920,
            height: 1080,
            fps: Some(fps),
            info: String::new(),
            proxy_path: None,
        };
        let m_b = MediaItem {
            id: new_id(),
            name: "clipe_b".into(),
            source: MediaSource::File {
                path: "/mnt/dados/segundo.mp4".into(),
            },
            duration: Some(fps.frame_to_ticks(200)),
            has_video: true,
            has_audio: false,
            width: 1920,
            height: 1080,
            fps: Some(fps),
            info: String::new(),
            proxy_path: None,
        };
        let m_gen = MediaItem {
            id: new_id(),
            name: "gerado".into(),
            source: MediaSource::Generated(crate::model::GenSpec::ColorBars),
            duration: Some(fps.frame_to_ticks(100)),
            has_video: true,
            has_audio: false,
            width: 1920,
            height: 1080,
            fps: Some(fps),
            info: String::new(),
            proxy_path: None,
        };
        p.media = vec![m_av.clone(), m_b.clone(), m_gen.clone()];

        let mut v_top = Track::new(TrackKind::Video, "Sobreposto");
        let ev_top = Event::new(&m_b, fps.frame_to_ticks(15), fps.frame_to_ticks(30));
        v_top.events.push(ev_top);

        let mut v_main = Track::new(TrackKind::Video, "Principal");
        let mut ev1 = Event::new(&m_av, Ticks::ZERO, fps.frame_to_ticks(60));
        ev1.offset = fps.frame_to_ticks(12);
        let mut ev2 = Event::new(&m_b, fps.frame_to_ticks(40), fps.frame_to_ticks(70));
        ev2.transition = Some(TransitionSpec {
            kind: TransitionKind::Dissolve,
            softness: 0.2,
        });
        let ev_g = Event::new(&m_gen, fps.frame_to_ticks(120), fps.frame_to_ticks(25));
        v_main.events = vec![ev1, ev2, ev_g];

        let mut a1 = Track::new(TrackKind::Audio, "Diálogo");
        let ev_a = Event::new(&m_av, fps.frame_to_ticks(10), fps.frame_to_ticks(80));
        a1.events.push(ev_a);

        p.tracks = vec![v_top, v_main, a1];

        let bytes = export_aaf_bytes(&p).expect("exporta AAF");
        let (re, warn) = import_aaf_bytes(&bytes).expect("reimporta AAF");
        assert!(warn.is_empty(), "avisos inesperados: {warn:?}");
        assert_eq!(re.notes, "Projeto AAF Completo");
        assert_eq!(re.settings.fps, fps);

        let v_tracks: Vec<&Track> = re
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Video)
            .collect();
        assert_eq!(v_tracks.len(), 2);
        assert_eq!(v_tracks[0].events.len(), 1);
        assert_eq!(v_tracks[0].events[0].start, fps.frame_to_ticks(15));
        assert_eq!(v_tracks[0].events[0].length, fps.frame_to_ticks(30));

        assert_eq!(v_tracks[1].events.len(), 2);
        assert_eq!(v_tracks[1].events[0].start, Ticks::ZERO);
        assert_eq!(v_tracks[1].events[0].length, fps.frame_to_ticks(60));
        assert_eq!(v_tracks[1].events[0].offset, fps.frame_to_ticks(12));
        assert!(v_tracks[1].events[0].transition.is_none());
        assert_eq!(v_tracks[1].events[1].start, fps.frame_to_ticks(40));
        assert_eq!(v_tracks[1].events[1].length, fps.frame_to_ticks(70));
        assert!(v_tracks[1].events[1].transition.is_some());

        let a_tracks: Vec<&Track> = re
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Audio)
            .collect();
        assert_eq!(a_tracks.len(), 1);
        assert_eq!(a_tracks[0].events.len(), 1);
        assert_eq!(a_tracks[0].events[0].start, fps.frame_to_ticks(10));
        assert_eq!(a_tracks[0].events[0].length, fps.frame_to_ticks(80));

        assert_eq!(re.media.len(), 2);
        let re_av = re
            .media
            .iter()
            .find(|m| m.name == "clipe_av")
            .expect("clipe_av");
        assert!(re_av.has_video && re_av.has_audio);
        assert!(
            matches!(&re_av.source, MediaSource::File { path } if path == "C:/midia/cena um.mov")
        );
    }

    #[test]
    fn exports_empty_and_overlapping_projects_without_panic() {
        let mut empty = Project::default();
        empty.tracks.clear();
        let bytes = export_aaf_bytes(&empty).expect("projeto vazio exporta");
        let (re, _) = import_aaf_bytes(&bytes).expect("projeto vazio reimporta");
        assert_eq!(re.notes, "Neoveg Timeline");
        assert!(!re.tracks.is_empty());

        let mut p = Project::new_default();
        p.settings.fps = FrameRate::FPS_29_97;
        p.settings.drop_frame = true;
        let m = MediaItem {
            id: new_id(),
            name: "sobreposto".into(),
            source: MediaSource::File {
                path: "D:/take.mov".into(),
            },
            duration: Some(FrameRate::FPS_29_97.frame_to_ticks(120)),
            has_video: true,
            has_audio: true,
            width: 1920,
            height: 1080,
            fps: Some(FrameRate::FPS_29_97),
            info: String::new(),
            proxy_path: None,
        };
        p.media.push(m.clone());
        let e1 = Event::new(&m, Ticks::ZERO, FrameRate::FPS_29_97.frame_to_ticks(60));
        let e2 = Event::new(
            &m,
            FrameRate::FPS_29_97.frame_to_ticks(20),
            FrameRate::FPS_29_97.frame_to_ticks(60),
        );
        p.tracks[0].events = vec![e1, e2];
        let b2 = export_aaf_bytes(&p).expect("sobreposto exporta");
        let (re2, warn2) = import_aaf_bytes(&b2).expect("sobreposto reimporta");
        assert!(warn2.is_empty(), "{warn2:?}");
        assert_eq!(
            re2.tracks
                .iter()
                .filter(|t| t.kind == TrackKind::Video)
                .count(),
            2
        );
        assert_eq!(re2.settings.fps, FrameRate::FPS_29_97);
        assert!(re2.settings.drop_frame);
    }

    #[test]
    fn headless_export_aaf_command_and_fuzz_export_mutations() {
        let tmp = std::env::temp_dir().join(format!("nvov_aaf_cmd_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).expect("mkdir");
        let proj_path = tmp.join("corte.neoveg");
        let aaf_path = tmp.join("corte.aaf");

        let mut p = Project::new_default();
        let fps = FrameRate::FPS_24;
        p.settings.fps = fps;
        let m = MediaItem {
            id: new_id(),
            name: "plano_01".into(),
            source: MediaSource::File {
                path: "/tmp/plano_01.mov".into(),
            },
            duration: Some(fps.frame_to_ticks(200)),
            has_video: true,
            has_audio: false,
            width: 1920,
            height: 1080,
            fps: Some(fps),
            info: String::new(),
            proxy_path: None,
        };
        p.media.push(m.clone());
        p.tracks[0]
            .events
            .push(Event::new(&m, Ticks::ZERO, fps.frame_to_ticks(48)));

        let mut session = crate::session::EditorSession::new(p);
        session.path = Some(proj_path);
        let mut host = crate::commands::HeadlessHost;
        crate::commands::run_command("file.export_aaf", &mut session, &mut host)
            .expect("comando headless file.export_aaf");
        assert!(aaf_path.is_file());

        let base = std::fs::read(&aaf_path).expect("lê exportação");
        let (imported, warn) = import_aaf_bytes(&base).expect("reimporta");
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!(imported.tracks[0].events[0].length, fps.frame_to_ticks(48));

        let mut seed: u64 = 0xEAA0_1234;
        for i in 0..80 {
            let mut b = base.clone();
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let pos = (seed as usize) % b.len().max(1);
            if i % 2 == 0 {
                b.truncate(pos);
            } else if let Some(x) = b.get_mut(pos) {
                *x ^= 0x5A;
            }
            let r = std::panic::catch_unwind(|| {
                let _ = import_aaf_bytes(&b);
            });
            assert!(r.is_ok(), "pânico na mutação de exportação {i}");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn oracle_pyaaf2_opens_exported_aaf_when_available() {
        let probe = std::process::Command::new("python")
            .args(["-c", "import aaf2"])
            .output();
        if !probe.is_ok_and(|o| o.status.success()) {
            return;
        }
        let mut p = Project::new_default();
        p.notes = "Oraculo AAF".into();
        let fps = FrameRate::FPS_25;
        p.settings.fps = fps;
        let m1 = MediaItem {
            id: new_id(),
            name: "clip_a".into(),
            source: MediaSource::File {
                path: "C:/midia/clip_a.mov".into(),
            },
            duration: Some(fps.frame_to_ticks(200)),
            has_video: true,
            has_audio: true,
            width: 1920,
            height: 1080,
            fps: Some(fps),
            info: String::new(),
            proxy_path: None,
        };
        let m2 = MediaItem {
            id: new_id(),
            name: "clip_b".into(),
            source: MediaSource::File {
                path: "C:/midia/clip_b.mov".into(),
            },
            duration: Some(fps.frame_to_ticks(200)),
            has_video: true,
            has_audio: false,
            width: 1920,
            height: 1080,
            fps: Some(fps),
            info: String::new(),
            proxy_path: None,
        };
        p.media = vec![m1.clone(), m2.clone()];
        let mut e1 = Event::new(&m1, Ticks::ZERO, fps.frame_to_ticks(60));
        e1.offset = fps.frame_to_ticks(10);
        let mut e2 = Event::new(&m2, fps.frame_to_ticks(40), fps.frame_to_ticks(70));
        e2.transition = Some(TransitionSpec {
            kind: TransitionKind::Dissolve,
            softness: 0.0,
        });
        p.tracks[0].events = vec![e1, e2];
        p.tracks[1]
            .events
            .push(Event::new(&m1, Ticks::ZERO, fps.frame_to_ticks(60)));

        let path = std::env::temp_dir().join(format!("nvov_aaf_oracle_{}.aaf", std::process::id()));
        export_aaf_file(&p, &path).expect("exporta AAF");

        let script = concat!(
            "import sys, aaf2\n",
            "with aaf2.open(sys.argv[1], 'r') as f:\n",
            "    assert len(f.metadict['ClassDefinitions'].value) >= 40\n",
            "    assert len(f.metadict['TypeDefinitions'].value) >= 50\n",
            "    comps = list(f.content.compositionmobs())\n",
            "    assert len(comps) == 1, len(comps)\n",
            "    assert comps[0].name == 'Oraculo AAF', comps[0].name\n",
            "    slots = list(comps[0].slots)\n",
            "    assert len(slots) == 2, len(slots)\n",
            "    assert slots[0]['PhysicalTrackNumber'].value == 1\n",
            "    assert slots[0].name == 'Vídeo 1', slots[0].name\n",
            "    v_comps = list(slots[0].segment.components)\n",
            "    assert len(v_comps) == 3, v_comps\n",
            "    assert v_comps[1]['OperationGroup'].value['Operation'].value['Name'].value == 'Video Dissolve'\n",
            "    srcs = list(f.content.sourcemobs())\n",
            "    assert len(srcs) == 2, len(srcs)\n",
            "    urls = [l['URLString'].value for s in srcs for l in s.descriptor.locator]\n",
            "    assert any('clip_a.mov' in u for u in urls), urls\n",
            "print('PYAAF2_OK')\n",
        );
        let check = std::process::Command::new("python")
            .args(["-c", script, &path.to_string_lossy()])
            .output()
            .expect("executa python");
        let _ = std::fs::remove_file(&path);
        assert!(
            check.status.success(),
            "pyaaf2 falhou:\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&check.stdout),
            String::from_utf8_lossy(&check.stderr)
        );
    }
}
