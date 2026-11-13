//! Preferências do usuário e configurações de qualidade/hardware (§3.2 / §3.6).

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

/// Escala de resolução da pré-visualização (Full / Half / Quarter).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum PreviewResolution {
    Quarter,
    #[default]
    Half,
    Full,
}

/// Qualidade combinada de pré-visualização. Mantém compatibilidade com os
/// rótulos usados na interface atual e prepara a matriz completa (§3.6).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum PreviewQuality {
    Draft,
    #[default]
    Preview,
    Good,
    BestFull,
}

impl PreviewQuality {
    pub fn name(self) -> &'static str {
        match self {
            PreviewQuality::Draft => "Rascunho (1/4)",
            PreviewQuality::Preview => "Prévia (1/2)",
            PreviewQuality::Good => "Boa (completa)",
            PreviewQuality::BestFull => "Melhor (completa)",
        }
    }

    pub fn scale(self) -> f32 {
        match self {
            PreviewQuality::Draft => 0.25,
            PreviewQuality::Preview => 0.5,
            PreviewQuality::Good | PreviewQuality::BestFull => 1.0,
        }
    }

    pub fn tier(self) -> PreviewTier {
        match self {
            PreviewQuality::Draft => PreviewTier::Draft,
            PreviewQuality::Preview => PreviewTier::Preview,
            PreviewQuality::Good => PreviewTier::Good,
            PreviewQuality::BestFull => PreviewTier::Best,
        }
    }

    pub fn resolution(self) -> PreviewResolution {
        match self {
            PreviewQuality::Draft => PreviewResolution::Quarter,
            PreviewQuality::Preview => PreviewResolution::Half,
            PreviewQuality::Good | PreviewQuality::BestFull => PreviewResolution::Full,
        }
    }
}

/// Preferências persistidas em JSON com proteção contra entradas hostis (§0.4).
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
    #[serde(default)]
    pub shortcuts: HashMap<String, Option<String>>,
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

impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: default_lang(),
            theme: default_theme(),
            ram_preview_mb: default_ram_cache_mb(),
            frame_cache_cap: default_frame_cache(),
            shortcuts: HashMap::new(),
        }
    }
}

impl Preferences {
    pub fn file_path() -> PathBuf {
        util::base_dir().join("preferencias.json")
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
        self.shortcuts.retain(|k, _| !k.is_empty() && k.len() <= 80);
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
        let raw = r#"{"language":"","theme":"x","ram_preview_mb":99999999,"frame_cache_cap":0}"#;
        let mut p: Preferences = serde_json::from_str(raw).unwrap();
        p.sanitize();
        assert_eq!(p.language, "pt-BR");
        assert_eq!(p.ram_preview_mb, 8192);
        assert_eq!(p.frame_cache_cap, 4);
    }
}
