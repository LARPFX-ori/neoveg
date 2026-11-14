//! Importação e exportação em sala limpa (*Clean Room*, `AGENTS.md` §2.3) de
//! **Final Cut Pro 7 XML** (`xmeml`, versões 4 e 5), a partir do documento
//! público da Apple *Final Cut Pro XML Interchange Format* (edição para Final
//! Cut Pro 7, `xmeml` versão 5).
//!
//! Reusa o analisador XML limitado e livre de pânicos de [`crate::fcpxml`].
//! Tempos do `xmeml` são inteiros de quadro (`<start>`, `<end>`, `<in>`,
//! `<out>`) na taxa `<rate>` (`<timebase>` + `<ntsc>`), convertidos para
//! `Ticks` exatos sem ponto flutuante.

use crate::fcpxml::parse_transition_kind_name;
use crate::fcpxml::{escape_xml, parse_xml_document, XmlNode};
use crate::model::{
    new_id, Event, Id, Marker, MediaItem, MediaSource, Project, Track, TrackKind, TransitionSpec,
    MAX_DIM,
};
use crate::otio::{decode_target_url, encode_target_url, read_bounded_utf8_file};
use neoveg_common::error::NeovegError;
use neoveg_common::time::{FrameRate, Ticks};
use std::collections::HashMap;
use std::path::Path;

/// Limite de tamanho de arquivo FCP7 XML lido (`16 MiB`, `AGENTS.md` §0.4).
pub const MAX_FCP7_BYTES: usize = 16 * 1024 * 1024;
const MAX_FCP7_CLIPS: usize = 20_000;
const MAX_FCP7_TRACKS: usize = 64;
/// Maior quadro aceito (~115 h a 120 fps); acima disso o valor é rejeitado.
const MAX_FRAMES: i64 = 50_000_000;
const MAX_TIMEBASE: u32 = 1000;

// ---------------------------------------------------------------------------
// Leitura de valores
// ---------------------------------------------------------------------------

fn child_text<'a>(n: &'a XmlNode, tag: &str) -> Option<&'a str> {
    n.child(tag).map(|c| c.text.trim())
}

fn child_bool(n: &XmlNode, tag: &str) -> Option<bool> {
    child_text(n, tag).map(|s| s.eq_ignore_ascii_case("TRUE"))
}

/// Quadro inteiro de um filho; `-1` é preservado (borda definida pela transição).
fn child_frames(n: &XmlNode, tag: &str) -> Option<i64> {
    let v: i64 = child_text(n, tag)?.parse().ok()?;
    if v == -1 || (0..=MAX_FRAMES).contains(&v) {
        Some(v)
    } else {
        None
    }
}

/// `<rate><timebase>30</timebase><ntsc>TRUE</ntsc></rate>` → 30000/1001.
fn parse_rate(n: &XmlNode) -> Option<FrameRate> {
    let r = n.child("rate")?;
    let tb: u32 = child_text(r, "timebase")?
        .parse()
        .ok()
        .filter(|v| (1..=MAX_TIMEBASE).contains(v))?;
    if child_bool(r, "ntsc").unwrap_or(false) {
        FrameRate::new(tb * 1000, 1001)
    } else {
        FrameRate::new(tb, 1)
    }
}

fn frames_to_ticks(f: i64, fps: FrameRate) -> Ticks {
    fps.frame_to_ticks(f.clamp(0, MAX_FRAMES))
}

/// Junta todos os descendentes com a marca `tag` (a profundidade já é limitada
/// pelo analisador, §0.5).
fn collect_descendants<'a>(n: &'a XmlNode, tag: &str, out: &mut Vec<&'a XmlNode>) {
    for c in &n.children {
        if c.tag == tag {
            out.push(c);
        }
        collect_descendants(c, tag, out);
    }
}

// ---------------------------------------------------------------------------
// Importação
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct FileDef {
    name: String,
    path: String,
    duration: Option<i64>,
    rate: Option<FrameRate>,
    width: u32,
    height: u32,
    has_video: bool,
    has_audio: bool,
}

fn parse_file_def(f: &XmlNode) -> Option<FileDef> {
    let path = child_text(f, "pathurl").map(decode_target_url);
    let name = child_text(f, "name").map(str::to_string);
    if path.is_none() && name.is_none() {
        return None; // referência (`<file id="file-1"/>`), não definição
    }
    let media = f.child("media");
    let video = media.and_then(|m| m.child("video"));
    let sc = video.and_then(|v| v.find_descendant("samplecharacteristics"));
    let dim = |tag: &str| {
        sc.and_then(|s| child_text(s, tag))
            .and_then(|v| v.parse::<u32>().ok())
            .map(|v| v.min(MAX_DIM))
            .unwrap_or(0)
    };
    Some(FileDef {
        name: name.clone().unwrap_or_default(),
        path: path.or(name).unwrap_or_default(),
        duration: child_frames(f, "duration").filter(|d| *d > 0),
        rate: parse_rate(f),
        width: dim("width"),
        height: dim("height"),
        has_video: video.is_some(),
        has_audio: media.and_then(|m| m.child("audio")).is_some(),
    })
}

enum TrackItem<'a> {
    Clip(&'a XmlNode),
    Transition { start: i64, end: i64, name: String },
}

struct Ctx {
    fps: FrameRate,
    files: HashMap<String, FileDef>,
    media: Vec<MediaItem>,
    media_by_file: HashMap<String, usize>,
    /// clipitem id → id do evento criado
    clip_events: HashMap<String, Id>,
    links: Vec<Vec<String>>,
    clips: usize,
    warnings: Vec<String>,
}

impl Ctx {
    fn media_for(&mut self, clip: &XmlNode, kind: TrackKind) -> Option<MediaItem> {
        let f = clip.child("file")?;
        let fid = f.attr("id").unwrap_or("").to_string();
        let key = if fid.is_empty() {
            child_text(f, "pathurl").unwrap_or("").to_string()
        } else {
            fid.clone()
        };
        if let Some(&i) = self.media_by_file.get(&key) {
            let m = self.media.get_mut(i)?;
            match kind {
                TrackKind::Video => m.has_video = true,
                TrackKind::Audio => m.has_audio = true,
            }
            return Some(m.clone());
        }
        let def = self
            .files
            .get(&fid)
            .cloned()
            .or_else(|| parse_file_def(f))?;
        let rate = def.rate.unwrap_or(self.fps);
        let item = MediaItem {
            id: new_id(),
            name: if def.name.is_empty() {
                def.path.clone()
            } else {
                def.name.clone()
            },
            source: MediaSource::File { path: def.path },
            duration: def.duration.map(|d| frames_to_ticks(d, rate)),
            has_video: def.has_video || kind == TrackKind::Video,
            has_audio: def.has_audio || kind == TrackKind::Audio,
            width: def.width,
            height: def.height,
            fps: def.rate,
            info: "Importado via FCP7 XML".into(),
            proxy_path: None,
        };
        self.media_by_file.insert(key, self.media.len());
        self.media.push(item.clone());
        Some(item)
    }

    fn import_track(&mut self, track_node: &XmlNode, kind: TrackKind, name: String) -> Track {
        let mut track = Track::new(kind, &name);
        if child_bool(track_node, "enabled") == Some(false) {
            track.mute = true;
        }
        track.locked = child_bool(track_node, "locked").unwrap_or(false);
        let items: Vec<TrackItem> = track_node
            .children
            .iter()
            .filter_map(|c| match c.tag.as_str() {
                "clipitem" => Some(TrackItem::Clip(c)),
                "transitionitem" => Some(TrackItem::Transition {
                    start: child_frames(c, "start").unwrap_or(-1),
                    end: child_frames(c, "end").unwrap_or(-1),
                    name: c
                        .child("effect")
                        .and_then(|e| child_text(e, "name"))
                        .unwrap_or("Cross Dissolve")
                        .to_string(),
                }),
                "generatoritem" => {
                    self.warnings.push(format!(
                        "gerador FCP7 '{}' ignorado (sem equivalente)",
                        child_text(c, "name").unwrap_or("?")
                    ));
                    None
                }
                _ => None,
            })
            .collect();

        for (i, item) in items.iter().enumerate() {
            let TrackItem::Clip(clip) = item else {
                continue;
            };
            if self.clips >= MAX_FCP7_CLIPS {
                self.warnings
                    .push(format!("limite de {MAX_FCP7_CLIPS} clipes FCP7 atingido"));
                break;
            }
            let prev_tr = i
                .checked_sub(1)
                .and_then(|j| items.get(j))
                .and_then(|t| match t {
                    TrackItem::Transition { start, end, name } => Some((*start, *end, name)),
                    TrackItem::Clip(_) => None,
                });
            let next_tr = items.get(i + 1).and_then(|t| match t {
                TrackItem::Transition { start, end, .. } => Some((*start, *end)),
                TrackItem::Clip(_) => None,
            });
            if let Some(ev) = self.import_clip(clip, kind, prev_tr, next_tr) {
                if let Some(cid) = clip.attr("id") {
                    self.clip_events.insert(cid.to_string(), ev.id);
                    let mut linked: Vec<String> = clip
                        .children
                        .iter()
                        .filter(|c| c.tag == "link")
                        .filter_map(|l| child_text(l, "linkclipref").map(str::to_string))
                        .collect();
                    if !linked.is_empty() {
                        linked.push(cid.to_string());
                        self.links.push(linked);
                    }
                }
                self.clips += 1;
                track.events.push(ev);
            }
        }
        track
    }

    fn import_clip(
        &mut self,
        clip: &XmlNode,
        kind: TrackKind,
        prev_tr: Option<(i64, i64, &String)>,
        next_tr: Option<(i64, i64)>,
    ) -> Option<Event> {
        let fps = self.fps;
        let clip_rate = parse_rate(clip).unwrap_or(fps);
        let name = child_text(clip, "name").unwrap_or("Clipe").to_string();
        let mut start = child_frames(clip, "start").unwrap_or(-1);
        let mut end = child_frames(clip, "end").unwrap_or(-1);
        let in_f = child_frames(clip, "in").filter(|v| *v >= 0);
        let out_f = child_frames(clip, "out").filter(|v| *v >= 0);
        let src_len = match (in_f, out_f) {
            (Some(a), Some(b)) if b > a => Some(b - a),
            _ => None,
        };
        let mut transition = None;
        let start_from_tr = start < 0;
        if start < 0 {
            if let Some((ts, _, tname)) = prev_tr.filter(|t| t.0 >= 0) {
                start = ts;
                transition = Some(parse_transition_kind_name(tname));
            }
        }
        if end < 0 {
            if let Some((_, te)) = next_tr.filter(|t| t.1 >= 0) {
                end = te;
            }
        }
        if start < 0 {
            start = match (end, src_len) {
                (e, Some(l)) if e >= 0 => (e - l).max(0),
                _ => 0,
            };
        }
        if end < 0 {
            end = start.saturating_add(src_len.unwrap_or(0));
        }
        if end <= start {
            self.warnings
                .push(format!("clipe FCP7 '{name}' sem duração válida ignorado"));
            return None;
        }
        let item = match self.media_for(clip, kind) {
            Some(m) => m,
            None => {
                self.warnings
                    .push(format!("clipe FCP7 '{name}' sem arquivo de mídia ignorado"));
                return None;
            }
        };
        let tl_start = frames_to_ticks(start, fps);
        let length = frames_to_ticks(end, fps) - tl_start;
        // ancora a mídia pela borda conhecida: com início vindo da transição,
        // o `<out>` coincide com o fim; senão o `<in>` coincide com o início.
        let offset_frames = match (start_from_tr, in_f, out_f) {
            (true, _, Some(o)) => {
                let len_src = clip_rate.ticks_to_frame_round(length);
                (o - len_src).max(0)
            }
            (_, Some(i), _) => i,
            _ => 0,
        };
        let mut ev = Event::new(&item, tl_start, length);
        ev.name = name;
        ev.offset = frames_to_ticks(offset_frames, clip_rate);
        ev.looped = false;
        if child_bool(clip, "enabled") == Some(false) {
            ev.mute = true;
        }
        ev.transition = transition.map(|kind| TransitionSpec {
            kind,
            softness: 0.0,
        });
        Some(ev)
    }
}

/// Faz parse em sala limpa de texto FCP7 XML (`xmeml`) em um `Project`.
pub fn import_fcp7_str(text: &str) -> Result<(Project, Vec<String>), NeovegError> {
    if text.len() > MAX_FCP7_BYTES {
        return Err(NeovegError::InvalidInput(format!(
            "arquivo FCP7 XML excede o limite de {} MiB",
            MAX_FCP7_BYTES / (1024 * 1024)
        )));
    }
    let root = parse_xml_document(text)?;
    if root.tag != "xmeml" {
        return Err(NeovegError::InvalidInput(format!(
            "elemento raiz esperado <xmeml>, encontrado <{}>",
            root.tag
        )));
    }
    let seq = root.find_descendant("sequence").ok_or_else(|| {
        NeovegError::InvalidInput("documento FCP7 XML não possui elemento <sequence>".into())
    })?;
    let fps = parse_rate(seq).unwrap_or(FrameRate::FPS_30);

    let mut file_nodes = Vec::new();
    collect_descendants(&root, "file", &mut file_nodes);
    let mut files = HashMap::new();
    for f in file_nodes {
        if let (Some(id), Some(def)) = (f.attr("id"), parse_file_def(f)) {
            files.entry(id.to_string()).or_insert(def);
        }
    }
    let mut ctx = Ctx {
        fps,
        files,
        media: vec![],
        media_by_file: HashMap::new(),
        clip_events: HashMap::new(),
        links: vec![],
        clips: 0,
        warnings: vec![],
    };

    let media = seq.child("media");
    let video = media.and_then(|m| m.child("video"));
    let audio = media.and_then(|m| m.child("audio"));
    let (mut width, mut height) = (1920u32, 1080u32);
    if let Some(sc) = video
        .and_then(|v| v.child("format"))
        .and_then(|f| f.child("samplecharacteristics"))
    {
        let dim = |tag: &str| {
            child_text(sc, tag)
                .and_then(|v| v.parse::<u32>().ok())
                .filter(|v| (16..=MAX_DIM).contains(v))
        };
        width = dim("width").unwrap_or(width);
        height = dim("height").unwrap_or(height);
    }

    let mut tracks: Vec<Track> = vec![];
    let v_nodes: Vec<&XmlNode> = video
        .map(|v| v.children.iter().filter(|c| c.tag == "track").collect())
        .unwrap_or_default();
    let a_nodes: Vec<&XmlNode> = audio
        .map(|a| a.children.iter().filter(|c| c.tag == "track").collect())
        .unwrap_or_default();
    if v_nodes.len() + a_nodes.len() > MAX_FCP7_TRACKS {
        ctx.warnings.push(format!(
            "limite de {MAX_FCP7_TRACKS} trilhas FCP7 atingido; trilhas extras ignoradas"
        ));
    }
    let v_take = v_nodes.len().min(MAX_FCP7_TRACKS);
    let a_take = a_nodes.len().min(MAX_FCP7_TRACKS - v_take);
    // V1 é a camada de baixo no FCP7; no Neoveg a trilha de cima vem primeiro.
    let mut v_tracks: Vec<Track> = v_nodes
        .iter()
        .take(v_take)
        .enumerate()
        .map(|(i, n)| ctx.import_track(n, TrackKind::Video, format!("Vídeo {}", i + 1)))
        .collect();
    v_tracks.reverse();
    tracks.extend(v_tracks);
    for (i, n) in a_nodes.iter().take(a_take).enumerate() {
        let t = ctx.import_track(n, TrackKind::Audio, format!("Áudio {}", i + 1));
        tracks.push(t);
    }

    // `<link>` liga clipes de vídeo e áudio do mesmo trecho → grupo de eventos.
    let mut group_of: HashMap<Id, Id> = HashMap::new();
    for set in &ctx.links {
        let ids: Vec<Id> = set
            .iter()
            .filter_map(|c| ctx.clip_events.get(c).copied())
            .collect();
        if ids.len() < 2 {
            continue;
        }
        let g = ids
            .iter()
            .find_map(|id| group_of.get(id).copied())
            .unwrap_or_else(new_id);
        for id in ids {
            group_of.insert(id, g);
        }
    }
    for t in &mut tracks {
        for e in &mut t.events {
            if let Some(g) = group_of.get(&e.id) {
                e.group = Some(*g);
            }
        }
    }

    let markers: Vec<Marker> = seq
        .children
        .iter()
        .filter(|c| c.tag == "marker")
        .filter_map(|m| {
            let f = child_frames(m, "in").filter(|v| *v >= 0)?;
            Some(Marker {
                id: new_id(),
                t: frames_to_ticks(f, fps),
                label: child_text(m, "name").unwrap_or("").to_string(),
            })
        })
        .collect();

    let mut project = Project::new_default();
    project.tracks = tracks;
    if project.tracks.is_empty() {
        project.tracks.push(Track::new(TrackKind::Video, "Vídeo 1"));
    }
    project.media = ctx.media;
    project.markers = markers;
    project.regions.clear();
    project.notes = child_text(seq, "name").unwrap_or("").to_string();
    project.settings.width = width;
    project.settings.height = height;
    project.settings.fps = fps;
    let df = seq
        .child("timecode")
        .and_then(|t| child_text(t, "displayformat"))
        .is_some_and(|s| s.eq_ignore_ascii_case("DF"));
    project.settings.drop_frame = df && fps.is_ntsc_drop_eligible();

    let mut fixes = ctx.warnings;
    fixes.extend(project.sanitize());
    crate::model::bump_ids(project.max_id());
    Ok((project, fixes))
}

/// Lê e importa um arquivo FCP7 XML com limite de tamanho (§0.4).
pub fn import_fcp7_file(path: &Path) -> Result<(Project, Vec<String>), NeovegError> {
    let text = read_bounded_utf8_file(path, MAX_FCP7_BYTES, "FCP7 XML")?;
    import_fcp7_str(&text)
}

/// Verdadeiro se o texto parece um documento `xmeml` (para `.xml` genérico).
pub fn looks_like_fcp7(text: &str) -> bool {
    text.get(..4096.min(text.len()))
        .or_else(|| text.get(..))
        .is_some_and(|head| head.contains("<xmeml"))
}

// ---------------------------------------------------------------------------
// Exportação
// ---------------------------------------------------------------------------

fn rate_xml(fps: FrameRate, indent: &str) -> String {
    let ntsc = fps.den == 1001;
    let tb = if ntsc {
        (u64::from(fps.num) + 500) / 1000
    } else {
        (u64::from(fps.num) + u64::from(fps.den.max(1)) / 2) / u64::from(fps.den.max(1))
    }
    .max(1);
    format!(
        "{indent}<rate>\n{indent}  <timebase>{tb}</timebase>\n{indent}  <ntsc>{}</ntsc>\n{indent}</rate>\n",
        if ntsc { "TRUE" } else { "FALSE" }
    )
}

/// Separa eventos sobrepostos em sub-trilhas: o FCP7 não admite clipes
/// sobrepostos numa mesma trilha.
fn split_lanes(events: &[&Event]) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..events.len()).collect();
    order.sort_by_key(|&i| events.get(i).map(|e| e.start).unwrap_or(Ticks::ZERO));
    let mut lanes: Vec<(Ticks, Vec<usize>)> = vec![];
    for i in order {
        let Some(e) = events.get(i) else {
            continue;
        };
        match lanes.iter_mut().find(|(end, _)| *end <= e.start) {
            Some((end, list)) => {
                *end = e.end();
                list.push(i);
            }
            None => lanes.push((e.end(), vec![i])),
        }
    }
    lanes.into_iter().map(|(_, l)| l).collect()
}

struct Exporter<'a> {
    project: &'a Project,
    fps: FrameRate,
    out: String,
    clip_n: usize,
    file_ids: HashMap<Id, String>,
}

impl Exporter<'_> {
    fn track(&mut self, events: &[&Event], kind: TrackKind, enabled: bool) {
        self.out.push_str("        <track>\n");
        let mut sorted: Vec<&&Event> = events.iter().collect();
        sorted.sort_by_key(|e| e.start);
        for e in sorted {
            self.clip(e, kind);
        }
        self.out.push_str(&format!(
            "          <enabled>{}</enabled>\n          <locked>FALSE</locked>\n        </track>\n",
            if enabled { "TRUE" } else { "FALSE" }
        ));
    }

    fn clip(&mut self, e: &Event, kind: TrackKind) {
        let Some(m) = self.project.media(e.media_id) else {
            return;
        };
        let path = match &m.source {
            MediaSource::File { path } => path.clone(),
            MediaSource::ImageSequence { paths, .. } => paths.first().cloned().unwrap_or_default(),
            MediaSource::Generated(_) => {
                self.out.push_str(&format!(
                    "          <!-- gerado pelo Neoveg, sem arquivo: {} -->\n",
                    escape_xml(&e.name).replace("--", "- -")
                ));
                return;
            }
        };
        let fps = self.fps;
        let media_rate = m.fps.unwrap_or(fps);
        let start = fps.ticks_to_frame_round(e.start);
        let end = fps.ticks_to_frame_round(e.end()).max(start + 1);
        let in_f = media_rate.ticks_to_frame_round(e.offset).max(0);
        let out_f = in_f + (end - start);
        let dur = m
            .duration
            .map(|d| media_rate.ticks_to_frame_round(d))
            .unwrap_or(out_f)
            .max(out_f);
        self.clip_n += 1;
        let o = &mut self.out;
        o.push_str(&format!(
            "          <clipitem id=\"clipitem-{}\">\n",
            self.clip_n
        ));
        o.push_str(&format!(
            "            <name>{}</name>\n            <enabled>{}</enabled>\n            <duration>{dur}</duration>\n",
            escape_xml(&e.name),
            if e.mute { "FALSE" } else { "TRUE" }
        ));
        o.push_str(&rate_xml(media_rate, "            "));
        o.push_str(&format!(
            "            <start>{start}</start>\n            <end>{end}</end>\n            <in>{in_f}</in>\n            <out>{out_f}</out>\n"
        ));
        match self.file_ids.get(&m.id) {
            Some(fid) => o.push_str(&format!("            <file id=\"{fid}\"/>\n")),
            None => {
                let fid = format!("file-{}", self.file_ids.len() + 1);
                o.push_str(&format!("            <file id=\"{fid}\">\n"));
                o.push_str(&format!(
                    "              <name>{}</name>\n              <pathurl>{}</pathurl>\n",
                    escape_xml(&m.name),
                    escape_xml(&encode_target_url(&path))
                ));
                o.push_str(&rate_xml(media_rate, "              "));
                o.push_str(&format!(
                    "              <duration>{dur}</duration>\n              <media>\n"
                ));
                if m.has_video {
                    o.push_str(&format!(
                        "                <video>\n                  <samplecharacteristics>\n                    <width>{}</width>\n                    <height>{}</height>\n                  </samplecharacteristics>\n                </video>\n",
                        m.width, m.height
                    ));
                }
                if m.has_audio {
                    o.push_str("                <audio/>\n");
                }
                o.push_str("              </media>\n            </file>\n");
                self.file_ids.insert(m.id, fid);
            }
        }
        if kind == TrackKind::Audio {
            o.push_str(
                "            <sourcetrack>\n              <mediatype>audio</mediatype>\n              <trackindex>1</trackindex>\n            </sourcetrack>\n",
            );
        }
        o.push_str("          </clipitem>\n");
    }
}

/// Exporta o projeto Neoveg como FCP7 XML (`xmeml` versão 5).
pub fn export_fcp7_string(project: &Project, name_override: Option<&str>) -> String {
    let fps = project.settings.fps;
    let name = name_override
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Neoveg".to_string());
    let dur = fps.ticks_to_frame_round(project.duration()).max(0);
    let mut ex = Exporter {
        project,
        fps,
        out: String::new(),
        clip_n: 0,
        file_ids: HashMap::new(),
    };
    ex.out.push_str(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE xmeml>\n<xmeml version=\"5\">\n",
    );
    ex.out.push_str(&format!(
        "  <sequence id=\"sequence-1\">\n    <name>{}</name>\n    <duration>{dur}</duration>\n",
        escape_xml(&name)
    ));
    ex.out.push_str(&rate_xml(fps, "    "));
    let df = project.settings.drop_frame && fps.is_ntsc_drop_eligible();
    ex.out.push_str("    <timecode>\n");
    ex.out.push_str(&rate_xml(fps, "      "));
    ex.out.push_str(&format!(
        "      <string>00:00:00{}00</string>\n      <frame>0</frame>\n      <displayformat>{}</displayformat>\n    </timecode>\n",
        if df { ";" } else { ":" },
        if df { "DF" } else { "NDF" }
    ));
    ex.out.push_str(
        "    <media>\n      <video>\n        <format>\n          <samplecharacteristics>\n",
    );
    ex.out.push_str(&rate_xml(fps, "            "));
    ex.out.push_str(&format!(
        "            <width>{}</width>\n            <height>{}</height>\n          </samplecharacteristics>\n        </format>\n",
        project.settings.width, project.settings.height
    ));
    // V1 (camada de baixo) primeiro: percorre as trilhas de vídeo de baixo para cima.
    for t in project
        .tracks
        .iter()
        .rev()
        .filter(|t| t.kind == TrackKind::Video && !t.is_adjustment())
    {
        let evs: Vec<&Event> = t.events.iter().collect();
        for lane in split_lanes(&evs) {
            let lane_evs: Vec<&Event> = lane.iter().filter_map(|&i| evs.get(i).copied()).collect();
            ex.track(&lane_evs, TrackKind::Video, !t.mute);
        }
    }
    ex.out.push_str("      </video>\n      <audio>\n");
    for t in project.tracks.iter().filter(|t| t.kind == TrackKind::Audio) {
        let evs: Vec<&Event> = t.events.iter().collect();
        for lane in split_lanes(&evs) {
            let lane_evs: Vec<&Event> = lane.iter().filter_map(|&i| evs.get(i).copied()).collect();
            ex.track(&lane_evs, TrackKind::Audio, !t.mute);
        }
    }
    ex.out.push_str("      </audio>\n    </media>\n");
    for m in &project.markers {
        let f = fps.ticks_to_frame_round(m.t).max(0);
        ex.out.push_str(&format!(
            "    <marker>\n      <name>{}</name>\n      <comment></comment>\n      <in>{f}</in>\n      <out>-1</out>\n    </marker>\n",
            escape_xml(&m.label)
        ));
    }
    ex.out.push_str("  </sequence>\n</xmeml>\n");
    ex.out
}

/// Grava o projeto como FCP7 XML.
pub fn export_fcp7_file(
    project: &Project,
    path: &Path,
    name_override: Option<&str>,
) -> Result<(), NeovegError> {
    let text = export_fcp7_string(project, name_override);
    std::fs::write(path, text)
        .map_err(|e| NeovegError::Io(format!("não foi possível gravar {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE xmeml>
<xmeml version="5">
  <sequence id="seq-1">
    <name>Corte</name>
    <rate><timebase>30</timebase><ntsc>TRUE</ntsc></rate>
    <timecode><displayformat>DF</displayformat></timecode>
    <media>
      <video>
        <format><samplecharacteristics><width>1280</width><height>720</height></samplecharacteristics></format>
        <track>
          <clipitem id="clipitem-1">
            <name>A</name>
            <rate><timebase>30</timebase><ntsc>TRUE</ntsc></rate>
            <start>0</start><end>-1</end><in>10</in><out>70</out>
            <file id="file-1">
              <name>a.mov</name>
              <pathurl>file://localhost/C:/midia/a%20b.mov</pathurl>
              <rate><timebase>30</timebase><ntsc>TRUE</ntsc></rate>
              <duration>300</duration>
              <media><video><samplecharacteristics><width>1280</width><height>720</height></samplecharacteristics></video><audio/></media>
            </file>
            <link><linkclipref>clipitem-1</linkclipref></link>
            <link><linkclipref>clipitem-3</linkclipref></link>
          </clipitem>
          <transitionitem>
            <start>50</start><end>70</end><alignment>center</alignment>
            <effect><name>Cross Dissolve</name></effect>
          </transitionitem>
          <clipitem id="clipitem-2">
            <name>B</name>
            <start>-1</start><end>120</end><in>0</in><out>70</out>
            <file id="file-1"/>
          </clipitem>
          <enabled>TRUE</enabled>
        </track>
      </video>
      <audio>
        <track>
          <clipitem id="clipitem-3">
            <name>A</name>
            <start>0</start><end>60</end><in>10</in><out>70</out>
            <file id="file-1"/>
            <link><linkclipref>clipitem-1</linkclipref></link>
          </clipitem>
        </track>
      </audio>
    </media>
    <marker><name>Início</name><in>15</in><out>-1</out></marker>
  </sequence>
</xmeml>"#;

    #[test]
    fn imports_sample_sequence() {
        let (p, _warn) = import_fcp7_str(SAMPLE).expect("importa");
        assert_eq!(p.settings.fps, FrameRate::FPS_29_97);
        assert!(p.settings.drop_frame);
        assert_eq!((p.settings.width, p.settings.height), (1280, 720));
        assert_eq!(p.media.len(), 1);
        let m = &p.media[0];
        assert!(matches!(&m.source, MediaSource::File { path } if path == "C:/midia/a b.mov"));
        assert!(m.has_video && m.has_audio);

        let v = &p.tracks[0];
        assert_eq!(v.kind, TrackKind::Video);
        assert_eq!(v.events.len(), 2);
        let fps = FrameRate::FPS_29_97;
        let (a, b) = (&v.events[0], &v.events[1]);
        // A vai até o fim da transição, B começa no início dela (sobreposição)
        assert_eq!(a.start, Ticks::ZERO);
        assert_eq!(a.end(), fps.frame_to_ticks(70));
        assert_eq!(a.offset, fps.frame_to_ticks(10));
        assert_eq!(b.start, fps.frame_to_ticks(50));
        assert_eq!(b.end(), fps.frame_to_ticks(120));
        assert_eq!(b.offset, Ticks::ZERO);
        assert!(b.transition.is_some());

        let au = p
            .tracks
            .iter()
            .find(|t| t.kind == TrackKind::Audio)
            .expect("áudio");
        assert_eq!(au.events.len(), 1);
        assert!(a.group.is_some());
        assert_eq!(a.group, au.events[0].group);

        assert_eq!(p.markers.len(), 1);
        assert_eq!(p.markers[0].t, fps.frame_to_ticks(15));
        assert_eq!(p.markers[0].label, "Início");
    }

    #[test]
    fn rejects_wrong_root_and_oversize() {
        assert!(import_fcp7_str("<fcpxml/>").is_err());
        assert!(import_fcp7_str("<xmeml version=\"5\"></xmeml>").is_err());
        let big = "a".repeat(MAX_FCP7_BYTES + 1);
        assert!(import_fcp7_str(&big).is_err());
    }

    #[test]
    fn hostile_numbers_are_ignored() {
        let doc = r#"<xmeml version="5"><sequence>
          <rate><timebase>0</timebase></rate>
          <media><video><track>
            <clipitem id="c"><name>X</name><start>99999999999999</start><end>-5</end>
              <in>-3</in><out>abc</out><file id="f"><name>x.mov</name></file></clipitem>
            <clipitem id="d"><name>Y</name><start>10</start><end>5</end><in>0</in><out>1</out>
              <file id="f"/></clipitem>
          </track></video></media></sequence></xmeml>"#;
        let (p, warn) = import_fcp7_str(doc).expect("não falha");
        assert_eq!(p.settings.fps, FrameRate::FPS_30);
        assert!(p.tracks.iter().all(|t| t.events.is_empty()));
        assert!(!warn.is_empty());
    }

    #[test]
    fn export_then_import_round_trips_timing() {
        let (p, _) = import_fcp7_str(SAMPLE).expect("importa");
        let xml = export_fcp7_string(&p, Some("Ida e volta"));
        assert!(xml.contains("<xmeml version=\"5\">"));
        assert!(xml.contains("<ntsc>TRUE</ntsc>"));
        let (q, _) = import_fcp7_str(&xml).expect("reimporta");
        let starts = |p: &Project| {
            let mut v: Vec<(Ticks, Ticks)> = p
                .tracks
                .iter()
                .filter(|t| t.kind == TrackKind::Video)
                .flat_map(|t| t.events.iter().map(|e| (e.start, e.end())))
                .collect();
            v.sort();
            v
        };
        assert_eq!(starts(&p), starts(&q));
        // a sobreposição A/B vira duas trilhas de vídeo na exportação
        assert_eq!(
            q.tracks
                .iter()
                .filter(|t| t.kind == TrackKind::Video)
                .count(),
            2
        );
        assert_eq!(q.media.len(), 1);
        assert_eq!(q.markers.len(), 1);
    }

    #[test]
    fn headless_export_command_writes_next_to_project() {
        use crate::commands::{run_command, HeadlessHost};
        use crate::session::EditorSession;
        let (p, _) = import_fcp7_str(SAMPLE).expect("importa");
        let dir = std::env::temp_dir().join(format!("neoveg_fcp7_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("cria pasta");
        let mut s = EditorSession::new(p);
        s.path = Some(dir.join("corte.neoveg"));
        run_command("file.export_fcp7", &mut s, &mut HeadlessHost).expect("exporta");
        let out = dir.join("corte.xml");
        let back = import_fcp7_file(&out);
        let _ = std::fs::remove_dir_all(&dir);
        let (q, _) = back.expect("reimporta");
        assert_eq!(q.media.len(), 1);
    }

    #[test]
    fn looks_like_fcp7_sniffs_root() {
        assert!(looks_like_fcp7(SAMPLE));
        assert!(!looks_like_fcp7("<fcpxml version=\"1.10\"/>"));
        assert!(!looks_like_fcp7(""));
    }

    /// Fuzzing por mutação (§0.8): truncamentos e inversões de bits não
    /// entram em pânico.
    #[test]
    fn fuzz_mutations_never_panic() {
        let base = SAMPLE.as_bytes();
        let mut seed: u64 = 0x5EED_F7C7;
        for i in 0..400 {
            let mut b = base.to_vec();
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let pos = (seed as usize) % b.len().max(1);
            match i % 3 {
                0 => b.truncate(pos),
                1 => {
                    if let Some(x) = b.get_mut(pos) {
                        *x ^= 1 << (seed % 8);
                    }
                }
                _ => b.splice(pos..pos, b"-1".iter().copied()).for_each(drop),
            }
            let text = String::from_utf8_lossy(&b);
            let r = std::panic::catch_unwind(|| {
                let _ = import_fcp7_str(&text);
            });
            assert!(r.is_ok(), "pânico na mutação {i}");
        }
    }
}
