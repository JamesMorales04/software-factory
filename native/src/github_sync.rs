//! Core-owned orchestration for one bounded GitHub issue synchronization.
//!
//! The caller must have already verified that the connection is enabled and
//! decrypted the token. Upstream failures are recorded as truthful sync state
//! and returned as `Ok`; only persistence failures are errors.

use crate::db::{Database, Error, GitHubSyncStateRecord};
use crate::github::GitHubClient;

pub async fn sync_project_issues(
    database: &Database,
    client: &GitHubClient,
    token: &str,
    project_id: &str,
) -> Result<GitHubSyncStateRecord, Error> {
    let connection = database.github_connection().await?;
    if !connection.enabled || connection.credential_ciphertext.is_empty() {
        return Err(Error::Forbidden(
            "GitHub connection is revoked; synchronization is disabled".into(),
        ));
    }
    let binding = database.project_github_binding(project_id).await?;
    let state = database.github_sync_state(project_id).await?;
    match client
        .issues(token, &binding.owner, &binding.repo, state.etag.as_deref())
        .await
    {
        Ok(page) if page.not_modified => {
            database
                .record_github_sync_not_modified(project_id, &binding.repo_full_name)
                .await
        }
        Ok(page) => {
            database
                .record_github_sync_result(
                    project_id,
                    &binding.repo_full_name,
                    &page.issues,
                    page.etag.as_deref(),
                    page.truncated,
                )
                .await
        }
        Err(error) => {
            database
                .record_github_sync_failure(
                    project_id,
                    &binding.repo_full_name,
                    error.kind(),
                    &error.detail(),
                    error.status_code(),
                    error.next_allowed_at(),
                )
                .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{GitHubIssueFilters, NewProjectGitHubBinding};
    use crate::github::GitHubClient;
    use crate::provider_secrets::ProviderSecretKey;
    use axum::{
        Router,
        extract::{Path as AxumPath, State},
        http::{HeaderMap, StatusCode, header},
        response::{IntoResponse, Response},
        routing::get,
    };
    use serde_json::json;
    use std::{
        io::Write,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

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
    struct Fake {
        mode: Arc<Mutex<Mode>>,
        issues: Arc<Mutex<Vec<serde_json::Value>>>,
        auth: Arc<Mutex<Vec<Option<String>>>>,
    }

    async fn issues_handler(
        State(fake): State<Fake>,
        AxumPath((_owner, _repo)): AxumPath<(String, String)>,
        headers: HeaderMap,
    ) -> Response {
        fake.auth.lock().unwrap().push(
            headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        );
        match *fake.mode.lock().unwrap() {
            Mode::Ok => axum::Json(fake.issues.lock().unwrap().clone()).into_response(),
            Mode::RateLimited => {
                let mut response = StatusCode::TOO_MANY_REQUESTS.into_response();
                response
                    .headers_mut()
                    .insert(header::RETRY_AFTER, header::HeaderValue::from_static("30"));
                response
            }
            Mode::Unauthorized => StatusCode::UNAUTHORIZED.into_response(),
            Mode::NotFound => StatusCode::NOT_FOUND.into_response(),
            Mode::Forbidden => StatusCode::FORBIDDEN.into_response(),
        }
    }

    async fn fake_server() -> (String, Fake) {
        let fake = Fake {
            mode: Arc::new(Mutex::new(Mode::Ok)),
            issues: Arc::new(Mutex::new(Vec::new())),
            auth: Arc::new(Mutex::new(Vec::new())),
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

    fn issue(number: u64, title: &str, state: &str) -> serde_json::Value {
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
        let encrypted = secret_key().encrypt(TOKEN.as_bytes()).unwrap();
        db.save_github_connection(&encrypted, "octo", &["repo".into()], true, "tester")
            .await
            .unwrap();
        (path, db)
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

    #[tokio::test]
    async fn sync_is_idempotent_and_reconciles_closures_and_deletions() {
        let (_path, db) = database().await;
        let (url, fake) = fake_server().await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        *fake.issues.lock().unwrap() = vec![
            issue(1, "First issue", "open"),
            issue(2, "Second issue", "open"),
        ];
        let first = sync_project_issues(&db, &client, TOKEN, "project-a")
            .await
            .unwrap();
        assert_eq!(first.status, "OK");
        assert_eq!(first.fetched, 2);
        assert_eq!(first.created, 2);
        assert_eq!(first.updated, 0);
        assert_eq!(first.total_known, 2);
        assert!(!first.stale);
        assert!(first.last_synced_at.is_some());
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

        let second = sync_project_issues(&db, &client, TOKEN, "project-a")
            .await
            .unwrap();
        assert_eq!(second.created, 0);
        assert_eq!(second.updated, 2);
        assert_eq!(second.total_known, 2);
        assert_eq!(
            db.github_issues("project-a", &filters("all"))
                .await
                .unwrap()
                .total,
            2
        );

        *fake.issues.lock().unwrap() = vec![issue(1, "First issue", "closed")];
        let third = sync_project_issues(&db, &client, TOKEN, "project-a")
            .await
            .unwrap();
        assert_eq!(third.removed, 1);
        assert_eq!(third.total_known, 1);
        let open = db
            .github_issues("project-a", &filters("open"))
            .await
            .unwrap();
        assert_eq!(open.total, 0);
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
    async fn rate_limit_preserves_the_last_snapshot_and_marks_it_stale() {
        let (_path, db) = database().await;
        let (url, fake) = fake_server().await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        *fake.issues.lock().unwrap() = vec![issue(1, "First issue", "open")];
        let first = sync_project_issues(&db, &client, TOKEN, "project-a")
            .await
            .unwrap();
        let synced_at = first.last_synced_at.clone();

        *fake.mode.lock().unwrap() = Mode::RateLimited;
        let limited = sync_project_issues(&db, &client, TOKEN, "project-a")
            .await
            .unwrap();
        assert_eq!(limited.status, "RATE_LIMITED");
        assert!(limited.stale);
        assert!(limited.next_allowed_at.is_some());
        assert_eq!(limited.last_synced_at, synced_at);
        assert_eq!(limited.total_known, 1);
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
        let recovered = sync_project_issues(&db, &client, TOKEN, "project-a")
            .await
            .unwrap();
        assert_eq!(recovered.status, "OK");
        assert!(!recovered.stale);
        assert!(recovered.next_allowed_at.is_none());
    }

    #[tokio::test]
    async fn revocation_clears_the_credential_and_stops_sync() {
        let (_path, db) = database().await;
        let (url, fake) = fake_server().await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        *fake.issues.lock().unwrap() = vec![issue(1, "First issue", "open")];
        sync_project_issues(&db, &client, TOKEN, "project-a")
            .await
            .unwrap();

        db.revoke_github_connection("tester").await.unwrap();
        let connection = db.github_connection().await.unwrap();
        assert!(!connection.enabled);
        assert!(connection.credential_ciphertext.is_empty());
        assert!(connection.credential_nonce.is_empty());
        assert!(!format!("{connection:?}").contains(TOKEN));
        let state = db.github_sync_state("project-a").await.unwrap();
        assert_eq!(state.status, "REVOKED");
        assert!(state.stale);
        assert!(matches!(
            sync_project_issues(&db, &client, TOKEN, "project-a")
                .await
                .unwrap_err(),
            Error::Forbidden(_)
        ));
        let after = db.github_sync_state("project-a").await.unwrap();
        assert_eq!(after.status, "REVOKED");
        assert!(matches!(
            db.record_github_sync_not_modified("project-a", "acme/widgets")
                .await
                .unwrap_err(),
            Error::Forbidden(_)
        ));
        assert_eq!(
            db.github_sync_state("project-a").await.unwrap().status,
            "REVOKED"
        );
        let audit = db.github_connection_audit(10).await.unwrap();
        assert!(audit.iter().any(|entry| entry.action == "configured"));
        assert!(audit.iter().any(|entry| entry.action == "revoked"));
        assert!(!serde_json::to_string(&audit).unwrap().contains(TOKEN));

        let encrypted = secret_key().encrypt(TOKEN.as_bytes()).unwrap();
        db.save_github_connection(&encrypted, "octo", &["repo".into()], true, "tester")
            .await
            .unwrap();
        let restored = db.github_sync_state("project-a").await.unwrap();
        assert_eq!(restored.status, "STALE");
        assert!(restored.stale);
        assert!(restored.detail.contains("run a sync"));
    }

    #[tokio::test]
    async fn project_issue_identity_is_isolated() {
        let (_path, db) = database().await;
        let (url, fake) = fake_server().await;
        let client = GitHubClient::with_base_url(&url, true).unwrap();
        *fake.issues.lock().unwrap() = vec![
            issue(1, "Project A issue", "open"),
            issue(2, "Only in project A", "open"),
        ];
        sync_project_issues(&db, &client, TOKEN, "project-a")
            .await
            .unwrap();
        *fake.issues.lock().unwrap() = vec![issue(1, "Project B issue", "open")];
        sync_project_issues(&db, &client, TOKEN, "project-b")
            .await
            .unwrap();

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
            let state = sync_project_issues(&db, &client, TOKEN, "project-a")
                .await
                .unwrap();
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
        *fake.issues.lock().unwrap() = vec![
            issue(1, "Export CSV endpoint", "open"),
            issue(2, "Fix 100% coverage", "open"),
            issue(3, "Unrelated", "closed"),
        ];
        sync_project_issues(&db, &client, TOKEN, "project-a")
            .await
            .unwrap();
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
