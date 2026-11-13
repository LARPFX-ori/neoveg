//! Recuperação automática do projeto em caso de pânico (AGENTS.md §0.7).

use crate::model::Project;
use crate::util;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

type Snapshot = Arc<Project>;

static LAST_SNAPSHOT: Mutex<Option<Snapshot>> = Mutex::new(None);

/// Guarda o último estado do projeto para o hook de pânico salvar (§0.7).
pub fn remember_snapshot(p: Snapshot) {
    *util::lock(&LAST_SNAPSHOT) = Some(p);
}

pub fn recovery_path() -> PathBuf {
    util::base_dir().join("autosave").join("recuperacao.neoveg")
}

pub fn legacy_recovery_path() -> PathBuf {
    util::base_dir().join("autosave").join("recuperacao.nvp")
}

/// Retorna o caminho do arquivo de recuperação existente (`.neoveg` preferencial, ou `.nvp` legado).
pub fn find_existing_recovery() -> Option<PathBuf> {
    let cur = recovery_path();
    if cur.exists() {
        return Some(cur);
    }
    let legacy = legacy_recovery_path();
    if legacy.exists() {
        return Some(legacy);
    }
    None
}

/// Grava o projeto no arquivo de recuperação.
pub fn write_recovery(p: &Project) -> Result<(), String> {
    let path = recovery_path();
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string(p).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

pub fn clear_recovery() {
    let _ = std::fs::remove_file(recovery_path());
    let _ = std::fs::remove_file(legacy_recovery_path());
}

fn save_snapshot_on_panic() -> Option<String> {
    let guard = LAST_SNAPSHOT.try_lock().ok()?;
    let p = guard.as_ref()?;
    match write_recovery(p) {
        Ok(()) => Some("projeto salvo para recuperação em autosave/recuperacao.neoveg".into()),
        Err(e) => Some(format!("falha ao salvar recuperação: {e}")),
    }
}

/// Instala o hook de pânico com persistência automática do snapshot do projeto.
pub fn install_panic_hook() {
    util::set_recovery_callback(save_snapshot_on_panic);
    util::install_panic_hook();
}
