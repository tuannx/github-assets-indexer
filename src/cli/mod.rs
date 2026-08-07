use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::application::build_app_for_cli;
use crate::application::install::{self, InstallOptions, InstallScope};
use crate::domain::error::AppError;

#[derive(Parser)]
#[command(name = "github-local-indexer", version, about = "Local-first GitHub indexer")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Install binary and/or Cursor agent skill (defaults to current git repo)
    Install {
        /// Copy binary into repo `.cursor/bin/` (default) or `~/.local/bin` with --global
        #[arg(long)]
        bin: bool,
        /// Install Cursor skill into repo `.cursor/skills/` (default)
        #[arg(long)]
        skill: bool,
        /// Install binary + skill for agent use in the detected repo
        #[arg(long)]
        agent: bool,
        /// Install to ~/.cursor/skills and ~/.local/bin instead of current repo
        #[arg(long)]
        global: bool,
        /// Use existing release binary (skip cargo build)
        #[arg(long)]
        skip_build: bool,
    },
    Doctor,
    Repo {
        #[command(subcommand)]
        cmd: RepoCmd,
    },
    IngestFake {
        source_id: String,
    },
    Search {
        query: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    Sync {
        repo: String,
        #[arg(long)]
        wait: bool,
    },
    Jobs {
        #[command(subcommand)]
        cmd: JobsCmd,
    },
    Status {
        repo: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum RepoCmd {
    Add { repo: String },
    List { #[arg(long)] json: bool },
}

#[derive(Subcommand)]
enum JobsCmd {
    Run,
    List { #[arg(long)] json: bool },
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Install {
            bin,
            skill,
            agent,
            global,
            skip_build,
        } => {
            let use_agent = agent || (!bin && !skill);
            let scope = if global {
                InstallScope::Global
            } else {
                InstallScope::Project
            };
            let opts = InstallOptions {
                bin: use_agent || bin,
                skill: use_agent || skill,
                agent: use_agent,
                scope,
                skip_build,
                repo_root: None,
            };
            run_install(opts).await?;
        }
        other => {
            let app = build_app_for_cli(None).await.map_err(to_anyhow)?;
            dispatch(other, &app).await?;
        }
    }
    Ok(())
}

async fn run_install(opts: InstallOptions) -> Result<()> {
    let report = install::run_sync(opts)?;
    install::finalize_install(&report)
        .await
        .map_err(to_anyhow)?;

    println!("repo:      {}", report.repo_root);
    if let Some(p) = &report.workspace_dir {
        println!("workspace: {p}");
    }
    if let Some(p) = &report.index_file {
        println!("index:     {p}  ← agents read this first");
    }
    if let Some(p) = &report.status_file {
        println!("status:    {p}");
    }
    if let Some(p) = &report.binary_path {
        println!("binary:    {p}");
    }
    if let Some(p) = &report.skill_path {
        println!("skill:     {p}");
    }
    if let Some(p) = &report.rule_path {
        println!("rule:      {p}");
    }
    if let Some(hint) = &report.path_hint {
        println!("{hint}");
    }
    println!("invoke:    {}", report.cli_invocation);
    println!();
    println!("Agent ready — read `.github-local-indexer/INDEX.md` before remote GitHub lookup.");
    Ok(())
}

async fn dispatch(command: Commands, app: &crate::application::App) -> Result<()> {
    match command {
        Commands::Install { .. } => unreachable!(),
        Commands::Doctor => {
            let r = app.doctor().await.map_err(to_anyhow)?;
            app.refresh_index_artifacts().await.map_err(to_anyhow)?;
            println!("github-local-indexer {}", r.version);
            println!("Schema version:   {}", r.schema_version);
            println!("Data directory:   {}", r.data_dir);
            println!("Database path:    {}", r.database_path);
            if let Some(ws) = crate::domain::project::discover_from_cwd() {
                println!("Index snapshot:   {}", ws.index_path().display());
                if ws.config.version != r.version {
                    eprintln!(
                        "Warning: workspace installed with v{}; binary is v{}. Re-run `install` to refresh.",
                        ws.config.version, r.version
                    );
                }
            }
        }
        Commands::Repo { cmd } => match cmd {
            RepoCmd::Add { repo } => {
                let s = app.add_repo(&repo).await.map_err(to_anyhow)?;
                println!("added {} ({})", s.slug(), s.id.as_str());
            }
            RepoCmd::List { json } => {
                let sources = app.list_repos().await.map_err(to_anyhow)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&sources)?);
                } else {
                    for s in sources {
                        println!("{}  {}", s.slug(), s.id.as_str());
                    }
                }
            }
        },
        Commands::IngestFake { source_id } => {
            let n = app.ingest_fake(&source_id).await.map_err(to_anyhow)?;
            println!("ingested {n} resources from fixture");
        }
        Commands::Search { query, limit, json } => {
            let hits = app.search(&query, limit).await.map_err(to_anyhow)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&hits)?);
            } else {
                for h in hits {
                    print_hit(&h);
                }
            }
        }
        Commands::Sync { repo, wait } => {
            if wait {
                let r = app.sync_wait(&repo).await.map_err(to_anyhow)?;
                print_sync_report(&r);
            } else {
                let job = app.enqueue_sync(&repo).await.map_err(to_anyhow)?;
                println!("enqueued job {} for {}", job.id, repo);
            }
        }
        Commands::Jobs { cmd } => match cmd {
            JobsCmd::Run => {
                if let Some(r) = app.run_next_job().await.map_err(to_anyhow)? {
                    print_sync_report(&r);
                } else {
                    println!("no pending jobs");
                }
            }
            JobsCmd::List { json } => {
                let statuses = app.status(None).await.map_err(to_anyhow)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&statuses)?);
                } else {
                    for s in statuses {
                        println!(
                            "{}  {:?}  resources={} pending_jobs={}",
                            s.slug, s.freshness, s.resource_count, s.pending_jobs
                        );
                    }
                }
            }
        },
        Commands::Status { repo, json } => {
            app.refresh_index_artifacts().await.map_err(to_anyhow)?;
            let statuses = app.status(repo.as_deref()).await.map_err(to_anyhow)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&statuses)?);
            } else {
                for s in statuses {
                    println!(
                        "{}  {:?}  last_success={:?} resources={}",
                        s.slug, s.freshness, s.last_success_at, s.resource_count
                    );
                }
            }
        },
    }
    Ok(())
}

fn print_sync_report(r: &crate::application::services::SyncReport) {
    let c = &r.counts;
    println!(
        "synced {} at {}\n  issues={} issue_comments={}\n  pull_requests={} pr_comments={} pr_reviews={} pr_review_comments={}",
        r.slug,
        r.synced_at,
        c.issues,
        c.issue_comments,
        c.pull_requests,
        c.pull_request_comments,
        c.pull_request_reviews,
        c.pull_request_review_comments,
    );
}

fn print_hit(h: &crate::domain::resource::SearchHit) {
    let title = h.title.as_deref().unwrap_or("(no title)");
    if let Some(pt) = &h.parent_title {
        println!(
            "[{}] {} (on {})\n  {}",
            h.resource_type.as_str(),
            title,
            pt,
            h.canonical_url
        );
    } else {
        println!(
            "[{}] {}\n  {}",
            h.resource_type.as_str(),
            title,
            h.canonical_url
        );
    }
}

fn to_anyhow(err: AppError) -> anyhow::Error {
    anyhow::anyhow!(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses() {
        Cli::try_parse_from(["gli", "doctor"]).unwrap();
        Cli::try_parse_from(["gli", "repo", "add", "acme/demo"]).unwrap();
        Cli::try_parse_from(["gli", "search", "login"]).unwrap();
        Cli::try_parse_from(["gli", "install", "--agent"]).unwrap();
        Cli::try_parse_from(["gli", "install", "--global"]).unwrap();
    }
}
