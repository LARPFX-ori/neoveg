//! Internacionalização e localização (i18n / l10n) do Neoveg.
//!
//! Suporta seis idiomas completos carregados a partir dos arquivos JSON em
//! `i18n/*.json` (embutidos no binário e com suporte opcional a sobrescrita em
//! disco em tempo de execução), com proteção total contra entradas hostis
//! (AGENTS.md §0):
//! - Português (Brasil) (`pt-BR`, padrão)
//! - Inglês (`en`)
//! - Espanhol (`es`)
//! - Francês (`fr`)
//! - Alemão (`de`)
//! - Japonês (`ja`)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

/// Tamanho máximo aceito para um arquivo JSON de idioma externo (4 MiB, AGENTS.md §0.4).
pub const MAX_CATALOG_BYTES: usize = 4 * 1024 * 1024;

/// Idiomas suportados pela interface e pelo motor de comandos do Neoveg.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "pt-BR", alias = "pt", alias = "pt_BR", alias = "pt-PT")]
    PtBr,
    #[serde(rename = "en", alias = "en-US", alias = "en-GB", alias = "en_US")]
    En,
    #[serde(rename = "es", alias = "es-ES", alias = "es-419", alias = "es_ES")]
    Es,
    #[serde(rename = "fr", alias = "fr-FR", alias = "fr-CA", alias = "fr_FR")]
    Fr,
    #[serde(rename = "de", alias = "de-DE", alias = "de-AT", alias = "de_DE")]
    De,
    #[serde(rename = "ja", alias = "ja-JP", alias = "ja_JP", alias = "jp")]
    Ja,
}

const EMBEDDED_JSON: [(Language, &str); 6] = [
    (Language::PtBr, include_str!("../../../../i18n/pt-BR.json")),
    (Language::En, include_str!("../../../../i18n/en.json")),
    (Language::Es, include_str!("../../../../i18n/es.json")),
    (Language::Fr, include_str!("../../../../i18n/fr.json")),
    (Language::De, include_str!("../../../../i18n/de.json")),
    (Language::Ja, include_str!("../../../../i18n/ja.json")),
];

impl Language {
    pub const ALL: [Language; 6] = [
        Language::PtBr,
        Language::En,
        Language::Es,
        Language::Fr,
        Language::De,
        Language::Ja,
    ];

    /// Código BCP-47 canônico persistido em `preferencias.json` e `--lang`.
    pub const fn code(self) -> &'static str {
        match self {
            Language::PtBr => "pt-BR",
            Language::En => "en",
            Language::Es => "es",
            Language::Fr => "fr",
            Language::De => "de",
            Language::Ja => "ja",
        }
    }

    /// Sigla curta de duas letras para botões compactos.
    pub const fn short_code(self) -> &'static str {
        match self {
            Language::PtBr => "PT",
            Language::En => "EN",
            Language::Es => "ES",
            Language::Fr => "FR",
            Language::De => "DE",
            Language::Ja => "JA",
        }
    }

    /// Nome nativo do próprio idioma (sempre legível mesmo ao trocar de idioma).
    pub const fn native_name(self) -> &'static str {
        match self {
            Language::PtBr => "Português (Brasil)",
            Language::En => "English",
            Language::Es => "Español",
            Language::Fr => "Français",
            Language::De => "Deutsch",
            Language::Ja => "日本語",
        }
    }

    const fn as_u8(self) -> u8 {
        match self {
            Language::PtBr => 0,
            Language::En => 1,
            Language::Es => 2,
            Language::Fr => 3,
            Language::De => 4,
            Language::Ja => 5,
        }
    }

    const fn from_u8(v: u8) -> Self {
        match v {
            1 => Language::En,
            2 => Language::Es,
            3 => Language::Fr,
            4 => Language::De,
            5 => Language::Ja,
            _ => Language::PtBr,
        }
    }

    /// Nome do idioma traduzido para `ui_lang`.
    pub fn localized_name(self, ui_lang: Language) -> &'static str {
        let pt = match self {
            Language::PtBr => "Português (Brasil)",
            Language::En => "Inglês",
            Language::Es => "Espanhol",
            Language::Fr => "Francês",
            Language::De => "Alemão",
            Language::Ja => "Japonês",
        };
        tr(ui_lang, pt)
    }

    /// Rótulo para seletores: `"Nome nativo — Traduzido"` quando diferente.
    pub fn display_option(self, ui_lang: Language) -> String {
        let native = self.native_name();
        let loc = self.localized_name(ui_lang);
        if native == loc {
            native.to_string()
        } else {
            format!("{native} ({loc})")
        }
    }

    /// Interpreta códigos BCP-47/POSIX e nomes de idioma sem entrar em pânico.
    pub fn from_code(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.len() > 32 || trimmed.contains('\0') {
            return None;
        }
        if trimmed == "日本語" {
            return Some(Language::Ja);
        }
        // Remove sufixo POSIX (.UTF-8) se presente.
        let base = trimmed.split('.').next().unwrap_or(trimmed).trim();
        let norm: String = base
            .chars()
            .map(|c| {
                if c == '_' {
                    '-'
                } else {
                    c.to_ascii_lowercase()
                }
            })
            .collect();

        if matches!(
            norm.as_str(),
            "pt" | "pt-br"
                | "pt-pt"
                | "portugues"
                | "português"
                | "portuguese"
                | "português (brasil)"
        ) {
            return Some(Language::PtBr);
        }
        if matches!(
            norm.as_str(),
            "en" | "en-us"
                | "en-gb"
                | "en-ca"
                | "en-au"
                | "english"
                | "ingles"
                | "inglês"
                | "anglais"
                | "englisch"
                | "inglés"
        ) {
            return Some(Language::En);
        }
        if matches!(
            norm.as_str(),
            "es" | "es-es"
                | "es-419"
                | "es-mx"
                | "es-ar"
                | "spanish"
                | "espanhol"
                | "espanol"
                | "español"
                | "espagnol"
                | "spanisch"
        ) {
            return Some(Language::Es);
        }
        if matches!(
            norm.as_str(),
            "fr" | "fr-fr"
                | "fr-ca"
                | "fr-ch"
                | "french"
                | "frances"
                | "francês"
                | "francais"
                | "français"
                | "francesa"
                | "französisch"
        ) {
            return Some(Language::Fr);
        }
        if matches!(
            norm.as_str(),
            "de" | "de-de"
                | "de-at"
                | "de-ch"
                | "german"
                | "deutsch"
                | "alemao"
                | "alemão"
                | "allemand"
                | "alemán"
        ) {
            return Some(Language::De);
        }
        if matches!(
            norm.as_str(),
            "ja" | "ja-jp"
                | "jp"
                | "japanese"
                | "japones"
                | "japonês"
                | "japonés"
                | "japonais"
                | "japanisch"
        ) {
            return Some(Language::Ja);
        }
        None
    }

    pub fn from_pref(raw: &str) -> Self {
        Self::from_code(raw).unwrap_or(Language::PtBr)
    }
}

static ACTIVE_LANG: AtomicU8 = AtomicU8::new(0);

/// Idioma padrão do processo atual (usado por helpers que não recebem `&Preferences`).
pub fn active_language() -> Language {
    Language::from_u8(ACTIVE_LANG.load(Ordering::Relaxed))
}

/// Atualiza o idioma padrão do processo.
pub fn set_active_language(lang: Language) {
    ACTIVE_LANG.store(lang.as_u8(), Ordering::Relaxed);
}

struct Catalog {
    tables: [HashMap<String, String>; 6],
}

static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// Analisa um catálogo JSON `{ "chave": "tradução" }` validando tamanho e
/// descartando entradas vazias ou com bytes NUL (AGENTS.md §0.4).
pub fn parse_json_catalog(raw: &str) -> Result<HashMap<String, String>, String> {
    if raw.len() > MAX_CATALOG_BYTES {
        return Err(format!("catálogo i18n excede {MAX_CATALOG_BYTES} bytes"));
    }
    let parsed: HashMap<String, String> =
        serde_json::from_str(raw).map_err(|e| format!("JSON i18n inválido: {e}"))?;
    let mut out = HashMap::with_capacity(parsed.len());
    for (k, v) in parsed {
        if !k.is_empty() && !v.trim().is_empty() && !k.contains('\0') && !v.contains('\0') {
            out.insert(k, v);
        }
    }
    Ok(out)
}

/// Lê um arquivo `.json` de idioma do disco com limite de tamanho e sem pânico.
pub fn load_json_catalog_file(path: &Path) -> Result<HashMap<String, String>, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("metadados {}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("não é arquivo regular: {}", path.display()));
    }
    if meta.len() > MAX_CATALOG_BYTES as u64 {
        return Err(format!(
            "arquivo i18n {} excede {MAX_CATALOG_BYTES} bytes",
            path.display()
        ));
    }
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("leitura {}: {e}", path.display()))?;
    parse_json_catalog(&content)
}

#[cfg(not(test))]
fn candidate_i18n_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join("i18n"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.join("i18n"));
        }
    }
    if let Some(env_dir) = std::env::var_os("NEOVEG_I18N_DIR") {
        if !env_dir.is_empty() {
            dirs.push(PathBuf::from(env_dir));
        }
    }
    dirs
}

#[cfg(not(test))]
fn overlay_external_json(lang: Language, dst: &mut HashMap<String, String>) {
    for dir in candidate_i18n_dirs() {
        let file = dir.join(format!("{}.json", lang.code()));
        if let Ok(extra) = load_json_catalog_file(&file) {
            dst.extend(extra);
        }
    }
}

#[cfg(test)]
fn overlay_external_json(_lang: Language, _dst: &mut HashMap<String, String>) {}

fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(|| {
        let mut tables: [HashMap<String, String>; 6] = Default::default();
        for &(lang, raw) in &EMBEDDED_JSON {
            let mut map = parse_json_catalog(raw).unwrap_or_default();
            overlay_external_json(lang, &mut map);
            tables[lang.as_u8() as usize] = map;
        }
        Catalog { tables }
    })
}

/// Retorna o caminho da pasta `i18n` ao lado do executável ou diretório atual.
pub fn runtime_i18n_dir() -> Option<PathBuf> {
    if let Some(env_dir) = std::env::var_os("NEOVEG_I18N_DIR") {
        let p = PathBuf::from(env_dir);
        if p.is_dir() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let p = parent.join("i18n");
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        let p = cwd.join("i18n");
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}

/// Retorna `true` se `pt` estiver presente no catálogo de traduções.
pub fn has_key(pt: &str) -> bool {
    catalog().tables[0].contains_key(pt)
}

/// Traduz uma frase em português do Brasil para `lang`.
/// Retorna o texto original caso `lang == PtBr` ou a chave não esteja no catálogo.
pub fn tr(lang: Language, pt: &str) -> &str {
    let idx = lang.as_u8() as usize;
    catalog().tables[idx].get(pt).map_or(pt, String::as_str)
}

/// Escolhe diretamente entre 6 strings fixas `[pt-BR, en, es, fr, de, ja]`.
pub const fn pick(lang: Language, row: [&'static str; 6]) -> &'static str {
    row[lang.as_u8() as usize]
}

/// Traduz um molde contendo marcadores nomeados `{nome}` e substitui cada valor,
/// preservando a ordem gramatical natural de cada idioma (ex.: japonês e alemão).
pub fn tr_args(lang: Language, pt: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = tr(lang, pt).to_string();
    for (name, val) in args {
        let token = format!("{{{name}}}");
        let rendered = val.to_string();
        out = out.replace(&token, &rendered);
    }
    out
}

/// Divide `pt_path` em segmentos de menu preservando rótulos que contêm `/`
/// (como `Pan/Crop` ou `(SRT/WebVTT)`).
pub fn split_menu_path(pt_path: &str) -> Vec<&str> {
    let mut parts = Vec::with_capacity(4);
    let mut rest = pt_path;
    for _ in 0..16 {
        if has_key(rest) {
            parts.push(rest);
            return parts;
        }
        if let Some((head, tail)) = rest.split_once('/') {
            parts.push(head);
            rest = tail;
        } else {
            break;
        }
    }
    if !rest.is_empty() {
        parts.push(rest);
    }
    parts
}

/// Traduz todos os segmentos de um caminho de menu sem quebrar barras dentro de rótulos.
pub fn tr_menu_path(lang: Language, pt_path: &str) -> String {
    if lang == Language::PtBr {
        return pt_path.to_string();
    }
    split_menu_path(pt_path)
        .into_iter()
        .map(|seg| tr(lang, seg))
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn extract_placeholders(s: &str) -> BTreeSet<String> {
        let mut set = BTreeSet::new();
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'{' {
                if let Some(end) = s[i + 1..].find('}') {
                    let name = &s[i + 1..i + 1 + end];
                    if !name.is_empty() && !name.contains('{') {
                        set.insert(name.to_string());
                    }
                    i += end + 2;
                    continue;
                }
            }
            i += 1;
        }
        set
    }

    #[test]
    fn language_codes_aliases_and_hostile_inputs() {
        assert_eq!(Language::ALL.len(), 6);
        for lang in Language::ALL {
            assert_eq!(Language::from_code(lang.code()), Some(lang));
            assert_eq!(
                Language::from_code(&lang.code().to_ascii_uppercase()),
                Some(lang)
            );
            assert_eq!(Language::from_code(lang.native_name()), Some(lang));
        }
        assert_eq!(Language::from_code("en_US.UTF-8"), Some(Language::En));
        assert_eq!(Language::from_code("ja_JP.UTF-8"), Some(Language::Ja));
        assert_eq!(Language::from_code("de-AT"), Some(Language::De));
        assert_eq!(Language::from_code("fr_CA"), Some(Language::Fr));
        assert_eq!(Language::from_code("es-419"), Some(Language::Es));
        assert_eq!(Language::from_code(""), None);
        assert_eq!(Language::from_code("ja\0"), None);
        assert_eq!(Language::from_code(&"a".repeat(64)), None);
        assert_eq!(Language::from_pref("xx-INVALID"), Language::PtBr);
    }

    fn parse_strict_unique_json(code: &str, raw: &str) -> HashMap<String, String> {
        use serde::de::{Deserializer, MapAccess, Visitor};
        struct UniqueMap<'a>(&'a str);
        impl<'de, 'a> Visitor<'de> for UniqueMap<'a> {
            type Value = HashMap<String, String>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "a JSON object for language {}", self.0)
            }
            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut map = HashMap::new();
                while let Some((k, v)) = access.next_entry::<String, String>()? {
                    assert!(!k.is_empty(), "chave vazia em {0}.json", self.0);
                    assert!(
                        !v.trim().is_empty(),
                        "tradução vazia em {}.json para chave {k:?}",
                        self.0
                    );
                    assert!(
                        map.insert(k.clone(), v).is_none(),
                        "chave duplicada em {}.json: {k:?}",
                        self.0
                    );
                }
                Ok(map)
            }
        }
        let mut de = serde_json::Deserializer::from_str(raw);
        let map = de
            .deserialize_map(UniqueMap(code))
            .unwrap_or_else(|e| panic!("falha ao validar {code}.json: {e}"));
        assert!(!map.is_empty(), "arquivo {code}.json não pode ser vazio");
        map
    }

    #[test]
    fn catalog_json_files_have_identical_keys_no_duplicates_and_no_empty_values() {
        let mut parsed: Vec<(Language, HashMap<String, String>)> = Vec::new();
        for &(lang, raw) in &EMBEDDED_JSON {
            parsed.push((lang, parse_strict_unique_json(lang.code(), raw)));
        }
        let base_keys: BTreeSet<&String> = parsed[0].1.keys().collect();
        for (lang, map) in &parsed[1..] {
            let cur_keys: BTreeSet<&String> = map.keys().collect();
            assert_eq!(
                cur_keys,
                base_keys,
                "chaves divergentes entre pt-BR.json e {}.json",
                lang.code()
            );
        }
    }

    #[test]
    fn catalog_placeholders_match_across_all_languages() {
        let mut parsed: Vec<(Language, HashMap<String, String>)> = Vec::new();
        for &(lang, raw) in &EMBEDDED_JSON {
            parsed.push((lang, parse_strict_unique_json(lang.code(), raw)));
        }
        for (key, pt_val) in &parsed[0].1 {
            let expected = extract_placeholders(key);
            assert_eq!(extract_placeholders(pt_val), expected);
            for (lang, map) in &parsed[1..] {
                let val = &map[key];
                let actual = extract_placeholders(val);
                assert_eq!(
                    actual,
                    expected,
                    "marcadores divergentes em {}.json para {key:?}: esperado {expected:?}, obtido {actual:?}",
                    lang.code()
                );
            }
        }
    }

    #[test]
    fn hostile_json_catalog_never_panics() {
        assert!(parse_json_catalog("").is_err());
        assert!(parse_json_catalog("null").is_err());
        assert!(parse_json_catalog("[1, 2, 3]").is_err());
        assert!(parse_json_catalog("{broken").is_err());
        let big = format!("{{\"a\":\"{}\"}}", "x".repeat(MAX_CATALOG_BYTES + 1));
        assert!(parse_json_catalog(&big).is_err());
        let sanitized = parse_json_catalog(
            r#"{"":"skip","blank":"   ","nul":"ok\u0000bad","ok":"Translated"}"#,
        )
        .expect("JSON sintaticamente válido");
        assert_eq!(sanitized.len(), 1);
        assert_eq!(sanitized.get("ok").map(String::as_str), Some("Translated"));
        assert!(load_json_catalog_file(Path::new("non_existent_path_12345.json")).is_err());
    }

    #[test]
    fn tr_args_reorders_cleanly_for_all_languages() {
        for lang in Language::ALL {
            let s = tr_args(lang, "Projeto aberto: {path}", &[("path", &"demo.neoveg")]);
            assert!(s.contains("demo.neoveg"));
            assert!(!s.contains("{path}"));
        }
    }
}
