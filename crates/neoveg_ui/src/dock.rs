//! Motor de acoplamento e divisórias interativas baseado em `egui_tiles`.
//!
//! Mantém a identidade e a semântica de 3 colunas do VEGAS Pro 21
//! (`Grupo de Abas | Prévia de Vídeo | Master Bus`), abas no rodapé por padrão
//! (ou no topo quando configurado), arraste para dividir/reorganizar painéis,
//! destaque para janelas nativas do sistema operacional e persistência segura em
//! `.neoveglayout` (`AGENTS.md §0`, `§2.2`).

use super::{App, DockTab};
use crate::icons::{self, Icon};
use eframe::egui;
use egui::{pos2, vec2, Color32, Rect, Sense, Stroke, StrokeKind, Ui, WidgetText};
use egui_tiles::{
    Behavior, Container, Linear, LinearDir, ResizeState, SimplificationOptions, TabState, Tabs,
    Tile, TileId, Tiles, Tree, UiResponse,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const TREE_SALT: &str = "neoveg_dock_tree";
const MAX_TREE_DEPTH: usize = 64;
const DEFAULT_MASTER_SHARE: f32 = 0.11;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DockPane {
    Tab(DockTab),
    Preview,
    MasterBus,
}

impl DockPane {
    pub fn title_in(self, lang: neoveg_common::Language) -> &'static str {
        match self {
            DockPane::Tab(t) => t.name_in(lang),
            DockPane::Preview => neoveg_common::tr(lang, "Prévia de Vídeo"),
            DockPane::MasterBus => neoveg_common::tr(lang, "Master"),
        }
    }

    pub fn title(self) -> &'static str {
        self.title_in(neoveg_common::Language::PtBr)
    }

    pub fn icon(self) -> Icon {
        match self {
            DockPane::Tab(t) => dock_tab_icon(t),
            DockPane::Preview => Icon::VideoTrack,
            DockPane::MasterBus => Icon::MasterBus,
        }
    }

    /// Painéis que a árvore do dock tem de ter; os só de janela (Encoder)
    /// ficam de fora.
    fn all_canonical() -> Vec<DockPane> {
        let mut out: Vec<DockPane> = DockTab::ALL
            .into_iter()
            .filter(|t| !crate::layout::window_only(*t))
            .map(DockPane::Tab)
            .collect();
        out.push(DockPane::Preview);
        out.push(DockPane::MasterBus);
        out
    }
}

pub fn dock_tab_icon(tab: DockTab) -> Icon {
    match tab {
        DockTab::ProjectMedia => Icon::MediaItem,
        DockTab::Explorer => Icon::FolderItem,
        DockTab::Trimmer => Icon::TrimmerBlade,
        DockTab::EditDetails => Icon::EditDetailsTable,
        DockTab::Transitions => Icon::TransitionSplit,
        DockTab::VideoFx => Icon::FxSparkle,
        DockTab::ColorGrading => Icon::ColorWheels,
        DockTab::AudioFx => Icon::FxAudio,
        DockTab::Generators => Icon::GeneratorFrame,
        DockTab::Mixer => Icon::MixerFaders,
        DockTab::Loudness => Icon::LoudnessMeter,
        DockTab::SurroundPanner => Icon::SurroundSpeakers,
        DockTab::PluginManager => Icon::PluginPlug,
        DockTab::Compositors => Icon::Compositors,
        DockTab::Scopes => Icon::ScopesWave,
        DockTab::Notes => Icon::NotesPad,
        DockTab::Encoder => Icon::EncoderQueue,
        DockTab::Transcript => Icon::NotesPad,
    }
}

pub fn split_pane_chrome(rect: Rect, strip_h: f32, tabs_at_top: bool) -> (Rect, Rect) {
    let clamped_strip = strip_h.min((rect.height() * 0.52).max(24.0));
    if tabs_at_top {
        let strip = Rect::from_min_max(
            rect.min,
            pos2(
                rect.right(),
                (rect.top() + clamped_strip).min(rect.bottom()),
            ),
        );
        let content = Rect::from_min_max(
            pos2(rect.left(), (strip.bottom() + 2.0).min(rect.bottom())),
            rect.max,
        );
        (content, strip)
    } else {
        let content = Rect::from_min_max(
            rect.min,
            pos2(
                rect.right(),
                (rect.bottom() - clamped_strip - 2.0).max(rect.top() + 16.0),
            ),
        );
        let strip = Rect::from_min_max(
            pos2(rect.left(), (rect.bottom() - clamped_strip).max(rect.top())),
            rect.max,
        );
        (content, strip)
    }
}

/// Simplificação da árvore: a mesma no desenho e depois de soltar uma aba.
pub(crate) fn dock_simplification() -> SimplificationOptions {
    SimplificationOptions {
        all_panes_must_have_tabs: true,
        prune_empty_tabs: true,
        prune_empty_containers: true,
        prune_single_child_tabs: false,
        prune_single_child_containers: true,
        join_nested_linear_containers: true,
    }
}

/// O que `Ctrl+F11` escondeu, para restaurar depois.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelineRestore {
    pub dock_visible: bool,
    pub header_expanded: bool,
}

/// Constrói a árvore padrão de 3 colunas do VEGAS Pro (`Tabs | Prévia | Master`).
pub fn build_dock_tree(
    preview_ratio: f32,
    active_tab: DockTab,
    preview_on_left: bool,
) -> Tree<DockPane> {
    let mut tiles = Tiles::default();
    // Grupo à esquerda: os painéis padrão primeiro, depois os outros (que
    // começam fechados e aparecem aqui quando abertos pelo menu Exibir).
    let left_order: Vec<DockTab> = crate::layout::DEFAULT_DOCK_TABS
        .into_iter()
        .chain(DockTab::ALL.into_iter().filter(|t| {
            !crate::layout::DEFAULT_DOCK_TABS.contains(t)
                && *t != DockTab::Trimmer
                && !crate::layout::window_only(*t)
        }))
        .collect();
    let mut tab_ids = Vec::with_capacity(left_order.len());
    let mut active_id = None;
    for tab in left_order {
        let id = tiles.insert_pane(DockPane::Tab(tab));
        if tab == active_tab {
            active_id = Some(id);
        }
        tab_ids.push(id);
    }
    let mut tabs = Tabs::new(tab_ids);
    if let Some(id) = active_id {
        tabs.set_active(id);
    }
    let tabs_group = tiles.insert_container(Container::Tabs(tabs));

    // O Trimmer fica no mesmo grupo da Prévia de Vídeo.
    let pv_pane = tiles.insert_pane(DockPane::Preview);
    let trimmer_pane = tiles.insert_pane(DockPane::Tab(DockTab::Trimmer));
    let mut pv_tabs = Tabs::new(vec![pv_pane, trimmer_pane]);
    pv_tabs.set_active(if active_tab == DockTab::Trimmer {
        trimmer_pane
    } else {
        pv_pane
    });
    let pv_group = tiles.insert_container(Container::Tabs(pv_tabs));

    let mb_pane = tiles.insert_pane(DockPane::MasterBus);
    let mb_group = tiles.insert_tab_tile(vec![mb_pane]);

    let order = if preview_on_left {
        vec![pv_group, tabs_group, mb_group]
    } else {
        vec![tabs_group, pv_group, mb_group]
    };
    let mut linear = Linear::new(LinearDir::Horizontal, order);

    let pv_frac = preview_ratio.clamp(0.20, 0.72) * (1.0 - DEFAULT_MASTER_SHARE);
    let tabs_frac = (1.0 - DEFAULT_MASTER_SHARE - pv_frac).max(0.18);
    linear.shares.set_share(tabs_group, tabs_frac * 3.0);
    linear.shares.set_share(pv_group, pv_frac * 3.0);
    linear
        .shares
        .set_share(mb_group, DEFAULT_MASTER_SHARE * 3.0);

    let root = tiles.insert_container(Container::Linear(linear));
    Tree::new(TREE_SALT, root, tiles)
}

/// Valida uma árvore vinda de arquivo `.neoveglayout` contra ciclos, IDs inválidos,
/// panes faltantes ou duplicados e pesos inválidos (`AGENTS.md §0.4`).
pub fn sanitize_tile_tree(
    tree: &mut Tree<DockPane>,
    preview_ratio: f32,
    active_tab: DockTab,
    preview_on_left: bool,
) {
    let Some(root) = tree.root else {
        *tree = build_dock_tree(preview_ratio, active_tab, preview_on_left);
        return;
    };

    let mut visited = HashSet::new();
    let mut found_panes = HashSet::new();
    if !validate_subtree(&tree.tiles, root, 0, &mut visited, &mut found_panes) {
        *tree = build_dock_tree(preview_ratio, active_tab, preview_on_left);
        return;
    }

    // árvore salva antes do Encoder virar só janela: o painel dele fica na
    // árvore, sempre escondido, e não conta
    found_panes.retain(|p| !matches!(p, DockPane::Tab(t) if crate::layout::window_only(*t)));
    let expected = DockPane::all_canonical();
    if found_panes.len() != expected.len() || expected.iter().any(|p| !found_panes.contains(p)) {
        *tree = build_dock_tree(preview_ratio, active_tab, preview_on_left);
        return;
    }

    // Corrige eventuais shares não finitas ou não positivas em containers lineares.
    let container_ids: Vec<TileId> = visited.iter().copied().collect();
    for id in container_ids {
        if let Some(Tile::Container(Container::Linear(lin))) = tree.tiles.get_mut(id) {
            let children = lin.children.clone();
            for child in children {
                let s = lin.shares[child];
                let clean = if s.is_finite() && s > 0.0 {
                    s.clamp(0.05, 20.0)
                } else {
                    1.0
                };
                lin.shares.set_share(child, clean);
            }
        }
    }
}

fn validate_subtree(
    tiles: &Tiles<DockPane>,
    id: TileId,
    depth: usize,
    visited: &mut HashSet<TileId>,
    found_panes: &mut HashSet<DockPane>,
) -> bool {
    if depth > MAX_TREE_DEPTH || !visited.insert(id) {
        return false;
    }
    let Some(tile) = tiles.get(id) else {
        return false;
    };
    match tile {
        Tile::Pane(pane) => found_panes.insert(*pane),
        Tile::Container(container_ref) => {
            let children: Vec<TileId> = container_ref.children().copied().collect();
            if children.is_empty() {
                return false;
            }
            children
                .into_iter()
                .all(|child| validate_subtree(tiles, child, depth + 1, visited, found_panes))
        }
    }
}

pub(crate) fn find_pane_tile(tree: &Tree<DockPane>, target: DockPane) -> Option<TileId> {
    tree.tiles.iter().find_map(|(&id, tile)| match tile {
        Tile::Pane(p) if *p == target => Some(id),
        _ => None,
    })
}

fn sync_subtree_visibility(
    app: &App,
    tiles: &mut Tiles<DockPane>,
    id: TileId,
    show_preview: bool,
    show_master: bool,
    depth: usize,
    visited: &mut HashSet<TileId>,
) -> bool {
    if depth > MAX_TREE_DEPTH || !visited.insert(id) {
        return false;
    }
    enum NodeKind {
        Pane(DockPane),
        Container(Vec<TileId>),
    }
    let kind = match tiles.get(id) {
        Some(Tile::Pane(p)) => NodeKind::Pane(*p),
        Some(Tile::Container(c)) => NodeKind::Container(c.children().copied().collect()),
        None => return false,
    };
    let visible = match kind {
        NodeKind::Pane(DockPane::Tab(t)) => {
            !app.detached_tabs.contains(&t) && !app.closed_tabs.contains(&t)
        }
        NodeKind::Pane(DockPane::Preview) => show_preview,
        NodeKind::Pane(DockPane::MasterBus) => show_master,
        NodeKind::Container(children) => {
            let mut any = false;
            for child in children {
                if sync_subtree_visibility(
                    app,
                    tiles,
                    child,
                    show_preview,
                    show_master,
                    depth + 1,
                    visited,
                ) {
                    any = true;
                }
            }
            any
        }
    };
    tiles.set_visible(id, visible);
    visible
}

#[derive(Clone)]
struct TabsGroupSnapshot {
    container_id: TileId,
    siblings: Vec<(TileId, DockPane, bool)>,
}

fn collect_tabs_groups(tree: &Tree<DockPane>) -> HashMap<TileId, TabsGroupSnapshot> {
    let mut map = HashMap::new();
    for (&container_id, tile) in tree.tiles.iter() {
        let Tile::Container(Container::Tabs(tabs)) = tile else {
            continue;
        };
        let mut siblings = Vec::new();
        for &child_id in &tabs.children {
            if !tree.is_visible(child_id) {
                continue;
            }
            if let Some(Tile::Pane(pane)) = tree.tiles.get(child_id) {
                siblings.push((child_id, *pane, tabs.active == Some(child_id)));
            }
        }
        if siblings.is_empty() {
            continue;
        }
        for &(child_id, _, _) in &siblings {
            map.insert(
                child_id,
                TabsGroupSnapshot {
                    container_id,
                    siblings: siblings.clone(),
                },
            );
        }
    }
    map
}

struct NeovegTileBehavior<'a> {
    app: &'a mut App,
    tabs_at_top: bool,
    tabs_info: HashMap<TileId, TabsGroupSnapshot>,
    pending_activate: Option<(TileId, TileId, DockPane)>,
    pending_maximize: Option<DockPane>,
    /// Faixa de abas de cada grupo desenhada neste quadro (alvo de juntar).
    strips: Vec<(TileId, Rect)>,
    pending_detach: Vec<DockPane>,
    pending_detach_all: bool,
    pending_close: Vec<DockPane>,
    pending_toggle_preview: bool,
    pending_toggle_master: bool,
}

impl NeovegTileBehavior<'_> {
    fn queue_pane_context_menu(&mut self, ui: &mut Ui, pane: DockPane) {
        let lang = self.app.lang();
        let detach_label = neoveg_common::tr(
            lang,
            match pane {
                DockPane::Preview => "Destacar Prévia em janela do Windows",
                DockPane::MasterBus => "Destacar Master Bus em janela do Windows",
                DockPane::Tab(_) => "Destacar em janela do Windows",
            },
        );
        if ui.button(detach_label).clicked() {
            self.pending_detach.push(pane);
            ui.close_menu();
        }
        if ui
            .button(neoveg_common::tr(lang, "Maximizar na área de janelas"))
            .on_hover_text(neoveg_common::tr(
                lang,
                "Preenche a área de janelas; duplo clique na aba também",
            ))
            .clicked()
        {
            self.pending_maximize = Some(pane);
            ui.close_menu();
        }
        match pane {
            DockPane::Tab(_) => {
                if ui
                    .button(neoveg_common::tr(lang, "Fechar painel"))
                    .on_hover_text(neoveg_common::tr(
                        lang,
                        "Fecha o painel; ele volta pelo menu Exibir",
                    ))
                    .clicked()
                {
                    self.pending_close.push(pane);
                    ui.close_menu();
                }
                if ui
                    .button(neoveg_common::tr(lang, "Destacar todas as abas"))
                    .clicked()
                {
                    self.pending_detach_all = true;
                    ui.close_menu();
                }
                ui.separator();
                let pv_label = neoveg_common::tr(
                    lang,
                    if self.app.preview_detached {
                        "Acoplar Prévia de Vídeo"
                    } else {
                        "Destacar Prévia de Vídeo em janela do Windows"
                    },
                );
                if ui.button(pv_label).clicked() {
                    self.pending_toggle_preview = true;
                    ui.close_menu();
                }
                let mb_label = neoveg_common::tr(
                    lang,
                    if self.app.master_bus_detached {
                        "Acoplar Master Bus"
                    } else {
                        "Destacar Master Bus em janela do Windows"
                    },
                );
                if ui.button(mb_label).clicked() {
                    self.pending_toggle_master = true;
                    ui.close_menu();
                }
            }
            DockPane::Preview | DockPane::MasterBus => {
                if ui
                    .button(neoveg_common::tr(lang, "Fechar painel"))
                    .clicked()
                {
                    self.pending_close.push(pane);
                    ui.close_menu();
                }
            }
        }
    }

    fn draw_bottom_tab_strip(&mut self, ui: &mut Ui, tile_id: TileId, pane: DockPane) {
        let snapshot = self
            .tabs_info
            .get(&tile_id)
            .cloned()
            .unwrap_or_else(|| TabsGroupSnapshot {
                container_id: tile_id,
                siblings: vec![(tile_id, pane, true)],
            });

        let outer_rect = ui.available_rect_before_wrap();
        let inner = outer_rect.shrink(2.0);
        let max_w = inner.width().max(80.0);
        let font_id = egui::TextStyle::Button.resolve(ui.style());
        let item_sp = ui.spacing().item_spacing;
        let text_fg = self.app.theme.text_primary;

        let lang = self.app.lang();
        let mut tabs_est_w = 0.0_f32;
        let mut btn_h = 20.0_f32;
        for (idx, (_, sib_pane, _)) in snapshot.siblings.iter().enumerate() {
            let g = ui.painter().layout_no_wrap(
                sib_pane.title_in(lang).to_owned(),
                font_id.clone(),
                text_fg,
            );
            let item_w = (6.0 + 13.0 + g.size().x + 8.0).max(24.0);
            btn_h = btn_h.max((4.0 + g.size().y.max(13.0)).max(19.0));
            if idx > 0 {
                tabs_est_w += item_sp.x;
            }
            tabs_est_w += item_w;
        }

        let has_close = matches!(pane, DockPane::Preview | DockPane::MasterBus);
        let detach_txt = neoveg_common::tr(lang, "Destacar");
        let action_w = if has_close {
            // [PopOut] + [CloseX]
            22.0 + item_sp.x + 22.0
        } else {
            let g = ui
                .painter()
                .layout_no_wrap(detach_txt.to_owned(), font_id, text_fg);
            btn_h = btn_h.max((4.0 + g.size().y.max(13.0)).max(19.0));
            (6.0 + 13.0 + g.size().x + 8.0).max(24.0)
        };

        let row_h = btn_h.ceil();
        let avail_tabs_w = (max_w - action_w - item_sp.x - 4.0).max(32.0);
        let cached_w_id = egui::Id::new(("bot_tab_content_w", snapshot.container_id));
        let prev_tabs_w = ui
            .ctx()
            .data_mut(|d| d.get_temp::<f32>(cached_w_id).unwrap_or(tabs_est_w));
        let overflow =
            snapshot.siblings.len() > 1 && prev_tabs_w.max(tabs_est_w) > avail_tabs_w + 1.0;

        let bar_h = if overflow {
            crate::scrollbars::TAB_SCROLL_BAR_H
        } else {
            0.0
        };
        let strip_h = 3.0 + row_h + if overflow { 2.0 + bar_h } else { 0.0 } + 3.0;
        let (content_rect, strip_rect) = split_pane_chrome(inner, strip_h, false);
        self.strips.push((snapshot.container_id, strip_rect));

        let mut content_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .id_salt(("tile_content_bot", tile_id)),
        );
        content_ui.set_clip_rect(content_rect);
        self.render_pane_body(&mut content_ui, pane);

        ui.painter().hline(
            (outer_rect.left() + 1.0)..=(outer_rect.right() - 1.0),
            strip_rect.top(),
            Stroke::new(1.0_f32, self.app.theme.border),
        );

        let row_top = strip_rect.top() + 3.0;
        let row_bottom = (row_top + row_h).min(strip_rect.bottom());
        let actions_left = (strip_rect.right() - action_w - 2.0).max(strip_rect.left() + 36.0);
        let tabs_rect = egui::Rect::from_min_max(
            egui::pos2(strip_rect.left() + 2.0, row_top),
            egui::pos2(
                (actions_left - 4.0).max(strip_rect.left() + 4.0),
                row_bottom,
            ),
        );
        let actions_rect = egui::Rect::from_min_max(
            egui::pos2(actions_left, row_top),
            egui::pos2(strip_rect.right() - 2.0, row_bottom),
        );

        let active_sib = snapshot
            .siblings
            .iter()
            .find(|(_, _, active)| *active)
            .map(|&(id, p, _)| (id, p))
            .unwrap_or((tile_id, pane));

        let active_cache_id = egui::Id::new(("bot_tab_last_active", snapshot.container_id));
        let prev_active = ui.ctx().data_mut(|d| d.get_temp::<TileId>(active_cache_id));
        let active_changed = prev_active != Some(active_sib.0);
        ui.ctx()
            .data_mut(|d| d.insert_temp(active_cache_id, active_sib.0));

        let mut tabs_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(tabs_rect)
                .id_salt(("tile_strip_tabs", snapshot.container_id)),
        );
        tabs_ui.set_clip_rect(tabs_rect);
        let scroll_out = egui::ScrollArea::horizontal()
            .id_salt(("tile_strip_scroll", snapshot.container_id))
            .auto_shrink([false, false])
            .max_width(tabs_rect.width())
            .max_height(tabs_rect.height())
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(&mut tabs_ui, |ui| {
                ui.horizontal(|ui| {
                    for &(sib_id, sib_pane, active) in &snapshot.siblings {
                        let resp = icons::small_icon_drag_button(
                            ui,
                            sib_pane.icon(),
                            Some(sib_pane.title_in(lang)),
                            active,
                            &self.app.theme,
                        )
                        .on_hover_text(neoveg_common::tr(
                            lang,
                            "Clique para selecionar; arraste para os alvos para acoplar ou para fora da área para destacar; duplo clique maximiza",
                        ));
                        if active && active_changed {
                            resp.scroll_to_me(Some(egui::Align::Center));
                        }
                        if resp.clicked() {
                            self.pending_activate = Some((snapshot.container_id, sib_id, sib_pane));
                        }
                        if resp.double_clicked() {
                            self.pending_maximize = Some(sib_pane);
                        }
                        if resp.drag_started() {
                            crate::dock_drop::begin(
                                ui.ctx(),
                                crate::dock_drop::DockDrag {
                                    tile: sib_id,
                                    pane: sib_pane,
                                    last: ui.ctx().pointer_latest_pos(),
                                },
                            );
                        }
                        resp.context_menu(|ui| self.queue_pane_context_menu(ui, sib_pane));
                    }
                });
            });

        let measured_content_w = scroll_out.content_size.x.max(tabs_est_w);
        ui.ctx()
            .data_mut(|d| d.insert_temp(cached_w_id, measured_content_w));

        let mut actions_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(actions_rect)
                .id_salt(("tile_strip_actions", snapshot.container_id)),
        );
        actions_ui.set_clip_rect(actions_rect);
        actions_ui.horizontal(|ui| {
            if icons::small_icon_button(
                ui,
                Icon::PopOut,
                (!has_close).then_some(detach_txt),
                false,
                &self.app.theme,
            )
            .on_hover_text(neoveg_common::tr(
                lang,
                "Destacar em janela normal do Windows",
            ))
            .clicked()
            {
                self.pending_detach.push(active_sib.1);
            }
            if has_close
                && icons::small_icon_button(ui, Icon::CloseX, None, false, &self.app.theme)
                    .on_hover_text(neoveg_common::tr(lang, "Fechar painel"))
                    .clicked()
            {
                self.pending_close.push(active_sib.1);
            }
        });

        let tab_range = crate::scrollbars::ScrollbarRange {
            offset: scroll_out.state.offset.x,
            content: measured_content_w,
            viewport: tabs_rect.width(),
        };

        let mut next_scroll_x = None;
        if overflow && strip_rect.bottom() - row_bottom >= bar_h + 2.0 {
            let bar_rect = egui::Rect::from_min_max(
                egui::pos2(strip_rect.left() + 2.0, row_bottom + 2.0),
                egui::pos2(strip_rect.right() - 2.0, row_bottom + 2.0 + bar_h),
            );
            let step = (tabs_rect.width() * 0.35).clamp(48.0, 160.0);
            let outcome = crate::scrollbars::arrow_scrollbar(
                ui,
                egui::Id::new(("tile_strip_hbar", snapshot.container_id)),
                bar_rect,
                crate::scrollbars::Axis::Horizontal,
                tab_range,
                step,
                neoveg_common::tr(lang, "Rolar abas para a esquerda"),
                neoveg_common::tr(lang, "Rolar abas para a direita"),
                &self.app.theme,
            );
            if outcome.new_offset.is_some() {
                next_scroll_x = outcome.new_offset;
            }
        }

        // Roda do mouse sobre a faixa de abas rola horizontalmente.
        if tab_range.is_scrollable()
            && ui
                .ctx()
                .pointer_hover_pos()
                .is_some_and(|p| strip_rect.contains(p))
        {
            let delta = ui.input(|i| i.smooth_scroll_delta);
            let wheel = if delta.x.abs() > 0.1 {
                -delta.x
            } else if delta.y.abs() > 0.1 {
                -delta.y
            } else {
                0.0
            };
            if wheel.abs() > 0.1 {
                let cur = next_scroll_x.unwrap_or_else(|| tab_range.sanitized_offset());
                next_scroll_x = Some((cur + wheel).clamp(0.0, tab_range.max_offset()));
            }
        }

        if let Some(sx) = next_scroll_x {
            let mut st = scroll_out.state;
            st.offset.x = sx;
            st.store(ui.ctx(), scroll_out.id);
            ui.ctx().request_repaint();
        }
    }

    fn render_pane_body(&mut self, ui: &mut Ui, pane: DockPane) {
        match pane {
            DockPane::Tab(tab) => {
                let theme = self.app.theme.clone();
                let lang = self.app.lang();
                crate::scrollbars::arrow_scroll_area(
                    ui,
                    egui::Id::new(("dock_tab_scroll", tab.command_id())),
                    &theme,
                    true,
                    lang,
                    |ui| self.app.show_dock_tab_contents(ui, tab),
                );
            }
            DockPane::Preview => self.app.preview_panel(ui, false),
            DockPane::MasterBus => self.app.master_bus_panel(ui, false),
        }
    }
}

impl Behavior<DockPane> for NeovegTileBehavior<'_> {
    fn pane_ui(&mut self, ui: &mut Ui, tile_id: TileId, pane: &mut DockPane) -> UiResponse {
        if self.tabs_at_top {
            let rect = ui.available_rect_before_wrap();
            let inner = rect.shrink(2.0);
            let mut body_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(inner)
                    .id_salt(("tile_content_top", tile_id)),
            );
            body_ui.set_clip_rect(inner);
            self.render_pane_body(&mut body_ui, *pane);
        } else {
            self.draw_bottom_tab_strip(ui, tile_id, *pane);
        }
        UiResponse::None
    }

    fn tab_title_for_pane(&mut self, pane: &DockPane) -> WidgetText {
        pane.title_in(self.app.lang()).into()
    }

    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        if self.tabs_at_top {
            26.0
        } else {
            0.0
        }
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        3.0
    }

    fn min_size(&self) -> f32 {
        96.0
    }

    fn simplification_options(&self) -> SimplificationOptions {
        dock_simplification()
    }

    fn retain_pane(&mut self, _pane: &DockPane) -> bool {
        true
    }

    fn is_tab_closable(&self, _tiles: &Tiles<DockPane>, _tile_id: TileId) -> bool {
        false
    }

    fn tab_bar_color(&self, _visuals: &egui::Visuals) -> Color32 {
        self.app.theme.panel_fill
    }

    fn tab_bg_color(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &Tiles<DockPane>,
        _tile_id: TileId,
        state: &TabState,
    ) -> Color32 {
        if state.active {
            self.app.theme.selection_bg
        } else {
            self.app.theme.widget_bg
        }
    }

    fn tab_outline_stroke(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &Tiles<DockPane>,
        _tile_id: TileId,
        state: &TabState,
    ) -> Stroke {
        Stroke::new(
            1.0_f32,
            if state.active {
                self.app.theme.accent
            } else {
                self.app.theme.border
            },
        )
    }

    fn tab_text_color(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &Tiles<DockPane>,
        _tile_id: TileId,
        state: &TabState,
    ) -> Color32 {
        if state.active {
            crate::theme::contrast_on(self.app.theme.selection_bg)
        } else {
            self.app.theme.text_primary
        }
    }

    fn resize_stroke(&self, _style: &egui::Style, resize_state: ResizeState) -> Stroke {
        match resize_state {
            ResizeState::Idle => Stroke::new(1.5_f32, self.app.theme.border),
            ResizeState::Hovering | ResizeState::Dragging => {
                Stroke::new(3.0_f32, self.app.theme.accent)
            }
        }
    }

    fn drag_preview_stroke(&self, _visuals: &egui::Visuals) -> Stroke {
        Stroke::new(2.0_f32, self.app.theme.accent)
    }

    fn drag_preview_color(&self, _visuals: &egui::Visuals) -> Color32 {
        self.app.theme.selection_bg.gamma_multiply(0.35)
    }

    fn paint_on_top_of_tile(
        &self,
        painter: &egui::Painter,
        _style: &egui::Style,
        tile_id: TileId,
        rect: Rect,
    ) {
        if self.tabs_info.values().any(|s| s.container_id == tile_id) {
            painter.rect_stroke(
                rect,
                2.0,
                Stroke::new(1.0_f32, self.app.theme.border),
                StrokeKind::Inside,
            );
        }
    }

    fn tab_ui(
        &mut self,
        tiles: &mut Tiles<DockPane>,
        ui: &mut Ui,
        id: egui::Id,
        tile_id: TileId,
        state: &TabState,
    ) -> egui::Response {
        if !self.tabs_at_top {
            let (_, rect) = ui.allocate_space(egui::Vec2::ZERO);
            return ui.interact(rect, id, Sense::hover());
        }
        let Some(&Tile::Pane(pane)) = tiles.get(tile_id) else {
            let (_, rect) = ui.allocate_space(vec2(64.0, ui.available_height()));
            return ui.interact(rect, id, Sense::click_and_drag());
        };
        let label = pane.title_in(self.app.lang());
        let font_id = egui::TextStyle::Button.resolve(ui.style());
        let text_color = self.tab_text_color(ui.visuals(), tiles, tile_id, state);
        let galley = ui
            .painter()
            .layout_no_wrap(label.to_string(), font_id, text_color);
        let icon_sz = 13.0;
        let x_pad = 7.0;
        let w = x_pad * 2.0 + icon_sz + 5.0 + galley.size().x;
        let (_, tab_rect) = ui.allocate_space(vec2(w, ui.available_height()));

        // Id próprio: o `egui_tiles` não vê este arraste; a queda é resolvida em
        // `dock_drop` (5 alvos sobre o grupo sob o ponteiro).
        let resp = ui
            .interact(tab_rect, id.with("neoveg_tab"), Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::Grab);
        if resp.drag_started() {
            crate::dock_drop::begin(
                ui.ctx(),
                crate::dock_drop::DockDrag {
                    tile: tile_id,
                    pane,
                    last: ui.ctx().pointer_latest_pos(),
                },
            );
        }
        if resp.double_clicked() {
            self.pending_maximize = Some(pane);
        }
        resp.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
        });

        if ui.is_rect_visible(tab_rect) && !state.is_being_dragged {
            let bg = if state.active {
                self.app.theme.selection_bg
            } else if resp.hovered() {
                self.app.theme.widget_hover_bg
            } else {
                self.app.theme.widget_bg
            };
            let stroke = self.tab_outline_stroke(ui.visuals(), tiles, tile_id, state);
            ui.painter()
                .rect(tab_rect.shrink(0.5), 2.0, bg, stroke, StrokeKind::Inside);

            let iy = tab_rect.center().y - icon_sz * 0.5;
            let icon_rect =
                Rect::from_min_size(pos2(tab_rect.left() + x_pad, iy), vec2(icon_sz, icon_sz));
            let mut icon_theme = self.app.theme.clone();
            if state.active && crate::theme::relative_luminance(bg) < 0.35 {
                icon_theme.dark_mode = true;
                if icon_theme.high_contrast {
                    icon_theme.text_primary = text_color;
                    icon_theme.accent = text_color;
                }
            }
            icons::paint(ui.painter(), icon_rect, pane.icon(), &icon_theme, true);
            ui.painter().galley(
                pos2(
                    icon_rect.right() + 5.0,
                    tab_rect.center().y - galley.size().y * 0.5,
                ),
                galley,
                text_color,
            );
        }

        if resp.clicked() {
            if let DockPane::Tab(t) = pane {
                self.app.dock_tab = t;
            }
        }
        resp.context_menu(|ui| self.queue_pane_context_menu(ui, pane));
        resp
    }

    fn top_bar_right_ui(
        &mut self,
        tiles: &Tiles<DockPane>,
        ui: &mut Ui,
        _tile_id: TileId,
        tabs: &Tabs,
        _scroll_offset: &mut f32,
    ) {
        if !self.tabs_at_top {
            return;
        }
        let Some(active_id) = tabs.active else {
            return;
        };
        let Some(&Tile::Pane(pane)) = tiles.get(active_id) else {
            return;
        };
        let lang = self.app.lang();
        if matches!(pane, DockPane::Preview | DockPane::MasterBus)
            && icons::small_icon_button(ui, Icon::CloseX, None, false, &self.app.theme)
                .on_hover_text(neoveg_common::tr(lang, "Fechar painel"))
                .clicked()
        {
            self.pending_close.push(pane);
        }
        if icons::small_icon_button(ui, Icon::PopOut, None, false, &self.app.theme)
            .on_hover_text(neoveg_common::tr(
                lang,
                "Destacar em janela normal do Windows",
            ))
            .clicked()
        {
            self.pending_detach.push(pane);
        }
    }
}

pub(crate) fn draw_dock_tree(app: &mut App, ui: &mut Ui) {
    let avail = ui.available_rect_before_wrap();
    ui.allocate_rect(avail, Sense::hover());
    if avail.width() < 8.0 || avail.height() < 8.0 {
        return;
    }

    let show_preview = app.preview_visible && !app.preview_detached;
    let min_w_for_master = if show_preview { 640.0 } else { 420.0 };
    let show_master =
        app.master_bus_visible && !app.master_bus_detached && avail.width() >= min_w_for_master;

    let mut tree = std::mem::replace(&mut app.dock_tree, Tree::empty(TREE_SALT));

    if let Some(root) = tree.root {
        let mut visited = HashSet::new();
        let any_visible = sync_subtree_visibility(
            app,
            &mut tree.tiles,
            root,
            show_preview,
            show_master,
            0,
            &mut visited,
        );
        if !any_visible {
            let lang = app.lang();
            crate::dock_drop::end(ui.ctx());
            app.dock_tree = tree;
            ui.label(
                egui::RichText::new(neoveg_common::tr(
                    lang,
                    "Todas as abas estão destacadas em janelas flutuantes (clique em Acoplar ou use Exibir/Layouts).",
                ))
                .weak(),
            );
            return;
        }
    }

    if let Some(focus_tab) = app.pending_dock_focus.take() {
        tree.make_active(|_, tile| matches!(tile, Tile::Pane(DockPane::Tab(t)) if *t == focus_tab));
    }

    // Painel maximizado: preenche a área de janelas até restaurar (manual do
    // VEGAS Pro 21, "Expanding a docked window"). O painel continua na árvore.
    if let Some(max) = app.dock_maximized {
        let shown = match max {
            DockPane::Tab(t) => !app.detached_tabs.contains(&t),
            DockPane::Preview => show_preview,
            DockPane::MasterBus => show_master,
        };
        if shown && find_pane_tile(&tree, max).is_some() {
            crate::dock_drop::end(ui.ctx());
            app.dock_tree = tree;
            draw_maximized_pane(app, ui, avail, max);
            return;
        }
        app.dock_maximized = None;
    }

    let tabs_info = collect_tabs_groups(&tree);
    let tabs_at_top = app.prefs.edit_layout.tabs_at_top;

    let mut ui_child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(avail)
            .id_salt("dock_tree_area"),
    );
    let (
        pending_activate,
        pending_maximize,
        strips,
        mut pending_detach,
        pending_detach_all,
        pending_close,
        pending_toggle_preview,
        pending_toggle_master,
    ) = {
        let mut behavior = NeovegTileBehavior {
            app,
            tabs_at_top,
            tabs_info,
            pending_activate: None,
            pending_maximize: None,
            strips: Vec::new(),
            pending_detach: Vec::new(),
            pending_detach_all: false,
            pending_close: Vec::new(),
            pending_toggle_preview: false,
            pending_toggle_master: false,
        };
        tree.ui(&mut behavior, &mut ui_child);
        (
            behavior.pending_activate,
            behavior.pending_maximize,
            behavior.strips,
            behavior.pending_detach,
            behavior.pending_detach_all,
            behavior.pending_close,
            behavior.pending_toggle_preview,
            behavior.pending_toggle_master,
        )
    };

    if let Some((container_id, tab_id, pane)) = pending_activate {
        if let Some(Tile::Container(Container::Tabs(tabs))) = tree.tiles.get_mut(container_id) {
            tabs.set_active(tab_id);
        }
        if let DockPane::Tab(t) = pane {
            app.dock_tab = t;
        }
    } else if !app.detached_tabs.contains(&app.dock_tab) {
        // Garante que `app.dock_tab` acompanhe caso o usuário troque de aba pela barra superior.
        if let Some(active_id) = tree.active_tiles().into_iter().find(|&id| {
            matches!(
                tree.tiles.get(id),
                Some(Tile::Pane(DockPane::Tab(t))) if *t != app.dock_tab
            )
        }) {
            if let Some(&Tile::Pane(DockPane::Tab(t))) = tree.tiles.get(active_id) {
                if tree.active_tiles().into_iter().all(|id| {
                    !matches!(tree.tiles.get(id), Some(Tile::Pane(DockPane::Tab(cur))) if *cur == app.dock_tab)
                }) {
                    app.dock_tab = t;
                }
            }
        }
    }

    if let Some(p) = pending_maximize {
        app.dock_maximized = Some(p);
    }
    if let Some(p) = handle_tab_drag(app, ui.ctx(), &mut tree, avail, &strips, tabs_at_top) {
        pending_detach.push(p);
    }

    if let Some(pv_id) = find_pane_tile(&tree, DockPane::Preview) {
        if let Some(pv_rect) = tree.tiles.rect(pv_id) {
            let denom = (avail.width() * (1.0 - DEFAULT_MASTER_SHARE)).max(120.0);
            let measured = (pv_rect.width() / denom).clamp(0.20, 0.72);
            if (measured - app.preview_ratio).abs() > 0.01 {
                app.preview_ratio = measured;
            }
        }
    }

    app.dock_tree = tree;

    if pending_detach_all {
        for tab in DockTab::ALL {
            app.detached_tabs.insert(tab);
        }
    } else {
        for pane in pending_detach {
            match pane {
                DockPane::Tab(t) => app.detach_dock_tab(t),
                DockPane::Preview => app.preview_detached = true,
                DockPane::MasterBus => app.master_bus_detached = true,
            }
        }
    }
    for pane in pending_close {
        match pane {
            DockPane::Tab(t) => app.close_dock_tab(t),
            DockPane::Preview => app.preview_visible = false,
            DockPane::MasterBus => app.master_bus_visible = false,
        }
    }
    if pending_toggle_preview {
        app.preview_detached = !app.preview_detached;
        if !app.preview_detached {
            app.preview_visible = true;
        }
    }
    if pending_toggle_master {
        app.master_bus_detached = !app.master_bus_detached;
        if !app.master_bus_detached {
            app.master_bus_visible = true;
        }
    }
}

/// Painel maximizado com a barra de restaurar.
fn draw_maximized_pane(app: &mut App, ui: &mut Ui, avail: Rect, pane: DockPane) {
    let bar_h = 24.0_f32.min(avail.height());
    let bar = Rect::from_min_max(avail.min, pos2(avail.right(), avail.top() + bar_h));
    let body = Rect::from_min_max(pos2(avail.left(), bar.bottom() + 2.0), avail.max);
    let mut restore = false;
    let mut bar_ui = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(4.0, 2.0))));
    bar_ui.horizontal(|ui| {
        let mut icon_rect = ui.available_rect_before_wrap();
        icon_rect.set_width(14.0);
        icon_rect.set_height(14.0);
        let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
        icons::paint(ui.painter(), r, pane.icon(), &app.theme, true);
        let title = ui
            .add(egui::Label::new(egui::RichText::new(pane.title()).strong()).sense(Sense::click()))
            .on_hover_text("Duplo clique para restaurar");
        if title.double_clicked() {
            restore = true;
        }
        if ui
            .button("Restaurar")
            .on_hover_text("Volta o painel ao tamanho de antes na área de janelas")
            .clicked()
        {
            restore = true;
        }
    });
    ui.painter().hline(
        avail.left()..=avail.right(),
        bar.bottom() + 1.0,
        Stroke::new(1.0_f32, app.theme.border),
    );
    if body.height() > 8.0 {
        let mut body_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(body)
                .id_salt("dock_maximized_body"),
        );
        body_ui.set_clip_rect(body);
        let mut behavior = NeovegTileBehavior {
            app,
            tabs_at_top: false,
            tabs_info: HashMap::new(),
            pending_activate: None,
            pending_maximize: None,
            strips: Vec::new(),
            pending_detach: Vec::new(),
            pending_detach_all: false,
            pending_close: Vec::new(),
            pending_toggle_preview: false,
            pending_toggle_master: false,
        };
        behavior.render_pane_body(&mut body_ui, pane);
    }
    if restore {
        app.dock_maximized = None;
    }
}

/// Arraste de uma aba: desenha os 5 alvos sobre o grupo sob o ponteiro e, ao
/// soltar, acopla ali. Solto fora da área de janelas, devolve o painel para
/// destacar numa janela própria. Esc cancela.
fn handle_tab_drag(
    app: &App,
    ctx: &egui::Context,
    tree: &mut Tree<DockPane>,
    dock_rect: Rect,
    strips: &[(TileId, Rect)],
    tabs_at_top: bool,
) -> Option<DockPane> {
    use crate::dock_drop::{self, DropZone};
    let drag = dock_drop::current(ctx)?;
    // A árvore pode ter sido trocada no meio do arraste (layout carregado).
    if !matches!(tree.tiles.get(drag.tile), Some(Tile::Pane(p)) if *p == drag.pane)
        || ctx.input(|i| i.key_pressed(egui::Key::Escape))
    {
        dock_drop::end(ctx);
        return None;
    }
    let latest = ctx.input(|i| i.pointer.latest_pos());
    let pointer = latest.or(drag.last);
    let released = ctx.input(|i| i.pointer.primary_released() || !i.pointer.primary_down());
    let hover = pointer.and_then(|p| {
        let (group, rect) = dock_drop::group_under(tree, p)?;
        let strip = strips
            .iter()
            .find(|(id, _)| *id == group)
            .map(|(_, r)| *r)
            .or_else(|| {
                tabs_at_top
                    .then(|| Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 26.0)))
            });
        dock_drop::drop_zone(rect, strip, p).map(|z| (group, rect, z))
    });
    if released {
        dock_drop::end(ctx);
        match (hover, pointer) {
            (Some((group, _, zone)), _) => {
                if dock_drop::apply_drop(tree, drag.tile, group, zone, &dock_simplification()) {
                    tree.make_active(|id, _| id == drag.tile);
                }
                None
            }
            (None, Some(p)) if !dock_rect.contains(p) => Some(drag.pane),
            // Soltou fora da janela sem posição conhecida: destaca.
            (None, None) => Some(drag.pane),
            _ => None,
        }
    } else {
        if latest.is_some() && latest != drag.last {
            dock_drop::begin(
                ctx,
                dock_drop::DockDrag {
                    last: latest,
                    ..drag
                },
            );
        }
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("neoveg_dock_drop_overlay"),
        ));
        let theme = &app.theme;
        if let Some((_, rect, zone)) = hover {
            painter.rect(
                dock_drop::preview_rect(rect, zone).shrink(2.0),
                2.0,
                theme.selection_bg.gamma_multiply(0.30),
                Stroke::new(2.0_f32, theme.accent),
                StrokeKind::Inside,
            );
            for (z, r) in dock_drop::marker_rects(rect) {
                let on = z == zone;
                painter.rect(
                    r,
                    3.0,
                    if on {
                        theme.selection_bg
                    } else {
                        theme.panel_fill
                    },
                    Stroke::new(1.5_f32, if on { theme.accent } else { theme.border }),
                    StrokeKind::Inside,
                );
                // Miniatura do resultado: a parte cheia é onde a janela fica.
                let inner = r.shrink(r.width() * 0.22);
                painter.rect_stroke(
                    inner,
                    1.0,
                    Stroke::new(1.0_f32, theme.text_primary),
                    StrokeKind::Inside,
                );
                let part = if z == DropZone::Center {
                    inner.shrink(inner.width() * 0.2)
                } else {
                    dock_drop::preview_rect(inner, z)
                };
                painter.rect_filled(part, 0.0, theme.text_primary.gamma_multiply(0.75));
            }
        }
        if let Some(p) = pointer {
            let font = egui::TextStyle::Button.resolve(&ctx.style());
            let label = match hover {
                Some(_) => drag.pane.title().to_owned(),
                None if !dock_rect.contains(p) => format!("{} (destacar)", drag.pane.title()),
                None => drag.pane.title().to_owned(),
            };
            let galley = painter.layout_no_wrap(label, font, theme.text_primary);
            let at = p + vec2(14.0, 14.0);
            let bg = Rect::from_min_size(at, galley.size()).expand(4.0);
            painter.rect(
                bg,
                3.0,
                theme.panel_fill,
                Stroke::new(1.0_f32, theme.border),
                StrokeKind::Inside,
            );
            painter.galley(at, galley, theme.text_primary);
        }
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        ctx.request_repaint();
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arvore_hostil_com_ciclo_e_corrupcao_recupera_limpamente() {
        let mut tree = build_dock_tree(0.40, DockTab::ProjectMedia, false);
        let root = tree.root.unwrap();
        // Injeta ciclo e remove uma aba para simular JSON adulterado.
        if let Some(Tile::Container(Container::Linear(lin))) = tree.tiles.get_mut(root) {
            lin.children.push(root);
        }
        sanitize_tile_tree(&mut tree, 0.40, DockTab::ProjectMedia, false);

        assert!(tree.root.is_some());
        let mut visited = HashSet::new();
        let mut found = HashSet::new();
        assert!(validate_subtree(
            &tree.tiles,
            tree.root.unwrap(),
            0,
            &mut visited,
            &mut found
        ));
        assert_eq!(found.len(), DockPane::all_canonical().len());
    }

    /// Layout salvo quando o Encoder ainda era aba do dock: a árvore continua
    /// válida (não é refeita) e a árvore nova não tem o Encoder.
    #[test]
    fn arvore_antiga_com_encoder_continua_valida_e_a_nova_nao_tem_encoder() {
        let mut tree = build_dock_tree(0.40, DockTab::ProjectMedia, false);
        assert!(find_pane_tile(&tree, DockPane::Tab(DockTab::Encoder)).is_none());
        let media = find_pane_tile(&tree, DockPane::Tab(DockTab::ProjectMedia)).expect("mídia");
        let grupo = tree.tiles.parent_of(media).expect("grupo");
        let enc = tree.tiles.insert_pane(DockPane::Tab(DockTab::Encoder));
        if let Some(Tile::Container(Container::Tabs(tabs))) = tree.tiles.get_mut(grupo) {
            tabs.add_child(enc);
        }
        let antes: Vec<TileId> = tree.tiles.tile_ids().collect();
        sanitize_tile_tree(&mut tree, 0.40, DockTab::ProjectMedia, false);
        let depois: Vec<TileId> = tree.tiles.tile_ids().collect();
        assert_eq!(antes.len(), depois.len(), "a árvore antiga foi refeita");
        assert!(find_pane_tile(&tree, DockPane::Tab(DockTab::Encoder)).is_some());
    }
}
