//! Tipos de erro compartilhados do Neoveg (AGENTS.md §0.1).

use std::fmt;

pub type Result<T> = std::result::Result<T, NeovegError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NeovegError {
    Io(String),
    Json(String),
    InvalidInput(String),
    Unsupported(String),
    Internal(String),
}

impl fmt::Display for NeovegError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NeovegError::Io(m) => write!(f, "erro de E/S: {m}"),
            NeovegError::Json(m) => write!(f, "erro ao interpretar dados JSON: {m}"),
            NeovegError::InvalidInput(m) => write!(f, "entrada inválida: {m}"),
            NeovegError::Unsupported(m) => write!(f, "recurso não suportado: {m}"),
            NeovegError::Internal(m) => write!(f, "erro interno: {m}"),
        }
    }
}

impl std::error::Error for NeovegError {}

impl From<std::io::Error> for NeovegError {
    fn from(e: std::io::Error) -> Self {
        NeovegError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for NeovegError {
    fn from(e: serde_json::Error) -> Self {
        NeovegError::Json(e.to_string())
    }
}

impl From<String> for NeovegError {
    fn from(s: String) -> Self {
        NeovegError::Internal(s)
    }
}
