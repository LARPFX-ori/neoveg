//! YUV4MPEG2 (.y4m): vídeo não comprimido, simples de ler e escrever.

use anyhow::{bail, Context, Result};
use image::RgbaImage;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Chroma {
    C420,
    C422,
    C444,
    Mono,
}

/// Maior dimensão aceita na leitura.
const MAX_DIM: u32 = 16384;

pub struct Y4mReader {
    file: File,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    chroma: Chroma,
    data_start: u64,
    frame_header_len: u64,
    frame_size: u64,
    pub frames: usize,
}

impl Y4mReader {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("abrindo {}", path.display()))?;
        let len = file.metadata()?.len();
        let mut br = BufReader::new(file);
        // o cabeçalho tem poucos bytes: limita a leitura (entrada hostil, §0.4)
        let mut header = String::new();
        (&mut br).take(4096).read_line(&mut header)?;
        if !header.ends_with('\n') {
            bail!("cabeçalho Y4M ausente ou grande demais");
        }
        if !header.starts_with("YUV4MPEG2") {
            bail!("não é um arquivo YUV4MPEG2");
        }
        let (mut w, mut h, mut fps, mut chroma) = (0u32, 0u32, 25.0f64, Chroma::C420);
        for tok in header.split_whitespace().skip(1) {
            let mut chars = tok.chars();
            let Some(k) = chars.next() else { continue };
            let v = chars.as_str();
            match k {
                'W' => w = v.parse()?,
                'H' => h = v.parse()?,
                'F' => {
                    if let Some((a, b)) = v.split_once(':') {
                        let (a, b): (f64, f64) = (a.parse()?, b.parse()?);
                        if b > 0.0 && (0.1..=1000.0).contains(&(a / b)) {
                            fps = a / b;
                        }
                    }
                }
                'C' => {
                    chroma = if v.starts_with("420") {
                        Chroma::C420
                    } else if v.starts_with("422") {
                        Chroma::C422
                    } else if v.starts_with("444") {
                        Chroma::C444
                    } else if v.starts_with("mono") {
                        Chroma::Mono
                    } else {
                        bail!("subamostragem Y4M não suportada: {v}")
                    }
                }
                _ => {}
            }
        }
        if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM {
            bail!("dimensões Y4M inválidas: {w}x{h}");
        }
        let data_start = header.len() as u64;
        // tamanho do cabeçalho de quadro
        let mut fh = String::new();
        (&mut br).take(4096).read_line(&mut fh)?;
        if !fh.starts_with("FRAME") {
            bail!("Y4M sem quadros");
        }
        let (cw, ch) = chroma_dims(chroma, w, h);
        let frame_size = w as u64 * h as u64 + 2 * (cw as u64 * ch as u64);
        let frame_header_len = fh.len() as u64;
        let frames = (len.saturating_sub(data_start) / (frame_header_len + frame_size)) as usize;
        if frames == 0 {
            bail!("Y4M sem nenhum quadro completo");
        }
        let file = br.into_inner();
        Ok(Y4mReader {
            file,
            width: w,
            height: h,
            fps,
            chroma,
            data_start,
            frame_header_len,
            frame_size,
            frames,
        })
    }

    pub fn duration(&self) -> f64 {
        self.frames as f64 / self.fps
    }

    pub fn decode_frame(&mut self, idx: usize) -> Result<RgbaImage> {
        let idx = idx.min(self.frames.saturating_sub(1));
        let off = self.data_start
            + idx as u64 * (self.frame_header_len + self.frame_size)
            + self.frame_header_len;
        self.file.seek(SeekFrom::Start(off))?;
        let mut buf = vec![0u8; self.frame_size as usize];
        self.file.read_exact(&mut buf)?;
        let (w, h) = (self.width as usize, self.height as usize);
        let (cw, ch) = chroma_dims(self.chroma, self.width, self.height);
        let (cw, ch) = (cw as usize, ch as usize);
        let (yp, rest) = buf.split_at(w * h);
        let (up, vp) = rest.split_at(cw * ch);
        let mut img = RgbaImage::new(w as u32, h as u32);
        let hd = h >= 720;
        for (y, row) in img.as_mut().chunks_exact_mut(w * 4).enumerate() {
            for x in 0..w {
                let yy = yp[y * w + x];
                let (u, v) = if self.chroma == Chroma::Mono {
                    (128, 128)
                } else {
                    let cx = x * cw / w;
                    let cy = y * ch / h;
                    (up[cy * cw + cx], vp[cy * cw + cx])
                };
                let (r, g, b) = yuv_to_rgb(yy, u, v, hd);
                row[x * 4..x * 4 + 4].copy_from_slice(&[r, g, b, 255]);
            }
        }
        Ok(img)
    }
}

fn chroma_dims(c: Chroma, w: u32, h: u32) -> (u32, u32) {
    match c {
        Chroma::C420 => (w.div_ceil(2), h.div_ceil(2)),
        Chroma::C422 => (w.div_ceil(2), h),
        Chroma::C444 => (w, h),
        Chroma::Mono => (0, 0),
    }
}

/// YCbCr de faixa limitada (BT.709 para HD, BT.601 para SD) -> RGB.
pub fn yuv_to_rgb(y: u8, u: u8, v: u8, hd: bool) -> (u8, u8, u8) {
    let y = (y as f32 - 16.0) * (255.0 / 219.0);
    let u = (u as f32 - 128.0) * (255.0 / 224.0);
    let v = (v as f32 - 128.0) * (255.0 / 224.0);
    let (r, g, b) = if hd {
        (y + 1.5748 * v, y - 0.1873 * u - 0.4681 * v, y + 1.8556 * u)
    } else {
        (
            y + 1.402 * v,
            y - 0.344136 * u - 0.714136 * v,
            y + 1.772 * u,
        )
    };
    (clamp8(r), clamp8(g), clamp8(b))
}

pub fn rgb_to_yuv(r: f32, g: f32, b: f32, hd: bool) -> (f32, f32, f32) {
    // r,g,b em 0..255
    let (kr, kb) = if hd { (0.2126, 0.0722) } else { (0.299, 0.114) };
    let kg = 1.0 - kr - kb;
    let y = kr * r + kg * g + kb * b;
    let u = (b - y) / (2.0 * (1.0 - kb));
    let v = (r - y) / (2.0 * (1.0 - kr));
    (
        16.0 + y * 219.0 / 255.0,
        128.0 + u * 224.0 / 255.0,
        128.0 + v * 224.0 / 255.0,
    )
}

fn clamp8(x: f32) -> u8 {
    x.round().clamp(0.0, 255.0) as u8
}

pub struct Y4mWriter {
    out: BufWriter<File>,
    w: u32,
    h: u32,
}

impl Y4mWriter {
    pub fn create(path: &Path, w: u32, h: u32, fps: f64) -> Result<Self> {
        let mut out = BufWriter::with_capacity(1 << 22, File::create(path)?);
        let (num, den) = if (fps - 29.97).abs() < 0.01 {
            (30000, 1001)
        } else if (fps - 23.976).abs() < 0.01 {
            (24000, 1001)
        } else if (fps - 59.94).abs() < 0.01 {
            (60000, 1001)
        } else {
            ((fps * 1000.0).round() as u32, 1000)
        };
        // largura/altura pares para 4:2:0
        writeln!(out, "YUV4MPEG2 W{w} H{h} F{num}:{den} Ip A1:1 C420jpeg")?;
        Ok(Y4mWriter { out, w, h })
    }

    /// Recebe RGBA 8 bits.
    pub fn write_frame(&mut self, rgba: &[u8]) -> Result<()> {
        let (w, h) = (self.w as usize, self.h as usize);
        if rgba.len() < w * h * 4 {
            bail!("quadro com tamanho incorreto para o Y4M");
        }
        let hd = h >= 720;
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        let mut yp = vec![0u8; w * h];
        let mut up = vec![0f32; cw * ch];
        let mut vp = vec![0f32; cw * ch];
        let mut cnt = vec![0f32; cw * ch];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                let (yy, u, v) =
                    rgb_to_yuv(rgba[i] as f32, rgba[i + 1] as f32, rgba[i + 2] as f32, hd);
                yp[y * w + x] = clamp8(yy);
                let c = (y / 2) * cw + x / 2;
                up[c] += u;
                vp[c] += v;
                cnt[c] += 1.0;
            }
        }
        self.out.write_all(b"FRAME\n")?;
        self.out.write_all(&yp)?;
        let u8s: Vec<u8> = up.iter().zip(&cnt).map(|(a, n)| clamp8(a / n)).collect();
        self.out.write_all(&u8s)?;
        let v8s: Vec<u8> = vp.iter().zip(&cnt).map(|(a, n)| clamp8(a / n)).collect();
        self.out.write_all(&v8s)?;
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        self.out.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        let p = std::env::temp_dir().join("nvov_test.y4m");
        let mut w = Y4mWriter::create(&p, 8, 6, 25.0).unwrap();
        let px: Vec<u8> = (0..8 * 6).flat_map(|_| [250u8, 20, 20, 255]).collect();
        w.write_frame(&px).unwrap();
        w.write_frame(&px).unwrap();
        w.finish().unwrap();
        let mut r = Y4mReader::open(&p).unwrap();
        assert_eq!(r.frames, 2);
        let img = r.decode_frame(1).unwrap();
        let q = img.get_pixel(3, 3);
        assert!(q[0] > 230 && q[1] < 40, "{:?}", q);
        let _ = std::fs::remove_file(p);
    }
}
