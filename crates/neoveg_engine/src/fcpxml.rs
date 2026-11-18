//! Importação e exportação em sala limpa (*Clean Room*, `AGENTS.md` §2.3)
//! de projetos **Final Cut Pro XML (`.fcpxml`)** a partir da especificação
//! pública FCPXML 1.8–1.11 (DTD e referência de formato).
//!
//! Usa um analisador XML limitado próprio sem `unsafe` e com aritmética racional
//! exata `i128` para converter durações fracionárias (`"1001/30000s"`) em `Ticks`.

use crate::model::{
    new_id, Event, GenSpec, Id, Marker, MediaItem, MediaSource, Project, TextAlign, TextSpec,
    Track, TrackKind, TransitionKind, TransitionSpec, MAX_DIM, MAX_TICKS,
};
use crate::otio::{
    clamp_i128_ticks, decode_target_url, div_round_i128, encode_target_url, read_bounded_utf8_file,
};
use neoveg_common::error::NeovegError;
use neoveg_common::time::{FrameRate, Rate, Ticks, TICKS_PER_SEC};
use std::collections::HashMap;
use std::path::Path;

/// Limite máximo de tamanho de arquivo FCPXML lido (`16 MiB`, `AGENTS.md` §0.4).
pub const MAX_FCPXML_BYTES: usize = 16 * 1024 * 1024;
const MAX_XML_DEPTH: usize = 48;
const MAX_XML_NODES: usize = 100_000;
const MAX_XML_ATTRS: usize = 64;
const MAX_FCPXML_CLIPS: usize = 20_000;
const MAX_FCPXML_LANES: usize = 32;
const DEFAULT_FCPXML_MS: i64 = 5_000;

// ---------------------------------------------------------------------------
// Parser XML estrito, iterativo/limitado e livre de pânicos (§0.4–§0.5)
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub(crate) struct XmlNode {
    pub(crate) tag: String,
    pub(crate) attrs: Vec<(String, String)>,
    pub(crate) text: String,
    pub(crate) children: Vec<XmlNode>,
}

impl XmlNode {
    pub(crate) fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub(crate) fn child(&self, tag: &str) -> Option<&XmlNode> {
        self.children.iter().find(|c| c.tag == tag)
    }

    pub(crate) fn find_descendant(&self, tag: &str) -> Option<&XmlNode> {
        if self.tag == tag {
            return Some(self);
        }
        for c in &self.children {
            if let Some(found) = c.find_descendant(tag) {
                return Some(found);
            }
        }
        None
    }

    fn collect_text(&self, out: &mut String) {
        if !self.text.is_empty() {
            out.push_str(&self.text);
        }
        for c in &self.children {
            c.collect_text(out);
        }
    }
}

fn decode_xml_entities(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after_amp = &rest[amp + 1..];
        if let Some(semi) = after_amp.find(';').filter(|&i| i <= 16) {
            let ent = &after_amp[..semi];
            let replaced = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                    u32::from_str_radix(&ent[2..], 16)
                        .ok()
                        .and_then(char::from_u32)
                        .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
                }
                _ if ent.starts_with('#') => ent[1..]
                    .parse::<u32>()
                    .ok()
                    .and_then(char::from_u32)
                    .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t')),
                _ => None,
            };
            if let Some(ch) = replaced {
                out.push(ch);
                rest = &after_amp[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = after_amp;
    }
    out.push_str(rest);
    out
}

pub(crate) fn escape_xml(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() && !matches!(c, '\n' | '\t') => {}
            c => out.push(c),
        }
    }
    out
}

fn skip_ws(b: &[u8], pos: &mut usize) {
    while *pos < b.len() && b[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
}

fn skip_decl_or_pi(b: &[u8], pos: &mut usize) -> Result<(), NeovegError> {
    if b.get(*pos..*pos + 4) == Some(b"<!--") {
        *pos += 4;
        while *pos + 2 < b.len() {
            if &b[*pos..*pos + 3] == b"-->" {
                *pos += 3;
                return Ok(());
            }
            *pos += 1;
        }
        return Err(NeovegError::InvalidInput(
            "comentário XML não fechado".into(),
        ));
    }
    if b.get(*pos..*pos + 2) == Some(b"<?") {
        *pos += 2;
        while *pos + 1 < b.len() {
            if &b[*pos..*pos + 2] == b"?>" {
                *pos += 2;
                return Ok(());
            }
            *pos += 1;
        }
        return Err(NeovegError::InvalidInput(
            "instrução XML <? não fechada".into(),
        ));
    }
    // <!DOCTYPE ...> ou similar com possível subconjunto [...]
    *pos += 2;
    let mut quote: Option<u8> = None;
    let mut bracket_depth: usize = 0;
    while *pos < b.len() {
        let ch = b[*pos];
        *pos += 1;
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            b'"' | b'\'' => quote = Some(ch),
            b'[' => bracket_depth = bracket_depth.saturating_add(1),
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'>' if bracket_depth == 0 => return Ok(()),
            _ => {}
        }
    }
    Err(NeovegError::InvalidInput(
        "declaração XML <! não fechada".into(),
    ))
}

fn read_name<'a>(s: &'a str, b: &[u8], pos: &mut usize) -> Result<&'a str, NeovegError> {
    let start = *pos;
    while *pos < b.len() {
        let c = b[*pos];
        if c.is_ascii_whitespace() || matches!(c, b'>' | b'/' | b'=' | b'<' | b'"' | b'\'') {
            break;
        }
        *pos += 1;
    }
    if *pos == start {
        return Err(NeovegError::InvalidInput("nome XML esperado".into()));
    }
    s.get(start..*pos)
        .ok_or_else(|| NeovegError::InvalidInput("fronteira UTF-8 inválida em tag XML".into()))
}

pub(crate) fn parse_xml_document(raw: &str) -> Result<XmlNode, NeovegError> {
    let text = raw.strip_prefix('\u{FEFF}').unwrap_or(raw);
    let b = text.as_bytes();
    let mut pos = 0usize;
    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root: Option<XmlNode> = None;
    let mut node_count = 0usize;

    while pos < b.len() {
        if b[pos] != b'<' {
            let start = pos;
            while pos < b.len() && b[pos] != b'<' {
                pos += 1;
            }
            if let Some(top) = stack.last_mut() {
                if let Some(slice) = text.get(start..pos) {
                    if top.text.len() + slice.len() <= 64 * 1024 {
                        top.text.push_str(&decode_xml_entities(slice));
                    }
                }
            }
            continue;
        }

        if b.get(pos..pos + 9) == Some(b"<![CDATA[") {
            pos += 9;
            let start = pos;
            let mut end = None;
            while pos + 2 < b.len() {
                if &b[pos..pos + 3] == b"]]>" {
                    end = Some(pos);
                    pos += 3;
                    break;
                }
                pos += 1;
            }
            let Some(e) = end else {
                return Err(NeovegError::InvalidInput("seção CDATA não fechada".into()));
            };
            if let Some(top) = stack.last_mut() {
                if let Some(slice) = text.get(start..e) {
                    if top.text.len() + slice.len() <= 64 * 1024 {
                        top.text.push_str(slice);
                    }
                }
            }
            continue;
        }

        if b.get(pos..pos + 2) == Some(b"<?")
            || b.get(pos..pos + 4) == Some(b"<!--")
            || b.get(pos..pos + 2) == Some(b"<!")
        {
            skip_decl_or_pi(b, &mut pos)?;
            continue;
        }

        if b.get(pos..pos + 2) == Some(b"</") {
            pos += 2;
            skip_ws(b, &mut pos);
            let close_name = read_name(text, b, &mut pos)?;
            skip_ws(b, &mut pos);
            if b.get(pos) != Some(&b'>') {
                return Err(NeovegError::InvalidInput(format!(
                    "fechamento '</{close_name}>' malformado"
                )));
            }
            pos += 1;
            let Some(finished) = stack.pop() else {
                return Err(NeovegError::InvalidInput(format!(
                    "tag de fechamento inesperada '</{close_name}>'"
                )));
            };
            if finished.tag != close_name {
                return Err(NeovegError::InvalidInput(format!(
                    "tag de fechamento '</{close_name}>' não corresponde a '<{}>'",
                    finished.tag
                )));
            }
            if let Some(parent) = stack.last_mut() {
                parent.children.push(finished);
            } else {
                root = Some(finished);
                break;
            }
            continue;
        }

        // Tag de abertura <tag ...> ou <tag .../>
        pos += 1;
        skip_ws(b, &mut pos);
        let tag_name = read_name(text, b, &mut pos)?.to_string();
        let mut attrs = Vec::new();
        let mut self_closing = false;

        loop {
            skip_ws(b, &mut pos);
            if pos >= b.len() {
                return Err(NeovegError::InvalidInput(format!(
                    "tag '<{tag_name}' sem fechamento '>'"
                )));
            }
            if b.get(pos..pos + 2) == Some(b"/>") {
                pos += 2;
                self_closing = true;
                break;
            }
            if b[pos] == b'>' {
                pos += 1;
                break;
            }
            let attr_name = read_name(text, b, &mut pos)?.to_string();
            skip_ws(b, &mut pos);
            let attr_val = if b.get(pos) == Some(&b'=') {
                pos += 1;
                skip_ws(b, &mut pos);
                let Some(&q) = b.get(pos).filter(|&&c| c == b'"' || c == b'\'') else {
                    return Err(NeovegError::InvalidInput(format!(
                        "aspas esperadas no atributo '{attr_name}'"
                    )));
                };
                pos += 1;
                let v_start = pos;
                while pos < b.len() && b[pos] != q {
                    pos += 1;
                }
                if pos >= b.len() {
                    return Err(NeovegError::InvalidInput(format!(
                        "valor do atributo '{attr_name}' não fechado"
                    )));
                }
                let raw_val = text.get(v_start..pos).ok_or_else(|| {
                    NeovegError::InvalidInput("fronteira UTF-8 inválida em atributo XML".into())
                })?;
                pos += 1;
                decode_xml_entities(raw_val)
            } else {
                String::new()
            };
            if attrs.len() < MAX_XML_ATTRS {
                attrs.push((attr_name, attr_val));
            }
        }

        node_count += 1;
        if node_count > MAX_XML_NODES {
            return Err(NeovegError::InvalidInput(format!(
                "documento XML excede o limite de {MAX_XML_NODES} nós"
            )));
        }

        let node = XmlNode {
            tag: tag_name,
            attrs,
            text: String::new(),
            children: Vec::new(),
        };
        if self_closing {
            if let Some(parent) = stack.last_mut() {
                parent.children.push(node);
            } else {
                root = Some(node);
                break;
            }
        } else {
            if stack.len() >= MAX_XML_DEPTH {
                return Err(NeovegError::InvalidInput(format!(
                    "profundidade XML excede o limite de {MAX_XML_DEPTH} níveis"
                )));
            }
            stack.push(node);
        }
    }

    if !stack.is_empty() {
        return Err(NeovegError::InvalidInput(
            "documento FCPXML possui tags abertas sem fechamento".into(),
        ));
    }
    root.ok_or_else(|| NeovegError::InvalidInput("documento FCPXML sem elemento raiz".into()))
}

// ---------------------------------------------------------------------------
// Aritmética racional exata de tempo FCPXML (`"N/Ds"` / `"Ns"`)
// ---------------------------------------------------------------------------

fn gcd_u128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a.max(1)
}

/// Converte um valor temporal FCPXML (`"1001/30000s"`, `"240/24s"`, `"5s"`, `"2.5s"`) em `Ticks` exatos.
pub fn parse_fcpxml_time(raw: &str) -> Option<Ticks> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    let body = s
        .strip_suffix('s')
        .or_else(|| s.strip_suffix('S'))
        .unwrap_or(s)
        .trim();
    if body.is_empty() {
        return None;
    }
    if let Some((num_s, den_s)) = body.split_once('/') {
        let num: i128 = num_s.trim().parse().ok()?;
        let den: i128 = den_s.trim().parse().ok()?;
        if den <= 0 {
            return None;
        }
        let g = gcd_u128(num.unsigned_abs(), den.unsigned_abs()) as i128;
        let rn = num / g;
        let rd = den / g;
        let scaled = rn.checked_mul(TICKS_PER_SEC as i128)?;
        return Some(clamp_i128_ticks(div_round_i128(scaled, rd)));
    }
    if let Ok(int_secs) = body.parse::<i128>() {
        let max_s = (MAX_TICKS / TICKS_PER_SEC) as i128;
        let clamped_s = int_secs.clamp(-max_s, max_s);
        return Some(clamp_i128_ticks(clamped_s * TICKS_PER_SEC as i128));
    }
    let secs: f64 = body.parse().ok()?;
    Ticks::from_secs_f64(secs)
}

fn format_fcpxml_time(t: Ticks, fps: FrameRate) -> String {
    if t == Ticks::ZERO {
        return "0s".into();
    }
    let frames = fps.ticks_to_frame_round(t);
    if fps.frame_to_ticks(frames) == t && fps.num > 0 {
        let num = (frames as i128) * (fps.den as i128);
        let den = fps.num as i128;
        let g = gcd_u128(num.unsigned_abs(), den.unsigned_abs()) as i128;
        let rn = num / g;
        let rd = den / g;
        if rd == 1 {
            return format!("{rn}s");
        }
        return format!("{rn}/{rd}s");
    }
    let g = gcd_u128(t.0.unsigned_abs() as u128, TICKS_PER_SEC as u128) as i128;
    let rn = (t.0 as i128) / g;
    let rd = (TICKS_PER_SEC as i128) / g;
    if rd == 1 {
        format!("{rn}s")
    } else {
        format!("{rn}/{rd}s")
    }
}

fn parse_frame_duration(raw: &str) -> Option<FrameRate> {
    let body = raw
        .trim()
        .strip_suffix('s')
        .or_else(|| raw.trim().strip_suffix('S'))
        .unwrap_or(raw.trim());
    if let Some((num_s, den_s)) = body.split_once('/') {
        let num: u64 = num_s.trim().parse().ok()?;
        let den: u64 = den_s.trim().parse().ok()?;
        if num == 0 || den == 0 {
            return None;
        }
        let f_num = u32::try_from(den).ok();
        let f_den = u32::try_from(num).ok();
        if let (Some(fn_), Some(fd_)) = (f_num, f_den) {
            if let Some(fr) = FrameRate::new(fn_, fd_) {
                return Some(fr);
            }
        }
        return FrameRate::from_f64(den as f64 / num as f64);
    }
    let secs: f64 = body.parse().ok()?;
    if secs > 0.0 && secs.is_finite() {
        FrameRate::from_f64(1.0 / secs)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Contexto de importação FCPXML
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct FcpxmlAsset {
    name: String,
    src: String,
    duration: Option<Ticks>,
    has_video: bool,
    has_audio: bool,
    width: u32,
    height: u32,
    fps: Option<FrameRate>,
}

#[derive(Clone, Debug)]
struct FcpxmlFormat {
    fps: Option<FrameRate>,
    width: u32,
    height: u32,
}

struct FcpxmlCtx {
    formats: HashMap<String, FcpxmlFormat>,
    assets: HashMap<String, FcpxmlAsset>,
    effects: HashMap<String, String>,
    media: Vec<MediaItem>,
    media_idx: HashMap<String, usize>,
    video_lanes: HashMap<i32, Track>,
    audio_lanes: HashMap<i32, Track>,
    markers: Vec<Marker>,
    warnings: Vec<String>,
    clip_count: usize,
    fps: FrameRate,
    width: u32,
    height: u32,
}

impl FcpxmlCtx {
    fn ensure_media_file(&mut self, asset_id: &str, clip_name: &str) -> MediaItem {
        let asset = self.assets.get(asset_id).cloned();
        let (path_or_name, display_name, dur, has_v, has_a, w, h, fps) = match asset {
            Some(a) => {
                let decoded = if a.src.is_empty() {
                    a.name.clone()
                } else {
                    decode_target_url(&a.src)
                };
                let name = if !a.name.trim().is_empty() {
                    a.name
                } else if !clip_name.trim().is_empty() {
                    clip_name.to_string()
                } else {
                    decoded.clone()
                };
                (
                    decoded,
                    name,
                    a.duration,
                    a.has_video || !a.has_audio,
                    a.has_audio,
                    if a.width > 0 { a.width } else { self.width },
                    if a.height > 0 { a.height } else { self.height },
                    a.fps.or(Some(self.fps)),
                )
            }
            None => {
                let fallback = if clip_name.trim().is_empty() {
                    format!("Asset FCPXML {asset_id}")
                } else {
                    clip_name.to_string()
                };
                (
                    fallback.clone(),
                    fallback,
                    Some(Ticks::from_millis_i64(DEFAULT_FCPXML_MS)),
                    true,
                    true,
                    self.width,
                    self.height,
                    Some(self.fps),
                )
            }
        };

        let key = format!("F:{asset_id}:{path_or_name}");
        if let Some(&idx) = self.media_idx.get(&key) {
            return self.media[idx].clone();
        }
        let item = MediaItem {
            id: new_id(),
            name: display_name,
            source: MediaSource::File { path: path_or_name },
            duration: dur,
            has_video: has_v,
            has_audio: has_a,
            width: w,
            height: h,
            fps,
            info: "Importado via FCPXML".into(),
            proxy_path: None,
        };
        let idx = self.media.len();
        self.media.push(item.clone());
        self.media_idx.insert(key, idx);
        item
    }

    fn ensure_text_media(&mut self, text: &str, name: &str, dur: Ticks) -> MediaItem {
        let clean = if text.trim().is_empty() {
            name.to_string()
        } else {
            text.trim().to_string()
        };
        let key = format!("T:{name}:{clean}");
        if let Some(&idx) = self.media_idx.get(&key) {
            return self.media[idx].clone();
        }
        let spec = TextSpec {
            text: clean,
            align: TextAlign::Center,
            ..TextSpec::default()
        };
        let item = MediaItem {
            id: new_id(),
            name: if name.trim().is_empty() {
                "Título FCPXML".into()
            } else {
                name.to_string()
            },
            source: MediaSource::Generated(GenSpec::Text(spec)),
            duration: Some(dur.max(Ticks::from_millis_i64(1))),
            has_video: true,
            has_audio: false,
            width: self.width,
            height: self.height,
            fps: Some(self.fps),
            info: "Título FCPXML".into(),
            proxy_path: None,
        };
        let idx = self.media.len();
        self.media.push(item.clone());
        self.media_idx.insert(key, idx);
        item
    }

    fn video_track(&mut self, lane: i32) -> &mut Track {
        let norm_lane = lane.clamp(0, MAX_FCPXML_LANES as i32);
        self.video_lanes.entry(norm_lane).or_insert_with(|| {
            let name = if norm_lane == 0 {
                "Vídeo 1".to_string()
            } else {
                format!("Vídeo {}", norm_lane + 1)
            };
            Track::new(TrackKind::Video, &name)
        })
    }

    fn audio_track(&mut self, lane: i32) -> &mut Track {
        let norm_lane = lane.clamp(-(MAX_FCPXML_LANES as i32), 0);
        self.audio_lanes.entry(norm_lane).or_insert_with(|| {
            let num = if norm_lane < 0 {
                norm_lane.abs() + 1
            } else {
                1
            };
            Track::new(TrackKind::Audio, &format!("Áudio {num}"))
        })
    }
}

fn parse_resources(root: &XmlNode, ctx: &mut FcpxmlCtx) {
    let Some(res) = root.find_descendant("resources") else {
        return;
    };
    for node in &res.children {
        match node.tag.as_str() {
            "format" => {
                if let Some(id) = node.attr("id") {
                    let fps = node.attr("frameDuration").and_then(parse_frame_duration);
                    let w = node
                        .attr("width")
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(1920)
                        .clamp(16, MAX_DIM);
                    let h = node
                        .attr("height")
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(1080)
                        .clamp(16, MAX_DIM);
                    ctx.formats.insert(
                        id.to_string(),
                        FcpxmlFormat {
                            fps,
                            width: w,
                            height: h,
                        },
                    );
                }
            }
            "asset" => {
                let Some(id) = node.attr("id") else {
                    continue;
                };
                let src_attr = node.attr("src").map(str::to_string).or_else(|| {
                    node.children
                        .iter()
                        .find(|c| c.tag == "media-rep")
                        .and_then(|m| m.attr("src").map(str::to_string))
                });
                let fmt = node.attr("format").and_then(|fid| ctx.formats.get(fid));
                let has_video = node.attr("hasVideo").is_none_or(|v| v != "0");
                let has_audio = node.attr("hasAudio").is_none_or(|v| v != "0");
                let dur = node
                    .attr("duration")
                    .and_then(parse_fcpxml_time)
                    .filter(|d| *d > Ticks::ZERO);
                ctx.assets.insert(
                    id.to_string(),
                    FcpxmlAsset {
                        name: node.attr("name").unwrap_or("").to_string(),
                        src: src_attr.unwrap_or_default(),
                        duration: dur,
                        has_video,
                        has_audio,
                        width: fmt.map_or(ctx.width, |f| f.width),
                        height: fmt.map_or(ctx.height, |f| f.height),
                        fps: fmt.and_then(|f| f.fps),
                    },
                );
            }
            "effect" => {
                if let Some(id) = node.attr("id") {
                    let name = node.attr("name").unwrap_or("Efeito").to_string();
                    ctx.effects.insert(id.to_string(), name);
                }
            }
            _ => {}
        }
    }
}

fn parse_timemap_rate(node: &XmlNode) -> Option<Rate> {
    let tm = node.child("timeMap")?;
    let pts: Vec<(Ticks, Ticks)> = tm
        .children
        .iter()
        .filter(|c| c.tag == "timept")
        .filter_map(|p| {
            let t = parse_fcpxml_time(p.attr("time")?)?;
            let v = parse_fcpxml_time(p.attr("value")?)?;
            Some((t, v))
        })
        .collect();
    if pts.len() < 2 {
        return None;
    }
    let dt = (pts[pts.len() - 1].0 - pts[0].0).0;
    let dv = (pts[pts.len() - 1].1 - pts[0].1).0;
    if dt <= 0 || dv <= 0 {
        return None;
    }
    Rate::from_ratio(dv as u64, dt as u64)
}

fn collect_markers_and_children(
    parent: &XmlNode,
    parent_tl_start: Ticks,
    parent_src_origin: Ticks,
    ctx: &mut FcpxmlCtx,
) {
    for child in &parent.children {
        match child.tag.as_str() {
            "marker" | "chapter-marker" => {
                if let Some(st) = child.attr("start").and_then(parse_fcpxml_time) {
                    let rel = (st - parent_src_origin).max(Ticks::ZERO);
                    let tl_t =
                        (parent_tl_start + rel).clamp_safe(Ticks::ZERO, Ticks::from_raw(MAX_TICKS));
                    let lbl = child
                        .attr("value")
                        .or_else(|| child.attr("name"))
                        .unwrap_or("Marcador")
                        .trim();
                    ctx.markers.push(Marker {
                        id: new_id(),
                        t: tl_t,
                        label: if lbl.is_empty() {
                            "Marcador".into()
                        } else {
                            lbl.to_string()
                        },
                    });
                }
            }
            "asset-clip" | "clip" | "ref-clip" | "title" | "video" | "audio" | "sync-clip" => {
                if ctx.clip_count >= MAX_FCPXML_CLIPS {
                    continue;
                }
                let child_off = child
                    .attr("offset")
                    .and_then(parse_fcpxml_time)
                    .unwrap_or(parent_src_origin);
                let rel = (child_off - parent_src_origin).max(Ticks::ZERO);
                let tl_start =
                    (parent_tl_start + rel).clamp_safe(Ticks::ZERO, Ticks::from_raw(MAX_TICKS));
                let lane = child
                    .attr("lane")
                    .and_then(|s| s.parse::<i32>().ok())
                    .unwrap_or(0);
                import_clip_node(child, tl_start, lane, None, ctx);
            }
            _ => {}
        }
    }
}

fn extract_title_text(node: &XmlNode) -> String {
    let mut parts = Vec::new();
    for ch in &node.children {
        if ch.tag == "text" {
            let mut buf = String::new();
            ch.collect_text(&mut buf);
            let t = buf.trim();
            if !t.is_empty() {
                parts.push(t.to_string());
            }
        }
    }
    parts.join("\n")
}

#[derive(Clone, Copy)]
struct PendingTransition {
    kind: TransitionKind,
    dur: Ticks,
    cut_pos: Ticks,
}

fn import_clip_node(
    node: &XmlNode,
    tl_start: Ticks,
    lane: i32,
    pending_tr: Option<PendingTransition>,
    ctx: &mut FcpxmlCtx,
) {
    if ctx.clip_count >= MAX_FCPXML_CLIPS {
        return;
    }
    ctx.clip_count += 1;

    let min_len = ctx.fps.frame_to_ticks(1).max(Ticks::from_millis_i64(1));
    let dur = node
        .attr("duration")
        .and_then(parse_fcpxml_time)
        .filter(|d| *d > Ticks::ZERO)
        .unwrap_or_else(|| Ticks::from_millis_i64(DEFAULT_FCPXML_MS))
        .clamp_safe(min_len, Ticks::MAX);
    let src_start = node
        .attr("start")
        .and_then(parse_fcpxml_time)
        .unwrap_or(Ticks::ZERO)
        .max(Ticks::ZERO);
    let rate = parse_timemap_rate(node);
    let name = node.attr("name").unwrap_or("").trim().to_string();

    if node.tag == "title" {
        let title_text = extract_title_text(node);
        let eff_name = if name.is_empty() {
            node.attr("ref")
                .and_then(|r| ctx.effects.get(r))
                .cloned()
                .unwrap_or_else(|| "Título".into())
        } else {
            name.clone()
        };
        let item = ctx.ensure_text_media(&title_text, &eff_name, dur);
        let mut ev = Event::new(&item, tl_start, dur);
        ev.name = eff_name;
        ev.offset = Ticks::ZERO;
        ev.looped = false;
        if let Some(r) = rate {
            ev.rate = r;
        }
        ctx.video_track(lane).events.push(ev);
        collect_markers_and_children(node, tl_start, src_start, ctx);
        return;
    }

    // Determina ref (asset-clip tem @ref diretamente; clip/sync-clip podem ter filho <video>/<audio>).
    let direct_ref = node.attr("ref");
    let nested_video_ref = node.child("video").and_then(|v| v.attr("ref"));
    let nested_audio_ref = node.child("audio").and_then(|a| a.attr("ref"));
    let ref_id = direct_ref
        .or(nested_video_ref)
        .or(nested_audio_ref)
        .unwrap_or("");

    let item = ctx.ensure_media_file(ref_id, &name);
    let clip_name = if name.is_empty() {
        item.name.clone()
    } else {
        name
    };

    if lane < 0 {
        let mut ev = Event::new(&item, tl_start, dur);
        ev.name = clip_name;
        ev.offset = src_start;
        if let Some(r) = rate {
            ev.rate = r;
        }
        ctx.audio_track(lane).events.push(ev);
    } else {
        let audio_role_disabled = node
            .child("audio")
            .is_some_and(|a| a.attr("enabled") == Some("0"));
        let emit_audio = lane == 0 && item.has_audio && !audio_role_disabled;
        let group = if emit_audio && item.has_video {
            Some(new_id())
        } else {
            None
        };

        if item.has_video || !emit_audio {
            let mut ev = Event::new(&item, tl_start, dur);
            ev.name = clip_name.clone();
            ev.offset = src_start;
            ev.group = group;
            if let Some(r) = rate {
                ev.rate = r;
            }
            if let Some(tr) = pending_tr {
                if tr.cut_pos == tl_start {
                    let half = Ticks::from_raw(tr.dur.0 / 2);
                    let overlap_in = half.min(ev.start);
                    if overlap_in > Ticks::ZERO {
                        ev.start -= overlap_in;
                        ev.length = (ev.length + overlap_in).clamp_safe(min_len, Ticks::MAX);
                        let m_shift = ev.rate.scale_ticks(overlap_in);
                        ev.offset = (ev.offset - m_shift).max(Ticks::ZERO);
                    }
                    if let Some(prev) = ctx.video_track(lane).events.last_mut() {
                        let out_ext = tr.dur - overlap_in;
                        if out_ext > Ticks::ZERO {
                            prev.length = (prev.length + out_ext).clamp_safe(min_len, Ticks::MAX);
                        }
                    }
                    ev.transition = Some(TransitionSpec {
                        kind: tr.kind,
                        softness: 0.0,
                    });
                }
            }
            ctx.video_track(lane).events.push(ev);
        }

        if emit_audio {
            let mut a_ev = Event::new(&item, tl_start, dur);
            a_ev.name = clip_name;
            a_ev.offset = src_start;
            a_ev.group = group;
            if let Some(r) = rate {
                a_ev.rate = r;
            }
            ctx.audio_track(0).events.push(a_ev);
        }
    }

    collect_markers_and_children(node, tl_start, src_start, ctx);
}

pub(crate) fn parse_transition_kind_name(name: &str) -> TransitionKind {
    let lower = name.to_ascii_lowercase();
    if lower.contains("dip") || lower.contains("fade to color") {
        TransitionKind::FadeThroughBlack
    } else if lower.contains("push") && lower.contains("left") {
        TransitionKind::PushLeft
    } else if lower.contains("push") && lower.contains("right") {
        TransitionKind::PushRight
    } else if lower.contains("wipe") && lower.contains("left") {
        TransitionKind::WipeLeft
    } else if lower.contains("wipe") && lower.contains("right") {
        TransitionKind::WipeRight
    } else if lower.contains("iris") {
        TransitionKind::IrisOpen
    } else if lower.contains("clock") {
        TransitionKind::ClockWipe
    } else {
        TransitionKind::Dissolve
    }
}

/// Faz parse em sala limpa de texto Final Cut Pro XML (`.fcpxml`) em um `Project`.
pub fn import_fcpxml_str(text: &str) -> Result<(Project, Vec<String>), NeovegError> {
    if text.len() > MAX_FCPXML_BYTES {
        return Err(NeovegError::InvalidInput(format!(
            "arquivo FCPXML excede o limite de {} MiB",
            MAX_FCPXML_BYTES / (1024 * 1024)
        )));
    }
    let root = parse_xml_document(text)?;
    if root.tag != "fcpxml" {
        return Err(NeovegError::InvalidInput(format!(
            "elemento raiz esperado <fcpxml>, encontrado <{}>",
            root.tag
        )));
    }

    let mut ctx = FcpxmlCtx {
        formats: HashMap::new(),
        assets: HashMap::new(),
        effects: HashMap::new(),
        media: Vec::new(),
        media_idx: HashMap::new(),
        video_lanes: HashMap::new(),
        audio_lanes: HashMap::new(),
        markers: Vec::new(),
        warnings: Vec::new(),
        clip_count: 0,
        fps: FrameRate::FPS_24,
        width: 1920,
        height: 1080,
    };

    parse_resources(&root, &mut ctx);

    let seq = root.find_descendant("sequence").ok_or_else(|| {
        NeovegError::InvalidInput("documento FCPXML não possui elemento <sequence>".into())
    })?;
    if let Some(fmt_id) = seq.attr("format") {
        if let Some(fmt) = ctx.formats.get(fmt_id) {
            ctx.width = fmt.width;
            ctx.height = fmt.height;
            if let Some(fps) = fmt.fps {
                ctx.fps = fps;
            }
        }
    }
    let tc_format_df = seq
        .attr("tcFormat")
        .is_some_and(|s| s.eq_ignore_ascii_case("DF"));
    let tc_start = seq
        .attr("tcStart")
        .and_then(parse_fcpxml_time)
        .unwrap_or(Ticks::ZERO)
        .max(Ticks::ZERO);
    let project_title = root
        .find_descendant("project")
        .and_then(|p| p.attr("name"))
        .unwrap_or("")
        .trim()
        .to_string();

    if let Some(spine) = seq.child("spine") {
        let mut cursor = Ticks::ZERO;
        let mut pending_tr: Option<PendingTransition> = None;

        for node in &spine.children {
            if ctx.clip_count >= MAX_FCPXML_CLIPS {
                ctx.warnings.push(format!(
                    "limite de {MAX_FCPXML_CLIPS} clipes FCPXML atingido"
                ));
                break;
            }
            let explicit_tl = node
                .attr("offset")
                .and_then(parse_fcpxml_time)
                .map(|o| (o - tc_start).max(Ticks::ZERO));

            match node.tag.as_str() {
                "gap" => {
                    let tl_start = explicit_tl.unwrap_or(cursor);
                    let src_origin = node
                        .attr("start")
                        .and_then(parse_fcpxml_time)
                        .unwrap_or(Ticks::ZERO);
                    collect_markers_and_children(node, tl_start, src_origin, &mut ctx);
                    let dur = node
                        .attr("duration")
                        .and_then(parse_fcpxml_time)
                        .unwrap_or(Ticks::ZERO)
                        .max(Ticks::ZERO);
                    cursor = tl_start + dur;
                    pending_tr = None;
                }
                "transition" => {
                    let dur = node
                        .attr("duration")
                        .and_then(parse_fcpxml_time)
                        .unwrap_or_else(|| ctx.fps.frame_to_ticks(24))
                        .clamp_safe(Ticks::from_millis_i64(1), Ticks::from_secs_i64(30));
                    let name = node.attr("name").unwrap_or("Cross Dissolve");
                    pending_tr = Some(PendingTransition {
                        kind: parse_transition_kind_name(name),
                        dur,
                        cut_pos: explicit_tl.unwrap_or(cursor),
                    });
                }
                "asset-clip" | "clip" | "ref-clip" | "title" | "video" | "audio" | "sync-clip" => {
                    let tl_start = explicit_tl.unwrap_or(cursor);
                    let lane = node
                        .attr("lane")
                        .and_then(|s| s.parse::<i32>().ok())
                        .unwrap_or(0);
                    let dur = node
                        .attr("duration")
                        .and_then(parse_fcpxml_time)
                        .filter(|d| *d > Ticks::ZERO)
                        .unwrap_or_else(|| Ticks::from_millis_i64(DEFAULT_FCPXML_MS));
                    import_clip_node(node, tl_start, lane, pending_tr.take(), &mut ctx);
                    cursor = tl_start + dur;
                }
                _ => {}
            }
        }
    }

    collect_markers_and_children(seq, Ticks::ZERO, tc_start, &mut ctx);

    let mut project = Project::new_default();
    project.tracks.clear();
    project.media = ctx.media;
    project.markers = ctx.markers;
    project.regions.clear();
    project.notes = project_title;
    project.settings.width = ctx.width;
    project.settings.height = ctx.height;
    project.settings.fps = ctx.fps;
    project.settings.drop_frame = tc_format_df && ctx.fps.is_ntsc_drop_eligible();

    let mut v_keys: Vec<i32> = ctx.video_lanes.keys().copied().collect();
    v_keys.sort();
    for k in v_keys {
        if let Some(tr) = ctx.video_lanes.remove(&k) {
            project.tracks.push(tr);
        }
    }
    let mut a_keys: Vec<i32> = ctx.audio_lanes.keys().copied().collect();
    a_keys.sort_by(|a, b| b.cmp(a));
    for k in a_keys {
        if let Some(tr) = ctx.audio_lanes.remove(&k) {
            project.tracks.push(tr);
        }
    }
    if project.tracks.is_empty() {
        project.tracks.push(Track::new(TrackKind::Video, "Vídeo 1"));
    }

    let mut fixes = ctx.warnings;
    fixes.extend(project.sanitize());
    crate::model::bump_ids(project.max_id());
    Ok((project, fixes))
}

/// Exporta o projeto Neoveg no formato Final Cut Pro XML (`.fcpxml` 1.10).
pub fn export_fcpxml_string(project: &Project, name_override: Option<&str>) -> String {
    let fps = project.settings.fps;
    let w = project.settings.width;
    let h = project.settings.height;
    let total_dur = project.duration();
    let frame_dur = fps.frame_to_ticks(1);
    let proj_name = name_override
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

    let mut asset_ids: HashMap<Id, String> = HashMap::new();
    let mut res_xml = String::new();
    let fd_str = format_fcpxml_time(frame_dur, fps);
    res_xml.push_str(&format!(
        "        <format id=\"r1\" name=\"FFVideoFormat{}p{}\" frameDuration=\"{fd_str}\" width=\"{w}\" height=\"{h}\"/>\n",
        h,
        fps.nominal_fps()
    ));

    let mut next_res = 2usize;
    let mut has_title = false;
    for m in &project.media {
        if matches!(m.source, MediaSource::Generated(GenSpec::Text(_))) {
            has_title = true;
            continue;
        }
        let rid = format!("r{next_res}");
        next_res += 1;
        let dur_ticks = m
            .duration
            .unwrap_or_else(|| Ticks::from_millis_i64(DEFAULT_FCPXML_MS))
            .max(frame_dur);
        let src = match &m.source {
            MediaSource::File { path } | MediaSource::NestedProject { path } => {
                encode_target_url(path)
            }
            MediaSource::ImageSequence { paths, .. } => paths
                .first()
                .map(|p| encode_target_url(p))
                .unwrap_or_default(),
            MediaSource::Generated(g) => g.label(),
        };
        res_xml.push_str(&format!(
            "        <asset id=\"{rid}\" name=\"{}\" src=\"{}\" start=\"0s\" duration=\"{}\" hasVideo=\"{}\" hasAudio=\"{}\" format=\"r1\"/>\n",
            escape_xml(&m.name),
            escape_xml(&src),
            format_fcpxml_time(dur_ticks, fps),
            if m.has_video { "1" } else { "0" },
            if m.has_audio { "1" } else { "0" },
        ));
        asset_ids.insert(m.id, rid);
    }

    let title_eff_id = if has_title {
        let rid = format!("r{next_res}");
        res_xml.push_str(&format!(
            "        <effect id=\"{rid}\" name=\"Basic Title\" uid=\".../Titles.localized/Bumper:Opener.localized/Basic Title.localized/Basic Title.moti\"/>\n"
        ));
        Some(rid)
    } else {
        None
    };

    // Coleta eventos conectados (lane != 0) para ancorar dentro do 1º item do spine (em 0s).
    let video_tracks: Vec<&Track> = project
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Video)
        .collect();
    let audio_tracks: Vec<&Track> = project
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Audio)
        .collect();

    let mut primary_events: Vec<&Event> = video_tracks
        .first()
        .map(|tr| tr.events.iter().collect())
        .unwrap_or_default();
    primary_events.sort_by_key(|e| (e.start, e.id));

    // Coleta itens de outras faixas + marcadores para emitir em lanes conectadas.
    let mut connected_xml = String::new();
    for (v_idx, tr) in video_tracks.iter().enumerate().skip(1) {
        let lane = v_idx as i32;
        let mut sorted: Vec<&Event> = tr.events.iter().collect();
        sorted.sort_by_key(|e| (e.start, e.id));
        for ev in sorted {
            connected_xml.push_str(&format_fcpxml_clip_line(
                project,
                ev,
                Some(lane),
                ev.start,
                &asset_ids,
                title_eff_id.as_deref(),
                fps,
                "                        ",
            ));
        }
    }
    for (a_idx, tr) in audio_tracks.iter().enumerate() {
        let lane = -((a_idx as i32) + 1);
        let mut sorted: Vec<&Event> = tr.events.iter().collect();
        sorted.sort_by_key(|e| (e.start, e.id));
        for ev in sorted {
            // Evita duplicar áudio já implicitamente agrupado com evento na trilha principal 0.
            let covered_by_primary = ev.group.is_some_and(|g| {
                primary_events
                    .iter()
                    .any(|pe| pe.group == Some(g) && pe.start == ev.start && pe.length == ev.length)
            });
            if covered_by_primary {
                continue;
            }
            connected_xml.push_str(&format_fcpxml_clip_line(
                project,
                ev,
                Some(lane),
                ev.start,
                &asset_ids,
                title_eff_id.as_deref(),
                fps,
                "                        ",
            ));
        }
    }

    let mut markers_xml = String::new();
    for mk in &project.markers {
        markers_xml.push_str(&format!(
            "                        <marker start=\"{}\" duration=\"{fd_str}\" value=\"{}\"/>\n",
            format_fcpxml_time(mk.t, fps),
            escape_xml(&mk.label),
        ));
    }

    let mut spine_xml = String::new();
    let mut cursor = Ticks::ZERO;
    let anchor_inserted_in_gap = primary_events.first().is_none_or(|e| e.start > Ticks::ZERO);
    let has_anchors = !connected_xml.is_empty() || !markers_xml.is_empty();

    if primary_events.is_empty() {
        let gap_dur = total_dur.max(frame_dur);
        if has_anchors {
            spine_xml.push_str(&format!(
                "                    <gap name=\"Gap\" offset=\"0s\" start=\"0s\" duration=\"{}\">\n{}{}\n                    </gap>\n",
                format_fcpxml_time(gap_dur, fps),
                connected_xml.trim_end(),
                if markers_xml.is_empty() { String::new() } else { format!("\n{}", markers_xml.trim_end()) },
            ));
        } else {
            spine_xml.push_str(&format!(
                "                    <gap name=\"Gap\" offset=\"0s\" start=\"0s\" duration=\"{}\"/>\n",
                format_fcpxml_time(gap_dur, fps)
            ));
        }
    } else {
        for (idx, ev) in primary_events.iter().enumerate() {
            if ev.start > cursor {
                let gap_len = ev.start - cursor;
                if idx == 0 && anchor_inserted_in_gap && has_anchors {
                    spine_xml.push_str(&format!(
                        "                    <gap name=\"Gap\" offset=\"{}\" start=\"0s\" duration=\"{}\">\n{}{}\n                    </gap>\n",
                        format_fcpxml_time(cursor, fps),
                        format_fcpxml_time(gap_len, fps),
                        connected_xml,
                        markers_xml,
                    ));
                } else {
                    spine_xml.push_str(&format!(
                        "                    <gap name=\"Gap\" offset=\"{}\" start=\"0s\" duration=\"{}\"/>\n",
                        format_fcpxml_time(cursor, fps),
                        format_fcpxml_time(gap_len, fps),
                    ));
                }
                cursor = ev.start;
            } else if ev.start < cursor {
                let ov = (cursor - ev.start).max(frame_dur);
                let tr_name = ev
                    .transition
                    .as_ref()
                    .map_or("Cross Dissolve", |t| match t.kind {
                        TransitionKind::FadeThroughBlack => "Fade to Color",
                        TransitionKind::WipeLeft => "Wipe Left",
                        TransitionKind::WipeRight => "Wipe Right",
                        TransitionKind::PushLeft => "Push Left",
                        TransitionKind::PushRight => "Push Right",
                        TransitionKind::IrisOpen => "Iris",
                        TransitionKind::ClockWipe => "Clock Wipe",
                        _ => "Cross Dissolve",
                    });
                spine_xml.push_str(&format!(
                    "                    <transition name=\"{tr_name}\" offset=\"{}\" duration=\"{}\"/>\n",
                    format_fcpxml_time(ev.start, fps),
                    format_fcpxml_time(ov, fps),
                ));
            }

            let extra = if idx == 0 && !anchor_inserted_in_gap && has_anchors {
                // Conectados em offset relativos ao start do evento 0 (offset_fcpxml = ev0.offset + t).
                // Como gap não foi inserido antes porque ev0.start == 0, ajustamos caso ev0.offset == 0.
                if ev.offset == Ticks::ZERO {
                    Some((connected_xml.as_str(), markers_xml.as_str()))
                } else {
                    None
                }
            } else {
                None
            };

            spine_xml.push_str(&format_fcpxml_clip_line_with_children(
                project,
                ev,
                None,
                ev.start,
                &asset_ids,
                title_eff_id.as_deref(),
                fps,
                "                    ",
                extra,
            ));
            cursor = cursor.max(ev.end());
        }

        // Caso evento 0 tinha offset > 0 e existiam marcadores/lanes em t=0..N, anexa ao <gap> final
        // ou recalcula em cima de um gap na sequência se ainda não emitidos.
        if !anchor_inserted_in_gap
            && has_anchors
            && primary_events
                .first()
                .is_some_and(|e| e.offset != Ticks::ZERO)
        {
            spine_xml.push_str(&format!(
                "                    <gap name=\"Anchor\" offset=\"0s\" start=\"0s\" duration=\"{fd_str}\">\n{}{}</gap>\n",
                connected_xml, markers_xml
            ));
        }
    }

    let tc_format = if project.settings.drop_frame && fps.is_ntsc_drop_eligible() {
        "DF"
    } else {
        "NDF"
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE fcpxml>\n<fcpxml version=\"1.10\">\n    <resources>\n{res_xml}    </resources>\n    <library>\n        <event name=\"Neoveg\">\n            <project name=\"{}\">\n                <sequence format=\"r1\" duration=\"{}\" tcStart=\"0s\" tcFormat=\"{tc_format}\" audioLayout=\"stereo\" audioRate=\"{}k\">\n                    <spine>\n{spine_xml}                    </spine>\n                </sequence>\n            </project>\n        </event>\n    </library>\n</fcpxml>\n",
        escape_xml(proj_name),
        format_fcpxml_time(total_dur, fps),
        project.settings.sample_rate / 1000,
    )
}

#[allow(clippy::too_many_arguments)]
fn format_fcpxml_clip_line(
    project: &Project,
    ev: &Event,
    lane: Option<i32>,
    tl_offset: Ticks,
    asset_ids: &HashMap<Id, String>,
    title_eff_id: Option<&str>,
    fps: FrameRate,
    indent: &str,
) -> String {
    format_fcpxml_clip_line_with_children(
        project,
        ev,
        lane,
        tl_offset,
        asset_ids,
        title_eff_id,
        fps,
        indent,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn format_fcpxml_clip_line_with_children(
    project: &Project,
    ev: &Event,
    lane: Option<i32>,
    tl_offset: Ticks,
    asset_ids: &HashMap<Id, String>,
    title_eff_id: Option<&str>,
    fps: FrameRate,
    indent: &str,
    extra: Option<(&str, &str)>,
) -> String {
    let media = project.media(ev.media_id);
    let lane_attr = lane.map(|l| format!(" lane=\"{l}\"")).unwrap_or_default();
    let off_s = format_fcpxml_time(tl_offset, fps);
    let start_s = format_fcpxml_time(ev.offset, fps);
    let dur_s = format_fcpxml_time(ev.length, fps);

    if let Some(m) = media {
        if let MediaSource::Generated(GenSpec::Text(spec)) = &m.source {
            let eff = title_eff_id.unwrap_or("r2");
            let mut out = format!(
                "{indent}<title name=\"{}\" ref=\"{eff}\"{lane_attr} offset=\"{off_s}\" start=\"0s\" duration=\"{dur_s}\">\n{indent}    <text><text-style ref=\"ts1\">{}</text-style></text>\n",
                escape_xml(&ev.name),
                escape_xml(&spec.text),
            );
            if let Some((conn, mks)) = extra {
                out.push_str(conn);
                out.push_str(mks);
            }
            out.push_str(&format!("{indent}</title>\n"));
            return out;
        }
    }

    let rid = asset_ids
        .get(&ev.media_id)
        .map(String::as_str)
        .unwrap_or("r1");
    let has_speed = !ev.rate.is_normal();
    if has_speed || extra.is_some() {
        let mut out = format!(
            "{indent}<asset-clip name=\"{}\" ref=\"{rid}\"{lane_attr} offset=\"{off_s}\" start=\"{start_s}\" duration=\"{dur_s}\">\n",
            escape_xml(&ev.name),
        );
        if has_speed {
            let val_dur = format_fcpxml_time(ev.rate.scale_ticks(ev.length), fps);
            out.push_str(&format!(
                "{indent}    <timeMap><timept time=\"0s\" value=\"0s\"/><timept time=\"{dur_s}\" value=\"{val_dur}\"/></timeMap>\n"
            ));
        }
        if let Some((conn, mks)) = extra {
            out.push_str(conn);
            out.push_str(mks);
        }
        out.push_str(&format!("{indent}</asset-clip>\n"));
        out
    } else {
        format!(
            "{indent}<asset-clip name=\"{}\" ref=\"{rid}\"{lane_attr} offset=\"{off_s}\" start=\"{start_s}\" duration=\"{dur_s}\"/>\n",
            escape_xml(&ev.name),
        )
    }
}

pub fn import_fcpxml_file(path: &Path) -> Result<(Project, Vec<String>), NeovegError> {
    let text = read_bounded_utf8_file(path, MAX_FCPXML_BYTES, "FCPXML")?;
    import_fcpxml_str(&text)
}

pub fn export_fcpxml_file(
    project: &Project,
    path: &Path,
    name_override: Option<&str>,
) -> Result<(), NeovegError> {
    let body = export_fcpxml_string(project, name_override);
    std::fs::write(path, body.as_bytes())
        .map_err(|e| NeovegError::Io(format!("erro ao gravar '{}': {e}", path.display())))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fcpxml_rational_times_lanes_titles_markers_and_roundtrip() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE fcpxml>
<fcpxml version="1.10">
    <resources>
        <format id="r1" name="FFVideoFormat1080p2997" frameDuration="1001/30000s" width="1920" height="1080"/>
        <asset id="r2" name="Entrevista.mp4" src="file:///C:/midia/entrevista.mp4" start="0s" duration="300300/30000s" hasVideo="1" hasAudio="1" format="r1"/>
        <effect id="r3" name="Basic Title" uid=".../Basic Title.moti"/>
    </resources>
    <library>
        <event name="Teste">
            <project name="Corte FCPXML">
                <sequence format="r1" duration="300300/30000s" tcStart="0s" tcFormat="DF">
                    <spine>
                        <asset-clip name="Entrevista" ref="r2" offset="0s" start="1001/30000s" duration="60060/30000s">
                            <title name="Legenda Capa" ref="r3" lane="1" offset="1001/30000s" duration="30030/30000s">
                                <text><text-style>Olá &amp; Bem-vindos</text-style></text>
                            </title>
                            <marker start="31031/30000s" value="Ponto Chave"/>
                        </asset-clip>
                    </spine>
                </sequence>
            </project>
        </event>
    </library>
</fcpxml>"#;

        let (p, warn) = import_fcpxml_str(xml).expect("parse fcpxml");
        assert!(warn.is_empty(), "avisos: {warn:?}");
        assert_eq!(p.notes, "Corte FCPXML");
        assert_eq!(p.settings.fps, FrameRate::FPS_29_97);
        assert!(p.settings.drop_frame);
        assert_eq!(p.markers.len(), 1);
        assert_eq!(p.markers[0].label, "Ponto Chave");
        assert_eq!(p.markers[0].t, FrameRate::FPS_29_97.frame_to_ticks(30));

        assert!(
            p.tracks
                .iter()
                .filter(|t| t.kind == TrackKind::Video)
                .count()
                >= 2
        );
        let title_track = &p.tracks[1];
        assert_eq!(title_track.events[0].start, Ticks::ZERO);
        assert_eq!(
            title_track.events[0].length,
            FrameRate::FPS_29_97.frame_to_ticks(30)
        );

        let exported = export_fcpxml_string(&p, None);
        assert!(exported.contains("<fcpxml version=\"1.10\">"));
        assert!(exported.contains("Olá &amp; Bem-vindos"));
        let (re_p, _) = import_fcpxml_str(&exported).expect("roundtrip fcpxml");
        assert_eq!(re_p.notes, "Corte FCPXML");
        assert_eq!(re_p.markers.len(), 1);
        assert_eq!(re_p.tracks[0].events.len(), 1);
    }

    #[test]
    fn fcpxml_rejects_deep_or_malformed_xml_safely() {
        let mut deep = String::new();
        for _ in 0..80 {
            deep.push_str("<a>");
        }
        for _ in 0..80 {
            deep.push_str("</a>");
        }
        assert!(import_fcpxml_str(&deep).is_err());
        assert!(import_fcpxml_str("<fcpxml><unclosed></fcpxml>").is_err());
    }
}
