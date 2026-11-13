//! Fronteira FFI isolada para hospedagem sob demanda do runtime .NET Core via
//! `netcorehost` (`hostfxr`) e descoberta do ambiente `.NET` instalado.
//!
//! `AGENTS.md` §0.3 / §3.6: nenhuma dependência em nuget em tempo de build, nenhum
//! carregamento no boot do Neoveg, e todo bloco `unsafe` restrito exclusivamente
//! a esta fronteira com justificativa `// SAFETY:` e retorno em `Result`.

use neoveg_common::error::NeovegError;
use netcorehost::hostfxr::{
    Hostfxr, HostfxrContext, InitializedForRuntimeConfig, ManagedFunctionWithDefaultSignature,
};
use netcorehost::pdcstring::PdCString;
use std::ffi::OsStr;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, OnceLock};

const MAX_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DotnetEnv {
    pub dotnet_root: PathBuf,
    pub hostfxr_path: PathBuf,
    pub dotnet_cli: Option<PathBuf>,
    pub tfm: String,
    pub framework_version: String,
    pub has_sdk: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SemVer {
    major: u32,
    minor: u32,
    patch: u32,
    is_stable: bool,
}

fn parse_semver(name: &str) -> Option<SemVer> {
    let (core, pre) = match name.split_once('-') {
        Some((c, _p)) => (c, true),
        None => (name, false),
    };
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?.parse::<u32>().ok()?;
    let patch = parts.next()?.parse::<u32>().ok()?;
    Some(SemVer {
        major,
        minor,
        patch,
        is_stable: !pre,
    })
}

fn hostfxr_lib_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "hostfxr.dll"
    } else if cfg!(target_os = "macos") {
        "libhostfxr.dylib"
    } else {
        "libhostfxr.so"
    }
}

fn dotnet_exe_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "dotnet.exe"
    } else {
        "dotnet"
    }
}

fn highest_versioned_subdir(dir: &Path) -> Option<(String, SemVer, PathBuf)> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(String, SemVer, PathBuf)> = None;
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let Some(name) = p.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        let Some(ver) = parse_semver(name) else {
            continue;
        };
        let better = match &best {
            None => true,
            Some((_, cur, _)) => ver > *cur,
        };
        if better {
            best = Some((name.to_string(), ver, p));
        }
    }
    best
}

fn find_highest_hostfxr(root: &Path) -> Option<(String, SemVer, PathBuf)> {
    let fxr_dir = root.join("host").join("fxr");
    let entries = std::fs::read_dir(fxr_dir).ok()?;
    let lib = hostfxr_lib_name();
    let mut best: Option<(String, SemVer, PathBuf)> = None;
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let Some(name) = p.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        let Some(ver) = parse_semver(name) else {
            continue;
        };
        let candidate = p.join(lib);
        if !candidate.is_file() {
            continue;
        }
        let better = match &best {
            None => true,
            Some((_, cur, _)) => ver > *cur,
        };
        if better {
            best = Some((name.to_string(), ver, candidate));
        }
    }
    best
}

fn candidate_dotnet_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for var in [
        "DOTNET_ROOT",
        "DOTNET_ROOT_X64",
        "DOTNET_ROOT_ARM64",
        "DOTNET_ROOT(x86)",
    ] {
        if let Some(val) = std::env::var_os(var) {
            if !val.is_empty() {
                roots.push(PathBuf::from(val));
            }
        }
    }
    if cfg!(target_os = "windows") {
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            roots.push(PathBuf::from(pf).join("dotnet"));
        }
        roots.push(PathBuf::from(r"C:\Program Files\dotnet"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            roots.push(PathBuf::from(local).join("Microsoft").join("dotnet"));
        }
        if let Some(home) = std::env::var_os("USERPROFILE") {
            roots.push(PathBuf::from(home).join(".dotnet"));
        }
    } else if cfg!(target_os = "macos") {
        roots.push(PathBuf::from("/usr/local/share/dotnet"));
        roots.push(PathBuf::from("/usr/local/share/dotnet/x64"));
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join(".dotnet"));
        }
    } else {
        for p in [
            "/usr/share/dotnet",
            "/usr/lib/dotnet",
            "/usr/lib64/dotnet",
            "/opt/dotnet",
        ] {
            roots.push(PathBuf::from(p));
        }
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join(".dotnet"));
        }
    }
    if let Some(path_var) = std::env::var_os("PATH") {
        let exe = dotnet_exe_name();
        for dir in std::env::split_paths(&path_var) {
            let cand = dir.join(exe);
            if cand.is_file() {
                if let Ok(canon) = std::fs::canonicalize(&cand) {
                    if let Some(parent) = canon.parent() {
                        roots.push(parent.to_path_buf());
                    }
                }
                roots.push(dir);
            }
        }
    }
    roots
}

/// Descobre uma instalação válida do runtime `.NET Core` (`hostfxr`) sem iniciar o host.
pub fn find_dotnet_env() -> Result<DotnetEnv, NeovegError> {
    let mut seen = std::collections::HashSet::new();
    for root in candidate_dotnet_roots() {
        if !seen.insert(root.clone()) || !root.is_dir() {
            continue;
        }
        let Some((fxr_name, fxr_ver, hostfxr_path)) = find_highest_hostfxr(&root) else {
            continue;
        };
        let shared_dir = root.join("shared").join("Microsoft.NETCore.App");
        let (framework_version, ver) = match highest_versioned_subdir(&shared_dir) {
            Some((name, sv, _)) => (name, sv),
            None => (fxr_name, fxr_ver),
        };
        if ver.major < 6 {
            continue;
        }
        let sdk_dir = root.join("sdk");
        let has_sdk = highest_versioned_subdir(&sdk_dir).is_some();
        let cli_path = root.join(dotnet_exe_name());
        let dotnet_cli = if cli_path.is_file() {
            Some(cli_path)
        } else {
            None
        };
        let tfm = format!("net{}.0", ver.major);
        return Ok(DotnetEnv {
            dotnet_root: root,
            hostfxr_path,
            dotnet_cli,
            tfm,
            framework_version,
            has_sdk,
        });
    }
    Err(NeovegError::Unsupported(
        "runtime .NET Core 6+ (hostfxr) não encontrado neste sistema; instale o .NET SDK/Runtime ou use '--command <id>'".into(),
    ))
}

struct HostJob {
    env: DotnetEnv,
    assembly_path: PathBuf,
    type_name: String,
    method_name: String,
    payload_utf8: Vec<u8>,
    reply: mpsc::SyncSender<Result<i32, NeovegError>>,
}

static HOST_THREAD: OnceLock<Result<mpsc::Sender<HostJob>, String>> = OnceLock::new();

fn os_to_pdc(s: &OsStr) -> Result<PdCString, NeovegError> {
    PdCString::from_os_str(s)
        .map_err(|e| NeovegError::InvalidInput(format!("caminho contém byte NUL inválido: {e}")))
}

fn str_to_pdc(s: &str) -> Result<PdCString, NeovegError> {
    os_to_pdc(OsStr::new(s))
}

fn ensure_host_runtime_config(env: &DotnetEnv) -> Result<PathBuf, NeovegError> {
    let dir = std::env::temp_dir().join(format!("neoveg_hostfxr_{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| {
        NeovegError::Io(format!(
            "não foi possível preparar diretório do host .NET: {e}"
        ))
    })?;
    let rc_path = dir.join("NeovegHost.runtimeconfig.json");
    let rc_json = serde_json::json!({
        "runtimeOptions": {
            "tfm": env.tfm,
            "rollForward": "LatestMajor",
            "framework": {
                "name": "Microsoft.NETCore.App",
                "version": env.framework_version
            }
        }
    });
    let text = serde_json::to_string_pretty(&rc_json)
        .map_err(|e| NeovegError::Json(format!("erro ao gerar runtimeconfig.json: {e}")))?;
    std::fs::write(&rc_path, text)
        .map_err(|e| NeovegError::Io(format!("erro ao gravar runtimeconfig.json: {e}")))?;
    Ok(rc_path)
}

#[allow(unsafe_code)]
fn call_component_entry(
    func: &ManagedFunctionWithDefaultSignature,
    payload: &[u8],
) -> Result<i32, NeovegError> {
    let len_i32 = i32::try_from(payload.len()).map_err(|_| {
        NeovegError::InvalidInput("payload excede o limite de 32 bits do entrypoint .NET".into())
    })?;
    let ptr = if payload.is_empty() {
        std::ptr::null()
    } else {
        payload.as_ptr().cast()
    };
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `func` é um ponteiro de função gerenciada carregado via
        // `Hostfxr` com assinatura padrão `ComponentEntryPoint(IntPtr args,
        // int sizeBytes) -> int` (`unsafe extern "system" fn(*const c_void, i32)
        // -> i32`); `ptr` aponta para `payload` válido e imutável durante toda
        // a chamada síncrona (ou nulo se `len_i32 == 0`).
        unsafe { func(ptr, len_i32) }
    }))
    .map_err(|_| {
        NeovegError::Internal("falha inesperada (unwind) ao executar assembly .NET".into())
    })
}

fn execute_on_host_thread(
    state: &mut Option<HostfxrContext<InitializedForRuntimeConfig>>,
    job: &HostJob,
) -> Result<i32, NeovegError> {
    if state.is_none() {
        let hostfxr = Hostfxr::load_from_path(&job.env.hostfxr_path).map_err(|e| {
            NeovegError::Unsupported(format!(
                "falha ao carregar hostfxr em '{}': {e}",
                job.env.hostfxr_path.display()
            ))
        })?;
        let rc_path = ensure_host_runtime_config(&job.env)?;
        let rc_pd = os_to_pdc(rc_path.as_os_str())?;
        let root_pd = os_to_pdc(job.env.dotnet_root.as_os_str())?;
        let ctx = hostfxr
            .initialize_for_runtime_config_with_dotnet_root(&rc_pd, &root_pd)
            .map_err(|e| {
                NeovegError::Internal(format!("falha ao inicializar runtime .NET (hostfxr): {e}"))
            })?;
        *state = Some(ctx);
    }
    let Some(ctx) = state.as_ref() else {
        return Err(NeovegError::Internal(
            "contexto hostfxr não inicializado".into(),
        ));
    };
    let asm_pd = os_to_pdc(job.assembly_path.as_os_str())?;
    let type_pd = str_to_pdc(&job.type_name)?;
    let method_pd = str_to_pdc(&job.method_name)?;
    let loader = ctx.get_delegate_loader_for_assembly(asm_pd).map_err(|e| {
        NeovegError::Internal(format!(
            "falha ao preparar delegate loader para '{}': {e}",
            job.assembly_path.display()
        ))
    })?;
    let func = loader
        .get_function_with_default_signature(&type_pd, &method_pd)
        .map_err(|e| {
            NeovegError::InvalidInput(format!(
                "falha ao resolver método '{}.{}' em '{}': {e}",
                job.type_name,
                job.method_name,
                job.assembly_path.display()
            ))
        })?;
    call_component_entry(&func, &job.payload_utf8)
}

fn host_sender() -> Result<&'static mpsc::Sender<HostJob>, NeovegError> {
    let res = HOST_THREAD.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<HostJob>();
        std::thread::Builder::new()
            .name("neoveg-dotnet-host".into())
            .spawn(move || {
                let mut runtime_ctx: Option<HostfxrContext<InitializedForRuntimeConfig>> = None;
                while let Ok(job) = rx.recv() {
                    let res = execute_on_host_thread(&mut runtime_ctx, &job);
                    let _ = job.reply.send(res);
                }
            })
            .map(|_| tx)
            .map_err(|e| format!("falha ao iniciar thread do host .NET: {e}"))
    });
    res.as_ref().map_err(|e| NeovegError::Internal(e.clone()))
}

/// Carrega o runtime `.NET` sob demanda via `hostfxr` e invoca o ponto de entrada
/// padrão `ComponentEntryPoint(IntPtr args, int sizeBytes) -> int` de `assembly_path`.
pub fn invoke_component(
    env: &DotnetEnv,
    assembly_path: &Path,
    type_name: &str,
    method_name: &str,
    payload_utf8: &[u8],
) -> Result<i32, NeovegError> {
    if payload_utf8.len() > MAX_PAYLOAD_BYTES {
        return Err(NeovegError::InvalidInput(format!(
            "payload para .NET Host excede o limite de {} MiB",
            MAX_PAYLOAD_BYTES / (1024 * 1024)
        )));
    }
    if !assembly_path.is_file() {
        return Err(NeovegError::Io(format!(
            "assembly .NET não encontrado: '{}'",
            assembly_path.display()
        )));
    }
    let sender = host_sender()?;
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    sender
        .send(HostJob {
            env: env.clone(),
            assembly_path: assembly_path.to_path_buf(),
            type_name: type_name.to_string(),
            method_name: method_name.to_string(),
            payload_utf8: payload_utf8.to_vec(),
            reply: reply_tx,
        })
        .map_err(|_| NeovegError::Internal("thread dedicada do host .NET foi encerrada".into()))?;
    reply_rx
        .recv()
        .map_err(|_| NeovegError::Internal("sem resposta da thread do host .NET".into()))?
}
