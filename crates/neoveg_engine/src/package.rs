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

/// Conteúdo de um arquivo `.nvegseq`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SequenceFile {
    pub format: String,
    pub version: u32,
    pub sequence: Sequence,
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
}
