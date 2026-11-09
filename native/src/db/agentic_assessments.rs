//! Durable advisory agentic triage/stack assessments.
//!
//! A model assessment is evidence, never authority. Core triage floors,
//! execution profiles, grants and the Quality Gate remain separate.

use super::*;
use chrono::Utc;
use sqlx::row::Row;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgenticAssessmentRecord {
    pub request_id: String,
    pub response_json: String,
    pub schema_sha256: String,
    pub provider: String,
    pub model: String,
    pub request_sha256: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: u64,
    pub usage_known: bool,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct NewAgenticAssessment<'a> {
    pub request_id: &'a str,
    pub response_json: &'a str,
    pub schema_sha256: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub request_sha256: &'a str,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: u64,
    pub usage_known: bool,
}

impl Database {
    pub async fn save_agentic_assessment(
        &self,
        input: &NewAgenticAssessment<'_>,
    ) -> Result<AgenticAssessmentRecord, Error> {
        if input.request_id.trim().is_empty()
            || input.response_json.trim().is_empty()
            || input.schema_sha256.len() != 64
            || !input
                .schema_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || input.provider.trim().is_empty()
            || input.model.trim().is_empty()
            || input.request_sha256.len() != 64
            || !input
                .request_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(Error::Invalid("agentic assessment identity"));
        }
        let created_at = Utc::now().to_rfc3339();
        let _write = self.write_lock.lock().await;
        sqlx::query::query(
            "INSERT INTO agentic_assessments
             (request_id,response_json,schema_sha256,provider,model,request_sha256,
              input_tokens,output_tokens,total_tokens,cost_micros,usage_known,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(request_id) DO UPDATE SET
               response_json=excluded.response_json,
               schema_sha256=excluded.schema_sha256,
               provider=excluded.provider,
               model=excluded.model,
               request_sha256=excluded.request_sha256,
               input_tokens=excluded.input_tokens,
               output_tokens=excluded.output_tokens,
               total_tokens=excluded.total_tokens,
               cost_micros=excluded.cost_micros,
               usage_known=excluded.usage_known,
               created_at=excluded.created_at",
        )
        .bind(input.request_id)
        .bind(input.response_json)
        .bind(input.schema_sha256)
        .bind(input.provider)
        .bind(input.model)
        .bind(input.request_sha256)
        .bind(i64::try_from(input.input_tokens).unwrap_or(i64::MAX))
        .bind(i64::try_from(input.output_tokens).unwrap_or(i64::MAX))
        .bind(i64::try_from(input.total_tokens).unwrap_or(i64::MAX))
        .bind(i64::try_from(input.cost_micros).unwrap_or(i64::MAX))
        .bind(i64::from(input.usage_known))
        .bind(&created_at)
        .execute(&self.pool)
        .await?;
        self.agentic_assessment(input.request_id).await
    }

    pub async fn agentic_assessment(
        &self,
        request_id: &str,
    ) -> Result<AgenticAssessmentRecord, Error> {
        let row = sqlx::query::query(
            "SELECT request_id,response_json,schema_sha256,provider,model,request_sha256,
                    input_tokens,output_tokens,total_tokens,cost_micros,usage_known,created_at
             FROM agentic_assessments WHERE request_id=?1",
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        let usage_known: i64 = row.try_get("usage_known")?;
        Ok(AgenticAssessmentRecord {
            request_id: row.try_get("request_id")?,
            response_json: row.try_get("response_json")?,
            schema_sha256: row.try_get("schema_sha256")?,
            provider: row.try_get("provider")?,
            model: row.try_get("model")?,
            request_sha256: row.try_get("request_sha256")?,
            input_tokens: u64::try_from(row.try_get::<i64, _>("input_tokens")?)
                .map_err(|_| Error::Conflict("stored agentic input usage is invalid"))?,
            output_tokens: u64::try_from(row.try_get::<i64, _>("output_tokens")?)
                .map_err(|_| Error::Conflict("stored agentic output usage is invalid"))?,
            total_tokens: u64::try_from(row.try_get::<i64, _>("total_tokens")?)
                .map_err(|_| Error::Conflict("stored agentic total usage is invalid"))?,
            cost_micros: u64::try_from(row.try_get::<i64, _>("cost_micros")?)
                .map_err(|_| Error::Conflict("stored agentic cost usage is invalid"))?,
            usage_known: usage_known == 1,
            created_at: row.try_get("created_at")?,
        })
    }
}
