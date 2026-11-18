//! Carregamento seguro das fontes do sistema operacional em tempo de execução
//! (AGENTS.md §1.5): a fonte da interface (Segoe UI no Windows, a mesma fonte
//! de sistema que o VEGAS Pro usa; Tahoma como alternativa; Noto Sans,
//! Liberation Sans ou DejaVu Sans no Linux), a monoespaçada dos códigos de
//! tempo (Consolas ou Cascadia Mono) e a de reserva para escrita japonesa.
//!
//! Nenhuma fonte proprietária é incorporada ou distribuída no repositório: os
//! arquivos são lidos da pasta de fontes do sistema e validados antes do uso.
//! As fontes embutidas do egui continuam na cadeia como reserva (símbolos).

use ab_glyph::{Font as _, FontVec};
use eframe::egui;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

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

/// Monta as fontes da interface. Com `system_ui`, a fonte do sistema (Segoe UI
/// no Windows, como o VEGAS Pro) e a monoespaçada do sistema vão para o início
/// da cadeia; as embutidas do egui ficam como reserva. A fonte japonesa do
/// sistema entra no fim. Sem as fontes no disco, fica a embutida (§0).
pub fn configure_fonts(ctx: &egui::Context, system_ui: bool) {
    ctx.set_fonts(font_definitions(system_ui));
}

fn font_definitions(system_ui: bool) -> egui::FontDefinitions {
    let mut defs = egui::FontDefinitions::default();
    if system_ui {
        for (key, font, fam) in [
            (
                UI_FONT_KEY,
                cached_ui_font(),
                egui::FontFamily::Proportional,
            ),
            (
                MONO_FONT_KEY,
                cached_mono_font(),
                egui::FontFamily::Monospace,
            ),
        ] {
            let Some(font) = font else {
                continue;
            };
            defs.font_data.insert(key.to_owned(), font_data(font));
            if let Some(chain) = defs.families.get_mut(&fam) {
                chain.insert(0, key.to_owned());
            }
        }
    }
    if let Some(sys_font) = cached_japanese_fallback() {
        defs.font_data
            .insert(CJK_FONT_KEY.to_owned(), font_data(sys_font));
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            if let Some(chain) = defs.families.get_mut(&fam) {
                chain.push(CJK_FONT_KEY.to_owned());
            }
        }
    }
    // Visor de tempo: a negrito do sistema na frente da cadeia monoespaçada
    let mut time_chain = defs
        .families
        .get(&egui::FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();
    if system_ui {
        if let Some(font) = cached_time_font() {
            defs.font_data
                .insert(TIME_FONT_KEY.to_owned(), font_data(font));
            time_chain.insert(0, TIME_FONT_KEY.to_owned());
        }
    }
    defs.families
        .insert(egui::FontFamily::Name(TIME_FAMILY.into()), time_chain);
    defs
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let defs = egui::FontDefinitions::default();
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
    fn japanese_fallback_covers_japanese_commands_when_os_font_present() {
        let Some(sys) = cached_japanese_fallback() else {
            // Ambiente headless mínimo sem fonte CJK instalada.
            return;
        };
        let fv = FontVec::try_from_vec_and_index(sys.data.clone(), sys.index)
            .expect("fonte validada deve abrir no ab_glyph");
        let defs = egui::FontDefinitions::default();
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
