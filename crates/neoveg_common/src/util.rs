//! Utilitários de robustez exigidos pelo AGENTS.md §0 (Nunca Travar):
//! locks tolerantes a envenenamento, `clamp` que não entra em pânico,
//! isolamento de pânicos em workers, log em `logs/neoveg.log`, hook de
//! pânico em `logs/crash.log` e recuperação automática do projeto.

use serde::{Deserialize, Serialize};
use std::any::Any;
use std::cell::Cell;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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

thread_local! {
    /// Profundidade de `catch_isolated` ativa na thread atual. Enquanto > 0, o
    /// gancho de pânico sabe que o pânico será contido e não abre a janela de
    /// encerramento inesperado.
    static ISOLATED_GUARD_DEPTH: Cell<usize> = const { Cell::new(0) };
}

struct IsolatedDepthGuard;

impl IsolatedDepthGuard {
    fn enter() -> Self {
        ISOLATED_GUARD_DEPTH.with(|d| d.set(d.get().saturating_add(1)));
        Self
    }
}

impl Drop for IsolatedDepthGuard {
    fn drop(&mut self) {
        ISOLATED_GUARD_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// Executa `f` sob `catch_unwind` e, se houver pânico, marca-o como isolado no
/// marcador da sessão para não ser apontado como queda fatal.
pub fn catch_isolated<T>(f: impl FnOnce() -> T) -> std::thread::Result<T> {
    let _depth = IsolatedDepthGuard::enter();
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

/// Nome da pasta de dados do usuário quando instalado (`%APPDATA%\Neoveg` no
/// Windows, `~/.local/share/Neoveg` no Linux).
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

/// Raiz das pastas de dados do usuário: `%APPDATA%` no Windows e, nos
/// outros sistemas, `$XDG_DATA_HOME` ou `~/.local/share` (XDG Base Directory).
/// Só caminhos absolutos; variáveis vazias ou relativas são ignoradas.
pub fn user_data_root() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
    if cfg!(windows) {
        user_data_root_from(var("APPDATA"), None, None)
    } else {
        user_data_root_from(None, var("XDG_DATA_HOME"), var("HOME"))
    }
}

/// Regra pura de [`user_data_root`]: `appdata` (Windows) ou `xdg_data_home`
/// ou `home/.local/share`.
pub fn user_data_root_from(
    appdata: Option<PathBuf>,
    xdg_data_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    let abs = |p: Option<PathBuf>| p.filter(|p| p.is_absolute());
    abs(appdata)
        .or_else(|| abs(xdg_data_home))
        .or_else(|| abs(home).map(|h| h.join(".local").join("share")))
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
            let user_data = user_data_root();
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

/// Hora local (`HH:MM:SS`) daqui a `secs` segundos, via `chrono` (Regra Rust
/// 5). Valores inválidos, negativos ou absurdos (mais de um ano) não têm hora.
pub fn local_time_after_secs(secs: f64) -> Option<String> {
    if !secs.is_finite() || !(0.0..=31_536_000.0).contains(&secs) {
        return None;
    }
    let d = chrono::Duration::milliseconds((secs * 1000.0).round() as i64);
    chrono::Local::now()
        .checked_add_signed(d)
        .map(|t| t.format("%H:%M:%S").to_string())
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
static GUI_ACTIVE: AtomicBool = AtomicBool::new(false);
static CRASH_DIALOG_SPAWNED: AtomicBool = AtomicBool::new(false);

/// Limite máximo para leitura do JSON do último erro (§0.4).
const MAX_CRASH_PAYLOAD_BYTES: u64 = 512 * 1024;
/// Limite de caracteres do stack trace incluído no payload da janela de erro.
const MAX_CRASH_BACKTRACE_CHARS: usize = 16_000;

/// Dados estruturados do último encerramento inesperado, gravados pelo hook de
/// pânico em `logs/ultimo_crash.json` para o subprocesso `--crash-dialog`.
/// Todos os campos já passam por [`redact`] antes da gravação (Regra da Casa 10).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrashDialogPayload {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub arch: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub backtrace: String,
    #[serde(default)]
    pub recovery_note: Option<String>,
    #[serde(default)]
    pub log_dir: String,
}

impl CrashDialogPayload {
    /// Lê um arquivo de payload de crash tratando-o como entrada hostil (§0.4).
    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        f.take(MAX_CRASH_PAYLOAD_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if (bytes.len() as u64) > MAX_CRASH_PAYLOAD_BYTES {
            return Err("arquivo de relatório excede o limite seguro".into());
        }
        serde_json::from_slice::<Self>(&bytes).map_err(|e| e.to_string())
    }

    /// Grava o payload em JSON (ignora erros de disco no caminho de pânico).
    pub fn save_to_file(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }

    /// Formata os detalhes técnicos do problema (estilo relatório de problema
    /// clássico: descrição do problema, ambiente, erro, notas opcionais e stack trace).
    pub fn format_problem_details(&self, user_notes: &str) -> String {
        let mut s = String::new();
        s.push_str("Descrição do Problema (Neoveg)\n");
        s.push_str("========================================\n");
        s.push_str("  Aplicativo:   Neoveg\n");
        if !self.version.is_empty() {
            s.push_str(&format!("  Versão:       {}\n", self.version));
        }
        let os = if self.os.is_empty() {
            std::env::consts::OS
        } else {
            &self.os
        };
        let arch = if self.arch.is_empty() {
            std::env::consts::ARCH
        } else {
            &self.arch
        };
        s.push_str(&format!("  Sistema:      {os} ({arch})\n"));
        if !self.timestamp.is_empty() {
            s.push_str(&format!("  Data/Hora:    {}\n", self.timestamp));
        }
        if !self.thread.is_empty() {
            s.push_str(&format!("  Thread:       {}\n", self.thread));
        }
        if !self.location.is_empty() {
            s.push_str(&format!("  Local:        {}\n", self.location));
        }
        if !self.message.is_empty() {
            s.push_str(&format!("  Erro:         {}\n", self.message));
        }
        if let Some(note) = &self.recovery_note {
            if !note.trim().is_empty() {
                s.push_str(&format!("  Recuperação:  {}\n", note.trim()));
            }
        }
        let notes = user_notes.trim();
        if !notes.is_empty() {
            s.push_str("\nDescrição do usuário:\n");
            s.push_str(&redact(notes));
            s.push('\n');
        }
        if !self.backtrace.trim().is_empty() {
            s.push_str("\nInformações Extras (Stack Trace):\n");
            s.push_str(&self.backtrace);
            if !self.backtrace.ends_with('\n') {
                s.push('\n');
            }
        }
        s
    }
}

/// Caminho padrão do payload do último encerramento inesperado (`logs/ultimo_crash.json`).
pub fn last_crash_payload_path() -> PathBuf {
    logs_dir().join("ultimo_crash.json")
}

/// Marca se o processo está atualmente executando a interface gráfica principal.
/// Apenas quedas não isoladas na interface abrem a janela imediata de erro.
pub fn set_gui_active(active: bool) {
    GUI_ACTIVE.store(active, Ordering::SeqCst);
}

/// Informa se a interface gráfica principal está marcada como ativa.
pub fn is_gui_active() -> bool {
    GUI_ACTIVE.load(Ordering::SeqCst)
}

fn should_spawn_crash_dialog(isolated: bool, gui_active: bool, disabled_env: bool) -> bool {
    !isolated && gui_active && !disabled_env
}

/// Registra uma função chamada sem pânicos durante o tratamento de crash (§0.7).
pub fn set_recovery_callback(cb: RecoveryCallback) {
    *lock(&RECOVERY_CB) = Some(cb);
}

/// Instala o hook de pânico: registra stack trace em `logs/crash.log`,
/// executa o callback de recuperação (ex.: salvar `autosave/recuperacao.neoveg`)
/// e, para falhas não isoladas da interface gráfica, abre o diálogo de erro
/// (`--crash-dialog`) em processo separado.
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
        let bt_text = bt.to_string();
        append(
            "crash.log",
            &format!("pânico na thread '{name}' em {loc}: {msg}\n{bt_text}\n"),
        );
        let mut recovery_note: Option<String> = None;
        if let Ok(guard) = RECOVERY_CB.try_lock() {
            if let Some(cb) = *guard {
                if let Some(note) = cb() {
                    append("crash.log", &note);
                    recovery_note = Some(redact(&note));
                }
            }
        }
        let isolated = ISOLATED_GUARD_DEPTH.with(|d| d.get() > 0);
        let disabled_env = std::env::var_os("NEOVEG_NO_CRASH_DIALOG").is_some();
        if !cfg!(test)
            && should_spawn_crash_dialog(isolated, is_gui_active(), disabled_env)
            && !CRASH_DIALOG_SPAWNED.swap(true, Ordering::SeqCst)
        {
            let bt_clean: String = redact(&bt_text)
                .chars()
                .take(MAX_CRASH_BACKTRACE_CHARS)
                .collect();
            let payload = CrashDialogPayload {
                version: env!("CARGO_PKG_VERSION").to_string(),
                os: std::env::consts::OS.to_string(),
                arch: std::env::consts::ARCH.to_string(),
                timestamp: timestamp(),
                thread: redact(&name),
                location: redact(&loc),
                message: redact(&msg),
                backtrace: bt_clean,
                recovery_note,
                log_dir: redact(&logs_dir().display().to_string()),
            };
            let payload_path = last_crash_payload_path();
            if payload.save_to_file(&payload_path).is_ok() {
                if let Ok(exe) = std::env::current_exe() {
                    let _ = std::process::Command::new(exe)
                        .arg("--crash-dialog")
                        .arg(&payload_path)
                        .spawn();
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

    #[test]
    #[cfg(not(windows))]
    fn raiz_dos_dados_do_usuario_segue_appdata_ou_xdg() {
        let p = PathBuf::from;
        assert_eq!(
            user_data_root_from(None, Some(p("/x/dados")), Some(p("/home/u"))),
            Some(p("/x/dados"))
        );
        assert_eq!(
            user_data_root_from(None, None, Some(p("/home/u"))),
            Some(p("/home/u/.local/share"))
        );
        // XDG_DATA_HOME relativo é ignorado (especificação XDG)
        assert_eq!(
            user_data_root_from(None, Some(p("rel")), Some(p("/home/u"))),
            Some(p("/home/u/.local/share"))
        );
        assert_eq!(user_data_root_from(None, Some(p("")), Some(p(""))), None);
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

    #[test]
    fn isolated_guard_depth_volta_a_zero_apos_panico() {
        assert_eq!(ISOLATED_GUARD_DEPTH.with(|d| d.get()), 0);
        let seen_inside = std::cell::Cell::new(0usize);
        let res = catch_isolated(|| {
            seen_inside.set(ISOLATED_GUARD_DEPTH.with(|d| d.get()));
            panic!("falha isolada de teste");
        });
        assert!(res.is_err());
        assert_eq!(seen_inside.get(), 1);
        assert_eq!(ISOLATED_GUARD_DEPTH.with(|d| d.get()), 0);
        assert!(should_spawn_crash_dialog(false, true, false));
        assert!(!should_spawn_crash_dialog(true, true, false));
        assert!(!should_spawn_crash_dialog(false, false, false));
        assert!(!should_spawn_crash_dialog(false, true, true));
    }

    #[test]
    fn crash_dialog_payload_roundtrip_e_entrada_hostil() {
        let dir = std::env::temp_dir().join(format!("neoveg_crash_payload_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("ultimo_crash.json");
        let p = CrashDialogPayload {
            version: "0.3.0".into(),
            os: "windows".into(),
            arch: "x86_64".into(),
            timestamp: "2026-11-23 15:00:00".into(),
            thread: "main".into(),
            location: "crates/neoveg_ui/src/kf_window.rs:315".into(),
            message: "falha simulada".into(),
            backtrace: "0: neoveg::main".into(),
            recovery_note: Some("projeto salvo".into()),
            log_dir: "~/logs".into(),
        };
        p.save_to_file(&file).unwrap();
        let back = CrashDialogPayload::load_from_file(&file).unwrap();
        assert_eq!(back, p);
        let text = back.format_problem_details("Movendo a trilha de vídeo");
        assert!(text.contains("Versão:       0.3.0"));
        assert!(text.contains("falha simulada"));
        assert!(text.contains("Movendo a trilha de vídeo"));
        assert!(text.contains("0: neoveg::main"));

        // JSON corrompido ou arquivo inexistente nunca entra em pânico
        std::fs::write(&file, b"{corrompido").unwrap();
        assert!(CrashDialogPayload::load_from_file(&file).is_err());
        assert!(CrashDialogPayload::load_from_file(&dir.join("inexistente.json")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
