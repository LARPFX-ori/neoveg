//! Testes reais de interação de interface com `egui_kittest` e AccessKit.

use super::*;
use crate::dock::{find_pane_tile, DockPane};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

fn build_ready_app(cc: &mut eframe::CreationContext<'_>) -> App {
    let mut app = App::new(cc, None);
    app.splash.active = false;
    app.pending_welcome_after_splash = false;
    app.dismiss_welcome();
    app.select_layout_type(Some(&cc.egui_ctx), LayoutType::Magix, false);
    app
}

#[test]
fn kittest_dock_abas_inferiores_e_topo_e_master_bus() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 920.0))
        .build_eframe(build_ready_app);

    harness.run_steps(2);

    // Abas inferiores padrão do VEGAS Pro 21 acessíveis via AccessKit.
    harness.get_by_label("Color Grading").click();
    harness.run_steps(2);
    assert_eq!(harness.state().dock_tab, DockTab::ColorGrading);

    // Master Bus acoplado no lado direito reage a cliques reais de monitoração.
    assert!(!harness.state().master_mono);
    harness.get_by_label("Mono").click();
    harness.run_steps(2);
    assert!(harness.state().master_mono);

    assert!(!harness.state().master_dim);
    harness.get_by_label("Dim").click();
    harness.run_steps(2);
    assert!(harness.state().master_dim);

    // Ordem horizontal das 3 colunas padrão: Abas | Prévia | Master Bus.
    let tabs_rect = find_pane_tile(
        &harness.state().dock_tree,
        DockPane::Tab(DockTab::ColorGrading),
    )
    .and_then(|id| harness.state().dock_tree.tiles.rect(id))
    .expect("painel de abas deve ter retângulo calculado");
    let pv_rect = find_pane_tile(&harness.state().dock_tree, DockPane::Preview)
        .and_then(|id| harness.state().dock_tree.tiles.rect(id))
        .expect("prévia deve ter retângulo calculado");
    let mb_rect = find_pane_tile(&harness.state().dock_tree, DockPane::MasterBus)
        .and_then(|id| harness.state().dock_tree.tiles.rect(id))
        .expect("master bus deve ter retângulo calculado");
    assert!(tabs_rect.left() < pv_rect.left());
    assert!(pv_rect.left() < mb_rect.left());

    // Alterna para abas no topo e troca de aba pela barra nativa do egui_tiles.
    let mut top_prefs = harness.state().prefs.edit_layout.clone();
    top_prefs.tabs_at_top = true;
    top_prefs.preview_on_left = true;
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .apply_edit_layout(Some(&ctx), top_prefs, false, false);
    harness.run_steps(2);

    harness.get_by_label("Mixer").click();
    harness.run_steps(2);
    assert_eq!(harness.state().dock_tab, DockTab::Mixer);

    // Prévia à esquerda coloca a Prévia antes das Abas e antes do Master Bus.
    let tabs_rect = find_pane_tile(&harness.state().dock_tree, DockPane::Tab(DockTab::Mixer))
        .and_then(|id| harness.state().dock_tree.tiles.rect(id))
        .expect("painel de abas deve ter retângulo após inverter");
    let pv_rect = find_pane_tile(&harness.state().dock_tree, DockPane::Preview)
        .and_then(|id| harness.state().dock_tree.tiles.rect(id))
        .expect("prévia deve ter retângulo à esquerda");
    let mb_rect = find_pane_tile(&harness.state().dock_tree, DockPane::MasterBus)
        .and_then(|id| harness.state().dock_tree.tiles.rect(id))
        .expect("master bus deve permanecer à direita");
    assert!(pv_rect.left() < tabs_rect.left());
    assert!(tabs_rect.left() < mb_rect.left());
}

#[test]
fn kittest_welcome_screen_interacao_e_monitor_pequeno() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .build_eframe(|cc| {
            let mut app = build_ready_app(cc);
            app.open_welcome();
            app
        });

    harness.run_steps(2);
    assert!(harness.state().is_welcome_open());

    // Seleciona retrato 9:16 e verifica ajuste automático de projeto ao criar.
    harness.get_by_label("Portrait (9:16)").click();
    harness.run_steps(2);

    // Em monitor reduzido (760x520) os controles principais continuam acessíveis.
    harness.set_size(egui::vec2(760.0, 520.0));
    harness.run_steps(2);
    let create_btn = harness.get_by_label("Criar Projeto");
    let bounds = create_btn.raw_bounds().expect("botão deve possuir limites");
    assert!(bounds.x1 <= 760.0 + 1.5 && bounds.y1 <= 520.0 + 1.5);

    create_btn.click();
    harness.run_steps(2);
    assert!(!harness.state().is_welcome_open());
    assert!(
        harness.state().project.settings.height > harness.state().project.settings.width,
        "projeto Portrait (9:16) deve ter altura maior que largura"
    );
}

#[test]
fn kittest_layout_salvo_e_preferencias_navegacao() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1360.0, 840.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);

    // Salva e recarrega `.neoveglayout` preservando a árvore do egui_tiles.
    let snap = harness.state().current_window_layout("Teste Kittest");
    assert!(snap.tile_tree.is_some());
    let path = std::env::temp_dir().join("neoveg_kittest_layout.neoveglayout");
    crate::layout::save_layout_file(&snap, &path).expect("gravação do layout de teste");
    let loaded = crate::layout::load_layout_file(&path).expect("leitura do layout de teste");
    let _ = std::fs::remove_file(&path);
    assert!(loaded.tile_tree.is_some());

    harness.state_mut().apply_window_layout(loaded);
    harness.run_steps(2);

    // Abre Preferências pela Tabela de Comandos, navega por categorias e cancela sem sujar disco.
    let ctx = harness.ctx.clone();
    harness.state_mut().exec_cmd(Some(&ctx), "options.prefs");
    harness.run_steps(2);
    assert!(harness.state().dlg.prefs_open);

    harness.get_by_label("Aparência e Layout").click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().dlg.prefs_tab,
        crate::dialogs::PrefsCategory::Appearance
    );

    harness.get_by_label("Memória e Desempenho").click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().dlg.prefs_tab,
        crate::dialogs::PrefsCategory::Memory
    );

    harness.get_by_label("Cancelar").click();
    harness.run_steps(2);
    assert!(!harness.state().dlg.prefs_open);
}

#[test]
fn kittest_mixer_barramentos_auxiliares_e_taxa_reproducao() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);

    harness.get_by_label("Mixer").click();
    harness.run_steps(2);
    assert_eq!(harness.state().dock_tab, DockTab::Mixer);
    assert!(harness.state().project.audio_buses.is_empty());

    harness.get_by_label("＋ Barramento").click();
    harness.run_steps(2);
    assert_eq!(harness.state().project.audio_buses.len(), 1);
    assert_eq!(harness.state().project.audio_buses[0].name, "Bus A");

    assert!(harness.get_all_by_label("1.00×").count() >= 1);
    harness.state_mut().playback_rate = 1.5;
    harness.state_mut().play();
    harness.run_steps(2);
    harness.state_mut().stop(false);
    harness.run_steps(2);
}
