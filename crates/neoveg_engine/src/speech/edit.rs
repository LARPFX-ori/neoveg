//! Edição de timeline orientada pelo texto transcrito (VEGAS Pro manual p. 674 / Filmcraft).
//!
//! Na semântica do VEGAS Pro, editar o texto ou selecionar palavras e exclui-las
//! aciona cortes e ondulação (*Smart Split* e *Auto Ripple*) na timeline.

use crate::model::Region;
use crate::session::EditorSession;
use crate::time::Ticks;

/// Exclui o intervalo de tempo `[start, end)` de fala na timeline e fecha o espaço (*Ripple Delete*).
///
/// Divide eventos de áudio e vídeo no início e término do trecho, remove os pedaços
/// contidos no intervalo, ondula os eventos subsequentes para trás e atualiza a transcrição.
pub fn ripple_delete_transcript_range(
    session: &mut EditorSession,
    start: Ticks,
    end: Ticks,
) -> Result<bool, String> {
    if start >= end {
        return Ok(false);
    }

    session.checkpoint();
    let old_time_sel = session.time_sel;
    let old_sel_events = session.sel_events.clone();

    // Aplica a remoção ondulada por Smart Split em toda a timeline
    session.sel_events.clear();
    session.time_sel = Some((start, end));
    let cuts = session.smart_split();

    // Restaura a seleção anterior se não foi afetada
    session.time_sel = old_time_sel;
    session.sel_events = old_sel_events;

    // Atualiza a transcrição ativa
    if let Some(t) = session.project.transcript.as_mut() {
        t.remove_range(start, end);
    }
    session.changed();

    Ok(cuts > 0)
}

/// Remove automaticamente todos os silêncios e pausas maiores ou iguais a `min_pause`.
///
/// Executa as remoções de trás para frente (da direita para a esquerda na timeline),
/// garantindo que os carimbos de tempo anteriores permaneçam estáveis a cada corte.
pub fn remove_transcript_silences(
    session: &mut EditorSession,
    min_pause: Ticks,
) -> Result<usize, String> {
    let silences = session
        .project
        .transcript
        .as_ref()
        .map(|t| t.find_silences(min_pause))
        .unwrap_or_default();

    if silences.is_empty() {
        return Ok(0);
    }

    session.checkpoint();
    let mut count = 0;

    // Ordena do final para o início da timeline
    let mut sorted_silences = silences;
    sorted_silences.sort_by_key(|a| std::cmp::Reverse(a.0));

    for (start, end) in sorted_silences {
        if ripple_delete_transcript_range(session, start, end)? {
            count += 1;
        }
    }

    Ok(count)
}

/// Gera marcadores de região de legendas/subtítulos na timeline a partir dos segmentos da transcrição.
pub fn generate_regions_from_transcript(session: &mut EditorSession) -> usize {
    let segments = session
        .project
        .transcript
        .as_ref()
        .map(|t| t.segments.clone())
        .unwrap_or_default();

    if segments.is_empty() {
        return 0;
    }

    session.checkpoint();
    let mut created = 0;

    for seg in segments {
        if seg.duration() > Ticks::ZERO && !seg.text.trim().is_empty() {
            let label = if let Some(spk) = seg.speaker {
                format!("[Locutor {}] {}", spk + 1, seg.text.trim())
            } else {
                seg.text.trim().to_string()
            };
            let reg = Region {
                id: crate::model::new_id(),
                start: seg.start,
                end: seg.end,
                label,
            };
            session.project.regions.push(reg);
            created += 1;
        }
    }

    session.project.regions.sort_by_key(|r| r.start);
    session.changed();
    created
}
