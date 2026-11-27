//! Capturas da interface usadas nos READMEs e no Manual do Usuário
//! (`docs/screenshots/*.png`).
//!
//! A interface é desenhada fora da tela com `egui_kittest` + `wgpu` (sem
//! janela, sem captura da área de trabalho), com um projeto demonstrativo de
//! mídia sintética: gradiente, barras de cor, texto e um WAV senoidal gerado
//! na hora. Nenhuma imagem de outro produto entra nas capturas.
//!
//! Não roda no `cargo test` comum: precisa de um adaptador gráfico e
//! reescreve arquivos versionados. Para gerar de novo:
//!
//! ```text
//! NEOVEG_SCREENSHOTS=1 cargo test -p neoveg_ui --lib readme_screenshots -- --ignored --nocapture
//! ```
//!
//! `NEOVEG_SCREENSHOT_DIR` troca a pasta de saída e `NEOVEG_SCREENSHOT_ONLY`
//! filtra por prefixo do nome do arquivo.

use super::*;
use crate::kittest_tests::build_ready_app;
use egui_kittest::Harness;
use neoveg_common::Ticks;
use std::path::{Path, PathBuf};

const W: f32 = 1600.0;
const H: f32 = 900.0;

fn output_dir() -> PathBuf {
    std::env::var_os("NEOVEG_SCREENSHOT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots"))
}

/// WAV PCM 16 bits estéreo, 48 kHz, com um acorde senoidal e fade nas pontas.
fn write_sine_wav(path: &Path, secs: u32) -> std::io::Result<()> {
    let rate = 48_000u32;
    let frames = rate * secs;
    let mut data = Vec::with_capacity(frames as usize * 4);
    for i in 0..frames {
        let t = i as f32 / rate as f32;
        let env = (t / 0.4).min(1.0) * ((secs as f32 - t) / 0.6).clamp(0.0, 1.0);
        let wobble = 0.55 + 0.45 * (t * std::f32::consts::TAU * 1.5).sin().abs();
        let s = ((t * 220.0 * std::f32::consts::TAU).sin() * 0.6
            + (t * 330.0 * std::f32::consts::TAU).sin() * 0.3)
            * env
            * wobble
            * 0.8;
        let v = (s * i16::MAX as f32) as i16;
        data.extend_from_slice(&v.to_le_bytes());
        data.extend_from_slice(&v.to_le_bytes());
    }
    let mut wav = Vec::with_capacity(data.len() + 44);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend_from_slice(&data);
    std::fs::write(path, wav)
}

fn add_generated(app: &mut App, spec: crate::model::GenSpec) -> crate::model::Id {
    let imp = neoveg_engine::media::import_generated(spec);
    let id = imp.item.id;
    app.media.insert(id, imp.entry);
    app.session.register_imported_media(imp.item, None);
    id
}

/// Projeto demonstrativo: título sobre gradiente e barras de cor com
/// crossfade, trilha sonora, dois marcadores, uma região e dois barramentos.
fn build_demo_project(app: &mut App, wav_dir: &Path) -> Result<(), String> {
    use crate::model::{GenSpec, Marker, MarkerKind, Region, TextSpec};
    let secs = |s: f64| Ticks::from_secs_f64_clamped(s);

    let gradient = add_generated(
        app,
        GenSpec::Gradient {
            a: [0.16, 0.12, 0.45, 1.0],
            b: [0.92, 0.25, 0.55, 1.0],
            angle: 35.0,
            radial: false,
        },
    );
    let bars = add_generated(app, GenSpec::ColorBars);
    let title = add_generated(
        app,
        GenSpec::Text(TextSpec {
            text: "Neoveg".into(),
            ..TextSpec::default()
        }),
    );
    let wav = wav_dir.join("Trilha Sonora.wav");
    write_sine_wav(&wav, 8).map_err(|e| format!("WAV de teste: {e}"))?;
    let audio =
        neoveg_engine::media::import_file(&wav).map_err(|e| format!("importar WAV: {e}"))?;
    let audio_id = audio.item.id;
    app.media.insert(audio_id, audio.entry);
    app.session.register_imported_media(audio.item, None);

    app.add_media_to_timeline(gradient, Ticks::ZERO, None);
    app.add_media_to_timeline(bars, secs(2.5), None);
    let title_track = app.insert_track(crate::model::TrackKind::Video);
    app.add_media_to_timeline(title, secs(2.0), Some(title_track));
    app.add_media_to_timeline(audio_id, Ticks::ZERO, None);

    app.project
        .markers
        .push(Marker::new(secs(0.5), MarkerKind::Standard, "Intro"));
    app.project
        .markers
        .push(Marker::new(secs(4.5), MarkerKind::Standard, "Transição"));
    app.project.regions.push(Region {
        id: crate::model::new_id(),
        start: secs(2.0),
        end: secs(5.5),
        label: "Cena principal".into(),
    });
    let _ = app.session.add_audio_bus();
    let _ = app.session.add_audio_bus();
    app.session.cursor = secs(3.5);
    app.session.sel_events.clear();
    app.changed();
    Ok(())
}

/// Roda quadros até os trabalhos em segundo plano (prévia, miniaturas,
/// formas de onda, varredura de loudness) chegarem.
fn settle(harness: &mut Harness<'_, App>) {
    for _ in 0..16 {
        harness.run_steps(3);
        std::thread::sleep(std::time::Duration::from_millis(160));
    }
}

fn save(harness: &mut Harness<'_, App>, dir: &Path, name: &str) -> Result<(), String> {
    settle(harness);
    let img = harness
        .render()
        .map_err(|e| format!("{name}: a renderização wgpu falhou: {e}"))?;
    let path = dir.join(name);
    img.save(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    eprintln!("gravado: {}", path.display());
    Ok(())
}

fn demo_harness(
    wav_dir: &Path,
    setup: impl FnOnce(&mut App, &egui::Context),
) -> Result<Harness<'static, App>, String> {
    let wav_dir = wav_dir.to_path_buf();
    let err_slot = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
    let err_slot_cb = std::sync::Arc::clone(&err_slot);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(W, H))
        .with_pixels_per_point(1.0)
        .wgpu()
        .build_eframe(move |cc| {
            let mut app = build_ready_app(cc);
            if let Err(e) = build_demo_project(&mut app, &wav_dir) {
                *err_slot_cb
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(e);
            }
            app
        });
    if let Some(e) = err_slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
    {
        return Err(e);
    }
    harness.run_steps(2);
    let ctx = harness.ctx.clone();
    harness.state_mut().exec_cmd(Some(&ctx), "timeline.zoom_in");
    harness.state_mut().exec_cmd(Some(&ctx), "timeline.zoom_in");
    setup(harness.state_mut(), &ctx);
    Ok(harness)
}

fn first_video_event_id(app: &App, nth: usize) -> Option<crate::model::Id> {
    app.project
        .tracks
        .iter()
        .filter(|t| t.kind == crate::model::TrackKind::Video)
        .flat_map(|t| t.events.iter())
        .map(|e| e.id)
        .nth(nth)
}

#[test]
#[ignore = "gera docs/screenshots: NEOVEG_SCREENSHOTS=1 ... -- --ignored"]
fn readme_screenshots() -> Result<(), String> {
    if std::env::var("NEOVEG_SCREENSHOTS").as_deref() != Ok("1") {
        eprintln!("pulado: defina NEOVEG_SCREENSHOTS=1 para gerar docs/screenshots");
        return Ok(());
    }
    let dir = output_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("pasta de saída: {e}"))?;
    let wav_dir = std::env::temp_dir().join(format!("neoveg_readme_{}", std::process::id()));
    std::fs::create_dir_all(&wav_dir).map_err(|e| format!("pasta temporária: {e}"))?;

    type Setup = fn(&mut App, &egui::Context);
    let shots: [(&str, Setup); 15] = [
        ("janela_principal.png", |app, ctx| {
            app.exec_cmd(Some(ctx), "view.tab.project_media");
        }),
        ("boas_vindas.png", |app, _| app.open_welcome()),
        ("renderizar_como.png", |app, ctx| {
            app.exec_cmd(Some(ctx), "file.render");
            if let Some(r) = &mut app.dlg.render {
                r.path = "renders/projeto_demo.avi".into();
            }
        }),
        ("efeitos_de_video.png", |app, ctx| {
            if let Some(ev) = first_video_event_id(app, 1) {
                app.session.sel_events.insert(ev);
            }
            app.exec_cmd(Some(ctx), "view.tab.video_fx");
        }),
        ("geradores_de_midia.png", |app, ctx| {
            app.exec_cmd(Some(ctx), "view.tab.generators");
        }),
        ("color_grading.png", |app, ctx| {
            if let Some(ev_id) = first_video_event_id(app, 1) {
                app.session.sel_events.insert(ev_id);
                for tr in &mut app.project.tracks {
                    if let Some(ev) = tr.events.iter_mut().find(|e| e.id == ev_id) {
                        ev.fx.push(crate::model::FxInstance::new(
                            crate::model::FxKind::ColorCorrector,
                        ));
                        break;
                    }
                }
                app.changed();
            }
            app.exec_cmd(Some(ctx), "view.tab.color_grading");
        }),
        ("escopos_de_video.png", |app, ctx| {
            app.exec_cmd(Some(ctx), "view.tab.scopes");
        }),
        ("mixer_e_barramentos.png", |app, ctx| {
            app.exec_cmd(Some(ctx), "view.tab.mixer");
        }),
        ("surround_panner.png", |app, ctx| {
            app.project.settings.channel_layout = crate::model::ChannelLayout::Surround51;
            app.changed();
            app.exec_cmd(Some(ctx), "view.tab.surround_panner");
        }),
        ("medidores_loudness.png", |app, ctx| {
            app.exec_cmd(Some(ctx), "view.tab.loudness");
            app.start_loudness_scan(None);
        }),
        ("propriedades_do_evento.png", |app, _| {
            app.dlg.event_props = first_video_event_id(app, 1);
        }),
        ("neoveg_encoder.png", |app, ctx| {
            let project = app.project.clone();
            let catalog = app.encoder.catalog.clone();
            let state = &mut app.encoder.state;
            state.add_project_snapshot("Projeto Demo 1080p".into(), None, project, None, &catalog);
            let mut wf = neoveg_engine::encoder::watch::WatchFolder::new(
                PathBuf::from("midia/entrada"),
                "sys.web_1080p_hq",
            );
            wf.output_dir = Some(PathBuf::from("midia/saida"));
            state.watch_folders.push(wf);
            app.exec_cmd(Some(ctx), "view.tab.encoder");
        }),
        ("pan_crop.png", |app, _| {
            if let Some(ev) = first_video_event_id(app, 1) {
                app.session.sel_events.insert(ev);
                app.dlg.pan_crop = Some(ev);
            }
        }),
        ("track_motion.png", |app, _| {
            let tid = app
                .project
                .tracks
                .iter()
                .find(|t| t.kind == crate::model::TrackKind::Video)
                .map(|t| t.id);
            app.dlg.track_motion = tid;
        }),
        ("preferencias.png", |app, ctx| {
            app.exec_cmd(Some(ctx), "options.prefs");
            app.dlg.prefs_tab = crate::dialogs::PrefsCategory::Appearance;
        }),
    ];
    let only = std::env::var("NEOVEG_SCREENSHOT_ONLY").ok();
    for (name, setup) in shots {
        if only.as_deref().is_some_and(|o| !name.starts_with(o)) {
            continue;
        }
        let mut harness = demo_harness(&wav_dir, setup)?;
        save(&mut harness, &dir, name)?;
    }
    let _ = std::fs::remove_dir_all(&wav_dir);
    Ok(())
}
