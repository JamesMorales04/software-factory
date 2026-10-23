use std::{fs::OpenOptions, path::Path, sync::Mutex, time::Duration};

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::quality::{
    AuthorizationStatus, EvidenceKind, FrozenActionSpec, GateOutcome, GatePolicy, ReceiptResult,
    ReviewerReceipt, RiskStatus, VerifierReceipt, evaluate_quality_gate,
};
use crate::workspace::verify_candidate_commit;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("database file error: {0}")]
    File(#[from] std::io::Error),
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

pub struct Database(Mutex<Connection>);

impl Database {
    pub fn open(path: &Path) -> Result<Self, Error> {
        prepare_database_file(path)?;
        let connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        let existing_projects: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='projects')",
            [],
            |row| row.get(0),
        )?;
        let schema_version: i64 =
            connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if existing_projects && schema_version != 1 {
            return Err(Error::Invalid(
                "unsupported experimental database schema; preserve this database and use a new file",
            ));
        }
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS projects (
               id TEXT PRIMARY KEY, repository TEXT NOT NULL UNIQUE,
               base_commit TEXT NOT NULL, created_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS actions (
               id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id),
               objective TEXT NOT NULL, provider TEXT NOT NULL, model TEXT NOT NULL,
               risk TEXT NOT NULL, status TEXT NOT NULL, objective_sha256 TEXT NOT NULL,
               created_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS action_specs (
               action_id TEXT PRIMARY KEY REFERENCES actions(id),
               spec_json TEXT NOT NULL, spec_sha256 TEXT NOT NULL
             );
             CREATE TRIGGER IF NOT EXISTS action_specs_immutable_update
             BEFORE UPDATE ON action_specs BEGIN SELECT RAISE(ABORT, 'ActionSpec is immutable'); END;
             CREATE TRIGGER IF NOT EXISTS action_specs_immutable_delete
             BEFORE DELETE ON action_specs BEGIN SELECT RAISE(ABORT, 'ActionSpec is immutable'); END;
             CREATE TABLE IF NOT EXISTS campaigns (
               id TEXT PRIMARY KEY, action_id TEXT NOT NULL REFERENCES actions(id),
               status TEXT NOT NULL, created_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS runs (
               id TEXT PRIMARY KEY, campaign_id TEXT NOT NULL REFERENCES campaigns(id),
               status TEXT NOT NULL, created_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS attempts (
               id TEXT PRIMARY KEY, run_id TEXT NOT NULL REFERENCES runs(id),
               status TEXT NOT NULL, created_at TEXT NOT NULL,
               sandbox_ready INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS worker_leases (
               attempt_id TEXT PRIMARY KEY REFERENCES attempts(id),
               worker_id TEXT NOT NULL, generation INTEGER NOT NULL,
               expires_at_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS worker_receipts (
               receipt_id TEXT PRIMARY KEY, request_digest TEXT NOT NULL,
               attempt_id TEXT NOT NULL REFERENCES attempts(id), recorded_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS worker_sessions (
               worker_id TEXT PRIMARY KEY, token_sha256 TEXT NOT NULL,
               updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS candidates (
               id TEXT PRIMARY KEY,
               attempt_id TEXT NOT NULL UNIQUE REFERENCES attempts(id),
               action_spec_sha256 TEXT NOT NULL,
               commit_sha TEXT NOT NULL, tree_sha TEXT NOT NULL,
               implementer_id TEXT NOT NULL, created_at TEXT NOT NULL
             );
             CREATE TRIGGER IF NOT EXISTS candidates_immutable_update
             BEFORE UPDATE ON candidates BEGIN SELECT RAISE(ABORT, 'candidate is immutable'); END;
             CREATE TRIGGER IF NOT EXISTS candidates_immutable_delete
             BEFORE DELETE ON candidates BEGIN SELECT RAISE(ABORT, 'candidate is immutable'); END;
             CREATE TABLE IF NOT EXISTS quality_receipts (
               receipt_id TEXT PRIMARY KEY,
               candidate_id TEXT NOT NULL REFERENCES candidates(id),
               role TEXT NOT NULL, principal_id TEXT NOT NULL,
               action_spec_sha256 TEXT NOT NULL,
               commit_sha TEXT NOT NULL, tree_sha TEXT NOT NULL,
               result TEXT NOT NULL, evidence TEXT NOT NULL,
               received_at TEXT NOT NULL,
               UNIQUE(candidate_id, role)
             );
             CREATE TRIGGER IF NOT EXISTS quality_receipts_immutable_update
             BEFORE UPDATE ON quality_receipts BEGIN SELECT RAISE(ABORT, 'quality receipt is immutable'); END;
             CREATE TRIGGER IF NOT EXISTS quality_receipts_immutable_delete
             BEFORE DELETE ON quality_receipts BEGIN SELECT RAISE(ABORT, 'quality receipt is immutable'); END;
             CREATE TABLE IF NOT EXISTS gate_decisions (
               candidate_id TEXT PRIMARY KEY REFERENCES candidates(id),
               outcome TEXT NOT NULL, reasons_json TEXT NOT NULL,
               decided_at TEXT NOT NULL
             );
             CREATE TRIGGER IF NOT EXISTS gate_decisions_immutable_update
             BEFORE UPDATE ON gate_decisions BEGIN SELECT RAISE(ABORT, 'gate decision is immutable'); END;
             CREATE TRIGGER IF NOT EXISTS gate_decisions_immutable_delete
             BEFORE DELETE ON gate_decisions BEGIN SELECT RAISE(ABORT, 'gate decision is immutable'); END;
             CREATE TABLE IF NOT EXISTS events (
               sequence INTEGER PRIMARY KEY AUTOINCREMENT,
               subject_id TEXT NOT NULL, kind TEXT NOT NULL,
               detail TEXT NOT NULL, recorded_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS idempotency (
               key TEXT PRIMARY KEY, operation TEXT NOT NULL,
               request_digest TEXT NOT NULL, resource_id TEXT NOT NULL
             );",
        )?;
        if !existing_projects {
            connection.pragma_update(None, "user_version", 1)?;
        }
        Ok(Self(Mutex::new(connection)))
    }

    pub fn events(&self) -> Result<Vec<Event>, Error> {
        let connection = self.0.lock().expect("database mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT sequence,subject_id,kind,detail,recorded_at FROM events ORDER BY sequence",
        )?;
        Ok(statement
            .query_map([], |row| {
                Ok(Event {
                    sequence: row.get(0)?,
                    subject_id: row.get(1)?,
                    kind: row.get(2)?,
                    detail: row.get(3)?,
                    recorded_at: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
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

fn record_event(
    connection: &Connection,
    subject_id: &str,
    kind: &str,
    detail: &str,
) -> Result<(), Error> {
    connection.execute(
        "INSERT INTO events(subject_id,kind,detail,recorded_at) VALUES (?1,?2,?3,?4)",
        params![subject_id, kind, detail, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

fn replay(
    connection: &Connection,
    key: &str,
    operation: &str,
    request_digest: &str,
) -> Result<Option<String>, Error> {
    let existing = connection
        .query_row(
            "SELECT operation,request_digest,resource_id FROM idempotency WHERE key=?1",
            [key],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    match existing {
        Some((saved_operation, saved_digest, id))
            if saved_operation == operation && saved_digest == request_digest =>
        {
            Ok(Some(id))
        }
        Some(_) => Err(Error::Invalid("idempotency key reused for another request")),
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

mod lifecycle;
mod projects;
mod quality;
mod workers;

#[cfg(test)]
mod tests;
