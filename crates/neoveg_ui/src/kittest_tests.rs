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

    // Alterna trilhas de barramentos na timeline (Ctrl+B via Tabela de Comandos).
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
    harness.get_by_label("Marcar In (I)").click();
    harness.run_steps(1);

    harness.state_mut().trimmer.cursor = neoveg_common::Ticks::from_secs_i64(5);
    harness.run_steps(1);
    harness.get_by_label("Marcar Out (O)").click();
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
        .get_by_label("Adicionar à Timeline no Cursor")
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

/// Botão Mais > Editar conjunto de botões visíveis (manual do VEGAS Pro 21,
/// p. 69): "Ocultar todos" + OK tira M, S e fx do cabeçalho das faixas de
/// vídeo; as de áudio continuam com os seus.
#[test]
fn kittest_conjunto_de_botoes_visiveis_do_cabecalho() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1500.0, 920.0))
        .build_eframe(build_ready_app);
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
