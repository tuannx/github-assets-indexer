use async_trait::async_trait;

use crate::domain::error::AppError;
use crate::domain::resource::{ResourceSnapshot, SearchHit, SourceStatus, SyncJob};
use crate::domain::source::Source;

#[async_trait]
pub trait SourceStore: Send + Sync {
    async fn migrate(&self) -> Result<i32, AppError>;
    async fn insert_source(&self, source: &Source) -> Result<(), AppError>;
    async fn list_sources(&self) -> Result<Vec<Source>, AppError>;
    async fn find_source(&self, owner: &str, repo: &str) -> Result<Option<Source>, AppError>;
    async fn find_source_by_id(&self, id: &str) -> Result<Option<Source>, AppError>;
}

#[async_trait]
pub trait ResourceStore: Send + Sync {
    async fn upsert_batch(&self, snapshots: &[ResourceSnapshot]) -> Result<usize, AppError>;
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, AppError>;
    async fn count_by_source(&self, source_id: &str) -> Result<usize, AppError>;
}

#[async_trait]
pub trait SyncStore: Send + Sync {
    async fn enqueue_job(&self, job: &SyncJob) -> Result<(), AppError>;
    async fn claim_next_job(&self) -> Result<Option<SyncJob>, AppError>;
    async fn finish_job(&self, job_id: &str, ok: bool, error: Option<&str>) -> Result<(), AppError>;
    async fn pending_jobs_for_source(&self, source_id: &str) -> Result<usize, AppError>;
    async fn get_checkpoint(&self, source_id: &str, collection: &str) -> Result<Option<String>, AppError>;
    async fn set_checkpoint(&self, source_id: &str, collection: &str, cursor: &str) -> Result<(), AppError>;
    async fn last_success_at(&self, source_id: &str, collection: &str) -> Result<Option<String>, AppError>;
    async fn source_status(&self, source: &Source) -> Result<SourceStatus, AppError>;
}

/// GitHub read adapter: issues, PRs, and all contribution types.
#[async_trait]
pub trait GithubProvider: Send + Sync {
    async fn fetch_issues(
        &self,
        source: &Source,
        since: Option<&str>,
    ) -> Result<Vec<ResourceSnapshot>, AppError>;

    async fn fetch_issue_comments(
        &self,
        source: &Source,
        issue_number: &str,
    ) -> Result<Vec<ResourceSnapshot>, AppError>;

    async fn fetch_pull_requests(
        &self,
        source: &Source,
        since: Option<&str>,
    ) -> Result<Vec<ResourceSnapshot>, AppError>;

    async fn fetch_pull_request_comments(
        &self,
        source: &Source,
        pr_number: &str,
    ) -> Result<Vec<ResourceSnapshot>, AppError>;

    async fn fetch_pull_request_reviews(
        &self,
        source: &Source,
        pr_number: &str,
    ) -> Result<Vec<ResourceSnapshot>, AppError>;

    async fn fetch_pull_request_review_comments(
        &self,
        source: &Source,
        pr_number: &str,
    ) -> Result<Vec<ResourceSnapshot>, AppError>;
}
