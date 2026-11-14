//! Importação e exportação em sala limpa (*Clean Room*, `AGENTS.md` §2.3)
//! de listas de decisão de edição **CMX 3600 EDL** (SMPTE 258M / ASCII EDL).
//!
//! Implementado exclusivamente a partir da especificação pública do formato CMX 3600:
//! - Cabeçalhos `TITLE:` e `FCM: DROP FRAME` / `FCM: NON-DROP FRAME`.
//! - Registros de edição `<NUM> <REEL> <CANAL> <MODO> [DUR_QUADROS] <SRC_IN> <SRC_OUT> <REC_IN> <REC_OUT>`.
//! - Comentários `* FROM CLIP NAME:`, `* SOURCE FILE:`, `* LOC:` (marcadores) e eventos de velocidade `M2`.

use crate::model::{
    new_id, Event, GenSpec, Id, Marker, MediaItem, MediaSource, Project, Track, TrackKind,
    TransitionKind, TransitionSpec, MAX_TICKS,
};
use neoveg_common::error::NeovegError;
use neoveg_common::time::{format_ticks, parse_time, FrameRate, Rate, Ticks, TimeFormat};
use std::collections::HashMap;
use std::path::Path;

/// Limite máximo de tamanho de arquivo EDL lido (`4 MiB`, `AGENTS.md` §0.4).
pub const MAX_EDL_BYTES: usize = 4 * 1024 * 1024;
/// Limite máximo de entradas de edição em um único EDL (`§0.4`).
pub const MAX_EDL_EVENTS: usize = 10_000;
const MAX_EDL_STR_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EdlChannels {
    Video,
    Audio,
    VideoAudio,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EdlMode {
    Cut,
    Dissolve(u64),
    Wipe(u16, u64),
}

#[derive(Clone, Debug)]
struct ParsedEdlEntry {
    reel: String,
    channels: EdlChannels,
    mode: EdlMode,
    src_in: Ticks,
    src_out: Ticks,
    rec_in: Ticks,
    rec_out: Ticks,
    clip_name: Option<String>,
    source_file: Option<String>,
    speed_rate: Option<Rate>,
}

fn bound_utf8(s: &str, max_bytes: usize) -> String {
    let t = s.trim();
    if t.len() <= max_bytes {
        return t.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !t.is_char_boundary(end) {
        end -= 1;
    }
    t.get(..end).unwrap_or("").to_string()
}

fn parse_edl_channels(tok: &str) -> Option<EdlChannels> {
    let u = tok.trim().to_ascii_uppercase();
    match u.as_str() {
        "V" | "V1" | "V2" | "V3" | "V4" => Some(EdlChannels::Video),
        "A" | "A1" | "A2" | "A3" | "A4" | "AA" | "A12" | "A34" | "AUD" => Some(EdlChannels::Audio),
        "B" | "AA/V" | "A2/V" | "A/V" | "V/A" | "V+A" | "VA" | "AV" | "A12/V" => {
            Some(EdlChannels::VideoAudio)
        }
        _ => {
            let has_v = u.contains('V');
            let has_a = u.contains('A');
            match (has_v, has_a) {
                (true, true) => Some(EdlChannels::VideoAudio),
                (true, false) => Some(EdlChannels::Video),
                (false, true) => Some(EdlChannels::Audio),
                (false, false) => None,
            }
        }
    }
}

fn parse_edl_tc(tok: &str, fps: FrameRate, drop_frame: bool) -> Option<(Ticks, bool)> {
    let s = tok.trim();
    let has_semi = s.contains(';');
    let colon_count = s.bytes().filter(|&b| b == b':').count();
    if !(has_semi && colon_count == 2 || !has_semi && colon_count == 3) {
        return None;
    }
    let norm = if drop_frame && !has_semi && colon_count == 3 {
        if let Some(pos) = s.rfind(':') {
            let mut owned = s.to_string();
            if pos < owned.len() {
                owned.replace_range(pos..=pos, ";");
            }
            owned
        } else {
            s.to_string()
        }
    } else {
        s.to_string()
    };
    let fmt = if norm.contains(';') {
        TimeFormat::SmpteDrop
    } else {
        TimeFormat::SmpteNonDrop
    };
    let t = parse_time(&norm, fps, 48_000, fmt)?;
    Some((
        t.clamp_safe(Ticks::ZERO, Ticks::from_raw(MAX_TICKS)),
        has_semi,
    ))
}

fn parse_edl_mode(
    mode_tok: &str,
    dur_tok: Option<&str>,
) -> Option<(EdlMode, Option<&'static str>)> {
    let u = mode_tok.trim().to_ascii_uppercase();
    if u == "C" {
        return Some((EdlMode::Cut, None));
    }
    if u == "D" {
        let frames = dur_tok
            .and_then(|d| d.parse::<u64>().ok())
            .unwrap_or(24)
            .min(1000);
        return Some((EdlMode::Dissolve(frames), None));
    }
    if let Some(code_str) = u.strip_prefix('W') {
        let code = if code_str.is_empty() {
            1
        } else {
            code_str.parse::<u16>().unwrap_or(1)
        };
        let frames = dur_tok
            .and_then(|d| d.parse::<u64>().ok())
            .unwrap_or(24)
            .min(1000);
        return Some((EdlMode::Wipe(code, frames), None));
    }
    if matches!(u.as_str(), "K" | "KB" | "KO") {
        return Some((
            EdlMode::Cut,
            Some("operação Key/Key-Background em EDL importada como corte"),
        ));
    }
    None
}

fn is_black_reel(reel: &str) -> bool {
    let u = reel.trim().to_ascii_uppercase();
    matches!(u.as_str(), "BL" | "BLK" | "BLACK" | "BLACK/SILENCE")
}

fn transition_from_edl_mode(mode: EdlMode) -> Option<(TransitionKind, u64)> {
    match mode {
        EdlMode::Cut => None,
        EdlMode::Dissolve(frames) => Some((TransitionKind::Dissolve, frames)),
        EdlMode::Wipe(code, frames) => {
            let kind = match code {
                1 => TransitionKind::WipeLeft,
                2 => TransitionKind::WipeRight,
                3 => TransitionKind::WipeUp,
                4 => TransitionKind::WipeDown,
                21 | 101 => TransitionKind::IrisOpen,
                22 | 102 => TransitionKind::IrisClose,
                201 => TransitionKind::ClockWipe,
                _ => TransitionKind::Dissolve,
            };
            Some((kind, frames))
        }
    }
}

/// Faz parse em sala limpa de texto EDL CMX 3600 usando a taxa de quadros informada.
pub fn import_edl_str(text: &str, fps: FrameRate) -> Result<(Project, Vec<String>), NeovegError> {
    if text.len() > MAX_EDL_BYTES {
        return Err(NeovegError::InvalidInput(format!(
            "arquivo EDL excede o limite de {} MiB",
            MAX_EDL_BYTES / (1024 * 1024)
        )));
    }
    let normalized = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut title = String::new();
    let mut drop_frame = fps.is_ntsc_drop_eligible();
    let mut entries: Vec<ParsedEdlEntry> = Vec::new();
    let mut raw_markers: Vec<(Ticks, String)> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut warned_key = false;

    for raw_line in normalized.lines() {
        let line = raw_line.trim_end_matches('\r').trim();
        if line.is_empty() {
            continue;
        }
        let upper = line.to_ascii_uppercase();
        if let Some(rest) = upper.strip_prefix("TITLE:") {
            let orig_rest = line.get("TITLE:".len()..).unwrap_or(rest);
            title = bound_utf8(orig_rest, 256);
            continue;
        }
        if let Some(fcm_rest) = upper.strip_prefix("FCM:") {
            let fcm = fcm_rest.trim();
            if fcm.contains("NON-DROP") || fcm.contains("NDF") {
                drop_frame = false;
            } else if fcm.contains("DROP") {
                drop_frame = true;
            }
            continue;
        }
        if let Some(comment) = line.strip_prefix('*') {
            let c_trim = comment.trim();
            let c_up = c_trim.to_ascii_uppercase();
            if let Some(name) = c_up
                .strip_prefix("FROM CLIP NAME:")
                .and_then(|_| c_trim.get("FROM CLIP NAME:".len()..))
            {
                if let Some(last) = entries.last_mut() {
                    let cleaned = bound_utf8(name, MAX_EDL_STR_BYTES);
                    if !cleaned.is_empty() {
                        last.clip_name = Some(cleaned);
                    }
                }
            } else if let Some(src) = c_up
                .strip_prefix("SOURCE FILE:")
                .and_then(|_| c_trim.get("SOURCE FILE:".len()..))
                .or_else(|| {
                    c_up.strip_prefix("CLIP FILE:")
                        .and_then(|_| c_trim.get("CLIP FILE:".len()..))
                })
            {
                if let Some(last) = entries.last_mut() {
                    let cleaned = bound_utf8(src, MAX_EDL_STR_BYTES);
                    if !cleaned.is_empty() {
                        last.source_file = Some(cleaned);
                    }
                }
            } else if let Some(loc_rest) = c_up
                .strip_prefix("LOC:")
                .and_then(|_| c_trim.get("LOC:".len()..))
            {
                let mut ltoks = loc_rest.split_whitespace();
                if let Some(tc_str) = ltoks.next() {
                    if let Some((tc, _)) = parse_edl_tc(tc_str, fps, drop_frame) {
                        let _color = ltoks.next();
                        let label: String = ltoks.collect::<Vec<_>>().join(" ");
                        let clean_lbl = bound_utf8(
                            if label.is_empty() {
                                "Marcador EDL"
                            } else {
                                &label
                            },
                            128,
                        );
                        raw_markers.push((tc, clean_lbl));
                    }
                }
            }
            continue;
        }
        if upper.starts_with("M2") {
            let toks: Vec<&str> = line.split_whitespace().collect();
            if toks.len() >= 3 {
                if let Ok(m2_fps) = toks[2].parse::<f64>() {
                    if m2_fps.is_finite() && m2_fps > 0.0 {
                        let ratio = m2_fps / fps.as_f64().max(1.0);
                        if let Some(rate) = Rate::from_f64(ratio) {
                            if let Some(last) = entries.iter_mut().rev().find(|e| e.reel == toks[1])
                            {
                                last.speed_rate = Some(rate);
                            } else if let Some(last) = entries.last_mut() {
                                last.speed_rate = Some(rate);
                            }
                        }
                    }
                }
            }
            continue;
        }

        let toks: Vec<&str> = line.split_whitespace().collect();
        if toks.len() < 8 {
            continue;
        }
        if toks[0].len() > 6 || !toks[0].bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Some(channels) = parse_edl_channels(toks[2]) else {
            continue;
        };
        let dur_tok = toks.get(4).copied().filter(|_| toks.len() >= 9);
        let Some((mode, mode_warn)) = parse_edl_mode(toks[3], dur_tok) else {
            continue;
        };
        if let Some(w) = mode_warn {
            if !warned_key {
                warnings.push(w.into());
                warned_key = true;
            }
        }
        let n = toks.len();
        let (Some((src_in, s0)), Some((src_out, s1)), Some((rec_in, s2)), Some((rec_out, s3))) = (
            parse_edl_tc(toks[n - 4], fps, drop_frame),
            parse_edl_tc(toks[n - 3], fps, drop_frame),
            parse_edl_tc(toks[n - 2], fps, drop_frame),
            parse_edl_tc(toks[n - 1], fps, drop_frame),
        ) else {
            continue;
        };
        if s0 || s1 || s2 || s3 {
            drop_frame = true;
        }
        if entries.len() >= MAX_EDL_EVENTS {
            warnings.push(format!("limite de {MAX_EDL_EVENTS} entradas EDL atingido"));
            break;
        }
        entries.push(ParsedEdlEntry {
            reel: bound_utf8(toks[1], 64),
            channels,
            mode,
            src_in,
            src_out,
            rec_in,
            rec_out,
            clip_name: None,
            source_file: None,
            speed_rate: None,
        });
    }

    if entries.is_empty() {
        return Err(NeovegError::InvalidInput(
            "nenhuma entrada de edição CMX 3600 válida encontrada".into(),
        ));
    }

    // Normaliza convenção clássica CMX de hora inicial 01:00:00:00 se todas as
    // marcas começam dentro da faixa [1h, 2h) e não há cortes anteriores a 1h.
    let one_hour = fps.frame_to_ticks(fps.nominal_fps().max(1) as i64 * 3600);
    let min_rec = entries
        .iter()
        .map(|e| e.rec_in)
        .chain(raw_markers.iter().map(|(t, _)| *t))
        .min()
        .unwrap_or(Ticks::ZERO);
    let base_offset = if min_rec >= one_hour && min_rec < one_hour + one_hour {
        warnings.push("Horário base 01:00:00:00 do EDL normalizado para 00:00:00:00.".into());
        one_hour
    } else {
        Ticks::ZERO
    };

    let mut project = Project::new_default();
    project.tracks.clear();
    project.media.clear();
    project.markers.clear();
    project.regions.clear();
    project.settings.fps = fps;
    project.settings.drop_frame = drop_frame && fps.is_ntsc_drop_eligible();
    project.notes = title;

    let mut media_map: HashMap<String, MediaItem> = HashMap::new();
    let mut video_tracks: Vec<Track> = Vec::new();
    let mut audio_tracks: Vec<Track> = Vec::new();
    let min_len = fps.frame_to_ticks(1).max(Ticks::from_millis_i64(1));

    for entry in entries {
        let start = (entry.rec_in - base_offset).max(Ticks::ZERO);
        let raw_rec_len = entry.rec_out - entry.rec_in;
        let raw_src_len = entry.src_out - entry.src_in;
        let len = if raw_rec_len > Ticks::ZERO {
            raw_rec_len
        } else if raw_src_len > Ticks::ZERO {
            raw_src_len
        } else {
            min_len
        }
        .clamp_safe(min_len, Ticks::MAX);
        let offset = entry.src_in.clamp_safe(Ticks::ZERO, Ticks::MAX);

        let is_black = is_black_reel(&entry.reel);
        let label = entry.clip_name.clone().unwrap_or_else(|| {
            if is_black {
                "Preto (BL)".to_string()
            } else {
                entry.reel.clone()
            }
        });
        let media_key = if is_black {
            "__BLACK__".to_string()
        } else if let Some(sf) = &entry.source_file {
            format!("F:{sf}")
        } else {
            format!("R:{}:{}", entry.reel, label)
        };

        let want_v = matches!(entry.channels, EdlChannels::Video | EdlChannels::VideoAudio);
        let want_a = matches!(entry.channels, EdlChannels::Audio | EdlChannels::VideoAudio);

        let item = media_map
            .entry(media_key)
            .or_insert_with(|| {
                let source = if is_black {
                    MediaSource::Generated(GenSpec::SolidColor {
                        color: [0.0, 0.0, 0.0, 1.0],
                    })
                } else {
                    MediaSource::File {
                        path: entry
                            .source_file
                            .clone()
                            .unwrap_or_else(|| entry.reel.clone()),
                    }
                };
                MediaItem {
                    id: new_id(),
                    name: label.clone(),
                    source,
                    duration: if is_black {
                        None
                    } else {
                        Some((offset + len).max(min_len))
                    },
                    has_video: want_v || is_black,
                    has_audio: want_a && !is_black,
                    width: project.settings.width,
                    height: project.settings.height,
                    fps: Some(fps),
                    info: format!("EDL Reel {}", entry.reel),
                    proxy_path: None,
                }
            })
            .clone();

        // Garante flags has_video/has_audio coerentes se um mesmo reel aparece em linhas V e A.
        if let Some(stored) = media_map.values_mut().find(|m| m.id == item.id) {
            if want_v {
                stored.has_video = true;
            }
            if want_a && !is_black {
                stored.has_audio = true;
            }
            if !is_black {
                stored.duration = Some(stored.duration.unwrap_or(Ticks::ZERO).max(offset + len));
            }
        }

        let group_id = if want_v && want_a {
            Some(new_id())
        } else {
            None
        };
        let tr_spec = transition_from_edl_mode(entry.mode);

        if want_v {
            place_edl_event(
                &mut video_tracks,
                TrackKind::Video,
                &item,
                &label,
                start,
                len,
                offset,
                entry.speed_rate,
                group_id,
                tr_spec,
                fps,
            );
        }
        if want_a && !is_black {
            place_edl_event(
                &mut audio_tracks,
                TrackKind::Audio,
                &item,
                &label,
                start,
                len,
                offset,
                entry.speed_rate,
                group_id,
                tr_spec,
                fps,
            );
        }
    }

    for (t, label) in raw_markers {
        project.markers.push(Marker {
            id: new_id(),
            t: (t - base_offset).clamp_safe(Ticks::ZERO, Ticks::from_raw(MAX_TICKS)),
            label,
        });
    }

    let mut media_vec: Vec<MediaItem> = media_map.into_values().collect();
    media_vec.sort_by_key(|m| m.id);
    project.media = media_vec;
    project.tracks.extend(video_tracks);
    project.tracks.extend(audio_tracks);
    if project.tracks.is_empty() {
        project.tracks.push(Track::new(TrackKind::Video, "Vídeo 1"));
    }

    warnings.extend(project.sanitize());
    crate::model::bump_ids(project.max_id());
    Ok((project, warnings))
}

#[allow(clippy::too_many_arguments)]
fn place_edl_event(
    tracks: &mut Vec<Track>,
    kind: TrackKind,
    item: &MediaItem,
    label: &str,
    start: Ticks,
    len: Ticks,
    offset: Ticks,
    speed_rate: Option<Rate>,
    group: Option<Id>,
    transition: Option<(TransitionKind, u64)>,
    fps: FrameRate,
) {
    let mut ev = Event::new(item, start, len);
    ev.name = label.to_string();
    ev.offset = offset;
    ev.group = group;
    if let Some(r) = speed_rate {
        ev.rate = r;
    }
    if matches!(item.source, MediaSource::Generated(_)) {
        ev.looped = false;
    }
    if tracks.is_empty() {
        let prefix = if kind == TrackKind::Video {
            "Vídeo"
        } else {
            "Áudio"
        };
        tracks.push(Track::new(kind, &format!("{prefix} 1")));
    }

    // Se houver transição (D/W) exatamente na junção após o último evento da trilha 0,
    // estende o evento anterior pela duração da transição para formar crossfade VEGAS.
    if let Some((tkind, dur_frames)) = transition {
        if let Some(prev) = tracks[0].events.last_mut() {
            let trans_dur = fps.frame_to_ticks(dur_frames.max(1) as i64);
            if prev.end() <= start + fps.frame_to_ticks(1) {
                prev.length =
                    (prev.length + trans_dur).clamp_safe(Ticks::from_millis_i64(1), Ticks::MAX);
                ev.transition = Some(TransitionSpec {
                    kind: tkind,
                    softness: 0.0,
                });
                tracks[0].events.push(ev);
                return;
            }
        }
        ev.transition = Some(TransitionSpec {
            kind: tkind,
            softness: 0.0,
        });
    }

    // Coloca na primeira trilha sem colisão rígida (até 8 faixas).
    let lane = tracks
        .iter()
        .position(|tr| tr.events.last().is_none_or(|last| last.end() <= start));
    match lane {
        Some(idx) => tracks[idx].events.push(ev),
        None if tracks.len() < 8 => {
            let prefix = if kind == TrackKind::Video {
                "Vídeo"
            } else {
                "Áudio"
            };
            let mut new_tr = Track::new(kind, &format!("{prefix} {}", tracks.len() + 1));
            new_tr.events.push(ev);
            tracks.push(new_tr);
        }
        None => tracks[0].events.push(ev),
    }
}

fn edl_reel_for_media(
    media: &MediaItem,
    reel_cache: &mut HashMap<Id, String>,
    next_idx: &mut usize,
) -> String {
    if let Some(r) = reel_cache.get(&media.id) {
        return r.clone();
    }
    if matches!(
        media.source,
        MediaSource::Generated(GenSpec::SolidColor { color }) if color[0] < 0.01 && color[1] < 0.01 && color[2] < 0.01
    ) {
        reel_cache.insert(media.id, "BL".into());
        return "BL".into();
    }
    *next_idx += 1;
    let reel = format!("R{:03}", *next_idx);
    reel_cache.insert(media.id, reel.clone());
    reel
}

fn format_edl_tc(t: Ticks, fps: FrameRate, drop_frame: bool) -> String {
    let fmt = if drop_frame && fps.is_ntsc_drop_eligible() {
        TimeFormat::SmpteDrop
    } else {
        TimeFormat::SmpteNonDrop
    };
    format_ticks(t.max(Ticks::ZERO), fps, 48_000, fmt)
}

/// Exporta o projeto Neoveg para lista EDL CMX 3600.
pub fn export_edl_string(project: &Project, title_override: Option<&str>) -> String {
    let fps = project.settings.fps;
    let drop = project.settings.drop_frame && fps.is_ntsc_drop_eligible();
    let title = title_override
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let t = project.notes.trim();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        })
        .unwrap_or("Neoveg Timeline");

    let mut out = String::new();
    out.push_str(&format!("TITLE: {title}\n"));
    out.push_str(if drop {
        "FCM: DROP FRAME\n\n"
    } else {
        "FCM: NON-DROP FRAME\n\n"
    });

    // Mapa auxiliar para juntar eventos V+A de mesmo grupo e mesmo intervalo temporal numa linha `B`.
    let mut group_partners: HashMap<(Id, TrackKind), &Event> = HashMap::new();
    for tr in &project.tracks {
        for ev in &tr.events {
            if let Some(g) = ev.group {
                group_partners.entry((g, tr.kind)).or_insert(ev);
            }
        }
    }

    #[derive(Clone)]
    struct ExportRow<'a> {
        chan: &'static str,
        ev: &'a Event,
        overlap_next_start: Option<Ticks>,
    }

    let mut rows: Vec<ExportRow<'_>> = Vec::new();
    let mut consumed_audio_groups: Vec<Id> = Vec::new();

    // Pista principal de vídeo + eventuais secundárias.
    for tr in project.tracks.iter().filter(|t| t.kind == TrackKind::Video) {
        let mut sorted: Vec<&Event> = tr.events.iter().collect();
        sorted.sort_by_key(|e| (e.start, e.id));
        for (i, ev) in sorted.iter().enumerate() {
            let chan = if let Some(g) = ev.group {
                if let Some(a_ev) = group_partners.get(&(g, TrackKind::Audio)) {
                    if a_ev.start == ev.start
                        && a_ev.length == ev.length
                        && a_ev.offset == ev.offset
                    {
                        consumed_audio_groups.push(g);
                        "B"
                    } else {
                        "V"
                    }
                } else {
                    "V"
                }
            } else {
                "V"
            };
            let next_overlap = sorted
                .get(i + 1)
                .filter(|nxt| nxt.start < ev.end() && nxt.start > ev.start)
                .map(|nxt| nxt.start);
            rows.push(ExportRow {
                chan,
                ev,
                overlap_next_start: next_overlap,
            });
        }
    }

    for tr in project.tracks.iter().filter(|t| t.kind == TrackKind::Audio) {
        let mut sorted: Vec<&Event> = tr.events.iter().collect();
        sorted.sort_by_key(|e| (e.start, e.id));
        for (i, ev) in sorted.iter().enumerate() {
            if ev.group.is_some_and(|g| consumed_audio_groups.contains(&g)) {
                continue;
            }
            let next_overlap = sorted
                .get(i + 1)
                .filter(|nxt| nxt.start < ev.end() && nxt.start > ev.start)
                .map(|nxt| nxt.start);
            rows.push(ExportRow {
                chan: "AA",
                ev,
                overlap_next_start: next_overlap,
            });
        }
    }

    rows.sort_by_key(|r| (r.ev.start, r.chan, r.ev.id));

    let mut reel_cache: HashMap<Id, String> = HashMap::new();
    let mut reel_seq = 0usize;
    let mut edit_num = 0usize;
    let mut prev_end_by_chan: HashMap<&str, Ticks> = HashMap::new();

    for row in &rows {
        edit_num += 1;
        let media = project.media(row.ev.media_id);
        let reel = media
            .map(|m| edl_reel_for_media(m, &mut reel_cache, &mut reel_seq))
            .unwrap_or_else(|| "AX".into());

        // Verifica se há sobreposição/crossfade com a entrada anterior desse canal.
        let prev_end = prev_end_by_chan.get(row.chan).copied();
        let is_dissolve = prev_end.is_some_and(|pe| pe > row.ev.start);
        let (mode_str, dur_str) = if is_dissolve {
            let ov_ticks = (prev_end.unwrap_or(row.ev.start) - row.ev.start).max(Ticks::ZERO);
            let frames = fps.ticks_to_frame_round(ov_ticks).clamp(1, 999);
            let mode = match row.ev.transition.as_ref().map(|t| t.kind) {
                Some(TransitionKind::WipeLeft) => "W001",
                Some(TransitionKind::WipeRight) => "W002",
                Some(TransitionKind::WipeUp) => "W003",
                Some(TransitionKind::WipeDown) => "W004",
                _ => "D",
            };
            (mode, format!("{frames:03}"))
        } else {
            ("C", String::new())
        };

        // Para preservação exata de crossfades, o evento anterior encerra rec_out no início da sobreposição.
        let rec_end = row.overlap_next_start.unwrap_or_else(|| row.ev.end());
        let visible_rec_dur = (rec_end - row.ev.start).max(Ticks::from_millis_i64(1));
        let src_dur = row.ev.rate.scale_ticks(visible_rec_dur);

        let src_in = format_edl_tc(row.ev.offset, fps, drop);
        let src_out = format_edl_tc(row.ev.offset + src_dur, fps, drop);
        let rec_in = format_edl_tc(row.ev.start, fps, drop);
        let rec_out = format_edl_tc(rec_end, fps, drop);

        if dur_str.is_empty() {
            out.push_str(&format!(
                "{edit_num:03}  {reel:<8} {chan:<5} {mode_str:<4}     {src_in} {src_out} {rec_in} {rec_out}\n",
                chan = row.chan
            ));
        } else {
            out.push_str(&format!(
                "{edit_num:03}  {reel:<8} {chan:<5} {mode_str:<4} {dur_str} {src_in} {src_out} {rec_in} {rec_out}\n",
                chan = row.chan
            ));
        }

        out.push_str(&format!("* FROM CLIP NAME: {}\n", row.ev.name));
        if let Some(m) = media {
            match &m.source {
                MediaSource::File { path } if !path.trim().is_empty() => {
                    out.push_str(&format!("* SOURCE FILE: {path}\n"));
                }
                _ => {}
            }
        }
        if !row.ev.rate.is_normal() {
            let eff_fps = row.ev.rate.as_f64() * fps.as_f64();
            out.push_str(&format!(
                "M2   {reel:<8} {eff_fps:>6.2}          {src_in}\n"
            ));
        }

        prev_end_by_chan.insert(row.chan, row.ev.end());
    }

    for mk in &project.markers {
        out.push_str(&format!(
            "* LOC: {} YELLOW {}\n",
            format_edl_tc(mk.t, fps, drop),
            mk.label
        ));
    }
    out
}

pub fn import_edl_file(path: &Path, fps: FrameRate) -> Result<(Project, Vec<String>), NeovegError> {
    let text = crate::otio::read_bounded_utf8_file(path, MAX_EDL_BYTES, "EDL")?;
    import_edl_str(&text, fps)
}

pub fn export_edl_file(
    project: &Project,
    path: &Path,
    title_override: Option<&str>,
) -> Result<(), NeovegError> {
    let body = export_edl_string(project, title_override);
    std::fs::write(path, body.as_bytes())
        .map_err(|e| NeovegError::Io(format!("erro ao gravar '{}': {e}", path.display())))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edl_cmx3600_hour1_dissolve_markers_and_roundtrip() {
        let edl = "\
TITLE: Montagem Teste
FCM: NON-DROP FRAME

001  001      V     C        00:00:02:00 00:00:07:00 01:00:00:00 01:00:04:00
* FROM CLIP NAME: Cena A
* SOURCE FILE: C:/midia/cena_a.mp4
002  002      V     D    024 00:00:00:00 00:00:04:00 01:00:04:00 01:00:08:00
* FROM CLIP NAME: Cena B
* LOC: 01:00:03:00 YELLOW Corte Musical
003  001      AA    C        00:00:02:00 00:00:06:00 01:00:00:00 01:00:04:00
* FROM CLIP NAME: Som A
";
        let (p, warn) = import_edl_str(edl, FrameRate::FPS_24).expect("parse edl");
        assert!(
            warn.iter().any(|w| w.contains("01:00:00:00")),
            "avisos esperados: {warn:?}"
        );
        assert_eq!(p.notes, "Montagem Teste");
        assert_eq!(p.markers.len(), 1);
        assert_eq!(p.markers[0].t, Ticks::from_secs_i64(3));
        assert_eq!(p.markers[0].label, "Corte Musical");

        let v_tr = p
            .tracks
            .iter()
            .find(|t| t.kind == TrackKind::Video)
            .expect("trilha de vídeo");
        assert_eq!(v_tr.events.len(), 2);
        assert_eq!(v_tr.events[0].start, Ticks::ZERO);
        assert_eq!(v_tr.events[0].length, Ticks::from_secs_i64(5));
        assert_eq!(v_tr.events[1].start, Ticks::from_secs_i64(4));
        assert_eq!(v_tr.events[1].length, Ticks::from_secs_i64(4));
        assert_eq!(
            v_tr.events[1].transition.as_ref().map(|t| t.kind),
            Some(TransitionKind::Dissolve)
        );

        let exported = export_edl_string(&p, None);
        assert!(exported.contains("TITLE: Montagem Teste"));
        assert!(exported.contains("D    024"));
        let (re_p, _) = import_edl_str(&exported, FrameRate::FPS_24).expect("roundtrip edl");
        let re_v = re_p
            .tracks
            .iter()
            .find(|t| t.kind == TrackKind::Video)
            .expect("v");
        assert_eq!(re_v.events.len(), 2);
        assert_eq!(re_v.events[0].length, Ticks::from_secs_i64(5));
        assert_eq!(re_v.events[1].start, Ticks::from_secs_i64(4));
    }

    #[test]
    fn edl_drop_frame_semicolon_roundtrip() {
        let edl = "TITLE: NTSC DF\nFCM: DROP FRAME\n001  AX       B     C        00:00:00;00 00:00:10;00 00:00:00;00 00:00:10;00\n";
        let (p, _) = import_edl_str(edl, FrameRate::FPS_29_97).expect("parse df");
        assert!(p.settings.drop_frame);
        let exp = export_edl_string(&p, None);
        assert!(exp.contains("FCM: DROP FRAME"));
        assert!(exp.contains("B"));
    }
}
