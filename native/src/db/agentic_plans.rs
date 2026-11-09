//! Durable Planner work graphs, frozen formal specifications and the
//! GitHub/local board projection.
//!
//! Core owns the frozen plan and specification. Model drafts, GitHub issues and
//! the kanban are evidence and projections; none of them can widen scope or
//! mutate a frozen record.

use super::*;
use chrono::Utc;
use sqlx::row::Row;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgenticWorkGraphRecord {
    pub request_id: String,
    pub graph_json: String,
    pub graph_sha256: String,
    pub topological_order_json: String,
    pub planner_provider: String,
    pub planner_model: String,
    pub schema_sha256: String,
    pub request_sha256: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: u64,
    pub usage_known: bool,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct NewAgenticWorkGraph<'a> {
    pub request_id: &'a str,
    pub graph: &'a crate::agentic::ValidatedWorkGraph,
    pub provider: &'a str,
    pub model: &'a str,
    pub schema_sha256: &'a str,
    pub request_sha256: &'a str,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: u64,
    pub usage_known: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgenticSpecRecord {
    pub id: String,
    pub request_id: String,
    pub revision: u32,
    pub state: String,
    pub draft_json: String,
    pub frozen_json: Option<String>,
    pub spec_sha256: Option<String>,
    pub graph_json: Option<String>,
    pub graph_sha256: Option<String>,
    pub provider: String,
    pub model: String,
    pub schema_sha256: String,
    pub request_sha256: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: u64,
    pub usage_known: bool,
    pub created_at: String,
    pub frozen_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewAgenticSpecDraft<'a> {
    pub request_id: &'a str,
    pub draft_json: &'a str,
    pub graph_sha256: Option<&'a str>,
    pub provider: &'a str,
    pub model: &'a str,
    pub schema_sha256: &'a str,
    pub request_sha256: &'a str,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: u64,
    pub usage_known: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgenticSpecPublicationRecord {
    pub id: String,
    pub spec_id: String,
    pub project_id: String,
    pub provider: String,
    pub repository: String,
    pub status: String,
    pub drift_status: String,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct NewAgenticSpecPublication<'a> {
    pub spec_id: &'a str,
    pub project_id: &'a str,
    pub provider: &'a str,
    pub repository: &'a str,
    pub status: &'a str,
    pub drift_status: &'a str,
    pub last_error: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgenticSpecIssueRecord {
    pub spec_id: String,
    pub unit_id: String,
    pub marker: String,
    pub external_number: Option<i64>,
    pub external_url: Option<String>,
    pub state: String,
    pub updated_at: String,
}

fn u64_field(value: i64) -> Result<u64, Error> {
    value
        .try_into()
        .map_err(|_| Error::Invalid("agentic numeric field is outside the bound"))
}

fn work_graph_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<AgenticWorkGraphRecord, Error> {
    Ok(AgenticWorkGraphRecord {
        request_id: row.try_get("request_id")?,
        graph_json: row.try_get("graph_json")?,
        graph_sha256: row.try_get("graph_sha256")?,
        topological_order_json: row.try_get("topological_order_json")?,
        planner_provider: row.try_get("planner_provider")?,
        planner_model: row.try_get("planner_model")?,
        schema_sha256: row.try_get("schema_sha256")?,
        request_sha256: row.try_get("request_sha256")?,
        input_tokens: u64_field(row.try_get::<i64, _>("input_tokens")?)?,
        output_tokens: u64_field(row.try_get::<i64, _>("output_tokens")?)?,
        total_tokens: u64_field(row.try_get::<i64, _>("total_tokens")?)?,
        cost_micros: u64_field(row.try_get::<i64, _>("cost_micros")?)?,
        usage_known: row.try_get::<bool, _>("usage_known")?,
        created_at: row.try_get("created_at")?,
    })
}

fn spec_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<AgenticSpecRecord, Error> {
    Ok(AgenticSpecRecord {
        id: row.try_get("id")?,
        request_id: row.try_get("request_id")?,
        revision: row
            .try_get::<i64, _>("revision")?
            .try_into()
            .map_err(|_| Error::Invalid("agentic spec revision is outside the bound"))?,
        state: row.try_get("state")?,
        draft_json: row.try_get("draft_json")?,
        frozen_json: row.try_get("frozen_json")?,
        spec_sha256: row.try_get("spec_sha256")?,
        graph_json: row.try_get("graph_json")?,
        graph_sha256: row.try_get("graph_sha256")?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        schema_sha256: row.try_get("schema_sha256")?,
        request_sha256: row.try_get("request_sha256")?,
        input_tokens: u64_field(row.try_get::<i64, _>("input_tokens")?)?,
        output_tokens: u64_field(row.try_get::<i64, _>("output_tokens")?)?,
        total_tokens: u64_field(row.try_get::<i64, _>("total_tokens")?)?,
        cost_micros: u64_field(row.try_get::<i64, _>("cost_micros")?)?,
        usage_known: row.try_get::<bool, _>("usage_known")?,
        created_at: row.try_get("created_at")?,
        frozen_at: row.try_get("frozen_at")?,
    })
}

fn publication_from_row(
    row: &sqlx_sqlite::SqliteRow,
) -> Result<AgenticSpecPublicationRecord, Error> {
    Ok(AgenticSpecPublicationRecord {
        id: row.try_get("id")?,
        spec_id: row.try_get("spec_id")?,
        project_id: row.try_get("project_id")?,
        provider: row.try_get("provider")?,
        repository: row.try_get("repository")?,
        status: row.try_get("status")?,
        drift_status: row.try_get("drift_status")?,
        last_error: row.try_get("last_error")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn spec_issue_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<AgenticSpecIssueRecord, Error> {
    Ok(AgenticSpecIssueRecord {
        spec_id: row.try_get("spec_id")?,
        unit_id: row.try_get("unit_id")?,
        marker: row.try_get("marker")?,
        external_number: row.try_get("external_number")?,
        external_url: row.try_get("external_url")?,
        state: row.try_get("state")?,
        updated_at: row.try_get("updated_at")?,
    })
}

impl Database {
    pub async fn save_agentic_work_graph(
        &self,
        input: &NewAgenticWorkGraph<'_>,
    ) -> Result<AgenticWorkGraphRecord, Error> {
        let graph_json = serde_json::to_string(input.graph)
            .map_err(|_| Error::Invalid("work graph encoding"))?;
        let topological_order_json = serde_json::to_string(&input.graph.topological_order)
            .map_err(|_| Error::Invalid("work graph order encoding"))?;
        if input.graph.graph_sha256.len() != 64
            || !input
                .graph
                .graph_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || input.schema_sha256.len() != 64
            || input.request_sha256.len() != 64
            || input.provider.trim().is_empty()
            || input.model.trim().is_empty()
        {
            return Err(Error::Invalid("work graph identity"));
        }
        let _write = self.write_lock.lock().await;
        let frozen: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT id FROM agentic_specs WHERE request_id=?1 AND state='FROZEN'",
        )
        .bind(input.request_id)
        .fetch_optional(&self.pool)
        .await?;
        if frozen.is_some() {
            return Err(Error::Conflict(
                "a frozen specification already binds this request's plan",
            ));
        }
        let created_at = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO agentic_work_graphs
             (request_id,graph_json,graph_sha256,topological_order_json,planner_provider,
              planner_model,schema_sha256,request_sha256,input_tokens,output_tokens,
              total_tokens,cost_micros,usage_known,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
             ON CONFLICT(request_id) DO UPDATE SET
               graph_json=excluded.graph_json,
               graph_sha256=excluded.graph_sha256,
               topological_order_json=excluded.topological_order_json,
               planner_provider=excluded.planner_provider,
               planner_model=excluded.planner_model,
               schema_sha256=excluded.schema_sha256,
               request_sha256=excluded.request_sha256,
               input_tokens=excluded.input_tokens,
               output_tokens=excluded.output_tokens,
               total_tokens=excluded.total_tokens,
               cost_micros=excluded.cost_micros,
               usage_known=excluded.usage_known,
               created_at=excluded.created_at",
        )
        .bind(input.request_id)
        .bind(&graph_json)
        .bind(&input.graph.graph_sha256)
        .bind(&topological_order_json)
        .bind(input.provider)
        .bind(input.model)
        .bind(input.schema_sha256)
        .bind(input.request_sha256)
        .bind(i64::try_from(input.input_tokens).unwrap_or(i64::MAX))
        .bind(i64::try_from(input.output_tokens).unwrap_or(i64::MAX))
        .bind(i64::try_from(input.total_tokens).unwrap_or(i64::MAX))
        .bind(i64::try_from(input.cost_micros).unwrap_or(i64::MAX))
        .bind(input.usage_known)
        .bind(&created_at)
        .execute(&self.pool)
        .await?;
        self.agentic_work_graph(input.request_id).await
    }

    pub async fn agentic_work_graph(
        &self,
        request_id: &str,
    ) -> Result<AgenticWorkGraphRecord, Error> {
        let row = sqlx::query::query(
            "SELECT request_id,graph_json,graph_sha256,topological_order_json,planner_provider,
                    planner_model,schema_sha256,request_sha256,input_tokens,output_tokens,
                    total_tokens,cost_micros,usage_known,created_at
             FROM agentic_work_graphs WHERE request_id=?1",
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        work_graph_from_row(&row)
    }

    /// Save a Spec Manager draft. An unfrozen draft is replaced atomically; a
    /// frozen specification is never mutated and a new revision is created.
    pub async fn save_agentic_spec_draft(
        &self,
        input: &NewAgenticSpecDraft<'_>,
    ) -> Result<AgenticSpecRecord, Error> {
        if input.draft_json.trim().is_empty()
            || input.provider.trim().is_empty()
            || input.model.trim().is_empty()
            || input.schema_sha256.len() != 64
            || input.request_sha256.len() != 64
        {
            return Err(Error::Invalid("agentic spec identity"));
        }
        let _write = self.write_lock.lock().await;
        let existing: Option<AgenticSpecRecord> = sqlx::query::query(
            "SELECT id,request_id,revision,state,draft_json,frozen_json,spec_sha256,
                    graph_json,graph_sha256,provider,model,schema_sha256,request_sha256,
                    input_tokens,output_tokens,total_tokens,cost_micros,usage_known,
                    created_at,frozen_at
             FROM agentic_specs WHERE request_id=?1 AND state='DRAFT'",
        )
        .bind(input.request_id)
        .fetch_optional(&self.pool)
        .await?
        .map(|row| spec_from_row(&row))
        .transpose()?;
        let now = Utc::now().to_rfc3339();
        if let Some(existing) = existing {
            sqlx::query::query(
                "UPDATE agentic_specs
                 SET draft_json=?1,provider=?2,model=?3,schema_sha256=?4,
                     request_sha256=?5,input_tokens=?6,output_tokens=?7,total_tokens=?8,
                     cost_micros=?9,usage_known=?10,graph_sha256=?11
                 WHERE id=?12",
            )
            .bind(input.draft_json)
            .bind(input.provider)
            .bind(input.model)
            .bind(input.schema_sha256)
            .bind(input.request_sha256)
            .bind(i64::try_from(input.input_tokens).unwrap_or(i64::MAX))
            .bind(i64::try_from(input.output_tokens).unwrap_or(i64::MAX))
            .bind(i64::try_from(input.total_tokens).unwrap_or(i64::MAX))
            .bind(i64::try_from(input.cost_micros).unwrap_or(i64::MAX))
            .bind(input.usage_known)
            .bind(input.graph_sha256)
            .bind(&existing.id)
            .execute(&self.pool)
            .await?;
            return self.agentic_spec_by_id(&existing.id).await;
        }
        let revision: (i64,) = sqlx::query_as::query_as(
            "SELECT COALESCE(MAX(revision),0)+1 FROM agentic_specs WHERE request_id=?1",
        )
        .bind(input.request_id)
        .fetch_one(&self.pool)
        .await?;
        let id = Uuid::new_v4().to_string();
        sqlx::query::query(
            "INSERT INTO agentic_specs
             (id,request_id,revision,state,draft_json,frozen_json,spec_sha256,
              graph_json,graph_sha256,provider,model,schema_sha256,request_sha256,
              input_tokens,output_tokens,total_tokens,cost_micros,usage_known,
              created_at,frozen_at)
             VALUES (?1,?2,?3,'DRAFT',?4,NULL,NULL,NULL,?15,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,NULL)",
        )
        .bind(&id)
        .bind(input.request_id)
        .bind(revision.0)
        .bind(input.draft_json)
        .bind(input.provider)
        .bind(input.model)
        .bind(input.schema_sha256)
        .bind(input.request_sha256)
        .bind(i64::try_from(input.input_tokens).unwrap_or(i64::MAX))
        .bind(i64::try_from(input.output_tokens).unwrap_or(i64::MAX))
        .bind(i64::try_from(input.total_tokens).unwrap_or(i64::MAX))
        .bind(i64::try_from(input.cost_micros).unwrap_or(i64::MAX))
        .bind(input.usage_known)
        .bind(&now)
        .bind(input.graph_sha256)
        .execute(&self.pool)
        .await?;
        self.agentic_spec_by_id(&id).await
    }

    pub async fn agentic_spec_by_id(&self, id: &str) -> Result<AgenticSpecRecord, Error> {
        let row = sqlx::query::query(
            "SELECT id,request_id,revision,state,draft_json,frozen_json,spec_sha256,
                    graph_json,graph_sha256,provider,model,schema_sha256,request_sha256,
                    input_tokens,output_tokens,total_tokens,cost_micros,usage_known,
                    created_at,frozen_at
             FROM agentic_specs WHERE id=?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        spec_from_row(&row)
    }

    /// The current frozen specification when one exists, otherwise the current
    /// draft. A superseded revision is never returned as current.
    pub async fn agentic_spec(&self, request_id: &str) -> Result<AgenticSpecRecord, Error> {
        let row = sqlx::query::query(
            "SELECT id,request_id,revision,state,draft_json,frozen_json,spec_sha256,
                    graph_json,graph_sha256,provider,model,schema_sha256,request_sha256,
                    input_tokens,output_tokens,total_tokens,cost_micros,usage_known,
                    created_at,frozen_at
             FROM agentic_specs
             WHERE request_id=?1 AND state IN ('FROZEN','DRAFT')
             ORDER BY CASE state WHEN 'FROZEN' THEN 0 ELSE 1 END,revision DESC
             LIMIT 1",
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        spec_from_row(&row)
    }

    pub async fn agentic_spec_draft(&self, request_id: &str) -> Result<AgenticSpecRecord, Error> {
        let row = sqlx::query::query(
            "SELECT id,request_id,revision,state,draft_json,frozen_json,spec_sha256,
                    graph_json,graph_sha256,provider,model,schema_sha256,request_sha256,
                    input_tokens,output_tokens,total_tokens,cost_micros,usage_known,
                    created_at,frozen_at
             FROM agentic_specs WHERE request_id=?1 AND state='DRAFT'",
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        spec_from_row(&row)
    }

    /// Freeze one exact draft revision. The previous frozen revision becomes
    /// superseded in the same transaction; its record is retained.
    pub async fn freeze_agentic_spec(
        &self,
        spec_id: &str,
        frozen: &crate::agentic::FrozenSpec,
        graph: &crate::agentic::ValidatedWorkGraph,
    ) -> Result<AgenticSpecRecord, Error> {
        let frozen_json =
            serde_json::to_string(frozen).map_err(|_| Error::Invalid("frozen spec encoding"))?;
        let graph_json =
            serde_json::to_string(graph).map_err(|_| Error::Invalid("frozen graph encoding"))?;
        let graph_sha256 = graph.graph_sha256.clone();
        let spec_sha256 = frozen.spec_sha256.clone();
        if spec_sha256.len() != 64 || graph_sha256.len() != 64 {
            return Err(Error::Invalid("frozen spec identity"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query::query(
            "SELECT id,request_id,revision,state,draft_json,frozen_json,spec_sha256,
                    graph_json,graph_sha256,provider,model,schema_sha256,request_sha256,
                    input_tokens,output_tokens,total_tokens,cost_micros,usage_known,
                    created_at,frozen_at
             FROM agentic_specs WHERE id=?1",
        )
        .bind(spec_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(Error::NotFound)?;
        let record = spec_from_row(&row)?;
        if record.state != "DRAFT" {
            return Err(Error::Conflict("only a draft specification can be frozen"));
        }
        if record.revision != frozen.revision {
            return Err(Error::Conflict(
                "the frozen revision does not match the draft",
            ));
        }
        if let Some(draft_graph) = record.graph_sha256.as_deref()
            && draft_graph != graph_sha256
        {
            return Err(Error::Conflict(
                "the WorkGraph changed after the draft was authored; author a new draft",
            ));
        }
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "UPDATE agentic_specs SET state='SUPERSEDED'
             WHERE request_id=?1 AND state='FROZEN'",
        )
        .bind(&record.request_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query(
            "UPDATE agentic_specs
             SET state='FROZEN',frozen_json=?1,spec_sha256=?2,graph_json=?3,
                 graph_sha256=?4,frozen_at=?5
             WHERE id=?6",
        )
        .bind(&frozen_json)
        .bind(&spec_sha256)
        .bind(&graph_json)
        .bind(&graph_sha256)
        .bind(&now)
        .bind(spec_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        self.agentic_spec_by_id(spec_id).await
    }

    pub async fn frozen_agentic_specs(&self) -> Result<Vec<AgenticSpecRecord>, Error> {
        let rows = sqlx::query::query(
            "SELECT id,request_id,revision,state,draft_json,frozen_json,spec_sha256,
                    graph_json,graph_sha256,provider,model,schema_sha256,request_sha256,
                    input_tokens,output_tokens,total_tokens,cost_micros,usage_known,
                    created_at,frozen_at
             FROM agentic_specs WHERE state='FROZEN'
             ORDER BY frozen_at DESC,id DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(spec_from_row).collect()
    }

    pub async fn upsert_agentic_spec_publication(
        &self,
        input: &NewAgenticSpecPublication<'_>,
    ) -> Result<AgenticSpecPublicationRecord, Error> {
        if !matches!(input.provider, "GITHUB" | "LOCAL")
            || !matches!(input.status, "PENDING" | "PUBLISHED" | "FAILED" | "LOCAL")
            || !matches!(input.drift_status, "IN_SYNC" | "DRIFTED" | "UNKNOWN")
        {
            return Err(Error::Invalid("spec publication state"));
        }
        let _write = self.write_lock.lock().await;
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO agentic_spec_publications
             (id,spec_id,project_id,provider,repository,status,drift_status,last_error,
              created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)
             ON CONFLICT(spec_id) DO UPDATE SET
               project_id=excluded.project_id,
               provider=excluded.provider,
               repository=excluded.repository,
               status=excluded.status,
               drift_status=excluded.drift_status,
               last_error=excluded.last_error,
               updated_at=excluded.updated_at",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(input.spec_id)
        .bind(input.project_id)
        .bind(input.provider)
        .bind(input.repository)
        .bind(input.status)
        .bind(input.drift_status)
        .bind(input.last_error)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.agentic_spec_publication(input.spec_id).await
    }

    pub async fn agentic_spec_publication(
        &self,
        spec_id: &str,
    ) -> Result<AgenticSpecPublicationRecord, Error> {
        let row = sqlx::query::query(
            "SELECT id,spec_id,project_id,provider,repository,status,drift_status,last_error,
                    created_at,updated_at
             FROM agentic_spec_publications WHERE spec_id=?1",
        )
        .bind(spec_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        publication_from_row(&row)
    }

    pub async fn record_agentic_spec_issue(
        &self,
        spec_id: &str,
        unit_id: &str,
        marker: &str,
        external_number: Option<i64>,
        external_url: Option<&str>,
        state: &str,
    ) -> Result<AgenticSpecIssueRecord, Error> {
        if !matches!(state, "PENDING" | "PUBLISHED" | "FAILED") {
            return Err(Error::Invalid("spec issue state"));
        }
        let _write = self.write_lock.lock().await;
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO agentic_spec_issues
             (spec_id,unit_id,marker,external_number,external_url,state,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(spec_id,unit_id) DO UPDATE SET
               marker=excluded.marker,
               external_number=excluded.external_number,
               external_url=excluded.external_url,
               state=excluded.state,
               updated_at=excluded.updated_at",
        )
        .bind(spec_id)
        .bind(unit_id)
        .bind(marker)
        .bind(external_number)
        .bind(external_url)
        .bind(state)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.agentic_spec_issue(spec_id, unit_id).await
    }

    pub async fn agentic_spec_issue(
        &self,
        spec_id: &str,
        unit_id: &str,
    ) -> Result<AgenticSpecIssueRecord, Error> {
        let row = sqlx::query::query(
            "SELECT spec_id,unit_id,marker,external_number,external_url,state,updated_at
             FROM agentic_spec_issues WHERE spec_id=?1 AND unit_id=?2",
        )
        .bind(spec_id)
        .bind(unit_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        spec_issue_from_row(&row)
    }

    pub async fn agentic_spec_issues(
        &self,
        spec_id: &str,
    ) -> Result<Vec<AgenticSpecIssueRecord>, Error> {
        let rows = sqlx::query::query(
            "SELECT spec_id,unit_id,marker,external_number,external_url,state,updated_at
             FROM agentic_spec_issues WHERE spec_id=?1 ORDER BY unit_id ASC",
        )
        .bind(spec_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(spec_issue_from_row).collect()
    }
}
