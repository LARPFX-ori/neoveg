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
    /// Índice de vértice Bézier atualmente arrastado na máscara.
    drag_mask_vertex: Option<usize>,
    drag: Option<PcDrag>,
}

impl Default for PanCropUi {
    fn default() -> Self {
        PanCropUi {
            show_props: true,
            tool: PcTool::Normal,
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
            drag_mask_vertex: None,
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
    MoveMaskKey(usize, Ticks),
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
            // Cabeçalho com cadeia em pílulas `●—[Pan/Crop]—[FX]—●` e 3 arranjos de janela
            ui.horizontal(|ui| {
                icons::paint_inline(ui, Icon::PanCrop, 14.0, &theme);
                ui.label(egui::RichText::new("●—").weak());
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
                ui.label(egui::RichText::new("—●").weak());
                ui.separator();
                ui.add(egui::Label::new(&ev.name).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::small_icon_button(ui, Icon::ViewList, None, st.compact_header, &theme)
                        .on_hover_text(tr(lang, "Arranjo compacto"))
                        .clicked()
                    {
                        st.compact_header = !st.compact_header;
                    }
                    if icons::small_icon_button(
                        ui,
                        Icon::LayoutWindow,
                        None,
                        !st.show_props && !st.compact_header,
                        &theme,
                    )
                    .on_hover_text(tr(lang, "Somente área de trabalho"))
                    .clicked()
                    {
                        st.show_props = false;
                        st.compact_header = false;
                    }
                    if icons::small_icon_button(
                        ui,
                        Icon::GearProps,
                        None,
                        st.show_props && !st.compact_header,
                        &theme,
                    )
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
                            vec2(220.0, top_h),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(220.0);
                                egui::ScrollArea::vertical()
                                    .id_salt("pc_props")
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        if let Some(k) = self.pc_properties(
                                            ui,
                                            &ev,
                                            &cur,
                                            key_idx,
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
                .map(|m| {
                    m.keyframes
                        .iter()
                        .map(|k| (k.time, Curve::Linear))
                        .collect()
                })
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
                if st.mask_mode && n == KfNav::Add {
                    actions.push(PcAction::AddMaskKey);
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
            if let Some((0, ki, c)) = out.set_curve {
                actions.push(PcAction::SetCurve(ki, c));
            }
            if let Some((ri, ki)) = out.delete_key {
                if ri == 1 {
                    actions.push(PcAction::DeleteMaskKey(ki));
                } else {
                    actions.push(PcAction::DeleteKey(ki));
                }
            }
        });

        // tempo dos botões de navegação
        let times: Vec<Ticks> = ev.pan_crop.keys.iter().map(|k| k.t).collect();
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
            false,
            th,
            |ui| {
                kf_window::prop_row(ui, tr(lang, "Tipo:"), |ui| {
                    let current = ev.pan_crop.keys.get(key_idx).map(|k| k.curve);
                    let mut c = current.unwrap_or_default();
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
                if kf_window::prop_num(ui, tr(lang, "Suavidade:"), &mut sm, 1.0, 0, 0.0..=100.0) {
                    out = Some(PanCropKey {
                        smoothness: sm,
                        ..out.clone().unwrap_or_else(|| cur.clone())
                    });
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
                    let mut m_changed = false;
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
                        &mut md.feather,
                        0.5,
                        1,
                        0.0..=100.0,
                    ) {
                        m_changed = true;
                    }
                    let mut op_pct = md.opacity * 100.0;
                    if kf_window::prop_num(
                        ui,
                        tr(lang, "Opacidade (%):"),
                        &mut op_pct,
                        1.0,
                        0,
                        0.0..=100.0,
                    ) {
                        md.opacity = (op_pct * 0.01).clamp(0.0, 1.0);
                        m_changed = true;
                    }
                    if kf_window::prop_num(
                        ui,
                        tr(lang, "Expansão:"),
                        &mut md.expansion,
                        0.5,
                        1,
                        -100.0..=100.0,
                    ) {
                        m_changed = true;
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.small_button(tr(lang, "Retângulo")).clicked() {
                            md.default_path =
                                neoveg_engine::mask::MaskPath::rect([0.2, 0.2], [0.8, 0.8]);
                            m_changed = true;
                        }
                        if ui.small_button(tr(lang, "Elipse")).clicked() {
                            md.default_path =
                                neoveg_engine::mask::MaskPath::ellipse([0.5, 0.5], [0.35, 0.30]);
                            m_changed = true;
                        }
                        if ui.small_button(tr(lang, "Arredondado")).clicked() {
                            md.default_path = neoveg_engine::mask::MaskPath::rounded_rect(
                                [0.2, 0.2],
                                [0.8, 0.8],
                                0.08,
                            );
                            m_changed = true;
                        }
                    });
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
        let corners: Vec<Pos2> = g.corners().iter().map(|p| sc(*p)).collect();
        dashed_closed(&painter, &corners, thin);
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
            let h = Rect::from_center_size(sc(g.handle(i)), vec2(7.0, 7.0));
            painter.rect_filled(h, 0.0, handle_fill);
            painter.rect_stroke(h, 0.0, thin, egui::StrokeKind::Inside);
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
                dashed_closed(&painter, &pts, Stroke::new(1.5_f32, mcol));
            }
            if st.mask_mode {
                let vcol = Color32::from_rgb(255, 200, 60);
                for v in &mpath.vertices {
                    let sp = sc(vec2(v.point[0] * frame.x, v.point[1] * frame.y));
                    let vr = Rect::from_center_size(sp, vec2(8.0, 8.0));
                    painter.rect_filled(vr, 1.0, vcol);
                    painter.rect_stroke(vr, 1.0, thin, egui::StrokeKind::Inside);
                }
            }
        }

        // interação
        let ptr = resp
            .hover_pos()
            .or_else(|| resp.interact_pointer_pos())
            .map(|p| ws.to_frame(rect, frame, p));
        let tol = ws.frame_len(6.0);
        if st.mask_mode {
            let mut md = ev.pan_crop_mask.as_deref().cloned().unwrap_or_default();
            let shift = ui.input(|i| i.modifiers.shift);
            let hit_v = ptr.and_then(|p| {
                md.default_path
                    .vertices
                    .iter()
                    .enumerate()
                    .find_map(|(i, v)| {
                        let vp = vec2(v.point[0] * frame.x, v.point[1] * frame.y);
                        ((vp - p).length() <= tol * 1.6).then_some(i)
                    })
            });
            if resp.hovered() || st.drag_mask_vertex.is_some() {
                let cur = if st.drag_mask_vertex.is_some() {
                    crate::cursors::NeovegCursor::MaskVertex
                } else if hit_v.is_some() && shift {
                    crate::cursors::NeovegCursor::MaskPenDel
                } else if hit_v.is_some() {
                    crate::cursors::NeovegCursor::MaskVertex
                } else {
                    crate::cursors::NeovegCursor::MaskPenAdd
                };
                crate::cursors::render_cursor(
                    ui.ctx(),
                    cur,
                    &self.theme,
                    self.prefs.custom_cursors,
                );
            }
            if resp.drag_started_by(egui::PointerButton::Primary) {
                st.drag_mask_vertex = hit_v;
            }
            if resp.dragged_by(egui::PointerButton::Primary) {
                if let (Some(vi), Some(p)) = (st.drag_mask_vertex, ptr) {
                    if let Some(v) = md.default_path.vertices.get_mut(vi) {
                        v.point = [
                            (p.x / frame.x.max(1.0)).clamp(0.0, 1.0),
                            (p.y / frame.y.max(1.0)).clamp(0.0, 1.0),
                        ];
                        actions.push(PcAction::UpdateMask(md.clone()));
                    }
                }
            }
            if resp.drag_stopped() {
                st.drag_mask_vertex = None;
            }
            if resp.clicked() {
                if let Some(vi) = hit_v.filter(|_| shift && md.default_path.vertices.len() > 3) {
                    md.default_path.vertices.remove(vi);
                    actions.push(PcAction::UpdateMask(md));
                } else if hit_v.is_none() {
                    if let Some(p) = ptr {
                        if md.default_path.vertices.len() < 128 {
                            md.default_path
                                .vertices
                                .push(neoveg_engine::mask::MaskVertex::corner([
                                    (p.x / frame.x.max(1.0)).clamp(0.0, 1.0),
                                    (p.y / frame.y.max(1.0)).clamp(0.0, 1.0),
                                ]));
                            actions.push(PcAction::UpdateMask(md));
                        }
                    }
                }
            }
            return None;
        }
        if let (Some(p), PcTool::Normal) = (ptr, st.tool) {
            let cur = match st.drag.map(|d| d.hit).unwrap_or_else(|| g.hit(p, tol)) {
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
            if resp.hovered() || st.drag.is_some() {
                crate::cursors::render_cursor(
                    ui.ctx(),
                    cur,
                    &self.theme,
                    self.prefs.custom_cursors,
                );
            }
        } else if resp.hovered() && st.tool == PcTool::Zoom {
            let cur = if ui.input(|i| i.modifiers.shift || i.modifiers.alt) {
                crate::cursors::NeovegCursor::ZoomOut
            } else {
                crate::cursors::NeovegCursor::ZoomIn
            };
            crate::cursors::render_cursor(ui.ctx(), cur, &self.theme, self.prefs.custom_cursors);
        }
        // roda = zoom da área; botão do meio = deslocar a área
        if resp.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll != 0.0 {
                if let Some(hp) = resp.hover_pos() {
                    let f = (1.0 + scroll * 0.0015).clamp(0.5, 2.0);
                    st.ws.zoom_at(rect, frame, hp, f);
                }
            }
        }
        if resp.dragged_by(egui::PointerButton::Middle) {
            let d = resp.drag_delta();
            st.ws.offset -= vec2(ws.frame_len(d.x), ws.frame_len(d.y));
        }
        let mut out = None;
        match st.tool {
            PcTool::Zoom => {
                if let Some(hp) = resp.interact_pointer_pos() {
                    if resp.clicked() {
                        st.ws.zoom_at(rect, frame, hp, 1.25);
                    } else if resp.secondary_clicked() {
                        st.ws.zoom_at(rect, frame, hp, 0.8);
                    }
                }
            }
            PcTool::Normal => {
                if resp.drag_started_by(egui::PointerButton::Primary) {
                    if let Some(p) = resp
                        .interact_pointer_pos()
                        .map(|p| ws.to_frame(rect, frame, p))
                    {
                        st.drag = Some(PcDrag {
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
                    md.keyframes.retain(|k| k.time.abs_diff(local) > tol);
                    md.keyframes.push(neoveg_engine::mask::MaskKeyframe {
                        time: local,
                        path,
                        feather,
                        opacity,
                        expansion,
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
                    self.open_fx_target(crate::FxTarget::Event(id));
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
}
