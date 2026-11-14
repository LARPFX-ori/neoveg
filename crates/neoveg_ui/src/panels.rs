//! Área de janelas acopláveis (Window Docking Area) e janela de prévia de vídeo.

use super::timeline::draw_meter;
use super::{App, DndPayload, DockTab, FxTarget, PreviewQuality};
use crate::app::{LoadMsg, LoudnessJob};
use crate::icons::{self, Icon};
use crate::model::*;
use eframe::egui;
use egui::{pos2, vec2, Color32, Rect, Sense, Stroke, StrokeKind};
use neoveg_engine::engine::loudness::{
    analyze_project_loudness, format_db, format_lufs, LoudnessSnapshot, LoudnessTarget,
};
use neoveg_engine::util;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

struct FloatingViewportSpec<'a> {
    title: &'a str,
    inner_size: [f32; 2],
    min_size: [f32; 2],
    icon: std::sync::Arc<egui::IconData>,
    theme: &'a crate::theme::ThemeColors,
}

struct FloatingOutcome {
    open: bool,
    reattach: bool,
}

fn show_floating_viewport(
    ctx: &egui::Context,
    id_salt: impl std::hash::Hash,
    win_id_salt: impl std::hash::Hash,
    spec: &FloatingViewportSpec<'_>,
    mut body: impl FnMut(&mut egui::Ui),
) -> FloatingOutcome {
    let mut out = FloatingOutcome {
        open: true,
        reattach: false,
    };
    let vid = egui::ViewportId::from_hash_of(&id_salt);
    let builder = egui::ViewportBuilder::default()
        .with_title(format!("{} - Neoveg", spec.title))
        .with_inner_size(spec.inner_size)
        .with_min_inner_size(spec.min_size)
        .with_resizable(true)
        .with_decorations(true)
        .with_icon(spec.icon.clone());
    ctx.show_viewport_immediate(vid, builder, |v_ctx, class| {
        if class == egui::ViewportClass::Embedded {
            egui::Window::new(spec.title)
                .id(egui::Id::new(&win_id_salt))
                .open(&mut out.open)
                .default_size([spec.inner_size[0] - 40.0, spec.inner_size[1] - 40.0])
                .min_size([spec.min_size[0] - 20.0, spec.min_size[1] - 20.0])
                .show(v_ctx, |ui| {
                    ui.horizontal(|ui| {
                        if icons::small_icon_button(
                            ui,
                            Icon::DockIn,
                            Some("Acoplar na área de janelas"),
                            false,
                            spec.theme,
                        )
                        .clicked()
                        {
                            out.reattach = true;
                        }
                    });
                    ui.separator();
                    body(ui);
                });
        } else {
            if v_ctx.input(|i| i.viewport().close_requested()) {
                out.open = false;
            }
            egui::CentralPanel::default().show(v_ctx, |ui| {
                ui.horizontal(|ui| {
                    if icons::small_icon_button(
                        ui,
                        Icon::DockIn,
                        Some("Acoplar na área de janelas"),
                        false,
                        spec.theme,
                    )
                    .clicked()
                    {
                        out.reattach = true;
                    }
                    ui.label(
                        egui::RichText::new(
                            "Janela independente (arraste pela barra de título para mover entre monitores)",
                        )
                        .small()
                        .weak(),
                    );
                });
                ui.separator();
                body(ui);
            });
        }
    });
    out
}

impl App {
    pub fn detach_dock_tab(&mut self, tab: DockTab) {
        self.detached_tabs.insert(tab);
        if self.dock_tab == tab {
            if let Some(next) = DockTab::ALL
                .into_iter()
                .find(|t| !self.detached_tabs.contains(t))
            {
                self.dock_tab = next;
            }
        }
    }

    pub fn dock_area(&mut self, ui: &mut egui::Ui) {
        crate::dock::draw_dock_tree(self, ui);
    }

    pub fn floating_panel_windows(&mut self, ctx: &egui::Context) {
        let icon = self.window_icon.clone();
        let theme = self.theme.clone();

        if self.preview_detached {
            let spec = FloatingViewportSpec {
                title: "Prévia de Vídeo",
                inner_size: [560.0, 380.0],
                min_size: [280.0, 200.0],
                icon: icon.clone(),
                theme: &theme,
            };
            let out = show_floating_viewport(
                ctx,
                "neoveg_floating_preview_viewport",
                "floating_preview_window",
                &spec,
                |ui| self.preview_panel(ui, true),
            );
            if !out.open || out.reattach {
                self.preview_detached = false;
                if out.reattach {
                    self.preview_visible = true;
                    self.dock_visible = true;
                } else {
                    self.preview_visible = false;
                }
            }
        }

        if self.master_bus_detached {
            let spec = FloatingViewportSpec {
                title: "Master Bus",
                inner_size: [220.0, 420.0],
                min_size: [140.0, 260.0],
                icon: icon.clone(),
                theme: &theme,
            };
            let out = show_floating_viewport(
                ctx,
                "neoveg_floating_master_bus_viewport",
                "floating_master_bus_window",
                &spec,
                |ui| self.master_bus_panel(ui, true),
            );
            if !out.open || out.reattach {
                self.master_bus_detached = false;
                if out.reattach {
                    self.master_bus_visible = true;
                    self.dock_visible = true;
                } else {
                    self.master_bus_visible = false;
                }
            }
        }

        let detached: Vec<DockTab> = DockTab::ALL
            .into_iter()
            .filter(|t| self.detached_tabs.contains(t))
            .collect();
        for tab in detached {
            let key = crate::layout::panel_key(tab);
            let spec = FloatingViewportSpec {
                title: tab.name(),
                inner_size: [520.0, 360.0],
                min_size: [260.0, 180.0],
                icon: icon.clone(),
                theme: &theme,
            };
            let out = show_floating_viewport(
                ctx,
                ("neoveg_floating_dock_viewport", key),
                ("floating_dock", tab.command_id()),
                &spec,
                |ui| {
                    egui::ScrollArea::both()
                        .id_salt(("floating_os_scroll", tab.command_id()))
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.show_dock_tab_contents(ui, tab));
                },
            );
            if !out.open || out.reattach {
                self.detached_tabs.remove(&tab);
                if out.reattach {
                    self.dock_visible = true;
                    self.dock_tab = tab;
                    self.pending_dock_focus = Some(tab);
                }
            }
        }
    }

    pub(crate) fn show_dock_tab_contents(&mut self, ui: &mut egui::Ui, tab: DockTab) {
        match tab {
            DockTab::ProjectMedia => self.media_tab(ui),
            DockTab::Explorer => self.explorer_tab(ui),
            DockTab::Transitions => self.transitions_tab(ui),
            DockTab::VideoFx => self.fx_tab(ui, false),
            DockTab::ColorGrading => self.color_grading_tab(ui),
            DockTab::AudioFx => self.fx_tab(ui, true),
            DockTab::Generators => self.generators_tab(ui),
            DockTab::Mixer => self.mixer_tab(ui),
            DockTab::Loudness => self.loudness_tab(ui),
            DockTab::Trimmer => self.trimmer_tab(ui),
            DockTab::EditDetails => self.edit_details_tab(ui),
            DockTab::Scopes => self.scopes_tab(ui),
            DockTab::Notes => {
                ui.label("Notas do projeto:");
                if ui
                    .add(
                        egui::TextEdit::multiline(&mut self.project.notes)
                            .desired_rows(12)
                            .desired_width(f32::INFINITY),
                    )
                    .changed()
                {
                    self.dirty = true;
                }
            }
        }
    }

    pub(crate) fn preview_panel(&mut self, ui: &mut egui::Ui, is_floating: bool) {
        ui.horizontal_wrapped(|ui| {
            self.cmd_tool_btn(ui, Icon::GearProps, None, "file.project_props");
            egui::ComboBox::from_id_salt(("pq", is_floating))
                .selected_text(self.opts.preview_quality.name())
                .show_ui(ui, |ui| {
                    for q in PreviewQuality::ALL {
                        ui.selectable_value(&mut self.opts.preview_quality, q, q.name());
                    }
                });
            if icons::icon_button(
                ui,
                Icon::SafeAreas,
                None,
                self.opts.safe_areas,
                true,
                &self.theme,
            )
            .on_hover_text("Áreas de segurança")
            .clicked()
            {
                self.opts.safe_areas = !self.opts.safe_areas;
            }
            if icons::icon_button(ui, Icon::CameraSnap, None, false, true, &self.theme)
                .on_hover_text("Salvar quadro em PNG")
                .clicked()
            {
                self.snapshot_dialog();
            }
            if ui
                .button("fx")
                .on_hover_text("Efeitos de saída de vídeo")
                .clicked()
            {
                self.dlg.fx_target = Some(FxTarget::MasterVideo);
            }
            if icons::small_icon_button(
                ui,
                Icon::ColorWheels,
                Some("Cor"),
                self.is_panel_open(DockTab::ColorGrading),
                &self.theme,
            )
            .on_hover_text("Abrir painel Color Grading")
            .clicked()
            {
                self.exec_cmd(Some(ui.ctx()), "view.tab.color_grading");
            }
            if !is_floating
                && icons::small_icon_button(ui, Icon::PopOut, None, false, &self.theme)
                    .on_hover_text("Destacar Prévia de Vídeo em janela normal do Windows")
                    .clicked()
            {
                self.preview_detached = true;
            }
        });
        let avail = ui.available_rect_before_wrap();
        let font = egui::FontId::proportional(10.5);
        let gap_y = ui.spacing().item_spacing.y;
        let line_h = ui.fonts(|f| f.row_height(&font)).ceil().max(13.0);
        let show_transport = avail.height() >= 132.0;
        let transport_h = if show_transport {
            ui.spacing().interact_size.y.max(22.0)
        } else {
            0.0
        };
        let two_line_info = avail.height() >= 96.0;
        let info_h = if two_line_info {
            line_h * 2.0 + 6.0
        } else {
            line_h + 4.0
        };
        let reserved_h = info_h
            + gap_y
            + if show_transport {
                transport_h + gap_y
            } else {
                0.0
            };
        let canvas_h = (avail.height() - reserved_h).max(24.0);
        let (rect, _resp) = ui.allocate_exact_size(vec2(avail.width(), canvas_h), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, self.theme.tl_preview_bg);
        let (proj_w, proj_h) = (self.project.settings.width, self.project.settings.height);
        let aspect = proj_w as f32 / proj_h.max(1) as f32;
        let (mut w, mut h) = (rect.width(), rect.width() / aspect);
        if h > rect.height() {
            h = rect.height();
            w = h * aspect;
        }
        let img_rect = Rect::from_center_size(rect.center(), vec2(w, h));
        if let Some(tex) = &self.preview_tex {
            painter.image(
                tex.id(),
                img_rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        if self.opts.safe_areas {
            let action_k = (self.prefs.action_safe_pct as f32 / 100.0).clamp(0.70, 0.98);
            let title_k = (self.prefs.title_safe_pct as f32 / 100.0).clamp(0.50, action_k);
            for k in [action_k, title_k] {
                let r = Rect::from_center_size(img_rect.center(), img_rect.size() * k);
                painter.rect_stroke(
                    r,
                    0.0,
                    Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 90)),
                    StrokeKind::Inside,
                );
            }
            let c = img_rect.center();
            painter.line_segment(
                [c - vec2(8.0, 0.0), c + vec2(8.0, 0.0)],
                Stroke::new(1.0_f32, Color32::from_white_alpha(120)),
            );
            painter.line_segment(
                [c - vec2(0.0, 8.0), c + vec2(0.0, 8.0)],
                Stroke::new(1.0_f32, Color32::from_white_alpha(120)),
            );
        }

        if show_transport {
            let playing = self.is_playing();
            ui.allocate_ui_with_layout(
                vec2(avail.width(), transport_h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let pad_x = ((ui.available_width() - 88.0) * 0.5).max(0.0);
                    ui.add_space(pad_x);
                    self.cmd_tool_toggle(
                        ui,
                        playing,
                        if playing { Icon::Pause } else { Icon::Play },
                        None,
                        "transport.play_toggle",
                    );
                    self.cmd_tool_btn(ui, Icon::Stop, None, "transport.stop");
                    self.cmd_tool_btn(ui, Icon::PlayFromStart, None, "transport.play_from_start");
                },
            );
        }

        let s = &self.project.settings;
        let pq = self.opts.preview_quality.scale();
        let (pw, ph) = (
            ((s.width as f32 * pq).round() as u32).max(1),
            ((s.height as f32 * pq).round() as u32).max(1),
        );
        let (dw, dh) = (
            (img_rect.width().round() as u32).max(1),
            (img_rect.height().round() as u32).max(1),
        );
        let frame_no = s.fps.ticks_to_frame_round(self.view_time());
        let (info_rect, _) =
            ui.allocate_exact_size(vec2(avail.width().max(80.0), info_h), Sense::hover());
        let ip = ui.painter_at(info_rect);
        let fg = self.theme.text_muted;
        if two_line_info {
            let y1 = info_rect.top() + 1.0;
            let y2 = y1 + line_h + 2.0;
            ip.text(
                pos2(info_rect.left() + 2.0, y1),
                egui::Align2::LEFT_TOP,
                format!("Projeto: {}x{}; {}p", s.width, s.height, s.fps.label()),
                font.clone(),
                fg,
            );
            ip.text(
                pos2(info_rect.left() + 2.0, y2),
                egui::Align2::LEFT_TOP,
                format!("Prévia: {pw}x{ph}; {}p", s.fps.label()),
                font.clone(),
                fg,
            );
            ip.text(
                pos2(info_rect.right() - 2.0, y1),
                egui::Align2::RIGHT_TOP,
                format!("Quadro: {frame_no}"),
                font.clone(),
                fg,
            );
            ip.text(
                pos2(info_rect.right() - 2.0, y2),
                egui::Align2::RIGHT_TOP,
                format!("Exibição: {dw}x{dh}"),
                font,
                fg,
            );
        } else {
            ip.text(
                pos2(info_rect.left() + 2.0, info_rect.center().y),
                egui::Align2::LEFT_CENTER,
                format!(
                    "Projeto: {}x{}  Prévia: {pw}x{ph}  Quadro: {frame_no}",
                    s.width, s.height
                ),
                font,
                fg,
            );
        }
    }

    fn sync_master_monitor_audio(&self) {
        let mut s = crate::util::lock(&self.audio.shared);
        s.dim = self.master_dim;
        s.mono_downmix = self.master_mono;
    }

    fn master_bus_controls(&mut self, ui: &mut egui::Ui, tall_dock: bool) {
        let mut changed = false;
        let mut monitor_changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            if ui
                .small_button("fx")
                .on_hover_text("Efeitos do Master Bus (áudio)")
                .clicked()
            {
                self.dlg.fx_target = Some(FxTarget::MasterAudio);
            }
            if ui
                .selectable_label(self.master_mono, "Mono")
                .on_hover_text("Downmix Stereo -> Mono na monitoração")
                .clicked()
            {
                self.master_mono = !self.master_mono;
                monitor_changed = true;
            }
            if ui
                .selectable_label(self.master_dim, "Dim")
                .on_hover_text("Atenuar saída de monitoração em -20 dB (Dim Output)")
                .clicked()
            {
                self.master_dim = !self.master_dim;
                monitor_changed = true;
            }
            if ui
                .selectable_label(self.project.master.mute, "M")
                .on_hover_text("Silenciar Master Bus (Mute)")
                .clicked()
            {
                self.checkpoint();
                self.project.master.mute = !self.project.master.mute;
                changed = true;
            }
        });
        if monitor_changed {
            self.sync_master_monitor_audio();
        }
        let pk = lin_to_db(self.master_hold[0].max(self.master_hold[1]));
        let peak_str = if pk < -100.0 {
            "-inf".to_string()
        } else {
            format!("{pk:+.1}")
        };
        ui.label(
            egui::RichText::new(format!("pico: {peak_str} dB"))
                .monospace()
                .small()
                .color(if pk > -0.1 {
                    self.theme.offline_fg
                } else {
                    self.theme.text_muted
                }),
        );
        let meter_h = if tall_dock {
            (ui.available_height() - 24.0).clamp(36.0, 240.0)
        } else {
            110.0
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let slider = ui.add_sized(
                [22.0, meter_h],
                egui::Slider::new(&mut self.project.master.volume_db, -60.0..=12.0)
                    .vertical()
                    .show_value(false),
            );
            if slider.drag_started() {
                self.checkpoint();
            }
            if slider.double_clicked() {
                self.checkpoint();
                self.project.master.volume_db = 0.0;
                changed = true;
            }
            changed |= slider
                .on_hover_text("Volume do Master Bus (duplo clique para 0,0 dB)")
                .changed();
            let (mr, _) = ui.allocate_exact_size(vec2(18.0, meter_h), Sense::hover());
            draw_meter(ui.painter(), mr, self.master_hold);
            if tall_dock && meter_h >= 78.0 {
                let (sr, _) = ui.allocate_exact_size(vec2(22.0, meter_h), Sense::hover());
                let sp = ui.painter_at(sr);
                let font = egui::FontId::monospace(8.5);
                for db in [0, -6, -12, -24, -48] {
                    let n = ((db as f32 + 60.0) / 66.0).clamp(0.0, 1.0);
                    let y = sr.bottom() - n * sr.height();
                    sp.text(
                        pos2(sr.left() + 1.0, y),
                        egui::Align2::LEFT_CENTER,
                        format!("{db}"),
                        font.clone(),
                        self.theme.text_muted,
                    );
                }
            }
        });
        ui.label(
            egui::RichText::new(format!("{:+.1} dB", self.project.master.volume_db))
                .monospace()
                .small(),
        );
        if changed {
            self.changed();
        }
    }

    pub(crate) fn master_bus_panel(&mut self, ui: &mut egui::Ui, is_floating: bool) {
        ui.vertical(|ui| {
            if !is_floating {
                ui.label(egui::RichText::new("Master").strong().small());
            }
            self.master_bus_controls(ui, true);
        });
    }

    // -----------------------------------------------------------------
    // Mídia do projeto
    // -----------------------------------------------------------------

    fn media_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if icons::icon_button(
                ui,
                Icon::ImportMedia,
                Some("Importar..."),
                false,
                true,
                &self.theme,
            )
            .clicked()
            {
                self.import_dialog();
            }
            if ui.button("Sequência de imagens...").clicked() {
                self.import_sequence_dialog();
            }
            if ui.button("Remover não usadas").clicked() {
                self.remove_unused_media();
            }
        });
        ui.label(egui::RichText::new("Arraste uma mídia para a timeline, ou dê um duplo clique para adicionar no cursor.").small().weak());
        ui.separator();
        let ctx = ui.ctx().clone();
        let items: Vec<MediaItem> = self.project.media.clone();
        let mut remove = None;
        let mut add = None;
        let mut open_trimmer = None;
        let mut edit_gen = None;
        let mut build_proxy = None;
        egui::Grid::new("media_grid")
            .striped(true)
            .num_columns(3)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                for m in &items {
                    let thumb = self.thumb(&ctx, m.id);
                    let id = egui::Id::new(("media_drag", m.id));
                    let resp = ui
                        .dnd_drag_source(id, DndPayload::Media(m.id), |ui| {
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                            ui.set_min_width(220.0);
                            ui.horizontal(|ui| {
                                ui.set_min_height(36.0);
                                if let Some(t) = &thumb {
                                    let s = t.size_vec2();
                                    let h = 36.0;
                                    let w = (h * s.x / s.y.max(1.0)).clamp(24.0, 84.0);
                                    ui.image((t.id(), vec2(w, h)));
                                } else {
                                    icons::paint_inline(
                                        ui,
                                        if m.has_audio {
                                            Icon::AudioTrack
                                        } else {
                                            Icon::MediaItem
                                        },
                                        18.0,
                                        &self.theme,
                                    );
                                }
                                ui.vertical(|ui| {
                                    ui.set_min_width(140.0);
                                    ui.add(
                                        egui::Label::new(egui::RichText::new(&m.name).strong())
                                            .extend(),
                                    );
                                    let dur = m
                                        .duration
                                        .map(|d| self.fmt_time(d))
                                        .unwrap_or_else(|| "estática".into());
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(format!("{}  •  {}", dur, m.info))
                                                .small(),
                                        )
                                        .extend(),
                                    );
                                });
                            });
                        })
                        .response;
                    let resp = ui.interact(resp.rect, id.with("click"), Sense::click());
                    if resp.double_clicked() {
                        add = Some(m.id);
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Adicionar no cursor").clicked() {
                            add = Some(m.id);
                            ui.close_menu();
                        }
                        if ui.button("Abrir no Trimmer").clicked() {
                            open_trimmer = Some(m.id);
                            ui.close_menu();
                        }
                        if m.has_video
                            && !matches!(m.source, MediaSource::Generated(_))
                            && ui.button("Gerar proxy de vídeo").clicked()
                        {
                            build_proxy = Some(m.id);
                            ui.close_menu();
                        }
                        if matches!(m.source, MediaSource::Generated(_))
                            && ui.button("Editar mídia gerada...").clicked()
                        {
                            edit_gen = Some(m.id);
                            ui.close_menu();
                        }
                        if ui.button("Remover do projeto").clicked() {
                            remove = Some(m.id);
                            ui.close_menu();
                        }
                    });
                    ui.label(if m.has_video && m.has_audio {
                        "A/V"
                    } else if m.has_video {
                        "V"
                    } else {
                        "A"
                    });
                    if !self.media.contains(m.id) && !matches!(m.source, MediaSource::Generated(_))
                    {
                        ui.colored_label(self.theme.offline_fg, "offline");
                    } else if m.proxy_path.is_some() {
                        ui.label(egui::RichText::new("[proxy]").small().weak());
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
        if let Some(id) = add {
            self.checkpoint();
            let hint = self.sel_track;
            self.add_media_to_timeline(id, self.cursor, hint);
        }
        if let Some(id) = open_trimmer {
            self.open_in_trimmer(id);
        }
        if let Some(id) = build_proxy {
            self.build_proxy_for_media(id);
        }
        if let Some(id) = edit_gen {
            self.dlg.gen_editor = Some(id);
        }
        if let Some(id) = remove {
            let used = self
                .project
                .tracks
                .iter()
                .any(|t| t.events.iter().any(|e| e.media_id == id));
            self.checkpoint();
            if used {
                for t in &mut self.project.tracks {
                    t.events.retain(|e| e.media_id != id);
                }
            }
            self.project.media.retain(|m| m.id != id);
            self.media.remove(id);
            self.thumbs.remove(&id);
            self.changed();
        }
    }

    // -----------------------------------------------------------------
    // Explorer
    // -----------------------------------------------------------------

    fn explorer_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if icons::icon_button(ui, Icon::FolderUp, None, false, true, &self.theme)
                .on_hover_text("Pasta acima")
                .clicked()
            {
                if let Some(p) = self.explorer_dir.parent() {
                    self.explorer_dir = p.to_path_buf();
                }
            }
            let mut s = self.explorer_dir.to_string_lossy().to_string();
            if ui
                .add(egui::TextEdit::singleline(&mut s).desired_width(320.0))
                .lost_focus()
            {
                let p = PathBuf::from(&s);
                if p.is_dir() {
                    self.explorer_dir = p;
                }
            }
            ui.label("Filtro:");
            ui.add(egui::TextEdit::singleline(&mut self.explorer_filter).desired_width(90.0));
        });
        #[cfg(windows)]
        ui.horizontal(|ui| {
            for c in b'C'..=b'Z' {
                let d = format!("{}:\\", c as char);
                if std::path::Path::new(&d).exists() && ui.small_button(&d).clicked() {
                    self.explorer_dir = PathBuf::from(&d);
                }
            }
        });
        ui.separator();
        let mut entries: Vec<(bool, PathBuf)> = std::fs::read_dir(&self.explorer_dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| (e.path().is_dir(), e.path()))
                    .collect()
            })
            .unwrap_or_default();
        entries.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.file_name().cmp(&b.1.file_name()))
        });
        let filter = self.explorer_filter.to_lowercase();
        let mut go = None;
        let mut import = vec![];
        for (is_dir, p) in entries {
            let name = p
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if name.starts_with('.') {
                continue;
            }
            let is_proj = crate::model::is_project_file(&p);
            if !is_dir && !crate::media::is_supported(&p) && !is_proj {
                continue;
            }
            if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                continue;
            }
            let r = ui
                .horizontal(|ui| {
                    icons::paint_inline(
                        ui,
                        if is_dir {
                            Icon::FolderItem
                        } else {
                            Icon::MediaItem
                        },
                        14.0,
                        &self.theme,
                    );
                    ui.selectable_label(false, name)
                })
                .inner;
            if r.double_clicked() {
                if is_dir {
                    go = Some(p.clone());
                } else if is_proj {
                    self.open_project_path(p.clone());
                } else {
                    import.push(p.clone());
                }
            }
            if !is_dir {
                r.on_hover_text("Duplo clique: importar e adicionar no cursor");
            }
        }
        if let Some(p) = go {
            self.explorer_dir = p;
        }
        if !import.is_empty() {
            let t = self.cursor;
            self.import_paths(import, Some(t));
        }
    }

    // -----------------------------------------------------------------
    // Transições, efeitos e geradores
    // -----------------------------------------------------------------

    fn transitions_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Arraste uma transição até um evento de vídeo (ela é usada no crossfade/fade de entrada), ou selecione o evento e clique em Aplicar.").small().weak());
        ui.separator();
        let sel: Vec<Id> = self.sel_events.iter().cloned().collect();
        egui::Grid::new("tr_grid")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                for k in TransitionKind::ALL {
                    let id = egui::Id::new(("tr", k as u32));
                    ui.dnd_drag_source(id, DndPayload::Transition(k), |ui| {
                        icons::icon_label(ui, Icon::TransitionSplit, 14.0, k.name(), &self.theme);
                    });
                    if ui
                        .add_enabled(!sel.is_empty(), egui::Button::new("Aplicar"))
                        .clicked()
                    {
                        for e in &sel {
                            self.apply_transition(*e, k);
                        }
                    }
                    ui.end_row();
                }
            });
    }

    fn fx_tab(&mut self, ui: &mut egui::Ui, audio: bool) {
        ui.label(
            egui::RichText::new("Arraste um efeito até um evento ou trilha, ou use os botões para aplicar à seleção / trilha selecionada / master.")
                .small()
                .weak(),
        );
        ui.separator();
        let list: Vec<FxKind> = if audio {
            FxKind::AUDIO.to_vec()
        } else {
            FxKind::VIDEO.to_vec()
        };
        let want_kind = if audio {
            TrackKind::Audio
        } else {
            TrackKind::Video
        };
        egui::Grid::new(("fx_grid", audio))
            .num_columns(4)
            .striped(true)
            .show(ui, |ui| {
                for k in list {
                    let id = egui::Id::new(("fx", k as u32));
                    ui.dnd_drag_source(id, DndPayload::Fx(k), |ui| {
                        icons::icon_label(
                            ui,
                            if audio {
                                Icon::FxAudio
                            } else {
                                Icon::FxSparkle
                            },
                            14.0,
                            k.name(),
                            &self.theme,
                        );
                    });
                    let sel_ev: Vec<Id> = self
                        .sel_events
                        .iter()
                        .filter(|id| {
                            self.project
                                .find_event(**id)
                                .map(|(t, _)| self.project.tracks[t].kind == want_kind)
                                .unwrap_or(false)
                        })
                        .cloned()
                        .collect();
                    if ui
                        .add_enabled(!sel_ev.is_empty(), egui::Button::new("Eventos"))
                        .clicked()
                    {
                        self.checkpoint();
                        for id in &sel_ev {
                            if let Some(ev) = self.project.event_mut(*id) {
                                ev.fx.push(FxInstance::new(k));
                            }
                        }
                        self.dlg.fx_target = Some(FxTarget::Event(sel_ev[0]));
                        self.changed();
                    }
                    let tr = self
                        .sel_track
                        .filter(|&t| self.project.tracks.get(t).map(|x| x.kind) == Some(want_kind));
                    if let (true, Some(t)) = (
                        ui.add_enabled(tr.is_some(), egui::Button::new("Trilha"))
                            .clicked(),
                        tr,
                    ) {
                        self.checkpoint();
                        self.project.tracks[t].fx.push(FxInstance::new(k));
                        self.dlg.fx_target = Some(FxTarget::Track(self.project.tracks[t].id));
                        self.changed();
                    }
                    if ui.button("Master").clicked() {
                        self.checkpoint();
                        if audio {
                            self.project.master.fx.push(FxInstance::new(k));
                            self.dlg.fx_target = Some(FxTarget::MasterAudio);
                        } else {
                            self.project.master.video_fx.push(FxInstance::new(k));
                            self.dlg.fx_target = Some(FxTarget::MasterVideo);
                        }
                        self.changed();
                    }
                    ui.end_row();
                }
            });
    }

    fn generators_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new(
                "Arraste um gerador para a timeline ou clique para inserir no cursor.",
            )
            .small()
            .weak(),
        );
        ui.separator();
        let presets: Vec<(&str, GenSpec)> = vec![
            ("Texto simples", GenSpec::Text(TextSpec::default())),
            (
                "Título com contorno",
                GenSpec::Text(TextSpec {
                    text: "TÍTULO".into(),
                    size: 16.0,
                    outline: 0.6,
                    outline_color: [0.0, 0.0, 0.0, 1.0],
                    color: [1.0, 0.85, 0.2, 1.0],
                    ..TextSpec::default()
                }),
            ),
            (
                "Terço inferior (lower third)",
                GenSpec::Text(TextSpec {
                    text: "Nome Sobrenome\nCargo ou descrição".into(),
                    size: 5.5,
                    align: TextAlign::Left,
                    x: 0.3,
                    y: 0.82,
                    background: [0.0, 0.0, 0.0, 0.0],
                    ..TextSpec::default()
                }),
            ),
            (
                "Créditos rolando",
                GenSpec::Text(TextSpec {
                    text: "Direção\nFulano de Tal\n\nEdição\nNeoveg\n\nObrigado por assistir"
                        .into(),
                    size: 6.0,
                    anim: TextAnim::ScrollUp,
                    anim_duration: 10.0,
                    ..TextSpec::default()
                }),
            ),
            (
                "Letreiro (crawl)",
                GenSpec::Text(TextSpec {
                    text: "Notícia urgente: texto passando na parte inferior da tela".into(),
                    size: 5.0,
                    y: 0.9,
                    anim: TextAnim::CrawlLeft,
                    anim_duration: 8.0,
                    background: [0.0, 0.0, 0.0, 0.0],
                    ..TextSpec::default()
                }),
            ),
            (
                "Máquina de escrever",
                GenSpec::Text(TextSpec {
                    text: "Era uma vez...".into(),
                    anim: TextAnim::Typewriter,
                    anim_duration: 3.0,
                    ..TextSpec::default()
                }),
            ),
            (
                "Cor sólida (preto)",
                GenSpec::SolidColor {
                    color: [0.0, 0.0, 0.0, 1.0],
                },
            ),
            (
                "Cor sólida (branco)",
                GenSpec::SolidColor {
                    color: [1.0, 1.0, 1.0, 1.0],
                },
            ),
            (
                "Gradiente linear",
                GenSpec::Gradient {
                    a: [0.05, 0.1, 0.35, 1.0],
                    b: [0.6, 0.2, 0.5, 1.0],
                    angle: 90.0,
                    radial: false,
                },
            ),
            (
                "Gradiente radial",
                GenSpec::Gradient {
                    a: [1.0, 0.9, 0.6, 1.0],
                    b: [0.3, 0.05, 0.0, 1.0],
                    angle: 0.0,
                    radial: true,
                },
            ),
            (
                "Xadrez",
                GenSpec::Checkerboard {
                    a: [0.85, 0.85, 0.85, 1.0],
                    b: [0.35, 0.35, 0.35, 1.0],
                    cells: 9,
                },
            ),
            ("Barras de cor", GenSpec::ColorBars),
            (
                "Ruído (estático)",
                GenSpec::Noise {
                    mono: true,
                    animated: true,
                },
            ),
        ];
        egui::Grid::new("gen_grid")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                for (i, (name, spec)) in presets.into_iter().enumerate() {
                    ui.dnd_drag_source(
                        egui::Id::new(("gen", i)),
                        DndPayload::Generator(spec.clone()),
                        |ui| {
                            icons::icon_label(ui, Icon::GeneratorFrame, 14.0, name, &self.theme);
                        },
                    );
                    if ui.button("Inserir no cursor").clicked() {
                        self.insert_generated(spec);
                    }
                    ui.end_row();
                }
            });
    }

    // -----------------------------------------------------------------
    // Mixer
    // -----------------------------------------------------------------

    fn mixer_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let can_add = self.project.audio_buses.len() < crate::model::MAX_AUDIO_BUSES;
            if ui
                .add_enabled(can_add, egui::Button::new("＋ Barramento"))
                .on_hover_text("Inserir barramento auxiliar de áudio (até 26)")
                .clicked()
            {
                let ctx = ui.ctx().clone();
                self.exec_cmd(Some(&ctx), "insert.audio_bus");
            }
            ui.label(
                egui::RichText::new(format!(
                    "{}/{} barramentos",
                    self.project.audio_buses.len(),
                    crate::model::MAX_AUDIO_BUSES
                ))
                .small()
                .color(self.theme.text_muted),
            );
        });
        ui.separator();

        let bus_choices: Vec<(Id, String)> = self
            .project
            .audio_buses
            .iter()
            .map(|b| (b.id, b.name.clone()))
            .collect();
        let mut remove_bus: Option<Id> = None;

        egui::ScrollArea::horizontal()
            .id_salt("mixer_channels_scroll")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let n = self.project.tracks.len();
                    for ti in 0..n {
                        if self.project.tracks[ti].kind != TrackKind::Audio {
                            continue;
                        }
                        let tid = self.project.tracks[ti].id;
                        let lv = self.meters_hold.get(&tid).cloned().unwrap_or([0.0; 2]);
                        let mut changed = false;
                        let mut checkpoint_needed = false;
                        let mut pending_toggle_mute = false;
                        let mut pending_toggle_solo = false;
                        let mut pending_output = self.project.tracks[ti].output_bus;
                        let mut vol_reset = false;
                        let mut pan_reset = false;
                        let mut toggle_send_bus: Option<Id> = None;
                        ui.group(|ui| {
                            ui.set_width(108.0);
                            ui.vertical(|ui| {
                                let t_name = self.project.tracks[ti].name.clone();
                                let t_mute = self.project.tracks[ti].mute;
                                let t_solo = self.project.tracks[ti].solo;
                                let send_count = self.project.tracks[ti].sends.len();
                                ui.label(egui::RichText::new(&t_name).strong().small());
                                ui.horizontal(|ui| {
                                    if ui.selectable_label(t_mute, "M").clicked() {
                                        checkpoint_needed = true;
                                        pending_toggle_mute = true;
                                    }
                                    if ui.selectable_label(t_solo, "S").clicked() {
                                        checkpoint_needed = true;
                                        pending_toggle_solo = true;
                                    }
                                    if ui.small_button("FX").clicked() {
                                        self.dlg.fx_target = Some(FxTarget::Track(tid));
                                    }
                                });
                                if !bus_choices.is_empty() {
                                    let sel_lbl = pending_output
                                        .and_then(|bid| {
                                            bus_choices
                                                .iter()
                                                .find(|(id, _)| *id == bid)
                                                .map(|(_, nm)| nm.as_str())
                                        })
                                        .unwrap_or("Master");
                                    let before = pending_output;
                                    egui::ComboBox::from_id_salt(("mix_trk_bus", tid))
                                        .width(98.0)
                                        .selected_text(sel_lbl)
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                &mut pending_output,
                                                None,
                                                "Master",
                                            );
                                            for (bid, bname) in &bus_choices {
                                                ui.selectable_value(
                                                    &mut pending_output,
                                                    Some(*bid),
                                                    bname,
                                                );
                                            }
                                        });
                                    if pending_output != before {
                                        checkpoint_needed = true;
                                        changed = true;
                                    }
                                }
                                let mut vol = self.project.tracks[ti].volume_db;
                                let mut vol_new: Option<f32> = None;
                                ui.horizontal(|ui| {
                                    let r = ui.add(
                                        egui::Slider::new(&mut vol, -60.0..=12.0)
                                            .vertical()
                                            .show_value(false),
                                    );
                                    if r.drag_started() {
                                        checkpoint_needed = true;
                                    }
                                    if r.double_clicked() {
                                        checkpoint_needed = true;
                                        vol = 0.0;
                                        vol_reset = true;
                                    } else if r.changed() {
                                        vol_new = Some(vol);
                                    }
                                    let (mr, _) =
                                        ui.allocate_exact_size(vec2(12.0, 96.0), Sense::hover());
                                    draw_meter(ui.painter(), mr, lv);
                                });
                                ui.label(egui::RichText::new(format!("{vol:+.1} dB")).small());
                                let mut pan = self.project.tracks[ti].pan;
                                let mut pan_new: Option<f32> = None;
                                let pan_r = ui
                                    .add(egui::Slider::new(&mut pan, -1.0..=1.0).show_value(false));
                                if pan_r.drag_started() {
                                    checkpoint_needed = true;
                                }
                                if pan_r.double_clicked() {
                                    checkpoint_needed = true;
                                    pan = 0.0;
                                    pan_reset = true;
                                } else if pan_r.changed() {
                                    pan_new = Some(pan);
                                }
                                ui.label(egui::RichText::new(EnvKind::Pan.format(pan)).small());
                                if !bus_choices.is_empty() {
                                    let send_lbl = if send_count > 0 {
                                        format!("Send ({send_count})")
                                    } else {
                                        "Send".into()
                                    };
                                    ui.menu_button(send_lbl, |ui| {
                                        for (bid, bname) in &bus_choices {
                                            let active = self.session.project.tracks[ti]
                                                .sends
                                                .iter()
                                                .any(|s| s.bus_id == *bid);
                                            if ui.selectable_label(active, bname).clicked() {
                                                toggle_send_bus = Some(*bid);
                                            }
                                        }
                                        let sc = self.session.project.tracks[ti].sends.len();
                                        for si in 0..sc {
                                            ui.separator();
                                            let bid =
                                                self.session.project.tracks[ti].sends[si].bus_id;
                                            let bn = bus_choices
                                                .iter()
                                                .find(|(id, _)| *id == bid)
                                                .map(|(_, nm)| nm.as_str())
                                                .unwrap_or("Bus");
                                            let mut pre =
                                                self.session.project.tracks[ti].sends[si].pre_fader;
                                            let mut sdb =
                                                self.session.project.tracks[ti].sends[si].level_db;
                                            ui.horizontal(|ui| {
                                                ui.label(egui::RichText::new(bn).small());
                                                if ui.toggle_value(&mut pre, "Pré").changed() {
                                                    self.checkpoint();
                                                    self.session.project.tracks[ti].sends[si]
                                                        .pre_fader = pre;
                                                    changed = true;
                                                }
                                            });
                                            let sr = ui.add(
                                                egui::Slider::new(&mut sdb, -60.0..=12.0)
                                                    .suffix(" dB")
                                                    .fixed_decimals(1),
                                            );
                                            if sr.drag_started() {
                                                self.checkpoint();
                                            }
                                            if sr.changed() {
                                                self.session.project.tracks[ti].sends[si]
                                                    .level_db = sdb;
                                                changed = true;
                                            }
                                        }
                                    });
                                }
                                if let Some(v) = vol_new {
                                    if !checkpoint_needed {
                                        self.checkpoint();
                                    }
                                    self.session.project.tracks[ti].volume_db = v;
                                    changed = true;
                                }
                                if let Some(p) = pan_new {
                                    if !checkpoint_needed {
                                        self.checkpoint();
                                    }
                                    self.session.project.tracks[ti].pan = p;
                                    changed = true;
                                }
                            });
                        });
                        if checkpoint_needed {
                            self.checkpoint();
                        }
                        if pending_toggle_mute {
                            self.session.project.tracks[ti].mute =
                                !self.session.project.tracks[ti].mute;
                            changed = true;
                        }
                        if pending_toggle_solo {
                            self.session.project.tracks[ti].solo =
                                !self.session.project.tracks[ti].solo;
                            changed = true;
                        }
                        if self.session.project.tracks[ti].output_bus != pending_output {
                            self.session.project.tracks[ti].output_bus = pending_output;
                            changed = true;
                        }
                        if vol_reset {
                            self.session.project.tracks[ti].volume_db = 0.0;
                            changed = true;
                        }
                        if pan_reset {
                            self.session.project.tracks[ti].pan = 0.0;
                            changed = true;
                        }
                        if let Some(bid) = toggle_send_bus {
                            self.checkpoint();
                            let sends = &mut self.session.project.tracks[ti].sends;
                            if sends.iter().any(|s| s.bus_id == bid) {
                                sends.retain(|s| s.bus_id != bid);
                            } else {
                                sends.push(crate::model::AudioSend {
                                    bus_id: bid,
                                    level_db: -6.0,
                                    pre_fader: false,
                                    mute: false,
                                });
                            }
                            changed = true;
                        }
                        if changed {
                            self.changed();
                        }
                    }

                    // Barramentos auxiliares (Bus A .. Bus Z)
                    let nb = self.project.audio_buses.len();
                    for bi in 0..nb {
                        let bid = self.project.audio_buses[bi].id;
                        let lv = self.meters_hold.get(&bid).cloned().unwrap_or([0.0; 2]);
                        let valid_targets: Vec<(Id, String)> = bus_choices
                            .iter()
                            .filter(|(cand, _)| {
                                *cand != bid
                                    && self.session.project.validate_bus_route(bid, Some(*cand))
                            })
                            .cloned()
                            .collect();
                        let mut changed = false;
                        let mut checkpoint_needed = false;
                        let mut toggle_mute = false;
                        let mut toggle_solo = false;
                        let mut pending_out = self.session.project.audio_buses[bi].output_bus;
                        let mut vol_reset = false;
                        let mut pan_reset = false;
                        let mut commit_name: Option<String> = None;
                        ui.group(|ui| {
                            ui.set_width(108.0);
                            ui.vertical(|ui| {
                                let cur_name = self.session.project.audio_buses[bi].name.clone();
                                let buf_id = egui::Id::new(("mix_bus_name", bid));
                                let mut draft = ui.data_mut(|d| {
                                    d.get_temp::<String>(buf_id)
                                        .unwrap_or_else(|| cur_name.clone())
                                });
                                ui.horizontal(|ui| {
                                    let nr = ui.add(
                                        egui::TextEdit::singleline(&mut draft)
                                            .desired_width(76.0)
                                            .font(egui::TextStyle::Small),
                                    );
                                    if nr.has_focus() {
                                        ui.data_mut(|d| d.insert_temp(buf_id, draft.clone()));
                                    } else if nr.lost_focus() {
                                        commit_name = Some(draft.clone());
                                        ui.data_mut(|d| d.remove::<String>(buf_id));
                                    } else if draft != cur_name {
                                        ui.data_mut(|d| d.remove::<String>(buf_id));
                                    }
                                    if ui
                                        .small_button("×")
                                        .on_hover_text("Remover barramento")
                                        .clicked()
                                    {
                                        remove_bus = Some(bid);
                                    }
                                });
                                let b_mute = self.session.project.audio_buses[bi].mute;
                                let b_solo = self.session.project.audio_buses[bi].solo;
                                ui.horizontal(|ui| {
                                    if ui.selectable_label(b_mute, "M").clicked() {
                                        checkpoint_needed = true;
                                        toggle_mute = true;
                                    }
                                    if ui.selectable_label(b_solo, "S").clicked() {
                                        checkpoint_needed = true;
                                        toggle_solo = true;
                                    }
                                    if ui.small_button("FX").clicked() {
                                        self.dlg.fx_target = Some(FxTarget::AudioBus(bid));
                                    }
                                });
                                let sel_lbl = pending_out
                                    .and_then(|out_id| {
                                        bus_choices
                                            .iter()
                                            .find(|(id, _)| *id == out_id)
                                            .map(|(_, nm)| nm.as_str())
                                    })
                                    .unwrap_or("Master");
                                let before = pending_out;
                                egui::ComboBox::from_id_salt(("mix_bus_out", bid))
                                    .width(98.0)
                                    .selected_text(sel_lbl)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut pending_out, None, "Master");
                                        for (cand, cname) in &valid_targets {
                                            ui.selectable_value(
                                                &mut pending_out,
                                                Some(*cand),
                                                cname,
                                            );
                                        }
                                    });
                                if pending_out != before {
                                    checkpoint_needed = true;
                                    changed = true;
                                }
                                let mut vol = self.session.project.audio_buses[bi].volume_db;
                                ui.horizontal(|ui| {
                                    let r = ui.add(
                                        egui::Slider::new(&mut vol, -60.0..=12.0)
                                            .vertical()
                                            .show_value(false),
                                    );
                                    if r.drag_started() {
                                        checkpoint_needed = true;
                                    }
                                    if r.double_clicked() {
                                        checkpoint_needed = true;
                                        vol = 0.0;
                                        vol_reset = true;
                                    }
                                    if r.changed() {
                                        self.session.project.audio_buses[bi].volume_db = vol;
                                        changed = true;
                                    }
                                    let (mr, _) =
                                        ui.allocate_exact_size(vec2(12.0, 96.0), Sense::hover());
                                    draw_meter(ui.painter(), mr, lv);
                                });
                                ui.label(egui::RichText::new(format!("{vol:+.1} dB")).small());
                                let mut pan = self.session.project.audio_buses[bi].pan;
                                let pan_r = ui
                                    .add(egui::Slider::new(&mut pan, -1.0..=1.0).show_value(false));
                                if pan_r.drag_started() {
                                    checkpoint_needed = true;
                                }
                                if pan_r.double_clicked() {
                                    checkpoint_needed = true;
                                    pan = 0.0;
                                    pan_reset = true;
                                }
                                if pan_r.changed() {
                                    self.session.project.audio_buses[bi].pan = pan;
                                    changed = true;
                                }
                                ui.label(egui::RichText::new(EnvKind::Pan.format(pan)).small());
                            });
                        });
                        if let Some(new_name) = commit_name {
                            self.session.rename_audio_bus(bid, new_name);
                        }
                        if checkpoint_needed {
                            self.checkpoint();
                        }
                        if toggle_mute {
                            self.session.project.audio_buses[bi].mute =
                                !self.session.project.audio_buses[bi].mute;
                            changed = true;
                        }
                        if toggle_solo {
                            self.session.project.audio_buses[bi].solo =
                                !self.session.project.audio_buses[bi].solo;
                            changed = true;
                        }
                        if self.session.project.audio_buses[bi].output_bus != pending_out {
                            self.session.project.audio_buses[bi].output_bus = pending_out;
                            changed = true;
                        }
                        if vol_reset {
                            self.session.project.audio_buses[bi].volume_db = 0.0;
                            changed = true;
                        }
                        if pan_reset {
                            self.session.project.audio_buses[bi].pan = 0.0;
                            changed = true;
                        }
                        if changed {
                            self.changed();
                        }
                    }

                    // master
                    ui.group(|ui| {
                        ui.set_width(114.0);
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new("Master").strong());
                            self.master_bus_controls(ui, false);
                        });
                    });
                });
            });

        if let Some(bid) = remove_bus {
            self.session.remove_audio_bus(bid);
        }
    }

    // -----------------------------------------------------------------
    // Medidores de Loudness (EBU R128 / ITU-R BS.1770-4)
    // -----------------------------------------------------------------

    pub fn cancel_loudness_scan(&mut self) {
        if let Some(job) = &self.loudness_job {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn start_loudness_scan(&mut self, range: Option<(Ticks, Ticks)>) {
        if self.loudness_job.is_some() {
            return;
        }
        let proj = self.project.clone();
        let media = self.media.clone();
        let sr = self.audio.sr.max(8_000);
        let cancel = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicU64::new(0));
        let total = Arc::new(AtomicU64::new(1));
        let cancel_bg = cancel.clone();
        let done_bg = done.clone();
        let total_bg = total.clone();
        let tx = self.loader_tx.clone();
        self.loading += 1;
        self.loudness_job = Some(LoudnessJob {
            cancel,
            done,
            total,
        });
        self.set_status("Analisando loudness EBU R128 do projeto...");
        if let Err(e) = util::spawn("loudness-scan", move || {
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                analyze_project_loudness(
                    &proj,
                    &media,
                    range,
                    sr,
                    &cancel_bg,
                    Some((&done_bg, &total_bg)),
                )
            }));
            let msg = match res {
                Ok(r) => r,
                Err(_) => Err("falha interna isolada ao analisar loudness".into()),
            };
            let _ = tx.send(LoadMsg::LoudnessAnalyzed(msg));
        }) {
            self.loading = self.loading.saturating_sub(1);
            self.loudness_job = None;
            self.set_status(e);
        }
    }

    fn loudness_tab(&mut self, ui: &mut egui::Ui) {
        let target = self.loudness_target;
        let target_lufs = target.target_lufs();
        let max_tp = target.max_true_peak_dbtp();

        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Padrão:").strong().small());
            egui::ComboBox::from_id_salt("loudness_target_preset")
                .selected_text(self.loudness_target.name())
                .show_ui(ui, |ui| {
                    for preset in LoudnessTarget::ALL {
                        ui.selectable_value(&mut self.loudness_target, preset, preset.name());
                    }
                });
            if ui
                .button("Zerar")
                .on_hover_text("Zera a integração EBU R128 e os picos acumulados")
                .clicked()
            {
                self.audio.reset_loudness();
                self.loudness_live = LoudnessSnapshot::default();
            }
            if let Some(job) = &self.loudness_job {
                let done = job.done.load(Ordering::Relaxed);
                let total = job.total.load(Ordering::Relaxed).max(1);
                let frac = (done as f32 / total as f32).clamp(0.0, 1.0);
                ui.add(
                    egui::ProgressBar::new(frac)
                        .desired_width(120.0)
                        .show_percentage(),
                );
                if ui.button("Cancelar").clicked() {
                    job.cancel.store(true, Ordering::Relaxed);
                }
            } else {
                if ui
                    .button("Analisar projeto")
                    .on_hover_text("Varredura offline EBU R128 de todo o projeto")
                    .clicked()
                {
                    self.start_loudness_scan(None);
                }
                if let Some(sel) = self.time_sel {
                    if sel.1 > sel.0
                        && ui
                            .button("Analisar seleção")
                            .on_hover_text("Varredura offline apenas na seleção de tempo")
                            .clicked()
                    {
                        self.start_loudness_scan(Some(sel));
                    }
                }
            }
        });
        ui.separator();

        let snap = self.loudness_live.clone();
        let meter_col = |val: f64, limit: f64| -> Color32 {
            if !val.is_finite() {
                self.theme.text_muted
            } else if val > limit + 0.5 {
                self.theme.offline_fg
            } else if val >= limit - 1.5 {
                Color32::from_rgb(230, 180, 45)
            } else {
                self.theme.accent
            }
        };

        let draw_vert_bar =
            |ui: &mut egui::Ui, label: &str, val: f64, min_db: f64, max_db: f64, ref_db: f64| {
                ui.vertical(|ui| {
                    ui.set_width(38.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(26.0, 108.0), Sense::hover());
                    let p = ui.painter_at(rect);
                    p.rect_filled(rect, 2.0, self.theme.extreme_bg);
                    p.rect_stroke(
                        rect,
                        2.0,
                        Stroke::new(1.0_f32, self.theme.border),
                        StrokeKind::Inside,
                    );
                    let norm = |v: f64| -> f32 {
                        if !v.is_finite() {
                            0.0
                        } else {
                            ((v - min_db) / (max_db - min_db).max(1.0)).clamp(0.0, 1.0) as f32
                        }
                    };
                    let fill_n = norm(val);
                    if fill_n > 0.0 {
                        let fill_h = (rect.height() - 2.0) * fill_n;
                        let fr = Rect::from_min_max(
                            pos2(rect.left() + 2.0, rect.bottom() - 1.0 - fill_h),
                            pos2(rect.right() - 2.0, rect.bottom() - 1.0),
                        );
                        p.rect_filled(fr, 1.0, meter_col(val, ref_db));
                    }
                    let ref_y = rect.bottom() - 1.0 - (rect.height() - 2.0) * norm(ref_db);
                    p.line_segment(
                        [pos2(rect.left(), ref_y), pos2(rect.right(), ref_y)],
                        Stroke::new(1.5_f32, self.theme.text_primary),
                    );
                    ui.label(egui::RichText::new(label).strong().small());
                });
            };

        egui::ScrollArea::vertical()
            .id_salt("loudness_tab_scroll")
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    draw_vert_bar(ui, "M", snap.momentary_lufs, -50.0, 0.0, target_lufs);
                    draw_vert_bar(ui, "S", snap.short_term_lufs, -50.0, 0.0, target_lufs);
                    draw_vert_bar(ui, "I", snap.integrated_lufs, -50.0, 0.0, target_lufs);
                    draw_vert_bar(ui, "TP L", snap.true_peak_dbtp[0], -50.0, 3.0, max_tp);
                    draw_vert_bar(ui, "TP R", snap.true_peak_dbtp[1], -50.0, 3.0, max_tp);
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Tempo real (Master)").strong().small());
                        egui::Grid::new("loudness_live_grid")
                            .num_columns(2)
                            .spacing([12.0, 4.0])
                            .show(ui, |ui| {
                                ui.label("Momentâneo (M):");
                                ui.label(
                                    egui::RichText::new(format_lufs(snap.momentary_lufs))
                                        .monospace(),
                                );
                                ui.end_row();
                                ui.label("Curto prazo (S):");
                                ui.label(
                                    egui::RichText::new(format_lufs(snap.short_term_lufs))
                                        .monospace(),
                                );
                                ui.end_row();
                                ui.label("Integrado (I):");
                                let int_col = meter_col(snap.integrated_lufs, target_lufs);
                                ui.label(
                                    egui::RichText::new(format_lufs(snap.integrated_lufs))
                                        .monospace()
                                        .strong()
                                        .color(int_col),
                                );
                                ui.end_row();
                                ui.label("LRA:");
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{:.1} LU",
                                        snap.loudness_range_lu
                                    ))
                                    .monospace(),
                                );
                                ui.end_row();
                                ui.label("True Peak (L / R):");
                                let tp_max = snap.true_peak_dbtp[0].max(snap.true_peak_dbtp[1]);
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} / {}",
                                        format_db(snap.true_peak_dbtp[0], "dBTP"),
                                        format_db(snap.true_peak_dbtp[1], "dBTP"),
                                    ))
                                    .monospace()
                                    .color(meter_col(tp_max, max_tp)),
                                );
                                ui.end_row();
                                ui.label("RMS (L / R):");
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} / {}",
                                        format_db(snap.rms_dbfs[0], "dBFS"),
                                        format_db(snap.rms_dbfs[1], "dBFS"),
                                    ))
                                    .monospace(),
                                );
                                ui.end_row();
                                if snap.integrated_lufs.is_finite() {
                                    let delta = snap.integrated_lufs - target_lufs;
                                    ui.label("Desvio do alvo:");
                                    ui.label(
                                        egui::RichText::new(format!("{delta:+.1} LU"))
                                            .monospace()
                                            .color(meter_col(snap.integrated_lufs, target_lufs)),
                                    );
                                    ui.end_row();
                                }
                            });
                    });
                });

                if let Some(rep) = &self.loudness_report {
                    ui.separator();
                    ui.group(|ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "Varredura offline ({} – {})",
                                    self.fmt_time(rep.start),
                                    self.fmt_time(rep.end)
                                ))
                                .strong()
                                .small(),
                            );
                            let over_i = rep.snapshot.integrated_lufs.is_finite()
                                && rep.snapshot.integrated_lufs > target_lufs + 1.0;
                            let tp_max =
                                rep.snapshot.true_peak_dbtp[0].max(rep.snapshot.true_peak_dbtp[1]);
                            let over_tp = tp_max.is_finite() && tp_max > max_tp;
                            let (badge, col) = if over_i || over_tp {
                                ("Acima do limite", self.theme.offline_fg)
                            } else if rep.snapshot.integrated_lufs.is_finite() {
                                ("Em conformidade", self.theme.accent)
                            } else {
                                ("Silêncio", self.theme.text_muted)
                            };
                            ui.label(egui::RichText::new(badge).strong().small().color(col));
                        });
                        egui::Grid::new("loudness_report_grid")
                            .num_columns(4)
                            .spacing([12.0, 4.0])
                            .show(ui, |ui| {
                                ui.label("Integrado:");
                                ui.label(
                                    egui::RichText::new(format_lufs(rep.snapshot.integrated_lufs))
                                        .monospace(),
                                );
                                ui.label("LRA:");
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{:.1} LU",
                                        rep.snapshot.loudness_range_lu
                                    ))
                                    .monospace(),
                                );
                                ui.end_row();
                                ui.label("Máx. S:");
                                ui.label(
                                    egui::RichText::new(format_lufs(rep.max_short_term_lufs))
                                        .monospace(),
                                );
                                ui.label("Máx. M:");
                                ui.label(
                                    egui::RichText::new(format_lufs(rep.max_momentary_lufs))
                                        .monospace(),
                                );
                                ui.end_row();
                                ui.label("True Peak L:");
                                ui.label(
                                    egui::RichText::new(format_db(
                                        rep.snapshot.true_peak_dbtp[0],
                                        "dBTP",
                                    ))
                                    .monospace(),
                                );
                                ui.label("True Peak R:");
                                ui.label(
                                    egui::RichText::new(format_db(
                                        rep.snapshot.true_peak_dbtp[1],
                                        "dBTP",
                                    ))
                                    .monospace(),
                                );
                                ui.end_row();
                            });
                    });
                }
            });
    }

    // -----------------------------------------------------------------
    // Escopos
    // -----------------------------------------------------------------

    fn scopes_tab(&mut self, ui: &mut egui::Ui) {
        let Some(t) = &self.scopes_tex else {
            ui.label("Os escopos são calculados a partir do quadro da prévia...");
            return;
        };
        let names = [
            "Histograma (RGB + luma)",
            "Forma de onda (luma)",
            "Vectorscope",
            "RGB Parade",
        ];
        let avail = ui.available_width();
        let w = ((avail - 24.0) / 2.0).clamp(160.0, 420.0);
        egui::Grid::new("scopes").num_columns(2).show(ui, |ui| {
            for i in 0..4 {
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(names[i]).small());
                    let s = t[i].size_vec2();
                    let h = if i == 2 { w.min(260.0) } else { w * s.y / s.x };
                    let ww = if i == 2 { h } else { w };
                    ui.image((t[i].id(), vec2(ww, h)));
                });
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
    }
}
