//! Decodificação de áudio em Rust puro: Symphonia (AAC, ALAC, ADPCM, CAF,
//! FLAC, MP1/MP2/MP3, PCM, Vorbis em ISOMP4/MKV/Ogg/RIFF — exceção `MPL-2.0`
//! documentada em `docs/licencas-de-dependencias.md`), leitor WAVE/AIFF próprio,
//! fallbacks `claxon`/`lewton`/`puremp3` e Windows Media Foundation (`media::mf`).

use anyhow::{anyhow, bail, Result};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoderOptions};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// Áudio decodificado em memória: estéreo intercalado f32.
pub struct AudioData {
    pub sample_rate: u32,
    pub data: Vec<f32>,
    /// Picos (mín, máx) mono por bloco de `PEAK_BLOCK` amostras, para desenhar
    /// a forma de onda.
    pub peaks: Vec<(f32, f32)>,
}

pub const PEAK_BLOCK: usize = 256;

/// Limite de amostras decodificadas em memória (~2 h estéreo a 48 kHz),
/// para que um arquivo hostil não esgote a memória (§0.4).
pub const MAX_SAMPLES: usize = 2 * 48_000 * 60 * 120;

impl AudioData {
    pub fn from_stereo(sample_rate: u32, mut data: Vec<f32>) -> Self {
        let sample_rate = sample_rate.max(1);
        data.truncate(data.len() & !1);
        let mut a = AudioData {
            sample_rate,
            data,
            peaks: vec![],
        };
        a.compute_peaks();
        a
    }

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
        Self::from_stereo(sample_rate, data)
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

    pub fn duration(&self) -> crate::time::Ticks {
        crate::time::Ticks::from_samples(self.frames() as i64, self.sample_rate)
    }

    /// Amostra (L, R) no tempo exato `t` (ticks), com interpolação linear.
    #[inline]
    pub fn sample(&self, t: crate::time::Ticks) -> (f32, f32) {
        let (pos, fr) = t.to_sample_pos(self.sample_rate);
        if pos < 0 {
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
        let l = self.data[i * 2] + (self.data[i * 2 + 2] - self.data[i * 2]) * fr;
        let r = self.data[i * 2 + 1] + (self.data[i * 2 + 3] - self.data[i * 2 + 1]) * fr;
        (l, r)
    }

    /// Pico (mín, máx) entre os tempos t0..t1 (ticks), usando a tabela de picos.
    pub fn peak_range(&self, t0: crate::time::Ticks, t1: crate::time::Ticks) -> (f32, f32) {
        let s0 = t0.to_samples_floor(self.sample_rate);
        let s1 = t1.to_samples_floor(self.sample_rate);
        let a = (s0 / PEAK_BLOCK as i64).max(0) as usize;
        let b = ((s1 / PEAK_BLOCK as i64).max(0) as usize).max(a + 1);
        let (mut lo, mut hi) = (0f32, 0f32);
        for p in self.peaks.iter().take(b.min(self.peaks.len())).skip(a) {
            lo = lo.min(p.0);
            hi = hi.max(p.1);
        }
        (lo, hi)
    }
}

fn finalize_audio(sr: u32, ch: usize, samples: Vec<f32>) -> Result<AudioData> {
    if samples.is_empty() {
        return Err(anyhow!("nenhuma amostra de áudio decodificada"));
    }
    if sr == 0 || sr > 1_000_000 {
        return Err(anyhow!("taxa de amostragem inválida"));
    }
    if samples.len() >= MAX_SAMPLES {
        crate::util::log_warn("áudio truncado: arquivo excede o limite de duração em memória");
    }
    if ch == 2 {
        Ok(AudioData::from_stereo(sr, samples))
    } else {
        Ok(AudioData::from_interleaved(sr, ch, &samples))
    }
}

pub fn decode_file(path: &Path) -> Result<AudioData> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "wav" | "wave" | "aif" | "aiff" | "aifc" => {
            decode_wav_aiff(path).or_else(|_| decode_symphonia(path))
        }
        "flac" => decode_symphonia(path).or_else(|_| decode_flac_raw(path)),
        "ogg" | "oga" => decode_symphonia(path).or_else(|_| decode_vorbis_raw(path)),
        "mp3" => decode_symphonia(path).or_else(|_| decode_mp3_raw(path)),
        "mp1" | "mp2" | "caf" | "mka" => decode_symphonia(path),
        "aac" | "m4a" => decode_aac_or_m4a(path),
        _ => bail!("formato de áudio não suportado: .{ext}"),
    }
}

fn decode_aac_or_m4a(path: &Path) -> Result<AudioData> {
    #[cfg(windows)]
    {
        decode_symphonia(path).or_else(|_| decode_via_mf(path))
    }
    #[cfg(not(windows))]
    {
        decode_symphonia(path)
    }
}

#[cfg(windows)]
fn decode_via_mf(path: &Path) -> Result<AudioData> {
    let loaded = super::mf::open_media(path)?;
    loaded
        .audio
        .ok_or_else(|| anyhow!("Media Foundation: nenhuma faixa de áudio encontrada"))
}

fn decode_wav_aiff(path: &Path) -> Result<AudioData> {
    let len = std::fs::metadata(path)?.len();
    if len > MAX_FILE_BYTES {
        bail!("arquivo de áudio grande demais ({len} bytes)");
    }
    let bytes = std::fs::read(path)?;
    let pcm = super::wav::decode(&bytes, MAX_SAMPLES)?;
    finalize_audio(pcm.sample_rate, pcm.channels as usize, pcm.samples)
}

pub(crate) fn decode_symphonia(path: &Path) -> Result<AudioData> {
    let (sr, stereo) =
        crate::util::guard("decodificação Symphonia", || decode_symphonia_inner(path))
            .map_err(|e| anyhow!("{e}"))??;
    finalize_audio(sr, 2, stereo)
}

fn sanitize_sample(v: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

fn push_stereo_chunk(stereo_out: &mut Vec<f32>, interleaved: &[f32], channels: usize) -> bool {
    let ch = channels.max(1);
    for frame in interleaved.chunks_exact(ch) {
        if stereo_out.len() + 2 > MAX_SAMPLES {
            return true;
        }
        let l = sanitize_sample(frame[0]);
        let r = if ch >= 2 {
            sanitize_sample(frame[1])
        } else {
            l
        };
        stereo_out.push(l);
        stereo_out.push(r);
    }
    false
}

fn select_audio_track(reader: &dyn FormatReader) -> Option<(u32, AudioCodecParameters)> {
    let track = reader.default_track(TrackType::Audio).or_else(|| {
        reader
            .tracks()
            .iter()
            .find(|t| matches!(t.codec_params, Some(CodecParameters::Audio(_))))
    })?;
    let Some(CodecParameters::Audio(audio)) = track.codec_params.as_ref() else {
        return None;
    };
    Some((track.id, audio.clone()))
}

fn decode_symphonia_inner(path: &Path) -> Result<(u32, Vec<f32>)> {
    const MAX_RESET_LOOPS: usize = 8;
    const MAX_DECODE_ERRORS: usize = 64;
    const MAX_CONSECUTIVE_ERRORS: usize = 16;

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if !ext.is_empty() {
            hint.with_extension(ext);
        }
    }

    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut reader = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| anyhow!("Symphonia: formato não reconhecido: {e}"))?;

    let dec_opts = AudioDecoderOptions::default().gapless(true);
    let (mut track_id, initial_params) = select_audio_track(reader.as_ref())
        .ok_or_else(|| anyhow!("Symphonia: nenhuma faixa de áudio encontrada"))?;
    let mut sample_rate = initial_params.sample_rate;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&initial_params, &dec_opts)
        .map_err(|e| anyhow!("Symphonia: decodificador indisponível: {e}"))?;

    let mut stereo_out = Vec::new();
    let mut scratch = Vec::new();
    let mut resets = 0usize;
    let mut decode_errs = 0usize;
    let mut consec_errs = 0usize;

    loop {
        let packet = match reader.next_packet() {
            Ok(Some(pkt)) => pkt,
            Ok(None) => break,
            Err(SymError::ResetRequired) => {
                resets += 1;
                if resets > MAX_RESET_LOOPS {
                    break;
                }
                let Some((id, params)) = select_audio_track(reader.as_ref()) else {
                    break;
                };
                track_id = id;
                if sample_rate.unwrap_or(0) == 0 {
                    sample_rate = params.sample_rate;
                }
                decoder = symphonia::default::get_codecs()
                    .make_audio_decoder(&params, &dec_opts)
                    .map_err(|e| anyhow!("Symphonia: falha ao reiniciar decodificador: {e}"))?;
                continue;
            }
            Err(SymError::IoError(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                if stereo_out.is_empty() {
                    return Err(anyhow!("Symphonia: fim inesperado de arquivo"));
                }
                break;
            }
            Err(e) => {
                if stereo_out.is_empty() {
                    return Err(anyhow!("Symphonia: erro ao ler pacote: {e}"));
                }
                break;
            }
        };

        if packet.track_id != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                consec_errs = 0;
                let spec = decoded.spec();
                if spec.rate() > 0 && sample_rate.unwrap_or(0) == 0 {
                    sample_rate = Some(spec.rate());
                }
                let ch = spec.channels().count();
                let frames = decoded.frames();
                if ch == 0 || frames == 0 {
                    continue;
                }
                if frames.saturating_mul(ch) > MAX_SAMPLES.saturating_mul(4) {
                    bail!("Symphonia: quadro de áudio grande demais ({frames} quadros)");
                }
                decoded.copy_to_vec_interleaved(&mut scratch);
                if push_stereo_chunk(&mut stereo_out, &scratch, ch) {
                    break;
                }
            }
            Err(SymError::DecodeError(_)) => {
                decode_errs += 1;
                consec_errs += 1;
                if decode_errs > MAX_DECODE_ERRORS || consec_errs >= MAX_CONSECUTIVE_ERRORS {
                    if stereo_out.is_empty() {
                        bail!("Symphonia: excesso de erros de decodificação");
                    }
                    break;
                }
            }
            Err(SymError::ResetRequired) => {
                resets += 1;
                if resets > MAX_RESET_LOOPS {
                    break;
                }
                let Some((id, params)) = select_audio_track(reader.as_ref()) else {
                    break;
                };
                track_id = id;
                if sample_rate.unwrap_or(0) == 0 {
                    sample_rate = params.sample_rate;
                }
                decoder = symphonia::default::get_codecs()
                    .make_audio_decoder(&params, &dec_opts)
                    .map_err(|e| anyhow!("Symphonia: falha ao reiniciar decodificador: {e}"))?;
            }
            Err(e) => {
                if stereo_out.is_empty() {
                    return Err(anyhow!("Symphonia: erro na decodificação: {e}"));
                }
                break;
            }
        }
    }

    let _ = decoder.finalize();
    Ok((sample_rate.unwrap_or(0), stereo_out))
}

/// Maior arquivo WAVE/AIFF lido inteiro para a memória.
const MAX_FILE_BYTES: u64 = 4 << 30;

fn decode_flac_raw(path: &Path) -> Result<AudioData> {
    let mut r = claxon::FlacReader::open(path).map_err(|e| anyhow!("FLAC inválido: {e}"))?;
    let info = r.streaminfo();
    if info.bits_per_sample == 0 || info.bits_per_sample > 32 || info.channels == 0 {
        bail!("FLAC com formato inválido");
    }
    let scale = 1.0 / (1u64 << (info.bits_per_sample - 1)) as f32;
    let mut out = Vec::new();
    for s in r.samples() {
        let Ok(s) = s else { break };
        out.push(s as f32 * scale);
        if out.len() >= MAX_SAMPLES {
            break;
        }
    }
    finalize_audio(info.sample_rate, info.channels as usize, out)
}

fn decode_vorbis_raw(path: &Path) -> Result<AudioData> {
    let f = BufReader::new(File::open(path)?);
    let mut r = lewton::inside_ogg::OggStreamReader::new(f)
        .map_err(|e| anyhow!("Ogg Vorbis inválido: {e}"))?;
    let sr = r.ident_hdr.audio_sample_rate;
    let ch = r.ident_hdr.audio_channels as usize;
    let mut out = Vec::new();
    while let Ok(Some(pkt)) = r.read_dec_packet_itl() {
        out.extend(pkt.iter().map(|&v| v as f32 / 32768.0));
        if out.len() >= MAX_SAMPLES {
            break;
        }
    }
    finalize_audio(sr, ch.max(1), out)
}

fn decode_mp3_raw(path: &Path) -> Result<AudioData> {
    let f = BufReader::new(File::open(path)?);
    let (header, samples) = puremp3::read_mp3(f).map_err(|e| anyhow!("MP3 inválido: {e:?}"))?;
    let sr = header.sample_rate.hz();
    // o decodificador sempre entrega pares (esquerdo, direito)
    let mut out = Vec::new();
    for (l, r) in samples {
        out.push(sanitize_sample(l));
        out.push(sanitize_sample(r));
        if out.len() >= MAX_SAMPLES {
            break;
        }
    }
    finalize_audio(sr, 2, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn symphonia_decodifica_wav_e_rejeita_vazio() {
        let p = std::env::temp_dir().join(format!("neoveg_sym_{}.wav", std::process::id()));
        let mut w =
            crate::render::WavWriter::create(&p, 8000, crate::render::WavBits::Pcm16).unwrap();
        let samples: Vec<f32> = (0..1600).map(|i| (i as f32 * 0.1).sin() * 0.4).collect();
        w.write(&samples).unwrap();
        w.finish().unwrap();

        let audio = decode_symphonia(&p).expect("Symphonia deveria decodificar WAV PCM");
        assert_eq!(audio.sample_rate, 8000);
        assert_eq!(audio.frames(), 800);
        assert!(!audio.peaks.is_empty());

        std::fs::write(&p, b"").unwrap();
        assert!(decode_symphonia(&p).is_err());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn symphonia_fixtures_opcionais_reais() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("target")
            .join("fixtures");
        for name in [
            "sym_aac.aac",
            "sym_aac.m4a",
            "sym_alac.m4a",
            "sym_adpcm.wav",
            "sym_caf.caf",
            "sym_flac.flac",
            "sym_ogg.ogg",
            "sym_mp3.mp3",
            "sym_mka.mka",
        ] {
            let p = dir.join(name);
            if p.exists() {
                let a = decode_file(&p)
                    .unwrap_or_else(|e| panic!("falha ao decodificar fixture {name}: {e}"));
                assert!(a.sample_rate >= 8000, "{name}: taxa inválida");
                assert!(a.frames() > 1000, "{name}: poucos quadros");
                assert!(!a.peaks.is_empty(), "{name}: picos ausentes");
            }
        }
    }
}
