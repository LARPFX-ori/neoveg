//! Decodificadores e escritores adicionais de áudio em Rust puro (Sala Limpa, AGENTS.md §2.3):
//! - **Sun / NeXT AU / SND** (`.au`, `.snd`): PCM 8/16/24/32 BE, Float 32/64 BE, ITU-T G.711 µ-law/A-law;
//! - **Apple Core Audio Format** (`.caf`): especificação pública Apple CAF (`caff`, `desc`, `data`);
//! - **Sony Wave64** (`.w64`): especificação pública Sony 64-bit WAVE (`riff`/`wave`/`fmt `/`data` GUIDs);
//! - **Broadcast Wave (`.bwf`) & EBU RF64 (`.rf64`)**: EBU Tech 3285 (`bext`) e Tech 3306 (`ds64`);
//! - **AIFF / AIFF-C escritor** (`.aif`, `.aiff`): Apple AIFF 1.3 (`NONE` 16/24b) e AIFF-C (`fl32`);
//! - **Creative Voice** (`.voc`): blocos 0, 1, 2, 6, 7, 8 e 9;
//! - **Amiga IFF 8SVX** (`.8svx`, `.svx`): EA IFF 85 `8SVX` (PCM 8b e *Fibonacci Delta*);
//! - **Quite OK Audio** (`.qoa`): especificação aberta QOA v1 (preditor Sign-Sign LMS em blocos de 20 amostras).

use super::wav::{alaw_to_f32, build, parse_wave_fmt_body, ulaw_to_f32, Pcm, SampleFmt};
use crate::render::WavBits;
use anyhow::{anyhow, bail, Context, Result};
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

fn rd_u16be(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|x| u16::from_be_bytes([x[0], x[1]]))
}
fn rd_u32be(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4)
        .map(|x| u32::from_be_bytes([x[0], x[1], x[2], x[3]]))
}
fn rd_i64be(b: &[u8], o: usize) -> Option<i64> {
    b.get(o..o + 8)
        .map(|x| i64::from_be_bytes([x[0], x[1], x[2], x[3], x[4], x[5], x[6], x[7]]))
}
fn rd_u64le(b: &[u8], o: usize) -> Option<u64> {
    b.get(o..o + 8)
        .map(|x| u64::from_le_bytes([x[0], x[1], x[2], x[3], x[4], x[5], x[6], x[7]]))
}
fn rd_u16le(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|x| u16::from_le_bytes([x[0], x[1]]))
}
fn rd_u32le(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4)
        .map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}

/// Decodifica um dos formatos extras de áudio a partir de bytes em memória.
pub fn decode_bytes(ext: &str, bytes: &[u8], max_samples: usize) -> Result<Pcm> {
    match ext.to_ascii_lowercase().as_str() {
        "au" | "snd" => decode_au(bytes, max_samples),
        "caf" => decode_caf(bytes, max_samples),
        "w64" => decode_w64(bytes, max_samples),
        "voc" => decode_voc(bytes, max_samples),
        "8svx" | "svx" => decode_8svx(bytes, max_samples),
        "qoa" => decode_qoa(bytes, max_samples),
        other => bail!("formato extra de áudio desconhecido: .{other}"),
    }
}

// ============================================================================
// 1. Sun / NeXT AU / SND (.au, .snd)
// ============================================================================

pub fn decode_au(b: &[u8], max_samples: usize) -> Result<Pcm> {
    if b.get(0..4) != Some(b".snd") || b.len() < 24 {
        bail!("não é um arquivo Sun/NeXT AU (.snd) válido");
    }
    let offset = rd_u32be(b, 4).unwrap_or(0) as usize;
    let size = rd_u32be(b, 8).unwrap_or(u32::MAX);
    let encoding = rd_u32be(b, 12).unwrap_or(0);
    let rate = rd_u32be(b, 16).unwrap_or(0);
    let channels = u16::try_from(rd_u32be(b, 20).unwrap_or(0))
        .map_err(|_| anyhow!("número de canais AU inválido"))?;
    if offset < 24 || offset > b.len() {
        bail!("offset de dados AU inválido: {offset}");
    }
    let payload = if size == u32::MAX {
        &b[offset..]
    } else {
        let end = offset.saturating_add(size as usize).min(b.len());
        &b[offset..end]
    };
    let fmt = match encoding {
        1 => SampleFmt::Ulaw,
        2 => SampleFmt::Int {
            bits: 8,
            big_endian: true,
        },
        3 => SampleFmt::Int {
            bits: 16,
            big_endian: true,
        },
        4 => SampleFmt::Int {
            bits: 24,
            big_endian: true,
        },
        5 => SampleFmt::Int {
            bits: 32,
            big_endian: true,
        },
        6 => SampleFmt::Float {
            bits: 32,
            big_endian: true,
        },
        7 => SampleFmt::Float {
            bits: 64,
            big_endian: true,
        },
        27 => SampleFmt::Alaw,
        other => bail!("codificação AU não suportada: {other}"),
    };
    build(fmt, channels, rate, payload, max_samples)
}

pub struct AuWriter {
    out: BufWriter<File>,
    bits: WavBits,
    data_bytes: u64,
}

impl AuWriter {
    pub fn create(path: &Path, sr: u32, bits: WavBits, channels: u16) -> Result<Self> {
        if channels == 0 || channels > 64 {
            bail!("AU com {channels} canais inválido");
        }
        let mut out = BufWriter::new(
            File::create(path).with_context(|| format!("criando {}", path.display()))?,
        );
        let encoding: u32 = match bits {
            WavBits::Pcm16 => 3,
            WavBits::Pcm24 => 4,
            WavBits::Float32 => 6,
        };
        out.write_all(b".snd")?;
        out.write_all(&24u32.to_be_bytes())?; // data offset
        out.write_all(&u32::MAX.to_be_bytes())?; // size desconhecido até finish()
        out.write_all(&encoding.to_be_bytes())?;
        out.write_all(&sr.to_be_bytes())?;
        out.write_all(&(channels as u32).to_be_bytes())?;
        Ok(Self {
            out,
            bits,
            data_bytes: 0,
        })
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        for &s in samples {
            let s = if s.is_finite() { s } else { 0.0 };
            match self.bits {
                WavBits::Pcm16 => {
                    let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                    self.out.write_all(&v.to_be_bytes())?;
                    self.data_bytes += 2;
                }
                WavBits::Pcm24 => {
                    let v = (s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32;
                    let be = v.to_be_bytes();
                    self.out.write_all(&be[1..4])?;
                    self.data_bytes += 3;
                }
                WavBits::Float32 => {
                    self.out.write_all(&s.to_be_bytes())?;
                    self.data_bytes += 4;
                }
            }
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        self.out.flush()?;
        let mut f = self.out.into_inner().map_err(|e| anyhow!("{e}"))?;
        if let Ok(sz) = u32::try_from(self.data_bytes) {
            f.seek(SeekFrom::Start(8))?;
            f.write_all(&sz.to_be_bytes())?;
        }
        Ok(())
    }
}

// ============================================================================
// 2. Apple Core Audio Format (.caf)
// ============================================================================

pub fn decode_caf(b: &[u8], max_samples: usize) -> Result<Pcm> {
    if b.get(0..4) != Some(b"caff") || b.len() < 8 {
        bail!("não é um arquivo Apple CAF válido");
    }
    let version = rd_u16be(b, 4).unwrap_or(0);
    if version != 1 {
        bail!("versão CAF não suportada: {version}");
    }

    let mut pos = 8usize;
    let mut fmt_spec: Option<(SampleFmt, u16, u32)> = None;
    let mut data: Option<&[u8]> = None;

    while pos + 12 <= b.len() {
        let typ = b.get(pos..pos + 4).unwrap_or(&[]);
        let sz_raw = rd_i64be(b, pos + 4).unwrap_or(0);
        let start = pos + 12;
        let payload = if sz_raw < 0 {
            &b[start..]
        } else {
            let sz = usize::try_from(sz_raw).unwrap_or(b.len());
            let end = start.saturating_add(sz).min(b.len());
            &b[start..end]
        };

        if typ == b"desc" && payload.len() >= 32 {
            let mut sr_bytes = [0u8; 8];
            sr_bytes.copy_from_slice(&payload[0..8]);
            let sr_f = f64::from_be_bytes(sr_bytes);
            if !sr_f.is_finite() || !(1.0..=1_000_000.0).contains(&sr_f) {
                bail!("taxa de amostragem CAF inválida: {sr_f}");
            }
            let rate = sr_f.round() as u32;
            let fmt_id = payload.get(8..12).unwrap_or(&[]);
            let flags = rd_u32be(payload, 12).unwrap_or(0);
            let ch = u16::try_from(rd_u32be(payload, 24).unwrap_or(0))
                .map_err(|_| anyhow!("canais CAF inválidos"))?;
            let bits = u16::try_from(rd_u32be(payload, 28).unwrap_or(0))
                .map_err(|_| anyhow!("bits CAF inválidos"))?;
            let is_float = (flags & 1) != 0;
            let is_le = (flags & 2) != 0;
            let sf = match (fmt_id, is_float, bits) {
                (b"lpcm", false, 8 | 16 | 24 | 32) => SampleFmt::Int {
                    bits,
                    big_endian: !is_le,
                },
                (b"lpcm", true, 32 | 64) => SampleFmt::Float {
                    bits,
                    big_endian: !is_le,
                },
                (b"ulaw", _, _) => SampleFmt::Ulaw,
                (b"alaw", _, _) => SampleFmt::Alaw,
                _ => bail!(
                    "codec CAF não suportado: '{}' ({bits} bits)",
                    String::from_utf8_lossy(fmt_id)
                ),
            };
            fmt_spec = Some((sf, ch, rate));
        } else if typ == b"data" && payload.len() >= 4 {
            // Primeiros 4 bytes de `data` em CAF são `mEditCount`.
            data = Some(&payload[4..]);
        }

        if sz_raw < 0 || (fmt_spec.is_some() && data.is_some()) {
            break;
        }
        let sz = usize::try_from(sz_raw).unwrap_or(b.len());
        pos = start.saturating_add(sz);
    }

    let Some((sf, ch, rate)) = fmt_spec else {
        bail!("CAF sem chunk 'desc'");
    };
    let Some(data) = data else {
        bail!("CAF sem chunk 'data'");
    };
    build(sf, ch, rate, data, max_samples)
}

pub struct CafWriter {
    out: BufWriter<File>,
    bits: WavBits,
    data_size_pos: u64,
    data_bytes: i64,
}

impl CafWriter {
    pub fn create(path: &Path, sr: u32, bits: WavBits, channels: u16) -> Result<Self> {
        if channels == 0 || channels > 64 {
            bail!("CAF com {channels} canais inválido");
        }
        let mut out = BufWriter::new(
            File::create(path).with_context(|| format!("criando {}", path.display()))?,
        );
        let bps: u32 = match bits {
            WavBits::Pcm16 => 16,
            WavBits::Pcm24 => 24,
            WavBits::Float32 => 32,
        };
        let bpp = (channels as u32) * (bps / 8);
        // flags: bit 1 = LittleEndian (2), bit 0 = Float (1)
        let flags: u32 = match bits {
            WavBits::Pcm16 | WavBits::Pcm24 => 2,
            WavBits::Float32 => 3,
        };
        out.write_all(b"caff")?;
        out.write_all(&1u16.to_be_bytes())?; // version
        out.write_all(&0u16.to_be_bytes())?; // flags

        // chunk 'desc' (32 bytes)
        out.write_all(b"desc")?;
        out.write_all(&32i64.to_be_bytes())?;
        out.write_all(&(sr as f64).to_be_bytes())?;
        out.write_all(b"lpcm")?;
        out.write_all(&flags.to_be_bytes())?;
        out.write_all(&bpp.to_be_bytes())?;
        out.write_all(&1u32.to_be_bytes())?; // framesPerPacket
        out.write_all(&(channels as u32).to_be_bytes())?;
        out.write_all(&bps.to_be_bytes())?;

        // chunk 'data' (+4 bytes mEditCount)
        out.write_all(b"data")?;
        let data_size_pos = 8 + 12 + 32 + 4;
        out.write_all(&(-1i64).to_be_bytes())?;
        out.write_all(&0u32.to_be_bytes())?; // mEditCount = 0

        Ok(Self {
            out,
            bits,
            data_size_pos,
            data_bytes: 0,
        })
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        for &s in samples {
            let s = if s.is_finite() { s } else { 0.0 };
            match self.bits {
                WavBits::Pcm16 => {
                    let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                    self.out.write_all(&v.to_le_bytes())?;
                    self.data_bytes += 2;
                }
                WavBits::Pcm24 => {
                    let v = (s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32;
                    self.out.write_all(&v.to_le_bytes()[..3])?;
                    self.data_bytes += 3;
                }
                WavBits::Float32 => {
                    self.out.write_all(&s.to_le_bytes())?;
                    self.data_bytes += 4;
                }
            }
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        self.out.flush()?;
        let mut f = self.out.into_inner().map_err(|e| anyhow!("{e}"))?;
        f.seek(SeekFrom::Start(self.data_size_pos))?;
        f.write_all(&(4 + self.data_bytes).to_be_bytes())?;
        Ok(())
    }
}

// ============================================================================
// 3. Sony Wave64 (.w64)
// ============================================================================

const W64_GUID_RIFF: [u8; 16] = [
    0x72, 0x69, 0x66, 0x66, 0x2E, 0x91, 0xCF, 0x11, 0xA5, 0xD6, 0x28, 0xDB, 0x04, 0xC1, 0x00, 0x00,
];
const W64_GUID_WAVE: [u8; 16] = [
    0x77, 0x61, 0x76, 0x65, 0xF3, 0xAC, 0xD3, 0x11, 0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E, 0xDB, 0x8A,
];
const W64_GUID_FMT: [u8; 16] = [
    0x66, 0x6D, 0x74, 0x20, 0xF3, 0xAC, 0xD3, 0x11, 0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E, 0xDB, 0x8A,
];
const W64_GUID_DATA: [u8; 16] = [
    0x64, 0x61, 0x74, 0x61, 0xF3, 0xAC, 0xD3, 0x11, 0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E, 0xDB, 0x8A,
];

pub fn decode_w64(b: &[u8], max_samples: usize) -> Result<Pcm> {
    if b.len() < 40 || b.get(0..16) != Some(&W64_GUID_RIFF) || b.get(24..40) != Some(&W64_GUID_WAVE)
    {
        bail!("não é um arquivo Sony Wave64 válido");
    }
    let mut pos = 40usize;
    let mut fmt = None;
    let mut data: Option<&[u8]> = None;

    while pos + 24 <= b.len() {
        let guid = b.get(pos..pos + 16).unwrap_or(&[]);
        let chunk_size = rd_u64le(b, pos + 16).unwrap_or(0);
        if chunk_size < 24 {
            bail!("tamanho de chunk Wave64 inválido: {chunk_size}");
        }
        let body_start = pos + 24;
        let payload_len = usize::try_from(chunk_size - 24).unwrap_or(b.len());
        let body_end = body_start.saturating_add(payload_len).min(b.len());
        let body = b.get(body_start..body_end).unwrap_or(&[]);
        if guid == W64_GUID_FMT {
            fmt = Some(parse_wave_fmt_body(body)?);
        } else if guid == W64_GUID_DATA {
            data = Some(body);
        }
        if fmt.is_some() && data.is_some() {
            break;
        }
        let aligned = payload_len.saturating_add((8 - (payload_len % 8)) % 8);
        pos = body_start.saturating_add(aligned);
    }

    let Some(wf) = fmt else {
        bail!("Wave64 sem chunk fmt");
    };
    let Some(data) = data else {
        bail!("Wave64 sem chunk data");
    };
    build(wf.fmt, wf.channels, wf.sample_rate, data, max_samples)
}

pub struct W64Writer {
    out: BufWriter<File>,
    bits: WavBits,
    data_bytes: u64,
    data_size_pos: u64,
}

impl W64Writer {
    pub fn create_with_channels(
        path: &Path,
        sr: u32,
        bits: WavBits,
        channels: u16,
    ) -> Result<Self> {
        if channels != 2 && channels != 6 {
            bail!("Wave64 com {channels} canais não suportado (use 2 ou 6)");
        }
        let mut out = BufWriter::new(
            File::create(path).with_context(|| format!("criando {}", path.display()))?,
        );
        let (fmt_tag, bps): (u16, u16) = match bits {
            WavBits::Pcm16 => (1, 16),
            WavBits::Pcm24 => (1, 24),
            WavBits::Float32 => (3, 32),
        };
        let block = channels * bps / 8;
        let extensible = channels > 2;
        let fmt_body_len: u64 = if extensible { 40 } else { 16 };

        out.write_all(&W64_GUID_RIFF)?;
        out.write_all(&0u64.to_le_bytes())?; // total size preenchido em finish()
        out.write_all(&W64_GUID_WAVE)?;

        out.write_all(&W64_GUID_FMT)?;
        out.write_all(&(24 + fmt_body_len).to_le_bytes())?;
        let tag = if extensible { 0xFFFE } else { fmt_tag };
        out.write_all(&tag.to_le_bytes())?;
        out.write_all(&channels.to_le_bytes())?;
        out.write_all(&sr.to_le_bytes())?;
        out.write_all(&sr.saturating_mul(block as u32).to_le_bytes())?;
        out.write_all(&block.to_le_bytes())?;
        out.write_all(&bps.to_le_bytes())?;
        if extensible {
            out.write_all(&22u16.to_le_bytes())?;
            out.write_all(&bps.to_le_bytes())?;
            out.write_all(&0x3Fu32.to_le_bytes())?; // 5.1 mask
            out.write_all(&u32::from(fmt_tag).to_le_bytes())?;
            out.write_all(&[
                0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
            ])?;
        }

        let data_chunk_start = 40 + 24 + fmt_body_len;
        out.write_all(&W64_GUID_DATA)?;
        let data_size_pos = data_chunk_start + 16;
        out.write_all(&24u64.to_le_bytes())?;

        Ok(Self {
            out,
            bits,
            data_bytes: 0,
            data_size_pos,
        })
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        write_le_samples(&mut self.out, samples, self.bits, &mut self.data_bytes)
    }

    pub fn finish(mut self) -> Result<()> {
        let pad = (8 - (self.data_bytes % 8)) % 8;
        if pad > 0 {
            self.out.write_all(&vec![0u8; pad as usize])?;
        }
        self.out.flush()?;
        let mut f = self.out.into_inner().map_err(|e| anyhow!("{e}"))?;
        let total = self.data_size_pos + 8 + self.data_bytes + pad;
        f.seek(SeekFrom::Start(16))?;
        f.write_all(&total.to_le_bytes())?;
        f.seek(SeekFrom::Start(self.data_size_pos))?;
        f.write_all(&(24 + self.data_bytes).to_le_bytes())?;
        Ok(())
    }
}

fn write_le_samples(
    out: &mut BufWriter<File>,
    samples: &[f32],
    bits: WavBits,
    counter: &mut u64,
) -> Result<()> {
    for &s in samples {
        let s = if s.is_finite() { s } else { 0.0 };
        match bits {
            WavBits::Pcm16 => {
                let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                out.write_all(&v.to_le_bytes())?;
                *counter += 2;
            }
            WavBits::Pcm24 => {
                let v = (s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32;
                out.write_all(&v.to_le_bytes()[..3])?;
                *counter += 3;
            }
            WavBits::Float32 => {
                out.write_all(&s.to_le_bytes())?;
                *counter += 4;
            }
        }
    }
    Ok(())
}

// ============================================================================
// 4. Broadcast Wave (BWF .bwf) & EBU RF64 (.rf64)
// ============================================================================

/// Escritor Broadcast Wave Format (EBU Tech 3285, RIFF WAVE com chunk `bext`)
/// e RF64 (EBU Tech 3306, cabeçalho 64 bits `ds64`).
pub struct ExtWaveWriter {
    out: BufWriter<File>,
    bits: WavBits,
    channels: u16,
    rf64: bool,
    prefix_bytes: u64,
    data_size_pos: u64,
    data_bytes: u64,
}

impl ExtWaveWriter {
    pub fn create_bwf(path: &Path, sr: u32, bits: WavBits, channels: u16) -> Result<Self> {
        Self::create_inner(path, sr, bits, channels, false)
    }

    pub fn create_rf64(path: &Path, sr: u32, bits: WavBits, channels: u16) -> Result<Self> {
        Self::create_inner(path, sr, bits, channels, true)
    }

    fn create_inner(
        path: &Path,
        sr: u32,
        bits: WavBits,
        channels: u16,
        rf64: bool,
    ) -> Result<Self> {
        if channels != 2 && channels != 6 {
            bail!("BWF/RF64 com {channels} canais não suportado (use 2 ou 6)");
        }
        let mut out = BufWriter::new(
            File::create(path).with_context(|| format!("criando {}", path.display()))?,
        );
        let (fmt_tag, bps): (u16, u16) = match bits {
            WavBits::Pcm16 => (1, 16),
            WavBits::Pcm24 => (1, 24),
            WavBits::Float32 => (3, 32),
        };
        let block = channels * bps / 8;
        let extensible = channels > 2;
        let fmt_len: u32 = if extensible { 40 } else { 16 };

        if rf64 {
            out.write_all(b"RF64")?;
            out.write_all(&u32::MAX.to_le_bytes())?;
            out.write_all(b"WAVEds64")?;
            out.write_all(&28u32.to_le_bytes())?;
            out.write_all(&[0u8; 28])?;
        } else {
            out.write_all(b"RIFF")?;
            out.write_all(&0u32.to_le_bytes())?;
            out.write_all(b"WAVE")?;
        }

        // Chunk `bext` EBU Tech 3285 (602 bytes fixos, coding history vazio)
        let mut bext = [0u8; 602];
        bext[..15].copy_from_slice(b"Neoveg Export\0\0");
        bext[256..262].copy_from_slice(b"Neoveg");
        bext[320..330].copy_from_slice(b"2026-01-01");
        bext[330..338].copy_from_slice(b"00:00:00");
        bext[346..348].copy_from_slice(&1u16.to_le_bytes()); // BWF version 1
        out.write_all(b"bext")?;
        out.write_all(&(bext.len() as u32).to_le_bytes())?;
        out.write_all(&bext)?;

        out.write_all(b"fmt ")?;
        out.write_all(&fmt_len.to_le_bytes())?;
        let tag = if extensible { 0xFFFE } else { fmt_tag };
        out.write_all(&tag.to_le_bytes())?;
        out.write_all(&channels.to_le_bytes())?;
        out.write_all(&sr.to_le_bytes())?;
        out.write_all(&sr.saturating_mul(block as u32).to_le_bytes())?;
        out.write_all(&block.to_le_bytes())?;
        out.write_all(&bps.to_le_bytes())?;
        if extensible {
            out.write_all(&22u16.to_le_bytes())?;
            out.write_all(&bps.to_le_bytes())?;
            out.write_all(&0x3Fu32.to_le_bytes())?;
            out.write_all(&u32::from(fmt_tag).to_le_bytes())?;
            out.write_all(&[
                0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
            ])?;
        }

        let ds64_part = if rf64 { 8 + 28 } else { 0 };
        let bext_part = 8 + bext.len() as u64;
        let prefix_bytes = 4 + ds64_part + bext_part + 8 + fmt_len as u64 + 8;
        let data_size_pos = 8 + prefix_bytes - 4;
        out.write_all(b"data")?;
        out.write_all(&(if rf64 { u32::MAX } else { 0 }).to_le_bytes())?;

        Ok(Self {
            out,
            bits,
            channels,
            rf64,
            prefix_bytes,
            data_size_pos,
            data_bytes: 0,
        })
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        write_le_samples(&mut self.out, samples, self.bits, &mut self.data_bytes)
    }

    pub fn finish(mut self) -> Result<()> {
        if self.data_bytes % 2 == 1 {
            self.out.write_all(&[0u8])?;
        }
        self.out.flush()?;
        let mut f = self.out.into_inner().map_err(|e| anyhow!("{e}"))?;
        let bps = match self.bits {
            WavBits::Pcm16 => 2u64,
            WavBits::Pcm24 => 3,
            WavBits::Float32 => 4,
        };
        let total_riff = self.prefix_bytes + self.data_bytes;
        if self.rf64 {
            let sample_count = self.data_bytes / (bps * self.channels as u64);
            f.seek(SeekFrom::Start(20))?; // ds64 payload
            f.write_all(&total_riff.to_le_bytes())?;
            f.write_all(&self.data_bytes.to_le_bytes())?;
            f.write_all(&sample_count.to_le_bytes())?;
            if let Ok(d32) = u32::try_from(self.data_bytes) {
                if total_riff <= u32::MAX as u64 {
                    f.seek(SeekFrom::Start(self.data_size_pos))?;
                    f.write_all(&d32.to_le_bytes())?;
                }
            }
        } else {
            let riff32 = u32::try_from(total_riff)
                .map_err(|_| anyhow!("BWF acima de 4 GiB requer formato RF64"))?;
            f.seek(SeekFrom::Start(4))?;
            f.write_all(&riff32.to_le_bytes())?;
            f.seek(SeekFrom::Start(self.data_size_pos))?;
            f.write_all(&(self.data_bytes as u32).to_le_bytes())?;
        }
        Ok(())
    }
}

// ============================================================================
// 5. Apple AIFF / AIFF-C escritor (.aiff)
// ============================================================================

fn write_extended80(sr: u32) -> [u8; 10] {
    let mut out = [0u8; 10];
    if sr == 0 {
        return out;
    }
    let f = sr as f64;
    let bits = f.to_bits();
    let exp_f64 = ((bits >> 52) & 0x7FF) as i32 - 1023;
    let mant_f64 = (bits & ((1u64 << 52) - 1)) | (1u64 << 52);
    let exp_ext = (exp_f64 + 16383) as u16;
    let mant_ext = mant_f64 << 11;
    out[0..2].copy_from_slice(&exp_ext.to_be_bytes());
    out[2..10].copy_from_slice(&mant_ext.to_be_bytes());
    out
}

pub struct AiffWriter {
    out: BufWriter<File>,
    bits: WavBits,
    channels: u16,
    frames_written: u64,
    data_bytes: u64,
    frames_pos: u64,
    ssnd_size_pos: u64,
}

impl AiffWriter {
    pub fn create_with_channels(
        path: &Path,
        sr: u32,
        bits: WavBits,
        channels: u16,
    ) -> Result<Self> {
        if channels == 0 || channels > 6 {
            bail!("AIFF com {channels} canais não suportado");
        }
        let mut out = BufWriter::new(
            File::create(path).with_context(|| format!("criando {}", path.display()))?,
        );
        let aifc = bits == WavBits::Float32;
        let bps: u16 = match bits {
            WavBits::Pcm16 => 16,
            WavBits::Pcm24 => 24,
            WavBits::Float32 => 32,
        };
        out.write_all(b"FORM")?;
        out.write_all(&0u32.to_be_bytes())?;
        out.write_all(if aifc { b"AIFC" } else { b"AIFF" })?;
        if aifc {
            out.write_all(b"FVER")?;
            out.write_all(&4u32.to_be_bytes())?;
            out.write_all(&0xA280_5140u32.to_be_bytes())?;
        }
        out.write_all(b"COMM")?;
        let comm_len: u32 = if aifc { 24 } else { 18 };
        out.write_all(&comm_len.to_be_bytes())?;
        let frames_pos = if aifc { 12 + 12 + 8 + 2 } else { 12 + 8 + 2 };
        out.write_all(&channels.to_be_bytes())?;
        out.write_all(&0u32.to_be_bytes())?; // numSampleFrames
        out.write_all(&bps.to_be_bytes())?;
        out.write_all(&write_extended80(sr))?;
        if aifc {
            out.write_all(b"fl32")?;
            out.write_all(&[0u8, 0u8])?; // pstring vazia alinhada
        }
        out.write_all(b"SSND")?;
        let ssnd_size_pos = if aifc {
            12 + 12 + 8 + 24 + 4
        } else {
            12 + 8 + 18 + 4
        };
        out.write_all(&8u32.to_be_bytes())?;
        out.write_all(&0u32.to_be_bytes())?; // offset
        out.write_all(&0u32.to_be_bytes())?; // blockSize
        Ok(Self {
            out,
            bits,
            channels,
            frames_written: 0,
            data_bytes: 0,
            frames_pos,
            ssnd_size_pos,
        })
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        for &s in samples {
            let s = if s.is_finite() { s } else { 0.0 };
            match self.bits {
                WavBits::Pcm16 => {
                    let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                    self.out.write_all(&v.to_be_bytes())?;
                    self.data_bytes += 2;
                }
                WavBits::Pcm24 => {
                    let v = (s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32;
                    let be = v.to_be_bytes();
                    self.out.write_all(&be[1..4])?;
                    self.data_bytes += 3;
                }
                WavBits::Float32 => {
                    self.out.write_all(&s.to_be_bytes())?;
                    self.data_bytes += 4;
                }
            }
        }
        self.frames_written += (samples.len() / self.channels as usize) as u64;
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        if self.data_bytes % 2 == 1 {
            self.out.write_all(&[0u8])?;
        }
        self.out.flush()?;
        let mut f = self.out.into_inner().map_err(|e| anyhow!("{e}"))?;
        let file_len = f.seek(SeekFrom::End(0))?;
        let form_sz = u32::try_from(file_len.saturating_sub(8))
            .map_err(|_| anyhow!("AIFF excede limite de 4 GiB"))?;
        f.seek(SeekFrom::Start(4))?;
        f.write_all(&form_sz.to_be_bytes())?;
        f.seek(SeekFrom::Start(self.frames_pos))?;
        f.write_all(&(self.frames_written as u32).to_be_bytes())?;
        f.seek(SeekFrom::Start(self.ssnd_size_pos))?;
        f.write_all(&((8 + self.data_bytes) as u32).to_be_bytes())?;
        Ok(())
    }
}

// ============================================================================
// 6. Creative Voice (.voc)
// ============================================================================

pub fn decode_voc(b: &[u8], max_samples: usize) -> Result<Pcm> {
    if b.len() < 26 || b.get(0..20) != Some(b"Creative Voice File\x1A") {
        bail!("não é um arquivo Creative Voice (.voc) válido");
    }
    let data_offset = rd_u16le(b, 20).unwrap_or(0) as usize;
    let version = rd_u16le(b, 22).unwrap_or(0);
    let check = rd_u16le(b, 24).unwrap_or(0);
    if (!version).wrapping_add(0x1234) != check {
        bail!("checksum do cabeçalho VOC inválido");
    }
    if data_offset < 26 || data_offset > b.len() {
        bail!("offset de dados VOC inválido");
    }

    let mut pos = data_offset;
    let mut sr = 0u32;
    let mut ch = 1u16;
    let mut active_codec = 0u8;
    let mut active_bits = 8u8;
    let mut ext_override = false;
    let mut samples = Vec::new();
    let mut blocks = 0usize;

    while pos < b.len() && blocks < 100_000 && samples.len() < max_samples {
        blocks += 1;
        let blk_type = b[pos];
        pos += 1;
        if blk_type == 0 {
            break;
        }
        let Some(sz_bytes) = b.get(pos..pos + 3) else {
            break;
        };
        pos += 3;
        let sz =
            (sz_bytes[0] as usize) | ((sz_bytes[1] as usize) << 8) | ((sz_bytes[2] as usize) << 16);
        let payload = b
            .get(pos..pos.saturating_add(sz).min(b.len()))
            .unwrap_or(&[]);
        pos = pos.saturating_add(sz);

        match blk_type {
            1 if payload.len() >= 2 => {
                if !ext_override {
                    let freq = payload[0] as u32;
                    sr = (1_000_000u32 / (256u32.saturating_sub(freq)).max(1)).clamp(4000, 192_000);
                    active_codec = payload[1];
                    active_bits = if active_codec == 4 { 16 } else { 8 };
                    ch = 1;
                }
                ext_override = false;
                decode_voc_payload(
                    &payload[2..],
                    active_codec,
                    active_bits,
                    &mut samples,
                    max_samples,
                )?;
            }
            2 => {
                decode_voc_payload(
                    payload,
                    active_codec,
                    active_bits,
                    &mut samples,
                    max_samples,
                )?;
            }
            8 if payload.len() >= 4 => {
                let tc = rd_u16le(payload, 0).unwrap_or(0) as u32;
                let pack = payload[2];
                let stereo = payload[3] == 1;
                ch = if stereo { 2 } else { 1 };
                let denom = (65536u32.saturating_sub(tc)).max(1);
                sr = (256_000_000u64 / ((ch as u64) * denom as u64)).clamp(4000, 192_000) as u32;
                active_codec = pack;
                active_bits = if pack == 4 { 16 } else { 8 };
                ext_override = true;
            }
            9 if payload.len() >= 12 => {
                sr = rd_u32le(payload, 0).unwrap_or(0).clamp(1, 1_000_000);
                active_bits = payload[4];
                ch = (payload[5] as u16).clamp(1, 64);
                let wfmt = rd_u16le(payload, 6).unwrap_or(0);
                active_codec = match wfmt {
                    0 => 0,
                    4 | 1 if active_bits == 16 => 4,
                    6 | 2 => 6,
                    7 | 3 => 7,
                    _ => bail!("formato VOC bloco 9 não suportado: {wfmt}"),
                };
                decode_voc_payload(
                    &payload[12..],
                    active_codec,
                    active_bits,
                    &mut samples,
                    max_samples,
                )?;
            }
            _ => {}
        }
    }
    if samples.is_empty() || sr == 0 {
        bail!("VOC sem dados de áudio decodificáveis");
    }
    let rem = samples.len() % ch as usize;
    samples.truncate(samples.len() - rem);
    Ok(Pcm {
        sample_rate: sr,
        channels: ch,
        samples,
    })
}

fn decode_voc_payload(
    data: &[u8],
    codec: u8,
    bits: u8,
    out: &mut Vec<f32>,
    max_samples: usize,
) -> Result<()> {
    match (codec, bits) {
        (0, 8) => {
            for &b in data.iter().take(max_samples.saturating_sub(out.len())) {
                out.push((b as f32 - 128.0) / 128.0);
            }
        }
        (4, 16) => {
            for c in data
                .as_chunks::<2>()
                .0
                .iter()
                .take(max_samples.saturating_sub(out.len()))
            {
                let v = i16::from_le_bytes(*c);
                out.push(v as f32 / 32768.0);
            }
        }
        (6, _) => {
            for &b in data.iter().take(max_samples.saturating_sub(out.len())) {
                out.push(alaw_to_f32(b));
            }
        }
        (7, _) => {
            for &b in data.iter().take(max_samples.saturating_sub(out.len())) {
                out.push(ulaw_to_f32(b));
            }
        }
        _ => bail!("codec VOC não suportado (codec={codec}, bits={bits})"),
    }
    Ok(())
}

// ============================================================================
// 7. Amiga IFF 8SVX (.8svx, .svx)
// ============================================================================

const FIB_DELTA_TAB: [i8; 16] = [-34, -21, -13, -8, -5, -3, -2, -1, 0, 1, 2, 3, 5, 8, 13, 21];

pub fn decode_8svx(b: &[u8], max_samples: usize) -> Result<Pcm> {
    if b.get(0..4) != Some(b"FORM") || b.get(8..12) != Some(b"8SVX") {
        bail!("não é um arquivo EA IFF 8SVX válido");
    }
    let mut pos = 12usize;
    let mut sr = 0u32;
    let mut comp = 0u8;
    let mut stereo = false;
    let mut body: Option<&[u8]> = None;

    while pos + 8 <= b.len() {
        let id = b.get(pos..pos + 4).unwrap_or(&[]);
        let size = rd_u32be(b, pos + 4).unwrap_or(0) as usize;
        let start = pos + 8;
        let end = start.saturating_add(size).min(b.len());
        let chunk = b.get(start..end).unwrap_or(&[]);
        if id == b"VHDR" && chunk.len() >= 16 {
            sr = rd_u16be(chunk, 12).unwrap_or(0) as u32;
            comp = chunk[15];
        } else if id == b"CHAN" && chunk.len() >= 4 {
            let mask = rd_u32be(chunk, 0).unwrap_or(0);
            stereo = mask == 6;
        } else if id == b"BODY" {
            body = Some(chunk);
        }
        pos = start.saturating_add(size).saturating_add(size & 1);
    }

    let Some(body) = body else {
        bail!("8SVX sem chunk BODY");
    };
    if sr == 0 || sr > 1_000_000 {
        bail!("taxa de amostragem 8SVX inválida: {sr}");
    }

    let pcm8: Vec<i8> = match comp {
        0 => body.iter().take(max_samples).map(|&u| u as i8).collect(),
        1 => {
            if body.len() < 2 {
                bail!("BODY Fibonacci-Delta 8SVX truncado");
            }
            let mut x = body[1] as i8;
            let mut v = Vec::with_capacity((body.len() - 2) * 2);
            for &byte in &body[2..] {
                if v.len() >= max_samples {
                    break;
                }
                x = x.wrapping_add(FIB_DELTA_TAB[(byte >> 4) as usize]);
                v.push(x);
                if v.len() < max_samples {
                    x = x.wrapping_add(FIB_DELTA_TAB[(byte & 0xF) as usize]);
                    v.push(x);
                }
            }
            v
        }
        _ => bail!("compressão 8SVX não suportada: {comp}"),
    };

    let ch = if stereo && pcm8.len() >= 2 { 2 } else { 1 };
    let mut out = Vec::with_capacity(pcm8.len());
    if ch == 2 {
        let half = pcm8.len() / 2;
        for i in 0..half {
            out.push(pcm8[i] as f32 / 128.0);
            out.push(pcm8[half + i] as f32 / 128.0);
        }
    } else {
        for s in pcm8 {
            out.push(s as f32 / 128.0);
        }
    }
    Ok(Pcm {
        sample_rate: sr,
        channels: ch,
        samples: out,
    })
}

// ============================================================================
// 8. Quite OK Audio (.qoa) — Especificação QOA v1 (qoaformat.org)
// ============================================================================

const QOA_SLICE_LEN: usize = 20;
const QOA_SLICES_PER_FRAME: usize = 256;
const QOA_MAX_CHANNELS: usize = 8;

const QOA_QUANT_TAB: [u8; 17] = [7, 7, 7, 5, 5, 3, 3, 1, 0, 0, 2, 2, 4, 4, 6, 6, 6];
const QOA_SCALEFACTOR_TAB: [i32; 16] = [
    1, 7, 21, 45, 84, 138, 211, 304, 421, 562, 731, 928, 1157, 1419, 1715, 2048,
];
const QOA_DEQUANT_TAB: [[i32; 8]; 16] = [
    [1, -1, 3, -3, 5, -5, 7, -7],
    [5, -5, 18, -18, 32, -32, 49, -49],
    [16, -16, 53, -53, 95, -95, 147, -147],
    [34, -34, 113, -113, 203, -203, 315, -315],
    [63, -63, 210, -210, 378, -378, 588, -588],
    [104, -104, 348, -348, 627, -627, 973, -973],
    [158, -158, 528, -528, 950, -950, 1477, -1477],
    [228, -228, 760, -760, 1368, -1368, 2128, -2128],
    [316, -316, 1053, -1053, 1895, -1895, 2947, -2947],
    [424, -424, 1415, -1415, 2547, -2547, 3962, -3962],
    [554, -554, 1848, -1848, 3327, -3327, 5173, -5173],
    [708, -708, 2360, -2360, 4248, -4248, 6608, -6608],
    [888, -888, 2960, -2960, 5328, -5328, 8288, -8288],
    [1096, -1096, 3653, -3653, 6575, -6575, 10223, -10223],
    [1334, -1334, 4448, -4448, 8006, -8006, 12450, -12450],
    [1604, -1604, 5348, -5348, 9627, -9627, 14973, -14973],
];

#[derive(Clone, Copy, Default)]
struct QoaLms {
    history: [i32; 4],
    weights: [i32; 4],
}

impl QoaLms {
    fn predict(&self) -> i32 {
        let mut p = 0i32;
        for i in 0..4 {
            p = p.wrapping_add(self.weights[i].wrapping_mul(self.history[i]));
        }
        p >> 13
    }

    fn update(&mut self, sample: i32, residual: i32) {
        let delta = residual >> 4;
        for i in 0..4 {
            if self.history[i] < 0 {
                self.weights[i] = self.weights[i].wrapping_sub(delta);
            } else {
                self.weights[i] = self.weights[i].wrapping_add(delta);
            }
        }
        self.history[0] = self.history[1];
        self.history[1] = self.history[2];
        self.history[2] = self.history[3];
        self.history[3] = sample;
    }
}

pub fn decode_qoa(b: &[u8], max_samples: usize) -> Result<Pcm> {
    if b.len() < 16 || b.get(0..4) != Some(b"qoaf") {
        bail!("não é um arquivo QOA válido (magic qoaf ausente)");
    }
    let total_frames = rd_u32be(b, 4).unwrap_or(0) as usize;
    let mut pos = 8usize;
    let mut sr = 0u32;
    let mut channels = 0u16;
    let mut out = Vec::new();

    while pos + 8 <= b.len() && out.len() < max_samples {
        let ch = b[pos] as usize;
        let frame_sr = ((b[pos + 1] as u32) << 16) | ((b[pos + 2] as u32) << 8) | b[pos + 3] as u32;
        let fsamples = rd_u16be(b, pos + 4).unwrap_or(0) as usize;
        let fsize = rd_u16be(b, pos + 6).unwrap_or(0) as usize;
        if ch == 0 || ch > QOA_MAX_CHANNELS || frame_sr == 0 || frame_sr > 1_000_000 {
            bail!("cabeçalho de quadro QOA inválido (ch={ch}, sr={frame_sr})");
        }
        let header_bytes = 8 + ch * 16;
        if fsize < header_bytes || pos + fsize > b.len() {
            bail!("quadro QOA truncado ou com tamanho inválido");
        }
        if sr == 0 {
            sr = frame_sr;
            channels = ch as u16;
        }
        let mut p = pos + 8;
        let mut lms = [QoaLms::default(); QOA_MAX_CHANNELS];
        for c in lms.iter_mut().take(ch) {
            for h in &mut c.history {
                *h = rd_u16be(b, p).unwrap_or(0) as i16 as i32;
                p += 2;
            }
            for w in &mut c.weights {
                *w = rd_u16be(b, p).unwrap_or(0) as i16 as i32;
                p += 2;
            }
        }
        let frame_base = out.len();
        out.resize(frame_base + fsamples * ch, 0.0);
        let mut sample_idx = 0usize;
        while sample_idx < fsamples && p + ch * 8 <= pos + fsize {
            for c in 0..ch {
                let mut slice_bytes = [0u8; 8];
                slice_bytes.copy_from_slice(&b[p..p + 8]);
                p += 8;
                let mut slice = u64::from_be_bytes(slice_bytes);
                let scalefactor = ((slice >> 60) & 0xF) as usize;
                let slice_end = (sample_idx + QOA_SLICE_LEN).min(fsamples);
                for s in sample_idx..slice_end {
                    let predicted = lms[c].predict();
                    let quantized = ((slice >> 57) & 0x7) as usize;
                    let dequantized = QOA_DEQUANT_TAB[scalefactor][quantized];
                    let reconstructed = (predicted + dequantized).clamp(-32768, 32767);
                    lms[c].update(reconstructed, dequantized);
                    slice <<= 3;
                    out[frame_base + s * ch + c] = reconstructed as f32 / 32768.0;
                }
            }
            sample_idx += QOA_SLICE_LEN;
        }
        pos += fsize;
    }

    if out.is_empty() || sr == 0 {
        bail!("arquivo QOA sem amostras decodificadas");
    }
    let expected = total_frames.saturating_mul(channels as usize);
    if expected > 0 && expected < out.len() {
        out.truncate(expected);
    }
    out.truncate(max_samples);
    Ok(Pcm {
        sample_rate: sr,
        channels,
        samples: out,
    })
}

pub struct QoaWriter {
    out: BufWriter<File>,
    sr: u32,
    channels: u16,
    lms: [QoaLms; QOA_MAX_CHANNELS],
    pending: Vec<i16>,
    total_frames: u32,
}

impl QoaWriter {
    pub fn create_with_channels(path: &Path, sr: u32, channels: u16) -> Result<Self> {
        if channels == 0 || channels as usize > QOA_MAX_CHANNELS || sr == 0 || sr > 0x00FF_FFFF {
            bail!("QOA não suporta {channels} canais ou taxa {sr} Hz");
        }
        let mut out = BufWriter::new(
            File::create(path).with_context(|| format!("criando {}", path.display()))?,
        );
        out.write_all(b"qoaf")?;
        out.write_all(&0u32.to_be_bytes())?; // total frames atualizado em finish()
        let mut lms = [QoaLms::default(); QOA_MAX_CHANNELS];
        for c in lms.iter_mut().take(channels as usize) {
            c.weights = [0, 0, -(1 << 13), 1 << 14];
        }
        Ok(Self {
            out,
            sr,
            channels,
            lms,
            pending: Vec::new(),
            total_frames: 0,
        })
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        for &s in samples {
            let v = if s.is_finite() { s } else { 0.0 };
            self.pending
                .push((v.clamp(-1.0, 1.0) * 32767.0).round() as i16);
        }
        let frame_samples = QOA_SLICES_PER_FRAME * QOA_SLICE_LEN * self.channels as usize;
        while self.pending.len() >= frame_samples {
            let chunk: Vec<i16> = self.pending.drain(..frame_samples).collect();
            self.write_frame(&chunk)?;
        }
        Ok(())
    }

    fn write_frame(&mut self, interleaved: &[i16]) -> Result<()> {
        let ch = self.channels as usize;
        let fsamples = interleaved.len() / ch;
        if fsamples == 0 {
            return Ok(());
        }
        let slices = fsamples.div_ceil(QOA_SLICE_LEN);
        let fsize = 8 + ch * 16 + slices * ch * 8;
        self.out.write_all(&[ch as u8])?;
        self.out.write_all(&[
            ((self.sr >> 16) & 0xFF) as u8,
            ((self.sr >> 8) & 0xFF) as u8,
            (self.sr & 0xFF) as u8,
        ])?;
        self.out.write_all(&(fsamples as u16).to_be_bytes())?;
        self.out.write_all(&(fsize as u16).to_be_bytes())?;

        for c in 0..ch {
            for &h in &self.lms[c].history {
                self.out.write_all(&(h as i16).to_be_bytes())?;
            }
            for &w in &self.lms[c].weights {
                self.out.write_all(&(w as i16).to_be_bytes())?;
            }
        }

        let mut prev_sf = [0usize; QOA_MAX_CHANNELS];
        let mut s_off = 0usize;
        while s_off < fsamples {
            let slice_len = (fsamples - s_off).min(QOA_SLICE_LEN);
            for c in 0..ch {
                let mut best_rank = u64::MAX;
                let mut best_slice = 0u64;
                let mut best_lms = self.lms[c];
                let mut best_sf = 0usize;
                for sfi in 0..16usize {
                    let sf_idx = (sfi + prev_sf[c]) % 16;
                    let sf = QOA_SCALEFACTOR_TAB[sf_idx];
                    let mut cand_lms = self.lms[c];
                    let mut slice = (sf_idx as u64) << 60;
                    let mut current_rank = 0u64;
                    for si in 0..slice_len {
                        let sample = interleaved[(s_off + si) * ch + c] as i32;
                        let pred = cand_lms.predict();
                        let residual = sample - pred;
                        let scaled = ((residual as f32) / (sf as f32)).round() as i32;
                        let clamped = scaled.clamp(-8, 8);
                        let quantized = QOA_QUANT_TAB[(clamped + 8) as usize] as usize;
                        let dequant = QOA_DEQUANT_TAB[sf_idx][quantized];
                        let recon = (pred + dequant).clamp(-32768, 32767);
                        let err = (sample - recon) as i64;
                        current_rank = current_rank.saturating_add((err * err) as u64);
                        if current_rank > best_rank {
                            break;
                        }
                        cand_lms.update(recon, dequant);
                        slice |= (quantized as u64) << ((19 - si) * 3);
                    }
                    if current_rank < best_rank {
                        best_rank = current_rank;
                        best_slice = slice;
                        best_lms = cand_lms;
                        best_sf = sf_idx;
                    }
                }
                prev_sf[c] = best_sf;
                self.lms[c] = best_lms;
                self.out.write_all(&best_slice.to_be_bytes())?;
            }
            s_off += slice_len;
        }
        self.total_frames = self.total_frames.saturating_add(fsamples as u32);
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        if !self.pending.is_empty() {
            let tail = std::mem::take(&mut self.pending);
            self.write_frame(&tail)?;
        }
        self.out.flush()?;
        let mut f = self.out.into_inner().map_err(|e| anyhow!("{e}"))?;
        f.seek(SeekFrom::Start(4))?;
        f.write_all(&self.total_frames.to_be_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn sine_samples(sr: u32, ch: usize, secs: f32) -> Vec<f32> {
        let frames = (sr as f32 * secs) as usize;
        let mut s = Vec::with_capacity(frames * ch);
        for i in 0..frames {
            let v = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sr as f32).sin() * 0.4;
            for c in 0..ch {
                s.push(if c == 1 { -v } else { v });
            }
        }
        s
    }

    #[test]
    fn roundtrip_au_caf_w64_bwf_rf64_aiff_qoa() {
        let dir = std::env::temp_dir().join(format!("neoveg_aud_extra_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sig = sine_samples(48000, 2, 0.25);

        // AU
        let p_au = dir.join("test.au");
        let mut w = AuWriter::create(&p_au, 48000, WavBits::Pcm16, 2).unwrap();
        w.write(&sig).unwrap();
        w.finish().unwrap();
        let d_au = decode_au(&std::fs::read(&p_au).unwrap(), usize::MAX).unwrap();
        assert_eq!(
            (d_au.sample_rate, d_au.channels, d_au.samples.len()),
            (48000, 2, sig.len())
        );

        // CAF
        let p_caf = dir.join("test.caf");
        let mut w = CafWriter::create(&p_caf, 48000, WavBits::Float32, 2).unwrap();
        w.write(&sig).unwrap();
        w.finish().unwrap();
        let d_caf = decode_caf(&std::fs::read(&p_caf).unwrap(), usize::MAX).unwrap();
        assert_eq!(
            (d_caf.sample_rate, d_caf.channels, d_caf.samples.len()),
            (48000, 2, sig.len())
        );

        // W64
        let p_w64 = dir.join("test.w64");
        let mut w = W64Writer::create_with_channels(&p_w64, 48000, WavBits::Pcm24, 2).unwrap();
        w.write(&sig).unwrap();
        w.finish().unwrap();
        let d_w64 = decode_w64(&std::fs::read(&p_w64).unwrap(), usize::MAX).unwrap();
        assert_eq!(
            (d_w64.sample_rate, d_w64.channels, d_w64.samples.len()),
            (48000, 2, sig.len())
        );

        // BWF e RF64
        let p_bwf = dir.join("test.bwf");
        let mut w = ExtWaveWriter::create_bwf(&p_bwf, 48000, WavBits::Pcm16, 2).unwrap();
        w.write(&sig).unwrap();
        w.finish().unwrap();
        let d_bwf = super::super::wav::decode(&std::fs::read(&p_bwf).unwrap(), usize::MAX).unwrap();
        assert_eq!(d_bwf.samples.len(), sig.len());

        let p_rf64 = dir.join("test.rf64");
        let mut w = ExtWaveWriter::create_rf64(&p_rf64, 48000, WavBits::Float32, 2).unwrap();
        w.write(&sig).unwrap();
        w.finish().unwrap();
        let d_rf64 =
            super::super::wav::decode(&std::fs::read(&p_rf64).unwrap(), usize::MAX).unwrap();
        assert_eq!(d_rf64.samples.len(), sig.len());

        // AIFF / AIFC
        let p_aif = dir.join("test.aiff");
        let mut w = AiffWriter::create_with_channels(&p_aif, 48000, WavBits::Float32, 2).unwrap();
        w.write(&sig).unwrap();
        w.finish().unwrap();
        let d_aif = super::super::wav::decode(&std::fs::read(&p_aif).unwrap(), usize::MAX).unwrap();
        assert_eq!(d_aif.samples.len(), sig.len());

        // QOA
        let p_qoa = dir.join("test.qoa");
        let mut w = QoaWriter::create_with_channels(&p_qoa, 48000, 2).unwrap();
        w.write(&sig).unwrap();
        w.finish().unwrap();
        let d_qoa = decode_qoa(&std::fs::read(&p_qoa).unwrap(), usize::MAX).unwrap();
        assert_eq!(
            (d_qoa.sample_rate, d_qoa.channels, d_qoa.samples.len()),
            (48000, 2, sig.len())
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn decodes_voc_and_8svx_fibonacci_delta() {
        // VOC v1.20 bloco 9 (PCM 16b 22050 Hz mono)
        let mut voc = b"Creative Voice File\x1A".to_vec();
        voc.extend_from_slice(&26u16.to_le_bytes());
        let ver = 0x0114u16;
        voc.extend_from_slice(&ver.to_le_bytes());
        voc.extend_from_slice(&(!ver).wrapping_add(0x1234).to_le_bytes());
        let mut blk = vec![];
        blk.extend_from_slice(&22050u32.to_le_bytes());
        blk.push(16);
        blk.push(1);
        blk.extend_from_slice(&4u16.to_le_bytes());
        blk.extend_from_slice(&0u32.to_le_bytes());
        blk.extend_from_slice(&16384i16.to_le_bytes());
        blk.extend_from_slice(&(-16384i16).to_le_bytes());
        let sz = blk.len() as u32;
        voc.push(9);
        voc.push((sz & 0xFF) as u8);
        voc.push(((sz >> 8) & 0xFF) as u8);
        voc.push(((sz >> 16) & 0xFF) as u8);
        voc.extend_from_slice(&blk);
        voc.push(0);
        let p = decode_voc(&voc, usize::MAX).unwrap();
        assert_eq!((p.sample_rate, p.channels, p.samples.len()), (22050, 1, 2));
        assert!((p.samples[0] - 0.5).abs() < 1e-3);

        // 8SVX Fibonacci delta (nibble 8 = 0, nibble 15 = +21)
        let mut svx = b"FORM\0\0\0\08SVXVHDR".to_vec();
        svx.extend_from_slice(&20u32.to_be_bytes());
        let mut vhdr = vec![0u8; 20];
        vhdr[12..14].copy_from_slice(&11025u16.to_be_bytes());
        vhdr[14] = 1;
        vhdr[15] = 1; // Fibonacci delta
        svx.extend_from_slice(&vhdr);
        svx.extend_from_slice(b"BODY");
        svx.extend_from_slice(&4u32.to_be_bytes());
        svx.extend_from_slice(&[0u8, 10, 0xF8, 0x8F]); // x=10 -> +21=31, +0=31, +0=31, +21=52
        let flen = (svx.len() - 8) as u32;
        svx[4..8].copy_from_slice(&flen.to_be_bytes());
        let p8 = decode_8svx(&svx, usize::MAX).unwrap();
        assert_eq!(
            (p8.sample_rate, p8.channels, p8.samples.len()),
            (11025, 1, 4)
        );
    }

    #[test]
    fn oracle_ffmpeg_au_caf_w64_voc_ulaw_adpcm() {
        let dir = std::env::temp_dir().join(format!("neoveg_aud_oracle_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let specs: &[(&str, &[&str])] = &[
            ("sine.au", &["-c:a", "pcm_mulaw"]),
            ("sine.caf", &["-c:a", "pcm_s16be"]),
            ("sine.w64", &["-c:a", "pcm_s16le"]),
            ("sine.voc", &["-c:a", "pcm_u8"]),
            ("sine_adpcm.wav", &["-c:a", "adpcm_ima_wav"]),
            ("sine_alaw.wav", &["-c:a", "pcm_alaw"]),
        ];
        for (name, args) in specs {
            let p = dir.join(name);
            let mut cmd = Command::new("ffmpeg");
            cmd.args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=44100:duration=0.3",
                "-ac",
                "2",
            ]);
            cmd.args(*args).arg(&p);
            let Ok(out) = cmd.output() else { return };
            if !out.status.success() {
                continue;
            }
            let bytes = std::fs::read(&p).unwrap();
            let ext = p.extension().unwrap().to_str().unwrap();
            let dec = if ext == "wav" {
                super::super::wav::decode(&bytes, usize::MAX)
            } else {
                decode_bytes(ext, &bytes, usize::MAX)
            }
            .unwrap_or_else(|e| panic!("falha ao abrir {name}: {e}"));
            if *name == "sine.voc" {
                // VOC bloco 8 quantiza a taxa via divisor inteiro (~44107 Hz para 44100 estéreo).
                assert!(
                    (44000..=44250).contains(&dec.sample_rate),
                    "{name}: {}",
                    dec.sample_rate
                );
            } else {
                assert_eq!(dec.sample_rate, 44100, "{name}");
            }
            assert!(dec.samples.len() > 8000, "{name}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hostiles_rejected_cleanly() {
        for ext in ["au", "caf", "w64", "voc", "8svx", "qoa"] {
            assert!(decode_bytes(ext, b"", usize::MAX).is_err());
            assert!(decode_bytes(ext, b"lixo", usize::MAX).is_err());
        }
    }
}
