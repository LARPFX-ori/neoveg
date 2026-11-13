//! Preferências do usuário, matriz de prévia e limites de hardware (§3.2 / §3.6).

use crate::util;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Nível de qualidade de pré-visualização (VEGAS Pro: Draft/Preview/Good/Best).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum PreviewTier {
    Draft,
    #[default]
    Preview,
    Good,
    Best,
}

impl PreviewTier {
    pub const ALL: [PreviewTier; 4] = [
        PreviewTier::Draft,
        PreviewTier::Preview,
        PreviewTier::Good,
        PreviewTier::Best,
    ];

    pub fn name(self) -> &'static str {
        match self {
            PreviewTier::Draft => "Rascunho",
            PreviewTier::Preview => "Prévia",
            PreviewTier::Good => "Boa",
            PreviewTier::Best => "Melhor",
        }
    }
}

/// Escala de resolução da pré-visualização (Full / Half / Quarter).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum PreviewResolution {
    Quarter,
    #[default]
    Half,
    Full,
}

impl PreviewResolution {
    pub const ALL: [PreviewResolution; 3] = [
        PreviewResolution::Quarter,
        PreviewResolution::Half,
        PreviewResolution::Full,
    ];

    pub fn name(self) -> &'static str {
        match self {
            PreviewResolution::Quarter => "1/4",
            PreviewResolution::Half => "1/2",
            PreviewResolution::Full => "Completa",
        }
    }

    pub fn scale(self) -> f32 {
        match self {
            PreviewResolution::Quarter => 0.25,
            PreviewResolution::Half => 0.5,
            PreviewResolution::Full => 1.0,
        }
    }
}

/// Matriz completa de qualidade de pré-visualização (*Draft/Preview/Good/Best*
/// × *Quarter/Half/Full*), conforme semântica do VEGAS Pro (§3.6).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum PreviewQuality {
    #[serde(alias = "Draft")]
    DraftQuarter,
    DraftHalf,
    DraftFull,
    PreviewQuarter,
    #[default]
    #[serde(alias = "Preview")]
    PreviewHalf,
    PreviewFull,
    GoodQuarter,
    GoodHalf,
    #[serde(alias = "Good")]
    GoodFull,
    BestQuarter,
    BestHalf,
    BestFull,
}

impl PreviewQuality {
    pub const ALL: [PreviewQuality; 12] = [
        PreviewQuality::DraftQuarter,
        PreviewQuality::DraftHalf,
        PreviewQuality::DraftFull,
        PreviewQuality::PreviewQuarter,
        PreviewQuality::PreviewHalf,
        PreviewQuality::PreviewFull,
        PreviewQuality::GoodQuarter,
        PreviewQuality::GoodHalf,
        PreviewQuality::GoodFull,
        PreviewQuality::BestQuarter,
        PreviewQuality::BestHalf,
        PreviewQuality::BestFull,
    ];

    pub fn from_parts(tier: PreviewTier, res: PreviewResolution) -> Self {
        match (tier, res) {
            (PreviewTier::Draft, PreviewResolution::Quarter) => PreviewQuality::DraftQuarter,
            (PreviewTier::Draft, PreviewResolution::Half) => PreviewQuality::DraftHalf,
            (PreviewTier::Draft, PreviewResolution::Full) => PreviewQuality::DraftFull,
            (PreviewTier::Preview, PreviewResolution::Quarter) => PreviewQuality::PreviewQuarter,
            (PreviewTier::Preview, PreviewResolution::Half) => PreviewQuality::PreviewHalf,
            (PreviewTier::Preview, PreviewResolution::Full) => PreviewQuality::PreviewFull,
            (PreviewTier::Good, PreviewResolution::Quarter) => PreviewQuality::GoodQuarter,
            (PreviewTier::Good, PreviewResolution::Half) => PreviewQuality::GoodHalf,
            (PreviewTier::Good, PreviewResolution::Full) => PreviewQuality::GoodFull,
            (PreviewTier::Best, PreviewResolution::Quarter) => PreviewQuality::BestQuarter,
            (PreviewTier::Best, PreviewResolution::Half) => PreviewQuality::BestHalf,
            (PreviewTier::Best, PreviewResolution::Full) => PreviewQuality::BestFull,
        }
    }

    pub fn tier(self) -> PreviewTier {
        match self {
            PreviewQuality::DraftQuarter
            | PreviewQuality::DraftHalf
            | PreviewQuality::DraftFull => PreviewTier::Draft,
            PreviewQuality::PreviewQuarter
            | PreviewQuality::PreviewHalf
            | PreviewQuality::PreviewFull => PreviewTier::Preview,
            PreviewQuality::GoodQuarter | PreviewQuality::GoodHalf | PreviewQuality::GoodFull => {
                PreviewTier::Good
            }
            PreviewQuality::BestQuarter | PreviewQuality::BestHalf | PreviewQuality::BestFull => {
                PreviewTier::Best
            }
        }
    }

    pub fn resolution(self) -> PreviewResolution {
        match self {
            PreviewQuality::DraftQuarter
            | PreviewQuality::PreviewQuarter
            | PreviewQuality::GoodQuarter
            | PreviewQuality::BestQuarter => PreviewResolution::Quarter,
            PreviewQuality::DraftHalf
            | PreviewQuality::PreviewHalf
            | PreviewQuality::GoodHalf
            | PreviewQuality::BestHalf => PreviewResolution::Half,
            PreviewQuality::DraftFull
            | PreviewQuality::PreviewFull
            | PreviewQuality::GoodFull
            | PreviewQuality::BestFull => PreviewResolution::Full,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            PreviewQuality::DraftQuarter => "Rascunho (1/4)",
            PreviewQuality::DraftHalf => "Rascunho (1/2)",
            PreviewQuality::DraftFull => "Rascunho (completa)",
            PreviewQuality::PreviewQuarter => "Prévia (1/4)",
            PreviewQuality::PreviewHalf => "Prévia (1/2)",
            PreviewQuality::PreviewFull => "Prévia (completa)",
            PreviewQuality::GoodQuarter => "Boa (1/4)",
            PreviewQuality::GoodHalf => "Boa (1/2)",
            PreviewQuality::GoodFull => "Boa (completa)",
            PreviewQuality::BestQuarter => "Melhor (1/4)",
            PreviewQuality::BestHalf => "Melhor (1/2)",
            PreviewQuality::BestFull => "Melhor (completa)",
        }
    }

    pub fn scale(self) -> f32 {
        self.resolution().scale()
    }

    pub fn prefer_proxy(self) -> bool {
        matches!(self.tier(), PreviewTier::Draft | PreviewTier::Preview)
    }
}

/// Tipos clássicos de layout de interface inspirados nas três eras do VEGAS Pro (§2.2).
///
/// Cada variante reflete a organização estrutural descrita nos manuais oficiais:
/// - **Sonic Foundry** (Vegas Pro 1.0–4.0): timeline no topo, transport acima do dock,
///   abas na base do dock e estética clara clássica.
/// - **Sony** (Sony Vegas Pro 8–13): área de acoplamento no topo (*Display timeline at
///   bottom*), timeline embaixo, transport no rodapé, abas na base e cinza médio Sony.
/// - **MAGIX** (VEGAS Pro 14–21): timeline no topo, barra unificada de transporte/edição,
///   abas no topo dos painéis acopláveis e interface escura moderna.
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

    pub fn label(self) -> &'static str {
        match self {
            LayoutType::SonicFoundry => "Sonic Foundry",
            LayoutType::Sony => "Sony",
            LayoutType::Magix => "MAGIX",
        }
    }

    pub fn era(self) -> &'static str {
        match self {
            LayoutType::SonicFoundry => "Vegas Pro 1.0–4.0",
            LayoutType::Sony => "Sony Vegas Pro 8–13",
            LayoutType::Magix => "MAGIX VEGAS Pro 14–21",
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
                 barra de transporte no rodapé, abas na base e cinza médio Sony."
            }
            LayoutType::Magix => {
                "Timeline no topo, barra unificada de transporte e edição, \
                 área acoplável embaixo com abas no topo e interface escura moderna."
            }
        }
    }

    pub fn from_pref(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "sonic_foundry" | "sonic-foundry" | "sonic foundry" | "sonic fonudry"
            | "sonicfoundry" => LayoutType::SonicFoundry,
            "sony" => LayoutType::Sony,
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
            LayoutType::Sony => 310.0,
            LayoutType::Magix => 330.0,
        }
    }

    pub fn default_preview_ratio(self) -> f32 {
        match self {
            LayoutType::SonicFoundry => 0.36,
            LayoutType::Sony => 0.42,
            LayoutType::Magix => 0.44,
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
                sync_theme_with_layout: true,
            },
            LayoutType::Sony => Self {
                layout_type,
                timeline_at_bottom: true,
                tabs_at_top: false,
                transport_at_bottom: true,
                preview_on_left: false,
                sync_theme_with_layout: true,
            },
            LayoutType::Magix => Self {
                layout_type,
                timeline_at_bottom: false,
                tabs_at_top: true,
                transport_at_bottom: false,
                preview_on_left: false,
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
}

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
        self.shortcuts.retain(|k, _| !k.is_empty() && k.len() <= 80);
        if self
            .window_layout
            .as_ref()
            .is_some_and(|s| s.trim().is_empty() || s.len() > 260)
        {
            self.window_layout = None;
        }
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
    }

    #[test]
    fn matriz_previa_quatro_por_tres() {
        assert_eq!(PreviewQuality::ALL.len(), 12);
        assert_eq!(
            PreviewQuality::from_parts(PreviewTier::Draft, PreviewResolution::Quarter),
            PreviewQuality::DraftQuarter
        );
        assert_eq!(
            PreviewQuality::from_parts(PreviewTier::Best, PreviewResolution::Full).scale(),
            1.0
        );
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

        let sf = EditLayoutPrefs::for_type(LayoutType::SonicFoundry);
        assert!(!sf.timeline_at_bottom);
        assert!(!sf.tabs_at_top);
        assert!(!sf.transport_at_bottom);

        let sony = EditLayoutPrefs::for_type(LayoutType::Sony);
        assert!(sony.timeline_at_bottom);
        assert!(!sony.tabs_at_top);
        assert!(sony.transport_at_bottom);

        let magix = EditLayoutPrefs::for_type(LayoutType::Magix);
        assert!(!magix.timeline_at_bottom);
        assert!(magix.tabs_at_top);
        assert!(!magix.transport_at_bottom);

        let raw = r#"{"edit_layout":{"layout_type":"Sonic Fonudry"}}"#;
        let p: Preferences = serde_json::from_str(raw).unwrap();
        assert_eq!(p.edit_layout.layout_type, LayoutType::SonicFoundry);
    }
}
