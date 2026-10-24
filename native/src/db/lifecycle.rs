use super::*;

use chrono::Utc;
use sqlx_sqlite::SqliteConnection;
use uuid::Uuid;

impl Database {
    pub async fn create_campaign(&self, action_id: &str, key: &str) -> Result<Campaign, Error> {
        let digest = sha256_hex(
            serde_json::to_string(&(action_id, "campaign-v1"))
                .expect("campaign digest serializes")
                .as_bytes(),
        );
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        if let Some(id) = replay(&mut transaction, key, "create_campaign", &digest).await? {
            let value = load_campaign(&mut transaction, &id).await?;
            transaction.commit().await?;
            return Ok(value);
        }
        let action: Option<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM actions WHERE id=?1")
                .bind(action_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if action.is_none() {
            return Err(Error::NotFound);
        }
        let value = Campaign {
            id: Uuid::new_v4().to_string(),
            action_id: action_id.into(),
            status: "BLOCKED_AUTHORIZATION".into(),
            created_at: Utc::now().to_rfc3339(),
        };
        sqlx::query::query("INSERT INTO campaigns VALUES (?1,?2,?3,?4)")
            .bind(&value.id)
            .bind(&value.action_id)
            .bind(&value.status)
            .bind(&value.created_at)
            .execute(&mut *transaction)
            .await?;
        sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
            .bind(key)
            .bind("create_campaign")
            .bind(&digest)
            .bind(&value.id)
            .execute(&mut *transaction)
            .await?;
        record_event(
            &mut transaction,
            &value.id,
            "campaign.created",
            "blocked pending explicit authorization",
        )
        .await?;
        transaction.commit().await?;
        Ok(value)
    }

    pub async fn campaign(&self, id: &str) -> Result<Campaign, Error> {
        let mut connection = self.pool.acquire().await?;
        load_campaign(&mut connection, id).await
    }

    pub async fn create_run(&self, campaign_id: &str, key: &str) -> Result<Run, Error> {
        let digest = sha256_hex(
            serde_json::to_string(&(campaign_id, "run-v1"))
                .expect("run digest serializes")
                .as_bytes(),
        );
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        if let Some(id) = replay(&mut transaction, key, "create_run", &digest).await? {
            let value = load_run(&mut transaction, &id).await?;
            transaction.commit().await?;
            return Ok(value);
        }
        let status: Option<(String,)> =
            sqlx::query_as::query_as("SELECT status FROM campaigns WHERE id=?1")
                .bind(campaign_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if status.as_ref().map(|(status,)| status.as_str()) != Some("AUTHORIZED") {
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
        sqlx::query::query("INSERT INTO runs VALUES (?1,?2,?3,?4)")
            .bind(&value.id)
            .bind(&value.campaign_id)
            .bind(&value.status)
            .bind(&value.created_at)
            .execute(&mut *transaction)
            .await?;
        sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
            .bind(key)
            .bind("create_run")
            .bind(&digest)
            .bind(&value.id)
            .execute(&mut *transaction)
            .await?;
        record_event(&mut transaction, &value.id, "run.created", "queued").await?;
        transaction.commit().await?;
        Ok(value)
    }

    pub async fn create_attempt(&self, run_id: &str, key: &str) -> Result<Attempt, Error> {
        let digest = sha256_hex(
            serde_json::to_string(&(run_id, "attempt-v1"))
                .expect("attempt digest serializes")
                .as_bytes(),
        );
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        if let Some(id) = replay(&mut transaction, key, "create_attempt", &digest).await? {
            let value = load_attempt(&mut transaction, &id).await?;
            transaction.commit().await?;
            return Ok(value);
        }
        let queued: Option<(String,)> =
            sqlx::query_as::query_as("SELECT status FROM runs WHERE id=?1")
                .bind(run_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if queued.as_ref().map(|(status,)| status.as_str()) != Some("QUEUED") {
            return Err(Error::Invalid("run is not queued"));
        }
        let value = Attempt {
            id: Uuid::new_v4().to_string(),
            run_id: run_id.into(),
            status: "QUEUED".into(),
            created_at: Utc::now().to_rfc3339(),
        };
        sqlx::query::query(
            "INSERT INTO attempts(id,run_id,status,created_at) VALUES (?1,?2,?3,?4)",
        )
        .bind(&value.id)
        .bind(&value.run_id)
        .bind(&value.status)
        .bind(&value.created_at)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query(
            "INSERT INTO attempt_specs(attempt_id,action_id,base_commit,spec_json,spec_sha256)
             SELECT ?1,c.action_id,p.base_commit,s.spec_json,s.spec_sha256
             FROM runs r JOIN campaigns c ON c.id=r.campaign_id
             JOIN actions ac ON ac.id=c.action_id
             JOIN projects p ON p.id=ac.project_id
             JOIN action_specs s ON s.action_id=ac.id
             WHERE r.id=?2",
        )
        .bind(&value.id)
        .bind(&value.run_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
            .bind(key)
            .bind("create_attempt")
            .bind(&digest)
            .bind(&value.id)
            .execute(&mut *transaction)
            .await?;
        record_event(
            &mut transaction,
            &value.id,
            "attempt.created",
            "queued; no worker assignment",
        )
        .await?;
        transaction.commit().await?;
        Ok(value)
    }

    pub async fn campaigns(&self) -> Result<Vec<Campaign>, Error> {
        let rows: Vec<(String, String, String, String)> = sqlx::query_as::query_as(
            "SELECT id,action_id,status,created_at FROM campaigns ORDER BY created_at,id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, action_id, status, created_at)| Campaign {
                id,
                action_id,
                status,
                created_at,
            })
            .collect())
    }

    pub async fn runs(&self) -> Result<Vec<Run>, Error> {
        let rows: Vec<(String, String, String, String)> = sqlx::query_as::query_as(
            "SELECT id,campaign_id,status,created_at FROM runs ORDER BY created_at,id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, campaign_id, status, created_at)| Run {
                id,
                campaign_id,
                status,
                created_at,
            })
            .collect())
    }

    pub async fn attempts(&self) -> Result<Vec<Attempt>, Error> {
        let rows: Vec<(String, String, String, String)> = sqlx::query_as::query_as(
            "SELECT id,run_id,status,created_at FROM attempts ORDER BY created_at,id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, run_id, status, created_at)| Attempt {
                id,
                run_id,
                status,
                created_at,
            })
            .collect())
    }
}

pub(super) async fn load_campaign(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<Campaign, Error> {
    let row: Option<(String, String, String, String)> = sqlx::query_as::query_as(
        "SELECT id,action_id,status,created_at FROM campaigns WHERE id=?1",
    )
    .bind(id)
    .fetch_optional(connection)
    .await?;
    let (id, action_id, status, created_at) = row.ok_or(Error::NotFound)?;
    Ok(Campaign {
        id,
        action_id,
        status,
        created_at,
    })
}

pub(super) async fn load_run(connection: &mut SqliteConnection, id: &str) -> Result<Run, Error> {
    let row: Option<(String, String, String, String)> =
        sqlx::query_as::query_as("SELECT id,campaign_id,status,created_at FROM runs WHERE id=?1")
            .bind(id)
            .fetch_optional(connection)
            .await?;
    let (id, campaign_id, status, created_at) = row.ok_or(Error::NotFound)?;
    Ok(Run {
        id,
        campaign_id,
        status,
        created_at,
    })
}

pub(super) async fn load_attempt(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<Attempt, Error> {
    let row: Option<(String, String, String, String)> =
        sqlx::query_as::query_as("SELECT id,run_id,status,created_at FROM attempts WHERE id=?1")
            .bind(id)
            .fetch_optional(connection)
            .await?;
    let (id, run_id, status, created_at) = row.ok_or(Error::NotFound)?;
    Ok(Attempt {
        id,
        run_id,
        status,
        created_at,
    })
}
