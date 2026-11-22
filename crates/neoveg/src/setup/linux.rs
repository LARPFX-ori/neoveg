//! Integração com o Linux sem `unsafe`, pelas especificações públicas do
//! freedesktop.org: *Desktop Entry Specification* 1.5 (atalho no menu de
//! aplicativos), *Shared MIME-info Database* 0.21 (tipo dos arquivos
//! `.neoveg`) e *XDG Base Directory* 0.8 / `xdg-user-dirs` (pastas).
//!
//! - Só para mim: programa em `~/.local/opt/neoveg`, atalho em
//!   `~/.local/share/applications`, comando `neoveg` em `~/.local/bin`.
//! - Para todos: programa em `/opt/neoveg`, atalho e tipo MIME em
//!   `/usr/local/share`, comando em `/usr/local/bin` (com `pkexec`).
//!
//! As ferramentas `update-desktop-database`, `update-mime-database`,
//! `xdg-mime` e `gio` são opcionais: se faltarem, a instalação continua.

use crate::system::{Manifest, Scope, APP_EXE, UNINSTALLER_EXE};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Nome do atalho (o *desktop file ID*), igual ao `app_id` da janela.
pub const DESKTOP_FILE: &str = "neoveg.desktop";
/// Classe da janela do Neoveg (`StartupWMClass`), ver `neoveg_ui`.
pub const APP_ID: &str = "neoveg";
/// Tipo MIME dos projetos.
pub const MIME_TYPE: &str = "application/x-neoveg-project";
/// Arquivo do tipo MIME em `mime/packages`.
pub const MIME_FILE: &str = "neoveg.xml";
/// Ícone gravado na pasta do programa (desenhado em código pelo Neoveg).
pub const ICON_FILE: &str = "neoveg.png";

/// Pastas do sistema usadas por uma instalação.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirs {
    /// `~/.local/share` ou `/usr/local/share`.
    pub data: PathBuf,
    /// `~/.local/bin` ou `/usr/local/bin`.
    pub bin: PathBuf,
    /// Área de Trabalho do usuário (só na instalação para mim).
    pub desktop: Option<PathBuf>,
}

impl Dirs {
    pub fn applications(&self) -> PathBuf {
        self.data.join("applications")
    }

    pub fn mime(&self) -> PathBuf {
        self.data.join("mime")
    }

    pub fn mime_package(&self) -> PathBuf {
        self.mime().join("packages").join(MIME_FILE)
    }
}

fn abs(p: Option<PathBuf>) -> Option<PathBuf> {
    p.filter(|p| p.is_absolute())
}

fn env_path(k: &str) -> Option<PathBuf> {
    std::env::var_os(k).map(PathBuf::from)
}

/// Pasta padrão de instalação.
pub fn default_install_dir(scope: Scope) -> Option<PathBuf> {
    default_install_dir_from(scope, env_path("HOME"))
}

pub fn default_install_dir_from(scope: Scope, home: Option<PathBuf>) -> Option<PathBuf> {
    match scope {
        Scope::User => abs(home).map(|h| h.join(".local").join("opt").join("neoveg")),
        Scope::AllUsers => Some(PathBuf::from("/opt/neoveg")),
    }
}

/// Lê `XDG_DESKTOP_DIR` de um `user-dirs.dirs`. O valor é `"$HOME/..."` ou
/// um caminho absoluto; igual à pasta pessoal, a Área de Trabalho está
/// desligada.
pub fn parse_desktop_dir(user_dirs: &str, home: &Path) -> Option<PathBuf> {
    for line in user_dirs.lines() {
        let Some(rest) = line.trim().strip_prefix("XDG_DESKTOP_DIR=") else {
            continue;
        };
        let v = rest.trim();
        let v = v
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(v);
        let p = if v == "$HOME" || v == "$HOME/" {
            return None;
        } else if let Some(r) = v.strip_prefix("$HOME/") {
            home.join(r)
        } else {
            PathBuf::from(v)
        };
        return p.is_absolute().then_some(p);
    }
    None
}

/// Pastas para `scope`, a partir do ambiente dado (regra pura, testável).
pub fn dirs_from(
    scope: Scope,
    home: Option<PathBuf>,
    xdg_data_home: Option<PathBuf>,
    user_dirs: Option<&str>,
) -> Option<Dirs> {
    match scope {
        Scope::AllUsers => Some(Dirs {
            data: PathBuf::from("/usr/local/share"),
            bin: PathBuf::from("/usr/local/bin"),
            desktop: None,
        }),
        Scope::User => {
            let home = abs(home)?;
            let data = abs(xdg_data_home).unwrap_or_else(|| home.join(".local").join("share"));
            let desktop = user_dirs
                .and_then(|u| parse_desktop_dir(u, &home))
                .or_else(|| Some(home.join("Desktop")).filter(|d| d.is_dir()));
            Some(Dirs {
                data,
                bin: home.join(".local").join("bin"),
                desktop,
            })
        }
    }
}

/// Maior `user-dirs.dirs` lido.
const MAX_USER_DIRS_BYTES: u64 = 64 << 10;

/// Pastas para `scope`, do ambiente deste processo.
pub fn dirs(scope: Scope) -> Option<Dirs> {
    let home = abs(env_path("HOME"));
    let config =
        abs(env_path("XDG_CONFIG_HOME")).or_else(|| home.as_ref().map(|h| h.join(".config")));
    let user_dirs = config.and_then(|c| {
        let p = c.join("user-dirs.dirs");
        let len = std::fs::metadata(&p).ok()?.len();
        (len <= MAX_USER_DIRS_BYTES)
            .then(|| std::fs::read_to_string(&p).ok())
            .flatten()
    });
    dirs_from(scope, home, env_path("XDG_DATA_HOME"), user_dirs.as_deref())
}

/// Caminho como argumento do `Exec=` de um atalho: entre aspas, com `"`,
/// `` ` ``, `$` e `\` escapados, `%` dobrado e, por fim, o escape de
/// valores de texto (`\` vira `\\`). Caracteres de controle são recusados.
pub fn exec_arg(path: &Path) -> Result<String, String> {
    let s = path
        .to_str()
        .ok_or_else(|| format!("caminho fora de UTF-8: {}", path.display()))?;
    if s.chars().any(char::is_control) {
        return Err(format!("caminho com caractere de controle: {s:?}"));
    }
    let mut quoted = String::with_capacity(s.len() + 2);
    quoted.push('"');
    for c in s.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    Ok(quoted.replace('\\', "\\\\"))
}

/// Valor de texto de um atalho (`\` vira `\\`; sem quebras de linha).
fn desktop_value(path: &Path) -> Result<String, String> {
    let s = path
        .to_str()
        .ok_or_else(|| format!("caminho fora de UTF-8: {}", path.display()))?;
    if s.chars().any(char::is_control) {
        return Err(format!("caminho com caractere de controle: {s:?}"));
    }
    Ok(s.replace('\\', "\\\\"))
}

/// Conteúdo do `neoveg.desktop`.
pub fn desktop_entry(install_dir: &Path, associate: bool) -> Result<String, String> {
    let exe = exec_arg(&install_dir.join(APP_EXE))?;
    let try_exec = desktop_value(&install_dir.join(APP_EXE))?;
    let icon = desktop_value(&install_dir.join(ICON_FILE))?;
    let uninst = exec_arg(&install_dir.join(UNINSTALLER_EXE))?;
    let workdir = desktop_value(install_dir)?;
    let mime = if associate {
        format!("MimeType={MIME_TYPE};\n")
    } else {
        String::new()
    };
    Ok(format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.5\n\
         Name=Neoveg\n\
         GenericName=Video Editor\n\
         GenericName[pt_BR]=Editor de vídeo\n\
         GenericName[es]=Editor de vídeo\n\
         GenericName[fr]=Éditeur vidéo\n\
         GenericName[de]=Videoeditor\n\
         GenericName[ja]=動画編集ソフト\n\
         Comment=Non-linear video editor\n\
         Comment[pt_BR]=Editor de vídeo não-linear\n\
         Exec={exe} %f\n\
         TryExec={try_exec}\n\
         Path={workdir}\n\
         Icon={icon}\n\
         Terminal=false\n\
         Categories=AudioVideo;Video;AudioVideoEditing;\n\
         Keywords=video;editor;timeline;nle;\n\
         {mime}\
         StartupNotify=true\n\
         StartupWMClass={APP_ID}\n\
         Actions=uninstall;\n\
         \n\
         [Desktop Action uninstall]\n\
         Name=Uninstall Neoveg\n\
         Name[pt_BR]=Desinstalar o Neoveg\n\
         Exec={uninst} --uninstall\n"
    ))
}

/// Tipo MIME dos projetos `.neoveg` (Shared MIME-info).
pub fn mime_xml() -> &'static str {
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
     <mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">\n  \
       <mime-type type=\"application/x-neoveg-project\">\n    \
         <comment>Neoveg project</comment>\n    \
         <comment xml:lang=\"pt_BR\">Projeto do Neoveg</comment>\n    \
         <generic-icon name=\"video-x-generic\"/>\n    \
         <glob pattern=\"*.neoveg\"/>\n  \
       </mime-type>\n\
     </mime-info>\n"
}

/// Lado do ícone gravado (os menus mostram até 256 pixels).
pub const ICON_SIDE: u32 = 256;

/// Grava o ícone do Neoveg em PNG.
pub fn write_icon(path: &Path) -> Result<(), String> {
    write_icon_sized(path, ICON_SIDE)
}

/// Grava o ícone do Neoveg em PNG com `side` pixels de lado.
pub fn write_icon_sized(path: &Path, side: u32) -> Result<(), String> {
    let icon = neoveg_ui::app_icon(side);
    let img = image::RgbaImage::from_raw(icon.width, icon.height, icon.rgba)
        .ok_or("ícone do Neoveg com tamanho inválido")?;
    img.save_with_format(path, image::ImageFormat::Png)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Arquivos de um `.iconset` do macOS: nome e lado em pixels. São os nomes
/// que o `iconutil -c icns` exige (tamanhos de 16 a 512 pontos, em 1x e 2x).
pub const MACOS_ICONSET: [(&str, u32); 10] = [
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
];

/// Grava o ícone do Neoveg como `.iconset` do macOS em `dir` (usado pelo
/// `build_installer.py` para montar o `Neoveg.icns` do pacote `.app`).
pub fn write_macos_iconset(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for (name, side) in MACOS_ICONSET {
        write_icon_sized(&dir.join(name), side)?;
    }
    Ok(())
}

fn write_file(path: &Path, contents: &str, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(path, contents).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Marca arquivos como executáveis (`rwxr-xr-x`).
pub fn set_executable(paths: &[PathBuf]) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    for p in paths {
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(())
}

/// Roda uma ferramenta opcional e ignora o resultado (ausente ou com erro,
/// a instalação segue).
fn run_optional(program: &str, args: &[&std::ffi::OsStr]) {
    let _ = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn refresh_databases(d: &Dirs) {
    run_optional("update-desktop-database", &[d.applications().as_os_str()]);
    if d.mime().join("packages").is_dir() {
        run_optional("update-mime-database", &[d.mime().as_os_str()]);
    }
}

/// O link `link` aponta para dentro de `install_dir`.
fn symlink_into(link: &Path, install_dir: &Path) -> bool {
    std::fs::symlink_metadata(link).is_ok_and(|m| m.file_type().is_symlink())
        && std::fs::read_link(link).is_ok_and(|t| t.starts_with(install_dir))
}

/// Cria o comando `neoveg` em `bin`. Um arquivo que não seja um link para
/// este programa não é substituído (devolve `None`).
fn link_command(bin: &Path, install_dir: &Path) -> Result<Option<PathBuf>, String> {
    let link = bin.join(APP_EXE);
    if std::fs::symlink_metadata(&link).is_ok() {
        let ours = std::fs::symlink_metadata(&link).is_ok_and(|m| m.file_type().is_symlink());
        if !ours {
            return Ok(None);
        }
        std::fs::remove_file(&link).map_err(|e| format!("{}: {e}", link.display()))?;
    }
    std::fs::create_dir_all(bin).map_err(|e| format!("{}: {e}", bin.display()))?;
    std::os::unix::fs::symlink(install_dir.join(APP_EXE), &link)
        .map_err(|e| format!("{}: {e}", link.display()))?;
    Ok(Some(link))
}

/// Integra a instalação ao sistema. Devolve o que foi criado fora da pasta
/// do programa (vai para o manifesto, para a desinstalação).
pub fn integrate(
    scope: Scope,
    install_dir: &Path,
    desktop_shortcut: bool,
    associate: bool,
) -> Result<Vec<PathBuf>, String> {
    let d = dirs(scope).ok_or("pasta pessoal ($HOME) desconhecida")?;
    let entry = desktop_entry(install_dir, associate)?;
    let mut created = Vec::new();

    let menu = d.applications().join(DESKTOP_FILE);
    write_file(&menu, &entry, 0o644)?;
    created.push(menu);

    if associate {
        let pkg = d.mime_package();
        write_file(&pkg, mime_xml(), 0o644)?;
        created.push(pkg);
    }
    if desktop_shortcut {
        if let Some(desk) = &d.desktop {
            let lnk = desk.join(DESKTOP_FILE);
            // atalhos na Área de Trabalho precisam ser executáveis
            write_file(&lnk, &entry, 0o755)?;
            // GNOME só abre atalhos marcados como confiáveis
            run_optional(
                "gio",
                &[
                    "set".as_ref(),
                    lnk.as_os_str(),
                    "metadata::trusted".as_ref(),
                    "true".as_ref(),
                ],
            );
            created.push(lnk);
        }
    }
    if let Some(link) = link_command(&d.bin, install_dir)? {
        created.push(link);
    }
    refresh_databases(&d);
    if associate && scope == Scope::User {
        run_optional(
            "xdg-mime",
            &[
                "default".as_ref(),
                DESKTOP_FILE.as_ref(),
                MIME_TYPE.as_ref(),
            ],
        );
    }
    Ok(created)
}

/// Um item do manifesto pode ser apagado ao desinstalar: um atalho ou tipo
/// MIME com o nome do Neoveg, ou um link para dentro da pasta instalada.
/// Protege contra um manifesto adulterado que liste arquivos alheios.
pub fn removable(item: &Path, install_dir: &Path) -> bool {
    let name = item.file_name().and_then(|n| n.to_str());
    item.is_absolute()
        && (matches!(name, Some(DESKTOP_FILE) | Some(MIME_FILE)) || symlink_into(item, install_dir))
}

/// Apaga os itens [`removable`] da lista.
pub fn remove_items(install_dir: &Path, items: &[PathBuf]) {
    for item in items {
        if removable(item, install_dir) {
            let _ = std::fs::remove_file(item);
        }
    }
}

/// Desfaz [`integrate`].
pub fn deintegrate(install_dir: &Path, m: &Manifest) {
    remove_items(install_dir, &m.atalhos);
    if let Some(d) = dirs(m.escopo) {
        refresh_databases(&d);
    }
}

/// UID efetivo de um `/proc/<pid>/status`.
pub fn effective_uid(status: &str) -> Option<u32> {
    let line = status.lines().find(|l| l.starts_with("Uid:"))?;
    line.split_whitespace().nth(2)?.parse().ok()
}

/// O processo atual roda como `root`.
pub fn is_elevated() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| effective_uid(&s))
        == Some(0)
}

/// Roda `exe` com `args` como administrador (`pkexec`, que pede a senha) e
/// espera. Devolve o código de saída.
pub fn run_elevated(exe: &Path, args: &[String]) -> Result<i32, String> {
    let st = Command::new("pkexec")
        .arg(exe)
        .args(args)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!(
                    "pkexec não encontrado. Para instalar para todos, rode no terminal:\n\
                     sudo \"{}\" {}",
                    exe.display(),
                    args.join(" ")
                )
            } else {
                format!("pkexec: {e}")
            }
        })?;
    match st.code() {
        // 126: senha recusada ou janela cancelada
        Some(126) => Err("a autorização de administrador foi cancelada".into()),
        Some(c) => Ok(c),
        None => Ok(1),
    }
}

/// Fim da desinstalação: no Linux um programa em uso pode ser apagado, então
/// o desinstalador e o manifesto saem já. A pasta só some se ficar vazia.
pub fn schedule_self_removal(install_dir: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(install_dir.join(UNINSTALLER_EXE));
    let _ = std::fs::remove_file(Manifest::path(install_dir));
    let _ = std::fs::remove_dir(install_dir);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn pastas_para_mim_e_para_todos() {
        assert_eq!(
            default_install_dir_from(Scope::User, Some(p("/home/u"))),
            Some(p("/home/u/.local/opt/neoveg"))
        );
        assert_eq!(default_install_dir_from(Scope::User, Some(p("rel"))), None);
        assert_eq!(
            default_install_dir_from(Scope::AllUsers, None),
            Some(p("/opt/neoveg"))
        );
        let ud = "# comentário\nXDG_DESKTOP_DIR=\"$HOME/Área de Trabalho\"\n";
        let d = dirs_from(Scope::User, Some(p("/home/u")), None, Some(ud)).expect("dirs");
        assert_eq!(d.data, p("/home/u/.local/share"));
        assert_eq!(d.bin, p("/home/u/.local/bin"));
        assert_eq!(d.desktop, Some(p("/home/u/Área de Trabalho")));
        assert_eq!(
            d.applications().join(DESKTOP_FILE),
            p("/home/u/.local/share/applications/neoveg.desktop")
        );
        let x = dirs_from(Scope::User, Some(p("/home/u")), Some(p("/x")), None).expect("dirs");
        assert_eq!(x.mime_package(), p("/x/mime/packages/neoveg.xml"));
        // sem HOME não há para onde instalar "só para mim"
        assert_eq!(dirs_from(Scope::User, None, None, None), None);
        let all = dirs_from(Scope::AllUsers, None, None, None).expect("dirs");
        assert_eq!(all.applications(), p("/usr/local/share/applications"));
        assert_eq!(all.desktop, None);
    }

    #[test]
    fn area_de_trabalho_do_user_dirs() {
        let h = p("/home/u");
        assert_eq!(
            parse_desktop_dir("XDG_DESKTOP_DIR=\"/dados/Mesa\"", &h),
            Some(p("/dados/Mesa"))
        );
        // igual à pasta pessoal: desligada
        assert_eq!(parse_desktop_dir("XDG_DESKTOP_DIR=\"$HOME/\"", &h), None);
        // hostis: relativo, vazio, sem a chave
        assert_eq!(parse_desktop_dir("XDG_DESKTOP_DIR=\"rel\"", &h), None);
        assert_eq!(parse_desktop_dir("XDG_DESKTOP_DIR=", &h), None);
        assert_eq!(parse_desktop_dir("XDG_MUSIC_DIR=\"$HOME/M\"", &h), None);
        assert_eq!(parse_desktop_dir("XDG_DESKTOP_DIR=\"", &h), None);
    }

    #[test]
    fn exec_do_atalho_escapa_o_caminho() {
        assert_eq!(
            exec_arg(Path::new("/opt/neoveg/neoveg")).expect("ok"),
            "\"/opt/neoveg/neoveg\""
        );
        // espaço fica entre aspas; $ ` " \ escapados duas vezes; % dobrado
        assert_eq!(
            exec_arg(Path::new("/a b/$x`y\"z\\w/100%")).expect("ok"),
            "\"/a b/\\\\$x\\\\`y\\\\\"z\\\\\\\\w/100%%\""
        );
        assert!(exec_arg(Path::new("/a\nb")).is_err());
    }

    #[test]
    fn atalho_e_tipo_mime() {
        let e = desktop_entry(Path::new("/opt/neoveg"), true).expect("atalho");
        assert!(e.starts_with("[Desktop Entry]\n"));
        assert!(e.contains("\nExec=\"/opt/neoveg/neoveg\" %f\n"));
        assert!(e.contains("\nIcon=/opt/neoveg/neoveg.png\n"));
        assert!(e.contains("\nMimeType=application/x-neoveg-project;\n"));
        assert!(e.contains("\nStartupWMClass=neoveg\n"));
        assert!(e.contains("\nExec=\"/opt/neoveg/desinstalar\" --uninstall\n"));
        let sem = desktop_entry(Path::new("/opt/neoveg"), false).expect("atalho");
        assert!(!sem.contains("MimeType"));
        assert!(desktop_entry(Path::new("/opt/x\ny"), true).is_err());
        let x = mime_xml();
        assert!(x.contains("type=\"application/x-neoveg-project\""));
        assert!(x.contains("<glob pattern=\"*.neoveg\"/>"));
    }

    #[test]
    fn uid_efetivo_do_proc_status() {
        let s = "Name:\tneoveg-setup\nUid:\t1000\t0\t0\t0\nGid:\t1000\t1000\t1000\t1000\n";
        assert_eq!(effective_uid(s), Some(0));
        assert_eq!(effective_uid("Uid:\t1000\t1000\t1000\t1000"), Some(1000));
        assert_eq!(effective_uid("Uid:\tx"), None);
        assert_eq!(effective_uid(""), None);
    }

    /// Instala os atalhos numa pasta pessoal falsa e desinstala: só o que é
    /// do Neoveg sai, mesmo com o manifesto listando arquivos alheios.
    #[test]
    fn desinstalacao_so_apaga_o_que_e_do_neoveg() {
        let root = std::env::temp_dir().join(format!("neoveg-linux-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let inst = root.join("opt").join("neoveg");
        std::fs::create_dir_all(&inst).expect("pasta");
        std::fs::write(inst.join(APP_EXE), b"x").expect("exe");
        let alheio = root.join("alheio.txt");
        std::fs::write(&alheio, b"do usuario").expect("alheio");
        let bin = root.join("bin");
        let link = link_command(&bin, &inst).expect("link").expect("criado");
        assert!(symlink_into(&link, &inst));
        // de novo: o link antigo é trocado
        assert!(link_command(&bin, &inst).expect("link").is_some());
        // um arquivo comum com o mesmo nome não é tocado
        let bin2 = root.join("bin2");
        std::fs::create_dir_all(&bin2).expect("bin2");
        std::fs::write(bin2.join(APP_EXE), b"outro").expect("outro");
        assert_eq!(link_command(&bin2, &inst).expect("ok"), None);

        let menu = root.join("applications").join(DESKTOP_FILE);
        write_file(&menu, "[Desktop Entry]\n", 0o644).expect("menu");
        assert!(removable(&menu, &inst));
        assert!(removable(&link, &inst));
        assert!(!removable(&alheio, &inst));
        assert!(!removable(&bin2.join(APP_EXE), &inst));
        assert!(!removable(Path::new("rel/neoveg.desktop"), &inst));

        remove_items(&inst, &[menu.clone(), link.clone(), alheio.clone()]);
        assert!(!menu.exists());
        assert!(std::fs::symlink_metadata(&link).is_err());
        assert!(alheio.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn iconset_do_macos_tem_os_dez_tamanhos() {
        let dir = std::env::temp_dir().join(format!("neoveg-iconset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_macos_iconset(&dir).unwrap();
        for (name, side) in MACOS_ICONSET {
            let img = image::open(dir.join(name)).unwrap();
            assert_eq!((img.width(), img.height()), (side, side), "{name}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn icone_em_png() {
        let path = std::env::temp_dir().join(format!("neoveg-icone-{}.png", std::process::id()));
        write_icon(&path).expect("grava");
        let bytes = std::fs::read(&path).expect("lê");
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        let _ = std::fs::remove_file(&path);
    }
}
