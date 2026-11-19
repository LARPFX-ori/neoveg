//! Utilitários de robustez exigidos pelo AGENTS.md §0 (Nunca Travar):
//! locks tolerantes a envenenamento, `clamp` que não entra em pânico,
//! isolamento de pânicos em workers, log em `logs/neoveg.log`, hook de
//! pânico em `logs/crash.log` e recuperação automática do projeto.

use std::any::Any;
use std::fs::OpenOptions;
use std::io::Write;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

/// Trava um `Mutex` sem entrar em pânico se ele estiver envenenado: o dado
/// continua acessível (§0.6).
pub fn lock<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `clamp` seguro: nunca entra em pânico com limites cruzados ou NaN
/// (`f32::clamp` entra em pânico se `min > max`).
pub fn clampf(v: f32, lo: f32, hi: f32) -> f32 {
    if v.is_nan() || hi < lo {
        return lo;
    }
    v.max(lo).min(hi)
}

/// Extrai a mensagem de um payload de pânico.
pub fn panic_message(p: &(dyn Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "pânico sem mensagem".to_string()
    }
}

/// Executa `f` sob `catch_unwind` e, se houver pânico, marca-o como isolado no
/// marcador da sessão para não ser apontado como queda fatal.
pub fn catch_isolated<T>(f: impl FnOnce() -> T) -> std::thread::Result<T> {
    let r = std::panic::catch_unwind(AssertUnwindSafe(f));
    if r.is_err() {
        crate::session_marker::mark_isolated();
    }
    r
}

/// Executa `f` isolando pânicos (§0.6): um pânico vira `Err` com contexto e é
/// registrado no log, em vez de matar a thread silenciosamente.
pub fn guard<T>(what: &str, f: impl FnOnce() -> T) -> Result<T, String> {
    catch_isolated(f).map_err(|p| {
        let msg = format!(
            "{what}: falha interna isolada ({})",
            panic_message(p.as_ref())
        );
        log_error(&msg);
        msg
    })
}

/// Cria uma thread nomeada sem entrar em pânico se o sistema recusar
/// (`std::thread::spawn` entra em pânico nesse caso).
pub fn spawn(name: &str, f: impl FnOnce() + Send + 'static) -> Result<(), String> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(f)
        .map(|_| ())
        .map_err(|e| format!("não foi possível criar a thread '{name}': {e}"))
}

// ---------------------------------------------------------------------------
// Logs
// ---------------------------------------------------------------------------

static BASE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Arquivo que o instalador grava ao lado do executável. Com ele, o Neoveg
/// está instalado (e não portátil): os dados do usuário vão para a pasta de
/// dados do usuário, porque a pasta do programa pode não ser gravável
/// (Arquivos de Programas) e é apagada ao desinstalar.
pub const INSTALL_MARKER: &str = "instalado.json";

/// Nome da pasta de dados do usuário quando instalado (`%APPDATA%\Neoveg`).
pub const USER_DATA_DIR_NAME: &str = "Neoveg";

/// Escolhe a pasta base (regra pura, testável):
/// - instalado e com pasta de dados do usuário conhecida: `<appdata>/Neoveg`;
/// - portátil com a pasta do executável gravável: a pasta do executável;
/// - senão a pasta de dados do usuário e, por último, a temporária.
pub fn choose_base_dir(
    exe_dir: Option<PathBuf>,
    installed: bool,
    exe_dir_writable: bool,
    user_data: Option<PathBuf>,
    temp: PathBuf,
) -> PathBuf {
    let user_dir = user_data
        .filter(|d| d.is_absolute())
        .map(|d| d.join(USER_DATA_DIR_NAME));
    if installed {
        if let Some(u) = user_dir.clone() {
            return u;
        }
    }
    if let Some(d) = exe_dir {
        if !installed && exe_dir_writable {
            return d;
        }
    }
    user_dir.unwrap_or_else(|| temp.join("neoveg"))
}

fn dir_is_writable(d: &std::path::Path) -> bool {
    let probe = d.join("logs");
    std::fs::create_dir_all(&probe).is_ok()
        && OpenOptions::new()
            .create(true)
            .append(true)
            .open(probe.join("neoveg.log"))
            .is_ok()
}

/// Pasta base para `logs/`, `autosave/`, preferências e layouts: ao lado do
/// executável (build portátil) ou, instalado pelo instalador, na pasta de
/// dados do usuário (`%APPDATA%\Neoveg`). Ver [`choose_base_dir`].
pub fn base_dir() -> PathBuf {
    BASE_DIR
        .get_or_init(|| {
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()));
            let installed = exe_dir
                .as_ref()
                .is_some_and(|d| d.join(INSTALL_MARKER).is_file());
            let writable = !installed && exe_dir.as_deref().is_some_and(dir_is_writable);
            let user_data = std::env::var_os("APPDATA")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from);
            let chosen = choose_base_dir(
                exe_dir,
                installed,
                writable,
                user_data,
                std::env::temp_dir(),
            );
            let _ = std::fs::create_dir_all(&chosen);
            chosen
        })
        .clone()
}

fn logs_dir() -> PathBuf {
    let d = base_dir().join("logs");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Remove dados pessoais óbvios (pasta do usuário) das mensagens de log (Regra da Casa 10).
pub fn redact(s: &str) -> String {
    let mut out = s.to_string();
    for var in ["USERPROFILE", "HOME"] {
        if let Ok(home) = std::env::var(var) {
            if home.len() > 3 {
                out = out.replace(&home, "~");
                let slash = home.replace('\\', "/");
                if slash != home {
                    out = out.replace(&slash, "~");
                }
            }
        }
    }
    out
}

fn timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Data (`YYYY-MM-DD`) e hora (`HH:MM:SS`) locais formatadas via `chrono` (Regra Rust 5).
pub fn local_date_time_strings() -> (String, String) {
    let now = chrono::Local::now();
    (
        now.format("%Y-%m-%d").to_string(),
        now.format("%H:%M:%S").to_string(),
    )
}

fn append(file: &str, line: &str) {
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs_dir().join(file))
    {
        let _ = writeln!(f, "{} {}", timestamp(), redact(line));
    }
}

pub fn log_info(msg: &str) {
    append("neoveg.log", &format!("[INFO] {msg}"));
}

pub fn log_warn(msg: &str) {
    append("neoveg.log", &format!("[AVISO] {msg}"));
}

pub fn log_error(msg: &str) {
    append("neoveg.log", &format!("[ERRO] {msg}"));
}

// ---------------------------------------------------------------------------
// Recuperação e hook de pânico
// ---------------------------------------------------------------------------

type RecoveryCallback = fn() -> Option<String>;

static RECOVERY_CB: Mutex<Option<RecoveryCallback>> = Mutex::new(None);

/// Registra uma função chamada sem pânicos durante o tratamento de crash (§0.7).
pub fn set_recovery_callback(cb: RecoveryCallback) {
    *lock(&RECOVERY_CB) = Some(cb);
}

/// Instala o hook de pânico: registra stack trace em `logs/crash.log` e
/// executa o callback de recuperação (ex.: salvar `autosave/recuperacao.neoveg`).
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("<sem nome>").to_string();
        let msg = panic_message(info.payload());
        let loc = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        // liga o pânico à sessão da interface que o sofreu (relatório na
        // próxima abertura); não faz nada fora do modo de interface
        crate::session_marker::record_panic(&name, &loc, &msg);
        let bt = std::backtrace::Backtrace::force_capture();
        append(
            "crash.log",
            &format!("pânico na thread '{name}' em {loc}: {msg}\n{bt}\n"),
        );
        if let Ok(guard) = RECOVERY_CB.try_lock() {
            if let Some(cb) = *guard {
                if let Some(note) = cb() {
                    append("crash.log", &note);
                }
            }
        }
        eprintln!("Neoveg: erro interno na thread '{name}': {msg} (detalhes em logs/crash.log)");
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn clamp_cruzado_nao_entra_em_panico() {
        assert_eq!(clampf(5.0, 1.0, 0.0), 1.0);
        assert_eq!(clampf(0.5, 0.0, 1.0), 0.5);
    }

    #[test]
    fn guard_isola_panico() {
        let r: Result<(), String> = guard("teste", || panic!("explodiu"));
        assert!(r.unwrap_err().contains("explodiu"));
    }

    #[test]
    fn lock_envenenado_continua_utilizavel() {
        let m = Arc::new(Mutex::new(1));
        let m2 = m.clone();
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("envenena");
        })
        .join();
        assert!(m.is_poisoned());
        *lock(&m) += 1;
        assert_eq!(*lock(&m), 2);
    }

    /// Instalado (marcador ao lado do executável): os dados vão para
    /// `%APPDATA%\Neoveg`, mesmo que a pasta do programa seja gravável;
    /// portátil continua ao lado do executável.
    #[test]
    fn pasta_base_instalado_vai_para_dados_do_usuario() {
        let abs = |w: &str, u: &str| PathBuf::from(if cfg!(windows) { w } else { u });
        let exe = abs(r"C:\Neoveg", "/opt/neoveg");
        let appdata = abs(r"C:\Users\u\AppData\Roaming", "/home/u/.config");
        let temp = abs(r"C:\Temp", "/tmp");
        let pick = |exe_dir, installed, writable, user| {
            choose_base_dir(exe_dir, installed, writable, user, temp.clone())
        };
        assert_eq!(
            pick(Some(exe.clone()), true, true, Some(appdata.clone())),
            appdata.join("Neoveg")
        );
        assert_eq!(
            pick(Some(exe.clone()), false, true, Some(appdata.clone())),
            exe
        );
        // portátil numa pasta só de leitura: dados do usuário, não a temporária
        assert_eq!(
            pick(Some(exe.clone()), false, false, Some(appdata.clone())),
            appdata.join("Neoveg")
        );
        // sem pasta de dados do usuário (ou relativa): temporária
        assert_eq!(
            pick(Some(exe.clone()), true, false, None),
            temp.join("neoveg")
        );
        assert_eq!(
            pick(None, false, false, Some(PathBuf::from("relativa"))),
            temp.join("neoveg")
        );
    }
}
