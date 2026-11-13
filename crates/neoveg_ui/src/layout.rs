//! Gerenciador de layouts de janela (`.neoveglayout`) com presets inspirados
//! no VEGAS Pro e suporte a painéis destacáveis (§2.2 / §3.2).

use crate::{DockTab, Preferences};
use neoveg_common::EditLayoutPrefs;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const MAX_LAYOUT_BYTES: u64 = 256 * 1024;

pub fn panel_key(tab: DockTab) -> &'static str {
    match tab {
        DockTab::ProjectMedia => "project_media",
        DockTab::Explorer => "explorer",
        DockTab::Transitions => "transitions",
        DockTab::VideoFx => "video_fx",
        DockTab::AudioFx => "audio_fx",
        DockTab::Generators => "generators",
        DockTab::Mixer => "mixer",
        DockTab::Scopes => "scopes",
        DockTab::Notes => "notes",
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
    pub px_per_sec: Option<f32>,
    #[serde(default)]
    pub edit_layout: Option<EditLayoutPrefs>,
}

fn default_dock_height() -> f32 {
    330.0
}

fn default_preview_ratio() -> f32 {
    0.42
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
            px_per_sec: None,
            edit_layout: None,
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
        self.px_per_sec = self
            .px_per_sec
            .filter(|v| v.is_finite() && *v > 0.0)
            .map(|v| v.clamp(2.0, 2000.0));
    }

    pub fn active_panel(&self) -> DockTab {
        panel_from_key(&self.active_tab).unwrap_or(DockTab::ProjectMedia)
    }

    pub fn detached_panels(&self) -> HashSet<DockTab> {
        self.detached_tabs
            .iter()
            .filter_map(|k| panel_from_key(k))
            .collect()
    }
}

pub fn builtin_presets() -> Vec<WindowLayout> {
    vec![
        WindowLayout::default(),
        WindowLayout {
            name: "Correção de Cor (Escopos)".into(),
            dock_height: 390.0,
            preview_ratio: 0.46,
            dock_visible: true,
            active_tab: panel_key(DockTab::Scopes).into(),
            detached_tabs: vec![panel_key(DockTab::VideoFx).into()],
            px_per_sec: None,
            edit_layout: None,
        },
        WindowLayout {
            name: "Mixagem de Áudio".into(),
            dock_height: 360.0,
            preview_ratio: 0.28,
            dock_visible: true,
            active_tab: panel_key(DockTab::Mixer).into(),
            detached_tabs: vec![panel_key(DockTab::AudioFx).into()],
            px_per_sec: None,
            edit_layout: None,
        },
        WindowLayout {
            name: "Timeline Ampla".into(),
            dock_height: 220.0,
            preview_ratio: 0.40,
            dock_visible: false,
            active_tab: panel_key(DockTab::ProjectMedia).into(),
            detached_tabs: Vec::new(),
            px_per_sec: None,
            edit_layout: None,
        },
    ]
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
            px_per_sec: Some(-10.0),
            edit_layout: Some(EditLayoutPrefs::for_type(neoveg_common::LayoutType::Sony)),
        };
        bad.sanitize();
        assert_eq!(bad.dock_height, 330.0);
        assert_eq!(bad.preview_ratio, 0.72);
        assert_eq!(bad.active_panel(), DockTab::ProjectMedia);
        assert_eq!(bad.detached_tabs, vec!["mixer", "scopes"]);
        assert_eq!(bad.px_per_sec, None);

        let mut clamped = bad.clone();
        clamped.px_per_sec = Some(0.1);
        clamped.sanitize();
        assert_eq!(clamped.px_per_sec, Some(2.0));

        let path = std::env::temp_dir().join("neoveg_test_layout.neoveglayout");
        save_layout_file(&bad, &path).unwrap();
        let loaded = load_layout_file(&path).unwrap();
        assert_eq!(loaded, bad);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn presets_embutidos_sao_validos() {
        let presets = builtin_presets();
        assert_eq!(presets.len(), 4);
        for mut p in presets {
            let before = p.clone();
            p.sanitize();
            assert_eq!(p, before);
        }
    }
}
