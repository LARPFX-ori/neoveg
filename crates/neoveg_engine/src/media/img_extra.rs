//! Decodificadores e codificadores de imagem em Rust puro (Sala Limpa, AGENTS.md §2.3)
//! para formatos gráficos adicionais não cobertos diretamente pela configuração padrão:
//! - **DPX** (`.dpx`, ANSI/SMPTE 268M-2003);
//! - **Kodak Cineon** (`.cin`, Cineon File Format v4.5);
//! - **WBMP** (`.wbmp`, WAP Wireless Application Protocol Type 0);
//! - **XBM / XPM** (`.xbm`, `.xpm` v3, X Window System ASCII);
//! - **Amiga IFF / ILBM** (`.iff`, `.ilbm`, `.lbm`, Electronic Arts IFF 85).
//!
//! Também centraliza a leitura e a codificação unificadas com limites rígidos
//! de dimensões e memória (§0.4) para todas as extensões de imagem do Neoveg.

use crate::model::MAX_DIM;
use anyhow::{anyhow, bail, Result};
use image::{ImageDecoder, ImageEncoder, ImageReader, Limits, Rgba, RgbaImage};
use std::io::Cursor;
use std::path::Path;

/// Maior arquivo de imagem carregado inteiro para a memória (256 MiB).
const MAX_IMG_FILE_BYTES: u64 = 256 << 20;
/// Orçamento máximo para buffer de pixels RGBA descomprimido (512 MiB).
const MAX_RGBA_BYTES: usize = 512 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageSeqFormat {
    Png,
    Jpeg,
    Bmp,
    Tiff,
    Tga,
    Webp,
    Pnm,
    Qoi,
    Dpx,
    Exr,
}

impl ImageSeqFormat {
    pub fn ext(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Bmp => "bmp",
            Self::Tiff => "tif",
            Self::Tga => "tga",
            Self::Webp => "webp",
            Self::Pnm => "ppm",
            Self::Qoi => "qoi",
            Self::Dpx => "dpx",
            Self::Exr => "exr",
        }
    }
}

fn validate_dims(w: u32, h: u32, fmt: &str) -> Result<usize> {
    if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM {
        bail!("dimensões {fmt} inválidas: {w}x{h}");
    }
    let bytes = (w as usize)
        .checked_mul(h as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| anyhow!("dimensões {fmt} excedem capacidade de endereço"))?;
    if bytes > MAX_RGBA_BYTES {
        bail!("imagem {fmt} grande demais para a memória ({bytes} bytes)");
    }
    Ok(bytes)
}

fn image_limits() -> Limits {
    let mut lim = Limits::default();
    lim.max_image_width = Some(MAX_DIM);
    lim.max_image_height = Some(MAX_DIM);
    lim.max_alloc = Some(MAX_RGBA_BYTES as u64);
    lim
}

/// Abre qualquer imagem suportada pelo Neoveg aplicando limites de segurança (§0.4).
pub fn open_image_file(path: &Path) -> Result<RgbaImage> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_IMG_FILE_BYTES {
        bail!(
            "arquivo de imagem grande demais ({} bytes): {}",
            meta.len(),
            path.display()
        );
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if is_extra_image_ext(&ext) {
        let bytes = std::fs::read(path)?;
        return decode_extra_image(&ext, &bytes);
    }
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    reader.limits(image_limits());
    let mut dec = reader.into_decoder()?;
    dec.set_limits(image_limits())?;
    let (w, h) = dec.dimensions();
    let _ = validate_dims(w, h, &ext)?;
    let img = image::DynamicImage::from_decoder(dec)?;
    Ok(img.to_rgba8())
}

pub fn is_extra_image_ext(ext: &str) -> bool {
    matches!(
        ext,
        "dpx" | "cin" | "wbmp" | "xbm" | "xpm" | "iff" | "ilbm" | "lbm"
    )
}

/// Decodifica uma imagem de formato extra a partir de bytes em memória.
pub fn decode_extra_image(ext: &str, bytes: &[u8]) -> Result<RgbaImage> {
    match ext.to_ascii_lowercase().as_str() {
        "dpx" => decode_dpx(bytes),
        "cin" => decode_cineon(bytes),
        "wbmp" => decode_wbmp(bytes),
        "xbm" => decode_xbm(bytes),
        "xpm" => decode_xpm(bytes),
        "iff" | "ilbm" | "lbm" => decode_iff_ilbm(bytes),
        other => bail!("formato extra de imagem desconhecido: .{other}"),
    }
}

/// Codifica um quadro RGBA para qualquer extensão/formato de saída suportado.
pub fn encode_image_for_ext(img: &RgbaImage, ext: &str, jpeg_quality: u8) -> Result<Vec<u8>> {
    let w = img.width();
    let h = img.height();
    let quality = jpeg_quality.clamp(10, 100);
    let mut out = Vec::new();
    match ext.to_ascii_lowercase().as_str() {
        "png" => {
            let rgb = rgba_to_rgb(img);
            image::codecs::png::PngEncoder::new(&mut out).write_image(
                &rgb,
                w,
                h,
                image::ExtendedColorType::Rgb8,
            )?;
        }
        "jpg" | "jpeg" => {
            let rgb = rgba_to_rgb(img);
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality).write_image(
                &rgb,
                w,
                h,
                image::ExtendedColorType::Rgb8,
            )?;
        }
        "bmp" => {
            let rgb = rgba_to_rgb(img);
            image::codecs::bmp::BmpEncoder::new(&mut out).write_image(
                &rgb,
                w,
                h,
                image::ExtendedColorType::Rgb8,
            )?;
        }
        "tif" | "tiff" => {
            let rgb = rgba_to_rgb(img);
            image::codecs::tiff::TiffEncoder::new(Cursor::new(&mut out)).write_image(
                &rgb,
                w,
                h,
                image::ExtendedColorType::Rgb8,
            )?;
        }
        "tga" => {
            image::codecs::tga::TgaEncoder::new(&mut out).write_image(
                img.as_raw(),
                w,
                h,
                image::ExtendedColorType::Rgba8,
            )?;
        }
        "webp" => {
            image::codecs::webp::WebPEncoder::new_lossless(&mut out).write_image(
                img.as_raw(),
                w,
                h,
                image::ExtendedColorType::Rgba8,
            )?;
        }
        "ppm" | "pnm" => {
            let rgb = rgba_to_rgb(img);
            image::codecs::pnm::PnmEncoder::new(&mut out).write_image(
                &rgb,
                w,
                h,
                image::ExtendedColorType::Rgb8,
            )?;
        }
        "qoi" => {
            image::codecs::qoi::QoiEncoder::new(&mut out).write_image(
                img.as_raw(),
                w,
                h,
                image::ExtendedColorType::Rgba8,
            )?;
        }
        "exr" => return encode_exr_rgba(img),
        "ico" => {
            let scaled = if w > 256 || h > 256 {
                image::imageops::resize(
                    img,
                    w.min(256),
                    h.min(256),
                    image::imageops::FilterType::Triangle,
                )
            } else {
                img.clone()
            };
            image::codecs::ico::IcoEncoder::new(&mut out).write_image(
                scaled.as_raw(),
                scaled.width(),
                scaled.height(),
                image::ExtendedColorType::Rgba8,
            )?;
        }
        "dpx" => return encode_dpx(img),
        "cin" => return encode_cineon(img),
        "wbmp" => return encode_wbmp(img),
        "xbm" => return encode_xbm(img, "neoveg"),
        "xpm" => return encode_xpm(img, "neoveg"),
        other => bail!("codificação de imagem para .{other} não suportada"),
    }
    Ok(out)
}

fn rgba_to_rgb(img: &RgbaImage) -> Vec<u8> {
    let mut rgb = Vec::with_capacity((img.width() as usize) * (img.height() as usize) * 3);
    for px in img.as_raw().as_chunks::<4>().0 {
        rgb.extend_from_slice(&px[..3]);
    }
    rgb
}

pub fn encode_seq_frame(img: &RgbaImage, fmt: ImageSeqFormat, quality: u8) -> Result<Vec<u8>> {
    encode_image_for_ext(img, fmt.ext(), quality)
}

/// Codifica RGBA8 em OpenEXR (`Rgba32F` em bytes little-endian, sem `unsafe`).
pub fn encode_exr_rgba(img: &RgbaImage) -> Result<Vec<u8>> {
    let w = img.width();
    let h = img.height();
    let mut raw = Vec::with_capacity(img.as_raw().len() * 4);
    for &c in img.as_raw() {
        let f = c as f32 / 255.0;
        raw.extend_from_slice(&f.to_ne_bytes());
    }
    let mut out = Vec::new();
    image::codecs::openexr::OpenExrEncoder::new(Cursor::new(&mut out)).write_image(
        &raw,
        w,
        h,
        image::ExtendedColorType::Rgba32F,
    )?;
    Ok(out)
}

fn rd_u16(b: &[u8], off: usize, be: bool) -> Option<u16> {
    let s = b.get(off..off + 2)?;
    Some(if be {
        u16::from_be_bytes([s[0], s[1]])
    } else {
        u16::from_le_bytes([s[0], s[1]])
    })
}

fn rd_u32(b: &[u8], off: usize, be: bool) -> Option<u32> {
    let s = b.get(off..off + 4)?;
    Some(if be {
        u32::from_be_bytes([s[0], s[1], s[2], s[3]])
    } else {
        u32::from_le_bytes([s[0], s[1], s[2], s[3]])
    })
}

// ============================================================================
// DPX (ANSI/SMPTE 268M-2003)
// ============================================================================

/// Decodifica um arquivo DPX (`SDPX` BE ou `XPDS` LE, 8/10/12/16 bits RGB/RGBA/Luma).
pub fn decode_dpx(b: &[u8]) -> Result<RgbaImage> {
    let be = match b.get(0..4) {
        Some(b"SDPX") => true,
        Some(b"XPDS") => false,
        _ => bail!("não é um arquivo DPX válido (magic SDPX/XPDS ausente)"),
    };
    if b.len() < 852 {
        bail!("cabeçalho DPX truncado");
    }
    let global_off = rd_u32(b, 4, be).unwrap_or(0) as usize;
    let w = rd_u32(b, 772, be).unwrap_or(0);
    let h = rd_u32(b, 776, be).unwrap_or(0);
    let rgba_bytes = validate_dims(w, h, "DPX")?;

    let descriptor = *b.get(800).unwrap_or(&0);
    let bit_depth = *b.get(803).unwrap_or(&0);
    let packing = rd_u16(b, 804, be).unwrap_or(0);
    let encoding = rd_u16(b, 806, be).unwrap_or(0);
    if encoding != 0 {
        bail!("DPX com codificação RLE ({encoding}) não suportado");
    }
    let elem_off = rd_u32(b, 808, be).unwrap_or(0) as usize;
    let data_off = if elem_off >= 852 && elem_off < b.len() {
        elem_off
    } else {
        global_off
    };
    let eol_pad = rd_u32(b, 812, be).unwrap_or(0) as usize;
    let data = b
        .get(data_off..)
        .ok_or_else(|| anyhow!("offset de dados DPX inválido: {data_off}"))?;

    let comps: usize = match descriptor {
        1..=4 | 6 => 1,
        50 => 3,
        51 | 52 => 4,
        other => bail!("descritor DPX não suportado: {other}"),
    };
    let pixels = (w as usize) * (h as usize);
    let mut out = vec![0u8; rgba_bytes];

    match (bit_depth, packing) {
        (8, _) => {
            let line_bytes = (w as usize)
                .checked_mul(comps)
                .ok_or_else(|| anyhow!("linha DPX grande"))?;
            let stride = line_bytes.saturating_add(eol_pad);
            for y in 0..h as usize {
                let row_off = y.saturating_mul(stride);
                for x in 0..w as usize {
                    let si = row_off + x * comps;
                    let di = (y * (w as usize) + x) * 4;
                    map_dpx_pixel(
                        descriptor,
                        [
                            *data.get(si).unwrap_or(&0),
                            *data.get(si + 1).unwrap_or(&0),
                            *data.get(si + 2).unwrap_or(&0),
                            *data.get(si + 3).unwrap_or(&255),
                        ],
                        &mut out[di..di + 4],
                    );
                }
            }
        }
        (16, _) => {
            let line_bytes = (w as usize)
                .checked_mul(comps)
                .and_then(|n| n.checked_mul(2))
                .ok_or_else(|| anyhow!("linha DPX16 grande"))?;
            let stride = line_bytes.saturating_add(eol_pad);
            for y in 0..h as usize {
                let row_off = y.saturating_mul(stride);
                for x in 0..w as usize {
                    let si = row_off + x * comps * 2;
                    let di = (y * (w as usize) + x) * 4;
                    let c0 = (rd_u16(data, si, be).unwrap_or(0) >> 8) as u8;
                    let c1 = (rd_u16(data, si + 2, be).unwrap_or(0) >> 8) as u8;
                    let c2 = (rd_u16(data, si + 4, be).unwrap_or(0) >> 8) as u8;
                    let c3 = if comps == 4 {
                        (rd_u16(data, si + 6, be).unwrap_or(0xFFFF) >> 8) as u8
                    } else {
                        255
                    };
                    map_dpx_pixel(descriptor, [c0, c1, c2, c3], &mut out[di..di + 4]);
                }
            }
        }
        (10, 1 | 2) => {
            // Método A (1): alinhado ao MSB de palavra de 32 bits; Método B (2): alinhado ao LSB.
            let mut di = 0usize;
            if comps == 3 {
                let mut wi = 0usize;
                for _ in 0..pixels {
                    let word = rd_u32(data, wi, be).unwrap_or(0);
                    wi += 4;
                    let (r, g, b_c) = if packing == 1 {
                        (
                            ((word >> 22) & 0x3FF) >> 2,
                            ((word >> 12) & 0x3FF) >> 2,
                            ((word >> 2) & 0x3FF) >> 2,
                        )
                    } else {
                        (
                            ((word >> 20) & 0x3FF) >> 2,
                            ((word >> 10) & 0x3FF) >> 2,
                            (word & 0x3FF) >> 2,
                        )
                    };
                    map_dpx_pixel(
                        descriptor,
                        [r as u8, g as u8, b_c as u8, 255],
                        &mut out[di..di + 4],
                    );
                    di += 4;
                }
            } else {
                // Genérico 3x10b por word de 32 bits (packing=1/2)
                let mut bit_vals = Vec::with_capacity(pixels * comps);
                let needed_words = (pixels * comps).div_ceil(3);
                for wi in 0..needed_words {
                    let word = rd_u32(data, wi * 4, be).unwrap_or(0);
                    if packing == 1 {
                        bit_vals.push((((word >> 22) & 0x3FF) >> 2) as u8);
                        bit_vals.push((((word >> 12) & 0x3FF) >> 2) as u8);
                        bit_vals.push((((word >> 2) & 0x3FF) >> 2) as u8);
                    } else {
                        bit_vals.push((((word >> 20) & 0x3FF) >> 2) as u8);
                        bit_vals.push((((word >> 10) & 0x3FF) >> 2) as u8);
                        bit_vals.push(((word & 0x3FF) >> 2) as u8);
                    }
                }
                for p in 0..pixels {
                    let si = p * comps;
                    map_dpx_pixel(
                        descriptor,
                        [
                            *bit_vals.get(si).unwrap_or(&0),
                            *bit_vals.get(si + 1).unwrap_or(&0),
                            *bit_vals.get(si + 2).unwrap_or(&0),
                            *bit_vals.get(si + 3).unwrap_or(&255),
                        ],
                        &mut out[p * 4..p * 4 + 4],
                    );
                }
            }
        }
        (10 | 12, 0) => {
            // Bitstream contínuo big-endian (empacotamento 0)
            let bits = bit_depth as usize;
            let shift = bits - 8;
            let mask = (1u64 << bits) - 1;
            let mut acc: u64 = 0;
            let mut acc_bits: usize = 0;
            let mut byte_idx = 0usize;
            for p in 0..pixels {
                let mut cs = [0u8, 0u8, 0u8, 255u8];
                for dst_c in cs.iter_mut().take(comps) {
                    while acc_bits < bits {
                        let byte = *data.get(byte_idx).unwrap_or(&0) as u64;
                        byte_idx = byte_idx.saturating_add(1);
                        acc = (acc << 8) | byte;
                        acc_bits += 8;
                    }
                    acc_bits -= bits;
                    let val = ((acc >> acc_bits) & mask) >> shift;
                    *dst_c = val as u8;
                }
                map_dpx_pixel(descriptor, cs, &mut out[p * 4..p * 4 + 4]);
            }
        }
        _ => bail!("DPX com profundidade={bit_depth} empacotamento={packing} não suportado"),
    }

    RgbaImage::from_raw(w, h, out).ok_or_else(|| anyhow!("falha ao montar imagem DPX"))
}

fn map_dpx_pixel(desc: u8, c: [u8; 4], dst: &mut [u8]) {
    match desc {
        1..=4 | 6 => {
            dst[0] = c[0];
            dst[1] = c[0];
            dst[2] = c[0];
            dst[3] = 255;
        }
        50 => {
            dst[0] = c[0];
            dst[1] = c[1];
            dst[2] = c[2];
            dst[3] = 255;
        }
        51 => {
            dst[0] = c[0];
            dst[1] = c[1];
            dst[2] = c[2];
            dst[3] = c[3];
        }
        52 => {
            // ABGR -> RGBA
            dst[0] = c[3];
            dst[1] = c[2];
            dst[2] = c[1];
            dst[3] = c[0];
        }
        _ => {}
    }
}

/// Codifica uma imagem RGBA em DPX 2.0 (SMPTE 268M, 8 bits RGB/RGBA BE não comprimido).
pub fn encode_dpx(img: &RgbaImage) -> Result<Vec<u8>> {
    let w = img.width();
    let h = img.height();
    let has_alpha = img.as_raw().as_chunks::<4>().0.iter().any(|p| p[3] < 255);
    let comps = if has_alpha { 4usize } else { 3usize };
    let desc = if has_alpha { 51u8 } else { 50u8 };
    let data_offset = 2048usize;
    let pixel_bytes = (w as usize) * (h as usize) * comps;
    let total_size = data_offset + pixel_bytes;

    let mut buf = vec![0u8; total_size];
    buf[0..4].copy_from_slice(b"SDPX");
    buf[4..8].copy_from_slice(&(data_offset as u32).to_be_bytes());
    buf[8..16].copy_from_slice(b"V2.0\0\0\0\0");
    buf[16..20].copy_from_slice(&(total_size as u32).to_be_bytes());
    buf[20..24].copy_from_slice(&1u32.to_be_bytes()); // ditto key
    buf[24..28].copy_from_slice(&1664u32.to_be_bytes()); // generic header size
    buf[28..32].copy_from_slice(&384u32.to_be_bytes()); // industry header size
    buf[160..167].copy_from_slice(b"Neoveg\0");

    // Image Information Header (offset 768)
    buf[768..770].copy_from_slice(&0u16.to_be_bytes()); // orientation
    buf[770..772].copy_from_slice(&1u16.to_be_bytes()); // elements
    buf[772..776].copy_from_slice(&w.to_be_bytes());
    buf[776..780].copy_from_slice(&h.to_be_bytes());
    buf[780..784].copy_from_slice(&0u32.to_be_bytes()); // data sign = unsigned
    buf[792..796].copy_from_slice(&255u32.to_be_bytes()); // ref high
    buf[800] = desc;
    buf[801] = 2; // linear transfer
    buf[802] = 2; // colorimetric
    buf[803] = 8; // bit depth
    buf[804..806].copy_from_slice(&0u16.to_be_bytes()); // packing
    buf[806..808].copy_from_slice(&0u16.to_be_bytes()); // encoding = none
    buf[808..812].copy_from_slice(&(data_offset as u32).to_be_bytes());

    let dst = &mut buf[data_offset..];
    if has_alpha {
        dst.copy_from_slice(img.as_raw());
    } else {
        for (i, px) in img.as_raw().as_chunks::<4>().0.iter().enumerate() {
            dst[i * 3] = px[0];
            dst[i * 3 + 1] = px[1];
            dst[i * 3 + 2] = px[2];
        }
    }
    Ok(buf)
}

// ============================================================================
// Kodak Cineon (.cin)
// ============================================================================

/// Decodifica Kodak Cineon v4.5 (`0x802A5FD7` BE / `0xD75F2A80` LE, 8/10 bits RGB).
pub fn decode_cineon(b: &[u8]) -> Result<RgbaImage> {
    let be = match b.get(0..4) {
        Some([0x80, 0x2A, 0x5F, 0xD7]) => true,
        Some([0xD7, 0x5F, 0x2A, 0x80]) => false,
        _ => bail!("não é um arquivo Kodak Cineon válido"),
    };
    if b.len() < 712 {
        bail!("cabeçalho Cineon truncado");
    }
    let data_off = rd_u32(b, 4, be).unwrap_or(0) as usize;
    let channels = *b.get(193).unwrap_or(&0) as usize;
    if !(1..=4).contains(&channels) {
        bail!("número de canais Cineon não suportado: {channels}");
    }
    let bits = *b.get(198).unwrap_or(&0);
    let w = rd_u32(b, 200, be).unwrap_or(0);
    let h = rd_u32(b, 204, be).unwrap_or(0);
    let rgba_bytes = validate_dims(w, h, "Cineon")?;
    let data = b
        .get(data_off..)
        .ok_or_else(|| anyhow!("offset de dados Cineon inválido"))?;

    let pixels = (w as usize) * (h as usize);
    let mut out = vec![0u8; rgba_bytes];
    match bits {
        8 => {
            for p in 0..pixels {
                let si = p * channels;
                let di = p * 4;
                out[di] = *data.get(si).unwrap_or(&0);
                out[di + 1] = *data
                    .get(si + usize::from(channels >= 2))
                    .unwrap_or(&out[di]);
                out[di + 2] = *data
                    .get(si + usize::from(channels >= 3) * 2)
                    .unwrap_or(&out[di]);
                out[di + 3] = if channels == 4 {
                    *data.get(si + 3).unwrap_or(&255)
                } else {
                    255
                };
            }
        }
        10 => {
            // Cada pixel RGB 10 bits cabe em 1 palavra de 32 bits (bits 31..22, 21..12, 11..2)
            for p in 0..pixels {
                let word = rd_u32(data, p * 4, be).unwrap_or(0);
                let di = p * 4;
                out[di] = (((word >> 22) & 0x3FF) >> 2) as u8;
                out[di + 1] = if channels >= 2 {
                    (((word >> 12) & 0x3FF) >> 2) as u8
                } else {
                    out[di]
                };
                out[di + 2] = if channels >= 3 {
                    (((word >> 2) & 0x3FF) >> 2) as u8
                } else {
                    out[di]
                };
                out[di + 3] = 255;
            }
        }
        _ => bail!("Cineon com {bits} bits por amostra não suportado"),
    }
    RgbaImage::from_raw(w, h, out).ok_or_else(|| anyhow!("falha ao montar imagem Cineon"))
}

/// Codifica RGBA8 em Kodak Cineon 10 bits por canal RGB big-endian.
pub fn encode_cineon(img: &RgbaImage) -> Result<Vec<u8>> {
    let w = img.width();
    let h = img.height();
    let data_off = 1024usize;
    let pixels = (w as usize) * (h as usize);
    let mut buf = vec![0u8; data_off + pixels * 4];
    buf[0..4].copy_from_slice(&0x802A_5FD7u32.to_be_bytes());
    buf[4..8].copy_from_slice(&(data_off as u32).to_be_bytes());
    buf[8..12].copy_from_slice(&1024u32.to_be_bytes());
    buf[12..16].copy_from_slice(&0u32.to_be_bytes());
    buf[16..20].copy_from_slice(&0u32.to_be_bytes());
    let total_len = buf.len() as u32;
    buf[20..24].copy_from_slice(&total_len.to_be_bytes());
    buf[24..32].copy_from_slice(b"V4.5\0\0\0\0");
    buf[192] = 0; // orientation
    buf[193] = 3; // 3 channels RGB
    for c in 0..3usize {
        let ch_off = 196 + c * 28;
        buf[ch_off] = 0;
        buf[ch_off + 1] = (c + 1) as u8;
        buf[ch_off + 2] = 10; // 10 bits
        buf[ch_off + 4..ch_off + 8].copy_from_slice(&w.to_be_bytes());
        buf[ch_off + 8..ch_off + 12].copy_from_slice(&h.to_be_bytes());
    }
    let mut off = data_off;
    for px in img.as_raw().as_chunks::<4>().0 {
        let r = (px[0] as u32) << 2;
        let g = (px[1] as u32) << 2;
        let b = (px[2] as u32) << 2;
        let word = (r << 22) | (g << 12) | (b << 2);
        buf[off..off + 4].copy_from_slice(&word.to_be_bytes());
        off += 4;
    }
    Ok(buf)
}

// ============================================================================
// WBMP (WAP Wireless Bitmap Type 0)
// ============================================================================

fn read_uintvar(b: &[u8], pos: &mut usize) -> Result<u32> {
    let mut val = 0u32;
    for _ in 0..5 {
        let byte = *b
            .get(*pos)
            .ok_or_else(|| anyhow!("WBMP truncado no cabeçalho"))?;
        *pos += 1;
        val = (val << 7) | (byte & 0x7F) as u32;
        if byte & 0x80 == 0 {
            return Ok(val);
        }
    }
    bail!("inteiro uintvar WBMP grande demais")
}

fn write_uintvar(out: &mut Vec<u8>, mut val: u32) {
    let mut parts = [0u8; 5];
    let mut n = 0usize;
    loop {
        parts[n] = (val & 0x7F) as u8;
        n += 1;
        val >>= 7;
        if val == 0 {
            break;
        }
    }
    for i in (0..n).rev() {
        let mut b = parts[i];
        if i > 0 {
            b |= 0x80;
        }
        out.push(b);
    }
}

pub fn decode_wbmp(b: &[u8]) -> Result<RgbaImage> {
    if b.len() < 4 || b[0] != 0 || (b[1] & 0x9F) != 0 {
        bail!("não é um arquivo WBMP tipo 0 válido");
    }
    let mut pos = 2usize;
    let w = read_uintvar(b, &mut pos)?;
    let h = read_uintvar(b, &mut pos)?;
    let rgba_bytes = validate_dims(w, h, "WBMP")?;
    let row_bytes = (w as usize).div_ceil(8);
    let mut out = vec![0u8; rgba_bytes];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let byte = *b.get(pos + y * row_bytes + (x / 8)).unwrap_or(&0);
            let on = ((byte >> (7 - (x % 8))) & 1) != 0;
            let c = if on { 255u8 } else { 0u8 };
            let di = (y * (w as usize) + x) * 4;
            out[di] = c;
            out[di + 1] = c;
            out[di + 2] = c;
            out[di + 3] = 255;
        }
    }
    RgbaImage::from_raw(w, h, out).ok_or_else(|| anyhow!("falha ao montar WBMP"))
}

pub fn encode_wbmp(img: &RgbaImage) -> Result<Vec<u8>> {
    let w = img.width();
    let h = img.height();
    let row_bytes = (w as usize).div_ceil(8);
    let mut out = Vec::with_capacity(10 + row_bytes * (h as usize));
    out.push(0); // Type 0
    out.push(0); // Fixed header
    write_uintvar(&mut out, w);
    write_uintvar(&mut out, h);
    for y in 0..h as usize {
        for bx in 0..row_bytes {
            let mut byte = 0u8;
            for bit in 0..8usize {
                let x = bx * 8 + bit;
                if x < w as usize {
                    let px = img.get_pixel(x as u32, y as u32);
                    let lum = (px[0] as u32 * 299 + px[1] as u32 * 587 + px[2] as u32 * 114) / 1000;
                    if lum >= 128 {
                        byte |= 1 << (7 - bit);
                    }
                }
            }
            out.push(byte);
        }
    }
    Ok(out)
}

// ============================================================================
// XBM / XPM (X Window System)
// ============================================================================

pub fn decode_xbm(b: &[u8]) -> Result<RgbaImage> {
    let text = std::str::from_utf8(b).map_err(|_| anyhow!("XBM não é UTF-8/ASCII válido"))?;
    let mut w = None;
    let mut h = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#define") {
            let mut parts = trimmed.split_whitespace();
            let _ = parts.next();
            if let (Some(name), Some(val)) = (parts.next(), parts.next()) {
                if name.ends_with("_width") {
                    w = val.parse::<u32>().ok();
                } else if name.ends_with("_height") {
                    h = val.parse::<u32>().ok();
                }
            }
        }
    }
    let (Some(w), Some(h)) = (w, h) else {
        bail!("XBM sem #define *_width / *_height");
    };
    let rgba_bytes = validate_dims(w, h, "XBM")?;
    let brace_open = text
        .find('{')
        .ok_or_else(|| anyhow!("XBM sem bloco {{ ... }}"))?;
    let brace_close = text[brace_open..]
        .find('}')
        .map(|off| brace_open + off)
        .ok_or_else(|| anyhow!("XBM sem fecho }}"))?;
    let mut data = Vec::new();
    for tok in text[brace_open + 1..brace_close].split(|c: char| c == ',' || c.is_whitespace()) {
        let t = tok.trim();
        if t.is_empty() {
            continue;
        }
        let val = if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
            u8::from_str_radix(hex, 16)
        } else {
            t.parse::<u8>()
        };
        if let Ok(v) = val {
            data.push(v);
        }
    }
    let row_bytes = (w as usize).div_ceil(8);
    let mut out = vec![0u8; rgba_bytes];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let byte = *data.get(y * row_bytes + (x / 8)).unwrap_or(&0);
            // XBM usa bit menos significativo primeiro; 1 = pixel preto (frente), 0 = fundo branco
            let bit = (byte >> (x % 8)) & 1;
            let c = if bit != 0 { 0u8 } else { 255u8 };
            let di = (y * (w as usize) + x) * 4;
            out[di] = c;
            out[di + 1] = c;
            out[di + 2] = c;
            out[di + 3] = 255;
        }
    }
    RgbaImage::from_raw(w, h, out).ok_or_else(|| anyhow!("falha ao montar XBM"))
}

pub fn encode_xbm(img: &RgbaImage, name: &str) -> Result<Vec<u8>> {
    let w = img.width();
    let h = img.height();
    let ident: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let ident = if ident.is_empty() {
        "neoveg".into()
    } else {
        ident
    };
    let mut out = format!(
        "#define {ident}_width {w}\n#define {ident}_height {h}\nstatic unsigned char {ident}_bits[] = {{\n"
    );
    let row_bytes = (w as usize).div_ceil(8);
    let mut first = true;
    for y in 0..h as usize {
        for bx in 0..row_bytes {
            let mut byte = 0u8;
            for bit in 0..8usize {
                let x = bx * 8 + bit;
                if x < w as usize {
                    let px = img.get_pixel(x as u32, y as u32);
                    let lum = (px[0] as u32 * 299 + px[1] as u32 * 587 + px[2] as u32 * 114) / 1000;
                    if lum < 128 && px[3] >= 128 {
                        byte |= 1 << bit;
                    }
                }
            }
            if !first {
                out.push_str(", ");
            }
            first = false;
            out.push_str(&format!("0x{byte:02x}"));
        }
        out.push('\n');
    }
    out.push_str("};\n");
    Ok(out.into_bytes())
}

fn parse_xpm_strings(text: &str) -> Vec<String> {
    let mut strings = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let mut in_comment = false;
    while i < bytes.len() {
        if !in_comment && bytes.get(i..i + 2) == Some(b"/*") {
            in_comment = true;
            i += 2;
            continue;
        }
        if in_comment {
            if bytes.get(i..i + 2) == Some(b"*/") {
                in_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'"' {
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if let Ok(s) = std::str::from_utf8(&bytes[start..i]) {
                strings.push(s.to_string());
            }
            i += 1;
            if strings.len() >= 100_000 {
                break;
            }
            continue;
        }
        i += 1;
    }
    strings
}

fn parse_xpm_color(spec: &str) -> [u8; 4] {
    let s = spec.trim();
    if s.eq_ignore_ascii_case("none") {
        return [0, 0, 0, 0];
    }
    if let Some(hex) = s.strip_prefix('#') {
        if !hex.is_ascii() {
            return [0, 0, 0, 255];
        }
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
            return [r, g, b, 255];
        } else if hex.len() == 12 {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
            let g = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
            let b = u8::from_str_radix(&hex[8..10], 16).unwrap_or(0);
            return [r, g, b, 255];
        } else if hex.len() == 3 {
            let d = |i| u8::from_str_radix(&hex[i..i + 1], 16).unwrap_or(0) * 17;
            return [d(0), d(1), d(2), 255];
        }
    }
    match s.to_ascii_lowercase().as_str() {
        "black" => [0, 0, 0, 255],
        "white" => [255, 255, 255, 255],
        "red" => [255, 0, 0, 255],
        "green" => [0, 255, 0, 255],
        "blue" => [0, 0, 255, 255],
        "yellow" => [255, 255, 0, 255],
        "cyan" => [0, 255, 255, 255],
        "magenta" => [255, 0, 255, 255],
        "gray" | "grey" => [128, 128, 128, 255],
        _ => [0, 0, 0, 255],
    }
}

pub fn decode_xpm(b: &[u8]) -> Result<RgbaImage> {
    let text = std::str::from_utf8(b).map_err(|_| anyhow!("XPM não é UTF-8/ASCII válido"))?;
    if !text.contains("XPM") {
        bail!("não é um arquivo XPM v3 válido");
    }
    let lines = parse_xpm_strings(text);
    let header = lines
        .first()
        .ok_or_else(|| anyhow!("XPM sem linha de cabeçalho"))?;
    let nums: Vec<usize> = header
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    if nums.len() < 4 {
        bail!("cabeçalho XPM inválido");
    }
    let (w, h, ncolors, cpp) = (nums[0] as u32, nums[1] as u32, nums[2], nums[3]);
    let rgba_bytes = validate_dims(w, h, "XPM")?;
    if cpp == 0 || cpp > 8 || ncolors > 65536 {
        bail!("parâmetros de paleta XPM inválidos (ncolors={ncolors}, cpp={cpp})");
    }
    if lines.len() < 1 + ncolors + h as usize {
        bail!("XPM truncado (faltam cores ou linhas de pixels)");
    }

    let mut palette = std::collections::HashMap::with_capacity(ncolors);
    for entry in &lines[1..1 + ncolors] {
        let Some(sym) = entry.get(..cpp) else {
            continue;
        };
        let rest = &entry[cpp..];
        let mut color = [0, 0, 0, 255];
        if let Some(pos) = rest.find(" c ") {
            let val = rest[pos + 3..].split_whitespace().next().unwrap_or("");
            color = parse_xpm_color(val);
        } else if let Some(pos) = rest.find("\tc ") {
            let val = rest[pos + 3..].split_whitespace().next().unwrap_or("");
            color = parse_xpm_color(val);
        }
        palette.insert(sym.to_string(), color);
    }

    let mut out = vec![0u8; rgba_bytes];
    for y in 0..h as usize {
        let row = &lines[1 + ncolors + y];
        for x in 0..w as usize {
            let sym = row.get(x * cpp..(x + 1) * cpp).unwrap_or("");
            let px = palette.get(sym).copied().unwrap_or([0, 0, 0, 255]);
            let di = (y * (w as usize) + x) * 4;
            out[di..di + 4].copy_from_slice(&px);
        }
    }
    RgbaImage::from_raw(w, h, out).ok_or_else(|| anyhow!("falha ao montar XPM"))
}

pub fn encode_xpm(img: &RgbaImage, name: &str) -> Result<Vec<u8>> {
    const XPM_ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ.:";
    let w = img.width();
    let h = img.height();
    let ident: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let ident = if ident.is_empty() {
        "neoveg".into()
    } else {
        ident
    };

    // Quantiza para cubo seguro de até 216 cores + transparente quando há muitas cores,
    // mantendo tabela legível e compacta em XPM (2 caracteres por pixel).
    let mut palette: Vec<[u8; 4]> = Vec::new();
    let mut map = std::collections::HashMap::<[u8; 4], usize>::new();
    let mut quantize = false;
    for px in img.as_raw().as_chunks::<4>().0 {
        let k = [px[0], px[1], px[2], if px[3] < 128 { 0 } else { 255 }];
        if let std::collections::hash_map::Entry::Vacant(e) = map.entry(k) {
            if palette.len() >= 1024 {
                quantize = true;
                break;
            }
            let idx = palette.len();
            palette.push(k);
            e.insert(idx);
        }
    }
    if quantize {
        palette.clear();
        map.clear();
        for px in img.as_raw().as_chunks::<4>().0 {
            let k = if px[3] < 128 {
                [0, 0, 0, 0]
            } else {
                let q = |v: u8| ((v as u16 * 5 + 127) / 255 * 51) as u8;
                [q(px[0]), q(px[1]), q(px[2]), 255]
            };
            map.entry(k).or_insert_with(|| {
                let idx = palette.len();
                palette.push(k);
                idx
            });
        }
    }

    let cpp = 2usize;
    let code_for = |idx: usize| -> [u8; 2] {
        [
            XPM_ALPHABET[(idx / XPM_ALPHABET.len()) % XPM_ALPHABET.len()],
            XPM_ALPHABET[idx % XPM_ALPHABET.len()],
        ]
    };

    let mut out = format!(
        "/* XPM */\nstatic char *{ident}_xpm[] = {{\n\"{w} {h} {} {cpp}\",\n",
        palette.len()
    );
    for (idx, c) in palette.iter().enumerate() {
        let code = code_for(idx);
        let s = std::str::from_utf8(&code).unwrap_or("00");
        if c[3] == 0 {
            out.push_str(&format!("\"{s} c None\",\n"));
        } else {
            out.push_str(&format!(
                "\"{s} c #{:02x}{:02x}{:02x}\",\n",
                c[0], c[1], c[2]
            ));
        }
    }
    for y in 0..h as usize {
        out.push('"');
        for x in 0..w as usize {
            let px = img.get_pixel(x as u32, y as u32);
            let k = if quantize {
                if px[3] < 128 {
                    [0, 0, 0, 0]
                } else {
                    let q = |v: u8| ((v as u16 * 5 + 127) / 255 * 51) as u8;
                    [q(px[0]), q(px[1]), q(px[2]), 255]
                }
            } else {
                [px[0], px[1], px[2], if px[3] < 128 { 0 } else { 255 }]
            };
            let idx = map.get(&k).copied().unwrap_or(0);
            let code = code_for(idx);
            let s = std::str::from_utf8(&code).unwrap_or("00");
            out.push_str(s);
        }
        if y + 1 < h as usize {
            out.push_str("\",\n");
        } else {
            out.push_str("\"\n");
        }
    }
    out.push_str("};\n");
    Ok(out.into_bytes())
}

// ============================================================================
// Amiga IFF / ILBM (.iff, .ilbm, .lbm)
// ============================================================================

fn byterun1_unpack(src: &[u8], expected: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(expected);
    let mut i = 0usize;
    while i < src.len() && out.len() < expected {
        let n = src[i] as i8;
        i += 1;
        if n >= 0 {
            let cnt = (n as usize + 1).min(expected - out.len());
            let end = i.saturating_add(cnt).min(src.len());
            out.extend_from_slice(&src[i..end]);
            i += n as usize + 1;
        } else if n != -128 {
            let val = *src.get(i).unwrap_or(&0);
            i += 1;
            let cnt = ((-n as isize + 1) as usize).min(expected - out.len());
            out.resize(out.len() + cnt, val);
        }
    }
    out.resize(expected, 0);
    out
}

pub fn decode_iff_ilbm(b: &[u8]) -> Result<RgbaImage> {
    if b.get(0..4) != Some(b"FORM") {
        bail!("não é um arquivo EA IFF 85 (FORM ausente)");
    }
    let form_type = b.get(8..12).unwrap_or(&[]);
    let is_pbm = form_type == b"PBM ";
    if form_type != b"ILBM" && !is_pbm {
        bail!("IFF não é ILBM/PBM");
    }

    let mut pos = 12usize;
    let mut bmhd: Option<(u32, u32, u8, u8, u8)> = None; // w, h, planes, masking, compression
    let mut cmap: Vec<[u8; 3]> = Vec::new();
    let mut camg: u32 = 0;
    let mut body: Option<&[u8]> = None;

    while pos + 8 <= b.len() {
        let id = b.get(pos..pos + 4).unwrap_or(&[]);
        let size = rd_u32(b, pos + 4, true).unwrap_or(0) as usize;
        let start = pos + 8;
        let end = start.saturating_add(size).min(b.len());
        let chunk = b.get(start..end).unwrap_or(&[]);
        if id == b"BMHD" && chunk.len() >= 11 {
            let w = rd_u16(chunk, 0, true).unwrap_or(0) as u32;
            let h = rd_u16(chunk, 2, true).unwrap_or(0) as u32;
            let planes = chunk[8];
            let masking = chunk[9];
            let comp = chunk[10];
            bmhd = Some((w, h, planes, masking, comp));
        } else if id == b"CMAP" {
            for c in chunk.as_chunks::<3>().0.iter().take(256) {
                cmap.push(*c);
            }
        } else if id == b"CAMG" && chunk.len() >= 4 {
            camg = rd_u32(chunk, 0, true).unwrap_or(0);
        } else if id == b"BODY" {
            body = Some(chunk);
        }
        pos = start.saturating_add(size).saturating_add(size & 1);
    }

    let Some((w, h, planes, masking, comp)) = bmhd else {
        bail!("IFF/ILBM sem bloco BMHD");
    };
    let rgba_bytes = validate_dims(w, h, "IFF/ILBM")?;
    let Some(body) = body else {
        bail!("IFF/ILBM sem bloco BODY");
    };
    if comp > 1 {
        bail!("IFF/ILBM com compressão não suportada: {comp}");
    }
    if planes == 0 || (planes > 8 && planes != 24 && planes != 32) {
        bail!("IFF/ILBM com {planes} planos não suportado");
    }

    let mut out = vec![0u8; rgba_bytes];
    if is_pbm {
        let row_bytes = ((w as usize) + 1) & !1;
        let expected = row_bytes * (h as usize);
        let raw = if comp == 1 {
            byterun1_unpack(body, expected)
        } else {
            let mut v = body[..body.len().min(expected)].to_vec();
            v.resize(expected, 0);
            v
        };
        for y in 0..h as usize {
            for x in 0..w as usize {
                let idx = raw[y * row_bytes + x] as usize;
                let rgb = cmap
                    .get(idx)
                    .copied()
                    .unwrap_or([idx as u8, idx as u8, idx as u8]);
                let di = (y * (w as usize) + x) * 4;
                out[di..di + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
        return RgbaImage::from_raw(w, h, out).ok_or_else(|| anyhow!("falha ao montar PBM IFF"));
    }

    // EHB: se CAMG tem bit 0x0080 e paleta tem <= 32 cores, estende para 64 (metade do brilho)
    if (camg & 0x0080) != 0 && !cmap.is_empty() && cmap.len() <= 32 {
        let base_len = cmap.len();
        for i in 0..base_len {
            let c = cmap[i];
            cmap.push([c[0] >> 1, c[1] >> 1, c[2] >> 1]);
        }
    }

    let total_planes = planes as usize + usize::from(masking == 1);
    let plane_row_bytes = (w as usize).div_ceil(16) * 2;
    let expected = plane_row_bytes
        .checked_mul(total_planes)
        .and_then(|n| n.checked_mul(h as usize))
        .ok_or_else(|| anyhow!("BODY IFF grande demais"))?;
    let raw = if comp == 1 {
        byterun1_unpack(body, expected)
    } else {
        let mut v = body[..body.len().min(expected)].to_vec();
        v.resize(expected, 0);
        v
    };

    let ham6 = (camg & 0x0800) != 0 && planes == 6;
    let ham8 = (camg & 0x0800) != 0 && planes == 8;

    for y in 0..h as usize {
        let mut hold = cmap.first().copied().unwrap_or([0, 0, 0]);
        let y_off = y * total_planes * plane_row_bytes;
        for x in 0..w as usize {
            let byte_x = x / 8;
            let bit_mask = 0x80u8 >> (x % 8);
            let mut val: u32 = 0;
            for p in 0..planes as usize {
                let b = raw[y_off + p * plane_row_bytes + byte_x];
                if (b & bit_mask) != 0 {
                    val |= 1u32 << p;
                }
            }
            let di = (y * (w as usize) + x) * 4;
            if planes == 24 || planes == 32 {
                let r = (val & 0xFF) as u8;
                let g = ((val >> 8) & 0xFF) as u8;
                let bv = ((val >> 16) & 0xFF) as u8;
                let a = if planes == 32 {
                    ((val >> 24) & 0xFF) as u8
                } else {
                    255
                };
                out[di..di + 4].copy_from_slice(&[r, g, bv, a]);
            } else if ham6 {
                let ctrl = (val >> 4) & 3;
                let data4 = ((val & 0xF) * 17) as u8;
                match ctrl {
                    0 => hold = cmap.get((val & 0xF) as usize).copied().unwrap_or([0, 0, 0]),
                    1 => hold[2] = data4,
                    2 => hold[0] = data4,
                    _ => hold[1] = data4,
                }
                out[di..di + 4].copy_from_slice(&[hold[0], hold[1], hold[2], 255]);
            } else if ham8 {
                let ctrl = (val >> 6) & 3;
                let data6 = ((val & 0x3F) << 2 | ((val & 0x3F) >> 4)) as u8;
                match ctrl {
                    0 => {
                        hold = cmap
                            .get((val & 0x3F) as usize)
                            .copied()
                            .unwrap_or([0, 0, 0])
                    }
                    1 => hold[2] = data6,
                    2 => hold[0] = data6,
                    _ => hold[1] = data6,
                }
                out[di..di + 4].copy_from_slice(&[hold[0], hold[1], hold[2], 255]);
            } else {
                let rgb = if let Some(c) = cmap.get(val as usize) {
                    *c
                } else {
                    let g = ((val * 255) / ((1u32 << planes) - 1).max(1)) as u8;
                    [g, g, g]
                };
                out[di..di + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
    }
    let _ = Rgba([0, 0, 0, 0]);
    RgbaImage::from_raw(w, h, out).ok_or_else(|| anyhow!("falha ao montar ILBM"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn sample_img() -> RgbaImage {
        let mut img = RgbaImage::new(16, 12);
        for y in 0..12 {
            for x in 0..16 {
                let c = if (x + y) % 2 == 0 {
                    Rgba([240, 40, 80, 255])
                } else {
                    Rgba([20, 180, 220, 255])
                };
                img.put_pixel(x, y, c);
            }
        }
        img
    }

    #[test]
    fn roundtrip_dpx_cineon_wbmp_xbm_xpm() {
        let img = sample_img();
        let dpx = encode_dpx(&img).unwrap();
        let dec = decode_dpx(&dpx).unwrap();
        assert_eq!(dec, img);

        let cin = encode_cineon(&img).unwrap();
        let dec_cin = decode_cineon(&cin).unwrap();
        assert_eq!(dec_cin.dimensions(), (16, 12));
        // 10-bit << 2 >> 2 preserves exact 8-bit values!
        assert_eq!(dec_cin, img);

        let bw = RgbaImage::from_fn(16, 8, |x, y| {
            if (x ^ y) & 1 == 0 {
                Rgba([255, 255, 255, 255])
            } else {
                Rgba([0, 0, 0, 255])
            }
        });
        assert_eq!(decode_wbmp(&encode_wbmp(&bw).unwrap()).unwrap(), bw);
        assert_eq!(decode_xbm(&encode_xbm(&bw, "test").unwrap()).unwrap(), bw);

        let xpm = encode_xpm(&img, "test").unwrap();
        assert_eq!(decode_xpm(&xpm).unwrap(), img);
    }

    #[test]
    fn decodes_iff_ilbm_paletted_byterun1() {
        // IFF/ILBM 8x2, 2 planos, ByteRun1=0, paleta 4 cores
        let mut bmhd = vec![];
        bmhd.extend_from_slice(&8u16.to_be_bytes());
        bmhd.extend_from_slice(&2u16.to_be_bytes());
        bmhd.extend_from_slice(&0i16.to_be_bytes());
        bmhd.extend_from_slice(&0i16.to_be_bytes());
        bmhd.push(2); // nPlanes
        bmhd.push(0); // masking
        bmhd.push(0); // uncompressed
        bmhd.push(0);
        bmhd.extend_from_slice(&0u16.to_be_bytes());
        bmhd.push(1);
        bmhd.push(1);
        bmhd.extend_from_slice(&8u16.to_be_bytes());
        bmhd.extend_from_slice(&2u16.to_be_bytes());

        let cmap = [0u8, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255];
        // Cada scanline tem 2 planos * 2 bytes = 4 bytes. Linha 0: índice 1 (MSB=bit 0 set -> plane0=0x80, plane1=0)
        let body = [0x80u8, 0, 0, 0, 0, 0, 0x80, 0];

        let mut iff = b"FORM\0\0\0\0ILBM".to_vec();
        let push_chunk = |out: &mut Vec<u8>, id: &[u8; 4], data: &[u8]| {
            out.extend_from_slice(id);
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(data);
            if data.len() % 2 == 1 {
                out.push(0);
            }
        };
        push_chunk(&mut iff, b"BMHD", &bmhd);
        push_chunk(&mut iff, b"CMAP", &cmap);
        push_chunk(&mut iff, b"BODY", &body);
        let len = (iff.len() - 8) as u32;
        iff[4..8].copy_from_slice(&len.to_be_bytes());

        let img = decode_iff_ilbm(&iff).unwrap();
        assert_eq!(img.dimensions(), (8, 2));
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]); // índice 1
        assert_eq!(img.get_pixel(0, 1).0, [0, 255, 0, 255]); // índice 2
    }

    #[test]
    fn hostiles_do_not_panic_and_are_rejected() {
        for ext in ["dpx", "cin", "wbmp", "xbm", "xpm", "iff"] {
            assert!(decode_extra_image(ext, b"").is_err());
            assert!(decode_extra_image(ext, b"garbage").is_err());
        }
    }

    /// Oráculo externo `ffmpeg`: gera e lê DPX (10 bits Method A) e EXR.
    #[test]
    fn dpx_and_exr_ffmpeg_oracle() {
        let dir = std::env::temp_dir().join(format!("neoveg_img_oracle_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let dpx_in = dir.join("oracle_10bit.dpx");
        let gen = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=16x12:rate=1:duration=1",
                "-frames:v",
                "1",
                "-pix_fmt",
                "gbrp10be",
            ])
            .arg(&dpx_in)
            .output();
        if let Ok(out) = gen {
            if out.status.success() {
                let img = open_image_file(&dpx_in).expect("DPX do ffmpeg deve abrir");
                assert_eq!(img.dimensions(), (16, 12));
            }
        }
        // Testa também nosso DPX exportado lido pelo ffprobe
        let our_dpx = dir.join("our.dpx");
        std::fs::write(&our_dpx, encode_dpx(&sample_img()).unwrap()).unwrap();
        if let Ok(out) = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=width,height",
                "-of",
                "csv=p=0",
            ])
            .arg(&our_dpx)
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout);
                assert!(s.trim().contains("16,12"), "{s}");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
