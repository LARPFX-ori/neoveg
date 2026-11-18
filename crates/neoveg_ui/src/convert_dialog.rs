//! Conversão de mídia que o Neoveg não abre diretamente: oferta ao usuário,
//! escolha do destino, progresso com cancelamento e importação do resultado.
//!
//! A conversão em si fica em `neoveg_engine::media::convert`. Nada muda no
//! projeto nem no histórico de Desfazer até a conversão terminar; depois o
//! arquivo convertido entra pelo caminho normal de importação, na posição
//! pedida originalmente.

use crate::app::{App, LoadMsg, PlaceTarget};
use eframe::egui;
use neoveg_common::{tr, tr_args, util};
use neoveg_engine::media::convert::{self, ConvertProgress, Converted};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// Arquivos aguardando confirmação para conversão.
pub struct ConvertOffer {
    /// Arquivo, posição de importação pedida e motivo da oferta.
    pub files: Vec<(PathBuf, Option<PlaceTarget>, String)>,
    /// `true` grava ao lado do original; `false` usa `folder`.
    pub next_to_original: bool,
    pub folder: Option<PathBuf>,
}

/// Conversão em andamento.
pub struct ConvertJob {
    pub progress: Arc<ConvertProgress>,
    pub total: usize,
    pub finished: usize,
}

impl App {
    /// Acrescenta um arquivo à oferta de conversão (abre o diálogo).
    pub fn offer_conversion(
        &mut self,
        path: PathBuf,
        place: Option<PlaceTarget>,
        reason: impl Into<String>,
    ) {
        let offer = self.dlg.convert_offer.get_or_insert_with(|| ConvertOffer {
            files: vec![],
            next_to_original: true,
            folder: None,
        });
        if !offer.files.iter().any(|(p, _, _)| *p == path) {
            offer.files.push((path, place, reason.into()));
        }
    }

    /// Comando `file.convert_media`: escolhe arquivos e oferece a conversão.
    pub fn convert_media_dialog(&mut self) {
        let lang = self.lang();
        if let Some(files) = rfd::FileDialog::new()
            .add_filter(tr(lang, "Todos os arquivos"), &["*"])
            .pick_files()
        {
            let reason = tr(lang, "escolhido para conversão");
            for f in files {
                self.offer_conversion(f, None, reason);
            }
        }
    }

    fn start_conversion(&mut self, offer: ConvertOffer) {
        let lang = self.lang();
        let n = offer.files.len();
        if n == 0 {
            return;
        }
        let folder = if offer.next_to_original {
            None
        } else {
            offer.folder
        };
        let progress = Arc::new(ConvertProgress::default());
        let worker_progress = progress.clone();
        let tx = self.loader_tx.clone();
        let files = offer.files;
        let cancel_msg = tr(lang, "conversão cancelada").to_string();
        // uma mensagem `Converted` por arquivo, mesmo se cancelado
        self.loading += n;
        let spawned = util::spawn("converter-midia", move || {
            for (src, place, _) in files {
                let result = if worker_progress.cancel.load(Ordering::Relaxed) {
                    Err(cancel_msg.clone())
                } else {
                    let dir = folder
                        .clone()
                        .or_else(|| src.parent().map(Path::to_path_buf))
                        .unwrap_or_else(|| PathBuf::from("."));
                    convert::convert_to_compatible(&src, &dir, &worker_progress)
                        .map_err(|e| format!("{e:#}"))
                };
                let _ = tx.send(LoadMsg::Converted { src, result, place });
            }
        });
        match spawned {
            Ok(()) => {
                self.dlg.convert_job = Some(ConvertJob {
                    progress,
                    total: n,
                    finished: 0,
                });
            }
            Err(e) => {
                self.loading = self.loading.saturating_sub(n);
                self.set_status(e);
            }
        }
    }

    /// Resultado de um arquivo convertido (chamado pelo `poll_loader`).
    pub(crate) fn on_converted(
        &mut self,
        src: &Path,
        result: Result<Converted, String>,
        place: Option<PlaceTarget>,
    ) {
        let lang = self.lang();
        if let Some(job) = &mut self.dlg.convert_job {
            job.finished += 1;
            if job.finished >= job.total {
                self.dlg.convert_job = None;
            }
        }
        match result {
            Ok(c) => {
                for w in &c.warnings {
                    util::log_warn(&format!("conversão de {}: {w}", src.display()));
                }
                let src_s = src.display().to_string();
                let dst_s = c.path.display().to_string();
                let mut msg = tr_args(
                    lang,
                    "Convertido: {src} → {dst}",
                    &[("src", &src_s), ("dst", &dst_s)],
                );
                if !c.warnings.is_empty() {
                    msg.push_str(&format!(" ({})", c.warnings.join("; ")));
                }
                self.set_status(msg);
                self.spawn_import(c.path, place);
            }
            Err(e) => {
                let src_s = src.display().to_string();
                self.set_status(tr_args(
                    lang,
                    "Conversão de {src} falhou: {err}",
                    &[("src", &src_s), ("err", &e)],
                ));
            }
        }
    }

    /// Janelas de confirmação e de progresso da conversão.
    pub(crate) fn convert_windows(&mut self, ctx: &egui::Context) {
        self.convert_offer_window(ctx);
        self.convert_progress_window(ctx);
    }

    fn convert_offer_window(&mut self, ctx: &egui::Context) {
        let lang = self.lang();
        let busy = self.dlg.convert_job.is_some();
        let Some(offer) = self.dlg.convert_offer.as_mut() else {
            return;
        };
        let mut open = true;
        let mut start = false;
        let mut cancel = false;
        let mut pick_folder = false;
        egui::Window::new(tr(lang, "Converter mídia"))
            .id(egui::Id::new("convert_offer_win"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.label(tr(
                    lang,
                    "Estes arquivos não podem ser abertos diretamente. O Neoveg pode \
                     convertê-los com o decodificador do sistema para um arquivo \
                     compatível (vídeo: AVI MJPEG + PCM; só áudio: WAV 24 bits).",
                ));
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        for (p, _, reason) in &offer.files {
                            let name = p
                                .file_name()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_else(|| p.display().to_string());
                            ui.label(egui::RichText::new(name).strong())
                                .on_hover_text(p.display().to_string());
                            ui.label(egui::RichText::new(reason).small().weak());
                        }
                    });
                ui.separator();
                ui.label(tr(lang, "Gravar o arquivo convertido:"));
                ui.radio_value(
                    &mut offer.next_to_original,
                    true,
                    tr(lang, "Na pasta do original"),
                );
                ui.horizontal(|ui| {
                    ui.radio_value(&mut offer.next_to_original, false, tr(lang, "Nesta pasta:"));
                    let label = offer
                        .folder
                        .as_ref()
                        .map(|f| f.display().to_string())
                        .unwrap_or_else(|| tr(lang, "(nenhuma escolhida)").into());
                    ui.label(label);
                    if ui.button(tr(lang, "Escolher...")).clicked() {
                        pick_folder = true;
                    }
                });
                ui.label(
                    egui::RichText::new(tr(
                        lang,
                        "O AVI MJPEG ocupa bastante espaço (centenas de MB por minuto em HD). \
                         Evite pastas sincronizadas na nuvem.",
                    ))
                    .small()
                    .weak(),
                );
                ui.separator();
                ui.horizontal(|ui| {
                    let can_start = !busy && (offer.next_to_original || offer.folder.is_some());
                    let btn = ui.add_enabled(can_start, egui::Button::new(tr(lang, "Converter")));
                    if busy {
                        let _ = btn
                            .on_disabled_hover_text(tr(lang, "Aguarde a conversão em andamento."));
                    } else if btn.clicked() {
                        start = true;
                    }
                    if ui.button(tr(lang, "Cancelar")).clicked() {
                        cancel = true;
                    }
                });
            });
        if pick_folder {
            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                offer.folder = Some(dir);
                offer.next_to_original = false;
            }
        }
        if start {
            if let Some(offer) = self.dlg.convert_offer.take() {
                self.start_conversion(offer);
            }
        } else if cancel || !open {
            self.dlg.convert_offer = None;
        }
    }

    fn convert_progress_window(&mut self, ctx: &egui::Context) {
        let lang = self.lang();
        let Some(job) = &self.dlg.convert_job else {
            return;
        };
        let p = job.progress.clone();
        let (finished, total) = (job.finished, job.total);
        let current = p
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let mut cancel = false;
        let idx_s = (finished + 1).min(total).to_string();
        let total_s = total.to_string();
        egui::Window::new(tr(lang, "Convertendo mídia..."))
            .id(egui::Id::new("convert_progress_win"))
            .collapsible(true)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(tr_args(
                    lang,
                    "Arquivo {idx} de {total}: {name}",
                    &[("idx", &idx_s), ("total", &total_s), ("name", &current)],
                ));
                ui.add(
                    egui::ProgressBar::new(p.fraction())
                        .show_percentage()
                        .desired_width(360.0),
                );
                let cancelling = p.cancel.load(Ordering::Relaxed);
                if ui
                    .add_enabled(!cancelling, egui::Button::new(tr(lang, "Cancelar")))
                    .clicked()
                {
                    cancel = true;
                }
            });
        if cancel {
            p.cancel.store(true, Ordering::SeqCst);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}
