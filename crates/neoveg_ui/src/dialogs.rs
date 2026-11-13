//! Janelas de diálogo: propriedades do projeto e do evento, Pan/Crop, Track
//! Motion, cadeias de efeitos, editor de mídia gerada, Renderizar Como,
//! atalhos e "sobre".

use super::{App, EditLayoutPrefs, FxTarget, LayoutType, Preferences, RenderJob};
use crate::model::*;
use crate::render::{RenderFormat, RenderSettings, WavBits};
use crate::theme::ThemeId;
use eframe::egui;
use egui::{pos2, vec2, Color32, Key, Pos2, Rect, Sense, Shape, Stroke, StrokeKind};
use neoveg_common::recommended_worker_threads;
use neoveg_engine::commands::{resolved_shortcut, Shortcut, ShortcutKey, COMMANDS};
use std::path::{Path, PathBuf};

pub struct RenderDialogState {
    pub format: RenderFormat,
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub quality: u8,
    pub include_audio: bool,
    pub wav_bits: WavBits,
}

#[derive(Default)]
pub struct Dialogs {
    pub project_props: bool,
    pub event_props: Option<Id>,
    pub pan_crop: Option<Id>,
    pub track_motion: Option<Id>,
    pub fx_target: Option<FxTarget>,
    pub gen_editor: Option<Id>,
    pub shortcuts: bool,
    pub prefs_open: bool,
    prefs_draft: Option<Preferences>,
    prefs_orig_theme: Option<String>,
    pub edit_layout_open: bool,
    edit_layout_draft: Option<EditLayoutPrefs>,
    edit_layout_orig: Option<(EditLayoutPrefs, String, f32, f32)>,
    shortcut_filter: String,
    pub capturing_shortcut: Option<&'static str>,
    pub save_layout_open: bool,
    pub save_layout_name: String,
    pub about: bool,
    pub goto_open: bool,
    goto_text: String,
    pub render: Option<RenderDialogState>,
    pub render_use_loop: bool,
    pub render_done: Option<String>,
    pc_tex: Option<(egui::TextureHandle, (Id, i64))>,
    pub pc_animate: bool,
    pub tm_animate: bool,
    tm_lock_aspect: bool,
    edit_in_progress: bool,
}

const PRESETS: [(&str, u32, u32, FrameRate); 12] = [
    ("HD 1080p 29,97", 1920, 1080, FrameRate::FPS_29_97),
    ("HD 1080p 30", 1920, 1080, FrameRate::FPS_30),
    ("HD 1080p 25 (PAL)", 1920, 1080, FrameRate::FPS_25),
    ("HD 1080p 24", 1920, 1080, FrameRate::FPS_24),
    ("HD 1080p 60", 1920, 1080, FrameRate::FPS_60),
    ("HD 720p 29,97", 1280, 720, FrameRate::FPS_29_97),
    ("HD 720p 30", 1280, 720, FrameRate::FPS_30),
    ("4K UHD 29,97", 3840, 2160, FrameRate::FPS_29_97),
    ("4K UHD 25", 3840, 2160, FrameRate::FPS_25),
    ("Vertical 1080x1920 30", 1080, 1920, FrameRate::FPS_30),
    ("Quadrado 1080x1080 30", 1080, 1080, FrameRate::FPS_30),
    ("SD 640x480 29,97", 640, 480, FrameRate::FPS_29_97),
];

impl Dialogs {
    pub fn open_prefs(&mut self, current: &Preferences) {
        self.prefs_draft = Some(current.clone());
        self.prefs_orig_theme = Some(current.theme.clone());
        self.prefs_open = true;
    }

    pub fn open_edit_layout(
        &mut self,
        current: &Preferences,
        dock_height: f32,
        preview_ratio: f32,
    ) {
        self.edit_layout_draft = Some(current.edit_layout.clone());
        self.edit_layout_orig = Some((
            current.edit_layout.clone(),
            current.theme.clone(),
            dock_height,
            preview_ratio,
        ));
        self.edit_layout_open = true;
    }

    pub fn open_render(&mut self, p: &Project, proj_path: Option<&Path>) {
        let base = proj_path
            .map(|pp| pp.with_extension("avi"))
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .unwrap_or_default()
                    .join("render.avi")
            });
        let (path, format, quality, include_audio, wav_bits) = match &self.render {
            Some(r) => (
                r.path.clone(),
                r.format,
                r.quality,
                r.include_audio,
                r.wav_bits,
            ),
            None => (
                base.to_string_lossy().to_string(),
                RenderFormat::AviMjpeg,
                90,
                true,
                WavBits::Pcm16,
            ),
        };
        self.render = Some(RenderDialogState {
            format,
            path,
            width: p.settings.width,
            height: p.settings.height,
            quality,
            include_audio,
            wav_bits,
        });
        self.render_use_loop = p.loop_region.is_some() && self.render_use_loop;
    }
}

fn time_field(
    ui: &mut egui::Ui,
    value: &mut Ticks,
    fps: FrameRate,
    sample_rate: u32,
    fmt: TimeFormat,
    id: impl std::hash::Hash,
) -> bool {
    let key = egui::Id::new(id);
    let mut text = ui
        .data_mut(|d| d.get_temp::<String>(key))
        .unwrap_or_else(|| format_ticks(*value, fps, sample_rate, fmt));
    let r = ui.add(
        egui::TextEdit::singleline(&mut text)
            .desired_width(110.0)
            .font(egui::TextStyle::Monospace),
    );
    let mut changed = false;
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(key, text.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(key));
    }
    if r.lost_focus() {
        if let Some(t) = parse_time(&text, fps, sample_rate, fmt) {
            let t = t.max(Ticks::ZERO);
            if t != *value {
                *value = t;
                changed = true;
            }
        }
        ui.data_mut(|d| d.remove::<String>(key));
    }
    changed
}

fn curve_combo(ui: &mut egui::Ui, c: &mut Curve, id: impl std::hash::Hash) -> bool {
    let before = *c;
    egui::ComboBox::from_id_salt(id)
        .selected_text(c.name())
        .show_ui(ui, |ui| {
            for k in Curve::ALL.iter().take(5) {
                ui.selectable_value(c, *k, k.name());
            }
        });
    *c != before
}

impl App {
    /// Registra desfazer para edições feitas em diálogos (uma vez por gesto).
    fn dialog_change(&mut self) {
        if !self.dlg.edit_in_progress {
            let snap = (*self.session.snapshot).clone();
            self.session.undo.push(snap);
            self.session.redo.clear();
            self.dlg.edit_in_progress = true;
        }
        self.changed();
    }

    pub fn show_dialogs(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.pointer.any_down()) && !ctx.wants_keyboard_input() {
            self.dlg.edit_in_progress = false;
        }
        self.project_props_window(ctx);
        self.event_props_window(ctx);
        self.pan_crop_window(ctx);
        self.track_motion_window(ctx);
        self.fx_window(ctx);
        self.gen_editor_window(ctx);
        self.render_window(ctx);
        self.render_progress_window(ctx);
        self.misc_windows(ctx);
    }

    // -----------------------------------------------------------------

    fn project_props_window(&mut self, ctx: &egui::Context) {
        if !self.dlg.project_props {
            return;
        }
        let mut open = true;
        let mut changed = false;
        egui::Window::new("Propriedades do projeto")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                let s = &mut self.project.settings;
                egui::ComboBox::from_label("Modelo")
                    .selected_text("Escolher modelo...")
                    .show_ui(ui, |ui| {
                        for (name, w, h, fps) in PRESETS {
                            if ui.selectable_label(false, name).clicked() {
                                s.width = w;
                                s.height = h;
                                s.fps = fps;
                                changed = true;
                            }
                        }
                    });
                egui::Grid::new("pp").num_columns(2).show(ui, |ui| {
                    ui.label("Largura");
                    changed |= ui
                        .add(egui::DragValue::new(&mut s.width).range(16..=8192))
                        .changed();
                    ui.end_row();
                    ui.label("Altura");
                    changed |= ui
                        .add(egui::DragValue::new(&mut s.height).range(16..=8192))
                        .changed();
                    ui.end_row();
                    ui.label("Quadros por segundo");
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("fps_std")
                            .selected_text(s.fps.label())
                            .show_ui(ui, |ui| {
                                for f in FrameRate::STANDARD {
                                    changed |=
                                        ui.selectable_value(&mut s.fps, f, f.label()).changed();
                                }
                            });
                        let (mut num, mut den) = (s.fps.num, s.fps.den);
                        let c_n = ui
                            .add(egui::DragValue::new(&mut num).range(1..=240_000))
                            .changed();
                        ui.label("/");
                        let c_d = ui
                            .add(egui::DragValue::new(&mut den).range(1..=1001))
                            .changed();
                        if c_n || c_d {
                            if let Some(fr) = FrameRate::new(num, den) {
                                let fv = fr.as_f64();
                                if (1.0..=240.0).contains(&fv) {
                                    s.fps = fr;
                                    changed = true;
                                }
                            }
                        }
                        ui.add_enabled_ui(s.fps.is_ntsc_drop_eligible(), |ui| {
                            changed |= ui
                                .checkbox(&mut s.drop_frame, "Drop-frame (;)")
                                .on_hover_text("SMPTE Drop-Frame para 29,97 / 59,94 qps")
                                .changed();
                        });
                    });
                    ui.end_row();
                    ui.label("Taxa de amostragem (áudio)");
                    egui::ComboBox::from_id_salt("sr")
                        .selected_text(format!("{} Hz", s.sample_rate))
                        .show_ui(ui, |ui| {
                            for r in [44100, 48000, 88200, 96000] {
                                changed |= ui
                                    .selectable_value(&mut s.sample_rate, r, format!("{r} Hz"))
                                    .changed();
                            }
                        });
                    ui.end_row();
                    ui.label("Cor de fundo");
                    changed |= ui.color_edit_button_rgb(&mut s.background).changed();
                    ui.end_row();
                });
                s.width = (s.width / 2).max(8) * 2;
                s.height = (s.height / 2).max(8) * 2;
                ui.separator();
                ui.label(
                    egui::RichText::new("Dimensões pares são exigidas pelos formatos 4:2:0.")
                        .small()
                        .weak(),
                );
            });
        if changed {
            self.session.project.settings.manually_configured = true;
            self.dialog_change();
        }
        self.dlg.project_props = open;
    }

    fn event_props_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.dlg.event_props else {
            return;
        };
        let Some((ti, _)) = self.project.find_event(id) else {
            self.dlg.event_props = None;
            return;
        };
        let is_video = self.project.tracks[ti].kind == TrackKind::Video;
        let fps = self.project.settings.fps;
        let sr = self.project.settings.sample_rate;
        let fd = self.project.settings.frame_dur();
        let tf = self
            .project
            .settings
            .effective_time_format(self.opts.time_format);
        let mut open = true;
        let mut changed = false;
        let media_info = {
            let Some(e) = self.project.event(id) else {
                return;
            };
            self.project
                .media(e.media_id)
                .map(|m| format!("{}  —  {}", m.name, m.info))
                .unwrap_or_default()
        };
        let mut open_pc = false;
        let mut open_fx = false;
        egui::Window::new("Propriedades do evento")
            .open(&mut open)
            .default_width(380.0)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(media_info).small());
                ui.separator();
                let Some(e) = self.project.event_mut(id) else {
                    return;
                };
                egui::Grid::new(("ep", id)).num_columns(2).show(ui, |ui| {
                    ui.label("Nome");
                    changed |= ui.text_edit_singleline(&mut e.name).changed();
                    ui.end_row();
                    ui.label("Início");
                    changed |= time_field(ui, &mut e.start, fps, sr, tf, ("ep_start", id));
                    ui.end_row();
                    ui.label("Duração");
                    let mut len = e.length;
                    if time_field(ui, &mut len, fps, sr, tf, ("ep_len", id)) {
                        e.length = len.max(fd);
                        e.fade_in.len = e.fade_in.len.min(e.length);
                        e.fade_out.len = e.fade_out.len.min(e.length - e.fade_in.len);
                        changed = true;
                    }
                    ui.end_row();
                    ui.label("Deslocamento na mídia");
                    changed |= time_field(ui, &mut e.offset, fps, sr, tf, ("ep_off", id));
                    ui.end_row();
                    ui.label("Velocidade");
                    let mut pct = e.rate.percent_f64();
                    if ui
                        .add(
                            egui::DragValue::new(&mut pct)
                                .speed(1.0)
                                .range(0.0..=1000.0)
                                .suffix(" %"),
                        )
                        .changed()
                    {
                        if let Some(r) = Rate::from_f64(pct / 100.0) {
                            e.rate = r;
                            changed = true;
                        }
                    }
                    ui.end_row();
                    if !is_video {
                        ui.label("Ganho");
                        changed |= ui
                            .add(egui::Slider::new(&mut e.gain_db, -40.0..=20.0).suffix(" dB"))
                            .changed();
                        ui.end_row();
                    }
                    ui.label("Fade de entrada");
                    ui.horizontal(|ui| {
                        let mut fi = e.fade_in.len;
                        if time_field(ui, &mut fi, fps, sr, tf, ("ep_fi", id)) {
                            e.fade_in.len = fi.clamp_safe(Ticks::ZERO, e.length - e.fade_out.len);
                            changed = true;
                        }
                        changed |= curve_combo(ui, &mut e.fade_in.curve, ("fic", id));
                    });
                    ui.end_row();
                    ui.label("Fade de saída");
                    ui.horizontal(|ui| {
                        let mut fo = e.fade_out.len;
                        if time_field(ui, &mut fo, fps, sr, tf, ("ep_fo", id)) {
                            e.fade_out.len = fo.clamp_safe(Ticks::ZERO, e.length - e.fade_in.len);
                            changed = true;
                        }
                        changed |= curve_combo(ui, &mut e.fade_out.curve, ("foc", id));
                    });
                    ui.end_row();
                    ui.label("Opções");
                    ui.horizontal_wrapped(|ui| {
                        changed |= ui.checkbox(&mut e.mute, "Mudo").changed();
                        changed |= ui.checkbox(&mut e.locked, "Travado").changed();
                        changed |= ui.checkbox(&mut e.looped, "Loop").changed();
                        changed |= ui.checkbox(&mut e.reverse, "Reverso").changed();
                        if is_video {
                            changed |= ui
                                .checkbox(&mut e.maintain_aspect, "Manter proporção")
                                .changed();
                        }
                    });
                    ui.end_row();
                    if is_video {
                        ui.label("Transição de entrada");
                        ui.vertical(|ui| {
                            let mut kind = e.transition.as_ref().map(|t| t.kind);
                            let before = kind;
                            egui::ComboBox::from_id_salt(("trk", id))
                                .selected_text(
                                    kind.map(|k| k.name()).unwrap_or("Nenhuma (crossfade)"),
                                )
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut kind, None, "Nenhuma (crossfade)");
                                    for k in TransitionKind::ALL {
                                        ui.selectable_value(&mut kind, Some(k), k.name());
                                    }
                                });
                            if kind != before {
                                e.transition = kind.map(|k| TransitionSpec {
                                    kind: k,
                                    softness: 0.05,
                                });
                                changed = true;
                            }
                            if let Some(tr) = &mut e.transition {
                                changed |= ui
                                    .add(
                                        egui::Slider::new(&mut tr.softness, 0.0..=0.5)
                                            .text("Suavidade da borda"),
                                    )
                                    .changed();
                            }
                        });
                        ui.end_row();
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if is_video && ui.button("Pan/Crop...").clicked() {
                        open_pc = true;
                    }
                    if ui.button("Efeitos do evento...").clicked() {
                        open_fx = true;
                    }
                });
            });
        if open_pc {
            self.dlg.pan_crop = Some(id);
        }
        if open_fx {
            self.dlg.fx_target = Some(FxTarget::Event(id));
        }
        if changed {
            self.dialog_change();
        }
        if !open {
            self.dlg.event_props = None;
        }
    }

    // -----------------------------------------------------------------
    // Pan/Crop
    // -----------------------------------------------------------------

    fn pan_crop_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.dlg.pan_crop else { return };
        let Some(ev) = self.project.event(id).cloned() else {
            self.dlg.pan_crop = None;
            return;
        };
        let Some(m) = self.project.media(ev.media_id).cloned() else {
            return;
        };
        let s = self.project.settings.clone();
        let local = (self.cursor - ev.start).clamp_safe(Ticks::ZERO, ev.length);
        let tol = s.frame_dur().div_i64(2);
        // textura da imagem de origem no tempo atual
        let frame_key = (id, s.fps.ticks_to_frame_round(local));
        if self
            .dlg
            .pc_tex
            .as_ref()
            .map(|t| t.1 != frame_key)
            .unwrap_or(true)
            || self.generation.is_multiple_of(97)
        {
            let mt = ev
                .media_time(ev.start + local, m.duration)
                .unwrap_or(Ticks::ZERO);
            let (gw, gh) = (s.width / 3, s.height / 3);
            if let Some(img) = self.media.video_frame(&m, mt, gw, gh) {
                let max = 640u32;
                let sc = (max as f32 / img.width().max(img.height()) as f32).min(1.0);
                let small = if sc < 1.0 {
                    image::imageops::resize(
                        &*img,
                        (img.width() as f32 * sc) as u32,
                        (img.height() as f32 * sc) as u32,
                        image::imageops::FilterType::Triangle,
                    )
                } else {
                    (*img).clone()
                };
                let ci = super::rgba_to_color_image(&small);
                let tex = ctx.load_texture("pancrop_src", ci, egui::TextureOptions::LINEAR);
                self.dlg.pc_tex = Some((tex, frame_key));
            }
        }
        let mut open = true;
        let mut new_key: Option<PanCropKey> = None;
        let mut key_action: Option<&str> = None;
        let mut sel_key_time: Option<Ticks> = None;
        let mut key_curve_change: Option<(usize, Curve)> = None;
        let cur = ev.pan_crop.eval(local);
        egui::Window::new(format!("Pan/Crop do evento — {}", ev.name))
            .open(&mut open)
            .default_size([760.0, 480.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // canvas
                    let canvas_size =
                        vec2(460.0, 460.0 / (s.width as f32 / s.height as f32).max(0.3))
                            .min(vec2(460.0, 420.0));
                    let (rect, resp) = ui.allocate_exact_size(canvas_size, Sense::click_and_drag());
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 0.0, Color32::from_rgb(20, 20, 22));
                    // espaço de saída com margem de 25%
                    let margin = 0.25;
                    let sx = rect.width() / (s.width as f32 * (1.0 + 2.0 * margin));
                    let sy = rect.height() / (s.height as f32 * (1.0 + 2.0 * margin));
                    let sc = sx.min(sy);
                    let origin = rect.center() - vec2(s.width as f32, s.height as f32) * sc / 2.0;
                    let to_canvas = |x: f32, y: f32| -> Pos2 { origin + vec2(x, y) * sc };
                    // imagem de origem no retângulo de ajuste (letterbox)
                    let (sw, sh) = if m.width > 0 {
                        (m.width as f32, m.height as f32)
                    } else {
                        (s.width as f32, s.height as f32)
                    };
                    let (fw, fh) = if ev.maintain_aspect {
                        let k = (s.width as f32 / sw).min(s.height as f32 / sh);
                        (sw * k, sh * k)
                    } else {
                        (s.width as f32, s.height as f32)
                    };
                    let fit = Rect::from_min_max(
                        to_canvas((s.width as f32 - fw) / 2.0, (s.height as f32 - fh) / 2.0),
                        to_canvas((s.width as f32 + fw) / 2.0, (s.height as f32 + fh) / 2.0),
                    );
                    if let Some((tex, _)) = &self.dlg.pc_tex {
                        painter.image(
                            tex.id(),
                            fit,
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::from_white_alpha(200),
                        );
                    }
                    painter.rect_stroke(
                        Rect::from_min_max(
                            to_canvas(0.0, 0.0),
                            to_canvas(s.width as f32, s.height as f32),
                        ),
                        0.0,
                        Stroke::new(1.0_f32, Color32::from_gray(90)),
                        StrokeKind::Inside,
                    );
                    // retângulo de enquadramento (rotacionado)
                    let (sn, cs) = cur.rot.to_radians().sin_cos();
                    let hw = s.width as f32 * cur.size / 2.0;
                    let hh = s.height as f32 * cur.size / 2.0;
                    let cx = cur.cx * s.width as f32;
                    let cy = cur.cy * s.height as f32;
                    let corner = |dx: f32, dy: f32| {
                        to_canvas(cx + dx * cs - dy * sn, cy + dx * sn + dy * cs)
                    };
                    let pts = vec![
                        corner(-hw, -hh),
                        corner(hw, -hh),
                        corner(hw, hh),
                        corner(-hw, hh),
                    ];
                    painter.add(Shape::closed_line(
                        pts.clone(),
                        Stroke::new(2.0_f32, Color32::from_rgb(255, 200, 60)),
                    ));
                    painter.circle_filled(to_canvas(cx, cy), 4.0, Color32::from_rgb(255, 200, 60));
                    painter.line_segment(
                        [to_canvas(cx, cy), corner(0.0, -hh)],
                        Stroke::new(1.0_f32, Color32::from_rgb(255, 200, 60)),
                    );
                    painter.text(
                        pts[0] + vec2(3.0, 3.0),
                        egui::Align2::LEFT_TOP,
                        "F",
                        egui::FontId::proportional(12.0),
                        Color32::from_rgb(255, 200, 60),
                    );
                    // interação: arrastar move; roda = zoom
                    if resp.dragged() {
                        let d = resp.drag_delta() / sc;
                        let mut k = cur.clone();
                        k.cx -= -d.x / s.width as f32;
                        k.cy -= -d.y / s.height as f32;
                        new_key = Some(k);
                    }
                    if resp.hovered() {
                        let sd = ui.input(|i| i.raw_scroll_delta.y);
                        if sd != 0.0 {
                            let mut k = cur.clone();
                            k.size = (k.size * (1.0 - sd * 0.001)).clamp(0.02, 10.0);
                            new_key = Some(k);
                        }
                    }
                    ui.vertical(|ui| {
                        ui.label(format!("Tempo local: {}", self.fmt_time(local)));
                        ui.checkbox(
                            &mut self.dlg.pc_animate,
                            "Animar (criar keyframe ao editar)",
                        );
                        let mut k = new_key.clone().unwrap_or(cur.clone());
                        let mut ch = false;
                        ch |= ui
                            .add(egui::Slider::new(&mut k.cx, -0.5..=1.5).text("Centro X"))
                            .changed();
                        ch |= ui
                            .add(egui::Slider::new(&mut k.cy, -0.5..=1.5).text("Centro Y"))
                            .changed();
                        let mut zoom = 100.0 / k.size;
                        if ui
                            .add(
                                egui::Slider::new(&mut zoom, 10.0..=1000.0)
                                    .logarithmic(true)
                                    .text("Zoom %"),
                            )
                            .changed()
                        {
                            k.size = 100.0 / zoom;
                            ch = true;
                        }
                        ch |= ui
                            .add(egui::Slider::new(&mut k.rot, -180.0..=180.0).text("Rotação °"))
                            .changed();
                        if ch {
                            new_key = Some(k);
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Redefinir").clicked() {
                                key_action = Some("reset");
                            }
                            if ui
                                .button("Ajustar preenchendo")
                                .on_hover_text("Recorta para preencher o quadro sem barras")
                                .clicked()
                            {
                                key_action = Some("fill");
                            }
                        });
                        ui.separator();
                        ui.label(egui::RichText::new("Keyframes").strong());
                        ui.horizontal(|ui| {
                            if ui.button("◆ Adicionar no cursor").clicked() {
                                key_action = Some("add");
                            }
                            if ui.button("Excluir no cursor").clicked() {
                                key_action = Some("del");
                            }
                            if ui.button("Limpar").clicked() {
                                key_action = Some("clear");
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Ken Burns: zoom in").clicked() {
                                key_action = Some("kb_in");
                            }
                            if ui.button("zoom out").clicked() {
                                key_action = Some("kb_out");
                            }
                        });
                        // faixa de keyframes
                        let (kr, kresp) = ui.allocate_exact_size(vec2(260.0, 22.0), Sense::click());
                        let p = ui.painter_at(kr);
                        p.rect_filled(kr, 2.0, Color32::from_gray(30));
                        let min_span = ev.length.max(Ticks::from_raw(1));
                        let x_of = |t: Ticks| {
                            kr.left() + t.fraction_in(Ticks::ZERO, min_span) * kr.width()
                        };
                        for kf in &ev.pan_crop.keys {
                            let x = x_of(kf.t);
                            p.add(Shape::convex_polygon(
                                vec![
                                    pos2(x, kr.top() + 3.0),
                                    pos2(x + 5.0, kr.center().y),
                                    pos2(x, kr.bottom() - 3.0),
                                    pos2(x - 5.0, kr.center().y),
                                ],
                                Color32::from_rgb(255, 200, 60),
                                Stroke::NONE,
                            ));
                        }
                        let cx = x_of(local);
                        p.line_segment(
                            [pos2(cx, kr.top()), pos2(cx, kr.bottom())],
                            Stroke::new(1.0_f32, Color32::WHITE),
                        );
                        if kresp.clicked() {
                            if let Some(pp) = kresp.interact_pointer_pos() {
                                let frac = ((pp.x - kr.left()) / kr.width()).clamp(0.0, 1.0) as f64;
                                let t =
                                    Ticks::from_secs_f64_clamped(ev.length.as_secs_f64() * frac);
                                // aproxima para um keyframe próximo
                                let near = ev
                                    .pan_crop
                                    .keys
                                    .iter()
                                    .find(|k| (x_of(k.t) - pp.x).abs() < 6.0)
                                    .map(|k| k.t);
                                sel_key_time = Some(near.unwrap_or(t));
                            }
                        }
                        egui::ScrollArea::vertical()
                            .max_height(120.0)
                            .show(ui, |ui| {
                                for (i, kf) in ev.pan_crop.keys.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        if ui
                                            .small_button(format!("◆ {}", self.fmt_time(kf.t)))
                                            .clicked()
                                        {
                                            sel_key_time = Some(kf.t);
                                        }
                                        let mut c = kf.curve;
                                        if curve_combo(ui, &mut c, ("pck", id, i)) {
                                            key_curve_change = Some((i, c));
                                        }
                                    });
                                }
                            });
                    });
                });
            });
        if let Some(t) = sel_key_time {
            self.set_cursor(ev.start + t);
        }
        let mut changed = false;
        if let Some(k) = new_key {
            let Some(e) = self.session.project.event_mut(id) else {
                return;
            };
            if self.dlg.pc_animate || e.pan_crop.keys.len() > 1 {
                e.pan_crop.set(local, k, tol);
            } else if let Some(first) = e.pan_crop.keys.first_mut() {
                first.value = k;
            }
            changed = true;
        }
        if let Some((i, c)) = key_curve_change {
            if let Some(k) = self
                .project
                .event_mut(id)
                .and_then(|e| e.pan_crop.keys.get_mut(i))
            {
                k.curve = c;
            }
            changed = true;
        }
        if let Some(a) = key_action {
            let fill_size = {
                let (sw, sh) = (m.width.max(1) as f32, m.height.max(1) as f32);
                let fit = (s.width as f32 / sw).min(s.height as f32 / sh);
                let fill = (s.width as f32 / sw).max(s.height as f32 / sh);
                fit / fill
            };
            let Some(e) = self.project.event_mut(id) else {
                return;
            };
            match a {
                "reset" => e.pan_crop = Keyframes::default(),
                "fill" => {
                    let mut k = cur.clone();
                    k.size = fill_size;
                    e.pan_crop.set(local, k, tol);
                }
                "add" => e.pan_crop.set(local, cur.clone(), tol),
                "del" => {
                    if e.pan_crop.keys.len() > 1 {
                        e.pan_crop.keys.retain(|k| k.t.abs_diff(local) > tol);
                    }
                }
                "clear" => {
                    let v = cur.clone();
                    e.pan_crop.keys = vec![Keyframe {
                        t: Ticks::ZERO,
                        value: v,
                        curve: Curve::Smooth,
                    }];
                }
                "kb_in" | "kb_out" => {
                    let a_k = PanCropKey {
                        cx: 0.5,
                        cy: 0.5,
                        size: fill_size,
                        rot: 0.0,
                    };
                    let b_k = PanCropKey {
                        cx: 0.55,
                        cy: 0.45,
                        size: fill_size * 0.7,
                        rot: 0.0,
                    };
                    let (k0, k1) = if a == "kb_in" { (a_k, b_k) } else { (b_k, a_k) };
                    e.pan_crop.keys = vec![
                        Keyframe {
                            t: Ticks::ZERO,
                            value: k0,
                            curve: Curve::Linear,
                        },
                        Keyframe {
                            t: e.length,
                            value: k1,
                            curve: Curve::Linear,
                        },
                    ];
                }
                _ => {}
            }
            changed = true;
        }
        if changed {
            self.dialog_change();
        }
        if !open {
            self.dlg.pan_crop = None;
            self.dlg.pc_tex = None;
        }
    }

    // -----------------------------------------------------------------
    // Track Motion
    // -----------------------------------------------------------------

    fn track_motion_window(&mut self, ctx: &egui::Context) {
        let Some(tid) = self.dlg.track_motion else {
            return;
        };
        let Some(ti) = self.project.tracks.iter().position(|t| t.id == tid) else {
            self.dlg.track_motion = None;
            return;
        };
        let s = self.project.settings.clone();
        let t = self.cursor;
        let tol = s.frame_dur().div_i64(2);
        let cur = self.project.tracks[ti].motion.eval(t);
        let keys: Vec<(Ticks, Curve)> = self.project.tracks[ti]
            .motion
            .keys
            .iter()
            .map(|k| (k.t, k.curve))
            .collect();
        let mut open = true;
        let mut new_key: Option<MotionKey> = None;
        let mut action: Option<&str> = None;
        let mut jump: Option<Ticks> = None;
        let name = self.project.tracks[ti].name.clone();
        egui::Window::new(format!("Movimento da trilha — {name}"))
            .open(&mut open)
            .default_size([720.0, 420.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let size = vec2(420.0, 420.0 * s.height as f32 / s.width as f32)
                        .min(vec2(420.0, 360.0));
                    let (rect, resp) = ui.allocate_exact_size(size, Sense::drag());
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 0.0, Color32::from_rgb(20, 20, 22));
                    let margin = 0.3;
                    let sc = (rect.width() / (s.width as f32 * (1.0 + 2.0 * margin)))
                        .min(rect.height() / (s.height as f32 * (1.0 + 2.0 * margin)));
                    let origin = rect.center() - vec2(s.width as f32, s.height as f32) * sc / 2.0;
                    let to_c = |x: f32, y: f32| origin + vec2(x, y) * sc;
                    if let Some(tex) = &self.preview_tex {
                        painter.image(
                            tex.id(),
                            Rect::from_min_max(
                                to_c(0.0, 0.0),
                                to_c(s.width as f32, s.height as f32),
                            ),
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::from_white_alpha(120),
                        );
                    }
                    painter.rect_stroke(
                        Rect::from_min_max(to_c(0.0, 0.0), to_c(s.width as f32, s.height as f32)),
                        0.0,
                        Stroke::new(1.0_f32, Color32::from_gray(120)),
                        StrokeKind::Inside,
                    );
                    let (sn, cs) = cur.rot.to_radians().sin_cos();
                    let hw = s.width as f32 * cur.scale_x / 2.0;
                    let hh = s.height as f32 * cur.scale_y / 2.0;
                    let cx = s.width as f32 / 2.0 + cur.x * s.width as f32;
                    let cy = s.height as f32 / 2.0 + cur.y * s.height as f32;
                    let corner =
                        |dx: f32, dy: f32| to_c(cx + dx * cs - dy * sn, cy + dx * sn + dy * cs);
                    painter.add(Shape::closed_line(
                        vec![
                            corner(-hw, -hh),
                            corner(hw, -hh),
                            corner(hw, hh),
                            corner(-hw, hh),
                        ],
                        Stroke::new(2.0_f32, Color32::from_rgb(90, 200, 255)),
                    ));
                    painter.circle_filled(to_c(cx, cy), 4.0, Color32::from_rgb(90, 200, 255));
                    if resp.dragged() {
                        let d = resp.drag_delta() / sc;
                        let mut k = cur.clone();
                        k.x += d.x / s.width as f32;
                        k.y += d.y / s.height as f32;
                        new_key = Some(k);
                    }
                    if resp.hovered() {
                        let sd = ui.input(|i| i.raw_scroll_delta.y);
                        if sd != 0.0 {
                            let mut k = cur.clone();
                            let f = 1.0 + sd * 0.001;
                            k.scale_x = (k.scale_x * f).clamp(0.01, 20.0);
                            k.scale_y = (k.scale_y * f).clamp(0.01, 20.0);
                            new_key = Some(k);
                        }
                    }
                    ui.vertical(|ui| {
                        ui.label(format!("Tempo: {}", self.fmt_time(t)));
                        ui.checkbox(
                            &mut self.dlg.tm_animate,
                            "Animar (criar keyframe ao editar)",
                        );
                        ui.checkbox(&mut self.dlg.tm_lock_aspect, "Travar proporção");
                        let mut k = new_key.clone().unwrap_or(cur.clone());
                        let mut ch = false;
                        ch |= ui
                            .add(egui::Slider::new(&mut k.x, -1.5..=1.5).text("Posição X"))
                            .changed();
                        ch |= ui
                            .add(egui::Slider::new(&mut k.y, -1.5..=1.5).text("Posição Y"))
                            .changed();
                        let old_sx = k.scale_x;
                        let old_sy = k.scale_y;
                        if ui
                            .add(
                                egui::Slider::new(&mut k.scale_x, 0.01..=5.0)
                                    .logarithmic(true)
                                    .text("Largura"),
                            )
                            .changed()
                        {
                            if self.dlg.tm_lock_aspect {
                                k.scale_y = old_sy * k.scale_x / old_sx.max(1e-4);
                            }
                            ch = true;
                        }
                        if ui
                            .add(
                                egui::Slider::new(&mut k.scale_y, 0.01..=5.0)
                                    .logarithmic(true)
                                    .text("Altura"),
                            )
                            .changed()
                        {
                            if self.dlg.tm_lock_aspect {
                                k.scale_x = old_sx * k.scale_y / old_sy.max(1e-4);
                            }
                            ch = true;
                        }
                        ch |= ui
                            .add(egui::Slider::new(&mut k.rot, -360.0..=360.0).text("Rotação °"))
                            .changed();
                        if ch {
                            new_key = Some(k);
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Redefinir").clicked() {
                                action = Some("reset");
                            }
                            if ui.button("Picture-in-picture").clicked() {
                                action = Some("pip");
                            }
                        });
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.button("◆ Keyframe no cursor").clicked() {
                                action = Some("add");
                            }
                            if ui.button("Excluir no cursor").clicked() {
                                action = Some("del");
                            }
                        });
                        egui::ScrollArea::vertical()
                            .max_height(140.0)
                            .show(ui, |ui| {
                                for (kt, _) in &keys {
                                    if ui
                                        .small_button(format!("◆ {}", self.fmt_time(*kt)))
                                        .clicked()
                                    {
                                        jump = Some(*kt);
                                    }
                                }
                            });
                    });
                });
            });
        if let Some(j) = jump {
            self.set_cursor(j);
        }
        let mut changed = false;
        if let Some(k) = new_key {
            let tr = &mut self.session.project.tracks[ti];
            if self.dlg.tm_animate || tr.motion.keys.len() > 1 {
                tr.motion.set(t, k, tol);
            } else if let Some(f) = tr.motion.keys.first_mut() {
                f.value = k;
            }
            changed = true;
        }
        if let Some(a) = action {
            let tr = &mut self.project.tracks[ti];
            match a {
                "reset" => tr.motion = Keyframes::default(),
                "pip" => {
                    let k = MotionKey {
                        x: 0.3,
                        y: 0.3,
                        scale_x: 0.33,
                        scale_y: 0.33,
                        rot: 0.0,
                    };
                    if tr.motion.keys.len() > 1 {
                        tr.motion.set(t, k, tol);
                    } else {
                        tr.motion.keys[0].value = k;
                    }
                }
                "add" => tr.motion.set(t, cur.clone(), tol),
                "del" if tr.motion.keys.len() > 1 => {
                    tr.motion.keys.retain(|k| k.t.abs_diff(t) > tol);
                }
                _ => {}
            }
            changed = true;
        }
        if changed {
            self.dialog_change();
        }
        if !open {
            self.dlg.track_motion = None;
        }
    }

    // -----------------------------------------------------------------
    // Cadeia de efeitos
    // -----------------------------------------------------------------

    fn fx_chain_mut(&mut self, target: FxTarget) -> Option<&mut Vec<FxInstance>> {
        match target {
            FxTarget::Event(id) => self.project.event_mut(id).map(|e| &mut e.fx),
            FxTarget::Track(id) => self
                .project
                .tracks
                .iter_mut()
                .find(|t| t.id == id)
                .map(|t| &mut t.fx),
            FxTarget::MasterAudio => Some(&mut self.project.master.fx),
            FxTarget::MasterVideo => Some(&mut self.project.master.video_fx),
        }
    }

    fn fx_window(&mut self, ctx: &egui::Context) {
        let Some(target) = self.dlg.fx_target else {
            return;
        };
        let (title, audio, local_t) = match target {
            FxTarget::Event(id) => {
                let Some((ti, _)) = self.project.find_event(id) else {
                    self.dlg.fx_target = None;
                    return;
                };
                let Some(e) = self.project.event(id) else {
                    return;
                };
                (
                    format!("Efeitos do evento — {}", e.name),
                    self.project.tracks[ti].kind == TrackKind::Audio,
                    (self.cursor - e.start).clamp_safe(Ticks::ZERO, e.length),
                )
            }
            FxTarget::Track(id) => {
                let Some(t) = self.project.tracks.iter().find(|t| t.id == id) else {
                    self.dlg.fx_target = None;
                    return;
                };
                (
                    format!("Efeitos da trilha — {}", t.name),
                    t.kind == TrackKind::Audio,
                    self.cursor,
                )
            }
            FxTarget::MasterAudio => ("Efeitos do master bus (áudio)".into(), true, self.cursor),
            FxTarget::MasterVideo => ("Efeitos de saída de vídeo".into(), false, self.cursor),
        };
        let tol = self.project.settings.frame_dur().div_i64(2);
        let fmt_local = self.fmt_time(local_t);
        let mut open = true;
        let mut changed = false;
        let mut add: Option<FxKind> = None;
        let mut op: Option<(usize, &str)> = None;
        let Some(chain) = self.fx_chain_mut(target) else {
            return;
        };
        let mut chain_copy = chain.clone();
        egui::Window::new(title)
            .open(&mut open)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("fx_add")
                        .selected_text("➕ Adicionar efeito...")
                        .show_ui(ui, |ui| {
                            let list: &[FxKind] = if audio {
                                &FxKind::AUDIO
                            } else {
                                &FxKind::VIDEO
                            };
                            for k in list {
                                if ui.selectable_label(false, k.name()).clicked() {
                                    add = Some(*k);
                                }
                            }
                        });
                    ui.label(egui::RichText::new(format!("tempo local: {fmt_local}")).small());
                });
                ui.separator();
                if chain_copy.is_empty() {
                    ui.label("Nenhum efeito na cadeia.");
                }
                egui::ScrollArea::vertical()
                    .max_height(520.0)
                    .show(ui, |ui| {
                        let n = chain_copy.len();
                        for (i, fx) in chain_copy.iter_mut().enumerate() {
                            ui.push_id(fx.id, |ui| {
                                ui.horizontal(|ui| {
                                    changed |= ui
                                        .checkbox(&mut fx.enabled, "")
                                        .on_hover_text("Ativar/ignorar (bypass)")
                                        .changed();
                                    ui.label(egui::RichText::new(fx.kind.name()).strong());
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .small_button("✕")
                                                .on_hover_text("Remover")
                                                .clicked()
                                            {
                                                op = Some((i, "del"));
                                            }
                                            if i + 1 < n && ui.small_button("▼").clicked() {
                                                op = Some((i, "down"));
                                            }
                                            if i > 0 && ui.small_button("▲").clicked() {
                                                op = Some((i, "up"));
                                            }
                                            if ui
                                                .small_button("⟲")
                                                .on_hover_text("Valores padrão")
                                                .clicked()
                                            {
                                                op = Some((i, "reset"));
                                            }
                                        },
                                    );
                                });
                                let specs = fx.kind.params();
                                egui::Grid::new("params").num_columns(3).show(ui, |ui| {
                                    for (pi, spec) in specs.iter().enumerate() {
                                        let Some(p) = fx.params.get_mut(pi) else {
                                            continue;
                                        };
                                        ui.label(spec.name);
                                        let mut v = p.eval(local_t);
                                        let r = if spec.toggle {
                                            let mut b = v > 0.5;
                                            let r = ui.checkbox(&mut b, "");
                                            v = if b { 1.0 } else { 0.0 };
                                            r
                                        } else {
                                            let mut sl =
                                                egui::Slider::new(&mut v, spec.min..=spec.max);
                                            if spec.max > 1000.0
                                                || (spec.min > 0.0
                                                    && spec.max / spec.min.max(1e-3) > 100.0)
                                            {
                                                sl = sl.logarithmic(true);
                                            }
                                            ui.add(sl)
                                        };
                                        if r.changed() {
                                            if p.keys.is_empty() {
                                                p.value = v;
                                            } else {
                                                p.set_key(local_t, v, tol);
                                            }
                                            changed = true;
                                        }
                                        ui.horizontal(|ui| {
                                            let has_key_here =
                                                p.keys.iter().any(|k| k.0.abs_diff(local_t) <= tol);
                                            let lbl = if has_key_here { "◆" } else { "◇" };
                                            if ui
                                                .small_button(lbl)
                                                .on_hover_text("Criar keyframe neste tempo")
                                                .clicked()
                                            {
                                                if p.keys.is_empty() && local_t > tol {
                                                    p.keys.push((Ticks::ZERO, p.value));
                                                }
                                                p.set_key(local_t, v, tol);
                                                changed = true;
                                            }
                                            if !p.keys.is_empty() {
                                                ui.label(
                                                    egui::RichText::new(format!(
                                                        "{}",
                                                        p.keys.len()
                                                    ))
                                                    .small(),
                                                );
                                                if ui
                                                    .small_button("⌫")
                                                    .on_hover_text("Remover keyframes")
                                                    .clicked()
                                                {
                                                    p.value = v;
                                                    p.keys.clear();
                                                    changed = true;
                                                }
                                            }
                                        });
                                        ui.end_row();
                                    }
                                });
                                ui.separator();
                            });
                        }
                    });
            });
        if let Some(k) = add {
            chain_copy.push(FxInstance::new(k));
            changed = true;
        }
        if let Some((i, what)) = op {
            match what {
                "del" => {
                    chain_copy.remove(i);
                }
                "up" => chain_copy.swap(i, i - 1),
                "down" => chain_copy.swap(i, i + 1),
                "reset" => {
                    let id = chain_copy[i].id;
                    chain_copy[i] = FxInstance::new(chain_copy[i].kind);
                    chain_copy[i].id = id;
                }
                _ => {}
            }
            changed = true;
        }
        if changed {
            if let Some(chain) = self.fx_chain_mut(target) {
                *chain = chain_copy;
            }
            self.dialog_change();
        }
        if !open {
            self.dlg.fx_target = None;
        }
    }

    // -----------------------------------------------------------------
    // Mídia gerada
    // -----------------------------------------------------------------

    fn gen_editor_window(&mut self, ctx: &egui::Context) {
        let Some(mid) = self.dlg.gen_editor else {
            return;
        };
        let Some(mi) = self.project.media.iter().position(|m| m.id == mid) else {
            self.dlg.gen_editor = None;
            return;
        };
        let MediaSource::Generated(mut spec) = self.project.media[mi].source.clone() else {
            self.dlg.gen_editor = None;
            return;
        };
        if self.gen_fonts.is_empty() {
            self.gen_fonts = crate::media::generate::list_system_fonts();
            if self.gen_fonts.is_empty() {
                self.gen_fonts.push(String::new());
            }
        }
        let fonts = self.gen_fonts.clone();
        let mut open = true;
        let before = spec.clone();
        egui::Window::new("Mídia gerada")
            .open(&mut open)
            .default_width(420.0)
            .show(ctx, |ui| match &mut spec {
                GenSpec::Text(t) => {
                    ui.label("Texto:");
                    ui.add(
                        egui::TextEdit::multiline(&mut t.text)
                            .desired_rows(4)
                            .desired_width(f32::INFINITY),
                    );
                    egui::Grid::new("gen_txt").num_columns(2).show(ui, |ui| {
                        ui.label("Fonte");
                        let cur = if t.font_path.is_empty() {
                            "Padrão".to_string()
                        } else {
                            Path::new(&t.font_path)
                                .file_name()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_default()
                        };
                        egui::ComboBox::from_id_salt("font")
                            .selected_text(cur)
                            .height(300.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut t.font_path, String::new(), "Padrão");
                                for f in &fonts {
                                    let n = Path::new(f)
                                        .file_name()
                                        .map(|s| s.to_string_lossy().to_string())
                                        .unwrap_or_default();
                                    ui.selectable_value(&mut t.font_path, f.clone(), n);
                                }
                            });
                        ui.end_row();
                        ui.label("Tamanho (% altura)");
                        ui.add(egui::Slider::new(&mut t.size, 1.0..=40.0));
                        ui.end_row();
                        ui.label("Cor do texto");
                        ui.color_edit_button_rgba_unmultiplied(&mut t.color);
                        ui.end_row();
                        ui.label("Contorno");
                        ui.horizontal(|ui| {
                            ui.add(egui::Slider::new(&mut t.outline, 0.0..=3.0));
                            ui.color_edit_button_rgba_unmultiplied(&mut t.outline_color);
                        });
                        ui.end_row();
                        ui.label("Sombra");
                        ui.checkbox(&mut t.shadow, "");
                        ui.end_row();
                        ui.label("Fundo");
                        ui.color_edit_button_rgba_unmultiplied(&mut t.background);
                        ui.end_row();
                        ui.label("Alinhamento");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut t.align, TextAlign::Left, "Esquerda");
                            ui.selectable_value(&mut t.align, TextAlign::Center, "Centro");
                            ui.selectable_value(&mut t.align, TextAlign::Right, "Direita");
                        });
                        ui.end_row();
                        ui.label("Posição X / Y");
                        ui.horizontal(|ui| {
                            ui.add(egui::Slider::new(&mut t.x, 0.0..=1.0));
                            ui.add(egui::Slider::new(&mut t.y, 0.0..=1.0));
                        });
                        ui.end_row();
                        ui.label("Espaçamento de linha");
                        ui.add(egui::Slider::new(&mut t.line_spacing, 0.6..=3.0));
                        ui.end_row();
                        ui.label("Animação");
                        egui::ComboBox::from_id_salt("anim")
                            .selected_text(t.anim.name())
                            .show_ui(ui, |ui| {
                                for a in TextAnim::ALL {
                                    ui.selectable_value(&mut t.anim, a, a.name());
                                }
                            });
                        ui.end_row();
                        ui.label("Duração da animação (s)");
                        ui.add(egui::Slider::new(&mut t.anim_duration, 0.5..=60.0));
                        ui.end_row();
                    });
                }
                GenSpec::SolidColor { color } => {
                    ui.horizontal(|ui| {
                        ui.label("Cor");
                        ui.color_edit_button_rgba_unmultiplied(color);
                    });
                }
                GenSpec::Gradient {
                    a,
                    b,
                    angle,
                    radial,
                } => {
                    ui.horizontal(|ui| {
                        ui.label("Cor inicial");
                        ui.color_edit_button_rgba_unmultiplied(a);
                        ui.label("Cor final");
                        ui.color_edit_button_rgba_unmultiplied(b);
                    });
                    ui.checkbox(radial, "Radial");
                    ui.add(egui::Slider::new(angle, -180.0..=180.0).text("Ângulo"));
                }
                GenSpec::Checkerboard { a, b, cells } => {
                    ui.horizontal(|ui| {
                        ui.color_edit_button_rgba_unmultiplied(a);
                        ui.color_edit_button_rgba_unmultiplied(b);
                    });
                    ui.add(egui::Slider::new(cells, 1..=64).text("Células (vertical)"));
                }
                GenSpec::ColorBars => {
                    ui.label("Barras de cor SMPTE (sem parâmetros).");
                }
                GenSpec::Noise { mono, animated } => {
                    ui.checkbox(mono, "Monocromático");
                    ui.checkbox(animated, "Animado");
                }
            });
        if spec != before {
            self.dlg.edit_in_progress = self.dlg.edit_in_progress && true;
            let label = spec.label();
            let m = &mut self.project.media[mi];
            m.source = MediaSource::Generated(spec);
            m.name = label.clone();
            for t in &mut self.project.tracks {
                for e in &mut t.events {
                    if e.media_id == mid {
                        e.name = label.clone();
                    }
                }
            }
            self.media.invalidate_generated(mid);
            self.thumbs.remove(&mid);
            self.dialog_change();
        }
        if !open {
            self.dlg.gen_editor = None;
        }
    }

    // -----------------------------------------------------------------
    // Renderização
    // -----------------------------------------------------------------

    fn render_window(&mut self, ctx: &egui::Context) {
        if self.dlg.render.is_none() {
            return;
        }
        let mut open = true;
        let mut start = false;
        let has_loop = self.project.loop_region.is_some();
        let pw = self.project.settings.width;
        let ph = self.project.settings.height;
        let dur = self.project.duration();
        let loop_r = self.project.loop_region;
        let fmt_dur = self.fmt_time(dur);
        let fmt_loop = loop_r.map(|(a, b)| format!("{} → {}", self.fmt_time(a), self.fmt_time(b)));
        let mut use_loop = self.dlg.render_use_loop;
        let Some(r) = self.dlg.render.as_mut() else {
            return;
        };
        egui::Window::new("Renderizar como").open(&mut open).resizable(false).show(ctx, |ui| {
            egui::Grid::new("rg").num_columns(2).show(ui, |ui| {
                ui.label("Formato");
                let before = r.format;
                egui::ComboBox::from_id_salt("rfmt").selected_text(r.format.name()).show_ui(ui, |ui| {
                    for f in RenderFormat::ALL {
                        ui.selectable_value(&mut r.format, f, f.name());
                    }
                });
                if before != r.format {
                    r.path = PathBuf::from(&r.path).with_extension(r.format.ext()).to_string_lossy().to_string();
                }
                ui.end_row();
                ui.label("Arquivo");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut r.path).desired_width(300.0));
                    if ui.button("...").clicked() {
                        let ext = r.format.ext();
                        if let Some(p) = rfd::FileDialog::new()
                            .add_filter(ext.to_uppercase(), &[ext])
                            .set_file_name(PathBuf::from(&r.path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default())
                            .save_file()
                        {
                            r.path = p.to_string_lossy().to_string();
                        }
                    }
                });
                ui.end_row();
                if r.format != RenderFormat::Wav {
                    ui.label("Resolução");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut r.width).range(16..=8192));
                        ui.label("x");
                        ui.add(egui::DragValue::new(&mut r.height).range(16..=8192));
                        if ui.small_button("Projeto").clicked() {
                            r.width = pw;
                            r.height = ph;
                        }
                        if ui.small_button("1/2").clicked() {
                            r.width = pw / 2;
                            r.height = ph / 2;
                        }
                    });
                    r.width = (r.width / 2).max(8) * 2;
                    r.height = (r.height / 2).max(8) * 2;
                    ui.end_row();
                }
                if r.format == RenderFormat::AviMjpeg {
                    ui.label("Qualidade JPEG");
                    ui.add(egui::Slider::new(&mut r.quality, 30..=100));
                    ui.end_row();
                }
                if r.format != RenderFormat::Wav {
                    ui.label("Áudio");
                    ui.checkbox(&mut r.include_audio, if r.format == RenderFormat::AviMjpeg { "Incluir (PCM 16 bits)" } else { "Gerar WAV ao lado" });
                    ui.end_row();
                }
                if r.format != RenderFormat::AviMjpeg {
                    ui.label("WAV");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut r.wav_bits, WavBits::Pcm16, "16 bits");
                        ui.selectable_value(&mut r.wav_bits, WavBits::Pcm24, "24 bits");
                        ui.selectable_value(&mut r.wav_bits, WavBits::Float32, "32 bits float");
                    });
                    ui.end_row();
                }
                ui.label("Intervalo");
                ui.vertical(|ui| {
                    ui.radio_value(&mut use_loop, false, format!("Projeto inteiro ({fmt_dur})"));
                    ui.add_enabled_ui(has_loop, |ui| {
                        ui.radio_value(&mut use_loop, true, format!("Somente a região de loop {}", fmt_loop.clone().unwrap_or_default()));
                    });
                });
                ui.end_row();
            });
            ui.separator();
            if r.format == RenderFormat::AviMjpeg {
                ui.label(egui::RichText::new("AVI Motion-JPEG é lido por VLC, ffmpeg, editores e pelo próprio Neoveg. Limite de 4 GB por arquivo.").small().weak());
            }
            ui.horizontal(|ui| {
                if ui.button("🎬 Renderizar").clicked() {
                    start = true;
                }
            });
        });
        self.dlg.render_use_loop = use_loop && has_loop;
        if start {
            let Some(r) = self.dlg.render.as_ref() else {
                return;
            };
            let (a, b) = match (self.dlg.render_use_loop, loop_r) {
                (true, Some(lr)) => lr,
                _ => (Ticks::ZERO, dur),
            };
            if b - a <= Ticks::ZERO {
                self.set_status("Nada para renderizar (projeto vazio)");
            } else if self.render_job.is_some() {
                self.set_status("Já existe uma renderização em andamento");
            } else {
                let settings = RenderSettings {
                    format: r.format,
                    path: PathBuf::from(&r.path),
                    width: r.width,
                    height: r.height,
                    start: a,
                    end: b,
                    jpeg_quality: r.quality,
                    include_audio: r.include_audio,
                    wav_bits: r.wav_bits,
                };
                self.project.sort_events();
                let progress = crate::render::spawn(
                    self.project.clone(),
                    self.media.clone(),
                    settings.clone(),
                );
                self.render_job = Some(RenderJob { progress, settings });
                open = false;
            }
        }
        if !open {
            self.dlg.render = None;
        }
    }

    fn render_progress_window(&mut self, ctx: &egui::Context) {
        if let Some(job) = &self.render_job {
            let p = &job.progress;
            let frac = p.fraction();
            let started = p
                .started
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .unwrap_or_else(std::time::Instant::now);
            let el = started.elapsed().as_secs_f64();
            let eta = if frac > 0.001 {
                el / frac as f64 - el
            } else {
                0.0
            };
            let mut cancel = false;
            egui::Window::new("Renderizando...")
                .collapsible(true)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(job.settings.path.display().to_string());
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .show_percentage()
                            .desired_width(360.0),
                    );
                    ui.label(format!(
                        "{} / {}   •   decorrido {:.0} s   •   restante ~{:.0} s",
                        p.done.load(std::sync::atomic::Ordering::Relaxed),
                        p.total.load(std::sync::atomic::Ordering::Relaxed),
                        el,
                        eta
                    ));
                    if ui.button("Cancelar").clicked() {
                        cancel = true;
                    }
                });
            if cancel {
                p.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }

    fn misc_windows(&mut self, ctx: &egui::Context) {
        if let Some(msg) = self.dlg.render_done.clone() {
            let mut close = false;
            egui::Window::new("Renderização")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(msg);
                    if ui.button("OK").clicked() {
                        close = true;
                    }
                });
            if close {
                self.dlg.render_done = None;
            }
        }
        if self.dlg.goto_open {
            let mut open = true;
            let mut go = None;
            egui::Window::new("Ir para")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Tempo (hh:mm:ss;ff, segundos ou Nf para quadros):");
                    let r = ui.text_edit_singleline(&mut self.dlg.goto_text);
                    r.request_focus();
                    if ui.button("Ir").clicked()
                        || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    {
                        let s = &self.project.settings;
                        go = parse_time(
                            &self.dlg.goto_text,
                            s.fps,
                            s.sample_rate,
                            s.effective_time_format(self.opts.time_format),
                        );
                    }
                });
            if let Some(t) = go {
                self.set_cursor(t);
                self.tl.center_on(t);
                open = false;
            }
            self.dlg.goto_open = open;
        }
        if self.dlg.shortcuts {
            self.shortcuts_window(ctx);
        }
        if self.dlg.prefs_open {
            self.prefs_window(ctx);
        }
        if self.dlg.edit_layout_open {
            self.edit_layout_window(ctx);
        }
        if self.dlg.save_layout_open {
            self.save_layout_window(ctx);
        }
        if self.dlg.about {
            let mut open = true;
            egui::Window::new("Sobre o Neoveg")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.heading("Neoveg");
                    ui.label(format!("Versão {}", env!("CARGO_PKG_VERSION")));
                    ui.label("Editor de vídeo não-linear escrito em Rust puro (eframe + wgpu, cpal, claxon, lewton, puremp3, image).");
                    ui.label("Fluxo de trabalho e atalhos baseados na semântica do VEGAS Pro.");
                    ui.separator();
                    ui.label(
                        egui::RichText::new(
                            "Formatos de vídeo: AVI (MJPEG/RGB), Y4M, GIF, sequências de imagens e MP4/MOV/MKV no Windows via Media Foundation. \
                             Áudio: WAV, AIFF, FLAC, OGG, MP3, M4A/AAC.",
                        )
                        .small(),
                    );
                });
            self.dlg.about = open;
        }
    }

    fn shortcuts_window(&mut self, ctx: &egui::Context) {
        if let Some(cmd_id) = self.dlg.capturing_shortcut {
            if let Some(captured) = capture_shortcut(ctx) {
                self.dlg.capturing_shortcut = None;
                if let Some(sc) = captured {
                    let formatted = sc.format();
                    self.prefs
                        .shortcuts
                        .insert(cmd_id.to_string(), Some(formatted.clone()));
                    if let Err(e) = self.prefs.save() {
                        self.set_status(format!("Aviso ao salvar atalho: {e}"));
                    } else {
                        self.set_status(format!("Atalho '{cmd_id}' definido para {formatted}"));
                    }
                }
            }
        }
        let mut open = true;
        egui::Window::new("Atalhos de teclado")
            .open(&mut open)
            .default_width(640.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Filtrar:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.dlg.shortcut_filter)
                            .hint_text("comando, menu ou tecla...")
                            .desired_width(240.0),
                    );
                    if ui.button("Restaurar padrões").clicked() {
                        self.prefs.shortcuts.clear();
                        self.dlg.capturing_shortcut = None;
                        let _ = self.prefs.save();
                        self.set_status("Atalhos restaurados para os padrões.");
                    }
                });
                if let Some(cap) = self.dlg.capturing_shortcut {
                    ui.colored_label(
                        self.theme.accent,
                        format!("Pressione a nova combinação para '{cap}' (Esc para cancelar)..."),
                    );
                }
                ui.separator();
                let filter = self.dlg.shortcut_filter.trim().to_ascii_lowercase();
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .show(ui, |ui| {
                        egui::Grid::new("sc_dyn")
                            .striped(true)
                            .num_columns(4)
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("Menu / Comando").strong());
                                ui.label(egui::RichText::new("Atalho ativo").strong());
                                ui.label("");
                                ui.label("");
                                ui.end_row();
                                for cmd in COMMANDS {
                                    let sc_text = resolved_shortcut(cmd, &self.prefs.shortcuts)
                                        .map(Shortcut::format)
                                        .unwrap_or_else(|| "-".into());
                                    if !filter.is_empty() {
                                        let m = cmd.menu_path.to_ascii_lowercase();
                                        let l = cmd.label.to_ascii_lowercase();
                                        let id_l = cmd.id.to_ascii_lowercase();
                                        let s = sc_text.to_ascii_lowercase();
                                        if !m.contains(&filter)
                                            && !l.contains(&filter)
                                            && !id_l.contains(&filter)
                                            && !s.contains(&filter)
                                        {
                                            continue;
                                        }
                                    }
                                    ui.label(cmd.menu_path)
                                        .on_hover_text(format!("{} ({})", cmd.label, cmd.id));
                                    ui.label(egui::RichText::new(sc_text).monospace());
                                    let waiting = self.dlg.capturing_shortcut == Some(cmd.id);
                                    if ui
                                        .small_button(if waiting {
                                            "Aguardando..."
                                        } else {
                                            "Gravar"
                                        })
                                        .clicked()
                                    {
                                        self.dlg.capturing_shortcut = Some(cmd.id);
                                    }
                                    if ui.small_button("Limpar").clicked() {
                                        self.dlg.capturing_shortcut = None;
                                        self.prefs.shortcuts.insert(cmd.id.to_string(), None);
                                        let _ = self.prefs.save();
                                    }
                                    ui.end_row();
                                }
                            });
                        ui.separator();
                        ui.label(egui::RichText::new("Gestos de mouse na timeline:").strong());
                        egui::Grid::new("sc_mouse")
                            .striped(true)
                            .num_columns(2)
                            .show(ui, |ui| {
                                for (k, d) in MOUSE_HINTS {
                                    ui.label(egui::RichText::new(*k).monospace());
                                    ui.label(*d);
                                    ui.end_row();
                                }
                            });
                    });
            });
        if !open {
            self.dlg.capturing_shortcut = None;
        }
        self.dlg.shortcuts = open;
    }

    fn prefs_window(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.dlg.prefs_draft.take() else {
            self.dlg.prefs_open = false;
            return;
        };
        let mut open = true;
        let mut apply_and_keep = false;
        let mut save_and_close = false;
        let mut cancel_close = false;
        let mut clear_cache_now = false;
        let rec_workers = recommended_worker_threads();

        egui::Window::new("Preferências...")
            .open(&mut open)
            .collapsible(false)
            .default_width(480.0)
            .show(ctx, |ui| {
                ui.heading("Aparência e Acessibilidade");
                let mut cur_theme = ThemeId::from_pref(&draft.theme);
                egui::ComboBox::from_label("Tema visual (inclui Alto Contraste AAA)")
                    .selected_text(cur_theme.label())
                    .show_ui(ui, |ui| {
                        for t in ThemeId::ALL {
                            if ui.selectable_value(&mut cur_theme, t, t.label()).clicked() {
                                draft.theme = t.id().to_string();
                                self.set_theme(ctx, t, false);
                            }
                        }
                    });
                ui.horizontal(|ui| {
                    ui.label("Idioma da interface:");
                    ui.label(egui::RichText::new(&draft.language).strong());
                });
                if ui
                    .button("Editar > Propriedades > Layout (Sonic Foundry / Sony / MAGIX)...")
                    .clicked()
                {
                    self.dlg
                        .open_edit_layout(&self.prefs, self.dock_height, self.preview_ratio);
                }

                ui.separator();
                ui.heading("Desempenho e Hardware Limitado (§3.6)");
                egui::Grid::new("prefs_hw").num_columns(2).show(ui, |ui| {
                    ui.label("Orçamento de RAM para prévia (MB):");
                    let mut ram = draft.ram_preview_mb;
                    if ui
                        .add(egui::DragValue::new(&mut ram).range(32..=4096).speed(16))
                        .changed()
                    {
                        draft.ram_preview_mb = ram;
                    }
                    ui.end_row();

                    ui.label("Máximo de quadros por mídia em cache:");
                    let mut cap = draft.frame_cache_cap;
                    if ui
                        .add(egui::DragValue::new(&mut cap).range(8..=256))
                        .changed()
                    {
                        draft.frame_cache_cap = cap;
                    }
                    ui.end_row();

                    ui.label("Threads de processamento (0 = automático):");
                    let mut wt = draft.worker_threads;
                    if ui.add(egui::DragValue::new(&mut wt).range(0..=32)).changed() {
                        draft.worker_threads = wt;
                    }
                    ui.end_row();
                });
                ui.label(
                    egui::RichText::new(format!(
                        "Recomendado para esta CPU: {rec_workers} worker(s). Alteração do pool de threads passa a valer totalmente ao reiniciar."
                    ))
                    .small()
                    .weak(),
                );
                if ui.button("Limpar cache de quadros em memória agora").clicked() {
                    clear_cache_now = true;
                }

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        save_and_close = true;
                    }
                    if ui.button("Aplicar").clicked() {
                        apply_and_keep = true;
                    }
                    if ui.button("Cancelar").clicked() {
                        cancel_close = true;
                    }
                });
            });

        if clear_cache_now {
            self.media.clear_frame_caches();
            self.thumbs.clear();
            self.set_status("Cache de quadros liberado.");
        }

        if cancel_close || (!open && !save_and_close) {
            if let Some(orig) = self.dlg.prefs_orig_theme.take() {
                let tid = ThemeId::from_pref(&orig);
                self.set_theme(ctx, tid, false);
            }
            self.dlg.prefs_open = false;
            self.dlg.prefs_draft = None;
            return;
        }

        if apply_and_keep || save_and_close {
            draft.sanitize();
            let tid = ThemeId::from_pref(&draft.theme);
            self.media
                .configure_limits(draft.frame_cache_cap, draft.ram_preview_mb);
            self.prefs.theme = draft.theme.clone();
            self.prefs.ram_preview_mb = draft.ram_preview_mb;
            self.prefs.frame_cache_cap = draft.frame_cache_cap;
            self.prefs.worker_threads = draft.worker_threads;
            self.prefs.language = draft.language.clone();
            self.set_theme(ctx, tid, true);
            if save_and_close {
                self.dlg.prefs_orig_theme = None;
                self.dlg.prefs_open = false;
                self.dlg.prefs_draft = None;
                self.set_status("Preferências salvas.");
            } else {
                self.dlg.prefs_orig_theme = Some(draft.theme.clone());
                self.dlg.prefs_draft = Some(draft);
                self.set_status("Preferências aplicadas.");
            }
        } else {
            self.dlg.prefs_draft = Some(draft);
        }
    }

    fn edit_layout_window(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.dlg.edit_layout_draft.take() else {
            self.dlg.edit_layout_open = false;
            return;
        };
        let mut open = true;
        let mut apply_type_live: Option<LayoutType> = None;
        let mut live_tweak = false;
        let mut restore_default = false;
        let mut save_and_close = false;
        let mut apply_and_keep = false;
        let mut cancel_close = false;

        egui::Window::new("Editar > Propriedades > Layout")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(600.0)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(280.0);
                        ui.heading("Tipos de layout");
                        ui.add_space(4.0);
                        for lt in LayoutType::ALL {
                            let selected = draft.layout_type == lt;
                            if ui
                                .radio(
                                    selected,
                                    egui::RichText::new(format!("{}  ({})", lt.label(), lt.era()))
                                        .strong(),
                                )
                                .clicked()
                                && !selected
                            {
                                apply_type_live = Some(lt);
                            }
                        }
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(draft.layout_type.description())
                                .small()
                                .weak(),
                        );
                    });

                    ui.separator();

                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Pré-visualização da janela").strong());
                        ui.add_space(4.0);
                        draw_layout_schematic(ui, &draft, &self.theme);
                    });
                });

                ui.separator();
                ui.heading("Opções da área de trabalho");
                if ui
                    .checkbox(
                        &mut draft.timeline_at_bottom,
                        "Exibir timeline na parte inferior da janela principal (área acoplável no topo)",
                    )
                    .changed()
                {
                    live_tweak = true;
                }
                if ui
                    .checkbox(
                        &mut draft.tabs_at_top,
                        "Posicionar abas no topo das janelas acopladas",
                    )
                    .changed()
                {
                    live_tweak = true;
                }
                if ui
                    .checkbox(
                        &mut draft.transport_at_bottom,
                        "Fixar barra de transporte no rodapé da janela",
                    )
                    .changed()
                {
                    live_tweak = true;
                }
                if ui
                    .checkbox(
                        &mut draft.preview_on_left,
                        "Posicionar Prévia de Vídeo à esquerda na área acoplável",
                    )
                    .changed()
                {
                    live_tweak = true;
                }
                if ui
                    .checkbox(
                        &mut draft.sync_theme_with_layout,
                        "Aplicar tema visual da era ao selecionar tipo de layout",
                    )
                    .changed()
                {
                    live_tweak = true;
                }
                if self.theme_id.is_high_contrast() {
                    ui.label(
                        egui::RichText::new(
                            "Tema Alto Contraste AAA ativo: a paleta de acessibilidade é preservada ao alternar tipos de layout.",
                        )
                        .small()
                        .color(self.theme.accent),
                    );
                }

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        save_and_close = true;
                    }
                    if ui.button("Aplicar").clicked() {
                        apply_and_keep = true;
                    }
                    if ui.button("Cancelar").clicked() {
                        cancel_close = true;
                    }
                    if ui
                        .button("Restaurar padrão do tipo")
                        .on_hover_text("Repõe a disposição clássica padrão do tipo selecionado")
                        .clicked()
                    {
                        restore_default = true;
                    }
                });
            });

        if cancel_close || (!open && !save_and_close) {
            if let Some((orig_el, orig_theme, orig_dock_h, orig_prev_r)) =
                self.dlg.edit_layout_orig.take()
            {
                self.prefs.edit_layout = orig_el;
                self.dock_height = orig_dock_h;
                self.preview_ratio = orig_prev_r;
                let tid = ThemeId::from_pref(&orig_theme);
                self.set_theme(ctx, tid, false);
            }
            self.dlg.edit_layout_open = false;
            self.dlg.edit_layout_draft = None;
            return;
        }

        if let Some(lt) = apply_type_live {
            draft.apply_type(lt);
            self.apply_edit_layout(Some(ctx), draft.clone(), true, false);
        } else if restore_default {
            draft.apply_type(draft.layout_type);
            self.apply_edit_layout(Some(ctx), draft.clone(), true, false);
        } else if live_tweak {
            self.apply_edit_layout(Some(ctx), draft.clone(), false, false);
        }

        if apply_and_keep || save_and_close {
            self.apply_edit_layout(Some(ctx), draft.clone(), false, true);
            if save_and_close {
                self.dlg.edit_layout_orig = None;
                self.dlg.edit_layout_open = false;
                self.dlg.edit_layout_draft = None;
                self.set_status(format!(
                    "Layout {} ({}) salvo.",
                    draft.layout_type.label(),
                    draft.layout_type.era()
                ));
            } else {
                self.dlg.edit_layout_orig = Some((
                    draft.clone(),
                    self.prefs.theme.clone(),
                    self.dock_height,
                    self.preview_ratio,
                ));
                self.dlg.edit_layout_draft = Some(draft);
                self.set_status("Propriedades de layout aplicadas.");
            }
        } else {
            self.dlg.edit_layout_draft = Some(draft);
        }
    }

    fn save_layout_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut commit = false;
        egui::Window::new("Salvar layout de janela (.neoveglayout)")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Nome do layout:");
                ui.text_edit_singleline(&mut self.dlg.save_layout_name);
                ui.horizontal(|ui| {
                    if ui.button("Salvar").clicked() {
                        commit = true;
                    }
                    if ui.button("Cancelar").clicked() {
                        self.dlg.save_layout_open = false;
                    }
                });
            });
        if commit {
            let name = self.dlg.save_layout_name.clone();
            self.save_named_layout(&name);
            open = false;
        }
        self.dlg.save_layout_open = open;
    }
}

fn capture_shortcut(ctx: &egui::Context) -> Option<Option<Shortcut>> {
    let mods = ctx.input(|i| i.modifiers);
    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        return Some(None);
    }
    const KEYS: &[(Key, ShortcutKey)] = &[
        (Key::A, ShortcutKey::A),
        (Key::B, ShortcutKey::B),
        (Key::C, ShortcutKey::C),
        (Key::D, ShortcutKey::D),
        (Key::E, ShortcutKey::E),
        (Key::F, ShortcutKey::F),
        (Key::G, ShortcutKey::G),
        (Key::H, ShortcutKey::H),
        (Key::I, ShortcutKey::I),
        (Key::J, ShortcutKey::J),
        (Key::K, ShortcutKey::K),
        (Key::L, ShortcutKey::L),
        (Key::M, ShortcutKey::M),
        (Key::N, ShortcutKey::N),
        (Key::O, ShortcutKey::O),
        (Key::P, ShortcutKey::P),
        (Key::Q, ShortcutKey::Q),
        (Key::R, ShortcutKey::R),
        (Key::S, ShortcutKey::S),
        (Key::T, ShortcutKey::T),
        (Key::U, ShortcutKey::U),
        (Key::V, ShortcutKey::V),
        (Key::W, ShortcutKey::W),
        (Key::X, ShortcutKey::X),
        (Key::Y, ShortcutKey::Y),
        (Key::Z, ShortcutKey::Z),
        (Key::Num0, ShortcutKey::Num0),
        (Key::Num1, ShortcutKey::Num1),
        (Key::Num2, ShortcutKey::Num2),
        (Key::Num3, ShortcutKey::Num3),
        (Key::Num4, ShortcutKey::Num4),
        (Key::Num5, ShortcutKey::Num5),
        (Key::Num6, ShortcutKey::Num6),
        (Key::Num7, ShortcutKey::Num7),
        (Key::Num8, ShortcutKey::Num8),
        (Key::Num9, ShortcutKey::Num9),
        (Key::Space, ShortcutKey::Space),
        (Key::Enter, ShortcutKey::Enter),
        (Key::Delete, ShortcutKey::Delete),
        (Key::F2, ShortcutKey::F2),
        (Key::F8, ShortcutKey::F8),
        (Key::ArrowLeft, ShortcutKey::ArrowLeft),
        (Key::ArrowRight, ShortcutKey::ArrowRight),
        (Key::ArrowUp, ShortcutKey::ArrowUp),
        (Key::ArrowDown, ShortcutKey::ArrowDown),
        (Key::Home, ShortcutKey::Home),
        (Key::End, ShortcutKey::End),
        (Key::OpenBracket, ShortcutKey::OpenBracket),
        (Key::CloseBracket, ShortcutKey::CloseBracket),
        (Key::Backslash, ShortcutKey::Backslash),
    ];
    for &(ek, sk) in KEYS {
        if ctx.input(|i| i.key_pressed(ek)) {
            return Some(Some(Shortcut {
                ctrl: mods.command,
                shift: mods.shift,
                alt: mods.alt,
                key: sk,
            }));
        }
    }
    None
}

const MOUSE_HINTS: &[(&str, &str)] = &[
    (
        "Roda / Shift+roda / Ctrl+roda",
        "Zoom / rolar horizontal / rolar vertical",
    ),
    ("Shift+arrastar", "Ignorar encaixe temporariamente"),
    ("Ctrl+arrastar evento", "Copiar evento"),
    (
        "Alt+arrastar evento",
        "Slip (mover a mídia dentro do evento)",
    ),
    ("Alt+arrastar borda", "Slip-trim"),
    (
        "Arrastar linha superior (áudio)",
        "Ajustar ganho do evento em dB",
    ),
    ("Duplo clique no envelope", "Adicionar ponto"),
    ("Botão do meio", "Mover a vista (pan)"),
];

fn draw_layout_schematic(
    ui: &mut egui::Ui,
    cfg: &EditLayoutPrefs,
    theme: &crate::theme::ThemeColors,
) {
    let size = vec2(270.0, 146.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, theme.extreme_bg);
    p.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, theme.border),
        StrokeKind::Outside,
    );

    let inner = rect.shrink(5.0);
    let mut top_y = inner.top();
    let mut bot_y = inner.bottom();

    let box_labeled = |r: Rect, fill: Color32, stroke: Color32, text: &str| {
        p.rect_filled(r, 2.0, fill);
        p.rect_stroke(r, 2.0, Stroke::new(1.0_f32, stroke), StrokeKind::Inside);
        p.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(10.0),
            theme.text_primary,
        );
    };

    // Menu + Toolbar no topo
    let menu_r = Rect::from_min_max(pos2(inner.left(), top_y), pos2(inner.right(), top_y + 15.0));
    top_y += 18.0;
    box_labeled(menu_r, theme.tl_header_bg, theme.border, "Menu / Toolbar");

    // Status no rodapé
    let status_r = Rect::from_min_max(pos2(inner.left(), bot_y - 13.0), pos2(inner.right(), bot_y));
    bot_y -= 16.0;
    box_labeled(
        status_r,
        theme.tl_header_bg,
        theme.border,
        "Barra de estado",
    );

    let transport_fill = theme.widget_bg;
    if cfg.transport_at_bottom {
        let tr_r = Rect::from_min_max(pos2(inner.left(), bot_y - 16.0), pos2(inner.right(), bot_y));
        bot_y -= 19.0;
        box_labeled(tr_r, transport_fill, theme.accent, "Barra de transporte");
    }

    let dock_h = ((bot_y - top_y) * 0.44).clamp(32.0, 56.0);
    let (dock_r, tl_r, tr_r_opt) = if cfg.timeline_at_bottom {
        let dr = Rect::from_min_max(
            pos2(inner.left(), top_y),
            pos2(inner.right(), top_y + dock_h),
        );
        let mut tl_top = dr.bottom() + 3.0;
        let tr_mid = if !cfg.transport_at_bottom {
            let r = Rect::from_min_max(
                pos2(inner.left(), tl_top),
                pos2(inner.right(), tl_top + 16.0),
            );
            tl_top += 19.0;
            Some(r)
        } else {
            None
        };
        let tl = Rect::from_min_max(pos2(inner.left(), tl_top), pos2(inner.right(), bot_y));
        (dr, tl, tr_mid)
    } else {
        let dr = Rect::from_min_max(
            pos2(inner.left(), bot_y - dock_h),
            pos2(inner.right(), bot_y),
        );
        let mut tl_bot = dr.top() - 3.0;
        let tr_mid = if !cfg.transport_at_bottom {
            let r = Rect::from_min_max(
                pos2(inner.left(), tl_bot - 16.0),
                pos2(inner.right(), tl_bot),
            );
            tl_bot -= 19.0;
            Some(r)
        } else {
            None
        };
        let tl = Rect::from_min_max(pos2(inner.left(), top_y), pos2(inner.right(), tl_bot));
        (dr, tl, tr_mid)
    };

    if let Some(tr_r) = tr_r_opt {
        box_labeled(tr_r, transport_fill, theme.accent, "Barra de transporte");
    }

    // Timeline (track list + faixas)
    box_labeled(
        tl_r,
        theme.tl_lane_even,
        theme.border,
        "Timeline (Track View)",
    );
    let split_x = tl_r.left() + tl_r.width() * 0.25;
    p.line_segment(
        [pos2(split_x, tl_r.top()), pos2(split_x, tl_r.bottom())],
        Stroke::new(1.0_f32, theme.border),
    );

    // Área acoplável (Painéis + Prévia + barra de abas no topo/base)
    let split = dock_r.width() * if cfg.preview_on_left { 0.40 } else { 0.60 };
    let left_sub = Rect::from_min_max(
        dock_r.min,
        pos2(dock_r.left() + split - 2.0, dock_r.bottom()),
    );
    let right_sub = Rect::from_min_max(pos2(dock_r.left() + split + 1.0, dock_r.top()), dock_r.max);
    let (tabs_sub, prev_sub) = if cfg.preview_on_left {
        (right_sub, left_sub)
    } else {
        (left_sub, right_sub)
    };
    box_labeled(tabs_sub, theme.panel_fill, theme.border, "Painéis");
    box_labeled(prev_sub, theme.tl_preview_bg, theme.accent, "Prévia");
    let strip_r = if cfg.tabs_at_top {
        Rect::from_min_max(tabs_sub.min, pos2(tabs_sub.right(), tabs_sub.top() + 6.0))
    } else {
        Rect::from_min_max(pos2(tabs_sub.left(), tabs_sub.bottom() - 6.0), tabs_sub.max)
    };
    p.rect_filled(strip_r, 1.0, theme.accent);
}
