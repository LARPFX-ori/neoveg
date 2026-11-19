//! Estruturas de dados de transcrição de fala por IA (VEGAS Pro manual p. 674 / Filmcraft).
//!
//! Cada transcrição contém segmentos e palavras com carimbos de data/hora exatos
//! em `Ticks`, confiança, identificador de locutor (*speaker diarization*) e detecção de silêncios.

use crate::time::Ticks;
use serde::{Deserialize, Serialize};

/// Palavra transcrita individual com carimbos de tempo e locutor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Word {
    pub text: String,
    pub start: Ticks,
    pub end: Ticks,
    #[serde(default)]
    pub speaker: Option<u32>,
    #[serde(default = "default_confidence")]
    pub confidence: f32,
}

fn default_confidence() -> f32 {
    1.0
}

impl Word {
    pub fn new(text: &str, start: Ticks, end: Ticks, speaker: Option<u32>) -> Self {
        Self {
            text: text.to_string(),
            start,
            end: end.max(start),
            speaker,
            confidence: 1.0,
        }
    }

    pub fn duration(&self) -> Ticks {
        (self.end - self.start).max(Ticks::ZERO)
    }
}

/// Segmento de fala (frase ou oração) contendo uma sequência de palavras.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub id: u32,
    pub start: Ticks,
    pub end: Ticks,
    #[serde(default)]
    pub speaker: Option<u32>,
    pub text: String,
    #[serde(default)]
    pub words: Vec<Word>,
}

impl Segment {
    pub fn new(id: u32, text: &str, start: Ticks, end: Ticks, speaker: Option<u32>) -> Self {
        Self {
            id,
            start,
            end: end.max(start),
            speaker,
            text: text.to_string(),
            words: Vec::new(),
        }
    }

    pub fn duration(&self) -> Ticks {
        (self.end - self.start).max(Ticks::ZERO)
    }
}

/// Documento de transcrição completo do projeto ou sequência.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Transcript {
    #[serde(default)]
    pub segments: Vec<Segment>,
    #[serde(default = "default_language")]
    pub language: String,
}

fn default_language() -> String {
    "pt".to_string()
}

impl Transcript {
    pub fn new(language: &str) -> Self {
        Self {
            segments: Vec::new(),
            language: language.to_string(),
        }
    }

    /// Retorna todas as palavras em ordem cronológica linear.
    pub fn words(&self) -> Vec<&Word> {
        let mut out = Vec::new();
        for seg in &self.segments {
            for w in &seg.words {
                out.push(w);
            }
        }
        out
    }

    /// Total de palavras na transcrição.
    pub fn word_count(&self) -> usize {
        self.segments.iter().map(|s| s.words.len()).sum()
    }

    /// Duração total da transcrição (término do último segmento/palavra).
    pub fn duration(&self) -> Ticks {
        let mut d = Ticks::ZERO;
        for s in &self.segments {
            d = d.max(s.end);
            for w in &s.words {
                d = d.max(w.end);
            }
        }
        d
    }

    /// Lista ordenada de locutores identificados na fala.
    pub fn speakers(&self) -> Vec<u32> {
        let mut set = std::collections::BTreeSet::new();
        for s in &self.segments {
            if let Some(spk) = s.speaker {
                set.insert(spk);
            }
            for w in &s.words {
                if let Some(spk) = w.speaker {
                    set.insert(spk);
                }
            }
        }
        set.into_iter().collect()
    }

    /// Texto contínuo completo da transcrição.
    pub fn text(&self) -> String {
        self.segments
            .iter()
            .map(|s| s.text.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Detecta pausas/silêncios entre palavras ou segmentos cuja duração seja maior ou igual a `min_pause`.
    ///
    /// Retorna uma lista de intervalos `(início_do_silêncio, fim_do_silêncio)` em `Ticks`.
    pub fn find_silences(&self, min_pause: Ticks) -> Vec<(Ticks, Ticks)> {
        if min_pause <= Ticks::ZERO {
            return Vec::new();
        }

        let words = self.words();
        let mut silences = Vec::new();

        if !words.is_empty() {
            for i in 0..words.len().saturating_sub(1) {
                let curr_end = words[i].end;
                let next_start = words[i + 1].start;
                if next_start > curr_end {
                    let gap = next_start - curr_end;
                    if gap >= min_pause {
                        silences.push((curr_end, next_start));
                    }
                }
            }
        } else {
            // Se as palavras não estiverem tokenizadas, usa os intervalos entre segmentos
            for i in 0..self.segments.len().saturating_sub(1) {
                let curr_end = self.segments[i].end;
                let next_start = self.segments[i + 1].start;
                if next_start > curr_end {
                    let gap = next_start - curr_end;
                    if gap >= min_pause {
                        silences.push((curr_end, next_start));
                    }
                }
            }
        }

        silences
    }

    /// Remove um intervalo de tempo `[start, end)` da transcrição e ajusta (ripple)
    /// todos os carimbos de data/hora subsequentes para trás em `end - start`.
    pub fn remove_range(&mut self, start: Ticks, end: Ticks) {
        if start >= end {
            return;
        }
        let cut_len = (end - start).max(Ticks::ZERO);

        for seg in &mut self.segments {
            // Filtra e desloca palavras dentro do segmento
            let mut remaining_words = Vec::new();
            for mut w in seg.words.drain(..) {
                if w.end <= start {
                    // Palavra totalmente antes do corte
                    remaining_words.push(w);
                } else if w.start >= end {
                    // Palavra totalmente depois do corte: desloca para trás
                    w.start = (w.start - cut_len).max(Ticks::ZERO);
                    w.end = (w.end - cut_len).max(Ticks::ZERO);
                    remaining_words.push(w);
                }
                // Palavras que caem dentro de [start, end) são descartadas
            }
            seg.words = remaining_words;

            if !seg.words.is_empty() {
                seg.start = seg.words.first().map(|w| w.start).unwrap_or(seg.start);
                seg.end = seg.words.last().map(|w| w.end).unwrap_or(seg.end);
                seg.text = seg
                    .words
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
            } else if seg.start >= end {
                seg.start = (seg.start - cut_len).max(Ticks::ZERO);
                seg.end = (seg.end - cut_len).max(Ticks::ZERO);
            }
        }

        // Remove segmentos que ficaram vazios e que estavam dentro do corte
        self.segments
            .retain(|s| !s.words.is_empty() || s.duration() > Ticks::ZERO);
    }
}
