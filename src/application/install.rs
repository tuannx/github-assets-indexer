use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::application::bootstrap::build_app;
use crate::domain::project::{self, WORKSPACE_DIR};

const SKILL_NAME: &str = "github-local-indexer";
const BIN_NAME: &str = "github-local-indexer";
const BIN_REL: &str = ".cursor/bin/github-local-indexer";
const RULE_REL: &str = ".cursor/rules/github-local-indexer.mdc";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallScope {
    Project,
    Global,
}

#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub bin: bool,
    pub skill: bool,
    pub agent: bool,
    pub scope: InstallScope,
    pub skip_build: bool,
    /// Override repo root (tests only).
    pub repo_root: Option<PathBuf>,
}

impl InstallOptions {
    pub fn resolve(self) -> Self {
        if self.agent {
            Self {
                bin: true,
                skill: true,
                agent: true,
                scope: self.scope,
                skip_build: self.skip_build,
                repo_root: self.repo_root,
            }
        } else {
            self
        }
    }
}

#[derive(Debug, Serialize)]
pub struct InstallReport {
    pub repo_root: String,
    pub workspace_dir: Option<String>,
    pub index_file: Option<String>,
    pub status_file: Option<String>,
    pub binary_path: Option<String>,
    pub skill_path: Option<String>,
    pub rule_path: Option<String>,
    pub cli_invocation: String,
    pub path_hint: Option<String>,
}

pub fn run_sync(opts: InstallOptions) -> Result<InstallReport> {
    let opts = opts.resolve();
    if !opts.bin && !opts.skill {
        bail!("nothing to install; use --bin, --skill, or --agent");
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = opts
        .repo_root
        .clone()
        .unwrap_or_else(|| std::env::current_dir().expect("current dir"));
    let repo_root = detect_repo_root(repo_root).context("detect git repo root")?;

    let mut binary_path = None;
    let mut skill_path = None;
    let mut rule_path = None;
    let mut path_hint = None;
    let mut workspace_dir = None;
    let mut index_file = None;
    let mut status_file = None;

    let workspace = if opts.scope == InstallScope::Project {
        let ws = project::new_workspace(&repo_root, BIN_REL)?;
        ws.write_config().map_err(to_anyhow)?;
        ensure_gitignore(&repo_root)?;
        workspace_dir = Some(ws.dir().display().to_string());
        index_file = Some(ws.index_path().display().to_string());
        status_file = Some(ws.status_path().display().to_string());
        Some(ws)
    } else {
        None
    };

    if opts.bin {
        let dest = match opts.scope {
            InstallScope::Project => install_binary_project(&manifest, &repo_root, opts.skip_build)?,
            InstallScope::Global => install_binary_global(&manifest, opts.skip_build)?,
        };
        if opts.scope == InstallScope::Global {
            path_hint = Some(format!(
                "Add to shell profile: export PATH=\"{}:$PATH\"",
                local_bin_dir().display()
            ));
        }
        binary_path = Some(dest.display().to_string());
    }

    if opts.skill {
        let bin_for_skill = binary_path
            .as_deref()
            .map(PathBuf::from)
            .or_else(|| default_bin_path(&repo_root, opts.scope).ok());
        let skill = install_skill(&manifest, &repo_root, opts.scope, bin_for_skill.as_deref())?;
        skill_path = Some(skill.display().to_string());

        if opts.scope == InstallScope::Project {
            let rule = install_agent_rule(&manifest, &repo_root)?;
            rule_path = Some(rule.display().to_string());
            if let (Some(ws), Some(skill), Some(rule)) =
                (workspace.as_ref(), skill_path.as_ref(), rule_path.as_ref())
            {
                crate::application::artifacts::write_manifest(ws, skill, rule)
                    .map_err(to_anyhow)?;
            }
        }
    }

    let cli_invocation = cli_invocation(&repo_root, opts.scope, binary_path.as_deref());

    Ok(InstallReport {
        repo_root: repo_root.display().to_string(),
        workspace_dir,
        index_file,
        status_file,
        binary_path,
        skill_path,
        rule_path,
        cli_invocation,
        path_hint,
    })
}

pub async fn finalize_install(report: &InstallReport) -> Result<(), AppError> {
    let db = report
        .workspace_dir
        .as_ref()
        .map(|w| PathBuf::from(w).join("index.db"));
    let app = build_app(db).await?;
    app.doctor().await?;
    app.refresh_index_artifacts().await?;
    Ok(())
}

use crate::domain::error::AppError;

fn to_anyhow(err: AppError) -> anyhow::Error {
    anyhow::anyhow!(err)
}

/// Walk up from `start` to find a directory containing `.git`.
pub fn detect_repo_root(start: PathBuf) -> Result<PathBuf> {
    let start = fs::canonicalize(&start)
        .with_context(|| format!("resolve path {}", start.display()))?;
    let mut dir = start.as_path();
    loop {
        if dir.join(".git").exists() {
            return Ok(dir.to_path_buf());
        }
        let Some(parent) = dir.parent() else {
            break;
        };
        dir = parent;
    }
    Ok(start)
}

fn ensure_gitignore(repo_root: &Path) -> Result<()> {
    let path = repo_root.join(".gitignore");
    let existing = fs::read_to_string(&path).unwrap_or_default();
    if existing.lines().any(|l| l.contains("github-local-indexer")) {
        return Ok(());
    }
    let block = format!(
        "\n# github-local-indexer (local index DB; keep INDEX.md + status.json)\n{WORKSPACE_DIR}/index.db\n{WORKSPACE_DIR}/index.db-*\n"
    );
    fs::write(path, format!("{existing}{block}")).context("update .gitignore")?;
    Ok(())
}

fn install_agent_rule(manifest: &Path, repo_root: &Path) -> Result<PathBuf> {
    let src = manifest.join(format!("skills/{SKILL_NAME}/RULE.mdc"));
    if !src.exists() {
        bail!("agent rule template missing at {}", src.display());
    }
    let dest_dir = repo_root.join(".cursor/rules");
    fs::create_dir_all(&dest_dir)?;
    let dest = repo_root.join(RULE_REL);
    fs::copy(&src, &dest).context("copy agent rule")?;
    Ok(dest)
}

fn install_binary_project(
    manifest: &Path,
    repo_root: &Path,
    skip_build: bool,
) -> Result<PathBuf> {
    let src = build_release_binary(manifest, skip_build)?;
    let dest_dir = repo_root.join(".cursor/bin");
    fs::create_dir_all(&dest_dir)?;
    let dest = dest_dir.join(BIN_NAME);
    fs::copy(&src, &dest).with_context(|| format!("copy to {}", dest.display()))?;
    set_executable(&dest)?;
    Ok(dest)
}

fn install_binary_global(manifest: &Path, skip_build: bool) -> Result<PathBuf> {
    let src = build_release_binary(manifest, skip_build)?;
    let dest_dir = local_bin_dir();
    fs::create_dir_all(&dest_dir)?;
    let dest = dest_dir.join(BIN_NAME);
    fs::copy(&src, &dest).with_context(|| format!("copy to {}", dest.display()))?;
    set_executable(&dest)?;
    Ok(dest)
}

fn build_release_binary(manifest: &Path, skip_build: bool) -> Result<PathBuf> {
    if !skip_build {
        let status = Command::new("cargo")
            .args(["build", "--release", "--quiet"])
            .current_dir(manifest)
            .status()
            .context("failed to run cargo build")?;
        if !status.success() {
            bail!("cargo build --release failed");
        }
    }

    let default_src = manifest.join(format!("target/release/{BIN_NAME}"));
    if default_src.exists() {
        return Ok(default_src);
    }

    if let Ok(target_dir) = std::env::var("CARGO_TARGET_DIR") {
        let alt = PathBuf::from(target_dir)
            .join("release")
            .join(BIN_NAME);
        if alt.exists() {
            return Ok(alt);
        }
    }

    let current = std::env::current_exe().context("resolve running binary")?;
    if current.file_name().is_some_and(|n| n == BIN_NAME) && current.exists() {
        return Ok(current);
    }

    bail!(
        "binary not found at {}; run without --skip-build",
        default_src.display()
    );
}

fn install_skill(
    manifest: &Path,
    repo_root: &Path,
    scope: InstallScope,
    binary: Option<&Path>,
) -> Result<PathBuf> {
    let src = manifest.join(format!("skills/{SKILL_NAME}"));
    if !src.join("SKILL.md").exists() {
        bail!("skill template missing at {}", src.display());
    }

    let dest = match scope {
        InstallScope::Project => repo_root.join(format!(".cursor/skills/{SKILL_NAME}")),
        InstallScope::Global => home_dir()?.join(format!(".cursor/skills/{SKILL_NAME}")),
    };

    copy_dir_recursive(&src, &dest)?;

    if let Some(bin) = binary {
        let rel = relative_path(repo_root, bin).unwrap_or_else(|_| bin.to_path_buf());
        fs::write(dest.join("BIN"), rel.display().to_string())?;
        fs::write(dest.join("REPO_ROOT"), repo_root.display().to_string())?;
    }

    Ok(dest)
}

fn default_bin_path(repo_root: &Path, scope: InstallScope) -> Result<PathBuf> {
    match scope {
        InstallScope::Project => Ok(repo_root.join(BIN_REL)),
        InstallScope::Global => Ok(local_bin_dir().join(BIN_NAME)),
    }
}

fn cli_invocation(repo_root: &Path, scope: InstallScope, binary: Option<&str>) -> String {
    match scope {
        InstallScope::Project => {
            let bin = binary.unwrap_or(BIN_REL);
            format!(
                "cd {} && cat .github-local-indexer/INDEX.md && {} search \"<query>\" --json",
                repo_root.display(),
                bin
            )
        }
        InstallScope::Global => {
            "github-local-indexer search \"<query>\" --json".to_string()
        }
    }
}

fn relative_path(base: &Path, target: &Path) -> Result<PathBuf> {
    let base = fs::canonicalize(base)?;
    let target = fs::canonicalize(target)?;
    Ok(target
        .strip_prefix(&base)
        .map(Path::to_path_buf)
        .unwrap_or(target))
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let name = entry.file_name();
        if name == "RULE.mdc" {
            continue;
        }
        let to = dest.join(name);
        if ty.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn local_bin_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/bin")
}

fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().context("home directory not found")
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::tempdir;

    #[test]
    fn copies_skill_template() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let out = tempdir().unwrap();
        let dest = out.path().join("skill");
        copy_dir_recursive(&manifest.join("skills/github-local-indexer"), &dest).unwrap();
        assert!(dest.join("SKILL.md").exists());
        assert!(!dest.join("RULE.mdc").exists());
    }

    #[test]
    fn agent_preset_enables_bin_and_skill() {
        let opts = InstallOptions {
            bin: false,
            skill: false,
            agent: true,
            scope: InstallScope::Project,
            skip_build: true,
            repo_root: None,
        }
        .resolve();
        assert!(opts.bin && opts.skill);
    }

    #[test]
    fn detect_finds_git_root_from_subdir() {
        let dir = tempdir().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let nested = dir.path().join("src/deep");
        fs::create_dir_all(&nested).unwrap();
        let root = detect_repo_root(nested).unwrap();
        assert_eq!(root, fs::canonicalize(dir.path()).unwrap());
    }
}
