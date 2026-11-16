//! Medição de Loudness em conformidade com EBU R128 / ITU-R BS.1770-4
//! (`ebur128`) e cálculo de RMS em janela deslizante via `dasp`.
//!
//! Executa tanto em tempo real na saída do Master Bus quanto em varredura
//! offline sobre o projeto ou seleção de tempo (`AGENTS.md` §0, §3.6).

use super::audio::Mixer;
use crate::media::MediaCache;
use crate::model::{Project, Ticks};
use dasp::ring_buffer;
use dasp::rms::Rms;
use ebur128::{EbuR128, Mode};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const MIN_METER_SR: u32 = 8_000;
const MAX_METER_SR: u32 = 192_000;
const ANALYZE_CHUNK_FRAMES: usize = 4_096;
/// Limite de segurança para varreduras offline (4 horas em ticks exatos).
const MAX_SCAN_TICKS: Ticks = Ticks::from_raw(4 * 3600 * crate::time::TICKS_PER_SEC);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LoudnessTarget {
    #[default]
    EbuR128,
    Streaming,
    AtscA85,
    Podcast,
}

impl LoudnessTarget {
    pub const ALL: [LoudnessTarget; 4] = [
        LoudnessTarget::EbuR128,
        LoudnessTarget::Streaming,
        LoudnessTarget::AtscA85,
        LoudnessTarget::Podcast,
    ];

    pub fn name(self) -> &'static str {
        match self {
            LoudnessTarget::EbuR128 => "EBU R128 (-23 LUFS)",
            LoudnessTarget::Streaming => "Streaming / Web (-14 LUFS)",
            LoudnessTarget::AtscA85 => "ATSC A/85 (-24 LUFS)",
            LoudnessTarget::Podcast => "Podcast Estéreo (-16 LUFS)",
        }
    }

    pub fn target_lufs(self) -> f64 {
        match self {
            LoudnessTarget::EbuR128 => -23.0,
            LoudnessTarget::Streaming => -14.0,
            LoudnessTarget::AtscA85 => -24.0,
            LoudnessTarget::Podcast => -16.0,
        }
    }

    pub fn max_true_peak_dbtp(self) -> f64 {
        match self {
            LoudnessTarget::EbuR128 | LoudnessTarget::Streaming | LoudnessTarget::Podcast => -1.0,
            LoudnessTarget::AtscA85 => -2.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoudnessSnapshot {
    pub momentary_lufs: f64,
    pub short_term_lufs: f64,
    pub integrated_lufs: f64,
    pub loudness_range_lu: f64,
    pub true_peak_dbtp: [f64; 2],
    pub rms_dbfs: [f64; 2],
    pub frames_measured: u64,
    pub sample_rate: u32,
}

impl Default for LoudnessSnapshot {
    fn default() -> Self {
        Self {
            momentary_lufs: -f64::INFINITY,
            short_term_lufs: -f64::INFINITY,
            integrated_lufs: -f64::INFINITY,
            loudness_range_lu: 0.0,
            true_peak_dbtp: [-f64::INFINITY; 2],
            rms_dbfs: [-f64::INFINITY; 2],
            frames_measured: 0,
            sample_rate: 48_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoudnessReport {
    pub snapshot: LoudnessSnapshot,
    pub max_momentary_lufs: f64,
    pub max_short_term_lufs: f64,
    pub start: Ticks,
    pub end: Ticks,
}

pub fn format_lufs(val: f64) -> String {
    if val.is_finite() && val > -120.0 {
        format!("{val:+.1} LUFS")
    } else {
        "-inf LUFS".into()
    }
}

pub fn format_db(val: f64, unit: &str) -> String {
    if val.is_finite() && val > -120.0 {
        format!("{val:+.1} {unit}")
    } else {
        format!("-inf {unit}")
    }
}

#[inline]
fn lin_to_db_f64(v: f64) -> f64 {
    if !v.is_finite() || v <= 1e-12 {
        -f64::INFINITY
    } else {
        20.0 * v.log10()
    }
}

#[inline]
fn sanitize_sample(s: f32) -> f32 {
    if s.is_finite() {
        s.clamp(-16.0, 16.0)
    } else {
        0.0
    }
}

/// Analisador contínuo de Loudness EBU R128 + RMS estéreo (sem alocações por bloco no callback de áudio).
pub struct LoudnessAnalyzer {
    ebur: Option<EbuR128>,
    rms: Rms<[f32; 2], Vec<[f32; 2]>>,
    scratch: Vec<f32>,
    sample_rate: u32,
    frames_measured: u64,
}

impl LoudnessAnalyzer {
    pub fn try_new(sample_rate: u32) -> Result<Self, String> {
        let sr = sample_rate.clamp(MIN_METER_SR, MAX_METER_SR);
        let mode = Mode::M | Mode::S | Mode::I | Mode::LRA | Mode::TRUE_PEAK;
        let ebur = EbuR128::new(2, sr, mode)
            .map_err(|e| format!("falha ao criar medidor EBU R128: {e:?}"))?;
        let rms_win = ((sr as usize) / 4).clamp(512, 16_384);
        let rms = Rms::new(ring_buffer::Fixed::from(vec![[0.0_f32; 2]; rms_win]));
        Ok(Self {
            ebur: Some(ebur),
            rms,
            scratch: Vec::with_capacity(ANALYZE_CHUNK_FRAMES * 2),
            sample_rate: sr,
            frames_measured: 0,
        })
    }

    pub fn new(sample_rate: u32) -> Self {
        let sr = sample_rate.clamp(MIN_METER_SR, MAX_METER_SR);
        match Self::try_new(sr) {
            Ok(a) => a,
            Err(_) => {
                let rms = Rms::new(ring_buffer::Fixed::from(vec![[0.0_f32; 2]; 2048]));
                Self {
                    ebur: None,
                    rms,
                    scratch: Vec::new(),
                    sample_rate: sr,
                    frames_measured: 0,
                }
            }
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn reset(&mut self) {
        if let Some(ebur) = &mut self.ebur {
            ebur.reset();
        }
        self.rms.reset();
        self.frames_measured = 0;
    }

    pub fn push_interleaved(&mut self, interleaved_stereo: &[f32]) {
        let pairs = interleaved_stereo.len() / 2;
        if pairs == 0 {
            return;
        }
        let usable = pairs * 2;
        let mut offset = 0;
        while offset < usable {
            let chunk_len = (usable - offset).min(ANALYZE_CHUNK_FRAMES * 2);
            self.scratch.clear();
            for pair in interleaved_stereo[offset..offset + chunk_len]
                .as_chunks::<2>()
                .0
            {
                let l = sanitize_sample(pair[0]);
                let r = sanitize_sample(pair[1]);
                self.rms.next([l, r]);
                self.scratch.push(l);
                self.scratch.push(r);
            }
            if let Some(ebur) = &mut self.ebur {
                let _ = ebur.add_frames_f32(&self.scratch);
            }
            self.frames_measured = self.frames_measured.saturating_add((chunk_len / 2) as u64);
            offset += chunk_len;
        }
    }

    pub fn snapshot(&self) -> LoudnessSnapshot {
        let rms_lin = self.rms.current();
        let mut snap = LoudnessSnapshot {
            rms_dbfs: [
                lin_to_db_f64(rms_lin[0] as f64),
                lin_to_db_f64(rms_lin[1] as f64),
            ],
            frames_measured: self.frames_measured,
            sample_rate: self.sample_rate,
            ..LoudnessSnapshot::default()
        };
        if let Some(ebur) = &self.ebur {
            if let Ok(m) = ebur.loudness_momentary() {
                snap.momentary_lufs = m;
            }
            if let Ok(s) = ebur.loudness_shortterm() {
                snap.short_term_lufs = s;
            }
            if let Ok(i) = ebur.loudness_global() {
                snap.integrated_lufs = i;
            }
            if let Ok(lra) = ebur.loudness_range() {
                snap.loudness_range_lu = if lra.is_finite() { lra.max(0.0) } else { 0.0 };
            }
            for (ch, slot) in (0u32..).zip(snap.true_peak_dbtp.iter_mut()) {
                if let Ok(tp) = ebur.true_peak(ch) {
                    *slot = lin_to_db_f64(tp);
                }
            }
        }
        snap
    }
}

/// Calcula o relatório completo EBU R128 de todo o projeto (ou do trecho especificado).
pub fn analyze_project_loudness(
    project: &Project,
    media: &MediaCache,
    range: Option<(Ticks, Ticks)>,
    sample_rate: u32,
    cancel: &AtomicBool,
    progress_samples: Option<(&AtomicU64, &AtomicU64)>,
) -> Result<LoudnessReport, String> {
    let sr = sample_rate.clamp(MIN_METER_SR, MAX_METER_SR);
    let raw_start = range
        .map(|(a, _)| a.max(Ticks::ZERO))
        .unwrap_or(Ticks::ZERO);
    let raw_end = range
        .map(|(_, b)| b)
        .unwrap_or_else(|| project.duration())
        .max(raw_start);
    let capped_end = (raw_start + MAX_SCAN_TICKS).min(raw_end);
    if capped_end <= raw_start {
        return Err("o trecho para análise de loudness tem duração zero".into());
    }

    let start_sample =
        ((raw_start.0 as i128 * sr as i128) / crate::time::TICKS_PER_SEC as i128) as i64;
    let end_sample =
        ((capped_end.0 as i128 * sr as i128) / crate::time::TICKS_PER_SEC as i128) as i64;
    let total_samples = (end_sample - start_sample).max(1) as u64;
    if let Some((done_ref, total_ref)) = progress_samples {
        done_ref.store(0, Ordering::Relaxed);
        total_ref.store(total_samples, Ordering::Relaxed);
    }

    let audio_map = media.audio_map();

    let mut mixer = Mixer::new_offline(sr);
    let mut analyzer = LoudnessAnalyzer::try_new(sr)?;
    let mut buf = vec![0.0_f32; ANALYZE_CHUNK_FRAMES * 2];
    let mut max_m = -f64::INFINITY;
    let mut max_s = -f64::INFINITY;
    let mut pos = start_sample;
    let mut done = 0_u64;

    while pos < end_sample {
        if cancel.load(Ordering::Relaxed) {
            return Err("análise de loudness cancelada".into());
        }
        let n = (end_sample - pos).min(ANALYZE_CHUNK_FRAMES as i64) as usize;
        let slice = &mut buf[..n * 2];
        mixer.process(project, &audio_map, pos, slice);
        analyzer.push_interleaved(slice);
        let cur = analyzer.snapshot();
        if cur.momentary_lufs.is_finite() {
            max_m = max_m.max(cur.momentary_lufs);
        }
        if cur.short_term_lufs.is_finite() {
            max_s = max_s.max(cur.short_term_lufs);
        }
        pos = pos.saturating_add(n as i64);
        done = done.saturating_add(n as u64);
        if let Some((done_ref, _)) = progress_samples {
            done_ref.store(done, Ordering::Relaxed);
        }
    }

    Ok(LoudnessReport {
        snapshot: analyzer.snapshot(),
        max_momentary_lufs: max_m,
        max_short_term_lufs: max_s,
        start: raw_start,
        end: capped_end,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{AudioData, MediaEntry, VideoSource};
    use crate::model::{AudioSend, Event, GenSpec, MediaItem, MediaSource, Track, TrackKind};
    use std::sync::Arc;

    #[test]
    fn senoide_menos_18_dbfs_mede_lufs_true_peak_e_rms() {
        let sr = 48_000_u32;
        let mut a = LoudnessAnalyzer::try_new(sr).expect("analyzer");
        // Amplitude correspondente a -18 dBFS de pico em senoide 1 kHz.
        let amp = 10.0_f32.powf(-18.0 / 20.0);
        let frames = sr as usize * 4;
        let mut pcm = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let phase = 2.0 * std::f32::consts::PI * 1000.0 * (i as f32) / (sr as f32);
            let s = amp * phase.sin();
            pcm.push(s);
            pcm.push(s);
        }
        a.push_interleaved(&pcm);
        let snap = a.snapshot();
        assert!(snap.integrated_lufs.is_finite());
        assert!(
            (snap.integrated_lufs - (-18.0)).abs() < 1.8,
            "LUFS integrado inesperado: {}",
            snap.integrated_lufs
        );
        assert!(
            (snap.true_peak_dbtp[0] - (-18.0)).abs() < 0.5,
            "True peak L inesperado: {}",
            snap.true_peak_dbtp[0]
        );
        // RMS de senoide é pico - 3.01 dB -> ~ -21.01 dBFS.
        assert!(
            (snap.rms_dbfs[0] - (-21.01)).abs() < 0.6,
            "RMS L inesperado: {}",
            snap.rms_dbfs[0]
        );

        a.reset();
        let after = a.snapshot();
        assert_eq!(after.frames_measured, 0);
        assert!(!after.integrated_lufs.is_finite());
    }

    #[test]
    fn entrada_hostil_com_nan_e_infinito_nao_entra_em_panico() {
        let mut a = LoudnessAnalyzer::new(0);
        a.push_interleaved(&[f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.25, 0.5]);
        let snap = a.snapshot();
        assert!(snap.true_peak_dbtp[0].is_finite() || snap.true_peak_dbtp[0] == -f64::INFINITY);
        assert!(!snap.rms_dbfs[0].is_nan());
        assert!(!snap.rms_dbfs[1].is_nan());
    }

    #[test]
    fn varredura_offline_reflete_roteamento_em_barramento_auxiliar() {
        let sr = 48_000_u32;
        let frames = sr as usize * 2;
        let amp = 0.25_f32;
        let mut pcm = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let phase = 2.0 * std::f32::consts::PI * 997.0 * (i as f32) / (sr as f32);
            let s = amp * phase.sin();
            pcm.push(s);
            pcm.push(s);
        }
        let mut p = Project::new_default();
        let item = MediaItem {
            id: 42,
            name: "tom.wav".into(),
            source: MediaSource::Generated(GenSpec::SolidColor {
                color: [0.0, 0.0, 0.0, 1.0],
            }),
            duration: Some(Ticks::from_secs_i64(2)),
            has_video: false,
            has_audio: true,
            width: 0,
            height: 0,
            fps: None,
            info: "test".into(),
            proxy_path: None,
        };
        p.media.push(item.clone());
        let cache = MediaCache::default();
        cache.insert(
            42,
            MediaEntry {
                video: VideoSource::None,
                audio: Some(Arc::new(AudioData::from_stereo(sr, pcm))),
            },
        );
        let bus_id = p.add_audio_bus(Some("Bus A")).expect("bus A");
        let mut tr = Track::new(TrackKind::Audio, "Audio 1");
        tr.events
            .push(Event::new(&item, Ticks::ZERO, Ticks::from_secs_i64(2)));
        tr.output_bus = Some(bus_id);
        tr.sends.push(AudioSend {
            bus_id,
            level_db: -6.0,
            pre_fader: false,
            mute: false,
        });
        p.tracks.clear();
        p.tracks.push(tr);

        let cancel = AtomicBool::new(false);
        let rep_base =
            analyze_project_loudness(&p, &cache, None, sr, &cancel, None).expect("analise base");
        assert!(rep_base.snapshot.integrated_lufs.is_finite());

        if let Some(b) = p.audio_bus_mut(bus_id) {
            b.volume_db = -12.0;
        }
        let rep_att = analyze_project_loudness(&p, &cache, None, sr, &cancel, None)
            .expect("analise atenuada");
        assert!(
            rep_att.snapshot.integrated_lufs < rep_base.snapshot.integrated_lufs - 10.0,
            "atenuar o barramento deve reduzir o loudness medido ({} vs {})",
            rep_att.snapshot.integrated_lufs,
            rep_base.snapshot.integrated_lufs
        );
    }
}
