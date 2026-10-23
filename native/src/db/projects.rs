use super::*;

use chrono::Utc;
use sqlx_sqlite::SqliteConnection;
use std::path::{Path, PathBuf};
use uuid::Uuid;

impl Database {
    pub async fn register_project(&self, repository: &Path, key: &str) -> Result<Project, Error> {
        if key.trim().is_empty() || key.len() > 256 {
            return Err(Error::Invalid("idempotency key"));
        }
        let requested = repository.to_path_buf();
        let (repository, base_commit) =
            tokio::task::spawn_blocking(move || resolve_repository(&requested))
                .await
                .map_err(|error| Error::Repository(format!("repository task failed: {error}")))??;
        let request_digest = sha256_hex(repository.as_bytes());
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        if let Some(id) = replay(&mut transaction, key, "register_project", &request_digest).await?
        {
            let project = load_project(&mut transaction, &id).await?;
            transaction.commit().await?;
            return Ok(project);
        }
        let project = Project {
            id: Uuid::new_v4().to_string(),
            repository,
            base_commit,
            created_at: Utc::now().to_rfc3339(),
        };
        sqlx::query::query("INSERT INTO projects VALUES (?1,?2,?3,?4)")
            .bind(&project.id)
            .bind(&project.repository)
            .bind(&project.base_commit)
            .bind(&project.created_at)
            .execute(&mut *transaction)
            .await?;
        sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
            .bind(key)
            .bind("register_project")
            .bind(&request_digest)
            .bind(&project.id)
            .execute(&mut *transaction)
            .await?;
        record_event(
            &mut transaction,
            &project.id,
            "project.registered",
            "registered",
        )
        .await?;
        transaction.commit().await?;
        Ok(project)
    }

    pub async fn create_action(
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
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        if let Some(id) = replay(&mut transaction, key, "create_action", &request_digest).await? {
            let action = load_action(&mut transaction, &id).await?;
            transaction.commit().await?;
            return Ok(action);
        }
        let project = load_project(&mut transaction, project_id).await?;
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
        sqlx::query::query("INSERT INTO actions VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)")
            .bind(&action.id)
            .bind(&action.project_id)
            .bind(&action.objective)
            .bind(&action.provider)
            .bind(&action.model)
            .bind(&action.risk)
            .bind(&action.status)
            .bind(&action.objective_sha256)
            .bind(&action.created_at)
            .execute(&mut *transaction)
            .await?;
        sqlx::query::query("INSERT INTO action_specs VALUES (?1,?2,?3)")
            .bind(&action.id)
            .bind(spec_json)
            .bind(&spec_sha256)
            .execute(&mut *transaction)
            .await?;
        sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
            .bind(key)
            .bind("create_action")
            .bind(&request_digest)
            .bind(&action.id)
            .execute(&mut *transaction)
            .await?;
        record_event(
            &mut transaction,
            &action.id,
            "action.created",
            "awaiting risk classification and execution authority",
        )
        .await?;
        transaction.commit().await?;
        Ok(action)
    }

    pub async fn projects(&self) -> Result<Vec<Project>, Error> {
        let rows: Vec<(String, String, String, String)> = sqlx::query_as::query_as(
            "SELECT id,repository,base_commit,created_at FROM projects ORDER BY created_at",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, repository, base_commit, created_at)| Project {
                id,
                repository,
                base_commit,
                created_at,
            })
            .collect())
    }

    pub async fn actions(&self) -> Result<Vec<Action>, Error> {
        let ids: Vec<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM actions ORDER BY created_at")
                .fetch_all(&self.pool)
                .await?;
        let mut actions = Vec::with_capacity(ids.len());
        for (id,) in ids {
            let mut connection = self.pool.acquire().await?;
            actions.push(load_action(&mut connection, &id).await?);
        }
        Ok(actions)
    }
}

fn resolve_repository(repository: &Path) -> Result<(String, String), Error> {
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
    let root: PathBuf = Path::new(root)
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
    Ok((root.to_string_lossy().to_string(), base_commit))
}

async fn load_project(connection: &mut SqliteConnection, id: &str) -> Result<Project, Error> {
    let row: Option<(String, String, String, String)> = sqlx::query_as::query_as(
        "SELECT id,repository,base_commit,created_at FROM projects WHERE id=?1",
    )
    .bind(id)
    .fetch_optional(connection)
    .await?;
    row.map(|(id, repository, base_commit, created_at)| Project {
        id,
        repository,
        base_commit,
        created_at,
    })
    .ok_or(Error::NotFound)
}

type ActionColumns = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
);

async fn load_action(connection: &mut SqliteConnection, id: &str) -> Result<Action, Error> {
    let row: Option<ActionColumns> = sqlx::query_as::query_as(
        "SELECT id,project_id,objective,provider,model,risk,status,objective_sha256,created_at FROM actions WHERE id=?1",
    )
    .bind(id)
    .fetch_optional(&mut *connection)
    .await?;
    let (id, project_id, objective, provider, model, risk, status, objective_sha256, created_at) =
        row.ok_or(Error::NotFound)?;
    let (spec_json, saved_digest): (String, String) = sqlx::query_as::query_as(
        "SELECT spec_json,spec_sha256 FROM action_specs WHERE action_id=?1",
    )
    .bind(&id)
    .fetch_one(&mut *connection)
    .await?;
    if sha256_hex(spec_json.as_bytes()) != saved_digest {
        return Err(Error::Invalid("frozen ActionSpec digest mismatch"));
    }
    let spec: ActionSpec = serde_json::from_str(&spec_json)
        .map_err(|_| Error::Invalid("frozen ActionSpec encoding"))?;
    if spec.schema_version != 1
        || spec.project_id != project_id
        || spec.objective != objective
        || spec.objective_sha256 != objective_sha256
        || spec.provider != provider
        || spec.model != model
        || sha256_hex(spec.objective.as_bytes()) != objective_sha256
    {
        return Err(Error::Invalid("frozen ActionSpec does not match Action"));
    }
    Ok(Action {
        id,
        project_id,
        objective,
        provider,
        model,
        risk,
        status,
        objective_sha256,
        created_at,
        spec,
        spec_sha256: saved_digest,
    })
}
