use super::Error;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::row::Row;
use sqlx_sqlite::{SqliteConnection, SqliteRow};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettingsRecord {
    pub provider: String,
    pub kind: String,
    pub model: String,
    pub base_url: String,
    pub auth_mode: String,
    pub credential_ciphertext: String,
    pub credential_nonce: String,
    pub credential_fingerprint: String,
    pub enabled: bool,
    pub is_default: bool,
    pub max_turns: u32,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_cost_micros: u64,
    pub input_micros_per_million: u64,
    pub output_micros_per_million: u64,
    pub revision: u32,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ProviderSettingsWrite {
    pub provider: String,
    pub kind: String,
    pub model: String,
    pub base_url: String,
    pub auth_mode: String,
    pub credential_ciphertext: String,
    pub credential_nonce: String,
    pub credential_fingerprint: String,
    pub enabled: bool,
    pub is_default: bool,
    pub max_turns: u32,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_cost_micros: u64,
    pub input_micros_per_million: u64,
    pub output_micros_per_million: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettingsAudit {
    pub audit_id: String,
    pub provider: String,
    pub revision: u32,
    pub action: String,
    pub actor: String,
    pub detail: String,
    pub created_at: String,
}

impl super::Database {
    pub async fn provider_settings(&self, provider: &str) -> Result<ProviderSettingsRecord, Error> {
        let mut connection = self.pool.acquire().await?;
        load_provider_settings(&mut connection, provider).await
    }

    pub async fn provider_settings_all(&self) -> Result<Vec<ProviderSettingsRecord>, Error> {
        let rows = sqlx::query::query(
            "SELECT provider,kind,model,base_url,auth_mode,credential_ciphertext,
                    credential_nonce,credential_fingerprint,enabled,is_default,
                    max_turns,max_input_tokens,max_output_tokens,max_cost_micros,
                    input_micros_per_million,output_micros_per_million,revision,updated_at
             FROM provider_settings ORDER BY provider",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(provider_settings_from_row).collect()
    }

    pub async fn default_provider_settings(&self) -> Result<ProviderSettingsRecord, Error> {
        let mut connection = self.pool.acquire().await?;
        load_default_provider_settings(&mut connection).await
    }

    pub async fn save_provider_settings(
        &self,
        input: &ProviderSettingsWrite,
        actor: &str,
    ) -> Result<ProviderSettingsRecord, Error> {
        if actor.trim().is_empty() || actor.len() > 128 {
            return Err(Error::Invalid("settings actor"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let existing = sqlx::query::query(
            "SELECT provider,kind,model,base_url,auth_mode,credential_ciphertext,
                    credential_nonce,credential_fingerprint,enabled,is_default,
                    max_turns,max_input_tokens,max_output_tokens,max_cost_micros,
                    input_micros_per_million,output_micros_per_million,revision,updated_at
             FROM provider_settings WHERE provider=?1",
        )
        .bind(&input.provider)
        .fetch_optional(&mut *transaction)
        .await?
        .map(|row| provider_settings_from_row(&row))
        .transpose()?;
        let revision = existing.map(|record| record.revision + 1).unwrap_or(1);
        let now = Utc::now().to_rfc3339();
        if input.is_default {
            sqlx::query::query("UPDATE provider_settings SET is_default=0 WHERE is_default=1")
                .execute(&mut *transaction)
                .await?;
        }
        sqlx::query::query(
            "INSERT INTO provider_settings
             (provider,kind,model,base_url,auth_mode,credential_ciphertext,credential_nonce,
              credential_fingerprint,enabled,is_default,max_turns,max_input_tokens,
              max_output_tokens,max_cost_micros,input_micros_per_million,
              output_micros_per_million,revision,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
             ON CONFLICT(provider) DO UPDATE SET
               kind=excluded.kind, model=excluded.model, base_url=excluded.base_url,
               auth_mode=excluded.auth_mode,
               credential_ciphertext=excluded.credential_ciphertext,
               credential_nonce=excluded.credential_nonce,
               credential_fingerprint=excluded.credential_fingerprint,
               enabled=excluded.enabled, is_default=excluded.is_default,
               max_turns=excluded.max_turns, max_input_tokens=excluded.max_input_tokens,
               max_output_tokens=excluded.max_output_tokens,
               max_cost_micros=excluded.max_cost_micros,
               input_micros_per_million=excluded.input_micros_per_million,
               output_micros_per_million=excluded.output_micros_per_million,
               revision=excluded.revision, updated_at=excluded.updated_at",
        )
        .bind(&input.provider)
        .bind(&input.kind)
        .bind(&input.model)
        .bind(&input.base_url)
        .bind(&input.auth_mode)
        .bind(&input.credential_ciphertext)
        .bind(&input.credential_nonce)
        .bind(&input.credential_fingerprint)
        .bind(input.enabled)
        .bind(input.is_default)
        .bind(input.max_turns)
        .bind(input.max_input_tokens as i64)
        .bind(input.max_output_tokens as i64)
        .bind(input.max_cost_micros as i64)
        .bind(input.input_micros_per_million as i64)
        .bind(input.output_micros_per_million as i64)
        .bind(revision)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        let audit_id = uuid::Uuid::new_v4().to_string();
        sqlx::query::query(
            "INSERT INTO provider_settings_audit
             (audit_id,provider,revision,action,actor,detail,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
        )
        .bind(&audit_id)
        .bind(&input.provider)
        .bind(revision)
        .bind("configured")
        .bind(actor)
        .bind(format!(
            "model={} kind={} enabled={} default={}",
            input.model, input.kind, input.enabled, input.is_default
        ))
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        super::record_event(
            &mut transaction,
            &input.provider,
            "provider.settings_configured",
            &format!("revision={revision}"),
        )
        .await?;
        transaction.commit().await?;
        let mut connection = self.pool.acquire().await?;
        load_provider_settings(&mut connection, &input.provider).await
    }

    pub async fn clear_provider_credential(
        &self,
        provider: &str,
        actor: &str,
    ) -> Result<ProviderSettingsRecord, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let existing = load_provider_settings(&mut transaction, provider).await?;
        sqlx::query::query(
            "UPDATE provider_settings
             SET credential_ciphertext='', credential_nonce='', credential_fingerprint='',
                 enabled=0, revision=revision+1, updated_at=?1
             WHERE provider=?2",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(provider)
        .execute(&mut *transaction)
        .await?;
        let audit_id = uuid::Uuid::new_v4().to_string();
        sqlx::query::query(
            "INSERT INTO provider_settings_audit
             (audit_id,provider,revision,action,actor,detail,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
        )
        .bind(&audit_id)
        .bind(provider)
        .bind(existing.revision + 1)
        .bind("credential_revoked")
        .bind(actor)
        .bind("credential removed and provider disabled")
        .bind(Utc::now().to_rfc3339())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        let mut connection = self.pool.acquire().await?;
        load_provider_settings(&mut connection, provider).await
    }

    pub async fn provider_settings_audit(
        &self,
        provider: &str,
    ) -> Result<Vec<ProviderSettingsAudit>, Error> {
        let rows = sqlx::query::query(
            "SELECT audit_id,provider,revision,action,actor,detail,created_at
             FROM provider_settings_audit WHERE provider=?1
             ORDER BY created_at DESC LIMIT 50",
        )
        .bind(provider)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(ProviderSettingsAudit {
                    audit_id: row.try_get("audit_id")?,
                    provider: row.try_get("provider")?,
                    revision: row.try_get::<i64, _>("revision")? as u32,
                    action: row.try_get("action")?,
                    actor: row.try_get("actor")?,
                    detail: row.try_get("detail")?,
                    created_at: row.try_get("created_at")?,
                })
            })
            .collect()
    }

    pub async fn record_provider_test(
        &self,
        provider: &str,
        actor: &str,
        detail: &str,
    ) -> Result<(), Error> {
        let revision: i64 = sqlx::query_scalar::query_scalar(
            "SELECT revision FROM provider_settings WHERE provider=?1",
        )
        .bind(provider)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(0);
        sqlx::query::query(
            "INSERT INTO provider_settings_audit
             (audit_id,provider,revision,action,actor,detail,created_at)
             VALUES (?1,?2,?3,'connection_tested',?4,?5,?6)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(provider)
        .bind(revision)
        .bind(actor)
        .bind(detail.chars().take(512).collect::<String>())
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

const SELECT_COLUMNS: &str = "SELECT provider,kind,model,base_url,auth_mode,credential_ciphertext,
        credential_nonce,credential_fingerprint,enabled,is_default,
        max_turns,max_input_tokens,max_output_tokens,max_cost_micros,
        input_micros_per_million,output_micros_per_million,revision,updated_at
 FROM provider_settings";

async fn load_provider_settings(
    connection: &mut SqliteConnection,
    provider: &str,
) -> Result<ProviderSettingsRecord, Error> {
    let query = format!("{SELECT_COLUMNS} WHERE provider=?1");
    sqlx::query::query(&query)
        .bind(provider)
        .fetch_optional(&mut *connection)
        .await?
        .map(|row| provider_settings_from_row(&row))
        .transpose()?
        .ok_or(Error::NotFound)
}

async fn load_default_provider_settings(
    connection: &mut SqliteConnection,
) -> Result<ProviderSettingsRecord, Error> {
    let query = format!("{SELECT_COLUMNS} WHERE is_default=1 AND enabled=1");
    sqlx::query::query(&query)
        .fetch_optional(&mut *connection)
        .await?
        .map(|row| provider_settings_from_row(&row))
        .transpose()?
        .ok_or(Error::NotFound)
}

fn provider_settings_from_row(row: &SqliteRow) -> Result<ProviderSettingsRecord, Error> {
    Ok(ProviderSettingsRecord {
        provider: row.try_get("provider")?,
        kind: row.try_get("kind")?,
        model: row.try_get("model")?,
        base_url: row.try_get("base_url")?,
        auth_mode: row.try_get("auth_mode")?,
        credential_ciphertext: row.try_get("credential_ciphertext")?,
        credential_nonce: row.try_get("credential_nonce")?,
        credential_fingerprint: row.try_get("credential_fingerprint")?,
        enabled: row.try_get::<i64, _>("enabled")? != 0,
        is_default: row.try_get::<i64, _>("is_default")? != 0,
        max_turns: row.try_get::<i64, _>("max_turns")? as u32,
        max_input_tokens: row.try_get::<i64, _>("max_input_tokens")? as u64,
        max_output_tokens: row.try_get::<i64, _>("max_output_tokens")? as u64,
        max_cost_micros: row.try_get::<i64, _>("max_cost_micros")? as u64,
        input_micros_per_million: row.try_get::<i64, _>("input_micros_per_million")? as u64,
        output_micros_per_million: row.try_get::<i64, _>("output_micros_per_million")? as u64,
        revision: row.try_get::<i64, _>("revision")? as u32,
        updated_at: row.try_get("updated_at")?,
    })
}
