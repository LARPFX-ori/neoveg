//! Loja de scripts e extensões: catálogo, instalação e remoção.
//!
//! O catálogo é o `index.json` do repositório `LARPFX-ori/neoveg-marketplace`
//! (formato 1). Cada pacote lista os arquivos com tamanho e SHA-256; a
//! instalação baixa cada arquivo, confere tamanho e hash, monta tudo numa
//! pasta temporária e só então troca a pasta do pacote em `loja/<id>/`, ao
//! lado dos dados do Neoveg. Um download interrompido não deixa pacote pela
//! metade.
//!
//! O catálogo vem da internet: tudo nele é entrada hostil (AGENTS.md §0.4).
//! Pacote com id, caminho, hash, licença ou tamanho inválido é descartado,
//! com o motivo em `StoreIndex::skipped`; o resto do catálogo continua.

use neoveg_common::error::NeovegError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// Endereço base do repositório da loja (arquivos crus do branch `main`).
pub const STORE_BASE_URL: &str =
    "https://raw.githubusercontent.com/LARPFX-ori/neoveg-marketplace/main";
/// Versão do formato do `index.json` que este Neoveg entende.
pub const INDEX_FORMAT: u32 = 1;
/// Pasta dos pacotes instalados, dentro da pasta de dados do Neoveg.
pub const STORE_DIR_NAME: &str = "loja";
/// Manifesto gravado em cada pacote instalado.
pub const INSTALLED_MANIFEST: &str = "pacote.json";

const MAX_INDEX_BYTES: u64 = 4 << 20;
const MAX_PACKAGES: usize = 2000;
const MAX_FILE_BYTES: u64 = 1 << 20;
const MAX_PACKAGE_BYTES: u64 = 5 << 20;
const MAX_FILES: usize = 64;
const MAX_TEXT: usize = 400;

/// Licenças aceitas (lista do AGENTS.md).
const ALLOWED_LICENSES: [&str; 12] = [
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "OFL-1.1",
    "Ubuntu-font-1.0",
    "Bitstream-Vera",
    "CC0-1.0",
    "CC-BY-4.0",
    "CC-BY-SA-4.0",
    "LicenseRef-Public-Domain",
];
const ALLOWED_EXTENSIONS: [&str; 9] = [
    "cs", "py", "md", "txt", "json", "config", "xml", "png", "svg",
];

/// Tipo de pacote.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PackageKind {
    #[serde(rename = "script-cs")]
    ScriptCs,
    #[serde(rename = "script-py")]
    ScriptPy,
    #[serde(rename = "extensao")]
    Extension,
}

impl PackageKind {
    /// Rótulo em português (passa por `tr` na interface).
    pub fn label(self) -> &'static str {
        match self {
            PackageKind::ScriptCs => "Script C#",
            PackageKind::ScriptPy => "Script Python",
            PackageKind::Extension => "Extensão",
        }
    }
}

/// Um arquivo de um pacote.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreFile {
    pub caminho: String,
    pub tamanho: u64,
    pub sha256: String,
}

/// Um pacote do catálogo (os nomes dos campos seguem o `index.json`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StorePackage {
    pub id: String,
    pub nome: String,
    pub descricao: String,
    pub versao: String,
    pub tipo: PackageKind,
    pub autor: String,
    pub licenca: String,
    pub fonte: String,
    pub neoveg_min: String,
    pub entrada: String,
    pub arquivos: Vec<StoreFile>,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl StorePackage {
    /// Página do pacote no GitHub.
    pub fn page_url(&self) -> String {
        format!(
            "https://github.com/LARPFX-ori/neoveg-marketplace/tree/main/pacotes/{}",
            self.id
        )
    }

    /// O pacote funciona nesta versão do Neoveg.
    pub fn compatible_with(&self, neoveg_version: &str) -> bool {
        match (
            Version::parse(&self.neoveg_min),
            Version::parse(neoveg_version),
        ) {
            (Some(min), Some(cur)) => cur >= min,
            _ => false,
        }
    }
}

/// Catálogo lido, com os pacotes descartados e o motivo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StoreIndex {
    pub packages: Vec<StorePackage>,
    pub skipped: Vec<String>,
}

#[derive(Deserialize)]
struct RawIndex {
    versao_formato: u32,
    #[serde(default)]
    pacotes: Vec<serde_json::Value>,
}

/// Versão `X.Y.Z`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let mut it = s.trim().split('.');
        let mut part = || -> Option<u64> {
            let p = it.next()?;
            if p.is_empty() || p.len() > 9 || !p.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            p.parse().ok()
        };
        let v = Version(part()?, part()?, part()?);
        if it.next().is_some() {
            return None;
        }
        Some(v)
    }
}

/// Compara duas versões `X.Y.Z`; uma inválida fica abaixo de qualquer válida.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    Version::parse(a).cmp(&Version::parse(b))
}

fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('-')
        && !id.ends_with('-')
        && !id.contains("--")
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Caminho relativo seguro: só `[A-Za-z0-9._-]` separados por `/`, sem `.`
/// nem `..` como componente.
fn is_safe_relative(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 200
        && path.split('/').all(|c| {
            !c.is_empty()
                && c != "."
                && c != ".."
                && c.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
}

fn license_allowed(expr: &str) -> bool {
    let expr = expr.trim();
    !expr.is_empty()
        && expr
            .split(" OR ")
            .flat_map(|p| p.split(" AND "))
            .all(|p| ALLOWED_LICENSES.contains(&p.trim()))
}

fn extension_allowed(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| ALLOWED_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Confere um pacote do catálogo; `Err` diz por que ele foi descartado.
pub fn validate_package(p: &StorePackage) -> Result<(), String> {
    if !is_valid_id(&p.id) {
        return Err(format!(
            "id inválido: {:?}",
            p.id.chars().take(64).collect::<String>()
        ));
    }
    let id = &p.id;
    if Version::parse(&p.versao).is_none() || Version::parse(&p.neoveg_min).is_none() {
        return Err(format!("{id}: versão inválida"));
    }
    for (campo, v) in [
        ("nome", &p.nome),
        ("descricao", &p.descricao),
        ("autor", &p.autor),
    ] {
        if v.trim().is_empty() || v.chars().count() > MAX_TEXT {
            return Err(format!("{id}: '{campo}' vazio ou longo demais"));
        }
    }
    if !license_allowed(&p.licenca) {
        return Err(format!("{id}: licença fora da lista permitida"));
    }
    if p.fonte != "original" && !p.fonte.starts_with("https://") {
        return Err(format!("{id}: fonte inválida"));
    }
    if p.arquivos.is_empty() || p.arquivos.len() > MAX_FILES {
        return Err(format!("{id}: número de arquivos inválido"));
    }
    let mut total: u64 = 0;
    let mut seen = std::collections::HashSet::new();
    for f in &p.arquivos {
        if !is_safe_relative(&f.caminho) || !extension_allowed(&f.caminho) {
            return Err(format!("{id}: caminho ou extensão não permitidos"));
        }
        if f.caminho == INSTALLED_MANIFEST || !seen.insert(f.caminho.to_ascii_lowercase()) {
            return Err(format!("{id}: arquivo repetido ou reservado"));
        }
        if !is_sha256_hex(&f.sha256) || f.tamanho > MAX_FILE_BYTES {
            return Err(format!("{id}: hash ou tamanho inválido"));
        }
        total = total.saturating_add(f.tamanho);
    }
    if total > MAX_PACKAGE_BYTES {
        return Err(format!("{id}: pacote grande demais"));
    }
    if !p.arquivos.iter().any(|f| f.caminho == p.entrada) {
        return Err(format!("{id}: a entrada não está entre os arquivos"));
    }
    let entry_ok = match p.tipo {
        PackageKind::ScriptCs => p.entrada.ends_with(".cs"),
        PackageKind::ScriptPy => p.entrada.ends_with(".py"),
        PackageKind::Extension => p.entrada.ends_with(".json"),
    };
    if !entry_ok {
        return Err(format!("{id}: entrada não combina com o tipo"));
    }
    Ok(())
}

/// Lê o `index.json`. Falha só se o arquivo inteiro for inválido; pacotes
/// ruins vão para `skipped`.
pub fn parse_index(text: &str) -> Result<StoreIndex, NeovegError> {
    if text.len() as u64 > MAX_INDEX_BYTES {
        return Err(NeovegError::InvalidInput("catálogo grande demais".into()));
    }
    let raw: RawIndex = serde_json::from_str(text).map_err(|e| NeovegError::Json(e.to_string()))?;
    if raw.versao_formato != INDEX_FORMAT {
        return Err(NeovegError::Unsupported(format!(
            "formato de catálogo {} (este Neoveg lê o {INDEX_FORMAT}); atualize o Neoveg",
            raw.versao_formato
        )));
    }
    let mut index = StoreIndex::default();
    let mut ids = std::collections::HashSet::new();
    for v in raw.pacotes.into_iter().take(MAX_PACKAGES) {
        let p: StorePackage = match serde_json::from_value(v) {
            Ok(p) => p,
            Err(e) => {
                index.skipped.push(format!("pacote ilegível: {e}"));
                continue;
            }
        };
        if let Err(e) = validate_package(&p) {
            index.skipped.push(e);
            continue;
        }
        if !ids.insert(p.id.clone()) {
            index.skipped.push(format!("{}: id repetido", p.id));
            continue;
        }
        index.packages.push(p);
    }
    index.packages.sort_by_key(|p| p.nome.to_lowercase());
    Ok(index)
}

/// SHA-256 em hexadecimal minúsculo.
pub fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    let mut s = String::with_capacity(64);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Baixa um endereço da loja com o `curl` do sistema (presente no Windows 10+,
/// no macOS e nas distribuições Linux), com limite de tamanho e de tempo.
pub fn fetch(url: &str, max_bytes: u64) -> Result<Vec<u8>, NeovegError> {
    if !url.starts_with(STORE_BASE_URL) || url.contains("..") || url.contains(char::is_whitespace) {
        return Err(NeovegError::InvalidInput("endereço fora da loja".into()));
    }
    let out = std::process::Command::new(if cfg!(windows) { "curl.exe" } else { "curl" })
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--max-time",
            "60",
            "--max-filesize",
            &max_bytes.to_string(),
            "--user-agent",
            "Neoveg-Loja",
            url,
        ])
        .output()
        .map_err(|e| NeovegError::Io(format!("não foi possível executar o curl: {e}")))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err: String = err.trim().chars().take(300).collect();
        return Err(NeovegError::Io(format!("download falhou: {err}")));
    }
    if out.stdout.len() as u64 > max_bytes {
        return Err(NeovegError::InvalidInput(
            "arquivo maior que o permitido".into(),
        ));
    }
    Ok(out.stdout)
}

/// Baixa e lê o catálogo.
pub fn fetch_index() -> Result<StoreIndex, NeovegError> {
    let bytes = fetch(&format!("{STORE_BASE_URL}/index.json"), MAX_INDEX_BYTES)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| NeovegError::InvalidInput("catálogo não é UTF-8".into()))?;
    parse_index(&text)
}

/// Pasta dos pacotes instalados.
pub fn store_dir() -> PathBuf {
    neoveg_common::util::base_dir().join(STORE_DIR_NAME)
}

/// Um pacote instalado.
#[derive(Clone, Debug, PartialEq)]
pub struct InstalledPackage {
    pub package: StorePackage,
    pub dir: PathBuf,
}

impl InstalledPackage {
    pub fn entry_path(&self) -> PathBuf {
        self.package
            .entrada
            .split('/')
            .fold(self.dir.clone(), |p, c| p.join(c))
    }
}

/// Pacotes instalados em `root` (os de manifesto inválido são ignorados).
pub fn list_installed_in(root: &Path) -> Vec<InstalledPackage> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in rd.flatten().take(MAX_PACKAGES) {
        let dir = entry.path();
        let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !is_valid_id(name) || !dir.is_dir() {
            continue;
        }
        let manifest = dir.join(INSTALLED_MANIFEST);
        let Ok(meta) = std::fs::metadata(&manifest) else {
            continue;
        };
        if meta.len() > MAX_INDEX_BYTES {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        let Ok(package) = serde_json::from_str::<StorePackage>(&text) else {
            continue;
        };
        if package.id != name || validate_package(&package).is_err() {
            continue;
        }
        out.push(InstalledPackage { package, dir });
    }
    out.sort_by(|a, b| a.package.id.cmp(&b.package.id));
    out
}

/// Pacotes instalados na pasta da loja.
pub fn list_installed() -> Vec<InstalledPackage> {
    list_installed_in(&store_dir())
}

/// Baixa e confere cada arquivo do pacote para `staging`, e grava o manifesto.
fn write_staged(
    staging: &Path,
    pkg: &StorePackage,
    get: &mut dyn FnMut(&StoreFile) -> Result<Vec<u8>, NeovegError>,
) -> Result<(), NeovegError> {
    for f in &pkg.arquivos {
        let data = get(f)?;
        if data.len() as u64 != f.tamanho {
            return Err(NeovegError::InvalidInput(format!(
                "{}: tamanho diferente do catálogo",
                f.caminho
            )));
        }
        if sha256_hex(&data) != f.sha256 {
            return Err(NeovegError::InvalidInput(format!(
                "{}: SHA-256 não confere com o catálogo",
                f.caminho
            )));
        }
        let dest = f
            .caminho
            .split('/')
            .fold(staging.to_path_buf(), |p, c| p.join(c));
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, &data)?;
    }
    let manifest =
        serde_json::to_string_pretty(pkg).map_err(|e| NeovegError::Json(e.to_string()))?;
    std::fs::write(staging.join(INSTALLED_MANIFEST), manifest)?;
    Ok(())
}

/// Instala (ou atualiza) `pkg` em `root/<id>/`, obtendo cada arquivo com
/// `get` (o caminho relativo do arquivo → bytes). Confere tamanho e SHA-256
/// de tudo antes de trocar a pasta.
pub fn install_into(
    root: &Path,
    pkg: &StorePackage,
    get: &mut dyn FnMut(&StoreFile) -> Result<Vec<u8>, NeovegError>,
) -> Result<InstalledPackage, NeovegError> {
    validate_package(pkg).map_err(NeovegError::InvalidInput)?;
    std::fs::create_dir_all(root)?;
    let staging = root.join(format!(".instalando-{}-{}", pkg.id, std::process::id()));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir_all(&staging)?;
    let result = write_staged(&staging, pkg, get);
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    let dir = root.join(&pkg.id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::rename(&staging, &dir)?;
    Ok(InstalledPackage {
        package: pkg.clone(),
        dir,
    })
}

/// Baixa e instala `pkg` na pasta da loja.
pub fn install(pkg: &StorePackage) -> Result<InstalledPackage, NeovegError> {
    let id = pkg.id.clone();
    install_into(&store_dir(), pkg, &mut |f| {
        fetch(
            &format!("{STORE_BASE_URL}/pacotes/{id}/{}", f.caminho),
            f.tamanho,
        )
    })
}

/// Remove o pacote `id` de `root`.
pub fn uninstall_from(root: &Path, id: &str) -> Result<(), NeovegError> {
    if !is_valid_id(id) {
        return Err(NeovegError::InvalidInput("id de pacote inválido".into()));
    }
    let dir = root.join(id);
    if dir.is_dir() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

/// Remove o pacote `id` da pasta da loja.
pub fn uninstall(id: &str) -> Result<(), NeovegError> {
    uninstall_from(&store_dir(), id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(caminho: &str, data: &[u8]) -> StoreFile {
        StoreFile {
            caminho: caminho.into(),
            tamanho: data.len() as u64,
            sha256: sha256_hex(data),
        }
    }

    fn pkg(id: &str, data: &[u8]) -> StorePackage {
        StorePackage {
            id: id.into(),
            nome: "Teste".into(),
            descricao: "Pacote de teste".into(),
            versao: "1.0.0".into(),
            tipo: PackageKind::ScriptCs,
            autor: "Neoveg".into(),
            licenca: "MIT OR Apache-2.0".into(),
            fonte: "original".into(),
            neoveg_min: "0.2.0".into(),
            entrada: "Script.cs".into(),
            arquivos: vec![file("Script.cs", data)],
            tags: vec!["teste".into()],
        }
    }

    fn temp_root(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("neoveg_loja_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn sha256_de_vetor_conhecido() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn versoes_e_compatibilidade() {
        assert_eq!(Version::parse("1.2.3"), Some(Version(1, 2, 3)));
        for ruim in [
            "",
            "1.2",
            "1.2.3.4",
            "a.b.c",
            "1..3",
            "-1.0.0",
            "1.0.0-beta",
            "9999999999.0.0",
        ] {
            assert_eq!(Version::parse(ruim), None, "{ruim}");
        }
        assert_eq!(compare_versions("1.10.0", "1.9.9"), Ordering::Greater);
        assert_eq!(compare_versions("lixo", "0.0.1"), Ordering::Less);
        let p = pkg("a", b"x");
        assert!(p.compatible_with("0.2.0"));
        assert!(p.compatible_with("1.0.0"));
        assert!(!p.compatible_with("0.1.9"));
        assert!(!p.compatible_with("lixo"));
    }

    #[test]
    fn catalogo_valido_e_pacotes_hostis_descartados() {
        let bom = pkg("bom", b"x");
        let mut traversal = pkg("traversal", b"x");
        traversal.arquivos[0].caminho = "../fora.cs".into();
        traversal.entrada = "../fora.cs".into();
        let mut exe = pkg("exe", b"x");
        exe.arquivos.push(file("virus.exe", b"MZ"));
        let mut gpl = pkg("gpl", b"x");
        gpl.licenca = "GPL-3.0".into();
        let mut id_ruim = pkg("ID Ruim", b"x");
        id_ruim.id = "ID Ruim".into();
        let mut hash_ruim = pkg("hash", b"x");
        hash_ruim.arquivos[0].sha256 = "zz".into();
        let mut entrada = pkg("entrada", b"x");
        entrada.entrada = "Outro.cs".into();
        let mut reservado = pkg("reservado", b"x");
        reservado.arquivos.push(file(INSTALLED_MANIFEST, b"{}"));
        let pacotes: Vec<serde_json::Value> = [
            &bom, &traversal, &exe, &gpl, &id_ruim, &hash_ruim, &entrada, &reservado, &bom,
        ]
        .iter()
        .map(|p| serde_json::to_value(p).expect("serializa"))
        .chain([serde_json::json!({"id": 5}), serde_json::json!("texto")])
        .collect();
        let text = serde_json::json!({"versao_formato": 1, "pacotes": pacotes}).to_string();
        let idx = parse_index(&text).expect("catálogo válido");
        assert_eq!(idx.packages.len(), 1);
        assert_eq!(idx.packages[0].id, "bom");
        assert_eq!(idx.skipped.len(), 10, "{:?}", idx.skipped);
    }

    #[test]
    fn catalogo_inteiro_invalido_nao_trava() {
        assert!(parse_index("").is_err());
        assert!(parse_index("[]").is_err());
        assert!(parse_index("{\"versao_formato\": 2, \"pacotes\": []}").is_err());
        let enorme = "x".repeat((MAX_INDEX_BYTES + 1) as usize);
        assert!(parse_index(&enorme).is_err());
        // truncamentos de um catálogo válido nunca entram em pânico
        let text = serde_json::json!({
            "versao_formato": 1,
            "pacotes": [serde_json::to_value(pkg("a", b"x")).expect("serializa")]
        })
        .to_string();
        for n in 0..text.len() {
            if text.is_char_boundary(n) {
                let _ = parse_index(&text[..n]);
            }
        }
    }

    #[test]
    fn instala_confere_hash_atualiza_e_remove() {
        let root = temp_root("instala");
        let p = pkg("meu-script", b"conteudo v1");
        let inst = install_into(&root, &p, &mut |_| Ok(b"conteudo v1".to_vec())).expect("instala");
        assert_eq!(
            std::fs::read(inst.entry_path()).expect("lê"),
            b"conteudo v1".to_vec()
        );
        assert_eq!(list_installed_in(&root).len(), 1);

        // hash errado: nada muda na versão instalada
        let mut p2 = pkg("meu-script", b"conteudo v2");
        p2.versao = "1.1.0".into();
        let err = install_into(&root, &p2, &mut |_| Ok(b"adulterado!".to_vec()));
        assert!(err.is_err());
        let lista = list_installed_in(&root);
        assert_eq!(lista.len(), 1);
        assert_eq!(lista[0].package.versao, "1.0.0");
        assert_eq!(
            std::fs::read_dir(&root).expect("lê").count(),
            1,
            "a pasta temporária foi apagada"
        );

        // atualização correta troca a versão
        install_into(&root, &p2, &mut |_| Ok(b"conteudo v2".to_vec())).expect("atualiza");
        assert_eq!(list_installed_in(&root)[0].package.versao, "1.1.0");

        assert!(uninstall_from(&root, "../fora").is_err());
        uninstall_from(&root, "meu-script").expect("remove");
        assert!(list_installed_in(&root).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn manifesto_instalado_hostil_e_ignorado() {
        let root = temp_root("hostil");
        for (nome, conteudo) in [
            ("a", "não é json"),
            ("b", "{\"id\": \"outro\"}"),
            ("Pasta Ruim", "{}"),
        ] {
            let d = root.join(nome);
            std::fs::create_dir_all(&d).expect("cria");
            std::fs::write(d.join(INSTALLED_MANIFEST), conteudo).expect("grava");
        }
        assert!(list_installed_in(&root).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn fetch_recusa_endereco_fora_da_loja() {
        assert!(fetch("https://example.com/x", 10).is_err());
        assert!(fetch(&format!("{STORE_BASE_URL}/../../x"), 10).is_err());
        assert!(fetch(&format!("{STORE_BASE_URL}/a b"), 10).is_err());
    }
}
