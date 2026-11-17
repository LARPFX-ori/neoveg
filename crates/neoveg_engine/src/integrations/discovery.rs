//! Descoberta multiplataforma de aplicativos ArtCraft, Blender e canais de
//! controle locais (`agent-connection.json` + `agent-control.token`).
//!
//! Nenhuma regra de caminho específico de máquina é gravada no código
//! (`AGENTS.md` Regra da Casa 13). Toda leitura de disco limita o tamanho máximo
//! do arquivo (§0.4).

use super::ExternalApp;
use neoveg_common::prefs::ExternalAppsPrefs;
use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAX_CONNECTION_JSON_BYTES: usize = 64 * 1024;
const MAX_TOKEN_FILE_BYTES: usize = 256;

/// Metadados publicados em `agent-connection.json` pelos apps ArtCraft em execução.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConnectionInfo {
    pub version: u32,
    pub pid: u32,
    pub port: u16,
    #[serde(rename = "tokenFile", default)]
    pub token_file: String,
    #[serde(default)]
    pub app: String,
}

/// Diagnóstico de descoberta para um aplicativo externo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppDiscoveryStatus {
    pub app: ExternalApp,
    pub id: String,
    pub name: String,
    pub desktop_exe: Option<PathBuf>,
    pub cli_exe: Option<PathBuf>,
    pub from_manual_pref: bool,
    pub settings_dir: Option<PathBuf>,
    pub live_port: Option<u16>,
    pub live_pid: Option<u32>,
    pub live_token_available: bool,
    pub live_reachable: bool,
    pub version_hint: Option<String>,
}

impl AppDiscoveryStatus {
    pub fn available(&self) -> bool {
        self.desktop_exe.is_some() || self.cli_exe.is_some() || self.live_reachable
    }
}

/// Relatório completo serializável em JSON para a UI e CLI `--probe-integrations`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryReport {
    pub apps: Vec<AppDiscoveryStatus>,
}

/// Varre as preferências, variáveis de ambiente, `PATH` e locais padrão de
/// instalação no sistema operacional.
pub fn discover_all(prefs: &ExternalAppsPrefs, probe_sockets: bool) -> DiscoveryReport {
    let path_dirs = split_path_env();
    let apps = ExternalApp::ALL
        .iter()
        .map(|&app| discover_app_with(app, prefs, &path_dirs, probe_sockets))
        .collect();
    DiscoveryReport { apps }
}

pub fn discover_app(
    app: ExternalApp,
    prefs: &ExternalAppsPrefs,
    probe_sockets: bool,
) -> AppDiscoveryStatus {
    let path_dirs = split_path_env();
    discover_app_with(app, prefs, &path_dirs, probe_sockets)
}

fn discover_app_with(
    app: ExternalApp,
    prefs: &ExternalAppsPrefs,
    path_dirs: &[PathBuf],
    probe_sockets: bool,
) -> AppDiscoveryStatus {
    let manual_raw = prefs.path_for(app.id()).trim();
    let mut from_manual_pref = false;
    let mut desktop_exe = None;
    let mut cli_exe = None;

    if !manual_raw.is_empty() {
        let p = PathBuf::from(manual_raw);
        if p.is_file() {
            from_manual_pref = true;
            if is_cli_path(app, &p) {
                cli_exe = Some(p);
            } else {
                desktop_exe = Some(p);
            }
        }
    }

    if desktop_exe.is_none() {
        desktop_exe = find_binary(app, false, path_dirs);
    }
    if cli_exe.is_none() {
        cli_exe = if let Some(ref d) = desktop_exe {
            find_sibling_cli(app, d).or_else(|| find_binary(app, true, path_dirs))
        } else {
            find_binary(app, true, path_dirs)
        };
    }

    // Se o usuário apontou o CLI manualmente, tenta inferir o binário desktop irmão.
    if desktop_exe.is_none() {
        if let Some(ref c) = cli_exe {
            desktop_exe = find_sibling_desktop(app, c);
        }
    }

    let settings_dir = default_settings_dir(app);
    let mut live_port = None;
    let mut live_pid = None;
    let mut live_token_available = false;
    let mut version_hint = None;
    let mut live_reachable = false;

    if let Some(ref dir) = settings_dir {
        let conn_path = dir.join("agent-connection.json");
        if let Some(info) = read_agent_connection(&conn_path) {
            live_port = Some(info.port);
            live_pid = Some(info.pid);
            if !info.app.trim().is_empty() {
                version_hint = Some(info.app.clone());
            }
            let token_path = if info.token_file.trim().is_empty() {
                dir.join("agent-control.token")
            } else {
                PathBuf::from(&info.token_file)
            };
            live_token_available = read_hex_token(&token_path).is_some();
            if probe_sockets {
                live_reachable = probe_loopback_port(info.port);
            }
        }
    }

    AppDiscoveryStatus {
        app,
        id: app.id().to_string(),
        name: app.name().to_string(),
        desktop_exe,
        cli_exe,
        from_manual_pref,
        settings_dir,
        live_port,
        live_pid,
        live_token_available,
        live_reachable,
        version_hint,
    }
}

fn is_cli_path(app: ExternalApp, p: &Path) -> bool {
    let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    app.cli_binary_stems()
        .iter()
        .any(|c| stem.eq_ignore_ascii_case(c) && !app.desktop_binary_stems().contains(c))
}

fn exe_file_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

fn find_sibling_cli(app: ExternalApp, desktop_exe: &Path) -> Option<PathBuf> {
    let parent = desktop_exe.parent()?;
    for stem in app.cli_binary_stems() {
        let candidate = parent.join(exe_file_name(stem));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn find_sibling_desktop(app: ExternalApp, cli_exe: &Path) -> Option<PathBuf> {
    let parent = cli_exe.parent()?;
    for stem in app.desktop_binary_stems() {
        let candidate = parent.join(exe_file_name(stem));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn split_path_env() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default()
}

fn find_binary(app: ExternalApp, cli: bool, path_dirs: &[PathBuf]) -> Option<PathBuf> {
    let stems = if cli {
        app.cli_binary_stems()
    } else {
        app.desktop_binary_stems()
    };

    for dir in path_dirs {
        for stem in stems {
            let p = dir.join(exe_file_name(stem));
            if p.is_file() {
                return Some(p);
            }
        }
    }

    standard_install_candidates(app, cli)
        .into_iter()
        .find(|candidate| candidate.is_file())
}

/// Locais padrão públicos descritos na documentação oficial dos instaladores
/// ArtCraft (Windows MSI / macOS DMG / Linux deb/rpm/tarball) e do Blender.
fn standard_install_candidates(app: ExternalApp, cli: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let stems = if cli {
        app.cli_binary_stems()
    } else {
        app.desktop_binary_stems()
    };

    if app == ExternalApp::Blender {
        #[cfg(windows)]
        {
            for root_var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
                if let Some(root) = non_empty_env(root_var) {
                    let bf = root.join("Blender Foundation");
                    if let Ok(rd) = std::fs::read_dir(&bf) {
                        let mut versions: Vec<PathBuf> = rd
                            .flatten()
                            .map(|e| e.path().join("blender.exe"))
                            .filter(|p| p.is_file())
                            .collect();
                        versions.sort();
                        versions.reverse();
                        out.extend(versions);
                    }
                    let direct = root.join("Programs").join("Blender").join("blender.exe");
                    out.push(direct);
                }
            }
            if let Some(pf86) = non_empty_env("ProgramFiles(x86)") {
                out.push(
                    pf86.join("Steam")
                        .join("steamapps")
                        .join("common")
                        .join("Blender")
                        .join("blender.exe"),
                );
            }
        }
        #[cfg(target_os = "macos")]
        {
            out.push(PathBuf::from(
                "/Applications/Blender.app/Contents/MacOS/Blender",
            ));
            if let Some(home) = non_empty_env("HOME") {
                out.push(home.join("Applications/Blender.app/Contents/MacOS/Blender"));
            }
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            out.push(PathBuf::from("/usr/bin/blender"));
            out.push(PathBuf::from("/usr/local/bin/blender"));
            out.push(PathBuf::from("/snap/bin/blender"));
            if let Some(home) = non_empty_env("HOME") {
                out.push(home.join(".local/bin/blender"));
            }
        }
        return out;
    }

    // ArtCraft apps (PhotoCraft, VectorCraft, FilmCraft, EffectCraft, AniCraft, SoundCraft)
    #[cfg(windows)]
    {
        let folder = app.app_folder_name();
        if let Some(local) = non_empty_env("LOCALAPPDATA") {
            for stem in stems {
                out.push(
                    local
                        .join("Programs")
                        .join("ArtCraft")
                        .join(folder)
                        .join(exe_file_name(stem)),
                );
            }
        }
        for root_var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(root) = non_empty_env(root_var) {
                for stem in stems {
                    out.push(root.join("ArtCraft").join(folder).join(exe_file_name(stem)));
                }
            }
        }
        if let Some(home) = non_empty_env("USERPROFILE") {
            for stem in stems {
                out.push(home.join(".cargo").join("bin").join(exe_file_name(stem)));
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let bundle = app.mac_bundle_name();
        for stem in stems {
            out.push(
                PathBuf::from("/Applications")
                    .join(bundle)
                    .join("Contents/MacOS")
                    .join(stem),
            );
            if let Some(home) = non_empty_env("HOME") {
                out.push(
                    home.join("Applications")
                        .join(bundle)
                        .join("Contents/MacOS")
                        .join(stem),
                );
                out.push(home.join(".cargo/bin").join(stem));
            }
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for stem in stems {
            out.push(PathBuf::from("/usr/bin").join(stem));
            out.push(PathBuf::from("/usr/local/bin").join(stem));
            if let Some(home) = non_empty_env("HOME") {
                out.push(home.join(".local/bin").join(stem));
                out.push(home.join(".cargo/bin").join(stem));
            }
        }
    }

    out
}

fn non_empty_env(name: &str) -> Option<PathBuf> {
    let val = std::env::var_os(name)?;
    if val.is_empty() {
        return None;
    }
    Some(PathBuf::from(val))
}

/// Retorna o diretório padrão de configurações/estado documentado de cada app.
pub fn default_settings_dir(app: ExternalApp) -> Option<PathBuf> {
    let env_override = match app {
        ExternalApp::PhotoCraft => non_empty_env("PHOTOCRAFT_CONFIG_DIR"),
        ExternalApp::VectorCraft => non_empty_env("VECTORCRAFT_CONFIG_DIR"),
        ExternalApp::FilmCraft => non_empty_env("FILMCRAFT_DATA_DIR"),
        ExternalApp::EffectCraft => non_empty_env("EFFECTCRAFT_CONFIG_DIR"),
        ExternalApp::AniCraft => non_empty_env("ANICRAFT_CONFIG_DIR"),
        ExternalApp::SoundCraft => non_empty_env("SOUNDCRAFT_CONFIG_DIR"),
        ExternalApp::Blender => non_empty_env("BLENDER_USER_CONFIG"),
    };
    if env_override.is_some() {
        return env_override;
    }

    #[cfg(windows)]
    {
        let appdata = non_empty_env("APPDATA")?;
        let folder = match app {
            ExternalApp::PhotoCraft => "Photocraft",
            ExternalApp::VectorCraft => "VectorCraft",
            ExternalApp::FilmCraft => "FilmCraft",
            ExternalApp::EffectCraft => "EffectCraft",
            ExternalApp::AniCraft => "AniCraft",
            ExternalApp::SoundCraft => "SoundCraft",
            ExternalApp::Blender => "Blender Foundation\\Blender",
        };
        Some(appdata.join(folder))
    }

    #[cfg(target_os = "macos")]
    {
        let home = non_empty_env("HOME")?;
        let base = home.join("Library/Application Support");
        let folder = match app {
            ExternalApp::PhotoCraft => "Photocraft",
            ExternalApp::VectorCraft => "VectorCraft",
            ExternalApp::FilmCraft => "FilmCraft",
            ExternalApp::EffectCraft => "EffectCraft",
            ExternalApp::AniCraft => "AniCraft",
            ExternalApp::SoundCraft => "SoundCraft",
            ExternalApp::Blender => "Blender",
        };
        Some(base.join(folder))
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let config = non_empty_env("XDG_CONFIG_HOME")
            .or_else(|| non_empty_env("HOME").map(|h| h.join(".config")))?;
        if app == ExternalApp::FilmCraft {
            let data = non_empty_env("XDG_DATA_HOME")
                .or_else(|| non_empty_env("HOME").map(|h| h.join(".local/share")))?;
            return Some(data.join("filmcraft"));
        }
        Some(config.join(app.id()))
    }
}

/// Lê e valida `agent-connection.json` com limite de tamanho e checagem de versão/porta (§0.4).
pub fn read_agent_connection(path: &Path) -> Option<AgentConnectionInfo> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_CONNECTION_JSON_BYTES as u64 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > MAX_CONNECTION_JSON_BYTES {
        return None;
    }
    let info: AgentConnectionInfo = serde_json::from_slice(&bytes).ok()?;
    if info.version != 1 || info.port == 0 || info.pid == 0 {
        return None;
    }
    if info.token_file.contains('\0') || info.app.len() > 256 {
        return None;
    }
    Some(info)
}

/// Lê e valida um token hexadecimal (64 chars lowercase ou case-insensitive ASCII) de um arquivo.
pub fn read_hex_token(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_TOKEN_FILE_BYTES as u64 {
        return None;
    }
    let raw = std::fs::read_to_string(path).ok()?;
    normalize_hex_token(&raw)
}

pub fn normalize_hex_token(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.len() != 64 || !trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(trimmed.to_ascii_lowercase())
}

/// Verifica se existe um serviço aceitando conexões exclusivamente em `127.0.0.1:<port>`.
pub fn probe_loopback_port(port: u16) -> bool {
    if port == 0 {
        return false;
    }
    let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(80)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("neoveg_int_disc_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn parse_e_saneamento_de_agent_connection_e_token() {
        let dir = temp_dir("conn");
        let conn = dir.join("agent-connection.json");
        let tok = dir.join("agent-control.token");
        std::fs::write(
            &tok,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n",
        )
        .expect("token");
        std::fs::write(
            &conn,
            format!(
                r#"{{"version":1,"pid":4242,"port":7878,"tokenFile":{},"app":"0.20.1"}}"#,
                serde_json::to_string(&tok.to_string_lossy().to_string()).expect("json")
            ),
        )
        .expect("conn");

        let parsed = read_agent_connection(&conn).expect("deve ler connection");
        assert_eq!(parsed.port, 7878);
        assert_eq!(parsed.pid, 4242);
        assert_eq!(read_hex_token(&tok).expect("tok").len(), 64);

        // Versão incompatível e porta 0 devem ser rejeitados
        std::fs::write(&conn, r#"{"version":2,"pid":1,"port":7878}"#).expect("v2");
        assert!(read_agent_connection(&conn).is_none());
        std::fs::write(&conn, r#"{"version":1,"pid":1,"port":0}"#).expect("port 0");
        assert!(read_agent_connection(&conn).is_none());
        std::fs::write(&tok, "curto").expect("bad tok");
        assert!(read_hex_token(&tok).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn descoberta_manual_e_irmao_cli_funcionam() {
        let dir = temp_dir("manual");
        let exe_desktop = dir.join(exe_file_name("photocraft"));
        let exe_cli = dir.join(exe_file_name("photocraft-cli"));
        std::fs::write(&exe_desktop, b"stub").expect("desktop");
        std::fs::write(&exe_cli, b"stub").expect("cli");

        let prefs = ExternalAppsPrefs {
            photocraft_path: exe_desktop.to_string_lossy().to_string(),
            ..Default::default()
        };
        let st = discover_app_with(ExternalApp::PhotoCraft, &prefs, &[], false);
        assert!(st.from_manual_pref);
        assert_eq!(st.desktop_exe.as_deref(), Some(exe_desktop.as_path()));
        assert_eq!(st.cli_exe.as_deref(), Some(exe_cli.as_path()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn probe_loopback_detecta_listener_ativo() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        assert!(probe_loopback_port(port));
        drop(listener);
        assert!(!probe_loopback_port(0));
    }
}
