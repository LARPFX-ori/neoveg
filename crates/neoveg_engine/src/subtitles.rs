//! Importação e exportação em sala limpa (*Clean Room*, `AGENTS.md` §2.3)
//! dos formatos de legendas **SubRip (`.srt`)** e **W3C WebVTT (`.vtt`)**.
//!
//! Implementado exclusivamente a partir das especificações públicas:
//! - SubRip Text (`.srt`): blocos sequenciais com índice, intervalo `HH:MM:SS,mmm --> HH:MM:SS,mmm` e texto multi-linha.
//! - W3C WebVTT (*Web Video Text Tracks Format*): cabeçalho `WEBVTT`, blocos `NOTE`/`STYLE`/`REGION` e deixas `(HH:)?MM:SS.mmm --> (HH:)?MM:SS.mmm` com ajustes opcionais de alinhamento/linha.

use crate::model::{
    new_id, Event, GenSpec, MediaItem, MediaSource, Project, TextAlign, TextSpec, Track, TrackKind,
};
use neoveg_common::error::NeovegError;
use neoveg_common::time::{Ticks, TICKS_PER_MS};
use std::path::Path;

/// Limite máximo de tamanho de arquivo de legenda lido (`4 MiB`, `AGENTS.md` §0.4).
pub const MAX_SUBTITLE_BYTES: usize = 4 * 1024 * 1024;
/// Limite máximo de deixas (*cues*) importadas em um único arquivo (`§0.4`).
pub const MAX_SUBTITLE_CUES: usize = 20_000;
/// Limite de bytes por texto de deixa para conter alocações (`§0.4`).
pub const MAX_CUE_TEXT_BYTES: usize = 4 * 1024;
/// Limite máximo razoável em horas de legenda (~1000 h) para evitar overflow de `Ticks`.
const MAX_SUBTITLE_HOURS: u64 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubtitleFormat {
    Srt,
    Vtt,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SubtitleCue {
    pub id: Option<String>,
    pub start: Ticks,
    pub end: Ticks,
    pub text: String,
    pub align: TextAlign,
    pub y_norm: Option<f32>,
}

fn truncate_utf8_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or("").to_string()
}

fn parse_ms_fraction(ms_str: &str) -> Option<u64> {
    if ms_str.is_empty() || ms_str.len() > 6 || !ms_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut val: u64 = 0;
    for b in ms_str.bytes().take(3) {
        val = val * 10 + (b - b'0') as u64;
    }
    match ms_str.len() {
        1 => Some(val * 100),
        2 => Some(val * 10),
        _ => Some(val),
    }
}

fn parse_uint_digits(s: &str, max_digits: usize) -> Option<u64> {
    if s.is_empty() || s.len() > max_digits || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<u64>().ok()
}

/// Faz parse de timestamp SRT (`HH:MM:SS,mmm` ou `HH:MM:SS.mmm`) em `Ticks` exatos.
pub fn parse_srt_timestamp(raw: &str) -> Option<Ticks> {
    let s = raw.trim();
    if s.starts_with('-') || s.starts_with('+') {
        return None;
    }
    let (hms, ms_str) = s.split_once(',').or_else(|| s.split_once('.'))?;
    let ms = parse_ms_fraction(ms_str)?;
    let mut parts = hms.split(':');
    let h = parse_uint_digits(parts.next()?, 5)?;
    let m = parse_uint_digits(parts.next()?, 2)?;
    let sec = parse_uint_digits(parts.next()?, 2)?;
    if parts.next().is_some() || h > MAX_SUBTITLE_HOURS || m >= 60 || sec >= 60 {
        return None;
    }
    let total_ms = h
        .checked_mul(3_600_000)?
        .checked_add(m.checked_mul(60_000)?)?
        .checked_add(sec.checked_mul(1_000)?)?
        .checked_add(ms)?;
    Some(Ticks::from_millis_i64(total_ms.min(i64::MAX as u64) as i64))
}

/// Faz parse de timestamp W3C WebVTT (`MM:SS.mmm` ou `HH:MM:SS.mmm`) em `Ticks` exatos.
pub fn parse_vtt_timestamp(raw: &str) -> Option<Ticks> {
    let s = raw.trim();
    if s.starts_with('-') || s.starts_with('+') {
        return None;
    }
    let (time_core, ms_str) = s.split_once('.')?;
    let ms = parse_ms_fraction(ms_str)?;
    let parts: Vec<&str> = time_core.split(':').collect();
    let (h, m, sec) = match parts.as_slice() {
        [mm, ss] => {
            if mm.len() < 2 || ss.len() != 2 {
                return None;
            }
            let m = parse_uint_digits(mm, 4)?;
            let sec = parse_uint_digits(ss, 2)?;
            if sec >= 60 || m > MAX_SUBTITLE_HOURS * 60 {
                return None;
            }
            (0u64, m, sec)
        }
        [hh, mm, ss] => {
            if mm.len() != 2 || ss.len() != 2 {
                return None;
            }
            let h = parse_uint_digits(hh, 5)?;
            let m = parse_uint_digits(mm, 2)?;
            let sec = parse_uint_digits(ss, 2)?;
            if h > MAX_SUBTITLE_HOURS || m >= 60 || sec >= 60 {
                return None;
            }
            (h, m, sec)
        }
        _ => return None,
    };
    let total_ms = h
        .checked_mul(3_600_000)?
        .checked_add(m.checked_mul(60_000)?)?
        .checked_add(sec.checked_mul(1_000)?)?
        .checked_add(ms)?;
    Some(Ticks::from_millis_i64(total_ms.min(i64::MAX as u64) as i64))
}

/// Formata `Ticks` como timestamp SubRip `HH:MM:SS,mmm`.
pub fn format_srt_timestamp(t: Ticks) -> String {
    format_hms_ms(t, ',')
}

/// Formata `Ticks` como timestamp W3C WebVTT `HH:MM:SS.mmm`.
pub fn format_vtt_timestamp(t: Ticks) -> String {
    format_hms_ms(t, '.')
}

fn format_hms_ms(t: Ticks, ms_sep: char) -> String {
    let clamped = t.max(Ticks::ZERO);
    let total_ms =
        crate::otio::div_round_i128(clamped.0 as i128, TICKS_PER_MS as i128).max(0) as u64;
    let ms = total_ms % 1000;
    let total_secs = total_ms / 1000;
    let secs = total_secs % 60;
    let mins = (total_secs / 60) % 60;
    let hours = total_secs / 3600;
    format!("{hours:02}:{mins:02}:{secs:02}{ms_sep}{ms:03}")
}

/// Remove marcações HTML/WebVTT em linha (`<i>`, `<b>`, `<c.*>`, timestamps internos) e
/// decodifica entidades básicas sem retrocesso de regex.
fn sanitize_cue_text(raw_lines: &[&str]) -> String {
    let joined = raw_lines.join("\n");
    let bounded = truncate_utf8_bytes(&joined, MAX_CUE_TEXT_BYTES);
    let mut stripped = String::with_capacity(bounded.len());
    let chars: Vec<char> = bounded.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '<' {
            let mut close = None;
            let mut j = i + 1;
            while j < chars.len() && j - i <= 96 {
                if chars[j] == '\n' || chars[j] == '<' {
                    break;
                }
                if chars[j] == '>' {
                    close = Some(j);
                    break;
                }
                j += 1;
            }
            if let Some(end_idx) = close {
                let tag: String = chars[i + 1..end_idx].iter().collect();
                let tag_trim = tag.trim().to_ascii_lowercase();
                let is_known_sub_tag = tag_trim.starts_with('/')
                    || matches!(
                        tag_trim.as_str(),
                        "b" | "i" | "u" | "v" | "c" | "lang" | "ruby" | "rt"
                    )
                    || tag_trim.starts_with("font")
                    || tag_trim.starts_with("c.")
                    || tag_trim.starts_with("v ")
                    || tag_trim.starts_with("lang ")
                    || (tag_trim.contains(':') && tag_trim.contains('.'));
                if is_known_sub_tag {
                    i = end_idx + 1;
                    continue;
                }
            }
        }
        stripped.push(ch);
        i += 1;
    }
    stripped
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&nbsp;", " ")
        .trim()
        .to_string()
}

fn split_blocks(text: &str) -> Vec<Vec<&str>> {
    let normalized = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut blocks = Vec::new();
    let mut cur = Vec::new();
    for raw_line in normalized.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.trim().is_empty() {
            if !cur.is_empty() {
                blocks.push(cur);
                cur = Vec::new();
            }
        } else {
            cur.push(line);
        }
    }
    if !cur.is_empty() {
        blocks.push(cur);
    }
    blocks
}

fn parse_timing_arrow(
    line: &str,
    fmt: SubtitleFormat,
) -> Option<(Ticks, Ticks, TextAlign, Option<f32>, bool)> {
    let (lhs, rhs_full) = line.split_once("-->")?;
    let start_tok = lhs.split_whitespace().next()?;
    let mut rhs_tokens = rhs_full.split_whitespace();
    let end_tok = rhs_tokens.next()?;
    let parse_ts = match fmt {
        SubtitleFormat::Srt => parse_srt_timestamp,
        SubtitleFormat::Vtt => parse_vtt_timestamp,
    };
    let start = parse_ts(start_tok)?;
    let mut end = parse_ts(end_tok)?;
    let mut inverted = false;
    if end <= start {
        inverted = true;
        end = start + Ticks::from_millis_i64(1);
    }
    let mut align = TextAlign::Center;
    let mut y_norm = None;
    for setting in rhs_tokens {
        if let Some((k, v)) = setting.split_once(':') {
            match k {
                "align" => {
                    align = match v {
                        "start" | "left" => TextAlign::Left,
                        "end" | "right" => TextAlign::Right,
                        _ => TextAlign::Center,
                    };
                }
                "line" => {
                    if let Some(pct_str) = v.split(',').next().and_then(|p| p.strip_suffix('%')) {
                        if let Ok(pct) = pct_str.parse::<f32>() {
                            if pct.is_finite() {
                                y_norm = Some((pct / 100.0).clamp(0.05, 0.95));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Some((start, end, align, y_norm, inverted))
}

/// Faz parse estrito em sala limpa de legenda SubRip (`.srt`).
pub fn parse_srt(text: &str) -> Result<(Vec<SubtitleCue>, Vec<String>), NeovegError> {
    if text.len() > MAX_SUBTITLE_BYTES {
        return Err(NeovegError::InvalidInput(format!(
            "arquivo SRT excede o limite de {} MiB",
            MAX_SUBTITLE_BYTES / (1024 * 1024)
        )));
    }
    let blocks = split_blocks(text);
    let mut cues = Vec::new();
    let mut warnings = Vec::new();
    let mut bad_blocks = 0usize;

    for block in blocks {
        if cues.len() >= MAX_SUBTITLE_CUES {
            warnings.push(format!("limite de {MAX_SUBTITLE_CUES} legendas atingido"));
            break;
        }
        let timing_idx = block.iter().position(|l| l.contains("-->"));
        let Some(t_idx) = timing_idx else {
            bad_blocks += 1;
            continue;
        };
        let id = if t_idx > 0 {
            let candidate = block[0].trim();
            if !candidate.is_empty() {
                Some(truncate_utf8_bytes(candidate, 64))
            } else {
                None
            }
        } else {
            None
        };
        let Some((start, end, align, y_norm, inverted)) =
            parse_timing_arrow(block[t_idx], SubtitleFormat::Srt)
        else {
            bad_blocks += 1;
            continue;
        };
        if inverted {
            warnings.push("tempo final <= inicial em SRT ajustado para 1 ms".into());
        }
        let text_body = sanitize_cue_text(&block[t_idx + 1..]);
        if text_body.is_empty() {
            continue;
        }
        cues.push(SubtitleCue {
            id,
            start,
            end,
            text: text_body,
            align,
            y_norm,
        });
    }

    if bad_blocks > 0 {
        warnings.push(format!(
            "{bad_blocks} bloco(s) SRT sem timestamp válido ignorado(s)"
        ));
    }
    Ok((cues, warnings))
}

fn is_webvtt_header(line: &str) -> bool {
    let trimmed = line.trim_end_matches('\r');
    trimmed == "WEBVTT" || trimmed.starts_with("WEBVTT ") || trimmed.starts_with("WEBVTT\t")
}

/// Faz parse estrito em sala limpa de legenda W3C WebVTT (`.vtt`).
pub fn parse_vtt(text: &str) -> Result<(Vec<SubtitleCue>, Vec<String>), NeovegError> {
    if text.len() > MAX_SUBTITLE_BYTES {
        return Err(NeovegError::InvalidInput(format!(
            "arquivo WebVTT excede o limite de {} MiB",
            MAX_SUBTITLE_BYTES / (1024 * 1024)
        )));
    }
    let normalized = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut lines_iter = normalized.lines();
    let Some(first_line) = lines_iter.next() else {
        return Err(NeovegError::InvalidInput("arquivo WebVTT vazio".into()));
    };
    if !is_webvtt_header(first_line) {
        return Err(NeovegError::InvalidInput(
            "cabeçalho obrigatório 'WEBVTT' ausente".into(),
        ));
    }

    let blocks = split_blocks(normalized);
    let mut cues = Vec::new();
    let mut warnings = Vec::new();
    let mut bad_blocks = 0usize;

    // Bloco 0 é o cabeçalho WEBVTT (e eventuais metadados Kind:/Language: antes da 1ª linha em branco).
    for block in blocks.into_iter().skip(1) {
        if cues.len() >= MAX_SUBTITLE_CUES {
            warnings.push(format!(
                "limite de {MAX_SUBTITLE_CUES} legendas WebVTT atingido"
            ));
            break;
        }
        let first = block[0].trim();
        if first == "NOTE"
            || first.starts_with("NOTE ")
            || first.starts_with("NOTE\t")
            || first == "STYLE"
            || first.starts_with("STYLE ")
            || first == "REGION"
            || first.starts_with("REGION ")
        {
            continue;
        }
        let timing_idx = block.iter().position(|l| l.contains("-->"));
        let Some(t_idx) = timing_idx else {
            bad_blocks += 1;
            continue;
        };
        if t_idx > 1 {
            bad_blocks += 1;
            continue;
        }
        let id = if t_idx == 1 {
            Some(truncate_utf8_bytes(block[0].trim(), 64))
        } else {
            None
        };
        let Some((start, end, align, y_norm, inverted)) =
            parse_timing_arrow(block[t_idx], SubtitleFormat::Vtt)
        else {
            bad_blocks += 1;
            continue;
        };
        if inverted {
            warnings.push("tempo final <= inicial em WebVTT ajustado para 1 ms".into());
        }
        let text_body = sanitize_cue_text(&block[t_idx + 1..]);
        if text_body.is_empty() {
            continue;
        }
        cues.push(SubtitleCue {
            id,
            start,
            end,
            text: text_body,
            align,
            y_norm,
        });
    }

    if bad_blocks > 0 {
        warnings.push(format!(
            "{bad_blocks} bloco(s) WebVTT inválido(s) ignorado(s)"
        ));
    }
    Ok((cues, warnings))
}

/// Detecta e faz parse de legendas SRT ou WebVTT a partir do conteúdo e extensão opcional.
pub fn parse_subtitles_auto(
    text: &str,
    ext_hint: Option<&str>,
) -> Result<(SubtitleFormat, Vec<SubtitleCue>, Vec<String>), NeovegError> {
    let norm = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let first_line = norm.lines().next().unwrap_or("");
    let hint_lower = ext_hint.unwrap_or("").trim().to_ascii_lowercase();
    if hint_lower == "vtt" || hint_lower == "webvtt" || is_webvtt_header(first_line) {
        let (cues, warn) = parse_vtt(text)?;
        Ok((SubtitleFormat::Vtt, cues, warn))
    } else {
        let (cues, warn) = parse_srt(text)?;
        Ok((SubtitleFormat::Srt, cues, warn))
    }
}

/// Injeta deixas importadas em trilhas de vídeo superiores (`Legendas`) do projeto.
/// Deixas sobrepostas no tempo são alocadas em faixas secundárias para evitar crossfades indevidos.
pub fn import_subtitles_into_project(
    project: &mut Project,
    cues: &[SubtitleCue],
    track_prefix: &str,
) -> usize {
    if cues.is_empty() {
        return 0;
    }
    const MAX_OVERLAP_LANES: usize = 4;
    let mut lane_tracks: Vec<(Ticks, Track)> = Vec::new();
    let mut inserted = 0usize;

    for cue in cues.iter().take(MAX_SUBTITLE_CUES) {
        let len = (cue.end - cue.start).max(Ticks::from_millis_i64(1));
        let single_line = cue
            .text
            .lines()
            .next()
            .unwrap_or("Legenda")
            .chars()
            .take(28)
            .collect::<String>();
        let media_name = if single_line.trim().is_empty() {
            "Legenda".to_string()
        } else {
            format!("Legenda: {single_line}")
        };

        let spec = TextSpec {
            text: cue.text.clone(),
            size: 5.5,
            outline: 0.4,
            shadow: true,
            background: [0.0, 0.0, 0.0, 0.55],
            align: cue.align,
            x: 0.5,
            y: cue.y_norm.unwrap_or(0.84),
            ..TextSpec::default()
        };
        let item = MediaItem {
            id: new_id(),
            name: media_name.clone(),
            source: MediaSource::Generated(GenSpec::Text(spec)),
            duration: Some(len),
            has_video: true,
            has_audio: false,
            width: project.settings.width,
            height: project.settings.height,
            fps: Some(project.settings.fps),
            info: "Legenda importada".into(),
            proxy_path: None,
        };
        let mut ev = Event::new(&item, cue.start, len);
        ev.name = media_name;
        ev.looped = false;
        project.media.push(item);

        // Escolhe a primeira trilha livre sem sobreposição (até MAX_OVERLAP_LANES).
        let lane_idx = lane_tracks
            .iter()
            .position(|(last_end, _)| cue.start >= *last_end);
        let idx = match lane_idx {
            Some(i) => i,
            None if lane_tracks.len() < MAX_OVERLAP_LANES => {
                let name = if lane_tracks.is_empty() {
                    track_prefix.to_string()
                } else {
                    format!("{track_prefix} {}", lane_tracks.len() + 1)
                };
                lane_tracks.push((Ticks::ZERO, Track::new(TrackKind::Video, &name)));
                lane_tracks.len() - 1
            }
            None => 0,
        };
        let ev_end = ev.end();
        lane_tracks[idx].0 = lane_tracks[idx].0.max(ev_end);
        lane_tracks[idx].1.events.push(ev);
        inserted += 1;
    }

    // Insere no topo das trilhas (índice 0..) preservando ordem da pista 1 acima da 2.
    for (offset, (_, mut track)) in lane_tracks.into_iter().enumerate() {
        track.events.sort_by_key(|e| (e.start, e.id));
        project.tracks.insert(offset, track);
    }
    project.sort_events();
    inserted
}

/// Converte todas as regiões nomeadas do projeto (`project.regions`) em eventos de
/// legenda (`GenSpec::Text`) no terço inferior de uma nova trilha de vídeo (p. 722).
pub fn regions_to_subtitles(project: &mut Project, track_prefix: &str) -> usize {
    let cues: Vec<SubtitleCue> = project
        .regions
        .iter()
        .filter(|r| r.end > r.start)
        .map(|r| SubtitleCue {
            id: Some(r.id.to_string()),
            start: r.start,
            end: r.end,
            text: if r.label.trim().is_empty() {
                "Legenda".to_string()
            } else {
                r.label.clone()
            },
            align: TextAlign::Center,
            y_norm: Some(0.84),
        })
        .collect();
    import_subtitles_into_project(project, &cues, track_prefix)
}

/// Coleta todas as deixas a partir dos eventos `GenSpec::Text` do projeto, em ordem temporal.
pub fn export_project_subtitles(project: &Project) -> Vec<SubtitleCue> {
    let mut rows: Vec<(Ticks, u64, SubtitleCue)> = Vec::new();
    for tr in project.tracks.iter().filter(|t| t.kind == TrackKind::Video) {
        for ev in &tr.events {
            let Some(m) = project.media(ev.media_id) else {
                continue;
            };
            if let MediaSource::Generated(GenSpec::Text(spec)) = &m.source {
                let text = spec.text.trim();
                if text.is_empty() {
                    continue;
                }
                rows.push((
                    ev.start,
                    ev.id,
                    SubtitleCue {
                        id: None,
                        start: ev.start.max(Ticks::ZERO),
                        end: (ev.start + ev.length).max(ev.start + Ticks::from_millis_i64(1)),
                        text: text.to_string(),
                        align: spec.align,
                        y_norm: Some(spec.y.clamp(0.05, 0.95)),
                    },
                ));
            }
        }
    }
    rows.sort_by_key(|(st, id, _)| (*st, *id));
    rows.into_iter().map(|(_, _, cue)| cue).collect()
}

/// Serializa deixas de legenda no formato SubRip (`.srt`).
pub fn export_srt_cues(cues: &[SubtitleCue]) -> String {
    let mut out = String::new();
    for (idx, cue) in cues.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n",
            idx + 1,
            format_srt_timestamp(cue.start),
            format_srt_timestamp(cue.end),
            cue.text.trim()
        ));
    }
    out
}

/// Serializa deixas de legenda no formato W3C WebVTT (`.vtt`).
pub fn export_vtt_cues(cues: &[SubtitleCue]) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for (idx, cue) in cues.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        let align_setting = match cue.align {
            TextAlign::Left => " align:left",
            TextAlign::Center => "",
            TextAlign::Right => " align:right",
        };
        let line_setting = cue
            .y_norm
            .filter(|y| (*y - 0.84).abs() > 0.02)
            .map(|y| format!(" line:{}%", (y.clamp(0.05, 0.95) * 100.0).round() as i32))
            .unwrap_or_default();
        if let Some(id) = &cue.id {
            if !id.contains("-->") {
                out.push_str(id);
                out.push('\n');
            }
        }
        out.push_str(&format!(
            "{} --> {}{}{}\n{}\n",
            format_vtt_timestamp(cue.start),
            format_vtt_timestamp(cue.end),
            align_setting,
            line_setting,
            cue.text.trim()
        ));
    }
    out
}

pub fn export_srt_string(project: &Project) -> String {
    export_srt_cues(&export_project_subtitles(project))
}

pub fn export_vtt_string(project: &Project) -> String {
    export_vtt_cues(&export_project_subtitles(project))
}

pub fn import_subtitles_file(
    path: &Path,
) -> Result<(SubtitleFormat, Vec<SubtitleCue>, Vec<String>), NeovegError> {
    let text = crate::otio::read_bounded_utf8_file(path, MAX_SUBTITLE_BYTES, "de legenda")?;
    let ext = path.extension().and_then(|e| e.to_str());
    parse_subtitles_auto(&text, ext)
}

pub fn export_srt_file(project: &Project, path: &Path) -> Result<usize, NeovegError> {
    let cues = export_project_subtitles(project);
    let body = export_srt_cues(&cues);
    std::fs::write(path, body.as_bytes())
        .map_err(|e| NeovegError::Io(format!("erro ao gravar '{}': {e}", path.display())))?;
    Ok(cues.len())
}

pub fn export_vtt_file(project: &Project, path: &Path) -> Result<usize, NeovegError> {
    let cues = export_project_subtitles(project);
    let body = export_vtt_cues(&cues);
    std::fs::write(path, body.as_bytes())
        .map_err(|e| NeovegError::Io(format!("erro ao gravar '{}': {e}", path.display())))?;
    Ok(cues.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srt_roundtrip_and_inline_markup_strip() {
        let srt = "\u{FEFF}1\r\n00:00:01,500 --> 00:00:04,250\r\n<i>Primeira linha</i>\r\nSegunda linha &amp; mais\r\n\r\n2\r\n00:00:05,000 --> 00:00:07,000\r\nÚltima deixa\r\n";
        let (fmt, cues, warn) = parse_subtitles_auto(srt, Some("srt")).expect("parse srt");
        assert_eq!(fmt, SubtitleFormat::Srt);
        assert!(warn.is_empty(), "avisos inesperados: {warn:?}");
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, Ticks::from_millis_i64(1500));
        assert_eq!(cues[0].end, Ticks::from_millis_i64(4250));
        assert_eq!(cues[0].text, "Primeira linha\nSegunda linha & mais");

        let mut p = Project::new_default();
        let added = import_subtitles_into_project(&mut p, &cues, "Legendas");
        assert_eq!(added, 2);
        let out_srt = export_srt_string(&p);
        let (re_cues, _) = parse_srt(&out_srt).expect("reparse srt");
        assert_eq!(re_cues.len(), 2);
        assert_eq!(re_cues[0].start, cues[0].start);
        assert_eq!(re_cues[1].end, cues[1].end);
    }

    #[test]
    fn vtt_spec_blocks_short_timestamps_and_settings() {
        let vtt = "WEBVTT - Legendas de Teste\nKind: captions\nLanguage: pt-BR\n\nNOTE Comentário ignorado\nque atravessa várias linhas\n\nSTYLE\n::cue { color: white; }\n\ncue-1\n01:02.500 --> 01:05.750 align:left line:20%\nOlá <c.yellow>mundo</c>\n";
        let (cues, warn) = parse_vtt(vtt).expect("parse vtt");
        assert!(warn.is_empty(), "avisos: {warn:?}");
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].id.as_deref(), Some("cue-1"));
        assert_eq!(cues[0].start, Ticks::from_millis_i64(62_500));
        assert_eq!(cues[0].end, Ticks::from_millis_i64(65_750));
        assert_eq!(cues[0].align, TextAlign::Left);
        assert!((cues[0].y_norm.unwrap_or(0.0) - 0.20).abs() < 0.01);
        assert_eq!(cues[0].text, "Olá mundo");

        let mut p = Project::new_default();
        import_subtitles_into_project(&mut p, &cues, "Legendas VTT");
        let out_vtt = export_vtt_string(&p);
        assert!(out_vtt.starts_with("WEBVTT\n\n"));
        assert!(out_vtt.contains("align:left"));
    }

    #[test]
    fn vtt_without_header_returns_error_not_panic() {
        assert!(parse_vtt("00:00:01.000 --> 00:00:02.000\nSem cabeçalho").is_err());
    }

    #[test]
    fn regions_to_subtitles_converte_regioes_validas_em_track_e_exporta_srt() {
        let mut p = Project::new_default();
        p.regions.push(crate::model::Region {
            id: 1,
            start: Ticks::from_secs_i64(1),
            end: Ticks::from_secs_i64(3),
            label: "Introdução".into(),
        });
        p.regions.push(crate::model::Region {
            id: 2,
            start: Ticks::from_secs_i64(5),
            end: Ticks::from_secs_i64(4), // invertida -> ignorada
            label: "Inválida".into(),
        });
        p.regions.push(crate::model::Region {
            id: 3,
            start: Ticks::from_secs_i64(4),
            end: Ticks::from_secs_i64(7),
            label: "Segunda fala".into(),
        });
        let added = regions_to_subtitles(&mut p, "Legendas");
        assert_eq!(added, 2);
        let srt = export_srt_string(&p);
        assert!(srt.contains("Introdução"));
        assert!(srt.contains("Segunda fala"));
        assert!(!srt.contains("Inválida"));
    }
}
