//! Neoveg — editor de vídeo não-linear em Rust puro, com a semântica de
//! edição do VEGAS Pro.
//!
//! Uso:
//!   nvov [--open] [projeto.nvp | mídia]   abre a interface gráfica
//!   nvov --render projeto.nvp saida.avi   renderiza sem interface (AVI, PNG, Y4M ou WAV)
//!   nvov --run-script script.cs           (ainda não suportado)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// AGENTS.md §0.9: nenhum caminho de pânico em código de produção.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod engine;
#[cfg(test)]
mod fuzz_tests;
mod media;
mod model;
mod playback;
mod render;
mod scopes;
mod ui;
mod util;

use std::path::PathBuf;
use std::sync::Arc;

fn headless_render(project: PathBuf, out: PathBuf) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(&project)?;
    let (mut p, fixes) = model::Project::from_json(&text).map_err(|e| anyhow::anyhow!(e))?;
    for f in fixes {
        eprintln!("aviso: {f}");
    }
    let cache = Arc::new(media::MediaCache::default());
    for m in p.media.iter_mut() {
        match media::load_entry(m) {
            Ok(imp) => {
                if let Some(w) = imp.warning {
                    eprintln!("aviso: {w}");
                }
                cache.insert(m.id, imp.entry);
            }
            Err(e) => eprintln!("mídia offline: {}: {e:#}", m.name),
        }
    }
    p.sort_events();
    let ext = out
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("avi")
        .to_ascii_lowercase();
    let format = match ext.as_str() {
        "png" => render::RenderFormat::PngSequence,
        "y4m" => render::RenderFormat::Y4m,
        "wav" => render::RenderFormat::Wav,
        _ => render::RenderFormat::AviMjpeg,
    };
    let settings = render::RenderSettings {
        format,
        path: out.clone(),
        width: p.settings.width,
        height: p.settings.height,
        start: 0.0,
        end: p.duration(),
        jpeg_quality: 90,
        include_audio: true,
        wav_bits: render::WavBits::Pcm16,
    };
    let prog = render::spawn(p, cache, settings);
    let mut last = -1i64;
    while !prog.finished.load(std::sync::atomic::Ordering::SeqCst) {
        let pct = (prog.fraction() * 100.0) as i64;
        if pct != last {
            eprint!("\rrenderizando... {pct:3}%");
            last = pct;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    eprintln!();
    if let Some(e) = util::lock(&prog.error).clone() {
        anyhow::bail!(e);
    }
    eprintln!(
        "concluído em {:.1} s: {}",
        *util::lock(&prog.elapsed),
        out.display()
    );
    Ok(())
}

/// Ação pedida pela linha de comando (parâmetros CLI são entrada hostil, §0.4).
enum Cli {
    Gui(Option<PathBuf>),
    Render(PathBuf, PathBuf),
    RunScript(PathBuf),
    Help,
}

fn parse_args(args: &[String]) -> Result<Cli, String> {
    let rest = args.get(1..).unwrap_or(&[]);
    match rest {
        [] => Ok(Cli::Gui(None)),
        [flag] if flag == "--help" || flag == "-h" => Ok(Cli::Help),
        [flag, path] if flag == "--open" => Ok(Cli::Gui(Some(PathBuf::from(path)))),
        [flag, script] if flag == "--run-script" => Ok(Cli::RunScript(PathBuf::from(script))),
        [flag, proj, out] if flag == "--render" => {
            Ok(Cli::Render(PathBuf::from(proj), PathBuf::from(out)))
        }
        [path] if !path.starts_with("--") => Ok(Cli::Gui(Some(PathBuf::from(path)))),
        _ => Err("argumentos inválidos; use --help".into()),
    }
}

const HELP: &str = "Uso:
  nvov [--open] [projeto.nvp | mídia]   abre a interface gráfica
  nvov --render projeto.nvp saida.avi   renderiza sem interface (.avi, .png, .y4m ou .wav)
  nvov --run-script script.cs           executa um script (ainda não suportado)";

fn main() -> eframe::Result<()> {
    util::install_panic_hook();
    util::log_info(&format!("Neoveg {} iniciado", env!("CARGO_PKG_VERSION")));
    let args: Vec<String> = std::env::args().collect();
    let open = match parse_args(&args) {
        Ok(Cli::Gui(p)) => p,
        Ok(Cli::Help) => {
            println!("{HELP}");
            return Ok(());
        }
        Ok(Cli::Render(proj, out)) => {
            if let Err(e) = headless_render(proj, out) {
                util::log_error(&format!("render pela linha de comando falhou: {e:#}"));
                eprintln!("erro: {e:#}");
                std::process::exit(1);
            }
            return Ok(());
        }
        Ok(Cli::RunScript(script)) => {
            // §0.2: funcionalidade inacabada retorna erro explícito de não suportado
            eprintln!(
                "erro: execução de scripts (.cs) ainda não é suportada: {}",
                util::redact(&script.display().to_string())
            );
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("erro: {e}\n{HELP}");
            std::process::exit(2);
        }
    };
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("NVOV")
            .with_inner_size([1500.0, 920.0])
            .with_min_inner_size([960.0, 600.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "NVOV",
        options,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, open)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn argumentos_hostis_nao_entram_em_panico() {
        assert!(matches!(parse_args(&a(&["nvov"])), Ok(Cli::Gui(None))));
        assert!(matches!(
            parse_args(&a(&["nvov", "--open", "x.nvp"])),
            Ok(Cli::Gui(Some(_)))
        ));
        assert!(matches!(
            parse_args(&a(&["nvov", "--render", "a", "b"])),
            Ok(Cli::Render(..))
        ));
        assert!(matches!(
            parse_args(&a(&["nvov", "--run-script", "s.cs"])),
            Ok(Cli::RunScript(_))
        ));
        assert!(parse_args(&a(&["nvov", "--render"])).is_err());
        assert!(parse_args(&a(&["nvov", "--xyz", "1", "2", "3", "4"])).is_err());
        assert!(parse_args(&[]).is_ok());
    }
}
