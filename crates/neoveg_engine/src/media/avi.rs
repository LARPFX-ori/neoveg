//! Leitura e escrita de AVI (RIFF) em Rust puro.
//!
//! Leitura: vídeo Motion-JPEG (MJPG) ou RGB não comprimido (BI_RGB 24/32 bits)
//! e áudio PCM (8/16/24/32 bits, float 32). Suporta OpenDML (RIFF AVIX) por
//! varredura das listas `movi`.
//!
//! Escrita: AVI com vídeo MJPEG + áudio PCM 16 bits estéreo e índice `idx1`.

use crate::time::{FrameRate, Ticks};
use anyhow::{anyhow, bail, Context, Result};
use image::RgbaImage;
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq)]
enum VideoCodec {
    Mjpeg,
    Rgb24,
    Rgb32,
}

#[derive(Clone, Debug)]
pub struct AviAudioInfo {
    pub format_tag: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub bits: u16,
}

pub struct AviReader {
    file: File,
    pub width: u32,
    pub height: u32,
    pub fps: FrameRate,
    codec: Option<VideoCodec>,
    /// Quadros: (offset absoluto dos dados, tamanho). Tamanho 0 = repetir o anterior.
    frames: Vec<(u64, u32)>,
    pub audio: Option<AviAudioInfo>,
    audio_chunks: Vec<(u64, u32)>,
    pub video_fourcc: String,
}

/// Maior lista `hdrl` aceita.
const MAX_HDRL: u64 = 16 << 20;
/// Maior chunk (quadro ou bloco de áudio) aceito.
const MAX_CHUNK: u32 = 256 << 20;
/// Maior dimensão de vídeo aceita na leitura.
const MAX_DIM: u32 = 16384;
/// Profundidade máxima de listas aninhadas em `movi` (§0.5).
const MAX_DEPTH: u32 = 4;

fn rd_u32(b: &[u8], o: usize) -> u32 {
    match b.get(o..o + 4) {
        Some(x) => u32::from_le_bytes([x[0], x[1], x[2], x[3]]),
        None => 0,
    }
}
fn rd_i32(b: &[u8], o: usize) -> i32 {
    rd_u32(b, o) as i32
}
fn rd_u16(b: &[u8], o: usize) -> u16 {
    match b.get(o..o + 2) {
        Some(x) => u16::from_le_bytes([x[0], x[1]]),
        None => 0,
    }
}

#[derive(Default)]
struct StreamHdr {
    kind: [u8; 4],
    handler: [u8; 4],
    scale: u32,
    rate: u32,
    strf: Vec<u8>,
}

impl AviReader {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path).with_context(|| format!("abrindo {}", path.display()))?;
        let file_len = file.metadata()?.len();
        let mut hdr = [0u8; 12];
        file.read_exact(&mut hdr)?;
        if &hdr[0..4] != b"RIFF" || &hdr[8..12] != b"AVI " {
            bail!("não é um arquivo AVI");
        }
        let mut streams: Vec<StreamHdr> = vec![];
        let mut movi_lists: Vec<(u64, u64)> = vec![]; // (início dos dados, fim)

        // percorre RIFFs de topo (AVI e AVIX)
        let mut riff_pos = 0u64;
        while riff_pos + 12 <= file_len {
            file.seek(SeekFrom::Start(riff_pos))?;
            let mut h = [0u8; 12];
            if file.read_exact(&mut h).is_err() {
                break;
            }
            if &h[0..4] != b"RIFF" {
                break;
            }
            let riff_size = rd_u32(&h, 4) as u64;
            let riff_end = (riff_pos + 8 + riff_size).min(file_len);
            let mut pos = riff_pos + 12;
            while pos + 8 <= riff_end {
                file.seek(SeekFrom::Start(pos))?;
                let mut ch = [0u8; 12];
                if file.read_exact(&mut ch[..8]).is_err() {
                    break;
                }
                let size = rd_u32(&ch, 4) as u64;
                if &ch[0..4] == b"LIST" {
                    file.read_exact(&mut ch[8..12])?;
                    let list_type = [ch[8], ch[9], ch[10], ch[11]];
                    if &list_type == b"hdrl" {
                        // limite: o cabeçalho vem do arquivo (entrada hostil, §0.4)
                        let hdrl_len = size
                            .saturating_sub(4)
                            .min(file_len.saturating_sub(pos + 12));
                        if hdrl_len > MAX_HDRL {
                            bail!("cabeçalho AVI grande demais ({hdrl_len} bytes)");
                        }
                        let mut buf = vec![0u8; hdrl_len as usize];
                        file.read_exact(&mut buf)?;
                        parse_hdrl(&buf, &mut streams);
                    } else if &list_type == b"movi" {
                        movi_lists.push((pos + 12, (pos + 8 + size).min(riff_end)));
                    }
                }
                pos += 8 + size + (size & 1);
            }
            riff_pos = riff_end + (riff_end & 1);
            if riff_end >= file_len {
                break;
            }
        }

        let mut r = AviReader {
            file,
            width: 0,
            height: 0,
            fps: FrameRate::FPS_30,
            codec: None,
            frames: vec![],
            audio: None,
            audio_chunks: vec![],
            video_fourcc: String::new(),
        };
        let mut video_idx = None;
        let mut audio_idx = None;
        for (i, s) in streams.iter().enumerate() {
            if &s.kind == b"vids" && video_idx.is_none() && s.strf.len() >= 40 {
                let w = rd_i32(&s.strf, 4);
                let h = rd_i32(&s.strf, 8);
                let bits = rd_u16(&s.strf, 14);
                let comp = [
                    rd_u8(&s.strf, 16),
                    rd_u8(&s.strf, 17),
                    rd_u8(&s.strf, 18),
                    rd_u8(&s.strf, 19),
                ];
                r.width = w.unsigned_abs();
                r.height = h.unsigned_abs();
                if r.width == 0 || r.height == 0 || r.width > MAX_DIM || r.height > MAX_DIM {
                    bail!("dimensões de vídeo inválidas: {}x{}", r.width, r.height);
                }
                if let Some(fps) = FrameRate::new(s.rate, s.scale) {
                    r.fps = fps;
                }
                let handler = String::from_utf8_lossy(&s.handler).to_string();
                let comps = String::from_utf8_lossy(&comp).to_string();
                r.video_fourcc = if comp == [0, 0, 0, 0] {
                    "RGB".into()
                } else {
                    comps.clone()
                };
                let up = comps.to_ascii_uppercase();
                r.codec = if up == "MJPG"
                    || up == "AVRN"
                    || up == "LJPG"
                    || up == "JPEG"
                    || handler.eq_ignore_ascii_case("mjpg") && comp != [0, 0, 0, 0]
                {
                    Some(VideoCodec::Mjpeg)
                } else if comp == [0, 0, 0, 0] && bits == 24 {
                    Some(VideoCodec::Rgb24)
                } else if comp == [0, 0, 0, 0] && bits == 32 {
                    Some(VideoCodec::Rgb32)
                } else {
                    None
                };
                // altura positiva = imagem de baixo para cima (só importa em RGB)

                video_idx = Some(i);
            } else if &s.kind == b"auds" && audio_idx.is_none() && s.strf.len() >= 16 {
                let mut tag = rd_u16(&s.strf, 0);
                let bits = rd_u16(&s.strf, 14);
                if tag == 0xFFFE && s.strf.len() >= 40 {
                    // WAVE_FORMAT_EXTENSIBLE: subformato nos 2 primeiros bytes do GUID
                    tag = rd_u16(&s.strf, 24);
                }
                r.audio = Some(AviAudioInfo {
                    format_tag: tag,
                    channels: rd_u16(&s.strf, 2),
                    sample_rate: rd_u32(&s.strf, 4),
                    bits,
                });
                audio_idx = Some(i);
            }
        }

        // varre as listas movi
        let vtag = video_idx.and_then(stream_tag);
        let atag = audio_idx.and_then(stream_tag);
        for (start, end) in movi_lists {
            scan_movi(
                &mut r.file,
                start,
                end,
                &vtag,
                &atag,
                &mut r.frames,
                &mut r.audio_chunks,
                0,
            )?;
        }
        Ok(r)
    }

    pub fn has_video(&self) -> bool {
        self.codec.is_some() && !self.frames.is_empty()
    }

    pub fn video_supported_msg(&self) -> Option<String> {
        if self.codec.is_none() && !self.video_fourcc.is_empty() {
            Some(format!(
                "codec de vídeo '{}' não suportado (use MJPEG ou RGB não comprimido)",
                self.video_fourcc
            ))
        } else {
            None
        }
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn duration(&self) -> Ticks {
        self.fps.frame_to_ticks(self.frames.len() as i64)
    }

    fn read_chunk(&mut self, off: u64, size: u32) -> Result<Vec<u8>> {
        let len = self.file.metadata()?.len();
        if size > MAX_CHUNK || off.saturating_add(size as u64) > len {
            bail!("chunk AVI inválido (offset {off}, tamanho {size})");
        }
        let mut buf = vec![0u8; size as usize];
        self.file.seek(SeekFrom::Start(off))?;
        self.file.read_exact(&mut buf)?;
        Ok(buf)
    }

    pub fn decode_frame(&mut self, idx: usize) -> Result<RgbaImage> {
        if self.frames.is_empty() {
            bail!("sem quadros de vídeo");
        }
        let mut i = idx.min(self.frames.len() - 1);
        while i > 0 && self.frames[i].1 == 0 {
            i -= 1;
        }
        let (off, size) = self.frames[i];
        if size == 0 {
            bail!("quadro vazio");
        }
        let data = self.read_chunk(off, size)?;
        match self.codec {
            Some(VideoCodec::Mjpeg) => {
                let img = image::load_from_memory_with_format(&data, image::ImageFormat::Jpeg)
                    .map_err(|e| anyhow!("JPEG inválido no quadro {i}: {e}"))?;
                Ok(img.to_rgba8())
            }
            Some(VideoCodec::Rgb24) | Some(VideoCodec::Rgb32) => {
                let bpp = if self.codec == Some(VideoCodec::Rgb24) {
                    3
                } else {
                    4
                };
                let (w, h) = (self.width as usize, self.height as usize);
                let stride = (w * bpp + 3) & !3;
                if data.len() < stride.saturating_mul(h) {
                    bail!("quadro RGB truncado no quadro {i}");
                }
                let mut img = RgbaImage::new(w as u32, h as u32);
                for y in 0..h {
                    let src_row = h - 1 - y; // baixo para cima
                    let row = &data
                        .get(src_row * stride..src_row * stride + w * bpp)
                        .unwrap_or(&[]);
                    if row.len() < w * bpp {
                        continue;
                    }
                    for (x, p) in row.chunks_exact(bpp).enumerate().take(w) {
                        img.put_pixel(x as u32, y as u32, image::Rgba([p[2], p[1], p[0], 255]));
                    }
                }
                Ok(img)
            }
            None => bail!("codec não suportado"),
        }
    }

    /// Lê todo o áudio PCM, convertido para f32 intercalado (canais originais).
    pub fn read_audio(&mut self) -> Result<Option<(u32, u16, Vec<f32>)>> {
        let Some(info) = self.audio.clone() else {
            return Ok(None);
        };
        if info.format_tag != 1 && info.format_tag != 3 {
            return Ok(None);
        }
        let mut out = Vec::new();
        let chunks = self.audio_chunks.clone();
        for (off, size) in chunks {
            let data = self.read_chunk(off, size)?;
            match (info.format_tag, info.bits) {
                (1, 8) => out.extend(data.iter().map(|&b| (b as f32 - 128.0) / 128.0)),
                (1, 16) => out.extend(
                    data.as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| i16::from_le_bytes(*c) as f32 / 32768.0),
                ),
                (1, 24) => out.extend(data.as_chunks::<3>().0.iter().map(|c| {
                    let v = ((c[2] as i32) << 24 | (c[1] as i32) << 16 | (c[0] as i32) << 8) >> 8;
                    v as f32 / 8_388_608.0
                })),
                (1, 32) => out.extend(
                    data.as_chunks::<4>()
                        .0
                        .iter()
                        .map(|c| i32::from_le_bytes(*c) as f32 / 2_147_483_648.0),
                ),
                (3, 32) => out.extend(
                    data.as_chunks::<4>()
                        .0
                        .iter()
                        .map(|c| f32::from_le_bytes(*c)),
                ),
                _ => return Ok(None),
            }
        }
        Ok(Some((info.sample_rate, info.channels.max(1), out)))
    }
}

fn parse_hdrl(buf: &[u8], streams: &mut Vec<StreamHdr>) {
    let mut pos = 0usize;
    while pos + 8 <= buf.len() {
        let id = &buf[pos..pos + 4];
        let size = rd_u32(buf, pos + 4) as usize;
        let body_start = pos + 8;
        let body_end = (body_start + size).min(buf.len());
        if id == b"LIST"
            && body_end >= body_start + 4
            && &buf[body_start..body_start + 4] == b"strl"
        {
            let mut s = StreamHdr::default();
            let sub = &buf[body_start + 4..body_end];
            let mut p = 0usize;
            while p + 8 <= sub.len() {
                let sid = &sub[p..p + 4];
                let ssize = rd_u32(sub, p + 4) as usize;
                let b = &sub[p + 8..(p + 8 + ssize).min(sub.len())];
                if sid == b"strh" && b.len() >= 28 {
                    s.kind.copy_from_slice(&b[0..4]);
                    s.handler.copy_from_slice(&b[4..8]);
                    s.scale = rd_u32(b, 20);
                    s.rate = rd_u32(b, 24);
                } else if sid == b"strf" {
                    s.strf = b.to_vec();
                }
                p += 8 + ssize + (ssize & 1);
            }
            streams.push(s);
        }
        pos = body_start + size + (size & 1);
    }
}

/// Prefixo de dois dígitos ASCII do chunk de um fluxo (ex.: `00`, `01`).
fn stream_tag(i: usize) -> Option<[u8; 2]> {
    if i > 99 {
        return None;
    }
    Some([b'0' + (i / 10) as u8, b'0' + (i % 10) as u8])
}

fn rd_u8(b: &[u8], o: usize) -> u8 {
    b.get(o).copied().unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
fn scan_movi(
    f: &mut File,
    start: u64,
    end: u64,
    vtag: &Option<[u8; 2]>,
    atag: &Option<[u8; 2]>,
    frames: &mut Vec<(u64, u32)>,
    audio: &mut Vec<(u64, u32)>,
    depth: u32,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let mut pos = start;
    let mut h = [0u8; 12];
    while pos + 8 <= end {
        f.seek(SeekFrom::Start(pos))?;
        if f.read_exact(&mut h[..8]).is_err() {
            break;
        }
        let size = rd_u32(&h, 4) as u64;
        if &h[0..4] == b"LIST" {
            // LIST 'rec ' contém chunks intercalados
            scan_movi(
                f,
                pos + 12,
                (pos + 8 + size).min(end),
                vtag,
                atag,
                frames,
                audio,
                depth + 1,
            )?;
        } else {
            let num = [h[0], h[1]];
            let kind = [h[2], h[3]];
            if Some(num) == *vtag && (&kind == b"dc" || &kind == b"db") {
                frames.push((pos + 8, size as u32));
            } else if Some(num) == *atag && &kind == b"wb" {
                audio.push((pos + 8, size as u32));
            }
        }
        pos += 8 + size + (size & 1);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Escrita
// ---------------------------------------------------------------------------

pub struct AviWriter {
    out: BufWriter<File>,
    pos: u64,
    movi_fourcc_pos: u64,
    index: Vec<([u8; 4], u32, u32)>, // id, offset relativo a 'movi', tamanho
    frames: u32,
    audio_samples: u64,
    has_audio: bool,
    max_chunk: u32,
    // posições para correção no final
    off_riff_size: u64,
    off_movi_size: u64,
    off_total_frames: u64,
    off_vlen: u64,
    off_alen: Option<u64>,
    off_sugg: u64,
}

impl AviWriter {
    pub fn create(
        path: &Path,
        width: u32,
        height: u32,
        fps: FrameRate,
        audio_sr: Option<u32>,
    ) -> Result<Self> {
        let file = File::create(path).with_context(|| format!("criando {}", path.display()))?;
        let (rate, scale) = (fps.num.max(1), fps.den.max(1));
        let us_per_frame = ((scale as u64 * 1_000_000) / rate as u64).max(1) as u32;
        let has_audio = audio_sr.is_some();
        let mut h: Vec<u8> = Vec::new();
        let w32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
        let w16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());

        h.extend_from_slice(b"RIFF");
        let off_riff_size = h.len() as u64;
        w32(&mut h, 0);
        h.extend_from_slice(b"AVI ");

        // hdrl
        h.extend_from_slice(b"LIST");
        let hdrl_size_pos = h.len();
        w32(&mut h, 0);
        h.extend_from_slice(b"hdrl");
        h.extend_from_slice(b"avih");
        w32(&mut h, 56);
        w32(&mut h, us_per_frame);
        w32(&mut h, 0); // max bytes/s
        w32(&mut h, 0); // padding
        w32(&mut h, 0x10 | 0x100); // HASINDEX | ISINTERLEAVED
        let off_total_frames = h.len() as u64;
        w32(&mut h, 0);
        w32(&mut h, 0);
        w32(&mut h, if has_audio { 2 } else { 1 });
        let off_sugg = h.len() as u64;
        w32(&mut h, 0);
        w32(&mut h, width);
        w32(&mut h, height);
        for _ in 0..4 {
            w32(&mut h, 0);
        }

        // strl vídeo
        h.extend_from_slice(b"LIST");
        let strl_pos = h.len();
        w32(&mut h, 0);
        h.extend_from_slice(b"strl");
        h.extend_from_slice(b"strh");
        w32(&mut h, 56);
        h.extend_from_slice(b"vids");
        h.extend_from_slice(b"MJPG");
        w32(&mut h, 0);
        w16(&mut h, 0);
        w16(&mut h, 0);
        w32(&mut h, 0);
        w32(&mut h, scale);
        w32(&mut h, rate);
        w32(&mut h, 0);
        let off_vlen = h.len() as u64;
        w32(&mut h, 0);
        w32(&mut h, width * height * 3);
        w32(&mut h, u32::MAX);
        w32(&mut h, 0);
        w16(&mut h, 0);
        w16(&mut h, 0);
        w16(&mut h, width as u16);
        w16(&mut h, height as u16);
        h.extend_from_slice(b"strf");
        w32(&mut h, 40);
        w32(&mut h, 40);
        w32(&mut h, width);
        w32(&mut h, height);
        w16(&mut h, 1);
        w16(&mut h, 24);
        h.extend_from_slice(b"MJPG");
        w32(&mut h, width * height * 3);
        w32(&mut h, 0);
        w32(&mut h, 0);
        w32(&mut h, 0);
        w32(&mut h, 0);
        let strl_size = (h.len() - strl_pos - 4) as u32;
        h[strl_pos..strl_pos + 4].copy_from_slice(&strl_size.to_le_bytes());

        let mut off_alen = None;
        if let Some(sr) = audio_sr {
            h.extend_from_slice(b"LIST");
            let p = h.len();
            w32(&mut h, 0);
            h.extend_from_slice(b"strl");
            h.extend_from_slice(b"strh");
            w32(&mut h, 56);
            h.extend_from_slice(b"auds");
            w32(&mut h, 0);
            w32(&mut h, 0);
            w16(&mut h, 0);
            w16(&mut h, 0);
            w32(&mut h, 0);
            w32(&mut h, 1); // scale
            w32(&mut h, sr); // rate
            w32(&mut h, 0);
            off_alen = Some(h.len() as u64);
            w32(&mut h, 0);
            w32(&mut h, sr * 4);
            w32(&mut h, u32::MAX);
            w32(&mut h, 4); // sample size (block align)
            for _ in 0..4 {
                w16(&mut h, 0);
            }
            h.extend_from_slice(b"strf");
            w32(&mut h, 16);
            w16(&mut h, 1); // PCM
            w16(&mut h, 2);
            w32(&mut h, sr);
            w32(&mut h, sr * 4);
            w16(&mut h, 4);
            w16(&mut h, 16);
            let s = (h.len() - p - 4) as u32;
            h[p..p + 4].copy_from_slice(&s.to_le_bytes());
        }
        let hdrl_size = (h.len() - hdrl_size_pos - 4) as u32;
        h[hdrl_size_pos..hdrl_size_pos + 4].copy_from_slice(&hdrl_size.to_le_bytes());

        h.extend_from_slice(b"LIST");
        let off_movi_size = h.len() as u64;
        w32(&mut h, 0);
        let movi_fourcc_pos = h.len() as u64;
        h.extend_from_slice(b"movi");

        let mut out = BufWriter::with_capacity(1 << 20, file);
        out.write_all(&h)?;
        Ok(AviWriter {
            out,
            pos: h.len() as u64,
            movi_fourcc_pos,
            index: vec![],
            frames: 0,
            audio_samples: 0,
            has_audio,
            max_chunk: 0,
            off_riff_size,
            off_movi_size,
            off_total_frames,
            off_vlen,
            off_alen,
            off_sugg,
        })
    }

    fn write_chunk(&mut self, id: [u8; 4], data: &[u8]) -> Result<()> {
        if self.pos + data.len() as u64 > u32::MAX as u64 - 1_000_000 {
            bail!("o arquivo AVI ultrapassou o limite de 4 GB; reduza a qualidade ou a duração");
        }
        let rel = (self.pos - self.movi_fourcc_pos) as u32;
        self.out.write_all(&id)?;
        self.out.write_all(&(data.len() as u32).to_le_bytes())?;
        self.out.write_all(data)?;
        let mut n = 8 + data.len() as u64;
        if data.len() & 1 == 1 {
            self.out.write_all(&[0])?;
            n += 1;
        }
        self.index.push((id, rel, data.len() as u32));
        self.max_chunk = self.max_chunk.max(data.len() as u32);
        self.pos += n;
        Ok(())
    }

    pub fn write_video_jpeg(&mut self, jpeg: &[u8]) -> Result<()> {
        self.frames += 1;
        self.write_chunk(*b"00dc", jpeg)
    }

    /// Áudio PCM 16 bits estéreo intercalado.
    pub fn write_audio(&mut self, pcm: &[i16]) -> Result<()> {
        if !self.has_audio || pcm.is_empty() {
            return Ok(());
        }
        let mut bytes = Vec::with_capacity(pcm.len() * 2);
        for s in pcm {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        self.audio_samples += (pcm.len() / 2) as u64;
        self.write_chunk(*b"01wb", &bytes)
    }

    pub fn finish(mut self) -> Result<()> {
        let movi_end = self.pos;
        // idx1
        let mut idx = Vec::with_capacity(self.index.len() * 16);
        for (id, off, size) in &self.index {
            idx.extend_from_slice(id);
            idx.extend_from_slice(&0x10u32.to_le_bytes());
            idx.extend_from_slice(&off.to_le_bytes());
            idx.extend_from_slice(&size.to_le_bytes());
        }
        self.out.write_all(b"idx1")?;
        self.out.write_all(&(idx.len() as u32).to_le_bytes())?;
        self.out.write_all(&idx)?;
        self.pos += 8 + idx.len() as u64;
        self.out.flush()?;
        let mut f = self.out.into_inner().map_err(|e| anyhow!("{e}"))?;
        let mut patch = |off: u64, v: u32| -> Result<()> {
            f.seek(SeekFrom::Start(off))?;
            f.write_all(&v.to_le_bytes())?;
            Ok(())
        };
        patch(self.off_riff_size, (self.pos - 8) as u32)?;
        patch(self.off_movi_size, (movi_end - self.movi_fourcc_pos) as u32)?;
        patch(self.off_total_frames, self.frames)?;
        patch(self.off_vlen, self.frames)?;
        patch(self.off_sugg, self.max_chunk + 8)?;
        if let Some(o) = self.off_alen {
            patch(o, self.audio_samples as u32)?;
        }
        f.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_read() {
        let dir = std::env::temp_dir().join("nvov_avi_test.avi");
        let mut w = AviWriter::create(&dir, 64, 48, FrameRate::FPS_25, Some(48000)).unwrap();
        let img = image::RgbImage::from_pixel(64, 48, image::Rgb([200, 30, 30]));
        let mut jpg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90)
            .encode_image(&img)
            .unwrap();
        for _ in 0..10 {
            w.write_video_jpeg(&jpg).unwrap();
            w.write_audio(&vec![1000i16; 48000 / 25 * 2]).unwrap();
        }
        w.finish().unwrap();
        let mut r = AviReader::open(&dir).unwrap();
        assert_eq!(r.frame_count(), 10);
        assert_eq!((r.width, r.height), (64, 48));
        assert_eq!(r.fps, FrameRate::FPS_25);
        let f = r.decode_frame(3).unwrap();
        assert!(f.get_pixel(10, 10)[0] > 150);
        let (sr, ch, a) = r.read_audio().unwrap().unwrap();
        assert_eq!((sr, ch), (48000, 2));
        assert_eq!(a.len(), 48000 / 25 * 2 * 10);
        let _ = std::fs::remove_file(dir);
    }
}
