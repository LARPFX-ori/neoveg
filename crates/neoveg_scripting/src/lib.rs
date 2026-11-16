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
mod script_ui;
#[forbid(unsafe_code)]
mod vegas_cs;
#[forbid(unsafe_code)]
mod winforms_shim;

pub use dotnet_host::{find_dotnet_env, DotnetEnv};
pub use script_ui::{
    ControlKind, ControlSpec, ControlValue, FileDialogMode, FileDialogSpec, FormSpec,
    MessageButtons, ScriptUi, ScriptUiReply, ScriptUiRequest,
};
pub use vegas_cs::ScriptRunOptions;

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
    run_cs_script_on_session_with(script_path, session, &ScriptRunOptions::default())
}

/// Como [`run_cs_script_on_session`], com as opções da execução (quem
/// mostra as janelas da camada de Forms própria).
#[forbid(unsafe_code)]
pub fn run_cs_script_on_session_with(
    script_path: &Path,
    session: &mut EditorSession,
    options: &ScriptRunOptions,
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
        vegas_cs::run_cs_script_impl(
            script_path,
            None,
            session,
            &mut host,
            &mut messages,
            options,
        )?;
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
        options,
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

    /// Projeto com 3 trilhas (V1, A1, V2) e 3 marcadores, para os testes de
    /// alterações em itens existentes.
    fn projeto_com_itens() -> Project {
        use neoveg_engine::model::{Marker, Track, TrackKind};
        let mut p = Project::new_default();
        p.tracks = vec![
            Track::new(TrackKind::Video, "V1"),
            Track::new(TrackKind::Audio, "A1"),
            Track::new(TrackKind::Video, "V2"),
        ];
        p.markers = (0..3)
            .map(|i| Marker {
                id: neoveg_engine::model::new_id(),
                t: neoveg_engine::time::Ticks::from_secs_i64(i + 1),
                label: format!("M{i}"),
            })
            .collect();
        p
    }

    fn roda_script(
        tag: &str,
        corpo: &str,
        p: Project,
    ) -> Option<(ScriptContext, Result<(), NeovegError>)> {
        let env = find_dotnet_env().ok()?;
        if !env.has_sdk {
            return None;
        }
        let path = temp_cs_path(tag);
        let src = format!(
            "using System.Linq;\nusing ScriptPortal.Vegas;\n\npublic class EntryPoint\n{{\n    public void FromVegas(Vegas vegas)\n    {{\n{corpo}\n    }}\n}}\n"
        );
        std::fs::write(&path, src).ok()?;
        let mut ctx = ScriptContext::new(p);
        let r = run_cs_script(&path, &mut ctx);
        let _ = std::fs::remove_file(&path);
        Some((ctx, r))
    }

    /// Antes: renomear trilha, mover/renomear marcador e apagar marcador
    /// "funcionavam" no script mas eram ignorados em silêncio.
    #[test]
    fn script_altera_trilhas_e_marcadores_existentes() {
        let p = projeto_com_itens();
        let ids: Vec<_> = p.tracks.iter().map(|t| t.id).collect();
        let m1_id = p.markers[1].id;
        let m2_t = p.markers[2].t;
        let corpo = r#"
        vegas.Project.Tracks[0].Name = "Câmera";
        vegas.Project.Markers[0].Position = Timecode.FromSeconds(10.0);
        vegas.Project.Markers[1].Label = "Novo rótulo";
        vegas.Project.Markers.RemoveAt(2);"#;
        let Some((ctx, r)) = roda_script("altera", corpo, p) else {
            return;
        };
        r.unwrap();
        let pr = &ctx.session.project;
        assert_eq!(pr.tracks[0].name, "Câmera");
        assert_eq!(
            pr.tracks.iter().map(|t| t.id).collect::<Vec<_>>(),
            ids,
            "ids mantidos"
        );
        assert_eq!(pr.markers.len(), 2, "marcador apagado");
        assert!(pr.markers.iter().all(|m| m.t != m2_t));
        let m0 = pr.markers.iter().find(|m| m.label == "M0").expect("M0");
        assert_eq!(m0.t, neoveg_engine::time::Ticks::from_secs_i64(10));
        let m1 = pr
            .markers
            .iter()
            .find(|m| m.id == m1_id)
            .expect("M1 mantém id");
        assert_eq!(m1.label, "Novo rótulo");
        assert_eq!(
            m1.t,
            neoveg_engine::time::Ticks::from_secs_i64(2),
            "tempo exato mantido"
        );
    }

    /// Antes: apagar e reordenar trilhas era ignorado, e trilha nova entrava
    /// sempre no fim e sem o Mute pedido.
    #[test]
    fn script_apaga_reordena_e_insere_trilhas() {
        let p = projeto_com_itens();
        let (v1, v2) = (p.tracks[0].id, p.tracks[2].id);
        let corpo = r#"
        var t = vegas.Project.Tracks;
        t.RemoveAt(1);
        var v2 = t[1];
        t.RemoveAt(1);
        t.Insert(0, v2);
        var nova = new AudioTrack { Name = "Narração", Mute = true };
        t.Insert(1, nova);"#;
        let Some((ctx, r)) = roda_script("reordena", corpo, p) else {
            return;
        };
        r.unwrap();
        let pr = &ctx.session.project;
        assert_eq!(pr.tracks.len(), 3);
        assert_eq!(pr.tracks[0].id, v2);
        assert_eq!(pr.tracks[1].name, "Narração");
        assert!(pr.tracks[1].mute, "Mute da trilha nova aplicado");
        assert_eq!(pr.tracks[2].id, v1);
        assert!(pr.tracks.iter().all(|t| t.name != "A1"), "A1 apagada");
    }

    /// §3.1: a mesma trilha duas vezes na lista é recusada inteira.
    #[test]
    fn script_com_trilha_repetida_e_rejeitado_sem_mudar_nada() {
        let p = projeto_com_itens();
        let antes: Vec<_> = p.tracks.iter().map(|t| (t.id, t.name.clone())).collect();
        let corpo = r#"
        vegas.Project.Tracks[1].Name = "mudou";
        vegas.Project.Tracks.Add(vegas.Project.Tracks[0]);"#;
        let Some((ctx, r)) = roda_script("repetida", corpo, p) else {
            return;
        };
        assert!(r.is_err());
        let depois: Vec<_> = ctx
            .session
            .project
            .tracks
            .iter()
            .map(|t| (t.id, t.name.clone()))
            .collect();
        assert_eq!(depois, antes);
    }

    /// Um script que só lê não cria passo de Desfazer nem muda o projeto.
    #[test]
    fn script_que_so_le_nao_muda_o_projeto() {
        let p = projeto_com_itens();
        let corpo = r#"
        var n = vegas.Project.Tracks.Count + vegas.Project.Markers.Count;
        vegas.ShowMessage("itens: " + n);"#;
        let Some((ctx, r)) = roda_script("leitura", corpo, p) else {
            return;
        };
        r.unwrap();
        assert!(ctx.session.undo.is_empty());
        assert_eq!(ctx.session.project.tracks.len(), 3);
        assert_eq!(ctx.session.project.markers.len(), 3);
    }

    /// Projeto com V1 (eventos a 0 s e 5 s), A1 (evento a 0 s) e V2 vazia.
    fn projeto_com_eventos() -> Project {
        use neoveg_engine::model::{Event, MediaItem, MediaSource, Track, TrackKind};
        use neoveg_engine::time::Ticks;
        let media = MediaItem {
            id: neoveg_engine::model::new_id(),
            name: "m".into(),
            source: MediaSource::File {
                path: "m.avi".into(),
            },
            duration: Some(Ticks::from_secs_i64(60)),
            has_video: true,
            has_audio: true,
            width: 64,
            height: 36,
            fps: None,
            info: String::new(),
            proxy_path: None,
        };
        let mut v1 = Track::new(TrackKind::Video, "V1");
        v1.events = vec![
            Event::new(&media, Ticks::ZERO, Ticks::from_secs_i64(4)),
            Event::new(&media, Ticks::from_secs_i64(5), Ticks::from_secs_i64(3)),
        ];
        let mut a1 = Track::new(TrackKind::Audio, "A1");
        a1.events = vec![Event::new(&media, Ticks::ZERO, Ticks::from_secs_i64(4))];
        let mut p = Project::new_default();
        p.media = vec![media];
        p.tracks = vec![v1, a1, Track::new(TrackKind::Video, "V2")];
        p
    }

    /// Eventos, cursor e seleção pela API (Scripting API Summary: TrackEvent,
    /// TransportControl, Vegas.Cursor): mover dentro da trilha e para outra
    /// trilha do mesmo tipo, apagar, mudar duração e fades, com tempo exato.
    #[test]
    fn script_edita_eventos_com_tempo_exato() {
        let p = projeto_com_eventos();
        let (e0, e1) = (p.tracks[0].events[0].id, p.tracks[0].events[1].id);
        let a0 = p.tracks[1].events[0].id;
        let corpo = r#"
        var v1 = vegas.Project.Tracks[0];
        var v2 = vegas.Project.Tracks[2];
        var ev = v1.Events[0];
        ev.Start = ev.Start + Timecode.FromFrames(10);
        ev.Length = Timecode.FromSeconds(2);
        ev.FadeIn.Length = Timecode.FromMilliseconds(500);
        ev.Name = "Abertura";
        v1.Events[1].Track = v2;
        vegas.Project.Tracks[1].Events.Clear();"#;
        let Some((ctx, r)) = roda_script("eventos", corpo, p) else {
            return;
        };
        r.unwrap();
        let pr = &ctx.session.project;
        let fps = pr.settings.fps;
        let ev = pr.tracks[0].events.iter().find(|e| e.id == e0).expect("e0");
        assert_eq!(ev.start, fps.frame_to_ticks(10), "10 quadros exatos");
        assert_eq!(ev.length, neoveg_engine::time::Ticks::from_secs_i64(2));
        assert_eq!(
            ev.fade_in.len,
            neoveg_engine::time::Ticks::from_millis_i64(500)
        );
        assert_eq!(ev.name, "Abertura");
        assert_eq!(pr.tracks[0].events.len(), 1);
        assert!(
            pr.tracks[2].events.iter().any(|e| e.id == e1),
            "e1 foi para V2"
        );
        assert!(
            pr.tracks[1].events.iter().all(|e| e.id != a0),
            "evento de áudio apagado"
        );
        assert_eq!(ctx.session.undo.len(), 1, "um passo de Desfazer");
    }

    #[test]
    fn script_evento_em_trilha_de_outro_tipo_e_recusado() {
        let p = projeto_com_eventos();
        let antes = p.tracks[0].events.len();
        let corpo = r#"
        vegas.Project.Tracks[0].Events[0].Track = vegas.Project.Tracks[1];"#;
        let Some((ctx, r)) = roda_script("tipo", corpo, p) else {
            return;
        };
        assert!(r.is_err());
        assert_eq!(ctx.session.project.tracks[0].events.len(), antes);
        assert!(ctx.session.undo.is_empty());
    }

    #[test]
    fn script_evento_repetido_ou_duracao_zero_e_recusado() {
        for (tag, corpo) in [
            (
                "repetido",
                "        vegas.Project.Tracks[2].Events.Add(vegas.Project.Tracks[0].Events[0]);",
            ),
            (
                "zero",
                "        vegas.Project.Tracks[0].Events[0].Length = new Timecode();",
            ),
            (
                "negativo",
                "        vegas.Project.Tracks[0].Events[0].Start = Timecode.FromSeconds(-1);",
            ),
        ] {
            let p = projeto_com_eventos();
            let antes: Vec<_> = p.tracks.iter().map(|t| t.events.len()).collect();
            let Some((ctx, r)) = roda_script(tag, corpo, p) else {
                return;
            };
            assert!(r.is_err(), "{tag} devia ser recusado");
            let depois: Vec<_> = ctx
                .session
                .project
                .tracks
                .iter()
                .map(|t| t.events.len())
                .collect();
            assert_eq!(depois, antes, "{tag}: projeto mudou");
        }
    }

    /// Seleção e cursor não são edição do projeto: sem passo de Desfazer.
    #[test]
    fn script_seleciona_e_move_cursor_sem_desfazer() {
        let p = projeto_com_eventos();
        let e1 = p.tracks[0].events[1].id;
        let corpo = r#"
        vegas.Project.Tracks[0].Events[1].Selected = true;
        // como no FAQ do manual (InsertFileAt): limpa a seleção de trilhas
        // antes de escolher uma, porque marcar uma não desmarca as outras
        foreach (var t in vegas.Project.Tracks) { t.Selected = false; }
        vegas.Project.Tracks[2].Selected = true;
        vegas.Cursor = Timecode.FromSeconds(3);
        vegas.Transport.SelectionStart = Timecode.FromSeconds(1);
        vegas.Transport.SelectionLength = Timecode.FromSeconds(2);"#;
        let Some((ctx, r)) = roda_script("selecao", corpo, p) else {
            return;
        };
        r.unwrap();
        let s = &ctx.session;
        assert!(s.undo.is_empty(), "seleção não cria passo de Desfazer");
        assert!(s.sel_events.contains(&e1) && s.sel_events.len() == 1);
        assert_eq!(s.sel_track, Some(2));
        assert_eq!(s.cursor, neoveg_engine::time::Ticks::from_secs_i64(3));
        assert_eq!(
            s.time_sel,
            Some((
                neoveg_engine::time::Ticks::from_secs_i64(1),
                neoveg_engine::time::Ticks::from_secs_i64(3)
            ))
        );
    }

    /// O Neoveg seleciona uma trilha por vez: com várias, fica a primeira e
    /// o script recebe uma mensagem (não some em silêncio).
    #[test]
    fn script_com_varias_trilhas_selecionadas_avisa() {
        let p = projeto_com_eventos();
        let corpo = r#"
        foreach (var t in vegas.Project.Tracks) { t.Selected = true; }"#;
        let env = match find_dotnet_env() {
            Ok(e) if e.has_sdk => e,
            _ => return,
        };
        let _ = env;
        let path = temp_cs_path("varias");
        let src = format!(
            "using ScriptPortal.Vegas;\npublic class EntryPoint {{ public void FromVegas(Vegas vegas) {{ {corpo} }} }}"
        );
        std::fs::write(&path, src).unwrap();
        let mut session = neoveg_engine::session::EditorSession::new(p);
        let msgs = run_cs_script_on_session(&path, &mut session).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(session.sel_track, Some(0));
        assert!(
            msgs.iter().any(|m| m.contains("uma trilha por vez")),
            "{msgs:?}"
        );
    }

    /// Eventos sem nenhuma mudança mantêm os ticks exatos (sem passar por
    /// milissegundos em ponto flutuante).
    #[test]
    fn script_que_le_eventos_nao_muda_tempo() {
        let mut p = projeto_com_eventos();
        // um início que não é múltiplo de milissegundo
        p.tracks[0].events[0].start = neoveg_engine::time::Ticks::from_raw(1_234_567);
        let antes = p.tracks[0].events[0].start;
        let corpo = r#"
        var total = new Timecode();
        foreach (var ev in vegas.Project.Tracks[0].Events) { total = total + ev.Length; }
        vegas.ShowMessage(total.ToString());"#;
        let Some((ctx, r)) = roda_script("leitura_eventos", corpo, p) else {
            return;
        };
        r.unwrap();
        assert!(ctx.session.undo.is_empty());
        assert_eq!(ctx.session.project.tracks[0].events[0].start, antes);
    }

    /// Projeto com duas mídias de vídeo+áudio (`a.avi`, `b.avi`) e uma só de
    /// áudio (`c.wav`); V1 tem um evento com dois takes (a ativa, b) e A1 um
    /// evento de áudio.
    fn projeto_com_takes() -> Project {
        use neoveg_engine::model::{Event, MediaItem, MediaSource, Take, Track, TrackKind};
        use neoveg_engine::time::Ticks;
        let item = |name: &str, video: bool| MediaItem {
            id: neoveg_engine::model::new_id(),
            name: name.into(),
            source: MediaSource::File { path: name.into() },
            duration: Some(Ticks::from_secs_i64(60)),
            has_video: video,
            has_audio: true,
            width: 64,
            height: 36,
            fps: None,
            info: String::new(),
            proxy_path: None,
        };
        let (a, b, c) = (
            item("a.avi", true),
            item("b.avi", true),
            item("c.wav", false),
        );
        let mut ev = Event::new(&a, Ticks::ZERO, Ticks::from_secs_i64(4));
        neoveg_engine::takes::add_take(
            &mut ev,
            Take {
                media_id: b.id,
                offset: Ticks::from_secs_i64(1),
                name: "B".into(),
            },
            false,
        );
        let mut v1 = Track::new(TrackKind::Video, "V1");
        v1.events = vec![ev];
        let mut a1 = Track::new(TrackKind::Audio, "A1");
        a1.events = vec![Event::new(&c, Ticks::ZERO, Ticks::from_secs_i64(4))];
        let mut p = Project::new_default();
        p.media = vec![a, b, c];
        p.tracks = vec![v1, a1];
        p
    }

    /// Takes, ActiveTake e Take.MediaPath (Scripting API Summary), como nos
    /// scripts que trocam o take ativo de todos os eventos.
    #[test]
    fn script_troca_take_ativo_e_le_a_midia() {
        let p = projeto_com_takes();
        let b = p.media[1].id;
        let id = p.tracks[0].events[0].id;
        let corpo = r#"
        var ev = vegas.Project.Tracks[0].Events[0];
        if (ev.Takes.Count != 2 || !ev.Takes[0].IsActive) throw new System.Exception("takes");
        ev.ActiveTake = ev.Takes[1];
        vegas.ShowMessage(ev.ActiveTake.MediaPath + "|" + ev.ActiveTake.Name + "|" + ev.ActiveTake.Offset.ToMilliseconds());"#;
        let Some((ctx, r)) = roda_script("take_ativo", corpo, p) else {
            return;
        };
        r.unwrap();
        let ev = ctx.session.project.event(id).expect("evento");
        assert_eq!(ev.media_id, b, "o take B ficou ativo");
        assert_eq!(ev.offset, neoveg_engine::time::Ticks::from_secs_i64(1));
        assert_eq!(neoveg_engine::takes::take_count(ev), 2);
        assert_eq!(ctx.session.undo.len(), 1, "um passo de Desfazer");
        assert_eq!(
            ctx.messages.last().map(String::as_str),
            Some("b.avi|B|1000")
        );
    }

    /// Ler mídia e takes não muda nada nem cria passo de Desfazer.
    #[test]
    fn script_que_so_le_media_e_takes_nao_muda_o_projeto() {
        let p = projeto_com_takes();
        let antes = format!("{:?}", p.tracks[0].events[0]);
        let corpo = r#"
        int n = 0;
        foreach (Media m in vegas.Project.MediaPool) { if (m.HasVideo()) n++; }
        var ev = vegas.Project.Tracks[0].Events[0];
        foreach (Take t in ev.Takes) { n += t.Index; }
        vegas.ShowMessage(n + "|" + vegas.Project.MediaPool.Count + "|" + ev.ActiveTake.Media.GetVideoStreamByIndex(0).Width);"#;
        let Some((ctx, r)) = roda_script("le_takes", corpo, p) else {
            return;
        };
        r.unwrap();
        assert!(ctx.session.undo.is_empty());
        assert_eq!(
            format!("{:?}", ctx.session.project.tracks[0].events[0]),
            antes
        );
        assert_eq!(ctx.messages.last().map(String::as_str), Some("3|3|64"));
    }

    /// AddTake com uma mídia achada pelo caminho (Media(path), como nos
    /// scripts que trocam MTS por AVI) e Offset do take ativo.
    #[test]
    fn script_adiciona_take_de_midia_do_projeto() {
        let mut p = projeto_com_takes();
        // só o take A
        neoveg_engine::takes::delete_take(&mut p.tracks[0].events[0], 1);
        let a = p.media[0].id;
        let b = p.media[1].id;
        let id = p.tracks[0].events[0].id;
        let corpo = r#"
        var ev = (VideoEvent)vegas.Project.Tracks[0].Events[0];
        var old = ev.ActiveTake.Offset;
        Media media = new Media("b.avi");
        ev.AddTake(media.GetVideoStreamByIndex(0), true);
        ev.ActiveTake.Offset = old + Timecode.FromSeconds(2);"#;
        let Some((ctx, r)) = roda_script("add_take", corpo, p) else {
            return;
        };
        r.unwrap();
        let ev = ctx.session.project.event(id).expect("evento");
        let list = neoveg_engine::takes::takes(ev);
        assert_eq!(
            list.iter().map(|t| t.media_id).collect::<Vec<_>>(),
            vec![a, b]
        );
        assert_eq!(ev.media_id, b, "o novo take é o ativo");
        assert_eq!(ev.offset, neoveg_engine::time::Ticks::from_secs_i64(2));
    }

    /// AddVideoEvent/AddAudioEvent + AddTake criam eventos com mídia do
    /// projeto, com tempo exato e um passo de Desfazer.
    #[test]
    fn script_cria_eventos_com_take() {
        let p = projeto_com_takes();
        let (a, c) = (p.media[0].id, p.media[2].id);
        let corpo = r#"
        var pool = vegas.Project.MediaPool;
        var v = (VideoTrack)vegas.Project.Tracks[0];
        var ev = v.AddVideoEvent(Timecode.FromSeconds(10), Timecode.FromFrames(30));
        ev.AddTake(pool.Find("a.avi").GetVideoStreamByIndex(0));
        ev.Name = "Novo";
        ev.Selected = true;
        var au = (AudioTrack)vegas.Project.Tracks[1];
        var ae = au.AddAudioEvent();
        ae.Start = Timecode.FromSeconds(10);
        ae.Length = Timecode.FromSeconds(1);
        ae.AddTake(pool.Find("c.wav").GetAudioStreamByIndex(0), true, "Som");"#;
        let Some((ctx, r)) = roda_script("cria_evento", corpo, p) else {
            return;
        };
        r.unwrap();
        let pr = &ctx.session.project;
        let fps = pr.settings.fps;
        let ten = neoveg_engine::time::Ticks::from_secs_i64(10);
        let ve = pr.tracks[0]
            .events
            .iter()
            .find(|e| e.start == ten)
            .expect("vídeo");
        assert_eq!(
            (ve.media_id, ve.length, ve.name.as_str()),
            (a, fps.frame_to_ticks(30), "Novo")
        );
        assert!(
            ctx.session.sel_events.contains(&ve.id),
            "o evento novo ficou selecionado"
        );
        let ae = pr.tracks[1]
            .events
            .iter()
            .find(|e| e.start == ten)
            .expect("áudio");
        assert_eq!((ae.media_id, ae.take_name.as_str()), (c, "Som"));
        assert_eq!(ctx.session.undo.len(), 1, "um passo de Desfazer");
    }

    /// Evento sem take, take do tipo errado, mídia fora do projeto e
    /// AddMedia recusam o script inteiro, sem mudar nada (§0).
    #[test]
    fn script_com_take_invalido_e_recusado_sem_mudar_nada() {
        for (tag, corpo) in [
            (
                "sem_take",
                r#"((VideoTrack)vegas.Project.Tracks[0]).AddVideoEvent(Timecode.FromSeconds(9), Timecode.FromSeconds(1));"#,
            ),
            (
                "tipo",
                r#"var ev = vegas.Project.Tracks[0].Events[0];
        ev.AddTake(vegas.Project.MediaPool.Find("c.wav").GetAudioStreamByIndex(0));"#,
            ),
            ("fora", r#"var m = new Media("nao_existe.avi");"#),
            (
                "add_media",
                r#"vegas.Project.MediaPool.AddMedia("nao_existe.avi");"#,
            ),
            (
                "offset",
                r#"vegas.Project.Tracks[0].Events[0].ActiveTake.Offset = Timecode.FromNanos(-5);"#,
            ),
            (
                "vazio",
                r#"vegas.Project.Tracks[0].Events[0].Takes.Clear();"#,
            ),
        ] {
            let p = projeto_com_takes();
            let antes = p.tracks.clone();
            let Some((ctx, r)) = roda_script(tag, corpo, p) else {
                return;
            };
            assert!(r.is_err(), "{tag} devia falhar");
            assert_eq!(ctx.session.project.tracks.len(), antes.len(), "{tag}");
            for (t, o) in ctx.session.project.tracks.iter().zip(&antes) {
                assert_eq!(
                    format!("{:?}", t.events),
                    format!("{:?}", o.events),
                    "{tag}: nada muda"
                );
            }
            assert!(ctx.session.undo.is_empty(), "{tag}");
        }
    }

    /// Interface de teste: guarda os pedidos e devolve respostas programadas
    /// (ou `NoUi` quando acabam).
    struct Respondedor {
        respostas: std::sync::Mutex<std::collections::VecDeque<ScriptUiReply>>,
        pedidos: std::sync::Mutex<Vec<ScriptUiRequest>>,
    }

    impl ScriptUi for Respondedor {
        fn request(&self, request: ScriptUiRequest) -> ScriptUiReply {
            if let Ok(mut p) = self.pedidos.lock() {
                p.push(request);
            }
            self.respostas
                .lock()
                .ok()
                .and_then(|mut r| r.pop_front())
                .unwrap_or(ScriptUiReply::NoUi)
        }
    }

    /// Contexto, resultado e pedidos de janela de uma rodada com a camada.
    type RodadaComForms = (
        ScriptContext,
        Result<Vec<String>, NeovegError>,
        Vec<ScriptUiRequest>,
    );

    /// Roda o script com a camada de Forms própria (também no Windows) e o
    /// respondedor; `None` sem .NET SDK.
    fn roda_com_forms(
        tag: &str,
        corpo: &str,
        respostas: Vec<ScriptUiReply>,
    ) -> Option<RodadaComForms> {
        let env = find_dotnet_env().ok()?;
        if !env.has_sdk {
            return None;
        }
        let path = temp_cs_path(tag);
        let src = format!(
            "using System.Windows.Forms;\nusing ScriptPortal.Vegas;\n\npublic class EntryPoint\n{{\n    public void FromVegas(Vegas vegas)\n    {{\n{corpo}\n    }}\n}}\n"
        );
        std::fs::write(&path, src).ok()?;
        let ui = std::sync::Arc::new(Respondedor {
            respostas: std::sync::Mutex::new(respostas.into()),
            pedidos: std::sync::Mutex::new(Vec::new()),
        });
        let opts = ScriptRunOptions {
            ui: Some(ui.clone()),
            force_forms_shim: true,
        };
        let mut ctx = ScriptContext::new(projeto_com_takes());
        let r = run_cs_script_on_session_with(&path, &mut ctx.session, &opts);
        let _ = std::fs::remove_file(&path);
        let pedidos = ui.pedidos.lock().map(|p| p.clone()).unwrap_or_default();
        Some((ctx, r, pedidos))
    }

    #[test]
    fn camada_forms_message_box_responde_pelo_neoveg() {
        let corpo = r#"
        var r = MessageBox.Show("Apagar?", "Confirma", MessageBoxButtons.YesNo, MessageBoxIcon.Question);
        vegas.ShowMessage(r.ToString());"#;
        let resp = vec![ScriptUiReply::MessageBox {
            result: "No".into(),
        }];
        let Some((_, r, pedidos)) = roda_com_forms("forms_msg", corpo, resp) else {
            return;
        };
        assert_eq!(r.unwrap().last().map(String::as_str), Some("No"));
        assert_eq!(
            pedidos,
            vec![ScriptUiRequest::MessageBox {
                text: "Apagar?".into(),
                caption: "Confirma".into(),
                buttons: MessageButtons::YesNo,
                icon: "Question".into(),
            }]
        );
    }

    /// Clique num botão com handler volta ao script, que muda outro
    /// controle; a janela é redesenhada e o OK fecha com DialogResult.OK.
    #[test]
    fn camada_forms_formulario_com_handler_e_ok() {
        let corpo = r#"
        var form = new Form { Text = "Marcador", ClientSize = new System.Drawing.Size(300, 120) };
        var rotulo = new Label { Text = "?", Left = 10, Top = 10 };
        var caixa = new TextBox { Text = "a", Left = 10, Top = 40 };
        var maiusc = new Button { Text = "Maiúsculas", Left = 10, Top = 80 };
        maiusc.Click += (s, e) => rotulo.Text = caixa.Text.ToUpper();
        var ok = new Button { Text = "OK", Left = 200, Top = 80, DialogResult = DialogResult.OK };
        form.Controls.AddRange(new Control[] { rotulo, caixa, maiusc, ok });
        form.AcceptButton = ok;
        var res = form.ShowDialog();
        vegas.Project.AddMarker(1.0, caixa.Text + "|" + rotulo.Text);
        vegas.ShowMessage(res.ToString());"#;
        let valores = |texto: &str| {
            vec![ControlValue {
                id: 1,
                text: texto.into(),
                ..Default::default()
            }]
        };
        let resp = vec![
            ScriptUiReply::Form {
                closed: false,
                result: None,
                clicked: Some(2),
                values: valores("abc"),
            },
            ScriptUiReply::Form {
                closed: false,
                result: None,
                clicked: Some(3),
                values: valores("abc"),
            },
        ];
        let Some((ctx, r, pedidos)) = roda_com_forms("forms_form", corpo, resp) else {
            return;
        };
        assert_eq!(r.unwrap().last().map(String::as_str), Some("OK"));
        assert_eq!(pedidos.len(), 2, "redesenhada depois do clique");
        let ScriptUiRequest::Form(f) = &pedidos[1] else {
            panic!("formulário");
        };
        assert_eq!(
            (f.title.as_str(), f.width, f.height),
            ("Marcador", 300.0, 120.0)
        );
        assert_eq!(f.accept, Some(3));
        assert_eq!(f.controls[0].text, "ABC", "o handler rodou no script");
        assert_eq!(f.controls[1].kind, Some(ControlKind::TextBox));
        assert_eq!(f.controls[3].dialog_result.as_deref(), Some("OK"));
        let m = ctx.session.project.markers.last().expect("marcador");
        assert_eq!(m.label, "abc|ABC");
    }

    #[test]
    fn camada_forms_dialogo_de_arquivo_e_extensao_padrao() {
        let corpo = r#"
        var d = new SaveFileDialog { Filter = "Texto|*.txt", DefaultExt = "txt", Title = "Salvar" };
        var res = d.ShowDialog();
        vegas.ShowMessage(res + "|" + System.IO.Path.GetFileName(d.FileName));"#;
        let resp = vec![ScriptUiReply::File {
            ok: true,
            paths: vec![std::env::temp_dir().join("lista").display().to_string()],
        }];
        let Some((_, r, pedidos)) = roda_com_forms("forms_file", corpo, resp) else {
            return;
        };
        assert_eq!(r.unwrap().last().map(String::as_str), Some("OK|lista.txt"));
        let ScriptUiRequest::FileDialog(d) = &pedidos[0] else {
            panic!("diálogo");
        };
        assert_eq!((d.mode, d.title.as_str()), (FileDialogMode::Save, "Salvar"));
        assert_eq!(
            d.filters(),
            vec![("Texto".to_string(), vec!["txt".to_string()])]
        );
    }

    /// Sem interface (CLI): MessageBox responde com o primeiro botão e
    /// registra o texto; formulário e diálogo de arquivo são cancelados.
    #[test]
    fn camada_forms_sem_interface_usa_o_padrao() {
        let corpo = r#"
        var r = MessageBox.Show("oi", "T", MessageBoxButtons.YesNo);
        var f = new Form().ShowDialog();
        var o = new OpenFileDialog().ShowDialog();
        vegas.ShowMessage(r + "|" + f + "|" + o);"#;
        let Some((_, r, _)) = roda_com_forms("forms_cli", corpo, Vec::new()) else {
            return;
        };
        let msgs = r.unwrap();
        assert!(msgs.iter().any(|m| m == "T: oi"), "{msgs:?}");
        assert_eq!(msgs.last().map(String::as_str), Some("Yes|Cancel|Cancel"));
    }

    /// No Windows, scripts usam o System.Windows.Forms real e rodam numa
    /// thread STA (diálogos de arquivo exigem STA). Só cria controles, sem
    /// abrir janela, para rodar também na CI.
    #[cfg(windows)]
    #[test]
    fn script_usa_winforms_real_no_windows_em_thread_sta() {
        let corpo = r#"
        var caixa = new System.Windows.Forms.TextBox();
        caixa.Text = "ok";
        var ap = System.Threading.Thread.CurrentThread.GetApartmentState();
        vegas.ShowMessage(caixa.Text + "|" + ap + "|" + System.Windows.Forms.DialogResult.OK);"#;
        let Some((ctx, r)) = roda_script("winforms", corpo, projeto_com_takes()) else {
            return;
        };
        r.unwrap();
        assert_eq!(ctx.messages.last().map(String::as_str), Some("ok|STA|OK"));
    }

    /// Uma exceção lançada na thread STA continua chegando como erro do
    /// script, sem mudar o projeto.
    #[cfg(windows)]
    #[test]
    fn excecao_na_thread_sta_vira_erro_do_script() {
        let corpo = r#"vegas.Project.Tracks[0].Name = "x";
        throw new System.InvalidOperationException("falhou na STA");"#;
        let p = projeto_com_takes();
        let nome = p.tracks[0].name.clone();
        let Some((ctx, r)) = roda_script("sta_erro", corpo, p) else {
            return;
        };
        let e = r.expect_err("o script falhou");
        assert!(format!("{e}").contains("falhou na STA"), "{e}");
        assert_eq!(ctx.session.project.tracks[0].name, nome);
    }

    /// WAV PCM 16 bits mono de `ms` milissegundos a 48 kHz, para o script
    /// importar (mídia gerada aqui, nunca commitada).
    fn grava_wav(tag: &str, ms: u32) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("neoveg_addmedia_{}_{tag}.wav", std::process::id()));
        let n = 48 * ms;
        let data = n * 2;
        let mut b = Vec::with_capacity(44 + data as usize);
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&1u16.to_le_bytes()); // mono
        b.extend_from_slice(&48_000u32.to_le_bytes());
        b.extend_from_slice(&96_000u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data.to_le_bytes());
        b.resize(44 + data as usize, 0);
        std::fs::write(&path, b).expect("grava wav");
        path
    }

    /// MediaPool.AddMedia e new Media(path) com um arquivo fora do projeto:
    /// o Neoveg abre o arquivo durante o script, e a mídia entra no projeto
    /// junto com o evento, num passo só de Desfazer.
    #[test]
    fn script_importa_midia_com_add_media() {
        let wav = grava_wav("importa", 1000);
        let caminho = wav.display().to_string();
        let corpo = format!(
            r#"
        var m = vegas.Project.MediaPool.AddMedia(@"{caminho}");
        if (!m.HasAudio() || m.HasVideo()) throw new System.Exception("streams");
        var au = (AudioTrack)vegas.Project.Tracks[1];
        var ae = au.AddAudioEvent(Timecode.FromSeconds(20), m.Length);
        ae.AddTake(m.GetAudioStreamByIndex(0));
        var de_novo = new Media(@"{caminho}");
        vegas.ShowMessage(vegas.Project.MediaPool.Count + "|" + (de_novo == m));"#
        );
        let p = projeto_com_takes();
        let Some((ctx, r)) = roda_script("add_media_ok", &corpo, p) else {
            let _ = std::fs::remove_file(&wav);
            return;
        };
        let _ = std::fs::remove_file(&wav);
        r.unwrap();
        let pr = &ctx.session.project;
        assert_eq!(pr.media.len(), 4, "a mídia importada entrou no projeto");
        let nova = pr.media.last().expect("mídia");
        assert!(nova.has_audio && !nova.has_video);
        let ev = pr.tracks[1]
            .events
            .iter()
            .find(|e| e.start == neoveg_engine::time::Ticks::from_secs_i64(20))
            .expect("evento novo");
        assert_eq!(ev.media_id, nova.id);
        assert_eq!(Some(ev.length), nova.duration);
        assert_eq!(ctx.messages.last().map(String::as_str), Some("4|True"));
        assert_eq!(ctx.session.undo.len(), 1, "um passo de Desfazer");
        assert_eq!(
            ctx.session.undo[0].media.len(),
            3,
            "o Desfazer volta ao projeto sem a mídia"
        );
    }

    /// Um script que importa mídia e depois falha não deixa a mídia no
    /// projeto (§0: nada muda num script que falha). Só importar, sem usar,
    /// põe a mídia no projeto, como no VEGAS.
    #[test]
    fn script_que_falha_depois_de_importar_nao_deixa_midia() {
        let wav = grava_wav("falha", 200);
        let caminho = wav.display().to_string();
        let falha = format!(
            r#"vegas.Project.MediaPool.AddMedia(@"{caminho}");
        throw new System.Exception("depois");"#
        );
        let so_importa = format!(r#"vegas.Project.MediaPool.AddMedia(@"{caminho}");"#);
        let r1 = roda_script("add_media_falha", &falha, projeto_com_takes());
        let r2 = roda_script("add_media_so", &so_importa, projeto_com_takes());
        let _ = std::fs::remove_file(&wav);
        let (Some((c1, r1)), Some((c2, r2))) = (r1, r2) else {
            return;
        };
        assert!(r1.is_err(), "o script falhou");
        assert_eq!(c1.session.project.media.len(), 3, "nada foi importado");
        assert!(c1.session.undo.is_empty());
        r2.unwrap();
        assert_eq!(c2.session.project.media.len(), 4);
        assert_eq!(c2.session.undo.len(), 1);
    }
}
