//! Janela do instalador: compacta, escura, com uma faixa no topo e os campos
//! empilhados embaixo, e uma fileira de botões no rodapé. A arte da faixa é
//! desenhada em código (original do Neoveg).

use crate::ops::{self, InstallOptions};
use crate::system::{self, Scope, APP_EXE};
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, FontId, Rect, RichText, Sense, Shape, Stroke, StrokeKind,
};
use neoveg_common::{tr, tr_args, util, Language};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

const WIN_W: f32 = 500.0;
const WIN_H: f32 = 500.0;
const BANNER_H: f32 = 210.0;

/// Abre a janela. `uninstall` mostra a desinstalação. Devolve o código de
/// saída do processo.
pub fn run(exe: PathBuf, uninstall: bool) -> i32 {
    let code = Arc::new(AtomicI32::new(1));
    let code_app = code.clone();
    let title = if uninstall {
        "Desinstalar o Neoveg"
    } else {
        "Instalar o Neoveg"
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([WIN_W, WIN_H])
            .with_resizable(false)
            .with_maximize_button(false)
            .with_icon(Arc::new(neoveg_ui::window_icon())),
        centered: true,
        ..Default::default()
    };
    let result = eframe::run_native(
        title,
        options,
        Box::new(move |cc| Ok(Box::new(SetupApp::new(cc, exe, uninstall, code_app)))),
    );
    match result {
        Ok(()) => code.load(Ordering::Relaxed),
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

/// Andamento do trabalho em segundo plano.
#[derive(Default)]
struct Job {
    done: u64,
    total: u64,
    result: Option<Result<(), String>>,
}

#[derive(Clone, Debug, PartialEq)]
enum Phase {
    Edit,
    Running,
    Done,
    Failed(String),
}

struct SetupApp {
    exe: PathBuf,
    uninstall: bool,
    lang: Language,
    scope: Scope,
    dir: String,
    dir_edited: bool,
    desktop: bool,
    assoc: bool,
    remove_data: bool,
    installed_dir: Option<PathBuf>,
    job: Option<Arc<Mutex<Job>>>,
    phase: Phase,
    about: bool,
    code: Arc<AtomicI32>,
}

impl SetupApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        exe: PathBuf,
        uninstall: bool,
        code: Arc<AtomicI32>,
    ) -> Self {
        // fontes do Neoveg, com as do sistema e a reserva japonesa: sem elas o
        // japonês aparece como quadrados
        neoveg_ui::fonts::configure_fonts(&cc.egui_ctx, true);
        style(&cc.egui_ctx);
        let installed_dir = ops::installed_dir_of(&exe);
        let dir = system::default_install_dir(Scope::User)
            .map(|d| d.display().to_string())
            .unwrap_or_default();
        SetupApp {
            exe,
            uninstall,
            lang: Language::PtBr,
            scope: Scope::User,
            dir,
            dir_edited: false,
            desktop: true,
            assoc: true,
            remove_data: false,
            installed_dir,
            job: None,
            phase: Phase::Edit,
            about: false,
            code,
        }
    }

    fn options(&self) -> InstallOptions {
        InstallOptions {
            scope: self.scope,
            dir: PathBuf::from(self.dir.trim()),
            desktop_shortcut: self.desktop,
            associate_files: self.assoc,
        }
    }

    fn start_install(&mut self) {
        let opts = self.options();
        if !opts.dir.is_absolute() {
            self.phase = Phase::Failed(
                tr(
                    self.lang,
                    if cfg!(windows) {
                        "Escolha uma pasta de instalação completa (ex.: C:\\...)."
                    } else {
                        "Escolha uma pasta de instalação completa (ex.: /opt/neoveg)."
                    },
                )
                .into(),
            );
            return;
        }
        let job = Arc::new(Mutex::new(Job::default()));
        let job_w = job.clone();
        let exe = self.exe.clone();
        let spawned = util::spawn("instalador", move || {
            let r = util::guard("instalação", || {
                if opts.scope == Scope::AllUsers && !system::is_elevated() {
                    match system::run_elevated(&exe, &opts.to_args()) {
                        Ok(0) => Ok(()),
                        Ok(c) => Err(format!(
                            "a instalação como administrador falhou (código {c})"
                        )),
                        Err(e) => Err(e),
                    }
                } else {
                    ops::install(&opts, &exe, &mut |d, t| {
                        let mut g = util::lock(&job_w);
                        g.done = d;
                        g.total = t;
                    })
                }
            })
            .and_then(|r| r);
            util::lock(&job_w).result = Some(r);
        });
        match spawned {
            Ok(()) => {
                self.job = Some(job);
                self.phase = Phase::Running;
            }
            Err(e) => self.phase = Phase::Failed(e),
        }
    }

    fn start_uninstall(&mut self) {
        let Some(dir) = self.installed_dir.clone() else {
            return;
        };
        let job = Arc::new(Mutex::new(Job::default()));
        let job_w = job.clone();
        let exe = self.exe.clone();
        let remove_data = self.remove_data;
        let spawned = util::spawn("desinstalador", move || {
            let r = util::guard("desinstalação", || {
                let scope = ops::installed_scope(&dir).unwrap_or(Scope::User);
                if scope == Scope::AllUsers && !system::is_elevated() {
                    let args = vec!["--uninstall".to_string(), "--silent".to_string()];
                    match system::run_elevated(&exe, &args) {
                        Ok(0) => {
                            // os dados ficam na pasta deste usuário, não na do
                            // administrador: apagados por este processo
                            if remove_data {
                                if let Some(d) = ops::user_data_dir() {
                                    let _ = std::fs::remove_dir_all(d);
                                }
                            }
                            Ok(())
                        }
                        Ok(c) => Err(format!(
                            "a desinstalação como administrador falhou (código {c})"
                        )),
                        Err(e) => Err(e),
                    }
                } else {
                    ops::uninstall(&dir, remove_data)
                }
            })
            .and_then(|r| r);
            util::lock(&job_w).result = Some(r);
        });
        match spawned {
            Ok(()) => {
                self.job = Some(job);
                self.phase = Phase::Running;
            }
            Err(e) => self.phase = Phase::Failed(e),
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.job else {
            return;
        };
        let result = util::lock(job).result.clone();
        match result {
            Some(Ok(())) => {
                self.phase = Phase::Done;
                self.code.store(0, Ordering::Relaxed);
                self.job = None;
            }
            Some(Err(e)) => {
                self.phase = Phase::Failed(e);
                self.job = None;
            }
            None => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
        }
    }

    fn progress(&self) -> Option<f32> {
        let job = self.job.as_ref()?;
        let g = util::lock(job);
        (g.total > 0).then(|| (g.done as f64 / g.total as f64).clamp(0.0, 1.0) as f32)
    }
}

/// Visual escuro e compacto.
fn style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::from_rgb(12, 13, 16);
    v.window_fill = Color32::from_rgb(18, 19, 23);
    v.extreme_bg_color = Color32::from_rgb(6, 6, 8);
    v.selection.bg_fill = Color32::from_rgb(38, 110, 210);
    ctx.set_visuals(v);
}

/// Faixa do topo (arte original, desenhada em código).
fn banner(ui: &mut egui::Ui, subtitle: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), BANNER_H), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, Color32::from_rgb(10, 22, 48));
    // faixas diagonais de fundo
    let step = 18.0;
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        let a = pos2(x, rect.bottom());
        let b = pos2(x + rect.height(), rect.top());
        p.line_segment([a, b], Stroke::new(6.0_f32, Color32::from_rgb(16, 34, 70)));
        x += step;
    }
    // lente estilizada à direita
    let c = pos2(rect.right() - 92.0, rect.center().y - 6.0);
    p.circle_filled(c, 70.0, Color32::from_rgb(30, 70, 150));
    p.circle_filled(c, 50.0, Color32::from_rgb(14, 30, 64));
    p.circle_stroke(
        c,
        50.0,
        Stroke::new(3.0_f32, Color32::from_rgb(80, 170, 255)),
    );
    p.circle_filled(
        c + vec2(-14.0, -14.0),
        12.0,
        Color32::from_rgb(120, 210, 255),
    );
    p.circle_filled(c + vec2(16.0, 14.0), 9.0, Color32::from_rgb(130, 90, 230));
    // marca "N"
    let mark = Rect::from_min_size(
        pos2(rect.left() + 22.0, rect.top() + 26.0),
        vec2(44.0, 44.0),
    );
    p.rect_filled(mark, 6.0, Color32::from_rgb(20, 24, 32));
    p.rect_stroke(
        mark,
        6.0,
        Stroke::new(2.0_f32, Color32::from_rgb(80, 170, 255)),
        StrokeKind::Inside,
    );
    p.text(
        mark.center(),
        Align2::CENTER_CENTER,
        "N",
        FontId::proportional(28.0),
        Color32::WHITE,
    );
    // nome grande, com contorno
    let name_pos = pos2(rect.left() + 22.0, rect.top() + 118.0);
    for (dx, dy) in [(-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0)] {
        p.text(
            name_pos + vec2(dx, dy),
            Align2::LEFT_CENTER,
            "NEOVEG",
            FontId::proportional(64.0),
            Color32::from_rgb(4, 8, 18),
        );
    }
    p.text(
        name_pos,
        Align2::LEFT_CENTER,
        "NEOVEG",
        FontId::proportional(64.0),
        Color32::from_rgb(235, 242, 255),
    );
    // faixa de rodapé com o subtítulo
    let strip = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 26.0), rect.max);
    p.add(Shape::rect_filled(
        strip,
        0.0,
        Color32::from_rgba_unmultiplied(0, 0, 0, 170),
    ));
    p.text(
        pos2(strip.left() + 12.0, strip.center().y),
        Align2::LEFT_CENTER,
        subtitle,
        FontId::proportional(13.0),
        Color32::from_rgb(170, 200, 240),
    );
    p.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0_f32, Color32::from_rgb(60, 70, 90)),
        StrokeKind::Inside,
    );
}

/// Rótulo pequeno acima de um campo.
fn field_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(
        RichText::new(text)
            .size(12.5)
            .color(Color32::from_rgb(190, 196, 206)),
    );
}

/// Botão do rodapé, todos da mesma largura.
fn foot_button(ui: &mut egui::Ui, text: &str, w: f32, enabled: bool) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).size(14.0)).min_size(vec2(w, 28.0)),
    )
    .clicked()
}

impl eframe::App for SetupApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        let lang = self.lang;
        let running = self.phase == Phase::Running;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(egui::Margin::same(12)),
            )
            .show(ctx, |ui| {
                let subtitle = tr_args(
                    lang,
                    "Editor de vídeo · versão {ver}",
                    &[("ver", &ops::VERSION)],
                );
                banner(ui, &subtitle);
                ui.add_space(6.0);
                field_label(ui, tr(lang, "Idioma:"));
                egui::ComboBox::from_id_salt("setup_lang")
                    .width(ui.available_width())
                    .selected_text(lang.native_name())
                    .show_ui(ui, |ui| {
                        for l in Language::ALL {
                            ui.selectable_value(&mut self.lang, l, l.native_name());
                        }
                    });
                if self.lang != lang {
                    let title = if self.uninstall {
                        tr(self.lang, "Desinstalar o Neoveg")
                    } else {
                        tr(self.lang, "Instalar o Neoveg")
                    };
                    ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.into()));
                }
                if self.uninstall {
                    self.uninstall_fields(ui);
                } else {
                    self.install_fields(ui, running);
                }
                ui.add_space(8.0);
                self.status(ui);
                let w = (ui.available_width() - 3.0 * ui.spacing().item_spacing.x) / 4.0;
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.horizontal(|ui| self.footer(ui, ctx, w, running));
                });
            });
        if self.about {
            egui::Window::new(tr(lang, "Sobre"))
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.heading("Neoveg");
                    ui.label(tr_args(lang, "Versão {ver}", &[("ver", &ops::VERSION)]));
                    ui.label(tr(
                        lang,
                        "Editor de vídeo não-linear livre, escrito em Rust. Licença MIT OR Apache-2.0.",
                    ));
                    if ui.button(tr(lang, "Fechar")).clicked() {
                        self.about = false;
                    }
                });
        }
    }
}

impl SetupApp {
    fn install_fields(&mut self, ui: &mut egui::Ui, running: bool) {
        let lang = self.lang;
        ui.add_enabled_ui(!running && self.phase != Phase::Done, |ui| {
            field_label(ui, tr(lang, "Instalar para:"));
            let before = self.scope;
            egui::ComboBox::from_id_salt("setup_scope")
                .width(ui.available_width())
                .selected_text(scope_label(lang, self.scope))
                .show_ui(ui, |ui| {
                    for s in [Scope::User, Scope::AllUsers] {
                        ui.selectable_value(&mut self.scope, s, scope_label(lang, s));
                    }
                });
            if self.scope != before && !self.dir_edited {
                if let Some(d) = system::default_install_dir(self.scope) {
                    self.dir = d.display().to_string();
                }
            }
            field_label(ui, tr(lang, "Pasta de instalação:"));
            ui.horizontal(|ui| {
                let browse_w = 96.0;
                let edit =
                    ui.add(egui::TextEdit::singleline(&mut self.dir).desired_width(
                        ui.available_width() - browse_w - ui.spacing().item_spacing.x,
                    ));
                if edit.changed() {
                    self.dir_edited = true;
                }
                if ui
                    .add(egui::Button::new(tr(lang, "Procurar...")).min_size(vec2(browse_w, 0.0)))
                    .clicked()
                {
                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        self.dir = p.join("Neoveg").display().to_string();
                        self.dir_edited = true;
                    }
                }
            });
            field_label(ui, tr(lang, "Opções:"));
            ui.checkbox(
                &mut self.desktop,
                tr(lang, "Criar atalho na Área de Trabalho"),
            );
            ui.checkbox(
                &mut self.assoc,
                tr(lang, "Abrir arquivos .neoveg com o Neoveg"),
            );
        });
    }

    fn uninstall_fields(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        field_label(ui, tr(lang, "Pasta instalada:"));
        let shown = self
            .installed_dir
            .as_ref()
            .map(|d| d.display().to_string())
            .unwrap_or_else(|| tr(lang, "O Neoveg não está instalado nesta pasta.").into());
        ui.add(
            egui::TextEdit::singleline(&mut shown.clone())
                .interactive(false)
                .desired_width(ui.available_width()),
        );
        ui.add_enabled_ui(self.phase == Phase::Edit, |ui| {
            ui.checkbox(
                &mut self.remove_data,
                tr(
                    lang,
                    "Apagar também preferências, layouts e cópias de recuperação",
                ),
            );
        });
    }

    fn status(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        match &self.phase {
            Phase::Edit => {}
            Phase::Running => {
                let text = if self.uninstall {
                    tr(lang, "Removendo...")
                } else {
                    tr(lang, "Instalando...")
                };
                match self.progress() {
                    Some(f) => {
                        ui.add(egui::ProgressBar::new(f).text(text).show_percentage());
                    }
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(text);
                        });
                    }
                }
            }
            Phase::Done => {
                let text = if self.uninstall {
                    tr(lang, "O Neoveg foi removido.")
                } else {
                    tr(lang, "O Neoveg foi instalado.")
                };
                ui.label(RichText::new(text).color(Color32::from_rgb(120, 220, 140)));
            }
            Phase::Failed(e) => {
                ui.label(RichText::new(e.as_str()).color(Color32::from_rgb(255, 120, 110)));
            }
        }
    }

    fn footer(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, w: f32, running: bool) {
        let lang = self.lang;
        if self.uninstall {
            let can = self.installed_dir.is_some() && self.phase == Phase::Edit;
            if foot_button(ui, tr(lang, "Desinstalar"), w * 2.0, can) {
                self.start_uninstall();
            }
        } else if self.phase == Phase::Done {
            if foot_button(ui, tr(lang, "Abrir o Neoveg"), w * 2.0, true) {
                let exe = PathBuf::from(self.dir.trim()).join(APP_EXE);
                let _ = std::process::Command::new(exe).spawn();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        } else {
            if foot_button(ui, tr(lang, "Instalar"), w, !running) {
                self.start_install();
            }
            if foot_button(ui, tr(lang, "Padrão"), w, !running) {
                self.scope = Scope::User;
                self.dir = system::default_install_dir(Scope::User)
                    .map(|d| d.display().to_string())
                    .unwrap_or_default();
                self.dir_edited = false;
                self.desktop = true;
                self.assoc = true;
                self.phase = Phase::Edit;
            }
        }
        if foot_button(ui, tr(lang, "Sobre"), w, true) {
            self.about = true;
        }
        if foot_button(ui, tr(lang, "Sair"), w, !running) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn scope_label(lang: Language, s: Scope) -> &'static str {
    match s {
        Scope::User => tr(lang, "Só para mim (sem administrador)"),
        Scope::AllUsers => tr(lang, "Para todos os usuários (pede administrador)"),
    }
}
