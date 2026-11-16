//! Marcador de sessão da interface: na abertura seguinte, detecta que o
//! programa não foi fechado normalmente (erro interno, fechamento forçado,
//! falta de energia) e diz o que se sabe sobre isso (AGENTS.md §0.7).
//!
//! Cada sessão da interface cria `logs/sessoes/<pid>-<início>.lock` e o
//! mantém travado (`File::try_lock`) enquanto roda. Só a saída normal
//! ([`end`]) apaga o arquivo. Na abertura seguinte:
//!
//! - um marcador que dá para travar é de uma sessão que terminou sem saída
//!   normal (o sistema solta a trava quando o processo morre, de qualquer
//!   jeito);
//! - um marcador que continua travado é de outra instância aberta, e fica
//!   como está.
//!
//! O gancho de pânico grava uma linha no marcador **da própria sessão**
//! ([`record_panic`]). Assim o relatório não culpa outra instância, a CLI ou
//! os subprocessos de plug-in (que também gravam em `crash.log`) por um
//! pânico que não foi dela.

use crate::util;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Máximo de marcadores examinados numa abertura (§0.5).
const MAX_MARKERS: usize = 64;
/// Maior marcador lido (§0.4).
const MAX_MARKER_BYTES: u64 = 64 * 1024;
/// Máximo de pânicos guardados por sessão.
const MAX_PANICS: usize = 32;
/// Maior texto de um campo do marcador.
const MAX_FIELD_CHARS: usize = 400;
const MARKER_EXT: &str = "lock";

/// Pânico registrado durante uma sessão.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanicNote {
    pub time: String,
    pub thread: String,
    pub location: String,
    pub message: String,
    /// Verdadeiro quando `catch_unwind` / `util::guard` conteve este pânico e
    /// a sessão continuou normalmente.
    pub isolated: bool,
}

impl PanicNote {
    /// Pânico não isolado na thread principal: derruba o laço da interface.
    /// Pânicos contidos por `catch_unwind` ou em threads de trabalho não
    /// fecham o programa.
    pub fn is_fatal(&self) -> bool {
        !self.isolated && self.thread == "main"
    }
}

/// Sessão anterior que não terminou com saída normal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrashReport {
    pub started: String,
    pub version: String,
    pub pid: String,
    pub panics: Vec<PanicNote>,
}

impl CrashReport {
    /// O pânico que fechou o programa, se houve um.
    pub fn fatal_panic(&self) -> Option<&PanicNote> {
        self.panics.iter().find(|p| p.is_fatal())
    }

    /// Pânicos isolados (o programa continuou depois deles).
    pub fn isolated_panics(&self) -> impl Iterator<Item = &PanicNote> {
        self.panics.iter().filter(|p| !p.is_fatal())
    }

    /// Uma frase sobre o que aconteceu.
    pub fn summary(&self) -> String {
        match self.fatal_panic() {
            Some(p) => format!(
                "O Neoveg parou por um erro interno às {}: {}",
                p.time, p.message
            ),
            None => "O Neoveg foi fechado sem aviso: fechamento forçado, falta de energia, \
                     desligamento do sistema ou uma falha que não chegou a ser registrada."
                .into(),
        }
    }

    /// Relatório em texto para copiar (sem dados pessoais: os campos já
    /// passaram por `util::redact` ao serem gravados).
    pub fn report_text(&self) -> String {
        let mut s = String::new();
        s.push_str("Relatório de sessão do Neoveg\n");
        s.push_str(&format!("Versão: {}\n", self.version));
        s.push_str(&format!(
            "Sistema: {} {}\n",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
        s.push_str(&format!("Sessão iniciada em: {}\n", self.started));
        s.push_str(&format!("{}\n", self.summary()));
        if let Some(p) = self.fatal_panic() {
            s.push_str(&format!("Local: {}\n", p.location));
        }
        let isolated: Vec<&PanicNote> = self.isolated_panics().collect();
        if !isolated.is_empty() {
            s.push_str("\nErros internos isolados registrados nesta sessão:\n");
            for p in isolated {
                s.push_str(&format!(
                    "- {} na thread '{}' em {}: {}\n",
                    p.time, p.thread, p.location, p.message
                ));
            }
        }
        s.push_str("\nDetalhes completos (stack trace) em logs/crash.log.\n");
        s
    }
}

/// Marcador da sessão atual, aberto e travado.
pub struct Marker {
    file: File,
    path: PathBuf,
}

static ACTIVE: Mutex<Option<Marker>> = Mutex::new(None);

/// Pasta dos marcadores: `logs/sessoes/` ao lado dos outros logs.
pub fn sessions_dir() -> PathBuf {
    util::base_dir().join("logs").join("sessoes")
}

/// Início da sessão da interface: devolve os relatórios das sessões
/// anteriores que não terminaram normalmente e cria o marcador desta.
/// Chamar uma vez por processo, só no modo de interface.
pub fn begin(version: &str) -> Vec<CrashReport> {
    let dir = sessions_dir();
    let reports = scan_stale(&dir);
    match create_marker(&dir, version) {
        Ok(m) => *util::lock(&ACTIVE) = Some(m),
        Err(e) => util::log_warn(&format!("marcador de sessão não criado: {e}")),
    }
    reports
}

/// Saída normal: apaga o marcador desta sessão.
pub fn end() {
    if let Some(m) = util::lock(&ACTIVE).take() {
        let Marker { file, path } = m;
        drop(file);
        let _ = std::fs::remove_file(path);
    }
}

/// Chamado pelo gancho de pânico: grava o pânico no marcador desta sessão.
/// Nunca bloqueia nem entra em pânico (só `try_lock`, erros ignorados).
pub fn record_panic(thread: &str, location: &str, message: &str) {
    if let Ok(mut guard) = ACTIVE.try_lock() {
        if let Some(m) = guard.as_mut() {
            let _ = write_panic(m, &now(), thread, location, message);
        }
    }
}

/// Marca o último pânico desta thread como isolado (`catch_unwind` conteve a
/// falha e a sessão seguiu).
pub fn mark_isolated() {
    let thread = std::thread::current();
    let name = thread.name().unwrap_or("<sem nome>");
    if let Some(m) = util::lock(&ACTIVE).as_mut() {
        let _ = write_isolated(m, name);
    }
}

/// Verdadeiro se existe outra instância da interface com marcador travado.
pub fn other_instance_active() -> bool {
    let own = util::lock(&ACTIVE).as_ref().map(|m| m.path.clone());
    other_instance_active_in(&sessions_dir(), own.as_deref())
}

pub(crate) fn other_instance_active_in(dir: &Path, own: Option<&Path>) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for path in entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(MARKER_EXT))
        .take(MAX_MARKERS)
    {
        if own.is_some_and(|o| o == path) {
            continue;
        }
        let Ok(f) = OpenOptions::new().read(true).write(true).open(&path) else {
            continue;
        };
        if matches!(f.try_lock(), Err(TryLockError::WouldBlock)) {
            return true;
        }
    }
    false
}

fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Um campo numa linha só, sem tabulações, curto e sem dados pessoais.
fn field(s: &str) -> String {
    util::redact(s)
        .chars()
        .map(|c| {
            if c == '\t' || c == '\n' || c == '\r' {
                ' '
            } else {
                c
            }
        })
        .take(MAX_FIELD_CHARS)
        .collect()
}

pub(crate) fn create_marker(dir: &Path, version: &str) -> std::io::Result<Marker> {
    std::fs::create_dir_all(dir)?;
    let millis = chrono::Local::now().timestamp_millis();
    let pid = std::process::id();
    let path = dir.join(format!("{pid}-{millis}.{MARKER_EXT}"));
    let mut file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&path)?;
    file.try_lock().map_err(|e| match e {
        TryLockError::Error(e) => e,
        TryLockError::WouldBlock => std::io::Error::other("marcador já travado"),
    })?;
    let header = format!(
        "inicio\t{}\nversao\t{}\npid\t{pid}\n",
        now(),
        field(version)
    );
    file.write_all(header.as_bytes())?;
    file.flush()?;
    Ok(Marker { file, path })
}

pub(crate) fn write_panic(
    m: &mut Marker,
    time: &str,
    thread: &str,
    location: &str,
    message: &str,
) -> std::io::Result<()> {
    let line = format!(
        "panico\t{}\t{}\t{}\t{}\n",
        field(time),
        field(thread),
        field(location),
        field(message)
    );
    m.file.write_all(line.as_bytes())?;
    m.file.flush()
}

pub(crate) fn write_isolated(m: &mut Marker, thread: &str) -> std::io::Result<()> {
    let line = format!("isolado\t{}\n", field(thread));
    m.file.write_all(line.as_bytes())?;
    m.file.flush()
}

const CONSUMED_MARKER: &[u8] = b"consumido\n";

/// Examina os marcadores de `dir`: os que dá para travar são de sessões que
/// não terminaram normalmente (viram relatório e são apagados); os que
/// continuam travados são de instâncias abertas e ficam.
pub(crate) fn scan_stale(dir: &Path) -> Vec<CrashReport> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(MARKER_EXT))
        .take(MAX_MARKERS)
        .collect();
    paths.sort();
    let mut out = vec![];
    for path in paths {
        let Ok(mut f) = OpenOptions::new().read(true).write(true).open(&path) else {
            continue;
        };
        match f.try_lock() {
            Ok(()) => {
                // a trava fica com este processo enquanto lê, marca como
                // consumido e apaga: evita relatório duplicado se outra
                // instância já abriu o arquivo antes do remove_file
                let mut bytes = Vec::new();
                let _ = (&mut f).take(MAX_MARKER_BYTES).read_to_end(&mut bytes);
                if bytes != CONSUMED_MARKER {
                    out.push(parse_marker(&String::from_utf8_lossy(&bytes)));
                    let _ = f.set_len(0);
                    let _ = f.rewind();
                    let _ = f.write_all(CONSUMED_MARKER);
                    let _ = f.flush();
                }
                drop(f);
                let _ = std::fs::remove_file(&path);
            }
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(_)) => {}
        }
    }
    out
}

/// Lê um marcador; campos ausentes ou corrompidos ficam como "desconhecido".
pub(crate) fn parse_marker(text: &str) -> CrashReport {
    let unknown = || "desconhecido".to_string();
    let mut r = CrashReport {
        started: unknown(),
        version: unknown(),
        pid: unknown(),
        panics: vec![],
    };
    for line in text.lines() {
        let mut parts = line.split('\t');
        match parts.next() {
            Some("inicio") => {
                if let Some(v) = parts.next() {
                    r.started = v.chars().take(MAX_FIELD_CHARS).collect();
                }
            }
            Some("versao") => {
                if let Some(v) = parts.next() {
                    r.version = v.chars().take(MAX_FIELD_CHARS).collect();
                }
            }
            Some("pid") => {
                if let Some(v) = parts.next() {
                    r.pid = v.chars().take(MAX_FIELD_CHARS).collect();
                }
            }
            Some("panico") if r.panics.len() < MAX_PANICS => {
                let mut next = || parts.next().unwrap_or("").to_string();
                let (time, thread, location, message) = (next(), next(), next(), next());
                r.panics.push(PanicNote {
                    time,
                    thread,
                    location,
                    message,
                    isolated: false,
                });
            }
            Some("isolado") => {
                let thread: String = parts
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(MAX_FIELD_CHARS)
                    .collect();
                if let Some(p) = r
                    .panics
                    .iter_mut()
                    .rev()
                    .find(|p| p.thread == thread && !p.isolated)
                {
                    p.isolated = true;
                }
            }
            _ => {}
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("neoveg_sessao_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn live_marker_is_left_alone_and_stale_one_becomes_a_report() {
        let d = tmp("viva");
        let m = create_marker(&d, "0.1.0").expect("marcador");
        // outra instância "aberta": o marcador continua travado
        assert!(scan_stale(&d).is_empty());
        assert!(m.path.exists());
        // a instância "morre": a trava é solta sem saída normal
        let path = m.path.clone();
        drop(m);
        let reports = scan_stale(&d);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].version, "0.1.0");
        assert!(reports[0].fatal_panic().is_none());
        assert!(!path.exists(), "marcador examinado é apagado");
        assert!(scan_stale(&d).is_empty(), "um relatório só");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn panics_are_tied_to_their_session_and_fatal_is_the_main_thread() {
        let d = tmp("panico");
        let mut m = create_marker(&d, "0.1.0").expect("marcador");
        write_panic(&mut m, "12:00:00", "decodificar", "a.rs:1", "isolado").expect("grava");
        write_panic(&mut m, "12:00:05", "main", "b.rs:2", "fatal\tcom\ttab").expect("grava");
        drop(m);
        let r = scan_stale(&d).pop().expect("relatório");
        assert_eq!(r.panics.len(), 2);
        let fatal = r.fatal_panic().expect("fatal");
        assert_eq!(fatal.location, "b.rs:2");
        assert_eq!(fatal.message, "fatal com tab");
        assert_eq!(r.isolated_panics().count(), 1);
        assert!(r.summary().contains("erro interno"));
        let text = r.report_text();
        assert!(text.contains("b.rs:2") && text.contains("isolado"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn corrupted_marker_still_reports_without_panicking() {
        let d = tmp("corrompido");
        std::fs::create_dir_all(&d).expect("pasta");
        std::fs::write(d.join("1-1.lock"), b"\xff\xfe lixo\npanico\tso-um-campo\n").expect("grava");
        std::fs::write(d.join("2-2.lock"), b"").expect("grava");
        std::fs::write(d.join("nao-e-marcador.txt"), b"x").expect("grava");
        let reports = scan_stale(&d);
        assert_eq!(reports.len(), 2);
        assert!(reports.iter().any(|r| r.panics.len() == 1));
        assert!(reports.iter().all(|r| r.started == "desconhecido"));
        assert!(d.join("nao-e-marcador.txt").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn scan_is_bounded() {
        let d = tmp("limite");
        std::fs::create_dir_all(&d).expect("pasta");
        for i in 0..(MAX_MARKERS + 10) {
            std::fs::write(d.join(format!("{i}-0.lock")), b"inicio\tx\n").expect("grava");
        }
        assert_eq!(scan_stale(&d).len(), MAX_MARKERS);
        assert_eq!(
            scan_stale(&d).len(),
            10,
            "o resto fica para a próxima abertura"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_dir_is_not_an_error() {
        assert!(scan_stale(&tmp("inexistente")).is_empty());
    }

    #[test]
    fn isolated_panic_on_main_thread_is_not_fatal() {
        let d = tmp("isolado_main");
        let mut m = create_marker(&d, "0.1.0").expect("marcador");
        // pânico na thread principal contido por catch_unwind: não pode culpar
        // um fechamento forçado posterior
        write_panic(&mut m, "12:00:00", "main", "a.rs:1", "contido").expect("grava");
        write_isolated(&mut m, "main").expect("marca isolado");
        drop(m);
        let r = scan_stale(&d).pop().expect("relatório");
        assert!(r.fatal_panic().is_none());
        assert_eq!(r.isolated_panics().count(), 1);
        assert!(!r.summary().contains("erro interno"));

        // se houver um segundo pânico na thread principal sem isolamento, este
        // sim é o fatal
        let mut m2 = create_marker(&d, "0.1.0").expect("marcador");
        write_panic(&mut m2, "12:00:00", "main", "a.rs:1", "contido").expect("grava");
        write_isolated(&mut m2, "main").expect("marca isolado");
        write_panic(&mut m2, "12:00:10", "main", "b.rs:2", "fatal").expect("grava");
        drop(m2);
        let r2 = scan_stale(&d).pop().expect("relatório");
        let fatal = r2.fatal_panic().expect("fatal");
        assert_eq!(fatal.message, "fatal");
        assert_eq!(r2.isolated_panics().count(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn consumed_marker_is_not_reported_again_and_live_sibling_is_detected() {
        let d = tmp("consumido");
        std::fs::create_dir_all(&d).expect("pasta");
        std::fs::write(d.join("1-1.lock"), CONSUMED_MARKER).expect("grava");
        assert!(scan_stale(&d).is_empty());
        assert!(!d.join("1-1.lock").exists());

        let m = create_marker(&d, "0.1.0").expect("marcador");
        assert!(!other_instance_active_in(&d, Some(&m.path)));
        assert!(other_instance_active_in(&d, None));
        drop(m);
        let _ = std::fs::remove_dir_all(&d);
    }
}
