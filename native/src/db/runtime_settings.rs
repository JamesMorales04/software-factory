use super::Error;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::row::Row;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactorySettings {
    pub semantic_review_enabled: bool,
    pub semantic_review_max_output_tokens: u32,
    pub updated_at: String,
}

impl super::Database {
    pub async fn factory_settings(&self) -> Result<FactorySettings, Error> {
        let row = sqlx::query::query(
            "SELECT semantic_review_enabled,semantic_review_max_output_tokens,updated_at
             FROM factory_settings WHERE id=1",
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        Ok(FactorySettings {
            semantic_review_enabled: row.try_get::<i64, _>("semantic_review_enabled")? != 0,
            semantic_review_max_output_tokens: row
                .try_get::<i64, _>("semantic_review_max_output_tokens")?
                as u32,
            updated_at: row.try_get("updated_at")?,
        })
    }

    pub async fn update_factory_settings(
        &self,
        semantic_review_enabled: bool,
        semantic_review_max_output_tokens: u32,
        actor: &str,
    ) -> Result<FactorySettings, Error> {
        if actor.trim().is_empty()
            || actor.len() > 128
            || !(128..=32_768).contains(&semantic_review_max_output_tokens)
        {
            return Err(Error::Invalid("runtime settings"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "UPDATE factory_settings
             SET semantic_review_enabled=?1, semantic_review_max_output_tokens=?2, updated_at=?3
             WHERE id=1",
        )
        .bind(semantic_review_enabled)
        .bind(i64::from(semantic_review_max_output_tokens))
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        super::record_event(
            &mut transaction,
            "factory-settings",
            "settings.runtime_updated",
            &format!(
                "semantic_review_enabled={semantic_review_enabled} max_output={semantic_review_max_output_tokens}"
            ),
        )
        .await?;
        transaction.commit().await?;
        self.factory_settings().await
    }
}
