//! Pacote do projeto `.neoveg`: um arquivo ZIP (especificação pública
//! *APPNOTE.TXT* da PKWARE) com
//!
//! - `manifest.json`: nome e versão do formato e a lista das sequências
//!   (id, entrada no pacote e qual é a ativa);
//! - `project.json`: o projeto sem o conteúdo das sequências (mídia, bins,
//!   barramentos, configurações...);
//! - `sequences/<nome>.nvegseq`: uma entrada por sequência, com as trilhas,
//!   eventos, marcadores e regiões dela.
//!
//! O projeto em memória não muda: o pacote é só a forma de gravá-lo. Projetos
//! antigos (JSON) e os arquivos de recuperação continuam abrindo pelo
//! [`crate::model::Project::from_bytes`].
//!
//! Todo pacote lido é hostil (AGENTS §0.4): o número de entradas, o tamanho de
//! cada uma e o total descompactado têm limite, os tamanhos declarados no ZIP
//! nunca são usados para alocar e o diretório central é conferido antes de
//! abrir o arquivo.

use crate::model::{Id, Project, Sequence, MAX_SEQUENCES};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{Cursor, Read, Write};

/// Extensão dos arquivos de sequência dentro do pacote.
pub const SEQUENCE_EXT: &str = "nvegseq";
/// Entrada com a lista das sequências.
pub const MANIFEST_ENTRY: &str = "manifest.json";
/// Entrada com o projeto sem o conteúdo das sequências.
pub const PROJECT_ENTRY: &str = "project.json";
/// Pasta das sequências dentro do pacote.
pub const SEQUENCES_DIR: &str = "sequences/";
/// Nome do formato no manifesto.
pub const PACKAGE_FORMAT: &str = "neoveg-package";
/// Nome do formato em cada `.nvegseq`.
pub const SEQUENCE_FORMAT: &str = "neoveg-sequence";
/// Versão do pacote. Um pacote de versão maior é recusado com erro claro.
pub const PACKAGE_VERSION: u32 = 1;
/// `Project::format_version` gravado nos pacotes.
pub const PACKAGE_PROJECT_FORMAT_VERSION: u32 = 3;
/// Maior entrada descompactada aceita.
pub const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
/// Maior total descompactado aceito (contra "bombas" de ZIP).
pub const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
/// Mais entradas aceitas no pacote.
pub const MAX_ENTRIES: usize = MAX_SEQUENCES + 16;
/// Maior nome de arquivo de sequência (caracteres, sem a extensão).
const MAX_ENTRY_NAME_CHARS: usize = 80;

const ZIP_LOCAL_MAGIC: &[u8] = b"PK\x03\x04";
const EOCD_MAGIC: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
const EOCD_LEN: usize = 22;

/// Lista das sequências do pacote.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub sequences: Vec<ManifestSequence>,
}

/// Uma sequência no manifesto. As sequências são casadas pelo `id`, nunca
/// pelo nome da entrada.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManifestSequence {
    pub id: Id,
    pub entry: String,
    #[serde(default)]
    pub active: bool,
}

/// Conteúdo de um arquivo `.nvegseq` (dentro do pacote `.neoveg` ou avulso).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SequenceFile {
    pub format: String,
    pub version: u32,
    pub sequence: Sequence,
    /// Mídias referenciadas pela sequência quando exportada como `.nvegseq` avulso.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<crate::model::MediaItem>,
}

/// Os bytes são um pacote ZIP (e não o JSON antigo)?
pub fn is_package(bytes: &[u8]) -> bool {
    bytes.starts_with(ZIP_LOCAL_MAGIC)
}

/// Nome da entrada `.nvegseq` de uma sequência, único no pacote (sem
/// diferenciar maiúsculas): "Sequência 01.nvegseq", "Sequência 01 (2).nvegseq"...
pub fn sequence_entry_name(name: &str, used: &mut HashSet<String>) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let base: String = cleaned
        .trim()
        .trim_matches('.')
        .chars()
        .take(MAX_ENTRY_NAME_CHARS)
        .collect();
    let base = if base.trim().is_empty() {
        "Sequência".to_string()
    } else {
        base.trim().to_string()
    };
    let mut n: u32 = 1;
    loop {
        let file = if n == 1 {
            format!("{base}.{SEQUENCE_EXT}")
        } else {
            format!("{base} ({n}).{SEQUENCE_EXT}")
        };
        if used.insert(file.to_lowercase()) {
            return format!("{SEQUENCES_DIR}{file}");
        }
        n = n.saturating_add(1);
    }
}

fn zip_err(e: zip::result::ZipError) -> String {
    format!("pacote .neoveg inválido: {e}")
}

/// Grava o projeto como pacote `.neoveg`.
pub fn write_package(p: &Project) -> Result<Vec<u8>, String> {
    let mut c = p.clone();
    c.ensure_sequences();
    let active = c.take_active_sequence(crate::time::Ticks::ZERO, crate::time::Ticks::ZERO, 100.0);
    let mut seqs: Vec<(Sequence, bool)> = vec![(active, true)];
    seqs.extend(
        std::mem::take(&mut c.sequences)
            .into_iter()
            .map(|s| (s, false)),
    );
    // na ordem das abas
    let order = c.sequence_order.clone();
    seqs.sort_by_key(|(s, _)| {
        order
            .iter()
            .position(|id| *id == s.id)
            .unwrap_or(usize::MAX)
    });
    c.format_version = c.format_version.max(PACKAGE_PROJECT_FORMAT_VERSION);

    let mut manifest = Manifest {
        format: PACKAGE_FORMAT.to_string(),
        version: PACKAGE_VERSION,
        sequences: Vec::with_capacity(seqs.len()),
    };
    let mut used = HashSet::new();
    let mut seq_entries = Vec::with_capacity(seqs.len());
    for (sequence, active) in seqs {
        let entry = sequence_entry_name(&sequence.name, &mut used);
        manifest.sequences.push(ManifestSequence {
            id: sequence.id,
            entry: entry.clone(),
            active,
        });
        let file = SequenceFile {
            format: SEQUENCE_FORMAT.to_string(),
            version: PACKAGE_VERSION,
            sequence,
            media: Vec::new(),
        };
        let json = serde_json::to_vec_pretty(&file)
            .map_err(|e| format!("erro ao serializar sequência: {e}"))?;
        seq_entries.push((entry, json));
    }
    let manifest_json = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| format!("erro ao serializar manifesto: {e}"))?;
    let project_json =
        serde_json::to_vec_pretty(&c).map_err(|e| format!("erro ao serializar projeto: {e}"))?;

    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    // compressão moderada: salvar é rápido mesmo em máquina fraca (§3.6)
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(3));
    let entries = [
        (MANIFEST_ENTRY.to_string(), manifest_json),
        (PROJECT_ENTRY.to_string(), project_json),
    ]
    .into_iter()
    .chain(seq_entries);
    for (name, data) in entries {
        zw.start_file(name, opts).map_err(zip_err)?;
        zw.write_all(&data)
            .map_err(|e| format!("erro ao gravar o pacote: {e}"))?;
    }
    let out = zw.finish().map_err(zip_err)?;
    Ok(out.into_inner())
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    let s = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// Confere o fim do diretório central antes de abrir o ZIP: número de
/// entradas dentro do limite, sem ZIP64 e diretório dentro do arquivo.
fn precheck_eocd(bytes: &[u8]) -> Result<(), String> {
    let bad = |m: &str| Err(format!("pacote .neoveg inválido: {m}"));
    if bytes.len() < EOCD_LEN {
        return bad("arquivo curto demais");
    }
    // o registro fica no fim, seguido de um comentário de até 64 KiB
    let start = bytes.len().saturating_sub(EOCD_LEN + u16::MAX as usize);
    let tail = bytes.get(start..).unwrap_or_default();
    let Some(pos) = (0..=tail.len().saturating_sub(EOCD_LEN))
        .rev()
        .find(|&i| tail.get(i..i + 4) == Some(&EOCD_MAGIC[..]))
    else {
        return bad("fim do diretório central não encontrado");
    };
    let e = tail.get(pos..pos + EOCD_LEN).unwrap_or_default();
    let (Some(total), Some(size), Some(offset)) = (u16_at(e, 10), u32_at(e, 12), u32_at(e, 16))
    else {
        return bad("fim do diretório central truncado");
    };
    if total == u16::MAX || size == u32::MAX || offset == u32::MAX {
        return bad("ZIP64 não é aceito");
    }
    if usize::from(total) > MAX_ENTRIES {
        return bad("entradas demais");
    }
    let eocd_at = (start + pos) as u64;
    if u64::from(offset) + u64::from(size) > eocd_at {
        return bad("diretório central fora do arquivo");
    }
    Ok(())
}

/// Lê uma entrada inteira, com limite (o tamanho declarado no ZIP não é usado).
fn read_entry(
    za: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
    total: &mut u64,
) -> Result<Option<Vec<u8>>, String> {
    let f = match za.by_name(name) {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(zip_err(e)),
    };
    let mut buf = Vec::new();
    f.take(MAX_ENTRY_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("pacote .neoveg inválido: {name}: {e}"))?;
    let len = buf.len() as u64;
    if len > MAX_ENTRY_BYTES {
        return Err(format!(
            "pacote .neoveg inválido: {name} passa de {} MiB",
            MAX_ENTRY_BYTES / (1024 * 1024)
        ));
    }
    *total = total.saturating_add(len);
    if *total > MAX_TOTAL_BYTES {
        return Err(format!(
            "pacote .neoveg inválido: conteúdo passa de {} MiB",
            MAX_TOTAL_BYTES / (1024 * 1024)
        ));
    }
    Ok(Some(buf))
}

/// Lê um pacote `.neoveg`. Devolve o projeto montado (ainda sem `sanitize`,
/// que o chamador faz) e os avisos do que foi descartado.
pub fn read_package(bytes: &[u8]) -> Result<(Project, Vec<String>), String> {
    precheck_eocd(bytes)?;
    let mut za = zip::ZipArchive::new(Cursor::new(bytes)).map_err(zip_err)?;
    if za.len() > MAX_ENTRIES {
        return Err("pacote .neoveg inválido: entradas demais".into());
    }
    let mut total = 0u64;
    let mut fixes = Vec::new();

    let manifest_bytes = read_entry(&mut za, MANIFEST_ENTRY, &mut total)?
        .ok_or_else(|| "pacote .neoveg inválido: falta manifest.json".to_string())?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| format!("pacote .neoveg inválido: manifest.json: {e}"))?;
    if manifest.format != PACKAGE_FORMAT {
        return Err("o arquivo não é um pacote de projeto do Neoveg".into());
    }
    if manifest.version > PACKAGE_VERSION {
        return Err(format!(
            "o projeto foi salvo por uma versão mais nova do Neoveg (pacote versão {}); atualize o Neoveg para abri-lo",
            manifest.version
        ));
    }

    let project_bytes = read_entry(&mut za, PROJECT_ENTRY, &mut total)?
        .ok_or_else(|| "pacote .neoveg inválido: falta project.json".to_string())?;
    let mut p: Project =
        serde_json::from_slice(&project_bytes).map_err(|e| format!("projeto inválido: {e}"))?;
    // o conteúdo das sequências vem só das entradas .nvegseq
    p.sequences.clear();

    let mut active: Option<Sequence> = None;
    let mut others = Vec::new();
    let mut seen = HashSet::new();
    for m in manifest.sequences.iter().take(MAX_SEQUENCES + 1) {
        if !seen.insert(m.id) {
            fixes.push(format!(
                "sequência repetida no manifesto ignorada: {}",
                m.entry
            ));
            continue;
        }
        let Some(data) = read_entry(&mut za, &m.entry, &mut total)? else {
            fixes.push(format!("sequência ausente no pacote: {}", m.entry));
            continue;
        };
        let file: SequenceFile = match serde_json::from_slice(&data) {
            Ok(f) => f,
            Err(e) => {
                fixes.push(format!("sequência ilegível {}: {e}", m.entry));
                continue;
            }
        };
        if file.format != SEQUENCE_FORMAT || file.version > PACKAGE_VERSION {
            fixes.push(format!(
                "sequência em formato desconhecido ignorada: {}",
                m.entry
            ));
            continue;
        }
        let mut seq = file.sequence;
        if seq.id != m.id {
            fixes.push(format!(
                "id da sequência {} corrigido pelo manifesto",
                m.entry
            ));
            seq.id = m.id;
        }
        if m.active && active.is_none() {
            active = Some(seq);
        } else {
            others.push(seq);
        }
    }
    if manifest.sequences.len() > MAX_SEQUENCES + 1 {
        fixes.push(format!("sequências limitadas ao máximo de {MAX_SEQUENCES}"));
    }
    if active.is_none() && !others.is_empty() {
        active = Some(others.remove(0));
        fixes.push("nenhuma sequência ativa no pacote: a primeira foi aberta".into());
    }
    if let Some(seq) = active {
        p.install_active_sequence(seq);
    }
    p.sequences = others;
    Ok((p, fixes))
}

/// Exporta uma sequência do projeto (ativa ou inativa) para um arquivo `.nvegseq`
/// avulso contendo a sequência e as definições de mídia que seus eventos referenciam.
pub fn export_standalone_sequence(p: &Project, seq_id: Id) -> Result<Vec<u8>, String> {
    let mut copy = p.clone();
    copy.ensure_sequences();
    let seq = if copy.active_sequence_id == Some(seq_id) {
        copy.take_active_sequence(crate::time::Ticks::ZERO, crate::time::Ticks::ZERO, 100.0)
    } else {
        copy.sequences
            .into_iter()
            .find(|s| s.id == seq_id)
            .ok_or_else(|| format!("sequência {seq_id} não encontrada no projeto"))?
    };

    let mut used_media = HashSet::new();
    for tr in &seq.tracks {
        for ev in &tr.events {
            used_media.insert(ev.media_id);
            for tk in &ev.alt_takes {
                used_media.insert(tk.media_id);
            }
        }
    }
    let media: Vec<crate::model::MediaItem> = p
        .media
        .iter()
        .filter(|m| used_media.contains(&m.id))
        .cloned()
        .collect();

    let file = SequenceFile {
        format: SEQUENCE_FORMAT.to_string(),
        version: PACKAGE_VERSION,
        sequence: seq,
        media,
    };
    serde_json::to_vec_pretty(&file).map_err(|e| format!("erro ao serializar .nvegseq: {e}"))
}

/// Grava a sequência `seq_id` (ou a ativa se `None`) em um arquivo `.nvegseq` avulso.
pub fn export_standalone_sequence_file(
    p: &Project,
    seq_id: Option<Id>,
    path: &std::path::Path,
) -> Result<(), String> {
    let mut c = p.clone();
    c.ensure_sequences();
    let id = seq_id
        .or(c.active_sequence_id)
        .ok_or_else(|| "projeto sem sequência ativa".to_string())?;
    let bytes = export_standalone_sequence(&c, id)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("não foi possível criar pasta de destino: {e}"))?;
        }
    }
    let tmp = path.with_extension("nvegseq.tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("erro ao gravar .nvegseq: {e}"))?;
    std::fs::rename(&tmp, path)
        .or_else(|_| std::fs::write(path, &bytes))
        .map_err(|e| format!("erro ao finalizar .nvegseq: {e}"))
}

/// Lê e valida um arquivo `.nvegseq` avulso tratando toda entrada como hostil (§0.4).
pub fn read_standalone_sequence(
    bytes: &[u8],
) -> Result<(Sequence, Vec<crate::model::MediaItem>), String> {
    if bytes.is_empty() {
        return Err("arquivo .nvegseq vazio".into());
    }
    if (bytes.len() as u64) > MAX_ENTRY_BYTES {
        return Err(format!(
            "arquivo .nvegseq excede o limite de {} MiB",
            MAX_ENTRY_BYTES / (1024 * 1024)
        ));
    }
    let file: SequenceFile =
        serde_json::from_slice(bytes).map_err(|e| format!("arquivo .nvegseq inválido: {e}"))?;
    if file.format != SEQUENCE_FORMAT {
        return Err("o arquivo não é uma sequência .nvegseq do Neoveg".into());
    }
    if file.version > PACKAGE_VERSION {
        return Err(format!(
            "sequência salva por uma versão mais nova do Neoveg (versão {})",
            file.version
        ));
    }
    let mut seq = file.sequence;
    if seq.tracks.len() > 256 {
        seq.tracks.truncate(256);
    }
    if seq.markers.len() > 10_000 {
        seq.markers.truncate(10_000);
    }
    if seq.regions.len() > 10_000 {
        seq.regions.truncate(10_000);
    }
    let mut media = file.media;
    if media.len() > 4096 {
        media.truncate(4096);
    }
    Ok((seq, media))
}

/// Importa uma sequência `.nvegseq` avulsa em `p`, remapeando todos os IDs e
/// incorporando/reaproveitando mídias referenciadas sem colisões. Devolve o novo ID.
pub fn import_standalone_sequence_into(
    p: &mut Project,
    mut seq: Sequence,
    media_items: Vec<crate::model::MediaItem>,
) -> Result<Id, String> {
    use std::collections::HashMap;
    p.ensure_sequences();
    let total = 1 + p.sequences.len();
    if total >= MAX_SEQUENCES {
        return Err(format!(
            "limite máximo de {MAX_SEQUENCES} sequências no projeto atingido"
        ));
    }

    // Mapeia IDs de mídia antigos → existentes no projeto (mesmo `source`) ou novos.
    let mut media_map: HashMap<Id, Id> = HashMap::new();
    for mut m in media_items {
        let old_id = m.id;
        let m_src = serde_json::to_string(&m.source).ok();
        if let Some(existing) = p
            .media
            .iter()
            .find(|em| m_src.is_some() && serde_json::to_string(&em.source).ok() == m_src)
        {
            media_map.insert(old_id, existing.id);
        } else {
            let nid = crate::model::new_id();
            m.id = nid;
            p.media.push(m);
            media_map.insert(old_id, nid);
        }
    }

    // Remapeia IDs de trilhas, eventos, grupos, sync links, marcadores e regiões.
    let mut track_map: HashMap<Id, Id> = HashMap::new();
    let mut event_map: HashMap<Id, Id> = HashMap::new();
    let mut group_map: HashMap<Id, Id> = HashMap::new();
    for tr in &mut seq.tracks {
        let nid = crate::model::new_id();
        track_map.insert(tr.id, nid);
        tr.id = nid;
        for ev in &mut tr.events {
            let neid = crate::model::new_id();
            event_map.insert(ev.id, neid);
            ev.id = neid;
            if let Some(&nm) = media_map.get(&ev.media_id) {
                ev.media_id = nm;
            }
            for tk in &mut ev.alt_takes {
                if let Some(&nm) = media_map.get(&tk.media_id) {
                    tk.media_id = nm;
                }
            }
            if let Some(g) = ev.group {
                let ng = *group_map.entry(g).or_insert_with(crate::model::new_id);
                ev.group = Some(ng);
            }
            for fx in &mut ev.fx {
                fx.id = crate::model::new_id();
            }
        }
        for fx in &mut tr.fx {
            fx.id = crate::model::new_id();
        }
        tr.output_bus = None;
        tr.sends.clear();
    }
    for tr in &mut seq.tracks {
        tr.parent_track_id = tr
            .parent_track_id
            .and_then(|pid| track_map.get(&pid).copied());
        for ev in &mut tr.events {
            ev.sync_link_master = ev
                .sync_link_master
                .and_then(|mid| event_map.get(&mid).copied());
        }
    }
    for m in &mut seq.markers {
        m.id = crate::model::new_id();
    }
    for r in &mut seq.regions {
        r.id = crate::model::new_id();
    }

    // Garante nome único entre as sequências do projeto.
    let base_name = seq.name.trim().chars().take(64).collect::<String>();
    let base_name = if base_name.is_empty() {
        "Sequência importada".to_string()
    } else {
        base_name
    };
    let existing_names: HashSet<String> = p
        .sequences
        .iter()
        .map(|s| s.name.to_lowercase())
        .chain(p.active_sequence_name.iter().map(|n| n.to_lowercase()))
        .collect();
    let mut final_name = base_name.clone();
    let mut suffix = 2u32;
    while existing_names.contains(&final_name.to_lowercase()) {
        final_name = format!("{base_name} ({suffix})");
        suffix = suffix.saturating_add(1);
    }

    let new_seq_id = crate::model::new_id();
    seq.id = new_seq_id;
    seq.name = final_name;
    p.sequences.push(seq);
    if !p.sequence_order.contains(&new_seq_id) {
        p.sequence_order.push(new_seq_id);
    }
    p.sanitize();
    Ok(new_seq_id)
}

/// Lê um arquivo `.nvegseq` do disco (com limite de tamanho) e o importa no projeto `p`.
pub fn import_standalone_sequence_file(
    p: &mut Project,
    path: &std::path::Path,
) -> Result<Id, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("erro ao abrir .nvegseq: {e}"))?;
    let mut bytes = Vec::new();
    f.take(MAX_ENTRY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("erro ao ler .nvegseq: {e}"))?;
    let (seq, media) = read_standalone_sequence(&bytes)?;
    import_standalone_sequence_into(p, seq, media)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Marker, Track, TrackKind};
    use crate::time::Ticks;

    fn projeto_com_tres_sequencias() -> Project {
        let mut p = Project::new_default();
        p.markers.push(Marker {
            id: crate::model::new_id(),
            t: Ticks::from_secs_i64(1),
            label: "um".into(),
            ..Default::default()
        });
        p.create_sequence(None);
        p.tracks.push(Track::new(TrackKind::Video, "Extra"));
        p.create_sequence(Some("Sequência 01"));
        p
    }

    #[test]
    fn ida_e_volta_mantem_o_projeto() {
        let p = projeto_com_tres_sequencias();
        let bytes = write_package(&p).unwrap();
        assert!(is_package(&bytes));
        let (mut back, fixes) = read_package(&bytes).unwrap();
        assert!(fixes.is_empty(), "{fixes:?}");
        back.format_version = p.format_version;
        let a = serde_json::to_value(&p).unwrap();
        let b = serde_json::to_value(&back).unwrap();
        // as inativas podem vir em outra ordem: compare por id
        let ids = |v: &serde_json::Value| {
            let mut ids: Vec<i64> = v["sequences"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["id"].as_i64().unwrap())
                .collect();
            ids.sort_unstable();
            ids
        };
        assert_eq!(ids(&a), ids(&b));
        for k in [
            "tracks",
            "markers",
            "media",
            "settings",
            "active_sequence_id",
            "active_sequence_name",
            "sequence_order",
        ] {
            assert_eq!(a[k], b[k], "campo {k}");
        }
    }

    #[test]
    fn json_antigo_e_pacote_abrem_igual() {
        let p = projeto_com_tres_sequencias();
        let json = serde_json::to_string(&p).unwrap();
        let (velho, _) = Project::from_bytes(json.as_bytes()).unwrap();
        let (novo, _) = Project::from_bytes(&write_package(&p).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(&velho.tracks).unwrap(),
            serde_json::to_value(&novo.tracks).unwrap()
        );
        assert_eq!(velho.sequence_tab_info(), novo.sequence_tab_info());
    }

    #[test]
    fn nomes_das_entradas_unicos_e_limpos() {
        let mut used = HashSet::new();
        assert_eq!(
            sequence_entry_name("Sequência 01", &mut used),
            "sequences/Sequência 01.nvegseq"
        );
        assert_eq!(
            sequence_entry_name("sequência 01", &mut used),
            "sequences/sequência 01 (2).nvegseq"
        );
        assert_eq!(
            sequence_entry_name("../a/b:c\0", &mut used),
            "sequences/_a_b_c_.nvegseq"
        );
        assert_eq!(
            sequence_entry_name("   ", &mut used),
            "sequences/Sequência.nvegseq"
        );
        let longo = "x".repeat(500);
        assert!(sequence_entry_name(&longo, &mut used).chars().count() < 120);
    }

    #[test]
    fn abas_mostram_o_arquivo_da_sequencia() {
        assert_eq!(
            crate::model::sequence_file_label("Sequência 01"),
            "Sequência 01.nvegseq"
        );
        let p = Project::new_default();
        assert_eq!(
            p.sequence_tab_info()[0].1,
            crate::model::DEFAULT_SEQUENCE_NAME
        );
    }

    fn pacote_com(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default();
        for (n, d) in entries {
            zw.start_file(*n, o).unwrap();
            zw.write_all(d).unwrap();
        }
        zw.finish().unwrap().into_inner()
    }

    #[test]
    fn pacotes_hostis_viram_erro_ou_aviso() {
        let p = Project::new_default();
        let pj = serde_json::to_vec(&p).unwrap();
        // sem manifesto / sem projeto
        assert!(read_package(&pacote_com(&[(PROJECT_ENTRY, &pj)])).is_err());
        let man = br#"{"format":"neoveg-package","version":1,"sequences":[]}"#;
        assert!(read_package(&pacote_com(&[(MANIFEST_ENTRY, man)])).is_err());
        // outro formato e versão mais nova
        let outro = br#"{"format":"outra-coisa","version":1}"#;
        assert!(read_package(&pacote_com(&[
            (MANIFEST_ENTRY, outro),
            (PROJECT_ENTRY, &pj)
        ]))
        .is_err());
        let nova = br#"{"format":"neoveg-package","version":99}"#;
        let e =
            read_package(&pacote_com(&[(MANIFEST_ENTRY, nova), (PROJECT_ENTRY, &pj)])).unwrap_err();
        assert!(e.contains("mais nova"), "{e}");
        // entrada que falta, repetida e ilegível: avisos, o projeto abre
        let man = br#"{"format":"neoveg-package","version":1,"sequences":[
            {"id":7,"entry":"sequences/falta.nvegseq","active":true},
            {"id":8,"entry":"sequences/lixo.nvegseq"},
            {"id":8,"entry":"sequences/lixo.nvegseq"}]}"#;
        let (_, fixes) = read_package(&pacote_com(&[
            (MANIFEST_ENTRY, man),
            (PROJECT_ENTRY, &pj),
            ("sequences/lixo.nvegseq", b"{nao e json"),
        ]))
        .unwrap();
        assert_eq!(fixes.len(), 3, "{fixes:?}");
        // lixo e arquivos curtos
        assert!(read_package(b"PK\x03\x04lixo").is_err());
        assert!(read_package(b"").is_err());
        assert!(Project::from_bytes(&[0xff, 0xfe, 0x00]).is_err());
    }

    #[test]
    fn diretorio_central_hostil_e_recusado() {
        let p = Project::new_default();
        let mut bytes = write_package(&p).unwrap();
        let n = bytes.len();
        // número de entradas declarado enorme
        let mut b1 = bytes.clone();
        b1[n - 12] = 0xff;
        b1[n - 11] = 0xfe;
        assert!(read_package(&b1).is_err());
        // ZIP64 (marcadores 0xFFFF)
        let mut b2 = bytes.clone();
        b2[n - 12] = 0xff;
        b2[n - 11] = 0xff;
        assert!(read_package(&b2).is_err());
        // diretório central apontando para fora do arquivo
        bytes[n - 6] = 0xff;
        bytes[n - 5] = 0xff;
        bytes[n - 4] = 0xff;
        bytes[n - 3] = 0x7f;
        assert!(read_package(&bytes).is_err());
    }

    #[test]
    fn bomba_de_zip_e_recusada() {
        // 80 MiB de zeros compactam para pouco, mas passam do limite por entrada
        let zeros = vec![0u8; (MAX_ENTRY_BYTES + 1024) as usize];
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zw.start_file(MANIFEST_ENTRY, o).unwrap();
        zw.write_all(&zeros).unwrap();
        let bytes = zw.finish().unwrap().into_inner();
        assert!(bytes.len() < 1024 * 1024);
        let e = read_package(&bytes).unwrap_err();
        assert!(e.contains("MiB"), "{e}");
    }

    #[test]
    fn salvar_e_abrir_pelo_arquivo() {
        let dir = std::env::temp_dir().join(format!("neoveg_pkg_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Meu projeto.neoveg");
        let p = projeto_com_tres_sequencias();
        p.save_file_with_backup(&path, false, 0).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(is_package(&bytes), "salvar grava o pacote");
        let (back, _) = Project::load_file(&path).unwrap();
        assert_eq!(back.sequence_tab_info().len(), 3);
        // o arquivo de recuperação (JSON) continua abrindo
        let rec = dir.join("recuperacao.neoveg");
        std::fs::write(&rec, serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(
            Project::load_file(&rec)
                .unwrap()
                .0
                .sequence_tab_info()
                .len(),
            3
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sequencia_avulsa_nvegseq_ida_e_volta_e_reimportacao_sem_colisao() {
        use crate::model::{Event, GenSpec, MediaItem, MediaSource};

        let mut src = Project::new_default();
        src.active_sequence_name = Some("Corte Principal".into());
        let media = MediaItem {
            id: crate::model::new_id(),
            name: "Cor".into(),
            source: MediaSource::Generated(GenSpec::SolidColor {
                color: [0.2, 0.4, 0.8, 1.0],
            }),
            duration: Some(Ticks::from_secs_i64(5)),
            has_video: true,
            has_audio: false,
            width: 1920,
            height: 1080,
            fps: None,
            info: String::new(),
            proxy_path: None,
        };
        let mut ev = Event::new(&media, Ticks::from_secs_i64(1), Ticks::from_secs_i64(3));
        ev.group = Some(crate::model::new_id());
        src.media.push(media);
        src.tracks[0].events.push(ev);
        src.markers.push(Marker {
            id: crate::model::new_id(),
            t: Ticks::from_secs_i64(2),
            label: "M1".into(),
            ..Default::default()
        });

        let seq_id = src.active_sequence_id.unwrap();
        let bytes = export_standalone_sequence(&src, seq_id).unwrap();

        let mut dst = Project::new_default();
        dst.active_sequence_name = Some("Corte Principal".into());
        let (s1, m1) = read_standalone_sequence(&bytes).unwrap();
        assert_eq!(m1.len(), 1);
        let id1 = import_standalone_sequence_into(&mut dst, s1, m1).unwrap();
        assert_eq!(dst.sequences.len(), 1);
        assert_eq!(dst.media.len(), 1);
        assert_eq!(dst.sequences[0].name, "Corte Principal (2)");
        assert_eq!(dst.sequences[0].id, id1);
        assert_eq!(
            dst.sequences[0].tracks[0].events[0].media_id,
            dst.media[0].id
        );

        // Reimportar o mesmo arquivo reutiliza a mídia sem duplicá-la e dá novo nome/IDs
        let (s2, m2) = read_standalone_sequence(&bytes).unwrap();
        let id2 = import_standalone_sequence_into(&mut dst, s2, m2).unwrap();
        assert_ne!(id1, id2);
        assert_eq!(dst.media.len(), 1);
        assert_eq!(dst.sequences.len(), 2);
        assert_eq!(dst.sequences[1].name, "Corte Principal (3)");
        assert_ne!(
            dst.sequences[0].tracks[0].events[0].id,
            dst.sequences[1].tracks[0].events[0].id
        );
    }

    #[test]
    fn sequencia_avulsa_nvegseq_entradas_hostis_e_fuzz() {
        assert!(read_standalone_sequence(b"").is_err());
        assert!(read_standalone_sequence(br#"{"format":"outro","version":1}"#).is_err());
        let p = Project::new_default();
        let valid = export_standalone_sequence(&p, p.active_sequence_id.unwrap()).unwrap();
        // Versão futura
        let newer = String::from_utf8_lossy(&valid).replace("\"version\": 1", "\"version\": 99");
        assert!(read_standalone_sequence(newer.as_bytes()).is_err());

        // Mutações por truncamento e inversão de bits nunca causam pânico (§0.8)
        for cut in [
            1usize,
            5,
            17,
            valid.len() / 2,
            valid.len().saturating_sub(1),
        ] {
            let slice = &valid[..cut.min(valid.len())];
            let res = std::panic::catch_unwind(|| read_standalone_sequence(slice));
            assert!(res.is_ok());
        }
        for i in (0..valid.len()).step_by(29) {
            let mut mutated = valid.clone();
            mutated[i] ^= 0x5A;
            let res = std::panic::catch_unwind(|| read_standalone_sequence(&mutated));
            assert!(res.is_ok());
        }
    }
}
