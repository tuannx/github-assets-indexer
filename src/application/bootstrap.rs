use std::path::PathBuf;
use std::sync::Arc;

use crate::adapters::fake::FakeProvider;
use crate::adapters::github::GitHubProvider;
use crate::adapters::sqlite::SqliteStore;
use crate::application::ports::SourceStore;
use crate::application::services::App;
use crate::domain::error::AppError;
use crate::domain::paths;
use crate::domain::project;

pub async fn build_app(db_path: Option<PathBuf>) -> Result<App, AppError> {
    open_app(db_path).await
}

pub async fn build_app_for_cli(db_path: Option<PathBuf>) -> Result<App, AppError> {
    let path = match db_path {
        Some(p) => p,
        None => resolve_cli_database_path()?,
    };
    open_app(Some(path)).await
}

fn resolve_cli_database_path() -> Result<PathBuf, AppError> {
    if use_global_database() {
        return Ok(paths::database_path());
    }
    project::discover_from_cwd()
        .map(|ws| ws.database_path())
        .ok_or(AppError::NoWorkspace)
}

fn use_global_database() -> bool {
    std::env::var("GITHUB_LOCAL_INDEXER_GLOBAL")
        .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

async fn open_app(db_path: Option<PathBuf>) -> Result<App, AppError> {
    let path = db_path.unwrap_or_else(paths::database_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(AppError::storage)?;
    }
    let store = Arc::new(SqliteStore::new(path.clone()));
    store.migrate().await?;

    Ok(App {
        db_path: path,
        sources: store.clone(),
        resources: store.clone(),
        sync: store,
        github: Arc::new(GitHubProvider::from_env()?),
        fake: Arc::new(FakeProvider::from_embedded()),
    })
}
