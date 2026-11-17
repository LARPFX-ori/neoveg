//! Neoveg - editor de vídeo não-linear em Rust puro com semântica VEGAS Pro.
//!
//! Uso na linha de comando:
//!   neoveg [--open] [projeto.neoveg | mídia]  abre a interface gráfica
//!   neoveg --render projeto.neoveg saida.ext  renderiza sem interface (MP4, MOV, 3GP, WMV, AVI, GIF, Y4M, sequências PNG/JPG/TIF/TGA/BMP/WebP/DPX/EXR/PNM/QOI ou áudio WAV/BWF/RF64/W64/AIFF/AU/CAF/QOA/M4A/WMA/MP3)
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
    Encode {
        src: PathBuf,
        out: PathBuf,
        preset_id: Option<String>,
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
        ["--encode", src, out] => Ok(Cli::Encode {
            src: PathBuf::from(src),
            out: PathBuf::from(out),
            preset_id: None,
        }),
        ["--encode", src, out, preset] => Ok(Cli::Encode {
            src: PathBuf::from(src),
            out: PathBuf::from(out),
            preset_id: Some((*preset).to_string()),
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
    let fmt = render::RenderFormat::from_path(out_path);
    let s = render::RenderSettings {
        format: fmt,
        path: out_path.to_path_buf(),
        width: p.settings.width,
        height: p.settings.height,
        start: model::Ticks::ZERO,
        end: p.duration(),
        ..render::RenderSettings::default()
    };
    let prog = render::RenderProgress::default();
    render::run(&p, &media, &s, &prog)?;
    eprintln!("Renderizado: {}", out_path.display());
    Ok(())
}

fn encode_cli(
    src_path: &std::path::Path,
    out_path: &std::path::Path,
    preset_id: Option<&str>,
) -> anyhow::Result<()> {
    use neoveg_engine::encoder::presets::PresetCatalog;
    use neoveg_engine::encoder::worker::encode_item_sync;
    use neoveg_engine::encoder::{EncoderState, JobStatus};

    let cat = PresetCatalog::default();
    let mut state = EncoderState::default();
    let id = if src_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("neoveg") || e.eq_ignore_ascii_case("nvp"))
        .unwrap_or(false)
    {
        let (p, _) = model::Project::load_file(src_path).map_err(anyhow::Error::msg)?;
        state.add_project_snapshot(
            src_path.display().to_string(),
            Some(src_path.to_path_buf()),
            p,
            None,
            &cat,
        )
    } else {
        state.add_media_file(src_path.to_path_buf(), &cat)?
    };
    if let Some(pid) = preset_id {
        if state.apply_preset_or_group(id, &cat, pid, false) == 0 {
            anyhow::bail!("predefinição ou grupo desconhecido: {pid}");
        }
    }
    if let Some(item) = state.items.iter_mut().find(|i| i.id == id) {
        for (idx, out) in item.outputs.iter_mut().enumerate() {
            if idx == 0 && preset_id.is_none() {
                let fmt = render::RenderFormat::from_path(out_path);
                out.format = fmt;
                out.output_path = out_path.to_path_buf();
            } else if idx == 0 {
                out.output_path = out_path.to_path_buf();
            } else {
                let dir = out_path
                    .parent()
                    .filter(|d| !d.as_os_str().is_empty())
                    .unwrap_or_else(|| std::path::Path::new("."));
                let stem = out_path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "saida".into());
                out.output_path = dir.join(format!("{stem}_{idx}.{}", out.format.ext()));
            }
        }
        let results = encode_item_sync(item, &state.prefs, None);
        for r in &results {
            if !matches!(r.status, JobStatus::Done | JobStatus::DoneWithWarnings) {
                anyhow::bail!(
                    "falha no Neoveg Encoder ({:?}): {}",
                    r.status,
                    r.message.as_deref().unwrap_or("erro desconhecido")
                );
            }
            if let Some(p) = &r.final_output_path {
                eprintln!("Codificado: {}", p.display());
            }
        }
    }
    Ok(())
}

const HELP: &str = "Uso:
  neoveg [--open] [projeto.neoveg | mídia]      abre a interface gráfica
  neoveg --render projeto.neoveg saida.ext      renderiza sem interface
  neoveg --encode entrada saida [predefinicao]  transcodifica via Neoveg Encoder
  neoveg --run-script script.<cs|dll>           executa script C#/.NET (ScriptPortal.Vegas) via netcorehost
  neoveg --command <id> [projeto [saida]]       executa comando da Tabela Única sem interface
  neoveg --list-commands                        lista os comandos e atalhos disponíveis";

/// Imprime o resultado de uma sondagem em JSON e devolve o código de saída.
fn print_probe<T: serde::Serialize>(res: Result<T, String>) -> i32 {
    match res.and_then(|list| serde_json::to_string(&list).map_err(|e| e.to_string())) {
        Ok(json) => {
            println!("{json}");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            3
        }
    }
}

/// Modo host de plug-ins (`neoveg --plugin-host probe-ofx <binário>`): roda
/// num subprocesso descartável, carrega o binário de terceiros e responde em
/// JSON no stdout. Fica antes de preferências e logs para não tocar no estado
/// do usuário.
fn plugin_host_mode(args: &[String]) -> Option<i32> {
    use neoveg_engine::plugin_host as ph;
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    match rest.as_slice() {
        ["--plugin-host", "probe-ofx", bin] => Some(print_probe(ph::probe_ofx_in_process(
            std::path::Path::new(bin),
        ))),
        ["--plugin-host", "serve-ofx", bin, id] => {
            Some(ph::serve_ofx(std::path::Path::new(bin), id))
        }
        ["--plugin-host", "probe-vst3", bin] => Some(print_probe(ph::probe_vst3_in_process(
            std::path::Path::new(bin),
        ))),
        ["--plugin-host", "serve-vst3", bin, class_id, sr] => {
            Some(ph::serve_vst3(std::path::Path::new(bin), class_id, sr))
        }
        ["--plugin-host", ..] => {
            eprintln!("modo de host de plug-ins desconhecido");
            Some(2)
        }
        _ => None,
    }
}

fn main() -> anyhow::Result<()> {
    let raw_args: Vec<String> = std::env::args().collect();
    if let Some(code) = plugin_host_mode(&raw_args) {
        std::process::exit(code);
    }
    recovery::install_panic_hook();
    let boot_prefs = prefs::Preferences::load_or_default();
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(boot_prefs.effective_worker_threads())
        .build_global();
    util::log_info(&format!(
        "Neoveg iniciado (workers={})",
        boot_prefs.effective_worker_threads()
    ));

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
        Ok(Cli::Encode {
            src,
            out,
            preset_id,
        }) => {
            if let Err(e) = encode_cli(&src, &out, preset_id.as_deref()) {
                eprintln!("erro no Neoveg Encoder: {e:#}");
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
            for msg in &ctx.messages {
                println!("{msg}");
            }
            println!(
                "Script executado ({}): trilhas={}, marcadores={}, barramentos={}",
                s.display(),
                ctx.session.project.tracks.len(),
                ctx.session.project.markers.len(),
                ctx.session.project.audio_buses.len(),
            );
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

    // marcador de sessão: só no modo de interface (CLI, scripts e hosts de
    // plug-in não criam); detecta sessões anteriores que não fecharam
    // normalmente e mostra o relatório na janela
    let crash_reports = neoveg_engine::session_marker::begin(env!("CARGO_PKG_VERSION"));
    let result = neoveg_ui::run_gui(open, crash_reports).map_err(|e| anyhow::anyhow!("{e}"));
    // saída normal da janela; um pânico na thread principal não chega aqui
    neoveg_engine::session_marker::end();
    result
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
            parse_args(&a(&["neoveg", "--encode", "in.mp4", "out.avi"])),
            Ok(Cli::Encode {
                src: PathBuf::from("in.mp4"),
                out: PathBuf::from("out.avi"),
                preset_id: None,
            })
        );
        assert_eq!(
            parse_args(&a(&[
                "neoveg",
                "--encode",
                "a.neoveg",
                "b.mp4",
                "sys.web_1080p_hq"
            ])),
            Ok(Cli::Encode {
                src: PathBuf::from("a.neoveg"),
                out: PathBuf::from("b.mp4"),
                preset_id: Some("sys.web_1080p_hq".into()),
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
