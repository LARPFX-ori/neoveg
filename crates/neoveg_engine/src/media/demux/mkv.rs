//! Demuxer EBML / Matroska / WebM em Rust puro.
//!
//! Implementado em Sala Limpa a partir das especificações públicas:
//! - RFC 8794 (*Extensible Binary Meta Language (EBML)*)
//! - RFC 9559 (*Matroska Media Container Format*) e WebM Container Guidelines

use super::{
    rational_fps, scale_to_ticks, ContainerKind, DemuxIndex, DemuxSample, DemuxTrack, TrackKind,
    MAX_CODEC_EXTRA, MAX_CONTAINER_DEPTH, MAX_METADATA_READ, MAX_SAMPLES, MAX_TRACKS,
};
use crate::time::Ticks;
use anyhow::{bail, Result};
use std::io::{Read, Seek, SeekFrom};

const ID_EBML: u64 = 0x1A45_DFA3;
const ID_DOC_TYPE: u64 = 0x4282;
const ID_SEGMENT: u64 = 0x1853_8067;
const ID_INFO: u64 = 0x1549_A966;
const ID_TIMESTAMP_SCALE: u64 = 0x002A_D7B1;
const ID_DURATION: u64 = 0x4489;
const ID_TRACKS: u64 = 0x1654_AE6B;
const ID_TRACK_ENTRY: u64 = 0xAE;
const ID_TRACK_NUMBER: u64 = 0xD7;
const ID_TRACK_TYPE: u64 = 0x83;
const ID_DEFAULT_DURATION: u64 = 0x0023_E383;
const ID_CODEC_DELAY: u64 = 0x56AA;
const ID_CODEC_ID: u64 = 0x86;
const ID_CODEC_PRIVATE: u64 = 0x63A2;
const ID_VIDEO: u64 = 0xE0;
const ID_PIXEL_WIDTH: u64 = 0xB0;
const ID_PIXEL_HEIGHT: u64 = 0xBA;
const ID_AUDIO: u64 = 0xE1;
const ID_SAMPLING_FREQ: u64 = 0xB5;
const ID_CHANNELS: u64 = 0x9F;
const ID_CLUSTER: u64 = 0x1F43_B675;
const ID_CLUSTER_TIMESTAMP: u64 = 0xE7;
const ID_SIMPLE_BLOCK: u64 = 0xA3;
const ID_BLOCK_GROUP: u64 = 0xA0;
const ID_BLOCK: u64 = 0xA1;
const ID_BLOCK_DURATION: u64 = 0x9B;
const ID_REFERENCE_BLOCK: u64 = 0xFB;

const MAX_MKV_ELEMENTS: usize = 512_000;

struct SliceCursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> SliceCursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn rem(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }

    fn u8(&mut self) -> Option<u8> {
        let &b = self.data.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    fn vint_raw(&mut self, max_bytes: usize) -> Result<(u64, usize, u64, bool)> {
        let Some(b0) = self.u8() else {
            bail!("VINT EBML truncado");
        };
        let lz = b0.leading_zeros() as usize;
        let width = lz + 1;
        if width > max_bytes || width > 8 {
            bail!("largura de VINT EBML inválida ({width})");
        }
        let mut raw = u64::from(b0);
        for _ in 1..width {
            let Some(b) = self.u8() else {
                bail!("VINT EBML truncado no meio");
            };
            raw = (raw << 8) | u64::from(b);
        }
        let mask = if width == 8 {
            (1u64 << 56) - 1
        } else {
            (1u64 << (7 * width)) - 1
        };
        let val = raw & mask;
        Ok((raw, width, val, val == mask))
    }
}

fn read_vint_stream<R: Read>(
    r: &mut R,
    max_bytes: usize,
) -> Result<Option<(u64, usize, u64, bool)>> {
    let mut first = [0u8; 1];
    match r.read(&mut first) {
        Ok(0) => return Ok(None),
        Ok(_) => {}
        Err(e) => return Err(e.into()),
    }
    let lz = first[0].leading_zeros() as usize;
    let width = lz + 1;
    if width > max_bytes || width > 8 {
        bail!("VINT EBML inválido em stream (width={width})");
    }
    let mut raw = u64::from(first[0]);
    if width > 1 {
        let mut rest = [0u8; 7];
        r.read_exact(&mut rest[..width - 1])?;
        for &b in &rest[..width - 1] {
            raw = (raw << 8) | u64::from(b);
        }
    }
    let mask = if width == 8 {
        (1u64 << 56) - 1
    } else {
        (1u64 << (7 * width)) - 1
    };
    let val = raw & mask;
    Ok(Some((raw, width, val, val == mask)))
}

fn read_elem_header_at<R: Read + Seek>(
    r: &mut R,
    pos: u64,
    limit: u64,
) -> Result<Option<(u64, u64, u64)>> {
    if pos >= limit {
        return Ok(None);
    }
    r.seek(SeekFrom::Start(pos))?;
    let Some((id, _, _, _)) = read_vint_stream(r, 4)? else {
        return Ok(None);
    };
    let Some((_, _, sz_val, unknown)) = read_vint_stream(r, 8)? else {
        bail!("tamanho EBML ausente após ID {id:#x}");
    };
    let data_start = r.stream_position()?;
    if data_start > limit {
        bail!("cabeçalho EBML excede o limite do contêiner");
    }
    let end = if unknown {
        limit
    } else {
        let e = data_start
            .checked_add(sz_val)
            .ok_or_else(|| anyhow::anyhow!("overflow em elemento EBML"))?;
        if e > limit {
            bail!("elemento EBML excede limite ({e} > {limit})");
        }
        e
    };
    Ok(Some((id, data_start, end)))
}

fn ebml_uint(bytes: &[u8]) -> Result<u64> {
    if bytes.len() > 8 {
        bail!("inteiro EBML maior que 8 bytes");
    }
    let mut v = 0u64;
    for &b in bytes {
        v = (v << 8) | u64::from(b);
    }
    Ok(v)
}

fn ebml_float(bytes: &[u8]) -> Result<f64> {
    match bytes.len() {
        0 => Ok(0.0),
        4 => {
            let v = f64::from(f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
            Ok(if v.is_finite() { v } else { 0.0 })
        }
        8 => {
            let v = f64::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]);
            Ok(if v.is_finite() { v } else { 0.0 })
        }
        _ => bail!("float EBML inválido ({} bytes)", bytes.len()),
    }
}

struct MkvChild<'a> {
    id: u64,
    data: &'a [u8],
}

fn parse_mem_children(buf: &[u8], depth: usize) -> Result<Vec<MkvChild<'_>>> {
    if depth >= MAX_CONTAINER_DEPTH {
        bail!("profundidade EBML máxima excedida");
    }
    let mut cur = SliceCursor::new(buf);
    let mut out = Vec::new();
    let mut count = 0usize;
    while cur.rem() > 0 {
        if count >= MAX_MKV_ELEMENTS {
            bail!("número excessivo de elementos EBML");
        }
        count += 1;
        let (id, _, _, _) = cur.vint_raw(4)?;
        let (_, _, sz, unknown) = cur.vint_raw(8)?;
        if unknown {
            let data = cur.data.get(cur.pos..).unwrap_or(&[]);
            out.push(MkvChild { id, data });
            break;
        }
        let n = usize::try_from(sz)
            .map_err(|_| anyhow::anyhow!("elemento EBML excede espaço endereçável"))?;
        let Some(data) = cur.take(n) else {
            bail!("elemento EBML {id:#x} truncado");
        };
        out.push(MkvChild { id, data });
    }
    Ok(out)
}

struct MkvTrackMeta {
    number: u64,
    kind: TrackKind,
    codec: String,
    extra: Vec<u8>,
    default_duration_ns: u64,
    codec_delay_ns: u64,
    width: u32,
    height: u32,
    sample_rate: u32,
    channels: u16,
}

fn parse_tracks(buf: &[u8]) -> Result<Vec<MkvTrackMeta>> {
    let mut tracks = Vec::new();
    for c in parse_mem_children(buf, 2)? {
        if c.id != ID_TRACK_ENTRY {
            continue;
        }
        if tracks.len() >= MAX_TRACKS {
            bail!("número de trilhas MKV excede máximo ({MAX_TRACKS})");
        }
        let mut tr = MkvTrackMeta {
            number: 0,
            kind: TrackKind::Other,
            codec: String::new(),
            extra: Vec::new(),
            default_duration_ns: 0,
            codec_delay_ns: 0,
            width: 0,
            height: 0,
            sample_rate: 0,
            channels: 0,
        };
        for f in parse_mem_children(c.data, 3)? {
            match f.id {
                ID_TRACK_NUMBER => tr.number = ebml_uint(f.data)?,
                ID_TRACK_TYPE => {
                    tr.kind = match ebml_uint(f.data)? {
                        1 => TrackKind::Video,
                        2 => TrackKind::Audio,
                        _ => TrackKind::Other,
                    };
                }
                ID_DEFAULT_DURATION => tr.default_duration_ns = ebml_uint(f.data)?,
                ID_CODEC_DELAY => tr.codec_delay_ns = ebml_uint(f.data)?,
                ID_CODEC_ID => {
                    tr.codec = String::from_utf8_lossy(f.data)
                        .trim_end_matches('\0')
                        .to_string();
                }
                ID_CODEC_PRIVATE => {
                    let n = f.data.len().min(MAX_CODEC_EXTRA);
                    tr.extra = f.data[..n].to_vec();
                }
                ID_VIDEO => {
                    for vf in parse_mem_children(f.data, 4)? {
                        match vf.id {
                            ID_PIXEL_WIDTH => {
                                tr.width = u32::try_from(ebml_uint(vf.data)?).unwrap_or(0);
                            }
                            ID_PIXEL_HEIGHT => {
                                tr.height = u32::try_from(ebml_uint(vf.data)?).unwrap_or(0);
                            }
                            _ => {}
                        }
                    }
                }
                ID_AUDIO => {
                    for af in parse_mem_children(f.data, 4)? {
                        match af.id {
                            ID_SAMPLING_FREQ => {
                                let sr = ebml_float(af.data)?;
                                if sr > 0.0 && sr <= 1_000_000.0 {
                                    tr.sample_rate = sr.round() as u32;
                                }
                            }
                            ID_CHANNELS => {
                                tr.channels = u16::try_from(ebml_uint(af.data)?).unwrap_or(0);
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        if tr.number > 0 {
            tracks.push(tr);
        }
    }
    Ok(tracks)
}

struct BlockFrames {
    track_num: u64,
    rel_ts: i16,
    keyframe: bool,
    frames: Vec<(u64, u32)>,
}

fn parse_block_frames<R: Read + Seek>(
    r: &mut R,
    block_start: u64,
    block_end: u64,
    is_simple_block: bool,
) -> Result<BlockFrames> {
    let total_len = block_end.saturating_sub(block_start);
    if total_len < 4 {
        bail!("bloco Matroska curto demais");
    }
    r.seek(SeekFrom::Start(block_start))?;
    let Some((_, t_w, track_num, _)) = read_vint_stream(r, 8)? else {
        bail!("TrackNumber ausente em bloco Matroska");
    };
    let mut hdr_tail = [0u8; 3];
    if block_start + (t_w as u64) + 3 > block_end {
        bail!("cabeçalho de bloco Matroska truncado");
    }
    r.read_exact(&mut hdr_tail)?;
    let rel_ts = i16::from_be_bytes([hdr_tail[0], hdr_tail[1]]);
    let flags = hdr_tail[2];
    let keyframe = if is_simple_block {
        flags & 0x80 != 0
    } else {
        true
    };
    let lacing = (flags >> 1) & 0x03;
    let payload_start = r.stream_position()?;
    let payload_len = block_end.saturating_sub(payload_start);

    if lacing == 0 {
        let sz = u32::try_from(payload_len)
            .map_err(|_| anyhow::anyhow!("quadro Matroska excede 4 GiB"))?;
        return Ok(BlockFrames {
            track_num,
            rel_ts,
            keyframe,
            frames: vec![(payload_start, sz)],
        });
    }

    // Lacing Matroska (Xiph = 1, Fixed = 2, EBML = 3). Só lê o cabeçalho do lace.
    if payload_len == 0 {
        bail!("cabeçalho de lacing Matroska ausente");
    }
    let mut cnt_b = [0u8; 1];
    r.read_exact(&mut cnt_b)?;
    let n_frames = usize::from(cnt_b[0]) + 1;

    let mut sizes = Vec::with_capacity(n_frames);
    match lacing {
        1 => {
            // Xiph lacing: soma bytes == 255 até byte < 255 para os n_frames - 1 primeiros quadros
            let mut used = 1u64;
            for _ in 0..n_frames - 1 {
                let mut fsz = 0u64;
                loop {
                    if used >= payload_len {
                        bail!("Xiph lacing ultrapassa o bloco");
                    }
                    let mut b = [0u8; 1];
                    r.read_exact(&mut b)?;
                    used += 1;
                    fsz = fsz.saturating_add(u64::from(b[0]));
                    if fsz > payload_len {
                        bail!("tamanho de quadro Xiph excede o bloco");
                    }
                    if b[0] < 255 {
                        break;
                    }
                }
                sizes.push(fsz);
            }
        }
        2 => {
            // Fixed-size lacing
            let data_rem = payload_len.saturating_sub(1);
            let n64 = n_frames as u64;
            if data_rem % n64 != 0 {
                bail!("fixed-size lacing não divisível");
            }
            let each = data_rem / n64;
            for _ in 0..n_frames {
                sizes.push(each);
            }
        }
        3 => {
            // EBML lacing: primeiro tamanho como VINT sem sinal, depois diferenças signed VINT
            let mut cur_sz: i128 = 0;
            for fi in 0..n_frames - 1 {
                let Some((_, w, val, _)) = read_vint_stream(r, 8)? else {
                    bail!("EBML lacing truncado");
                };
                if fi == 0 {
                    cur_sz = i128::from(val);
                } else {
                    let bias = (1i128 << (7 * w - 1)) - 1;
                    cur_sz = cur_sz.saturating_add(i128::from(val) - bias);
                }
                if cur_sz < 0 || cur_sz > i128::from(payload_len) {
                    bail!("tamanho EBML lacing negativo ou excede bloco ({cur_sz})");
                }
                sizes.push(cur_sz as u64);
            }
        }
        _ => bail!("modo de lacing desconhecido"),
    }

    let data_start = r.stream_position()?;
    let data_rem = block_end.saturating_sub(data_start);
    if lacing != 2 {
        let sum_first: u64 = sizes.iter().copied().fold(0u64, u64::saturating_add);
        if sum_first > data_rem {
            bail!("soma de quadros em lacing excede payload do bloco");
        }
        sizes.push(data_rem - sum_first);
    }

    let mut frames = Vec::with_capacity(sizes.len());
    let mut off = data_start;
    for sz64 in sizes {
        let end = off
            .checked_add(sz64)
            .filter(|&e| e <= block_end)
            .ok_or_else(|| anyhow::anyhow!("quadro Matroska excede limite do bloco"))?;
        let sz =
            u32::try_from(sz64).map_err(|_| anyhow::anyhow!("quadro Matroska excede u32::MAX"))?;
        frames.push((off, sz));
        off = end;
    }
    Ok(BlockFrames {
        track_num,
        rel_ts,
        keyframe,
        frames,
    })
}

pub(crate) fn parse_mkv<R: Read + Seek>(r: &mut R, len: u64) -> Result<DemuxIndex> {
    let mut pos = 0u64;
    let mut container = ContainerKind::Mkv;
    let mut seg_span: Option<(u64, u64)> = None;

    while let Some((id, start, end)) = read_elem_header_at(r, pos, len)? {
        if end <= start && end == pos {
            bail!("elemento EBML com tamanho zero em loop");
        }
        match id {
            ID_EBML => {
                let sz = end.saturating_sub(start).min(64 * 1024) as usize;
                let mut buf = vec![0u8; sz];
                r.seek(SeekFrom::Start(start))?;
                r.read_exact(&mut buf)?;
                for c in parse_mem_children(&buf, 1)? {
                    if c.id == ID_DOC_TYPE {
                        let dt = String::from_utf8_lossy(c.data).to_ascii_lowercase();
                        if dt.trim_end_matches('\0') == "webm" {
                            container = ContainerKind::WebM;
                        }
                    }
                }
            }
            ID_SEGMENT => {
                seg_span = Some((start, end));
                break;
            }
            _ => {}
        }
        pos = end;
    }

    let Some((seg_start, seg_end)) = seg_span else {
        bail!("contêiner EBML sem Segment Matroska");
    };

    let mut ts_scale_ns = 1_000_000u64;
    let mut seg_dur_ticks: Option<Ticks> = None;
    let mut seg_dur_raw: Option<f64> = None;
    let mut raw_tracks: Vec<MkvTrackMeta> = Vec::new();
    let mut samples: Vec<DemuxSample> = Vec::new();
    let mut elem_count = 0usize;

    let mut s_pos = seg_start;
    while let Some((id, d_start, d_end)) = read_elem_header_at(r, s_pos, seg_end)? {
        if elem_count >= MAX_MKV_ELEMENTS {
            bail!("elementos de Segment MKV excedem limite");
        }
        elem_count += 1;
        match id {
            ID_INFO => {
                let sz = d_end.saturating_sub(d_start);
                if sz > MAX_METADATA_READ as u64 {
                    bail!("Info MKV excede limite");
                }
                let mut buf = vec![0u8; sz as usize];
                r.seek(SeekFrom::Start(d_start))?;
                r.read_exact(&mut buf)?;
                for c in parse_mem_children(&buf, 1)? {
                    match c.id {
                        ID_TIMESTAMP_SCALE => {
                            let sc = ebml_uint(c.data)?;
                            if sc == 0 {
                                bail!("TimestampScale MKV nulo");
                            }
                            ts_scale_ns = sc;
                        }
                        ID_DURATION => {
                            let d = ebml_float(c.data)?;
                            if d > 0.0 {
                                seg_dur_raw = Some(d);
                            }
                        }
                        _ => {}
                    }
                }
            }
            ID_TRACKS => {
                let sz = d_end.saturating_sub(d_start);
                if sz > MAX_METADATA_READ as u64 {
                    bail!("Tracks MKV excedem limite");
                }
                let mut buf = vec![0u8; sz as usize];
                r.seek(SeekFrom::Start(d_start))?;
                r.read_exact(&mut buf)?;
                raw_tracks = parse_tracks(&buf)?;
            }
            ID_CLUSTER => {
                let mut cluster_ts: i128 = 0;
                let mut c_pos = d_start;
                let mut cluster_end = d_end;
                while let Some((cid, c_start, c_end)) = read_elem_header_at(r, c_pos, cluster_end)?
                {
                    if matches!(
                        cid,
                        ID_EBML
                            | ID_SEGMENT
                            | ID_INFO
                            | ID_TRACKS
                            | ID_CLUSTER
                            | 0x114D_9B74 // SeekHead
                            | 0x1C53_BB6B // Cues
                            | 0x1254_C367 // Tags
                            | 0x1043_A770 // Chapters
                            | 0x1941_A469 // Attachments
                    ) {
                        cluster_end = c_pos;
                        break;
                    }
                    if elem_count >= MAX_MKV_ELEMENTS {
                        bail!("elementos em Cluster MKV excedem limite");
                    }
                    elem_count += 1;
                    match cid {
                        ID_CLUSTER_TIMESTAMP => {
                            let sz = c_end.saturating_sub(c_start).min(16) as usize;
                            let mut buf = vec![0u8; sz];
                            r.seek(SeekFrom::Start(c_start))?;
                            r.read_exact(&mut buf)?;
                            cluster_ts = i128::from(ebml_uint(&buf)?);
                        }
                        ID_SIMPLE_BLOCK => {
                            let bf = parse_block_frames(r, c_start, c_end, true)?;
                            let (tnum, rel_ts, kf, frames) =
                                (bf.track_num, bf.rel_ts, bf.keyframe, bf.frames);
                            if let Some(ti) = raw_tracks.iter().position(|t| t.number == tnum) {
                                let abs_units = cluster_ts.saturating_add(i128::from(rel_ts));
                                let delay = scale_to_ticks(
                                    i128::from(raw_tracks[ti].codec_delay_ns),
                                    1,
                                    1_000_000_000,
                                )
                                .unwrap_or(Ticks::ZERO);
                                let raw_t = scale_to_ticks(abs_units, ts_scale_ns, 1_000_000_000)
                                    .unwrap_or(Ticks::ZERO);
                                let pts = Ticks(raw_t.0.saturating_sub(delay.0));
                                let def_dur = if raw_tracks[ti].default_duration_ns > 0 {
                                    scale_to_ticks(
                                        i128::from(raw_tracks[ti].default_duration_ns),
                                        1,
                                        1_000_000_000,
                                    )
                                    .unwrap_or(Ticks::ZERO)
                                } else {
                                    Ticks::ZERO
                                };
                                if samples.len().saturating_add(frames.len()) > MAX_SAMPLES {
                                    bail!("total de amostras MKV excede o limite");
                                }
                                let is_audio = raw_tracks[ti].kind == TrackKind::Audio;
                                for (fi, (off, sz)) in frames.into_iter().enumerate() {
                                    let step = Ticks(def_dur.0.saturating_mul(fi as i64));
                                    let t_f = Ticks(pts.0.saturating_add(step.0));
                                    samples.push(DemuxSample {
                                        track_index: ti,
                                        offset: off,
                                        size: sz,
                                        pts: t_f,
                                        dts: t_f,
                                        duration: def_dur,
                                        is_keyframe: kf && (is_audio || fi == 0),
                                    });
                                }
                            }
                        }
                        ID_BLOCK_GROUP => {
                            let mut bg_pos = c_start;
                            let mut bg_block: Option<BlockFrames> = None;
                            let mut bg_dur: Option<u64> = None;
                            let mut has_ref = false;
                            while let Some((gid, g_start, g_end)) =
                                read_elem_header_at(r, bg_pos, c_end)?
                            {
                                match gid {
                                    ID_BLOCK => {
                                        bg_block =
                                            Some(parse_block_frames(r, g_start, g_end, false)?);
                                    }
                                    ID_BLOCK_DURATION => {
                                        let sz = g_end.saturating_sub(g_start).min(16) as usize;
                                        let mut buf = vec![0u8; sz];
                                        r.seek(SeekFrom::Start(g_start))?;
                                        r.read_exact(&mut buf)?;
                                        bg_dur = Some(ebml_uint(&buf)?);
                                    }
                                    ID_REFERENCE_BLOCK => {
                                        has_ref = true;
                                    }
                                    _ => {}
                                }
                                bg_pos = g_end;
                            }
                            if let Some(bf) = bg_block {
                                let (tnum, rel_ts, frames) = (bf.track_num, bf.rel_ts, bf.frames);
                                if let Some(ti) = raw_tracks.iter().position(|t| t.number == tnum) {
                                    let abs_units = cluster_ts.saturating_add(i128::from(rel_ts));
                                    let delay = scale_to_ticks(
                                        i128::from(raw_tracks[ti].codec_delay_ns),
                                        1,
                                        1_000_000_000,
                                    )
                                    .unwrap_or(Ticks::ZERO);
                                    let raw_t =
                                        scale_to_ticks(abs_units, ts_scale_ns, 1_000_000_000)
                                            .unwrap_or(Ticks::ZERO);
                                    let pts = Ticks(raw_t.0.saturating_sub(delay.0));
                                    let dur_ticks = if let Some(d_units) = bg_dur {
                                        let n_f = (frames.len() as i64).max(1);
                                        let total_t = scale_to_ticks(
                                            i128::from(d_units),
                                            ts_scale_ns,
                                            1_000_000_000,
                                        )
                                        .unwrap_or(Ticks::ZERO);
                                        Ticks(total_t.0 / n_f)
                                    } else if raw_tracks[ti].default_duration_ns > 0 {
                                        scale_to_ticks(
                                            i128::from(raw_tracks[ti].default_duration_ns),
                                            1,
                                            1_000_000_000,
                                        )
                                        .unwrap_or(Ticks::ZERO)
                                    } else {
                                        Ticks::ZERO
                                    };
                                    if samples.len().saturating_add(frames.len()) > MAX_SAMPLES {
                                        bail!("total de amostras MKV excede o limite");
                                    }
                                    let is_audio = raw_tracks[ti].kind == TrackKind::Audio;
                                    for (fi, (off, sz)) in frames.into_iter().enumerate() {
                                        let t_f =
                                            Ticks(pts.0.saturating_add(
                                                dur_ticks.0.saturating_mul(fi as i64),
                                            ));
                                        samples.push(DemuxSample {
                                            track_index: ti,
                                            offset: off,
                                            size: sz,
                                            pts: t_f,
                                            dts: t_f,
                                            duration: dur_ticks,
                                            is_keyframe: !has_ref && (is_audio || fi == 0),
                                        });
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    c_pos = c_end;
                }
                s_pos = cluster_end;
                continue;
            }
            _ => {}
        }
        s_pos = d_end;
    }

    if let Some(d_raw) = seg_dur_raw {
        let total_ns = d_raw * (ts_scale_ns as f64);
        if total_ns.is_finite() && total_ns > 0.0 && total_ns < (i64::MAX as f64) {
            seg_dur_ticks = scale_to_ticks(total_ns.round() as i128, 1, 1_000_000_000);
        }
    }

    let mut tracks = Vec::with_capacity(raw_tracks.len());
    for t in raw_tracks {
        let fps = if t.kind == TrackKind::Video && t.default_duration_ns > 0 {
            rational_fps(1_000_000_000, t.default_duration_ns)
        } else {
            None
        };
        let delay =
            scale_to_ticks(i128::from(t.codec_delay_ns), 1, 1_000_000_000).unwrap_or(Ticks::ZERO);
        tracks.push(DemuxTrack {
            id: u32::try_from(t.number).unwrap_or(u32::MAX),
            kind: t.kind,
            codec: t.codec,
            extra: t.extra,
            timescale: 1_000_000_000u64
                .checked_div(ts_scale_ns.max(1))
                .unwrap_or(1000)
                .max(1),
            duration: seg_dur_ticks,
            width: t.width,
            height: t.height,
            fps,
            sample_rate: t.sample_rate,
            channels: t.channels,
            codec_delay: delay,
        });
    }

    Ok(DemuxIndex {
        container,
        duration: seg_dur_ticks,
        tracks,
        samples,
    })
}
