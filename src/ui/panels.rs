//! Área de janelas acopláveis (Window Docking Area) e janela de prévia de vídeo.

use super::timeline::draw_meter;
use super::{App, DndPayload, DockTab, FxTarget, PreviewQuality};
use crate::model::*;
use eframe::egui;
use egui::{pos2, vec2, Color32, Rect, Sense, Stroke, StrokeKind};
use std::path::PathBuf;

impl App {
    pub fn dock_area(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_rect_before_wrap();
        let preview_w = (avail.width() * 0.42).clamp(260.0, 900.0);
        let left_rect = Rect::from_min_max(avail.min, pos2(avail.right() - preview_w - 6.0, avail.bottom()));
        let right_rect = Rect::from_min_max(pos2(avail.right() - preview_w, avail.top()), avail.max);
        ui.allocate_rect(avail, Sense::hover());

        let mut left = ui.new_child(egui::UiBuilder::new().max_rect(left_rect).id_salt("dock_left"));
        left.set_clip_rect(left_rect);
        left.horizontal_wrapped(|ui| {
            for tab in DockTab::ALL {
                if ui.selectable_label(self.dock_tab == tab, tab.name()).clicked() {
                    self.dock_tab = tab;
                }
            }
        });
        left.separator();
        egui::ScrollArea::both().id_salt("dock_scroll").auto_shrink([false, false]).show(&mut left, |ui| match self.dock_tab {
            DockTab::ProjectMedia => self.media_tab(ui),
            DockTab::Explorer => self.explorer_tab(ui),
            DockTab::Transitions => self.transitions_tab(ui),
            DockTab::VideoFx => self.fx_tab(ui, false),
            DockTab::AudioFx => self.fx_tab(ui, true),
            DockTab::Generators => self.generators_tab(ui),
            DockTab::Mixer => self.mixer_tab(ui),
            DockTab::Scopes => self.scopes_tab(ui),
            DockTab::Notes => {
                ui.label("Notas do projeto:");
                if ui
                    .add(egui::TextEdit::multiline(&mut self.project.notes).desired_rows(12).desired_width(f32::INFINITY))
                    .changed()
                {
                    self.dirty = true;
                }
            }
        });

        let mut right = ui.new_child(egui::UiBuilder::new().max_rect(right_rect).id_salt("dock_right"));
        right.set_clip_rect(right_rect);
        self.preview_panel(&mut right);
    }

    fn preview_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Prévia de Vídeo").strong());
            egui::ComboBox::from_id_salt("pq")
                .selected_text(self.opts.preview_quality.name())
                .show_ui(ui, |ui| {
                    for q in [PreviewQuality::Draft, PreviewQuality::Preview, PreviewQuality::Good] {
                        ui.selectable_value(&mut self.opts.preview_quality, q, q.name());
                    }
                });
            ui.toggle_value(&mut self.opts.safe_areas, "▣").on_hover_text("Áreas de segurança");
            if ui.button("📷").on_hover_text("Salvar quadro em PNG").clicked() {
                self.snapshot_dialog();
            }
            if ui.button("fx").on_hover_text("Efeitos de saída de vídeo").clicked() {
                self.dlg.fx_target = Some(FxTarget::MasterVideo);
            }
        });
        let avail = ui.available_rect_before_wrap();
        let (rect, _resp) = ui.allocate_exact_size(avail.size(), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(16, 16, 18));
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
            painter.image(tex.id(), img_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        if self.opts.safe_areas {
            for k in [0.9, 0.8] {
                let r = Rect::from_center_size(img_rect.center(), img_rect.size() * k);
                painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 90)), StrokeKind::Inside);
            }
            let c = img_rect.center();
            painter.line_segment([c - vec2(8.0, 0.0), c + vec2(8.0, 0.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(120)));
            painter.line_segment([c - vec2(0.0, 8.0), c + vec2(0.0, 8.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(120)));
        }
        let pq = self.opts.preview_quality.scale();
        painter.text(
            pos2(rect.left() + 4.0, rect.bottom() - info_h + 2.0),
            egui::Align2::LEFT_TOP,
            format!(
                "Projeto: {}x{}, {:.3}p   Prévia: {}x{}   Quadro: {}",
                s.width,
                s.height,
                s.fps,
                (s.width as f32 * pq) as u32,
                (s.height as f32 * pq) as u32,
                (self.view_time() * s.fps).round() as i64
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
            if ui.button("➕ Importar...").clicked() {
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
        egui::Grid::new("media_grid").striped(true).num_columns(3).show(ui, |ui| {
            for m in &items {
                let thumb = self.thumb(&ctx, m.id);
                let id = egui::Id::new(("media_drag", m.id));
                let resp = ui
                    .dnd_drag_source(id, DndPayload::Media(m.id), |ui| {
                        ui.horizontal(|ui| {
                            if let Some(t) = &thumb {
                                let s = t.size_vec2();
                                let h = 36.0;
                                ui.image((t.id(), vec2(h * s.x / s.y.max(1.0), h)));
                            } else {
                                ui.label(if m.has_audio { "🔊" } else { "❔" });
                            }
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(&m.name).strong());
                                let dur = m.duration.map(|d| self.fmt_time(d)).unwrap_or_else(|| "estática".into());
                                ui.label(egui::RichText::new(format!("{}  •  {}", dur, m.info)).small());
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
                    if matches!(m.source, MediaSource::Generated(_)) && ui.button("Editar mídia gerada...").clicked() {
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
                if !self.media.contains(m.id) && !matches!(m.source, MediaSource::Generated(_)) {
                    ui.colored_label(Color32::from_rgb(240, 120, 80), "offline");
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
        if let Some(id) = edit_gen {
            self.dlg.gen_editor = Some(id);
        }
        if let Some(id) = remove {
            let used = self.project.tracks.iter().any(|t| t.events.iter().any(|e| e.media_id == id));
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
            if ui.button("⬆").on_hover_text("Pasta acima").clicked() {
                if let Some(p) = self.explorer_dir.parent() {
                    self.explorer_dir = p.to_path_buf();
                }
            }
            let mut s = self.explorer_dir.to_string_lossy().to_string();
            if ui.add(egui::TextEdit::singleline(&mut s).desired_width(320.0)).lost_focus() {
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
            .map(|rd| rd.flatten().map(|e| (e.path().is_dir(), e.path())).collect())
            .unwrap_or_default();
        entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.file_name().cmp(&b.1.file_name())));
        let filter = self.explorer_filter.to_lowercase();
        let mut go = None;
        let mut import = vec![];
        for (is_dir, p) in entries {
            let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if name.starts_with('.') {
                continue;
            }
            if !is_dir && !crate::media::is_supported(&p) && !name.ends_with(".nvp") {
                continue;
            }
            if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                continue;
            }
            let label = if is_dir { format!("📁 {name}") } else { format!("🎞 {name}") };
            let r = ui.selectable_label(false, label);
            if r.double_clicked() {
                if is_dir {
                    go = Some(p.clone());
                } else if name.ends_with(".nvp") {
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
        egui::Grid::new("tr_grid").num_columns(2).striped(true).show(ui, |ui| {
            for k in TransitionKind::ALL {
                let id = egui::Id::new(("tr", k as u32));
                ui.dnd_drag_source(id, DndPayload::Transition(k), |ui| {
                    ui.label(format!("◪ {}", k.name()));
                });
                if ui.add_enabled(!sel.is_empty(), egui::Button::new("Aplicar")).clicked() {
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
        let list: Vec<FxKind> = if audio { FxKind::AUDIO.to_vec() } else { FxKind::VIDEO.to_vec() };
        let want_kind = if audio { TrackKind::Audio } else { TrackKind::Video };
        egui::Grid::new(("fx_grid", audio)).num_columns(4).striped(true).show(ui, |ui| {
            for k in list {
                let id = egui::Id::new(("fx", k as u32));
                ui.dnd_drag_source(id, DndPayload::Fx(k), |ui| {
                    ui.label(format!("✨ {}", k.name()));
                });
                let sel_ev: Vec<Id> = self
                    .sel_events
                    .iter()
                    .filter(|id| self.project.find_event(**id).map(|(t, _)| self.project.tracks[t].kind == want_kind).unwrap_or(false))
                    .cloned()
                    .collect();
                if ui.add_enabled(!sel_ev.is_empty(), egui::Button::new("Eventos")).clicked() {
                    self.checkpoint();
                    for id in &sel_ev {
                        self.project.event_mut(*id).unwrap().fx.push(FxInstance::new(k));
                    }
                    self.dlg.fx_target = Some(FxTarget::Event(sel_ev[0]));
                    self.changed();
                }
                let tr = self.sel_track.filter(|&t| self.project.tracks.get(t).map(|x| x.kind) == Some(want_kind));
                if ui.add_enabled(tr.is_some(), egui::Button::new("Trilha")).clicked() {
                    let t = tr.unwrap();
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
        ui.label(egui::RichText::new("Arraste um gerador para a timeline ou clique para inserir no cursor.").small().weak());
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
                    text: "Direção\nFulano de Tal\n\nEdição\nNVOV\n\nObrigado por assistir".into(),
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
            ("Cor sólida (preto)", GenSpec::SolidColor { color: [0.0, 0.0, 0.0, 1.0] }),
            ("Cor sólida (branco)", GenSpec::SolidColor { color: [1.0, 1.0, 1.0, 1.0] }),
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
            ("Ruído (estático)", GenSpec::Noise { mono: true, animated: true }),
        ];
        egui::Grid::new("gen_grid").num_columns(2).striped(true).show(ui, |ui| {
            for (i, (name, spec)) in presets.into_iter().enumerate() {
                ui.dnd_drag_source(egui::Id::new(("gen", i)), DndPayload::Generator(spec.clone()), |ui| {
                    ui.label(format!("🖼 {name}"));
                });
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
                        let t = &mut self.project.tracks[ti];
                        ui.label(egui::RichText::new(&t.name).strong().small());
                        let mut changed = false;
                        ui.horizontal(|ui| {
                            changed |= ui.toggle_value(&mut t.mute, "M").changed();
                            changed |= ui.toggle_value(&mut t.solo, "S").changed();
                            if ui.small_button("FX").clicked() {
                                self.dlg.fx_target = Some(FxTarget::Track(tid));
                            }
                        });
                        let t = &mut self.project.tracks[ti];
                        ui.horizontal(|ui| {
                            let r = ui.add(egui::Slider::new(&mut t.volume_db, -60.0..=12.0).vertical().show_value(false));
                            changed |= r.changed();
                            let (mr, _) = ui.allocate_exact_size(vec2(12.0, 110.0), Sense::hover());
                            draw_meter(ui.painter(), mr, lv);
                        });
                        ui.label(egui::RichText::new(format!("{:+.1} dB", t.volume_db)).small());
                        changed |= ui.add(egui::Slider::new(&mut t.pan, -1.0..=1.0).show_value(false)).changed();
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
                            .add(egui::Slider::new(&mut self.project.master.volume_db, -60.0..=12.0).vertical().show_value(false))
                            .changed();
                        let (mr, _) = ui.allocate_exact_size(vec2(16.0, 110.0), Sense::hover());
                        draw_meter(ui.painter(), mr, self.master_hold);
                    });
                    ui.label(egui::RichText::new(format!("{:+.1} dB", self.project.master.volume_db)).small());
                    let pk = lin_to_db(self.master_hold[0].max(self.master_hold[1]));
                    ui.label(egui::RichText::new(if pk < -100.0 { "pico: -inf".into() } else { format!("pico: {pk:.1} dB") }).small());
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
        let names = ["Histograma (RGB + luma)", "Forma de onda (luma)", "Vectorscope", "RGB Parade"];
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
