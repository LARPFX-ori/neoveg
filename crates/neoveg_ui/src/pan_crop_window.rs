//! Janela *Pan/Crop do evento* com a disposição do VEGAS Pro (manual do
//! VEGAS Pro 21, pp. 250–251; `docs_obsidian/ui_references/
//! referencia_visual_janelas.md`): predefinição no alto, barra de
//! ferramentas vertical, grade de propriedades, área de trabalho clara com o
//! retângulo de enquadramento e a linha do tempo de quadros-chave embaixo.

use crate::icons::{self, Icon};
use crate::kf_window::{self, KfNav, KfRow, Workspace};
use crate::pan_crop_geom::{self, BoxGeom, Hit, MoveMode};
use crate::App;
use eframe::egui::{self, pos2, vec2, Color32, CursorIcon, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use neoveg_common::curve::{Curve, Keyframe, Keyframes};
use neoveg_common::i18n::{tr, tr_args, Language};
use neoveg_common::time::Ticks;
use neoveg_engine::model::PanCropKey;

/// Ferramenta da barra vertical.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum PcTool {
    #[default]
    Normal,
    Zoom,
}

/// Ferramenta ativa quando a linha *Máscara* está selecionada.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum MaskTool {
    #[default]
    Normal,
    AddAnchor,
    DelAnchor,
    SplitTangent,
}

/// Arrasto em andamento no modo de máscara Bézier.
#[derive(Clone, Debug, PartialEq)]
enum MaskDrag {
    Vertex(usize),
    HandleIn(usize),
    HandleOut(usize),
    /// Arrasto imediatamente após inserir nova âncora: puxa tangentes simétricas.
    NewAnchorTangents(usize),
    Whole {
        start_ptr: Vec2,
        start_path: neoveg_engine::mask::MaskPath,
    },
}

/// Arrasto em andamento na área de trabalho.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PcDrag {
    hit: Hit,
    start: BoxGeom,
    start_ptr: Vec2,
}

/// Estado da janela (não vai para o projeto).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PanCropUi {
    pub show_props: bool,
    pub tool: PcTool,
    pub mask_tool: MaskTool,
    pub snap: bool,
    pub lock_aspect: bool,
    pub about_center: bool,
    pub move_mode: MoveMode,
    pub sync_cursor: bool,
    /// Cursor próprio da linha de quadros-chave quando não sincronizado.
    pub local: Ticks,
    pub ws: Workspace,
    pub preset: usize,
    /// Linha ativa na trilha de quadros-chave: `false` = Posição, `true` = Máscara.
    pub mask_mode: bool,
    /// Modo compacto da janela (arranjo sem linha de predefinições/título extra).
    pub compact_header: bool,
    /// Nome digitado para salvar nova predefinição própria.
    pub preset_name_draft: String,
    /// Vértice Bézier selecionado para exibir alças tangentes e atalho Delete.
    pub selected_mask_vertex: Option<usize>,
    /// Arrasto ativo na máscara Bézier.
    drag_mask: Option<MaskDrag>,
    /// Posição e vértice sob o ponteiro ao abrir o menu de contexto da máscara.
    mask_menu_vertex: Option<usize>,
    mask_menu_pos: Option<[f32; 2]>,
    drag: Option<PcDrag>,
}

impl Default for PanCropUi {
    fn default() -> Self {
        PanCropUi {
            show_props: true,
            tool: PcTool::Normal,
            mask_tool: MaskTool::Normal,
            snap: false,
            lock_aspect: true,
            about_center: true,
            move_mode: MoveMode::Free,
            sync_cursor: true,
            local: Ticks::ZERO,
            ws: Workspace::default(),
            preset: 0,
            mask_mode: false,
            compact_header: false,
            preset_name_draft: String::new(),
            selected_mask_vertex: None,
            drag_mask: None,
            mask_menu_vertex: None,
            mask_menu_pos: None,
            drag: None,
        }
    }
}

/// Predefinições embutidas da lista do alto.
const PRESETS: [&str; 4] = [
    "(Padrão)",
    "Preencher o quadro",
    "Ken Burns: aproximar",
    "Ken Burns: afastar",
];

/// Comandos que mexem no evento, aplicados depois de desenhar.
#[derive(Clone, Debug, PartialEq)]
enum PcAction {
    Restore,
    Center,
    FlipH,
    FlipV,
    MatchOutput,
    MatchSource,
    Preset(usize),
    ApplyUserPreset(usize),
    SaveUserPreset(String),
    DeleteUserPreset(usize),
    OpenEventFx,
    Nav(KfNav),
    MoveKey(usize, Ticks),
    DeleteKey(usize),
    SetCurve(usize, Curve),
    MaintainAspect(bool),
    StretchToFill(bool),
    ToggleMaskEnabled(bool),
    UpdateMask(neoveg_engine::mask::BezierMaskData),
    AddMaskKey,
    DeleteMaskKey(usize),
    DeleteMaskKeyAtCursor,
    MoveMaskKey(usize, Ticks),
    SetMaskCurve(Option<usize>, Curve),
    SetMaskSmoothness(f32),
}

/// Contorno da letra "F" no espaço do retângulo (−1..1 nos dois eixos),
/// que mostra a orientação e o espelhamento da imagem.
const F_SHAPE: [(f32, f32); 10] = [
    (-0.30, -0.45),
    (0.30, -0.45),
    (0.30, -0.33),
    (-0.18, -0.33),
    (-0.18, -0.06),
    (0.15, -0.06),
    (0.15, 0.06),
    (-0.18, 0.06),
    (-0.18, 0.45),
    (-0.30, 0.45),
];

fn dashed_closed(painter: &egui::Painter, pts: &[Pos2], stroke: Stroke) {
    let mut v = pts.to_vec();
    if let Some(first) = pts.first() {
        v.push(*first);
    }
    painter.extend(Shape::dashed_line(&v, stroke, 4.0, 3.0));
}

impl App {
    pub(crate) fn pan_crop_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.dlg.pan_crop else { return };
        let Some(ev) = self.project.event(id).cloned() else {
            self.dlg.pan_crop = None;
            return;
        };
        let Some(m) = self.project.media(ev.media_id).cloned() else {
            return;
        };
        let s = self.project.settings.clone();
        let frame = vec2(s.width.max(1) as f32, s.height.max(1) as f32);
        let src_size = if m.width > 0 && m.height > 0 {
            vec2(m.width as f32, m.height as f32)
        } else {
            frame
        };
        let tol = s.frame_dur().div_i64(2);
        let mut st = self.dlg.pc.clone();
        let timeline_local = (self.cursor - ev.start).clamp_safe(Ticks::ZERO, ev.length);
        if st.sync_cursor {
            st.local = timeline_local;
        }
        let local = st.local.clamp_safe(Ticks::ZERO, ev.length);
        self.load_pan_crop_texture(ctx, id, &ev, &m, local);
        let lang = self.lang();
        let cur = neoveg_engine::model::eval_pan_crop_smooth(&ev.pan_crop, local);
        let user_presets = self.prefs.pan_crop_presets.clone();
        let mut open = true;
        let mut new_key: Option<PanCropKey> = None;
        let mut actions: Vec<PcAction> = Vec::new();
        let mut seek: Option<Ticks> = None;
        let theme = self.theme.clone();
        let key_idx = ev
            .pan_crop
            .keys
            .iter()
            .rposition(|k| k.t <= local + tol)
            .unwrap_or(0);
        crate::dialog_window::DialogWindow::new(tr_args(
            lang,
            "Pan/Crop do evento — {name}",
            &[("name", &ev.name)],
        ))
        .id(egui::Id::new("pan_crop_win"))
        .open(&mut open)
        .default_size([1080.0, 700.0])
        .min_size([640.0, 420.0])
        .resizable(true)
        .show(ctx, |ui| {
            // Cabeçalho com cadeia em pílulas `o—[Pan/Crop]—[FX]—o` e 3 arranjos de janela
            ui.horizontal(|ui| {
                icons::paint_inline(ui, Icon::PanCrop, 14.0, &theme);
                ui.label(egui::RichText::new("○—").weak());
                let _ = ui.selectable_label(true, tr(lang, "Pan/Crop"));
                ui.label(egui::RichText::new("—").weak());
                let fx_lbl = if ev.fx.is_empty() {
                    tr(lang, "FX").to_string()
                } else {
                    format!("FX ({})", ev.fx.len())
                };
                if ui
                    .selectable_label(false, fx_lbl)
                    .on_hover_text(tr(lang, "Abrir cadeia de efeitos do evento"))
                    .clicked()
                {
                    actions.push(PcAction::OpenEventFx);
                }
                ui.label(egui::RichText::new("—○").weak());
                ui.separator();
                ui.label(&ev.name);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .selectable_label(st.compact_header, "▪")
                        .on_hover_text(tr(lang, "Arranjo compacto"))
                        .clicked()
                    {
                        st.compact_header = !st.compact_header;
                    }
                    if ui
                        .selectable_label(!st.show_props && !st.compact_header, "□")
                        .on_hover_text(tr(lang, "Somente área de trabalho"))
                        .clicked()
                    {
                        st.show_props = false;
                        st.compact_header = false;
                    }
                    if ui
                        .selectable_label(st.show_props && !st.compact_header, "◧")
                        .on_hover_text(tr(lang, "Propriedades e área de trabalho"))
                        .clicked()
                    {
                        st.show_props = true;
                        st.compact_header = false;
                    }
                });
            });
            if !st.compact_header {
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "Predefinição:"));
                    let sel_text = if st.preset < PRESETS.len() {
                        tr(lang, PRESETS[st.preset]).to_string()
                    } else {
                        user_presets
                            .get(st.preset - PRESETS.len())
                            .map(|p| p.name.clone())
                            .unwrap_or_else(|| tr(lang, PRESETS[0]).to_string())
                    };
                    egui::ComboBox::from_id_salt("pc_preset")
                        .width((ui.available_width() - 250.0).max(140.0))
                        .selected_text(sel_text)
                        .show_ui(ui, |ui| {
                            for (i, p) in PRESETS.iter().enumerate() {
                                if ui.selectable_label(st.preset == i, tr(lang, p)).clicked() {
                                    st.preset = i;
                                    actions.push(PcAction::Preset(i));
                                }
                            }
                            if !user_presets.is_empty() {
                                ui.separator();
                                for (ui_i, up) in user_presets.iter().enumerate() {
                                    let idx = PRESETS.len() + ui_i;
                                    if ui.selectable_label(st.preset == idx, &up.name).clicked() {
                                        st.preset = idx;
                                        actions.push(PcAction::ApplyUserPreset(ui_i));
                                    }
                                }
                            }
                        });
                    ui.add(
                        egui::TextEdit::singleline(&mut st.preset_name_draft)
                            .desired_width(120.0)
                            .hint_text(tr(lang, "Nome da predefinição")),
                    );
                    if ui
                        .button(tr(lang, "Salvar"))
                        .on_hover_text(tr(lang, "Salvar predefinição própria de Pan/Crop"))
                        .clicked()
                    {
                        let n = st.preset_name_draft.trim().to_string();
                        if !n.is_empty() {
                            actions.push(PcAction::SaveUserPreset(n));
                            st.preset_name_draft.clear();
                        }
                    }
                    if st.preset >= PRESETS.len() {
                        let ui_i = st.preset - PRESETS.len();
                        if ui
                            .button("×")
                            .on_hover_text(tr(lang, "Excluir predefinição selecionada"))
                            .clicked()
                        {
                            actions.push(PcAction::DeleteUserPreset(ui_i));
                            st.preset = 0;
                        }
                    }
                });
            }
            ui.separator();
            let strip_h = 156.0;
            let top_h = (ui.available_height() - strip_h).max(200.0);
            let top_w = ui.available_width();
            ui.allocate_ui(vec2(top_w, top_h), |ui| {
                ui.horizontal_top(|ui| {
                    self.pc_toolbar(ui, &mut st, lang);
                    if st.show_props {
                        // coluna das propriedades: layout vertical explícito; herdando o
                        // horizontal do pai, as seções saíam lado a lado e se sobrepunham
                        ui.allocate_ui_with_layout(
                            vec2(256.0, top_h),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(256.0);
                                egui::ScrollArea::vertical()
                                    .id_salt("pc_props")
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        if let Some(k) = self.pc_properties(
                                            ui,
                                            &ev,
                                            &cur,
                                            key_idx,
                                            local,
                                            tol,
                                            frame,
                                            &mut st,
                                            &mut actions,
                                            lang,
                                        ) {
                                            new_key = Some(k);
                                        }
                                    });
                            },
                        );
                    }
                    let canvas = vec2(ui.available_width(), top_h);
                    if let Some(k) = self.pc_workspace(
                        ui,
                        canvas,
                        &ev,
                        &cur,
                        frame,
                        src_size,
                        local,
                        tol,
                        &mut st,
                        &mut actions,
                        lang,
                    ) {
                        new_key = Some(k);
                    }
                });
            });
            ui.add_space(2.0);
            let mask_keys: Vec<(Ticks, Curve)> = ev
                .pan_crop_mask
                .as_deref()
                .map(|m| m.keyframes.iter().map(|k| (k.time, k.curve)).collect())
                .unwrap_or_default();
            let rows = [
                KfRow {
                    label: tr(lang, "Posição"),
                    keys: ev.pan_crop.keys.iter().map(|k| (k.t, k.curve)).collect(),
                    checked: None,
                    selected: !st.mask_mode,
                },
                KfRow {
                    label: tr(lang, "Máscara"),
                    keys: mask_keys,
                    checked: Some(ev.pan_crop_mask_enabled),
                    selected: st.mask_mode,
                },
            ];
            let mut sync = st.sync_cursor;
            let fmt = |t: Ticks| self.fmt_time(t);
            let out = kf_window::keyframe_strip(
                ui,
                egui::Id::new("pc_strip"),
                &rows,
                ev.length,
                local,
                &mut sync,
                &fmt,
                &theme,
                lang,
            );
            if let Some(ri) = out.select_row {
                st.mask_mode = ri == 1;
            }
            if out.toggle_row == Some(1) {
                actions.push(PcAction::ToggleMaskEnabled(!ev.pan_crop_mask_enabled));
                st.mask_mode = true;
            }
            if sync != st.sync_cursor {
                st.sync_cursor = sync;
                if sync {
                    seek = Some(local);
                }
            }
            if let Some(t) = out.seek {
                seek = Some(t);
            }
            if let Some(n) = out.nav {
                if st.mask_mode {
                    match n {
                        KfNav::Add => actions.push(PcAction::AddMaskKey),
                        KfNav::Delete => actions.push(PcAction::DeleteMaskKeyAtCursor),
                        other => actions.push(PcAction::Nav(other)),
                    }
                } else {
                    actions.push(PcAction::Nav(n));
                }
            }
            if let Some((ri, ki, t)) = out.move_key {
                if ri == 1 {
                    actions.push(PcAction::MoveMaskKey(ki, t));
                } else {
                    actions.push(PcAction::MoveKey(ki, t));
                }
            }
            if let Some((ri, ki, c)) = out.set_curve {
                if ri == 1 {
                    actions.push(PcAction::SetMaskCurve(Some(ki), c));
                } else {
                    actions.push(PcAction::SetCurve(ki, c));
                }
            }
            if let Some((ri, ki)) = out.delete_key {
                if ri == 1 {
                    actions.push(PcAction::DeleteMaskKey(ki));
                } else {
                    actions.push(PcAction::DeleteKey(ki));
                }
            }
        });

        // tempo dos botões de navegação (segue a linha ativa: Posição ou Máscara)
        let times: Vec<Ticks> = if st.mask_mode {
            ev.pan_crop_mask
                .as_deref()
                .map(|m| m.keyframes.iter().map(|k| k.time).collect())
                .unwrap_or_default()
        } else {
            ev.pan_crop.keys.iter().map(|k| k.t).collect()
        };
        for a in &actions {
            if let PcAction::Nav(n) = a {
                if let Some(t) = kf_window::nav_target(&times, local, *n) {
                    seek = Some(t);
                }
            }
        }
        if let Some(t) = seek {
            let q = s.fps.quantize(ev.start + t);
            let t = (q - ev.start).clamp_safe(Ticks::ZERO, ev.length);
            st.local = t;
            if st.sync_cursor {
                self.set_cursor(ev.start + t);
            }
        }
        let changed = self.apply_pan_crop(id, &ev, local, tol, frame, src_size, new_key, &actions);
        self.dlg.pc = st;
        if changed {
            self.dialog_change();
        }
        if !open {
            self.dlg.pan_crop = None;
            self.dlg.pc_tex = None;
            self.dlg.pc.drag = None;
            self.dlg.pc.drag_mask = None;
        }
    }

    /// Textura da imagem de origem no tempo local, refeita quando o quadro muda.
    fn load_pan_crop_texture(
        &mut self,
        ctx: &egui::Context,
        id: neoveg_common::curve::Id,
        ev: &neoveg_engine::model::Event,
        m: &neoveg_engine::model::MediaItem,
        local: Ticks,
    ) {
        let s = &self.project.settings;
        let frame_key = (id, s.fps.ticks_to_frame_round(local));
        let stale = self
            .dlg
            .pc_tex
            .as_ref()
            .map(|t| t.1 != frame_key)
            .unwrap_or(true)
            || self.generation.is_multiple_of(97);
        if !stale {
            return;
        }
        let mt = ev
            .media_time(ev.start + local, m.duration)
            .unwrap_or(Ticks::ZERO);
        let (gw, gh) = (s.width / 2, s.height / 2);
        if let Some(img) = self.media.video_frame(m, mt, gw.max(1), gh.max(1)) {
            let max = 960u32;
            let sc = (max as f32 / img.width().max(img.height()).max(1) as f32).min(1.0);
            let small = if sc < 1.0 {
                image::imageops::resize(
                    &*img,
                    ((img.width() as f32 * sc) as u32).max(1),
                    ((img.height() as f32 * sc) as u32).max(1),
                    image::imageops::FilterType::Triangle,
                )
            } else {
                (*img).clone()
            };
            let ci = crate::rgba_to_color_image(&small);
            let tex = ctx.load_texture("pancrop_src", ci, egui::TextureOptions::LINEAR);
            self.dlg.pc_tex = Some((tex, frame_key));
        }
    }

    /// Barra de ferramentas vertical (manual do VEGAS Pro 21, pp. 250–251).
    fn pc_toolbar(&self, ui: &mut egui::Ui, st: &mut PanCropUi, lang: Language) {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let th = &self.theme;
            if icons::icon_button(ui, Icon::GearProps, None, st.show_props, true, th)
                .on_hover_text(tr(lang, "Mostrar propriedades"))
                .clicked()
            {
                st.show_props = !st.show_props;
            }
            if icons::icon_button(
                ui,
                Icon::NormalTool,
                None,
                st.tool == PcTool::Normal,
                true,
                th,
            )
            .on_hover_text(tr(lang, "Ferramenta de edição normal"))
            .clicked()
            {
                st.tool = PcTool::Normal;
            }
            if icons::icon_button(ui, Icon::ZoomGlass, None, st.tool == PcTool::Zoom, true, th)
                .on_hover_text(tr(
                    lang,
                    "Ferramenta de zoom (clique amplia, botão direito reduz)",
                ))
                .clicked()
            {
                st.tool = PcTool::Zoom;
            }
            if icons::icon_button(ui, Icon::MagnetSnap, None, st.snap, true, th)
                .on_hover_text(tr(lang, "Encaixar na grade"))
                .clicked()
            {
                st.snap = !st.snap;
            }
            if icons::icon_button(ui, Icon::LockClosed, None, st.lock_aspect, true, th)
                .on_hover_text(tr(lang, "Travar proporção"))
                .clicked()
            {
                st.lock_aspect = !st.lock_aspect;
            }
            if icons::icon_button(ui, Icon::ScaleAboutCenter, None, st.about_center, true, th)
                .on_hover_text(tr(lang, "Dimensionar em torno do centro"))
                .clicked()
            {
                st.about_center = !st.about_center;
            }
            let (icon, tip, next) = match st.move_mode {
                MoveMode::Free => (Icon::MoveFree, "Mover livremente", MoveMode::XOnly),
                MoveMode::XOnly => (Icon::MoveXOnly, "Mover só em X", MoveMode::YOnly),
                MoveMode::YOnly => (Icon::MoveYOnly, "Mover só em Y", MoveMode::Free),
            };
            if icons::icon_button(ui, icon, None, false, true, th)
                .on_hover_text(tr(lang, tip))
                .clicked()
            {
                st.move_mode = next;
            }
            if st.mask_mode {
                ui.separator();
                if icons::icon_button(
                    ui,
                    Icon::TrackMask,
                    None,
                    st.tool == PcTool::Normal && st.mask_tool == MaskTool::Normal,
                    true,
                    th,
                )
                .on_hover_text(tr(
                    lang,
                    "Máscara: selecionar e mover vértices, tangentes ou a máscara inteira",
                ))
                .clicked()
                {
                    st.tool = PcTool::Normal;
                    st.mask_tool = MaskTool::Normal;
                }
                if icons::icon_button(
                    ui,
                    Icon::InsertMarker,
                    None,
                    st.tool == PcTool::Normal && st.mask_tool == MaskTool::AddAnchor,
                    true,
                    th,
                )
                .on_hover_text(tr(
                    lang,
                    "Máscara: criar âncora (clique na borda ou arraste para curvar)",
                ))
                .clicked()
                {
                    st.tool = PcTool::Normal;
                    st.mask_tool = MaskTool::AddAnchor;
                }
                if icons::icon_button(
                    ui,
                    Icon::KeyframeDelete,
                    None,
                    st.tool == PcTool::Normal && st.mask_tool == MaskTool::DelAnchor,
                    true,
                    th,
                )
                .on_hover_text(tr(lang, "Máscara: excluir âncora"))
                .clicked()
                {
                    st.tool = PcTool::Normal;
                    st.mask_tool = MaskTool::DelAnchor;
                }
                if icons::icon_button(
                    ui,
                    Icon::Envelopes,
                    None,
                    st.tool == PcTool::Normal && st.mask_tool == MaskTool::SplitTangent,
                    true,
                    th,
                )
                .on_hover_text(tr(lang, "Máscara: alternar tangente suave/canto"))
                .clicked()
                {
                    st.tool = PcTool::Normal;
                    st.mask_tool = MaskTool::SplitTangent;
                }
            }
            ui.separator();
            if ui
                .small_button("+")
                .on_hover_text(tr(lang, "Ampliar zoom"))
                .clicked()
            {
                st.ws.zoom_pct =
                    (st.ws.zoom_pct * 1.25).clamp(kf_window::WS_MIN_ZOOM, kf_window::WS_MAX_ZOOM);
            }
            if ui
                .small_button("−")
                .on_hover_text(tr(lang, "Reduzir zoom"))
                .clicked()
            {
                st.ws.zoom_pct =
                    (st.ws.zoom_pct * 0.8).clamp(kf_window::WS_MIN_ZOOM, kf_window::WS_MAX_ZOOM);
            }
        });
    }

    /// Grade de propriedades. Devolve o quadro-chave editado, se houver.
    #[allow(clippy::too_many_arguments)]
    fn pc_properties(
        &self,
        ui: &mut egui::Ui,
        ev: &neoveg_engine::model::Event,
        cur: &PanCropKey,
        key_idx: usize,
        local: Ticks,
        tol: Ticks,
        frame: Vec2,
        st: &mut PanCropUi,
        actions: &mut Vec<PcAction>,
        lang: Language,
    ) -> Option<PanCropKey> {
        let th = &self.theme;
        let g = BoxGeom::from_key(cur, frame);
        let mut out = None;
        let sec = |name: &str| egui::Id::new(("pc_sec", name.to_string()));
        kf_window::prop_section(ui, sec("pos"), tr(lang, "Posição"), true, th, |ui| {
            let (mut w, mut h) = (g.half.x * 2.0, g.half.y * 2.0);
            let (mut x, mut y) = (g.center.x, g.center.y);
            let big = frame.x.max(frame.y) * 20.0;
            let mut ng = g;
            if kf_window::prop_num(ui, tr(lang, "Largura:"), &mut w, 1.0, 1, 2.0..=big) {
                let k = w / (g.half.x * 2.0).max(1e-3);
                ng.half = if st.lock_aspect {
                    g.half * k
                } else {
                    vec2(w * 0.5, g.half.y)
                };
                out = Some(ng.to_key(cur, frame));
            }
            if kf_window::prop_num(ui, tr(lang, "Altura:"), &mut h, 1.0, 1, 2.0..=big) {
                let k = h / (g.half.y * 2.0).max(1e-3);
                ng.half = if st.lock_aspect {
                    g.half * k
                } else {
                    vec2(g.half.x, h * 0.5)
                };
                out = Some(ng.to_key(cur, frame));
            }
            if kf_window::prop_num(ui, tr(lang, "Centro X:"), &mut x, 1.0, 1, -big..=big) {
                ng.center.x = x;
                out = Some(ng.to_key(cur, frame));
            }
            if kf_window::prop_num(ui, tr(lang, "Centro Y:"), &mut y, 1.0, 1, -big..=big) {
                ng.center.y = y;
                out = Some(ng.to_key(cur, frame));
            }
        });
        kf_window::prop_section(ui, sec("rot"), tr(lang, "Rotação"), true, th, |ui| {
            let mut a = g.rot_deg;
            if kf_window::prop_num(ui, tr(lang, "Ângulo:"), &mut a, 0.5, 1, -360.0..=360.0) {
                out = Some(PanCropKey {
                    rot: a,
                    ..out.clone().unwrap_or_else(|| cur.clone())
                });
            }
            // a rotação é sempre em torno do centro do retângulo
            kf_window::prop_row(ui, tr(lang, "Centro X:"), |ui| {
                ui.add_enabled(false, egui::Label::new(format!("{:.1}", g.center.x)));
            });
            kf_window::prop_row(ui, tr(lang, "Centro Y:"), |ui| {
                ui.add_enabled(false, egui::Label::new(format!("{:.1}", g.center.y)));
            });
        });
        kf_window::prop_section(
            ui,
            sec("interp"),
            tr(lang, "Interpolação de quadros-chave"),
            true,
            th,
            |ui| {
                let target_lbl = if st.mask_mode {
                    tr(lang, "Máscara")
                } else {
                    tr(lang, "Posição")
                };
                kf_window::prop_row(ui, tr(lang, "Linha ativa:"), |ui| {
                    ui.label(egui::RichText::new(target_lbl).strong().small());
                });
                if st.mask_mode {
                    let md = ev.pan_crop_mask.as_deref();
                    let m_idx = md.and_then(|m| m.active_key_index(local, tol));
                    let current = m_idx
                        .and_then(|i| md.and_then(|m| m.keyframes.get(i)))
                        .map(|k| k.curve);
                    let mut c = current.unwrap_or(Curve::Smooth);
                    kf_window::prop_row(ui, tr(lang, "Tipo:"), |ui| {
                        egui::ComboBox::from_id_salt("pc_mask_curve")
                            .selected_text(c.name_in(lang))
                            .show_ui(ui, |ui| {
                                for k in Curve::ALL.iter().take(6) {
                                    ui.selectable_value(&mut c, *k, k.name_in(lang));
                                }
                            });
                        if Some(c) != current {
                            actions.push(PcAction::SetMaskCurve(m_idx, c));
                        }
                    });
                    let mut sm = m_idx
                        .and_then(|i| md.and_then(|m| m.keyframes.get(i)))
                        .map(|k| k.smoothness_safe())
                        .unwrap_or(0.0);
                    if kf_window::prop_num(ui, tr(lang, "Suavidade:"), &mut sm, 1.0, 0, 0.0..=100.0)
                    {
                        actions.push(PcAction::SetMaskSmoothness(sm));
                    }
                    kf_window::paint_curve_preview(ui, c, sm, th);
                } else {
                    let current = ev.pan_crop.keys.get(key_idx).map(|k| k.curve);
                    let mut c = current.unwrap_or_default();
                    kf_window::prop_row(ui, tr(lang, "Tipo:"), |ui| {
                        egui::ComboBox::from_id_salt("pc_curve")
                            .selected_text(c.name_in(lang))
                            .show_ui(ui, |ui| {
                                for k in Curve::ALL.iter().take(6) {
                                    ui.selectable_value(&mut c, *k, k.name_in(lang));
                                }
                            });
                        if current.is_some() && Some(c) != current {
                            actions.push(PcAction::SetCurve(key_idx, c));
                        }
                    });
                    let mut sm = cur.smoothness_safe();
                    if kf_window::prop_num(ui, tr(lang, "Suavidade:"), &mut sm, 1.0, 0, 0.0..=100.0)
                    {
                        out = Some(PanCropKey {
                            smoothness: sm,
                            ..out.clone().unwrap_or_else(|| cur.clone())
                        });
                    }
                    kf_window::paint_curve_preview(ui, c, sm, th);
                }
            },
        );
        kf_window::prop_section(ui, sec("src"), tr(lang, "Origem"), false, th, |ui| {
            kf_window::prop_row(ui, tr(lang, "Manter proporção:"), |ui| {
                let mut v = ev.maintain_aspect;
                if ui.checkbox(&mut v, "").changed() {
                    actions.push(PcAction::MaintainAspect(v));
                }
            });
            kf_window::prop_row(ui, tr(lang, "Esticar para preencher:"), |ui| {
                let mut v = ev.pan_crop_stretch_to_fill;
                if ui.checkbox(&mut v, "").changed() {
                    actions.push(PcAction::StretchToFill(v));
                }
            });
        });
        if st.mask_mode || ev.pan_crop_mask_enabled {
            kf_window::prop_section(
                ui,
                sec("mask"),
                tr(lang, "Máscara Bézier"),
                true,
                th,
                |ui| {
                    let mut mask_on = ev.pan_crop_mask_enabled;
                    kf_window::prop_row(ui, tr(lang, "Ativar máscara:"), |ui| {
                        if ui.checkbox(&mut mask_on, "").changed() {
                            actions.push(PcAction::ToggleMaskEnabled(mask_on));
                        }
                    });
                    let mut md = ev.pan_crop_mask.as_deref().cloned().unwrap_or_default();
                    let (mut cur_path, mut cur_feather, mut cur_opacity, mut cur_expansion) =
                        md.evaluate_at(local);
                    let mut m_changed = false;
                    let mut path_or_vals_changed = false;
                    kf_window::prop_row(ui, tr(lang, "Modo:"), |ui| {
                        let lbl = if md.invert {
                            tr(lang, "Negativa (recorte)")
                        } else {
                            tr(lang, "Positiva (manter interior)")
                        };
                        egui::ComboBox::from_id_salt("pc_mask_inv")
                            .selected_text(lbl)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(
                                        !md.invert,
                                        tr(lang, "Positiva (manter interior)"),
                                    )
                                    .clicked()
                                {
                                    md.invert = false;
                                    m_changed = true;
                                }
                                if ui
                                    .selectable_label(md.invert, tr(lang, "Negativa (recorte)"))
                                    .clicked()
                                {
                                    md.invert = true;
                                    m_changed = true;
                                }
                            });
                    });
                    if kf_window::prop_num(
                        ui,
                        tr(lang, "Suavização:"),
                        &mut cur_feather,
                        0.5,
                        1,
                        0.0..=100.0,
                    ) {
                        path_or_vals_changed = true;
                    }
                    let mut op_pct = cur_opacity * 100.0;
                    if kf_window::prop_num(
                        ui,
                        tr(lang, "Opacidade (%):"),
                        &mut op_pct,
                        1.0,
                        0,
                        0.0..=100.0,
                    ) {
                        cur_opacity = (op_pct * 0.01).clamp(0.0, 1.0);
                        path_or_vals_changed = true;
                    }
                    if kf_window::prop_num(
                        ui,
                        tr(lang, "Expansão:"),
                        &mut cur_expansion,
                        0.5,
                        1,
                        -100.0..=100.0,
                    ) {
                        path_or_vals_changed = true;
                    }
                    kf_window::prop_row(ui, tr(lang, "Fechar caminho:"), |ui| {
                        if ui.checkbox(&mut cur_path.closed, "").changed() {
                            path_or_vals_changed = true;
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        if ui.small_button(tr(lang, "Retângulo")).clicked() {
                            cur_path = neoveg_engine::mask::MaskPath::rect([0.2, 0.2], [0.8, 0.8]);
                            st.selected_mask_vertex = None;
                            path_or_vals_changed = true;
                        }
                        if ui.small_button(tr(lang, "Elipse")).clicked() {
                            cur_path =
                                neoveg_engine::mask::MaskPath::ellipse([0.5, 0.5], [0.35, 0.30]);
                            st.selected_mask_vertex = None;
                            path_or_vals_changed = true;
                        }
                        if ui.small_button(tr(lang, "Arredondado")).clicked() {
                            cur_path = neoveg_engine::mask::MaskPath::rounded_rect(
                                [0.2, 0.2],
                                [0.8, 0.8],
                                0.08,
                            );
                            st.selected_mask_vertex = None;
                            path_or_vals_changed = true;
                        }
                        if ui.small_button(tr(lang, "Limpar (desenhar)")).clicked() {
                            cur_path = neoveg_engine::mask::MaskPath::freeform(Vec::new(), true);
                            st.selected_mask_vertex = None;
                            st.mask_mode = true;
                            st.mask_tool = MaskTool::AddAnchor;
                            path_or_vals_changed = true;
                        }
                    });
                    if path_or_vals_changed {
                        md.set_at(
                            local,
                            tol,
                            cur_path,
                            cur_feather,
                            cur_opacity,
                            cur_expansion,
                        );
                        m_changed = true;
                    }
                    if m_changed {
                        actions.push(PcAction::UpdateMask(md));
                    }
                },
            );
        }
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
                    1,
                    kf_window::WS_MIN_ZOOM..=kf_window::WS_MAX_ZOOM,
                ) {
                    st.ws.zoom_pct = z;
                }
                kf_window::prop_num(
                    ui,
                    tr(lang, "Deslocamento X:"),
                    &mut st.ws.offset.x,
                    1.0,
                    1,
                    -1e5..=1e5,
                );
                kf_window::prop_num(
                    ui,
                    tr(lang, "Deslocamento Y:"),
                    &mut st.ws.offset.y,
                    1.0,
                    1,
                    -1e5..=1e5,
                );
                kf_window::prop_num(
                    ui,
                    tr(lang, "Espaçamento da grade:"),
                    &mut st.ws.grid,
                    1.0,
                    0,
                    1.0..=1024.0,
                );
            },
        );
        out
    }

    /// Área de trabalho com a imagem de origem e o retângulo de
    /// enquadramento. Devolve o quadro-chave editado, se houver.
    #[allow(clippy::too_many_arguments)]
    fn pc_workspace(
        &self,
        ui: &mut egui::Ui,
        size: Vec2,
        ev: &neoveg_engine::model::Event,
        cur: &PanCropKey,
        frame: Vec2,
        src_size: Vec2,
        local: Ticks,
        tol: Ticks,
        st: &mut PanCropUi,
        actions: &mut Vec<PcAction>,
        lang: Language,
    ) -> Option<PanCropKey> {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        if !st.ws.fitted {
            st.ws.fit(rect, frame);
        }
        let ws = st.ws;
        st.ws.paint_background(&painter, rect, frame, &self.theme);
        // imagem de origem no retângulo de ajuste do quadro
        let fit = if ev.maintain_aspect {
            let k = (frame.x / src_size.x).min(frame.y / src_size.y);
            src_size * k
        } else {
            frame
        };
        let fit_min = (frame - fit) * 0.5;
        if let Some((tex, _)) = &self.dlg.pc_tex {
            let r = Rect::from_min_max(
                ws.to_screen(rect, frame, fit_min),
                ws.to_screen(rect, frame, fit_min + fit),
            );
            painter.image(
                tex.id(),
                r,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        // retângulo de enquadramento
        let g = BoxGeom::from_key(cur, frame);
        let sc = |p: Vec2| ws.to_screen(rect, frame, p);
        let ink = if self.theme.high_contrast {
            self.theme.text_primary
        } else {
            Color32::BLACK
        };
        let stroke_w = if self.theme.high_contrast {
            1.5_f32
        } else {
            1.0_f32
        };
        let thin = Stroke::new(stroke_w, ink);
        let ptr_screen = resp.hover_pos().or_else(|| resp.interact_pointer_pos());
        let ptr = ptr_screen.map(|p| ws.to_frame(rect, frame, p));
        let tol_px = ws.frame_len(13.0);
        let active_hit = if !st.mask_mode && st.tool == PcTool::Normal {
            st.drag
                .map(|d| d.hit)
                .or_else(|| ptr.map(|p| g.hit(p, tol_px)))
        } else {
            None
        };
        let corners: Vec<Pos2> = g.corners().iter().map(|p| sc(*p)).collect();
        dashed_closed(&painter, &corners, thin);
        // destaca a borda sob o ponteiro para feedback claro ao pegar largura/altura
        if let Some(Hit::Handle(h_idx @ (1 | 3 | 5 | 7))) = active_hit {
            let (a, b) = match h_idx {
                1 => (corners[0], corners[1]),
                3 => (corners[1], corners[2]),
                5 => (corners[2], corners[3]),
                _ => (corners[3], corners[0]),
            };
            painter.line_segment([a, b], Stroke::new(2.5_f32, self.theme.accent));
        }
        let r = ws.frame_len(1.0).recip() * g.rotate_radius();
        let ring: Vec<Pos2> = (0..72)
            .map(|i| {
                let a = i as f32 / 72.0 * std::f32::consts::TAU;
                sc(g.center) + vec2(a.cos(), a.sin()) * r
            })
            .collect();
        dashed_closed(&painter, &ring, thin);
        let (fx, fy) = (
            if cur.flip_h { -1.0 } else { 1.0 },
            if cur.flip_v { -1.0 } else { 1.0 },
        );
        let f_pts: Vec<Pos2> = F_SHAPE
            .iter()
            .map(|(x, y)| sc(g.to_frame(vec2(x * fx * g.half.x, y * fy * g.half.y))))
            .collect();
        dashed_closed(&painter, &f_pts, thin);
        painter.circle_stroke(sc(g.center), 4.0, thin);
        let handle_fill = if self.theme.high_contrast {
            self.theme.accent
        } else {
            Color32::WHITE
        };
        for i in 0..8 {
            let is_hot = active_hit == Some(Hit::Handle(i));
            let sz = if is_hot { 12.0 } else { 10.0 };
            let h = Rect::from_center_size(sc(g.handle(i)), vec2(sz, sz));
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

        // contorno da máscara Bézier (desenhado quando habilitada ou em modo Máscara)
        if ev.pan_crop_mask_enabled || st.mask_mode {
            let mask_data = ev.pan_crop_mask.as_deref().cloned().unwrap_or_default();
            let (mpath, _, _, _) = mask_data.evaluate_at(local);
            let poly = mpath.flatten(16);
            if poly.len() >= 2 {
                let pts: Vec<Pos2> = poly
                    .iter()
                    .map(|p| sc(vec2(p[0] * frame.x, p[1] * frame.y)))
                    .collect();
                let mcol = if self.theme.high_contrast {
                    self.theme.accent
                } else {
                    Color32::from_rgb(240, 175, 40)
                };
                if mpath.closed {
                    dashed_closed(&painter, &pts, Stroke::new(1.8_f32, mcol));
                } else {
                    painter.extend(Shape::dashed_line(
                        &pts,
                        Stroke::new(1.8_f32, mcol),
                        4.0,
                        3.0,
                    ));
                }
            }
            if st.mask_mode {
                let vcol = Color32::from_rgb(255, 200, 60);
                let tan_col = Color32::from_rgb(70, 195, 255);
                for (vi, v) in mpath.vertices.iter().enumerate() {
                    let vp = sc(vec2(v.point[0] * frame.x, v.point[1] * frame.y));
                    let is_sel = st.selected_mask_vertex == Some(vi);
                    // desenha alças tangentes Bézier quando não nulas ou vértice selecionado
                    for h in [v.handle_in, v.handle_out] {
                        if h[0].abs() > 1e-4 || h[1].abs() > 1e-4 {
                            let hp = sc(vec2(
                                (v.point[0] + h[0]) * frame.x,
                                (v.point[1] + h[1]) * frame.y,
                            ));
                            painter.line_segment([vp, hp], Stroke::new(1.2_f32, tan_col));
                            painter.circle_filled(hp, if is_sel { 4.5 } else { 3.5 }, tan_col);
                            painter.circle_stroke(hp, if is_sel { 4.5 } else { 3.5 }, thin);
                        }
                    }
                    let vsz = if is_sel { 11.0 } else { 9.0 };
                    let vr = Rect::from_center_size(vp, vec2(vsz, vsz));
                    painter.rect_filled(vr, 1.0, if is_sel { self.theme.accent } else { vcol });
                    painter.rect_stroke(vr, 1.0, thin, egui::StrokeKind::Inside);
                }
            }
        }

        // barra flutuante de zoom (sempre visível em Posição e Máscara)
        let zoom_bar_rect = kf_window::workspace_zoom_bar(
            ui,
            ui.id().with("pc_zoom_bar"),
            rect,
            frame,
            &mut st.ws,
            &self.theme,
            lang,
        );
        let over_zoom_bar = ptr_screen.is_some_and(|p| zoom_bar_rect.contains(p));

        // roda = zoom da área; pinça/+/- = zoom; botão do meio = deslocar (em Posição E Máscara)
        kf_window::handle_workspace_wheel_and_zoom(ui, &resp, rect, frame, &mut st.ws);

        if st.tool == PcTool::Zoom && !over_zoom_bar {
            if resp.hovered() {
                let cur = if ui.input(|i| i.modifiers.shift || i.modifiers.alt) {
                    crate::cursors::NeovegCursor::ZoomOut
                } else {
                    crate::cursors::NeovegCursor::ZoomIn
                };
                crate::cursors::render_cursor(
                    ui.ctx(),
                    cur,
                    &self.theme,
                    self.prefs.custom_cursors,
                );
            }
            if let Some(hp) = resp.interact_pointer_pos() {
                let zoom_out = ui.input(|i| i.modifiers.shift || i.modifiers.alt);
                if resp.clicked() {
                    st.ws
                        .zoom_at(rect, frame, hp, if zoom_out { 0.8 } else { 1.25 });
                } else if resp.secondary_clicked() {
                    st.ws.zoom_at(rect, frame, hp, 0.8);
                }
            }
            return None;
        }

        if st.mask_mode {
            let mut md = ev.pan_crop_mask.as_deref().cloned().unwrap_or_default();
            let (mut mpath, feather, opacity, expansion) = md.evaluate_at(local);
            let fw = frame.x.max(1.0);
            let fh = frame.y.max(1.0);
            let v_tol = ws.frame_len(11.5);
            let h_tol = ws.frame_len(9.5);
            let shift = ui.input(|i| i.modifiers.shift);
            let alt = ui.input(|i| i.modifiers.alt);

            // detecta alça tangente sob o ponteiro
            let hit_handle = ptr.and_then(|p| {
                for (i, v) in mpath.vertices.iter().enumerate() {
                    for (is_out, h) in [(false, v.handle_in), (true, v.handle_out)] {
                        if h[0].abs() <= 1e-4 && h[1].abs() <= 1e-4 {
                            continue;
                        }
                        let hp = vec2((v.point[0] + h[0]) * fw, (v.point[1] + h[1]) * fh);
                        if (hp - p).length() <= h_tol {
                            return Some((i, is_out));
                        }
                    }
                }
                None
            });
            // detecta vértice mais próximo sob o ponteiro
            let hit_v = ptr.and_then(|p| {
                mpath
                    .vertices
                    .iter()
                    .enumerate()
                    .filter_map(|(i, v)| {
                        let vp = vec2(v.point[0] * fw, v.point[1] * fh);
                        let d = (vp - p).length();
                        (d <= v_tol).then_some((i, d))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(i, _)| i)
            });
            let ptr_norm = ptr.map(|p| [(p.x / fw).clamp(-1.0, 2.0), (p.y / fh).clamp(-1.0, 2.0)]);
            let inside_mask = ptr_norm.is_some_and(|pn| mpath.contains_point(pn));

            if (resp.hovered() && !over_zoom_bar) || st.drag_mask.is_some() {
                let cur = match (&st.drag_mask, st.mask_tool) {
                    (Some(MaskDrag::Whole { .. }), _) => crate::cursors::NeovegCursor::Move,
                    (Some(_), _) => crate::cursors::NeovegCursor::MaskVertex,
                    (None, MaskTool::AddAnchor) => crate::cursors::NeovegCursor::MaskPenAdd,
                    (None, MaskTool::DelAnchor) => crate::cursors::NeovegCursor::MaskPenDel,
                    (None, MaskTool::SplitTangent) => crate::cursors::NeovegCursor::MaskVertex,
                    (None, MaskTool::Normal) => {
                        if hit_handle.is_some() {
                            crate::cursors::NeovegCursor::MaskVertex
                        } else if hit_v.is_some() && shift {
                            crate::cursors::NeovegCursor::MaskPenDel
                        } else if hit_v.is_some() {
                            crate::cursors::NeovegCursor::MaskVertex
                        } else if inside_mask {
                            crate::cursors::NeovegCursor::Move
                        } else {
                            crate::cursors::NeovegCursor::MaskPenAdd
                        }
                    }
                };
                crate::cursors::render_cursor(
                    ui.ctx(),
                    cur,
                    &self.theme,
                    self.prefs.custom_cursors,
                );
            }

            let mut mask_dirty = false;
            // tecla Delete/Backspace remove vértice selecionado (se > 3 vértices)
            if !ui.ctx().wants_keyboard_input()
                && ui.input(|i| {
                    i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)
                })
            {
                if let Some(vi) = st.selected_mask_vertex {
                    if vi < mpath.vertices.len() && mpath.vertices.len() > 3 {
                        mpath.vertices.remove(vi);
                        st.selected_mask_vertex = None;
                        mask_dirty = true;
                    }
                }
            }

            if !over_zoom_bar {
                if resp.drag_started_by(egui::PointerButton::Primary) {
                    if let (Some(p), Some(pn)) = (ptr, ptr_norm) {
                        match st.mask_tool {
                            MaskTool::Normal => {
                                if let Some((vi, is_out)) = hit_handle {
                                    st.selected_mask_vertex = Some(vi);
                                    st.drag_mask = Some(if is_out {
                                        MaskDrag::HandleOut(vi)
                                    } else {
                                        MaskDrag::HandleIn(vi)
                                    });
                                } else if let Some(vi) = hit_v {
                                    st.selected_mask_vertex = Some(vi);
                                    st.drag_mask = Some(MaskDrag::Vertex(vi));
                                } else if inside_mask {
                                    st.drag_mask = Some(MaskDrag::Whole {
                                        start_ptr: p,
                                        start_path: mpath.clone(),
                                    });
                                }
                            }
                            MaskTool::AddAnchor => {
                                if let Some(vi) = hit_v {
                                    st.selected_mask_vertex = Some(vi);
                                    st.drag_mask = Some(MaskDrag::Vertex(vi));
                                } else {
                                    let idx = mpath.insert_vertex_on_nearest_segment(pn);
                                    st.selected_mask_vertex = Some(idx);
                                    st.drag_mask = Some(MaskDrag::NewAnchorTangents(idx));
                                    mask_dirty = true;
                                }
                            }
                            MaskTool::SplitTangent => {
                                if let Some((vi, is_out)) = hit_handle {
                                    st.selected_mask_vertex = Some(vi);
                                    st.drag_mask = Some(if is_out {
                                        MaskDrag::HandleOut(vi)
                                    } else {
                                        MaskDrag::HandleIn(vi)
                                    });
                                }
                            }
                            MaskTool::DelAnchor => {}
                        }
                    }
                }

                if resp.dragged_by(egui::PointerButton::Primary) {
                    if let (Some(drag), Some(p)) = (st.drag_mask.clone(), ptr) {
                        let p = if st.snap && st.ws.grid.is_finite() && st.ws.grid >= 1.0 {
                            let gs = st.ws.grid;
                            vec2((p.x / gs).round() * gs, (p.y / gs).round() * gs)
                        } else {
                            p
                        };
                        let pn = [(p.x / fw).clamp(-1.0, 2.0), (p.y / fh).clamp(-1.0, 2.0)];
                        match drag {
                            MaskDrag::Vertex(vi) => {
                                if let Some(v) = mpath.vertices.get_mut(vi) {
                                    v.point = pn;
                                    mask_dirty = true;
                                }
                            }
                            MaskDrag::HandleIn(vi) => {
                                if let Some(v) = mpath.vertices.get_mut(vi) {
                                    let dx = (pn[0] - v.point[0]).clamp(-1.5, 1.5);
                                    let dy = (pn[1] - v.point[1]).clamp(-1.5, 1.5);
                                    v.handle_in = [dx, dy];
                                    if !alt && st.mask_tool != MaskTool::SplitTangent {
                                        v.handle_out = [-dx, -dy];
                                    }
                                    mask_dirty = true;
                                }
                            }
                            MaskDrag::HandleOut(vi) => {
                                if let Some(v) = mpath.vertices.get_mut(vi) {
                                    let dx = (pn[0] - v.point[0]).clamp(-1.5, 1.5);
                                    let dy = (pn[1] - v.point[1]).clamp(-1.5, 1.5);
                                    v.handle_out = [dx, dy];
                                    if !alt && st.mask_tool != MaskTool::SplitTangent {
                                        v.handle_in = [-dx, -dy];
                                    }
                                    mask_dirty = true;
                                }
                            }
                            MaskDrag::NewAnchorTangents(vi) => {
                                if let Some(v) = mpath.vertices.get_mut(vi) {
                                    let dx = (pn[0] - v.point[0]).clamp(-1.5, 1.5);
                                    let dy = (pn[1] - v.point[1]).clamp(-1.5, 1.5);
                                    v.handle_out = [dx, dy];
                                    v.handle_in = [-dx, -dy];
                                    mask_dirty = true;
                                }
                            }
                            MaskDrag::Whole {
                                start_ptr,
                                start_path,
                            } => {
                                let mut delta = p - start_ptr;
                                match st.move_mode {
                                    MoveMode::Free => {}
                                    MoveMode::XOnly => delta.y = 0.0,
                                    MoveMode::YOnly => delta.x = 0.0,
                                }
                                let mut moved = start_path;
                                moved.translate(delta.x / fw, delta.y / fh);
                                mpath = moved;
                                mask_dirty = true;
                            }
                        }
                    }
                }
                if resp.drag_stopped() {
                    st.drag_mask = None;
                }

                if resp.double_clicked() {
                    if let Some(vi) = hit_v {
                        mpath.toggle_vertex_tangent(vi);
                        st.selected_mask_vertex = Some(vi);
                        mask_dirty = true;
                    }
                } else if resp.clicked() {
                    match st.mask_tool {
                        MaskTool::DelAnchor => {
                            if let Some(vi) = hit_v.filter(|_| mpath.vertices.len() > 3) {
                                mpath.vertices.remove(vi);
                                st.selected_mask_vertex = None;
                                mask_dirty = true;
                            }
                        }
                        MaskTool::SplitTangent => {
                            if let Some(vi) = hit_v {
                                mpath.toggle_vertex_tangent(vi);
                                st.selected_mask_vertex = Some(vi);
                                mask_dirty = true;
                            }
                        }
                        MaskTool::AddAnchor => {
                            if let Some(pn) = ptr_norm {
                                if hit_v.is_none() {
                                    let idx = mpath.insert_vertex_on_nearest_segment(pn);
                                    st.selected_mask_vertex = Some(idx);
                                    mask_dirty = true;
                                } else {
                                    st.selected_mask_vertex = hit_v;
                                }
                            }
                        }
                        MaskTool::Normal => {
                            if let Some(vi) = hit_v {
                                if shift && mpath.vertices.len() > 3 {
                                    mpath.vertices.remove(vi);
                                    st.selected_mask_vertex = None;
                                    mask_dirty = true;
                                } else {
                                    st.selected_mask_vertex = Some(vi);
                                }
                            } else if !inside_mask {
                                if let Some(pn) = ptr_norm {
                                    let idx = mpath.insert_vertex_on_nearest_segment(pn);
                                    st.selected_mask_vertex = Some(idx);
                                    mask_dirty = true;
                                }
                            }
                        }
                    }
                }
            }

            if resp.secondary_clicked() {
                st.mask_menu_vertex = hit_v;
                st.mask_menu_pos = ptr_norm;
            }
            resp.context_menu(|ui| {
                if let Some(vi) = st.mask_menu_vertex {
                    if ui
                        .button(tr(lang, "Alternar tangente suave/canto"))
                        .clicked()
                    {
                        mpath.toggle_vertex_tangent(vi);
                        st.selected_mask_vertex = Some(vi);
                        mask_dirty = true;
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            mpath.vertices.len() > 3,
                            egui::Button::new(tr(lang, "Excluir âncora")),
                        )
                        .clicked()
                    {
                        mpath.vertices.remove(vi);
                        st.selected_mask_vertex = None;
                        mask_dirty = true;
                        ui.close_menu();
                    }
                    ui.separator();
                } else if let Some(pn) = st.mask_menu_pos {
                    if ui.button(tr(lang, "Adicionar âncora aqui")).clicked() {
                        let idx = mpath.insert_vertex_on_nearest_segment(pn);
                        st.selected_mask_vertex = Some(idx);
                        mask_dirty = true;
                        ui.close_menu();
                    }
                    ui.separator();
                }
                if ui.button(tr(lang, "Retângulo")).clicked() {
                    mpath = neoveg_engine::mask::MaskPath::rect([0.2, 0.2], [0.8, 0.8]);
                    st.selected_mask_vertex = None;
                    mask_dirty = true;
                    ui.close_menu();
                }
                if ui.button(tr(lang, "Elipse")).clicked() {
                    mpath = neoveg_engine::mask::MaskPath::ellipse([0.5, 0.5], [0.35, 0.30]);
                    st.selected_mask_vertex = None;
                    mask_dirty = true;
                    ui.close_menu();
                }
                if ui.button(tr(lang, "Arredondado")).clicked() {
                    mpath =
                        neoveg_engine::mask::MaskPath::rounded_rect([0.2, 0.2], [0.8, 0.8], 0.08);
                    st.selected_mask_vertex = None;
                    mask_dirty = true;
                    ui.close_menu();
                }
                ui.separator();
                if ui
                    .button(tr(lang, "Ajustar a área de trabalho ao quadro"))
                    .clicked()
                {
                    st.ws.fit(rect, frame);
                    ui.close_menu();
                }
            });

            if mask_dirty {
                md.set_at(local, tol, mpath, feather, opacity, expansion);
                actions.push(PcAction::UpdateMask(md));
            }
            return None;
        }

        if let (Some(p), PcTool::Normal) = (ptr, st.tool) {
            let cur = match st.drag.map(|d| d.hit).unwrap_or_else(|| g.hit(p, tol_px)) {
                Hit::Inside => match st.move_mode {
                    MoveMode::Free => crate::cursors::NeovegCursor::Move,
                    MoveMode::XOnly => crate::cursors::NeovegCursor::MoveX,
                    MoveMode::YOnly => crate::cursors::NeovegCursor::MoveY,
                },
                Hit::Handle(1 | 5) => {
                    crate::cursors::NeovegCursor::System(CursorIcon::ResizeVertical)
                }
                Hit::Handle(3 | 7) => {
                    crate::cursors::NeovegCursor::System(CursorIcon::ResizeHorizontal)
                }
                Hit::Handle(0 | 4) => crate::cursors::NeovegCursor::System(CursorIcon::ResizeNwSe),
                Hit::Handle(_) => crate::cursors::NeovegCursor::System(CursorIcon::ResizeNeSw),
                Hit::Rotate => crate::cursors::NeovegCursor::Rotate,
            };
            if (resp.hovered() && !over_zoom_bar) || st.drag.is_some() {
                crate::cursors::render_cursor(
                    ui.ctx(),
                    cur,
                    &self.theme,
                    self.prefs.custom_cursors,
                );
            }
        }
        let mut out = None;
        if !over_zoom_bar {
            if resp.drag_started_by(egui::PointerButton::Primary) {
                if let Some(p) = resp
                    .interact_pointer_pos()
                    .map(|p| ws.to_frame(rect, frame, p))
                {
                    st.drag = Some(PcDrag {
                        hit: g.hit(p, tol_px),
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
                    let snap = st.snap.then_some(st.ws.grid);
                    let ng = match d.hit {
                        Hit::Inside => d.start.moved(p - d.start_ptr, st.move_mode, snap),
                        Hit::Handle(i) => {
                            let p = match snap {
                                Some(gs) if gs >= 1.0 => {
                                    vec2((p.x / gs).round() * gs, (p.y / gs).round() * gs)
                                }
                                _ => p,
                            };
                            d.start.resize(i, p, st.lock_aspect, st.about_center)
                        }
                        Hit::Rotate => d.start.rotate(d.start_ptr, p),
                    };
                    out = Some(ng.to_key(cur, frame));
                }
            }
            if resp.drag_stopped() {
                st.drag = None;
            }
            resp.context_menu(|ui| {
                let items = [
                    (PcAction::Restore, "Restaurar"),
                    (PcAction::Center, "Centralizar"),
                    (PcAction::FlipH, "Inverter na horizontal"),
                    (PcAction::FlipV, "Inverter na vertical"),
                    (PcAction::MatchOutput, "Combinar proporção de saída"),
                    (PcAction::MatchSource, "Combinar proporção da origem"),
                ];
                for (i, (a, label)) in items.into_iter().enumerate() {
                    if i == 2 || i == 4 {
                        ui.separator();
                    }
                    if ui.button(tr(lang, label)).clicked() {
                        actions.push(a);
                        ui.close_menu();
                    }
                }
            });
        }
        out
    }

    /// Aplica o quadro-chave editado e os comandos ao evento. Devolve `true`
    /// se o projeto mudou.
    #[allow(clippy::too_many_arguments)]
    fn apply_pan_crop(
        &mut self,
        id: neoveg_common::curve::Id,
        ev: &neoveg_engine::model::Event,
        local: Ticks,
        tol: Ticks,
        frame: Vec2,
        src_size: Vec2,
        new_key: Option<PanCropKey>,
        actions: &[PcAction],
    ) -> bool {
        if new_key.is_none()
            && actions.iter().all(|a| {
                matches!(
                    a,
                    PcAction::Nav(KfNav::First | KfNav::Prev | KfNav::Next | KfNav::Last)
                )
            })
        {
            return false;
        }
        // tamanho que preenche o quadro sem barras
        let fill_size = {
            let fit = (frame.x / src_size.x).min(frame.y / src_size.y);
            let fill = (frame.x / src_size.x).max(frame.y / src_size.y);
            if fill > 0.0 && ev.maintain_aspect {
                fit / fill
            } else {
                1.0
            }
        };
        let len = ev.length;
        let Some(e) = self.project.event_mut(id) else {
            return false;
        };
        let mut changed = false;
        // como no VEGAS Pro: editar fora de um quadro-chave cria um novo
        if let Some(k) = new_key {
            e.pan_crop.set(local, k, tol);
            changed = true;
        }
        for a in actions {
            // cada comando parte do valor já atualizado pelos anteriores
            let cur = &e.pan_crop.eval(local);
            let set_cur = |e: &mut neoveg_engine::model::Event, k: PanCropKey| {
                e.pan_crop.set(local, k, tol);
            };
            match a {
                PcAction::Restore => set_cur(e, PanCropKey::default()),
                PcAction::Center => set_cur(
                    e,
                    PanCropKey {
                        cx: 0.5,
                        cy: 0.5,
                        ..cur.clone()
                    },
                ),
                PcAction::FlipH => set_cur(
                    e,
                    PanCropKey {
                        flip_h: !cur.flip_h,
                        ..cur.clone()
                    },
                ),
                PcAction::FlipV => set_cur(
                    e,
                    PanCropKey {
                        flip_v: !cur.flip_v,
                        ..cur.clone()
                    },
                ),
                PcAction::MatchOutput => set_cur(e, pan_crop_geom::match_output_aspect(cur)),
                PcAction::MatchSource => {
                    // a origem já aparece com a proporção dela quando
                    // "Manter proporção" está ligado; o retângulo acompanha
                    let src = if e.maintain_aspect { src_size } else { frame };
                    set_cur(e, pan_crop_geom::match_source_aspect(cur, frame, src));
                }
                PcAction::Preset(i) => match i {
                    0 => e.pan_crop = Keyframes::default(),
                    1 => set_cur(
                        e,
                        PanCropKey {
                            size: fill_size,
                            ..cur.clone()
                        },
                    ),
                    2 | 3 => {
                        let a_k = PanCropKey {
                            size: fill_size,
                            ..PanCropKey::default()
                        };
                        let b_k = PanCropKey {
                            cx: 0.55,
                            cy: 0.45,
                            size: fill_size * 0.7,
                            ..PanCropKey::default()
                        };
                        let (k0, k1) = if *i == 2 { (a_k, b_k) } else { (b_k, a_k) };
                        e.pan_crop.keys = vec![
                            Keyframe {
                                t: Ticks::ZERO,
                                value: k0,
                                curve: Curve::Linear,
                            },
                            Keyframe {
                                t: len,
                                value: k1,
                                curve: Curve::Linear,
                            },
                        ];
                    }
                    _ => continue,
                },
                PcAction::Nav(KfNav::Add) => set_cur(e, cur.clone()),
                PcAction::Nav(KfNav::Delete) => {
                    if e.pan_crop.keys.len() > 1 {
                        e.pan_crop.keys.retain(|k| k.t.abs_diff(local) > tol);
                    } else {
                        continue;
                    }
                }
                PcAction::Nav(_) => continue,
                PcAction::MoveKey(ki, t) => {
                    let t = (*t).clamp_safe(Ticks::ZERO, len);
                    let clash = e
                        .pan_crop
                        .keys
                        .iter()
                        .enumerate()
                        .any(|(j, k)| j != *ki && k.t.abs_diff(t) <= tol);
                    let Some(k) = e.pan_crop.keys.get_mut(*ki) else {
                        continue;
                    };
                    if clash || k.t == t {
                        continue;
                    }
                    k.t = t;
                    e.pan_crop.keys.sort_by_key(|k| k.t);
                }
                PcAction::DeleteKey(ki) => {
                    if e.pan_crop.keys.len() > 1 && *ki < e.pan_crop.keys.len() {
                        e.pan_crop.keys.remove(*ki);
                    } else {
                        continue;
                    }
                }
                PcAction::SetCurve(ki, c) => {
                    let Some(k) = e.pan_crop.keys.get_mut(*ki) else {
                        continue;
                    };
                    k.curve = *c;
                }
                PcAction::MaintainAspect(v) => e.maintain_aspect = *v,
                PcAction::StretchToFill(v) => e.pan_crop_stretch_to_fill = *v,
                PcAction::ToggleMaskEnabled(v) => {
                    e.pan_crop_mask_enabled = *v;
                    if *v && e.pan_crop_mask.is_none() {
                        e.pan_crop_mask =
                            Some(Box::new(neoveg_engine::mask::BezierMaskData::default()));
                    }
                }
                PcAction::UpdateMask(md) => {
                    e.pan_crop_mask = Some(Box::new(md.clone()));
                    e.pan_crop_mask_enabled = true;
                }
                PcAction::AddMaskKey => {
                    let md = e.pan_crop_mask.get_or_insert_with(|| {
                        Box::new(neoveg_engine::mask::BezierMaskData::default())
                    });
                    let (path, feather, opacity, expansion) = md.evaluate_at(local);
                    let (curve, smoothness) = md
                        .active_key_index(local, tol)
                        .and_then(|i| md.keyframes.get(i))
                        .map(|k| (k.curve, k.smoothness_safe()))
                        .unwrap_or((Curve::Smooth, 0.0));
                    md.keyframes.retain(|k| k.time.abs_diff(local) > tol);
                    md.keyframes.push(neoveg_engine::mask::MaskKeyframe {
                        time: local,
                        path,
                        feather,
                        opacity,
                        expansion,
                        curve,
                        smoothness,
                    });
                    md.keyframes.sort_by_key(|k| k.time);
                    e.pan_crop_mask_enabled = true;
                }
                PcAction::DeleteMaskKey(ki) => {
                    if let Some(md) = e.pan_crop_mask.as_deref_mut() {
                        if *ki < md.keyframes.len() {
                            md.keyframes.remove(*ki);
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }
                PcAction::DeleteMaskKeyAtCursor => {
                    if let Some(md) = e.pan_crop_mask.as_deref_mut() {
                        let before = md.keyframes.len();
                        md.keyframes.retain(|k| k.time.abs_diff(local) > tol);
                        if md.keyframes.len() == before {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }
                PcAction::MoveMaskKey(ki, t) => {
                    if let Some(md) = e.pan_crop_mask.as_deref_mut() {
                        if let Some(k) = md.keyframes.get_mut(*ki) {
                            k.time = (*t).clamp_safe(Ticks::ZERO, len);
                            md.keyframes.sort_by_key(|k| k.time);
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }
                PcAction::SetMaskCurve(ki, c) => {
                    let md = e.pan_crop_mask.get_or_insert_with(|| {
                        Box::new(neoveg_engine::mask::BezierMaskData::default())
                    });
                    let idx = match *ki {
                        Some(i) if i < md.keyframes.len() => i,
                        Some(_) => continue,
                        None => md.ensure_key_at(local, tol),
                    };
                    if let Some(k) = md.keyframes.get_mut(idx) {
                        k.curve = *c;
                    } else {
                        continue;
                    }
                    e.pan_crop_mask_enabled = true;
                }
                PcAction::SetMaskSmoothness(sm) => {
                    let md = e.pan_crop_mask.get_or_insert_with(|| {
                        Box::new(neoveg_engine::mask::BezierMaskData::default())
                    });
                    let idx = md.ensure_key_at(local, tol);
                    if let Some(k) = md.keyframes.get_mut(idx) {
                        k.smoothness = if sm.is_finite() {
                            sm.clamp(0.0, 100.0)
                        } else {
                            0.0
                        };
                    }
                    e.pan_crop_mask_enabled = true;
                }
                PcAction::ApplyUserPreset(_)
                | PcAction::SaveUserPreset(_)
                | PcAction::DeleteUserPreset(_)
                | PcAction::OpenEventFx => continue,
            }
            changed = true;
        }
        // Ações de preferências/navegação aplicadas fora do empréstimo mutável de `project`.
        for a in actions {
            match a {
                PcAction::ApplyUserPreset(ui_i) => {
                    if let Some(up) = self.prefs.pan_crop_presets.get(*ui_i).cloned() {
                        if let Some(e) = self.project.event_mut(id) {
                            e.pan_crop_stretch_to_fill = up.stretch_to_fill;
                            e.pan_crop.set(
                                local,
                                PanCropKey {
                                    cx: up.cx,
                                    cy: up.cy,
                                    size: up.size,
                                    rot: up.rot,
                                    ratio: up.ratio,
                                    flip_h: up.flip_h,
                                    flip_v: up.flip_v,
                                    smoothness: up.smoothness,
                                },
                                tol,
                            );
                            changed = true;
                        }
                    }
                }
                PcAction::SaveUserPreset(name) => {
                    if let Some(e) = self.project.event(id) {
                        let k = neoveg_engine::model::eval_pan_crop_smooth(&e.pan_crop, local);
                        let mut preset = neoveg_common::prefs::SavedPanCropPreset {
                            name: name.clone(),
                            cx: k.cx,
                            cy: k.cy,
                            size: k.size,
                            rot: k.rot,
                            ratio: k.ratio_safe(),
                            flip_h: k.flip_h,
                            flip_v: k.flip_v,
                            smoothness: k.smoothness_safe(),
                            stretch_to_fill: e.pan_crop_stretch_to_fill,
                        };
                        if preset.sanitize() {
                            self.prefs
                                .pan_crop_presets
                                .retain(|p| p.name != preset.name);
                            self.prefs.pan_crop_presets.push(preset);
                            self.prefs.sanitize();
                            let _ = self.prefs.save();
                        }
                    }
                }
                PcAction::DeleteUserPreset(ui_i) => {
                    if *ui_i < self.prefs.pan_crop_presets.len() {
                        self.prefs.pan_crop_presets.remove(*ui_i);
                        let _ = self.prefs.save();
                    }
                }
                PcAction::OpenEventFx => {
                    self.dlg.fx_target = Some(crate::FxTarget::Event(id));
                }
                _ => {}
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_event() -> (App, neoveg_common::curve::Id) {
        let ctx = egui::Context::default();
        let mut app = {
            let _prefs = crate::tests::PREFS_LOCK
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            App::with_context(&ctx, None)
        };
        app.insert_generated(neoveg_engine::model::GenSpec::SolidColor {
            color: [1.0, 0.0, 0.0, 1.0],
        });
        let eid = app
            .project
            .tracks
            .iter()
            .flat_map(|t| t.events.iter())
            .map(|e| e.id)
            .next()
            .expect("evento gerado");
        (app, eid)
    }

    fn apply(app: &mut App, eid: neoveg_common::curve::Id, local: Ticks, a: &[PcAction]) -> bool {
        let ev = app.project.event(eid).cloned().expect("evento");
        let tol = app.project.settings.frame_dur().div_i64(2);
        let s = &app.project.settings;
        let frame = vec2(s.width as f32, s.height as f32);
        app.apply_pan_crop(eid, &ev, local, tol, frame, frame, None, a)
    }

    #[test]
    fn comandos_do_menu_de_contexto() {
        let (mut app, eid) = app_with_event();
        assert!(apply(&mut app, eid, Ticks::ZERO, &[PcAction::FlipH]));
        let k = app.project.event(eid).unwrap().pan_crop.eval(Ticks::ZERO);
        assert!(k.flip_h && !k.flip_v);
        apply(
            &mut app,
            eid,
            Ticks::ZERO,
            &[PcAction::FlipV, PcAction::MatchOutput],
        );
        let k = app.project.event(eid).unwrap().pan_crop.eval(Ticks::ZERO);
        assert!(k.flip_v && k.ratio == 1.0);
        apply(&mut app, eid, Ticks::ZERO, &[PcAction::Restore]);
        let k = app.project.event(eid).unwrap().pan_crop.eval(Ticks::ZERO);
        assert_eq!(k, PanCropKey::default());
        // navegar não muda o projeto
        assert!(!apply(
            &mut app,
            eid,
            Ticks::ZERO,
            &[PcAction::Nav(KfNav::Next)]
        ));
    }

    #[test]
    fn quadros_chave_adicionar_mover_curva_e_excluir() {
        let (mut app, eid) = app_with_event();
        let len = app.project.event(eid).unwrap().length;
        let mid = len.div_i64(2);
        apply(&mut app, eid, mid, &[PcAction::Nav(KfNav::Add)]);
        assert_eq!(app.project.event(eid).unwrap().pan_crop.keys.len(), 2);
        // mover o segundo para além do fim fica preso no fim
        apply(&mut app, eid, mid, &[PcAction::MoveKey(1, len + len)]);
        assert_eq!(app.project.event(eid).unwrap().pan_crop.keys[1].t, len);
        // mover para cima do primeiro é recusado
        apply(&mut app, eid, mid, &[PcAction::MoveKey(1, Ticks::ZERO)]);
        assert_eq!(app.project.event(eid).unwrap().pan_crop.keys[1].t, len);
        apply(&mut app, eid, mid, &[PcAction::SetCurve(0, Curve::Hold)]);
        assert_eq!(
            app.project.event(eid).unwrap().pan_crop.keys[0].curve,
            Curve::Hold
        );
        // índices hostis não travam
        assert!(!apply(
            &mut app,
            eid,
            mid,
            &[PcAction::SetCurve(99, Curve::Fast)]
        ));
        assert!(!apply(&mut app, eid, mid, &[PcAction::MoveKey(99, mid)]));
        apply(&mut app, eid, mid, &[PcAction::DeleteKey(1)]);
        assert_eq!(app.project.event(eid).unwrap().pan_crop.keys.len(), 1);
        // o último quadro-chave não se exclui
        assert!(!apply(&mut app, eid, mid, &[PcAction::DeleteKey(0)]));
        assert_eq!(app.project.event(eid).unwrap().pan_crop.keys.len(), 1);
    }

    #[test]
    fn editar_fora_de_um_quadro_chave_cria_um_novo() {
        let (mut app, eid) = app_with_event();
        let ev = app.project.event(eid).cloned().unwrap();
        let mid = ev.length.div_i64(2);
        let cur = ev.pan_crop.eval(mid);
        let tol = app.project.settings.frame_dur().div_i64(2);
        let frame = vec2(1920.0, 1080.0);
        let k = PanCropKey {
            size: 0.5,
            ..cur.clone()
        };
        app.apply_pan_crop(eid, &ev, mid, tol, frame, frame, Some(k), &[]);
        let e = app.project.event(eid).unwrap();
        assert_eq!(e.pan_crop.keys.len(), 2);
        assert_eq!(e.pan_crop.eval(mid).size, 0.5);
        assert_eq!(e.pan_crop.eval(Ticks::ZERO).size, 1.0);
    }

    #[test]
    fn mascara_curva_suavidade_e_excluir_quadro_chave() {
        let (mut app, eid) = app_with_event();
        let len = app.project.event(eid).unwrap().length;
        let mid = len.div_i64(2);
        assert!(apply(&mut app, eid, Ticks::ZERO, &[PcAction::AddMaskKey]));
        assert!(apply(&mut app, eid, mid, &[PcAction::AddMaskKey]));
        assert!(apply(
            &mut app,
            eid,
            Ticks::ZERO,
            &[
                PcAction::SetMaskCurve(Some(0), Curve::Slow),
                PcAction::SetMaskSmoothness(75.0),
            ]
        ));
        let md = app
            .project
            .event(eid)
            .unwrap()
            .pan_crop_mask
            .as_deref()
            .unwrap();
        assert_eq!(md.keyframes.len(), 2);
        assert_eq!(md.keyframes[0].curve, Curve::Slow);
        assert!((md.keyframes[0].smoothness - 75.0).abs() < 1e-4);
        // índice hostil é ignorado sem pânico
        assert!(!apply(
            &mut app,
            eid,
            mid,
            &[PcAction::SetMaskCurve(Some(99), Curve::Hold)]
        ));
        assert!(apply(
            &mut app,
            eid,
            mid,
            &[PcAction::DeleteMaskKeyAtCursor]
        ));
        assert_eq!(
            app.project
                .event(eid)
                .unwrap()
                .pan_crop_mask
                .as_deref()
                .unwrap()
                .keyframes
                .len(),
            1
        );
    }
}
