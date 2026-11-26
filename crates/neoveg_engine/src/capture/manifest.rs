//! Formato de projeto de captura `.nvcap` / `export.json` do **Neoveg Capture**
//! (paridade limpa com o `.vegcap` e `export.json` do VEGAS Capture) e
//! importação atômica em trilhas dedicadas na linha do tempo do projeto
//! (`AGENTS.md` §0.4 e §3.1).

use crate::media::{import_file, MediaCache};
use crate::model::{Event, Id, Ticks, Track, TrackKind};
use crate::session::EditorSession;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// Tamanho máximo aceito de um manifesto `.nvcap` / `export.json` (§0.4).
pub const MAX_MANIFEST_BYTES: u64 = 2 * 1024 * 1024;
/// Número máximo de faixas aceitas por sessão de captura (§0.4).
pub const MAX_MANIFEST_TRACKS: usize = 64;

/// Identificador de formato do manifesto de sessão do Neoveg Capture.
pub const MANIFEST_FORMAT_ID: &str = "neoveg_capture";
/// Versão atual do esquema `.nvcap` / `export.json`.
pub const MANIFEST_VERSION: u32 = 1;

/// Entrada individual de mídia gravada em uma sessão do Neoveg Capture.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedTrackEntry {
    pub source_name: String,
    pub is_video: bool,
    /// Nome de arquivo relativo à pasta da sessão (ou caminho validado).
    pub file: String,
    pub codec: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub fps_num: u32,
    #[serde(default)]
    pub fps_den: u32,
    #[serde(default)]
    pub sample_rate: u32,
    #[serde(default)]
    pub channels: u16,
    #[serde(default)]
    pub duration_ticks: i64,
    #[serde(default)]
    pub frame_count: u64,
}

/// Manifesto de uma sessão gravada pelo Neoveg Capture (`.nvcap` ou `export.json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureProjectManifest {
    pub format: String,
    pub version: u32,
    pub created_at: String,
    #[serde(default)]
    pub duration_ticks: i64,
    #[serde(default)]
    pub tracks: Vec<CapturedTrackEntry>,
}

impl CaptureProjectManifest {
    pub fn new(created_at: String, duration_ticks: Ticks, tracks: Vec<CapturedTrackEntry>) -> Self {
        Self {
            format: MANIFEST_FORMAT_ID.to_string(),
            version: MANIFEST_VERSION,
            created_at,
            duration_ticks: duration_ticks.0.max(0),
            tracks,
        }
    }

    pub fn sanitize(&mut self) {
        self.created_at = sanitize_text(&self.created_at, 64);
        self.duration_ticks = self.duration_ticks.clamp(0, Ticks::MAX.0);
        self.tracks.truncate(MAX_MANIFEST_TRACKS);
        for tr in &mut self.tracks {
            tr.source_name = sanitize_text(&tr.source_name, 160);
            if tr.source_name.is_empty() {
                tr.source_name = if tr.is_video {
                    "Captura de vídeo".into()
                } else {
                    "Captura de áudio".into()
                };
            }
            tr.file = sanitize_text(&tr.file, 512);
            tr.codec = sanitize_text(&tr.codec, 64);
            tr.width = tr.width.min(crate::media::capture_os::MAX_CAPTURE_DIM);
            tr.height = tr.height.min(crate::media::capture_os::MAX_CAPTURE_DIM);
            tr.sample_rate = tr.sample_rate.min(192_000);
            tr.channels = tr.channels.min(8);
            tr.duration_ticks = tr.duration_ticks.clamp(0, Ticks::MAX.0);
        }
    }
}

fn sanitize_text(s: &str, max_chars: usize) -> String {
    s.chars()
        .filter(|&c| c != '\0' && !c.is_control())
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Resolve um caminho de arquivo listado no manifesto em relação a `base_dir`,
/// bloqueando travessia de diretórios (`..`) em caminhos relativos (§0.4).
pub fn resolve_track_file(base_dir: &Path, entry_file: &str) -> Result<PathBuf> {
    let trimmed = entry_file.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        bail!("caminho de faixa vazio ou inválido no manifesto");
    }
    let p = Path::new(trimmed);
    if p.is_absolute() {
        for comp in p.components() {
            if matches!(comp, Component::ParentDir) {
                bail!("caminho com '..' recusado no manifesto: {trimmed}");
            }
        }
        return Ok(p.to_path_buf());
    }
    for comp in p.components() {
        if matches!(
            comp,
            Component::ParentDir | Component::Prefix(_) | Component::RootDir
        ) {
            bail!("caminho relativo inseguro no manifesto: {trimmed}");
        }
    }
    Ok(base_dir.join(p))
}

/// Grava `manifest` em `path` de forma atômica (arquivo temporário + rename).
pub fn save_manifest(path: &Path, manifest: &CaptureProjectManifest) -> Result<()> {
    let mut clean = manifest.clone();
    clean.sanitize();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("criando pasta {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(&clean)?;
    let tmp = path.with_extension("nvcap.tmp");
    std::fs::write(&tmp, json).with_context(|| format!("gravando {}", tmp.display()))?;
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    std::fs::rename(&tmp, path).with_context(|| format!("finalizando {}", path.display()))?;
    Ok(())
}

/// Carrega e valida um manifesto `.nvcap` ou `export.json` (tratando toda
/// entrada como hostil, §0.4).
pub fn load_manifest(path: &Path) -> Result<CaptureProjectManifest> {
    let meta =
        std::fs::metadata(path).with_context(|| format!("abrindo manifesto {}", path.display()))?;
    if meta.len() > MAX_MANIFEST_BYTES {
        bail!(
            "manifesto de captura excede o limite de {} bytes",
            MAX_MANIFEST_BYTES
        );
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("lendo manifesto {}", path.display()))?;
    parse_manifest_str(&raw)
}

/// Faz o parsing validado de uma string JSON de manifesto `.nvcap` / `export.json`.
pub fn parse_manifest_str(raw: &str) -> Result<CaptureProjectManifest> {
    if raw.len() as u64 > MAX_MANIFEST_BYTES {
        bail!("manifesto de captura grande demais");
    }
    let mut m: CaptureProjectManifest =
        serde_json::from_str(raw).map_err(|e| anyhow!("manifesto .nvcap inválido: {e}"))?;
    if m.format != MANIFEST_FORMAT_ID {
        bail!(
            "formato de manifesto não reconhecido: '{}' (esperado '{}')",
            m.format,
            MANIFEST_FORMAT_ID
        );
    }
    if m.version == 0 || m.version > MANIFEST_VERSION {
        bail!("versão de manifesto .nvcap não suportada: {}", m.version);
    }
    m.sanitize();
    Ok(m)
}

/// Resultado da importação de uma sessão do Neoveg Capture para a timeline.
#[derive(Clone, Debug, Default)]
pub struct CaptureImportReport {
    pub imported_media_ids: Vec<Id>,
    pub created_track_ids: Vec<Id>,
    pub placed_event_ids: Vec<Id>,
    pub warnings: Vec<String>,
}

/// Importa todos os arquivos gravados de `manifest` para o projeto ativo,
/// colocando cada fonte capturada em uma **trilha dedicada com o nome da fonte**
/// alinhada em `place_at` — tudo em um único checkpoint atômico de Desfazer/Refazer
/// (espelho de *Export to VEGAS* no manual do VEGAS Pro 23).
pub fn apply_manifest_to_session(
    session: &mut EditorSession,
    media_cache: &MediaCache,
    manifest: &CaptureProjectManifest,
    base_dir: &Path,
    place_at: Ticks,
) -> Result<CaptureImportReport> {
    let mut report = CaptureImportReport::default();
    let place_at = place_at.max(Ticks::ZERO);

    // Fase 1: importa mídia fora da mutação do projeto (se todas falharem, não suja Undo).
    let mut ready: Vec<(&CapturedTrackEntry, crate::media::Imported)> = Vec::new();
    for tr_entry in &manifest.tracks {
        let file_path = match resolve_track_file(base_dir, &tr_entry.file) {
            Ok(p) => p,
            Err(e) => {
                report
                    .warnings
                    .push(format!("{}: {e}", tr_entry.source_name));
                continue;
            }
        };
        match import_file(&file_path) {
            Ok(imp) => ready.push((tr_entry, imp)),
            Err(e) => report
                .warnings
                .push(format!("{}: {e}", tr_entry.source_name)),
        }
    }

    if ready.is_empty() {
        bail!(
            "Nenhuma mídia válida da sessão de captura pôde ser importada{}",
            if report.warnings.is_empty() {
                ".".to_string()
            } else {
                format!(": {}", report.warnings.join("; "))
            }
        );
    }

    // Fase 2: mutação atômica com um único checkpoint no histórico de Undo.
    session.checkpoint();
    session.sel_events.clear();

    // Primeiro as faixas de vídeo no topo (ordem visual do VEGAS Capture na timeline),
    // depois as faixas de áudio abaixo.
    let mut video_tracks: Vec<Track> = Vec::new();
    let mut audio_tracks: Vec<Track> = Vec::new();

    for (entry, imp) in ready {
        let item = imp.item;
        let media_id = item.id;
        let dur = item
            .duration
            .filter(|&d| d > Ticks::ZERO)
            .or_else(|| (entry.duration_ticks > 0).then(|| Ticks::from_raw(entry.duration_ticks)))
            .unwrap_or_else(|| Ticks::from_secs_i64(5));

        media_cache.insert(media_id, imp.entry);
        session.project.media.push(item.clone());
        report.imported_media_ids.push(media_id);

        let kind = if entry.is_video && item.has_video {
            TrackKind::Video
        } else if item.has_audio || !entry.is_video {
            TrackKind::Audio
        } else {
            TrackKind::Video
        };

        let mut tr = Track::new(kind, &entry.source_name);
        let ev = Event::new(&item, place_at, dur);
        let ev_id = ev.id;
        tr.events.push(ev);
        report.created_track_ids.push(tr.id);
        report.placed_event_ids.push(ev_id);
        session.sel_events.insert(ev_id);

        if kind == TrackKind::Video {
            video_tracks.push(tr);
        } else {
            audio_tracks.push(tr);
        }
    }

    // Insere as trilhas de vídeo novas logo antes das trilhas de áudio existentes
    // e as trilhas de áudio novas ao final, mantendo a organização Vídeo-acima / Áudio-abaixo.
    let first_audio = session
        .project
        .tracks
        .iter()
        .position(|t| t.kind == TrackKind::Audio)
        .unwrap_or(session.project.tracks.len());
    for (insert_at, vt) in (first_audio..).zip(video_tracks) {
        session.project.tracks.insert(insert_at, vt);
    }
    session.project.tracks.extend(audio_tracks);
    session.dirty = true;

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifesto_nvcap_salva_reabre_e_rejeita_entradas_hostis() {
        let dir = std::env::temp_dir().join(format!("neoveg_nvcap_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("sessao.nvcap");

        let m = CaptureProjectManifest::new(
            "2026-11-26T08:00:00Z".into(),
            Ticks::from_secs_i64(12),
            vec![CapturedTrackEntry {
                source_name: "Tela 1".into(),
                is_video: true,
                file: "tela_1.avi".into(),
                codec: "MJPEG (AVI)".into(),
                width: 1920,
                height: 1080,
                fps_num: 30,
                fps_den: 1,
                sample_rate: 0,
                channels: 0,
                duration_ticks: Ticks::from_secs_i64(12).0,
                frame_count: 360,
            }],
        );
        save_manifest(&p, &m).expect("salvar manifesto");
        let loaded = load_manifest(&p).expect("reabrir manifesto");
        assert_eq!(loaded, m);

        // Entradas hostis nunca entram em pânico (§0.4 e §0.8)
        assert!(parse_manifest_str("").is_err());
        assert!(parse_manifest_str("{broken").is_err());
        assert!(parse_manifest_str(r#"{"format":"outro","version":1,"created_at":""}"#).is_err());
        assert!(
            parse_manifest_str(r#"{"format":"neoveg_capture","version":999,"created_at":""}"#)
                .is_err()
        );
        assert!(resolve_track_file(&dir, "../segredo.txt").is_err());
        assert!(resolve_track_file(&dir, "sub/../../fora.avi").is_err());
        assert!(resolve_track_file(&dir, "ok\0nulo.avi").is_err());
        assert!(resolve_track_file(&dir, "tela_1.avi").is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
