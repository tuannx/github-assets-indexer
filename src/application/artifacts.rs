use std::fs;

use serde::Serialize;

use crate::application::services::App;
use crate::domain::error::AppError;
use crate::domain::freshness::FRESHNESS_TTL_MINUTES;
use crate::domain::project::{ProjectWorkspace, STATUS_FILE};
use crate::domain::resource::SourceStatus;
use crate::domain::time::now_iso;

#[derive(Debug, Serialize)]
pub struct StatusSnapshot {
    pub generated_at: String,
    pub cli_version: String,
    pub schema_version: i32,
    pub database_path: String,
    pub stale_ttl_minutes: i64,
    pub sources: Vec<SourceStatus>,
}

#[derive(Debug, Serialize)]
pub struct InstallManifest {
    pub installed_at: String,
    pub cli_version: String,
    pub repo_root: String,
    pub workspace: String,
    pub database: String,
    pub binary: String,
    pub index_file: String,
    pub status_file: String,
    pub skill_file: String,
    pub agent_rule: String,
}

pub async fn refresh_workspace(app: &App, workspace: &ProjectWorkspace) -> Result<(), AppError> {
    let doctor = app.doctor().await?;
    let sources = app.status(None).await?;
    let snapshot = StatusSnapshot {
        generated_at: now_iso(),
        cli_version: doctor.version.clone(),
        schema_version: doctor.schema_version,
        database_path: workspace.database_path().display().to_string(),
        stale_ttl_minutes: FRESHNESS_TTL_MINUTES,
        sources,
    };

    fs::create_dir_all(workspace.dir()).map_err(AppError::storage)?;
    let status_json = serde_json::to_string_pretty(&snapshot).map_err(AppError::storage)?;
    fs::write(workspace.status_path(), status_json).map_err(AppError::storage)?;
    fs::write(workspace.index_path(), render_index_md(&snapshot, workspace))
        .map_err(AppError::storage)?;
    Ok(())
}

pub fn write_manifest(
    workspace: &ProjectWorkspace,
    skill_path: &str,
    rule_path: &str,
) -> Result<(), AppError> {
    let manifest = InstallManifest {
        installed_at: workspace.config.installed_at.clone(),
        cli_version: workspace.config.version.clone(),
        repo_root: workspace.repo_root.display().to_string(),
        workspace: workspace.dir().display().to_string(),
        database: workspace.database_path().display().to_string(),
        binary: workspace.binary_path().display().to_string(),
        index_file: workspace.index_path().display().to_string(),
        status_file: workspace.status_path().display().to_string(),
        skill_file: skill_path.to_string(),
        agent_rule: rule_path.to_string(),
    };
    let json = serde_json::to_string_pretty(&manifest).map_err(AppError::storage)?;
    fs::write(workspace.manifest_path(), json).map_err(AppError::storage)?;
    Ok(())
}

fn render_index_md(snapshot: &StatusSnapshot, workspace: &ProjectWorkspace) -> String {
    let mut md = String::new();
    md.push_str("# GitHub Local Index\n\n");
    md.push_str(
        "> **Agent priority read.** Check `last_indexed_at`, `index_age_minutes`, and \
         `refresh_recommended` in this file or `status.json` before searching or hitting GitHub remote.\n\n",
    );
    md.push_str(&format!("- **Snapshot generated:** {}\n", snapshot.generated_at));
    md.push_str(&format!("- **CLI version:** {}\n", snapshot.cli_version));
    md.push_str(&format!("- **Schema:** v{}\n", snapshot.schema_version));
    md.push_str(&format!(
        "- **Stale after:** {} minutes (`stale_ttl_minutes`)\n",
        snapshot.stale_ttl_minutes
    ));
    md.push_str(&format!(
        "- **Database:** `{}`\n",
        workspace.config.database
    ));
    md.push_str(&format!(
        "- **Status JSON:** `.github-local-indexer/{STATUS_FILE}`\n\n"
    ));

    md.push_str("## Refresh policy (for agents)\n\n");
    md.push_str("| Signal | Action |\n");
    md.push_str("|---|---|\n");
    md.push_str("| `refresh_recommended: true` or `freshness: stale` | Run `sync owner/repo --wait` |\n");
    md.push_str("| `freshness: never_synced` | Run `repo add` then `sync --wait` |\n");
    md.push_str("| `index_age_minutes` > stale TTL | Treat as stale; refresh cache |\n");
    md.push_str("| `freshness: fresh` | Search locally; no sync needed |\n\n");

    md.push_str("## Indexed sources\n\n");
    if snapshot.sources.is_empty() {
        md.push_str("_No repositories indexed yet._ Run:\n\n");
        md.push_str("```bash\n");
        md.push_str(&format!(
            "{} repo add owner/name\n",
            workspace.config.binary
        ));
        md.push_str(&format!(
            "{} sync owner/name --wait\n",
            workspace.config.binary
        ));
        md.push_str("```\n\n");
    } else {
        md.push_str(
            "| Repository | Freshness | Last indexed | Age (min) | Refresh? | Resources |\n",
        );
        md.push_str("|---|---|---|---:|---:|---:|\n");
        for s in &snapshot.sources {
            md.push_str(&format!(
                "| {} | {:?} | {} | {} | {} | {} |\n",
                s.slug,
                s.freshness,
                format_time(s.last_indexed_at.as_deref()),
                format_age(s.index_age_minutes),
                yes_no(s.refresh_recommended),
                s.resource_count,
            ));
        }
        md.push('\n');
        for s in &snapshot.sources {
            if s.collections.is_empty() {
                continue;
            }
            md.push_str(&format!("### Collections — {}\n\n", s.slug));
            md.push_str(
                "| Collection | Freshness | Last indexed | Age (min) | Refresh? |\n",
            );
            md.push_str("|---|---|---|---:|---:|\n");
            for c in &s.collections {
                md.push_str(&format!(
                    "| {} | {:?} | {} | {} | {} |\n",
                    c.collection,
                    c.freshness,
                    format_time(c.last_indexed_at.as_deref()),
                    format_age(c.index_age_minutes),
                    yes_no(c.refresh_recommended),
                ));
            }
            md.push('\n');
        }
    }

    md.push_str("## Agent commands\n\n");
    md.push_str("| Goal | Command |\n");
    md.push_str("|---|---|\n");
    md.push_str(&format!(
        "| Check index age / refresh hint | `{} status --json` |\n",
        workspace.config.binary
    ));
    md.push_str(&format!(
        "| Refresh INDEX snapshot only | `{} status` or `{} doctor` |\n",
        workspace.config.binary, workspace.config.binary
    ));
    md.push_str(&format!(
        "| Pull GitHub → local cache | `{} sync owner/repo --wait` |\n",
        workspace.config.binary
    ));
    md.push_str(&format!(
        "| Enqueue background sync | `{} sync owner/repo` then `{} jobs run` |\n",
        workspace.config.binary, workspace.config.binary
    ));
    md.push_str(&format!(
        "| Add repository | `{} repo add owner/repo` |\n",
        workspace.config.binary
    ));
    md.push_str(&format!(
        "| Search local cache | `{} search \"<query>\" --json` |\n\n",
        workspace.config.binary
    ));

    md.push_str("## Searchable resource types\n\n");
    md.push_str(
        "`issue`, `issue_comment`, `pull_request`, `pull_request_comment`, \
         `pull_request_review`, `pull_request_review_comment`\n",
    );

    md
}

fn format_time(value: Option<&str>) -> String {
    value.unwrap_or("—").to_string()
}

fn format_age(minutes: Option<i64>) -> String {
    minutes
        .map(|m| m.to_string())
        .unwrap_or_else(|| "—".to_string())
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

pub async fn refresh_if_project(app: &App) -> Result<(), AppError> {
    if let Some(ws) = crate::domain::project::discover_from_cwd() {
        refresh_workspace(app, &ws).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::resource::Freshness;

    #[test]
    fn policy_table_mentions_refresh_recommended() {
        let snapshot = StatusSnapshot {
            generated_at: "2026-01-01T00:00:00Z".into(),
            cli_version: "0.1.0".into(),
            schema_version: 3,
            database_path: "/tmp/index.db".into(),
            stale_ttl_minutes: FRESHNESS_TTL_MINUTES,
            sources: vec![SourceStatus {
                slug: "acme/demo".into(),
                freshness: Freshness::Fresh,
                last_success_at: Some("2026-01-01T00:00:00Z".into()),
                last_indexed_at: Some("2026-01-01T00:00:00Z".into()),
                index_age_minutes: Some(5),
                refresh_recommended: false,
                resource_count: 10,
                pending_jobs: 0,
                collections: vec![],
            }],
        };
        let ws = crate::domain::project::ProjectWorkspace {
            repo_root: "/tmp".into(),
            config: crate::domain::project::ProjectConfig {
                repo_root: "/tmp".into(),
                database: ".github-local-indexer/index.db".into(),
                binary: ".cursor/bin/github-local-indexer".into(),
                installed_at: "2026-01-01".into(),
                version: "0.1.0".into(),
            },
        };
        let md = render_index_md(&snapshot, &ws);
        assert!(md.contains("last_indexed_at"));
        assert!(md.contains("Refresh policy"));
        assert!(md.contains("sync owner/repo --wait"));
    }
}
