use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::source::SourceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    Issue,
    IssueComment,
    PullRequest,
    PullRequestComment,
    PullRequestReview,
    PullRequestReviewComment,
}

impl ResourceType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Issue => "issue",
            Self::IssueComment => "issue_comment",
            Self::PullRequest => "pull_request",
            Self::PullRequestComment => "pull_request_comment",
            Self::PullRequestReview => "pull_request_review",
            Self::PullRequestReviewComment => "pull_request_review_comment",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "issue" => Some(Self::Issue),
            "issue_comment" => Some(Self::IssueComment),
            "pull_request" => Some(Self::PullRequest),
            "pull_request_comment" => Some(Self::PullRequestComment),
            "pull_request_review" => Some(Self::PullRequestReview),
            "pull_request_review_comment" => Some(Self::PullRequestReviewComment),
            _ => None,
        }
    }

    pub fn default_parent(self) -> Option<ResourceType> {
        match self {
            Self::IssueComment => Some(Self::Issue),
            Self::PullRequestComment
            | Self::PullRequestReview
            | Self::PullRequestReviewComment => Some(Self::PullRequest),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSnapshot {
    pub source_id: SourceId,
    pub resource_type: ResourceType,
    pub remote_id: String,
    pub parent_remote_id: Option<String>,
    pub parent_resource_type: Option<ResourceType>,
    pub canonical_url: String,
    pub title: Option<String>,
    pub body: Option<String>,
    pub remote_updated_at: Option<String>,
}

impl ResourceSnapshot {
    pub fn stable_id(&self) -> String {
        format!(
            "{}:{}:{}",
            self.source_id.as_str(),
            self.resource_type.as_str(),
            self.remote_id
        )
    }

    pub fn parent_type(&self) -> Option<ResourceType> {
        self.parent_resource_type
            .or_else(|| self.resource_type.default_parent())
    }

    pub fn parent_stable_id(&self) -> Option<String> {
        let parent_type = self.parent_type()?;
        let parent_remote = self.parent_remote_id.as_ref()?;
        Some(format!(
            "{}:{}:{}",
            self.source_id.as_str(),
            parent_type.as_str(),
            parent_remote
        ))
    }

    pub fn content_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.title.as_deref().unwrap_or(""));
        hasher.update(b"\0");
        hasher.update(self.body.as_deref().unwrap_or(""));
        hasher.update(b"\0");
        hasher.update(&self.remote_id);
        format!("{:x}", hasher.finalize())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub resource_id: String,
    pub resource_type: ResourceType,
    pub source_slug: String,
    pub title: Option<String>,
    pub body_excerpt: Option<String>,
    pub canonical_url: String,
    pub remote_updated_at: Option<String>,
    pub local_synced_at: String,
    pub parent_title: Option<String>,
    pub parent_url: Option<String>,
    pub rank: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Pending,
    Running,
    Succeeded,
    Failed,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncJob {
    pub id: String,
    pub source_id: SourceId,
    pub collection: String,
    pub kind: String,
    pub state: JobState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    NeverSynced,
    Fresh,
    Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceStatus {
    pub slug: String,
    pub freshness: Freshness,
    /// Last successful sync completion (collection checkpoint time).
    pub last_success_at: Option<String>,
    /// Newest `local_synced_at` across indexed resources (content write time).
    pub last_indexed_at: Option<String>,
    /// Minutes since `last_indexed_at`; null if never indexed.
    pub index_age_minutes: Option<i64>,
    /// True when agent should run `sync --wait` before trusting search results.
    pub refresh_recommended: bool,
    pub resource_count: usize,
    pub pending_jobs: usize,
    pub collections: Vec<CollectionStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionStatus {
    pub collection: String,
    pub last_success_at: Option<String>,
    pub last_indexed_at: Option<String>,
    pub index_age_minutes: Option<i64>,
    pub refresh_recommended: bool,
    pub freshness: Freshness,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncCounts {
    pub issues: usize,
    pub issue_comments: usize,
    pub pull_requests: usize,
    pub pull_request_comments: usize,
    pub pull_request_reviews: usize,
    pub pull_request_review_comments: usize,
}

impl SyncCounts {
    pub fn total(&self) -> usize {
        self.issues
            + self.issue_comments
            + self.pull_requests
            + self.pull_request_comments
            + self.pull_request_reviews
            + self.pull_request_review_comments
    }
}
