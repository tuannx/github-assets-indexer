use crate::domain::error::AppError;
use crate::domain::resource::{ResourceSnapshot, ResourceType};
use crate::domain::source::Source;

pub fn issue_from_json(source: &Source, item: &serde_json::Value) -> Result<ResourceSnapshot, AppError> {
    let number = item
        .get("number")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| AppError::provider("issue missing number"))?;
    snapshot(
        source,
        ResourceType::Issue,
        number.to_string(),
        None,
        None,
        item,
        true,
    )
}

pub fn issue_comment_from_json(
    source: &Source,
    issue_number: &str,
    item: &serde_json::Value,
) -> Result<ResourceSnapshot, AppError> {
    let id = item
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| AppError::provider("comment missing id"))?;
    snapshot(
        source,
        ResourceType::IssueComment,
        id.to_string(),
        Some(issue_number.to_string()),
        Some(ResourceType::Issue),
        item,
        false,
    )
}

pub fn pull_request_from_json(
    source: &Source,
    item: &serde_json::Value,
) -> Result<ResourceSnapshot, AppError> {
    let number = item
        .get("number")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| AppError::provider("pull request missing number"))?;
    snapshot(
        source,
        ResourceType::PullRequest,
        number.to_string(),
        None,
        None,
        item,
        true,
    )
}

pub fn pull_request_comment_from_json(
    source: &Source,
    pr_number: &str,
    item: &serde_json::Value,
) -> Result<ResourceSnapshot, AppError> {
    let id = item
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| AppError::provider("pr comment missing id"))?;
    snapshot(
        source,
        ResourceType::PullRequestComment,
        id.to_string(),
        Some(pr_number.to_string()),
        Some(ResourceType::PullRequest),
        item,
        false,
    )
}

pub fn pull_request_review_from_json(
    source: &Source,
    pr_number: &str,
    item: &serde_json::Value,
) -> Result<ResourceSnapshot, AppError> {
    let id = item
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| AppError::provider("review missing id"))?;
    let state = item
        .get("state")
        .and_then(|v| v.as_str())
        .unwrap_or("COMMENTED");
    let body = item
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let title = format!("Review ({state})");
    let mut snap = snapshot(
        source,
        ResourceType::PullRequestReview,
        id.to_string(),
        Some(pr_number.to_string()),
        Some(ResourceType::PullRequest),
        item,
        false,
    )?;
    snap.title = Some(title);
    snap.body = Some(body.to_string());
    snap.remote_updated_at = item
        .get("submitted_at")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    Ok(snap)
}

pub fn pull_request_review_comment_from_json(
    source: &Source,
    pr_number: &str,
    item: &serde_json::Value,
) -> Result<ResourceSnapshot, AppError> {
    let id = item
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| AppError::provider("review comment missing id"))?;
    let path = item.get("path").and_then(|v| v.as_str()).unwrap_or("?");
    let line = item.get("line").and_then(|v| v.as_u64()).unwrap_or(0);
    let body = item
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let enriched = format!("[{path}:{line}] {body}");
    let mut snap = snapshot(
        source,
        ResourceType::PullRequestReviewComment,
        id.to_string(),
        Some(pr_number.to_string()),
        Some(ResourceType::PullRequest),
        item,
        false,
    )?;
    snap.title = Some(format!("Inline comment on {path}:{line}"));
    snap.body = Some(enriched);
    Ok(snap)
}

fn snapshot(
    source: &Source,
    resource_type: ResourceType,
    remote_id: String,
    parent_remote_id: Option<String>,
    parent_resource_type: Option<ResourceType>,
    item: &serde_json::Value,
    with_title: bool,
) -> Result<ResourceSnapshot, AppError> {
    Ok(ResourceSnapshot {
        source_id: source.id.clone(),
        resource_type,
        remote_id,
        parent_remote_id,
        parent_resource_type,
        canonical_url: item
            .get("html_url")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        title: if with_title {
            item.get("title")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        } else {
            None
        },
        body: item
            .get("body")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        remote_updated_at: item
            .get("updated_at")
            .or_else(|| item.get("submitted_at"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
    })
}
