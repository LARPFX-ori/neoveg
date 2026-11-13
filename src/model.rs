//! Modelo de dados do projeto (equivalente ao arquivo .veg do VEGAS Pro).
//!
//! Conceitos seguem a terminologia dos manuais:
//! projeto -> trilhas (track list) -> eventos (events) que referenciam mídias do
//! "Project Media", com fades, envelopes, pan/crop, track motion, efeitos
//! (event FX / track FX / master bus FX), transições, marcadores e regiões.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

pub type Id = u64;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn new_id() -> Id {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Garante que os próximos ids gerados sejam maiores que `max`.
pub fn bump_ids(max: Id) {
    NEXT_ID.fetch_max(max + 1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Curvas e interpolação
// ---------------------------------------------------------------------------

/// Tipos de curva de fade / segmento de envelope (Linear, Fast, Slow, Smooth,
/// Sharp e Hold, como no VEGAS).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Curve {
    #[default]
    Linear,
    Fast,
    Slow,
    Smooth,
    Sharp,
    Hold,
}

impl Curve {
    pub const ALL: [Curve; 6] = [
        Curve::Linear,
        Curve::Fast,
        Curve::Slow,
        Curve::Smooth,
        Curve::Sharp,
        Curve::Hold,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Curve::Linear => "Linear",
            Curve::Fast => "Rápida",
            Curve::Slow => "Lenta",
            Curve::Smooth => "Suave",
            Curve::Sharp => "Abrupta",
            Curve::Hold => "Manter",
        }
    }
    /// Mapeia x em [0,1] para y em [0,1] segundo a curva.
    pub fn shape(self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Curve::Linear => x,
            Curve::Fast => 1.0 - (1.0 - x) * (1.0 - x) * (1.0 - x),
            Curve::Slow => x * x * x,
            Curve::Smooth => x * x * (3.0 - 2.0 * x),
            Curve::Sharp => {
                // sobe rápido, estabiliza, e termina rápido (forma de "S" invertido)
                let s = x * x * (3.0 - 2.0 * x);
                2.0 * x - s
            }
            Curve::Hold => {
                if x >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
}

pub trait Lerp: Clone {
    fn lerp(&self, other: &Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        self + (other - self) * t
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Keyframe<T> {
    /// Tempo local (segundos) relativo ao início do evento (ou da timeline,
    /// para track motion).
    pub t: f64,
    pub value: T,
    #[serde(default)]
    pub curve: Curve,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Keyframes<T> {
    pub keys: Vec<Keyframe<T>>,
}

impl<T: Lerp + Default> Default for Keyframes<T> {
    fn default() -> Self {
        Keyframes {
            keys: vec![Keyframe {
                t: 0.0,
                value: T::default(),
                curve: Curve::Smooth,
            }],
        }
    }
}

impl<T: Lerp + Default> Keyframes<T> {
    pub fn eval(&self, t: f64) -> T {
        if self.keys.is_empty() {
            return T::default();
        }
        if t <= self.keys[0].t {
            return self.keys[0].value.clone();
        }
        for w in self.keys.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if t >= a.t && t < b.t {
                let span = (b.t - a.t).max(1e-9);
                let x = ((t - a.t) / span) as f32;
                return a.value.lerp(&b.value, a.curve.shape(x));
            }
        }
        self.keys
            .last()
            .map(|k| k.value.clone())
            .unwrap_or_default()
    }

    /// Insere ou substitui o keyframe no tempo `t` (tolerância de meio frame).
    pub fn set(&mut self, t: f64, value: T, tol: f64) {
        if let Some(k) = self.keys.iter_mut().find(|k| (k.t - t).abs() <= tol) {
            k.value = value;
            return;
        }
        let curve = self.keys.first().map(|k| k.curve).unwrap_or(Curve::Smooth);
        self.keys.push(Keyframe { t, value, curve });
        self.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
    }

    pub fn is_identity(&self) -> bool
    where
        T: PartialEq,
    {
        self.keys.iter().all(|k| k.value == T::default())
    }
}

// ---------------------------------------------------------------------------
// Projeto
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub sample_rate: u32,
    #[serde(default = "default_bg")]
    pub background: [f32; 3],
}

fn default_bg() -> [f32; 3] {
    [0.0, 0.0, 0.0]
}

impl Default for ProjectSettings {
    fn default() -> Self {
        ProjectSettings {
            width: 1920,
            height: 1080,
            fps: 29.97,
            sample_rate: 48000,
            background: default_bg(),
        }
    }
}

impl ProjectSettings {
    pub fn frame_dur(&self) -> f64 {
        1.0 / self.fps.max(1.0)
    }
    pub fn quantize(&self, t: f64) -> f64 {
        (t * self.fps).round() / self.fps
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Project {
    pub settings: ProjectSettings,
    pub media: Vec<MediaItem>,
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub markers: Vec<Marker>,
    #[serde(default)]
    pub regions: Vec<Region>,
    #[serde(default)]
    pub master: MasterBus,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub loop_region: Option<(f64, f64)>,
}

impl Project {
    pub fn new_default() -> Self {
        let mut p = Project::default();
        p.tracks.push(Track::new(TrackKind::Video, "Vídeo 1"));
        p.tracks.push(Track::new(TrackKind::Audio, "Áudio 1"));
        p
    }

    pub fn duration(&self) -> f64 {
        self.tracks
            .iter()
            .flat_map(|t| t.events.iter())
            .map(|e| e.end())
            .fold(0.0, f64::max)
    }

    pub fn media(&self, id: Id) -> Option<&MediaItem> {
        self.media.iter().find(|m| m.id == id)
    }

    pub fn find_event(&self, id: Id) -> Option<(usize, usize)> {
        for (ti, t) in self.tracks.iter().enumerate() {
            if let Some(ei) = t.events.iter().position(|e| e.id == id) {
                return Some((ti, ei));
            }
        }
        None
    }

    pub fn event(&self, id: Id) -> Option<&Event> {
        self.find_event(id).map(|(t, e)| &self.tracks[t].events[e])
    }

    pub fn event_mut(&mut self, id: Id) -> Option<&mut Event> {
        let (t, e) = self.find_event(id)?;
        Some(&mut self.tracks[t].events[e])
    }

    pub fn any_solo(&self, kind: TrackKind) -> bool {
        self.tracks.iter().any(|t| t.kind == kind && t.solo)
    }

    /// Maior id usado em qualquer objeto do projeto (para continuar a numeração
    /// após carregar um arquivo).
    pub fn max_id(&self) -> Id {
        let mut m = 0;
        for x in &self.media {
            m = m.max(x.id);
        }
        for t in &self.tracks {
            m = m.max(t.id);
            for f in &t.fx {
                m = m.max(f.id);
            }
            for e in &t.events {
                m = m.max(e.id).max(e.group.unwrap_or(0));
                for f in &e.fx {
                    m = m.max(f.id);
                }
            }
        }
        for f in self.master.fx.iter().chain(self.master.video_fx.iter()) {
            m = m.max(f.id);
        }
        for x in &self.markers {
            m = m.max(x.id);
        }
        for x in &self.regions {
            m = m.max(x.id);
        }
        m
    }

    /// Ordena os eventos das trilhas por tempo de início.
    pub fn sort_events(&mut self) {
        for t in &mut self.tracks {
            t.events.sort_by(|a, b| a.start.total_cmp(&b.start));
        }
    }

    /// Valida e corrige um projeto vindo de arquivo (entrada hostil, §0.4):
    /// números não finitos, faixas inválidas, ids duplicados, fades maiores
    /// que o evento, tamanhos que causariam alocações gigantes etc.
    /// Retorna a lista de correções aplicadas.
    pub fn sanitize(&mut self) -> Vec<String> {
        use crate::util::{clampd, clampf};
        let mut fixes: Vec<String> = vec![];
        // ids novos (renumeração) precisam ficar acima dos já existentes
        bump_ids(self.max_id());
        let fin32 = |v: f32, d: f32| if v.is_finite() { v } else { d };
        let fin64 = |v: f64, d: f64| if v.is_finite() { v } else { d };

        let s = &mut self.settings;
        let (w0, h0) = (s.width, s.height);
        s.width = (s.width.clamp(16, MAX_DIM) / 2) * 2;
        s.height = (s.height.clamp(16, MAX_DIM) / 2) * 2;
        if (w0, h0) != (s.width, s.height) {
            fixes.push(format!(
                "resolução {w0}x{h0} ajustada para {}x{}",
                s.width, s.height
            ));
        }
        if !s.fps.is_finite() || !(1.0..=240.0).contains(&s.fps) {
            fixes.push(format!(
                "taxa de quadros inválida ({}) trocada por 29,97",
                s.fps
            ));
            s.fps = 29.97;
        }
        if !(8000..=384_000).contains(&s.sample_rate) {
            fixes.push(format!(
                "taxa de amostragem inválida ({}) trocada por 48000",
                s.sample_rate
            ));
            s.sample_rate = 48000;
        }
        for c in &mut s.background {
            *c = clampf(fin32(*c, 0.0), 0.0, 1.0);
        }

        // mídia
        let mut seen_media = std::collections::HashSet::new();
        for m in &mut self.media {
            if !seen_media.insert(m.id) {
                let old = m.id;
                m.id = new_id();
                fixes.push(format!("id de mídia duplicado {old} renumerado"));
            }
            m.duration = m
                .duration
                .filter(|d| d.is_finite() && *d > 0.0)
                .map(|d| d.min(MAX_TIME));
            m.width = m.width.min(MAX_DIM * 4);
            m.height = m.height.min(MAX_DIM * 4);
            m.fps = clampd(fin64(m.fps, 0.0), 0.0, 1000.0);
            match &mut m.source {
                MediaSource::ImageSequence { fps, .. } => {
                    *fps = clampd(fin64(*fps, 30.0), 1.0, 240.0)
                }
                MediaSource::Generated(g) => sanitize_gen(g),
                MediaSource::File { .. } => {}
            }
        }

        // trilhas e eventos
        let mut seen_tracks = std::collections::HashSet::new();
        let mut seen_events = std::collections::HashSet::new();
        for t in &mut self.tracks {
            if !seen_tracks.insert(t.id) {
                t.id = new_id();
                fixes.push("id de trilha duplicado renumerado".into());
            }
            t.height = clampf(fin32(t.height, 72.0), 28.0, 400.0);
            t.volume_db = clampf(fin32(t.volume_db, 0.0), -120.0, 24.0);
            t.pan = clampf(fin32(t.pan, 0.0), -1.0, 1.0);
            t.opacity = clampf(fin32(t.opacity, 1.0), 0.0, 1.0);
            t.motion.keys.retain(|k| k.t.is_finite());
            for k in &mut t.motion.keys {
                k.t = clampd(k.t, 0.0, MAX_TIME);
                let v = &mut k.value;
                v.x = clampf(fin32(v.x, 0.0), -100.0, 100.0);
                v.y = clampf(fin32(v.y, 0.0), -100.0, 100.0);
                v.scale_x = clampf(fin32(v.scale_x, 1.0), 0.001, 100.0);
                v.scale_y = clampf(fin32(v.scale_y, 1.0), 0.001, 100.0);
                v.rot = fin32(v.rot, 0.0) % 3600.0;
            }
            t.motion.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
            if t.motion.keys.is_empty() {
                t.motion = Keyframes::default();
            }
            let mut kinds = std::collections::HashSet::new();
            t.envelopes.retain(|e| kinds.insert(e.kind));
            for env in &mut t.envelopes {
                let (lo, hi) = env.kind.range();
                env.points.retain(|p| p.t.is_finite() && p.v.is_finite());
                for p in &mut env.points {
                    p.t = clampd(p.t, 0.0, MAX_TIME);
                    p.v = clampf(p.v, lo, hi);
                }
                env.sort();
                if env.points.is_empty() {
                    *env = Envelope::new(env.kind);
                }
            }
            for f in &mut t.fx {
                sanitize_fx(f);
            }
            for e in &mut t.events {
                if !seen_events.insert(e.id) {
                    e.id = new_id();
                    fixes.push("id de evento duplicado renumerado".into());
                }
                e.start = clampd(fin64(e.start, 0.0), 0.0, MAX_TIME);
                let len = fin64(e.length, 1.0);
                if len <= 0.0 {
                    fixes.push(format!("evento \"{}\" com duração inválida", e.name));
                }
                e.length = clampd(len, 0.001, MAX_TIME);
                e.offset = clampd(fin64(e.offset, 0.0), 0.0, MAX_TIME);
                e.rate = clampd(fin64(e.rate, 1.0), 0.0, 100.0);
                e.gain_db = clampf(fin32(e.gain_db, 0.0), -120.0, 24.0);
                e.fade_in.len = clampd(fin64(e.fade_in.len, 0.0), 0.0, e.length);
                e.fade_out.len = clampd(fin64(e.fade_out.len, 0.0), 0.0, e.length - e.fade_in.len);
                e.pan_crop.keys.retain(|k| k.t.is_finite());
                for k in &mut e.pan_crop.keys {
                    k.t = clampd(k.t, -MAX_TIME, MAX_TIME);
                    let v = &mut k.value;
                    v.cx = clampf(fin32(v.cx, 0.5), -100.0, 100.0);
                    v.cy = clampf(fin32(v.cy, 0.5), -100.0, 100.0);
                    v.size = clampf(fin32(v.size, 1.0), 0.001, 100.0);
                    v.rot = fin32(v.rot, 0.0) % 3600.0;
                }
                e.pan_crop.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
                if e.pan_crop.keys.is_empty() {
                    e.pan_crop = Keyframes::default();
                }
                if let Some(tr) = &mut e.transition {
                    tr.softness = clampf(fin32(tr.softness, 0.0), 0.0, 0.5);
                }
                for f in &mut e.fx {
                    sanitize_fx(f);
                }
            }
        }
        for f in self
            .master
            .fx
            .iter_mut()
            .chain(self.master.video_fx.iter_mut())
        {
            sanitize_fx(f);
        }
        self.master.volume_db = clampf(fin32(self.master.volume_db, 0.0), -120.0, 24.0);
        self.markers.retain(|m| m.t.is_finite());
        for m in &mut self.markers {
            m.t = clampd(m.t, 0.0, MAX_TIME);
        }
        self.regions
            .retain(|r| r.start.is_finite() && r.end.is_finite());
        for r in &mut self.regions {
            if r.end < r.start {
                std::mem::swap(&mut r.start, &mut r.end);
            }
            r.start = clampd(r.start, 0.0, MAX_TIME);
            r.end = clampd(r.end, r.start, MAX_TIME);
        }
        self.loop_region = self
            .loop_region
            .filter(|(a, b)| a.is_finite() && b.is_finite() && b > a && *a >= 0.0);
        self.sort_events();
        fixes
    }
}

/// Maior dimensão de quadro aceita (evita alocações gigantes).
pub const MAX_DIM: u32 = 8192;
/// Maior tempo aceito na timeline (48 horas).
pub const MAX_TIME: f64 = 48.0 * 3600.0;

fn sanitize_fx(f: &mut FxInstance) {
    let specs = f.kind.params();
    f.params.truncate(specs.len());
    while f.params.len() < specs.len() {
        let d = specs.get(f.params.len()).map(|s| s.default).unwrap_or(0.0);
        f.params.push(AnimParam {
            value: d,
            keys: vec![],
        });
    }
    for (p, spec) in f.params.iter_mut().zip(specs.iter()) {
        if !p.value.is_finite() {
            p.value = spec.default;
        }
        p.keys.retain(|k| k.0.is_finite() && k.1.is_finite());
        p.keys.sort_by(|a, b| a.0.total_cmp(&b.0));
    }
}

fn sanitize_gen(g: &mut GenSpec) {
    use crate::util::clampf;
    let col = |c: &mut [f32; 4]| {
        for v in c.iter_mut() {
            *v = if v.is_finite() {
                clampf(*v, 0.0, 1.0)
            } else {
                1.0
            };
        }
    };
    match g {
        GenSpec::SolidColor { color } => col(color),
        GenSpec::Gradient { a, b, angle, .. } => {
            col(a);
            col(b);
            if !angle.is_finite() {
                *angle = 0.0;
            }
        }
        GenSpec::Checkerboard { a, b, cells } => {
            col(a);
            col(b);
            *cells = (*cells).clamp(1, 512);
        }
        GenSpec::ColorBars | GenSpec::Noise { .. } => {}
        GenSpec::Text(t) => {
            col(&mut t.color);
            col(&mut t.outline_color);
            col(&mut t.background);
            let f = |v: f32, d: f32, lo: f32, hi: f32| {
                if v.is_finite() {
                    clampf(v, lo, hi)
                } else {
                    d
                }
            };
            t.size = f(t.size, 10.0, 0.5, 100.0);
            t.outline = f(t.outline, 0.0, 0.0, 10.0);
            t.line_spacing = f(t.line_spacing, 1.2, 0.5, 5.0);
            t.anim_duration = f(t.anim_duration, 5.0, 0.1, 3600.0);
            t.x = f(t.x, 0.5, -10.0, 10.0);
            t.y = f(t.y, 0.5, -10.0, 10.0);
            if t.text.len() > 100_000 {
                let mut cut = 100_000;
                while !t.text.is_char_boundary(cut) {
                    cut -= 1;
                }
                t.text.truncate(cut);
            }
        }
    }
}

impl Project {
    /// Carrega um projeto de JSON validando-o.
    pub fn from_json(text: &str) -> Result<(Project, Vec<String>), String> {
        let mut p: Project =
            serde_json::from_str(text).map_err(|e| format!("projeto inválido: {e}"))?;
        let fixes = p.sanitize();
        bump_ids(p.max_id());
        Ok((p, fixes))
    }
}

// ---------------------------------------------------------------------------
// Mídia
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MediaItem {
    pub id: Id,
    pub name: String,
    pub source: MediaSource,
    /// Duração em segundos (None = ilimitada: imagens estáticas e mídia gerada).
    pub duration: Option<f64>,
    pub has_video: bool,
    pub has_audio: bool,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub fps: f64,
    #[serde(default)]
    pub info: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MediaSource {
    File { path: String },
    ImageSequence { paths: Vec<String>, fps: f64 },
    Generated(GenSpec),
}

/// Mídia gerada (Media Generators): cores sólidas, gradientes, padrões, texto.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum GenSpec {
    SolidColor {
        color: [f32; 4],
    },
    Gradient {
        a: [f32; 4],
        b: [f32; 4],
        angle: f32,
        radial: bool,
    },
    Checkerboard {
        a: [f32; 4],
        b: [f32; 4],
        cells: u32,
    },
    ColorBars,
    Noise {
        mono: bool,
        animated: bool,
    },
    Text(TextSpec),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum TextAnim {
    #[default]
    None,
    ScrollUp,
    CrawlLeft,
    Typewriter,
    FadeInOut,
}

impl TextAnim {
    pub const ALL: [TextAnim; 5] = [
        TextAnim::None,
        TextAnim::ScrollUp,
        TextAnim::CrawlLeft,
        TextAnim::Typewriter,
        TextAnim::FadeInOut,
    ];
    pub fn name(self) -> &'static str {
        match self {
            TextAnim::None => "Nenhuma",
            TextAnim::ScrollUp => "Rolagem (créditos)",
            TextAnim::CrawlLeft => "Letreiro (crawl)",
            TextAnim::Typewriter => "Máquina de escrever",
            TextAnim::FadeInOut => "Fade in/out",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum TextAlign {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TextSpec {
    pub text: String,
    /// Caminho de uma fonte TTF/OTF; vazio = fonte padrão embutida.
    pub font_path: String,
    /// Tamanho em % da altura do quadro.
    pub size: f32,
    pub color: [f32; 4],
    pub outline_color: [f32; 4],
    /// Espessura do contorno em % da altura.
    pub outline: f32,
    pub shadow: bool,
    pub background: [f32; 4],
    pub align: TextAlign,
    /// Posição do centro do bloco de texto (0..1).
    pub x: f32,
    pub y: f32,
    pub anim: TextAnim,
    /// Duração de referência para animações (segundos).
    pub anim_duration: f32,
    pub line_spacing: f32,
}

impl Default for TextSpec {
    fn default() -> Self {
        TextSpec {
            text: "Texto de exemplo".into(),
            font_path: String::new(),
            size: 10.0,
            color: [1.0, 1.0, 1.0, 1.0],
            outline_color: [0.0, 0.0, 0.0, 1.0],
            outline: 0.0,
            shadow: true,
            background: [0.0, 0.0, 0.0, 0.0],
            align: TextAlign::Center,
            x: 0.5,
            y: 0.5,
            anim: TextAnim::None,
            anim_duration: 5.0,
            line_spacing: 1.2,
        }
    }
}

impl GenSpec {
    pub fn label(&self) -> String {
        match self {
            GenSpec::SolidColor { .. } => "Cor sólida".into(),
            GenSpec::Gradient { radial, .. } => {
                if *radial {
                    "Gradiente radial".into()
                } else {
                    "Gradiente linear".into()
                }
            }
            GenSpec::Checkerboard { .. } => "Xadrez".into(),
            GenSpec::ColorBars => "Barras de cor".into(),
            GenSpec::Noise { .. } => "Ruído".into(),
            GenSpec::Text(t) => {
                let first = t
                    .text
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(24)
                    .collect::<String>();
                format!("Texto: {first}")
            }
        }
    }
    pub fn is_animated(&self) -> bool {
        match self {
            GenSpec::Noise { animated, .. } => *animated,
            GenSpec::Text(t) => t.anim != TextAnim::None,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Trilhas
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum TrackKind {
    Video,
    Audio,
}

/// Modos de composição de trilha de vídeo (Compositing Mode).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BlendMode {
    #[default]
    SourceAlpha,
    Add,
    Subtract,
    Multiply,
    Screen,
    Overlay,
    HardLight,
    Lighten,
    Darken,
    Difference,
}

impl BlendMode {
    pub const ALL: [BlendMode; 10] = [
        BlendMode::SourceAlpha,
        BlendMode::Add,
        BlendMode::Subtract,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::HardLight,
        BlendMode::Lighten,
        BlendMode::Darken,
        BlendMode::Difference,
    ];
    pub fn name(self) -> &'static str {
        match self {
            BlendMode::SourceAlpha => "Alfa da origem",
            BlendMode::Add => "Adicionar",
            BlendMode::Subtract => "Subtrair",
            BlendMode::Multiply => "Multiplicar (máscara)",
            BlendMode::Screen => "Tela (screen)",
            BlendMode::Overlay => "Sobrepor (overlay)",
            BlendMode::HardLight => "Luz forte",
            BlendMode::Lighten => "Clarear",
            BlendMode::Darken => "Escurecer",
            BlendMode::Difference => "Diferença",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum EnvKind {
    /// Volume da trilha de áudio (ganho linear 0..4, 1 = 0 dB).
    Volume,
    /// Pan da trilha de áudio (-1..1).
    Pan,
    /// Nível de composição (opacidade) da trilha de vídeo (0..1).
    Opacity,
    /// Fade para cor da trilha de vídeo (-1 = preto, 0 = nada, 1 = branco).
    FadeToColor,
}

impl EnvKind {
    pub fn name(self) -> &'static str {
        match self {
            EnvKind::Volume => "Volume",
            EnvKind::Pan => "Pan",
            EnvKind::Opacity => "Nível de composição",
            EnvKind::FadeToColor => "Fade para cor",
        }
    }
    pub fn range(self) -> (f32, f32) {
        match self {
            EnvKind::Volume => (0.0, 4.0),
            EnvKind::Pan => (-1.0, 1.0),
            EnvKind::Opacity => (0.0, 1.0),
            EnvKind::FadeToColor => (-1.0, 1.0),
        }
    }
    pub fn default_value(self) -> f32 {
        match self {
            EnvKind::Volume => 1.0,
            EnvKind::Opacity => 1.0,
            _ => 0.0,
        }
    }
    /// Converte valor -> posição vertical normalizada (0 = base, 1 = topo).
    pub fn to_norm(self, v: f32) -> f32 {
        match self {
            EnvKind::Volume => {
                // escala em dB: -60 dB..+12 dB
                let db = lin_to_db(v).clamp(-60.0, 12.0);
                if v <= 0.0 {
                    0.0
                } else {
                    (db + 60.0) / 72.0
                }
            }
            _ => {
                let (a, b) = self.range();
                (v - a) / (b - a)
            }
        }
    }
    pub fn value_from_norm(self, n: f32) -> f32 {
        let n = n.clamp(0.0, 1.0);
        match self {
            EnvKind::Volume => {
                if n <= 0.001 {
                    0.0
                } else {
                    db_to_lin(n * 72.0 - 60.0)
                }
            }
            _ => {
                let (a, b) = self.range();
                a + (b - a) * n
            }
        }
    }
    pub fn format(self, v: f32) -> String {
        match self {
            EnvKind::Volume => {
                if v <= 0.0 {
                    "-inf dB".into()
                } else {
                    format!("{:+.1} dB", lin_to_db(v))
                }
            }
            EnvKind::Pan => {
                if v.abs() < 0.005 {
                    "Centro".into()
                } else if v < 0.0 {
                    format!("{:.0}% E", -v * 100.0)
                } else {
                    format!("{:.0}% D", v * 100.0)
                }
            }
            EnvKind::Opacity => format!("{:.0}%", v * 100.0),
            EnvKind::FadeToColor => {
                if v < 0.0 {
                    format!("{:.0}% preto", -v * 100.0)
                } else {
                    format!("{:.0}% branco", v * 100.0)
                }
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnvPoint {
    /// Tempo absoluto na timeline (segundos).
    pub t: f64,
    pub v: f32,
    #[serde(default)]
    pub curve: Curve,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Envelope {
    pub kind: EnvKind,
    pub points: Vec<EnvPoint>,
    #[serde(default = "yes")]
    pub visible: bool,
}

fn yes() -> bool {
    true
}

impl Envelope {
    pub fn new(kind: EnvKind) -> Self {
        Envelope {
            kind,
            points: vec![EnvPoint {
                t: 0.0,
                v: kind.default_value(),
                curve: Curve::Linear,
            }],
            visible: true,
        }
    }
    pub fn eval(&self, t: f64) -> f32 {
        let p = &self.points;
        if p.is_empty() {
            return self.kind.default_value();
        }
        if t <= p[0].t {
            return p[0].v;
        }
        for w in p.windows(2) {
            if t >= w[0].t && t < w[1].t {
                let x = ((t - w[0].t) / (w[1].t - w[0].t).max(1e-9)) as f32;
                let y = w[0].curve.shape(x);
                return w[0].v + (w[1].v - w[0].v) * y;
            }
        }
        p.last().map(|x| x.v).unwrap_or(self.kind.default_value())
    }
    pub fn sort(&mut self) {
        self.points.sort_by(|a, b| a.t.total_cmp(&b.t));
    }
}

/// Track Motion (movimento 2D da trilha inteira).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MotionKey {
    /// Deslocamento do centro em fração da largura/altura do quadro.
    pub x: f32,
    pub y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    /// Rotação em graus.
    pub rot: f32,
}

impl Default for MotionKey {
    fn default() -> Self {
        MotionKey {
            x: 0.0,
            y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rot: 0.0,
        }
    }
}

impl Lerp for MotionKey {
    fn lerp(&self, o: &Self, t: f32) -> Self {
        MotionKey {
            x: self.x.lerp(&o.x, t),
            y: self.y.lerp(&o.y, t),
            scale_x: self.scale_x.lerp(&o.scale_x, t),
            scale_y: self.scale_y.lerp(&o.scale_y, t),
            rot: self.rot.lerp(&o.rot, t),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    pub id: Id,
    pub name: String,
    pub kind: TrackKind,
    pub events: Vec<Event>,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default = "default_track_height")]
    pub height: f32,
    /// Áudio: volume em dB.
    #[serde(default)]
    pub volume_db: f32,
    /// Áudio: pan -1..1.
    #[serde(default)]
    pub pan: f32,
    /// Vídeo: nível de composição 0..1.
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default)]
    pub blend: BlendMode,
    #[serde(default)]
    pub motion: Keyframes<MotionKey>,
    #[serde(default)]
    pub fx: Vec<FxInstance>,
    #[serde(default)]
    pub envelopes: Vec<Envelope>,
    #[serde(default)]
    pub color: [u8; 3],
}

fn default_track_height() -> f32 {
    72.0
}
fn one() -> f32 {
    1.0
}

const TRACK_COLORS: [[u8; 3]; 8] = [
    [86, 132, 214],
    [214, 120, 86],
    [96, 186, 120],
    [200, 170, 70],
    [170, 100, 200],
    [70, 180, 190],
    [210, 90, 140],
    [140, 150, 160],
];

impl Track {
    pub fn new(kind: TrackKind, name: &str) -> Self {
        let id = new_id();
        Track {
            id,
            name: name.into(),
            kind,
            events: vec![],
            mute: false,
            solo: false,
            locked: false,
            height: default_track_height(),
            volume_db: 0.0,
            pan: 0.0,
            opacity: 1.0,
            blend: BlendMode::SourceAlpha,
            motion: Keyframes::default(),
            fx: vec![],
            envelopes: vec![],
            color: TRACK_COLORS[(id as usize) % TRACK_COLORS.len()],
        }
    }

    pub fn envelope(&self, kind: EnvKind) -> Option<&Envelope> {
        self.envelopes.iter().find(|e| e.kind == kind)
    }

    pub fn env_value(&self, kind: EnvKind, t: f64) -> f32 {
        self.envelope(kind)
            .map(|e| e.eval(t))
            .unwrap_or(kind.default_value())
    }

    pub fn is_audible(&self, any_solo: bool) -> bool {
        !self.mute && (!any_solo || self.solo)
    }
}

// ---------------------------------------------------------------------------
// Eventos
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Fade {
    pub len: f64,
    #[serde(default)]
    pub curve: Curve,
}

/// Pan/Crop de evento de vídeo: retângulo de enquadramento sobre a imagem.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PanCropKey {
    /// Centro do enquadramento, em fração do quadro (0.5,0.5 = centro).
    pub cx: f32,
    pub cy: f32,
    /// Tamanho do enquadramento relativo ao quadro (1 = quadro inteiro,
    /// 0.5 = zoom 2x).
    pub size: f32,
    /// Rotação em graus.
    pub rot: f32,
}

impl Default for PanCropKey {
    fn default() -> Self {
        PanCropKey {
            cx: 0.5,
            cy: 0.5,
            size: 1.0,
            rot: 0.0,
        }
    }
}

impl Lerp for PanCropKey {
    fn lerp(&self, o: &Self, t: f32) -> Self {
        PanCropKey {
            cx: self.cx.lerp(&o.cx, t),
            cy: self.cy.lerp(&o.cy, t),
            size: self.size.lerp(&o.size, t),
            rot: self.rot.lerp(&o.rot, t),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TransitionKind {
    #[default]
    Dissolve,
    FadeThroughBlack,
    WipeLeft,
    WipeRight,
    WipeUp,
    WipeDown,
    PushLeft,
    PushRight,
    SlideOver,
    IrisOpen,
    IrisClose,
    ClockWipe,
    BarnDoor,
    Zoom,
    Blinds,
    Checker,
}

impl TransitionKind {
    pub const ALL: [TransitionKind; 16] = [
        TransitionKind::Dissolve,
        TransitionKind::FadeThroughBlack,
        TransitionKind::WipeLeft,
        TransitionKind::WipeRight,
        TransitionKind::WipeUp,
        TransitionKind::WipeDown,
        TransitionKind::PushLeft,
        TransitionKind::PushRight,
        TransitionKind::SlideOver,
        TransitionKind::IrisOpen,
        TransitionKind::IrisClose,
        TransitionKind::ClockWipe,
        TransitionKind::BarnDoor,
        TransitionKind::Zoom,
        TransitionKind::Blinds,
        TransitionKind::Checker,
    ];
    pub fn name(self) -> &'static str {
        match self {
            TransitionKind::Dissolve => "Dissolver (crossfade)",
            TransitionKind::FadeThroughBlack => "Fade pelo preto",
            TransitionKind::WipeLeft => "Wipe para a esquerda",
            TransitionKind::WipeRight => "Wipe para a direita",
            TransitionKind::WipeUp => "Wipe para cima",
            TransitionKind::WipeDown => "Wipe para baixo",
            TransitionKind::PushLeft => "Empurrar (push) esquerda",
            TransitionKind::PushRight => "Empurrar (push) direita",
            TransitionKind::SlideOver => "Deslizar por cima",
            TransitionKind::IrisOpen => "Íris abrindo",
            TransitionKind::IrisClose => "Íris fechando",
            TransitionKind::ClockWipe => "Relógio (clock wipe)",
            TransitionKind::BarnDoor => "Porta de celeiro",
            TransitionKind::Zoom => "Zoom",
            TransitionKind::Blinds => "Persianas",
            TransitionKind::Checker => "Tabuleiro",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransitionSpec {
    pub kind: TransitionKind,
    /// Suavidade da borda (0..0.5).
    #[serde(default)]
    pub softness: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub id: Id,
    pub name: String,
    pub media_id: Id,
    /// Início na timeline (s).
    pub start: f64,
    /// Duração na timeline (s).
    pub length: f64,
    /// Deslocamento dentro da mídia (take start offset), em segundos de mídia.
    #[serde(default)]
    pub offset: f64,
    /// Velocidade de reprodução (playback rate).
    #[serde(default = "one64")]
    pub rate: f64,
    #[serde(default)]
    pub fade_in: Fade,
    #[serde(default)]
    pub fade_out: Fade,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub looped: bool,
    #[serde(default)]
    pub reverse: bool,
    /// Ganho do evento de áudio (dB).
    #[serde(default)]
    pub gain_db: f32,
    /// Manter proporção da mídia (letterbox) em eventos de vídeo.
    #[serde(default = "yes")]
    pub maintain_aspect: bool,
    #[serde(default)]
    pub pan_crop: Keyframes<PanCropKey>,
    #[serde(default)]
    pub fx: Vec<FxInstance>,
    /// Grupo de eventos (ex.: áudio + vídeo do mesmo arquivo).
    #[serde(default)]
    pub group: Option<Id>,
    /// Transição usada quando este evento se sobrepõe ao anterior (crossfade).
    #[serde(default)]
    pub transition: Option<TransitionSpec>,
}

fn one64() -> f64 {
    1.0
}

impl Event {
    pub fn new(media: &MediaItem, start: f64, length: f64) -> Self {
        Event {
            id: new_id(),
            name: media.name.clone(),
            media_id: media.id,
            start,
            length,
            offset: 0.0,
            rate: 1.0,
            fade_in: Fade::default(),
            fade_out: Fade::default(),
            mute: false,
            locked: false,
            looped: media.duration.is_some(),
            reverse: false,
            gain_db: 0.0,
            maintain_aspect: true,
            pan_crop: Keyframes::default(),
            fx: vec![],
            group: None,
            transition: None,
        }
    }

    pub fn end(&self) -> f64 {
        self.start + self.length
    }

    pub fn contains(&self, t: f64) -> bool {
        t >= self.start && t < self.end()
    }

    /// Converte tempo da timeline em tempo da mídia (considerando offset,
    /// velocidade, loop e reversão). Retorna None se fora da mídia.
    pub fn media_time(&self, t: f64, media_dur: Option<f64>) -> Option<f64> {
        let local = (t - self.start) * self.rate;
        let mut mt = if self.reverse {
            let total = self.length * self.rate;
            self.offset + (total - local)
        } else {
            self.offset + local
        };
        if let Some(d) = media_dur {
            if d <= 0.0 {
                return None;
            }
            if mt >= d && mt < d + 1e-4 {
                mt = (d - 1e-6).max(0.0);
            }
            if self.looped {
                mt = mt.rem_euclid(d);
            } else if mt < 0.0 || mt >= d {
                return None;
            }
        }
        Some(mt.max(0.0))
    }

    /// Ganho dos fades de entrada/saída (0..1) no tempo local.
    pub fn fade_gain(&self, t: f64) -> f32 {
        let local = t - self.start;
        let mut g = 1.0f32;
        if self.fade_in.len > 0.0 && local < self.fade_in.len {
            g *= self.fade_in.curve.shape((local / self.fade_in.len) as f32);
        }
        let rem = self.end() - t;
        if self.fade_out.len > 0.0 && rem < self.fade_out.len {
            g *= self.fade_out.curve.shape((rem / self.fade_out.len) as f32);
        }
        g
    }
}

// ---------------------------------------------------------------------------
// Efeitos
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum FxKind {
    // vídeo
    BrightnessContrast,
    HslAdjust,
    ColorCorrector,
    Levels,
    BlackWhite,
    Sepia,
    Invert,
    GaussianBlur,
    Sharpen,
    Vignette,
    ChromaKeyer,
    Pixelate,
    Mirror,
    Glow,
    FilmGrain,
    Emboss,
    Posterize,
    Colorize,
    // áudio
    Gain,
    ParametricEq,
    HighPass,
    LowPass,
    Compressor,
    NoiseGate,
    Delay,
    Reverb,
    Chorus,
    Distortion,
}

#[derive(Clone, Copy, Debug)]
pub struct ParamSpec {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    /// true = parâmetro liga/desliga (0/1).
    pub toggle: bool,
}

const fn p(name: &'static str, min: f32, max: f32, default: f32) -> ParamSpec {
    ParamSpec {
        name,
        min,
        max,
        default,
        toggle: false,
    }
}
const fn tg(name: &'static str, default: f32) -> ParamSpec {
    ParamSpec {
        name,
        min: 0.0,
        max: 1.0,
        default,
        toggle: true,
    }
}

impl FxKind {
    pub const VIDEO: [FxKind; 18] = [
        FxKind::BrightnessContrast,
        FxKind::HslAdjust,
        FxKind::ColorCorrector,
        FxKind::Levels,
        FxKind::BlackWhite,
        FxKind::Sepia,
        FxKind::Invert,
        FxKind::GaussianBlur,
        FxKind::Sharpen,
        FxKind::Vignette,
        FxKind::ChromaKeyer,
        FxKind::Pixelate,
        FxKind::Mirror,
        FxKind::Glow,
        FxKind::FilmGrain,
        FxKind::Emboss,
        FxKind::Posterize,
        FxKind::Colorize,
    ];
    pub const AUDIO: [FxKind; 10] = [
        FxKind::Gain,
        FxKind::ParametricEq,
        FxKind::HighPass,
        FxKind::LowPass,
        FxKind::Compressor,
        FxKind::NoiseGate,
        FxKind::Delay,
        FxKind::Reverb,
        FxKind::Chorus,
        FxKind::Distortion,
    ];

    pub fn is_audio(self) -> bool {
        Self::AUDIO.contains(&self)
    }

    pub fn name(self) -> &'static str {
        match self {
            FxKind::BrightnessContrast => "Brilho e Contraste",
            FxKind::HslAdjust => "Ajuste HSL",
            FxKind::ColorCorrector => "Corretor de Cor",
            FxKind::Levels => "Níveis",
            FxKind::BlackWhite => "Preto e Branco",
            FxKind::Sepia => "Sépia",
            FxKind::Invert => "Inverter",
            FxKind::GaussianBlur => "Desfoque Gaussiano",
            FxKind::Sharpen => "Nitidez",
            FxKind::Vignette => "Vinheta",
            FxKind::ChromaKeyer => "Chroma Key",
            FxKind::Pixelate => "Pixelizar",
            FxKind::Mirror => "Espelhar",
            FxKind::Glow => "Brilho (Glow)",
            FxKind::FilmGrain => "Granulação de Filme",
            FxKind::Emboss => "Relevo",
            FxKind::Posterize => "Posterizar",
            FxKind::Colorize => "Colorir (Tint)",
            FxKind::Gain => "Ganho",
            FxKind::ParametricEq => "EQ Paramétrico",
            FxKind::HighPass => "Passa-Altas",
            FxKind::LowPass => "Passa-Baixas",
            FxKind::Compressor => "Compressor",
            FxKind::NoiseGate => "Noise Gate",
            FxKind::Delay => "Delay",
            FxKind::Reverb => "Reverb",
            FxKind::Chorus => "Chorus",
            FxKind::Distortion => "Distorção",
        }
    }

    pub fn params(self) -> &'static [ParamSpec] {
        match self {
            FxKind::BrightnessContrast => {
                const { &[p("Brilho", -1.0, 1.0, 0.0), p("Contraste", -1.0, 1.0, 0.0)] }
            }
            FxKind::HslAdjust => {
                const {
                    &[
                        p("Matiz (graus)", -180.0, 180.0, 0.0),
                        p("Saturação", 0.0, 3.0, 1.0),
                        p("Luminância", -1.0, 1.0, 0.0),
                    ]
                }
            }
            FxKind::ColorCorrector => {
                const {
                    &[
                        p("Lift (sombras)", -0.5, 0.5, 0.0),
                        p("Gamma", 0.2, 5.0, 1.0),
                        p("Gain (realces)", 0.0, 3.0, 1.0),
                        p("Temperatura", -1.0, 1.0, 0.0),
                        p("Tonalidade", -1.0, 1.0, 0.0),
                        p("Saturação", 0.0, 3.0, 1.0),
                    ]
                }
            }
            FxKind::Levels => {
                const {
                    &[
                        p("Entrada mín.", 0.0, 1.0, 0.0),
                        p("Entrada máx.", 0.0, 1.0, 1.0),
                        p("Gamma", 0.1, 10.0, 1.0),
                        p("Saída mín.", 0.0, 1.0, 0.0),
                        p("Saída máx.", 0.0, 1.0, 1.0),
                    ]
                }
            }
            FxKind::BlackWhite => const { &[p("Intensidade", 0.0, 1.0, 1.0)] },
            FxKind::Sepia => const { &[p("Intensidade", 0.0, 1.0, 1.0)] },
            FxKind::Invert => const { &[p("Intensidade", 0.0, 1.0, 1.0)] },
            FxKind::GaussianBlur => {
                const {
                    &[
                        p("Raio horizontal (%)", 0.0, 10.0, 1.0),
                        p("Raio vertical (%)", 0.0, 10.0, 1.0),
                    ]
                }
            }
            FxKind::Sharpen => const { &[p("Intensidade", 0.0, 3.0, 1.0)] },
            FxKind::Vignette => {
                const {
                    &[
                        p("Intensidade", 0.0, 1.0, 0.6),
                        p("Raio", 0.1, 1.5, 0.8),
                        p("Suavidade", 0.01, 1.0, 0.5),
                    ]
                }
            }
            FxKind::ChromaKeyer => {
                const {
                    &[
                        p("Cor R", 0.0, 1.0, 0.0),
                        p("Cor G", 0.0, 1.0, 1.0),
                        p("Cor B", 0.0, 1.0, 0.0),
                        p("Tolerância", 0.0, 1.0, 0.3),
                        p("Suavidade", 0.0, 1.0, 0.1),
                        p("Supressão de spill", 0.0, 1.0, 0.5),
                    ]
                }
            }
            FxKind::Pixelate => const { &[p("Tamanho do bloco (%)", 0.1, 10.0, 2.0)] },
            FxKind::Mirror => const { &[tg("Horizontal", 1.0), tg("Vertical", 0.0)] },
            FxKind::Glow => {
                const {
                    &[
                        p("Limiar", 0.0, 1.0, 0.7),
                        p("Intensidade", 0.0, 3.0, 1.0),
                        p("Raio (%)", 0.0, 10.0, 2.0),
                    ]
                }
            }
            FxKind::FilmGrain => {
                const { &[p("Quantidade", 0.0, 1.0, 0.15), tg("Monocromático", 1.0)] }
            }
            FxKind::Emboss => const { &[p("Intensidade", 0.0, 1.0, 1.0)] },
            FxKind::Posterize => const { &[p("Níveis", 2.0, 32.0, 6.0)] },
            FxKind::Colorize => {
                const {
                    &[
                        p("Cor R", 0.0, 1.0, 1.0),
                        p("Cor G", 0.0, 1.0, 0.6),
                        p("Cor B", 0.0, 1.0, 0.2),
                        p("Intensidade", 0.0, 1.0, 0.5),
                    ]
                }
            }
            FxKind::Gain => const { &[p("Ganho (dB)", -60.0, 24.0, 0.0)] },
            FxKind::ParametricEq => {
                const {
                    &[
                        p("Graves freq. (Hz)", 20.0, 1000.0, 100.0),
                        p("Graves ganho (dB)", -24.0, 24.0, 0.0),
                        p("Médios freq. (Hz)", 200.0, 8000.0, 1000.0),
                        p("Médios ganho (dB)", -24.0, 24.0, 0.0),
                        p("Médios Q", 0.1, 10.0, 1.0),
                        p("Agudos freq. (Hz)", 1000.0, 20000.0, 8000.0),
                        p("Agudos ganho (dB)", -24.0, 24.0, 0.0),
                    ]
                }
            }
            FxKind::HighPass => {
                const {
                    &[
                        p("Frequência (Hz)", 20.0, 5000.0, 80.0),
                        p("Q", 0.3, 5.0, 0.707),
                    ]
                }
            }
            FxKind::LowPass => {
                const {
                    &[
                        p("Frequência (Hz)", 200.0, 20000.0, 8000.0),
                        p("Q", 0.3, 5.0, 0.707),
                    ]
                }
            }
            FxKind::Compressor => {
                const {
                    &[
                        p("Limiar (dB)", -60.0, 0.0, -18.0),
                        p("Razão", 1.0, 20.0, 4.0),
                        p("Ataque (ms)", 0.1, 200.0, 10.0),
                        p("Liberação (ms)", 5.0, 2000.0, 150.0),
                        p("Ganho de saída (dB)", 0.0, 24.0, 0.0),
                    ]
                }
            }
            FxKind::NoiseGate => {
                const {
                    &[
                        p("Limiar (dB)", -90.0, 0.0, -50.0),
                        p("Ataque (ms)", 0.1, 100.0, 2.0),
                        p("Liberação (ms)", 5.0, 2000.0, 100.0),
                    ]
                }
            }
            FxKind::Delay => {
                const {
                    &[
                        p("Tempo (ms)", 1.0, 2000.0, 350.0),
                        p("Realimentação", 0.0, 0.95, 0.35),
                        p("Mix", 0.0, 1.0, 0.3),
                    ]
                }
            }
            FxKind::Reverb => {
                const {
                    &[
                        p("Tamanho da sala", 0.0, 1.0, 0.6),
                        p("Amortecimento", 0.0, 1.0, 0.5),
                        p("Largura", 0.0, 1.0, 1.0),
                        p("Mix", 0.0, 1.0, 0.25),
                    ]
                }
            }
            FxKind::Chorus => {
                const {
                    &[
                        p("Taxa (Hz)", 0.05, 5.0, 0.8),
                        p("Profundidade (ms)", 0.0, 20.0, 5.0),
                        p("Mix", 0.0, 1.0, 0.4),
                    ]
                }
            }
            FxKind::Distortion => const { &[p("Drive", 1.0, 50.0, 5.0), p("Mix", 0.0, 1.0, 1.0)] },
        }
    }
}

/// Parâmetro de efeito animável por keyframes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnimParam {
    pub value: f32,
    /// (tempo local, valor). Vazio = valor constante.
    #[serde(default)]
    pub keys: Vec<(f64, f32)>,
}

impl AnimParam {
    pub fn eval(&self, t: f64) -> f32 {
        if self.keys.is_empty() {
            return self.value;
        }
        let k = &self.keys;
        if t <= k[0].0 {
            return k[0].1;
        }
        for w in k.windows(2) {
            if t >= w[0].0 && t < w[1].0 {
                let x = ((t - w[0].0) / (w[1].0 - w[0].0).max(1e-9)) as f32;
                return w[0].1 + (w[1].1 - w[0].1) * x;
            }
        }
        k.last().map(|x| x.1).unwrap_or(self.value)
    }
    pub fn set_key(&mut self, t: f64, v: f32, tol: f64) {
        if let Some(k) = self.keys.iter_mut().find(|k| (k.0 - t).abs() <= tol) {
            k.1 = v;
        } else {
            self.keys.push((t, v));
            self.keys.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FxInstance {
    pub id: Id,
    pub kind: FxKind,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub params: Vec<AnimParam>,
}

impl FxInstance {
    pub fn new(kind: FxKind) -> Self {
        FxInstance {
            id: new_id(),
            kind,
            enabled: true,
            params: kind
                .params()
                .iter()
                .map(|s| AnimParam {
                    value: s.default,
                    keys: vec![],
                })
                .collect(),
        }
    }
    /// Valores de todos os parâmetros no tempo local `t`, sempre dentro da
    /// faixa declarada (valores vindos de arquivo são hostis: §0.4).
    pub fn values(&self, t: f64) -> Vec<f32> {
        self.kind
            .params()
            .iter()
            .enumerate()
            .map(|(i, spec)| {
                let v = self
                    .params
                    .get(i)
                    .map(|p| p.eval(t))
                    .unwrap_or(spec.default);
                if v.is_finite() {
                    crate::util::clampf(v, spec.min, spec.max)
                } else {
                    spec.default
                }
            })
            .collect()
    }
    /// Copia com novo id (para colar/duplicar).
    pub fn duplicate(&self) -> Self {
        let mut f = self.clone();
        f.id = new_id();
        f
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct MasterBus {
    #[serde(default)]
    pub volume_db: f32,
    #[serde(default)]
    pub fx: Vec<FxInstance>,
    /// Efeitos de saída de vídeo (Video Output FX).
    #[serde(default)]
    pub video_fx: Vec<FxInstance>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Marker {
    pub id: Id,
    pub t: f64,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Region {
    pub id: Id,
    pub start: f64,
    pub end: f64,
    pub label: String,
}

// ---------------------------------------------------------------------------
// Utilidades
// ---------------------------------------------------------------------------

pub fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub fn lin_to_db(v: f32) -> f32 {
    if v <= 1e-9 {
        -120.0
    } else {
        20.0 * v.log10()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TimeFormat {
    #[default]
    Timecode,
    Seconds,
    Frames,
}

/// Formata um tempo segundo o formato escolhido (Time Display).
pub fn format_time(t: f64, fps: f64, fmt: TimeFormat) -> String {
    let neg = t < 0.0;
    let t = t.abs();
    let s = match fmt {
        TimeFormat::Timecode => {
            let fr = fps.round().max(1.0) as u64;
            let total_frames = (t * fps).round() as u64;
            let f = total_frames % fr;
            let secs = total_frames / fr;
            format!(
                "{:02}:{:02}:{:02};{:02}",
                secs / 3600,
                (secs / 60) % 60,
                secs % 60,
                f
            )
        }
        TimeFormat::Seconds => {
            let ms = (t * 1000.0).round() as u64;
            format!(
                "{:02}:{:02}:{:02}.{:03}",
                ms / 3_600_000,
                (ms / 60_000) % 60,
                (ms / 1000) % 60,
                ms % 1000
            )
        }
        TimeFormat::Frames => format!("{}", (t * fps).round() as u64),
    };
    if neg {
        format!("-{s}")
    } else {
        s
    }
}

/// Interpreta um tempo digitado (HH:MM:SS;FF, HH:MM:SS.mmm, segundos ou
/// "Nf" para quadros).
pub fn parse_time(s: &str, fps: f64) -> Option<f64> {
    let s = s.trim();
    if let Some(f) = s.strip_suffix('f') {
        return f.trim().parse::<f64>().ok().map(|f| f / fps);
    }
    if !s.contains(':') {
        return s.replace(',', ".").parse::<f64>().ok();
    }
    let (main, frames) = match s.split_once(';') {
        Some((a, b)) => (a, b.parse::<f64>().ok()),
        None => (s, None),
    };
    let mut total = 0.0;
    for part in main.split(':') {
        total = total * 60.0 + part.replace(',', ".").parse::<f64>().ok()?;
    }
    if let Some(f) = frames {
        total += f / fps;
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_roundtrip() {
        let t = 3723.5;
        let s = format_time(t, 30.0, TimeFormat::Timecode);
        assert_eq!(s, "01:02:03;15");
        assert!((parse_time(&s, 30.0).unwrap() - t).abs() < 1e-6);
        assert!((parse_time("90f", 30.0).unwrap() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn envelope_eval() {
        let mut e = Envelope::new(EnvKind::Opacity);
        e.points.push(EnvPoint {
            t: 2.0,
            v: 0.0,
            curve: Curve::Linear,
        });
        assert!((e.eval(1.0) - 0.5).abs() < 1e-6);
        assert_eq!(e.eval(5.0), 0.0);
    }

    #[test]
    fn media_time_loop_reverse() {
        let m = MediaItem {
            id: 1,
            name: "x".into(),
            source: MediaSource::File { path: "x".into() },
            duration: Some(4.0),
            has_video: true,
            has_audio: false,
            width: 0,
            height: 0,
            fps: 30.0,
            info: String::new(),
        };
        let mut e = Event::new(&m, 10.0, 6.0);
        assert!((e.media_time(15.0, Some(4.0)).unwrap() - 1.0).abs() < 1e-9);
        e.looped = false;
        assert!(e.media_time(15.0, Some(4.0)).is_none());
        e.reverse = true;
        e.length = 4.0;
        assert!((e.media_time(11.0, Some(4.0)).unwrap() - 3.0).abs() < 1e-9);
    }
}
