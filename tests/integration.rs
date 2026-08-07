use std::fs;
use std::path::Path;
use std::process::Command;

use github_local_indexer::application::artifacts::refresh_workspace;
use github_local_indexer::application::build_app;
use github_local_indexer::application::install::{self, InstallOptions, InstallScope};
use github_local_indexer::domain::project::{self, WORKSPACE_DIR};
use tempfile::tempdir;

#[tokio::test]
async fn fake_ingest_and_search_flow() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("index.db");
    let app = build_app(Some(db)).await.unwrap();

    let source = app.add_repo("acme/demo").await.unwrap();
    app.ingest_fake(source.id.as_str()).await.unwrap();

    let hits = app.search("OAuth on iOS", 10).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].title.as_ref().unwrap().contains("login"));

    let comment_hits = app.search("reproduces", 10).await.unwrap();
    assert_eq!(comment_hits.len(), 1);
    assert!(comment_hits[0].parent_title.is_some());

    let review_hits = app.search("error handling", 10).await.unwrap();
    assert!(review_hits
        .iter()
        .any(|h| h.resource_type == github_local_indexer::domain::resource::ResourceType::PullRequestReview));

    let inline_hits = app.search("unwrap", 10).await.unwrap();
    assert!(inline_hits
        .iter()
        .any(|h| h.resource_type == github_local_indexer::domain::resource::ResourceType::PullRequestReviewComment));
}

#[tokio::test]
async fn job_queue_roundtrip() {
    let dir = tempdir().unwrap();
    let app = build_app(Some(dir.path().join("index.db"))).await.unwrap();
    app.add_repo("acme/demo").await.unwrap();

    let job = app.enqueue_sync("acme/demo").await.unwrap();
    assert!(!job.id.is_empty());
    assert_eq!(job.collection, "full");

    let statuses = app.status(None).await.unwrap();
    assert_eq!(statuses[0].pending_jobs, 1);
    assert_eq!(statuses[0].collections.len(), 2);
}

#[tokio::test]
async fn project_workspace_index_reflects_sources() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());

    let ws = project::new_workspace(dir.path(), ".cursor/bin/github-local-indexer").unwrap();
    ws.write_config().unwrap();

    let db = ws.database_path();
    let app = build_app(Some(db)).await.unwrap();
    let source = app.add_repo("acme/demo").await.unwrap();
    app.ingest_fake(source.id.as_str()).await.unwrap();
    refresh_workspace(&app, &ws).await.unwrap();

    let index = fs::read_to_string(ws.index_path()).unwrap();
    assert!(index.contains("acme/demo"));
    assert!(index.contains("Agent priority read"));

    let status: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(ws.status_path()).unwrap()).unwrap();
    let source = &status["sources"][0];
    assert_eq!(source["slug"], "acme/demo");
    assert!(source["resource_count"].as_u64().unwrap() > 0);
    assert_ne!(source["freshness"], "never_synced");
    assert!(source["last_indexed_at"].is_string());
    assert!(source["refresh_recommended"].is_boolean());
    assert_eq!(status["stale_ttl_minutes"], 15);
}

#[tokio::test]
async fn incremental_ingest_updates_fewer_resources_on_second_run() {
    let dir = tempdir().unwrap();
    let app = build_app(Some(dir.path().join("index.db"))).await.unwrap();
    let source = app.add_repo("acme/demo").await.unwrap();

    let first = app.ingest_fake(source.id.as_str()).await.unwrap();
    let second = app.ingest_fake(source.id.as_str()).await.unwrap();

    assert!(first > second);
    assert!(second > 0);
}

#[tokio::test]
async fn install_scaffold_writes_agent_files() {
    let dir = tempdir().unwrap();
    init_git_repo(dir.path());

    let report = install::run_sync(InstallOptions {
        bin: false,
        skill: true,
        agent: false,
        scope: InstallScope::Project,
        skip_build: true,
        repo_root: Some(dir.path().to_path_buf()),
    })
    .unwrap();

    assert!(dir.path().join(".cursor/skills/github-local-indexer/SKILL.md").exists());
    assert!(dir.path().join(".cursor/rules/github-local-indexer.mdc").exists());
    assert!(dir.path().join(WORKSPACE_DIR).join("config.json").exists());
    assert!(report.index_file.is_some());
}

fn init_git_repo(path: &Path) {
    Command::new("git")
        .args(["init", "-q"])
        .current_dir(path)
        .output()
        .expect("git init");
}
