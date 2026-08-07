mod orchestrator;

use std::sync::Arc;

pub use orchestrator::SyncReport;
use orchestrator::sync_source;

use crate::application::ports::{GithubProvider, ResourceStore, SourceStore, SyncStore};
use crate::domain::error::AppError;
use crate::domain::resource::{JobState, SearchHit, SourceStatus, SyncJob};
use crate::domain::source::{parse_repo_slug, Source};

pub struct App {
    pub db_path: std::path::PathBuf,
    pub sources: Arc<dyn SourceStore>,
    pub resources: Arc<dyn ResourceStore>,
    pub sync: Arc<dyn SyncStore>,
    pub github: Arc<dyn GithubProvider>,
    pub fake: Arc<dyn GithubProvider>,
}

impl App {
    pub async fn doctor(&self) -> Result<DoctorReport, AppError> {
        let version = self.sources.migrate().await?;
        Ok(DoctorReport {
            version: env!("CARGO_PKG_VERSION").to_string(),
            schema_version: version,
            data_dir: self
                .db_path
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            database_path: self.db_path.display().to_string(),
        })
    }

    pub async fn add_repo(&self, input: &str) -> Result<Source, AppError> {
        let slug = parse_repo_slug(input).map_err(AppError::InvalidInput)?;
        if self
            .sources
            .find_source(&slug.owner, &slug.repository)
            .await?
            .is_some()
        {
            return Err(AppError::AlreadyExists(format!(
                "{}/{}",
                slug.owner, slug.repository
            )));
        }
        let source = Source::new(slug.owner, slug.repository);
        self.sources.insert_source(&source).await?;
        self.refresh_index_artifacts().await?;
        Ok(source)
    }

    pub async fn list_repos(&self) -> Result<Vec<Source>, AppError> {
        self.sources.list_sources().await
    }

    pub async fn ingest_fake(&self, source_id: &str) -> Result<usize, AppError> {
        let source = self.require_source(source_id).await?;
        let report = sync_source(
            &source,
            self.fake.as_ref(),
            self.resources.as_ref(),
            self.sync.as_ref(),
        )
        .await?;
        self.refresh_index_artifacts().await?;
        Ok(report.counts.total())
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, AppError> {
        if query.trim().is_empty() {
            return Err(AppError::InvalidInput("query cannot be empty".into()));
        }
        self.resources.search(query, limit).await
    }

    pub async fn enqueue_sync(&self, input: &str) -> Result<SyncJob, AppError> {
        let source = self.require_source_by_slug(input).await?;
        let job = SyncJob {
            id: uuid::Uuid::new_v4().to_string(),
            source_id: source.id.clone(),
            collection: "full".into(),
            kind: "incremental".into(),
            state: JobState::Pending,
        };
        self.sync.enqueue_job(&job).await?;
        Ok(job)
    }

    pub async fn sync_wait(&self, input: &str) -> Result<SyncReport, AppError> {
        let source = self.require_source_by_slug(input).await?;
        let report = sync_source(
            &source,
            self.github.as_ref(),
            self.resources.as_ref(),
            self.sync.as_ref(),
        )
        .await?;
        self.refresh_index_artifacts().await?;
        Ok(report)
    }

    pub async fn run_next_job(&self) -> Result<Option<SyncReport>, AppError> {
        let Some(job) = self.sync.claim_next_job().await? else {
            return Ok(None);
        };
        let source = self
            .sources
            .find_source_by_id(job.source_id.as_str())
            .await?
            .ok_or_else(|| AppError::NotFound(job.source_id.as_str().to_string()))?;
        match sync_source(
            &source,
            self.github.as_ref(),
            self.resources.as_ref(),
            self.sync.as_ref(),
        )
        .await
        {
            Ok(r) => {
                self.sync.finish_job(&job.id, true, None).await?;
                self.refresh_index_artifacts().await?;
                Ok(Some(r))
            }
            Err(e) => {
                let msg = e.to_string();
                self.sync.finish_job(&job.id, false, Some(&msg)).await?;
                Err(e)
            }
        }
    }

    pub async fn status(&self, input: Option<&str>) -> Result<Vec<SourceStatus>, AppError> {
        let sources = match input {
            Some(slug) => vec![self.require_source_by_slug(slug).await?],
            None => self.sources.list_sources().await?,
        };
        let mut out = Vec::with_capacity(sources.len());
        for source in sources {
            out.push(self.sync.source_status(&source).await?);
        }
        Ok(out)
    }

    pub async fn refresh_index_artifacts(&self) -> Result<(), AppError> {
        crate::application::artifacts::refresh_if_project(self).await
    }

    async fn require_source(&self, id: &str) -> Result<Source, AppError> {
        self.sources
            .find_source_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound(id.to_string()))
    }

    async fn require_source_by_slug(&self, input: &str) -> Result<Source, AppError> {
        let slug = parse_repo_slug(input).map_err(AppError::InvalidInput)?;
        self.sources
            .find_source(&slug.owner, &slug.repository)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("{}/{}", slug.owner, slug.repository)))
    }
}

#[derive(Debug, Clone)]
pub struct DoctorReport {
    pub version: String,
    pub schema_version: i32,
    pub data_dir: String,
    pub database_path: String,
}
