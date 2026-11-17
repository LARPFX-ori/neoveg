//! Leitor de áudio PCM/G.711/ADPCM em contêineres RIFF WAVE, RF64, BWF e AIFF/AIFF-C.
//!
//! Implementado a partir das especificações públicas: "Multimedia Programming
//! Interface and Data Specifications 1.0" (IBM/Microsoft, 1991) com extensão
//! WAVE_FORMAT_EXTENSIBLE e IMA/DVI ADPCM (tag `0x0011`), EBU Tech 3285 (BWF),
//! EBU Tech 3306 (RF64), ITU-T G.711 (µ-law / A-law), e "Audio Interchange File
//! Format: AIFF, version 1.3" (Apple, 1989) com AIFF-C (`NONE`, `sowt`, `fl32`,
//! `ulaw`, `alaw`). Todo campo de tamanho é tratado como hostil (§0.4).

use anyhow::{bail, Result};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SampleFmt {
    /// Inteiro com sinal (8 bits em WAVE é sem sinal).
    Int {
        bits: u16,
        big_endian: bool,
    },
    Float {
        bits: u16,
        big_endian: bool,
    },
    /// ITU-T G.711 µ-law (8 bits por amostra).
    Ulaw,
    /// ITU-T G.711 A-law (8 bits por amostra).
    Alaw,
    /// Microsoft IMA / DVI ADPCM (tag 0x0011, 4 bits em blocos `block_align`).
    ImaAdpcm {
        block_align: u16,
        samples_per_block: u16,
    },
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WaveFmt {
    pub fmt: SampleFmt,
    pub channels: u16,
    pub sample_rate: u32,
}

#[inline]
pub(crate) fn ulaw_to_f32(byte: u8) -> f32 {
    let u = !byte;
    let sign = (u & 0x80) != 0;
    let exp = ((u >> 4) & 0x07) as u32;
    let mant = (u & 0x0F) as i32;
    let mag = (((mant << 3) + 0x84) << exp) - 0x84;
    let s = if sign { -mag } else { mag };
    (s as f32) / 32768.0
}

#[inline]
pub(crate) fn alaw_to_f32(byte: u8) -> f32 {
    let a = byte ^ 0x55;
    let positive = (a & 0x80) != 0;
    let exp = (a >> 4) & 0x07;
    let mant = (a & 0x0F) as i32;
    let mag = if exp == 0 {
        (mant << 4) + 8
    } else {
        ((mant << 4) + 0x108) << (exp - 1)
    };
    let s = if positive { mag } else { -mag };
    (s as f32) / 32768.0
}

/// Decodifica um arquivo WAVE, BWF, RF64 ou AIFF já lido para a memória.
pub fn decode(bytes: &[u8], max_samples: usize) -> Result<Pcm> {
    match bytes.get(0..4) {
        Some(b"RIFF") if bytes.get(8..12) == Some(b"WAVE") => decode_wave(bytes, max_samples),
        Some(b"RF64") if bytes.get(8..12) == Some(b"WAVE") => decode_wave(bytes, max_samples),
        Some(b"FORM") if matches!(bytes.get(8..12), Some(b"AIFF") | Some(b"AIFC")) => {
            decode_aiff(bytes, max_samples)
        }
        _ => bail!("não é um arquivo WAVE, RF64 ou AIFF"),
    }
}

pub(crate) fn parse_wave_fmt_body(body: &[u8]) -> Result<WaveFmt> {
    if body.len() < 16 {
        bail!("bloco WAVE 'fmt ' truncado");
    }
    let raw_tag = u16le(body, 0).unwrap_or(0);
    let channels = u16le(body, 2).unwrap_or(0);
    let rate = u32le(body, 4).unwrap_or(0);
    let align = u16le(body, 12).unwrap_or(0);
    let bits = u16le(body, 14).unwrap_or(0);
    // WAVE_FORMAT_EXTENSIBLE: o subformato fica nos 2 primeiros bytes do GUID
    let tag = if raw_tag == 0xFFFE {
        u16le(body, 24).unwrap_or(0)
    } else {
        raw_tag
    };
    let fmt = match (tag, bits) {
        (1, 8 | 16 | 24 | 32) => SampleFmt::Int {
            bits,
            big_endian: false,
        },
        (3, 32 | 64) => SampleFmt::Float {
            bits,
            big_endian: false,
        },
        (6, _) => SampleFmt::Alaw,
        (7, _) => SampleFmt::Ulaw,
        (0x0011, 4) if align >= 4 * channels.max(1) && (channels == 1 || channels == 2) => {
            let spb = u16le(body, 18).unwrap_or_else(|| {
                ((align as usize).saturating_sub(4 * channels as usize) * 2 / channels as usize + 1)
                    as u16
            });
            SampleFmt::ImaAdpcm {
                block_align: align,
                samples_per_block: spb.max(1),
            }
        }
        _ => bail!("formato WAVE não suportado (tag {tag}, {bits} bits)"),
    };
    Ok(WaveFmt {
        fmt,
        channels,
        sample_rate: rate,
    })
}

fn decode_wave(b: &[u8], max_samples: usize) -> Result<Pcm> {
    let is_rf64 = b.get(0..4) == Some(b"RF64");
    let mut pos = 12usize;
    let mut fmt: Option<WaveFmt> = None;
    let mut data: Option<&[u8]> = None;
    let mut rf64_data_size: Option<u64> = None;
    while pos + 8 <= b.len() {
        let id = b.get(pos..pos + 4).unwrap_or(&[]);
        let size32 = u32le(b, pos + 4).unwrap_or(0) as u64;
        let start = pos + 8;
        let actual_size = if is_rf64 && id == b"data" && size32 == u32::MAX as u64 {
            rf64_data_size.unwrap_or(b.len().saturating_sub(start) as u64)
        } else if size32 == u32::MAX as u64 {
            b.len().saturating_sub(start) as u64
        } else {
            size32
        };
        let step_size = usize::try_from(actual_size).unwrap_or(b.len());
        let end = start.saturating_add(step_size).min(b.len());
        let body = b.get(start..end).unwrap_or(&[]);
        if id == b"ds64" && body.len() >= 16 {
            let mut arr = [0u8; 8];
            arr.copy_from_slice(&body[8..16]);
            rf64_data_size = Some(u64::from_le_bytes(arr));
        } else if id == b"fmt " {
            fmt = Some(parse_wave_fmt_body(body)?);
        } else if id == b"data" {
            data = Some(body);
        }
        if data.is_some() && fmt.is_some() {
            break;
        }
        pos = start
            .saturating_add(step_size)
            .saturating_add(step_size & 1);
    }
    let Some(wf) = fmt else {
        bail!("WAVE sem bloco 'fmt '")
    };
    let Some(data) = data else {
        bail!("WAVE sem bloco 'data'")
    };
    build(wf.fmt, wf.channels, wf.sample_rate, data, max_samples)
}

/// Resultado da decodificação: taxa, canais e amostras intercaladas em f32.
pub struct Pcm {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

fn u16le(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|x| u16::from_le_bytes([x[0], x[1]]))
}
fn u32le(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4)
        .map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}
fn u16be(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|x| u16::from_be_bytes([x[0], x[1]]))
}
fn u32be(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4)
        .map(|x| u32::from_be_bytes([x[0], x[1], x[2], x[3]]))
}

/// Converte o ponto flutuante estendido de 80 bits (IEEE 754) do AIFF.
fn extended80(b: &[u8]) -> Option<f64> {
    let b = b.get(0..10)?;
    let sign = if b[0] & 0x80 != 0 { -1.0 } else { 1.0 };
    let exp = (((b[0] & 0x7F) as i32) << 8) | b[1] as i32;
    let mut mant: u64 = 0;
    for &x in &b[2..10] {
        mant = (mant << 8) | x as u64;
    }
    if exp == 0 && mant == 0 {
        return Some(0.0);
    }
    if exp == 0x7FFF {
        return None; // infinito ou NaN
    }
    Some(sign * mant as f64 * 2f64.powi(exp - 16383 - 63))
}

const IMA_INDEX_TABLE: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

const IMA_STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 81, 89, 98, 108, 119, 131, 144, 158, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4429, 4872, 5359, 5895, 6485, 7134, 7848, 8633, 9497,
    10447, 11492, 12642, 13907, 15297, 16827, 18510, 20361, 22397, 24637, 27101, 29811, 32767,
];

#[inline]
fn ima_step(nibble: u8, pred: &mut i32, idx: &mut i32) -> f32 {
    let step = IMA_STEP_TABLE[(*idx).clamp(0, 88) as usize];
    let mut diff = step >> 3;
    if (nibble & 4) != 0 {
        diff += step;
    }
    if (nibble & 2) != 0 {
        diff += step >> 1;
    }
    if (nibble & 1) != 0 {
        diff += step >> 2;
    }
    if (nibble & 8) != 0 {
        *pred = pred.saturating_sub(diff);
    } else {
        *pred = pred.saturating_add(diff);
    }
    *pred = (*pred).clamp(-32768, 32767);
    *idx = (*idx + IMA_INDEX_TABLE[(nibble & 0xF) as usize]).clamp(0, 88);
    *pred as f32 / 32768.0
}

fn decode_ima_adpcm(
    data: &[u8],
    channels: u16,
    block_align: u16,
    samples_per_block: u16,
    max_samples: usize,
) -> Vec<f32> {
    let ch = channels as usize;
    let blk_sz = (block_align as usize).max(4 * ch);
    let spb = samples_per_block as usize;
    let mut out = Vec::new();
    for blk in data.chunks(blk_sz) {
        if blk.len() < 4 * ch || out.len() >= max_samples {
            break;
        }
        let mut pred = [0i32; 2];
        let mut idx = [0i32; 2];
        for c in 0..ch {
            let h = c * 4;
            pred[c] = i16::from_le_bytes([blk[h], blk[h + 1]]) as i32;
            idx[c] = (blk[h + 2] as i32).clamp(0, 88);
        }
        // Primeira amostra do bloco vem direto do cabeçalho
        for &p in pred.iter().take(ch) {
            out.push(p as f32 / 32768.0);
        }
        let payload = &blk[4 * ch..];
        let mut off = 0usize;
        let mut decoded_frames = 1usize;
        if ch == 1 {
            while off < payload.len() && decoded_frames < spb && out.len() < max_samples {
                let b = payload[off];
                off += 1;
                out.push(ima_step(b & 0xF, &mut pred[0], &mut idx[0]));
                decoded_frames += 1;
                if decoded_frames < spb && out.len() < max_samples {
                    out.push(ima_step(b >> 4, &mut pred[0], &mut idx[0]));
                    decoded_frames += 1;
                }
            }
        } else {
            // Estéreo: 4 bytes (8 nibbles) do canal esquerdo seguidos de 4 bytes do canal direito
            while off + 8 <= payload.len() && decoded_frames < spb && out.len() + 2 <= max_samples {
                let mut l = [0.0f32; 8];
                let mut r = [0.0f32; 8];
                for k in 0..4 {
                    let bl = payload[off + k];
                    let br = payload[off + 4 + k];
                    l[k * 2] = ima_step(bl & 0xF, &mut pred[0], &mut idx[0]);
                    l[k * 2 + 1] = ima_step(bl >> 4, &mut pred[0], &mut idx[0]);
                    r[k * 2] = ima_step(br & 0xF, &mut pred[1], &mut idx[1]);
                    r[k * 2 + 1] = ima_step(br >> 4, &mut pred[1], &mut idx[1]);
                }
                for k in 0..8 {
                    if decoded_frames >= spb || out.len() + 2 > max_samples {
                        break;
                    }
                    out.push(l[k]);
                    out.push(r[k]);
                    decoded_frames += 1;
                }
                off += 8;
            }
        }
    }
    out.truncate(max_samples);
    out
}

fn decode_aiff(b: &[u8], max_samples: usize) -> Result<Pcm> {
    let aifc = b.get(8..12) == Some(b"AIFC");
    let mut pos = 12usize;
    let mut comm: Option<(SampleFmt, u16, u32)> = None;
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= b.len() {
        let id = b.get(pos..pos + 4).unwrap_or(&[]);
        let size = u32be(b, pos + 4).unwrap_or(0) as usize;
        let start = pos + 8;
        let end = start.saturating_add(size).min(b.len());
        let body = b.get(start..end).unwrap_or(&[]);
        if id == b"COMM" {
            let channels = u16be(body, 0).unwrap_or(0);
            let bits = u16be(body, 6).unwrap_or(0);
            let rate = extended80(body.get(8..18).unwrap_or(&[])).unwrap_or(0.0);
            let compression = if aifc {
                body.get(18..22).unwrap_or(b"NONE")
            } else {
                b"NONE"
            };
            let f = match (compression, bits) {
                (b"NONE", 8 | 16 | 24 | 32) => SampleFmt::Int {
                    bits,
                    big_endian: true,
                },
                (b"sowt", 16 | 24 | 32) => SampleFmt::Int {
                    bits,
                    big_endian: false,
                },
                (b"fl32" | b"FL32", _) => SampleFmt::Float {
                    bits: 32,
                    big_endian: true,
                },
                (b"fl64" | b"FL64", _) => SampleFmt::Float {
                    bits: 64,
                    big_endian: true,
                },
                (b"ulaw" | b"ULAW", _) => SampleFmt::Ulaw,
                (b"alaw" | b"ALAW", _) => SampleFmt::Alaw,
                _ => bail!("compressão AIFF não suportada ({bits} bits)"),
            };
            if !(1.0..=1_000_000.0).contains(&rate) {
                bail!("taxa de amostragem AIFF inválida");
            }
            comm = Some((f, channels, rate.round() as u32));
        } else if id == b"SSND" {
            let offset = u32be(body, 0).unwrap_or(0) as usize;
            data = Some(
                body.get(8usize.saturating_add(offset).min(body.len())..)
                    .unwrap_or(&[]),
            );
        }
        if data.is_some() && comm.is_some() {
            break;
        }
        pos = start.saturating_add(size).saturating_add(size & 1);
    }
    let Some((f, channels, rate)) = comm else {
        bail!("AIFF sem bloco COMM")
    };
    let Some(data) = data else {
        bail!("AIFF sem bloco SSND")
    };
    build(f, channels, rate, data, max_samples)
}

pub(crate) fn build(
    f: SampleFmt,
    channels: u16,
    rate: u32,
    data: &[u8],
    max_samples: usize,
) -> Result<Pcm> {
    if channels == 0 || channels > 64 {
        bail!("número de canais inválido: {channels}");
    }
    if rate == 0 || rate > 1_000_000 {
        bail!("taxa de amostragem inválida: {rate}");
    }
    match f {
        SampleFmt::Ulaw => {
            let n = data.len().min(max_samples);
            let n = n - n % channels as usize;
            let samples = data[..n].iter().map(|&u| ulaw_to_f32(u)).collect();
            return Ok(Pcm {
                sample_rate: rate,
                channels,
                samples,
            });
        }
        SampleFmt::Alaw => {
            let n = data.len().min(max_samples);
            let n = n - n % channels as usize;
            let samples = data[..n].iter().map(|&a| alaw_to_f32(a)).collect();
            return Ok(Pcm {
                sample_rate: rate,
                channels,
                samples,
            });
        }
        SampleFmt::ImaAdpcm {
            block_align,
            samples_per_block,
        } => {
            let samples =
                decode_ima_adpcm(data, channels, block_align, samples_per_block, max_samples);
            return Ok(Pcm {
                sample_rate: rate,
                channels,
                samples,
            });
        }
        _ => {}
    }
    let (bits, be, float) = match f {
        SampleFmt::Int { bits, big_endian } => (bits, big_endian, false),
        SampleFmt::Float { bits, big_endian } => (bits, big_endian, true),
        SampleFmt::Ulaw | SampleFmt::Alaw | SampleFmt::ImaAdpcm { .. } => (8, false, false),
    };
    let bps = (bits as usize).div_ceil(8);
    let n = (data.len() / bps).min(max_samples);
    // só quadros completos
    let n = n - n % channels as usize;
    let mut out = Vec::with_capacity(n);
    for c in data.chunks_exact(bps).take(n) {
        let v = match (float, bits) {
            (false, 8) => {
                if be {
                    c[0] as i8 as f32 / 128.0 // AIFF: 8 bits com sinal
                } else {
                    (c[0] as f32 - 128.0) / 128.0 // WAVE: 8 bits sem sinal
                }
            }
            (false, 16) => {
                let x = if be {
                    i16::from_be_bytes([c[0], c[1]])
                } else {
                    i16::from_le_bytes([c[0], c[1]])
                };
                x as f32 / 32768.0
            }
            (false, 24) => {
                let (b0, b1, b2) = if be {
                    (c[2], c[1], c[0])
                } else {
                    (c[0], c[1], c[2])
                };
                let x = (((b2 as i32) << 24) | ((b1 as i32) << 16) | ((b0 as i32) << 8)) >> 8;
                x as f32 / 8_388_608.0
            }
            (false, 32) => {
                let a = [c[0], c[1], c[2], c[3]];
                let x = if be {
                    i32::from_be_bytes(a)
                } else {
                    i32::from_le_bytes(a)
                };
                x as f32 / 2_147_483_648.0
            }
            (true, 32) => {
                let a = [c[0], c[1], c[2], c[3]];
                let x = if be {
                    f32::from_be_bytes(a)
                } else {
                    f32::from_le_bytes(a)
                };
                if x.is_finite() {
                    x
                } else {
                    0.0
                }
            }
            (true, 64) => {
                let a = [c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]];
                let x = if be {
                    f64::from_be_bytes(a)
                } else {
                    f64::from_le_bytes(a)
                };
                if x.is_finite() {
                    x as f32
                } else {
                    0.0
                }
            }
            _ => 0.0,
        };
        out.push(v);
    }
    Ok(Pcm {
        sample_rate: rate,
        channels,
        samples: out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aiff_16_be(rate_ext: [u8; 10], frames: &[i16]) -> Vec<u8> {
        let mut comm = vec![];
        comm.extend_from_slice(&1u16.to_be_bytes()); // canais
        comm.extend_from_slice(&(frames.len() as u32).to_be_bytes());
        comm.extend_from_slice(&16u16.to_be_bytes());
        comm.extend_from_slice(&rate_ext);
        let mut ssnd = vec![0u8; 8];
        for f in frames {
            ssnd.extend_from_slice(&f.to_be_bytes());
        }
        let mut body = b"AIFF".to_vec();
        body.extend_from_slice(b"COMM");
        body.extend_from_slice(&(comm.len() as u32).to_be_bytes());
        body.extend_from_slice(&comm);
        body.extend_from_slice(b"SSND");
        body.extend_from_slice(&(ssnd.len() as u32).to_be_bytes());
        body.extend_from_slice(&ssnd);
        let mut out = b"FORM".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn aiff_44100_mono() {
        // 44100 em ponto flutuante estendido de 80 bits
        let rate = [0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0];
        let b = aiff_16_be(rate, &[0, 16384, -16384, 32767]);
        let p = decode(&b, usize::MAX).unwrap();
        assert_eq!((p.sample_rate, p.channels), (44100, 1));
        assert_eq!(p.samples.len(), 4);
        assert!((p.samples[1] - 0.5).abs() < 1e-4);
        assert!((p.samples[2] + 0.5).abs() < 1e-4);
    }

    #[test]
    fn wave_24_bits() {
        let mut b = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&48000u32.to_le_bytes());
        b.extend_from_slice(&(48000u32 * 6).to_le_bytes());
        b.extend_from_slice(&6u16.to_le_bytes());
        b.extend_from_slice(&24u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&6u32.to_le_bytes());
        b.extend_from_slice(&[0x00, 0x00, 0x40, 0x00, 0x00, 0xC0]); // +0.5, -0.5
        let p = decode(&b, usize::MAX).unwrap();
        assert_eq!((p.sample_rate, p.channels), (48000, 2));
        assert!((p.samples[0] - 0.5).abs() < 1e-6);
        assert!((p.samples[1] + 0.5).abs() < 1e-6);
    }

    #[test]
    fn tamanhos_hostis_nao_entram_em_panico() {
        let mut b = b"RIFF\xFF\xFF\xFF\xFFWAVEfmt \xFF\xFF\xFF\xFF".to_vec();
        b.extend_from_slice(&[1, 0, 2, 0]);
        assert!(decode(&b, usize::MAX).is_err());
        assert!(decode(b"FORM\0\0\0\x04AIFC", usize::MAX).is_err());
        assert!(extended80(&[0x7F, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0]).is_none());
    }
}
