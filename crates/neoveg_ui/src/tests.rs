//! Teste de fumaça da interface: executa quadros do egui sem janela,
//! simulando mouse e teclado, e abre todos os diálogos.

use super::*;
use crate::model::*;
use eframe::egui::Key;
use egui::{pos2, vec2, Event, Modifiers, PointerButton, Pos2};

pub(super) static PREFS_LOCK: std::sync::RwLock<()> = std::sync::RwLock::new(());

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
    let path = std::env::temp_dir().join(format!("nvov_ui_smoke_{}.avi", std::process::id()));
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
    struct PrefsGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
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
    let lock = crate::kittest_tests::lock_test_prefs();
    let _prefs_lock = PREFS_LOCK
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let prefs_path = crate::Preferences::file_path();
    let _prefs_guard = PrefsGuard {
        _lock: lock,
        prev: std::fs::read(&prefs_path).ok(),
        path: prefs_path,
    };

    let ctx = egui::Context::default();
    let mut app = App::with_context(&ctx, None);
    // o teste de fumaça percorre todas as abas do dock
    app.closed_tabs.clear();
    let mut t = 0.0;
    let mut frame = |app: &mut App, ev: Vec<Event>, mods: Modifiers| {
        t += 1.0 / 60.0;
        let _ = ctx.run(raw(ev, mods, t), |c| app.frame_ui(c));
    };
    let no_recovery = neoveg_engine::recovery::find_existing_recovery().is_none();
    assert_eq!(app.is_splash_open(), app.prefs.show_splash && no_recovery);
    frame(&mut app, vec![], Modifiers::NONE);
    app.dismiss_splash();
    assert!(!app.is_splash_open());
    assert_eq!(app.is_welcome_open(), app.prefs.show_welcome && no_recovery);
    frame(&mut app, vec![], Modifiers::NONE);
    app.dismiss_welcome();
    assert!(!app.is_welcome_open());

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
    let pp = std::env::temp_dir().join(format!("neoveg_ui_smoke_{}.neoveg", std::process::id()));
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

    app.exec_cmd(Some(&ctx), "help.splash");
    assert!(app.is_splash_open());
    frame(&mut app, vec![], Modifiers::NONE);
    frame(&mut app, key(Key::Escape, Modifiers::NONE), Modifiers::NONE);
    assert!(!app.is_splash_open());

    app.exec_cmd(Some(&ctx), "help.welcome");
    assert!(app.is_welcome_open());
    frame(&mut app, vec![], Modifiers::NONE);
    frame(&mut app, key(Key::Escape, Modifiers::NONE), Modifiers::NONE);
    assert!(!app.is_welcome_open());

    // Renderizar como em 2 colunas + aba Color Grading
    app.exec_cmd(Some(&ctx), "view.tab.color_grading");
    assert_eq!(app.dock_tab, DockTab::ColorGrading);
    frame(&mut app, vec![], Modifiers::NONE);
    app.exec_cmd(Some(&ctx), "file.render");
    assert!(app.dlg.render.is_some());
    frame(&mut app, vec![], Modifiers::NONE);
    app.dlg.render = None;

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
    assert!(!app.prefs.edit_layout.master_bus_docked);
    assert_eq!(app.theme_id, crate::theme::ThemeId::VegasClaro);
    frame(&mut app, vec![], Modifiers::NONE);

    app.exec_cmd(Some(&ctx), "edit.properties.layout.sony");
    assert_eq!(app.prefs.edit_layout.layout_type, crate::LayoutType::Sony);
    assert!(app.prefs.edit_layout.timeline_at_bottom);
    assert!(!app.prefs.edit_layout.tabs_at_top);
    assert!(app.prefs.edit_layout.transport_at_bottom);
    assert!(!app.prefs.edit_layout.master_bus_docked);
    assert_eq!(app.theme_id, crate::theme::ThemeId::VegasMedio);
    frame(&mut app, vec![], Modifiers::NONE);

    // Alto Contraste AAA deve ser preservado ao alternar o tipo de layout
    app.set_theme(&ctx, crate::theme::ThemeId::AltoContrasteEscuro, false);
    app.exec_cmd(Some(&ctx), "edit.properties.layout.sonic_foundry");
    assert_eq!(app.theme_id, crate::theme::ThemeId::AltoContrasteEscuro);
    app.set_theme(&ctx, crate::theme::ThemeId::VegasEscuro, false);

    app.exec_cmd(Some(&ctx), "edit.properties.layout.magix");
    assert_eq!(app.prefs.edit_layout.layout_type, crate::LayoutType::Magix);
    assert!(app.prefs.edit_layout.timeline_at_bottom);
    assert!(!app.prefs.edit_layout.tabs_at_top);
    assert!(app.prefs.edit_layout.transport_at_bottom);
    assert!(app.prefs.edit_layout.master_bus_docked);
    assert_eq!(app.theme_id, crate::theme::ThemeId::VegasEscuro);
    frame(&mut app, vec![], Modifiers::NONE);

    for preset in crate::layout::builtin_presets() {
        app.apply_window_layout(preset);
        frame(&mut app, vec![], Modifiers::NONE);
    }
    app.preview_detached = true;
    app.detached_tabs.insert(DockTab::Mixer);
    app.detached_tabs.insert(DockTab::VideoFx);
    frame(&mut app, vec![], Modifiers::NONE);
    app.detached_tabs.clear();
    app.preview_detached = false;

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
    let avi_idx = app
        .project
        .media
        .iter()
        .position(|m| m.id == avi_mid)
        .unwrap();
    assert_narrow_layout_fixes(&mut app, avi_idx);
    assert_vegas21_ui_upgrades(&mut app);
    assert_welcome_ui_layout(&mut app);
    if let Some(pf) = proxy_file {
        let _ = std::fs::remove_file(pf);
    }
}

fn collect_galley_shapes(
    shape: &egui::epaint::Shape,
    clip: egui::Rect,
    out: &mut Vec<(String, usize, egui::Rect, egui::Rect)>,
) {
    match shape {
        egui::epaint::Shape::Text(ts) => {
            let rect = egui::Rect::from_min_size(ts.pos, ts.galley.size());
            out.push((
                ts.galley.text().to_owned(),
                ts.galley.rows.len(),
                rect,
                clip,
            ));
        }
        egui::epaint::Shape::Vec(items) => {
            for item in items {
                collect_galley_shapes(item, clip, out);
            }
        }
        _ => {}
    }
}

/// Garante que o layout clássico estreito não quebre nomes de mídia na vertical,
/// não corte a barra de ferramentas nem esconda as abas inferiores.
fn assert_narrow_layout_fixes(app: &mut App, media_idx: usize) {
    let prev_layout = app.prefs.edit_layout.layout_type;
    let prev_master_docked = app.prefs.edit_layout.master_bus_docked;
    let prev_tab = app.dock_tab;
    let prev_dock_vis = app.dock_visible;
    let prev_loading = app.loading;
    let prev_name = app.project.media.get(media_idx).map(|m| m.name.clone());
    if let Some(item) = app.project.media.get_mut(media_idx) {
        item.name = "BigBuckBunny_320x180.mp4".to_owned();
    }
    app.prefs.edit_layout.layout_type = LayoutType::Sony;
    app.prefs.edit_layout.master_bus_docked = false;
    app.dock_tab = DockTab::ProjectMedia;
    app.dock_visible = true;
    app.detached_tabs.clear();
    app.loading = 1;

    let ctx = egui::Context::default();
    let narrow_raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(840.0, 800.0))),
        time: Some(2.5),
        ..Default::default()
    };
    let _ = ctx.run(narrow_raw.clone(), |c| app.frame_ui(c));
    let out = ctx.run(narrow_raw, |c| app.frame_ui(c));

    let mut texts = Vec::new();
    for clipped in &out.shapes {
        collect_galley_shapes(&clipped.shape, clipped.clip_rect, &mut texts);
    }
    let sample_texts = || {
        texts
            .iter()
            .skip(45)
            .take(80)
            .map(|(s, r, rc, _)| format!("{s:?}@{r}:{rc:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let media_text = texts
        .iter()
        .find(|(txt, _, _, _)| txt == "BigBuckBunny_320x180.mp4");
    assert!(
        media_text.is_some(),
        "nome da mídia deve ser desenhado na aba Mídia do Projeto; textos={}",
        sample_texts()
    );
    if let Some((_, rows, rect, _)) = media_text {
        assert_eq!(
            *rows, 1,
            "nome de mídia sem espaços não deve quebrar letra por letra na vertical ({rows} linhas)"
        );
        assert!(
            rect.width() > 60.0,
            "nome de mídia deve ocupar largura horizontal legível ({})",
            rect.width()
        );
    }

    let import_btn = texts.iter().find(|(txt, _, _, _)| txt == "Importar...");
    assert!(
        import_btn.is_some(),
        "botão 'Importar...' deve aparecer na Mídia do Projeto; textos={}",
        sample_texts()
    );
    if let Some((_, rows, rect, clip)) = import_btn {
        assert_eq!(*rows, 1);
        assert!(
            rect.right() <= clip.right() + 1.5 && rect.bottom() <= clip.bottom() + 1.5,
            "'Importar...' não pode ser cortado em janela estreita: rect={rect:?}, clip={clip:?}"
        );
    }
    for (txt, _, rect, clip) in &texts {
        if rect.top() < 60.0 {
            assert!(
                rect.right() <= clip.right() + 1.5 && rect.right() <= 840.0 + 1.5,
                "texto do topo '{txt}' não pode vazar à direita em 840x800: rect={rect:?}, clip={clip:?}"
            );
        }
    }

    let mut row_tops = Vec::new();
    for tab_label in ["Mídia do Projeto", "Explorer", "Destacar"] {
        let tab = texts
            .iter()
            .find(|(txt, _, _, _)| txt == tab_label)
            .unwrap_or_else(|| {
                panic!(
                    "aba/botão '{tab_label}' deve estar visível no rodapé; textos={}",
                    sample_texts()
                )
            });
        let (_, _, rect, clip) = tab;
        assert!(
            rect.top() >= clip.top() - 1.5
                && rect.bottom() <= clip.bottom() + 1.5
                && rect.bottom() <= 800.0,
            "aba/botão '{tab_label}' não pode ficar clipado fora do rodapé: rect={rect:?}, clip={clip:?}"
        );
        row_tops.push(rect.top());
    }
    if let (Some(&first), Some(&last)) = (row_tops.first(), row_tops.last()) {
        assert!(
            (first - last).abs() <= 2.5,
            "abas inferiores devem ficar em linha única com rolagem horizontal em vez de quebrar em várias linhas: row_tops={row_tops:?}"
        );
    }

    if let Some(name) = prev_name {
        if let Some(item) = app.project.media.get_mut(media_idx) {
            item.name = name;
        }
    }
    app.prefs.edit_layout.layout_type = prev_layout;
    app.prefs.edit_layout.master_bus_docked = prev_master_docked;
    app.dock_tab = prev_tab;
    app.dock_visible = prev_dock_vis;
    app.loading = prev_loading;
}

/// Garante Visor de Tempo dedicado, Barra de Status de Renderização, janela responsiva
/// de Layout em monitor pequeno, janelas destacáveis e legibilidade em temas claros.
fn assert_vegas21_ui_upgrades(app: &mut App) {
    let prev_theme = app.theme_id;
    let prev_layout_snap = app.current_window_layout("test_prev");
    let prev_edit_layout = app.prefs.edit_layout.clone();
    let prev_prerendered = app.session.prerendered_ranges.clone();

    let ctx = egui::Context::default();
    app.apply_window_layout(crate::layout::WindowLayout::default());
    app.exec_cmd(Some(&ctx), "edit.properties.layout.magix");
    let small_raw = |t: f64| egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(760.0, 520.0))),
        time: Some(t),
        ..Default::default()
    };

    // 1. Timeline exibe Visor de Tempo dedicado e Barra de Status de Renderização
    app.session.clear_prerendered();
    let _ = ctx.run(raw(vec![], Modifiers::NONE, 3.0), |c| app.frame_ui(c));
    let out = ctx.run(raw(vec![], Modifiers::NONE, 3.05), |c| app.frame_ui(c));
    let mut texts = Vec::new();
    for clipped in &out.shapes {
        collect_galley_shapes(&clipped.shape, clipped.clip_rect, &mut texts);
    }
    let visor_rect = texts
        .iter()
        .find(|(t, _, _, _)| t == "Visor de tempo")
        .map(|(_, _, r, _)| *r)
        .expect("Visor de tempo deve ser exibido no canto superior da lista de faixas");
    let status_rect = texts
        .iter()
        .find(|(t, _, _, _)| t == "Status de renderização: Pronto (sem trechos pré-renderizados)")
        .map(|(_, _, r, _)| *r)
        .expect("Barra de Status de Renderização deve aparecer na base da timeline");
    let track1_rect = texts
        .iter()
        .find(|(t, _, r, _)| t == "1" && r.left() < 100.0 && r.top() > visor_rect.top())
        .map(|(_, _, r, _)| *r)
        .expect("cabeçalho da trilha 1 deve ser exibido abaixo do Visor de tempo");
    assert!(
        visor_rect.left() < 250.0 && visor_rect.bottom() < track1_rect.top(),
        "Visor de tempo ({visor_rect:?}) deve ficar acima dos cabeçalhos das faixas ({track1_rect:?})"
    );
    assert!(
        visor_rect.bottom() < status_rect.top(),
        "Visor de tempo ({visor_rect:?}) deve ficar acima do rodapé ({status_rect:?})"
    );
    assert!(
        texts
            .iter()
            .any(|(t, _, r, _)| t == "Taxa:" && r.left() < 250.0 && r.top() > track1_rect.bottom()),
        "controle Taxa (Playback Rate) deve aparecer abaixo da lista de faixas"
    );

    app.session
        .record_prerendered_range(Ticks::ZERO, Ticks::from_secs_i64(2));
    let out = ctx.run(raw(vec![], Modifiers::NONE, 3.1), |c| app.frame_ui(c));
    texts.clear();
    for clipped in &out.shapes {
        collect_galley_shapes(&clipped.shape, clipped.clip_rect, &mut texts);
    }
    assert!(
        texts
            .iter()
            .any(|(t, _, _, _)| t == "Status de renderização: 1 trecho(s) pré-renderizado(s)"),
        "Barra de Status deve indicar trechos pré-renderizados registrados"
    );
    assert!(
        texts.iter().any(|(t, _, _, _)| t == "Prévia de Vídeo")
            && texts.iter().any(|(t, _, _, _)| t == "Master")
            && texts.iter().any(|(t, _, _, _)| t.starts_with("Projeto:")),
        "Área 2 (VEGAS 21) deve exibir Prévia de Vídeo, Master Bus acoplado à direita e resumo do projeto"
    );
    for needle in [
        "Projeto:",
        "Prévia:",
        "Quadro:",
        "Exibição:",
        "Prévia de Vídeo",
        "Master",
    ] {
        let found = texts
            .iter()
            .find(|(t, _, _, _)| t == needle || t.starts_with(needle));
        let (txt, _, rect, clip) =
            found.unwrap_or_else(|| panic!("texto `{needle}` deve estar visível na Área 2"));
        assert!(
            rect.top() >= clip.top() - 0.5 && rect.bottom() <= clip.bottom() + 0.5,
            "`{txt}` não deve ser cortado verticalmente: text={rect:?}, clip={clip:?}"
        );
    }
    assert!(
        texts.iter().any(|(t, _, _, _)| t == "Tam:"),
        "Área 6 (VEGAS 21) deve exibir o bloco In/Fim/Tam na barra de transporte em largura desktop"
    );
    // em cada caixa o nome vem antes do valor (antes: "00:00:00;00 Tam:"):
    // na linha das caixas, da esquerda para a direita, cada nome é seguido
    // por um timecode
    let (_, _, tam_r, _) = texts
        .iter()
        .find(|(t, _, _, _)| t == "Tam:")
        .expect("`Tam:` visível");
    let is_tc = |t: &str| t.chars().filter(|c| c.is_ascii_digit()).count() >= 6;
    let mut row: Vec<(f32, &str)> = texts
        .iter()
        .filter(|(t, _, r, _)| {
            (r.center().y - tam_r.center().y).abs() < 4.0
                && (["Pos:", "In:", "Fim:", "Tam:"].contains(&t.as_str()) || is_tc(t))
        })
        .map(|(t, _, r, _)| (r.left(), t.as_str()))
        .collect();
    row.sort_by(|a, b| a.0.total_cmp(&b.0));
    let seq: Vec<&str> = row.iter().map(|(_, t)| *t).collect();
    for (k, t) in seq.iter().enumerate() {
        if !is_tc(t) {
            assert!(
                seq.get(k + 1).is_some_and(|v| is_tc(v)),
                "`{t}` deve vir logo antes do valor: {seq:?}"
            );
        }
    }
    app.exec_cmd(Some(&ctx), "view.master_bus");
    assert!(!app.master_bus_visible);
    app.exec_cmd(Some(&ctx), "view.master_bus");
    assert!(app.master_bus_visible);

    // 2. Janela "Editar > Propriedades > Layout" deve caber integralmente em 760x520
    app.exec_cmd(Some(&ctx), "edit.properties.layout");
    let _ = ctx.run(small_raw(3.15), |c| app.frame_ui(c));
    let out = ctx.run(small_raw(3.20), |c| app.frame_ui(c));
    texts.clear();
    for clipped in &out.shapes {
        collect_galley_shapes(&clipped.shape, clipped.clip_rect, &mut texts);
    }
    for btn_label in ["OK", "Restaurar padrão do tipo"] {
        let found = texts.iter().find(|(t, _, _, _)| t == btn_label).unwrap_or_else(|| {
            let all = texts
                .iter()
                .map(|(s, r, rc, _)| format!("{s:?}@{r}:{rc:?}"))
                .collect::<Vec<_>>()
                .join(" | ");
            panic!(
                "botão '{btn_label}' deve aparecer no diálogo de Layout (edit_layout_open={}, shapes={}, all=[{all}])",
                app.dlg.edit_layout_open,
                out.shapes.len()
            );
        });
        let (_, _, rect, clip) = found;
        assert!(
            rect.right() <= 760.0 + 1.5
                && rect.bottom() <= 520.0 + 1.5
                && rect.right() <= clip.right() + 1.5
                && rect.bottom() <= clip.bottom() + 1.5,
            "botão '{btn_label}' saiu dos limites em monitor 760x520: rect={rect:?}, clip={clip:?}"
        );
    }
    app.dlg.edit_layout_open = false;

    // 3. Temas claros + Prévia destacada, Master Bus destacado e janelas do dock destacadas sem pânico
    app.preview_detached = true;
    app.master_bus_detached = true;
    app.detached_tabs.insert(DockTab::Mixer);
    for tid in [
        crate::theme::ThemeId::VegasClaro,
        crate::theme::ThemeId::AltoContrasteClaro,
    ] {
        app.set_theme(&ctx, tid, false);
        let _ = ctx.run(raw(vec![], Modifiers::NONE, 3.3), |c| app.frame_ui(c));
        let _ = ctx.run(raw(vec![], Modifiers::NONE, 3.35), |c| app.frame_ui(c));
    }

    // 4. Janela Preferências em 2 painéis (VEGAS) adaptável em monitor pequeno, compacto e temas claro/escuro
    app.detached_tabs.clear();
    app.preview_detached = false;
    app.master_bus_detached = false;
    let prev_prefs_tab = app.dlg.prefs_tab;
    app.exec_cmd(Some(&ctx), "options.prefs");
    assert!(app.dlg.prefs_open);
    for (idx, cat) in crate::dialogs::PrefsCategory::ALL.iter().enumerate() {
        app.dlg.prefs_tab = *cat;
        let t0 = 4.0 + idx as f64 * 0.04;
        let _ = ctx.run(small_raw(t0), |c| app.frame_ui(c));
        let out = ctx.run(small_raw(t0 + 0.02), |c| app.frame_ui(c));
        let mut texts = Vec::new();
        for clipped in &out.shapes {
            collect_galley_shapes(&clipped.shape, clipped.clip_rect, &mut texts);
        }
        assert!(
            texts.iter().any(|(t, _, _, _)| t == cat.label()),
            "categoria '{}' deve aparecer no painel esquerdo de Preferências",
            cat.label()
        );
        for btn_label in ["Restaurar padrões", "OK", "Aplicar", "Cancelar"] {
            assert!(
                texts.iter().any(|(t, _, rect, clip)| {
                    t == btn_label
                        && rect.bottom() <= clip.bottom() + 2.0
                        && rect.bottom() <= 520.0
                        && rect.right() <= 760.0
                }),
                "botão '{btn_label}' deve estar visível no rodapé de Preferências (categoria '{}') em 760x520",
                cat.label()
            );
        }
    }
    let compact_raw = |t: f64| egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(420.0, 420.0))),
        time: Some(t),
        ..Default::default()
    };
    app.dlg.prefs_tab = crate::dialogs::PrefsCategory::General;
    let _ = ctx.run(compact_raw(5.0), |c| app.frame_ui(c));
    let compact_out = ctx.run(compact_raw(5.05), |c| app.frame_ui(c));
    let mut compact_texts = Vec::new();
    for clipped in &compact_out.shapes {
        collect_galley_shapes(&clipped.shape, clipped.clip_rect, &mut compact_texts);
    }
    assert!(
        compact_texts.iter().any(|(t, _, rect, clip)| {
            t == "OK" && rect.bottom() <= clip.bottom() + 2.0 && rect.bottom() <= 420.0
        }),
        "botão 'OK' deve permanecer visível mesmo no modo compacto 420x420"
    );
    for (i, tid) in [
        crate::theme::ThemeId::VegasClaro,
        crate::theme::ThemeId::VegasEscuro,
        crate::theme::ThemeId::AltoContrasteClaro,
    ]
    .iter()
    .enumerate()
    {
        app.set_theme(&ctx, *tid, false);
        let t0 = 5.2 + i as f64 * 0.1;
        let _ = ctx.run(raw(vec![], Modifiers::NONE, t0), |c| app.frame_ui(c));
        let _ = ctx.run(raw(vec![], Modifiers::NONE, t0 + 0.05), |c| app.frame_ui(c));
    }
    app.dlg.prefs_tab = prev_prefs_tab;
    app.dlg.prefs_open = false;

    app.session.prerendered_ranges = prev_prerendered;
    app.apply_window_layout(prev_layout_snap);
    app.apply_edit_layout(Some(&ctx), prev_edit_layout, false, false);
    app.set_theme(&ctx, prev_theme, false);
}

/// Garante o layout da Welcome Screen estilo VEGAS (barra lateral, 5 proporções,
/// formulário de novo projeto e rodapé sem recorte em desktop e monitor 760x520).
fn assert_welcome_ui_layout(app: &mut App) {
    let prev_open = app.is_welcome_open();
    let prev_welcome = app.welcome.clone();
    let ctx = egui::Context::default();
    app.open_welcome();
    app.welcome.section = crate::welcome::WelcomeSection::NewProject;

    let _ = ctx.run(raw(vec![], Modifiers::NONE, 1.0), |c| app.frame_ui(c));
    let out = ctx.run(raw(vec![], Modifiers::NONE, 1.05), |c| app.frame_ui(c));
    let mut texts = Vec::new();
    for clipped in &out.shapes {
        collect_galley_shapes(&clipped.shape, clipped.clip_rect, &mut texts);
    }
    for expected in [
        "Neoveg",
        "Novo Projeto",
        "Abrir Projeto",
        "Primeiros Passos",
        "Widescreen (16:9)",
        "Fullscreen (4:3)",
        "Portrait (9:16)",
        "Square (1:1)",
        "Scope (2.4:1)",
        "Criar um novo projeto",
        "Resolução:",
        "Taxa de quadros:",
        "Criar Projeto",
        "Configurações Avançadas...",
        "Mostrar tela de boas-vindas ao iniciar",
    ] {
        assert!(
            texts.iter().any(|(txt, _, rc, clip)| {
                txt == expected
                    && rc.right() <= clip.right() + 1.5
                    && rc.bottom() <= clip.bottom() + 1.5
            }),
            "Welcome Screen deve exibir '{expected}' sem recorte em 1500x920"
        );
    }

    let small_raw = |t: f64| egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(760.0, 520.0))),
        time: Some(t),
        ..Default::default()
    };
    let _ = ctx.run(small_raw(2.0), |c| app.frame_ui(c));
    let small_out = ctx.run(small_raw(2.05), |c| app.frame_ui(c));
    texts.clear();
    for clipped in &small_out.shapes {
        collect_galley_shapes(&clipped.shape, clipped.clip_rect, &mut texts);
    }
    for expected in [
        "Criar Projeto",
        "Configurações Avançadas...",
        "Mostrar tela de boas-vindas ao iniciar",
    ] {
        assert!(
            texts.iter().any(|(txt, _, rc, clip)| {
                txt == expected
                    && rc.right() <= clip.right() + 1.5
                    && rc.bottom() <= clip.bottom() + 1.5
                    && rc.bottom() <= 520.0
                    && rc.right() <= 760.0
            }),
            "Welcome Screen deve manter '{expected}' visível sem corte em monitor 760x520"
        );
    }

    app.welcome = prev_welcome;
    if !prev_open {
        app.dismiss_welcome();
    }
}

/// Ferramentas de edição na linha do tempo (manual do VEGAS Pro 21, p. 236):
/// Envelope não move eventos, Seleção seleciona pela caixa sem mover, Zoom
/// aproxima pela caixa e ajusta o projeto com um clique, e o modo Slip da
/// Normal desliza a mídia sem mover o evento.
#[test]
fn ferramentas_de_edicao_na_timeline() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(&ctx, None);
    let mut t = 0.0;
    let mut frame = |app: &mut App, ev: Vec<Event>| {
        t += 1.0 / 60.0;
        let _ = ctx.run(raw(ev, Modifiers::NONE, t), |c| app.frame_ui(c));
    };
    frame(&mut app, vec![]);
    app.dismiss_splash();
    frame(&mut app, vec![]);
    app.dismiss_welcome();
    app.session.sel_track = Some(0);
    app.cursor = Ticks::ZERO;
    app.insert_generated(GenSpec::ColorBars);
    app.cursor = Ticks::from_secs_i64(6);
    app.session.sel_track = Some(0);
    app.insert_generated(GenSpec::ColorBars);
    app.dlg.gen_editor = None;
    app.sel_events.clear();
    for _ in 0..4 {
        frame(&mut app, vec![]);
    }
    let ids: Vec<Id> = app.project.tracks[0].events.iter().map(|e| e.id).collect();
    assert_eq!(ids.len(), 2, "dois eventos na trilha de vídeo");
    let (a, b) = (ids[0], ids[1]);
    let ra = app.event_screen_rect(a).expect("evento a na tela");
    let rb = app.event_screen_rect(b).expect("evento b na tela");
    let start = |app: &App, id| app.project.event(id).map(|e| e.start);
    let drag = |app: &mut App, frame: &mut dyn FnMut(&mut App, Vec<Event>), p0: Pos2, p1: Pos2| {
        frame(app, vec![Event::PointerMoved(p0), press(p0, true)]);
        for k in 1..=8 {
            let p = p0 + (p1 - p0) * (k as f32 / 8.0);
            frame(app, vec![Event::PointerMoved(p)]);
        }
        frame(app, vec![press(p1, false)]);
        frame(app, vec![]);
    };

    // Envelope: arrastar o corpo do evento não o move nem cria Desfazer
    app.exec_cmd(Some(&ctx), "edit.tool.envelope");
    let undo0 = app.undo.len();
    let sa = start(&app, a);
    drag(
        &mut app,
        &mut frame,
        ra.center(),
        ra.center() + vec2(120.0, 0.0),
    );
    assert_eq!(start(&app, a), sa, "Envelope não move eventos");
    assert_eq!(app.undo.len(), undo0, "nem cria passo de Desfazer");

    // Seleção: a caixa seleciona os dois eventos e não move nenhum
    app.exec_cmd(Some(&ctx), "edit.tool.selection");
    app.sel_events.clear();
    let sb = start(&app, b);
    drag(&mut app, &mut frame, ra.center(), rb.center());
    assert!(app.sel_events.contains(&a) && app.sel_events.contains(&b));
    assert_eq!(
        (start(&app, a), start(&app, b)),
        (sa, sb),
        "Seleção não move"
    );

    // Zoom: a caixa aproxima; um clique mostra o projeto inteiro
    app.exec_cmd(Some(&ctx), "edit.tool.zoom");
    let pps0 = app.tl.px_per_sec;
    let y = ra.center().y;
    drag(
        &mut app,
        &mut frame,
        pos2(ra.left() + 10.0, y),
        pos2(ra.left() + 70.0, y),
    );
    assert!(app.tl.px_per_sec > pps0, "a caixa aproxima o zoom");
    let p = pos2(ra.left() + 10.0, y);
    frame(&mut app, vec![Event::PointerMoved(p), press(p, true)]);
    frame(&mut app, vec![press(p, false)]);
    frame(&mut app, vec![]);
    assert_eq!(app.tl.scroll_x, Ticks::ZERO, "o clique volta ao início");
    assert!(
        app.tl.px_per_sec < pps0 * 2.0,
        "e afasta para o projeto inteiro"
    );

    // Normal no modo Slip: o arraste desliza a mídia e o evento fica
    app.exec_cmd(Some(&ctx), "edit.tool.mode.slip");
    for _ in 0..2 {
        frame(&mut app, vec![]);
    }
    let rb = app.event_screen_rect(b).expect("evento b na tela");
    let off0 = app.project.event(b).map(|e| e.offset);
    drag(
        &mut app,
        &mut frame,
        rb.center(),
        rb.center() - vec2(40.0, 0.0),
    );
    assert_eq!(start(&app, b), sb, "Slip não move o evento");
    assert_ne!(
        app.project.event(b).map(|e| e.offset),
        off0,
        "a mídia desliza"
    );
}

#[test]
fn modos_shuffle_slide_stretch_split_trim_na_timeline() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(&ctx, None);
    let mut t = 0.0;
    let mut frame = |app: &mut App, ev: Vec<Event>| {
        t += 1.0 / 60.0;
        let _ = ctx.run(raw(ev, Modifiers::NONE, t), |c| app.frame_ui(c));
    };
    frame(&mut app, vec![]);
    app.dismiss_splash();
    frame(&mut app, vec![]);
    app.dismiss_welcome();

    app.session.sel_track = Some(0);
    app.cursor = Ticks::ZERO;
    app.insert_generated(GenSpec::ColorBars);
    app.cursor = Ticks::from_secs_i64(5);
    app.session.sel_track = Some(0);
    app.insert_generated(GenSpec::ColorBars);
    app.cursor = Ticks::from_secs_i64(10);
    app.session.sel_track = Some(0);
    app.insert_generated(GenSpec::ColorBars);
    app.dlg.gen_editor = None;
    app.sel_events.clear();
    for _ in 0..4 {
        frame(&mut app, vec![]);
    }
    let ids: Vec<Id> = app.project.tracks[0].events.iter().map(|e| e.id).collect();
    assert_eq!(ids.len(), 3);
    let (a, b, c) = (ids[0], ids[1], ids[2]);

    let drag = |app: &mut App, frame: &mut dyn FnMut(&mut App, Vec<Event>), p0: Pos2, p1: Pos2| {
        frame(app, vec![Event::PointerMoved(p0), press(p0, true)]);
        for k in 1..=8 {
            let p = p0 + (p1 - p0) * (k as f32 / 8.0);
            frame(app, vec![Event::PointerMoved(p)]);
        }
        frame(app, vec![press(p1, false)]);
        frame(app, vec![]);
    };

    // 1. Modo Shuffle: arrastar C para antes de A reordena [C, A, B] sem lacunas.
    app.exec_cmd(Some(&ctx), "edit.tool.mode.shuffle");
    frame(&mut app, vec![]);
    let ra = app.event_screen_rect(a).expect("evento a na tela");
    let rc = app.event_screen_rect(c).expect("evento c na tela");
    drag(
        &mut app,
        &mut frame,
        rc.center(),
        ra.left_top() + vec2(4.0, ra.height() * 0.5),
    );
    assert_eq!(app.project.event(c).map(|e| e.start), Some(Ticks::ZERO));

    // 2. Modo Slide: desliza evento do meio (A) ajustando vizinhos sem mover seu conteúdo.
    app.exec_cmd(Some(&ctx), "edit.tool.mode.slide");
    frame(&mut app, vec![]);
    let ra_after = app.event_screen_rect(a).expect("evento a após shuffle");
    let a_off_before = app.project.event(a).map(|e| e.offset);
    let a_start_before = app.project.event(a).unwrap().start;
    drag(
        &mut app,
        &mut frame,
        ra_after.center(),
        ra_after.center() + vec2(40.0, 0.0),
    );
    assert!(app.project.event(a).unwrap().start > a_start_before);
    assert_eq!(app.project.event(a).map(|e| e.offset), a_off_before);
    assert_eq!(
        app.project.event(b).map(|e| e.start),
        app.project.event(a).map(|e| e.end())
    );

    // 3. Modo Stretch: arrastar borda direita altera Rate e comprimento.
    app.exec_cmd(Some(&ctx), "edit.tool.mode.stretch");
    frame(&mut app, vec![]);
    let rc_now = app.event_screen_rect(c).expect("evento c para stretch");
    let rate_before = app.project.event(c).unwrap().rate;
    drag(
        &mut app,
        &mut frame,
        pos2(rc_now.right() - 2.0, rc_now.center().y),
        pos2(rc_now.right() + 40.0, rc_now.center().y),
    );
    assert_ne!(app.project.event(c).unwrap().rate, rate_before);

    // 4. Modo SplitTrim: arrastar trecho interno recorta o segmento.
    app.exec_cmd(Some(&ctx), "edit.tool.mode.split_trim");
    frame(&mut app, vec![]);
    let n_before = app.project.tracks[0].events.len();
    let rb_now = app.event_screen_rect(b).expect("evento b para split trim");
    let rx0 = rb_now.left() + rb_now.width() * 0.25;
    let rx1 = rb_now.left() + rb_now.width() * 0.65;
    drag(
        &mut app,
        &mut frame,
        pos2(rx0, rb_now.center().y),
        pos2(rx1, rb_now.center().y),
    );
    assert!(app.project.tracks[0].events.len() > n_before);
}

#[test]
fn ui_sync_links_arraste_unidirecional_e_eixos_selecao() {
    use neoveg_engine::session::BoxAxisMode;

    let ctx = egui::Context::default();
    let mut app = App::with_context(&ctx, None);
    let mut t = 0.0f64;
    let mut frame = |app: &mut App, ev: Vec<Event>| {
        t += 1.0 / 60.0;
        let _ = ctx.run(raw(ev, Modifiers::NONE, t), |c| app.frame_ui(c));
    };
    frame(&mut app, vec![]);
    app.dismiss_splash();
    frame(&mut app, vec![]);
    app.dismiss_welcome();

    app.session.sel_track = Some(0);
    app.cursor = Ticks::ZERO;
    app.insert_generated(GenSpec::ColorBars);
    app.cursor = Ticks::from_secs_i64(7);
    app.session.sel_track = Some(0);
    app.insert_generated(GenSpec::ColorBars);
    app.session.sel_track = Some(1);
    app.cursor = Ticks::ZERO;
    app.session.insert_empty_event();
    app.dlg.gen_editor = None;
    for _ in 0..4 {
        frame(&mut app, vec![]);
    }
    let m_id = app.project.tracks[0].events[0].id;
    let f_id = app.project.tracks[1].events[0].id;
    let other_v = app.project.tracks[0].events[1].id;
    app.sel_events = [m_id, f_id].into_iter().collect();
    assert_eq!(app.session.create_sync_links(Some(m_id)), 1);
    app.sync_after_session();
    frame(&mut app, vec![]);

    let drag = |app: &mut App, frame: &mut dyn FnMut(&mut App, Vec<Event>), p0: Pos2, p1: Pos2| {
        frame(app, vec![Event::PointerMoved(p0), press(p0, true)]);
        for k in 1..=6 {
            let p = p0 + (p1 - p0) * (k as f32 / 6.0);
            frame(app, vec![Event::PointerMoved(p)]);
        }
        frame(app, vec![press(p1, false)]);
        frame(app, vec![]);
    };

    // 1. Arrastar mestre (m_id) arrasta o seguidor (f_id) pelo mesmo delta horizontal.
    app.sel_events = [m_id].into_iter().collect();
    let rm = app.event_screen_rect(m_id).expect("retângulo mestre");
    let m0 = app.project.event(m_id).unwrap().start;
    let f0 = app.project.event(f_id).unwrap().start;
    drag(
        &mut app,
        &mut frame,
        rm.center(),
        rm.center() + vec2(60.0, 0.0),
    );
    let dm = app.project.event(m_id).unwrap().start - m0;
    let df = app.project.event(f_id).unwrap().start - f0;
    assert!(dm > Ticks::ZERO);
    assert_eq!(dm, df, "seguidor Sync Link deve acompanhar mestre");

    // 2. Arrastar seguidor (f_id) não move o mestre (unidirecional).
    app.sel_events = [f_id].into_iter().collect();
    frame(&mut app, vec![]);
    let rf = app.event_screen_rect(f_id).expect("retângulo seguidor");
    let m_before = app.project.event(m_id).unwrap().start;
    let f_before = app.project.event(f_id).unwrap().start;
    drag(
        &mut app,
        &mut frame,
        rf.center(),
        rf.center() + vec2(40.0, 0.0),
    );
    assert!(app.project.event(f_id).unwrap().start > f_before);
    assert_eq!(app.project.event(m_id).unwrap().start, m_before);

    // 3. Eixo Vertical da ferramenta Seleção seleciona todos os eventos da trilha 0 cruzada.
    app.exec_cmd(Some(&ctx), "edit.tool.selection");
    app.exec_cmd(Some(&ctx), "edit.tool.axis.vertical");
    app.sel_events.clear();
    frame(&mut app, vec![]);
    let rm_now = app.event_screen_rect(m_id).expect("mestre após move");
    drag(
        &mut app,
        &mut frame,
        pos2(rm_now.left() + 4.0, rm_now.top() + 4.0),
        pos2(rm_now.left() + 10.0, rm_now.bottom() - 4.0),
    );
    assert!(app.sel_events.contains(&m_id));
    assert!(
        app.sel_events.contains(&other_v),
        "eixo vertical deve pegar toda a trilha"
    );
    assert!(!app.sel_events.contains(&f_id));

    // 4. Eixo Horizontal seleciona todas as trilhas dentro do intervalo temporal da caixa.
    app.exec_cmd(Some(&ctx), "edit.tool.axis.horizontal");
    app.sel_events.clear();
    frame(&mut app, vec![]);
    let rf_now = app.event_screen_rect(f_id).expect("seguidor após move");
    let box_x0 = rm_now.center().x.min(rf_now.center().x);
    let box_x1 = rm_now.center().x.max(rf_now.center().x);
    drag(
        &mut app,
        &mut frame,
        pos2(box_x0, rm_now.top() + 4.0),
        pos2(box_x1, rm_now.top() + 10.0),
    );
    assert_eq!(app.session.box_axis, BoxAxisMode::Horizontal);
    assert!(app.sel_events.contains(&m_id));
    assert!(
        app.sel_events.contains(&f_id),
        "eixo horizontal deve atravessar todas as trilhas"
    );
}

/// F11, Shift+F11 e Ctrl+F11 do VEGAS Pro 21: área de janelas, lista de
/// trilhas e timeline maximizada; Ctrl+F11 de novo volta ao estado de antes.
#[test]
fn f11_area_de_janelas_lista_de_trilhas_e_timeline_maximizada() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(&ctx, None);
    let mut t = 0.0;
    let mut frame = |app: &mut App, ev: Vec<Event>, mods: Modifiers| {
        t += 1.0 / 60.0;
        let _ = ctx.run(raw(ev, mods, t), |c| app.frame_ui(c));
    };
    frame(&mut app, vec![], Modifiers::NONE);
    app.dismiss_splash();
    frame(&mut app, vec![], Modifiers::NONE);
    app.dismiss_welcome();
    frame(&mut app, vec![], Modifiers::NONE);
    assert!(app.dock_visible);
    assert!(!app.tl.header_collapsed());

    frame(&mut app, key(Key::F11, Modifiers::NONE), Modifiers::NONE);
    assert!(!app.dock_visible, "F11 minimiza a área de janelas");
    frame(&mut app, key(Key::F11, Modifiers::NONE), Modifiers::NONE);
    assert!(app.dock_visible, "F11 restaura a área de janelas");

    frame(&mut app, key(Key::F11, Modifiers::SHIFT), Modifiers::SHIFT);
    assert!(
        app.tl.header_collapsed(),
        "Shift+F11 recolhe a lista de trilhas"
    );
    frame(&mut app, key(Key::F11, Modifiers::SHIFT), Modifiers::SHIFT);
    assert!(!app.tl.header_collapsed());

    frame(
        &mut app,
        key(Key::F11, Modifiers::COMMAND),
        Modifiers::COMMAND,
    );
    assert!(!app.dock_visible && app.tl.header_collapsed());
    frame(&mut app, vec![], Modifiers::NONE);
    frame(
        &mut app,
        key(Key::F11, Modifiers::COMMAND),
        Modifiers::COMMAND,
    );
    assert!(app.dock_visible && !app.tl.header_collapsed());

    // Com a timeline maximizada, F11 também restaura tudo.
    app.exec_cmd(Some(&ctx), "view.maximize_timeline");
    app.exec_cmd(Some(&ctx), "view.toggle_dock_area");
    assert!(app.dock_visible && !app.tl.header_collapsed());
    assert!(app.timeline_restore.is_none());

    // Painel maximizado na área de janelas: desenha e restaura sem pânico.
    app.dock_maximized = Some(crate::dock::DockPane::Tab(DockTab::Mixer));
    frame(&mut app, vec![], Modifiers::NONE);
    assert_eq!(
        app.dock_maximized,
        Some(crate::dock::DockPane::Tab(DockTab::Mixer))
    );
    app.detach_dock_tab(DockTab::Mixer);
    frame(&mut app, vec![], Modifiers::NONE);
    assert_eq!(
        app.dock_maximized, None,
        "destacado, deixa de estar maximizado"
    );
}

/// Layout novo: à esquerda da Prévia só Mídia do Projeto, Transições, Efeitos
/// de Vídeo, Geradores de Mídia e Plug-In Manager; o Trimmer no grupo da
/// Prévia; os outros painéis fechados até o menu Exibir abrir. "Fechar
/// painel" fecha de verdade e o layout salvo lembra. O zoom de teclado do
/// egui fica desligado (Ctrl+= / Ctrl+- são do tom em oitavas).
#[test]
fn dock_padrao_cinco_paineis_trimmer_na_previa_e_fechar_painel() {
    use crate::dock::{build_dock_tree, find_pane_tile, DockPane};
    use crate::layout::{builtin_presets, WindowLayout, DEFAULT_DOCK_TABS};

    let novo = WindowLayout::default();
    let fechados = novo.closed_panels();
    for t in DEFAULT_DOCK_TABS {
        assert!(!fechados.contains(&t), "{t:?} deve abrir no dock");
    }
    assert!(!fechados.contains(&DockTab::Trimmer));
    for t in [
        DockTab::Explorer,
        DockTab::Mixer,
        DockTab::Notes,
        DockTab::Encoder,
    ] {
        assert!(fechados.contains(&t), "{t:?} deve começar fechado");
    }
    // preset que abre nos Escopos mostra os Escopos
    let cor = builtin_presets()
        .into_iter()
        .find(|l| l.active_panel() == DockTab::Scopes)
        .expect("preset de cor");
    assert!(!cor.closed_panels().contains(&DockTab::Scopes));

    let tree = build_dock_tree(0.40, DockTab::ProjectMedia, false);
    let grupo = |p: DockPane| find_pane_tile(&tree, p).and_then(|id| tree.tiles.parent_of(id));
    assert_eq!(
        grupo(DockPane::Tab(DockTab::Trimmer)),
        grupo(DockPane::Preview)
    );
    assert_ne!(
        grupo(DockPane::Tab(DockTab::ProjectMedia)),
        grupo(DockPane::Preview)
    );

    let _lock = crate::kittest_tests::lock_test_prefs();
    let ctx = egui::Context::default();
    let mut app = {
        let _r = PREFS_LOCK
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        App::with_context(&ctx, None)
    };
    assert!(!ctx.options(|o| o.zoom_with_keyboard));
    app.apply_window_layout(WindowLayout::default());
    assert!(app.closed_tabs.contains(&DockTab::Mixer));
    assert!(!app.is_panel_open(DockTab::Mixer));

    app.exec_cmd(Some(&ctx), "view.tab.mixer");
    assert!(!app.closed_tabs.contains(&DockTab::Mixer));
    assert_eq!(app.dock_tab, DockTab::Mixer);

    app.close_dock_tab(DockTab::Mixer);
    assert!(app.closed_tabs.contains(&DockTab::Mixer));
    assert_ne!(app.dock_tab, DockTab::Mixer);
    let salvo = app.current_window_layout("teste");
    assert!(salvo.closed_panels().contains(&DockTab::Mixer));
    assert!(!salvo.closed_panels().contains(&DockTab::ProjectMedia));

    // destacado e depois fechado: some da janela própria também
    app.detach_dock_tab(DockTab::Transitions);
    app.close_dock_tab(DockTab::Transitions);
    assert!(!app.detached_tabs.contains(&DockTab::Transitions));
    assert!(app.closed_tabs.contains(&DockTab::Transitions));
}
