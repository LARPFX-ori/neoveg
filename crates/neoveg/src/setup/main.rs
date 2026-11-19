//! Instalador do Neoveg para Windows.
//!
//! O `Neoveg-Setup.exe` é este executável com o pacote do programa (a pasta
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
    /// Instalação silenciosa. `wait_pid`: espera esse processo (o Neoveg
    /// que pediu a atualização) fechar antes; `launch`: abre o Neoveg no fim.
    InstallSilent {
        opts: InstallOptions,
        wait_pid: Option<u32>,
        launch: bool,
    },
    /// Desinstalação (assistente ou silenciosa).
    Uninstall {
        silent: bool,
        remove_data: bool,
        scope: Option<Scope>,
    },
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
    let mut wait_pid: Option<u32> = None;
    let mut launch = false;
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
            "--launch" => launch = true,
            "--wait-pid" => {
                let p = it.next().ok_or("--wait-pid precisa de um número")?;
                wait_pid = Some(p.parse().map_err(|_| format!("--wait-pid inválido: {p}"))?);
            }
            "--dir" => {
                let d = it.next().ok_or("--dir precisa de uma pasta")?;
                dir = Some(PathBuf::from(d));
            }
            other => return Err(format!("argumento desconhecido: {other}")),
        }
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
    Ok(Mode::InstallSilent {
        opts: InstallOptions {
            scope,
            dir,
            desktop_shortcut: desktop,
            associate_files: assoc,
        },
        wait_pid,
        launch,
    })
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
        Ok(Mode::InstallSilent {
            opts,
            wait_pid,
            launch,
        }) => install_silent(&exe, &opts, wait_pid, launch),
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
        Ok(Mode::Uninstall { silent: false, .. }) => gui::run(exe, true),
        Ok(Mode::InstallGui) => gui::run(exe, false),
    }
}

/// Instalação silenciosa (também a atualização pedida pelo Neoveg).
fn install_silent(
    exe: &std::path::Path,
    opts: &InstallOptions,
    wait_pid: Option<u32>,
    launch: bool,
) -> i32 {
    if let Some(pid) = wait_pid {
        system::wait_for_process(pid);
    }
    let result = if opts.scope == Scope::AllUsers && !system::is_elevated() {
        // Arquivos de Programas pede administrador: só a instalação sobe, o
        // Neoveg reaberto no fim continua com o usuário comum
        match system::run_elevated(exe, &opts.to_args()) {
            Ok(0) => Ok(()),
            Ok(c) => Err(format!(
                "a instalação como administrador falhou (código {c})"
            )),
            Err(e) => Err(e),
        }
    } else {
        ops::install(opts, exe, &mut |_, _| {})
    };
    match result {
        Ok(()) => {
            if launch {
                let _ = std::process::Command::new(opts.dir.join(system::APP_EXE)).spawn();
            }
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
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
            Mode::InstallSilent {
                opts: InstallOptions {
                    scope: Scope::AllUsers,
                    dir: PathBuf::from(r"D:\Apps\Neoveg"),
                    desktop_shortcut: true,
                    associate_files: true,
                },
                wait_pid: None,
                launch: false,
            }
        );
        // atualização pedida pelo Neoveg
        match parse_args(&a(&[
            "--silent",
            "--user",
            "--wait-pid",
            "4242",
            "--launch",
        ])) {
            Ok(Mode::InstallSilent {
                wait_pid, launch, ..
            }) => {
                assert_eq!(wait_pid, Some(4242));
                assert!(launch);
            }
            other => panic!("esperada instalação silenciosa: {other:?}"),
        }
        assert!(parse_args(&a(&["--silent", "--wait-pid", "abc"])).is_err());
        assert!(parse_args(&a(&["--silent", "--wait-pid"])).is_err());
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
        assert!(parse_args(&a(&["--formatar-disco"])).is_err());
    }
}
