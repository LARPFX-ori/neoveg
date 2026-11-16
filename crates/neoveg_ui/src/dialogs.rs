//! Janelas de diálogo: propriedades do projeto e do evento, Pan/Crop, Track
//! Motion, cadeias de efeitos, editor de mídia gerada, Renderizar Como,
//! atalhos e "sobre".

use super::{
    apply_pref_defaults, App, EditLayoutPrefs, FxTarget, LayoutType, PlaybackScrollMode,
    Preferences, PreviewQuality, RenderJob, TimelineMouseScroll,
};
use crate::icons::{self, Icon};
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
    pub template_id: String,
    pub filter: String,
    pub only_favorites: bool,
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub quality: u8,
    pub include_audio: bool,
    pub wav_bits: WavBits,
}

#[derive(Clone, Copy)]
struct RenderTemplate {
    id: &'static str,
    format: RenderFormat,
    title: &'static str,
    summary: &'static str,
    /// `None` mantém a resolução do projeto (ou `true` na tupla indica 1/2 do projeto).
    resolution: Option<(u32, u32)>,
    half_project: bool,
    quality: u8,
    include_audio: bool,
    wav_bits: WavBits,
}

const RENDER_TEMPLATES: &[RenderTemplate] = &[
    RenderTemplate {
        id: "avi.project",
        format: RenderFormat::AviMjpeg,
        title: "Igual ao projeto (Full)",
        summary: "AVI Motion-JPEG 90% + PCM 16 bits na resolução da timeline",
        resolution: None,
        half_project: false,
        quality: 90,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
    },
    RenderTemplate {
        id: "avi.1080p",
        format: RenderFormat::AviMjpeg,
        title: "Full HD 1080p (1920×1080)",
        summary: "AVI Motion-JPEG 92% • ideal para entrega Full HD em Rust puro",
        resolution: Some((1920, 1080)),
        half_project: false,
        quality: 92,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
    },
    RenderTemplate {
        id: "avi.720p",
        format: RenderFormat::AviMjpeg,
        title: "HD 720p (1280×720)",
        summary: "AVI Motion-JPEG 88% • arquivos menores e prévia rápida",
        resolution: Some((1280, 720)),
        half_project: false,
        quality: 88,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
    },
    RenderTemplate {
        id: "avi.4k",
        format: RenderFormat::AviMjpeg,
        title: "4K UHD (3840×2160)",
        summary: "AVI Motion-JPEG 94% • saída de alta resolução",
        resolution: Some((3840, 2160)),
        half_project: false,
        quality: 94,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
    },
    RenderTemplate {
        id: "avi.vertical",
        format: RenderFormat::AviMjpeg,
        title: "Vertical 9:16 (1080×1920)",
        summary: "AVI Motion-JPEG 90% para vídeos verticais",
        resolution: Some((1080, 1920)),
        half_project: false,
        quality: 90,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
    },
    RenderTemplate {
        id: "avi.draft",
        format: RenderFormat::AviMjpeg,
        title: "Rascunho leve (1/2 projeto, JPEG 75%)",
        summary: "Metade da resolução do projeto para revisão rápida",
        resolution: None,
        half_project: true,
        quality: 75,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
    },
    RenderTemplate {
        id: "png.project",
        format: RenderFormat::PngSequence,
        title: "Sequência PNG sem perdas (Full)",
        summary: "Quadros PNG RGB8 + áudio WAV ao lado na resolução do projeto",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm24,
    },
    RenderTemplate {
        id: "png.half",
        format: RenderFormat::PngSequence,
        title: "Sequência PNG reduzida (1/2)",
        summary: "Quadros PNG em metade da resolução do projeto",
        resolution: None,
        half_project: true,
        quality: 100,
        include_audio: false,
        wav_bits: WavBits::Pcm16,
    },
    RenderTemplate {
        id: "y4m.master",
        format: RenderFormat::Y4m,
        title: "YUV4MPEG2 Master não comprimido",
        summary: "Fluxo Y4M bruto interoperável com pipelines externos + WAV",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm24,
    },
    RenderTemplate {
        id: "wav.16",
        format: RenderFormat::Wav,
        title: "WAV PCM 16 bits (Estéreo)",
        summary: "Mixagem Master sem compressão padrão CD",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm16,
    },
    RenderTemplate {
        id: "wav.24",
        format: RenderFormat::Wav,
        title: "WAV PCM 24 bits (Estúdio)",
        summary: "Maior faixa dinâmica para masterização de áudio",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Pcm24,
    },
    RenderTemplate {
        id: "wav.32f",
        format: RenderFormat::Wav,
        title: "WAV IEEE Float 32 bits",
        summary: "Precisão de ponto flutuante sem ceifamento acima de 0 dB",
        resolution: None,
        half_project: false,
        quality: 100,
        include_audio: true,
        wav_bits: WavBits::Float32,
    },
];

/// Categorias exibidas na lista lateral de `Preferências...`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PrefsCategory {
    #[default]
    General,
    Appearance,
    Editing,
    VideoPreview,
    Audio,
    Memory,
    AutoSave,
    Shortcuts,
}

impl PrefsCategory {
    pub const ALL: [PrefsCategory; 8] = [
        PrefsCategory::General,
        PrefsCategory::Appearance,
        PrefsCategory::Editing,
        PrefsCategory::VideoPreview,
        PrefsCategory::Audio,
        PrefsCategory::Memory,
        PrefsCategory::AutoSave,
        PrefsCategory::Shortcuts,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PrefsCategory::General => "Geral",
            PrefsCategory::Appearance => "Aparência e Layout",
            PrefsCategory::Editing => "Edição e Timeline",
            PrefsCategory::VideoPreview => "Vídeo e Prévia",
            PrefsCategory::Audio => "Áudio",
            PrefsCategory::Memory => "Memória e Desempenho",
            PrefsCategory::AutoSave => "Salvamento Automático",
            PrefsCategory::Shortcuts => "Atalhos e Controles",
        }
    }
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
    pub prefs_tab: PrefsCategory,
    prefs_draft: Option<Preferences>,
    prefs_orig: Option<(String, EditLayoutPrefs)>,
    prefs_orig_metrics: Option<(f32, f32)>,
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
    /// Arquivos aguardando confirmação para conversão.
    pub convert_offer: Option<crate::convert_dialog::ConvertOffer>,
    /// Conversão em andamento.
    pub convert_job: Option<crate::convert_dialog::ConvertJob>,
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
        self.prefs_orig = Some((current.theme.clone(), current.edit_layout.clone()));
        self.prefs_orig_metrics = None;
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
        let (path, format, template_id, filter, only_favorites, quality, include_audio, wav_bits) =
            match &self.render {
                Some(r) => (
                    r.path.clone(),
                    r.format,
                    r.template_id.clone(),
                    r.filter.clone(),
                    r.only_favorites,
                    r.quality,
                    r.include_audio,
                    r.wav_bits,
                ),
                None => (
                    base.to_string_lossy().to_string(),
                    RenderFormat::AviMjpeg,
                    "avi.project".to_string(),
                    String::new(),
                    false,
                    90,
                    true,
                    WavBits::Pcm16,
                ),
            };
        self.render = Some(RenderDialogState {
            format,
            template_id,
            filter,
            only_favorites,
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
    pub(crate) fn dialog_change(&mut self) {
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
        self.convert_windows(ctx);
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
                    ui.label("Modo do Master (áudio)");
                    egui::ComboBox::from_id_salt("master_layout")
                        .selected_text(s.channel_layout.name())
                        .show_ui(ui, |ui| {
                            for l in crate::model::ChannelLayout::ALL {
                                changed |= ui
                                    .selectable_value(&mut s.channel_layout, l, l.name())
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
                            if icons::icon_button(
                                ui,
                                Icon::KeyframeFilled,
                                Some("Adicionar no cursor"),
                                false,
                                true,
                                &self.theme,
                            )
                            .clicked()
                            {
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
                                        let t_txt = self.fmt_time(kf.t);
                                        if icons::small_icon_button(
                                            ui,
                                            Icon::KeyframeFilled,
                                            Some(&t_txt),
                                            false,
                                            &self.theme,
                                        )
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
                            if icons::icon_button(
                                ui,
                                Icon::KeyframeFilled,
                                Some("Keyframe no cursor"),
                                false,
                                true,
                                &self.theme,
                            )
                            .clicked()
                            {
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
                                    let t_txt = self.fmt_time(*kt);
                                    if icons::small_icon_button(
                                        ui,
                                        Icon::KeyframeFilled,
                                        Some(&t_txt),
                                        false,
                                        &self.theme,
                                    )
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

    pub(crate) fn fx_chain_mut(&mut self, target: FxTarget) -> Option<&mut Vec<FxInstance>> {
        match target {
            FxTarget::Event(id) => self.project.event_mut(id).map(|e| &mut e.fx),
            FxTarget::Track(id) => self
                .project
                .tracks
                .iter_mut()
                .find(|t| t.id == id)
                .map(|t| &mut t.fx),
            FxTarget::AudioBus(id) => self.project.audio_bus_mut(id).map(|b| &mut b.fx),
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
            FxTarget::AudioBus(id) => {
                let Some(b) = self.project.audio_bus(id) else {
                    self.dlg.fx_target = None;
                    return;
                };
                (
                    format!("Efeitos do barramento — {}", b.name),
                    true,
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
                        .selected_text("+ Adicionar efeito...")
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
                                    ui.label(egui::RichText::new(fx.display_name()).strong());
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if icons::small_icon_button(
                                                ui,
                                                Icon::CloseX,
                                                None,
                                                false,
                                                &self.theme,
                                            )
                                            .on_hover_text("Remover")
                                            .clicked()
                                            {
                                                op = Some((i, "del"));
                                            }
                                            if i + 1 < n
                                                && icons::small_icon_button(
                                                    ui,
                                                    Icon::MoveDown,
                                                    None,
                                                    false,
                                                    &self.theme,
                                                )
                                                .on_hover_text("Mover para baixo")
                                                .clicked()
                                            {
                                                op = Some((i, "down"));
                                            }
                                            if i > 0
                                                && icons::small_icon_button(
                                                    ui,
                                                    Icon::MoveUp,
                                                    None,
                                                    false,
                                                    &self.theme,
                                                )
                                                .on_hover_text("Mover para cima")
                                                .clicked()
                                            {
                                                op = Some((i, "up"));
                                            }
                                            if icons::small_icon_button(
                                                ui,
                                                Icon::ResetSpin,
                                                None,
                                                false,
                                                &self.theme,
                                            )
                                            .on_hover_text("Valores padrão")
                                            .clicked()
                                            {
                                                op = Some((i, "reset"));
                                            }
                                        },
                                    );
                                });
                                if fx.kind == FxKind::Ofx {
                                    changed |= crate::plugin_manager::ofx_params_editor(
                                        ui,
                                        fx,
                                        &self.theme,
                                        local_t,
                                        tol,
                                    );
                                }
                                if fx.kind == FxKind::Vst3 {
                                    changed |= crate::plugin_manager::vst3_params_editor(
                                        ui,
                                        fx,
                                        &self.theme,
                                    );
                                }
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
                                            if icons::small_icon_button(
                                                ui,
                                                if has_key_here {
                                                    Icon::KeyframeFilled
                                                } else {
                                                    Icon::KeyframeEmpty
                                                },
                                                None,
                                                has_key_here,
                                                &self.theme,
                                            )
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
                                                if icons::small_icon_button(
                                                    ui,
                                                    Icon::Backspace,
                                                    None,
                                                    false,
                                                    &self.theme,
                                                )
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
                    // OFX/VST3: mantém o plug-in e volta só os valores ao padrão dele
                    let ofx = chain_copy[i].ofx.take().map(|mut o| {
                        o.params.clear();
                        o
                    });
                    let vst3 = chain_copy[i].vst3.take().map(|mut v| {
                        v.params.clear();
                        v.state = None;
                        v
                    });
                    chain_copy[i] = FxInstance::new(chain_copy[i].kind);
                    chain_copy[i].id = id;
                    chain_copy[i].ofx = ofx;
                    chain_copy[i].vst3 = vst3;
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
        let fps = self.project.settings.fps;
        let sr = self.project.settings.sample_rate;
        let dur = self.project.duration();
        let loop_r = self.project.loop_region;
        let fmt_dur = self.fmt_time(dur);
        let fmt_loop = loop_r.map(|(a, b)| format!("{} → {}", self.fmt_time(a), self.fmt_time(b)));
        let mut use_loop = self.dlg.render_use_loop;
        let mut toggle_fav: Option<String> = None;
        let mut apply_tmpl: Option<RenderTemplate> = None;
        let mut cancel_btn = false;
        let favs = self.prefs.render_favorites.clone();
        let (range_a, range_b) = match (use_loop && has_loop, loop_r) {
            (true, Some(lr)) => lr,
            _ => (Ticks::ZERO, dur),
        };
        let range_ticks = (range_b - range_a).max(Ticks::ZERO);
        let est_frames = fps.ticks_to_frame_round(range_ticks).max(0);
        let est_len_fmt = self.fmt_time(range_ticks);
        let Some(r) = self.dlg.render.as_mut() else {
            return;
        };

        egui::Window::new("Renderizar como")
            .open(&mut open)
            .resizable(true)
            .default_size([740.0, 440.0])
            .min_width(560.0)
            .show(ctx, |ui| {
                ui.columns(2, |cols| {
                    // Coluna esquerda: Formatos, pesquisa, favoritos e modelos (templates)
                    let left = &mut cols[0];
                    left.label(egui::RichText::new("Formatos e modelos").strong());
                    left.add_space(3.0);
                    left.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut r.filter)
                                .hint_text("Filtrar formatos ou modelos...")
                                .desired_width(175.0),
                        );
                        ui.checkbox(&mut r.only_favorites, "★ Favoritos");
                    });
                    left.separator();
                    let query = r.filter.trim().to_lowercase();
                    egui::ScrollArea::vertical()
                        .id_salt("render_templates_scroll")
                        .max_height(280.0)
                        .show(left, |ui| {
                            let mut any = false;
                            for fmt in RenderFormat::ALL {
                                let matching: Vec<&RenderTemplate> = RENDER_TEMPLATES
                                    .iter()
                                    .filter(|t| t.format == fmt)
                                    .filter(|t| {
                                        if r.only_favorites && !favs.iter().any(|f| f == t.id) {
                                            return false;
                                        }
                                        if query.is_empty() {
                                            return true;
                                        }
                                        t.title.to_lowercase().contains(&query)
                                            || t.summary.to_lowercase().contains(&query)
                                            || fmt.name().to_lowercase().contains(&query)
                                    })
                                    .collect();
                                if matching.is_empty() {
                                    continue;
                                }
                                any = true;
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} (.{})",
                                        fmt.name(),
                                        fmt.ext()
                                    ))
                                    .small()
                                    .strong()
                                    .color(self.theme.accent),
                                );
                                for t in matching {
                                    let is_fav = favs.iter().any(|f| f == t.id);
                                    let selected = r.template_id == t.id && r.format == t.format;
                                    ui.horizontal(|ui| {
                                        if ui
                                            .small_button(if is_fav { "★" } else { "☆" })
                                            .on_hover_text("Alternar favorito")
                                            .clicked()
                                        {
                                            toggle_fav = Some(t.id.to_string());
                                        }
                                        if ui.selectable_label(selected, t.title).clicked() {
                                            apply_tmpl = Some(*t);
                                        }
                                    });
                                }
                                ui.add_space(4.0);
                            }
                            if !any {
                                ui.label(
                                    egui::RichText::new(
                                        "Nenhum modelo corresponde ao filtro atual.",
                                    )
                                    .small()
                                    .weak(),
                                );
                            }
                        });
                    if let Some(active_t) = RENDER_TEMPLATES.iter().find(|t| t.id == r.template_id)
                    {
                        left.separator();
                        left.group(|ui| {
                            ui.label(egui::RichText::new(active_t.title).small().strong());
                            ui.label(egui::RichText::new(active_t.summary).small().weak());
                        });
                    }

                    // Coluna direita: Arquivo de saída, parâmetros de saída, intervalo e resumo
                    let right = &mut cols[1];
                    right.label(egui::RichText::new("Destino e configurações").strong());
                    right.add_space(3.0);

                    if let Some(tmpl) = apply_tmpl {
                        r.format = tmpl.format;
                        r.template_id = tmpl.id.to_string();
                        if tmpl.half_project {
                            r.width = ((pw / 2) / 2).max(8) * 2;
                            r.height = ((ph / 2) / 2).max(8) * 2;
                        } else if let Some((tw, th)) = tmpl.resolution {
                            r.width = tw;
                            r.height = th;
                        } else {
                            r.width = pw;
                            r.height = ph;
                        }
                        r.quality = tmpl.quality;
                        r.include_audio = tmpl.include_audio;
                        r.wav_bits = tmpl.wav_bits;
                        r.path = PathBuf::from(&r.path)
                            .with_extension(r.format.ext())
                            .to_string_lossy()
                            .to_string();
                    }

                    egui::Grid::new("rg_right").num_columns(2).show(right, |ui| {
                        ui.label("Formato");
                        let before = r.format;
                        egui::ComboBox::from_id_salt("rfmt")
                            .selected_text(r.format.name())
                            .show_ui(ui, |ui| {
                                for f in RenderFormat::ALL {
                                    ui.selectable_value(&mut r.format, f, f.name());
                                }
                            });
                        if before != r.format {
                            r.path = PathBuf::from(&r.path)
                                .with_extension(r.format.ext())
                                .to_string_lossy()
                                .to_string();
                            if let Some(first_t) =
                                RENDER_TEMPLATES.iter().find(|t| t.format == r.format)
                            {
                                r.template_id = first_t.id.to_string();
                            }
                        }
                        ui.end_row();

                        ui.label("Arquivo");
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut r.path).desired_width(240.0));
                            if ui.button("...").on_hover_text("Procurar destino").clicked() {
                                let ext = r.format.ext();
                                if let Some(p) = rfd::FileDialog::new()
                                    .add_filter(ext.to_uppercase(), &[ext])
                                    .set_file_name(
                                        PathBuf::from(&r.path)
                                            .file_name()
                                            .map(|s| s.to_string_lossy().to_string())
                                            .unwrap_or_default(),
                                    )
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
                            ui.checkbox(
                                &mut r.include_audio,
                                if r.format == RenderFormat::AviMjpeg {
                                    "Incluir (PCM 16 bits)"
                                } else {
                                    "Gerar WAV ao lado"
                                },
                            );
                            ui.end_row();
                        }
                        if r.format != RenderFormat::AviMjpeg {
                            ui.label("WAV");
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut r.wav_bits, WavBits::Pcm16, "16 bits");
                                ui.selectable_value(&mut r.wav_bits, WavBits::Pcm24, "24 bits");
                                ui.selectable_value(
                                    &mut r.wav_bits,
                                    WavBits::Float32,
                                    "32 bits float",
                                );
                            });
                            ui.end_row();
                        }
                        ui.label("Intervalo");
                        ui.vertical(|ui| {
                            ui.radio_value(
                                &mut use_loop,
                                false,
                                format!("Projeto inteiro ({fmt_dur})"),
                            );
                            ui.add_enabled_ui(has_loop, |ui| {
                                ui.radio_value(
                                    &mut use_loop,
                                    true,
                                    format!(
                                        "Somente a região de loop {}",
                                        fmt_loop.clone().unwrap_or_default()
                                    ),
                                );
                            });
                        });
                        ui.end_row();
                    });

                    right.add_space(6.0);
                    right.group(|ui| {
                        ui.label(egui::RichText::new("Resumo da saída").small().strong());
                        let video_line = if r.format == RenderFormat::Wav {
                            format!("Somente áudio • {sr} Hz")
                        } else {
                            format!(
                                "{}×{} • {} qps • {} quadro(s) • {} Hz",
                                r.width,
                                r.height,
                                fps.label(),
                                est_frames,
                                sr
                            )
                        };
                        ui.label(egui::RichText::new(video_line).small());
                        ui.label(
                            egui::RichText::new(format!("Duração selecionada: {est_len_fmt}"))
                                .small()
                                .weak(),
                        );
                        if r.format == RenderFormat::AviMjpeg {
                            ui.label(
                                egui::RichText::new(
                                    "AVI Motion-JPEG é compatível com VLC e editores externos (acima de 1 GB usa as extensões OpenDML).",
                                )
                                .small()
                                .weak(),
                            );
                        }
                    });

                    right.add_space(6.0);
                    right.horizontal(|ui| {
                        if icons::icon_button(
                            ui,
                            Icon::RenderClapper,
                            Some("Renderizar"),
                            false,
                            true,
                            &self.theme,
                        )
                        .clicked()
                        {
                            start = true;
                        }
                        if ui.button("Cancelar").clicked() {
                            cancel_btn = true;
                        }
                    });
                });
            });

        if cancel_btn {
            open = false;
        }

        if let Some(fav_id) = toggle_fav {
            if let Some(pos) = self
                .prefs
                .render_favorites
                .iter()
                .position(|x| x == &fav_id)
            {
                self.prefs.render_favorites.remove(pos);
            } else {
                self.prefs.render_favorites.push(fav_id);
            }
            if let Err(e) = self.prefs.save() {
                self.set_status(format!("Aviso ao salvar favoritos de renderização: {e}"));
            }
        }
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
            let theme = self.theme.clone();
            egui::Window::new("Renderizando...")
                .collapsible(true)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        crate::splash::spinner_inline(ui, 20.0, el, &theme);
                        ui.label(job.settings.path.display().to_string());
                    });
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
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
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
                    ui.label("Editor de vídeo não-linear escrito em Rust puro (eframe + wgpu, cpal, claxon, lewton, puremp3, fundsp, rubato, netcorehost, image).");
                    ui.label("Fluxo de trabalho e atalhos baseados na semântica do VEGAS Pro.");
                    ui.separator();
                    ui.label(
                        egui::RichText::new(
                            "Formatos de vídeo: AVI (MJPEG/RGB), Y4M, GIF, sequências de imagens e MP4/MOV/MKV/WMV no Windows via Media Foundation. \
                             Áudio: WAV/AIFF, FLAC, Ogg Vorbis, MP3 e M4A/AAC (via Media Foundation no Windows).",
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
        let mut reset_defaults = false;
        let mut clear_cache_now = false;
        let mut clear_recents_now = false;
        let mut open_shortcuts = false;
        let mut apply_defaults_to_session = false;
        let mut live_layout_preview = false;

        let screen = ctx.screen_rect().size();
        let max_w = if screen.x.is_finite() {
            (screen.x - 36.0).clamp(300.0, 860.0)
        } else {
            720.0
        };
        let max_h = if screen.y.is_finite() {
            (screen.y - 40.0).clamp(240.0, 660.0)
        } else {
            540.0
        };
        let def_w = 690.0_f32.min(max_w);
        let def_h = 500.0_f32.min(max_h);
        let extra_wrap = if def_w < 460.0 { 26.0 } else { 0.0 };
        let body_h = (def_h - 108.0 - extra_wrap).max(110.0);
        let nav_inner_h = (body_h - 10.0).max(90.0);

        self.dlg
            .prefs_orig_metrics
            .get_or_insert((self.dock_height, self.preview_ratio));
        let rec_workers = recommended_worker_threads();
        let active_sr = self.project.settings.sample_rate;
        let audio_device = self.audio.device_name.clone();
        let audio_sr = self.audio.sr;
        let audio_error = self.audio.error.clone();
        let render_favs_count = self.prefs.render_favorites.len();
        let pref_path_text = Preferences::file_path().display().to_string();
        let layout_dir_text = Preferences::layouts_dir().display().to_string();
        let theme = self.theme.clone();
        let sel_bg = theme.selection_bg;
        let sel_fg = crate::theme::contrast_on(sel_bg);
        let mut live_theme: Option<ThemeId> = None;

        egui::Window::new("Preferências...")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .constrain_to(ctx.screen_rect())
            .default_size([def_w, def_h])
            .max_size([max_w, max_h])
            .show(ctx, |ui| {
                let avail_w = ui.available_width().max(260.0);
                let side_by_side = avail_w >= 480.0;

                if side_by_side {
                    let side_w = (avail_w * 0.25).clamp(140.0, 185.0);
                    let right_w = (avail_w - side_w - 14.0).max(260.0);
                    ui.horizontal_top(|ui| {
                        // Painel esquerdo: lista de categorias em moldura escura/contrastada
                        egui::Frame::NONE
                            .fill(theme.extreme_bg)
                            .stroke(Stroke::new(1.0_f32, theme.border))
                            .corner_radius(3.0)
                            .inner_margin(egui::Margin::same(4))
                            .show(ui, |ui| {
                                ui.set_width(side_w);
                                ui.set_min_height(nav_inner_h);
                                egui::ScrollArea::vertical()
                                    .id_salt("prefs_nav_scroll")
                                    .max_height(nav_inner_h)
                                    .show(ui, |ui| {
                                        ui.with_layout(
                                            egui::Layout::top_down_justified(egui::Align::LEFT),
                                            |ui| {
                                                ui.spacing_mut().item_spacing.y = 1.0;
                                                let visuals = ui.visuals_mut();
                                                visuals.selection.bg_fill = sel_bg;
                                                visuals.selection.stroke = Stroke::NONE;
                                                visuals.widgets.hovered.bg_fill =
                                                    theme.widget_hover_bg;
                                                visuals.widgets.hovered.weak_bg_fill =
                                                    theme.widget_hover_bg;
                                                for cat in PrefsCategory::ALL {
                                                    let selected = self.dlg.prefs_tab == cat;
                                                    let fg = if selected {
                                                        sel_fg
                                                    } else {
                                                        theme.text_primary
                                                    };
                                                    let resp = ui.selectable_label(
                                                        selected,
                                                        egui::RichText::new(cat.label()).color(fg),
                                                    );
                                                    if resp.clicked() {
                                                        self.dlg.prefs_tab = cat;
                                                    }
                                                }
                                            },
                                        );
                                    });
                            });

                        ui.add_space(6.0);

                        // Painel direito: opções da categoria selecionada
                        let inner_right_w = (right_w - 14.0).max(200.0);
                        ui.vertical(|ui| {
                            ui.set_width(right_w);
                            ui.set_max_width(right_w);
                            egui::ScrollArea::vertical()
                                .id_salt(("prefs_body_scroll", self.dlg.prefs_tab as u8))
                                .max_height(body_h)
                                .show(ui, |ui| {
                                    ui.set_min_width(inner_right_w);
                                    ui.set_max_width(inner_right_w);
                                    let ctx_refs = PrefsFormContext {
                                        theme: &theme,
                                        tab: self.dlg.prefs_tab,
                                        rec_workers,
                                        active_sr,
                                        audio_device: &audio_device,
                                        audio_sr,
                                        audio_error: audio_error.as_deref(),
                                        render_favs_count,
                                        pref_path: &pref_path_text,
                                        layout_dir: &layout_dir_text,
                                        label_w: (right_w * 0.42).clamp(150.0, 225.0),
                                    };
                                    render_prefs_category(
                                        ui,
                                        &ctx_refs,
                                        &mut draft,
                                        &mut PrefsActions {
                                            live_theme: &mut live_theme,
                                            live_layout_preview: &mut live_layout_preview,
                                            clear_cache_now: &mut clear_cache_now,
                                            clear_recents_now: &mut clear_recents_now,
                                            open_shortcuts: &mut open_shortcuts,
                                            apply_defaults_to_session:
                                                &mut apply_defaults_to_session,
                                        },
                                    );
                                });
                        });
                    });
                } else {
                    egui::ScrollArea::horizontal()
                        .id_salt("prefs_nav_compact")
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for cat in PrefsCategory::ALL {
                                    let selected = self.dlg.prefs_tab == cat;
                                    if ui.selectable_label(selected, cat.label()).clicked() {
                                        self.dlg.prefs_tab = cat;
                                    }
                                }
                            });
                        });
                    ui.separator();
                    let compact_h = (body_h - 34.0).max(80.0);
                    egui::ScrollArea::vertical()
                        .id_salt(("prefs_body_scroll_compact", self.dlg.prefs_tab as u8))
                        .max_height(compact_h)
                        .show(ui, |ui| {
                            let ctx_refs = PrefsFormContext {
                                theme: &theme,
                                tab: self.dlg.prefs_tab,
                                rec_workers,
                                active_sr,
                                audio_device: &audio_device,
                                audio_sr,
                                audio_error: audio_error.as_deref(),
                                render_favs_count,
                                pref_path: &pref_path_text,
                                layout_dir: &layout_dir_text,
                                label_w: 0.0,
                            };
                            render_prefs_category(
                                ui,
                                &ctx_refs,
                                &mut draft,
                                &mut PrefsActions {
                                    live_theme: &mut live_theme,
                                    live_layout_preview: &mut live_layout_preview,
                                    clear_cache_now: &mut clear_cache_now,
                                    clear_recents_now: &mut clear_recents_now,
                                    open_shortcuts: &mut open_shortcuts,
                                    apply_defaults_to_session: &mut apply_defaults_to_session,
                                },
                            );
                        });
                }

                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button("Restaurar padrões")
                        .on_hover_text(
                            "Repõe as preferências padrão (preserva projetos recentes e favoritos)",
                        )
                        .clicked()
                    {
                        reset_defaults = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Cancelar").clicked() {
                            cancel_close = true;
                        }
                        if ui.button("Aplicar").clicked() {
                            apply_and_keep = true;
                        }
                        if ui.button("OK").clicked() {
                            save_and_close = true;
                        }
                    });
                });
            });

        if let Some(tid) = live_theme {
            self.set_theme(ctx, tid, false);
        }
        if live_layout_preview {
            self.apply_edit_layout(Some(ctx), draft.edit_layout.clone(), true, false);
        }

        if reset_defaults {
            let keep_recents = draft.recent_projects.clone();
            let keep_favs = draft.render_favorites.clone();
            draft = Preferences {
                recent_projects: keep_recents,
                render_favorites: keep_favs,
                ..Default::default()
            };
            let tid = ThemeId::from_pref(&draft.theme);
            self.set_theme(ctx, tid, false);
            self.apply_edit_layout(Some(ctx), draft.edit_layout.clone(), true, false);
            self.set_status("Rascunho restaurado para as preferências padrão.");
        }

        if clear_cache_now {
            self.media.clear_frame_caches();
            self.thumbs.clear();
            self.set_status("Cache de quadros e miniaturas liberado.");
        }

        if clear_recents_now {
            draft.clear_recent_projects();
            self.prefs.clear_recent_projects();
            if let Err(e) = self.prefs.save() {
                self.set_status(format!("Aviso ao limpar projetos recentes: {e}"));
            } else {
                self.set_status("Histórico de projetos recentes limpo.");
            }
        }

        if open_shortcuts {
            self.dlg.shortcuts = true;
        }

        if cancel_close || (!open && !save_and_close) {
            if let Some((orig_theme, orig_layout)) = self.dlg.prefs_orig.take() {
                let tid = ThemeId::from_pref(&orig_theme);
                self.set_theme(ctx, tid, false);
                self.apply_edit_layout(Some(ctx), orig_layout, false, false);
            }
            if let Some((orig_dock_h, orig_prev_r)) = self.dlg.prefs_orig_metrics.take() {
                self.dock_height = orig_dock_h;
                self.preview_ratio = orig_prev_r;
            }
            self.dlg.prefs_open = false;
            self.dlg.prefs_draft = None;
            return;
        }

        if apply_defaults_to_session {
            apply_pref_defaults(&mut self.session.opts, &draft);
            self.set_status("Opções padrão aplicadas à sessão aberta.");
        }

        if apply_and_keep || save_and_close {
            draft.sanitize();
            self.apply_saved_prefs(ctx, &draft);
            if save_and_close {
                self.dlg.prefs_orig = None;
                self.dlg.prefs_orig_metrics = None;
                self.dlg.prefs_open = false;
                self.dlg.prefs_draft = None;
                self.set_status("Preferências salvas.");
            } else {
                self.dlg.prefs_orig = Some((draft.theme.clone(), draft.edit_layout.clone()));
                self.dlg.prefs_orig_metrics = Some((self.dock_height, self.preview_ratio));
                self.dlg.prefs_draft = Some(draft);
                self.set_status("Preferências aplicadas.");
            }
        } else {
            self.dlg.prefs_draft = Some(draft);
        }
    }

    fn apply_saved_prefs(&mut self, ctx: &egui::Context, draft: &Preferences) {
        let prev = self.prefs.clone();
        self.media
            .configure_limits(draft.frame_cache_cap, draft.ram_preview_mb);

        let opts = &mut self.session.opts;
        if draft.default_snapping != prev.default_snapping {
            opts.snapping = draft.default_snapping;
        }
        if draft.default_quantize != prev.default_quantize {
            opts.quantize = draft.default_quantize;
        }
        if draft.default_show_waveforms != prev.default_show_waveforms {
            opts.show_waveforms = draft.default_show_waveforms;
        }
        if draft.default_show_thumbs != prev.default_show_thumbs {
            opts.show_thumbs = draft.default_show_thumbs;
        }
        if draft.default_show_envelopes != prev.default_show_envelopes {
            opts.show_envelopes = draft.default_show_envelopes;
        }
        if draft.default_loop_playback != prev.default_loop_playback {
            opts.loop_playback = draft.default_loop_playback;
        }
        if draft.default_preview_quality != prev.default_preview_quality {
            opts.preview_quality = draft.default_preview_quality;
        }
        if draft.default_time_format != prev.default_time_format {
            opts.time_format = draft.default_time_format;
        }

        let next_theme = ThemeId::from_pref(&draft.theme);
        if draft.edit_layout != prev.edit_layout {
            let layout_type_changed = draft.edit_layout.layout_type != prev.edit_layout.layout_type;
            self.apply_edit_layout(
                Some(ctx),
                draft.edit_layout.clone(),
                layout_type_changed,
                false,
            );
        }
        if next_theme != self.theme_id {
            self.set_theme(ctx, next_theme, false);
        }

        let mut next = draft.clone();
        next.recent_projects = prev.recent_projects;
        next.render_favorites = prev.render_favorites;
        next.sanitize();
        self.prefs = next;
        if let Err(e) = self.prefs.save() {
            self.set_status(format!("Aviso ao salvar preferências: {e}"));
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

        let screen = ctx.screen_rect().size();
        let max_w = if screen.x.is_finite() {
            (screen.x - 24.0).clamp(260.0, 760.0)
        } else {
            580.0
        };
        let max_h = if screen.y.is_finite() {
            (screen.y - 36.0).clamp(220.0, 620.0)
        } else {
            460.0
        };
        let def_w = 560.0_f32.min(max_w);
        let def_h = 460.0_f32.min(max_h);
        let extra_wrap = if def_w < 440.0 { 26.0 } else { 0.0 };
        let scroll_max_h = (def_h - 96.0 - extra_wrap).max(90.0);

        egui::Window::new("Editar > Propriedades > Layout")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .constrain_to(ctx.screen_rect())
            .default_size([def_w, def_h])
            .max_size([max_w, max_h])
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("edit_layout_scroll")
                    .max_height(scroll_max_h)
                    .show(ui, |ui| {
                        let avail_w = ui.available_width().max(220.0);
                        let wide = avail_w >= 460.0;
                        let mut render_types = |ui: &mut egui::Ui| {
                            ui.heading("Tipos de layout");
                            ui.add_space(4.0);
                            for lt in LayoutType::ALL {
                                let selected = draft.layout_type == lt;
                                if ui
                                    .radio(
                                        selected,
                                        egui::RichText::new(format!(
                                            "{}  ({})",
                                            lt.label(),
                                            lt.era()
                                        ))
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
                                egui::RichText::new(draft.layout_type.description()).small().weak(),
                            );
                        };
                        if wide {
                            let left_w = (avail_w * 0.52).clamp(210.0, 290.0);
                            let schem_w = (avail_w - left_w - 22.0).clamp(170.0, 270.0);
                            ui.horizontal_top(|ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(left_w);
                                    render_types(ui);
                                });
                                ui.separator();
                                ui.vertical(|ui| {
                                    ui.label(
                                        egui::RichText::new("Pré-visualização da janela").strong(),
                                    );
                                    ui.add_space(4.0);
                                    draw_layout_schematic(ui, &draft, &self.theme, schem_w);
                                });
                            });
                        } else {
                            render_types(ui);
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("Pré-visualização da janela").strong());
                            ui.add_space(4.0);
                            draw_layout_schematic(ui, &draft, &self.theme, avail_w);
                        }

                        ui.separator();
                        ui.heading("Opções da área de trabalho");
                        live_tweak |= edit_layout_checkboxes(ui, &mut draft, None);
                        if self.theme_id.is_high_contrast() {
                            ui.label(
                                egui::RichText::new(
                                    "Tema Alto Contraste AAA ativo: a paleta de acessibilidade é preservada ao alternar tipos de layout.",
                                )
                                .small()
                                .color(self.theme.accent),
                            );
                        }
                    });

                ui.separator();
                ui.horizontal_wrapped(|ui| {
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
                self.dock_tree = crate::dock::build_dock_tree(
                    self.preview_ratio,
                    self.dock_tab,
                    self.prefs.edit_layout.preview_on_left,
                );
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
    max_width: f32,
) {
    let w = max_width.clamp(170.0, 270.0);
    let h = (w * 0.54).clamp(112.0, 146.0);
    let size = vec2(w, h);
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
    let bar_h = (h * 0.10).clamp(11.0, 15.0);
    let tr_h = (h * 0.11).clamp(12.0, 16.0);
    let font_sz = (w / 27.0).clamp(8.0, 10.0);

    let box_labeled = |r: Rect, fill: Color32, stroke: Color32, text: &str| {
        p.rect_filled(r, 2.0, fill);
        p.rect_stroke(r, 2.0, Stroke::new(1.0_f32, stroke), StrokeKind::Inside);
        p.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(font_sz),
            crate::theme::contrast_on(fill),
        );
    };

    // Menu + Toolbar no topo
    let menu_r = Rect::from_min_max(
        pos2(inner.left(), top_y),
        pos2(inner.right(), top_y + bar_h),
    );
    top_y += bar_h + 2.5;
    box_labeled(menu_r, theme.tl_header_bg, theme.border, "Menu / Toolbar");

    // Status no rodapé
    let status_r = Rect::from_min_max(
        pos2(inner.left(), bot_y - (bar_h - 1.5)),
        pos2(inner.right(), bot_y),
    );
    bot_y -= bar_h + 1.0;
    box_labeled(
        status_r,
        theme.tl_header_bg,
        theme.border,
        "Barra de estado",
    );

    let transport_fill = theme.widget_bg;
    if cfg.transport_at_bottom {
        let tr_r = Rect::from_min_max(pos2(inner.left(), bot_y - tr_h), pos2(inner.right(), bot_y));
        bot_y -= tr_h + 2.5;
        box_labeled(tr_r, transport_fill, theme.accent, "Barra de transporte");
    }

    let dock_h = ((bot_y - top_y) * 0.44).clamp(24.0, 56.0);
    let (dock_r, tl_r, tr_r_opt) = if cfg.timeline_at_bottom {
        let dr = Rect::from_min_max(
            pos2(inner.left(), top_y),
            pos2(inner.right(), top_y + dock_h),
        );
        let mut tl_top = dr.bottom() + 2.5;
        let tr_mid = if !cfg.transport_at_bottom {
            let r = Rect::from_min_max(
                pos2(inner.left(), tl_top),
                pos2(inner.right(), tl_top + tr_h),
            );
            tl_top += tr_h + 2.5;
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
        let mut tl_bot = dr.top() - 2.5;
        let tr_mid = if !cfg.transport_at_bottom {
            let r = Rect::from_min_max(
                pos2(inner.left(), tl_bot - tr_h),
                pos2(inner.right(), tl_bot),
            );
            tl_bot -= tr_h + 2.5;
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

    // Área acoplável (Painéis + Prévia + Master Bus opcional + barra de abas no topo/base)
    let master_w = if cfg.master_bus_docked {
        (dock_r.width() * 0.14).clamp(26.0, 38.0)
    } else {
        0.0
    };
    let main_dock_w =
        (dock_r.width() - if master_w > 0.0 { master_w + 2.0 } else { 0.0 }).max(40.0);
    let split = main_dock_w * if cfg.preview_on_left { 0.42 } else { 0.58 };
    let left_sub = Rect::from_min_max(
        dock_r.min,
        pos2(dock_r.left() + split - 2.0, dock_r.bottom()),
    );
    let right_sub = Rect::from_min_max(
        pos2(dock_r.left() + split + 1.0, dock_r.top()),
        pos2(dock_r.left() + main_dock_w, dock_r.bottom()),
    );
    let (tabs_sub, prev_sub) = if cfg.preview_on_left {
        (right_sub, left_sub)
    } else {
        (left_sub, right_sub)
    };
    box_labeled(tabs_sub, theme.panel_fill, theme.border, "Painéis");
    box_labeled(prev_sub, theme.tl_preview_bg, theme.accent, "Prévia");
    let strip_span = if master_w > 0.0 {
        let master_sub =
            Rect::from_min_max(pos2(dock_r.right() - master_w, dock_r.top()), dock_r.max);
        box_labeled(master_sub, theme.panel_fill, theme.border, "Master");
        dock_r
    } else {
        tabs_sub
    };
    let strip_r = if cfg.tabs_at_top {
        Rect::from_min_max(
            strip_span.min,
            pos2(strip_span.right(), strip_span.top() + 5.0),
        )
    } else {
        Rect::from_min_max(
            pos2(strip_span.left(), strip_span.bottom() - 5.0),
            strip_span.max,
        )
    };
    p.rect_filled(strip_r, 1.0, theme.accent);
}

fn edit_layout_checkboxes(
    ui: &mut egui::Ui,
    cfg: &mut EditLayoutPrefs,
    sync_theme: Option<&mut String>,
) -> bool {
    let mut changed = false;
    changed |= ui
        .checkbox(
            &mut cfg.timeline_at_bottom,
            "Exibir timeline na parte inferior da janela principal (área acoplável no topo)",
        )
        .changed();
    changed |= ui
        .checkbox(
            &mut cfg.tabs_at_top,
            "Posicionar abas no topo das janelas acopladas",
        )
        .changed();
    changed |= ui
        .checkbox(
            &mut cfg.transport_at_bottom,
            "Fixar barra de transporte no rodapé da janela",
        )
        .changed();
    changed |= ui
        .checkbox(
            &mut cfg.preview_on_left,
            "Posicionar Prévia de Vídeo à esquerda na área acoplável",
        )
        .changed();
    changed |= ui
        .checkbox(
            &mut cfg.master_bus_docked,
            "Exibir Master Bus acoplado à direita (padrão VEGAS Pro 21)",
        )
        .changed();
    if ui
        .checkbox(
            &mut cfg.sync_theme_with_layout,
            "Aplicar tema visual da era ao selecionar tipo de layout",
        )
        .changed()
    {
        if cfg.sync_theme_with_layout {
            if let Some(theme_pref) = sync_theme {
                if !ThemeId::from_pref(theme_pref).is_high_contrast() {
                    *theme_pref = cfg.layout_type.default_theme_id().to_string();
                }
            }
        }
        changed = true;
    }
    changed
}

struct PrefsFormContext<'a> {
    theme: &'a crate::theme::ThemeColors,
    tab: PrefsCategory,
    rec_workers: usize,
    active_sr: u32,
    audio_device: &'a str,
    audio_sr: u32,
    audio_error: Option<&'a str>,
    render_favs_count: usize,
    pref_path: &'a str,
    layout_dir: &'a str,
    label_w: f32,
}

struct PrefsActions<'a> {
    live_theme: &'a mut Option<ThemeId>,
    live_layout_preview: &'a mut bool,
    clear_cache_now: &'a mut bool,
    clear_recents_now: &'a mut bool,
    open_shortcuts: &'a mut bool,
    apply_defaults_to_session: &'a mut bool,
}

fn inner_label_w(label_w: f32) -> f32 {
    if label_w > 1.0 {
        (label_w - 11.0).max(110.0)
    } else {
        0.0
    }
}

fn prefs_row(
    ui: &mut egui::Ui,
    label_w: f32,
    label: &str,
    add_control: impl FnOnce(&mut egui::Ui),
) {
    if label_w > 1.0 {
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                vec2(label_w, 20.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.add(egui::Label::new(label).wrap());
                },
            );
            add_control(ui);
        });
    } else {
        ui.vertical(|ui| {
            ui.add(egui::Label::new(label).wrap());
            ui.horizontal(|ui| {
                add_control(ui);
            });
        });
    }
}

fn prefs_ro_row(
    ui: &mut egui::Ui,
    theme: &crate::theme::ThemeColors,
    label_w: f32,
    label: &str,
    value: &str,
    monospace: bool,
) {
    prefs_row(ui, label_w, label, |ui| {
        let mut rt = egui::RichText::new(value).color(theme.text_muted);
        if monospace {
            rt = rt.monospace().size(11.5);
            ui.add(egui::Label::new(rt).truncate()).on_hover_text(value);
        } else {
            ui.add(egui::Label::new(rt).wrap());
        }
    });
}

fn prefs_indented(ui: &mut egui::Ui, label_w: f32, add_contents: impl FnOnce(&mut egui::Ui)) {
    if label_w > 1.0 {
        let indent = label_w + ui.spacing().item_spacing.x;
        ui.horizontal(|ui| {
            ui.add_space(indent);
            ui.vertical(|ui| add_contents(ui));
        });
    } else {
        add_contents(ui);
    }
}

fn prefs_group_box(
    ui: &mut egui::Ui,
    theme: &crate::theme::ThemeColors,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    ui.add_space(4.0);
    egui::Frame::NONE
        .fill(theme.panel_fill)
        .stroke(Stroke::new(1.0_f32, theme.border))
        .corner_radius(2.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                egui::RichText::new(title)
                    .strong()
                    .color(theme.text_primary),
            );
            ui.add_space(4.0);
            add_contents(ui);
        });
    ui.add_space(3.0);
}

fn render_prefs_category(
    ui: &mut egui::Ui,
    cx: &PrefsFormContext<'_>,
    draft: &mut Preferences,
    act: &mut PrefsActions<'_>,
) {
    ui.spacing_mut().item_spacing.y = 4.0;
    match cx.tab {
        PrefsCategory::General => prefs_general(ui, cx, draft, act),
        PrefsCategory::Appearance => prefs_appearance(ui, cx, draft, act),
        PrefsCategory::Editing => prefs_editing(ui, cx, draft, act),
        PrefsCategory::VideoPreview => prefs_video_preview(ui, cx, draft, act),
        PrefsCategory::Audio => prefs_audio(ui, cx, draft, act),
        PrefsCategory::Memory => prefs_memory(ui, cx, draft, act),
        PrefsCategory::AutoSave => prefs_autosave(ui, cx, draft),
        PrefsCategory::Shortcuts => prefs_shortcuts(ui, cx, draft, act),
    }
}

fn prefs_general(
    ui: &mut egui::Ui,
    cx: &PrefsFormContext<'_>,
    draft: &mut Preferences,
    act: &mut PrefsActions<'_>,
) {
    prefs_row(
        ui,
        cx.label_w,
        "Duração padrão de imagem estática:",
        |ui| {
            ui.add_sized(
                [74.0, 20.0],
                egui::DragValue::new(&mut draft.still_image_secs)
                    .range(0.5..=60.0)
                    .speed(0.1)
                    .max_decimals(2),
            );
            ui.label("segundos");
        },
    );

    prefs_row(
        ui,
        cx.label_w,
        "Rolagem automática na reprodução:",
        |ui| {
            egui::ComboBox::from_id_salt("prefs_playback_scroll")
                .selected_text(draft.playback_scroll.label())
                .show_ui(ui, |ui| {
                    for mode in PlaybackScrollMode::ALL {
                        ui.selectable_value(&mut draft.playback_scroll, mode, mode.label());
                    }
                });
        },
    );

    prefs_row(ui, cx.label_w, "Roda do mouse na timeline:", |ui| {
        egui::ComboBox::from_id_salt("prefs_mouse_scroll")
            .selected_text(draft.timeline_mouse_scroll.label())
            .show_ui(ui, |ui| {
                for mode in TimelineMouseScroll::ALL {
                    ui.selectable_value(&mut draft.timeline_mouse_scroll, mode, mode.label());
                }
            });
    });

    ui.add_space(2.0);
    prefs_indented(ui, cx.label_w, |ui| {
        ui.checkbox(
            &mut draft.show_splash,
            "Exibir tela de abertura (Splash) ao iniciar",
        );
        ui.checkbox(
            &mut draft.show_welcome,
            "Exibir tela de boas-vindas com projetos recentes ao iniciar",
        );
        ui.checkbox(
            &mut draft.always_draw_marker_lines,
            "Desenhar sempre as linhas de marcadores na timeline",
        );
        ui.checkbox(
            &mut draft.show_tooltips,
            "Exibir dicas flutuantes (Tool Tips) nos controles",
        );
    });

    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(ui, cx.theme, "Histórico e Projetos Recentes", |ui| {
        prefs_row(ui, inner_lw, "Máximo de projetos recentes:", |ui| {
            ui.add_sized(
                [64.0, 20.0],
                egui::DragValue::new(&mut draft.max_recent_projects).range(1..=20),
            );
            ui.label("itens  (1–20)");
        });
        prefs_indented(ui, inner_lw, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Limpar projetos recentes").clicked() {
                    *act.clear_recents_now = true;
                }
                ui.label(
                    egui::RichText::new(format!(
                        "{} projeto(s) no histórico • {} modelo(s) favorito(s)",
                        draft.recent_projects.len(),
                        cx.render_favs_count
                    ))
                    .color(cx.theme.text_muted),
                );
            });
        });
    });

    prefs_group_box(ui, cx.theme, "Idioma e Arquivos de Configuração", |ui| {
        let lang_label = format!("Português (Brasil) [{}]", draft.language);
        prefs_ro_row(ui, cx.theme, inner_lw, "Idioma ativo:", &lang_label, false);
        prefs_ro_row(
            ui,
            cx.theme,
            inner_lw,
            "Arquivo de preferências:",
            cx.pref_path,
            true,
        );
        prefs_ro_row(
            ui,
            cx.theme,
            inner_lw,
            "Pasta de layouts (.neoveglayout):",
            cx.layout_dir,
            true,
        );
    });
}

fn prefs_appearance(
    ui: &mut egui::Ui,
    cx: &PrefsFormContext<'_>,
    draft: &mut Preferences,
    act: &mut PrefsActions<'_>,
) {
    let mut active_theme = ThemeId::from_pref(&draft.theme);
    prefs_row(ui, cx.label_w, "Tema visual:", |ui| {
        egui::ComboBox::from_id_salt("prefs_theme_combo")
            .selected_text(active_theme.label())
            .show_ui(ui, |ui| {
                for t in ThemeId::ALL {
                    if ui
                        .selectable_value(&mut active_theme, t, t.label())
                        .changed()
                    {
                        draft.theme = t.id().to_string();
                        *act.live_theme = Some(t);
                    }
                }
            });
    });

    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(
        ui,
        cx.theme,
        "Disposição Clássica da Área de Trabalho (Layout)",
        |ui| {
            let cur_lt = draft.edit_layout.layout_type;
            prefs_row(ui, inner_lw, "Tipo de layout:", |ui| {
                egui::ComboBox::from_id_salt("prefs_layout_type_combo")
                    .selected_text(format!("{} ({})", cur_lt.label(), cur_lt.era()))
                    .show_ui(ui, |ui| {
                        for lt in LayoutType::ALL {
                            if ui
                                .selectable_label(
                                    cur_lt == lt,
                                    format!("{} — {}", lt.label(), lt.era()),
                                )
                                .clicked()
                            {
                                draft.edit_layout.apply_type(lt);
                                if draft.edit_layout.sync_theme_with_layout
                                    && !ThemeId::from_pref(&draft.theme).is_high_contrast()
                                {
                                    draft.theme = lt.default_theme_id().to_string();
                                }
                                *act.live_layout_preview = true;
                            }
                        }
                    });
            });

            prefs_indented(ui, inner_lw, |ui| {
                if edit_layout_checkboxes(ui, &mut draft.edit_layout, Some(&mut draft.theme)) {
                    *act.live_layout_preview = true;
                }
            });
        },
    );
}

fn prefs_editing(
    ui: &mut egui::Ui,
    cx: &PrefsFormContext<'_>,
    draft: &mut Preferences,
    act: &mut PrefsActions<'_>,
) {
    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(
        ui,
        cx.theme,
        "Opções Padrão da Timeline (Novos Projetos)",
        |ui| {
            prefs_row(ui, inner_lw, "Formato padrão de tempo:", |ui| {
                egui::ComboBox::from_id_salt("prefs_default_time_format")
                    .selected_text(draft.default_time_format.name())
                    .show_ui(ui, |ui| {
                        for tf in TimeFormat::ALL {
                            ui.selectable_value(&mut draft.default_time_format, tf, tf.name());
                        }
                    });
            });

            ui.add_space(2.0);
            prefs_indented(ui, inner_lw, |ui| {
                ui.checkbox(
                    &mut draft.default_snapping,
                    "Ativar encaixe magnético (Snapping) por padrão",
                );
                ui.checkbox(
                    &mut draft.default_quantize,
                    "Quantizar edições nos limites exatos de quadros",
                );
                ui.checkbox(
                    &mut draft.default_show_waveforms,
                    "Exibir formas de onda (waveforms) nos eventos de áudio",
                );
                ui.checkbox(
                    &mut draft.default_show_thumbs,
                    "Exibir miniaturas nos eventos de vídeo",
                );
                ui.checkbox(
                    &mut draft.default_show_envelopes,
                    "Exibir curvas de envelope nas faixas",
                );
                ui.checkbox(
                    &mut draft.default_loop_playback,
                    "Ativar reprodução contínua em loop por padrão",
                );
                ui.add_space(3.0);
                if ui
                    .button("Aplicar padrões à sessão aberta")
                    .on_hover_text("Atualiza imediatamente a timeline atual com os padrões acima")
                    .clicked()
                {
                    *act.apply_defaults_to_session = true;
                }
            });
        },
    );

    prefs_group_box(ui, cx.theme, "Comportamento de Edição e Mídia", |ui| {
        ui.add(
            egui::Label::new(
                egui::RichText::new(
                    "Ao importar o primeiro arquivo de mídia, o formato do projeto é ajustado \
                     automaticamente somente se as propriedades do projeto ainda não foram \
                     personalizadas.",
                )
                .color(cx.theme.text_muted),
            )
            .wrap(),
        );
    });
}

fn prefs_video_preview(
    ui: &mut egui::Ui,
    cx: &PrefsFormContext<'_>,
    draft: &mut Preferences,
    act: &mut PrefsActions<'_>,
) {
    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(ui, cx.theme, "Prévia de Vídeo Escalonável", |ui| {
        prefs_row(ui, inner_lw, "Qualidade inicial da Prévia:", |ui| {
            egui::ComboBox::from_id_salt("prefs_default_preview_quality")
                .selected_text(draft.default_preview_quality.name())
                .show_ui(ui, |ui| {
                    for pq in PreviewQuality::ALL {
                        ui.selectable_value(&mut draft.default_preview_quality, pq, pq.name());
                    }
                });
        });
        prefs_indented(ui, inner_lw, |ui| {
            if ui.button("Aplicar qualidade à sessão aberta").clicked() {
                *act.apply_defaults_to_session = true;
            }
        });
    });

    prefs_group_box(
        ui,
        cx.theme,
        "Guias de Área Segura na Prévia (Safe Areas)",
        |ui| {
            prefs_row(ui, inner_lw, "Área segura de ação (Action Safe):", |ui| {
                if ui
                    .add_sized(
                        [64.0, 20.0],
                        egui::DragValue::new(&mut draft.action_safe_pct).range(70..=98),
                    )
                    .changed()
                    && draft.title_safe_pct > draft.action_safe_pct
                {
                    draft.title_safe_pct = draft.action_safe_pct;
                }
                ui.label("%  (70–98%)");
            });
            prefs_row(ui, inner_lw, "Área segura de título (Title Safe):", |ui| {
                let max_title = draft.action_safe_pct.max(60);
                ui.add_sized(
                    [64.0, 20.0],
                    egui::DragValue::new(&mut draft.title_safe_pct).range(60..=max_title),
                );
                ui.label(format!("%  (60–{max_title}%)"));
            });
            prefs_indented(ui, inner_lw, |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(
                            "As guias são desenhadas na Prévia de Vídeo quando o botão \
                         \"Áreas seguras\" está ativado.",
                        )
                        .color(cx.theme.text_muted),
                    )
                    .wrap(),
                );
            });
        },
    );
}

fn prefs_audio(
    ui: &mut egui::Ui,
    cx: &PrefsFormContext<'_>,
    draft: &mut Preferences,
    _act: &mut PrefsActions<'_>,
) {
    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(
        ui,
        cx.theme,
        "Configurações de Áudio de Novos Projetos",
        |ui| {
            let sr_label = match draft.default_sample_rate {
                44100 => "44100 Hz (44,1 kHz — CD)",
                96000 => "96000 Hz (96 kHz — Estúdio)",
                _ => "48000 Hz (48 kHz — Vídeo)",
            };
            prefs_row(ui, inner_lw, "Taxa de amostragem padrão:", |ui| {
                egui::ComboBox::from_id_salt("prefs_default_sr")
                    .selected_text(sr_label)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut draft.default_sample_rate,
                            44100,
                            "44100 Hz (44,1 kHz — CD)",
                        );
                        ui.selectable_value(
                            &mut draft.default_sample_rate,
                            48000,
                            "48000 Hz (48 kHz — Vídeo)",
                        );
                        ui.selectable_value(
                            &mut draft.default_sample_rate,
                            96000,
                            "96000 Hz (96 kHz — Estúdio)",
                        );
                    });
            });
            prefs_ro_row(
                ui,
                cx.theme,
                inner_lw,
                "Taxa do projeto aberto:",
                &format!("{} Hz  (Arquivo > Propriedades)", cx.active_sr),
                false,
            );
        },
    );

    prefs_group_box(
        ui,
        cx.theme,
        "Dispositivo de Saída e Motor (cpal / fundsp / rubato)",
        |ui| {
            prefs_ro_row(
                ui,
                cx.theme,
                inner_lw,
                "Dispositivo ativo:",
                cx.audio_device,
                false,
            );
            prefs_ro_row(
                ui,
                cx.theme,
                inner_lw,
                "Taxa do hardware de saída:",
                &format!("{} Hz", cx.audio_sr),
                false,
            );
            if let Some(err) = cx.audio_error {
                prefs_ro_row(ui, cx.theme, inner_lw, "Estado da saída:", err, false);
            }
        },
    );
}

fn prefs_memory(
    ui: &mut egui::Ui,
    cx: &PrefsFormContext<'_>,
    draft: &mut Preferences,
    act: &mut PrefsActions<'_>,
) {
    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(ui, cx.theme, "Limites de RAM, Cache e Threads", |ui| {
        prefs_row(ui, inner_lw, "Prévia dinâmica em RAM máx.:", |ui| {
            ui.add_sized(
                [72.0, 20.0],
                egui::DragValue::new(&mut draft.ram_preview_mb)
                    .range(32..=8192)
                    .speed(16),
            );
            ui.label("MB  (32–8192 MB)");
        });

        prefs_row(ui, inner_lw, "Máximo de quadros em cache:", |ui| {
            ui.add_sized(
                [72.0, 20.0],
                egui::DragValue::new(&mut draft.frame_cache_cap).range(4..=512),
            );
            ui.label("quadros  (4–512)");
        });

        prefs_row(ui, inner_lw, "Threads de trabalho:", |ui| {
            ui.add_sized(
                [64.0, 20.0],
                egui::DragValue::new(&mut draft.worker_threads).range(0..=32),
            );
            let desc = if draft.worker_threads == 0 {
                format!("automático ({} rec. nesta CPU)", cx.rec_workers)
            } else {
                "manuais (0 = auto)".to_string()
            };
            ui.add(egui::Label::new(desc).wrap());
        });

        prefs_indented(ui, inner_lw, |ui| {
            if ui.button("Limpar cache e miniaturas agora").clicked() {
                *act.clear_cache_now = true;
            }
        });
    });

    prefs_group_box(
        ui,
        cx.theme,
        "Adaptabilidade a Hardware Limitado (§3.6)",
        |ui| {
            ui.add(
            egui::Label::new(
                egui::RichText::new(
                    "Em máquinas com 2 núcleos ou 4 GB de RAM, manter threads em 0 e Prévia RAM \
                     moderada evita pressão de memória; mídia é lida em streaming.",
                )
                .color(cx.theme.text_muted),
            )
            .wrap(),
        );
        },
    );
}

fn prefs_autosave(ui: &mut egui::Ui, cx: &PrefsFormContext<'_>, draft: &mut Preferences) {
    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(
        ui,
        cx.theme,
        "Salvamento Automático e Recuperação",
        |ui| {
            prefs_indented(ui, inner_lw, |ui| {
                ui.checkbox(
                    &mut draft.autosave_enabled,
                    "Ativar salvamento automático periódico do projeto",
                );
            });

            ui.add_enabled_ui(draft.autosave_enabled, |ui| {
                prefs_row(ui, inner_lw, "Intervalo de gravação:", |ui| {
                    ui.add_sized(
                        [72.0, 20.0],
                        egui::DragValue::new(&mut draft.autosave_secs)
                            .range(15..=600)
                            .speed(5),
                    );
                    ui.label("segundos  (15–600 s)");
                });
            });

            let summary = if draft.autosave_enabled {
                let mins = draft.autosave_secs / 60;
                let secs = draft.autosave_secs % 60;
                if mins > 0 && secs > 0 {
                    format!("ativo — a cada {mins} min {secs} s")
                } else if mins > 0 {
                    format!("ativo — a cada {mins} min")
                } else {
                    format!("ativo — a cada {secs} s")
                }
            } else {
                "desativado".to_string()
            };
            prefs_ro_row(ui, cx.theme, inner_lw, "Estado atual:", &summary, false);
            prefs_ro_row(
                ui,
                cx.theme,
                inner_lw,
                "Arquivo de configuração:",
                cx.pref_path,
                true,
            );
        },
    );
}

fn prefs_shortcuts(
    ui: &mut egui::Ui,
    cx: &PrefsFormContext<'_>,
    draft: &Preferences,
    act: &mut PrefsActions<'_>,
) {
    let inner_lw = inner_label_w(cx.label_w);
    prefs_group_box(ui, cx.theme, "Atalhos de Teclado e Comandos", |ui| {
        let stats = format!(
            "{} comando(s) na Tabela Única • {} atalho(s) personalizado(s)",
            COMMANDS.len(),
            draft.shortcuts.len()
        );
        prefs_ro_row(ui, cx.theme, inner_lw, "Resumo de atalhos:", &stats, false);
        prefs_indented(ui, inner_lw, |ui| {
            if ui.button("Configurar atalhos de teclado...").clicked() {
                *act.open_shortcuts = true;
            }
        });
    });

    prefs_group_box(
        ui,
        cx.theme,
        "Mapeamento Clássico do VEGAS Pro Preservado",
        |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(
                        "S = Dividir • M = Marcador • R = Região • J/K/L = Scrub • \
                     G/U = Agrupar/Desagrupar • Z/X = Mudo/Solo • Ctrl+L = Auto Ripple • \
                     Alt+[ / Alt+] = Aparo • Ctrl+T = Aparar à seleção",
                    )
                    .color(cx.theme.text_muted),
                )
                .wrap(),
            );
        },
    );
}
