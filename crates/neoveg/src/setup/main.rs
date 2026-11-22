//! Instalador do Neoveg para Windows e Linux.
//!
//! O `Neoveg-Setup.exe` (no Linux, `Neoveg-Setup-<versão>-linux-x86_64`) é este executável com o pacote do programa (a pasta
//! `dist/` em ZIP) anexado ao fim; ver `payload.rs` e
//! `tools/build_installer.py`. Instalado, uma cópia sem o pacote vira o
//! `desinstalar.exe`.
//!
//! Uso:
//!   Neoveg-Setup.exe                         assistente
//!   Neoveg-Setup.exe --silent [--user | --all-users] [--dir <pasta>] [--desktop] [--no-assoc]
//!   desinstalar.exe --uninstall [--silent] [--remove-data]

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod gui;
#[cfg(not(windows))]
mod linux;
mod ops;
mod payload;
mod system;

use ops::InstallOptions;
use std::path::PathBuf;
use system::Scope;

/// O que a linha de comando pede.
#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    /// Assistente de instalação.
    InstallGui,
    /// Instalação silenciosa.
    InstallSilent(InstallOptions),
    /// Desinstalação (assistente ou silenciosa).
    Uninstall {
        silent: bool,
        remove_data: bool,
        scope: Option<Scope>,
    },
    /// Ferramenta de build: grava o ícone como `.iconset` do macOS na pasta
    /// (`build_installer.py`). Não instala nada.
    WriteIconset(PathBuf),
}

/// Interpreta os argumentos (sem o nome do programa).
pub fn parse_args(args: &[String]) -> Result<Mode, String> {
    let mut silent = false;
    let mut uninstall = false;
    let mut remove_data = false;
    let mut scope: Option<Scope> = None;
    let mut dir: Option<PathBuf> = None;
    let mut desktop = false;
    let mut assoc = true;
    let mut iconset: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--silent" | "/S" | "/s" => silent = true,
            "--uninstall" => uninstall = true,
            "--remove-data" => remove_data = true,
            "--user" => scope = Some(Scope::User),
            "--all-users" => scope = Some(Scope::AllUsers),
            "--desktop" => desktop = true,
            "--no-assoc" => assoc = false,
            "--dir" => {
                let d = it.next().ok_or("--dir precisa de uma pasta")?;
                dir = Some(PathBuf::from(d));
            }
            "--write-iconset" => {
                let d = it.next().ok_or("--write-iconset precisa de uma pasta")?;
                iconset = Some(PathBuf::from(d));
            }
            other => return Err(format!("argumento desconhecido: {other}")),
        }
    }
    if let Some(d) = iconset {
        return Ok(Mode::WriteIconset(d));
    }
    if uninstall {
        return Ok(Mode::Uninstall {
            silent,
            remove_data,
            scope,
        });
    }
    if !silent {
        return Ok(Mode::InstallGui);
    }
    let scope = scope.unwrap_or(Scope::User);
    let dir = match dir {
        Some(d) => d,
        None => system::default_install_dir(scope).ok_or("pasta padrão desconhecida")?,
    };
    Ok(Mode::InstallSilent(InstallOptions {
        scope,
        dir,
        desktop_shortcut: desktop,
        associate_files: assoc,
    }))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match neoveg_common::util::guard("instalador", || run(&args)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    };
    std::process::exit(code);
}

fn run(args: &[String]) -> i32 {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    match parse_args(args) {
        Err(e) => {
            eprintln!("{e}");
            2
        }
        Ok(Mode::InstallSilent(opts)) => match ops::install(&opts, &exe, &mut |_, _| {}) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        Ok(Mode::Uninstall {
            silent: true,
            remove_data,
            ..
        }) => match ops::installed_dir_of(&exe) {
            Some(dir) => match ops::uninstall(&dir, remove_data) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            },
            None => {
                eprintln!("o Neoveg não está instalado nesta pasta");
                1
            }
        },
        Ok(Mode::WriteIconset(dir)) => write_iconset(&dir),
        Ok(Mode::Uninstall { silent: false, .. }) => gui::run(exe, true),
        Ok(Mode::InstallGui) => gui::run(exe, false),
    }
}

#[cfg(not(windows))]
fn write_iconset(dir: &std::path::Path) -> i32 {
    match linux::write_macos_iconset(dir) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

#[cfg(windows)]
fn write_iconset(_dir: &std::path::Path) -> i32 {
    eprintln!("--write-iconset só existe no Linux e no macOS");
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn linha_de_comando_do_instalador() {
        assert_eq!(parse_args(&[]), Ok(Mode::InstallGui));
        let m = parse_args(&a(&[
            "--silent",
            "--all-users",
            "--dir",
            r"D:\Apps\Neoveg",
            "--desktop",
        ]))
        .expect("válido");
        assert_eq!(
            m,
            Mode::InstallSilent(InstallOptions {
                scope: Scope::AllUsers,
                dir: PathBuf::from(r"D:\Apps\Neoveg"),
                desktop_shortcut: true,
                associate_files: true,
            })
        );
        assert_eq!(
            parse_args(&a(&["--uninstall", "--silent", "--remove-data"])),
            Ok(Mode::Uninstall {
                silent: true,
                remove_data: true,
                scope: None
            })
        );
        // hostis: sem valor, desconhecido
        assert!(parse_args(&a(&["--dir"])).is_err());
        assert!(parse_args(&a(&["--write-iconset"])).is_err());
        assert_eq!(
            parse_args(&a(&["--write-iconset", "icones.iconset"])),
            Ok(Mode::WriteIconset(PathBuf::from("icones.iconset")))
        );
        assert!(parse_args(&a(&["--formatar-disco"])).is_err());
    }
}
