//! Codificador de vídeo sem perdas FFV1 v3 em Rust puro (`#[forbid(unsafe_code)]`, Sala Limpa).
//!
//! Implementado a partir da especificação pública:
//! - IETF RFC 9043 (*FFV1 Video Coding Format Versions 0, 1, and 3*, agosto de 2021):
//!   - `ConfigurationRecord` + `Parameters` v3 (`ec=1`, `intra=1`, `coder_type=1`, `colorspace_type=1`) (§4.2–4.3);
//!   - Codificador por faixa (*Range Coder*) e inteiros não binários (`put_symbol`) (§3.8.1);
//!   - Transformada reversível de cor JPEG 2000 RCT (Y, Cb, Cr + plano extra Alfa opcional) (§3.7.2);
//!   - Preditor mediano e fronteiras por slice (§3.1–3.3), múltiplas slices acima de 352×288 (§5) e paridade CRC-32 IEEE (§4.9.3).

use anyhow::{bail, Result};

/// Polinômio IEEE CRC-32 MSB-first sem pré/pós-inversão (RFC 9043 §4.9.3).
fn crc32_ffv1(data: &[u8]) -> u32 {
    let mut crc = 0u32;
    for &b in data {
        crc ^= u32::from(b) << 24;
        for _ in 0..8 {
            if (crc & 0x8000_0000) != 0 {
                crc = (crc << 1) ^ 0x04C1_1DB7;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Tabela padrão de transição de estados para o Range Coder (RFC 9043 §3.8.1.5).
#[rustfmt::skip]
const DEFAULT_STATE_TRANSITION: [u8; 256] = [
      0,  0,  0,  0,  0,  0,  0,  0, 20, 21, 22, 23, 24, 25, 26, 27,
     28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 37, 38, 39, 40, 41, 42,
     43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 56, 57,
     58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73,
     74, 75, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88,
     89, 90, 91, 92, 93, 94, 94, 95, 96, 97, 98, 99,100,101,102,103,
    104,105,106,107,108,109,110,111,112,113,114,114,115,116,117,118,
    119,120,121,122,123,124,125,126,127,128,129,130,131,132,133,133,
    134,135,136,137,138,139,140,141,142,143,144,145,146,147,148,149,
    150,151,152,152,153,154,155,156,157,158,159,160,161,162,163,164,
    165,166,167,168,169,170,171,171,172,173,174,175,176,177,178,179,
    180,181,182,183,184,185,186,187,188,189,190,190,191,192,194,194,
    195,196,197,198,199,200,201,202,202,204,205,206,207,208,209,209,
    210,211,212,213,215,215,216,217,218,219,220,220,222,223,224,225,
    226,227,227,229,229,230,231,232,234,234,235,236,237,238,239,240,
    241,242,243,244,245,246,247,248,248,  0,  0,  0,  0,  0,  0,  0,
];

fn build_state_tables() -> ([u8; 256], [u8; 256]) {
    let one = DEFAULT_STATE_TRANSITION;
    let mut zero = [0u8; 256];
    for i in 1..256 {
        zero[i] = 256u16.saturating_sub(u16::from(one[256 - i])) as u8;
    }
    (zero, one)
}

struct RangeEncoder {
    low: u32,
    range: u32,
    outstanding_byte: i32,
    outstanding_count: usize,
    out: Vec<u8>,
    zero_state: [u8; 256],
    one_state: [u8; 256],
}

impl RangeEncoder {
    fn with_capacity(cap: usize) -> Self {
        let (zero_state, one_state) = build_state_tables();
        Self {
            low: 0,
            range: 0xFF00,
            outstanding_byte: -1,
            outstanding_count: 0,
            out: Vec::with_capacity(cap),
            zero_state,
            one_state,
        }
    }

    #[inline]
    fn renorm(&mut self) {
        if self.range < 0x100 {
            if self.low < 0xFF00 {
                if self.outstanding_byte >= 0 {
                    self.out.push(self.outstanding_byte as u8);
                }
                for _ in 0..self.outstanding_count {
                    self.out.push(0xFF);
                }
                self.outstanding_count = 0;
                self.outstanding_byte = (self.low >> 8) as i32;
            } else if self.low >= 0x1_0000 {
                if self.outstanding_byte >= 0 {
                    self.out.push((self.outstanding_byte + 1) as u8);
                } else {
                    self.out.push(1);
                }
                for _ in 0..self.outstanding_count {
                    self.out.push(0x00);
                }
                self.outstanding_count = 0;
                self.outstanding_byte = ((self.low >> 8) & 0xFF) as i32;
            } else {
                self.outstanding_count += 1;
            }
            self.range <<= 8;
            self.low = (self.low & 0xFF) << 8;
        }
    }

    #[inline]
    fn put_rac(&mut self, state: &mut u8, bit: bool) {
        let rangeoff = (self.range * u32::from(*state)) >> 8;
        self.range -= rangeoff;
        if bit {
            self.low += self.range;
            self.range = rangeoff;
            *state = self.one_state[*state as usize];
        } else {
            *state = self.zero_state[*state as usize];
        }
        self.renorm();
    }

    /// Codificação de escalar não binário no Range Coder (espelho da Figura 21 da RFC 9043).
    #[inline]
    fn put_symbol(&mut self, state: &mut [u8; 32], val: i32, is_signed: bool) {
        if val == 0 {
            self.put_rac(&mut state[0], true);
            return;
        }
        self.put_rac(&mut state[0], false);
        let a = val.unsigned_abs();
        let e = (31 - a.leading_zeros()) as usize;
        for i in 0..e {
            self.put_rac(&mut state[1 + i.min(9)], true);
        }
        self.put_rac(&mut state[1 + e.min(9)], false);
        for i in (0..e).rev() {
            self.put_rac(&mut state[22 + i.min(9)], ((a >> i) & 1) != 0);
        }
        if is_signed {
            self.put_rac(&mut state[11 + e.min(10)], val < 0);
        }
    }

    fn finish(mut self, sentinel: bool) -> Vec<u8> {
        if sentinel {
            let mut dummy = 129u8;
            self.put_rac(&mut dummy, false);
        }
        let c = (self.low + 0xFF) & 0xFFFF_FF00;
        if c < 0x1_0000 {
            if self.outstanding_byte >= 0 {
                self.out.push(self.outstanding_byte as u8);
            }
            for _ in 0..self.outstanding_count {
                self.out.push(0xFF);
            }
            self.out.push((c >> 8) as u8);
        } else {
            if self.outstanding_byte >= 0 {
                self.out.push((self.outstanding_byte + 1) as u8);
            } else {
                self.out.push(1);
            }
            for _ in 0..self.outstanding_count {
                self.out.push(0x00);
            }
            self.out.push(((c >> 8) & 0xFF) as u8);
        }
        while self.out.len() < 2 {
            self.out.push(0);
        }
        self.out
    }
}

#[inline]
fn median3(a: i32, b: i32, c: i32) -> i32 {
    a + b + c - a.min(b).min(c) - a.max(b).max(c)
}

pub struct Ffv1Encoder {
    width: u32,
    height: u32,
    has_alpha: bool,
    num_h_slices: u32,
    num_v_slices: u32,
    config_record: Vec<u8>,
}

impl Ffv1Encoder {
    pub fn new(width: u32, height: u32, has_alpha: bool) -> Result<Self> {
        if width == 0 || height == 0 {
            bail!("dimensões FFV1 inválidas: {width}x{height}");
        }
        // RFC 9043 §5: acima de CIF (352x288 = 101376 px), versão 3 exige >= 4 slices.
        let (num_h_slices, num_v_slices) =
            if width >= 2 && height >= 2 && u64::from(width) * u64::from(height) > 101_376 {
                (2u32, 2u32)
            } else {
                (1u32, 1u32)
            };

        let mut rc = RangeEncoder::with_capacity(64);
        let mut p_state = [128u8; 32];
        rc.put_symbol(&mut p_state, 3, false); // version = 3
        rc.put_symbol(&mut p_state, 4, false); // micro_version = 4
        rc.put_symbol(&mut p_state, 1, false); // coder_type = 1 (Range coder padrão)
        rc.put_symbol(&mut p_state, 1, false); // colorspace_type = 1 (RGB JPEG2000-RCT)
        rc.put_symbol(&mut p_state, 8, false); // bits_per_raw_sample = 8
        rc.put_rac(&mut p_state[0], true); // chroma_planes = 1
        rc.put_symbol(&mut p_state, 0, false); // log2_h_chroma_subsample = 0
        rc.put_symbol(&mut p_state, 0, false); // log2_v_chroma_subsample = 0
        rc.put_rac(&mut p_state[0], has_alpha); // extra_plane
        rc.put_symbol(&mut p_state, (num_h_slices - 1) as i32, false);
        rc.put_symbol(&mut p_state, (num_v_slices - 1) as i32, false);
        rc.put_symbol(&mut p_state, 1, false); // quant_table_set_count = 1

        // QuantizationTableSet(0): cada uma das 5 tabelas tem seu próprio estado inicial [128; 32] (RFC 9043 §4.1).
        for _ in 0..5 {
            let mut q_state = [128u8; 32];
            rc.put_symbol(&mut q_state, 127, false);
        }

        rc.put_rac(&mut p_state[0], false); // states_coded = 0 (todos iniciais 128)
        rc.put_symbol(&mut p_state, 1, false); // ec = 1 (CRC-32 nas slices)
        rc.put_symbol(&mut p_state, 1, false); // intra = 1 (apenas quadros-chave)

        let mut config_record = rc.finish(false);
        let crc = crc32_ffv1(&config_record);
        config_record.extend_from_slice(&crc.to_be_bytes());

        Ok(Self {
            width,
            height,
            has_alpha,
            num_h_slices,
            num_v_slices,
            config_record,
        })
    }

    pub fn configuration_record(&self) -> &[u8] {
        &self.config_record
    }

    pub fn has_alpha(&self) -> bool {
        self.has_alpha
    }

    /// Codifica um quadro RGBA8 (`width * height * 4`) para um pacote FFV1 completo.
    pub fn encode_frame(&self, rgba: &[u8]) -> Result<Vec<u8>> {
        let expected_len = (self.width as usize)
            .saturating_mul(self.height as usize)
            .saturating_mul(4);
        if rgba.len() < expected_len {
            bail!(
                "buffer RGBA FFV1 menor que o esperado ({} < {})",
                rgba.len(),
                expected_len
            );
        }

        let mut frame = Vec::with_capacity(expected_len + 256);
        let nh = self.num_h_slices;
        let nv = self.num_v_slices;
        for sy in 0..nv {
            for sx in 0..nh {
                let is_first = sx == 0 && sy == 0;
                let slice_bytes = self.encode_slice(rgba, sx, sy, is_first)?;
                frame.extend_from_slice(&slice_bytes);
            }
        }
        Ok(frame)
    }

    fn encode_slice(&self, rgba: &[u8], sx: u32, sy: u32, is_first: bool) -> Result<Vec<u8>> {
        let x0 = ((sx as usize) * (self.width as usize)) / (self.num_h_slices as usize);
        let x1 = (((sx + 1) as usize) * (self.width as usize)) / (self.num_h_slices as usize);
        let y0 = ((sy as usize) * (self.height as usize)) / (self.num_v_slices as usize);
        let y1 = (((sy + 1) as usize) * (self.height as usize)) / (self.num_v_slices as usize);
        let sw = x1.saturating_sub(x0);
        let sh = y1.saturating_sub(y0);

        let mut rc = RangeEncoder::with_capacity(sw * sh * 4 + 64);
        if is_first {
            let mut kf_state = 128u8;
            rc.put_rac(&mut kf_state, true); // keyframe = 1
        }

        // SliceHeader (RFC 9043 §4.6)
        let mut sh_state = [128u8; 32];
        let q_idx_count = 2 + usize::from(self.has_alpha);
        rc.put_symbol(&mut sh_state, sx as i32, false);
        rc.put_symbol(&mut sh_state, sy as i32, false);
        rc.put_symbol(&mut sh_state, 0, false); // slice_width - 1 = 0
        rc.put_symbol(&mut sh_state, 0, false); // slice_height - 1 = 0
        for _ in 0..q_idx_count {
            rc.put_symbol(&mut sh_state, 0, false); // quant_table_set_index = 0
        }
        rc.put_symbol(&mut sh_state, 3, false); // picture_structure = progressive
        rc.put_symbol(&mut sh_state, 0, false); // sar_num
        rc.put_symbol(&mut sh_state, 0, false); // sar_den

        // Estados de contexto por QuantizationTableSetIndex:
        // Y -> índice 0, Cb e Cr compartilham índice 1, Alfa -> índice 2 (RFC 9043 §3.6).
        let planes = if self.has_alpha { 4usize } else { 3usize };
        let mut plane_states = [[128u8; 32]; 3];

        // Buffers de linhas (com borda: [col -2, col -1, cols 0..sw-1, col sw])
        let row_len = sw + 3;
        let mut prev_row = vec![vec![0i32; row_len]; planes];
        let mut cur_row = vec![vec![0i32; row_len]; planes];

        for ry in 0..sh {
            let gy = y0 + ry;
            let row_px_start = gy * (self.width as usize);
            // Preenche as amostras transformadas (JPEG2000-RCT) da linha atual nas colunas 2..sw+2
            for p in 0..planes {
                cur_row[p][0] = 0;
                cur_row[p][1] = if ry == 0 { 0 } else { prev_row[p][2] };
            }
            for rx in 0..sw {
                let pi = (row_px_start + x0 + rx) * 4;
                let r = i32::from(rgba[pi]);
                let g = i32::from(rgba[pi + 1]);
                let b = i32::from(rgba[pi + 2]);
                let a = i32::from(rgba[pi + 3]);

                let cb = b - g;
                let cr = r - g;
                let y = g + ((cb + cr) >> 2);
                cur_row[0][2 + rx] = y;
                cur_row[1][2 + rx] = cb + 256;
                cur_row[2][2 + rx] = cr + 256;
                if self.has_alpha {
                    cur_row[3][2 + rx] = a;
                }
            }
            for row in cur_row.iter_mut().take(planes) {
                row[sw + 2] = row[sw + 1];
            }

            // Codifica linhas por plano (RGB intercala linha a linha: Y, Cb, Cr [, Alfa]).
            // No RCT (colorspace_type == 1), todos os planos codificam diferenças de 9 bits (RFC 9043 §3.8).
            for p in 0..planes {
                let state_idx = if p == 0 {
                    0
                } else if p <= 2 {
                    1
                } else {
                    2
                };
                let bits = 9i32;
                let half = 1i32 << (bits - 1);
                let mask = (1i32 << bits) - 1;
                for rx in 0..sw {
                    let c = 2 + rx;
                    let l = cur_row[p][c - 1];
                    let t = prev_row[p][c];
                    let tl = prev_row[p][c - 1];
                    let pred = median3(l, t, l + t - tl);
                    let raw_diff = cur_row[p][c] - pred;
                    let wrapped = ((raw_diff + half) & mask) - half;
                    rc.put_symbol(&mut plane_states[state_idx], wrapped, true);
                }
            }

            std::mem::swap(&mut prev_row, &mut cur_row);
        }

        let mut slice_buf = rc.finish(true);
        let slice_sz =
            u32::try_from(slice_buf.len()).map_err(|_| anyhow_slice_overflow(slice_buf.len()))?;
        if slice_sz > 0x00FF_FFFF {
            bail!("slice FFV1 excede 16 MiB ({slice_sz} bytes)");
        }
        // SliceFooter (RFC 9043 §4.9): u(24) slice_size + u(8) error_status=0 + u(32) crc_parity
        slice_buf.extend_from_slice(&slice_sz.to_be_bytes()[1..4]);
        slice_buf.push(0); // error_status = 0
        let crc = crc32_ffv1(&slice_buf);
        slice_buf.extend_from_slice(&crc.to_be_bytes());
        Ok(slice_buf)
    }
}

fn anyhow_slice_overflow(len: usize) -> anyhow::Error {
    anyhow::anyhow!("slice FFV1 excede limite u32 ({len} bytes)")
}

/// Limite defensivo de dimensão aceita pelo decodificador FFV1 (§0.4).
const MAX_FFV1_DIM: u32 = 16_384;
/// Limite de slices por eixo (§0.4).
const MAX_FFV1_SLICES: u32 = 64;

struct RangeDecoder<'a> {
    low: u32,
    range: u32,
    buf: &'a [u8],
    pos: usize,
    zero_state: [u8; 256],
    one_state: [u8; 256],
}

impl<'a> RangeDecoder<'a> {
    fn new(buf: &'a [u8]) -> Self {
        let (zero_state, one_state) = build_state_tables();
        let b0 = buf.first().copied().unwrap_or(0);
        let b1 = buf.get(1).copied().unwrap_or(0);
        let mut low = (u32::from(b0) << 8) | u32::from(b1);
        let range = 0xFF00;
        if low >= range {
            low = range - 1;
        }
        Self {
            low,
            range,
            buf,
            pos: 2,
            zero_state,
            one_state,
        }
    }

    #[inline]
    fn renorm(&mut self) {
        if self.range < 0x100 {
            self.range <<= 8;
            let next = self.buf.get(self.pos).copied().unwrap_or(0);
            self.pos = self.pos.saturating_add(1);
            self.low = ((self.low & 0xFF) << 8) | u32::from(next);
        }
    }

    #[inline]
    fn get_rac(&mut self, state: &mut u8) -> bool {
        let rangeoff = (self.range * u32::from(*state)) >> 8;
        self.range -= rangeoff;
        let bit = if self.low < self.range {
            *state = self.zero_state[*state as usize];
            false
        } else {
            self.low -= self.range;
            self.range = rangeoff;
            *state = self.one_state[*state as usize];
            true
        };
        self.renorm();
        bit
    }

    /// Decodificação de escalar não binário no Range Coder (espelho da RFC 9043 §3.8.1).
    #[inline]
    fn get_symbol(&mut self, state: &mut [u8; 32], is_signed: bool) -> i32 {
        if self.get_rac(&mut state[0]) {
            return 0;
        }
        let mut e = 0usize;
        while e < 30 && self.get_rac(&mut state[1 + e.min(9)]) {
            e += 1;
        }
        let mut a = 1u32 << e;
        for i in (0..e).rev() {
            if self.get_rac(&mut state[22 + i.min(9)]) {
                a |= 1u32 << i;
            }
        }
        let val = a as i32;
        if is_signed && self.get_rac(&mut state[11 + e.min(10)]) {
            val.wrapping_neg()
        } else {
            val
        }
    }
}

/// Decodificador FFV1 v3 sem perdas em Rust puro (RFC 9043, Sala Limpa, §0).
#[derive(Clone, Debug)]
pub struct Ffv1Decoder {
    width: u32,
    height: u32,
    has_alpha: bool,
    num_h_slices: u32,
    num_v_slices: u32,
    ec: u32,
}

impl Ffv1Decoder {
    /// Analisa o `ConfigurationRecord` (Matroska `CodecPrivate`) com validação CRC-32.
    pub fn from_config_record(width: u32, height: u32, config: &[u8]) -> Result<Self> {
        if width == 0 || height == 0 || width > MAX_FFV1_DIM || height > MAX_FFV1_DIM {
            bail!("dimensões FFV1 inválidas: {width}x{height}");
        }
        if config.len() < 6 || config.len() > 65_536 {
            bail!(
                "ConfigurationRecord FFV1 com tamanho inválido ({} bytes)",
                config.len()
            );
        }
        let body_len = config.len() - 4;
        let expected_crc = u32::from_be_bytes([
            config[body_len],
            config[body_len + 1],
            config[body_len + 2],
            config[body_len + 3],
        ]);
        let actual_crc = crc32_ffv1(&config[..body_len]);
        if actual_crc != expected_crc {
            bail!("CRC-32 inválido no ConfigurationRecord FFV1");
        }

        let mut rc = RangeDecoder::new(&config[..body_len]);
        let mut p_state = [128u8; 32];
        let version = rc.get_symbol(&mut p_state, false);
        if version != 3 {
            bail!("versão FFV1 não suportada no decodificador fallback: {version} (esperada 3)");
        }
        let _micro_version = rc.get_symbol(&mut p_state, false);
        let coder_type = rc.get_symbol(&mut p_state, false);
        if coder_type != 1 {
            bail!("coder_type FFV1 não suportado: {coder_type}");
        }
        let colorspace_type = rc.get_symbol(&mut p_state, false);
        if colorspace_type != 1 {
            bail!("colorspace_type FFV1 não suportado: {colorspace_type} (esperado 1 = RGB RCT)");
        }
        let bits = rc.get_symbol(&mut p_state, false);
        if bits != 8 {
            bail!("profundidade FFV1 não suportada: {bits} bits (esperado 8)");
        }
        let _chroma_planes = rc.get_rac(&mut p_state[0]);
        let _log2_h = rc.get_symbol(&mut p_state, false);
        let _log2_v = rc.get_symbol(&mut p_state, false);
        let has_alpha = rc.get_rac(&mut p_state[0]);
        let nh_m1 = rc.get_symbol(&mut p_state, false);
        let nv_m1 = rc.get_symbol(&mut p_state, false);
        let num_h_slices = u32::try_from(nh_m1)
            .ok()
            .and_then(|v| v.checked_add(1))
            .filter(|v| (1..=MAX_FFV1_SLICES).contains(v))
            .ok_or_else(|| anyhow::anyhow!("num_h_slices FFV1 inválido ({nh_m1})"))?;
        let num_v_slices = u32::try_from(nv_m1)
            .ok()
            .and_then(|v| v.checked_add(1))
            .filter(|v| (1..=MAX_FFV1_SLICES).contains(v))
            .ok_or_else(|| anyhow::anyhow!("num_v_slices FFV1 inválido ({nv_m1})"))?;
        let qt_count = rc.get_symbol(&mut p_state, false);
        if !(1..=8).contains(&qt_count) {
            bail!("quant_table_set_count FFV1 inválido: {qt_count}");
        }
        for _ in 0..qt_count {
            for _ in 0..5 {
                let mut q_state = [128u8; 32];
                let mut filled = 0i32;
                let mut guard = 0usize;
                while filled < 127 && guard < 256 {
                    let len = rc.get_symbol(&mut q_state, false).clamp(0, 127);
                    filled = filled.saturating_add(len.max(1));
                    guard += 1;
                }
            }
        }
        let states_coded = rc.get_rac(&mut p_state[0]);
        if states_coded {
            bail!("states_coded personalizado não suportado no decodificador FFV1 fallback");
        }
        let ec = u32::try_from(rc.get_symbol(&mut p_state, false)).unwrap_or(0);
        let _intra = rc.get_symbol(&mut p_state, false);

        Ok(Self {
            width,
            height,
            has_alpha,
            num_h_slices,
            num_v_slices,
            ec,
        })
    }

    /// Decodifica um pacote FFV1 v3 (uma ou mais slices) para RGBA8 (`width * height * 4`).
    pub fn decode_frame(&self, pkt: &[u8]) -> Result<Vec<u8>> {
        let total_px = (self.width as usize)
            .checked_mul(self.height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| anyhow::anyhow!("overflow no buffer RGBA FFV1"))?;
        if total_px > 256 * 1024 * 1024 {
            bail!("quadro FFV1 excede limite de memória ({total_px} bytes)");
        }
        if pkt.is_empty() {
            bail!("pacote FFV1 vazio");
        }

        let footer_len = if self.ec >= 1 { 8usize } else { 3usize };
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        let max_slices = (self.num_h_slices as usize).saturating_mul(self.num_v_slices as usize);
        let mut cursor = pkt.len();
        while cursor > 0 && ranges.len() <= max_slices {
            if cursor < footer_len {
                bail!("rodapé de slice FFV1 truncado");
            }
            let footer_start = cursor - footer_len;
            let sz_bytes = &pkt[footer_start..footer_start + 3];
            let slice_sz = ((usize::from(sz_bytes[0])) << 16)
                | ((usize::from(sz_bytes[1])) << 8)
                | usize::from(sz_bytes[2]);
            if slice_sz == 0 || slice_sz > footer_start {
                bail!("tamanho de slice FFV1 inválido ({slice_sz})");
            }
            let slice_start = footer_start - slice_sz;
            if self.ec >= 1 {
                let expected_crc = u32::from_be_bytes([
                    pkt[cursor - 4],
                    pkt[cursor - 3],
                    pkt[cursor - 2],
                    pkt[cursor - 1],
                ]);
                let actual_crc = crc32_ffv1(&pkt[slice_start..cursor - 4]);
                if actual_crc != expected_crc {
                    bail!("CRC-32 inválido em slice FFV1");
                }
            }
            ranges.push((slice_start, footer_start));
            cursor = slice_start;
        }
        if cursor != 0 || ranges.is_empty() {
            bail!("estrutura de slices FFV1 inconsistente");
        }
        ranges.reverse();

        let mut rgba = vec![0u8; total_px];
        for (idx, (start, end)) in ranges.into_iter().enumerate() {
            self.decode_slice(&pkt[start..end], idx == 0, &mut rgba)?;
        }
        Ok(rgba)
    }

    fn decode_slice(&self, slice_bytes: &[u8], is_first: bool, rgba: &mut [u8]) -> Result<()> {
        let mut rc = RangeDecoder::new(slice_bytes);
        if is_first {
            let mut kf_state = 128u8;
            let _kf = rc.get_rac(&mut kf_state);
        }
        let mut sh_state = [128u8; 32];
        let q_idx_count = 2 + usize::from(self.has_alpha);
        let sx = u32::try_from(rc.get_symbol(&mut sh_state, false))
            .ok()
            .filter(|&v| v < self.num_h_slices)
            .ok_or_else(|| anyhow::anyhow!("índice X de slice FFV1 fora da faixa"))?;
        let sy = u32::try_from(rc.get_symbol(&mut sh_state, false))
            .ok()
            .filter(|&v| v < self.num_v_slices)
            .ok_or_else(|| anyhow::anyhow!("índice Y de slice FFV1 fora da faixa"))?;
        let sw_m1 = u32::try_from(rc.get_symbol(&mut sh_state, false)).unwrap_or(0);
        let sh_m1 = u32::try_from(rc.get_symbol(&mut sh_state, false)).unwrap_or(0);
        let sx_end = sx
            .saturating_add(sw_m1)
            .saturating_add(1)
            .min(self.num_h_slices);
        let sy_end = sy
            .saturating_add(sh_m1)
            .saturating_add(1)
            .min(self.num_v_slices);
        for _ in 0..q_idx_count {
            let _ = rc.get_symbol(&mut sh_state, false);
        }
        let _pic_struct = rc.get_symbol(&mut sh_state, false);
        let _sar_num = rc.get_symbol(&mut sh_state, false);
        let _sar_den = rc.get_symbol(&mut sh_state, false);

        let x0 = ((sx as usize) * (self.width as usize)) / (self.num_h_slices as usize);
        let x1 = ((sx_end as usize) * (self.width as usize)) / (self.num_h_slices as usize);
        let y0 = ((sy as usize) * (self.height as usize)) / (self.num_v_slices as usize);
        let y1 = ((sy_end as usize) * (self.height as usize)) / (self.num_v_slices as usize);
        let sw = x1.saturating_sub(x0);
        let sh = y1.saturating_sub(y0);
        if sw == 0 || sh == 0 {
            return Ok(());
        }

        let planes = if self.has_alpha { 4usize } else { 3usize };
        let mut plane_states = [[128u8; 32]; 3];
        let row_len = sw + 3;
        let mut prev_row = vec![vec![0i32; row_len]; planes];
        let mut cur_row = vec![vec![0i32; row_len]; planes];
        let mask = (1i32 << 9) - 1;

        for ry in 0..sh {
            for p in 0..planes {
                cur_row[p][0] = 0;
                cur_row[p][1] = if ry == 0 { 0 } else { prev_row[p][2] };
            }
            for p in 0..planes {
                let state_idx = if p == 0 {
                    0
                } else if p <= 2 {
                    1
                } else {
                    2
                };
                for rx in 0..sw {
                    let c = 2 + rx;
                    let l = cur_row[p][c - 1];
                    let t = prev_row[p][c];
                    let tl = prev_row[p][c - 1];
                    let pred = median3(l, t, l + t - tl);
                    let diff = rc.get_symbol(&mut plane_states[state_idx], true);
                    cur_row[p][c] = (pred.wrapping_add(diff)) & mask;
                }
                cur_row[p][sw + 2] = cur_row[p][sw + 1];
            }

            let gy = y0 + ry;
            let row_px_start = gy * (self.width as usize);
            for rx in 0..sw {
                let y_val = cur_row[0][2 + rx];
                let cb = cur_row[1][2 + rx] - 256;
                let cr = cur_row[2][2 + rx] - 256;
                let g = y_val - ((cb + cr) >> 2);
                let b = cb + g;
                let r = cr + g;
                let a = if self.has_alpha {
                    cur_row[3][2 + rx]
                } else {
                    255
                };
                let pi = (row_px_start + x0 + rx) * 4;
                if let Some(dst) = rgba.get_mut(pi..pi + 4) {
                    dst[0] = r.clamp(0, 255) as u8;
                    dst[1] = g.clamp(0, 255) as u8;
                    dst[2] = b.clamp(0, 255) as u8;
                    dst[3] = a.clamp(0, 255) as u8;
                }
            }
            std::mem::swap(&mut prev_row, &mut cur_row);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::mkv::{MkvVideoCodec, MkvWriter};
    use crate::model::FrameRate;

    #[test]
    fn ffv1_pixel_exact_ffmpeg_oracle_rgb_rgba_odd_and_hd_multislice() {
        for (tag, w, h, has_alpha) in [
            ("1x1", 1u32, 1u32, false),
            ("odd_63x47", 63u32, 47u32, false),
            ("rgba_32x24", 32u32, 24u32, true),
            ("hd_400x300", 400u32, 300u32, false), // > 352x288 => 2x2 slices (§5)
        ] {
            let mut rgba = vec![0u8; (w * h * 4) as usize];
            for y in 0..h {
                for x in 0..w {
                    let i = ((y * w + x) * 4) as usize;
                    rgba[i] = ((x * 13 + y * 5) % 256) as u8;
                    rgba[i + 1] = ((x * 3 + y * 17) % 256) as u8;
                    rgba[i + 2] = ((x * 29 + y * 7 + 11) % 256) as u8;
                    rgba[i + 3] = if has_alpha {
                        ((x * 19 + y * 23) % 256) as u8
                    } else {
                        255
                    };
                }
            }

            let enc = Ffv1Encoder::new(w, h, has_alpha).unwrap();
            let pkt = enc.encode_frame(&rgba).unwrap();
            let mkv_path = std::env::temp_dir().join(format!("neoveg_ffv1_{tag}.mkv"));
            let mut mkv = MkvWriter::create(
                &mkv_path,
                w,
                h,
                FrameRate::FPS_25,
                MkvVideoCodec::Ffv1 {
                    codec_private: enc.configuration_record().to_vec(),
                    has_alpha,
                },
                None,
            )
            .unwrap();
            mkv.write_video_packet(0, true, pkt.clone()).unwrap();
            mkv.finish().unwrap();

            if let Ok(out) = std::process::Command::new("ffmpeg")
                .args(["-nostdin", "-hide_banner", "-v", "error", "-i"])
                .arg(&mkv_path)
                .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
                .output()
            {
                assert!(
                    out.status.success(),
                    "ffmpeg falhou ao decodificar FFV1 {tag}: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
                assert_eq!(
                    out.stdout,
                    rgba,
                    "FFV1 {tag} não foi 100% lossless (pkt_len={})",
                    pkt.len()
                );
                assert!(
                    out.stderr.is_empty(),
                    "ffmpeg erro FFV1 {tag} (pkt_len={}): {}",
                    pkt.len(),
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            let _ = std::fs::remove_file(&mkv_path);
        }
    }

    #[test]
    fn ffv1_decoder_roundtrip_pixel_exact_e_entradas_hostis() {
        for (tag, w, h, has_alpha) in [
            ("1x1", 1u32, 1u32, false),
            ("odd_63x47", 63u32, 47u32, false),
            ("rgba_32x24", 32u32, 24u32, true),
            ("hd_400x300", 400u32, 300u32, false),
        ] {
            let mut rgba = vec![0u8; (w * h * 4) as usize];
            for y in 0..h {
                for x in 0..w {
                    let i = ((y * w + x) * 4) as usize;
                    rgba[i] = ((x * 13 + y * 5) % 256) as u8;
                    rgba[i + 1] = ((x * 3 + y * 17) % 256) as u8;
                    rgba[i + 2] = ((x * 29 + y * 7 + 11) % 256) as u8;
                    rgba[i + 3] = if has_alpha {
                        ((x * 19 + y * 23) % 256) as u8
                    } else {
                        255
                    };
                }
            }
            let enc = Ffv1Encoder::new(w, h, has_alpha).unwrap();
            let pkt = enc.encode_frame(&rgba).unwrap();
            let dec = Ffv1Decoder::from_config_record(w, h, enc.configuration_record()).unwrap();
            let out = dec.decode_frame(&pkt).unwrap();
            assert_eq!(out, rgba, "FFV1 decoder roundtrip divergiu em {tag}");

            // Mutação hostil: CRC corrompido deve falhar sem pânico.
            let mut bad_cfg = enc.configuration_record().to_vec();
            bad_cfg[0] ^= 0xFF;
            assert!(Ffv1Decoder::from_config_record(w, h, &bad_cfg).is_err());

            let mut bad_pkt = pkt.clone();
            bad_pkt[0] ^= 0x55;
            assert!(dec.decode_frame(&bad_pkt).is_err());
        }
        // Entradas vazias e truncadas nunca entram em pânico (§0.4).
        assert!(Ffv1Decoder::from_config_record(0, 10, &[0; 16]).is_err());
        assert!(Ffv1Decoder::from_config_record(16, 16, &[]).is_err());
        let enc = Ffv1Encoder::new(8, 8, false).unwrap();
        let dec = Ffv1Decoder::from_config_record(8, 8, enc.configuration_record()).unwrap();
        for len in 0..12 {
            let mut buf = vec![0xAAu8; len];
            let res = std::panic::catch_unwind(|| dec.decode_frame(&buf));
            assert!(res.is_ok());
            if let Some(last) = buf.last_mut() {
                *last = 0;
            }
            let res2 = std::panic::catch_unwind(|| dec.decode_frame(&buf));
            assert!(res2.is_ok());
        }
    }
}
