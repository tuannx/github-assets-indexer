use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("already exists: {0}")]
    AlreadyExists(String),
    #[error("authentication failed")]
    AuthFailed,
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("rate limited")]
    RateLimited,
    #[error("provider error: {0}")]
    Provider(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("cancelled")]
    Cancelled,
    #[error(
        "no project workspace found — run `github-local-indexer install` inside a git repo, \
         or set GITHUB_LOCAL_INDEXER_GLOBAL=1 to use ~/.local/share"
    )]
    NoWorkspace,
}

impl AppError {
    pub fn storage(err: impl std::fmt::Display) -> Self {
        Self::Storage(err.to_string())
    }

    pub fn provider(err: impl std::fmt::Display) -> Self {
        Self::Provider(err.to_string())
    }
}
