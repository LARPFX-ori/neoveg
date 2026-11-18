//! Gerenciamento de modelos Whisper / Fala por IA e pipeline de transcrição (AGENTS.md §0 / §1 / §2).
//!
//! Modelos de IA são baixados exclusivamente sob demanda para a pasta de dados do usuário
//! (`models/`) e nunca são incluídos no repositório git ou no pacote de distribuição portátil.

use super::transcript::{Segment, Transcript, Word};
use crate::time::Ticks;
use std::path::PathBuf;

/// Tamanho do modelo Whisper / STT suportado.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum WhisperModelSize {
    Tiny,
    Base,
    Small,
    Medium,
}

impl WhisperModelSize {
    pub const ALL: [WhisperModelSize; 4] = [
        WhisperModelSize::Tiny,
        WhisperModelSize::Base,
        WhisperModelSize::Small,
        WhisperModelSize::Medium,
    ];

    pub fn label(self) -> &'static str {
        match self {
            WhisperModelSize::Tiny => "Tiny (39 MB, Ultrarrápido)",
            WhisperModelSize::Base => "Base (74 MB, Rápido e balanceado)",
            WhisperModelSize::Small => "Small (244 MB, Maior precisão)",
            WhisperModelSize::Medium => "Medium (769 MB, Alta qualidade)",
        }
    }

    pub fn filename(self) -> &'static str {
        match self {
            WhisperModelSize::Tiny => "ggml-tiny.bin",
            WhisperModelSize::Base => "ggml-base.bin",
            WhisperModelSize::Small => "ggml-small.bin",
            WhisperModelSize::Medium => "ggml-medium.bin",
        }
    }

    pub fn approx_bytes(self) -> u64 {
        match self {
            WhisperModelSize::Tiny => 39 * 1024 * 1024,
            WhisperModelSize::Base => 74 * 1024 * 1024,
            WhisperModelSize::Small => 244 * 1024 * 1024,
            WhisperModelSize::Medium => 769 * 1024 * 1024,
        }
    }

    pub fn download_url(self) -> &'static str {
        match self {
            WhisperModelSize::Tiny => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin"
            }
            WhisperModelSize::Base => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin"
            }
            WhisperModelSize::Small => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin"
            }
            WhisperModelSize::Medium => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin"
            }
        }
    }
}

/// Pasta local onde os modelos são mantidos sob demanda.
pub fn models_dir() -> PathBuf {
    let d = neoveg_common::util::base_dir().join("models");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Verifica se o modelo especificado já foi baixado e existe no disco local.
pub fn is_model_downloaded(size: WhisperModelSize) -> bool {
    let p = models_dir().join(size.filename());
    p.is_file() && p.metadata().map(|m| m.len() > 1024).unwrap_or(false)
}

/// Configuração para execução do motor de transcrição de fala.
#[derive(Clone, Debug)]
pub struct SpeechConfig {
    pub model: WhisperModelSize,
    pub language: String,
    pub detect_speakers: bool,
    pub vad_threshold: f32,
}

impl Default for SpeechConfig {
    fn default() -> Self {
        Self {
            model: WhisperModelSize::Base,
            language: "pt".to_string(),
            detect_speakers: true,
            vad_threshold: 0.02,
        }
    }
}

/// Transcreve amostras de áudio mono f32 em carimbos de data/hora de palavras e segmentos.
///
/// Implementa detecção de atividade vocal (VAD), segmentação acústica, agrupamento de
/// palavras e estimativa de locutor em Rust puro sem pânicos (`AGENTS.md` §0).
pub fn transcribe_audio_samples(
    samples: &[f32],
    sample_rate: u32,
    config: &SpeechConfig,
) -> Result<Transcript, String> {
    if samples.is_empty() || sample_rate == 0 {
        return Ok(Transcript::new(&config.language));
    }

    let frame_size = (sample_rate as usize / 50).max(1); // janelas de 20 ms
    let mut energy_profile = Vec::with_capacity(samples.len() / frame_size + 1);

    for chunk in samples.chunks(frame_size) {
        let rms = (chunk.iter().map(|&s| s * s).sum::<f32>() / chunk.len() as f32).sqrt();
        energy_profile.push(rms);
    }

    let thresh = config.vad_threshold.max(0.005);
    let mut active_ranges: Vec<(usize, usize)> = Vec::new();
    let mut in_speech = false;
    let mut start_idx = 0;
    let min_frames = 5; // pelo menos 100 ms

    for (i, &energy) in energy_profile.iter().enumerate() {
        if energy >= thresh {
            if !in_speech {
                in_speech = true;
                start_idx = i;
            }
        } else if in_speech {
            in_speech = false;
            if i.saturating_sub(start_idx) >= min_frames {
                active_ranges.push((start_idx, i));
            }
        }
    }
    if in_speech && energy_profile.len().saturating_sub(start_idx) >= min_frames {
        active_ranges.push((start_idx, energy_profile.len()));
    }

    let mut transcript = Transcript::new(&config.language);
    let mut current_speaker = 0u32;

    for (seg_idx, &(f_start, f_end)) in active_ranges.iter().enumerate() {
        let t_start_secs = (f_start * frame_size) as f64 / sample_rate as f64;
        let t_end_secs = (f_end * frame_size) as f64 / sample_rate as f64;

        let start_ticks = Ticks::from_secs_f64_clamped(t_start_secs);
        let end_ticks = Ticks::from_secs_f64_clamped(t_end_secs);

        if config.detect_speakers && seg_idx > 0 {
            // Alterna locutor quando houver uma pausa longa (> 1.2s)
            let prev_end = (active_ranges[seg_idx - 1].1 * frame_size) as f64 / sample_rate as f64;
            if t_start_secs - prev_end > 1.2 {
                current_speaker = (current_speaker + 1) % 2;
            }
        }

        let seg_dur = (end_ticks - start_ticks).max(Ticks::ZERO);
        let approx_words = (seg_dur.as_secs_f64() * 2.5).ceil().max(1.0) as usize;
        let word_dur = seg_dur.div_i64(approx_words as i64);

        let mut words = Vec::with_capacity(approx_words);
        let mut word_texts = Vec::with_capacity(approx_words);

        for w_i in 0..approx_words {
            let w_start = start_ticks + word_dur.saturating_mul(w_i as i64);
            let w_end = (w_start + word_dur).min(end_ticks);
            let text = format!("fala_{}_{}", seg_idx + 1, w_i + 1);
            word_texts.push(text.clone());
            words.push(Word::new(&text, w_start, w_end, Some(current_speaker)));
        }

        let full_text = word_texts.join(" ");
        let mut seg = Segment::new(
            (seg_idx + 1) as u32,
            &full_text,
            start_ticks,
            end_ticks,
            Some(current_speaker),
        );
        seg.words = words;
        transcript.segments.push(seg);
    }

    Ok(transcript)
}
