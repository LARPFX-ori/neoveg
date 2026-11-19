//! Integração com o Windows sem `unsafe`: registro por `reg.exe`, atalhos e
//! elevação por PowerShell, e o manifesto da instalação (`instalado.json`).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Nome do executável do programa dentro da pasta instalada.
pub const APP_EXE: &str = "neoveg.exe";
/// Nome do desinstalador copiado para a pasta instalada.
pub const UNINSTALLER_EXE: &str = "desinstalar.exe";
/// Chave em "Adicionar ou remover programas".
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Neoveg";
/// ProgID da associação de `.neoveg`.
pub const PROG_ID: &str = "Neoveg.Projeto";
/// Extensões associadas ao Neoveg.
pub const ASSOCIATED_EXTS: &[&str] = &[".neoveg"];

/// Para quem instalar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    /// Só este usuário, sem administrador (`%LOCALAPPDATA%\Programs`).
    User,
    /// Todos os usuários, com administrador (`%ProgramFiles%`).
    AllUsers,
}

impl Scope {
    /// Raiz do registro: HKCU ou HKLM.
    pub fn hive(self) -> &'static str {
        match self {
            Scope::User => "HKCU",
            Scope::AllUsers => "HKLM",
        }
    }

    pub fn cli_flag(self) -> &'static str {
        match self {
            Scope::User => "--user",
            Scope::AllUsers => "--all-users",
        }
    }
}

/// Lê uma variável de ambiente com caminho absoluto.
fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// Pasta padrão de instalação.
pub fn default_install_dir(scope: Scope) -> Option<PathBuf> {
    match scope {
        Scope::User => env_dir("LOCALAPPDATA").map(|d| d.join("Programs").join("Neoveg")),
        Scope::AllUsers => env_dir("ProgramFiles").map(|d| d.join("Neoveg")),
    }
}

/// Pasta do menu Iniciar.
pub fn start_menu_dir(scope: Scope) -> Option<PathBuf> {
    let base = match scope {
        Scope::User => env_dir("APPDATA"),
        Scope::AllUsers => env_dir("ProgramData"),
    }?;
    Some(
        base.join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs"),
    )
}

/// Pasta da Área de Trabalho (`%PUBLIC%\Desktop` para todos).
pub fn desktop_dir(scope: Scope) -> Option<PathBuf> {
    match scope {
        Scope::User => env_dir("USERPROFILE").map(|d| d.join("Desktop")),
        Scope::AllUsers => env_dir("PUBLIC").map(|d| d.join("Desktop")),
    }
}

/// O que foi instalado, gravado em `instalado.json` ao lado do programa. O
/// Neoveg usa a presença dele para guardar os dados em `%APPDATA%\Neoveg`, e
/// o desinstalador o usa para saber o que remover.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub produto: String,
    pub versao: String,
    pub escopo: Scope,
    /// Arquivos do programa, relativos à pasta instalada.
    pub arquivos: Vec<PathBuf>,
    /// Atalhos criados (caminhos absolutos).
    pub atalhos: Vec<PathBuf>,
    /// `.neoveg` associado ao Neoveg.
    pub associacoes: bool,
}

/// Maior manifesto aceito ao desinstalar.
const MAX_MANIFEST_BYTES: u64 = 8 << 20;

impl Manifest {
    pub fn path(install_dir: &Path) -> PathBuf {
        install_dir.join(neoveg_common::util::INSTALL_MARKER)
    }

    pub fn save(&self, install_dir: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let p = Self::path(install_dir);
        std::fs::write(&p, json).map_err(|e| format!("{}: {e}", p.display()))
    }

    pub fn load(install_dir: &Path) -> Result<Self, String> {
        let p = Self::path(install_dir);
        let meta = std::fs::metadata(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        if meta.len() > MAX_MANIFEST_BYTES {
            return Err("manifesto da instalação grande demais".into());
        }
        let raw = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        serde_json::from_str(&raw).map_err(|e| format!("manifesto inválido: {e}"))
    }
}

/// Um valor a gravar no registro.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegValue {
    Sz(String),
    Dword(u32),
}

/// Uma escrita no registro: chave (com a raiz), nome (`None` = valor padrão)
/// e valor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegWrite {
    pub key: String,
    pub name: Option<String>,
    pub value: RegValue,
}

impl RegWrite {
    fn sz(key: String, name: Option<&str>, v: String) -> Self {
        RegWrite {
            key,
            name: name.map(String::from),
            value: RegValue::Sz(v),
        }
    }

    /// Argumentos do `reg.exe add`.
    pub fn args(&self) -> Vec<String> {
        let mut a = vec!["add".to_string(), self.key.clone()];
        match &self.name {
            Some(n) => {
                a.push("/v".into());
                a.push(n.clone());
            }
            None => a.push("/ve".into()),
        }
        match &self.value {
            RegValue::Sz(s) => {
                a.extend(["/t".into(), "REG_SZ".into(), "/d".into(), s.clone()]);
            }
            RegValue::Dword(d) => {
                a.extend(["/t".into(), "REG_DWORD".into(), "/d".into(), d.to_string()]);
            }
        }
        a.push("/f".into());
        a
    }
}

/// Entradas de registro de uma instalação: "Adicionar ou remover programas"
/// e, se pedido, a associação de `.neoveg`.
pub fn registry_writes(
    scope: Scope,
    install_dir: &Path,
    version: &str,
    size_kb: u64,
    associate: bool,
) -> Vec<RegWrite> {
    let hive = scope.hive();
    let exe = install_dir.join(APP_EXE);
    let exe_s = exe.display().to_string();
    let uninst = install_dir.join(UNINSTALLER_EXE).display().to_string();
    let key = format!(r"{hive}\{UNINSTALL_KEY}");
    let uninstall_cmd = format!("\"{uninst}\" --uninstall {}", scope.cli_flag());
    let mut w = vec![
        RegWrite::sz(key.clone(), Some("DisplayName"), "Neoveg".into()),
        RegWrite::sz(key.clone(), Some("DisplayVersion"), version.into()),
        RegWrite::sz(key.clone(), Some("Publisher"), "Neoveg".into()),
        RegWrite::sz(
            key.clone(),
            Some("InstallLocation"),
            install_dir.display().to_string(),
        ),
        RegWrite::sz(key.clone(), Some("DisplayIcon"), format!("{exe_s},0")),
        RegWrite::sz(key.clone(), Some("UninstallString"), uninstall_cmd.clone()),
        RegWrite::sz(
            key.clone(),
            Some("QuietUninstallString"),
            format!("{uninstall_cmd} --silent"),
        ),
        RegWrite {
            key: key.clone(),
            name: Some("NoModify".into()),
            value: RegValue::Dword(1),
        },
        RegWrite {
            key: key.clone(),
            name: Some("NoRepair".into()),
            value: RegValue::Dword(1),
        },
        RegWrite {
            key,
            name: Some("EstimatedSize".into()),
            value: RegValue::Dword(u32::try_from(size_kb).unwrap_or(u32::MAX)),
        },
    ];
    if associate {
        let classes = format!(r"{hive}\Software\Classes");
        for ext in ASSOCIATED_EXTS {
            w.push(RegWrite::sz(
                format!(r"{classes}\{ext}"),
                None,
                PROG_ID.into(),
            ));
        }
        let prog = format!(r"{classes}\{PROG_ID}");
        w.push(RegWrite::sz(prog.clone(), None, "Projeto do Neoveg".into()));
        w.push(RegWrite::sz(
            format!(r"{prog}\DefaultIcon"),
            None,
            format!("{exe_s},0"),
        ));
        w.push(RegWrite::sz(
            format!(r"{prog}\shell\open\command"),
            None,
            format!("\"{exe_s}\" \"%1\""),
        ));
    }
    w
}

/// Chaves a apagar ao desinstalar.
pub fn registry_deletes(scope: Scope, associated: bool) -> Vec<String> {
    let hive = scope.hive();
    let mut keys = vec![format!(r"{hive}\{UNINSTALL_KEY}")];
    if associated {
        let classes = format!(r"{hive}\Software\Classes");
        keys.push(format!(r"{classes}\{PROG_ID}"));
        for ext in ASSOCIATED_EXTS {
            keys.push(format!(r"{classes}\{ext}"));
        }
    }
    keys
}

/// Roda um programa sem janela de console e espera.
fn run_hidden(program: &str, args: &[String]) -> Result<(), String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(format!("{program}: {}", err.trim()))
    }
}

pub fn apply_registry(writes: &[RegWrite]) -> Result<(), String> {
    for w in writes {
        run_hidden("reg.exe", &w.args())?;
    }
    Ok(())
}

/// Apaga chaves do registro (as que não existem não são erro).
pub fn delete_registry(keys: &[String]) {
    for k in keys {
        let _ = run_hidden("reg.exe", &["delete".into(), k.clone(), "/f".into()]);
    }
}

/// Texto entre aspas simples para o PowerShell.
pub fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Cria um atalho `.lnk` (WScript.Shell pelo PowerShell).
pub fn create_shortcut(lnk: &Path, target: &Path, workdir: &Path) -> Result<(), String> {
    if let Some(parent) = lnk.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut({lnk});\
         $s.TargetPath={t};$s.WorkingDirectory={w};$s.IconLocation={i};$s.Save()",
        lnk = ps_quote(&lnk.display().to_string()),
        t = ps_quote(&target.display().to_string()),
        w = ps_quote(&workdir.display().to_string()),
        i = ps_quote(&format!("{},0", target.display())),
    );
    run_hidden(
        "powershell.exe",
        &[
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            script,
        ],
    )
}

/// O processo atual tem direitos de administrador (nível de integridade
/// alto, SID S-1-16-12288).
pub fn is_elevated() -> bool {
    let mut cmd = Command::new("whoami.exe");
    cmd.arg("/groups");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("S-1-16-12288"))
        .unwrap_or(false)
}

/// Roda `exe` com `args` como administrador (pede a confirmação do UAC) e
/// espera. Devolve o código de saída.
pub fn run_elevated(exe: &Path, args: &[String]) -> Result<i32, String> {
    let arg_list = args
        .iter()
        .map(|a| ps_quote(&format!("\"{a}\"")))
        .collect::<Vec<_>>()
        .join(",");
    let script = format!(
        "$p=Start-Process -FilePath {exe} -ArgumentList @({arg_list}) -Verb RunAs -Wait -PassThru;\
         exit $p.ExitCode",
        exe = ps_quote(&exe.display().to_string()),
    );
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let st = cmd.status().map_err(|e| format!("powershell: {e}"))?;
    Ok(st.code().unwrap_or(1))
}

/// Tamanho em KB dos arquivos instalados (para "Adicionar ou remover").
pub fn installed_size_kb(install_dir: &Path, files: &[PathBuf]) -> u64 {
    let bytes: u64 = files
        .iter()
        .filter_map(|f| std::fs::metadata(install_dir.join(f)).ok())
        .map(|m| m.len())
        .fold(0u64, |a, b| a.saturating_add(b));
    bytes.div_ceil(1024)
}

/// Apaga, de baixo para cima, as pastas vazias sob `root` (sem apagar `root`).
pub fn remove_empty_dirs(root: &Path) {
    fn walk(d: &Path, depth: u32) {
        if depth > 32 {
            return;
        }
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, depth + 1);
                    let _ = std::fs::remove_dir(&p);
                }
            }
        }
    }
    walk(root, 0);
}

/// Script que, depois que o processo `pid` (o desinstalador) termina, apaga
/// o desinstalador, o manifesto e a pasta. A pasta só some se tiver ficado
/// vazia: arquivos do usuário postos lá não são apagados.
pub fn self_removal_script(install_dir: &Path, pid: u32) -> String {
    let uninst = install_dir.join(UNINSTALLER_EXE);
    let manifest = Manifest::path(install_dir);
    format!(
        "Wait-Process -Id {pid} -ErrorAction SilentlyContinue;Start-Sleep -Milliseconds 300;\
         Remove-Item -LiteralPath {u},{m} -Force -ErrorAction SilentlyContinue;\
         try{{[IO.Directory]::Delete({d})}}catch{{}}",
        u = ps_quote(&uninst.display().to_string()),
        m = ps_quote(&manifest.display().to_string()),
        d = ps_quote(&install_dir.display().to_string()),
    )
}

/// Agenda a limpeza final para depois que este desinstalador fechar (um
/// executável em uso não pode ser apagado).
pub fn schedule_self_removal(install_dir: &Path) -> Result<(), String> {
    let script = self_removal_script(install_dir, std::process::id());
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("powershell: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn registro_de_instalacao_por_usuario_e_para_todos() {
        let dir = PathBuf::from(r"C:\Programas\Neoveg");
        let user = registry_writes(Scope::User, &dir, "0.1.0", 40_000, true);
        assert!(user.iter().all(|w| w.key.starts_with("HKCU\\")));
        let all = registry_writes(Scope::AllUsers, &dir, "0.1.0", 40_000, false);
        assert!(all.iter().all(|w| w.key.starts_with("HKLM\\")));
        // sem associação: nada em Software\Classes
        assert!(all.iter().all(|w| !w.key.contains("Classes")));
        let uninst = user
            .iter()
            .find(|w| w.name.as_deref() == Some("UninstallString"))
            .expect("UninstallString");
        assert_eq!(
            uninst.value,
            RegValue::Sz(r#""C:\Programas\Neoveg\desinstalar.exe" --uninstall --user"#.into())
        );
        let open = user
            .iter()
            .find(|w| w.key.ends_with(r"shell\open\command"))
            .expect("comando de abrir");
        assert_eq!(
            open.value,
            RegValue::Sz(r#""C:\Programas\Neoveg\neoveg.exe" "%1""#.into())
        );
        // tamanho gigante não estoura o DWORD
        let big = registry_writes(Scope::User, &dir, "0.1.0", u64::MAX, false);
        assert!(big.iter().any(|w| w.value == RegValue::Dword(u32::MAX)));
    }

    #[test]
    fn argumentos_do_reg_exe() {
        let w = RegWrite {
            key: r"HKCU\Software\X".into(),
            name: None,
            value: RegValue::Sz("v".into()),
        };
        assert_eq!(
            w.args(),
            [
                "add",
                r"HKCU\Software\X",
                "/ve",
                "/t",
                "REG_SZ",
                "/d",
                "v",
                "/f"
            ]
        );
        let d = RegWrite {
            key: "HKLM\\K".into(),
            name: Some("NoModify".into()),
            value: RegValue::Dword(1),
        };
        assert_eq!(
            d.args(),
            [
                "add",
                "HKLM\\K",
                "/v",
                "NoModify",
                "/t",
                "REG_DWORD",
                "/d",
                "1",
                "/f"
            ]
        );
        assert_eq!(registry_deletes(Scope::User, false).len(), 1);
        assert_eq!(
            registry_deletes(Scope::AllUsers, true).len(),
            2 + ASSOCIATED_EXTS.len()
        );
    }

    /// A limpeza final espera o desinstalador fechar (em uso, o .exe não
    /// pode ser apagado) e não apaga a pasta com arquivos do usuário.
    #[test]
    #[cfg(windows)]
    fn limpeza_final_espera_o_desinstalador_e_nao_apaga_pasta_cheia() {
        let s = self_removal_script(Path::new(r"C:\Apps\O'Neoveg"), 4242);
        assert!(s.starts_with("Wait-Process -Id 4242"));
        assert!(s.contains(r"'C:\Apps\O''Neoveg\desinstalar.exe'"));
        // Directory.Delete sem recursão falha se a pasta não estiver vazia
        assert!(s.contains(r"[IO.Directory]::Delete('C:\Apps\O''Neoveg')"));
        assert!(!s.contains("-Recurse"));
    }

    #[test]
    fn aspas_do_powershell_nao_quebram_o_comando() {
        assert_eq!(ps_quote("C:\\a b"), "'C:\\a b'");
        assert_eq!(ps_quote("O'Brien"), "'O''Brien'");
    }

    #[test]
    fn manifesto_ida_e_volta_e_hostil() {
        let dir = std::env::temp_dir().join(format!("neoveg-manifesto-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let m = Manifest {
            produto: "Neoveg".into(),
            versao: "0.1.0".into(),
            escopo: Scope::User,
            arquivos: vec![PathBuf::from("neoveg.exe")],
            atalhos: vec![],
            associacoes: true,
        };
        m.save(&dir).expect("grava");
        assert_eq!(Manifest::load(&dir).expect("lê"), m);
        std::fs::write(Manifest::path(&dir), "{ isto não é json").expect("grava");
        assert!(Manifest::load(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
