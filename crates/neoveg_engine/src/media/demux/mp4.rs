//! Demuxer ISO Base Media File Format (MP4/MOV/M4V/M4A) em Rust puro.
//!
//! Implementado em Sala Limpa a partir das especificações públicas:
//! - ISO/IEC 14496-12 (*ISO Base Media File Format*)
//! - Apple *QuickTime File Format Specification*

use super::{
    rational_fps, read_exact_at, scale_to_ticks, ContainerKind, DemuxIndex, DemuxSample,
    DemuxTrack, TrackKind, MAX_CODEC_EXTRA, MAX_CONTAINER_DEPTH, MAX_METADATA_READ, MAX_SAMPLES,
    MAX_TRACKS,
};
use crate::time::{FrameRate, Ticks};
use anyhow::{bail, Result};
use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};

const MAX_BOX_COUNT: usize = 65_536;
const MAX_TABLE_RUNS: usize = MAX_SAMPLES;

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn rem(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn skip(&mut self, n: usize) -> Option<()> {
        let end = self.pos.checked_add(n)?;
        if end > self.data.len() {
            return None;
        }
        self.pos = end;
        Some(())
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let out = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(out)
    }

    fn u8(&mut self) -> Option<u8> {
        let &b = self.data.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    fn u16(&mut self) -> Option<u16> {
        let s = self.take(2)?;
        Some(u16::from_be_bytes([s[0], s[1]]))
    }

    fn u32(&mut self) -> Option<u32> {
        let s = self.take(4)?;
        Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }

    fn i32(&mut self) -> Option<i32> {
        self.u32().map(|v| v as i32)
    }

    fn u64(&mut self) -> Option<u64> {
        let s = self.take(8)?;
        Some(u64::from_be_bytes([
            s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
        ]))
    }

    fn i64(&mut self) -> Option<i64> {
        self.u64().map(|v| v as i64)
    }

    fn fourcc(&mut self) -> Option<[u8; 4]> {
        let s = self.take(4)?;
        Some([s[0], s[1], s[2], s[3]])
    }
}

struct MemBox<'a> {
    fourcc: [u8; 4],
    data: &'a [u8],
}

fn iter_boxes(buf: &[u8], depth: usize) -> Result<Vec<MemBox<'_>>> {
    if depth >= MAX_CONTAINER_DEPTH {
        bail!("profundidade máxima de caixas MP4 excedida");
    }
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut count = 0usize;
    while pos + 8 <= buf.len() {
        if count >= MAX_BOX_COUNT {
            bail!("número excessivo de subcaixas MP4");
        }
        count += 1;
        let sz_bytes = buf.get(pos..pos + 4).unwrap_or(&[]);
        let cc_bytes = buf.get(pos + 4..pos + 8).unwrap_or(&[]);
        if sz_bytes.len() < 4 || cc_bytes.len() < 4 {
            break;
        }
        let sz32 = u32::from_be_bytes([sz_bytes[0], sz_bytes[1], sz_bytes[2], sz_bytes[3]]);
        let fourcc = [cc_bytes[0], cc_bytes[1], cc_bytes[2], cc_bytes[3]];
        let (hdr, total): (usize, usize) = if sz32 == 1 {
            let ext = buf.get(pos + 8..pos + 16);
            let Some(e) = ext else {
                bail!("cabeçalho de caixa MP4 estendida truncado");
            };
            let large = u64::from_be_bytes([e[0], e[1], e[2], e[3], e[4], e[5], e[6], e[7]]);
            if large < 16 {
                bail!("tamanho estendido de caixa MP4 inválido ({large})");
            }
            let tot = usize::try_from(large)
                .map_err(|_| anyhow::anyhow!("caixa MP4 excede espaço de endereçamento"))?;
            (16, tot)
        } else if sz32 == 0 {
            (8, buf.len() - pos)
        } else {
            if sz32 < 8 {
                bail!("tamanho de caixa MP4 inválido ({sz32})");
            }
            (8, sz32 as usize)
        };
        let end = pos
            .checked_add(total)
            .filter(|&e| e <= buf.len())
            .ok_or_else(|| anyhow::anyhow!("caixa MP4 excede o limite do contêiner"))?;
        if let Some(payload) = buf.get(pos + hdr..end) {
            out.push(MemBox {
                fourcc,
                data: payload,
            });
        }
        pos = end;
    }
    Ok(out)
}

#[derive(Clone, Default)]
struct TrackDefaults {
    sample_description_index: u32,
    default_sample_duration: u32,
    default_sample_size: u32,
    default_sample_flags: u32,
}

#[derive(Default)]
struct StblData {
    codec: String,
    extra: Vec<u8>,
    width: u32,
    height: u32,
    sample_rate: u32,
    channels: u16,
    stts: Vec<(u32, u32)>,
    ctts: Option<(u8, Vec<(u32, i64)>)>,
    stsc: Vec<(u32, u32)>,
    uniform_size: u32,
    sizes: Vec<u32>,
    chunk_offsets: Vec<u64>,
    sync_samples: Option<HashSet<u32>>,
}

struct ParsedTrack {
    id: u32,
    kind: TrackKind,
    media_timescale: u64,
    media_duration: Option<Ticks>,
    edit_shift_ticks: i128,
    stbl: StblData,
}

fn fourcc_string(cc: [u8; 4]) -> String {
    cc.iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                char::from(b)
            } else {
                '?'
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

fn read_mp4_descr_len(r: &mut Reader<'_>) -> Option<usize> {
    let mut len = 0usize;
    for _ in 0..4 {
        let b = r.u8()?;
        len = (len << 7) | usize::from(b & 0x7f);
        if b & 0x80 == 0 {
            return Some(len);
        }
    }
    Some(len)
}

fn extract_esds_config(payload: &[u8]) -> Vec<u8> {
    if payload.len() < 4 {
        return vec![];
    }
    let mut r = Reader::new(&payload[4..]);
    while r.rem() >= 2 {
        let Some(tag) = r.u8() else {
            break;
        };
        let Some(len) = read_mp4_descr_len(&mut r) else {
            break;
        };
        match tag {
            0x03 => {
                // ES_Descriptor: ES_ID(2) + flags(1)
                if r.skip(2).is_none() {
                    break;
                }
                let Some(flags) = r.u8() else {
                    break;
                };
                if flags & 0x80 != 0 && r.skip(2).is_none() {
                    break;
                }
                if flags & 0x40 != 0 {
                    let Some(ulen) = r.u8() else {
                        break;
                    };
                    if r.skip(usize::from(ulen)).is_none() {
                        break;
                    }
                }
                if flags & 0x20 != 0 && r.skip(2).is_none() {
                    break;
                }
            }
            0x04 => {
                // DecoderConfigDescriptor: objectType(1) + streamType(1) + bufSize(3) + maxBitrate(4) + avgBitrate(4)
                if r.skip(13).is_none() {
                    break;
                }
            }
            0x05 => {
                let capped = len.min(MAX_CODEC_EXTRA).min(r.rem());
                return r.take(capped).unwrap_or(&[]).to_vec();
            }
            _ => {
                let skip = len.min(r.rem());
                let _ = r.skip(skip);
            }
        }
    }
    payload[..payload.len().min(MAX_CODEC_EXTRA)].to_vec()
}

fn find_audio_extra_in_subboxes(buf: &[u8], depth: usize) -> Vec<u8> {
    let Ok(boxes) = iter_boxes(buf, depth) else {
        return vec![];
    };
    for b in boxes {
        match &b.fourcc {
            b"esds" => {
                let cfg = extract_esds_config(b.data);
                if !cfg.is_empty() {
                    return cfg;
                }
            }
            b"alac" | b"dfLa" | b"dac3" | b"dec3" | b"dOps" => {
                return b.data[..b.data.len().min(MAX_CODEC_EXTRA)].to_vec();
            }
            b"wave" => {
                let inner = find_audio_extra_in_subboxes(b.data, depth + 1);
                if !inner.is_empty() {
                    return inner;
                }
            }
            _ => {}
        }
    }
    vec![]
}

fn parse_stsd(data: &[u8], kind: TrackKind, stbl: &mut StblData, depth: usize) -> Result<()> {
    let mut r = Reader::new(data);
    let _vf = r.u32().ok_or_else(|| anyhow::anyhow!("stsd truncado"))?;
    let count = r
        .u32()
        .ok_or_else(|| anyhow::anyhow!("stsd sem contagem"))?;
    if count == 0 {
        return Ok(());
    }
    let entry_len = r
        .u32()
        .ok_or_else(|| anyhow::anyhow!("stsd entrada truncada"))? as usize;
    if entry_len < 16 {
        bail!("entrada stsd inválida ({entry_len})");
    }
    let body = r
        .take(entry_len.saturating_sub(4).min(r.rem()))
        .ok_or_else(|| anyhow::anyhow!("entrada stsd incompleta"))?;
    let mut er = Reader::new(body);
    let cc = er.fourcc().unwrap_or([0; 4]);
    stbl.codec = fourcc_string(cc);
    let _ = er.skip(8); // reserved(6) + data_reference_index(2)

    match kind {
        TrackKind::Video => {
            if er.skip(16).is_some() {
                stbl.width = u32::from(er.u16().unwrap_or(0));
                stbl.height = u32::from(er.u16().unwrap_or(0));
                if er.skip(50).is_some() {
                    let sub_start = er.pos;
                    if let Ok(sub) = iter_boxes(body.get(sub_start..).unwrap_or(&[]), depth + 1) {
                        for b in sub {
                            match &b.fourcc {
                                b"avcC" | b"hvcC" | b"av1C" | b"vpcC" => {
                                    stbl.extra =
                                        b.data[..b.data.len().min(MAX_CODEC_EXTRA)].to_vec();
                                    break;
                                }
                                b"esds" => {
                                    let cfg = extract_esds_config(b.data);
                                    if !cfg.is_empty() {
                                        stbl.extra = cfg;
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        TrackKind::Audio => {
            let ver = er.u16().unwrap_or(0);
            let _ = er.skip(6); // revision(2) + vendor(4)
            let ch = er.u16().unwrap_or(0);
            let _bits = er.u16().unwrap_or(16);
            let _ = er.skip(4); // compression_id(2) + packet_size(2)
            let sr_fp = er.u32().unwrap_or(0);
            stbl.channels = ch;
            stbl.sample_rate = sr_fp >> 16;
            let extra_skip = match ver {
                0 => 0usize,
                1 => 16usize,
                2 => {
                    // SoundSampleEntry v2 (QuickTime): 36 bytes extra após version+...+sr_fp (16 bytes v2 header + f64 sr + u32 ch...)
                    let v2_base = er.pos;
                    if let Some(sr_bytes) = body.get(v2_base + 4..v2_base + 12) {
                        let sr_f = f64::from_be_bytes([
                            sr_bytes[0],
                            sr_bytes[1],
                            sr_bytes[2],
                            sr_bytes[3],
                            sr_bytes[4],
                            sr_bytes[5],
                            sr_bytes[6],
                            sr_bytes[7],
                        ]);
                        if sr_f.is_finite() && sr_f > 0.0 && sr_f <= 1_000_000.0 {
                            stbl.sample_rate = sr_f.round() as u32;
                        }
                    }
                    if let Some(ch_bytes) = body.get(v2_base + 12..v2_base + 16) {
                        let ch32 = u32::from_be_bytes([
                            ch_bytes[0],
                            ch_bytes[1],
                            ch_bytes[2],
                            ch_bytes[3],
                        ]);
                        if ch32 > 0 && ch32 <= 64 {
                            stbl.channels = ch32 as u16;
                        }
                    }
                    36usize
                }
                _ => 0usize,
            };
            if er.skip(extra_skip).is_some() {
                if let Some(rest) = body.get(er.pos..) {
                    stbl.extra = find_audio_extra_in_subboxes(rest, depth + 1);
                }
            }
        }
        TrackKind::Other => {}
    }
    Ok(())
}

fn parse_stbl(data: &[u8], kind: TrackKind, depth: usize) -> Result<StblData> {
    let mut stbl = StblData::default();
    for b in iter_boxes(data, depth)? {
        match &b.fourcc {
            b"stsd" => parse_stsd(b.data, kind, &mut stbl, depth + 1)?,
            b"stts" => {
                let mut r = Reader::new(b.data);
                let _ = r.u32();
                let n = r.u32().unwrap_or(0) as usize;
                if n > MAX_TABLE_RUNS {
                    bail!("stts excede limite de entradas ({n})");
                }
                stbl.stts.reserve(n.min(4096));
                let mut total = 0u64;
                for _ in 0..n {
                    let (Some(sc), Some(sd)) = (r.u32(), r.u32()) else {
                        break;
                    };
                    total = total.saturating_add(u64::from(sc));
                    if total > MAX_SAMPLES as u64 {
                        bail!("stts excede limite máximo de amostras");
                    }
                    stbl.stts.push((sc, sd));
                }
            }
            b"ctts" => {
                let mut r = Reader::new(b.data);
                let Some(vf) = r.u32() else {
                    continue;
                };
                let ver = (vf >> 24) as u8;
                let n = r.u32().unwrap_or(0) as usize;
                if n > MAX_TABLE_RUNS {
                    bail!("ctts excede limite de entradas ({n})");
                }
                let mut entries = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    let Some(sc) = r.u32() else {
                        break;
                    };
                    let off = if ver == 0 {
                        i64::from(r.u32().unwrap_or(0))
                    } else {
                        i64::from(r.i32().unwrap_or(0))
                    };
                    entries.push((sc, off));
                }
                stbl.ctts = Some((ver, entries));
            }
            b"stsc" => {
                let mut r = Reader::new(b.data);
                let _ = r.u32();
                let n = r.u32().unwrap_or(0) as usize;
                if n > MAX_TABLE_RUNS {
                    bail!("stsc excede limite de entradas ({n})");
                }
                stbl.stsc.reserve(n.min(4096));
                for _ in 0..n {
                    let (Some(fc), Some(spc), Some(_sdi)) = (r.u32(), r.u32(), r.u32()) else {
                        break;
                    };
                    if fc == 0 || spc as usize > MAX_SAMPLES {
                        bail!("entrada stsc hostil (first_chunk={fc}, spc={spc})");
                    }
                    stbl.stsc.push((fc, spc));
                }
            }
            b"stsz" => {
                let mut r = Reader::new(b.data);
                let _ = r.u32();
                let uniform = r.u32().unwrap_or(0);
                let n = r.u32().unwrap_or(0) as usize;
                if n > MAX_SAMPLES {
                    bail!("stsz excede limite de amostras ({n})");
                }
                stbl.uniform_size = uniform;
                if uniform == 0 {
                    stbl.sizes = Vec::with_capacity(n.min(65_536));
                    for _ in 0..n {
                        let Some(sz) = r.u32() else {
                            break;
                        };
                        stbl.sizes.push(sz);
                    }
                } else {
                    stbl.sizes = vec![uniform; n];
                }
            }
            b"stz2" => {
                let mut r = Reader::new(b.data);
                let _ = r.u32();
                let field_size = r.u32().unwrap_or(0) & 0xff;
                let n = r.u32().unwrap_or(0) as usize;
                if n > MAX_SAMPLES {
                    bail!("stz2 excede limite de amostras ({n})");
                }
                stbl.sizes = Vec::with_capacity(n.min(65_536));
                match field_size {
                    16 => {
                        for _ in 0..n {
                            let Some(sz) = r.u16() else {
                                break;
                            };
                            stbl.sizes.push(u32::from(sz));
                        }
                    }
                    8 => {
                        for _ in 0..n {
                            let Some(sz) = r.u8() else {
                                break;
                            };
                            stbl.sizes.push(u32::from(sz));
                        }
                    }
                    4 => {
                        let mut i = 0;
                        while i < n {
                            let Some(b) = r.u8() else {
                                break;
                            };
                            stbl.sizes.push(u32::from(b >> 4));
                            i += 1;
                            if i < n {
                                stbl.sizes.push(u32::from(b & 0x0f));
                                i += 1;
                            }
                        }
                    }
                    _ => bail!("stz2 field_size inválido ({field_size})"),
                }
            }
            b"stco" => {
                let mut r = Reader::new(b.data);
                let _ = r.u32();
                let n = r.u32().unwrap_or(0) as usize;
                if n > MAX_SAMPLES {
                    bail!("stco excede limite de chunks ({n})");
                }
                stbl.chunk_offsets = Vec::with_capacity(n.min(65_536));
                for _ in 0..n {
                    let Some(off) = r.u32() else {
                        break;
                    };
                    stbl.chunk_offsets.push(u64::from(off));
                }
            }
            b"co64" => {
                let mut r = Reader::new(b.data);
                let _ = r.u32();
                let n = r.u32().unwrap_or(0) as usize;
                if n > MAX_SAMPLES {
                    bail!("co64 excede limite de chunks ({n})");
                }
                stbl.chunk_offsets = Vec::with_capacity(n.min(65_536));
                for _ in 0..n {
                    let Some(off) = r.u64() else {
                        break;
                    };
                    stbl.chunk_offsets.push(off);
                }
            }
            b"stss" => {
                let mut r = Reader::new(b.data);
                let _ = r.u32();
                let n = r.u32().unwrap_or(0) as usize;
                if n > MAX_SAMPLES {
                    bail!("stss excede limite de keyframes ({n})");
                }
                let mut set = HashSet::with_capacity(n.min(65_536));
                for _ in 0..n {
                    let Some(idx) = r.u32() else {
                        break;
                    };
                    set.insert(idx);
                }
                stbl.sync_samples = Some(set);
            }
            _ => {}
        }
    }
    Ok(stbl)
}

fn parse_mvhd(data: &[u8]) -> Result<(u64, Option<Ticks>)> {
    let mut r = Reader::new(data);
    let vf = r.u32().ok_or_else(|| anyhow::anyhow!("mvhd truncado"))?;
    let ver = (vf >> 24) as u8;
    let (ts, dur) = if ver == 1 {
        let _ = r.skip(16);
        let ts = u64::from(r.u32().ok_or_else(|| anyhow::anyhow!("mvhd v1 truncado"))?);
        let d = r.u64().unwrap_or(0);
        (ts, d)
    } else {
        let _ = r.skip(8);
        let ts = u64::from(r.u32().ok_or_else(|| anyhow::anyhow!("mvhd v0 truncado"))?);
        let d = u64::from(r.u32().unwrap_or(0));
        (ts, d)
    };
    if ts == 0 {
        bail!("mvhd timescale nulo");
    }
    let dur_ticks = if dur > 0 {
        scale_to_ticks(i128::from(dur), 1, ts)
    } else {
        None
    };
    Ok((ts, dur_ticks))
}

fn parse_elst(data: &[u8], movie_ts: u64, media_ts: u64) -> i128 {
    let mut r = Reader::new(data);
    let Some(vf) = r.u32() else {
        return 0;
    };
    let ver = (vf >> 24) as u8;
    let count = r.u32().unwrap_or(0).min(1024);
    let mut empty_delay_ticks = 0i128;
    for _ in 0..count {
        let (seg_dur, media_time) = if ver == 1 {
            let Some(sd) = r.u64() else { break };
            let Some(mt) = r.i64() else { break };
            let _ = r.skip(4);
            (sd, mt)
        } else {
            let Some(sd) = r.u32() else { break };
            let Some(mt) = r.i32() else { break };
            let _ = r.skip(4);
            (u64::from(sd), i64::from(mt))
        };
        if media_time < 0 {
            if let Some(dt) = scale_to_ticks(i128::from(seg_dur), 1, movie_ts) {
                empty_delay_ticks = empty_delay_ticks.saturating_add(i128::from(dt.0));
            }
        } else {
            let media_shift = scale_to_ticks(i128::from(media_time), 1, media_ts)
                .map(|t| i128::from(t.0))
                .unwrap_or(0);
            return empty_delay_ticks.saturating_sub(media_shift);
        }
    }
    empty_delay_ticks
}

fn parse_trak(data: &[u8], movie_ts: u64, depth: usize) -> Result<Option<ParsedTrack>> {
    let boxes = iter_boxes(data, depth)?;
    let mut track_id = 0u32;
    let mut tkhd_w = 0u32;
    let mut tkhd_h = 0u32;
    let mut edts_box: Option<&[u8]> = None;
    let mut mdia_box: Option<&[u8]> = None;

    for b in &boxes {
        match &b.fourcc {
            b"tkhd" => {
                let mut r = Reader::new(b.data);
                if let Some(vf) = r.u32() {
                    let ver = (vf >> 24) as u8;
                    let ok = if ver == 1 {
                        r.skip(16).is_some()
                    } else {
                        r.skip(8).is_some()
                    };
                    if ok {
                        track_id = r.u32().unwrap_or(0);
                        let tail = if ver == 1 { 56 } else { 52 };
                        if r.skip(tail).is_some() {
                            tkhd_w = r.u32().unwrap_or(0) >> 16;
                            tkhd_h = r.u32().unwrap_or(0) >> 16;
                        }
                    }
                }
            }
            b"edts" => edts_box = Some(b.data),
            b"mdia" => mdia_box = Some(b.data),
            _ => {}
        }
    }
    let Some(mdia) = mdia_box else {
        return Ok(None);
    };
    let mut media_ts = 0u64;
    let mut media_dur = None;
    let mut kind = TrackKind::Other;
    let mut minf_box: Option<&[u8]> = None;
    for b in iter_boxes(mdia, depth + 1)? {
        match &b.fourcc {
            b"mdhd" => {
                let mut r = Reader::new(b.data);
                if let Some(vf) = r.u32() {
                    let ver = (vf >> 24) as u8;
                    let (ts, d) = if ver == 1 {
                        let _ = r.skip(16);
                        (u64::from(r.u32().unwrap_or(0)), r.u64().unwrap_or(0))
                    } else {
                        let _ = r.skip(8);
                        (
                            u64::from(r.u32().unwrap_or(0)),
                            u64::from(r.u32().unwrap_or(0)),
                        )
                    };
                    media_ts = ts;
                    if ts > 0 && d > 0 {
                        media_dur = scale_to_ticks(i128::from(d), 1, ts);
                    }
                }
            }
            b"hdlr" => {
                let mut r = Reader::new(b.data);
                if r.skip(8).is_some() {
                    if let Some(ht) = r.fourcc() {
                        kind = match &ht {
                            b"vide" => TrackKind::Video,
                            b"soun" => TrackKind::Audio,
                            _ => TrackKind::Other,
                        };
                    }
                }
            }
            b"minf" => minf_box = Some(b.data),
            _ => {}
        }
    }
    if media_ts == 0 {
        bail!("mdhd sem timescale válido na trilha {track_id}");
    }
    let mut stbl = StblData::default();
    if let Some(minf) = minf_box {
        for b in iter_boxes(minf, depth + 2)? {
            if &b.fourcc == b"stbl" {
                stbl = parse_stbl(b.data, kind, depth + 3)?;
                break;
            }
        }
    }
    if stbl.width == 0 && tkhd_w > 0 {
        stbl.width = tkhd_w;
    }
    if stbl.height == 0 && tkhd_h > 0 {
        stbl.height = tkhd_h;
    }
    let mut edit_shift_ticks = 0i128;
    if let Some(edts) = edts_box {
        for b in iter_boxes(edts, depth + 1)? {
            if &b.fourcc == b"elst" {
                edit_shift_ticks = parse_elst(b.data, movie_ts, media_ts);
                break;
            }
        }
    }
    Ok(Some(ParsedTrack {
        id: track_id,
        kind,
        media_timescale: media_ts,
        media_duration: media_dur,
        edit_shift_ticks,
        stbl,
    }))
}

fn expand_track_samples(
    track_index: usize,
    tr: &ParsedTrack,
    file_len: u64,
    total_samples_so_far: usize,
    out: &mut Vec<DemuxSample>,
) -> Result<(u64, u64)> {
    let st = &tr.stbl;
    let n_samples = st.sizes.len();
    if n_samples == 0 || st.chunk_offsets.is_empty() {
        return Ok((0, 0));
    }
    if total_samples_so_far.saturating_add(n_samples) > MAX_SAMPLES {
        bail!("total de amostras MP4 excede o limite");
    }

    // Constrói offsets/tamanhos por amostra via stsc + stco/co64 + stsz.
    let mut offsets = Vec::with_capacity(n_samples.min(65_536));
    let n_chunks = st.chunk_offsets.len();
    let mut sc_run = 0usize;
    let mut sample_idx = 0usize;
    for chunk_i in 0..n_chunks {
        let chunk_1based = (chunk_i + 1) as u32;
        while sc_run + 1 < st.stsc.len() && st.stsc[sc_run + 1].0 <= chunk_1based {
            sc_run += 1;
        }
        let spc = st.stsc.get(sc_run).map(|e| e.1).unwrap_or(1) as usize;
        let mut cursor = st.chunk_offsets[chunk_i];
        for _ in 0..spc {
            if sample_idx >= n_samples {
                break;
            }
            let sz = st.sizes[sample_idx];
            let end = cursor
                .checked_add(u64::from(sz))
                .ok_or_else(|| anyhow::anyhow!("overflow no offset de amostra MP4"))?;
            if end > file_len {
                bail!("amostra MP4 aponta além do fim do arquivo ({end} > {file_len})");
            }
            offsets.push(cursor);
            cursor = end;
            sample_idx += 1;
        }
        if sample_idx >= n_samples {
            break;
        }
    }
    if offsets.len() < n_samples {
        bail!(
            "tabela stsc/stco incompleta: {} offsets para {} amostras",
            offsets.len(),
            n_samples
        );
    }

    // Cursores de stts e ctts (O(N), sem alocação extra).
    let mut stts_i = 0usize;
    let mut stts_rem = st.stts.first().map(|x| x.0).unwrap_or(0);
    let mut stts_delta = st.stts.first().map(|x| x.1).unwrap_or(0);

    let ctts_slice = st.ctts.as_ref().map(|c| c.1.as_slice()).unwrap_or(&[]);
    let mut ctts_i = 0usize;
    let mut ctts_rem = ctts_slice.first().map(|x| x.0).unwrap_or(0);
    let mut ctts_off = ctts_slice.first().map(|x| x.1).unwrap_or(0);

    let mut dts_units = 0i128;
    let mut dur_sum = 0u64;
    let ts = tr.media_timescale;

    out.reserve(n_samples);
    for (idx, &off) in offsets.iter().enumerate() {
        while stts_rem == 0 && stts_i + 1 < st.stts.len() {
            stts_i += 1;
            stts_rem = st.stts[stts_i].0;
            stts_delta = st.stts[stts_i].1;
        }
        let delta = if stts_rem > 0 {
            stts_rem -= 1;
            stts_delta
        } else {
            0
        };

        while ctts_rem == 0 && ctts_i + 1 < ctts_slice.len() {
            ctts_i += 1;
            ctts_rem = ctts_slice[ctts_i].0;
            ctts_off = ctts_slice[ctts_i].1;
        }
        let cts = if ctts_rem > 0 {
            ctts_rem -= 1;
            ctts_off
        } else {
            0
        };

        let pts_units = dts_units.saturating_add(i128::from(cts));
        let base_dts = scale_to_ticks(dts_units, 1, ts).unwrap_or(Ticks::ZERO);
        let base_pts = scale_to_ticks(pts_units, 1, ts).unwrap_or(Ticks::ZERO);
        let dts = Ticks(
            i64::try_from(i128::from(base_dts.0).saturating_add(tr.edit_shift_ticks))
                .unwrap_or(base_dts.0),
        );
        let pts = Ticks(
            i64::try_from(i128::from(base_pts.0).saturating_add(tr.edit_shift_ticks))
                .unwrap_or(base_pts.0),
        );
        let duration = scale_to_ticks(i128::from(delta), 1, ts).unwrap_or(Ticks::ZERO);
        let sample_1based = (idx + 1) as u32;
        let is_keyframe = match &st.sync_samples {
            Some(set) => set.contains(&sample_1based),
            None => true,
        };
        out.push(DemuxSample {
            track_index,
            offset: off,
            size: st.sizes[idx],
            pts,
            dts,
            duration,
            is_keyframe,
        });
        dts_units = dts_units.saturating_add(i128::from(delta));
        dur_sum = dur_sum.saturating_add(u64::from(delta));
    }
    Ok((n_samples as u64, dur_sum))
}

struct FragAccum<'a> {
    tracks: &'a [ParsedTrack],
    defaults: &'a [(u32, TrackDefaults)],
    frag_dts: Vec<u64>,
    samples: Vec<DemuxSample>,
    sample_counts: Vec<u64>,
    dur_sums: Vec<u64>,
}

fn parse_moof(
    moof_data: &[u8],
    moof_offset: u64,
    file_len: u64,
    acc: &mut FragAccum<'_>,
) -> Result<()> {
    let tracks = acc.tracks;
    let defaults = acc.defaults;
    let frag_dts = &mut acc.frag_dts;
    let out = &mut acc.samples;
    let frag_sample_counts = &mut acc.sample_counts;
    let frag_dur_sums = &mut acc.dur_sums;
    let mut prev_traf_end: Option<u64> = None;
    for box_item in iter_boxes(moof_data, 1)? {
        if &box_item.fourcc != b"traf" {
            continue;
        }
        let mut track_id = 0u32;
        let mut base_data_offset: Option<u64> = None;
        let mut default_base_is_moof = false;
        let mut def_dur: Option<u32> = None;
        let mut def_size: Option<u32> = None;
        let mut def_flags: Option<u32> = None;
        let mut decode_time: Option<u64> = None;
        let mut truns: Vec<&[u8]> = Vec::new();

        for sub in iter_boxes(box_item.data, 2)? {
            match &sub.fourcc {
                b"tfhd" => {
                    let mut r = Reader::new(sub.data);
                    let Some(vf) = r.u32() else {
                        continue;
                    };
                    let flags = vf & 0x00ff_ffff;
                    track_id = r.u32().unwrap_or(0);
                    default_base_is_moof = flags & 0x02_0000 != 0;
                    if flags & 0x00_0001 != 0 {
                        base_data_offset = r.u64();
                    }
                    if flags & 0x00_0002 != 0 {
                        let _ = r.u32();
                    }
                    if flags & 0x00_0008 != 0 {
                        def_dur = r.u32();
                    }
                    if flags & 0x00_0010 != 0 {
                        def_size = r.u32();
                    }
                    if flags & 0x00_0020 != 0 {
                        def_flags = r.u32();
                    }
                }
                b"tfdt" => {
                    let mut r = Reader::new(sub.data);
                    if let Some(vf) = r.u32() {
                        let ver = (vf >> 24) as u8;
                        decode_time = if ver == 1 {
                            r.u64()
                        } else {
                            r.u32().map(u64::from)
                        };
                    }
                }
                b"trun" => truns.push(sub.data),
                _ => {}
            }
        }
        let Some(ti) = tracks.iter().position(|t| t.id == track_id) else {
            continue;
        };
        let tr = &tracks[ti];
        let trex = defaults
            .iter()
            .find(|(id, _)| *id == track_id)
            .map(|(_, d)| d);
        let d_dur = def_dur
            .or_else(|| trex.map(|t| t.default_sample_duration))
            .unwrap_or(0);
        let d_size = def_size
            .or_else(|| trex.map(|t| t.default_sample_size))
            .unwrap_or(0);
        let d_flags = def_flags
            .or_else(|| trex.map(|t| t.default_sample_flags))
            .unwrap_or(0);
        let _ = trex.map(|t| t.sample_description_index);

        let anchor = match base_data_offset {
            Some(off) => off,
            None if default_base_is_moof => moof_offset,
            None => prev_traf_end.unwrap_or(moof_offset),
        };

        let mut dts_units: u64 =
            decode_time.unwrap_or_else(|| frag_dts.get(ti).copied().unwrap_or(0));
        let mut cursor = anchor;

        for trun in truns {
            let mut r = Reader::new(trun);
            let Some(vf) = r.u32() else {
                continue;
            };
            let ver = (vf >> 24) as u8;
            let flags = vf & 0x00ff_ffff;
            let count = r.u32().unwrap_or(0) as usize;
            if out.len().saturating_add(count) > MAX_SAMPLES {
                bail!("fragmento trun excede limite máximo de amostras ({count})");
            }
            if flags & 0x00_0001 != 0 {
                let off = r.i32().unwrap_or(0);
                let signed = i128::from(anchor).saturating_add(i128::from(off));
                if signed < 0 {
                    bail!("trun data_offset negativo");
                }
                cursor = u64::try_from(signed).unwrap_or(anchor);
            }
            let first_flags = if flags & 0x00_0004 != 0 {
                r.u32()
            } else {
                None
            };
            out.reserve(count);
            for i in 0..count {
                let dur = if flags & 0x00_0100 != 0 {
                    r.u32().unwrap_or(d_dur)
                } else {
                    d_dur
                };
                let sz = if flags & 0x00_0200 != 0 {
                    r.u32().unwrap_or(d_size)
                } else {
                    d_size
                };
                let s_flags = if flags & 0x00_0400 != 0 {
                    r.u32().unwrap_or(d_flags)
                } else if i == 0 {
                    first_flags.unwrap_or(d_flags)
                } else {
                    d_flags
                };
                let cts = if flags & 0x00_0800 != 0 {
                    if ver == 0 {
                        i64::from(r.u32().unwrap_or(0))
                    } else {
                        i64::from(r.i32().unwrap_or(0))
                    }
                } else {
                    0
                };
                let end = cursor
                    .checked_add(u64::from(sz))
                    .ok_or_else(|| anyhow::anyhow!("overflow em offset de amostra fragmentada"))?;
                if end > file_len {
                    bail!("amostra fragmentada aponta além do EOF ({end} > {file_len})");
                }
                let pts_units = i128::from(dts_units).saturating_add(i128::from(cts));
                let ts = tr.media_timescale;
                let base_dts = scale_to_ticks(i128::from(dts_units), 1, ts).unwrap_or(Ticks::ZERO);
                let base_pts = scale_to_ticks(pts_units, 1, ts).unwrap_or(Ticks::ZERO);
                let dts = Ticks(
                    i64::try_from(i128::from(base_dts.0).saturating_add(tr.edit_shift_ticks))
                        .unwrap_or(base_dts.0),
                );
                let pts = Ticks(
                    i64::try_from(i128::from(base_pts.0).saturating_add(tr.edit_shift_ticks))
                        .unwrap_or(base_pts.0),
                );
                let duration = scale_to_ticks(i128::from(dur), 1, ts).unwrap_or(Ticks::ZERO);
                let is_keyframe = (s_flags & 0x0001_0000) == 0;
                out.push(DemuxSample {
                    track_index: ti,
                    offset: cursor,
                    size: sz,
                    pts,
                    dts,
                    duration,
                    is_keyframe,
                });
                cursor = end;
                dts_units = dts_units.saturating_add(u64::from(dur));
                frag_sample_counts[ti] = frag_sample_counts[ti].saturating_add(1);
                frag_dur_sums[ti] = frag_dur_sums[ti].saturating_add(u64::from(dur));
            }
        }
        prev_traf_end = Some(cursor);
        if let Some(slot) = frag_dts.get_mut(ti) {
            *slot = dts_units;
        }
    }
    Ok(())
}

pub(crate) fn parse_mp4<R: Read + Seek>(r: &mut R, len: u64) -> Result<DemuxIndex> {
    let mut pos = 0u64;
    let mut container = ContainerKind::Mp4;
    let mut seen_ftyp = false;
    let mut seen_moov = false;
    let mut movie_duration: Option<Ticks> = None;
    let mut parsed_tracks: Vec<ParsedTrack> = Vec::new();
    let mut trex_defaults: Vec<(u32, TrackDefaults)> = Vec::new();
    let mut moofs: Vec<(u64, Vec<u8>)> = Vec::new();
    let mut top_count = 0usize;

    while pos + 8 <= len {
        if top_count >= MAX_BOX_COUNT {
            bail!("número excessivo de caixas no topo MP4");
        }
        top_count += 1;
        let mut hdr = [0u8; 8];
        read_exact_at(r, pos, &mut hdr)?;
        let sz32 = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
        let fourcc = [hdr[4], hdr[5], hdr[6], hdr[7]];
        let (hdr_len, total_size): (u64, u64) = if sz32 == 1 {
            if pos + 16 > len {
                bail!("caixa estendida no topo MP4 truncada");
            }
            let mut ext = [0u8; 8];
            read_exact_at(r, pos + 8, &mut ext)?;
            let large = u64::from_be_bytes(ext);
            if large < 16 {
                bail!("tamanho estendido inválido ({large})");
            }
            (16, large)
        } else if sz32 == 0 {
            (8, len - pos)
        } else {
            if sz32 < 8 {
                bail!("tamanho de caixa inválido ({sz32})");
            }
            (8, u64::from(sz32))
        };
        let end = pos
            .checked_add(total_size)
            .filter(|&e| e <= len)
            .ok_or_else(|| anyhow::anyhow!("caixa no topo MP4 excede EOF"))?;
        let payload_len = total_size.saturating_sub(hdr_len);

        match &fourcc {
            b"ftyp" => {
                seen_ftyp = true;
                if payload_len >= 4 {
                    let read_len = usize::try_from(payload_len.min(4096)).unwrap_or(4096);
                    let mut buf = vec![0u8; read_len];
                    read_exact_at(r, pos + hdr_len, &mut buf)?;
                    if buf.get(0..4) == Some(b"qt  ") {
                        container = ContainerKind::Mov;
                    }
                }
            }
            b"moov" => {
                seen_moov = true;
                if payload_len > MAX_METADATA_READ as u64 {
                    bail!("caixa moov excede limite de memória ({payload_len})");
                }
                let mut buf = vec![0u8; payload_len as usize];
                read_exact_at(r, pos + hdr_len, &mut buf)?;
                let children = iter_boxes(&buf, 0)?;
                let mut movie_ts = 0u64;
                for c in &children {
                    if &c.fourcc == b"mvhd" {
                        let (ts, dur) = parse_mvhd(c.data)?;
                        movie_ts = ts;
                        movie_duration = dur;
                    } else if &c.fourcc == b"mvex" {
                        for sub in iter_boxes(c.data, 1)? {
                            if &sub.fourcc == b"trex" {
                                let mut tr_r = Reader::new(sub.data);
                                if tr_r.u32().is_some() {
                                    let tid = tr_r.u32().unwrap_or(0);
                                    let sdi = tr_r.u32().unwrap_or(1);
                                    let dur = tr_r.u32().unwrap_or(0);
                                    let sz = tr_r.u32().unwrap_or(0);
                                    let fl = tr_r.u32().unwrap_or(0);
                                    trex_defaults.push((
                                        tid,
                                        TrackDefaults {
                                            sample_description_index: sdi,
                                            default_sample_duration: dur,
                                            default_sample_size: sz,
                                            default_sample_flags: fl,
                                        },
                                    ));
                                }
                            }
                        }
                    }
                }
                if movie_ts == 0 {
                    bail!("moov sem mvhd válido");
                }
                for c in &children {
                    if &c.fourcc == b"trak" {
                        if parsed_tracks.len() >= MAX_TRACKS {
                            bail!("número de trilhas MP4 excede o máximo ({MAX_TRACKS})");
                        }
                        if let Some(tr) = parse_trak(c.data, movie_ts, 1)? {
                            parsed_tracks.push(tr);
                        }
                    }
                }
            }
            b"moof" => {
                if payload_len > MAX_METADATA_READ as u64 {
                    bail!("caixa moof excede limite de memória ({payload_len})");
                }
                let mut buf = vec![0u8; payload_len as usize];
                read_exact_at(r, pos + hdr_len, &mut buf)?;
                moofs.push((pos, buf));
            }
            _ => {
                // mdat, free, skip, meta, etc.: pular sem ler conteúdo para RAM (§3.6)
            }
        }
        r.seek(SeekFrom::Start(end))?;
        pos = end;
    }

    if !seen_moov && !seen_ftyp {
        bail!("arquivo não é um contêiner MP4/MOV/EBML válido");
    }
    if !seen_moov {
        bail!("contêiner MP4/MOV sem caixa moov");
    }

    let n_tr = parsed_tracks.len();
    let mut samples = Vec::new();
    let mut sample_counts = vec![0u64; n_tr];
    let mut dur_sums = vec![0u64; n_tr];

    for (ti, tr) in parsed_tracks.iter().enumerate() {
        let (cnt, sum) = expand_track_samples(ti, tr, len, samples.len(), &mut samples)?;
        sample_counts[ti] = cnt;
        dur_sums[ti] = sum;
    }

    let mut acc = FragAccum {
        tracks: &parsed_tracks,
        defaults: &trex_defaults,
        frag_dts: vec![0u64; n_tr],
        samples,
        sample_counts,
        dur_sums,
    };
    for (moof_off, moof_data) in &moofs {
        parse_moof(moof_data, *moof_off, len, &mut acc)?;
    }
    let samples = acc.samples;
    let sample_counts = acc.sample_counts;
    let dur_sums = acc.dur_sums;

    let mut tracks = Vec::with_capacity(n_tr);
    let mut max_dur = movie_duration;
    for (ti, tr) in parsed_tracks.into_iter().enumerate() {
        let fps: Option<FrameRate> =
            if tr.kind == TrackKind::Video && sample_counts[ti] > 0 && dur_sums[ti] > 0 {
                rational_fps(
                    sample_counts[ti].saturating_mul(tr.media_timescale),
                    dur_sums[ti],
                )
            } else {
                None
            };
        let tr_dur = tr.media_duration.or_else(|| {
            if dur_sums[ti] > 0 {
                scale_to_ticks(i128::from(dur_sums[ti]), 1, tr.media_timescale)
            } else {
                None
            }
        });
        if let Some(td) = tr_dur {
            max_dur = Some(match max_dur {
                Some(md) if md >= td => md,
                _ => td,
            });
        }
        tracks.push(DemuxTrack {
            id: tr.id,
            kind: tr.kind,
            codec: tr.stbl.codec,
            extra: tr.stbl.extra,
            timescale: tr.media_timescale,
            duration: tr_dur,
            width: tr.stbl.width,
            height: tr.stbl.height,
            fps,
            sample_rate: tr.stbl.sample_rate,
            channels: tr.stbl.channels,
            codec_delay: Ticks::ZERO,
        });
    }

    Ok(DemuxIndex {
        container,
        duration: max_dur,
        tracks,
        samples,
    })
}
