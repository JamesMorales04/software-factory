use super::*;

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use uuid::Uuid;

impl Database {
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
