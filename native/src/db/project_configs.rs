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

    /// Advance the Project base to the exact candidate under repair. The
    /// candidate is already a Factory-authored commit; the new config revision
    /// keeps the same scope and quality policy and records a new hash.
    pub async fn advance_project_base_for_repair(
        &self,
        project_id: &str,
        candidate_commit: &str,
        expected_config_revision: u32,
    ) -> Result<ProjectConfig, Error> {
        if candidate_commit.len() != 40
            || !candidate_commit
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(Error::Invalid("repair candidate commit"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let current = load_project_config(&mut transaction, project_id).await?;
        if current.revision != expected_config_revision {
            return Err(Error::Conflict("project config revision is stale"));
        }
        let (repository,): (String,) =
            sqlx::query_as::query_as("SELECT repository FROM projects WHERE id=?1")
                .bind(project_id)
                .fetch_one(&mut *transaction)
                .await?;
        if !crate::workspace::is_ancestor(
            std::path::Path::new(&repository),
            &current.base_commit,
            candidate_commit,
        )
        .map_err(|error| Error::Validation(error.to_string()))?
        {
            return Err(Error::Conflict(
                "repair candidate is not on the current Project base lineage",
            ));
        }
        sqlx::query::query("UPDATE projects SET base_commit=?1 WHERE id=?2")
            .bind(candidate_commit)
            .bind(project_id)
            .execute(&mut *transaction)
            .await?;
        let mut next = current;
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or(Error::Constraint("project config revision overflow".into()))?;
        next.base_commit = candidate_commit.into();
        next.updated_at = Utc::now().to_rfc3339();
        next.config_hash = next.compute_hash();
        write_project_config(&mut transaction, &next).await?;
        record_event(
            &mut transaction,
            project_id,
            "project.config_updated",
            &format!(
                "repair_base_candidate={} revision={}",
                candidate_commit, next.revision
            ),
        )
        .await?;
        transaction.commit().await?;
        Ok(next)
    }

    /// Advance the Project base to the exact merge commit that a Factory
    /// delivery recorded on the protected default branch. The write is a new
    /// versioned project-config revision with a fencing revision/base guard and
    /// an audit event; it never rewrites a historical revision or accepts a
    /// non-default branch. A duplicate call for the same merge is idempotent
    /// and leaves the revision unchanged.
    ///
    /// The caller must have proved Git ancestry first: this method cannot prove
    /// that `merge_commit_sha` descends from `expected_base_commit`. It does
    /// require the exact expected base, so a concurrent advancement cannot be
    /// overwritten with an older merge.
    pub async fn advance_project_base_after_merged_delivery(
        &self,
        project_id: &str,
        merge_commit_sha: &str,
        default_branch: &str,
        expected_config_revision: u32,
        expected_base_commit: &str,
    ) -> Result<ProjectConfig, Error> {
        if merge_commit_sha.len() != 40
            || !merge_commit_sha
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || expected_base_commit.len() != 40
            || !expected_base_commit
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || default_branch.trim().is_empty()
            || default_branch.len() > 255
        {
            return Err(Error::Invalid("merged delivery base"));
        }
        let merge_commit_sha = merge_commit_sha.to_ascii_lowercase();
        let expected_base_commit = expected_base_commit.to_ascii_lowercase();
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = match load_project_config(&mut transaction, project_id).await {
            Ok(config) => config,
            Err(Error::NotFound) => {
                self.ensure_project_config(&mut transaction, project_id)
                    .await?
            }
            Err(error) => return Err(error),
        };
        if current.base_commit.eq_ignore_ascii_case(&merge_commit_sha) {
            transaction.commit().await?;
            return Ok(current);
        }
        if !current
            .base_commit
            .eq_ignore_ascii_case(&expected_base_commit)
        {
            return Err(Error::Conflict("project base is stale"));
        }
        if current.revision != expected_config_revision {
            return Err(Error::Conflict("project config revision is stale"));
        }
        let project: Option<(String, String)> =
            sqlx::query_as::query_as("SELECT default_branch,repository FROM projects WHERE id=?1")
                .bind(project_id)
                .fetch_optional(&mut *transaction)
                .await?;
        let Some((project_default_branch, repository)) = project else {
            return Err(Error::NotFound);
        };
        let effective_branch = if project_default_branch.trim().is_empty() {
            let binding: Option<(String,)> = sqlx::query_as::query_as(
                "SELECT default_branch FROM project_github_bindings WHERE project_id=?1",
            )
            .bind(project_id)
            .fetch_optional(&mut *transaction)
            .await?;
            binding.map(|(branch,)| branch).unwrap_or_default()
        } else {
            project_default_branch
        };
        if effective_branch != default_branch {
            return Err(Error::Conflict(
                "merged delivery did not target the Project default branch",
            ));
        }
        let changed =
            sqlx::query::query("UPDATE projects SET base_commit=?1 WHERE id=?2 AND base_commit=?3")
                .bind(&merge_commit_sha)
                .bind(project_id)
                .bind(&expected_base_commit)
                .execute(&mut *transaction)
                .await?
                .rows_affected();
        if changed != 1 {
            return Err(Error::Conflict("project base changed during advancement"));
        }
        let mut next = current;
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or(Error::Constraint("project config revision overflow".into()))?;
        next.repository = repository;
        next.base_commit = merge_commit_sha.clone();
        next.updated_at = Utc::now().to_rfc3339();
        next.config_hash = next.compute_hash();
        write_project_config(&mut transaction, &next).await?;
        record_event(
            &mut transaction,
            project_id,
            "project.base_advanced",
            &format!(
                "merge_commit={} branch={} revision={}",
                merge_commit_sha, default_branch, next.revision
            ),
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

    async fn fixture() -> (std::path::PathBuf, Database, String) {
        let root =
            std::env::temp_dir().join(format!("factory-project-config-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "fixture@example.invalid"],
            vec!["config", "user.name", "Fixture"],
            vec!["commit", "--allow-empty", "-qm", "base"],
            vec!["commit", "--allow-empty", "-qm", "candidate"],
        ] {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap();
            assert!(output.status.success());
        }
        let base = String::from_utf8(
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD~1"])
                .current_dir(&repo)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned();
        let candidate = String::from_utf8(
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&repo)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned();
        let path = root.join("factory.sqlite");
        let db = Database::open(&path).await.unwrap();
        sqlx::query::query(
            "INSERT INTO projects(id,repository,base_commit,created_at,identity,default_branch)
             VALUES (?1,?2,?3,?4,'','main')",
        )
        .bind("project")
        .bind(repo.to_string_lossy().as_ref())
        .bind(&base)
        .bind(Utc::now().to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();
        let mut connection = db.pool.acquire().await.unwrap();
        db.ensure_project_config(&mut connection, "project")
            .await
            .unwrap();
        drop(connection);
        (path, db, candidate)
    }

    #[tokio::test]
    async fn repair_base_advances_the_project_and_config_with_stale_fencing() {
        let (path, db, candidate) = fixture().await;
        let advanced = db
            .advance_project_base_for_repair("project", &candidate, 1)
            .await
            .unwrap();
        assert_eq!(advanced.revision, 2);
        assert_eq!(advanced.base_commit, candidate);
        assert_eq!(advanced.config_hash, advanced.compute_hash());
        let stored: (String,) =
            sqlx::query_as::query_as("SELECT base_commit FROM projects WHERE id='project'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(stored.0, candidate);
        assert!(matches!(
            db.advance_project_base_for_repair("project", &"d".repeat(40), 1)
                .await,
            Err(Error::Conflict(_))
        ));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn merged_delivery_advances_the_base_only_for_the_default_branch() {
        let (path, db, _candidate) = fixture().await;
        let merge = "e".repeat(40);
        let initial = db.project_config("project").await.unwrap();
        let advanced = db
            .advance_project_base_after_merged_delivery(
                "project",
                &merge,
                "main",
                initial.revision,
                &initial.base_commit,
            )
            .await
            .unwrap();
        assert_eq!(advanced.revision, 2);
        assert_eq!(advanced.base_commit, merge);
        assert_eq!(advanced.config_hash, advanced.compute_hash());
        let stored: (String,) =
            sqlx::query_as::query_as("SELECT base_commit FROM projects WHERE id='project'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(stored.0, merge);

        // Replaying the same exact merge is idempotent and does not append a
        // second config revision.
        let replay = db
            .advance_project_base_after_merged_delivery("project", &merge, "main", 999, &merge)
            .await
            .unwrap();
        assert_eq!(replay.revision, 2);

        // A non-default branch never advances the Project base.
        assert!(matches!(
            db.advance_project_base_after_merged_delivery(
                "project",
                &"f".repeat(40),
                "release",
                advanced.revision,
                &advanced.base_commit,
            )
            .await,
            Err(Error::Conflict(_))
        ));
        // A stale revision fence is refused instead of overwriting a newer
        // Project config revision.
        assert!(matches!(
            db.advance_project_base_after_merged_delivery(
                "project",
                &"f".repeat(40),
                "main",
                initial.revision,
                &advanced.base_commit,
            )
            .await,
            Err(Error::Conflict(_))
        ));
        // A stale base expectation is refused even when the revision happens
        // to match; the conditional update is the final fence.
        assert!(matches!(
            db.advance_project_base_after_merged_delivery(
                "project",
                &"f".repeat(40),
                "main",
                advanced.revision,
                &initial.base_commit,
            )
            .await,
            Err(Error::Conflict(_))
        ));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn default_config_is_hashed_and_updates_require_the_current_revision() {
        let (path, db, _candidate) = fixture().await;
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
