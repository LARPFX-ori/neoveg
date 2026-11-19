//! Recuperação automática do projeto principal e das abas de projetos
//! `.neoveg` aninhados abertos em caso de pânico ou queda (AGENTS.md §0.7).

use crate::model::Project;
use crate::util;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

type Snapshot = Arc<Project>;

static LAST_SNAPSHOT: Mutex<Option<Snapshot>> = Mutex::new(None);
static LAST_NESTED_SNAPSHOTS: Mutex<Vec<(PathBuf, Snapshot)>> = Mutex::new(Vec::new());

/// Limite máximo de abas aninhadas salvas na recuperação automática (§0.4).
pub const MAX_NESTED_RECOVERY_TABS: usize = 7;
/// Limite de leitura do arquivo de recuperação de aninhados (64 MiB, §0.4).
const MAX_NESTED_RECOVERY_BYTES: u64 = 64 * 1024 * 1024;

/// Uma aba de projeto `.neoveg` aninhado preservada pela recuperação automática.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NestedRecoveryTab {
    pub path: PathBuf,
    pub project: Project,
}

/// Guarda o último estado do projeto principal para o hook de pânico salvar (§0.7).
pub fn remember_snapshot(p: Snapshot) {
    *util::lock(&LAST_SNAPSHOT) = Some(p);
}

/// Guarda os últimos estados das abas de projetos aninhados modificados (§0.7).
pub fn remember_nested_snapshots(tabs: Vec<(PathBuf, Snapshot)>) {
    let mut g = util::lock(&LAST_NESTED_SNAPSHOTS);
    *g = tabs.into_iter().take(MAX_NESTED_RECOVERY_TABS).collect();
}

pub fn recovery_path() -> PathBuf {
    util::base_dir().join("autosave").join("recuperacao.neoveg")
}

pub fn nested_recovery_path() -> PathBuf {
    util::base_dir()
        .join("autosave")
        .join("recuperacao_aninhados.json")
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

/// Retorna o caminho de uma cópia histórica do salvamento automático no índice `idx`.
pub fn recovery_history_path(idx: usize) -> PathBuf {
    if idx == 0 {
        recovery_path()
    } else {
        util::base_dir()
            .join("autosave")
            .join(format!("recuperacao.{idx}.neoveg"))
    }
}

/// Rotaciona os arquivos de recuperação preservando até `copies` instâncias.
pub fn rotate_recovery_files(copies: usize) {
    if copies <= 1 {
        return;
    }
    let max_idx = copies.saturating_sub(1);
    let oldest = recovery_history_path(max_idx);
    if oldest.exists() {
        let _ = std::fs::remove_file(&oldest);
    }
    for i in (1..=max_idx).rev() {
        let prev = recovery_history_path(i - 1);
        let curr = recovery_history_path(i);
        if prev.exists() {
            let _ = std::fs::rename(&prev, &curr)
                .or_else(|_| std::fs::copy(&prev, &curr).and_then(|_| std::fs::remove_file(&prev)));
        }
    }
}

/// Grava o projeto no arquivo de recuperação com histórico padrão (3 cópias).
pub fn write_recovery(p: &Project) -> Result<(), String> {
    write_recovery_with_history(p, 3)
}

/// Grava o projeto no arquivo de recuperação mantendo histórico rotativo.
pub fn write_recovery_with_history(p: &Project, history_copies: usize) -> Result<(), String> {
    let path = recovery_path();
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string(p).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    if path.exists() && history_copies > 1 {
        rotate_recovery_files(history_copies);
    }
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// Grava as abas de projetos `.neoveg` aninhados modificados na pasta `autosave/`.
/// Se a lista estiver vazia, remove o arquivo auxiliar anterior.
pub fn write_nested_recovery(tabs: &[(PathBuf, Project)]) -> Result<(), String> {
    let path = nested_recovery_path();
    if tabs.is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let payload: Vec<NestedRecoveryTab> = tabs
        .iter()
        .take(MAX_NESTED_RECOVERY_TABS)
        .map(|(p, proj)| NestedRecoveryTab {
            path: p.clone(),
            project: proj.clone(),
        })
        .collect();
    let json = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// Lê as abas de projetos aninhados preservadas na recuperação (tratando entrada
/// como hostil: limite de bytes e de abas; erro ou corrupção retorna lista vazia).
pub fn read_nested_recovery() -> Vec<NestedRecoveryTab> {
    read_nested_recovery_from(&nested_recovery_path())
}

pub fn read_nested_recovery_from(path: &std::path::Path) -> Vec<NestedRecoveryTab> {
    let Ok(f) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut bytes = Vec::new();
    if f.take(MAX_NESTED_RECOVERY_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || (bytes.len() as u64) > MAX_NESTED_RECOVERY_BYTES
    {
        return Vec::new();
    }
    let Ok(mut tabs) = serde_json::from_slice::<Vec<NestedRecoveryTab>>(&bytes) else {
        return Vec::new();
    };
    tabs.truncate(MAX_NESTED_RECOVERY_TABS);
    for t in &mut tabs {
        t.project.sanitize();
    }
    tabs
}

pub fn clear_recovery() {
    for idx in 0..10 {
        let _ = std::fs::remove_file(recovery_history_path(idx));
    }
    let _ = std::fs::remove_file(legacy_recovery_path());
    let _ = std::fs::remove_file(nested_recovery_path());
}

fn save_snapshot_on_panic() -> Option<String> {
    let guard = LAST_SNAPSHOT.try_lock().ok()?;
    let p = guard.as_ref()?;
    let main_msg = match write_recovery(p) {
        Ok(()) => "projeto salvo para recuperação em autosave/recuperacao.neoveg".to_string(),
        Err(e) => format!("falha ao salvar recuperação: {e}"),
    };
    if let Ok(nested) = LAST_NESTED_SNAPSHOTS.try_lock() {
        if !nested.is_empty() {
            let list: Vec<(PathBuf, Project)> = nested
                .iter()
                .map(|(path, proj)| (path.clone(), (**proj).clone()))
                .collect();
            let _ = write_nested_recovery(&list);
        }
    }
    Some(main_msg)
}

/// Instala o hook de pânico com persistência automática do snapshot do projeto.
pub fn install_panic_hook() {
    util::set_recovery_callback(save_snapshot_on_panic);
    util::install_panic_hook();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recuperacao_de_aninhados_roundtrip_e_entrada_corrompida() {
        let dir = std::env::temp_dir().join(format!("neoveg_rec_nested_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("nested_rec.json");
        let mut p = Project::new_default();
        p.active_sequence_name = Some("Editado em aba".into());
        let payload = vec![NestedRecoveryTab {
            path: dir.join("sub.neoveg"),
            project: p,
        }];
        std::fs::write(&file, serde_json::to_string(&payload).unwrap()).unwrap();
        let back = read_nested_recovery_from(&file);
        assert_eq!(back.len(), 1);
        assert_eq!(
            back[0].project.active_sequence_name.as_deref(),
            Some("Editado em aba")
        );

        // Arquivo corrompido nunca entra em pânico
        std::fs::write(&file, b"{corrompido!!!").unwrap();
        assert!(read_nested_recovery_from(&file).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
