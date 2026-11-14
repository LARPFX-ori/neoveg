//! Fronteira FFI isolada (`AGENTS.md` §0.3) para hospedar plug-ins externos.
//!
//! Etapa atual: sondar binários **OpenFX (OFX)** e listar os plug-ins que eles
//! declaram, a partir do cabeçalho público `ofxCore.h` (BSD-3-Clause):
//! `int OfxGetNumberOfPlugins(void)` e `OfxPlugin *OfxGetPlugin(int nth)`.
//!
//! Carregar uma biblioteca executa código de terceiros. Por isso
//! [`probe_ofx_in_process`] só roda dentro do subprocesso
//! `neoveg --plugin-host probe-ofx <binário>`, e a interface usa
//! [`probe_ofx_isolated`], que dispara esse subprocesso com tempo limite: se o
//! plug-in travar ou derrubar o processo, a sessão do editor segue intacta
//! (§0.6).
#![allow(unsafe_code)]

#[cfg(all(windows, target_arch = "x86_64"))]
mod ofx;

use crate::model::{OfxFxRef, OfxParamValue};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_uint, c_void};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// Descritor de um parâmetro de plug-in OFX (para a interface).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OfxParamInfo {
    pub name: String,
    pub label: String,
    /// Tipo sem o prefixo `OfxParamType` (ex.: `Double`, `RGBA`, `Choice`).
    pub kind: String,
    pub default: Vec<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub choices: Vec<String>,
}

/// Maior cabeçalho JSON trocado com o host.
const MAX_JSON_BYTES: u32 = 4 * 1024 * 1024;
/// Maior quadro trocado com o host (8K RGBA float).
const MAX_FRAME_BYTES: u64 = 7680 * 4320 * 16;
/// Tempo máximo para abrir um plug-in ou renderizar um quadro.
const WORKER_TIMEOUT: Duration = Duration::from_secs(60);

/// Máximo de plug-ins aceitos de um binário (entrada hostil, §0.4).
const MAX_OFX_PLUGINS: c_int = 4096;
/// Comprimento máximo lido de uma string C do plug-in.
const MAX_CSTR: usize = 1024;
/// Tamanho máximo da resposta do subprocesso.
const MAX_REPLY_BYTES: u64 = 4 * 1024 * 1024;

/// Plug-in declarado por um binário OFX.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfxPluginInfo {
    pub identifier: String,
    pub api: String,
    pub api_version: i32,
    pub version_major: u32,
    pub version_minor: u32,
}

/// Layout de `OfxPlugin` em `ofxCore.h`. `set_host` e `main_entry` ainda não
/// são chamados, mas precisam existir para o layout bater com o C.
#[repr(C)]
#[allow(dead_code)]
struct OfxPluginRaw {
    plugin_api: *const c_char,
    api_version: c_int,
    plugin_identifier: *const c_char,
    plugin_version_major: c_uint,
    plugin_version_minor: c_uint,
    set_host: Option<unsafe extern "C" fn(*mut c_void)>,
    main_entry: Option<
        unsafe extern "C" fn(*const c_char, *const c_void, *mut c_void, *mut c_void) -> c_int,
    >,
}

type GetCountFn = unsafe extern "C" fn() -> c_int;
type GetPluginFn = unsafe extern "C" fn(c_int) -> *const OfxPluginRaw;

/// Lê uma string C limitada a [`MAX_CSTR`] bytes.
///
/// # Safety
/// `p` deve ser nulo ou apontar para memória legível até o terminador NUL ou
/// por [`MAX_CSTR`] bytes, o que vier primeiro.
unsafe fn bounded_cstr(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut bytes = Vec::new();
    for i in 0..MAX_CSTR {
        // SAFETY: pré-condição da função; `i` < MAX_CSTR e a leitura para no NUL.
        let b = unsafe { p.add(i).read() } as u8;
        if b == 0 {
            break;
        }
        bytes.push(b);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Carrega o binário OFX **no processo atual** e lista os plug-ins dele.
/// Só deve ser chamada pelo subprocesso `--plugin-host`.
pub fn probe_ofx_in_process(binary: &Path) -> Result<Vec<OfxPluginInfo>, String> {
    // SAFETY: carregar a biblioteca executa a inicialização dela; o risco é
    // aceito porque esta função só roda no subprocesso isolado.
    let lib = unsafe { libloading::Library::new(binary) }
        .map_err(|e| format!("não foi possível carregar {}: {e}", binary.display()))?;
    // SAFETY: assinaturas definidas em `ofxCore.h` para os dois símbolos.
    let count_fn: libloading::Symbol<GetCountFn> =
        unsafe { lib.get(b"OfxGetNumberOfPlugins\0") }
            .map_err(|_| "binário sem OfxGetNumberOfPlugins (não é um plug-in OFX)".to_string())?;
    // SAFETY: idem.
    let get_fn: libloading::Symbol<GetPluginFn> = unsafe { lib.get(b"OfxGetPlugin\0") }
        .map_err(|_| "binário sem OfxGetPlugin (não é um plug-in OFX)".to_string())?;
    // SAFETY: função sem argumentos exportada pelo plug-in.
    let n = unsafe { count_fn() };
    if !(0..=MAX_OFX_PLUGINS).contains(&n) {
        return Err(format!("contagem de plug-ins OFX inválida: {n}"));
    }
    let mut out = Vec::new();
    for i in 0..n {
        // SAFETY: `i` está em `0..n`, como pede o contrato do OFX.
        let p = unsafe { get_fn(i) };
        if p.is_null() {
            continue;
        }
        // SAFETY: ponteiro não nulo devolvido pelo plug-in, válido enquanto a
        // biblioteca estiver carregada (ela só é liberada no fim do processo).
        let raw = unsafe { &*p };
        // SAFETY: strings NUL-terminadas do `OfxPlugin`, lidas com limite.
        let (identifier, api) = unsafe {
            (
                bounded_cstr(raw.plugin_identifier),
                bounded_cstr(raw.plugin_api),
            )
        };
        out.push(OfxPluginInfo {
            identifier,
            api,
            api_version: raw.api_version,
            version_major: raw.plugin_version_major,
            version_minor: raw.plugin_version_minor,
        });
    }
    // Alguns plug-ins não suportam descarregar; o subprocesso termina logo em seguida.
    std::mem::forget(lib);
    Ok(out)
}

/// Sonda o binário OFX num subprocesso `host_exe --plugin-host probe-ofx`,
/// com tempo limite. Um plug-in que trava ou derruba o processo vira erro.
pub fn probe_ofx_isolated(
    host_exe: &Path,
    binary: &Path,
    timeout: Duration,
) -> Result<Vec<OfxPluginInfo>, String> {
    let mut child = Command::new(host_exe)
        .args(["--plugin-host", "probe-ofx"])
        .arg(binary)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("não foi possível iniciar o host de plug-ins: {e}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        if let Some(s) = stdout {
            let _ = s.take(MAX_REPLY_BYTES).read_to_end(&mut out);
        }
        out
    });
    let err_reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        if let Some(s) = stderr {
            let _ = s.take(64 * 1024).read_to_end(&mut out);
        }
        out
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(25)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "plug-in não respondeu em {} s e o host foi encerrado",
                    timeout.as_secs()
                ));
            }
            Err(e) => return Err(format!("falha ao aguardar o host de plug-ins: {e}")),
        }
    };
    let out = reader.join().unwrap_or_default();
    let err = err_reader.join().unwrap_or_default();
    if !status.success() {
        let msg = String::from_utf8_lossy(&err).trim().to_string();
        return Err(if msg.is_empty() {
            format!("host de plug-ins terminou com falha ({status})")
        } else {
            msg
        });
    }
    serde_json::from_slice(&out).map_err(|e| format!("resposta inválida do host de plug-ins: {e}"))
}

// ---------------------------------------------------------------------------
// Protocolo entre editor e host: quadros `u32 json` + `u64 bytes`
// ---------------------------------------------------------------------------

fn write_msg(w: &mut impl Write, json: &serde_json::Value, data: &[u8]) -> std::io::Result<()> {
    let j = serde_json::to_vec(json).map_err(std::io::Error::other)?;
    let jl = u32::try_from(j.len()).map_err(std::io::Error::other)?;
    w.write_all(&jl.to_le_bytes())?;
    w.write_all(&j)?;
    w.write_all(&(data.len() as u64).to_le_bytes())?;
    w.write_all(data)?;
    w.flush()
}

fn read_msg(r: &mut impl Read) -> std::io::Result<(serde_json::Value, Vec<u8>)> {
    let mut b4 = [0u8; 4];
    r.read_exact(&mut b4)?;
    let jl = u32::from_le_bytes(b4);
    if jl > MAX_JSON_BYTES {
        return Err(std::io::Error::other("cabeçalho grande demais"));
    }
    let mut j = vec![0u8; jl as usize];
    r.read_exact(&mut j)?;
    let mut b8 = [0u8; 8];
    r.read_exact(&mut b8)?;
    let dl = u64::from_le_bytes(b8);
    if dl > MAX_FRAME_BYTES {
        return Err(std::io::Error::other("quadro grande demais"));
    }
    let mut d = vec![0u8; dl as usize];
    r.read_exact(&mut d)?;
    let v = serde_json::from_slice(&j).map_err(std::io::Error::other)?;
    Ok((v, d))
}

fn f32_to_bytes(px: &[f32]) -> Vec<u8> {
    px.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn bytes_to_f32(b: &[u8], out: &mut [f32]) -> bool {
    if b.len() != out.len().saturating_mul(4) {
        return false;
    }
    for (o, c) in out.iter_mut().zip(b.as_chunks::<4>().0) {
        let v = f32::from_le_bytes(*c);
        *o = if v.is_finite() { v } else { 0.0 };
    }
    true
}

/// Laço do subprocesso `neoveg --plugin-host serve-ofx <binário> <id>`:
/// abre o plug-in, responde com os parâmetros e renderiza quadros até o EOF.
pub fn serve_ofx(binary: &Path, plugin_id: &str) -> i32 {
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let mut input = stdin.lock();
        let mut output = stdout.lock();
        let mut fx = match ofx::HostedEffect::open(binary, plugin_id) {
            Ok(fx) => fx,
            Err(e) => {
                let _ = write_msg(
                    &mut output,
                    &serde_json::json!({"ok": false, "error": e}),
                    &[],
                );
                return 3;
            }
        };
        let params = fx.params();
        if write_msg(
            &mut output,
            &serde_json::json!({"ok": true, "params": params}),
            &[],
        )
        .is_err()
        {
            return 4;
        }
        loop {
            let Ok((req, data)) = read_msg(&mut input) else {
                return 0; // editor fechou o canal
            };
            let w = req
                .get("w")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let h = req
                .get("h")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let time = req
                .get("time")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0);
            let fps = req
                .get("fps")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(30.0);
            let values: Vec<OfxParamValue> = req
                .get("values")
                .cloned()
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();
            let n = (w as usize).saturating_mul(h as usize).saturating_mul(4);
            let mut px = vec![0f32; n];
            let reply = if w == 0 || h == 0 || w > 7680 || h > 4320 || !bytes_to_f32(&data, &mut px)
            {
                Err("quadro inválido".to_string())
            } else {
                fx.render(w as u32, h as u32, time, fps, &values, &mut px)
            };
            let sent = match reply {
                Ok(()) => write_msg(
                    &mut output,
                    &serde_json::json!({"ok": true}),
                    &f32_to_bytes(&px),
                ),
                Err(e) => write_msg(
                    &mut output,
                    &serde_json::json!({"ok": false, "error": e}),
                    &[],
                ),
            };
            if sent.is_err() {
                return 4;
            }
        }
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        let _ = (binary, plugin_id);
        eprintln!("host OFX disponível só no Windows x86_64 por enquanto");
        5
    }
}

struct Worker {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<std::io::Result<(serde_json::Value, Vec<u8>)>>,
    params: Vec<OfxParamInfo>,
}

impl Worker {
    fn spawn(host_exe: &Path, binary: &str, plugin_id: &str) -> Result<Self, String> {
        let mut child = Command::new(host_exe)
            .args(["--plugin-host", "serve-ofx"])
            .arg(binary)
            .arg(plugin_id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("não foi possível iniciar o host de plug-ins: {e}"))?;
        let (Some(stdin), Some(mut stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            return Err("host de plug-ins sem canais".into());
        };
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new()
            .name("ofx-host-reader".into())
            .spawn(move || loop {
                let msg = read_msg(&mut stdout);
                let end = msg.is_err();
                if tx.send(msg).is_err() || end {
                    break;
                }
            });
        if spawned.is_err() {
            let _ = child.kill();
            return Err("não foi possível criar a thread do host".into());
        }
        let mut w = Worker {
            child,
            stdin,
            rx,
            params: vec![],
        };
        let (hello, _) = w.recv()?;
        if hello.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
            let e = hello
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("plug-in não abriu")
                .to_string();
            w.kill();
            return Err(e);
        }
        w.params = hello
            .get("params")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        Ok(w)
    }

    fn recv(&mut self) -> Result<(serde_json::Value, Vec<u8>), String> {
        match self.rx.recv_timeout(WORKER_TIMEOUT) {
            Ok(Ok(m)) => Ok(m),
            Ok(Err(e)) => {
                self.kill();
                Err(format!("host de plug-ins encerrou: {e}"))
            }
            Err(RecvTimeoutError::Timeout) => {
                self.kill();
                Err(format!(
                    "plug-in não respondeu em {} s e o host foi encerrado",
                    WORKER_TIMEOUT.as_secs()
                ))
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.kill();
                Err("host de plug-ins encerrou".into())
            }
        }
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn render(
        &mut self,
        w: u32,
        h: u32,
        time: f64,
        fps: f64,
        values: &[OfxParamValue],
        px: &mut [f32],
    ) -> Result<(), String> {
        let req = serde_json::json!({"w": w, "h": h, "time": time, "fps": fps, "values": values});
        write_msg(&mut self.stdin, &req, &f32_to_bytes(px))
            .map_err(|e| format!("falha ao enviar o quadro ao host: {e}"))?;
        let (reply, data) = self.recv()?;
        if reply.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(reply
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("falha no plug-in")
                .to_string());
        }
        if bytes_to_f32(&data, px) {
            Ok(())
        } else {
            Err("quadro devolvido com tamanho errado".into())
        }
    }
}

type Pool = Mutex<HashMap<(PathBuf, String, String), Worker>>;

fn pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Executável usado como host: `NEOVEG_PLUGIN_HOST` ou o próprio Neoveg.
pub fn default_host_exe() -> Option<PathBuf> {
    std::env::var_os("NEOVEG_PLUGIN_HOST")
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
}

fn with_worker<T>(
    host_exe: &Path,
    r: &OfxFxRef,
    f: impl FnOnce(&mut Worker) -> Result<T, String>,
) -> Result<T, String> {
    let k = (
        host_exe.to_path_buf(),
        r.binary.clone(),
        r.plugin_id.clone(),
    );
    let mut pool = pool().lock().unwrap_or_else(PoisonError::into_inner);
    if !pool.contains_key(&k) {
        let w = Worker::spawn(host_exe, &r.binary, &r.plugin_id)?;
        pool.insert(k.clone(), w);
    }
    let Some(w) = pool.get_mut(&k) else {
        return Err("host de plug-ins indisponível".into());
    };
    let res = f(w);
    if res.is_err() && w.child.try_wait().map(|s| s.is_some()).unwrap_or(true) {
        // host morto: sai do pool para ser recriado na próxima chamada
        pool.remove(&k);
    }
    res
}

/// Parâmetros declarados pelo plug-in (abre o host se preciso).
pub fn ofx_params(host_exe: &Path, r: &OfxFxRef) -> Result<Vec<OfxParamInfo>, String> {
    with_worker(host_exe, r, |w| Ok(w.params.clone()))
}

/// Aplica o efeito OFX a um quadro RGBA premultiplicado (de cima para baixo),
/// no host isolado. Em erro, o quadro fica como estava.
pub fn ofx_render(
    host_exe: &Path,
    r: &OfxFxRef,
    w: u32,
    h: u32,
    time: f64,
    fps: f64,
    px: &mut [f32],
) -> Result<(), String> {
    with_worker(host_exe, r, |wk| wk.render(w, h, time, fps, &r.params, px))
}

/// Encerra todos os hosts de plug-ins abertos.
pub fn shutdown_workers() {
    let mut pool = pool().lock().unwrap_or_else(PoisonError::into_inner);
    for (_, mut w) in pool.drain() {
        w.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_framing_round_trips_and_rejects_oversize() {
        let mut buf = vec![];
        write_msg(&mut buf, &serde_json::json!({"a": 1}), &[1, 2, 3]).expect("grava");
        let (v, d) = read_msg(&mut buf.as_slice()).expect("lê");
        assert_eq!(v["a"], 1);
        assert_eq!(d, vec![1, 2, 3]);
        let mut big = (MAX_JSON_BYTES + 1).to_le_bytes().to_vec();
        big.extend([0u8; 16]);
        assert!(read_msg(&mut big.as_slice()).is_err());
        assert!(read_msg(&mut [1u8, 0].as_slice()).is_err());
    }

    #[test]
    fn float_conversion_checks_length_and_nan() {
        let b = f32_to_bytes(&[1.0, f32::NAN]);
        let mut out = [0f32; 2];
        assert!(bytes_to_f32(&b, &mut out));
        assert_eq!(out, [1.0, 0.0]);
        assert!(!bytes_to_f32(&b[..4], &mut out));
    }

    #[test]
    fn in_process_rejects_missing_and_non_ofx_libraries() {
        assert!(probe_ofx_in_process(Path::new("nao-existe.ofx")).is_err());
        #[cfg(windows)]
        {
            // biblioteca do sistema, carregada em todo processo: não exporta OFX
            let e = probe_ofx_in_process(Path::new("kernel32.dll")).expect_err("não é OFX");
            assert!(e.contains("OFX"), "{e}");
        }
    }

    #[test]
    fn isolated_reports_missing_host() {
        let r = probe_ofx_isolated(
            Path::new("host-que-nao-existe.exe"),
            Path::new("x.ofx"),
            Duration::from_secs(1),
        );
        assert!(r.is_err());
    }

    #[test]
    fn bounded_cstr_stops_at_nul_and_handles_null() {
        let s = b"abc\0def";
        // SAFETY: buffer local NUL-terminado.
        assert_eq!(unsafe { bounded_cstr(s.as_ptr().cast()) }, "abc");
        // SAFETY: ponteiro nulo é aceito pela função.
        assert_eq!(unsafe { bounded_cstr(std::ptr::null()) }, "");
    }
}
