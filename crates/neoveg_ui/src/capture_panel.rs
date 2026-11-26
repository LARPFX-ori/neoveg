//! Interface do **Neoveg Capture** (`neoveg_ui`), modelada sobre os manuais do
//! *VEGAS Capture* (VEGAS Pro 21–26) e integrada nativamente aos 7 temas do
//! Neoveg (incluindo *Alto Contraste AAA*) — `AGENTS.md` §0, §1, §2.2 e §3.3.
//!
//! Abre em janela própria (`window_only`, como o *Neoveg Encoder*) a partir de:
//! - *Arquivo → Neoveg Capture...* (`file.capture`)
//! - *Exibir → Neoveg Capture* (`view.tab.capture`)
//! - Barra de ferramentas de *Mídia do Projeto* (*Capturar...*)
//! - *Ferramentas → Neoveg Capture*

use crate::app::{rgba_to_color_image, App, DockTab};
use crate::icons::{self, Icon};
use crate::theme::ThemeColors;
use eframe::egui;
use egui::{pos2, vec2, Color32, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle};
use neoveg_common::time::{format_ticks, TimeFormat};
use neoveg_common::{tr, tr_args, Language};
use neoveg_engine::capture::{
    apply_manifest_to_session, discover_sources, estimate_audio_10min_bytes,
    estimate_video_10min_bytes, format_bytes_human, free_disk_bytes, load_manifest, save_manifest,
    CaptureAudioCodec, CaptureQuality, CaptureResolution, CaptureRuntime, CaptureSessionResult,
    CaptureSettings, CaptureSourceCard, CaptureSourceKind, CaptureVideoCodec, DiscoveredSources,
    SourceRuntimeStatus, CAPTURE_FPS_OPTIONS, CAPTURE_SAMPLE_RATES, CAPTURE_VERSION,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CapturePickerTab {
    #[default]
    Screens,
    Windows,
    Cameras,
    Audio,
}

impl CapturePickerTab {
    const ALL: [Self; 4] = [Self::Screens, Self::Windows, Self::Cameras, Self::Audio];

    fn label_in(self, lang: Language) -> &'static str {
        let pt = match self {
            Self::Screens => "Telas",
            Self::Windows => "Janelas",
            Self::Cameras => "Câmeras",
            Self::Audio => "Áudio",
        };
        tr(lang, pt)
    }
}

/// Estado completo da janela **Neoveg Capture**.
pub struct CapturePanelState {
    pub settings: CaptureSettings,
    pub sources: Vec<CaptureSourceCard>,
    next_source_id: u64,
    pub runtime: CaptureRuntime,
    pub discovered: Option<DiscoveredSources>,
    pub picker_open: bool,
    pub picker_tab: CapturePickerTab,
    pub settings_open: bool,
    /// Cartão atualmente com a gaveta *Configurações do dispositivo* expandida.
    pub expanded_card_id: Option<u64>,
    preview_textures: HashMap<u64, (u64, TextureHandle)>,
    pub last_session: Option<CaptureSessionResult>,
    /// Contagem regressiva ativa antes de disparar `runtime.start_recording`.
    countdown_until: Option<Instant>,
    countdown_dir: Option<PathBuf>,
    last_disk_probe: Option<Instant>,
    free_disk_bytes: Option<u64>,
    pub status_note: Option<(String, bool)>,
}

impl CapturePanelState {
    pub fn load_from_disk() -> Self {
        Self {
            settings: CaptureSettings::load_from_disk(),
            sources: Vec::new(),
            next_source_id: 1,
            runtime: CaptureRuntime::new(),
            discovered: None,
            picker_open: false,
            picker_tab: CapturePickerTab::Screens,
            settings_open: false,
            expanded_card_id: None,
            preview_textures: HashMap::new(),
            last_session: None,
            countdown_until: None,
            countdown_dir: None,
            last_disk_probe: None,
            free_disk_bytes: None,
            status_note: None,
        }
    }

    pub fn is_recording(&self) -> bool {
        self.runtime.is_recording()
    }

    pub fn is_counting_down(&self) -> bool {
        self.countdown_until.is_some()
    }

    pub fn refresh_sources(&mut self) {
        self.discovered = Some(discover_sources());
    }

    pub fn add_source(&mut self, name: String, kind: CaptureSourceKind) -> u64 {
        let key = kind.unique_key();
        if let Some(existing) = self.sources.iter().find(|s| s.kind.unique_key() == key) {
            return existing.id;
        }
        let id = self.next_source_id;
        self.next_source_id = self.next_source_id.saturating_add(1);
        let card = CaptureSourceCard::new(id, name, kind, &self.settings);
        self.sources.push(card);
        id
    }

    pub fn remove_source(&mut self, id: u64) {
        self.sources.retain(|s| s.id != id);
        self.preview_textures.remove(&id);
        if self.expanded_card_id == Some(id) {
            self.expanded_card_id = None;
        }
    }

    pub fn has_source_key(&self, key: &str) -> bool {
        self.sources.iter().any(|s| s.kind.unique_key() == key)
    }

    pub fn add_default_sources(&mut self) -> usize {
        if self.discovered.is_none() {
            self.refresh_sources();
        }
        let mut added = 0;
        let Some(disc) = self.discovered.clone() else {
            return 0;
        };
        if let Some(mon) = disc
            .monitors
            .iter()
            .find(|m| m.is_primary)
            .or_else(|| disc.monitors.first())
        {
            let kind = CaptureSourceKind::Screen {
                monitor_id: mon.id.clone(),
                x: mon.x,
                y: mon.y,
                width: mon.width,
                height: mon.height,
            };
            if !self.has_source_key(&kind.unique_key()) {
                self.add_source(mon.name.clone(), kind);
                added += 1;
            }
        }
        if let Some(mic) = disc
            .audio_inputs
            .iter()
            .find(|d| d.is_default)
            .or_else(|| disc.audio_inputs.first())
        {
            let kind = CaptureSourceKind::AudioInput {
                device_name: mic.name.clone(),
            };
            if !self.has_source_key(&kind.unique_key()) {
                self.add_source(mic.name.clone(), kind);
                added += 1;
            }
        }
        added
    }

    pub fn shutdown(&mut self) {
        self.countdown_until = None;
        self.countdown_dir = None;
        let _ = self.settings.save_to_disk();
        self.runtime.shutdown();
    }

    fn refresh_free_disk(&mut self, dir: &std::path::Path, force: bool) {
        let should_probe = force
            || self
                .last_disk_probe
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(5));
        if should_probe {
            self.last_disk_probe = Some(Instant::now());
            self.free_disk_bytes = free_disk_bytes(dir);
        }
    }

    fn set_note(&mut self, msg: impl Into<String>, is_error: bool) {
        self.status_note = Some((msg.into(), is_error));
    }
}

impl App {
    /// Abre a janela própria do **Neoveg Capture** e a traz para a frente.
    pub(crate) fn open_capture_window(&mut self, ctx: Option<&egui::Context>) {
        self.open_window_panel(ctx, DockTab::Capture);
        if self.capture.discovered.is_none() {
            self.capture.refresh_sources();
        }
    }

    /// Atualiza contagem regressiva, monitoramento ao vivo e pedido de redesenho
    /// quando o **Neoveg Capture** está aberto ou gravando.
    pub(crate) fn tick_capture(&mut self, ctx: &egui::Context) {
        let window_open = self.detached_tabs.contains(&DockTab::Capture);
        if let Some(until) = self.capture.countdown_until {
            if Instant::now() >= until {
                self.capture.countdown_until = None;
                let dir = self.capture.countdown_dir.take().unwrap_or_else(|| {
                    self.capture
                        .settings
                        .effective_recording_dir(&self.project.work_folders, self.path.as_deref())
                });
                self.begin_capture_recording_now(&dir);
            } else {
                ctx.request_repaint_after(Duration::from_millis(80));
            }
        }
        self.capture
            .runtime
            .sync_monitors(&self.capture.sources, window_open);
        if self.capture.is_recording() {
            ctx.request_repaint_after(Duration::from_millis(50));
        } else if window_open && !self.capture.sources.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
    }

    fn begin_capture_recording_now(&mut self, dir: &std::path::Path) {
        let lang = self.lang();
        match self
            .capture
            .runtime
            .start_recording(&self.capture.sources, dir)
        {
            Ok(session_dir) => {
                let msg = tr_args(
                    lang,
                    "Gravação iniciada em {path}",
                    &[("path", &session_dir.display().to_string())],
                );
                self.capture.set_note(msg.clone(), false);
                self.set_status(msg);
            }
            Err(e) => {
                let msg = format!("{e:#}");
                self.capture.set_note(msg.clone(), true);
                self.set_status(msg);
            }
        }
    }

    /// Alterna gravação do **Neoveg Capture** (inicia com ou sem contagem regressiva,
    /// ou finaliza a gravação ativa gerando o manifesto da sessão).
    pub(crate) fn capture_toggle_record(&mut self) {
        let lang = self.lang();
        if self.capture.is_counting_down() {
            self.capture.countdown_until = None;
            self.capture.countdown_dir = None;
            let msg = tr(lang, "Contagem regressiva cancelada.");
            self.capture.set_note(msg, false);
            self.set_status(msg);
            return;
        }
        if self.capture.is_recording() {
            match self.capture.runtime.stop_recording() {
                Ok(result) => {
                    let count_s = result.manifest.tracks.len().to_string();
                    let note = if result.warnings.is_empty() {
                        tr_args(
                            lang,
                            "Captura concluída: {n} faixa(s) gravada(s).",
                            &[("n", &count_s)],
                        )
                    } else {
                        let warn = result.warnings.join("; ");
                        tr_args(
                            lang,
                            "Captura concluída ({n} faixa(s)) com avisos: {warn}",
                            &[("n", &count_s), ("warn", &warn)],
                        )
                    };
                    let auto_import = self.capture.settings.auto_import_on_stop;
                    self.capture.last_session = Some(result);
                    self.capture.set_note(note.clone(), false);
                    self.set_status(note);
                    if auto_import {
                        self.capture_export_last_session();
                    }
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    self.capture.set_note(msg.clone(), true);
                    self.set_status(msg);
                }
            }
            return;
        }

        let dir = self
            .capture
            .settings
            .effective_recording_dir(&self.project.work_folders, self.path.as_deref());
        let has_active = self
            .capture
            .sources
            .iter()
            .any(|s| s.enabled && !(s.is_audio() && s.muted));
        if !has_active {
            let msg = tr(lang, "Adicione e ative ao menos uma fonte antes de gravar.");
            self.capture.set_note(msg, true);
            self.set_status(msg);
            return;
        }
        let cd = self.capture.settings.countdown_secs;
        if cd > 0 {
            self.capture.countdown_until =
                Some(Instant::now() + Duration::from_secs(u64::from(cd)));
            self.capture.countdown_dir = Some(dir);
            let cd_s = cd.to_string();
            let msg = tr_args(lang, "Iniciando gravação em {s} s...", &[("s", &cd_s)]);
            self.capture.set_note(msg.clone(), false);
            self.set_status(msg);
        } else {
            self.begin_capture_recording_now(&dir);
        }
    }

    /// Importa a última sessão gravada do **Neoveg Capture** diretamente em
    /// trilhas dedicadas na linha do tempo do projeto ativo (espelho de
    /// *Export to VEGAS* no manual do VEGAS Pro 23).
    pub(crate) fn capture_export_last_session(&mut self) {
        let lang = self.lang();
        let Some(sess) = self.capture.last_session.clone() else {
            let msg = tr(lang, "Nenhuma gravação concluída para exportar.");
            self.capture.set_note(msg, true);
            self.set_status(msg);
            return;
        };
        let place_at = self.cursor;
        match apply_manifest_to_session(
            &mut self.session,
            &self.media,
            &sess.manifest,
            &sess.session_dir,
            place_at,
        ) {
            Ok(report) => {
                self.sync_after_session();
                let n_s = report.placed_event_ids.len().to_string();
                let msg = tr_args(
                    lang,
                    "Sessão do Neoveg Capture importada em {n} trilha(s) na timeline.",
                    &[("n", &n_s)],
                );
                self.capture.set_note(msg.clone(), false);
                self.set_status(msg);
            }
            Err(e) => {
                let msg = format!("{e:#}");
                self.capture.set_note(msg.clone(), true);
                self.set_status(msg);
            }
        }
    }

    /// Salva a última sessão como arquivo de projeto `.nvcap` (equivalente ao `.vegcap`).
    pub(crate) fn capture_save_project_dialog(&mut self) {
        let lang = self.lang();
        let Some(sess) = self.capture.last_session.clone() else {
            let msg = tr(lang, "Nenhuma sessão gravada para salvar.");
            self.capture.set_note(msg, true);
            self.set_status(msg);
            return;
        };
        let mut dlg = rfd::FileDialog::new()
            .set_title(tr(lang, "Salvar projeto Neoveg Capture"))
            .set_file_name("captura.nvcap")
            .add_filter(tr(lang, "Projeto Neoveg Capture"), &["nvcap", "json"]);
        if sess.session_dir.exists() {
            dlg = dlg.set_directory(&sess.session_dir);
        }
        if let Some(path) = dlg.save_file() {
            // Se salvo fora da pasta da sessão, converte caminhos relativos para absolutos
            // para que o `.nvcap` continue abrindo de qualquer diretório.
            let mut manifest = sess.manifest.clone();
            let same_dir = path.parent() == Some(sess.session_dir.as_path());
            if !same_dir {
                for tr_entry in &mut manifest.tracks {
                    let p = std::path::Path::new(&tr_entry.file);
                    if !p.is_absolute() {
                        tr_entry.file = sess.session_dir.join(p).to_string_lossy().to_string();
                    }
                }
            }
            match save_manifest(&path, &manifest) {
                Ok(()) => {
                    let msg = tr_args(
                        lang,
                        "Projeto de captura salvo em {path}",
                        &[("path", &path.display().to_string())],
                    );
                    self.capture.set_note(msg.clone(), false);
                    self.set_status(msg);
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    self.capture.set_note(msg.clone(), true);
                    self.set_status(msg);
                }
            }
        }
    }

    /// Abre um projeto `.nvcap` ou `export.json` salvo e importa suas faixas
    /// diretamente na linha do tempo do projeto ativo.
    pub(crate) fn capture_import_project_dialog(&mut self) {
        let lang = self.lang();
        let rec_dir = self
            .capture
            .settings
            .effective_recording_dir(&self.project.work_folders, self.path.as_deref());
        let mut dlg = rfd::FileDialog::new()
            .set_title(tr(lang, "Importar projeto Neoveg Capture (.nvcap)"))
            .add_filter(tr(lang, "Projeto Neoveg Capture"), &["nvcap", "json"]);
        if rec_dir.exists() {
            dlg = dlg.set_directory(&rec_dir);
        }
        if let Some(path) = dlg.pick_file() {
            let base_dir = path
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_else(|| rec_dir.clone());
            match load_manifest(&path) {
                Ok(manifest) => {
                    let place_at = self.cursor;
                    match apply_manifest_to_session(
                        &mut self.session,
                        &self.media,
                        &manifest,
                        &base_dir,
                        place_at,
                    ) {
                        Ok(report) => {
                            self.sync_after_session();
                            self.capture.last_session = Some(CaptureSessionResult {
                                session_dir: base_dir,
                                manifest_path: path,
                                manifest,
                                warnings: report.warnings,
                            });
                            let n_s = report.placed_event_ids.len().to_string();
                            let msg = tr_args(
                                lang,
                                "Sessão do Neoveg Capture importada em {n} trilha(s) na timeline.",
                                &[("n", &n_s)],
                            );
                            self.capture.set_note(msg.clone(), false);
                            self.set_status(msg);
                        }
                        Err(e) => {
                            let msg = format!("{e:#}");
                            self.capture.set_note(msg.clone(), true);
                            self.set_status(msg);
                        }
                    }
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    self.capture.set_note(msg.clone(), true);
                    self.set_status(msg);
                }
            }
        }
    }

    /// Corpo principal da janela **Neoveg Capture**.
    pub(crate) fn capture_tab(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang();
        let theme = self.theme;
        let rec_dir = self
            .capture
            .settings
            .effective_recording_dir(&self.project.work_folders, self.path.as_deref());
        self.capture.refresh_free_disk(&rec_dir, false);

        // Barra superior principal (espelho do VEGAS Capture 23/26)
        self.draw_capture_toolbar(ui, lang, &theme, &rec_dir);
        ui.separator();

        // Vista de configurações ou seletor de fontes ou grade de cartões
        if self.capture.settings_open {
            self.draw_capture_settings_view(ui, lang, &theme, &rec_dir);
        } else if self.capture.picker_open {
            self.draw_capture_source_picker(ui, lang, &theme);
        } else {
            self.draw_capture_dashboard(ui, lang, &theme);
        }

        // Rodapé informativo com contagem de fontes, pasta e avisos
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            let enabled_n = self.capture.sources.iter().filter(|s| s.enabled).count();
            let total_n = self.capture.sources.len();
            let active_s = enabled_n.to_string();
            let total_s = total_n.to_string();
            ui.label(
                RichText::new(tr_args(
                    lang,
                    "Fontes ativas: {active}/{total}",
                    &[("active", &active_s), ("total", &total_s)],
                ))
                .small()
                .color(theme.text_muted),
            );
            if let Some(free) = self.capture.free_disk_bytes {
                ui.separator();
                let free_s = format_bytes_human(free);
                ui.label(
                    RichText::new(tr_args(
                        lang,
                        "Espaço livre: {space}",
                        &[("space", &free_s)],
                    ))
                    .small()
                    .color(theme.text_muted),
                );
            }
            if let Some((msg, is_err)) = &self.capture.status_note {
                ui.separator();
                let col = if *is_err {
                    theme.offline_fg
                } else {
                    theme.text_primary
                };
                ui.label(RichText::new(msg).small().color(col));
            }
        });
    }

    fn draw_capture_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        theme: &ThemeColors,
        rec_dir: &std::path::Path,
    ) {
        let recording = self.capture.is_recording();
        let counting = self.capture.is_counting_down();
        let can_record = recording
            || counting
            || self
                .capture
                .sources
                .iter()
                .any(|s| s.enabled && !(s.is_audio() && s.muted));
        let has_session = self.capture.last_session.is_some();

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            let add_lbl = format!("+ {}", tr(lang, "Adicionar fontes"));
            if ui
                .add_enabled(
                    !recording && !counting,
                    egui::Button::new(RichText::new(add_lbl).strong()),
                )
                .clicked()
            {
                self.capture.settings_open = false;
                self.capture.picker_open = !self.capture.picker_open;
                if self.capture.picker_open {
                    self.capture.refresh_sources();
                }
            }

            // Botão principal Gravar / Parar + cronômetro exato
            let rec_btn = if recording {
                let elapsed = format_ticks(
                    self.capture.runtime.elapsed_ticks(),
                    self.project.settings.fps,
                    self.project.settings.sample_rate,
                    TimeFormat::Time,
                );
                let txt = format!("■ {}  {elapsed}", tr(lang, "Parar"));
                let fill = Color32::from_rgb(208, 66, 48);
                egui::Button::new(RichText::new(txt).strong().color(Color32::WHITE)).fill(fill)
            } else if let Some(until) = self.capture.countdown_until {
                let rem = until
                    .saturating_duration_since(Instant::now())
                    .as_secs()
                    .saturating_add(1);
                let txt = format!("{} ({rem} s)", tr(lang, "Cancelar"));
                egui::Button::new(RichText::new(txt).strong().color(Color32::WHITE))
                    .fill(Color32::from_rgb(186, 118, 28))
            } else {
                let txt = format!("● {}", tr(lang, "Iniciar gravação"));
                egui::Button::new(RichText::new(txt).strong())
            };
            if ui.add_enabled(can_record, rec_btn).clicked() {
                self.capture.picker_open = false;
                self.capture.settings_open = false;
                self.capture_toggle_record();
            }

            if ui
                .add_enabled(
                    has_session && !recording,
                    egui::Button::new(tr(lang, "Exportar para o Neoveg")),
                )
                .on_hover_text(tr(
                    lang,
                    "Importa os arquivos gravados em trilhas separadas na timeline atual",
                ))
                .clicked()
            {
                self.capture_export_last_session();
            }

            if ui
                .add_enabled(
                    has_session && !recording,
                    egui::Button::new(tr(lang, "Salvar .nvcap")),
                )
                .on_hover_text(tr(lang, "Salvar manifesto do projeto de captura (.nvcap)"))
                .clicked()
            {
                self.capture_save_project_dialog();
            }

            if ui
                .add_enabled(!recording, egui::Button::new(tr(lang, "Abrir .nvcap")))
                .on_hover_text(tr(lang, "Abrir e importar projeto Neoveg Capture (.nvcap)"))
                .clicked()
            {
                self.capture_import_project_dialog();
            }

            if ui
                .button(tr(lang, "Abrir pasta"))
                .on_hover_text(tr(
                    lang,
                    "Abrir pasta das gravações no gerenciador de arquivos",
                ))
                .clicked()
            {
                let target = self
                    .capture
                    .last_session
                    .as_ref()
                    .map(|s| s.session_dir.clone())
                    .unwrap_or_else(|| rec_dir.to_path_buf());
                let _ = std::fs::create_dir_all(&target);
                if let Err(e) = crate::crash_report::open_folder(&target) {
                    self.capture.set_note(e.clone(), true);
                    self.set_status(e);
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let gear = icons::icon_button(
                    ui,
                    Icon::GearProps,
                    Some(tr(lang, "Configurações")),
                    self.capture.settings_open,
                    !recording,
                    theme,
                );
                if gear.clicked() {
                    self.capture.picker_open = false;
                    self.capture.settings_open = !self.capture.settings_open;
                    if self.capture.settings_open {
                        self.capture.refresh_free_disk(rec_dir, true);
                    }
                }
            });
        });
    }

    fn draw_capture_settings_view(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        theme: &ThemeColors,
        rec_dir: &std::path::Path,
    ) {
        let mut changed = false;
        let mut browse_dir = false;
        let mut open_dir = false;
        let mut use_project_dir = false;
        let mut close_settings = false;

        egui::ScrollArea::vertical()
            .id_salt("nvcap_settings_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(tr(lang, "Neoveg Capture — Configurações"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(tr(lang, "Voltar")).clicked() {
                            close_settings = true;
                        }
                    });
                });
                ui.add_space(6.0);

                // Pasta de gravação + espaço livre em disco
                ui.label(RichText::new(tr(lang, "Pasta de gravação:")).strong());
                ui.horizontal(|ui| {
                    let placeholder = rec_dir.display().to_string();
                    let te =
                        egui::TextEdit::singleline(&mut self.capture.settings.recording_folder)
                            .hint_text(placeholder)
                            .desired_width((ui.available_width() - 220.0).max(180.0));
                    if ui.add(te).changed() {
                        changed = true;
                    }
                    if ui.button(tr(lang, "Procurar...")).clicked() {
                        browse_dir = true;
                    }
                    if ui.button(tr(lang, "Abrir")).clicked() {
                        open_dir = true;
                    }
                    if !self.capture.settings.recording_folder.is_empty()
                        && ui.button(tr(lang, "Pasta do projeto")).clicked()
                    {
                        use_project_dir = true;
                    }
                });
                let free_str = self
                    .capture
                    .free_disk_bytes
                    .map(format_bytes_human)
                    .unwrap_or_else(|| tr(lang, "Indisponível").to_string());
                ui.label(
                    RichText::new(tr_args(
                        lang,
                        "Espaço livre em disco: {space}",
                        &[("space", &free_str)],
                    ))
                    .small()
                    .color(theme.text_muted),
                );
                ui.add_space(10.0);

                // Configurações padrão de vídeo
                ui.label(
                    RichText::new(tr(lang, "Configurações padrão de vídeo"))
                        .strong()
                        .size(13.5),
                );
                egui::Grid::new("nvcap_def_vid_grid")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(tr(lang, "Codec:"));
                        egui::ComboBox::from_id_salt("nvcap_def_vcodec")
                            .selected_text(self.capture.settings.default_video_codec.label_in(lang))
                            .show_ui(ui, |ui| {
                                for c in CaptureVideoCodec::ALL {
                                    if !c.available_on_os() {
                                        continue;
                                    }
                                    if ui
                                        .selectable_value(
                                            &mut self.capture.settings.default_video_codec,
                                            c,
                                            c.label_in(lang),
                                        )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label(tr(lang, "Qualidade:"));
                        egui::ComboBox::from_id_salt("nvcap_def_vqual")
                            .selected_text(
                                self.capture.settings.default_video_quality.label_in(lang),
                            )
                            .show_ui(ui, |ui| {
                                for q in CaptureQuality::ALL {
                                    if ui
                                        .selectable_value(
                                            &mut self.capture.settings.default_video_quality,
                                            q,
                                            q.label_in(lang),
                                        )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label(tr(lang, "Taxa de quadros padrão:"));
                        let cur_fps = CAPTURE_FPS_OPTIONS
                            .iter()
                            .find(|(_, f)| *f == self.capture.settings.default_fps)
                            .map(|(l, _)| *l)
                            .unwrap_or("30 fps");
                        egui::ComboBox::from_id_salt("nvcap_def_fps")
                            .selected_text(cur_fps)
                            .show_ui(ui, |ui| {
                                for (lbl, fps) in CAPTURE_FPS_OPTIONS {
                                    if ui
                                        .selectable_value(
                                            &mut self.capture.settings.default_fps,
                                            fps,
                                            lbl,
                                        )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label(tr(lang, "Contagem regressiva:"));
                        let cd_lbl = if self.capture.settings.countdown_secs == 0 {
                            tr(lang, "Desligada").to_string()
                        } else {
                            format!("{} s", self.capture.settings.countdown_secs)
                        };
                        egui::ComboBox::from_id_salt("nvcap_def_cd")
                            .selected_text(cd_lbl)
                            .show_ui(ui, |ui| {
                                for sec in [0u8, 3, 5] {
                                    let txt = if sec == 0 {
                                        tr(lang, "Desligada").to_string()
                                    } else {
                                        format!("{sec} s")
                                    };
                                    if ui
                                        .selectable_value(
                                            &mut self.capture.settings.countdown_secs,
                                            sec,
                                            txt,
                                        )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                }
                            });
                        ui.end_row();
                    });
                if ui
                    .checkbox(
                        &mut self.capture.settings.capture_cursor,
                        tr(lang, "Capturar cursor do mouse nas telas e janelas"),
                    )
                    .changed()
                {
                    changed = true;
                }
                let v_est = format_bytes_human(estimate_video_10min_bytes(
                    self.capture.settings.default_video_codec,
                    self.capture.settings.default_video_quality,
                ));
                ui.label(
                    RichText::new(tr_args(
                        lang,
                        "10 min Full HD a 30 fps ≈ {size}",
                        &[("size", &v_est)],
                    ))
                    .small()
                    .color(theme.text_muted),
                );
                ui.add_space(10.0);

                // Configurações padrão de áudio
                ui.label(
                    RichText::new(tr(lang, "Configurações padrão de áudio"))
                        .strong()
                        .size(13.5),
                );
                egui::Grid::new("nvcap_def_aud_grid")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(tr(lang, "Codec:"));
                        egui::ComboBox::from_id_salt("nvcap_def_acodec")
                            .selected_text(self.capture.settings.default_audio_codec.label_in(lang))
                            .show_ui(ui, |ui| {
                                for c in CaptureAudioCodec::ALL {
                                    if !c.available_on_os() {
                                        continue;
                                    }
                                    if ui
                                        .selectable_value(
                                            &mut self.capture.settings.default_audio_codec,
                                            c,
                                            c.label_in(lang),
                                        )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label(tr(lang, "Qualidade:"));
                        egui::ComboBox::from_id_salt("nvcap_def_aqual")
                            .selected_text(
                                self.capture.settings.default_audio_quality.label_in(lang),
                            )
                            .show_ui(ui, |ui| {
                                for q in CaptureQuality::ALL {
                                    if ui
                                        .selectable_value(
                                            &mut self.capture.settings.default_audio_quality,
                                            q,
                                            q.label_in(lang),
                                        )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label(tr(lang, "Canais:"));
                        let ch_lbl = if self.capture.settings.default_audio_channels <= 1 {
                            tr(lang, "Mono")
                        } else {
                            tr(lang, "Estéreo")
                        };
                        egui::ComboBox::from_id_salt("nvcap_def_ach")
                            .selected_text(ch_lbl)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_value(
                                        &mut self.capture.settings.default_audio_channels,
                                        2,
                                        tr(lang, "Estéreo"),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                                if ui
                                    .selectable_value(
                                        &mut self.capture.settings.default_audio_channels,
                                        1,
                                        tr(lang, "Mono"),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                        ui.end_row();

                        ui.label(tr(lang, "Taxa de amostragem:"));
                        egui::ComboBox::from_id_salt("nvcap_def_asr")
                            .selected_text(format!(
                                "{} Hz",
                                self.capture.settings.default_audio_sample_rate
                            ))
                            .show_ui(ui, |ui| {
                                for sr in CAPTURE_SAMPLE_RATES {
                                    if ui
                                        .selectable_value(
                                            &mut self.capture.settings.default_audio_sample_rate,
                                            sr,
                                            format!("{sr} Hz"),
                                        )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                }
                            });
                        ui.end_row();
                    });
                let a_est = format_bytes_human(estimate_audio_10min_bytes(
                    self.capture.settings.default_audio_codec,
                    self.capture.settings.default_audio_quality,
                    self.capture.settings.default_audio_channels,
                    self.capture.settings.default_audio_sample_rate,
                ));
                ui.label(
                    RichText::new(tr_args(
                        lang,
                        "10 min de áudio ≈ {size}",
                        &[("size", &a_est)],
                    ))
                    .small()
                    .color(theme.text_muted),
                );
                ui.add_space(8.0);

                if ui
                    .checkbox(
                        &mut self.capture.settings.auto_import_on_stop,
                        tr(lang, "Exportar automaticamente para a timeline ao parar"),
                    )
                    .changed()
                {
                    changed = true;
                }
                ui.add_space(10.0);
                ui.label(RichText::new(tr(lang, "Versão instalada")).strong());
                ui.label(
                    RichText::new(format!("Neoveg Capture {CAPTURE_VERSION}"))
                        .small()
                        .color(theme.text_muted),
                );
            });

        if browse_dir {
            if let Some(folder) = rfd::FileDialog::new()
                .set_title(tr(lang, "Escolher pasta de gravação"))
                .pick_folder()
            {
                self.capture.settings.recording_folder = folder.to_string_lossy().to_string();
                changed = true;
                self.capture.refresh_free_disk(&folder, true);
            }
        }
        if use_project_dir {
            self.capture.settings.recording_folder.clear();
            changed = true;
            let proj_dir = self
                .capture
                .settings
                .effective_recording_dir(&self.project.work_folders, self.path.as_deref());
            self.capture.refresh_free_disk(&proj_dir, true);
        }
        if open_dir {
            let _ = std::fs::create_dir_all(rec_dir);
            if let Err(e) = crate::crash_report::open_folder(rec_dir) {
                self.capture.set_note(e, true);
            }
        }
        if changed {
            self.capture.settings.sanitize();
            let _ = self.capture.settings.save_to_disk();
        }
        if close_settings {
            self.capture.settings_open = false;
        }
    }

    fn draw_capture_source_picker(
        &mut self,
        ui: &mut egui::Ui,
        lang: Language,
        theme: &ThemeColors,
    ) {
        let mut close_picker = false;
        let mut refresh = false;
        let mut to_add: Vec<(String, CaptureSourceKind)> = Vec::new();
        let mut to_remove_key: Vec<String> = Vec::new();

        ui.horizontal(|ui| {
            ui.heading(tr(lang, "Selecionar fontes de captura"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr(lang, "Concluir")).clicked() {
                    close_picker = true;
                }
                if ui.button(tr(lang, "Atualizar lista")).clicked() {
                    refresh = true;
                }
            });
        });
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            for tab in CapturePickerTab::ALL {
                ui.selectable_value(&mut self.capture.picker_tab, tab, tab.label_in(lang));
            }
        });
        ui.separator();

        if self.capture.discovered.is_none() {
            self.capture.refresh_sources();
        }
        let disc = self.capture.discovered.clone().unwrap_or_default();

        egui::ScrollArea::vertical()
            .id_salt("nvcap_picker_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| match self.capture.picker_tab {
                CapturePickerTab::Screens => {
                    if disc.monitors.is_empty() {
                        ui.label(
                            RichText::new(tr(
                                lang,
                                "Nenhuma tela detectada pelo sistema gráfico atual.",
                            ))
                            .color(theme.text_muted),
                        );
                    }
                    for mon in &disc.monitors {
                        let kind = CaptureSourceKind::Screen {
                            monitor_id: mon.id.clone(),
                            x: mon.x,
                            y: mon.y,
                            width: mon.width,
                            height: mon.height,
                        };
                        let key = kind.unique_key();
                        let present = self.capture.has_source_key(&key);
                        let sub = format!("{}×{}  ({}, {})", mon.width, mon.height, mon.x, mon.y);
                        if draw_picker_row(ui, &mon.name, &sub, present, lang, theme) {
                            if present {
                                to_remove_key.push(key);
                            } else {
                                to_add.push((mon.name.clone(), kind));
                            }
                        }
                    }
                }
                CapturePickerTab::Windows => {
                    if disc.windows.is_empty() {
                        ui.label(
                            RichText::new(tr(lang, "Nenhuma janela visível encontrada."))
                                .color(theme.text_muted),
                        );
                    }
                    for win in &disc.windows {
                        let kind = CaptureSourceKind::Window {
                            window_id: win.id.clone(),
                            width: win.width,
                            height: win.height,
                        };
                        let key = kind.unique_key();
                        let present = self.capture.has_source_key(&key);
                        let sub = format!("{}×{}", win.width, win.height);
                        if draw_picker_row(ui, &win.title, &sub, present, lang, theme) {
                            if present {
                                to_remove_key.push(key);
                            } else {
                                to_add.push((win.title.clone(), kind));
                            }
                        }
                    }
                }
                CapturePickerTab::Cameras => {
                    if disc.cameras.is_empty() {
                        ui.label(
                            RichText::new(tr(
                                lang,
                                "Nenhuma câmera ou dispositivo de vídeo conectado.",
                            ))
                            .color(theme.text_muted),
                        );
                    }
                    for cam in &disc.cameras {
                        let kind = CaptureSourceKind::Camera {
                            device_id: cam.id.clone(),
                            width: cam.default_width,
                            height: cam.default_height,
                        };
                        let key = kind.unique_key();
                        let present = self.capture.has_source_key(&key);
                        let sub = format!("{}×{}", cam.default_width, cam.default_height);
                        if draw_picker_row(ui, &cam.name, &sub, present, lang, theme) {
                            if present {
                                to_remove_key.push(key);
                            } else {
                                to_add.push((cam.name.clone(), kind));
                            }
                        }
                    }
                }
                CapturePickerTab::Audio => {
                    ui.label(RichText::new(tr(lang, "Microfones e entradas")).strong());
                    if disc.audio_inputs.is_empty() {
                        ui.label(
                            RichText::new(tr(lang, "Nenhum dispositivo de entrada encontrado."))
                                .small()
                                .color(theme.text_muted),
                        );
                    }
                    for dev in &disc.audio_inputs {
                        let kind = CaptureSourceKind::AudioInput {
                            device_name: dev.name.clone(),
                        };
                        let key = kind.unique_key();
                        let present = self.capture.has_source_key(&key);
                        let sub = format!(
                            "{} ch · {} Hz{}",
                            dev.channels,
                            dev.sample_rate,
                            if dev.is_default { " (Padrão)" } else { "" }
                        );
                        if draw_picker_row(ui, &dev.name, &sub, present, lang, theme) {
                            if present {
                                to_remove_key.push(key);
                            } else {
                                to_add.push((dev.name.clone(), kind));
                            }
                        }
                    }

                    ui.add_space(8.0);
                    ui.label(RichText::new(tr(lang, "Áudio do sistema (Loopback)")).strong());
                    if disc.audio_outputs.is_empty() {
                        ui.label(
                            RichText::new(tr(
                                lang,
                                "Loopback direto de saída está disponível via WASAPI no Windows.",
                            ))
                            .small()
                            .color(theme.text_muted),
                        );
                    }
                    for dev in &disc.audio_outputs {
                        let kind = CaptureSourceKind::AudioOutputLoopback {
                            device_name: dev.name.clone(),
                        };
                        let key = kind.unique_key();
                        let present = self.capture.has_source_key(&key);
                        let title = format!("{} ({})", dev.name, tr(lang, "Áudio do sistema"));
                        let sub = format!(
                            "{} ch · {} Hz{}",
                            dev.channels,
                            dev.sample_rate,
                            if dev.is_default { " (Padrão)" } else { "" }
                        );
                        if draw_picker_row(ui, &title, &sub, present, lang, theme) {
                            if present {
                                to_remove_key.push(key);
                            } else {
                                to_add.push((title, kind));
                            }
                        }
                    }
                }
            });

        for (name, kind) in to_add {
            self.capture.add_source(name, kind);
        }
        for key in to_remove_key {
            if let Some(id) = self
                .capture
                .sources
                .iter()
                .find(|s| s.kind.unique_key() == key)
                .map(|s| s.id)
            {
                self.capture.remove_source(id);
            }
        }
        if refresh {
            self.capture.refresh_sources();
        }
        if close_picker {
            self.capture.picker_open = false;
        }
    }

    fn draw_capture_dashboard(&mut self, ui: &mut egui::Ui, lang: Language, theme: &ThemeColors) {
        if self.capture.sources.is_empty() {
            // Tela inicial limpa (semelhante a capture-start.png, com ícone vetorial original)
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() * 0.20).clamp(16.0, 96.0));
                let (rect, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
                ui.painter().rect_filled(rect, 10.0, theme.accent);
                let inner = rect.shrink(11.0);
                icons::paint(ui.painter(), inner, Icon::ScreenCapture, theme, true);
                ui.add_space(10.0);
                ui.heading(RichText::new("Neoveg Capture").strong());
                ui.label(
                    RichText::new(tr(lang, "Selecione fontes de captura para começar."))
                        .color(theme.text_muted),
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let avail = ui.available_width();
                    let pad = ((avail - 360.0) * 0.5).max(0.0);
                    ui.add_space(pad);
                    if ui
                        .button(
                            RichText::new(format!("+ {}", tr(lang, "Adicionar fontes"))).strong(),
                        )
                        .clicked()
                    {
                        self.capture.picker_open = true;
                        self.capture.refresh_sources();
                    }
                    if ui.button(tr(lang, "Detectar tela e microfone")).clicked() {
                        let n = self.capture.add_default_sources();
                        if n == 0 {
                            self.capture.picker_open = true;
                            self.capture.set_note(
                                tr(lang, "Nenhuma fonte padrão detectada automaticamente."),
                                false,
                            );
                        }
                    }
                });
            });
            return;
        }

        let recording = self.capture.is_recording();
        let avail_w = ui.available_width();
        let cols = if avail_w >= 680.0 { 2 } else { 1 };
        let card_w = ((avail_w - 16.0 * (cols as f32 - 1.0) - 12.0) / cols as f32).max(260.0);

        let ids: Vec<u64> = self.capture.sources.iter().map(|s| s.id).collect();
        let mut to_remove: Vec<u64> = Vec::new();
        let mut toggle_expand: Option<u64> = None;

        egui::ScrollArea::vertical()
            .id_salt("nvcap_cards_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for row in ids.chunks(cols) {
                    ui.horizontal_top(|ui| {
                        for &sid in row {
                            let snap = self.capture.runtime.live_snapshot(sid);
                            if let Some(s) = snap.as_ref() {
                                if let Some(img) = s.preview.as_ref() {
                                    let needs_upload = self
                                        .capture
                                        .preview_textures
                                        .get(&sid)
                                        .is_none_or(|(g, _)| *g != s.preview_gen);
                                    if needs_upload {
                                        let ci = rgba_to_color_image(img);
                                        let tex = ui.ctx().load_texture(
                                            format!("nvcap_prev_tex_{sid}"),
                                            ci,
                                            egui::TextureOptions::LINEAR,
                                        );
                                        self.capture
                                            .preview_textures
                                            .insert(sid, (s.preview_gen, tex));
                                    }
                                }
                            }
                            let tex_opt = self
                                .capture
                                .preview_textures
                                .get(&sid)
                                .map(|(_, t)| t.clone());
                            let expanded = self.capture.expanded_card_id == Some(sid);
                            if let Some(card) =
                                self.capture.sources.iter_mut().find(|c| c.id == sid)
                            {
                                let (rem, exp) = draw_source_card(
                                    ui,
                                    card,
                                    snap.as_ref(),
                                    tex_opt.as_ref(),
                                    card_w,
                                    expanded,
                                    recording,
                                    lang,
                                    theme,
                                );
                                if rem {
                                    to_remove.push(sid);
                                }
                                if exp {
                                    toggle_expand = Some(sid);
                                }
                            }
                            ui.add_space(8.0);
                        }
                    });
                    ui.add_space(8.0);
                }
            });

        if let Some(sid) = toggle_expand {
            if self.capture.expanded_card_id == Some(sid) {
                self.capture.expanded_card_id = None;
            } else {
                self.capture.expanded_card_id = Some(sid);
            }
        }
        for id in to_remove {
            self.capture.remove_source(id);
        }
    }
}

fn draw_picker_row(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    present: bool,
    lang: Language,
    theme: &ThemeColors,
) -> bool {
    let mut toggled = false;
    egui::Frame::NONE
        .fill(theme.widget_bg)
        .stroke(Stroke::new(
            1.0,
            if present { theme.accent } else { theme.border },
        ))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).strong());
                    ui.label(RichText::new(subtitle).small().color(theme.text_muted));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let lbl = if present {
                        tr(lang, "Remover")
                    } else {
                        tr(lang, "Adicionar")
                    };
                    if ui.button(lbl).clicked() {
                        toggled = true;
                    }
                });
            });
        });
    ui.add_space(4.0);
    toggled
}

#[allow(clippy::too_many_arguments)]
fn draw_source_card(
    ui: &mut egui::Ui,
    card: &mut CaptureSourceCard,
    snap: Option<&neoveg_engine::capture::SourceLiveSnapshot>,
    tex: Option<&TextureHandle>,
    card_w: f32,
    expanded: bool,
    recording: bool,
    lang: Language,
    theme: &ThemeColors,
) -> (bool, bool) {
    let mut remove_clicked = false;
    let mut expand_clicked = false;

    let border_col = if snap.is_some_and(|s| s.status == SourceRuntimeStatus::Recording) {
        Color32::from_rgb(208, 66, 48)
    } else if snap.is_some_and(|s| s.status == SourceRuntimeStatus::Error) {
        theme.offline_fg
    } else if expanded {
        theme.accent
    } else {
        theme.border
    };

    egui::Frame::NONE
        .fill(theme.widget_bg)
        .stroke(Stroke::new(1.0, border_col))
        .corner_radius(5.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width((card_w - 18.0).max(240.0));
            let inner_w = ui.available_width();

            if card.is_video() {
                // Área de prévia 16:9 ao vivo
                let prev_h = (inner_w * 9.0 / 16.0).clamp(110.0, 210.0);
                let (rect, _) = ui.allocate_exact_size(vec2(inner_w, prev_h), Sense::hover());
                ui.painter().rect_filled(rect, 3.0, theme.extreme_bg);
                if let Some(t) = tex {
                    let tsz = t.size_vec2();
                    let sx = rect.width() / tsz.x.max(1.0);
                    let sy = rect.height() / tsz.y.max(1.0);
                    let sc = sx.min(sy);
                    let dw = tsz.x * sc;
                    let dh = tsz.y * sc;
                    let img_rect = Rect::from_center_size(rect.center(), vec2(dw, dh));
                    egui::Image::new((t.id(), img_rect.size())).paint_at(ui, img_rect);
                } else {
                    let status_txt = snap
                        .and_then(|s| s.last_error.as_deref())
                        .unwrap_or_else(|| tr(lang, "Aguardando prévia..."));
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        status_txt,
                        egui::FontId::proportional(11.0),
                        theme.text_muted,
                    );
                }
                // Selo de status no canto superior esquerdo da prévia
                let badge = match snap.map(|s| s.status) {
                    Some(SourceRuntimeStatus::Recording) => {
                        Some((tr(lang, "REC"), Color32::from_rgb(208, 66, 48)))
                    }
                    Some(SourceRuntimeStatus::Error) => Some((tr(lang, "Erro"), theme.offline_fg)),
                    Some(SourceRuntimeStatus::Monitoring) => {
                        Some((tr(lang, "Ao vivo"), Color32::from_rgb(42, 148, 76)))
                    }
                    _ => None,
                };
                if let Some((txt, bg)) = badge {
                    let br = Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(52.0, 17.0));
                    ui.painter().rect_filled(br, 3.0, bg);
                    ui.painter().text(
                        br.center(),
                        egui::Align2::CENTER_CENTER,
                        txt,
                        egui::FontId::proportional(10.0),
                        Color32::WHITE,
                    );
                }
                ui.add_space(6.0);
            } else {
                // Medidor VU estéreo ao vivo com escala em dBFS (VEGAS Capture 21–26)
                let peaks = snap.map(|s| s.peak_db).unwrap_or([-96.0, -96.0]);
                draw_stereo_vu_meter(ui, inner_w, peaks, card.muted, theme);
                ui.add_space(6.0);
            }

            // Linha do título + controles rápidos (Ativar, Silenciar, Configurações do dispositivo, Remover)
            ui.horizontal(|ui| {
                ui.add_enabled(!recording, egui::Checkbox::without_text(&mut card.enabled));
                ui.vertical(|ui| {
                    ui.label(RichText::new(&card.name).strong());
                    ui.label(
                        RichText::new(format!(
                            "{} · {}",
                            card.kind.category_label(lang),
                            card.summary_line(lang)
                        ))
                        .small()
                        .color(theme.text_muted),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(!recording, egui::Button::new("×").small())
                        .on_hover_text(tr(lang, "Remover fonte"))
                        .clicked()
                    {
                        remove_clicked = true;
                    }
                    let cfg_btn =
                        icons::small_icon_button(ui, Icon::GearProps, None, expanded, theme)
                            .on_hover_text(tr(lang, "Configurações do dispositivo"));
                    if cfg_btn.clicked() {
                        expand_clicked = true;
                    }
                    if card.is_audio() {
                        let mute_btn =
                            icons::small_icon_button(ui, Icon::MuteAudio, None, card.muted, theme)
                                .on_hover_text(if card.muted {
                                    tr(lang, "Reativar áudio")
                                } else {
                                    tr(lang, "Silenciar fonte")
                                });
                        if !recording && mute_btn.clicked() {
                            card.muted = !card.muted;
                        }
                    }
                });
            });

            if let Some(err) = snap.and_then(|s| s.last_error.as_deref()) {
                ui.add_space(3.0);
                ui.label(RichText::new(err).small().color(theme.offline_fg));
            }

            // Gaveta expansível "Configurações do dispositivo" (espelho de capture-devicesettings.png)
            if expanded {
                ui.separator();
                ui.add_enabled_ui(!recording, |ui| {
                    if card.is_video() {
                        ui.horizontal_wrapped(|ui| {
                            ui.vertical(|ui| {
                                ui.label(RichText::new(tr(lang, "Resolução")).small());
                                egui::ComboBox::from_id_salt(("nvcap_res", card.id))
                                    .width(120.0)
                                    .selected_text(card.resolution.label_in(lang))
                                    .show_ui(ui, |ui| {
                                        for r in CaptureResolution::ALL {
                                            ui.selectable_value(
                                                &mut card.resolution,
                                                r,
                                                r.label_in(lang),
                                            );
                                        }
                                    });
                            });
                            ui.vertical(|ui| {
                                ui.label(RichText::new(tr(lang, "Taxa de quadros")).small());
                                let fps_lbl = CAPTURE_FPS_OPTIONS
                                    .iter()
                                    .find(|(_, f)| *f == card.fps)
                                    .map(|(l, _)| *l)
                                    .unwrap_or("30 fps");
                                egui::ComboBox::from_id_salt(("nvcap_fps", card.id))
                                    .width(92.0)
                                    .selected_text(fps_lbl)
                                    .show_ui(ui, |ui| {
                                        for (lbl, f) in CAPTURE_FPS_OPTIONS {
                                            ui.selectable_value(&mut card.fps, f, lbl);
                                        }
                                    });
                            });
                            ui.vertical(|ui| {
                                ui.label(RichText::new(tr(lang, "Codec")).small());
                                egui::ComboBox::from_id_salt(("nvcap_vc", card.id))
                                    .width(136.0)
                                    .selected_text(card.video_codec.short_name())
                                    .show_ui(ui, |ui| {
                                        for c in CaptureVideoCodec::ALL {
                                            if !c.available_on_os() {
                                                continue;
                                            }
                                            ui.selectable_value(
                                                &mut card.video_codec,
                                                c,
                                                c.label_in(lang),
                                            );
                                        }
                                    });
                            });
                            ui.vertical(|ui| {
                                ui.label(RichText::new(tr(lang, "Qualidade")).small());
                                egui::ComboBox::from_id_salt(("nvcap_vq", card.id))
                                    .width(96.0)
                                    .selected_text(card.video_quality.label_in(lang))
                                    .show_ui(ui, |ui| {
                                        for q in CaptureQuality::ALL {
                                            ui.selectable_value(
                                                &mut card.video_quality,
                                                q,
                                                q.label_in(lang),
                                            );
                                        }
                                    });
                            });
                        });
                        if matches!(
                            card.kind,
                            CaptureSourceKind::Screen { .. } | CaptureSourceKind::Window { .. }
                        ) {
                            ui.checkbox(
                                &mut card.capture_cursor,
                                tr(lang, "Capturar cursor do mouse"),
                            );
                        }
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            ui.vertical(|ui| {
                                ui.label(RichText::new(tr(lang, "Codec")).small());
                                egui::ComboBox::from_id_salt(("nvcap_ac", card.id))
                                    .width(120.0)
                                    .selected_text(card.audio_codec.short_name())
                                    .show_ui(ui, |ui| {
                                        for c in CaptureAudioCodec::ALL {
                                            if !c.available_on_os() {
                                                continue;
                                            }
                                            ui.selectable_value(
                                                &mut card.audio_codec,
                                                c,
                                                c.label_in(lang),
                                            );
                                        }
                                    });
                            });
                            ui.vertical(|ui| {
                                ui.label(RichText::new(tr(lang, "Canais")).small());
                                let ch_lbl = if card.audio_channels <= 1 {
                                    tr(lang, "Mono")
                                } else {
                                    tr(lang, "Estéreo")
                                };
                                egui::ComboBox::from_id_salt(("nvcap_ach", card.id))
                                    .width(90.0)
                                    .selected_text(ch_lbl)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut card.audio_channels,
                                            2,
                                            tr(lang, "Estéreo"),
                                        );
                                        ui.selectable_value(
                                            &mut card.audio_channels,
                                            1,
                                            tr(lang, "Mono"),
                                        );
                                    });
                            });
                            ui.vertical(|ui| {
                                ui.label(RichText::new(tr(lang, "Taxa de amostragem")).small());
                                egui::ComboBox::from_id_salt(("nvcap_asr", card.id))
                                    .width(96.0)
                                    .selected_text(format!("{} Hz", card.audio_sample_rate))
                                    .show_ui(ui, |ui| {
                                        for sr in CAPTURE_SAMPLE_RATES {
                                            ui.selectable_value(
                                                &mut card.audio_sample_rate,
                                                sr,
                                                format!("{sr} Hz"),
                                            );
                                        }
                                    });
                            });
                        });
                    }
                });
            }
        });

    (remove_clicked, expand_clicked)
}

/// Medidor VU estéreo horizontal com segmentos verde/amarelo/vermelho e escala
/// em dBFS (`-60 .. 0 dB`).
fn draw_stereo_vu_meter(
    ui: &mut egui::Ui,
    width: f32,
    peaks_db: [f32; 2],
    muted: bool,
    theme: &ThemeColors,
) {
    let h = 30.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, theme.extreme_bg);
    p.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0, theme.border),
        StrokeKind::Inside,
    );

    let bar_w = (rect.width() - 46.0).max(40.0);
    let bar_x0 = rect.left() + 4.0;
    let bar_h = 8.0;

    for (ch, &db) in peaks_db.iter().enumerate() {
        let y0 = rect.top() + 4.0 + (ch as f32) * (bar_h + 3.0);
        let bg_r = Rect::from_min_size(pos2(bar_x0, y0), vec2(bar_w, bar_h));
        p.rect_filled(bg_r, 1.5, theme.panel_bg);

        let norm = if muted {
            0.0
        } else {
            ((db + 60.0) / 60.0).clamp(0.0, 1.0)
        };
        let fill_w = bar_w * norm;
        if fill_w > 0.5 {
            // Segmentos verde (-60..-12), amarelo (-12..-3) e vermelho (-3..0)
            let green_end = bar_w * 0.78;
            let yellow_end = bar_w * 0.94;
            let g_w = fill_w.min(green_end);
            if g_w > 0.0 {
                p.rect_filled(
                    Rect::from_min_size(pos2(bar_x0, y0), vec2(g_w, bar_h)),
                    1.0,
                    Color32::from_rgb(46, 204, 86),
                );
            }
            if fill_w > green_end {
                let y_w = (fill_w - green_end).min(yellow_end - green_end);
                p.rect_filled(
                    Rect::from_min_size(pos2(bar_x0 + green_end, y0), vec2(y_w, bar_h)),
                    0.0,
                    Color32::from_rgb(236, 198, 38),
                );
            }
            if fill_w > yellow_end {
                let r_w = fill_w - yellow_end;
                p.rect_filled(
                    Rect::from_min_size(pos2(bar_x0 + yellow_end, y0), vec2(r_w, bar_h)),
                    0.0,
                    Color32::from_rgb(224, 62, 52),
                );
            }
        }
    }

    let peak_max = if muted {
        -96.0
    } else {
        peaks_db[0].max(peaks_db[1])
    };
    let db_label = if peak_max <= -90.0 {
        "-∞".to_string()
    } else {
        format!("{peak_max:.1}")
    };
    p.text(
        pos2(rect.right() - 4.0, rect.top() + 14.0),
        egui::Align2::RIGHT_CENTER,
        db_label,
        egui::FontId::monospace(9.5),
        theme.text_muted,
    );

    // Escala de referência abaixo das barras
    for &(mark_db, label) in &[
        (-48.0f32, "-48"),
        (-36.0, "-36"),
        (-24.0, "-24"),
        (-12.0, "-12"),
        (-6.0, "-6"),
    ] {
        let frac = ((mark_db + 60.0) / 60.0).clamp(0.0, 1.0);
        let x = bar_x0 + bar_w * frac;
        p.text(
            pos2(x, rect.bottom() - 3.5),
            egui::Align2::CENTER_BOTTOM,
            label,
            egui::FontId::monospace(7.5),
            theme.text_muted,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::{build_dock_tree, find_pane_tile, DockPane};
    use crate::layout::WindowLayout;
    use crate::theme::{palette, ThemeId};

    #[test]
    fn capture_abre_em_janela_propria_fora_do_dock() {
        let tree = build_dock_tree(0.40, DockTab::Capture, false);
        assert!(find_pane_tile(&tree, DockPane::Tab(DockTab::Capture)).is_none());

        let l = WindowLayout {
            active_tab: crate::layout::panel_key(DockTab::Capture).to_string(),
            ..WindowLayout::default()
        };
        assert_eq!(l.active_panel(), DockTab::ProjectMedia);

        let _lock = crate::kittest_tests::lock_test_prefs();
        let ctx = egui::Context::default();
        let mut app = {
            let _r = crate::tests::PREFS_LOCK
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            App::with_context(&ctx, None)
        };
        let antes = app.dock_tab;
        app.exec_cmd(Some(&ctx), "file.capture");
        assert!(app.detached_tabs.contains(&DockTab::Capture));
        assert!(!app.closed_tabs.contains(&DockTab::Capture));
        assert_eq!(app.dock_tab, antes, "a aba ativa do dock não muda");

        app.close_dock_tab(DockTab::Capture);
        assert!(!app.detached_tabs.contains(&DockTab::Capture));
        assert!(app.closed_tabs.contains(&DockTab::Capture));
    }

    #[test]
    fn capture_painel_renderiza_vazio_configuracoes_e_cartoes_em_todos_os_temas() {
        let _lock = crate::kittest_tests::lock_test_prefs();
        let ctx = egui::Context::default();
        let mut app = {
            let _r = crate::tests::PREFS_LOCK
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            App::with_context(&ctx, None)
        };

        // Renderiza estado vazio, picker, configurações e cartões com configurações expandidas
        for tid in ThemeId::ALL {
            app.theme_id = tid;
            app.theme = palette(tid);
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.capture_tab(ui);
                });
            });
        }

        let vid_id = app.capture.add_source(
            "Tela 1".into(),
            CaptureSourceKind::SyntheticVideo {
                width: 128,
                height: 72,
                seed: 3,
            },
        );
        let _aud_id = app.capture.add_source(
            "Microfone".into(),
            CaptureSourceKind::SyntheticAudio {
                freq_hz: 440,
                sample_rate: 48_000,
            },
        );
        app.capture.expanded_card_id = Some(vid_id);
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.capture_tab(ui);
            });
        });

        app.capture.settings_open = true;
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.capture_tab(ui);
            });
        });
        app.capture.shutdown();
    }

    #[test]
    fn capture_gravacao_sintetica_e_exportacao_para_timeline_via_comandos() {
        let _lock = crate::kittest_tests::lock_test_prefs();
        let ctx = egui::Context::default();
        let mut app = {
            let _r = crate::tests::PREFS_LOCK
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            App::with_context(&ctx, None)
        };
        let dir = std::env::temp_dir().join(format!("neoveg_ui_cap_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("criar pasta de captura de teste");
        app.capture.settings.recording_folder = dir.to_string_lossy().to_string();
        app.capture.settings.countdown_secs = 0;

        let vid_id = app.capture.add_source(
            "Tela Teste".into(),
            CaptureSourceKind::SyntheticVideo {
                width: 96,
                height: 54,
                seed: 5,
            },
        );
        if let Some(c) = app.capture.sources.iter_mut().find(|s| s.id == vid_id) {
            c.video_codec = CaptureVideoCodec::MjpegAvi;
        }
        let _aud_id = app.capture.add_source(
            "Mic Teste".into(),
            CaptureSourceKind::SyntheticAudio {
                freq_hz: 440,
                sample_rate: 48_000,
            },
        );

        let tracks_before = app.project.tracks.len();
        app.exec_cmd(Some(&ctx), "tools.capture_toggle_record");
        assert!(app.capture.is_recording());
        std::thread::sleep(Duration::from_millis(160));
        app.exec_cmd(Some(&ctx), "tools.capture_toggle_record");
        assert!(!app.capture.is_recording());
        assert!(app.capture.last_session.is_some());

        app.exec_cmd(Some(&ctx), "tools.capture_export");
        assert_eq!(app.project.tracks.len(), tracks_before + 2);
        assert!(app
            .project
            .tracks
            .iter()
            .any(|t| t.name == "Tela Teste" && t.events.len() == 1));
        assert!(app
            .project
            .tracks
            .iter()
            .any(|t| t.name == "Mic Teste" && t.events.len() == 1));

        app.capture.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
