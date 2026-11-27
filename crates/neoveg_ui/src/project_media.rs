//! Janela Mídia do Projeto: pastas (bins), pastas inteligentes (smart bins),
//! modos de exibição (miniaturas, lista, detalhes e storyboard), hover scrub,
//! substituição/troca de mídia mantendo eventos e seleção dos eventos na timeline
//! (semântica do VEGAS Pro 21, pp. 135–155).

use crate::app::LoadMsg;
use crate::icons::{self, Icon};
use crate::model::*;
use crate::{rgba_to_color_image, App, DndPayload};
use eframe::egui;
use egui::{pos2, vec2, Color32, Rect, Sense, Stroke, TextureHandle};
use neoveg_common::{tr, tr_args};
use neoveg_engine::media;
use neoveg_engine::time::Ticks;
use std::collections::{HashMap, HashSet};

const MAX_SCRUB_CACHE: usize = 64;
const SCRUB_STEPS: f32 = 23.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MediaThumbSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl MediaThumbSize {
    pub const ALL: [MediaThumbSize; 3] = [
        MediaThumbSize::Small,
        MediaThumbSize::Medium,
        MediaThumbSize::Large,
    ];

    pub fn card_width(self) -> f32 {
        match self {
            MediaThumbSize::Small => 132.0,
            MediaThumbSize::Medium => 168.0,
            MediaThumbSize::Large => 216.0,
        }
    }

    pub fn thumb_height(self) -> f32 {
        match self {
            MediaThumbSize::Small => 60.0,
            MediaThumbSize::Medium => 74.0,
            MediaThumbSize::Large => 104.0,
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            MediaThumbSize::Small => "P",
            MediaThumbSize::Medium => "M",
            MediaThumbSize::Large => "G",
        }
    }

    pub fn tip_key(self) -> &'static str {
        match self {
            MediaThumbSize::Small => "Miniaturas pequenas",
            MediaThumbSize::Medium => "Miniaturas médias",
            MediaThumbSize::Large => "Miniaturas grandes",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MediaViewMode {
    #[default]
    Thumbnails,
    List,
    Details,
    Storyboard,
}

impl MediaViewMode {
    pub const ALL: [MediaViewMode; 4] = [
        MediaViewMode::Thumbnails,
        MediaViewMode::List,
        MediaViewMode::Details,
        MediaViewMode::Storyboard,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MediaViewMode::Thumbnails => "Miniaturas",
            MediaViewMode::List => "Lista",
            MediaViewMode::Details => "Detalhes",
            MediaViewMode::Storyboard => "Storyboard",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            MediaViewMode::Thumbnails => Icon::ViewThumbnails,
            MediaViewMode::List => Icon::ViewList,
            MediaViewMode::Details => Icon::ViewDetails,
            MediaViewMode::Storyboard => Icon::StoryboardGrid,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MediaFilterTarget {
    #[default]
    All,
    Bin(Id),
    BuiltinSmart(SmartBinRule),
    CustomSmart(Id),
    Storyboard(Id),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MediaSortCol {
    #[default]
    Original,
    Name,
    Type,
    Duration,
    Usage,
}

pub struct ProjectMediaState {
    pub view_mode: MediaViewMode,
    pub thumb_size: MediaThumbSize,
    pub filter_target: MediaFilterTarget,
    pub search: String,
    pub sort_col: MediaSortCol,
    pub sort_asc: bool,
    pub scrub_thumbs: HashMap<(Id, u8), TextureHandle>,
    pub scrub_order: Vec<(Id, u8)>,
    pub renaming_bin: Option<Id>,
    pub bin_name_buf: String,
    pub smart_dialog_open: bool,
    pub editing_smart_id: Option<Id>,
    pub smart_name_buf: String,
    pub smart_rule_buf: SmartBinRule,
    pub smart_query_buf: String,
    pub renaming_storyboard: Option<Id>,
    pub storyboard_name_buf: String,
}

impl Default for ProjectMediaState {
    fn default() -> Self {
        Self {
            view_mode: MediaViewMode::Thumbnails,
            thumb_size: MediaThumbSize::Medium,
            filter_target: MediaFilterTarget::All,
            search: String::new(),
            sort_col: MediaSortCol::Original,
            sort_asc: true,
            scrub_thumbs: HashMap::new(),
            scrub_order: Vec::new(),
            renaming_bin: None,
            bin_name_buf: String::new(),
            smart_dialog_open: false,
            editing_smart_id: None,
            smart_name_buf: "Minha pasta inteligente".into(),
            smart_rule_buf: SmartBinRule::NameContains,
            smart_query_buf: String::new(),
            renaming_storyboard: None,
            storyboard_name_buf: String::new(),
        }
    }
}

impl ProjectMediaState {
    pub fn invalidate_media(&mut self, id: Id) {
        self.scrub_thumbs.retain(|(mid, _), _| *mid != id);
        self.scrub_order.retain(|(mid, _)| *mid != id);
    }

    pub fn clear_thumbs(&mut self) {
        self.scrub_thumbs.clear();
        self.scrub_order.clear();
    }
}

enum MediaRowAction {
    AddAtCursor(Id),
    OpenTrimmer(Id),
    OpenNestedTab(Id),
    AddAsTake(Id),
    SelectEvents(Id),
    ReplaceFromDisk(Id),
    SwapExisting(Id, Id),
    AssignBin(Id, Option<Id>),
    RemoveFromBin(Id, Id),
    AddToStoryboard(Id, Id),
    CreateStoryboardWith(Id),
    BuildProxy(Id),
    EditGenerated(Id),
    OpenExternal(neoveg_engine::integrations::ExternalApp, Id),
    RemoveFromProject(Id),
}

const BUILTIN_SMART_BINS: [SmartBinRule; 5] = [
    SmartBinRule::AllVideo,
    SmartBinRule::AudioOnly,
    SmartBinRule::StillImages,
    SmartBinRule::Generated,
    SmartBinRule::Unused,
];

impl App {
    pub(crate) fn thumb_scrub(
        &mut self,
        ctx: &egui::Context,
        id: Id,
        frac: f32,
    ) -> (Option<TextureHandle>, Ticks) {
        let Some(m) = self.project.media(id).cloned() else {
            return (None, Ticks::ZERO);
        };
        let frac = frac.clamp(0.0, 1.0);
        let step = (frac * SCRUB_STEPS).round().clamp(0.0, SCRUB_STEPS) as u8;
        let sample_t = match m.duration {
            Some(d) if d > Ticks::ZERO => {
                Ticks::from_raw(((d.0 as f64) * f64::from(step) / f64::from(SCRUB_STEPS)) as i64)
            }
            _ => Ticks::from_millis_i64(i64::from(step) * 200),
        };
        if !m.has_video
            || (!self.media.contains(id) && !matches!(m.source, MediaSource::Generated(_)))
        {
            return (None, sample_t);
        }
        let key = (id, step);
        if let Some(tex) = self.project_media.scrub_thumbs.get(&key) {
            return (Some(tex.clone()), sample_t);
        }
        let Some(img) = self.media.thumbnail_at(&m, sample_t, 160) else {
            return (self.thumb(ctx, id), sample_t);
        };
        let tex = ctx.load_texture(
            format!("media_scrub_{id}_{step}"),
            rgba_to_color_image(&img),
            egui::TextureOptions::LINEAR,
        );
        if self.project_media.scrub_thumbs.len() >= MAX_SCRUB_CACHE {
            if let Some(oldest) = self.project_media.scrub_order.first().copied() {
                self.project_media.scrub_order.remove(0);
                self.project_media.scrub_thumbs.remove(&oldest);
            }
        }
        self.project_media.scrub_thumbs.insert(key, tex.clone());
        self.project_media.scrub_order.push(key);
        (Some(tex), sample_t)
    }

    pub(crate) fn replace_media_dialog(&mut self, media_id: Id) {
        if self.project.media(media_id).is_none() {
            self.set_status("Selecione uma mídia válida para substituir.");
            return;
        }
        let mut all: Vec<&str> = Vec::new();
        all.extend_from_slice(media::IMAGE_EXTS);
        all.extend_from_slice(media::VIDEO_EXTS);
        all.extend_from_slice(media::AUDIO_EXTS);
        all.extend_from_slice(media::SYSTEM_CONTAINER_EXTS);
        if let Some(file) = rfd::FileDialog::new()
            .set_title("Substituir arquivo de mídia")
            .add_filter("Mídia suportada", &all)
            .add_filter("Todos os arquivos", &["*"])
            .pick_file()
        {
            let tx = self.loader_tx.clone();
            self.loading += 1;
            let spawned =
                crate::util::spawn("substituir-midia", move || {
                    match media::import_file(&file) {
                        Ok(imp) => {
                            let _ = tx.send(LoadMsg::Replaced(media_id, Ok(imp)));
                        }
                        Err(e) => {
                            let _ = tx.send(LoadMsg::Replaced(media_id, Err(format!("{e:#}"))));
                        }
                    }
                });
            if let Err(e) = spawned {
                self.loading = self.loading.saturating_sub(1);
                self.set_status(e);
            }
        }
    }

    pub(crate) fn replace_media_with_imported(&mut self, target_id: Id, imp: media::Imported) {
        self.media.insert(target_id, imp.entry);
        self.thumbs.remove(&target_id);
        self.project_media.invalidate_media(target_id);
        if self.session.replace_media_item(target_id, imp.item) {
            self.sync_after_session();
        }
    }

    pub(crate) fn filtered_project_media(&self) -> Vec<MediaItem> {
        let used = self.project.used_media_ids();
        let bin_set: Option<HashSet<Id>> = match self.project_media.filter_target {
            MediaFilterTarget::Bin(bid) => Some(self.project.collect_bin_media(bid, true)),
            _ => None,
        };
        let custom_smart = match self.project_media.filter_target {
            MediaFilterTarget::CustomSmart(sid) => self
                .project
                .smart_bins
                .iter()
                .find(|s| s.id == sid)
                .cloned(),
            _ => None,
        };
        let q = self.project_media.search.trim().to_lowercase();
        let mut items: Vec<MediaItem> = self
            .project
            .media
            .iter()
            .filter(|m| {
                let in_target = match self.project_media.filter_target {
                    MediaFilterTarget::All | MediaFilterTarget::Storyboard(_) => true,
                    MediaFilterTarget::Bin(_) => {
                        bin_set.as_ref().is_some_and(|set| set.contains(&m.id))
                    }
                    MediaFilterTarget::BuiltinSmart(rule) => rule.matches("", m, &used),
                    MediaFilterTarget::CustomSmart(_) => custom_smart
                        .as_ref()
                        .is_some_and(|sb| sb.rule.matches(&sb.query, m, &used)),
                };
                if !in_target {
                    return false;
                }
                if q.is_empty() {
                    return true;
                }
                m.name.to_lowercase().contains(&q) || m.info.to_lowercase().contains(&q)
            })
            .cloned()
            .collect();

        match self.project_media.sort_col {
            MediaSortCol::Original => {}
            MediaSortCol::Name => {
                items.sort_by_key(|a| a.name.to_lowercase());
            }
            MediaSortCol::Type => {
                items.sort_by_key(|m| (m.has_video, m.has_audio, m.duration.is_some()))
            }
            MediaSortCol::Duration => items.sort_by_key(|m| m.duration.unwrap_or(Ticks::ZERO)),
            MediaSortCol::Usage => {
                items.sort_by_key(|m| self.project.media_usage_count(m.id));
            }
        }
        if !self.project_media.sort_asc && self.project_media.sort_col != MediaSortCol::Original {
            items.reverse();
        }
        items
    }

    pub(crate) fn project_media_ui(&mut self, ui: &mut egui::Ui) {
        self.sanitize_media_panel_selection();
        self.draw_media_toolbar(ui);
        ui.separator();
        self.draw_smart_bin_editor(ui);

        let visible_w = (ui.clip_rect().right() - ui.cursor().left()).min(ui.available_width());
        let visible_w = visible_w - ui.spacing().scroll.allocated_width();
        let avail_h = media_body_height(ui.available_height());
        // O corpo inteiro (pastas, mídias e as informações abaixo) rola na
        // vertical: num painel baixo, as informações da mídia ficavam cortadas
        // sob a borda do painel, sem como alcançá-las.
        egui::ScrollArea::vertical()
            .id_salt("pm_body_scroll")
            .auto_shrink([false, true])
            .show(ui, |ui| self.project_media_body(ui, visible_w, avail_h));
    }

    fn project_media_body(&mut self, ui: &mut egui::Ui, visible_w: f32, avail_h: f32) {
        if visible_w < 430.0 {
            self.draw_compact_bin_selector(ui);
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(visible_w.max(200.0));
                ui.set_min_height(avail_h);
                ui.set_max_height(avail_h);
                if self.project_media.view_mode == MediaViewMode::Storyboard {
                    self.draw_storyboard_view(ui);
                } else {
                    self.draw_media_collection(ui);
                }
            });
            ui.separator();
            self.draw_media_status_footer(ui);
            return;
        }
        let left_w = (visible_w * 0.24).clamp(150.0, 225.0);
        let right_w = (visible_w - left_w - 18.0).max(200.0);

        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(left_w);
                ui.set_min_height(avail_h);
                ui.set_max_height(avail_h);
                egui::ScrollArea::vertical()
                    .id_salt("pm_bins_tree")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.draw_bins_rail(ui);
                    });
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(right_w);
                ui.set_min_height(avail_h);
                ui.set_max_height(avail_h);
                if self.project_media.view_mode == MediaViewMode::Storyboard {
                    self.draw_storyboard_view(ui);
                } else {
                    self.draw_media_collection(ui);
                }
            });
        });
        ui.separator();
        self.draw_media_status_footer(ui);
    }

    /// Barra inferior de metadados estilo VEGAS Pro (manual p. 142): mostra formato,
    /// duração e uso da mídia selecionada, além do total exibido.
    fn draw_media_status_footer(&self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let shown = self.filtered_project_media().len();
        let total = self.project.media.len();
        let summary = self
            .session
            .active_media_id
            .and_then(|id| self.project.media(id))
            .map(|m| {
                let dur = m
                    .duration
                    .map(|d| self.fmt_time(d))
                    .unwrap_or_else(|| tr(lang, "estática").to_string());
                let fmt = if m.has_video && m.width > 0 {
                    match m.fps {
                        Some(fps) => format!("{}×{} @ {}", m.width, m.height, fps.label()),
                        None => format!("{}×{}", m.width, m.height),
                    }
                } else if !m.info.is_empty() {
                    m.info.clone()
                } else {
                    media_kind_tag(m).to_string()
                };
                let usage = self.project.media_usage_count(m.id);
                format!(
                    "{}  |  {}  |  {}  |  {}: {usage}",
                    m.name,
                    fmt,
                    dur,
                    tr(lang, "Uso")
                )
            })
            .unwrap_or_else(|| tr(lang, "Selecione uma mídia para ver os detalhes").to_string());

        ui.horizontal(|ui| {
            let count_txt = if shown == total {
                let n_s = total.to_string();
                tr_args(lang, "{n} mídia(s)", &[("n", &n_s)])
            } else {
                format!("{shown}/{total}")
            };
            ui.add(
                egui::Label::new(
                    egui::RichText::new(summary)
                        .small()
                        .color(self.theme.text_muted),
                )
                .truncate(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(count_txt)
                        .small()
                        .color(self.theme.text_muted),
                );
            });
        });
        // Origem da mídia selecionada (caminho do arquivo, gerador ou sequência).
        if let Some(m) = self
            .session
            .active_media_id
            .and_then(|id| self.project.media(id))
        {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(media_source_label(&m.source))
                        .small()
                        .color(self.theme.text_muted),
                )
                .wrap(),
            );
        }
    }

    fn draw_compact_bin_selector(&mut self, ui: &mut egui::Ui) {
        let current_label = match self.project_media.filter_target {
            MediaFilterTarget::All => format!("Todas as Mídias ({})", self.project.media.len()),
            MediaFilterTarget::Bin(bid) => self
                .project
                .media_bins
                .iter()
                .find(|b| b.id == bid)
                .map(|b| format!("Pasta: {}", b.name))
                .unwrap_or_else(|| "Pasta".into()),
            MediaFilterTarget::BuiltinSmart(r) => format!("Inteligente: {}", r.label()),
            MediaFilterTarget::CustomSmart(sid) => self
                .project
                .smart_bins
                .iter()
                .find(|s| s.id == sid)
                .map(|s| format!("Inteligente: {}", s.name))
                .unwrap_or_else(|| "Pasta inteligente".into()),
            MediaFilterTarget::Storyboard(st_id) => self
                .project
                .storyboards
                .iter()
                .find(|s| s.id == st_id)
                .map(|s| format!("Storyboard: {}", s.name))
                .unwrap_or_else(|| "Storyboard".into()),
        };
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("pm_compact_filter_combo")
                .selected_text(current_label)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(
                            self.project_media.filter_target == MediaFilterTarget::All,
                            format!("Todas as Mídias ({})", self.project.media.len()),
                        )
                        .clicked()
                    {
                        self.project_media.filter_target = MediaFilterTarget::All;
                    }
                    let bins = self.project.media_bins.clone();
                    for b in bins {
                        let sel = self.project_media.filter_target == MediaFilterTarget::Bin(b.id);
                        if ui
                            .selectable_label(sel, format!("Pasta: {}", b.name))
                            .clicked()
                        {
                            self.project_media.filter_target = MediaFilterTarget::Bin(b.id);
                        }
                    }
                    for r in BUILTIN_SMART_BINS {
                        let sel =
                            self.project_media.filter_target == MediaFilterTarget::BuiltinSmart(r);
                        if ui
                            .selectable_label(sel, format!("Inteligente: {}", r.label()))
                            .clicked()
                        {
                            self.project_media.filter_target = MediaFilterTarget::BuiltinSmart(r);
                        }
                    }
                });
            let bin_btn =
                icons::small_icon_button(ui, Icon::FolderItem, Some("+"), false, &self.theme);
            bin_btn.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Nova pasta (Bin)")
            });
            if bin_btn.clicked() {
                if let Some(bid) = self.session.create_media_bin("", None) {
                    self.project_media.filter_target = MediaFilterTarget::Bin(bid);
                    self.sync_after_session();
                }
            }
            let sb_btn =
                icons::small_icon_button(ui, Icon::SmartFolder, Some("+"), false, &self.theme);
            sb_btn.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Nova pasta inteligente")
            });
            if sb_btn.clicked() {
                self.project_media.editing_smart_id = None;
                self.project_media.smart_dialog_open = true;
            }
        });
    }

    fn sanitize_media_panel_selection(&mut self) {
        if let MediaFilterTarget::Bin(bid) = self.project_media.filter_target {
            if !self.project.media_bins.iter().any(|b| b.id == bid) {
                self.project_media.filter_target = MediaFilterTarget::All;
            }
        }
        if let MediaFilterTarget::CustomSmart(sid) = self.project_media.filter_target {
            if !self.project.smart_bins.iter().any(|b| b.id == sid) {
                self.project_media.filter_target = MediaFilterTarget::All;
            }
        }
        if let MediaFilterTarget::Storyboard(st_id) = self.project_media.filter_target {
            if !self.project.storyboards.iter().any(|s| s.id == st_id) {
                self.project_media.filter_target = MediaFilterTarget::All;
            }
        }
        if self
            .session
            .active_media_id
            .is_some_and(|id| self.project.media(id).is_none())
        {
            self.session.active_media_id = None;
        }
        if self
            .session
            .active_storyboard_id
            .is_some_and(|id| !self.project.storyboards.iter().any(|s| s.id == id))
        {
            self.session.active_storyboard_id = self.project.storyboards.first().map(|s| s.id);
        }
    }

    fn draw_media_toolbar(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            if icons::icon_button(
                ui,
                Icon::ImportMedia,
                Some(tr(lang, "Importar...")),
                false,
                true,
                &self.theme,
            )
            .on_hover_text(tr(lang, "Importar mídia para o projeto"))
            .clicked()
            {
                self.import_dialog();
            }
            if icons::icon_button(
                ui,
                Icon::GeneratorFrame,
                Some(tr(lang, "Sequência de imagens...")),
                false,
                true,
                &self.theme,
            )
            .clicked()
            {
                self.import_sequence_dialog();
            }
            if icons::icon_button(
                ui,
                Icon::ScreenCapture,
                Some(tr(lang, "Neoveg Capture...")),
                self.detached_tabs.contains(&crate::DockTab::Capture),
                true,
                &self.theme,
            )
            .on_hover_text(tr(
                lang,
                "Gravar tela, janelas, câmeras e áudio (Neoveg Capture)",
            ))
            .clicked()
            {
                self.exec_cmd(Some(ui.ctx()), "file.capture");
            }
            if icons::icon_button(
                ui,
                Icon::DeleteTrash,
                Some(tr(lang, "Remover não usadas")),
                false,
                true,
                &self.theme,
            )
            .on_hover_text(tr(
                lang,
                "Remover mídias que não estão em nenhuma sequência",
            ))
            .clicked()
            {
                self.remove_unused_media();
            }
            let active_mid = self.session.active_media_id;
            if icons::icon_button(
                ui,
                Icon::TrimmerBlade,
                None,
                false,
                active_mid.is_some(),
                &self.theme,
            )
            .on_hover_text(tr(lang, "Abrir mídia selecionada no Trimmer"))
            .clicked()
            {
                if let Some(mid) = active_mid {
                    self.open_in_trimmer(mid);
                }
            }
            ui.separator();
            for mode in MediaViewMode::ALL {
                let active = self.project_media.view_mode == mode;
                let lbl = tr(lang, mode.label());
                let resp =
                    icons::icon_button(ui, mode.icon(), Some(lbl), active, true, &self.theme);
                if resp
                    .on_hover_text(tr_args(lang, "Exibir como {mode}", &[("mode", &lbl)]))
                    .clicked()
                {
                    self.project_media.view_mode = mode;
                    if mode == MediaViewMode::Storyboard && self.project.storyboards.is_empty() {
                        self.session.create_storyboard("Storyboard 1");
                        self.sync_after_session();
                    }
                }
            }
            if self.project_media.view_mode == MediaViewMode::Thumbnails {
                ui.separator();
                for sz in MediaThumbSize::ALL {
                    let active = self.project_media.thumb_size == sz;
                    if ui
                        .selectable_label(active, sz.short_label())
                        .on_hover_text(tr(lang, sz.tip_key()))
                        .clicked()
                    {
                        self.project_media.thumb_size = sz;
                    }
                }
            }
            ui.separator();
            ui.add(
                egui::TextEdit::singleline(&mut self.project_media.search)
                    .hint_text(tr(lang, "Filtrar mídias..."))
                    .desired_width(140.0),
            );
            if !self.project_media.search.is_empty()
                && ui.small_button(tr(lang, "Limpar")).clicked()
            {
                self.project_media.search.clear();
            }
        });
    }

    fn draw_smart_bin_editor(&mut self, ui: &mut egui::Ui) {
        if !self.project_media.smart_dialog_open {
            return;
        }
        let mut close = false;
        let mut save = false;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("Pasta inteligente:").strong());
                ui.add(
                    egui::TextEdit::singleline(&mut self.project_media.smart_name_buf)
                        .desired_width(140.0)
                        .hint_text("Nome"),
                );
                egui::ComboBox::from_id_salt("smart_bin_rule_combo")
                    .selected_text(self.project_media.smart_rule_buf.label())
                    .show_ui(ui, |ui| {
                        for r in SmartBinRule::ALL {
                            ui.selectable_value(
                                &mut self.project_media.smart_rule_buf,
                                r,
                                r.label(),
                            );
                        }
                    });
                ui.add(
                    egui::TextEdit::singleline(&mut self.project_media.smart_query_buf)
                        .desired_width(130.0)
                        .hint_text("Contém texto..."),
                );
                if ui.button("Salvar filtro").clicked() {
                    save = true;
                }
                if ui.button("Cancelar").clicked() {
                    close = true;
                }
            });
        });
        if save {
            let name = self.project_media.smart_name_buf.clone();
            let rule = self.project_media.smart_rule_buf;
            let query = self.project_media.smart_query_buf.clone();
            if let Some(eid) = self.project_media.editing_smart_id {
                self.session.update_smart_bin(eid, &name, rule, &query);
                self.project_media.filter_target = MediaFilterTarget::CustomSmart(eid);
            } else if let Some(nid) = self.session.create_smart_bin(&name, rule, &query) {
                self.project_media.filter_target = MediaFilterTarget::CustomSmart(nid);
            }
            self.sync_after_session();
            self.project_media.editing_smart_id = None;
            close = true;
        }
        if close {
            self.project_media.smart_dialog_open = false;
        }
    }

    fn draw_bins_rail(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let total = self.project.media.len();
        let all_sel = self.project_media.filter_target == MediaFilterTarget::All;
        ui.horizontal(|ui| {
            icons::paint_inline(ui, Icon::MediaItem, 13.0, &self.theme);
            if ui
                .selectable_label(
                    all_sel,
                    format!("{} ({total})", tr(lang, "Todas as Mídias")),
                )
                .clicked()
            {
                self.project_media.filter_target = MediaFilterTarget::All;
                if self.project_media.view_mode == MediaViewMode::Storyboard {
                    self.project_media.view_mode = MediaViewMode::Thumbnails;
                }
            }
        });

        ui.separator();
        let used = self.project.used_media_ids();
        // Mídia por Tipo recolhível (VEGAS Pro 21, p. 52: Media By Type -> Video/Audio/Stills/Generated/Unused)
        egui::CollapsingHeader::new(
            egui::RichText::new(tr(lang, "Mídia por Tipo"))
                .strong()
                .small(),
        )
        .id_salt("pm_media_by_type_hdr")
        .default_open(true)
        .show(ui, |ui| {
            for rule in BUILTIN_SMART_BINS {
                let count = self
                    .project
                    .media
                    .iter()
                    .filter(|m| rule.matches("", m, &used))
                    .count();
                let sel = self.project_media.filter_target == MediaFilterTarget::BuiltinSmart(rule);
                ui.horizontal(|ui| {
                    icons::paint_inline(ui, Icon::SmartFolder, 12.0, &self.theme);
                    if ui
                        .selectable_label(sel, format!("{} ({count})", tr(lang, rule.label())))
                        .clicked()
                    {
                        self.project_media.filter_target = MediaFilterTarget::BuiltinSmart(rule);
                        if self.project_media.view_mode == MediaViewMode::Storyboard {
                            self.project_media.view_mode = MediaViewMode::Thumbnails;
                        }
                    }
                });
            }
        });

        ui.separator();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(tr(lang, "Pastas (Bins)"))
                    .strong()
                    .small(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r =
                    icons::small_icon_button(ui, Icon::FolderItem, Some("+"), false, &self.theme);
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        tr(lang, "Nova pasta (Bin)"),
                    )
                });
                if r.on_hover_text(tr(lang, "Criar nova pasta de mídia (Bin)"))
                    .clicked()
                {
                    let parent = match self.project_media.filter_target {
                        MediaFilterTarget::Bin(pid) => Some(pid),
                        _ => None,
                    };
                    if let Some(bid) = self.session.create_media_bin("", parent) {
                        self.project_media.filter_target = MediaFilterTarget::Bin(bid);
                        self.sync_after_session();
                    }
                }
            });
        });

        let bins = self.project.media_bins.clone();
        let mut drop_assign: Option<(Id, Id)> = None;
        let mut rename_commit: Option<(Id, String)> = None;
        let mut create_sub: Option<Id> = None;
        let mut delete_bin: Option<Id> = None;
        self.draw_bin_tree_recursive(
            ui,
            &bins,
            None,
            0,
            &mut drop_assign,
            &mut rename_commit,
            &mut create_sub,
            &mut delete_bin,
        );
        if let Some((mid, bid)) = drop_assign {
            if self.session.assign_media_to_bin(mid, Some(bid)) {
                self.sync_after_session();
            }
        }
        if let Some((bid, name)) = rename_commit {
            if self.session.rename_media_bin(bid, &name) {
                self.sync_after_session();
            }
            self.project_media.renaming_bin = None;
        }
        if let Some(pid) = create_sub {
            if let Some(bid) = self.session.create_media_bin("", Some(pid)) {
                self.project_media.filter_target = MediaFilterTarget::Bin(bid);
                self.sync_after_session();
            }
        }
        if let Some(bid) = delete_bin {
            if self.session.delete_media_bin(bid) {
                if self.project_media.filter_target == MediaFilterTarget::Bin(bid) {
                    self.project_media.filter_target = MediaFilterTarget::All;
                }
                self.sync_after_session();
            }
        }

        ui.separator();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(tr(lang, "Pastas Inteligentes"))
                    .strong()
                    .small(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r =
                    icons::small_icon_button(ui, Icon::SmartFolder, Some("+"), false, &self.theme);
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        "Nova pasta inteligente",
                    )
                });
                if r.on_hover_text("Criar pasta inteligente personalizada")
                    .clicked()
                {
                    self.project_media.editing_smart_id = None;
                    self.project_media.smart_name_buf = "Filtro personalizado".into();
                    self.project_media.smart_rule_buf = SmartBinRule::NameContains;
                    self.project_media.smart_query_buf.clear();
                    self.project_media.smart_dialog_open = true;
                }
            });
        });

        let custom_bins = self.project.smart_bins.clone();
        let mut del_smart = None;
        for sb in custom_bins {
            let count = self
                .project
                .media
                .iter()
                .filter(|m| sb.rule.matches(&sb.query, m, &used))
                .count();
            let sel = self.project_media.filter_target == MediaFilterTarget::CustomSmart(sb.id);
            let resp = ui
                .horizontal(|ui| {
                    icons::paint_inline(ui, Icon::SmartFolder, 12.0, &self.theme);
                    ui.selectable_label(sel, format!("{} ({count})", sb.name))
                })
                .inner;
            if resp.clicked() {
                self.project_media.filter_target = MediaFilterTarget::CustomSmart(sb.id);
                if self.project_media.view_mode == MediaViewMode::Storyboard {
                    self.project_media.view_mode = MediaViewMode::Thumbnails;
                }
            }
            resp.context_menu(|ui| {
                if ui.button("Editar regra...").clicked() {
                    self.project_media.editing_smart_id = Some(sb.id);
                    self.project_media.smart_name_buf = sb.name.clone();
                    self.project_media.smart_rule_buf = sb.rule;
                    self.project_media.smart_query_buf = sb.query.clone();
                    self.project_media.smart_dialog_open = true;
                    ui.close_menu();
                }
                if ui.button("Excluir pasta inteligente").clicked() {
                    del_smart = Some(sb.id);
                    ui.close_menu();
                }
            });
        }
        if let Some(sid) = del_smart {
            self.session.delete_smart_bin(sid);
            self.sync_after_session();
        }

        ui.separator();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Storyboards").strong().small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r = icons::small_icon_button(
                    ui,
                    Icon::StoryboardGrid,
                    Some("+"),
                    false,
                    &self.theme,
                );
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Novo storyboard")
                });
                if r.on_hover_text("Criar novo storyboard").clicked() {
                    if let Some(id) = self.session.create_storyboard("") {
                        self.project_media.view_mode = MediaViewMode::Storyboard;
                        self.project_media.filter_target = MediaFilterTarget::Storyboard(id);
                        self.sync_after_session();
                    }
                }
            });
        });

        let storyboards = self.project.storyboards.clone();
        let mut del_sb = None;
        for st in storyboards {
            let active = self.session.active_storyboard_id == Some(st.id)
                && self.project_media.view_mode == MediaViewMode::Storyboard;
            let resp = ui.selectable_label(active, format!("{} ({})", st.name, st.items.len()));
            if resp.clicked() {
                self.session.active_storyboard_id = Some(st.id);
                self.project_media.view_mode = MediaViewMode::Storyboard;
                self.project_media.filter_target = MediaFilterTarget::Storyboard(st.id);
            }
            resp.context_menu(|ui| {
                if ui.button("Inserir na timeline").clicked() {
                    let cur = self.cursor;
                    self.session.insert_storyboard_at_cursor(
                        st.id,
                        cur,
                        self.prefs.still_image_secs,
                    );
                    self.sync_after_session();
                    ui.close_menu();
                }
                if ui.button("Excluir storyboard").clicked() {
                    del_sb = Some(st.id);
                    ui.close_menu();
                }
            });
        }
        if let Some(id) = del_sb {
            self.session.delete_storyboard(id);
            self.sync_after_session();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_bin_tree_recursive(
        &mut self,
        ui: &mut egui::Ui,
        bins: &[MediaBin],
        parent: Option<Id>,
        depth: usize,
        drop_assign: &mut Option<(Id, Id)>,
        rename_commit: &mut Option<(Id, String)>,
        create_sub: &mut Option<Id>,
        delete_bin: &mut Option<Id>,
    ) {
        if depth >= MAX_BIN_DEPTH {
            return;
        }
        for bin in bins.iter().filter(|b| b.parent_id == parent) {
            if self.project_media.renaming_bin == Some(bin.id) {
                let mut enter_pressed = false;
                ui.horizontal(|ui| {
                    ui.add_space(depth as f32 * 12.0);
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.project_media.bin_name_buf)
                            .desired_width(110.0),
                    );
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        enter_pressed = true;
                    }
                    if ui.small_button("OK").clicked() || enter_pressed {
                        *rename_commit = Some((bin.id, self.project_media.bin_name_buf.clone()));
                    }
                });
            } else {
                let sel = self.project_media.filter_target == MediaFilterTarget::Bin(bin.id);
                let count = self.project.collect_bin_media(bin.id, true).len();
                let resp = ui
                    .horizontal(|ui| {
                        ui.add_space(depth as f32 * 12.0);
                        ui.dnd_drop_zone::<DndPayload, ()>(egui::Frame::NONE, |ui| {
                            ui.horizontal(|ui| {
                                icons::paint_inline(ui, Icon::FolderItem, 12.0, &self.theme);
                                let label = format!("{} ({count})", bin.name);
                                let r = ui.selectable_label(sel, label);
                                if r.clicked() {
                                    self.project_media.filter_target =
                                        MediaFilterTarget::Bin(bin.id);
                                    if self.project_media.view_mode == MediaViewMode::Storyboard {
                                        self.project_media.view_mode = MediaViewMode::Thumbnails;
                                    }
                                }
                                let lang = self.lang();
                                r.context_menu(|ui| {
                                    if ui.button(tr(lang, "Nova subpasta")).clicked() {
                                        *create_sub = Some(bin.id);
                                        ui.close_menu();
                                    }
                                    if ui.button(tr(lang, "Renomear pasta")).clicked() {
                                        self.project_media.renaming_bin = Some(bin.id);
                                        self.project_media.bin_name_buf = bin.name.clone();
                                        ui.close_menu();
                                    }
                                    if ui.button(tr(lang, "Excluir pasta")).clicked() {
                                        *delete_bin = Some(bin.id);
                                        ui.close_menu();
                                    }
                                });
                            });
                        })
                    })
                    .inner;
                if let Some(payload) = resp.1 {
                    if let DndPayload::Media(mid) = *payload {
                        *drop_assign = Some((mid, bin.id));
                    }
                }
            }
            self.draw_bin_tree_recursive(
                ui,
                bins,
                Some(bin.id),
                depth + 1,
                drop_assign,
                rename_commit,
                create_sub,
                delete_bin,
            );
        }
    }

    fn draw_media_collection(&mut self, ui: &mut egui::Ui) {
        let items = self.filtered_project_media();
        if items.is_empty() {
            ui.label(
                egui::RichText::new(
                    "Nenhuma mídia nesta visualização. Arraste arquivos ou clique em Importar...",
                )
                .small()
                .weak(),
            );
            return;
        }
        let ctx = ui.ctx().clone();
        let mut actions: Vec<MediaRowAction> = Vec::new();
        egui::ScrollArea::both()
            .id_salt("pm_collection_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| match self.project_media.view_mode {
                MediaViewMode::Thumbnails => {
                    self.draw_media_thumbnails(ui, &ctx, &items, &mut actions)
                }
                MediaViewMode::List => self.draw_media_list(ui, &items, &mut actions),
                MediaViewMode::Details => self.draw_media_details(ui, &items, &mut actions),
                MediaViewMode::Storyboard => {}
            });
        self.apply_media_actions(actions);
    }

    fn draw_media_thumbnails(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        items: &[MediaItem],
        actions: &mut Vec<MediaRowAction>,
    ) {
        let lang = self.lang();
        let card_w = self.project_media.thumb_size.card_width();
        let vis_h = ui.clip_rect().height();
        let thumb_h = if vis_h.is_finite() && vis_h > 48.0 {
            self.project_media
                .thumb_size
                .thumb_height()
                .min((vis_h - 30.0).max(42.0))
        } else {
            self.project_media.thumb_size.thumb_height()
        };
        // Colunas pela largura visível (a área rola nos dois eixos, então a
        // largura disponível não limita nada) e pela largura real do cartão no
        // quadro anterior: o nome ao lado da miniatura alarga o cartão, e com a
        // largura nominal a última coluna ficava cortada à direita.
        let measured_id = egui::Id::new("pm_thumb_card_measured_w");
        let real_w = ui
            .ctx()
            .data(|d| d.get_temp::<f32>(measured_id))
            .unwrap_or(card_w)
            .max(card_w);
        let cols = thumb_grid_columns(ui.clip_rect().width(), real_w);
        let mut widest = card_w;
        let paint_aspect_tex = |painter: &egui::Painter, rect: Rect, t: &TextureHandle| {
            let [tw, th] = t.size();
            let asp = (tw.max(1) as f32) / (th.max(1) as f32);
            let mut dw = rect.width();
            let mut dh = dw / asp.max(0.01);
            if dh > rect.height() {
                dh = rect.height();
                dw = dh * asp;
            }
            let fit_r = Rect::from_center_size(rect.center(), vec2(dw, dh));
            painter.image(
                t.id(),
                fit_r,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        };
        egui::Grid::new("pm_thumbs_grid")
            .spacing([8.0, 8.0])
            .show(ui, |ui| {
                for (idx, m) in items.iter().enumerate() {
                    let selected = self.session.active_media_id == Some(m.id);
                    let frame_fill = if selected {
                        self.theme.selection_bg
                    } else {
                        self.theme.panel_fill
                    };
                    let stroke = Stroke::new(
                        if selected { 1.5_f32 } else { 1.0_f32 },
                        if selected {
                            self.theme.accent
                        } else {
                            self.theme.border
                        },
                    );
                    let id = egui::Id::new(("pm_thumb_card", m.id));
                    let dur = m
                        .duration
                        .map(|d| self.fmt_time(d))
                        .unwrap_or_else(|| tr(lang, "estática").to_string());
                    let tag = media_kind_tag(m);
                    let use_cnt = self.project.media_usage_count(m.id);
                    let offline = !self.media.contains(m.id)
                        && !matches!(
                            m.source,
                            MediaSource::Generated(_) | MediaSource::Sequence { .. }
                        );
                    let card_resp = ui
                        .scope(|ui| {
                            ui.set_min_width(card_w);
                            ui.set_max_width(card_w);
                            egui::Frame::new()
                                .fill(frame_fill)
                                .stroke(stroke)
                                .corner_radius(2.0)
                                .inner_margin(4.0)
                                .show(ui, |ui| {
                                    ui.vertical(|ui| {
                                        ui.set_width(card_w - 8.0);
                                        ui.set_max_width(card_w - 8.0);
                                        let thumb_size = vec2(card_w - 8.0, thumb_h);
                                        let (thumb_rect, thumb_sense) =
                                            ui.allocate_exact_size(thumb_size, Sense::hover());
                                        if !self.theme.high_contrast
                                            && (matches!(m.source, MediaSource::Generated(_))
                                                || (m.has_video && m.duration.is_none()))
                                        {
                                            paint_alpha_checkerboard(ui.painter(), thumb_rect);
                                        } else {
                                            ui.painter().rect_filled(
                                                thumb_rect,
                                                1.0,
                                                self.theme.extreme_bg,
                                            );
                                        }
                                        let pointer_hover = thumb_sense
                                            .hovered()
                                            .then(|| ui.input(|i| i.pointer.hover_pos()))
                                            .flatten();
                                        let scrubbing =
                                            pointer_hover.filter(|p| thumb_rect.contains(*p));
                                        if let Some(pos) = scrubbing {
                                            let frac = ((pos.x - thumb_rect.left())
                                                / thumb_rect.width().max(1.0))
                                            .clamp(0.0, 1.0);
                                            let (scrub_tex, sample_t) =
                                                self.thumb_scrub(ctx, m.id, frac);
                                            if let Some(t) = scrub_tex {
                                                paint_aspect_tex(ui.painter(), thumb_rect, &t);
                                            }
                                            let sx = thumb_rect.left() + frac * thumb_rect.width();
                                            ui.painter().line_segment(
                                                [
                                                    pos2(sx, thumb_rect.top()),
                                                    pos2(sx, thumb_rect.bottom()),
                                                ],
                                                Stroke::new(1.5_f32, self.theme.accent),
                                            );
                                            let tc = self.fmt_time(sample_t);
                                            ui.painter().rect_filled(
                                                Rect::from_min_max(
                                                    pos2(
                                                        thumb_rect.left() + 2.0,
                                                        thumb_rect.bottom() - 15.0,
                                                    ),
                                                    pos2(
                                                        thumb_rect.right() - 2.0,
                                                        thumb_rect.bottom() - 1.0,
                                                    ),
                                                ),
                                                2.0,
                                                Color32::from_black_alpha(190),
                                            );
                                            ui.painter().text(
                                                pos2(
                                                    thumb_rect.center().x,
                                                    thumb_rect.bottom() - 8.0,
                                                ),
                                                egui::Align2::CENTER_CENTER,
                                                tc,
                                                egui::FontId::monospace(9.5),
                                                Color32::WHITE,
                                            );
                                        } else {
                                            if let Some(t) = self.thumb(ctx, m.id) {
                                                paint_aspect_tex(ui.painter(), thumb_rect, &t);
                                            } else {
                                                let icon = if matches!(
                                                    m.source,
                                                    MediaSource::Sequence { .. }
                                                        | MediaSource::NestedProject { .. }
                                                ) {
                                                    Icon::NestedTimeline
                                                } else if m.has_audio {
                                                    Icon::AudioTrack
                                                } else {
                                                    Icon::MediaItem
                                                };
                                                let icon_box = Rect::from_center_size(
                                                    thumb_rect.center(),
                                                    vec2(24.0, 24.0),
                                                );
                                                icons::paint(
                                                    ui.painter(),
                                                    icon_box,
                                                    icon,
                                                    &self.theme,
                                                    true,
                                                );
                                            }
                                            // Selo superior esquerdo (tipo de mídia, estilo VEGAS Pro)
                                            let tag_r = Rect::from_min_size(
                                                thumb_rect.left_top() + vec2(3.0, 3.0),
                                                vec2(28.0, 14.0),
                                            );
                                            ui.painter().rect_filled(
                                                tag_r,
                                                2.0,
                                                Color32::from_black_alpha(175),
                                            );
                                            ui.painter().text(
                                                tag_r.center(),
                                                egui::Align2::CENTER_CENTER,
                                                tag,
                                                egui::FontId::proportional(9.0),
                                                Color32::WHITE,
                                            );
                                            // Selo superior direito (contagem de uso na timeline)
                                            if use_cnt > 0 {
                                                let cnt_s = format!("{use_cnt}×");
                                                let cnt_w = (cnt_s.len() as f32 * 5.8 + 8.0)
                                                    .clamp(22.0, 44.0);
                                                let cnt_r = Rect::from_min_size(
                                                    pos2(
                                                        thumb_rect.right() - cnt_w - 3.0,
                                                        thumb_rect.top() + 3.0,
                                                    ),
                                                    vec2(cnt_w, 14.0),
                                                );
                                                ui.painter().rect_filled(
                                                    cnt_r,
                                                    2.0,
                                                    Color32::from_black_alpha(175),
                                                );
                                                ui.painter().text(
                                                    cnt_r.center(),
                                                    egui::Align2::CENTER_CENTER,
                                                    cnt_s,
                                                    egui::FontId::monospace(9.0),
                                                    Color32::WHITE,
                                                );
                                            }
                                            // Selo inferior direito (duração)
                                            let dur_w = (dur.len() as f32 * 5.6 + 8.0)
                                                .clamp(42.0, thumb_rect.width() - 6.0);
                                            let dur_r = Rect::from_min_size(
                                                pos2(
                                                    thumb_rect.right() - dur_w - 3.0,
                                                    thumb_rect.bottom() - 16.0,
                                                ),
                                                vec2(dur_w, 13.0),
                                            );
                                            ui.painter().rect_filled(
                                                dur_r,
                                                2.0,
                                                Color32::from_black_alpha(180),
                                            );
                                            ui.painter().text(
                                                dur_r.center(),
                                                egui::Align2::CENTER_CENTER,
                                                &dur,
                                                egui::FontId::monospace(9.0),
                                                Color32::WHITE,
                                            );
                                        }
                                        ui.painter().rect_stroke(
                                            thumb_rect,
                                            1.0,
                                            Stroke::new(1.0_f32, self.theme.border),
                                            egui::StrokeKind::Inside,
                                        );
                                        let name_fg = if selected {
                                            crate::theme::contrast_on(self.theme.selection_bg)
                                        } else {
                                            self.theme.text_primary
                                        };
                                        ui.horizontal(|ui| {
                                            ui.set_max_width(card_w - 8.0);
                                            ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(&m.name)
                                                        .small()
                                                        .color(name_fg),
                                                )
                                                .truncate(),
                                            );
                                            if offline {
                                                ui.colored_label(
                                                    self.theme.offline_fg,
                                                    egui::RichText::new("offline").small(),
                                                );
                                            }
                                        });
                                    });
                                });
                        })
                        .response;
                    let fmt_tip = if m.has_video && m.width > 0 {
                        match m.fps {
                            Some(fps) => format!("{}×{} @ {}", m.width, m.height, fps.label()),
                            None => format!("{}×{}", m.width, m.height),
                        }
                    } else {
                        m.info.clone()
                    };
                    let tip = format!(
                        "{}\n{} • {} • {}: {use_cnt}\n{}",
                        m.name,
                        tag,
                        dur,
                        tr(lang, "Uso"),
                        fmt_tip
                    );
                    let resp = ui
                        .interact(card_resp.rect, id, Sense::click_and_drag())
                        .on_hover_text(tip);
                    resp.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &m.name)
                    });
                    if resp.drag_started() {
                        self.session.active_media_id = Some(m.id);
                    }
                    if resp.drag_started() || resp.dragged() {
                        egui::DragAndDrop::set_payload(ui.ctx(), DndPayload::Media(m.id));
                        paint_media_drag_ghost(ui.ctx(), &m.name, &self.theme);
                    }
                    if resp.clicked() {
                        self.session.active_media_id = Some(m.id);
                    }
                    if resp.double_clicked() {
                        self.session.active_media_id = Some(m.id);
                        actions.push(double_click_action(
                            self.prefs.double_click_loads_trimmer,
                            m.id,
                        ));
                    }
                    widest = widest.max(resp.rect.width());
                    self.media_item_context_menu(&resp, m, actions);
                    if (idx + 1) % cols == 0 {
                        ui.end_row();
                    }
                }
            });
        if (widest - real_w).abs() > 0.5 {
            ui.ctx().data_mut(|d| d.insert_temp(measured_id, widest));
            ui.ctx().request_repaint();
        }
    }

    fn draw_media_list(
        &mut self,
        ui: &mut egui::Ui,
        items: &[MediaItem],
        actions: &mut Vec<MediaRowAction>,
    ) {
        for m in items {
            let selected = self.session.active_media_id == Some(m.id);
            let id = egui::Id::new(("pm_list_row", m.id));
            let row_resp = ui
                .scope(|ui| {
                    ui.horizontal(|ui| {
                        icons::paint_inline(
                            ui,
                            if matches!(
                                m.source,
                                MediaSource::Sequence { .. } | MediaSource::NestedProject { .. }
                            ) {
                                Icon::NestedTimeline
                            } else if m.has_video {
                                Icon::MediaItem
                            } else {
                                Icon::AudioTrack
                            },
                            14.0,
                            &self.theme,
                        );
                        let dur = m
                            .duration
                            .map(|d| self.fmt_time(d))
                            .unwrap_or_else(|| "estática".into());
                        let mut text = egui::RichText::new(format!(
                            "{}   [{}]   {}",
                            m.name,
                            media_kind_tag(m),
                            dur
                        ));
                        if selected {
                            text = text.strong().color(self.theme.accent);
                        }
                        ui.label(text);
                    });
                })
                .response;
            let resp = ui.interact(row_resp.rect, id, Sense::click_and_drag());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &m.name));
            if resp.drag_started() {
                self.session.active_media_id = Some(m.id);
            }
            if resp.drag_started() || resp.dragged() {
                egui::DragAndDrop::set_payload(ui.ctx(), DndPayload::Media(m.id));
                paint_media_drag_ghost(ui.ctx(), &m.name, &self.theme);
            }
            if resp.clicked() {
                self.session.active_media_id = Some(m.id);
            }
            if resp.double_clicked() {
                actions.push(double_click_action(
                    self.prefs.double_click_loads_trimmer,
                    m.id,
                ));
            }
            self.media_item_context_menu(&resp, m, actions);
        }
    }

    fn draw_media_details(
        &mut self,
        ui: &mut egui::Ui,
        items: &[MediaItem],
        actions: &mut Vec<MediaRowAction>,
    ) {
        let lang = self.lang();
        egui::Grid::new("pm_details_table")
            .striped(true)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                self.sort_header_btn(ui, tr(lang, "Nome"), MediaSortCol::Name);
                self.sort_header_btn(ui, tr(lang, "Tipo"), MediaSortCol::Type);
                self.sort_header_btn(ui, tr(lang, "Duração"), MediaSortCol::Duration);
                ui.label(egui::RichText::new(tr(lang, "Formato")).strong());
                self.sort_header_btn(ui, tr(lang, "Uso"), MediaSortCol::Usage);
                ui.label(egui::RichText::new(tr(lang, "Origem")).strong());
                ui.end_row();

                for m in items {
                    let selected = self.session.active_media_id == Some(m.id);
                    let id = egui::Id::new(("pm_det_row", m.id));
                    let row_resp = ui
                        .scope(|ui| {
                            ui.horizontal(|ui| {
                                ui.set_max_width(220.0);
                                icons::paint_inline(
                                    ui,
                                    if matches!(
                                        m.source,
                                        MediaSource::Sequence { .. }
                                            | MediaSource::NestedProject { .. }
                                    ) {
                                        Icon::NestedTimeline
                                    } else if m.has_video {
                                        Icon::MediaItem
                                    } else {
                                        Icon::AudioTrack
                                    },
                                    13.0,
                                    &self.theme,
                                );
                                let label = if selected {
                                    egui::RichText::new(&m.name)
                                        .strong()
                                        .color(self.theme.accent)
                                } else {
                                    egui::RichText::new(&m.name)
                                };
                                ui.add(egui::Label::new(label).truncate());
                            });
                        })
                        .response;
                    let resp = ui
                        .interact(row_resp.rect, id, Sense::click_and_drag())
                        .on_hover_text(&m.name);
                    resp.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &m.name)
                    });
                    if resp.drag_started() {
                        self.session.active_media_id = Some(m.id);
                    }
                    if resp.drag_started() || resp.dragged() {
                        egui::DragAndDrop::set_payload(ui.ctx(), DndPayload::Media(m.id));
                        paint_media_drag_ghost(ui.ctx(), &m.name, &self.theme);
                    }
                    if resp.clicked() {
                        self.session.active_media_id = Some(m.id);
                    }
                    if resp.double_clicked() {
                        actions.push(double_click_action(
                            self.prefs.double_click_loads_trimmer,
                            m.id,
                        ));
                    }
                    self.media_item_context_menu(&resp, m, actions);

                    ui.label(media_kind_tag(m));
                    ui.label(
                        m.duration
                            .map(|d| self.fmt_time(d))
                            .unwrap_or_else(|| tr(lang, "estática").to_string()),
                    );
                    let fmt = if m.has_video && m.width > 0 {
                        match m.fps {
                            Some(fps) => format!("{}×{} @ {}", m.width, m.height, fps.label()),
                            None => format!("{}×{}", m.width, m.height),
                        }
                    } else {
                        m.info.clone()
                    };
                    ui.add(egui::Label::new(egui::RichText::new(fmt).small()).truncate());
                    let use_cnt = self.project.media_usage_count(m.id);
                    ui.label(format!("{use_cnt}"));
                    let src_txt = media_source_label(&m.source);
                    ui.add(
                        egui::Label::new(egui::RichText::new(&src_txt).small().weak()).truncate(),
                    )
                    .on_hover_text(src_txt);
                    ui.end_row();
                }
            });
    }

    fn sort_header_btn(&mut self, ui: &mut egui::Ui, label: &str, col: MediaSortCol) {
        let mark = if self.project_media.sort_col == col {
            if self.project_media.sort_asc {
                " ^"
            } else {
                " v"
            }
        } else {
            ""
        };
        if ui
            .button(egui::RichText::new(format!("{label}{mark}")).strong())
            .clicked()
        {
            if self.project_media.sort_col == col {
                self.project_media.sort_asc = !self.project_media.sort_asc;
            } else {
                self.project_media.sort_col = col;
                self.project_media.sort_asc = true;
            }
        }
    }

    fn media_item_context_menu(
        &self,
        resp: &egui::Response,
        m: &MediaItem,
        actions: &mut Vec<MediaRowAction>,
    ) {
        let has_event_sel = !self.sel_events.is_empty();
        let active_bin = match self.project_media.filter_target {
            MediaFilterTarget::Bin(bid) => Some(bid),
            _ => None,
        };
        let use_cnt = self.project.media_usage_count(m.id);
        let bins = self.project.media_bins.clone();
        let storyboards = self.project.storyboards.clone();
        let other_media: Vec<(Id, String)> = self
            .project
            .media
            .iter()
            .filter(|o| o.id != m.id)
            .map(|o| (o.id, o.name.clone()))
            .collect();

        resp.context_menu(|ui| {
            if ui.button("Adicionar no cursor").clicked() {
                actions.push(MediaRowAction::AddAtCursor(m.id));
                ui.close_menu();
            }
            if ui.button("Abrir no Trimmer").clicked() {
                actions.push(MediaRowAction::OpenTrimmer(m.id));
                ui.close_menu();
            }
            if matches!(m.source, MediaSource::NestedProject { .. })
                && ui
                    .button(neoveg_common::tr(
                        self.lang(),
                        "Abrir projeto aninhado em aba",
                    ))
                    .clicked()
            {
                actions.push(MediaRowAction::OpenNestedTab(m.id));
                ui.close_menu();
            }
            if ui
                .add_enabled(
                    has_event_sel,
                    egui::Button::new("Adicionar como take aos eventos selecionados"),
                )
                .clicked()
            {
                actions.push(MediaRowAction::AddAsTake(m.id));
                ui.close_menu();
            }
            if ui
                .button(format!("Selecionar eventos na timeline ({use_cnt})"))
                .clicked()
            {
                actions.push(MediaRowAction::SelectEvents(m.id));
                ui.close_menu();
            }
            ui.separator();
            if ui
                .button("Substituir mídia (arquivo do disco)...")
                .clicked()
            {
                actions.push(MediaRowAction::ReplaceFromDisk(m.id));
                ui.close_menu();
            }
            if !other_media.is_empty() {
                ui.menu_button("Trocar eventos para outra mídia", |ui| {
                    for (oid, oname) in &other_media {
                        if ui.button(oname).clicked() {
                            actions.push(MediaRowAction::SwapExisting(m.id, *oid));
                            ui.close_menu();
                        }
                    }
                });
            }
            ui.menu_button("Mover para pasta (Bin)", |ui| {
                if let Some(bid) = active_bin {
                    if ui.button("Remover desta pasta").clicked() {
                        actions.push(MediaRowAction::RemoveFromBin(m.id, bid));
                        ui.close_menu();
                    }
                }
                if ui.button("Raiz (remover de todas as pastas)").clicked() {
                    actions.push(MediaRowAction::AssignBin(m.id, None));
                    ui.close_menu();
                }
                for b in &bins {
                    if ui.button(&b.name).clicked() {
                        actions.push(MediaRowAction::AssignBin(m.id, Some(b.id)));
                        ui.close_menu();
                    }
                }
            });
            ui.menu_button("Adicionar ao Storyboard", |ui| {
                for st in &storyboards {
                    if ui.button(&st.name).clicked() {
                        actions.push(MediaRowAction::AddToStoryboard(st.id, m.id));
                        ui.close_menu();
                    }
                }
                if ui.button("Novo storyboard com esta mídia").clicked() {
                    actions.push(MediaRowAction::CreateStoryboardWith(m.id));
                    ui.close_menu();
                }
            });
            ui.separator();
            if m.can_build_proxy() && ui.button(tr(self.lang(), "Gerar proxy de vídeo")).clicked()
            {
                actions.push(MediaRowAction::BuildProxy(m.id));
                ui.close_menu();
            }
            if matches!(m.source, MediaSource::Generated(_))
                && ui.button("Editar mídia gerada...").clicked()
            {
                actions.push(MediaRowAction::EditGenerated(m.id));
                ui.close_menu();
            }
            ui.menu_button("Abrir em aplicativo externo", |ui| {
                use neoveg_engine::integrations::ExternalApp;
                for app in ExternalApp::ALL {
                    let show = (m.has_video && app.can_edit_video_or_still())
                        || (m.has_audio && app.can_edit_audio());
                    if show && ui.button(format!("{}...", app.name())).clicked() {
                        actions.push(MediaRowAction::OpenExternal(app, m.id));
                        ui.close_menu();
                    }
                }
            });
            if ui.button("Remover do projeto").clicked() {
                actions.push(MediaRowAction::RemoveFromProject(m.id));
                ui.close_menu();
            }
        });
    }

    fn apply_media_actions(&mut self, actions: Vec<MediaRowAction>) {
        for act in actions {
            match act {
                MediaRowAction::AddAtCursor(id) => {
                    self.checkpoint();
                    let hint = self.sel_track;
                    self.add_media_to_timeline(id, self.cursor, hint);
                }
                MediaRowAction::OpenTrimmer(id) => self.open_in_trimmer(id),
                MediaRowAction::OpenNestedTab(id) => self.open_nested_media_tab(id),
                MediaRowAction::AddAsTake(id) => {
                    let n = self.session.add_media_as_take_to_selected(id);
                    self.set_status(if n == 0 {
                        "Nenhum evento selecionado compatível recebeu o take.".to_string()
                    } else {
                        format!("Take adicionado a {n} evento(s)")
                    });
                }
                MediaRowAction::SelectEvents(id) => {
                    self.session.select_events_using_media(id);
                    self.sync_status_from_session();
                }
                MediaRowAction::ReplaceFromDisk(id) => self.replace_media_dialog(id),
                MediaRowAction::SwapExisting(old_id, new_id) => {
                    self.session.swap_media_references(old_id, new_id);
                    self.sync_after_session();
                }
                MediaRowAction::AssignBin(mid, bid) => {
                    self.session.assign_media_to_bin(mid, bid);
                    self.sync_after_session();
                }
                MediaRowAction::RemoveFromBin(mid, bid) => {
                    self.session.remove_media_from_bin(mid, bid);
                    self.sync_after_session();
                }
                MediaRowAction::AddToStoryboard(sb_id, mid) => {
                    if self.session.add_media_to_storyboard(sb_id, mid) {
                        self.session.active_storyboard_id = Some(sb_id);
                        self.sync_after_session();
                    }
                }
                MediaRowAction::CreateStoryboardWith(mid) => {
                    if let Some(sb_id) = self.session.create_storyboard("") {
                        self.session.add_media_to_storyboard(sb_id, mid);
                        self.project_media.view_mode = MediaViewMode::Storyboard;
                        self.project_media.filter_target = MediaFilterTarget::Storyboard(sb_id);
                        self.sync_after_session();
                    }
                }
                MediaRowAction::BuildProxy(id) => self.build_proxy_for_media(id),
                MediaRowAction::EditGenerated(id) => self.dlg.gen_editor = Some(id),
                MediaRowAction::OpenExternal(app, id) => {
                    self.send_to_external_app(app, None, Some(id));
                }
                MediaRowAction::RemoveFromProject(id) => {
                    self.session.remove_media_by_id(id, Some(&self.media));
                    self.thumbs.remove(&id);
                    self.project_media.invalidate_media(id);
                    self.sync_after_session();
                }
            }
        }
    }

    fn draw_storyboard_view(&mut self, ui: &mut egui::Ui) {
        let Some(sb_id) = self
            .session
            .active_storyboard_id
            .or_else(|| self.project.storyboards.first().map(|s| s.id))
        else {
            ui.horizontal(|ui| {
                ui.label("Nenhum storyboard no projeto.");
                if ui.button("Criar storyboard").clicked() {
                    if let Some(id) = self.session.create_storyboard("") {
                        self.project_media.filter_target = MediaFilterTarget::Storyboard(id);
                        self.sync_after_session();
                    }
                }
            });
            return;
        };
        self.session.active_storyboard_id = Some(sb_id);
        let Some(sb) = self
            .project
            .storyboards
            .iter()
            .find(|s| s.id == sb_id)
            .cloned()
        else {
            return;
        };

        let total_dur = self
            .session
            .storyboard_duration(sb_id, self.prefs.still_image_secs);
        ui.horizontal_wrapped(|ui| {
            if self.project_media.renaming_storyboard == Some(sb_id) {
                ui.add(
                    egui::TextEdit::singleline(&mut self.project_media.storyboard_name_buf)
                        .desired_width(140.0),
                );
                if ui.small_button("Salvar").clicked() {
                    self.session
                        .rename_storyboard(sb_id, &self.project_media.storyboard_name_buf);
                    self.project_media.renaming_storyboard = None;
                    self.sync_after_session();
                }
            } else {
                ui.label(egui::RichText::new(&sb.name).strong());
                if ui.small_button("Renomear").clicked() {
                    self.project_media.renaming_storyboard = Some(sb_id);
                    self.project_media.storyboard_name_buf = sb.name.clone();
                }
            }
            ui.label(
                egui::RichText::new(format!(
                    "{} item(ns) • {}",
                    sb.items.len(),
                    self.fmt_time(total_dur)
                ))
                .small()
                .weak(),
            );
            let active_mid = self.session.active_media_id;
            if ui
                .add_enabled(
                    active_mid.is_some(),
                    egui::Button::new("+ Adicionar selecionada"),
                )
                .clicked()
            {
                if let Some(mid) = active_mid {
                    self.session.add_media_to_storyboard(sb_id, mid);
                    self.sync_after_session();
                }
            }
            if ui
                .add_enabled(
                    !sb.items.is_empty(),
                    egui::Button::new("Inserir storyboard na timeline"),
                )
                .clicked()
            {
                let cur = self.cursor;
                self.session
                    .insert_storyboard_at_cursor(sb_id, cur, self.prefs.still_image_secs);
                self.sync_after_session();
            }
        });
        ui.separator();

        let drop_res = ui.dnd_drop_zone::<DndPayload, ()>(egui::Frame::NONE, |ui| {
            if sb.items.is_empty() {
                ui.label(
                    egui::RichText::new("Arraste mídias para cá ou clique em '+ Adicionar selecionada' para montar a sequência.")
                        .small()
                        .weak(),
                );
                return;
            }
            let ctx = ui.ctx().clone();
            let mut move_pair: Option<(usize, usize)> = None;
            let mut remove_idx: Option<usize> = None;
            let card_w = 136.0;
            let cols = ((ui.available_width() + 8.0) / (card_w + 8.0))
                .floor()
                .max(1.0) as usize;
            egui::ScrollArea::both()
                .id_salt("pm_storyboard_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("pm_storyboard_grid")
                        .spacing([8.0, 8.0])
                        .show(ui, |ui| {
                            let len = sb.items.len();
                            for (idx, it) in sb.items.iter().enumerate() {
                                let Some(m) = self.project.media(it.media_id).cloned() else {
                                    continue;
                                };
                                egui::Frame::new()
                                    .fill(self.theme.panel_fill)
                                    .stroke(Stroke::new(1.0_f32, self.theme.border))
                                    .corner_radius(3.0)
                                    .inner_margin(4.0)
                                    .show(ui, |ui| {
                                        ui.set_width(card_w - 8.0);
                                        let thumb_size = vec2(card_w - 10.0, 68.0);
                                        let (tr, ts) =
                                            ui.allocate_exact_size(thumb_size, Sense::hover());
                                        let hover_pos = ts
                                            .hovered()
                                            .then(|| ui.input(|i| i.pointer.hover_pos()))
                                            .flatten();
                                        let tex = if let Some(pos) =
                                            hover_pos.filter(|p| tr.contains(*p))
                                        {
                                            let frac = ((pos.x - tr.left())
                                                / tr.width().max(1.0))
                                            .clamp(0.0, 1.0);
                                            self.thumb_scrub(&ctx, m.id, frac).0
                                        } else {
                                            self.thumb(&ctx, m.id)
                                        };
                                        if let Some(t) = tex {
                                            ui.painter().image(
                                                t.id(),
                                                tr,
                                                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                                Color32::WHITE,
                                            );
                                        } else {
                                            ui.painter().rect_filled(tr, 2.0, self.theme.extreme_bg);
                                        }
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(format!("{}. {}", idx + 1, m.name))
                                                    .small()
                                                    .strong(),
                                            )
                                            .truncate(),
                                        );
                                        ui.horizontal(|ui| {
                                            if idx > 0 && ui.small_button("<").clicked() {
                                                move_pair = Some((idx, idx - 1));
                                            }
                                            if idx + 1 < len && ui.small_button(">").clicked() {
                                                move_pair = Some((idx, idx + 1));
                                            }
                                            if ui.small_button("X").clicked() {
                                                remove_idx = Some(idx);
                                            }
                                        });
                                    });
                                if (idx + 1) % cols == 0 {
                                    ui.end_row();
                                }
                            }
                        });
                });
            if let Some((from, to)) = move_pair {
                self.session.move_storyboard_item(sb_id, from, to);
                self.sync_after_session();
            }
            if let Some(idx) = remove_idx {
                self.session.remove_storyboard_item(sb_id, idx);
                self.sync_after_session();
            }
        });
        if let Some(payload) = drop_res.1 {
            if let DndPayload::Media(mid) = *payload {
                if self.session.add_media_to_storyboard(sb_id, mid) {
                    self.sync_after_session();
                }
            }
        }
    }
}

/// Altura reservada para as informações abaixo das mídias.
const MEDIA_FOOTER_H: f32 = 22.0;
/// Altura mínima da área de mídias: num painel mais baixo que isto, o corpo
/// do painel passa a rolar para mostrar as informações de baixo.
const MEDIA_BODY_MIN_H: f32 = 110.0;

/// Altura da área de pastas e mídias para um painel com `view_h` de altura
/// livre, deixando espaço para as informações de baixo.
fn media_body_height(view_h: f32) -> f32 {
    let h = view_h - MEDIA_FOOTER_H - 6.0;
    if h.is_finite() {
        h.max(MEDIA_BODY_MIN_H)
    } else {
        MEDIA_BODY_MIN_H
    }
}

fn media_kind_tag(m: &MediaItem) -> &'static str {
    if m.has_video && m.has_audio {
        "A/V"
    } else if m.has_video {
        if m.duration.is_none() {
            "Img"
        } else {
            "V"
        }
    } else {
        "A"
    }
}

fn media_source_label(src: &MediaSource) -> String {
    match src {
        MediaSource::File { path } => path.clone(),
        MediaSource::ImageSequence { paths, .. } => format!("Sequência ({} quadros)", paths.len()),
        MediaSource::Generated(_) => "Gerador interno".into(),
        MediaSource::NestedProject { path } => format!("Projeto aninhado: {path}"),
        MediaSource::Sequence { id } => format!("Sequência aninhada (ID: {id})"),
    }
}

/// Etiqueta flutuante exibida sob o cursor enquanto o usuário arrasta uma mídia
/// ou seleção do Trimmer (para a timeline, Storyboard ou janela Trimmer).
pub(crate) fn paint_media_drag_ghost(
    ctx: &egui::Context,
    name: &str,
    theme: &crate::theme::ThemeColors,
) {
    let Some(pos) = ctx.pointer_latest_pos() else {
        return;
    };
    let layer = egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("neoveg_media_drag_ghost"),
    );
    let painter = ctx.layer_painter(layer);
    let font = egui::FontId::proportional(11.5);
    let fg = crate::theme::contrast_on(theme.selection_bg);
    let galley = painter.layout_no_wrap(name.to_string(), font, fg);
    let pad = vec2(8.0, 4.0);
    let rect = Rect::from_min_size(pos + vec2(14.0, 10.0), galley.size() + pad * 2.0);
    painter.rect_filled(rect, 3.0, theme.selection_bg.gamma_multiply(0.95));
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0_f32, theme.accent),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.min + pad, galley, fg);
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
}

/// Fundo quadriculado de transparência (alfa) para miniaturas de imagens
/// estáticas e geradores na Mídia do Projeto (manual VEGAS Pro 21, pp. 52 e 56).
fn paint_alpha_checkerboard(painter: &egui::Painter, rect: Rect) {
    if !rect.is_finite() || rect.width() < 4.0 || rect.height() < 4.0 {
        return;
    }
    let light = Color32::from_rgb(138, 140, 146);
    let dark = Color32::from_rgb(94, 96, 102);
    painter.rect_filled(rect, 1.0, dark);
    let cell = 8.0_f32;
    let cols = ((rect.width() / cell).ceil() as usize).min(64);
    let rows = ((rect.height() / cell).ceil() as usize).min(64);
    for r in 0..rows {
        let y0 = rect.top() + r as f32 * cell;
        let y1 = (y0 + cell).min(rect.bottom());
        for c in 0..cols {
            if (r + c) % 2 == 0 {
                let x0 = rect.left() + c as f32 * cell;
                let x1 = (x0 + cell).min(rect.right());
                painter.rect_filled(Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)), 0.0, light);
            }
        }
    }
}

/// Quantas colunas de cartões de largura `card_w` cabem em `view_w` (com 8 px
/// entre eles), no mínimo uma.
fn thumb_grid_columns(view_w: f32, card_w: f32) -> usize {
    if !view_w.is_finite() || !card_w.is_finite() || card_w <= 0.0 {
        return 1;
    }
    ((view_w + 8.0) / (card_w + 8.0)).floor().clamp(1.0, 256.0) as usize
}

#[cfg(test)]
mod media_body_tests {
    use super::{media_body_height, MEDIA_BODY_MIN_H, MEDIA_FOOTER_H};

    /// Painel alto: a área de mídias e as informações cabem juntas, sem rolar.
    #[test]
    fn painel_alto_cabe_sem_rolar() {
        let h = media_body_height(500.0);
        assert!(h + MEDIA_FOOTER_H + 6.0 <= 500.0 + 0.01);
    }

    /// Painel baixo: a área de mídias mantém a altura mínima e o conteúdo
    /// passa da altura do painel, por isso o corpo do painel tem de rolar
    /// para as informações de baixo aparecerem.
    #[test]
    fn painel_baixo_mantem_minimo_e_exige_rolagem() {
        let view = 80.0;
        let h = media_body_height(view);
        assert_eq!(h, MEDIA_BODY_MIN_H);
        assert!(h + MEDIA_FOOTER_H > view);
        assert_eq!(media_body_height(f32::NAN), MEDIA_BODY_MIN_H);
        assert_eq!(media_body_height(f32::INFINITY), MEDIA_BODY_MIN_H);
    }
}

#[cfg(test)]
mod thumb_grid_tests {
    use super::thumb_grid_columns;

    #[test]
    fn colunas_cabem_na_largura_visivel() {
        // painel de 550 px com cartões reais de 260 px: duas colunas, não três
        assert_eq!(thumb_grid_columns(550.0, 260.0), 2);
        assert_eq!(thumb_grid_columns(100.0, 260.0), 1);
        assert_eq!(thumb_grid_columns(f32::INFINITY, 168.0), 1);
        assert_eq!(thumb_grid_columns(f32::NAN, 168.0), 1);
        assert_eq!(thumb_grid_columns(800.0, 0.0), 1);
        assert_eq!(thumb_grid_columns(1e9, 1.0), 256);
    }
}

/// Ação do duplo clique numa mídia: abrir no Trimmer (com a preferência
/// *Duplo clique carrega no Trimmer*) ou adicionar no cursor.
fn double_click_action(loads_trimmer: bool, id: Id) -> MediaRowAction {
    if loads_trimmer {
        MediaRowAction::OpenTrimmer(id)
    } else {
        MediaRowAction::AddAtCursor(id)
    }
}

#[cfg(test)]
mod double_click_tests {
    use super::{double_click_action, MediaRowAction};

    /// O duplo clique sempre criava eventos, sem a opção do VEGAS Pro de
    /// carregar no Trimmer.
    #[test]
    fn duplo_clique_respeita_a_preferencia() {
        assert!(matches!(
            double_click_action(true, 7),
            MediaRowAction::OpenTrimmer(7)
        ));
        assert!(matches!(
            double_click_action(false, 7),
            MediaRowAction::AddAtCursor(7)
        ));
    }
}
