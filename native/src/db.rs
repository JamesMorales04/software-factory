use std::{fs::OpenOptions, path::Path, sync::Mutex, time::Duration};

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

    pub fn register_project(&self, repository: &Path, key: &str) -> Result<Project, Error> {
        let canonical = repository
            .canonicalize()
            .map_err(|error| Error::Repository(error.to_string()))?;
        if !canonical.is_dir() {
            return Err(Error::Repository("not a directory".into()));
        }
        let top_level = std::process::Command::new("git")
            .args([
                "-C",
                canonical
                    .to_str()
                    .ok_or(Error::Invalid("repository path encoding"))?,
                "rev-parse",
                "--show-toplevel",
            ])
            .output()
            .map_err(|error| Error::Repository(error.to_string()))?;
        if !top_level.status.success() {
            return Err(Error::Repository(
                "path is not inside a Git worktree".into(),
            ));
        }
        let root = String::from_utf8(top_level.stdout)
            .map_err(|_| Error::Invalid("repository path encoding"))?;
        let root = root
            .strip_suffix('\n')
            .ok_or(Error::Invalid("Git top-level output terminator"))?;
        let root = Path::new(root)
            .canonicalize()
            .map_err(|error| Error::Repository(error.to_string()))?;
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["rev-parse", "--verify", "HEAD"])
            .output()
            .map_err(|error| Error::Repository(error.to_string()))?;
        if !output.status.success() {
            return Err(Error::Repository("repository has no HEAD commit".into()));
        }
        let base_commit = String::from_utf8(output.stdout)
            .map_err(|_| Error::Invalid("commit encoding"))?
            .trim()
            .to_string();
        let repository = root.to_string_lossy().to_string();
        let request_digest = sha256_hex(repository.as_bytes());
        let mut connection = self.0.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        if let Some(id) = replay(&transaction, key, "register_project", &request_digest)? {
            let project = load_project(&transaction, &id)?;
            transaction.commit()?;
            return Ok(project);
        }
        let project = Project {
            id: Uuid::new_v4().to_string(),
            repository,
            base_commit,
            created_at: Utc::now().to_rfc3339(),
        };
        transaction.execute(
            "INSERT INTO projects VALUES (?1,?2,?3,?4)",
            params![
                project.id,
                project.repository,
                project.base_commit,
                project.created_at
            ],
        )?;
        transaction.execute(
            "INSERT INTO idempotency VALUES (?1,?2,?3,?4)",
            params![key, "register_project", request_digest, project.id],
        )?;
        record_event(
            &transaction,
            &project.id,
            "project.registered",
            "registered",
        )?;
        transaction.commit()?;
        Ok(project)
    }

    pub fn create_action(
        &self,
        project_id: &str,
        objective: &str,
        provider: &str,
        model: &str,
        key: &str,
    ) -> Result<Action, Error> {
        if objective.trim().is_empty() || objective.len() > 20_000 {
            return Err(Error::Invalid("objective length"));
        }
        if !matches!(provider, "codex" | "opencode") {
            return Err(Error::Invalid("provider"));
        }
        if model.trim().is_empty() || model.len() > 128 {
            return Err(Error::Invalid("model"));
        }
        let request_digest = sha256_hex(
            serde_json::to_string(&(project_id, objective, provider, model))
                .expect("string tuple serializes")
                .as_bytes(),
        );
        let mut connection = self.0.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        if let Some(id) = replay(&transaction, key, "create_action", &request_digest)? {
            let action = load_action(&transaction, &id)?;
            transaction.commit()?;
            return Ok(action);
        }
        let project = transaction
            .query_row(
                "SELECT id,repository,base_commit,created_at FROM projects WHERE id=?1",
                [project_id],
                project_row,
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        let objective_sha256 = sha256_hex(objective.as_bytes());
        let spec = ActionSpec {
            schema_version: 1,
            project_id: project_id.into(),
            repository: project.repository,
            base_commit: project.base_commit,
            objective: objective.into(),
            objective_sha256: objective_sha256.clone(),
            provider: provider.into(),
            model: model.into(),
        };
        let spec_json = serde_json::to_string(&spec).expect("ActionSpec serializes");
        let spec_sha256 = sha256_hex(spec_json.as_bytes());
        let action = Action {
            id: Uuid::new_v4().to_string(),
            project_id: project_id.into(),
            objective: objective.into(),
            provider: provider.into(),
            model: model.into(),
            risk: "UNASSESSED".into(),
            status: "HUMAN_REQUIRED".into(),
            objective_sha256,
            created_at: Utc::now().to_rfc3339(),
            spec: spec.clone(),
            spec_sha256: spec_sha256.clone(),
        };
        transaction.execute(
            "INSERT INTO actions VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                action.id,
                action.project_id,
                action.objective,
                action.provider,
                action.model,
                action.risk,
                action.status,
                action.objective_sha256,
                action.created_at
            ],
        )?;
        transaction.execute(
            "INSERT INTO action_specs VALUES (?1,?2,?3)",
            params![action.id, spec_json, spec_sha256],
        )?;
        transaction.execute(
            "INSERT INTO idempotency VALUES (?1,?2,?3,?4)",
            params![key, "create_action", request_digest, action.id],
        )?;
        record_event(
            &transaction,
            &action.id,
            "action.created",
            "awaiting risk classification and execution authority",
        )?;
        transaction.commit()?;
        Ok(action)
    }

    /// Register or restart a Worker session. Replaying the same identity and
    /// registration key rotates the token; the prior token is immediately
    /// invalidated. Only its SHA-256 digest is persisted.
    pub fn register_worker_session(
        &self,
        worker_id: &str,
        registration_key: &str,
        bootstrap_secret: &str,
        session_token: &str,
    ) -> Result<WorkerSession, Error> {
        if worker_id.trim().is_empty()
            || worker_id.len() > 128
            || registration_key.trim().is_empty()
            || registration_key.len() > 256
            || bootstrap_secret.len() < 32
            || session_token.len() < 32
        {
            return Err(Error::Invalid("worker registration"));
        }
        let digest = sha256_hex(
            serde_json::to_string(&(worker_id, "worker-session-v1"))
                .expect("tuple serializes")
                .as_bytes(),
        );
        let persisted_registration_key = format!(
            "worker-registration-v1:{}",
            hmac_sha256_hex(bootstrap_secret.as_bytes(), registration_key.as_bytes())
        );
        let token_hash = sha256_hex(session_token.as_bytes());
        let mut connection = self.0.lock().expect("database mutex poisoned");
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let replayed = replay(
            &tx,
            &persisted_registration_key,
            "register_worker_session",
            &digest,
        )?;
        if let Some(resource_id) = replayed {
            if resource_id != worker_id {
                return Err(Error::Conflict(
                    "registration key belongs to another Worker",
                ));
            }
            tx.execute(
                "UPDATE worker_sessions SET token_sha256=?1,updated_at=?2 WHERE worker_id=?3",
                params![token_hash, Utc::now().to_rfc3339(), worker_id],
            )?;
            if tx.changes() != 1 {
                return Err(Error::Conflict("Worker registration session is missing"));
            }
            record_event(
                &tx,
                worker_id,
                "worker.session_rotated",
                "session token rotated; prior token invalidated",
            )?;
            tx.commit()?;
            return Ok(WorkerSession {
                worker_id: worker_id.into(),
                created: false,
            });
        }
        let existing: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM worker_sessions WHERE worker_id=?1)",
            [worker_id],
            |r| r.get(0),
        )?;
        if existing {
            return Err(Error::Conflict(
                "Worker identity already registered; reuse its registration key to rotate session",
            ));
        }
        tx.execute(
            "INSERT INTO worker_sessions VALUES (?1,?2,?3)",
            params![worker_id, token_hash, Utc::now().to_rfc3339()],
        )?;
        tx.execute(
            "INSERT INTO idempotency VALUES (?1,?2,?3,?4)",
            params![
                persisted_registration_key,
                "register_worker_session",
                digest,
                worker_id
            ],
        )?;
        record_event(
            &tx,
            worker_id,
            "worker.registered",
            "session token digest stored",
        )?;
        tx.commit()?;
        Ok(WorkerSession {
            worker_id: worker_id.into(),
            created: true,
        })
    }

    pub fn worker_session_matches(&self, worker_id: &str, token: &str) -> Result<bool, Error> {
        if token.len() < 32 || token.len() > 256 {
            return Ok(false);
        }
        let connection = self.0.lock().expect("database mutex poisoned");
        let stored: Option<String> = connection
            .query_row(
                "SELECT token_sha256 FROM worker_sessions WHERE worker_id=?1",
                [worker_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(stored.is_some_and(|stored| {
            constant_time_eq(stored.as_bytes(), sha256_hex(token.as_bytes()).as_bytes())
        }))
    }

    /// Create a durable Campaign identity. Campaign creation does not approve
    /// risk or permit execution; every new Campaign remains BLOCKED pending
    /// explicit authorization implemented by a separate Core policy slice.
    pub fn create_campaign(&self, action_id: &str, key: &str) -> Result<Campaign, Error> {
        let digest = sha256_hex(
            serde_json::to_string(&(action_id, "campaign-v1"))
                .unwrap()
                .as_bytes(),
        );
        let mut connection = self.0.lock().expect("database mutex poisoned");
        let tx = connection.transaction()?;
        if let Some(id) = replay(&tx, key, "create_campaign", &digest)? {
            let value = load_campaign(&tx, &id)?;
            tx.commit()?;
            return Ok(value);
        }
        let exists = tx
            .query_row("SELECT 1 FROM actions WHERE id=?1", [action_id], |_| Ok(()))
            .optional()?
            .is_some();
        if !exists {
            return Err(Error::NotFound);
        }
        let value = Campaign {
            id: Uuid::new_v4().to_string(),
            action_id: action_id.into(),
            status: "BLOCKED_AUTHORIZATION".into(),
            created_at: Utc::now().to_rfc3339(),
        };
        tx.execute(
            "INSERT INTO campaigns VALUES (?1,?2,?3,?4)",
            params![value.id, value.action_id, value.status, value.created_at],
        )?;
        tx.execute(
            "INSERT INTO idempotency VALUES (?1,?2,?3,?4)",
            params![key, "create_campaign", digest, value.id],
        )?;
        record_event(
            &tx,
            &value.id,
            "campaign.created",
            "blocked pending explicit authorization",
        )?;
        tx.commit()?;
        Ok(value)
    }

    pub fn create_run(&self, campaign_id: &str, key: &str) -> Result<Run, Error> {
        let digest = sha256_hex(
            serde_json::to_string(&(campaign_id, "run-v1"))
                .unwrap()
                .as_bytes(),
        );
        let mut connection = self.0.lock().expect("database mutex poisoned");
        let tx = connection.transaction()?;
        if let Some(id) = replay(&tx, key, "create_run", &digest)? {
            let value = load_run(&tx, &id)?;
            tx.commit()?;
            return Ok(value);
        }
        let authorized: bool = tx
            .query_row(
                "SELECT status='AUTHORIZED' FROM campaigns WHERE id=?1",
                [campaign_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        if !authorized {
            return Err(Error::Invalid(
                "campaign lacks explicit execution authorization",
            ));
        }
        let value = Run {
            id: Uuid::new_v4().to_string(),
            campaign_id: campaign_id.into(),
            status: "QUEUED".into(),
            created_at: Utc::now().to_rfc3339(),
        };
        tx.execute(
            "INSERT INTO runs VALUES (?1,?2,?3,?4)",
            params![value.id, value.campaign_id, value.status, value.created_at],
        )?;
        tx.execute(
            "INSERT INTO idempotency VALUES (?1,?2,?3,?4)",
            params![key, "create_run", digest, value.id],
        )?;
        record_event(&tx, &value.id, "run.created", "queued")?;
        tx.commit()?;
        Ok(value)
    }

    pub fn create_attempt(&self, run_id: &str, key: &str) -> Result<Attempt, Error> {
        let digest = sha256_hex(
            serde_json::to_string(&(run_id, "attempt-v1"))
                .unwrap()
                .as_bytes(),
        );
        let mut connection = self.0.lock().expect("database mutex poisoned");
        let tx = connection.transaction()?;
        if let Some(id) = replay(&tx, key, "create_attempt", &digest)? {
            let value = load_attempt(&tx, &id)?;
            tx.commit()?;
            return Ok(value);
        }
        let exists = tx
            .query_row(
                "SELECT 1 FROM runs WHERE id=?1 AND status='QUEUED'",
                [run_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(Error::Invalid("run is not queued"));
        }
        let value = Attempt {
            id: Uuid::new_v4().to_string(),
            run_id: run_id.into(),
            status: "QUEUED".into(),
            created_at: Utc::now().to_rfc3339(),
        };
        tx.execute(
            "INSERT INTO attempts(id,run_id,status,created_at) VALUES (?1,?2,?3,?4)",
            params![value.id, value.run_id, value.status, value.created_at],
        )?;
        tx.execute(
            "INSERT INTO idempotency VALUES (?1,?2,?3,?4)",
            params![key, "create_attempt", digest, value.id],
        )?;
        record_event(
            &tx,
            &value.id,
            "attempt.created",
            "queued; no worker assignment",
        )?;
        tx.commit()?;
        Ok(value)
    }

    /// Claim is atomic and only returns work whose Campaign is AUTHORIZED and
    /// whose sandbox readiness was independently persisted by a trusted Core
    /// path. This slice intentionally provides no public readiness grant API.
    pub fn claim_attempt(
        &self,
        worker_id: &str,
        now_ms: i64,
        lease_ms: i64,
    ) -> Result<Option<WorkerLease>, Error> {
        if worker_id.trim().is_empty() || lease_ms <= 0 || lease_ms > 60_000 {
            return Err(Error::Invalid("worker lease request"));
        }
        let mut connection = self.0.lock().expect("database mutex poisoned");
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let candidate: Option<String> = tx.query_row(
            "SELECT a.id FROM attempts a JOIN runs r ON r.id=a.run_id JOIN campaigns c ON c.id=r.campaign_id
             LEFT JOIN worker_leases l ON l.attempt_id=a.id
             WHERE a.status IN ('QUEUED','LEASED') AND c.status='AUTHORIZED' AND a.sandbox_ready=1
             AND (l.attempt_id IS NULL OR l.expires_at_ms<=?1) ORDER BY a.created_at,a.id LIMIT 1",
            [now_ms], |r| r.get(0)).optional()?;
        let Some(attempt_id) = candidate else {
            tx.commit()?;
            return Ok(None);
        };
        let prior: Option<(String, i64)> = tx
            .query_row(
                "SELECT worker_id,generation FROM worker_leases WHERE attempt_id=?1",
                [&attempt_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let generation = match prior {
            Some((_, generation)) => generation
                .checked_add(1)
                .ok_or(Error::Invalid("worker generation exhausted"))?,
            None => 1,
        };
        let expires_at_ms = now_ms
            .checked_add(lease_ms)
            .ok_or(Error::Invalid("worker lease expiry overflow"))?;
        tx.execute("INSERT INTO worker_leases VALUES (?1,?2,?3,?4) ON CONFLICT(attempt_id) DO UPDATE SET worker_id=excluded.worker_id,generation=excluded.generation,expires_at_ms=excluded.expires_at_ms", params![attempt_id,worker_id,generation,expires_at_ms])?;
        tx.execute(
            "UPDATE attempts SET status='LEASED' WHERE id=?1",
            [&attempt_id],
        )?;
        record_event(
            &tx,
            &attempt_id,
            "worker.claimed",
            &format!("generation={generation}"),
        )?;
        tx.commit()?;
        Ok(Some(WorkerLease {
            attempt_id,
            worker_id: worker_id.into(),
            generation,
            expires_at_ms,
        }))
    }

    pub fn heartbeat(
        &self,
        attempt_id: &str,
        worker_id: &str,
        generation: i64,
        now_ms: i64,
        lease_ms: i64,
    ) -> Result<WorkerLease, Error> {
        if lease_ms <= 0 || lease_ms > 60_000 {
            return Err(Error::Invalid("worker lease duration"));
        }
        let mut connection = self.0.lock().expect("database mutex poisoned");
        let tx = connection.transaction()?;
        let current: (String,i64,i64,String)=tx.query_row("SELECT l.worker_id,l.generation,l.expires_at_ms,a.status FROM worker_leases l JOIN attempts a ON a.id=l.attempt_id WHERE l.attempt_id=?1",[attempt_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?.ok_or(Error::NotFound)?;
        if current.0 != worker_id
            || current.1 != generation
            || current.2 <= now_ms
            || current.3 != "LEASED"
        {
            return Err(Error::Invalid("stale or expired worker lease"));
        }
        let expires_at_ms = now_ms
            .checked_add(lease_ms)
            .ok_or(Error::Invalid("worker lease expiry overflow"))?;
        tx.execute("UPDATE worker_leases SET expires_at_ms=?1 WHERE attempt_id=?2 AND worker_id=?3 AND generation=?4",params![expires_at_ms,attempt_id,worker_id,generation])?;
        tx.commit()?;
        Ok(WorkerLease {
            attempt_id: attempt_id.into(),
            worker_id: worker_id.into(),
            generation,
            expires_at_ms,
        })
    }

    pub fn report_terminal(&self, report: &WorkerReport, now_ms: i64) -> Result<(), Error> {
        if report.worker_id.trim().is_empty()
            || report.receipt_id.trim().is_empty()
            || report.receipt_id.len() > 128
            || report.generation <= 0
            || !is_sha256(&report.result_sha256)
            || !matches!(report.terminal_status.as_str(), "FAILED" | "CANCELLED")
        {
            return Err(Error::Invalid("worker terminal report"));
        }
        let digest = sha256_hex(
            serde_json::to_string(report)
                .expect("report serializes")
                .as_bytes(),
        );
        let mut connection = self.0.lock().expect("database mutex poisoned");
        let tx = connection.transaction()?;
        let receipt: Option<(String, String)> = tx
            .query_row(
                "SELECT request_digest,attempt_id FROM worker_receipts WHERE receipt_id=?1",
                [&report.receipt_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((saved_digest, saved_attempt)) = receipt {
            if saved_digest == digest && saved_attempt == report.attempt_id {
                tx.commit()?;
                return Ok(());
            }
            return Err(Error::Invalid("receipt id reused with different report"));
        }
        let lease: (String, i64, i64) = tx
            .query_row(
                "SELECT worker_id,generation,expires_at_ms FROM worker_leases WHERE attempt_id=?1",
                [&report.attempt_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        if lease.0 != report.worker_id || lease.1 != report.generation || lease.2 <= now_ms {
            return Err(Error::Invalid("stale or expired worker lease"));
        }
        let changed = tx.execute(
            "UPDATE attempts SET status=?1 WHERE id=?2 AND status='LEASED'",
            params![report.terminal_status, report.attempt_id],
        )?;
        if changed != 1 {
            return Err(Error::Invalid("attempt is not leased"));
        }
        tx.execute(
            "INSERT INTO worker_receipts VALUES (?1,?2,?3,?4)",
            params![
                report.receipt_id,
                digest,
                report.attempt_id,
                Utc::now().to_rfc3339()
            ],
        )?;
        record_event(
            &tx,
            &report.attempt_id,
            "worker.terminal_reported",
            &report.terminal_status,
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn projects(&self) -> Result<Vec<Project>, Error> {
        let connection = self.0.lock().expect("database mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT id,repository,base_commit,created_at FROM projects ORDER BY created_at",
        )?;
        Ok(statement
            .query_map([], project_row)?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn actions(&self) -> Result<Vec<Action>, Error> {
        let connection = self.0.lock().expect("database mutex poisoned");
        let ids = {
            let mut statement = connection.prepare("SELECT id FROM actions ORDER BY created_at")?;
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        ids.iter().map(|id| load_action(&connection, id)).collect()
    }

    pub fn campaigns(&self) -> Result<Vec<Campaign>, Error> {
        let connection = self.0.lock().expect("database mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT id,action_id,status,created_at FROM campaigns ORDER BY created_at,id",
        )?;
        Ok(statement
            .query_map([], |row| {
                Ok(Campaign {
                    id: row.get(0)?,
                    action_id: row.get(1)?,
                    status: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn runs(&self) -> Result<Vec<Run>, Error> {
        let connection = self.0.lock().expect("database mutex poisoned");
        let mut statement = connection
            .prepare("SELECT id,campaign_id,status,created_at FROM runs ORDER BY created_at,id")?;
        Ok(statement
            .query_map([], |r| {
                Ok(Run {
                    id: r.get(0)?,
                    campaign_id: r.get(1)?,
                    status: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn attempts(&self) -> Result<Vec<Attempt>, Error> {
        let connection = self.0.lock().expect("database mutex poisoned");
        let mut statement = connection
            .prepare("SELECT id,run_id,status,created_at FROM attempts ORDER BY created_at,id")?;
        Ok(statement
            .query_map([], |r| {
                Ok(Attempt {
                    id: r.get(0)?,
                    run_id: r.get(1)?,
                    status: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
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

fn project_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        repository: row.get(1)?,
        base_commit: row.get(2)?,
        created_at: row.get(3)?,
    })
}
fn action_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Action> {
    Ok(Action {
        id: row.get(0)?,
        project_id: row.get(1)?,
        objective: row.get(2)?,
        provider: row.get(3)?,
        model: row.get(4)?,
        risk: row.get(5)?,
        status: row.get(6)?,
        objective_sha256: row.get(7)?,
        created_at: row.get(8)?,
        spec: ActionSpec {
            schema_version: 0,
            project_id: String::new(),
            repository: String::new(),
            base_commit: String::new(),
            objective: String::new(),
            objective_sha256: String::new(),
            provider: String::new(),
            model: String::new(),
        },
        spec_sha256: String::new(),
    })
}
fn load_project(connection: &Connection, id: &str) -> Result<Project, Error> {
    Ok(connection.query_row(
        "SELECT id,repository,base_commit,created_at FROM projects WHERE id=?1",
        [id],
        project_row,
    )?)
}
fn load_action(connection: &Connection, id: &str) -> Result<Action, Error> {
    let mut action=connection.query_row("SELECT id,project_id,objective,provider,model,risk,status,objective_sha256,created_at FROM actions WHERE id=?1", [id], action_row)?;
    let (spec_json, saved_digest): (String, String) = connection.query_row(
        "SELECT spec_json,spec_sha256 FROM action_specs WHERE action_id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if sha256_hex(spec_json.as_bytes()) != saved_digest {
        return Err(Error::Invalid("frozen ActionSpec digest mismatch"));
    }
    action.spec_sha256 = saved_digest;
    action.spec = serde_json::from_str(&spec_json)
        .map_err(|_| Error::Invalid("frozen ActionSpec encoding"))?;
    if action.spec.schema_version != 1
        || action.spec.project_id != action.project_id
        || action.spec.objective != action.objective
        || action.spec.objective_sha256 != action.objective_sha256
        || action.spec.provider != action.provider
        || action.spec.model != action.model
        || sha256_hex(action.spec.objective.as_bytes()) != action.objective_sha256
    {
        return Err(Error::Invalid("frozen ActionSpec does not match Action"));
    }
    Ok(action)
}
fn load_campaign(connection: &Connection, id: &str) -> Result<Campaign, Error> {
    Ok(connection.query_row(
        "SELECT id,action_id,status,created_at FROM campaigns WHERE id=?1",
        [id],
        |r| {
            Ok(Campaign {
                id: r.get(0)?,
                action_id: r.get(1)?,
                status: r.get(2)?,
                created_at: r.get(3)?,
            })
        },
    )?)
}
fn load_run(connection: &Connection, id: &str) -> Result<Run, Error> {
    Ok(connection.query_row(
        "SELECT id,campaign_id,status,created_at FROM runs WHERE id=?1",
        [id],
        |r| {
            Ok(Run {
                id: r.get(0)?,
                campaign_id: r.get(1)?,
                status: r.get(2)?,
                created_at: r.get(3)?,
            })
        },
    )?)
}
fn load_attempt(connection: &Connection, id: &str) -> Result<Attempt, Error> {
    Ok(connection.query_row(
        "SELECT id,run_id,status,created_at FROM attempts WHERE id=?1",
        [id],
        |r| {
            Ok(Attempt {
                id: r.get(0)?,
                run_id: r.get(1)?,
                status: r.get(2)?,
                created_at: r.get(3)?,
            })
        },
    )?)
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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_experimental_database_is_rejected_without_modification() {
        let path = std::env::temp_dir().join(format!("factory-db-test-{}.sqlite", Uuid::new_v4()));
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("CREATE TABLE projects (id TEXT PRIMARY KEY); INSERT INTO projects VALUES ('keep');")
            .unwrap();
        drop(connection);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(matches!(Database::open(&path), Err(Error::Invalid(_))));
        let connection = Connection::open(&path).unwrap();
        let retained: String = connection
            .query_row("SELECT id FROM projects", [], |row| row.get(0))
            .unwrap();
        assert_eq!(retained, "keep");
        drop(connection);
        let _ = std::fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn existing_group_readable_database_is_rejected_without_changing_data_or_mode() {
        use std::os::unix::fs::PermissionsExt;
        let path =
            std::env::temp_dir().join(format!("factory-insecure-db-{}.sqlite", Uuid::new_v4()));
        let db = Database::open(&path).unwrap();
        db.0.lock()
            .unwrap()
            .execute_batch(
                "CREATE TABLE retained(value TEXT); INSERT INTO retained VALUES ('keep');",
            )
            .unwrap();
        drop(db);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            Database::open(&path),
            Err(Error::Invalid(
                "existing database file grants group or other access"
            ))
        ));
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );
        let connection = Connection::open(&path).unwrap();
        let value: String = connection
            .query_row("SELECT value FROM retained", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "keep");
        drop(connection);
        let _ = std::fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn database_symlink_path_is_rejected() {
        let directory = std::env::temp_dir().join(format!("factory-db-link-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let target = directory.join("target.sqlite");
        let link = directory.join("linked.sqlite");
        let _db = Database::open(&target).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(matches!(
            Database::open(&link),
            Err(Error::Invalid(
                "database path must be a regular non-symlink file"
            ))
        ));
        drop(_db);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn idempotency_rejects_changed_action_and_cross_operation() {
        let path = std::env::temp_dir().join(format!("factory-db-test-{}.sqlite", Uuid::new_v4()));
        let db = Database::open(&path).unwrap();
        {
            let connection = db.0.lock().unwrap();
            connection
                .execute(
                    "INSERT INTO projects VALUES (?1,?2,?3,?4)",
                    params!["project", "/fixture", "base", "now"],
                )
                .unwrap();
        }
        let action = db
            .create_action("project", "fix bug", "codex", "model-a", "key-one")
            .unwrap();
        assert_eq!(
            action.id,
            db.create_action("project", "fix bug", "codex", "model-a", "key-one")
                .unwrap()
                .id
        );
        assert!(matches!(
            db.create_action("project", "different", "codex", "model-a", "key-one"),
            Err(Error::Invalid(_))
        ));
        {
            let connection = db.0.lock().unwrap();
            assert!(matches!(
                replay(&connection, "key-one", "register_project", "digest"),
                Err(Error::Invalid(_))
            ));
        }
        drop(db);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn action_spec_is_frozen_but_execution_remains_human_required() {
        let path = std::env::temp_dir().join(format!("factory-db-test-{}.sqlite", Uuid::new_v4()));
        let db = Database::open(&path).unwrap();
        {
            let connection = db.0.lock().unwrap();
            connection
                .execute(
                    "INSERT INTO projects VALUES (?1,?2,?3,?4)",
                    params!["project", "/fixture", "base", "now"],
                )
                .unwrap();
        }
        let action = db
            .create_action(
                "project",
                "fix bug",
                "opencode",
                "provider/model",
                "key-two",
            )
            .unwrap();
        assert_eq!(action.status, "HUMAN_REQUIRED");
        assert_eq!(action.risk, "UNASSESSED");
        assert_eq!(action.spec.base_commit, "base");
        assert_eq!(action.spec.schema_version, 1);
        assert_eq!(action.spec.objective, "fix bug");
        let canonical_spec = serde_json::to_string(&action.spec).unwrap();
        assert_eq!(action.spec_sha256, sha256_hex(canonical_spec.as_bytes()));
        assert_ne!(action.spec_sha256, action.objective_sha256);
        let listed = db.actions().unwrap();
        assert_eq!(listed[0].spec, action.spec);
        assert_eq!(listed[0].spec_sha256, action.spec_sha256);
        drop(db);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn campaign_is_durable_but_cannot_create_run_without_authorization() {
        let path = std::env::temp_dir().join(format!("factory-db-test-{}.sqlite", Uuid::new_v4()));
        let db = Database::open(&path).unwrap();
        db.0.lock()
            .unwrap()
            .execute(
                "INSERT INTO projects VALUES (?1,?2,?3,?4)",
                params!["project", "/fixture", "base", "now"],
            )
            .unwrap();
        let action = db
            .create_action("project", "objective", "codex", "model", "action-key")
            .unwrap();
        let campaign = db.create_campaign(&action.id, "campaign-key").unwrap();
        assert_eq!(campaign.status, "BLOCKED_AUTHORIZATION");
        assert_eq!(
            campaign.id,
            db.create_campaign(&action.id, "campaign-key").unwrap().id
        );
        assert_eq!(db.campaigns().unwrap()[0].id, campaign.id);
        assert!(matches!(
            db.create_run(&campaign.id, "run-key"),
            Err(Error::Invalid(_))
        ));
        let events = db.events().unwrap();
        assert!(events.iter().any(|event| event.kind == "campaign.created"));
        drop(db);
        let _ = std::fs::remove_file(path);
    }

    fn authorized_attempt(db: &Database) -> String {
        db.0.lock()
            .unwrap()
            .execute(
                "INSERT INTO projects VALUES (?1,?2,?3,?4)",
                params!["project", "/fixture", "base", "now"],
            )
            .unwrap();
        let action = db
            .create_action("project", "objective", "codex", "model", "action-key")
            .unwrap();
        let campaign = db.create_campaign(&action.id, "campaign-key").unwrap();
        // Fixture setup only: there is deliberately no public authorization or
        // sandbox attestation route in the product API.
        db.0.lock()
            .unwrap()
            .execute(
                "UPDATE campaigns SET status='AUTHORIZED' WHERE id=?1",
                [&campaign.id],
            )
            .unwrap();
        let run = db.create_run(&campaign.id, "run-key").unwrap();
        let attempt = db.create_attempt(&run.id, "attempt-key").unwrap();
        db.0.lock()
            .unwrap()
            .execute(
                "UPDATE attempts SET sandbox_ready=1 WHERE id=?1",
                [&attempt.id],
            )
            .unwrap();
        attempt.id
    }

    #[test]
    fn worker_claim_is_fenced_expires_and_receipts_survive_restart() {
        let path =
            std::env::temp_dir().join(format!("factory-lease-test-{}.sqlite", Uuid::new_v4()));
        let db = Database::open(&path).unwrap();
        let attempt_id = authorized_attempt(&db);
        assert_eq!(db.runs().unwrap().len(), 1);
        assert_eq!(db.attempts().unwrap()[0].id, attempt_id);
        let first = db.claim_attempt("worker-a", 100, 50).unwrap().unwrap();
        assert_eq!(first.generation, 1);
        assert!(db.heartbeat(&attempt_id, "worker-a", 1, 120, 50).is_ok());
        assert!(matches!(
            db.heartbeat(&attempt_id, "worker-a", 1, 170, 50),
            Err(Error::Invalid(_))
        ));
        let second = db.claim_attempt("worker-b", 170, 50).unwrap().unwrap();
        assert_eq!(second.generation, 2);
        let report = WorkerReport {
            attempt_id: attempt_id.clone(),
            worker_id: "worker-b".into(),
            generation: 2,
            receipt_id: "receipt-one".into(),
            result_sha256: "a".repeat(64),
            terminal_status: "FAILED".into(),
        };
        assert!(matches!(
            db.report_terminal(
                &WorkerReport {
                    terminal_status: "SUCCEEDED".into(),
                    ..report.clone()
                },
                180
            ),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            db.report_terminal(
                &WorkerReport {
                    worker_id: "worker-a".into(),
                    generation: 1,
                    ..report.clone()
                },
                180
            ),
            Err(Error::Invalid(_))
        ));
        db.report_terminal(&report, 180).unwrap();
        db.report_terminal(&report, 181).unwrap();
        drop(db);
        let reopened = Database::open(&path).unwrap();
        reopened.report_terminal(&report, 182).unwrap();
        assert_eq!(
            load_attempt(&reopened.0.lock().unwrap(), &attempt_id)
                .unwrap()
                .status,
            "FAILED"
        );
        drop(reopened);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn concurrent_workers_claim_distinct_ready_attempts_atomically() {
        let path =
            std::env::temp_dir().join(format!("factory-claim-test-{}.sqlite", Uuid::new_v4()));
        let db = Database::open(&path).unwrap();
        let first = authorized_attempt(&db);
        db.0.lock()
            .unwrap()
            .execute(
                "INSERT INTO projects VALUES (?1,?2,?3,?4)",
                params!["project-2", "/fixture-2", "base", "now"],
            )
            .unwrap();
        let action = db
            .create_action("project-2", "objective", "codex", "model", "action-key-2")
            .unwrap();
        let campaign = db.create_campaign(&action.id, "campaign-key-2").unwrap();
        db.0.lock()
            .unwrap()
            .execute(
                "UPDATE campaigns SET status='AUTHORIZED' WHERE id=?1",
                [&campaign.id],
            )
            .unwrap();
        let run = db.create_run(&campaign.id, "run-key-2").unwrap();
        let second = db.create_attempt(&run.id, "attempt-key-2").unwrap();
        db.0.lock()
            .unwrap()
            .execute(
                "UPDATE attempts SET sandbox_ready=1 WHERE id=?1",
                [&second.id],
            )
            .unwrap();
        let left = Database::open(&path).unwrap();
        let right = Database::open(&path).unwrap();
        let a =
            std::thread::spawn(move || left.claim_attempt("worker-a", 100, 1000).unwrap().unwrap());
        let b = std::thread::spawn(move || {
            right.claim_attempt("worker-b", 100, 1000).unwrap().unwrap()
        });
        let a = a.join().unwrap();
        let b = b.join().unwrap();
        assert_ne!(a.attempt_id, b.attempt_id);
        assert!(a.attempt_id == first || a.attempt_id == second.id);
        drop(db);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn lease_arithmetic_overflow_fails_closed_without_reusing_generation() {
        let path =
            std::env::temp_dir().join(format!("factory-overflow-test-{}.sqlite", Uuid::new_v4()));
        let db = Database::open(&path).unwrap();
        let attempt = authorized_attempt(&db);
        assert!(matches!(
            db.claim_attempt("worker-a", i64::MAX - 5, 50),
            Err(Error::Invalid(_))
        ));
        assert!(db.claim_attempt("worker-a", 100, 50).unwrap().is_some());
        db.0.lock()
            .unwrap()
            .execute(
                "UPDATE worker_leases SET generation=?1,expires_at_ms=0 WHERE attempt_id=?2",
                params![i64::MAX, attempt],
            )
            .unwrap();
        assert!(matches!(
            db.claim_attempt("worker-b", 10, 50),
            Err(Error::Invalid("worker generation exhausted"))
        ));
        let generation: i64 =
            db.0.lock()
                .unwrap()
                .query_row(
                    "SELECT generation FROM worker_leases WHERE attempt_id=?1",
                    [attempt],
                    |row| row.get(0),
                )
                .unwrap();
        assert_eq!(generation, i64::MAX);
        drop(db);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn worker_registration_stores_only_hash_and_replay_rotates_session_across_restart() {
        let path =
            std::env::temp_dir().join(format!("factory-worker-session-{}.sqlite", Uuid::new_v4()));
        let db = Database::open(&path).unwrap();
        assert!(
            db.register_worker_session(
                "worker-a",
                "registration-a",
                "bootstrap-secret-12345678901234567890",
                "session-token-aaaaaaaaaaaaaaaaaaaaaaaa"
            )
            .unwrap()
            .created
        );
        assert!(
            db.worker_session_matches("worker-a", "session-token-aaaaaaaaaaaaaaaaaaaaaaaa")
                .unwrap()
        );
        assert!(
            !db.worker_session_matches("worker-b", "session-token-aaaaaaaaaaaaaaaaaaaaaaaa")
                .unwrap()
        );
        assert!(
            !db.register_worker_session(
                "worker-a",
                "registration-a",
                "bootstrap-secret-12345678901234567890",
                "session-token-bbbbbbbbbbbbbbbbbbbbbbbb"
            )
            .unwrap()
            .created
        );
        assert!(
            !db.worker_session_matches("worker-a", "session-token-aaaaaaaaaaaaaaaaaaaaaaaa")
                .unwrap()
        );
        assert!(
            db.worker_session_matches("worker-a", "session-token-bbbbbbbbbbbbbbbbbbbbbbbb")
                .unwrap()
        );
        assert!(matches!(
            db.register_worker_session(
                "worker-a",
                "new-key",
                "bootstrap-secret-12345678901234567890",
                "session-token-cccccccccccccccccccccccc"
            ),
            Err(Error::Conflict(_))
        ));
        let event_details: Vec<String> = {
            let connection = db.0.lock().unwrap();
            let mut query = connection
                .prepare("SELECT detail FROM events WHERE subject_id='worker-a'")
                .unwrap();
            query
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert!(
            event_details
                .iter()
                .all(|detail| !detail.contains("session-token"))
        );
        let persisted_registration_key: String =
            db.0.lock()
                .unwrap()
                .query_row(
                    "SELECT key FROM idempotency WHERE operation='register_worker_session'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
        assert_ne!(persisted_registration_key, "registration-a");
        assert_eq!(
            persisted_registration_key,
            format!(
                "worker-registration-v1:{}",
                hmac_sha256_hex(b"bootstrap-secret-12345678901234567890", b"registration-a")
            )
        );
        drop(db);
        let persisted_bytes = std::fs::read(&path).unwrap();
        assert!(
            !persisted_bytes
                .windows(b"registration-a".len())
                .any(|window| window == b"registration-a")
        );
        assert!(
            !persisted_bytes
                .windows(b"bootstrap-secret-12345678901234567890".len())
                .any(|window| window == b"bootstrap-secret-12345678901234567890")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let reopened = Database::open(&path).unwrap();
        assert!(
            reopened
                .worker_session_matches("worker-a", "session-token-bbbbbbbbbbbbbbbbbbbbbbbb")
                .unwrap()
        );
        let persisted_hash: String = reopened
            .0
            .lock()
            .unwrap()
            .query_row(
                "SELECT token_sha256 FROM worker_sessions WHERE worker_id='worker-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            persisted_hash,
            sha256_hex("session-token-bbbbbbbbbbbbbbbbbbbbbbbb".as_bytes())
        );
        drop(reopened);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn action_read_rejects_tampered_frozen_spec() {
        let path = std::env::temp_dir().join(format!("factory-db-test-{}.sqlite", Uuid::new_v4()));
        let db = Database::open(&path).unwrap();
        db.0.lock()
            .unwrap()
            .execute(
                "INSERT INTO projects VALUES (?1,?2,?3,?4)",
                params!["project", "/fixture", "base", "now"],
            )
            .unwrap();
        let action = db
            .create_action("project", "objective", "codex", "model", "action-key")
            .unwrap();
        db.0.lock()
            .unwrap()
            .execute(
                "UPDATE actions SET objective='tampered' WHERE id=?1",
                [&action.id],
            )
            .unwrap();
        assert!(matches!(db.actions(), Err(Error::Invalid(_))));
        drop(db);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn project_retry_keeps_first_base_after_repository_advances() {
        let root = std::env::temp_dir().join(format!("factory-project-test-{} ", Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let git = |arguments: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(&root)
                    .args(arguments)
                    .status()
                    .unwrap()
                    .success()
            );
        };
        git(&["init", "-q"]);
        git(&["config", "user.name", "Fixture"]);
        git(&["config", "user.email", "fixture@example.invalid"]);
        std::fs::write(root.join("content"), "one").unwrap();
        git(&["add", "content"]);
        git(&["commit", "-qm", "first"]);
        std::fs::create_dir(root.join("nested")).unwrap();
        let db = Database::open(&root.join("factory.sqlite")).unwrap();
        let first = db.register_project(&root, "same-request").unwrap();
        assert_eq!(first.repository, root.to_string_lossy());
        std::fs::write(root.join("content"), "two").unwrap();
        git(&["add", "content"]);
        git(&["commit", "-qm", "second"]);
        let replayed = db
            .register_project(&root.join("nested"), "same-request")
            .unwrap();
        assert_eq!(first.id, replayed.id);
        assert_eq!(first.base_commit, replayed.base_commit);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
}
