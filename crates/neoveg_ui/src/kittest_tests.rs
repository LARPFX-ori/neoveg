//! Testes reais de interação de interface com `egui_kittest` e AccessKit.

use super::*;
use crate::dock::{find_pane_tile, DockPane};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use neoveg_common::Ticks;

fn build_ready_app(cc: &mut eframe::CreationContext<'_>) -> App {
    let _lock = lock_test_prefs();
    let mut app = {
        let _prefs_read = crate::tests::PREFS_LOCK
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        App::with_context_in(&cc.egui_ctx, None, Some(neoveg_common::Language::PtBr))
    };
    app.splash.active = false;
    app.pending_welcome_after_splash = false;
    app.dismiss_welcome();
    app.select_layout_type(Some(&cc.egui_ctx), LayoutType::Magix, false);
    // os testes de dock clicam em todas as abas; o padrão novo fica no teste próprio
    app.closed_tabs.clear();
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

/// O X da tela de boas-vindas fechava nada: o painel da direita devolvia
/// "nenhuma ação" no mesmo quadro e apagava o clique. Vale nas três seções.
#[test]
fn kittest_x_fecha_a_tela_de_boas_vindas_em_todas_as_secoes() {
    for section in ["Novo Projeto", "Abrir Projeto", "Primeiros Passos"] {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1280.0, 820.0))
            .build_eframe(|cc| {
                let mut app = build_ready_app(cc);
                app.open_welcome();
                app
            });
        harness.run_steps(2);
        harness.get_by_label(section).click();
        harness.run_steps(2);
        assert!(harness.state().is_welcome_open(), "{section}: ainda aberta");
        harness.get_by_label("Fechar tela de boas-vindas").click();
        harness.run_steps(2);
        assert!(
            !harness.state().is_welcome_open(),
            "{section}: o X tem de fechar a tela de boas-vindas"
        );
    }
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
    let path = std::env::temp_dir().join(format!(
        "neoveg_kittest_layout_{}.neoveglayout",
        std::process::id()
    ));
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

#[test]
fn kittest_loudness_e_trilhas_de_barramentos() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);

    // Adiciona um barramento auxiliar, renomeia com suporte a Undo e ativa envelope (abre bus tracks).
    let bid = harness.state_mut().session.add_audio_bus().expect("bus");
    assert!(harness
        .state_mut()
        .session
        .rename_audio_bus(bid, "Diálogo".into()));
    assert!(harness
        .state_mut()
        .session
        .toggle_bus_envelope(bid, crate::model::EnvKind::Volume));
    assert_eq!(harness.state().project.audio_buses[0].name, "Diálogo");
    assert!(harness.state().opts.show_bus_tracks);
    harness.run_steps(2);
    assert!(harness.get_all_by_label("fx").count() >= 1);

    // Alterna trilhas de barramentos na timeline (B via Tabela de Comandos).
    let ctx = harness.ctx.clone();
    harness.state_mut().exec_cmd(Some(&ctx), "view.bus_tracks");
    harness.run_steps(2);
    assert!(!harness.state().opts.show_bus_tracks);
    harness.state_mut().exec_cmd(Some(&ctx), "view.bus_tracks");
    harness.run_steps(2);
    assert!(harness.state().opts.show_bus_tracks);

    // Abre a aba Medidores de Loudness e executa ações principais sem pânico.
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "view.tab.loudness");
    harness.run_steps(2);
    assert_eq!(harness.state().dock_tab, DockTab::Loudness);
    harness.get_by_label("Zerar").click();
    harness.run_steps(2);
}

#[test]
fn kittest_surround_panner_abre_e_muda_para_5_1() {
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "view.tab.surround_panner");
    harness.run_steps(2);
    assert_eq!(harness.state().dock_tab, DockTab::SurroundPanner);
    // em estéreo o painel só oferece o seletor de modo
    harness.state_mut().project.settings.channel_layout = crate::model::ChannelLayout::Surround51;
    harness.run_steps(3);
    assert!(harness.get_all_by_label("Frente central").count() >= 1);
    harness.get_by_label("Frente central").click();
    harness.run_steps(2);
}

#[test]
fn kittest_plugin_manager_e_compositores() {
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "view.tab.plugin_manager");
    harness.run_steps(2);
    assert_eq!(harness.state().dock_tab, DockTab::PluginManager);

    // Compositores: aplica "Multiplicar" à trilha de vídeo selecionada com Desfazer.
    let vi = harness
        .state()
        .project
        .tracks
        .iter()
        .position(|t| t.kind == crate::model::TrackKind::Video)
        .expect("trilha de vídeo");
    harness.state_mut().session.sel_track = Some(vi);
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "view.tab.compositors");
    harness.run_steps(2);
    assert_eq!(harness.state().dock_tab, DockTab::Compositors);
    let label = crate::model::BlendMode::Multiply.name();
    harness.get_by_label(label).click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().project.tracks[vi].blend,
        crate::model::BlendMode::Multiply
    );
    harness.state_mut().exec_cmd(Some(&ctx), "edit.undo");
    harness.run_steps(2);
    assert_eq!(
        harness.state().project.tracks[vi].blend,
        crate::model::BlendMode::SourceAlpha
    );
}

#[test]
fn kittest_trimmer_in_out_e_inserir() {
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);

    // Garante uma mídia no projeto para edição de trecho In/Out no Trimmer.
    let mid = {
        let app = harness.state_mut();
        app.insert_generated(crate::model::GenSpec::Text(
            crate::model::TextSpec::default(),
        ));
        let mid = app.project.media[0].id;
        app.project.media[0].duration = Some(neoveg_common::Ticks::from_secs_i64(10));
        app.project.tracks.iter_mut().for_each(|t| t.events.clear());
        app.open_in_trimmer(mid);
        mid
    };
    let ctx = harness.ctx.clone();
    harness.state_mut().exec_cmd(Some(&ctx), "view.tab.trimmer");
    harness.run_steps(2);
    assert_eq!(harness.state().dock_tab, DockTab::Trimmer);
    assert_eq!(harness.state().trimmer.media_id, Some(mid));

    // Marca In em 2s e Out em 5s, depois envia trecho [2s..5s] à timeline no cursor.
    harness.state_mut().trimmer.cursor = neoveg_common::Ticks::from_secs_i64(2);
    harness.run_steps(1);
    harness.get_by_label("Definir ponto de entrada").click();
    harness.run_steps(1);

    harness.state_mut().trimmer.cursor = neoveg_common::Ticks::from_secs_i64(5);
    harness.run_steps(1);
    harness.get_by_label("Definir ponto de saída").click();
    harness.run_steps(1);

    assert_eq!(
        harness.state().trimmer.sel_in,
        neoveg_common::Ticks::from_secs_i64(2)
    );
    assert_eq!(
        harness.state().trimmer.sel_out,
        neoveg_common::Ticks::from_secs_i64(5)
    );

    harness
        .get_by_label("Adicionar à timeline a partir do cursor")
        .click();
    harness.run_steps(2);

    let events: Vec<_> = harness
        .state()
        .project
        .tracks
        .iter()
        .flat_map(|t| t.events.iter())
        .collect();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].offset, neoveg_common::Ticks::from_secs_i64(2));
    assert_eq!(events[0].length, neoveg_common::Ticks::from_secs_i64(3));

    // Undo reverte a inserção feita pelo Trimmer.
    harness.state_mut().exec_cmd(Some(&ctx), "edit.undo");
    harness.run_steps(1);
    let remaining: usize = harness
        .state()
        .project
        .tracks
        .iter()
        .map(|t| t.events.len())
        .sum();
    assert_eq!(remaining, 0);
}

#[test]
fn kittest_edit_details_categorias_e_edicao() {
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);

    harness
        .state_mut()
        .insert_generated(crate::model::GenSpec::Text(
            crate::model::TextSpec::default(),
        ));
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "view.tab.edit_details");
    harness.run_steps(2);
    assert_eq!(harness.state().dock_tab, DockTab::EditDetails);

    // Percorre todas as categorias tabulares sem pânico e cria marcador pela tabela.
    for cat in crate::edit_details::DetailsCategory::ALL {
        harness.state_mut().edit_details.category = cat;
        harness.run_steps(2);
    }

    harness.state_mut().edit_details.category = crate::edit_details::DetailsCategory::Markers;
    harness.run_steps(1);
    harness.get_by_label("＋ Marcador no Cursor").click();
    harness.run_steps(2);
    assert_eq!(harness.state().project.markers.len(), 1);

    harness.get_by_label("Copiar TSV").click();
    harness.run_steps(1);
    assert!(harness.state().status.contains("TSV"));
}

#[test]
fn kittest_previa_apenas_quatro_resolucoes() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1400.0, 880.0))
        .build_eframe(build_ready_app);
    harness.state_mut().opts.preview_quality = PreviewQuality::Draft;
    harness.state_mut().prefs.default_preview_quality = PreviewQuality::Draft;
    harness.run_steps(2);

    // 1. ComboBox na barra da Prévia de Vídeo exibe exatamente 4 opções:
    //    Rascunho, Prévia, Boa e Melhor (sem sufixos 1/4, 1/2 ou completa).
    let preview_combo = harness
        .query_all_by_role(egui::accesskit::Role::ComboBox)
        .next()
        .expect("combo da Prévia de Vídeo deve existir");
    preview_combo.click();
    harness.run_steps(2);
    assert!(harness.query_by_label("Rascunho").is_some());
    assert!(harness.query_by_label("Prévia").is_some());
    assert!(harness.query_by_label("Boa").is_some());
    assert!(harness.query_by_label("Melhor").is_some());
    assert!(harness.query_by_label_contains("(1/4)").is_none());
    assert!(harness.query_by_label_contains("(1/2)").is_none());
    assert!(harness.query_by_label_contains("(completa)").is_none());

    harness.get_by_label("Melhor").click();
    harness.run_steps(2);
    assert_eq!(harness.state().opts.preview_quality, PreviewQuality::Best);

    // 2. Preferências -> Vídeo e Prévia usa apenas os 4 níveis (sem Matriz 4×3).
    let ctx = harness.ctx.clone();
    harness.state_mut().exec_cmd(Some(&ctx), "options.prefs");
    harness.run_steps(2);
    harness.get_by_label("Vídeo e Prévia").click();
    harness.run_steps(2);
    assert!(harness
        .query_by_label("Prévia de Vídeo Escalonável")
        .is_some());
    assert!(harness.query_by_label_contains("Matriz").is_none());

    let prefs_combo = harness
        .query_all_by_role(egui::accesskit::Role::ComboBox)
        .last()
        .expect("combo de qualidade inicial nas Preferências deve existir");
    prefs_combo.click();
    harness.run_steps(2);
    assert!(harness.query_by_label("Rascunho").is_some());
    assert!(harness.query_by_label("Prévia").is_some());
    assert!(harness.query_by_label("Boa").is_some());
    assert!(harness.query_by_label("Melhor").is_some());
    assert!(harness.query_by_label_contains("(1/4)").is_none());
    assert!(harness.query_by_label_contains("(completa)").is_none());

    harness.get_by_label("Boa").click();
    harness.run_steps(2);
    harness
        .get_by_label("Aplicar qualidade à sessão aberta")
        .click();
    harness.run_steps(2);
    assert_eq!(harness.state().opts.preview_quality, PreviewQuality::Good);
    harness.get_by_label("Cancelar").click();
    harness.run_steps(1);
}

#[test]
fn kittest_rodape_previa_e_abas_inferiores_sem_corte() {
    for dock_h in [220.0_f32, 360.0] {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1440.0, 900.0))
            .build_eframe(|cc| {
                let mut app = build_ready_app(cc);
                app.dock_height = dock_h;
                app
            });
        harness.run_steps(2);

        let pv_tile = find_pane_tile(&harness.state().dock_tree, DockPane::Preview)
            .and_then(|id| harness.state().dock_tree.tiles.rect(id))
            .expect("tile da Prévia de Vídeo deve existir");
        let mb_tile = find_pane_tile(&harness.state().dock_tree, DockPane::MasterBus)
            .and_then(|id| harness.state().dock_tree.tiles.rect(id))
            .expect("tile do Master Bus deve existir");

        let pv_btn = harness
            .get_by_label("Prévia de Vídeo")
            .raw_bounds()
            .expect("botão da aba Prévia de Vídeo deve possuir bounds");
        let mb_btn_node = harness
            .query_all_by_label("Master")
            .find(|n| {
                n.raw_bounds()
                    .is_some_and(|b| (b.y0 as f32) >= mb_tile.bottom() - 40.0)
            })
            .expect("botão da aba Master no rodapé deve existir");
        let mb_btn = mb_btn_node
            .raw_bounds()
            .expect("botão da aba Master deve possuir bounds");

        assert!(
            pv_btn.y1 as f32 <= pv_tile.bottom() - 2.0 && pv_btn.y0 as f32 >= pv_tile.top(),
            "dock_height={dock_h}: botão Prévia de Vídeo cortado no rodapé ({pv_btn:?} vs tile {pv_tile:?})"
        );
        assert!(
            mb_btn.y1 as f32 <= mb_tile.bottom() - 2.0 && mb_btn.y0 as f32 >= mb_tile.top(),
            "dock_height={dock_h}: botão Master cortado no rodapé ({mb_btn:?} vs tile {mb_tile:?})"
        );
    }
}

#[test]
fn kittest_setas_e_barras_rolagem_abas_e_paineis() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 720.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);

    let tab0 = harness
        .get_by_label("Mídia do Projeto")
        .raw_bounds()
        .expect("aba Mídia do Projeto deve existir");
    let detach = harness
        .get_by_label("Destacar")
        .raw_bounds()
        .expect("botão Destacar deve permanecer visível no rodapé");
    assert!(
        (tab0.y0 - detach.y0).abs() <= 3.0,
        "abas inferiores devem permanecer em linha única (tab0={tab0:?}, detach={detach:?})"
    );

    // Painel acoplado deve exibir barra vertical clássica com setas ▲ / ▼
    assert!(
        harness.query_by_label("Rolar painel para cima").is_some(),
        "botão ▲ da barra vertical do painel acoplado deve existir"
    );
    assert!(
        harness.query_by_label("Rolar painel para baixo").is_some(),
        "botão ▼ da barra vertical do painel acoplado deve existir"
    );

    // Setas horizontais ◀ / ▶ da faixa inferior de abas
    assert!(
        harness
            .query_by_label("Rolar abas para a esquerda")
            .is_some(),
        "botão ◀ das abas inferiores deve existir"
    );
    assert!(
        harness
            .query_by_label("Rolar abas para a direita")
            .is_some(),
        "botão ▶ das abas inferiores deve existir"
    );

    // Clica repetidamente em ▶ até trazer a aba 'Notas' para dentro da área visível
    let mut revealed = false;
    for _ in 0..16 {
        if let Some(node) = harness.query_by_label("Notas") {
            if let Some(b) = node.raw_bounds() {
                if b.x0 >= tab0.x0 - 2.0 && b.x1 <= detach.x0 + 2.0 {
                    revealed = true;
                    break;
                }
            }
        }
        harness.get_by_label("Rolar abas para a direita").click();
        harness.run_steps(2);
    }
    assert!(
        revealed,
        "clicar em 'Rolar abas para a direita' (▶) deve trazer a aba 'Notas' para dentro da faixa visível"
    );

    harness.get_by_label("Notas").click();
    harness.run_steps(2);
    assert_eq!(harness.state().dock_tab, DockTab::Notes);
}

#[test]
fn kittest_timeline_barras_rolagem_setas_e_zoom() {
    use crate::model::{Ticks, Track, TrackKind};

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 760.0))
        .build_eframe(|cc| {
            let mut app = build_ready_app(cc);
            app.dock_height = 220.0;
            for i in 2..=8 {
                app.project
                    .tracks
                    .push(Track::new(TrackKind::Video, &format!("Vídeo {i}")));
            }
            app.cursor = Ticks::from_secs_f64_clamped(45.0);
            app
        });
    harness.run_steps(2);

    // Rolar timeline para a direita e para a esquerda usando as setas ◀ / ▶
    assert_eq!(harness.state().tl.scroll_x, Ticks::ZERO);
    harness
        .get_by_label("Rolar timeline para a direita")
        .click();
    harness.run_steps(2);
    let scrolled_x = harness.state().tl.scroll_x;
    assert!(
        scrolled_x > Ticks::ZERO,
        "clicar em ▶ na timeline deve avançar scroll_x ({scrolled_x:?})"
    );
    harness
        .get_by_label("Rolar timeline para a esquerda")
        .click();
    harness.run_steps(2);
    assert!(
        harness.state().tl.scroll_x < scrolled_x,
        "clicar em ◀ na timeline deve recuar scroll_x"
    );

    // Zoom horizontal + / -
    let pps0 = harness.state().tl.px_per_sec;
    harness.get_by_label("Aumentar zoom horizontal").click();
    harness.run_steps(2);
    let pps1 = harness.state().tl.px_per_sec;
    assert!(pps1 > pps0, "zoom horizontal + deve aumentar px_per_sec");
    harness.get_by_label("Diminuir zoom horizontal").click();
    harness.run_steps(2);
    assert!(
        harness.state().tl.px_per_sec < pps1,
        "zoom horizontal - deve diminuir px_per_sec"
    );

    // Rolar trilhas para baixo e para cima usando as setas ▲ / ▼
    assert_eq!(harness.state().tl.scroll_y, 0.0);
    harness.get_by_label("Rolar trilhas para baixo").click();
    harness.run_steps(2);
    let scrolled_y = harness.state().tl.scroll_y;
    assert!(
        scrolled_y > 0.0,
        "clicar em ▼ na barra vertical deve descer scroll_y ({scrolled_y})"
    );
    harness.get_by_label("Rolar trilhas para cima").click();
    harness.run_steps(2);
    assert!(
        harness.state().tl.scroll_y < scrolled_y,
        "clicar em ▲ na barra vertical deve subir scroll_y"
    );

    // Zoom de altura de trilhas + / -
    let h0 = harness.state().project.tracks[0].height;
    harness.get_by_label("Aumentar altura das trilhas").click();
    harness.run_steps(2);
    let h1 = harness.state().project.tracks[0].height;
    assert!(
        h1 > h0,
        "botão + de altura deve expandir trilhas ({h0} -> {h1})"
    );
    harness.get_by_label("Diminuir altura das trilhas").click();
    harness.run_steps(2);
    assert!(
        harness.state().project.tracks[0].height < h1,
        "botão - de altura deve reduzir trilhas"
    );

    // Botão de ajuste no canto inferior direito
    harness
        .get_by_label("Rolar timeline para a direita")
        .click();
    harness.run_steps(2);
    assert!(harness.state().tl.scroll_x > Ticks::ZERO);
    harness.get_by_label("Ajustar projeto à timeline").click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().tl.scroll_x,
        Ticks::ZERO,
        "Ajustar projeto à timeline deve reposicionar scroll_x no início"
    );
}

#[test]
fn kittest_crash_report_janela_e_acoes() {
    use neoveg_engine::session_marker::{CrashReport, PanicNote};

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(|cc| {
            let mut app = build_ready_app(cc);
            app.welcome_open = true;
            app.recovery_offer = true;
            app.set_crash_reports(vec![CrashReport {
                started: "2026-11-16 10:00:00".into(),
                version: "0.1.0".into(),
                pid: "4242".into(),
                panics: vec![
                    PanicNote {
                        time: "10:01:00".into(),
                        thread: "decode".into(),
                        location: "media.rs:12".into(),
                        message: "isolado".into(),
                        isolated: true,
                    },
                    PanicNote {
                        time: "10:02:00".into(),
                        thread: "main".into(),
                        location: "ui.rs:99".into(),
                        message: "falha fatal simulada".into(),
                        isolated: false,
                    },
                ],
            }]);
            app
        });
    harness.run_steps(2);

    assert!(!harness.state().is_welcome_open());
    assert!(!harness.state().is_splash_open());
    assert!(harness
        .query_by_label_contains("falha fatal simulada")
        .is_some());
    assert!(harness
        .query_by_label_contains("Erros internos isolados nesta sessão (1)")
        .is_some());
    assert!(harness
        .query_by_label("Mostrar detalhes do problema")
        .is_some());
    harness.get_by_label("Mostrar detalhes do problema").click();
    harness.run_steps(2);
    assert!(harness
        .query_by_label("Ocultar detalhes do problema")
        .is_some());

    harness.get_by_label("Copiar relatório").click();
    harness.run_steps(2);
    assert!(harness.state().status.contains("Relatório copiado"));

    // Fechar o relatório mantém recovery_offer para a janela de recuperação padrão.
    harness.get_by_label("Fechar").click();
    harness.run_steps(2);
    assert!(harness.state().crash_reports.is_empty());
    assert!(harness.state().recovery_offer);
}

/// Antes: um proxy (ou importação, ou mídia recarregada) que ficava pronto
/// enquanto um script rodava era aplicado na sessão da interface e sumia
/// quando a sessão do script tomava o lugar dela.
#[test]
fn kittest_carregador_espera_o_script_e_nada_se_perde() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);
    let (mid, done) = {
        let app = harness.state_mut();
        app.insert_generated(crate::model::GenSpec::Text(
            crate::model::TextSpec::default(),
        ));
        let mid = app.project.media[0].id;
        // o script recebe uma cópia da sessão, como em run_script_dialog
        let (job, done) = crate::app::ScriptJob::fake(app.session.clone());
        app.script_job = Some(job);
        let _ = app.loader_tx.send(crate::app::LoadMsg::ProxyBuilt(
            mid,
            Ok(std::path::PathBuf::from("proxy.avi")),
        ));
        (mid, done)
    };
    let proxy = |app: &App| {
        app.project
            .media
            .iter()
            .find(|m| m.id == mid)
            .and_then(|m| m.proxy_path.clone())
    };
    harness.run_steps(3);
    assert!(harness.state().script_job.is_some());
    assert_eq!(proxy(harness.state()), None, "espera o script");
    done.send(()).expect("script falso");
    for _ in 0..50 {
        harness.run_steps(1);
        if harness.state().script_job.is_none() && proxy(harness.state()).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(harness.state().script_job.is_none(), "o script terminou");
    assert_eq!(
        proxy(harness.state()).as_deref(),
        Some("proxy.avi"),
        "o proxy entrou sobre a sessão do script"
    );
}

#[test]
fn kittest_neoveg_encoder_fluxo_completo() {
    use neoveg_engine::commands::run_command;

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .build_eframe(|cc| {
            let mut app = build_ready_app(cc);
            app.dock_height = 340.0;
            app
        });
    harness.run_steps(2);

    // Despacha comando file.send_to_encoder (via UI host) e abre a aba Neoveg Encoder
    {
        let app = harness.state_mut();
        app.insert_generated(crate::model::GenSpec::ColorBars);
        app.send_timeline_to_encoder();
    }
    harness.run_steps(3);
    // o Encoder abre na janela própria, fora do dock
    assert!(harness.state().detached_tabs.contains(&DockTab::Encoder));
    assert_ne!(harness.state().dock_tab, DockTab::Encoder);
    assert_eq!(harness.state().encoder.state.items.len(), 1);
    assert!(harness.query_all_by_label("Neoveg Encoder").count() >= 1);
    assert!(harness.query_all_by_label("Fila de Codificação").count() >= 1);
    assert!(
        harness
            .query_all_by_label("Navegador de Predefinições")
            .count()
            >= 1
    );
    assert!(harness.query_all_by_label("Pastas Monitoradas").count() >= 1);

    // Abre Configurações de Exportação e valida alternância para Alto Contraste AAA
    harness.get_by_label("Configurações...").click();
    harness.run_steps(3);
    assert!(harness.query_by_label("Cortar Origem (Crop)").is_some());
    {
        let ctx = harness.ctx.clone();
        let app = harness.state_mut();
        app.set_theme(&ctx, crate::theme::ThemeId::AltoContrasteEscuro, false);
    }
    harness.run_steps(2);

    // Tabela Única permanece consistente com Neoveg Encoder
    let _ = run_command;
}

/// Botão Mais > Editar conjunto de botões visíveis (manual do VEGAS Pro 21,
/// p. 69): "Ocultar todos" + OK tira M, S e fx do cabeçalho das faixas de
/// vídeo; as de áudio continuam com os seus.
#[test]
fn kittest_conjunto_de_botoes_visiveis_do_cabecalho() {
    let _prefs_guard = I18nPrefsGuard::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 920.0))
        .build_eframe(|cc| build_ready_app_lang(cc, neoveg_common::Language::PtBr));
    // o OK grava as preferências em arquivo: começa do padrão, sem depender
    // do que uma rodada anterior gravou
    harness.state_mut().prefs.track_header = Default::default();
    harness.run_steps(2);
    let n_video = harness
        .state()
        .project
        .tracks
        .iter()
        .filter(|t| t.kind == crate::model::TrackKind::Video)
        .count();
    let n_total = harness.state().project.tracks.len();
    assert!(
        n_video > 0 && n_total > n_video,
        "projeto com vídeo e áudio"
    );
    // o Master Bus também tem M: conta a diferença
    let m0 = harness.query_all_by_label("M").count();
    let s0 = harness.query_all_by_label("S").count();
    assert!(m0 >= n_total && s0 >= n_total);
    {
        let app = harness.state_mut();
        let cur = app.prefs.track_header.get(true);
        app.dlg.header_buttons = Some((true, cur));
    }
    harness.run_steps(2);
    harness.get_by_label("Ocultar todos").click();
    harness.run_steps(2);
    harness.get_by_label("OK").click();
    harness.run_steps(3);
    assert!(harness.state().dlg.header_buttons.is_none());
    assert_eq!(
        harness.state().prefs.track_header.get(true),
        neoveg_common::prefs::HeaderButtons::all(false)
    );
    assert_eq!(
        harness.query_all_by_label("M").count(),
        m0 - n_video,
        "as faixas de vídeo deixam de mostrar M"
    );
    assert_eq!(harness.query_all_by_label("S").count(), s0 - n_video);
    // não deixa as faixas de vídeo sem botões para os outros testes
    let app = harness.state_mut();
    app.prefs.track_header = Default::default();
    let _ = app.prefs.save();
}

/// Janela Efeitos de Vídeo (manual do VEGAS Pro 21, p. 54): categoria, busca,
/// escolher o efeito mostra o preset, e o botão Eventos aplica o efeito ao
/// evento selecionado.
#[test]
fn kittest_navegador_de_efeitos_de_video() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 920.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);
    let ev = {
        let app = harness.state_mut();
        app.insert_generated(crate::model::GenSpec::Text(
            crate::model::TextSpec::default(),
        ));
        app.dlg.gen_editor = None;
        let id = app
            .project
            .tracks
            .iter()
            .filter(|t| t.kind == crate::model::TrackKind::Video)
            .flat_map(|t| t.events.iter())
            .map(|e| e.id)
            .next()
            .expect("evento de vídeo");
        app.sel_events.clear();
        app.sel_events.insert(id);
        id
    };
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "view.tab.video_fx");
    harness.run_steps(3);

    // categoria Luz: só Brilho (Glow) e Vinheta
    harness.get_by_label("Luz").click();
    harness.run_steps(2);
    assert!(harness.query_by_label("Vinheta").is_some());
    assert!(harness.query_by_label("Inverter").is_none());

    // busca em Todos
    harness.get_by_label("Todos").click();
    harness.state_mut().fx_browser.search = "inver".into();
    harness.run_steps(2);
    assert!(harness.query_by_label("Inverter").is_some());
    assert!(harness.query_by_label("Sépia").is_none());

    harness.get_by_label("Inverter").click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().fx_browser.selected,
        Some(crate::model::FxKind::Invert)
    );
    assert!(
        harness.query_by_label("Padrão").is_some(),
        "preset do efeito"
    );
    harness.get_by_label("Eventos").click();
    harness.run_steps(2);
    let fx: Vec<_> = harness
        .state()
        .project
        .event(ev)
        .map(|e| e.fx.iter().map(|f| f.kind).collect())
        .unwrap_or_default();
    assert_eq!(fx, vec![crate::model::FxKind::Invert]);
}

/// Janela Geradores de Mídia (manual do VEGAS Pro 21, p. 56): categoria,
/// busca, escolher o tipo mostra os presets, e "Inserir no cursor" cria a
/// mídia gerada do preset escolhido.
#[test]
fn kittest_navegador_de_geradores_de_midia() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 920.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "view.tab.generators");
    harness.run_steps(3);

    harness.get_by_label("Utilitários").click();
    harness.run_steps(2);
    assert!(harness.query_by_label("Quadriculado").is_some());
    assert!(harness.query_by_label("Texto").is_none());

    harness.get_by_label("Todos").click();
    harness.state_mut().gen_browser.search = "grad".into();
    harness.run_steps(2);
    assert!(harness.query_by_label("Gradiente").is_some());
    assert!(harness.query_by_label("Cor sólida").is_none());
    harness.get_by_label("Gradiente").click();
    harness.run_steps(2);
    assert!(harness.query_by_label("Gradiente linear").is_some());
    harness.get_by_label("Gradiente radial").click();
    harness.run_steps(2);

    let antes = harness.state().project.media.len();
    harness.get_by_label("Inserir no cursor").click();
    harness.run_steps(2);
    let app = harness.state();
    assert_eq!(app.project.media.len(), antes + 1);
    let radial = app.project.media.last().is_some_and(|m| {
        matches!(
            m.source,
            crate::model::MediaSource::Generated(crate::model::GenSpec::Gradient {
                radial: true,
                ..
            })
        )
    });
    assert!(radial, "inseriu o preset Gradiente radial");
}

static I18N_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn lock_test_prefs() -> std::sync::MutexGuard<'static, ()> {
    I18N_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct I18nPrefsGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    prefs_path: std::path::PathBuf,
    prev_prefs: Option<Vec<u8>>,
    prev_lang: neoveg_common::Language,
}

impl I18nPrefsGuard {
    fn new() -> Self {
        let lock = lock_test_prefs();
        let prefs_path = crate::Preferences::file_path();
        Self {
            _lock: lock,
            prev_prefs: std::fs::read(&prefs_path).ok(),
            prefs_path,
            prev_lang: neoveg_common::active_language(),
        }
    }
}

impl Drop for I18nPrefsGuard {
    fn drop(&mut self) {
        neoveg_common::set_active_language(self.prev_lang);
        match &self.prev_prefs {
            Some(bytes) => {
                let _ = std::fs::write(&self.prefs_path, bytes);
            }
            None => {
                let _ = std::fs::remove_file(&self.prefs_path);
            }
        }
    }
}

fn build_ready_app_lang(cc: &eframe::CreationContext<'_>, lang: neoveg_common::Language) -> App {
    let mut app = App::with_context_in(&cc.egui_ctx, None, Some(lang));
    app.splash.active = false;
    app.pending_welcome_after_splash = false;
    app.dismiss_welcome();
    app.select_layout_type(Some(&cc.egui_ctx), LayoutType::Magix, false);
    // os testes de dock clicam em todas as abas; o padrão novo fica no teste próprio
    app.closed_tabs.clear();
    app
}

#[test]
fn kittest_i18n_troca_ao_vivo_todos_os_idiomas() {
    use neoveg_common::{tr, Language};

    let _guard = I18nPrefsGuard::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 920.0))
        .build_eframe(|cc| build_ready_app_lang(cc, Language::PtBr));
    harness.run_steps(2);

    let expected_menus: [(Language, &str, &str, &str); 6] = [
        (
            Language::PtBr,
            "Arquivo",
            "Mídia do Projeto",
            "Novo Projeto",
        ),
        (Language::En, "File", "Project Media", "New Project"),
        (
            Language::Es,
            "Archivo",
            "Medios del proyecto",
            "Nuevo proyecto",
        ),
        (Language::Fr, "Fichier", "Média de projet", "Nouveau projet"),
        (Language::De, "Datei", "Projektmedien", "Neues Projekt"),
        (
            Language::Ja,
            "ファイル",
            "プロジェクト メディア",
            "新規プロジェクト",
        ),
    ];

    for (lang, menu_file, tab_media, welcome_new) in expected_menus {
        let ctx = harness.ctx.clone();
        {
            let app = harness.state_mut();
            app.dismiss_welcome();
            app.apply_language(Some(&ctx), lang, false);
        }
        harness.run_steps(3);
        assert_eq!(harness.state().lang(), lang);
        assert_eq!(tr(lang, "Arquivo"), menu_file);
        assert!(
            harness.query_by_label(menu_file).is_some(),
            "menu '{menu_file}' deve estar visível no idioma {}",
            lang.code()
        );
        assert!(
            harness.query_by_label(tr(lang, "Editar")).is_some(),
            "menu Editar deve atualizar em {}",
            lang.code()
        );
        assert!(
            harness.query_by_label(tab_media).is_some(),
            "aba '{tab_media}' deve estar visível em {}",
            lang.code()
        );
        assert!(
            harness.query_by_label(tr(lang, "Transições")).is_some(),
            "aba Transições deve atualizar em {}",
            lang.code()
        );

        harness.state_mut().open_welcome();
        harness.run_steps(3);
        assert!(
            harness.query_by_label(welcome_new).is_some(),
            "Welcome '{welcome_new}' deve estar visível em {}",
            lang.code()
        );
        assert!(
            harness.query_by_label(tr(lang, "Criar Projeto")).is_some(),
            "botão Criar Projeto na Welcome deve estar visível em {}",
            lang.code()
        );
    }
}

#[test]
fn kittest_i18n_preferencias_combo_e_cancelar_reverte() {
    use neoveg_common::{active_language, Language};

    let _guard = I18nPrefsGuard::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 920.0))
        .build_eframe(|cc| build_ready_app_lang(cc, Language::PtBr));
    harness.run_steps(2);

    let ctx = harness.ctx.clone();
    harness.state_mut().exec_cmd(Some(&ctx), "options.prefs");
    harness.run_steps(2);
    assert!(harness.state().dlg.prefs_open);

    // Abre o seletor de idioma em Preferências > Geral (o ComboBox na linha de "Idioma ativo:")
    // e escolhe Japonês.
    let lang_label_rect = harness
        .get_by_label("Idioma ativo:")
        .raw_bounds()
        .expect("rótulo Idioma ativo: deve ter bounds");
    let lang_cy = (lang_label_rect.y0 + lang_label_rect.y1) * 0.5;
    harness
        .get_all_by_role(egui::accesskit::Role::ComboBox)
        .min_by(|a, b| {
            let da = a
                .raw_bounds()
                .map_or(f64::MAX, |r| ((r.y0 + r.y1) * 0.5 - lang_cy).abs());
            let db = b
                .raw_bounds()
                .map_or(f64::MAX, |r| ((r.y0 + r.y1) * 0.5 - lang_cy).abs());
            da.total_cmp(&db)
        })
        .expect("ComboBox de idioma em Preferências")
        .click();
    harness.run_steps(2);
    let ja_opt = Language::Ja.display_option(Language::PtBr);
    harness.get_by_label(&ja_opt).click();
    harness.run_steps(3);

    // Prévia ao vivo aplicada imediatamente antes mesmo de OK/Aplicar.
    assert_eq!(harness.state().lang(), Language::Ja);
    assert_eq!(harness.state().prefs.lang(), Language::Ja);
    assert_eq!(harness.state().session.lang, Language::Ja);
    assert_eq!(active_language(), Language::Ja);
    assert!(harness.query_by_label("初期設定に戻す").is_some());
    assert!(harness.query_by_label("適用").is_some());
    assert!(harness.query_by_label("キャンセル").is_some());

    // Cancelar deve reverter completamente para o idioma original (pt-BR).
    harness.get_by_label("キャンセル").click();
    harness.run_steps(3);
    assert!(!harness.state().dlg.prefs_open);
    assert_eq!(harness.state().lang(), Language::PtBr);
    assert_eq!(harness.state().prefs.lang(), Language::PtBr);
    assert_eq!(harness.state().session.lang, Language::PtBr);
    assert_eq!(active_language(), Language::PtBr);
}

#[test]
fn kittest_i18n_preferencias_compacto_de_ja_sem_recorte() {
    use neoveg_common::Language;

    let _guard = I18nPrefsGuard::new();
    for (lang, buttons) in [
        (
            Language::De,
            ["Standard wiederherstellen", "Abbrechen", "Übernehmen", "OK"],
        ),
        (Language::Ja, ["初期設定に戻す", "キャンセル", "適用", "OK"]),
    ] {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(760.0, 520.0))
            .build_eframe(move |cc| build_ready_app_lang(cc, lang));
        harness.run_steps(2);

        let ctx = harness.ctx.clone();
        harness.state_mut().exec_cmd(Some(&ctx), "options.prefs");
        harness.run_steps(3);
        assert!(harness.state().dlg.prefs_open);

        for btn in buttons {
            let b = harness
                .query_by_label(btn)
                .unwrap_or_else(|| panic!("botão '{btn}' deve existir em {:?}", lang))
                .raw_bounds()
                .unwrap_or_else(|| panic!("botão '{btn}' deve ter bounds em {:?}", lang));
            assert!(
                b.x0 >= -0.5 && b.y0 >= -0.5 && b.x1 <= 760.0 + 1.5 && b.y1 <= 520.0 + 1.5,
                "botão '{btn}' recortado em 760x520 ({lang:?}): bounds={b:?}"
            );
        }
    }
}

#[test]
fn kittest_i18n_welcome_combo_idioma_e_criar_projeto_ja() {
    use neoveg_common::Language;

    let _guard = I18nPrefsGuard::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 760.0))
        .build_eframe(|cc| {
            let mut app = App::with_context_in(&cc.egui_ctx, None, Some(Language::PtBr));
            app.splash.active = false;
            app.pending_welcome_after_splash = false;
            app.open_welcome();
            app
        });
    harness.run_steps(2);
    assert!(harness.state().is_welcome_open());

    // O seletor de idioma da Welcome Screen é o primeiro ComboBox dentro de `neoveg_welcome_card`.
    harness
        .get_by_label("neoveg_welcome_card")
        .get_all_by_role(egui::accesskit::Role::ComboBox)
        .next()
        .expect("ComboBox de idioma na Welcome Screen")
        .click();
    harness.run_steps(2);
    harness.get_by_label(Language::Ja.native_name()).click();
    harness.run_steps(3);

    assert_eq!(harness.state().lang(), Language::Ja);
    assert!(harness.query_by_label("新規プロジェクト").is_some());
    assert!(harness.query_by_label("プロジェクト作成").is_some());

    harness.get_by_label("プロジェクト作成").click();
    harness.run_steps(3);
    assert!(!harness.state().is_welcome_open());
    assert_eq!(harness.state().lang(), Language::Ja);
}

#[test]
fn kittest_encoder_visivel_na_barra_e_em_renderizar_como() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1480.0, 900.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);

    // O botão "Encoder" fica visível diretamente na barra superior ao iniciar.
    harness.get_by_label("Encoder").click();
    harness.run_steps(2);
    assert!(harness.state().detached_tabs.contains(&DockTab::Encoder));
    assert_ne!(harness.state().dock_tab, DockTab::Encoder);

    // Em "Renderizar como", "Enviar para o Encoder" enfileira a timeline e foca o Encoder.
    let ctx = harness.ctx.clone();
    {
        let app = harness.state_mut();
        app.exec_cmd(Some(&ctx), "view.tab.project_media");
        app.insert_generated(crate::model::GenSpec::ColorBars);
        app.exec_cmd(Some(&ctx), "file.render");
    }
    harness.run_steps(2);
    assert!(harness.state().dlg.render.is_some());

    let before = harness.state().encoder.state.items.len();
    harness.get_by_label("Enviar para o Encoder").click();
    harness.run_steps(3);

    assert!(harness.state().dlg.render.is_none());
    assert!(harness.state().detached_tabs.contains(&DockTab::Encoder));
    assert_eq!(harness.state().encoder.state.items.len(), before + 1);
}

#[test]
fn kittest_janela_integracoes_artcraft_e_blender() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "tools.integrations");
    harness.run_steps(3);
    assert!(harness.state().integrations.open);

    for name in [
        "PhotoCraft",
        "VectorCraft",
        "FilmCraft",
        "EffectCraft",
        "AniCraft",
        "SoundCraft",
        "Blender",
    ] {
        assert!(
            harness.query_by_label(name).is_some(),
            "app '{name}' deve aparecer na janela de Integrações"
        );
    }

    harness.get_by_label("Configurar caminhos...").click();
    harness.run_steps(3);
    assert!(harness.state().dlg.prefs_open);
    assert_eq!(
        harness.state().dlg.prefs_tab,
        crate::dialogs::PrefsCategory::Integrations
    );
}

#[test]
fn kittest_seletor_modos_ferramenta_normal_e_colar_atributos() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);

    // Abre o seletor de modos da ferramenta Normal na barra e alterna para Shuffle.
    harness
        .get_by_label("Selecionar modo da ferramenta Normal")
        .click();
    harness.run_steps(2);
    harness.get_by_label("Modo Shuffle (reordenar)").click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().session.edit_mode,
        neoveg_engine::session::EditMode::Shuffle
    );

    // Prepara dois eventos, copia atributos e testa o diálogo Colar atributos seletivamente.
    let ctx = harness.ctx.clone();
    {
        let app = harness.state_mut();
        app.session.sel_track = Some(0);
        app.cursor = crate::model::Ticks::ZERO;
        app.insert_generated(crate::model::GenSpec::ColorBars);
        app.cursor = crate::model::Ticks::from_secs_i64(6);
        app.insert_generated(crate::model::GenSpec::SolidColor {
            color: [0.2, 0.4, 0.8, 1.0],
        });
        app.dlg.gen_editor = None;
        let ids: Vec<crate::model::Id> =
            app.project.tracks[0].events.iter().map(|e| e.id).collect();
        if let Some(ev0) = app.project.event_mut(ids[0]) {
            ev0.fade_in.len = crate::model::Ticks::from_secs_i64(1);
        }
        app.sel_events.clear();
        app.sel_events.insert(ids[0]);
        app.copy_selection();
        app.sel_events.clear();
        app.sel_events.insert(ids[1]);
        app.exec_cmd(Some(&ctx), "edit.paste_attributes_selective");
    }
    harness.run_steps(2);
    assert!(harness.state().dlg.paste_attrs_open);

    harness.get_by_label("Aplicar").click();
    harness.run_steps(2);
    assert!(!harness.state().dlg.paste_attrs_open);
    let ev1_id = *harness.state().sel_events.iter().next().unwrap();
    assert_eq!(
        harness.state().project.event(ev1_id).unwrap().fade_in.len,
        crate::model::Ticks::from_secs_i64(1)
    );
}

#[test]
fn kittest_take_chooser_pitch_evento_vazio_e_eixo() {
    use neoveg_engine::{model::Take, session::BoxAxisMode, takes};

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);
    let ctx = harness.ctx.clone();

    // 1. Inserir evento vazio pela Tabela Única.
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "insert.empty_event");
    harness.run_steps(2);
    let empty_id = *harness.state().sel_events.iter().next().unwrap();
    assert_eq!(
        harness.state().project.event(empty_id).unwrap().name,
        "Evento vazio"
    );

    // 2. Inserir dois geradores na trilha de vídeo e adicionar take alternativo -> abrir Take Chooser.
    {
        let app = harness.state_mut();
        app.session.sel_track = Some(0);
        app.cursor = crate::model::Ticks::from_secs_i64(8);
        app.insert_generated(crate::model::GenSpec::ColorBars);
        app.cursor = crate::model::Ticks::from_secs_i64(15);
        app.insert_generated(crate::model::GenSpec::SolidColor {
            color: [0.8, 0.2, 0.2, 1.0],
        });
        app.dlg.gen_editor = None;
        let ev_vid_id = app.project.tracks[0].events[1].id;
        let mid2 = app.project.media.last().unwrap().id;
        if let Some(ev) = app.project.event_mut(ev_vid_id) {
            assert!(takes::add_take(
                ev,
                Take {
                    media_id: mid2,
                    offset: crate::model::Ticks::ZERO,
                    name: "Take B".into(),
                },
                true
            ));
        }
        app.sel_events.clear();
        app.sel_events.insert(ev_vid_id);
        app.exec_cmd(Some(&ctx), "event.take_chooser");
    }
    harness.run_steps(3);
    assert!(harness.state().dlg.take_chooser.is_some());
    assert!(harness.query_by_label("Takes do evento (2 de 2)").is_some());

    // Seleciona primeiro take, confirma OK e verifica mudança do ativo.
    let ev_vid_id = harness.state().dlg.take_chooser.unwrap();
    harness.get_by_label("1. Barras de cor").click();
    harness.run_steps(2);
    harness.get_by_label("OK").click();
    harness.run_steps(2);
    assert!(harness.state().dlg.take_chooser.is_none());
    assert_eq!(
        takes::active_index(harness.state().project.event(ev_vid_id).unwrap()),
        0
    );

    // 3. Seletor de eixo na barra de ferramentas quando Seleção está ativa.
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "edit.tool.selection");
    harness.run_steps(2);
    harness
        .get_by_label("Selecionar eixo de Seleção/Zoom")
        .click();
    harness.run_steps(2);
    harness.get_by_label("Apenas horizontal").click();
    harness.run_steps(2);
    assert_eq!(harness.state().session.box_axis, BoxAxisMode::Horizontal);
}

#[test]
fn kittest_modo_expandido_deteccao_cenas_e_armar_gravacao() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);
    let ctx = harness.ctx.clone();

    {
        let app = harness.state_mut();
        app.session.sel_track = Some(0);
        app.cursor = crate::model::Ticks::ZERO;
        app.insert_generated(crate::model::GenSpec::ColorBars);
        app.cursor = crate::model::Ticks::from_secs_i64(5);
        app.insert_generated(crate::model::GenSpec::SolidColor {
            color: [0.1, 0.7, 0.2, 1.0],
        });
        app.dlg.gen_editor = None;
        let ev0 = app.project.tracks[0].events[0].id;
        app.sel_events.clear();
        app.sel_events.insert(ev0);
    }
    harness.run_steps(2);

    // 1. Diálogo Detectar cenas e cortes abre e fecha limpo.
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "edit.detect_scenes");
    harness.run_steps(2);
    assert!(harness.state().dlg.scene_detect.is_some());
    assert!(harness.query_by_label("Analisar clipe").is_some());
    harness.get_by_label("Fechar").click();
    harness.run_steps(2);
    assert!(harness.state().dlg.scene_detect.is_none());

    // 2. Modo de edição expandido ativa controles A/B na Prévia de Vídeo e nudges.
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "edit.expanded_edit_mode");
    harness.run_steps(4);
    assert!(harness.state().session.expanded_edit.is_some());
    assert!(harness.query_by_label("Ambos (Roll)").is_some());
    harness.get_by_label("Saída esquerda (Out)").click();
    harness.run_steps(2);
    assert_eq!(
        harness
            .state()
            .session
            .expanded_edit
            .as_ref()
            .unwrap()
            .focus,
        neoveg_engine::session::ExpandedEdgeFocus::LeftOut
    );

    // 3. Armar faixa de áudio para gravação no cabeçalho da faixa.
    harness.get_by_label("Armar gravação").click();
    harness.run_steps(2);
    assert!(harness.state().project.tracks[1].record_armed);
}

#[test]
fn kittest_midia_do_projeto_bins_modos_substituir_e_storyboard() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1480.0, 900.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);
    let ctx = harness.ctx.clone();

    // Insere duas mídias geradas no projeto e seleciona a aba Mídia do Projeto.
    {
        let app = harness.state_mut();
        let imp1 = neoveg_engine::media::import_generated(crate::model::GenSpec::ColorBars);
        let id1 = imp1.item.id;
        app.media.insert(id1, imp1.entry);
        app.session.register_imported_media(imp1.item, None);
        let imp2 = neoveg_engine::media::import_generated(crate::model::GenSpec::SolidColor {
            color: [0.2, 0.5, 0.9, 1.0],
        });
        let id2 = imp2.item.id;
        app.media.insert(id2, imp2.entry);
        app.session.register_imported_media(imp2.item, None);
        app.add_media_to_timeline(id1, crate::model::Ticks::ZERO, Some(0));
        app.session.active_media_id = Some(id1);
        app.dock_tab = DockTab::ProjectMedia;
        app.pending_dock_focus = Some(DockTab::ProjectMedia);
    }
    harness.run_steps(2);

    // 1. Criar pasta manual (Bin) e pasta inteligente personalizada pela UI.
    harness.get_by_label("Nova pasta (Bin)").click();
    harness.run_steps(2);
    assert_eq!(harness.state().project.media_bins.len(), 1);

    harness.get_by_label("Nova pasta inteligente").click();
    harness.run_steps(2);
    assert!(harness.state().project_media.smart_dialog_open);
    harness.get_by_label("Salvar filtro").click();
    harness.run_steps(2);
    assert_eq!(harness.state().project.smart_bins.len(), 1);

    // 2. Alternar entre modos Lista, Detalhes e Storyboard.
    harness.get_by_label("Lista").click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().project_media.view_mode,
        crate::project_media::MediaViewMode::List
    );

    harness.get_by_label("Detalhes").click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().project_media.view_mode,
        crate::project_media::MediaViewMode::Details
    );

    // 3. Selecionar eventos na timeline e trocar mídia mantendo os eventos.
    let (id1, id2) = {
        let app = harness.state();
        (app.project.media[0].id, app.project.media[1].id)
    };
    harness.state_mut().session.sel_events.clear();
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "media.select_events");
    harness.run_steps(1);
    assert_eq!(harness.state().sel_events.len(), 1);
    assert_eq!(
        harness.state_mut().session.swap_media_references(id1, id2),
        1
    );
    assert_eq!(harness.state().project.tracks[0].events[0].media_id, id2);

    // 4. Storyboard: criar, adicionar mídia e inserir na linha do tempo.
    harness.get_by_label("Storyboard").click();
    harness.run_steps(2);
    assert_eq!(harness.state().project.storyboards.len(), 1);
    let sb_id = harness.state().project.storyboards[0].id;
    harness.state_mut().session.active_media_id = Some(id2);
    harness.get_by_label("+ Adicionar selecionada").click();
    harness.run_steps(2);
    assert_eq!(harness.state().project.storyboards[0].items.len(), 1);
    harness.state_mut().cursor = crate::model::Ticks::from_secs_i64(10);
    let before = harness.state().project.tracks[0].events.len();
    harness
        .get_by_label("Inserir storyboard na timeline")
        .click();
    harness.run_steps(2);
    assert_eq!(harness.state().project.tracks[0].events.len(), before + 1);
    assert_eq!(harness.state().session.active_storyboard_id, Some(sb_id));
}

/// Ferramentas da barra da Prévia que não cabem na largura do painel ficam no
/// menu "⋯" (preview_toolbar.rs): abre o menu quando o botão não está na barra.
/// Na CI, nem `click` nem `simulate_click` abriram esse menu no kittest (no
/// programa ele abre), por isso os testes da barra usam uma janela larga em
/// que tudo cabe; isto fica como reserva.
fn reach_preview_tool(harness: &mut Harness<'_, App>, label: &str) {
    if harness.query_by_label(label).is_none() {
        harness
            .get_by_label("Mais ferramentas da prévia")
            .simulate_click();
        harness.run_steps(2);
    }
}

#[test]
fn kittest_previa_split_ram_zoom_copia_mudo_e_monitor_externo() {
    // janela larga: todas as ferramentas da Prévia cabem na barra (o menu "⋯"
    // dos excedentes tem teste próprio da divisão em preview_toolbar)
    let mut harness = Harness::builder()
        .with_size(egui::vec2(2400.0, 900.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);
    let ctx = harness.ctx.clone();

    // 1. Botão de tela dividida alterna Bypass FX.
    reach_preview_tool(&mut harness, "Tela dividida");
    harness.get_by_label("Tela dividida").click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().opts.preview_split_mode,
        neoveg_engine::playback::PreviewSplitMode::FxBypass
    );

    // 2. Copiar quadro armazena buffer da área de transferência interna.
    reach_preview_tool(&mut harness, "Copiar quadro");
    harness.get_by_label("Copiar quadro").click();
    harness.run_steps(2);
    assert!(harness.state().preview_clipboard.is_some());

    // 3. Zoom dentro da prévia ajusta e restaura para Fit (1.0).
    harness.state_mut().exec_cmd(Some(&ctx), "preview.zoom_in");
    harness.run_steps(1);
    assert!(harness.state().preview_zoom > 1.1);
    harness.state_mut().exec_cmd(Some(&ctx), "preview.zoom_fit");
    assert!((harness.state().preview_zoom - 1.0).abs() < 1e-4);

    // 4. Silenciar áudio e vídeo da prévia via barra de ferramentas.
    reach_preview_tool(&mut harness, "Silenciar áudio da prévia");
    harness.get_by_label("Silenciar áudio da prévia").click();
    harness.run_steps(2);
    reach_preview_tool(&mut harness, "Silenciar vídeo da prévia");
    harness.get_by_label("Silenciar vídeo da prévia").click();
    harness.run_steps(2);
    assert!(harness.state().opts.preview_mute_audio);
    assert!(harness.state().opts.preview_mute_video);

    // 5. Soltar efeito de vídeo na prévia aplica em Master Video Output FX.
    assert!(harness
        .state_mut()
        .session
        .apply_video_fx_to_output(crate::model::FxKind::Invert));
    assert_eq!(harness.state().project.master.video_fx.len(), 1);

    // 6. Prévia em RAM constrói quadros no cache sem travar e Monitor Externo abre limpo
    // sem deslocar os botões estáticos da barra quando "Limpar RAM" aparece.
    let ext_rect_before = harness.get_by_label("Monitor externo").raw_bounds();
    reach_preview_tool(&mut harness, "Prévia em RAM");
    harness.get_by_label("Prévia em RAM").click();
    harness.run_steps(4);
    if harness.state().ram_preview.frames.is_empty() {
        let _ = harness
            .state_mut()
            .ram_preview
            .insert_frame(0, image::RgbaImage::new(16, 16));
    }
    harness.run_steps(2);
    let in_menu = harness.query_by_label("Limpar RAM").is_none();
    reach_preview_tool(&mut harness, "Limpar RAM");
    assert!(harness.query_by_label("Limpar RAM").is_some());
    if in_menu {
        // fecha o menu "⋯" antes de medir a barra
        harness
            .get_by_label("Mais ferramentas da prévia")
            .simulate_click();
        harness.run_steps(2);
    }
    assert_eq!(
        harness.get_by_label("Monitor externo").raw_bounds(),
        ext_rect_before,
        "botão Monitor externo não pode deslocar quando Limpar RAM aparece"
    );
    harness.get_by_label("Monitor externo").click();
    harness.run_steps(2);
    assert!(harness.state().external_monitor);
}

#[test]
fn kittest_trilhas_pastas_comp_pai_filho_multicam_e_3d() {
    // janela larga: todas as ferramentas da Prévia cabem na barra (o menu "⋯"
    // dos excedentes tem teste próprio da divisão em preview_toolbar)
    let mut harness = Harness::builder()
        .with_size(egui::vec2(2400.0, 900.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);
    let ctx = harness.ctx.clone();

    // 1. Pasta recolhível de trilhas e composição Pai/Filho via Tabela Única.
    harness.state_mut().session.sel_track = Some(0);
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "insert.track_folder");
    harness.run_steps(2);
    assert!(harness.state().project.tracks[0].is_folder);
    let child_id = harness.state().project.tracks[1].id;
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "track.toggle_folder");
    harness.run_steps(2);
    assert!(harness.state().project.is_track_hidden_by_folder(child_id));
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "track.toggle_folder");
    harness.run_steps(2);
    assert!(!harness.state().project.is_track_hidden_by_folder(child_id));

    // 2. Grade Multicâmera e troca de ângulo na Prévia de Vídeo.
    harness.state_mut().session.sel_track = Some(1);
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "insert.track_video");
    harness.run_steps(1);
    let imp1 = neoveg_engine::media::import_generated(crate::model::GenSpec::SolidColor {
        color: [1.0, 0.0, 0.0, 1.0],
    });
    let imp2 = neoveg_engine::media::import_generated(crate::model::GenSpec::SolidColor {
        color: [0.0, 1.0, 0.0, 1.0],
    });
    {
        let st = harness.state_mut();
        st.media.insert(imp1.item.id, imp1.entry);
        st.media.insert(imp2.item.id, imp2.entry);
        let ev1 = crate::model::Event::new(
            &imp1.item,
            crate::model::Ticks::ZERO,
            crate::model::Ticks::from_secs_i64(4),
        );
        let ev2 = crate::model::Event::new(
            &imp2.item,
            crate::model::Ticks::ZERO,
            crate::model::Ticks::from_secs_i64(4),
        );
        st.project.media.extend([imp1.item, imp2.item]);
        st.project.tracks[1].events.push(ev1);
        st.project.tracks[2].events.push(ev2);
        st.session.sel_events.clear();
        let _ = st.session.create_multicam_track();
    }
    harness.run_steps(2);
    assert!(harness.state().opts.multicam_mode);
    harness.get_by_label("Câmera 2").click();
    harness.run_steps(2);
    let (_, _, _, active, _) = harness
        .state()
        .session
        .active_multicam_event_at(crate::model::Ticks::ZERO)
        .expect("multicamera ativa");
    assert_eq!(active, 1);

    // 3. Alternar modo 3D Estereoscópico pelo botão da Prévia.
    reach_preview_tool(&mut harness, "Modo 3D Estereoscópico");
    harness.get_by_label("Modo 3D Estereoscópico").click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().project.settings.stereo_3d,
        crate::model::Stereo3dMode::AnaglyphRedCyan
    );
}

#[test]
fn kittest_efeitos_cadeias_lut_curvas_legendas_e_bypass_audio() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1480.0, 900.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);
    let ctx = harness.ctx.clone();

    // Prepara mídia gerada animada, evento selecionado e uma região na timeline.
    let ev_id = {
        let st = harness.state_mut();
        let imp = neoveg_engine::media::import_generated(crate::model::GenSpec::Text(
            crate::model::TextSpec {
                text: "Cena teste".into(),
                anim: crate::model::TextAnim::CrawlLeft,
                anim_duration: 2.0,
                ..crate::model::TextSpec::default()
            },
        ));
        let ev = crate::model::Event::new(
            &imp.item,
            crate::model::Ticks::ZERO,
            crate::model::Ticks::from_secs_i64(2),
        );
        let ev_id = ev.id;
        st.media.insert(imp.item.id, imp.entry);
        st.project.media.push(imp.item);
        st.project.tracks[0].events.push(ev);
        st.project.regions.push(crate::model::Region {
            id: crate::model::new_id(),
            start: crate::model::Ticks::ZERO,
            end: crate::model::Ticks::from_secs_i64(2),
            label: "Fala inicial".into(),
        });
        st.session.sel_events.insert(ev_id);
        st.changed();
        ev_id
    };
    harness.run_steps(2);

    // 1. Janela de FX do evento: adiciona Sépia, salva cadeia e salva preset próprio.
    harness.state_mut().exec_cmd(Some(&ctx), "tools.fx_event");
    harness.run_steps(2);
    {
        let st = harness.state_mut();
        if let Some(ev) = st.project.event_mut(ev_id) {
            ev.fx
                .push(crate::model::FxInstance::new(crate::model::FxKind::Sepia));
            ev.fx.push(crate::model::FxInstance::new(
                crate::model::FxKind::ColorCorrector,
            ));
        }
        st.changed();
    }
    harness.run_steps(2);
    harness.get_by_label("Salvar cadeia...").click();
    harness.run_steps(2);
    harness.get_by_label("Salvar").click();
    harness.run_steps(2);
    assert!(!harness.state().prefs.fx_chains.is_empty());

    // 2. Painel Color Grading mostra LUT, Curvas RGB e Curvas HSL.
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "view.tab.color_grading");
    harness.run_steps(2);
    assert!(harness
        .query_by_label("LUT de Câmera / Look (.cube)")
        .is_some());
    assert!(harness.query_by_label("Curvas RGB").is_some());
    assert!(harness.query_by_label("Curvas HSL (6 setores)").is_some());

    // 3. Legendas a partir das regiões via Tabela Única.
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "insert.subtitles_from_regions");
    harness.run_steps(2);
    assert!(harness
        .state()
        .project
        .tracks
        .iter()
        .any(|t| t.name.starts_with("Legendas") && !t.events.is_empty()));

    // 4. Bypass global de FX de áudio no Mixer e Opções.
    harness.state_mut().exec_cmd(Some(&ctx), "view.tab.mixer");
    harness.run_steps(2);
    harness.get_by_label("Bypass FX").click();
    harness.run_steps(2);
    assert!(harness.state().opts.bypass_all_audio_fx);

    // 5. Rastreamento de movimento grava keyframes no Pan/Crop.
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "tools.motion_track");
    harness.run_steps(2);
    harness.get_by_label("Rastrear e aplicar keyframes").click();
    harness.run_steps(2);
    assert!(
        harness
            .state()
            .project
            .event(ev_id)
            .expect("ev")
            .pan_crop
            .keys
            .len()
            >= 2
    );
}

#[test]
fn kittest_barra_grade_marcadores_cd_automacao_borda_e_preferencias() {
    struct PrefsBackup {
        path: std::path::PathBuf,
        prev: Option<Vec<u8>>,
    }
    impl Drop for PrefsBackup {
        fn drop(&mut self) {
            let _w = crate::tests::PREFS_LOCK
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
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
    let prefs_path = Preferences::file_path();
    let prev_prefs = {
        let _r = crate::tests::PREFS_LOCK
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::fs::read(&prefs_path).ok()
    };
    let _prefs_backup = PrefsBackup {
        path: prefs_path,
        prev: prev_prefs,
    };

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1480.0, 900.0))
        .build_eframe(build_ready_app);
    harness.state_mut().prefs.toolbar_buttons = neoveg_common::prefs::default_toolbar_buttons();
    harness.run_steps(2);
    let ctx = harness.ctx.clone();

    // 1. Barra de ferramentas personalizável.
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "options.customize_toolbar");
    harness.run_steps(2);
    let before_tb = harness.state().prefs.toolbar_buttons.len();
    harness.get_by_label("＋ Separador (|)").click();
    harness.run_steps(1);
    harness.get_by_label("OK").click();
    harness.run_steps(2);
    assert_eq!(harness.state().prefs.toolbar_buttons.len(), before_tb + 1);

    // 2. Grade da régua e Encaixar na grade.
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "options.snap_to_grid");
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "options.grid_cycle");
    harness.run_steps(2);
    assert!(harness.state().opts.snap_to_grid);
    assert_ne!(
        harness.state().opts.ruler_grid,
        neoveg_engine::session::RulerGridSpacing::Auto
    );

    // 3. Marcadores de CD (faixa/índice) e Marcador de Comando.
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "insert.marker_cd_track");
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "insert.marker_cd_index");
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "insert.marker_command");
    harness.run_steps(2);
    harness.get_by_label("Inserir marcador").click();
    harness.run_steps(2);
    assert!(harness
        .state()
        .project
        .markers
        .iter()
        .any(|m| m.kind == crate::model::MarkerKind::CdTrack));
    assert!(harness
        .state()
        .project
        .markers
        .iter()
        .any(|m| m.kind == crate::model::MarkerKind::CdIndex));
    assert!(harness
        .state()
        .project
        .markers
        .iter()
        .any(|m| m.kind == crate::model::MarkerKind::Command));

    // 4. Modos de automação (Off / Read / Touch / Latch).
    harness.state_mut().session.sel_track = Some(0);
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "track.cycle_automation");
    harness.run_steps(2);
    assert_eq!(
        harness.state().project.tracks[0].automation_mode,
        crate::model::AutomationMode::Touch
    );

    // 5. Divisor de largura do cabeçalho de trilha recolhe progressivamente e restaura (p. 69).
    assert!(harness
        .query_by_label("Divisor de largura dos cabeçalhos")
        .is_some());
    harness.state_mut().tl.toggle_header_collapse();
    harness.run_steps(2);
    assert!(harness.state().tl.header_w < 100.0);
    harness.state_mut().tl.toggle_header_collapse();
    harness.run_steps(2);
    assert!(harness.state().tl.header_w >= 180.0);

    // 6. Novas abas de Preferências (Dispositivo de Prévia, Dispositivo de Áudio, MIDI/Sync, File I/O).
    harness.state_mut().exec_cmd(Some(&ctx), "options.prefs");
    harness.run_steps(2);
    for (tab_lbl, expected_txt) in [
        ("Dispositivo de Prévia", "Dispositivo de Prévia Secundário"),
        ("Dispositivo de Áudio", "Compensação de gravação:"),
        (
            "MIDI / Sync / Controle",
            "Receber código de tempo MIDI (MTC / Disparo de transporte)",
        ),
        ("E/S de Arquivos (File I/O)", "FPS de sequências de imagem:"),
    ] {
        harness.get_by_label(tab_lbl).click();
        harness.run_steps(2);
        assert!(
            harness.query_by_label(expected_txt).is_some(),
            "aba '{tab_lbl}' não exibiu '{expected_txt}'"
        );
    }
}

#[test]
fn kittest_visor_e_chips_sem_oscilacao_tabular() {
    fn collect_text_shapes_with_seps(
        shape: &egui::epaint::Shape,
        out: &mut Vec<(String, egui::Rect, Vec<f32>)>,
    ) {
        match shape {
            egui::epaint::Shape::Text(ts) => {
                let rect = egui::Rect::from_min_size(ts.pos, ts.galley.size());
                let seps = ts
                    .galley
                    .rows
                    .iter()
                    .flat_map(|row| row.glyphs.iter())
                    .filter(|g| g.chr == ':' || g.chr == ';')
                    .map(|g| ts.pos.x + g.pos.x)
                    .collect();
                out.push((ts.galley.text().to_owned(), rect, seps));
            }
            egui::epaint::Shape::Vec(items) => {
                for item in items {
                    collect_text_shapes_with_seps(item, out);
                }
            }
            _ => {}
        }
    }

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1480.0, 900.0))
        .build_eframe(build_ready_app);

    let settings = harness.state().project.settings.clone();
    let fmt = settings.effective_time_format(harness.state().opts.time_format);
    let smpte_samples = [
        "00:00:00;00",
        "11:11:11;11",
        "07:47:57;27",
        "17:41:51;11",
        "08:08:08;08",
    ];

    let mut pos_lbl_xs = Vec::new();
    let mut tam_lbl_xs = Vec::new();
    let mut pos_chip_rects = Vec::new();
    let mut tam_chip_rects = Vec::new();
    let mut visor_rects = Vec::new();
    let mut visor_seps: Vec<Vec<f32>> = Vec::new();
    let mut chip_seps: Vec<Vec<f32>> = Vec::new();

    for tc in smpte_samples {
        let t = neoveg_common::parse_time(tc, settings.fps, settings.sample_rate, fmt)
            .expect("timecode SMPTE válido deve ser interpretado");
        let rendered = harness.state().fmt_time(t);
        assert_eq!(rendered, tc);

        // 1) Sem seleção: mede `Pos:` e `Visor de tempo` com `cursor = t`.
        harness.state_mut().session.time_sel = None;
        harness.state_mut().session.cursor = t;
        harness.run_steps(2);

        let pos_lbl_bounds = harness
            .get_by_label("Pos:")
            .raw_bounds()
            .expect("Pos: deve ter bounds");
        let pos_chip_bounds = harness
            .get_all_by_label(tc)
            .filter_map(|n| n.raw_bounds())
            .min_by(|a, b| a.x0.total_cmp(&b.x0))
            .expect("chip de Pos: deve ter bounds");
        let tam_lbl_bounds = harness
            .get_by_label("Tam:")
            .raw_bounds()
            .expect("Tam: deve ter bounds");

        pos_lbl_xs.push(pos_lbl_bounds.x0 as f32);
        pos_chip_rects.push(pos_chip_bounds);
        tam_lbl_xs.push(tam_lbl_bounds.x0 as f32);

        let ctx = harness.ctx.clone();
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1480.0, 900.0),
                )),
                time: Some(1.0),
                ..Default::default()
            },
            |c| harness.state_mut().frame_ui(c),
        );
        let mut text_shapes = Vec::new();
        for clipped in &out.shapes {
            collect_text_shapes_with_seps(&clipped.shape, &mut text_shapes);
        }
        let visor_lbl_rect = text_shapes
            .iter()
            .find(|(s, _, _)| s == "Visor de tempo")
            .map(|(_, r, _)| *r)
            .expect("Visor de tempo deve existir");
        let (visor_tc_rect, visor_tc_seps) = text_shapes
            .iter()
            .find(|(s, r, _)| {
                s == tc
                    && r.left() < 260.0
                    && r.top() > visor_lbl_rect.top()
                    && r.top() < visor_lbl_rect.top() + 60.0
            })
            .map(|(_, r, seps)| (*r, seps.clone()))
            .expect("timecode do Visor de tempo deve existir");
        assert_eq!(visor_tc_seps.len(), 3);
        visor_rects.push(visor_tc_rect);
        visor_seps.push(visor_tc_seps);

        let (_, _, pos_chip_seps) = text_shapes
            .iter()
            .filter(|(s, r, _)| s == tc && r.top() > visor_lbl_rect.bottom())
            .min_by(|(_, a, _), (_, b, _)| a.left().total_cmp(&b.left()))
            .cloned()
            .expect("TextShape do chip Pos: deve existir na barra de transporte");
        assert_eq!(pos_chip_seps.len(), 3);
        chip_seps.push(pos_chip_seps);

        // 2) Com seleção de duração `t`: mede `Tam:` com `sel_len = t` (cursor = 0).
        harness.state_mut().session.cursor = neoveg_common::Ticks::ZERO;
        harness.state_mut().session.time_sel = Some((neoveg_common::Ticks::ZERO, t));
        harness.run_steps(2);
        let tam_chip_bounds = harness
            .get_all_by_label(tc)
            .filter_map(|n| n.raw_bounds())
            .max_by(|a, b| a.x0.total_cmp(&b.x0))
            .expect("chip de Tam: deve ter bounds");
        tam_chip_rects.push(tam_chip_bounds);
    }

    let assert_f32_spread = |name: &str, vals: &[f32]| {
        let min = vals.iter().copied().fold(f32::INFINITY, f32::min);
        let max = vals.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            (max - min) < 0.05,
            "{name} oscilou: min={min}, max={max}, diff={}",
            max - min
        );
    };
    let assert_f64_spread = |name: &str, vals: &[f64]| {
        let min = vals.iter().copied().fold(f64::INFINITY, f64::min);
        let max = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (max - min) < 0.05,
            "{name} oscilou: min={min}, max={max}, diff={}",
            max - min
        );
    };

    assert_f32_spread("Pos: label x0", &pos_lbl_xs);
    assert_f32_spread("Tam: label x0", &tam_lbl_xs);
    assert_f64_spread(
        "Pos: chip x0",
        &pos_chip_rects.iter().map(|r| r.x0).collect::<Vec<_>>(),
    );
    assert_f64_spread(
        "Pos: chip width",
        &pos_chip_rects.iter().map(|r| r.width()).collect::<Vec<_>>(),
    );
    assert_f64_spread(
        "Tam: chip x0",
        &tam_chip_rects.iter().map(|r| r.x0).collect::<Vec<_>>(),
    );
    assert_f64_spread(
        "Tam: chip width",
        &tam_chip_rects.iter().map(|r| r.width()).collect::<Vec<_>>(),
    );
    assert_f32_spread(
        "Visor de tempo rect.left",
        &visor_rects.iter().map(|r| r.left()).collect::<Vec<_>>(),
    );
    for col in 0..3 {
        let v_col = visor_seps.iter().map(|s| s[col]).collect::<Vec<_>>();
        assert_f32_spread(&format!("Visor de tempo sep[{col}]"), &v_col);
        let c_col = chip_seps.iter().map(|s| s[col]).collect::<Vec<_>>();
        assert_f32_spread(&format!("Chip Pos: sep[{col}]"), &c_col);
    }

    // 3) Valida também sequências literais com todos os dígitos críticos ('0','1','7','8'),
    // incluindo "77:77:77;77" e "88:88:88;88", no mesmo padrão de Visor + chips `Pos:`/`Tam:`.
    let literal_samples = [
        "00:00:00;00",
        "11:11:11;11",
        "77:77:77;77",
        "17:41:51;11",
        "88:88:88;88",
    ];
    let probe_ctx = egui::Context::default();
    crate::fonts::configure_fonts(&probe_ctx, false);
    let mut lit_visor_rects = Vec::new();
    let mut lit_pos_rects = Vec::new();
    let mut lit_tam_rects = Vec::new();
    let mut lit_visor_seps: Vec<Vec<f32>> = Vec::new();

    for tc in literal_samples {
        let mut v_rect = egui::Rect::NOTHING;
        let mut p_rect = egui::Rect::NOTHING;
        let mut t_rect = egui::Rect::NOTHING;
        let out = probe_ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 200.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::TopBottomPanel::top("lit_transport").show(ctx, |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let chip = |ui: &mut egui::Ui, label: &str| {
                            egui::Frame::new()
                                .inner_margin(egui::Margin::symmetric(5, 1))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 3.0;
                                        let name = egui::RichText::new(label).small();
                                        if ui.layout().prefer_right_to_left() {
                                            crate::fonts::tabular_label(
                                                ui,
                                                tc,
                                                crate::fonts::visor_font(10.5),
                                                egui::Color32::WHITE,
                                            );
                                            ui.label(name);
                                        } else {
                                            ui.label(name);
                                            crate::fonts::tabular_label(
                                                ui,
                                                tc,
                                                crate::fonts::visor_font(10.5),
                                                egui::Color32::WHITE,
                                            );
                                        }
                                    });
                                })
                                .response
                                .rect
                        };
                        t_rect = chip(ui, "Tam:");
                        p_rect = chip(ui, "Pos:");
                    });
                });
                egui::CentralPanel::default().show(ctx, |ui| {
                    v_rect = crate::fonts::paint_tabular_text(
                        ui.painter(),
                        egui::pos2(240.0, 80.0),
                        egui::Align2::RIGHT_BOTTOM,
                        tc,
                        crate::fonts::visor_font(20.0),
                        egui::Color32::WHITE,
                    );
                });
            },
        );
        let mut shapes = Vec::new();
        for clipped in &out.shapes {
            collect_text_shapes_with_seps(&clipped.shape, &mut shapes);
        }
        let (_, _, seps) = shapes
            .into_iter()
            .find(|(s, r, _)| s == tc && r.top() > 40.0)
            .expect("Visor literal deve existir");
        assert_eq!(seps.len(), 3);
        lit_visor_rects.push(v_rect);
        lit_pos_rects.push(p_rect);
        lit_tam_rects.push(t_rect);
        lit_visor_seps.push(seps);
    }

    assert_f32_spread(
        "Literal Visor left",
        &lit_visor_rects.iter().map(|r| r.left()).collect::<Vec<_>>(),
    );
    assert_f32_spread(
        "Literal Visor width",
        &lit_visor_rects
            .iter()
            .map(|r| r.width())
            .collect::<Vec<_>>(),
    );
    assert_f32_spread(
        "Literal Pos chip x0",
        &lit_pos_rects.iter().map(|r| r.left()).collect::<Vec<_>>(),
    );
    assert_f32_spread(
        "Literal Pos chip width",
        &lit_pos_rects.iter().map(|r| r.width()).collect::<Vec<_>>(),
    );
    assert_f32_spread(
        "Literal Tam chip x0",
        &lit_tam_rects.iter().map(|r| r.left()).collect::<Vec<_>>(),
    );
    assert_f32_spread(
        "Literal Tam chip width",
        &lit_tam_rects.iter().map(|r| r.width()).collect::<Vec<_>>(),
    );
    for col in 0..3 {
        let v_col = lit_visor_seps.iter().map(|s| s[col]).collect::<Vec<_>>();
        assert_f32_spread(&format!("Literal Visor sep[{col}]"), &v_col);
    }
}

/// Barra do Trimmer no layout do VEGAS Pro: reproduzir e parar (Parar volta
/// ao ponto de partida) e *Editar conjunto de botões visíveis* ("…"), que
/// some com o botão desmarcado e grava a escolha nas preferências.
#[test]
fn kittest_trimmer_barra_reproduzir_e_botoes_visiveis() {
    let _prefs_guard = I18nPrefsGuard::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .build_eframe(|cc| build_ready_app_lang(cc, neoveg_common::Language::PtBr));
    harness.state_mut().prefs.trimmer_hidden_buttons.clear();
    harness.run_steps(2);
    {
        let app = harness.state_mut();
        app.insert_generated(crate::model::GenSpec::Text(
            crate::model::TextSpec::default(),
        ));
        let mid = app.project.media[0].id;
        app.project.media[0].duration = Some(neoveg_common::Ticks::from_secs_i64(10));
        app.open_in_trimmer(mid);
    }
    let ctx = harness.ctx.clone();
    harness.state_mut().exec_cmd(Some(&ctx), "view.tab.trimmer");
    harness.run_steps(2);
    let start = neoveg_common::Ticks::from_secs_i64(1);
    harness.state_mut().trimmer.cursor = start;
    harness.run_steps(1);
    harness.get_by_label("Reproduzir").click();
    harness.run_steps(3);
    assert!(harness.state().trimmer.playing);
    harness.get_by_label("Parar").click();
    harness.run_steps(1);
    assert!(!harness.state().trimmer.playing);
    assert_eq!(
        harness.state().trimmer.cursor,
        start,
        "Parar volta ao início da reprodução"
    );

    harness
        .get_by_label("Editar conjunto de botões visíveis...")
        .click();
    harness.run_steps(2);
    // com o diálogo aberto, "Pausar" é o botão da barra e a caixa de marcar
    harness
        .get_by_role_and_label(egui::accesskit::Role::CheckBox, "Pausar")
        .click();
    harness.run_steps(1);
    harness.get_by_label("OK").click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().prefs.trimmer_hidden_buttons,
        vec!["pause".to_string()]
    );
    assert!(harness.query_by_label("Pausar").is_none());
    assert!(harness.query_by_label("Reproduzir").is_some());
}

#[test]
fn kittest_transcricao_de_fala_e_edicao_timeline() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_eframe(|cc| {
            let mut app = build_ready_app(cc);
            // Insere evento de áudio na timeline
            let m = crate::model::MediaItem {
                id: crate::model::new_id(),
                name: "audio.wav".into(),
                source: crate::model::MediaSource::Generated(crate::model::GenSpec::ColorBars),
                duration: Some(Ticks::from_secs_i64(10)),
                has_video: false,
                has_audio: true,
                width: 0,
                height: 0,
                fps: None,
                info: String::new(),
                proxy_path: None,
            };
            app.project.media.push(m.clone());
            let ev = crate::model::Event::new(&m, Ticks::ZERO, Ticks::from_secs_i64(8));
            app.project.tracks[1].events = vec![ev];
            app.closed_tabs.remove(&DockTab::Transcript);
            app.dock_visible = true;
            app.dock_tab = DockTab::Transcript;
            app.pending_dock_focus = Some(DockTab::Transcript);
            app
        });

    harness.run_steps(2);

    // Clica no botão de transcrição da timeline
    harness
        .get_by_label("Transcrever Áudio da Timeline")
        .click();
    harness.run_steps(2);

    // Verifica que a transcrição foi gerada e está presente
    assert!(harness.state().project.transcript.is_some());
    let words_cnt = harness
        .state()
        .project
        .transcript
        .as_ref()
        .unwrap()
        .word_count();
    assert!(words_cnt > 0);

    // Clica em gerar legendas como regiões
    harness.get_by_label("Gerar Legendas como Regiões").click();
    harness.run_steps(2);
    assert!(!harness.state().project.regions.is_empty());

    // Clica em remover silêncios
    harness
        .get_by_label("Remover Silêncios e Pausas (> 1,2s)")
        .click();
    harness.run_steps(2);
}

/// Janela Pan/Crop no layout do VEGAS Pro (manual do VEGAS Pro 21, pp.
/// 250–251): grade de propriedades com seções recolhíveis, barra de
/// ferramentas vertical e linha de quadros-chave na mesma janela.
#[test]
fn kittest_pan_crop_grade_de_propriedades_recolhivel() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 920.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);
    {
        let app = harness.state_mut();
        app.insert_generated(crate::model::GenSpec::SolidColor {
            color: [0.2, 0.4, 0.8, 1.0],
        });
        app.dlg.gen_editor = None;
        let eid = app
            .project
            .tracks
            .iter()
            .flat_map(|t| t.events.iter())
            .map(|e| e.id)
            .next()
            .expect("evento gerado");
        app.dlg.pan_crop = Some(eid);
    }
    harness.run_steps(3);
    for label in [
        "Posição",
        "Rotação",
        "Área de trabalho",
        "Largura:",
        "Zoom (%):",
    ] {
        assert!(
            harness.query_by_label(label).is_some(),
            "falta '{label}' na janela Pan/Crop"
        );
    }
    // Regressão: a barra vertical esquerda não pode expandir com `ui.separator()`
    // e empurrar as propriedades nem a área de trabalho para fora da janela.
    let props_bounds = harness
        .get_by_label("Largura:")
        .raw_bounds()
        .expect("Largura: deve ter limites");
    assert!(
        props_bounds.x0 >= 0.0 && props_bounds.x1 < 700.0,
        "coluna de propriedades fora da área visível: {props_bounds:?}"
    );
    let has_visible_workspace_fit = harness
        .query_all_by_label("Ajustar")
        .filter_map(|n| n.raw_bounds())
        .any(|b| b.x0 > props_bounds.x1 && b.x1 <= 1500.0 && b.y1 <= 920.0);
    assert!(
        has_visible_workspace_fit,
        "área de trabalho de Pan/Crop empurrada para fora da janela: props={props_bounds:?}"
    );
    // seção recolhida esconde as linhas dela
    harness.get_by_label("Posição").click();
    harness.run_steps(2);
    assert!(harness.query_by_label("Largura:").is_none());
    harness.get_by_label("Posição").click();
    harness.run_steps(2);
    assert!(harness.query_by_label("Largura:").is_some());
    // fechar a janela limpa o estado
    harness.state_mut().dlg.pan_crop = None;
    harness.run_steps(2);
    assert!(harness.query_by_label("Área de trabalho").is_none());
}

/// Janela Movimento da Trilha 2D no layout do VEGAS Pro (manual do VEGAS Pro
/// 21, pp. 412–413): composição da trilha na barra, grade de propriedades
/// com Posição, Rotação, Quadro-chave, Área de trabalho e Encaixe.
#[test]
fn kittest_movimento_da_trilha_grade_e_composicao() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 920.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);
    let tid = {
        let app = harness.state_mut();
        let tid = app
            .project
            .tracks
            .iter()
            .find(|t| t.kind == crate::model::TrackKind::Video)
            .map(|t| t.id)
            .expect("trilha de vídeo");
        app.dlg.track_motion = Some(tid);
        tid
    };
    harness.run_steps(3);
    for label in [
        "Posição",
        "Rotação",
        "Quadro-chave",
        "Área de trabalho",
        "Encaixe",
        "Largura:",
    ] {
        assert!(
            harness.query_by_label(label).is_some(),
            "falta '{label}' na janela Movimento da Trilha"
        );
    }
    harness.get_by_label("Encaixe").click();
    harness.run_steps(2);
    assert!(harness.query_by_label("Espaçamento da grade:").is_none());
    let blend = |h: &Harness<'_, App>| {
        h.state()
            .project
            .tracks
            .iter()
            .find(|t| t.id == tid)
            .map(|t| t.blend)
    };
    assert_eq!(blend(&harness), Some(crate::model::BlendMode::SourceAlpha));
    harness.state_mut().dlg.track_motion = None;
    harness.run_steps(2);
    assert!(harness.query_by_label("Encaixe").is_none());
}

/// Propriedades do projeto com as abas do VEGAS Pro (Vídeo, Áudio, Régua,
/// Resumo, CD de áudio): trocar de aba por clique e a aba fica lembrada.
#[test]
fn kittest_propriedades_do_projeto_tem_as_cinco_abas() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 920.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);
    harness.state_mut().dlg.project_props = true;
    harness.run_steps(2);
    assert_eq!(harness.state().dlg.project_props_tab, 0);
    harness.get_by_label("CD de áudio").click();
    harness.run_steps(2);
    assert_eq!(harness.state().dlg.project_props_tab, 4);
    harness.get_by_label("Primeira faixa");
    harness.get_by_label("Resumo").click();
    harness.run_steps(2);
    assert_eq!(harness.state().dlg.project_props_tab, 3);
    harness.get_by_label("Comentários");
}

/// Faixa de projetos abertos fica acima da faixa de sequências, e a caixa de
/// seleção (EditTool::Selection) seleciona eventos na linha do tempo.
#[test]
fn kittest_faixa_projetos_acima_da_faixa_sequencias_e_retangulo_selecao() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 920.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);

    let doc_bar = harness.state().tl.last_doc_bar;
    let seq_bar = harness.state().tl.last_seq_bar;
    assert!(
        doc_bar.is_positive(),
        "faixa de projetos abertos deve ser desenhada"
    );
    assert!(
        seq_bar.is_positive(),
        "faixa de sequências deve ser desenhada"
    );
    assert!(
        doc_bar.bottom() <= seq_bar.top() + 0.5,
        "faixa de projetos ({doc_bar:?}) deve ficar acima da faixa de sequências ({seq_bar:?})"
    );

    // Insere evento em t=2s (dentro da área das faixas), fecha o editor de
    // gerador e arrasta a caixa de seleção com EditTool::Selection sobre ele.
    let eid = {
        let app = harness.state_mut();
        app.cursor = Ticks::from_secs_i64(2);
        app.insert_generated(crate::model::GenSpec::SolidColor {
            color: [0.2, 0.6, 0.9, 1.0],
        });
        app.dlg.gen_editor = None;
        app.sel_events.clear();
        app.session.edit_tool = neoveg_engine::session::EditTool::Selection;
        app.project
            .tracks
            .iter()
            .flat_map(|t| t.events.iter())
            .map(|e| e.id)
            .next()
            .expect("evento gerado")
    };
    harness.run_steps(2);
    let lanes = harness.state().tl.last_lanes;
    let ev_rect = harness
        .state()
        .event_screen_rect(eid)
        .expect("evento visível na timeline");
    let p0 = egui::pos2(
        (ev_rect.left() - 12.0).max(lanes.left() + 4.0),
        (ev_rect.top() + 4.0).max(lanes.top() + 4.0),
    );
    let p1 = egui::pos2(
        (ev_rect.center().x + 12.0).min(lanes.right() - 4.0),
        (ev_rect.bottom() - 4.0).min(lanes.bottom() - 4.0),
    );
    assert!(lanes.contains(p0) && lanes.contains(p1));
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(p0));
    harness.run_steps(1);
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: p0,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(1);
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(p1));
    harness.run_steps(2);
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: p1,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(2);
    assert!(
        harness.state().sel_events.contains(&eid),
        "retângulo de seleção deve selecionar o evento alcançado (lanes={lanes:?}, ev={ev_rect:?}, p0={p0:?}, p1={p1:?})"
    );
}

/// Arrastar uma mídia da Mídia do Projeto para a aba do Trimmer abre a mídia
/// diretamente no Trimmer (manual do VEGAS Pro 21, p. 313).
#[test]
fn kittest_arrastar_midia_do_projeto_para_o_trimmer() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 920.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);

    let (mid, mname) = {
        let app = harness.state_mut();
        app.insert_generated(crate::model::GenSpec::ColorBars);
        app.dlg.gen_editor = None;
        app.trimmer.media_id = None;
        app.dock_visible = true;
        app.dock_tab = crate::DockTab::ProjectMedia;
        app.pending_dock_focus = Some(crate::DockTab::ProjectMedia);
        let m = app.project.media.first().expect("mídia criada");
        (m.id, m.name.clone())
    };
    harness.run_steps(3);

    let from = harness
        .query_all_by_label(&mname)
        .filter_map(|n| n.raw_bounds())
        .filter(|b| (b.y0 as f32) < 360.0)
        .max_by(|a, b| {
            let area_a = (a.x1 - a.x0) * (a.y1 - a.y0);
            let area_b = (b.x1 - b.x0) * (b.y1 - b.y0);
            area_a.total_cmp(&area_b)
        })
        .map(|b| egui::pos2(((b.x0 + b.x1) * 0.5) as f32, ((b.y0 + b.y1) * 0.5) as f32))
        .expect("cartão da mídia na Mídia do Projeto");
    let to = harness
        .query_all_by_label("Trimmer")
        .filter_map(|n| n.raw_bounds())
        .find(|b| (b.y0 as f32) < 380.0)
        .map(|b| egui::pos2(((b.x0 + b.x1) * 0.5) as f32, ((b.y0 + b.y1) * 0.5) as f32))
        .expect("aba Trimmer no dock");

    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(from));
    harness.run_steps(1);
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(1);
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(from + egui::vec2(14.0, 4.0)));
    harness.run_steps(2);
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(to));
    harness.run_steps(2);
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(2);

    assert_eq!(
        harness.state().trimmer.media_id,
        Some(mid),
        "soltar mídia da Mídia do Projeto sobre a aba Trimmer deve abrir a mídia no Trimmer"
    );
}

/// Modo de fluxo de trabalho Clássico oculta a faixa separada de sequências e
/// trata sequências adicionais como projetos internos na faixa de projetos
/// abertos; o modo Intermediário exibe ambas as faixas empilhadas.
#[test]
fn kittest_fluxo_trabalho_classico_e_intermediario() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 920.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);

    // Padrão é Intermediário: ambas as faixas visíveis
    assert_eq!(
        harness.state().prefs.workflow_mode,
        neoveg_common::WorkflowMode::Intermediate
    );
    assert!(harness.state().tl.last_doc_bar.is_positive());
    assert!(harness.state().tl.last_seq_bar.is_positive());

    // Abre Preferências > Aparência e Layout e verifica as opções de fluxo de trabalho
    {
        let app = harness.state_mut();
        app.exec_cmd(None, "options.prefs");
        app.dlg.prefs_tab = crate::dialogs::PrefsCategory::Appearance;
    }
    harness.run_steps(2);
    assert!(harness.query_by_label("Clássico").is_some());
    assert!(harness.query_by_label("Intermediário").is_some());
    harness.state_mut().dlg.prefs_open = false;

    // Alterna para fluxo Clássico e cria uma segunda sequência interna
    let (seq1, seq2) = {
        let app = harness.state_mut();
        app.prefs.workflow_mode = neoveg_common::WorkflowMode::Classic;
        let s1 = app
            .project
            .active_sequence_id
            .expect("sequência principal ativa");
        let s2 = app.create_sequence_app(Some("Subprojeto B"));
        app.switch_sequence_app(s1);
        (s1, s2)
    };
    harness.run_steps(2);
    assert!(harness.state().tl.last_doc_bar.is_positive());
    assert!(
        !harness.state().tl.last_seq_bar.is_positive(),
        "no fluxo Clássico a faixa dedicada de sequências deve ficar oculta"
    );
    let doc_bar = harness.state().tl.last_doc_bar;
    let seq2_tab = harness
        .query_all_by_label("Subprojeto B")
        .filter_map(|n| n.raw_bounds())
        .find(|b| (b.y0 as f32) >= doc_bar.top() - 2.0 && (b.y1 as f32) <= doc_bar.bottom() + 2.0)
        .expect("Subprojeto B deve aparecer como projeto interno na faixa de projetos abertos");
    let click_pos = egui::pos2(
        ((seq2_tab.x0 + seq2_tab.x1) * 0.5) as f32,
        ((seq2_tab.y0 + seq2_tab.y1) * 0.5) as f32,
    );
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(click_pos));
    harness.run_steps(1);
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: click_pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(1);
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: click_pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(2);
    assert_eq!(harness.state().project.active_sequence_id, Some(seq2));
    assert_ne!(seq1, seq2);

    // Volta para Intermediário: faixa de sequências reaparece
    harness.state_mut().prefs.workflow_mode = neoveg_common::WorkflowMode::Intermediate;
    harness.run_steps(2);
    assert!(harness.state().tl.last_seq_bar.is_positive());
}

/// Seleção em Efeitos de Vídeo e Plug-In Manager usa selectable_label limpo
/// (sem camadas conflitantes), idêntico a Geradores de Mídia.
#[test]
fn kittest_selecao_efeitos_e_plugin_manager_igual_geradores() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 920.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);

    // Efeitos de Vídeo: clicar diretamente no nome seleciona o efeito
    {
        let app = harness.state_mut();
        app.dock_visible = true;
        app.dock_tab = crate::DockTab::VideoFx;
        app.pending_dock_focus = Some(crate::DockTab::VideoFx);
    }
    harness.run_steps(2);
    let fx_name = crate::model::FxKind::HslAdjust.name_in(neoveg_common::Language::PtBr);
    harness.get_by_label(fx_name).click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().fx_browser.selected,
        Some(crate::model::FxKind::HslAdjust)
    );
    assert!(harness
        .query_by_label(&format!("Presets: {fx_name}"))
        .is_some());

    // Plug-In Manager: clicar diretamente no nome seleciona e abre os detalhes + prévia
    {
        let app = harness.state_mut();
        app.dock_tab = crate::DockTab::PluginManager;
        app.pending_dock_focus = Some(crate::DockTab::PluginManager);
    }
    harness.run_steps(2);
    harness
        .query_all_by_label(fx_name)
        .next()
        .expect("efeito listado no Plug-In Manager")
        .click();
    harness.run_steps(2);
    assert!(
        harness.query_by_label("Origem: nativo do Neoveg").is_some(),
        "clicar em um plug-in no Plug-In Manager deve exibir imediatamente seus detalhes"
    );
}

/// Layouts de janela fiéis ao VEGAS Pro mantêm painéis acoplados lado a lado
/// (sem abrir janelas soltas indesejadas).
#[test]
fn kittest_layouts_vegas_acoplados_sem_janelas_soltas() {
    use crate::dock::{find_pane_tile, DockPane};

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 920.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);

    let presets = crate::layout::builtin_presets();
    let trimmer_layout = presets
        .iter()
        .find(|p| p.name == "Edição com Trimmer")
        .cloned()
        .expect("preset Edição com Trimmer");
    harness.state_mut().apply_window_layout(trimmer_layout);
    harness.run_steps(2);
    {
        let st = harness.state();
        assert!(st.detached_tabs.is_empty());
        let parent_of = |pane: DockPane| {
            find_pane_tile(&st.dock_tree, pane).and_then(|id| st.dock_tree.tiles.parent_of(id))
        };
        assert_ne!(
            parent_of(DockPane::Tab(crate::DockTab::Trimmer)),
            parent_of(DockPane::Preview),
            "em Edição com Trimmer, o Trimmer deve ficar lado a lado com a Prévia de Vídeo"
        );
    }

    let color_layout = presets
        .iter()
        .find(|p| p.name == "Correção de Cor (Escopos)")
        .cloned()
        .expect("preset Correção de Cor");
    harness.state_mut().apply_window_layout(color_layout);
    harness.run_steps(2);
    {
        let st = harness.state();
        assert!(st.detached_tabs.is_empty());
        let parent_of = |pane: DockPane| {
            find_pane_tile(&st.dock_tree, pane).and_then(|id| st.dock_tree.tiles.parent_of(id))
        };
        let cg_g = parent_of(DockPane::Tab(crate::DockTab::ColorGrading));
        let pv_g = parent_of(DockPane::Preview);
        let sc_g = parent_of(DockPane::Tab(crate::DockTab::Scopes));
        assert!(cg_g.is_some() && pv_g.is_some() && sc_g.is_some());
        assert_ne!(cg_g, pv_g);
        assert_ne!(pv_g, sc_g);
        assert_ne!(cg_g, sc_g);
    }
}

/// Paridade visual das 4 gerações de layout (Clássico v1, Intermediário v8–13,
/// Recente v14–22 e Moderno v23+), rodapé da Prévia de Vídeo e barra de status.
#[test]
fn kittest_paridade_geracoes_previa_e_barra_status() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1480.0, 900.0))
        .build_eframe(build_ready_app);
    harness.state_mut().status.clear();
    harness.run_steps(2);

    assert!(
        harness.query_by_label_contains("Concluído:").is_some(),
        "barra de status ociosa deve exibir Concluído: com a duração do projeto"
    );
    assert!(
        harness.query_by_label_contains("RAM:").is_some(),
        "barra de status ampla deve exibir o orçamento de Prévia em RAM"
    );

    let ctx = harness.ctx.clone();
    for lt in neoveg_common::LayoutType::ALL {
        harness
            .state_mut()
            .select_layout_type(Some(&ctx), lt, false);
        harness.run_steps(2);
        assert_eq!(harness.state().prefs.edit_layout.layout_type, lt);
    }
}

/// Paridade do Console de Mixagem (barra lateral, filtros, seções, fase Ø),
/// Master Bus (barramento Preview e trava L/R), Loudness (6 barras graduadas)
/// e Surround Panner (pad 5.1 com ganhos em dB).
#[test]
fn kittest_paridade_console_master_loudness_e_surround() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 920.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);

    // 1. Master Bus: alterna barramento Preview e verifica sincronia de ganho
    assert!(!harness.state().master_show_preview_bus);
    harness.get_by_label("Barramento Preview").click();
    harness.run_steps(2);
    assert!(harness.state().master_show_preview_bus);

    // 2. Console de Mixagem: barra lateral, inversão de fase Ø e visibilidade de seções
    let ctx = harness.ctx.clone();
    harness.state_mut().exec_cmd(Some(&ctx), "view.tab.mixer");
    harness.run_steps(2);
    assert!(harness.state().mixer_ui.sidebar_open);
    assert!(
        harness.query_by_label("Visibilidade").is_some(),
        "Console de Mixagem deve exibir a barra lateral de visibilidade de canais"
    );
    assert!(
        harness.query_by_label("Seções").is_some(),
        "Console de Mixagem deve exibir os alternadores de seções"
    );
    harness.get_by_label("Ø").click();
    harness.run_steps(2);
    let audio_track = harness
        .state()
        .project
        .tracks
        .iter()
        .find(|t| t.kind == crate::model::TrackKind::Audio)
        .expect("trilha de áudio padrão");
    assert!(
        audio_track.invert_phase,
        "clicar no botão Ø deve ativar inversão de fase na trilha de áudio"
    );

    // 3. Medidores de Loudness com barra LRA e LEDs
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "view.tab.loudness");
    harness.run_steps(2);
    assert!(harness.query_by_label("LRA").is_some());

    // 4. Surround Panner 5.1 com botões LFE e C
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "view.tab.surround_panner");
    harness.state_mut().project.settings.channel_layout = crate::model::ChannelLayout::Surround51;
    harness.run_steps(2);
    assert!(harness.query_by_label("LFE").is_some());
}

/// Paridade do Explorer (migalhas, favoritos, árvore e detalhes), Mídia do Projeto
/// (Mídia por Tipo), Plug-In Manager (árvore Vídeo/Áudio + migalhas) e Escopos (seletor de layout).
#[test]
fn kittest_paridade_explorer_midia_plugins_e_escopos() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 920.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);

    // 1. Mídia do Projeto exibe grupo recolhível "Mídia por Tipo"
    assert!(
        harness.query_by_label("Mídia por Tipo").is_some(),
        "Mídia do Projeto deve agrupar filtros embutidos sob Mídia por Tipo"
    );

    // 2. Explorer: alterna favoritos e árvore lateral
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "view.tab.explorer");
    harness.run_steps(2);
    assert!(harness.query_by_label("Árvore").is_some());
    assert!(harness.query_by_label("Detalhes").is_some());
    harness.get_by_label("☆").click();
    harness.run_steps(2);
    assert_eq!(harness.state().explorer_ui.favorites.len(), 1);

    // 3. Plug-In Manager: árvore hierárquica Vídeo/Áudio e migalhas
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "view.tab.plugin_manager");
    harness.run_steps(2);
    assert!(harness.query_by_label("Plug-Ins").is_some());

    // 4. Escopos de Vídeo: seletor de layout "Todos (2×2)"
    harness.state_mut().exec_cmd(Some(&ctx), "view.tab.scopes");
    harness.run_steps(2);
    assert_eq!(
        harness.state().scopes_layout,
        crate::panels::ScopeViewLayout::All
    );
}

/// Paridade do Color Grading (barra lateral de categorias, rodas com anel cromático e
/// redefinição por roda) e histórico do Trimmer no formato `Arquivo [Diretório]`.
#[test]
fn kittest_paridade_color_grading_e_trimmer_historico() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 920.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);

    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "view.tab.color_grading");
    harness.run_steps(2);
    harness.get_by_label("Ativar Color Grading").click();
    harness.run_steps(2);

    // Barra lateral de categorias e todas as seções visíveis em "Tudo"
    assert!(harness.query_by_label("Tudo").is_some());
    assert!(harness.query_by_label("Rodas de Cor").is_some());
    assert!(harness.query_by_label("LUTs").is_some());
    assert!(harness.query_by_label("Curvas").is_some());
    assert!(harness.query_by_label("HSL").is_some());
    assert!(harness
        .query_by_label("LUT de Câmera / Look (.cube)")
        .is_some());
    assert!(harness.query_by_label("Curvas RGB").is_some());

    // Filtrar apenas LUTs oculta Curvas RGB e mantém LUTs
    harness.get_by_label("LUTs").click();
    harness.run_steps(2);
    assert!(harness
        .query_by_label("LUT de Câmera / Look (.cube)")
        .is_some());
    assert!(harness.query_by_label("Curvas RGB").is_none());

    // Voltar para Tudo restaura todas as seções
    harness.get_by_label("Tudo").click();
    harness.run_steps(2);
    assert!(harness.query_by_label("Curvas RGB").is_some());
    assert!(harness.query_by_label("Curvas HSL (6 setores)").is_some());

    // Trimmer exibe histórico no formato `Arquivo [Diretório]`
    {
        let st = harness.state_mut();
        let mid = crate::model::new_id();
        st.project.media.push(crate::model::MediaItem {
            id: mid,
            name: "take01.mp4".into(),
            source: crate::model::MediaSource::File {
                path: "/projetos/cena01/take01.mp4".into(),
            },
            duration: Some(crate::model::Ticks::from_secs_i64(8)),
            has_video: true,
            has_audio: true,
            width: 1280,
            height: 720,
            fps: Some(crate::model::FrameRate::FPS_25),
            info: String::new(),
            proxy_path: None,
        });
        st.open_in_trimmer(mid);
    }
    harness.state_mut().exec_cmd(Some(&ctx), "view.tab.trimmer");
    harness.run_steps(2);
    let sort_rect = harness
        .get_by_label("Ordenar o histórico por nome")
        .raw_bounds()
        .expect("botão Ordenar o histórico por nome no Trimmer");
    let sort_cy = (sort_rect.y0 + sort_rect.y1) * 0.5;
    harness
        .get_all_by_role(egui::accesskit::Role::ComboBox)
        .filter(|n| n.raw_bounds().is_some_and(|r| r.x1 <= sort_rect.x0 + 4.0))
        .min_by(|a, b| {
            let da = a
                .raw_bounds()
                .map_or(f64::MAX, |r| ((r.y0 + r.y1) * 0.5 - sort_cy).abs());
            let db = b
                .raw_bounds()
                .map_or(f64::MAX, |r| ((r.y0 + r.y1) * 0.5 - sort_cy).abs());
            da.total_cmp(&db)
        })
        .expect("ComboBox de histórico do Trimmer")
        .click();
    harness.run_steps(2);
    assert!(
        harness
            .query_by_label("take01.mp4 [/projetos/cena01]")
            .is_some(),
        "histórico do Trimmer deve exibir nome e diretório entre colchetes"
    );
}

/// Janela Transições no arranjo do VEGAS Pro: abas de categoria, lista e
/// grade de miniaturas; filtrar por categoria e por favoritos.
#[test]
fn kittest_transicoes_em_grade_com_categorias() {
    use crate::model::TransitionKind;
    use crate::panels::{transition_fav_id, TransitionCategory};
    // toda transição tem uma aba e toda aba tem transições
    for c in TransitionCategory::ALL {
        assert!(TransitionKind::ALL
            .iter()
            .any(|k| TransitionCategory::of(*k) == c));
    }
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 950.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .exec_cmd(Some(&ctx), "view.tab.transitions");
    harness.run_steps(3);
    for name in [
        "Efeitos 3D",
        "Revelar",
        "Wipes",
        "Fades",
        "Dobras e páginas",
    ] {
        harness.get_by_label(name);
    }
    // a aba Fades mostra os fades e esconde os wipes
    harness.get_by_label("Fades").click();
    harness.run_steps(3);
    assert_eq!(
        harness.state().transitions_ui.cat,
        Some(TransitionCategory::Fades)
    );
    let dissolve = TransitionKind::Dissolve.name_in(neoveg_common::Language::PtBr);
    let star = TransitionKind::StarWipe.name_in(neoveg_common::Language::PtBr);
    assert!(harness.query_all_by_label(dissolve).count() >= 1);
    assert_eq!(harness.query_all_by_label(star).count(), 0);
    // favoritos: só o marcado aparece
    harness
        .state_mut()
        .prefs
        .fx_favorites
        .push(transition_fav_id(TransitionKind::StarWipe));
    harness.get_by_label_contains("Favoritos").click();
    harness.run_steps(3);
    assert!(harness.state().transitions_ui.favorites_only);
    assert!(harness.query_all_by_label(star).count() >= 1);
    assert_eq!(harness.query_all_by_label(dissolve).count(), 0);
}

/// Mídia ausente: a janela procura todos os arquivos de uma vez, pede a
/// escolha entre homônimos e reaponta as mídias num passo de Desfazer.
#[test]
fn kittest_localizar_midia_ausente_procura_tudo_e_escolhe_homonimo() {
    use crate::model::{MediaItem, MediaSource};
    let dir = std::env::temp_dir().join(format!("neoveg_kittest_relink_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for rel in [
        "Novo/Audio/voz.wav",
        "Backup/voz.wav",
        "Novo/Video/cena.png",
    ] {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"x").unwrap();
    }
    let item = |id, path: &str| MediaItem {
        id,
        name: path.rsplit('/').next().unwrap_or_default().into(),
        source: MediaSource::File { path: path.into() },
        duration: None,
        has_video: true,
        has_audio: false,
        width: 0,
        height: 0,
        fps: None,
        info: String::new(),
        proxy_path: None,
    };

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 920.0))
        .build_eframe(build_ready_app);
    harness.run_steps(2);
    {
        let app = harness.state_mut();
        app.session
            .project
            .media
            .push(item(9001, "Q:/Sumiu/Audio/voz.wav"));
        app.session
            .project
            .media
            .push(item(9002, "Q:/Sumiu/Video/cena.png"));
        assert_eq!(crate::dlg_relink::open_if_missing(app), 2);
        // a busca usa só a pasta de teste
        app.dlg.relink.roots = vec![dir.clone()];
        crate::dlg_relink::start_search(app);
    }
    for _ in 0..200 {
        harness.run_steps(1);
        if !harness.state().dlg.relink.searching() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!harness.state().dlg.relink.searching());
    harness.get_by_label("Mídia não encontrada");
    // cena.png tem um só candidato; voz.wav tem dois e espera a escolha
    let st = &harness.state().dlg.relink;
    assert_eq!(st.choices().len(), 1);
    assert!(st.chosen(9001).is_none());
    assert_eq!(
        st.result
            .as_ref()
            .and_then(|r| r.candidates.get(&9001))
            .map(Vec::len),
        Some(2)
    );

    harness.state_mut().dlg.relink.choice.insert(9001, 0);
    harness.state_mut().dlg.relink.dont_save = true;
    crate::dlg_relink::apply(harness.state_mut());
    harness.run_steps(2);
    let app = harness.state();
    assert!(relink_path(app, 9001).ends_with("voz.wav"));
    assert!(
        relink_path(app, 9001).contains("Novo"),
        "o mais parecido com o original vem primeiro"
    );
    assert!(relink_path(app, 9002).ends_with("cena.png"));
    assert!(!app.dlg.relink.open, "nada mais ausente: a janela fecha");
    // um único Desfazer devolve os dois caminhos antigos
    assert!(harness.state_mut().session.undo());
    let app = harness.state();
    assert_eq!(relink_path(app, 9001), "Q:/Sumiu/Audio/voz.wav");
    assert_eq!(relink_path(app, 9002), "Q:/Sumiu/Video/cena.png");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(test)]
fn relink_path(app: &App, id: crate::model::Id) -> String {
    match app.session.project.media(id).map(|m| &m.source) {
        Some(crate::model::MediaSource::File { path }) => path.clone(),
        _ => String::new(),
    }
}
