//! Transcrição de fala por IA e edição de timeline pelo texto (AGENTS.md §0 / §1 / §2 / VEGAS Pro manual p. 674).
//!
//! Fornece:
//! - Modelo de dados de transcrição com carimbos exatos em `Ticks` (`Word`, `Segment`, `Transcript`);
//! - Detecção automática de pausas e silêncios;
//! - Edição de timeline orientada pelo texto (*Ripple Delete* / *Smart Split*);
//! - Gerenciamento de modelos Whisper sob demanda (fora do repositório git e do instalador);
//! - Geração de marcadores de região para legendas a partir do texto transcrito.

pub mod edit;
pub mod model_cache;
pub mod transcript;

pub use edit::{
    generate_regions_from_transcript, remove_transcript_silences, ripple_delete_transcript_range,
};
pub use model_cache::{
    is_model_downloaded, models_dir, transcribe_audio_samples, SpeechConfig, WhisperModelSize,
};
pub use transcript::{Segment, Transcript, Word};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{new_id, Event, MediaItem, MediaSource, Project};
    use crate::session::EditorSession;
    use crate::time::Ticks;

    #[test]
    fn transcricao_palavras_segmentos_e_deteccao_de_silencios() {
        let mut t = Transcript::new("pt");
        let w1 = Word::new(
            "Olá",
            Ticks::from_secs_i64(0),
            Ticks::from_secs_i64(1),
            Some(0),
        );
        let w2 = Word::new(
            "mundo,",
            Ticks::from_secs_i64(1),
            Ticks::from_secs_i64(2),
            Some(0),
        );
        // Silêncio de 2 segundos entre w2 (2s) e w3 (4s)
        let w3 = Word::new(
            "bem-vindo",
            Ticks::from_secs_i64(4),
            Ticks::from_secs_i64(5),
            Some(1),
        );
        let w4 = Word::new(
            "ao",
            Ticks::from_secs_i64(5),
            Ticks::from_secs_i64(6),
            Some(1),
        );
        let w5 = Word::new(
            "Neoveg.",
            Ticks::from_secs_i64(6),
            Ticks::from_secs_i64(7),
            Some(1),
        );

        let mut seg1 = Segment::new(
            1,
            "Olá mundo,",
            Ticks::from_secs_i64(0),
            Ticks::from_secs_i64(2),
            Some(0),
        );
        seg1.words = vec![w1, w2];

        let mut seg2 = Segment::new(
            2,
            "bem-vindo ao Neoveg.",
            Ticks::from_secs_i64(4),
            Ticks::from_secs_i64(7),
            Some(1),
        );
        seg2.words = vec![w3, w4, w5];

        t.segments = vec![seg1, seg2];

        assert_eq!(t.word_count(), 5);
        assert_eq!(t.duration(), Ticks::from_secs_i64(7));
        assert_eq!(t.speakers(), vec![0, 1]);
        assert_eq!(t.text(), "Olá mundo, bem-vindo ao Neoveg.");

        // Detecta pausa de 2 segundos (entre 2s e 4s)
        let silences = t.find_silences(Ticks::from_secs_i64(1));
        assert_eq!(silences.len(), 1);
        assert_eq!(
            silences[0],
            (Ticks::from_secs_i64(2), Ticks::from_secs_i64(4))
        );

        // Silêncios com limite maior que 2s não encontram nada
        assert!(t.find_silences(Ticks::from_secs_i64(3)).is_empty());
    }

    #[test]
    fn edicao_de_timeline_pelo_texto_ripple_delete_e_remover_silencios() {
        let mut p = Project::new_default();
        let mut m = MediaItem {
            id: new_id(),
            name: "voz.wav".into(),
            source: MediaSource::Generated(crate::model::GenSpec::ColorBars),
            duration: Some(Ticks::from_secs_i64(30)),
            has_video: false,
            has_audio: true,
            width: 0,
            height: 0,
            fps: None,
            info: String::new(),
            proxy_path: None,
        };
        m.has_video = false;
        p.media.push(m.clone());

        // Evento de 10 segundos na trilha de áudio
        let ev = Event::new(&m, Ticks::ZERO, Ticks::from_secs_i64(10));
        let _ev_id = ev.id;
        p.tracks[1].events = vec![ev];

        let mut t = Transcript::new("pt");
        let w1 = Word::new(
            "Trecho1",
            Ticks::from_secs_i64(0),
            Ticks::from_secs_i64(2),
            None,
        );
        // Pausa entre 2s e 5s (3 segundos de silêncio)
        let w2 = Word::new(
            "Trecho2",
            Ticks::from_secs_i64(5),
            Ticks::from_secs_i64(10),
            None,
        );

        let mut seg1 = Segment::new(
            1,
            "Trecho1",
            Ticks::from_secs_i64(0),
            Ticks::from_secs_i64(2),
            None,
        );
        seg1.words = vec![w1];
        let mut seg2 = Segment::new(
            2,
            "Trecho2",
            Ticks::from_secs_i64(5),
            Ticks::from_secs_i64(10),
            None,
        );
        seg2.words = vec![w2];
        t.segments = vec![seg1, seg2];
        p.transcript = Some(t);

        let mut session = EditorSession::new(p);

        // Remove o silêncio detectado (2s a 5s = 3 segundos)
        let removed = remove_transcript_silences(&mut session, Ticks::from_secs_i64(2)).unwrap();
        assert_eq!(removed, 1);

        // A timeline deve ter sido ondulada: a duração do evento restante agora deve ser menor (7s)
        let total_dur = session.project.duration();
        assert!(total_dur <= Ticks::from_secs_i64(8));

        // Gera legendas/regiões a partir dos segmentos restantes
        let regs = generate_regions_from_transcript(&mut session);
        assert!(regs >= 1);
        assert!(!session.project.regions.is_empty());
    }

    #[test]
    fn cache_de_modelos_whisper_e_pastas() {
        let dir = models_dir();
        assert!(dir.is_dir());

        for size in WhisperModelSize::ALL {
            assert!(!size.label().is_empty());
            assert!(size.filename().ends_with(".bin"));
            assert!(size.download_url().starts_with("https://"));
            assert!(size.approx_bytes() > 1_000_000);
        }
    }

    #[test]
    #[allow(clippy::needless_range_loop)]
    fn transcricao_de_amostras_de_audio_vad() {
        let sample_rate = 16000;
        // 2 segundos de áudio com rajada de sinal no meio
        let mut samples = vec![0.0_f32; sample_rate as usize * 2];
        for i in (sample_rate as usize / 2)..(sample_rate as usize) {
            samples[i] = 0.5 * (i as f32 * 0.1).sin();
        }

        let cfg = SpeechConfig::default();
        let transcript = transcribe_audio_samples(&samples, sample_rate, &cfg).unwrap();
        assert!(!transcript.segments.is_empty());
        assert!(transcript.word_count() > 0);
    }
}
