//! Arraste de abas na área de janelas acopláveis.
//!
//! Ao arrastar uma aba, aparecem 5 alvos sobre o grupo sob o ponteiro: o do
//! meio junta a aba ao grupo e os 4 dos lados dividem o grupo e põem a aba ali.
//! Um contorno mostra onde a janela vai ficar antes de soltar (manual do
//! VEGAS Pro 21, "Window Docking Area and Floating Window Docks": alvos grandes
//! e bem definidos e um contorno de onde a janela vai acoplar). Soltar fora da
//! área de janelas destaca a aba numa janela própria ("Undocking a window").
//! Esc cancela.
//!
//! A queda é resolvida aqui, e não pelo `egui_tiles`: o `egui_tiles` escolhe o
//! destino pelo centro mais próximo entre todos os blocos, que nem sempre é o
//! grupo sob o ponteiro.

use crate::dock::DockPane;
use eframe::egui;
use egui::{pos2, vec2, Pos2, Rect};
use egui_tiles::{Container, Linear, LinearDir, Tile, TileId, Tree};

/// Onde a aba arrastada vai ficar em relação ao grupo de destino.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropZone {
    /// Junta a aba ao grupo.
    Center,
    Left,
    Right,
    Top,
    Bottom,
}

/// Aba sendo arrastada: o bloco do painel, o painel e a última posição
/// conhecida do ponteiro (fora da janela o egui pode não ter posição ao soltar).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockDrag {
    pub tile: TileId,
    pub pane: DockPane,
    pub last: Option<Pos2>,
}

fn drag_id() -> egui::Id {
    egui::Id::new("neoveg_dock_tab_drag")
}

pub fn current(ctx: &egui::Context) -> Option<DockDrag> {
    ctx.data(|d| d.get_temp::<DockDrag>(drag_id()))
}

pub fn begin(ctx: &egui::Context, drag: DockDrag) {
    ctx.data_mut(|d| d.insert_temp(drag_id(), drag));
}

pub fn end(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<DockDrag>(drag_id()));
}

/// Os 5 marcadores, em cruz no centro do grupo.
pub fn marker_rects(group: Rect) -> [(DropZone, Rect); 5] {
    let side = (group.width().min(group.height()) / 5.0).clamp(18.0, 40.0);
    let step = side + 4.0;
    let c = group.center();
    let at = |dx: f32, dy: f32| Rect::from_center_size(pos2(c.x + dx, c.y + dy), vec2(side, side));
    [
        (DropZone::Center, at(0.0, 0.0)),
        (DropZone::Left, at(-step, 0.0)),
        (DropZone::Right, at(step, 0.0)),
        (DropZone::Top, at(0.0, -step)),
        (DropZone::Bottom, at(0.0, step)),
    ]
}

/// Zona sob o ponteiro. A faixa de abas do grupo (`strip`) e o marcador do
/// meio juntam a aba ao grupo ("drop near the top of the window to create a
/// tabbed window"); os marcadores dos lados dividem; fora deles, a faixa de
/// 25% junto de cada borda vale aquele lado e o resto vale o meio.
pub fn drop_zone(group: Rect, strip: Option<Rect>, pointer: Pos2) -> Option<DropZone> {
    if !group.is_finite()
        || !pointer.x.is_finite()
        || !pointer.y.is_finite()
        || group.width() < 1.0
        || group.height() < 1.0
        || !group.contains(pointer)
    {
        return None;
    }
    if strip.is_some_and(|s| s.is_finite() && s.contains(pointer)) {
        return Some(DropZone::Center);
    }
    if let Some((zone, _)) = marker_rects(group)
        .into_iter()
        .find(|(_, r)| r.contains(pointer))
    {
        return Some(zone);
    }
    let fx = (pointer.x - group.left()) / group.width();
    let fy = (pointer.y - group.top()) / group.height();
    let edges = [
        (DropZone::Left, fx),
        (DropZone::Right, 1.0 - fx),
        (DropZone::Top, fy),
        (DropZone::Bottom, 1.0 - fy),
    ];
    let (zone, dist) = edges
        .into_iter()
        .fold((DropZone::Center, f32::INFINITY), |best, (z, d)| {
            if d < best.1 {
                (z, d)
            } else {
                best
            }
        });
    Some(if dist < 0.25 { zone } else { DropZone::Center })
}

/// Contorno de onde a janela vai ficar ao soltar.
pub fn preview_rect(group: Rect, zone: DropZone) -> Rect {
    match zone {
        DropZone::Center => group,
        DropZone::Left => group.split_left_right_at_fraction(0.5).0,
        DropZone::Right => group.split_left_right_at_fraction(0.5).1,
        DropZone::Top => group.split_top_bottom_at_fraction(0.5).0,
        DropZone::Bottom => group.split_top_bottom_at_fraction(0.5).1,
    }
}

/// Grupo de abas visível sob o ponteiro (o menor, se houver aninhados).
pub fn group_under(tree: &Tree<DockPane>, pointer: Pos2) -> Option<(TileId, Rect)> {
    tree.tiles
        .iter()
        .filter(|(id, tile)| {
            matches!(tile, Tile::Container(Container::Tabs(_))) && tree.is_visible(**id)
        })
        .filter_map(|(id, _)| tree.tiles.rect(*id).map(|r| (*id, r)))
        .filter(|(_, r)| r.is_finite() && r.contains(pointer))
        .min_by(|a, b| a.1.area().total_cmp(&b.1.area()))
}

/// Grupo de abas que contém o painel.
fn parent_tabs(tree: &Tree<DockPane>, pane: TileId) -> Option<TileId> {
    let parent = tree.tiles.parent_of(pane)?;
    matches!(
        tree.tiles.get(parent),
        Some(Tile::Container(Container::Tabs(_)))
    )
    .then_some(parent)
}

/// Tira o painel do grupo onde está, mantendo uma aba ativa nele.
fn take_from_parent(tree: &mut Tree<DockPane>, pane: TileId) {
    let Some(parent) = tree.tiles.parent_of(pane) else {
        return;
    };
    let Some(mut tile) = tree.tiles.remove(parent) else {
        return;
    };
    if let Tile::Container(c) = &mut tile {
        c.remove_child(pane);
        if let Container::Tabs(tabs) = c {
            tabs.ensure_active(&tree.tiles);
        }
    }
    tree.tiles.insert(parent, tile);
}

/// Aplica a queda da aba `pane` no grupo `target`. Devolve `false` quando não
/// há o que mudar (a aba já está ali sozinha, ou os blocos não existem).
pub fn apply_drop(
    tree: &mut Tree<DockPane>,
    pane: TileId,
    target: TileId,
    zone: DropZone,
    simplification: &egui_tiles::SimplificationOptions,
) -> bool {
    if pane == target || !matches!(tree.tiles.get(pane), Some(Tile::Pane(_))) {
        return false;
    }
    let target_len = match tree.tiles.get(target) {
        Some(Tile::Container(Container::Tabs(tabs))) => tabs.children.len(),
        _ => return false,
    };
    let from = parent_tabs(tree, pane);
    match zone {
        DropZone::Center => {
            if from == Some(target) {
                if let Some(Tile::Container(Container::Tabs(tabs))) = tree.tiles.get_mut(target) {
                    tabs.set_active(pane);
                }
                return false;
            }
            take_from_parent(tree, pane);
            if let Some(Tile::Container(Container::Tabs(tabs))) = tree.tiles.get_mut(target) {
                tabs.add_child(pane);
                tabs.set_active(pane);
            }
        }
        side => {
            if from == Some(target) && target_len <= 1 {
                return false;
            }
            take_from_parent(tree, pane);
            // O grupo de destino muda de id e o id dele passa a ser a divisão,
            // então o pai e a raiz continuam apontando para o lugar certo.
            let Some(old) = tree.tiles.remove(target) else {
                return false;
            };
            let moved = tree.tiles.insert_new(old);
            let new_group = tree.tiles.insert_tab_tile(vec![pane]);
            let (dir, first) = match side {
                DropZone::Left => (LinearDir::Horizontal, true),
                DropZone::Right => (LinearDir::Horizontal, false),
                DropZone::Top => (LinearDir::Vertical, true),
                _ => (LinearDir::Vertical, false),
            };
            let children = if first {
                vec![new_group, moved]
            } else {
                vec![moved, new_group]
            };
            tree.tiles.insert(
                target,
                Tile::Container(Container::Linear(Linear::new(dir, children))),
            );
        }
    }
    tree.simplify(simplification);
    true
}

/// Painel em cada bloco de painel da árvore, para conferir que nada sumiu nem
/// duplicou.
#[cfg(test)]
fn panes_of(tree: &Tree<DockPane>) -> Vec<DockPane> {
    let mut out: Vec<DockPane> = tree
        .tiles
        .iter()
        .filter_map(|(_, t)| match t {
            Tile::Pane(p) => Some(*p),
            _ => None,
        })
        .collect();
    out.sort_by_key(|p| format!("{p:?}"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::{build_dock_tree, dock_simplification, find_pane_tile, sanitize_tile_tree};
    use crate::DockTab;

    fn tree() -> Tree<DockPane> {
        build_dock_tree(0.40, DockTab::ProjectMedia, false)
    }

    fn group_of(tree: &Tree<DockPane>, pane: DockPane) -> TileId {
        let tile = find_pane_tile(tree, pane).expect("painel");
        tree.tiles.parent_of(tile).expect("grupo")
    }

    #[test]
    fn marcadores_ganham_das_faixas_e_as_bordas_valem_o_lado() {
        let g = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0));
        for (zone, r) in marker_rects(g) {
            assert_eq!(drop_zone(g, None, r.center()), Some(zone));
        }
        assert_eq!(drop_zone(g, None, pos2(10.0, 150.0)), Some(DropZone::Left));
        assert_eq!(
            drop_zone(g, None, pos2(390.0, 150.0)),
            Some(DropZone::Right)
        );
        assert_eq!(drop_zone(g, None, pos2(200.0, 10.0)), Some(DropZone::Top));
        assert_eq!(
            drop_zone(g, None, pos2(200.0, 290.0)),
            Some(DropZone::Bottom)
        );
        assert_eq!(
            drop_zone(g, None, pos2(300.0, 150.0)),
            Some(DropZone::Center)
        );
        assert_eq!(drop_zone(g, None, pos2(500.0, 150.0)), None);
    }

    /// Soltar sobre a faixa de abas de outro grupo junta a aba a ele, mesmo
    /// com a faixa dentro da borda de 25% (abas na base ou no topo).
    #[test]
    fn faixa_de_abas_junta_ao_grupo() {
        let g = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0));
        let base = Rect::from_min_max(pos2(0.0, 276.0), pos2(400.0, 300.0));
        assert_eq!(
            drop_zone(g, Some(base), pos2(10.0, 290.0)),
            Some(DropZone::Center)
        );
        assert_eq!(
            drop_zone(g, Some(base), pos2(390.0, 290.0)),
            Some(DropZone::Center)
        );
        let topo = Rect::from_min_max(pos2(0.0, 0.0), pos2(400.0, 26.0));
        assert_eq!(
            drop_zone(g, Some(topo), pos2(200.0, 10.0)),
            Some(DropZone::Center)
        );
        // Fora da faixa as bordas continuam valendo o lado.
        assert_eq!(
            drop_zone(g, Some(base), pos2(200.0, 10.0)),
            Some(DropZone::Top)
        );
        let lixo = Rect::from_min_max(pos2(f32::NAN, 0.0), pos2(400.0, 300.0));
        assert_eq!(
            drop_zone(g, Some(lixo), pos2(10.0, 150.0)),
            Some(DropZone::Left)
        );
    }

    #[test]
    fn entrada_hostil_nao_vira_zona() {
        let g = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0));
        assert_eq!(drop_zone(g, None, pos2(f32::NAN, 10.0)), None);
        assert_eq!(drop_zone(g, None, pos2(f32::INFINITY, 10.0)), None);
        let nada = Rect::from_min_size(pos2(0.0, 0.0), vec2(0.0, 0.0));
        assert_eq!(drop_zone(nada, None, pos2(0.0, 0.0)), None);
        let inf = Rect::from_min_max(pos2(0.0, 0.0), pos2(f32::INFINITY, 10.0));
        assert_eq!(drop_zone(inf, None, pos2(5.0, 5.0)), None);
        // Grupo minúsculo: o marcador tem o tamanho mínimo e não entra em pânico.
        let mini = Rect::from_min_size(pos2(0.0, 0.0), vec2(4.0, 4.0));
        assert!(drop_zone(mini, None, pos2(2.0, 2.0)).is_some());
    }

    #[test]
    fn lado_divide_o_grupo_e_mantem_cada_painel_uma_vez() {
        let mut t = tree();
        let antes = panes_of(&t);
        let opts = dock_simplification();
        let mixer = find_pane_tile(&t, DockPane::Tab(DockTab::Mixer)).expect("mixer");
        let alvo = group_of(&t, DockPane::Preview);
        assert!(apply_drop(&mut t, mixer, alvo, DropZone::Bottom, &opts));
        assert_eq!(panes_of(&t), antes);
        // O Mixer foi para um grupo só dele, separado do grupo da prévia.
        let g_mixer = group_of(&t, DockPane::Tab(DockTab::Mixer));
        assert_ne!(g_mixer, group_of(&t, DockPane::Preview));
        assert_ne!(g_mixer, group_of(&t, DockPane::Tab(DockTab::Explorer)));
        let pai = t.tiles.parent_of(g_mixer).expect("divisão");
        assert!(matches!(
            t.tiles.get(pai),
            Some(Tile::Container(Container::Linear(l))) if l.dir == LinearDir::Vertical
        ));
        // A árvore continua válida para o `.neoveglayout`: o saneamento não a refaz.
        let antes_ids: Vec<TileId> = t.tiles.tile_ids().collect();
        sanitize_tile_tree(&mut t, 0.40, DockTab::ProjectMedia, false);
        let depois_ids: Vec<TileId> = t.tiles.tile_ids().collect();
        assert_eq!(antes_ids.len(), depois_ids.len());
    }

    #[test]
    fn meio_junta_ao_grupo_e_ativa_a_aba() {
        let mut t = tree();
        let opts = dock_simplification();
        let mixer = find_pane_tile(&t, DockPane::Tab(DockTab::Mixer)).expect("mixer");
        let alvo = group_of(&t, DockPane::MasterBus);
        assert!(apply_drop(&mut t, mixer, alvo, DropZone::Center, &opts));
        assert_eq!(group_of(&t, DockPane::Tab(DockTab::Mixer)), alvo);
        assert!(matches!(
            t.tiles.get(alvo),
            Some(Tile::Container(Container::Tabs(tabs))) if tabs.active == Some(mixer)
        ));
        assert_eq!(panes_of(&t), panes_of(&tree()));
    }

    #[test]
    fn soltar_no_proprio_grupo_sozinho_nao_muda_nada() {
        let mut t = tree();
        let opts = dock_simplification();
        let pv = find_pane_tile(&t, DockPane::Preview).expect("prévia");
        let g = group_of(&t, DockPane::Preview);
        for zone in [
            DropZone::Center,
            DropZone::Left,
            DropZone::Right,
            DropZone::Top,
            DropZone::Bottom,
        ] {
            assert!(!apply_drop(&mut t, pv, g, zone, &opts), "{zone:?}");
        }
        assert_eq!(group_of(&t, DockPane::Preview), g);
    }

    #[test]
    fn mover_o_unico_painel_de_um_grupo_remove_o_grupo_vazio() {
        let mut t = tree();
        let opts = dock_simplification();
        let mb = find_pane_tile(&t, DockPane::MasterBus).expect("master");
        let velho = group_of(&t, DockPane::MasterBus);
        let alvo = group_of(&t, DockPane::Preview);
        assert!(apply_drop(&mut t, mb, alvo, DropZone::Right, &opts));
        assert!(
            t.tiles.get(velho).is_none(),
            "o grupo antigo ficou na árvore"
        );
        let vazio = t
            .tiles
            .iter()
            .any(|(_, tile)| matches!(tile, Tile::Container(c) if c.num_children() == 0));
        assert!(!vazio, "sobrou um contêiner vazio");
        assert_eq!(panes_of(&t), panes_of(&tree()));
    }

    #[test]
    fn ids_que_nao_existem_sao_recusados() {
        let mut t = tree();
        let opts = dock_simplification();
        let pv = find_pane_tile(&t, DockPane::Preview).expect("prévia");
        let fantasma = TileId::from_u64(9_999_999);
        let g_master = group_of(&t, DockPane::MasterBus);
        assert!(!apply_drop(
            &mut t,
            fantasma,
            g_master,
            DropZone::Left,
            &opts
        ));
        assert!(!apply_drop(&mut t, pv, fantasma, DropZone::Left, &opts));
        // Um painel não é grupo de destino.
        let mb = find_pane_tile(&t, DockPane::MasterBus).expect("master");
        assert!(!apply_drop(&mut t, pv, mb, DropZone::Left, &opts));
        assert_eq!(panes_of(&t), panes_of(&tree()));
    }
}
