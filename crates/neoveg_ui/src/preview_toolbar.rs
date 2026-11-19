//! Barra de ferramentas da Prévia de Vídeo.
//!
//! Fica numa linha só, como a barra da Prévia do VEGAS Pro: as ferramentas que
//! não cabem na largura do painel vão para o menu "⋯" no fim da barra, em vez
//! de quebrar a barra em duas linhas e roubar altura da imagem. A largura de
//! cada ferramenta é medida quando ela aparece na barra e lembrada para os
//! quadros seguintes, então a divisão acompanha idioma, escala e tema.

use super::{App, DockTab, FxTarget, PreviewQuality};
use crate::icons::{self, Icon};
use eframe::egui;
use neoveg_common::tr;
use neoveg_engine::model::Stereo3dMode;
use neoveg_engine::playback::PreviewSplitMode;

/// Ferramentas da barra, na ordem em que aparecem. As dez primeiras seguem a
/// barra do VEGAS Pro (propriedades, monitor externo, fx, tela dividida,
/// qualidade, zoom, sobreposições, copiar e salvar quadro); as demais são do
/// Neoveg e são as primeiras a ir para o menu "⋯" quando falta espaço.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PreviewTool {
    ProjectProps,
    ExternalMonitor,
    OutputFx,
    Split,
    SplitMode,
    Quality,
    Zoom,
    Overlays,
    CopyFrame,
    SaveFrame,
    ColorGrading,
    RamPreview,
    MuteAudio,
    MuteVideo,
    Multicam,
    Stereo3d,
    PopOut,
    ExpandedEdit,
    /// No fim da barra: aparece quando os quadros em RAM chegam e não pode
    /// empurrar os botões fixos.
    ClearRam,
}

impl PreviewTool {
    pub(crate) const ALL: [PreviewTool; 19] = [
        Self::ProjectProps,
        Self::ExternalMonitor,
        Self::OutputFx,
        Self::Split,
        Self::SplitMode,
        Self::Quality,
        Self::Zoom,
        Self::Overlays,
        Self::CopyFrame,
        Self::SaveFrame,
        Self::ColorGrading,
        Self::RamPreview,
        Self::MuteAudio,
        Self::MuteVideo,
        Self::Multicam,
        Self::Stereo3d,
        Self::PopOut,
        Self::ExpandedEdit,
        Self::ClearRam,
    ];

    /// Largura usada antes da primeira medição.
    fn default_width(self) -> f32 {
        match self {
            Self::Quality => 110.0,
            Self::Zoom => 64.0,
            Self::Overlays => 48.0,
            Self::ClearRam => 72.0,
            Self::ColorGrading | Self::Multicam | Self::Stereo3d | Self::ExpandedEdit => 34.0,
            _ => 26.0,
        }
    }
}

/// Largura reservada para o botão "⋯".
const MORE_BUTTON_W: f32 = 24.0;

/// Quantas ferramentas cabem na barra. `widths` são as larguras na ordem da
/// barra; `gap` é o espaço entre itens. Se nem todas cabem, reserva o botão
/// "⋯" no fim. Larguras inválidas contam como zero.
pub(crate) fn preview_tools_that_fit(widths: &[f32], avail: f32, gap: f32) -> usize {
    let w = |x: f32| if x.is_finite() && x > 0.0 { x } else { 0.0 };
    let total: f32 = widths.iter().map(|x| w(*x) + gap).sum();
    if total <= avail {
        return widths.len();
    }
    let room = avail - MORE_BUTTON_W - gap;
    let mut used = 0.0;
    let mut n = 0;
    for x in widths {
        let next = used + w(*x) + gap;
        if next > room {
            break;
        }
        used = next;
        n += 1;
    }
    n
}

impl App {
    fn preview_tool_visible(&self, tool: PreviewTool, is_floating: bool) -> bool {
        match tool {
            PreviewTool::PopOut => !is_floating,
            PreviewTool::ExpandedEdit => self.session.expanded_edit.is_some(),
            PreviewTool::ClearRam => !self.ram_preview.frames.is_empty(),
            _ => true,
        }
    }

    /// Barra da Prévia de Vídeo numa linha, com o excedente no menu "⋯".
    pub(crate) fn preview_toolbar(&mut self, ui: &mut egui::Ui, is_floating: bool) {
        let lang = self.lang();
        let tools: Vec<PreviewTool> = PreviewTool::ALL
            .into_iter()
            .filter(|t| self.preview_tool_visible(*t, is_floating))
            .collect();
        let width_id = |t: PreviewTool| egui::Id::new(("preview_tool_w", is_floating, t as u8));
        ui.horizontal(|ui| {
            let gap = ui.spacing().item_spacing.x;
            let widths: Vec<f32> = tools
                .iter()
                .map(|t| {
                    ui.ctx()
                        .data(|d| d.get_temp::<f32>(width_id(*t)))
                        .unwrap_or_else(|| t.default_width())
                })
                .collect();
            let inline = preview_tools_that_fit(&widths, ui.available_width(), gap);
            let (shown, rest) = tools.split_at(inline.min(tools.len()));
            for tool in shown {
                let rect = ui
                    .scope(|ui| self.preview_tool_inline(ui, *tool, is_floating))
                    .response
                    .rect;
                ui.ctx()
                    .data_mut(|d| d.insert_temp(width_id(*tool), rect.width()));
            }
            if !rest.is_empty() {
                let more = ui.menu_button("⋯", |ui| {
                    for tool in rest {
                        self.preview_tool_menu(ui, *tool);
                    }
                });
                let label = tr(lang, "Mais ferramentas da prévia");
                more.response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label)
                });
                more.response.on_hover_text(label);
            }
        });
    }

    /// A ferramenta como botão da barra.
    fn preview_tool_inline(&mut self, ui: &mut egui::Ui, tool: PreviewTool, is_floating: bool) {
        let lang = self.lang();
        match tool {
            PreviewTool::ProjectProps => {
                self.cmd_tool_btn(ui, Icon::GearProps, None, "file.project_props");
            }
            PreviewTool::ExternalMonitor => {
                let resp = icons::small_icon_button(
                    ui,
                    Icon::ExternalMonitor,
                    None,
                    self.external_monitor,
                    &self.theme,
                );
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Monitor externo")
                });
                if resp
                    .on_hover_text("Abrir ou fechar Monitor Externo (Alt+Shift+4)")
                    .clicked()
                {
                    self.exec_cmd(Some(ui.ctx()), "preview.external_monitor");
                }
            }
            PreviewTool::OutputFx => {
                if ui
                    .button("fx")
                    .on_hover_text(tr(lang, "Efeitos de saída de vídeo"))
                    .clicked()
                {
                    self.dlg.fx_target = Some(FxTarget::MasterVideo);
                }
            }
            PreviewTool::Split => {
                let active = self.opts.preview_split_mode != PreviewSplitMode::Off;
                let resp =
                    icons::icon_button(ui, Icon::SplitPreview, None, active, true, &self.theme);
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Tela dividida")
                });
                if resp
                    .on_hover_text("Alternar Prévia em tela dividida (Antes/Depois)")
                    .clicked()
                {
                    self.exec_cmd(Some(ui.ctx()), "preview.split_toggle");
                }
            }
            PreviewTool::SplitMode => {
                ui.menu_button("⏷", |ui| self.preview_split_menu(ui))
                    .response
                    .on_hover_text(tr(lang, "Divisão"));
            }
            PreviewTool::Quality => {
                egui::ComboBox::from_id_salt(("pq", is_floating))
                    .selected_text(self.opts.preview_quality.name_in(lang))
                    .show_ui(ui, |ui| {
                        for q in PreviewQuality::ALL {
                            ui.selectable_value(&mut self.opts.preview_quality, q, q.name_in(lang));
                        }
                    });
            }
            PreviewTool::Zoom => {
                let zoom_txt = if (self.preview_zoom - 1.0).abs() < 0.01 {
                    "Ajustar".to_string()
                } else {
                    format!("{:.0}%", self.preview_zoom * 100.0)
                };
                ui.menu_button(zoom_txt, |ui| self.preview_zoom_menu(ui));
            }
            PreviewTool::Overlays => {
                let resp = icons::icon_button(
                    ui,
                    Icon::SafeAreas,
                    None,
                    self.opts.safe_areas,
                    true,
                    &self.theme,
                )
                .on_hover_text(tr(
                    lang,
                    "Sobreposições: áreas de segurança, grade e canais (seta ao lado ou botão direito: escolher)",
                ));
                if resp.clicked() {
                    self.opts.safe_areas = !self.opts.safe_areas;
                }
                resp.context_menu(|ui| self.safe_guides_menu(ui));
                ui.menu_button("⏷", |ui| self.safe_guides_menu(ui))
                    .response
                    .on_hover_text(tr(lang, "Escolher as sobreposições"));
            }
            PreviewTool::CopyFrame => {
                let resp = icons::icon_button(ui, Icon::CopyFrame, None, false, true, &self.theme);
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Copiar quadro")
                });
                if resp
                    .on_hover_text("Copiar quadro atual para a área de transferência")
                    .clicked()
                {
                    self.copy_preview_frame_to_clipboard(Some(ui.ctx()));
                }
            }
            PreviewTool::SaveFrame => {
                if icons::icon_button(ui, Icon::CameraSnap, None, false, true, &self.theme)
                    .on_hover_text(tr(lang, "Salvar quadro em PNG"))
                    .clicked()
                {
                    self.snapshot_dialog();
                }
            }
            PreviewTool::ColorGrading => {
                if icons::small_icon_button(
                    ui,
                    Icon::ColorWheels,
                    Some(tr(lang, "Cor")),
                    self.is_panel_open(DockTab::ColorGrading),
                    &self.theme,
                )
                .on_hover_text(tr(lang, "Abrir painel Color Grading"))
                .clicked()
                {
                    self.exec_cmd(Some(ui.ctx()), "view.tab.color_grading");
                }
            }
            PreviewTool::RamPreview => {
                let busy = self.ram_preview_job.is_some();
                let resp = icons::icon_button(
                    ui,
                    Icon::RamCache,
                    None,
                    busy || !self.ram_preview.frames.is_empty(),
                    true,
                    &self.theme,
                );
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Prévia em RAM")
                });
                if resp
                    .on_hover_text("Prévia dinâmica em RAM na seleção de tempo (Shift+B)")
                    .clicked()
                {
                    self.start_ram_preview();
                }
            }
            PreviewTool::MuteAudio => {
                let resp = icons::icon_button(
                    ui,
                    Icon::MuteAudio,
                    None,
                    self.opts.preview_mute_audio,
                    true,
                    &self.theme,
                );
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        "Silenciar áudio da prévia",
                    )
                });
                if resp
                    .on_hover_text("Silenciar todo o áudio na monitoração da prévia")
                    .clicked()
                {
                    self.exec_cmd(Some(ui.ctx()), "preview.mute_audio");
                }
            }
            PreviewTool::MuteVideo => {
                let resp = icons::icon_button(
                    ui,
                    Icon::MuteVideo,
                    None,
                    self.opts.preview_mute_video,
                    true,
                    &self.theme,
                );
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        "Silenciar vídeo da prévia",
                    )
                });
                if resp
                    .on_hover_text("Silenciar todo o vídeo na prévia")
                    .clicked()
                {
                    self.exec_cmd(Some(ui.ctx()), "preview.mute_video");
                }
            }
            PreviewTool::Multicam => {
                let on = self.opts.multicam_mode;
                let resp = icons::small_icon_button(
                    ui,
                    Icon::Multicam,
                    if on { Some("MC") } else { None },
                    on,
                    &self.theme,
                );
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Edição Multicâmera")
                });
                if resp
                    .on_hover_text("Alternar modo de edição multicâmera (Ctrl+Shift+D)")
                    .clicked()
                {
                    self.exec_cmd(Some(ui.ctx()), "tools.multicam_mode");
                }
            }
            PreviewTool::Stereo3d => {
                let mode = self.project.settings.stereo_3d;
                let on = mode != Stereo3dMode::Off;
                let resp = icons::small_icon_button(
                    ui,
                    Icon::Stereo3d,
                    if on { Some(mode.short_label()) } else { None },
                    on,
                    &self.theme,
                );
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        "Modo 3D Estereoscópico",
                    )
                });
                if resp
                    .on_hover_text("Alternar modo de prévia 3D Estereoscópico")
                    .clicked()
                {
                    self.exec_cmd(Some(ui.ctx()), "view.stereo_3d");
                }
            }
            PreviewTool::PopOut => {
                if icons::small_icon_button(ui, Icon::PopOut, None, false, &self.theme)
                    .on_hover_text(tr(
                        lang,
                        "Destacar Prévia de Vídeo em janela normal do Windows",
                    ))
                    .clicked()
                {
                    self.preview_detached = true;
                }
            }
            PreviewTool::ExpandedEdit => {
                if icons::small_icon_button(ui, Icon::ExpandedEdit, Some("A/B"), true, &self.theme)
                    .on_hover_text("Modo de edição expandido ativo (clique para fechar)")
                    .clicked()
                {
                    self.exec_cmd(Some(ui.ctx()), "edit.expanded_edit_mode");
                }
            }
            PreviewTool::ClearRam => {
                if ui.small_button("Limpar RAM").clicked() {
                    self.clear_ram_preview();
                }
            }
        }
    }

    /// A ferramenta como item do menu "⋯". Os itens têm o mesmo nome
    /// acessível do botão da barra.
    fn preview_tool_menu(&mut self, ui: &mut egui::Ui, tool: PreviewTool) {
        let lang = self.lang();
        let toggle = |ui: &mut egui::Ui, on: bool, label: &str| {
            ui.add(egui::Button::new(label).selected(on)).clicked()
        };
        match tool {
            PreviewTool::ProjectProps => self.cmd_menu_btn(ui, "file.project_props"),
            PreviewTool::ExternalMonitor => {
                if toggle(ui, self.external_monitor, "Monitor externo") {
                    self.exec_cmd(Some(ui.ctx()), "preview.external_monitor");
                    ui.close_menu();
                }
            }
            PreviewTool::OutputFx => {
                if ui.button(tr(lang, "Efeitos de saída de vídeo")).clicked() {
                    self.dlg.fx_target = Some(FxTarget::MasterVideo);
                    ui.close_menu();
                }
            }
            PreviewTool::Split => {
                let on = self.opts.preview_split_mode != PreviewSplitMode::Off;
                if toggle(ui, on, "Tela dividida") {
                    self.exec_cmd(Some(ui.ctx()), "preview.split_toggle");
                    ui.close_menu();
                }
            }
            PreviewTool::SplitMode => {
                ui.menu_button(tr(lang, "Divisão"), |ui| self.preview_split_menu(ui));
            }
            PreviewTool::Quality => {
                ui.menu_button(tr(lang, "Qualidade da prévia"), |ui| {
                    for q in PreviewQuality::ALL {
                        if ui
                            .selectable_value(&mut self.opts.preview_quality, q, q.name_in(lang))
                            .clicked()
                        {
                            ui.close_menu();
                        }
                    }
                });
            }
            PreviewTool::Zoom => {
                ui.menu_button(tr(lang, "Zoom"), |ui| self.preview_zoom_menu(ui));
            }
            PreviewTool::Overlays => {
                if toggle(ui, self.opts.safe_areas, tr(lang, "Sobreposições")) {
                    self.opts.safe_areas = !self.opts.safe_areas;
                    ui.close_menu();
                }
                ui.menu_button(tr(lang, "Escolher as sobreposições"), |ui| {
                    self.safe_guides_menu(ui);
                });
            }
            PreviewTool::CopyFrame => {
                if ui.button("Copiar quadro").clicked() {
                    self.copy_preview_frame_to_clipboard(Some(ui.ctx()));
                    ui.close_menu();
                }
            }
            PreviewTool::SaveFrame => {
                if ui.button(tr(lang, "Salvar quadro em PNG")).clicked() {
                    self.snapshot_dialog();
                    ui.close_menu();
                }
            }
            PreviewTool::ColorGrading => {
                let on = self.is_panel_open(DockTab::ColorGrading);
                if toggle(ui, on, tr(lang, "Abrir painel Color Grading")) {
                    self.exec_cmd(Some(ui.ctx()), "view.tab.color_grading");
                    ui.close_menu();
                }
            }
            PreviewTool::RamPreview => {
                let on = self.ram_preview_job.is_some() || !self.ram_preview.frames.is_empty();
                if toggle(ui, on, "Prévia em RAM") {
                    self.start_ram_preview();
                    ui.close_menu();
                }
            }
            PreviewTool::MuteAudio => {
                if toggle(
                    ui,
                    self.opts.preview_mute_audio,
                    "Silenciar áudio da prévia",
                ) {
                    self.exec_cmd(Some(ui.ctx()), "preview.mute_audio");
                    ui.close_menu();
                }
            }
            PreviewTool::MuteVideo => {
                if toggle(
                    ui,
                    self.opts.preview_mute_video,
                    "Silenciar vídeo da prévia",
                ) {
                    self.exec_cmd(Some(ui.ctx()), "preview.mute_video");
                    ui.close_menu();
                }
            }
            PreviewTool::Multicam => {
                if toggle(ui, self.opts.multicam_mode, "Edição Multicâmera") {
                    self.exec_cmd(Some(ui.ctx()), "tools.multicam_mode");
                    ui.close_menu();
                }
            }
            PreviewTool::Stereo3d => {
                let on = self.project.settings.stereo_3d != Stereo3dMode::Off;
                if toggle(ui, on, "Modo 3D Estereoscópico") {
                    self.exec_cmd(Some(ui.ctx()), "view.stereo_3d");
                    ui.close_menu();
                }
            }
            PreviewTool::PopOut => {
                if ui
                    .button(tr(lang, "Prévia de Vídeo em janela separada"))
                    .clicked()
                {
                    self.preview_detached = true;
                    ui.close_menu();
                }
            }
            PreviewTool::ExpandedEdit => {
                if toggle(ui, true, "A/B") {
                    self.exec_cmd(Some(ui.ctx()), "edit.expanded_edit_mode");
                    ui.close_menu();
                }
            }
            PreviewTool::ClearRam => {
                if ui.button("Limpar RAM").clicked() {
                    self.clear_ram_preview();
                    ui.close_menu();
                }
            }
        }
    }

    /// Modos da tela dividida (seta ao lado do botão Tela dividida).
    fn preview_split_menu(&mut self, ui: &mut egui::Ui) {
        for m in PreviewSplitMode::ALL {
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
    }

    /// Menu do zoom da Prévia.
    fn preview_zoom_menu(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
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
        ui.separator();
        if ui.button(tr(lang, "Zoom +5%")).clicked() {
            self.exec_cmd(Some(ui.ctx()), "preview.zoom_step_in");
        }
        if ui.button(tr(lang, "Zoom -5%")).clicked() {
            self.exec_cmd(Some(ui.ctx()), "preview.zoom_step_out");
        }
        if ui
            .selectable_label(self.preview_zoom_bypass.is_some(), tr(lang, "Ignorar zoom"))
            .on_hover_text(tr(lang, "Alterna entre este zoom e o ajuste à janela"))
            .clicked()
        {
            self.exec_cmd(Some(ui.ctx()), "preview.zoom_bypass");
            ui.close_menu();
        }
    }
}
