use std::{path::Path, sync::Mutex, time::Duration};

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("invalid project repository: {0}")]
    Repository(String),
    #[error("invalid request: {0}")]
    Invalid(&'static str),
    #[error("record not found")]
    NotFound,
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
        let connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
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
        let exists = transaction
            .query_row("SELECT 1 FROM projects WHERE id=?1", [project_id], |_| {
                Ok(())
            })
            .optional()?
            .is_some();
        if !exists {
            return Err(Error::NotFound);
        }
        let objective_sha256 = sha256_hex(objective.as_bytes());
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
        let mut statement = connection.prepare("SELECT id,project_id,objective,provider,model,risk,status,objective_sha256,created_at FROM actions ORDER BY created_at")?;
        Ok(statement
            .query_map([], action_row)?
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
    Ok(connection.query_row("SELECT id,project_id,objective,provider,model,risk,status,objective_sha256,created_at FROM actions WHERE id=?1", [id], action_row)?)
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn action_remains_human_required_without_risk_and_spec() {
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
