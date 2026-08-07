use github_local_indexer::adapters::github::GitHubProvider;
use github_local_indexer::application::ports::GithubProvider;
use github_local_indexer::domain::source::Source;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn github_provider_fetches_issues_and_pull_requests() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"/repos/acme/demo/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "number": 7,
                "title": "Bug",
                "body": "details",
                "updated_at": "2026-01-01T00:00:00Z",
                "html_url": "https://github.com/acme/demo/issues/7"
            }
        ])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/repos/acme/demo/pulls$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "number": 8,
                "title": "Feature PR",
                "body": "adds feature",
                "updated_at": "2026-02-01T00:00:00Z",
                "html_url": "https://github.com/acme/demo/pull/8"
            }
        ])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/repos/acme/demo/pulls/8/reviews"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "id": 99,
                "body": "Looks good",
                "state": "APPROVED",
                "submitted_at": "2026-02-01T01:00:00Z",
                "html_url": "https://github.com/acme/demo/pull/8#review-99"
            }
        ])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"/repos/acme/demo/pulls/8/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "id": 100,
                "body": "nit: rename variable",
                "path": "src/main.rs",
                "line": 12,
                "updated_at": "2026-02-01T02:00:00Z",
                "html_url": "https://github.com/acme/demo/pull/8#discussion-100"
            }
        ])))
        .mount(&server)
        .await;

    let provider = GitHubProvider::with_api_base(server.uri());
    let source = Source::new("acme".into(), "demo".into());

    let issues = provider.fetch_issues(&source, None).await.unwrap();
    assert_eq!(issues.len(), 1);

    let prs = provider.fetch_pull_requests(&source, None).await.unwrap();
    assert_eq!(prs.len(), 1);

    let reviews = provider
        .fetch_pull_request_reviews(&source, "8")
        .await
        .unwrap();
    assert_eq!(reviews.len(), 1);

    let inline = provider
        .fetch_pull_request_review_comments(&source, "8")
        .await
        .unwrap();
    assert_eq!(inline.len(), 1);
}

#[tokio::test]
async fn pulls_incremental_paginates_by_updated_and_stops_at_cutoff() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/repos/acme/demo/pulls$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "number": 20,
                "title": "New PR",
                "body": "recent",
                "updated_at": "2026-03-01T12:00:00Z",
                "html_url": "https://github.com/acme/demo/pull/20"
            },
            {
                "number": 10,
                "title": "Old PR",
                "body": "stale",
                "updated_at": "2026-01-01T00:00:00Z",
                "html_url": "https://github.com/acme/demo/pull/10"
            }
        ])))
        .mount(&server)
        .await;

    let provider = GitHubProvider::with_api_base(server.uri());
    let source = Source::new("acme".into(), "demo".into());

    let all = provider.fetch_pull_requests(&source, None).await.unwrap();
    assert_eq!(all.len(), 2);

    let incremental = provider
        .fetch_pull_requests(&source, Some("2026-02-01T00:00:00Z"))
        .await
        .unwrap();
    assert_eq!(incremental.len(), 1);
    assert_eq!(incremental[0].remote_id, "20");
}
