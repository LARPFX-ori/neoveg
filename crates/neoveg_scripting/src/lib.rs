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

/// Contexto de automação compatível com o modelo de objeto `ScriptPortal.Vegas`.
pub struct ScriptContext {
    pub session: EditorSession,
    pub host: HeadlessHost,
}

impl ScriptContext {
    pub fn new(project: Project) -> Self {
        Self {
            session: EditorSession::new(project),
            host: HeadlessHost,
        }
    }

    /// Executa qualquer ação registrada na Tabela Única de Comandos pelo `id`.
    pub fn execute(&mut self, command_id: &str) -> Result<CommandOutcome, String> {
        run_command(command_id, &mut self.session, &mut self.host)
    }
}

/// Tenta executar um script C# (`.cs`) do VEGAS Pro (`ScriptPortal.Vegas`).
///
/// Como a fronteira `.NET Host` ainda não está vinculada nesta etapa,
/// valida o arquivo com segurança e retorna erro explícito de recurso não
/// suportado (§0.2).
pub fn run_cs_script(script_path: &Path, _ctx: &mut ScriptContext) -> Result<(), NeovegError> {
    let meta = std::fs::metadata(script_path).map_err(|e| {
        NeovegError::Io(format!(
            "não foi possível abrir o script {}: {e}",
            script_path.display()
        ))
    })?;
    if meta.len() > MAX_SCRIPT_BYTES {
        return Err(NeovegError::InvalidInput(format!(
            "script '{}' excede o limite de {} MiB",
            script_path.display(),
            MAX_SCRIPT_BYTES / (1024 * 1024)
        )));
    }
    let _source = std::fs::read_to_string(script_path).map_err(|e| {
        NeovegError::Io(format!(
            "erro ao ler o script {}: {e}",
            script_path.display()
        ))
    })?;
    Err(NeovegError::Unsupported(format!(
        "execução de scripts .NET/C# ('{}' via ScriptPortal.Vegas) ainda não suportada; use '--command <id>' para automação headless",
        script_path.display()
    )))
}

/// Executa um comando da Tabela Única via automação headless/CLI.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_cs_retorna_erro_explicito_sem_panico() {
        let p = std::env::temp_dir().join(format!("neoveg_script_test_{}.cs", std::process::id()));
        std::fs::write(
            &p,
            "using ScriptPortal.Vegas;\npublic class EntryPoint {}\n",
        )
        .unwrap();
        let mut ctx = ScriptContext::new(Project::new_default());
        let err = run_cs_script(&p, &mut ctx).unwrap_err();
        let _ = std::fs::remove_file(&p);
        assert!(matches!(err, NeovegError::Unsupported(_)));
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
