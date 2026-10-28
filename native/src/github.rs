//! Bounded, read-only GitHub REST client for Core-owned issue synchronization.
//!
//! Security boundaries:
//! - the base URL is the explicit `https://api.github.com` endpoint. A
//!   loopback override is available only to test builds and to a deployment
//!   that explicitly sets `FACTORY_ALLOW_TEST_UPSTREAM=1`; Office can never
//!   supply a URL.
//! - the bearer token is passed per call, never stored in the client, never
//!   placed in a URL and never rendered in an error.
//! - redirects are refused so a credential cannot be replayed to another host.
//! - responses, pages, individual fields and request time are bounded.
//!
//! Issue bodies are untrusted text. This module stores them as opaque strings;
//! it never executes or interprets them.

use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::HeaderMap;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

pub const API_BASE: &str = "https://api.github.com";
pub const MAX_ISSUE_PAGES: usize = 10;
pub const MAX_ISSUE_BODY_BYTES: usize = 64 * 1024;
pub const MAX_ISSUE_TITLE_BYTES: usize = 512;
const MAX_REPOSITORY_PAGES: usize = 1;
const PER_PAGE: usize = 100;
const MAX_REST_BODY_BYTES: usize = 16 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_TOKEN_BYTES: usize = 512;
const MAX_REPO_PART_BYTES: usize = 100;
const MAX_BRANCH_BYTES: usize = 255;
const MAX_LABEL_NAME_BYTES: usize = 100;
const MAX_LABELS: usize = 50;
const MAX_SCOPES: usize = 50;
const MAX_PULL_REQUEST_TITLE_BYTES: usize = 256;
const MAX_PULL_REQUEST_BODY_BYTES: usize = 64 * 1024;
const MAX_PULL_REQUEST_DIFF_BYTES: usize = 512 * 1024;
const MAX_PULL_REQUEST_COMMENTS: usize = 100;
const MAX_PULL_REQUEST_CHECKS: usize = 100;
const MAX_REVIEWERS: usize = 15;
const MAX_GITHUB_LOGIN_BYTES: usize = 39;
const USER_AGENT: &str = "software-factory-core";

#[derive(Clone)]
pub struct GitHubClient {
    base_url: String,
    client: reqwest::Client,
}

impl std::fmt::Debug for GitHubClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GitHubClient")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GitHubError {
    #[error("GitHub API configuration is invalid: {0}")]
    Invalid(String),
    #[error("GitHub rejected the credential")]
    Unauthorized,
    #[error("GitHub denied the request: {0}")]
    Forbidden(String),
    #[error("GitHub resource was not found or is not visible to this credential")]
    NotFound,
    #[error("GitHub rate limit is exhausted")]
    RateLimited {
        status_code: u16,
        next_allowed_at: Option<String>,
    },
    #[error("GitHub refused a redirect")]
    Redirected,
    #[error("GitHub rejected the request ({status_code}): {message}")]
    Rejected { status_code: u16, message: String },
    #[error("GitHub blocked the merge ({status_code}): {message}")]
    MergeBlocked { status_code: u16, message: String },
    #[error("GitHub upstream failed: {0}")]
    Upstream(String),
    #[error("GitHub response was invalid: {0}")]
    InvalidResponse(String),
}

impl GitHubError {
    /// Stable classification used for durable sync state.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Unauthorized => "DENIED",
            Self::Forbidden(_) => "DENIED",
            Self::NotFound => "NOT_FOUND",
            Self::RateLimited { .. } => "RATE_LIMITED",
            Self::Invalid(_) => "ERROR",
            Self::Rejected { .. } => "REJECTED",
            Self::MergeBlocked { .. } => "BLOCKED",
            Self::Redirected | Self::Upstream(_) | Self::InvalidResponse(_) => "ERROR",
        }
    }

    pub fn status_code(&self) -> Option<u16> {
        match self {
            Self::Unauthorized => Some(401),
            Self::Forbidden(_) => Some(403),
            Self::NotFound => Some(404),
            Self::RateLimited { status_code, .. } => Some(*status_code),
            Self::Rejected { status_code, .. } | Self::MergeBlocked { status_code, .. } => {
                Some(*status_code)
            }
            _ => None,
        }
    }

    pub fn next_allowed_at(&self) -> Option<&str> {
        match self {
            Self::RateLimited {
                next_allowed_at, ..
            } => next_allowed_at.as_deref(),
            _ => None,
        }
    }

    /// Human-readable detail that is safe to persist and display. It never
    /// contains the response body, request headers or the credential.
    pub fn detail(&self) -> String {
        match self {
            Self::Invalid(message) => format!("GitHub integration configuration: {message}"),
            Self::Unauthorized => {
                "GitHub rejected the credential (401). Replace the token.".into()
            }
            Self::Forbidden(message) => format!("GitHub denied repository access: {message}"),
            Self::NotFound => {
                "GitHub reported the repository as not found or not visible to this credential (404).".into()
            }
            Self::RateLimited {
                next_allowed_at, ..
            } => match next_allowed_at {
                Some(value) => format!("GitHub rate limit is exhausted until {value}."),
                None => "GitHub rate limit is exhausted; retry later.".into(),
            },
            Self::Redirected => {
                "GitHub returned a redirect. Core refused to forward the credential.".into()
            }
            Self::Rejected {
                status_code,
                message,
            } => format!("GitHub rejected the request ({status_code}): {message}"),
            Self::MergeBlocked {
                status_code,
                message,
            } => format!("GitHub blocked the merge ({status_code}): {message}"),
            Self::Upstream(message) => format!("GitHub is unreachable: {message}"),
            Self::InvalidResponse(message) => format!("GitHub response was invalid: {message}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub login: String,
    pub scopes: Vec<String>,
    pub scopes_reported: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct GitHubRepository {
    pub full_name: String,
    pub private: bool,
    pub default_branch: String,
    pub html_url: String,
    pub pushed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GitHubLabel {
    pub name: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GitHubIssue {
    pub number: u64,
    pub github_id: i64,
    pub title: String,
    pub body: String,
    pub body_truncated: bool,
    pub state: String,
    pub state_reason: Option<String>,
    pub labels: Vec<GitHubLabel>,
    pub author: Option<String>,
    pub html_url: String,
    pub comments: u32,
    pub created_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
}

impl GitHubIssue {
    pub fn content_hash(&self) -> String {
        let payload = serde_json::json!({
            "number": self.number,
            "title": self.title,
            "body": self.body,
            "body_truncated": self.body_truncated,
            "state": self.state,
            "state_reason": self.state_reason,
            "labels": self.labels,
            "author": self.author,
            "html_url": self.html_url,
            "comments": self.comments,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
            "closed_at": self.closed_at,
        });
        let digest = Sha256::digest(payload.to_string().as_bytes());
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

#[derive(Debug, Clone)]
pub struct IssuePage {
    pub issues: Vec<GitHubIssue>,
    pub etag: Option<String>,
    pub not_modified: bool,
    pub page_full: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPullRequest {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub draft: bool,
    pub merged: bool,
    pub mergeable: Option<bool>,
    pub mergeable_state: String,
    pub head_ref: String,
    pub head_sha: String,
    pub base_ref: String,
    pub base_sha: String,
    pub author: Option<String>,
    pub html_url: String,
    pub changed_files: u32,
    pub additions: u32,
    pub deletions: u32,
    pub comments: u32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubBranch {
    pub name: String,
    pub commit_sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubCheckRun {
    pub id: u64,
    pub name: String,
    pub status: String,
    pub conclusion: String,
    pub app: Option<String>,
    pub details_url: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubStatusContext {
    pub context: String,
    pub state: String,
    pub description: String,
    pub target_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPullRequestChecks {
    pub head_sha: String,
    pub state: String,
    pub detail: String,
    pub ready: bool,
    pub check_runs: Vec<GitHubCheckRun>,
    pub check_runs_truncated: bool,
    pub combined_status: String,
    pub statuses: Vec<GitHubStatusContext>,
    pub statuses_truncated: bool,
    pub fetched_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubComment {
    pub id: u64,
    pub kind: String,
    pub author: Option<String>,
    pub body: String,
    pub body_truncated: bool,
    pub path: Option<String>,
    pub line: Option<u32>,
    pub html_url: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubCommentList {
    pub comments: Vec<GitHubComment>,
    pub truncated: bool,
    pub fetched_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPullRequestDiff {
    pub patch: String,
    pub bytes: usize,
    pub truncated: bool,
    pub fetched_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GitHubMergeMethod {
    Merge,
    Squash,
    Rebase,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubMergeOutcome {
    pub merged: bool,
    pub merge_commit_sha: String,
    pub message: String,
}

pub struct NewPullRequest<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub head: &'a str,
    pub base: &'a str,
    pub draft: bool,
}

pub struct MergePullRequest<'a> {
    pub number: u64,
    pub expected_head_sha: &'a str,
    pub merge_method: GitHubMergeMethod,
    pub commit_title: Option<&'a str>,
    pub commit_message: Option<&'a str>,
}

struct BoundedResponse {
    status: reqwest::StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl GitHubClient {
    /// Production client. Set `FACTORY_ALLOW_TEST_UPSTREAM=1` together with a
    /// loopback `FACTORY_GITHUB_API_BASE` for deterministic local harnesses;
    /// the override is refused for non-loopback hosts and without the flag.
    pub fn from_env() -> Result<Self, GitHubError> {
        let allow_loopback_http =
            std::env::var("FACTORY_ALLOW_TEST_UPSTREAM").ok().as_deref() == Some("1");
        match std::env::var("FACTORY_GITHUB_API_BASE") {
            Ok(base) => {
                if !allow_loopback_http {
                    return Err(GitHubError::Invalid(
                        "FACTORY_GITHUB_API_BASE requires FACTORY_ALLOW_TEST_UPSTREAM=1".into(),
                    ));
                }
                let url = reqwest::Url::parse(&base)
                    .map_err(|_| GitHubError::Invalid("test GitHub base is not absolute".into()))?;
                if !url.host_str().is_some_and(loopback_host) {
                    return Err(GitHubError::Invalid(
                        "test GitHub base must be a loopback host".into(),
                    ));
                }
                Self::build(&base, true)
            }
            Err(std::env::VarError::NotPresent) => Self::build(API_BASE, false),
            Err(_) => Err(GitHubError::Invalid(
                "FACTORY_GITHUB_API_BASE is not valid text".into(),
            )),
        }
    }

    /// Production constructor used by deterministic tests and startup. The
    /// only base URL is the explicit GitHub API.
    pub fn production() -> Result<Self, GitHubError> {
        Self::build(API_BASE, false)
    }

    /// Construction-time test client. Never reachable from an Office request.
    #[cfg(test)]
    pub fn with_base_url(base_url: &str, allow_loopback_http: bool) -> Result<Self, GitHubError> {
        Self::build(base_url, allow_loopback_http)
    }

    fn build(base_url: &str, allow_loopback_http: bool) -> Result<Self, GitHubError> {
        let normalized = validate_base_url(base_url, allow_loopback_http)?;
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| GitHubError::Invalid("HTTP client could not be created".into()))?;
        Ok(Self {
            base_url: normalized,
            client,
        })
    }

    pub async fn authenticated_user(&self, token: &str) -> Result<AuthenticatedUser, GitHubError> {
        validate_token(token)?;
        let response = self
            .execute(self.client.get(format!("{}/user", self.base_url)), token)
            .await?;
        let user: RawUser = parse_json(&response.body, "user")?;
        let login = bounded(&clean_single_line(&user.login), 255);
        if login.is_empty() {
            return Err(GitHubError::InvalidResponse(
                "user payload had no usable login".into(),
            ));
        }
        let scopes_reported = response.headers.contains_key("x-oauth-scopes");
        let scopes = response
            .headers
            .get("x-oauth-scopes")
            .and_then(|value| value.to_str().ok())
            .map(parse_scopes)
            .unwrap_or_default();
        Ok(AuthenticatedUser {
            login,
            scopes,
            scopes_reported,
        })
    }

    pub async fn repositories(
        &self,
        token: &str,
        query: Option<&str>,
    ) -> Result<Vec<GitHubRepository>, GitHubError> {
        validate_token(token)?;
        let needle = query.unwrap_or("").trim().to_lowercase();
        if needle.len() > 100 {
            return Err(GitHubError::Invalid(
                "repository query is longer than 100 characters".into(),
            ));
        }
        let mut repositories = Vec::new();
        for page in 1..=MAX_REPOSITORY_PAGES {
            let response = self
                .execute(
                    self.client
                        .get(format!("{}/user/repos", self.base_url))
                        .query(&[
                            ("per_page", PER_PAGE.to_string()),
                            ("page", page.to_string()),
                            ("sort", "updated".to_string()),
                            (
                                "affiliation",
                                "owner,collaborator,organization_member".to_string(),
                            ),
                        ]),
                    token,
                )
                .await?;
            let items: Vec<RawRepository> = parse_json(&response.body, "repositories")?;
            let page_len = items.len();
            for item in items {
                if !needle.is_empty() && !item.full_name.to_lowercase().contains(&needle) {
                    continue;
                }
                let default_branch = item.default_branch.unwrap_or_default();
                if !valid_repository_part(&item.full_name) {
                    continue;
                }
                repositories.push(GitHubRepository {
                    full_name: item.full_name,
                    private: item.private,
                    default_branch,
                    html_url: safe_https_url(&item.html_url),
                    pushed_at: item.pushed_at.as_deref().and_then(normalize_timestamp),
                });
                if repositories.len() >= PER_PAGE {
                    return Ok(repositories);
                }
            }
            if page_len < PER_PAGE {
                break;
            }
        }
        Ok(repositories)
    }

    /// Resolve one branch to its exact head commit.
    pub async fn branch_commit(
        &self,
        token: &str,
        owner: &str,
        repo: &str,
        branch: &str,
    ) -> Result<GitHubBranch, GitHubError> {
        validate_token(token)?;
        validate_repo_coordinates(owner, repo)?;
        validate_default_branch(branch)?;
        let response = self
            .execute(
                self.client.get(format!(
                    "{}/repos/{owner}/{repo}/branches/{branch}",
                    self.base_url
                )),
                token,
            )
            .await?;
        let raw: RawBranch = parse_json(&response.body, "branch")?;
        let commit_sha = raw.commit.sha.to_ascii_lowercase();
        if !is_object_id(&commit_sha) {
            return Err(GitHubError::InvalidResponse(
                "branch payload had a malformed commit".into(),
            ));
        }
        Ok(GitHubBranch {
            name: bounded(&clean_single_line(branch), MAX_BRANCH_BYTES),
            commit_sha,
        })
    }

    /// Read one pull request and its mergeability facts.
    pub async fn pull_request(
        &self,
        token: &str,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<GitHubPullRequest, GitHubError> {
        validate_token(token)?;
        validate_repo_coordinates(owner, repo)?;
        validate_pull_number(number)?;
        let response = self
            .execute(
                self.client.get(format!(
                    "{}/repos/{owner}/{repo}/pulls/{number}",
                    self.base_url
                )),
                token,
            )
            .await?;
        let raw: RawPullRequest = parse_json(&response.body, "pull request")?;
        project_pull_request(raw)
    }

    /// List open pull requests whose head is one exact repository branch.
    pub async fn open_pull_requests_for_head(
        &self,
        token: &str,
        owner: &str,
        repo: &str,
        head: &str,
    ) -> Result<Vec<GitHubPullRequest>, GitHubError> {
        validate_token(token)?;
        validate_repo_coordinates(owner, repo)?;
        validate_default_branch(head)?;
        let response = self
            .execute(
                self.client
                    .get(format!("{}/repos/{owner}/{repo}/pulls", self.base_url))
                    .query(&[
                        ("state", "open"),
                        ("head", &format!("{owner}:{head}")),
                        ("per_page", "100"),
                    ]),
                token,
            )
            .await?;
        let raw: Vec<RawPullRequest> = parse_json(&response.body, "pull requests")?;
        raw.into_iter()
            .take(100)
            .map(project_pull_request)
            .collect()
    }

    /// Read bounded check runs and legacy commit statuses for one exact ref.
    pub async fn pull_request_checks(
        &self,
        token: &str,
        owner: &str,
        repo: &str,
        reference: &str,
    ) -> Result<GitHubPullRequestChecks, GitHubError> {
        validate_token(token)?;
        validate_repo_coordinates(owner, repo)?;
        if !is_object_id(reference) {
            return Err(GitHubError::Invalid(
                "check status requires a full Git object ID".into(),
            ));
        }
        let checks = self
            .execute(
                self.client
                    .get(format!(
                        "{}/repos/{owner}/{repo}/commits/{reference}/check-runs",
                        self.base_url
                    ))
                    .query(&[("per_page", "100"), ("filter", "latest")]),
                token,
            )
            .await?;
        let raw_checks: RawCheckRuns = parse_json(&checks.body, "check runs")?;
        let combined = self
            .execute(
                self.client.get(format!(
                    "{}/repos/{owner}/{repo}/commits/{reference}/status",
                    self.base_url
                )),
                token,
            )
            .await?;
        let raw_status: RawCombinedStatus = parse_json(&combined.body, "commit status")?;
        project_checks(reference, raw_checks, raw_status)
    }

    /// Read the bounded unified diff GitHub renders for one pull request.
    pub async fn pull_request_diff(
        &self,
        token: &str,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<GitHubPullRequestDiff, GitHubError> {
        validate_token(token)?;
        validate_repo_coordinates(owner, repo)?;
        validate_pull_number(number)?;
        let response = self
            .execute_with_accept(
                self.client.get(format!(
                    "{}/repos/{owner}/{repo}/pulls/{number}",
                    self.base_url
                )),
                token,
                "application/vnd.github.diff",
            )
            .await?;
        let (body, truncated) = read_truncated(response.body, MAX_PULL_REQUEST_DIFF_BYTES)?;
        let patch = String::from_utf8(body)
            .map_err(|_| GitHubError::InvalidResponse("pull request diff was not UTF-8".into()))?;
        let bytes = patch.len();
        Ok(GitHubPullRequestDiff {
            patch,
            bytes,
            truncated,
            fetched_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        })
    }

    /// Read bounded issue and review comments for one pull request.
    pub async fn pull_request_comments(
        &self,
        token: &str,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<GitHubCommentList, GitHubError> {
        validate_token(token)?;
        validate_repo_coordinates(owner, repo)?;
        validate_pull_number(number)?;
        let issue_comments = self
            .execute(
                self.client
                    .get(format!(
                        "{}/repos/{owner}/{repo}/issues/{number}/comments",
                        self.base_url
                    ))
                    .query(&[("per_page", "100"), ("page", "1")]),
                token,
            )
            .await?;
        let review_comments = self
            .execute(
                self.client
                    .get(format!(
                        "{}/repos/{owner}/{repo}/pulls/{number}/comments",
                        self.base_url
                    ))
                    .query(&[("per_page", "100"), ("page", "1")]),
                token,
            )
            .await?;
        let raw_issue: Vec<RawIssueComment> = parse_json(&issue_comments.body, "issue comments")?;
        let raw_review: Vec<RawReviewComment> =
            parse_json(&review_comments.body, "review comments")?;
        let issue_full = raw_issue.len() >= 100;
        let review_full = raw_review.len() >= 100;
        let mut comments = Vec::with_capacity(raw_issue.len() + raw_review.len());
        for raw in raw_issue {
            if let Some(comment) = raw.into_comment("issue") {
                comments.push(comment);
            }
        }
        for raw in raw_review {
            if let Some(comment) = raw.into_comment() {
                comments.push(comment);
            }
        }
        comments.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        let mut truncated = issue_full || review_full || comments.len() > MAX_PULL_REQUEST_COMMENTS;
        comments.truncate(MAX_PULL_REQUEST_COMMENTS);
        truncated |= comments.len() == MAX_PULL_REQUEST_COMMENTS && (issue_full || review_full);
        Ok(GitHubCommentList {
            comments,
            truncated,
            fetched_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        })
    }

    /// Open one pull request from an already verified exact head branch.
    pub async fn create_pull_request(
        &self,
        token: &str,
        owner: &str,
        repo: &str,
        input: NewPullRequest<'_>,
    ) -> Result<GitHubPullRequest, GitHubError> {
        validate_token(token)?;
        validate_repo_coordinates(owner, repo)?;
        let title = clean_single_line(input.title.trim());
        if title.is_empty() || title.len() > MAX_PULL_REQUEST_TITLE_BYTES {
            return Err(GitHubError::Invalid(
                "pull request title must be 1-256 characters".into(),
            ));
        }
        let body = clean_body(input.body);
        if body.len() > MAX_PULL_REQUEST_BODY_BYTES {
            return Err(GitHubError::Invalid(
                "pull request body exceeds 64 KiB".into(),
            ));
        }
        validate_default_branch(input.head)?;
        validate_default_branch(input.base)?;
        if input.head == input.base {
            return Err(GitHubError::Invalid(
                "pull request head and base branches must differ".into(),
            ));
        }
        let response = self
            .execute(
                self.client
                    .post(format!("{}/repos/{owner}/{repo}/pulls", self.base_url))
                    .json(&serde_json::json!({
                        "title": title,
                        "body": body,
                        "head": input.head,
                        "base": input.base,
                        "draft": input.draft,
                    })),
                token,
            )
            .await?;
        let raw: RawPullRequest = parse_json(&response.body, "pull request")?;
        project_pull_request(raw)
    }

    /// Add one plain comment to a pull request conversation.
    pub async fn create_pull_request_comment(
        &self,
        token: &str,
        owner: &str,
        repo: &str,
        number: u64,
        body: &str,
    ) -> Result<GitHubComment, GitHubError> {
        validate_token(token)?;
        validate_repo_coordinates(owner, repo)?;
        validate_pull_number(number)?;
        let cleaned = clean_body(body);
        if cleaned.trim().is_empty() || cleaned.len() > MAX_PULL_REQUEST_BODY_BYTES {
            return Err(GitHubError::Invalid(
                "comment body must be 1-65536 characters".into(),
            ));
        }
        let response = self
            .execute(
                self.client
                    .post(format!(
                        "{}/repos/{owner}/{repo}/issues/{number}/comments",
                        self.base_url
                    ))
                    .json(&serde_json::json!({"body": cleaned})),
                token,
            )
            .await?;
        let raw: RawIssueComment = parse_json(&response.body, "comment")?;
        raw.into_comment("issue")
            .ok_or_else(|| GitHubError::InvalidResponse("comment had no usable shape".into()))
    }

    /// Request reviews from named GitHub users. No approval is inferred here.
    pub async fn request_pull_request_reviewers(
        &self,
        token: &str,
        owner: &str,
        repo: &str,
        number: u64,
        reviewers: &[String],
    ) -> Result<Vec<String>, GitHubError> {
        validate_token(token)?;
        validate_repo_coordinates(owner, repo)?;
        validate_pull_number(number)?;
        if reviewers.is_empty() || reviewers.len() > MAX_REVIEWERS {
            return Err(GitHubError::Invalid(
                "request between 1 and 15 reviewers".into(),
            ));
        }
        let mut cleaned = Vec::with_capacity(reviewers.len());
        for reviewer in reviewers {
            let reviewer = reviewer.trim();
            if !valid_github_login(reviewer) {
                return Err(GitHubError::Invalid(
                    "reviewer login has an unsafe shape".into(),
                ));
            }
            if !cleaned
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(reviewer))
            {
                cleaned.push(reviewer.to_owned());
            }
        }
        self.execute(
            self.client
                .post(format!(
                    "{}/repos/{owner}/{repo}/pulls/{number}/requested_reviewers",
                    self.base_url
                ))
                .json(&serde_json::json!({"reviewers": cleaned})),
            token,
        )
        .await?;
        Ok(cleaned)
    }

    /// Merge one pull request only with the exact expected head pinned. GitHub
    /// remains authoritative for branch protection and required checks; a
    /// 405/409/422 is returned as a typed blocked outcome, never retried.
    pub async fn merge_pull_request(
        &self,
        token: &str,
        owner: &str,
        repo: &str,
        input: MergePullRequest<'_>,
    ) -> Result<GitHubMergeOutcome, GitHubError> {
        validate_token(token)?;
        validate_repo_coordinates(owner, repo)?;
        validate_pull_number(input.number)?;
        if !is_object_id(input.expected_head_sha) {
            return Err(GitHubError::Invalid(
                "merge requires the exact expected head commit".into(),
            ));
        }
        if let Some(title) = input.commit_title {
            let cleaned = clean_single_line(title);
            if cleaned.is_empty() || cleaned.len() > MAX_PULL_REQUEST_TITLE_BYTES {
                return Err(GitHubError::Invalid(
                    "merge commit title must be 1-256 characters".into(),
                ));
            }
        }
        if let Some(message) = input.commit_message
            && message.len() > MAX_PULL_REQUEST_BODY_BYTES
        {
            return Err(GitHubError::Invalid(
                "merge commit message exceeds 64 KiB".into(),
            ));
        }
        let response = self
            .client
            .put(format!(
                "{}/repos/{owner}/{repo}/pulls/{}/merge",
                self.base_url, input.number
            ))
            .bearer_auth(token)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header("x-github-api-version", "2022-11-28")
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .json(&serde_json::json!({
                "sha": input.expected_head_sha,
                "merge_method": input.merge_method,
                "commit_title": input.commit_title.map(clean_single_line),
                "commit_message": input.commit_message.map(clean_body),
            }))
            .send()
            .await
            .map_err(|error| GitHubError::Upstream(safe_transport_error(&error)))?;
        let status = response.status();
        let headers = response.headers().clone();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(GitHubError::Unauthorized);
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(GitHubError::NotFound);
        }
        if status == reqwest::StatusCode::FORBIDDEN
            || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            if rate_limited(&headers) || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Err(GitHubError::RateLimited {
                    status_code: status.as_u16(),
                    next_allowed_at: next_allowed_at(&headers),
                });
            }
            return Err(GitHubError::Forbidden(
                "the token lacks merge permission or the organization restricts access".into(),
            ));
        }
        if status.is_redirection() {
            return Err(GitHubError::Redirected);
        }
        let body = read_bounded(response, MAX_REST_BODY_BYTES).await?;
        if status.is_success() {
            let raw: RawMergeResult = parse_json(&body, "merge result")?;
            if !raw.merged {
                return Err(GitHubError::MergeBlocked {
                    status_code: status.as_u16(),
                    message: bounded(
                        &clean_single_line(
                            raw.message.as_deref().unwrap_or("merge was not accepted"),
                        ),
                        MAX_GITHUB_MESSAGE_BYTES,
                    ),
                });
            }
            let merge_commit_sha = raw.sha.unwrap_or_default().to_ascii_lowercase();
            return Ok(GitHubMergeOutcome {
                merged: true,
                merge_commit_sha: if is_object_id(&merge_commit_sha) {
                    merge_commit_sha
                } else {
                    String::new()
                },
                message: bounded(
                    &clean_single_line(raw.message.as_deref().unwrap_or("merged")),
                    MAX_GITHUB_MESSAGE_BYTES,
                ),
            });
        }
        let message = safe_error_message(&body)
            .unwrap_or_else(|| format!("merge rejected with HTTP {}", status.as_u16()));
        Err(GitHubError::MergeBlocked {
            status_code: status.as_u16(),
            message,
        })
    }

    /// Fetch one bounded issue page. Pull requests are excluded because GitHub
    /// returns them from the issues endpoint. Callers validate each page's
    /// generation and ETag independently.
    pub async fn issue_page(
        &self,
        token: &str,
        owner: &str,
        repo: &str,
        page: usize,
        etag: Option<&str>,
    ) -> Result<IssuePage, GitHubError> {
        validate_token(token)?;
        validate_repo_coordinates(owner, repo)?;
        if page == 0 || page > MAX_ISSUE_PAGES {
            return Err(GitHubError::Invalid(
                "GitHub issue page is outside the supported bound".into(),
            ));
        }
        let mut request = self
            .client
            .get(format!("{}/repos/{owner}/{repo}/issues", self.base_url))
            .query(&[
                ("state", "all".to_string()),
                ("per_page", PER_PAGE.to_string()),
                ("page", page.to_string()),
                ("sort", "updated".to_string()),
                ("direction", "desc".to_string()),
            ]);
        if let Some(value) = etag.filter(|value| !value.is_empty()) {
            request = request.header(reqwest::header::IF_NONE_MATCH, value);
        }
        let response = self.execute(request, token).await?;
        if response.status == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(IssuePage {
                issues: Vec::new(),
                etag: etag.map(str::to_owned),
                not_modified: true,
                page_full: false,
            });
        }
        let response_etag = response
            .headers
            .get(reqwest::header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(|value| bounded(value, 512));
        let raw: Vec<RawIssue> = parse_json(&response.body, "issues")?;
        let page_full = raw.len() == PER_PAGE;
        let issues = raw
            .into_iter()
            .filter(|item| item.pull_request.is_none())
            .filter_map(RawIssue::into_issue)
            .collect();
        Ok(IssuePage {
            issues,
            etag: response_etag,
            not_modified: false,
            page_full,
        })
    }

    async fn execute(
        &self,
        request: reqwest::RequestBuilder,
        token: &str,
    ) -> Result<BoundedResponse, GitHubError> {
        self.execute_with_accept(request, token, "application/vnd.github+json")
            .await
    }

    async fn execute_with_accept(
        &self,
        request: reqwest::RequestBuilder,
        token: &str,
        accept: &'static str,
    ) -> Result<BoundedResponse, GitHubError> {
        validate_token(token)?;
        let response = request
            .bearer_auth(token)
            .header(reqwest::header::ACCEPT, accept)
            .header("x-github-api-version", "2022-11-28")
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .send()
            .await
            .map_err(|error| GitHubError::Upstream(safe_transport_error(&error)))?;
        let status = response.status();
        let headers = response.headers().clone();
        if status == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(BoundedResponse {
                status,
                headers,
                body: Vec::new(),
            });
        }
        if status.is_redirection() {
            return Err(GitHubError::Redirected);
        }
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(GitHubError::Unauthorized);
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(GitHubError::NotFound);
        }
        if status == reqwest::StatusCode::FORBIDDEN
            || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            if rate_limited(&headers) || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Err(GitHubError::RateLimited {
                    status_code: status.as_u16(),
                    next_allowed_at: next_allowed_at(&headers),
                });
            }
            return Err(GitHubError::Forbidden(
                "the token lacks the required permission or the organization restricts access"
                    .into(),
            ));
        }
        if status == reqwest::StatusCode::UNPROCESSABLE_ENTITY {
            let message = read_bounded(response, 8 * 1024)
                .await
                .ok()
                .and_then(|body| safe_error_message(&body))
                .unwrap_or_else(|| "the request was not accepted".into());
            return Err(GitHubError::Rejected {
                status_code: status.as_u16(),
                message,
            });
        }
        if !status.is_success() {
            return Err(GitHubError::Upstream(format!("HTTP {}", status.as_u16())));
        }
        let body = read_bounded(response, MAX_REST_BODY_BYTES).await?;
        Ok(BoundedResponse {
            status,
            headers,
            body,
        })
    }
}

pub fn validate_token(token: &str) -> Result<(), GitHubError> {
    if token.len() < 16
        || token.len() > MAX_TOKEN_BYTES
        || !token.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(GitHubError::Invalid(
            "GitHub token must be 16-512 printable characters".into(),
        ));
    }
    Ok(())
}

pub fn validate_repo_coordinates(owner: &str, repo: &str) -> Result<(), GitHubError> {
    if !repository_part(owner) || !repository_part(repo) {
        return Err(GitHubError::Invalid(
            "GitHub owner and repository must use letters, digits, '.', '_' or '-'".into(),
        ));
    }
    if repo == "." || repo == ".." || repo.ends_with(".git") {
        return Err(GitHubError::Invalid(
            "GitHub repository name is invalid".into(),
        ));
    }
    Ok(())
}

pub fn validate_default_branch(branch: &str) -> Result<(), GitHubError> {
    if branch.is_empty()
        || branch.len() > MAX_BRANCH_BYTES
        || branch.trim() != branch
        || branch.starts_with('-')
        || branch.starts_with('/')
        || branch.ends_with('/')
        || branch.contains("..")
        || branch.contains("//")
        || branch.chars().any(|character| {
            character.is_control()
                || character.is_whitespace()
                || matches!(character, '\\' | '~' | '^' | ':' | '?' | '*' | '[' | '%')
        })
    {
        return Err(GitHubError::Invalid(
            "GitHub default branch has an unsafe shape".into(),
        ));
    }
    Ok(())
}

fn repository_part(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_REPO_PART_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_repository_part(full_name: &str) -> bool {
    let mut parts = full_name.split('/');
    matches!((parts.next(), parts.next(), parts.next()), (Some(owner), Some(repo), None)
        if repository_part(owner) && repository_part(repo))
}

fn validate_base_url(base_url: &str, allow_loopback_http: bool) -> Result<String, GitHubError> {
    let url = reqwest::Url::parse(base_url)
        .map_err(|_| GitHubError::Invalid("base URL is not absolute".into()))?;
    if url.username() != "" || url.password().is_some() {
        return Err(GitHubError::Invalid(
            "base URL must not contain credentials".into(),
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(GitHubError::Invalid(
            "base URL must not contain a query or fragment".into(),
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| GitHubError::Invalid("base URL must have a host".into()))?;
    match url.scheme() {
        "https" => {}
        "http" if allow_loopback_http && loopback_host(host) => {}
        "http" => {
            return Err(GitHubError::Invalid(
                "plain HTTP is allowed only for a loopback test upstream".into(),
            ));
        }
        _ => {
            return Err(GitHubError::Invalid("base URL must use HTTPS".into()));
        }
    }
    if url.path() != "/" && !url.path().is_empty() {
        return Err(GitHubError::Invalid(
            "base URL must be a clean origin without a path".into(),
        ));
    }
    Ok(url.origin().ascii_serialization())
}

fn loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "[::1]"
        || host == "::1"
}

async fn read_bounded(response: reqwest::Response, limit: usize) -> Result<Vec<u8>, GitHubError> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| GitHubError::Upstream("response stream failed".into()))?;
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(GitHubError::InvalidResponse(
                "response exceeded the bounded size".into(),
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn parse_json<T: DeserializeOwned>(body: &[u8], what: &str) -> Result<T, GitHubError> {
    serde_json::from_slice(body)
        .map_err(|_| GitHubError::InvalidResponse(format!("{what} payload could not be decoded")))
}

fn parse_scopes(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .map(clean_single_line)
        .filter(|scope| !scope.is_empty())
        .map(|scope| bounded(&scope, 100))
        .take(MAX_SCOPES)
        .collect()
}

fn safe_transport_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "request timed out".into()
    } else if error.is_connect() {
        "connection failed".into()
    } else {
        "network request failed".into()
    }
}

fn rate_limited(headers: &HeaderMap) -> bool {
    headers
        .get("x-ratelimit-remaining")
        .and_then(|value| value.to_str().ok())
        == Some("0")
        || headers.contains_key("retry-after")
}

fn next_allowed_at(headers: &HeaderMap) -> Option<String> {
    let now = chrono::Utc::now();
    let reset = headers
        .get("x-ratelimit-reset")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok())
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0));
    let retry = headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .and_then(|seconds| {
            now.checked_add_signed(chrono::Duration::seconds(seconds.min(86_400) as i64))
        });
    reset
        .or(retry)
        .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn normalize_timestamp(value: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn safe_https_url(value: &str) -> String {
    match reqwest::Url::parse(value) {
        Ok(url) if url.scheme() == "https" && url.host_str().is_some() => bounded(value, 2048),
        _ => String::new(),
    }
}

fn bounded(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn bounded_body(value: &str) -> (String, bool) {
    let cleaned = clean_body(value);
    if cleaned.len() <= MAX_ISSUE_BODY_BYTES {
        return (cleaned, false);
    }
    let mut end = MAX_ISSUE_BODY_BYTES;
    while !cleaned.is_char_boundary(end) {
        end -= 1;
    }
    (cleaned[..end].to_owned(), true)
}

/// Untrusted single-line fields lose control and bidi-formatting characters.
fn clean_single_line(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            !character.is_control()
                && !matches!(*character, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        })
        .collect()
}

/// Issue bodies keep ordinary whitespace but lose other control characters.
fn clean_body(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control() || matches!(*character, '\n' | '\r' | '\t'))
        .collect()
}

#[derive(serde::Deserialize)]
struct RawUser {
    login: String,
}

#[derive(serde::Deserialize)]
struct RawRepository {
    full_name: String,
    #[serde(default)]
    private: bool,
    #[serde(default)]
    default_branch: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    pushed_at: Option<String>,
}

#[derive(serde::Deserialize)]
struct RawIssue {
    id: i64,
    number: u64,
    title: String,
    #[serde(default)]
    body: Option<String>,
    state: String,
    #[serde(default)]
    state_reason: Option<String>,
    #[serde(default)]
    labels: Vec<RawLabel>,
    #[serde(default)]
    user: Option<RawUser>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    comments: u32,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    closed_at: Option<String>,
    #[serde(default)]
    pull_request: Option<serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct RawLabel {
    #[serde(default)]
    name: String,
    #[serde(default)]
    color: Option<String>,
}

impl RawIssue {
    fn into_issue(self) -> Option<GitHubIssue> {
        if self.number == 0 || self.number > i64::MAX as u64 {
            return None;
        }
        let state = match self.state.as_str() {
            "open" => "open",
            "closed" => "closed",
            _ => return None,
        };
        let created_at = normalize_timestamp(&self.created_at)?;
        let updated_at = normalize_timestamp(&self.updated_at)?;
        let closed_at = self.closed_at.as_deref().and_then(normalize_timestamp);
        let (body, body_truncated) = bounded_body(self.body.as_deref().unwrap_or(""));
        let labels = self
            .labels
            .into_iter()
            .filter(|label| !label.name.trim().is_empty())
            .take(MAX_LABELS)
            .map(|label| GitHubLabel {
                name: bounded(&clean_single_line(label.name.trim()), MAX_LABEL_NAME_BYTES),
                color: label
                    .color
                    .filter(|color| {
                        color.len() == 6 && color.bytes().all(|byte| byte.is_ascii_hexdigit())
                    })
                    .map(|color| color.to_lowercase()),
            })
            .filter(|label| !label.name.is_empty())
            .collect();
        Some(GitHubIssue {
            number: self.number,
            github_id: self.id,
            title: bounded(&clean_single_line(self.title.trim()), MAX_ISSUE_TITLE_BYTES),
            body,
            body_truncated,
            state: state.into(),
            state_reason: self
                .state_reason
                .map(|reason| bounded(&clean_single_line(&reason), 64))
                .filter(|reason| !reason.is_empty()),
            labels,
            author: self
                .user
                .map(|user| bounded(&clean_single_line(&user.login), 255))
                .filter(|login| !login.is_empty()),
            html_url: safe_https_url(&self.html_url),
            comments: self.comments,
            created_at,
            updated_at,
            closed_at,
        })
    }
}

#[derive(serde::Deserialize)]
struct RawBranch {
    commit: RawCommitRef,
}

#[derive(serde::Deserialize)]
struct RawCommitRef {
    sha: String,
}

#[derive(serde::Deserialize)]
struct RawPullRequest {
    number: u64,
    #[serde(default)]
    title: String,
    state: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    merged: bool,
    #[serde(default)]
    mergeable: Option<bool>,
    #[serde(default)]
    mergeable_state: Option<String>,
    head: RawPullRequestRef,
    base: RawPullRequestRef,
    #[serde(default)]
    user: Option<RawUser>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    changed_files: u32,
    #[serde(default)]
    additions: u32,
    #[serde(default)]
    deletions: u32,
    #[serde(default)]
    comments: u32,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
}

#[derive(serde::Deserialize)]
struct RawPullRequestRef {
    #[serde(default)]
    r#ref: String,
    sha: String,
}

#[derive(serde::Deserialize)]
struct RawCheckRuns {
    #[serde(default)]
    total_count: u64,
    #[serde(default)]
    check_runs: Vec<RawCheckRun>,
}

#[derive(serde::Deserialize)]
struct RawCheckRun {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    details_url: Option<String>,
    #[serde(default)]
    started_at: Option<String>,
    #[serde(default)]
    completed_at: Option<String>,
    #[serde(default)]
    app: Option<RawApp>,
}

#[derive(serde::Deserialize)]
struct RawApp {
    #[serde(default)]
    slug: String,
}

#[derive(serde::Deserialize)]
struct RawCombinedStatus {
    #[serde(default)]
    state: String,
    #[serde(default)]
    total_count: u64,
    #[serde(default)]
    statuses: Vec<RawStatusContext>,
}

#[derive(serde::Deserialize)]
struct RawStatusContext {
    #[serde(default)]
    context: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    target_url: Option<String>,
}

#[derive(serde::Deserialize)]
struct RawIssueComment {
    id: u64,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    user: Option<RawUser>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    created_at: String,
}

impl RawIssueComment {
    fn into_comment(self, kind: &str) -> Option<GitHubComment> {
        let created_at = normalize_timestamp(&self.created_at)?;
        let (body, body_truncated) = bounded_body(self.body.as_deref().unwrap_or(""));
        Some(GitHubComment {
            id: self.id,
            kind: kind.into(),
            author: self
                .user
                .map(|user| bounded(&clean_single_line(&user.login), 255))
                .filter(|login| !login.is_empty()),
            body,
            body_truncated,
            path: None,
            line: None,
            html_url: safe_https_url(&self.html_url),
            created_at,
        })
    }
}

#[derive(serde::Deserialize)]
struct RawReviewComment {
    id: u64,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    user: Option<RawUser>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    line: Option<u32>,
    #[serde(default)]
    original_line: Option<u32>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    created_at: String,
}

impl RawReviewComment {
    fn into_comment(self) -> Option<GitHubComment> {
        let created_at = normalize_timestamp(&self.created_at)?;
        let (body, body_truncated) = bounded_body(self.body.as_deref().unwrap_or(""));
        Some(GitHubComment {
            id: self.id,
            kind: "review".into(),
            author: self
                .user
                .map(|user| bounded(&clean_single_line(&user.login), 255))
                .filter(|login| !login.is_empty()),
            body,
            body_truncated,
            path: self
                .path
                .map(|path| bounded(&clean_single_line(&path), MAX_COMMENT_PATH_BYTES))
                .filter(|path| !path.is_empty()),
            line: self.line.or(self.original_line),
            html_url: safe_https_url(&self.html_url),
            created_at,
        })
    }
}

const MAX_COMMENT_PATH_BYTES: usize = 1_000;
const MAX_GITHUB_MESSAGE_BYTES: usize = 512;

#[derive(serde::Deserialize)]
struct RawMergeResult {
    #[serde(default)]
    merged: bool,
    #[serde(default)]
    sha: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

fn project_pull_request(raw: RawPullRequest) -> Result<GitHubPullRequest, GitHubError> {
    if raw.number == 0 || raw.number > i64::MAX as u64 {
        return Err(GitHubError::InvalidResponse(
            "pull request number is invalid".into(),
        ));
    }
    let state = match raw.state.as_str() {
        "open" | "closed" => raw.state,
        _ => {
            return Err(GitHubError::InvalidResponse(
                "pull request state is unknown".into(),
            ));
        }
    };
    let head_sha = raw.head.sha.to_ascii_lowercase();
    let base_sha = raw.base.sha.to_ascii_lowercase();
    if !is_object_id(&head_sha) || !is_object_id(&base_sha) {
        return Err(GitHubError::InvalidResponse(
            "pull request revision is malformed".into(),
        ));
    }
    let created_at = normalize_timestamp(&raw.created_at)
        .ok_or_else(|| GitHubError::InvalidResponse("pull request createdAt is invalid".into()))?;
    let updated_at = normalize_timestamp(&raw.updated_at)
        .ok_or_else(|| GitHubError::InvalidResponse("pull request updatedAt is invalid".into()))?;
    Ok(GitHubPullRequest {
        number: raw.number,
        title: bounded(
            &clean_single_line(raw.title.trim()),
            MAX_PULL_REQUEST_TITLE_BYTES,
        ),
        state,
        draft: raw.draft,
        merged: raw.merged,
        mergeable: raw.mergeable,
        mergeable_state: bounded(
            &clean_single_line(raw.mergeable_state.as_deref().unwrap_or("unknown")),
            64,
        ),
        head_ref: bounded(&clean_single_line(&raw.head.r#ref), MAX_BRANCH_BYTES),
        head_sha,
        base_ref: bounded(&clean_single_line(&raw.base.r#ref), MAX_BRANCH_BYTES),
        base_sha,
        author: raw
            .user
            .map(|user| bounded(&clean_single_line(&user.login), 255))
            .filter(|login| !login.is_empty()),
        html_url: safe_https_url(&raw.html_url),
        changed_files: raw.changed_files,
        additions: raw.additions,
        deletions: raw.deletions,
        comments: raw.comments,
        created_at,
        updated_at,
    })
}

fn project_checks(
    reference: &str,
    raw_checks: RawCheckRuns,
    raw_status: RawCombinedStatus,
) -> Result<GitHubPullRequestChecks, GitHubError> {
    let check_runs_truncated = raw_checks.total_count > raw_checks.check_runs.len() as u64
        || raw_checks.check_runs.len() > MAX_PULL_REQUEST_CHECKS;
    let check_runs: Vec<GitHubCheckRun> = raw_checks
        .check_runs
        .into_iter()
        .take(MAX_PULL_REQUEST_CHECKS)
        .map(|run| GitHubCheckRun {
            id: run.id,
            name: bounded(&clean_single_line(&run.name), 256),
            status: bounded(
                &clean_single_line(run.status.trim()).to_ascii_lowercase(),
                32,
            ),
            conclusion: bounded(
                &clean_single_line(run.conclusion.as_deref().unwrap_or("unknown"))
                    .to_ascii_lowercase(),
                32,
            ),
            app: run
                .app
                .map(|app| bounded(&clean_single_line(&app.slug), 100))
                .filter(|slug| !slug.is_empty()),
            details_url: run
                .details_url
                .as_deref()
                .map(safe_https_url)
                .unwrap_or_default(),
            started_at: run.started_at.as_deref().and_then(normalize_timestamp),
            completed_at: run.completed_at.as_deref().and_then(normalize_timestamp),
        })
        .collect();
    let statuses_truncated = raw_status.total_count > raw_status.statuses.len() as u64
        || raw_status.statuses.len() > MAX_PULL_REQUEST_CHECKS;
    let statuses: Vec<GitHubStatusContext> = raw_status
        .statuses
        .into_iter()
        .take(MAX_PULL_REQUEST_CHECKS)
        .map(|status| GitHubStatusContext {
            context: bounded(&clean_single_line(&status.context), 256),
            state: bounded(
                &clean_single_line(status.state.trim()).to_ascii_lowercase(),
                32,
            ),
            description: status
                .description
                .as_deref()
                .map(|value| bounded(&clean_single_line(value), 512))
                .unwrap_or_default(),
            target_url: status
                .target_url
                .as_deref()
                .map(safe_https_url)
                .unwrap_or_default(),
        })
        .collect();

    let checks_failed = check_runs.iter().any(|run| {
        run.status == "completed"
            && !matches!(run.conclusion.as_str(), "success" | "neutral" | "skipped")
    });
    let checks_pending = check_runs.iter().any(|run| run.status != "completed");
    let status_failed = statuses
        .iter()
        .any(|status| matches!(status.state.as_str(), "failure" | "error"));
    let status_pending = statuses.iter().any(|status| status.state == "pending");
    let combined_status = bounded(
        &clean_single_line(raw_status.state.trim()).to_ascii_lowercase(),
        32,
    );
    let state = if checks_failed
        || status_failed
        || combined_status == "failure"
        || combined_status == "error"
    {
        "FAILURE"
    } else if checks_pending
        || status_pending
        || combined_status == "pending" && !statuses.is_empty()
        || check_runs_truncated
        || statuses_truncated
    {
        "PENDING"
    } else if check_runs.is_empty() && statuses.is_empty() {
        "NO_CHECKS"
    } else {
        "SUCCESS"
    };
    let ready = state == "SUCCESS" || state == "NO_CHECKS";
    let detail = match state {
        "SUCCESS" => "all reported checks succeeded".to_owned(),
        "NO_CHECKS" => {
            "GitHub reported no required check or commit status for this revision".to_owned()
        }
        "PENDING" => "checks are pending, truncated or unresolved".to_owned(),
        _ => "at least one reported check failed".to_owned(),
    };
    Ok(GitHubPullRequestChecks {
        head_sha: reference.to_ascii_lowercase(),
        state: state.into(),
        detail,
        ready,
        check_runs,
        check_runs_truncated,
        combined_status,
        statuses,
        statuses_truncated,
        fetched_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    })
}

fn validate_pull_number(number: u64) -> Result<(), GitHubError> {
    if number == 0 || number > i64::MAX as u64 {
        return Err(GitHubError::Invalid(
            "pull request number must be a positive integer".into(),
        ));
    }
    Ok(())
}

fn valid_github_login(login: &str) -> bool {
    !login.is_empty()
        && login.len() <= MAX_GITHUB_LOGIN_BYTES
        && !login.starts_with('-')
        && !login.ends_with('-')
        && login
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn read_truncated(body: Vec<u8>, limit: usize) -> Result<(Vec<u8>, bool), GitHubError> {
    if body.len() <= limit {
        return Ok((body, false));
    }
    let mut end = limit;
    while end > 0 && std::str::from_utf8(&body[..end]).is_err() {
        end -= 1;
    }
    let text = std::str::from_utf8(&body[..end])
        .map_err(|_| GitHubError::InvalidResponse("response was not UTF-8 text".into()))?;
    let cut = text
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(text.len());
    Ok((body[..cut].to_vec(), true))
}

fn safe_error_message(body: &[u8]) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct ErrorBody {
        message: Option<String>,
    }
    let parsed: ErrorBody = serde_json::from_slice(body).ok()?;
    let message = bounded(&clean_single_line(parsed.message.as_deref()?.trim()), 512);
    if message.is_empty() {
        None
    } else {
        Some(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        extract::{Query, State},
        http::{HeaderMap, StatusCode, header},
        response::{IntoResponse, Response},
        routing::get,
    };
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    const TOKEN: &str = "ghp_test_token_1234567890";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum UserMode {
        Ok,
        Unauthorized,
        Forbidden,
        NotFound,
        RateLimited,
        TooManyRequests,
        Redirect,
    }

    #[derive(Clone)]
    struct UserFake {
        mode: UserMode,
        redirect: Option<String>,
        last_auth: Arc<Mutex<Option<String>>>,
    }

    async fn user_handler(State(fake): State<UserFake>, headers: HeaderMap) -> Response {
        let auth = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        *fake.last_auth.lock().unwrap() = auth.clone();
        match fake.mode {
            UserMode::Ok => {
                let mut response = axum::Json(serde_json::json!({"login": "octo"})).into_response();
                response.headers_mut().insert(
                    "x-oauth-scopes",
                    header::HeaderValue::from_static("repo, read:org"),
                );
                response
            }
            UserMode::Unauthorized => {
                let echoed = auth
                    .as_deref()
                    .and_then(|value| value.strip_prefix("Bearer "))
                    .unwrap_or("unknown");
                (
                    StatusCode::UNAUTHORIZED,
                    axum::Json(serde_json::json!({"message": format!("bad credential {echoed}")})),
                )
                    .into_response()
            }
            UserMode::Forbidden => (
                StatusCode::FORBIDDEN,
                axum::Json(serde_json::json!({"message": "resource not accessible"})),
            )
                .into_response(),
            UserMode::NotFound => StatusCode::NOT_FOUND.into_response(),
            UserMode::RateLimited => {
                let mut response = StatusCode::FORBIDDEN.into_response();
                response.headers_mut().insert(
                    "x-ratelimit-remaining",
                    header::HeaderValue::from_static("0"),
                );
                let reset = (chrono::Utc::now().timestamp() + 120).to_string();
                response.headers_mut().insert(
                    "x-ratelimit-reset",
                    header::HeaderValue::from_str(&reset).unwrap(),
                );
                response
            }
            UserMode::TooManyRequests => {
                let mut response = StatusCode::TOO_MANY_REQUESTS.into_response();
                response
                    .headers_mut()
                    .insert(header::RETRY_AFTER, header::HeaderValue::from_static("45"));
                response
            }
            UserMode::Redirect => {
                let mut response = StatusCode::FOUND.into_response();
                response.headers_mut().insert(
                    header::LOCATION,
                    header::HeaderValue::from_str(fake.redirect.as_deref().unwrap()).unwrap(),
                );
                response
            }
        }
    }

    async fn user_server(
        mode: UserMode,
        redirect: Option<String>,
    ) -> (String, Arc<Mutex<Option<String>>>) {
        let last_auth = Arc::new(Mutex::new(None));
        let app = Router::new()
            .route("/user", get(user_handler))
            .with_state(UserFake {
                mode,
                redirect,
                last_auth: last_auth.clone(),
            });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (url, last_auth)
    }

    fn issue(number: u64) -> serde_json::Value {
        serde_json::json!({
            "id": 10_000 + number,
            "number": number,
            "title": format!("Issue {number}"),
            "body": format!("Body {number}"),
            "state": "open",
            "labels": [{"name": "bug", "color": "ff0000"}],
            "user": {"login": "octo"},
            "html_url": format!("https://github.com/acme/widgets/issues/{number}"),
            "comments": 0,
            "created_at": "2026-10-01T00:00:00Z",
            "updated_at": "2026-10-02T00:00:00Z",
            "closed_at": null
        })
    }

    #[derive(Clone)]
    struct IssueFake {
        honor_etag: bool,
        seen_if_none_match: Arc<Mutex<Vec<Option<String>>>>,
        token_seen: Arc<Mutex<Vec<Option<String>>>>,
    }

    async fn issue_handler(
        State(fake): State<IssueFake>,
        Query(query): Query<HashMap<String, String>>,
        headers: HeaderMap,
    ) -> Response {
        fake.token_seen.lock().unwrap().push(
            headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        );
        let if_none_match = headers
            .get(header::IF_NONE_MATCH)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        fake.seen_if_none_match
            .lock()
            .unwrap()
            .push(if_none_match.clone());
        if fake.honor_etag && if_none_match.as_deref() == Some("\"etag-1\"") {
            return StatusCode::NOT_MODIFIED.into_response();
        }
        let page = query
            .get("page")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1);
        let items = if page == 1 {
            let mut items: Vec<serde_json::Value> = (1..=99).map(issue).collect();
            items.push(serde_json::json!({
                "id": 99_999,
                "number": 999,
                "title": "A pull request",
                "state": "open",
                "pull_request": {"url": "https://api.github.com/repos/acme/widgets/pulls/999"},
                "created_at": "2026-10-01T00:00:00Z",
                "updated_at": "2026-10-02T00:00:00Z"
            }));
            items
        } else {
            vec![issue(100)]
        };
        let mut response = axum::Json(items).into_response();
        if page == 1 {
            response
                .headers_mut()
                .insert(header::ETAG, header::HeaderValue::from_static("\"etag-1\""));
        }
        response
    }

    async fn issue_server(honor_etag: bool) -> (String, IssueFake) {
        let fake = IssueFake {
            honor_etag,
            seen_if_none_match: Arc::new(Mutex::new(Vec::new())),
            token_seen: Arc::new(Mutex::new(Vec::new())),
        };
        let app = Router::new()
            .route("/repos/{owner}/{repo}/issues", get(issue_handler))
            .with_state(fake.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (url, fake)
    }

    #[tokio::test]
    async fn authenticates_with_bearer_token_and_reports_scopes() {
        let (url, last_auth) = user_server(UserMode::Ok, None).await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        let user = client.authenticated_user(TOKEN).await.unwrap();
        assert_eq!(user.login, "octo");
        assert!(user.scopes_reported);
        assert_eq!(user.scopes, vec!["repo", "read:org"]);
        assert_eq!(
            last_auth.lock().unwrap().as_deref(),
            Some("Bearer ghp_test_token_1234567890")
        );
    }

    #[tokio::test]
    async fn classifies_unauthorized_forbidden_not_found_and_rate_limits() {
        let cases = [
            (UserMode::Unauthorized, "DENIED", Some(401)),
            (UserMode::Forbidden, "DENIED", Some(403)),
            (UserMode::NotFound, "NOT_FOUND", Some(404)),
            (UserMode::RateLimited, "RATE_LIMITED", Some(403)),
            (UserMode::TooManyRequests, "RATE_LIMITED", Some(429)),
        ];
        for (mode, kind, code) in cases {
            let (url, _) = user_server(mode, None).await;
            let client = GitHubClient::with_base_url(&url, true).unwrap();
            let error = client.authenticated_user(TOKEN).await.unwrap_err();
            assert_eq!(error.kind(), kind, "mode {mode:?}");
            assert_eq!(error.status_code(), code, "mode {mode:?}");
            if mode == UserMode::RateLimited {
                assert!(error.next_allowed_at().is_some());
                assert!(error.detail().contains("rate limit"));
            }
        }
    }

    #[tokio::test]
    async fn paginates_bounded_issues_excludes_pull_requests_and_reuses_etag() {
        let (url, fake) = issue_server(true).await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        let first = client
            .issue_page(TOKEN, "acme", "widgets", 1, None)
            .await
            .unwrap();
        assert_eq!(first.issues.len(), 99);
        assert!(!first.not_modified);
        assert!(first.page_full);
        assert_eq!(first.etag.as_deref(), Some("\"etag-1\""));
        assert!(!first.issues.iter().any(|issue| issue.number == 999));
        assert!(
            first
                .issues
                .iter()
                .all(|issue| issue.labels[0].name == "bug")
        );
        let second_page = client
            .issue_page(TOKEN, "acme", "widgets", 2, None)
            .await
            .unwrap();
        assert_eq!(second_page.issues.len(), 1);
        assert!(!second_page.page_full);
        assert!(!second_page.not_modified);
        assert_eq!(
            fake.token_seen
                .lock()
                .unwrap()
                .first()
                .cloned()
                .flatten()
                .as_deref(),
            Some("Bearer ghp_test_token_1234567890")
        );
        let second = client
            .issue_page(TOKEN, "acme", "widgets", 1, first.etag.as_deref())
            .await
            .unwrap();
        assert!(second.not_modified);
        assert!(second.issues.is_empty());
        assert_eq!(
            fake.seen_if_none_match
                .lock()
                .unwrap()
                .last()
                .cloned()
                .flatten()
                .as_deref(),
            Some("\"etag-1\"")
        );
    }

    #[tokio::test]
    async fn lists_private_repositories_and_filters_the_query() {
        let app = Router::new().route(
            "/user/repos",
            get(|| async {
                axum::Json(serde_json::json!([
                    {
                        "full_name": "acme/private-widgets",
                        "private": true,
                        "default_branch": "develop",
                        "html_url": "https://github.com/acme/private-widgets",
                        "pushed_at": "2026-10-01T00:00:00Z"
                    },
                    {
                        "full_name": "acme/public-tools",
                        "private": false,
                        "default_branch": "main",
                        "html_url": "https://github.com/acme/public-tools",
                        "pushed_at": null
                    }
                ]))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        let all = client.repositories(TOKEN, None).await.unwrap();
        assert_eq!(all.len(), 2);
        assert!(all[0].private);
        assert_eq!(all[0].default_branch, "develop");
        let filtered = client.repositories(TOKEN, Some("private")).await.unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].full_name, "acme/private-widgets");
        assert!(
            client
                .repositories(TOKEN, Some("missing"))
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn refuses_redirects_without_contacting_the_target() {
        let target = Arc::new(Mutex::new(0usize));
        let target_state = target.clone();
        let target_app = Router::new().route(
            "/user",
            get(move || {
                let target = target_state.clone();
                async move {
                    *target.lock().unwrap() += 1;
                    StatusCode::OK
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_url = format!("http://{}/user", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, target_app).await.unwrap();
        });
        let (url, _) = user_server(UserMode::Redirect, Some(target_url)).await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        let error = client.authenticated_user(TOKEN).await.unwrap_err();
        assert!(matches!(error, GitHubError::Redirected));
        assert_eq!(*target.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn errors_never_echo_the_token_or_response_body() {
        let (url, _) = user_server(UserMode::Unauthorized, None).await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        let error = client.authenticated_user(TOKEN).await.unwrap_err();
        assert!(!error.detail().contains(TOKEN));
        assert!(!error.to_string().contains(TOKEN));
        assert!(!format!("{client:?}").contains(TOKEN));
    }

    #[test]
    fn rejects_unsafe_bases_and_tokens() {
        assert!(GitHubClient::with_base_url("http://example.com", true).is_err());
        assert!(GitHubClient::with_base_url("http://127.0.0.1:9", false).is_err());
        assert!(GitHubClient::with_base_url("http://127.0.0.1:9", true).is_ok());
        assert!(GitHubClient::with_base_url("https://api.github.com/path", false).is_err());
        assert!(GitHubClient::with_base_url("https://token@api.github.com", false).is_err());
        assert!(GitHubClient::with_base_url("https://api.github.com", false).is_ok());
        assert!(validate_token("short").is_err());
        assert!(validate_token("has whitespace in the token value").is_err());
    }

    #[test]
    fn validates_repository_and_branch_shapes() {
        assert!(validate_repo_coordinates("acme", "widgets").is_ok());
        assert!(validate_repo_coordinates("acme", "widgets.git").is_err());
        assert!(validate_repo_coordinates("acme/extra", "widgets").is_err());
        assert!(validate_repo_coordinates("../acme", "widgets").is_err());
        assert!(validate_default_branch("main").is_ok());
        assert!(validate_default_branch("release/1.0").is_ok());
        assert!(validate_default_branch("bad branch").is_err());
        assert!(validate_default_branch("--force").is_err());
        assert!(validate_default_branch("main..next").is_err());
        assert!(validate_default_branch("candidate/%2e%2e").is_err());
        assert!(valid_github_login("octocat"));
        assert!(valid_github_login("octo-cat"));
        assert!(!valid_github_login("octo cat"));
        assert!(!valid_github_login("../octocat"));
        assert!(validate_pull_number(42).is_ok());
        assert!(validate_pull_number(0).is_err());
    }

    const PR_HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const PR_BASE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const PR_HEAD_REF: &str = "candidate/office-ux";
    const PR_BASE_REF: &str = "main";

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum MergeMode {
        Ok,
        Forbidden,
        Blocked,
    }

    #[derive(Clone)]
    struct PullFake {
        merge_mode: MergeMode,
        diff: String,
        token_seen: Arc<Mutex<Vec<Option<String>>>>,
        merge_body: Arc<Mutex<Option<serde_json::Value>>>,
        create_body: Arc<Mutex<Option<serde_json::Value>>>,
        review_body: Arc<Mutex<Option<serde_json::Value>>>,
        comment_body: Arc<Mutex<Option<serde_json::Value>>>,
    }

    fn pull_payload() -> serde_json::Value {
        serde_json::json!({
            "number": 42,
            "title": "Candidate 42",
            "state": "open",
            "draft": false,
            "merged": false,
            "mergeable": true,
            "mergeable_state": "clean",
            "head": {"ref": PR_HEAD_REF, "sha": PR_HEAD},
            "base": {"ref": PR_BASE_REF, "sha": PR_BASE},
            "user": {"login": "octo"},
            "html_url": "https://github.com/acme/widgets/pull/42",
            "changed_files": 3,
            "additions": 12,
            "deletions": 4,
            "comments": 1,
            "created_at": "2026-10-01T00:00:00Z",
            "updated_at": "2026-10-02T00:00:00Z"
        })
    }

    fn record_auth(fake: &PullFake, headers: &HeaderMap) {
        fake.token_seen.lock().unwrap().push(
            headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        );
    }

    async fn pull_pr_handler(State(fake): State<PullFake>, headers: HeaderMap) -> Response {
        record_auth(&fake, &headers);
        if headers
            .get(header::ACCEPT)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("diff"))
        {
            return ([(header::CONTENT_TYPE, "text/plain")], fake.diff.clone()).into_response();
        }
        axum::Json(pull_payload()).into_response()
    }

    async fn pull_diff_handler(State(fake): State<PullFake>, headers: HeaderMap) -> Response {
        record_auth(&fake, &headers);
        ([(header::CONTENT_TYPE, "text/plain")], fake.diff).into_response()
    }

    async fn pull_checks_handler(State(fake): State<PullFake>, headers: HeaderMap) -> Response {
        record_auth(&fake, &headers);
        axum::Json(serde_json::json!({
            "total_count": 2,
            "check_runs": [
                {"id": 1, "name": "lint", "status": "completed", "conclusion": "success",
                 "details_url": "https://github.com/acme/widgets/actions/1",
                 "started_at": "2026-10-01T00:00:00Z", "completed_at": "2026-10-01T00:05:00Z",
                 "app": {"slug": "github-actions"}},
                {"id": 2, "name": "test", "status": "completed", "conclusion": "success",
                 "details_url": "https://github.com/acme/widgets/actions/2",
                 "started_at": "2026-10-01T00:00:00Z", "completed_at": "2026-10-01T00:06:00Z",
                 "app": {"slug": "github-actions"}}
            ]
        }))
        .into_response()
    }

    async fn pull_status_handler(State(fake): State<PullFake>, headers: HeaderMap) -> Response {
        record_auth(&fake, &headers);
        axum::Json(serde_json::json!({
            "state": "success",
            "total_count": 1,
            "statuses": [
                {"context": "ci/build", "state": "success", "description": "ok",
                 "target_url": "https://ci.example.invalid/1"}
            ]
        }))
        .into_response()
    }

    async fn pull_comments_handler(
        State(fake): State<PullFake>,
        headers: HeaderMap,
        axum::extract::Path((_owner, _repo, number)): axum::extract::Path<(String, String, u64)>,
    ) -> Response {
        record_auth(&fake, &headers);
        if number == 42 {
            axum::Json(serde_json::json!([
                {"id": 1, "body": "Root comment\nwith two lines", "user": {"login": "octo"},
                 "html_url": "https://github.com/acme/widgets/pull/42#issuecomment-1",
                 "created_at": "2026-10-01T00:00:00Z"}
            ]))
            .into_response()
        } else {
            StatusCode::NOT_FOUND.into_response()
        }
    }

    async fn pull_review_comments_handler(
        State(fake): State<PullFake>,
        headers: HeaderMap,
    ) -> Response {
        record_auth(&fake, &headers);
        axum::Json(serde_json::json!([
            {"id": 2, "body": "Please rename this", "user": {"login": "reviewer"},
             "path": "src/main.rs", "line": 7,
             "html_url": "https://github.com/acme/widgets/pull/42#discussion_r2",
             "created_at": "2026-10-01T01:00:00Z"}
        ]))
        .into_response()
    }

    async fn pull_merge_handler(
        State(fake): State<PullFake>,
        headers: HeaderMap,
        axum::Json(body): axum::Json<serde_json::Value>,
    ) -> Response {
        record_auth(&fake, &headers);
        *fake.merge_body.lock().unwrap() = Some(body);
        match fake.merge_mode {
            MergeMode::Ok => axum::Json(serde_json::json!({
                "merged": true,
                "sha": "c".repeat(40),
                "message": "Pull Request successfully merged"
            }))
            .into_response(),
            MergeMode::Forbidden => (
                StatusCode::FORBIDDEN,
                axum::Json(serde_json::json!({"message": "Resource not accessible by integration"})),
            )
                .into_response(),
            MergeMode::Blocked => (
                StatusCode::CONFLICT,
                axum::Json(serde_json::json!({
                    "message": "Pull request is not mergeable: required status check \"test\" is expected"
                })),
            )
                .into_response(),
        }
    }

    async fn pull_comment_create_handler(
        State(fake): State<PullFake>,
        headers: HeaderMap,
        axum::Json(body): axum::Json<serde_json::Value>,
    ) -> Response {
        record_auth(&fake, &headers);
        *fake.comment_body.lock().unwrap() = Some(body);
        (
            StatusCode::CREATED,
            axum::Json(serde_json::json!({
                "id": 3,
                "body": "A bounded comment",
                "user": {"login": "octo"},
                "html_url": "https://github.com/acme/widgets/pull/42#issuecomment-3",
                "created_at": "2026-10-01T02:00:00Z"
            })),
        )
            .into_response()
    }

    async fn pull_create_handler(
        State(fake): State<PullFake>,
        headers: HeaderMap,
        axum::Json(body): axum::Json<serde_json::Value>,
    ) -> Response {
        record_auth(&fake, &headers);
        *fake.create_body.lock().unwrap() = Some(body);
        (StatusCode::CREATED, axum::Json(pull_payload())).into_response()
    }

    async fn pull_review_handler(
        State(fake): State<PullFake>,
        headers: HeaderMap,
        axum::Json(body): axum::Json<serde_json::Value>,
    ) -> Response {
        record_auth(&fake, &headers);
        *fake.review_body.lock().unwrap() = Some(body);
        (StatusCode::CREATED, axum::Json(serde_json::json!({}))).into_response()
    }

    async fn pull_branch_handler(State(fake): State<PullFake>, headers: HeaderMap) -> Response {
        record_auth(&fake, &headers);
        axum::Json(serde_json::json!({"name": PR_HEAD_REF, "commit": {"sha": PR_HEAD}}))
            .into_response()
    }

    async fn pull_list_handler(State(fake): State<PullFake>, headers: HeaderMap) -> Response {
        record_auth(&fake, &headers);
        axum::Json(serde_json::json!([pull_payload()])).into_response()
    }

    async fn pull_server(mode: MergeMode, diff: &str) -> (String, PullFake) {
        let fake = PullFake {
            merge_mode: mode,
            diff: diff.into(),
            token_seen: Arc::new(Mutex::new(Vec::new())),
            merge_body: Arc::new(Mutex::new(None)),
            create_body: Arc::new(Mutex::new(None)),
            review_body: Arc::new(Mutex::new(None)),
            comment_body: Arc::new(Mutex::new(None)),
        };
        let app = Router::new()
            .route(
                "/repos/{owner}/{repo}/branches/{*branch}",
                get(pull_branch_handler),
            )
            .route(
                "/repos/{owner}/{repo}/pulls",
                get(pull_list_handler).post(pull_create_handler),
            )
            .route("/repos/{owner}/{repo}/pulls/{number}", get(pull_pr_handler))
            .route(
                "/repos/{owner}/{repo}/pulls/{number}/comments",
                get(pull_review_comments_handler),
            )
            .route(
                "/repos/{owner}/{repo}/pulls/{number}/requested_reviewers",
                axum::routing::post(pull_review_handler),
            )
            .route(
                "/repos/{owner}/{repo}/pulls/{number}/merge",
                axum::routing::put(pull_merge_handler),
            )
            .route(
                "/repos/{owner}/{repo}/issues/{number}/comments",
                get(pull_comments_handler).post(pull_comment_create_handler),
            )
            .route(
                "/repos/{owner}/{repo}/commits/{reference}/check-runs",
                get(pull_checks_handler),
            )
            .route(
                "/repos/{owner}/{repo}/commits/{reference}/status",
                get(pull_status_handler),
            )
            .route("/diff", get(pull_diff_handler))
            .with_state(fake.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (url, fake)
    }

    #[tokio::test]
    async fn reads_pull_request_checks_diff_comments_and_branch_identity() {
        let (url, fake) = pull_server(
            MergeMode::Ok,
            "diff --git a/src/main.rs b/src/main.rs\n+hello\n",
        )
        .await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        let pr = client
            .pull_request(TOKEN, "acme", "widgets", 42)
            .await
            .unwrap();
        assert_eq!(pr.number, 42);
        assert_eq!(pr.head_sha, PR_HEAD);
        assert_eq!(pr.head_ref, PR_HEAD_REF);
        assert_eq!(pr.mergeable, Some(true));
        assert_eq!(pr.mergeable_state, "clean");
        assert_eq!(pr.html_url, "https://github.com/acme/widgets/pull/42");

        let branch = client
            .branch_commit(TOKEN, "acme", "widgets", PR_HEAD_REF)
            .await
            .unwrap();
        assert_eq!(branch.commit_sha, PR_HEAD);

        let pulls = client
            .open_pull_requests_for_head(TOKEN, "acme", "widgets", PR_HEAD_REF)
            .await
            .unwrap();
        assert_eq!(pulls.len(), 1);
        assert_eq!(pulls[0].number, 42);

        let checks = client
            .pull_request_checks(TOKEN, "acme", "widgets", PR_HEAD)
            .await
            .unwrap();
        assert_eq!(checks.state, "SUCCESS");
        assert!(checks.ready);
        assert!(!checks.check_runs_truncated);
        assert_eq!(checks.check_runs.len(), 2);
        assert_eq!(checks.statuses.len(), 1);
        assert_eq!(checks.statuses[0].context, "ci/build");

        let diff = client
            .pull_request_diff(TOKEN, "acme", "widgets", 42)
            .await
            .unwrap();
        assert!(diff.patch.contains("+hello"));
        assert!(!diff.truncated);

        let comments = client
            .pull_request_comments(TOKEN, "acme", "widgets", 42)
            .await
            .unwrap();
        assert_eq!(comments.comments.len(), 2);
        assert_eq!(comments.comments[0].kind, "issue");
        assert_eq!(comments.comments[1].kind, "review");
        assert_eq!(comments.comments[1].path.as_deref(), Some("src/main.rs"));
        assert_eq!(comments.comments[1].line, Some(7));
        assert!(!comments.truncated);

        let first_auth = fake
            .token_seen
            .lock()
            .unwrap()
            .first()
            .cloned()
            .flatten()
            .unwrap();
        assert_eq!(first_auth, format!("Bearer {TOKEN}"));
    }

    #[tokio::test]
    async fn merges_only_with_exact_head_and_reports_typed_blocks() {
        let (url, fake) = pull_server(MergeMode::Ok, "").await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        let outcome = client
            .merge_pull_request(
                TOKEN,
                "acme",
                "widgets",
                MergePullRequest {
                    number: 42,
                    expected_head_sha: PR_HEAD,
                    merge_method: GitHubMergeMethod::Squash,
                    commit_title: Some("Candidate 42"),
                    commit_message: None,
                },
            )
            .await
            .unwrap();
        assert!(outcome.merged);
        assert_eq!(outcome.merge_commit_sha, "c".repeat(40));
        let body = fake.merge_body.lock().unwrap().clone().unwrap();
        assert_eq!(body["sha"], PR_HEAD);
        assert_eq!(body["merge_method"], "squash");
        assert_eq!(body["commit_title"], "Candidate 42");

        let (url, _) = pull_server(MergeMode::Blocked, "").await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        let blocked = client
            .merge_pull_request(
                TOKEN,
                "acme",
                "widgets",
                MergePullRequest {
                    number: 42,
                    expected_head_sha: PR_HEAD,
                    merge_method: GitHubMergeMethod::Merge,
                    commit_title: None,
                    commit_message: None,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(blocked.kind(), "BLOCKED");
        assert!(blocked.detail().contains("not mergeable"));
        assert!(!blocked.detail().contains(TOKEN));

        let (url, _) = pull_server(MergeMode::Forbidden, "").await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        let denied = client
            .merge_pull_request(
                TOKEN,
                "acme",
                "widgets",
                MergePullRequest {
                    number: 42,
                    expected_head_sha: PR_HEAD,
                    merge_method: GitHubMergeMethod::Merge,
                    commit_title: None,
                    commit_message: None,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(denied.kind(), "DENIED");
    }

    #[tokio::test]
    async fn creates_pull_request_comments_and_review_requests() {
        let (url, fake) = pull_server(MergeMode::Ok, "").await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        let pr = client
            .create_pull_request(
                TOKEN,
                "acme",
                "widgets",
                NewPullRequest {
                    title: "Candidate 42",
                    body: "Bounded summary",
                    head: PR_HEAD_REF,
                    base: PR_BASE_REF,
                    draft: false,
                },
            )
            .await
            .unwrap();
        assert_eq!(pr.number, 42);
        let body = fake.create_body.lock().unwrap().clone().unwrap();
        assert_eq!(body["head"], PR_HEAD_REF);
        assert_eq!(body["base"], PR_BASE_REF);
        assert_eq!(body["draft"], false);
        assert_eq!(body["body"], "Bounded summary");

        let reviewers = client
            .request_pull_request_reviewers(
                TOKEN,
                "acme",
                "widgets",
                42,
                &["reviewer".into(), "reviewer".into()],
            )
            .await
            .unwrap();
        assert_eq!(reviewers, vec!["reviewer"]);
        assert_eq!(
            fake.review_body.lock().unwrap().clone().unwrap()["reviewers"],
            serde_json::json!(["reviewer"])
        );

        let comment = client
            .create_pull_request_comment(TOKEN, "acme", "widgets", 42, "Bounded finding")
            .await
            .unwrap();
        assert_eq!(comment.kind, "issue");
        assert_eq!(comment.author.as_deref(), Some("octo"));
        assert_eq!(
            fake.comment_body.lock().unwrap().clone().unwrap()["body"],
            "Bounded finding"
        );
    }

    #[tokio::test]
    async fn bounds_large_pull_request_diffs() {
        let body = format!(
            "{}\n",
            "+".to_owned() + &"x".repeat(MAX_PULL_REQUEST_DIFF_BYTES + 1024)
        );
        let (url, _) = pull_server(MergeMode::Ok, &body).await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        let diff = client
            .pull_request_diff(TOKEN, "acme", "widgets", 42)
            .await
            .unwrap();
        assert!(diff.truncated);
        assert!(diff.bytes <= MAX_PULL_REQUEST_DIFF_BYTES);
        assert_eq!(diff.patch.len(), diff.bytes);
    }

    #[test]
    fn classifies_pending_failed_and_missing_checks() {
        let raw = |status: &str, conclusion: Option<&str>| RawCheckRuns {
            total_count: 1,
            check_runs: vec![RawCheckRun {
                id: 1,
                name: "test".into(),
                status: status.into(),
                conclusion: conclusion.map(str::to_owned),
                details_url: None,
                started_at: None,
                completed_at: None,
                app: None,
            }],
        };
        let status = || RawCombinedStatus {
            state: "pending".into(),
            total_count: 0,
            statuses: Vec::new(),
        };
        let pending = project_checks(PR_HEAD, raw("in_progress", None), status()).unwrap();
        assert_eq!(pending.state, "PENDING");
        assert!(!pending.ready);
        let failed = project_checks(
            PR_HEAD,
            raw("completed", Some("failure")),
            RawCombinedStatus {
                state: "failure".into(),
                total_count: 0,
                statuses: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(failed.state, "FAILURE");
        assert!(!failed.ready);
        let missing = project_checks(
            PR_HEAD,
            RawCheckRuns {
                total_count: 0,
                check_runs: Vec::new(),
            },
            RawCombinedStatus {
                state: "pending".into(),
                total_count: 0,
                statuses: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(missing.state, "NO_CHECKS");
        assert!(missing.ready);
    }

    #[tokio::test]
    async fn rejects_unsafe_pull_request_inputs_without_contacting_github() {
        let (url, fake) = pull_server(MergeMode::Ok, "").await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        assert!(
            client
                .pull_request(TOKEN, "acme", "widgets", 0)
                .await
                .is_err()
        );
        assert!(
            client
                .create_pull_request(
                    TOKEN,
                    "acme",
                    "widgets",
                    NewPullRequest {
                        title: "",
                        body: "",
                        head: PR_HEAD_REF,
                        base: PR_BASE_REF,
                        draft: false,
                    },
                )
                .await
                .is_err()
        );
        assert!(
            client
                .merge_pull_request(
                    TOKEN,
                    "acme",
                    "widgets",
                    MergePullRequest {
                        number: 42,
                        expected_head_sha: "short",
                        merge_method: GitHubMergeMethod::Merge,
                        commit_title: None,
                        commit_message: None,
                    },
                )
                .await
                .is_err()
        );
        assert!(
            client
                .request_pull_request_reviewers(
                    TOKEN,
                    "acme",
                    "widgets",
                    42,
                    &["bad reviewer".into()],
                )
                .await
                .is_err()
        );
        assert!(fake.token_seen.lock().unwrap().is_empty());
    }
}
