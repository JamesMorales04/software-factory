//! Persistence for the GitHub issue integration.
//!
//! Only ciphertext, a nonce and a one-way fingerprint are stored. Issue bodies
//! are untrusted text and are persisted as opaque strings with a size bound.

use super::*;

use chrono::Utc;
use sqlx::row::Row;
use sqlx_sqlite::{SqliteConnection, SqliteRow};
use uuid::Uuid;

use crate::github::{GitHubIssue, GitHubLabel};
use crate::provider_secrets::EncryptedCredential;

const AUDIT_RETENTION: i64 = 200;
const MAX_SYNC_DETAIL_BYTES: usize = 1024;
const MAX_LABEL_FILTER_BYTES: usize = 100;
const MAX_QUERY_FILTER_BYTES: usize = 200;
const MAX_ISSUE_OFFSET: u32 = 100_000;

#[derive(Clone)]
pub struct GitHubConnectionRecord {
    pub login: String,
    pub scopes: Vec<String>,
    pub scopes_reported: bool,
    pub credential_ciphertext: String,
    pub credential_nonce: String,
    pub credential_fingerprint: String,
    pub enabled: bool,
    pub revision: u32,
    pub verified_at: String,
    pub revoked_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl std::fmt::Debug for GitHubConnectionRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GitHubConnectionRecord")
            .field("login", &self.login)
            .field("scopes", &self.scopes)
            .field("scopes_reported", &self.scopes_reported)
            .field("enabled", &self.enabled)
            .field("revision", &self.revision)
            .field("verified_at", &self.verified_at)
            .field("revoked_at", &self.revoked_at)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GitHubConnectionAudit {
    pub audit_id: String,
    pub revision: u32,
    pub action: String,
    pub actor: String,
    pub detail: String,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProjectGitHubBinding {
    pub project_id: String,
    pub owner: String,
    pub repo: String,
    pub repo_full_name: String,
    pub default_branch: String,
    pub repository_github_id: Option<i64>,
    pub bound_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy)]
pub struct NewProjectGitHubBinding<'a> {
    pub owner: &'a str,
    pub repo: &'a str,
    pub default_branch: &'a str,
    pub repository_github_id: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GitHubSyncStateRecord {
    pub project_id: String,
    pub repo_full_name: String,
    pub status: String,
    pub detail: String,
    pub etag: Option<String>,
    pub last_synced_at: Option<String>,
    pub last_attempt_at: Option<String>,
    pub next_allowed_at: Option<String>,
    pub last_status_code: Option<u16>,
    pub fetched: u32,
    pub created: u32,
    pub updated: u32,
    pub removed: u32,
    pub total_known: u32,
    pub truncated: bool,
    pub stale: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GitHubIssueSummary {
    pub number: u64,
    pub github_id: i64,
    pub title: String,
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
    pub synced_at: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GitHubIssueDetail {
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
    pub synced_at: String,
    pub content_hash: String,
}

#[derive(Debug, Clone)]
pub struct GitHubIssueFilters {
    pub state: String,
    pub label: Option<String>,
    pub query: Option<String>,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GitHubIssueList {
    pub items: Vec<GitHubIssueSummary>,
    pub total: u64,
    pub limit: u32,
    pub offset: u32,
    pub synced_at: Option<String>,
    pub stale: bool,
    pub sync_status: String,
    pub last_error: Option<String>,
    pub next_allowed_at: Option<String>,
    pub truncated: bool,
    pub binding: Option<ProjectGitHubBinding>,
}

pub struct GitHubSyncResultWrite<'a> {
    pub project_id: &'a str,
    pub repo_full_name: &'a str,
    pub issues: &'a [GitHubIssue],
    pub page_etags: &'a [(u32, String)],
    pub etag: Option<&'a str>,
    pub truncated: bool,
    pub connection_revision: u32,
}

pub struct GitHubSyncFailureWrite<'a> {
    pub project_id: &'a str,
    pub repo_full_name: &'a str,
    pub status: &'a str,
    pub detail: &'a str,
    pub status_code: Option<u16>,
    pub next_allowed_at: Option<&'a str>,
    pub connection_revision: u32,
}

impl super::Database {
    pub async fn save_github_connection(
        &self,
        credential: &EncryptedCredential,
        login: &str,
        scopes: &[String],
        scopes_reported: bool,
        actor: &str,
    ) -> Result<GitHubConnectionRecord, Error> {
        validate_actor(actor)?;
        if login.trim().is_empty()
            || login.len() > 255
            || login.chars().any(char::is_control)
            || credential.ciphertext.is_empty()
            || credential.nonce.is_empty()
            || credential.fingerprint.is_empty()
        {
            return Err(Error::Invalid("GitHub connection"));
        }
        let scopes_json = serde_json::to_string(scopes)
            .map_err(|_| Error::Invalid("GitHub connection scopes"))?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let existing: Option<(i64, String)> = sqlx::query_as::query_as(
            "SELECT revision,created_at FROM github_connections WHERE id='default'",
        )
        .fetch_optional(&mut *transaction)
        .await?;
        let revision = existing
            .as_ref()
            .map(|(revision, _)| (*revision as u32).saturating_add(1))
            .unwrap_or(1);
        let created_at = existing
            .map(|(_, created_at)| created_at)
            .unwrap_or_else(|| Utc::now().to_rfc3339());
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO github_connections
             (id,login,scopes_json,scopes_reported,credential_ciphertext,credential_nonce,
              credential_fingerprint,enabled,revision,verified_at,revoked_at,
              rate_limited_until,created_at,updated_at)
             VALUES ('default',?1,?2,?3,?4,?5,?6,1,?7,?8,NULL,NULL,?9,?10)
             ON CONFLICT(id) DO UPDATE SET
               login=excluded.login, scopes_json=excluded.scopes_json,
               scopes_reported=excluded.scopes_reported,
               credential_ciphertext=excluded.credential_ciphertext,
               credential_nonce=excluded.credential_nonce,
               credential_fingerprint=excluded.credential_fingerprint,
               enabled=1, revision=excluded.revision, verified_at=excluded.verified_at,
               revoked_at=NULL, rate_limited_until=NULL,
               updated_at=excluded.updated_at",
        )
        .bind(login)
        .bind(&scopes_json)
        .bind(scopes_reported)
        .bind(&credential.ciphertext)
        .bind(&credential.nonce)
        .bind(&credential.fingerprint)
        .bind(revision)
        .bind(&now)
        .bind(&created_at)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query(
            "UPDATE github_sync_state
             SET status='STALE',
                 detail='GitHub connection restored; run a sync to refresh the snapshot',
                 stale=1, next_allowed_at=NULL, updated_at=?1
             WHERE status='REVOKED'",
        )
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        insert_connection_audit(
            &mut transaction,
            revision,
            "configured",
            actor,
            &format!(
                "login={login} scopes={} scopesReported={scopes_reported}",
                scopes.len()
            ),
        )
        .await?;
        super::record_event(
            &mut transaction,
            "github",
            "github.connection_configured",
            &format!("revision={revision}"),
        )
        .await?;
        transaction.commit().await?;
        let mut connection = self.pool.acquire().await?;
        load_github_connection(&mut connection).await
    }

    pub async fn revoke_github_connection(
        &self,
        actor: &str,
    ) -> Result<GitHubConnectionRecord, Error> {
        validate_actor(actor)?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let existing = load_github_connection(&mut transaction).await?;
        let revision = existing.revision.saturating_add(1);
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "UPDATE github_connections
             SET credential_ciphertext='', credential_nonce='', credential_fingerprint='',
                 enabled=0, revision=?1, revoked_at=?2, rate_limited_until=NULL,
                 updated_at=?2
             WHERE id='default'",
        )
        .bind(revision)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query(
            "UPDATE github_sync_state
             SET status='REVOKED', detail=?1, next_allowed_at=NULL, stale=1, updated_at=?2",
        )
        .bind("GitHub connection was revoked; synchronization is disabled")
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        insert_connection_audit(
            &mut transaction,
            revision,
            "revoked",
            actor,
            "credential cleared; synchronization disabled",
        )
        .await?;
        super::record_event(
            &mut transaction,
            "github",
            "github.connection_revoked",
            &format!("revision={revision}"),
        )
        .await?;
        transaction.commit().await?;
        let mut connection = self.pool.acquire().await?;
        load_github_connection(&mut connection).await
    }

    pub async fn github_connection(&self) -> Result<GitHubConnectionRecord, Error> {
        let mut connection = self.pool.acquire().await?;
        load_github_connection(&mut connection).await
    }

    /// Reject a request whose decrypted credential no longer matches the
    /// current connection generation.
    pub async fn require_github_connection_revision(
        &self,
        expected_revision: u32,
    ) -> Result<(), Error> {
        let record = self.github_connection().await?;
        if !record.enabled || record.credential_ciphertext.is_empty() {
            return Err(Error::Forbidden(
                "GitHub connection is revoked; synchronization is disabled".into(),
            ));
        }
        if record.revision != expected_revision {
            return Err(Error::Conflict(
                "GitHub connection changed during synchronization",
            ));
        }
        Ok(())
    }

    /// Active credential-wide GitHub backoff. GitHub rate limits apply to the
    /// token, not to a single Project.
    pub async fn github_backoff_until(&self) -> Result<Option<String>, Error> {
        let row: Option<(Option<String>,)> = sqlx::query_as::query_as(
            "SELECT rate_limited_until FROM github_connections WHERE id='default'",
        )
        .fetch_optional(&self.pool)
        .await?;
        let Some((Some(value),)) = row else {
            return Ok(None);
        };
        let deadline = chrono::DateTime::parse_from_rfc3339(&value).map_err(|_| {
            Error::Unavailable("stored GitHub rate-limit deadline is invalid".into())
        })?;
        if deadline > Utc::now() {
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    /// Persist a credential-wide backoff for a repository listing. The write
    /// is fenced by the credential generation that produced the rate limit.
    pub async fn set_github_rate_limit(
        &self,
        expected_revision: u32,
        until: &str,
    ) -> Result<(), Error> {
        chrono::DateTime::parse_from_rfc3339(until)
            .map_err(|_| Error::Invalid("GitHub rate-limit deadline"))?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        ensure_connection_generation(&mut transaction, expected_revision).await?;
        let updated = sqlx::query::query(
            "UPDATE github_connections SET
               rate_limited_until=CASE
                 WHEN rate_limited_until IS NULL OR rate_limited_until < ?1 THEN ?1
                 ELSE rate_limited_until END,
               updated_at=?2
             WHERE id='default' AND revision=?3",
        )
        .bind(until)
        .bind(Utc::now().to_rfc3339())
        .bind(i64::from(expected_revision))
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if updated == 0 {
            return Err(Error::Conflict(
                "GitHub connection changed during synchronization",
            ));
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn github_issue_page_etags(
        &self,
        project_id: &str,
    ) -> Result<Vec<(u32, String)>, Error> {
        let rows: Vec<(i64, String)> = sqlx::query_as::query_as(
            "SELECT page,etag FROM github_issue_page_etags
             WHERE project_id=?1 ORDER BY page",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|(page, etag)| {
                u32::try_from(page)
                    .map(|page| (page, etag))
                    .map_err(|_| Error::Invalid("GitHub issue page"))
            })
            .collect()
    }

    pub async fn github_connection_audit(
        &self,
        limit: u32,
    ) -> Result<Vec<GitHubConnectionAudit>, Error> {
        let limit = limit.clamp(1, 50);
        let rows = sqlx::query::query(
            "SELECT audit_id,revision,action,actor,detail,created_at
             FROM github_connection_audit ORDER BY rowid DESC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(GitHubConnectionAudit {
                    audit_id: row.try_get("audit_id")?,
                    revision: row.try_get::<i64, _>("revision")? as u32,
                    action: row.try_get("action")?,
                    actor: row.try_get("actor")?,
                    detail: row.try_get("detail")?,
                    created_at: row.try_get("created_at")?,
                })
            })
            .collect()
    }

    pub async fn bind_project_github(
        &self,
        project_id: &str,
        input: NewProjectGitHubBinding<'_>,
        actor: &str,
    ) -> Result<ProjectGitHubBinding, Error> {
        validate_actor(actor)?;
        crate::github::validate_repo_coordinates(input.owner, input.repo)
            .map_err(|_| Error::Invalid("GitHub owner or repository"))?;
        crate::github::validate_default_branch(input.default_branch)
            .map_err(|_| Error::Invalid("GitHub default branch"))?;
        let repo_full_name = format!("{}/{}", input.owner, input.repo);
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let exists: Option<(i64,)> = sqlx::query_as::query_as("SELECT 1 FROM projects WHERE id=?1")
            .bind(project_id)
            .fetch_optional(&mut *transaction)
            .await?;
        if exists.is_none() {
            return Err(Error::NotFound);
        }
        let previous: Option<String> = sqlx::query_scalar::query_scalar(
            "SELECT repo_full_name FROM project_github_bindings WHERE project_id=?1",
        )
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let repo_changed = previous.as_deref() != Some(repo_full_name.as_str());
        let now = Utc::now().to_rfc3339();
        let bound_at = sqlx::query_scalar::query_scalar(
            "SELECT bound_at FROM project_github_bindings WHERE project_id=?1",
        )
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await?
        .unwrap_or_else(|| now.clone());
        sqlx::query::query(
            "INSERT INTO project_github_bindings
             (project_id,owner,repo,repo_full_name,default_branch,repository_github_id,
              bound_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(project_id) DO UPDATE SET
               owner=excluded.owner, repo=excluded.repo,
               repo_full_name=excluded.repo_full_name,
               default_branch=excluded.default_branch,
               repository_github_id=excluded.repository_github_id,
               updated_at=excluded.updated_at",
        )
        .bind(project_id)
        .bind(input.owner)
        .bind(input.repo)
        .bind(&repo_full_name)
        .bind(input.default_branch)
        .bind(input.repository_github_id.filter(|id| *id > 0))
        .bind(&bound_at)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        if repo_changed {
            sqlx::query::query(
                "DELETE FROM github_issues WHERE project_id=?1 AND repo_full_name<>?2",
            )
            .bind(project_id)
            .bind(&repo_full_name)
            .execute(&mut *transaction)
            .await?;
            sqlx::query::query("DELETE FROM github_issue_page_etags WHERE project_id=?1")
                .bind(project_id)
                .execute(&mut *transaction)
                .await?;
            sqlx::query::query(
                "INSERT INTO github_sync_state
                 (project_id,repo_full_name,status,detail,etag,last_synced_at,last_attempt_at,
                  next_allowed_at,last_status_code,fetched,created_count,updated_count,
                  removed_count,total_known,truncated,stale,updated_at)
                 VALUES (?1,?2,'NEVER_SYNCED','',NULL,NULL,NULL,NULL,NULL,0,0,0,0,0,0,1,?3)
                 ON CONFLICT(project_id) DO UPDATE SET
                   repo_full_name=excluded.repo_full_name, status='NEVER_SYNCED', detail='',
                   etag=NULL, last_synced_at=NULL, last_attempt_at=NULL, next_allowed_at=NULL,
                   last_status_code=NULL, fetched=0, created_count=0, updated_count=0,
                   removed_count=0, total_known=0, truncated=0, stale=1,
                   updated_at=excluded.updated_at",
            )
            .bind(project_id)
            .bind(&repo_full_name)
            .bind(&now)
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query::query(
            "INSERT INTO github_sync_state
             (project_id,repo_full_name,status,detail,etag,last_synced_at,last_attempt_at,
              next_allowed_at,last_status_code,fetched,created_count,updated_count,
              removed_count,total_known,truncated,stale,updated_at)
             VALUES (?1,?2,'NEVER_SYNCED','',NULL,NULL,NULL,NULL,NULL,0,0,0,0,0,0,1,?3)
             ON CONFLICT(project_id) DO NOTHING",
        )
        .bind(project_id)
        .bind(&repo_full_name)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        super::record_event(
            &mut transaction,
            project_id,
            "github.project_bound",
            &format!("repo={repo_full_name} branch={}", input.default_branch),
        )
        .await?;
        transaction.commit().await?;
        self.project_github_binding(project_id).await
    }

    pub async fn project_github_binding(
        &self,
        project_id: &str,
    ) -> Result<ProjectGitHubBinding, Error> {
        let mut connection = self.pool.acquire().await?;
        load_project_github_binding(&mut connection, project_id).await
    }

    pub async fn github_sync_state(
        &self,
        project_id: &str,
    ) -> Result<GitHubSyncStateRecord, Error> {
        let mut connection = self.pool.acquire().await?;
        load_github_sync_state(&mut connection, project_id).await
    }

    /// Persist one complete bounded issue page set and reconcile deletions.
    /// `truncated` means additional pages exist, so absent rows are preserved.
    pub async fn record_github_sync_result(
        &self,
        input: GitHubSyncResultWrite<'_>,
    ) -> Result<GitHubSyncStateRecord, Error> {
        let GitHubSyncResultWrite {
            project_id,
            repo_full_name,
            issues,
            page_etags,
            etag,
            truncated,
            connection_revision,
        } = input;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        ensure_connection_generation(&mut transaction, connection_revision).await?;
        ensure_binding_matches(&mut transaction, project_id, repo_full_name).await?;
        let rows = sqlx::query::query(
            "SELECT number FROM github_issues WHERE project_id=?1 AND repo_full_name=?2",
        )
        .bind(project_id)
        .bind(repo_full_name)
        .fetch_all(&mut *transaction)
        .await?;
        let existing: std::collections::HashSet<i64> = rows
            .iter()
            .filter_map(|row| row.try_get::<i64, _>("number").ok())
            .collect();
        let repository_github_id: Option<i64> = sqlx::query_scalar::query_scalar(
            "SELECT repository_github_id FROM project_github_bindings WHERE project_id=?1",
        )
        .bind(project_id)
        .fetch_one(&mut *transaction)
        .await?;
        let now = Utc::now().to_rfc3339();
        let sync_mark = Uuid::new_v4().to_string();
        let mut created = 0u32;
        let mut updated = 0u32;
        for issue in issues {
            let labels = serde_json::to_string(&issue.labels)
                .map_err(|_| Error::Invalid("GitHub issue labels"))?;
            let number =
                i64::try_from(issue.number).map_err(|_| Error::Invalid("GitHub issue number"))?;
            if existing.contains(&number) {
                updated = updated.saturating_add(1);
            } else {
                created = created.saturating_add(1);
            }
            sqlx::query::query(
                "INSERT INTO github_issues
                 (project_id,repo_full_name,number,github_id,title,body,body_truncated,state,
                  state_reason,labels_json,author,author_id,author_association,html_url,comments,
                  created_at,updated_at,closed_at,synced_at,sync_mark,content_hash,
                  repository_github_id,is_pull_request)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,
                         ?19,?20,?21,?22,0)
                 ON CONFLICT(project_id,repo_full_name,number) DO UPDATE SET
                   github_id=excluded.github_id, title=excluded.title, body=excluded.body,
                   body_truncated=excluded.body_truncated, state=excluded.state,
                   state_reason=excluded.state_reason, labels_json=excluded.labels_json,
                   author=excluded.author, author_id=excluded.author_id,
                   author_association=excluded.author_association,
                   html_url=excluded.html_url,
                   comments=excluded.comments, created_at=excluded.created_at,
                   updated_at=excluded.updated_at, closed_at=excluded.closed_at,
                   synced_at=excluded.synced_at, sync_mark=excluded.sync_mark,
                   content_hash=excluded.content_hash,
                   repository_github_id=excluded.repository_github_id,
                   is_pull_request=0",
            )
            .bind(project_id)
            .bind(repo_full_name)
            .bind(number)
            .bind(issue.github_id)
            .bind(&issue.title)
            .bind(&issue.body)
            .bind(issue.body_truncated)
            .bind(&issue.state)
            .bind(&issue.state_reason)
            .bind(&labels)
            .bind(&issue.author)
            .bind(issue.author_id)
            .bind(&issue.author_association)
            .bind(&issue.html_url)
            .bind(i64::from(issue.comments))
            .bind(&issue.created_at)
            .bind(&issue.updated_at)
            .bind(&issue.closed_at)
            .bind(&now)
            .bind(&sync_mark)
            .bind(issue.content_hash())
            .bind(repository_github_id)
            .execute(&mut *transaction)
            .await?;
        }
        let removed = if truncated {
            0
        } else {
            sqlx::query::query(
                "DELETE FROM github_issues
                 WHERE project_id=?1 AND repo_full_name=?2 AND sync_mark<>?3",
            )
            .bind(project_id)
            .bind(repo_full_name)
            .bind(&sync_mark)
            .execute(&mut *transaction)
            .await?
            .rows_affected() as u32
        };
        let total_known: i64 = sqlx::query_scalar::query_scalar(
            "SELECT COUNT(*) FROM github_issues WHERE project_id=?1 AND repo_full_name=?2",
        )
        .bind(project_id)
        .bind(repo_full_name)
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query::query("DELETE FROM github_issue_page_etags WHERE project_id=?1")
            .bind(project_id)
            .execute(&mut *transaction)
            .await?;
        for (page, page_etag) in page_etags {
            sqlx::query::query(
                "INSERT INTO github_issue_page_etags(project_id,page,etag)
                 VALUES (?1,?2,?3)",
            )
            .bind(project_id)
            .bind(i64::from(*page))
            .bind(page_etag)
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query::query(
            "UPDATE github_sync_state SET
               status='OK', detail='', etag=?1, last_synced_at=?2, last_attempt_at=?2,
               next_allowed_at=NULL, last_status_code=200, fetched=?3, created_count=?4,
               updated_count=?5, removed_count=?6, total_known=?7, truncated=?8,
               stale=0, updated_at=?2
             WHERE project_id=?9 AND repo_full_name=?10",
        )
        .bind(etag)
        .bind(&now)
        .bind(issues.len() as u32)
        .bind(created)
        .bind(updated)
        .bind(removed)
        .bind(total_known as u32)
        .bind(truncated)
        .bind(project_id)
        .bind(repo_full_name)
        .execute(&mut *transaction)
        .await?;
        super::record_event(
            &mut transaction,
            project_id,
            "github.issues_synced",
            &format!(
                "repo={repo_full_name} fetched={} created={created} updated={updated} removed={removed} truncated={truncated}",
                issues.len()
            ),
        )
        .await?;
        let state = load_github_sync_state(&mut transaction, project_id).await?;
        transaction.commit().await?;
        Ok(state)
    }

    /// Conditional request returned 304: the snapshot is current.
    pub async fn record_github_sync_not_modified(
        &self,
        project_id: &str,
        repo_full_name: &str,
        connection_revision: u32,
    ) -> Result<GitHubSyncStateRecord, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        ensure_connection_generation(&mut transaction, connection_revision).await?;
        ensure_binding_matches(&mut transaction, project_id, repo_full_name).await?;
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "UPDATE github_sync_state SET
               status='OK', detail='', last_synced_at=?1, last_attempt_at=?1,
               next_allowed_at=NULL, last_status_code=304, fetched=0, created_count=0,
               updated_count=0, removed_count=0, stale=0, updated_at=?1
             WHERE project_id=?2 AND repo_full_name=?3",
        )
        .bind(&now)
        .bind(project_id)
        .bind(repo_full_name)
        .execute(&mut *transaction)
        .await?;
        let state = load_github_sync_state(&mut transaction, project_id).await?;
        transaction.commit().await?;
        Ok(state)
    }

    pub async fn record_github_sync_failure(
        &self,
        input: GitHubSyncFailureWrite<'_>,
    ) -> Result<GitHubSyncStateRecord, Error> {
        let GitHubSyncFailureWrite {
            project_id,
            repo_full_name,
            status,
            detail,
            status_code,
            next_allowed_at,
            connection_revision,
        } = input;
        if !matches!(status, "RATE_LIMITED" | "DENIED" | "NOT_FOUND" | "ERROR") {
            return Err(Error::Invalid("GitHub sync status"));
        }
        let detail = bounded_text(detail, MAX_SYNC_DETAIL_BYTES);
        let now = Utc::now();
        let next_allowed_at = if status == "RATE_LIMITED" {
            Some(
                next_allowed_at
                    .map(str::to_owned)
                    .unwrap_or_else(|| default_rate_limit_deadline(now)),
            )
        } else {
            None
        };
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        ensure_connection_generation(&mut transaction, connection_revision).await?;
        ensure_binding_matches(&mut transaction, project_id, repo_full_name).await?;
        let now = now.to_rfc3339();
        sqlx::query::query(
            "UPDATE github_sync_state SET
               status=?1, detail=?2, last_attempt_at=?3, next_allowed_at=?4,
               last_status_code=?5, stale=1, updated_at=?3
             WHERE project_id=?6 AND repo_full_name=?7",
        )
        .bind(status)
        .bind(&detail)
        .bind(&now)
        .bind(next_allowed_at.as_deref())
        .bind(status_code.map(i64::from))
        .bind(project_id)
        .bind(repo_full_name)
        .execute(&mut *transaction)
        .await?;
        if let Some(deadline) = next_allowed_at.as_deref() {
            sqlx::query::query(
                "UPDATE github_connections SET
                   rate_limited_until=CASE
                     WHEN rate_limited_until IS NULL OR rate_limited_until < ?1 THEN ?1
                     ELSE rate_limited_until END,
                   updated_at=?2
                 WHERE id='default' AND revision=?3",
            )
            .bind(deadline)
            .bind(&now)
            .bind(i64::from(connection_revision))
            .execute(&mut *transaction)
            .await?;
        }
        let state = load_github_sync_state(&mut transaction, project_id).await?;
        transaction.commit().await?;
        Ok(state)
    }

    pub async fn github_issues(
        &self,
        project_id: &str,
        filters: &GitHubIssueFilters,
    ) -> Result<GitHubIssueList, Error> {
        validate_issue_filters(filters)?;
        let binding = match self.project_github_binding(project_id).await {
            Ok(binding) => Some(binding),
            Err(Error::NotFound) => None,
            Err(error) => return Err(error),
        };
        let Some(binding) = binding else {
            return Ok(GitHubIssueList {
                items: Vec::new(),
                total: 0,
                limit: filters.limit,
                offset: filters.offset,
                synced_at: None,
                stale: true,
                sync_status: "UNBOUND".into(),
                last_error: None,
                next_allowed_at: None,
                truncated: false,
                binding: None,
            });
        };
        let state = self
            .github_sync_state(project_id)
            .await
            .unwrap_or_else(|_| empty_sync_state(project_id, &binding.repo_full_name));
        let shared_backoff = self.github_backoff_until().await?;
        let mut where_clause = String::from("project_id=?1 AND repo_full_name=?2");
        let mut parameter = 3;
        let mut state_parameter = None;
        if filters.state != "all" {
            where_clause.push_str(&format!(" AND state=?{parameter}"));
            state_parameter = Some(parameter);
            parameter += 1;
        }
        let mut label_parameter = None;
        if filters.label.is_some() {
            where_clause.push_str(&format!(
                " AND EXISTS (SELECT 1 FROM json_each(github_issues.labels_json) AS label
                    WHERE lower(json_extract(label.value,'$.name'))=lower(?{parameter}))"
            ));
            label_parameter = Some(parameter);
            parameter += 1;
        }
        let mut query_parameter = None;
        if filters.query.is_some() {
            where_clause.push_str(&format!(
                " AND (title LIKE ?{parameter} ESCAPE '\\' OR body LIKE ?{parameter} ESCAPE '\\')"
            ));
            query_parameter = Some(parameter);
            parameter += 1;
        }
        let list_sql = format!(
            "SELECT number,github_id,title,state,state_reason,labels_json,author,author_id,
                    author_association,html_url,comments,created_at,updated_at,closed_at,
                    synced_at,content_hash
             FROM github_issues WHERE {where_clause}
             ORDER BY updated_at DESC, number DESC LIMIT ?{parameter} OFFSET ?{}",
            parameter + 1
        );
        let count_sql = format!("SELECT COUNT(*) FROM github_issues WHERE {where_clause}");
        let mut list_query = sqlx::query::query(&list_sql)
            .bind(project_id)
            .bind(&binding.repo_full_name);
        let mut count_query = sqlx::query_scalar::query_scalar::<_, i64>(&count_sql)
            .bind(project_id)
            .bind(&binding.repo_full_name);
        if let (Some(_), Some(state)) = (state_parameter, Some(filters.state.as_str())) {
            list_query = list_query.bind(state);
            count_query = count_query.bind(state);
        }
        if let (Some(_), Some(label)) = (label_parameter, filters.label.as_deref()) {
            list_query = list_query.bind(label);
            count_query = count_query.bind(label);
        }
        if let (Some(_), Some(query)) = (query_parameter, filters.query.as_deref()) {
            let pattern = like_pattern(query);
            list_query = list_query.bind(pattern.clone());
            count_query = count_query.bind(pattern);
        }
        let rows = list_query
            .bind(i64::from(filters.limit))
            .bind(i64::from(filters.offset))
            .fetch_all(&self.pool)
            .await?;
        let total: i64 = count_query.fetch_one(&self.pool).await?;
        let items = rows
            .iter()
            .map(issue_summary_from_row)
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(GitHubIssueList {
            items,
            total: total.max(0) as u64,
            limit: filters.limit,
            offset: filters.offset,
            synced_at: state.last_synced_at.clone(),
            stale: state.stale,
            sync_status: state.status.clone(),
            last_error: if matches!(
                state.status.as_str(),
                "RATE_LIMITED" | "DENIED" | "NOT_FOUND" | "ERROR" | "REVOKED" | "STALE"
            ) {
                Some(state.detail.clone())
            } else {
                None
            },
            next_allowed_at: shared_backoff.or_else(|| state.next_allowed_at.clone()),
            truncated: state.truncated,
            binding: Some(binding),
        })
    }

    pub async fn github_issue(
        &self,
        project_id: &str,
        number: u64,
    ) -> Result<GitHubIssueDetail, Error> {
        if number == 0 || number > i64::MAX as u64 {
            return Err(Error::NotFound);
        }
        let binding = self.project_github_binding(project_id).await?;
        let number = number as i64;
        let row = sqlx::query::query(
            "SELECT number,github_id,title,body,body_truncated,state,state_reason,labels_json,
                    author,author_id,author_association,html_url,comments,created_at,updated_at,
                    closed_at,synced_at,content_hash
             FROM github_issues
             WHERE project_id=?1 AND repo_full_name=?2 AND number=?3",
        )
        .bind(project_id)
        .bind(&binding.repo_full_name)
        .bind(number)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        issue_detail_from_row(&row)
    }
}

fn validate_actor(actor: &str) -> Result<(), Error> {
    if actor.trim().is_empty() || actor.len() > 128 || actor.chars().any(char::is_control) {
        return Err(Error::Invalid("settings actor"));
    }
    Ok(())
}

fn validate_issue_filters(filters: &GitHubIssueFilters) -> Result<(), Error> {
    if !matches!(filters.state.as_str(), "all" | "open" | "closed")
        || !(1..=100).contains(&filters.limit)
        || filters.offset > MAX_ISSUE_OFFSET
        || filters
            .label
            .as_deref()
            .is_some_and(|label| label.is_empty() || label.len() > MAX_LABEL_FILTER_BYTES)
        || filters
            .query
            .as_deref()
            .is_some_and(|query| query.trim().is_empty() || query.len() > MAX_QUERY_FILTER_BYTES)
    {
        return Err(Error::Invalid("GitHub issue filters"));
    }
    Ok(())
}

async fn ensure_connection_generation(
    transaction: &mut SqliteConnection,
    expected_revision: u32,
) -> Result<(), Error> {
    let row: Option<(i64, i64, String)> = sqlx::query_as::query_as(
        "SELECT revision,enabled,credential_ciphertext
         FROM github_connections WHERE id='default'",
    )
    .fetch_optional(&mut *transaction)
    .await?;
    match row {
        Some((revision, 1, ciphertext))
            if !ciphertext.is_empty() && revision == i64::from(expected_revision) =>
        {
            Ok(())
        }
        Some((_, 1, ciphertext)) if !ciphertext.is_empty() => Err(Error::Conflict(
            "GitHub connection changed during synchronization",
        )),
        Some(_) => Err(Error::Forbidden(
            "GitHub connection is revoked; synchronization is disabled".into(),
        )),
        None => Err(Error::Forbidden(
            "GitHub connection is not configured".into(),
        )),
    }
}

fn default_rate_limit_deadline(now: chrono::DateTime<Utc>) -> String {
    (now + chrono::Duration::minutes(1)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

async fn ensure_binding_matches(
    transaction: &mut SqliteConnection,
    project_id: &str,
    repo_full_name: &str,
) -> Result<(), Error> {
    let binding: Option<String> = sqlx::query_scalar::query_scalar(
        "SELECT repo_full_name FROM project_github_bindings WHERE project_id=?1",
    )
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await?;
    match binding {
        Some(current) if current == repo_full_name => Ok(()),
        Some(_) => Err(Error::Conflict(
            "project GitHub binding changed during synchronization",
        )),
        None => Err(Error::NotFound),
    }
}

async fn insert_connection_audit(
    transaction: &mut SqliteConnection,
    revision: u32,
    action: &str,
    actor: &str,
    detail: &str,
) -> Result<(), Error> {
    sqlx::query::query(
        "INSERT INTO github_connection_audit
         (audit_id,connection_id,revision,action,actor,detail,created_at)
         VALUES (?1,'default',?2,?3,?4,?5,?6)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(revision)
    .bind(action)
    .bind(actor)
    .bind(bounded_text(detail, MAX_SYNC_DETAIL_BYTES))
    .bind(Utc::now().to_rfc3339())
    .execute(&mut *transaction)
    .await?;
    sqlx::query::query(
        "DELETE FROM github_connection_audit WHERE rowid NOT IN
         (SELECT rowid FROM github_connection_audit ORDER BY rowid DESC LIMIT ?1)",
    )
    .bind(AUDIT_RETENTION)
    .execute(&mut *transaction)
    .await?;
    Ok(())
}

async fn load_github_connection(
    connection: &mut SqliteConnection,
) -> Result<GitHubConnectionRecord, Error> {
    let row = sqlx::query::query(
        "SELECT login,scopes_json,scopes_reported,credential_ciphertext,credential_nonce,
                credential_fingerprint,enabled,revision,verified_at,revoked_at,created_at,updated_at
         FROM github_connections WHERE id='default'",
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(Error::NotFound)?;
    let scopes_json: String = row.try_get("scopes_json")?;
    let scopes: Vec<String> =
        serde_json::from_str(&scopes_json).map_err(|_| Error::Invalid("GitHub scopes"))?;
    Ok(GitHubConnectionRecord {
        login: row.try_get("login")?,
        scopes,
        scopes_reported: row.try_get("scopes_reported")?,
        credential_ciphertext: row.try_get("credential_ciphertext")?,
        credential_nonce: row.try_get("credential_nonce")?,
        credential_fingerprint: row.try_get("credential_fingerprint")?,
        enabled: row.try_get("enabled")?,
        revision: row.try_get::<i64, _>("revision")? as u32,
        verified_at: row.try_get("verified_at")?,
        revoked_at: row.try_get("revoked_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

async fn load_project_github_binding(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> Result<ProjectGitHubBinding, Error> {
    let row = sqlx::query::query(
        "SELECT project_id,owner,repo,repo_full_name,default_branch,repository_github_id,
                bound_at,updated_at
         FROM project_github_bindings WHERE project_id=?1",
    )
    .bind(project_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(Error::NotFound)?;
    Ok(ProjectGitHubBinding {
        project_id: row.try_get("project_id")?,
        owner: row.try_get("owner")?,
        repo: row.try_get("repo")?,
        repo_full_name: row.try_get("repo_full_name")?,
        default_branch: row.try_get("default_branch")?,
        repository_github_id: row.try_get("repository_github_id")?,
        bound_at: row.try_get("bound_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

async fn load_github_sync_state(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> Result<GitHubSyncStateRecord, Error> {
    let row = sqlx::query::query(
        "SELECT project_id,repo_full_name,status,detail,etag,last_synced_at,last_attempt_at,
                next_allowed_at,last_status_code,fetched,created_count,updated_count,
                removed_count,total_known,truncated,stale,updated_at
         FROM github_sync_state WHERE project_id=?1",
    )
    .bind(project_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(Error::NotFound)?;
    sync_state_from_row(&row)
}

fn sync_state_from_row(row: &SqliteRow) -> Result<GitHubSyncStateRecord, Error> {
    Ok(GitHubSyncStateRecord {
        project_id: row.try_get("project_id")?,
        repo_full_name: row.try_get("repo_full_name")?,
        status: row.try_get("status")?,
        detail: row.try_get("detail")?,
        etag: row.try_get("etag")?,
        last_synced_at: row.try_get("last_synced_at")?,
        last_attempt_at: row.try_get("last_attempt_at")?,
        next_allowed_at: row.try_get("next_allowed_at")?,
        last_status_code: row
            .try_get::<Option<i64>, _>("last_status_code")?
            .map(|value| value as u16),
        fetched: row.try_get::<i64, _>("fetched")? as u32,
        created: row.try_get::<i64, _>("created_count")? as u32,
        updated: row.try_get::<i64, _>("updated_count")? as u32,
        removed: row.try_get::<i64, _>("removed_count")? as u32,
        total_known: row.try_get::<i64, _>("total_known")? as u32,
        truncated: row.try_get("truncated")?,
        stale: row.try_get("stale")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn empty_sync_state(project_id: &str, repo_full_name: &str) -> GitHubSyncStateRecord {
    GitHubSyncStateRecord {
        project_id: project_id.into(),
        repo_full_name: repo_full_name.into(),
        status: "NEVER_SYNCED".into(),
        detail: String::new(),
        etag: None,
        last_synced_at: None,
        last_attempt_at: None,
        next_allowed_at: None,
        last_status_code: None,
        fetched: 0,
        created: 0,
        updated: 0,
        removed: 0,
        total_known: 0,
        truncated: false,
        stale: true,
        updated_at: String::new(),
    }
}

fn issue_summary_from_row(row: &SqliteRow) -> Result<GitHubIssueSummary, Error> {
    Ok(GitHubIssueSummary {
        number: row.try_get::<i64, _>("number")? as u64,
        github_id: row.try_get("github_id")?,
        title: row.try_get("title")?,
        state: row.try_get("state")?,
        state_reason: row.try_get("state_reason")?,
        labels: parse_labels(row.try_get("labels_json")?)?,
        author: row.try_get("author")?,
        author_id: row.try_get("author_id")?,
        author_association: row.try_get("author_association")?,
        html_url: row.try_get("html_url")?,
        comments: row.try_get::<i64, _>("comments")? as u32,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        closed_at: row.try_get("closed_at")?,
        synced_at: row.try_get("synced_at")?,
        content_hash: row.try_get("content_hash")?,
    })
}

fn issue_detail_from_row(row: &SqliteRow) -> Result<GitHubIssueDetail, Error> {
    Ok(GitHubIssueDetail {
        number: row.try_get::<i64, _>("number")? as u64,
        github_id: row.try_get("github_id")?,
        title: row.try_get("title")?,
        body: row.try_get("body")?,
        body_truncated: row.try_get("body_truncated")?,
        state: row.try_get("state")?,
        state_reason: row.try_get("state_reason")?,
        labels: parse_labels(row.try_get("labels_json")?)?,
        author: row.try_get("author")?,
        author_id: row.try_get("author_id")?,
        author_association: row.try_get("author_association")?,
        html_url: row.try_get("html_url")?,
        comments: row.try_get::<i64, _>("comments")? as u32,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        closed_at: row.try_get("closed_at")?,
        synced_at: row.try_get("synced_at")?,
        content_hash: row.try_get("content_hash")?,
    })
}

fn parse_labels(value: String) -> Result<Vec<GitHubLabel>, Error> {
    serde_json::from_str(&value).map_err(|_| Error::Invalid("GitHub issue labels"))
}

fn like_pattern(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

fn bounded_text(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

#[cfg(test)]
impl super::Database {
    pub(crate) async fn insert_test_project(&self, id: &str, repository: &str) {
        sqlx::query::query(
            "INSERT INTO projects(id,repository,base_commit,created_at,identity)
             VALUES (?1,?2,'0000000000000000000000000000000000000000',?3,'')",
        )
        .bind(id)
        .bind(repository)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await
        .expect("insert test project");
    }

    pub(crate) async fn clear_github_page_etags(&self, project_id: &str) {
        sqlx::query::query("DELETE FROM github_issue_page_etags WHERE project_id=?1")
            .bind(project_id)
            .execute(&self.pool)
            .await
            .expect("clear test page etags");
    }

    pub(crate) async fn clear_github_rate_limit(&self, expected_revision: u32) {
        let updated = sqlx::query::query(
            "UPDATE github_connections SET rate_limited_until=NULL
             WHERE id='default' AND revision=?1",
        )
        .bind(i64::from(expected_revision))
        .execute(&self.pool)
        .await
        .expect("clear test rate limit")
        .rows_affected();
        assert_eq!(updated, 1);
    }
}
