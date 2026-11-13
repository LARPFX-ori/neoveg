//! Leitor de áudio PCM não comprimido: WAVE (RIFF) e AIFF/AIFF-C.
//!
//! Implementado a partir das especificações públicas: "Multimedia Programming
//! Interface and Data Specifications 1.0" (IBM/Microsoft, 1991) com a extensão
//! WAVE_FORMAT_EXTENSIBLE, e "Audio Interchange File Format: AIFF, version
//! 1.3" (Apple, 1989) com o adendo AIFF-C (compressão `NONE`, `sowt`, `fl32`).
//! Todo campo de tamanho vem do arquivo e é tratado como hostil (§0.4).

use anyhow::{bail, Result};

#[derive(Clone, Copy, Debug, PartialEq)]
enum SampleFmt {
    /// Inteiro com sinal (8 bits em WAVE é sem sinal).
    Int {
        bits: u16,
        big_endian: bool,
    },
    Float {
        bits: u16,
        big_endian: bool,
    },
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

/// Decodifica um arquivo WAVE ou AIFF já lido para a memória.
pub fn decode(bytes: &[u8], max_samples: usize) -> Result<Pcm> {
    match bytes.get(0..4) {
        Some(b"RIFF") if bytes.get(8..12) == Some(b"WAVE") => decode_wave(bytes, max_samples),
        Some(b"FORM") if matches!(bytes.get(8..12), Some(b"AIFF") | Some(b"AIFC")) => {
            decode_aiff(bytes, max_samples)
        }
        _ => bail!("não é um arquivo WAVE ou AIFF"),
    }
}

fn decode_wave(b: &[u8], max_samples: usize) -> Result<Pcm> {
    let mut pos = 12usize;
    let mut fmt: Option<(SampleFmt, u16, u32, u16)> = None; // formato, canais, taxa, block align
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= b.len() {
        let id = b.get(pos..pos + 4).unwrap_or(&[]);
        let size = u32le(b, pos + 4).unwrap_or(0) as usize;
        let start = pos + 8;
        // tamanho 0xFFFFFFFF (gravação em fluxo) ou maior que o arquivo: usa o que existe
        let end = start.saturating_add(size).min(b.len());
        let body = b.get(start..end).unwrap_or(&[]);
        if id == b"fmt " {
            let tag = u16le(body, 0).unwrap_or(0);
            let channels = u16le(body, 2).unwrap_or(0);
            let rate = u32le(body, 4).unwrap_or(0);
            let align = u16le(body, 12).unwrap_or(0);
            let bits = u16le(body, 14).unwrap_or(0);
            // WAVE_FORMAT_EXTENSIBLE: o subformato fica nos 2 primeiros bytes do GUID
            let tag = if tag == 0xFFFE {
                u16le(body, 24).unwrap_or(0)
            } else {
                tag
            };
            let f = match (tag, bits) {
                (1, 8 | 16 | 24 | 32) => SampleFmt::Int {
                    bits,
                    big_endian: false,
                },
                (3, 32 | 64) => SampleFmt::Float {
                    bits,
                    big_endian: false,
                },
                _ => bail!("formato WAVE não suportado (tag {tag}, {bits} bits)"),
            };
            fmt = Some((f, channels, rate, align));
        } else if id == b"data" {
            data = Some(body);
        }
        if data.is_some() && fmt.is_some() {
            break;
        }
        pos = start.saturating_add(size).saturating_add(size & 1);
    }
    let Some((f, channels, rate, _align)) = fmt else {
        bail!("WAVE sem bloco 'fmt '")
    };
    let Some(data) = data else {
        bail!("WAVE sem bloco 'data'")
    };
    build(f, channels, rate, data, max_samples)
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

fn build(f: SampleFmt, channels: u16, rate: u32, data: &[u8], max_samples: usize) -> Result<Pcm> {
    if channels == 0 || channels > 64 {
        bail!("número de canais inválido: {channels}");
    }
    if rate == 0 || rate > 1_000_000 {
        bail!("taxa de amostragem inválida: {rate}");
    }
    let (bits, be, float) = match f {
        SampleFmt::Int { bits, big_endian } => (bits, big_endian, false),
        SampleFmt::Float { bits, big_endian } => (bits, big_endian, true),
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
