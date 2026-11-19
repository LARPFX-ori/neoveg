//! Importação e exportação em Sala Limpa do formato **OpenTimelineIO (.otio)**.
//!
//! Implementado estritamente a partir da especificação pública OpenTimelineIO 0.17
//! (esquemas JSON versionados `.1` / `.2`), sem leitura de código-fonte de
//! terceiros (`AGENTS.md` §2.3). Toda entrada é tratada como hostil (§0.4–§0.5).

use crate::model::{
    Event, FrameRate, GenSpec, Marker, MediaItem, MediaSource, Project, Rate, TextSpec, Track,
    TrackKind, TransitionKind, TransitionSpec, MAX_SECONDS, MAX_TICKS, TICKS_PER_SEC,
};
use crate::time::Ticks;
use neoveg_common::error::NeovegError;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// Limite máximo de tamanho para arquivos `.otio` lidos em memória (16 MiB).
pub const MAX_OTIO_BYTES: usize = 16 * 1024 * 1024;
/// Profundidade máxima de recursão para `Stack`s aninhados (§0.5).
pub const MAX_OTIO_STACK_DEPTH: usize = 32;
/// Limite máximo de trilhas importadas de um único arquivo `.otio`.
pub const MAX_OTIO_TRACKS: usize = 256;
/// Limite máximo de itens (clips/gaps/transições) por trilha.
pub const MAX_OTIO_ITEMS_PER_TRACK: usize = 50_000;
const MAX_NAME_BYTES: usize = 256;
const MAX_URL_BYTES: usize = 4096;
const DEFAULT_CLIP_MS: i64 = 5_000;

fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

fn clean_name(raw: Option<&str>, fallback: &str) -> String {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => truncate_utf8(s, MAX_NAME_BYTES),
        None => fallback.to_string(),
    }
}

fn schema_matches(v: &Value, prefix: &str) -> bool {
    v.get("OTIO_SCHEMA")
        .and_then(Value::as_str)
        .is_some_and(|s| s.starts_with(prefix))
}

pub(crate) fn div_round_i128(num: i128, den: i128) -> i128 {
    if den <= 0 {
        return 0;
    }
    if num >= 0 {
        (num + den / 2) / den
    } else {
        -(((-num) + den / 2) / den)
    }
}

pub(crate) fn clamp_i128_ticks(v: i128) -> Ticks {
    Ticks::from_raw(v.clamp(-MAX_TICKS as i128, MAX_TICKS as i128) as i64)
}

fn parse_rational_time(v: &Value) -> Option<(Ticks, f64)> {
    let obj = v.as_object()?;
    if let Some(schema) = obj.get("OTIO_SCHEMA").and_then(Value::as_str) {
        if !schema.starts_with("RationalTime.") {
            return None;
        }
    }
    let value = obj.get("value").and_then(Value::as_f64)?;
    let rate = obj.get("rate").and_then(Value::as_f64)?;
    if !value.is_finite() || !rate.is_finite() || rate <= 0.0 || rate > 1e8 {
        return None;
    }

    let rate_round = rate.round();
    let val_round = value.round();
    if (rate - rate_round).abs() < 1e-7
        && (1.0..=10_000_000.0).contains(&rate_round)
        && (value - val_round).abs() < 1e-7
        && val_round.abs() <= (MAX_SECONDS as f64 * rate_round).min((i64::MAX / 4) as f64)
    {
        let num = (val_round as i128).checked_mul(TICKS_PER_SEC as i128)?;
        let den = rate_round as i128;
        return Some((clamp_i128_ticks(div_round_i128(num, den)), rate));
    }

    if let Some(fr) = FrameRate::from_f64(rate) {
        if (value - val_round).abs() < 1e-7 && val_round.abs() <= (MAX_SECONDS as f64 * 1000.0) {
            return Some((fr.frame_to_ticks(val_round as i64), rate));
        }
    }

    let secs = value / rate;
    if !secs.is_finite() {
        return None;
    }
    let clamped_secs = secs.clamp(-(MAX_SECONDS as f64), MAX_SECONDS as f64);
    let ticks_f = (clamped_secs * TICKS_PER_SEC as f64).round();
    if !ticks_f.is_finite() {
        return None;
    }
    let ticks_i = ticks_f.clamp(-(MAX_TICKS as f64), MAX_TICKS as f64) as i64;
    Some((Ticks::from_raw(ticks_i), rate))
}

fn parse_time_range(v: &Value) -> Option<(Ticks, Ticks, f64)> {
    let obj = v.as_object()?;
    if let Some(schema) = obj.get("OTIO_SCHEMA").and_then(Value::as_str) {
        if !schema.starts_with("TimeRange.") {
            return None;
        }
    }
    let (start, r1) = parse_rational_time(obj.get("start_time")?)?;
    let (dur, r2) = parse_rational_time(obj.get("duration")?)?;
    let rate = if r2 > 0.0 { r2 } else { r1 };
    Some((start, dur, rate))
}

fn percent_decode_bytes(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push(((h << 4) | l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub(crate) fn decode_target_url(raw: &str) -> String {
    let bounded = truncate_utf8(raw.trim(), MAX_URL_BYTES);
    let rest = if let Some(s) = bounded.strip_prefix("file://localhost/") {
        s
    } else if let Some(s) = bounded.strip_prefix("file:///") {
        s
    } else if let Some(s) = bounded.strip_prefix("file://") {
        s
    } else {
        return percent_decode_bytes(&bounded);
    };
    let decoded = percent_decode_bytes(rest);
    let b = decoded.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        decoded
    } else if bounded.starts_with("file:///") {
        format!("/{decoded}")
    } else {
        decoded
    }
}

pub(crate) fn encode_target_url(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with("file://") || normalized.contains("://") {
        return normalized;
    }
    let p = Path::new(path);
    if p.is_absolute() {
        let trimmed = normalized.trim_start_matches('/');
        format!("file:///{}", encode_url_spaces(trimmed))
    } else {
        normalized
    }
}

fn encode_url_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            '%' => out.push_str("%25"),
            _ => out.push(ch),
        }
    }
    out
}

fn parse_transition_kind(v: &Value) -> TransitionKind {
    if let Some(meta_kind) = v
        .get("metadata")
        .and_then(|m| m.get("neoveg_transition"))
        .and_then(Value::as_str)
    {
        for k in TransitionKind::ALL {
            if format!("{k:?}").eq_ignore_ascii_case(meta_kind) {
                return k;
            }
        }
    }
    let ty = v
        .get("transition_type")
        .and_then(Value::as_str)
        .unwrap_or("SMPTE_Dissolve");
    if ty.eq_ignore_ascii_case("SMPTE_Dissolve") || ty.eq_ignore_ascii_case("dissolve") {
        TransitionKind::Dissolve
    } else if ty.contains("FadeThroughBlack") || ty.contains("DipToBlack") {
        TransitionKind::FadeThroughBlack
    } else if ty.contains("WipeLeft") {
        TransitionKind::WipeLeft
    } else if ty.contains("WipeRight") {
        TransitionKind::WipeRight
    } else if ty.contains("WipeUp") {
        TransitionKind::WipeUp
    } else if ty.contains("WipeDown") {
        TransitionKind::WipeDown
    } else {
        TransitionKind::Dissolve
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
enum MediaCacheKey {
    File(String),
    Generator(String),
    Missing(String),
}

struct ImportCtx {
    media: Vec<MediaItem>,
    media_idx: HashMap<MediaCacheKey, usize>,
    detected_rates: Vec<f64>,
    warnings: Vec<String>,
}

impl ImportCtx {
    fn new() -> Self {
        Self {
            media: Vec::new(),
            media_idx: HashMap::new(),
            detected_rates: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn record_rate(&mut self, r: f64) {
        if r.is_finite() && (0.1..=1000.0).contains(&r) {
            self.detected_rates.push(r);
        }
    }

    fn get_or_insert_media(
        &mut self,
        clip_name: &str,
        kind_hint: TrackKind,
        ref_val: Option<&Value>,
        fallback_dur: Option<Ticks>,
    ) -> &MediaItem {
        let mut avail_dur = fallback_dur;
        if let Some(rv) = ref_val {
            if let Some(ar) = rv.get("available_range") {
                if let Some((_, dur, rate)) = parse_time_range(ar) {
                    self.record_rate(rate);
                    if dur > Ticks::ZERO {
                        avail_dur = Some(dur);
                    }
                }
            }
        }

        let schema = ref_val
            .and_then(|rv| rv.get("OTIO_SCHEMA"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let ref_name = ref_val
            .and_then(|rv| rv.get("name"))
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty());

        let (key, source, name, dur, info) = if schema.starts_with("GeneratorReference.") {
            let gen_kind = ref_val
                .and_then(|rv| rv.get("generator_kind"))
                .and_then(Value::as_str)
                .unwrap_or("SolidColor");
            let spec = parse_generator_spec(gen_kind, ref_val, clip_name);
            let label = clean_name(ref_name.or(Some(clip_name)), &spec.label());
            let cache_key = MediaCacheKey::Generator(format!("{gen_kind}:{label}"));
            (
                cache_key,
                MediaSource::Generated(spec),
                label,
                None,
                format!("OTIO Generator: {gen_kind}"),
            )
        } else if schema.starts_with("ExternalReference.")
            || ref_val.and_then(|r| r.get("target_url")).is_some()
        {
            let raw_url = ref_val
                .and_then(|rv| rv.get("target_url"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let decoded = decode_target_url(raw_url);
            if decoded.is_empty() {
                let label = clean_name(ref_name.or(Some(clip_name)), "Mídia sem URL");
                (
                    MediaCacheKey::Missing(label.clone()),
                    MediaSource::File {
                        path: label.clone(),
                    },
                    label,
                    avail_dur,
                    "Referência externa sem target_url".to_string(),
                )
            } else {
                let fname = Path::new(&decoded)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .filter(|s| !s.is_empty());
                let label = clean_name(ref_name.or(fname.as_deref()).or(Some(clip_name)), &decoded);
                (
                    MediaCacheKey::File(decoded.clone()),
                    MediaSource::File { path: decoded },
                    label,
                    avail_dur,
                    String::new(),
                )
            }
        } else {
            let label = clean_name(ref_name.or(Some(clip_name)), "Mídia ausente (OTIO)");
            self.warnings.push(format!(
                "clipe \"{clip_name}\" referencia mídia ausente/desconhecida ({schema})"
            ));
            (
                MediaCacheKey::Missing(label.clone()),
                MediaSource::File {
                    path: label.clone(),
                },
                label,
                avail_dur,
                "OTIO MissingReference".to_string(),
            )
        };

        let idx = if let Some(&existing) = self.media_idx.get(&key) {
            existing
        } else {
            let i = self.media.len();
            self.media_idx.insert(key, i);
            self.media.push(MediaItem {
                id: crate::model::new_id(),
                name,
                source,
                duration: dur,
                has_video: kind_hint == TrackKind::Video,
                has_audio: kind_hint == TrackKind::Audio,
                width: 0,
                height: 0,
                fps: None,
                info,
                proxy_path: None,
            });
            i
        };
        if let Some(m) = self.media.get_mut(idx) {
            if kind_hint == TrackKind::Video {
                m.has_video = true;
            } else {
                m.has_audio = true;
            }
            if let Some(new_d) = avail_dur {
                if m.duration.is_some_and(|d| new_d > d) {
                    m.duration = Some(new_d);
                }
            }
        }
        &self.media[idx]
    }
}

fn parse_generator_spec(gen_kind: &str, ref_val: Option<&Value>, clip_name: &str) -> GenSpec {
    if gen_kind.eq_ignore_ascii_case("SMPTEBars")
        || gen_kind.eq_ignore_ascii_case("ColorBars")
        || gen_kind.to_ascii_lowercase().contains("bars")
    {
        return GenSpec::ColorBars;
    }
    if gen_kind.eq_ignore_ascii_case("Checkerboard") {
        return GenSpec::Checkerboard {
            a: [0.1, 0.1, 0.1, 1.0],
            b: [0.9, 0.9, 0.9, 1.0],
            cells: 8,
        };
    }
    if gen_kind.eq_ignore_ascii_case("Text") || gen_kind.eq_ignore_ascii_case("Title") {
        let txt = ref_val
            .and_then(|rv| rv.get("parameters"))
            .and_then(|p| p.get("text"))
            .and_then(Value::as_str)
            .unwrap_or(clip_name);
        return GenSpec::Text(TextSpec {
            text: truncate_utf8(txt, MAX_NAME_BYTES),
            ..TextSpec::default()
        });
    }
    let mut color = [0.0f32, 0.0, 0.0, 1.0];
    if let Some(arr) = ref_val
        .and_then(|rv| rv.get("parameters"))
        .and_then(|p| p.get("color"))
        .and_then(Value::as_array)
    {
        for (i, slot) in color.iter_mut().enumerate() {
            if let Some(val) = arr.get(i).and_then(Value::as_f64) {
                if val.is_finite() {
                    *slot = val.clamp(0.0, 1.0) as f32;
                }
            }
        }
    }
    GenSpec::SolidColor { color }
}

fn resolve_clip_media_ref(clip: &Value) -> Option<&Value> {
    if let Some(refs) = clip.get("media_references").and_then(Value::as_object) {
        let key = clip
            .get("active_media_reference_key")
            .and_then(Value::as_str)
            .unwrap_or("DEFAULT_MEDIA");
        if let Some(r) = refs.get(key) {
            return Some(r);
        }
        if let Some(r) = refs.get("DEFAULT_MEDIA") {
            return Some(r);
        }
        if let Some((_, r)) = refs.iter().next() {
            return Some(r);
        }
    }
    clip.get("media_reference")
}

fn apply_clip_effects(clip: &Value, ev: &mut Event) {
    let Some(effects) = clip.get("effects").and_then(Value::as_array) else {
        return;
    };
    for eff in effects.iter().take(64) {
        let schema = eff.get("OTIO_SCHEMA").and_then(Value::as_str).unwrap_or("");
        if schema.starts_with("FreezeFrame.") {
            ev.rate = Rate::FROZEN;
        } else if schema.starts_with("LinearTimeWarp.") {
            if let Some(scalar) = eff.get("time_scalar").and_then(Value::as_f64) {
                if scalar.is_finite() {
                    if scalar < 0.0 {
                        ev.reverse = true;
                        let mag = (-scalar).clamp(0.0, 100.0);
                        if let Some(r) = Rate::from_f64(mag) {
                            ev.rate = r;
                        }
                    } else if let Some(r) = Rate::from_f64(scalar.clamp(0.0, 100.0)) {
                        ev.rate = r;
                    }
                }
            }
        }
    }
}

fn extract_item_markers(item: &Value, ctx: &mut ImportCtx) -> Vec<(Ticks, String)> {
    let mut out = Vec::new();
    let Some(arr) = item.get("markers").and_then(Value::as_array) else {
        return out;
    };
    for m in arr.iter().take(4096) {
        if let Some(mr) = m.get("marked_range") {
            if let Some((start, _, rate)) = parse_time_range(mr) {
                ctx.record_rate(rate);
                let label = clean_name(m.get("name").and_then(Value::as_str), "Marcador");
                out.push((start, label));
            }
        }
    }
    out
}

struct PendingTransition {
    prev_event_idx: usize,
    cut_pos: Ticks,
    in_offset: Ticks,
    out_offset: Ticks,
    kind: TransitionKind,
}

fn import_track(
    track_val: &Value,
    default_idx: usize,
    ctx: &mut ImportCtx,
    markers_out: &mut Vec<Marker>,
) -> Track {
    let kind = match track_val.get("kind").and_then(Value::as_str) {
        Some(k) if k.eq_ignore_ascii_case("Audio") => TrackKind::Audio,
        _ => TrackKind::Video,
    };
    let fallback = match kind {
        TrackKind::Video => format!("Vídeo {default_idx}"),
        TrackKind::Audio => format!("Áudio {default_idx}"),
    };
    let name = clean_name(track_val.get("name").and_then(Value::as_str), &fallback);
    let mut track = Track::new(kind, &name);

    for (t, label) in extract_item_markers(track_val, ctx) {
        if t >= Ticks::ZERO {
            markers_out.push(Marker {
                id: crate::model::new_id(),
                t: t.clamp_safe(Ticks::ZERO, Ticks::MAX),
                label,
                ..Default::default()
            });
        }
    }

    let Some(children) = track_val.get("children").and_then(Value::as_array) else {
        return track;
    };
    if children.len() > MAX_OTIO_ITEMS_PER_TRACK {
        ctx.warnings.push(format!(
            "trilha \"{name}\" truncada em {MAX_OTIO_ITEMS_PER_TRACK} itens"
        ));
    }

    let mut cursor = Ticks::ZERO;
    let mut last_ev_idx: Option<usize> = None;
    let mut pending_tr: Option<PendingTransition> = None;
    let min_ev_len = Ticks::from_millis_i64(1);

    for child in children.iter().take(MAX_OTIO_ITEMS_PER_TRACK) {
        let schema = child
            .get("OTIO_SCHEMA")
            .and_then(Value::as_str)
            .unwrap_or("");
        if schema.starts_with("Gap.") {
            let dur = child
                .get("source_range")
                .and_then(parse_time_range)
                .map(|(_, d, r)| {
                    ctx.record_rate(r);
                    d
                })
                .unwrap_or(Ticks::ZERO);
            if dur > Ticks::ZERO {
                cursor += dur;
            }
            last_ev_idx = None;
            pending_tr = None;
        } else if schema.starts_with("Transition.") {
            let in_off = child
                .get("in_offset")
                .and_then(parse_rational_time)
                .map(|(t, r)| {
                    ctx.record_rate(r);
                    t.max(Ticks::ZERO)
                })
                .unwrap_or(Ticks::ZERO);
            let out_off = child
                .get("out_offset")
                .and_then(parse_rational_time)
                .map(|(t, r)| {
                    ctx.record_rate(r);
                    t.max(Ticks::ZERO)
                })
                .unwrap_or(Ticks::ZERO);
            let tkind = parse_transition_kind(child);
            if let Some(prev_idx) = last_ev_idx {
                pending_tr = Some(PendingTransition {
                    prev_event_idx: prev_idx,
                    cut_pos: cursor,
                    in_offset: in_off,
                    out_offset: out_off,
                    kind: tkind,
                });
            }
        } else if schema.starts_with("Clip.") {
            let clip_name = clean_name(child.get("name").and_then(Value::as_str), "Clipe");
            let mref = resolve_clip_media_ref(child);

            let (src_start, src_dur) = if let Some(sr) = child.get("source_range") {
                match parse_time_range(sr) {
                    Some((st, d, r)) => {
                        ctx.record_rate(r);
                        (st.max(Ticks::ZERO), Some(d))
                    }
                    None => (Ticks::ZERO, None),
                }
            } else {
                (Ticks::ZERO, None)
            };

            let avail_from_ref = mref
                .and_then(|r| r.get("available_range"))
                .and_then(parse_time_range)
                .map(|(_, d, rate)| {
                    ctx.record_rate(rate);
                    d
                })
                .filter(|d| *d > Ticks::ZERO);

            let clip_markers = extract_item_markers(child, ctx);
            let len = src_dur
                .or(avail_from_ref)
                .unwrap_or_else(|| Ticks::from_millis_i64(DEFAULT_CLIP_MS))
                .clamp_safe(min_ev_len, Ticks::MAX);
            let ev_end_cursor = (cursor + len).clamp_safe(min_ev_len, Ticks::MAX);

            let media_ref_item = ctx
                .get_or_insert_media(&clip_name, kind, mref, avail_from_ref)
                .clone();

            let mut ev = Event::new(&media_ref_item, cursor, len);
            ev.name = clip_name;
            ev.offset = src_start.clamp_safe(Ticks::ZERO, Ticks::MAX);
            apply_clip_effects(child, &mut ev);

            if let Some(tr) = pending_tr.take() {
                if tr.cut_pos == cursor {
                    let applied_in = tr.in_offset.min(ev.start);
                    if applied_in > Ticks::ZERO {
                        ev.start -= applied_in;
                        ev.length = (ev.length + applied_in).clamp_safe(min_ev_len, Ticks::MAX);
                        let media_in = ev.rate.scale_ticks(applied_in);
                        ev.offset = (ev.offset - media_in).max(Ticks::ZERO);
                    }
                    if let Some(prev) = track.events.get_mut(tr.prev_event_idx) {
                        if tr.out_offset > Ticks::ZERO {
                            prev.length =
                                (prev.length + tr.out_offset).clamp_safe(min_ev_len, Ticks::MAX);
                        }
                    }
                    ev.transition = Some(TransitionSpec {
                        kind: tr.kind,
                        softness: 0.0,
                    });
                }
            }

            let media_src_origin = src_start;
            for (m_pos, m_label) in clip_markers {
                let rel_media = m_pos - media_src_origin;
                if rel_media >= Ticks::ZERO {
                    let rel_tl = ev.rate.unscale_ticks(rel_media).unwrap_or(rel_media);
                    let tl_t = (cursor + rel_tl).clamp_safe(Ticks::ZERO, Ticks::MAX);
                    markers_out.push(Marker {
                        id: crate::model::new_id(),
                        t: tl_t,
                        label: m_label,
                        ..Default::default()
                    });
                }
            }

            let new_idx = track.events.len();
            track.events.push(ev);
            last_ev_idx = Some(new_idx);
            cursor = ev_end_cursor;
        }
    }

    track
}

fn collect_stack_tracks(
    stack_val: &Value,
    depth: usize,
    ctx: &mut ImportCtx,
    tracks_out: &mut Vec<Track>,
    markers_out: &mut Vec<Marker>,
) -> Result<(), NeovegError> {
    if depth >= MAX_OTIO_STACK_DEPTH {
        ctx.warnings.push(format!(
            "profundidade máxima de Stack OTIO ({MAX_OTIO_STACK_DEPTH}) atingida; subárvore ignorada"
        ));
        return Ok(());
    }

    for (t, label) in extract_item_markers(stack_val, ctx) {
        if t >= Ticks::ZERO {
            markers_out.push(Marker {
                id: crate::model::new_id(),
                t: t.clamp_safe(Ticks::ZERO, Ticks::MAX),
                label,
                ..Default::default()
            });
        }
    }

    let Some(children) = stack_val.get("children").and_then(Value::as_array) else {
        return Ok(());
    };
    for child in children {
        if tracks_out.len() >= MAX_OTIO_TRACKS {
            ctx.warnings
                .push(format!("limite de {MAX_OTIO_TRACKS} trilhas OTIO atingido"));
            break;
        }
        if schema_matches(child, "Track.") {
            let tr = import_track(child, tracks_out.len() + 1, ctx, markers_out);
            tracks_out.push(tr);
        } else if schema_matches(child, "Stack.") {
            collect_stack_tracks(child, depth + 1, ctx, tracks_out, markers_out)?;
        }
    }
    Ok(())
}

/// Faz parse e valida um texto OpenTimelineIO JSON (`.otio`) em um `Project`.
pub fn import_otio_str(text: &str) -> Result<(Project, Vec<String>), NeovegError> {
    if text.len() > MAX_OTIO_BYTES {
        return Err(NeovegError::InvalidInput(format!(
            "arquivo OTIO excede o limite de {} MiB",
            MAX_OTIO_BYTES / (1024 * 1024)
        )));
    }
    let root: Value = serde_json::from_str(text)
        .map_err(|e| NeovegError::Json(format!("JSON OpenTimelineIO inválido: {e}")))?;
    let Some(root_obj) = root.as_object() else {
        return Err(NeovegError::InvalidInput(
            "raiz OpenTimelineIO deve ser um objeto JSON".into(),
        ));
    };

    let mut project = Project::new_default();
    project.tracks.clear();
    project.media.clear();
    project.markers.clear();
    project.regions.clear();
    project.notes = clean_name(root_obj.get("name").and_then(Value::as_str), "");

    let mut ctx = ImportCtx::new();

    if let Some(gst) = root_obj.get("global_start_time") {
        if let Some((_, rate)) = parse_rational_time(gst) {
            ctx.record_rate(rate);
        }
    }

    if schema_matches(&root, "Timeline.") || root_obj.contains_key("tracks") {
        let Some(tracks_val) = root_obj.get("tracks") else {
            return Err(NeovegError::InvalidInput(
                "Timeline OTIO sem campo obrigatório 'tracks'".into(),
            ));
        };
        if schema_matches(tracks_val, "Stack.") || tracks_val.get("children").is_some() {
            collect_stack_tracks(
                tracks_val,
                0,
                &mut ctx,
                &mut project.tracks,
                &mut project.markers,
            )?;
        } else if schema_matches(tracks_val, "Track.") {
            let tr = import_track(tracks_val, 1, &mut ctx, &mut project.markers);
            project.tracks.push(tr);
        } else {
            return Err(NeovegError::InvalidInput(
                "campo 'tracks' do Timeline OTIO precisa ser Stack.1 ou Track.1".into(),
            ));
        }
    } else if schema_matches(&root, "Stack.") {
        collect_stack_tracks(
            &root,
            0,
            &mut ctx,
            &mut project.tracks,
            &mut project.markers,
        )?;
    } else if schema_matches(&root, "Track.") {
        let tr = import_track(&root, 1, &mut ctx, &mut project.markers);
        project.tracks.push(tr);
    } else {
        return Err(NeovegError::InvalidInput(
            "documento não possui OTIO_SCHEMA Timeline.1/Stack.1/Track.1 reconhecido".into(),
        ));
    }

    if let Some(nv) = root_obj
        .get("metadata")
        .and_then(|m| m.get("neoveg"))
        .and_then(Value::as_object)
    {
        if let Some(w) = nv.get("width").and_then(Value::as_u64) {
            project.settings.width = (w as u32).clamp(16, crate::model::MAX_DIM);
        }
        if let Some(h) = nv.get("height").and_then(Value::as_u64) {
            project.settings.height = (h as u32).clamp(16, crate::model::MAX_DIM);
        }
        if let Some(sr) = nv.get("sample_rate").and_then(Value::as_u64) {
            if matches!(sr, 8000..=384_000) {
                project.settings.sample_rate = sr as u32;
            }
        }
        if let Some(df) = nv.get("drop_frame").and_then(Value::as_bool) {
            project.settings.drop_frame = df;
        }
        if let (Some(num), Some(den)) = (
            nv.get("fps_num").and_then(Value::as_u64),
            nv.get("fps_den").and_then(Value::as_u64),
        ) {
            if let Some(fr) = FrameRate::new(
                num.min(u32::MAX as u64) as u32,
                den.min(u32::MAX as u64) as u32,
            ) {
                project.settings.fps = fr;
            }
        }
    } else if let Some(fr) = ctx
        .detected_rates
        .iter()
        .find_map(|&r| FrameRate::from_f64(r))
    {
        project.settings.fps = fr;
        project.settings.drop_frame = fr.is_ntsc_drop_eligible();
    }

    if project.tracks.is_empty() {
        project.tracks.push(Track::new(TrackKind::Video, "Vídeo 1"));
    }
    project.media = ctx.media;

    let mut fixes = ctx.warnings;
    fixes.extend(project.sanitize());
    crate::model::bump_ids(project.max_id());
    Ok((project, fixes))
}

/// Lê um arquivo de texto UTF-8 respeitando um limite rígido de bytes (`AGENTS.md` §0.4).
pub(crate) fn read_bounded_utf8_file(
    path: &Path,
    max_bytes: usize,
    label: &str,
) -> Result<String, NeovegError> {
    use std::io::Read;

    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() > max_bytes as u64 {
            return Err(NeovegError::InvalidInput(format!(
                "arquivo {label} '{}' excede o limite de {} bytes",
                path.display(),
                max_bytes
            )));
        }
    }
    let file = std::fs::File::open(path)
        .map_err(|e| NeovegError::Io(format!("não foi possível abrir {}: {e}", path.display())))?;
    let mut buf = Vec::with_capacity(max_bytes.min(64 * 1024));
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| NeovegError::Io(format!("erro ao ler '{}': {e}", path.display())))?;
    if buf.len() > max_bytes {
        return Err(NeovegError::InvalidInput(format!(
            "arquivo {label} '{}' excede o limite de {} bytes",
            path.display(),
            max_bytes
        )));
    }
    String::from_utf8(buf).map_err(|_| {
        NeovegError::InvalidInput(format!(
            "arquivo {label} '{}' não está codificado em UTF-8 válido",
            path.display()
        ))
    })
}

/// Carrega e valida um arquivo `.otio` do disco com limite de tamanho (§0.4).
pub fn import_otio_file(path: &Path) -> Result<(Project, Vec<String>), NeovegError> {
    let text = read_bounded_utf8_file(path, MAX_OTIO_BYTES, "OTIO")?;
    import_otio_str(&text)
}

// ---------------------------------------------------------------------------
// Exportação OpenTimelineIO (.otio)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct OtioRationalTime {
    #[serde(rename = "OTIO_SCHEMA")]
    schema: &'static str,
    value: f64,
    rate: f64,
}

#[derive(Serialize)]
struct OtioTimeRange {
    #[serde(rename = "OTIO_SCHEMA")]
    schema: &'static str,
    start_time: OtioRationalTime,
    duration: OtioRationalTime,
}

#[derive(Serialize)]
struct OtioMarker {
    #[serde(rename = "OTIO_SCHEMA")]
    schema: &'static str,
    name: String,
    color: &'static str,
    marked_range: OtioTimeRange,
}

#[derive(Serialize)]
struct OtioNeovegMeta {
    width: u32,
    height: u32,
    sample_rate: u32,
    fps_num: u32,
    fps_den: u32,
    drop_frame: bool,
}

#[derive(Serialize)]
struct OtioRootMetadata {
    neoveg: OtioNeovegMeta,
}

#[derive(Serialize)]
struct OtioTimeline {
    #[serde(rename = "OTIO_SCHEMA")]
    schema: &'static str,
    name: String,
    metadata: OtioRootMetadata,
    global_start_time: OtioRationalTime,
    tracks: OtioStack,
}

#[derive(Serialize)]
struct OtioStack {
    #[serde(rename = "OTIO_SCHEMA")]
    schema: &'static str,
    name: &'static str,
    children: Vec<OtioTrack>,
    markers: Vec<OtioMarker>,
}

#[derive(Serialize)]
struct OtioTrack {
    #[serde(rename = "OTIO_SCHEMA")]
    schema: &'static str,
    name: String,
    kind: &'static str,
    children: Vec<Value>,
}

fn ticks_to_rational(t: Ticks, fps: FrameRate, audio_sr: Option<u32>) -> OtioRationalTime {
    if let Some(sr) = audio_sr {
        if let Some(step) = Ticks::exact_step_for_sample_rate(sr) {
            if step > 0 && t.0 % step == 0 {
                return OtioRationalTime {
                    schema: "RationalTime.1",
                    value: (t.0 / step) as f64,
                    rate: sr as f64,
                };
            }
        }
    }
    let num = fps.num.max(1) as i128;
    let den = fps.den.max(1) as i128;
    let den_ticks = TICKS_PER_SEC as i128 * den;
    let scaled = t.0 as i128 * num;
    let value = if den_ticks > 0 && scaled % den_ticks == 0 {
        (scaled / den_ticks) as f64
    } else {
        (t.0 as f64 * fps.as_f64()) / (TICKS_PER_SEC as f64)
    };
    OtioRationalTime {
        schema: "RationalTime.1",
        value,
        rate: fps.as_f64(),
    }
}

fn ticks_range(start: Ticks, dur: Ticks, fps: FrameRate, sr: Option<u32>) -> OtioTimeRange {
    OtioTimeRange {
        schema: "TimeRange.1",
        start_time: ticks_to_rational(start, fps, sr),
        duration: ticks_to_rational(dur, fps, sr),
    }
}

fn export_transition_type(k: TransitionKind) -> &'static str {
    match k {
        TransitionKind::Dissolve => "SMPTE_Dissolve",
        _ => "Custom_Transition",
    }
}

fn export_media_ref(item: Option<&MediaItem>, fps: FrameRate, sr: Option<u32>) -> Value {
    let Some(m) = item else {
        return serde_json::json!({
            "OTIO_SCHEMA": "MissingReference.1",
            "name": ""
        });
    };
    let avail = m
        .duration
        .filter(|d| *d > Ticks::ZERO)
        .map(|d| ticks_range(Ticks::ZERO, d, fps, sr));

    match &m.source {
        MediaSource::File { path } | MediaSource::NestedProject { path } => serde_json::json!({
            "OTIO_SCHEMA": "ExternalReference.1",
            "name": m.name,
            "target_url": encode_target_url(path),
            "available_range": avail,
        }),
        MediaSource::ImageSequence { paths, .. } => {
            let first = paths.first().map(String::as_str).unwrap_or("");
            serde_json::json!({
                "OTIO_SCHEMA": "ExternalReference.1",
                "name": m.name,
                "target_url": encode_target_url(first),
                "available_range": avail,
            })
        }
        MediaSource::Generated(spec) => {
            let (gen_kind, params) = match spec {
                GenSpec::ColorBars => ("SMPTEBars", serde_json::json!({})),
                GenSpec::SolidColor { color } => {
                    ("SolidColor", serde_json::json!({ "color": color }))
                }
                GenSpec::Checkerboard { .. } => ("Checkerboard", serde_json::json!({})),
                GenSpec::Text(t) => ("Text", serde_json::json!({ "text": t.text })),
                GenSpec::Gradient { .. } => ("Gradient", serde_json::json!({})),
                GenSpec::Noise { .. } => ("Noise", serde_json::json!({})),
                GenSpec::TestPattern { pattern } => {
                    ("TestPattern", serde_json::json!({ "pattern": pattern }))
                }
                GenSpec::NoiseTexture { .. } => ("NoiseTexture", serde_json::json!({})),
                GenSpec::Ofx(r) => (
                    "Ofx",
                    serde_json::json!({ "plugin_id": r.plugin_id, "label": r.label }),
                ),
            };
            serde_json::json!({
                "OTIO_SCHEMA": "GeneratorReference.1",
                "name": m.name,
                "generator_kind": gen_kind,
                "parameters": params,
            })
        }
        MediaSource::Sequence { id } => serde_json::json!({
            "OTIO_SCHEMA": "ExternalReference.1",
            "name": m.name,
            "target_url": format!("sequence:{id}"),
            "available_range": avail,
        }),
    }
}

fn export_event_effects(ev: &Event) -> Vec<Value> {
    let mut effects = Vec::new();
    if ev.rate == Rate::FROZEN {
        effects.push(serde_json::json!({
            "OTIO_SCHEMA": "FreezeFrame.1",
            "name": "Congelar quadro",
            "effect_name": "FreezeFrame"
        }));
    } else if !ev.rate.is_normal() || ev.reverse {
        let mut scalar = ev.rate.as_f64();
        if ev.reverse {
            scalar = -scalar;
        }
        effects.push(serde_json::json!({
            "OTIO_SCHEMA": "LinearTimeWarp.1",
            "name": "Velocidade",
            "effect_name": "LinearTimeWarp",
            "time_scalar": scalar
        }));
    }
    effects
}

fn export_track_children(
    track: &Track,
    project: &Project,
    fps: FrameRate,
    sr: Option<u32>,
) -> Vec<Value> {
    let mut events = track.events.clone();
    events.sort_by_key(|e| (e.start, e.id));

    let n = events.len();
    let mut trim_in = vec![Ticks::ZERO; n];
    let mut trim_out = vec![Ticks::ZERO; n];
    let mut cuts: Vec<Option<(TransitionKind, Ticks, Ticks)>> = vec![None; n];

    for i in 0..n.saturating_sub(1) {
        let a = &events[i];
        let b = &events[i + 1];
        if b.start < a.end() {
            let overlap = (a.end() - b.start)
                .min(a.length.div_i64(2))
                .min(b.length.div_i64(2));
            if overlap > Ticks::ZERO {
                let in_off = overlap.div_i64(2);
                let out_off = overlap - in_off;
                trim_out[i] = out_off;
                trim_in[i + 1] = in_off;
                let kind = b
                    .transition
                    .as_ref()
                    .map(|t| t.kind)
                    .unwrap_or(TransitionKind::Dissolve);
                cuts[i] = Some((kind, in_off, out_off));
            }
        }
    }

    let mut children = Vec::new();
    let mut cursor = Ticks::ZERO;
    for (i, ev) in events.iter().enumerate() {
        let start = ev.start + trim_in[i];
        let end = (ev.end() - trim_out[i]).max(start);
        if start > cursor {
            let gap = start - cursor;
            children.push(serde_json::json!({
                "OTIO_SCHEMA": "Gap.1",
                "name": "",
                "source_range": ticks_range(Ticks::ZERO, gap, fps, sr),
            }));
        }
        let tl_dur = (end - start).max(Ticks::from_millis_i64(1));
        let src_start = ev.offset + ev.rate.scale_ticks(trim_in[i]);
        let src_dur = tl_dur;

        let media_item = project.media.iter().find(|m| m.id == ev.media_id);
        let mref = export_media_ref(media_item, fps, sr);
        let mut refs = BTreeMap::new();
        refs.insert("DEFAULT_MEDIA", mref);

        let effects = export_event_effects(ev);
        children.push(serde_json::json!({
            "OTIO_SCHEMA": "Clip.2",
            "name": ev.name,
            "source_range": ticks_range(src_start, src_dur, fps, sr),
            "media_references": refs,
            "active_media_reference_key": "DEFAULT_MEDIA",
            "effects": effects,
            "markers": [],
        }));

        cursor = start + tl_dur;

        if let Some((kind, in_off, out_off)) = cuts[i] {
            children.push(serde_json::json!({
                "OTIO_SCHEMA": "Transition.1",
                "name": kind.name(),
                "transition_type": export_transition_type(kind),
                "in_offset": ticks_to_rational(in_off, fps, sr),
                "out_offset": ticks_to_rational(out_off, fps, sr),
                "metadata": { "neoveg_transition": format!("{kind:?}") },
            }));
        }
    }
    children
}

/// Exporta o projeto Neoveg para JSON no formato OpenTimelineIO 0.17 (`Timeline.1`).
pub fn export_otio_string(project: &Project, name: Option<&str>) -> Result<String, NeovegError> {
    let fps = project.settings.fps;
    let sr_val = project.settings.sample_rate;
    let mut tracks = Vec::with_capacity(project.tracks.len());

    for t in &project.tracks {
        let sr_opt = if t.kind == TrackKind::Audio {
            Some(sr_val)
        } else {
            None
        };
        let kind_str = match t.kind {
            TrackKind::Video => "Video",
            TrackKind::Audio => "Audio",
        };
        tracks.push(OtioTrack {
            schema: "Track.1",
            name: t.name.clone(),
            kind: kind_str,
            children: export_track_children(t, project, fps, sr_opt),
        });
    }

    let markers = project
        .markers
        .iter()
        .map(|m| OtioMarker {
            schema: "Marker.2",
            name: m.label.clone(),
            color: "GREEN",
            marked_range: ticks_range(m.t, Ticks::ZERO, fps, None),
        })
        .collect();

    let timeline = OtioTimeline {
        schema: "Timeline.1",
        name: clean_name(name, "Neoveg Timeline"),
        metadata: OtioRootMetadata {
            neoveg: OtioNeovegMeta {
                width: project.settings.width,
                height: project.settings.height,
                sample_rate: project.settings.sample_rate,
                fps_num: fps.num,
                fps_den: fps.den,
                drop_frame: project.settings.drop_frame,
            },
        },
        global_start_time: ticks_to_rational(Ticks::ZERO, fps, None),
        tracks: OtioStack {
            schema: "Stack.1",
            name: "tracks",
            children: tracks,
            markers,
        },
    };

    serde_json::to_string_pretty(&timeline)
        .map_err(|e| NeovegError::Json(format!("erro ao serializar OTIO: {e}")))
}

/// Salva o projeto como arquivo `.otio` em disco.
pub fn export_otio_file(project: &Project, path: &Path) -> Result<(), NeovegError> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty());
    let json = export_otio_string(project, stem)?;
    std::fs::write(path, json)
        .map_err(|e| NeovegError::Io(format!("erro ao gravar OTIO '{}': {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn otio_roundtrip_preserva_trilhas_gaps_transicoes_velocidade_e_marcadores() {
        let mut p = Project::new_default();
        p.settings.fps = FrameRate::FPS_24;
        p.tracks.clear();

        let m_vid = MediaItem {
            id: crate::model::new_id(),
            name: "cena 01.avi".into(),
            source: MediaSource::File {
                path: "C:/midia/cena 01.avi".into(),
            },
            duration: Some(Ticks::from_secs_i64(20)),
            has_video: true,
            has_audio: false,
            width: 1920,
            height: 1080,
            fps: Some(FrameRate::FPS_24),
            info: String::new(),
            proxy_path: None,
        };
        let m_gen = MediaItem {
            id: crate::model::new_id(),
            name: "Barras".into(),
            source: MediaSource::Generated(GenSpec::ColorBars),
            duration: None,
            has_video: true,
            has_audio: false,
            width: 0,
            height: 0,
            fps: None,
            info: String::new(),
            proxy_path: None,
        };
        let m_aud = MediaItem {
            id: crate::model::new_id(),
            name: "trilha.wav".into(),
            source: MediaSource::File {
                path: "C:/midia/trilha.wav".into(),
            },
            duration: Some(Ticks::from_secs_i64(10)),
            has_video: false,
            has_audio: true,
            width: 0,
            height: 0,
            fps: None,
            info: String::new(),
            proxy_path: None,
        };
        p.media = vec![m_vid.clone(), m_gen.clone(), m_aud.clone()];

        let mut v_track = Track::new(TrackKind::Video, "V1");
        let e1 = Event::new(&m_vid, Ticks::from_secs_i64(2), Ticks::from_secs_i64(6));
        let mut e2 = Event::new(&m_gen, Ticks::from_secs_i64(6), Ticks::from_secs_i64(4));
        e2.transition = Some(TransitionSpec {
            kind: TransitionKind::WipeLeft,
            softness: 0.0,
        });
        e2.rate = Rate::from_ratio(2, 1).unwrap();
        v_track.events = vec![e1, e2];

        let mut a_track = Track::new(TrackKind::Audio, "A1");
        let e3 = Event::new(&m_aud, Ticks::ZERO, Ticks::from_secs_i64(5));
        a_track.events = vec![e3];
        p.tracks = vec![v_track, a_track];

        p.markers.push(Marker {
            id: crate::model::new_id(),
            t: Ticks::from_secs_i64(3),
            label: "Corte principal".into(),
            ..Default::default()
        });

        let otio_json = export_otio_string(&p, Some("Teste")).unwrap();
        assert!(otio_json.contains("\"OTIO_SCHEMA\": \"Timeline.1\""));
        assert!(otio_json.contains("\"OTIO_SCHEMA\": \"Transition.1\""));
        assert!(otio_json.contains("\"OTIO_SCHEMA\": \"LinearTimeWarp.1\""));

        let (re, fixes) = import_otio_str(&otio_json).unwrap();
        assert!(fixes.is_empty(), "correções inesperadas: {fixes:?}");
        assert_eq!(re.tracks.len(), 2);
        assert_eq!(re.tracks[0].events.len(), 2);
        // Gap inicial de 2s + sobreposição [6s..8s] preservada no round-trip
        assert_eq!(re.tracks[0].events[0].start, Ticks::from_secs_i64(2));
        assert_eq!(re.tracks[0].events[0].length, Ticks::from_secs_i64(6));
        assert_eq!(re.tracks[0].events[1].start, Ticks::from_secs_i64(6));
        assert_eq!(re.tracks[0].events[1].length, Ticks::from_secs_i64(4));
        assert_eq!(
            re.tracks[0].events[1].transition.as_ref().map(|t| t.kind),
            Some(TransitionKind::WipeLeft)
        );
        assert!((re.tracks[0].events[1].rate.as_f64() - 2.0).abs() < 1e-4);
        assert_eq!(re.markers.len(), 1);
        assert_eq!(re.markers[0].t, Ticks::from_secs_i64(3));
        assert_eq!(re.markers[0].label, "Corte principal");
    }

    #[test]
    fn otio_externo_especificacao_017_com_clip1_e_missing_ref() {
        let sample = r#"{
            "OTIO_SCHEMA": "Timeline.1",
            "name": "Spec 0.17",
            "tracks": {
                "OTIO_SCHEMA": "Stack.1",
                "children": [
                    {
                        "OTIO_SCHEMA": "Track.1",
                        "name": "Video 1",
                        "kind": "Video",
                        "children": [
                            {
                                "OTIO_SCHEMA": "Clip.1",
                                "name": "Intro",
                                "media_reference": {
                                    "OTIO_SCHEMA": "ExternalReference.1",
                                    "target_url": "file:///C:/videos/meu%20clipe.mp4",
                                    "available_range": {
                                        "OTIO_SCHEMA": "TimeRange.1",
                                        "start_time": {"OTIO_SCHEMA": "RationalTime.1", "value": 0, "rate": 24},
                                        "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 240, "rate": 24}
                                    }
                                },
                                "source_range": {
                                    "OTIO_SCHEMA": "TimeRange.1",
                                    "start_time": {"OTIO_SCHEMA": "RationalTime.1", "value": 24, "rate": 24},
                                    "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 48, "rate": 24}
                                }
                            },
                            {
                                "OTIO_SCHEMA": "Transition.1",
                                "transition_type": "SMPTE_Dissolve",
                                "in_offset": {"OTIO_SCHEMA": "RationalTime.1", "value": 12, "rate": 24},
                                "out_offset": {"OTIO_SCHEMA": "RationalTime.1", "value": 12, "rate": 24}
                            },
                            {
                                "OTIO_SCHEMA": "Clip.1",
                                "name": "Outro",
                                "media_reference": { "OTIO_SCHEMA": "MissingReference.1", "name": "offline.mov" },
                                "source_range": {
                                    "OTIO_SCHEMA": "TimeRange.1",
                                    "start_time": {"OTIO_SCHEMA": "RationalTime.1", "value": 24, "rate": 24},
                                    "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 72, "rate": 24}
                                }
                            }
                        ]
                    }
                ]
            }
        }"#;
        let (p, warnings) = import_otio_str(sample).unwrap();
        assert!(!warnings.is_empty());
        assert_eq!(p.settings.fps, FrameRate::FPS_24);
        assert_eq!(p.tracks[0].events.len(), 2);
        // Primeiro clipe começa em 0s com 2s (+ 0.5s out_offset)
        assert_eq!(p.tracks[0].events[0].start, Ticks::ZERO);
        assert_eq!(p.tracks[0].events[0].offset, Ticks::from_secs_i64(1));
        assert_eq!(p.tracks[0].events[0].length, Ticks::from_millis_i64(2500));
        // Segundo clipe começa em 2s - 0.5s = 1.5s com transição Dissolve
        assert_eq!(p.tracks[0].events[1].start, Ticks::from_millis_i64(1500));
        assert_eq!(
            p.tracks[0].events[1].transition.as_ref().map(|t| t.kind),
            Some(TransitionKind::Dissolve)
        );
        assert!(matches!(
            &p.media[0].source,
            MediaSource::File { path } if path == "C:/videos/meu clipe.mp4"
        ));
    }

    #[test]
    fn otio_hostil_nao_entra_em_panico() {
        for bad in [
            "",
            "{}",
            "[]",
            r#"{"OTIO_SCHEMA":"Timeline.1","tracks":{"OTIO_SCHEMA":"Stack.1","children":[{"OTIO_SCHEMA":"Track.1","children":[{"OTIO_SCHEMA":"Clip.2","source_range":{"start_time":{"value":NaN,"rate":0},"duration":{"value":1e300,"rate":-5}}}]}]}}"#,
            r#"{"OTIO_SCHEMA":"Desconhecido.1"}"#,
        ] {
            let _ = import_otio_str(bad);
        }
    }
}
