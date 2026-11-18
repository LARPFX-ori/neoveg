//! Modelo de dados do projeto (equivalente ao arquivo .veg do VEGAS Pro).
//!
//! Conceitos seguem a terminologia dos manuais:
//! projeto -> trilhas (track list) -> eventos (events) que referenciam mídias do
//! "Project Media", com fades, envelopes, pan/crop, track motion, efeitos
//! (event FX / track FX / master bus FX), transições, marcadores e regiões.

pub use crate::curve::*;
pub use crate::time::*;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Projeto
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub width: u32,
    pub height: u32,
    pub fps: FrameRate,
    /// Usa contagem SMPTE Drop-Frame (`HH:MM:SS;FF`) quando a taxa é 29,97 ou 59,94.
    #[serde(default = "yes")]
    pub drop_frame: bool,
    pub sample_rate: u32,
    #[serde(default = "default_bg")]
    pub background: [f32; 3],
    /// Indica se o formato do projeto foi escolhido explicitamente pelo usuário (§3.1).
    #[serde(default)]
    pub manually_configured: bool,
    /// Layout de canais da mixagem (estéreo ou 5.1 surround).
    #[serde(default)]
    pub channel_layout: ChannelLayout,
    /// Modo de visualização/renderização 3D estereoscópica (manual p. 426).
    #[serde(default)]
    pub stereo_3d: Stereo3dMode,
    /// Tempo musical em batidas por minuto para régua e grade de compassos (p. 232).
    #[serde(default = "default_bpm")]
    pub bpm: f64,
    /// Formato de pixel do projeto (8-bit ou 32-bit ponto flutuante).
    #[serde(default)]
    pub pixel_format: crate::color::PixelFormat,
    /// Gama de composição (2,222 vídeo ou 1,000 linear).
    #[serde(default)]
    pub compositing_gamma: crate::color::CompositingGamma,
    /// Espaço de cor do projeto / ACES.
    #[serde(default)]
    pub color_space: crate::color::ProjectColorSpace,
    /// Transformação de visualização na prévia (View Transform).
    #[serde(default)]
    pub view_transform: crate::color::ViewTransform,
}

fn default_bpm() -> f64 {
    120.0
}

/// Modo 3D estereoscópico do projeto (*Project Properties → Video → Stereoscopic 3D mode*).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Stereo3dMode {
    #[default]
    Off,
    AnaglyphRedCyan,
    SideBySideHalf,
    SideBySideFull,
    TopBottomHalf,
    LeftOnly,
    RightOnly,
}

impl Stereo3dMode {
    pub const ALL: [Stereo3dMode; 7] = [
        Stereo3dMode::Off,
        Stereo3dMode::AnaglyphRedCyan,
        Stereo3dMode::SideBySideHalf,
        Stereo3dMode::SideBySideFull,
        Stereo3dMode::TopBottomHalf,
        Stereo3dMode::LeftOnly,
        Stereo3dMode::RightOnly,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Stereo3dMode::Off => "Desligado (2D)",
            Stereo3dMode::AnaglyphRedCyan => "Anáglifo (Vermelho / Ciano)",
            Stereo3dMode::SideBySideHalf => "Lado a lado (50% largura)",
            Stereo3dMode::SideBySideFull => "Lado a lado (largura cheia)",
            Stereo3dMode::TopBottomHalf => "Acima / Abaixo (50% altura)",
            Stereo3dMode::LeftOnly => "Somente olho esquerdo",
            Stereo3dMode::RightOnly => "Somente olho direito",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Stereo3dMode::Off => "2D",
            Stereo3dMode::AnaglyphRedCyan => "3D Anáglifo",
            Stereo3dMode::SideBySideHalf => "3D L/R 50%",
            Stereo3dMode::SideBySideFull => "3D L/R 100%",
            Stereo3dMode::TopBottomHalf => "3D Cima/Baixo",
            Stereo3dMode::LeftOnly => "3D Esq.",
            Stereo3dMode::RightOnly => "3D Dir.",
        }
    }
}

/// Disposição de origem dos dois olhos em um clipe/evento 3D estereoscópico.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Stereo3dSourceLayout {
    #[default]
    Mono2D,
    SideBySideHalf,
    SideBySideFull,
    TopBottomHalf,
}

impl Stereo3dSourceLayout {
    pub const ALL: [Stereo3dSourceLayout; 4] = [
        Stereo3dSourceLayout::Mono2D,
        Stereo3dSourceLayout::SideBySideHalf,
        Stereo3dSourceLayout::SideBySideFull,
        Stereo3dSourceLayout::TopBottomHalf,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Stereo3dSourceLayout::Mono2D => "2D padrão (sintetizar profundidade)",
            Stereo3dSourceLayout::SideBySideHalf => "Lado a lado (50%)",
            Stereo3dSourceLayout::SideBySideFull => "Lado a lado (completo)",
            Stereo3dSourceLayout::TopBottomHalf => "Acima / Abaixo (50%)",
        }
    }
}

/// Ajuste 3D estereoscópico por evento de vídeo (convergência horizontal e inversão L/R).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Stereo3dClip {
    pub layout: Stereo3dSourceLayout,
    pub swap_lr: bool,
    /// Deslocamento horizontal relativo à largura do quadro (`-0.25..=0.25`).
    pub horizontal_offset: f32,
}

impl Stereo3dClip {
    pub fn sanitized(self) -> Self {
        let off = if self.horizontal_offset.is_finite() {
            self.horizontal_offset.clamp(-0.25, 0.25)
        } else {
            0.0
        };
        Self {
            layout: self.layout,
            swap_lr: self.swap_lr,
            horizontal_offset: off,
        }
    }

    pub fn is_default(&self) -> bool {
        self.layout == Stereo3dSourceLayout::Mono2D
            && !self.swap_lr
            && self.horizontal_offset.abs() <= 1e-6
    }
}

fn is_false(v: &bool) -> bool {
    !*v
}

/// Layout de canais do projeto (*Project Properties → Audio → Master bus mode*).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ChannelLayout {
    #[default]
    Stereo,
    /// 5.1 na ordem FL, FR, C, LFE, BL, BR (ver `engine::surround`).
    Surround51,
}

impl ChannelLayout {
    pub const ALL: [ChannelLayout; 2] = [ChannelLayout::Stereo, ChannelLayout::Surround51];

    pub fn channels(self) -> usize {
        match self {
            ChannelLayout::Stereo => 2,
            ChannelLayout::Surround51 => 6,
        }
    }

    pub fn name_in(self, lang: neoveg_common::Language) -> &'static str {
        let pt = match self {
            ChannelLayout::Stereo => "Estéreo",
            ChannelLayout::Surround51 => "5.1 Surround",
        };
        neoveg_common::tr(lang, pt)
    }

    pub fn name(self) -> &'static str {
        self.name_in(neoveg_common::Language::PtBr)
    }
}

/// Posição no *Surround Panner* 5.1. `x`: -1 esquerda .. 1 direita;
/// `y`: -1 frente .. 1 trás. `lfe`: nível linear 0..1 enviado ao LFE.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurroundPan {
    #[serde(default)]
    pub x: f32,
    #[serde(default = "minus_one")]
    pub y: f32,
    /// Usa o alto-falante central; sem ele, a frente vira um par fantasma L/R.
    #[serde(default = "yes")]
    pub use_center: bool,
    #[serde(default)]
    pub lfe: f32,
}

impl Default for SurroundPan {
    fn default() -> Self {
        SurroundPan {
            x: 0.0,
            y: -1.0,
            use_center: true,
            lfe: 0.0,
        }
    }
}

impl SurroundPan {
    /// Valores finitos e dentro da faixa (entrada hostil, §0.4).
    pub fn sanitized(self) -> Self {
        let fin = |v: f32, d: f32| if v.is_finite() { v } else { d };
        SurroundPan {
            x: fin(self.x, 0.0).clamp(-1.0, 1.0),
            y: fin(self.y, -1.0).clamp(-1.0, 1.0),
            use_center: self.use_center,
            lfe: fin(self.lfe, 0.0).clamp(0.0, 1.0),
        }
    }
}

fn minus_one() -> f32 {
    -1.0
}

fn default_bg() -> [f32; 3] {
    [0.0, 0.0, 0.0]
}

impl Default for ProjectSettings {
    fn default() -> Self {
        ProjectSettings {
            width: 1920,
            height: 1080,
            fps: FrameRate::FPS_29_97,
            drop_frame: true,
            sample_rate: 48000,
            background: default_bg(),
            manually_configured: false,
            channel_layout: ChannelLayout::Stereo,
            stereo_3d: Stereo3dMode::Off,
            bpm: default_bpm(),
            pixel_format: crate::color::PixelFormat::Bits8,
            compositing_gamma: crate::color::CompositingGamma::Gamma2_222,
            color_space: crate::color::ProjectColorSpace::DefaultSrgb,
            view_transform: crate::color::ViewTransform::Off,
        }
    }
}

impl ProjectSettings {
    pub fn frame_dur(&self) -> Ticks {
        self.fps.ticks_per_frame()
    }
    pub fn quantize(&self, t: Ticks) -> Ticks {
        self.fps.quantize(t)
    }
    pub fn effective_time_format(&self, fmt: TimeFormat) -> TimeFormat {
        if fmt == TimeFormat::SmpteDrop && !self.drop_frame {
            TimeFormat::SmpteNonDrop
        } else {
            fmt
        }
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
    /// Barramentos auxiliares de áudio (até 26, endereçáveis de trilhas ou entre si
    /// de forma acíclica para o Master Bus).
    #[serde(default)]
    pub audio_buses: Vec<AudioBus>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub loop_region: Option<(Ticks, Ticks)>,
    /// Pastas manuais (*bins*) da janela Mídia do Projeto.
    #[serde(default)]
    pub media_bins: Vec<MediaBin>,
    /// Pastas inteligentes (*smart bins*) personalizadas do projeto.
    #[serde(default)]
    pub smart_bins: Vec<SmartBin>,
    /// Sequências de *storyboard* para montagem rápida na timeline.
    #[serde(default)]
    pub storyboards: Vec<Storyboard>,
    /// Sequências em abas na timeline (item 13 do backlog / VEGAS Pro).
    #[serde(default)]
    pub sequences: Vec<Sequence>,
    /// ID da sequência ativa no momento.
    #[serde(default)]
    pub active_sequence_id: Option<Id>,
}

/// Sequência em aba na timeline (item 13 do backlog / VEGAS Pro).
///
/// Cada sequência mantém sua própria coleção de trilhas, eventos, marcadores,
/// regiões e região de loop, permitindo múltiplos cortes e montagens no mesmo projeto.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sequence {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub markers: Vec<Marker>,
    #[serde(default)]
    pub regions: Vec<Region>,
    #[serde(default)]
    pub loop_region: Option<(Ticks, Ticks)>,
    #[serde(default)]
    pub cursor: Ticks,
}

impl Sequence {
    pub fn new(id: Id, name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            tracks: Vec::new(),
            markers: Vec::new(),
            regions: Vec::new(),
            loop_region: None,
            cursor: Ticks::ZERO,
        }
    }
}

impl Project {
    pub fn new_default() -> Self {
        let mut p = Project::default();
        p.tracks.push(Track::new(TrackKind::Video, "Vídeo 1"));
        p.tracks.push(Track::new(TrackKind::Audio, "Áudio 1"));
        p.ensure_sequences();
        p
    }

    /// Garante que o projeto tenha pelo menos uma sequência configurada e sincronizada.
    pub fn ensure_sequences(&mut self) {
        if self.sequences.is_empty() {
            let seq_id = new_id();
            let seq = Sequence {
                id: seq_id,
                name: "Sequência 1".to_string(),
                tracks: self.tracks.clone(),
                markers: self.markers.clone(),
                regions: self.regions.clone(),
                loop_region: self.loop_region,
                cursor: Ticks::ZERO,
            };
            self.sequences.push(seq);
            self.active_sequence_id = Some(seq_id);
        } else if self.active_sequence_id.is_none() {
            self.active_sequence_id = self.sequences.first().map(|s| s.id);
        }
    }

    /// Sincroniza o estado atual das trilhas/marcadores com a sequência ativa.
    pub fn sync_to_active_sequence(&mut self) {
        self.ensure_sequences();
        if let Some(active_id) = self.active_sequence_id {
            if let Some(seq) = self.sequences.iter_mut().find(|s| s.id == active_id) {
                seq.tracks = self.tracks.clone();
                seq.markers = self.markers.clone();
                seq.regions = self.regions.clone();
                seq.loop_region = self.loop_region;
            }
        }
    }

    /// Alterna para uma sequência diferente pelo ID, sincronizando antes a atual.
    pub fn switch_sequence(&mut self, target_id: Id) -> bool {
        self.sync_to_active_sequence();
        if let Some(target) = self.sequences.iter().find(|s| s.id == target_id).cloned() {
            self.tracks = target.tracks;
            self.markers = target.markers;
            self.regions = target.regions;
            self.loop_region = target.loop_region;
            self.active_sequence_id = Some(target_id);
            true
        } else {
            false
        }
    }

    /// Cria uma nova sequência no projeto e a torna ativa.
    pub fn create_sequence(&mut self, name: Option<&str>) -> Id {
        self.sync_to_active_sequence();
        let new_id = new_id();
        let seq_name = name
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("Sequência {}", self.sequences.len() + 1));
        let seq = Sequence {
            id: new_id,
            name: seq_name,
            tracks: vec![
                Track::new(TrackKind::Video, "Vídeo 1"),
                Track::new(TrackKind::Audio, "Áudio 1"),
            ],
            markers: Vec::new(),
            regions: Vec::new(),
            loop_region: None,
            cursor: Ticks::ZERO,
        };
        self.tracks = seq.tracks.clone();
        self.markers.clear();
        self.regions.clear();
        self.loop_region = None;
        self.sequences.push(seq);
        self.active_sequence_id = Some(new_id);
        new_id
    }

    /// Duplica uma sequência existente com todo o seu conteúdo.
    pub fn duplicate_sequence(&mut self, source_id: Id) -> Option<Id> {
        self.sync_to_active_sequence();
        let source = self.sequences.iter().find(|s| s.id == source_id)?.clone();
        let new_id = new_id();
        let dup = Sequence {
            id: new_id,
            name: format!("{} (Cópia)", source.name),
            tracks: source.tracks,
            markers: source.markers,
            regions: source.regions,
            loop_region: source.loop_region,
            cursor: source.cursor,
        };
        self.sequences.push(dup);
        Some(new_id)
    }

    /// Remove uma sequência se houver mais de uma.
    pub fn delete_sequence(&mut self, seq_id: Id) -> bool {
        self.ensure_sequences();
        if self.sequences.len() <= 1 {
            return false;
        }
        let was_active = self.active_sequence_id == Some(seq_id);
        self.sequences.retain(|s| s.id != seq_id);
        if was_active {
            if let Some(first) = self.sequences.first().cloned() {
                self.tracks = first.tracks;
                self.markers = first.markers;
                self.regions = first.regions;
                self.loop_region = first.loop_region;
                self.active_sequence_id = Some(first.id);
            }
        }
        true
    }

    /// Renomeia uma sequência.
    pub fn rename_sequence(&mut self, seq_id: Id, new_name: &str) -> bool {
        if let Some(seq) = self.sequences.iter_mut().find(|s| s.id == seq_id) {
            seq.name = new_name.to_string();
            true
        } else {
            false
        }
    }

    pub fn duration(&self) -> Ticks {
        self.tracks
            .iter()
            .flat_map(|t| t.events.iter())
            .map(|e| e.end())
            .fold(Ticks::ZERO, Ticks::max)
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

    pub fn any_bus_solo(&self) -> bool {
        self.audio_buses.iter().any(|b| b.solo)
    }

    pub fn audio_bus(&self, id: Id) -> Option<&AudioBus> {
        self.audio_buses.iter().find(|b| b.id == id)
    }

    pub fn audio_bus_mut(&mut self, id: Id) -> Option<&mut AudioBus> {
        self.audio_buses.iter_mut().find(|b| b.id == id)
    }

    pub fn next_audio_bus_name(&self) -> String {
        for idx in 0..MAX_AUDIO_BUSES {
            let letter = (b'A' + idx as u8) as char;
            let candidate = format!("Bus {letter}");
            if !self.audio_buses.iter().any(|b| b.name == candidate) {
                return candidate;
            }
        }
        format!("Bus {}", self.audio_buses.len() + 1)
    }

    pub fn add_audio_bus(&mut self, name: Option<&str>) -> Option<Id> {
        if self.audio_buses.len() >= MAX_AUDIO_BUSES {
            return None;
        }
        let label = name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| self.next_audio_bus_name());
        let bus = AudioBus::new(&label);
        let id = bus.id;
        self.audio_buses.push(bus);
        Some(id)
    }

    /// Garante que atribuir `target` como saída do barramento `bus_id` não cria ciclo
    /// nem ultrapassa o limite máximo de profundidade (`AGENTS.md §0.5`).
    pub fn validate_bus_route(&self, bus_id: Id, target: Option<Id>) -> bool {
        let Some(mut cur) = target else {
            return true;
        };
        if cur == bus_id {
            return false;
        }
        let mut steps = 0usize;
        while steps < MAX_AUDIO_BUSES {
            let Some(next_bus) = self.audio_bus(cur) else {
                return false;
            };
            let Some(next) = next_bus.output_bus else {
                return true;
            };
            if next == bus_id {
                return false;
            }
            cur = next;
            steps += 1;
        }
        false
    }

    /// Ordem topológica dos índices de `audio_buses` (emissores antes dos destinos),
    /// protegida contra ciclos para uso seguro na mixagem em passada única.
    pub fn topological_bus_order(&self) -> Vec<usize> {
        let n = self.audio_buses.len().min(MAX_AUDIO_BUSES);
        let mut emitted = vec![false; n];
        let mut order = Vec::with_capacity(n);
        for _pass in 0..n {
            let mut progressed = false;
            for idx in 0..n {
                if emitted[idx] {
                    continue;
                }
                let bid = self.audio_buses[idx].id;
                let blocked = self.audio_buses[..n]
                    .iter()
                    .enumerate()
                    .any(|(src_idx, src)| {
                        src_idx != idx && !emitted[src_idx] && src.output_bus == Some(bid)
                    });
                if !blocked {
                    emitted[idx] = true;
                    order.push(idx);
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
        }
        for (idx, done) in emitted.into_iter().enumerate() {
            if !done {
                order.push(idx);
            }
        }
        order
    }

    /// Índice da trilha por ID.
    pub fn track_index(&self, track_id: Id) -> Option<usize> {
        self.tracks.iter().position(|t| t.id == track_id)
    }

    /// Profundidade hierárquica da trilha (0 = raiz), protegida contra ciclos (§0.5).
    pub fn track_depth(&self, track_id: Id) -> usize {
        let mut depth = 0usize;
        let mut cur = track_id;
        let mut visited = std::collections::HashSet::new();
        visited.insert(cur);
        while depth < MAX_TRACK_DEPTH {
            let Some(tr) = self.tracks.iter().find(|t| t.id == cur) else {
                break;
            };
            let Some(pid) = tr.parent_track_id else {
                break;
            };
            if !visited.insert(pid) || !self.tracks.iter().any(|t| t.id == pid) {
                break;
            }
            depth += 1;
            cur = pid;
        }
        depth
    }

    /// Valida atribuição acíclica de `parent_id` para `child_id` dentro de `MAX_TRACK_DEPTH` (§0.5).
    pub fn validate_track_parent(&self, child_id: Id, parent_id: Option<Id>) -> Result<(), String> {
        let Some(pid) = parent_id else {
            return Ok(());
        };
        if pid == child_id {
            return Err("uma trilha não pode ser pai de si mesma".into());
        }
        let Some(child) = self.tracks.iter().find(|t| t.id == child_id) else {
            return Err("trilha filha inexistente".into());
        };
        let Some(parent) = self.tracks.iter().find(|t| t.id == pid) else {
            return Err("trilha pai inexistente".into());
        };
        if !parent.is_folder && parent.kind != child.kind {
            return Err("composição pai/filho requer trilhas do mesmo tipo (ou uma pasta)".into());
        }
        let mut cur = pid;
        let mut depth = 1usize;
        let mut visited = std::collections::HashSet::new();
        visited.insert(child_id);
        while depth <= MAX_TRACK_DEPTH {
            if !visited.insert(cur) {
                return Err("vínculo pai/filho criaria ciclo".into());
            }
            let Some(tr) = self.tracks.iter().find(|t| t.id == cur) else {
                return Err("ancestral inexistente".into());
            };
            let Some(next) = tr.parent_track_id else {
                return Ok(());
            };
            cur = next;
            depth += 1;
        }
        Err(format!(
            "hierarquia de trilhas excede o limite de {MAX_TRACK_DEPTH} níveis"
        ))
    }

    /// Verdadeiro se alguma pasta ancestral da trilha está recolhida.
    pub fn is_track_hidden_by_folder(&self, track_id: Id) -> bool {
        let mut cur = self
            .tracks
            .iter()
            .find(|t| t.id == track_id)
            .and_then(|t| t.parent_track_id);
        let mut steps = 0usize;
        while let Some(pid) = cur {
            if steps >= MAX_TRACK_DEPTH || pid == track_id {
                break;
            }
            let Some(anc) = self.tracks.iter().find(|t| t.id == pid) else {
                break;
            };
            if anc.is_folder && anc.collapsed {
                return true;
            }
            cur = anc.parent_track_id;
            steps += 1;
        }
        false
    }

    /// Verdadeiro se a trilha ou alguma pasta/pai ancestral estiver em mudo.
    pub fn is_track_effectively_muted(&self, track_id: Id) -> bool {
        let mut cur = Some(track_id);
        let mut steps = 0usize;
        let mut visited = std::collections::HashSet::new();
        while let Some(tid) = cur {
            if steps > MAX_TRACK_DEPTH || !visited.insert(tid) {
                break;
            }
            let Some(tr) = self.tracks.iter().find(|t| t.id == tid) else {
                break;
            };
            if tr.mute {
                return true;
            }
            cur = tr.parent_track_id;
            steps += 1;
        }
        false
    }

    /// Verdadeiro se houver pelo menos uma trilha filha apontando para `track_id`.
    pub fn has_track_children(&self, track_id: Id) -> bool {
        self.tracks
            .iter()
            .any(|t| t.parent_track_id == Some(track_id))
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
        for b in &self.audio_buses {
            m = m.max(b.id);
            for f in &b.fx {
                m = m.max(f.id);
            }
        }
        for x in &self.markers {
            m = m.max(x.id);
        }
        for x in &self.regions {
            m = m.max(x.id);
        }
        for b in &self.media_bins {
            m = m.max(b.id);
        }
        for sb in &self.smart_bins {
            m = m.max(sb.id);
        }
        for st in &self.storyboards {
            m = m.max(st.id);
        }
        m
    }

    /// Garante que atribuir `parent` a uma pasta (`bin_id`) é acíclico e respeita [`MAX_BIN_DEPTH`].
    pub fn validate_bin_parent(&self, bin_id: Id, parent: Option<Id>) -> bool {
        let Some(mut cur) = parent else {
            return true;
        };
        if cur == bin_id {
            return false;
        }
        let mut visited = std::collections::HashSet::new();
        visited.insert(bin_id);
        for _ in 0..MAX_BIN_DEPTH {
            if !visited.insert(cur) {
                return false;
            }
            let Some(bin) = self.media_bins.iter().find(|b| b.id == cur) else {
                return false;
            };
            match bin.parent_id {
                Some(next) => cur = next,
                None => return true,
            }
        }
        false
    }

    /// IDs das mídias efetivamente usadas na timeline (take ativo ou alternativo).
    pub fn used_media_ids(&self) -> std::collections::HashSet<Id> {
        self.tracks
            .iter()
            .flat_map(|t| t.events.iter().flat_map(crate::takes::media_ids))
            .collect()
    }

    /// Quantos eventos da linha do tempo referenciam a mídia `media_id`.
    pub fn media_usage_count(&self, media_id: Id) -> usize {
        self.tracks
            .iter()
            .flat_map(|t| t.events.iter())
            .filter(|e| crate::takes::uses_media(e, media_id))
            .count()
    }

    /// Coleta IDs de mídia pertencentes à pasta `bin_id` (opcionalmente recursivo em subpastas).
    pub fn collect_bin_media(&self, bin_id: Id, recursive: bool) -> std::collections::HashSet<Id> {
        let mut out = std::collections::HashSet::new();
        let mut stack = vec![(bin_id, 0usize)];
        let mut seen_bins = std::collections::HashSet::new();
        while let Some((cur, depth)) = stack.pop() {
            if depth >= MAX_BIN_DEPTH || !seen_bins.insert(cur) {
                continue;
            }
            if let Some(bin) = self.media_bins.iter().find(|b| b.id == cur) {
                out.extend(bin.media_ids.iter().copied());
                if recursive {
                    for child in self.media_bins.iter().filter(|b| b.parent_id == Some(cur)) {
                        stack.push((child.id, depth + 1));
                    }
                }
            }
        }
        out
    }

    /// Ordena os eventos das trilhas por tempo de início.
    pub fn sort_events(&mut self) {
        for t in &mut self.tracks {
            t.events.sort_by_key(|e| e.start);
        }
    }

    /// Valida e corrige um projeto vindo de arquivo (entrada hostil, §0.4):
    /// números não finitos, faixas inválidas, ids duplicados, fades maiores
    /// que o evento, tamanhos que causariam alocações gigantes etc.
    /// Retorna a lista de correções aplicadas.
    pub fn sanitize(&mut self) -> Vec<String> {
        use crate::util::clampf;
        let mut fixes: Vec<String> = vec![];
        // ids novos (renumeração) precisam ficar acima dos já existentes
        bump_ids(self.max_id());
        let fin32 = |v: f32, d: f32| if v.is_finite() { v } else { d };

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
        let fps_f = s.fps.as_f64();
        if !(1.0..=240.0).contains(&fps_f) {
            fixes.push(format!(
                "taxa de quadros inválida ({:.3}) trocada por 29,97",
                fps_f
            ));
            s.fps = FrameRate::FPS_29_97;
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
        if !s.bpm.is_finite() || s.bpm < 20.0 || s.bpm > 300.0 {
            s.bpm = default_bpm();
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
                .filter(|d| *d > Ticks::ZERO)
                .map(|d| d.min(Ticks::MAX));
            m.width = m.width.min(MAX_DIM * 4);
            m.height = m.height.min(MAX_DIM * 4);
            match &mut m.source {
                MediaSource::ImageSequence { fps, .. } => {
                    if !(1.0..=240.0).contains(&fps.as_f64()) {
                        *fps = FrameRate::FPS_30;
                    }
                }
                MediaSource::Generated(g) => sanitize_gen(g),
                MediaSource::File { .. } | MediaSource::NestedProject { .. } => {}
            }
        }

        // trilhas e eventos
        let mut seen_tracks = std::collections::HashSet::new();
        let mut seen_events = std::collections::HashSet::new();
        // ids de mídia do projeto: takes alternativos sem mídia são descartados
        let media_ids: std::collections::HashSet<Id> = self.media.iter().map(|m| m.id).collect();
        for t in &mut self.tracks {
            if !seen_tracks.insert(t.id) {
                t.id = new_id();
                fixes.push("id de trilha duplicado renumerado".into());
            }
            t.height = clampf(fin32(t.height, 72.0), 28.0, 400.0);
            t.volume_db = clampf(fin32(t.volume_db, 0.0), -120.0, 24.0);
            t.pan = clampf(fin32(t.pan, 0.0), -1.0, 1.0);
            t.surround = t.surround.sanitized();
            t.opacity = clampf(fin32(t.opacity, 1.0), 0.0, 1.0);
            let sanitize_motion_list = |mk: &mut Keyframes<MotionKey>| {
                for k in &mut mk.keys {
                    k.t = k.t.clamp_safe(Ticks::ZERO, Ticks::MAX);
                    let v = &mut k.value;
                    v.x = clampf(fin32(v.x, 0.0), -100.0, 100.0);
                    v.y = clampf(fin32(v.y, 0.0), -100.0, 100.0);
                    v.scale_x = clampf(fin32(v.scale_x, 1.0), 0.001, 100.0);
                    v.scale_y = clampf(fin32(v.scale_y, 1.0), 0.001, 100.0);
                    v.rot = fin32(v.rot, 0.0) % 3600.0;
                }
                mk.keys.sort_by_key(|k| k.t);
                if mk.keys.is_empty() {
                    *mk = Keyframes::default();
                }
            };
            sanitize_motion_list(&mut t.motion);
            sanitize_motion_list(&mut t.parent_motion);
            sanitize_envelopes(
                &mut t.envelopes,
                &[
                    EnvKind::Volume,
                    EnvKind::Pan,
                    EnvKind::Opacity,
                    EnvKind::FadeToColor,
                ],
            );
            for f in &mut t.fx {
                sanitize_fx(f);
            }
            for e in &mut t.events {
                if !seen_events.insert(e.id) {
                    e.id = new_id();
                    fixes.push("id de evento duplicado renumerado".into());
                }
                e.start = e.start.clamp_safe(Ticks::ZERO, Ticks::MAX);
                if e.length <= Ticks::ZERO {
                    fixes.push(format!("evento \"{}\" com duração inválida", e.name));
                }
                e.length = e.length.clamp_safe(Ticks::from_millis_i64(1), Ticks::MAX);
                e.offset = e.offset.clamp_safe(Ticks::ZERO, Ticks::MAX);
                fixes.extend(crate::takes::sanitize_takes(e, &media_ids));
                if e.rate.as_f64() > 100.0 {
                    e.rate = Rate::from_ratio(100, 1).unwrap_or(Rate::NORMAL);
                }
                e.gain_db = clampf(fin32(e.gain_db, 0.0), -120.0, 24.0);
                e.pitch_semitones = clampf(fin32(e.pitch_semitones, 0.0), -24.0, 24.0);
                e.fade_in.len = e.fade_in.len.clamp_safe(Ticks::ZERO, e.length);
                e.fade_out.len = e
                    .fade_out
                    .len
                    .clamp_safe(Ticks::ZERO, e.length - e.fade_in.len);
                for k in &mut e.pan_crop.keys {
                    k.t = k.t.clamp_safe(Ticks::MIN_NEGATIVE, Ticks::MAX);
                    let v = &mut k.value;
                    v.cx = clampf(fin32(v.cx, 0.5), -100.0, 100.0);
                    v.cy = clampf(fin32(v.cy, 0.5), -100.0, 100.0);
                    v.size = clampf(fin32(v.size, 1.0), 0.001, 100.0);
                    v.rot = fin32(v.rot, 0.0) % 3600.0;
                }
                e.pan_crop.keys.sort_by_key(|k| k.t);
                if e.pan_crop.keys.is_empty() {
                    e.pan_crop = Keyframes::default();
                }
                if let Some(tr) = &mut e.transition {
                    tr.softness = clampf(fin32(tr.softness, 0.0), 0.0, 0.5);
                }
                for f in &mut e.fx {
                    sanitize_fx(f);
                }
                e.stereo_3d = e.stereo_3d.sanitized();
            }
        }
        // Saneamento da hierarquia pai/filho de trilhas (§0.5: referências inexistentes ou ciclos).
        let track_ids: Vec<Id> = self.tracks.iter().map(|t| t.id).collect();
        for ti in 0..track_ids.len() {
            let tid = track_ids[ti];
            if self.tracks[ti].parent_track_id.is_some()
                && self
                    .validate_track_parent(tid, self.tracks[ti].parent_track_id)
                    .is_err()
            {
                let name = self.tracks[ti].name.clone();
                self.tracks[ti].parent_track_id = None;
                fixes.push(format!(
                    "vínculo pai/filho inválido ou cíclico removido da trilha \"{name}\""
                ));
            }
        }
        // Saneamento de Sync Links (§0.4 e §0.5: referências inexistentes ou ciclos).
        let valid_sync: std::collections::HashMap<Id, Id> = self
            .tracks
            .iter()
            .flat_map(|t| t.events.iter())
            .filter_map(|e| e.sync_link_master.map(|m| (e.id, m)))
            .collect();
        for t in &mut self.tracks {
            for e in &mut t.events {
                if let Some(mid) = e.sync_link_master {
                    let mut cur = mid;
                    let mut visited = std::collections::HashSet::new();
                    visited.insert(e.id);
                    let mut ok = seen_events.contains(&mid);
                    let mut depth = 0usize;
                    while ok {
                        if !visited.insert(cur) || depth >= 32 {
                            ok = false;
                            break;
                        }
                        depth += 1;
                        let Some(&next) = valid_sync.get(&cur) else {
                            break;
                        };
                        cur = next;
                    }
                    if !ok {
                        e.sync_link_master = None;
                        fixes.push(format!(
                            "Sync Link inválido ou cíclico removido do evento \"{}\"",
                            e.name
                        ));
                    }
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
        if self.audio_buses.len() > MAX_AUDIO_BUSES {
            self.audio_buses.truncate(MAX_AUDIO_BUSES);
            fixes.push(format!(
                "barramentos de áudio limitados ao máximo de {MAX_AUDIO_BUSES}"
            ));
        }
        let mut seen_buses = std::collections::HashSet::new();
        for b in &mut self.audio_buses {
            if !seen_buses.insert(b.id) {
                let old = b.id;
                b.id = new_id();
                fixes.push(format!("id de barramento duplicado {old} renumerado"));
                seen_buses.insert(b.id);
            }
            b.volume_db = clampf(fin32(b.volume_db, 0.0), -120.0, 24.0);
            b.pan = clampf(fin32(b.pan, 0.0), -1.0, 1.0);
            b.surround = b.surround.sanitized();
            for f in &mut b.fx {
                sanitize_fx(f);
            }
            sanitize_envelopes(&mut b.envelopes, &[EnvKind::Volume, EnvKind::Pan]);
        }
        let bus_ids: std::collections::HashSet<Id> =
            self.audio_buses.iter().map(|b| b.id).collect();
        for bi in 0..self.audio_buses.len() {
            let bid = self.audio_buses[bi].id;
            let tgt = self.audio_buses[bi].output_bus;
            if let Some(tid) = tgt {
                if !bus_ids.contains(&tid) || !self.validate_bus_route(bid, Some(tid)) {
                    self.audio_buses[bi].output_bus = None;
                    fixes.push(format!(
                        "roteamento inválido/cíclico do barramento \"{}\" restaurado para Master",
                        self.audio_buses[bi].name
                    ));
                }
            }
        }
        for t in &mut self.tracks {
            if t.kind != TrackKind::Audio {
                t.output_bus = None;
                t.sends.clear();
                continue;
            }
            if let Some(bid) = t.output_bus {
                if !bus_ids.contains(&bid) {
                    t.output_bus = None;
                    fixes.push(format!(
                        "barramento de saída inexistente na trilha \"{}\" restaurado para Master",
                        t.name
                    ));
                }
            }
            t.sends.retain(|s| bus_ids.contains(&s.bus_id));
            if t.sends.len() > MAX_AUDIO_BUSES {
                t.sends.truncate(MAX_AUDIO_BUSES);
            }
            for s in &mut t.sends {
                s.level_db = clampf(fin32(s.level_db, -6.0), -120.0, 24.0);
            }
        }
        for m in &mut self.markers {
            m.t = m.t.clamp_safe(Ticks::ZERO, Ticks::MAX);
            m.label = m.label.chars().filter(|&c| c != '\0').take(256).collect();
            m.command_type = m
                .command_type
                .trim()
                .chars()
                .filter(|&c| c != '\0')
                .take(64)
                .collect();
            m.command_arg = m
                .command_arg
                .chars()
                .filter(|&c| c != '\0')
                .take(512)
                .collect();
        }
        for r in &mut self.regions {
            if r.end < r.start {
                std::mem::swap(&mut r.start, &mut r.end);
            }
            r.start = r.start.clamp_safe(Ticks::ZERO, Ticks::MAX);
            r.end = r.end.clamp_safe(r.start, Ticks::MAX);
        }
        self.loop_region = self
            .loop_region
            .filter(|(a, b)| b > a && *a >= Ticks::ZERO)
            .map(|(a, b)| (a.min(Ticks::MAX), b.min(Ticks::MAX)));

        // Pastas manuais (bins), pastas inteligentes e storyboards (§0.4 e §0.5)
        if self.media_bins.len() > MAX_MEDIA_BINS {
            self.media_bins.truncate(MAX_MEDIA_BINS);
            fixes.push(format!("pastas de mídia limitadas a {MAX_MEDIA_BINS}"));
        }
        let mut seen_bins = std::collections::HashSet::new();
        for bin in &mut self.media_bins {
            if !seen_bins.insert(bin.id) {
                bin.id = new_id();
                seen_bins.insert(bin.id);
                fixes.push("id de pasta de mídia duplicado renumerado".into());
            }
            if bin.name.trim().is_empty() {
                bin.name = "Pasta".into();
            }
            let mut seen_m = std::collections::HashSet::new();
            bin.media_ids
                .retain(|id| media_ids.contains(id) && seen_m.insert(*id));
        }
        let bin_ids: std::collections::HashSet<Id> = self.media_bins.iter().map(|b| b.id).collect();
        for bi in 0..self.media_bins.len() {
            let bid = self.media_bins[bi].id;
            if let Some(pid) = self.media_bins[bi].parent_id {
                if !bin_ids.contains(&pid) || !self.validate_bin_parent(bid, Some(pid)) {
                    self.media_bins[bi].parent_id = None;
                    fixes.push(format!(
                        "hierarquia inválida/cíclica da pasta \"{}\" restaurada para a raiz",
                        self.media_bins[bi].name
                    ));
                }
            }
        }

        if self.smart_bins.len() > MAX_MEDIA_BINS {
            self.smart_bins.truncate(MAX_MEDIA_BINS);
            fixes.push(format!("pastas inteligentes limitadas a {MAX_MEDIA_BINS}"));
        }
        let mut seen_smart = std::collections::HashSet::new();
        for sb in &mut self.smart_bins {
            if !seen_smart.insert(sb.id) {
                sb.id = new_id();
                seen_smart.insert(sb.id);
                fixes.push("id de pasta inteligente duplicado renumerado".into());
            }
            if sb.name.trim().is_empty() {
                sb.name = sb.rule.label().into();
            }
            if sb.query.len() > 128 {
                sb.query = sb.query.chars().take(128).collect();
            }
        }

        if self.storyboards.len() > MAX_STORYBOARDS {
            self.storyboards.truncate(MAX_STORYBOARDS);
            fixes.push(format!("storyboards limitados a {MAX_STORYBOARDS}"));
        }
        let mut seen_sb = std::collections::HashSet::new();
        for st in &mut self.storyboards {
            if !seen_sb.insert(st.id) {
                st.id = new_id();
                seen_sb.insert(st.id);
                fixes.push("id de storyboard duplicado renumerado".into());
            }
            if st.name.trim().is_empty() {
                st.name = "Storyboard".into();
            }
            if st.items.len() > MAX_STORYBOARD_ITEMS {
                st.items.truncate(MAX_STORYBOARD_ITEMS);
                fixes.push(format!(
                    "itens do storyboard \"{}\" limitados a {MAX_STORYBOARD_ITEMS}",
                    st.name
                ));
            }
            st.items.retain(|it| media_ids.contains(&it.media_id));
            for it in &mut st.items {
                it.trim_in = it.trim_in.map(|t| t.clamp_safe(Ticks::ZERO, Ticks::MAX));
                it.duration_override = it
                    .duration_override
                    .filter(|d| *d > Ticks::ZERO)
                    .map(|d| d.min(Ticks::MAX));
            }
        }

        self.sort_events();
        fixes
    }
}

/// Maior dimensão de quadro aceita (evita alocações gigantes).
pub const MAX_DIM: u32 = 8192;

fn sanitize_envelopes(envelopes: &mut Vec<Envelope>, allowed: &[EnvKind]) {
    let mut kinds = std::collections::HashSet::new();
    envelopes.retain(|e| allowed.contains(&e.kind) && kinds.insert(e.kind));
    for env in envelopes.iter_mut() {
        let (lo, hi) = env.kind.range();
        env.points.retain(|p| p.v.is_finite());
        for p in &mut env.points {
            p.t = p.t.clamp_safe(Ticks::ZERO, Ticks::MAX);
            p.v = crate::util::clampf(p.v, lo, hi);
        }
        env.sort();
        if env.points.is_empty() {
            *env = Envelope::new(env.kind);
        }
    }
}

fn sanitize_fx(f: &mut FxInstance) {
    // a referência OFX só vale em efeitos OFX; sem ela o efeito vira inofensivo
    if f.kind == FxKind::Ofx {
        match f.ofx.as_mut() {
            Some(o) => o.sanitize(),
            None => f.enabled = false,
        }
    } else {
        f.ofx = None;
    }
    // idem para a referência VST3
    if f.kind == FxKind::Vst3 {
        match f.vst3.as_mut() {
            Some(v) => v.sanitize(),
            None => f.enabled = false,
        }
    } else {
        f.vst3 = None;
    }
    if let Some(cg) = f.color_grade.as_mut() {
        if f.kind == FxKind::ColorCorrector {
            cg.sanitize();
        } else {
            f.color_grade = None;
        }
    }
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
        p.keys.retain(|k| k.1.is_finite());
        for k in &mut p.keys {
            k.0 = k.0.clamp_safe(Ticks::MIN_NEGATIVE, Ticks::MAX);
        }
        p.keys.sort_by_key(|k| k.0);
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

    /// Carrega um projeto `.neoveg` (ou `.nvp` legado) a partir de um arquivo.
    pub fn load_file(path: &std::path::Path) -> Result<(Project, Vec<String>), String> {
        const MAX_PROJECT_BYTES: u64 = 64 * 1024 * 1024;
        let meta = std::fs::metadata(path)
            .map_err(|e| format!("não foi possível acessar {}: {e}", path.display()))?;
        if meta.len() > MAX_PROJECT_BYTES {
            return Err(format!(
                "arquivo de projeto excede o limite de {} MiB",
                MAX_PROJECT_BYTES / (1024 * 1024)
            ));
        }
        let txt = std::fs::read_to_string(path)
            .map_err(|e| format!("erro ao ler {}: {e}", path.display()))?;
        Self::from_json(&txt)
    }

    /// Salva o projeto em disco com rotação de backup padrão (ativo, 3 cópias).
    pub fn save_file(&self, path: &std::path::Path) -> Result<(), String> {
        self.save_file_with_backup(path, true, 3)
    }

    /// Salva o projeto em disco gravando em arquivo temporário, rotacionando cópias
    /// de segurança existentes (.bak, .bak1, .bak2) e renomeando atomicamente.
    pub fn save_file_with_backup(
        &self,
        path: &std::path::Path,
        backup_enabled: bool,
        backup_copies: usize,
    ) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("erro ao serializar projeto: {e}"))?;
        let tmp = path.with_extension("neoveg.tmp");
        std::fs::write(&tmp, json).map_err(|e| format!("erro ao gravar {}: {e}", tmp.display()))?;

        if backup_enabled && path.exists() {
            if let Err(e) = rotate_project_backups(path, backup_copies) {
                // Em caso de falha na rotação (ex.: permissão temporária de leitura),
                // não aborta o salvamento do projeto atual.
                eprintln!("falha ao rotacionar backups de {}: {e}", path.display());
            }
        }

        std::fs::rename(&tmp, path)
            .map_err(|e| format!("erro ao finalizar {}: {e}", path.display()))
    }
}

/// Gera o caminho para a cópia de backup do projeto no índice `idx` (0-indexado).
///
/// O primeiro backup (índice 0) recebe a extensão canônica `.bak` (ex.: `projeto.neoveg.bak`).
/// Os backups históricos subsequentes recebem sufixos numerados `.bak1`, `.bak2`, etc.
pub fn backup_path_for(path: &std::path::Path, idx: usize) -> std::path::PathBuf {
    let file_name = path
        .file_name()
        .map(|f| f.to_string_lossy())
        .unwrap_or_else(|| std::borrow::Cow::Borrowed("projeto.neoveg"));
    let suffix = if idx == 0 {
        format!("{file_name}.bak")
    } else {
        format!("{file_name}.bak{idx}")
    };
    path.with_file_name(suffix)
}

/// Rotaciona as cópias de segurança existentes preservando até `copies` arquivos de backup.
pub fn rotate_project_backups(path: &std::path::Path, copies: usize) -> Result<(), String> {
    if copies == 0 || !path.exists() {
        return Ok(());
    }
    let max_idx = copies.saturating_sub(1);
    let oldest = backup_path_for(path, max_idx);
    if oldest.exists() {
        let _ = std::fs::remove_file(&oldest);
    }
    for i in (1..=max_idx).rev() {
        let prev = backup_path_for(path, i - 1);
        let curr = backup_path_for(path, i);
        if prev.exists() {
            let _ = std::fs::rename(&prev, &curr)
                .or_else(|_| std::fs::copy(&prev, &curr).and_then(|_| std::fs::remove_file(&prev)));
        }
    }
    let first = backup_path_for(path, 0);
    std::fs::copy(path, &first)
        .map_err(|e| format!("erro ao criar cópia de backup {}: {e}", first.display()))?;
    Ok(())
}

/// Extensões reconhecidas para arquivos de projeto do Neoveg (`.neoveg` padrão + `.nvp` legado).
pub const PROJECT_EXTS: [&str; 2] = ["neoveg", "nvp"];

pub fn is_project_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| PROJECT_EXTS.iter().any(|p| ext.eq_ignore_ascii_case(p)))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Mídia
// ---------------------------------------------------------------------------

fn de_opt_fps<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<FrameRate>, D::Error> {
    use serde::de::{self, MapAccess, Visitor};
    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = Option<FrameRate>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("taxa de quadros opcional ou null")
        }
        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D2: serde::Deserializer<'de>>(
            self,
            des: D2,
        ) -> Result<Self::Value, D2::Error> {
            des.deserialize_any(V)
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(if v <= 0 {
                None
            } else {
                FrameRate::from_f64(v as f64)
            })
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(if v == 0 {
                None
            } else {
                FrameRate::from_f64(v as f64)
            })
        }
        fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
            Ok(if !v.is_finite() || v <= 0.0 {
                None
            } else {
                FrameRate::from_f64(v)
            })
        }
        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
            let mut num: Option<u32> = None;
            let mut den: Option<u32> = None;
            while let Some(k) = map.next_key::<String>()? {
                match k.as_str() {
                    "num" => num = Some(map.next_value()?),
                    "den" => den = Some(map.next_value()?),
                    _ => {
                        let _: de::IgnoredAny = map.next_value()?;
                    }
                }
            }
            match (num, den) {
                (Some(n), Some(d)) => Ok(FrameRate::new(n, d)),
                _ => Ok(None),
            }
        }
    }
    d.deserialize_option(V)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MediaItem {
    pub id: Id,
    pub name: String,
    pub source: MediaSource,
    /// Duração exata em ticks (`None` = ilimitada: imagens estáticas e mídia gerada).
    pub duration: Option<Ticks>,
    pub has_video: bool,
    pub has_audio: bool,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default, deserialize_with = "de_opt_fps")]
    pub fps: Option<FrameRate>,
    #[serde(default)]
    pub info: String,
    /// Arquivo auxiliar leve de proxy para edição em hardware modesto (§3.6).
    #[serde(default)]
    pub proxy_path: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MediaSource {
    File {
        path: String,
    },
    ImageSequence {
        paths: Vec<String>,
        fps: FrameRate,
    },
    Generated(GenSpec),
    /// Linha do tempo aninhada (projeto `.neoveg` inserido como evento, p. 114).
    NestedProject {
        path: String,
    },
}

pub const MAX_MEDIA_BINS: usize = 256;
pub const MAX_BIN_DEPTH: usize = 32;
pub const MAX_STORYBOARDS: usize = 64;
pub const MAX_STORYBOARD_ITEMS: usize = 1024;

/// Pasta manual (*Bin*) para organização hierárquica da mídia do projeto.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaBin {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<Id>,
    #[serde(default)]
    pub media_ids: Vec<Id>,
}

/// Critério de filtragem de uma pasta inteligente (*Smart Bin*).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SmartBinRule {
    #[default]
    AllVideo,
    AudioOnly,
    StillImages,
    Generated,
    Unused,
    WithProxy,
    NameContains,
}

impl SmartBinRule {
    pub const ALL: [SmartBinRule; 7] = [
        SmartBinRule::AllVideo,
        SmartBinRule::AudioOnly,
        SmartBinRule::StillImages,
        SmartBinRule::Generated,
        SmartBinRule::Unused,
        SmartBinRule::WithProxy,
        SmartBinRule::NameContains,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SmartBinRule::AllVideo => "Com vídeo",
            SmartBinRule::AudioOnly => "Somente áudio",
            SmartBinRule::StillImages => "Imagens estáticas",
            SmartBinRule::Generated => "Mídias geradas",
            SmartBinRule::Unused => "Não utilizadas",
            SmartBinRule::WithProxy => "Com proxy de vídeo",
            SmartBinRule::NameContains => "Texto no nome",
        }
    }

    pub fn matches(
        self,
        query: &str,
        m: &MediaItem,
        used_media: &std::collections::HashSet<Id>,
    ) -> bool {
        let base = match self {
            SmartBinRule::AllVideo => m.has_video && m.duration.is_some(),
            SmartBinRule::AudioOnly => m.has_audio && !m.has_video,
            SmartBinRule::StillImages => {
                m.has_video
                    && m.duration.is_none()
                    && !matches!(m.source, MediaSource::Generated(_))
            }
            SmartBinRule::Generated => matches!(m.source, MediaSource::Generated(_)),
            SmartBinRule::Unused => !used_media.contains(&m.id),
            SmartBinRule::WithProxy => m.proxy_path.is_some(),
            SmartBinRule::NameContains => true,
        };
        if !base {
            return false;
        }
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        m.name.to_lowercase().contains(&q) || m.info.to_lowercase().contains(&q)
    }
}

/// Pasta inteligente (*Smart Bin*) persistente com filtro dinâmico.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartBin {
    pub id: Id,
    pub name: String,
    pub rule: SmartBinRule,
    #[serde(default)]
    pub query: String,
}

/// Entrada ordenada de uma mídia dentro de um *Storyboard*.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoryboardItem {
    pub media_id: Id,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub trim_in: Option<Ticks>,
    #[serde(default)]
    pub duration_override: Option<Ticks>,
}

/// Sequência de *storyboard* do projeto para organização e inserção sequencial.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Storyboard {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub items: Vec<StoryboardItem>,
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
    pub fn name_in(self, lang: neoveg_common::Language) -> &'static str {
        let pt = match self {
            TextAnim::None => "Nenhuma",
            TextAnim::ScrollUp => "Rolagem (créditos)",
            TextAnim::CrawlLeft => "Letreiro (crawl)",
            TextAnim::Typewriter => "Máquina de escrever",
            TextAnim::FadeInOut => "Fade in/out",
        };
        neoveg_common::tr(lang, pt)
    }

    pub fn name(self) -> &'static str {
        self.name_in(neoveg_common::Language::PtBr)
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
    /// Espaçamento entre caracteres (tracking/kerning manual, em fração do tamanho da fonte).
    #[serde(default)]
    pub tracking: f32,
    /// Quebra automática de linha baseada na largura da caixa.
    #[serde(default)]
    pub wrap: bool,
    /// Largura máxima para quebra automática de linha (% da largura do quadro).
    #[serde(default = "default_wrap_width")]
    pub wrap_width: f32,
}

fn default_wrap_width() -> f32 {
    80.0
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
            tracking: 0.0,
            wrap: false,
            wrap_width: default_wrap_width(),
        }
    }
}

impl GenSpec {
    pub fn label_in(&self, lang: neoveg_common::Language) -> String {
        use neoveg_common::{tr, tr_args};
        match self {
            GenSpec::SolidColor { .. } => tr(lang, "Cor sólida").into(),
            GenSpec::Gradient { radial, .. } => tr(
                lang,
                if *radial {
                    "Gradiente radial"
                } else {
                    "Gradiente linear"
                },
            )
            .into(),
            GenSpec::Checkerboard { .. } => tr(lang, "Xadrez").into(),
            GenSpec::ColorBars => tr(lang, "Barras de cor").into(),
            GenSpec::Noise { .. } => tr(lang, "Ruído").into(),
            GenSpec::Text(t) => {
                let first = t
                    .text
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(24)
                    .collect::<String>();
                tr_args(lang, "Texto: {first}", &[("first", &first)])
            }
        }
    }

    pub fn label(&self) -> String {
        self.label_in(neoveg_common::Language::PtBr)
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
    pub fn name_in(self, lang: neoveg_common::Language) -> &'static str {
        let pt = match self {
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
        };
        neoveg_common::tr(lang, pt)
    }

    pub fn name(self) -> &'static str {
        self.name_in(neoveg_common::Language::PtBr)
    }

    pub fn id(self) -> &'static str {
        match self {
            BlendMode::SourceAlpha => "SourceAlpha",
            BlendMode::Add => "Add",
            BlendMode::Subtract => "Subtract",
            BlendMode::Multiply => "Multiply",
            BlendMode::Screen => "Screen",
            BlendMode::Overlay => "Overlay",
            BlendMode::HardLight => "HardLight",
            BlendMode::Lighten => "Lighten",
            BlendMode::Darken => "Darken",
            BlendMode::Difference => "Difference",
        }
    }

    pub fn from_id(raw: &str) -> Self {
        match raw.trim() {
            "Add" => BlendMode::Add,
            "Subtract" => BlendMode::Subtract,
            "Multiply" => BlendMode::Multiply,
            "Screen" => BlendMode::Screen,
            "Overlay" => BlendMode::Overlay,
            "HardLight" => BlendMode::HardLight,
            "Lighten" => BlendMode::Lighten,
            "Darken" => BlendMode::Darken,
            "Difference" => BlendMode::Difference,
            _ => BlendMode::SourceAlpha,
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
    pub fn name_in(self, lang: neoveg_common::Language) -> &'static str {
        let pt = match self {
            EnvKind::Volume => "Volume",
            EnvKind::Pan => "Pan",
            EnvKind::Opacity => "Nível de composição",
            EnvKind::FadeToColor => "Fade para cor",
        };
        neoveg_common::tr(lang, pt)
    }

    pub fn name(self) -> &'static str {
        self.name_in(neoveg_common::Language::PtBr)
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
    /// Tempo absoluto na timeline (em ticks).
    pub t: Ticks,
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
                t: Ticks::ZERO,
                v: kind.default_value(),
                curve: Curve::Linear,
            }],
            visible: true,
        }
    }
    pub fn eval(&self, t: Ticks) -> f32 {
        let p = &self.points;
        if p.is_empty() {
            return self.kind.default_value();
        }
        if t <= p[0].t {
            return p[0].v;
        }
        for w in p.windows(2) {
            if t >= w[0].t && t < w[1].t {
                let x = Ticks::lerp_factor(t, w[0].t, w[1].t);
                let y = w[0].curve.shape(x);
                return w[0].v + (w[1].v - w[0].v) * y;
            }
        }
        p.last().map(|x| x.v).unwrap_or(self.kind.default_value())
    }
    pub fn sort(&mut self) {
        self.points.sort_by_key(|p| p.t);
    }

    pub fn set_point(&mut self, t: Ticks, v: f32, tol: Ticks) {
        let (lo, hi) = self.kind.range();
        let v = if v.is_finite() {
            crate::util::clampf(v, lo, hi)
        } else {
            self.kind.default_value()
        };
        let t = t.clamp_safe(Ticks::ZERO, Ticks::MAX);
        if let Some(p) = self.points.iter_mut().find(|p| p.t.abs_diff(t) <= tol) {
            p.v = v;
        } else {
            self.points.push(EnvPoint {
                t,
                v,
                curve: Curve::Linear,
            });
            self.sort();
        }
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

/// Profundidade máxima permitida na hierarquia de pastas e trilhas pai/filho (§0.5).
pub const MAX_TRACK_DEPTH: usize = 16;

/// Modo de máscara em composição Pai/Filho (VEGAS Pro p. 740).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TrackMaskMode {
    #[default]
    None,
    /// Usa luminância × alfa da trilha como máscara sobre a composição.
    MultiplyMask,
    /// Inverte a máscara de luminância (`1 - luma * alpha`).
    InvertedMultiplyMask,
    /// Usa exclusivamente o canal alfa da trilha como máscara.
    AlphaMask,
    /// Inverte a máscara de canal alfa (`1 - alpha`).
    InvertedAlphaMask,
}

impl TrackMaskMode {
    pub const ALL: [TrackMaskMode; 5] = [
        TrackMaskMode::None,
        TrackMaskMode::MultiplyMask,
        TrackMaskMode::InvertedMultiplyMask,
        TrackMaskMode::AlphaMask,
        TrackMaskMode::InvertedAlphaMask,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TrackMaskMode::None => "Normal (sem máscara)",
            TrackMaskMode::MultiplyMask => "Máscara por luminância",
            TrackMaskMode::InvertedMultiplyMask => "Máscara por luminância invertida",
            TrackMaskMode::AlphaMask => "Máscara por alfa",
            TrackMaskMode::InvertedAlphaMask => "Máscara por alfa invertido",
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
    /// Áudio: pan -1..1 (modo estéreo).
    #[serde(default)]
    pub pan: f32,
    /// Áudio: posição no *Surround Panner* (modo 5.1; substitui `pan`).
    #[serde(default)]
    pub surround: SurroundPan,
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
    /// Barramento auxiliar de destino (`None` = Master Bus). Apenas trilhas de áudio.
    #[serde(default)]
    pub output_bus: Option<Id>,
    /// Envios auxiliares para barramentos de áudio (`sends`).
    #[serde(default)]
    pub sends: Vec<AudioSend>,
    /// Vídeo: trilha de ajuste (*Adjustment Track*). Não desenha eventos; aplica
    /// os FX da trilha sobre a composição das trilhas abaixo dela, com a
    /// intensidade dada por opacidade × envelope de opacidade. Ignorado em áudio.
    #[serde(default)]
    pub adjustment: bool,
    /// Áudio: faixa armada para gravação ao vivo (`Rec`).
    #[serde(default)]
    pub record_armed: bool,
    /// Áudio: monitoração de entrada durante gravação/espera.
    #[serde(default)]
    pub input_monitor: bool,
    /// Pasta de trilhas (Track Group) recolhível sem eventos próprios.
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_folder: bool,
    /// Pasta recolhida na timeline (oculta visualmente as trilhas filhas).
    #[serde(default, skip_serializing_if = "is_false")]
    pub collapsed: bool,
    /// Trilha pai (pasta de grupo ou composição Pai/Filho em vídeo).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_track_id: Option<Id>,
    /// Modo de máscara da trilha na composição Pai/Filho.
    #[serde(default)]
    pub mask_mode: TrackMaskMode,
    /// Movimento 2D/3D do grupo Pai/Filho (Parent Track Motion, p. 740).
    #[serde(default)]
    pub parent_motion: Keyframes<MotionKey>,
    /// Trilha marcada como multicâmera (angles em takes).
    #[serde(default, skip_serializing_if = "is_false")]
    pub multicam: bool,
    /// Modo de automação dos envelopes da faixa (Desligado / Ler / Touch / Latch, p. 1008).
    #[serde(default)]
    pub automation_mode: AutomationMode,
}

/// Modo de automação de envelopes por trilha ou barramento (manual VEGAS Pro 21, p. 1008).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum AutomationMode {
    /// Ignora os envelopes durante a reprodução/mixagem.
    Off,
    /// Apenas lê a curva de automação existente (padrão).
    #[default]
    Read,
    /// Grava pontos ao arrastar durante a reprodução e retorna ao liberar.
    Touch,
    /// Grava pontos e mantém o último valor enquanto a reprodução continua.
    Latch,
}

impl AutomationMode {
    pub const ALL: [AutomationMode; 4] = [
        AutomationMode::Off,
        AutomationMode::Read,
        AutomationMode::Touch,
        AutomationMode::Latch,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AutomationMode::Off => "Automação desligada (Off)",
            AutomationMode::Read => "Automação: Ler (Read)",
            AutomationMode::Touch => "Automação: Touch (Gravar ao tocar)",
            AutomationMode::Latch => "Automação: Latch (Gravar e reter)",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            AutomationMode::Off => "Off",
            AutomationMode::Read => "Read",
            AutomationMode::Touch => "Touch",
            AutomationMode::Latch => "Latch",
        }
    }

    pub fn is_write(self) -> bool {
        matches!(self, AutomationMode::Touch | AutomationMode::Latch)
    }
}

/// Máximo de barramentos auxiliares de áudio (Bus A..Bus Z + Master, AGENTS.md §3.2).
pub const MAX_AUDIO_BUSES: usize = 26;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AudioSend {
    pub bus_id: Id,
    #[serde(default)]
    pub level_db: f32,
    #[serde(default)]
    pub pre_fader: bool,
    #[serde(default)]
    pub mute: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioBus {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub volume_db: f32,
    #[serde(default)]
    pub pan: f32,
    /// Posição no *Surround Panner* quando o barramento sai para o Master em 5.1.
    #[serde(default)]
    pub surround: SurroundPan,
    /// Saída do barramento (`None` = Master Bus, `Some(id)` = outro barramento acíclico).
    #[serde(default)]
    pub output_bus: Option<Id>,
    #[serde(default)]
    pub fx: Vec<FxInstance>,
    #[serde(default)]
    pub envelopes: Vec<Envelope>,
    #[serde(default)]
    pub automation_mode: AutomationMode,
}

impl AudioBus {
    pub fn new(name: &str) -> Self {
        AudioBus {
            id: new_id(),
            name: name.into(),
            mute: false,
            solo: false,
            volume_db: 0.0,
            pan: 0.0,
            surround: SurroundPan::default(),
            output_bus: None,
            fx: vec![],
            envelopes: vec![],
            automation_mode: AutomationMode::Read,
        }
    }

    pub fn envelope(&self, kind: EnvKind) -> Option<&Envelope> {
        self.envelopes.iter().find(|e| e.kind == kind)
    }

    pub fn active_envelope(&self, kind: EnvKind) -> Option<&Envelope> {
        (self.automation_mode != AutomationMode::Off)
            .then(|| self.envelope(kind))
            .flatten()
    }

    pub fn is_audible(&self, any_bus_solo: bool) -> bool {
        !self.mute && (!any_bus_solo || self.solo)
    }
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
            surround: SurroundPan::default(),
            opacity: 1.0,
            blend: BlendMode::SourceAlpha,
            motion: Keyframes::default(),
            fx: vec![],
            envelopes: vec![],
            color: TRACK_COLORS[(id as usize) % TRACK_COLORS.len()],
            output_bus: None,
            sends: vec![],
            adjustment: false,
            record_armed: false,
            input_monitor: false,
            is_folder: false,
            collapsed: false,
            parent_track_id: None,
            mask_mode: TrackMaskMode::None,
            parent_motion: Keyframes::default(),
            multicam: false,
            automation_mode: AutomationMode::Read,
        }
    }

    /// Verdadeiro para pasta/grupo de trilhas.
    pub fn is_folder_track(&self) -> bool {
        self.is_folder
    }

    /// Verdadeiro para trilha de vídeo marcada como trilha de ajuste.
    pub fn is_adjustment(&self) -> bool {
        self.kind == TrackKind::Video && self.adjustment && !self.is_folder
    }

    /// Verdadeiro se a trilha pode receber eventos do tipo `kind`. Trilhas de
    /// ajuste ou pastas não recebem eventos: o compositor os ignoraria.
    pub fn accepts(&self, kind: TrackKind) -> bool {
        self.kind == kind && !self.is_adjustment() && !self.is_folder
    }

    pub fn envelope(&self, kind: EnvKind) -> Option<&Envelope> {
        self.envelopes.iter().find(|e| e.kind == kind)
    }

    pub fn active_envelope(&self, kind: EnvKind) -> Option<&Envelope> {
        (self.automation_mode != AutomationMode::Off)
            .then(|| self.envelope(kind))
            .flatten()
    }

    pub fn env_value(&self, kind: EnvKind, t: Ticks) -> f32 {
        self.active_envelope(kind)
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
    WarpFlow,
}

impl TransitionKind {
    pub const ALL: [TransitionKind; 17] = [
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
        TransitionKind::WarpFlow,
    ];
    pub fn name_in(self, lang: neoveg_common::Language) -> &'static str {
        let pt = match self {
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
            TransitionKind::WarpFlow => "Warp Flow (fluxo óptico)",
        };
        neoveg_common::tr(lang, pt)
    }

    pub fn name(self) -> &'static str {
        self.name_in(neoveg_common::Language::PtBr)
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
    /// Início na timeline (em ticks).
    pub start: Ticks,
    /// Duração na timeline (em ticks).
    pub length: Ticks,
    /// Deslocamento dentro da mídia (take start offset), em ticks de mídia.
    #[serde(default)]
    pub offset: Ticks,
    /// Velocidade de reprodução racional (playback rate).
    #[serde(default)]
    pub rate: Rate,
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
    /// Takes alternativos (não ativos) do evento (manual do VEGAS Pro 21,
    /// p. 346). O take ativo continua em `media_id` / `offset` /
    /// `take_name`, então render e edição não mudam.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alt_takes: Vec<Take>,
    /// Posição do take ativo na lista completa de takes (`0..=alt_takes.len()`).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub take_index: u32,
    /// Nome do take ativo (vazio = nome da mídia).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub take_name: String,
    /// Deslocamento de tom em semitons (`-24.0..=24.0`, manual VEGAS Pro 21 p. 363).
    #[serde(default)]
    pub pitch_semitones: f32,
    /// Preserva o tom ao esticar/comprimir o evento (`Event::rate != 1`).
    #[serde(default)]
    pub lock_pitch_to_stretch: bool,
    /// ID do evento mestre para *Sync Link* unidirecional (manual VEGAS Pro 21 p. 351).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_link_master: Option<Id>,
    /// Parâmetros 3D estereoscópicos do evento (disposição L/R, inversão e deslocamento).
    #[serde(default, skip_serializing_if = "Stereo3dClip::is_default")]
    pub stereo_3d: Stereo3dClip,
}

/// Um take de um evento: a mídia, onde ela começa e o nome do take.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Take {
    pub media_id: Id,
    #[serde(default)]
    pub offset: Ticks,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

impl Event {
    pub fn new(media: &MediaItem, start: Ticks, length: Ticks) -> Self {
        Event {
            id: new_id(),
            name: media.name.clone(),
            media_id: media.id,
            start,
            length,
            offset: Ticks::ZERO,
            rate: Rate::NORMAL,
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
            alt_takes: Vec::new(),
            take_index: 0,
            take_name: String::new(),
            pitch_semitones: 0.0,
            lock_pitch_to_stretch: false,
            sync_link_master: None,
            stereo_3d: Stereo3dClip::default(),
        }
    }

    /// Fator efetivo de altura tonal do áudio (`1.0` = tom original), considerando
    /// `pitch_semitones` e `lock_pitch_to_stretch`.
    pub fn effective_pitch_factor(&self) -> f64 {
        let rate = self.rate.as_f64();
        if rate <= 0.0 || !rate.is_finite() {
            return 1.0;
        }
        let semi = if self.pitch_semitones.is_finite() {
            f64::from(self.pitch_semitones.clamp(-24.0, 24.0))
        } else {
            0.0
        };
        let p = 2.0_f64.powf(semi / 12.0);
        if self.lock_pitch_to_stretch {
            p
        } else {
            p * rate
        }
    }

    /// Desloca a mídia de **todos** os takes por `delta` (ticks de mídia).
    /// Usado quando o início do evento anda junto com a mídia (dividir,
    /// aparar à esquerda): os takes continuam sincronizados entre si.
    pub fn shift_offsets(&mut self, delta: Ticks) {
        self.offset += delta;
        for t in &mut self.alt_takes {
            t.offset = (t.offset + delta).max(Ticks::ZERO);
        }
    }

    pub fn end(&self) -> Ticks {
        self.start + self.length
    }

    pub fn contains(&self, t: Ticks) -> bool {
        t >= self.start && t < self.end()
    }

    /// Converte tempo da timeline em tempo da mídia (em ticks exatos),
    /// considerando offset, velocidade racional, loop e reversão.
    pub fn media_time(&self, t: Ticks, media_dur: Option<Ticks>) -> Option<Ticks> {
        let local = self.rate.scale_ticks(t - self.start);
        let mut mt = if self.reverse {
            let total = self.rate.scale_ticks(self.length);
            self.offset + (total - local)
        } else {
            self.offset + local
        };
        if let Some(d) = media_dur {
            if d <= Ticks::ZERO {
                return None;
            }
            let eps = Ticks::from_raw(TICKS_PER_SEC / 10_000);
            if mt >= d && mt < d + eps {
                mt = (d - Ticks::from_raw(1)).max(Ticks::ZERO);
            }
            if self.looped {
                mt = mt.rem_euclid(d);
            } else if mt < Ticks::ZERO || mt >= d {
                return None;
            }
        }
        Some(mt.max(Ticks::ZERO))
    }

    /// Ganho dos fades de entrada/saída (0..1) no tempo `t`.
    pub fn fade_gain(&self, t: Ticks) -> f32 {
        let local = t - self.start;
        let mut g = 1.0f32;
        if self.fade_in.len > Ticks::ZERO && local < self.fade_in.len {
            g *= self
                .fade_in
                .curve
                .shape(local.fraction_in(Ticks::ZERO, self.fade_in.len));
        }
        let rem = self.end() - t;
        if self.fade_out.len > Ticks::ZERO && rem < self.fade_out.len {
            g *= self
                .fade_out
                .curve
                .shape(rem.fraction_in(Ticks::ZERO, self.fade_out.len));
        }
        g
    }
}

// ---------------------------------------------------------------------------
// Efeitos
// ---------------------------------------------------------------------------

/// Categoria de um efeito de vídeo na janela Efeitos de Vídeo (manual do
/// VEGAS Pro 21, p. 54: os efeitos ficam organizados em categorias). As
/// categorias e os nomes são do Neoveg.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FxCategory {
    Color,
    Light,
    BlurSharpen,
    Stylize,
    Transform,
    Key,
}

impl FxCategory {
    pub const ALL: [FxCategory; 6] = [
        FxCategory::Color,
        FxCategory::Light,
        FxCategory::BlurSharpen,
        FxCategory::Stylize,
        FxCategory::Transform,
        FxCategory::Key,
    ];

    pub fn name_in(self, lang: neoveg_common::Language) -> &'static str {
        let pt = match self {
            FxCategory::Color => "Cor",
            FxCategory::Light => "Luz",
            FxCategory::BlurSharpen => "Desfoque e nitidez",
            FxCategory::Stylize => "Estilizar",
            FxCategory::Transform => "Estabilização",
            FxCategory::Key => "Recorte",
        };
        neoveg_common::tr(lang, pt)
    }

    pub fn name(self) -> &'static str {
        self.name_in(neoveg_common::Language::PtBr)
    }
}

// Efeitos externos (OFX para vídeo, VST3 para áudio) rodam no host isolado
// `neoveg --plugin-host` (fronteira FFI §0.3); ver `plugin_host`.
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
    Stabilization,
    BezierMasking,
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
    Limiter,
    /// Efeito de vídeo OpenFX de terceiros (ver `FxInstance::ofx`), processado
    /// no host isolado `neoveg --plugin-host`.
    Ofx,
    /// Efeito de áudio VST3 de terceiros (ver `FxInstance::vst3`), processado
    /// no host isolado `neoveg --plugin-host serve-vst3`.
    Vst3,
}

/// Valor de um parâmetro de plug-in OFX, pelo nome do parâmetro. Sem
/// keyframes vale `values`; com eles, o valor no tempo local do efeito.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OfxParamValue {
    pub name: String,
    #[serde(default)]
    pub values: Vec<f64>,
    /// Keyframes (tempo local do efeito, valores), em ordem de tempo.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<(Ticks, Vec<f64>)>,
    /// Entre keyframes segura o valor anterior (Boolean, Choice) em vez de
    /// interpolar.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hold: bool,
    /// Interpola e arredonda para inteiro (parâmetros Integer).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub integer: bool,
}

/// Máximo de keyframes por parâmetro OFX (entrada hostil, §0.4).
pub(crate) const MAX_OFX_KEYS: usize = 4096;

impl OfxParamValue {
    /// Valores no tempo local `t`: antes do primeiro keyframe vale o
    /// primeiro, depois do último vale o último; entre dois, interpolação
    /// linear (ou o anterior, com `hold`).
    pub fn values_at(&self, t: Ticks) -> Vec<f64> {
        let (Some(first), Some(last)) = (self.keys.first(), self.keys.last()) else {
            return self.values.clone();
        };
        if t <= first.0 {
            return first.1.clone();
        }
        if t >= last.0 {
            return last.1.clone();
        }
        for w in self.keys.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if t >= a.0 && t < b.0 {
                if self.hold {
                    return a.1.clone();
                }
                let x = f64::from(Ticks::lerp_factor(t, a.0, b.0));
                return a
                    .1
                    .iter()
                    .enumerate()
                    .map(|(i, va)| {
                        let vb = b.1.get(i).copied().unwrap_or(*va);
                        let v = va + (vb - va) * x;
                        if self.integer {
                            v.round()
                        } else {
                            v
                        }
                    })
                    .collect();
            }
        }
        last.1.clone()
    }

    /// Grava `v` como keyframe em `t` (substitui um keyframe a até `tol`).
    pub fn set_key(&mut self, t: Ticks, v: Vec<f64>, tol: Ticks) {
        if let Some(k) = self.keys.iter_mut().find(|k| k.0.abs_diff(t) <= tol) {
            k.1 = v;
        } else if self.keys.len() < MAX_OFX_KEYS {
            self.keys.push((t, v));
            self.keys.sort_by_key(|k| k.0);
        }
    }
}

/// Referência a um efeito OFX: binário, identificador e valores dos parâmetros.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OfxFxRef {
    pub binary: String,
    pub plugin_id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub params: Vec<OfxParamValue>,
}

const MAX_OFX_REF_TEXT: usize = 4096;
const MAX_OFX_REF_PARAMS: usize = 1024;

impl OfxFxRef {
    /// Valores de todos os parâmetros no tempo local `t` (keyframes
    /// avaliados), prontos para o host.
    pub fn params_at(&self, t: Ticks) -> Vec<OfxParamValue> {
        self.params
            .iter()
            .map(|p| OfxParamValue {
                name: p.name.clone(),
                values: p.values_at(t),
                ..Default::default()
            })
            .collect()
    }

    /// Saneia textos, contagens e valores vindos de arquivo (§0.4).
    pub fn sanitize(&mut self) {
        let cut = |s: &mut String| {
            if s.len() > MAX_OFX_REF_TEXT {
                let mut end = MAX_OFX_REF_TEXT;
                while !s.is_char_boundary(end) {
                    end -= 1;
                }
                s.truncate(end);
            }
        };
        cut(&mut self.binary);
        cut(&mut self.plugin_id);
        cut(&mut self.label);
        self.params.truncate(MAX_OFX_REF_PARAMS);
        let clean = |vals: &mut Vec<f64>| {
            vals.truncate(4);
            for v in vals.iter_mut() {
                if !v.is_finite() {
                    *v = 0.0;
                }
            }
        };
        for p in &mut self.params {
            cut(&mut p.name);
            clean(&mut p.values);
            p.keys.truncate(MAX_OFX_KEYS);
            p.keys.retain(|k| k.0 >= Ticks::ZERO);
            for k in &mut p.keys {
                clean(&mut k.1);
            }
            p.keys.sort_by_key(|k| k.0);
            p.keys.dedup_by_key(|k| k.0);
        }
    }
}

/// Valor normalizado (0..1) de um parâmetro VST3, pelo `ParamID` do plug-in.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vst3ParamValue {
    pub id: u32,
    pub value: f64,
}

/// Tamanho máximo em bytes do estado binário serializado de um componente VST3.
pub const MAX_VST3_STATE_BYTES: usize = 1024 * 1024;
const MAX_VST3_STATE_HEX: usize = MAX_VST3_STATE_BYTES * 2;

/// Codifica bytes arbitrários em string hexadecimal minúscula (limitada a [`MAX_VST3_STATE_BYTES`]).
pub fn encode_hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let capped = &bytes[..bytes.len().min(MAX_VST3_STATE_BYTES)];
    let mut out = String::with_capacity(capped.len().saturating_mul(2));
    for &b in capped {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Decodifica string hexadecimal (par, apenas dígitos ASCII hex e limitada a
/// [`MAX_VST3_STATE_BYTES`]). Retorna `None` para entrada malformada ou excessiva.
pub fn decode_hex_bytes(hex: &str) -> Option<Vec<u8>> {
    let trimmed = hex.trim();
    if trimmed.len() > MAX_VST3_STATE_HEX || !trimmed.len().is_multiple_of(2) {
        return None;
    }
    let (pairs, _) = trimmed.as_bytes().as_chunks::<2>();
    let mut out = Vec::with_capacity(pairs.len());
    for pair in pairs {
        let hi = char::from(pair[0]).to_digit(16)?;
        let lo = char::from(pair[1]).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
    }
    Some(out)
}

/// Referência a um efeito VST3: binário do módulo, classe (`TUID` em
/// hexadecimal), valores normalizados dos parâmetros e estado opaco do
/// componente (`IComponent::getState` em hexadecimal).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vst3FxRef {
    pub binary: String,
    pub class_id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub params: Vec<Vst3ParamValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

const MAX_VST3_REF_PARAMS: usize = 16384;

impl Vst3FxRef {
    /// Saneia textos, contagens, valores e estado binário vindos de arquivo (§0.4).
    pub fn sanitize(&mut self) {
        let cut = |s: &mut String| {
            if s.len() > MAX_OFX_REF_TEXT {
                let mut end = MAX_OFX_REF_TEXT;
                while !s.is_char_boundary(end) {
                    end -= 1;
                }
                s.truncate(end);
            }
        };
        cut(&mut self.binary);
        cut(&mut self.class_id);
        cut(&mut self.label);
        self.params.truncate(MAX_VST3_REF_PARAMS);
        let mut seen = std::collections::HashSet::new();
        self.params.retain(|p| seen.insert(p.id));
        for p in &mut self.params {
            p.value = if p.value.is_finite() {
                p.value.clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
        self.state = self
            .state
            .as_deref()
            .and_then(decode_hex_bytes)
            .filter(|b| !b.is_empty())
            .map(|b| encode_hex_bytes(&b));
    }

    /// Grava o valor normalizado de um parâmetro (substitui o anterior).
    pub fn set_param(&mut self, id: u32, value: f64) {
        let value = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
        match self.params.iter_mut().find(|p| p.id == id) {
            Some(p) => p.value = value,
            None => self.params.push(Vst3ParamValue { id, value }),
        }
    }
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

impl ParamSpec {
    pub fn name_in(self, lang: neoveg_common::Language) -> &'static str {
        neoveg_common::tr(lang, self.name)
    }
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
    /// Categoria na janela Efeitos de Vídeo (`None` para áudio e plug-ins
    /// externos, que ficam em Terceiros).
    pub fn category(self) -> Option<FxCategory> {
        match self {
            FxKind::BrightnessContrast
            | FxKind::HslAdjust
            | FxKind::ColorCorrector
            | FxKind::Levels
            | FxKind::BlackWhite
            | FxKind::Sepia
            | FxKind::Invert
            | FxKind::Posterize
            | FxKind::Colorize => Some(FxCategory::Color),
            FxKind::Glow | FxKind::Vignette => Some(FxCategory::Light),
            FxKind::GaussianBlur | FxKind::Sharpen => Some(FxCategory::BlurSharpen),
            FxKind::Pixelate | FxKind::Mirror | FxKind::FilmGrain | FxKind::Emboss => {
                Some(FxCategory::Stylize)
            }
            FxKind::Stabilization => Some(FxCategory::Transform),
            FxKind::ChromaKeyer | FxKind::BezierMasking => Some(FxCategory::Key),
            FxKind::Gain
            | FxKind::ParametricEq
            | FxKind::HighPass
            | FxKind::LowPass
            | FxKind::Compressor
            | FxKind::NoiseGate
            | FxKind::Delay
            | FxKind::Reverb
            | FxKind::Chorus
            | FxKind::Distortion
            | FxKind::Limiter
            | FxKind::Ofx
            | FxKind::Vst3 => None,
        }
    }

    pub const VIDEO: [FxKind; 20] = [
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
        FxKind::Stabilization,
        FxKind::BezierMasking,
    ];
    pub const AUDIO: [FxKind; 11] = [
        FxKind::Gain,
        FxKind::ParametricEq,
        FxKind::HighPass,
        FxKind::LowPass,
        FxKind::Compressor,
        FxKind::Limiter,
        FxKind::NoiseGate,
        FxKind::Delay,
        FxKind::Reverb,
        FxKind::Chorus,
        FxKind::Distortion,
    ];

    pub fn is_audio(self) -> bool {
        self == FxKind::Vst3 || Self::AUDIO.contains(&self)
    }

    pub fn id(self) -> &'static str {
        match self {
            FxKind::BrightnessContrast => "BrightnessContrast",
            FxKind::HslAdjust => "HslAdjust",
            FxKind::ColorCorrector => "ColorCorrector",
            FxKind::Levels => "Levels",
            FxKind::BlackWhite => "BlackWhite",
            FxKind::Sepia => "Sepia",
            FxKind::Invert => "Invert",
            FxKind::GaussianBlur => "GaussianBlur",
            FxKind::Sharpen => "Sharpen",
            FxKind::Vignette => "Vignette",
            FxKind::ChromaKeyer => "ChromaKeyer",
            FxKind::Pixelate => "Pixelate",
            FxKind::Mirror => "Mirror",
            FxKind::Glow => "Glow",
            FxKind::FilmGrain => "FilmGrain",
            FxKind::Emboss => "Emboss",
            FxKind::Posterize => "Posterize",
            FxKind::Colorize => "Colorize",
            FxKind::Stabilization => "Stabilization",
            FxKind::BezierMasking => "BezierMasking",
            FxKind::Gain => "Gain",
            FxKind::ParametricEq => "ParametricEq",
            FxKind::HighPass => "HighPass",
            FxKind::LowPass => "LowPass",
            FxKind::Compressor => "Compressor",
            FxKind::NoiseGate => "NoiseGate",
            FxKind::Delay => "Delay",
            FxKind::Reverb => "Reverb",
            FxKind::Chorus => "Chorus",
            FxKind::Distortion => "Distortion",
            FxKind::Limiter => "Limiter",
            FxKind::Ofx => "Ofx",
            FxKind::Vst3 => "Vst3",
        }
    }

    pub fn from_id(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        Self::VIDEO
            .into_iter()
            .chain(Self::AUDIO)
            .find(|&k| k.id() == trimmed)
    }

    pub fn name_in(self, lang: neoveg_common::Language) -> &'static str {
        let pt = match self {
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
            FxKind::Stabilization => "Estabilizador de Vídeo",
            FxKind::BezierMasking => "Máscara Bézier",
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
            FxKind::Limiter => "Limiter",
            FxKind::Ofx => "Plug-in OFX",
            FxKind::Vst3 => "Plug-in VST3",
        };
        neoveg_common::tr(lang, pt)
    }

    pub fn name(self) -> &'static str {
        self.name_in(neoveg_common::Language::PtBr)
    }

    pub fn params(self) -> &'static [ParamSpec] {
        match self {
            // os parâmetros de um plug-in OFX/VST3 vêm dele, em
            // `FxInstance::ofx` / `FxInstance::vst3`
            FxKind::Ofx | FxKind::Vst3 => &[],
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
            FxKind::Stabilization => {
                const {
                    &[
                        p("Suavização (%)", 0.0, 100.0, 65.0),
                        p("Zoom de borda", 1.0, 1.5, 1.12),
                        p("Compensar rotação", 0.0, 1.0, 0.35),
                    ]
                }
            }
            FxKind::BezierMasking => {
                const {
                    &[
                        p("Modo (0:Add, 1:Sub, 2:Mul, 3:Inv)", 0.0, 3.0, 0.0),
                        p("Suavização (Feather)", 0.0, 100.0, 0.0),
                        p("Opacidade", 0.0, 1.0, 1.0),
                        p("Expansão", -100.0, 100.0, 0.0),
                        p("Inverter (0:Não, 1:Sim)", 0.0, 1.0, 0.0),
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
            FxKind::Limiter => {
                const {
                    &[
                        p("Ataque (ms)", 0.5, 50.0, 5.0),
                        p("Liberação (ms)", 10.0, 1000.0, 100.0),
                        p("Drive (dB)", -12.0, 24.0, 0.0),
                        p("Teto (dB)", -24.0, 0.0, -0.3),
                    ]
                }
            }
        }
    }
}

/// Parâmetro de efeito animável por keyframes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnimParam {
    pub value: f32,
    /// (tempo local em ticks, valor). Vazio = valor constante.
    #[serde(default)]
    pub keys: Vec<(Ticks, f32)>,
}

impl AnimParam {
    pub fn eval(&self, t: Ticks) -> f32 {
        if self.keys.is_empty() {
            return self.value;
        }
        let k = &self.keys;
        if t <= k[0].0 {
            return k[0].1;
        }
        for w in k.windows(2) {
            if t >= w[0].0 && t < w[1].0 {
                let x = Ticks::lerp_factor(t, w[0].0, w[1].0);
                return w[0].1 + (w[1].1 - w[0].1) * x;
            }
        }
        k.last().map(|x| x.1).unwrap_or(self.value)
    }
    pub fn set_key(&mut self, t: Ticks, v: f32, tol: Ticks) {
        if let Some(k) = self.keys.iter_mut().find(|k| k.0.abs_diff(t) <= tol) {
            k.1 = v;
        } else {
            self.keys.push((t, v));
            self.keys.sort_by_key(|a| a.0);
        }
    }
}

/// Curvas RGB, bandas HSL e LUT (.cube ou predefinida) para Color Grading (pp. 769–781).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorGradeCurves {
    /// Caminho para arquivo `.cube` externo ou `"builtin:<nome>"` (`"builtin:rec709"`, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lut_path: Option<String>,
    /// Intensidade de mistura da LUT (`0.0..=1.0`).
    pub lut_strength: f32,
    /// Curva Master (luminância), pontos `(x, y)` em `[0..1]`. Vazio = identidade.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub master: Vec<[f32; 2]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub red: Vec<[f32; 2]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub green: Vec<[f32; 2]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blue: Vec<[f32; 2]>,
    /// 6 setores HSL (Vermelho, Amarelo, Verde, Ciano, Azul, Magenta): `[matiz (-0.5..0.5), sat (-1..1), lum (-1..1)]`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hsl_bands: Vec<[f32; 3]>,
}

impl Default for ColorGradeCurves {
    fn default() -> Self {
        Self {
            lut_path: None,
            lut_strength: 1.0,
            master: Vec::new(),
            red: Vec::new(),
            green: Vec::new(),
            blue: Vec::new(),
            hsl_bands: Vec::new(),
        }
    }
}

impl ColorGradeCurves {
    pub const BUILTIN_LUTS: [(&'static str, &'static str); 4] = [
        ("builtin:rec709", "Câmera: Log -> Rec.709"),
        ("builtin:teal_orange", "Look: Cinema Teal & Orange"),
        ("builtin:film_cool", "Look: Película Fria"),
        ("builtin:mono_film", "Look: P&B Contraste Fílmico"),
    ];

    pub const HSL_BAND_NAMES: [&'static str; 6] =
        ["Vermelho", "Amarelo", "Verde", "Ciano", "Azul", "Magenta"];

    pub fn default_curve() -> Vec<[f32; 2]> {
        vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]]
    }

    pub fn default_hsl_bands() -> Vec<[f32; 3]> {
        vec![[0.0, 0.0, 0.0]; 6]
    }

    pub fn sanitize(&mut self) {
        let fin = |v: f32, d: f32| if v.is_finite() { v } else { d };
        self.lut_strength = fin(self.lut_strength, 1.0).clamp(0.0, 1.0);
        if self
            .lut_path
            .as_ref()
            .is_some_and(|p| p.trim().is_empty() || p.len() > 1024 || p.contains('\0'))
        {
            self.lut_path = None;
        }
        let clean_curve = |pts: &mut Vec<[f32; 2]>| {
            pts.truncate(32);
            for pt in pts.iter_mut() {
                pt[0] = fin(pt[0], 0.0).clamp(0.0, 1.0);
                pt[1] = fin(pt[1], 0.0).clamp(0.0, 1.0);
            }
            pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
        };
        clean_curve(&mut self.master);
        clean_curve(&mut self.red);
        clean_curve(&mut self.green);
        clean_curve(&mut self.blue);
        self.hsl_bands.truncate(6);
        for b in &mut self.hsl_bands {
            b[0] = fin(b[0], 0.0).clamp(-0.5, 0.5);
            b[1] = fin(b[1], 0.0).clamp(-1.0, 1.0);
            b[2] = fin(b[2], 0.0).clamp(-1.0, 1.0);
        }
    }

    /// Avalia uma curva linear por partes em `x in [0..1]`; curva vazia devolve `x`.
    pub fn eval_curve(pts: &[[f32; 2]], x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        if pts.is_empty() {
            return x;
        }
        if pts.len() == 1 {
            return pts[0][1].clamp(0.0, 1.0);
        }
        if x <= pts[0][0] {
            return pts[0][1].clamp(0.0, 1.0);
        }
        for w in pts.windows(2) {
            if x <= w[1][0] {
                let span = (w[1][0] - w[0][0]).max(1e-5);
                let t = ((x - w[0][0]) / span).clamp(0.0, 1.0);
                return (w[0][1] + (w[1][1] - w[0][1]) * t).clamp(0.0, 1.0);
            }
        }
        pts.last().map(|p| p[1].clamp(0.0, 1.0)).unwrap_or(x)
    }

    pub fn is_identity(&self) -> bool {
        self.lut_path.is_none()
            && self.master.is_empty()
            && self.red.is_empty()
            && self.green.is_empty()
            && self.blue.is_empty()
            && self
                .hsl_bands
                .iter()
                .all(|b| b[0].abs() < 1e-4 && b[1].abs() < 1e-4 && b[2].abs() < 1e-4)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FxInstance {
    pub id: Id,
    pub kind: FxKind,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub params: Vec<AnimParam>,
    /// Só em `FxKind::Ofx`: o plug-in OFX e os valores dos parâmetros dele.
    #[serde(default)]
    pub ofx: Option<OfxFxRef>,
    /// Só em `FxKind::Vst3`: o plug-in VST3 e os valores dos parâmetros dele.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vst3: Option<Vst3FxRef>,
    /// Curvas RGB/HSL e LUT (.cube) para `FxKind::ColorCorrector`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_grade: Option<Box<ColorGradeCurves>>,
    /// Dados e quadros-chave para `FxKind::BezierMasking`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bezier_mask: Option<Box<crate::mask::BezierMaskData>>,
}

impl FxInstance {
    pub fn new(kind: FxKind) -> Self {
        let bezier_mask = if kind == FxKind::BezierMasking {
            Some(Box::new(crate::mask::BezierMaskData::default()))
        } else {
            None
        };
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
            ofx: None,
            vst3: None,
            color_grade: None,
            bezier_mask,
        }
    }

    /// Efeito OFX de terceiros (`binary` + `plugin_id`), com os parâmetros padrão.
    pub fn new_ofx(binary: &str, plugin_id: &str, label: &str) -> Self {
        let mut f = FxInstance::new(FxKind::Ofx);
        f.ofx = Some(OfxFxRef {
            binary: binary.into(),
            plugin_id: plugin_id.into(),
            label: label.into(),
            params: vec![],
        });
        f
    }

    /// Efeito de áudio VST3 de terceiros (`binary` + classe), com os
    /// parâmetros no padrão do plug-in.
    pub fn new_vst3(binary: &str, class_id: &str, label: &str) -> Self {
        let mut f = FxInstance::new(FxKind::Vst3);
        f.vst3 = Some(Vst3FxRef {
            binary: binary.into(),
            class_id: class_id.into(),
            label: label.into(),
            params: vec![],
            state: None,
        });
        f
    }

    /// Nome para exibição (rótulo do plug-in externo ou nome nativo).
    pub fn display_name(&self) -> String {
        match (&self.ofx, &self.vst3, self.kind) {
            (Some(o), _, FxKind::Ofx) if !o.label.is_empty() => o.label.clone(),
            (Some(o), _, FxKind::Ofx) => o.plugin_id.clone(),
            (_, Some(v), FxKind::Vst3) if !v.label.is_empty() => v.label.clone(),
            (_, Some(v), FxKind::Vst3) => v.class_id.clone(),
            _ => self.kind.name().to_string(),
        }
    }
    /// Valores de todos os parâmetros no tempo local `t`, sempre dentro da
    /// faixa declarada (valores vindos de arquivo são hostis: §0.4).
    pub fn values(&self, t: Ticks) -> Vec<f32> {
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
    pub mute: bool,
    #[serde(default)]
    pub fx: Vec<FxInstance>,
    /// Efeitos de saída de vídeo (Video Output FX).
    #[serde(default)]
    pub video_fx: Vec<FxInstance>,
}

/// Tipo de marcador na timeline (padrão, faixa/índice de CD ou comando, pp. 131, 495, 725).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum MarkerKind {
    #[default]
    Standard,
    CdTrack,
    CdIndex,
    Command,
}

impl MarkerKind {
    pub const ALL: [MarkerKind; 4] = [
        MarkerKind::Standard,
        MarkerKind::CdTrack,
        MarkerKind::CdIndex,
        MarkerKind::Command,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MarkerKind::Standard => "Marcador padrão",
            MarkerKind::CdTrack => "Faixa de CD de áudio",
            MarkerKind::CdIndex => "Índice de CD de áudio",
            MarkerKind::Command => "Marcador de comando",
        }
    }

    pub fn short_prefix(self) -> &'static str {
        match self {
            MarkerKind::Standard => "",
            MarkerKind::CdTrack => "CD",
            MarkerKind::CdIndex => "IDX",
            MarkerKind::Command => "CMD",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Marker {
    pub id: Id,
    pub t: Ticks,
    pub label: String,
    pub kind: MarkerKind,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub command_type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub command_arg: String,
}

impl Marker {
    pub fn new(t: Ticks, kind: MarkerKind, label: impl Into<String>) -> Self {
        Self {
            id: new_id(),
            t,
            label: label.into(),
            kind,
            command_type: String::new(),
            command_arg: String::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Region {
    pub id: Id,
    pub start: Ticks,
    pub end: Ticks,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Keyframes de parâmetros OFX: interpolação linear, `hold` para Boolean
    /// e Choice, arredondamento para Integer, e saneamento de entrada hostil.
    #[test]
    fn ofx_param_keyframes_interpolate_and_sanitize() {
        let s = Ticks::from_secs_i64;
        let mut p = OfxParamValue {
            name: "amount".into(),
            values: vec![9.0],
            ..Default::default()
        };
        assert_eq!(p.values_at(s(3)), vec![9.0], "sem keyframes vale `values`");
        let tol = Ticks::ZERO;
        p.set_key(s(2), vec![10.0, 0.0], tol);
        p.set_key(s(0), vec![0.0, 4.0], tol);
        assert_eq!(p.values_at(Ticks(-5)), vec![0.0, 4.0]);
        assert_eq!(p.values_at(s(1)), vec![5.0, 2.0]);
        assert_eq!(p.values_at(s(7)), vec![10.0, 0.0]);
        p.integer = true;
        assert_eq!(p.values_at(Ticks::from_millis_i64(250)), vec![1.0, 4.0]);
        p.hold = true;
        assert_eq!(p.values_at(Ticks::from_millis_i64(1999)), vec![0.0, 4.0]);
        // substituir dentro da tolerância não duplica
        p.set_key(s(2), vec![1.0, 1.0], Ticks::from_millis_i64(1));
        assert_eq!(p.keys.len(), 2);

        // vindo de arquivo: fora de ordem, tempo negativo, NaN, duplicado
        let mut r = OfxFxRef {
            binary: "b".into(),
            plugin_id: "p".into(),
            label: String::new(),
            params: vec![OfxParamValue {
                name: "x".into(),
                values: vec![1.0],
                keys: vec![
                    (s(4), vec![f64::NAN; 9]),
                    (Ticks(-1), vec![1.0]),
                    (s(1), vec![2.0]),
                    (s(4), vec![3.0]),
                ],
                ..Default::default()
            }],
        };
        r.sanitize();
        let k = &r.params[0].keys;
        assert_eq!(k.len(), 2);
        assert_eq!(k[0], (s(1), vec![2.0]));
        assert_eq!(k[1].0, s(4));
        assert_eq!(k[1].1, vec![0.0; 4]);
        // `params_at` entrega só valores estáticos ao host
        let at = r.params_at(Ticks::from_millis_i64(2500));
        // entre 2.0 e 0.0: metade; componentes ausentes ficam para o host (zero)
        assert_eq!(at[0].values, vec![1.0]);
        assert!(at[0].keys.is_empty());
        // parâmetro sem keyframes não grava os campos novos (formato antigo)
        let json = serde_json::to_value(OfxParamValue {
            name: "y".into(),
            values: vec![0.5],
            ..Default::default()
        })
        .expect("grava");
        assert_eq!(json, serde_json::json!({"name": "y", "values": [0.5]}));
    }

    /// Referência VST3 vinda de arquivo é hostil (§0.4): valores fora de
    /// 0..1, NaN, ids repetidos e textos enormes são saneados; um FX VST3 sem
    /// referência fica desligado; e projetos antigos carregam sem o campo.
    #[test]
    fn vst3_fx_ref_is_sanitized_and_optional() {
        let mut p = Project::new_default();
        let mut fx = FxInstance::new_vst3("x.vst3", &"A".repeat(32), &"é".repeat(5000));
        if let Some(r) = fx.vst3.as_mut() {
            r.params = vec![
                Vst3ParamValue { id: 1, value: 2.5 },
                Vst3ParamValue { id: 2, value: -3.0 },
                Vst3ParamValue { id: 1, value: 0.5 },
            ];
        }
        let mut orphan = FxInstance::new(FxKind::Vst3);
        orphan.vst3 = None;
        let mut native = FxInstance::new(FxKind::Gain);
        native.vst3 = Some(Vst3FxRef {
            binary: "y".into(),
            class_id: "z".into(),
            label: String::new(),
            params: vec![],
            state: Some("00FF".into()),
        });
        p.master.fx = vec![fx, orphan, native];
        let json = serde_json::to_string(&p).expect("serializa");
        let mut back: Project = serde_json::from_str(&json).expect("lê de volta");
        // NaN não sobrevive ao JSON; injeta direto, como um script faria
        if let Some(r) = back.master.fx[0].vst3.as_mut() {
            r.params.push(Vst3ParamValue {
                id: 3,
                value: f64::NAN,
            });
            r.state = Some("AABB".into());
        }
        let _ = back.sanitize();
        let r = back.master.fx[0].vst3.as_ref().expect("referência mantida");
        assert_eq!(
            r.params,
            vec![
                Vst3ParamValue { id: 1, value: 1.0 },
                Vst3ParamValue { id: 2, value: 0.0 },
                Vst3ParamValue { id: 3, value: 0.0 },
            ]
        );
        assert_eq!(r.state.as_deref(), Some("aabb"));
        assert!(r.label.len() <= 4096 && r.label.is_char_boundary(r.label.len()));
        assert!(!back.master.fx[1].enabled, "FX VST3 sem referência desliga");
        assert!(
            back.master.fx[2].vst3.is_none(),
            "FX nativo não guarda VST3"
        );
        assert!(FxKind::Vst3.is_audio());

        // estado VST3 hostil ou malformado é descartado na sanitização (§0.4)
        let mut hostile = Vst3FxRef {
            binary: "b".into(),
            class_id: "c".into(),
            label: "l".into(),
            params: vec![],
            state: Some("abc".into()), // ímpar
        };
        hostile.sanitize();
        assert_eq!(hostile.state, None);
        hostile.state = Some("zzzz".into());
        hostile.sanitize();
        assert_eq!(hostile.state, None);
        hostile.state = Some("a".repeat(MAX_VST3_STATE_BYTES * 2 + 2));
        hostile.sanitize();
        assert_eq!(hostile.state, None);
        assert_eq!(
            decode_hex_bytes(&encode_hex_bytes(b"\x01\xfe")),
            Some(vec![0x01, 0xfe])
        );

        // projeto salvo antes do host VST3: sem os campos `vst3`/`state`
        let old = serde_json::json!({
            "id": 5, "kind": "Gain", "enabled": true, "params": [{"value": 0.0, "keys": []}]
        });
        let f: FxInstance = serde_json::from_value(old).expect("formato antigo");
        assert!(f.vst3.is_none());
        let no_state: Vst3FxRef = serde_json::from_value(serde_json::json!({
            "binary": "a.vst3",
            "class_id": "00",
        }))
        .expect("vst3 sem campo state");
        assert_eq!(no_state.state, None);
        // e um FX sem VST3 não grava o campo
        let out = serde_json::to_value(FxInstance::new(FxKind::Gain)).expect("grava");
        assert!(out.get("vst3").is_none());
    }

    #[test]
    fn time_roundtrip() {
        let t = Ticks::from_millis_i64(3_723_500);
        let s = format_ticks(t, FrameRate::FPS_30, 48000, TimeFormat::SmpteNonDrop);
        assert_eq!(s, "01:02:03:15");
        assert_eq!(
            parse_time(&s, FrameRate::FPS_30, 48000, TimeFormat::SmpteNonDrop).unwrap(),
            t
        );
        assert_eq!(
            parse_time("90f", FrameRate::FPS_30, 48000, TimeFormat::SmpteNonDrop).unwrap(),
            Ticks::from_secs_i64(3)
        );
    }

    #[test]
    fn envelope_eval() {
        let mut e = Envelope::new(EnvKind::Opacity);
        e.points.push(EnvPoint {
            t: Ticks::from_secs_i64(2),
            v: 0.0,
            curve: Curve::Linear,
        });
        assert!((e.eval(Ticks::from_secs_i64(1)) - 0.5).abs() < 1e-6);
        assert_eq!(e.eval(Ticks::from_secs_i64(5)), 0.0);
    }

    #[test]
    fn media_time_loop_reverse() {
        let m = MediaItem {
            id: 1,
            name: "x".into(),
            source: MediaSource::File { path: "x".into() },
            duration: Some(Ticks::from_secs_i64(4)),
            has_video: true,
            has_audio: false,
            width: 0,
            height: 0,
            fps: Some(FrameRate::FPS_30),
            info: String::new(),
            proxy_path: None,
        };
        let dur = Some(Ticks::from_secs_i64(4));
        let mut e = Event::new(&m, Ticks::from_secs_i64(10), Ticks::from_secs_i64(6));
        assert_eq!(
            e.media_time(Ticks::from_secs_i64(15), dur),
            Some(Ticks::from_secs_i64(1))
        );
        e.looped = false;
        assert!(e.media_time(Ticks::from_secs_i64(15), dur).is_none());
        e.reverse = true;
        e.length = Ticks::from_secs_i64(4);
        assert_eq!(
            e.media_time(Ticks::from_secs_i64(11), dur),
            Some(Ticks::from_secs_i64(3))
        );
    }

    #[test]
    fn audio_buses_limite_26_e_saneamento_de_ciclos() {
        let mut p = Project::new_default();
        for _ in 0..30 {
            let _ = p.add_audio_bus(None);
        }
        assert_eq!(p.audio_buses.len(), MAX_AUDIO_BUSES);
        assert_eq!(p.audio_buses[0].name, "Bus A");
        assert_eq!(p.audio_buses[25].name, "Bus Z");

        let a = p.audio_buses[0].id;
        let b = p.audio_buses[1].id;
        // Roteamento válido A -> B -> Master
        assert!(p.validate_bus_route(a, Some(b)));
        p.audio_buses[0].output_bus = Some(b);
        // Ciclo B -> A precisa ser rejeitado na validação e quebrado por sanitize()
        assert!(!p.validate_bus_route(b, Some(a)));
        assert!(!p.validate_bus_route(a, Some(a)));

        p.audio_buses[1].output_bus = Some(a);
        let fixes = p.sanitize();
        assert!(!fixes.is_empty());
        assert!(p.validate_bus_route(a, p.audio_buses[0].output_bus));
        assert!(p.validate_bus_route(b, p.audio_buses[1].output_bus));

        let order = p.topological_bus_order();
        assert_eq!(order.len(), MAX_AUDIO_BUSES);
    }

    #[test]
    fn rotacao_de_backups_do_projeto_ao_salvar() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let tmp_dir = std::env::temp_dir().join(format!("neoveg_test_bak_{nanos}"));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let path = tmp_dir.join("meu_projeto.neoveg");

        let mut p = Project::new_default();
        p.notes = "v1".to_string();
        p.save_file_with_backup(&path, true, 3).expect("salva v1");
        assert!(path.exists());
        assert!(!backup_path_for(&path, 0).exists());

        // Salva v2: v1 vai para .bak
        p.notes = "v2".to_string();
        p.save_file_with_backup(&path, true, 3).expect("salva v2");
        assert!(path.exists());
        assert!(backup_path_for(&path, 0).exists());
        let (bak0, _) = Project::load_file(&backup_path_for(&path, 0)).expect("abre bak0");
        assert_eq!(bak0.notes, "v1");
        let (cur, _) = Project::load_file(&path).expect("abre cur");
        assert_eq!(cur.notes, "v2");

        // Salva v3: v2 vai para .bak, v1 para .bak1
        p.notes = "v3".to_string();
        p.save_file_with_backup(&path, true, 3).expect("salva v3");
        assert!(backup_path_for(&path, 1).exists());
        let (bak0, _) = Project::load_file(&backup_path_for(&path, 0)).expect("abre bak0");
        assert_eq!(bak0.notes, "v2");
        let (bak1, _) = Project::load_file(&backup_path_for(&path, 1)).expect("abre bak1");
        assert_eq!(bak1.notes, "v1");

        // Salva v4: v3 -> .bak, v2 -> .bak1, v1 -> .bak2
        p.notes = "v4".to_string();
        p.save_file_with_backup(&path, true, 3).expect("salva v4");
        assert!(backup_path_for(&path, 2).exists());
        let (bak2, _) = Project::load_file(&backup_path_for(&path, 2)).expect("abre bak2");
        assert_eq!(bak2.notes, "v1");

        // Salva v5: rotação mantém no máximo 3 cópias (.bak, .bak1, .bak2)
        p.notes = "v5".to_string();
        p.save_file_with_backup(&path, true, 3).expect("salva v5");
        assert!(!backup_path_for(&path, 3).exists());
        let (bak0, _) = Project::load_file(&backup_path_for(&path, 0)).expect("abre bak0");
        assert_eq!(bak0.notes, "v4");
        let (bak2, _) = Project::load_file(&backup_path_for(&path, 2)).expect("abre bak2");
        assert_eq!(bak2.notes, "v2");

        // Salva sem backup ativado
        p.notes = "v6".to_string();
        p.save_file_with_backup(&path, false, 3)
            .expect("salva v6 sem backup");
        let (bak0_unchanged, _) =
            Project::load_file(&backup_path_for(&path, 0)).expect("abre bak0");
        assert_eq!(bak0_unchanged.notes, "v4"); // mantido
        let (cur, _) = Project::load_file(&path).expect("abre cur");
        assert_eq!(cur.notes, "v6");

        // Entrada hostil de caminho não entra em pânico
        let hostile = std::path::Path::new("");
        assert!(p.save_file_with_backup(hostile, true, 3).is_err());

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn sequencias_multiplas_e_gerenciamento() {
        let mut p = Project::new_default();
        assert_eq!(p.sequences.len(), 1);
        assert!(p.active_sequence_id.is_some());
        let seq1_id = p.active_sequence_id.unwrap();
        assert_eq!(p.sequences[0].name, "Sequência 1");

        // Cria nova sequência
        let seq2_id = p.create_sequence(Some("Corte Diretor"));
        assert_eq!(p.sequences.len(), 2);
        assert_eq!(p.active_sequence_id, Some(seq2_id));
        assert_eq!(p.sequences[1].name, "Corte Diretor");

        // Altera nome da sequência 2
        assert!(p.rename_sequence(seq2_id, "Corte Final"));
        assert_eq!(p.sequences[1].name, "Corte Final");

        // Duplica sequência 2
        let seq3_id = p.duplicate_sequence(seq2_id).expect("duplica sequencia");
        assert_eq!(p.sequences.len(), 3);
        assert_eq!(p.sequences[2].name, "Corte Final (Cópia)");

        // Alterna de volta para sequência 1
        assert!(p.switch_sequence(seq1_id));
        assert_eq!(p.active_sequence_id, Some(seq1_id));

        // Tenta alternar para ID inexistente
        assert!(!p.switch_sequence(999999));

        // Exclui sequência 3
        assert!(p.delete_sequence(seq3_id));
        assert_eq!(p.sequences.len(), 2);

        // Exclui sequência ativa (seq1): ativa deve mudar para a primeira restante
        assert!(p.delete_sequence(seq1_id));
        assert_eq!(p.sequences.len(), 1);
        assert_eq!(p.active_sequence_id, Some(seq2_id));

        // Tenta excluir última sequência: deve recusar para nunca deixar projeto sem sequência
        assert!(!p.delete_sequence(seq2_id));
        assert_eq!(p.sequences.len(), 1);
    }
}
