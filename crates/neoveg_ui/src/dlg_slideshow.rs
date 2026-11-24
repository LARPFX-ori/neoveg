//! Diálogo *Criador de Apresentação de Slides* (*Inserir → Apresentação de slides...*,
//! paridade com o *Slideshow Creator* do VEGAS Pro 21/2026).

use crate::icons::{self, Icon};
use eframe::egui;
use neoveg_common::{tr, tr_args};
use neoveg_engine::model::{MediaSource, Ticks, TransitionKind};
use neoveg_engine::slideshow::{
    apply_slideshow, sort_slideshow_items, total_slideshow_duration, SlideshowItem,
    SlideshowPanCropMode, SlideshowSort, SlideshowSpec, SlideshowTransitionMode,
};

pub struct SlideshowUiState {
    pub open: bool,
    pub tab: u8,
    pub items: Vec<SlideshowItem>,
    pub selected: Option<usize>,
    pub slide_secs: f32,
    pub overlap_secs: f32,
    pub transition_mode: SlideshowTransitionMode,
    pub fixed_transition: TransitionKind,
    pub pan_crop_mode: SlideshowPanCropMode,
    pub start_at_cursor: bool,
    pub create_new_track: bool,
    pub shuffle_counter: u64,
}

impl Default for SlideshowUiState {
    fn default() -> Self {
        Self {
            open: false,
            tab: 0,
            items: Vec::new(),
            selected: None,
            slide_secs: 4.0,
            overlap_secs: 1.0,
            transition_mode: SlideshowTransitionMode::Crossfade,
            fixed_transition: TransitionKind::Dissolve,
            pan_crop_mode: SlideshowPanCropMode::FitAspect,
            start_at_cursor: true,
            create_new_track: false,
            shuffle_counter: 1,
        }
    }
}

impl SlideshowUiState {
    fn to_spec(&self) -> SlideshowSpec {
        let mode = match self.transition_mode {
            SlideshowTransitionMode::Fixed(_) => {
                SlideshowTransitionMode::Fixed(self.fixed_transition)
            }
            other => other,
        };
        SlideshowSpec {
            items: self.items.clone(),
            slide_duration: Ticks::from_secs_f64_clamped(f64::from(
                self.slide_secs.clamp(0.25, 60.0),
            )),
            overlap_duration: Ticks::from_secs_f64_clamped(f64::from(
                self.overlap_secs.clamp(0.0, 15.0),
            )),
            transition_mode: mode,
            pan_crop_mode: self.pan_crop_mode,
            start_at_cursor: self.start_at_cursor,
            create_new_track: self.create_new_track,
        }
    }
}

pub fn show_slideshow_window(app: &mut crate::App, ctx: &egui::Context) {
    if !app.dlg.slideshow.open {
        return;
    }
    let lang = app.lang();
    let theme = app.theme.clone();
    let mut open = true;
    let mut create_clicked = false;
    let mut add_from_media = false;

    let total_dur = total_slideshow_duration(&app.dlg.slideshow.to_spec());
    let total_dur_str = app.fmt_time(total_dur);

    crate::dialog_window::DialogWindow::new(tr(lang, "Criador de apresentação de slides"))
        .id(egui::Id::new("slideshow_creator_win"))
        .open(&mut open)
        .resizable(true)
        .default_size([660.0, 420.0])
        .min_size([500.0, 340.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut app.dlg.slideshow.tab, 0, tr(lang, "Imagens"));
                ui.selectable_value(&mut app.dlg.slideshow.tab, 1, tr(lang, "Configurações"));
            });
            ui.separator();

            if app.dlg.slideshow.tab == 0 {
                // Barra de ferramentas da aba Imagens (VEGAS 2026)
                ui.horizontal_wrapped(|ui| {
                    if icons::icon_button(
                        ui,
                        Icon::OpenFolder,
                        Some(tr(lang, "Adicionar arquivos...")),
                        false,
                        true,
                        &theme,
                    )
                    .clicked()
                    {
                        if let Some(files) = rfd::FileDialog::new()
                            .add_filter(
                                tr(lang, "Imagens"),
                                &["png", "jpg", "jpeg", "bmp", "webp", "tif", "tiff", "gif"],
                            )
                            .pick_files()
                        {
                            for p in files {
                                let name = p
                                    .file_name()
                                    .map(|s| s.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| "imagem".into());
                                let mtime = std::fs::metadata(&p)
                                    .and_then(|m| m.modified())
                                    .ok()
                                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                    .map(|d| d.as_secs())
                                    .unwrap_or(0);
                                app.dlg.slideshow.items.push(SlideshowItem {
                                    media_id: None,
                                    path: Some(p),
                                    name,
                                    rotation_quarters: 0,
                                    modified_secs: mtime,
                                });
                            }
                        }
                    }
                    if icons::icon_button(
                        ui,
                        Icon::MediaItem,
                        Some(tr(lang, "Da Mídia do Projeto")),
                        false,
                        true,
                        &theme,
                    )
                    .clicked()
                    {
                        add_from_media = true;
                    }
                    let sel = app.dlg.slideshow.selected;
                    let has_sel = sel.is_some_and(|i| i < app.dlg.slideshow.items.len());
                    if icons::icon_button(
                        ui,
                        Icon::CloseX,
                        None,
                        false,
                        has_sel,
                        &theme,
                    )
                    .on_hover_text(tr(lang, "Remover imagem selecionada"))
                    .clicked()
                    {
                        if let Some(i) = sel {
                            if i < app.dlg.slideshow.items.len() {
                                app.dlg.slideshow.items.remove(i);
                                app.dlg.slideshow.selected =
                                    if app.dlg.slideshow.items.is_empty() {
                                        None
                                    } else {
                                        Some(i.min(app.dlg.slideshow.items.len() - 1))
                                    };
                            }
                        }
                    }
                    ui.separator();
                    if ui
                        .add_enabled(has_sel, egui::Button::new("↺ 90°"))
                        .on_hover_text(tr(lang, "Girar 90° sentido anti-horário"))
                        .clicked()
                    {
                        if let Some(i) = sel {
                            if let Some(it) = app.dlg.slideshow.items.get_mut(i) {
                                it.rotate_ccw();
                            }
                        }
                    }
                    if ui
                        .add_enabled(has_sel, egui::Button::new("↻ 90°"))
                        .on_hover_text(tr(lang, "Girar 90° sentido horário"))
                        .clicked()
                    {
                        if let Some(i) = sel {
                            if let Some(it) = app.dlg.slideshow.items.get_mut(i) {
                                it.rotate_cw();
                            }
                        }
                    }
                    if ui
                        .add_enabled(
                            sel.is_some_and(|i| i > 0),
                            egui::Button::new("◀"),
                        )
                        .on_hover_text(tr(lang, "Mover para a esquerda"))
                        .clicked()
                    {
                        if let Some(i) = sel {
                            if i > 0 && i < app.dlg.slideshow.items.len() {
                                app.dlg.slideshow.items.swap(i - 1, i);
                                app.dlg.slideshow.selected = Some(i - 1);
                            }
                        }
                    }
                    if ui
                        .add_enabled(
                            sel.is_some_and(|i| i + 1 < app.dlg.slideshow.items.len()),
                            egui::Button::new("▶"),
                        )
                        .on_hover_text(tr(lang, "Mover para a direita"))
                        .clicked()
                    {
                        if let Some(i) = sel {
                            if i + 1 < app.dlg.slideshow.items.len() {
                                app.dlg.slideshow.items.swap(i, i + 1);
                                app.dlg.slideshow.selected = Some(i + 1);
                            }
                        }
                    }
                    ui.separator();
                    if ui.button("A→Z").clicked() {
                        sort_slideshow_items(
                            &mut app.dlg.slideshow.items,
                            SlideshowSort::NameAsc,
                            0,
                        );
                    }
                    if ui.button("Z→A").clicked() {
                        sort_slideshow_items(
                            &mut app.dlg.slideshow.items,
                            SlideshowSort::NameDesc,
                            0,
                        );
                    }
                    if ui.button(tr(lang, "Mais recente")).clicked() {
                        sort_slideshow_items(
                            &mut app.dlg.slideshow.items,
                            SlideshowSort::NewestFirst,
                            0,
                        );
                    }
                    if ui.button(tr(lang, "Mais antiga")).clicked() {
                        sort_slideshow_items(
                            &mut app.dlg.slideshow.items,
                            SlideshowSort::OldestFirst,
                            0,
                        );
                    }
                    if ui.button(tr(lang, "Embaralhar")).clicked() {
                        app.dlg.slideshow.shuffle_counter =
                            app.dlg.slideshow.shuffle_counter.wrapping_add(1);
                        sort_slideshow_items(
                            &mut app.dlg.slideshow.items,
                            SlideshowSort::Shuffle,
                            app.dlg.slideshow.shuffle_counter,
                        );
                    }
                });
                ui.separator();

                egui::ScrollArea::vertical()
                    .max_height(250.0)
                    .show(ui, |ui| {
                        if app.dlg.slideshow.items.is_empty() {
                            ui.add_space(12.0);
                            ui.label(
                                egui::RichText::new(tr(
                                    lang,
                                    "Adicione imagens do computador ou da Mídia do Projeto para montar a apresentação de slides.",
                                ))
                                .color(theme.text_muted),
                            );
                        } else {
                            for (i, item) in app.dlg.slideshow.items.iter().enumerate() {
                                let sel = app.dlg.slideshow.selected == Some(i);
                                let rot_tag = if item.rotation_quarters == 0 {
                                    String::new()
                                } else {
                                    format!("  [{}°]", (item.rotation_quarters % 4) * 90)
                                };
                                let label = format!("{:>2}. {}{}", i + 1, item.name, rot_tag);
                                if ui.selectable_label(sel, label).clicked() {
                                    app.dlg.slideshow.selected = Some(i);
                                }
                            }
                        }
                    });
            } else {
                egui::Grid::new("slideshow_settings_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(tr(lang, "Duração por slide (segundos):"));
                        ui.add(
                            egui::DragValue::new(&mut app.dlg.slideshow.slide_secs)
                                .range(0.25..=60.0)
                                .speed(0.1)
                                .max_decimals(2),
                        );
                        ui.end_row();

                        ui.label(tr(lang, "Sobreposição / transição (segundos):"));
                        ui.add(
                            egui::DragValue::new(&mut app.dlg.slideshow.overlap_secs)
                                .range(0.0..=15.0)
                                .speed(0.05)
                                .max_decimals(2),
                        );
                        ui.end_row();

                        ui.label(tr(lang, "Efeito de transição:"));
                        let cur_mode_lbl = match app.dlg.slideshow.transition_mode {
                            SlideshowTransitionMode::None => tr(lang, "Nenhum (corte seco)"),
                            SlideshowTransitionMode::Crossfade => tr(lang, "Crossfade (dissolver)"),
                            SlideshowTransitionMode::Fixed(_) => tr(lang, "Transição fixa"),
                            SlideshowTransitionMode::Random => tr(lang, "Aleatória"),
                        };
                        egui::ComboBox::from_id_salt("slideshow_tr_mode")
                            .selected_text(cur_mode_lbl)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut app.dlg.slideshow.transition_mode,
                                    SlideshowTransitionMode::None,
                                    tr(lang, "Nenhum (corte seco)"),
                                );
                                ui.selectable_value(
                                    &mut app.dlg.slideshow.transition_mode,
                                    SlideshowTransitionMode::Crossfade,
                                    tr(lang, "Crossfade (dissolver)"),
                                );
                                ui.selectable_value(
                                    &mut app.dlg.slideshow.transition_mode,
                                    SlideshowTransitionMode::Fixed(
                                        app.dlg.slideshow.fixed_transition,
                                    ),
                                    tr(lang, "Transição fixa"),
                                );
                                ui.selectable_value(
                                    &mut app.dlg.slideshow.transition_mode,
                                    SlideshowTransitionMode::Random,
                                    tr(lang, "Aleatória"),
                                );
                            });
                        ui.end_row();

                        if matches!(
                            app.dlg.slideshow.transition_mode,
                            SlideshowTransitionMode::Fixed(_)
                        ) {
                            ui.label(tr(lang, "Transição:"));
                            egui::ComboBox::from_id_salt("slideshow_fixed_tr")
                                .selected_text(app.dlg.slideshow.fixed_transition.name_in(lang))
                                .show_ui(ui, |ui| {
                                    for k in TransitionKind::ALL {
                                        if ui
                                            .selectable_value(
                                                &mut app.dlg.slideshow.fixed_transition,
                                                k,
                                                k.name_in(lang),
                                            )
                                            .changed()
                                        {
                                            app.dlg.slideshow.transition_mode =
                                                SlideshowTransitionMode::Fixed(k);
                                        }
                                    }
                                });
                            ui.end_row();
                        }

                        ui.label(tr(lang, "Enquadramento / Pan/Crop:"));
                        let pc_lbl = match app.dlg.slideshow.pan_crop_mode {
                            SlideshowPanCropMode::FitAspect => tr(lang, "Manter proporção"),
                            SlideshowPanCropMode::CropToFill => tr(lang, "Preencher quadro"),
                            SlideshowPanCropMode::GentleZoomIn => {
                                tr(lang, "Movimento suave (Ken Burns)")
                            }
                        };
                        egui::ComboBox::from_id_salt("slideshow_pc_mode")
                            .selected_text(pc_lbl)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut app.dlg.slideshow.pan_crop_mode,
                                    SlideshowPanCropMode::FitAspect,
                                    tr(lang, "Manter proporção"),
                                );
                                ui.selectable_value(
                                    &mut app.dlg.slideshow.pan_crop_mode,
                                    SlideshowPanCropMode::CropToFill,
                                    tr(lang, "Preencher quadro"),
                                );
                                ui.selectable_value(
                                    &mut app.dlg.slideshow.pan_crop_mode,
                                    SlideshowPanCropMode::GentleZoomIn,
                                    tr(lang, "Movimento suave (Ken Burns)"),
                                );
                            });
                        ui.end_row();
                    });
                ui.add_space(6.0);
                ui.checkbox(
                    &mut app.dlg.slideshow.start_at_cursor,
                    tr(lang, "Inserir na posição atual do cursor"),
                );
                ui.checkbox(
                    &mut app.dlg.slideshow.create_new_track,
                    tr(lang, "Criar nova trilha de vídeo para a apresentação"),
                );
            }

            ui.separator();
            ui.horizontal(|ui| {
                let total_s = app.dlg.slideshow.items.len().to_string();
                let sel_s = usize::from(app.dlg.slideshow.selected.is_some()).to_string();
                ui.label(
                    egui::RichText::new(tr_args(
                        lang,
                        "Total: {total}, Selecionados: {sel}  •  {dur}",
                        &[("total", &total_s), ("sel", &sel_s), ("dur", &total_dur_str)],
                    ))
                    .small()
                    .color(theme.text_muted),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::icon_button(
                        ui,
                        Icon::Slideshow,
                        Some(tr(lang, "Criar")),
                        false,
                        !app.dlg.slideshow.items.is_empty(),
                        &theme,
                    )
                    .clicked()
                    {
                        create_clicked = true;
                    }
                });
            });
        });

    if add_from_media {
        for m in &app.project.media {
            if !m.has_video {
                continue;
            }
            let already = app
                .dlg
                .slideshow
                .items
                .iter()
                .any(|it| it.media_id == Some(m.id));
            if already {
                continue;
            }
            let path = match &m.source {
                MediaSource::File { path } => Some(std::path::PathBuf::from(path)),
                _ => None,
            };
            app.dlg.slideshow.items.push(SlideshowItem {
                media_id: Some(m.id),
                path,
                name: m.name.clone(),
                rotation_quarters: 0,
                modified_secs: 0,
            });
        }
    }

    if create_clicked {
        let spec = app.dlg.slideshow.to_spec();
        let (count, new_media_ids) = apply_slideshow(&mut app.session, &spec);
        for mid in new_media_ids {
            if let Some(m) = app.project.media(mid).cloned() {
                app.spawn_media_load(m);
            }
        }
        app.sync_after_session();
        let n_s = count.to_string();
        app.set_status(tr_args(
            lang,
            "Apresentação de slides criada ({n} slide(s)).",
            &[("n", &n_s)],
        ));
        app.dlg.slideshow.open = false;
    } else if !open {
        app.dlg.slideshow.open = false;
    }
}
