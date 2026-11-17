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
#[cfg(windows)]
mod vst3;

use crate::model::{
    decode_hex_bytes, encode_hex_bytes, Id, OfxFxRef, OfxParamValue, Vst3FxRef, Vst3ParamValue,
    MAX_VST3_STATE_BYTES,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_uint, c_void};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
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

/// Classe de efeito de áudio declarada por um módulo VST3.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vst3ClassInfo {
    /// `TUID` da classe em 32 dígitos hexadecimais.
    pub class_id: String,
    pub name: String,
    /// Subcategorias (`Fx|Delay`, `Instrument|Synth`...), se o plug-in as declara.
    #[serde(default)]
    pub sub_categories: String,
    #[serde(default)]
    pub vendor: String,
    #[serde(default)]
    pub version: String,
}

impl Vst3ClassInfo {
    /// Verdadeiro para instrumentos (precisam de MIDI, que o host ainda não envia).
    pub fn is_instrument(&self) -> bool {
        self.sub_categories
            .split('|')
            .any(|c| c.eq_ignore_ascii_case("Instrument"))
    }
}

/// Descritor de um parâmetro VST3 (para a interface). Valores normalizados.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vst3ParamInfo {
    pub id: u32,
    pub title: String,
    #[serde(default)]
    pub units: String,
    /// 0 = contínuo; n > 0 = n + 1 valores discretos.
    #[serde(default)]
    pub step_count: i32,
    pub default: f64,
    /// Valor no momento em que o plug-in foi aberto.
    pub current: f64,
    #[serde(default)]
    pub bypass: bool,
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
    let out = wait_child(&mut child, timeout)?;
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
    /// Primeira mensagem do host (`{"ok": true, ...}`), com os parâmetros.
    hello: serde_json::Value,
    /// Latência declarada pelo processador em amostras (VST3).
    latency: u32,
}

impl Worker {
    fn spawn_ofx(host_exe: &Path, binary: &str, plugin_id: &str) -> Result<Self, String> {
        Self::spawn(host_exe, &["serve-ofx", binary, plugin_id])
    }

    /// Inicia `host_exe --plugin-host <args...>` e espera o `hello`.
    fn spawn(host_exe: &Path, args: &[&str]) -> Result<Self, String> {
        let mut child = Command::new(host_exe)
            .arg("--plugin-host")
            .args(args)
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
            .name("plugin-host-reader".into())
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
            hello: serde_json::Value::Null,
            latency: 0,
        };
        let (hello, _) = w.recv(WORKER_TIMEOUT)?;
        if hello.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
            let e = hello
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("plug-in não abriu")
                .to_string();
            w.kill();
            return Err(e);
        }
        w.latency = hello
            .get("latency")
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0);
        w.hello = hello;
        Ok(w)
    }

    /// Parâmetros anunciados no `hello` (OFX ou VST3, conforme o host).
    fn hello_params<T: serde::de::DeserializeOwned>(&self) -> Vec<T> {
        self.hello
            .get("params")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }

    fn recv(&mut self, timeout: Duration) -> Result<(serde_json::Value, Vec<u8>), String> {
        match self.rx.recv_timeout(timeout) {
            Ok(Ok(m)) => Ok(m),
            Ok(Err(e)) => {
                self.kill();
                Err(format!("host de plug-ins encerrou: {e}"))
            }
            Err(RecvTimeoutError::Timeout) => {
                self.kill();
                Err(format!(
                    "plug-in não respondeu em {} ms e o host foi encerrado",
                    timeout.as_millis()
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

    fn is_dead(&mut self) -> bool {
        self.child.try_wait().map(|s| s.is_some()).unwrap_or(true)
    }

    /// Envia um pedido e devolve a resposta, exigindo `"ok": true`.
    fn request(
        &mut self,
        req: &serde_json::Value,
        data: &[u8],
        timeout: Duration,
    ) -> Result<(serde_json::Value, Vec<u8>), String> {
        write_msg(&mut self.stdin, req, data)
            .map_err(|e| format!("falha ao enviar o pedido ao host: {e}"))?;
        let (reply, out) = self.recv(timeout)?;
        if reply.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(reply
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("falha no plug-in")
                .to_string());
        }
        Ok((reply, out))
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
        let (_, data) = self.request(&req, &f32_to_bytes(px), WORKER_TIMEOUT)?;
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
        let w = Worker::spawn_ofx(host_exe, &r.binary, &r.plugin_id)?;
        pool.insert(k.clone(), w);
    }
    let Some(w) = pool.get_mut(&k) else {
        return Err("host de plug-ins indisponível".into());
    };
    let res = f(w);
    if res.is_err() && w.is_dead() {
        // host morto: sai do pool para ser recriado na próxima chamada
        pool.remove(&k);
    }
    res
}

/// Parâmetros declarados pelo plug-in (abre o host se preciso).
pub fn ofx_params(host_exe: &Path, r: &OfxFxRef) -> Result<Vec<OfxParamInfo>, String> {
    with_worker(host_exe, r, |w| Ok(w.hello_params()))
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
    drop(pool);
    let drained: Vec<Vst3Slot> = lock_vst3_pool().drain().map(|(_, s)| s).collect();
    for s in drained {
        s.kill();
    }
}

// ---------------------------------------------------------------------------
// VST3
// ---------------------------------------------------------------------------

/// Tempo máximo de um bloco de áudio no host VST3. Acima disso o host é
/// encerrado e o efeito passa o sinal seco até a próxima tentativa.
const VST3_BLOCK_TIMEOUT: Duration = Duration::from_secs(2);
/// Espera antes de tentar reabrir um plug-in VST3 que falhou.
const VST3_RETRY_AFTER: Duration = Duration::from_secs(10);
/// Maior bloco (em quadros) enviado por pedido; igual ao do host.
const VST3_MAX_BLOCK: usize = 8192;
/// Dono reservado às consultas da interface (parâmetros, textos).
const VST3_UI_OWNER: u64 = 0;

/// Sonda o módulo VST3 **no processo atual**. Só no subprocesso `--plugin-host`.
pub fn probe_vst3_in_process(binary: &Path) -> Result<Vec<Vst3ClassInfo>, String> {
    #[cfg(windows)]
    {
        vst3::probe(binary)
    }
    #[cfg(not(windows))]
    {
        let _ = binary;
        Err("host VST3 disponível só no Windows por enquanto".into())
    }
}

/// Sonda o módulo VST3 num subprocesso `host_exe --plugin-host probe-vst3`.
pub fn probe_vst3_isolated(
    host_exe: &Path,
    binary: &Path,
    timeout: Duration,
) -> Result<Vec<Vst3ClassInfo>, String> {
    let mut child = Command::new(host_exe)
        .args(["--plugin-host", "probe-vst3"])
        .arg(binary)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("não foi possível iniciar o host de plug-ins: {e}"))?;
    let out = wait_child(&mut child, timeout)?;
    serde_json::from_slice(&out).map_err(|e| format!("resposta inválida do host de plug-ins: {e}"))
}

/// Espera o subprocesso com tempo limite e devolve o `stdout` (limitado).
fn wait_child(child: &mut Child, timeout: Duration) -> Result<Vec<u8>, String> {
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
    Ok(out)
}

/// Laço do subprocesso `neoveg --plugin-host serve-vst3 <binário> <classe> <taxa>`:
/// abre o efeito, responde com os parâmetros e processa blocos até o EOF.
///
/// Pedidos: `{"cmd":"process","values":[...]}` + amostras `f32` estéreo
/// intercaladas; `{"cmd":"reset"}`; `{"cmd":"text","id":n,"value":v}`.
pub fn serve_vst3(binary: &Path, class_id: &str, sample_rate: &str) -> i32 {
    #[cfg(windows)]
    {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let mut input = stdin.lock();
        let mut output = stdout.lock();
        let opened = sample_rate
            .parse::<u32>()
            .map_err(|_| format!("taxa de amostragem inválida: {sample_rate}"))
            .and_then(|sr| vst3::HostedVst3::open(binary, class_id, sr));
        let mut fx = match opened {
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
        let hello = serde_json::json!({
            "ok": true,
            "params": fx.params(),
            "latency": fx.latency(),
        });
        if write_msg(&mut output, &hello, &[]).is_err() {
            return 4;
        }
        let mut buf: Vec<f32> = Vec::new();
        loop {
            let Ok((req, data)) = read_msg(&mut input) else {
                return 0; // editor fechou o canal
            };
            let cmd = req
                .get("cmd")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let sent = match cmd {
                "process" => {
                    let values: Vec<Vst3ParamValue> = req
                        .get("values")
                        .cloned()
                        .and_then(|v| serde_json::from_value(v).ok())
                        .unwrap_or_default();
                    let n = data.len() / 4;
                    buf.resize(n, 0.0);
                    let res =
                        if n % 2 != 0 || n > VST3_MAX_BLOCK * 2 || !bytes_to_f32(&data, &mut buf) {
                            Err("bloco de áudio inválido".to_string())
                        } else {
                            fx.process(&mut buf, &values)
                        };
                    match res {
                        Ok(()) => write_msg(
                            &mut output,
                            &serde_json::json!({"ok": true}),
                            &f32_to_bytes(&buf),
                        ),
                        Err(e) => write_msg(
                            &mut output,
                            &serde_json::json!({"ok": false, "error": e}),
                            &[],
                        ),
                    }
                }
                "reset" => {
                    let reply = match fx.reset() {
                        Ok(()) => serde_json::json!({"ok": true, "latency": fx.latency()}),
                        Err(e) => serde_json::json!({"ok": false, "error": e}),
                    };
                    write_msg(&mut output, &reply, &[])
                }
                "get_state" => match fx.state_bytes() {
                    Ok(bytes) => {
                        let capped = &bytes[..bytes.len().min(MAX_VST3_STATE_BYTES)];
                        write_msg(&mut output, &serde_json::json!({"ok": true}), capped)
                    }
                    Err(e) => write_msg(
                        &mut output,
                        &serde_json::json!({"ok": false, "error": e}),
                        &[],
                    ),
                },
                "set_state" => {
                    let res = if data.len() > MAX_VST3_STATE_BYTES {
                        Err("estado VST3 excede o limite".to_string())
                    } else {
                        fx.set_state_bytes(&data)
                    };
                    let reply = match res {
                        Ok(()) => serde_json::json!({
                            "ok": true,
                            "params": fx.params(),
                            "latency": fx.latency(),
                        }),
                        Err(e) => serde_json::json!({"ok": false, "error": e}),
                    };
                    write_msg(&mut output, &reply, &[])
                }
                "text" => {
                    let id = req
                        .get("id")
                        .and_then(serde_json::Value::as_u64)
                        .and_then(|v| u32::try_from(v).ok());
                    let value = req
                        .get("value")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(0.0);
                    let reply = match id.and_then(|id| fx.param_text(id, value)) {
                        Some(t) => serde_json::json!({"ok": true, "text": t}),
                        None => serde_json::json!({"ok": false, "error": "sem texto"}),
                    };
                    write_msg(&mut output, &reply, &[])
                }
                other => write_msg(
                    &mut output,
                    &serde_json::json!({"ok": false, "error": format!("pedido desconhecido: {other}")}),
                    &[],
                ),
            };
            if sent.is_err() {
                return 4;
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (binary, class_id, sample_rate);
        eprintln!("host VST3 disponível só no Windows por enquanto");
        5
    }
}

fn hash_vst3_state(state: Option<&str>) -> u64 {
    let Some(s) = state else {
        return 0;
    };
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for &b in s.as_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

impl Worker {
    fn spawn_vst3(host_exe: &Path, r: &Vst3FxRef, sr: u32) -> Result<Self, String> {
        let sr_str = sr.to_string();
        let mut w = Self::spawn(host_exe, &["serve-vst3", &r.binary, &r.class_id, &sr_str])?;
        if let Some(hex) = r.state.as_deref() {
            let Some(bytes) = decode_hex_bytes(hex) else {
                w.kill();
                return Err("estado salvo VST3 inválido".into());
            };
            if let Ok((reply, _)) = w.request(
                &serde_json::json!({"cmd": "set_state"}),
                &bytes,
                WORKER_TIMEOUT,
            ) {
                if let Some(p) = reply.get("params").cloned() {
                    if let Some(obj) = w.hello.as_object_mut() {
                        obj.insert("params".into(), p);
                    }
                }
                if let Some(l) = reply
                    .get("latency")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|v| u32::try_from(v).ok())
                {
                    w.latency = l;
                }
            }
        }
        Ok(w)
    }

    fn vst3_process(&mut self, buf: &mut [f32], values: &[Vst3ParamValue]) -> Result<(), String> {
        let req = serde_json::json!({"cmd": "process", "values": values});
        let (_, data) = self.request(&req, &f32_to_bytes(buf), VST3_BLOCK_TIMEOUT)?;
        if bytes_to_f32(&data, buf) {
            Ok(())
        } else {
            Err("bloco devolvido com tamanho errado".into())
        }
    }
}

/// Chave de uma instância VST3: cada mixer (`owner`) tem a sua instância de
/// cada efeito, porque o plug-in guarda estado (caudas, envelopes).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Vst3Key {
    owner: u64,
    fx: Id,
    host: PathBuf,
    binary: String,
    class_id: String,
    sr: u32,
    state_hash: u64,
}

enum Vst3Slot {
    Loading,
    Ready(Arc<Mutex<Worker>>),
    Failed(Instant, String),
}

impl Vst3Slot {
    fn kill(self) {
        if let Vst3Slot::Ready(w) = self {
            w.lock().unwrap_or_else(PoisonError::into_inner).kill();
        }
    }
}

type Vst3Pool = Mutex<HashMap<Vst3Key, Vst3Slot>>;

fn lock_vst3_pool() -> std::sync::MutexGuard<'static, HashMap<Vst3Key, Vst3Slot>> {
    static POOL: OnceLock<Vst3Pool> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Novo identificador de dono de instâncias VST3 (um por mixer).
pub fn new_vst3_owner() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Abre o host da chave e grava o resultado no pool. Se a chave saiu do pool
/// enquanto abria (dono liberado), o host recém-aberto é encerrado.
fn open_vst3_slot(key: &Vst3Key, r: &Vst3FxRef) {
    let res = Worker::spawn_vst3(&key.host, r, key.sr);
    let mut pool = lock_vst3_pool();
    match (pool.get(key), res) {
        (Some(Vst3Slot::Loading), Ok(w)) => {
            pool.insert(key.clone(), Vst3Slot::Ready(Arc::new(Mutex::new(w))));
        }
        (Some(Vst3Slot::Loading), Err(e)) => {
            pool.insert(key.clone(), Vst3Slot::Failed(Instant::now(), e));
        }
        (_, Ok(mut w)) => w.kill(),
        (_, Err(_)) => {}
    }
}

/// Resultado de [`vst3_process`] quando o bloco não passou pelo plug-in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Vst3Skip {
    /// O host ainda está abrindo o plug-in (só sem `wait`).
    Loading,
    /// O plug-in falhou há pouco; nova tentativa depois de um intervalo.
    CoolingDown(String),
}

/// Processa `buf` (estéreo intercalado) pelo efeito VST3 `fx` do mixer
/// `owner`, no host isolado. Sem `wait`, a primeira chamada só dispara a
/// abertura numa thread e devolve [`Vst3Skip::Loading`]: a thread de áudio
/// nunca espera o plug-in carregar. Em qualquer falha `buf` fica intacto.
pub fn vst3_process(
    host_exe: &Path,
    owner: u64,
    fx: Id,
    r: &Vst3FxRef,
    sr: u32,
    buf: &mut [f32],
    wait: bool,
) -> Result<Result<(), Vst3Skip>, String> {
    let key = Vst3Key {
        owner,
        fx,
        host: host_exe.to_path_buf(),
        binary: r.binary.clone(),
        class_id: r.class_id.clone(),
        sr,
        state_hash: hash_vst3_state(r.state.as_deref()),
    };
    let worker = {
        let mut pool = lock_vst3_pool();
        match pool.get(&key) {
            Some(Vst3Slot::Ready(w)) => Some(w.clone()),
            Some(Vst3Slot::Loading) if !wait => return Ok(Err(Vst3Skip::Loading)),
            Some(Vst3Slot::Failed(t, e)) if t.elapsed() < VST3_RETRY_AFTER => {
                return Ok(Err(Vst3Skip::CoolingDown(e.clone())));
            }
            _ => {
                pool.insert(key.clone(), Vst3Slot::Loading);
                None
            }
        }
    };
    let worker = match worker {
        Some(w) => w,
        None if wait => {
            open_vst3_slot(&key, r);
            match lock_vst3_pool().get(&key) {
                Some(Vst3Slot::Ready(w)) => w.clone(),
                Some(Vst3Slot::Failed(_, e)) => return Err(e.clone()),
                _ => return Err("host VST3 indisponível".into()),
            }
        }
        None => {
            let (k2, r2) = (key.clone(), r.clone());
            let spawned = std::thread::Builder::new()
                .name("vst3-open".into())
                .spawn(move || open_vst3_slot(&k2, &r2));
            if spawned.is_err() {
                lock_vst3_pool().remove(&key);
                return Err("não foi possível criar a thread do host VST3".into());
            }
            return Ok(Err(Vst3Skip::Loading));
        }
    };
    let mut w = worker.lock().unwrap_or_else(PoisonError::into_inner);
    // trabalha numa cópia: se um bloco falhar no meio, `buf` fica intacto
    let mut out = buf.to_vec();
    let mut res = Ok(());
    for chunk in out.chunks_mut(VST3_MAX_BLOCK * 2) {
        res = w.vst3_process(chunk, &r.params);
        if res.is_err() {
            break;
        }
    }
    match res {
        Ok(()) => {
            buf.copy_from_slice(&out);
            Ok(Ok(()))
        }
        Err(e) => {
            if w.is_dead() {
                drop(w);
                lock_vst3_pool().insert(key, Vst3Slot::Failed(Instant::now(), e.clone()));
            }
            Err(e)
        }
    }
}

/// Zera o estado dos efeitos VST3 do mixer `owner` (após salto de posição).
pub fn vst3_reset_owner(owner: u64) {
    let workers: Vec<Arc<Mutex<Worker>>> = lock_vst3_pool()
        .iter()
        .filter(|(k, _)| k.owner == owner)
        .filter_map(|(_, s)| match s {
            Vst3Slot::Ready(w) => Some(w.clone()),
            _ => None,
        })
        .collect();
    for w in workers {
        let mut w = w.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = w.request(
            &serde_json::json!({"cmd": "reset"}),
            &[],
            VST3_BLOCK_TIMEOUT,
        );
    }
}

/// Encerra os hosts VST3 do mixer `owner` (o mixer foi descartado).
pub fn vst3_release_owner(owner: u64) {
    let slots: Vec<Vst3Slot> = {
        let mut pool = lock_vst3_pool();
        let keys: Vec<Vst3Key> = pool.keys().filter(|k| k.owner == owner).cloned().collect();
        keys.iter().filter_map(|k| pool.remove(k)).collect()
    };
    for s in slots {
        s.kill();
    }
}

/// Instância da interface para o plug-in de `r` (abre o host se preciso).
fn vst3_ui_worker(host_exe: &Path, r: &Vst3FxRef) -> Result<Arc<Mutex<Worker>>, String> {
    let ui_ref = Vst3FxRef {
        params: vec![],
        ..r.clone()
    };
    let mut silence = [0f32; 2];
    vst3_process(
        host_exe,
        VST3_UI_OWNER,
        0,
        &ui_ref,
        48_000,
        &mut silence,
        true,
    )?
    .map_err(|s| match s {
        Vst3Skip::Loading => "plug-in ainda abrindo".to_string(),
        Vst3Skip::CoolingDown(e) => e,
    })?;
    let key = Vst3Key {
        owner: VST3_UI_OWNER,
        fx: 0,
        host: host_exe.to_path_buf(),
        binary: r.binary.clone(),
        class_id: r.class_id.clone(),
        sr: 48_000,
        state_hash: hash_vst3_state(r.state.as_deref()),
    };
    match lock_vst3_pool().get(&key) {
        Some(Vst3Slot::Ready(w)) => Ok(w.clone()),
        _ => Err("host VST3 indisponível".into()),
    }
}

/// Parâmetros declarados pelo plug-in VST3 (para a interface).
pub fn vst3_params(host_exe: &Path, r: &Vst3FxRef) -> Result<Vec<Vst3ParamInfo>, String> {
    let w = vst3_ui_worker(host_exe, r)?;
    let w = w.lock().unwrap_or_else(PoisonError::into_inner);
    Ok(w.hello_params())
}

/// Latência declarada pelo efeito VST3 (`getLatencySamples`) em amostras (48 kHz UI probe).
pub fn vst3_latency(host_exe: &Path, r: &Vst3FxRef) -> Result<u32, String> {
    let w = vst3_ui_worker(host_exe, r)?;
    let w = w.lock().unwrap_or_else(PoisonError::into_inner);
    Ok(w.latency)
}

/// Consulta a latência em cache da instância do mixer `owner`; quando `wait_open`
/// é verdadeiro (modo offline), abre o host de forma síncrona se necessário.
pub fn vst3_cached_latency(
    host_exe: &Path,
    owner: u64,
    fx: Id,
    r: &Vst3FxRef,
    sr: u32,
    wait_open: bool,
) -> u32 {
    let key = Vst3Key {
        owner,
        fx,
        host: host_exe.to_path_buf(),
        binary: r.binary.clone(),
        class_id: r.class_id.clone(),
        sr,
        state_hash: hash_vst3_state(r.state.as_deref()),
    };
    if let Some(Vst3Slot::Ready(w)) = lock_vst3_pool().get(&key) {
        return w.lock().unwrap_or_else(PoisonError::into_inner).latency;
    }
    if !wait_open {
        return 0;
    }
    let mut silence = [0f32; 2];
    if vst3_process(host_exe, owner, fx, r, sr, &mut silence, true).is_ok() {
        let pool = lock_vst3_pool();
        if let Some(Vst3Slot::Ready(w)) = pool.get(&key) {
            let mut wg = w.lock().unwrap_or_else(PoisonError::into_inner);
            let lat = wg.latency;
            let _ = wg.request(
                &serde_json::json!({"cmd": "reset"}),
                &[],
                VST3_BLOCK_TIMEOUT,
            );
            return lat;
        }
    }
    0
}

/// Captura o estado completo do componente VST3 (`IComponent::getState`) em hexadecimal,
/// aplicando primeiro os parâmetros atuais de `r`.
pub fn vst3_get_state(host_exe: &Path, r: &Vst3FxRef) -> Result<String, String> {
    let w = vst3_ui_worker(host_exe, r)?;
    let mut w = w.lock().unwrap_or_else(PoisonError::into_inner);
    if !r.params.is_empty() {
        let mut silence = [0f32; 2];
        w.vst3_process(&mut silence, &r.params)?;
    }
    let (_, data) = w.request(
        &serde_json::json!({"cmd": "get_state"}),
        &[],
        VST3_BLOCK_TIMEOUT,
    )?;
    if data.is_empty() {
        return Err("plug-in devolveu estado vazio".into());
    }
    Ok(encode_hex_bytes(&data))
}

/// Texto do plug-in para um valor normalizado (ex.: "-6.0 dB").
pub fn vst3_param_text(
    host_exe: &Path,
    r: &Vst3FxRef,
    id: u32,
    value: f64,
) -> Result<String, String> {
    let w = vst3_ui_worker(host_exe, r)?;
    let mut w = w.lock().unwrap_or_else(PoisonError::into_inner);
    let (reply, _) = w.request(
        &serde_json::json!({"cmd": "text", "id": id, "value": value}),
        &[],
        VST3_BLOCK_TIMEOUT,
    )?;
    Ok(reply
        .get("text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string())
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
