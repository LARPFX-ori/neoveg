//! Preferências do usuário, matriz de prévia e limites de hardware (§3.2 / §3.6).

use crate::time::TimeFormat;
use crate::util;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Nível de qualidade/resolução de pré-visualização da Prévia de Vídeo (§3.6).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum PreviewQuality {
    #[serde(alias = "DraftQuarter", alias = "DraftHalf", alias = "DraftFull")]
    Draft,
    #[default]
    #[serde(alias = "PreviewQuarter", alias = "PreviewHalf", alias = "PreviewFull")]
    Preview,
    #[serde(alias = "GoodQuarter", alias = "GoodHalf", alias = "GoodFull")]
    Good,
    #[serde(alias = "BestQuarter", alias = "BestHalf", alias = "BestFull")]
    Best,
}

impl PreviewQuality {
    pub const ALL: [PreviewQuality; 4] = [
        PreviewQuality::Draft,
        PreviewQuality::Preview,
        PreviewQuality::Good,
        PreviewQuality::Best,
    ];

    pub fn name(self) -> &'static str {
        match self {
            PreviewQuality::Draft => "Rascunho",
            PreviewQuality::Preview => "Prévia",
            PreviewQuality::Good => "Boa",
            PreviewQuality::Best => "Melhor",
        }
    }

    pub fn scale(self) -> f32 {
        match self {
            PreviewQuality::Draft => 0.25,
            PreviewQuality::Preview => 0.5,
            PreviewQuality::Good => 1.0,
            PreviewQuality::Best => 1.0,
        }
    }

    pub fn prefer_proxy(self) -> bool {
        matches!(self, PreviewQuality::Draft | PreviewQuality::Preview)
    }
}

/// Tipos clássicos de layout de interface inspirados nas três eras do VEGAS Pro (§2.2).
///
/// Cada variante reflete a organização estrutural descrita nos manuais oficiais:
/// - **Sonic Foundry** (Vegas Pro 1.0–4.0): timeline no topo, transport acima do dock,
///   abas na base do dock e estética clara clássica.
/// - **Sony** (Sony Vegas Pro 8–13): área de acoplamento no topo (*Display timeline at
///   bottom*), timeline embaixo, transport no rodapé, abas na base e cinza médio Sony.
/// - **MAGIX** (VEGAS Pro 14–21): área acoplável no topo com 3 colunas (Painéis,
///   Prévia de Vídeo e Master Bus) e abas na base, Visor de tempo e Track List abaixo,
///   timeline com Status de renderização, barra de transporte/edição no rodapé e tema escuro.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LayoutType {
    #[serde(
        alias = "sonic_foundry",
        alias = "sonic-foundry",
        alias = "Sonic Foundry",
        alias = "Sonic Fonudry"
    )]
    SonicFoundry,
    #[serde(alias = "sony", alias = "Sony")]
    Sony,
    #[default]
    #[serde(alias = "magix", alias = "MAGIX")]
    Magix,
}

impl LayoutType {
    pub const ALL: [LayoutType; 3] = [
        LayoutType::SonicFoundry,
        LayoutType::Sony,
        LayoutType::Magix,
    ];

    pub fn id(self) -> &'static str {
        match self {
            LayoutType::SonicFoundry => "sonic_foundry",
            LayoutType::Sony => "sony",
            LayoutType::Magix => "magix",
        }
    }

    /// Nome mostrado na interface. Sem nomes de produtos ou empresas de
    /// terceiros (AGENTS.md §1): o `id` e os aliases de serde continuam os
    /// mesmos, então preferências e atalhos já salvos não mudam.
    pub fn label(self) -> &'static str {
        match self {
            LayoutType::SonicFoundry => "Clássico",
            LayoutType::Sony => "Intermediário",
            LayoutType::Magix => "Atual",
        }
    }

    /// Resumo curto da disposição, mostrado junto do nome.
    pub fn era(self) -> &'static str {
        match self {
            LayoutType::SonicFoundry => "timeline no topo, tema claro",
            LayoutType::Sony => "área acoplável no topo, cinza médio",
            LayoutType::Magix => "3 painéis no topo, tema escuro",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            LayoutType::SonicFoundry => {
                "Timeline (Track View) no topo, barra de transporte logo abaixo da timeline, \
                 área acoplável na parte inferior com abas na base e tema claro clássico."
            }
            LayoutType::Sony => {
                "Área acoplável (Prévia e painéis) no topo, timeline na parte inferior da janela, \
                 barra de transporte no rodapé, abas na base e cinza médio."
            }
            LayoutType::Magix => {
                "Área acoplável no topo em 3 painéis (Abas, Prévia e Master Bus) com abas na base, \
                 timeline na metade inferior, barra de transporte/edição no rodapé e tema escuro."
            }
        }
    }

    pub fn from_pref(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "sonic_foundry" | "sonic-foundry" | "sonic foundry" | "sonic fonudry"
            | "sonicfoundry" | "clássico" | "classico" => LayoutType::SonicFoundry,
            "sony" | "intermediário" | "intermediario" => LayoutType::Sony,
            _ => LayoutType::Magix,
        }
    }

    pub fn default_theme_id(self) -> &'static str {
        match self {
            LayoutType::SonicFoundry => "vegas-claro",
            LayoutType::Sony => "vegas-medio",
            LayoutType::Magix => "vegas-escuro",
        }
    }

    pub fn default_dock_height(self) -> f32 {
        match self {
            LayoutType::SonicFoundry => 270.0,
            LayoutType::Sony => 300.0,
            LayoutType::Magix => 320.0,
        }
    }

    pub fn default_preview_ratio(self) -> f32 {
        match self {
            LayoutType::SonicFoundry => 0.36,
            LayoutType::Sony => 0.42,
            LayoutType::Magix => 0.40,
        }
    }
}

/// Opções de disposição da área de trabalho configuradas em `Editar > Propriedades > Layout`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditLayoutPrefs {
    pub layout_type: LayoutType,
    /// Exibe a área acoplável no topo e a timeline na metade inferior (clássico Sony).
    pub timeline_at_bottom: bool,
    /// Posiciona as abas das janelas acopladas na borda superior em vez da base.
    pub tabs_at_top: bool,
    /// Fixa a barra de transporte no rodapé da janela principal (abaixo da área inferior).
    pub transport_at_bottom: bool,
    /// Exibe a janela de prévia à esquerda na área acoplável.
    pub preview_on_left: bool,
    /// Exibe o painel Master Bus acoplado à direita na área de janelas (padrão VEGAS Pro 21).
    pub master_bus_docked: bool,
    /// Aplica automaticamente a paleta visual da era ao escolher um tipo de layout
    /// (preservando temas de Alto Contraste AAA quando ativos).
    pub sync_theme_with_layout: bool,
}

impl EditLayoutPrefs {
    pub fn for_type(layout_type: LayoutType) -> Self {
        match layout_type {
            LayoutType::SonicFoundry => Self {
                layout_type,
                timeline_at_bottom: false,
                tabs_at_top: false,
                transport_at_bottom: false,
                preview_on_left: false,
                master_bus_docked: false,
                sync_theme_with_layout: true,
            },
            LayoutType::Sony => Self {
                layout_type,
                timeline_at_bottom: true,
                tabs_at_top: false,
                transport_at_bottom: true,
                preview_on_left: false,
                master_bus_docked: false,
                sync_theme_with_layout: true,
            },
            LayoutType::Magix => Self {
                layout_type,
                timeline_at_bottom: true,
                tabs_at_top: false,
                transport_at_bottom: true,
                preview_on_left: false,
                master_bus_docked: true,
                sync_theme_with_layout: true,
            },
        }
    }

    pub fn apply_type(&mut self, layout_type: LayoutType) {
        let sync = self.sync_theme_with_layout;
        *self = Self::for_type(layout_type);
        self.sync_theme_with_layout = sync;
    }
}

impl Default for EditLayoutPrefs {
    fn default() -> Self {
        Self::for_type(LayoutType::Magix)
    }
}

/// Modo de rolagem automática da timeline durante a reprodução.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackScrollMode {
    #[default]
    Page,
    Continuous,
}

impl PlaybackScrollMode {
    pub const ALL: [PlaybackScrollMode; 2] =
        [PlaybackScrollMode::Page, PlaybackScrollMode::Continuous];

    pub fn label(self) -> &'static str {
        match self {
            PlaybackScrollMode::Page => "Por página (Page Scroll)",
            PlaybackScrollMode::Continuous => "Contínua (playhead centralizado)",
        }
    }
}

/// Comportamento padrão da roda do mouse sobre a área de faixas da timeline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimelineMouseScroll {
    #[default]
    ZoomDefault,
    HorizontalScroll,
}

impl TimelineMouseScroll {
    pub const ALL: [TimelineMouseScroll; 2] = [
        TimelineMouseScroll::ZoomDefault,
        TimelineMouseScroll::HorizontalScroll,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TimelineMouseScroll::ZoomDefault => "Zoom (Shift = rolar horizontal • Ctrl = vertical)",
            TimelineMouseScroll::HorizontalScroll => {
                "Rolar horizontal (Ctrl = zoom • Shift = vertical)"
            }
        }
    }
}

/// Botões visíveis no cabeçalho de um tipo de faixa: botão Mais >
/// *Editar conjunto de botões visíveis* (manual do VEGAS Pro 21, p. 69).
/// Os ocultos continuam no menu do botão Mais.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HeaderButtons {
    pub mute: bool,
    pub solo: bool,
    pub fx: bool,
    /// Só em faixas de vídeo.
    pub track_motion: bool,
}

impl HeaderButtons {
    /// Todos visíveis (`true`) ou todos ocultos (`false`).
    pub fn all(on: bool) -> Self {
        Self {
            mute: on,
            solo: on,
            fx: on,
            track_motion: on,
        }
    }
}

impl Default for HeaderButtons {
    fn default() -> Self {
        Self::all(true)
    }
}

/// Conjuntos de botões visíveis dos cabeçalhos, um para vídeo e um para
/// áudio, lembrados entre as sessões.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrackHeaderPrefs {
    pub video: HeaderButtons,
    pub audio: HeaderButtons,
}

impl TrackHeaderPrefs {
    pub fn get(&self, video: bool) -> HeaderButtons {
        if video {
            self.video
        } else {
            self.audio
        }
    }

    pub fn set(&mut self, video: bool, b: HeaderButtons) {
        if video {
            self.video = b;
        } else {
            self.audio = b;
        }
    }
}

fn default_rpc_timeout_secs() -> u64 {
    60
}

fn sanitize_exe_path(path: &mut String) {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.len() > 1024 || trimmed.contains('\0') {
        path.clear();
    } else if trimmed.len() != path.len() {
        *path = trimmed.to_string();
    }
}

/// Caminhos manuais e opções de integração com a suíte ArtCraft e o Blender (§0.4).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExternalAppsPrefs {
    #[serde(default)]
    pub photocraft_path: String,
    #[serde(default)]
    pub vectorcraft_path: String,
    #[serde(default)]
    pub filmcraft_path: String,
    #[serde(default)]
    pub effectcraft_path: String,
    #[serde(default)]
    pub anicraft_path: String,
    #[serde(default)]
    pub soundcraft_path: String,
    #[serde(default)]
    pub blender_path: String,
    /// Tempo limite de espera em segundos para RPC / CLI headless (`5..=300`).
    #[serde(default = "default_rpc_timeout_secs")]
    pub rpc_timeout_secs: u64,
    /// Recarregar automaticamente cópias editadas externamente quando modificadas no disco.
    #[serde(default = "default_true")]
    pub watch_edited_copies: bool,
}

impl Default for ExternalAppsPrefs {
    fn default() -> Self {
        Self {
            photocraft_path: String::new(),
            vectorcraft_path: String::new(),
            filmcraft_path: String::new(),
            effectcraft_path: String::new(),
            anicraft_path: String::new(),
            soundcraft_path: String::new(),
            blender_path: String::new(),
            rpc_timeout_secs: default_rpc_timeout_secs(),
            watch_edited_copies: true,
        }
    }
}

impl ExternalAppsPrefs {
    pub fn path_for(&self, id: &str) -> &str {
        match id {
            "photocraft" => &self.photocraft_path,
            "vectorcraft" => &self.vectorcraft_path,
            "filmcraft" => &self.filmcraft_path,
            "effectcraft" => &self.effectcraft_path,
            "anicraft" => &self.anicraft_path,
            "soundcraft" => &self.soundcraft_path,
            "blender" => &self.blender_path,
            _ => "",
        }
    }

    pub fn path_mut(&mut self, id: &str) -> Option<&mut String> {
        match id {
            "photocraft" => Some(&mut self.photocraft_path),
            "vectorcraft" => Some(&mut self.vectorcraft_path),
            "filmcraft" => Some(&mut self.filmcraft_path),
            "effectcraft" => Some(&mut self.effectcraft_path),
            "anicraft" => Some(&mut self.anicraft_path),
            "soundcraft" => Some(&mut self.soundcraft_path),
            "blender" => Some(&mut self.blender_path),
            _ => None,
        }
    }

    pub fn sanitize(&mut self) {
        sanitize_exe_path(&mut self.photocraft_path);
        sanitize_exe_path(&mut self.vectorcraft_path);
        sanitize_exe_path(&mut self.filmcraft_path);
        sanitize_exe_path(&mut self.effectcraft_path);
        sanitize_exe_path(&mut self.anicraft_path);
        sanitize_exe_path(&mut self.soundcraft_path);
        sanitize_exe_path(&mut self.blender_path);
        self.rpc_timeout_secs = self.rpc_timeout_secs.clamp(5, 300);
    }
}

/// Preferências persistidas em JSON com proteção contra entradas hostis (§0.4 / §3.6).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default = "default_lang")]
    pub language: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_ram_cache_mb")]
    pub ram_preview_mb: u32,
    #[serde(default = "default_frame_cache")]
    pub frame_cache_cap: usize,
    /// Número de threads de trabalho (`0` = automático proporcional ao hardware).
    #[serde(default)]
    pub worker_threads: usize,
    #[serde(default)]
    pub shortcuts: HashMap<String, Option<String>>,
    /// Caminho opcional do último layout `.neoveglayout` carregado.
    #[serde(default)]
    pub window_layout: Option<String>,
    /// Tipo e disposição clássica de layout configurados em `Editar > Propriedades > Layout`.
    #[serde(default)]
    pub edit_layout: EditLayoutPrefs,
    /// Botões visíveis nos cabeçalhos de faixa (botão Mais).
    #[serde(default)]
    pub track_header: TrackHeaderPrefs,
    /// Exibe a tela de abertura (Splash Screen) durante a inicialização gráfica.
    #[serde(default = "default_show_splash")]
    pub show_splash: bool,
    /// Exibe a tela de boas-vindas com ações rápidas e projetos recentes ao iniciar.
    #[serde(default = "default_show_welcome")]
    pub show_welcome: bool,
    /// Caminhos dos projetos recentes (mais recente primeiro, máx. 10).
    #[serde(default)]
    pub recent_projects: Vec<String>,
    /// Identificadores de modelos favoritados na janela `Renderizar como...`.
    #[serde(default)]
    pub render_favorites: Vec<String>,
    /// Efeitos de vídeo marcados com a estrela na janela Efeitos de Vídeo.
    #[serde(default)]
    pub fx_favorites: Vec<String>,
    /// Tipos de gerador marcados com a estrela na janela Geradores de Mídia.
    #[serde(default)]
    pub gen_favorites: Vec<String>,
    /// Quantidade máxima de projetos recentes mantidos no histórico (`1..=20`).
    #[serde(default = "default_max_recent_projects")]
    pub max_recent_projects: usize,
    /// Ativa o salvamento automático periódico para recuperação após falhas.
    #[serde(default = "default_true")]
    pub autosave_enabled: bool,
    /// Intervalo do salvamento automático em segundos (`15..=600`).
    #[serde(default = "default_autosave_secs")]
    pub autosave_secs: u64,
    /// Duração padrão ao inserir imagem estática ou mídia gerada na timeline (`0.5..=60.0 s`).
    #[serde(default = "default_still_image_secs")]
    pub still_image_secs: f32,
    /// Modo de rolagem automática da timeline durante a reprodução.
    #[serde(default)]
    pub playback_scroll: PlaybackScrollMode,
    /// Ação padrão da roda do mouse sobre as faixas da timeline.
    #[serde(default)]
    pub timeline_mouse_scroll: TimelineMouseScroll,
    /// Desenha sempre as linhas pontilhadas dos marcadores mesmo com Encaixe desativado.
    #[serde(default = "default_true")]
    pub always_draw_marker_lines: bool,
    /// Exibe dicas flutuantes (tooltips) nos botões e controles da interface.
    #[serde(default = "default_true")]
    pub show_tooltips: bool,
    /// Ativa o encaixe magnético (Snapping) ao iniciar ou criar novo projeto.
    #[serde(default = "default_true")]
    pub default_snapping: bool,
    /// Quantiza edições nos limites exatos de quadros ao iniciar ou criar novo projeto.
    #[serde(default = "default_true")]
    pub default_quantize: bool,
    /// Exibe formas de onda nos eventos de áudio por padrão.
    #[serde(default = "default_true")]
    pub default_show_waveforms: bool,
    /// Exibe miniaturas de vídeo nos eventos por padrão.
    #[serde(default = "default_true")]
    pub default_show_thumbs: bool,
    /// Exibe curvas de envelope nas trilhas por padrão.
    #[serde(default = "default_true")]
    pub default_show_envelopes: bool,
    /// Ativa reprodução contínua em loop por padrão ao iniciar.
    #[serde(default)]
    pub default_loop_playback: bool,
    /// Qualidade inicial da Prévia de Vídeo (*Rascunho / Prévia / Boa / Melhor*).
    #[serde(default)]
    pub default_preview_quality: PreviewQuality,
    /// Formato inicial padrão da régua e do Visor de Tempo.
    #[serde(default = "default_time_format")]
    pub default_time_format: TimeFormat,
    /// Taxa de amostragem padrão (Hz) ao criar um novo projeto (`44100`, `48000` ou `96000`).
    #[serde(default = "default_sample_rate")]
    pub default_sample_rate: u32,
    /// Percentual da borda de área segura de ação (`70..=98`).
    #[serde(default = "default_action_safe_pct")]
    pub action_safe_pct: u8,
    /// Percentual da borda de área segura de títulos (`50..=95`, <= `action_safe_pct`).
    #[serde(default = "default_title_safe_pct")]
    pub title_safe_pct: u8,
    /// Integração com aplicativos ArtCraft (PhotoCraft, VectorCraft, FilmCraft, EffectCraft, AniCraft, SoundCraft) e Blender.
    #[serde(default)]
    pub external_apps: ExternalAppsPrefs,
    /// Propriedades padrão de novas trilhas (`Definir propriedades padrão da trilha`, p. 414).
    #[serde(default)]
    pub default_track: DefaultTrackPrefs,
}

/// Propriedades padrão herdadas por novas trilhas de vídeo ou áudio.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DefaultTrackPrefs {
    pub video_height: f32,
    pub video_opacity: f32,
    pub video_blend: String,
    pub audio_height: f32,
    pub audio_volume_db: f32,
    pub audio_pan: f32,
    pub mute: bool,
    pub solo: bool,
}

impl Default for DefaultTrackPrefs {
    fn default() -> Self {
        Self {
            video_height: 72.0,
            video_opacity: 1.0,
            video_blend: "SourceAlpha".to_string(),
            audio_height: 72.0,
            audio_volume_db: 0.0,
            audio_pan: 0.0,
            mute: false,
            solo: false,
        }
    }
}

impl DefaultTrackPrefs {
    pub fn sanitize(&mut self) {
        let fin = |v: f32, d: f32| if v.is_finite() { v } else { d };
        self.video_height = fin(self.video_height, 72.0).clamp(28.0, 400.0);
        self.audio_height = fin(self.audio_height, 72.0).clamp(28.0, 400.0);
        self.video_opacity = fin(self.video_opacity, 1.0).clamp(0.0, 1.0);
        self.audio_volume_db = fin(self.audio_volume_db, 0.0).clamp(-120.0, 24.0);
        self.audio_pan = fin(self.audio_pan, 0.0).clamp(-1.0, 1.0);
        if self.video_blend.trim().is_empty() || self.video_blend.len() > 32 {
            self.video_blend = "SourceAlpha".to_string();
        }
    }
}

const MAX_RECENT_PROJECTS: usize = 20;
const MAX_RENDER_FAVORITES: usize = 64;

fn default_lang() -> String {
    "pt-BR".to_string()
}

fn default_theme() -> String {
    "vegas-escuro".to_string()
}

fn default_ram_cache_mb() -> u32 {
    256
}

fn default_frame_cache() -> usize {
    48
}

fn default_show_splash() -> bool {
    true
}

fn default_show_welcome() -> bool {
    true
}

fn default_true() -> bool {
    true
}

fn default_max_recent_projects() -> usize {
    10
}

fn default_autosave_secs() -> u64 {
    60
}

fn default_still_image_secs() -> f32 {
    5.0
}

fn default_time_format() -> TimeFormat {
    TimeFormat::SmpteDrop
}

fn default_sample_rate() -> u32 {
    48000
}

fn default_action_safe_pct() -> u8 {
    90
}

fn default_title_safe_pct() -> u8 {
    80
}

fn sanitize_string_list(
    list: &mut Vec<String>,
    max_items: usize,
    max_len: usize,
    ignore_case: bool,
) {
    let mut cleaned: Vec<String> = Vec::with_capacity(list.len().min(max_items));
    for raw in list.drain(..) {
        let s = raw.trim();
        if s.is_empty() || s.len() > max_len || s.contains('\0') {
            continue;
        }
        let dup = cleaned.iter().any(|existing| {
            if ignore_case {
                existing.eq_ignore_ascii_case(s)
            } else {
                existing == s
            }
        });
        if !dup {
            cleaned.push(s.to_string());
            if cleaned.len() >= max_items {
                break;
            }
        }
    }
    *list = cleaned;
}

/// Número padrão de workers proporcionais aos núcleos disponíveis (§3.6),
/// reservando fôlego para a interface gráfica.
pub fn recommended_worker_threads() -> usize {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2);
    if cores <= 2 {
        1
    } else {
        (cores - 1).min(8)
    }
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: default_lang(),
            theme: default_theme(),
            ram_preview_mb: default_ram_cache_mb(),
            frame_cache_cap: default_frame_cache(),
            worker_threads: 0,
            shortcuts: HashMap::new(),
            window_layout: None,
            edit_layout: EditLayoutPrefs::default(),
            track_header: TrackHeaderPrefs::default(),
            show_splash: default_show_splash(),
            show_welcome: default_show_welcome(),
            recent_projects: Vec::new(),
            render_favorites: Vec::new(),
            fx_favorites: Vec::new(),
            gen_favorites: Vec::new(),
            max_recent_projects: default_max_recent_projects(),
            autosave_enabled: true,
            autosave_secs: default_autosave_secs(),
            still_image_secs: default_still_image_secs(),
            playback_scroll: PlaybackScrollMode::Page,
            timeline_mouse_scroll: TimelineMouseScroll::ZoomDefault,
            always_draw_marker_lines: true,
            show_tooltips: true,
            default_snapping: true,
            default_quantize: true,
            default_show_waveforms: true,
            default_show_thumbs: true,
            default_show_envelopes: true,
            default_loop_playback: false,
            default_preview_quality: PreviewQuality::Preview,
            default_time_format: default_time_format(),
            default_sample_rate: default_sample_rate(),
            action_safe_pct: default_action_safe_pct(),
            title_safe_pct: default_title_safe_pct(),
            external_apps: ExternalAppsPrefs::default(),
            default_track: DefaultTrackPrefs::default(),
        }
    }
}

impl Preferences {
    pub fn file_path() -> PathBuf {
        util::base_dir().join("preferencias.json")
    }

    pub fn layouts_dir() -> PathBuf {
        util::base_dir().join("layouts")
    }

    pub fn effective_worker_threads(&self) -> usize {
        if self.worker_threads == 0 {
            recommended_worker_threads()
        } else {
            self.worker_threads.clamp(1, 16)
        }
    }

    /// Registra um projeto recente no topo da lista (deduplicado, máx. configurável).
    pub fn record_recent_project(&mut self, path: &std::path::Path) {
        let raw = path.to_string_lossy();
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.len() > 1024 || trimmed.contains('\0') {
            return;
        }
        let cap = self.max_recent_projects.clamp(1, MAX_RECENT_PROJECTS);
        self.recent_projects
            .retain(|p| !p.eq_ignore_ascii_case(trimmed));
        self.recent_projects.insert(0, trimmed.to_string());
        self.recent_projects.truncate(cap);
    }

    pub fn remove_recent_project(&mut self, raw: &str) {
        self.recent_projects
            .retain(|p| !p.eq_ignore_ascii_case(raw));
    }

    pub fn clear_recent_projects(&mut self) {
        self.recent_projects.clear();
    }

    pub fn sanitize(&mut self) {
        if self.language.trim().is_empty() || self.language.len() > 16 {
            self.language = default_lang();
        }
        if self.theme.trim().is_empty() || self.theme.len() > 48 {
            self.theme = default_theme();
        }
        self.ram_preview_mb = self.ram_preview_mb.clamp(32, 8192);
        self.frame_cache_cap = self.frame_cache_cap.clamp(4, 512);
        self.worker_threads = self.worker_threads.min(16);
        self.max_recent_projects = self.max_recent_projects.clamp(1, MAX_RECENT_PROJECTS);
        self.autosave_secs = self.autosave_secs.clamp(15, 600);
        if !self.still_image_secs.is_finite() {
            self.still_image_secs = default_still_image_secs();
        }
        self.still_image_secs = self.still_image_secs.clamp(0.5, 60.0);
        if !matches!(self.default_sample_rate, 44100 | 48000 | 96000) {
            self.default_sample_rate = default_sample_rate();
        }
        self.action_safe_pct = self.action_safe_pct.clamp(70, 98);
        self.title_safe_pct = self.title_safe_pct.clamp(50, self.action_safe_pct);
        self.shortcuts.retain(|k, _| !k.is_empty() && k.len() <= 80);
        if self
            .window_layout
            .as_ref()
            .is_some_and(|s| s.trim().is_empty() || s.len() > 260)
        {
            self.window_layout = None;
        }
        sanitize_string_list(
            &mut self.recent_projects,
            self.max_recent_projects,
            1024,
            cfg!(windows),
        );
        sanitize_string_list(&mut self.render_favorites, MAX_RENDER_FAVORITES, 80, false);
        sanitize_string_list(&mut self.fx_favorites, MAX_RENDER_FAVORITES, 80, false);
        sanitize_string_list(&mut self.gen_favorites, MAX_RENDER_FAVORITES, 80, false);
        self.external_apps.sanitize();
        self.default_track.sanitize();
    }

    pub fn load_or_default() -> Self {
        let p = Self::file_path();
        let Ok(bytes) = std::fs::read(p) else {
            return Self::default();
        };
        let Ok(mut prefs) = serde_json::from_slice::<Preferences>(&bytes) else {
            return Self::default();
        };
        prefs.sanitize();
        prefs
    }

    pub fn save(&self) -> Result<(), String> {
        let mut copy = self.clone();
        copy.sanitize();
        let p = Self::file_path();
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(&copy).map_err(|e| e.to_string())?;
        let tmp = p.with_extension("tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferencias_hostis_sao_saneadas() {
        let raw = r#"{"language":"","theme":"x","ram_preview_mb":99999999,"frame_cache_cap":0,"worker_threads":9999}"#;
        let mut p: Preferences = serde_json::from_str(raw).unwrap();
        p.sanitize();
        assert_eq!(p.language, "pt-BR");
        assert_eq!(p.ram_preview_mb, 8192);
        assert_eq!(p.frame_cache_cap, 4);
        assert_eq!(p.worker_threads, 16);
        assert!(p.show_splash);
        assert!(p.show_welcome);

        let p_no_splash: Preferences =
            serde_json::from_str(r#"{"show_splash":false,"show_welcome":false}"#).unwrap();
        assert!(!p_no_splash.show_splash);
        assert!(!p_no_splash.show_welcome);

        let mut p_rec = Preferences {
            recent_projects: vec![
                "".to_string(),
                "a\0b.neoveg".to_string(),
                "proj1.neoveg".to_string(),
                "proj1.neoveg".to_string(),
            ],
            ..Default::default()
        };
        p_rec.sanitize();
        assert_eq!(p_rec.recent_projects, vec!["proj1.neoveg".to_string()]);
        for i in 0..15 {
            p_rec.record_recent_project(std::path::Path::new(&format!("p{i}.neoveg")));
        }
        assert_eq!(p_rec.recent_projects.len(), 10);
        assert_eq!(p_rec.recent_projects[0], "p14.neoveg");
        p_rec.remove_recent_project("p14.neoveg");
        assert_eq!(p_rec.recent_projects[0], "p13.neoveg");

        let mut p_ext = Preferences {
            max_recent_projects: 999,
            autosave_secs: 1,
            still_image_secs: f32::NAN,
            default_sample_rate: 12345,
            action_safe_pct: 72,
            title_safe_pct: 95,
            ..Default::default()
        };
        p_ext.sanitize();
        assert_eq!(p_ext.max_recent_projects, MAX_RECENT_PROJECTS);
        assert_eq!(p_ext.autosave_secs, 15);
        assert!((p_ext.still_image_secs - 5.0).abs() < f32::EPSILON);
        assert_eq!(p_ext.default_sample_rate, 48000);
        assert_eq!(p_ext.action_safe_pct, 72);
        assert_eq!(p_ext.title_safe_pct, 72);
    }

    #[test]
    fn resolucoes_de_previa_e_aliases_legados() {
        let names: Vec<&str> = PreviewQuality::ALL.iter().map(|q| q.name()).collect();
        assert_eq!(names, vec!["Rascunho", "Prévia", "Boa", "Melhor"]);
        assert_eq!(PreviewQuality::Draft.scale(), 0.25);
        assert_eq!(PreviewQuality::Preview.scale(), 0.5);
        assert_eq!(PreviewQuality::Good.scale(), 1.0);
        assert_eq!(PreviewQuality::Best.scale(), 1.0);
        assert!(PreviewQuality::Draft.prefer_proxy());
        assert!(PreviewQuality::Preview.prefer_proxy());
        assert!(!PreviewQuality::Good.prefer_proxy());
        assert!(!PreviewQuality::Best.prefer_proxy());

        for (raw, expected) in [
            ("DraftQuarter", PreviewQuality::Draft),
            ("DraftHalf", PreviewQuality::Draft),
            ("DraftFull", PreviewQuality::Draft),
            ("PreviewQuarter", PreviewQuality::Preview),
            ("PreviewHalf", PreviewQuality::Preview),
            ("PreviewFull", PreviewQuality::Preview),
            ("GoodQuarter", PreviewQuality::Good),
            ("GoodHalf", PreviewQuality::Good),
            ("GoodFull", PreviewQuality::Good),
            ("BestQuarter", PreviewQuality::Best),
            ("BestHalf", PreviewQuality::Best),
            ("BestFull", PreviewQuality::Best),
        ] {
            let json = format!(r#"{{"default_preview_quality":"{raw}"}}"#);
            let parsed: Preferences = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed.default_preview_quality, expected);
        }
    }

    #[test]
    fn botoes_do_cabecalho_tem_padrao_e_sobrevivem_a_ida_e_volta() {
        // preferências antigas, sem o campo: todos os botões visíveis
        let p: Preferences = serde_json::from_str("{}").unwrap();
        assert_eq!(p.track_header, TrackHeaderPrefs::default());
        assert!(p.track_header.get(true).solo && p.track_header.get(false).fx);
        // campo parcial: o que falta fica visível
        let p: Preferences =
            serde_json::from_str(r#"{"track_header":{"audio":{"mute":false}}}"#).unwrap();
        assert!(!p.track_header.audio.mute && p.track_header.audio.solo);
        assert_eq!(p.track_header.video, HeaderButtons::all(true));
        let mut p = Preferences::default();
        p.track_header.set(true, HeaderButtons::all(false));
        let back: Preferences = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back.track_header.get(true), HeaderButtons::all(false));
        assert_eq!(back.track_header.get(false), HeaderButtons::all(true));
    }

    #[test]
    fn tipos_de_layout_e_aliases_funcionam() {
        assert_eq!(LayoutType::ALL.len(), 3);
        assert_eq!(
            LayoutType::from_pref("Sonic Fonudry"),
            LayoutType::SonicFoundry
        );
        assert_eq!(
            LayoutType::from_pref("Sonic Foundry"),
            LayoutType::SonicFoundry
        );
        assert_eq!(LayoutType::from_pref("Sony"), LayoutType::Sony);
        assert_eq!(LayoutType::from_pref("MAGIX"), LayoutType::Magix);
        assert_eq!(LayoutType::from_pref("desconhecido"), LayoutType::Magix);
        // os nomes mostrados na interface também são aceitos
        for lt in LayoutType::ALL {
            assert_eq!(LayoutType::from_pref(lt.label()), lt);
            assert_eq!(LayoutType::from_pref(lt.id()), lt);
        }

        let sf = EditLayoutPrefs::for_type(LayoutType::SonicFoundry);
        assert!(!sf.timeline_at_bottom);
        assert!(!sf.tabs_at_top);
        assert!(!sf.transport_at_bottom);
        assert!(!sf.master_bus_docked);

        let sony = EditLayoutPrefs::for_type(LayoutType::Sony);
        assert!(sony.timeline_at_bottom);
        assert!(!sony.tabs_at_top);
        assert!(sony.transport_at_bottom);
        assert!(!sony.master_bus_docked);

        let magix = EditLayoutPrefs::for_type(LayoutType::Magix);
        assert!(magix.timeline_at_bottom);
        assert!(!magix.tabs_at_top);
        assert!(magix.transport_at_bottom);
        assert!(magix.master_bus_docked);

        let raw = r#"{"edit_layout":{"layout_type":"Sonic Fonudry"}}"#;
        let p: Preferences = serde_json::from_str(raw).unwrap();
        assert_eq!(p.edit_layout.layout_type, LayoutType::SonicFoundry);
    }

    #[test]
    fn external_apps_prefs_sanitizam_entradas_hostis() {
        let raw = r#"{"external_apps":{"photocraft_path":"  /usr/bin/photocraft  ","blender_path":"bad\u0000path","rpc_timeout_secs":9999}}"#;
        let mut p: Preferences = serde_json::from_str(raw).unwrap();
        p.sanitize();
        assert_eq!(
            p.external_apps.path_for("photocraft"),
            "/usr/bin/photocraft"
        );
        assert_eq!(p.external_apps.path_for("blender"), "");
        assert_eq!(p.external_apps.rpc_timeout_secs, 300);
        assert!(p.external_apps.watch_edited_copies);
    }
}
