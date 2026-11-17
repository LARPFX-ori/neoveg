//! Pastas Monitoradas (*Watch Folders*) do Neoveg Encoder.
//!
//! Múltiplas entradas podem apontar para a mesma pasta de origem com
//! predefinições distintas para gerar automaticamente saídas simultâneas
//! (§"Crie saída em múltiplos formatos usando pastas monitoradas").

use super::presets::PresetCatalog;
use super::{EncoderState, MediaInterpretation, OutputJob, QueueItem, QueueSource};
use crate::media::is_supported;
use crate::model::new_id;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const MAX_FILES_PER_SCAN: usize = 500;
const MAX_SEEN_ENTRIES: usize = 20_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchFolder {
    pub id: u64,
    pub folder: PathBuf,
    #[serde(default)]
    pub output_dir: Option<PathBuf>,
    #[serde(default)]
    pub preset_ids: Vec<String>,
    #[serde(default = "yes")]
    pub auto_encode: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub seen_signatures: HashSet<String>,
}

fn yes() -> bool {
    true
}

impl WatchFolder {
    pub fn new(folder: PathBuf, preset_id: &str) -> Self {
        Self {
            id: new_id(),
            folder,
            output_dir: None,
            preset_ids: vec![preset_id.to_string()],
            auto_encode: true,
            enabled: true,
            seen_signatures: HashSet::new(),
        }
    }

    pub fn effective_output_dir(&self) -> PathBuf {
        self.output_dir
            .clone()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| self.folder.join("Saida"))
    }
}

/// Varre todas as pastas monitoradas ativas e enfileira novos arquivos de mídia.
/// Retorna `(novos_itens_na_fila, iniciar_automaticamente)`.
pub fn scan_watch_folders(state: &mut EncoderState, catalog: &PresetCatalog) -> (usize, bool) {
    let mut added = 0usize;
    let mut should_auto_start = false;
    let global_auto = state.prefs.auto_encode_watch_folders;

    let mut new_items = Vec::new();
    for wf in &mut state.watch_folders {
        if !wf.enabled || !wf.folder.is_dir() {
            continue;
        }
        let out_dir = wf.effective_output_dir();
        let Ok(rd) = std::fs::read_dir(&wf.folder) else {
            continue;
        };
        let mut files: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| is_watch_candidate(p, &out_dir))
            .take(MAX_FILES_PER_SCAN)
            .collect();
        files.sort();

        // Coleta predefinições válidas (expandindo grupos quando houver)
        let mut resolved_presets = Vec::new();
        for pid in &wf.preset_ids {
            resolved_presets.extend(catalog.resolve_group_or_preset(pid));
        }
        if resolved_presets.is_empty() {
            resolved_presets
                .extend(catalog.resolve_group_or_preset(&state.prefs.default_preset_id));
        }
        if resolved_presets.is_empty() {
            continue;
        }

        for path in files {
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            if meta.len() == 0 {
                continue;
            }
            let sig = file_signature(&path, &meta);
            if wf.seen_signatures.contains(&sig) {
                continue;
            }
            if wf.seen_signatures.len() >= MAX_SEEN_ENTRIES {
                wf.seen_signatures.clear();
            }
            wf.seen_signatures.insert(sig);

            let src = QueueSource::MediaFile {
                path,
                is_sequence: false,
                sequence_fps: None,
                interpret: MediaInterpretation::default(),
            };
            let mut outputs = Vec::with_capacity(resolved_presets.len());
            for p in &resolved_presets {
                outputs.push(OutputJob::from_preset(
                    p,
                    &src,
                    Some(&out_dir),
                    (1920, 1080),
                ));
            }
            new_items.push(QueueItem {
                id: new_id(),
                source: src,
                outputs,
                expanded: true,
                watch_folder_id: Some(wf.id),
            });
            added += 1;
            if global_auto && wf.auto_encode {
                should_auto_start = true;
            }
        }
    }

    state.items.extend(new_items);
    (added, should_auto_start)
}

fn is_watch_candidate(path: &Path, out_dir: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    if path.starts_with(out_dir) {
        return false;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.starts_with('.') || name.contains(".neoveg_part_") {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".tmp") || lower.ends_with(".part") || lower.ends_with(".neoveg") {
        return false;
    }
    is_supported(path)
}

fn file_signature(path: &Path, meta: &std::fs::Metadata) -> String {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!(
        "{}|{}|{}",
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        meta.len(),
        mtime
    )
}
