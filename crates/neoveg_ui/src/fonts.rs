//! Configuração tipográfica do Neoveg com fontes abertas (`OFL-1.1`) embutidas
//! em `assets/fonts/` e utilitários para renderização tabular sem oscilação
//! horizontal em timecodes (`Visor de tempo`, `Régua`, barra de transporte e `Trimmer`).

use eframe::egui;
use std::collections::HashSet;
use std::sync::Arc;

/// Selawik Regular (`OFL-1.1`) — fonte primária proporcional metricamente compatível com Segoe UI.
const SELAWIK_REGULAR_BYTES: &[u8] = include_bytes!("../../../assets/fonts/selawk.ttf");
/// Selawik SemiBold (`OFL-1.1`) — destaques e títulos da interface.
const SELAWIK_SEMIBOLD_BYTES: &[u8] = include_bytes!("../../../assets/fonts/selawksb.ttf");
/// Inter Regular (`OFL-1.1`) — Visor de tempo, Régua da timeline e fallback de setas/símbolos UI.
const INTER_REGULAR_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
/// Noto Sans Regular (`OFL-1.1`) — fallback multilíngue e de diacríticos.
const NOTO_SANS_REGULAR_BYTES: &[u8] = include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf");
/// Noto Sans Math Regular (`OFL-1.1`) — fallback técnico para símbolos ausentes (`⋮`, `─`).
const NOTO_SANS_MATH_REGULAR_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/NotoSansMath-Regular.ttf");

const KEY_SELAWIK_REGULAR: &str = "Selawik-Regular";
const KEY_SELAWIK_SEMIBOLD: &str = "Selawik-SemiBold";
const KEY_INTER_REGULAR: &str = "Inter-Regular";
const KEY_NOTO_SANS_REGULAR: &str = "NotoSans-Regular";
const KEY_NOTO_SANS_MATH: &str = "NotoSansMath-Regular";
const KEY_HACK_BUILTIN: &str = "Hack";

/// Família nomeada para títulos e destaques (`Selawik-SemiBold`).
pub const FAMILY_SEMIBOLD: &str = "semibold";
/// Família nomeada para o Visor de tempo e chips numéricos (`Inter-Regular`).
pub const FAMILY_VISOR: &str = "visor";
/// Família nomeada para a Régua da timeline (`Inter-Regular`).
pub const FAMILY_RULER: &str = "ruler";

/// Retorna o `FontId` da família `semibold` (`Selawik-SemiBold` com fallback completo).
pub fn semibold_font(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(FAMILY_SEMIBOLD.into()))
}

/// Retorna o `FontId` da família `visor` (`Inter-Regular` com fallback completo).
pub fn visor_font(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(FAMILY_VISOR.into()))
}

/// Retorna o `FontId` da família `ruler` (`Inter-Regular` com fallback completo).
pub fn ruler_font(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(FAMILY_RULER.into()))
}

/// Constrói um `RichText` utilizando `Selawik-SemiBold`.
pub fn semibold_text(text: impl Into<String>, size: f32) -> egui::RichText {
    egui::RichText::new(text).font(semibold_font(size))
}

fn push_unique_chain(target: &mut Vec<String>, items: &[&str], tail: &[String]) {
    let mut seen = HashSet::new();
    for &item in items {
        if seen.insert(item.to_owned()) {
            target.push(item.to_owned());
        }
    }
    for item in tail {
        if seen.insert(item.clone()) {
            target.push(item.clone());
        }
    }
}

/// Registra as fontes `OFL-1.1` do Neoveg e ajusta o estilo tipográfico base do `egui::Context`.
///
/// É idempotente e segura contra falhas (zero `unwrap`/`expect`/`panic`).
pub fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let default_prop = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let default_mono = fonts
        .families
        .get(&egui::FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();

    fonts.font_data.insert(
        KEY_SELAWIK_REGULAR.to_owned(),
        Arc::new(egui::FontData::from_static(SELAWIK_REGULAR_BYTES)),
    );
    fonts.font_data.insert(
        KEY_SELAWIK_SEMIBOLD.to_owned(),
        Arc::new(egui::FontData::from_static(SELAWIK_SEMIBOLD_BYTES)),
    );
    fonts.font_data.insert(
        KEY_INTER_REGULAR.to_owned(),
        Arc::new(egui::FontData::from_static(INTER_REGULAR_BYTES)),
    );
    fonts.font_data.insert(
        KEY_NOTO_SANS_REGULAR.to_owned(),
        Arc::new(egui::FontData::from_static(NOTO_SANS_REGULAR_BYTES)),
    );
    fonts.font_data.insert(
        KEY_NOTO_SANS_MATH.to_owned(),
        Arc::new(egui::FontData::from_static(NOTO_SANS_MATH_REGULAR_BYTES)),
    );

    let mut prop_chain = Vec::new();
    push_unique_chain(
        &mut prop_chain,
        &[
            KEY_SELAWIK_REGULAR,
            KEY_NOTO_SANS_REGULAR,
            KEY_INTER_REGULAR,
            KEY_NOTO_SANS_MATH,
            KEY_HACK_BUILTIN,
        ],
        &default_prop,
    );
    fonts
        .families
        .insert(egui::FontFamily::Proportional, prop_chain);

    let mut semibold_chain = Vec::new();
    push_unique_chain(
        &mut semibold_chain,
        &[
            KEY_SELAWIK_SEMIBOLD,
            KEY_SELAWIK_REGULAR,
            KEY_NOTO_SANS_REGULAR,
            KEY_INTER_REGULAR,
            KEY_NOTO_SANS_MATH,
            KEY_HACK_BUILTIN,
        ],
        &default_prop,
    );
    fonts.families.insert(
        egui::FontFamily::Name(FAMILY_SEMIBOLD.into()),
        semibold_chain,
    );

    let mut visor_chain = Vec::new();
    push_unique_chain(
        &mut visor_chain,
        &[
            KEY_INTER_REGULAR,
            KEY_NOTO_SANS_REGULAR,
            KEY_NOTO_SANS_MATH,
            KEY_HACK_BUILTIN,
        ],
        &default_prop,
    );
    fonts
        .families
        .insert(egui::FontFamily::Name(FAMILY_VISOR.into()), visor_chain);

    let mut ruler_chain = Vec::new();
    push_unique_chain(
        &mut ruler_chain,
        &[
            KEY_INTER_REGULAR,
            KEY_NOTO_SANS_REGULAR,
            KEY_NOTO_SANS_MATH,
            KEY_HACK_BUILTIN,
        ],
        &default_prop,
    );
    fonts
        .families
        .insert(egui::FontFamily::Name(FAMILY_RULER.into()), ruler_chain);

    let mut mono_chain = Vec::new();
    let mut mono_tail = Vec::with_capacity(
        default_mono
            .len()
            .saturating_add(4)
            .saturating_add(default_prop.len()),
    );
    mono_tail.extend(default_mono);
    for extra in [KEY_NOTO_SANS_REGULAR, KEY_INTER_REGULAR, KEY_NOTO_SANS_MATH] {
        mono_tail.push(extra.to_owned());
    }
    mono_tail.extend(default_prop);
    push_unique_chain(&mut mono_chain, &[], &mono_tail);
    fonts
        .families
        .insert(egui::FontFamily::Monospace, mono_chain);

    ctx.set_fonts(fonts);
    ctx.style_mut(|style| {
        style
            .text_styles
            .insert(egui::TextStyle::Heading, semibold_font(18.0));
    });
}

/// Limite superior seguro de tamanho de fonte (em pontos) para evitar estourar o atlas do `epaint`.
const MAX_TABULAR_FONT_SIZE: f32 = 512.0;

fn sanitize_font_size(size: f32) -> Option<f32> {
    if size.is_finite() && size > 0.0 {
        Some(size.min(MAX_TABULAR_FONT_SIZE))
    } else {
        None
    }
}

fn sanitize_font_id(mut font_id: egui::FontId) -> Option<egui::FontId> {
    font_id.size = sanitize_font_size(font_id.size)?;
    Some(font_id)
}

fn safe_pixels_per_point(fonts: &egui::epaint::Fonts) -> f32 {
    let ppp = fonts.pixels_per_point();
    if ppp.is_finite() && ppp > 0.0 {
        ppp
    } else {
        1.0
    }
}

#[inline]
fn round_to_pixel(point: f32, ppp: f32) -> f32 {
    (point * ppp).round() / ppp
}

#[inline]
fn ceil_ui(width: f32) -> f32 {
    if !width.is_finite() || width <= 0.0 {
        return 0.0;
    }
    let step = egui::emath::GUI_ROUNDING;
    (width / step).ceil() * step
}

fn safe_glyph_width(fonts: &egui::epaint::Fonts, font_id: &egui::FontId, ch: char) -> f32 {
    let w = fonts.glyph_width(font_id, ch);
    if w.is_finite() {
        w.max(0.0)
    } else {
        0.0
    }
}

/// Calcula a largura tabular fixa da célula de dígito (`max('0'..='9')` ajustada
/// à grade de pixels) para o `font_id`, evitando deriva cumulativa em `epaint`.
pub fn tabular_digit_advance(fonts: &egui::epaint::Fonts, font_id: &egui::FontId) -> f32 {
    let Some(font_id) = sanitize_font_id(font_id.clone()) else {
        return 0.0;
    };
    let raw_max = ('0'..='9')
        .map(|d| safe_glyph_width(fonts, &font_id, d))
        .fold(0.0_f32, f32::max);
    if raw_max <= 0.0 {
        return 0.0;
    }
    let ppp = safe_pixels_per_point(fonts);
    (raw_max * ppp).ceil().max(1.0) / ppp
}

/// Constrói um `LayoutJob` de linha única mantendo a string inteira em `job.text`
/// (preservando acessibilidade/AccessKit e inspeção de texto completo), centralizando
/// cada dígito `'0'..='9'` em células fixas alinhadas à grade de pixels de `epaint`.
///
/// Retorna `(job, total_width)` com colunas invariantes entre dígitos (`'1'` vs `'8'`)
/// e `galley.size().x <= total_width` garantido.
pub fn tabular_layout_job(
    fonts: &egui::epaint::Fonts,
    text: &str,
    font_id: egui::FontId,
    color: egui::Color32,
) -> (egui::text::LayoutJob, f32) {
    let mut job = egui::text::LayoutJob {
        wrap: egui::text::TextWrapping::no_max_width(),
        break_on_newline: false,
        ..Default::default()
    };
    let Some(font_id) = sanitize_font_id(font_id) else {
        return (job, 0.0);
    };
    if text.is_empty() {
        return (job, 0.0);
    }

    let ppp = safe_pixels_per_point(fonts);
    let cell_w = tabular_digit_advance(fonts, &font_id);
    let mut cursor_x = 0.0_f32;
    let mut pending_trailing = 0.0_f32;
    let mut logical_x = 0.0_f32;
    let mut non_digit_start: Option<usize> = None;

    let flush_non_digits = |job: &mut egui::text::LayoutJob,
                            cursor_x: &mut f32,
                            pending_trailing: &mut f32,
                            logical_x: &mut f32,
                            start: usize,
                            end: usize| {
        let Some(run) = text.get(start..end) else {
            return;
        };
        if run.is_empty() {
            return;
        }
        let start_x = *cursor_x + *pending_trailing;
        let end_unrounded = if run.chars().nth(1).is_none() {
            let gw = run
                .chars()
                .next()
                .map(|c| safe_glyph_width(fonts, &font_id, c))
                .unwrap_or(0.0);
            start_x + gw
        } else {
            let measure_job = egui::text::LayoutJob {
                text: run.to_owned(),
                sections: vec![egui::text::LayoutSection {
                    leading_space: start_x,
                    byte_range: 0..run.len(),
                    format: egui::text::TextFormat {
                        font_id: font_id.clone(),
                        color,
                        ..Default::default()
                    },
                }],
                wrap: egui::text::TextWrapping::no_max_width(),
                break_on_newline: false,
                ..Default::default()
            };
            let g = fonts.layout_job(measure_job);
            g.rows
                .last()
                .and_then(|r| r.glyphs.last())
                .map(|gl| gl.max_x())
                .filter(|w| w.is_finite() && *w >= start_x)
                .unwrap_or_else(|| {
                    let w = g.size().x;
                    if w.is_finite() {
                        w.max(start_x)
                    } else {
                        start_x
                    }
                })
        };
        let format = egui::text::TextFormat {
            font_id: font_id.clone(),
            color,
            ..Default::default()
        };
        job.append(run, *pending_trailing, format);
        *cursor_x = round_to_pixel(end_unrounded, ppp);
        *pending_trailing = 0.0;
        *logical_x = end_unrounded;
    };

    for (byte_idx, ch) in text.char_indices() {
        if ch.is_ascii_digit() {
            if let Some(start) = non_digit_start.take() {
                flush_non_digits(
                    &mut job,
                    &mut cursor_x,
                    &mut pending_trailing,
                    &mut logical_x,
                    start,
                    byte_idx,
                );
            }
            let slice_end = byte_idx.saturating_add(ch.len_utf8());
            if let Some(slice) = text.get(byte_idx..slice_end) {
                let cell_start = cursor_x + pending_trailing;
                let cell_end = round_to_pixel(cell_start + cell_w, ppp);
                let gw = safe_glyph_width(fonts, &font_id, ch).min(cell_w);
                let pad_left = ((cell_w - gw) * 0.5).max(0.0);
                let format = egui::text::TextFormat {
                    font_id: font_id.clone(),
                    color,
                    ..Default::default()
                };
                job.append(slice, pending_trailing + pad_left, format);
                let unrounded_end = cell_start + pad_left + gw;
                let new_cursor_x = round_to_pixel(unrounded_end, ppp);
                cursor_x = new_cursor_x;
                pending_trailing = (cell_end - new_cursor_x).max(0.0);
                logical_x = cell_end.max(unrounded_end);
            }
        } else if non_digit_start.is_none() {
            non_digit_start = Some(byte_idx);
        }
    }

    if let Some(start) = non_digit_start {
        flush_non_digits(
            &mut job,
            &mut cursor_x,
            &mut pending_trailing,
            &mut logical_x,
            start,
            text.len(),
        );
    }

    (job, ceil_ui(logical_x))
}

/// Gera a `Galley` tabular e sua largura fixa total.
pub fn tabular_galley(
    fonts: &egui::epaint::Fonts,
    text: &str,
    font_id: egui::FontId,
    color: egui::Color32,
) -> (Arc<egui::Galley>, f32) {
    let Some(font_id) = sanitize_font_id(font_id) else {
        return (fonts.layout_job(egui::text::LayoutJob::default()), 0.0);
    };
    let (job, total_w) = tabular_layout_job(fonts, text, font_id, color);
    let galley = fonts.layout_job(job);
    (galley, total_w)
}

/// Desenha texto com dígitos tabulares ancorado em `pos` sem oscilação horizontal.
pub fn paint_tabular_text(
    painter: &egui::Painter,
    pos: egui::Pos2,
    anchor: egui::Align2,
    text: &str,
    font_id: egui::FontId,
    color: egui::Color32,
) -> egui::Rect {
    let Some(font_id) = sanitize_font_id(font_id) else {
        return egui::Rect::from_min_size(pos, egui::Vec2::ZERO);
    };
    let (galley, total_w) = painter.fonts(|f| tabular_galley(f, text, font_id, color));
    let rect = anchor.anchor_size(pos, egui::vec2(total_w, galley.size().y));
    painter.galley(rect.min, galley, color);
    rect
}

/// Adiciona um rótulo com dígitos tabulares e largura fixa reservada (`total_w`), evitando
/// que o container pai ou os limites do widget oscilem quando os dígitos mudam.
pub fn tabular_label(
    ui: &mut egui::Ui,
    text: &str,
    font_id: egui::FontId,
    color: egui::Color32,
) -> egui::Response {
    let sense = if ui.memory(|mem| mem.options.screen_reader) {
        egui::Sense::focusable_noninteractive()
    } else {
        egui::Sense::hover()
    };
    let Some(font_id) = sanitize_font_id(font_id) else {
        return ui.allocate_exact_size(egui::Vec2::ZERO, sense).1;
    };
    let (galley, total_w, row_h) = ui.fonts(|f| {
        let row_h = f.row_height(&font_id);
        let (galley, total_w) = tabular_galley(f, text, font_id, color);
        (galley, total_w, row_h)
    });
    let size = egui::vec2(total_w, galley.size().y.max(row_h));
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), galley.text())
    });
    if ui.is_rect_visible(rect) {
        let galley_pos = egui::pos2(rect.left(), rect.center().y - galley.size().y * 0.5);
        ui.painter().galley(galley_pos, galley, color);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured_ctx_with_ppp(ppp: f32) -> egui::Context {
        let ctx = egui::Context::default();
        configure_fonts(&ctx);
        ctx.set_pixels_per_point(ppp);
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            },
            |_| {},
        );
        ctx
    }

    fn configured_ctx() -> egui::Context {
        configured_ctx_with_ppp(1.0)
    }

    #[test]
    fn configure_fonts_registra_familias_e_preserva_text_styles() {
        let ctx = configured_ctx();
        // Idempotência: chamar duas vezes não altera nem falha.
        configure_fonts(&ctx);

        ctx.fonts(|f| {
            let prop_w = f.glyph_width(&egui::FontId::proportional(12.5), 'A');
            let sb_w = f.glyph_width(&semibold_font(12.5), 'A');
            let visor_w = f.glyph_width(&visor_font(20.0), '0');
            let ruler_w = f.glyph_width(&ruler_font(10.0), '0');
            assert!(prop_w > 0.0 && sb_w > 0.0 && visor_w > 0.0 && ruler_w > 0.0);
        });

        let style = ctx.style();
        assert_eq!(
            style.text_styles.get(&egui::TextStyle::Heading),
            Some(&semibold_font(18.0))
        );
        assert_eq!(
            style.text_styles.get(&egui::TextStyle::Body),
            Some(&egui::FontId::proportional(12.5))
        );
        assert_eq!(
            style.text_styles.get(&egui::TextStyle::Button),
            Some(&egui::FontId::proportional(12.5))
        );
        assert_eq!(
            style.text_styles.get(&egui::TextStyle::Small),
            Some(&egui::FontId::proportional(9.0))
        );
    }

    #[test]
    fn cobertura_de_glifos_ascii_diacriticos_e_simbolos_ui() {
        let ctx = configured_ctx();
        let required = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789:;.,-+/\
            §°·ÁÂÍ×Úàáâãçéêíóõúºô–—•…ℹ←↑→↓↳⋮─▲▶▼◀●★☆⚠✖";

        ctx.fonts(|f| {
            for (family_name, font_id) in [
                ("Proportional", egui::FontId::proportional(12.5)),
                ("semibold", semibold_font(12.5)),
                ("visor", visor_font(20.0)),
                ("ruler", ruler_font(10.0)),
                ("Monospace", egui::FontId::monospace(12.0)),
            ] {
                for ch in required.chars() {
                    let has = f.lock().fonts.font(&font_id).has_glyph(ch);
                    assert!(
                        has,
                        "glifo '{ch}' (U+{:04X}) ausente na família {family_name}",
                        ch as u32
                    );
                }
            }
        });
    }

    #[test]
    fn largura_tabular_invariante_e_entradas_hostis_sem_panico() {
        let ctx = configured_ctx();
        let samples = ["11:11:11;11", "00:00:00;00", "88:88:88;88", "04:17:29;14"];
        ctx.fonts(|f| {
            for font_id in [visor_font(20.0), visor_font(10.5), ruler_font(10.0)] {
                let mut widths = Vec::new();
                for s in samples {
                    let (job, w) = tabular_layout_job(f, s, font_id.clone(), egui::Color32::WHITE);
                    assert_eq!(job.text, s);
                    let galley = f.layout_job(job);
                    assert_eq!(galley.text(), s);
                    widths.push(w);
                }
                let first = widths[0];
                for (s, w) in samples.iter().zip(widths.iter()) {
                    assert!(
                        (*w - first).abs() < 1e-3,
                        "largura tabular divergiu em {font_id:?} para '{s}': {w} vs {first}"
                    );
                }
            }

            // Entradas vazias e hostis (emoji, multilingue, controle, apenas pontuação) não entram em pânico.
            for hostile in ["", "   ", "::;;--..", "Cursor: 11:11:11;11", "⚠⋮★◀▶\n\0"] {
                let (job, w) = tabular_layout_job(f, hostile, visor_font(12.0), egui::Color32::RED);
                assert_eq!(job.text, hostile);
                assert!(w.is_finite() && w >= 0.0);
                let galley = f.layout_job(job);
                assert_eq!(galley.text(), hostile);
                assert!(
                    galley.size().x <= w + 1e-3,
                    "galley.size().x ({}) excedeu total_w ({w}) para '{hostile}'",
                    galley.size().x
                );
            }
        });
    }

    #[test]
    fn alinhamento_tabular_por_coluna_sem_deriva_em_multiplos_ppp() {
        let samples = [
            "00:00:00;00",
            "11:11:11;11",
            "44:44:44;44",
            "55:55:55;55",
            "77:77:77;77",
            "17:41:51;11",
            "88:88:88;88",
        ];

        for ppp in [1.0_f32, 1.25, 1.5, 2.0] {
            let ctx = configured_ctx_with_ppp(ppp);
            assert!((ctx.pixels_per_point() - ppp).abs() < 1e-4);

            for font_id in [visor_font(20.0), visor_font(10.5), ruler_font(10.0)] {
                let mut total_ws = Vec::new();
                let mut sep_xs_per_sample: Vec<Vec<f32>> = Vec::new();
                let mut digit_centers_per_sample: Vec<Vec<f32>> = Vec::new();
                let mut right_anchor_rects = Vec::new();
                let mut right_anchor_seps: Vec<Vec<f32>> = Vec::new();

                ctx.fonts(|f| {
                    for s in samples {
                        let (galley, total_w) =
                            tabular_galley(f, s, font_id.clone(), egui::Color32::WHITE);
                        assert!(
                            galley.size().x <= total_w + 1e-3,
                            "ppp={ppp}, {font_id:?}, '{s}': galley_w={} > total_w={total_w}",
                            galley.size().x
                        );
                        total_ws.push(total_w);

                        let row = &galley.rows[0];
                        let mut seps = Vec::new();
                        let mut centers = Vec::new();
                        for g in &row.glyphs {
                            if g.chr == ':' || g.chr == ';' {
                                seps.push(g.pos.x);
                            } else if g.chr.is_ascii_digit() {
                                centers.push(g.pos.x + 0.5 * g.advance_width);
                            }
                        }
                        assert_eq!(seps.len(), 3);
                        assert_eq!(centers.len(), 8);
                        sep_xs_per_sample.push(seps);
                        digit_centers_per_sample.push(centers);

                        let anchor_pos = egui::pos2(240.0, 40.0);
                        let rect = egui::Align2::RIGHT_BOTTOM
                            .anchor_size(anchor_pos, egui::vec2(total_w, galley.size().y));
                        right_anchor_rects.push(rect);
                        let abs_seps = row
                            .glyphs
                            .iter()
                            .filter(|g| g.chr == ':' || g.chr == ';')
                            .map(|g| rect.min.x + g.pos.x)
                            .collect::<Vec<_>>();
                        right_anchor_seps.push(abs_seps);
                    }
                });

                let min_w = total_ws.iter().copied().fold(f32::INFINITY, f32::min);
                let max_w = total_ws.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                assert!(
                    (max_w - min_w) < 1e-3,
                    "ppp={ppp}, {font_id:?}: total_w spread={}",
                    max_w - min_w
                );

                for col in 0..3 {
                    let min_s = sep_xs_per_sample
                        .iter()
                        .map(|v| v[col])
                        .fold(f32::INFINITY, f32::min);
                    let max_s = sep_xs_per_sample
                        .iter()
                        .map(|v| v[col])
                        .fold(f32::NEG_INFINITY, f32::max);
                    assert!(
                        (max_s - min_s) < 0.05,
                        "ppp={ppp}, {font_id:?}, sep[{col}] spread={}",
                        max_s - min_s
                    );

                    let min_as = right_anchor_seps
                        .iter()
                        .map(|v| v[col])
                        .fold(f32::INFINITY, f32::min);
                    let max_as = right_anchor_seps
                        .iter()
                        .map(|v| v[col])
                        .fold(f32::NEG_INFINITY, f32::max);
                    assert!(
                        (max_as - min_as) < 0.05,
                        "ppp={ppp}, {font_id:?}, RIGHT_BOTTOM sep[{col}] spread={}",
                        max_as - min_as
                    );
                }

                for col in 0..8 {
                    let min_c = digit_centers_per_sample
                        .iter()
                        .map(|v| v[col])
                        .fold(f32::INFINITY, f32::min);
                    let max_c = digit_centers_per_sample
                        .iter()
                        .map(|v| v[col])
                        .fold(f32::NEG_INFINITY, f32::max);
                    assert!(
                        (max_c - min_c) < 0.5,
                        "ppp={ppp}, {font_id:?}, digit_center[{col}] spread={}",
                        max_c - min_c
                    );
                }

                let min_l = right_anchor_rects
                    .iter()
                    .map(|r| r.left())
                    .fold(f32::INFINITY, f32::min);
                let max_l = right_anchor_rects
                    .iter()
                    .map(|r| r.left())
                    .fold(f32::NEG_INFINITY, f32::max);
                assert!(
                    (max_l - min_l) < 0.05,
                    "ppp={ppp}, {font_id:?}: RIGHT_BOTTOM left spread={}",
                    max_l - min_l
                );
            }
        }
    }

    #[test]
    fn chips_transporte_tabular_label_sem_oscilacao_em_multiplos_ppp() {
        let samples = [
            "00:00:00;00",
            "11:11:11;11",
            "44:44:44;44",
            "55:55:55;55",
            "77:77:77;77",
            "17:41:51;11",
            "88:88:88;88",
        ];

        for ppp in [1.0_f32, 1.25, 1.5, 2.0] {
            let ctx = configured_ctx_with_ppp(ppp);
            let mut pos_frame_rects = Vec::new();
            let mut tam_frame_rects = Vec::new();
            let mut pos_lbl_rects = Vec::new();
            let mut tam_lbl_rects = Vec::new();

            for tc in samples {
                let mut pos_frame = egui::Rect::NOTHING;
                let mut tam_frame = egui::Rect::NOTHING;
                let mut pos_lbl = egui::Rect::NOTHING;
                let mut tam_lbl = egui::Rect::NOTHING;

                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(800.0, 200.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::TopBottomPanel::top("transport_probe").show(ctx, |ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let chip = |ui: &mut egui::Ui, label: &str, val: &str| {
                                        let mut lbl_rect = egui::Rect::NOTHING;
                                        let frame_resp = egui::Frame::new()
                                            .inner_margin(egui::Margin::symmetric(5, 1))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 3.0;
                                                    let name = egui::RichText::new(label).small();
                                                    if ui.layout().prefer_right_to_left() {
                                                        lbl_rect = tabular_label(
                                                            ui,
                                                            val,
                                                            visor_font(10.5),
                                                            egui::Color32::WHITE,
                                                        )
                                                        .rect;
                                                        ui.label(name);
                                                    } else {
                                                        ui.label(name);
                                                        lbl_rect = tabular_label(
                                                            ui,
                                                            val,
                                                            visor_font(10.5),
                                                            egui::Color32::WHITE,
                                                        )
                                                        .rect;
                                                    }
                                                });
                                            })
                                            .response;
                                        (frame_resp.rect, lbl_rect)
                                    };
                                    (tam_frame, tam_lbl) = chip(ui, "Tam:", tc);
                                    (pos_frame, pos_lbl) = chip(ui, "Pos:", tc);
                                },
                            );
                        });
                    },
                );
                pos_frame_rects.push(pos_frame);
                tam_frame_rects.push(tam_frame);
                pos_lbl_rects.push(pos_lbl);
                tam_lbl_rects.push(tam_lbl);
            }

            for (name, rects) in [
                ("Pos frame", &pos_frame_rects),
                ("Tam frame", &tam_frame_rects),
                ("Pos tabular_label", &pos_lbl_rects),
                ("Tam tabular_label", &tam_lbl_rects),
            ] {
                let min_x0 = rects.iter().map(|r| r.left()).fold(f32::INFINITY, f32::min);
                let max_x0 = rects
                    .iter()
                    .map(|r| r.left())
                    .fold(f32::NEG_INFINITY, f32::max);
                let min_w = rects
                    .iter()
                    .map(|r| r.width())
                    .fold(f32::INFINITY, f32::min);
                let max_w = rects
                    .iter()
                    .map(|r| r.width())
                    .fold(f32::NEG_INFINITY, f32::max);
                assert!(
                    (max_x0 - min_x0) < 0.05 && (max_w - min_w) < 0.05,
                    "ppp={ppp}, {name}: x0 spread={}, width spread={}",
                    max_x0 - min_x0,
                    max_w - min_w
                );
            }
        }
    }

    #[test]
    fn sanitizacao_tamanho_fonte_hostil_sem_panico() {
        let hostile_sizes = [
            0.0_f32,
            -1.0,
            -100.0,
            0.001,
            0.5,
            500.0,
            2000.0,
            10_000.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ];

        for ppp in [1.0_f32, 1.25] {
            let ctx = configured_ctx_with_ppp(ppp);
            for size in hostile_sizes {
                let font_id = visor_font(size);
                let is_invalid = !size.is_finite() || size <= 0.0;

                ctx.fonts(|f| {
                    let adv = tabular_digit_advance(f, &font_id);
                    assert!(adv.is_finite() && adv >= 0.0);
                    if is_invalid {
                        assert_eq!(adv, 0.0);
                    }

                    let (job, total_w) =
                        tabular_layout_job(f, "00:00:00;00", font_id.clone(), egui::Color32::WHITE);
                    assert!(total_w.is_finite() && total_w >= 0.0);
                    if is_invalid {
                        assert_eq!(total_w, 0.0);
                        assert!(job.sections.is_empty());
                    }

                    let (galley, galley_w) =
                        tabular_galley(f, "17:41:51;11", font_id.clone(), egui::Color32::WHITE);
                    assert!(galley_w.is_finite() && galley_w >= 0.0);
                    assert!(galley.size().x <= galley_w + 1e-3);
                    if is_invalid {
                        assert_eq!(galley_w, 0.0);
                        assert_eq!(galley.size(), egui::Vec2::ZERO);
                    }
                });

                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(800.0, 600.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let rect = paint_tabular_text(
                                ui.painter(),
                                egui::pos2(100.0, 100.0),
                                egui::Align2::LEFT_TOP,
                                "88:88:88;88",
                                font_id.clone(),
                                egui::Color32::WHITE,
                            );
                            assert!(rect.width().is_finite() && rect.height().is_finite());
                            if is_invalid {
                                assert_eq!(rect.size(), egui::Vec2::ZERO);
                            }

                            let resp = tabular_label(
                                ui,
                                "77:77:77;77",
                                font_id.clone(),
                                egui::Color32::WHITE,
                            );
                            assert!(
                                resp.rect.width().is_finite() && resp.rect.height().is_finite()
                            );
                            if is_invalid {
                                assert_eq!(resp.rect.size(), egui::Vec2::ZERO);
                            }
                        });
                    },
                );
            }
        }
    }

    #[test]
    fn flush_non_digits_alinhado_com_break_on_newline_false() {
        let non_digit_tail_cases = [
            "\n00:00:00;00\n11:11:11;11\r\n",
            "\n\r\n12:34\n",
            "In:\n00:00\r\nOut:\n11:11\r\n",
        ];

        for ppp in [1.0_f32, 1.25, 1.5, 2.0] {
            let ctx = configured_ctx_with_ppp(ppp);
            ctx.fonts(|f| {
                for font_id in [visor_font(20.0), visor_font(10.5)] {
                    for s in non_digit_tail_cases {
                        let (galley, total_w) =
                            tabular_galley(f, s, font_id.clone(), egui::Color32::WHITE);
                        assert_eq!(galley.text(), s);
                        assert_eq!(galley.rows.len(), 1);
                        assert!(
                            (total_w - galley.size().x).abs() < 0.5,
                            "ppp={ppp}, {font_id:?}, s={s:?}: total_w={total_w} vs galley_w={}",
                            galley.size().x
                        );
                        assert!(
                            galley.size().x <= total_w + 1e-3,
                            "ppp={ppp}, {font_id:?}, s={s:?}: galley_w={} > total_w={total_w}",
                            galley.size().x
                        );
                    }

                    let (g_ones, w_ones) = tabular_galley(
                        f,
                        "In:\n00:00\r\nOut:\n11:11",
                        font_id.clone(),
                        egui::Color32::WHITE,
                    );
                    let (g_eights, w_eights) = tabular_galley(
                        f,
                        "In:\n00:00\r\nOut:\n88:88",
                        font_id.clone(),
                        egui::Color32::WHITE,
                    );
                    assert_eq!(g_ones.rows.len(), 1);
                    assert_eq!(g_eights.rows.len(), 1);
                    assert!((w_ones - w_eights).abs() < 1e-3);
                    assert!(g_ones.size().x <= w_ones + 1e-3);
                    assert!(g_eights.size().x <= w_eights + 1e-3);
                }
            });
        }
    }
}
