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
            let (inner_size, min_size) = match tab {
                DockTab::Encoder => ([1120.0, 680.0], [740.0, 440.0]),
                _ => ([520.0, 360.0], [260.0, 180.0]),
            };
            let spec = FloatingViewportSpec {
                title: tab.name(),
                inner_size,
                min_size,
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

        if self.external_monitor {
            let vid = egui::ViewportId::from_hash_of("neoveg_external_monitor_viewport");
            let builder = egui::ViewportBuilder::default()
                .with_title("Monitor Externo (Prévia) - Neoveg")
                .with_inner_size([960.0, 560.0])
                .with_min_inner_size([320.0, 200.0])
                .with_resizable(true)
                .with_icon(icon);
            let mut close_ext = false;
            ctx.show_viewport_immediate(vid, builder, |v_ctx, class| {
                let draw_body = |ui: &mut egui::Ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Monitor Externo").strong().small());
                        if ui
                            .selectable_label(self.external_monitor_fullscreen, "Tela cheia")
                            .clicked()
                        {
                            self.external_monitor_fullscreen = !self.external_monitor_fullscreen;
                            v_ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(
                                self.external_monitor_fullscreen,
                            ));
                        }
                        if ui.small_button("Fechar").clicked() {
                            close_ext = true;
                        }
                    });
                    let avail_ext = ui.available_rect_before_wrap();
                    let ext_sz = vec2(
                        avail_ext.width().clamp(64.0, 4096.0),
                        avail_ext.height().clamp(48.0, 2160.0),
                    );
                    let (rect, _) = ui.allocate_exact_size(ext_sz, Sense::hover());
                    let _ = self.draw_preview_canvas(ui, rect, false);
                };
                if class == egui::ViewportClass::Embedded {
                    let mut open_win = true;
                    egui::Window::new("Monitor Externo")
                        .id(egui::Id::new("external_monitor_embedded_window"))
                        .open(&mut open_win)
                        .default_size([640.0, 380.0])
                        .show(v_ctx, draw_body);
                    if !open_win {
                        close_ext = true;
                    }
                } else {
                    egui::CentralPanel::default().show(v_ctx, draw_body);
                    if v_ctx.input(|i| i.viewport().close_requested()) {
                        close_ext = true;
                    }
                }
            });
            if close_ext {
                self.external_monitor = false;
                self.external_monitor_fullscreen = false;
            }
        }
    }

    pub(crate) fn show_dock_tab_contents(&mut self, ui: &mut egui::Ui, tab: DockTab) {
        match tab {
            DockTab::ProjectMedia => self.media_tab(ui),
            DockTab::Explorer => self.explorer_tab(ui),
            DockTab::Transitions => self.transitions_tab(ui),
            DockTab::VideoFx => self.video_fx_browser(ui),
            DockTab::ColorGrading => self.color_grading_tab(ui),
            DockTab::AudioFx => self.fx_tab(ui, true),
            DockTab::Generators => self.generators_browser(ui),
            DockTab::Mixer => self.mixer_tab(ui),
            DockTab::Loudness => self.loudness_tab(ui),
            DockTab::SurroundPanner => self.surround_panner_tab(ui),
            DockTab::PluginManager => self.plugin_manager_tab(ui),
            DockTab::Compositors => self.compositors_tab(ui),
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
            DockTab::Encoder => self.encoder_tab(ui),
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
            let split_active =
                self.opts.preview_split_mode != neoveg_engine::playback::PreviewSplitMode::Off;
            let split_resp = icons::icon_button(
                ui,
                Icon::SplitPreview,
                None,
                split_active,
                true,
                &self.theme,
            );
            split_resp.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Tela dividida")
            });
            if split_resp
                .on_hover_text("Alternar Prévia em tela dividida (Antes/Depois)")
                .clicked()
            {
                self.exec_cmd(Some(ui.ctx()), "preview.split_toggle");
            }
            ui.menu_button("Divisão", |ui| {
                for m in neoveg_engine::playback::PreviewSplitMode::ALL {
                    if ui
                        .selectable_label(self.opts.preview_split_mode == m, m.label())
                        .clicked()
                    {
                        self.opts.preview_split_mode = m;
                        ui.close_menu();
                    }
                }
                ui.separator();
                ui.checkbox(
                    &mut self.opts.preview_split_horizontal,
                    "Divisão horizontal",
                );
            });
            let zoom_txt = if (self.preview_zoom - 1.0).abs() < 0.01 {
                "Ajustar".to_string()
            } else {
                format!("{:.0}%", self.preview_zoom * 100.0)
            };
            ui.menu_button(zoom_txt, |ui| {
                for (lbl, v) in [
                    ("Ajustar (Fit)", 0.0_f32),
                    ("50%", 0.5),
                    ("100%", 1.0),
                    ("150%", 1.5),
                    ("200%", 2.0),
                    ("400%", 4.0),
                ] {
                    if ui.button(lbl).clicked() {
                        if v == 1.0 {
                            let (pw, ph) = (
                                self.project.settings.width as f32,
                                self.project.settings.height as f32,
                            );
                            let cw = ui.available_width().max(64.0);
                            self.preview_zoom = (pw / cw).clamp(1.0, 8.0);
                            if ph > 0.0 && (self.preview_zoom - 1.0).abs() < 0.01 {
                                self.preview_zoom = 1.01;
                            }
                        } else {
                            self.apply_preview_zoom(v);
                        }
                        ui.close_menu();
                    }
                }
            });
            let ram_busy = self.ram_preview_job.is_some();
            let ram_resp = icons::icon_button(
                ui,
                Icon::RamCache,
                None,
                ram_busy || !self.ram_preview.frames.is_empty(),
                true,
                &self.theme,
            );
            ram_resp.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Prévia em RAM")
            });
            if ram_resp
                .on_hover_text("Prévia dinâmica em RAM na seleção de tempo (Shift+B)")
                .clicked()
            {
                self.start_ram_preview();
            }
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
            let copy_resp = icons::icon_button(ui, Icon::CopyFrame, None, false, true, &self.theme);
            copy_resp.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Copiar quadro")
            });
            if copy_resp
                .on_hover_text("Copiar quadro atual para a área de transferência")
                .clicked()
            {
                self.copy_preview_frame_to_clipboard(Some(ui.ctx()));
            }
            if icons::icon_button(ui, Icon::CameraSnap, None, false, true, &self.theme)
                .on_hover_text("Salvar quadro em PNG")
                .clicked()
            {
                self.snapshot_dialog();
            }
            let mute_a_resp = icons::icon_button(
                ui,
                Icon::MuteAudio,
                None,
                self.opts.preview_mute_audio,
                true,
                &self.theme,
            );
            mute_a_resp.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    "Silenciar áudio da prévia",
                )
            });
            if mute_a_resp
                .on_hover_text("Silenciar todo o áudio na monitoração da prévia")
                .clicked()
            {
                self.exec_cmd(Some(ui.ctx()), "preview.mute_audio");
            }
            let mute_v_resp = icons::icon_button(
                ui,
                Icon::MuteVideo,
                None,
                self.opts.preview_mute_video,
                true,
                &self.theme,
            );
            mute_v_resp.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    "Silenciar vídeo da prévia",
                )
            });
            if mute_v_resp
                .on_hover_text("Silenciar todo o vídeo na prévia")
                .clicked()
            {
                self.exec_cmd(Some(ui.ctx()), "preview.mute_video");
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
            let ext_resp = icons::small_icon_button(
                ui,
                Icon::ExternalMonitor,
                None,
                self.external_monitor,
                &self.theme,
            );
            ext_resp.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Monitor externo")
            });
            if ext_resp
                .on_hover_text("Abrir ou fechar Monitor Externo (Alt+Shift+4)")
                .clicked()
            {
                self.exec_cmd(Some(ui.ctx()), "preview.external_monitor");
            }
            let mc_resp = icons::small_icon_button(
                ui,
                Icon::Multicam,
                if self.opts.multicam_mode {
                    Some("MC")
                } else {
                    None
                },
                self.opts.multicam_mode,
                &self.theme,
            );
            mc_resp.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Edição Multicâmera")
            });
            if mc_resp
                .on_hover_text("Alternar modo de edição multicâmera (Ctrl+Shift+D)")
                .clicked()
            {
                self.exec_cmd(Some(ui.ctx()), "tools.multicam_mode");
            }
            let s3d_mode = self.project.settings.stereo_3d;
            let s3d_lbl = if s3d_mode == neoveg_engine::model::Stereo3dMode::Off {
                None
            } else {
                Some(s3d_mode.short_label())
            };
            let s3d_resp = icons::small_icon_button(
                ui,
                Icon::Stereo3d,
                s3d_lbl,
                s3d_mode != neoveg_engine::model::Stereo3dMode::Off,
                &self.theme,
            );
            s3d_resp.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Modo 3D Estereoscópico")
            });
            if s3d_resp
                .on_hover_text("Alternar modo de prévia 3D Estereoscópico")
                .clicked()
            {
                self.exec_cmd(Some(ui.ctx()), "view.stereo_3d");
            }
            if !is_floating
                && icons::small_icon_button(ui, Icon::PopOut, None, false, &self.theme)
                    .on_hover_text("Destacar Prévia de Vídeo em janela normal do Windows")
                    .clicked()
            {
                self.preview_detached = true;
            }
            if self.session.expanded_edit.is_some()
                && icons::small_icon_button(ui, Icon::ExpandedEdit, Some("A/B"), true, &self.theme)
                    .on_hover_text("Modo de edição expandido ativo (clique para fechar)")
                    .clicked()
            {
                self.exec_cmd(Some(ui.ctx()), "edit.expanded_edit_mode");
            }
            // Mantido ao final da barra para evitar deslocamento horizontal dos botões
            // estáticos (Monitor externo, Cor, MC, 3D) quando quadros em RAM chegam assincronamente.
            if !self.ram_preview.frames.is_empty() && ui.small_button("Limpar RAM").clicked() {
                self.clear_ram_preview();
            }
        });
        if let Some(exp) = self.session.expanded_edit.clone() {
            use neoveg_engine::session::ExpandedEdgeFocus;
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("Expandido:").small().strong());
                for f in [
                    ExpandedEdgeFocus::Both,
                    ExpandedEdgeFocus::LeftOut,
                    ExpandedEdgeFocus::RightIn,
                ] {
                    if ui.selectable_label(exp.focus == f, f.label()).clicked() {
                        self.session.set_expanded_focus(f);
                    }
                }
                if ui.small_button("< 1q").clicked() {
                    self.exec_cmd(Some(ui.ctx()), "edit.expanded_nudge_left");
                }
                if ui.small_button("1q >").clicked() {
                    self.exec_cmd(Some(ui.ctx()), "edit.expanded_nudge_right");
                }
            });
        }
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
        let drop_out = ui.dnd_drop_zone::<DndPayload, Rect>(egui::Frame::NONE, |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(avail.width(), canvas_h), Sense::hover());
            self.draw_preview_canvas(ui, rect, true)
        });
        if let Some(payload) = drop_out.1 {
            match (*payload).clone() {
                DndPayload::Fx(kind) => {
                    if self.session.apply_video_fx_to_output(kind) {
                        self.dlg.fx_target = Some(FxTarget::MasterVideo);
                        self.sync_after_session();
                    } else {
                        self.sync_status_from_session();
                    }
                }
                DndPayload::FxPreset(mut fx) => {
                    if fx.kind.is_audio() {
                        self.set_status(
                            "Efeitos de áudio não podem ser aplicados na saída de vídeo",
                        );
                    } else {
                        self.checkpoint();
                        fx.id = new_id();
                        self.project.master.video_fx.push(fx);
                        self.dlg.fx_target = Some(FxTarget::MasterVideo);
                        self.changed();
                    }
                }
                _ => {}
            }
        }
        let img_rect = drop_out.0.inner;

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

    pub(crate) fn sync_master_monitor_audio(&self) {
        {
            let mut s = crate::util::lock(&self.audio.shared);
            s.dim = self.master_dim;
            s.mono_downmix = self.master_mono;
            s.preview_muted = self.opts.preview_mute_audio;
        }
        self.audio
            .set_bypass_audio_fx(self.opts.bypass_all_audio_fx);
    }

    fn draw_preview_canvas(&mut self, ui: &mut egui::Ui, rect: Rect, interactive: bool) -> Rect {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, self.theme.tl_preview_bg);
        let (proj_w, proj_h) = (self.project.settings.width, self.project.settings.height);
        let aspect = proj_w as f32 / proj_h.max(1) as f32;
        let (mut fw, mut fh) = (rect.width(), rect.width() / aspect);
        if fh > rect.height() {
            fh = rect.height();
            fw = fh * aspect;
        }
        let zoom = self.preview_zoom.clamp(0.25, 8.0);
        let w = (fw * zoom).max(8.0);
        let h = (fh * zoom).max(8.0);
        let max_px = ((w - rect.width()) * 0.5).max(0.0);
        let max_py = ((h - rect.height()) * 0.5).max(0.0);
        self.preview_pan.x = self.preview_pan.x.clamp(-max_px, max_px);
        self.preview_pan.y = self.preview_pan.y.clamp(-max_py, max_py);
        let img_rect = Rect::from_center_size(rect.center() + self.preview_pan, vec2(w, h));

        let split_mode = self.opts.preview_split_mode;
        if self.session.expanded_edit.is_some() {
            if let Some((lt_tex, rt_tex)) = &self.expanded_ab_tex {
                let mid_x = img_rect.center().x;
                let left_r = Rect::from_min_max(
                    img_rect.left_top(),
                    pos2((mid_x - 1.0).max(img_rect.left()), img_rect.bottom()),
                );
                let right_r = Rect::from_min_max(
                    pos2((mid_x + 1.0).min(img_rect.right()), img_rect.top()),
                    img_rect.right_bottom(),
                );
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                painter.image(lt_tex.id(), left_r, uv, Color32::WHITE);
                painter.image(rt_tex.id(), right_r, uv, Color32::WHITE);
                painter.line_segment(
                    [pos2(mid_x, img_rect.top()), pos2(mid_x, img_rect.bottom())],
                    Stroke::new(2.0_f32, self.theme.accent),
                );
                painter.text(
                    left_r.left_top() + vec2(4.0, 3.0),
                    egui::Align2::LEFT_TOP,
                    "A: Saída",
                    egui::FontId::proportional(10.0),
                    Color32::WHITE,
                );
                painter.text(
                    right_r.left_top() + vec2(4.0, 3.0),
                    egui::Align2::LEFT_TOP,
                    "B: Entrada",
                    egui::FontId::proportional(10.0),
                    Color32::WHITE,
                );
            }
        } else if split_mode != neoveg_engine::playback::PreviewSplitMode::Off
            && !self.opts.preview_mute_video
        {
            use neoveg_engine::playback::PreviewSplitMode;
            let cmp_tex = match split_mode {
                PreviewSplitMode::FxBypass => {
                    self.preview_split_tex.as_ref().map(|t| (t.id(), "Sem FX"))
                }
                PreviewSplitMode::Clipboard => self
                    .preview_clipboard
                    .as_ref()
                    .map(|(t, _)| (t.id(), "Transferência")),
                PreviewSplitMode::Off => None,
            };
            let k = self.opts.preview_split_pos.clamp(0.08, 0.92);
            if let (Some((alt_id, alt_lbl)), Some(main_tex)) = (cmp_tex, &self.preview_tex) {
                if self.opts.preview_split_horizontal {
                    let div_y = img_rect.top() + img_rect.height() * k;
                    let top_r =
                        Rect::from_min_max(img_rect.left_top(), pos2(img_rect.right(), div_y));
                    let bot_r =
                        Rect::from_min_max(pos2(img_rect.left(), div_y), img_rect.right_bottom());
                    painter.image(
                        alt_id,
                        top_r,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, k)),
                        Color32::WHITE,
                    );
                    painter.image(
                        main_tex.id(),
                        bot_r,
                        Rect::from_min_max(pos2(0.0, k), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    painter.line_segment(
                        [pos2(img_rect.left(), div_y), pos2(img_rect.right(), div_y)],
                        Stroke::new(2.0_f32, self.theme.accent),
                    );
                } else {
                    let div_x = img_rect.left() + img_rect.width() * k;
                    let left_r =
                        Rect::from_min_max(img_rect.left_top(), pos2(div_x, img_rect.bottom()));
                    let right_r =
                        Rect::from_min_max(pos2(div_x, img_rect.top()), img_rect.right_bottom());
                    painter.image(
                        alt_id,
                        left_r,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(k, 1.0)),
                        Color32::WHITE,
                    );
                    painter.image(
                        main_tex.id(),
                        right_r,
                        Rect::from_min_max(pos2(k, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    painter.line_segment(
                        [pos2(div_x, img_rect.top()), pos2(div_x, img_rect.bottom())],
                        Stroke::new(2.0_f32, self.theme.accent),
                    );
                }
                painter.text(
                    img_rect.left_top() + vec2(6.0, 4.0),
                    egui::Align2::LEFT_TOP,
                    alt_lbl,
                    egui::FontId::proportional(10.0),
                    Color32::WHITE,
                );
                painter.text(
                    img_rect.right_top() + vec2(-6.0, 4.0),
                    egui::Align2::RIGHT_TOP,
                    "Ativa",
                    egui::FontId::proportional(10.0),
                    Color32::WHITE,
                );
            } else if let Some(tex) = &self.preview_tex {
                painter.image(
                    tex.id(),
                    img_rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                if split_mode == PreviewSplitMode::Clipboard {
                    painter.text(
                        img_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Copie um quadro antes para comparar",
                        egui::FontId::proportional(11.0),
                        Color32::from_white_alpha(200),
                    );
                }
            }
        } else if let Some(tex) = &self.preview_tex {
            painter.image(
                tex.id(),
                img_rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        if interactive {
            let mut dragging_split = false;
            if split_mode != neoveg_engine::playback::PreviewSplitMode::Off
                && self.session.expanded_edit.is_none()
            {
                let k = self.opts.preview_split_pos.clamp(0.08, 0.92);
                let hit_rect = if self.opts.preview_split_horizontal {
                    let y = img_rect.top() + img_rect.height() * k;
                    Rect::from_min_max(
                        pos2(img_rect.left(), y - 6.0),
                        pos2(img_rect.right(), y + 6.0),
                    )
                } else {
                    let x = img_rect.left() + img_rect.width() * k;
                    Rect::from_min_max(
                        pos2(x - 6.0, img_rect.top()),
                        pos2(x + 6.0, img_rect.bottom()),
                    )
                };
                let div_resp = ui.interact(
                    hit_rect,
                    ui.id().with("pv_split_handle"),
                    Sense::click_and_drag(),
                );
                if div_resp.dragged() {
                    dragging_split = true;
                    if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                        let new_k = if self.opts.preview_split_horizontal {
                            (pos.y - img_rect.top()) / img_rect.height().max(1.0)
                        } else {
                            (pos.x - img_rect.left()) / img_rect.width().max(1.0)
                        };
                        self.opts.preview_split_pos = new_k.clamp(0.08, 0.92);
                    }
                }
            }
            let canvas_resp = ui.interact(
                rect,
                ui.id().with("pv_zoom_pan_canvas"),
                Sense::click_and_drag(),
            );
            if canvas_resp.hovered() {
                let sy = ui.input(|i| i.smooth_scroll_delta.y);
                if sy.abs() > 0.5 {
                    let factor = if sy > 0.0 { 1.12 } else { 1.0 / 1.12 };
                    self.preview_zoom = (self.preview_zoom * factor).clamp(0.25, 8.0);
                    if (self.preview_zoom - 1.0).abs() < 0.02 {
                        self.apply_preview_zoom(0.0);
                    }
                }
            }
            if !dragging_split && self.preview_zoom > 1.01 && canvas_resp.dragged() {
                self.preview_pan += canvas_resp.drag_delta();
            }
            if canvas_resp.double_clicked() && (self.preview_zoom - 1.0).abs() > 0.02 {
                self.apply_preview_zoom(0.0);
            }
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

        if self.opts.preview_mute_video {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "VÍDEO SILENCIADO",
                egui::FontId::proportional(13.0),
                self.theme.offline_fg,
            );
        } else {
            let mc_drawn = if interactive && self.opts.multicam_mode {
                if let Some((_, _, ev_id, active_idx, count)) =
                    self.session.active_multicam_event_at(self.cursor)
                {
                    self.draw_multicam_grid(ui, &painter, img_rect, ev_id, active_idx, count);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if !mc_drawn {
                if let Some(job) = &self.ram_preview_job {
                    let done = job.done.load(Ordering::Relaxed);
                    let total = job.total.load(Ordering::Relaxed).max(1);
                    let pct = (done.saturating_mul(100) / total).min(100);
                    painter.text(
                        rect.left_bottom() + vec2(6.0, -4.0),
                        egui::Align2::LEFT_BOTTOM,
                        format!("RAM Preview: {pct}% ({done}/{total})"),
                        egui::FontId::proportional(10.0),
                        Color32::WHITE,
                    );
                }
            }
        }
        img_rect
    }

    fn draw_multicam_grid(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        rect: Rect,
        ev_id: Id,
        active_idx: usize,
        count: usize,
    ) {
        let shown = count.clamp(1, 16);
        let cols = if shown <= 4 {
            2
        } else if shown <= 9 {
            3
        } else {
            4
        };
        let rows = shown.div_ceil(cols).max(1);
        let gap = 3.0_f32;
        let cell_w = ((rect.width() - gap * (cols as f32 - 1.0)) / cols as f32).max(16.0);
        let cell_h = ((rect.height() - gap * (rows as f32 - 1.0)) / rows as f32).max(16.0);
        painter.rect_filled(rect, 0.0, Color32::BLACK);
        let qt = self.project.settings.quantize(self.cursor).0;
        let take_names: Vec<String> = self
            .project
            .event(ev_id)
            .map(|e| {
                neoveg_engine::takes::takes(e)
                    .into_iter()
                    .enumerate()
                    .map(|(i, tk)| {
                        if tk.name.is_empty() {
                            format!("Câm {}", i + 1)
                        } else {
                            tk.name
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        if self.multicam_thumbs.len() > 64 {
            self.multicam_thumbs.clear();
        }
        let mut clicked_angle: Option<usize> = None;
        for angle_i in 0..shown {
            let col = angle_i % cols;
            let row = angle_i / cols;
            let x0 = rect.left() + col as f32 * (cell_w + gap);
            let y0 = rect.top() + row as f32 * (cell_h + gap);
            let cr = Rect::from_min_size(pos2(x0, y0), vec2(cell_w, cell_h));
            let key = (ev_id, angle_i, qt);
            if !self.multicam_thumbs.contains_key(&key) {
                let frame = neoveg_engine::engine::video::render_event_take_still(
                    &self.project,
                    &self.media,
                    ev_id,
                    angle_i,
                    Ticks::from_raw(qt),
                    160,
                    90,
                    true,
                );
                if let Some(rgba) = frame.to_image() {
                    let ci = crate::app::rgba_to_color_image(&rgba);
                    let tex = ui.ctx().load_texture(
                        format!("mc_{ev_id}_{angle_i}_{qt}"),
                        ci,
                        egui::TextureOptions::LINEAR,
                    );
                    self.multicam_thumbs.insert(key, tex);
                }
            }
            if let Some(tex) = self.multicam_thumbs.get(&key) {
                painter.image(
                    tex.id(),
                    cr,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            let active = angle_i == active_idx;
            let border_col = if active {
                Color32::from_rgb(250, 210, 50)
            } else {
                Color32::from_gray(95)
            };
            painter.rect_stroke(
                cr,
                0.0,
                Stroke::new(if active { 2.5_f32 } else { 1.0_f32 }, border_col),
                StrokeKind::Inside,
            );
            let lbl = take_names
                .get(angle_i)
                .cloned()
                .unwrap_or_else(|| format!("Câm {}", angle_i + 1));
            painter.rect_filled(
                Rect::from_min_size(
                    cr.left_top() + vec2(3.0, 3.0),
                    vec2((cell_w - 6.0).min(120.0), 15.0),
                ),
                2.0,
                Color32::from_black_alpha(180),
            );
            painter.text(
                cr.left_top() + vec2(6.0, 5.0),
                egui::Align2::LEFT_TOP,
                format!("[{}] {}", angle_i + 1, lbl),
                egui::FontId::proportional(10.0),
                if active {
                    Color32::from_rgb(255, 225, 90)
                } else {
                    Color32::WHITE
                },
            );
            let resp = ui.interact(
                cr,
                ui.id().with(("mc_cell", ev_id, angle_i)),
                Sense::click(),
            );
            resp.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    format!("Câmera {}", angle_i + 1),
                )
            });
            if resp.clicked() {
                clicked_angle = Some(angle_i);
            }
        }
        if let Some(angle_i) = clicked_angle {
            if self.session.switch_multicam_angle(self.cursor, angle_i) {
                self.sync_after_session();
            }
        }
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
        self.project_media_ui(ui);
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
                    self.fx_apply_buttons(ui, k, audio);
                    ui.end_row();
                }
            });
    }

    /// Botões Eventos, Trilha e Master de um efeito (lista de áudio e
    /// janela Efeitos de Vídeo): aplicam à seleção do tipo certo, à trilha
    /// selecionada ou ao Master, com um passo de Desfazer.
    pub(crate) fn fx_apply_buttons(&mut self, ui: &mut egui::Ui, k: FxKind, audio: bool) {
        self.fx_apply_instance_buttons(ui, FxInstance::new(k), audio);
    }

    pub(crate) fn fx_apply_instance_buttons(
        &mut self,
        ui: &mut egui::Ui,
        fx: FxInstance,
        audio: bool,
    ) {
        let want_kind = if audio {
            TrackKind::Audio
        } else {
            TrackKind::Video
        };
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
            for (idx, id) in sel_ev.iter().enumerate() {
                if let Some(ev) = self.project.event_mut(*id) {
                    let mut inst = fx.clone();
                    if idx > 0 {
                        inst.id = new_id();
                    }
                    ev.fx.push(inst);
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
            self.project.tracks[t].fx.push(fx.clone());
            self.dlg.fx_target = Some(FxTarget::Track(self.project.tracks[t].id));
            self.changed();
        }
        if ui.button("Master").clicked() {
            self.checkpoint();
            if audio {
                self.project.master.fx.push(fx);
                self.dlg.fx_target = Some(FxTarget::MasterAudio);
            } else {
                self.project.master.video_fx.push(fx);
                self.dlg.fx_target = Some(FxTarget::MasterVideo);
            }
            self.changed();
        }
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
            ui.separator();
            let mut bypass = self.opts.bypass_all_audio_fx;
            if ui
                .toggle_value(&mut bypass, "Bypass FX")
                .on_hover_text("Ignorar todos os plug-ins de efeito de áudio (Opções > Ignorar todos os FX de áudio)")
                .changed()
            {
                let ctx = ui.ctx().clone();
                self.exec_cmd(Some(&ctx), "options.bypass_all_audio_fx");
            }
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
            let res = crate::util::catch_isolated(|| {
                analyze_project_loudness(
                    &proj,
                    &media,
                    range,
                    sr,
                    &cancel_bg,
                    Some((&done_bg, &total_bg)),
                )
            });
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
