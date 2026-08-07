mod mapper;

use async_trait::async_trait;
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
        match resp.status() {
            StatusCode::OK => resp.json().await.map_err(AppError::provider),
            StatusCode::UNAUTHORIZED => Err(AppError::AuthFailed),
            StatusCode::FORBIDDEN => Err(AppError::PermissionDenied(url.to_string())),
            StatusCode::TOO_MANY_REQUESTS => Err(AppError::RateLimited),
            s if s.is_server_error() => Err(AppError::provider(format!("github {s}"))),
            s => Err(AppError::provider(format!("unexpected status {s}"))),
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
                    url.push_str(&format!("&since={since}"));
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
        let since = since.map(str::to_string);
        let source = source.clone();
        self.paginate(
            move |page| {
                let mut url = format!(
                    "{api_base}/repos/{owner}/{repo}/pulls?state=all&per_page=100&page={page}"
                );
                if let Some(since) = &since {
                    url.push_str(&format!("&since={since}"));
                }
                url
            },
            move |item| Ok(Some(mapper::pull_request_from_json(&source, item)?)),
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
