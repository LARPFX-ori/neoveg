//! Interface do **Neoveg Encoder** (`neoveg_ui`) integrada à identidade visual
//! e aos 7 temas do Neoveg (`AGENTS.md` §0, §1, §2.2, §3.3).

use crate::app::{rgba_to_color_image, App, DockTab};
use crate::icons::{self, Icon};
use crate::theme::ThemeColors;
use eframe::egui;
use egui::{Color32, RichText, TextureHandle};
use neoveg_common::time::format_ticks;
use neoveg_common::{tr, tr_args, Language};
use neoveg_engine::encoder::effects::{
    CropAspect, GradePreset, NameFormat, OverlayPosition, SourceScalingMode, TimecodeSource,
};
use neoveg_engine::encoder::presets::{
    export_preset_file, import_preset_file, system_presets, EncoderPreset, PresetCatalog,
    PresetCategory,
};
use neoveg_engine::encoder::watch::{scan_watch_folders, WatchFolder};
use neoveg_engine::encoder::worker::{
    encode_item_sync, preview_transformed_frame, spawn_queue, EncoderRuntime,
};
use neoveg_engine::encoder::{
    encoder_log_paths, load_state, presets_store_path, presets_user_dir, queue_store_path,
    save_state, AlphaInterpret, EncoderState, JobStatus, OutputJob, QueueSource, SourceRangeMode,
};
use neoveg_engine::engine::{video::render_frame, Frame};
use neoveg_engine::media::{AUDIO_EXTS, IMAGE_EXTS, SYSTEM_CONTAINER_EXTS, VIDEO_EXTS};
use neoveg_engine::model::{FrameRate, Project, Ticks};
use neoveg_engine::render::{RenderFormat, WavBits};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EncoderSubView {
    #[default]
    AllPanels,
    QueueAndLive,
    Presets,
    WatchFolders,
}

impl EncoderSubView {
    pub const ALL: [Self; 4] = [
        Self::AllPanels,
        Self::QueueAndLive,
        Self::Presets,
        Self::WatchFolders,
    ];

    pub fn name_in(self, lang: Language) -> &'static str {
        let key = match self {
            Self::AllPanels => "Painéis (4)",
            Self::QueueAndLive => "Fila + Ao Vivo",
            Self::Presets => "Predefinições",
            Self::WatchFolders => "Pastas Monitoradas",
        };
        tr(lang, key)
    }
}

fn preset_display_name(pr: &EncoderPreset, lang: Language) -> &str {
    if pr.system {
        tr(lang, &pr.name)
    } else {
        &pr.name
    }
}

fn preset_display_notes(pr: &EncoderPreset, lang: Language) -> &str {
    if pr.system && !pr.notes.is_empty() {
        tr(lang, &pr.notes)
    } else {
        &pr.notes
    }
}

fn output_preset_label(catalog: &PresetCatalog, out: &OutputJob, lang: Language) -> String {
    if let Some(pr) = catalog.resolve(&out.preset_id) {
        preset_display_name(pr, lang).to_string()
    } else {
        out.preset_name.clone()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ExportSettingsTab {
    #[default]
    Video,
    Audio,
    Effects,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SettingsPreviewPane {
    Source,
    #[default]
    Output,
}

pub struct ExportSettingsModal {
    pub item_id: u64,
    pub output_id: u64,
    pub draft: OutputJob,
    pub tab: ExportSettingsTab,
    pub preview_pane: SettingsPreviewPane,
    pub preview_sec: f32,
    pub preview_tex: Option<TextureHandle>,
    pub preview_cache_key: String,
    pub src_frame_cache: Option<Frame>,
    pub src_frame_key: String,
}

pub struct EncoderPanelState {
    pub state: EncoderState,
    pub catalog: PresetCatalog,
    pub runtime: Arc<EncoderRuntime>,
    pub sub_view: EncoderSubView,
    pub selected_item: Option<u64>,
    pub selected_output: Option<u64>,
    pub selected_preset: Option<String>,
    pub preset_search: String,
    pub live_tex: Option<TextureHandle>,
    pub export_modal: Option<ExportSettingsModal>,
    pub interpret_item_id: Option<u64>,
    pub prefs_open: bool,
    pub log_modal: Option<(&'static str, PathBuf, String)>,
    pub create_preset_open: bool,
    pub new_preset_name: String,
    pub create_group_open: bool,
    pub new_group_name: String,
    pub new_group_presets: Vec<String>,
    pub rename_preset_target: Option<(String, String)>,
    pub watch_timer: f64,
    pub idle_timer: f64,
    pub persist_dirty: bool,
}

impl Default for EncoderPanelState {
    fn default() -> Self {
        Self {
            state: EncoderState::default(),
            catalog: PresetCatalog::default(),
            runtime: EncoderRuntime::new(),
            sub_view: EncoderSubView::AllPanels,
            selected_item: None,
            selected_output: None,
            selected_preset: Some("sys.web_1080p_hq".to_string()),
            preset_search: String::new(),
            live_tex: None,
            export_modal: None,
            interpret_item_id: None,
            prefs_open: false,
            log_modal: None,
            create_preset_open: false,
            new_preset_name: "Minha Predefinição".to_string(),
            create_group_open: false,
            new_group_name: "Meu Grupo".to_string(),
            new_group_presets: vec!["sys.web_1080p_hq".into()],
            rename_preset_target: None,
            watch_timer: 0.0,
            idle_timer: 0.0,
            persist_dirty: false,
        }
    }
}

impl EncoderPanelState {
    pub fn load_from_disk() -> Self {
        if cfg!(test) {
            return Self::default();
        }
        let state = load_state(&queue_store_path());
        let catalog = PresetCatalog::load(&presets_store_path(), &presets_user_dir());
        Self {
            state,
            catalog,
            ..Self::default()
        }
    }

    pub fn save_to_disk(&mut self) {
        if cfg!(test) {
            self.persist_dirty = false;
            return;
        }
        let _ = save_state(&self.state, &queue_store_path());
        let _ = self.catalog.save(&presets_store_path());
        self.persist_dirty = false;
    }

    pub fn enqueue_project_snapshot(
        &mut self,
        title: String,
        project_path: Option<PathBuf>,
        project: Project,
        in_out: Option<(Ticks, Ticks)>,
    ) -> u64 {
        let id =
            self.state
                .add_project_snapshot(title, project_path, project, in_out, &self.catalog);
        self.selected_item = Some(id);
        self.selected_output = self
            .state
            .items
            .iter()
            .find(|i| i.id == id)
            .and_then(|i| i.outputs.first())
            .map(|o| o.id);
        self.persist_dirty = true;
        id
    }

    pub fn start_queue(&mut self, cache: &Arc<neoveg_engine::media::MediaCache>) {
        if self.runtime.is_running() {
            if self.runtime.is_paused() {
                self.runtime.toggle_pause();
            }
            return;
        }
        let pending: Vec<_> = self
            .state
            .items
            .iter()
            .filter(|it| it.outputs.iter().any(|o| o.status == JobStatus::Ready))
            .cloned()
            .collect();
        if pending.is_empty() {
            return;
        }
        spawn_queue(
            pending,
            self.state.prefs.clone(),
            self.runtime.clone(),
            Some(cache.clone()),
        );
    }

    pub fn tick(
        &mut self,
        ctx: &egui::Context,
        dt: f64,
        cache: &Arc<neoveg_engine::media::MediaCache>,
        lang: Language,
    ) -> Option<String> {
        let mut status_msg = None;
        for upd in self.runtime.drain_updates() {
            if let Some(item) = self.state.items.iter_mut().find(|i| i.id == upd.item_id) {
                if let Some(out) = item.outputs.iter_mut().find(|o| o.id == upd.output_id) {
                    out.status = upd.status;
                    out.progress = upd.progress;
                    out.elapsed_secs = upd.elapsed_secs;
                    out.eta_secs = upd.eta_secs;
                    out.last_message = upd.message;
                    if !upd.warnings.is_empty() {
                        out.warnings = upd.warnings;
                    }
                    if let Some(p) = upd.final_output_path {
                        out.output_path = p;
                    }
                    self.persist_dirty = true;
                }
            }
        }
        // Sincroniza barras de progresso em tempo real sem esperar fim do arquivo.
        {
            let active = neoveg_engine::util::lock(&self.runtime.active_progress);
            let started = *neoveg_engine::util::lock(&self.runtime.started_at);
            for item in &mut self.state.items {
                for out in &mut item.outputs {
                    if let Some(prog) = active.get(&out.id) {
                        let frac = prog.fraction().clamp(0.0, 1.0);
                        out.progress = frac;
                        if let Some(st) = started {
                            let el = st.elapsed().as_secs_f64();
                            out.elapsed_secs = el;
                            out.eta_secs = if frac > 0.02 {
                                Some(((el / frac as f64) - el).max(0.0))
                            } else {
                                None
                            };
                        }
                    }
                }
            }
        }
        if self.runtime.is_running() {
            if let Some(pv) = self.runtime.take_preview() {
                if let Some(img) = image::RgbaImage::from_raw(pv.width, pv.height, pv.rgba) {
                    let ci = rgba_to_color_image(&img);
                    self.live_tex =
                        Some(ctx.load_texture("encoder_live_pv", ci, egui::TextureOptions::LINEAR));
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        } else if self.runtime.finished_chime.swap(false, Ordering::Relaxed) {
            status_msg =
                Some(tr(lang, "Neoveg Encoder: fila de codificação concluída.").to_string());
        }

        // Varredura periódica das Pastas Monitoradas (Watch Folders)
        if !self.state.watch_folders.is_empty() {
            self.watch_timer += dt;
            if self.watch_timer >= 2.0 {
                self.watch_timer = 0.0;
                let (added, auto_start) = scan_watch_folders(&mut self.state, &self.catalog);
                if added > 0 {
                    self.persist_dirty = true;
                    let added_s = added.to_string();
                    status_msg = Some(tr_args(
                        lang,
                        "Pastas monitoradas: {added} arquivo(s) adicionado(s) à fila",
                        &[("added", &added_s)],
                    ));
                    if auto_start && !self.runtime.is_running() {
                        self.start_queue(cache);
                    }
                }
            }
        }

        // Início automático quando ocioso (preferência opcional)
        if let Some(idle_secs) = self.state.prefs.auto_start_idle_secs {
            let has_ready = self
                .state
                .items
                .iter()
                .any(|i| i.outputs.iter().any(|o| o.status == JobStatus::Ready));
            let user_active = ctx.input(|i| i.pointer.any_down() || !i.keys_down.is_empty());
            if user_active || !has_ready || self.runtime.is_running() {
                self.idle_timer = 0.0;
            } else {
                self.idle_timer += dt;
                if self.idle_timer >= idle_secs as f64 {
                    self.idle_timer = 0.0;
                    self.start_queue(cache);
                }
            }
        }

        if self.persist_dirty && !self.runtime.is_running() {
            self.save_to_disk();
        }
        status_msg
    }
}

pub(crate) fn media_filter_extensions() -> Vec<&'static str> {
    let mut exts = Vec::new();
    exts.extend_from_slice(VIDEO_EXTS);
    exts.extend_from_slice(SYSTEM_CONTAINER_EXTS);
    exts.extend_from_slice(AUDIO_EXTS);
    exts.extend_from_slice(IMAGE_EXTS);
    exts
}

fn status_color(st: JobStatus, theme: &ThemeColors) -> Color32 {
    match st {
        JobStatus::Done => theme.timecode_fg,
        JobStatus::DoneWithWarnings => Color32::from_rgb(220, 175, 45),
        JobStatus::Encoding => theme.accent,
        JobStatus::Failed => Color32::from_rgb(220, 75, 75),
        JobStatus::Stopped | JobStatus::Skipped => theme.text_muted,
        JobStatus::Ready => theme.text_primary,
    }
}

fn format_eta(secs: Option<f64>) -> String {
    match secs {
        Some(s) if s.is_finite() && s >= 0.0 => {
            let total = s.round() as u64;
            let m = total / 60;
            let sec = total % 60;
            format!("{m:02}:{sec:02}")
        }
        _ => "--:--".to_string(),
    }
}

impl App {
    pub(crate) fn send_timeline_to_encoder(&mut self) {
        let lang = self.lang();
        let in_out = self
            .session
            .time_sel
            .or(self.session.project.loop_region)
            .filter(|(a, b)| b > a);
        let title = self
            .session
            .path
            .as_deref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| tr(lang, "Timeline Atual").to_string());
        self.encoder.enqueue_project_snapshot(
            title,
            self.session.path.clone(),
            self.session.project.clone(),
            in_out,
        );
        // o Encoder abre na janela própria, fora da área de janelas
        self.open_window_panel(None, DockTab::Encoder);
        self.set_status(tr(lang, "Timeline enviada para a fila do Neoveg Encoder"));
    }

    pub(crate) fn encoder_tab(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let theme = self.theme.clone();
        let running = self.encoder.runtime.is_running();
        let paused = self.encoder.runtime.is_paused();

        // Barra superior do estúdio Neoveg Encoder (estilo VEGAS Pro)
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(tr(lang, "Neoveg Encoder"))
                    .strong()
                    .color(theme.accent),
            );
            ui.separator();

            if icons::small_icon_button(
                ui,
                Icon::ImportMedia,
                Some(tr(lang, "+ Arquivo...")),
                false,
                &theme,
            )
            .clicked()
            {
                let exts = media_filter_extensions();
                if let Some(files) = rfd::FileDialog::new()
                    .add_filter(tr(lang, "Mídia e Sequências"), &exts)
                    .pick_files()
                {
                    let mut n = 0usize;
                    for p in files {
                        if let Ok(id) = self.encoder.state.add_media_file(p, &self.encoder.catalog)
                        {
                            self.encoder.selected_item = Some(id);
                            n += 1;
                        }
                    }
                    self.encoder.persist_dirty = true;
                    if n > 0 {
                        let n_s = n.to_string();
                        self.set_status(tr_args(
                            lang,
                            "{n} arquivo(s) adicionado(s) ao Neoveg Encoder",
                            &[("n", &n_s)],
                        ));
                    }
                }
            }
            if icons::small_icon_button(
                ui,
                Icon::OpenFolder,
                Some(tr(lang, "+ Sequência...")),
                false,
                &theme,
            )
            .clicked()
            {
                if let Some(first) = rfd::FileDialog::new()
                    .add_filter(tr(lang, "Quadros de imagem"), IMAGE_EXTS)
                    .pick_file()
                {
                    let fps = self.encoder.state.prefs.undefined_media_fps;
                    match self
                        .encoder
                        .state
                        .add_image_sequence(first, fps, &self.encoder.catalog)
                    {
                        Ok(id) => {
                            self.encoder.selected_item = Some(id);
                            self.encoder.persist_dirty = true;
                        }
                        Err(e) => {
                            let err_s = e.to_string();
                            self.set_status(tr_args(
                                lang,
                                "Erro na sequência: {e}",
                                &[("e", &err_s)],
                            ));
                        }
                    }
                }
            }
            if ui
                .small_button(tr(lang, "+ Timeline Atual"))
                .on_hover_text(tr(
                    lang,
                    "Adiciona a timeline do projeto à fila de codificação",
                ))
                .clicked()
            {
                self.send_timeline_to_encoder();
            }

            ui.separator();
            let play_label = tr(
                lang,
                if running && paused {
                    "Retomar Fila"
                } else if running {
                    "Codificando..."
                } else {
                    "Iniciar Fila"
                },
            );
            if icons::small_icon_button(
                ui,
                Icon::Play,
                Some(play_label),
                running && !paused,
                &theme,
            )
            .clicked()
            {
                self.encoder.start_queue(&self.media);
            }
            if icons::small_icon_button(ui, Icon::Pause, Some(tr(lang, "Pausar")), paused, &theme)
                .clicked()
                && running
            {
                self.encoder.runtime.toggle_pause();
            }
            if icons::small_icon_button(ui, Icon::Stop, Some(tr(lang, "Parar Fila")), false, &theme)
                .clicked()
                && running
            {
                self.encoder.runtime.stop_queue();
            }
            if running && ui.small_button(tr(lang, "Parar Atual")).clicked() {
                self.encoder.runtime.stop_current();
            }

            ui.separator();
            for sv in EncoderSubView::ALL {
                ui.selectable_value(&mut self.encoder.sub_view, sv, sv.name_in(lang));
            }

            ui.separator();
            if icons::small_icon_button(
                ui,
                Icon::SaveDisk,
                Some(tr(lang, "Salvar Fila")),
                false,
                &theme,
            )
            .clicked()
            {
                self.encoder.save_to_disk();
                self.set_status(tr(lang, "Fila do Neoveg Encoder salva"));
            }
            if ui.small_button(tr(lang, "Preferências...")).clicked() {
                self.encoder.prefs_open = true;
            }
            if ui.small_button(tr(lang, "Registro")).clicked() {
                let (ok_log, _) = encoder_log_paths();
                let txt = std::fs::read_to_string(&ok_log).unwrap_or_else(|_| {
                    tr(lang, "(Nenhuma entrada registrada ainda.)").to_string()
                });
                self.encoder.log_modal = Some(("Registro de Codificação", ok_log, txt));
            }
            if ui.small_button(tr(lang, "Erros")).clicked() {
                let (_, err_log) = encoder_log_paths();
                let txt = std::fs::read_to_string(&err_log)
                    .unwrap_or_else(|_| tr(lang, "(Nenhum erro registrado.)").to_string());
                self.encoder.log_modal = Some(("Registro de Erros do Encoder", err_log, txt));
            }
        });
        ui.separator();

        let avail = ui.available_size();
        match self.encoder.sub_view {
            EncoderSubView::AllPanels => {
                if avail.x < 720.0 {
                    // Em janelas estreitas empilha para preservar legibilidade.
                    self.encoder_queue_section(ui, &theme, 160.0);
                    ui.separator();
                    self.encoder_live_section(ui, &theme);
                    ui.separator();
                    self.encoder_presets_section(ui, &theme, 150.0);
                    ui.separator();
                    self.encoder_watch_section(ui, &theme);
                } else {
                    ui.columns(2, |cols| {
                        self.encoder_queue_section(
                            &mut cols[0],
                            &theme,
                            (avail.y * 0.50).max(120.0),
                        );
                        cols[0].separator();
                        self.encoder_watch_section(&mut cols[0], &theme);

                        self.encoder_live_section(&mut cols[1], &theme);
                        cols[1].separator();
                        self.encoder_presets_section(
                            &mut cols[1],
                            &theme,
                            (avail.y * 0.48).max(120.0),
                        );
                    });
                }
            }
            EncoderSubView::QueueAndLive => {
                if avail.x < 680.0 {
                    self.encoder_queue_section(ui, &theme, (avail.y * 0.55).max(120.0));
                    ui.separator();
                    self.encoder_live_section(ui, &theme);
                } else {
                    ui.columns(2, |cols| {
                        self.encoder_queue_section(
                            &mut cols[0],
                            &theme,
                            (avail.y - 24.0).max(120.0),
                        );
                        self.encoder_live_section(&mut cols[1], &theme);
                    });
                }
            }
            EncoderSubView::Presets => {
                self.encoder_presets_section(ui, &theme, (avail.y - 24.0).max(150.0));
            }
            EncoderSubView::WatchFolders => {
                self.encoder_watch_section(ui, &theme);
            }
        }
    }

    fn encoder_queue_section(&mut self, ui: &mut egui::Ui, theme: &ThemeColors, max_h: f32) {
        let lang = self.lang();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tr(lang, "Fila de Codificação")).strong());
            let sel_item = self.encoder.selected_item;
            let sel_out = self.encoder.selected_output;
            ui.add_enabled_ui(sel_item.is_some(), |ui| {
                if ui.small_button(tr(lang, "+ Saída")).clicked() {
                    if let Some(iid) = sel_item {
                        let pid = self
                            .encoder
                            .selected_preset
                            .as_deref()
                            .unwrap_or("sys.web_1080p_hq");
                        let preset = self
                            .encoder
                            .catalog
                            .resolve(pid)
                            .cloned()
                            .unwrap_or_else(|| system_presets()[0].clone());
                        if let Some(oid) = self.encoder.state.add_output(iid, &preset) {
                            self.encoder.selected_output = Some(oid);
                            self.encoder.persist_dirty = true;
                        }
                    }
                }
                if ui.small_button(tr(lang, "Configurações...")).clicked() {
                    if let (Some(iid), Some(oid)) = (sel_item, sel_out) {
                        self.open_export_settings_modal(iid, oid);
                    }
                }
                if ui.small_button(tr(lang, "Interpretar...")).clicked() {
                    self.encoder.interpret_item_id = sel_item;
                }
                if ui.small_button(tr(lang, "Duplicar")).clicked() {
                    if let Some(iid) = sel_item {
                        if let Some(oid) = sel_out {
                            self.encoder.selected_output =
                                self.encoder.state.duplicate_output(iid, oid);
                        } else {
                            self.encoder.selected_item = self.encoder.state.duplicate_item(iid);
                        }
                        self.encoder.persist_dirty = true;
                    }
                }
                if ui.small_button(tr(lang, "Ignorar")).clicked() {
                    if let Some(iid) = sel_item {
                        self.encoder.state.set_skipped(iid, sel_out, true);
                        self.encoder.persist_dirty = true;
                    }
                }
                if ui.small_button(tr(lang, "Redefinir")).clicked() {
                    if let Some(iid) = sel_item {
                        self.encoder.state.reset_status(iid, sel_out);
                        self.encoder.persist_dirty = true;
                    }
                }
                if ui.small_button("↑").clicked() {
                    if let Some(iid) = sel_item {
                        self.encoder.state.move_item(iid, -1);
                        self.encoder.persist_dirty = true;
                    }
                }
                if ui.small_button("↓").clicked() {
                    if let Some(iid) = sel_item {
                        self.encoder.state.move_item(iid, 1);
                        self.encoder.persist_dirty = true;
                    }
                }
                if ui.small_button(tr(lang, "Remover")).clicked() {
                    if let Some(iid) = sel_item {
                        if let Some(oid) = sel_out {
                            self.encoder.state.remove_output(iid, oid);
                            self.encoder.selected_output = None;
                        } else {
                            self.encoder.state.remove_item(iid);
                            self.encoder.selected_item = None;
                        }
                        self.encoder.persist_dirty = true;
                    }
                }
            });
            if ui.small_button(tr(lang, "Limpar Concluídos")).clicked() {
                self.encoder.state.clear_completed();
                self.encoder.persist_dirty = true;
            }
        });

        let mut open_settings_req: Option<(u64, u64)> = None;
        let mut pick_path_req: Option<(u64, u64, PathBuf)> = None;
        let mut format_change_req: Option<(u64, u64, RenderFormat)> = None;
        let mut preset_change_req: Option<(u64, u64, String)> = None;
        let mut reset_req: Option<(u64, u64)> = None;
        let mut open_log_req = false;

        egui::ScrollArea::vertical()
            .id_salt("encoder_queue_scroll")
            .max_height(max_h)
            .show(ui, |ui| {
                if self.encoder.state.items.is_empty() {
                    ui.label(
                        RichText::new(tr(
                            lang,
                            "Fila vazia. Clique em '+ Arquivo...', '+ Timeline Atual' ou adicione uma Pasta Monitorada.",
                        ))
                        .weak(),
                    );
                    return;
                }
                let all_presets: Vec<EncoderPreset> =
                    self.encoder.catalog.all_presets().cloned().collect();
                for item in &mut self.encoder.state.items {
                    let is_item_sel = self.encoder.selected_item == Some(item.id);
                    let agg = item.aggregate_status();
                    ui.horizontal(|ui| {
                        let arrow = if item.expanded { "▼" } else { "▶" };
                        if ui.small_button(arrow).clicked() {
                            item.expanded = !item.expanded;
                        }
                        let src_disp = item.source.display_name();
                        let out_count = item.outputs.len().to_string();
                        if ui
                            .selectable_label(
                                is_item_sel && self.encoder.selected_output.is_none(),
                                tr_args(
                                    lang,
                                    "{src} ({n} saída(s))",
                                    &[("src", &src_disp), ("n", &out_count)],
                                ),
                            )
                            .clicked()
                        {
                            self.encoder.selected_item = Some(item.id);
                            self.encoder.selected_output =
                                item.outputs.first().map(|o| o.id);
                        }
                        ui.label(
                            RichText::new(agg.name_in(lang))
                                .small()
                                .color(status_color(agg, theme)),
                        );
                    });

                    if !item.expanded {
                        continue;
                    }
                    ui.indent(("enc_item", item.id), |ui| {
                        for out in &item.outputs {
                            let is_out_sel = self.encoder.selected_item == Some(item.id)
                                && self.encoder.selected_output == Some(out.id);
                            let busy = out.status == JobStatus::Encoding;
                            ui.horizontal_wrapped(|ui| {
                                if ui.selectable_label(is_out_sel, tr(lang, "Saída")).clicked() {
                                    self.encoder.selected_item = Some(item.id);
                                    self.encoder.selected_output = Some(out.id);
                                }
                                ui.add_enabled_ui(!busy, |ui| {
                                    let mut cur_fmt = out.format;
                                    egui::ComboBox::from_id_salt(("enc_fmt", out.id))
                                        .width(145.0)
                                        .selected_text(cur_fmt.name_in(lang))
                                        .show_ui(ui, |ui| {
                                            for f in RenderFormat::ALL {
                                                if ui
                                                    .selectable_value(
                                                        &mut cur_fmt,
                                                        f,
                                                        f.name_in(lang),
                                                    )
                                                    .clicked()
                                                {
                                                    format_change_req =
                                                        Some((item.id, out.id, f));
                                                }
                                            }
                                        });
                                    let sel_pre_lbl =
                                        output_preset_label(&self.encoder.catalog, out, lang);
                                    egui::ComboBox::from_id_salt(("enc_pre", out.id))
                                        .width(165.0)
                                        .selected_text(sel_pre_lbl)
                                        .show_ui(ui, |ui| {
                                            for pr in &all_presets {
                                                if ui
                                                    .selectable_label(
                                                        out.preset_id == pr.id,
                                                        preset_display_name(pr, lang),
                                                    )
                                                    .clicked()
                                                {
                                                    preset_change_req =
                                                        Some((item.id, out.id, pr.id.clone()));
                                                }
                                            }
                                        });
                                });

                                let fname = out
                                    .output_path
                                    .file_name()
                                    .map(|s| s.to_string_lossy().to_string())
                                    .unwrap_or_else(|| out.output_path.display().to_string());
                                if ui
                                    .link(fname)
                                    .on_hover_text(out.output_path.display().to_string())
                                    .clicked()
                                    && !busy
                                {
                                    pick_path_req =
                                        Some((item.id, out.id, out.output_path.clone()));
                                }
                                if ui.small_button(tr(lang, "Ajustar...")).clicked() {
                                    open_settings_req = Some((item.id, out.id));
                                }

                                let st_resp = ui.add(
                                    egui::Button::new(
                                        RichText::new(out.status.name_in(lang))
                                            .small()
                                            .color(status_color(out.status, theme)),
                                    )
                                    .frame(false),
                                );
                                if let Some(msg) = &out.last_message {
                                    st_resp.clone().on_hover_text(msg);
                                }
                                if st_resp.clicked() {
                                    open_log_req = true;
                                }
                                if out.status == JobStatus::Encoding
                                    || (out.progress > 0.0 && out.progress < 1.0)
                                {
                                    ui.add(
                                        egui::ProgressBar::new(out.progress.clamp(0.0, 1.0))
                                            .desired_width(80.0)
                                            .show_percentage(),
                                    );
                                }
                                if (out.status == JobStatus::Stopped
                                    || out.status == JobStatus::Failed)
                                    && ui.small_button(tr(lang, "Reativar")).clicked()
                                {
                                    reset_req = Some((item.id, out.id));
                                }
                            });
                        }
                    });
                }
            });

        if let Some((iid, oid)) = open_settings_req {
            self.open_export_settings_modal(iid, oid);
        }
        if let Some((iid, oid, cur)) = pick_path_req {
            let ext = cur
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("mp4")
                .to_string();
            let mut dlg = rfd::FileDialog::new().add_filter(&ext, &[ext.as_str()]);
            if let Some(dir) = cur.parent() {
                if dir.is_dir() {
                    dlg = dlg.set_directory(dir);
                }
            }
            if let Some(name) = cur.file_name().and_then(|n| n.to_str()) {
                dlg = dlg.set_file_name(name);
            }
            if let Some(picked) = dlg.save_file() {
                if let Some(it) = self.encoder.state.items.iter_mut().find(|i| i.id == iid) {
                    if let Some(o) = it.outputs.iter_mut().find(|o| o.id == oid) {
                        o.output_path = picked;
                        self.encoder.persist_dirty = true;
                    }
                }
            }
        }
        if let Some((iid, oid, fmt)) = format_change_req {
            if let Some(it) = self.encoder.state.items.iter_mut().find(|i| i.id == iid) {
                if let Some(o) = it.outputs.iter_mut().find(|o| o.id == oid) {
                    o.format = fmt;
                    let fmt_n = fmt.name_in(lang);
                    o.preset_name = tr_args(lang, "Personalizado ({fmt})", &[("fmt", &fmt_n)]);
                    o.output_path = o.output_path.with_extension(fmt.ext());
                    if fmt.is_audio_only() {
                        o.include_video = false;
                        o.include_audio = true;
                    }
                    self.encoder.persist_dirty = true;
                }
            }
        }
        if let Some((iid, oid, pid)) = preset_change_req {
            if let Some(pr) = self.encoder.catalog.resolve(&pid).cloned() {
                if let Some(it) = self.encoder.state.items.iter_mut().find(|i| i.id == iid) {
                    let hint = (
                        self.session.project.settings.width,
                        self.session.project.settings.height,
                    );
                    if let Some(o) = it.outputs.iter_mut().find(|o| o.id == oid) {
                        o.apply_preset(&pr, hint);
                        self.encoder.persist_dirty = true;
                    }
                }
            }
        }
        if let Some((iid, oid)) = reset_req {
            self.encoder.state.reset_status(iid, Some(oid));
            self.encoder.persist_dirty = true;
        }
        if open_log_req {
            let (ok_log, err_log) = encoder_log_paths();
            let target = if err_log.exists() { err_log } else { ok_log };
            let txt = std::fs::read_to_string(&target)
                .unwrap_or_else(|_| tr(lang, "(Nenhuma entrada registrada.)").into());
            self.encoder.log_modal = Some(("Registro de Codificação", target, txt));
        }
    }

    fn encoder_live_section(&mut self, ui: &mut egui::Ui, theme: &ThemeColors) {
        let lang = self.lang();
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr(lang, "Painel Codificação (Ao Vivo)")).strong());
            ui.checkbox(
                &mut self.encoder.state.prefs.preview_during_encoding,
                tr(lang, "Prévia visual"),
            );
        });

        let active_rows: Vec<(String, String, JobStatus, f32, f64, Option<f64>)> = self
            .encoder
            .state
            .items
            .iter()
            .flat_map(|item| {
                let sname = item.source.display_name();
                let cat = &self.encoder.catalog;
                item.outputs.iter().filter_map(move |o| {
                    if o.status == JobStatus::Encoding
                        || (o.progress > 0.0 && o.status == JobStatus::Done)
                    {
                        Some((
                            sname.clone(),
                            output_preset_label(cat, o, lang),
                            o.status,
                            o.progress,
                            o.elapsed_secs,
                            o.eta_secs,
                        ))
                    } else {
                        None
                    }
                })
            })
            .collect();

        ui.horizontal(|ui| {
            let pv_size = egui::vec2(176.0, 99.0);
            if let Some(tex) = &self.encoder.live_tex {
                ui.image((tex.id(), pv_size));
            } else {
                let (rect, _) = ui.allocate_exact_size(pv_size, egui::Sense::hover());
                ui.painter().rect_filled(rect, 3.0, theme.panel_fill);
                ui.painter().rect_stroke(
                    rect,
                    3.0,
                    egui::Stroke::new(1.0_f32, theme.border),
                    egui::StrokeKind::Inside,
                );
                let msg = tr(
                    lang,
                    if self.encoder.runtime.is_running() {
                        if self.encoder.runtime.is_paused() {
                            "Pausado"
                        } else {
                            "Renderizando..."
                        }
                    } else {
                        "Ocioso"
                    },
                );
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    msg,
                    egui::FontId::proportional(12.0),
                    theme.text_muted,
                );
            }

            ui.vertical(|ui| {
                if active_rows.is_empty() {
                    ui.label(
                        RichText::new(tr(lang, "Nenhuma codificação ativa no momento.")).weak(),
                    );
                } else {
                    for (src_n, pre_n, st, prog, elapsed, eta) in active_rows.iter().take(4) {
                        ui.label(
                            RichText::new(format!("{src_n} — {pre_n}"))
                                .small()
                                .color(status_color(*st, theme)),
                        );
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::ProgressBar::new(prog.clamp(0.0, 1.0))
                                    .desired_width(140.0)
                                    .show_percentage(),
                            );
                            if self.encoder.state.prefs.show_elapsed_time {
                                let el_s = format!("{elapsed:.1}");
                                let eta_s = format_eta(*eta);
                                ui.label(
                                    RichText::new(tr_args(
                                        lang,
                                        "{elapsed}s | ETA {eta}",
                                        &[("elapsed", &el_s), ("eta", &eta_s)],
                                    ))
                                    .small()
                                    .weak(),
                                );
                            }
                        });
                    }
                }
            });
        });
    }

    fn encoder_presets_section(&mut self, ui: &mut egui::Ui, _theme: &ThemeColors, max_h: f32) {
        let lang = self.lang();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tr(lang, "Navegador de Predefinições")).strong());
            ui.add(
                egui::TextEdit::singleline(&mut self.encoder.preset_search)
                    .desired_width(130.0)
                    .hint_text(tr(lang, "Pesquisar...")),
            );
            if ui.small_button(tr(lang, "+ Predefinição")).clicked() {
                self.encoder.create_preset_open = true;
            }
            if ui.small_button(tr(lang, "+ Grupo")).clicked() {
                self.encoder.create_group_open = true;
            }
            if ui.small_button(tr(lang, "Importar")).clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter(tr(lang, "Predefinição Neoveg"), &["neovegpreset"])
                    .pick_file()
                {
                    match import_preset_file(&p) {
                        Ok(pr) => {
                            let id = self.encoder.catalog.save_custom(pr);
                            self.encoder.selected_preset = Some(id);
                            self.encoder.persist_dirty = true;
                        }
                        Err(e) => {
                            let err_s = e.to_string();
                            self.set_status(tr_args(
                                lang,
                                "Erro ao importar predefinição: {e}",
                                &[("e", &err_s)],
                            ));
                        }
                    }
                }
            }
            if ui.small_button(tr(lang, "Exportar")).clicked() {
                if let Some(pid) = &self.encoder.selected_preset {
                    if let Some(pr) = self.encoder.catalog.resolve(pid).cloned() {
                        if let Some(dest) = rfd::FileDialog::new()
                            .add_filter(tr(lang, "Predefinição Neoveg"), &["neovegpreset"])
                            .set_file_name(format!("{}.neovegpreset", pr.name))
                            .save_file()
                        {
                            if let Err(e) = export_preset_file(&pr, &dest) {
                                let err_s = e.to_string();
                                self.set_status(tr_args(
                                    lang,
                                    "Erro ao exportar: {e}",
                                    &[("e", &err_s)],
                                ));
                            }
                        }
                    }
                }
            }
        });

        let sel_item = self.encoder.selected_item;
        ui.horizontal(|ui| {
            ui.add_enabled_ui(
                sel_item.is_some() && self.encoder.selected_preset.is_some(),
                |ui| {
                    if ui.small_button(tr(lang, "Aplicar à Origem")).clicked() {
                        if let (Some(iid), Some(pid)) =
                            (sel_item, self.encoder.selected_preset.clone())
                        {
                            self.encoder.state.apply_preset_or_group(
                                iid,
                                &self.encoder.catalog,
                                &pid,
                                false,
                            );
                            self.encoder.persist_dirty = true;
                        }
                    }
                    if ui.small_button(tr(lang, "Adicionar como Saída")).clicked() {
                        if let (Some(iid), Some(pid)) =
                            (sel_item, self.encoder.selected_preset.clone())
                        {
                            self.encoder.state.apply_preset_or_group(
                                iid,
                                &self.encoder.catalog,
                                &pid,
                                true,
                            );
                            self.encoder.persist_dirty = true;
                        }
                    }
                },
            );
            if let Some(pid) = self.encoder.selected_preset.clone() {
                if let Some(u) = self.encoder.catalog.user.iter().find(|u| u.id == pid) {
                    let cur_name = u.name.clone();
                    if ui.small_button(tr(lang, "Renomear")).clicked() {
                        self.encoder.rename_preset_target = Some((pid.clone(), cur_name));
                    }
                    if ui.small_button(tr(lang, "Criar Apelido")).clicked() {
                        let base_name = self
                            .encoder
                            .catalog
                            .resolve(&pid)
                            .map(|p| preset_display_name(p, lang))
                            .unwrap_or_else(|| tr(lang, "Predefinição"));
                        let name = tr_args(lang, "Apelido ({name})", &[("name", &base_name)]);
                        if let Ok(aid) = self.encoder.catalog.create_alias(&name, &pid, None) {
                            self.encoder.selected_preset = Some(aid);
                            self.encoder.persist_dirty = true;
                        }
                    }
                    if ui.small_button(tr(lang, "Excluir")).clicked() {
                        self.encoder.catalog.delete_custom(&pid);
                        self.encoder.selected_preset = Some("sys.web_1080p_hq".into());
                        self.encoder.persist_dirty = true;
                    }
                }
            }
        });

        let mut rename_commit: Option<(String, String)> = None;
        let mut rename_cancel = false;
        if let Some((ref target_id, ref mut draft_name)) = self.encoder.rename_preset_target {
            ui.horizontal(|ui| {
                ui.label(tr(lang, "Novo nome:"));
                ui.add(egui::TextEdit::singleline(draft_name).desired_width(150.0));
                if ui.small_button(tr(lang, "OK")).clicked() {
                    rename_commit = Some((target_id.clone(), draft_name.clone()));
                }
                if ui.small_button(tr(lang, "Cancelar")).clicked() {
                    rename_cancel = true;
                }
            });
        }
        if let Some((id, name)) = rename_commit {
            if self.encoder.catalog.rename_custom(&id, &name) {
                self.encoder.persist_dirty = true;
            }
            self.encoder.rename_preset_target = None;
        } else if rename_cancel {
            self.encoder.rename_preset_target = None;
        }

        let q = self.encoder.preset_search.trim().to_lowercase();
        egui::ScrollArea::vertical()
            .id_salt("encoder_preset_scroll")
            .max_height(max_h)
            .show(ui, |ui| {
                // Grupos de múltiplas saídas
                egui::CollapsingHeader::new(tr(lang, "Grupos de Predefinições (Múltiplas Saídas)"))
                    .default_open(true)
                    .show(ui, |ui| {
                        for grp in &self.encoder.catalog.groups {
                            let disp_grp = if grp.id == "grp.web_multi" {
                                tr(lang, &grp.name)
                            } else {
                                grp.name.as_str()
                            };
                            if !q.is_empty()
                                && !disp_grp.to_lowercase().contains(&q)
                                && !grp.name.to_lowercase().contains(&q)
                            {
                                continue;
                            }
                            let sel = self.encoder.selected_preset.as_deref() == Some(&grp.id);
                            let n_s = grp.preset_ids.len().to_string();
                            if ui
                                .selectable_label(
                                    sel,
                                    tr_args(
                                        lang,
                                        "{name} ({n} item(ns))",
                                        &[("name", &disp_grp), ("n", &n_s)],
                                    ),
                                )
                                .clicked()
                            {
                                self.encoder.selected_preset = Some(grp.id.clone());
                            }
                        }
                    });

                // Apelidos
                if !self.encoder.catalog.aliases.is_empty() {
                    egui::CollapsingHeader::new(tr(lang, "Apelidos (Aliases)")).show(ui, |ui| {
                        for als in &self.encoder.catalog.aliases {
                            if !q.is_empty() && !als.name.to_lowercase().contains(&q) {
                                continue;
                            }
                            let sel = self.encoder.selected_preset.as_deref() == Some(&als.id);
                            if ui.selectable_label(sel, &als.name).clicked() {
                                self.encoder.selected_preset = Some(als.id.clone());
                            }
                        }
                    });
                }

                // Categorias (Usuário + Sistema)
                for cat in PresetCategory::ALL {
                    let list: Vec<_> = self
                        .encoder
                        .catalog
                        .all_presets()
                        .filter(|p| {
                            p.category == cat
                                && (q.is_empty()
                                    || preset_display_name(p, lang).to_lowercase().contains(&q)
                                    || p.name.to_lowercase().contains(&q)
                                    || p.format.name_in(lang).to_lowercase().contains(&q))
                        })
                        .collect();
                    if list.is_empty() {
                        continue;
                    }
                    // todos os grupos começam abertos
                    egui::CollapsingHeader::new(cat.name_in(lang))
                        .default_open(true)
                        .show(ui, |ui| {
                            for p in list {
                                let sel = self.encoder.selected_preset.as_deref() == Some(&p.id);
                                if ui
                                    .selectable_label(sel, preset_display_name(p, lang))
                                    .on_hover_text(preset_display_notes(p, lang))
                                    .clicked()
                                {
                                    self.encoder.selected_preset = Some(p.id.clone());
                                }
                            }
                        });
                }
            });
    }

    fn encoder_watch_section(&mut self, ui: &mut egui::Ui, _theme: &ThemeColors) {
        let lang = self.lang();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tr(lang, "Pastas Monitoradas")).strong());
            if ui.small_button(tr(lang, "+ Adicionar Pasta...")).clicked() {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    let pid = self
                        .encoder
                        .selected_preset
                        .clone()
                        .unwrap_or_else(|| "sys.web_1080p_hq".into());
                    self.encoder
                        .state
                        .watch_folders
                        .push(WatchFolder::new(dir, &pid));
                    self.encoder.persist_dirty = true;
                }
            }
            if ui.small_button(tr(lang, "Varrer Agora")).clicked() {
                let (added, auto_start) =
                    scan_watch_folders(&mut self.encoder.state, &self.encoder.catalog);
                self.encoder.persist_dirty = true;
                let added_s = added.to_string();
                self.set_status(tr_args(
                    lang,
                    "{added} novo(s) arquivo(s) encontrado(s)",
                    &[("added", &added_s)],
                ));
                if auto_start && !self.encoder.runtime.is_running() {
                    self.encoder.start_queue(&self.media);
                }
            }
            ui.checkbox(
                &mut self.encoder.state.prefs.auto_encode_watch_folders,
                tr(lang, "Codificar automático"),
            );
        });

        let mut pick_out_for: Option<u64> = None;
        let mut remove_id: Option<u64> = None;
        let all_presets: Vec<EncoderPreset> = self.encoder.catalog.all_presets().cloned().collect();

        egui::ScrollArea::vertical()
            .id_salt("encoder_watch_scroll")
            .max_height(150.0)
            .show(ui, |ui| {
                if self.encoder.state.watch_folders.is_empty() {
                    ui.label(
                        RichText::new(tr(
                            lang,
                            "Nenhuma pasta monitorada configurada. Novos arquivos adicionados serão codificados na fila.",
                        ))
                        .weak(),
                    );
                    return;
                }
                for wf in &mut self.encoder.state.watch_folders {
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut wf.enabled, "");
                        ui.label(wf.folder.display().to_string());
                        if ui.small_button(tr(lang, "Saída...")).clicked() {
                            pick_out_for = Some(wf.id);
                        }
                        let mut cur_pid = wf
                            .preset_ids
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "sys.web_1080p_hq".into());
                        let cur_name = self
                            .encoder
                            .catalog
                            .resolve(&cur_pid)
                            .map(|p| preset_display_name(p, lang))
                            .unwrap_or_else(|| tr(lang, "Selecionar..."));
                        egui::ComboBox::from_id_salt(("wf_pre", wf.id))
                            .width(155.0)
                            .selected_text(cur_name)
                            .show_ui(ui, |ui| {
                                for pr in &all_presets {
                                    if ui
                                        .selectable_value(
                                            &mut cur_pid,
                                            pr.id.clone(),
                                            preset_display_name(pr, lang),
                                        )
                                        .clicked()
                                    {
                                        if wf.preset_ids.is_empty() {
                                            wf.preset_ids.push(pr.id.clone());
                                        } else {
                                            wf.preset_ids[0] = pr.id.clone();
                                        }
                                        self.encoder.persist_dirty = true;
                                    }
                                }
                            });
                        ui.checkbox(&mut wf.auto_encode, tr(lang, "Auto"));
                        if ui.small_button("X").clicked() {
                            remove_id = Some(wf.id);
                        }
                    });
                }
            });

        if let Some(id) = pick_out_for {
            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                if let Some(wf) = self
                    .encoder
                    .state
                    .watch_folders
                    .iter_mut()
                    .find(|w| w.id == id)
                {
                    wf.output_dir = Some(dir);
                    self.encoder.persist_dirty = true;
                }
            }
        }
        if let Some(id) = remove_id {
            self.encoder.state.watch_folders.retain(|w| w.id != id);
            self.encoder.persist_dirty = true;
        }
    }

    pub(crate) fn open_export_settings_modal(&mut self, item_id: u64, output_id: u64) {
        let Some(item) = self.encoder.state.items.iter().find(|i| i.id == item_id) else {
            return;
        };
        let Some(out) = item.outputs.iter().find(|o| o.id == output_id) else {
            return;
        };
        self.encoder.export_modal = Some(ExportSettingsModal {
            item_id,
            output_id,
            draft: out.clone(),
            tab: ExportSettingsTab::Video,
            preview_pane: SettingsPreviewPane::Output,
            preview_sec: 0.0,
            preview_tex: None,
            preview_cache_key: String::new(),
            src_frame_cache: None,
            src_frame_key: String::new(),
        });
    }

    pub(crate) fn show_encoder_dialogs(&mut self, ctx: &egui::Context) {
        self.show_export_settings_dialog(ctx);
        self.show_interpret_dialog(ctx);
        self.show_encoder_prefs_dialog(ctx);
        self.show_create_preset_dialog(ctx);
        self.show_create_group_dialog(ctx);
        self.show_log_modal(ctx);
    }

    fn show_export_settings_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut modal) = self.encoder.export_modal.take() else {
            return;
        };
        let lang = self.lang();
        let mut keep_open = true;
        let mut save_clicked = false;
        let mut run_now_clicked = false;
        let mut cancel_clicked = false;

        let item_src = self
            .encoder
            .state
            .items
            .iter()
            .find(|i| i.id == modal.item_id)
            .map(|i| i.source.clone());

        crate::dialog_window::DialogWindow::new(tr(
            lang,
            "Configurações de Exportação — Neoveg Encoder",
        ))
        .id(egui::Id::new("enc_export_win"))
        .open(&mut keep_open)
        .default_size([780.0, 520.0])
        .resizable(true)
        .show(ctx, |ui| {
            ui.columns(2, |cols| {
                // Coluna Esquerda: Origem / Saída, Scrub, Intervalo, Corte e Escala
                let left = &mut cols[0];
                left.horizontal(|ui| {
                    ui.selectable_value(
                        &mut modal.preview_pane,
                        SettingsPreviewPane::Source,
                        tr(lang, "Origem"),
                    );
                    ui.selectable_value(
                        &mut modal.preview_pane,
                        SettingsPreviewPane::Output,
                        tr(lang, "Saída"),
                    );
                });

                // Gera/atualiza prévia rápida da origem ou do quadro transformado
                let t = Ticks::from_millis_i64((modal.preview_sec.max(0.0) * 1000.0) as i64);
                let src_key = format!("{}:{:.2}", modal.item_id, modal.preview_sec);
                if modal.src_frame_key != src_key {
                    modal.src_frame_key = src_key;
                    if let Some(QueueSource::ProjectSnapshot { project, .. }) = &item_src {
                        modal.src_frame_cache =
                            Some(render_frame(project, &self.media, t, 320, 180));
                    } else {
                        modal.src_frame_cache =
                            Some(Frame::filled(320, 180, [0.16, 0.24, 0.34, 1.0]));
                    }
                }

                if let Some(src_f) = &modal.src_frame_cache {
                    let pane_tag = match modal.preview_pane {
                        SettingsPreviewPane::Source => 0u8,
                        SettingsPreviewPane::Output => 1u8,
                    };
                    let key = format!(
                        "{pane_tag}:{}:{:?}:{:?}:{:?}",
                        modal.src_frame_key,
                        modal.draft.crop,
                        modal.draft.scaling,
                        modal.draft.effects.grade
                    );
                    if modal.preview_cache_key != key {
                        modal.preview_cache_key = key;
                        let frame = if modal.preview_pane == SettingsPreviewPane::Source {
                            src_f.clone()
                        } else {
                            let name = item_src
                                .as_ref()
                                .map(QueueSource::display_name)
                                .unwrap_or_else(|| "origem".into());
                            preview_transformed_frame(
                                src_f,
                                t,
                                modal
                                    .draft
                                    .fps_override
                                    .unwrap_or(self.session.project.settings.fps),
                                self.session.project.settings.sample_rate,
                                &modal.draft,
                                &name,
                            )
                        };
                        if let Some(img) = frame.to_image() {
                            let thumb = rgba_to_color_image(&img);
                            modal.preview_tex = Some(ctx.load_texture(
                                "enc_settings_pv",
                                thumb,
                                egui::TextureOptions::LINEAR,
                            ));
                        }
                    }
                }
                if let Some(tex) = &modal.preview_tex {
                    left.image((tex.id(), egui::vec2(320.0, 180.0)));
                }

                let fps_disp = modal
                    .draft
                    .fps_override
                    .unwrap_or(self.session.project.settings.fps);
                let tc_str = format_ticks(
                    t,
                    fps_disp,
                    self.session.project.settings.sample_rate,
                    self.session.opts.time_format,
                );
                left.horizontal(|ui| {
                    ui.label(tr(lang, "Tempo:"));
                    ui.add(egui::Slider::new(&mut modal.preview_sec, 0.0..=60.0).text(tc_str));
                });

                left.separator();
                left.label(RichText::new(tr(lang, "Intervalo de Origem")).strong());
                egui::ComboBox::from_id_salt("enc_range_mode")
                    .selected_text(modal.draft.range.name_in(lang))
                    .show_ui(left, |ui| {
                        for rm in SourceRangeMode::ALL {
                            ui.selectable_value(&mut modal.draft.range, rm, rm.name_in(lang));
                        }
                    });
                if modal.draft.range == SourceRangeMode::Custom {
                    let mut in_s = modal.draft.custom_in.as_secs_f64();
                    let mut out_s = modal.draft.custom_out.as_secs_f64().max(in_s + 0.1);
                    left.horizontal(|ui| {
                        ui.label(tr(lang, "Entrada (s):"));
                        ui.add(
                            egui::DragValue::new(&mut in_s)
                                .speed(0.1)
                                .range(0.0..=86400.0),
                        );
                        ui.label(tr(lang, "Saída (s):"));
                        ui.add(
                            egui::DragValue::new(&mut out_s)
                                .speed(0.1)
                                .range(0.1..=86400.0),
                        );
                    });
                    modal.draft.custom_in = Ticks::from_millis_i64((in_s * 1000.0) as i64);
                    modal.draft.custom_out =
                        Ticks::from_millis_i64((out_s.max(in_s + 0.04) * 1000.0) as i64);
                }

                left.separator();
                left.horizontal(|ui| {
                    ui.checkbox(
                        &mut modal.draft.crop.enabled,
                        tr(lang, "Cortar Origem (Crop)"),
                    );
                    if modal.draft.crop.enabled {
                        egui::ComboBox::from_id_salt("enc_crop_ar")
                            .selected_text(modal.draft.crop.aspect.name_in(lang))
                            .show_ui(ui, |ui| {
                                for ar in CropAspect::ALL {
                                    ui.selectable_value(
                                        &mut modal.draft.crop.aspect,
                                        ar,
                                        ar.name_in(lang),
                                    );
                                }
                            });
                    }
                });
                if modal.draft.crop.enabled {
                    left.horizontal(|ui| {
                        ui.label(tr(lang, "E:"));
                        ui.add(egui::DragValue::new(&mut modal.draft.crop.left_px).range(0..=4096));
                        ui.label(tr(lang, "S:"));
                        ui.add(egui::DragValue::new(&mut modal.draft.crop.top_px).range(0..=4096));
                        ui.label(tr(lang, "D:"));
                        ui.add(
                            egui::DragValue::new(&mut modal.draft.crop.right_px).range(0..=4096),
                        );
                        ui.label(tr(lang, "I:"));
                        ui.add(
                            egui::DragValue::new(&mut modal.draft.crop.bottom_px).range(0..=4096),
                        );
                    });
                }
                left.horizontal(|ui| {
                    ui.label(tr(lang, "Escala de Origem:"));
                    egui::ComboBox::from_id_salt("enc_scale_mode")
                        .selected_text(modal.draft.scaling.name_in(lang))
                        .show_ui(ui, |ui| {
                            for sm in SourceScalingMode::ALL {
                                ui.selectable_value(&mut modal.draft.scaling, sm, sm.name_in(lang));
                            }
                        });
                });

                // Coluna Direita: Abas Vídeo / Áudio / Efeitos
                let right = &mut cols[1];
                right.horizontal(|ui| {
                    ui.selectable_value(
                        &mut modal.tab,
                        ExportSettingsTab::Video,
                        tr(lang, "Vídeo"),
                    );
                    ui.selectable_value(
                        &mut modal.tab,
                        ExportSettingsTab::Audio,
                        tr(lang, "Áudio"),
                    );
                    ui.selectable_value(
                        &mut modal.tab,
                        ExportSettingsTab::Effects,
                        tr(lang, "Efeitos e Sobreposições"),
                    );
                });
                right.separator();

                egui::ScrollArea::vertical()
                    .id_salt("enc_modal_right")
                    .max_height(370.0)
                    .show(right, |ui| match modal.tab {
                        ExportSettingsTab::Video => {
                            ui.horizontal(|ui| {
                                ui.label(tr(lang, "Formato:"));
                                egui::ComboBox::from_id_salt("modal_fmt")
                                    .selected_text(modal.draft.format.name_in(lang))
                                    .show_ui(ui, |ui| {
                                        for f in RenderFormat::ALL {
                                            if ui
                                                .selectable_value(
                                                    &mut modal.draft.format,
                                                    f,
                                                    f.name_in(lang),
                                                )
                                                .clicked()
                                            {
                                                modal.draft.output_path =
                                                    modal.draft.output_path.with_extension(f.ext());
                                            }
                                        }
                                    });
                            });
                            ui.checkbox(&mut modal.draft.include_video, tr(lang, "Exportar Vídeo"));
                            ui.checkbox(
                                &mut modal.draft.match_source_dims,
                                tr(lang, "Corresponder à resolução da origem"),
                            );
                            ui.add_enabled_ui(!modal.draft.match_source_dims, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(tr(lang, "Largura:"));
                                    ui.add(
                                        egui::DragValue::new(&mut modal.draft.width)
                                            .range(16..=7680),
                                    );
                                    ui.label(tr(lang, "Altura:"));
                                    ui.add(
                                        egui::DragValue::new(&mut modal.draft.height)
                                            .range(16..=4320),
                                    );
                                });
                            });
                            if modal.draft.format.uses_video_bitrate() {
                                ui.horizontal(|ui| {
                                    ui.label(tr(lang, "Taxa de vídeo (kbps):"));
                                    ui.add(
                                        egui::Slider::new(
                                            &mut modal.draft.video_bitrate_kbps,
                                            500..=100_000,
                                        )
                                        .logarithmic(true),
                                    );
                                });
                            }
                            if modal.draft.format.uses_jpeg_quality() {
                                ui.horizontal(|ui| {
                                    ui.label(tr(lang, "Qualidade JPEG:"));
                                    ui.add(egui::Slider::new(
                                        &mut modal.draft.jpeg_quality,
                                        10..=100,
                                    ));
                                });
                            }
                        }
                        ExportSettingsTab::Audio => {
                            ui.checkbox(&mut modal.draft.include_audio, tr(lang, "Exportar Áudio"));
                            if modal.draft.format.uses_audio_bitrate() {
                                ui.horizontal(|ui| {
                                    ui.label(tr(lang, "Taxa de áudio (kbps):"));
                                    ui.add(egui::Slider::new(
                                        &mut modal.draft.audio_bitrate_kbps,
                                        64..=512,
                                    ));
                                });
                            }
                            if modal.draft.format.uses_wav_bits() {
                                ui.horizontal(|ui| {
                                    ui.label(tr(lang, "Profundidade PCM:"));
                                    ui.selectable_value(
                                        &mut modal.draft.wav_bits,
                                        WavBits::Pcm16,
                                        tr(lang, "16 bits"),
                                    );
                                    ui.selectable_value(
                                        &mut modal.draft.wav_bits,
                                        WavBits::Pcm24,
                                        tr(lang, "24 bits"),
                                    );
                                    ui.selectable_value(
                                        &mut modal.draft.wav_bits,
                                        WavBits::Float32,
                                        tr(lang, "32 bits float"),
                                    );
                                });
                            }
                        }
                        ExportSettingsTab::Effects => {
                            ui.label(
                                RichText::new(tr(lang, "Correção de Cor / LUT (.cube)")).strong(),
                            );
                            egui::ComboBox::from_id_salt("modal_grade")
                                .selected_text(modal.draft.effects.grade.name_in(lang))
                                .show_ui(ui, |ui| {
                                    for (label, g) in GradePreset::NAMED {
                                        ui.selectable_value(
                                            &mut modal.draft.effects.grade,
                                            g,
                                            tr(lang, label),
                                        );
                                    }
                                });
                            ui.horizontal(|ui| {
                                let ltxt = modal
                                    .draft
                                    .effects
                                    .lut_path
                                    .as_deref()
                                    .and_then(|p| p.file_name())
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| tr(lang, "Nenhuma LUT selecionada").into());
                                ui.label(ltxt);
                                if ui.small_button(tr(lang, "Selecionar .cube...")).clicked() {
                                    if let Some(p) = rfd::FileDialog::new()
                                        .add_filter(tr(lang, "Tabela LUT"), &["cube"])
                                        .pick_file()
                                    {
                                        modal.draft.effects.lut_path = Some(p);
                                    }
                                }
                                if modal.draft.effects.lut_path.is_some()
                                    && ui.small_button(tr(lang, "Limpar")).clicked()
                                {
                                    modal.draft.effects.lut_path = None;
                                }
                            });

                            ui.separator();
                            ui.checkbox(
                                &mut modal.draft.effects.timecode_overlay.enabled,
                                tr(lang, "Sobreposição de Timecode"),
                            );
                            if modal.draft.effects.timecode_overlay.enabled {
                                ui.horizontal(|ui| {
                                    ui.selectable_value(
                                        &mut modal.draft.effects.timecode_overlay.source,
                                        TimecodeSource::MediaTime,
                                        tr(lang, "Tempo da Mídia"),
                                    );
                                    ui.selectable_value(
                                        &mut modal.draft.effects.timecode_overlay.source,
                                        TimecodeSource::CustomStart,
                                        tr(lang, "Timecode Inicial"),
                                    );
                                    ui.checkbox(
                                        &mut modal.draft.effects.timecode_overlay.drop_frame,
                                        tr(lang, "Drop-Frame"),
                                    );
                                });
                                overlay_pos_controls(
                                    ui,
                                    lang,
                                    "tc_pos",
                                    &mut modal.draft.effects.timecode_overlay.position,
                                    &mut modal.draft.effects.timecode_overlay.size_pct,
                                    &mut modal.draft.effects.timecode_overlay.bg_opacity,
                                );
                            }

                            ui.separator();
                            ui.checkbox(
                                &mut modal.draft.effects.name_overlay.enabled,
                                tr(lang, "Sobreposição de Nome"),
                            );
                            if modal.draft.effects.name_overlay.enabled {
                                egui::ComboBox::from_id_salt("name_fmt")
                                    .selected_text(
                                        modal.draft.effects.name_overlay.format.name_in(lang),
                                    )
                                    .show_ui(ui, |ui| {
                                        for nf in NameFormat::ALL {
                                            ui.selectable_value(
                                                &mut modal.draft.effects.name_overlay.format,
                                                nf,
                                                nf.name_in(lang),
                                            );
                                        }
                                    });
                                ui.horizontal(|ui| {
                                    ui.label(tr(lang, "Prefixo:"));
                                    ui.add(
                                        egui::TextEdit::singleline(
                                            &mut modal.draft.effects.name_overlay.prefix,
                                        )
                                        .desired_width(80.0),
                                    );
                                    ui.label(tr(lang, "Sufixo:"));
                                    ui.add(
                                        egui::TextEdit::singleline(
                                            &mut modal.draft.effects.name_overlay.suffix,
                                        )
                                        .desired_width(80.0),
                                    );
                                });
                                overlay_pos_controls(
                                    ui,
                                    lang,
                                    "nm_pos",
                                    &mut modal.draft.effects.name_overlay.position,
                                    &mut modal.draft.effects.name_overlay.size_pct,
                                    &mut modal.draft.effects.name_overlay.bg_opacity,
                                );
                            }

                            ui.separator();
                            ui.checkbox(
                                &mut modal.draft.effects.image_overlay.enabled,
                                tr(lang, "Sobreposição de Imagem (Marca d'água)"),
                            );
                            if modal.draft.effects.image_overlay.enabled {
                                ui.horizontal(|ui| {
                                    if ui.small_button(tr(lang, "Escolher imagem...")).clicked() {
                                        if let Some(p) = rfd::FileDialog::new()
                                            .add_filter(tr(lang, "Imagens"), IMAGE_EXTS)
                                            .pick_file()
                                        {
                                            modal.draft.effects.image_overlay.path = Some(p);
                                        }
                                    }
                                    ui.checkbox(
                                        &mut modal.draft.effects.image_overlay.absolute_size,
                                        tr(lang, "Tamanho absoluto"),
                                    );
                                });
                                overlay_pos_controls(
                                    ui,
                                    lang,
                                    "im_pos",
                                    &mut modal.draft.effects.image_overlay.position,
                                    &mut modal.draft.effects.image_overlay.size_pct,
                                    &mut modal.draft.effects.image_overlay.opacity,
                                );
                            }

                            ui.separator();
                            ui.checkbox(
                                &mut modal.draft.effects.time_tuner.enabled,
                                tr(lang, "Ajustador de Tempo (Time Tuner)"),
                            );
                            if modal.draft.effects.time_tuner.enabled {
                                ui.horizontal(|ui| {
                                    ui.label(tr(lang, "Duração (%):"));
                                    ui.add(egui::Slider::new(
                                        &mut modal.draft.effects.time_tuner.duration_pct,
                                        25.0..=200.0,
                                    ));
                                });
                            }
                        }
                    });
            });

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(tr(lang, "OK (Salvar na Fila)")).clicked() {
                    save_clicked = true;
                }
                if ui
                    .button(tr(lang, "Codificar Agora (Teste Síncrono)"))
                    .clicked()
                {
                    save_clicked = true;
                    run_now_clicked = true;
                }
                if ui.button(tr(lang, "Cancelar")).clicked() {
                    cancel_clicked = true;
                }
            });
        });

        if cancel_clicked {
            keep_open = false;
        }
        if save_clicked {
            let iid = modal.item_id;
            let oid = modal.output_id;
            if let Some(it) = self.encoder.state.items.iter_mut().find(|i| i.id == iid) {
                if let Some(o) = it.outputs.iter_mut().find(|o| o.id == oid) {
                    *o = modal.draft.clone();
                    if o.status.is_terminal() {
                        o.status = JobStatus::Ready;
                        o.progress = 0.0;
                    }
                    self.encoder.persist_dirty = true;
                }
                if run_now_clicked {
                    let item_clone = it.clone();
                    let results =
                        encode_item_sync(&item_clone, &self.encoder.state.prefs, Some(&self.media));
                    for upd in results {
                        if let Some(o) = it.outputs.iter_mut().find(|o| o.id == upd.output_id) {
                            o.status = upd.status;
                            o.progress = upd.progress;
                            o.elapsed_secs = upd.elapsed_secs;
                            o.last_message = upd.message;
                            if let Some(p) = upd.final_output_path {
                                o.output_path = p;
                            }
                        }
                    }
                }
            }
            keep_open = false;
        }
        if keep_open {
            self.encoder.export_modal = Some(modal);
        }
    }

    fn show_interpret_dialog(&mut self, ctx: &egui::Context) {
        let Some(iid) = self.encoder.interpret_item_id else {
            return;
        };
        let lang = self.lang();
        let mut keep = true;
        let Some(idx) = self.encoder.state.items.iter().position(|i| i.id == iid) else {
            self.encoder.interpret_item_id = None;
            return;
        };
        let QueueSource::MediaFile { mut interpret, .. } = self.encoder.state.items[idx].source
        else {
            crate::dialog_window::DialogWindow::new(tr(lang, "Interpretar Gravação"))
                .id(egui::Id::new("enc_interpret_na_win"))
                .open(&mut keep)
                .show(ctx, |ui| {
                    ui.label(tr(
                        lang,
                        "A interpretação de pixel/FPS se aplica apenas a arquivos de mídia importados.",
                    ));
                });
            if !keep {
                self.encoder.interpret_item_id = None;
            }
            return;
        };
        let mut dirty = false;
        crate::dialog_window::DialogWindow::new(tr(lang, "Interpretar Gravação — Neoveg Encoder"))
            .id(egui::Id::new("enc_interpret_win"))
            .open(&mut keep)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "Taxa de quadros:"));
                    let lbl = interpret
                        .fps_override
                        .map(|f| {
                            let fps_s = format!("{:.3}", f.as_f64());
                            tr_args(lang, "{fps} qps", &[("fps", &fps_s)])
                        })
                        .unwrap_or_else(|| tr(lang, "Usar taxa do arquivo").into());
                    egui::ComboBox::from_id_salt("interp_fps")
                        .selected_text(lbl)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(
                                    interpret.fps_override.is_none(),
                                    tr(lang, "Usar taxa do arquivo"),
                                )
                                .clicked()
                            {
                                interpret.fps_override = None;
                                dirty = true;
                            }
                            for fps in [
                                FrameRate::FPS_23_976,
                                FrameRate::FPS_24,
                                FrameRate::FPS_25,
                                FrameRate::FPS_29_97,
                                FrameRate::FPS_30,
                                FrameRate::FPS_50,
                                FrameRate::FPS_59_94,
                                FrameRate::FPS_60,
                            ] {
                                let fps_s = format!("{:.3}", fps.as_f64());
                                if ui
                                    .selectable_label(
                                        interpret.fps_override == Some(fps),
                                        tr_args(lang, "{fps} qps", &[("fps", &fps_s)]),
                                    )
                                    .clicked()
                                {
                                    interpret.fps_override = Some(fps);
                                    dirty = true;
                                }
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "Proporção de pixel (PAR):"));
                    if ui
                        .add(
                            egui::DragValue::new(&mut interpret.pixel_aspect)
                                .speed(0.01)
                                .range(0.5..=2.5),
                        )
                        .changed()
                    {
                        dirty = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "Canal Alfa:"));
                    egui::ComboBox::from_id_salt("interp_alpha")
                        .selected_text(interpret.alpha.name_in(lang))
                        .show_ui(ui, |ui| {
                            for a in AlphaInterpret::ALL {
                                if ui
                                    .selectable_value(&mut interpret.alpha, a, a.name_in(lang))
                                    .clicked()
                                {
                                    dirty = true;
                                }
                            }
                        });
                });
            });
        if dirty {
            if let QueueSource::MediaFile {
                interpret: ref mut target,
                ..
            } = self.encoder.state.items[idx].source
            {
                *target = interpret;
                self.encoder.persist_dirty = true;
            }
        }
        if !keep {
            self.encoder.interpret_item_id = None;
        }
    }

    fn show_encoder_prefs_dialog(&mut self, ctx: &egui::Context) {
        if !self.encoder.prefs_open {
            return;
        }
        let lang = self.lang();
        let mut open = true;
        let mut dirty = false;
        crate::dialog_window::DialogWindow::new(tr(lang, "Preferências do Neoveg Encoder"))
            .id(egui::Id::new("enc_prefs_win"))
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                let p = &mut self.encoder.state.prefs;
                let mut auto_idle = p.auto_start_idle_secs.is_some();
                if ui
                    .checkbox(
                        &mut auto_idle,
                        tr(lang, "Iniciar fila automaticamente quando ocioso por:"),
                    )
                    .changed()
                {
                    p.auto_start_idle_secs = if auto_idle { Some(30) } else { None };
                    dirty = true;
                }
                if let Some(secs) = &mut p.auto_start_idle_secs {
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "Tempo ocioso (segundos):"));
                        if ui.add(egui::DragValue::new(secs).range(5..=3600)).changed() {
                            dirty = true;
                        }
                    });
                }
                dirty |= ui
                    .checkbox(
                        &mut p.show_elapsed_time,
                        tr(lang, "Mostrar tempo decorrido e ETA"),
                    )
                    .changed();
                dirty |= ui
                    .checkbox(
                        &mut p.preview_during_encoding,
                        tr(lang, "Visualizar durante a codificação"),
                    )
                    .changed();
                dirty |= ui
                    .checkbox(
                        &mut p.play_chime_on_finish,
                        tr(lang, "Notificar ao concluir a codificação"),
                    )
                    .changed();
                dirty |= ui
                    .checkbox(
                        &mut p.remove_completed_on_exit,
                        tr(lang, "Remover arquivos concluídos da fila ao sair"),
                    )
                    .changed();
                dirty |= ui
                    .checkbox(
                        &mut p.increment_existing_output,
                        tr(lang, "Incrementar nome do arquivo de saída se já existir"),
                    )
                    .changed();
                ui.horizontal(|ui| {
                    let lbl = p
                        .default_output_dir
                        .as_deref()
                        .map(|d| d.display().to_string())
                        .unwrap_or_else(|| tr(lang, "Mesma pasta da origem").into());
                    ui.label(tr_args(
                        lang,
                        "Pasta padrão de saída: {lbl}",
                        &[("lbl", &lbl)],
                    ));
                    if ui.small_button(tr(lang, "Escolher...")).clicked() {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            p.default_output_dir = Some(dir);
                            dirty = true;
                        }
                    }
                    if p.default_output_dir.is_some()
                        && ui.small_button(tr(lang, "Padrão")).clicked()
                    {
                        p.default_output_dir = None;
                        dirty = true;
                    }
                });
            });
        if dirty {
            self.encoder.persist_dirty = true;
        }
        if !open {
            self.encoder.prefs_open = false;
        }
    }

    fn show_create_preset_dialog(&mut self, ctx: &egui::Context) {
        if !self.encoder.create_preset_open {
            return;
        }
        let lang = self.lang();
        let mut open = true;
        let mut create = false;
        crate::dialog_window::DialogWindow::new(tr(lang, "Nova Predefinição Personalizada"))
            .id(egui::Id::new("enc_new_preset_win"))
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "Nome:"));
                    ui.text_edit_singleline(&mut self.encoder.new_preset_name);
                });
                ui.label(
                    RichText::new(tr(
                        lang,
                        "Baseia-se na saída ou predefinição atualmente selecionada.",
                    ))
                    .small()
                    .weak(),
                );
                if ui.button(tr(lang, "Salvar Predefinição")).clicked() {
                    create = true;
                }
            });
        if create {
            let mut base = self
                .encoder
                .selected_preset
                .as_deref()
                .and_then(|pid| self.encoder.catalog.resolve(pid))
                .cloned()
                .unwrap_or_else(|| system_presets()[0].clone());
            if let (Some(iid), Some(oid)) =
                (self.encoder.selected_item, self.encoder.selected_output)
            {
                if let Some(out) = self
                    .encoder
                    .state
                    .items
                    .iter()
                    .find(|i| i.id == iid)
                    .and_then(|i| i.outputs.iter().find(|o| o.id == oid))
                {
                    base.format = out.format;
                    base.width = if out.match_source_dims {
                        None
                    } else {
                        Some(out.width)
                    };
                    base.height = if out.match_source_dims {
                        None
                    } else {
                        Some(out.height)
                    };
                    base.include_video = out.include_video;
                    base.include_audio = out.include_audio;
                    base.video_bitrate_kbps = out.video_bitrate_kbps;
                    base.audio_bitrate_kbps = out.audio_bitrate_kbps;
                    base.jpeg_quality = out.jpeg_quality;
                    base.wav_bits = out.wav_bits;
                    base.scaling = out.scaling;
                    base.effects = out.effects.clone();
                }
            }
            base.id.clear();
            base.name = self.encoder.new_preset_name.trim().to_string();
            let id = self.encoder.catalog.save_custom(base);
            self.encoder.selected_preset = Some(id);
            self.encoder.persist_dirty = true;
            open = false;
        }
        if !open {
            self.encoder.create_preset_open = false;
        }
    }

    fn show_create_group_dialog(&mut self, ctx: &egui::Context) {
        if !self.encoder.create_group_open {
            return;
        }
        let lang = self.lang();
        let mut open = true;
        let mut create = false;
        crate::dialog_window::DialogWindow::new(tr(lang, "Novo Grupo de Predefinições"))
            .id(egui::Id::new("enc_new_group_win"))
            .open(&mut open)
            .default_size([380.0, 320.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "Nome do grupo:"));
                    ui.text_edit_singleline(&mut self.encoder.new_group_name);
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(210.0)
                    .show(ui, |ui| {
                        for pr in self.encoder.catalog.all_presets() {
                            let mut chk = self.encoder.new_group_presets.contains(&pr.id);
                            if ui
                                .checkbox(&mut chk, preset_display_name(pr, lang))
                                .changed()
                            {
                                if chk {
                                    self.encoder.new_group_presets.push(pr.id.clone());
                                } else {
                                    self.encoder.new_group_presets.retain(|x| x != &pr.id);
                                }
                            }
                        }
                    });
                if ui.button(tr(lang, "Criar Grupo")).clicked()
                    && !self.encoder.new_group_presets.is_empty()
                {
                    create = true;
                }
            });
        if create {
            let gid = self.encoder.catalog.create_group(
                &self.encoder.new_group_name,
                self.encoder.new_group_presets.clone(),
            );
            self.encoder.selected_preset = Some(gid);
            self.encoder.persist_dirty = true;
            open = false;
        }
        if !open {
            self.encoder.create_group_open = false;
        }
    }

    fn show_log_modal(&mut self, ctx: &egui::Context) {
        let Some((title, path, mut content)) = self.encoder.log_modal.take() else {
            return;
        };
        let lang = self.lang();
        let mut open = true;
        crate::dialog_window::DialogWindow::new(tr(lang, title))
            .id(egui::Id::new("enc_log_win"))
            .open(&mut open)
            .default_size([560.0, 340.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(path.display().to_string());
                    if ui.small_button(tr(lang, "Limpar Registro")).clicked() {
                        let _ = std::fs::write(&path, "");
                        content.clear();
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut content)
                            .desired_width(f32::INFINITY)
                            .desired_rows(14),
                    );
                });
            });
        if open {
            self.encoder.log_modal = Some((title, path, content));
        }
    }
}

fn overlay_pos_controls(
    ui: &mut egui::Ui,
    lang: Language,
    salt: &str,
    pos: &mut OverlayPosition,
    size_or_pct: &mut f32,
    opacity: &mut f32,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(tr(lang, "Posição:"));
        egui::ComboBox::from_id_salt(salt)
            .selected_text(pos.name_in(lang))
            .show_ui(ui, |ui| {
                for p in OverlayPosition::ALL {
                    ui.selectable_value(pos, p, p.name_in(lang));
                }
            });
        ui.label(tr(lang, "Tam %:"));
        ui.add(
            egui::DragValue::new(size_or_pct)
                .speed(0.5)
                .range(1.5..=50.0),
        );
        ui.label(tr(lang, "Opac:"));
        ui.add(egui::DragValue::new(opacity).speed(0.05).range(0.0..=1.0));
    });
}
