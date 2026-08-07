use serde::{Deserialize, Serialize};

use crate::domain::time::now_iso;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceId(pub String);

impl Default for SourceId {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: SourceId,
    pub owner: String,
    pub repository: String,
    pub canonical_url: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl Source {
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.repository)
    }

    pub fn new(owner: String, repository: String) -> Self {
        let canonical_url = format!("https://github.com/{owner}/{repository}");
        let now = now_iso();
        Self {
            id: SourceId::new(),
            owner,
            repository,
            canonical_url,
            enabled: true,
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RepoSlug {
    pub owner: String,
    pub repository: String,
}

pub fn parse_repo_slug(input: &str) -> Result<RepoSlug, String> {
    let input = input
        .trim()
        .trim_start_matches("https://github.com/")
        .trim_end_matches('/');

    let (owner, repo) = input
        .split_once('/')
        .ok_or_else(|| "expected owner/repo".to_string())?;

    if !is_valid_segment(owner) || !is_valid_segment(repo) {
        return Err("invalid owner or repository name".into());
    }

    Ok(RepoSlug {
        owner: owner.to_string(),
        repository: repo.to_string(),
    })
}

fn is_valid_segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_slug() {
        let slug = parse_repo_slug("acme/demo").unwrap();
        assert_eq!(slug.owner, "acme");
        assert_eq!(slug.repository, "demo");
    }

    #[test]
    fn rejects_invalid_slug() {
        assert!(parse_repo_slug("bad").is_err());
    }
}
