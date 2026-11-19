//! Diálogos em janela nativa própria, fora dos limites da janela do Neoveg.
//!
//! [`DialogWindow`] troca `egui::Window` sem mudar o conteúdo dos diálogos:
//! mesmo construtor e mesmo `.show(ctx, |ui| ...)`. Cada diálogo abre numa
//! janela do sistema sem moldura, com uma barra própria no topo:
//!
//! - `:::` à esquerda: arraste a janela e solte-a sobre a janela do Neoveg
//!   para acoplá-la; numa janela acoplada, arraste o `:::` para soltá-la de
//!   novo como janela própria;
//! - a área vazia com o título move a janela livremente, sem acoplar;
//! - à direita, só o botão de fechar.
//!
//! A janela clicada vai para a frente das outras (o sistema faz isso com
//! janelas nativas; o egui, com as acopladas). Onde o egui desenha as janelas
//! dentro da janela principal (`embed_viewports`, como nos testes de UI), o
//! diálogo é a `egui::Window` de sempre.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, Id, Pos2, Rect, ResizeDirection, Sense, Stroke, StrokeKind,
    Vec2, ViewportCommand, ViewportId,
};
use neoveg_common::{tr, Language};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Altura da barra de título própria (pontos).
pub const BAR_H: f32 = 22.0;
/// Margem interna do conteúdo na janela nativa (pontos).
const MARGIN: f32 = 6.0;
/// Faixa das bordas que redimensiona a janela nativa (pontos).
const EDGE: f32 = 5.0;
/// Tempo parada que conta como "soltou a janela" ao arrastar pelo `:::`.
pub const SETTLE: Duration = Duration::from_millis(250);
/// Apertou o `:::` e não arrastou: desiste depois disso.
pub const GRIP_GIVE_UP: Duration = Duration::from_millis(1500);
/// Arraste mínimo do `:::` numa janela acoplada para soltá-la (pontos).
pub const UNDOCK_MIN_DRAG: f32 = 24.0;
/// Largura usada para medir diálogos de tamanho automático sem largura dada.
const AUTO_WIDTH: f32 = 560.0;

/// Chaves dos dados temporários do egui gravados pelo `App` a cada quadro.
const LANG_KEY: &str = "neoveg_dialog_lang";
const ICON_KEY: &str = "neoveg_dialog_icon";

/// Idioma e ícone das janelas dos diálogos (chamado pelo `App` a cada quadro).
pub fn set_context(ctx: &egui::Context, lang: Language, icon: &Arc<egui::IconData>) {
    ctx.data_mut(|d| {
        d.insert_temp(Id::new(LANG_KEY), lang);
        d.insert_temp(Id::new(ICON_KEY), icon.clone());
    });
}

/// Traz para a frente a janela própria do diálogo de id `id` (o mesmo
/// passado em `.id(...)`).
pub fn focus(ctx: &egui::Context, id: Id) {
    ctx.send_viewport_cmd_to(viewport_id(id), ViewportCommand::Focus);
}

fn viewport_id(key: Id) -> ViewportId {
    ViewportId::from_hash_of(key.with("neoveg_dialog_viewport"))
}

/// Arraste da janela nativa pelo `:::` (decide se ela acopla ao parar).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GripDrag {
    last: Pos2,
    moved_at: Option<Instant>,
    started: Instant,
}

/// Estado do arraste pelo `:::` neste quadro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GripPhase {
    /// Ainda não se moveu.
    Waiting,
    /// Em movimento.
    Moving,
    /// Moveu e parou: decida se acopla.
    Settled,
    /// Apertou e não arrastou: desiste.
    GaveUp,
}

impl GripDrag {
    pub fn new(pos: Pos2, now: Instant) -> Self {
        Self {
            last: pos,
            moved_at: None,
            started: now,
        }
    }

    /// Atualiza com a posição da janela neste quadro.
    pub fn step(&mut self, pos: Pos2, now: Instant) -> GripPhase {
        if !pos.x.is_finite() || !pos.y.is_finite() {
            return GripPhase::Waiting;
        }
        if (self.last - pos).length() > 0.5 {
            self.last = pos;
            self.moved_at = Some(now);
            return GripPhase::Moving;
        }
        match self.moved_at {
            Some(at) if now.saturating_duration_since(at) >= SETTLE => GripPhase::Settled,
            Some(_) => GripPhase::Moving,
            None if now.saturating_duration_since(self.started) >= GRIP_GIVE_UP => {
                GripPhase::GaveUp
            }
            None => GripPhase::Waiting,
        }
    }
}

/// Ponto da barra de título (o `:::`) usado para decidir se a janela solta
/// sobre a janela de origem acopla.
pub fn grip_point(outer_min: Pos2) -> Pos2 {
    pos2(outer_min.x + 14.0, outer_min.y + BAR_H * 0.5)
}

/// Borda da janela sob o ponteiro, para redimensionar.
pub fn resize_dir(rect: Rect, p: Pos2, edge: f32) -> Option<ResizeDirection> {
    if !rect.is_finite() || !p.x.is_finite() || !p.y.is_finite() || !rect.contains(p) {
        return None;
    }
    let w = p.x - rect.left() < edge;
    let e = rect.right() - p.x < edge;
    let n = p.y - rect.top() < edge;
    let s = rect.bottom() - p.y < edge;
    Some(match (n, s, w, e) {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (true, _, _, true) => ResizeDirection::NorthEast,
        (_, true, true, _) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, _, _, _) => ResizeDirection::North,
        (_, true, _, _) => ResizeDirection::South,
        (_, _, true, _) => ResizeDirection::West,
        (_, _, _, true) => ResizeDirection::East,
        _ => return None,
    })
}

fn resize_cursor(d: ResizeDirection) -> egui::CursorIcon {
    match d {
        ResizeDirection::North | ResizeDirection::South => egui::CursorIcon::ResizeVertical,
        ResizeDirection::East | ResizeDirection::West => egui::CursorIcon::ResizeHorizontal,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => egui::CursorIcon::ResizeNwSe,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => egui::CursorIcon::ResizeNeSw,
    }
}

/// Posição e tamanho (`[x, y, largura, altura]` da área interna) com que a
/// janela nativa abre: centrada na janela de origem e cabendo no monitor.
pub fn initial_geom(parent: Option<Rect>, inner: Vec2, monitor: Option<Vec2>) -> [f32; 4] {
    let mut w = if inner.x.is_finite() {
        inner.x.max(160.0)
    } else {
        420.0
    };
    let mut h = if inner.y.is_finite() {
        inner.y.max(80.0)
    } else {
        300.0
    };
    if let Some(m) = monitor.filter(|m| m.x.is_finite() && m.y.is_finite() && m.x > 0.0) {
        w = w.min((m.x * 0.95).max(160.0));
        h = h.min((m.y * 0.9).max(80.0));
    }
    let (x, y) = match parent.filter(|r| r.is_finite()) {
        Some(r) => (
            r.center().x - w * 0.5,
            (r.center().y - h * 0.5).max(r.top()),
        ),
        None => (100.0, 100.0),
    };
    [x, y, w, h]
}

/// Tamanho da área interna que mostra o conteúdo `content` inteiro com a
/// barra e as margens.
pub fn fit_inner(content: Vec2, min: Vec2) -> Vec2 {
    let w = content.x + 2.0 * MARGIN;
    let h = content.y + 2.0 * MARGIN + BAR_H;
    let w = if w.is_finite() {
        w.max(min.x).max(160.0)
    } else {
        420.0
    };
    let h = if h.is_finite() {
        h.max(min.y).max(BAR_H + 40.0)
    } else {
        300.0
    };
    vec2(w.ceil(), h.ceil())
}

/// Onde a janela acoplada solta pelo `:::` abre como janela nativa.
pub fn undock_geom(parent_min: Option<Pos2>, win: Rect, drag: Vec2) -> Option<[f32; 4]> {
    let p = parent_min?;
    let g = [
        p.x + win.min.x + drag.x,
        p.y + win.min.y + drag.y,
        win.width(),
        win.height(),
    ];
    g.iter().all(|v| v.is_finite()).then_some(g)
}

/// Onde a janela fica ao ser arrastada pela barra: o ponto agarrado
/// (`grab`, relativo ao canto da janela) acompanha o ponteiro. `outer_min` é o
/// canto atual na tela e `pointer` a posição do ponteiro dentro da janela.
pub fn moved_window_pos(outer_min: Pos2, pointer: Pos2, grab: Vec2) -> Option<Pos2> {
    let p = outer_min + pointer.to_vec2() - grab;
    (p.x.is_finite() && p.y.is_finite()).then_some(p)
}

/// Retângulo da janela redimensionada pela borda `dir`, com o ponteiro
/// deslocado `delta` desde o início. Nunca fica menor que `min`; a borda
/// oposta não se move.
pub fn resized_rect(start: Rect, dir: ResizeDirection, delta: Vec2, min: Vec2) -> Option<Rect> {
    use ResizeDirection as D;
    if !(delta.x.is_finite() && delta.y.is_finite()) {
        return None;
    }
    let (west, east, north, south) = match dir {
        D::North => (false, false, true, false),
        D::South => (false, false, false, true),
        D::West => (true, false, false, false),
        D::East => (false, true, false, false),
        D::NorthWest => (true, false, true, false),
        D::NorthEast => (false, true, true, false),
        D::SouthWest => (true, false, false, true),
        D::SouthEast => (false, true, false, true),
    };
    let min_w = min.x.max(1.0);
    let min_h = min.y.max(1.0);
    let mut r = start;
    if west {
        r.min.x = (start.min.x + delta.x).min(start.max.x - min_w);
    }
    if east {
        r.max.x = (start.max.x + delta.x).max(start.min.x + min_w);
    }
    if north {
        r.min.y = (start.min.y + delta.y).min(start.max.y - min_h);
    }
    if south {
        r.max.y = (start.max.y + delta.y).max(start.min.y + min_h);
    }
    (r.is_finite() && r.width() >= 1.0 && r.height() >= 1.0).then_some(r)
}

/// Redimensionamento manual em curso: borda, ponteiro (na tela) e retângulo
/// da janela no início.
#[derive(Clone, Copy, Debug)]
struct ResizeDrag {
    dir: ResizeDirection,
    start_pointer: Pos2,
    start_rect: Rect,
}

/// Estado de cada diálogo entre os quadros (dados temporários do egui).
#[derive(Clone, Debug, Default)]
struct DlgState {
    /// Acoplado na janela de origem (padrão: janela própria).
    docked: bool,
    /// Posição e tamanho com que a janela nativa abriu (fixos enquanto ela
    /// está aberta: o egui só reenvia o que muda no `ViewportBuilder`).
    geom: Option<[f32; 4]>,
    /// Último quadro (da janela de origem) em que o diálogo foi mostrado.
    last_pass: u64,
    /// Quadros desde que a janela nativa abriu (ajuste inicial do tamanho).
    frames: u32,
    /// Arraste pelo `:::` da janela nativa em curso.
    grip: Option<GripDrag>,
    /// A janela vai acoplar: contorno na janela de origem.
    hint: bool,
    /// Posição (na janela de origem) da janela recém-acoplada.
    dock_pos: Option<Pos2>,
    /// Arraste acumulado do `:::` da janela acoplada.
    drag_acc: Vec2,
    /// Largura natural do conteúdo, medida numa passada invisível ao abrir
    /// (separadores e afins não se esticam nela).
    natural_w: Option<f32>,
    /// Janela nativa sendo arrastada pela barra: ponto agarrado, relativo ao
    /// canto da janela. O Neoveg move a janela ele mesmo (e não pelo laço de
    /// arraste do Windows), então ela continua opaca e acompanha o ponteiro
    /// mesmo com "Mostrar conteúdo da janela ao arrastar" desligado.
    moving: Option<Vec2>,
    /// Redimensionamento manual em curso, pelo mesmo motivo.
    resize: Option<ResizeDrag>,
    /// Janela acoplada sendo arrastada pelo `:::`: onde ela estava no início.
    dock_drag_base: Option<Pos2>,
}

/// Barra de título própria: `:::`, área vazia com o título e fechar.
struct BarOut {
    grip: egui::Response,
    area: egui::Response,
    close: bool,
}

fn title_bar(
    ui: &mut egui::Ui,
    title: &str,
    has_close: bool,
    lang: Language,
    area_sense: Sense,
) -> BarOut {
    let mut close = false;
    let inner = ui.horizontal(|ui| {
        ui.set_height(BAR_H - 2.0);
        ui.spacing_mut().item_spacing.x = 4.0;
        let grip = ui
            .add(
                egui::Label::new(egui::RichText::new(":::").strong().monospace())
                    .sense(Sense::click_and_drag())
                    .selectable(false),
            )
            .on_hover_cursor(egui::CursorIcon::Grab)
            .on_hover_text(tr(
                lang,
                "Arraste para soltar a janela ou acoplá-la na janela do Neoveg",
            ));
        let close_w = if has_close { 20.0 } else { 0.0 };
        let area_w = (ui.available_width() - close_w).max(0.0);
        let (rect, area) = ui.allocate_exact_size(vec2(area_w, BAR_H - 2.0), area_sense);
        let visuals = ui.visuals().clone();
        ui.painter().with_clip_rect(rect).text(
            rect.left_center(),
            Align2::LEFT_CENTER,
            title,
            egui::TextStyle::Button.resolve(ui.style()),
            visuals.strong_text_color(),
        );
        if has_close {
            let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
            let resp = resp.on_hover_text(tr(lang, "Fechar"));
            let hovered = resp.hovered();
            if hovered {
                ui.painter()
                    .rect_filled(r, 2.0, Color32::from_rgb(196, 43, 28));
            }
            let c = if hovered {
                Color32::WHITE
            } else {
                visuals.text_color()
            };
            let k = r.shrink(6.0);
            let st = Stroke::new(1.5_f32, c);
            ui.painter()
                .line_segment([k.left_top(), k.right_bottom()], st);
            ui.painter()
                .line_segment([k.right_top(), k.left_bottom()], st);
            if resp.clicked() {
                close = true;
            }
        }
        (grip, area)
    });
    let (grip, area) = inner.inner;
    BarOut { grip, area, close }
}

/// Substituto de `egui::Window` para os diálogos (ver o módulo).
#[must_use = "chame .show(ctx, |ui| ...)"]
pub struct DialogWindow<'open> {
    title: String,
    id: Option<Id>,
    open: Option<&'open mut bool>,
    resizable: bool,
    collapsible: bool,
    default_width: Option<f32>,
    default_height: Option<f32>,
    min_width: Option<f32>,
    min_height: Option<f32>,
    max_size: Option<Vec2>,
    anchor: Option<(Align2, Vec2)>,
    pivot: Option<Align2>,
    default_pos: Option<Pos2>,
    constrain_to: Option<Rect>,
}

impl<'open> DialogWindow<'open> {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            id: None,
            open: None,
            resizable: true,
            collapsible: true,
            default_width: None,
            default_height: None,
            min_width: None,
            min_height: None,
            max_size: None,
            anchor: None,
            pivot: None,
            default_pos: None,
            constrain_to: None,
        }
    }
    pub fn id(mut self, id: Id) -> Self {
        self.id = Some(id);
        self
    }
    pub fn open(mut self, open: &'open mut bool) -> Self {
        self.open = Some(open);
        self
    }
    pub fn resizable(mut self, v: bool) -> Self {
        self.resizable = v;
        self
    }
    pub fn collapsible(mut self, v: bool) -> Self {
        self.collapsible = v;
        self
    }
    pub fn default_size(mut self, s: impl Into<Vec2>) -> Self {
        let s = s.into();
        self.default_width = Some(s.x);
        self.default_height = Some(s.y);
        self
    }
    pub fn default_width(mut self, w: f32) -> Self {
        self.default_width = Some(w);
        self
    }
    pub fn default_height(mut self, h: f32) -> Self {
        self.default_height = Some(h);
        self
    }
    pub fn min_size(mut self, s: impl Into<Vec2>) -> Self {
        let s = s.into();
        self.min_width = Some(s.x);
        self.min_height = Some(s.y);
        self
    }
    pub fn min_width(mut self, w: f32) -> Self {
        self.min_width = Some(w);
        self
    }
    pub fn max_size(mut self, s: impl Into<Vec2>) -> Self {
        self.max_size = Some(s.into());
        self
    }
    pub fn anchor(mut self, a: Align2, offset: impl Into<Vec2>) -> Self {
        self.anchor = Some((a, offset.into()));
        self
    }
    pub fn pivot(mut self, p: Align2) -> Self {
        self.pivot = Some(p);
        self
    }
    pub fn default_pos(mut self, p: impl Into<Pos2>) -> Self {
        self.default_pos = Some(p.into());
        self
    }
    pub fn constrain_to(mut self, r: Rect) -> Self {
        self.constrain_to = Some(r);
        self
    }

    fn key(&self) -> Id {
        self.id
            .unwrap_or_else(|| Id::new(("neoveg_dialog", &self.title)))
    }

    /// Tamanhos da `egui::Window` em comum (janela de sempre e acoplada).
    fn sized<'a>(&self, mut w: egui::Window<'a>) -> egui::Window<'a> {
        w = w.resizable(self.resizable);
        match (self.default_width, self.default_height) {
            (Some(x), Some(y)) => w = w.default_size([x, y]),
            (Some(x), None) => w = w.default_width(x),
            (None, Some(y)) => w = w.default_height(y),
            (None, None) => {}
        }
        match (self.min_width, self.min_height) {
            (Some(x), Some(y)) => w = w.min_size([x, y]),
            (Some(x), None) => w = w.min_width(x),
            (None, Some(y)) => w = w.min_height(y),
            (None, None) => {}
        }
        if let Some(m) = self.max_size {
            w = w.max_size(m);
        }
        if let Some(r) = self.constrain_to {
            w = w.constrain_to(r);
        }
        w
    }

    /// As mesmas opções, sem o `open` (para montar a janela dentro de closures).
    fn config(&self) -> DialogWindow<'static> {
        DialogWindow {
            title: self.title.clone(),
            id: self.id,
            open: None,
            resizable: self.resizable,
            collapsible: self.collapsible,
            default_width: self.default_width,
            default_height: self.default_height,
            min_width: self.min_width,
            min_height: self.min_height,
            max_size: self.max_size,
            anchor: self.anchor,
            pivot: self.pivot,
            default_pos: self.default_pos,
            constrain_to: self.constrain_to,
        }
    }

    /// A `egui::Window` de sempre, sem o `open`.
    fn plain_window<'a>(&self) -> egui::Window<'a> {
        let mut w = self.sized(egui::Window::new(self.title.clone()).id(self.key()));
        w = w.collapsible(self.collapsible);
        if let Some((a, off)) = self.anchor {
            w = w.anchor(a, off);
        }
        if let Some(p) = self.pivot {
            w = w.pivot(p);
        }
        if let Some(p) = self.default_pos {
            w = w.default_pos(p);
        }
        w
    }

    /// Mostra o diálogo. Devolve o resultado do conteúdo quando ele foi
    /// desenhado neste quadro.
    pub fn show<R>(
        self,
        ctx: &egui::Context,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> Option<R> {
        if ctx.embed_viewports() {
            return self.show_plain(ctx, add_contents);
        }
        let key = self.key();
        let sid = key.with("neoveg_dialog_state");
        let mut st: DlgState = ctx.data_mut(|d| d.get_temp(sid)).unwrap_or_default();
        let pass = ctx.cumulative_pass_nr();
        let reopened = st.last_pass == 0 || pass > st.last_pass.saturating_add(1);
        st.last_pass = pass;
        if reopened {
            st.geom = None;
            st.frames = 0;
            st.grip = None;
            st.hint = false;
            st.drag_acc = Vec2::ZERO;
            st.natural_w = None;
            st.moving = None;
            st.resize = None;
            st.dock_drag_base = None;
        }
        let lang = ctx
            .data_mut(|d| d.get_temp::<Language>(Id::new(LANG_KEY)))
            .unwrap_or(Language::PtBr);
        let out = if st.docked {
            self.show_docked(ctx, key, &mut st, lang, add_contents)
        } else {
            self.show_native(ctx, key, &mut st, lang, reopened, add_contents)
        };
        ctx.data_mut(|d| d.insert_temp(sid, st));
        out
    }

    /// A `egui::Window` de sempre (onde não há janelas nativas).
    fn show_plain<R>(
        self,
        ctx: &egui::Context,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> Option<R> {
        let w = self.plain_window();
        match self.open {
            Some(open) => w.open(open).show(ctx, add_contents),
            None => w.show(ctx, add_contents),
        }
        .and_then(|r| r.inner)
    }

    /// Acoplado na janela de origem: `egui::Window` com a barra própria.
    fn show_docked<R>(
        self,
        ctx: &egui::Context,
        key: Id,
        st: &mut DlgState,
        lang: Language,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> Option<R> {
        let parent = ctx.input(|i| i.viewport().inner_rect);
        let mut w = self
            .sized(egui::Window::new(self.title.clone()).id(key))
            .title_bar(false)
            .collapsible(false);
        match st.dock_pos.take() {
            Some(p) => w = w.current_pos(p),
            // arrastando pelo `:::`: a própria janela acompanha o ponteiro
            None if st.dock_drag_base.is_some() => {
                if let Some(base) = st.dock_drag_base {
                    w = w.current_pos(base + st.drag_acc);
                }
            }
            None => {
                // âncora fixaria a janela: centrada, mas livre para mover
                w = w
                    .pivot(self.pivot.unwrap_or(Align2::CENTER_CENTER))
                    .default_pos(
                        self.default_pos
                            .unwrap_or_else(|| ctx.screen_rect().center()),
                    );
            }
        }
        let has_close = self.open.is_some();
        let title = self.title.clone();
        let mut close = false;
        let mut grip_resp = None;
        let shown = w.show(ctx, |ui| {
            let bar = title_bar(ui, &title, has_close, lang, Sense::hover());
            close = bar.close;
            grip_resp = Some(bar.grip);
            ui.separator();
            add_contents(ui)
        });
        if close {
            if let Some(o) = self.open {
                *o = false;
            }
        }
        let win_rect = shown.as_ref().map(|r| r.response.rect);
        if let (Some(g), Some(win)) = (grip_resp, win_rect) {
            if g.drag_started() {
                st.dock_drag_base = Some(win.min);
                st.drag_acc = Vec2::ZERO;
            }
            // a janela já está onde o arraste a levou (sem fantasma): só falta
            // o deslocamento deste quadro
            let pending = g.drag_delta();
            if g.dragged() {
                st.drag_acc += pending;
                ctx.request_repaint();
            }
            if g.drag_stopped() {
                if st.drag_acc.length() >= UNDOCK_MIN_DRAG {
                    st.docked = false;
                    st.geom = undock_geom(parent.map(|r| r.min), win, pending);
                    st.frames = u32::MAX / 2;
                } else if let Some(base) = st.dock_drag_base {
                    // arraste curto: fica acoplada onde foi solta
                    st.dock_pos = Some(base + st.drag_acc);
                }
                st.drag_acc = Vec2::ZERO;
                st.dock_drag_base = None;
            }
        }
        shown.and_then(|r| r.inner)
    }

    /// Janela nativa própria, sem moldura do sistema.
    fn show_native<R>(
        self,
        ctx: &egui::Context,
        key: Id,
        st: &mut DlgState,
        lang: Language,
        reopened: bool,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> Option<R> {
        let (parent, monitor) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().monitor_size));
        // contorno na janela de origem enquanto a janela vai acoplar
        if st.hint {
            let r = ctx.screen_rect().shrink(1.0);
            let accent = ctx.style().visuals.selection.bg_fill;
            let p = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                key.with("dock_hint"),
            ));
            p.rect_filled(r, 2.0, accent.gamma_multiply(0.12));
            p.rect_stroke(r, 2.0, Stroke::new(2.0_f32, accent), StrokeKind::Inside);
        }
        let auto_w = !self.resizable || self.default_width.is_none();
        let auto_h = !self.resizable || self.default_height.is_none();
        let guess = vec2(
            self.default_width.unwrap_or(AUTO_WIDTH.min(420.0)) + 2.0 * MARGIN,
            self.default_height.unwrap_or(240.0) + 2.0 * MARGIN + BAR_H,
        );
        let geom = *st
            .geom
            .get_or_insert_with(|| initial_geom(parent, guess, monitor));
        let min_inner = vec2(
            self.min_width.unwrap_or(160.0) + 2.0 * MARGIN,
            self.min_height.unwrap_or(40.0) + 2.0 * MARGIN + BAR_H,
        );
        let vid = viewport_id(key);
        let mut builder = egui::ViewportBuilder::default()
            .with_title(format!("{} - Neoveg", self.title))
            .with_decorations(false)
            .with_resizable(self.resizable)
            .with_min_inner_size(min_inner)
            .with_position([geom[0], geom[1]])
            .with_inner_size([geom[2], geom[3]]);
        if let Some(icon) = ctx.data_mut(|d| d.get_temp::<Arc<egui::IconData>>(Id::new(ICON_KEY))) {
            builder = builder.with_icon(icon);
        }
        if reopened {
            ctx.send_viewport_cmd_to(vid, ViewportCommand::Focus);
        }
        let fit_w = self
            .default_width
            .unwrap_or(AUTO_WIDTH)
            .min(self.max_size.map_or(f32::INFINITY, |m| m.x));
        let resizable = self.resizable;
        let fallback = self.config();
        let has_close = self.open.is_some();
        let title = self.title.clone();
        let mut open = self.open;
        let mut add = Some(add_contents);
        let mut result = None;
        let mut dock_now: Option<Pos2> = None;
        let frames = st.frames;
        let mut grip = st.grip;
        let mut moving = st.moving;
        let mut resize = st.resize;
        let mut hint = false;
        let natural_w = st.natural_w;
        let mut new_natural_w = None;
        // mede o conteúdo: sempre nos de tamanho fixo, só ao abrir nos outros
        let measure = (auto_w || auto_h) && (!resizable || frames < 3);
        // maior janela que o ajuste ao conteúdo cria: cabe no monitor
        let max_inner = monitor
            .filter(|m| m.x.is_finite() && m.y.is_finite() && m.x > 0.0 && m.y > 0.0)
            .map_or(vec2(1600.0, 900.0), |m| vec2(m.x * 0.95, m.y * 0.9));
        ctx.show_viewport_immediate(vid, builder, |vctx, class| {
            if class == egui::ViewportClass::Embedded {
                // o sistema não abriu a janela: a de sempre, dentro da origem
                let mut w = fallback.plain_window();
                if let Some(o) = open.as_deref_mut() {
                    w = w.open(o);
                }
                if let Some(f) = add.take() {
                    result = w.show(vctx, f).and_then(|r| r.inner);
                }
                return;
            }
            if vctx.input(|i| i.viewport().close_requested()) {
                match open.as_deref_mut() {
                    Some(o) => *o = false,
                    None => vctx.send_viewport_cmd(ViewportCommand::CancelClose),
                }
            }
            // O Neoveg move e redimensiona a janela ele mesmo, em vez de usar o
            // laço de arraste do Windows (StartDrag/BeginResize): com "Mostrar
            // conteúdo da janela ao arrastar" desligado, o Windows mostraria só
            // um contorno e a janela ficaria parada, como um fantasma.
            let (outer, pointer, down, pressed) = vctx.input(|i| {
                (
                    i.viewport().outer_rect,
                    i.pointer.latest_pos(),
                    i.pointer.primary_down(),
                    i.pointer.primary_pressed(),
                )
            });
            // bordas que redimensionam (antes da barra, que também arrasta)
            let mut resizing = false;
            if resizable {
                let r = vctx.screen_rect();
                if let Some(d) = vctx
                    .input(|i| i.pointer.hover_pos())
                    .and_then(|p| resize_dir(r, p, EDGE))
                {
                    vctx.set_cursor_icon(resize_cursor(d));
                    if pressed {
                        if let (Some(o), Some(p)) = (outer, pointer) {
                            resize = Some(ResizeDrag {
                                dir: d,
                                start_pointer: o.min + p.to_vec2(),
                                start_rect: o,
                            });
                        }
                    }
                }
            }
            if let Some(rd) = resize {
                if down {
                    resizing = true;
                    vctx.set_cursor_icon(resize_cursor(rd.dir));
                    if let (Some(o), Some(p)) = (outer, pointer) {
                        let delta = o.min + p.to_vec2() - rd.start_pointer;
                        if let Some(r) = resized_rect(rd.start_rect, rd.dir, delta, min_inner) {
                            if (r.min - o.min).length() >= 0.5 {
                                vctx.send_viewport_cmd(ViewportCommand::OuterPosition(r.min));
                            }
                            if (r.size() - o.size()).length() >= 0.5 {
                                vctx.send_viewport_cmd(ViewportCommand::InnerSize(r.size()));
                            }
                        }
                    }
                    vctx.request_repaint();
                } else {
                    resize = None;
                }
            }
            let visuals = vctx.style().visuals.clone();
            let bar = egui::TopBottomPanel::top(key.with("bar"))
                .exact_height(BAR_H)
                .frame(
                    egui::Frame::NONE
                        .fill(visuals.faint_bg_color)
                        .inner_margin(egui::Margin::symmetric(4, 1)),
                )
                .show(vctx, |ui| {
                    title_bar(ui, &title, has_close, lang, Sense::click_and_drag())
                })
                .inner;
            if bar.close {
                if let Some(o) = open.as_deref_mut() {
                    *o = false;
                }
            }
            let outer_min = outer.map(|r| r.min);
            let grab_started = bar.area.drag_started() || bar.grip.drag_started();
            if !resizing && grab_started {
                moving = pointer.map(|p| p.to_vec2());
            }
            if !resizing && bar.grip.drag_started() {
                if let Some(p) = outer_min {
                    grip = Some(GripDrag::new(p, Instant::now()));
                }
            }
            // a janela acompanha o ponteiro enquanto o botão estiver apertado
            let released = moving.is_some() && !down;
            if let Some(grab) = moving {
                if down {
                    if let (Some(o), Some(p)) = (outer_min, pointer) {
                        if let Some(to) = moved_window_pos(o, p, grab) {
                            if (to - o).length() >= 0.5 {
                                vctx.send_viewport_cmd(ViewportCommand::OuterPosition(to));
                            }
                        }
                    }
                    vctx.request_repaint();
                } else {
                    moving = None;
                }
            }
            // arrastando pelo `:::`: acopla ao soltar sobre a janela de origem
            if let (Some(g), Some(p)) = (grip.as_mut(), outer_min) {
                let over = parent.is_some_and(|r| r.contains(grip_point(p)));
                if released {
                    if over {
                        dock_now = parent.map(|r| pos2(p.x - r.min.x, p.y - r.min.y));
                    }
                    grip = None;
                } else if g.step(p, Instant::now()) != GripPhase::Waiting {
                    hint = over;
                }
            }
            let panel = egui::CentralPanel::default().frame(
                egui::Frame::NONE
                    .fill(visuals.window_fill)
                    .inner_margin(egui::Margin::same(MARGIN as i8)),
            );
            let content = panel
                .show(vctx, |ui| {
                    let f = add.take()?;
                    if measure {
                        // mede o conteúdo numa largura fixa (o texto quebra
                        // sempre do mesmo jeito, sem oscilar)
                        let sizing = auto_w && natural_w.is_none();
                        let w = match (auto_w, natural_w) {
                            (true, Some(n)) => n,
                            (true, None) => fit_w,
                            (false, _) => ui.available_width(),
                        };
                        let max = Rect::from_min_size(ui.cursor().min, vec2(w, max_inner.y));
                        let mut ub = egui::UiBuilder::new().max_rect(max);
                        if sizing {
                            ub = ub.sizing_pass();
                        }
                        let r = ui.scope_builder(ub, f);
                        result = Some(r.inner);
                        let size = r.response.rect.size();
                        if sizing {
                            new_natural_w = Some(size.x.clamp(1.0, fit_w.max(1.0)));
                            ui.ctx().request_repaint();
                        }
                        Some(size)
                    } else {
                        result = Some(f(ui));
                        None
                    }
                })
                .inner;
            if let Some(size) = content {
                {
                    let cur = vctx.screen_rect().size();
                    let want = fit_inner(size, min_inner).min(max_inner);
                    let want = vec2(
                        if auto_w { want.x } else { cur.x },
                        if auto_h { want.y } else { cur.y },
                    );
                    if (want - cur).abs().max_elem() > 1.0 {
                        vctx.send_viewport_cmd(ViewportCommand::InnerSize(want));
                        if frames < 3 {
                            if let Some(r) = parent {
                                let p = pos2(
                                    r.center().x - want.x * 0.5,
                                    (r.center().y - want.y * 0.5).max(r.top()),
                                );
                                vctx.send_viewport_cmd(ViewportCommand::OuterPosition(p));
                            }
                        }
                    }
                }
            }
            // as janelas sem moldura do sistema não têm borda: desenha uma
            vctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                key.with("border"),
            ))
            .rect_stroke(
                vctx.screen_rect(),
                0.0,
                visuals.widgets.noninteractive.bg_stroke,
                StrokeKind::Inside,
            );
        });
        st.frames = frames.saturating_add(1);
        st.grip = grip;
        st.moving = moving;
        st.resize = resize;
        st.hint = hint;
        if new_natural_w.is_some() {
            st.natural_w = new_natural_w;
        }
        if st.grip.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if let Some(p) = dock_now {
            st.docked = true;
            st.dock_pos = Some(p);
            st.hint = false;
            ctx.request_repaint();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrastar_pelo_grip_move_para_e_decide() {
        let t0 = Instant::now();
        let mut g = GripDrag::new(pos2(10.0, 10.0), t0);
        assert_eq!(g.step(pos2(10.0, 10.0), t0), GripPhase::Waiting);
        assert_eq!(g.step(pos2(50.0, 10.0), t0 + SETTLE / 4), GripPhase::Moving);
        assert_eq!(g.step(pos2(50.0, 10.0), t0 + SETTLE / 2), GripPhase::Moving);
        assert_eq!(
            g.step(pos2(50.2, 10.1), t0 + SETTLE * 2),
            GripPhase::Settled,
            "tremida não é movimento"
        );
        assert_eq!(
            g.step(pos2(f32::NAN, 0.0), t0),
            GripPhase::Waiting,
            "posição inválida"
        );
        let mut parado = GripDrag::new(pos2(0.0, 0.0), t0);
        assert_eq!(
            parado.step(pos2(0.0, 0.0), t0 + GRIP_GIVE_UP),
            GripPhase::GaveUp
        );
    }

    #[test]
    fn bordas_que_redimensionam() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 100.0));
        assert_eq!(
            resize_dir(r, pos2(1.0, 1.0), EDGE),
            Some(ResizeDirection::NorthWest)
        );
        assert_eq!(
            resize_dir(r, pos2(199.0, 99.0), EDGE),
            Some(ResizeDirection::SouthEast)
        );
        assert_eq!(
            resize_dir(r, pos2(100.0, 1.0), EDGE),
            Some(ResizeDirection::North)
        );
        assert_eq!(
            resize_dir(r, pos2(198.0, 50.0), EDGE),
            Some(ResizeDirection::East)
        );
        assert_eq!(resize_dir(r, pos2(100.0, 50.0), EDGE), None, "meio");
        assert_eq!(resize_dir(r, pos2(300.0, 50.0), EDGE), None, "fora");
        assert_eq!(resize_dir(r, pos2(f32::NAN, 1.0), EDGE), None);
    }

    #[test]
    fn abre_centrada_na_origem_e_cabe_no_monitor() {
        let parent = Rect::from_min_size(pos2(100.0, 50.0), vec2(1000.0, 800.0));
        let [x, y, w, h] = initial_geom(Some(parent), vec2(400.0, 200.0), None);
        assert_eq!([x + w / 2.0, y + h / 2.0], [600.0, 450.0]);
        let [_, _, w, h] = initial_geom(
            Some(parent),
            vec2(5000.0, 5000.0),
            Some(vec2(1920.0, 1080.0)),
        );
        assert!(w <= 1920.0 && h <= 1080.0);
        let [x, y, w, h] = initial_geom(None, vec2(f32::NAN, f32::INFINITY), None);
        assert!([x, y, w, h].iter().all(|v| v.is_finite()));
        // janela de origem baixa: o topo não sobe acima dela
        let baixa = Rect::from_min_size(pos2(0.0, 300.0), vec2(800.0, 100.0));
        assert!(initial_geom(Some(baixa), vec2(300.0, 400.0), None)[1] >= 300.0);
    }

    #[test]
    fn ajuste_ao_conteudo_e_soltar_da_janela_acoplada() {
        let s = fit_inner(vec2(300.0, 100.0), Vec2::ZERO);
        assert_eq!(s, vec2(300.0 + 2.0 * MARGIN, 100.0 + 2.0 * MARGIN + BAR_H));
        assert_eq!(fit_inner(vec2(f32::NAN, 10.0), Vec2::ZERO).x, 420.0);
        assert!(
            fit_inner(vec2(10.0, 10.0), vec2(500.0, 0.0)).x >= 500.0,
            "respeita o mínimo"
        );
        let win = Rect::from_min_size(pos2(20.0, 30.0), vec2(300.0, 200.0));
        assert_eq!(
            undock_geom(Some(pos2(100.0, 100.0)), win, vec2(50.0, -10.0)),
            Some([170.0, 120.0, 300.0, 200.0])
        );
        assert_eq!(undock_geom(None, win, Vec2::ZERO), None);
        assert_eq!(
            undock_geom(Some(pos2(f32::NAN, 0.0)), win, Vec2::ZERO),
            None
        );
    }

    /// O Neoveg move a janela ele mesmo: o ponto agarrado na barra fica sob
    /// o ponteiro, sem depender do arraste do Windows.
    #[test]
    fn arrastar_a_barra_mantem_o_ponto_agarrado_sob_o_ponteiro() {
        // janela em (100, 50), agarrada a (30, 10) do canto; o ponteiro foi
        // para (70, 40) dentro da janela, ou seja (170, 90) na tela
        let to = moved_window_pos(pos2(100.0, 50.0), pos2(70.0, 40.0), vec2(30.0, 10.0));
        assert_eq!(to, Some(pos2(140.0, 80.0)));
        // ponteiro parado no ponto agarrado: a janela não se move
        let same = moved_window_pos(pos2(100.0, 50.0), pos2(30.0, 10.0), vec2(30.0, 10.0));
        assert_eq!(same, Some(pos2(100.0, 50.0)));
        assert_eq!(
            moved_window_pos(pos2(f32::NAN, 0.0), pos2(1.0, 1.0), Vec2::ZERO),
            None
        );
    }

    /// Redimensionar pela borda move só aquela borda e respeita o mínimo.
    #[test]
    fn redimensionar_pela_borda_respeita_o_minimo() {
        let start = Rect::from_min_size(pos2(100.0, 100.0), vec2(400.0, 300.0));
        let min = vec2(200.0, 100.0);
        let se = resized_rect(start, ResizeDirection::SouthEast, vec2(50.0, 20.0), min);
        assert_eq!(
            se,
            Some(Rect::from_min_max(pos2(100.0, 100.0), pos2(550.0, 420.0)))
        );
        // borda esquerda: o canto direito fica onde estava
        let w = resized_rect(start, ResizeDirection::West, vec2(-40.0, 0.0), min);
        assert_eq!(
            w,
            Some(Rect::from_min_max(pos2(60.0, 100.0), pos2(500.0, 400.0)))
        );
        // encolher demais para no mínimo, sem inverter
        let nw = resized_rect(start, ResizeDirection::NorthWest, vec2(900.0, 900.0), min)
            .expect("retângulo válido");
        assert_eq!(nw.size(), min);
        assert_eq!(nw.max, start.max);
        let n = resized_rect(start, ResizeDirection::North, vec2(0.0, 500.0), min)
            .expect("retângulo válido");
        assert_eq!(n.height(), 100.0);
        assert_eq!(
            resized_rect(start, ResizeDirection::East, vec2(f32::INFINITY, 0.0), min),
            None
        );
    }
}
