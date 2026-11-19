//! Atualizações do programa: consulta os lançamentos públicos do Neoveg no
//! GitHub, compara com a versão em uso e baixa o instalador novo.
//!
//! A consulta não manda nenhum dado do usuário: só um `User-Agent` genérico.
//! Usa o `curl.exe` que vem no Windows 10 e 11 (e no Linux/macOS), sem
//! biblioteca de rede própria. A resposta é entrada hostil (§0.4): tamanho
//! limitado, número de lançamentos limitado e campos validados.

use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Repositório público dos lançamentos.
pub const REPO: &str = "LARPFX-ori/neoveg";
/// Página dos lançamentos (para quem prefere baixar à mão).
pub const RELEASES_PAGE: &str = "https://github.com/LARPFX-ori/neoveg/releases";
const API_URL: &str = "https://api.github.com/repos/LARPFX-ori/neoveg/releases?per_page=20";
/// Identificador genérico das requisições (sem dados do usuário).
const USER_AGENT: &str = "Neoveg-Updater";
/// Maior resposta da API aceita.
const MAX_RESPONSE: usize = 2 << 20;
/// Mais lançamentos lidos da resposta.
const MAX_RELEASES: usize = 50;
/// Maior trecho das novidades mostrado.
const MAX_NOTES_CHARS: usize = 6000;
/// Maior instalador aceito no download.
const MAX_INSTALLER_BYTES: u64 = 4 << 30;
/// Assinatura no fim do instalador (ver `crates/neoveg/src/setup/payload.rs`).
const SETUP_MAGIC: &[u8; 8] = b"NVSETUP1";

/// Versão `maior.menor.correção` com sufixo de pré-lançamento opcional.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// `-beta.1` etc. Uma versão com sufixo vem antes da mesma sem sufixo.
    pub pre: Option<String>,
}

impl Version {
    /// Lê `v0.2.0`, `0.2.0`, `v1.4` e `v1.0.0-beta.2`.
    pub fn parse(raw: &str) -> Option<Self> {
        let s = raw.trim();
        let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
        if s.is_empty() || s.len() > 64 {
            return None;
        }
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) if !p.is_empty() => (c, Some(p.to_string())),
            Some(_) => return None,
            None => (s, None),
        };
        let mut nums = core.split('.').map(|p| p.parse::<u64>().ok());
        let major = nums.next()??;
        let minor = nums.next().unwrap_or(Some(0))?;
        let patch = nums.next().unwrap_or(Some(0))?;
        if nums.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
            pre,
        })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(p) = &self.pre {
            write!(f, "-{p}")?;
        }
        Ok(())
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.pre, &other.pre) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.cmp(b),
            })
    }
}

/// Um lançamento mais novo que a versão em uso.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: Version,
    pub tag: String,
    pub name: String,
    /// Página do lançamento no GitHub.
    pub page_url: String,
    /// Novidades (texto do lançamento, cortado).
    pub notes: String,
    pub prerelease: bool,
    /// Download do `Neoveg-Setup-*.exe`, se o lançamento tiver.
    pub installer_url: Option<String>,
    /// Download do `.sha256` do instalador, se houver.
    pub checksum_url: Option<String>,
}

fn https_github(url: &str) -> bool {
    url.starts_with("https://github.com/")
        || url.starts_with("https://objects.githubusercontent.com/")
}

/// Escolhe o lançamento mais novo que `current` na resposta da API.
/// Rascunhos são ignorados; pré-lançamentos só com `prereleases`.
pub fn pick_update(
    current: &Version,
    json: &str,
    prereleases: bool,
) -> Result<Option<UpdateInfo>, String> {
    if json.len() > MAX_RESPONSE {
        return Err("resposta grande demais".into());
    }
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("resposta inválida: {e}"))?;
    let list = v
        .as_array()
        .ok_or("resposta inválida: esperada uma lista")?;
    let mut best: Option<UpdateInfo> = None;
    for r in list.iter().take(MAX_RELEASES) {
        let s = |k: &str| r.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let b = |k: &str| r.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
        if b("draft") {
            continue;
        }
        let pre = b("prerelease");
        if pre && !prereleases {
            continue;
        }
        let tag = s("tag_name");
        let Some(version) = Version::parse(&tag) else {
            continue;
        };
        if version <= *current || best.as_ref().is_some_and(|x| version <= x.version) {
            continue;
        }
        let page = s("html_url");
        let page_url = if https_github(&page) {
            page
        } else {
            RELEASES_PAGE.to_string()
        };
        let mut installer_url = None;
        let mut checksum_url = None;
        if let Some(assets) = r.get("assets").and_then(|a| a.as_array()) {
            for a in assets.iter().take(100) {
                let name = a.get("name").and_then(|x| x.as_str()).unwrap_or("");
                let url = a
                    .get("browser_download_url")
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                if !https_github(url) {
                    continue;
                }
                let lower = name.to_ascii_lowercase();
                if lower.starts_with("neoveg-setup") && lower.ends_with(".exe") {
                    installer_url = Some(url.to_string());
                } else if lower.starts_with("neoveg-setup") && lower.ends_with(".exe.sha256") {
                    checksum_url = Some(url.to_string());
                }
            }
        }
        let notes: String = s("body").chars().take(MAX_NOTES_CHARS).collect();
        let name = s("name");
        best = Some(UpdateInfo {
            version,
            name: if name.trim().is_empty() {
                tag.clone()
            } else {
                name
            },
            tag,
            page_url,
            notes,
            prerelease: pre,
            installer_url,
            checksum_url,
        });
    }
    Ok(best)
}

/// Versão deste programa.
pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version {
        major: 0,
        minor: 0,
        patch: 0,
        pre: None,
    })
}

fn curl() -> Command {
    let mut cmd = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // sem janela de console
    }
    cmd
}

/// Baixa um texto pequeno (limite `MAX_RESPONSE`).
fn fetch_text(url: &str) -> Result<String, String> {
    let out = curl()
        .args([
            "-sSfL",
            "--max-time",
            "20",
            "--max-filesize",
            &MAX_RESPONSE.to_string(),
            "-H",
            "Accept: application/vnd.github+json",
            "-A",
            USER_AGENT,
            url,
        ])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "sem resposta do servidor de atualizações: {}",
            err.trim()
        ));
    }
    if out.stdout.len() > MAX_RESPONSE {
        return Err("resposta grande demais".into());
    }
    String::from_utf8(out.stdout).map_err(|_| "resposta não é texto".to_string())
}

/// Consulta os lançamentos e devolve o mais novo que a versão em uso.
pub fn check(prereleases: bool) -> Result<Option<UpdateInfo>, String> {
    let json = fetch_text(API_URL)?;
    pick_update(&current_version(), &json, prereleases)
}

/// SHA-256 de um arquivo, em hexadecimal minúsculo.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(buf.get(..n).unwrap_or(&[]));
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Primeiro hash hexadecimal de 64 caracteres num arquivo `.sha256`
/// (`<hash>  <nome>` ou só o hash).
pub fn parse_checksum(text: &str) -> Option<String> {
    let token = text.split_whitespace().next()?;
    (token.len() == 64 && token.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| token.to_ascii_lowercase())
}

/// O arquivo termina com a assinatura do instalador do Neoveg.
pub fn looks_like_installer(path: &Path) -> bool {
    use std::io::{Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut tail = [0u8; 8];
    f.seek(SeekFrom::End(-8)).is_ok() && f.read_exact(&mut tail).is_ok() && &tail == SETUP_MAGIC
}

/// Baixa o instalador da atualização para a pasta temporária, confere o
/// SHA-256 (quando o lançamento o publica) e a assinatura do pacote.
pub fn download_installer(info: &UpdateInfo) -> Result<PathBuf, String> {
    let url = info
        .installer_url
        .as_deref()
        .ok_or("este lançamento não tem instalador para baixar")?;
    let dir = std::env::temp_dir().join("neoveg-atualizacao");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let dest = dir.join(format!("Neoveg-Setup-{}.exe", info.version));
    let _ = std::fs::remove_file(&dest);
    let out = curl()
        .args([
            "-sSfL",
            "--max-time",
            "1800",
            "--max-filesize",
            &MAX_INSTALLER_BYTES.to_string(),
            "-A",
            USER_AGENT,
            "-o",
        ])
        .arg(&dest)
        .arg(url)
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("o download falhou: {}", err.trim()));
    }
    if let Some(sum_url) = &info.checksum_url {
        let expected = parse_checksum(&fetch_text(sum_url)?)
            .ok_or("arquivo de verificação (SHA-256) inválido")?;
        let got = sha256_file(&dest)?;
        if got != expected {
            let _ = std::fs::remove_file(&dest);
            return Err("o instalador baixado não confere com o SHA-256 publicado".into());
        }
    }
    if !looks_like_installer(&dest) {
        let _ = std::fs::remove_file(&dest);
        return Err("o arquivo baixado não é um instalador do Neoveg".into());
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).expect("versão válida")
    }

    #[test]
    fn versoes_comparam_como_semver() {
        assert!(v("v0.2.0") > v("0.1.9"));
        assert!(v("1.0") == v("v1.0.0"));
        assert!(v("1.0.0") > v("1.0.0-beta.2"));
        assert!(v("1.0.0-beta.2") > v("1.0.0-beta.1"));
        assert!(v("0.10.0") > v("0.9.9"));
        for bad in [
            "", "v", "x.y.z", "1.2.3.4", "1.-", "-1.0", "1..2", "v1.0.0-",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
        assert_eq!(v("v2.3.4-rc.1").to_string(), "2.3.4-rc.1");
    }

    const RESPOSTA: &str = r#"[
        {"tag_name":"v0.3.0","name":"Neoveg 0.3.0","draft":false,"prerelease":true,
         "html_url":"https://github.com/LARPFX-ori/neoveg/releases/tag/v0.3.0","body":"novidades",
         "assets":[{"name":"Neoveg-Setup-0.3.0.exe","browser_download_url":"https://github.com/LARPFX-ori/neoveg/releases/download/v0.3.0/Neoveg-Setup-0.3.0.exe"},
                   {"name":"Neoveg-Setup-0.3.0.exe.sha256","browser_download_url":"https://github.com/LARPFX-ori/neoveg/releases/download/v0.3.0/Neoveg-Setup-0.3.0.exe.sha256"}]},
        {"tag_name":"v9.0.0","draft":true,"prerelease":false},
        {"tag_name":"v0.2.5","name":"","draft":false,"prerelease":false,"html_url":"https://exemplo.invalido/x","assets":[]},
        {"tag_name":"v0.1.0","draft":false,"prerelease":true}
    ]"#;

    #[test]
    fn escolhe_o_lancamento_mais_novo() {
        let up = pick_update(&v("0.2.0"), RESPOSTA, true)
            .expect("resposta válida")
            .expect("há atualização");
        assert_eq!(up.version, v("0.3.0"));
        assert!(up.prerelease);
        assert!(up
            .installer_url
            .as_deref()
            .is_some_and(|u| u.ends_with("Neoveg-Setup-0.3.0.exe")));
        assert!(up.checksum_url.is_some());
        // sem pré-lançamentos: a 0.2.5 estável; rascunho 9.0.0 nunca entra;
        // link fora do GitHub vira a página de lançamentos
        let stable = pick_update(&v("0.2.0"), RESPOSTA, false)
            .expect("resposta válida")
            .expect("há atualização");
        assert_eq!(stable.version, v("0.2.5"));
        assert_eq!(stable.name, "v0.2.5");
        assert_eq!(stable.page_url, RELEASES_PAGE);
        assert_eq!(stable.installer_url, None);
        // já na mais nova
        assert_eq!(pick_update(&v("0.3.0"), RESPOSTA, true), Ok(None));
    }

    #[test]
    fn resposta_hostil_e_erro_sem_panico() {
        assert!(pick_update(&v("0.1.0"), "{ não é json", true).is_err());
        assert!(pick_update(&v("0.1.0"), r#"{"message":"rate limit"}"#, true).is_err());
        assert_eq!(pick_update(&v("0.1.0"), "[]", true), Ok(None));
        // campos com tipo errado e tags inválidas são ignorados
        let estranho = r#"[{"tag_name":5,"draft":"sim"},{"tag_name":"lixo"},{"tag_name":"v0.2.0","assets":"x"}]"#;
        let up = pick_update(&v("0.1.0"), estranho, true)
            .expect("válida")
            .expect("0.2.0");
        assert_eq!(up.version, v("0.2.0"));
        // download fora do GitHub não é aceito
        let fora = r#"[{"tag_name":"v1.0.0","assets":[{"name":"Neoveg-Setup-1.0.0.exe","browser_download_url":"http://exemplo.invalido/s.exe"}]}]"#;
        let up = pick_update(&v("0.1.0"), fora, true)
            .expect("válida")
            .expect("1.0.0");
        assert_eq!(up.installer_url, None);
        let enorme = "a".repeat(MAX_RESPONSE + 1);
        assert!(pick_update(&v("0.1.0"), &enorme, true).is_err());
    }

    #[test]
    fn soma_de_verificacao() {
        let h = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(
            parse_checksum(&format!("{h}  Neoveg-Setup.exe\n")),
            Some(h.into())
        );
        assert_eq!(parse_checksum(&h.to_uppercase()), Some(h.into()));
        assert_eq!(parse_checksum("abc"), None);
        assert_eq!(parse_checksum(""), None);
        let dir = std::env::temp_dir().join(format!("neoveg-sha-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("vazio.bin");
        std::fs::write(&f, b"").expect("grava");
        assert_eq!(sha256_file(&f).expect("lê"), h);
        assert!(!looks_like_installer(&f));
        std::fs::write(&f, b"stub...NVSETUP1").expect("grava");
        assert!(looks_like_installer(&f));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
