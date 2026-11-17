//! Carregamento seguro de fontes de fallback do sistema operacional para escrita
//! japonesa (hiragana, katakana e kanji) em tempo de execução (AGENTS.md §1.5).
//!
//! Nenhuma fonte proprietária é incorporada ou distribuída no repositório.

use ab_glyph::{Font as _, FontVec};
use eframe::egui;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

const MAX_SYSTEM_FONT_BYTES: u64 = 64 * 1024 * 1024;
const CJK_FONT_KEY: &str = "neoveg_sys_jp";

#[derive(Clone)]
struct LoadedSystemFont {
    data: Vec<u8>,
    index: u32,
}

static CJK_FONT_CACHE: OnceLock<Option<LoadedSystemFont>> = OnceLock::new();

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

fn read_bounded_font(path: &Path) -> Option<LoadedSystemFont> {
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
    let index = validate_japanese_font(&buf)?;
    Some(LoadedSystemFont { data: buf, index })
}

fn cached_japanese_fallback() -> Option<&'static LoadedSystemFont> {
    CJK_FONT_CACHE
        .get_or_init(|| {
            for p in candidate_japanese_font_paths() {
                if let Some(loaded) = read_bounded_font(&p) {
                    return Some(loaded);
                }
            }
            None
        })
        .as_ref()
}

/// Configura fontes de fallback do sistema para suportar glifos japoneses sem
/// incluir fontes proprietárias no repositório (AGENTS.md §0 / §1.5).
pub fn configure_fonts(ctx: &egui::Context) {
    let Some(sys_font) = cached_japanese_fallback() else {
        return;
    };
    let mut defs = egui::FontDefinitions::default();
    let mut fd = egui::FontData::from_owned(sys_font.data.clone());
    fd.index = sys_font.index;
    defs.font_data.insert(CJK_FONT_KEY.to_owned(), Arc::new(fd));
    for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        if let Some(chain) = defs.families.get_mut(&fam) {
            chain.push(CJK_FONT_KEY.to_owned());
        }
    }
    ctx.set_fonts(defs);
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
