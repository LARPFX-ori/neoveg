//! Tela de abertura (Splash Screen) vetorial nativa do Neoveg (`AGENTS.md` §1 / §3.6).
//! Toda a arte e o indicador animado são desenhados do zero com `egui::Painter`,
//! respeitando integralmente os 7 temas visuais (incluindo Alto Contraste WCAG AAA).

use crate::theme::{ThemeColors, ThemeId};
use eframe::egui;
use egui::{
    pos2, vec2, Align2, Color32, FontId, Key, Margin, Pos2, Rect, Sense, Shape, Stroke, StrokeKind,
};
use neoveg_common::prefs::LayoutType;
use neoveg_common::{tr, tr_args, Language};
use std::time::Instant;

const STARTUP_HOLD_SECS: f64 = 1.35;
/// Tamanho da janela da tela de abertura na inicialização.
pub(crate) const SPLASH_WINDOW_SIZE: [f32; 2] = [640.0, 388.0];
const STARTUP_MAX_WAIT_SECS: f64 = 6.0;

#[derive(Debug, Clone)]
pub struct SplashState {
    pub active: bool,
    pub auto_close: bool,
    started_at: Instant,
    elapsed_secs: f64,
}

impl SplashState {
    pub fn startup(show_on_startup: bool) -> Self {
        Self {
            active: show_on_startup,
            auto_close: show_on_startup,
            started_at: Instant::now(),
            elapsed_secs: 0.0,
        }
    }

    pub fn open_manual(&mut self) {
        self.active = true;
        self.auto_close = false;
        self.started_at = Instant::now();
        self.elapsed_secs = 0.0;
    }

    pub fn dismiss(&mut self) {
        self.active = false;
        self.auto_close = false;
    }

    pub fn update_tick(&mut self, media_loading: usize) -> bool {
        if !self.active {
            return false;
        }
        let wall = self.started_at.elapsed().as_secs_f64();
        if wall.is_finite() && wall > self.elapsed_secs {
            self.elapsed_secs = wall;
        }
        self.apply_auto_close(media_loading);
        self.active
    }

    #[cfg(test)]
    pub(crate) fn advance_for_test(&mut self, secs: f64, media_loading: usize) {
        if secs.is_finite() && secs >= 0.0 {
            self.elapsed_secs = secs;
        }
        self.apply_auto_close(media_loading);
    }

    fn apply_auto_close(&mut self, media_loading: usize) {
        if !self.auto_close {
            return;
        }
        let done_startup = self.elapsed_secs >= STARTUP_HOLD_SECS && media_loading == 0;
        let safety_timeout = self.elapsed_secs >= STARTUP_MAX_WAIT_SECS;
        if done_startup || safety_timeout {
            self.dismiss();
        }
    }

    pub fn progress(&self, media_loading: usize) -> f32 {
        if !self.auto_close {
            return 1.0;
        }
        let raw = (self.elapsed_secs / STARTUP_HOLD_SECS) as f32;
        let clamped = if raw.is_finite() {
            raw.clamp(0.05, 1.0)
        } else {
            1.0
        };
        if media_loading > 0 {
            clamped.min(0.88)
        } else {
            clamped
        }
    }

    pub fn stage_label(&self, snap: &SplashSnapshot<'_>) -> String {
        let lang = snap.lang;
        let cmds_s = snap.commands_count.to_string();
        if !self.auto_close {
            return tr_args(
                lang,
                "Pronto para editar · {cmds} ações na Tabela Única de Comandos",
                &[("cmds", &cmds_s)],
            );
        }
        if snap.loading > 0 && self.elapsed_secs >= 0.65 {
            let loading_s = snap.loading.to_string();
            return tr_args(
                lang,
                "Carregando {n} arquivo(s) de mídia em segundo plano...",
                &[("n", &loading_s)],
            );
        }
        if self.elapsed_secs < 0.28 {
            return tr_args(
                lang,
                "Carregando preferências e tema ({theme})...",
                &[("theme", &snap.theme_id.label_in(lang))],
            );
        }
        if self.elapsed_secs < 0.60 {
            let workers_s = snap.workers.to_string();
            let ram_s = snap.ram_preview_mb.to_string();
            let cap_s = snap.frame_cache_cap.to_string();
            return tr_args(
                lang,
                "Configurando {workers} worker(s) e cache ({mb} MB · {frames} quadros)...",
                &[("workers", &workers_s), ("mb", &ram_s), ("frames", &cap_s)],
            );
        }
        if self.elapsed_secs < 0.95 {
            let sr_s = snap.audio_sr.to_string();
            return tr_args(
                lang,
                "Inicializando motor de áudio ({dev} · {sr} Hz)...",
                &[("dev", &snap.audio_device), ("sr", &sr_s)],
            );
        }
        tr_args(
            lang,
            "Tabela Única de Comandos ({cmds} ações) · Pronto",
            &[("cmds", &cmds_s)],
        )
    }
}

pub struct SplashSnapshot<'a> {
    pub lang: Language,
    pub theme_id: ThemeId,
    pub theme: &'a ThemeColors,
    pub layout_type: LayoutType,
    pub audio_device: &'a str,
    pub audio_sr: u32,
    pub workers: usize,
    pub ram_preview_mb: u32,
    pub frame_cache_cap: usize,
    pub loading: usize,
    pub commands_count: usize,
    pub fallback_notice: Option<&'a str>,
    /// A tela de abertura ocupa a própria janela (inicialização): sem fundo
    /// escurecido e com o cartão do tamanho da janela.
    pub own_window: bool,
}

#[derive(Default, Clone, Copy, Debug)]
pub struct SplashOutcome {
    pub dismissed: bool,
    pub pref_changed: bool,
}

/// Emblema vetorial original do Neoveg: moldura de fotograma + monograma geométrico
/// com haste diagonal funcionando como cursor de timeline (playhead).
pub fn draw_neoveg_mark(painter: &egui::Painter, rect: Rect, theme: &ThemeColors) {
    if rect.width() <= 4.0 || rect.height() <= 4.0 {
        return;
    }
    let side = rect.width().min(rect.height());
    let sq = Rect::from_center_size(rect.center(), vec2(side, side));
    let pt = |nx: f32, ny: f32| pos2(sq.left() + nx * sq.width(), sq.top() + ny * sq.height());

    let badge_fill = theme.extreme_bg;
    let frame_border = if theme.high_contrast {
        theme.accent
    } else {
        theme.selection_bg
    };
    let bar_primary = theme.accent;
    let bar_secondary = if theme.high_contrast {
        theme.text_primary
    } else {
        theme.timecode_fg
    };
    let needle = if theme.high_contrast {
        theme.text_primary
    } else {
        theme.tl_playhead_live
    };

    painter.rect_filled(sq, side * 0.18, badge_fill);
    painter.rect_stroke(
        sq,
        side * 0.18,
        Stroke::new((side * 0.045).max(1.5), frame_border),
        StrokeKind::Inside,
    );

    // Pequenas perfurações laterais estilizadas de película / timeline
    let perf_w = side * 0.08;
    let perf_h = side * 0.06;
    for ny in [0.24_f32, 0.50, 0.76] {
        for nx in [0.11_f32, 0.89] {
            let r = Rect::from_center_size(pt(nx, ny), vec2(perf_w, perf_h));
            painter.rect_filled(r, 1.5_f32, theme.border);
        }
    }

    // Coluna esquerda e direita do monograma "N" em forma de barras de mídia
    let stroke_w = (side * 0.105).max(2.0);
    painter.line_segment(
        [pt(0.29, 0.76), pt(0.29, 0.25)],
        Stroke::new(stroke_w, bar_primary),
    );
    painter.line_segment(
        [pt(0.71, 0.76), pt(0.71, 0.25)],
        Stroke::new(stroke_w, bar_secondary),
    );

    // Diagonal do "N" combinada com ponta superior de agulha de reprodução (playhead)
    painter.line_segment(
        [pt(0.29, 0.27), pt(0.71, 0.74)],
        Stroke::new((side * 0.095).max(2.0), needle),
    );
    let head_pts = vec![pt(0.21, 0.17), pt(0.37, 0.17), pt(0.29, 0.28)];
    painter.add(Shape::convex_polygon(head_pts, needle, Stroke::NONE));
}

/// Indicador de carregamento vetorial original do Neoveg: 8 segmentos radiais
/// inspirados em uma roda de transporte / marcação de quadros em órbita contínua.
pub fn draw_original_spinner(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    phase_secs: f64,
    theme: &ThemeColors,
) {
    if !radius.is_finite() || radius <= 2.0 {
        return;
    }
    let phase = if phase_secs.is_finite() {
        phase_secs.rem_euclid(10_000.0) as f32
    } else {
        0.0
    };
    const SPOKES: usize = 8;
    let active_idx = ((phase * 9.0) as usize) % SPOKES;
    let inner = radius * 0.42;
    let outer = radius * 0.92;
    let stroke_w = (radius * 0.18).max(1.5);

    for i in 0..SPOKES {
        let angle =
            (i as f32 / SPOKES as f32) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        let dist_back = (active_idx + SPOKES - i) % SPOKES;
        let col = if theme.high_contrast {
            if dist_back <= 2 {
                theme.accent
            } else {
                theme.text_muted
            }
        } else {
            let weight = match dist_back {
                0 => 1.0,
                1 => 0.76,
                2 => 0.52,
                3 => 0.34,
                _ => 0.18,
            };
            if i % 2 == 0 {
                theme.accent.gamma_multiply(weight)
            } else {
                theme.timecode_fg.gamma_multiply(weight)
            }
        };
        let dir = vec2(angle.cos(), angle.sin());
        painter.line_segment(
            [center + dir * inner, center + dir * outer],
            Stroke::new(stroke_w, col),
        );
    }
    painter.circle_filled(center, (radius * 0.18).max(1.5), theme.text_primary);
}

/// Widget inline do indicador de carregamento vetorial original do Neoveg.
pub fn spinner_inline(ui: &mut egui::Ui, diameter: f32, phase_secs: f64, theme: &ThemeColors) {
    let d = if diameter.is_finite() {
        diameter.clamp(10.0, 64.0)
    } else {
        16.0
    };
    let (rect, _) = ui.allocate_exact_size(vec2(d, d), Sense::hover());
    if ui.is_rect_visible(rect) {
        draw_original_spinner(ui.painter(), rect.center(), d * 0.46, phase_secs, theme);
    }
}

fn dist_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = bx - ax;
    let dy = by - ay;
    let len_sq = dx * dx + dy * dy;
    if len_sq <= 1e-6 {
        return ((px - ax).powi(2) + (py - ay).powi(2)).sqrt();
    }
    let t = (((px - ax) * dx + (py - ay) * dy) / len_sq).clamp(0.0, 1.0);
    let cx = ax + t * dx;
    let cy = ay + t * dy;
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

fn sd_rounded_square(px: f32, py: f32, half: f32, radius: f32) -> f32 {
    let qx = (px - 15.5).abs() - (half - radius);
    let qy = (py - 15.5).abs() - (half - radius);
    let out = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let inside = qx.max(qy).min(0.0);
    out + inside - radius
}

fn blend_channel(dst: f32, src: f32, cov: f32) -> f32 {
    dst + (src - dst) * cov
}

fn f32_to_u8(v: f32) -> u8 {
    if v.is_finite() {
        v.round().clamp(0.0, 255.0) as u8
    } else {
        0
    }
}

/// Gera em código Rust puro o ícone original 32x32 da janela do Neoveg.
pub(crate) fn window_icon_data() -> egui::IconData {
    const SIDE: u32 = 32;
    let mut rgba = Vec::with_capacity((SIDE as usize) * (SIDE as usize) * 4);
    for y in 0..SIDE {
        let py = y as f32;
        for x in 0..SIDE {
            let px = x as f32;
            let sd_box = sd_rounded_square(px, py, 14.2, 5.2);
            let outer_cov = (0.75 - sd_box).clamp(0.0, 1.0);
            if outer_cov <= 0.0 {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }

            let grad = ((px + py) / 62.0).clamp(0.0, 1.0);
            let mut r = blend_channel(24.0, 14.0, grad);
            let mut g = blend_channel(34.0, 20.0, grad);
            let mut b = blend_channel(52.0, 32.0, grad);

            // Borda sutil azul no perímetro do emblema arredondado
            let border_cov = (1.0 - (sd_box + 0.85).abs()).clamp(0.0, 1.0);
            r = blend_channel(r, 72.0, border_cov * 0.90);
            g = blend_channel(g, 162.0, border_cov * 0.90);
            b = blend_channel(b, 245.0, border_cov * 0.90);

            // Hastes verticais do "N" em branco frio
            let d_left = dist_to_segment(px, py, 8.8, 22.2, 8.8, 9.8) - 1.65;
            let d_right = dist_to_segment(px, py, 23.2, 22.2, 23.2, 9.8) - 1.65;
            let pillar_cov = (0.75 - d_left.min(d_right)).clamp(0.0, 1.0);
            r = blend_channel(r, 238.0, pillar_cov);
            g = blend_channel(g, 245.0, pillar_cov);
            b = blend_channel(b, 255.0, pillar_cov);

            // Diagonal do "N" em ciano -> âmbar coral original do Neoveg
            let d_diag = dist_to_segment(px, py, 9.0, 10.0, 23.0, 22.0) - 1.75;
            let diag_cov = (0.75 - d_diag).clamp(0.0, 1.0);
            let diag_t = ((px - 9.0) / 14.0).clamp(0.0, 1.0);
            let dr = blend_channel(58.0, 255.0, diag_t);
            let dg = blend_channel(196.0, 124.0, diag_t);
            let db = blend_channel(255.0, 68.0, diag_t);
            r = blend_channel(r, dr, diag_cov);
            g = blend_channel(g, dg, diag_cov);
            b = blend_channel(b, db, diag_cov);

            rgba.push(f32_to_u8(r));
            rgba.push(f32_to_u8(g));
            rgba.push(f32_to_u8(b));
            rgba.push(f32_to_u8(outer_cov * 255.0));
        }
    }
    egui::IconData {
        rgba,
        width: SIDE,
        height: SIDE,
    }
}

fn draw_splash_hero(painter: &egui::Painter, banner: Rect, snap: &SplashSnapshot<'_>) {
    if banner.width() <= 8.0 || banner.height() <= 8.0 {
        return;
    }
    let theme = snap.theme;
    let hero_bg = if theme.high_contrast {
        theme.extreme_bg
    } else if theme.dark_mode {
        Color32::from_rgb(20, 24, 34)
    } else {
        Color32::from_rgb(228, 234, 244)
    };
    painter.rect_filled(banner, 0.0_f32, hero_bg);

    // Linhas decorativas sutis de timeline e onda sonora ao fundo (apenas em temas padrão)
    if !theme.high_contrast {
        let grid_col = theme.border.gamma_multiply(0.32);
        let step = 48.0_f32;
        let mut x = banner.left() + step;
        while x < banner.right() - 12.0 {
            painter.line_segment(
                [
                    pos2(x, banner.top() + 10.0),
                    pos2(x, banner.bottom() - 10.0),
                ],
                Stroke::new(1.0_f32, grid_col),
            );
            x += step;
        }
        let wave_col = theme.accent.gamma_multiply(0.22);
        let mid_y = banner.bottom() - 22.0;
        let mut wx = banner.left() + 18.0;
        let mut idx = 0usize;
        while wx < banner.right() - 18.0 {
            let amp = 4.0 + ((idx * 7) % 5) as f32 * 3.0;
            painter.line_segment(
                [pos2(wx, mid_y - amp), pos2(wx, mid_y + amp)],
                Stroke::new(2.0_f32, wave_col),
            );
            wx += 10.0;
            idx += 1;
        }
    }

    painter.line_segment(
        [banner.left_bottom(), banner.right_bottom()],
        Stroke::new(
            if theme.high_contrast {
                2.0_f32
            } else {
                1.0_f32
            },
            theme.accent,
        ),
    );

    let mark_side = (banner.height() - 36.0).clamp(48.0, 92.0);
    let mark_rect = Rect::from_min_size(
        pos2(banner.left() + 24.0, banner.center().y - mark_side * 0.5),
        vec2(mark_side, mark_side),
    );
    draw_neoveg_mark(painter, mark_rect, theme);

    let tx = mark_rect.right() + 20.0;
    let lang = snap.lang;
    painter.text(
        pos2(tx, banner.top() + 34.0),
        Align2::LEFT_CENTER,
        "NEOVEG",
        FontId::proportional(28.0),
        theme.text_primary,
    );
    painter.text(
        pos2(tx, banner.top() + 62.0),
        Align2::LEFT_CENTER,
        tr(lang, "Editor de Vídeo Não-Linear • Rust Puro"),
        FontId::proportional(13.5),
        theme.accent,
    );
    painter.text(
        pos2(tx, banner.top() + 86.0),
        Align2::LEFT_CENTER,
        tr_args(
            lang,
            "Versão {ver}  |  Semântica VEGAS Pro  |  MIT OR Apache-2.0",
            &[("ver", &env!("CARGO_PKG_VERSION"))],
        ),
        FontId::proportional(11.5),
        theme.text_muted,
    );

    let badge_text = if theme.high_contrast {
        tr_args(
            lang,
            "{label} · Alto Contraste AAA",
            &[("label", &snap.layout_type.label_in(lang))],
        )
    } else {
        tr_args(
            lang,
            "Layout {name} · {era}",
            &[
                ("name", &snap.layout_type.label_in(lang)),
                ("era", &snap.theme_id.label_in(lang)),
            ],
        )
    };
    painter.text(
        pos2(tx, banner.top() + 108.0),
        Align2::LEFT_CENTER,
        badge_text,
        FontId::monospace(11.0),
        theme.timecode_fg,
    );
}

fn draw_progress_bar(painter: &egui::Painter, rect: Rect, progress: f32, theme: &ThemeColors) {
    if rect.width() <= 4.0 || rect.height() <= 2.0 {
        return;
    }
    let p = if progress.is_finite() {
        progress.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let r = rect.height() * 0.5;
    painter.rect_filled(rect, r, theme.extreme_bg);
    painter.rect_stroke(
        rect,
        r,
        Stroke::new(
            if theme.high_contrast {
                1.5_f32
            } else {
                1.0_f32
            },
            theme.border,
        ),
        StrokeKind::Inside,
    );
    let fill_w = (rect.width() * p).max(if p > 0.0 { 6.0 } else { 0.0 });
    if fill_w > 0.0 {
        let fill_rect =
            Rect::from_min_size(rect.min, vec2(fill_w.min(rect.width()), rect.height()));
        painter.rect_filled(fill_rect, r, theme.accent);
    }
}

/// Renderiza a Splash Screen centralizada sobre o editor e devolve se foi dispensada
/// ou se o usuário alterou a preferência "Mostrar tela de abertura ao iniciar".
pub fn show_splash_screen(
    ctx: &egui::Context,
    state: &mut SplashState,
    show_on_startup: &mut bool,
    snap: &SplashSnapshot<'_>,
) -> SplashOutcome {
    let mut out = SplashOutcome::default();
    if !state.active {
        return out;
    }

    let close_key = ctx.input(|i| {
        i.key_pressed(Key::Escape)
            || i.key_pressed(Key::Enter)
            || (state.auto_close && i.key_pressed(Key::Space))
    });
    if close_key {
        state.dismiss();
        out.dismissed = true;
        return out;
    }

    let sr = ctx.screen_rect();
    let backdrop_col = if snap.theme.high_contrast {
        Color32::from_black_alpha(195)
    } else if snap.theme.dark_mode {
        Color32::from_black_alpha(150)
    } else {
        Color32::from_rgba_unmultiplied(18, 22, 30, 115)
    };

    if !snap.own_window {
        egui::Area::new(egui::Id::new("neoveg_splash_backdrop"))
            .fixed_pos(sr.min)
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                let resp = ui.allocate_response(sr.size(), Sense::click());
                ui.painter().rect_filled(sr, 0.0_f32, backdrop_col);
                if resp.clicked() {
                    state.dismiss();
                    out.dismissed = true;
                }
            });
    }

    if out.dismissed {
        return out;
    }

    let safe_w = if sr.width().is_finite() {
        sr.width() - 32.0
    } else {
        320.0
    };
    let safe_h = if sr.height().is_finite() {
        sr.height() - 32.0
    } else {
        260.0
    };
    let (card_w, card_h) = if snap.own_window && sr.width().is_finite() && sr.height().is_finite() {
        // o cartão é a janela inteira
        (sr.width().max(320.0), sr.height().max(260.0))
    } else {
        (safe_w.clamp(320.0, 640.0), safe_h.clamp(260.0, 388.0))
    };
    let border_w = if snap.theme.high_contrast {
        2.0_f32
    } else {
        1.2_f32
    };
    let card_frame = egui::Frame::window(&ctx.style())
        .fill(snap.theme.window_fill)
        .stroke(Stroke::new(border_w, snap.theme.accent))
        .inner_margin(Margin::ZERO);

    let mut close_clicked = false;
    egui::Window::new("neoveg_splash_card")
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .order(egui::Order::Foreground)
        .fixed_size(vec2(card_w, card_h))
        .frame(card_frame)
        .show(ctx, |ui| {
            let hero_h = 136.0_f32.min(card_h * 0.42);
            let (hero_rect, _) = ui.allocate_exact_size(vec2(card_w, hero_h), Sense::hover());
            draw_splash_hero(ui.painter(), hero_rect, snap);

            egui::Frame::NONE
                .inner_margin(Margin::symmetric(20, 14))
                .show(ui, |ui| {
                    let lang = snap.lang;
                    ui.spacing_mut().item_spacing.y = 6.0;
                    egui::Grid::new("splash_specs")
                        .num_columns(2)
                        .spacing([14.0, 5.0])
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(tr(lang, "Áudio:")).strong());
                            ui.label(format!("{} ({} Hz)", snap.audio_device, snap.audio_sr));
                            ui.end_row();

                            let workers_s = snap.workers.to_string();
                            let ram_s = snap.ram_preview_mb.to_string();
                            let cap_s = snap.frame_cache_cap.to_string();
                            ui.label(egui::RichText::new(tr(lang, "Processamento:")).strong());
                            ui.label(tr_args(
                                lang,
                                "{workers} worker(s) · Cache {mb} MB ({frames} quadros/mídia)",
                                &[("workers", &workers_s), ("mb", &ram_s), ("frames", &cap_s)],
                            ));
                            ui.end_row();

                            ui.label(egui::RichText::new(tr(lang, "Codecs Rust puro:")).strong());
                            ui.label(tr(
                                lang,
                                "WAV/AIFF (próprio), FLAC, OGG, MP3, AVI, Y4M, GIF",
                            ));
                            ui.end_row();

                            ui.label(egui::RichText::new(tr(lang, "Contêineres / UI:")).strong());
                            let os_codecs = if cfg!(windows) {
                                tr(lang, "Media Foundation (MP4/MOV/MKV/M4A/AAC/WMV)")
                            } else {
                                tr(lang, "Formatos em Rust puro (APIs Linux/macOS previstas)")
                            };
                            let cmds_s = snap.commands_count.to_string();
                            ui.label(tr_args(
                                lang,
                                "{codecs} · {cmds} comandos",
                                &[("codecs", &os_codecs), ("cmds", &cmds_s)],
                            ));
                            ui.end_row();
                        });

                    if let Some(notice) = snap.fallback_notice {
                        ui.label(
                            egui::RichText::new(notice)
                                .small()
                                .color(snap.theme.offline_fg),
                        );
                    }

                    ui.add_space(4.0);
                    ui.separator();

                    let prog = state.progress(snap.loading);
                    let stage = state.stage_label(snap);
                    ui.horizontal(|ui| {
                        let (spin_r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                        draw_original_spinner(
                            ui.painter(),
                            spin_r.center(),
                            9.5,
                            state.elapsed_secs,
                            snap.theme,
                        );
                        ui.label(egui::RichText::new(stage).small());
                    });

                    let bar_w = (card_w - 44.0).max(120.0);
                    let (bar_r, _) = ui.allocate_exact_size(vec2(bar_w, 8.0), Sense::hover());
                    draw_progress_bar(ui.painter(), bar_r, prog, snap.theme);

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui
                            .checkbox(
                                show_on_startup,
                                tr(lang, "Mostrar tela de abertura ao iniciar"),
                            )
                            .changed()
                        {
                            out.pref_changed = true;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let btn_label = if state.auto_close {
                                tr(lang, "Continuar")
                            } else {
                                tr(lang, "Fechar")
                            };
                            if ui.button(btn_label).clicked() {
                                close_clicked = true;
                            }
                        });
                    });
                });
        });

    if close_clicked {
        state.dismiss();
        out.dismissed = true;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::palette;

    fn sample_snap(theme: &ThemeColors, loading: usize) -> SplashSnapshot<'_> {
        SplashSnapshot {
            lang: Language::PtBr,
            theme_id: ThemeId::VegasEscuro,
            theme,
            layout_type: LayoutType::Magix,
            audio_device: "Sem dispositivo",
            audio_sr: 48_000,
            workers: 2,
            ram_preview_mb: 256,
            frame_cache_cap: 48,
            loading,
            commands_count: 68,
            fallback_notice: None,
            own_window: false,
        }
    }

    #[test]
    fn splash_startup_e_fechamento_automatico_sem_travamento() {
        let off = SplashState::startup(false);
        assert!(!off.active);

        let mut on = SplashState::startup(true);
        assert!(on.active);
        assert!(on.auto_close);
        let th = palette(ThemeId::VegasEscuro);
        let s_loading = sample_snap(&th, 2);

        on.advance_for_test(1.5, 2);
        assert!(
            on.active,
            "não deve fechar enquanto mídia inicial ainda está carregando"
        );
        assert!(on.progress(2) < 0.90);
        assert!(on.stage_label(&s_loading).contains("mídia"));

        on.advance_for_test(1.5, 0);
        assert!(
            !on.active,
            "deve fechar após tempo mínimo com carga concluída"
        );

        on.open_manual();
        assert!(on.active);
        assert!(!on.auto_close);
        on.advance_for_test(120.0, 0);
        assert!(
            on.active,
            "abertura manual permanece aberta até o usuário fechar"
        );
        on.dismiss();
        assert!(!on.active);
    }

    #[test]
    fn splash_desenha_em_todos_os_temas_e_dimensoes_hostis() {
        let ctx = egui::Context::default();
        let mut show_pref = true;
        for tid in ThemeId::ALL {
            let th = palette(tid);
            let mut st = SplashState::startup(true);
            let mut snap = sample_snap(&th, 0);
            snap.theme_id = tid;
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 720.0))),
                ..Default::default()
            };
            let _ = ctx.run(raw, |c| {
                let out = show_splash_screen(c, &mut st, &mut show_pref, &snap);
                assert!(!out.dismissed);
                egui::CentralPanel::default().show(c, |ui| {
                    draw_neoveg_mark(ui.painter(), Rect::ZERO, &th);
                    draw_original_spinner(ui.painter(), Pos2::ZERO, f32::NAN, f64::INFINITY, &th);
                    draw_progress_bar(
                        ui.painter(),
                        Rect::from_min_size(Pos2::ZERO, vec2(1.0, 1.0)),
                        f32::NAN,
                        &th,
                    );
                });
            });
        }
    }

    #[test]
    fn icone_da_janela_gerado_em_rust_e_valido() {
        let icon = window_icon_data();
        assert_eq!(icon.width, 32);
        assert_eq!(icon.height, 32);
        assert_eq!(icon.rgba.len(), 32 * 32 * 4);
        // Canto (0, 0) é transparente pela borda arredondada; centro (16, 16) é opaco.
        assert_eq!(icon.rgba[3], 0);
        let center_idx = (16 * 32 + 16) * 4;
        assert_eq!(icon.rgba[center_idx + 3], 255);
    }
}
