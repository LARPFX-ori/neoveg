//! Configuração tipográfica do Neoveg com fontes abertas (`OFL-1.1`) embutidas
//! em `assets/fonts/` (Selawik, Inter, Noto Sans), carregamento seguro opcional das
//! fontes do sistema operacional em tempo de execução (Segoe UI, Tahoma, Consolas,
//! Cascadia Mono, fontes CJK japonesas) e utilitários para renderização tabular sem
//! oscilação horizontal em timecodes (`Visor de tempo`, `Régua`, barra de transporte e `Trimmer`).

use ab_glyph::{Font as _, FontVec};
use eframe::egui;
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

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

const MAX_SYSTEM_FONT_BYTES: u64 = 64 * 1024 * 1024;
const CJK_FONT_KEY: &str = "neoveg_sys_jp";
const UI_FONT_KEY: &str = "neoveg_sys_ui";
const MONO_FONT_KEY: &str = "neoveg_sys_mono";
const TIME_FONT_KEY: &str = "neoveg_sys_time";
/// Família do Visor de tempo: números grandes em negrito, como no VEGAS Pro.
/// Sempre existe na cadeia (cai na monoespaçada sem a fonte do sistema).
pub const TIME_FAMILY: &str = "neoveg_time";

#[derive(Clone)]
struct LoadedSystemFont {
    data: Vec<u8>,
    index: u32,
}

static CJK_FONT_CACHE: OnceLock<Option<LoadedSystemFont>> = OnceLock::new();
static UI_FONT_CACHE: OnceLock<Option<LoadedSystemFont>> = OnceLock::new();
static MONO_FONT_CACHE: OnceLock<Option<LoadedSystemFont>> = OnceLock::new();
static TIME_FONT_CACHE: OnceLock<Option<LoadedSystemFont>> = OnceLock::new();

/// Pasta de fontes do Windows (`%WINDIR%\Fonts`, ou `C:\Windows\Fonts`).
fn windows_font_dir() -> PathBuf {
    std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("Fonts")
}

/// Fonte da interface, em ordem de preferência. No Windows é a fonte de
/// sistema que o VEGAS Pro usa (Segoe UI), com Tahoma como alternativa.
fn candidate_ui_font_paths() -> Vec<PathBuf> {
    let win = windows_font_dir();
    let mut out = vec![win.join("segoeui.ttf"), win.join("tahoma.ttf")];
    for p in [
        // macOS
        "/System/Library/Fonts/SFNS.ttf",
        "/System/Library/Fonts/Helvetica.ttc",
        // Linux
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
        "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/liberation-sans/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
    ] {
        out.push(PathBuf::from(p));
    }
    out
}

/// Fonte monoespaçada (códigos de tempo, campos numéricos).
fn candidate_mono_font_paths() -> Vec<PathBuf> {
    let win = windows_font_dir();
    let mut out = vec![win.join("consola.ttf"), win.join("CascadiaMono.ttf")];
    for p in [
        "/System/Library/Fonts/Menlo.ttc",
        "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
        "/usr/share/fonts/truetype/liberation2/LiberationMono-Regular.ttf",
        "/usr/share/fonts/liberation/LiberationMono-Regular.ttf",
        "/usr/share/fonts/liberation-mono/LiberationMono-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf",
    ] {
        out.push(PathBuf::from(p));
    }
    out
}

/// Fonte do Visor de tempo: a sem serifa do sistema em negrito (Segoe UI Bold
/// no Windows), com números de largura fixa.
fn candidate_time_font_paths() -> Vec<PathBuf> {
    let win = windows_font_dir();
    let mut out = vec![win.join("segoeuib.ttf"), win.join("tahomabd.ttf")];
    for p in [
        "/System/Library/Fonts/Helvetica.ttc",
        "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
        "/usr/share/fonts/noto/NotoSans-Bold.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
        "/usr/share/fonts/truetype/liberation2/LiberationSans-Bold.ttf",
        "/usr/share/fonts/liberation/LiberationSans-Bold.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
    ] {
        out.push(PathBuf::from(p));
    }
    out
}

/// Para o Visor de tempo: precisa dos números e dos separadores dos formatos
/// de tempo, com todos os dígitos da mesma largura (o visor não "dança").
fn validate_time_font(bytes: &[u8]) -> Option<u32> {
    use ab_glyph::ScaleFont as _;
    for idx in 0..4u32 {
        let Ok(font) = FontVec::try_from_vec_and_index(bytes.to_vec(), idx) else {
            continue;
        };
        let valid_units = font
            .units_per_em()
            .is_some_and(|u| (16.0..=16384.0).contains(&u));
        if !valid_units || !"0123456789:;.,-".chars().all(|c| font.glyph_id(c).0 != 0) {
            continue;
        }
        let scaled = font.as_scaled(100.0);
        let w0 = scaled.h_advance(font.glyph_id('0'));
        let tabular = w0.is_finite()
            && w0 > 0.0
            && "123456789"
                .chars()
                .all(|c| (scaled.h_advance(font.glyph_id(c)) - w0).abs() < 0.5);
        if tabular {
            return Some(idx);
        }
    }
    None
}

fn cached_time_font() -> Option<&'static LoadedSystemFont> {
    TIME_FONT_CACHE
        .get_or_init(|| first_valid_font(candidate_time_font_paths(), validate_time_font))
        .as_ref()
}

/// Fonte do Visor de tempo no tamanho pedido. Se a família não estiver
/// registrada no contexto (fontes trocadas por fora), usa a monoespaçada em
/// vez de travar o egui.
pub fn time_font(ctx: &egui::Context, size: f32) -> egui::FontId {
    let fam = egui::FontFamily::Name(TIME_FAMILY.into());
    if ctx.fonts(|f| f.families().contains(&fam)) {
        egui::FontId::new(size, fam)
    } else {
        egui::FontId::monospace(size)
    }
}

/// Aceita a fonte só se ela abrir e cobrir os idiomas latinos do programa
/// (pt-BR, en, es, fr, de); devolve o índice dentro de um `.ttc`.
fn validate_latin_font(bytes: &[u8]) -> Option<u32> {
    for idx in 0..4u32 {
        let Ok(font) = FontVec::try_from_vec_and_index(bytes.to_vec(), idx) else {
            continue;
        };
        let valid_units = font
            .units_per_em()
            .is_some_and(|u| (16.0..=16384.0).contains(&u));
        if !valid_units {
            continue;
        }
        let covers = "Aaçãõéêíóúñ¿¡œàèëîïùûüäößÄÖÜ0123456789%:;.,()[]+-"
            .chars()
            .all(|c| font.glyph_id(c).0 != 0);
        if covers {
            return Some(idx);
        }
    }
    None
}

fn first_valid_font(
    paths: Vec<PathBuf>,
    validate: fn(&[u8]) -> Option<u32>,
) -> Option<LoadedSystemFont> {
    paths.iter().find_map(|p| read_bounded_font(p, validate))
}

fn cached_ui_font() -> Option<&'static LoadedSystemFont> {
    UI_FONT_CACHE
        .get_or_init(|| first_valid_font(candidate_ui_font_paths(), validate_latin_font))
        .as_ref()
}

fn cached_mono_font() -> Option<&'static LoadedSystemFont> {
    MONO_FONT_CACHE
        .get_or_init(|| first_valid_font(candidate_mono_font_paths(), validate_latin_font))
        .as_ref()
}

fn candidate_japanese_font_paths() -> Vec<PathBuf> {
    let mut out = vec![
        // Windows
        PathBuf::from(r"C:\Windows\Fonts\YuGothM.ttc"),
        PathBuf::from(r"C:\Windows\Fonts\YuGothR.ttc"),
        PathBuf::from(r"C:\Windows\Fonts\meiryo.ttc"),
        PathBuf::from(r"C:\Windows\Fonts\msgothic.ttc"),
        PathBuf::from(r"C:\Windows\Fonts\msmincho.ttc"),
        // macOS
        PathBuf::from("/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc"),
        PathBuf::from("/System/Library/Fonts/Hiragino Sans GB.ttc"),
        PathBuf::from("/System/Library/Fonts/Supplemental/Osaka.ttf"),
        PathBuf::from("/Library/Fonts/Osaka.ttf"),
        // Linux
        PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
        PathBuf::from("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc"),
        PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansJP-Regular.otf"),
        PathBuf::from("/usr/share/fonts/truetype/noto/NotoSansJP-Regular.ttf"),
        PathBuf::from("/usr/share/fonts/OTF/NotoSansCJKjp-Regular.otf"),
        PathBuf::from("/usr/share/fonts/TTF/NotoSansJP-Regular.ttf"),
        PathBuf::from("/usr/share/fonts/truetype/fonts-japanese-gothic.ttf"),
        PathBuf::from("/usr/share/fonts/truetype/vlgothic/VL-Gothic-Regular.ttf"),
        PathBuf::from("/usr/share/fonts/truetype/takao-gothic/TakaoPGothic.ttf"),
    ];

    if let Ok(windir) = std::env::var("WINDIR") {
        let dir = PathBuf::from(windir).join("Fonts");
        for name in [
            "YuGothM.ttc",
            "YuGothR.ttc",
            "meiryo.ttc",
            "msgothic.ttc",
            "msmincho.ttc",
        ] {
            let p = dir.join(name);
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

fn validate_japanese_font(bytes: &[u8]) -> Option<u32> {
    for idx in 0..4u32 {
        let Ok(font) = FontVec::try_from_vec_and_index(bytes.to_vec(), idx) else {
            continue;
        };
        let valid_units = font
            .units_per_em()
            .is_some_and(|u| (16.0..=16384.0).contains(&u));
        if !valid_units {
            continue;
        }
        // Exige glifos reais de Hiragana, Katakana e Kanji comum de interface.
        let has_jp = font.glyph_id('あ').0 != 0
            && font.glyph_id('ア').0 != 0
            && font.glyph_id('編').0 != 0
            && font.glyph_id('保').0 != 0;
        if has_jp {
            return Some(idx);
        }
    }
    None
}

fn read_bounded_font(path: &Path, validate: fn(&[u8]) -> Option<u32>) -> Option<LoadedSystemFont> {
    let file = std::fs::File::open(path).ok()?;
    let meta_len = file.metadata().ok().map_or(0, |m| m.len());
    if meta_len == 0 || meta_len > MAX_SYSTEM_FONT_BYTES {
        return None;
    }
    let mut buf = Vec::with_capacity(meta_len as usize);
    file.take(MAX_SYSTEM_FONT_BYTES + 1)
        .read_to_end(&mut buf)
        .ok()?;
    if buf.len() as u64 > MAX_SYSTEM_FONT_BYTES {
        return None;
    }
    let index = validate(&buf)?;
    Some(LoadedSystemFont { data: buf, index })
}

fn cached_japanese_fallback() -> Option<&'static LoadedSystemFont> {
    CJK_FONT_CACHE
        .get_or_init(|| first_valid_font(candidate_japanese_font_paths(), validate_japanese_font))
        .as_ref()
}

fn font_data(f: &LoadedSystemFont) -> Arc<egui::FontData> {
    let mut fd = egui::FontData::from_owned(f.data.clone());
    fd.index = f.index;
    Arc::new(fd)
}

/// Monta as definições de fontes da aplicação.
///
/// Sempre registra as fontes abertas OFL-1.1 do Neoveg (`assets/fonts/`: Selawik, Inter, Noto Sans).
/// Quando `system_ui` é verdadeiro, as fontes do sistema operacional (Segoe UI, Tahoma, Consolas,
/// Cascadia Mono) são inseridas no início da cadeia para paridade clássica de SO.
pub fn font_definitions(system_ui: bool) -> egui::FontDefinitions {
    let mut defs = egui::FontDefinitions::default();
    let default_prop = defs
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let default_mono = defs
        .families
        .get(&egui::FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();

    // 1. Fontes abertas OFL-1.1 do Neoveg embutidas
    defs.font_data.insert(
        KEY_SELAWIK_REGULAR.to_owned(),
        Arc::new(egui::FontData::from_static(SELAWIK_REGULAR_BYTES)),
    );
    defs.font_data.insert(
        KEY_SELAWIK_SEMIBOLD.to_owned(),
        Arc::new(egui::FontData::from_static(SELAWIK_SEMIBOLD_BYTES)),
    );
    defs.font_data.insert(
        KEY_INTER_REGULAR.to_owned(),
        Arc::new(egui::FontData::from_static(INTER_REGULAR_BYTES)),
    );
    defs.font_data.insert(
        KEY_NOTO_SANS_REGULAR.to_owned(),
        Arc::new(egui::FontData::from_static(NOTO_SANS_REGULAR_BYTES)),
    );
    defs.font_data.insert(
        KEY_NOTO_SANS_MATH.to_owned(),
        Arc::new(egui::FontData::from_static(NOTO_SANS_MATH_REGULAR_BYTES)),
    );

    // 2. Fallback japonês do sistema
    let mut cjk_tail: &[String] = &[];
    let cjk_owned: Vec<String>;
    if let Some(sys_font) = cached_japanese_fallback() {
        defs.font_data
            .insert(CJK_FONT_KEY.to_owned(), font_data(sys_font));
        cjk_owned = vec![CJK_FONT_KEY.to_owned()];
        cjk_tail = &cjk_owned;
    }

    // 3. Cadeia Proporcional base
    let mut prop_tail = default_prop;
    prop_tail.extend_from_slice(cjk_tail);
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
        &prop_tail,
    );

    // 4. Cadeia Semibold
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
        &prop_tail,
    );
    defs.families.insert(
        egui::FontFamily::Name(FAMILY_SEMIBOLD.into()),
        semibold_chain,
    );

    // 5. Cadeia Visor e Régua
    let mut visor_chain = Vec::new();
    push_unique_chain(
        &mut visor_chain,
        &[
            KEY_INTER_REGULAR,
            KEY_NOTO_SANS_REGULAR,
            KEY_NOTO_SANS_MATH,
            KEY_HACK_BUILTIN,
        ],
        &prop_tail,
    );
    defs.families
        .insert(egui::FontFamily::Name(FAMILY_VISOR.into()), visor_chain.clone());

    let mut ruler_chain = Vec::new();
    push_unique_chain(
        &mut ruler_chain,
        &[
            KEY_INTER_REGULAR,
            KEY_NOTO_SANS_REGULAR,
            KEY_NOTO_SANS_MATH,
            KEY_HACK_BUILTIN,
        ],
        &prop_tail,
    );
    defs.families
        .insert(egui::FontFamily::Name(FAMILY_RULER.into()), ruler_chain);

    // 6. Cadeia Monospace base
    let mut mono_chain = Vec::new();
    let mut mono_tail = Vec::with_capacity(
        default_mono
            .len()
            .saturating_add(4)
            .saturating_add(prop_tail.len()),
    );
    mono_tail.extend(default_mono.clone());
    for extra in [KEY_NOTO_SANS_REGULAR, KEY_INTER_REGULAR, KEY_NOTO_SANS_MATH] {
        mono_tail.push(extra.to_owned());
    }
    mono_tail.extend(prop_tail);
    push_unique_chain(&mut mono_chain, &[], &mono_tail);

    // 7. TIME_FAMILY base (Visor de tempo)
    let mut time_chain = default_mono;
    time_chain.insert(0, KEY_INTER_REGULAR.to_owned());

    // 8. Se system_ui estiver ativo, insere fontes do SO na frente das cadeias
    if system_ui {
        if let Some(font) = cached_ui_font() {
            defs.font_data.insert(UI_FONT_KEY.to_owned(), font_data(font));
            prop_chain.insert(0, UI_FONT_KEY.to_owned());
        }
        if let Some(font) = cached_mono_font() {
            defs.font_data.insert(MONO_FONT_KEY.to_owned(), font_data(font));
            mono_chain.insert(0, MONO_FONT_KEY.to_owned());
        }
        if let Some(font) = cached_time_font() {
            defs.font_data.insert(TIME_FONT_KEY.to_owned(), font_data(font));
            time_chain.insert(0, TIME_FONT_KEY.to_owned());
        }
    }

    defs.families.insert(egui::FontFamily::Proportional, prop_chain);
    defs.families.insert(egui::FontFamily::Monospace, mono_chain);
    defs.families
        .insert(egui::FontFamily::Name(TIME_FAMILY.into()), time_chain);

    defs
}

/// Constrói a tabela padrão de fontes com as fontes abertas OFL-1.1 embutidas.
pub fn build_font_definitions() -> egui::FontDefinitions {
    font_definitions(false)
}

/// Configura as fontes do contexto egui e aplica os estilos tipográficos base.
pub fn configure_fonts(ctx: &egui::Context, system_ui: bool) {
    ctx.set_fonts(font_definitions(system_ui));
    ctx.style_mut(|style| {
        style
            .text_styles
            .insert(egui::TextStyle::Heading, semibold_font(18.0));
    });
}
#[cfg(test)]
mod tests {
    use super::*;

    fn configured_ctx_with_ppp(ppp: f32) -> egui::Context {
        let ctx = egui::Context::default();
        configure_fonts(&ctx, false);
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
        configure_fonts(&ctx, false);

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

    use neoveg_common::{tr, Language};
    use neoveg_engine::commands::COMMANDS;

    #[test]
    fn corrupted_font_bytes_never_panic() {
        assert_eq!(validate_japanese_font(&[]), None);
        assert_eq!(validate_japanese_font(b"not a valid font"), None);
        assert_eq!(validate_japanese_font(&vec![0xFF; 4096]), None);
        assert_eq!(validate_time_font(&[]), None);
        assert_eq!(validate_time_font(&vec![0xFF; 4096]), None);
        assert_eq!(validate_latin_font(&[]), None);
        assert_eq!(validate_latin_font(b"not a valid font"), None);
        assert_eq!(validate_latin_font(&vec![0xFF; 4096]), None);
        // arquivo inexistente ou pasta: sem fonte, sem pânico
        assert!(read_bounded_font(Path::new("nao/existe.ttf"), validate_latin_font).is_none());
        assert!(read_bounded_font(Path::new("."), validate_latin_font).is_none());
    }

    #[test]
    fn familia_do_visor_de_tempo_sempre_existe_e_cai_na_monoespacada() {
        let fam = egui::FontFamily::Name(TIME_FAMILY.into());
        for system_ui in [false, true] {
            let defs = font_definitions(system_ui);
            let chain = defs.families.get(&fam).expect("família do visor");
            let mono = defs
                .families
                .get(&egui::FontFamily::Monospace)
                .expect("monoespaçada");
            // a reserva é a cadeia monoespaçada inteira
            assert!(chain.ends_with(mono), "{chain:?} / {mono:?}");
            if !system_ui {
                assert_eq!(chain, mono);
            }
        }
        // contexto sem as fontes do programa: cai na monoespaçada
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        assert_eq!(time_font(&ctx, 20.0), egui::FontId::monospace(20.0));
        ctx.set_fonts(font_definitions(false));
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        assert_eq!(time_font(&ctx, 20.0).family, fam);
    }

    #[test]
    fn fontes_do_sistema_vao_para_o_inicio_e_as_embutidas_ficam_de_reserva() {
        let embutida = font_definitions(false);
        assert!(!embutida.font_data.contains_key(UI_FONT_KEY));
        assert!(!embutida.font_data.contains_key(MONO_FONT_KEY));

        let sistema = font_definitions(true);
        for (key, fam) in [
            (UI_FONT_KEY, egui::FontFamily::Proportional),
            (MONO_FONT_KEY, egui::FontFamily::Monospace),
        ] {
            let chain = sistema.families.get(&fam).expect("família");
            let base = embutida.families.get(&fam).expect("família");
            if sistema.font_data.contains_key(key) {
                assert_eq!(chain.first().map(String::as_str), Some(key));
                assert_eq!(chain.len(), base.len() + 1);
            } else {
                // máquina sem a fonte: a cadeia é a embutida
                assert_eq!(chain, base);
            }
            for name in base {
                assert!(chain.contains(name), "a reserva {name} saiu da cadeia");
            }
        }
    }

    #[test]
    fn fonte_da_interface_do_sistema_cobre_os_textos_latinos() {
        let Some(sys) = cached_ui_font() else {
            // máquina sem Segoe UI, Tahoma, Noto, Liberation ou DejaVu
            return;
        };
        let fv = FontVec::try_from_vec_and_index(sys.data.clone(), sys.index)
            .expect("fonte validada deve abrir no ab_glyph");
        let defs = egui::FontDefinitions::default();
        let mut chain = vec![fv];
        if let Some(names) = defs.families.get(&egui::FontFamily::Proportional) {
            for name in names {
                if let Some(fd) = defs.font_data.get(name) {
                    if let Ok(f) = FontVec::try_from_vec_and_index(fd.font.to_vec(), fd.index) {
                        chain.push(f);
                    }
                }
            }
        }
        for lang in [
            Language::PtBr,
            Language::En,
            Language::Es,
            Language::Fr,
            Language::De,
        ] {
            for cmd in COMMANDS {
                for ch in cmd.label_in(lang).chars() {
                    if ch.is_control() || ch.is_whitespace() {
                        continue;
                    }
                    assert!(
                        chain.iter().any(|f| f.glyph_id(ch).0 != 0),
                        "glifo ausente para {:?} (U+{:04X}) em {:?}",
                        ch,
                        ch as u32,
                        cmd.label_in(lang)
                    );
                }
            }
        }
    }

    #[test]
    fn default_fonts_render_all_latin_languages_cleanly() {
        let defs = build_font_definitions();
        let mut parsed = Vec::new();
        if let Some(names) = defs.families.get(&egui::FontFamily::Proportional) {
            for name in names {
                if let Some(fd) = defs.font_data.get(name) {
                    if let Ok(fv) = FontVec::try_from_vec_and_index(fd.font.to_vec(), fd.index) {
                        parsed.push(fv);
                    }
                }
            }
        }
        assert!(!parsed.is_empty());

        for lang in [
            Language::PtBr,
            Language::En,
            Language::Es,
            Language::Fr,
            Language::De,
        ] {
            for cmd in COMMANDS {
                for ch in cmd.label_in(lang).chars() {
                    if ch.is_control() || ch.is_whitespace() {
                        continue;
                    }
                    let ok = parsed.iter().any(|f| f.glyph_id(ch).0 != 0);
                    assert!(
                        ok,
                        "glifo ausente para {:?} (U+{:04X}) no idioma {} em {:?}",
                        ch,
                        ch as u32,
                        lang.code(),
                        cmd.label_in(lang)
                    );
                }
            }
        }
        // Testa caracteres acentuados e especiais típicos dos idiomas latinos.
        for sample in [
            "Português (Brasil) ãõçáéíóú",
            "Español ñ¿¡áéíóúü",
            "Français œŒàâçéèêëîïôùûü",
            "Deutsch äöüßÄÖÜ",
            tr(
                Language::Es,
                "O projeto tem alterações não salvas. Deseja salvar antes de sair?",
            ),
            tr(Language::Fr, "À propos de Neoveg"),
            tr(Language::De, "Größe (% Höhe)"),
        ] {
            for ch in sample.chars().filter(|c| !c.is_whitespace()) {
                let ok = parsed.iter().any(|f| f.glyph_id(ch).0 != 0);
                assert!(ok, "glifo latino ausente para {:?} em {:?}", ch, sample);
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

    #[test]
    fn japanese_fallback_covers_japanese_commands_when_os_font_present() {
        let Some(sys) = cached_japanese_fallback() else {
            // Ambiente headless mínimo sem fonte CJK instalada.
            return;
        };
        let fv = FontVec::try_from_vec_and_index(sys.data.clone(), sys.index)
            .expect("fonte validada deve abrir no ab_glyph");
        let defs = build_font_definitions();
        let mut chain = Vec::new();
        if let Some(names) = defs.families.get(&egui::FontFamily::Proportional) {
            for name in names {
                if let Some(fd) = defs.font_data.get(name) {
                    if let Ok(f) = FontVec::try_from_vec_and_index(fd.font.to_vec(), fd.index) {
                        chain.push(f);
                    }
                }
            }
        }
        chain.push(fv);

        for cmd in COMMANDS {
            for ch in cmd.label_in(Language::Ja).chars() {
                if ch.is_control() || ch.is_whitespace() {
                    continue;
                }
                let ok = chain.iter().any(|f| f.glyph_id(ch).0 != 0);
                assert!(
                    ok,
                    "glifo japonês ausente para {:?} (U+{:04X}) no comando {:?}",
                    ch,
                    ch as u32,
                    cmd.label_in(Language::Ja)
                );
            }
        }
    }
}
