//! Decodificação de áudio em Rust puro sob licenças da lista SPDX permitida
//! (`AGENTS.md`): WAVE/BWF/RF64/AIFF/AU/CAF/Wave64/VOC/8SVX/QOA próprios,
//! FLAC (`claxon`, Apache-2.0), Ogg Vorbis (`lewton`, MIT OR Apache-2.0) e
//! MP3 (`puremp3`, MIT OR CC0-1.0), além de `AAC`/`M4A`/`WMA` via API nativa
//! do sistema operacional (`media::mf` no Windows).

use anyhow::{anyhow, bail, Result};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// Áudio decodificado em memória: estéreo intercalado f32.
pub struct AudioData {
    pub sample_rate: u32,
    pub data: Vec<f32>,
    /// Picos (mín, máx) mono por bloco de `PEAK_BLOCK` amostras, para desenhar
    /// a forma de onda.
    pub peaks: Vec<(f32, f32)>,
    /// Picos (mín, máx) de cada canal (L, R) pelos mesmos blocos, para
    /// desenhar os canais de um evento estéreo separados.
    pub peaks_lr: Vec<[(f32, f32); 2]>,
    /// A fonte tinha um canal só (L e R são a mesma amostra).
    pub mono_source: bool,
}

/// Canal desenhado na forma de onda.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaveChannel {
    /// Média de L e R.
    Mix,
    Left,
    Right,
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
            peaks_lr: vec![],
            mono_source: false,
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
        let mut a = Self::from_stereo(sample_rate, data);
        a.mono_source = ch == 1;
        a
    }

    fn compute_peaks(&mut self) {
        let frames = self.frames();
        let mut peaks = Vec::with_capacity(frames / PEAK_BLOCK + 1);
        let mut peaks_lr = Vec::with_capacity(frames / PEAK_BLOCK + 1);
        for block in self.data.chunks(PEAK_BLOCK * 2) {
            let (mut lo, mut hi) = (0f32, 0f32);
            let mut lr = [(0f32, 0f32); 2];
            let (frames, _) = block.as_chunks::<2>();
            for &[l, r] in frames {
                let v = (l + r) * 0.5;
                lo = lo.min(v);
                hi = hi.max(v);
                lr[0] = (lr[0].0.min(l), lr[0].1.max(l));
                lr[1] = (lr[1].0.min(r), lr[1].1.max(r));
            }
            peaks.push((lo, hi));
            peaks_lr.push(lr);
        }
        self.peaks = peaks;
        self.peaks_lr = peaks_lr;
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

    /// Amostra do canal `ch` no frame `f` (0 fora do áudio).
    fn value(&self, ch: WaveChannel, f: usize) -> f32 {
        let l = self.data.get(f * 2).copied().unwrap_or(0.0);
        let r = self.data.get(f * 2 + 1).copied().unwrap_or(0.0);
        match ch {
            WaveChannel::Mix => (l + r) * 0.5,
            WaveChannel::Left => l,
            WaveChannel::Right => r,
        }
    }

    /// Valor do canal `ch` no tempo exato `t`, interpolado (para desenhar a
    /// forma de onda amostra a amostra com zoom alto).
    pub fn sample_channel(&self, ch: WaveChannel, t: crate::time::Ticks) -> f32 {
        let (l, r) = self.sample(t);
        match ch {
            WaveChannel::Mix => (l + r) * 0.5,
            WaveChannel::Left => l,
            WaveChannel::Right => r,
        }
    }

    /// Pico (mín, máx) do canal `ch` entre t0..t1 (ticks). Trechos curtos (menos
    /// de `PEAK_BLOCK` amostras) são lidos das amostras, para o zoom alto não
    /// virar degraus; trechos longos usam a tabela de picos, incluindo o bloco
    /// parcial do fim. O intervalo sempre inclui o zero.
    pub fn channel_range(
        &self,
        ch: WaveChannel,
        t0: crate::time::Ticks,
        t1: crate::time::Ticks,
    ) -> (f32, f32) {
        let n = self.frames() as i64;
        let s0 = t0.to_samples_floor(self.sample_rate).max(0);
        let s1 = t1
            .to_samples_floor(self.sample_rate)
            .max(s0.saturating_add(1))
            .min(n);
        let (mut lo, mut hi) = (0f32, 0f32);
        if s0 >= n {
            return (lo, hi);
        }
        let block = PEAK_BLOCK as i64;
        if s1 - s0 < block {
            for f in s0..s1 {
                let v = self.value(ch, f as usize);
                lo = lo.min(v);
                hi = hi.max(v);
            }
            return (lo, hi);
        }
        let a = (s0 / block) as usize;
        let b = (s1.saturating_add(block - 1) / block) as usize;
        for i in a..b {
            let p = match ch {
                WaveChannel::Mix => self.peaks.get(i).copied(),
                WaveChannel::Left => self.peaks_lr.get(i).map(|p| p[0]),
                WaveChannel::Right => self.peaks_lr.get(i).map(|p| p[1]),
            };
            let Some((l, h)) = p else {
                break;
            };
            lo = lo.min(l);
            hi = hi.max(h);
        }
        (lo, hi)
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
        "wav" | "wave" | "bwf" | "rf64" | "aif" | "aiff" | "aifc" => decode_wav_aiff(path),
        "au" | "snd" | "caf" | "w64" | "voc" | "8svx" | "svx" | "qoa" => {
            crate::util::guard("decodificação de áudio", || {
                decode_extra_audio(path, &ext)
            })
            .map_err(|e| anyhow!("{e}"))?
        }
        "flac" => crate::util::guard("decodificação FLAC", || decode_flac_raw(path))
            .map_err(|e| anyhow!("{e}"))?,
        "ogg" | "oga" => {
            crate::util::guard("decodificação Ogg Vorbis", || decode_vorbis_raw(path))
                .map_err(|e| anyhow!("{e}"))?
        }
        "mp3" => crate::util::guard("decodificação MP3", || decode_mp3_raw(path))
            .map_err(|e| anyhow!("{e}"))?,
        "aac" | "m4a" | "m4b" | "wma" | "mp2" | "mpa" | "alac" => decode_aac_or_m4a(path),
        _ => bail!("formato de áudio não suportado: .{ext}"),
    }
}

fn decode_extra_audio(path: &Path, ext: &str) -> Result<AudioData> {
    let len = std::fs::metadata(path)?.len();
    if len > MAX_FILE_BYTES {
        bail!("arquivo de áudio grande demais ({len} bytes)");
    }
    let bytes = std::fs::read(path)?;
    let pcm = super::audio_extra::decode_bytes(ext, &bytes, MAX_SAMPLES)?;
    finalize_audio(pcm.sample_rate, pcm.channels as usize, pcm.samples)
}

fn decode_aac_or_m4a(path: &Path) -> Result<AudioData> {
    let loaded = super::system::open_media(path)?;
    loaded
        .audio
        .ok_or_else(|| anyhow!("{}: nenhuma faixa de áudio encontrada", loaded.backend))
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

fn sanitize_sample(v: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
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
    use std::process::Command;

    #[test]
    fn wav_decodifica_pcm_e_rejeita_vazio() {
        let p = std::env::temp_dir().join(format!("neoveg_wav_{}.wav", std::process::id()));
        let mut w =
            crate::render::WavWriter::create(&p, 8000, crate::render::WavBits::Pcm16).unwrap();
        let samples: Vec<f32> = (0..1600).map(|i| (i as f32 * 0.1).sin() * 0.4).collect();
        w.write(&samples).unwrap();
        w.finish().unwrap();

        let audio = decode_file(&p).expect("leitor próprio deveria decodificar WAV PCM");
        assert_eq!(audio.sample_rate, 8000);
        assert_eq!(audio.frames(), 800);
        assert!(!audio.peaks.is_empty());

        std::fs::write(&p, b"").unwrap();
        assert!(decode_file(&p).is_err());
        let _ = std::fs::remove_file(&p);
    }

    fn ffprobe_oracle_sr_dur(path: &Path) -> Option<(u32, f64)> {
        let out = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "a:0",
                "-show_entries",
                "stream=sample_rate:format=duration",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
            ])
            .arg(path)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let txt = String::from_utf8_lossy(&out.stdout);
        let mut lines = txt.lines().map(str::trim).filter(|s| !s.is_empty());
        let sr = lines.next()?.parse::<u32>().ok()?;
        let dur = lines.next()?.parse::<f64>().ok()?;
        Some((sr, dur))
    }

    #[test]
    fn fixtures_e_oraculo_externo_ffprobe() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("target")
            .join("fixtures");
        let mut names = vec![
            "oracle_wav.wav",
            "oracle_flac.flac",
            "oracle_ogg.ogg",
            "oracle_mp3.mp3",
            "oracle_au.au",
            "oracle_caf.caf",
            "oracle_w64.w64",
            "oracle_adpcm.wav",
            "oracle_alaw.wav",
        ];
        if cfg!(windows) {
            names.push("oracle_aac.m4a");
            names.push("oracle_aac.aac");
        }
        for name in names {
            let p = dir.join(name);
            if !p.exists() {
                continue;
            }
            let a = decode_file(&p)
                .unwrap_or_else(|e| panic!("falha ao decodificar fixture {name}: {e}"));
            assert!(a.sample_rate >= 8000, "{name}: taxa inválida");
            assert!(a.frames() > 1000, "{name}: poucos quadros");
            assert!(!a.peaks.is_empty(), "{name}: picos ausentes");
            if let Some((oracle_sr, oracle_dur)) = ffprobe_oracle_sr_dur(&p) {
                assert_eq!(a.sample_rate, oracle_sr, "{name}: taxa diverge do ffprobe");
                let decoded_dur = a.duration().as_secs_f64();
                assert!(
                    (decoded_dur - oracle_dur).abs() <= 0.15,
                    "{name}: duração decodificada ({decoded_dur:.3}s) diverge do oráculo ({oracle_dur:.3}s)"
                );
            }
        }
    }

    fn tone(frames: usize, f: impl Fn(usize) -> (f32, f32)) -> AudioData {
        let mut d = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let (l, r) = f(i);
            d.push(l);
            d.push(r);
        }
        AudioData::from_stereo(48_000, d)
    }

    fn t(samples: i64) -> crate::time::Ticks {
        crate::time::Ticks::from_samples(samples, 48_000)
    }

    #[test]
    fn canais_separados_na_forma_de_onda() {
        // L só positivo, R só negativo: a mistura esconde os dois
        let a = tone(4096, |_| (0.5, -0.5));
        assert_eq!(
            a.channel_range(WaveChannel::Left, t(0), t(4096)),
            (0.0, 0.5)
        );
        assert_eq!(
            a.channel_range(WaveChannel::Right, t(0), t(4096)),
            (-0.5, 0.0)
        );
        assert_eq!(a.channel_range(WaveChannel::Mix, t(0), t(4096)), (0.0, 0.0));
        assert!(!a.mono_source);
        let m = AudioData::from_interleaved(48_000, 1, &[0.25; 512]);
        assert!(m.mono_source);
    }

    #[test]
    fn zoom_alto_le_as_amostras_e_nao_o_bloco() {
        // um pico isolado na amostra 300 (bloco 1): fora dele, nada
        let a = tone(1024, |i| if i == 300 { (0.9, 0.9) } else { (0.0, 0.0) });
        assert_eq!(
            a.channel_range(WaveChannel::Mix, t(256), t(290)),
            (0.0, 0.0)
        );
        assert_eq!(
            a.channel_range(WaveChannel::Mix, t(290), t(310)),
            (0.0, 0.9)
        );
        // com o trecho longo, a tabela de picos acha o mesmo pico
        assert_eq!(
            a.channel_range(WaveChannel::Left, t(0), t(1024)),
            (0.0, 0.9)
        );
    }

    #[test]
    fn bloco_parcial_do_fim_entra_e_fora_do_audio_e_zero() {
        // pico na última amostra, num bloco parcial
        let a = tone(600, |i| if i == 599 { (-0.7, 0.3) } else { (0.0, 0.0) });
        assert_eq!(
            a.channel_range(WaveChannel::Left, t(0), t(600)),
            (-0.7, 0.0)
        );
        assert_eq!(
            a.channel_range(WaveChannel::Right, t(0), t(600)),
            (0.0, 0.3)
        );
        assert_eq!(
            a.channel_range(WaveChannel::Mix, t(5000), t(9000)),
            (0.0, 0.0)
        );
        // intervalo invertido ou negativo não entra em pânico
        let _ = a.channel_range(WaveChannel::Mix, t(500), t(-10));
        let _ = a.channel_range(WaveChannel::Mix, t(-500), t(-10));
        assert_eq!(a.sample_channel(WaveChannel::Left, t(599)), -0.7);
        assert_eq!(a.sample_channel(WaveChannel::Right, t(599)), 0.3);
    }
}
