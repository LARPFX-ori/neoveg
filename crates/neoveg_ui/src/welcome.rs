//! Tela de boas-vindas (Welcome Screen) nativa do Neoveg em Rust puro:
//! barra lateral com Novo Projeto / Abrir Projeto / Primeiros Passos,
//! banner vetorial original de cinema (Clean Room), seletor de proporção
//! em 5 formatos, resolução/taxa de quadros (incluindo Drop Frame) e projetos recentes
//! com adaptação integral aos 7 temas (AGENTS.md §0, §1, §2.2 e §3.6).

use super::App;
use crate::icons::{self, Icon};
use crate::model::{FrameRate, Project};
use crate::splash::draw_neoveg_mark;
use crate::theme::{contrast_on, ThemeColors};
use eframe::egui;
use egui::{
    pos2, vec2, Align2, Color32, CornerRadius, FontId, Margin, Rect, RichText, Sense, Shape,
    Stroke, StrokeKind, Vec2,
};
use neoveg_common::{tr, tr_args, Language};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WelcomeSection {
    #[default]
    NewProject,
    OpenProject,
    GettingStarted,
}

impl WelcomeSection {
    pub const ALL: [WelcomeSection; 3] = [
        WelcomeSection::NewProject,
        WelcomeSection::OpenProject,
        WelcomeSection::GettingStarted,
    ];

    pub fn label_in(self, lang: Language) -> &'static str {
        let key = match self {
            WelcomeSection::NewProject => "Novo Projeto",
            WelcomeSection::OpenProject => "Abrir Projeto",
            WelcomeSection::GettingStarted => "Primeiros Passos",
        };
        tr(lang, key)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WelcomeAspect {
    #[default]
    Widescreen16_9,
    Fullscreen4_3,
    Portrait9_16,
    Square1_1,
    Scope240_1,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WelcomeResolution {
    pub label: &'static str,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WelcomeFps {
    pub label: &'static str,
    pub fps: FrameRate,
    pub default_drop_frame: bool,
}

const RES_16_9: &[WelcomeResolution] = &[
    WelcomeResolution {
        label: "FullHD (1920 x 1080)",
        width: 1920,
        height: 1080,
    },
    WelcomeResolution {
        label: "4K UHD (3840 x 2160)",
        width: 3840,
        height: 2160,
    },
    WelcomeResolution {
        label: "HD (1280 x 720)",
        width: 1280,
        height: 720,
    },
    WelcomeResolution {
        label: "8K UHD (7680 x 4320)",
        width: 7680,
        height: 4320,
    },
];

const RES_4_3: &[WelcomeResolution] = &[
    WelcomeResolution {
        label: "HDV / 4:3 (1440 x 1080)",
        width: 1440,
        height: 1080,
    },
    WelcomeResolution {
        label: "XGA (1024 x 768)",
        width: 1024,
        height: 768,
    },
    WelcomeResolution {
        label: "SVGA (800 x 600)",
        width: 800,
        height: 600,
    },
    WelcomeResolution {
        label: "VGA / SD (640 x 480)",
        width: 640,
        height: 480,
    },
];

const RES_9_16: &[WelcomeResolution] = &[
    WelcomeResolution {
        label: "FullHD Vertical (1080 x 1920)",
        width: 1080,
        height: 1920,
    },
    WelcomeResolution {
        label: "4K Vertical (2160 x 3840)",
        width: 2160,
        height: 3840,
    },
    WelcomeResolution {
        label: "HD Vertical (720 x 1280)",
        width: 720,
        height: 1280,
    },
];

const RES_1_1: &[WelcomeResolution] = &[
    WelcomeResolution {
        label: "Full Quadrado (1080 x 1080)",
        width: 1080,
        height: 1080,
    },
    WelcomeResolution {
        label: "4K Quadrado (2160 x 2160)",
        width: 2160,
        height: 2160,
    },
    WelcomeResolution {
        label: "HD Quadrado (720 x 720)",
        width: 720,
        height: 720,
    },
];

const RES_SCOPE: &[WelcomeResolution] = &[
    WelcomeResolution {
        label: "2K Scope (2048 x 858)",
        width: 2048,
        height: 858,
    },
    WelcomeResolution {
        label: "4K DCI Scope (4096 x 1716)",
        width: 4096,
        height: 1716,
    },
    WelcomeResolution {
        label: "FullHD Scope (1920 x 800)",
        width: 1920,
        height: 800,
    },
];

const WELCOME_FPS: &[WelcomeFps] = &[
    WelcomeFps {
        label: "24p (23,976)",
        fps: FrameRate::FPS_23_976,
        default_drop_frame: false,
    },
    WelcomeFps {
        label: "24p Cinema (24,000)",
        fps: FrameRate::FPS_24,
        default_drop_frame: false,
    },
    WelcomeFps {
        label: "25p PAL (25,000)",
        fps: FrameRate::FPS_25,
        default_drop_frame: false,
    },
    WelcomeFps {
        label: "30p (29,970)",
        fps: FrameRate::FPS_29_97,
        default_drop_frame: true,
    },
    WelcomeFps {
        label: "30p (30,000)",
        fps: FrameRate::FPS_30,
        default_drop_frame: false,
    },
    WelcomeFps {
        label: "50p (50,000)",
        fps: FrameRate::FPS_50,
        default_drop_frame: false,
    },
    WelcomeFps {
        label: "60p (59,940)",
        fps: FrameRate::FPS_59_94,
        default_drop_frame: true,
    },
    WelcomeFps {
        label: "60p (60,000)",
        fps: FrameRate::FPS_60,
        default_drop_frame: false,
    },
];

impl WelcomeAspect {
    pub const ALL: [WelcomeAspect; 5] = [
        WelcomeAspect::Widescreen16_9,
        WelcomeAspect::Fullscreen4_3,
        WelcomeAspect::Portrait9_16,
        WelcomeAspect::Square1_1,
        WelcomeAspect::Scope240_1,
    ];

    pub fn label_in(self, lang: Language) -> &'static str {
        let key = match self {
            WelcomeAspect::Widescreen16_9 => "Widescreen (16:9)",
            WelcomeAspect::Fullscreen4_3 => "Fullscreen (4:3)",
            WelcomeAspect::Portrait9_16 => "Portrait (9:16)",
            WelcomeAspect::Square1_1 => "Square (1:1)",
            WelcomeAspect::Scope240_1 => "Scope (2.4:1)",
        };
        tr(lang, key)
    }

    pub fn short_label(self) -> &'static str {
        match self {
            WelcomeAspect::Widescreen16_9 => "16:9",
            WelcomeAspect::Fullscreen4_3 => "4:3",
            WelcomeAspect::Portrait9_16 => "9:16",
            WelcomeAspect::Square1_1 => "1:1",
            WelcomeAspect::Scope240_1 => "2.4:1",
        }
    }

    pub fn resolutions(self) -> &'static [WelcomeResolution] {
        match self {
            WelcomeAspect::Widescreen16_9 => RES_16_9,
            WelcomeAspect::Fullscreen4_3 => RES_4_3,
            WelcomeAspect::Portrait9_16 => RES_9_16,
            WelcomeAspect::Square1_1 => RES_1_1,
            WelcomeAspect::Scope240_1 => RES_SCOPE,
        }
    }
}

#[derive(Clone, Debug)]
pub struct WelcomeState {
    pub section: WelcomeSection,
    pub aspect: WelcomeAspect,
    pub res_idx: usize,
    pub fps_idx: usize,
    pub drop_frame: bool,
}

impl Default for WelcomeState {
    fn default() -> Self {
        Self {
            section: WelcomeSection::NewProject,
            aspect: WelcomeAspect::Widescreen16_9,
            res_idx: 0,
            fps_idx: 3, // 30p (29,970)
            drop_frame: true,
        }
    }
}

impl WelcomeState {
    pub fn select_aspect(&mut self, aspect: WelcomeAspect) {
        if self.aspect != aspect {
            self.aspect = aspect;
            self.res_idx = 0;
        }
    }

    pub fn current_resolution(&self) -> WelcomeResolution {
        let list = self.aspect.resolutions();
        list.get(self.res_idx)
            .copied()
            .or_else(|| list.first().copied())
            .unwrap_or(WelcomeResolution {
                label: "FullHD (1920 x 1080)",
                width: 1920,
                height: 1080,
            })
    }

    pub fn current_fps(&self) -> WelcomeFps {
        WELCOME_FPS
            .get(self.fps_idx)
            .copied()
            .or_else(|| WELCOME_FPS.first().copied())
            .unwrap_or(WelcomeFps {
                label: "30p (29,970)",
                fps: FrameRate::FPS_29_97,
                default_drop_frame: true,
            })
    }

    pub fn select_fps(&mut self, idx: usize) {
        if let Some(item) = WELCOME_FPS.get(idx) {
            self.fps_idx = idx;
            self.drop_frame = item.default_drop_frame;
        }
    }
}

enum WelcomeAction {
    None,
    CreateProject {
        width: u32,
        height: u32,
        fps: FrameRate,
        drop_frame: bool,
        open_advanced: bool,
    },
    OpenDialog,
    ImportMedia,
    OpenRecent(PathBuf),
    RemoveRecent(String),
    ClearRecents,
    RecoverSession,
    OpenLayoutProps,
    OpenPrefs,
    OpenShortcuts,
    Close,
}

fn draw_nav_icon(painter: &egui::Painter, rect: Rect, sec: WelcomeSection, col: Color32) {
    let side = rect.width().min(rect.height());
    let r = Rect::from_center_size(rect.center(), vec2(side, side));
    let pt = |nx: f32, ny: f32| pos2(r.left() + nx * r.width(), r.top() + ny * r.height());
    let stroke = Stroke::new((side * 0.065).max(1.2), col);

    match sec {
        WelcomeSection::NewProject => {
            let fold = 0.24;
            let body = vec![
                pt(0.24, 0.14),
                pt(1.0 - fold - 0.18, 0.14),
                pt(0.76, 0.14 + fold),
                pt(0.76, 0.86),
                pt(0.24, 0.86),
            ];
            painter.add(Shape::convex_polygon(body, col, Stroke::NONE));
            let crease = Color32::from_rgba_unmultiplied(
                col.r().saturating_sub(45),
                col.g().saturating_sub(45),
                col.b().saturating_sub(35),
                210,
            );
            painter.add(Shape::convex_polygon(
                vec![
                    pt(1.0 - fold - 0.18, 0.14),
                    pt(0.76, 0.14 + fold),
                    pt(1.0 - fold - 0.18, 0.14 + fold),
                ],
                crease,
                Stroke::NONE,
            ));
        }
        WelcomeSection::OpenProject => {
            painter.rect_filled(
                Rect::from_min_max(pt(0.14, 0.22), pt(0.48, 0.36)),
                CornerRadius::same(2),
                col,
            );
            painter.rect_filled(
                Rect::from_min_max(pt(0.14, 0.32), pt(0.86, 0.80)),
                CornerRadius::same(2),
                col,
            );
        }
        WelcomeSection::GettingStarted => {
            let brim = vec![
                pt(0.50, 0.20),
                pt(0.88, 0.38),
                pt(0.50, 0.56),
                pt(0.12, 0.38),
            ];
            painter.add(Shape::convex_polygon(brim, col, Stroke::NONE));
            let crown = vec![
                pt(0.28, 0.50),
                pt(0.50, 0.60),
                pt(0.72, 0.50),
                pt(0.72, 0.72),
                pt(0.50, 0.80),
                pt(0.28, 0.72),
            ];
            painter.add(Shape::convex_polygon(crown, col, Stroke::NONE));
            painter.line_segment([pt(0.84, 0.40), pt(0.84, 0.68)], stroke);
            painter.circle_filled(pt(0.84, 0.72), (side * 0.055).max(1.8), col);
        }
    }
}

fn draw_nav_tile(
    ui: &mut egui::Ui,
    size: Vec2,
    sec: WelcomeSection,
    active: bool,
    theme: &ThemeColors,
    lang: Language,
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let label = sec.label_in(lang);
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let bg = if active {
        theme.selection_bg
    } else if resp.hovered() {
        theme.widget_hover_bg
    } else {
        Color32::TRANSPARENT
    };
    let fg = if active {
        contrast_on(bg)
    } else {
        theme.text_primary
    };
    let p = ui.painter_at(rect);
    if bg != Color32::TRANSPARENT {
        p.rect_filled(rect, 0.0_f32, bg);
    }
    if active && theme.high_contrast {
        p.rect_stroke(
            rect.shrink(1.0),
            0.0_f32,
            Stroke::new(2.0_f32, theme.accent),
            StrokeKind::Inside,
        );
    }
    let icon_h = (rect.height() * 0.44).clamp(18.0, 30.0);
    let icon_rect = Rect::from_center_size(
        pos2(rect.center().x, rect.top() + rect.height() * 0.36),
        vec2(icon_h, icon_h),
    );
    draw_nav_icon(&p, icon_rect, sec, fg);
    p.text(
        pos2(rect.center().x, rect.bottom() - rect.height() * 0.22),
        Align2::CENTER_CENTER,
        sec.label_in(lang),
        FontId::proportional(if size.x < 92.0 { 10.5 } else { 11.5 }),
        fg,
    );
    resp.clicked()
}

fn draw_aspect_glyph(painter: &egui::Painter, rect: Rect, aspect: WelcomeAspect, col: Color32) {
    let frame_rect = match aspect {
        WelcomeAspect::Widescreen16_9 => Rect::from_center_size(
            pos2(rect.center().x, rect.center().y - 2.0),
            vec2(rect.width() * 0.86, rect.height() * 0.56),
        ),
        WelcomeAspect::Fullscreen4_3 => Rect::from_center_size(
            rect.center(),
            vec2(rect.width() * 0.72, rect.height() * 0.62),
        ),
        WelcomeAspect::Portrait9_16 => Rect::from_center_size(
            rect.center(),
            vec2(rect.width() * 0.42, rect.height() * 0.78),
        ),
        WelcomeAspect::Square1_1 => {
            let s = rect.height().min(rect.width()) * 0.68;
            Rect::from_center_size(rect.center(), vec2(s, s))
        }
        WelcomeAspect::Scope240_1 => Rect::from_center_size(
            pos2(rect.center().x, rect.center().y - 1.0),
            vec2(rect.width() * 0.90, rect.height() * 0.40),
        ),
    };
    let stroke = Stroke::new(1.5_f32, col);
    let inner = frame_rect.shrink(3.0);
    painter.rect_stroke(frame_rect, 2.0_f32, stroke, StrokeKind::Inside);
    painter.rect_stroke(
        inner,
        1.0_f32,
        Stroke::new(1.0_f32, col.linear_multiply(0.85)),
        StrokeKind::Inside,
    );
    match aspect {
        WelcomeAspect::Widescreen16_9 => {
            let mid_x = frame_rect.center().x;
            let y1 = frame_rect.bottom();
            painter.line_segment([pos2(mid_x, y1), pos2(mid_x, y1 + 4.0)], stroke);
            painter.line_segment(
                [pos2(mid_x - 6.0, y1 + 4.0), pos2(mid_x + 6.0, y1 + 4.0)],
                stroke,
            );
        }
        WelcomeAspect::Portrait9_16 => {
            painter.line_segment(
                [
                    pos2(frame_rect.center().x - 3.0, frame_rect.bottom() - 2.2),
                    pos2(frame_rect.center().x + 3.0, frame_rect.bottom() - 2.2),
                ],
                stroke,
            );
        }
        WelcomeAspect::Square1_1 => {
            let tail = vec![
                pos2(frame_rect.left() + 5.0, frame_rect.bottom() - 1.0),
                pos2(frame_rect.left() + 5.0, frame_rect.bottom() + 4.5),
                pos2(frame_rect.left() + 11.0, frame_rect.bottom() - 1.0),
            ];
            painter.add(Shape::convex_polygon(tail, col, Stroke::NONE));
        }
        WelcomeAspect::Scope240_1 => {
            let cy = frame_rect.bottom() + 4.5;
            let cx = frame_rect.center().x;
            painter.line_segment([pos2(cx - 5.0, cy), pos2(cx + 5.0, cy)], stroke);
            painter.line_segment([pos2(cx, frame_rect.bottom()), pos2(cx, cy)], stroke);
        }
        WelcomeAspect::Fullscreen4_3 => {}
    }
}

fn draw_aspect_tile(
    ui: &mut egui::Ui,
    size: Vec2,
    aspect: WelcomeAspect,
    active: bool,
    compact: bool,
    theme: &ThemeColors,
    lang: Language,
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let bg = if active {
        theme.selection_bg
    } else if resp.hovered() {
        theme.widget_hover_bg
    } else if theme.high_contrast {
        theme.extreme_bg
    } else {
        theme.widget_bg.linear_multiply(0.85)
    };
    let fg = if active {
        contrast_on(bg)
    } else {
        theme.text_primary
    };
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0_f32, bg);
    p.rect_stroke(
        rect,
        2.0_f32,
        Stroke::new(if active { 1.5_f32 } else { 1.0_f32 }, theme.border),
        StrokeKind::Inside,
    );
    let glyph_h = (rect.height() * 0.48).clamp(16.0, 28.0);
    let glyph_rect = Rect::from_center_size(
        pos2(rect.center().x, rect.top() + rect.height() * 0.36),
        vec2((rect.width() * 0.42).clamp(20.0, 36.0), glyph_h),
    );
    draw_aspect_glyph(&p, glyph_rect, aspect, fg);
    let text = if compact {
        aspect.short_label()
    } else {
        aspect.label_in(lang)
    };
    p.text(
        pos2(rect.center().x, rect.bottom() - rect.height() * 0.20),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(if rect.width() < 110.0 { 9.5 } else { 10.8 }),
        fg,
    );
    let full_label = aspect.label_in(lang);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, full_label));
    resp.clicked()
}

/// Ilustração vetorial original de câmera cinematográfica e lente óptica em Rust puro
/// (Clean Room — sem nenhuma imagem ou marca de terceiros).
fn draw_camera_hero_banner(ui: &mut egui::Ui, size: Vec2, theme: &ThemeColors) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter_at(rect);

    if theme.high_contrast {
        p.rect_filled(rect, 2.0_f32, theme.extreme_bg);
    } else {
        // Fundo azul estúdio em faixas horizontais suaves
        let steps = 8;
        let step_h = rect.height() / steps as f32;
        for i in 0..steps {
            let t = i as f32 / (steps - 1) as f32;
            let r = (18.0 + 18.0 * (1.0 - t)) as u8;
            let g = (48.0 + 48.0 * (1.0 - t)) as u8;
            let b = (132.0 + 82.0 * (1.0 - t)) as u8;
            let band = Rect::from_min_max(
                pos2(rect.left(), rect.top() + i as f32 * step_h),
                pos2(rect.right(), rect.top() + (i + 1) as f32 * step_h + 0.5),
            );
            p.rect_filled(band, 0.0_f32, Color32::from_rgb(r, g, b));
        }
        p.circle_filled(
            pos2(rect.left() + rect.width() * 0.58, rect.center().y),
            rect.height() * 0.72,
            Color32::from_rgba_unmultiplied(120, 175, 255, 24),
        );
    }

    let pt = |nx: f32, ny: f32| {
        pos2(
            rect.left() + nx * rect.width(),
            rect.top() + ny * rect.height(),
        )
    };
    let body_dark = Color32::from_rgb(24, 30, 42);
    let body_mid = Color32::from_rgb(44, 54, 72);
    let metal_hi = Color32::from_rgb(138, 152, 174);
    let rec_red = if theme.high_contrast {
        theme.accent
    } else {
        Color32::from_rgb(228, 70, 52)
    };

    // Alça / disparador superior em destaque vermelho
    p.add(Shape::convex_polygon(
        vec![
            pt(0.24, 0.14),
            pt(0.36, 0.14),
            pt(0.34, 0.25),
            pt(0.22, 0.25),
        ],
        rec_red,
        Stroke::new(1.0_f32, Color32::from_white_alpha(60)),
    ));

    // Corpo principal da câmera cinematográfica
    let cam_body = Rect::from_min_max(pt(0.11, 0.23), pt(0.45, 0.86));
    p.rect_filled(cam_body, CornerRadius::same(6), body_dark);
    p.rect_stroke(
        cam_body,
        CornerRadius::same(6),
        Stroke::new(1.5_f32, metal_hi.linear_multiply(0.75)),
        StrokeKind::Inside,
    );

    // Chanfro lateral e aletas de ventilação
    let side_plate = Rect::from_min_max(pt(0.20, 0.29), pt(0.40, 0.79));
    p.rect_filled(side_plate, CornerRadius::same(4), body_mid);
    for i in 0..4 {
        let x = 0.21 + i as f32 * 0.016;
        p.line_segment([pt(x, 0.34), pt(x, 0.62)], Stroke::new(2.0_f32, body_dark));
    }

    // Conectores laterais SDI / áudio
    for ny in [0.40, 0.53, 0.68] {
        let port_center = pt(0.16, ny);
        p.circle_filled(
            port_center,
            (rect.height() * 0.045).clamp(4.0, 9.0),
            metal_hi,
        );
        p.circle_filled(
            port_center,
            (rect.height() * 0.022).clamp(2.0, 4.5),
            body_dark,
        );
    }

    // Placa original Neoveg (monograma "N") na lateral da câmera
    let badge_s = (rect.height() * 0.14).clamp(14.0, 26.0);
    let badge_rect = Rect::from_center_size(pt(0.305, 0.56), vec2(badge_s, badge_s));
    p.rect_filled(badge_rect, 2.0_f32, Color32::from_rgb(20, 26, 36));
    p.rect_stroke(
        badge_rect,
        2.0_f32,
        Stroke::new(1.2_f32, metal_hi),
        StrokeKind::Inside,
    );
    p.text(
        badge_rect.center(),
        Align2::CENTER_CENTER,
        "N",
        FontId::proportional((badge_s * 0.58).max(9.0)),
        Color32::WHITE,
    );

    // Tubo e anéis da objetiva cinematográfica
    let barrel_rear = Rect::from_min_max(pt(0.39, 0.27), pt(0.56, 0.80));
    p.rect_filled(
        barrel_rear,
        CornerRadius::same(4),
        Color32::from_rgb(32, 40, 54),
    );
    let knurl_x = [0.43, 0.46, 0.51];
    for x in knurl_x {
        p.line_segment(
            [pt(x, 0.27), pt(x, 0.80)],
            Stroke::new(2.5_f32, metal_hi.linear_multiply(0.55)),
        );
    }
    p.line_segment(
        [pt(0.54, 0.28), pt(0.54, 0.79)],
        Stroke::new(2.0_f32, rec_red),
    );

    // Conjunto óptico frontal (elipse em perspectiva + reflexos multicoloridos)
    let lens_outer_r = (rect.height() * 0.34).clamp(26.0, 74.0);
    let lens_c = pt(0.61, 0.53);
    p.circle_filled(lens_c, lens_outer_r, Color32::from_rgb(20, 24, 32));
    p.circle_stroke(lens_c, lens_outer_r, Stroke::new(2.2_f32, metal_hi));
    p.circle_filled(lens_c, lens_outer_r * 0.84, Color32::from_rgb(34, 44, 62));
    p.circle_filled(lens_c, lens_outer_r * 0.66, Color32::from_rgb(14, 22, 38));
    p.circle_stroke(
        lens_c,
        lens_outer_r * 0.66,
        Stroke::new(1.5_f32, Color32::from_rgb(80, 175, 220)),
    );
    // Reflexos ópticos anti-reflexo
    p.circle_filled(
        lens_c + vec2(-lens_outer_r * 0.22, -lens_outer_r * 0.20),
        lens_outer_r * 0.24,
        Color32::from_rgba_unmultiplied(80, 210, 255, 72),
    );
    p.circle_filled(
        lens_c + vec2(lens_outer_r * 0.20, lens_outer_r * 0.16),
        lens_outer_r * 0.16,
        Color32::from_rgba_unmultiplied(150, 110, 255, 64),
    );
    p.circle_filled(
        lens_c + vec2(-lens_outer_r * 0.28, -lens_outer_r * 0.26),
        (lens_outer_r * 0.08).max(2.0),
        Color32::from_white_alpha(190),
    );

    p.rect_stroke(
        rect,
        0.0_f32,
        Stroke::new(1.0_f32, theme.border),
        StrokeKind::Inside,
    );
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
        self.new_project_configured(width, height, fps, fps.is_ntsc_drop_eligible(), false);
    }

    pub fn new_project_configured(
        &mut self,
        width: u32,
        height: u32,
        fps: FrameRate,
        drop_frame: bool,
        manually_configured: bool,
    ) {
        self.stop(false);
        let mut p = Project::new_default();
        p.settings.width = width.clamp(16, 8192);
        p.settings.height = height.clamp(16, 8192);
        p.settings.fps = fps;
        p.settings.drop_frame = fps.is_ntsc_drop_eligible() && drop_frame;
        p.settings.manually_configured = manually_configured;
        let lang = self.lang();
        self.session.reset_project(p, None);
        self.thumbs.clear();
        self.project_media.clear_thumbs();
        let w_s = self.project.settings.width.to_string();
        let h_s = self.project.settings.height.to_string();
        let fps_s = self.project.settings.fps.label_in(lang);
        self.set_status(tr_args(
            lang,
            "Novo projeto: {w}×{h} @ {fps} qps",
            &[("w", &w_s), ("h", &h_s), ("fps", &fps_s)],
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

        let lang = self.lang();
        let sr = ctx.screen_rect();
        let backdrop_col = if self.theme.high_contrast {
            Color32::from_black_alpha(230)
        } else if self.theme.dark_mode {
            Color32::from_rgba_unmultiplied(10, 12, 16, 175)
        } else {
            Color32::from_rgba_unmultiplied(24, 30, 42, 130)
        };

        let mut action = WelcomeAction::None;
        let mut lang_change: Option<Language> = None;
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
            sr.width() - 28.0
        } else {
            760.0
        };
        let safe_h = if sr.height().is_finite() {
            sr.height() - 32.0
        } else {
            540.0
        };
        let card_w = safe_w.clamp(340.0, 820.0);
        let card_h = safe_h.clamp(320.0, 560.0);
        let compact_w = card_w < 580.0;
        let compact_h = card_h < 420.0;

        let border_w = if self.theme.high_contrast {
            2.0_f32
        } else {
            1.0_f32
        };
        let frame = egui::Frame::window(&ctx.style())
            .fill(self.theme.window_fill)
            .stroke(Stroke::new(
                border_w,
                if self.theme.high_contrast {
                    self.theme.accent
                } else {
                    self.theme.border
                },
            ))
            .inner_margin(Margin::symmetric(0, 0));

        let workers = self.prefs.effective_worker_threads();
        let recents = self.prefs.recent_projects.clone();
        let has_recovery = crate::recovery::find_existing_recovery().is_some();
        let mut show_on_startup = self.prefs.show_welcome;
        let show_before = show_on_startup;
        let theme = self.theme.clone();
        let mut state = self.welcome.clone();

        egui::Window::new("neoveg_welcome_card")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .order(egui::Order::Middle)
            .fixed_size(vec2(card_w, card_h))
            .frame(frame)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                // Barra superior centralizada com logo e nome Neoveg + seletor de idioma + botão Fechar
                let header_h = 40.0;
                let (h_rect, _) = ui.allocate_exact_size(vec2(card_w, header_h), Sense::hover());
                let hp = ui.painter_at(h_rect);
                let header_fill = if theme.high_contrast {
                    theme.extreme_bg
                } else {
                    theme.panel_fill
                };
                hp.rect_filled(h_rect, 0.0_f32, header_fill);
                let lang_w = if compact_w { 88.0 } else { 128.0 };
                let lang_rect = Rect::from_center_size(
                    pos2(h_rect.left() + lang_w * 0.5 + 10.0, h_rect.center().y),
                    vec2(lang_w, 24.0),
                );
                let mut lang_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(lang_rect)
                        .id_salt("welcome_lang_child"),
                );
                egui::ComboBox::from_id_salt("welcome_language_combo")
                    .width(lang_w)
                    .selected_text(lang.native_name())
                    .show_ui(&mut lang_ui, |ui| {
                        for l in Language::ALL {
                            if ui.selectable_label(lang == l, l.native_name()).clicked()
                                && l != lang
                            {
                                lang_change = Some(l);
                            }
                        }
                    });
                let mark_rect = Rect::from_center_size(
                    pos2(h_rect.center().x - 42.0, h_rect.center().y),
                    vec2(24.0, 24.0),
                );
                draw_neoveg_mark(&hp, mark_rect, &theme);
                hp.text(
                    pos2(h_rect.center().x - 24.0, h_rect.center().y),
                    Align2::LEFT_CENTER,
                    "Neoveg",
                    FontId::proportional(17.5),
                    theme.text_primary,
                );
                let close_rect = Rect::from_center_size(
                    pos2(h_rect.right() - 20.0, h_rect.center().y),
                    vec2(26.0, 26.0),
                );
                let mut close_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(close_rect)
                        .id_salt("welcome_close_child"),
                );
                if icons::small_icon_button(&mut close_ui, Icon::CloseX, None, false, &theme)
                    .on_hover_text(tr(lang, "Fechar (Esc)"))
                    .clicked()
                {
                    action = WelcomeAction::Close;
                }

                // Corpo principal + Rodapé
                let footer_h = 34.0;
                let body_h = (card_h - header_h - footer_h).max(200.0);
                let nav_w = if compact_w { 86.0 } else { 114.0 };
                let right_w = (card_w - nav_w).max(220.0);

                ui.allocate_ui_with_layout(
                    vec2(card_w, body_h),
                    egui::Layout::left_to_right(egui::Align::Min),
                    |ui| {
                        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                        // Coluna esquerda escura/painel com os 3 botões grandes
                        let (nav_rect, _) =
                            ui.allocate_exact_size(vec2(nav_w, body_h), Sense::hover());
                        let nav_bg = if theme.high_contrast {
                            theme.extreme_bg
                        } else {
                            theme.panel_fill
                        };
                        ui.painter_at(nav_rect)
                            .rect_filled(nav_rect, 0.0_f32, nav_bg);

                        let mut nav_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(nav_rect)
                                .layout(egui::Layout::top_down(egui::Align::LEFT))
                                .id_salt("welcome_nav_rail"),
                        );
                        nav_ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                        let tile_h = if compact_h { 58.0 } else { 82.0 };
                        for sec in WelcomeSection::ALL {
                            if draw_nav_tile(
                                &mut nav_ui,
                                vec2(nav_w, tile_h),
                                sec,
                                state.section == sec,
                                &theme,
                                lang,
                            ) {
                                state.section = sec;
                            }
                        }
                        if has_recovery {
                            nav_ui.add_space(6.0);
                            nav_ui.with_layout(
                                egui::Layout::bottom_up(egui::Align::Center),
                                |ui| {
                                    ui.add_space(6.0);
                                    if ui
                                        .add_sized(
                                            [nav_w - 10.0, 26.0],
                                            egui::Button::new(
                                                RichText::new(tr(lang, "Recuperar"))
                                                    .small()
                                                    .color(theme.accent),
                                            ),
                                        )
                                        .on_hover_text(tr(
                                            lang,
                                            "Recuperar projeto salvo automaticamente",
                                        ))
                                        .clicked()
                                    {
                                        action = WelcomeAction::RecoverSession;
                                    }
                                },
                            );
                        }

                        // Painel principal à direita
                        let (right_rect, _) =
                            ui.allocate_exact_size(vec2(right_w, body_h), Sense::hover());
                        let mut right_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(right_rect)
                                .layout(egui::Layout::top_down(egui::Align::LEFT))
                                .id_salt("welcome_right_pane"),
                        );
                        right_ui.set_clip_rect(right_rect);
                        right_ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
                        match state.section {
                            WelcomeSection::NewProject => {
                                action = draw_new_project_pane(
                                    &mut right_ui,
                                    &mut state,
                                    &theme,
                                    lang,
                                    compact_w,
                                    compact_h,
                                );
                            }
                            WelcomeSection::OpenProject => {
                                action = draw_open_project_pane(
                                    &mut right_ui,
                                    &recents,
                                    has_recovery,
                                    &theme,
                                    lang,
                                );
                            }
                            WelcomeSection::GettingStarted => {
                                action = draw_getting_started_pane(&mut right_ui, &theme, lang);
                            }
                        }
                    },
                );

                // Rodapé: checkbox Mostrar tela de boas-vindas ao iniciar
                let (foot_rect, _) = ui.allocate_exact_size(vec2(card_w, footer_h), Sense::hover());
                let fp = ui.painter_at(foot_rect);
                fp.rect_filled(foot_rect, 0.0_f32, theme.window_fill);
                fp.line_segment(
                    [foot_rect.left_top(), foot_rect.right_top()],
                    Stroke::new(1.0_f32, theme.border),
                );
                let mut foot_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(foot_rect.shrink2(vec2(12.0, 4.0)))
                        .id_salt("welcome_footer_child"),
                );
                foot_ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut show_on_startup,
                        tr(lang, "Mostrar tela de boas-vindas ao iniciar"),
                    );
                    if !compact_w {
                        let ver_s = env!("CARGO_PKG_VERSION");
                        let w_s = workers.to_string();
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(tr_args(
                                    lang,
                                    "v{ver} • {n} worker(s)",
                                    &[("ver", &ver_s), ("n", &w_s)],
                                ))
                                .small()
                                .color(theme.text_muted),
                            );
                        });
                    }
                });
            });

        self.welcome = state;

        if let Some(new_lang) = lang_change {
            self.apply_language(Some(ctx), new_lang, true);
        }

        if show_on_startup != show_before {
            self.prefs.show_welcome = show_on_startup;
            if let Err(e) = self.prefs.save() {
                let err_s = e.to_string();
                self.set_status(tr_args(
                    self.lang(),
                    "Aviso ao salvar preferência: {err}",
                    &[("err", &err_s)],
                ));
            }
        }

        match action {
            WelcomeAction::None => {}
            WelcomeAction::Close => {
                self.welcome_open = false;
            }
            WelcomeAction::CreateProject {
                width,
                height,
                fps,
                drop_frame,
                open_advanced,
            } => {
                self.new_project_configured(width, height, fps, drop_frame, true);
                self.welcome_open = false;
                if open_advanced {
                    self.dlg.project_props = true;
                }
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
                    let err_s = e.to_string();
                    self.set_status(tr_args(
                        lang,
                        "Aviso ao atualizar recentes: {err}",
                        &[("err", &err_s)],
                    ));
                }
            }
            WelcomeAction::ClearRecents => {
                self.prefs.clear_recent_projects();
                if let Err(e) = self.prefs.save() {
                    let err_s = e.to_string();
                    self.set_status(tr_args(
                        lang,
                        "Aviso ao limpar recentes: {err}",
                        &[("err", &err_s)],
                    ));
                }
            }
            WelcomeAction::RecoverSession => {
                if let Some(p) = crate::recovery::find_existing_recovery() {
                    self.welcome_open = false;
                    self.open_project_path(p);
                    self.path = None;
                    self.dirty = true;
                    self.set_status(tr(lang, "Projeto recuperado. Salve-o com Salvar como..."));
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
            WelcomeAction::OpenShortcuts => {
                self.welcome_open = false;
                self.dlg.shortcuts = true;
            }
        }
    }
}

fn draw_new_project_pane(
    ui: &mut egui::Ui,
    state: &mut WelcomeState,
    theme: &ThemeColors,
    lang: Language,
    compact_w: bool,
    compact_h: bool,
) -> WelcomeAction {
    let mut act = WelcomeAction::None;
    let total_w = ui.available_width();
    let total_h = ui.available_height();
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

    // Em janelas baixas/compactas (760x520 ou 420x420) preservamos o formulário inteiro
    // reduzindo suavemente o banner e usando rolagem caso necessário.
    let hero_h = if compact_h {
        76.0
    } else {
        (total_h * 0.43).clamp(115.0, 205.0)
    };
    draw_camera_hero_banner(ui, vec2(total_w, hero_h), theme);

    // Tira de 5 botões de proporção (16:9, 4:3, 9:16, 1:1, 2.4:1)
    let tile_h = if compact_h { 50.0 } else { 66.0 };
    let n_tiles = WelcomeAspect::ALL.len() as f32;
    let tile_w = (total_w / n_tiles).max(46.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        for aspect in WelcomeAspect::ALL {
            if draw_aspect_tile(
                ui,
                vec2(tile_w, tile_h),
                aspect,
                state.aspect == aspect,
                compact_w,
                theme,
                lang,
            ) {
                state.select_aspect(aspect);
            }
        }
    });

    ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
    ui.add_space(if compact_h { 4.0 } else { 10.0 });
    egui::Frame::NONE
        .inner_margin(Margin::symmetric(if compact_w { 10 } else { 18 }, 4))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("welcome_new_form_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(tr(lang, "Criar um novo projeto"))
                            .strong()
                            .color(theme.text_primary),
                    );
                    ui.add_space(6.0);

                    let res_list = state.aspect.resolutions();
                    let cur_res = state.current_resolution();
                    let cur_fps = state.current_fps();
                    let drop_eligible = cur_fps.fps.is_ntsc_drop_eligible();
                    let label_w = if compact_w { 92.0 } else { 112.0 };
                    let field_w = (ui.available_width() - label_w - 8.0).max(120.0);

                    ui.horizontal(|ui| {
                        ui.add_sized([label_w, 22.0], egui::Label::new(tr(lang, "Resolução:")));
                        egui::ComboBox::from_id_salt("welcome_res_combo")
                            .width(field_w)
                            .selected_text(tr(lang, cur_res.label))
                            .show_ui(ui, |ui| {
                                for (idx, item) in res_list.iter().enumerate() {
                                    ui.selectable_value(
                                        &mut state.res_idx,
                                        idx,
                                        tr(lang, item.label),
                                    );
                                }
                            });
                    });

                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.add_sized(
                            [label_w, 22.0],
                            egui::Label::new(tr(lang, "Taxa de quadros:")),
                        );
                        let fps_w = if compact_w {
                            field_w.max(120.0)
                        } else {
                            (field_w * 0.62 - 4.0).max(130.0)
                        };
                        let drop_w = (field_w - fps_w - 6.0).max(115.0);
                        let prev_fps_idx = state.fps_idx;
                        egui::ComboBox::from_id_salt("welcome_fps_combo")
                            .width(fps_w)
                            .selected_text(tr(lang, cur_fps.label))
                            .show_ui(ui, |ui| {
                                for (idx, item) in WELCOME_FPS.iter().enumerate() {
                                    ui.selectable_value(
                                        &mut state.fps_idx,
                                        idx,
                                        tr(lang, item.label),
                                    );
                                }
                            });
                        if state.fps_idx != prev_fps_idx {
                            state.select_fps(state.fps_idx);
                        }

                        let drop_label = if drop_eligible {
                            if state.drop_frame {
                                tr(lang, "Drop Frame (1/1.001)")
                            } else {
                                tr(lang, "Non-Drop Frame (NTSC)")
                            }
                        } else if cur_fps.fps == FrameRate::FPS_23_976 {
                            tr(lang, "NTSC Film (1/1.001)")
                        } else {
                            tr(lang, "Taxa inteira (1/1)")
                        };
                        egui::ComboBox::from_id_salt("welcome_drop_combo")
                            .width(drop_w)
                            .selected_text(drop_label)
                            .show_ui(ui, |ui| {
                                if drop_eligible {
                                    ui.selectable_value(
                                        &mut state.drop_frame,
                                        true,
                                        tr(lang, "Drop Frame (1/1.001)"),
                                    );
                                    ui.selectable_value(
                                        &mut state.drop_frame,
                                        false,
                                        tr(lang, "Non-Drop Frame (NTSC)"),
                                    );
                                } else {
                                    ui.label(
                                        RichText::new(tr(
                                            lang,
                                            "Drop Frame aplica-se a 29,970 e 59,940 qps.",
                                        ))
                                        .small()
                                        .color(theme.text_muted),
                                    );
                                }
                            });
                    });

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if !compact_w {
                            ui.add_space(label_w + 6.0);
                        }
                        if ui.button(tr(lang, "Criar Projeto")).clicked() {
                            act = WelcomeAction::CreateProject {
                                width: cur_res.width,
                                height: cur_res.height,
                                fps: cur_fps.fps,
                                drop_frame: state.drop_frame,
                                open_advanced: false,
                            };
                        }
                        if ui.button(tr(lang, "Configurações Avançadas...")).clicked() {
                            act = WelcomeAction::CreateProject {
                                width: cur_res.width,
                                height: cur_res.height,
                                fps: cur_fps.fps,
                                drop_frame: state.drop_frame,
                                open_advanced: true,
                            };
                        }
                    });
                });
        });
    act
}

fn draw_open_project_pane(
    ui: &mut egui::Ui,
    recents: &[String],
    has_recovery: bool,
    theme: &ThemeColors,
    lang: Language,
) -> WelcomeAction {
    let mut act = WelcomeAction::None;
    egui::Frame::NONE
        .inner_margin(Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.label(
                RichText::new(tr(lang, "Abrir Projeto"))
                    .size(16.0)
                    .strong()
                    .color(theme.text_primary),
            );
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if icons::icon_button(
                    ui,
                    Icon::OpenFolder,
                    Some(tr(lang, "Procurar arquivo (.neoveg / .nvp)...")),
                    false,
                    true,
                    theme,
                )
                .clicked()
                {
                    act = WelcomeAction::OpenDialog;
                }
                if icons::icon_button(
                    ui,
                    Icon::ImportMedia,
                    Some(tr(lang, "Importar mídia...")),
                    false,
                    true,
                    theme,
                )
                .clicked()
                {
                    act = WelcomeAction::ImportMedia;
                }
                if !recents.is_empty() && ui.small_button(tr(lang, "Limpar lista")).clicked() {
                    act = WelcomeAction::ClearRecents;
                }
            });

            if has_recovery {
                ui.add_space(6.0);
                if ui
                    .button(tr(lang, "Recuperar sessão anterior..."))
                    .clicked()
                {
                    act = WelcomeAction::RecoverSession;
                }
            }

            ui.add_space(10.0);
            ui.label(
                RichText::new(tr(lang, "Projetos recentes"))
                    .strong()
                    .color(theme.text_primary),
            );
            ui.add_space(4.0);
            if recents.is_empty() {
                ui.group(|ui| {
                    ui.label(
                        RichText::new(tr(lang, "Nenhum projeto recente registrado ainda."))
                            .color(theme.text_muted),
                    );
                });
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("welcome_recents_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for raw in recents {
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
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        let title_rt = if exists {
                                            RichText::new(&name).strong()
                                        } else {
                                            RichText::new(tr_args(
                                                lang,
                                                "{name} (ausente)",
                                                &[("name", &name)],
                                            ))
                                            .color(theme.text_muted)
                                        };
                                        if ui
                                            .add(egui::Button::new(title_rt).frame(false))
                                            .clicked()
                                        {
                                            act = WelcomeAction::OpenRecent(p.to_path_buf());
                                        }
                                        ui.label(RichText::new(dir_str).small().weak());
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if icons::small_icon_button(
                                                ui,
                                                Icon::CloseX,
                                                None,
                                                false,
                                                theme,
                                            )
                                            .on_hover_text(tr(lang, "Remover dos recentes"))
                                            .clicked()
                                            {
                                                act = WelcomeAction::RemoveRecent(raw.clone());
                                            }
                                            if ui
                                                .add_enabled(
                                                    exists,
                                                    egui::Button::new(tr(lang, "Abrir")).small(),
                                                )
                                                .clicked()
                                            {
                                                act = WelcomeAction::OpenRecent(p.to_path_buf());
                                            }
                                        },
                                    );
                                });
                            });
                        }
                    });
            }
        });
    act
}

fn draw_getting_started_pane(
    ui: &mut egui::Ui,
    theme: &ThemeColors,
    lang: Language,
) -> WelcomeAction {
    let mut act = WelcomeAction::None;
    egui::Frame::NONE
        .inner_margin(Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.label(
                RichText::new(tr(lang, "Primeiros Passos no Neoveg"))
                    .size(16.0)
                    .strong()
                    .color(theme.text_primary),
            );
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .id_salt("welcome_guide_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let tips = [
                        (
                            "1. Importar e organizar mídia",
                            "Use Ctrl+I ou a aba Mídia do Projeto para adicionar vídeo, áudio ou imagens à timeline.",
                        ),
                        (
                            "2. Edição rápida estilo VEGAS",
                            "S divide eventos no cursor; M adiciona marcador; R cria região; J/K/L faz scrub; Ctrl+L ativa Auto Ripple.",
                        ),
                        (
                            "3. Prévia, Cor e Master Bus",
                            "Alterne Rascunho/Prévia/Boa/Melhor na Prévia de Vídeo, ajuste rodas tonais em Color Grading e monitore o áudio no Master Bus.",
                        ),
                        (
                            "4. Exportação",
                            "Abra Arquivo → Renderizar como... para escolher modelo de saída, pesquisa e favoritos ★.",
                        ),
                    ];
                    for (title, body) in tips {
                        ui.group(|ui| {
                            ui.label(
                                RichText::new(tr(lang, title))
                                    .strong()
                                    .color(theme.text_primary),
                            );
                            ui.label(
                                RichText::new(tr(lang, body))
                                    .small()
                                    .color(theme.text_muted),
                            );
                        });
                        ui.add_space(4.0);
                    }
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        if icons::small_icon_button(
                            ui,
                            Icon::LayoutWindow,
                            Some(tr(lang, "Layout da interface...")),
                            false,
                            theme,
                        )
                        .clicked()
                        {
                            act = WelcomeAction::OpenLayoutProps;
                        }
                        if icons::small_icon_button(
                            ui,
                            Icon::GearProps,
                            Some(tr(lang, "Preferências...")),
                            false,
                            theme,
                        )
                        .clicked()
                        {
                            act = WelcomeAction::OpenPrefs;
                        }
                        if ui.button(tr(lang, "Atalhos de teclado...")).clicked() {
                            act = WelcomeAction::OpenShortcuts;
                        }
                    });
                });
        });
    act
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeId;
    use egui::{Modifiers, RawInput};

    #[test]
    fn welcome_screen_desenha_em_todos_os_temas_sem_panico() {
        let _lock = crate::kittest_tests::lock_test_prefs();
        let ctx = egui::Context::default();
        let mut app = App::with_context(&ctx, None);
        app.dismiss_splash();
        app.prefs
            .record_recent_project(Path::new("C:/projetos/exemplo_neoveg.neoveg"));
        app.open_welcome();

        for (i, tid) in ThemeId::ALL.into_iter().enumerate() {
            app.set_theme(&ctx, tid, false);
            for (j, sec) in WelcomeSection::ALL.into_iter().enumerate() {
                app.welcome.section = sec;
                let input = RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        pos2(0.0, 0.0),
                        if j == 2 {
                            vec2(420.0, 420.0)
                        } else {
                            vec2(1280.0, 760.0)
                        },
                    )),
                    time: Some((i * 10 + j) as f64 * 0.1),
                    modifiers: Modifiers::NONE,
                    ..Default::default()
                };
                let _ = ctx.run(input, |c| {
                    app.show_welcome_overlay(c);
                });
                assert!(app.is_welcome_open());
            }
        }

        app.welcome.section = WelcomeSection::NewProject;
        app.welcome.select_aspect(WelcomeAspect::Scope240_1);
        assert_eq!(app.welcome.current_resolution().width, 2048);
        assert_eq!(app.welcome.current_resolution().height, 858);

        app.welcome.select_aspect(WelcomeAspect::Portrait9_16);
        let res = app.welcome.current_resolution();
        assert_eq!((res.width, res.height), (1080, 1920));

        app.new_project_configured(res.width, res.height, FrameRate::FPS_29_97, true, true);
        assert_eq!(app.project.settings.width, 1080);
        assert_eq!(app.project.settings.height, 1920);
        assert!(app.project.settings.drop_frame);
        assert!(app.project.settings.manually_configured);

        app.new_project_with_preset(1280, 720, FrameRate::FPS_60);
        assert_eq!(app.project.settings.width, 1280);
        assert_eq!(app.project.settings.height, 720);
        assert!(!app.project.settings.manually_configured);

        app.dismiss_welcome();
        assert!(!app.is_welcome_open());
    }
}
