use super::*;

use chrono::Utc;
use sqlx_sqlite::SqliteConnection;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::project_import::ProjectDiscovery;

/// Explicit source metadata captured when a Project is registered. The source
/// key is the duplicate-identity guard and must never contain credentials.
#[derive(Clone, Debug, Default)]
pub struct RegisterProjectSource {
    pub name: String,
    pub source_kind: String,
    pub source_url: String,
    pub source_key: String,
    pub source_visibility: String,
    pub default_branch: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCampaignSummary {
    pub id: String,
    pub action_id: String,
    pub objective: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDiscoveryRecord {
    pub project_id: String,
    pub base_commit: String,
    pub scanned_at: String,
    pub discovery: ProjectDiscovery,
}

type ProjectColumns = (
    String,
    String,
    String,
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

struct ResolvedRepository {
    repository: String,
    base_commit: String,
    identity: String,
    branch: String,
    remote_origin: String,
}

impl Database {
    pub async fn register_project(&self, repository: &Path, key: &str) -> Result<Project, Error> {
        self.register_project_with_source(repository, &RegisterProjectSource::default(), key)
            .await
    }

    pub async fn register_project_with_source(
        &self,
        repository: &Path,
        source: &RegisterProjectSource,
        key: &str,
    ) -> Result<Project, Error> {
        if key.trim().is_empty() || key.len() > 256 {
            return Err(Error::Invalid("idempotency key"));
        }
        let requested = repository.to_path_buf();
        let resolved = tokio::task::spawn_blocking(move || resolve_repository(&requested))
            .await
            .map_err(|error| Error::Repository(format!("repository task failed: {error}")))??;
        let repository = resolved.repository;
        let source_kind = if source.source_kind.trim().is_empty() {
            "local".to_owned()
        } else {
            source.source_kind.trim().to_owned()
        };
        if !matches!(source_kind.as_str(), "local" | "git" | "github") {
            return Err(Error::Invalid("project source kind"));
        }
        let name = if source.name.trim().is_empty() {
            default_project_name(&source_kind, &source.source_url, &repository)
        } else {
            source.name.trim().to_owned()
        };
        if name.is_empty()
            || name.chars().count() > crate::project_import::MAX_PROJECT_NAME_CHARS
            || name.chars().any(char::is_control)
        {
            return Err(Error::Invalid("project name"));
        }
        let source_url = source.source_url.trim().to_owned();
        if source_url.len() > 2_048 || source_url.chars().any(char::is_control) {
            return Err(Error::Invalid("project source URL"));
        }
        let source_key = if source.source_key.trim().is_empty() {
            format!("local:{repository}")
        } else {
            source.source_key.trim().to_owned()
        };
        if source_key.len() > 4_096 || source_key.chars().any(char::is_control) {
            return Err(Error::Invalid("project source identity"));
        }
        let source_visibility = source.source_visibility.trim().to_owned();
        if !matches!(source_visibility.as_str(), "" | "public" | "private") {
            return Err(Error::Invalid("project source visibility"));
        }
        let default_branch = if source.default_branch.trim().is_empty() {
            resolved.branch.clone()
        } else {
            source.default_branch.trim().to_owned()
        };
        if default_branch.len() > 255
            || default_branch.chars().any(char::is_control)
            || default_branch.contains(char::is_whitespace)
        {
            return Err(Error::Invalid("project default branch"));
        }
        let request_digest = sha256_hex(
            serde_json::to_string(&(
                &repository,
                &name,
                &source_kind,
                &source_url,
                &source_key,
                &source_visibility,
                &default_branch,
            ))
            .expect("project registration digest serializes")
            .as_bytes(),
        );
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        if let Some(id) = replay(&mut transaction, key, "register_project", &request_digest).await?
        {
            let project = load_project(&mut transaction, &id).await?;
            transaction.commit().await?;
            return Ok(project);
        }
        let duplicate: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT id FROM projects WHERE source_key=?1 OR repository=?2 LIMIT 1",
        )
        .bind(&source_key)
        .bind(&repository)
        .fetch_optional(&mut *transaction)
        .await?;
        if duplicate.is_some() {
            return Err(Error::Conflict(
                "project source is already registered; open the existing Project",
            ));
        }
        let project = Project {
            id: Uuid::new_v4().to_string(),
            repository,
            base_commit: resolved.base_commit,
            created_at: Utc::now().to_rfc3339(),
            identity: resolved.identity,
            name,
            source_kind,
            source_url,
            source_key,
            source_visibility,
            default_branch,
            remote_origin: resolved.remote_origin,
        };
        sqlx::query::query(
            "INSERT INTO projects
             (id,repository,base_commit,created_at,identity,name,source_kind,source_url,
              source_key,source_visibility,default_branch,remote_origin)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        )
        .bind(&project.id)
        .bind(&project.repository)
        .bind(&project.base_commit)
        .bind(&project.created_at)
        .bind(&project.identity)
        .bind(&project.name)
        .bind(&project.source_kind)
        .bind(&project.source_url)
        .bind(&project.source_key)
        .bind(&project.source_visibility)
        .bind(&project.default_branch)
        .bind(&project.remote_origin)
        .execute(&mut *transaction)
        .await?;
        self.ensure_project_config(&mut transaction, &project.id)
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
            &format!(
                "source={} key={} base={}",
                project.source_kind, project.source_key, project.base_commit
            ),
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
        self.create_action_with_spec(
            project_id,
            objective,
            provider,
            model,
            key,
            ActionSpecInput {
                allowed_paths: &[],
                checks: &[],
            },
        )
        .await
    }

    pub async fn create_action_with_spec(
        &self,
        project_id: &str,
        objective: &str,
        provider: &str,
        model: &str,
        key: &str,
        spec_input: ActionSpecInput<'_>,
    ) -> Result<Action, Error> {
        let allowed_paths = spec_input.allowed_paths;
        let checks = spec_input.checks;
        if !allowed_paths.is_empty() {
            crate::lead::validate_repository_paths("allowedPaths", allowed_paths)
                .map_err(|error| Error::Delegation(error.to_string()))?;
        }
        crate::lead::validate_mixed_checks(checks)
            .map_err(|error| Error::Delegation(error.to_string()))?;
        if objective.trim().is_empty() || objective.len() > 20_000 {
            return Err(Error::Invalid("objective length"));
        }
        if !matches!(provider, "codex" | "opencode" | "opencode-go") {
            return Err(Error::Invalid("provider"));
        }
        if model.trim().is_empty() || model.len() > 128 {
            return Err(Error::Invalid("model"));
        }
        let request_digest = sha256_hex(
            serde_json::to_string(&(
                project_id,
                objective,
                provider,
                model,
                allowed_paths,
                checks,
            ))
            .expect("action digest serializes")
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
            allowed_paths: allowed_paths.to_vec(),
            checks: checks.to_vec(),
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

    pub async fn classify_action(
        &self,
        action_id: &str,
        risk_class: &str,
        actor: &str,
        rationale: &str,
        key: &str,
    ) -> Result<Action, Error> {
        if risk_class != "R0" {
            return Err(Error::Forbidden(
                "only R0 fixture actions can be classified in this slice".into(),
            ));
        }
        if actor.trim().is_empty() || rationale.trim().is_empty() || key.trim().is_empty() {
            return Err(Error::Invalid("action classification input"));
        }
        let digest = sha256_hex(
            serde_json::to_string(&(action_id, risk_class, actor, rationale))
                .expect("classification digest serializes")
                .as_bytes(),
        );
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        if let Some(id) = replay(&mut transaction, key, "classify_action", &digest).await? {
            let action = load_action(&mut transaction, &id).await?;
            transaction.commit().await?;
            return Ok(action);
        }
        let current: Option<(String, String)> =
            sqlx::query_as::query_as("SELECT risk,status FROM actions WHERE id=?1")
                .bind(action_id)
                .fetch_optional(&mut *transaction)
                .await?;
        let (risk, status) = current.ok_or(Error::NotFound)?;
        if risk != "UNASSESSED" || status != "HUMAN_REQUIRED" {
            return Err(Error::Conflict("action is already classified"));
        }
        sqlx::query::query("UPDATE actions SET risk=?1,status='AUTHORIZED' WHERE id=?2")
            .bind(risk_class)
            .bind(action_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
            .bind(key)
            .bind("classify_action")
            .bind(&digest)
            .bind(action_id)
            .execute(&mut *transaction)
            .await?;
        record_event(
            &mut transaction,
            action_id,
            "action.classified",
            &format!("{risk_class} by {actor}: {rationale}"),
        )
        .await?;
        let action = load_action(&mut transaction, action_id).await?;
        transaction.commit().await?;
        Ok(action)
    }

    pub async fn project(&self, id: &str) -> Result<Project, Error> {
        let mut connection = self.pool.acquire().await?;
        load_project(&mut connection, id).await
    }

    pub async fn project_by_source_key(&self, source_key: &str) -> Result<Option<Project>, Error> {
        if source_key.is_empty() {
            return Ok(None);
        }
        let row: Option<ProjectColumns> = sqlx::query_as::query_as(
            "SELECT id,repository,base_commit,created_at,identity,name,source_kind,source_url,
                    source_key,source_visibility,default_branch,remote_origin
             FROM projects WHERE source_key=?1",
        )
        .bind(source_key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(project_from_columns))
    }

    pub async fn project_exists(&self, id: &str) -> Result<bool, Error> {
        let row: Option<(i64,)> = sqlx::query_as::query_as("SELECT 1 FROM projects WHERE id=?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.is_some())
    }

    pub async fn projects(&self) -> Result<Vec<Project>, Error> {
        let rows: Vec<ProjectColumns> = sqlx::query_as::query_as(
            "SELECT id,repository,base_commit,created_at,identity,name,source_kind,source_url,
                    source_key,source_visibility,default_branch,remote_origin
             FROM projects ORDER BY created_at",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(project_from_columns).collect())
    }

    pub async fn project_campaigns(
        &self,
        project_id: &str,
    ) -> Result<Vec<ProjectCampaignSummary>, Error> {
        let rows: Vec<(String, String, String, String, String)> = sqlx::query_as::query_as(
            "SELECT c.id,c.action_id,a.objective,c.status,c.created_at
             FROM campaigns c JOIN actions a ON a.id=c.action_id
             WHERE a.project_id=?1 ORDER BY c.created_at DESC, c.id",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, action_id, objective, status, created_at)| ProjectCampaignSummary {
                    id,
                    action_id,
                    objective,
                    status,
                    created_at,
                },
            )
            .collect())
    }

    /// Latest durable event for the Project itself or any Action, Campaign,
    /// Run or Attempt reached through it. Never fabricated from file mtimes.
    pub async fn project_last_activity(&self, project_id: &str) -> Result<Option<String>, Error> {
        let row: Option<(Option<String>,)> = sqlx::query_as::query_as(
            "SELECT MAX(recorded_at) FROM events
             WHERE subject_id=?1
                OR subject_id IN (SELECT id FROM actions WHERE project_id=?1)
                OR subject_id IN (
                    SELECT c.id FROM campaigns c JOIN actions a ON a.id=c.action_id
                    WHERE a.project_id=?1)
                OR subject_id IN (
                    SELECT r.id FROM runs r JOIN campaigns c ON c.id=r.campaign_id
                    JOIN actions a ON a.id=c.action_id WHERE a.project_id=?1)
                OR subject_id IN (
                    SELECT at.id FROM attempts at JOIN runs r ON r.id=at.run_id
                    JOIN campaigns c ON c.id=r.campaign_id
                    JOIN actions a ON a.id=c.action_id WHERE a.project_id=?1)
                OR subject_id IN (
                    SELECT cd.id FROM candidates cd JOIN attempts at ON at.id=cd.attempt_id
                    JOIN runs r ON r.id=at.run_id
                    JOIN campaigns c ON c.id=r.campaign_id
                    JOIN actions a ON a.id=c.action_id WHERE a.project_id=?1)",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(|(value,)| value))
    }

    pub async fn save_project_discovery(
        &self,
        project_id: &str,
        discovery: &ProjectDiscovery,
    ) -> Result<ProjectDiscoveryRecord, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let exists: Option<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM projects WHERE id=?1")
                .bind(project_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if exists.is_none() {
            return Err(Error::NotFound);
        }
        let encoded = serde_json::to_string(discovery)
            .map_err(|_| Error::Invalid("project discovery encoding"))?;
        sqlx::query::query(
            "INSERT INTO project_discoveries(project_id,base_commit,discovery_json,scanned_at)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(project_id) DO UPDATE SET
               base_commit=excluded.base_commit,
               discovery_json=excluded.discovery_json,
               scanned_at=excluded.scanned_at",
        )
        .bind(project_id)
        .bind(&discovery.base_commit)
        .bind(&encoded)
        .bind(&discovery.scanned_at)
        .execute(&mut *transaction)
        .await?;
        record_event(
            &mut transaction,
            project_id,
            "project.discovery_completed",
            &format!(
                "base={} toolchains={} capabilities={} truncated={}",
                discovery.base_commit,
                discovery.toolchains.len(),
                discovery.suggested_capabilities.len(),
                discovery.truncated
            ),
        )
        .await?;
        transaction.commit().await?;
        Ok(ProjectDiscoveryRecord {
            project_id: project_id.to_owned(),
            base_commit: discovery.base_commit.clone(),
            scanned_at: discovery.scanned_at.clone(),
            discovery: discovery.clone(),
        })
    }

    pub async fn project_discovery(
        &self,
        project_id: &str,
    ) -> Result<Option<ProjectDiscoveryRecord>, Error> {
        if !self.project_exists(project_id).await? {
            return Err(Error::NotFound);
        }
        let row: Option<(String, String, String)> = sqlx::query_as::query_as(
            "SELECT base_commit,discovery_json,scanned_at FROM project_discoveries
             WHERE project_id=?1",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((base_commit, discovery_json, scanned_at)) = row else {
            return Ok(None);
        };
        let discovery: ProjectDiscovery = serde_json::from_str(&discovery_json)
            .map_err(|_| Error::Invalid("stored project discovery encoding"))?;
        if discovery.base_commit != base_commit {
            return Err(Error::Invalid(
                "stored project discovery commit does not match",
            ));
        }
        Ok(Some(ProjectDiscoveryRecord {
            project_id: project_id.to_owned(),
            base_commit,
            scanned_at,
            discovery,
        }))
    }

    pub async fn action(&self, id: &str) -> Result<Action, Error> {
        let mut connection = self.pool.acquire().await?;
        load_action(&mut connection, id).await
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

fn resolve_repository(repository: &Path) -> Result<ResolvedRepository, Error> {
    let canonical = repository
        .canonicalize()
        .map_err(|error| Error::Repository(error.to_string()))?;
    if !canonical.is_dir() {
        return Err(Error::Repository("not a directory".into()));
    }
    let canonical_text = canonical
        .to_str()
        .ok_or(Error::Invalid("repository path encoding"))?;
    let top_level = std::process::Command::new("git")
        .args(["-C", canonical_text, "rev-parse", "--show-toplevel"])
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
    let inspected = crate::repository::inspect(&root, &["HEAD".to_string()])
        .map_err(|error| Error::Repository(error.to_string()))?;
    let base_commit = inspected
        .base_tips
        .get("HEAD")
        .cloned()
        .ok_or_else(|| Error::Repository("repository has no HEAD commit".into()))?;
    let branch = git_text(&root, &["symbolic-ref", "--short", "HEAD"])
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "HEAD".to_owned());
    if branch.len() > 255 || branch.chars().any(char::is_control) {
        return Err(Error::Repository(
            "repository current branch has an unsafe shape".into(),
        ));
    }
    let remote_origin = git_text(&root, &["remote", "get-url", "origin"])
        .map(|value| sanitize_remote_origin(&value))
        .unwrap_or_default();
    Ok(ResolvedRepository {
        repository: inspected.path.to_string_lossy().to_string(),
        base_commit,
        identity: inspected.identity,
        branch,
        remote_origin,
    })
}

fn git_text(repository: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_owned())
}

/// A Git remote may be an scp-like SSH location; user names there are not
/// credentials. URL-embedded passwords are removed before persistence.
fn sanitize_remote_origin(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter(|character| !character.is_control())
        .take(2_048)
        .collect();
    match reqwest::Url::parse(&cleaned) {
        Ok(mut url) if !matches!(url.scheme(), "file") => {
            let has_credentials = url.password().is_some() || url.username() != "";
            if has_credentials {
                let _ = url.set_username("");
                let _ = url.set_password(None);
            }
            url.to_string()
        }
        _ => cleaned,
    }
}

fn default_project_name(source_kind: &str, source_url: &str, repository: &str) -> String {
    let candidate = if source_kind == "github" || !source_url.is_empty() {
        source_url
    } else {
        repository
    };
    let candidate = candidate.trim_end_matches('/');
    let candidate = candidate.rsplit('/').next().unwrap_or(candidate);
    let candidate = candidate.strip_suffix(".git").unwrap_or(candidate);
    let name: String = candidate
        .chars()
        .filter(|character| !character.is_control())
        .take(crate::project_import::MAX_PROJECT_NAME_CHARS)
        .collect();
    if name.is_empty() {
        "Project".to_owned()
    } else {
        name
    }
}

fn project_from_columns(columns: ProjectColumns) -> Project {
    let (
        id,
        repository,
        base_commit,
        created_at,
        identity,
        name,
        source_kind,
        source_url,
        source_key,
        source_visibility,
        default_branch,
        remote_origin,
    ) = columns;
    Project {
        id,
        repository,
        base_commit,
        created_at,
        identity,
        name,
        source_kind,
        source_url,
        source_key,
        source_visibility,
        default_branch,
        remote_origin,
    }
}

async fn load_project(connection: &mut SqliteConnection, id: &str) -> Result<Project, Error> {
    let row: Option<ProjectColumns> = sqlx::query_as::query_as(
        "SELECT id,repository,base_commit,created_at,identity,name,source_kind,source_url,
                source_key,source_visibility,default_branch,remote_origin
         FROM projects WHERE id=?1",
    )
    .bind(id)
    .fetch_optional(connection)
    .await?;
    row.map(project_from_columns).ok_or(Error::NotFound)
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
