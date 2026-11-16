//! Conversão de mídia que o Neoveg não abre diretamente: oferta ao usuário,
//! escolha do destino, progresso com cancelamento e importação do resultado.
//!
//! A conversão em si fica em `neoveg_engine::media::convert`. Nada muda no
//! projeto nem no histórico de Desfazer até a conversão terminar; depois o
//! arquivo convertido entra pelo caminho normal de importação, na posição
//! pedida originalmente.

use crate::app::{App, LoadMsg, PlaceTarget};
use eframe::egui;
use neoveg_common::util;
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
        if let Some(files) = rfd::FileDialog::new()
            .add_filter("Todos os arquivos", &["*"])
            .pick_files()
        {
            for f in files {
                self.offer_conversion(f, None, "escolhido para conversão");
            }
        }
    }

    fn start_conversion(&mut self, offer: ConvertOffer) {
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
        // uma mensagem `Converted` por arquivo, mesmo se cancelado
        self.loading += n;
        let spawned = util::spawn("converter-midia", move || {
            for (src, place, _) in files {
                let result = if worker_progress.cancel.load(Ordering::Relaxed) {
                    Err("conversão cancelada".to_string())
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
                let mut msg = format!("Convertido: {} → {}", src.display(), c.path.display());
                if !c.warnings.is_empty() {
                    msg.push_str(&format!(" ({})", c.warnings.join("; ")));
                }
                self.set_status(msg);
                self.spawn_import(c.path, place);
            }
            Err(e) => self.set_status(format!("Conversão de {} falhou: {e}", src.display())),
        }
    }

    /// Janelas de confirmação e de progresso da conversão.
    pub(crate) fn convert_windows(&mut self, ctx: &egui::Context) {
        self.convert_offer_window(ctx);
        self.convert_progress_window(ctx);
    }

    fn convert_offer_window(&mut self, ctx: &egui::Context) {
        let busy = self.dlg.convert_job.is_some();
        let Some(offer) = self.dlg.convert_offer.as_mut() else {
            return;
        };
        let mut open = true;
        let mut start = false;
        let mut cancel = false;
        let mut pick_folder = false;
        egui::Window::new("Converter mídia")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.label(
                    "Estes arquivos não podem ser abertos diretamente. O Neoveg pode \
                     convertê-los com o decodificador do sistema para um arquivo \
                     compatível (vídeo: AVI MJPEG + PCM; só áudio: WAV 24 bits).",
                );
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
                ui.label("Gravar o arquivo convertido:");
                ui.radio_value(&mut offer.next_to_original, true, "Na pasta do original");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut offer.next_to_original, false, "Nesta pasta:");
                    let label = offer
                        .folder
                        .as_ref()
                        .map(|f| f.display().to_string())
                        .unwrap_or_else(|| "(nenhuma escolhida)".into());
                    ui.label(label);
                    if ui.button("Escolher...").clicked() {
                        pick_folder = true;
                    }
                });
                ui.label(
                    egui::RichText::new(
                        "O AVI MJPEG ocupa bastante espaço (centenas de MB por minuto em HD; \
                         limite de 4 GB por arquivo). Evite pastas sincronizadas na nuvem.",
                    )
                    .small()
                    .weak(),
                );
                ui.separator();
                ui.horizontal(|ui| {
                    let can_start = !busy && (offer.next_to_original || offer.folder.is_some());
                    let btn = ui.add_enabled(can_start, egui::Button::new("Converter"));
                    if busy {
                        let _ = btn.on_disabled_hover_text("Aguarde a conversão em andamento.");
                    } else if btn.clicked() {
                        start = true;
                    }
                    if ui.button("Cancelar").clicked() {
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
        egui::Window::new("Convertendo mídia...")
            .collapsible(true)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Arquivo {} de {}: {current}",
                    (finished + 1).min(total),
                    total
                ));
                ui.add(
                    egui::ProgressBar::new(p.fraction())
                        .show_percentage()
                        .desired_width(360.0),
                );
                let cancelling = p.cancel.load(Ordering::Relaxed);
                if ui
                    .add_enabled(!cancelling, egui::Button::new("Cancelar"))
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
