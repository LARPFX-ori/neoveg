//! Decodificação de áudio com Symphonia (WAV, MP3, FLAC, OGG/Vorbis, AAC/M4A,
//! ALAC, AIFF, CAF, MKV/WebM-áudio) — tudo em Rust puro.

use anyhow::{anyhow, Result};
use std::fs::File;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Áudio decodificado em memória: estéreo intercalado f32.
pub struct AudioData {
    pub sample_rate: u32,
    pub data: Vec<f32>,
    /// Picos (mín, máx) mono por bloco de `PEAK_BLOCK` amostras, para desenhar
    /// a forma de onda.
    pub peaks: Vec<(f32, f32)>,
}

pub const PEAK_BLOCK: usize = 256;

impl AudioData {
    pub fn from_interleaved(sample_rate: u32, channels: usize, samples: &[f32]) -> Self {
        let ch = channels.max(1);
        let frames = samples.len() / ch;
        let mut data = Vec::with_capacity(frames * 2);
        for f in 0..frames {
            let l = samples[f * ch];
            let r = if ch >= 2 { samples[f * ch + 1] } else { l };
            data.push(l);
            data.push(r);
        }
        let mut a = AudioData {
            sample_rate,
            data,
            peaks: vec![],
        };
        a.compute_peaks();
        a
    }

    fn compute_peaks(&mut self) {
        let frames = self.frames();
        let mut peaks = Vec::with_capacity(frames / PEAK_BLOCK + 1);
        for b in (0..frames).step_by(PEAK_BLOCK) {
            let (mut lo, mut hi) = (0f32, 0f32);
            for f in b..(b + PEAK_BLOCK).min(frames) {
                let v = (self.data[f * 2] + self.data[f * 2 + 1]) * 0.5;
                lo = lo.min(v);
                hi = hi.max(v);
            }
            peaks.push((lo, hi));
        }
        self.peaks = peaks;
    }

    pub fn frames(&self) -> usize {
        self.data.len() / 2
    }

    pub fn duration(&self) -> f64 {
        self.frames() as f64 / self.sample_rate as f64
    }

    /// Amostra (L, R) no tempo `t` (segundos), com interpolação linear.
    #[inline]
    pub fn sample(&self, t: f64) -> (f32, f32) {
        let pos = t * self.sample_rate as f64;
        if pos < 0.0 {
            return (0.0, 0.0);
        }
        let i = pos as usize;
        let n = self.frames();
        if i + 1 >= n {
            if i < n {
                return (self.data[i * 2], self.data[i * 2 + 1]);
            }
            return (0.0, 0.0);
        }
        let fr = (pos - i as f64) as f32;
        let l = self.data[i * 2] + (self.data[i * 2 + 2] - self.data[i * 2]) * fr;
        let r = self.data[i * 2 + 1] + (self.data[i * 2 + 3] - self.data[i * 2 + 1]) * fr;
        (l, r)
    }

    /// Pico (mín, máx) entre os tempos t0..t1, usando a tabela de picos.
    pub fn peak_range(&self, t0: f64, t1: f64) -> (f32, f32) {
        let sr = self.sample_rate as f64;
        let a = ((t0 * sr) as isize / PEAK_BLOCK as isize).max(0) as usize;
        let b = (((t1 * sr) as isize / PEAK_BLOCK as isize).max(0) as usize).max(a + 1);
        let (mut lo, mut hi) = (0f32, 0f32);
        for p in self.peaks.iter().take(b.min(self.peaks.len())).skip(a) {
            lo = lo.min(p.0);
            hi = hi.max(p.1);
        }
        (lo, hi)
    }
}

pub fn decode_file(path: &Path) -> Result<AudioData> {
    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some())
        .ok_or_else(|| anyhow!("nenhuma faixa de áudio suportada"))?;
    let track_id = track.id;
    let mut sr = track.codec_params.sample_rate.unwrap_or(44100);
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    let mut out: Vec<f32> = Vec::new();
    let mut channels = 0usize;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(_)) => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => {
                if out.is_empty() {
                    return Err(e.into());
                }
                break;
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                let spec = *buf.spec();
                sr = spec.rate;
                let ch = spec.channels.count().max(1);
                let mut sb = SampleBuffer::<f32>::new(buf.capacity() as u64, spec);
                sb.copy_interleaved_ref(buf);
                let s = sb.samples();
                if channels == 0 {
                    channels = ch;
                }
                // normaliza para estéreo já aqui (canais podem variar raramente)
                let frames = s.len() / ch;
                for f in 0..frames {
                    let l = s[f * ch];
                    let r = if ch >= 2 { s[f * ch + 1] } else { l };
                    out.push(l);
                    out.push(r);
                }
            }
            Err(SymError::DecodeError(_)) => continue,
            Err(SymError::IoError(_)) => break,
            Err(e) => {
                if out.is_empty() {
                    return Err(e.into());
                }
                break;
            }
        }
    }
    if out.is_empty() {
        return Err(anyhow!("nenhuma amostra de áudio decodificada"));
    }
    Ok(AudioData::from_interleaved(sr, 2, &out))
}
