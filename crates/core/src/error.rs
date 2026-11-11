use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path} is not a git repository")]
    NotARepository { path: PathBuf },
    #[error("git {command} failed: {message}")]
    GitCommand { command: String, message: String },
    #[error("git is not installed or not on PATH")]
    GitMissing,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
