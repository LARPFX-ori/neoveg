//! Tela de boas-vindas (Welcome Screen) nativa do Neoveg em Rust puro:
//! modelos rápidos de projeto, ações iniciais, lista persistida de projetos recentes
//! e adaptação integral aos 7 temas (AGENTS.md §0, §1, §2.2 e §3.6).

use super::App;
use crate::icons::{self, Icon};
use crate::model::{FrameRate, Project};
use crate::splash::draw_neoveg_mark;
use eframe::egui;
use egui::{vec2, Align2, Color32, Margin, RichText, Stroke, StrokeKind};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
struct QuickTemplate {
    title: &'static str,
    subtitle: &'static str,
    width: u32,
    height: u32,
    fps: FrameRate,
}

const QUICK_TEMPLATES: &[QuickTemplate] = &[
    QuickTemplate {
        title: "HD 1080p • 29,97 qps",
        subtitle: "1920×1080 • Broadcast NTSC / YouTube",
        width: 1920,
        height: 1080,
        fps: FrameRate::FPS_29_97,
    },
    QuickTemplate {
        title: "HD 1080p • 60 qps",
        subtitle: "1920×1080 • Fluidez alta",
        width: 1920,
        height: 1080,
        fps: FrameRate::FPS_60,
    },
    QuickTemplate {
        title: "4K UHD • 29,97 qps",
        subtitle: "3840×2160 • Masterização",
        width: 3840,
        height: 2160,
        fps: FrameRate::FPS_29_97,
    },
    QuickTemplate {
        title: "Vertical 9:16 • 30 qps",
        subtitle: "1080×1920 • Shorts / Reels",
        width: 1080,
        height: 1920,
        fps: FrameRate::FPS_30,
    },
];

enum WelcomeAction {
    None,
    NewBlank,
    NewTemplate(QuickTemplate),
    OpenDialog,
    ImportMedia,
    OpenRecent(PathBuf),
    RemoveRecent(String),
    ClearRecents,
    RecoverSession,
    OpenLayoutProps,
    OpenPrefs,
    Close,
}

impl App {
    pub fn is_welcome_open(&self) -> bool {
        self.welcome_open
    }

    pub fn open_welcome(&mut self) {
        self.welcome_open = true;
    }

    pub fn dismiss_welcome(&mut self) {
        self.welcome_open = false;
    }

    pub fn new_project_with_preset(&mut self, width: u32, height: u32, fps: FrameRate) {
        self.stop(false);
        let mut p = Project::new_default();
        p.settings.width = width.clamp(16, 8192);
        p.settings.height = height.clamp(16, 8192);
        p.settings.fps = fps;
        self.session.reset_project(p, None);
        self.thumbs.clear();
        self.set_status(format!(
            "Novo projeto: {}×{} @ {} qps",
            self.project.settings.width,
            self.project.settings.height,
            self.project.settings.fps.label()
        ));
    }

    pub(crate) fn show_welcome_overlay(&mut self, ctx: &egui::Context) {
        if !self.welcome_open || self.splash.active || self.recovery_offer {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.welcome_open = false;
            return;
        }

        let sr = ctx.screen_rect();
        let backdrop_col = if self.theme.high_contrast {
            Color32::from_black_alpha(230)
        } else if self.theme.dark_mode {
            Color32::from_rgba_unmultiplied(10, 12, 16, 175)
        } else {
            Color32::from_rgba_unmultiplied(24, 30, 42, 130)
        };

        let mut action = WelcomeAction::None;
        egui::Area::new(egui::Id::new("neoveg_welcome_backdrop"))
            .fixed_pos(sr.min)
            .order(egui::Order::Middle)
            .interactable(true)
            .show(ctx, |ui| {
                let resp = ui.allocate_response(sr.size(), egui::Sense::click());
                ui.painter().rect_filled(sr, 0.0_f32, backdrop_col);
                if resp.clicked() {
                    action = WelcomeAction::Close;
                }
            });

        if matches!(action, WelcomeAction::Close) {
            self.welcome_open = false;
            return;
        }

        let safe_w = if sr.width().is_finite() {
            sr.width() - 40.0
        } else {
            680.0
        };
        let safe_h = if sr.height().is_finite() {
            sr.height() - 48.0
        } else {
            460.0
        };
        let card_w = safe_w.clamp(360.0, 820.0);
        let card_h = safe_h.clamp(300.0, 520.0);
        let border_w = if self.theme.high_contrast {
            2.0_f32
        } else {
            1.2_f32
        };
        let frame = egui::Frame::window(&ctx.style())
            .fill(self.theme.window_fill)
            .stroke(Stroke::new(border_w, self.theme.accent))
            .inner_margin(Margin::symmetric(20, 16));

        let workers = self.prefs.effective_worker_threads();
        let recents = self.prefs.recent_projects.clone();
        let has_recovery = crate::recovery::find_existing_recovery().is_some();
        let mut show_on_startup = self.prefs.show_welcome;
        let show_before = show_on_startup;
        let theme = self.theme.clone();
        let layout_label = self.prefs.edit_layout.layout_type.label();

        egui::Window::new("neoveg_welcome_card")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Middle)
            .fixed_size(vec2(card_w, card_h))
            .frame(frame)
            .show(ctx, |ui| {
                // Cabeçalho com marca original do Neoveg
                ui.horizontal(|ui| {
                    let (mark_rect, _) =
                        ui.allocate_exact_size(vec2(42.0, 42.0), egui::Sense::hover());
                    draw_neoveg_mark(ui.painter(), mark_rect, &theme);
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("NEOVEG")
                                    .size(20.0)
                                    .strong()
                                    .color(theme.text_primary),
                            );
                            ui.label(
                                RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                    .small()
                                    .color(theme.accent),
                            );
                        });
                        ui.label(
                            RichText::new(format!(
                                "Layout: {}  •  {} worker(s)  •  Rust puro",
                                layout_label, workers
                            ))
                            .small()
                            .color(theme.text_muted),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::small_icon_button(ui, Icon::CloseX, None, false, &theme)
                            .on_hover_text("Fechar (Esc)")
                            .clicked()
                        {
                            action = WelcomeAction::Close;
                        }
                    });
                });

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                // Corpo em 2 colunas: Iniciar/Modelos à esquerda, Projetos Recentes à direita
                let footer_h = 34.0;
                let body_h = (ui.available_height() - footer_h - 10.0).max(160.0);
                let left_w = (ui.available_width() * 0.44).clamp(180.0, 340.0);
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), body_h),
                    egui::Layout::left_to_right(egui::Align::Min),
                    |ui| {
                        ui.allocate_ui_with_layout(
                            vec2(left_w, body_h),
                            egui::Layout::top_down(egui::Align::LEFT),
                            |ui| {
                                ui.label(RichText::new("Começar").strong());
                                ui.add_space(4.0);
                                if icons::icon_button(
                                    ui,
                                    Icon::NewFile,
                                    Some("Novo projeto em branco"),
                                    true,
                                    true,
                                    &theme,
                                )
                                .clicked()
                                {
                                    action = WelcomeAction::NewBlank;
                                }
                                ui.horizontal_wrapped(|ui| {
                                    if icons::icon_button(
                                        ui,
                                        Icon::OpenFolder,
                                        Some("Abrir projeto..."),
                                        false,
                                        true,
                                        &theme,
                                    )
                                    .clicked()
                                    {
                                        action = WelcomeAction::OpenDialog;
                                    }
                                    if icons::icon_button(
                                        ui,
                                        Icon::ImportMedia,
                                        Some("Importar mídia..."),
                                        false,
                                        true,
                                        &theme,
                                    )
                                    .clicked()
                                    {
                                        action = WelcomeAction::ImportMedia;
                                    }
                                });
                                if has_recovery
                                    && ui
                                        .button("Recuperar sessão anterior...")
                                        .clicked()
                                {
                                    action = WelcomeAction::RecoverSession;
                                }

                                ui.add_space(8.0);
                                ui.label(RichText::new("Modelos rápidos").strong());
                                ui.add_space(2.0);
                                for tmpl in QUICK_TEMPLATES {
                                    let btn = ui.add_sized(
                                        [left_w - 8.0, 32.0],
                                        egui::Button::new(format!(
                                            "{}\n{}",
                                            tmpl.title, tmpl.subtitle
                                        )),
                                    );
                                    if btn.clicked() {
                                        action = WelcomeAction::NewTemplate(*tmpl);
                                    }
                                }

                                ui.add_space(6.0);
                                ui.horizontal_wrapped(|ui| {
                                    if icons::small_icon_button(
                                        ui,
                                        Icon::LayoutWindow,
                                        Some("Layout..."),
                                        false,
                                        &theme,
                                    )
                                    .clicked()
                                    {
                                        action = WelcomeAction::OpenLayoutProps;
                                    }
                                    if icons::small_icon_button(
                                        ui,
                                        Icon::GearProps,
                                        Some("Preferências..."),
                                        false,
                                        &theme,
                                    )
                                    .clicked()
                                    {
                                        action = WelcomeAction::OpenPrefs;
                                    }
                                });
                            },
                        );

                        ui.separator();

                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Projetos recentes").strong());
                                if !recents.is_empty() {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.small_button("Limpar lista").clicked() {
                                                action = WelcomeAction::ClearRecents;
                                            }
                                        },
                                    );
                                }
                            });
                            ui.add_space(4.0);
                            if recents.is_empty() {
                                ui.group(|ui| {
                                    ui.label(
                                        RichText::new(
                                            "Nenhum projeto recente registrado ainda.",
                                        )
                                        .color(theme.text_muted),
                                    );
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(
                                            "Dicas rápidas:\n• Ctrl+O abre projetos .neoveg / .nvp\n• Ctrl+I importa áudio, vídeo ou imagem\n• S divide eventos no cursor; M marca; R cria região",
                                        )
                                        .small()
                                        .weak(),
                                    );
                                });
                            } else {
                                egui::ScrollArea::vertical()
                                    .id_salt("welcome_recents_scroll")
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        for raw in &recents {
                                            let p = Path::new(raw);
                                            let exists = p.exists();
                                            let name = p
                                                .file_name()
                                                .map(|s| s.to_string_lossy().to_string())
                                                .unwrap_or_else(|| raw.clone());
                                            let dir_str = p
                                                .parent()
                                                .map(|d| d.to_string_lossy().to_string())
                                                .unwrap_or_default();

                                            let box_resp = ui.group(|ui| {
                                                ui.horizontal(|ui| {
                                                    ui.vertical(|ui| {
                                                        let title_rt = if exists {
                                                            RichText::new(&name).strong()
                                                        } else {
                                                            RichText::new(format!(
                                                                "{name} (ausente)"
                                                            ))
                                                            .color(theme.text_muted)
                                                        };
                                                        if ui
                                                            .add(
                                                                egui::Button::new(title_rt)
                                                                    .frame(false),
                                                            )
                                                            .clicked()
                                                        {
                                                            action = WelcomeAction::OpenRecent(
                                                                p.to_path_buf(),
                                                            );
                                                        }
                                                        ui.label(
                                                            RichText::new(dir_str).small().weak(),
                                                        );
                                                    });
                                                    ui.with_layout(
                                                        egui::Layout::right_to_left(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            if icons::small_icon_button(
                                                                ui,
                                                                Icon::CloseX,
                                                                None,
                                                                false,
                                                                &theme,
                                                            )
                                                            .on_hover_text(
                                                                "Remover dos recentes",
                                                            )
                                                            .clicked()
                                                            {
                                                                action =
                                                                    WelcomeAction::RemoveRecent(
                                                                        raw.clone(),
                                                                    );
                                                            }
                                                            if ui
                                                                .add_enabled(
                                                                    exists,
                                                                    egui::Button::new("Abrir")
                                                                        .small(),
                                                                )
                                                                .clicked()
                                                            {
                                                                action =
                                                                    WelcomeAction::OpenRecent(
                                                                        p.to_path_buf(),
                                                                    );
                                                            }
                                                        },
                                                    );
                                                });
                                            });
                                            let _ = (box_resp, StrokeKind::Inside);
                                        }
                                    });
                            }
                        });
                    },
                );

                ui.separator();
                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut show_on_startup,
                        "Mostrar tela de boas-vindas ao iniciar",
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Ir para a timeline").clicked() {
                            action = WelcomeAction::Close;
                        }
                    });
                });
            });

        if show_on_startup != show_before {
            self.prefs.show_welcome = show_on_startup;
            if let Err(e) = self.prefs.save() {
                self.set_status(format!("Aviso ao salvar preferência: {e}"));
            }
        }

        match action {
            WelcomeAction::None => {}
            WelcomeAction::Close => {
                self.welcome_open = false;
            }
            WelcomeAction::NewBlank => {
                self.new_project();
                self.welcome_open = false;
            }
            WelcomeAction::NewTemplate(t) => {
                self.new_project_with_preset(t.width, t.height, t.fps);
                self.welcome_open = false;
            }
            WelcomeAction::OpenDialog => {
                self.welcome_open = false;
                self.open_project_dialog();
            }
            WelcomeAction::ImportMedia => {
                self.welcome_open = false;
                self.import_dialog();
            }
            WelcomeAction::OpenRecent(path) => {
                self.welcome_open = false;
                self.open_project_path(path);
            }
            WelcomeAction::RemoveRecent(raw) => {
                self.prefs.remove_recent_project(&raw);
                if let Err(e) = self.prefs.save() {
                    self.set_status(format!("Aviso ao atualizar recentes: {e}"));
                }
            }
            WelcomeAction::ClearRecents => {
                self.prefs.clear_recent_projects();
                if let Err(e) = self.prefs.save() {
                    self.set_status(format!("Aviso ao limpar recentes: {e}"));
                }
            }
            WelcomeAction::RecoverSession => {
                if let Some(p) = crate::recovery::find_existing_recovery() {
                    self.welcome_open = false;
                    self.open_project_path(p);
                    self.path = None;
                    self.dirty = true;
                    self.set_status("Projeto recuperado. Salve-o com Salvar como...");
                }
            }
            WelcomeAction::OpenLayoutProps => {
                self.welcome_open = false;
                self.dlg
                    .open_edit_layout(&self.prefs, self.dock_height, self.preview_ratio);
            }
            WelcomeAction::OpenPrefs => {
                self.welcome_open = false;
                self.dlg.open_prefs(&self.prefs);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeId;
    use egui::{Modifiers, RawInput};

    #[test]
    fn welcome_screen_desenha_em_todos_os_temas_sem_panico() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(&ctx, None);
        app.dismiss_splash();
        app.prefs
            .record_recent_project(Path::new("C:/projetos/exemplo_neoveg.neoveg"));
        app.open_welcome();

        for (i, tid) in ThemeId::ALL.into_iter().enumerate() {
            app.set_theme(&ctx, tid, false);
            let input = RawInput {
                time: Some((i as f64) * 0.1),
                modifiers: Modifiers::NONE,
                ..Default::default()
            };
            let _ = ctx.run(input, |c| {
                app.show_welcome_overlay(c);
            });
            assert!(app.is_welcome_open());
        }

        app.new_project_with_preset(1080, 1920, FrameRate::FPS_30);
        assert_eq!(app.project.settings.width, 1080);
        assert_eq!(app.project.settings.height, 1920);
        app.dismiss_welcome();
        assert!(!app.is_welcome_open());
    }
}
