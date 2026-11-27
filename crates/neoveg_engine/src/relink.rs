//! Localizar mídia ausente (pesquisa avançada de mídia offline).
//!
//! Quando um projeto aponta para arquivos que não existem mais (pasta movida,
//! disco com outra letra, projeto copiado de outra máquina), o VEGAS Pro
//! pergunta onde estão. Aqui a busca é feita de uma vez para todos os
//! arquivos ausentes: as pastas escolhidas são percorridas uma única vez,
//! comparando o nome de cada arquivo encontrado com o de todas as mídias
//! ausentes. Quando há mais de um arquivo com o mesmo nome, todos viram
//! candidatos, ordenados pela semelhança com o caminho original, e o usuário
//! escolhe um.
//!
//! A árvore de pastas é entrada hostil (§0.4): a travessia é iterativa, não
//! segue links simbólicos nem junções (evita ciclos), tem limite de
//! profundidade, de entradas visitadas e de candidatos por mídia, e pode ser
//! cancelada a qualquer momento.

use crate::model::{Id, MediaItem, MediaSource, Project};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Profundidade máxima de pastas abaixo de cada raiz.
pub const MAX_DEPTH: usize = 48;
/// Entradas (arquivos + pastas) visitadas no máximo por busca.
pub const MAX_ENTRIES: u64 = 3_000_000;
/// Candidatos guardados no máximo por mídia ausente.
pub const MAX_CANDIDATES: usize = 64;
/// Pastas raiz aceitas numa busca.
pub const MAX_ROOTS: usize = 64;

/// Uma mídia do projeto cujo arquivo não foi encontrado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingMedia {
    pub id: Id,
    /// Nome mostrado na Mídia do Projeto.
    pub name: String,
    /// Caminho gravado no projeto.
    pub path: String,
    /// Só o nome do arquivo (o que é procurado).
    pub file_name: String,
}

/// Lista as mídias de arquivo do projeto cujo caminho não existe.
pub fn find_missing(project: &Project) -> Vec<MissingMedia> {
    project
        .media
        .iter()
        .filter_map(|m| {
            let MediaSource::File { path } = &m.source else {
                return None;
            };
            let p = Path::new(path);
            if path.trim().is_empty() || p.exists() {
                return None;
            }
            let file_name = file_name_of(path)?;
            Some(MissingMedia {
                id: m.id,
                name: m.name.clone(),
                path: path.clone(),
                file_name,
            })
        })
        .collect()
}

/// Nome do arquivo de um caminho gravado no projeto, aceitando separadores
/// de Windows e de Unix (o projeto pode vir de outro sistema).
fn file_name_of(path: &str) -> Option<String> {
    let last = path
        .rsplit(['/', '\\'])
        .find(|s| !s.is_empty())?
        .trim()
        .to_string();
    (!last.is_empty() && last != "." && last != "..").then_some(last)
}

/// Chave de comparação de nomes: sem diferença de maiúsculas e minúsculas,
/// como nos sistemas de arquivos do Windows e do macOS.
fn name_key(name: &str) -> String {
    name.to_lowercase()
}

/// Pastas para começar a busca: a pasta do projeto e, para cada mídia
/// ausente, a pasta existente mais próxima do caminho original. Pastas que
/// já estão dentro de outra da lista são removidas.
pub fn default_roots(project_path: Option<&Path>, missing: &[MissingMedia]) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(dir) = project_path.and_then(Path::parent) {
        if dir.is_dir() {
            roots.push(dir.to_path_buf());
        }
    }
    for m in missing {
        let mut cur = Path::new(&m.path).parent();
        while let Some(dir) = cur {
            if dir.as_os_str().is_empty() {
                break;
            }
            if dir.is_dir() {
                // a raiz de um disco inteiro é grande demais para começar
                if dir.parent().is_some() {
                    roots.push(dir.to_path_buf());
                }
                break;
            }
            cur = dir.parent();
        }
    }
    normalize_roots(roots)
}

/// Remove repetidas e as que estão dentro de outra raiz da lista.
pub fn normalize_roots(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut uniq: Vec<PathBuf> = roots
        .into_iter()
        .filter(|r| seen.insert(r.clone()))
        .take(MAX_ROOTS)
        .collect();
    uniq.sort_by_key(|r| r.components().count());
    let mut out: Vec<PathBuf> = Vec::new();
    for r in uniq {
        if !out.iter().any(|o| r.starts_with(o)) {
            out.push(r);
        }
    }
    out
}

/// Progresso compartilhado com a interface (contadores atômicos e pedido de
/// cancelamento).
#[derive(Debug, Default)]
pub struct SearchState {
    pub visited: AtomicU64,
    pub matches: AtomicU64,
    pub cancel: AtomicBool,
}

impl SearchState {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Resultado de uma busca.
#[derive(Clone, Debug, Default)]
pub struct SearchResult {
    /// Candidatos de cada mídia ausente, o mais provável primeiro.
    pub candidates: HashMap<Id, Vec<PathBuf>>,
    /// Entradas visitadas.
    pub visited: u64,
    /// A busca parou no limite de entradas.
    pub truncated: bool,
    /// A busca foi cancelada.
    pub cancelled: bool,
}

/// Procura de uma vez todos os arquivos ausentes nas pastas `roots`.
pub fn search(roots: &[PathBuf], missing: &[MissingMedia], state: &SearchState) -> SearchResult {
    search_with_limits(roots, missing, state, MAX_DEPTH, MAX_ENTRIES)
}

fn search_with_limits(
    roots: &[PathBuf],
    missing: &[MissingMedia],
    state: &SearchState,
    max_depth: usize,
    max_entries: u64,
) -> SearchResult {
    let mut result = SearchResult::default();
    // nome (sem caixa) -> mídias ausentes com esse nome
    let mut wanted: HashMap<String, Vec<Id>> = HashMap::new();
    for m in missing {
        wanted.entry(name_key(&m.file_name)).or_default().push(m.id);
    }
    if wanted.is_empty() {
        return result;
    }
    let mut found: HashMap<Id, Vec<PathBuf>> = HashMap::new();
    let mut seen_files: HashSet<PathBuf> = HashSet::new();
    let mut stack: Vec<(PathBuf, usize)> = normalize_roots(roots.to_vec())
        .into_iter()
        .map(|r| (r, 0))
        .collect();
    'walk: while let Some((dir, depth)) = stack.pop() {
        if state.cancelled() {
            result.cancelled = true;
            break;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue; // sem permissão, removida durante a busca etc.
        };
        let mut subdirs = Vec::new();
        for entry in rd.flatten() {
            result.visited = result.visited.saturating_add(1);
            if result.visited % 1024 == 0 {
                state.visited.store(result.visited, Ordering::Relaxed);
                if state.cancelled() {
                    result.cancelled = true;
                    break 'walk;
                }
            }
            if result.visited >= max_entries {
                result.truncated = true;
                break 'walk;
            }
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            // links simbólicos e junções não são seguidos (ciclos)
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                if depth < max_depth {
                    subdirs.push(entry.path());
                }
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(ids) = wanted.get(&name_key(name)) else {
                continue;
            };
            let path = entry.path();
            if !seen_files.insert(path.clone()) {
                continue;
            }
            for id in ids {
                let list = found.entry(*id).or_default();
                if list.len() < MAX_CANDIDATES {
                    list.push(path.clone());
                    state.matches.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        // ordem estável: as subpastas são visitadas em ordem alfabética
        subdirs.sort();
        stack.extend(subdirs.into_iter().rev().map(|d| (d, depth + 1)));
    }
    state.visited.store(result.visited, Ordering::Relaxed);
    let by_id: HashMap<Id, &MissingMedia> = missing.iter().map(|m| (m.id, m)).collect();
    for (id, mut list) in found {
        if let Some(m) = by_id.get(&id) {
            rank_candidates(&m.path, &mut list);
        }
        result.candidates.insert(id, list);
    }
    result
}

/// Ordena os candidatos: primeiro os que têm mais pastas finais em comum com
/// o caminho original (ex.: `Projeto/Áudio/voz.wav` ganha de `Backup/voz.wav`),
/// depois pelo caminho.
pub fn rank_candidates(original: &str, list: &mut [PathBuf]) {
    let orig: Vec<String> = original
        .split(['/', '\\'])
        .filter(|s| !s.is_empty())
        .map(name_key)
        .collect();
    let score = |p: &Path| -> usize {
        let comps: Vec<String> = p
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => s.to_str().map(name_key),
                _ => None,
            })
            .collect();
        comps
            .iter()
            .rev()
            .zip(orig.iter().rev())
            .take_while(|(a, b)| a == b)
            .count()
    };
    list.sort_by(|a, b| score(b).cmp(&score(a)).then_with(|| a.cmp(b)));
}

/// Escolha automática: só quando há exatamente um candidato. Com vários
/// arquivos do mesmo nome, o usuário escolhe na lista.
pub fn auto_choice(result: &SearchResult, id: Id) -> Option<&PathBuf> {
    match result.candidates.get(&id).map(Vec::as_slice) {
        Some([only]) => Some(only),
        _ => None,
    }
}

/// Aponta as mídias para os arquivos escolhidos. Ignora ids desconhecidos,
/// mídias que não são de arquivo e caminhos vazios ou inexistentes. Devolve
/// quantas mídias mudaram.
pub fn apply_relinks(media: &mut [MediaItem], choices: &[(Id, PathBuf)]) -> usize {
    let mut n = 0;
    for (id, new_path) in choices {
        if !new_path.is_file() {
            continue;
        }
        let Some(m) = media.iter_mut().find(|m| m.id == *id) else {
            continue;
        };
        let MediaSource::File { path } = &mut m.source else {
            continue;
        };
        let s = new_path.to_string_lossy().to_string();
        if *path != s {
            *path = s;
            // o proxy era do arquivo antigo
            m.proxy_path = None;
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn media(id: Id, path: &str) -> MediaItem {
        MediaItem {
            id,
            name: file_name_of(path).unwrap_or_default(),
            source: MediaSource::File { path: path.into() },
            duration: None,
            has_video: true,
            has_audio: false,
            width: 0,
            height: 0,
            fps: None,
            info: String::new(),
            proxy_path: Some("proxy_antigo.avi".into()),
        }
    }

    /// Pasta temporária própria do teste, removida no fim.
    struct TmpDir(PathBuf);
    impl TmpDir {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "neoveg_relink_{tag}_{}_{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            TmpDir(p)
        }
        fn file(&self, rel: &str) -> PathBuf {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, b"x").unwrap();
            p
        }
    }
    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn acha_todas_as_ausentes_de_uma_vez_e_lista_os_homonimos() {
        let t = TmpDir::new("busca");
        let voz = t.file("Projeto/Audio/voz.wav");
        let voz_backup = t.file("Backup/voz.wav");
        let cena = t.file("Projeto/Video/Cena 1.MP4");
        t.file("Outros/nada.txt");

        let mut p = Project::new_default();
        p.media.push(media(1, "Z:/Sumiu/Projeto/Audio/voz.wav"));
        p.media.push(media(2, "Z:\\Sumiu\\Video\\cena 1.mp4"));
        p.media.push(media(3, "Z:/Sumiu/nunca_existiu.mov"));
        let missing = find_missing(&p);
        assert_eq!(missing.len(), 3);

        let st = SearchState::default();
        let r = search(std::slice::from_ref(&t.0), &missing, &st);
        assert!(!r.truncated && !r.cancelled);
        // homônimos: os dois aparecem, o de pastas parecidas primeiro
        assert_eq!(r.candidates.get(&1), Some(&vec![voz.clone(), voz_backup]));
        assert!(
            auto_choice(&r, 1).is_none(),
            "dois candidatos: o usuário escolhe"
        );
        // nome sem diferença de caixa
        assert_eq!(auto_choice(&r, 2), Some(&cena));
        assert!(!r.candidates.contains_key(&3));
        assert!(st.visited.load(Ordering::Relaxed) > 0);

        let n = apply_relinks(&mut p.media, &[(1, voz.clone()), (2, cena.clone())]);
        assert_eq!(n, 2);
        assert_eq!(find_missing(&p).len(), 1);
        // reaplicar não muda nada; caminho inexistente e id desconhecido são ignorados
        assert_eq!(apply_relinks(&mut p.media, &[(1, voz)]), 0);
        assert_eq!(
            apply_relinks(&mut p.media, &[(3, t.0.join("nao.mov")), (99, cena)]),
            0
        );
    }

    #[test]
    fn limites_e_cancelamento_param_a_busca() {
        let t = TmpDir::new("limites");
        for i in 0..20 {
            t.file(&format!("d{i}/arq{i}.wav"));
        }
        t.file("a/b/c/d/e/fundo.wav");
        let missing = vec![
            MissingMedia {
                id: 1,
                name: "fundo".into(),
                path: "X:/fundo.wav".into(),
                file_name: "fundo.wav".into(),
            },
            MissingMedia {
                id: 2,
                name: "arq3".into(),
                path: "X:/arq3.wav".into(),
                file_name: "arq3.wav".into(),
            },
        ];
        let st = SearchState::default();
        // profundidade 2 não chega em a/b/c/d/e
        let r = search_with_limits(std::slice::from_ref(&t.0), &missing, &st, 2, MAX_ENTRIES);
        assert!(!r.candidates.contains_key(&1));
        assert!(r.candidates.contains_key(&2));
        // limite de entradas
        let r = search_with_limits(std::slice::from_ref(&t.0), &missing, &st, MAX_DEPTH, 5);
        assert!(r.truncated);
        // cancelado antes de começar
        let st = SearchState::default();
        st.cancel();
        let r = search(std::slice::from_ref(&t.0), &missing, &st);
        assert!(r.cancelled && r.candidates.is_empty());
    }

    #[test]
    fn entradas_hostis_nao_travam() {
        let st = SearchState::default();
        let m = vec![MissingMedia {
            id: 1,
            name: String::new(),
            path: String::new(),
            file_name: "x".into(),
        }];
        // raízes inexistentes, vazias ou repetidas
        let roots = vec![
            PathBuf::from("Z:/nao/existe"),
            PathBuf::new(),
            PathBuf::from("Z:/nao/existe"),
        ];
        let r = search(&roots, &m, &st);
        assert!(r.candidates.is_empty());
        assert!(search(&roots, &[], &st).candidates.is_empty());
        // nomes de arquivo estranhos no projeto
        assert_eq!(file_name_of(""), None);
        assert_eq!(file_name_of("C:\\"), Some("C:".into()));
        assert_eq!(file_name_of("a/b/"), Some("b".into()));
        assert_eq!(file_name_of(".."), None);
        let mut list = vec![PathBuf::from("b/x"), PathBuf::from("a/x")];
        rank_candidates("", &mut list);
        assert_eq!(list, vec![PathBuf::from("a/x"), PathBuf::from("b/x")]);
    }

    #[test]
    fn raizes_padrao_sem_repeticao_nem_aninhadas() {
        let t = TmpDir::new("raizes");
        t.file("proj/sub/a.wav");
        let proj = t.0.join("proj").join("projeto.neoveg");
        let missing = vec![MissingMedia {
            id: 1,
            name: "a".into(),
            path: t
                .0
                .join("proj/sub/sumiu/a.wav")
                .to_string_lossy()
                .to_string(),
            file_name: "a.wav".into(),
        }];
        // a pasta do projeto já contém proj/sub: fica só ela
        let roots = default_roots(Some(&proj), &missing);
        assert_eq!(roots, vec![t.0.join("proj")]);
        assert_eq!(
            normalize_roots(vec![t.0.join("x/y"), t.0.join("x"), t.0.join("x")]),
            vec![t.0.join("x")]
        );
    }
}
