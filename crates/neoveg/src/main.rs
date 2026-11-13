//! Neoveg - editor de vídeo não-linear em Rust puro com semântica VEGAS Pro.
//!
//! Uso na linha de comando:
//!   neoveg [--open] [projeto.neoveg | mídia]  abre a interface gráfica
//!   neoveg --render projeto.neoveg saida.avi  renderiza sem interface (AVI, PNG, Y4M ou WAV)
//!   neoveg --run-script script.cs             tenta executar um script C# (.NET Host)
//!   neoveg --command <id> [projeto [saida]]   executa comando da Tabela Única headless
//!   neoveg --list-commands                    lista todos os comandos e atalhos

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use neoveg_engine::{media, model, prefs, recovery, render, util};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, PartialEq, Eq)]
enum Cli {
    Help,
    ListCommands,
    Gui(Option<PathBuf>),
    Render {
        proj: PathBuf,
        out: PathBuf,
    },
    RunScript(PathBuf),
    RunCommand {
        id: String,
        proj: Option<PathBuf>,
        out: Option<PathBuf>,
    },
}

fn parse_args(args: &[String]) -> Result<Cli, String> {
    let args: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    if args.len() > 8 {
        return Err("argumentos demais na linha de comando".into());
    }
    match args.as_slice() {
        [] => Ok(Cli::Gui(None)),
        ["-h"] | ["--help"] | ["/?"] => Ok(Cli::Help),
        ["--list-commands"] => Ok(Cli::ListCommands),
        ["--open", p] | [p] if !p.starts_with('-') => Ok(Cli::Gui(Some(PathBuf::from(p)))),
        ["--render", proj, out] => Ok(Cli::Render {
            proj: PathBuf::from(proj),
            out: PathBuf::from(out),
        }),
        ["--run-script", s] => Ok(Cli::RunScript(PathBuf::from(s))),
        ["--command", id] => Ok(Cli::RunCommand {
            id: (*id).to_string(),
            proj: None,
            out: None,
        }),
        ["--command", id, proj] => Ok(Cli::RunCommand {
            id: (*id).to_string(),
            proj: Some(PathBuf::from(proj)),
            out: None,
        }),
        ["--command", id, proj, out] => Ok(Cli::RunCommand {
            id: (*id).to_string(),
            proj: Some(PathBuf::from(proj)),
            out: Some(PathBuf::from(out)),
        }),
        _ => Err(format!("uso inválido: {}\n{}", args.join(" "), HELP)),
    }
}

fn render_cli(proj_path: &std::path::Path, out_path: &std::path::Path) -> anyhow::Result<()> {
    let (p, fixes) = model::Project::load_file(proj_path).map_err(anyhow::Error::msg)?;
    for f in &fixes {
        util::log_warn(&format!("projeto corrigido ao renderizar: {f}"));
    }
    let media = Arc::new(media::MediaCache::default());
    for m in &p.media {
        let mut m = m.clone();
        if let Ok(imp) = media::load_entry(&mut m) {
            media.insert(m.id, imp.entry);
        }
    }
    let fmt = match out_path.extension().and_then(|e| e.to_str()) {
        Some(e) if e.eq_ignore_ascii_case("png") => render::RenderFormat::PngSequence,
        Some(e) if e.eq_ignore_ascii_case("y4m") => render::RenderFormat::Y4m,
        Some(e) if e.eq_ignore_ascii_case("wav") => render::RenderFormat::Wav,
        _ => render::RenderFormat::AviMjpeg,
    };
    let s = render::RenderSettings {
        format: fmt,
        path: out_path.to_path_buf(),
        width: p.settings.width,
        height: p.settings.height,
        start: model::Ticks::ZERO,
        end: p.duration(),
        jpeg_quality: 90,
        include_audio: true,
        wav_bits: render::WavBits::Pcm16,
    };
    let prog = render::RenderProgress::default();
    render::run(&p, &media, &s, &prog)?;
    eprintln!("Renderizado: {}", out_path.display());
    Ok(())
}

const HELP: &str = "Uso:
  neoveg [--open] [projeto.neoveg | mídia]  abre a interface gráfica
  neoveg --render projeto.neoveg saida.avi  renderiza sem interface (.avi, .png, .y4m ou .wav)
  neoveg --run-script script.cs             executa script .NET/C# (em implementação)
  neoveg --command <id> [projeto [saida]]   executa comando da Tabela Única sem interface
  neoveg --list-commands                    lista os comandos e atalhos disponíveis";

fn main() -> anyhow::Result<()> {
    recovery::install_panic_hook();
    util::log_info("Neoveg iniciado");

    let args: Vec<String> = std::env::args().collect();
    let open = match parse_args(&args) {
        Ok(Cli::Help) => {
            println!("{HELP}");
            return Ok(());
        }
        Ok(Cli::ListCommands) => {
            let p = prefs::Preferences::load_or_default();
            for (id, menu, label, sc) in neoveg_scripting::list_commands(&p.shortcuts) {
                println!(
                    "{:<28} {:<36} {} ({})",
                    id,
                    menu,
                    label,
                    sc.unwrap_or_else(|| "-".into())
                );
            }
            return Ok(());
        }
        Ok(Cli::Render { proj, out }) => {
            if let Err(e) = render_cli(&proj, &out) {
                eprintln!("erro ao renderizar: {e:#}");
                std::process::exit(1);
            }
            return Ok(());
        }
        Ok(Cli::RunScript(s)) => {
            let mut ctx = neoveg_scripting::ScriptContext::new(model::Project::new_default());
            if let Err(e) = neoveg_scripting::run_cs_script(&s, &mut ctx) {
                eprintln!("Neoveg: {e}");
                std::process::exit(2);
            }
            return Ok(());
        }
        Ok(Cli::RunCommand { id, proj, out }) => {
            match neoveg_scripting::run_command_headless(&id, proj.as_deref(), out.as_deref()) {
                Ok(msg) => {
                    println!("{msg}");
                    return Ok(());
                }
                Err(e) => {
                    eprintln!("Neoveg: {e}");
                    std::process::exit(1);
                }
            }
        }
        Ok(Cli::Gui(p)) => p,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
    };

    neoveg_ui::run_gui(open).map_err(|e| anyhow::anyhow!("{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn argumentos_hostis_nao_entram_em_panico() {
        assert_eq!(parse_args(&a(&["neoveg"])), Ok(Cli::Gui(None)));
        assert_eq!(
            parse_args(&a(&["neoveg", "--open", "x.neoveg"])),
            Ok(Cli::Gui(Some(PathBuf::from("x.neoveg"))))
        );
        assert_eq!(
            parse_args(&a(&["neoveg", "--render", "a", "b"])),
            Ok(Cli::Render {
                proj: PathBuf::from("a"),
                out: PathBuf::from("b"),
            })
        );
        assert_eq!(
            parse_args(&a(&["neoveg", "--run-script", "s.cs"])),
            Ok(Cli::RunScript(PathBuf::from("s.cs")))
        );
        assert_eq!(
            parse_args(&a(&["neoveg", "--command", "insert.marker"])),
            Ok(Cli::RunCommand {
                id: "insert.marker".into(),
                proj: None,
                out: None,
            })
        );
        assert_eq!(
            parse_args(&a(&["neoveg", "--list-commands"])),
            Ok(Cli::ListCommands)
        );
        assert!(parse_args(&a(&["neoveg", "--render"])).is_err());
        assert!(parse_args(&a(&["neoveg", "--xyz", "1", "2", "3", "4"])).is_err());
    }
}
