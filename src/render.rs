//! Renderização final (Render As): AVI MJPEG + PCM, sequência PNG, Y4M e WAV.

use crate::engine::audio::Mixer;
use crate::engine::video::render_frame;
use crate::media::avi::AviWriter;
use crate::media::y4m::Y4mWriter;
use crate::media::MediaCache;
use crate::model::Project;
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
    pub const ALL: [RenderFormat; 4] = [RenderFormat::AviMjpeg, RenderFormat::PngSequence, RenderFormat::Y4m, RenderFormat::Wav];
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
    pub start: f64,
    pub end: f64,
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
    *prog.started.lock().unwrap() = Some(Instant::now());
    let pr = prog.clone();
    std::thread::Builder::new()
        .name("render".into())
        .spawn(move || {
            let t0 = Instant::now();
            let r = run(&project, &media, &s, &pr);
            *pr.elapsed.lock().unwrap() = t0.elapsed().as_secs_f64();
            if let Err(e) = r {
                *pr.error.lock().unwrap() = Some(format!("{e:#}"));
            }
            pr.finished.store(true, Ordering::SeqCst);
        })
        .expect("thread de render");
    prog
}

/// Gravador WAV simples (PCM 16/24 bits ou float 32), estéreo.
pub struct WavWriter {
    out: BufWriter<File>,
    bits: WavBits,
    data_bytes: u64,
}

impl WavWriter {
    pub fn create(path: &Path, sr: u32, bits: WavBits) -> Result<Self> {
        let mut out = BufWriter::new(File::create(path).with_context(|| format!("criando {}", path.display()))?);
        let (fmt, bps): (u16, u16) = match bits {
            WavBits::Pcm16 => (1, 16),
            WavBits::Pcm24 => (1, 24),
            WavBits::Float32 => (3, 32),
        };
        let block = 2 * bps / 8;
        out.write_all(b"RIFF")?;
        out.write_all(&0u32.to_le_bytes())?;
        out.write_all(b"WAVEfmt ")?;
        out.write_all(&16u32.to_le_bytes())?;
        out.write_all(&fmt.to_le_bytes())?;
        out.write_all(&2u16.to_le_bytes())?;
        out.write_all(&sr.to_le_bytes())?;
        out.write_all(&(sr * block as u32).to_le_bytes())?;
        out.write_all(&block.to_le_bytes())?;
        out.write_all(&bps.to_le_bytes())?;
        out.write_all(b"data")?;
        out.write_all(&0u32.to_le_bytes())?;
        Ok(WavWriter { out, bits, data_bytes: 0 })
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
        f.seek(SeekFrom::Start(4))?;
        f.write_all(&((36 + self.data_bytes) as u32).to_le_bytes())?;
        f.seek(SeekFrom::Start(40))?;
        f.write_all(&(self.data_bytes as u32).to_le_bytes())?;
        Ok(())
    }
}

fn to_i16(s: &[f32]) -> Vec<i16> {
    s.iter().map(|v| (v.clamp(-1.0, 1.0) * 32767.0).round() as i16).collect()
}

fn run(p: &Project, media: &MediaCache, s: &RenderSettings, prog: &RenderProgress) -> Result<()> {
    if s.end <= s.start {
        bail!("intervalo de renderização vazio");
    }
    let fps = p.settings.fps;
    let sr = p.settings.sample_rate;
    let n_frames = ((s.end - s.start) * fps).round().max(1.0) as u64;
    let (w, h) = (s.width.max(2) as usize, s.height.max(2) as usize);
    let mut mixer = Mixer::new(sr);
    let sample_at = |i: u64| ((s.start + i as f64 / fps) * sr as f64).round() as i64;

    if s.format == RenderFormat::Wav {
        let total = (sample_at(n_frames) - sample_at(0)).max(0) as u64;
        prog.total.store(total, Ordering::Relaxed);
        let mut wav = WavWriter::create(&s.path, sr, s.wav_bits)?;
        let mut pos = sample_at(0);
        let end = sample_at(n_frames);
        let mut buf = vec![0f32; 8192 * 2];
        let am = media.audio_map();
        while pos < end {
            if prog.cancel.load(Ordering::Relaxed) {
                bail!("cancelado");
            }
            let n = ((end - pos) as usize).min(8192);
            mixer.process(p, &am, pos, &mut buf[..n * 2]);
            wav.write(&buf[..n * 2])?;
            pos += n as i64;
            prog.done.store((pos - sample_at(0)) as u64, Ordering::Relaxed);
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
        RenderFormat::AviMjpeg => Sink::Avi(AviWriter::create(&s.path, w as u32, h as u32, fps, if has_audio { Some(sr) } else { None })?),
        RenderFormat::PngSequence => {
            let dir = s.path.parent().map(|d| d.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
            let stem = s.path.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| "quadro".into());
            Sink::Png(dir, stem)
        }
        RenderFormat::Y4m => Sink::Y4m(Y4mWriter::create(&s.path, w as u32, h as u32, fps)?),
        RenderFormat::Wav => unreachable!(),
    };
    // WAV separado para formatos sem áudio embutido
    let mut side_wav = if has_audio && s.format != RenderFormat::AviMjpeg {
        Some(WavWriter::create(&s.path.with_extension("wav"), sr, s.wav_bits)?)
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
                let t = s.start + fi as f64 / fps;
                let f = render_frame(p, media, t, w, h);
                match s.format {
                    RenderFormat::AviMjpeg => {
                        let rgb = f.to_rgb8_opaque();
                        let mut jpg = Vec::with_capacity(w * h / 4);
                        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, quality).encode(
                            &rgb,
                            w as u32,
                            h as u32,
                            image::ExtendedColorType::Rgb8,
                        )?;
                        Ok(jpg)
                    }
                    RenderFormat::PngSequence => {
                        let rgb = f.to_rgb8_opaque();
                        let mut png = Vec::new();
                        image::codecs::png::PngEncoder::new(&mut png).write_image_compat(&rgb, w as u32, h as u32)?;
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
pub fn save_snapshot(p: &Project, media: &MediaCache, t: f64, path: &Path) -> Result<()> {
    let f = render_frame(p, media, t, p.settings.width as usize, p.settings.height as usize);
    f.to_image().save(path)?;
    Ok(())
}
