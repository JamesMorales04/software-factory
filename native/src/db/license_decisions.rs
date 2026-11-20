//! Durable exact Lead licence decisions under an Owner delegation.
//!
//! The Owner delegation is an `agentic_owner_decisions` row. This table records
//! the exact Lead decision bound to its verified actor, identity, evidence, use
//! context and revision. Decisions are immutable; revocation is recorded.

use super::*;
use chrono::Utc;
use sqlx::row::Row;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseDecisionRecord {
    pub id: String,
    pub delegation_id: String,
    pub project_id: String,
    pub lead_request_id: String,
    pub lead_reservation_id: String,
    pub lead_actor_sha256: String,
    pub identity: serde_json::Value,
    pub license_expression: String,
    pub selected_branch: String,
    pub documentation_license: Option<String>,
    pub evidence_sha256: String,
    pub provenance: serde_json::Value,
    pub use_context: String,
    pub commercial_use: bool,
    pub payment_required: bool,
    pub obligations: serde_json::Value,
    pub decision: String,
    pub revision: u64,
    pub created_at: String,
    pub revoked_at: Option<String>,
    pub revocation_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewLicenseDecision<'a> {
    pub delegation_id: &'a str,
    pub project_id: &'a str,
    pub lead_request_id: &'a str,
    pub lead_reservation_id: &'a str,
    pub lead_actor_sha256: &'a str,
    pub identity: &'a serde_json::Value,
    pub license_expression: &'a str,
    pub selected_branch: &'a str,
    pub documentation_license: Option<&'a str>,
    pub evidence_sha256: &'a str,
    pub provenance: &'a serde_json::Value,
    pub use_context: &'a str,
    pub commercial_use: bool,
    pub payment_required: bool,
    pub obligations: &'a serde_json::Value,
    pub decision: &'a str,
    pub revision: u64,
    pub idempotency_key: &'a str,
}

fn decision_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<LicenseDecisionRecord, Error> {
    Ok(LicenseDecisionRecord {
        id: row.try_get("id")?,
        delegation_id: row.try_get("delegation_id")?,
        project_id: row.try_get("project_id")?,
        lead_request_id: row.try_get("lead_request_id")?,
        lead_reservation_id: row.try_get("lead_reservation_id")?,
        lead_actor_sha256: row.try_get("lead_actor_sha256")?,
        identity: serde_json::from_str(&row.try_get::<String, _>("identity_json")?)
            .map_err(|_| Error::Invalid("licence decision identity"))?,
        license_expression: row.try_get("license_expression")?,
        selected_branch: row.try_get("selected_branch")?,
        documentation_license: row.try_get("documentation_license")?,
        evidence_sha256: row.try_get("evidence_sha256")?,
        provenance: serde_json::from_str(&row.try_get::<String, _>("provenance_json")?)
            .map_err(|_| Error::Invalid("licence decision provenance"))?,
        use_context: row.try_get("use_context")?,
        commercial_use: row.try_get("commercial_use")?,
        payment_required: row.try_get("payment_required")?,
        obligations: serde_json::from_str(&row.try_get::<String, _>("obligations_json")?)
            .map_err(|_| Error::Invalid("licence decision obligations"))?,
        decision: row.try_get("decision")?,
        revision: row.try_get::<i64, _>("revision")? as u64,
        created_at: row.try_get("created_at")?,
        revoked_at: row.try_get("revoked_at")?,
        revocation_reason: row.try_get("revocation_reason")?,
    })
}

impl Database {
    pub async fn create_license_decision(
        &self,
        input: &NewLicenseDecision<'_>,
    ) -> Result<LicenseDecisionRecord, Error> {
        if input.delegation_id.trim().is_empty()
            || input.project_id.trim().is_empty()
            || input.lead_request_id.trim().is_empty()
            || input.lead_reservation_id.trim().is_empty()
            || input.lead_actor_sha256.len() != 64
            || input.license_expression.trim().is_empty()
            || input.selected_branch.trim().is_empty()
            || input.evidence_sha256.len() != 64
            || input.use_context.trim().is_empty()
            || input.revision == 0
            || input.idempotency_key.trim().is_empty()
            || !matches!(input.decision, "ACCEPT" | "NEEDS_EVIDENCE" | "DENY")
            || (input.decision == "ACCEPT" && (input.payment_required || !input.commercial_use))
        {
            return Err(Error::Invalid("licence decision fields"));
        }
        let identity_json = serde_json::to_string(input.identity)
            .map_err(|_| Error::Invalid("licence decision identity encoding"))?;
        let provenance_json = serde_json::to_string(input.provenance)
            .map_err(|_| Error::Invalid("licence decision provenance encoding"))?;
        let obligations_json = serde_json::to_string(input.obligations)
            .map_err(|_| Error::Invalid("licence decision obligations encoding"))?;
        let now = Utc::now().to_rfc3339();
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        if let Some(existing) = sqlx::query::query(
            "SELECT id,delegation_id,project_id,lead_request_id,lead_reservation_id,
                    lead_actor_sha256,identity_json,license_expression,selected_branch,documentation_license,
                    evidence_sha256,provenance_json,use_context,commercial_use,payment_required,
                    obligations_json,decision,revision,created_at,revoked_at,revocation_reason
             FROM license_decisions WHERE idempotency_key=?1",
        )
        .bind(input.idempotency_key)
        .fetch_optional(&mut *transaction)
        .await?
        {
            let existing = decision_from_row(&existing)?;
            if existing.delegation_id != input.delegation_id
                || existing.lead_actor_sha256 != input.lead_actor_sha256
                || existing.evidence_sha256 != input.evidence_sha256
                || existing.license_expression != input.license_expression
                || existing.use_context != input.use_context
                || existing.decision != input.decision
            {
                return Err(Error::Conflict(
                    "licence decision idempotency key was used for different evidence",
                ));
            }
            transaction.commit().await?;
            return Ok(existing);
        }
        let id = Uuid::new_v4().to_string();
        let result = sqlx::query::query(
            "INSERT INTO license_decisions
             (id,delegation_id,project_id,lead_request_id,lead_reservation_id,lead_actor_sha256,
              identity_json,license_expression,selected_branch,documentation_license,evidence_sha256,
              provenance_json,use_context,commercial_use,payment_required,obligations_json,
              decision,revision,idempotency_key,created_at,revoked_at,revocation_reason)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,NULL,NULL)",
        )
        .bind(&id)
        .bind(input.delegation_id)
        .bind(input.project_id)
        .bind(input.lead_request_id)
        .bind(input.lead_reservation_id)
        .bind(input.lead_actor_sha256)
        .bind(&identity_json)
        .bind(input.license_expression)
        .bind(input.selected_branch)
        .bind(input.documentation_license)
        .bind(input.evidence_sha256)
        .bind(&provenance_json)
        .bind(input.use_context)
        .bind(input.commercial_use)
        .bind(input.payment_required)
        .bind(&obligations_json)
        .bind(input.decision)
        .bind(input.revision as i64)
        .bind(input.idempotency_key)
        .bind(&now)
        .execute(&mut *transaction)
        .await;
        if let Err(error) = result {
            if error
                .as_database_error()
                .is_some_and(|error| error.is_unique_violation())
            {
                transaction.rollback().await?;
                let existing = sqlx::query::query(
                    "SELECT id,delegation_id,project_id,lead_request_id,lead_reservation_id,
                            lead_actor_sha256,identity_json,license_expression,selected_branch,documentation_license,
                            evidence_sha256,provenance_json,use_context,commercial_use,payment_required,
                            obligations_json,decision,revision,created_at,revoked_at,revocation_reason
                     FROM license_decisions WHERE idempotency_key=?1",
                )
                .bind(input.idempotency_key)
                .fetch_one(&self.pool)
                .await?;
                let existing = decision_from_row(&existing)?;
                if existing.delegation_id == input.delegation_id
                    && existing.lead_actor_sha256 == input.lead_actor_sha256
                    && existing.evidence_sha256 == input.evidence_sha256
                    && existing.license_expression == input.license_expression
                    && existing.use_context == input.use_context
                    && existing.decision == input.decision
                {
                    return Ok(existing);
                }
                return Err(Error::Conflict(
                    "licence decision idempotency key was used for different evidence",
                ));
            }
            return Err(error.into());
        }
        let row = sqlx::query::query(
            "SELECT id,delegation_id,project_id,lead_request_id,lead_reservation_id,
                    lead_actor_sha256,identity_json,license_expression,selected_branch,documentation_license,
                    evidence_sha256,provenance_json,use_context,commercial_use,payment_required,
                    obligations_json,decision,revision,created_at,revoked_at,revocation_reason
             FROM license_decisions WHERE id=?1",
        )
        .bind(&id)
        .fetch_one(&mut *transaction)
        .await?;
        let record = decision_from_row(&row)?;
        transaction.commit().await?;
        Ok(record)
    }

    pub async fn license_decision(&self, id: &str) -> Result<LicenseDecisionRecord, Error> {
        let row = sqlx::query::query(
            "SELECT id,delegation_id,project_id,lead_request_id,lead_reservation_id,
                    lead_actor_sha256,identity_json,license_expression,selected_branch,documentation_license,
                    evidence_sha256,provenance_json,use_context,commercial_use,payment_required,
                    obligations_json,decision,revision,created_at,revoked_at,revocation_reason
             FROM license_decisions WHERE id=?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        decision_from_row(&row)
    }

    pub async fn license_decisions(
        &self,
        project_id: &str,
    ) -> Result<Vec<LicenseDecisionRecord>, Error> {
        let rows = sqlx::query::query(
            "SELECT id,delegation_id,project_id,lead_request_id,lead_reservation_id,
                    lead_actor_sha256,identity_json,license_expression,selected_branch,documentation_license,
                    evidence_sha256,provenance_json,use_context,commercial_use,payment_required,
                    obligations_json,decision,revision,created_at,revoked_at,revocation_reason
             FROM license_decisions WHERE project_id=?1 ORDER BY created_at DESC,id DESC",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(decision_from_row).collect()
    }

    pub async fn revoke_license_decision(
        &self,
        id: &str,
        reason: &str,
    ) -> Result<LicenseDecisionRecord, Error> {
        if reason.trim().is_empty() {
            return Err(Error::Invalid("licence decision revocation reason"));
        }
        let _write = self.write_lock.lock().await;
        let result = sqlx::query::query(
            "UPDATE license_decisions SET revoked_at=?1,revocation_reason=?2
             WHERE id=?3 AND revoked_at IS NULL",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(reason)
        .bind(id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            let existing = self.license_decision(id).await?;
            if existing.revoked_at.is_some() {
                return Ok(existing);
            }
            return Err(Error::Conflict("licence decision revocation race"));
        }
        self.license_decision(id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn fixture() -> (std::path::PathBuf, Database) {
        let path = std::env::temp_dir().join(format!(
            "factory-license-decisions-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let db = Database::open(&path).await.unwrap();
        sqlx::query::query(
            "INSERT INTO projects(id,repository,base_commit,created_at,identity)
             VALUES ('project','/fixture',?1,?2,'')",
        )
        .bind("b".repeat(40))
        .bind(Utc::now().to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();
        (path, db)
    }

    fn new_decision<'a>(
        delegation_id: &'a str,
        lead_actor_sha256: &'a str,
        identity: &'a serde_json::Value,
        evidence_sha256: &'a str,
        provenance: &'a serde_json::Value,
        obligations: &'a serde_json::Value,
        license_expression: &'a str,
        idempotency_key: &'a str,
    ) -> NewLicenseDecision<'a> {
        NewLicenseDecision {
            delegation_id,
            project_id: "project",
            lead_request_id: "lead-1",
            lead_reservation_id: "reservation-1",
            lead_actor_sha256,
            identity,
            license_expression,
            selected_branch: "MIT",
            documentation_license: None,
            evidence_sha256,
            provenance,
            use_context: "commercial-development",
            commercial_use: true,
            payment_required: false,
            obligations,
            decision: "ACCEPT",
            revision: 1,
            idempotency_key,
        }
    }

    #[tokio::test]
    async fn decisions_are_idempotent_revocable_and_survive_restart() {
        let (path, db) = fixture().await;
        let owner = db
            .create_agentic_owner_decision(&NewAgenticOwnerDecision {
                project_id: "project",
                request_id: None,
                campaign_id: None,
                matter: "LICENSE_IDENTITY_EVIDENCE_DELEGATION",
                risk: crate::lead::RiskClass::R0,
                options: &["APPROVE_DELEGATION".into(), "DENY".into()],
                evidence: &serde_json::json!({
                    "domain":"licenses-identity-evidence",
                    "pin":"pin",
                    "useContext":"commercial-development"
                }),
                recommendation: "Approve the exact delegated licence decision.",
                deadline: &(Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
            })
            .await
            .unwrap();
        db.resolve_agentic_owner_decision(
            &owner.id,
            "APPROVE_DELEGATION",
            "Owner delegated the exact decision.",
            "operator",
        )
        .await
        .unwrap();
        let actor = "a".repeat(64);
        let evidence = "b".repeat(64);
        let identity = serde_json::json!({"pin":"pin","storePath":"/nix/store/x"});
        let provenance = serde_json::json!({"source":"fixture"});
        let obligations = serde_json::json!(["notice"]);
        let first = db
            .create_license_decision(&new_decision(
                &owner.id,
                &actor,
                &identity,
                &evidence,
                &provenance,
                &obligations,
                "MIT",
                "decision-1",
            ))
            .await
            .unwrap();
        let replay = db
            .create_license_decision(&new_decision(
                &owner.id,
                &actor,
                &identity,
                &evidence,
                &provenance,
                &obligations,
                "MIT",
                "decision-1",
            ))
            .await
            .unwrap();
        assert_eq!(first, replay);
        let changed = new_decision(
            &owner.id,
            &actor,
            &identity,
            &evidence,
            &provenance,
            &obligations,
            "Apache-2.0",
            "decision-1",
        );
        assert!(matches!(
            db.create_license_decision(&changed).await,
            Err(Error::Conflict(_))
        ));
        let revoked = db
            .revoke_license_decision(&first.id, "superseded")
            .await
            .unwrap();
        assert!(revoked.revoked_at.is_some());
        assert_eq!(db.license_decisions("project").await.unwrap().len(), 1);
        db.close().await;
        let reopened = Database::open(&path).await.unwrap();
        let durable = reopened.license_decision(&first.id).await.unwrap();
        assert_eq!(durable.revoked_at, revoked.revoked_at);
        reopened.close().await;
        let _ = std::fs::remove_file(path);
    }
}
