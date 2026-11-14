//! Descoberta de plug-ins externos **VST3** e **OpenFX (OFX)** nas pastas
//! padrão do sistema, a partir da documentação pública de empacotamento de
//! cada formato:
//!
//! - VST3: pacote `Nome.vst3/Contents/<arquitetura>/Nome.vst3` (ou arquivo
//!   único `Nome.vst3` legado) e metadados opcionais
//!   `Contents/Resources/moduleinfo.json`;
//! - OFX: pacote `Nome.ofx.bundle/Contents/<Win64|Linux-x86-64|MacOS>/Nome.ofx`
//!   e pastas extras na variável `OFX_PLUGIN_PATH`.
//!
//! Este módulo só lista arquivos e lê metadados declarativos: nenhum binário
//! de plug-in é carregado ou executado aqui. A hospedagem (carregar e
//! processar) exige um processo isolado (`AGENTS.md` §0.3 e §0.6) e ainda não
//! está implementada.

use std::path::{Path, PathBuf};

/// Máximo de plug-ins listados por pasta (entrada hostil, §0.4).
const MAX_PLUGINS_PER_DIR: usize = 2000;
/// Tamanho máximo do `moduleinfo.json` lido.
const MAX_MODULEINFO_BYTES: u64 = 1024 * 1024;
const MAX_CLASSES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExternalKind {
    Vst3,
    Ofx,
}

impl ExternalKind {
    pub fn name(self) -> &'static str {
        match self {
            ExternalKind::Vst3 => "VST3",
            ExternalKind::Ofx => "OFX",
        }
    }
}

/// Plug-in externo encontrado no disco (ainda não carregado).
#[derive(Clone, Debug, PartialEq)]
pub struct ExternalPlugin {
    pub kind: ExternalKind,
    pub name: String,
    pub vendor: Option<String>,
    pub version: Option<String>,
    /// Pacote (pasta) ou arquivo do plug-in.
    pub path: PathBuf,
    /// Binário da arquitetura atual dentro do pacote, se existir.
    pub binary: Option<PathBuf>,
    /// Classes declaradas no `moduleinfo.json` (VST3), com categoria.
    pub classes: Vec<String>,
}

/// Subpasta de binários da arquitetura atual dentro de um pacote VST3.
fn vst3_arch_dir() -> &'static str {
    if cfg!(target_os = "windows") {
        if cfg!(target_arch = "aarch64") {
            "arm64-win"
        } else {
            "x86_64-win"
        }
    } else if cfg!(target_os = "macos") {
        "MacOS"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64-linux"
    } else {
        "x86_64-linux"
    }
}

/// Subpasta de binários da arquitetura atual dentro de um pacote OFX.
fn ofx_arch_dir() -> &'static str {
    if cfg!(target_os = "windows") {
        "Win64"
    } else if cfg!(target_os = "macos") {
        "MacOS"
    } else {
        "Linux-x86-64"
    }
}

/// Pastas padrão de busca, por formato.
pub fn default_search_dirs() -> Vec<(ExternalKind, PathBuf)> {
    let mut dirs = vec![];
    if cfg!(target_os = "windows") {
        if let Some(common) = std::env::var_os("CommonProgramFiles") {
            let common = PathBuf::from(common);
            dirs.push((ExternalKind::Vst3, common.join("VST3")));
            dirs.push((ExternalKind::Ofx, common.join("OFX").join("Plugins")));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push((
                ExternalKind::Vst3,
                PathBuf::from(local)
                    .join("Programs")
                    .join("Common")
                    .join("VST3"),
            ));
        }
    } else if cfg!(target_os = "macos") {
        dirs.push((
            ExternalKind::Vst3,
            PathBuf::from("/Library/Audio/Plug-Ins/VST3"),
        ));
        dirs.push((ExternalKind::Ofx, PathBuf::from("/Library/OFX/Plugins")));
    } else {
        dirs.push((ExternalKind::Vst3, PathBuf::from("/usr/lib/vst3")));
        dirs.push((ExternalKind::Vst3, PathBuf::from("/usr/local/lib/vst3")));
        dirs.push((ExternalKind::Ofx, PathBuf::from("/usr/OFX/Plugins")));
    }
    if let Some(extra) = std::env::var_os("OFX_PLUGIN_PATH") {
        for p in std::env::split_paths(&extra) {
            if !p.as_os_str().is_empty() {
                dirs.push((ExternalKind::Ofx, p));
            }
        }
    }
    dirs
}

fn stem_name(path: &Path, suffix: &str) -> String {
    let file = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    let lower = file.to_ascii_lowercase();
    if lower.ends_with(suffix) {
        file.get(..file.len().saturating_sub(suffix.len()))
            .unwrap_or(&file)
            .to_string()
    } else {
        file
    }
}

/// Remove vírgulas finais antes de `}` ou `]`, fora de strings. O
/// `moduleinfo.json` gerado pelo VST 3 SDK oficial usa vírgulas finais, que o
/// JSON estrito não aceita.
fn strip_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let (mut in_str, mut escaped) = (false, false);
    for (i, &c) in chars.iter().enumerate() {
        if in_str {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
            }
            ',' => {
                let next = chars
                    .get(i + 1..)
                    .and_then(|rest| rest.iter().find(|n| !n.is_whitespace()));
                if !matches!(next, Some('}') | Some(']')) {
                    out.push(c);
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Lê os metadados de `moduleinfo.json` (tolerante: JSON inválido só é ignorado).
fn read_moduleinfo(bundle: &Path, plugin: &mut ExternalPlugin) {
    let path = bundle
        .join("Contents")
        .join("Resources")
        .join("moduleinfo.json");
    let Ok(meta) = std::fs::metadata(&path) else {
        return;
    };
    if meta.len() > MAX_MODULEINFO_BYTES {
        return;
    }
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&strip_trailing_commas(&text)) else {
        return;
    };
    let s = |v: &serde_json::Value| v.as_str().map(|s| s.trim().to_string());
    if let Some(n) = v.get("Name").and_then(s).filter(|n| !n.is_empty()) {
        plugin.name = n;
    }
    plugin.version = v.get("Version").and_then(s);
    plugin.vendor = v
        .get("Factory Info")
        .and_then(|f| f.get("Vendor"))
        .and_then(s);
    if let Some(classes) = v.get("Classes").and_then(|c| c.as_array()) {
        plugin.classes = classes
            .iter()
            .take(MAX_CLASSES)
            .filter_map(|c| {
                let name = c.get("Name").and_then(s)?;
                match c.get("Category").and_then(s) {
                    Some(cat) => Some(format!("{name} ({cat})")),
                    None => Some(name),
                }
            })
            .collect();
    }
}

fn scan_vst3_entry(path: &Path) -> Option<ExternalPlugin> {
    let lower = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if !lower.ends_with(".vst3") {
        return None;
    }
    let name = stem_name(path, ".vst3");
    let mut plugin = ExternalPlugin {
        kind: ExternalKind::Vst3,
        name: name.clone(),
        vendor: None,
        version: None,
        path: path.to_path_buf(),
        binary: None,
        classes: vec![],
    };
    if path.is_dir() {
        let bin = path
            .join("Contents")
            .join(vst3_arch_dir())
            .join(format!("{name}.vst3"));
        plugin.binary = bin.is_file().then_some(bin);
        read_moduleinfo(path, &mut plugin);
    } else if path.is_file() {
        plugin.binary = Some(path.to_path_buf());
    }
    Some(plugin)
}

fn scan_ofx_entry(path: &Path) -> Option<ExternalPlugin> {
    let lower = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if !lower.ends_with(".ofx.bundle") || !path.is_dir() {
        return None;
    }
    let name = stem_name(path, ".ofx.bundle");
    let bin = path
        .join("Contents")
        .join(ofx_arch_dir())
        .join(format!("{name}.ofx"));
    Some(ExternalPlugin {
        kind: ExternalKind::Ofx,
        name,
        vendor: None,
        version: None,
        path: path.to_path_buf(),
        binary: bin.is_file().then_some(bin),
        classes: vec![],
    })
}

/// Profundidade máxima de subpastas varridas (fabricantes costumam usar
/// `VST3/<Fabricante>/<Categoria>/`).
const MAX_SCAN_DEPTH: usize = 8;
/// Máximo de entradas de diretório visitadas por varredura (§0.4).
const MAX_SCAN_ENTRIES: usize = 50_000;

fn is_bundle_name(kind: ExternalKind, name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    match kind {
        ExternalKind::Vst3 => n.ends_with(".vst3"),
        ExternalKind::Ofx => n.ends_with(".ofx.bundle"),
    }
}

/// Lista os plug-ins de um formato numa pasta e nas subpastas dela, como pede a
/// documentação de cada formato. Pacotes (`.vst3`, `.ofx.bundle`) não são
/// abertos como pastas; links simbólicos não são seguidos; profundidade e
/// número de entradas são limitados. Pasta ausente devolve lista vazia.
pub fn scan_dir(kind: ExternalKind, dir: &Path) -> Vec<ExternalPlugin> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    let mut visited = 0usize;
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.filter_map(Result::ok) {
            visited += 1;
            if visited > MAX_SCAN_ENTRIES || out.len() >= MAX_PLUGINS_PER_DIR {
                stack.clear();
                break;
            }
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            // `file_type` não segue links: um link simbólico nunca é descido
            let Ok(ft) = e.file_type() else {
                continue;
            };
            if is_bundle_name(kind, &name) {
                let found = match kind {
                    ExternalKind::Vst3 => scan_vst3_entry(&p),
                    ExternalKind::Ofx => scan_ofx_entry(&p),
                };
                out.extend(found);
            } else if ft.is_dir() && depth < MAX_SCAN_DEPTH {
                stack.push((p, depth + 1));
            }
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// Varre todas as pastas padrão, sem duplicar o mesmo pacote.
pub fn scan_default() -> Vec<ExternalPlugin> {
    let mut out: Vec<ExternalPlugin> = vec![];
    for (kind, dir) in default_search_dirs() {
        for p in scan_dir(kind, &dir) {
            if !out.iter().any(|o| o.path == p.path) {
                out.push(p);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("neoveg_plugins_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("cria pasta temporária");
        d
    }

    #[test]
    fn finds_vst3_bundle_with_moduleinfo() {
        let d = tmp_dir("vst3");
        let bundle = d.join("Eco.vst3");
        let bin_dir = bundle.join("Contents").join(vst3_arch_dir());
        std::fs::create_dir_all(&bin_dir).expect("cria pacote");
        std::fs::write(bin_dir.join("Eco.vst3"), b"nao e um binario").expect("grava");
        let res = bundle.join("Contents").join("Resources");
        std::fs::create_dir_all(&res).expect("cria recursos");
        std::fs::write(
            res.join("moduleinfo.json"),
            r#"{"Name": "Eco Teste", "Version": "1.2.0",
                "Factory Info": {"Vendor": "Exemplo"},
                "Classes": [{"Name": "Eco", "Category": "Audio Module Class"}]}"#,
        )
        .expect("grava moduleinfo");
        std::fs::write(d.join("legado.vst3"), b"x").expect("grava legado");
        std::fs::write(d.join("leia-me.txt"), b"x").expect("grava texto");

        let found = scan_dir(ExternalKind::Vst3, &d);
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(found.len(), 2, "{found:?}");
        let eco = found.iter().find(|p| p.name == "Eco Teste").expect("eco");
        assert_eq!(eco.vendor.as_deref(), Some("Exemplo"));
        assert_eq!(eco.version.as_deref(), Some("1.2.0"));
        assert!(eco.binary.is_some());
        assert_eq!(eco.classes, vec!["Eco (Audio Module Class)".to_string()]);
        assert!(found.iter().any(|p| p.name == "legado"));
    }

    #[test]
    fn finds_ofx_bundle_and_ignores_other_entries() {
        let d = tmp_dir("ofx");
        let bundle = d.join("Brilho.ofx.bundle");
        let bin_dir = bundle.join("Contents").join(ofx_arch_dir());
        std::fs::create_dir_all(&bin_dir).expect("cria pacote");
        std::fs::write(bin_dir.join("Brilho.ofx"), b"x").expect("grava");
        std::fs::create_dir_all(d.join("Vazio.ofx.bundle")).expect("pacote sem binário");
        std::fs::write(d.join("solto.ofx"), b"x").expect("arquivo solto");

        let found = scan_dir(ExternalKind::Ofx, &d);
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].binary.is_some());
        assert_eq!(found[0].name, "Brilho");
        assert!(found[1].binary.is_none());
    }

    /// Regressão: o `moduleinfo.json` do VST 3 SDK oficial tem vírgulas finais;
    /// antes da correção o JSON era rejeitado e o plug-in ficava sem metadados.
    #[test]
    fn moduleinfo_with_trailing_commas_is_read() {
        let d = tmp_dir("virgulas");
        let res = d.join("again.vst3").join("Contents").join("Resources");
        std::fs::create_dir_all(&res).expect("cria");
        std::fs::write(
            res.join("moduleinfo.json"),
            r#"{
  "Name": "again",
  "Version": "3.8.1.0",
  "Factory Info": {
    "Vendor": "Steinberg Media Technologies",
    "Flags": {
      "Unicode": true,
      "Component Non Discardable": false,
    },
  },
  "Classes": [
    {
      "Category": "Audio Module Class",
      "Name": "AGain VST3",
      "Sub Categories": [
        "Fx",
      ],
    },
  ],
}"#,
        )
        .expect("grava");
        let found = scan_dir(ExternalKind::Vst3, &d);
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].vendor.as_deref(),
            Some("Steinberg Media Technologies")
        );
        assert_eq!(found[0].version.as_deref(), Some("3.8.1.0"));
        assert_eq!(
            found[0].classes,
            vec!["AGain VST3 (Audio Module Class)".to_string()]
        );
    }

    /// Regressão: fabricantes como a MeldaProduction instalam em subpastas
    /// (`VST3/<Fabricante>/<Categoria>/X.vst3`); antes só o 1º nível era lido.
    #[test]
    fn scan_finds_plugins_in_nested_vendor_folders() {
        let d = tmp_dir("aninhado");
        let deep = d.join("Fabricante").join("Dinamica");
        std::fs::create_dir_all(&deep).expect("cria");
        std::fs::write(deep.join("Compressor.vst3"), b"x").expect("grava");
        std::fs::write(d.join("Fabricante").join("Eq.vst3"), b"x").expect("grava");
        // pacote com outro .vst3 dentro: conta uma vez, sem descer nele
        let bundle_bin = d.join("Pacote.vst3").join("Contents").join(vst3_arch_dir());
        std::fs::create_dir_all(&bundle_bin).expect("cria pacote");
        std::fs::write(bundle_bin.join("Pacote.vst3"), b"x").expect("grava");
        // além da profundidade máxima: ignorado
        let mut too_deep = d.clone();
        for i in 0..=MAX_SCAN_DEPTH {
            too_deep = too_deep.join(format!("n{i}"));
        }
        std::fs::create_dir_all(&too_deep).expect("cria");
        std::fs::write(too_deep.join("Fundo.vst3"), b"x").expect("grava");

        let found = scan_dir(ExternalKind::Vst3, &d);
        let _ = std::fs::remove_dir_all(&d);
        let names: Vec<&str> = found.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["Compressor", "Eq", "Pacote"], "{names:?}");
    }

    #[test]
    fn strip_trailing_commas_keeps_commas_inside_strings() {
        assert_eq!(
            strip_trailing_commas(r#"{"a": [1, 2,], }"#),
            r#"{"a": [1, 2] }"#
        );
        assert_eq!(
            strip_trailing_commas(r#"{"t": "x,}\",]"}"#),
            r#"{"t": "x,}\",]"}"#
        );
        assert_eq!(strip_trailing_commas(""), "");
        assert_eq!(strip_trailing_commas(",,,"), ",,,");
    }

    #[test]
    fn broken_moduleinfo_and_missing_dir_do_not_fail() {
        let d = tmp_dir("ruim");
        let res = d.join("Ruim.vst3").join("Contents").join("Resources");
        std::fs::create_dir_all(&res).expect("cria");
        std::fs::write(res.join("moduleinfo.json"), b"{ nao e json").expect("grava");
        let found = scan_dir(ExternalKind::Vst3, &d);
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Ruim");
        assert!(scan_dir(ExternalKind::Ofx, &d.join("nao-existe")).is_empty());
    }
}
