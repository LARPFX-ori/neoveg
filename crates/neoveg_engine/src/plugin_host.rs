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

use serde::{Deserialize, Serialize};
use std::ffi::{c_char, c_int, c_uint, c_void};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

#[cfg(test)]
mod tests {
    use super::*;

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
