//! Renderização final (Render As): AVI MJPEG + PCM, sequência PNG, Y4M e WAV.

use crate::engine::audio::Mixer;
use crate::engine::video::render_frame;
use crate::media::avi::AviWriter;
use crate::media::y4m::Y4mWriter;
use crate::media::MediaCache;
use crate::model::{Project, Ticks, TICKS_PER_SEC};
use crate::util::lock;
use anyhow::{bail, Context, Result};
use rayon::prelude::*;
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderFormat {
    AviMjpeg,
    PngSequence,
    Y4m,
    Wav,
}

impl RenderFormat {
    pub const ALL: [RenderFormat; 4] = [
        RenderFormat::AviMjpeg,
        RenderFormat::PngSequence,
        RenderFormat::Y4m,
        RenderFormat::Wav,
    ];
    pub fn name(self) -> &'static str {
        match self {
            RenderFormat::AviMjpeg => "AVI (Motion-JPEG + PCM)",
            RenderFormat::PngSequence => "Sequência de imagens PNG (+ WAV)",
            RenderFormat::Y4m => "YUV4MPEG2 .y4m não comprimido (+ WAV)",
            RenderFormat::Wav => "Somente áudio WAV",
        }
    }
    pub fn ext(self) -> &'static str {
        match self {
            RenderFormat::AviMjpeg => "avi",
            RenderFormat::PngSequence => "png",
            RenderFormat::Y4m => "y4m",
            RenderFormat::Wav => "wav",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WavBits {
    Pcm16,
    Pcm24,
    Float32,
}

#[derive(Clone, Debug)]
pub struct RenderSettings {
    pub format: RenderFormat,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub start: Ticks,
    pub end: Ticks,
    pub jpeg_quality: u8,
    pub include_audio: bool,
    pub wav_bits: WavBits,
}

#[derive(Default)]
pub struct RenderProgress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
    pub finished: AtomicBool,
    pub error: Mutex<Option<String>>,
    pub started: Mutex<Option<Instant>>,
    pub elapsed: Mutex<f64>,
}

impl RenderProgress {
    pub fn fraction(&self) -> f32 {
        let t = self.total.load(Ordering::Relaxed).max(1);
        self.done.load(Ordering::Relaxed) as f32 / t as f32
    }
}

pub fn spawn(project: Project, media: Arc<MediaCache>, s: RenderSettings) -> Arc<RenderProgress> {
    let prog = Arc::new(RenderProgress::default());
    *prog
        .started
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Instant::now());
    let pr = prog.clone();
    let spawned = std::thread::Builder::new()
        .name("render".into())
        .spawn(move || {
            let t0 = Instant::now();
            // §0.6: um pânico no render vira erro e o job termina (nunca fica "em execução")
            let r = match crate::util::guard("renderização", || run(&project, &media, &s, &pr)) {
                Ok(r) => r.map_err(|e| format!("{e:#}")),
                Err(e) => Err(e),
            };
            *lock(&pr.elapsed) = t0.elapsed().as_secs_f64();
            if let Err(e) = r {
                crate::util::log_error(&format!("renderização falhou: {e}"));
                *lock(&pr.error) = Some(e);
            }
            pr.finished.store(true, Ordering::SeqCst);
        });
    if let Err(e) = spawned {
        *lock(&prog.error) = Some(format!("não foi possível iniciar a renderização: {e}"));
        prog.finished.store(true, Ordering::SeqCst);
    }
    prog
}

/// Gravador WAV simples (PCM 16/24 bits ou float 32), estéreo ou 5.1.
pub struct WavWriter {
    out: BufWriter<File>,
    bits: WavBits,
    data_bytes: u64,
    /// Tamanho do bloco `fmt ` (16 no PCM clássico, 40 no WAVE_FORMAT_EXTENSIBLE).
    fmt_len: u32,
}

/// Máscara de canais 5.1 do WAVE_FORMAT_EXTENSIBLE: FL, FR, FC, LFE, BL, BR.
const WAV_MASK_5_1: u32 = 0x3F;

impl WavWriter {
    pub fn create(path: &Path, sr: u32, bits: WavBits) -> Result<Self> {
        Self::create_with_channels(path, sr, bits, 2)
    }

    /// Cria um WAV com `channels` canais (2 ou 6). Acima de 2 canais usa
    /// WAVE_FORMAT_EXTENSIBLE com máscara 5.1, como pede a especificação
    /// pública do formato para mais de dois canais.
    pub fn create_with_channels(
        path: &Path,
        sr: u32,
        bits: WavBits,
        channels: u16,
    ) -> Result<Self> {
        if channels != 2 && channels != 6 {
            bail!("WAV com {channels} canais não suportado (use 2 ou 6)");
        }
        let mut out = BufWriter::new(
            File::create(path).with_context(|| format!("criando {}", path.display()))?,
        );
        let (fmt, bps): (u16, u16) = match bits {
            WavBits::Pcm16 => (1, 16),
            WavBits::Pcm24 => (1, 24),
            WavBits::Float32 => (3, 32),
        };
        let block = channels * bps / 8;
        let extensible = channels > 2;
        let fmt_len: u32 = if extensible { 40 } else { 16 };
        out.write_all(b"RIFF")?;
        out.write_all(&0u32.to_le_bytes())?;
        out.write_all(b"WAVEfmt ")?;
        out.write_all(&fmt_len.to_le_bytes())?;
        let tag: u16 = if extensible { 0xFFFE } else { fmt };
        out.write_all(&tag.to_le_bytes())?;
        out.write_all(&channels.to_le_bytes())?;
        out.write_all(&sr.to_le_bytes())?;
        out.write_all(&sr.saturating_mul(block as u32).to_le_bytes())?;
        out.write_all(&block.to_le_bytes())?;
        out.write_all(&bps.to_le_bytes())?;
        if extensible {
            out.write_all(&22u16.to_le_bytes())?; // cbSize
            out.write_all(&bps.to_le_bytes())?; // wValidBitsPerSample
            out.write_all(&WAV_MASK_5_1.to_le_bytes())?;
            // SubFormat GUID {0000000X-0000-0010-8000-00AA00389B71}
            out.write_all(&u32::from(fmt).to_le_bytes())?;
            out.write_all(&[
                0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
            ])?;
        }
        out.write_all(b"data")?;
        out.write_all(&0u32.to_le_bytes())?;
        Ok(WavWriter {
            out,
            bits,
            data_bytes: 0,
            fmt_len,
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
        let mut f = self.out.into_inner().map_err(|e| anyhow::anyhow!("{e}"))?;
        // RIFF = "WAVE" + bloco fmt (8 + fmt_len) + cabeçalho data (8) + dados
        let header = 4 + 8 + u64::from(self.fmt_len) + 8;
        let riff = u32::try_from(header + self.data_bytes)
            .map_err(|_| anyhow::anyhow!("WAV acima de 4 GiB não é suportado"))?;
        f.seek(SeekFrom::Start(4))?;
        f.write_all(&riff.to_le_bytes())?;
        f.seek(SeekFrom::Start(12 + 8 + u64::from(self.fmt_len) + 4))?;
        f.write_all(&(self.data_bytes as u32).to_le_bytes())?;
        Ok(())
    }
}

fn to_i16(s: &[f32]) -> Vec<i16> {
    s.iter()
        .map(|v| (v.clamp(-1.0, 1.0) * 32767.0).round() as i16)
        .collect()
}

pub fn run(
    p: &Project,
    media: &MediaCache,
    s: &RenderSettings,
    prog: &RenderProgress,
) -> Result<()> {
    if s.end <= s.start {
        bail!("intervalo de renderização vazio");
    }
    let fps = p.settings.fps;
    let sr = p.settings.sample_rate;
    let dur = (s.end - s.start).0 as i128;
    let den = TICKS_PER_SEC as i128 * fps.den.max(1) as i128;
    let n_frames = ((dur * fps.num.max(1) as i128 + den / 2) / den).max(1) as u64;
    let (w, h) = (s.width.max(2) as usize, s.height.max(2) as usize);
    let mut mixer = Mixer::new_offline(sr);
    let sample_at = |i: u64| (s.start + fps.frame_to_ticks(i as i64)).to_samples_round(sr);

    if s.format == RenderFormat::Wav {
        let total = (sample_at(n_frames) - sample_at(0)).max(0) as u64;
        prog.total.store(total, Ordering::Relaxed);
        // WAV sai no layout do projeto (estéreo ou 5.1 de 6 canais)
        let ch = p.settings.channel_layout.channels();
        let mut wav = WavWriter::create_with_channels(&s.path, sr, s.wav_bits, ch as u16)?;
        let mut pos = sample_at(0);
        let end = sample_at(n_frames);
        let mut buf = vec![0f32; 8192 * ch];
        let am = media.audio_map();
        while pos < end {
            if prog.cancel.load(Ordering::Relaxed) {
                bail!("cancelado");
            }
            let n = ((end - pos) as usize).min(8192);
            mixer.process_multichannel(p, &am, pos, &mut buf[..n * ch]);
            wav.write(&buf[..n * ch])?;
            pos += n as i64;
            prog.done
                .store((pos - sample_at(0)) as u64, Ordering::Relaxed);
        }
        return wav.finish();
    }

    prog.total.store(n_frames, Ordering::Relaxed);
    let audio_map = media.audio_map();
    let has_audio = s.include_audio && !audio_map.is_empty();

    enum Sink {
        Avi(AviWriter),
        Png(PathBuf, String),
        Y4m(Y4mWriter),
    }
    let mut sink = match s.format {
        RenderFormat::AviMjpeg => Sink::Avi(AviWriter::create(
            &s.path,
            w as u32,
            h as u32,
            fps,
            if has_audio { Some(sr) } else { None },
        )?),
        RenderFormat::PngSequence => {
            let dir = s
                .path
                .parent()
                .map(|d| d.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            let stem = s
                .path
                .file_stem()
                .map(|x| x.to_string_lossy().to_string())
                .unwrap_or_else(|| "quadro".into());
            Sink::Png(dir, stem)
        }
        RenderFormat::Y4m => Sink::Y4m(Y4mWriter::create(&s.path, w as u32, h as u32, fps)?),
        RenderFormat::Wav => bail!("formato de vídeo esperado"),
    };
    // WAV separado para formatos sem áudio embutido
    let mut side_wav = if has_audio && s.format != RenderFormat::AviMjpeg {
        Some(WavWriter::create(
            &s.path.with_extension("wav"),
            sr,
            s.wav_bits,
        )?)
    } else {
        None
    };

    let batch = rayon::current_num_threads().clamp(1, 8) as u64;
    let mut i = 0u64;
    let quality = s.jpeg_quality.clamp(10, 100);
    while i < n_frames {
        if prog.cancel.load(Ordering::Relaxed) {
            bail!("cancelado");
        }
        let b_end = (i + batch).min(n_frames);
        // renderiza e codifica um lote de quadros em paralelo
        let encoded: Vec<Result<Vec<u8>>> = (i..b_end)
            .into_par_iter()
            .map(|fi| {
                let t = s.start + fps.frame_to_ticks(fi as i64);
                let f = render_frame(p, media, t, w, h);
                match s.format {
                    RenderFormat::AviMjpeg => {
                        let rgb = f.to_rgb8_opaque();
                        let mut jpg = Vec::with_capacity(w * h / 4);
                        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, quality)
                            .encode(&rgb, w as u32, h as u32, image::ExtendedColorType::Rgb8)?;
                        Ok(jpg)
                    }
                    RenderFormat::PngSequence => {
                        let rgb = f.to_rgb8_opaque();
                        let mut png = Vec::new();
                        image::codecs::png::PngEncoder::new(&mut png)
                            .write_image_compat(&rgb, w as u32, h as u32)?;
                        Ok(png)
                    }
                    _ => {
                        let mut v = f.to_rgba8();
                        v.shrink_to_fit();
                        Ok(v)
                    }
                }
            })
            .collect();
        for (k, data) in encoded.into_iter().enumerate() {
            let fi = i + k as u64;
            let data = data?;
            match &mut sink {
                Sink::Avi(wr) => wr.write_video_jpeg(&data)?,
                Sink::Png(dir, stem) => {
                    std::fs::write(dir.join(format!("{stem}_{:06}.png", fi)), &data)?;
                }
                Sink::Y4m(wr) => wr.write_frame(&data)?,
            }
            if has_audio {
                let a0 = sample_at(fi);
                let a1 = sample_at(fi + 1);
                let n = (a1 - a0).max(0) as usize;
                let mut buf = vec![0f32; n * 2];
                mixer.process(p, &audio_map, a0, &mut buf);
                match (&mut sink, &mut side_wav) {
                    (Sink::Avi(wr), _) => wr.write_audio(&to_i16(&buf))?,
                    (_, Some(wav)) => wav.write(&buf)?,
                    _ => {}
                }
            }
            prog.done.store(fi + 1, Ordering::Relaxed);
        }
        i = b_end;
    }
    match sink {
        Sink::Avi(wr) => wr.finish()?,
        Sink::Y4m(wr) => wr.finish()?,
        Sink::Png(..) => {}
    }
    if let Some(w) = side_wav {
        w.finish()?;
    }
    Ok(())
}

trait PngCompat {
    fn write_image_compat(self, buf: &[u8], w: u32, h: u32) -> Result<()>;
}

impl<W: Write> PngCompat for image::codecs::png::PngEncoder<W> {
    fn write_image_compat(self, buf: &[u8], w: u32, h: u32) -> Result<()> {
        use image::ImageEncoder;
        self.write_image(buf, w, h, image::ExtendedColorType::Rgb8)?;
        Ok(())
    }
}

/// Salva o quadro atual em PNG na resolução do projeto (Save Snapshot).
pub fn save_snapshot(p: &Project, media: &MediaCache, t: Ticks, path: &Path) -> Result<()> {
    let f = render_frame(
        p,
        media,
        t,
        p.settings.width as usize,
        p.settings.height as usize,
    );
    let Some(img) = f.to_image() else {
        bail!("quadro com tamanho inválido")
    };
    img.save(path)?;
    Ok(())
}

#[cfg(test)]
mod wav_tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("neoveg_{}_{name}", std::process::id()))
    }

    fn u16_at(b: &[u8], i: usize) -> u16 {
        u16::from_le_bytes([b[i], b[i + 1]])
    }

    fn u32_at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    }

    #[test]
    fn wav_5_1_uses_extensible_header() {
        let path = tmp("51.wav");
        let mut w =
            WavWriter::create_with_channels(&path, 48000, WavBits::Float32, 6).expect("cria wav");
        w.write(&[0.25; 12]).expect("grava");
        w.finish().expect("fecha");
        let b = std::fs::read(&path).expect("lê");
        let _ = std::fs::remove_file(&path);
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(u32_at(&b, 4) as usize, b.len() - 8);
        assert_eq!(u32_at(&b, 16), 40);
        assert_eq!(u16_at(&b, 20), 0xFFFE);
        assert_eq!(u16_at(&b, 22), 6);
        assert_eq!(u16_at(&b, 32), 24); // bloco = 6 canais × 4 bytes
        assert_eq!(u32_at(&b, 40), WAV_MASK_5_1);
        assert_eq!(u32_at(&b, 44), 3); // SubFormat IEEE float
        assert_eq!(&b[60..64], b"data");
        assert_eq!(u32_at(&b, 64), 48);
        assert_eq!(b.len(), 68 + 48);
    }

    #[test]
    fn wav_stereo_header_unchanged() {
        let path = tmp("st.wav");
        let mut w = WavWriter::create(&path, 44100, WavBits::Pcm16).expect("cria wav");
        w.write(&[0.0; 4]).expect("grava");
        w.finish().expect("fecha");
        let b = std::fs::read(&path).expect("lê");
        let _ = std::fs::remove_file(&path);
        assert_eq!(u32_at(&b, 4) as usize, b.len() - 8);
        assert_eq!(u32_at(&b, 16), 16);
        assert_eq!(u16_at(&b, 20), 1);
        assert_eq!(u16_at(&b, 22), 2);
        assert_eq!(&b[36..40], b"data");
        assert_eq!(u32_at(&b, 40), 8);
    }

    /// Oráculo externo (§2.3): o `ffprobe` lê o WAV 5.1 como 6 canais em
    /// layout 5.1. Pula quando o `ffprobe` não está instalado.
    #[test]
    fn wav_5_1_oracle_ffprobe() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("target")
            .join("fixtures");
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let path = dir.join("oracle_5_1.wav");
        let mut w =
            WavWriter::create_with_channels(&path, 48000, WavBits::Pcm24, 6).expect("cria wav");
        w.write(&vec![0.1; 48000 * 6 / 10]).expect("grava");
        w.finish().expect("fecha");
        let Ok(out) = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "a:0",
                "-show_entries",
                "stream=channels,channel_layout",
                "-of",
                "default=noprint_wrappers=1",
            ])
            .arg(&path)
            .output()
        else {
            return;
        };
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "ffprobe falhou: {text}");
        assert!(text.contains("channels=6"), "{text}");
        assert!(text.contains("channel_layout=5.1"), "{text}");
    }

    #[test]
    fn wav_rejects_unsupported_channel_count() {
        let path = tmp("bad.wav");
        assert!(WavWriter::create_with_channels(&path, 48000, WavBits::Pcm16, 3).is_err());
        assert!(!path.exists());
    }
}
