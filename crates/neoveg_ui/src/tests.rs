//! Teste de fumaça da interface: executa quadros do egui sem janela,
//! simulando mouse e teclado, e abre todos os diálogos.

use super::*;
use crate::model::*;
use eframe::egui::Key;
use egui::{pos2, vec2, Event, Modifiers, PointerButton, Pos2};

fn raw(events: Vec<Event>, modifiers: Modifiers, t: f64) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(1500.0, 920.0))),
        events,
        modifiers,
        time: Some(t),
        ..Default::default()
    }
}

fn key(k: Key, mods: Modifiers) -> Vec<Event> {
    vec![
        Event::Key {
            key: k,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: mods,
        },
        Event::Key {
            key: k,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: mods,
        },
    ]
}

fn press(p: Pos2, down: bool) -> Event {
    Event::PointerButton {
        pos: p,
        button: PointerButton::Primary,
        pressed: down,
        modifiers: Modifiers::NONE,
    }
}

fn make_avi() -> PathBuf {
    let path = std::env::temp_dir().join("nvov_ui_smoke.avi");
    let mut w =
        crate::media::avi::AviWriter::create(&path, 160, 90, FrameRate::FPS_25, Some(48000))
            .unwrap();
    for i in 0..50u32 {
        let img = image::RgbImage::from_fn(160, 90, |x, y| {
            image::Rgb([(x * 2) as u8, (y * 3) as u8, (i * 5) as u8])
        });
        let mut jpg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 80)
            .encode_image(&img)
            .unwrap();
        w.write_video_jpeg(&jpg).unwrap();
        let pcm: Vec<i16> = (0..1920 * 2)
            .map(|s| ((s as f32 * 0.05).sin() * 8000.0) as i16)
            .collect();
        w.write_audio(&pcm).unwrap();
    }
    w.finish().unwrap();
    path
}

#[test]
fn ui_smoke() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(&ctx, None);
    let mut t = 0.0;
    let mut frame = |app: &mut App, ev: Vec<Event>, mods: Modifiers| {
        t += 1.0 / 60.0;
        let _ = ctx.run(raw(ev, mods, t), |c| app.frame_ui(c));
    };
    frame(&mut app, vec![], Modifiers::NONE);

    app.import_paths(vec![make_avi()], Some(Ticks::ZERO));
    for _ in 0..200 {
        if app.loading == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        frame(&mut app, vec![], Modifiers::NONE);
    }
    assert_eq!(app.loading, 0, "importação não terminou");
    assert_eq!(app.project.media.len(), 1);
    let n_events: usize = app.project.tracks.iter().map(|t| t.events.len()).sum();
    assert_eq!(n_events, 2, "evento de vídeo + áudio agrupados");

    app.cursor = Ticks::from_secs_i64(3);
    app.insert_generated(GenSpec::Text(TextSpec::default()));
    frame(&mut app, vec![], Modifiers::NONE);

    // percorre todas as abas da área de janelas
    for tab in DockTab::ALL {
        app.dock_tab = tab;
        frame(&mut app, vec![], Modifiers::NONE);
        frame(&mut app, vec![], Modifiers::NONE);
    }

    // mouse: passa sobre a timeline, clica, arrasta, roda
    let mut y = 90.0;
    while y < 500.0 {
        frame(
            &mut app,
            vec![Event::PointerMoved(pos2(400.0, y))],
            Modifiers::NONE,
        );
        y += 15.0;
    }
    for (x, yy) in [
        (300.0, 150.0),
        (420.0, 220.0),
        (600.0, 300.0),
        (330.0, 70.0),
        (500.0, 40.0),
    ] {
        let p = pos2(x, yy);
        frame(
            &mut app,
            vec![Event::PointerMoved(p), press(p, true)],
            Modifiers::NONE,
        );
        for k in 1..8 {
            frame(
                &mut app,
                vec![Event::PointerMoved(p + vec2(k as f32 * 6.0, 0.0))],
                Modifiers::NONE,
            );
        }
        frame(
            &mut app,
            vec![press(p + vec2(42.0, 0.0), false)],
            Modifiers::NONE,
        );
    }
    frame(
        &mut app,
        vec![Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(0.0, 2.0),
            modifiers: Modifiers::NONE,
        }],
        Modifiers::NONE,
    );

    // teclado
    app.sel_events = app
        .project
        .tracks
        .iter()
        .flat_map(|t| t.events.iter().map(|e| e.id))
        .collect();
    app.cursor = Ticks::from_millis_i64(800);
    frame(&mut app, key(Key::S, Modifiers::NONE), Modifiers::NONE);
    let after_split: usize = app.project.tracks.iter().map(|t| t.events.len()).sum();
    assert!(
        after_split > 3,
        "divisão deveria criar eventos ({after_split})"
    );
    frame(
        &mut app,
        key(Key::Z, Modifiers::COMMAND),
        Modifiers::COMMAND,
    );
    frame(
        &mut app,
        key(Key::Y, Modifiers::COMMAND),
        Modifiers::COMMAND,
    );
    frame(&mut app, key(Key::M, Modifiers::NONE), Modifiers::NONE);
    frame(&mut app, key(Key::V, Modifiers::NONE), Modifiers::NONE);
    frame(&mut app, vec![Event::Copy], Modifiers::COMMAND);
    app.cursor = Ticks::from_secs_i64(10);
    frame(
        &mut app,
        vec![Event::Paste(String::new())],
        Modifiers::COMMAND,
    );
    frame(&mut app, key(Key::Delete, Modifiers::NONE), Modifiers::NONE);
    frame(&mut app, key(Key::Space, Modifiers::NONE), Modifiers::NONE);
    for _ in 0..10 {
        frame(&mut app, vec![], Modifiers::NONE);
    }
    frame(&mut app, key(Key::Space, Modifiers::NONE), Modifiers::NONE);

    // abre todos os diálogos
    let eid = app
        .project
        .tracks
        .iter()
        .find(|t| t.kind == TrackKind::Video)
        .and_then(|t| t.events.first())
        .map(|e| e.id)
        .unwrap();
    let tid = app.project.tracks[0].id;
    let gid = app
        .project
        .media
        .iter()
        .find(|m| matches!(m.source, MediaSource::Generated(_)))
        .map(|m| m.id)
        .unwrap();
    app.project.tracks[0]
        .fx
        .push(FxInstance::new(FxKind::GaussianBlur));
    app.dlg.project_props = true;
    app.dlg.event_props = Some(eid);
    app.dlg.pan_crop = Some(eid);
    app.dlg.track_motion = Some(tid);
    app.dlg.fx_target = Some(FxTarget::Track(tid));
    app.dlg.gen_editor = Some(gid);
    app.dlg.shortcuts = true;
    app.dlg.about = true;
    app.dlg.goto_open = true;
    app.dlg.open_render(&app.session.project, None);
    for _ in 0..5 {
        frame(&mut app, vec![], Modifiers::NONE);
    }
    app.dlg.fx_target = Some(FxTarget::MasterAudio);
    frame(&mut app, vec![], Modifiers::NONE);

    // salvar / abrir projeto (.neoveg e retrocompatibilidade .nvp)
    let pp = std::env::temp_dir().join("neoveg_ui_smoke.neoveg");
    assert!(app.write_project(&pp));
    app.open_project_path(pp.clone());
    for _ in 0..100 {
        if app.loading == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        frame(&mut app, vec![], Modifiers::NONE);
    }
    assert!(!app.project.media.is_empty());
    let _ = std::fs::remove_file(pp);

    // despacho via Tabela Única de Comandos também funciona com histórico
    let n_tracks_before = app.project.tracks.len();
    app.exec_cmd(None, "insert.track_video");
    assert_eq!(app.project.tracks.len(), n_tracks_before + 1);
    app.exec_cmd(None, "edit.undo");
    assert_eq!(app.project.tracks.len(), n_tracks_before);

    // 7 temas (incluindo Alto Contraste AAA), preferências, layouts e aba destacada
    for tid in crate::theme::ThemeId::ALL {
        app.set_theme(&ctx, tid, false);
        frame(&mut app, vec![], Modifiers::NONE);
    }
    app.set_theme(&ctx, crate::theme::ThemeId::VegasEscuro, false);

    app.exec_cmd(Some(&ctx), "options.prefs");
    assert!(app.dlg.prefs_open);
    frame(&mut app, vec![], Modifiers::NONE);
    app.dlg.prefs_open = false;

    // Editar > Propriedades > Layout: diálogo + 3 tipos (Sonic Foundry, Sony, MAGIX)
    struct PrefsGuard {
        path: std::path::PathBuf,
        prev: Option<Vec<u8>>,
    }
    impl Drop for PrefsGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(bytes) => {
                    let _ = std::fs::write(&self.path, bytes);
                }
                None => {
                    let _ = std::fs::remove_file(&self.path);
                }
            }
        }
    }
    let prefs_path = crate::Preferences::file_path();
    let _prefs_guard = PrefsGuard {
        prev: std::fs::read(&prefs_path).ok(),
        path: prefs_path,
    };

    app.exec_cmd(Some(&ctx), "edit.properties.layout");
    assert!(app.dlg.edit_layout_open);
    frame(&mut app, vec![], Modifiers::NONE);
    app.dlg.edit_layout_open = false;

    app.exec_cmd(Some(&ctx), "edit.properties.layout.sonic_foundry");
    assert_eq!(
        app.prefs.edit_layout.layout_type,
        crate::LayoutType::SonicFoundry
    );
    assert!(!app.prefs.edit_layout.timeline_at_bottom);
    assert!(!app.prefs.edit_layout.tabs_at_top);
    assert!(!app.prefs.edit_layout.transport_at_bottom);
    assert_eq!(app.theme_id, crate::theme::ThemeId::VegasClaro);
    frame(&mut app, vec![], Modifiers::NONE);

    app.exec_cmd(Some(&ctx), "edit.properties.layout.sony");
    assert_eq!(app.prefs.edit_layout.layout_type, crate::LayoutType::Sony);
    assert!(app.prefs.edit_layout.timeline_at_bottom);
    assert!(!app.prefs.edit_layout.tabs_at_top);
    assert!(app.prefs.edit_layout.transport_at_bottom);
    assert_eq!(app.theme_id, crate::theme::ThemeId::VegasMedio);
    frame(&mut app, vec![], Modifiers::NONE);

    // Alto Contraste AAA deve ser preservado ao alternar o tipo de layout
    app.set_theme(&ctx, crate::theme::ThemeId::AltoContrasteEscuro, false);
    app.exec_cmd(Some(&ctx), "edit.properties.layout.sonic_foundry");
    assert_eq!(app.theme_id, crate::theme::ThemeId::AltoContrasteEscuro);
    app.set_theme(&ctx, crate::theme::ThemeId::VegasEscuro, false);

    app.exec_cmd(Some(&ctx), "edit.properties.layout.magix");
    assert_eq!(app.prefs.edit_layout.layout_type, crate::LayoutType::Magix);
    assert!(!app.prefs.edit_layout.timeline_at_bottom);
    assert!(app.prefs.edit_layout.tabs_at_top);
    assert!(!app.prefs.edit_layout.transport_at_bottom);
    assert_eq!(app.theme_id, crate::theme::ThemeId::VegasEscuro);
    frame(&mut app, vec![], Modifiers::NONE);

    for preset in crate::layout::builtin_presets() {
        app.apply_window_layout(preset);
        frame(&mut app, vec![], Modifiers::NONE);
    }
    app.detached_tabs.insert(DockTab::Mixer);
    frame(&mut app, vec![], Modifiers::NONE);
    app.detached_tabs.remove(&DockTab::Mixer);

    // Playback rate contínuo durante reprodução + geração de proxy
    app.playback_rate = 1.5;
    app.play();
    frame(&mut app, vec![], Modifiers::NONE);
    assert!(app.is_playing());
    app.stop(false);
    app.playback_rate = 1.0;

    let avi_mid = app
        .project
        .media
        .iter()
        .find(|m| m.has_video && !matches!(m.source, MediaSource::Generated(_)))
        .map(|m| m.id)
        .unwrap();
    app.build_proxy_for_media(avi_mid);
    for _ in 0..150 {
        if app.loading == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        frame(&mut app, vec![], Modifiers::NONE);
    }
    let proxy_file = app
        .project
        .media(avi_mid)
        .and_then(|m| m.proxy_path.clone());
    assert!(proxy_file.is_some(), "proxy deveria ter sido criado");
    if let Some(pf) = proxy_file {
        let _ = std::fs::remove_file(pf);
    }
}
