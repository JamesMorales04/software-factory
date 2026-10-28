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
            Self::Redirected | Self::Upstream(_) | Self::InvalidResponse(_) => "ERROR",
        }
    }

    pub fn status_code(&self) -> Option<u16> {
        match self {
            Self::Unauthorized => Some(401),
            Self::Forbidden(_) => Some(403),
            Self::NotFound => Some(404),
            Self::RateLimited { status_code, .. } => Some(*status_code),
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
    pub id: Option<i64>,
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
    pub author_id: Option<i64>,
    pub author_association: Option<String>,
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
            "github_id": self.github_id,
            "title": self.title,
            "body": self.body,
            "body_truncated": self.body_truncated,
            "state": self.state,
            "state_reason": self.state_reason,
            "labels": self.labels,
            "author": self.author,
            "author_id": self.author_id,
            "author_association": self.author_association,
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
                    id: item.id.filter(|id| *id > 0),
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

    /// Fetch one exact issue for the pre-proposal freshness check. Pull
    /// requests are refused because they are not eligible issue intake.
    pub async fn get_issue(
        &self,
        token: &str,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> Result<GitHubIssue, GitHubError> {
        validate_token(token)?;
        validate_repo_coordinates(owner, repo)?;
        if number == 0 || number > i64::MAX as u64 {
            return Err(GitHubError::Invalid(
                "GitHub issue number is outside the supported bound".into(),
            ));
        }
        let request = self
            .client
            .get(format!(
                "{}/repos/{owner}/{repo}/issues/{number}",
                self.base_url
            ))
            .header(reqwest::header::ACCEPT, "application/vnd.github+json");
        let response = self.execute(request, token).await?;
        let raw: RawIssue = parse_json(&response.body, "issue")?;
        if raw.pull_request.is_some() {
            return Err(GitHubError::NotFound);
        }
        raw.into_issue()
            .ok_or_else(|| GitHubError::InvalidResponse("issue fields were invalid".into()))
    }

    async fn execute(
        &self,
        request: reqwest::RequestBuilder,
        token: &str,
    ) -> Result<BoundedResponse, GitHubError> {
        validate_token(token)?;
        let response = request
            .bearer_auth(token)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
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
                || matches!(character, '\\' | '~' | '^' | ':' | '?' | '*' | '[')
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
    #[serde(default)]
    id: Option<i64>,
    login: String,
}

#[derive(serde::Deserialize)]
struct RawRepository {
    #[serde(default)]
    id: Option<i64>,
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
    author_association: Option<String>,
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
        let author_id = self
            .user
            .as_ref()
            .and_then(|user| user.id)
            .filter(|id| *id > 0);
        let author = self
            .user
            .map(|user| bounded(&clean_single_line(&user.login), 255))
            .filter(|login| !login.is_empty());
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
            author,
            author_id,
            author_association: self
                .author_association
                .map(|association| bounded(&clean_single_line(&association), 64))
                .filter(|association| !association.is_empty()),
            html_url: safe_https_url(&self.html_url),
            comments: self.comments,
            created_at,
            updated_at,
            closed_at,
        })
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
            "user": {"id": 7, "login": "octo"},
            "author_association": "MEMBER",
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

    async fn single_issue_handler(
        State(fake): State<IssueFake>,
        axum::extract::Path((_owner, _repo, number)): axum::extract::Path<(String, String, u64)>,
    ) -> Response {
        fake.token_seen.lock().unwrap().push(
            axum::http::HeaderMap::new()
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        );
        if number == 999 {
            return axum::Json(serde_json::json!({
                "id": 10999,
                "number": 999,
                "title": "PR",
                "state": "open",
                "pull_request": {"url": "https://api.github.com/pulls/999"},
                "created_at": "2026-10-01T00:00:00Z",
                "updated_at": "2026-10-02T00:00:00Z"
            }))
            .into_response();
        }
        axum::Json(issue(number)).into_response()
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
            .route(
                "/repos/{owner}/{repo}/issues/{number}",
                get(single_issue_handler),
            )
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
    async fn fetches_one_exact_issue_and_refuses_pull_requests() {
        let (url, _fake) = issue_server(true).await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        let issue = client.get_issue(TOKEN, "acme", "widgets", 5).await.unwrap();
        assert_eq!(issue.number, 5);
        assert_eq!(issue.author_id, Some(7));
        assert_eq!(issue.author_association.as_deref(), Some("MEMBER"));
        assert_eq!(issue.content_hash().len(), 64);
        let error = client
            .get_issue(TOKEN, "acme", "widgets", 999)
            .await
            .unwrap_err();
        assert!(matches!(error, GitHubError::NotFound));
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
    }
}
