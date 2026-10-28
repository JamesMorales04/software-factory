//! Durable Execution Profile revisions, defaults and audit.
//!
//! Core owns profile identity, immutable revision content, default bindings
//! and the audit trail. Executability is derived on read from Core-owned
//! provider settings, never supplied by a client. A stale expected revision is
//! a conflict; an unsupported route is rejected by the domain before write.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::row::Row;
use sqlx_sqlite::{SqliteConnection, SqliteRow};
use uuid::Uuid;

use super::{Database, Error};
use crate::execution_profile::{
    ExecutionProfileContent, ProviderRouteAvailability, evaluate_executability,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionProfileRecord {
    pub id: String,
    pub revision: u32,
    pub revision_id: String,
    pub content_hash: String,
    pub global_default: bool,
    pub project_defaults: Vec<String>,
    #[serde(flatten)]
    pub content: ExecutionProfileContent,
    pub executable: bool,
    pub warnings: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionProfileAudit {
    pub audit_id: String,
    pub profile_id: String,
    pub revision: Option<u32>,
    pub action: String,
    pub actor: String,
    pub detail: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionProfileDefaultSource {
    ProjectDefault,
    GlobalDefault,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionProfileDefaultBinding {
    pub profile_id: String,
    pub scope: String,
    pub project_id: Option<String>,
    pub revision_id: String,
    pub revision: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedExecutionProfile {
    pub project_id: String,
    pub profile_id: String,
    pub revision_id: String,
    pub revision: u32,
    pub content_hash: String,
    pub source: ExecutionProfileDefaultSource,
}

#[derive(Debug)]
struct ProfileRow {
    id: String,
    revision: u32,
    revision_id: String,
    content_json: String,
    content_hash: String,
    profile_hash: String,
    created_at: String,
    updated_at: String,
}

impl Database {
    pub async fn create_execution_profile(
        &self,
        content: &ExecutionProfileContent,
        actor: &str,
    ) -> Result<ExecutionProfileRecord, Error> {
        validate_actor(actor)?;
        let content_hash = content.validate_and_hash()?;
        let content_json =
            serde_json::to_string(content).map_err(|_| Error::Invalid("profile encoding"))?;
        let id = Uuid::new_v4().to_string();
        let revision_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        sqlx::query::query(
            "INSERT INTO execution_profiles
             (id,current_revision,content_hash,created_at,updated_at)
             VALUES (?1,1,?2,?3,?3)",
        )
        .bind(&id)
        .bind(&content_hash)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query(
            "INSERT INTO execution_profile_revisions
             (revision_id,profile_id,revision,content_json,content_hash,actor,created_at)
             VALUES (?1,?2,1,?3,?4,?5,?6)",
        )
        .bind(&revision_id)
        .bind(&id)
        .bind(&content_json)
        .bind(&content_hash)
        .bind(actor)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        insert_audit(
            &mut transaction,
            &id,
            Some(1),
            "created",
            actor,
            &format!("content_hash={content_hash}"),
            &now,
        )
        .await?;
        transaction.commit().await?;
        self.execution_profile(&id).await
    }

    pub async fn update_execution_profile(
        &self,
        id: &str,
        expected_revision: u32,
        content: &ExecutionProfileContent,
        actor: &str,
    ) -> Result<ExecutionProfileRecord, Error> {
        validate_actor(actor)?;
        if expected_revision == 0 {
            return Err(Error::Invalid("execution profile revision"));
        }
        let content_hash = content.validate_and_hash()?;
        let content_json =
            serde_json::to_string(content).map_err(|_| Error::Invalid("profile encoding"))?;
        let now = Utc::now().to_rfc3339();

        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let current: Option<(i64,)> =
            sqlx::query_as::query_as("SELECT current_revision FROM execution_profiles WHERE id=?1")
                .bind(id)
                .fetch_optional(&mut *transaction)
                .await?;
        let Some((current,)) = current else {
            return Err(Error::NotFound);
        };
        let current = u32::try_from(current)
            .map_err(|_| Error::Conflict("stored execution profile revision is invalid"))?;
        if current != expected_revision {
            return Err(Error::Conflict("execution profile revision is stale"));
        }
        let next = current
            .checked_add(1)
            .ok_or(Error::Conflict("execution profile revision overflow"))?;
        let revision_id = Uuid::new_v4().to_string();
        sqlx::query::query(
            "INSERT INTO execution_profile_revisions
             (revision_id,profile_id,revision,content_json,content_hash,actor,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
        )
        .bind(&revision_id)
        .bind(id)
        .bind(i64::from(next))
        .bind(&content_json)
        .bind(&content_hash)
        .bind(actor)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query(
            "UPDATE execution_profiles
             SET current_revision=?1, content_hash=?2, updated_at=?3
             WHERE id=?4",
        )
        .bind(i64::from(next))
        .bind(&content_hash)
        .bind(&now)
        .bind(id)
        .execute(&mut *transaction)
        .await?;
        insert_audit(
            &mut transaction,
            id,
            Some(next),
            "updated",
            actor,
            &format!("revision={next} content_hash={content_hash}"),
            &now,
        )
        .await?;
        transaction.commit().await?;
        self.execution_profile(id).await
    }

    pub async fn execution_profile(&self, id: &str) -> Result<ExecutionProfileRecord, Error> {
        let mut connection = self.pool.acquire().await?;
        load_execution_profile(&mut connection, id).await
    }

    pub async fn execution_profiles(&self) -> Result<Vec<ExecutionProfileRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        let rows = sqlx::query::query(
            "SELECT p.id,p.current_revision,p.content_hash AS profile_hash,
                    p.created_at,p.updated_at,r.revision_id,r.content_json,r.content_hash
             FROM execution_profiles p
             JOIN execution_profile_revisions r
               ON r.profile_id=p.id AND r.revision=p.current_revision
             ORDER BY p.created_at,p.id",
        )
        .fetch_all(&mut *connection)
        .await?;
        let availability = load_provider_availability(&mut connection).await?;
        let defaults = load_defaults(&mut connection).await?;
        let mut profiles = Vec::with_capacity(rows.len());
        for row in rows.iter() {
            let profile = profile_from_row(row)?;
            let mut project_defaults = defaults
                .iter()
                .filter(|(scope, profile_id, _)| scope == "project" && profile_id == &profile.id)
                .map(|(_, _, project_id)| project_id.clone())
                .collect::<Vec<_>>();
            project_defaults.sort();
            let global_default = defaults
                .iter()
                .any(|(scope, profile_id, _)| scope == "global" && profile_id == &profile.id);
            profiles.push(build_record(
                profile,
                &availability,
                global_default,
                project_defaults,
            )?);
        }
        Ok(profiles)
    }

    pub async fn set_execution_profile_default(
        &self,
        profile_id: &str,
        project_id: Option<&str>,
        actor: &str,
    ) -> Result<ExecutionProfileDefaultBinding, Error> {
        validate_actor(actor)?;
        if let Some(project_id) = project_id {
            validate_project_id(project_id)?;
        }
        let scope = if project_id.is_some() {
            "project"
        } else {
            "global"
        };
        let now = Utc::now().to_rfc3339();

        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        if let Some(project_id) = project_id {
            ensure_project(&mut transaction, project_id).await?;
        }
        let current = load_current_content(&mut transaction, profile_id).await?;
        let availability = load_provider_availability(&mut transaction).await?;
        let (executable, _) = evaluate_executability(&current.content, &availability);
        if !executable {
            return Err(Error::Conflict("execution profile is not executable"));
        }
        let existing = load_bound_profile(&mut transaction, scope, project_id).await?;
        if existing.as_deref() != Some(profile_id) {
            sqlx::query::query(
                "DELETE FROM execution_profile_defaults
                 WHERE scope=?1 AND (project_id IS ?2 OR project_id=?2)",
            )
            .bind(scope)
            .bind(project_id)
            .execute(&mut *transaction)
            .await?;
            sqlx::query::query(
                "INSERT INTO execution_profile_defaults
                 (scope,project_id,profile_id,actor,updated_at)
                 VALUES (?1,?2,?3,?4,?5)",
            )
            .bind(scope)
            .bind(project_id)
            .bind(profile_id)
            .bind(actor)
            .bind(&now)
            .execute(&mut *transaction)
            .await?;
            insert_audit(
                &mut transaction,
                profile_id,
                Some(current.revision),
                if project_id.is_some() {
                    "project_default_set"
                } else {
                    "global_default_set"
                },
                actor,
                &format!(
                    "project={} revision={}",
                    project_id.unwrap_or("global"),
                    current.revision
                ),
                &now,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(ExecutionProfileDefaultBinding {
            profile_id: profile_id.to_owned(),
            scope: scope.to_owned(),
            project_id: project_id.map(str::to_owned),
            revision_id: current.revision_id,
            revision: current.revision,
        })
    }

    pub async fn clear_execution_profile_default(
        &self,
        profile_id: &str,
        project_id: Option<&str>,
        actor: &str,
    ) -> Result<bool, Error> {
        validate_actor(actor)?;
        if let Some(project_id) = project_id {
            validate_project_id(project_id)?;
        }
        let scope = if project_id.is_some() {
            "project"
        } else {
            "global"
        };
        let now = Utc::now().to_rfc3339();

        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        if let Some(project_id) = project_id {
            ensure_project(&mut transaction, project_id).await?;
        }
        let Some(bound) = load_bound_profile(&mut transaction, scope, project_id).await? else {
            transaction.commit().await?;
            return Ok(false);
        };
        if bound != profile_id {
            return Err(Error::Conflict(
                "execution profile is not the current default",
            ));
        }
        let current = load_current_content(&mut transaction, profile_id).await?;
        sqlx::query::query(
            "DELETE FROM execution_profile_defaults
             WHERE scope=?1 AND (project_id IS ?2 OR project_id=?2)",
        )
        .bind(scope)
        .bind(project_id)
        .execute(&mut *transaction)
        .await?;
        insert_audit(
            &mut transaction,
            profile_id,
            Some(current.revision),
            if project_id.is_some() {
                "project_default_cleared"
            } else {
                "global_default_cleared"
            },
            actor,
            &format!("project={}", project_id.unwrap_or("global")),
            &now,
        )
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn resolve_project_execution_profile(
        &self,
        project_id: &str,
    ) -> Result<ResolvedExecutionProfile, Error> {
        validate_project_id(project_id)?;
        let mut connection = self.pool.acquire().await?;
        ensure_project(&mut connection, project_id).await?;
        let (profile_id, source) = if let Some(profile_id) =
            load_bound_profile(&mut connection, "project", Some(project_id)).await?
        {
            (profile_id, ExecutionProfileDefaultSource::ProjectDefault)
        } else if let Some(profile_id) = load_bound_profile(&mut connection, "global", None).await?
        {
            (profile_id, ExecutionProfileDefaultSource::GlobalDefault)
        } else {
            return Err(Error::NotFound);
        };
        let profile = load_execution_profile(&mut connection, &profile_id).await?;
        Ok(ResolvedExecutionProfile {
            project_id: project_id.to_owned(),
            profile_id: profile.id,
            revision_id: profile.revision_id,
            revision: profile.revision,
            content_hash: profile.content_hash,
            source,
        })
    }

    pub async fn execution_profile_audit(
        &self,
        profile_id: &str,
    ) -> Result<Vec<ExecutionProfileAudit>, Error> {
        let rows = sqlx::query::query(
            "SELECT audit_id,profile_id,revision,action,actor,detail,created_at
             FROM execution_profile_audit
             WHERE profile_id=?1
             ORDER BY created_at DESC LIMIT 100",
        )
        .bind(profile_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(ExecutionProfileAudit {
                    audit_id: row.try_get("audit_id")?,
                    profile_id: row.try_get("profile_id")?,
                    revision: row
                        .try_get::<Option<i64>, _>("revision")?
                        .map(|revision| {
                            u32::try_from(revision)
                                .map_err(|_| Error::Conflict("stored audit revision is invalid"))
                        })
                        .transpose()?,
                    action: row.try_get("action")?,
                    actor: row.try_get("actor")?,
                    detail: row.try_get("detail")?,
                    created_at: row.try_get("created_at")?,
                })
            })
            .collect()
    }
}

struct CurrentContent {
    revision: u32,
    revision_id: String,
    content: ExecutionProfileContent,
}

async fn load_execution_profile(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<ExecutionProfileRecord, Error> {
    let row = sqlx::query::query(
        "SELECT p.id,p.current_revision,p.content_hash AS profile_hash,
                p.created_at,p.updated_at,r.revision_id,r.content_json,r.content_hash
         FROM execution_profiles p
         JOIN execution_profile_revisions r
           ON r.profile_id=p.id AND r.revision=p.current_revision
         WHERE p.id=?1",
    )
    .bind(id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(Error::NotFound)?;
    let profile = profile_from_row(&row)?;
    let availability = load_provider_availability(connection).await?;
    let defaults = load_defaults(connection).await?;
    let mut project_defaults = defaults
        .iter()
        .filter(|(scope, profile_id, _)| scope == "project" && profile_id == &profile.id)
        .map(|(_, _, project_id)| project_id.clone())
        .collect::<Vec<_>>();
    project_defaults.sort();
    let global_default = defaults
        .iter()
        .any(|(scope, profile_id, _)| scope == "global" && profile_id == &profile.id);
    build_record(profile, &availability, global_default, project_defaults)
}

fn profile_from_row(row: &SqliteRow) -> Result<ProfileRow, Error> {
    let revision = u32::try_from(row.try_get::<i64, _>("current_revision")?)
        .map_err(|_| Error::Conflict("stored execution profile revision is invalid"))?;
    Ok(ProfileRow {
        id: row.try_get("id")?,
        revision,
        revision_id: row.try_get("revision_id")?,
        content_json: row.try_get("content_json")?,
        content_hash: row.try_get("content_hash")?,
        profile_hash: row.try_get("profile_hash")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn build_record(
    row: ProfileRow,
    availability: &[ProviderRouteAvailability],
    global_default: bool,
    project_defaults: Vec<String>,
) -> Result<ExecutionProfileRecord, Error> {
    let content: ExecutionProfileContent = serde_json::from_str(&row.content_json)
        .map_err(|_| Error::Conflict("stored execution profile content is invalid"))?;
    content
        .validate()
        .map_err(|_| Error::Conflict("stored execution profile content is invalid"))?;
    let computed = content.compute_content_hash();
    if computed != row.content_hash || computed != row.profile_hash {
        return Err(Error::Conflict(
            "stored execution profile hash is inconsistent",
        ));
    }
    let (executable, warnings) = evaluate_executability(&content, availability);
    Ok(ExecutionProfileRecord {
        id: row.id,
        revision: row.revision,
        revision_id: row.revision_id,
        content_hash: row.content_hash,
        global_default,
        project_defaults,
        content,
        executable,
        warnings,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

async fn load_current_content(
    connection: &mut SqliteConnection,
    profile_id: &str,
) -> Result<CurrentContent, Error> {
    let row = sqlx::query::query(
        "SELECT r.revision_id,r.revision,r.content_json
         FROM execution_profiles p
         JOIN execution_profile_revisions r
           ON r.profile_id=p.id AND r.revision=p.current_revision
         WHERE p.id=?1",
    )
    .bind(profile_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(Error::NotFound)?;
    let revision = u32::try_from(row.try_get::<i64, _>("revision")?)
        .map_err(|_| Error::Conflict("stored execution profile revision is invalid"))?;
    let content_json: String = row.try_get("content_json")?;
    let content: ExecutionProfileContent = serde_json::from_str(&content_json)
        .map_err(|_| Error::Conflict("stored execution profile content is invalid"))?;
    content
        .validate()
        .map_err(|_| Error::Conflict("stored execution profile content is invalid"))?;
    Ok(CurrentContent {
        revision,
        revision_id: row.try_get("revision_id")?,
        content,
    })
}

async fn load_provider_availability(
    connection: &mut SqliteConnection,
) -> Result<Vec<ProviderRouteAvailability>, Error> {
    let rows = sqlx::query::query(
        "SELECT provider,model,enabled,is_default,credential_ciphertext FROM provider_settings",
    )
    .fetch_all(&mut *connection)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(ProviderRouteAvailability {
                provider: row.try_get("provider")?,
                model: row.try_get("model")?,
                enabled: row.try_get::<i64, _>("enabled")? != 0,
                has_credential: !row
                    .try_get::<String, _>("credential_ciphertext")?
                    .is_empty(),
                is_default: row.try_get::<i64, _>("is_default")? != 0,
            })
        })
        .collect()
}

async fn load_defaults(
    connection: &mut SqliteConnection,
) -> Result<Vec<(String, String, String)>, Error> {
    let rows =
        sqlx::query::query("SELECT scope,profile_id,project_id FROM execution_profile_defaults")
            .fetch_all(&mut *connection)
            .await?;
    rows.iter()
        .map(|row| {
            Ok((
                row.try_get("scope")?,
                row.try_get("profile_id")?,
                row.try_get::<Option<String>, _>("project_id")?
                    .unwrap_or_default(),
            ))
        })
        .collect()
}

async fn load_bound_profile(
    connection: &mut SqliteConnection,
    scope: &str,
    project_id: Option<&str>,
) -> Result<Option<String>, Error> {
    let row = match (scope, project_id) {
        ("global", None) => {
            sqlx::query::query(
                "SELECT profile_id FROM execution_profile_defaults WHERE scope='global'",
            )
            .fetch_optional(&mut *connection)
            .await?
        }
        ("project", Some(project_id)) => {
            sqlx::query::query(
                "SELECT profile_id FROM execution_profile_defaults
                 WHERE scope='project' AND project_id=?1",
            )
            .bind(project_id)
            .fetch_optional(&mut *connection)
            .await?
        }
        _ => return Err(Error::Invalid("execution profile default scope")),
    };
    row.map(|row| row.try_get::<String, _>("profile_id"))
        .transpose()
        .map_err(Error::from)
}

async fn ensure_project(connection: &mut SqliteConnection, project_id: &str) -> Result<(), Error> {
    let project: Option<(String,)> =
        sqlx::query_as::query_as("SELECT id FROM projects WHERE id=?1")
            .bind(project_id)
            .fetch_optional(&mut *connection)
            .await?;
    project.map(|_| ()).ok_or(Error::NotFound)
}

async fn insert_audit(
    connection: &mut SqliteConnection,
    profile_id: &str,
    revision: Option<u32>,
    action: &str,
    actor: &str,
    detail: &str,
    created_at: &str,
) -> Result<(), Error> {
    sqlx::query::query(
        "INSERT INTO execution_profile_audit
         (audit_id,profile_id,revision,action,actor,detail,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(profile_id)
    .bind(revision.map(i64::from))
    .bind(action)
    .bind(actor)
    .bind(detail)
    .bind(created_at)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

fn validate_actor(actor: &str) -> Result<(), Error> {
    if actor.trim().is_empty() || actor.len() > 128 || actor.trim() != actor {
        return Err(Error::Invalid("execution profile actor"));
    }
    Ok(())
}

fn validate_project_id(project_id: &str) -> Result<(), Error> {
    if project_id.is_empty()
        || project_id.len() > 128
        || project_id.chars().any(char::is_whitespace)
    {
        return Err(Error::Invalid("execution profile project id"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ProviderSettingsWrite;
    use crate::execution_profile::{
        AutoStartPolicy, CampaignBudgets, ExecutionProfileCategory, ExecutionProfileRoles,
        ExecutionRoleRoute, ReasoningEffort,
    };
    use crate::lead::RiskClass;
    use std::path::PathBuf;

    fn temp_path(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{prefix}-{}.sqlite", Uuid::new_v4()))
    }

    async fn temp_db(prefix: &str) -> (PathBuf, Database) {
        let path = temp_path(prefix);
        let db = Database::open(&path).await.unwrap();
        (path, db)
    }

    async fn ensure_project_row(db: &Database, id: &str) {
        sqlx::query::query(
            "INSERT INTO projects(id,repository,base_commit,created_at,identity)
             VALUES (?1,?2,'base',?3,'')",
        )
        .bind(id)
        .bind(format!("/fixture/{id}"))
        .bind(Utc::now().to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();
    }

    async fn configure_provider(db: &Database, provider: &str, kind: &str, model: &str) {
        db.save_provider_settings(
            &ProviderSettingsWrite {
                provider: provider.into(),
                kind: kind.into(),
                model: model.into(),
                base_url: "https://provider.example/v1".into(),
                auth_mode: "api_key".into(),
                credential_ciphertext: "ciphertext".into(),
                credential_nonce: "nonce".into(),
                credential_fingerprint: "fingerprint".into(),
                enabled: true,
                is_default: provider == "opencode-go",
                max_turns: 4,
                max_input_tokens: 100_000,
                max_output_tokens: 12_000,
                max_cost_micros: 500_000,
                input_micros_per_million: 1000,
                output_micros_per_million: 2000,
            },
            "test-operator",
        )
        .await
        .unwrap();
    }

    fn route(provider: &str, model: &str) -> ExecutionRoleRoute {
        ExecutionRoleRoute {
            provider: provider.into(),
            model: model.into(),
            reasoning: Some(ReasoningEffort::Medium),
            max_input_tokens: 64_000,
            max_output_tokens: 8_192,
            max_cost_micros: 300_000,
        }
    }

    fn content(name: &str) -> ExecutionProfileContent {
        ExecutionProfileContent {
            name: name.into(),
            description: "test profile".into(),
            category: ExecutionProfileCategory::Balanced,
            roles: ExecutionProfileRoles {
                lead: route("opencode-go", "deepseek-v4.1-flash"),
                implementer: route("opencode-go", "deepseek-v4.1-flash"),
                semantic_reviewer: route("opencode-go", "deepseek-v4.1-flash"),
            },
            budgets: CampaignBudgets {
                max_attempts: 3,
                max_sessions: 2,
                max_turns: 4,
                max_input_tokens: 40_000,
                max_output_tokens: 12_000,
                max_cost_micros: 500_000,
                deadline_secs: 600,
            },
            auto_start: AutoStartPolicy {
                enabled: false,
                risk_classes: Vec::new(),
            },
        }
    }

    async fn count(db: &Database, sql: &str) -> i64 {
        let (value,): (i64,) = sqlx::query_as::query_as(sql)
            .fetch_one(&db.pool)
            .await
            .unwrap();
        value
    }

    #[tokio::test]
    async fn create_and_update_keep_immutable_revisions() {
        let (path, db) = temp_db("factory-execution-profiles-revisions").await;
        configure_provider(&db, "opencode-go", "opencode", "deepseek-v4.1-flash").await;
        configure_provider(&db, "codex", "codex", "gpt-6-luna").await;

        let created = db
            .create_execution_profile(&content("Balanced"), "owner")
            .await
            .unwrap();
        assert_eq!(created.revision, 1);
        assert!(created.executable, "{:?}", created.warnings);
        assert!(created.warnings.is_empty());
        assert_eq!(created.content_hash.len(), 64);
        assert!(!created.global_default);

        let updated = db
            .update_execution_profile(&created.id, 1, &content("Balanced v2"), "owner")
            .await
            .unwrap();
        assert_eq!(updated.revision, 2);
        assert_ne!(created.content_hash, updated.content_hash);
        assert_eq!(
            count(
                &db,
                &format!(
                    "SELECT COUNT(*) FROM execution_profile_revisions WHERE profile_id='{}'",
                    created.id
                )
            )
            .await,
            2
        );
        let (first_content, first_hash): (String, String) = sqlx::query_as::query_as(
            "SELECT content_json,content_hash FROM execution_profile_revisions
             WHERE profile_id=?1 AND revision=1",
        )
        .bind(&created.id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert!(first_content.contains("Balanced"));
        assert!(!first_content.contains("Balanced v2"));
        assert_eq!(first_hash, created.content_hash);

        let stale = db
            .update_execution_profile(&created.id, 1, &content("stale"), "owner")
            .await
            .unwrap_err();
        assert!(matches!(
            stale,
            Error::Conflict("execution profile revision is stale")
        ));

        let direct_update = sqlx::query::query(
            "UPDATE execution_profile_revisions SET content_json='{}' WHERE profile_id=?1",
        )
        .bind(&created.id)
        .execute(&db.pool)
        .await;
        assert!(direct_update.is_err());

        let audit = db.execution_profile_audit(&created.id).await.unwrap();
        assert!(audit.iter().any(|entry| entry.action == "created"));
        assert!(audit.iter().any(|entry| entry.action == "updated"));
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn defaults_are_single_and_project_resolution_prefers_project() {
        let (path, db) = temp_db("factory-execution-profiles-defaults").await;
        configure_provider(&db, "opencode-go", "opencode", "deepseek-v4.1-flash").await;
        configure_provider(&db, "codex", "codex", "gpt-6-luna").await;
        ensure_project_row(&db, "project-a").await;
        ensure_project_row(&db, "project-b").await;

        let first = db
            .create_execution_profile(&content("First"), "owner")
            .await
            .unwrap();
        let second = db
            .create_execution_profile(&content("Second"), "owner")
            .await
            .unwrap();

        db.set_execution_profile_default(&first.id, None, "owner")
            .await
            .unwrap();
        let resolved = db
            .resolve_project_execution_profile("project-a")
            .await
            .unwrap();
        assert_eq!(resolved.profile_id, first.id);
        assert_eq!(
            resolved.source,
            ExecutionProfileDefaultSource::GlobalDefault
        );

        db.set_execution_profile_default(&second.id, None, "owner")
            .await
            .unwrap();
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM execution_profile_defaults WHERE scope='global'"
            )
            .await,
            1
        );
        let first_reloaded = db.execution_profile(&first.id).await.unwrap();
        let second_reloaded = db.execution_profile(&second.id).await.unwrap();
        assert!(!first_reloaded.global_default);
        assert!(second_reloaded.global_default);

        db.set_execution_profile_default(&second.id, Some("project-a"), "owner")
            .await
            .unwrap();
        let project_default = db.execution_profile(&second.id).await.unwrap();
        assert_eq!(
            project_default.project_defaults,
            vec!["project-a".to_owned()]
        );
        let resolved = db
            .resolve_project_execution_profile("project-a")
            .await
            .unwrap();
        assert_eq!(
            resolved.source,
            ExecutionProfileDefaultSource::ProjectDefault
        );
        let other = db
            .resolve_project_execution_profile("project-b")
            .await
            .unwrap();
        assert_eq!(other.source, ExecutionProfileDefaultSource::GlobalDefault);

        let wrong = db
            .clear_execution_profile_default(&first.id, Some("project-a"), "owner")
            .await
            .unwrap_err();
        assert!(matches!(wrong, Error::Conflict(_)));

        assert!(
            db.clear_execution_profile_default(&second.id, Some("project-a"), "owner")
                .await
                .unwrap()
        );
        let resolved = db
            .resolve_project_execution_profile("project-a")
            .await
            .unwrap();
        assert_eq!(
            resolved.source,
            ExecutionProfileDefaultSource::GlobalDefault
        );

        assert!(
            db.clear_execution_profile_default(&second.id, None, "owner")
                .await
                .unwrap()
        );
        assert!(matches!(
            db.resolve_project_execution_profile("project-a").await,
            Err(Error::NotFound)
        ));

        let audit = db.execution_profile_audit(&second.id).await.unwrap();
        assert!(
            audit
                .iter()
                .any(|entry| entry.action == "global_default_set")
        );
        assert!(
            audit
                .iter()
                .any(|entry| entry.action == "project_default_set")
        );
        assert!(
            audit
                .iter()
                .any(|entry| entry.action == "project_default_cleared")
        );
        assert!(
            audit
                .iter()
                .any(|entry| entry.action == "global_default_cleared")
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn unconfigured_provider_is_a_marked_draft_and_cannot_be_default() {
        let (path, db) = temp_db("factory-execution-profiles-draft").await;
        let draft = db
            .create_execution_profile(&content("Draft"), "owner")
            .await
            .unwrap();
        assert!(!draft.executable);
        assert_eq!(draft.warnings.len(), 4);
        assert!(
            draft
                .warnings
                .iter()
                .any(|warning| warning.contains("not configured"))
        );
        assert!(
            draft
                .warnings
                .iter()
                .any(|warning| warning.contains("no active Core default model gateway"))
        );

        let error = db
            .set_execution_profile_default(&draft.id, None, "owner")
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            Error::Conflict("execution profile is not executable")
        ));
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn cross_provider_profile_is_non_executable_without_role_routing() {
        let (path, db) = temp_db("factory-execution-profiles-routing").await;
        configure_provider(&db, "opencode-go", "opencode", "deepseek-v4.1-flash").await;
        configure_provider(&db, "codex", "codex", "gpt-6-luna").await;

        let mut cross = content("Cross provider");
        cross.roles.semantic_reviewer = route("codex", "gpt-6-luna");
        let created = db.create_execution_profile(&cross, "owner").await.unwrap();
        assert!(!created.executable);
        assert!(created.warnings.iter().any(|warning| warning.contains(
            "semanticReviewer route codex/gpt-6-luna differs from the active Core default gateway opencode-go/deepseek-v4.1-flash"
        )));
        assert!(
            created
                .warnings
                .iter()
                .any(|warning| warning.contains("role-specific routing is not applied"))
        );

        let error = db
            .set_execution_profile_default(&created.id, None, "owner")
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            Error::Conflict("execution profile is not executable")
        ));
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn unsupported_route_is_rejected_before_persistence() {
        let (path, db) = temp_db("factory-execution-profiles-route").await;
        let mut invalid = content("Invalid");
        invalid.roles.lead = route("other-provider", "deepseek-v4.1-flash");
        let error = db
            .create_execution_profile(&invalid, "owner")
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Validation(message) if message.contains("unsupported")));
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM execution_profiles").await,
            0
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn auto_start_risk_classes_are_part_of_the_validated_content() {
        let (path, db) = temp_db("factory-execution-profiles-autostart").await;
        configure_provider(&db, "opencode-go", "opencode", "deepseek-v4.1-flash").await;
        configure_provider(&db, "codex", "codex", "gpt-6-luna").await;
        let mut profile = content("Autonomous");
        profile.auto_start = AutoStartPolicy {
            enabled: true,
            risk_classes: vec![RiskClass::R0],
        };
        let created = db
            .create_execution_profile(&profile, "owner")
            .await
            .unwrap();
        assert_eq!(created.content.auto_start.risk_classes, vec![RiskClass::R0]);
        let _ = std::fs::remove_file(path);
    }
}
