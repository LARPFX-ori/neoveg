//! Área de janelas acopláveis (Window Docking Area) e janela de prévia de vídeo.

use super::timeline::draw_meter;
use super::{App, DndPayload, DockTab, FxTarget, PreviewQuality};
use crate::icons::{self, Icon};
use crate::model::*;
use eframe::egui;
use egui::{pos2, vec2, Color32, Rect, Sense, Stroke, StrokeKind};
use std::path::PathBuf;

fn dock_tab_icon(tab: DockTab) -> Icon {
    match tab {
        DockTab::ProjectMedia => Icon::MediaItem,
        DockTab::Explorer => Icon::FolderItem,
        DockTab::Transitions => Icon::TransitionSplit,
        DockTab::VideoFx => Icon::FxSparkle,
        DockTab::ColorGrading => Icon::ColorWheels,
        DockTab::AudioFx => Icon::FxAudio,
        DockTab::Generators => Icon::GeneratorFrame,
        DockTab::Mixer => Icon::MixerFaders,
        DockTab::Scopes => Icon::ScopesWave,
        DockTab::Notes => Icon::NotesPad,
    }
}

impl App {
    fn dock_tab_strip(&mut self, ui: &mut egui::Ui, docked: &[DockTab]) {
        let mut detach_req: Option<DockTab> = None;
        ui.horizontal_wrapped(|ui| {
            for &tab in docked {
                if icons::small_icon_button(
                    ui,
                    dock_tab_icon(tab),
                    Some(tab.name()),
                    self.dock_tab == tab,
                    &self.theme,
                )
                .clicked()
                {
                    self.dock_tab = tab;
                }
            }
            if docked.contains(&self.dock_tab)
                && icons::small_icon_button(ui, Icon::PopOut, Some("Destacar"), false, &self.theme)
                    .on_hover_text("Abrir a aba ativa em janela flutuante")
                    .clicked()
            {
                detach_req = Some(self.dock_tab);
            }
        });
        if let Some(tab) = detach_req {
            self.detached_tabs.insert(tab);
            if let Some(next) = DockTab::ALL
                .into_iter()
                .find(|t| !self.detached_tabs.contains(t))
            {
                self.dock_tab = next;
            }
        }
    }

    fn dock_tab_contents(&mut self, ui: &mut egui::Ui, docked: &[DockTab]) {
        if docked.is_empty() {
            ui.label(
                egui::RichText::new(
                    "Todas as abas estão destacadas em janelas flutuantes (clique em Acoplar ou use Exibir/Layouts).",
                )
                .weak(),
            );
        } else {
            let tab = self.dock_tab;
            egui::ScrollArea::both()
                .id_salt("dock_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| self.show_dock_tab_contents(ui, tab));
        }
    }

    pub fn dock_area(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_rect_before_wrap();
        let preview_w = (avail.width() * self.preview_ratio).clamp(260.0, 900.0);
        let (tabs_rect, preview_rect) = if self.prefs.edit_layout.preview_on_left {
            (
                Rect::from_min_max(pos2(avail.left() + preview_w + 6.0, avail.top()), avail.max),
                Rect::from_min_max(avail.min, pos2(avail.left() + preview_w, avail.bottom())),
            )
        } else {
            (
                Rect::from_min_max(
                    avail.min,
                    pos2(avail.right() - preview_w - 6.0, avail.bottom()),
                ),
                Rect::from_min_max(pos2(avail.right() - preview_w, avail.top()), avail.max),
            )
        };
        ui.allocate_rect(avail, Sense::hover());

        let docked: Vec<DockTab> = DockTab::ALL
            .into_iter()
            .filter(|t| !self.detached_tabs.contains(t))
            .collect();
        if !docked.contains(&self.dock_tab) {
            if let Some(&first) = docked.first() {
                self.dock_tab = first;
            }
        }

        if self.prefs.edit_layout.tabs_at_top {
            let mut left = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(tabs_rect)
                    .id_salt("dock_tabs_pane_top"),
            );
            left.set_clip_rect(tabs_rect);
            self.dock_tab_strip(&mut left, &docked);
            left.separator();
            self.dock_tab_contents(&mut left, &docked);
        } else {
            // Nas gerações Sonic Foundry e Sony, as abas ficam na borda inferior do dock.
            // Calcula quantas linhas serão usadas em `dock_tab_strip` para não cortar abas
            // como `Color Grading`, `Notas` ou `Destacar` em janelas médias/estreitas.
            let max_w = (tabs_rect.width() - 8.0).max(120.0);
            let mut rows = 1usize;
            let mut cur_x = 0.0_f32;
            for item_w in docked
                .iter()
                .map(|t| 30.0 + t.name().chars().count() as f32 * 6.8)
                .chain(std::iter::once(
                    30.0 + "Destacar".chars().count() as f32 * 6.8,
                ))
            {
                if cur_x > 0.0 && cur_x + item_w + 4.0 > max_w {
                    rows += 1;
                    cur_x = item_w;
                } else {
                    cur_x += item_w + 4.0;
                }
            }
            let base_h: f32 = (rows.min(4) as f32) * 24.0 + 4.0;
            let strip_h = base_h.min((tabs_rect.height() * 0.45).max(24.0));
            let content_rect = Rect::from_min_max(
                tabs_rect.min,
                pos2(
                    tabs_rect.right(),
                    (tabs_rect.bottom() - strip_h - 4.0).max(tabs_rect.top() + 20.0),
                ),
            );
            let strip_rect = Rect::from_min_max(
                pos2(
                    tabs_rect.left(),
                    (tabs_rect.bottom() - strip_h).max(tabs_rect.top()),
                ),
                tabs_rect.max,
            );
            let mut content_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(content_rect)
                    .id_salt("dock_content_pane_bot"),
            );
            content_ui.set_clip_rect(content_rect);
            self.dock_tab_contents(&mut content_ui, &docked);

            let mut strip_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(strip_rect)
                    .id_salt("dock_strip_pane_bot"),
            );
            strip_ui.set_clip_rect(strip_rect);
            strip_ui.separator();
            self.dock_tab_strip(&mut strip_ui, &docked);
        }

        let mut right = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(preview_rect)
                .id_salt("dock_right"),
        );
        right.set_clip_rect(preview_rect);
        self.preview_panel(&mut right);
    }

    pub fn floating_panel_windows(&mut self, ctx: &egui::Context) {
        let detached: Vec<DockTab> = DockTab::ALL
            .into_iter()
            .filter(|t| self.detached_tabs.contains(t))
            .collect();
        for tab in detached {
            let mut open = true;
            let mut reattach = false;
            egui::Window::new(tab.name())
                .id(egui::Id::new(("floating_dock", tab.command_id())))
                .open(&mut open)
                .default_size([460.0, 320.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if icons::small_icon_button(
                            ui,
                            Icon::DockIn,
                            Some("Acoplar na área inferior"),
                            false,
                            &self.theme,
                        )
                        .clicked()
                        {
                            reattach = true;
                        }
                    });
                    ui.separator();
                    egui::ScrollArea::both()
                        .id_salt(("floating_scroll", tab.command_id()))
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.show_dock_tab_contents(ui, tab));
                });
            if !open || reattach {
                self.detached_tabs.remove(&tab);
                if reattach {
                    self.dock_visible = true;
                    self.dock_tab = tab;
                }
            }
        }
    }

    fn show_dock_tab_contents(&mut self, ui: &mut egui::Ui, tab: DockTab) {
        match tab {
            DockTab::ProjectMedia => self.media_tab(ui),
            DockTab::Explorer => self.explorer_tab(ui),
            DockTab::Transitions => self.transitions_tab(ui),
            DockTab::VideoFx => self.fx_tab(ui, false),
            DockTab::ColorGrading => self.color_grading_tab(ui),
            DockTab::AudioFx => self.fx_tab(ui, true),
            DockTab::Generators => self.generators_tab(ui),
            DockTab::Mixer => self.mixer_tab(ui),
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

    fn preview_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Prévia de Vídeo").strong());
            egui::ComboBox::from_id_salt("pq")
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
        });
        let avail = ui.available_rect_before_wrap();
        let (rect, _resp) = ui.allocate_exact_size(avail.size(), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, self.theme.tl_preview_bg);
        let s = &self.project.settings;
        let aspect = s.width as f32 / s.height.max(1) as f32;
        let info_h = 18.0;
        let area = Rect::from_min_max(rect.min, pos2(rect.right(), rect.bottom() - info_h));
        let (mut w, mut h) = (area.width(), area.width() / aspect);
        if h > area.height() {
            h = area.height();
            w = h * aspect;
        }
        let img_rect = Rect::from_center_size(area.center(), vec2(w, h));
        if let Some(tex) = &self.preview_tex {
            painter.image(
                tex.id(),
                img_rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        if self.opts.safe_areas {
            for k in [0.9, 0.8] {
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
        let pq = self.opts.preview_quality.scale();
        painter.text(
            pos2(rect.left() + 4.0, rect.bottom() - info_h + 2.0),
            egui::Align2::LEFT_TOP,
            format!(
                "Projeto: {}x{}, {}p   Prévia: {}x{}   Quadro: {}",
                s.width,
                s.height,
                s.fps.label(),
                (s.width as f32 * pq) as u32,
                (s.height as f32 * pq) as u32,
                s.fps.ticks_to_frame_round(self.view_time())
            ),
            egui::FontId::proportional(11.0),
            Color32::from_gray(180),
        );
    }

    // -----------------------------------------------------------------
    // Mídia do projeto
    // -----------------------------------------------------------------

    fn media_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
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
            let n = self.project.tracks.len();
            for ti in 0..n {
                if self.project.tracks[ti].kind != TrackKind::Audio {
                    continue;
                }
                let tid = self.project.tracks[ti].id;
                let lv = self.meters_hold.get(&tid).cloned().unwrap_or([0.0; 2]);
                ui.group(|ui| {
                    ui.set_width(92.0);
                    ui.vertical(|ui| {
                        let t = &mut self.session.project.tracks[ti];
                        ui.label(egui::RichText::new(&t.name).strong().small());
                        let mut changed = false;
                        ui.horizontal(|ui| {
                            changed |= ui.toggle_value(&mut t.mute, "M").changed();
                            changed |= ui.toggle_value(&mut t.solo, "S").changed();
                            if ui.small_button("FX").clicked() {
                                self.dlg.fx_target = Some(FxTarget::Track(tid));
                            }
                        });
                        let t = &mut self.session.project.tracks[ti];
                        ui.horizontal(|ui| {
                            let r = ui.add(
                                egui::Slider::new(&mut t.volume_db, -60.0..=12.0)
                                    .vertical()
                                    .show_value(false),
                            );
                            changed |= r.changed();
                            let (mr, _) = ui.allocate_exact_size(vec2(12.0, 110.0), Sense::hover());
                            draw_meter(ui.painter(), mr, lv);
                        });
                        ui.label(egui::RichText::new(format!("{:+.1} dB", t.volume_db)).small());
                        changed |= ui
                            .add(egui::Slider::new(&mut t.pan, -1.0..=1.0).show_value(false))
                            .changed();
                        ui.label(egui::RichText::new(EnvKind::Pan.format(t.pan)).small());
                        if changed {
                            self.changed();
                        }
                    });
                });
            }
            // master
            ui.group(|ui| {
                ui.set_width(100.0);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new("Master").strong());
                    if ui.small_button("FX master").clicked() {
                        self.dlg.fx_target = Some(FxTarget::MasterAudio);
                    }
                    let mut changed = false;
                    ui.horizontal(|ui| {
                        changed |= ui
                            .add(
                                egui::Slider::new(&mut self.project.master.volume_db, -60.0..=12.0)
                                    .vertical()
                                    .show_value(false),
                            )
                            .changed();
                        let (mr, _) = ui.allocate_exact_size(vec2(16.0, 110.0), Sense::hover());
                        draw_meter(ui.painter(), mr, self.master_hold);
                    });
                    ui.label(
                        egui::RichText::new(format!("{:+.1} dB", self.project.master.volume_db))
                            .small(),
                    );
                    let pk = lin_to_db(self.master_hold[0].max(self.master_hold[1]));
                    ui.label(
                        egui::RichText::new(if pk < -100.0 {
                            "pico: -inf".into()
                        } else {
                            format!("pico: {pk:.1} dB")
                        })
                        .small(),
                    );
                    if changed {
                        self.changed();
                    }
                });
            });
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
