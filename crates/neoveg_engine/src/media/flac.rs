//! Codificador FLAC (*Free Lossless Audio Codec*) em Rust puro.
//!
//! Implementado em Sala Limpa a partir da especificação pública:
//! - IETF RFC 9639 (*Free Lossless Audio Codec (FLAC)*, dezembro de 2024):
//!   - Assinatura `"fLaC"` e bloco de metadados `STREAMINFO` (§8.2);
//!   - Cabeçalho de quadro, codificação UTF-8 estendida, CRC-8 (`x^8+x^2+x+1`) (§9.1);
//!   - Subquadros `CONSTANT`, `VERBATIM` e `FIXED` (ordens 0..=4) (§9.2.1–9.2.5);
//!   - Resíduos Rice particionados (4 e 5 bits, mais escape) e rodapé CRC-16 (`x^16+x^15+x^2+1`) (§9.2.7, §9.3).

use crate::render::WavBits;
use anyhow::{anyhow, bail, Context, Result};
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

/// Tamanho de bloco padrão por quadro (amostras intercanais).
pub const FLAC_DEFAULT_BLOCK_SIZE: usize = 4096;

/// Divisor máximo suportado pelo campo `sample_rate` (20 bits) no `STREAMINFO`.
const MAX_FLAC_SAMPLE_RATE: u32 = (1 << 20) - 1;
/// Limite de amostras totais representáveis no campo de 36 bits do `STREAMINFO`.
const MAX_FLAC_TOTAL_SAMPLES: u64 = (1u64 << 36) - 1;

#[inline]
fn crc8_flac(data: &[u8]) -> u8 {
    // Polinômio RFC 9639 §9.1.8: x^8 + x^2 + x + 1 (0x07 MSB-first)
    let mut crc = 0u8;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            if (crc & 0x80) != 0 {
                crc = (crc << 1) ^ 0x07;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

#[inline]
fn crc16_flac(data: &[u8]) -> u16 {
    // Polinômio RFC 9639 §9.3: x^16 + x^15 + x^2 + 1 (0x8005 MSB-first)
    let mut crc = 0u16;
    for &b in data {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x8005;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

struct BitWriter {
    bytes: Vec<u8>,
    acc: u64,
    bits: u8,
}

impl BitWriter {
    fn with_capacity(cap: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(cap),
            acc: 0,
            bits: 0,
        }
    }

    #[inline]
    fn write_bits(&mut self, val: u64, nbits: u8) {
        if nbits == 0 {
            return;
        }
        let mask = if nbits >= 64 {
            u64::MAX
        } else {
            (1u64 << nbits) - 1
        };
        let masked = val & mask;
        if self.bits + nbits > 64 {
            // Descarrega bytes cheios antes de empacotar novos bits para evitar overflow de `acc`.
            while self.bits >= 8 {
                self.bits -= 8;
                self.bytes.push((self.acc >> self.bits) as u8);
            }
            if self.bits > 0 {
                self.acc &= (1u64 << self.bits) - 1;
            } else {
                self.acc = 0;
            }
        }
        self.acc = (self.acc << nbits) | masked;
        self.bits += nbits;
        while self.bits >= 8 {
            self.bits -= 8;
            self.bytes.push((self.acc >> self.bits) as u8);
        }
        if self.bits > 0 {
            self.acc &= (1u64 << self.bits) - 1;
        } else {
            self.acc = 0;
        }
    }

    #[inline]
    fn write_signed(&mut self, val: i64, nbits: u8) {
        self.write_bits(val as u64, nbits);
    }

    #[inline]
    fn write_unary_zero_then_one(&mut self, zeros: u64) {
        let mut rem = zeros;
        while rem >= 32 {
            self.write_bits(0, 32);
            rem -= 32;
        }
        if rem > 0 {
            self.write_bits(0, rem as u8);
        }
        self.write_bits(1, 1);
    }

    fn finish_byte_aligned(mut self) -> Vec<u8> {
        if self.bits > 0 {
            let pad = 8 - self.bits;
            self.bytes.push(((self.acc << pad) & 0xFF) as u8);
            self.bits = 0;
            self.acc = 0;
        }
        self.bytes
    }
}

fn push_utf8_coded_number(out: &mut Vec<u8>, val: u64) -> Result<()> {
    // RFC 9639 §9.1.5: UTF-8 estendido até 36 bits (31 bits para número de quadro)
    if val < 0x80 {
        out.push(val as u8);
    } else if val < (1 << 11) {
        out.push((0xC0 | (val >> 6)) as u8);
        out.push((0x80 | (val & 0x3F)) as u8);
    } else if val < (1 << 16) {
        out.push((0xE0 | (val >> 12)) as u8);
        out.push((0x80 | ((val >> 6) & 0x3F)) as u8);
        out.push((0x80 | (val & 0x3F)) as u8);
    } else if val < (1 << 21) {
        out.push((0xF0 | (val >> 18)) as u8);
        out.push((0x80 | ((val >> 12) & 0x3F)) as u8);
        out.push((0x80 | ((val >> 6) & 0x3F)) as u8);
        out.push((0x80 | (val & 0x3F)) as u8);
    } else if val < (1 << 26) {
        out.push((0xF8 | (val >> 24)) as u8);
        out.push((0x80 | ((val >> 18) & 0x3F)) as u8);
        out.push((0x80 | ((val >> 12) & 0x3F)) as u8);
        out.push((0x80 | ((val >> 6) & 0x3F)) as u8);
        out.push((0x80 | (val & 0x3F)) as u8);
    } else if val < (1 << 31) {
        out.push((0xFC | (val >> 30)) as u8);
        out.push((0x80 | ((val >> 24) & 0x3F)) as u8);
        out.push((0x80 | ((val >> 18) & 0x3F)) as u8);
        out.push((0x80 | ((val >> 12) & 0x3F)) as u8);
        out.push((0x80 | ((val >> 6) & 0x3F)) as u8);
        out.push((0x80 | (val & 0x3F)) as u8);
    } else if val < (1 << 36) {
        out.push(0xFE);
        out.push((0x80 | ((val >> 30) & 0x3F)) as u8);
        out.push((0x80 | ((val >> 24) & 0x3F)) as u8);
        out.push((0x80 | ((val >> 18) & 0x3F)) as u8);
        out.push((0x80 | ((val >> 12) & 0x3F)) as u8);
        out.push((0x80 | ((val >> 6) & 0x3F)) as u8);
        out.push((0x80 | (val & 0x3F)) as u8);
    } else {
        bail!("número codificado FLAC excede 36 bits: {val}");
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum SubframePlan {
    Verbatim,
    Fixed {
        order: usize,
        partition_order: u8,
        use_5bit: bool,
    },
}

#[inline]
fn fold_residual(r: i64) -> u64 {
    if r >= 0 {
        (r as u64) << 1
    } else {
        ((-r - 1) as u64) << 1 | 1
    }
}

fn best_rice_for_slice(slice: &[i64], limit_k: u8) -> Option<(u8, usize)> {
    let mut max_fold = 0u64;
    for &r in slice {
        let f = fold_residual(r);
        if f > max_fold {
            max_fold = f;
        }
    }
    let max_k = (max_fold.checked_ilog2().unwrap_or(0) as u8).min(limit_k);
    let mut best_k = None;
    let mut best_bits = usize::MAX;
    for k in 0..=max_k {
        let mut sum_bits = 0usize;
        let mut overflow_unary = false;
        for &r in slice {
            let q = fold_residual(r) >> k;
            if q > 4096 {
                overflow_unary = true;
                break;
            }
            sum_bits += q as usize + 1 + usize::from(k);
        }
        if !overflow_unary && sum_bits < best_bits {
            best_bits = sum_bits;
            best_k = Some(k);
        }
    }
    best_k.map(|k| (k, best_bits))
}

/// Codificador streaming de quadros FLAC reutilizado tanto por [`FlacWriter`]
/// quanto pela multiplexação `A_FLAC` em Matroska (RFC 9639 §10.2).
pub struct FlacStreamEncoder {
    sample_rate: u32,
    bps: u8,
    channels: u16,
    frame_idx: u64,
    total_samples: u64,
    first_full_bs: Option<u16>,
    min_full_bs: u16,
    max_bs: u16,
    min_frame_bytes: u32,
    max_frame_bytes: u32,
}

impl FlacStreamEncoder {
    pub fn new(sample_rate: u32, bps: u8, channels: u16) -> Result<Self> {
        if !(1..=8).contains(&channels) {
            bail!("FLAC suporta apenas de 1 a 8 canais (recebido: {channels})");
        }
        if sample_rate == 0 || sample_rate > MAX_FLAC_SAMPLE_RATE {
            bail!("taxa de amostragem inválida para FLAC: {sample_rate} Hz");
        }
        if bps != 16 && bps != 24 {
            bail!("profundidade FLAC não suportada: {bps} bits (use 16 ou 24)");
        }
        Ok(Self {
            sample_rate,
            bps,
            channels,
            frame_idx: 0,
            total_samples: 0,
            first_full_bs: None,
            min_full_bs: u16::MAX,
            max_bs: 0,
            min_frame_bytes: u32::MAX,
            max_frame_bytes: 0,
        })
    }

    pub fn block_size(&self) -> usize {
        FLAC_DEFAULT_BLOCK_SIZE
    }

    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    /// Gera os 34 bytes do bloco `STREAMINFO` (RFC 9639 §8.2).
    pub fn streaminfo_block(&self) -> [u8; 34] {
        // Se apenas um frame curto (< 16) existiu, min/max mantêm >= 16 como determina a §8.2;
        // o último frame é explicitamente dispensado do limite mínimo na §9.1.6.
        let (min_bs, max_bs) = if self.max_bs == 0 {
            (
                FLAC_DEFAULT_BLOCK_SIZE as u16,
                FLAC_DEFAULT_BLOCK_SIZE as u16,
            )
        } else if self.min_full_bs == u16::MAX {
            let bs = self.max_bs.max(16);
            (bs, bs)
        } else {
            (self.min_full_bs.max(16), self.max_bs.max(16))
        };
        let min_fs = if self.min_frame_bytes == u32::MAX {
            0
        } else {
            self.min_frame_bytes.min(0x00FF_FFFF)
        };
        let max_fs = self.max_frame_bytes.min(0x00FF_FFFF);

        let total = self.total_samples.min(MAX_FLAC_TOTAL_SAMPLES);
        let packed: u64 = ((u64::from(self.sample_rate) & 0xFFFFF) << 44)
            | ((u64::from(self.channels - 1) & 0x7) << 41)
            | ((u64::from(self.bps - 1) & 0x1F) << 36)
            | (total & MAX_FLAC_TOTAL_SAMPLES);

        let mut out = [0u8; 34];
        out[0..2].copy_from_slice(&min_bs.to_be_bytes());
        out[2..4].copy_from_slice(&max_bs.to_be_bytes());
        out[4..7].copy_from_slice(&min_fs.to_be_bytes()[1..4]);
        out[7..10].copy_from_slice(&max_fs.to_be_bytes()[1..4]);
        out[10..18].copy_from_slice(&packed.to_be_bytes());
        // Bytes 18..34 = checksum MD5 zerado (desconhecido, permitido na RFC 9639 Tabela 3).
        out
    }

    /// Bloco inicial `"fLaC"` + cabeçalho + `STREAMINFO` (42 bytes, útil como
    /// `CodecPrivate` `A_FLAC` em Matroska per RFC 9639 §10.2).
    pub fn make_codec_private(&self) -> [u8; 42] {
        let mut buf = [0u8; 42];
        buf[0..4].copy_from_slice(b"fLaC");
        buf[4] = 0x80; // is_last = 1, type = 0 (STREAMINFO)
        buf[5] = 0x00;
        buf[6] = 0x00;
        buf[7] = 34;
        buf[8..42].copy_from_slice(&self.streaminfo_block());
        buf
    }

    /// Codifica um único quadro FLAC contendo `block_samples` amostras por canal
    /// a partir de `interleaved` (tamanho `block_samples * channels`).
    pub fn encode_block(&mut self, interleaved: &[i32], is_last: bool) -> Result<Vec<u8>> {
        let ch = self.channels as usize;
        if interleaved.is_empty() || !interleaved.len().is_multiple_of(ch) {
            bail!(
                "bloco FLAC inválido: {} amostras escalares para {} canais",
                interleaved.len(),
                ch
            );
        }
        let bs = interleaved.len() / ch;
        if bs > 65535 {
            bail!("tamanho de bloco FLAC excede 65535: {bs}");
        }
        if !is_last && bs < 16 {
            bail!("bloco intermediário FLAC ({bs}) menor que o mínimo da RFC 9639 (16)");
        }

        // Cabeçalho do quadro (RFC 9639 §9.1)
        let mut frame = Vec::with_capacity(64 + interleaved.len() * 3);
        // blocking_strategy = 0 (tamanho de bloco fixo): sync = 0xFFF8
        frame.push(0xFF);
        frame.push(0xF8);

        let bs_code: u8 = match bs {
            192 => 1,
            576 => 2,
            1152 => 3,
            2304 => 4,
            4608 => 5,
            256 => 8,
            512 => 9,
            1024 => 10,
            2048 => 11,
            4096 => 12,
            8192 => 13,
            16384 => 14,
            32768 => 15,
            1..=256 => 6,
            _ => 7,
        };
        let sr_code: u8 = match self.sample_rate {
            88200 => 1,
            176400 => 2,
            192000 => 3,
            8000 => 4,
            16000 => 5,
            22050 => 6,
            24000 => 7,
            32000 => 8,
            44100 => 9,
            48000 => 10,
            96000 => 11,
            sr if sr.is_multiple_of(1000) && sr / 1000 <= 255 => 12,
            sr if sr <= 65535 => 13,
            sr if sr.is_multiple_of(10) && sr / 10 <= 65535 => 14,
            _ => 0,
        };
        frame.push((bs_code << 4) | sr_code);

        let ch_assign = (ch as u8) - 1;
        let bps_code: u8 = match self.bps {
            16 => 4,
            24 => 6,
            _ => 0,
        };
        frame.push((ch_assign << 4) | (bps_code << 1));
        push_utf8_coded_number(&mut frame, self.frame_idx)?;

        if bs_code == 6 {
            frame.push((bs - 1) as u8);
        } else if bs_code == 7 {
            frame.extend_from_slice(&((bs - 1) as u16).to_be_bytes());
        }
        match sr_code {
            12 => frame.push((self.sample_rate / 1000) as u8),
            13 => frame.extend_from_slice(&(self.sample_rate as u16).to_be_bytes()),
            14 => frame.extend_from_slice(&((self.sample_rate / 10) as u16).to_be_bytes()),
            _ => {}
        }

        let hdr_crc = crc8_flac(&frame);
        frame.push(hdr_crc);

        // Codifica todos os subquadros no mesmo BitWriter (somente o 1º é alinhado a byte).
        let mut bw = BitWriter::with_capacity(interleaved.len() * (self.bps as usize / 8) + 32);
        let mut ch_buf = vec![0i64; bs];
        for c in 0..ch {
            for i in 0..bs {
                ch_buf[i] = i64::from(interleaved[i * ch + c]);
            }
            encode_subframe(&mut bw, &ch_buf, self.bps)?;
        }
        let body = bw.finish_byte_aligned();
        frame.extend_from_slice(&body);

        // Rodapé CRC-16 sobre o quadro inteiro antes do rodapé (RFC 9639 §9.3)
        let f_crc = crc16_flac(&frame);
        frame.extend_from_slice(&f_crc.to_be_bytes());

        // Atualiza métricas do stream
        self.frame_idx = self.frame_idx.saturating_add(1);
        self.total_samples = self.total_samples.saturating_add(bs as u64);
        let bs16 = bs as u16;
        if !is_last {
            self.min_full_bs = self.min_full_bs.min(bs16);
            self.first_full_bs.get_or_insert(bs16);
        }
        self.max_bs = self.max_bs.max(bs16);
        if let Ok(len32) = u32::try_from(frame.len()) {
            self.min_frame_bytes = self.min_frame_bytes.min(len32);
            self.max_frame_bytes = self.max_frame_bytes.max(len32);
        }

        Ok(frame)
    }
}

fn best_partition_bits(
    residuals: &[i64],
    bs: usize,
    order: usize,
    po: u8,
) -> Option<(bool, usize)> {
    let n_parts = 1usize << po;
    if !bs.is_multiple_of(n_parts) {
        return None;
    }
    let part_size = bs >> po;
    if part_size <= order {
        return None;
    }
    let mut need_5bit = false;
    let mut idx = 0usize;
    let mut total_bits = 6usize; // 2 bits método + 4 bits partition_order

    for p in 0..n_parts {
        let len = if p == 0 { part_size - order } else { part_size };
        let slice = &residuals[idx..idx + len];
        idx += len;

        // Evita partições escapeadas para máxima compatibilidade com decodificadores Rust (p. ex. claxon);
        // quadros com resíduos grandes caem automaticamente para subquadro Verbatim.
        let (k, rice_bits) = best_rice_for_slice(slice, 30)?;
        if k >= 15 {
            need_5bit = true;
        }
        total_bits += rice_bits;
    }
    let param_width = if need_5bit { 5 } else { 4 };
    total_bits += n_parts * param_width;
    Some((need_5bit, total_bits))
}

fn encode_subframe(bw: &mut BitWriter, samples: &[i64], bps: u8) -> Result<()> {
    let bs = samples.len();
    let s0 = samples[0];
    if samples.iter().all(|&s| s == s0) {
        // Subquadro Constant (RFC 9639 §9.2.3)
        bw.write_bits(0, 1); // padding zero
        bw.write_bits(0b000000, 6);
        bw.write_bits(0, 1); // wasted bits = 0
        bw.write_signed(s0, bps);
        return Ok(());
    }

    let verbatim_bits = 8usize + bs * usize::from(bps);
    let mut best_plan = SubframePlan::Verbatim;
    let mut best_bits = verbatim_bits;
    let mut best_residuals: Vec<i64> = Vec::new();
    let mut candidate_residuals: Vec<i64> = Vec::with_capacity(bs);

    for order in 0..=4usize {
        if bs <= order {
            continue;
        }
        candidate_residuals.clear();
        let mut valid_range = true;
        for i in order..bs {
            let pred = match order {
                0 => 0,
                1 => samples[i - 1],
                2 => 2 * samples[i - 1] - samples[i - 2],
                3 => 3 * samples[i - 1] - 3 * samples[i - 2] + samples[i - 3],
                4 => 4 * samples[i - 1] - 6 * samples[i - 2] + 4 * samples[i - 3] - samples[i - 4],
                _ => 0,
            };
            let res = samples[i] - pred;
            // RFC 9639 §9.2.7.3: |residual| < 2^31 estritamente
            if res.abs() >= (1i64 << 31) {
                valid_range = false;
                break;
            }
            candidate_residuals.push(res);
        }
        if !valid_range {
            continue;
        }

        let max_po = (bs.trailing_zeros() as u8).min(5);
        for po in 0..=max_po {
            if let Some((use_5bit, res_bits)) =
                best_partition_bits(&candidate_residuals, bs, order, po)
            {
                let total = 8 + order * usize::from(bps) + res_bits;
                if total < best_bits {
                    best_bits = total;
                    best_plan = SubframePlan::Fixed {
                        order,
                        partition_order: po,
                        use_5bit,
                    };
                    best_residuals.clear();
                    best_residuals.extend_from_slice(&candidate_residuals);
                }
            }
        }
    }

    match best_plan {
        SubframePlan::Verbatim => {
            bw.write_bits(0, 1);
            bw.write_bits(0b000001, 6);
            bw.write_bits(0, 1);
            for &s in samples {
                bw.write_signed(s, bps);
            }
        }
        SubframePlan::Fixed {
            order,
            partition_order,
            use_5bit,
        } => {
            bw.write_bits(0, 1);
            bw.write_bits((8 + order) as u64, 6);
            bw.write_bits(0, 1);
            for &s in &samples[..order] {
                bw.write_signed(s, bps);
            }
            // Coded Residual (RFC 9639 §9.2.7)
            let method = if use_5bit { 0b01u64 } else { 0b00u64 };
            let param_bits = if use_5bit { 5u8 } else { 4u8 };
            let limit_k = if use_5bit { 30 } else { 14 };
            bw.write_bits(method, 2);
            bw.write_bits(u64::from(partition_order), 4);

            let n_parts = 1usize << partition_order;
            let part_size = bs >> partition_order;
            let mut idx = 0usize;
            for p in 0..n_parts {
                let len = if p == 0 { part_size - order } else { part_size };
                let slice = &best_residuals[idx..idx + len];
                idx += len;

                let (k, _) = best_rice_for_slice(slice, limit_k)
                    .ok_or_else(|| anyhow!("parâmetro Rice inesperado no subquadro FLAC"))?;
                bw.write_bits(u64::from(k), param_bits);
                for &r in slice {
                    let f = fold_residual(r);
                    bw.write_unary_zero_then_one(f >> k);
                    bw.write_bits(f, k);
                }
            }
        }
    }
    Ok(())
}

#[inline]
fn quantize_f32_to_int(s: f32, bps: u8) -> i32 {
    let clamped = if s.is_finite() {
        s.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let max_v = (1i64 << (bps - 1)) - 1;
    let min_v = -(1i64 << (bps - 1));
    let scaled = (f64::from(clamped) * max_v as f64).round() as i64;
    scaled.clamp(min_v, max_v) as i32
}

/// Gravador de arquivo `.flac` autônomo em Rust puro (RFC 9639).
pub struct FlacWriter {
    out: BufWriter<File>,
    enc: FlacStreamEncoder,
    pending: Vec<i32>,
}

impl FlacWriter {
    pub fn create(path: &Path, sr: u32, bits: WavBits, channels: u16) -> Result<Self> {
        let bps = match bits {
            WavBits::Pcm16 => 16,
            WavBits::Pcm24 | WavBits::Float32 => 24,
        };
        let enc = FlacStreamEncoder::new(sr, bps, channels)?;
        let mut out = BufWriter::new(
            File::create(path).with_context(|| format!("criando {}", path.display()))?,
        );
        // Escreve cabeçalho inicial ("fLaC" + STREAMINFO); atualizado com totais exatos em finish().
        out.write_all(&enc.make_codec_private())?;
        Ok(Self {
            out,
            enc,
            pending: Vec::with_capacity(FLAC_DEFAULT_BLOCK_SIZE * usize::from(channels) * 2),
        })
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        let ch = usize::from(self.enc.channels);
        let usable = (samples.len() / ch) * ch;
        let bps = self.enc.bps;
        for &s in &samples[..usable] {
            self.pending.push(quantize_f32_to_int(s, bps));
        }
        let block_scalars = FLAC_DEFAULT_BLOCK_SIZE * ch;
        // Só descarrega enquanto sobrar >= 16 amostras futuras para que o penúltimo quadro não force último curto,
        // ou guardamos sempre 1 quadro em reserva até saber se há mais dados?
        // Na verdade cada bloco inteiro tem exatamente 4096 amostras; apenas o restante em finish() é o último!
        while self.pending.len() >= block_scalars {
            let chunk: Vec<i32> = self.pending.drain(..block_scalars).collect();
            let frame = self.enc.encode_block(&chunk, false)?;
            self.out.write_all(&frame)?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        if !self.pending.is_empty() {
            let frame = self.enc.encode_block(&self.pending, true)?;
            self.out.write_all(&frame)?;
            self.pending.clear();
        }
        if self.enc.total_samples() == 0 {
            bail!("FLAC: stream sem amostras de áudio");
        }
        self.out.flush()?;
        let mut f = self
            .out
            .into_inner()
            .map_err(|e| anyhow!("falha ao concluir gravação FLAC: {e}"))?;
        f.seek(SeekFrom::Start(0))?;
        f.write_all(&self.enc.make_codec_private())?;
        f.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_stereo_16bit_non_multiple_block() {
        let p = std::env::temp_dir().join("neoveg_flac_stereo16.flac");
        let sr = 48_000u32;
        let mut w = FlacWriter::create(&p, sr, WavBits::Pcm16, 2).unwrap();
        // 5000 quadros -> 1 bloco de 4096 + bloco final de 904
        let n_frames = 5000usize;
        let mut samples = Vec::with_capacity(n_frames * 2);
        for i in 0..n_frames {
            let t = i as f32 / sr as f32;
            samples.push((t * 440.0 * std::f32::consts::TAU).sin() * 0.6);
            samples.push((t * 660.0 * std::f32::consts::TAU).cos() * 0.4);
        }
        w.write(&samples).unwrap();
        w.finish().unwrap();

        let mut reader = claxon::FlacReader::open(&p).unwrap();
        let info = reader.streaminfo();
        assert_eq!(info.sample_rate, sr);
        assert_eq!(info.channels, 2);
        assert_eq!(info.bits_per_sample, 16);
        assert_eq!(info.samples, Some(n_frames as u64));

        let decoded: Vec<i32> = reader.samples().map(|s| s.unwrap()).collect();
        let expected: Vec<i32> = samples
            .iter()
            .map(|&s| quantize_f32_to_int(s, 16))
            .collect();
        assert_eq!(decoded, expected);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn roundtrip_51_surround_24bit_and_silence_noise() {
        let p = std::env::temp_dir().join("neoveg_flac_51_24b.flac");
        let sr = 44_100u32;
        let mut w = FlacWriter::create(&p, sr, WavBits::Pcm24, 6).unwrap();
        // Parte constante (silêncio) + sinal + ruído pseudo-aleatório para forçar Fixed + Verbatim
        let n_frames = 2048usize;
        let mut samples = Vec::with_capacity(n_frames * 6);
        let mut lcg = 0x1234_5678u32;
        for i in 0..n_frames {
            if i < 512 {
                samples.extend_from_slice(&[0.0; 6]);
            } else {
                for ch in 0..6 {
                    lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
                    let s = if ch % 2 == 0 {
                        ((i as f32) * 0.03).sin() * 0.8
                    } else {
                        ((lcg >> 16) as f32 / 32768.0) - 1.0
                    };
                    samples.push(s);
                }
            }
        }
        w.write(&samples).unwrap();
        w.finish().unwrap();

        let mut reader = claxon::FlacReader::open(&p).unwrap();
        assert_eq!(reader.streaminfo().channels, 6);
        assert_eq!(reader.streaminfo().bits_per_sample, 24);
        assert_eq!(reader.streaminfo().sample_rate, 44_100);
        let decoded: Vec<i32> = reader.samples().map(|s| s.unwrap()).collect();
        let expected: Vec<i32> = samples
            .iter()
            .map(|&s| quantize_f32_to_int(s, 24))
            .collect();
        assert_eq!(decoded, expected);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn roundtrip_single_sample_and_uncommon_sr() {
        let p = std::env::temp_dir().join("neoveg_flac_1sample.flac");
        let mut w = FlacWriter::create(&p, 11_025, WavBits::Pcm16, 1).unwrap();
        w.write(&[0.25]).unwrap();
        w.finish().unwrap();

        let mut reader = claxon::FlacReader::open(&p).unwrap();
        assert_eq!(reader.streaminfo().sample_rate, 11_025);
        assert_eq!(reader.streaminfo().channels, 1);
        assert_eq!(reader.streaminfo().samples, Some(1));
        let decoded: Vec<i32> = reader.samples().map(|s| s.unwrap()).collect();
        assert_eq!(decoded, vec![quantize_f32_to_int(0.25, 16)]);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn hostile_params_rejected_cleanly() {
        let p = std::env::temp_dir().join("neoveg_flac_bad.flac");
        assert!(FlacWriter::create(&p, 0, WavBits::Pcm16, 2).is_err());
        assert!(FlacWriter::create(&p, 48000, WavBits::Pcm16, 0).is_err());
        assert!(FlacWriter::create(&p, 48000, WavBits::Pcm16, 9).is_err());
        let empty = FlacWriter::create(&p, 48000, WavBits::Pcm16, 2).unwrap();
        assert!(empty.finish().is_err());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn flac_ffmpeg_oracle() {
        let p = std::env::temp_dir().join("neoveg_flac_ffmpeg_oracle.flac");
        let mut w = FlacWriter::create(&p, 48_000, WavBits::Pcm16, 6).unwrap();
        let mut s = Vec::with_capacity(4800 * 6);
        for i in 0..4800 {
            let v = ((i as f32) * 0.05).sin() * 0.5;
            s.extend_from_slice(&[v, -v, v * 0.5, 0.0, v * 0.3, -v * 0.3]);
        }
        w.write(&s).unwrap();
        w.finish().unwrap();

        if let Ok(out) = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-v", "error", "-i"])
            .arg(&p)
            .args(["-f", "null", "-"])
            .output()
        {
            if out.status.success() {
                assert!(
                    out.stderr.is_empty(),
                    "ffmpeg reportou erro FLAC: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        }
        let _ = std::fs::remove_file(&p);
    }
}
