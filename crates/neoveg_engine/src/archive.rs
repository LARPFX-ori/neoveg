//! Arquivamento autocontido de projetos com mídias (*Arquivo → Arquivar projeto com mídias...*).
//!
//! Copia o projeto `.neoveg` e os arquivos de mídia referenciados para uma pasta
//! de destino com subpasta `media/`, reescrevendo os caminhos de forma relativa
//! para facilitar entrega, backup ou migração entre máquinas (AGENTS.md §0).

use crate::model::{Id, MediaSource, Project};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Opções de arquivamento do projeto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveOptions {
    /// Quando `true`, inclui apenas mídias efetivamente usadas em eventos/takes
    /// (da sequência ativa ou de qualquer sequência interna do projeto).
    pub only_used_media: bool,
    /// Sobrescreve arquivos existentes na pasta de destino quando necessário.
    pub overwrite: bool,
}

impl Default for ArchiveOptions {
    fn default() -> Self {
        Self {
            only_used_media: false,
            overwrite: true,
        }
    }
}

/// Resultado do arquivamento.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArchiveReport {
    pub project_file: PathBuf,
    pub copied_files: usize,
    pub missing_files: Vec<String>,
    pub skipped_unused: usize,
}

/// Coleta todos os IDs de mídia referenciados por eventos e takes em todas as sequências.
pub fn collect_used_media_ids(project: &Project) -> HashSet<Id> {
    let mut used = HashSet::new();
    let mut scan_tracks = |tracks: &[crate::model::Track]| {
        for tr in tracks {
            for ev in &tr.events {
                used.insert(ev.media_id);
                for tk in &ev.alt_takes {
                    used.insert(tk.media_id);
                }
            }
        }
    };
    scan_tracks(&project.tracks);
    for seq in &project.sequences {
        scan_tracks(&seq.tracks);
    }
    used
}

fn safe_filename(raw: &str, fallback: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|&c| {
            !matches!(
                c,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0'
            ) && !c.is_control()
        })
        .take(96)
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn unique_dest_name(
    orig: &Path,
    used_names: &mut HashSet<String>,
    counter: usize,
) -> (String, PathBuf) {
    let stem = orig
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| safe_filename(s, "midia"))
        .unwrap_or_else(|| format!("midia_{counter}"));
    let ext = orig
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| safe_filename(s, ""))
        .unwrap_or_default();
    let make = |n: usize| -> String {
        let base = if n == 0 {
            stem.clone()
        } else {
            format!("{stem}_{n}")
        };
        if ext.is_empty() {
            base
        } else {
            format!("{base}.{ext}")
        }
    };
    let mut n = 0usize;
    loop {
        let candidate = make(n);
        let key = candidate.to_ascii_lowercase();
        if used_names.insert(key) {
            return (candidate.clone(), PathBuf::from("media").join(candidate));
        }
        n = n.saturating_add(1);
    }
}

/// Arquiva o projeto e suas mídias locais em `dest_dir`, gravando `<project_name>.neoveg`
/// e copiando os arquivos para `dest_dir/media/`.
pub fn archive_project_to_dir(
    project: &Project,
    dest_dir: &Path,
    project_name: &str,
    opts: ArchiveOptions,
) -> Result<ArchiveReport, String> {
    if dest_dir.as_os_str().is_empty() {
        return Err("Pasta de destino inválida.".to_string());
    }
    std::fs::create_dir_all(dest_dir)
        .map_err(|e| format!("Não foi possível criar a pasta de destino: {e}"))?;
    let media_dir = dest_dir.join("media");
    std::fs::create_dir_all(&media_dir)
        .map_err(|e| format!("Não foi possível criar a subpasta media/: {e}"))?;

    let mut archived = project.clone();
    let mut report = ArchiveReport::default();

    if opts.only_used_media {
        let used = collect_used_media_ids(&archived);
        let before = archived.media.len();
        archived.media.retain(|m| {
            used.contains(&m.id)
                || matches!(
                    m.source,
                    MediaSource::Generated(_) | MediaSource::Sequence { .. }
                )
        });
        report.skipped_unused = before.saturating_sub(archived.media.len());
    }

    let mut copied_map: HashMap<PathBuf, String> = HashMap::new();
    let mut used_names: HashSet<String> = HashSet::new();

    for (idx, item) in archived.media.iter_mut().enumerate() {
        match &mut item.source {
            MediaSource::File { path } | MediaSource::NestedProject { path } => {
                let src = PathBuf::from(path.as_str());
                if let Some(rel) = copied_map.get(&src) {
                    *path = rel.clone();
                } else if src.is_file() {
                    let (fname, rel_path) = unique_dest_name(&src, &mut used_names, idx + 1);
                    let dst = media_dir.join(&fname);
                    if opts.overwrite || !dst.exists() {
                        std::fs::copy(&src, &dst).map_err(|e| {
                            format!(
                                "Falha ao copiar mídia '{}' para '{}': {e}",
                                src.display(),
                                dst.display()
                            )
                        })?;
                        report.copied_files = report.copied_files.saturating_add(1);
                    }
                    let rel_str = rel_path.to_string_lossy().into_owned();
                    copied_map.insert(src, rel_str.clone());
                    *path = rel_str;
                } else {
                    report.missing_files.push(path.clone());
                }
            }
            MediaSource::ImageSequence { paths, .. } => {
                for (si, p_str) in paths.iter_mut().enumerate() {
                    let src = PathBuf::from(p_str.as_str());
                    if let Some(rel) = copied_map.get(&src) {
                        *p_str = rel.clone();
                    } else if src.is_file() {
                        let (fname, rel_path) =
                            unique_dest_name(&src, &mut used_names, idx * 1000 + si + 1);
                        let dst = media_dir.join(&fname);
                        if opts.overwrite || !dst.exists() {
                            std::fs::copy(&src, &dst).map_err(|e| {
                                format!(
                                    "Falha ao copiar mídia '{}' para '{}': {e}",
                                    src.display(),
                                    dst.display()
                                )
                            })?;
                            report.copied_files = report.copied_files.saturating_add(1);
                        }
                        let rel_str = rel_path.to_string_lossy().into_owned();
                        copied_map.insert(src, rel_str.clone());
                        *p_str = rel_str;
                    } else {
                        report.missing_files.push(p_str.clone());
                    }
                }
            }
            MediaSource::Generated(_) | MediaSource::Sequence { .. } => {}
        }
        // Limpa proxies temporários absolutos no arquivo arquivado
        item.proxy_path = None;
    }

    let clean_name = safe_filename(project_name, "projeto");
    let proj_file = dest_dir.join(format!("{clean_name}.neoveg"));
    archived.sort_events();
    archived.save_file(&proj_file)?;
    report.project_file = proj_file;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Event, MediaItem, Track, TrackKind};
    use crate::time::Ticks;

    #[test]
    fn arquivar_projeto_copia_midias_e_reescreve_caminhos_relativos() {
        let base = std::env::temp_dir().join(format!(
            "neoveg_archive_test_{}_{}",
            std::process::id(),
            crate::model::new_id()
        ));
        let src_dir = base.join("src");
        let dst_dir = base.join("out");
        std::fs::create_dir_all(&src_dir).unwrap();
        let f1 = src_dir.join("clip.wav");
        let f2 = src_dir.join("unused.wav");
        std::fs::write(&f1, b"RIFF0000WAVE").unwrap();
        std::fs::write(&f2, b"RIFF0000WAVE").unwrap();

        let mut p = Project::new_default();
        p.tracks.clear();
        let m1 = MediaItem {
            id: crate::model::new_id(),
            name: "clip.wav".into(),
            source: MediaSource::File {
                path: f1.to_string_lossy().into_owned(),
            },
            duration: Some(Ticks::from_secs_i64(2)),
            has_video: false,
            has_audio: true,
            width: 0,
            height: 0,
            fps: None,
            info: String::new(),
            proxy_path: None,
        };
        let m2 = MediaItem {
            id: crate::model::new_id(),
            name: "unused.wav".into(),
            source: MediaSource::File {
                path: f2.to_string_lossy().into_owned(),
            },
            duration: Some(Ticks::from_secs_i64(2)),
            has_video: false,
            has_audio: true,
            width: 0,
            height: 0,
            fps: None,
            info: String::new(),
            proxy_path: None,
        };
        let mut tr = Track::new(TrackKind::Audio, "A1");
        tr.events
            .push(Event::new(&m1, Ticks::ZERO, Ticks::from_secs_i64(2)));
        p.media.push(m1);
        p.media.push(m2);
        p.tracks.push(tr);

        let rep = archive_project_to_dir(
            &p,
            &dst_dir,
            "meu_projeto",
            ArchiveOptions {
                only_used_media: true,
                overwrite: true,
            },
        )
        .expect("archive ok");
        assert_eq!(rep.copied_files, 1);
        assert_eq!(rep.skipped_unused, 1);
        assert!(rep.missing_files.is_empty());
        assert!(rep.project_file.is_file());
        assert!(dst_dir.join("media").join("clip.wav").is_file());
        assert!(!dst_dir.join("media").join("unused.wav").exists());

        let (loaded, _warns) = Project::load_file(&rep.project_file).expect("reload archived");
        assert_eq!(loaded.media.len(), 1);
        let expected_rel = PathBuf::from("media")
            .join("clip.wav")
            .to_string_lossy()
            .into_owned();
        assert!(matches!(
            &loaded.media[0].source,
            MediaSource::File { path } if path == &expected_rel
        ));

        let _ = std::fs::remove_dir_all(&base);
    }
}
