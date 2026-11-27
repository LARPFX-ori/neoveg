//! Gerenciador de layouts de janela (`.neoveglayout`) com presets inspirados
//! no VEGAS Pro e suporte a painéis destacáveis (§2.2 / §3.2).

use crate::{DockTab, Preferences};
use neoveg_common::EditLayoutPrefs;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

const MAX_LAYOUT_BYTES: u64 = 256 * 1024;

pub fn panel_key(tab: DockTab) -> &'static str {
    match tab {
        DockTab::ProjectMedia => "project_media",
        DockTab::Explorer => "explorer",
        DockTab::Trimmer => "trimmer",
        DockTab::EditDetails => "edit_details",
        DockTab::Transitions => "transitions",
        DockTab::VideoFx => "video_fx",
        DockTab::ColorGrading => "color_grading",
        DockTab::AudioFx => "audio_fx",
        DockTab::Generators => "generators",
        DockTab::Mixer => "mixer",
        DockTab::Loudness => "loudness",
        DockTab::SurroundPanner => "surround_panner",
        DockTab::PluginManager => "plugin_manager",
        DockTab::Compositors => "compositors",
        DockTab::Scopes => "scopes",
        DockTab::Notes => "notes",
        DockTab::Encoder => "encoder",
        DockTab::Capture => "capture",
        DockTab::Transcript => "transcript",
        DockTab::Store => "store",
    }
}

pub fn panel_from_key(key: &str) -> Option<DockTab> {
    let k = key.trim().to_ascii_lowercase();
    DockTab::ALL.into_iter().find(|t| panel_key(*t) == k)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowLayout {
    pub name: String,
    #[serde(default = "default_dock_height")]
    pub dock_height: f32,
    #[serde(default = "default_preview_ratio")]
    pub preview_ratio: f32,
    #[serde(default = "yes")]
    pub dock_visible: bool,
    #[serde(default = "default_active_tab")]
    pub active_tab: String,
    #[serde(default)]
    pub detached_tabs: Vec<String>,
    #[serde(default)]
    pub preview_detached: bool,
    #[serde(default = "yes")]
    pub preview_visible: bool,
    #[serde(default = "yes")]
    pub master_bus_visible: bool,
    #[serde(default)]
    pub master_bus_detached: bool,
    #[serde(default)]
    pub px_per_sec: Option<f32>,
    #[serde(default)]
    pub edit_layout: Option<EditLayoutPrefs>,
    #[serde(default)]
    pub tile_tree: Option<egui_tiles::Tree<crate::dock::DockPane>>,
    /// Painéis fechados (fora da área de janelas e sem janela própria); voltam
    /// pelo menu Exibir. `None` (layout antigo ou preset): usa
    /// [`DEFAULT_CLOSED_TABS`].
    #[serde(default)]
    pub closed_tabs: Option<Vec<String>>,
    /// Posição e tamanho de cada janela destacada (Prévia, Master Bus e
    /// painéis), por chave: `[x, y, largura, altura]` em pontos.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub floating_windows: BTreeMap<String, FloatingGeom>,
    /// Grupos de janelas flutuantes com múltiplas abas (docas flutuantes).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub floating_docks: Vec<FloatingDockGroup>,
}

/// Grupo de painéis em uma mesma janela flutuante (doca flutuante multi-abas).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FloatingDockGroup {
    pub id: String,
    pub panes: Vec<crate::dock::DockPane>,
    #[serde(default)]
    pub active: usize,
}

impl FloatingDockGroup {
    pub fn new(id: impl Into<String>, panes: Vec<crate::dock::DockPane>) -> Self {
        Self {
            id: id.into(),
            panes,
            active: 0,
        }
    }

    pub fn active_pane(&self) -> Option<crate::dock::DockPane> {
        self.panes
            .get(self.active)
            .copied()
            .or_else(|| self.panes.first().copied())
    }
}

/// Posição (canto de fora) e tamanho interno de uma janela destacada:
/// `[x, y, largura, altura]`.
pub type FloatingGeom = [f32; 4];

/// Chaves das janelas destacadas que não são painéis do dock.
pub const FLOATING_PREVIEW_KEY: &str = "preview";
pub const FLOATING_MASTER_KEY: &str = "master_bus";
const MAX_FLOATING_WINDOWS: usize = 64;

/// Geometria de janela válida, ou `None` (valores não finitos). Tamanho entre
/// 120 e 8192 e posição entre -16384 e 16384 (vários monitores, inclusive à
/// esquerda ou acima do principal).
pub fn sanitize_floating_geom(g: FloatingGeom) -> Option<FloatingGeom> {
    if !g.iter().all(|v| v.is_finite()) {
        return None;
    }
    Some([
        g[0].clamp(-16384.0, 16384.0),
        g[1].clamp(-16384.0, 16384.0),
        g[2].clamp(120.0, 8192.0),
        g[3].clamp(120.0, 8192.0),
    ])
}

fn floating_key_ok(k: &str) -> bool {
    k == FLOATING_PREVIEW_KEY
        || k == FLOATING_MASTER_KEY
        || panel_from_key(k).is_some()
        || k.starts_with("float_")
}

/// Painéis do grupo à esquerda da Prévia num layout novo, na ordem das abas
/// do VEGAS Pro (o Explorer vem primeiro). O Trimmer fica no grupo da Prévia
/// de Vídeo; os outros painéis começam fechados e abrem pelo menu Exibir.
pub const DEFAULT_DOCK_TABS: [DockTab; 6] = [
    DockTab::Explorer,
    DockTab::ProjectMedia,
    DockTab::Transitions,
    DockTab::VideoFx,
    DockTab::Generators,
    DockTab::PluginManager,
];

/// Painéis que começam fechados num layout novo.
/// Painéis que só abrem em janela própria, fora da área de janelas: o
/// Neoveg Encoder é uma ferramenta à parte, como um programa separado.
pub fn window_only(tab: DockTab) -> bool {
    matches!(tab, DockTab::Encoder | DockTab::Capture)
}

pub fn default_closed_tabs() -> HashSet<DockTab> {
    DockTab::ALL
        .into_iter()
        .filter(|t| !DEFAULT_DOCK_TABS.contains(t) && *t != DockTab::Trimmer)
        .collect()
}

fn default_dock_height() -> f32 {
    440.0
}

fn default_preview_ratio() -> f32 {
    0.40
}

fn default_active_tab() -> String {
    panel_key(DockTab::ProjectMedia).to_string()
}

fn yes() -> bool {
    true
}

impl Default for WindowLayout {
    fn default() -> Self {
        Self {
            name: "Padrão (Edição)".into(),
            dock_height: default_dock_height(),
            preview_ratio: default_preview_ratio(),
            dock_visible: true,
            active_tab: default_active_tab(),
            detached_tabs: Vec::new(),
            preview_detached: false,
            preview_visible: true,
            master_bus_visible: true,
            master_bus_detached: false,
            px_per_sec: None,
            edit_layout: None,
            tile_tree: None,
            closed_tabs: None,
            floating_windows: BTreeMap::new(),
            floating_docks: Vec::new(),
        }
    }
}

impl WindowLayout {
    pub fn sanitize(&mut self) {
        if self.name.trim().is_empty() {
            self.name = "Layout personalizado".into();
        }
        if self.name.len() > 80 {
            self.name = self.name.chars().take(80).collect();
        }
        self.dock_height = if self.dock_height.is_finite() {
            self.dock_height.clamp(160.0, 720.0)
        } else {
            default_dock_height()
        };
        self.preview_ratio = if self.preview_ratio.is_finite() {
            self.preview_ratio.clamp(0.20, 0.72)
        } else {
            default_preview_ratio()
        };
        if panel_from_key(&self.active_tab).is_none() {
            self.active_tab = default_active_tab();
        }
        let mut seen = HashSet::new();
        self.detached_tabs.retain(|k| {
            let Some(tab) = panel_from_key(k) else {
                return false;
            };
            seen.insert(panel_key(tab))
        });
        for k in &mut self.detached_tabs {
            if let Some(tab) = panel_from_key(k) {
                *k = panel_key(tab).to_string();
            }
        }
        if let Some(list) = self.closed_tabs.as_mut() {
            let mut seen = HashSet::new();
            list.retain(|k| panel_from_key(k).is_some_and(|tab| seen.insert(panel_key(tab))));
            for k in list.iter_mut() {
                if let Some(tab) = panel_from_key(k) {
                    *k = panel_key(tab).to_string();
                }
            }
        }
        let floating = std::mem::take(&mut self.floating_windows);
        self.floating_windows = floating
            .into_iter()
            .filter(|(k, _)| floating_key_ok(k))
            .filter_map(|(k, g)| sanitize_floating_geom(g).map(|g| (k, g)))
            .take(MAX_FLOATING_WINDOWS)
            .collect();
        let mut pane_seen = HashSet::new();
        self.floating_docks.retain_mut(|grp| {
            grp.panes.retain(|p| pane_seen.insert(*p));
            if grp.panes.is_empty() {
                return false;
            }
            if grp.active >= grp.panes.len() {
                grp.active = 0;
            }
            if grp.id.trim().is_empty() {
                grp.id = grp.panes[0].key().to_string();
            }
            true
        });
        for grp in &self.floating_docks {
            for pane in &grp.panes {
                match pane {
                    crate::dock::DockPane::Tab(t) => {
                        let k = panel_key(*t).to_string();
                        if !self.detached_tabs.contains(&k) {
                            self.detached_tabs.push(k);
                        }
                    }
                    crate::dock::DockPane::Preview => {
                        self.preview_detached = true;
                    }
                    crate::dock::DockPane::MasterBus => {
                        self.master_bus_detached = true;
                    }
                }
            }
        }
        self.px_per_sec = self
            .px_per_sec
            .filter(|v| v.is_finite() && *v > 0.0)
            .map(crate::timeline::TimelineState::clamp_pps);
        let pv_on_left = self
            .edit_layout
            .as_ref()
            .map(|e| e.preview_on_left)
            .unwrap_or(false);
        let active = self.active_panel();
        if let Some(ref mut tree) = self.tile_tree {
            crate::dock::sanitize_tile_tree(tree, self.preview_ratio, active, pv_on_left);
        }
    }

    pub fn active_panel(&self) -> DockTab {
        // painel só de janela (Encoder) nunca é a aba ativa do dock
        panel_from_key(&self.active_tab)
            .filter(|t| !window_only(*t))
            .unwrap_or(DockTab::ProjectMedia)
    }

    pub fn detached_panels(&self) -> HashSet<DockTab> {
        self.detached_tabs
            .iter()
            .filter_map(|k| panel_from_key(k))
            .collect()
    }

    /// Painéis fechados. O painel ativo e os destacados nunca contam como
    /// fechados (um preset que abre nos Escopos mostra os Escopos).
    pub fn closed_panels(&self) -> HashSet<DockTab> {
        let mut closed = match &self.closed_tabs {
            Some(list) => list.iter().filter_map(|k| panel_from_key(k)).collect(),
            None => default_closed_tabs(),
        };
        closed.remove(&self.active_panel());
        for t in self.detached_panels() {
            closed.remove(&t);
        }
        closed
    }
}

fn closed_except(open: &[DockTab]) -> Option<Vec<String>> {
    Some(
        DockTab::ALL
            .into_iter()
            .filter(|t| !open.contains(t))
            .map(|t| panel_key(t).to_string())
            .collect(),
    )
}

pub fn builtin_presets() -> Vec<WindowLayout> {
    vec![
        WindowLayout::default(),
        WindowLayout {
            name: "Edição com Trimmer".into(),
            dock_height: 460.0,
            preview_ratio: 0.34,
            dock_visible: true,
            active_tab: panel_key(DockTab::ProjectMedia).into(),
            detached_tabs: Vec::new(),
            preview_detached: false,
            preview_visible: true,
            master_bus_visible: true,
            master_bus_detached: false,
            px_per_sec: None,
            edit_layout: None,
            tile_tree: Some(crate::dock::build_trimmer_editing_tree()),
            closed_tabs: closed_except(&[
                DockTab::ProjectMedia,
                DockTab::Explorer,
                DockTab::Trimmer,
                DockTab::Transitions,
                DockTab::VideoFx,
                DockTab::Generators,
                DockTab::PluginManager,
            ]),
            floating_windows: BTreeMap::new(),
            floating_docks: Vec::new(),
        },
        WindowLayout {
            name: "Correção de Cor (Escopos)".into(),
            dock_height: 520.0,
            preview_ratio: 0.36,
            dock_visible: true,
            active_tab: panel_key(DockTab::Scopes).into(),
            detached_tabs: Vec::new(),
            preview_detached: false,
            preview_visible: true,
            master_bus_visible: false,
            master_bus_detached: false,
            px_per_sec: None,
            edit_layout: None,
            tile_tree: Some(crate::dock::build_color_grading_tree()),
            closed_tabs: closed_except(&[
                DockTab::ColorGrading,
                DockTab::VideoFx,
                DockTab::Scopes,
                DockTab::ProjectMedia,
                DockTab::Trimmer,
            ]),
            floating_windows: BTreeMap::new(),
            floating_docks: Vec::new(),
        },
        WindowLayout {
            name: "Mixagem de Áudio".into(),
            dock_height: 500.0,
            preview_ratio: 0.30,
            dock_visible: true,
            active_tab: panel_key(DockTab::Mixer).into(),
            detached_tabs: Vec::new(),
            preview_detached: false,
            preview_visible: true,
            master_bus_visible: true,
            master_bus_detached: false,
            px_per_sec: None,
            edit_layout: None,
            tile_tree: Some(crate::dock::build_audio_mixing_tree()),
            closed_tabs: closed_except(&[
                DockTab::Mixer,
                DockTab::AudioFx,
                DockTab::Loudness,
                DockTab::SurroundPanner,
                DockTab::ProjectMedia,
                DockTab::Trimmer,
            ]),
            floating_windows: BTreeMap::new(),
            floating_docks: Vec::new(),
        },
        WindowLayout {
            name: "Timeline Ampla".into(),
            dock_height: 220.0,
            preview_ratio: 0.40,
            dock_visible: false,
            active_tab: panel_key(DockTab::ProjectMedia).into(),
            detached_tabs: Vec::new(),
            preview_detached: false,
            preview_visible: true,
            master_bus_visible: false,
            master_bus_detached: false,
            px_per_sec: None,
            edit_layout: None,
            tile_tree: None,
            closed_tabs: None,
            floating_windows: BTreeMap::new(),
            floating_docks: Vec::new(),
        },
    ]
}

pub fn localized_preset_name(name: &str, lang: neoveg_common::Language) -> &str {
    neoveg_common::tr(lang, name)
}

pub fn sanitize_layout_filename(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .to_string();
    if out.is_empty() {
        out = "layout".into();
    }
    if out.len() > 64 {
        out = out.chars().take(64).collect();
    }
    out
}

pub fn load_layout_file(path: &Path) -> Result<WindowLayout, String> {
    let meta = std::fs::metadata(path)
        .map_err(|e| format!("não foi possível acessar '{}': {e}", path.display()))?;
    if meta.len() > MAX_LAYOUT_BYTES {
        return Err(format!(
            "arquivo de layout '{}' excede o limite de {} bytes",
            path.display(),
            MAX_LAYOUT_BYTES
        ));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("falha ao ler '{}': {e}", path.display()))?;
    let mut layout: WindowLayout = serde_json::from_str(&text)
        .map_err(|e| format!("layout inválido em '{}': {e}", path.display()))?;
    layout.sanitize();
    Ok(layout)
}

pub fn save_layout_file(layout: &WindowLayout, path: &Path) -> Result<(), String> {
    let mut clean = layout.clone();
    clean.sanitize();
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("erro ao criar pasta '{}': {e}", dir.display()))?;
        }
    }
    let json = serde_json::to_string_pretty(&clean)
        .map_err(|e| format!("erro ao serializar layout: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("erro ao gravar '{}': {e}", path.display()))
}

pub fn list_saved_layouts() -> Vec<(String, PathBuf)> {
    let dir = Preferences::layouts_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("neoveglayout") {
            continue;
        }
        if let Ok(layout) = load_layout_file(&p) {
            out.push((layout.name, p));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_hostil_e_sanitizado_e_round_trip_funciona() {
        let mut bad = WindowLayout {
            name: String::new(),
            dock_height: f32::NAN,
            preview_ratio: 99.0,
            dock_visible: true,
            active_tab: "aba_inexistente".into(),
            detached_tabs: vec![
                "mixer".into(),
                "MIXER".into(),
                "bogus".into(),
                "scopes".into(),
            ],
            preview_detached: true,
            preview_visible: true,
            master_bus_visible: true,
            master_bus_detached: true,
            px_per_sec: Some(-10.0),
            edit_layout: Some(EditLayoutPrefs::for_type(neoveg_common::LayoutType::Sony)),
            tile_tree: None,
            closed_tabs: None,
            floating_windows: BTreeMap::new(),
            floating_docks: Vec::new(),
        };
        bad.sanitize();
        assert_eq!(bad.dock_height, 440.0);
        assert_eq!(bad.preview_ratio, 0.72);
        assert_eq!(bad.active_panel(), DockTab::ProjectMedia);
        assert_eq!(bad.detached_tabs, vec!["mixer", "scopes"]);
        assert_eq!(bad.px_per_sec, None);

        let mut clamped = bad.clone();
        clamped.px_per_sec = Some(1.0e-12);
        clamped.sanitize();
        assert_eq!(
            clamped.px_per_sec,
            Some(crate::timeline::TimelineState::MIN_PX_PER_SEC)
        );
        // zoom muito aproximado agora é aceito (sem o antigo teto de 2000 px/s)
        clamped.px_per_sec = Some(1.0e6);
        clamped.sanitize();
        assert_eq!(clamped.px_per_sec, Some(1.0e6));

        let path = std::env::temp_dir().join("neoveg_test_layout.neoveglayout");
        save_layout_file(&bad, &path).unwrap();
        let loaded = load_layout_file(&path).unwrap();
        assert_eq!(loaded, bad);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn presets_embutidos_sao_validos() {
        let presets = builtin_presets();
        assert_eq!(presets.len(), 5);
        for mut p in presets {
            let before = p.clone();
            p.sanitize();
            assert_eq!(p, before);
            assert!(
                p.detached_tabs.is_empty(),
                "presets embutidos devem manter todos os painéis acoplados ({})",
                p.name
            );
        }
    }

    #[test]
    fn geometria_das_janelas_destacadas_e_saneada_e_volta_no_arquivo() {
        let mut l = WindowLayout::default();
        l.floating_windows
            .insert(FLOATING_PREVIEW_KEY.into(), [10.0, 20.0, 640.0, 360.0]);
        l.floating_windows
            .insert("mixer".into(), [f32::NAN, 0.0, 300.0, 300.0]);
        l.floating_windows
            .insert("encoder".into(), [-1.0e9, 1.0e9, 1.0, 1.0e9]);
        l.floating_windows
            .insert("chave_inexistente".into(), [0.0, 0.0, 300.0, 300.0]);
        l.sanitize();
        assert_eq!(
            l.floating_windows.get(FLOATING_PREVIEW_KEY),
            Some(&[10.0, 20.0, 640.0, 360.0])
        );
        assert!(!l.floating_windows.contains_key("mixer"), "NaN sai");
        assert!(!l.floating_windows.contains_key("chave_inexistente"));
        assert_eq!(
            l.floating_windows.get("encoder"),
            Some(&[-16384.0, 16384.0, 120.0, 8192.0])
        );
        let json = serde_json::to_string(&l).expect("json");
        let back: WindowLayout = serde_json::from_str(&json).expect("volta");
        assert_eq!(back.floating_windows, l.floating_windows);
        // layout antigo, sem o campo
        let mut antigo = WindowLayout::default();
        antigo.floating_windows.clear();
        let json = serde_json::to_string(&antigo).expect("json");
        assert!(!json.contains("floating_windows"));
    }
}
