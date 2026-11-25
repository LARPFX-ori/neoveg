//! Janela *Movimento da Trilha* 2D com a disposição do VEGAS Pro (manual do
//! VEGAS Pro 21, pp. 412–413; `docs_obsidian/ui_references/
//! referencia_visual_janelas.md`): predefinição no alto, barra de
//! ferramentas com a composição da trilha e as travas, grade de
//! propriedades, área de trabalho clara com o retângulo e os eixos do
//! objeto e a linha do tempo de quadros-chave embaixo.
//!
//! Reaproveita as peças de `kf_window` e a geometria de `pan_crop_geom`.

use crate::icons::{self, Icon};
use crate::kf_window::{self, KfNav, KfRow, Workspace};
use crate::pan_crop_geom::{BoxGeom, Hit};
use crate::App;
use eframe::egui::{self, vec2, Color32, CursorIcon, Pos2, Sense, Stroke, Vec2};
use neoveg_common::curve::{Curve, Id, Keyframes};
use neoveg_common::i18n::{tr, tr_args, Language};
use neoveg_common::time::Ticks;
use neoveg_engine::model::{BlendMode, MotionKey, ShadowGlowKey};

/// Maior escala aceita (a mesma faixa da janela antiga, com folga).
const MAX_SCALE: f32 = 100.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct TmDrag {
    hit: Hit,
    start: BoxGeom,
    start_ptr: Vec2,
}

/// Estado da janela (não vai para o projeto).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TrackMotionUi {
    pub enable_rotation: bool,
    pub snap: bool,
    pub object_space: bool,
    pub prevent_move_x: bool,
    pub prevent_move_y: bool,
    pub lock_aspect: bool,
    pub about_center: bool,
    pub prevent_scale_x: bool,
    pub prevent_scale_y: bool,
    /// Modo 3D: impedir movimento em Z.
    pub prevent_move_z: bool,
    /// Modo 3D: disposição das vistas.
    pub layout: Layout3d,
    /// Modo 3D: área de trabalho de cada vista (Topo, Esquerda, Frente,
    /// Perspectiva).
    pub views: [Workspace; 4],
    pub sync_cursor: bool,
    /// Cursor próprio da linha de quadros-chave quando não sincronizado
    /// (tempo da timeline).
    pub local: Ticks,
    /// Passo do encaixe da rotação, em graus.
    pub snap_rotation: f32,
    /// Editando o movimento Pai/Filho do grupo em vez do da trilha.
    pub parent_mode: bool,
    /// Linha ativa na trilha de quadros-chave: 0 = Posição, 1 = Sombra 2D, 2 = Brilho 2D.
    pub active_row: usize,
    pub preset: usize,
    pub ws: Workspace,
    drag: Option<TmDrag>,
}

impl Default for TrackMotionUi {
    fn default() -> Self {
        TrackMotionUi {
            enable_rotation: true,
            snap: false,
            object_space: false,
            prevent_move_x: false,
            prevent_move_y: false,
            lock_aspect: true,
            about_center: true,
            prevent_scale_x: false,
            prevent_scale_y: false,
            prevent_move_z: false,
            layout: Layout3d::FourViews,
            views: [Workspace::default(); 4],
            sync_cursor: true,
            local: Ticks::ZERO,
            snap_rotation: 5.0,
            parent_mode: false,
            active_row: 0,
            preset: 0,
            ws: Workspace {
                grid: 10.0,
                ..Workspace::default()
            },
            drag: None,
        }
    }
}

const PRESETS: [&str; 2] = ["(Padrão)", "Picture-in-picture"];

/// Disposição das vistas do modo 3D (lista *Layout*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Layout3d {
    /// Topo, Esquerda e Frente em cima, Perspectiva grande embaixo.
    FourViews,
    Perspective,
    Front,
}

impl Layout3d {
    const ALL: [Layout3d; 3] = [Layout3d::FourViews, Layout3d::Perspective, Layout3d::Front];

    fn name(self) -> &'static str {
        match self {
            Layout3d::FourViews => "Quatro vistas",
            Layout3d::Perspective => "Só perspectiva",
            Layout3d::Front => "Só frente",
        }
    }
}

/// Vistas da área de trabalho no modo 3D.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum View3d {
    Top,
    Left,
    Front,
    Perspective,
}

impl View3d {
    fn name(self) -> &'static str {
        match self {
            View3d::Top => "Topo",
            View3d::Left => "Esquerda",
            View3d::Front => "Frente",
            View3d::Perspective => "Perspectiva",
        }
    }

    fn index(self) -> usize {
        match self {
            View3d::Top => 0,
            View3d::Left => 1,
            View3d::Front => 2,
            View3d::Perspective => 3,
        }
    }
}

/// Ponto do mundo (pixels, centro do quadro na origem) na vista, também
/// com o centro na origem. Topo: Z para cima; Esquerda: Z para a esquerda.
pub(crate) fn view_point(view: View3d, p: [f32; 3], frame: Vec2) -> Option<Vec2> {
    match view {
        View3d::Front => Some(vec2(p[0], p[1])),
        View3d::Top => Some(vec2(p[0], -p[2])),
        View3d::Left => Some(vec2(-p[2], p[1])),
        View3d::Perspective => neoveg_engine::engine::motion3d::project(p, frame.x, frame.y)
            .map(|(x, y)| vec2(x - frame.x * 0.5, y - frame.y * 0.5)),
    }
}

/// Arrasto de `delta` pixels numa vista vira deslocamento em X/Y/Z
/// (frações do quadro), respeitando as travas.
pub(crate) fn view_drag(
    view: View3d,
    delta: Vec2,
    frame: Vec2,
    st: &TrackMotionUi,
) -> (f32, f32, f32) {
    let d = vec2(finite_or(delta.x, 0.0), finite_or(delta.y, 0.0));
    let (mut dx, mut dy, mut dz) = match view {
        View3d::Front | View3d::Perspective => (d.x, d.y, 0.0),
        View3d::Top => (d.x, 0.0, -d.y),
        View3d::Left => (0.0, d.y, -d.x),
    };
    if st.prevent_move_x {
        dx = 0.0;
    }
    if st.prevent_move_y {
        dy = 0.0;
    }
    if st.prevent_move_z {
        dz = 0.0;
    }
    let fw = frame.x.max(1.0);
    (dx / fw, dy / frame.y.max(1.0), dz / fw)
}

#[derive(Clone, Debug, PartialEq)]
enum TmAction {
    Restore,
    Center,
    MatchOutput,
    Preset(usize),
    Nav(KfNav),
    MoveKey(usize, usize, Ticks),
    DeleteKey(usize, usize),
    SetCurve(usize, usize, Curve),
    Blend(BlendMode),
    /// *Alfa da origem 3D* ligado/desligado (vale para trilha e para Pai/Filho 3D).
    Mode3d(bool),
    ToggleShadow(bool),
    ToggleGlow(bool),
    UpdateShadow(ShadowGlowKey),
    UpdateGlow(ShadowGlowKey),
}

fn finite_or(v: f32, d: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        d
    }
}

/// Retângulo da camada no quadro (pixels), a partir do quadro-chave.
pub(crate) fn motion_to_geom(k: &MotionKey, frame: Vec2) -> BoxGeom {
    let sx = finite_or(k.scale_x, 1.0).clamp(0.001, MAX_SCALE);
    let sy = finite_or(k.scale_y, 1.0).clamp(0.001, MAX_SCALE);
    BoxGeom {
        center: frame * 0.5 + vec2(finite_or(k.x, 0.0) * frame.x, finite_or(k.y, 0.0) * frame.y),
        half: vec2(frame.x * sx * 0.5, frame.y * sy * 0.5),
        rot_deg: finite_or(k.rot, 0.0),
    }
}

/// Quadro-chave a partir do retângulo da camada.
pub(crate) fn geom_to_motion(g: &BoxGeom, frame: Vec2, base: &MotionKey) -> MotionKey {
    let fw = frame.x.max(1.0);
    let fh = frame.y.max(1.0);
    MotionKey {
        x: finite_or((g.center.x - fw * 0.5) / fw, 0.0),
        y: finite_or((g.center.y - fh * 0.5) / fh, 0.0),
        scale_x: finite_or(g.half.x * 2.0 / fw, 1.0).clamp(0.001, MAX_SCALE),
        scale_y: finite_or(g.half.y * 2.0 / fh, 1.0).clamp(0.001, MAX_SCALE),
        rot: finite_or(g.rot_deg, 0.0),
        ..base.clone()
    }
}

/// Desloca o centro respeitando as travas de movimento, no espaço do
/// objeto (eixos girados) ou no da câmera, e o encaixe na grade.
pub(crate) fn constrained_move(g: &BoxGeom, delta: Vec2, st: &TrackMotionUi) -> BoxGeom {
    let d = vec2(finite_or(delta.x, 0.0), finite_or(delta.y, 0.0));
    let (sn, cs) = g.rot_deg.to_radians().sin_cos();
    // no espaço do objeto as travas valem nos eixos do objeto
    let mut l = if st.object_space {
        vec2(d.x * cs + d.y * sn, -d.x * sn + d.y * cs)
    } else {
        d
    };
    if st.prevent_move_x {
        l.x = 0.0;
    }
    if st.prevent_move_y {
        l.y = 0.0;
    }
    let d = if st.object_space {
        vec2(l.x * cs - l.y * sn, l.x * sn + l.y * cs)
    } else {
        l
    };
    let mut c = g.center + d;
    if st.snap && st.ws.grid.is_finite() && st.ws.grid >= 1.0 {
        let gs = st.ws.grid;
        if !st.prevent_move_x {
            c.x = (c.x / gs).round() * gs;
        }
        if !st.prevent_move_y {
            c.y = (c.y / gs).round() * gs;
        }
    }
    BoxGeom { center: c, ..*g }
}

/// Redimensiona pela alça `i` respeitando as travas de escala.
pub(crate) fn constrained_resize(g: &BoxGeom, i: usize, p: Vec2, st: &TrackMotionUi) -> BoxGeom {
    let mut r = g.resize(i, p, st.lock_aspect, st.about_center);
    // eixo travado: volta ao tamanho original e o centro não anda nele
    let mut cl = g.to_local(r.center);
    if st.prevent_scale_x {
        r.half.x = g.half.x;
        cl.x = 0.0;
    }
    if st.prevent_scale_y {
        r.half.y = g.half.y;
        cl.y = 0.0;
    }
    r.center = g.to_frame(cl);
    r
}

/// Gira com o encaixe de rotação, quando ligado.
pub(crate) fn constrained_rotate(g: &BoxGeom, from: Vec2, to: Vec2, st: &TrackMotionUi) -> BoxGeom {
    let mut r = g.rotate(from, to);
    if st.snap && st.snap_rotation.is_finite() && st.snap_rotation >= 0.1 {
        r.rot_deg = (r.rot_deg / st.snap_rotation).round() * st.snap_rotation;
    }
    r
}

impl App {
    pub(crate) fn track_motion_window(&mut self, ctx: &egui::Context) {
        self.dlg.tm_kb_rect = None;
        let Some(tid) = self.dlg.track_motion else {
            return;
        };
        let Some(ti) = self.project.tracks.iter().position(|t| t.id == tid) else {
            self.dlg.track_motion = None;
            return;
        };
        let s = self.project.settings.clone();
        let frame = vec2(s.width.max(1) as f32, s.height.max(1) as f32);
        let tol = s.frame_dur().div_i64(2);
        let mut st = self.dlg.tm.clone();
        let has_children = self.project.has_track_children(tid);
        if !has_children {
            st.parent_mode = false;
        }
        if st.sync_cursor {
            st.local = self.cursor;
        }
        let t = st.local.max(Ticks::ZERO);
        let track = &self.project.tracks[ti];
        let motion = if st.parent_mode {
            track.parent_motion.clone()
        } else {
            track.motion.clone()
        };
        let blend = track.blend;
        let is_3d = if st.parent_mode {
            track.parent_motion_3d
        } else {
            track.motion_3d
        };
        let shadow_enabled = track.shadow_2d_enabled;
        let shadow_kf = track.shadow_2d.clone();
        let glow_enabled = track.glow_2d_enabled;
        let glow_kf = track.glow_2d.clone();
        let cur_shadow = shadow_kf.eval(t);
        let cur_glow = glow_kf.eval(t);
        let name = track.name.clone();
        let cur = motion.eval(t);
        let length = self
            .project
            .duration()
            .max(t)
            .max(motion.keys.iter().map(|k| k.t).max().unwrap_or(Ticks::ZERO))
            .max(Ticks::from_secs_i64(10));
        let key_idx = motion
            .keys
            .iter()
            .rposition(|k| k.t <= t + tol)
            .unwrap_or(0);
        let lang = self.lang();
        let theme = self.theme.clone();
        let mut open = true;
        let mut new_key: Option<MotionKey> = None;
        let mut actions: Vec<TmAction> = Vec::new();
        let mut seek: Option<Ticks> = None;
        let mut kb_rect: Option<egui::Rect> = None;
        crate::dialog_window::DialogWindow::new(tr_args(
            lang,
            "Movimento da trilha — {name}",
            &[("name", &name)],
        ))
        .id(egui::Id::new("track_motion_win"))
        .open(&mut open)
        .default_size([1080.0, 700.0])
        .min_size([640.0, 420.0])
        .resizable(true)
        .show(ctx, |ui| {
            // teclas da janela (manual do VEGAS Pro 21, atalhos "Track Motion"):
            // valem na janela solta (o teclado é dela) ou, acoplada, com o
            // mouse sobre ela
            let docked = ui.ctx().viewport_id() == egui::ViewportId::ROOT;
            let area = ui.max_rect();
            if docked {
                kb_rect = Some(area);
            }
            if !ui.ctx().wants_keyboard_input() && (!docked || ui.rect_contains_pointer(area)) {
                for k in take_window_keys(ui) {
                    apply_window_key(&mut st, k);
                }
            }
            ui.horizontal(|ui| {
                ui.label(tr(lang, "Predefinição:"));
                let sel = PRESETS.get(st.preset).copied().unwrap_or(PRESETS[0]);
                egui::ComboBox::from_id_salt("tm_preset")
                    .width((ui.available_width() - 8.0).max(120.0))
                    .selected_text(tr(lang, sel))
                    .show_ui(ui, |ui| {
                        for (i, p) in PRESETS.iter().enumerate() {
                            if ui.selectable_label(st.preset == i, tr(lang, p)).clicked() {
                                st.preset = i;
                                actions.push(TmAction::Preset(i));
                            }
                        }
                    });
            });
            self.tm_toolbar(ui, &mut st, blend, is_3d, has_children, &mut actions);
            ui.separator();
            let strip_h = 150.0;
            let top_h = (ui.available_height() - strip_h).max(200.0);
            let top_w = ui.available_width();
            ui.allocate_ui(vec2(top_w, top_h), |ui| {
                ui.horizontal_top(|ui| {
                    // coluna das propriedades: layout vertical explícito; herdando o
                    // horizontal do pai, as seções saíam lado a lado e se sobrepunham
                    ui.allocate_ui_with_layout(
                        vec2(256.0, top_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(256.0);
                            egui::ScrollArea::vertical()
                                .id_salt("tm_props")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    if let Some(k) = self.tm_properties(
                                        ui,
                                        &motion,
                                        &cur,
                                        key_idx,
                                        frame,
                                        is_3d,
                                        shadow_enabled,
                                        &cur_shadow,
                                        glow_enabled,
                                        &cur_glow,
                                        &mut st,
                                        &mut actions,
                                        lang,
                                    ) {
                                        new_key = Some(k);
                                    }
                                });
                        },
                    );
                    let canvas = vec2(ui.available_width().max(64.0), top_h.max(64.0));
                    let edited = if is_3d {
                        self.tm_workspace_3d(ui, canvas, &cur, frame, &mut st, &mut actions, lang)
                    } else {
                        self.tm_workspace(ui, canvas, &cur, frame, &mut st, &mut actions, lang)
                    };
                    if let Some(k) = edited {
                        new_key = Some(k);
                    }
                });
            });
            ui.add_space(2.0);
            let rows = [
                KfRow {
                    label: tr(lang, "Posição"),
                    keys: motion.keys.iter().map(|k| (k.t, k.curve)).collect(),
                    checked: None,
                    selected: st.active_row == 0,
                },
                KfRow {
                    label: tr(lang, "Sombra 2D"),
                    keys: shadow_kf.keys.iter().map(|k| (k.t, k.curve)).collect(),
                    checked: Some(shadow_enabled),
                    selected: st.active_row == 1,
                },
                KfRow {
                    label: tr(lang, "Brilho 2D"),
                    keys: glow_kf.keys.iter().map(|k| (k.t, k.curve)).collect(),
                    checked: Some(glow_enabled),
                    selected: st.active_row == 2,
                },
            ];
            let mut sync = st.sync_cursor;
            let fmt = |x: Ticks| self.fmt_time(x);
            let out = kf_window::keyframe_strip(
                ui,
                egui::Id::new("tm_strip"),
                &rows,
                length,
                t,
                &mut sync,
                &fmt,
                &theme,
                lang,
            );
            if let Some(ri) = out.select_row {
                st.active_row = ri;
            }
            if let Some(ri) = out.toggle_row {
                if ri == 1 {
                    actions.push(TmAction::ToggleShadow(!shadow_enabled));
                } else if ri == 2 {
                    actions.push(TmAction::ToggleGlow(!glow_enabled));
                }
            }
            if sync != st.sync_cursor {
                st.sync_cursor = sync;
                if sync {
                    seek = Some(t);
                }
            }
            if let Some(x) = out.seek {
                seek = Some(x);
            }
            if let Some(n) = out.nav {
                actions.push(TmAction::Nav(n));
            }
            if let Some((ri, ki, x)) = out.move_key {
                actions.push(TmAction::MoveKey(ri, ki, x));
            }
            if let Some((ri, ki, c)) = out.set_curve {
                actions.push(TmAction::SetCurve(ri, ki, c));
            }
            if let Some((ri, ki)) = out.delete_key {
                actions.push(TmAction::DeleteKey(ri, ki));
            }
        });
        let active_times: Vec<Ticks> = match st.active_row {
            1 => shadow_kf.keys.iter().map(|k| k.t).collect(),
            2 => glow_kf.keys.iter().map(|k| k.t).collect(),
            _ => motion.keys.iter().map(|k| k.t).collect(),
        };
        for a in &actions {
            if let TmAction::Nav(n) = a {
                if let Some(x) = kf_window::nav_target(&active_times, t, *n) {
                    seek = Some(x);
                }
            }
        }
        if let Some(x) = seek {
            let x = s.fps.quantize(x.max(Ticks::ZERO));
            st.local = x;
            if st.sync_cursor {
                self.set_cursor(x);
            }
        }
        let parent = st.parent_mode;
        let active_row = st.active_row;
        let changed = self.apply_track_motion(
            tid, parent, active_row, t, tol, frame, length, new_key, &actions,
        );
        self.dlg.tm = st;
        self.dlg.tm_kb_rect = kb_rect;
        if changed {
            self.dialog_change();
        }
        if !open {
            self.dlg.track_motion = None;
            self.dlg.tm.drag = None;
        }
    }

    /// Barra de ferramentas (manual do VEGAS Pro 21, pp. 412–413).
    fn tm_toolbar(
        &self,
        ui: &mut egui::Ui,
        st: &mut TrackMotionUi,
        blend: BlendMode,
        is_3d: bool,
        has_children: bool,
        actions: &mut Vec<TmAction>,
    ) {
        let lang = self.lang();
        let th = &self.theme;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let label_3d = tr(lang, "Alfa da origem 3D");
            egui::ComboBox::from_id_salt("tm_blend")
                .selected_text(if is_3d { label_3d } else { blend.name_in(lang) })
                .show_ui(ui, |ui| {
                    if ui.selectable_label(is_3d, label_3d).clicked() {
                        actions.push(TmAction::Mode3d(true));
                    }
                    for b in BlendMode::ALL {
                        if ui
                            .selectable_label(!is_3d && b == blend, b.name_in(lang))
                            .clicked()
                        {
                            actions.push(TmAction::Blend(b));
                        }
                    }
                })
                .response
                .on_hover_text(tr(lang, "Modo de composição da trilha"));
            if is_3d {
                egui::ComboBox::from_id_salt("tm_layout3d")
                    .selected_text(tr(lang, st.layout.name()))
                    .show_ui(ui, |ui| {
                        for l in Layout3d::ALL {
                            ui.selectable_value(&mut st.layout, l, tr(lang, l.name()));
                        }
                    })
                    .response
                    .on_hover_text(tr(lang, "Disposição das vistas"));
            }
            ui.separator();
            let toggle = |ui: &mut egui::Ui, icon: Icon, on: &mut bool, tip: &str| {
                if icons::icon_button(ui, icon, None, *on, true, th)
                    .on_hover_text(tr(lang, tip))
                    .clicked()
                {
                    *on = !*on;
                }
            };
            toggle(
                ui,
                Icon::ResetSpin,
                &mut st.enable_rotation,
                "Habilitar rotação",
            );
            toggle(ui, Icon::MagnetSnap, &mut st.snap, "Encaixar na grade");
            toggle(
                ui,
                Icon::ObjectSpace,
                &mut st.object_space,
                "Editar no espaço do objeto",
            );
            ui.separator();
            toggle(
                ui,
                Icon::MoveYOnly,
                &mut st.prevent_move_x,
                "Impedir movimento (X)",
            );
            toggle(
                ui,
                Icon::MoveXOnly,
                &mut st.prevent_move_y,
                "Impedir movimento (Y)",
            );
            if is_3d {
                toggle(
                    ui,
                    Icon::MoveZOnly,
                    &mut st.prevent_move_z,
                    "Impedir movimento (Z)",
                );
            }
            ui.separator();
            toggle(
                ui,
                Icon::LockClosed,
                &mut st.lock_aspect,
                "Travar proporção",
            );
            toggle(
                ui,
                Icon::ScaleAboutCenter,
                &mut st.about_center,
                "Dimensionar em torno do centro",
            );
            toggle(
                ui,
                Icon::PreventScaleX,
                &mut st.prevent_scale_x,
                "Impedir dimensionar (X)",
            );
            toggle(
                ui,
                Icon::PreventScaleY,
                &mut st.prevent_scale_y,
                "Impedir dimensionar (Y)",
            );
            ui.separator();
            if ui
                .small_button("−")
                .on_hover_text(tr(lang, "Reduzir zoom"))
                .clicked()
            {
                if is_3d {
                    for v in &mut st.views {
                        v.zoom_pct = (v.zoom_pct * 0.8)
                            .clamp(kf_window::WS_MIN_ZOOM, kf_window::WS_MAX_ZOOM);
                    }
                } else {
                    st.ws.zoom_pct = (st.ws.zoom_pct * 0.8)
                        .clamp(kf_window::WS_MIN_ZOOM, kf_window::WS_MAX_ZOOM);
                }
            }
            if ui
                .small_button("+")
                .on_hover_text(tr(lang, "Ampliar zoom"))
                .clicked()
            {
                if is_3d {
                    for v in &mut st.views {
                        v.zoom_pct = (v.zoom_pct * 1.25)
                            .clamp(kf_window::WS_MIN_ZOOM, kf_window::WS_MAX_ZOOM);
                    }
                } else {
                    st.ws.zoom_pct = (st.ws.zoom_pct * 1.25)
                        .clamp(kf_window::WS_MIN_ZOOM, kf_window::WS_MAX_ZOOM);
                }
            }
            if ui
                .small_button(tr(lang, "Ajustar"))
                .on_hover_text(tr(lang, "Ajustar a área de trabalho ao quadro"))
                .clicked()
            {
                if is_3d {
                    for v in &mut st.views {
                        v.fitted = false;
                    }
                } else {
                    st.ws.fitted = false;
                }
            }
            if has_children {
                ui.separator();
                ui.selectable_value(&mut st.parent_mode, false, tr(lang, "Movimento da trilha"));
                ui.selectable_value(
                    &mut st.parent_mode,
                    true,
                    tr(lang, "Movimento Pai/Filho (Grupo)"),
                );
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn tm_properties(
        &self,
        ui: &mut egui::Ui,
        motion: &Keyframes<MotionKey>,
        cur: &MotionKey,
        key_idx: usize,
        frame: Vec2,
        is_3d: bool,
        shadow_enabled: bool,
        cur_shadow: &ShadowGlowKey,
        glow_enabled: bool,
        cur_glow: &ShadowGlowKey,
        st: &mut TrackMotionUi,
        actions: &mut Vec<TmAction>,
        lang: Language,
    ) -> Option<MotionKey> {
        let th = &self.theme;
        let g = motion_to_geom(cur, frame);
        let mut out = None;
        let sec = |name: &str| egui::Id::new(("tm_sec", name.to_string()));
        let big = frame.x.max(frame.y) * 50.0;
        kf_window::prop_section(ui, sec("pos"), tr(lang, "Posição"), true, th, |ui| {
            let mut x = g.center.x - frame.x * 0.5;
            let mut y = g.center.y - frame.y * 0.5;
            let (mut w, mut h) = (g.half.x * 2.0, g.half.y * 2.0);
            let mut ng = g;
            if kf_window::prop_num(ui, "X:", &mut x, 1.0, 2, -big..=big) {
                ng.center.x = x + frame.x * 0.5;
                out = Some(geom_to_motion(&ng, frame, cur));
            }
            if kf_window::prop_num(ui, "Y:", &mut y, 1.0, 2, -big..=big) {
                ng.center.y = y + frame.y * 0.5;
                out = Some(geom_to_motion(&ng, frame, cur));
            }
            if is_3d {
                let mut z = cur.z * frame.x;
                if kf_window::prop_num(ui, "Z:", &mut z, 1.0, 2, -big..=big) {
                    out = Some(MotionKey {
                        z: z / frame.x.max(1.0),
                        ..out.clone().unwrap_or_else(|| cur.clone())
                    });
                }
            }
            if kf_window::prop_num(ui, tr(lang, "Largura:"), &mut w, 1.0, 2, 1.0..=big) {
                let k = w / (g.half.x * 2.0).max(1e-3);
                ng.half = if st.lock_aspect {
                    g.half * k
                } else {
                    vec2(w * 0.5, g.half.y)
                };
                out = Some(geom_to_motion(&ng, frame, cur));
            }
            if kf_window::prop_num(ui, tr(lang, "Altura:"), &mut h, 1.0, 2, 1.0..=big) {
                let k = h / (g.half.y * 2.0).max(1e-3);
                ng.half = if st.lock_aspect {
                    g.half * k
                } else {
                    vec2(g.half.x, h * 0.5)
                };
                out = Some(geom_to_motion(&ng, frame, cur));
            }
        });
        kf_window::prop_section(ui, sec("rot"), tr(lang, "Rotação"), true, th, |ui| {
            if is_3d {
                let (mut ax, mut ay) = (cur.rot_x, cur.rot_y);
                if kf_window::prop_num(ui, "X:", &mut ax, 0.5, 1, -3600.0..=3600.0) {
                    out = Some(MotionKey {
                        rot_x: ax,
                        ..out.clone().unwrap_or_else(|| cur.clone())
                    });
                }
                if kf_window::prop_num(ui, "Y:", &mut ay, 0.5, 1, -3600.0..=3600.0) {
                    out = Some(MotionKey {
                        rot_y: ay,
                        ..out.clone().unwrap_or_else(|| cur.clone())
                    });
                }
            }
            let mut a = cur.rot;
            if kf_window::prop_num(ui, tr(lang, "Ângulo:"), &mut a, 0.5, 1, -3600.0..=3600.0) {
                out = Some(MotionKey {
                    rot: a,
                    ..out.clone().unwrap_or_else(|| cur.clone())
                });
            }
            let mut px = cur.pivot_x * frame.x;
            let mut py = cur.pivot_y * frame.y;
            if kf_window::prop_num(ui, tr(lang, "Pivô X:"), &mut px, 1.0, 1, -big..=big) {
                out = Some(MotionKey {
                    pivot_x: px / frame.x.max(1.0),
                    ..out.clone().unwrap_or_else(|| cur.clone())
                });
            }
            if kf_window::prop_num(ui, tr(lang, "Pivô Y:"), &mut py, 1.0, 1, -big..=big) {
                out = Some(MotionKey {
                    pivot_y: py / frame.y.max(1.0),
                    ..out.clone().unwrap_or_else(|| cur.clone())
                });
            }
            if is_3d {
                let mut pz = cur.pivot_z * frame.x;
                if kf_window::prop_num(ui, tr(lang, "Pivô Z:"), &mut pz, 1.0, 1, -big..=big) {
                    out = Some(MotionKey {
                        pivot_z: pz / frame.x.max(1.0),
                        ..out.clone().unwrap_or_else(|| cur.clone())
                    });
                }
            }
        });
        kf_window::prop_section(
            ui,
            sec("orient"),
            tr(lang, "Orientação"),
            false,
            th,
            |ui| {
                if is_3d {
                    let (mut ox, mut oy) = (cur.orient_x, cur.orient_y);
                    if kf_window::prop_num(ui, "X:", &mut ox, 0.5, 1, -3600.0..=3600.0) {
                        out = Some(MotionKey {
                            orient_x: ox,
                            ..out.clone().unwrap_or_else(|| cur.clone())
                        });
                    }
                    if kf_window::prop_num(ui, "Y:", &mut oy, 0.5, 1, -3600.0..=3600.0) {
                        out = Some(MotionKey {
                            orient_y: oy,
                            ..out.clone().unwrap_or_else(|| cur.clone())
                        });
                    }
                }
                let mut oz = cur.orient_z;
                if kf_window::prop_num(ui, "Z:", &mut oz, 0.5, 1, -3600.0..=3600.0) {
                    out = Some(MotionKey {
                        orient_z: oz,
                        ..out.clone().unwrap_or_else(|| cur.clone())
                    });
                }
            },
        );
        if st.active_row == 1 || shadow_enabled {
            kf_window::prop_section(
                ui,
                sec("shadow2d"),
                tr(lang, "Sombra 2D"),
                st.active_row == 1,
                th,
                |ui| {
                    let mut on = shadow_enabled;
                    kf_window::prop_row(ui, tr(lang, "Ativar:"), |ui| {
                        if ui.checkbox(&mut on, "").changed() {
                            actions.push(TmAction::ToggleShadow(on));
                        }
                    });
                    let mut sk = cur_shadow.clone();
                    let mut sc = false;
                    let mut ox = sk.offset_x * frame.x;
                    let mut oy = sk.offset_y * frame.y;
                    if kf_window::prop_num(
                        ui,
                        tr(lang, "Deslocamento X:"),
                        &mut ox,
                        1.0,
                        1,
                        -big..=big,
                    ) {
                        sk.offset_x = ox / frame.x.max(1.0);
                        sc = true;
                    }
                    if kf_window::prop_num(
                        ui,
                        tr(lang, "Deslocamento Y:"),
                        &mut oy,
                        1.0,
                        1,
                        -big..=big,
                    ) {
                        sk.offset_y = oy / frame.y.max(1.0);
                        sc = true;
                    }
                    let mut blur_pct = sk.blur * 100.0;
                    if kf_window::prop_num(
                        ui,
                        tr(lang, "Desfoque (%):"),
                        &mut blur_pct,
                        1.0,
                        0,
                        0.0..=100.0,
                    ) {
                        sk.blur = (blur_pct * 0.01).clamp(0.0, 1.0);
                        sc = true;
                    }
                    let mut int_pct = sk.intensity * 100.0;
                    if kf_window::prop_num(
                        ui,
                        tr(lang, "Intensidade (%):"),
                        &mut int_pct,
                        1.0,
                        0,
                        0.0..=100.0,
                    ) {
                        sk.intensity = (int_pct * 0.01).clamp(0.0, 2.0);
                        sc = true;
                    }
                    kf_window::prop_row(ui, tr(lang, "Cor:"), |ui| {
                        if ui
                            .color_edit_button_rgba_unmultiplied(&mut sk.color)
                            .changed()
                        {
                            sc = true;
                        }
                    });
                    if sc {
                        actions.push(TmAction::UpdateShadow(sk));
                    }
                },
            );
        }
        if st.active_row == 2 || glow_enabled {
            kf_window::prop_section(
                ui,
                sec("glow2d"),
                tr(lang, "Brilho 2D"),
                st.active_row == 2,
                th,
                |ui| {
                    let mut on = glow_enabled;
                    kf_window::prop_row(ui, tr(lang, "Ativar:"), |ui| {
                        if ui.checkbox(&mut on, "").changed() {
                            actions.push(TmAction::ToggleGlow(on));
                        }
                    });
                    let mut gk = cur_glow.clone();
                    let mut gc = false;
                    let mut ox = gk.offset_x * frame.x;
                    let mut oy = gk.offset_y * frame.y;
                    if kf_window::prop_num(
                        ui,
                        tr(lang, "Deslocamento X:"),
                        &mut ox,
                        1.0,
                        1,
                        -big..=big,
                    ) {
                        gk.offset_x = ox / frame.x.max(1.0);
                        gc = true;
                    }
                    if kf_window::prop_num(
                        ui,
                        tr(lang, "Deslocamento Y:"),
                        &mut oy,
                        1.0,
                        1,
                        -big..=big,
                    ) {
                        gk.offset_y = oy / frame.y.max(1.0);
                        gc = true;
                    }
                    let mut blur_pct = gk.blur * 100.0;
                    if kf_window::prop_num(
                        ui,
                        tr(lang, "Desfoque (%):"),
                        &mut blur_pct,
                        1.0,
                        0,
                        0.0..=100.0,
                    ) {
                        gk.blur = (blur_pct * 0.01).clamp(0.0, 1.0);
                        gc = true;
                    }
                    let mut int_pct = gk.intensity * 100.0;
                    if kf_window::prop_num(
                        ui,
                        tr(lang, "Intensidade (%):"),
                        &mut int_pct,
                        1.0,
                        0,
                        0.0..=100.0,
                    ) {
                        gk.intensity = (int_pct * 0.01).clamp(0.0, 2.0);
                        gc = true;
                    }
                    kf_window::prop_row(ui, tr(lang, "Cor:"), |ui| {
                        if ui
                            .color_edit_button_rgba_unmultiplied(&mut gk.color)
                            .changed()
                        {
                            gc = true;
                        }
                    });
                    if gc {
                        actions.push(TmAction::UpdateGlow(gk));
                    }
                },
            );
        }
        kf_window::prop_section(ui, sec("kf"), tr(lang, "Quadro-chave"), true, th, |ui| {
            kf_window::prop_row(ui, tr(lang, "Tipo:"), |ui| {
                let current = motion.keys.get(key_idx).map(|k| k.curve);
                let mut c = current.unwrap_or_default();
                egui::ComboBox::from_id_salt("tm_curve")
                    .selected_text(c.name_in(lang))
                    .show_ui(ui, |ui| {
                        for k in Curve::ALL.iter().take(6) {
                            ui.selectable_value(&mut c, *k, k.name_in(lang));
                        }
                    });
                if current.is_some() && Some(c) != current {
                    actions.push(TmAction::SetCurve(0, key_idx, c));
                }
            });
        });
        kf_window::prop_section(
            ui,
            sec("ws"),
            tr(lang, "Área de trabalho"),
            true,
            th,
            |ui| {
                let mut z = st.ws.zoom_pct;
                if kf_window::prop_num(
                    ui,
                    tr(lang, "Zoom (%):"),
                    &mut z,
                    0.5,
                    2,
                    kf_window::WS_MIN_ZOOM..=kf_window::WS_MAX_ZOOM,
                ) {
                    st.ws.zoom_pct = z;
                }
                kf_window::prop_num(
                    ui,
                    tr(lang, "Deslocamento X:"),
                    &mut st.ws.offset.x,
                    1.0,
                    2,
                    -1e5..=1e5,
                );
                kf_window::prop_num(
                    ui,
                    tr(lang, "Deslocamento Y:"),
                    &mut st.ws.offset.y,
                    1.0,
                    2,
                    -1e5..=1e5,
                );
            },
        );
        kf_window::prop_section(ui, sec("snap"), tr(lang, "Encaixe"), true, th, |ui| {
            kf_window::prop_num(
                ui,
                tr(lang, "Espaçamento da grade:"),
                &mut st.ws.grid,
                1.0,
                0,
                1.0..=1024.0,
            );
            kf_window::prop_num(
                ui,
                tr(lang, "Rotação:"),
                &mut st.snap_rotation,
                0.5,
                0,
                0.1..=180.0,
            );
        });
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn tm_workspace(
        &self,
        ui: &mut egui::Ui,
        size: Vec2,
        cur: &MotionKey,
        frame: Vec2,
        st: &mut TrackMotionUi,
        actions: &mut Vec<TmAction>,
        lang: Language,
    ) -> Option<MotionKey> {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        if !st.ws.fitted {
            st.ws.fit(rect, frame);
        }
        let ws = st.ws;
        // a grade de pontos do VEGAS Pro aqui segue o espaçamento de encaixe
        ws.paint_background(&painter, rect, frame, &self.theme);
        let sc = |p: Vec2| ws.to_screen(rect, frame, p);
        let ink = if self.theme.high_contrast {
            self.theme.text_primary
        } else {
            Color32::from_gray(40)
        };
        let stroke_w = if self.theme.high_contrast {
            1.5_f32
        } else {
            1.0_f32
        };
        let thin = Stroke::new(stroke_w, ink);
        // quadro de saída
        let frame_stroke_col = if self.theme.high_contrast {
            self.theme.border
        } else {
            Color32::from_gray(90)
        };
        painter.rect_stroke(
            egui::Rect::from_min_max(sc(Vec2::ZERO), sc(frame)),
            0.0,
            Stroke::new(stroke_w, frame_stroke_col),
            egui::StrokeKind::Middle,
        );
        // camada: retângulo, alças de redimensionamento, círculo, pivô e eixos X/Y do objeto
        let g = motion_to_geom(cur, frame);
        let tol = ws.frame_len(13.0);
        let ptr_screen = resp.hover_pos().or_else(|| resp.interact_pointer_pos());
        let ptr_frame = ptr_screen.map(|p| ws.to_frame(rect, frame, p));
        let active_hit = st
            .drag
            .map(|d| d.hit)
            .or_else(|| ptr_frame.map(|p| g.hit(p, tol)));
        let corners: Vec<Pos2> = g.corners().iter().map(|p| sc(*p)).collect();
        painter.add(egui::Shape::closed_line(corners.clone(), thin));
        if let Some(Hit::Handle(h_idx @ (1 | 3 | 5 | 7))) = active_hit {
            let (a, b) = match h_idx {
                1 => (corners[0], corners[1]),
                3 => (corners[1], corners[2]),
                5 => (corners[2], corners[3]),
                _ => (corners[3], corners[0]),
            };
            painter.line_segment([a, b], Stroke::new(2.5_f32, self.theme.accent));
        }
        let handle_fill = if self.theme.high_contrast {
            self.theme.accent
        } else {
            Color32::WHITE
        };
        for i in 0..8 {
            let is_hot = active_hit == Some(Hit::Handle(i));
            let sz = if is_hot { 11.0 } else { 9.0 };
            let h = egui::Rect::from_center_size(sc(g.handle(i)), vec2(sz, sz));
            painter.rect_filled(
                h,
                0.0,
                if is_hot {
                    self.theme.accent
                } else {
                    handle_fill
                },
            );
            painter.rect_stroke(
                h,
                0.0,
                Stroke::new(if is_hot { 1.5_f32 } else { stroke_w }, ink),
                egui::StrokeKind::Inside,
            );
        }
        if cur.pivot_x.abs() > 1e-4 || cur.pivot_y.abs() > 1e-4 {
            let piv = sc(g.center + vec2(cur.pivot_x * frame.x, cur.pivot_y * frame.y));
            let p_st = Stroke::new(1.2_f32, self.theme.accent);
            painter.circle_stroke(piv, 5.0, p_st);
            painter.line_segment([piv - vec2(7.0, 0.0), piv + vec2(7.0, 0.0)], p_st);
            painter.line_segment([piv - vec2(0.0, 7.0), piv + vec2(0.0, 7.0)], p_st);
        }
        let r = (g.half.x.min(g.half.y) * 0.5).max(1.0);
        let ring: Vec<Pos2> = (0..72)
            .map(|i| {
                let a = i as f32 / 72.0 * std::f32::consts::TAU;
                sc(g.to_frame(vec2(a.cos() * r, a.sin() * r)))
            })
            .collect();
        painter.add(egui::Shape::closed_line(ring, thin));
        let c = sc(g.center);
        painter.circle_stroke(c, 5.0, thin);
        let ax = sc(g.to_frame(vec2(r, 0.0)));
        let ay = sc(g.to_frame(vec2(0.0, -r)));
        painter.line_segment([c, ax], thin);
        painter.line_segment([c, ay], thin);
        let font = egui::FontId::proportional(10.0);
        let label_col = if self.theme.high_contrast {
            self.theme.accent
        } else {
            Color32::from_rgb(170, 110, 90)
        };
        painter.text(
            ax + vec2(4.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "X",
            font.clone(),
            label_col,
        );
        painter.text(
            ay - vec2(0.0, 4.0),
            egui::Align2::CENTER_BOTTOM,
            "Y",
            font,
            label_col,
        );

        // barra flutuante de zoom no canto inferior direito
        let zoom_bar_rect = kf_window::workspace_zoom_bar(
            ui,
            ui.id().with("tm_zoom_bar"),
            rect,
            frame,
            &mut st.ws,
            &self.theme,
            lang,
        );
        let over_zoom_bar = ptr_screen.is_some_and(|p| zoom_bar_rect.contains(p));

        // interação
        if let Some(p) =
            ptr_frame.filter(|_| !over_zoom_bar && (resp.hovered() || st.drag.is_some()))
        {
            let hit = st.drag.map(|d| d.hit).unwrap_or_else(|| g.hit(p, tol));
            let cur = match hit {
                Hit::Inside => match (st.prevent_move_x, st.prevent_move_y) {
                    (false, true) => crate::cursors::NeovegCursor::MoveX,
                    (true, false) => crate::cursors::NeovegCursor::MoveY,
                    _ => crate::cursors::NeovegCursor::Move,
                },
                Hit::Handle(1 | 5) => {
                    crate::cursors::NeovegCursor::System(CursorIcon::ResizeVertical)
                }
                Hit::Handle(3 | 7) => {
                    crate::cursors::NeovegCursor::System(CursorIcon::ResizeHorizontal)
                }
                Hit::Handle(0 | 4) => crate::cursors::NeovegCursor::System(CursorIcon::ResizeNwSe),
                Hit::Handle(_) => crate::cursors::NeovegCursor::System(CursorIcon::ResizeNeSw),
                Hit::Rotate if st.enable_rotation => crate::cursors::NeovegCursor::Rotate,
                Hit::Rotate => crate::cursors::NeovegCursor::System(CursorIcon::Default),
            };
            crate::cursors::render_cursor(ui.ctx(), cur, &self.theme, self.prefs.custom_cursors);
        }
        kf_window::handle_workspace_wheel_and_zoom(ui, &resp, rect, frame, &mut st.ws);
        let mut out = None;
        if !over_zoom_bar {
            if resp.drag_started_by(egui::PointerButton::Primary) {
                if let Some(p) = resp
                    .interact_pointer_pos()
                    .map(|p| ws.to_frame(rect, frame, p))
                {
                    st.drag = Some(TmDrag {
                        hit: g.hit(p, tol),
                        start: g,
                        start_ptr: p,
                    });
                }
            }
            if let (Some(d), true) = (st.drag, resp.dragged_by(egui::PointerButton::Primary)) {
                if let Some(p) = resp
                    .interact_pointer_pos()
                    .map(|p| ws.to_frame(rect, frame, p))
                {
                    let ng = match d.hit {
                        Hit::Inside => Some(constrained_move(&d.start, p - d.start_ptr, st)),
                        Hit::Handle(i) => Some(constrained_resize(&d.start, i, p, st)),
                        Hit::Rotate if st.enable_rotation => {
                            Some(constrained_rotate(&d.start, d.start_ptr, p, st))
                        }
                        Hit::Rotate => None,
                    };
                    if let Some(ng) = ng {
                        out = Some(geom_to_motion(&ng, frame, cur));
                    }
                }
            }
            if resp.drag_stopped() {
                st.drag = None;
            }
            resp.context_menu(|ui| {
                for (a, label) in [
                    (TmAction::Restore, "Restaurar"),
                    (TmAction::Center, "Centralizar"),
                    (TmAction::MatchOutput, "Combinar proporção de saída"),
                ] {
                    if ui.button(tr(lang, label)).clicked() {
                        actions.push(a);
                        ui.close_menu();
                    }
                }
            });
        }
        out
    }

    /// Área de trabalho do modo 3D: uma ou quatro vistas (lista *Layout*).
    #[allow(clippy::too_many_arguments)]
    fn tm_workspace_3d(
        &self,
        ui: &mut egui::Ui,
        size: Vec2,
        cur: &MotionKey,
        frame: Vec2,
        st: &mut TrackMotionUi,
        actions: &mut Vec<TmAction>,
        lang: Language,
    ) -> Option<MotionKey> {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let views: Vec<(View3d, egui::Rect)> = match st.layout {
            Layout3d::FourViews => {
                let top_h = rect.height() * 0.36;
                let w3 = rect.width() / 3.0;
                let mut v: Vec<(View3d, egui::Rect)> = [View3d::Top, View3d::Left, View3d::Front]
                    .into_iter()
                    .enumerate()
                    .map(|(i, view)| {
                        let r = egui::Rect::from_min_size(
                            rect.min + vec2(w3 * i as f32, 0.0),
                            vec2(w3, top_h),
                        );
                        (view, r.shrink(2.0))
                    })
                    .collect();
                v.push((
                    View3d::Perspective,
                    egui::Rect::from_min_max(egui::pos2(rect.left(), rect.top() + top_h), rect.max)
                        .shrink(2.0),
                ));
                v
            }
            Layout3d::Perspective => vec![(View3d::Perspective, rect)],
            Layout3d::Front => vec![(View3d::Front, rect)],
        };
        let mut out = None;
        for (view, r) in views {
            if let Some(k) = self.tm_view(ui, view, r, cur, frame, st, actions, lang) {
                out = Some(k);
            }
        }
        out
    }

    /// Uma vista do 3D: o quadro de saída (Frente e Perspectiva), o plano da
    /// camada e os eixos X/Y/Z do objeto. Arrastar move nos eixos da vista.
    #[allow(clippy::too_many_arguments)]
    fn tm_view(
        &self,
        ui: &mut egui::Ui,
        view: View3d,
        rect: egui::Rect,
        cur: &MotionKey,
        frame: Vec2,
        st: &mut TrackMotionUi,
        actions: &mut Vec<TmAction>,
        lang: Language,
    ) -> Option<MotionKey> {
        use neoveg_engine::engine::motion3d;
        let resp = ui.interact(
            rect,
            ui.id().with(("tm_view", view.index())),
            Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);
        let vi = view.index();
        if !st.views[vi].fitted {
            st.views[vi].fit(rect, frame);
        }
        let ws = st.views[vi];
        let edges = matches!(view, View3d::Front | View3d::Perspective);
        ws.paint_background_opt(&painter, rect, frame, &self.theme, edges);
        let to_s = |v: Vec2| ws.to_screen(rect, frame, v + frame * 0.5);
        let ink = if self.theme.high_contrast {
            self.theme.text_primary
        } else {
            Color32::from_gray(40)
        };
        let stroke_w = if self.theme.high_contrast {
            1.5_f32
        } else {
            1.0_f32
        };
        let thin = Stroke::new(stroke_w, ink);
        if edges {
            let edge_col = if self.theme.high_contrast {
                self.theme.border
            } else {
                Color32::from_gray(90)
            };
            painter.rect_stroke(
                egui::Rect::from_min_max(to_s(-frame * 0.5), to_s(frame * 0.5)),
                0.0,
                Stroke::new(stroke_w, edge_col),
                egui::StrokeKind::Middle,
            );
        }
        let (w, h) = (frame.x, frame.y);
        let corners = motion3d::corners_world(cur, w, h);
        let pts: Vec<Pos2> = corners
            .iter()
            .filter_map(|c| view_point(view, *c, frame))
            .map(to_s)
            .collect();
        if pts.len() == 4 {
            painter.add(egui::Shape::closed_line(pts, thin));
        }
        // eixos do objeto: X e Y no plano, Z pela normal
        let c0 = motion3d::layer_to_world(cur, w, h, 0.0, 0.0);
        let ex = motion3d::layer_to_world(cur, w, h, w * 0.2, 0.0);
        let ey = motion3d::layer_to_world(cur, w, h, 0.0, -h * 0.3);
        let a = [ex[0] - c0[0], ex[1] - c0[1], ex[2] - c0[2]];
        let b = [ey[0] - c0[0], ey[1] - c0[1], ey[2] - c0[2]];
        let n = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        let nl = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
        let len = w * 0.15;
        // a normal aponta para quem assiste (Z positivo) sem rotação
        let ez = [
            c0[0] - n[0] / nl * len,
            c0[1] - n[1] / nl * len,
            c0[2] - n[2] / nl * len,
        ];
        let label_col = Color32::from_rgb(170, 110, 90);
        let font = egui::FontId::proportional(10.0);
        if let Some(c) = view_point(view, c0, frame).map(to_s) {
            painter.circle_stroke(c, 4.0, thin);
            for (end, name) in [(ex, "X"), (ey, "Y"), (ez, "Z")] {
                if let Some(e) = view_point(view, end, frame).map(to_s) {
                    if (e - c).length() > 3.0 {
                        painter.line_segment([c, e], thin);
                        painter.text(e, egui::Align2::LEFT_BOTTOM, name, font.clone(), label_col);
                    }
                }
            }
        }
        painter.text(
            rect.left_top() + vec2(4.0, 2.0),
            egui::Align2::LEFT_TOP,
            tr(lang, view.name()),
            egui::FontId::proportional(11.0),
            ink,
        );
        painter.rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0_f32, self.theme.border),
            egui::StrokeKind::Inside,
        );

        // barra flutuante de zoom nesta vista 3D
        let zoom_bar_rect = kf_window::workspace_zoom_bar(
            ui,
            ui.id().with(("tm_view_zoom_bar", vi)),
            rect,
            frame,
            &mut st.views[vi],
            &self.theme,
            lang,
        );
        let ptr_screen = resp.hover_pos().or_else(|| resp.interact_pointer_pos());
        let over_zoom_bar = ptr_screen.is_some_and(|p| zoom_bar_rect.contains(p));

        // interação: roda/pinça/+/- = zoom da vista, botão do meio = deslocar a vista
        kf_window::handle_workspace_wheel_and_zoom(ui, &resp, rect, frame, &mut st.views[vi]);
        if resp.hovered() && !over_zoom_bar {
            crate::cursors::render_cursor(
                ui.ctx(),
                crate::cursors::NeovegCursor::Move,
                &self.theme,
                self.prefs.custom_cursors,
            );
        }
        let mut out = None;
        if !over_zoom_bar && resp.dragged_by(egui::PointerButton::Primary) {
            let d = resp.drag_delta();
            let (dx, dy, dz) =
                view_drag(view, vec2(ws.frame_len(d.x), ws.frame_len(d.y)), frame, st);
            if dx != 0.0 || dy != 0.0 || dz != 0.0 {
                out = Some(MotionKey {
                    x: cur.x + dx,
                    y: cur.y + dy,
                    z: cur.z + dz,
                    ..cur.clone()
                });
            }
        }
        if !over_zoom_bar {
            resp.context_menu(|ui| {
                for (a, label) in [
                    (TmAction::Restore, "Restaurar"),
                    (TmAction::Center, "Centralizar"),
                    (TmAction::MatchOutput, "Combinar proporção de saída"),
                ] {
                    if ui.button(tr(lang, label)).clicked() {
                        actions.push(a);
                        ui.close_menu();
                    }
                }
            });
        }
        out
    }

    /// Aplica o quadro-chave editado e os comandos. Devolve `true` se o
    /// projeto mudou.
    #[allow(clippy::too_many_arguments)]
    fn apply_track_motion(
        &mut self,
        tid: Id,
        parent: bool,
        active_row: usize,
        t: Ticks,
        tol: Ticks,
        frame: Vec2,
        length: Ticks,
        new_key: Option<MotionKey>,
        actions: &[TmAction],
    ) -> bool {
        let navigation_only = actions.iter().all(|a| {
            matches!(
                a,
                TmAction::Nav(KfNav::First | KfNav::Prev | KfNav::Next | KfNav::Last)
            )
        });
        if new_key.is_none() && navigation_only {
            return false;
        }
        let Some(track) = self.project.tracks.iter_mut().find(|x| x.id == tid) else {
            return false;
        };
        let mut changed = false;
        if let Some(k) = new_key {
            let m = if parent {
                &mut track.parent_motion
            } else {
                &mut track.motion
            };
            m.set(t, k, tol);
            changed = true;
        }
        for a in actions {
            match a {
                TmAction::Blend(b) => {
                    if track.blend != *b
                        || (!parent && track.motion_3d)
                        || (parent && track.parent_motion_3d)
                    {
                        track.blend = *b;
                        if parent {
                            track.parent_motion_3d = false;
                        } else {
                            track.motion_3d = false;
                        }
                        changed = true;
                    }
                    continue;
                }
                TmAction::Mode3d(on) => {
                    let flag = if parent {
                        &mut track.parent_motion_3d
                    } else {
                        &mut track.motion_3d
                    };
                    if *flag != *on {
                        *flag = *on;
                        if *on && !parent {
                            track.blend = BlendMode::SourceAlpha;
                        }
                        changed = true;
                    }
                    continue;
                }
                TmAction::ToggleShadow(on) => {
                    track.shadow_2d_enabled = *on;
                    changed = true;
                    continue;
                }
                TmAction::ToggleGlow(on) => {
                    track.glow_2d_enabled = *on;
                    changed = true;
                    continue;
                }
                TmAction::UpdateShadow(sk) => {
                    track.shadow_2d.set(t, sk.clone(), tol);
                    track.shadow_2d_enabled = true;
                    changed = true;
                    continue;
                }
                TmAction::UpdateGlow(gk) => {
                    track.glow_2d.set(t, gk.clone(), tol);
                    track.glow_2d_enabled = true;
                    changed = true;
                    continue;
                }
                TmAction::MoveKey(ri, ki, nt) => {
                    let nt = (*nt).clamp_safe(Ticks::ZERO, length);
                    let moved = match *ri {
                        1 => move_kf(&mut track.shadow_2d, *ki, nt, tol),
                        2 => move_kf(&mut track.glow_2d, *ki, nt, tol),
                        _ => {
                            let m = if parent {
                                &mut track.parent_motion
                            } else {
                                &mut track.motion
                            };
                            move_kf(m, *ki, nt, tol)
                        }
                    };
                    if moved {
                        changed = true;
                    }
                    continue;
                }
                TmAction::DeleteKey(ri, ki) => {
                    let removed = match *ri {
                        1 => remove_kf(&mut track.shadow_2d, *ki),
                        2 => remove_kf(&mut track.glow_2d, *ki),
                        _ => {
                            let m = if parent {
                                &mut track.parent_motion
                            } else {
                                &mut track.motion
                            };
                            remove_kf(m, *ki)
                        }
                    };
                    if removed {
                        changed = true;
                    }
                    continue;
                }
                TmAction::SetCurve(ri, ki, c) => {
                    let ok = match *ri {
                        1 => set_kf_curve(&mut track.shadow_2d, *ki, *c),
                        2 => set_kf_curve(&mut track.glow_2d, *ki, *c),
                        _ => {
                            let m = if parent {
                                &mut track.parent_motion
                            } else {
                                &mut track.motion
                            };
                            set_kf_curve(m, *ki, *c)
                        }
                    };
                    if ok {
                        changed = true;
                    }
                    continue;
                }
                TmAction::Nav(KfNav::Add) => {
                    match active_row {
                        1 => {
                            let v = track.shadow_2d.eval(t);
                            track.shadow_2d.set(t, v, tol);
                        }
                        2 => {
                            let v = track.glow_2d.eval(t);
                            track.glow_2d.set(t, v, tol);
                        }
                        _ => {
                            let m = if parent {
                                &mut track.parent_motion
                            } else {
                                &mut track.motion
                            };
                            let cur = m.eval(t);
                            m.set(t, cur, tol);
                        }
                    }
                    changed = true;
                    continue;
                }
                TmAction::Nav(KfNav::Delete) => {
                    let del = match active_row {
                        1 => delete_kf_at(&mut track.shadow_2d, t, tol),
                        2 => delete_kf_at(&mut track.glow_2d, t, tol),
                        _ => {
                            let m = if parent {
                                &mut track.parent_motion
                            } else {
                                &mut track.motion
                            };
                            delete_kf_at(m, t, tol)
                        }
                    };
                    if del {
                        changed = true;
                    }
                    continue;
                }
                TmAction::Nav(_) => continue,
                _ => {}
            }
            let m = if parent {
                &mut track.parent_motion
            } else {
                &mut track.motion
            };
            let cur = m.eval(t);
            match a {
                TmAction::Restore => m.set(t, MotionKey::default(), tol),
                TmAction::Center => m.set(
                    t,
                    MotionKey {
                        x: 0.0,
                        y: 0.0,
                        ..cur
                    },
                    tol,
                ),
                TmAction::MatchOutput => {
                    let g = motion_to_geom(&cur, frame);
                    let ng = BoxGeom {
                        half: vec2(g.half.x, g.half.x * frame.y / frame.x.max(1.0)),
                        ..g
                    };
                    m.set(t, geom_to_motion(&ng, frame, &cur), tol);
                }
                TmAction::Preset(0) => *m = Keyframes::default(),
                TmAction::Preset(1) => m.set(
                    t,
                    MotionKey {
                        x: 0.3,
                        y: 0.3,
                        scale_x: 0.33,
                        scale_y: 0.33,
                        ..MotionKey::default()
                    },
                    tol,
                ),
                _ => continue,
            }
            changed = true;
        }
        changed
    }
}

fn move_kf<T>(kf: &mut Keyframes<T>, ki: usize, nt: Ticks, tol: Ticks) -> bool {
    let clash = kf
        .keys
        .iter()
        .enumerate()
        .any(|(j, k)| j != ki && k.t.abs_diff(nt) <= tol);
    let Some(k) = kf.keys.get_mut(ki) else {
        return false;
    };
    if clash || k.t == nt {
        return false;
    }
    k.t = nt;
    kf.keys.sort_by_key(|k| k.t);
    true
}

fn remove_kf<T>(kf: &mut Keyframes<T>, ki: usize) -> bool {
    if kf.keys.len() > 1 && ki < kf.keys.len() {
        kf.keys.remove(ki);
        true
    } else {
        false
    }
}

fn delete_kf_at<T>(kf: &mut Keyframes<T>, t: Ticks, tol: Ticks) -> bool {
    if kf.keys.len() > 1 && kf.keys.iter().any(|k| k.t.abs_diff(t) <= tol) {
        kf.keys.retain(|k| k.t.abs_diff(t) > tol);
        true
    } else {
        false
    }
}

fn set_kf_curve<T>(kf: &mut Keyframes<T>, ki: usize, c: Curve) -> bool {
    let Some(k) = kf.keys.get_mut(ki) else {
        return false;
    };
    k.curve = c;
    true
}

/// Eixo das teclas X, Y e Z da janela Movimento da trilha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    X,
    Y,
    Z,
}

/// Ação de uma tecla da janela Movimento da trilha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowKey {
    LockAspect,
    AboutCenter,
    ObjectSpace,
    PreventMove(Axis),
    PreventScale(Axis),
    Snap,
    Rotation,
    /// Disposição 3D `1..=6` (as que o Neoveg ainda não tem são ignoradas).
    Layout(u8),
}

/// Tecla → ação, como na lista de atalhos do VEGAS Pro 21 ("Track
/// Motion"): A, C, O, X/Y/Z, Shift+X/Y/Z, F8, Shift+F8 e 1–6 (fora do
/// teclado numérico). Com Ctrl ou Alt não é tecla da janela.
pub(crate) fn window_key(ctrl: bool, shift: bool, alt: bool, key: egui::Key) -> Option<WindowKey> {
    use egui::Key as K;
    if ctrl || alt {
        return None;
    }
    Some(match (shift, key) {
        (false, K::A) => WindowKey::LockAspect,
        (false, K::C) => WindowKey::AboutCenter,
        (false, K::O) => WindowKey::ObjectSpace,
        (false, K::X) => WindowKey::PreventMove(Axis::X),
        (false, K::Y) => WindowKey::PreventMove(Axis::Y),
        (false, K::Z) => WindowKey::PreventMove(Axis::Z),
        (true, K::X) => WindowKey::PreventScale(Axis::X),
        (true, K::Y) => WindowKey::PreventScale(Axis::Y),
        (true, K::Z) => WindowKey::PreventScale(Axis::Z),
        (false, K::F8) => WindowKey::Snap,
        (true, K::F8) => WindowKey::Rotation,
        (false, K::Num1) => WindowKey::Layout(1),
        (false, K::Num2) => WindowKey::Layout(2),
        (false, K::Num3) => WindowKey::Layout(3),
        (false, K::Num4) => WindowKey::Layout(4),
        (false, K::Num5) => WindowKey::Layout(5),
        (false, K::Num6) => WindowKey::Layout(6),
        _ => return None,
    })
}

/// Teclas da janela apertadas neste quadro (consumidas, para não chegarem
/// a outro tratador).
fn take_window_keys(ui: &egui::Ui) -> Vec<WindowKey> {
    ui.input_mut(|i| {
        let presses: Vec<(egui::Modifiers, egui::Key)> = i
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } => Some((*modifiers, *key)),
                _ => None,
            })
            .collect();
        presses
            .into_iter()
            .filter_map(|(m, k)| {
                let wk = window_key(m.command || m.ctrl, m.shift, m.alt, k)?;
                i.consume_key(m, k).then_some(wk)
            })
            .collect()
    })
}

/// Com o mouse sobre a janela acoplada (`rect`), alguma tecla deste quadro
/// é da janela: a timeline não deve tratá-la.
pub(crate) fn window_owns_keys(ctx: &egui::Context, rect: Option<egui::Rect>) -> bool {
    let Some(rect) = rect else {
        return false;
    };
    ctx.input(|i| {
        i.pointer.hover_pos().is_some_and(|p| rect.contains(p))
            && i.events.iter().any(|e| {
                matches!(e, egui::Event::Key { key, pressed: true, modifiers, .. }
                    if window_key(modifiers.command || modifiers.ctrl, modifiers.shift, modifiers.alt, *key).is_some())
            })
    })
}

/// Aplica a tecla ao estado da janela (liga/desliga a opção).
pub(crate) fn apply_window_key(st: &mut TrackMotionUi, k: WindowKey) {
    match k {
        WindowKey::LockAspect => st.lock_aspect = !st.lock_aspect,
        WindowKey::AboutCenter => st.about_center = !st.about_center,
        WindowKey::ObjectSpace => st.object_space = !st.object_space,
        WindowKey::PreventMove(Axis::X) => st.prevent_move_x = !st.prevent_move_x,
        WindowKey::PreventMove(Axis::Y) => st.prevent_move_y = !st.prevent_move_y,
        WindowKey::PreventMove(Axis::Z) => st.prevent_move_z = !st.prevent_move_z,
        WindowKey::PreventScale(Axis::X) => st.prevent_scale_x = !st.prevent_scale_x,
        WindowKey::PreventScale(Axis::Y) => st.prevent_scale_y = !st.prevent_scale_y,
        // o Neoveg ainda não tem "impedir escala em Z"
        WindowKey::PreventScale(Axis::Z) => {}
        WindowKey::Snap => st.snap = !st.snap,
        WindowKey::Rotation => st.enable_rotation = !st.enable_rotation,
        WindowKey::Layout(n) => {
            if let Some(l) = usize::from(n)
                .checked_sub(1)
                .and_then(|i| Layout3d::ALL.get(i))
            {
                st.layout = *l;
            }
        }
    }
}

#[cfg(test)]
mod window_key_tests {
    use super::*;
    use egui::Key as K;

    #[test]
    fn teclas_da_janela_seguem_o_vegas() {
        assert_eq!(
            window_key(false, false, false, K::A),
            Some(WindowKey::LockAspect)
        );
        assert_eq!(
            window_key(false, false, false, K::C),
            Some(WindowKey::AboutCenter)
        );
        assert_eq!(
            window_key(false, false, false, K::O),
            Some(WindowKey::ObjectSpace)
        );
        assert_eq!(
            window_key(false, true, false, K::Y),
            Some(WindowKey::PreventScale(Axis::Y))
        );
        assert_eq!(
            window_key(false, true, false, K::F8),
            Some(WindowKey::Rotation)
        );
        assert_eq!(
            window_key(false, false, false, K::Num6),
            Some(WindowKey::Layout(6))
        );
        // com Ctrl/Alt são atalhos globais (ex.: Ctrl+C copiar, Alt+C aninhar)
        assert_eq!(window_key(true, false, false, K::C), None);
        assert_eq!(window_key(false, false, true, K::C), None);
        assert_eq!(window_key(false, false, false, K::Num7), None);
    }

    #[test]
    fn aplicar_tecla_alterna_a_opcao_e_ignora_layout_inexistente() {
        let mut st = TrackMotionUi::default();
        let before = st.lock_aspect;
        apply_window_key(&mut st, WindowKey::LockAspect);
        assert_eq!(st.lock_aspect, !before);
        apply_window_key(&mut st, WindowKey::Layout(2));
        assert_eq!(st.layout, Layout3d::Perspective);
        apply_window_key(&mut st, WindowKey::Layout(6));
        assert_eq!(st.layout, Layout3d::Perspective);
        apply_window_key(&mut st, WindowKey::Layout(0));
        assert_eq!(st.layout, Layout3d::Perspective);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const F: Vec2 = Vec2::new(1920.0, 1080.0);

    #[test]
    fn conversao_ida_e_volta_e_valores_hostis() {
        let k = MotionKey {
            x: 0.1,
            y: -0.2,
            scale_x: 0.5,
            scale_y: 0.25,
            rot: 30.0,
            z: 0.5,
            ..MotionKey::default()
        };
        let g = motion_to_geom(&k, F);
        assert!((g.center - vec2(960.0 + 192.0, 540.0 - 216.0)).length() < 0.01);
        assert!((g.half - vec2(480.0, 135.0)).length() < 0.01);
        let back = geom_to_motion(&g, F, &k);
        assert!((back.x - k.x).abs() < 1e-5 && (back.scale_y - k.scale_y).abs() < 1e-5);
        assert_eq!(back.z, 0.5, "editar em 2D não zera o Z");
        let hostil = MotionKey {
            x: f32::NAN,
            scale_x: f32::INFINITY,
            rot: f32::NAN,
            ..MotionKey::default()
        };
        let g = motion_to_geom(&hostil, F);
        let back = geom_to_motion(&g, F, &hostil);
        assert!(back.x.is_finite() && back.scale_x.is_finite() && back.rot.is_finite());
    }

    #[test]
    fn travas_de_movimento_na_camera_e_no_objeto() {
        let g = motion_to_geom(&MotionKey::default(), F);
        let st = TrackMotionUi {
            prevent_move_y: true,
            ..TrackMotionUi::default()
        };
        let m = constrained_move(&g, vec2(10.0, 50.0), &st);
        assert!((m.center - vec2(970.0, 540.0)).length() < 0.01);
        // girado 90° no espaço do objeto, travar Y do objeto trava X da tela
        let mut gr = g;
        gr.rot_deg = 90.0;
        let st = TrackMotionUi {
            prevent_move_y: true,
            object_space: true,
            ..TrackMotionUi::default()
        };
        let m = constrained_move(&gr, vec2(10.0, 50.0), &st);
        assert!(
            (m.center - vec2(960.0, 590.0)).length() < 0.01,
            "{:?}",
            m.center
        );
        let m = constrained_move(&g, vec2(f32::NAN, 1.0), &TrackMotionUi::default());
        assert!(m.center.x.is_finite());
    }

    #[test]
    fn travas_de_escala_e_encaixe_da_rotacao() {
        let g = motion_to_geom(&MotionKey::default(), F);
        let st = TrackMotionUi {
            prevent_scale_y: true,
            lock_aspect: false,
            ..TrackMotionUi::default()
        };
        let r = constrained_resize(&g, 4, vec2(960.0 + 100.0, 540.0 + 100.0), &st);
        assert!(
            (r.half - vec2(100.0, 540.0)).length() < 0.01,
            "{:?}",
            r.half
        );
        let st = TrackMotionUi {
            snap: true,
            snap_rotation: 15.0,
            ..TrackMotionUi::default()
        };
        // de 0° para ~20°: encaixa em 15°
        let from = g.center + vec2(100.0, 0.0);
        let to = g.center + vec2(100.0, 36.4);
        let r = constrained_rotate(&g, from, to, &st);
        assert!((r.rot_deg - 15.0).abs() < 1e-3, "{}", r.rot_deg);
    }

    fn app_with_track() -> (App, Id) {
        let ctx = egui::Context::default();
        let app = {
            let _prefs = crate::tests::PREFS_LOCK
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            App::with_context(&ctx, None)
        };
        let tid = app
            .project
            .tracks
            .iter()
            .find(|t| t.kind == neoveg_engine::model::TrackKind::Video)
            .map(|t| t.id)
            .expect("trilha de vídeo");
        (app, tid)
    }

    fn motion(app: &App, tid: Id) -> Keyframes<MotionKey> {
        app.project
            .tracks
            .iter()
            .find(|t| t.id == tid)
            .map(|t| t.motion.clone())
            .expect("trilha")
    }

    #[test]
    fn comandos_e_quadros_chave() {
        let (mut app, tid) = app_with_track();
        let tol = Ticks::from_millis_i64(10);
        let len = Ticks::from_secs_i64(10);
        let t2 = Ticks::from_secs_i64(2);
        let k = MotionKey {
            x: 0.25,
            ..MotionKey::default()
        };
        assert!(app.apply_track_motion(tid, false, 0, t2, tol, F, len, Some(k), &[]));
        assert_eq!(
            motion(&app, tid).keys.len(),
            2,
            "editar fora cria quadro-chave"
        );
        app.apply_track_motion(tid, false, 0, t2, tol, F, len, None, &[TmAction::Center]);
        assert_eq!(motion(&app, tid).eval(t2).x, 0.0);
        app.apply_track_motion(
            tid,
            false,
            0,
            t2,
            tol,
            F,
            len,
            None,
            &[TmAction::MoveKey(0, 1, len.saturating_mul(2))],
        );
        assert_eq!(motion(&app, tid).keys[1].t, len);
        assert!(!app.apply_track_motion(
            tid,
            false,
            0,
            t2,
            tol,
            F,
            len,
            None,
            &[TmAction::SetCurve(0, 9, Curve::Hold)]
        ));
        app.apply_track_motion(
            tid,
            false,
            0,
            t2,
            tol,
            F,
            len,
            None,
            &[TmAction::DeleteKey(0, 1)],
        );
        assert_eq!(motion(&app, tid).keys.len(), 1);
        assert!(!app.apply_track_motion(
            tid,
            false,
            0,
            t2,
            tol,
            F,
            len,
            None,
            &[TmAction::DeleteKey(0, 0)]
        ));
        app.apply_track_motion(
            tid,
            false,
            0,
            t2,
            tol,
            F,
            len,
            None,
            &[TmAction::Blend(BlendMode::Multiply)],
        );
        let blend = app
            .project
            .tracks
            .iter()
            .find(|t| t.id == tid)
            .map(|t| t.blend);
        assert_eq!(blend, Some(BlendMode::Multiply));
        assert!(!app.apply_track_motion(
            tid,
            false,
            0,
            t2,
            tol,
            F,
            len,
            None,
            &[TmAction::Nav(KfNav::Next)]
        ));
    }

    #[test]
    fn vistas_3d_e_arrasto_nos_eixos_de_cada_vista() {
        let p = [10.0, 20.0, 30.0];
        assert_eq!(view_point(View3d::Front, p, F), Some(vec2(10.0, 20.0)));
        assert_eq!(view_point(View3d::Top, p, F), Some(vec2(10.0, -30.0)));
        assert_eq!(view_point(View3d::Left, p, F), Some(vec2(-30.0, 20.0)));
        assert!(view_point(View3d::Perspective, [0.0, 0.0, 1e9], F).is_none());
        let st = TrackMotionUi::default();
        // na vista de cima, arrastar para cima aproxima (Z positivo)
        let (dx, dy, dz) = view_drag(View3d::Top, vec2(0.0, -192.0), F, &st);
        assert_eq!((dx, dy), (0.0, 0.0));
        assert!((dz - 0.1).abs() < 1e-6);
        let st = TrackMotionUi {
            prevent_move_z: true,
            ..TrackMotionUi::default()
        };
        assert_eq!(
            view_drag(View3d::Left, vec2(50.0, 0.0), F, &st),
            (0.0, 0.0, 0.0)
        );
        let (dx, _, _) = view_drag(View3d::Front, vec2(f32::NAN, 0.0), F, &st);
        assert_eq!(dx, 0.0);
    }

    #[test]
    fn ligar_e_desligar_o_3d() {
        let (mut app, tid) = app_with_track();
        let tol = Ticks::from_millis_i64(10);
        let len = Ticks::from_secs_i64(10);
        let track = |app: &App| {
            app.project
                .tracks
                .iter()
                .find(|t| t.id == tid)
                .cloned()
                .unwrap()
        };
        app.apply_track_motion(
            tid,
            false,
            0,
            Ticks::ZERO,
            tol,
            F,
            len,
            None,
            &[TmAction::Blend(BlendMode::Multiply)],
        );
        assert!(app.apply_track_motion(
            tid,
            false,
            0,
            Ticks::ZERO,
            tol,
            F,
            len,
            None,
            &[TmAction::Mode3d(true)]
        ));
        let t = track(&app);
        assert!(t.motion_3d);
        assert_eq!(t.blend, BlendMode::SourceAlpha, "3D usa alfa da origem");
        assert!(!app.apply_track_motion(
            tid,
            false,
            0,
            Ticks::ZERO,
            tol,
            F,
            len,
            None,
            &[TmAction::Mode3d(true)]
        ));
        app.apply_track_motion(
            tid,
            false,
            0,
            Ticks::ZERO,
            tol,
            F,
            len,
            None,
            &[TmAction::Blend(BlendMode::Screen)],
        );
        let t = track(&app);
        assert!(!t.motion_3d && t.blend == BlendMode::Screen);
    }
}
