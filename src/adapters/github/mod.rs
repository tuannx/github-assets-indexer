mod mapper;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::StatusCode;

use crate::application::ports::GithubProvider;
use crate::domain::error::AppError;
use crate::domain::resource::ResourceSnapshot;
use crate::domain::source::Source;

pub struct GitHubProvider {
    client: reqwest::Client,
    token: Option<String>,
    api_base: String,
}

impl GitHubProvider {
    pub fn from_env() -> Result<Self, AppError> {
        Ok(Self {
            client: reqwest::Client::new(),
            token: std::env::var("GITHUB_TOKEN").ok(),
            api_base: "https://api.github.com".into(),
        })
    }

    pub fn with_api_base(api_base: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            token: None,
            api_base: api_base.into(),
        }
    }

    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(t) => req.header("Authorization", format!("Bearer {t}")),
            None => req,
        }
    }

    async fn get_json(&self, url: &str) -> Result<serde_json::Value, AppError> {
        let resp = self
            .auth(
                self.client
                    .get(url)
                    .header("User-Agent", "github-local-indexer")
                    .header("X-GitHub-Api-Version", "2022-11-28")
                    .header("Accept", "application/vnd.github+json"),
            )
            .send()
            .await
            .map_err(AppError::provider)?;
        let status = resp.status();
        if status == StatusCode::OK {
            return resp.json().await.map_err(AppError::provider);
        }
        let body = resp.text().await.unwrap_or_default();
        match status {
            StatusCode::UNAUTHORIZED => Err(AppError::AuthFailed),
            StatusCode::FORBIDDEN => Err(AppError::PermissionDenied(url.to_string())),
            StatusCode::TOO_MANY_REQUESTS => Err(AppError::RateLimited),
            StatusCode::UNPROCESSABLE_ENTITY => Err(AppError::provider(format!(
                "github 422 validation failed for {url}: {body}"
            ))),
            s if s.is_server_error() => Err(AppError::provider(format!("github {s}: {body}"))),
            s => Err(AppError::provider(format!(
                "unexpected status {s} for {url}: {body}"
            ))),
        }
    }

    async fn paginate<F>(
        &self,
        build_url: impl Fn(u32) -> String,
        mut map_item: F,
    ) -> Result<Vec<ResourceSnapshot>, AppError>
    where
        F: FnMut(&serde_json::Value) -> Result<Option<ResourceSnapshot>, AppError>,
    {
        let mut page = 1u32;
        let mut all = Vec::new();
        loop {
            let items = self.get_json(&build_url(page)).await?;
            let Some(arr) = items.as_array() else {
                break;
            };
            if arr.is_empty() {
                break;
            }
            for item in arr {
                if let Some(snap) = map_item(item)? {
                    all.push(snap);
                }
            }
            if arr.len() < 100 {
                break;
            }
            page += 1;
        }
        Ok(all)
    }

    /// Paginate list endpoints sorted by `updated` desc; stop when `updated_at` < checkpoint.
    /// Used for `/pulls` which has no `since` query param (would return 422).
    async fn paginate_updated_since<M>(
        &self,
        build_url: impl Fn(u32) -> String,
        since: Option<&str>,
        mut map_item: M,
    ) -> Result<Vec<ResourceSnapshot>, AppError>
    where
        M: FnMut(&serde_json::Value) -> Result<ResourceSnapshot, AppError>,
    {
        let since_ts = since.and_then(parse_github_time);
        let mut page = 1u32;
        let mut all = Vec::new();
        loop {
            let items = self.get_json(&build_url(page)).await?;
            let Some(arr) = items.as_array() else {
                break;
            };
            if arr.is_empty() {
                break;
            }
            let mut reached_cutoff = false;
            for item in arr {
                let snap = map_item(item)?;
                if let Some(cutoff) = since_ts {
                    if snap
                        .remote_updated_at
                        .as_deref()
                        .and_then(parse_github_time)
                        .is_some_and(|updated| updated < cutoff)
                    {
                        reached_cutoff = true;
                        break;
                    }
                }
                all.push(snap);
            }
            if reached_cutoff || arr.len() < 100 {
                break;
            }
            page += 1;
        }
        Ok(all)
    }

    async fn list_array<F>(
        &self,
        url: &str,
        mut map_item: F,
    ) -> Result<Vec<ResourceSnapshot>, AppError>
    where
        F: FnMut(&serde_json::Value) -> Result<ResourceSnapshot, AppError>,
    {
        let items = self.get_json(url).await?;
        let Some(arr) = items.as_array() else {
            return Ok(vec![]);
        };
        arr.iter().map(&mut map_item).collect()
    }
}

#[async_trait]
impl GithubProvider for GitHubProvider {
    async fn fetch_issues(
        &self,
        source: &Source,
        since: Option<&str>,
    ) -> Result<Vec<ResourceSnapshot>, AppError> {
        let api_base = self.api_base.clone();
        let owner = source.owner.clone();
        let repo = source.repository.clone();
        let since = since.map(str::to_string);
        let source = source.clone();
        self.paginate(
            move |page| {
                let mut url = format!(
                    "{api_base}/repos/{owner}/{repo}/issues?state=all&per_page=100&page={page}"
                );
                if let Some(since) = &since {
                    url.push_str(&format!("&since={}", github_since_param(since)));
                }
                url
            },
            move |item| {
                if item.get("pull_request").is_some() {
                    return Ok(None);
                }
                Ok(Some(mapper::issue_from_json(&source, item)?))
            },
        )
        .await
    }

    async fn fetch_issue_comments(
        &self,
        source: &Source,
        issue_number: &str,
    ) -> Result<Vec<ResourceSnapshot>, AppError> {
        let url = format!(
            "{}/repos/{}/{}/issues/{}/comments?per_page=100",
            self.api_base, source.owner, source.repository, issue_number
        );
        let source = source.clone();
        let issue_number = issue_number.to_string();
        self.list_array(&url, move |item| {
            mapper::issue_comment_from_json(&source, &issue_number, item)
        })
        .await
    }

    async fn fetch_pull_requests(
        &self,
        source: &Source,
        since: Option<&str>,
    ) -> Result<Vec<ResourceSnapshot>, AppError> {
        let api_base = self.api_base.clone();
        let owner = source.owner.clone();
        let repo = source.repository.clone();
        let source = source.clone();
        self.paginate_updated_since(
            move |page| {
                format!(
                    "{api_base}/repos/{owner}/{repo}/pulls?state=all&per_page=100&page={page}&sort=updated&direction=desc"
                )
            },
            since,
            move |item| mapper::pull_request_from_json(&source, item),
        )
        .await
    }

    async fn fetch_pull_request_comments(
        &self,
        source: &Source,
        pr_number: &str,
    ) -> Result<Vec<ResourceSnapshot>, AppError> {
        let url = format!(
            "{}/repos/{}/{}/issues/{}/comments?per_page=100",
            self.api_base, source.owner, source.repository, pr_number
        );
        let source = source.clone();
        let pr_number = pr_number.to_string();
        self.list_array(&url, move |item| {
            mapper::pull_request_comment_from_json(&source, &pr_number, item)
        })
        .await
    }

    async fn fetch_pull_request_reviews(
        &self,
        source: &Source,
        pr_number: &str,
    ) -> Result<Vec<ResourceSnapshot>, AppError> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{}/reviews?per_page=100",
            self.api_base, source.owner, source.repository, pr_number
        );
        let source = source.clone();
        let pr_number = pr_number.to_string();
        self.list_array(&url, move |item| {
            mapper::pull_request_review_from_json(&source, &pr_number, item)
        })
        .await
    }

    async fn fetch_pull_request_review_comments(
        &self,
        source: &Source,
        pr_number: &str,
    ) -> Result<Vec<ResourceSnapshot>, AppError> {
        let url = format!(
            "{}/repos/{}/{}/pulls/{}/comments?per_page=100",
            self.api_base, source.owner, source.repository, pr_number
        );
        let source = source.clone();
        let pr_number = pr_number.to_string();
        self.list_array(&url, move |item| {
            mapper::pull_request_review_comment_from_json(&source, &pr_number, item)
        })
        .await
    }
}

fn github_since_param(since: &str) -> String {
    parse_github_time(since)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| since.to_string())
}

fn parse_github_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}
