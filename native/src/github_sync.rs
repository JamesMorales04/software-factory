//! Core-owned orchestration for bounded GitHub synchronization.
//!
//! The caller decrypts a credential into [`GitHubCredential`], which owns the
//! plaintext and its scoped log-redaction registration. Every request and
//! persistence step is fenced by that credential's exact connection revision,
//! so a result fetched with credential A can never be published under a later
//! credential B. Rate-limit backoff is credential-wide and checked before any
//! upstream request.

use std::collections::HashSet;

use crate::db::{Database, Error, GitHubSyncStateRecord};
use crate::github::{GitHubClient, GitHubError, GitHubRepository, MAX_ISSUE_PAGES};
use crate::telemetry::{ScopedSecret, SecretValue};

pub struct GitHubCredential {
    revision: u32,
    token: SecretValue,
    _redaction: ScopedSecret,
}

impl std::fmt::Debug for GitHubCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GitHubCredential")
            .field("revision", &self.revision)
            .field("token", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl GitHubCredential {
    pub fn new(revision: u32, token: SecretValue) -> Result<Self, Error> {
        if revision == 0 {
            return Err(Error::Invalid("GitHub credential revision"));
        }
        let redaction = ScopedSecret::register(token.expose()).ok_or_else(|| {
            Error::Unavailable("GitHub credential redaction capacity is exhausted".into())
        })?;
        Ok(Self {
            revision,
            token,
            _redaction: redaction,
        })
    }

    pub fn revision(&self) -> u32 {
        self.revision
    }

    pub fn token(&self) -> &str {
        self.token.expose()
    }
}

#[derive(Debug, Clone)]
pub struct GitHubRepositoryListing {
    pub repositories: Vec<GitHubRepository>,
    pub rate_limited: bool,
    pub next_allowed_at: Option<String>,
    pub detail: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RepositoryListError {
    #[error("database error: {0}")]
    Database(#[from] Error),
    #[error("GitHub upstream error: {0}")]
    Upstream(#[from] GitHubError),
}

enum Collected {
    NotModified,
    Snapshot {
        issues: Vec<crate::github::GitHubIssue>,
        page_etags: Vec<(u32, String)>,
        truncated: bool,
    },
}

enum CollectError {
    Database(Error),
    Upstream(GitHubError),
}

pub async fn sync_project_issues(
    database: &Database,
    client: &GitHubClient,
    credential: &GitHubCredential,
    project_id: &str,
) -> Result<GitHubSyncStateRecord, Error> {
    database
        .require_github_connection_revision(credential.revision())
        .await?;
    if let Some(until) = database.github_backoff_until().await? {
        return Err(Error::RateLimited(format!(
            "GitHub rate limit backoff is active until {until}"
        )));
    }
    let binding = database.project_github_binding(project_id).await?;
    let collected = match collect_pages(database, client, credential, project_id, &binding).await {
        Ok(collected) => collected,
        Err(CollectError::Database(error)) => return Err(error),
        Err(CollectError::Upstream(error)) => {
            return database
                .record_github_sync_failure(crate::db::GitHubSyncFailureWrite {
                    project_id,
                    repo_full_name: &binding.repo_full_name,
                    status: error.kind(),
                    detail: &error.detail(),
                    status_code: error.status_code(),
                    next_allowed_at: error.next_allowed_at(),
                    connection_revision: credential.revision(),
                })
                .await;
        }
    };
    match collected {
        Collected::NotModified => {
            database
                .record_github_sync_not_modified(
                    project_id,
                    &binding.repo_full_name,
                    credential.revision(),
                )
                .await
        }
        Collected::Snapshot {
            issues,
            page_etags,
            truncated,
        } => {
            let etag = page_etags
                .iter()
                .find(|(page, _)| *page == 1)
                .map(|(_, etag)| etag.as_str());
            database
                .record_github_sync_result(crate::db::GitHubSyncResultWrite {
                    project_id,
                    repo_full_name: &binding.repo_full_name,
                    issues: &issues,
                    page_etags: &page_etags,
                    etag,
                    truncated,
                    connection_revision: credential.revision(),
                })
                .await
        }
    }
}

/// List repositories with credential-generation suppression and shared
/// backoff. A rate-limited or superseded response is never returned as a
/// current repository list.
pub async fn list_repositories(
    database: &Database,
    client: &GitHubClient,
    credential: &GitHubCredential,
    query: &str,
) -> Result<GitHubRepositoryListing, RepositoryListError> {
    database
        .require_github_connection_revision(credential.revision())
        .await?;
    if let Some(until) = database.github_backoff_until().await? {
        return Ok(GitHubRepositoryListing {
            repositories: Vec::new(),
            rate_limited: true,
            next_allowed_at: Some(until.clone()),
            detail: format!("GitHub rate limit backoff is active until {until}."),
        });
    }
    let result = client.repositories(credential.token(), Some(query)).await;
    // Suppress a stale response even when the request itself failed.
    database
        .require_github_connection_revision(credential.revision())
        .await?;
    match result {
        Ok(repositories) => Ok(GitHubRepositoryListing {
            repositories,
            rate_limited: false,
            next_allowed_at: None,
            detail: String::new(),
        }),
        Err(error @ GitHubError::RateLimited { .. }) => {
            let next = error
                .next_allowed_at()
                .map(str::to_owned)
                .unwrap_or_else(default_rate_limit_deadline);
            database
                .set_github_rate_limit(credential.revision(), &next)
                .await?;
            Ok(GitHubRepositoryListing {
                repositories: Vec::new(),
                rate_limited: true,
                next_allowed_at: Some(next),
                detail: "GitHub rate limit is exhausted; retry after the deadline.".into(),
            })
        }
        Err(error) => Err(RepositoryListError::Upstream(error)),
    }
}

async fn collect_pages(
    database: &Database,
    client: &GitHubClient,
    credential: &GitHubCredential,
    project_id: &str,
    binding: &crate::db::ProjectGitHubBinding,
) -> Result<Collected, CollectError> {
    let stored = database
        .github_issue_page_etags(project_id)
        .await
        .map_err(CollectError::Database)?;
    let highest = stored.iter().map(|(page, _)| *page).max().unwrap_or(0);
    let mut issues = Vec::new();
    let mut seen = HashSet::new();
    let mut page_etags = Vec::new();
    let mut saw_modified = false;
    let mut saw_not_modified = false;
    let mut truncated = false;
    for page in 1..=MAX_ISSUE_PAGES {
        database
            .require_github_connection_revision(credential.revision())
            .await
            .map_err(CollectError::Database)?;
        let etag = stored
            .iter()
            .find(|(stored_page, _)| *stored_page as usize == page)
            .map(|(_, etag)| etag.as_str());
        let response = client
            .issue_page(
                credential.token(),
                &binding.owner,
                &binding.repo,
                page,
                etag,
            )
            .await
            .map_err(CollectError::Upstream)?;
        // A revocation or replacement while a page is in flight must stop the
        // scan before its response is used or another page is requested.
        database
            .require_github_connection_revision(credential.revision())
            .await
            .map_err(CollectError::Database)?;
        if response.not_modified {
            if etag.is_none() {
                return Err(CollectError::Upstream(GitHubError::InvalidResponse(
                    "unexpected not-modified response to an unconditional request".into(),
                )));
            }
            saw_not_modified = true;
            if page as u32 >= highest {
                break;
            }
            continue;
        }
        saw_modified = true;
        if let Some(etag) = response.etag {
            page_etags.push((page as u32, etag));
        }
        for issue in response.issues {
            if seen.insert(issue.number) {
                issues.push(issue);
            }
        }
        if !response.page_full {
            break;
        }
        if page == MAX_ISSUE_PAGES {
            truncated = true;
            break;
        }
    }
    if !saw_modified {
        return Ok(Collected::NotModified);
    }
    if saw_not_modified {
        // A mixed conditional scan cannot reconstitute the unchanged pages.
        // Run one unconditional bounded scan and reconcile only from it.
        return collect_unconditional(database, client, credential, binding).await;
    }
    Ok(Collected::Snapshot {
        issues,
        page_etags,
        truncated,
    })
}

async fn collect_unconditional(
    database: &Database,
    client: &GitHubClient,
    credential: &GitHubCredential,
    binding: &crate::db::ProjectGitHubBinding,
) -> Result<Collected, CollectError> {
    let mut issues = Vec::new();
    let mut seen = HashSet::new();
    let mut page_etags = Vec::new();
    let mut truncated = false;
    for page in 1..=MAX_ISSUE_PAGES {
        database
            .require_github_connection_revision(credential.revision())
            .await
            .map_err(CollectError::Database)?;
        let response = client
            .issue_page(
                credential.token(),
                &binding.owner,
                &binding.repo,
                page,
                None,
            )
            .await
            .map_err(CollectError::Upstream)?;
        database
            .require_github_connection_revision(credential.revision())
            .await
            .map_err(CollectError::Database)?;
        if response.not_modified {
            return Err(CollectError::Upstream(GitHubError::InvalidResponse(
                "unexpected not-modified response to an unconditional scan".into(),
            )));
        }
        if let Some(etag) = response.etag {
            page_etags.push((page as u32, etag));
        }
        for issue in response.issues {
            if seen.insert(issue.number) {
                issues.push(issue);
            }
        }
        if !response.page_full {
            return Ok(Collected::Snapshot {
                issues,
                page_etags,
                truncated,
            });
        }
        if page == MAX_ISSUE_PAGES {
            truncated = true;
        }
    }
    Ok(Collected::Snapshot {
        issues,
        page_etags,
        truncated,
    })
}

fn default_rate_limit_deadline() -> String {
    (chrono::Utc::now() + chrono::Duration::minutes(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{GitHubIssueFilters, NewProjectGitHubBinding};
    use crate::github::GitHubClient;
    use crate::provider_secrets::ProviderSecretKey;
    use axum::{
        Router,
        extract::{Path as AxumPath, Query, State},
        http::{HeaderMap, StatusCode, header},
        response::{IntoResponse, Response},
        routing::get,
    };
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use std::{
        collections::HashMap,
        io::Write,
        path::PathBuf,
        sync::{Arc, Mutex},
    };
    use tokio::sync::Semaphore;

    const TOKEN: &str = "ghp_sync_test_token_1234567890";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Mode {
        Ok,
        RateLimited,
        Unauthorized,
        NotFound,
        Forbidden,
    }

    #[derive(Clone)]
    struct Gate {
        entered: Arc<Semaphore>,
        release: Arc<Semaphore>,
    }

    impl Gate {
        fn new() -> Self {
            Self {
                entered: Arc::new(Semaphore::new(0)),
                release: Arc::new(Semaphore::new(0)),
            }
        }

        async fn wait(&self) {
            self.entered.add_permits(1);
            let _permit = self.release.acquire().await.expect("gate released");
        }
    }

    #[derive(Clone)]
    struct Fake {
        mode: Arc<Mutex<Mode>>,
        pages: Arc<Mutex<HashMap<usize, Vec<Value>>>>,
        requests: Arc<Mutex<Vec<PageRequest>>>,
        auth: Arc<Mutex<Vec<Option<String>>>>,
        gate: Option<Gate>,
    }

    #[derive(Clone, Debug)]
    struct PageRequest {
        page: usize,
        if_none_match: Option<String>,
    }

    fn etag_for(page: usize, items: &[Value]) -> String {
        let mut digest = Sha256::new();
        digest.update(page.to_le_bytes());
        digest.update(serde_json::to_string(items).unwrap_or_default());
        let hex: String = digest
            .finalize()
            .iter()
            .take(8)
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!("\"page-{page}-{hex}\"")
    }

    async fn issues_handler(
        State(fake): State<Fake>,
        AxumPath((_owner, _repo)): AxumPath<(String, String)>,
        Query(query): Query<HashMap<String, String>>,
        headers: HeaderMap,
    ) -> Response {
        let page = query
            .get("page")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1);
        let if_none_match = headers
            .get(header::IF_NONE_MATCH)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        fake.requests.lock().unwrap().push(PageRequest {
            page,
            if_none_match: if_none_match.clone(),
        });
        fake.auth.lock().unwrap().push(
            headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        );
        if let Some(gate) = &fake.gate {
            gate.wait().await;
        }
        match *fake.mode.lock().unwrap() {
            Mode::Ok => {}
            Mode::RateLimited => {
                let mut response = StatusCode::TOO_MANY_REQUESTS.into_response();
                response
                    .headers_mut()
                    .insert(header::RETRY_AFTER, header::HeaderValue::from_static("30"));
                return response;
            }
            Mode::Unauthorized => return StatusCode::UNAUTHORIZED.into_response(),
            Mode::NotFound => return StatusCode::NOT_FOUND.into_response(),
            Mode::Forbidden => return StatusCode::FORBIDDEN.into_response(),
        }
        let items = fake
            .pages
            .lock()
            .unwrap()
            .get(&page)
            .cloned()
            .unwrap_or_default();
        let etag = etag_for(page, &items);
        if if_none_match.as_deref() == Some(etag.as_str()) {
            return StatusCode::NOT_MODIFIED.into_response();
        }
        let mut response = axum::Json(items).into_response();
        if let Ok(value) = header::HeaderValue::from_str(&etag) {
            response.headers_mut().insert(header::ETAG, value);
        }
        response
    }

    async fn fake_server() -> (String, Fake) {
        fake_server_with_gate(None).await
    }

    async fn fake_server_with_gate(gate: Option<Gate>) -> (String, Fake) {
        let fake = Fake {
            mode: Arc::new(Mutex::new(Mode::Ok)),
            pages: Arc::new(Mutex::new(HashMap::new())),
            requests: Arc::new(Mutex::new(Vec::new())),
            auth: Arc::new(Mutex::new(Vec::new())),
            gate,
        };
        let app = Router::new()
            .route("/repos/{owner}/{repo}/issues", get(issues_handler))
            .with_state(fake.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (url, fake)
    }

    #[derive(Clone)]
    struct RepoFake {
        requests: Arc<Mutex<usize>>,
        gate: Option<Gate>,
    }

    async fn repo_handler(State(fake): State<RepoFake>) -> Response {
        *fake.requests.lock().unwrap() += 1;
        if let Some(gate) = &fake.gate {
            gate.wait().await;
        }
        axum::Json(json!([{
            "full_name": "acme/widgets",
            "private": true,
            "default_branch": "main",
            "html_url": "https://github.com/acme/widgets",
            "pushed_at": "2026-10-03T00:00:00Z"
        }]))
        .into_response()
    }

    async fn repo_server(gate: Option<Gate>) -> (String, RepoFake) {
        let fake = RepoFake {
            requests: Arc::new(Mutex::new(0)),
            gate,
        };
        let app = Router::new()
            .route("/user/repos", get(repo_handler))
            .with_state(fake.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (url, fake)
    }

    fn issue(number: u64, title: &str, state: &str) -> Value {
        json!({
            "id": 20_000 + number,
            "number": number,
            "title": title,
            "body": format!("Untrusted body for {title}"),
            "state": state,
            "state_reason": if state == "closed" { json!("completed") } else { json!(null) },
            "labels": [{"name": "ops", "color": "00ff00"}],
            "user": {"login": "octo"},
            "html_url": format!("https://github.com/acme/widgets/issues/{number}"),
            "comments": 3,
            "created_at": "2026-10-01T00:00:00Z",
            "updated_at": "2026-10-02T00:00:00Z",
            "closed_at": if state == "closed" { json!("2026-10-03T00:00:00Z") } else { json!(null) }
        })
    }

    fn secret_key() -> ProviderSecretKey {
        let path = std::env::temp_dir().join(format!(
            "factory-github-master-key-{}.txt",
            uuid::Uuid::new_v4()
        ));
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all("22".repeat(32).as_bytes()).unwrap();
        drop(file);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let key = ProviderSecretKey::from_file(&path).unwrap();
        let _ = std::fs::remove_file(path);
        key
    }

    async fn database() -> (PathBuf, Database) {
        let path = std::env::temp_dir().join(format!(
            "factory-github-sync-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let db = Database::open(&path).await.unwrap();
        db.insert_test_project("project-a", "repo-a").await;
        db.insert_test_project("project-b", "repo-b").await;
        db.bind_project_github(
            "project-a",
            NewProjectGitHubBinding {
                owner: "acme",
                repo: "widgets",
                default_branch: "main",
            },
            "tester",
        )
        .await
        .unwrap();
        db.bind_project_github(
            "project-b",
            NewProjectGitHubBinding {
                owner: "acme",
                repo: "other",
                default_branch: "main",
            },
            "tester",
        )
        .await
        .unwrap();
        save_token(&db).await;
        (path, db)
    }

    async fn save_token(db: &Database) {
        let encrypted = secret_key().encrypt(TOKEN.as_bytes()).unwrap();
        db.save_github_connection(&encrypted, "octo", &["repo".into()], true, "tester")
            .await
            .unwrap();
    }

    async fn credential(db: &Database) -> GitHubCredential {
        let record = db.github_connection().await.unwrap();
        GitHubCredential::new(record.revision, SecretValue::from_string(TOKEN.to_owned())).unwrap()
    }

    async fn sync(
        db: &Database,
        client: &GitHubClient,
        project: &str,
    ) -> Result<GitHubSyncStateRecord, Error> {
        let credential = credential(db).await;
        sync_project_issues(db, client, &credential, project).await
    }

    fn filters(state: &str) -> GitHubIssueFilters {
        GitHubIssueFilters {
            state: state.into(),
            label: None,
            query: None,
            limit: 50,
            offset: 0,
        }
    }

    fn set_pages(fake: &Fake, pages: &[(usize, Vec<Value>)]) {
        *fake.pages.lock().unwrap() = pages.iter().cloned().collect();
    }

    #[tokio::test]
    async fn sync_is_idempotent_and_reconciles_closures_and_deletions() {
        let (_path, db) = database().await;
        let (url, fake) = fake_server().await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        set_pages(
            &fake,
            &[(
                1,
                vec![
                    issue(1, "First issue", "open"),
                    issue(2, "Second issue", "open"),
                ],
            )],
        );
        let first = sync(&db, &client, "project-a").await.unwrap();
        assert_eq!(first.status, "OK");
        assert_eq!(first.fetched, 2);
        assert_eq!(first.created, 2);
        assert_eq!(first.total_known, 2);
        assert!(!first.stale);
        assert_eq!(
            fake.auth
                .lock()
                .unwrap()
                .last()
                .cloned()
                .flatten()
                .as_deref(),
            Some("Bearer ghp_sync_test_token_1234567890")
        );

        let unchanged = sync(&db, &client, "project-a").await.unwrap();
        assert_eq!(unchanged.status, "OK");
        assert_eq!(unchanged.fetched, 0);
        assert_eq!(unchanged.created, 0);
        assert_eq!(unchanged.updated, 0);
        assert_eq!(unchanged.total_known, 2);

        db.clear_github_page_etags("project-a").await;
        let duplicate = sync(&db, &client, "project-a").await.unwrap();
        assert_eq!(duplicate.created, 0);
        assert_eq!(duplicate.updated, 2);
        assert_eq!(duplicate.total_known, 2);

        set_pages(&fake, &[(1, vec![issue(1, "First issue", "closed")])]);
        let reconciled = sync(&db, &client, "project-a").await.unwrap();
        assert_eq!(reconciled.removed, 1);
        assert_eq!(reconciled.total_known, 1);
        assert_eq!(
            db.github_issues("project-a", &filters("open"))
                .await
                .unwrap()
                .total,
            0
        );
        let closed = db
            .github_issues("project-a", &filters("closed"))
            .await
            .unwrap();
        assert_eq!(closed.total, 1);
        assert_eq!(closed.items[0].state, "closed");
        assert_eq!(
            db.github_issue("project-a", 1)
                .await
                .unwrap()
                .state_reason
                .as_deref(),
            Some("completed")
        );
        assert!(matches!(
            db.github_issue("project-a", 2).await.unwrap_err(),
            Error::NotFound
        ));
    }

    #[tokio::test]
    async fn rate_limit_preserves_the_snapshot_and_enforces_credential_wide_backoff() {
        let (_path, db) = database().await;
        let (url, fake) = fake_server().await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        set_pages(&fake, &[(1, vec![issue(1, "First issue", "open")])]);
        let first = sync(&db, &client, "project-a").await.unwrap();
        let synced_at = first.last_synced_at.clone();

        *fake.mode.lock().unwrap() = Mode::RateLimited;
        let limited = sync(&db, &client, "project-a").await.unwrap();
        assert_eq!(limited.status, "RATE_LIMITED");
        assert!(limited.stale);
        assert!(limited.next_allowed_at.is_some());
        assert_eq!(limited.last_synced_at, synced_at);
        assert_eq!(limited.total_known, 1);
        assert!(db.github_backoff_until().await.unwrap().is_some());
        let b_list = db
            .github_issues("project-b", &filters("all"))
            .await
            .unwrap();
        assert!(
            b_list.next_allowed_at.is_some(),
            "credential-wide backoff must be visible from another Project"
        );
        let list = db
            .github_issues("project-a", &filters("all"))
            .await
            .unwrap();
        assert_eq!(list.total, 1);
        assert!(list.stale);
        assert_eq!(list.sync_status, "RATE_LIMITED");
        assert!(
            list.last_error
                .as_deref()
                .unwrap_or_default()
                .contains("rate limit")
        );
        assert!(!serde_json::to_string(&list).unwrap().contains(TOKEN));

        *fake.mode.lock().unwrap() = Mode::Ok;
        let calls_before = fake.requests.lock().unwrap().len();
        assert!(matches!(
            sync(&db, &client, "project-a").await.unwrap_err(),
            Error::RateLimited(_)
        ));
        assert!(matches!(
            sync(&db, &client, "project-b").await.unwrap_err(),
            Error::RateLimited(_)
        ));
        assert_eq!(fake.requests.lock().unwrap().len(), calls_before);

        let (repo_url, repo_fake) = repo_server(None).await;
        let repo_client = GitHubClient::with_base_url(&repo_url, true).unwrap();
        let credential = credential(&db).await;
        let listing = list_repositories(&db, &repo_client, &credential, "")
            .await
            .unwrap();
        assert!(listing.rate_limited);
        assert!(listing.next_allowed_at.is_some());
        assert_eq!(*repo_fake.requests.lock().unwrap(), 0);

        let revision = db.github_connection().await.unwrap().revision;
        db.clear_github_rate_limit(revision).await;
        assert!(db.github_backoff_until().await.unwrap().is_none());
        let recovered = sync(&db, &client, "project-a").await.unwrap();
        assert_eq!(recovered.status, "OK");
        assert!(!recovered.stale);
        assert!(recovered.next_allowed_at.is_none());
    }

    #[tokio::test]
    async fn revocation_clears_the_credential_and_stops_sync() {
        let (_path, db) = database().await;
        let (url, fake) = fake_server().await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        set_pages(&fake, &[(1, vec![issue(1, "First issue", "open")])]);
        let credential = credential(&db).await;
        sync_project_issues(&db, &client, &credential, "project-a")
            .await
            .unwrap();
        let revision = credential.revision();

        db.revoke_github_connection("tester").await.unwrap();
        let connection = db.github_connection().await.unwrap();
        assert!(!connection.enabled);
        assert!(connection.credential_ciphertext.is_empty());
        assert!(connection.credential_nonce.is_empty());
        assert!(!format!("{connection:?}").contains(TOKEN));
        assert!(db.github_backoff_until().await.unwrap().is_none());
        let state = db.github_sync_state("project-a").await.unwrap();
        assert_eq!(state.status, "REVOKED");
        assert!(state.stale);
        assert!(matches!(
            sync_project_issues(&db, &client, &credential, "project-a")
                .await
                .unwrap_err(),
            Error::Forbidden(_)
        ));
        assert!(matches!(
            db.record_github_sync_not_modified("project-a", "acme/widgets", revision)
                .await
                .unwrap_err(),
            Error::Forbidden(_)
        ));
        let audit = db.github_connection_audit(10).await.unwrap();
        assert!(audit.iter().any(|entry| entry.action == "configured"));
        assert!(audit.iter().any(|entry| entry.action == "revoked"));
        assert!(!serde_json::to_string(&audit).unwrap().contains(TOKEN));

        save_token(&db).await;
        assert!(matches!(
            db.record_github_sync_not_modified("project-a", "acme/widgets", revision)
                .await
                .unwrap_err(),
            Error::Conflict(_)
        ));
        let restored = db.github_sync_state("project-a").await.unwrap();
        assert_eq!(restored.status, "STALE");
        assert!(restored.stale);
        assert!(restored.detail.contains("run a sync"));
    }

    #[tokio::test]
    async fn in_flight_credential_replacement_cannot_publish_or_continue_pages() {
        let (_path, db) = database().await;
        let gate = Gate::new();
        let (url, fake) = fake_server_with_gate(Some(gate.clone())).await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        let page = (1..=100)
            .map(|number| issue(number, "In flight", "open"))
            .collect();
        set_pages(&fake, &[(1, page)]);
        let revision = db.github_connection().await.unwrap().revision;
        let db_for_task = db.clone();
        let client_for_task = client.clone();
        let task = tokio::spawn(async move {
            let credential =
                GitHubCredential::new(revision, SecretValue::from_string(TOKEN.to_owned()))
                    .unwrap();
            sync_project_issues(&db_for_task, &client_for_task, &credential, "project-a").await
        });
        let _entered = gate.entered.acquire().await.unwrap();
        db.revoke_github_connection("tester").await.unwrap();
        save_token(&db).await;
        gate.release.add_permits(1);
        let result = task.await.unwrap();
        assert!(
            matches!(result, Err(Error::Conflict(_)) | Err(Error::Forbidden(_))),
            "stale credential published a result: {result:?}"
        );
        assert_eq!(
            fake.requests
                .lock()
                .unwrap()
                .iter()
                .map(|request| request.page)
                .collect::<Vec<_>>(),
            vec![1],
            "a later page was requested after credential replacement"
        );
        assert_eq!(
            db.github_issues("project-a", &filters("all"))
                .await
                .unwrap()
                .total,
            0
        );
        assert_eq!(
            db.github_sync_state("project-a").await.unwrap().status,
            "STALE"
        );
    }

    #[tokio::test]
    async fn stale_repository_response_is_suppressed_after_generation_change() {
        let (_path, db) = database().await;
        let gate = Gate::new();
        let (url, fake) = repo_server(Some(gate.clone())).await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        let revision = db.github_connection().await.unwrap().revision;
        let db_for_task = db.clone();
        let client_for_task = client.clone();
        let task = tokio::spawn(async move {
            let credential =
                GitHubCredential::new(revision, SecretValue::from_string(TOKEN.to_owned()))
                    .unwrap();
            list_repositories(&db_for_task, &client_for_task, &credential, "").await
        });
        let _entered = gate.entered.acquire().await.unwrap();
        db.revoke_github_connection("tester").await.unwrap();
        save_token(&db).await;
        gate.release.add_permits(1);
        let error = task.await.unwrap().unwrap_err();
        assert!(matches!(
            error,
            RepositoryListError::Database(Error::Conflict(_))
                | RepositoryListError::Database(Error::Forbidden(_))
        ));
        assert_eq!(*fake.requests.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn later_page_deletion_is_reconciled_when_page_one_etag_matches() {
        let (_path, db) = database().await;
        let (url, fake) = fake_server().await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        let page_one = (1..=100)
            .map(|number| issue(number, "Page one", "open"))
            .collect();
        set_pages(
            &fake,
            &[
                (1, page_one),
                (
                    2,
                    vec![
                        issue(101, "Page two first", "open"),
                        issue(102, "Page two second", "open"),
                    ],
                ),
            ],
        );
        let first = sync(&db, &client, "project-a").await.unwrap();
        assert_eq!(first.total_known, 102);
        assert_eq!(
            db.github_issue_page_etags("project-a").await.unwrap().len(),
            2
        );

        set_pages(
            &fake,
            &[
                (
                    1,
                    (1..=100)
                        .map(|number| issue(number, "Page one", "open"))
                        .collect(),
                ),
                (2, vec![issue(101, "Page two first", "open")]),
            ],
        );
        let second = sync(&db, &client, "project-a").await.unwrap();
        assert_eq!(second.removed, 1);
        assert_eq!(second.total_known, 101);
        assert!(matches!(
            db.github_issue("project-a", 102).await.unwrap_err(),
            Error::NotFound
        ));
        let pages: Vec<usize> = fake
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.page)
            .collect();
        assert_eq!(pages, vec![1, 2, 1, 2, 1, 2]);
        let requests = fake.requests.lock().unwrap();
        assert!(requests[2].if_none_match.is_some());
        assert!(requests[4].if_none_match.is_none());
    }

    #[tokio::test]
    async fn credential_redaction_is_scoped_to_the_credential_lifetime() {
        let (_path, db) = database().await;
        let probe = "ghp_scope_probe_token_1234567890";
        let revision = db.github_connection().await.unwrap().revision;
        let credential =
            GitHubCredential::new(revision, SecretValue::from_string(probe.to_owned())).unwrap();
        assert!(!crate::telemetry::redact(&format!("value={probe}")).contains(probe));
        drop(credential);
        assert!(crate::telemetry::redact(&format!("value={probe}")).contains(probe));
    }

    #[tokio::test]
    async fn rebinding_clears_page_etags_and_old_snapshot() {
        let (_path, db) = database().await;
        let (url, fake) = fake_server().await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        set_pages(&fake, &[(1, vec![issue(1, "Old repository", "open")])]);
        sync(&db, &client, "project-a").await.unwrap();
        assert_eq!(
            db.github_issue_page_etags("project-a").await.unwrap().len(),
            1
        );

        db.bind_project_github(
            "project-a",
            NewProjectGitHubBinding {
                owner: "acme",
                repo: "renamed",
                default_branch: "main",
            },
            "tester",
        )
        .await
        .unwrap();
        assert!(
            db.github_issue_page_etags("project-a")
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            db.github_issues("project-a", &filters("all"))
                .await
                .unwrap()
                .total,
            0
        );
        let state = db.github_sync_state("project-a").await.unwrap();
        assert_eq!(state.status, "NEVER_SYNCED");
        assert!(state.etag.is_none());
    }

    #[tokio::test]
    async fn project_issue_identity_is_isolated() {
        let (_path, db) = database().await;
        let (url, fake) = fake_server().await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        set_pages(
            &fake,
            &[(
                1,
                vec![
                    issue(1, "Project A issue", "open"),
                    issue(2, "Only in project A", "open"),
                ],
            )],
        );
        sync(&db, &client, "project-a").await.unwrap();
        set_pages(&fake, &[(1, vec![issue(1, "Project B issue", "open")])]);
        sync(&db, &client, "project-b").await.unwrap();

        let a = db
            .github_issues("project-a", &filters("all"))
            .await
            .unwrap();
        let b = db
            .github_issues("project-b", &filters("all"))
            .await
            .unwrap();
        assert_eq!(a.total, 2);
        assert_eq!(b.total, 1);
        assert_eq!(
            db.github_issue("project-a", 1).await.unwrap().title,
            "Project A issue"
        );
        assert_eq!(
            db.github_issue("project-b", 1).await.unwrap().title,
            "Project B issue"
        );
        assert!(matches!(
            db.github_issue("project-b", 2).await.unwrap_err(),
            Error::NotFound
        ));
        assert!(matches!(
            db.github_issue("missing-project", 1).await.unwrap_err(),
            Error::NotFound
        ));
    }

    #[tokio::test]
    async fn denied_and_missing_repositories_are_recorded_distinctly() {
        let (_path, db) = database().await;
        let (url, fake) = fake_server().await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        for (mode, expected, code) in [
            (Mode::Unauthorized, "DENIED", 401),
            (Mode::Forbidden, "DENIED", 403),
            (Mode::NotFound, "NOT_FOUND", 404),
        ] {
            *fake.mode.lock().unwrap() = mode;
            let state = sync(&db, &client, "project-a").await.unwrap();
            assert_eq!(state.status, expected);
            assert_eq!(state.last_status_code, Some(code));
            assert!(state.stale);
            assert!(state.last_synced_at.is_none());
            assert_eq!(
                db.github_issues("project-a", &filters("all"))
                    .await
                    .unwrap()
                    .total,
                0
            );
        }
    }

    #[tokio::test]
    async fn search_and_label_filters_are_bounded_and_escape_wildcards() {
        let (_path, db) = database().await;
        let (url, fake) = fake_server().await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        set_pages(
            &fake,
            &[(
                1,
                vec![
                    issue(1, "Export CSV endpoint", "open"),
                    issue(2, "Fix 100% coverage", "open"),
                    issue(3, "Unrelated", "closed"),
                ],
            )],
        );
        sync(&db, &client, "project-a").await.unwrap();
        let by_query = db
            .github_issues(
                "project-a",
                &GitHubIssueFilters {
                    state: "all".into(),
                    label: None,
                    query: Some("export".into()),
                    limit: 10,
                    offset: 0,
                },
            )
            .await
            .unwrap();
        assert_eq!(by_query.total, 1);
        assert_eq!(by_query.items[0].number, 1);
        let escaped = db
            .github_issues(
                "project-a",
                &GitHubIssueFilters {
                    state: "all".into(),
                    label: None,
                    query: Some("100%".into()),
                    limit: 10,
                    offset: 0,
                },
            )
            .await
            .unwrap();
        assert_eq!(escaped.total, 1);
        let by_label = db
            .github_issues(
                "project-a",
                &GitHubIssueFilters {
                    state: "all".into(),
                    label: Some("OPS".into()),
                    query: None,
                    limit: 10,
                    offset: 0,
                },
            )
            .await
            .unwrap();
        assert_eq!(by_label.total, 3);
        assert!(matches!(
            db.github_issues(
                "project-a",
                &GitHubIssueFilters {
                    state: "invalid".into(),
                    label: None,
                    query: None,
                    limit: 10,
                    offset: 0,
                },
            )
            .await
            .unwrap_err(),
            Error::Invalid(_)
        ));
    }
}
