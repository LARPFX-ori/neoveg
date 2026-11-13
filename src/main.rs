#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! NVOV — editor de vídeo não-linear em Rust puro, inspirado no VEGAS Pro.
//!
//! Uso:
//!   nvov [projeto.nvp | mídia]           abre a interface gráfica
//!   nvov --render projeto.nvp saida.avi  renderiza sem interface (AVI, PNG, Y4M ou WAV)

mod engine;
mod media;
mod model;
mod playback;
mod render;
mod scopes;
mod ui;

use std::path::PathBuf;
use std::sync::Arc;

fn headless_render(project: PathBuf, out: PathBuf) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(&project)?;
    let mut p: model::Project = serde_json::from_str(&text)?;
    model::bump_ids(p.max_id());
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
    let ext = out.extension().and_then(|e| e.to_str()).unwrap_or("avi").to_ascii_lowercase();
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
    if let Some(e) = prog.error.lock().unwrap().clone() {
        anyhow::bail!(e);
    }
    eprintln!("concluído em {:.1} s: {}", *prog.elapsed.lock().unwrap(), out.display());
    Ok(())
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "--render" {
        if let Err(e) = headless_render(PathBuf::from(&args[2]), PathBuf::from(&args[3])) {
            eprintln!("erro: {e:#}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let open = args.get(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("NVOV")
            .with_inner_size([1500.0, 920.0])
            .with_min_inner_size([960.0, 600.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("NVOV", options, Box::new(move |cc| Ok(Box::new(ui::App::new(cc, open)))))
}
