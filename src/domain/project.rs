use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::error::AppError;

pub const WORKSPACE_DIR: &str = ".github-local-indexer";
pub const CONFIG_FILE: &str = "config.json";
pub const INDEX_FILE: &str = "INDEX.md";
pub const STATUS_FILE: &str = "status.json";
pub const MANIFEST_FILE: &str = "manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub repo_root: String,
    pub database: String,
    pub binary: String,
    pub installed_at: String,
    pub version: String,
}

#[derive(Debug, Clone)]
pub struct ProjectWorkspace {
    pub repo_root: PathBuf,
    pub config: ProjectConfig,
}

impl ProjectWorkspace {
    pub fn dir(&self) -> PathBuf {
        self.repo_root.join(WORKSPACE_DIR)
    }

    pub fn database_path(&self) -> PathBuf {
        resolve_relative(&self.repo_root, &self.config.database)
    }

    pub fn binary_path(&self) -> PathBuf {
        resolve_relative(&self.repo_root, &self.config.binary)
    }

    pub fn index_path(&self) -> PathBuf {
        self.dir().join(INDEX_FILE)
    }

    pub fn status_path(&self) -> PathBuf {
        self.dir().join(STATUS_FILE)
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.dir().join(MANIFEST_FILE)
    }

    pub fn config_path(&self) -> PathBuf {
        self.dir().join(CONFIG_FILE)
    }

    pub fn write_config(&self) -> Result<(), AppError> {
        fs::create_dir_all(self.dir()).map_err(AppError::storage)?;
        let json = serde_json::to_string_pretty(&self.config).map_err(AppError::storage)?;
        fs::write(self.config_path(), json).map_err(AppError::storage)?;
        Ok(())
    }
}

pub fn discover_from(start: &Path) -> Option<ProjectWorkspace> {
    let start = fs::canonicalize(start).ok()?;
    let mut dir = start.as_path();
    loop {
        let config_path = dir.join(WORKSPACE_DIR).join(CONFIG_FILE);
        if config_path.exists() {
            return load_workspace(dir, &config_path).ok();
        }
        dir = dir.parent()?;
    }
}

pub fn discover_from_cwd() -> Option<ProjectWorkspace> {
    discover_from(&std::env::current_dir().ok()?)
}

pub fn new_workspace(repo_root: &Path, binary_rel: &str) -> Result<ProjectWorkspace, AppError> {
    let repo_root = fs::canonicalize(repo_root).map_err(AppError::storage)?;
    Ok(ProjectWorkspace {
        config: ProjectConfig {
            repo_root: repo_root.display().to_string(),
            database: format!("{WORKSPACE_DIR}/index.db"),
            binary: binary_rel.to_string(),
            installed_at: crate::domain::time::now_iso(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        repo_root,
    })
}

fn load_workspace(repo_root: &Path, config_path: &Path) -> Result<ProjectWorkspace, AppError> {
    let raw = fs::read_to_string(config_path).map_err(AppError::storage)?;
    let config: ProjectConfig = serde_json::from_str(&raw).map_err(AppError::storage)?;
    let root = PathBuf::from(&config.repo_root);
    let repo_root = if root.is_absolute() {
        root
    } else {
        repo_root.to_path_buf()
    };
    Ok(ProjectWorkspace { repo_root, config })
}

fn resolve_relative(base: &Path, rel: &str) -> PathBuf {
    let p = PathBuf::from(rel);
    if p.is_absolute() {
        p
    } else {
        base.join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn roundtrip_config() {
        let dir = tempdir().unwrap();
        let ws = new_workspace(dir.path(), ".cursor/bin/github-local-indexer").unwrap();
        ws.write_config().unwrap();
        let found = discover_from(dir.path()).unwrap();
        assert_eq!(found.database_path(), ws.database_path());
    }
}
