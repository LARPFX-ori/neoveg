//! Ponte `.NET Host` (`ScriptPortal.Vegas`) para scripts C# (`.cs`),
//! extensões e automação CLI/headless do Neoveg (AGENTS.md §3.2 / §3.3).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod dotnet_host;
#[forbid(unsafe_code)]
mod vegas_cs;

pub use dotnet_host::{find_dotnet_env, DotnetEnv};

use neoveg_common::error::NeovegError;
use neoveg_engine::commands::{
    resolved_shortcut, run_command, CommandOutcome, HeadlessHost, COMMANDS,
};
use neoveg_engine::model::Project;
use neoveg_engine::session::EditorSession;
use std::collections::HashMap;
use std::path::Path;

/// Limite seguro ao inspecionar um arquivo `.cs` de entrada (§0.4).
const MAX_SCRIPT_BYTES: u64 = 4 * 1024 * 1024;
/// Limite seguro ao carregar um assembly `.dll` compilado (§0.4).
const MAX_DLL_BYTES: u64 = 64 * 1024 * 1024;

/// Contexto de automação compatível com o modelo de objeto `ScriptPortal.Vegas`.
pub struct ScriptContext {
    pub session: EditorSession,
    pub host: HeadlessHost,
    pub messages: Vec<String>,
}

impl ScriptContext {
    #[forbid(unsafe_code)]
    pub fn new(project: Project) -> Self {
        Self {
            session: EditorSession::new(project),
            host: HeadlessHost,
            messages: Vec::new(),
        }
    }

    /// Executa qualquer ação registrada na Tabela Única de Comandos pelo `id`.
    #[forbid(unsafe_code)]
    pub fn execute(&mut self, command_id: &str) -> Result<CommandOutcome, String> {
        run_command(command_id, &mut self.session, &mut self.host)
    }
}

/// Executa um script C# (`.cs`) ou assembly pré-compilado (`.dll`) do modelo
/// `ScriptPortal.Vegas` carregando o runtime `.NET Core` sob demanda via `netcorehost`,
/// retornando eventuais mensagens emitidas pelo script via `Vegas.ShowMessage(...)`.
#[forbid(unsafe_code)]
pub fn run_cs_script_on_session(
    script_path: &Path,
    session: &mut EditorSession,
) -> Result<Vec<String>, NeovegError> {
    let mut host = HeadlessHost;
    let mut messages = Vec::new();
    let meta = std::fs::metadata(script_path).map_err(|e| {
        NeovegError::Io(format!(
            "não foi possível abrir o script {}: {e}",
            script_path.display()
        ))
    })?;
    if !meta.is_file() {
        return Err(NeovegError::InvalidInput(format!(
            "caminho '{}' não é um arquivo de script válido",
            script_path.display()
        )));
    }
    let is_dll = script_path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("dll"));
    if is_dll {
        if meta.len() > MAX_DLL_BYTES {
            return Err(NeovegError::InvalidInput(format!(
                "assembly '{}' excede o limite de {} MiB",
                script_path.display(),
                MAX_DLL_BYTES / (1024 * 1024)
            )));
        }
        vegas_cs::run_cs_script_impl(script_path, None, session, &mut host, &mut messages)?;
        return Ok(messages);
    }

    if meta.len() > MAX_SCRIPT_BYTES {
        return Err(NeovegError::InvalidInput(format!(
            "script '{}' excede o limite de {} MiB",
            script_path.display(),
            MAX_SCRIPT_BYTES / (1024 * 1024)
        )));
    }
    let source = std::fs::read_to_string(script_path).map_err(|e| {
        NeovegError::Io(format!(
            "erro ao ler o script {}: {e}",
            script_path.display()
        ))
    })?;
    if source.contains('\0') {
        return Err(NeovegError::InvalidInput(format!(
            "script '{}' contém byte nulo inválido",
            script_path.display()
        )));
    }

    vegas_cs::run_cs_script_impl(
        script_path,
        Some(&source),
        session,
        &mut host,
        &mut messages,
    )?;
    Ok(messages)
}

/// Executa um script C# (`.cs`) ou assembly pré-compilado (`.dll`) no `ScriptContext`.
#[forbid(unsafe_code)]
pub fn run_cs_script(script_path: &Path, ctx: &mut ScriptContext) -> Result<(), NeovegError> {
    let msgs = run_cs_script_on_session(script_path, &mut ctx.session)?;
    ctx.messages.extend(msgs);
    Ok(())
}

/// Executa um comando da Tabela Única via automação headless/CLI.
#[forbid(unsafe_code)]
pub fn run_command_headless(
    command_id: &str,
    project_path: Option<&Path>,
    save_path: Option<&Path>,
) -> Result<String, String> {
    let project = match project_path {
        Some(p) => Project::load_file(p)?.0,
        None => Project::new_default(),
    };
    let mut ctx = ScriptContext::new(project);
    ctx.session.path = project_path.map(Path::to_path_buf);
    let outcome = ctx.execute(command_id)?;
    if let Some(out) = save_path {
        ctx.session.project.save_file(out)?;
    }
    Ok(format!(
        "Comando '{command_id}' executado ({outcome:?}). Trilhas: {}, mídia: {}, marcadores: {}.",
        ctx.session.project.tracks.len(),
        ctx.session.project.media.len(),
        ctx.session.project.markers.len(),
    ))
}

/// Lista todos os comandos disponíveis na Tabela Única (para CLI/documentação).
#[forbid(unsafe_code)]
pub fn list_commands(
    overrides: &HashMap<String, Option<String>>,
) -> Vec<(&'static str, &'static str, &'static str, Option<String>)> {
    COMMANDS
        .iter()
        .map(|c| {
            (
                c.id,
                c.menu_path,
                c.label,
                resolved_shortcut(c, overrides).map(|s| s.format()),
            )
        })
        .collect()
}

#[forbid(unsafe_code)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_cs_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("neoveg_script_{tag}_{}.cs", std::process::id()))
    }

    #[test]
    fn script_inexistente_retorna_erro_sem_panico() {
        let p = temp_cs_path("inexistente");
        let _ = std::fs::remove_file(&p);
        let mut ctx = ScriptContext::new(Project::new_default());
        let err = run_cs_script(&p, &mut ctx).unwrap_err();
        assert!(matches!(err, NeovegError::Io(_)));
    }

    #[test]
    fn script_nulo_rejeitado_sem_panico() {
        let p = temp_cs_path("nulo");
        std::fs::write(&p, b"using ScriptPortal.Vegas;\0public class EntryPoint {}").unwrap();
        let mut ctx = ScriptContext::new(Project::new_default());
        let err = run_cs_script(&p, &mut ctx).unwrap_err();
        let _ = std::fs::remove_file(&p);
        assert!(matches!(err, NeovegError::InvalidInput(_)));
    }

    #[test]
    fn script_cs_sintaxe_invalida_retorna_erro_sem_panico() {
        let p = temp_cs_path("sintaxe_quebrada");
        std::fs::write(
            &p,
            "using ScriptPortal.Vegas;\npublic class EntryPoint { invalid C# syntax ??? }\n",
        )
        .unwrap();
        let mut ctx = ScriptContext::new(Project::new_default());
        let err = run_cs_script(&p, &mut ctx).unwrap_err();
        let _ = std::fs::remove_file(&p);
        assert!(
            matches!(
                err,
                NeovegError::InvalidInput(_) | NeovegError::Unsupported(_)
            ),
            "erro inesperado: {err:?}"
        );
    }

    #[test]
    fn script_cs_excecao_hostil_retorna_erro_sem_panico() {
        let Ok(env) = find_dotnet_env() else {
            return;
        };
        if !env.has_sdk {
            return;
        }
        let p = temp_cs_path("excecao_hostil");
        std::fs::write(
            &p,
            r#"using System;
using ScriptPortal.Vegas;

public class EntryPoint
{
    public void FromVegas(Vegas vegas)
    {
        throw new InvalidOperationException("entrada hostil simulada");
    }
}
"#,
        )
        .unwrap();
        let mut ctx = ScriptContext::new(Project::new_default());
        let err = run_cs_script(&p, &mut ctx).unwrap_err();
        let _ = std::fs::remove_file(&p);
        assert!(
            matches!(err, NeovegError::InvalidInput(ref msg) if msg.contains("InvalidOperationException")),
            "esperava InvalidOperationException controlada, obtive: {err:?}"
        );
    }

    #[test]
    fn script_cs_executa_via_netcorehost_e_aplica_mutacoes() {
        let Ok(env) = find_dotnet_env() else {
            return;
        };
        if !env.has_sdk {
            return;
        }
        let p = temp_cs_path("valido");
        std::fs::write(
            &p,
            r#"using ScriptPortal.Vegas;

public class EntryPoint
{
    public void FromVegas(Vegas vegas)
    {
        vegas.AddVideoTrack("Cor");
        vegas.AddAudioBus("FX Verb");
        vegas.AddMarker(1.5, "Intro");
        vegas.Transport.CursorPosition = Timecode.FromSeconds(2.0);
        vegas.RunCommand("insert.marker");
        vegas.ShowMessage("OK-Bridge");
    }
}
"#,
        )
        .unwrap();
        let mut ctx = ScriptContext::new(Project::new_default());
        let base_tracks = ctx.session.project.tracks.len();
        run_cs_script(&p, &mut ctx).unwrap();
        let _ = std::fs::remove_file(&p);

        assert_eq!(ctx.session.project.tracks.len(), base_tracks + 1);
        assert_eq!(ctx.session.project.tracks.last().unwrap().name, "Cor");
        assert!(ctx
            .session
            .project
            .audio_buses
            .iter()
            .any(|b| b.name == "FX Verb"));
        assert!(ctx
            .session
            .project
            .markers
            .iter()
            .any(|m| m.label == "Intro"));
        assert_eq!(ctx.session.project.markers.len(), 2);
        assert!(ctx.session.cursor > neoveg_engine::time::Ticks::ZERO);
        assert_eq!(ctx.messages, vec!["OK-Bridge".to_string()]);

        // Todo o script é agrupado em um único passo atômico de Desfazer.
        ctx.execute("edit.undo").unwrap();
        assert_eq!(ctx.session.project.tracks.len(), base_tracks);
        assert_eq!(ctx.session.project.markers.len(), 0);
        assert!(!ctx
            .session
            .project
            .audio_buses
            .iter()
            .any(|b| b.name == "FX Verb"));

        ctx.execute("edit.redo").unwrap();
        assert_eq!(ctx.session.project.tracks.len(), base_tracks + 1);
        assert_eq!(ctx.session.project.markers.len(), 2);
    }

    #[test]
    fn script_cs_falha_em_comando_faz_rollback_atomico() {
        let Ok(env) = find_dotnet_env() else {
            return;
        };
        if !env.has_sdk {
            return;
        }
        let p = temp_cs_path("rollback");
        std::fs::write(
            &p,
            r#"using ScriptPortal.Vegas;

public class EntryPoint
{
    public void FromVegas(Vegas vegas)
    {
        vegas.AddVideoTrack("NaoDevePermanecer");
        vegas.AddMarker(3.0, "Orfao");
        vegas.RunCommand("comando.inexistente");
    }
}
"#,
        )
        .unwrap();
        let mut ctx = ScriptContext::new(Project::new_default());
        let base_tracks = ctx.session.project.tracks.len();
        let err = run_cs_script(&p, &mut ctx).unwrap_err();
        let _ = std::fs::remove_file(&p);

        assert!(matches!(err, NeovegError::InvalidInput(_)), "erro: {err:?}");
        assert_eq!(ctx.session.project.tracks.len(), base_tracks);
        assert!(ctx.session.project.markers.is_empty());
        assert!(!ctx.session.can_undo());
        assert!(!ctx.session.dirty);
    }

    #[test]
    fn script_cs_exceder_limite_26_barramentos_falha_atomicamente() {
        let Ok(env) = find_dotnet_env() else {
            return;
        };
        if !env.has_sdk {
            return;
        }
        let p = temp_cs_path("bus_limit");
        std::fs::write(
            &p,
            r#"using ScriptPortal.Vegas;

public class EntryPoint
{
    public void FromVegas(Vegas vegas)
    {
        for (int i = 0; i < 27; i++)
        {
            vegas.AddAudioBus($"Extra {i}");
        }
    }
}
"#,
        )
        .unwrap();
        let mut ctx = ScriptContext::new(Project::new_default());
        let orig_buses = ctx.session.project.audio_buses.len();
        let err = run_cs_script(&p, &mut ctx).unwrap_err();
        let _ = std::fs::remove_file(&p);

        assert!(
            matches!(err, NeovegError::InvalidInput(ref msg) if msg.contains("26")),
            "esperava erro de limite de 26 barramentos, obtive: {err:?}"
        );
        assert_eq!(ctx.session.project.audio_buses.len(), orig_buses);
    }

    #[test]
    fn script_dll_hostil_rejeitado_sem_panico() {
        let dir = std::env::temp_dir().join(format!("neoveg_bad_dll_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let p_text = dir.join("lixo.dll");
        std::fs::write(&p_text, b"isto nao e uma dll valida").unwrap();
        let mut ctx = ScriptContext::new(Project::new_default());
        assert!(matches!(
            run_cs_script(&p_text, &mut ctx).unwrap_err(),
            NeovegError::InvalidInput(_)
        ));

        let p_mz = dir.join("truncada.dll");
        let mut mz_buf = vec![0u8; 256];
        mz_buf[0] = b'M';
        mz_buf[1] = b'Z';
        mz_buf[60..64].copy_from_slice(&0x80u32.to_le_bytes());
        mz_buf[0x80..0x84].copy_from_slice(b"PE\0\0");
        std::fs::write(&p_mz, &mz_buf).unwrap();
        assert!(matches!(
            run_cs_script(&p_mz, &mut ctx).unwrap_err(),
            NeovegError::InvalidInput(_)
        ));

        // Cabeçalho PE32 + diretório CLR válidos em tamanho, porém metadados CLR nulos/corrompidos.
        let Ok(env) = find_dotnet_env() else {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        if !env.has_sdk {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let p_corrupt_clr = dir.join("corrompida_clr.dll");
        let mut fake_clr = vec![0u8; 1024];
        fake_clr[0] = b'M';
        fake_clr[1] = b'Z';
        fake_clr[60..64].copy_from_slice(&0x80u32.to_le_bytes());
        let pe = 0x80usize;
        fake_clr[pe..pe + 4].copy_from_slice(b"PE\0\0");
        fake_clr[pe + 20..pe + 22].copy_from_slice(&224u16.to_le_bytes());
        fake_clr[pe + 24..pe + 26].copy_from_slice(&0x010bu16.to_le_bytes());
        fake_clr[pe + 24 + 92..pe + 24 + 96].copy_from_slice(&16u32.to_le_bytes());
        fake_clr[pe + 24 + 208..pe + 24 + 212].copy_from_slice(&0x2000u32.to_le_bytes());
        fake_clr[pe + 24 + 212..pe + 24 + 216].copy_from_slice(&72u32.to_le_bytes());
        std::fs::write(&p_corrupt_clr, &fake_clr).unwrap();

        let err_corrupt = run_cs_script(&p_corrupt_clr, &mut ctx).unwrap_err();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            matches!(
                err_corrupt,
                NeovegError::InvalidInput(_) | NeovegError::Internal(_)
            ),
            "esperava falha controlada para CLR corrompida, obtive: {err_corrupt:?}"
        );
    }

    #[test]
    fn script_dll_externa_compilada_executa_sem_neoveg_bridge() {
        let Ok(env) = find_dotnet_env() else {
            return;
        };
        if !env.has_sdk {
            return;
        }
        let cli = env
            .dotnet_cli
            .as_deref()
            .unwrap_or_else(|| Path::new("dotnet"));
        let dir = std::env::temp_dir().join(format!("neoveg_ext_dll_{}", std::process::id()));
        let src_dir = dir.join("src");
        let out_dir = dir.join("out");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();

        let csproj = format!(
            r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>{}</TargetFramework>
    <OutputType>Library</OutputType>
    <AssemblyName>CustomUserPlugin</AssemblyName>
    <Nullable>disable</Nullable>
    <UseSharedCompilation>false</UseSharedCompilation>
  </PropertyGroup>
</Project>
"#,
            env.tfm
        );
        std::fs::write(src_dir.join("CustomUserPlugin.csproj"), csproj).unwrap();
        std::fs::write(
            src_dir.join("Plugin.cs"),
            r#"internal class EntryPoint
{
    public void FromVegas(dynamic vegas)
    {
        vegas.Project.AddVideoTrack("ViaDllExterna");
        vegas.Project.AddMarker(2.5, "MarcoDll");
        vegas.ShowMessage("DLL-OK");
    }
}
"#,
        )
        .unwrap();

        let build = std::process::Command::new(cli)
            .arg("build")
            .arg(src_dir.join("CustomUserPlugin.csproj"))
            .arg("-c")
            .arg("Release")
            .arg("--nologo")
            .arg("-nodeReuse:false")
            .arg("-p:UseSharedCompilation=false")
            .arg("-o")
            .arg(&out_dir)
            .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
            .env("DOTNET_NOLOGO", "1")
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "falha ao compilar DLL externa: {}",
            String::from_utf8_lossy(&build.stderr)
        );

        let dll_path = out_dir.join("CustomUserPlugin.dll");
        let mut ctx = ScriptContext::new(Project::new_default());
        let base_tracks = ctx.session.project.tracks.len();
        run_cs_script(&dll_path, &mut ctx).unwrap();

        assert_eq!(ctx.session.project.tracks.len(), base_tracks + 1);
        assert_eq!(
            ctx.session.project.tracks.last().unwrap().name,
            "ViaDllExterna"
        );
        assert!(ctx
            .session
            .project
            .markers
            .iter()
            .any(|m| m.label == "MarcoDll"));
        assert_eq!(ctx.messages, vec!["DLL-OK".to_string()]);

        ctx.execute("edit.undo").unwrap();
        assert_eq!(ctx.session.project.tracks.len(), base_tracks);
        assert!(ctx.session.project.markers.is_empty());

        // Limpa depois de tudo para provar que a DLL não ficou bloqueada em arquivo no Windows.
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn script_cs_main_com_argumentos_executa_sem_panico() {
        let Ok(env) = find_dotnet_env() else {
            return;
        };
        if !env.has_sdk {
            return;
        }
        let p = temp_cs_path("main_args");
        std::fs::write(
            &p,
            r#"public class Program
{
    public static int Main(string[] args)
    {
        return args.Length;
    }
}
"#,
        )
        .unwrap();
        let mut ctx = ScriptContext::new(Project::new_default());
        run_cs_script(&p, &mut ctx).unwrap();
        let _ = std::fs::remove_file(&p);
        assert!(!ctx.session.can_undo());
    }

    #[test]
    fn automacao_headless_despacha_comandos_da_tabela() {
        let mut ctx = ScriptContext::new(Project::new_default());
        ctx.execute("insert.marker").unwrap();
        ctx.execute("insert.track_video").unwrap();
        assert_eq!(ctx.session.project.markers.len(), 1);
        assert_eq!(ctx.session.project.tracks.len(), 3);
        ctx.execute("edit.undo").unwrap();
        assert_eq!(ctx.session.project.tracks.len(), 2);
    }
}
