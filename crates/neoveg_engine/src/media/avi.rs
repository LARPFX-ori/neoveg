//! Leitura e escrita de AVI (RIFF) em Rust puro.
//!
//! Leitura: vídeo Motion-JPEG (MJPG) ou RGB não comprimido (BI_RGB 24/32 bits)
//! e áudio PCM (8/16/24/32 bits, float 32). Suporta OpenDML (RIFF AVIX) por
//! varredura das listas `movi`.
//!
//! Escrita: AVI com vídeo MJPEG + áudio PCM 16 bits estéreo, sem limite de
//! 4 GB, seguindo a especificação pública *OpenDML AVI File Format Extensions*
//! v1.02 (1996): segmentos `RIFF 'AVIX'` de até 1 GB, super-índice `indx` por
//! stream, índices padrão `ix##` por segmento, `odml/dmlh` com o total de
//! quadros e `idx1` no primeiro segmento para leitores AVI 1.0.

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
//
// OpenDML AVI File Format Extensions v1.02 (1996), §2.0 e §3.0:
//
// - o primeiro segmento é `RIFF 'AVI '` (lido por qualquer leitor AVI, com
//   `idx1` próprio) e os seguintes são `RIFF 'AVIX'` só com `LIST 'movi'`;
//   cada segmento tem no máximo 1 GB;
// - cada stream tem um super-índice `indx` (índice de índices) no `strl`,
//   que aponta para um índice padrão `ix##` gravado no `movi` de cada
//   segmento;
// - `LIST 'odml'` / `dmlh` guarda o total de quadros; `avih` guarda só os
//   quadros do primeiro segmento.

/// Tamanho máximo de cada segmento RIFF (OpenDML 1.02 §2.0: "Each RIFF chunk
/// can have a maximum size of 1 GB").
const SEGMENT_BYTES: u64 = 1 << 30;
/// Entradas reservadas no super-índice de cada stream: com segmentos de 1 GB,
/// cerca de 256 GB por arquivo.
const SUPER_INDEX_ENTRIES: usize = 256;
/// Maior bloco de áudio gravado num chunk (~2,7 s estéreo 16 bits a 48 kHz).
const MAX_AUDIO_CHUNK_BYTES: usize = 512 << 10;
/// Tamanho do `dmlh`: o exemplo da especificação usa 0x54 bytes; só o
/// primeiro DWORD (`dwTotalFrames`) é definido, o resto fica zerado.
const DMLH_BYTES: u32 = 0x54;
/// Cabeçalho de um índice (`indx`/`ix##`) depois de `fcc` e `cb`.
const INDEX_HEADER_BYTES: u64 = 24;

/// Ids dos chunks de dados e dos índices padrão, por stream (0 = vídeo, 1 = áudio).
const DATA_IDS: [[u8; 4]; 2] = [*b"00dc", *b"01wb"];
const IX_IDS: [[u8; 4]; 2] = [*b"ix00", *b"ix01"];

/// Entrada do super-índice: posição absoluta do `ix##`, tamanho do chunk
/// inteiro (com os 8 bytes de `fcc` + `cb`; a especificação não diz se inclui
/// o cabeçalho, e esta é a leitura adotada) e duração em ticks do stream
/// (quadros no vídeo, amostras no áudio).
struct SuperEntry {
    offset: u64,
    size: u32,
    duration: u32,
}

pub struct AviWriter {
    out: BufWriter<File>,
    pos: u64,
    segment_limit: u64,
    super_capacity: usize,
    // segmento atual
    seg_start: u64,
    seg_movi_fourcc: u64,
    seg_riff_size_pos: u64,
    seg_movi_size_pos: u64,
    /// Chunks do segmento atual por stream: (offset absoluto dos dados, tamanho).
    seg_index: [Vec<(u64, u32)>; 2],
    /// Duração do segmento atual por stream, em ticks.
    seg_duration: [u64; 2],
    first_segment: bool,
    /// `idx1` do primeiro segmento: id, offset relativo a 'movi', tamanho.
    first_idx1: Vec<([u8; 4], u32, u32)>,
    first_frames: u64,
    super_index: [Vec<SuperEntry>; 2],
    off_super: [Option<u64>; 2],
    /// Tamanhos a corrigir no fim: (posição, valor).
    patches: Vec<(u64, u32)>,
    frames: u64,
    audio_samples: u64,
    has_audio: bool,
    max_chunk: u32,
    off_total_frames: u64,
    off_vlen: u64,
    off_alen: Option<u64>,
    off_sugg: u64,
    off_dmlh: u64,
}

impl AviWriter {
    pub fn create(
        path: &Path,
        width: u32,
        height: u32,
        fps: FrameRate,
        audio_sr: Option<u32>,
    ) -> Result<Self> {
        Self::create_with_limits(
            path,
            width,
            height,
            fps,
            audio_sr,
            SEGMENT_BYTES,
            SUPER_INDEX_ENTRIES,
        )
    }

    /// Como [`Self::create`], com tamanho de segmento e capacidade do
    /// super-índice escolhidos (os testes forçam vários segmentos pequenos).
    pub(crate) fn create_with_limits(
        path: &Path,
        width: u32,
        height: u32,
        fps: FrameRate,
        audio_sr: Option<u32>,
        segment_limit: u64,
        super_capacity: usize,
    ) -> Result<Self> {
        let file = File::create(path).with_context(|| format!("criando {}", path.display()))?;
        let (rate, scale) = (fps.num.max(1), fps.den.max(1));
        let us_per_frame = ((scale as u64 * 1_000_000) / rate as u64).max(1) as u32;
        let has_audio = audio_sr.is_some();
        let super_capacity = super_capacity.clamp(1, 1 << 16);
        let super_cb = u32::try_from(INDEX_HEADER_BYTES as usize + 16 * super_capacity)
            .map_err(|_| anyhow!("super-índice AVI grande demais"))?;
        let mut h: Vec<u8> = Vec::new();
        let w32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
        let w16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
        let mut off_super = [None, None];
        // `indx` reservado (zerado) logo depois do `strf`; preenchido no fim
        let reserve_indx = |v: &mut Vec<u8>| -> u64 {
            let at = v.len() as u64;
            v.extend_from_slice(b"indx");
            v.extend_from_slice(&super_cb.to_le_bytes());
            v.resize(v.len() + super_cb as usize, 0);
            at
        };

        h.extend_from_slice(b"RIFF");
        let seg_riff_size_pos = h.len() as u64;
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
        off_super[0] = Some(reserve_indx(&mut h));
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
            off_super[1] = Some(reserve_indx(&mut h));
            let s = (h.len() - p - 4) as u32;
            h[p..p + 4].copy_from_slice(&s.to_le_bytes());
        }

        // LIST 'odml' / dmlh (total de quadros, corrigido no fim)
        h.extend_from_slice(b"LIST");
        w32(&mut h, 4 + 8 + DMLH_BYTES);
        h.extend_from_slice(b"odml");
        h.extend_from_slice(b"dmlh");
        w32(&mut h, DMLH_BYTES);
        let off_dmlh = h.len() as u64;
        h.resize(h.len() + DMLH_BYTES as usize, 0);

        let hdrl_size = (h.len() - hdrl_size_pos - 4) as u32;
        h[hdrl_size_pos..hdrl_size_pos + 4].copy_from_slice(&hdrl_size.to_le_bytes());

        h.extend_from_slice(b"LIST");
        let seg_movi_size_pos = h.len() as u64;
        w32(&mut h, 0);
        let seg_movi_fourcc = h.len() as u64;
        h.extend_from_slice(b"movi");

        let mut out = BufWriter::with_capacity(1 << 20, file);
        out.write_all(&h)?;
        Ok(AviWriter {
            out,
            pos: h.len() as u64,
            // o cabeçalho conta no primeiro segmento; deixa margem para ele
            segment_limit: segment_limit.max(h.len() as u64 * 2),
            super_capacity,
            seg_start: 0,
            seg_movi_fourcc,
            seg_riff_size_pos,
            seg_movi_size_pos,
            seg_index: [vec![], vec![]],
            seg_duration: [0, 0],
            first_segment: true,
            first_idx1: vec![],
            first_frames: 0,
            super_index: [vec![], vec![]],
            off_super,
            patches: vec![],
            frames: 0,
            audio_samples: 0,
            has_audio,
            max_chunk: 0,
            off_total_frames,
            off_vlen,
            off_alen,
            off_sugg,
            off_dmlh,
        })
    }

    fn put(&mut self, bytes: &[u8]) -> Result<()> {
        self.out.write_all(bytes)?;
        self.pos += bytes.len() as u64;
        Ok(())
    }

    /// Bytes que ainda serão gravados no fim do segmento atual (índices
    /// `ix##` e, no primeiro, `idx1`), contando um chunk a mais em `extra`.
    fn pending_index_bytes(&self, extra: usize) -> u64 {
        let mut total = 0u64;
        for s in 0..2 {
            let n = self.seg_index[s].len() as u64 + u64::from(s == extra);
            if n > 0 {
                total += 8 + INDEX_HEADER_BYTES + 8 * n;
            }
        }
        if self.first_segment {
            total += 8 + 16 * (self.first_idx1.len() as u64 + 1);
        }
        total
    }

    fn write_chunk(&mut self, stream: usize, data: &[u8], ticks: u64) -> Result<()> {
        let len = u32::try_from(data.len()).map_err(|_| anyhow!("chunk AVI grande demais"))?;
        let chunk_bytes = 8 + data.len() as u64 + (data.len() as u64 & 1);
        // fecha o segmento antes de estourar, contando os índices do fim dele
        let fits = |w: &Self| {
            (w.pos - w.seg_start) + chunk_bytes + w.pending_index_bytes(stream) <= w.segment_limit
        };
        if !fits(self) {
            if self.seg_index.iter().any(|v| !v.is_empty()) {
                self.close_segment()?;
                self.open_avix_segment()?;
            }
            if !fits(self) {
                bail!(
                    "um único chunk AVI ({} bytes) não cabe num segmento de {} bytes",
                    data.len(),
                    self.segment_limit
                );
            }
        }
        let id = DATA_IDS[stream];
        let data_at = self.pos + 8;
        if self.first_segment {
            let rel = u32::try_from(self.pos - self.seg_movi_fourcc)
                .map_err(|_| anyhow!("offset do idx1 fora do limite"))?;
            self.first_idx1.push((id, rel, len));
        }
        self.put(&id)?;
        self.put(&len.to_le_bytes())?;
        self.put(data)?;
        if data.len() & 1 == 1 {
            self.put(&[0])?;
        }
        self.seg_index[stream].push((data_at, len));
        self.seg_duration[stream] += ticks;
        self.max_chunk = self.max_chunk.max(len);
        Ok(())
    }

    /// Grava os índices `ix##` do segmento, fecha `movi` (e, no primeiro
    /// segmento, grava `idx1`) e agenda a correção dos tamanhos.
    fn close_segment(&mut self) -> Result<()> {
        for s in 0..2 {
            let entries = std::mem::take(&mut self.seg_index[s]);
            let duration = std::mem::take(&mut self.seg_duration[s]);
            if entries.is_empty() {
                continue;
            }
            if self.super_index[s].len() >= self.super_capacity {
                bail!(
                    "o AVI passou do limite de {} segmentos de índice",
                    self.super_capacity
                );
            }
            let base = self.seg_start;
            let cb = INDEX_HEADER_BYTES + 8 * entries.len() as u64;
            let cb32 = u32::try_from(cb).map_err(|_| anyhow!("índice AVI grande demais"))?;
            let at = self.pos;
            let mut ix = Vec::with_capacity(8 + cb as usize);
            ix.extend_from_slice(&IX_IDS[s]);
            ix.extend_from_slice(&cb32.to_le_bytes());
            ix.extend_from_slice(&2u16.to_le_bytes()); // wLongsPerEntry
            ix.push(0); // bIndexSubType
            ix.push(1); // bIndexType = AVI_INDEX_OF_CHUNKS
            ix.extend_from_slice(&(entries.len() as u32).to_le_bytes());
            ix.extend_from_slice(&DATA_IDS[s]);
            ix.extend_from_slice(&base.to_le_bytes()); // qwBaseOffset
            ix.extend_from_slice(&0u32.to_le_bytes()); // dwReserved3
            for (data_at, size) in &entries {
                let rel = u32::try_from(data_at - base)
                    .map_err(|_| anyhow!("offset do índice AVI fora do limite"))?;
                ix.extend_from_slice(&rel.to_le_bytes());
                // bit 31 zerado: todo quadro MJPEG e todo bloco PCM é chave
                ix.extend_from_slice(&size.to_le_bytes());
            }
            self.put(&ix)?;
            self.super_index[s].push(SuperEntry {
                offset: at,
                size: cb32 + 8,
                duration: u32::try_from(duration).unwrap_or(u32::MAX),
            });
        }
        let movi_size = u32::try_from(self.pos - self.seg_movi_fourcc)
            .map_err(|_| anyhow!("lista movi grande demais"))?;
        self.patches.push((self.seg_movi_size_pos, movi_size));
        if self.first_segment {
            let idx = std::mem::take(&mut self.first_idx1);
            let mut b = Vec::with_capacity(8 + idx.len() * 16);
            b.extend_from_slice(b"idx1");
            b.extend_from_slice(&((idx.len() * 16) as u32).to_le_bytes());
            for (id, off, size) in &idx {
                b.extend_from_slice(id);
                b.extend_from_slice(&0x10u32.to_le_bytes()); // AVIIF_KEYFRAME
                b.extend_from_slice(&off.to_le_bytes());
                b.extend_from_slice(&size.to_le_bytes());
            }
            self.put(&b)?;
            self.first_segment = false;
        }
        let riff_size = u32::try_from(self.pos - self.seg_start - 8)
            .map_err(|_| anyhow!("segmento RIFF grande demais"))?;
        self.patches.push((self.seg_riff_size_pos, riff_size));
        Ok(())
    }

    fn open_avix_segment(&mut self) -> Result<()> {
        // cada segmento acrescenta uma entrada por stream ao super-índice
        if self
            .super_index
            .iter()
            .any(|v| v.len() >= self.super_capacity)
        {
            bail!(
                "o AVI passou do limite de {} segmentos de índice",
                self.super_capacity
            );
        }
        self.seg_start = self.pos;
        self.seg_riff_size_pos = self.pos + 4;
        self.seg_movi_size_pos = self.pos + 16;
        self.seg_movi_fourcc = self.pos + 20;
        self.put(b"RIFF\0\0\0\0AVIXLIST\0\0\0\0movi")?;
        Ok(())
    }

    pub fn write_video_jpeg(&mut self, jpeg: &[u8]) -> Result<()> {
        self.write_chunk(0, jpeg, 1)?;
        self.frames += 1;
        if self.first_segment {
            self.first_frames += 1;
        }
        Ok(())
    }

    /// Áudio PCM 16 bits estéreo intercalado. Blocos grandes são divididos em
    /// chunks limitados (nenhum chunk passa de um segmento nem do limite de
    /// leitura).
    pub fn write_audio(&mut self, pcm: &[i16]) -> Result<()> {
        if !self.has_audio || pcm.is_empty() {
            return Ok(());
        }
        let cap_bytes = (MAX_AUDIO_CHUNK_BYTES as u64)
            .min(self.segment_limit / 4)
            .max(4) as usize;
        let per_chunk = (cap_bytes / 4).max(1) * 2; // amostras i16, sempre pares
        for part in pcm.chunks(per_chunk) {
            let mut bytes = Vec::with_capacity(part.len() * 2);
            for s in part {
                bytes.extend_from_slice(&s.to_le_bytes());
            }
            let frames = (part.len() / 2) as u64;
            self.write_chunk(1, &bytes, frames)?;
            self.audio_samples += frames;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        self.close_segment()?;
        self.out.flush()?;
        let mut f = self.out.into_inner().map_err(|e| anyhow!("{e}"))?;
        let mut patch = |off: u64, v: &[u8]| -> Result<()> {
            f.seek(SeekFrom::Start(off))?;
            f.write_all(v)?;
            Ok(())
        };
        let sat = |v: u64| u32::try_from(v).unwrap_or(u32::MAX);
        for (off, v) in &self.patches {
            patch(*off, &v.to_le_bytes())?;
        }
        patch(self.off_total_frames, &sat(self.first_frames).to_le_bytes())?;
        patch(self.off_vlen, &sat(self.frames).to_le_bytes())?;
        patch(self.off_dmlh, &sat(self.frames).to_le_bytes())?;
        patch(
            self.off_sugg,
            &self.max_chunk.saturating_add(8).to_le_bytes(),
        )?;
        if let Some(o) = self.off_alen {
            patch(o, &sat(self.audio_samples).to_le_bytes())?;
        }
        for ((off, entries), id) in self.off_super.iter().zip(&self.super_index).zip(&DATA_IDS) {
            let Some(at) = *off else {
                continue;
            };
            let mut b = Vec::with_capacity(INDEX_HEADER_BYTES as usize + entries.len() * 16);
            b.extend_from_slice(&4u16.to_le_bytes()); // wLongsPerEntry
            b.push(0); // bIndexSubType
            b.push(0); // bIndexType = AVI_INDEX_OF_INDEXES
            b.extend_from_slice(&(entries.len() as u32).to_le_bytes());
            b.extend_from_slice(id);
            b.extend_from_slice(&[0u8; 12]); // dwReserved[3]
            for e in entries {
                b.extend_from_slice(&e.offset.to_le_bytes());
                b.extend_from_slice(&e.size.to_le_bytes());
                b.extend_from_slice(&e.duration.to_le_bytes());
            }
            // depois de `fcc` + `cb` do `indx` reservado
            patch(at + 8, &b)?;
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

    fn jpeg_frame(v: u8) -> Vec<u8> {
        let img =
            image::RgbImage::from_fn(64, 48, |x, y| image::Rgb([v, (x * 4) as u8, (y * 5) as u8]));
        let mut jpg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90)
            .encode_image(&img)
            .unwrap();
        jpg
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("nvov_odml_{}_{name}.avi", std::process::id()))
    }

    /// Grava `n` quadros com áudio em segmentos de `limit` bytes.
    fn write_segmented(path: &Path, n: usize, limit: u64) -> (u64, u64) {
        let mut w = AviWriter::create_with_limits(
            path,
            64,
            48,
            FrameRate::FPS_25,
            Some(48000),
            limit,
            SUPER_INDEX_ENTRIES,
        )
        .unwrap();
        for i in 0..n {
            w.write_video_jpeg(&jpeg_frame((i * 7) as u8)).unwrap();
            w.write_audio(&vec![i as i16; 48000 / 25 * 2]).unwrap();
        }
        w.finish().unwrap();
        (n as u64, (n * 48000 / 25) as u64)
    }

    fn u32_at(b: &[u8], o: usize) -> u32 {
        u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
    }

    fn u64_at(b: &[u8], o: usize) -> u64 {
        u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
    }

    /// Posições dos chunks `indx` dentro de `LIST hdrl` / `LIST strl`.
    fn find_super_indexes(b: &[u8]) -> Vec<usize> {
        let mut out = vec![];
        let hdrl = 12; // depois de RIFF <tamanho> AVI
        assert_eq!(&b[hdrl..hdrl + 4], b"LIST");
        assert_eq!(&b[hdrl + 8..hdrl + 12], b"hdrl");
        let end = hdrl + 8 + u32_at(b, hdrl + 4) as usize;
        let mut p = hdrl + 12;
        while p + 8 <= end {
            let size = u32_at(b, p + 4) as usize;
            if &b[p..p + 4] == b"LIST" && &b[p + 8..p + 12] == b"strl" {
                let mut q = p + 12;
                while q + 8 <= p + 8 + size {
                    let s = u32_at(b, q + 4) as usize;
                    if &b[q..q + 4] == b"indx" {
                        out.push(q);
                    }
                    q += 8 + s + (s & 1);
                }
            }
            p += 8 + size + (size & 1);
        }
        out
    }

    #[test]
    fn opendml_segments_round_trip() {
        let path = tmp("ida_volta");
        let (frames, samples) = write_segmented(&path, 60, 64 << 10);
        let bytes = std::fs::read(&path).unwrap();
        let avix = bytes.windows(4).filter(|w| *w == b"AVIX").count();
        assert!(avix >= 2, "esperava vários segmentos AVIX, achou {avix}");
        let first_riff = u64::from(u32_at(&bytes, 4)) + 8;
        assert!(first_riff <= 64 << 10, "1.º segmento passou do limite");
        let mut r = AviReader::open(&path).unwrap();
        assert_eq!(r.frame_count() as u64, frames);
        let last = r.decode_frame(59).unwrap();
        assert_eq!((last.width(), last.height()), (64, 48));
        let (_, ch, a) = r.read_audio().unwrap().unwrap();
        assert_eq!(a.len() as u64, samples * u64::from(ch));
        let _ = std::fs::remove_file(path);
    }

    /// Segue `indx` -> `ix##` -> chunk de dados e confere ids, tamanhos e
    /// durações (OpenDML 1.02 §3.0). O ffprobe cai na varredura do `movi`
    /// quando o índice está errado, então é este teste que prova o índice.
    #[test]
    fn opendml_index_walk_lands_on_every_chunk() {
        let path = tmp("indice");
        let (frames, samples) = write_segmented(&path, 60, 64 << 10);
        let b = std::fs::read(&path).unwrap();
        let supers = find_super_indexes(&b);
        assert_eq!(supers.len(), 2, "um indx por stream");
        let mut totals = vec![];
        for (s, at) in supers.iter().enumerate() {
            let h = at + 8;
            assert_eq!(u32_at(&b, h) & 0xFFFF, 4, "wLongsPerEntry");
            assert_eq!(b[h + 3], 0, "AVI_INDEX_OF_INDEXES");
            let n = u32_at(&b, h + 4) as usize;
            assert_eq!(&b[h + 8..h + 12], &DATA_IDS[s]);
            assert!(n >= 2, "stream {s}: {n} entradas");
            let mut total_dur = 0u64;
            let mut total_chunks = 0usize;
            for e in 0..n {
                let ent = h + 24 + e * 16;
                let ix = u64_at(&b, ent) as usize;
                let ix_size = u32_at(&b, ent + 8) as usize;
                total_dur += u64::from(u32_at(&b, ent + 12));
                assert_eq!(&b[ix..ix + 4], &IX_IDS[s]);
                assert_eq!(u32_at(&b, ix + 4) as usize + 8, ix_size);
                assert_eq!(b[ix + 11], 1, "AVI_INDEX_OF_CHUNKS");
                let count = u32_at(&b, ix + 12) as usize;
                assert_eq!(&b[ix + 16..ix + 20], &DATA_IDS[s]);
                let base = u64_at(&b, ix + 20) as usize;
                assert_eq!(
                    &b[base..base + 4],
                    b"RIFF",
                    "qwBaseOffset no início do segmento"
                );
                for k in 0..count {
                    let off = u32_at(&b, ix + 32 + k * 8) as usize;
                    let size = u32_at(&b, ix + 36 + k * 8);
                    let data = base + off;
                    assert_eq!(&b[data - 8..data - 4], &DATA_IDS[s], "entrada {k}");
                    assert_eq!(u32_at(&b, data - 4), size & 0x7FFF_FFFF);
                    assert_eq!(size & 0x8000_0000, 0, "todo chunk é chave");
                }
                total_chunks += count;
            }
            assert!(total_chunks > 0);
            totals.push(total_dur);
        }
        assert_eq!(totals, vec![frames, samples]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn opendml_full_super_index_is_an_error() {
        let path = tmp("cheio");
        let mut w =
            AviWriter::create_with_limits(&path, 64, 48, FrameRate::FPS_25, None, 32 << 10, 2)
                .unwrap();
        let mut failed = false;
        for i in 0..400 {
            if w.write_video_jpeg(&jpeg_frame(i as u8)).is_err() {
                failed = true;
                break;
            }
        }
        assert!(
            failed,
            "o 3.º segmento devia ser recusado com super-índice de 2 entradas"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn large_audio_block_is_split_into_bounded_chunks() {
        let path = tmp("audio");
        let mut w = AviWriter::create_with_limits(
            &path,
            64,
            48,
            FrameRate::FPS_25,
            Some(48000),
            64 << 10,
            SUPER_INDEX_ENTRIES,
        )
        .unwrap();
        w.write_video_jpeg(&jpeg_frame(1)).unwrap();
        // 5 s de áudio numa chamada só (como o resto do áudio na conversão)
        w.write_audio(&vec![3i16; 5 * 48000 * 2]).unwrap();
        w.finish().unwrap();
        let mut r = AviReader::open(&path).unwrap();
        assert!(r
            .audio_chunks
            .iter()
            .all(|(_, s)| u64::from(*s) <= 16 << 10));
        let (_, _, a) = r.read_audio().unwrap().unwrap();
        assert_eq!(a.len(), 5 * 48000 * 2);
        let _ = std::fs::remove_file(path);
    }

    /// Oráculo externo (AGENTS.md §2.3): o ffprobe lê o AVI segmentado sem
    /// reclamar e conta todos os quadros. Pula sem ffprobe.
    #[test]
    fn opendml_file_passes_ffprobe() {
        let path = tmp("ffprobe");
        let (frames, _) = write_segmented(&path, 60, 64 << 10);
        let out = std::process::Command::new("ffprobe")
            .args(["-v", "error", "-count_frames", "-select_streams", "v:0"])
            .args(["-show_entries", "stream=nb_read_frames", "-of", "csv=p=0"])
            .arg(&path)
            .output();
        let _ = std::fs::remove_file(&path);
        let Ok(out) = out else {
            return;
        };
        assert!(out.status.success());
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.trim().is_empty(), "ffprobe reclamou: {stderr}");
        let n = String::from_utf8_lossy(&out.stdout).trim().to_string();
        assert_eq!(n, frames.to_string());
    }
}
