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
    crate::fonts::configure_fonts(&ctx, false);
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
    // Vegas 1.0: o Mixer fica como aba junto da Video Preview
    assert!(app.prefs.edit_layout.master_bus_docked);
    assert_eq!(
        app.prefs.edit_layout.master_bus_placement,
        neoveg_common::prefs::MasterBusPlacement::WithPreview
    );
    assert_eq!(app.theme_id, crate::theme::ThemeId::VegasClaro);
    frame(&mut app, vec![], Modifiers::NONE);

    app.exec_cmd(Some(&ctx), "edit.properties.layout.sony");
    assert_eq!(app.prefs.edit_layout.layout_type, crate::LayoutType::Sony);
    assert!(app.prefs.edit_layout.timeline_at_bottom);
    assert!(!app.prefs.edit_layout.tabs_at_top);
    assert!(app.prefs.edit_layout.transport_at_bottom);
    // Vegas 9 e 13: Master Bus entre as abas e a prévia
    assert!(app.prefs.edit_layout.master_bus_docked);
    assert_eq!(
        app.prefs.edit_layout.master_bus_placement,
        neoveg_common::prefs::MasterBusPlacement::Center
    );
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

pub(super) fn collect_galley_shapes(
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
    crate::fonts::configure_fonts(&ctx, false);
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
    // as duas primeiras abas da ordem padrão (o rodapé rola na horizontal)
    for tab_label in ["Mídia do Projeto", "Transições", "Destacar"] {
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
    crate::fonts::configure_fonts(&ctx, false);
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
    crate::fonts::configure_fonts(&ctx, false);
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
    crate::fonts::configure_fonts(&ctx, false);
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
    crate::fonts::configure_fonts(&ctx, false);
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
    crate::fonts::configure_fonts(&ctx, false);
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

/// Tela de abertura na própria janela (inicialização, como no VEGAS Pro):
/// enquanto ela está ativa só ela é desenhada; quando termina, a janela sai
/// do modo abertura. Sem abertura ativa o modo nem liga.
#[test]
fn abertura_em_janela_propria_desenha_so_a_abertura_e_depois_libera_a_janela() {
    let _lock = crate::kittest_tests::lock_test_prefs();
    let ctx = egui::Context::default();
    let mut app = {
        let _r = PREFS_LOCK
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        App::with_context(&ctx, None)
    };
    let small = |t: f64| egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            Pos2::ZERO,
            vec2(
                crate::splash::SPLASH_WINDOW_SIZE[0],
                crate::splash::SPLASH_WINDOW_SIZE[1],
            ),
        )),
        time: Some(t),
        ..Default::default()
    };
    // janela nasceu pequena, mas a abertura não está ativa (ex.: oferta de
    // recuperação): o primeiro quadro já devolve bordas e tamanho
    app.splash = crate::splash::SplashState::startup(false);
    app.set_startup_splash_window(true);
    let out = ctx.run(small(0.0), |c| app.frame_ui(c));
    assert!(
        !app.startup_window,
        "sem abertura ativa a janela não pode ficar presa no tamanho da abertura"
    );
    assert!(out.viewport_output.values().any(|v| v
        .commands
        .contains(&egui::ViewportCommand::Decorations(true))));

    app.splash = crate::splash::SplashState::startup(true);
    app.set_startup_splash_window(true);
    assert!(app.startup_window);
    let out = ctx.run(small(0.05), |c| app.frame_ui(c));
    assert!(app.startup_window);
    assert!(!out.viewport_output.values().any(|v| v
        .commands
        .contains(&egui::ViewportCommand::Decorations(true))));

    app.splash.advance_for_test(30.0, 0);
    let out = ctx.run(small(0.1), |c| app.frame_ui(c));
    assert!(
        !app.startup_window,
        "a abertura terminou: a janela sai do modo abertura"
    );
    let cmds: Vec<_> = out
        .viewport_output
        .values()
        .flat_map(|v| v.commands.iter().cloned())
        .collect();
    assert!(cmds.contains(&egui::ViewportCommand::Decorations(true)));
    assert!(cmds.contains(&egui::ViewportCommand::Resizable(true)));
    // sem tamanho máximo antes de liberar o redimensionamento: no X11 o
    // máximo da abertura (640x388) voltava e sumiam maximizar e minimizar
    let pos = |c: &egui::ViewportCommand| cmds.iter().position(|x| x == c);
    let max = pos(&egui::ViewportCommand::MaxInnerSize(egui::Vec2::INFINITY))
        .expect("a janela principal não pode herdar o tamanho máximo da abertura");
    let resizable = pos(&egui::ViewportCommand::Resizable(true)).expect("Resizable");
    assert!(max < resizable);
}

/// Num monitor 1440x900 (barra de tarefas de 40 pontos), a janela abria com
/// 820 pontos internos em y=40 e terminava em y=891: a barra de transporte e
/// a de status ficavam atrás da barra de tarefas. A janela inteira, barra de
/// título incluída, tem de caber acima da barra de tarefas.
#[test]
fn janela_principal_cabe_na_area_util_do_monitor() {
    use crate::app::main_window_geometry;
    for (mw, mh) in [
        (1366.0, 768.0),
        (1440.0, 900.0),
        (1600.0, 900.0),
        (1920.0, 1080.0),
        (2560.0, 1440.0),
    ] {
        let geo = main_window_geometry(vec2(mw, mh)).expect("monitor válido");
        let [min_w, min_h] = crate::MAIN_WINDOW_MIN;
        assert!(geo.size.x >= min_w && geo.size.y >= min_h);
        assert!(geo.pos.x >= 0.0 && geo.pos.y >= 0.0);
        assert!(
            geo.pos.x + geo.size.x <= mw,
            "{mw}x{mh}: a janela passa da borda direita"
        );
        // 32 de barra de título + 48 de barra de tarefas
        assert!(
            geo.pos.y + 32.0 + geo.size.y <= mh - 48.0,
            "{mw}x{mh}: a janela passa para trás da barra de tarefas ({geo:?})"
        );
    }
    // o tamanho padrão não cabe em 1440x900: abre maximizada
    let small = main_window_geometry(vec2(1440.0, 900.0)).expect("monitor válido");
    assert!(small.maximize);
    // em Full HD cabe: tamanho padrão, sem maximizar
    let full_hd = main_window_geometry(vec2(1920.0, 1080.0)).expect("monitor válido");
    assert!(!full_hd.maximize);
    assert_eq!(
        full_hd.size,
        vec2(crate::MAIN_WINDOW_SIZE[0], crate::MAIN_WINDOW_SIZE[1])
    );
    for bad in [
        vec2(0.0, 900.0),
        vec2(f32::NAN, 900.0),
        vec2(1440.0, f32::INFINITY),
        vec2(-1.0, -1.0),
    ] {
        assert!(main_window_geometry(bad).is_none());
    }
}

/// Sem a tela de abertura, a janela nasce com MAIN_WINDOW_SIZE; no primeiro
/// quadro em que o monitor é conhecido, ela é ajustada à área útil.
#[test]
fn janela_sem_abertura_se_ajusta_ao_monitor_no_primeiro_quadro() {
    let _lock = crate::kittest_tests::lock_test_prefs();
    let ctx = egui::Context::default();
    let mut app = {
        let _r = PREFS_LOCK
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        App::with_context(&ctx, None)
    };
    app.set_startup_splash_window(false);
    let raw = |t: f64, monitor: Option<egui::Vec2>| {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))),
            time: Some(t),
            ..Default::default()
        };
        if let Some(vp) = input.viewports.get_mut(&egui::ViewportId::ROOT) {
            vp.monitor_size = monitor;
        }
        input
    };
    let cmds = |out: &egui::FullOutput| -> Vec<egui::ViewportCommand> {
        out.viewport_output
            .values()
            .flat_map(|v| v.commands.iter().cloned())
            .collect()
    };
    // monitor ainda desconhecido: espera
    let out = ctx.run(raw(0.0, None), |c| app.frame_ui(c));
    assert!(!cmds(&out)
        .iter()
        .any(|c| matches!(c, egui::ViewportCommand::InnerSize(_))));
    // monitor 1440x900: ajusta e maximiza uma vez só
    let out = ctx.run(raw(0.05, Some(vec2(1440.0, 900.0))), |c| app.frame_ui(c));
    let first = cmds(&out);
    assert!(first.contains(&egui::ViewportCommand::Maximized(true)));
    assert!(first
        .iter()
        .any(|c| matches!(c, egui::ViewportCommand::InnerSize(_))));
    let out = ctx.run(raw(0.1, Some(vec2(1440.0, 900.0))), |c| app.frame_ui(c));
    assert!(
        !cmds(&out).contains(&egui::ViewportCommand::Maximized(true)),
        "o ajuste inicial não pode se repetir e brigar com o usuário"
    );
}

/// Exibir > Escala da interface: o comando muda a preferência e o quadro
/// seguinte aplica o fator no egui; os limites seguram cliques repetidos.
#[test]
fn escala_da_interface_pelo_menu_aplica_no_proximo_quadro() {
    let _lock = crate::kittest_tests::lock_test_prefs();
    let ctx = egui::Context::default();
    let mut app = {
        let _r = PREFS_LOCK
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        App::with_context(&ctx, None)
    };
    app.prefs.ui_scale_pct = 100;
    let raw = |t: f64| egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))),
        time: Some(t),
        ..Default::default()
    };
    app.exec_cmd(Some(&ctx), "view.ui_scale_up");
    assert_eq!(app.prefs.ui_scale_pct, 110);
    // o egui aplica o zoom novo no começo do quadro seguinte
    let _ = ctx.run(raw(0.0), |c| app.frame_ui(c));
    let _ = ctx.run(raw(0.05), |c| app.frame_ui(c));
    assert!((ctx.zoom_factor() - 1.1).abs() < 0.001);

    for _ in 0..40 {
        app.exec_cmd(Some(&ctx), "view.ui_scale_down");
    }
    assert_eq!(app.prefs.ui_scale_pct, neoveg_common::prefs::UI_SCALE_MIN);
    app.exec_cmd(Some(&ctx), "view.ui_scale_reset");
    assert_eq!(app.prefs.ui_scale_pct, 100);
    let _ = ctx.run(raw(0.1), |c| app.frame_ui(c));
    let _ = ctx.run(raw(0.15), |c| app.frame_ui(c));
    assert!((ctx.zoom_factor() - 1.0).abs() < 0.001);
}

/// Setas Esquerda/Direita (lista de atalhos do VEGAS Pro): com Quantizar
/// para quadros, cada toque anda pelo menos um quadro na grade, mesmo com o
/// zoom aberto (um pixel menor que um quadro); sem quantizar, um pixel.
#[test]
fn setas_andam_um_quadro_na_agulha_com_quantizar() {
    let _lock = crate::kittest_tests::lock_test_prefs();
    let ctx = egui::Context::default();
    let mut app = {
        let _r = PREFS_LOCK
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        App::with_context(&ctx, None)
    };
    let fps = app.project.settings.fps;
    let fd = app.project.settings.frame_dur();
    app.opts.quantize = true;
    // zoom aberto: 1 px bem menor que um quadro
    app.tl.px_per_sec = 4000.0;
    app.cursor = Ticks::ZERO;
    let t = app.arrow_step_target(1);
    assert_eq!(t, fps.frame_to_ticks(1));
    app.cursor = t;
    assert_eq!(app.arrow_step_target(1), fps.frame_to_ticks(2));
    assert_eq!(app.arrow_step_target(-1), Ticks::ZERO);
    // no início não passa de zero
    app.cursor = Ticks::ZERO;
    assert_eq!(app.arrow_step_target(-1), Ticks::ZERO);
    // cursor fora da grade volta para a grade
    app.cursor = fd + Ticks::from_raw(7);
    assert_eq!(app.arrow_step_target(1), fps.frame_to_ticks(2));
    // zoom fechado: um pixel vale vários quadros
    app.tl.px_per_sec = 2.0;
    app.cursor = Ticks::ZERO;
    assert!(app.arrow_step_target(1) > fps.frame_to_ticks(1));
    // sem quantizar: um pixel
    app.opts.quantize = false;
    app.tl.px_per_sec = 4000.0;
    app.cursor = Ticks::ZERO;
    let t = app.arrow_step_target(1);
    assert!(t > Ticks::ZERO && t < fd);

    // pelo teclado, sem foco em campo de texto
    app.opts.quantize = true;
    app.cursor = Ticks::ZERO;
    app.welcome_open = false;
    app.splash = crate::splash::SplashState::startup(false);
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))),
        events: vec![egui::Event::Key {
            key: egui::Key::ArrowRight,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
        ..Default::default()
    };
    let _ = ctx.run(raw, |c| app.handle_shortcuts(c));
    assert_eq!(app.cursor, fps.frame_to_ticks(1));
}

/// O Neoveg Encoder abre em janela própria, fora da área de janelas: não
/// entra na árvore do dock, o menu Exibir abre a janela, um layout salvo com
/// ele como aba ativa cai na Mídia do Projeto e fechar a janela fecha o painel.
#[test]
fn encoder_abre_em_janela_propria_fora_do_dock() {
    use crate::dock::{build_dock_tree, find_pane_tile, DockPane};
    use crate::layout::WindowLayout;

    let tree = build_dock_tree(0.40, DockTab::Encoder, false);
    assert!(find_pane_tile(&tree, DockPane::Tab(DockTab::Encoder)).is_none());

    let l = WindowLayout {
        active_tab: crate::layout::panel_key(DockTab::Encoder).to_string(),
        ..WindowLayout::default()
    };
    assert_eq!(l.active_panel(), DockTab::ProjectMedia);

    let _lock = crate::kittest_tests::lock_test_prefs();
    let ctx = egui::Context::default();
    let mut app = {
        let _r = PREFS_LOCK
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        App::with_context(&ctx, None)
    };
    let antes = app.dock_tab;
    app.exec_cmd(Some(&ctx), "view.tab.encoder");
    assert!(app.detached_tabs.contains(&DockTab::Encoder));
    assert!(!app.closed_tabs.contains(&DockTab::Encoder));
    assert_eq!(app.dock_tab, antes, "a aba ativa do dock não muda");
    app.close_dock_tab(DockTab::Encoder);
    assert!(!app.detached_tabs.contains(&DockTab::Encoder));
    assert!(app.closed_tabs.contains(&DockTab::Encoder));
}

/// Prévia de Vídeo (manual do VEGAS Pro 21, p. 197-209): zoom em passos de
/// 5%, Ignorar zoom alterna e volta, e o isolamento de canal só vale com as
/// Sobreposições ligadas (e muda a imagem enviada à textura).
#[test]
fn previa_zoom_5_por_cento_ignorar_zoom_e_isolamento_de_canal() {
    use crate::preview_overlays::ChannelIsolation;
    let _lock = crate::kittest_tests::lock_test_prefs();
    let ctx = egui::Context::default();
    let mut app = {
        let _r = PREFS_LOCK
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        App::with_context(&ctx, None)
    };
    app.apply_preview_zoom(0.0);
    app.exec_cmd(Some(&ctx), "preview.zoom_step_in");
    assert!((app.preview_zoom - 1.05).abs() < 1e-4);
    app.exec_cmd(Some(&ctx), "preview.zoom_step_out");
    app.exec_cmd(Some(&ctx), "preview.zoom_step_out");
    assert!((app.preview_zoom - 0.95).abs() < 1e-4);
    for _ in 0..500 {
        app.exec_cmd(Some(&ctx), "preview.zoom_step_out");
    }
    assert!(app.preview_zoom >= 0.25);

    app.apply_preview_zoom(2.0);
    app.preview_pan = vec2(30.0, -10.0);
    app.exec_cmd(Some(&ctx), "preview.zoom_bypass");
    assert!((app.preview_zoom - 1.0).abs() < 1e-4);
    assert!(app.preview_zoom_bypass.is_some());
    app.exec_cmd(Some(&ctx), "preview.zoom_bypass");
    assert!((app.preview_zoom - 2.0).abs() < 1e-4);
    assert_eq!(app.preview_pan, vec2(30.0, -10.0));
    assert!(app.preview_zoom_bypass.is_none());
    // no zoom de ajuste, Ignorar zoom não tem o que guardar
    app.apply_preview_zoom(0.0);
    app.exec_cmd(Some(&ctx), "preview.zoom_bypass");
    assert!(app.preview_zoom_bypass.is_none());

    app.preview_channel = ChannelIsolation::Red;
    app.opts.safe_areas = false;
    assert_eq!(app.effective_preview_channel().0, ChannelIsolation::Off);
    app.opts.safe_areas = true;
    assert_eq!(
        app.effective_preview_channel(),
        (ChannelIsolation::Red, false)
    );
    let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 100, 50, 255]));
    let ci = crate::app::isolated_color_image(&img, app.effective_preview_channel());
    assert_eq!(ci.pixels[0], egui::Color32::from_rgb(200, 0, 0));
    let ci = crate::app::isolated_color_image(&img, (ChannelIsolation::Off, false));
    assert_eq!(ci.pixels[0], egui::Color32::from_rgb(200, 100, 50));
}

/// Cabeçalho de trilha (manual do VEGAS Pro 21, p. 388): Minimizar e
/// Maximizar voltam à altura de antes quando clicados de novo; minimizar e
/// depois maximizar ainda volta à altura original; índice inválido não trava.
#[test]
fn minimizar_maximizar_altura_da_trilha_e_voltar() {
    let _lock = crate::kittest_tests::lock_test_prefs();
    let ctx = egui::Context::default();
    let mut app = {
        let _r = PREFS_LOCK
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        App::with_context(&ctx, None)
    };
    if app.project.tracks.is_empty() {
        app.session.insert_track(TrackKind::Video);
        app.sync_after_session();
    }
    app.tl.last_lanes = egui::Rect::from_min_size(Pos2::ZERO, vec2(800.0, 500.0));
    app.project.tracks[0].height = 90.0;

    app.toggle_track_height(0, false);
    assert_eq!(app.project.tracks[0].height, crate::timeline::TRACK_MIN_H);
    app.toggle_track_height(0, false);
    assert_eq!(app.project.tracks[0].height, 90.0);

    app.toggle_track_height(0, true);
    assert_eq!(app.project.tracks[0].height, 500.0);
    app.toggle_track_height(0, true);
    assert_eq!(app.project.tracks[0].height, 90.0);

    // minimizar, depois maximizar, depois maximizar de novo: volta aos 90
    app.toggle_track_height(0, false);
    app.toggle_track_height(0, true);
    assert_eq!(app.project.tracks[0].height, 500.0);
    app.toggle_track_height(0, true);
    assert_eq!(app.project.tracks[0].height, 90.0);

    // área das faixas inválida: Maximizar usa 400
    app.tl.last_lanes = egui::Rect::NOTHING;
    app.toggle_track_height(0, true);
    assert_eq!(app.project.tracks[0].height, 400.0);
    app.toggle_track_height(usize::MAX, true);
}

/// Janelas destacadas lembram posição e tamanho (`.neoveglayout`): a
/// geometria de abertura fica fixa enquanto a janela está aberta (o egui não
/// briga com o usuário ao mover), a janela reaberta parte do último lugar e
/// o layout salvo leva e traz as posições.
#[test]
fn janelas_destacadas_lembram_posicao_e_tamanho() {
    use crate::layout::FLOATING_PREVIEW_KEY as PV;
    let _lock = crate::kittest_tests::lock_test_prefs();
    let ctx = egui::Context::default();
    let mut app = {
        let _r = PREFS_LOCK
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        App::with_context(&ctx, None)
    };
    app.floating_geom.clear();
    app.floating_open.clear();
    // primeira abertura: sem geometria guardada
    assert_eq!(app.floating_open_geom(PV), None);
    app.remember_floating_geom(PV, Some([100.0, 80.0, 640.0, 360.0]));
    // ainda aberta: continua a de abertura
    assert_eq!(app.floating_open_geom(PV), None);
    // fechou (não desenhada num quadro) e reabriu: parte do último lugar
    app.floating_open.clear();
    assert_eq!(
        app.floating_open_geom(PV),
        Some([100.0, 80.0, 640.0, 360.0])
    );
    app.remember_floating_geom(PV, None);
    assert_eq!(
        app.floating_geom.get(PV),
        Some(&[100.0, 80.0, 640.0, 360.0])
    );

    let salvo = app.current_window_layout("teste");
    assert_eq!(
        salvo.floating_windows.get(PV),
        Some(&[100.0, 80.0, 640.0, 360.0])
    );
    app.floating_geom.clear();
    app.apply_window_layout(salvo);
    assert_eq!(
        app.floating_geom.get(PV),
        Some(&[100.0, 80.0, 640.0, 360.0])
    );
    assert!(
        app.floating_open.is_empty(),
        "layout novo reabre as janelas no lugar dele"
    );
}

/// Atalhos alternativos da lista do VEGAS Pro: Alt+` minimiza a área de
/// janelas (como F11). No ABNT2 a crase é tecla morta, então vale a tecla
/// física à esquerda do 1 mesmo quando ela produz outra tecla lógica.
#[test]
fn alt_crase_alterna_a_area_de_janelas_tambem_no_abnt2() {
    use crate::app::backtick_window_alias;
    assert_eq!(
        backtick_window_alias(false, false, true),
        Some("view.toggle_dock_area")
    );
    assert_eq!(
        backtick_window_alias(true, false, true),
        Some("view.maximize_timeline")
    );
    assert_eq!(
        backtick_window_alias(false, true, true),
        Some("view.toggle_track_list")
    );
    assert_eq!(backtick_window_alias(false, false, false), None);
    assert_eq!(backtick_window_alias(true, true, true), None);

    let _lock = crate::kittest_tests::lock_test_prefs();
    let ctx = egui::Context::default();
    let mut app = {
        let _r = PREFS_LOCK
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        App::with_context(&ctx, None)
    };
    app.welcome_open = false;
    app.splash = crate::splash::SplashState::startup(false);
    app.timeline_restore = None;
    let alt = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };
    let tecla = |key: Key, physical: Option<Key>| egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))),
        modifiers: alt,
        events: vec![egui::Event::Key {
            key,
            physical_key: physical,
            pressed: true,
            repeat: false,
            modifiers: alt,
        }],
        ..Default::default()
    };
    let antes = app.dock_visible;
    let _ = ctx.run(tecla(Key::Backtick, None), |c| app.handle_shortcuts(c));
    assert_eq!(app.dock_visible, !antes, "Alt+` como F11");
    // ABNT2: a tecla física da crase chega como outra tecla lógica
    let _ = ctx.run(tecla(Key::Quote, Some(Key::Backtick)), |c| {
        app.handle_shortcuts(c)
    });
    assert_eq!(app.dock_visible, antes, "tecla física da crase");
}

/// Painel Compositores com eventos de vídeo selecionados: o modo vale só
/// para eles, num passo de Desfazer, e "Usar o da trilha" volta ao da trilha.
#[test]
fn compositor_aplicado_aos_eventos_selecionados_com_desfazer() {
    use crate::model::BlendMode;
    let _lock = crate::kittest_tests::lock_test_prefs();
    let ctx = egui::Context::default();
    let mut app = {
        let _r = PREFS_LOCK
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        App::with_context(&ctx, None)
    };
    app.insert_generated(crate::model::GenSpec::ColorBars);
    let ids = app.selected_video_events();
    let ids = if ids.is_empty() {
        app.project
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Video)
            .flat_map(|t| t.events.iter().map(|e| e.id))
            .take(1)
            .collect::<Vec<_>>()
    } else {
        ids
    };
    let Some(&id) = ids.first() else {
        panic!("o gerador deve criar um evento de vídeo");
    };
    let undo_antes = app.session.undo.len();
    app.apply_event_compositor(&[id], Some(BlendMode::Multiply));
    assert_eq!(
        app.project.event(id).and_then(|e| e.blend),
        Some(BlendMode::Multiply)
    );
    assert_eq!(
        app.session.undo.len(),
        undo_antes + 1,
        "um passo de Desfazer"
    );
    app.apply_event_compositor(&[id], None);
    assert_eq!(app.project.event(id).and_then(|e| e.blend), None);
    // lista vazia não cria passo de Desfazer
    let n = app.session.undo.len();
    app.apply_event_compositor(&[], Some(BlendMode::Screen));
    assert_eq!(app.session.undo.len(), n);
}

/// Projeto `.neoveg` aninhado aberto em aba: editar não toca no principal,
/// trocar de aba não conta como edição, Salvar grava os dois e um arquivo
/// corrompido não abre aba.
#[test]
fn projeto_aninhado_editado_em_aba_e_salvo_junto() {
    let _lock = crate::kittest_tests::lock_test_prefs();
    let ctx = egui::Context::default();
    let mut app = {
        let _r = PREFS_LOCK
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        App::with_context(&ctx, None)
    };
    let dir = std::env::temp_dir().join(format!("neoveg_abas_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sub_path = dir.join("Sub.neoveg");
    let main_path = dir.join("Principal.neoveg");
    crate::model::Project::new_default()
        .save_file(&sub_path)
        .unwrap();
    app.session.path = Some(main_path.clone());
    app.prefs.project_backup_enabled = false;
    let main_tracks = app.project.tracks.len();

    // o próprio projeto principal não abre em aba
    crate::model::Project::new_default()
        .save_file(&main_path)
        .unwrap();
    app.open_nested_project_tab(&main_path);
    assert!(!app.has_doc_tabs());

    app.open_nested_project_tab(&sub_path);
    assert!(app.has_doc_tabs());
    assert_eq!(app.active_doc, 1);
    assert_eq!(app.path.as_deref(), Some(sub_path.as_path()));
    // uma aba por arquivo
    app.open_nested_project_tab(&sub_path);
    assert_eq!(app.doc_tabs.len(), 2);

    // editar o aninhado
    app.checkpoint();
    app.session.project.tracks.push(crate::model::Track::new(
        crate::model::TrackKind::Video,
        "Nova",
    ));
    app.changed();
    assert!(app.dirty);

    // voltar ao principal: ele não mudou e o aninhado continua "não salvo"
    app.switch_doc_tab(0);
    assert_eq!(app.active_doc, 0);
    assert_eq!(app.project.tracks.len(), main_tracks);
    assert_eq!(app.path.as_deref(), Some(main_path.as_path()));
    assert!(app.any_doc_dirty());
    assert_eq!(
        app.main_session().path.as_deref(),
        Some(main_path.as_path())
    );

    // trocar de aba sem editar não muda o contador de edições
    let edits = app.doc_tabs[1].synced_edits;
    app.switch_doc_tab(1);
    app.switch_doc_tab(0);
    assert_eq!(app.doc_tabs[1].synced_edits, edits);

    // Salvar grava o principal e o aninhado
    assert!(app.save_project(false));
    assert!(!app.any_doc_dirty());
    let (sub, _) = crate::model::Project::load_file(&sub_path).unwrap();
    assert_eq!(sub.tracks.len(), main_tracks + 1);

    // arquivo corrompido: erro na barra de status, nenhuma aba nova
    let bad = dir.join("Quebrado.neoveg");
    std::fs::write(&bad, b"PK\x03\x04lixo").unwrap();
    app.open_nested_project_tab(&bad);
    assert_eq!(app.doc_tabs.len(), 2);

    // fechar a aba sem alterações
    app.request_close_doc_tab(1);
    assert!(!app.has_doc_tabs());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Aba Exibição: a paleta de trilhas troca a cor só na tela, as cores de
/// envelope têm padrão do tema e a intensidade dos ícones muda a saturação.
#[test]
fn aba_exibicao_cores_de_trilha_envelope_e_icones() {
    let _lock = crate::kittest_tests::lock_test_prefs();
    let ctx = egui::Context::default();
    let mut app = {
        let _r = PREFS_LOCK
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        App::with_context(&ctx, None)
    };
    let p0 = crate::model::TRACK_COLORS[0];
    assert_eq!(app.track_rgb(p0), p0, "sem escolha, a cor padrão");
    app.prefs.display.set_track_color(0, Some([1, 2, 3]));
    assert_eq!(app.track_rgb(p0), [1, 2, 3]);
    assert_eq!(
        app.track_rgb([9, 9, 9]),
        [9, 9, 9],
        "cor fora da paleta não muda"
    );
    // a trilha guarda a cor original: a troca é só de exibição
    let t = crate::model::Track::new(crate::model::TrackKind::Video, "V");
    assert!(crate::model::TRACK_COLORS.contains(&t.color));

    let fb = egui::Color32::from_rgb(10, 20, 30);
    assert_eq!(app.envelope_color(1, fb), fb);
    app.prefs.display.envelope_colors[1] = Some([200, 0, 0]);
    assert_eq!(
        app.envelope_color(1, fb),
        egui::Color32::from_rgb(200, 0, 0)
    );
    assert_eq!(app.envelope_color(99, fb), fb);

    let c = egui::Color32::from_rgb(240, 60, 20);
    assert_eq!(crate::icons::saturate(c, 100), c);
    let g = crate::icons::saturate(c, 0);
    assert!(g.r() == g.g() && g.g() == g.b(), "0% é cinza");
    let vivo = crate::icons::saturate(c, 200);
    assert!(vivo.r() >= c.r() && vivo.b() <= c.b());
    crate::icons::set_display_prefs(100, None);
}

/// Posição do Master Bus na área de janelas por geração: à direita (21 em
/// diante), entre as abas e a prévia (9 e 13) e como aba da prévia (1.0).
#[test]
fn master_bus_nas_tres_posicoes_da_area_de_janelas() {
    use crate::dock::{build_dock_tree_with, DockPane};
    use egui_tiles::{Container, Tile};
    use neoveg_common::prefs::MasterBusPlacement as Mb;
    let kinds = |place: Mb| -> Vec<Vec<DockPane>> {
        let tree = build_dock_tree_with(0.40, DockTab::ProjectMedia, false, place);
        let root = tree.root.unwrap();
        let Some(Tile::Container(Container::Linear(lin))) = tree.tiles.get(root) else {
            panic!("raiz horizontal");
        };
        lin.children
            .iter()
            .map(|c| match tree.tiles.get(*c) {
                Some(Tile::Container(Container::Tabs(t))) => t
                    .children
                    .iter()
                    .filter_map(|p| match tree.tiles.get(*p) {
                        Some(Tile::Pane(pane)) => Some(*pane),
                        _ => None,
                    })
                    .collect(),
                _ => vec![],
            })
            .collect()
    };
    let right = kinds(Mb::Right);
    assert_eq!(right.len(), 3);
    assert_eq!(right[2], vec![DockPane::MasterBus]);
    let center = kinds(Mb::Center);
    assert_eq!(center.len(), 3);
    assert_eq!(center[1], vec![DockPane::MasterBus]);
    assert!(center[2].contains(&DockPane::Preview));
    let with_pv = kinds(Mb::WithPreview);
    assert_eq!(with_pv.len(), 2);
    assert!(with_pv[1].contains(&DockPane::Preview) && with_pv[1].contains(&DockPane::MasterBus));
}

#[test]
fn floating_docks_layout_serialization_and_sanitize() {
    use crate::dock::DockPane;
    use crate::layout::{FloatingDockGroup, WindowLayout};

    let mut layout = WindowLayout::default();
    layout.floating_docks.push(FloatingDockGroup {
        id: "group1".to_string(),
        panes: vec![
            DockPane::Tab(crate::DockTab::ProjectMedia),
            DockPane::Tab(crate::DockTab::Explorer),
            DockPane::Tab(crate::DockTab::ProjectMedia),
        ],
        active: 99,
    });
    layout.floating_docks.push(FloatingDockGroup {
        id: "   ".to_string(),
        panes: vec![DockPane::Preview],
        active: 0,
    });
    layout.floating_docks.push(FloatingDockGroup {
        id: "vazio".to_string(),
        panes: vec![],
        active: 0,
    });

    layout.sanitize();

    assert_eq!(layout.floating_docks.len(), 2);
    assert_eq!(layout.floating_docks[0].id, "group1");
    assert_eq!(layout.floating_docks[0].panes.len(), 2);
    assert_eq!(layout.floating_docks[0].active, 0);
    assert_eq!(layout.floating_docks[1].id, "preview");
    assert_eq!(layout.floating_docks[1].panes, vec![DockPane::Preview]);
    assert!(layout.detached_tabs.iter().any(|s| s == "project_media"));
    assert!(layout.detached_tabs.iter().any(|s| s == "explorer"));
    assert!(layout.preview_detached);

    let json = serde_json::to_string(&layout).expect("serialização json");
    let loaded: WindowLayout = serde_json::from_str(&json).expect("desserialização json");
    assert_eq!(loaded.floating_docks.len(), 2);
    assert_eq!(loaded.floating_docks[0].panes.len(), 2);
}

#[test]
fn app_floating_docks_grouping_detaching_and_docking() {
    use crate::dock::DockPane;

    let ctx = egui::Context::default();
    let mut app = {
        let _r = PREFS_LOCK
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        App::with_context(&ctx, None)
    };

    assert!(app.floating_docks.is_empty());

    app.detach_dock_tab(crate::DockTab::ProjectMedia);
    app.preview_detached = true;
    app.sync_floating_docks();
    assert_eq!(app.floating_docks.len(), 2);

    let target = DockPane::Tab(crate::DockTab::ProjectMedia);
    let to_add = DockPane::Preview;
    app.group_floating_panes(target, to_add);

    assert_eq!(app.floating_docks.len(), 1);
    let grp = &app.floating_docks[0];
    assert_eq!(grp.panes.len(), 2);
    assert_eq!(grp.panes[0], target);
    assert_eq!(grp.panes[1], to_add);
    assert_eq!(grp.active, 1);

    app.detach_pane_to_own_window(DockPane::Preview);
    assert_eq!(app.floating_docks.len(), 2);
    let preview_grp = app
        .floating_docks
        .iter()
        .find(|g| g.panes.contains(&DockPane::Preview))
        .expect("grupo com prévia");
    assert_eq!(preview_grp.panes.len(), 1);

    app.dock_pane(DockPane::Preview);
    assert!(!app.preview_detached);
    assert_eq!(app.floating_docks.len(), 1);

    let remaining_id = app.floating_docks[0].id.clone();
    app.dock_floating_group(&remaining_id);
    assert!(app.floating_docks.is_empty());
    assert!(!app.detached_tabs.contains(&crate::DockTab::ProjectMedia));
}

#[test]
fn teste_aninhar_selecao_fluxo_completo_e_desfazer() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(&ctx, None);
    let m = MediaItem {
        id: new_id(),
        name: "Video Teste".into(),
        source: MediaSource::Generated(GenSpec::ColorBars),
        duration: Some(Ticks::from_secs_i64(20)),
        has_video: true,
        has_audio: false,
        width: 1920,
        height: 1080,
        fps: Some(FrameRate::FPS_30),
        info: String::new(),
        proxy_path: None,
    };
    app.project.media.push(m.clone());

    let ev1 = crate::model::Event::new(&m, Ticks::from_secs_i64(2), Ticks::from_secs_i64(4));
    let ev2 = crate::model::Event::new(&m, Ticks::from_secs_i64(4), Ticks::from_secs_i64(5));
    let ev1_id = ev1.id;
    let ev2_id = ev2.id;
    app.project.tracks[0].events.push(ev1);
    app.project.tracks[0].events.push(ev2);

    app.session.sel_events.insert(ev1_id);
    app.session.sel_events.insert(ev2_id);
    app.sel_events = app.session.sel_events.clone();

    // Executa comando de aninhamento
    app.exec_cmd(Some(&ctx), "sequence.nest_selection");

    // Na timeline ativa resta 1 evento aninhado
    assert_eq!(app.project.tracks[0].events.len(), 1);
    let nested_clip = &app.project.tracks[0].events[0];
    assert_eq!(nested_clip.start, Ticks::from_secs_i64(2));
    assert_eq!(nested_clip.length, Ticks::from_secs_i64(7)); // de 2s a 9s

    // Nova sequência criada
    assert_eq!(app.project.sequences.len(), 1);
    let seq = &app.project.sequences[0];
    assert_eq!(seq.tracks[0].events.len(), 2);
    assert_eq!(seq.tracks[0].events[0].start, Ticks::ZERO);
    assert_eq!(seq.tracks[0].events[1].start, Ticks::from_secs_i64(2));

    // Desfazer restaura os 2 eventos originais
    app.exec_cmd(None, "edit.undo");
    assert_eq!(app.project.tracks[0].events.len(), 2);
    assert!(app.project.tracks[0].events.iter().any(|e| e.id == ev1_id));
    assert!(app.project.tracks[0].events.iter().any(|e| e.id == ev2_id));

    // Refazer volta a ter o clipe aninhado
    app.exec_cmd(None, "edit.redo");
    assert_eq!(app.project.tracks[0].events.len(), 1);
}

#[test]
fn teste_aninhar_selecao_deteccao_de_ciclo_e_duplo_clique() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(&ctx, None);
    let m = MediaItem {
        id: new_id(),
        name: "Clipe A".into(),
        source: MediaSource::Generated(GenSpec::ColorBars),
        duration: Some(Ticks::from_secs_i64(10)),
        has_video: true,
        has_audio: false,
        width: 1920,
        height: 1080,
        fps: Some(FrameRate::FPS_30),
        info: String::new(),
        proxy_path: None,
    };
    app.project.media.push(m.clone());

    let ev = crate::model::Event::new(&m, Ticks::from_secs_i64(1), Ticks::from_secs_i64(3));
    let ev_id = ev.id;
    app.project.tracks[0].events.push(ev);
    app.session.sel_events.insert(ev_id);

    let active_id = app.project.active_sequence_id.unwrap_or(1);
    let nested_id = app
        .session
        .nest_selection(Some("Sequência Sub"))
        .unwrap_or(0);
    assert_ne!(nested_id, 0);

    // Verificação de ciclo: aninhar a ativa dentro da sub criada deve ser proibido
    assert!(app
        .project
        .validate_no_sequence_cycle(nested_id, active_id)
        .is_err());
    // Auto-referência direta também proibida
    assert!(app
        .project
        .validate_no_sequence_cycle(nested_id, nested_id)
        .is_err());

    // Duplo clique / alternância pelo ID da sequência aninhada
    let clip_media_id = app.project.tracks[0].events[0].media_id;
    let found_seq_id = app.nested_sequence_id(clip_media_id);
    assert_eq!(found_seq_id, Some(nested_id));

    assert!(app.switch_sequence_app(nested_id));
    assert_eq!(app.project.active_sequence_id, Some(nested_id));
}

/// Barra da Prévia numa linha só: o que não cabe vai para o menu "⋯", que
/// também precisa de espaço; com tudo cabendo, o menu não aparece.
#[test]
fn barra_da_previa_cabe_numa_linha_com_menu_de_excedentes() {
    use crate::preview_toolbar::{preview_tools_that_fit, PreviewTool};
    let gap = 4.0;
    // tudo cabe: sem menu
    assert_eq!(preview_tools_that_fit(&[26.0; 5], 500.0, gap), 5);
    assert_eq!(preview_tools_that_fit(&[], 10.0, gap), 0);
    // não cabe: as primeiras ficam, com espaço para o "⋯" (24 + gap)
    let widths = [26.0; 19];
    let n = preview_tools_that_fit(&widths, 300.0, gap);
    assert!(n < widths.len());
    let used: f32 = widths[..n].iter().map(|w| w + gap).sum();
    assert!(used + 24.0 + gap <= 300.0, "a barra passou da largura");
    assert!(
        used + 26.0 + gap + 24.0 + gap > 300.0,
        "sobrou espaço sem uso"
    );
    // painel estreito demais: tudo no menu, sem pânico
    assert_eq!(preview_tools_that_fit(&widths, 10.0, gap), 0);
    assert_eq!(preview_tools_that_fit(&widths, -5.0, gap), 0);
    // larguras inválidas não quebram a conta
    assert_eq!(
        preview_tools_that_fit(&[f32::NAN, f32::INFINITY, -3.0], 100.0, gap),
        3
    );
    // a barra começa pelas ferramentas da barra do VEGAS Pro
    assert_eq!(PreviewTool::ALL[0], PreviewTool::ProjectProps);
    assert_eq!(PreviewTool::ALL[1], PreviewTool::ExternalMonitor);
    assert_eq!(PreviewTool::ALL.last(), Some(&PreviewTool::ClearRam));
}

#[test]
fn open_fx_target_abre_plugin_chooser_quando_cadeia_vazia() {
    let ctx = egui::Context::default();
    let mut app = App::with_context(&ctx, None);
    let tid = app.project.tracks[0].id;

    // Sem efeitos na trilha: abre o Plug-In Chooser (semântica VEGAS Pro)
    app.open_fx_target(FxTarget::Track(tid));
    assert_eq!(app.dlg.fx_target, Some(FxTarget::Track(tid)));
    assert!(
        app.dlg.fx_chooser_open,
        "clicar em FX com cadeia vazia deve abrir o Plug-In Chooser"
    );

    // Com efeito já aplicado: abre direto no editor de parâmetros da cadeia
    app.project.tracks[0]
        .fx
        .push(crate::model::FxInstance::new(crate::model::FxKind::Invert));
    app.open_fx_target(FxTarget::Track(tid));
    assert!(!app.dlg.fx_chooser_open);
}

#[test]
fn agulha_preservada_ao_alternar_sequencias_e_projetos_abertos() {
    let _lock = crate::kittest_tests::lock_test_prefs();
    let ctx = egui::Context::default();
    let mut app = {
        let _r = PREFS_LOCK
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        App::with_context(&ctx, None)
    };
    app.project.ensure_sequences();
    let seq1 = app.project.active_sequence_id.expect("seq1");
    let t1 = Ticks::from_secs_i64(5);
    let t2 = Ticks::from_secs_i64(12);
    // Simula play_start diferente de cursor (reproduz o bug antigo onde `stop(false)` zerava a agulha)
    app.session.play_start = Ticks::ZERO;
    app.session.cursor = t1;

    let seq2 = app.create_sequence_app(Some("Sequência 02"));
    assert_eq!(app.session.cursor, Ticks::ZERO);
    app.session.play_start = Ticks::ZERO;
    app.session.cursor = t2;

    assert!(app.switch_sequence_app(seq1));
    assert_eq!(
        app.session.cursor, t1,
        "agulha da Sequência 01 deve ficar em 5s"
    );
    assert!(app.switch_sequence_app(seq2));
    assert_eq!(
        app.session.cursor, t2,
        "agulha da Sequência 02 deve ficar em 12s"
    );

    // Também preserva a agulha entre projetos abertos em abas (principal <-> aninhado)
    let dir = std::env::temp_dir().join(format!("neoveg_doc_cursor_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let sub_path = dir.join("sub.neoveg");
    crate::model::Project::new_default()
        .save_file(&sub_path)
        .unwrap();
    app.session.play_start = Ticks::ZERO;
    app.open_nested_project_tab(&sub_path);
    assert_eq!(app.active_doc, 1);
    let t_sub = Ticks::from_secs_i64(8);
    app.session.play_start = Ticks::ZERO;
    app.session.cursor = t_sub;

    app.switch_doc_tab(0);
    assert_eq!(
        app.session.cursor, t2,
        "agulha do projeto principal deve permanecer em 12s"
    );
    app.switch_doc_tab(1);
    assert_eq!(
        app.session.cursor, t_sub,
        "agulha do projeto aninhado deve permanecer em 8s"
    );
    app.request_close_doc_tab(1);
    let _ = std::fs::remove_dir_all(&dir);
}
