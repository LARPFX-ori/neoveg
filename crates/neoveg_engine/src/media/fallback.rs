//! Decodificador fallback em Rust puro (`#[forbid(unsafe_code)]`) sobre o
//! demuxer [`crate::media::demux`] para contêineres MP4, MOV, MKV e WEBM.
//!
//! Funciona em todas as plataformas (Linux, macOS e Windows) sem depender de
//! APIs proprietárias ou bibliotecas LGPL/GPL (`AGENTS.md` §2.3), cobrindo:
//! - Vídeo: FFV1 v3 (`V_FFV1`), Photo-JPEG / MJPEG (`jpeg`, `mjpa`, `V_MJPEG`),
//!   PNG (`png `, `V_PNG`), RGB/RGBA/BGRA/BGR24 sem compressão, YUV 4:2:2
//!   (`2vuy`/`UYVY`, `yuvs`/`YUY2`), planar `I420`/`IYUV` e `NV12`;
//! - Áudio: PCM inteiro e ponto flutuante (`sowt`, `twos`, `lpcm`, `in24`,
//!   `in32`, `fl32`, `fl64`, `A_PCM/*`), FLAC (`fLaC`, `A_FLAC`), MP3
//!   (`.mp3`, `mp3 `, `A_MPEG/L3`) e Vorbis (`A_VORBIS`).
//!
//! Também expõe utilitários de bitstream (`avcC`/`hvcC`, Annex-B) e conversão
//! de pixels (`NV12`/`BGRA` → `RGBA`) compartilhados com as bridges FFI
//! nativas (`vaapi`, `videotoolbox`), permitindo validação contínua em todas
//! as plataformas.

use crate::media::audio::{AudioData, MAX_SAMPLES};
use crate::media::demux::{self, DemuxSample, DemuxTrack, TrackKind};
use crate::media::ffv1::Ffv1Decoder;
use crate::media::system::{SystemLoaded, SystemVideoReader};
use crate::model::{FrameRate, Ticks};
use anyhow::{anyhow, bail, Context, Result};
use image::RgbaImage;
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

/// Limite máximo de tamanho para um único pacote comprimido lido em RAM (§0.4).
pub(crate) const MAX_FALLBACK_PACKET: usize = 64 * 1024 * 1024;
/// Limite máximo de dimensão de quadro decodificado (§0.4).
pub(crate) const MAX_FALLBACK_DIM: u32 = 16_384;
/// Limite máximo de fluxo contínuo montado em RAM para áudio comprimido (FLAC/MP3).
const MAX_AUDIO_STREAM_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, Debug)]
enum FallbackVideoCodec {
    Ffv1(Ffv1Decoder),
    Mjpeg,
    Png,
    RawBgra,
    RawRgba,
    RawRgb24,
    RawBgr24,
    Uyvy,
    Yuy2,
    I420,
    Nv12,
}

impl FallbackVideoCodec {
    fn from_track(track: &DemuxTrack) -> Option<Self> {
        let c = track.codec.as_str();
        let cu = c.to_ascii_uppercase();
        if cu == "V_FFV1" || cu == "FFV1" {
            let dec =
                Ffv1Decoder::from_config_record(track.width, track.height, &track.extra).ok()?;
            return Some(Self::Ffv1(dec));
        }
        if matches!(
            cu.as_str(),
            "JPEG" | "MJPA" | "MJPB" | "MJPG" | "V_MJPEG" | "DMB1"
        ) {
            return Some(Self::Mjpeg);
        }
        if matches!(cu.as_str(), "PNG " | "PNG" | "MPNG" | "V_PNG") {
            return Some(Self::Png);
        }
        if matches!(cu.as_str(), "BGRA" | "BGR4" | "RAW ") && track.width > 0 && track.height > 0 {
            if cu == "RAW " {
                return Some(Self::RawRgb24);
            }
            return Some(Self::RawBgra);
        }
        if matches!(cu.as_str(), "RGBA" | "RGB4" | "ABGR") {
            return Some(Self::RawRgba);
        }
        if matches!(cu.as_str(), "RGB " | "RGB3" | "RV24") {
            return Some(Self::RawRgb24);
        }
        if matches!(cu.as_str(), "BGR " | "BGR3" | "V_UNCOMPRESSED") {
            return Some(Self::RawBgr24);
        }
        if matches!(cu.as_str(), "2VUY" | "UYVY" | "HDYC") {
            return Some(Self::Uyvy);
        }
        if matches!(cu.as_str(), "YUVS" | "YUY2" | "YUYV") {
            return Some(Self::Yuy2);
        }
        if matches!(cu.as_str(), "I420" | "IYUV" | "YV12") {
            return Some(Self::I420);
        }
        if cu == "NV12" {
            return Some(Self::Nv12);
        }
        None
    }
}

/// Leitor de vídeo em streaming puro Rust implementando [`SystemVideoReader`].
pub struct DemuxFallbackVideoReader {
    path: PathBuf,
    file: Mutex<File>,
    width: u32,
    height: u32,
    fps: FrameRate,
    codec: FallbackVideoCodec,
    samples: Vec<DemuxSample>,
}

impl DemuxFallbackVideoReader {
    fn open(path: &Path, track: &DemuxTrack, samples: Vec<DemuxSample>) -> Result<Self> {
        if track.width == 0
            || track.height == 0
            || track.width > MAX_FALLBACK_DIM
            || track.height > MAX_FALLBACK_DIM
        {
            bail!(
                "dimensões de vídeo inválidas no contêiner: {}x{}",
                track.width,
                track.height
            );
        }
        if samples.is_empty() {
            bail!("faixa de vídeo sem amostras indexadas");
        }
        let codec = FallbackVideoCodec::from_track(track).ok_or_else(|| {
            anyhow!(
                "codec de vídeo '{}' não suportado pelo decodificador fallback em Rust puro",
                track.codec
            )
        })?;
        let file = File::open(path).with_context(|| format!("abrindo {}", path.display()))?;
        Ok(Self {
            path: path.to_path_buf(),
            file: Mutex::new(file),
            width: track.width,
            height: track.height,
            fps: track.fps.unwrap_or(FrameRate::FPS_30),
            codec,
            samples,
        })
    }

    fn sample_index_at(&self, t: Ticks) -> usize {
        if self.samples.is_empty() {
            return 0;
        }
        let idx = self.samples.partition_point(|s| s.pts <= t);
        idx.saturating_sub(1).min(self.samples.len() - 1)
    }

    fn read_packet(&self, sample: &DemuxSample) -> Result<Vec<u8>> {
        let size = sample.size as usize;
        if size == 0 || size > MAX_FALLBACK_PACKET {
            bail!("pacote de vídeo com tamanho fora do limite ({size} bytes)");
        }
        let mut f = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        f.seek(SeekFrom::Start(sample.offset))
            .with_context(|| format!("seek em {} ({})", self.path.display(), sample.offset))?;
        let mut buf = vec![0u8; size];
        f.read_exact(&mut buf)
            .with_context(|| format!("leitura de pacote ({size} bytes)"))?;
        Ok(buf)
    }
}

impl SystemVideoReader for DemuxFallbackVideoReader {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn fps(&self) -> FrameRate {
        self.fps
    }

    fn decode_at(&self, t: Ticks) -> Result<RgbaImage> {
        let idx = self.sample_index_at(t);
        let sample = self
            .samples
            .get(idx)
            .ok_or_else(|| anyhow!("amostra de vídeo ausente no índice {idx}"))?;
        let pkt = self.read_packet(sample)?;
        decode_video_packet(&self.codec, self.width, self.height, &pkt)
    }
}

fn decode_video_packet(
    codec: &FallbackVideoCodec,
    w: u32,
    h: u32,
    pkt: &[u8],
) -> Result<RgbaImage> {
    let expected_px = (w as usize)
        .checked_mul(h as usize)
        .ok_or_else(|| anyhow!("overflow de pixels {w}x{h}"))?;
    match codec {
        FallbackVideoCodec::Ffv1(dec) => {
            let rgba = dec.decode_frame(pkt)?;
            RgbaImage::from_raw(w, h, rgba)
                .ok_or_else(|| anyhow!("tamanho de quadro FFV1 incompatível com {w}x{h}"))
        }
        FallbackVideoCodec::Mjpeg => {
            let img = image::load_from_memory_with_format(pkt, image::ImageFormat::Jpeg)
                .map_err(|e| anyhow!("quadro MJPEG/JPEG inválido: {e}"))?;
            Ok(img.to_rgba8())
        }
        FallbackVideoCodec::Png => {
            let img = image::load_from_memory_with_format(pkt, image::ImageFormat::Png)
                .map_err(|e| anyhow!("quadro PNG inválido: {e}"))?;
            Ok(img.to_rgba8())
        }
        FallbackVideoCodec::RawBgra => {
            let need = expected_px
                .checked_mul(4)
                .ok_or_else(|| anyhow!("overflow BGRA"))?;
            if pkt.len() < need {
                bail!("quadro BGRA truncado ({} < {need})", pkt.len());
            }
            bgra_to_rgba(w, h, &pkt[..need], w as usize * 4)
        }
        FallbackVideoCodec::RawRgba => {
            let need = expected_px
                .checked_mul(4)
                .ok_or_else(|| anyhow!("overflow RGBA"))?;
            if pkt.len() < need {
                bail!("quadro RGBA truncado ({} < {need})", pkt.len());
            }
            RgbaImage::from_raw(w, h, pkt[..need].to_vec())
                .ok_or_else(|| anyhow!("quadro RGBA inválido"))
        }
        FallbackVideoCodec::RawRgb24 => {
            let need = expected_px
                .checked_mul(3)
                .ok_or_else(|| anyhow!("overflow RGB24"))?;
            if pkt.len() < need {
                bail!("quadro RGB24 truncado ({} < {need})", pkt.len());
            }
            let mut out = vec![255u8; expected_px * 4];
            for (src, dst) in pkt[..need]
                .as_chunks::<3>()
                .0
                .iter()
                .zip(out.as_chunks_mut::<4>().0.iter_mut())
            {
                dst[0] = src[0];
                dst[1] = src[1];
                dst[2] = src[2];
                dst[3] = 255;
            }
            RgbaImage::from_raw(w, h, out).ok_or_else(|| anyhow!("quadro RGB24 inválido"))
        }
        FallbackVideoCodec::RawBgr24 => {
            let need = expected_px
                .checked_mul(3)
                .ok_or_else(|| anyhow!("overflow BGR24"))?;
            if pkt.len() < need {
                bail!("quadro BGR24 truncado ({} < {need})", pkt.len());
            }
            let mut out = vec![255u8; expected_px * 4];
            for (src, dst) in pkt[..need]
                .as_chunks::<3>()
                .0
                .iter()
                .zip(out.as_chunks_mut::<4>().0.iter_mut())
            {
                dst[0] = src[2];
                dst[1] = src[1];
                dst[2] = src[0];
                dst[3] = 255;
            }
            RgbaImage::from_raw(w, h, out).ok_or_else(|| anyhow!("quadro BGR24 inválido"))
        }
        FallbackVideoCodec::Uyvy | FallbackVideoCodec::Yuy2 => {
            let uyvy = matches!(codec, FallbackVideoCodec::Uyvy);
            let stride = (w as usize)
                .checked_mul(2)
                .ok_or_else(|| anyhow!("overflow YUV422"))?;
            let need = stride
                .checked_mul(h as usize)
                .ok_or_else(|| anyhow!("overflow YUV422"))?;
            if pkt.len() < need {
                bail!("quadro YUV 4:2:2 truncado ({} < {need})", pkt.len());
            }
            let mut img = RgbaImage::new(w, h);
            for y in 0..h as usize {
                let row = &pkt[y * stride..(y + 1) * stride];
                for x in (0..(w as usize).saturating_sub(1)).step_by(2) {
                    let b = row.get(x * 2..x * 2 + 4).unwrap_or(&[0; 4]);
                    let (y0, u, y1, v) = if uyvy {
                        (b[1], b[0], b[3], b[2])
                    } else {
                        (b[0], b[1], b[2], b[3])
                    };
                    img.put_pixel(x as u32, y as u32, yuv_bt601_to_rgba(y0, u, v));
                    img.put_pixel((x + 1) as u32, y as u32, yuv_bt601_to_rgba(y1, u, v));
                }
            }
            Ok(img)
        }
        FallbackVideoCodec::I420 => {
            let y_len = expected_px;
            let cw = (w as usize).div_ceil(2);
            let ch = (h as usize).div_ceil(2);
            let uv_len = cw.checked_mul(ch).ok_or_else(|| anyhow!("overflow I420"))?;
            let need = y_len
                .checked_add(uv_len.saturating_mul(2))
                .ok_or_else(|| anyhow!("overflow I420"))?;
            if pkt.len() < need {
                bail!("quadro I420 truncado ({} < {need})", pkt.len());
            }
            let y_plane = &pkt[..y_len];
            let u_plane = &pkt[y_len..y_len + uv_len];
            let v_plane = &pkt[y_len + uv_len..need];
            let mut img = RgbaImage::new(w, h);
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let yv = y_plane[y * (w as usize) + x];
                    let ui = (y / 2) * cw + (x / 2);
                    let uv = u_plane[ui];
                    let vv = v_plane[ui];
                    img.put_pixel(x as u32, y as u32, yuv_bt601_to_rgba(yv, uv, vv));
                }
            }
            Ok(img)
        }
        FallbackVideoCodec::Nv12 => {
            let y_len = expected_px;
            let cw = (w as usize).div_ceil(2) * 2;
            let ch = (h as usize).div_ceil(2);
            let uv_len = cw.checked_mul(ch).ok_or_else(|| anyhow!("overflow NV12"))?;
            let need = y_len
                .checked_add(uv_len)
                .ok_or_else(|| anyhow!("overflow NV12"))?;
            if pkt.len() < need {
                bail!("quadro NV12 truncado ({} < {need})", pkt.len());
            }
            nv12_to_rgba(w, h, &pkt[..y_len], w as usize, &pkt[y_len..need], cw)
        }
    }
}

#[inline]
pub(crate) fn yuv_bt601_to_rgba(y: u8, u: u8, v: u8) -> image::Rgba<u8> {
    let c = i32::from(y) - 16;
    let d = i32::from(u) - 128;
    let e = i32::from(v) - 128;
    let r = ((298 * c + 409 * e + 128) >> 8).clamp(0, 255) as u8;
    let g = ((298 * c - 100 * d - 208 * e + 128) >> 8).clamp(0, 255) as u8;
    let b = ((298 * c + 516 * d + 128) >> 8).clamp(0, 255) as u8;
    image::Rgba([r, g, b, 255])
}

/// Converte plano `NV12` (Y + UV intercalado, usado por VA-API e VideoToolbox) para `RgbaImage`.
pub(crate) fn nv12_to_rgba(
    w: u32,
    h: u32,
    y_plane: &[u8],
    y_stride: usize,
    uv_plane: &[u8],
    uv_stride: usize,
) -> Result<RgbaImage> {
    if w == 0 || h == 0 || w > MAX_FALLBACK_DIM || h > MAX_FALLBACK_DIM {
        bail!("dimensões NV12 inválidas: {w}x{h}");
    }
    let (wu, hu) = (w as usize, h as usize);
    if y_stride < wu || uv_stride < wu.div_ceil(2) * 2 {
        bail!("stride NV12 menor que a largura");
    }
    let y_need = y_stride
        .checked_mul(hu)
        .ok_or_else(|| anyhow!("overflow Y NV12"))?;
    let uv_need = uv_stride
        .checked_mul(hu.div_ceil(2))
        .ok_or_else(|| anyhow!("overflow UV NV12"))?;
    if y_plane.len() < y_need || uv_plane.len() < uv_need {
        bail!("buffer NV12 truncado");
    }
    let mut out = vec![255u8; wu * hu * 4];
    for y in 0..hu {
        let y_row = &y_plane[y * y_stride..y * y_stride + wu];
        let uv_row = &uv_plane[(y / 2) * uv_stride..(y / 2) * uv_stride + wu.div_ceil(2) * 2];
        let dst_row = &mut out[y * wu * 4..(y + 1) * wu * 4];
        for (x, &yv) in y_row.iter().enumerate().take(wu) {
            let uv_idx = (x / 2) * 2;
            let px = yuv_bt601_to_rgba(yv, uv_row[uv_idx], uv_row[uv_idx + 1]);
            let o = x * 4;
            dst_row[o..o + 4].copy_from_slice(&px.0);
        }
    }
    RgbaImage::from_raw(w, h, out).ok_or_else(|| anyhow!("falha ao montar imagem NV12"))
}

/// Converte buffer `BGRA` com passo arbitrário (usado por VideoToolbox e QuickTime) para `RgbaImage`.
pub(crate) fn bgra_to_rgba(w: u32, h: u32, bgra: &[u8], stride: usize) -> Result<RgbaImage> {
    if w == 0 || h == 0 || w > MAX_FALLBACK_DIM || h > MAX_FALLBACK_DIM {
        bail!("dimensões BGRA inválidas: {w}x{h}");
    }
    let (wu, hu) = (w as usize, h as usize);
    let row_bytes = wu.checked_mul(4).ok_or_else(|| anyhow!("overflow BGRA"))?;
    if stride < row_bytes {
        bail!("stride BGRA menor que a largura");
    }
    let need = stride
        .checked_mul(hu)
        .ok_or_else(|| anyhow!("overflow BGRA"))?;
    if bgra.len() < need {
        bail!("buffer BGRA truncado ({} < {need})", bgra.len());
    }
    let mut out = vec![0u8; row_bytes * hu];
    for y in 0..hu {
        let src = &bgra[y * stride..y * stride + row_bytes];
        let dst = &mut out[y * row_bytes..(y + 1) * row_bytes];
        for (s, d) in src
            .as_chunks::<4>()
            .0
            .iter()
            .zip(dst.as_chunks_mut::<4>().0.iter_mut())
        {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
    }
    RgbaImage::from_raw(w, h, out).ok_or_else(|| anyhow!("falha ao montar imagem BGRA"))
}

/// Extrai conjuntos de parâmetros SPS/PPS e `nal_length_size` de um registro `avcC`
/// (ISO/IEC 14496-15 §5.2.4.1), com limites estritos contra entradas hostis (§0.4).
#[allow(dead_code)]
pub(crate) fn extract_avcc_parameter_sets(avcc: &[u8]) -> Result<(usize, Vec<Vec<u8>>)> {
    if avcc.len() < 7 || avcc[0] != 1 {
        bail!("registro avcC inválido ou truncado");
    }
    let nal_len_size = usize::from((avcc[4] & 0x03) + 1);
    let num_sps = usize::from(avcc[5] & 0x1F);
    let mut pos = 6usize;
    let mut sets = Vec::with_capacity(num_sps + 2);
    for _ in 0..num_sps {
        let len_b = avcc
            .get(pos..pos + 2)
            .ok_or_else(|| anyhow!("SPS truncado em avcC"))?;
        let len = usize::from(u16::from_be_bytes([len_b[0], len_b[1]]));
        pos += 2;
        let data = avcc
            .get(pos..pos + len)
            .ok_or_else(|| anyhow!("payload SPS truncado"))?;
        if !data.is_empty() {
            sets.push(data.to_vec());
        }
        pos += len;
    }
    let num_pps = usize::from(
        *avcc
            .get(pos)
            .ok_or_else(|| anyhow!("contagem PPS ausente"))?,
    );
    pos += 1;
    for _ in 0..num_pps {
        let len_b = avcc
            .get(pos..pos + 2)
            .ok_or_else(|| anyhow!("PPS truncado em avcC"))?;
        let len = usize::from(u16::from_be_bytes([len_b[0], len_b[1]]));
        pos += 2;
        let data = avcc
            .get(pos..pos + len)
            .ok_or_else(|| anyhow!("payload PPS truncado"))?;
        if !data.is_empty() {
            sets.push(data.to_vec());
        }
        pos += len;
    }
    if sets.is_empty() {
        bail!("registro avcC sem conjuntos de parâmetros");
    }
    Ok((nal_len_size, sets))
}

/// Extrai conjuntos de parâmetros VPS/SPS/PPS e `nal_length_size` de um registro `hvcC`
/// (ISO/IEC 14496-15 §8.3.3.1), com limites estritos contra entradas hostis (§0.4).
#[allow(dead_code)]
pub(crate) fn extract_hvcc_parameter_sets(hvcc: &[u8]) -> Result<(usize, Vec<Vec<u8>>)> {
    if hvcc.len() < 23 {
        bail!("registro hvcC inválido ou truncado");
    }
    let nal_len_size = usize::from((hvcc[21] & 0x03) + 1);
    let num_arrays = usize::from(hvcc[22]);
    let mut pos = 23usize;
    let mut sets = Vec::new();
    for _ in 0..num_arrays {
        let hdr = hvcc
            .get(pos..pos + 3)
            .ok_or_else(|| anyhow!("array hvcC truncado"))?;
        let num_nalus = usize::from(u16::from_be_bytes([hdr[1], hdr[2]]));
        pos += 3;
        if num_nalus > 256 {
            bail!("excesso de NALUs em hvcC ({num_nalus})");
        }
        for _ in 0..num_nalus {
            let len_b = hvcc
                .get(pos..pos + 2)
                .ok_or_else(|| anyhow!("NALU hvcC truncado"))?;
            let len = usize::from(u16::from_be_bytes([len_b[0], len_b[1]]));
            pos += 2;
            let data = hvcc
                .get(pos..pos + len)
                .ok_or_else(|| anyhow!("payload hvcC truncado"))?;
            if !data.is_empty() {
                sets.push(data.to_vec());
            }
            pos += len;
        }
    }
    if sets.is_empty() {
        bail!("registro hvcC sem conjuntos de parâmetros");
    }
    Ok((nal_len_size, sets))
}

/// Converte um pacote AVCC/HVCC (prefixado por comprimento) para bitstream Annex-B (`00 00 00 01`).
#[allow(dead_code)]
pub(crate) fn length_prefixed_to_annexb(pkt: &[u8], nal_len_size: usize) -> Result<Vec<u8>> {
    if !(1..=4).contains(&nal_len_size) {
        bail!("nal_len_size inválido: {nal_len_size}");
    }
    let mut pos = 0usize;
    let mut out = Vec::with_capacity(pkt.len().min(MAX_FALLBACK_PACKET));
    while pos < pkt.len() {
        let len_slice = pkt
            .get(pos..pos + nal_len_size)
            .ok_or_else(|| anyhow!("comprimento NALU truncado"))?;
        pos += nal_len_size;
        let mut nal_len = 0usize;
        for &b in len_slice {
            nal_len = (nal_len << 8) | usize::from(b);
        }
        if nal_len == 0 {
            continue;
        }
        let nal = pkt
            .get(pos..pos + nal_len)
            .ok_or_else(|| anyhow!("payload NALU truncado ({nal_len} bytes)"))?;
        pos += nal_len;
        if out.len().saturating_add(4 + nal.len()) > MAX_FALLBACK_PACKET {
            bail!("bitstream Annex-B excede limite de memória");
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(nal);
    }
    Ok(out)
}

fn decode_fallback_audio(
    path: &Path,
    track: &DemuxTrack,
    samples: &[DemuxSample],
) -> Result<(AudioData, bool)> {
    if samples.is_empty() {
        bail!("faixa de áudio sem amostras");
    }
    let sr = track.sample_rate.max(8_000);
    let ch = usize::from(track.channels).clamp(1, 8);
    let cu = track.codec.to_ascii_uppercase();
    let mut f = File::open(path).with_context(|| format!("abrindo {}", path.display()))?;

    // 1. PCM linear inteiro ou ponto flutuante (QuickTime / Matroska)
    if matches!(
        cu.as_str(),
        "SOWT"
            | "TWOS"
            | "LPCM"
            | "RAW "
            | "IN24"
            | "IN32"
            | "FL32"
            | "FL64"
            | "A_PCM/INT/LIT"
            | "A_PCM/INT/BIG"
            | "A_PCM/FLOAT/IEEE"
    ) {
        let mut out = Vec::new();
        let mut truncated = false;
        let mut buf = Vec::new();
        for s in samples {
            let sz = s.size as usize;
            if sz == 0 || sz > MAX_FALLBACK_PACKET {
                continue;
            }
            buf.resize(sz, 0);
            f.seek(SeekFrom::Start(s.offset))?;
            if f.read_exact(&mut buf).is_err() {
                break;
            }
            match cu.as_str() {
                "SOWT" | "LPCM" | "A_PCM/INT/LIT" => {
                    for b in buf.as_chunks::<2>().0 {
                        out.push(f32::from(i16::from_le_bytes(*b)) / 32768.0);
                        if out.len() >= MAX_SAMPLES {
                            truncated = true;
                            break;
                        }
                    }
                }
                "TWOS" | "A_PCM/INT/BIG" => {
                    for b in buf.as_chunks::<2>().0 {
                        out.push(f32::from(i16::from_be_bytes(*b)) / 32768.0);
                        if out.len() >= MAX_SAMPLES {
                            truncated = true;
                            break;
                        }
                    }
                }
                "RAW " => {
                    for &b in &buf {
                        out.push((f32::from(b) - 128.0) / 128.0);
                        if out.len() >= MAX_SAMPLES {
                            truncated = true;
                            break;
                        }
                    }
                }
                "IN24" => {
                    for b in buf.as_chunks::<3>().0 {
                        let v = ((i32::from(b[0]) << 8)
                            | (i32::from(b[1]) << 16)
                            | (i32::from(b[2]) << 24))
                            >> 8;
                        out.push(v as f32 / 8_388_608.0);
                        if out.len() >= MAX_SAMPLES {
                            truncated = true;
                            break;
                        }
                    }
                }
                "IN32" => {
                    for b in buf.as_chunks::<4>().0 {
                        let v = i32::from_le_bytes(*b);
                        out.push(v as f32 / 2_147_483_648.0);
                        if out.len() >= MAX_SAMPLES {
                            truncated = true;
                            break;
                        }
                    }
                }
                "FL32" | "A_PCM/FLOAT/IEEE" => {
                    for b in buf.as_chunks::<4>().0 {
                        let v = f32::from_le_bytes(*b);
                        out.push(if v.is_finite() {
                            v.clamp(-4.0, 4.0)
                        } else {
                            0.0
                        });
                        if out.len() >= MAX_SAMPLES {
                            truncated = true;
                            break;
                        }
                    }
                }
                "FL64" => {
                    for b in buf.as_chunks::<8>().0 {
                        let v = f64::from_le_bytes(*b) as f32;
                        out.push(if v.is_finite() {
                            v.clamp(-4.0, 4.0)
                        } else {
                            0.0
                        });
                        if out.len() >= MAX_SAMPLES {
                            truncated = true;
                            break;
                        }
                    }
                }
                _ => {}
            }
            if truncated {
                break;
            }
        }
        if out.is_empty() {
            bail!("nenhuma amostra PCM decodificada");
        }
        return Ok((AudioData::from_interleaved(sr, ch, &out), truncated));
    }

    // 2. FLAC em MKV/MP4 (`A_FLAC` / `fLaC`)
    if cu == "A_FLAC" || cu == "FLAC" {
        let mut stream = Vec::new();
        if !track.extra.is_empty() {
            stream.extend_from_slice(&track.extra);
        }
        for s in samples {
            let sz = s.size as usize;
            if sz == 0 || stream.len().saturating_add(sz) > MAX_AUDIO_STREAM_BYTES {
                break;
            }
            let start = stream.len();
            stream.resize(start + sz, 0);
            f.seek(SeekFrom::Start(s.offset))?;
            if f.read_exact(&mut stream[start..]).is_err() {
                stream.truncate(start);
                break;
            }
        }
        let mut reader = claxon::FlacReader::new(Cursor::new(stream))
            .map_err(|e| anyhow!("FLAC embutido inválido: {e}"))?;
        let info = reader.streaminfo();
        if info.bits_per_sample == 0 || info.bits_per_sample > 32 || info.channels == 0 {
            bail!("FLAC embutido com formato inválido");
        }
        let scale = 1.0 / (1u64 << (info.bits_per_sample - 1)) as f32;
        let mut out = Vec::new();
        let mut truncated = false;
        for sample in reader.samples() {
            let Ok(v) = sample else { break };
            out.push(v as f32 * scale);
            if out.len() >= MAX_SAMPLES {
                truncated = true;
                break;
            }
        }
        if out.is_empty() {
            bail!("FLAC embutido sem amostras");
        }
        return Ok((
            AudioData::from_interleaved(info.sample_rate, info.channels as usize, &out),
            truncated,
        ));
    }

    // 3. MP3 em MKV/MP4 (`A_MPEG/L3` / `.mp3` / `mp3 `)
    if matches!(cu.as_str(), "A_MPEG/L3" | ".MP3" | "MP3 " | "MP3") {
        let mut stream = Vec::new();
        for s in samples {
            let sz = s.size as usize;
            if sz == 0 || stream.len().saturating_add(sz) > MAX_AUDIO_STREAM_BYTES {
                break;
            }
            let start = stream.len();
            stream.resize(start + sz, 0);
            f.seek(SeekFrom::Start(s.offset))?;
            if f.read_exact(&mut stream[start..]).is_err() {
                stream.truncate(start);
                break;
            }
        }
        let (hdr, iter) = puremp3::read_mp3(Cursor::new(stream))
            .map_err(|e| anyhow!("MP3 embutido inválido: {e:?}"))?;
        let mut out = Vec::new();
        let mut truncated = false;
        for (l, r) in iter {
            out.push(if l.is_finite() {
                l.clamp(-4.0, 4.0)
            } else {
                0.0
            });
            out.push(if r.is_finite() {
                r.clamp(-4.0, 4.0)
            } else {
                0.0
            });
            if out.len() >= MAX_SAMPLES {
                truncated = true;
                break;
            }
        }
        if out.is_empty() {
            bail!("MP3 embutido sem amostras");
        }
        return Ok((AudioData::from_stereo(hdr.sample_rate.hz(), out), truncated));
    }

    // 4. Vorbis em MKV/WebM (`A_VORBIS`) usando cabeçalhos Xiph-laced em `CodecPrivate`
    if cu == "A_VORBIS" {
        let (ident_b, comment_b, setup_b) = parse_xiph_vorbis_private(&track.extra)?;
        let ident = lewton::header::read_header_ident(ident_b)
            .map_err(|e| anyhow!("cabeçalho ident Vorbis inválido: {e:?}"))?;
        let _comment = lewton::header::read_header_comment(comment_b)
            .map_err(|e| anyhow!("cabeçalho comment Vorbis inválido: {e:?}"))?;
        let setup = lewton::header::read_header_setup(
            setup_b,
            ident.audio_channels,
            (ident.blocksize_0, ident.blocksize_1),
        )
        .map_err(|e| anyhow!("cabeçalho setup Vorbis inválido: {e:?}"))?;
        let mut pwr = lewton::audio::PreviousWindowRight::new();
        let mut out = Vec::new();
        let mut truncated = false;
        let mut buf = Vec::new();
        for s in samples {
            let sz = s.size as usize;
            if sz == 0 || sz > MAX_FALLBACK_PACKET {
                continue;
            }
            buf.resize(sz, 0);
            f.seek(SeekFrom::Start(s.offset))?;
            if f.read_exact(&mut buf).is_err() {
                break;
            }
            if let Ok(pkt) = lewton::audio::read_audio_packet_generic::<
                lewton::samples::InterleavedSamples<i16>,
            >(&ident, &setup, &buf, &mut pwr)
            {
                for v in pkt.samples {
                    out.push(f32::from(v) / 32768.0);
                    if out.len() >= MAX_SAMPLES {
                        truncated = true;
                        break;
                    }
                }
            }
            if truncated {
                break;
            }
        }
        if out.is_empty() {
            bail!("Vorbis embutido sem amostras");
        }
        return Ok((
            AudioData::from_interleaved(
                ident.audio_sample_rate,
                usize::from(ident.audio_channels).max(1),
                &out,
            ),
            truncated,
        ));
    }

    bail!(
        "codec de áudio '{}' não suportado pelo fallback em Rust puro",
        track.codec
    )
}

fn parse_xiph_vorbis_private(extra: &[u8]) -> Result<(&[u8], &[u8], &[u8])> {
    let Some(&count) = extra.first() else {
        bail!("CodecPrivate Vorbis vazio");
    };
    if count != 2 {
        bail!("CodecPrivate Vorbis com contagem inválida ({count})");
    }
    let mut pos = 1usize;
    let mut lens = [0usize; 2];
    for len in &mut lens {
        let mut total = 0usize;
        loop {
            let &b = extra
                .get(pos)
                .ok_or_else(|| anyhow!("lacing Xiph truncado"))?;
            pos += 1;
            total = total.saturating_add(usize::from(b));
            if b < 255 {
                break;
            }
            if total > extra.len() {
                bail!("tamanho Xiph excede CodecPrivate");
            }
        }
        *len = total;
    }
    let p0 = extra
        .get(pos..pos.saturating_add(lens[0]))
        .ok_or_else(|| anyhow!("pacote ident Vorbis truncado"))?;
    let pos1 = pos.saturating_add(lens[0]);
    let p1 = extra
        .get(pos1..pos1.saturating_add(lens[1]))
        .ok_or_else(|| anyhow!("pacote comment Vorbis truncado"))?;
    let pos2 = pos1.saturating_add(lens[1]);
    let p2 = extra
        .get(pos2..)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("pacote setup Vorbis truncado"))?;
    Ok((p0, p1, p2))
}

/// Abre um contêiner MP4/MOV/MKV/WEBM usando exclusivamente o demuxer e
/// decodificadores fallback em Rust puro.
pub fn open_media(path: &Path) -> Result<SystemLoaded> {
    let idx = demux::demux_file(path)?;
    let mut video: Option<Arc<dyn SystemVideoReader>> = None;
    let mut video_err: Option<String> = None;
    for (ti, tr) in idx.tracks.iter().enumerate() {
        if tr.kind == TrackKind::Video {
            let samples: Vec<DemuxSample> = idx.samples_for_track(ti).copied().collect();
            match DemuxFallbackVideoReader::open(path, tr, samples) {
                Ok(r) => {
                    video = Some(Arc::new(r));
                    break;
                }
                Err(e) => video_err = Some(e.to_string()),
            }
        }
    }

    let mut audio: Option<AudioData> = None;
    let mut audio_truncated = false;
    let mut audio_err: Option<String> = None;
    for (ti, tr) in idx.tracks.iter().enumerate() {
        if tr.kind == TrackKind::Audio {
            let samples: Vec<DemuxSample> = idx.samples_for_track(ti).copied().collect();
            match decode_fallback_audio(path, tr, &samples) {
                Ok((a, trunc)) => {
                    audio = Some(a);
                    audio_truncated = trunc;
                    break;
                }
                Err(e) => audio_err = Some(e.to_string()),
            }
        }
    }

    if video.is_none() && audio.is_none() {
        let detail = video_err
            .or(audio_err)
            .unwrap_or_else(|| "nenhuma trilha suportada encontrada".into());
        bail!(
            "fallback Rust puro não decodificou {}: {detail}",
            path.display()
        );
    }

    let duration = idx
        .duration
        .or_else(|| audio.as_ref().map(AudioData::duration));
    Ok(SystemLoaded {
        backend: "Rust Fallback",
        duration,
        video,
        audio,
        audio_truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::ffv1::Ffv1Encoder;
    use crate::media::mkv::{MkvVideoCodec, MkvWriter};
    use crate::media::mov::{MovVideoCodec, MovWriter};

    #[test]
    fn fallback_decodifica_mov_bgra_png_2vuy_e_audio_pcm() {
        let dir = std::env::temp_dir();
        for (tag, codec) in [
            ("bgra", MovVideoCodec::UncompressedBgra),
            ("2vuy", MovVideoCodec::UncompressedYuv422),
            ("png", MovVideoCodec::Png),
        ] {
            let path = dir.join(format!("neoveg_fb_mov_{tag}_{}.mov", std::process::id()));
            let mut w = MovWriter::create(&path, 16, 12, FrameRate::FPS_25, Some(48_000), codec)
                .expect("criar mov");
            let mut frame = vec![0u8; 16 * 12 * 4];
            for px in frame.as_chunks_mut::<4>().0 {
                px.copy_from_slice(&[200, 100, 40, 255]);
            }
            let encoded = match codec {
                MovVideoCodec::UncompressedBgra => MovWriter::rgba_to_bgra_topdown(&frame, 16, 12),
                MovVideoCodec::UncompressedYuv422 => MovWriter::rgba_to_2vuy(&frame, 16, 12),
                MovVideoCodec::Png => {
                    let mut buf = Vec::new();
                    let img = RgbaImage::from_raw(16, 12, frame.clone()).expect("rgba");
                    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
                        .expect("png");
                    buf
                }
                MovVideoCodec::PhotoJpeg => Vec::new(),
            };
            w.write_video_frame(&encoded).expect("frame 0");
            w.write_video_frame(&encoded).expect("frame 1");
            w.write_audio(&[1200i16; 480]).expect("audio");
            w.finish().expect("finish mov");

            let loaded = open_media(&path).expect("fallback deve abrir mov");
            assert_eq!(loaded.backend, "Rust Fallback");
            let vr = loaded.video.expect("video reader");
            assert_eq!(vr.width(), 16);
            assert_eq!(vr.height(), 12);
            let img = vr.decode_at(Ticks::ZERO).expect("decode frame 0");
            assert_eq!(img.dimensions(), (16, 12));
            let p = img.get_pixel(4, 4);
            assert!(p[0] > 170 && p[2] < 80, "cor inesperada em {tag}: {p:?}");
            let aud = loaded.audio.expect("audio pcm");
            assert_eq!(aud.sample_rate, 48_000);
            assert!(aud.frames() > 0);
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn fallback_decodifica_mkv_ffv1_lossless() {
        let path = std::env::temp_dir().join(format!("neoveg_fb_ffv1_{}.mkv", std::process::id()));
        let (w, h) = (24u32, 16u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for (i, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            px[0] = ((i * 7) % 256) as u8;
            px[1] = ((i * 13) % 256) as u8;
            px[2] = ((i * 29) % 256) as u8;
            px[3] = 255;
        }
        let enc = Ffv1Encoder::new(w, h, false).expect("enc");
        let pkt = enc.encode_frame(&rgba).expect("encode");
        let mut mkv = MkvWriter::create(
            &path,
            w,
            h,
            FrameRate::FPS_30,
            MkvVideoCodec::Ffv1 {
                codec_private: enc.configuration_record().to_vec(),
                has_alpha: false,
            },
            None,
        )
        .expect("mkv");
        mkv.write_video_packet(0, true, pkt).expect("write");
        mkv.finish().expect("finish");

        let loaded = open_media(&path).expect("fallback deve abrir mkv ffv1");
        let vr = loaded.video.expect("video");
        let img = vr.decode_at(Ticks::ZERO).expect("decode");
        assert_eq!(img.as_raw().as_slice(), rgba.as_slice());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn helpers_avcc_hvcc_nv12_e_entradas_hostis_nao_travam() {
        // avcC sintético válido (1 SPS + 1 PPS, nal_len_size = 4)
        let avcc = [
            1, 0x64, 0x00, 0x1F, 0xFF, 0xE1, 0x00, 0x04, 0x67, 0x64, 0x00, 0x1F, 0x01, 0x00, 0x03,
            0x68, 0xEE, 0x3C,
        ];
        let (nl, sets) = extract_avcc_parameter_sets(&avcc).expect("avcc");
        assert_eq!(nl, 4);
        assert_eq!(sets.len(), 2);
        let annexb = length_prefixed_to_annexb(&[0, 0, 0, 2, 0x65, 0x88], 4).expect("annexb");
        assert_eq!(annexb, vec![0, 0, 0, 1, 0x65, 0x88]);

        // NV12 sintético válido
        let y = vec![128u8; 4 * 4];
        let uv = vec![128u8; 4 * 2];
        let img = nv12_to_rgba(4, 4, &y, 4, &uv, 4).expect("nv12");
        assert_eq!(img.dimensions(), (4, 4));

        // Entradas hostis / truncadas
        for len in 0..avcc.len() {
            assert!(std::panic::catch_unwind(|| extract_avcc_parameter_sets(&avcc[..len])).is_ok());
        }
        assert!(extract_hvcc_parameter_sets(&[]).is_err());
        assert!(extract_hvcc_parameter_sets(&[0u8; 25]).is_err());
        assert!(length_prefixed_to_annexb(&[0, 0, 0, 99, 1], 4).is_err());
        assert!(nv12_to_rgba(0, 4, &[], 0, &[], 0).is_err());
        assert!(bgra_to_rgba(4, 4, &[0u8; 4], 16).is_err());
    }
}
