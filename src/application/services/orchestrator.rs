use crate::application::ports::{GithubProvider, ResourceStore, SyncStore};
use crate::domain::error::AppError;
use crate::domain::resource::{ResourceSnapshot, SyncCounts};
use crate::domain::source::Source;
use crate::domain::sync_cursor::collection_checkpoint;
use crate::domain::time::now_iso;

pub struct SyncReport {
    pub slug: String,
    pub counts: SyncCounts,
    pub synced_at: String,
}

pub async fn sync_source(
    source: &Source,
    provider: &dyn GithubProvider,
    resources: &dyn ResourceStore,
    sync: &dyn SyncStore,
) -> Result<SyncReport, AppError> {
    let mut counts = SyncCounts::default();

    let issue_since = sync
        .get_checkpoint(source.id.as_str(), "issues")
        .await?;
    let issues = provider
        .fetch_issues(source, issue_since.as_deref())
        .await?;
    counts.issues = resources.upsert_batch(&issues).await?;

    let mut issue_comments = Vec::new();
    for snap in &issues {
        let comments = provider
            .fetch_issue_comments(source, &snap.remote_id)
            .await?;
        counts.issue_comments += sync_children(resources, &comments).await?;
        issue_comments.extend(comments);
    }

    let issue_checkpoint = collection_checkpoint(&issues, &issue_comments, issue_since.as_deref());
    sync.set_checkpoint(source.id.as_str(), "issues", &issue_checkpoint)
        .await?;

    let pr_since = sync
        .get_checkpoint(source.id.as_str(), "pull_requests")
        .await?;
    let pull_requests = provider
        .fetch_pull_requests(source, pr_since.as_deref())
        .await?;
    counts.pull_requests = resources.upsert_batch(&pull_requests).await?;

    let mut pr_children = Vec::new();
    for snap in &pull_requests {
        let pr = &snap.remote_id;
        let comments = provider.fetch_pull_request_comments(source, pr).await?;
        counts.pull_request_comments += sync_children(resources, &comments).await?;
        pr_children.extend(comments);

        let reviews = provider.fetch_pull_request_reviews(source, pr).await?;
        counts.pull_request_reviews += sync_children(resources, &reviews).await?;
        pr_children.extend(reviews);

        let inline = provider
            .fetch_pull_request_review_comments(source, pr)
            .await?;
        counts.pull_request_review_comments += sync_children(resources, &inline).await?;
        pr_children.extend(inline);
    }

    let pr_checkpoint =
        collection_checkpoint(&pull_requests, &pr_children, pr_since.as_deref());
    sync.set_checkpoint(source.id.as_str(), "pull_requests", &pr_checkpoint)
        .await?;

    Ok(SyncReport {
        slug: source.slug(),
        counts,
        synced_at: now_iso(),
    })
}

async fn sync_children(
    resources: &dyn ResourceStore,
    snapshots: &[ResourceSnapshot],
) -> Result<usize, AppError> {
    if snapshots.is_empty() {
        return Ok(0);
    }
    resources.upsert_batch(snapshots).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::fake::FakeProvider;
    use crate::adapters::sqlite::SqliteStore;
    use crate::application::ports::SourceStore;
    use crate::domain::resource::ResourceType;
    use tempfile::tempdir;

    #[tokio::test]
    async fn syncs_all_contribution_types_from_fake() {
        let dir = tempdir().unwrap();
        let store = SqliteStore::new(dir.path().join("test.db"));
        store.migrate().await.unwrap();
        let source = Source::new("acme".into(), "demo".into());
        store.insert_source(&source).await.unwrap();
        let fake = FakeProvider::from_embedded();

        let report = sync_source(&source, &fake, &store, &store)
            .await
            .unwrap();
        assert!(report.counts.issues >= 3);

        let second = sync_source(&source, &fake, &store, &store)
            .await
            .unwrap();
        assert_eq!(second.counts.issues, 1);
        assert_eq!(second.counts.issue_comments, 0);

        let checkpoint = store
            .get_checkpoint(source.id.as_str(), "issues")
            .await
            .unwrap();
        assert!(checkpoint.is_some());

        let hits = store.search("unwrap", 10).await.unwrap();
        assert!(hits.iter().any(|h| {
            h.resource_type == ResourceType::PullRequestReviewComment
        }));
    }

    #[tokio::test]
    async fn first_sync_indexes_all_contribution_types() {
        let dir = tempdir().unwrap();
        let store = SqliteStore::new(dir.path().join("test.db"));
        store.migrate().await.unwrap();
        let source = Source::new("acme".into(), "demo".into());
        store.insert_source(&source).await.unwrap();
        let fake = FakeProvider::from_embedded();

        let report = sync_source(&source, &fake, &store, &store)
            .await
            .unwrap();
        assert!(report.counts.pull_requests >= 1);
        assert!(report.counts.pull_request_reviews >= 1);
        assert!(report.counts.pull_request_review_comments >= 1);
    }
}
