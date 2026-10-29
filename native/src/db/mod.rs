use std::{fs::OpenOptions, path::Path, sync::Arc, time::Duration};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx_macros::migrate;
use sqlx_sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};

use crate::lead::SpecCheck;
use crate::quality::{
    AuthorizationStatus, EvidenceKind, FrozenActionSpec, GateOutcome, GatePolicy, ReceiptResult,
    ReviewerReceipt, RiskStatus, VerifierReceipt, evaluate_quality_gate,
};
use crate::workspace::verify_candidate_commit;

pub use provider_sessions::{
    NewProviderIntent, ProviderSessionRecord, ProviderTerminal, ProviderToolEventInput,
    ProviderToolEventRecord, ProviderTurnAuthorization, ProviderTurnBinding,
    ProviderTurnBindingInput, ProviderTurnReservationRecord, ProviderTurnUsage,
};

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
    #[error("invalid request: {0}")]
    Validation(String),
    #[error("record not found")]
    NotFound,
    #[error("rate limited: {0}")]
    RateLimited(String),
    #[error("service unavailable: {0}")]
    Unavailable(String),
    #[error("conflict: {0}")]
    Conflict(&'static str),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("lead request was withdrawn by an authenticated operator")]
    Withdrawn,
    #[error("lead request already started Campaign {campaign_id} ({campaign_status})")]
    AlreadyStarted {
        campaign_id: String,
        campaign_status: String,
    },
    #[error("intake conflict {code}: {message}")]
    IntakeConflict { code: &'static str, message: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub repository: String,
    pub base_commit: String,
    pub created_at: String,
    pub identity: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub source_kind: String,
    #[serde(default)]
    pub source_url: String,
    #[serde(default)]
    pub source_key: String,
    #[serde(default)]
    pub source_visibility: String,
    #[serde(default)]
    pub default_branch: String,
    #[serde(default)]
    pub remote_origin: String,
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

#[derive(Clone, Copy, Debug)]
pub struct ActionSpecInput<'a> {
    pub allowed_paths: &'a [String],
    pub checks: &'a [SpecCheck],
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
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    #[serde(default)]
    pub checks: Vec<SpecCheck>,
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
pub struct WorkerAssignment {
    pub lease: WorkerLease,
    pub campaign_id: String,
    /// Stable identity of the work item across attempts/repair sessions. Lead
    /// Campaigns use the frozen WorkGraph unit id; direct Campaigns use the
    /// Campaign id.
    pub work_item_id: String,
    /// Highest observed progress sequence accepted by Core for this Campaign;
    /// the Worker uses it to seed a monotonic sequence across restarts.
    pub progress_last_sequence: u64,
    pub action_id: String,
    pub action_spec: ActionSpec,
    pub action_spec_sha256: String,
    pub repository: String,
    pub execution_mode: String,
    pub kind: String,
    pub integration_candidates: Vec<String>,
    pub provider: String,
    pub model: String,
    pub provider_mode: String,
    pub delegation_id: String,
    /// Frozen Execution Policy V2. `None` and `LegacyV1` are historical evidence
    /// only and never dispatch under V2.
    pub execution_policy: Option<crate::execution_policy::FrozenExecutionPolicy>,
    pub allowed_tools: Vec<String>,
    /// Populated by Core immediately before returning a claim. The database
    /// layer never invents or stores this capability.
    #[serde(default)]
    pub attempt_grant: String,
}

#[derive(Clone, Debug)]
pub struct WorkspaceOperationAuthorization {
    pub attempt_id: String,
    pub worker_id: String,
    pub generation: i64,
    pub allowed_paths: Vec<String>,
    pub checks: Vec<Vec<String>>,
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
pub struct WorkerSummary {
    pub worker_id: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkerSession {
    pub worker_id: String,
    pub created: bool,
}

/// Maximum page returned by replay and exporter queries. Callers may request a
/// smaller page; a larger request is clamped rather than rejected.
pub const EVENTS_PAGE_MAX: usize = 500;

pub const EVENT_KINDS: &[&str] = &[
    "action.classified",
    "action.created",
    "action.spec_frozen",
    "attempt.created",
    "campaign.authorized",
    "campaign.cancelled",
    "campaign.created",
    "campaign.started",
    "candidate.integrated",
    "candidate.recorded",
    "delegation.recorded",
    "delegation.released",
    "delegation.reserved",
    "delegation.revoked",
    "execution_policy.granted",
    "github.connection_configured",
    "github.connection_revoked",
    "github.issues_synced",
    "github.project_bound",
    "intake.message_recorded",
    "intake.request_created",
    "issue_intake.batch_acknowledged",
    "issue_intake.batch_revoked",
    "issue_intake.policy_configured",
    "issue_intake.policy_disabled",
    "issue_intake.policy_updated",
    "issue_intake.queue_blocked",
    "issue_intake.queue_recovered",
    "issue_intake.queue_reconciled",
    "issue_intake.queue_retry",
    "lead.approved",
    "lead.assessment_recorded",
    "lead.plan_completed",
    "lead.plan_frozen",
    "lead.proposal_recorded",
    "lead.request_blocked",
    "lead.request_created",
    "lead.started",
    "lead.state_changed",
    "lead.withdrawn",
    "lead.work_failed",
    "model.call_ambiguous",
    "model.call_authorized",
    "model.call_released",
    "model.call_resolved",
    "model.call_settled",
    "provider.settings_configured",
    "settings.runtime_updated",
    "provider.session_active",
    "provider.tool_event",
    "provider.session_cancel_requested",
    "provider.session_cleaned",
    "provider.session_intended",
    "provider.turn_authorized",
    "provider.turn_request_bound",
    "provider.turn_resolved",
    "provider.turn_settled",
    "provider.session_terminal",
    "project.config_updated",
    "project.discovery_completed",
    "project.registered",
    "quality.gate_decided",
    "quality.receipt_recorded",
    "run.created",
    "run.status_changed",
    "worker.claimed",
    "worker.claim_released",
    "worker.registered",
    "worker.session_rotated",
    "worker.terminal_reported",
    "work_integration.started",
];

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
    #[serde(default)]
    pub integration: bool,
    #[serde(default)]
    pub lineage: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CandidateInspection {
    pub candidate: CandidateRecord,
    pub action_id: String,
    pub action_spec: ActionSpec,
    pub action_spec_sha256: String,
    pub repository: String,
    pub base_commit: String,
    pub integration: bool,
    pub lineage: Vec<String>,
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NewIntegratedCandidate {
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

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewMechanism {
    #[default]
    Deterministic,
    Model,
}

impl ReviewMechanism {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Deterministic => "DETERMINISTIC",
            Self::Model => "MODEL",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "DETERMINISTIC" => Some(Self::Deterministic),
            "MODEL" => Some(Self::Model),
            _ => None,
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
    #[serde(default)]
    pub findings: Vec<crate::review::ReviewFinding>,
    #[serde(default)]
    pub mechanism: ReviewMechanism,
    #[serde(default)]
    pub model_provider: String,
    #[serde(default)]
    pub model_name: String,
    #[serde(default)]
    pub model_reservation_id: String,
    #[serde(default)]
    pub model_request_sha256: String,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub cost_micros: u64,
    #[serde(default)]
    pub usage_known: bool,
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
    pub findings: Vec<crate::review::ReviewFinding>,
    pub mechanism: ReviewMechanism,
    pub model_provider: String,
    pub model_name: String,
    pub model_reservation_id: String,
    #[serde(default)]
    pub model_request_sha256: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: u64,
    pub usage_known: bool,
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
    /// Process-local provider model discovery cache. It is not authoritative
    /// state: it can only substantiate a model ID while the provider connection
    /// is enabled, credentialed and has an implemented adapter. A poisoned or
    /// empty cache fails closed.
    provider_models: Arc<std::sync::RwLock<crate::provider_catalog::ProviderModelCache>>,
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
            .acquire_timeout(Duration::from_secs(30))
            .connect_with(options)
            .await?;
        migrate!("../migrations").run(&pool).await?;
        issue_intake::recover_issue_intake_leases_on_open(&pool).await?;
        // Fail closed if an unrelated/older schema was present and the
        // migrations could not establish the expected columns.
        sqlx::query::query(
            "SELECT id,repository,base_commit,created_at,identity FROM projects LIMIT 0",
        )
        .execute(&pool)
        .await?;
        Ok(Self {
            pool,
            write_lock: Arc::new(tokio::sync::Mutex::new(())),
            provider_models: Arc::new(std::sync::RwLock::new(
                crate::provider_catalog::ProviderModelCache::default(),
            )),
        })
    }

    /// Snapshot the process-local provider model catalog. Cloning releases the
    /// lock before any await so no blocking guard crosses an async boundary.
    pub fn provider_model_cache(&self) -> crate::provider_catalog::ProviderModelCache {
        match self.provider_models.read() {
            Ok(cache) => cache.clone(),
            Err(_) => crate::provider_catalog::ProviderModelCache::default(),
        }
    }

    /// Record one bounded discovery outcome. An unavailable outcome replaces the
    /// entry with an empty catalog, so stale model IDs never keep a route
    /// executable after a failed rediscovery.
    pub fn record_provider_model_discovery(
        &self,
        discovery: crate::provider_catalog::ProviderModelCatalogEntry,
    ) {
        if let Ok(mut cache) = self.provider_models.write() {
            cache.record(discovery);
        }
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

    /// Return a bounded suffix of the ledger strictly after `after`.
    pub async fn events_after(&self, after: i64, limit: usize) -> Result<Vec<Event>, Error> {
        if after < 0 {
            return Err(Error::Invalid("event cursor"));
        }
        let limit = limit.clamp(1, EVENTS_PAGE_MAX) as i64;
        let rows: Vec<(i64, String, String, String, String)> = sqlx::query_as::query_as(
            "SELECT sequence,subject_id,kind,detail,recorded_at FROM events
             WHERE sequence > ?1 ORDER BY sequence LIMIT ?2",
        )
        .bind(after)
        .bind(limit)
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

    /// Count events still ahead of the durable export cursor.
    pub async fn event_count_after(&self, after: i64) -> Result<u64, Error> {
        let (count,): (i64,) =
            sqlx::query_as::query_as("SELECT COUNT(*) FROM events WHERE sequence > ?1")
                .bind(after)
                .fetch_one(&self.pool)
                .await?;
        Ok(count.max(0) as u64)
    }

    /// Read the last event sequence confirmed by the OTLP collector.
    pub async fn observability_cursor(&self) -> Result<i64, Error> {
        let row: Option<(i64,)> =
            sqlx::query_as::query_as("SELECT last_sequence FROM observability_cursor WHERE id=1")
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(sequence,)| sequence).unwrap_or(0))
    }

    /// Advance the durable export cursor monotonically. Callers persist this
    /// only after the collector accepted the corresponding batch.
    pub async fn advance_observability_cursor(&self, sequence: i64) -> Result<(), Error> {
        if sequence < 0 {
            return Err(Error::Invalid("event cursor"));
        }
        let _write = self.write_lock.lock().await;
        sqlx::query::query(
            "INSERT INTO observability_cursor(id,last_sequence,updated_at) VALUES (1,?1,?2)
             ON CONFLICT(id) DO UPDATE SET
               last_sequence=excluded.last_sequence,
               updated_at=excluded.updated_at
             WHERE excluded.last_sequence > observability_cursor.last_sequence",
        )
        .bind(sequence)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn insert_event_for_test(
        &self,
        subject_id: &str,
        kind: &str,
        detail: &str,
    ) -> Result<i64, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        record_event(&mut transaction, subject_id, kind, detail).await?;
        let (sequence,): (i64,) =
            sqlx::query_as::query_as("SELECT sequence FROM events ORDER BY sequence DESC LIMIT 1")
                .fetch_one(&mut *transaction)
                .await?;
        transaction.commit().await?;
        Ok(sequence)
    }
}

impl From<crate::lead::LeadError> for Error {
    fn from(error: crate::lead::LeadError) -> Self {
        match error {
            crate::lead::LeadError::Delegation(_) => Self::Forbidden(error.to_string()),
            _ => Self::Delegation(error.to_string()),
        }
    }
}

impl From<crate::execution_profile::ExecutionProfileError> for Error {
    fn from(error: crate::execution_profile::ExecutionProfileError) -> Self {
        Self::Validation(error.to_string())
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
    if !EVENT_KINDS.contains(&kind) {
        return Err(Error::Invalid("unknown event kind"));
    }
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

mod authorization;
pub use authorization::{CampaignAuthorizationRecord, NewCampaignAuthorization};
mod delegations;
mod device_sessions;
pub use device_sessions::{DeviceSessionRecord, NewDeviceSession};
mod execution_profiles;
pub use execution_profiles::{
    ExecutionProfileAudit, ExecutionProfileDefaultBinding, ExecutionProfileDefaultSource,
    ExecutionProfileRecord, ResolvedExecutionProfile,
};
mod github;
pub use github::{
    GitHubConnectionAudit, GitHubConnectionRecord, GitHubIssueDetail, GitHubIssueFilters,
    GitHubIssueList, GitHubIssueSummary, GitHubSyncFailureWrite, GitHubSyncResultWrite,
    GitHubSyncStateRecord, NewProjectGitHubBinding, ProjectGitHubBinding,
};
mod lead;
mod model_calls;
pub use lead::{
    ActionDraftRecord, AuthenticatedOperatorApproval, CampaignExecutionBindingRecord,
    ExpectedStartBinding, LeadRequestRecord, LeadStartEvidenceRecord, LeadStartResult,
    LeadTaskRecord, LeadWithdrawalRecord, NewDraftLeadRequest, StartAuthority,
    ValidatedAutonomyContext,
};
mod intake;
pub use intake::{
    ExecutionPolicyGrantRecord, ExecutionPolicyGrantResult, IntakeApprovalContext,
    IntakeMessageRecord, IntakeRequestRecord, LeadExecutionContextRecord, NewExecutionPolicyGrant,
};
mod issue_intake;
pub use issue_intake::{
    ClaimedIssueIntakeItem, IssueIntakeBatchResult, IssueIntakeBatchState, IssueIntakeItemState,
    IssueIntakeLimits, IssueIntakePolicyResult, IssueIntakeQueueFailure,
    IssueIntakeReconcileResult, IssueIntakeState, NewIssueIntakeBatch, NewIssueIntakeBatchItem,
    NewIssueIntakePolicy, NewIssueIntakePolicyV2,
};
pub use model_calls::{
    ModelCallDecision, ModelCallGrant, ModelCallPolicy, ModelCallRecord, ModelCallResult,
    ModelCallRole, ModelCallUsage, NewModelCallAuthorization,
};
mod lifecycle;
pub mod participants;
pub use participants::{
    ParticipantEvidence, ParticipantPage, ParticipantPageInfo, ParticipantQuery, ParticipantRecord,
    ParticipantRoute, ParticipantSession, ParticipantSource, ParticipantUsage,
    ProviderSessionContext, ProviderToolEventProjection, ProviderToolEventView,
};
mod project_configs;
mod provider_sessions;
mod provider_settings;
pub use provider_settings::{ProviderSettingsAudit, ProviderSettingsRecord, ProviderSettingsWrite};
mod runtime_settings;
pub use project_configs::NewProjectConfig;
pub use runtime_settings::FactorySettings;
mod projects;
pub use projects::{ProjectCampaignSummary, ProjectDiscoveryRecord, RegisterProjectSource};
mod quality;
mod workers;

mod device_pairing;
#[cfg(test)]
mod issue_intake_tests;
#[cfg(test)]
mod tests;
pub use device_pairing::{DevicePairingPoll, DevicePairingRecord, SecurityAuditEntry};
