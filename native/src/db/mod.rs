use std::{fs::OpenOptions, path::Path, sync::Arc, time::Duration};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx_macros::migrate;
use sqlx_sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};

use crate::quality::{
    AuthorizationStatus, EvidenceKind, FrozenActionSpec, GateOutcome, GatePolicy, ReceiptResult,
    ReviewerReceipt, RiskStatus, VerifierReceipt, evaluate_quality_gate,
};
use crate::workspace::verify_candidate_commit;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Sqlx(sqlx::Error),
    #[error("database migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("database file error: {0}")]
    File(#[from] std::io::Error),
    #[error("database constraint: {0}")]
    Constraint(String),
    #[error("database is busy; retry the operation")]
    Busy,
    #[error("delegation validation: {0}")]
    Delegation(String),
    #[error("invalid project repository: {0}")]
    Repository(String),
    #[error("invalid request: {0}")]
    Invalid(&'static str),
    #[error("record not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(&'static str),
    #[error("forbidden: {0}")]
    Forbidden(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub repository: String,
    pub base_commit: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    pub project_id: String,
    pub objective: String,
    pub provider: String,
    pub model: String,
    pub risk: String,
    pub status: String,
    pub objective_sha256: String,
    pub created_at: String,
    /// Immutable execution inputs captured when the Action is created. This is
    /// descriptive state only; it does not grant execution authority.
    pub spec: ActionSpec,
    pub spec_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ActionSpec {
    pub schema_version: u32,
    pub project_id: String,
    pub repository: String,
    pub base_commit: String,
    pub objective: String,
    pub objective_sha256: String,
    pub provider: String,
    pub model: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Campaign {
    pub id: String,
    pub action_id: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub campaign_id: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attempt {
    pub id: String,
    pub run_id: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkerLease {
    pub attempt_id: String,
    pub worker_id: String,
    pub generation: i64,
    pub expires_at_ms: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkerReport {
    pub attempt_id: String,
    pub worker_id: String,
    pub generation: i64,
    pub receipt_id: String,
    pub result_sha256: String,
    pub terminal_status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkerSession {
    pub worker_id: String,
    pub created: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub sequence: i64,
    pub subject_id: String,
    pub kind: String,
    pub detail: String,
    pub recorded_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CandidateRecord {
    pub id: String,
    pub attempt_id: String,
    pub action_spec_sha256: String,
    pub commit_sha: String,
    pub tree_sha: String,
    pub implementer_id: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NewCandidate {
    pub attempt_id: String,
    pub worker_id: String,
    pub generation: i64,
    pub action_spec_sha256: String,
    pub commit_sha: String,
    pub tree_sha: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum QualityRole {
    Verifier,
    Reviewer,
}

impl QualityRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Verifier => "verifier",
            Self::Reviewer => "reviewer",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct QualityReceiptInput {
    pub receipt_id: String,
    pub principal_id: String,
    pub action_spec_sha256: String,
    pub commit_sha: String,
    pub tree_sha: String,
    pub result: ReceiptResult,
    pub evidence: EvidenceKind,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct QualityReceiptRecord {
    pub receipt_id: String,
    pub candidate_id: String,
    pub role: QualityRole,
    pub principal_id: String,
    pub action_spec_sha256: String,
    pub commit_sha: String,
    pub tree_sha: String,
    pub result: ReceiptResult,
    pub evidence: EvidenceKind,
    pub received_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GateRecord {
    pub candidate_id: String,
    pub outcome: GateOutcome,
    pub reasons: Vec<String>,
    pub decided_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct QualityIngestResult {
    pub receipt: QualityReceiptRecord,
    pub gate: Option<GateRecord>,
}

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
    /// SQLite permits one writer. Serialize write transactions through an
    /// async gate so callers never hold a blocking lock or a connection mutex
    /// across an await. Reads continue under WAL.
    write_lock: Arc<tokio::sync::Mutex<()>>,
}

impl Database {
    /// Open or create the database through versioned migrations. Existing
    /// experimental rows are adopted by `0001_initial.sql`; an incompatible
    /// schema fails with a database error instead of being recreated.
    pub async fn open(path: &Path) -> Result<Self, Error> {
        prepare_database_file(path)?;
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .min_connections(0)
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(options)
            .await?;
        migrate!("../migrations").run(&pool).await?;
        // Fail closed if an unrelated/older schema was present and the
        // migrations could not establish the expected columns.
        sqlx::query::query("SELECT id,repository,base_commit,created_at FROM projects LIMIT 0")
            .execute(&pool)
            .await?;
        Ok(Self {
            pool,
            write_lock: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub async fn events(&self) -> Result<Vec<Event>, Error> {
        let rows: Vec<(i64, String, String, String, String)> = sqlx::query_as::query_as(
            "SELECT sequence,subject_id,kind,detail,recorded_at FROM events ORDER BY sequence",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(sequence, subject_id, kind, detail, recorded_at)| Event {
                sequence,
                subject_id,
                kind,
                detail,
                recorded_at,
            })
            .collect())
    }
}

impl From<crate::lead::LeadError> for Error {
    fn from(error: crate::lead::LeadError) -> Self {
        Self::Delegation(error.to_string())
    }
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        match &error {
            sqlx::Error::Database(database) => {
                if database.is_unique_violation() {
                    return Self::Conflict("duplicate record");
                }
                if database.is_foreign_key_violation() {
                    return Self::Constraint("foreign key violation".into());
                }
                if database.code().as_deref() == Some("5")
                    || database
                        .message()
                        .to_ascii_lowercase()
                        .contains("database is locked")
                {
                    return Self::Busy;
                }
                Self::Sqlx(error)
            }
            _ => Self::Sqlx(error),
        }
    }
}

#[cfg(unix)]
fn prepare_database_file(path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(Error::Invalid(
                    "database path must be a regular non-symlink file",
                ));
            }
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(Error::Invalid(
                    "existing database file grants group or other access",
                ));
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true).mode(0o600);
            match options.open(path) {
                Ok(file) => {
                    drop(file);
                    Ok(())
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    prepare_database_file(path)
                }
                Err(error) => Err(Error::File(error)),
            }
        }
        Err(error) => Err(Error::File(error)),
    }
}

#[cfg(not(unix))]
fn prepare_database_file(path: &Path) -> Result<(), Error> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(
            Error::Invalid("database path must be a regular non-symlink file"),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(file) => {
                    drop(file);
                    Ok(())
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    prepare_database_file(path)
                }
                Err(error) => Err(Error::File(error)),
            }
        }
        Err(error) => Err(Error::File(error)),
    }
}

async fn record_event(
    connection: &mut sqlx_sqlite::SqliteConnection,
    subject_id: &str,
    kind: &str,
    detail: &str,
) -> Result<(), Error> {
    sqlx::query::query(
        "INSERT INTO events(subject_id,kind,detail,recorded_at) VALUES (?1,?2,?3,?4)",
    )
    .bind(subject_id)
    .bind(kind)
    .bind(detail)
    .bind(Utc::now().to_rfc3339())
    .execute(connection)
    .await?;
    Ok(())
}

async fn replay(
    connection: &mut sqlx_sqlite::SqliteConnection,
    key: &str,
    operation: &str,
    request_digest: &str,
) -> Result<Option<String>, Error> {
    let existing: Option<(String, String, String)> = sqlx::query_as::query_as(
        "SELECT operation,request_digest,resource_id FROM idempotency WHERE key=?1",
    )
    .bind(key)
    .fetch_optional(connection)
    .await?;
    match existing {
        Some((saved_operation, saved_digest, id))
            if saved_operation == operation && saved_digest == request_digest =>
        {
            Ok(Some(id))
        }
        Some(_) => Err(Error::Conflict(
            "idempotency key reused for another request",
        )),
        None => Ok(None),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hmac_sha256_hex(key: &[u8], message: &[u8]) -> String {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    let mut input = b"factory-worker-registration-key-v1\0".to_vec();
    input.extend_from_slice(message);
    let tag = ring::hmac::sign(&key, &input);
    tag.as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

mod delegations;
mod lifecycle;
mod projects;
mod quality;
mod workers;

#[cfg(test)]
mod tests;
