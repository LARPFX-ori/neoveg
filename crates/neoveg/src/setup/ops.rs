//! Instalar e desinstalar (usados pela interface e pelo modo silencioso).

use crate::payload::{self, PayloadSpan};
use crate::system::{self, Manifest, Scope, APP_EXE, UNINSTALLER_EXE};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Opções da instalação.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallOptions {
    pub scope: Scope,
    pub dir: PathBuf,
    pub desktop_shortcut: bool,
    pub associate_files: bool,
}

impl InstallOptions {
    /// Argumentos para repetir esta instalação em modo silencioso (no
    /// processo com administrador).
    pub fn to_args(&self) -> Vec<String> {
        let mut a = vec![
            "--silent".to_string(),
            self.scope.cli_flag().to_string(),
            "--dir".to_string(),
            self.dir.display().to_string(),
        ];
        if self.desktop_shortcut {
            a.push("--desktop".into());
        }
        if !self.associate_files {
            a.push("--no-assoc".into());
        }
        a
    }
}

/// Versão do programa no pacote (a do próprio instalador).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Copia o executável do instalador sem o pacote anexado: vira o
/// desinstalador (poucos MB em vez do pacote inteiro).
fn write_uninstaller(setup_exe: &Path, span: PayloadSpan, dest: &Path) -> Result<(), String> {
    let mut src = File::open(setup_exe).map_err(|e| format!("{}: {e}", setup_exe.display()))?;
    src.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut out = File::create(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let n = std::io::copy(&mut src.take(span.start), &mut out)
        .map_err(|e| format!("{}: {e}", dest.display()))?;
    if n != span.start {
        return Err("cópia do desinstalador incompleta".into());
    }
    Ok(())
}

/// Instala. `progress(feito, total)` em bytes descompactados.
pub fn install(
    opts: &InstallOptions,
    setup_exe: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), String> {
    if !opts.dir.is_absolute() {
        return Err("a pasta de instalação precisa ser um caminho completo".into());
    }
    let previous = Manifest::load(&opts.dir).ok();
    let (region, span) = payload::open_payload(setup_exe)?;
    let mut archive =
        zip::ZipArchive::new(region).map_err(|e| format!("pacote do programa ilegível: {e}"))?;
    let files = payload::extract_all(&mut archive, &opts.dir, progress).map_err(|e| {
        // 32/5: arquivo em uso no Windows; 26: "Text file busy" no Linux
        if e.contains("os error 32") || e.contains("os error 5") || e.contains("os error 26") {
            format!("{e}\nFeche o Neoveg antes de instalar.")
        } else {
            e
        }
    })?;
    // arquivos da versão anterior que não existem mais
    if let Some(old) = &previous {
        for f in &old.arquivos {
            if !files.contains(f) {
                if let Some(rel) = payload::safe_relative(&f.to_string_lossy()) {
                    let _ = std::fs::remove_file(opts.dir.join(rel));
                }
            }
        }
        system::remove_empty_dirs(&opts.dir);
    }
    write_uninstaller(setup_exe, span, &opts.dir.join(UNINSTALLER_EXE))?;
    let mut files = files;
    let shortcuts = integrate(opts, &mut files)?;
    Manifest {
        produto: "Neoveg".into(),
        versao: VERSION.into(),
        escopo: opts.scope,
        arquivos: files,
        atalhos: shortcuts,
        associacoes: opts.associate_files,
    }
    .save(&opts.dir)
}

/// Atalhos do menu Iniciar e da Área de Trabalho e registro do Windows.
/// Devolve os atalhos criados.
#[cfg(windows)]
// mesma assinatura da versão do Linux, que acrescenta o ícone à lista
#[allow(clippy::ptr_arg)]
fn integrate(opts: &InstallOptions, files: &mut Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    let exe = opts.dir.join(APP_EXE);
    let mut shortcuts = Vec::new();
    if let Some(menu) = system::start_menu_dir(opts.scope) {
        let lnk = menu.join("Neoveg.lnk");
        system::create_shortcut(&lnk, &exe, &opts.dir)?;
        shortcuts.push(lnk);
    }
    if opts.desktop_shortcut {
        if let Some(desk) = system::desktop_dir(opts.scope) {
            let lnk = desk.join("Neoveg.lnk");
            system::create_shortcut(&lnk, &exe, &opts.dir)?;
            shortcuts.push(lnk);
        }
    }
    let size_kb = system::installed_size_kb(&opts.dir, files);
    system::apply_registry(&system::registry_writes(
        opts.scope,
        &opts.dir,
        VERSION,
        size_kb,
        opts.associate_files,
    ))?;
    Ok(shortcuts)
}

/// Permissão de execução, ícone, atalho no menu de aplicativos, tipo MIME
/// de `.neoveg` e comando `neoveg` (ver `linux.rs`). Devolve o que foi
/// criado fora da pasta do programa.
#[cfg(not(windows))]
fn integrate(opts: &InstallOptions, files: &mut Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    use crate::linux;
    linux::set_executable(&[opts.dir.join(APP_EXE), opts.dir.join(UNINSTALLER_EXE)])?;
    linux::write_icon(&opts.dir.join(linux::ICON_FILE))?;
    files.push(PathBuf::from(linux::ICON_FILE));
    linux::integrate(
        opts.scope,
        &opts.dir,
        opts.desktop_shortcut,
        opts.associate_files,
    )
}

/// Pasta dos dados do usuário (preferências, layouts, recuperação, logs):
/// `%APPDATA%\Neoveg` no Windows, `~/.local/share/Neoveg` no Linux,
/// `~/Library/Application Support/Neoveg` no macOS.
pub fn user_data_dir() -> Option<PathBuf> {
    neoveg_common::util::user_data_root().map(|p| p.join(neoveg_common::util::USER_DATA_DIR_NAME))
}

/// Desinstala o que o manifesto de `dir` lista. Os dados do usuário só são
/// apagados com `remove_user_data`.
pub fn uninstall(dir: &Path, remove_user_data: bool) -> Result<(), String> {
    let m = Manifest::load(dir)?;
    #[cfg(windows)]
    {
        for lnk in &m.atalhos {
            if lnk
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
            {
                let _ = std::fs::remove_file(lnk);
            }
        }
        system::delete_registry(&system::registry_deletes(m.escopo, m.associacoes));
    }
    #[cfg(not(windows))]
    crate::linux::deintegrate(dir, &m);
    for f in &m.arquivos {
        // só dentro da pasta instalada, mesmo com manifesto adulterado
        if let Some(rel) = payload::safe_relative(&f.to_string_lossy()) {
            let _ = std::fs::remove_file(dir.join(rel));
        }
    }
    system::remove_empty_dirs(dir);
    if remove_user_data {
        if let Some(data) = user_data_dir() {
            let _ = std::fs::remove_dir_all(data);
        }
    }
    system::schedule_self_removal(dir)
}

/// Pasta instalada do desinstalador em execução: a dele mesmo, se tiver o
/// manifesto ao lado.
pub fn installed_dir_of(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    Manifest::path(dir).is_file().then(|| dir.to_path_buf())
}

/// Lê o escopo do manifesto, se houver.
pub fn installed_scope(dir: &Path) -> Option<Scope> {
    Manifest::load(dir).ok().map(|m| m.escopo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argumentos_da_instalacao_silenciosa() {
        let o = InstallOptions {
            scope: Scope::AllUsers,
            dir: PathBuf::from(r"C:\Program Files\Neoveg"),
            desktop_shortcut: true,
            associate_files: false,
        };
        assert_eq!(
            o.to_args(),
            [
                "--silent",
                "--all-users",
                "--dir",
                r"C:\Program Files\Neoveg",
                "--desktop",
                "--no-assoc"
            ]
        );
    }

    #[test]
    fn pasta_relativa_e_recusada() {
        let o = InstallOptions {
            scope: Scope::User,
            dir: PathBuf::from("relativa"),
            desktop_shortcut: false,
            associate_files: false,
        };
        assert!(install(&o, Path::new("nao-existe.exe"), &mut |_, _| {}).is_err());
    }
}
