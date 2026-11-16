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
}
