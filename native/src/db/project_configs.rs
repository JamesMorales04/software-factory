use super::*;

use crate::lead::{Capability, ProjectConfig, QualityProfile, validate_repository_paths};
use chrono::Utc;
use sqlx_sqlite::SqliteConnection;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NewProjectConfig {
    pub expected_revision: u32,
    pub allowed_paths: Vec<String>,
    pub capabilities: Vec<Capability>,
    pub quality_profile: QualityProfile,
    pub active: bool,
}

impl Database {
    pub async fn project_config(&self, project_id: &str) -> Result<ProjectConfig, Error> {
        let mut connection = self.pool.acquire().await?;
        load_project_config(&mut connection, project_id).await
    }

    pub async fn update_project_config(
        &self,
        project_id: &str,
        input: &NewProjectConfig,
    ) -> Result<ProjectConfig, Error> {
        if input.expected_revision == 0 {
            return Err(Error::Invalid("project config revision"));
        }
        validate_repository_paths("allowedPaths", &input.allowed_paths)?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let current = load_project_config(&mut transaction, project_id).await?;
        if current.revision != input.expected_revision {
            return Err(Error::Conflict("project config revision is stale"));
        }
        let repository: (String,) =
            sqlx::query_as::query_as("SELECT repository FROM projects WHERE id=?1")
                .bind(project_id)
                .fetch_optional(&mut *transaction)
                .await?
                .ok_or(Error::NotFound)?;
        let repository = repository.0;
        let base_commit: (String,) =
            sqlx::query_as::query_as("SELECT base_commit FROM projects WHERE id=?1")
                .bind(project_id)
                .fetch_one(&mut *transaction)
                .await?;
        let base_commit = base_commit.0;
        let mut next = current;
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or(Error::Constraint("project config revision overflow".into()))?;
        next.repository = repository;
        next.base_commit = base_commit;
        next.allowed_paths = input.allowed_paths.clone();
        next.capabilities = input.capabilities.clone();
        next.quality_profile = input.quality_profile;
        next.active = input.active;
        next.updated_at = Utc::now().to_rfc3339();
        next.config_hash = next.compute_hash();
        write_project_config(&mut transaction, &next).await?;
        record_event(
            &mut transaction,
            project_id,
            "project.config_updated",
            &format!("revision={} hash={}", next.revision, next.config_hash),
        )
        .await?;
        transaction.commit().await?;
        Ok(next)
    }

    pub(super) async fn ensure_project_config(
        &self,
        transaction: &mut SqliteConnection,
        project_id: &str,
    ) -> Result<ProjectConfig, Error> {
        match load_project_config(transaction, project_id).await {
            Ok(config) => Ok(config),
            Err(Error::NotFound) => {
                let repository: (String,) =
                    sqlx::query_as::query_as("SELECT repository FROM projects WHERE id=?1")
                        .bind(project_id)
                        .fetch_optional(&mut *transaction)
                        .await?
                        .ok_or(Error::NotFound)?;
                let repository = repository.0;
                let base_commit: (String,) =
                    sqlx::query_as::query_as("SELECT base_commit FROM projects WHERE id=?1")
                        .bind(project_id)
                        .fetch_one(&mut *transaction)
                        .await?;
                let base_commit = base_commit.0;
                let config = ProjectConfig {
                    project_id: project_id.into(),
                    revision: 1,
                    config_hash: String::new(),
                    repository,
                    base_commit,
                    allowed_paths: Vec::new(),
                    capabilities: Vec::new(),
                    quality_profile: QualityProfile::Generic,
                    active: true,
                    updated_at: Utc::now().to_rfc3339(),
                };
                let config = ProjectConfig {
                    config_hash: config.compute_hash(),
                    ..config
                };
                write_project_config(transaction, &config).await?;
                Ok(config)
            }
            Err(error) => Err(error),
        }
    }
}

pub(super) async fn load_project_config(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> Result<ProjectConfig, Error> {
    use sqlx::row::Row;
    let row = sqlx::query::query(
        "SELECT project_id,revision,config_hash,repository,base_commit,allowed_paths_json,
                capabilities_json,quality_profile,active,updated_at
         FROM project_configs WHERE project_id=?1",
    )
    .bind(project_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Err(Error::NotFound);
    };
    let paths: String = row.try_get("allowed_paths_json")?;
    let capabilities: String = row.try_get("capabilities_json")?;
    let active: i64 = row.try_get("active")?;
    let quality: String = row.try_get("quality_profile")?;
    let config = ProjectConfig {
        project_id: row.try_get("project_id")?,
        revision: u32::try_from(row.try_get::<i64, _>("revision")?)
            .map_err(|_| Error::Invalid("stored project config revision"))?,
        config_hash: row.try_get("config_hash")?,
        repository: row.try_get("repository")?,
        base_commit: row.try_get("base_commit")?,
        allowed_paths: serde_json::from_str(&paths)
            .map_err(|_| Error::Invalid("stored project allowed paths"))?,
        capabilities: serde_json::from_str(&capabilities)
            .map_err(|_| Error::Invalid("stored project capabilities"))?,
        quality_profile: serde_json::from_value(serde_json::Value::String(quality))
            .map_err(|_| Error::Invalid("stored project quality profile"))?,
        active: active == 1,
        updated_at: row.try_get("updated_at")?,
    };
    if config.config_hash != config.compute_hash() {
        return Err(Error::Conflict(
            "stored project config hash is inconsistent",
        ));
    }
    Ok(config)
}

async fn write_project_config(
    transaction: &mut SqliteConnection,
    config: &ProjectConfig,
) -> Result<(), Error> {
    let paths = serde_json::to_string(&config.allowed_paths)
        .map_err(|_| Error::Invalid("project allowed paths encoding"))?;
    let capabilities = serde_json::to_string(&config.capabilities)
        .map_err(|_| Error::Invalid("project capabilities encoding"))?;
    let quality = serde_json::to_value(config.quality_profile)
        .map_err(|_| Error::Invalid("project quality profile encoding"))?;
    let quality = quality
        .as_str()
        .ok_or(Error::Invalid("project quality profile encoding"))?;
    sqlx::query::query(
        "INSERT INTO project_configs
         (project_id,revision,config_hash,repository,base_commit,allowed_paths_json,
          capabilities_json,quality_profile,active,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
         ON CONFLICT(project_id) DO UPDATE SET
           revision=excluded.revision,config_hash=excluded.config_hash,
           repository=excluded.repository,base_commit=excluded.base_commit,
           allowed_paths_json=excluded.allowed_paths_json,
           capabilities_json=excluded.capabilities_json,
           quality_profile=excluded.quality_profile,active=excluded.active,
           updated_at=excluded.updated_at",
    )
    .bind(&config.project_id)
    .bind(i64::from(config.revision))
    .bind(&config.config_hash)
    .bind(&config.repository)
    .bind(&config.base_commit)
    .bind(paths)
    .bind(capabilities)
    .bind(quality)
    .bind(i64::from(config.active))
    .bind(&config.updated_at)
    .execute(&mut *transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lead::Capability;

    async fn fixture() -> (std::path::PathBuf, Database) {
        let path = std::env::temp_dir().join(format!(
            "factory-project-config-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let db = Database::open(&path).await.unwrap();
        sqlx::query::query("INSERT INTO projects(id,repository,base_commit,created_at,identity) VALUES (?1,?2,?3,?4,'')")
            .bind("project")
            .bind("/fixture")
            .bind("a".repeat(40))
            .bind(Utc::now().to_rfc3339())
            .execute(&db.pool)
            .await
            .unwrap();
        let mut connection = db.pool.acquire().await.unwrap();
        db.ensure_project_config(&mut connection, "project")
            .await
            .unwrap();
        drop(connection);
        (path, db)
    }

    #[tokio::test]
    async fn default_config_is_hashed_and_updates_require_the_current_revision() {
        let (path, db) = fixture().await;
        let initial = db.project_config("project").await.unwrap();
        assert_eq!(initial.revision, 1);
        assert_eq!(initial.config_hash, initial.compute_hash());
        assert!(initial.active);
        let update = NewProjectConfig {
            expected_revision: 1,
            allowed_paths: vec!["src".into()],
            capabilities: vec![Capability::RepositoryEdit],
            quality_profile: QualityProfile::NodeBasic,
            active: true,
        };
        let next = db.update_project_config("project", &update).await.unwrap();
        assert_eq!(next.revision, 2);
        assert_ne!(next.config_hash, initial.config_hash);
        assert_eq!(next.config_hash, next.compute_hash());
        assert!(matches!(
            db.update_project_config("project", &update).await,
            Err(Error::Conflict(_))
        ));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }
}
