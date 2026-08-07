use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::application::ports::GithubProvider;
use crate::domain::error::AppError;
use crate::domain::resource::{ResourceSnapshot, ResourceType};
use crate::domain::source::Source;

#[derive(Deserialize)]
struct FixtureIssue {
    number: u64,
    title: String,
    body: String,
    updated_at: String,
    html_url: String,
}

#[derive(Clone, Deserialize)]
struct FixtureComment {
    id: u64,
    body: String,
    updated_at: String,
    html_url: String,
}

#[derive(Clone, Deserialize)]
struct FixtureReview {
    id: u64,
    body: String,
    state: String,
    submitted_at: String,
    html_url: String,
}

#[derive(Clone, Deserialize)]
struct FixtureReviewComment {
    id: u64,
    body: String,
    path: String,
    line: u64,
    updated_at: String,
    html_url: String,
}

pub struct FakeProvider {
    issues: Vec<FixtureIssue>,
    issue_comments: HashMap<String, Vec<FixtureComment>>,
    pull_requests: Vec<FixtureIssue>,
    pr_comments: HashMap<String, Vec<FixtureComment>>,
    pr_reviews: HashMap<String, Vec<FixtureReview>>,
    pr_review_comments: HashMap<String, Vec<FixtureReviewComment>>,
}

impl FakeProvider {
    pub fn from_embedded() -> Self {
        Self {
            issues: serde_json::from_str(include_str!("../../../fixtures/issues.json")).unwrap(),
            issue_comments: serde_json::from_str(include_str!(
                "../../../fixtures/issue_comments.json"
            ))
            .unwrap(),
            pull_requests: serde_json::from_str(include_str!(
                "../../../fixtures/pull_requests.json"
            ))
            .unwrap(),
            pr_comments: serde_json::from_str(include_str!(
                "../../../fixtures/pull_request_comments.json"
            ))
            .unwrap(),
            pr_reviews: serde_json::from_str(include_str!(
                "../../../fixtures/pull_request_reviews.json"
            ))
            .unwrap(),
            pr_review_comments: serde_json::from_str(include_str!(
                "../../../fixtures/pull_request_review_comments.json"
            ))
            .unwrap(),
        }
    }
}

#[async_trait]
impl GithubProvider for FakeProvider {
    async fn fetch_issues(
        &self,
        source: &Source,
        since: Option<&str>,
    ) -> Result<Vec<ResourceSnapshot>, AppError> {
        Ok(filter_since(&self.issues, since, |i| i.updated_at.as_str())
            .into_iter()
            .map(|i| issue_snap(source, i))
            .collect())
    }

    async fn fetch_issue_comments(
        &self,
        source: &Source,
        issue_number: &str,
    ) -> Result<Vec<ResourceSnapshot>, AppError> {
        Ok(self
            .issue_comments
            .get(issue_number)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|c| comment_snap(source, issue_number, ResourceType::IssueComment, c))
            .collect())
    }

    async fn fetch_pull_requests(
        &self,
        source: &Source,
        since: Option<&str>,
    ) -> Result<Vec<ResourceSnapshot>, AppError> {
        Ok(filter_since(&self.pull_requests, since, |pr| pr.updated_at.as_str())
            .into_iter()
            .map(|pr| {
                let mut snap = issue_snap(source, pr);
                snap.resource_type = ResourceType::PullRequest;
                snap
            })
            .collect())
    }

    async fn fetch_pull_request_comments(
        &self,
        source: &Source,
        pr_number: &str,
    ) -> Result<Vec<ResourceSnapshot>, AppError> {
        Ok(self
            .pr_comments
            .get(pr_number)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|c| comment_snap(source, pr_number, ResourceType::PullRequestComment, c))
            .collect())
    }

    async fn fetch_pull_request_reviews(
        &self,
        source: &Source,
        pr_number: &str,
    ) -> Result<Vec<ResourceSnapshot>, AppError> {
        Ok(self
            .pr_reviews
            .get(pr_number)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|r| {
                ResourceSnapshot {
                    source_id: source.id.clone(),
                    resource_type: ResourceType::PullRequestReview,
                    remote_id: r.id.to_string(),
                    parent_remote_id: Some(pr_number.to_string()),
                    parent_resource_type: Some(ResourceType::PullRequest),
                    canonical_url: r.html_url,
                    title: Some(format!("Review ({})", r.state)),
                    body: Some(r.body),
                    remote_updated_at: Some(r.submitted_at),
                }
            })
            .collect())
    }

    async fn fetch_pull_request_review_comments(
        &self,
        source: &Source,
        pr_number: &str,
    ) -> Result<Vec<ResourceSnapshot>, AppError> {
        Ok(self
            .pr_review_comments
            .get(pr_number)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|c| ResourceSnapshot {
                source_id: source.id.clone(),
                resource_type: ResourceType::PullRequestReviewComment,
                remote_id: c.id.to_string(),
                parent_remote_id: Some(pr_number.to_string()),
                parent_resource_type: Some(ResourceType::PullRequest),
                canonical_url: c.html_url,
                title: Some(format!("Inline comment on {}:{}", c.path, c.line)),
                body: Some(format!("[{}:{}] {}", c.path, c.line, c.body)),
                remote_updated_at: Some(c.updated_at),
            })
            .collect())
    }
}

fn filter_since<'a, T>(
    items: &'a [T],
    since: Option<&str>,
    updated_at: impl Fn(&'a T) -> &str,
) -> Vec<&'a T> {
    let Some(since) = since.and_then(parse_ts) else {
        return items.iter().collect();
    };
    items
        .iter()
        .filter(|item| parse_ts(updated_at(item)).is_some_and(|ts| ts >= since))
        .collect()
}

fn parse_ts(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn issue_snap(source: &Source, i: &FixtureIssue) -> ResourceSnapshot {
    ResourceSnapshot {
        source_id: source.id.clone(),
        resource_type: ResourceType::Issue,
        remote_id: i.number.to_string(),
        parent_remote_id: None,
        parent_resource_type: None,
        canonical_url: i.html_url.clone(),
        title: Some(i.title.clone()),
        body: Some(i.body.clone()),
        remote_updated_at: Some(i.updated_at.clone()),
    }
}

fn comment_snap(
    source: &Source,
    parent: &str,
    resource_type: ResourceType,
    c: FixtureComment,
) -> ResourceSnapshot {
    ResourceSnapshot {
        source_id: source.id.clone(),
        resource_type,
        remote_id: c.id.to_string(),
        parent_remote_id: Some(parent.to_string()),
        parent_resource_type: resource_type.default_parent(),
        canonical_url: c.html_url,
        title: None,
        body: Some(c.body),
        remote_updated_at: Some(c.updated_at),
    }
}
