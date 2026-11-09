//! Durable owner charter, reserved matters and owner decisions.
//!
//! A charter narrows autonomy; it cannot grant policy autonomy or bypass the
//! owner. Reserved matters always require an explicit owner decision.

use super::*;
use crate::lead::RiskClass;
use chrono::Utc;
use sqlx::row::Row;
use uuid::Uuid;

pub const RESERVED_MATTERS: &[&str] = &[
    "credentials",
    "isolation",
    "production",
    "destructive_migrations",
    "public_contracts",
    "security_exceptions",
    "policy_changes",
    "out_of_budget",
    "cross_project_effects",
    "delivery",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgenticCharterRecord {
    pub id: String,
    pub owner: String,
    pub project_id: String,
    pub domains: Vec<String>,
    pub risk_appetite: RiskClass,
    pub execution_autonomy: bool,
    pub repair_autonomy: bool,
    pub delivery_autonomy: bool,
    pub policy_autonomy: bool,
    pub reserved_matters: Vec<String>,
    pub expires_at: String,
    pub revoked_at: Option<String>,
    pub revocation_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl AgenticCharterRecord {
    pub fn to_owner_charter(&self) -> crate::agentic::OwnerCharter {
        crate::agentic::OwnerCharter {
            id: self.id.clone(),
            owner: self.owner.clone(),
            projects: vec![self.project_id.clone()],
            domains: self.domains.clone(),
            risk_appetite: risk_assurance(self.risk_appetite),
            execution_autonomy: self.execution_autonomy,
            repair_autonomy: self.repair_autonomy,
            delivery_autonomy: self.delivery_autonomy,
            policy_autonomy: self.policy_autonomy,
            reserved_matters: self.reserved_matters.iter().cloned().collect(),
            expires_at: self.expires_at.clone(),
            revoked_at: self.revoked_at.clone(),
        }
    }
}

pub fn risk_assurance(risk: RiskClass) -> crate::agentic::Assurance {
    use crate::agentic::Assurance;
    match risk {
        RiskClass::R0 => Assurance::None,
        RiskClass::R1 => Assurance::Standard,
        RiskClass::R2 => Assurance::Elevated,
        RiskClass::R3 => Assurance::Critical,
    }
}

#[derive(Debug, Clone)]
pub struct NewAgenticCharter<'a> {
    pub owner: &'a str,
    pub project_id: &'a str,
    pub domains: &'a [String],
    pub risk_appetite: RiskClass,
    pub execution_autonomy: bool,
    pub repair_autonomy: bool,
    pub delivery_autonomy: bool,
    pub reserved_matters: &'a [String],
    pub expires_at: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgenticOwnerDecisionRecord {
    pub id: String,
    pub project_id: String,
    pub request_id: Option<String>,
    pub campaign_id: Option<String>,
    pub matter: String,
    pub risk: RiskClass,
    pub status: String,
    pub options: Vec<String>,
    pub evidence: serde_json::Value,
    pub recommendation: String,
    pub deadline: String,
    pub selected_option: Option<String>,
    pub rationale: Option<String>,
    pub resolved_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewAgenticOwnerDecision<'a> {
    pub project_id: &'a str,
    pub request_id: Option<&'a str>,
    pub campaign_id: Option<&'a str>,
    pub matter: &'a str,
    pub risk: RiskClass,
    pub options: &'a [String],
    pub evidence: &'a serde_json::Value,
    pub recommendation: &'a str,
    pub deadline: &'a str,
}

fn charter_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<AgenticCharterRecord, Error> {
    Ok(AgenticCharterRecord {
        id: row.try_get("id")?,
        owner: row.try_get("owner")?,
        project_id: row.try_get("project_id")?,
        domains: serde_json::from_str(&row.try_get::<String, _>("domains_json")?)
            .map_err(|_| Error::Invalid("charter domains"))?,
        risk_appetite: parse_risk(&row.try_get::<String, _>("risk_appetite")?)?,
        execution_autonomy: row.try_get::<bool, _>("execution_autonomy")?,
        repair_autonomy: row.try_get::<bool, _>("repair_autonomy")?,
        delivery_autonomy: row.try_get::<bool, _>("delivery_autonomy")?,
        policy_autonomy: row.try_get::<bool, _>("policy_autonomy")?,
        reserved_matters: serde_json::from_str(&row.try_get::<String, _>("reserved_matters_json")?)
            .map_err(|_| Error::Invalid("charter reserved matters"))?,
        expires_at: row.try_get("expires_at")?,
        revoked_at: row.try_get("revoked_at")?,
        revocation_reason: row.try_get("revocation_reason")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn parse_risk(value: &str) -> Result<RiskClass, Error> {
    match value {
        "R0" => Ok(RiskClass::R0),
        "R1" => Ok(RiskClass::R1),
        "R2" => Ok(RiskClass::R2),
        "R3" => Ok(RiskClass::R3),
        _ => Err(Error::Invalid("risk class")),
    }
}

fn risk_str(risk: RiskClass) -> &'static str {
    match risk {
        RiskClass::R0 => "R0",
        RiskClass::R1 => "R1",
        RiskClass::R2 => "R2",
        RiskClass::R3 => "R3",
    }
}

fn decision_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<AgenticOwnerDecisionRecord, Error> {
    Ok(AgenticOwnerDecisionRecord {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        request_id: row.try_get("request_id")?,
        campaign_id: row.try_get("campaign_id")?,
        matter: row.try_get("matter")?,
        risk: parse_risk(&row.try_get::<String, _>("risk")?)?,
        status: row.try_get("status")?,
        options: serde_json::from_str(&row.try_get::<String, _>("options_json")?)
            .map_err(|_| Error::Invalid("decision options"))?,
        evidence: serde_json::from_str(&row.try_get::<String, _>("evidence_json")?)
            .map_err(|_| Error::Invalid("decision evidence"))?,
        recommendation: row.try_get("recommendation")?,
        deadline: row.try_get("deadline")?,
        selected_option: row.try_get("selected_option")?,
        rationale: row.try_get("rationale")?,
        resolved_by: row.try_get("resolved_by")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        resolved_at: row.try_get("resolved_at")?,
    })
}

impl Database {
    pub async fn create_agentic_charter(
        &self,
        input: &NewAgenticCharter<'_>,
    ) -> Result<AgenticCharterRecord, Error> {
        if input.owner.trim().is_empty()
            || input.domains.is_empty()
            || input.reserved_matters.is_empty()
            || input.expires_at.trim().is_empty()
        {
            return Err(Error::Invalid("owner charter fields"));
        }
        if input
            .reserved_matters
            .iter()
            .any(|matter| !RESERVED_MATTERS.contains(&matter.as_str()))
        {
            return Err(Error::Invalid("owner charter reserved matter"));
        }
        let now = Utc::now().to_rfc3339();
        if input.expires_at <= now.as_str() {
            return Err(Error::Invalid("owner charter expiry"));
        }
        let charter = crate::agentic::OwnerCharter {
            id: "draft".into(),
            owner: input.owner.into(),
            projects: vec![input.project_id.into()],
            domains: input.domains.to_vec(),
            risk_appetite: risk_assurance(input.risk_appetite),
            execution_autonomy: input.execution_autonomy,
            repair_autonomy: input.repair_autonomy,
            delivery_autonomy: input.delivery_autonomy,
            policy_autonomy: false,
            reserved_matters: input.reserved_matters.iter().cloned().collect(),
            expires_at: input.expires_at.into(),
            revoked_at: None,
        };
        charter.validate().map_err(Error::Validation)?;
        let domains_json = serde_json::to_string(input.domains)
            .map_err(|_| Error::Invalid("charter domains encoding"))?;
        let reserved_matters_json = serde_json::to_string(input.reserved_matters)
            .map_err(|_| Error::Invalid("charter reserved encoding"))?;
        let id = Uuid::new_v4().to_string();
        let _write = self.write_lock.lock().await;
        sqlx::query::query(
            "INSERT INTO agentic_owner_charters
             (id,owner,project_id,domains_json,risk_appetite,execution_autonomy,
              repair_autonomy,delivery_autonomy,policy_autonomy,reserved_matters_json,
              expires_at,revoked_at,revocation_reason,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,0,?9,?10,NULL,NULL,?11,?11)",
        )
        .bind(&id)
        .bind(input.owner)
        .bind(input.project_id)
        .bind(&domains_json)
        .bind(risk_str(input.risk_appetite))
        .bind(input.execution_autonomy)
        .bind(input.repair_autonomy)
        .bind(input.delivery_autonomy)
        .bind(&reserved_matters_json)
        .bind(input.expires_at)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.agentic_charter(&id).await
    }

    pub async fn agentic_charter(&self, id: &str) -> Result<AgenticCharterRecord, Error> {
        let row = sqlx::query::query(
            "SELECT id,owner,project_id,domains_json,risk_appetite,execution_autonomy,
                    repair_autonomy,delivery_autonomy,policy_autonomy,reserved_matters_json,
                    expires_at,revoked_at,revocation_reason,created_at,updated_at
             FROM agentic_owner_charters WHERE id=?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        charter_from_row(&row)
    }

    pub async fn active_agentic_charter(
        &self,
        project_id: &str,
    ) -> Result<Option<AgenticCharterRecord>, Error> {
        let now = Utc::now().to_rfc3339();
        let row = sqlx::query::query(
            "SELECT id,owner,project_id,domains_json,risk_appetite,execution_autonomy,
                    repair_autonomy,delivery_autonomy,policy_autonomy,reserved_matters_json,
                    expires_at,revoked_at,revocation_reason,created_at,updated_at
             FROM agentic_owner_charters
             WHERE project_id=?1 AND revoked_at IS NULL AND expires_at > ?2
             ORDER BY created_at DESC,id DESC LIMIT 1",
        )
        .bind(project_id)
        .bind(&now)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(charter_from_row).transpose()
    }

    pub async fn agentic_charters(
        &self,
        project_id: Option<&str>,
    ) -> Result<Vec<AgenticCharterRecord>, Error> {
        let rows = if let Some(project_id) = project_id {
            sqlx::query::query(
                "SELECT id,owner,project_id,domains_json,risk_appetite,execution_autonomy,
                        repair_autonomy,delivery_autonomy,policy_autonomy,reserved_matters_json,
                        expires_at,revoked_at,revocation_reason,created_at,updated_at
                 FROM agentic_owner_charters WHERE project_id=?1 ORDER BY created_at DESC,id DESC",
            )
            .bind(project_id)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query::query(
                "SELECT id,owner,project_id,domains_json,risk_appetite,execution_autonomy,
                        repair_autonomy,delivery_autonomy,policy_autonomy,reserved_matters_json,
                        expires_at,revoked_at,revocation_reason,created_at,updated_at
                 FROM agentic_owner_charters ORDER BY created_at DESC,id DESC",
            )
            .fetch_all(&self.pool)
            .await?
        };
        rows.iter().map(charter_from_row).collect()
    }

    pub async fn revoke_agentic_charter(
        &self,
        id: &str,
        reason: &str,
    ) -> Result<AgenticCharterRecord, Error> {
        if reason.trim().is_empty() || reason.len() > 500 {
            return Err(Error::Invalid("charter revocation reason"));
        }
        let now = Utc::now().to_rfc3339();
        let _write = self.write_lock.lock().await;
        sqlx::query::query(
            "UPDATE agentic_owner_charters
             SET revoked_at=?1,revocation_reason=?2,updated_at=?1 WHERE id=?3",
        )
        .bind(&now)
        .bind(reason)
        .bind(id)
        .execute(&self.pool)
        .await?;
        self.agentic_charter(id).await
    }

    pub async fn create_agentic_owner_decision(
        &self,
        input: &NewAgenticOwnerDecision<'_>,
    ) -> Result<AgenticOwnerDecisionRecord, Error> {
        if input.matter.trim().is_empty()
            || input.options.is_empty()
            || input.recommendation.trim().is_empty()
            || input.deadline.trim().is_empty()
        {
            return Err(Error::Invalid("owner decision fields"));
        }
        let now = Utc::now().to_rfc3339();
        if input.deadline <= now.as_str() {
            return Err(Error::Invalid("owner decision deadline"));
        }
        let options_json = serde_json::to_string(input.options)
            .map_err(|_| Error::Invalid("decision options encoding"))?;
        let evidence_json = serde_json::to_string(input.evidence)
            .map_err(|_| Error::Invalid("decision evidence encoding"))?;
        let id = Uuid::new_v4().to_string();
        let _write = self.write_lock.lock().await;
        sqlx::query::query(
            "INSERT INTO agentic_owner_decisions
             (id,project_id,request_id,campaign_id,matter,risk,status,options_json,
              evidence_json,recommendation,deadline,selected_option,rationale,resolved_by,
              created_at,updated_at,resolved_at)
             VALUES (?1,?2,?3,?4,?5,?6,'PENDING',?7,?8,?9,?10,NULL,NULL,NULL,?11,?11,NULL)",
        )
        .bind(&id)
        .bind(input.project_id)
        .bind(input.request_id)
        .bind(input.campaign_id)
        .bind(input.matter)
        .bind(risk_str(input.risk))
        .bind(&options_json)
        .bind(&evidence_json)
        .bind(input.recommendation)
        .bind(input.deadline)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.agentic_owner_decision(&id).await
    }

    pub async fn agentic_owner_decision(
        &self,
        id: &str,
    ) -> Result<AgenticOwnerDecisionRecord, Error> {
        let row = sqlx::query::query(
            "SELECT id,project_id,request_id,campaign_id,matter,risk,status,options_json,
                    evidence_json,recommendation,deadline,selected_option,rationale,
                    resolved_by,created_at,updated_at,resolved_at
             FROM agentic_owner_decisions WHERE id=?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        decision_from_row(&row)
    }

    pub async fn agentic_owner_decisions(
        &self,
        project_id: Option<&str>,
        pending_only: bool,
    ) -> Result<Vec<AgenticOwnerDecisionRecord>, Error> {
        let now = Utc::now().to_rfc3339();
        let _write = self.write_lock.lock().await;
        sqlx::query::query(
            "UPDATE agentic_owner_decisions SET status='EXPIRED',updated_at=?1
             WHERE status='PENDING' AND deadline <= ?1",
        )
        .bind(&now)
        .execute(&self.pool)
        .await?;
        let rows = if let Some(project_id) = project_id {
            sqlx::query::query(
                "SELECT id,project_id,request_id,campaign_id,matter,risk,status,options_json,
                        evidence_json,recommendation,deadline,selected_option,rationale,
                        resolved_by,created_at,updated_at,resolved_at
                 FROM agentic_owner_decisions WHERE project_id=?1
                 ORDER BY created_at DESC,id DESC",
            )
            .bind(project_id)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query::query(
                "SELECT id,project_id,request_id,campaign_id,matter,risk,status,options_json,
                        evidence_json,recommendation,deadline,selected_option,rationale,
                        resolved_by,created_at,updated_at,resolved_at
                 FROM agentic_owner_decisions ORDER BY created_at DESC,id DESC",
            )
            .fetch_all(&self.pool)
            .await?
        };
        let decisions = rows
            .iter()
            .map(decision_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(if pending_only {
            decisions
                .into_iter()
                .filter(|decision| decision.status == "PENDING")
                .collect()
        } else {
            decisions
        })
    }

    pub async fn resolve_agentic_owner_decision(
        &self,
        id: &str,
        option: &str,
        rationale: &str,
        actor: &str,
    ) -> Result<AgenticOwnerDecisionRecord, Error> {
        if option.trim().is_empty() || rationale.trim().is_empty() || actor.trim().is_empty() {
            return Err(Error::Invalid("owner decision resolution"));
        }
        let decision = self.agentic_owner_decision(id).await?;
        if decision.status != "PENDING" {
            return Err(Error::Conflict("owner decision is not pending"));
        }
        if !decision.options.iter().any(|candidate| candidate == option) {
            return Err(Error::Invalid("owner decision option"));
        }
        let status = if option.starts_with("APPROVE") {
            "APPROVED"
        } else if option.starts_with("DENY") {
            "DENIED"
        } else {
            return Err(Error::Invalid("owner decision outcome"));
        };
        let now = Utc::now().to_rfc3339();
        let _write = self.write_lock.lock().await;
        sqlx::query::query(
            "UPDATE agentic_owner_decisions
             SET status=?1,selected_option=?2,rationale=?3,resolved_by=?4,
                 resolved_at=?5,updated_at=?5
             WHERE id=?6 AND status='PENDING'",
        )
        .bind(status)
        .bind(option)
        .bind(rationale)
        .bind(actor)
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        self.agentic_owner_decision(id).await
    }
}
