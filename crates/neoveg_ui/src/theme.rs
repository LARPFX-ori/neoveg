//! Catálogo dos 11 temas visuais da interface Neoveg (§2.2), cobrindo as
//! quatro gerações (Clássico, Prata Intermediário, Escuro/Médio/Claro/Branco/Ardósia
//! e Moderno) e dois temas de Alto Contraste WCAG AAA (razão de contraste >= 7:1).

use eframe::egui;
use egui::{Color32, Stroke};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ThemeId {
    #[default]
    VegasEscuro,
    VegasModerno,
    VegasProfundo,
    VegasMedio,
    VegasPrata,
    VegasClaro,
    VegasClassico,
    VegasBranco,
    VegasArdosia,
    AltoContrasteEscuro,
    AltoContrasteClaro,
}

impl ThemeId {
    pub const ALL: [ThemeId; 11] = [
        ThemeId::VegasEscuro,
        ThemeId::VegasModerno,
        ThemeId::VegasProfundo,
        ThemeId::VegasMedio,
        ThemeId::VegasPrata,
        ThemeId::VegasClaro,
        ThemeId::VegasClassico,
        ThemeId::VegasBranco,
        ThemeId::VegasArdosia,
        ThemeId::AltoContrasteEscuro,
        ThemeId::AltoContrasteClaro,
    ];

    pub fn id(self) -> &'static str {
        match self {
            ThemeId::VegasEscuro => "vegas-escuro",
            ThemeId::VegasModerno => "vegas-moderno",
            ThemeId::VegasProfundo => "vegas-profundo",
            ThemeId::VegasMedio => "vegas-medio",
            ThemeId::VegasPrata => "vegas-prata",
            ThemeId::VegasClaro => "vegas-claro",
            ThemeId::VegasClassico => "vegas-classico",
            ThemeId::VegasBranco => "vegas-branco",
            ThemeId::VegasArdosia => "vegas-ardosia",
            ThemeId::AltoContrasteEscuro => "alto-contraste-escuro",
            ThemeId::AltoContrasteClaro => "alto-contraste-claro",
        }
    }

    pub fn label_in(self, lang: neoveg_common::Language) -> &'static str {
        let pt = match self {
            ThemeId::VegasEscuro => "VEGAS Escuro (padrão)",
            ThemeId::VegasModerno => "VEGAS Moderno (23+)",
            ThemeId::VegasProfundo => "VEGAS Profundo (Preto)",
            ThemeId::VegasMedio => "VEGAS Médio",
            ThemeId::VegasPrata => "VEGAS Prata (Intermediário)",
            ThemeId::VegasClaro => "VEGAS Claro (Clássico)",
            ThemeId::VegasClassico => "VEGAS Clássico (Cinza Quente)",
            ThemeId::VegasBranco => "VEGAS Branco",
            ThemeId::VegasArdosia => "VEGAS Ardósia Azul",
            ThemeId::AltoContrasteEscuro => "Alto Contraste AAA (Escuro)",
            ThemeId::AltoContrasteClaro => "Alto Contraste AAA (Claro)",
        };
        neoveg_common::tr(lang, pt)
    }

    pub fn label(self) -> &'static str {
        self.label_in(neoveg_common::Language::PtBr)
    }

    pub fn from_pref(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "vegas-moderno" | "moderno" | "modern" => ThemeId::VegasModerno,
            "vegas-profundo" | "deep" | "black" => ThemeId::VegasProfundo,
            "vegas-medio" | "medium" | "medio" => ThemeId::VegasMedio,
            "vegas-prata" | "silver" | "prata" => ThemeId::VegasPrata,
            "vegas-claro" | "light" | "claro" => ThemeId::VegasClaro,
            "vegas-classico" | "classic" | "classico" => ThemeId::VegasClassico,
            "vegas-branco" | "white" | "branco" => ThemeId::VegasBranco,
            "vegas-ardosia" | "slate" | "blue" => ThemeId::VegasArdosia,
            "alto-contraste-escuro" | "high-contrast-dark" | "aaa-dark" => {
                ThemeId::AltoContrasteEscuro
            }
            "alto-contraste-claro" | "high-contrast-light" | "aaa-light" => {
                ThemeId::AltoContrasteClaro
            }
            _ => ThemeId::VegasEscuro,
        }
    }

    pub fn is_high_contrast(self) -> bool {
        matches!(
            self,
            ThemeId::AltoContrasteEscuro | ThemeId::AltoContrasteClaro
        )
    }
}

/// Paleta semântica completa para interface e timeline.
#[derive(Clone, Debug)]
pub struct ThemeColors {
    pub dark_mode: bool,
    pub high_contrast: bool,
    pub panel_fill: Color32,
    pub window_fill: Color32,
    pub extreme_bg: Color32,
    pub widget_bg: Color32,
    pub widget_hover_bg: Color32,
    pub selection_bg: Color32,
    pub text_primary: Color32,
    pub text_muted: Color32,
    pub border: Color32,
    pub accent: Color32,
    pub timecode_fg: Color32,
    pub offline_fg: Color32,
    pub tl_bg: Color32,
    pub tl_header_bg: Color32,
    pub tl_header_sel: Color32,
    pub tl_header_idle: Color32,
    pub tl_loop_bar: Color32,
    pub tl_marker_bar: Color32,
    pub tl_ruler_bg: Color32,
    pub tl_grid_line: Color32,
    pub tl_lane_even: Color32,
    pub tl_lane_odd: Color32,
    pub tl_playhead_edit: Color32,
    pub tl_playhead_live: Color32,
    pub tl_loop_active: Color32,
    pub tl_loop_inactive: Color32,
    pub tl_marker: Color32,
    pub tl_region: Color32,
    pub tl_sel_band: Color32,
    pub tl_fade: Color32,
    pub tl_crossfade: Color32,
    pub tl_gain_line: Color32,
    pub tl_env_primary: Color32,
    pub tl_env_secondary: Color32,
    pub tl_event_border: Color32,
    pub tl_event_sel_border: Color32,
    pub tl_preview_bg: Color32,
    /// Fator da cor da trilha no corpo do evento (multiplica 0,62 quando o
    /// evento não está selecionado e 1,0 quando está). O VEGAS 15–22 usa o
    /// corpo apagado; o 23/2026, a cor da trilha cheia.
    pub tl_event_body_k: f32,
    /// Fator da cor da trilha na barra de nome do evento.
    pub tl_event_header_k: f32,
    /// Chanfros 3D sutis nos cabeçalhos e painéis, como nos temas antigos
    /// (Clássico v1–v7, Claro XP v8 e Prata v10–v14).
    pub classic_bevel: bool,
}

pub fn palette(id: ThemeId) -> ThemeColors {
    match id {
        ThemeId::VegasEscuro => ThemeColors {
            // Geração 15–22: cinza neutro 34 em painéis, régua, cabeçalhos e
            // faixas (tons observados na interface)
            dark_mode: true,
            high_contrast: false,
            panel_fill: Color32::from_rgb(34, 34, 34),
            window_fill: Color32::from_rgb(40, 40, 40),
            extreme_bg: Color32::from_rgb(20, 20, 20),
            widget_bg: Color32::from_rgb(46, 46, 46),
            widget_hover_bg: Color32::from_rgb(62, 62, 62),
            selection_bg: Color32::from_rgb(38, 108, 192),
            text_primary: Color32::from_rgb(234, 236, 240),
            text_muted: Color32::from_rgb(176, 180, 190),
            border: Color32::from_rgb(58, 58, 58),
            accent: Color32::from_rgb(120, 200, 255),
            timecode_fg: Color32::from_rgb(228, 232, 238),
            offline_fg: Color32::from_rgb(245, 130, 90),
            tl_bg: Color32::from_rgb(34, 34, 34),
            tl_header_bg: Color32::from_rgb(34, 34, 34),
            tl_header_sel: Color32::from_rgb(50, 66, 86),
            tl_header_idle: Color32::from_rgb(36, 36, 36),
            tl_loop_bar: Color32::from_rgb(28, 28, 28),
            tl_marker_bar: Color32::from_rgb(30, 30, 30),
            tl_ruler_bg: Color32::from_rgb(34, 34, 34),
            tl_grid_line: Color32::from_rgb(44, 44, 44),
            tl_lane_even: Color32::from_rgb(37, 37, 37),
            tl_lane_odd: Color32::from_rgb(34, 34, 34),
            tl_playhead_edit: Color32::from_rgb(255, 220, 90),
            tl_playhead_live: Color32::from_rgb(255, 120, 60),
            tl_loop_active: Color32::from_rgb(76, 136, 224),
            tl_loop_inactive: Color32::from_rgb(74, 76, 88),
            tl_marker: Color32::from_rgb(232, 140, 44),
            tl_region: Color32::from_rgb(66, 178, 92),
            tl_sel_band: Color32::from_rgba_unmultiplied(76, 136, 224, 42),
            tl_fade: Color32::from_rgb(250, 246, 150),
            tl_crossfade: Color32::from_rgb(90, 175, 255),
            tl_gain_line: Color32::from_rgb(130, 235, 155),
            tl_env_primary: Color32::from_rgb(90, 160, 255),
            tl_env_secondary: Color32::from_rgb(255, 110, 110),
            tl_event_border: Color32::from_gray(14),
            tl_event_sel_border: Color32::WHITE,
            tl_preview_bg: Color32::from_rgb(34, 34, 34),
            tl_event_body_k: 0.55,
            tl_event_header_k: 0.9,
            classic_bevel: false,
        },
        ThemeId::VegasModerno => ThemeColors {
            // Geração 23/2026: cinza neutro 40 em painéis, régua e faixas,
            // prévia em 20 e seleção azul (tons observados na interface)
            dark_mode: true,
            high_contrast: false,
            panel_fill: Color32::from_rgb(40, 40, 40),
            window_fill: Color32::from_rgb(44, 44, 44),
            extreme_bg: Color32::from_rgb(20, 20, 20),
            widget_bg: Color32::from_rgb(50, 50, 50),
            widget_hover_bg: Color32::from_rgb(64, 64, 64),
            selection_bg: Color32::from_rgb(61, 141, 220),
            text_primary: Color32::from_rgb(238, 240, 244),
            text_muted: Color32::from_rgb(168, 172, 180),
            border: Color32::from_rgb(58, 58, 58),
            accent: Color32::from_rgb(84, 184, 255),
            timecode_fg: Color32::from_rgb(232, 236, 242),
            offline_fg: Color32::from_rgb(245, 118, 86),
            tl_bg: Color32::from_rgb(40, 40, 40),
            tl_header_bg: Color32::from_rgb(40, 40, 40),
            tl_header_sel: Color32::from_rgb(48, 70, 96),
            tl_header_idle: Color32::from_rgb(42, 42, 42),
            tl_loop_bar: Color32::from_rgb(34, 34, 34),
            tl_marker_bar: Color32::from_rgb(36, 36, 36),
            tl_ruler_bg: Color32::from_rgb(40, 40, 40),
            tl_grid_line: Color32::from_rgb(48, 48, 48),
            tl_lane_even: Color32::from_rgb(42, 42, 42),
            tl_lane_odd: Color32::from_rgb(40, 40, 40),
            tl_playhead_edit: Color32::from_rgb(255, 212, 72),
            tl_playhead_live: Color32::from_rgb(255, 112, 58),
            tl_loop_active: Color32::from_rgb(56, 146, 238),
            tl_loop_inactive: Color32::from_rgb(72, 74, 82),
            tl_marker: Color32::from_rgb(242, 144, 36),
            tl_region: Color32::from_rgb(56, 194, 102),
            tl_sel_band: Color32::from_rgba_unmultiplied(56, 146, 238, 42),
            tl_fade: Color32::from_rgb(252, 244, 142),
            tl_crossfade: Color32::from_rgb(74, 174, 255),
            tl_gain_line: Color32::from_rgb(112, 236, 152),
            tl_env_primary: Color32::from_rgb(74, 166, 255),
            tl_env_secondary: Color32::from_rgb(255, 106, 106),
            tl_event_border: Color32::from_gray(12),
            tl_event_sel_border: Color32::WHITE,
            tl_preview_bg: Color32::from_rgb(20, 20, 20),
            tl_event_body_k: 1.6,
            tl_event_header_k: 1.6,
            classic_bevel: false,
        },
        ThemeId::VegasPrata => ThemeColors {
            // Cinza prata metálico característico da geração Intermediária (v10–v14):
            // painéis titânio, régua clara, trilhas densas e cabeçalho selecionado azul-aço
            dark_mode: false,
            high_contrast: false,
            panel_fill: Color32::from_rgb(158, 160, 164),
            window_fill: Color32::from_rgb(172, 174, 178),
            extreme_bg: Color32::from_rgb(230, 232, 236),
            widget_bg: Color32::from_rgb(148, 150, 156),
            widget_hover_bg: Color32::from_rgb(138, 152, 174),
            selection_bg: Color32::from_rgb(46, 94, 162),
            text_primary: Color32::from_rgb(12, 14, 18),
            text_muted: Color32::from_rgb(34, 38, 46),
            border: Color32::from_rgb(98, 102, 110),
            accent: Color32::from_rgb(24, 72, 148),
            timecode_fg: Color32::from_rgb(14, 16, 20),
            offline_fg: Color32::from_rgb(156, 28, 12),
            tl_bg: Color32::from_rgb(136, 138, 142),
            tl_header_bg: Color32::from_rgb(162, 164, 168),
            tl_header_sel: Color32::from_rgb(128, 156, 188),
            tl_header_idle: Color32::from_rgb(170, 172, 176),
            tl_loop_bar: Color32::from_rgb(142, 144, 150),
            tl_marker_bar: Color32::from_rgb(158, 160, 166),
            tl_ruler_bg: Color32::from_rgb(212, 214, 218),
            tl_grid_line: Color32::from_rgb(116, 119, 126),
            tl_lane_even: Color32::from_rgb(148, 150, 154),
            tl_lane_odd: Color32::from_rgb(138, 140, 144),
            tl_playhead_edit: Color32::from_rgb(122, 14, 8),
            tl_playhead_live: Color32::from_rgb(158, 22, 10),
            tl_loop_active: Color32::from_rgb(44, 92, 168),
            tl_loop_inactive: Color32::from_rgb(96, 100, 110),
            tl_marker: Color32::from_rgb(184, 86, 12),
            tl_region: Color32::from_rgb(20, 112, 46),
            tl_sel_band: Color32::from_rgba_unmultiplied(44, 92, 168, 48),
            tl_fade: Color32::from_rgb(122, 82, 0),
            tl_crossfade: Color32::from_rgb(18, 62, 178),
            tl_gain_line: Color32::from_rgb(16, 104, 48),
            tl_env_primary: Color32::from_rgb(20, 74, 168),
            tl_env_secondary: Color32::from_rgb(168, 28, 28),
            tl_event_border: Color32::from_rgb(56, 60, 68),
            tl_event_sel_border: Color32::from_rgb(16, 46, 112),
            tl_preview_bg: Color32::from_rgb(20, 22, 26),
            tl_event_body_k: 0.55,
            tl_event_header_k: 0.9,
            classic_bevel: true,
        },
        ThemeId::VegasClassico => ThemeColors {
            // Cinza clássico da geração 1.0–7.0 com chanfros 3D, seleção marinho
            // e visor de tempo escuro de alto contraste
            dark_mode: false,
            high_contrast: false,
            panel_fill: Color32::from_rgb(204, 202, 196),
            window_fill: Color32::from_rgb(214, 212, 206),
            extreme_bg: Color32::WHITE,
            widget_bg: Color32::from_rgb(212, 208, 200),
            widget_hover_bg: Color32::from_rgb(194, 198, 208),
            selection_bg: Color32::from_rgb(10, 36, 106),
            text_primary: Color32::from_rgb(14, 14, 16),
            text_muted: Color32::from_rgb(54, 54, 58),
            border: Color32::from_rgb(128, 128, 128),
            accent: Color32::from_rgb(16, 60, 146),
            timecode_fg: Color32::from_rgb(14, 14, 16),
            offline_fg: Color32::from_rgb(172, 34, 16),
            tl_bg: Color32::from_rgb(192, 192, 192),
            tl_header_bg: Color32::from_rgb(208, 204, 196),
            tl_header_sel: Color32::from_rgb(158, 178, 210),
            tl_header_idle: Color32::from_rgb(214, 210, 202),
            tl_loop_bar: Color32::from_rgb(184, 182, 176),
            tl_marker_bar: Color32::from_rgb(198, 196, 190),
            tl_ruler_bg: Color32::from_rgb(222, 220, 214),
            tl_grid_line: Color32::from_rgb(156, 156, 156),
            tl_lane_even: Color32::from_rgb(202, 200, 196),
            tl_lane_odd: Color32::from_rgb(192, 190, 186),
            tl_playhead_edit: Color32::from_rgb(192, 42, 20),
            tl_playhead_live: Color32::from_rgb(208, 30, 18),
            tl_loop_active: Color32::from_rgb(24, 76, 162),
            tl_loop_inactive: Color32::from_rgb(112, 110, 106),
            tl_marker: Color32::from_rgb(192, 38, 28),
            tl_region: Color32::from_rgb(22, 118, 48),
            tl_sel_band: Color32::from_rgba_unmultiplied(10, 36, 106, 40),
            tl_fade: Color32::from_rgb(142, 92, 0),
            tl_crossfade: Color32::from_rgb(18, 66, 202),
            tl_gain_line: Color32::from_rgb(18, 114, 52),
            tl_env_primary: Color32::from_rgb(20, 82, 184),
            tl_env_secondary: Color32::from_rgb(184, 34, 34),
            tl_event_border: Color32::from_rgb(68, 66, 62),
            tl_event_sel_border: Color32::from_rgb(10, 36, 106),
            tl_preview_bg: Color32::from_rgb(18, 18, 20),
            tl_event_body_k: 0.55,
            tl_event_header_k: 0.9,
            classic_bevel: true,
        },
        ThemeId::VegasBranco => ThemeColors {
            // Superfície branca limpa (equivalente ao Interface Type: White)
            dark_mode: false,
            high_contrast: false,
            panel_fill: Color32::from_rgb(245, 246, 248),
            window_fill: Color32::from_rgb(252, 253, 255),
            extreme_bg: Color32::WHITE,
            widget_bg: Color32::from_rgb(232, 235, 240),
            widget_hover_bg: Color32::from_rgb(214, 222, 234),
            selection_bg: Color32::from_rgb(32, 102, 196),
            text_primary: Color32::from_rgb(20, 22, 28),
            text_muted: Color32::from_rgb(72, 76, 86),
            border: Color32::from_rgb(196, 202, 212),
            accent: Color32::from_rgb(20, 92, 186),
            timecode_fg: Color32::from_rgb(20, 22, 28),
            offline_fg: Color32::from_rgb(184, 38, 18),
            tl_bg: Color32::from_rgb(234, 236, 242),
            tl_header_bg: Color32::from_rgb(242, 244, 248),
            tl_header_sel: Color32::from_rgb(202, 220, 246),
            tl_header_idle: Color32::from_rgb(248, 249, 252),
            tl_loop_bar: Color32::from_rgb(226, 229, 236),
            tl_marker_bar: Color32::from_rgb(236, 238, 244),
            tl_ruler_bg: Color32::from_rgb(248, 250, 253),
            tl_grid_line: Color32::from_rgb(210, 214, 224),
            tl_lane_even: Color32::from_rgb(238, 240, 244),
            tl_lane_odd: Color32::from_rgb(246, 247, 250),
            tl_playhead_edit: Color32::from_rgb(186, 52, 0),
            tl_playhead_live: Color32::from_rgb(206, 36, 20),
            tl_loop_active: Color32::from_rgb(32, 102, 196),
            tl_loop_inactive: Color32::from_rgb(148, 154, 168),
            tl_marker: Color32::from_rgb(196, 98, 10),
            tl_region: Color32::from_rgb(20, 128, 54),
            tl_sel_band: Color32::from_rgba_unmultiplied(32, 102, 196, 38),
            tl_fade: Color32::from_rgb(154, 104, 0),
            tl_crossfade: Color32::from_rgb(18, 76, 214),
            tl_gain_line: Color32::from_rgb(18, 128, 60),
            tl_env_primary: Color32::from_rgb(22, 92, 196),
            tl_env_secondary: Color32::from_rgb(196, 38, 38),
            tl_event_border: Color32::from_rgb(112, 118, 130),
            tl_event_sel_border: Color32::from_rgb(14, 48, 126),
            tl_preview_bg: Color32::from_rgb(18, 20, 24),
            tl_event_body_k: 0.55,
            tl_event_header_k: 0.9,
            classic_bevel: false,
        },
        ThemeId::VegasProfundo => ThemeColors {
            dark_mode: true,
            high_contrast: false,
            panel_fill: Color32::from_rgb(22, 23, 26),
            window_fill: Color32::from_rgb(28, 29, 34),
            extreme_bg: Color32::from_rgb(12, 12, 14),
            widget_bg: Color32::from_rgb(38, 40, 46),
            widget_hover_bg: Color32::from_rgb(56, 59, 68),
            selection_bg: Color32::from_rgb(48, 102, 185),
            text_primary: Color32::from_rgb(226, 228, 232),
            text_muted: Color32::from_rgb(155, 160, 170),
            border: Color32::from_rgb(64, 66, 76),
            accent: Color32::from_rgb(110, 195, 255),
            timecode_fg: Color32::from_rgb(226, 230, 236),
            offline_fg: Color32::from_rgb(245, 120, 80),
            tl_bg: Color32::from_rgb(18, 19, 22),
            tl_header_bg: Color32::from_rgb(24, 25, 29),
            tl_header_sel: Color32::from_rgb(42, 56, 76),
            tl_header_idle: Color32::from_rgb(31, 32, 37),
            tl_loop_bar: Color32::from_rgb(16, 16, 19),
            tl_marker_bar: Color32::from_rgb(22, 22, 26),
            tl_ruler_bg: Color32::from_rgb(33, 34, 39),
            tl_grid_line: Color32::from_rgb(30, 31, 36),
            tl_lane_even: Color32::from_rgb(26, 28, 34),
            tl_lane_odd: Color32::from_rgb(20, 21, 25),
            tl_playhead_edit: Color32::from_rgb(255, 215, 85),
            tl_playhead_live: Color32::from_rgb(255, 110, 55),
            tl_loop_active: Color32::from_rgb(80, 138, 225),
            tl_loop_inactive: Color32::from_rgb(68, 70, 82),
            tl_marker: Color32::from_rgb(235, 155, 45),
            tl_region: Color32::from_rgb(70, 185, 100),
            tl_sel_band: Color32::from_rgba_unmultiplied(80, 130, 215, 45),
            tl_fade: Color32::from_rgb(255, 245, 150),
            tl_crossfade: Color32::from_rgb(85, 175, 255),
            tl_gain_line: Color32::from_rgb(120, 235, 150),
            tl_env_primary: Color32::from_rgb(85, 165, 255),
            tl_env_secondary: Color32::from_rgb(255, 105, 105),
            tl_event_border: Color32::from_gray(8),
            tl_event_sel_border: Color32::from_rgb(245, 245, 255),
            tl_preview_bg: Color32::from_rgb(8, 8, 10),
            tl_event_body_k: 0.55,
            tl_event_header_k: 0.9,
            classic_bevel: false,
        },
        ThemeId::VegasMedio => ThemeColors {
            dark_mode: true,
            high_contrast: false,
            panel_fill: Color32::from_rgb(66, 68, 74),
            window_fill: Color32::from_rgb(74, 76, 82),
            extreme_bg: Color32::from_rgb(44, 46, 50),
            widget_bg: Color32::from_rgb(84, 87, 94),
            widget_hover_bg: Color32::from_rgb(102, 106, 116),
            selection_bg: Color32::from_rgb(64, 118, 198),
            text_primary: Color32::from_rgb(242, 244, 248),
            text_muted: Color32::from_rgb(206, 210, 218),
            border: Color32::from_rgb(112, 116, 126),
            accent: Color32::from_rgb(135, 210, 255),
            timecode_fg: Color32::from_rgb(238, 240, 244),
            offline_fg: Color32::from_rgb(255, 145, 105),
            tl_bg: Color32::from_rgb(54, 56, 62),
            tl_header_bg: Color32::from_rgb(64, 66, 72),
            tl_header_sel: Color32::from_rgb(76, 94, 118),
            tl_header_idle: Color32::from_rgb(72, 74, 82),
            tl_loop_bar: Color32::from_rgb(48, 50, 56),
            tl_marker_bar: Color32::from_rgb(56, 58, 64),
            tl_ruler_bg: Color32::from_rgb(78, 80, 88),
            tl_grid_line: Color32::from_rgb(68, 70, 78),
            tl_lane_even: Color32::from_rgb(64, 66, 74),
            tl_lane_odd: Color32::from_rgb(56, 58, 65),
            tl_playhead_edit: Color32::from_rgb(255, 230, 100),
            tl_playhead_live: Color32::from_rgb(255, 125, 65),
            tl_loop_active: Color32::from_rgb(95, 150, 230),
            tl_loop_inactive: Color32::from_rgb(110, 112, 125),
            tl_marker: Color32::from_rgb(245, 165, 55),
            tl_region: Color32::from_rgb(80, 195, 105),
            tl_sel_band: Color32::from_rgba_unmultiplied(100, 145, 220, 50),
            tl_fade: Color32::from_rgb(255, 250, 165),
            tl_crossfade: Color32::from_rgb(100, 185, 255),
            tl_gain_line: Color32::from_rgb(145, 245, 165),
            tl_env_primary: Color32::from_rgb(100, 175, 255),
            tl_env_secondary: Color32::from_rgb(255, 120, 120),
            tl_event_border: Color32::from_gray(24),
            tl_event_sel_border: Color32::WHITE,
            tl_preview_bg: Color32::from_rgb(20, 20, 22),
            tl_event_body_k: 0.55,
            tl_event_header_k: 0.9,
            classic_bevel: false,
        },
        ThemeId::VegasClaro => ThemeColors {
            // Bege/creme quente clássico da geração v8 (estilo Windows XP) com
            // listas claras e seleção azul vibrante
            dark_mode: false,
            high_contrast: false,
            panel_fill: Color32::from_rgb(236, 233, 216),
            window_fill: Color32::from_rgb(242, 240, 228),
            extreme_bg: Color32::WHITE,
            widget_bg: Color32::from_rgb(224, 220, 204),
            widget_hover_bg: Color32::from_rgb(206, 216, 234),
            selection_bg: Color32::from_rgb(0, 84, 228),
            text_primary: Color32::from_rgb(18, 20, 24),
            text_muted: Color32::from_rgb(60, 62, 68),
            border: Color32::from_rgb(152, 148, 134),
            accent: Color32::from_rgb(0, 72, 188),
            timecode_fg: Color32::from_rgb(18, 20, 24),
            offline_fg: Color32::from_rgb(180, 40, 18),
            tl_bg: Color32::from_rgb(220, 217, 202),
            tl_header_bg: Color32::from_rgb(232, 229, 214),
            tl_header_sel: Color32::from_rgb(178, 204, 238),
            tl_header_idle: Color32::from_rgb(238, 235, 220),
            tl_loop_bar: Color32::from_rgb(208, 204, 188),
            tl_marker_bar: Color32::from_rgb(220, 216, 200),
            tl_ruler_bg: Color32::from_rgb(244, 242, 232),
            tl_grid_line: Color32::from_rgb(186, 182, 168),
            tl_lane_even: Color32::from_rgb(228, 225, 212),
            tl_lane_odd: Color32::from_rgb(218, 215, 200),
            tl_playhead_edit: Color32::from_rgb(188, 68, 0),
            tl_playhead_live: Color32::from_rgb(205, 38, 20),
            tl_loop_active: Color32::from_rgb(0, 84, 228),
            tl_loop_inactive: Color32::from_rgb(124, 126, 136),
            tl_marker: Color32::from_rgb(180, 92, 10),
            tl_region: Color32::from_rgb(20, 120, 48),
            tl_sel_band: Color32::from_rgba_unmultiplied(0, 84, 228, 38),
            tl_fade: Color32::from_rgb(155, 110, 0),
            tl_crossfade: Color32::from_rgb(18, 72, 210),
            tl_gain_line: Color32::from_rgb(18, 125, 58),
            tl_env_primary: Color32::from_rgb(20, 90, 195),
            tl_env_secondary: Color32::from_rgb(195, 40, 40),
            tl_event_border: Color32::from_rgb(80, 80, 74),
            tl_event_sel_border: Color32::from_rgb(0, 56, 160),
            tl_preview_bg: Color32::from_rgb(20, 22, 26),
            tl_event_body_k: 0.55,
            tl_event_header_k: 0.9,
            classic_bevel: true,
        },
        ThemeId::VegasArdosia => ThemeColors {
            dark_mode: true,
            high_contrast: false,
            panel_fill: Color32::from_rgb(31, 38, 49),
            window_fill: Color32::from_rgb(37, 45, 58),
            extreme_bg: Color32::from_rgb(19, 24, 32),
            widget_bg: Color32::from_rgb(46, 56, 72),
            widget_hover_bg: Color32::from_rgb(60, 74, 96),
            selection_bg: Color32::from_rgb(55, 126, 205),
            text_primary: Color32::from_rgb(230, 236, 245),
            text_muted: Color32::from_rgb(170, 182, 200),
            border: Color32::from_rgb(72, 86, 108),
            accent: Color32::from_rgb(105, 205, 245),
            timecode_fg: Color32::from_rgb(230, 236, 245),
            offline_fg: Color32::from_rgb(250, 130, 95),
            tl_bg: Color32::from_rgb(24, 30, 40),
            tl_header_bg: Color32::from_rgb(29, 36, 48),
            tl_header_sel: Color32::from_rgb(52, 68, 92),
            tl_header_idle: Color32::from_rgb(38, 47, 61),
            tl_loop_bar: Color32::from_rgb(22, 27, 36),
            tl_marker_bar: Color32::from_rgb(28, 35, 46),
            tl_ruler_bg: Color32::from_rgb(40, 49, 64),
            tl_grid_line: Color32::from_rgb(38, 46, 60),
            tl_lane_even: Color32::from_rgb(34, 42, 54),
            tl_lane_odd: Color32::from_rgb(27, 34, 44),
            tl_playhead_edit: Color32::from_rgb(255, 222, 95),
            tl_playhead_live: Color32::from_rgb(255, 120, 68),
            tl_loop_active: Color32::from_rgb(76, 152, 232),
            tl_loop_inactive: Color32::from_rgb(76, 90, 114),
            tl_marker: Color32::from_rgb(238, 162, 52),
            tl_region: Color32::from_rgb(72, 188, 130),
            tl_sel_band: Color32::from_rgba_unmultiplied(80, 150, 230, 42),
            tl_fade: Color32::from_rgb(250, 248, 165),
            tl_crossfade: Color32::from_rgb(95, 190, 255),
            tl_gain_line: Color32::from_rgb(120, 238, 180),
            tl_env_primary: Color32::from_rgb(95, 185, 255),
            tl_env_secondary: Color32::from_rgb(255, 115, 120),
            tl_event_border: Color32::from_rgb(14, 18, 24),
            tl_event_sel_border: Color32::WHITE,
            tl_preview_bg: Color32::from_rgb(12, 16, 22),
            tl_event_body_k: 0.55,
            tl_event_header_k: 0.9,
            classic_bevel: false,
        },
        ThemeId::AltoContrasteEscuro => ThemeColors {
            // Contraste WCAG AAA >= 7:1 sobre #000000 / #0A0A0A
            dark_mode: true,
            high_contrast: true,
            panel_fill: Color32::BLACK,
            window_fill: Color32::from_rgb(10, 10, 10),
            extreme_bg: Color32::BLACK,
            widget_bg: Color32::from_rgb(18, 18, 18),
            widget_hover_bg: Color32::from_rgb(38, 38, 38),
            selection_bg: Color32::from_rgb(0, 82, 170),
            text_primary: Color32::WHITE,
            text_muted: Color32::from_rgb(218, 218, 218),
            border: Color32::WHITE,
            accent: Color32::from_rgb(255, 230, 0),
            timecode_fg: Color32::WHITE,
            offline_fg: Color32::from_rgb(255, 130, 110),
            tl_bg: Color32::BLACK,
            tl_header_bg: Color32::BLACK,
            tl_header_sel: Color32::from_rgb(0, 64, 140),
            tl_header_idle: Color32::from_rgb(16, 16, 16),
            tl_loop_bar: Color32::BLACK,
            tl_marker_bar: Color32::from_rgb(10, 10, 10),
            tl_ruler_bg: Color32::from_rgb(18, 18, 18),
            tl_grid_line: Color32::from_rgb(90, 90, 90),
            tl_lane_even: Color32::from_rgb(12, 12, 12),
            tl_lane_odd: Color32::BLACK,
            tl_playhead_edit: Color32::from_rgb(255, 235, 0),
            tl_playhead_live: Color32::from_rgb(0, 240, 255),
            tl_loop_active: Color32::from_rgb(0, 220, 255),
            tl_loop_inactive: Color32::from_rgb(170, 170, 170),
            tl_marker: Color32::from_rgb(255, 215, 0),
            tl_region: Color32::from_rgb(110, 255, 120),
            tl_sel_band: Color32::from_rgba_unmultiplied(0, 220, 255, 55),
            tl_fade: Color32::from_rgb(255, 240, 0),
            tl_crossfade: Color32::from_rgb(0, 235, 255),
            tl_gain_line: Color32::from_rgb(120, 255, 130),
            tl_env_primary: Color32::from_rgb(0, 225, 255),
            tl_env_secondary: Color32::from_rgb(255, 130, 130),
            tl_event_border: Color32::WHITE,
            tl_event_sel_border: Color32::from_rgb(255, 235, 0),
            tl_preview_bg: Color32::BLACK,
            tl_event_body_k: 0.55,
            tl_event_header_k: 0.9,
            classic_bevel: false,
        },
        ThemeId::AltoContrasteClaro => ThemeColors {
            // Contraste WCAG AAA >= 7:1 sobre #FFFFFF / #F4F4F4
            dark_mode: false,
            high_contrast: true,
            panel_fill: Color32::WHITE,
            window_fill: Color32::from_rgb(246, 246, 246),
            extreme_bg: Color32::WHITE,
            widget_bg: Color32::from_rgb(236, 236, 236),
            widget_hover_bg: Color32::from_rgb(212, 222, 242),
            selection_bg: Color32::from_rgb(0, 52, 148),
            text_primary: Color32::BLACK,
            text_muted: Color32::from_rgb(35, 35, 35),
            border: Color32::BLACK,
            accent: Color32::from_rgb(0, 42, 132),
            timecode_fg: Color32::BLACK,
            offline_fg: Color32::from_rgb(148, 0, 0),
            tl_bg: Color32::WHITE,
            tl_header_bg: Color32::WHITE,
            tl_header_sel: Color32::from_rgb(185, 210, 250),
            tl_header_idle: Color32::from_rgb(242, 242, 242),
            tl_loop_bar: Color32::from_rgb(245, 245, 245),
            tl_marker_bar: Color32::from_rgb(238, 238, 238),
            tl_ruler_bg: Color32::from_rgb(232, 232, 232),
            tl_grid_line: Color32::from_rgb(130, 130, 130),
            tl_lane_even: Color32::from_rgb(246, 246, 246),
            tl_lane_odd: Color32::WHITE,
            tl_playhead_edit: Color32::from_rgb(135, 0, 0),
            tl_playhead_live: Color32::from_rgb(0, 45, 150),
            tl_loop_active: Color32::from_rgb(0, 52, 148),
            tl_loop_inactive: Color32::from_rgb(75, 75, 75),
            tl_marker: Color32::from_rgb(130, 52, 0),
            tl_region: Color32::from_rgb(0, 92, 24),
            tl_sel_band: Color32::from_rgba_unmultiplied(0, 52, 148, 45),
            tl_fade: Color32::from_rgb(120, 62, 0),
            tl_crossfade: Color32::from_rgb(0, 38, 160),
            tl_gain_line: Color32::from_rgb(0, 92, 24),
            tl_env_primary: Color32::from_rgb(0, 48, 145),
            tl_env_secondary: Color32::from_rgb(150, 0, 0),
            tl_event_border: Color32::BLACK,
            tl_event_sel_border: Color32::from_rgb(160, 0, 0),
            tl_preview_bg: Color32::BLACK,
            tl_event_body_k: 0.55,
            tl_event_header_k: 0.9,
            classic_bevel: false,
        },
    }
}

/// Aplica o tema escolhido ao contexto `egui` e devolve a paleta semântica.
pub fn apply_theme(ctx: &egui::Context, id: ThemeId) -> ThemeColors {
    let c = palette(id);
    let mut v = if c.dark_mode {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let sel_fg = contrast_on(c.selection_bg);
    // alça retangular nos faders e controles deslizantes, como no VEGAS Pro
    // (antes era redonda): na vertical vira uma tampa larga e baixa
    v.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.55 };
    v.panel_fill = c.panel_fill;
    v.window_fill = c.window_fill;
    v.extreme_bg_color = c.extreme_bg;
    v.override_text_color = None;
    v.selection.bg_fill = c.selection_bg;
    v.selection.stroke = Stroke::new(if c.high_contrast { 1.5_f32 } else { 1.0_f32 }, sel_fg);

    let border_w = if c.high_contrast { 1.5_f32 } else { 1.0_f32 };
    let cr = if c.high_contrast { 0.into() } else { 1.into() };

    v.widgets.noninteractive.bg_fill = c.panel_fill;
    v.widgets.noninteractive.bg_stroke = Stroke::new(border_w, c.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, c.text_primary);
    v.widgets.noninteractive.corner_radius = cr;

    v.widgets.inactive.bg_fill = c.widget_bg;
    v.widgets.inactive.weak_bg_fill = if c.high_contrast {
        c.widget_bg
    } else {
        Color32::TRANSPARENT
    };
    v.widgets.inactive.bg_stroke = if c.high_contrast {
        Stroke::new(border_w, c.border)
    } else {
        Stroke::new(
            1.0_f32,
            Color32::from_rgba_unmultiplied(c.border.r(), c.border.g(), c.border.b(), 150),
        )
    };
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, c.text_primary);
    v.widgets.inactive.corner_radius = cr;

    v.widgets.hovered.bg_fill = c.widget_hover_bg;
    v.widgets.hovered.weak_bg_fill = c.widget_hover_bg;
    v.widgets.hovered.bg_stroke = Stroke::new(border_w, c.accent);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5_f32, c.text_primary);
    v.widgets.hovered.corner_radius = cr;

    // No egui, `Visuals::strong_text_color()` lê `widgets.active.fg_stroke.color`.
    // Sempre usar `c.text_primary` aqui garante que rótulos `.strong()` tenham
    // contraste pleno sobre `panel_fill` em todos os 11 temas (inclusive escuros
    // com seleção clara como `VegasModerno` e todos os temas claros).
    let min_active_ratio = if c.high_contrast { 7.0 } else { 4.5 };
    let active_bg = if c.dark_mode {
        if contrast_ratio(c.text_primary, c.selection_bg) >= min_active_ratio {
            c.selection_bg
        } else {
            c.tl_header_sel
        }
    } else if c.high_contrast {
        Color32::from_rgb(196, 212, 240)
    } else {
        c.tl_header_sel
    };
    v.widgets.active.bg_fill = active_bg;
    v.widgets.active.weak_bg_fill = active_bg;
    v.widgets.active.bg_stroke = Stroke::new(border_w, if c.dark_mode { sel_fg } else { c.accent });
    v.widgets.active.fg_stroke = Stroke::new(1.5_f32, c.text_primary);
    v.widgets.active.corner_radius = cr;

    v.widgets.open.bg_fill = c.widget_hover_bg;
    v.widgets.open.bg_stroke = Stroke::new(border_w, c.accent);
    v.widgets.open.fg_stroke = Stroke::new(1.0_f32, c.text_primary);
    v.widgets.open.corner_radius = cr;

    v.window_corner_radius = if c.high_contrast { 0.into() } else { 2.into() };
    v.menu_corner_radius = if c.high_contrast { 0.into() } else { 2.into() };
    v.window_stroke = Stroke::new(border_w, c.border);
    v.window_highlight_topmost = true;

    ctx.set_visuals(v);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(5.0, 3.0);
    style.spacing.button_padding = egui::vec2(4.0, 2.0);
    style.spacing.window_margin = egui::Margin::same(6);
    style.spacing.indent = 12.0;
    style.spacing.interact_size = egui::vec2(16.0, 16.0);
    style.spacing.scroll = egui::style::ScrollStyle {
        bar_width: 10.0,
        ..egui::style::ScrollStyle::solid()
    };
    ctx.set_style(style);
    c
}

/// Escolhe branco ou preto conforme a maior razão de contraste WCAG 2.1 contra `bg`.
pub fn contrast_on(bg: Color32) -> Color32 {
    if contrast_ratio(Color32::WHITE, bg) >= contrast_ratio(Color32::BLACK, bg) {
        Color32::WHITE
    } else {
        Color32::BLACK
    }
}

/// Luminância relativa sRGB segundo WCAG 2.1.
pub fn relative_luminance(col: Color32) -> f64 {
    fn channel(v: u8) -> f64 {
        let s = f64::from(v) / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(col.r()) + 0.7152 * channel(col.g()) + 0.0722 * channel(col.b())
}

/// Razão de contraste WCAG 2.1 entre duas cores (1.0 .. 21.0).
pub fn contrast_ratio(a: Color32, b: Color32) -> f64 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sete_temas_e_contraste_aaa() {
        assert_eq!(ThemeId::ALL.len(), 11);
        for id in [ThemeId::AltoContrasteEscuro, ThemeId::AltoContrasteClaro] {
            let p = palette(id);
            assert!(p.high_contrast);
            assert!(
                contrast_ratio(p.text_primary, p.panel_fill) >= 7.0,
                "texto principal de {:?} precisa >= 7:1",
                id
            );
            assert!(
                contrast_ratio(p.text_muted, p.window_fill) >= 7.0,
                "texto secundário de {:?} precisa >= 7:1",
                id
            );
            assert!(
                contrast_ratio(p.accent, p.panel_fill) >= 7.0,
                "realce de {:?} precisa >= 7:1",
                id
            );
            assert!(
                contrast_ratio(p.timecode_fg, p.panel_fill) >= 7.0,
                "timecode de {:?} precisa >= 7:1",
                id
            );
        }
        for id in ThemeId::ALL {
            let p = palette(id);
            assert!(
                contrast_ratio(p.text_primary, p.panel_fill) >= 4.5,
                "texto principal de {:?} precisa >= 4.5:1",
                id
            );
            assert!(
                contrast_ratio(p.text_muted, p.window_fill) >= 4.5,
                "texto secundário de {:?} precisa >= 4.5:1",
                id
            );
            assert!(
                contrast_ratio(contrast_on(p.selection_bg), p.selection_bg) >= 4.5,
                "texto selecionado de {:?} precisa >= 4.5:1",
                id
            );
            assert!(
                contrast_ratio(p.tl_playhead_edit, p.tl_lane_even) >= 3.0,
                "playhead de {:?} precisa de contraste visível na timeline",
                id
            );
            let ctx = egui::Context::default();
            apply_theme(&ctx, id);
            let vis = &ctx.style().visuals;
            let min_ratio = if p.high_contrast { 7.0 } else { 4.5 };
            assert!(
                contrast_ratio(vis.strong_text_color(), p.panel_fill) >= min_ratio,
                "texto .strong() de {:?} precisa >= {}:1 sobre panel_fill",
                id,
                min_ratio
            );
            assert!(
                contrast_ratio(
                    vis.widgets.active.fg_stroke.color,
                    vis.widgets.active.bg_fill
                ) >= min_ratio,
                "texto ativo de {:?} precisa >= {}:1 sobre fundo ativo",
                id,
                min_ratio
            );
        }
        assert_eq!(ThemeId::from_pref("invalido"), ThemeId::VegasEscuro);
    }

    /// Marcadores são alaranjados e regiões, verdes, em todos os temas. Antes,
    /// seis temas tinham as duas cores trocadas.
    #[test]
    fn marcador_laranja_e_regiao_verde_em_todos_os_temas() {
        for id in ThemeId::ALL {
            let p = palette(id);
            assert!(
                p.tl_marker.r() > p.tl_marker.g() && p.tl_marker.r() > p.tl_marker.b(),
                "marcador de {:?} precisa ser alaranjado: {:?}",
                id,
                p.tl_marker
            );
            assert!(
                p.tl_region.g() > p.tl_region.r() && p.tl_region.g() > p.tl_region.b(),
                "região de {:?} precisa ser verde: {:?}",
                id,
                p.tl_region
            );
        }
    }
}
