//! Durable exact Lead licence decisions under an Owner delegation.
//!
//! The Owner delegation is an `agentic_owner_decisions` row. This table records
//! the exact Lead decision bound to its verified actor, identity, evidence,
//! identity-facts receipt, environment stage and revision. Decisions are
//! immutable except for one append-only, actor/project-bound revocation.

use super::*;
use crate::planning::license_decision::{
    CandidateBinding, DocumentationDecision, VerifiedLicenseDecision,
};
use chrono::Utc;
use sqlx::row::Row;
use uuid::Uuid;

pub const LICENSE_DELEGATION_MATTER: &str = "LICENSE_IDENTITY_EVIDENCE_DELEGATION";
pub const LICENSE_DELEGATION_OPTION: &str = "APPROVE_DELEGATION";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseDecisionRecord {
    pub id: String,
    pub delegation_id: String,
    pub project_id: String,
    pub lead_request_id: String,
    pub lead_reservation_id: String,
    pub lead_actor_sha256: String,
    pub lead_request_sha256: String,
    pub lead_output_sha256: String,
    pub lead_principal: String,
    pub delegation_matter: String,
    pub delegation_option: String,
    pub delegation_option_sha256: String,
    pub delegation_revision: u64,
    pub delegation_pin: String,
    pub delegation_use_context: String,
    pub delegation_criterion: String,
    pub delegation_deadline: String,
    pub identity: serde_json::Value,
    pub license_expression: String,
    pub selected_branch: String,
    pub software_obligations: Vec<String>,
    pub documentation: Vec<DocumentationDecision>,
    pub documentation_obligations: Vec<String>,
    pub software_verdict: String,
    pub evidence_sha256: String,
    pub identity_facts_receipt_digest: String,
    pub content_sha256: String,
    pub provenance: serde_json::Value,
    pub use_context: String,
    pub commercial_use: bool,
    pub payment_required: bool,
    pub catalog_sha256: String,
    pub profile_sha256: String,
    pub policy_sha256: String,
    pub stage: serde_json::Value,
    pub plan_sha256: String,
    pub plan_revision: u32,
    pub spec_sha256: Option<String>,
    pub inventory_sha256: String,
    pub evidence_bundle_sha256: String,
    pub candidate: serde_json::Value,
    pub decision: String,
    pub revision: u64,
    pub status: String,
    pub created_at: String,
    pub revoked_at: Option<String>,
    pub revocation_reason: Option<String>,
    pub revoked_by: Option<String>,
}

const COLUMNS: &str = "id,delegation_id,project_id,lead_request_id,lead_reservation_id,\
     lead_actor_sha256,lead_request_sha256,lead_output_sha256,lead_principal,\
     delegation_matter,delegation_option,delegation_option_sha256,delegation_revision,delegation_pin,\
     delegation_use_context,delegation_criterion,delegation_deadline,identity_json,\
     license_expression,selected_branch,software_obligations_json,documentation_json,\
     documentation_obligations_json,software_verdict,evidence_sha256,\
     identity_facts_receipt_digest,content_sha256,provenance_json,use_context,\
     commercial_use,payment_required,catalog_sha256,profile_sha256,policy_sha256,\
     stage_json,decision,revision,created_at,revoked_at,revocation_reason,revoked_by";

fn json_value<T: serde::Serialize>(
    value: &T,
    label: &'static str,
) -> Result<serde_json::Value, Error> {
    serde_json::to_value(value).map_err(|_| Error::Invalid(label))
}

type StageFields = (
    String,
    u32,
    Option<String>,
    String,
    String,
    serde_json::Value,
);

fn stage_fields(stage: &serde_json::Value) -> Result<StageFields, Error> {
    let plan_sha256 = stage
        .get("planSha256")
        .and_then(serde_json::Value::as_str)
        .ok_or(Error::Invalid("licence stage plan"))?
        .to_owned();
    let plan_revision = stage
        .get("planRevision")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(Error::Invalid("licence stage revision"))?;
    let spec_sha256 = stage
        .get("specSha256")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let inventory_sha256 = stage
        .get("inventorySha256")
        .and_then(serde_json::Value::as_str)
        .ok_or(Error::Invalid("licence stage inventory"))?
        .to_owned();
    let evidence_bundle_sha256 = stage
        .get("evidenceBundleSha256")
        .and_then(serde_json::Value::as_str)
        .ok_or(Error::Invalid("licence stage evidence"))?
        .to_owned();
    let candidate = stage
        .get("candidate")
        .cloned()
        .ok_or(Error::Invalid("licence stage candidate"))?;
    Ok((
        plan_sha256,
        plan_revision,
        spec_sha256,
        inventory_sha256,
        evidence_bundle_sha256,
        candidate,
    ))
}

fn decision_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<LicenseDecisionRecord, Error> {
    let stage_json: String = row.try_get("stage_json")?;
    let stage: serde_json::Value =
        serde_json::from_str(&stage_json).map_err(|_| Error::Invalid("licence decision stage"))?;
    let (
        plan_sha256,
        plan_revision,
        spec_sha256,
        inventory_sha256,
        evidence_bundle_sha256,
        candidate,
    ) = stage_fields(&stage)?;
    let revoked_at: Option<String> = row.try_get("revoked_at")?;
    Ok(LicenseDecisionRecord {
        id: row.try_get("id")?,
        delegation_id: row.try_get("delegation_id")?,
        project_id: row.try_get("project_id")?,
        lead_request_id: row.try_get("lead_request_id")?,
        lead_reservation_id: row.try_get("lead_reservation_id")?,
        lead_actor_sha256: row.try_get("lead_actor_sha256")?,
        lead_request_sha256: row.try_get("lead_request_sha256")?,
        lead_output_sha256: row.try_get("lead_output_sha256")?,
        lead_principal: row.try_get("lead_principal")?,
        delegation_matter: row.try_get("delegation_matter")?,
        delegation_option: row.try_get("delegation_option")?,
        delegation_option_sha256: row.try_get("delegation_option_sha256")?,
        delegation_revision: row.try_get::<i64, _>("delegation_revision")? as u64,
        delegation_pin: row.try_get("delegation_pin")?,
        delegation_use_context: row.try_get("delegation_use_context")?,
        delegation_criterion: row.try_get("delegation_criterion")?,
        delegation_deadline: row.try_get("delegation_deadline")?,
        identity: serde_json::from_str(&row.try_get::<String, _>("identity_json")?)
            .map_err(|_| Error::Invalid("licence decision identity"))?,
        license_expression: row.try_get("license_expression")?,
        selected_branch: row.try_get("selected_branch")?,
        software_obligations: serde_json::from_str(
            &row.try_get::<String, _>("software_obligations_json")?,
        )
        .map_err(|_| Error::Invalid("licence decision software obligations"))?,
        documentation: serde_json::from_str(&row.try_get::<String, _>("documentation_json")?)
            .map_err(|_| Error::Invalid("licence decision documentation"))?,
        documentation_obligations: serde_json::from_str(
            &row.try_get::<String, _>("documentation_obligations_json")?,
        )
        .map_err(|_| Error::Invalid("licence decision documentation obligations"))?,
        software_verdict: row.try_get("software_verdict")?,
        evidence_sha256: row.try_get("evidence_sha256")?,
        identity_facts_receipt_digest: row.try_get("identity_facts_receipt_digest")?,
        content_sha256: row.try_get("content_sha256")?,
        provenance: serde_json::from_str(&row.try_get::<String, _>("provenance_json")?)
            .map_err(|_| Error::Invalid("licence decision provenance"))?,
        use_context: row.try_get("use_context")?,
        commercial_use: row.try_get("commercial_use")?,
        payment_required: row.try_get("payment_required")?,
        catalog_sha256: row.try_get("catalog_sha256")?,
        profile_sha256: row.try_get("profile_sha256")?,
        policy_sha256: row.try_get("policy_sha256")?,
        stage,
        plan_sha256,
        plan_revision,
        spec_sha256,
        inventory_sha256,
        evidence_bundle_sha256,
        candidate,
        decision: row.try_get("decision")?,
        revision: row.try_get::<i64, _>("revision")? as u64,
        status: if revoked_at.is_some() {
            "REVOKED".into()
        } else {
            "ACTIVE".into()
        },
        created_at: row.try_get("created_at")?,
        revoked_at,
        revocation_reason: row.try_get("revocation_reason")?,
        revoked_by: row.try_get("revoked_by")?,
    })
}

impl Database {
    /// Persist one already-verified Lead decision.
    ///
    /// The delegation, reservation and revocation state are re-read inside the
    /// same transaction so a decision cannot be written after a delegation is
    /// revoked, expires or moves project between verification and persistence.
    pub async fn create_license_decision(
        &self,
        decision: &VerifiedLicenseDecision,
        idempotency_key: &str,
    ) -> Result<LicenseDecisionRecord, Error> {
        if idempotency_key.trim().is_empty() || idempotency_key.len() > 200 {
            return Err(Error::Invalid("licence decision idempotency key"));
        }
        if decision.project_id().trim().is_empty()
            || decision.delegation_id().trim().is_empty()
            || decision.lead_request_id().trim().is_empty()
            || decision.lead_reservation_id().trim().is_empty()
            || decision.lead_principal().trim().is_empty()
            || decision.lead_actor_sha256().len() != 64
            || decision.lead_request_sha256().len() != 64
            || decision.lead_output_sha256().len() != 64
            || decision.delegation_option_sha256().len() != 64
            || decision.evidence_sha256().len() != 64
            || decision.identity_facts_receipt_digest().len() != 64
            || decision.content_sha256().len() != 64
            || decision.revision() == 0
            || !matches!(decision.decision(), "ACCEPT" | "NEEDS_EVIDENCE" | "DENY")
        {
            return Err(Error::Invalid("licence decision fields"));
        }
        let identity_json = serde_json::to_string(decision.identity())
            .map_err(|_| Error::Invalid("licence decision identity encoding"))?;
        let software_obligations = json_value(
            &decision.software_obligations().to_vec(),
            "licence decision software obligations",
        )?;
        let documentation = json_value(
            &decision.documentation().to_vec(),
            "licence decision documentation",
        )?;
        let documentation_obligations = json_value(
            &decision.documentation_obligations().to_vec(),
            "licence decision documentation obligations",
        )?;
        let provenance = serde_json::to_string(decision.provenance())
            .map_err(|_| Error::Invalid("licence decision provenance encoding"))?;
        let stage = serde_json::to_value(decision.binding().stage())
            .map_err(|_| Error::Invalid("licence decision stage encoding"))?;
        let stage_json = serde_json::to_string(&stage)
            .map_err(|_| Error::Invalid("licence decision stage encoding"))?;
        let now = Utc::now().to_rfc3339();

        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;

        let delegation = sqlx::query::query(
            "SELECT project_id,matter,status,selected_option,options_json,evidence_json,deadline
             FROM agentic_owner_decisions WHERE id=?1",
        )
        .bind(decision.delegation_id())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(Error::NotFound)?;
        let delegation_project: String = delegation.try_get("project_id")?;
        let delegation_matter: String = delegation.try_get("matter")?;
        let delegation_status: String = delegation.try_get("status")?;
        let delegation_option: Option<String> = delegation.try_get("selected_option")?;
        let delegation_options: Vec<String> =
            serde_json::from_str(&delegation.try_get::<String, _>("options_json")?)
                .map_err(|_| Error::Invalid("licence delegation options"))?;
        let delegation_deadline: String = delegation.try_get("deadline")?;
        let delegation_evidence: serde_json::Value =
            serde_json::from_str(&delegation.try_get::<String, _>("evidence_json")?)
                .map_err(|_| Error::Invalid("licence delegation evidence"))?;
        let evidence_revision = delegation_evidence
            .get("revision")
            .and_then(serde_json::Value::as_u64);
        let evidence_pin = delegation_evidence
            .get("pin")
            .and_then(serde_json::Value::as_str);
        let evidence_use_context = delegation_evidence
            .get("useContext")
            .and_then(serde_json::Value::as_str);
        let evidence_criterion = delegation_evidence
            .get("economicCriterion")
            .and_then(serde_json::Value::as_str);
        let evidence_domain = delegation_evidence
            .get("domain")
            .and_then(serde_json::Value::as_str);
        let expected_option_sha256 =
            crate::planning::license_decision::canonical_json_sha256(&serde_json::json!({
                "matter": delegation_matter,
                "options": delegation_options,
                "selectedOption": delegation_option,
            }));
        if delegation_project != decision.project_id()
            || delegation_matter != LICENSE_DELEGATION_MATTER
            || delegation_matter != decision.delegation_matter()
            || delegation_status != "APPROVED"
            || delegation_option.as_deref() != Some(LICENSE_DELEGATION_OPTION)
            || delegation_option.as_deref() != Some(decision.delegation_option())
            || !delegation_options
                .iter()
                .any(|option| option == LICENSE_DELEGATION_OPTION)
            || delegation_deadline != decision.delegation_deadline()
            || expected_option_sha256 != decision.delegation_option_sha256()
            || evidence_domain != Some("licenses-identity-evidence")
            || evidence_pin != Some(decision.delegation_pin())
            || evidence_use_context != Some(decision.delegation_use_context())
            || evidence_criterion != Some(decision.delegation_criterion())
            || evidence_revision != Some(decision.delegation_revision())
        {
            transaction.rollback().await?;
            return Err(Error::Conflict(
                "licence delegation changed before persistence",
            ));
        }
        let deadline = chrono::DateTime::parse_from_rfc3339(&delegation_deadline)
            .map_err(|_| Error::Invalid("licence delegation deadline"))?;
        if deadline <= Utc::now() {
            transaction.rollback().await?;
            return Err(Error::Conflict("licence delegation expired"));
        }
        let revoked: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT delegation_id FROM license_delegation_revocations WHERE delegation_id=?1",
        )
        .bind(decision.delegation_id())
        .fetch_optional(&mut *transaction)
        .await?;
        if revoked.is_some() {
            transaction.rollback().await?;
            return Err(Error::Conflict("licence delegation is revoked"));
        }

        let reservation = sqlx::query::query(
            "SELECT role,purpose,state,project_id,objective_id,request_sha256,principal_id,
                    COALESCE(response_text,'') AS response_text,expires_at_ms
             FROM model_call_reservations WHERE reservation_id=?1",
        )
        .bind(decision.lead_reservation_id())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(Error::NotFound)?;
        let role: String = reservation.try_get("role")?;
        let purpose: String = reservation.try_get("purpose")?;
        let state: String = reservation.try_get("state")?;
        let reservation_project: String = reservation.try_get("project_id")?;
        let objective_id: String = reservation.try_get("objective_id")?;
        let request_sha256: String = reservation.try_get("request_sha256")?;
        let principal_id: String = reservation.try_get("principal_id")?;
        let response_text: String = reservation.try_get("response_text")?;
        let expires_at_ms: i64 = reservation.try_get("expires_at_ms")?;
        let output_sha256 = license_decisions_sha256(response_text.as_bytes());
        let withdrawn: Option<(String,)> =
            sqlx::query_as::query_as("SELECT request_id FROM lead_withdrawals WHERE request_id=?1")
                .bind(decision.lead_request_id())
                .fetch_optional(&mut *transaction)
                .await?;
        let lead_state: Option<(String,)> =
            sqlx::query_as::query_as("SELECT state FROM lead_requests WHERE id=?1")
                .bind(decision.lead_request_id())
                .fetch_optional(&mut *transaction)
                .await?;
        let lead_state = lead_state.map(|(state,)| state);
        if role != "LEAD"
            || !matches!(
                lead_state.as_deref(),
                Some("SUBMITTED" | "ASSESSING" | "PLANNED" | "FROZEN" | "READY" | "RUNNING")
            )
            || purpose != "license_evidence_review"
            || state != "SETTLED"
            || withdrawn.is_some()
            || reservation_project != decision.project_id()
            || objective_id != decision.lead_request_id()
            || request_sha256 != decision.lead_request_sha256()
            || principal_id != decision.lead_principal()
            || output_sha256 != decision.lead_output_sha256()
            || expires_at_ms <= Utc::now().timestamp_millis()
        {
            transaction.rollback().await?;
            return Err(Error::Conflict(
                "licence evidence reservation changed before persistence",
            ));
        }

        let plan = sqlx::query::query(
            "SELECT p.project_id,p.action_id,p.revision,p.base_commit,p.license_inventory_sha256,p.status
             FROM environment_plans p
             JOIN lead_requests l ON l.action_id=p.action_id
             WHERE p.plan_sha256=?1 AND l.id=?2",
        )
        .bind(decision.binding().stage().plan_sha256())
        .bind(decision.lead_request_id())
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| Error::Conflict("licence stage plan is not the exact Lead request plan"))?;
        let plan_project: String = plan.try_get("project_id")?;
        let plan_revision: i64 = plan.try_get("revision")?;
        let plan_base: String = plan.try_get("base_commit")?;
        let plan_inventory: String = plan.try_get("license_inventory_sha256")?;
        let plan_status: String = plan.try_get("status")?;
        if plan_project != decision.project_id()
            || plan_revision != decision.binding().stage().plan_revision() as i64
            || plan_inventory != decision.binding().stage().inventory_sha256()
            || !matches!(
                plan_status.as_str(),
                "CORE_VALIDATED" | "LEAD_APPROVED" | "FROZEN"
            )
        {
            transaction.rollback().await?;
            return Err(Error::Conflict(
                "licence stage plan changed before persistence",
            ));
        }
        if let CandidateBinding::Base { base_commit } = decision.binding().candidate()
            && base_commit != &plan_base
        {
            transaction.rollback().await?;
            return Err(Error::Conflict(
                "licence stage candidate base differs from the plan",
            ));
        }
        if let Some(spec_sha256) = decision.binding().stage().spec_sha256() {
            let spec = sqlx::query::query(
                "SELECT project_id,plan_sha256,base_commit,license_inventory_sha256,status
                 FROM environment_specs WHERE spec_sha256=?1",
            )
            .bind(spec_sha256)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or_else(|| Error::Conflict("licence stage spec is not persisted"))?;
            let spec_project: String = spec.try_get("project_id")?;
            let spec_plan: String = spec.try_get("plan_sha256")?;
            let spec_base: String = spec.try_get("base_commit")?;
            let spec_inventory: String = spec.try_get("license_inventory_sha256")?;
            let spec_status: String = spec.try_get("status")?;
            if spec_project != decision.project_id()
                || spec_plan != decision.binding().stage().plan_sha256()
                || spec_base != plan_base
                || spec_inventory != decision.binding().stage().inventory_sha256()
                || spec_status != "FROZEN"
            {
                transaction.rollback().await?;
                return Err(Error::Conflict(
                    "licence stage spec changed before persistence",
                ));
            }
        }

        if let Some(existing) = sqlx::query::query(&format!(
            "SELECT {COLUMNS} FROM license_decisions WHERE idempotency_key=?1"
        ))
        .bind(idempotency_key)
        .fetch_optional(&mut *transaction)
        .await?
        {
            let existing = decision_from_row(&existing)?;
            transaction.commit().await?;
            if existing.content_sha256 == decision.content_sha256()
                && existing.project_id == decision.project_id()
                && existing.delegation_id == decision.delegation_id()
            {
                return Ok(existing);
            }
            return Err(Error::Conflict(
                "licence decision idempotency key was used for different evidence",
            ));
        }

        if let Some(existing) = sqlx::query::query(&format!(
            "SELECT {COLUMNS} FROM license_decisions
             WHERE project_id=?1 AND delegation_id=?2 AND lead_request_id=?3 AND content_sha256=?4
             LIMIT 1"
        ))
        .bind(decision.project_id())
        .bind(decision.delegation_id())
        .bind(decision.lead_request_id())
        .bind(decision.content_sha256())
        .fetch_optional(&mut *transaction)
        .await?
        {
            let existing = decision_from_row(&existing)?;
            transaction.commit().await?;
            return Ok(existing);
        }

        let id = Uuid::new_v4().to_string();
        let result = sqlx::query::query(
            "INSERT INTO license_decisions
             (id,delegation_id,project_id,lead_request_id,lead_reservation_id,lead_actor_sha256,
              lead_request_sha256,lead_output_sha256,lead_principal,delegation_matter,
              delegation_option,delegation_option_sha256,delegation_revision,delegation_pin,delegation_use_context,
              delegation_criterion,delegation_deadline,identity_json,license_expression,
              selected_branch,software_obligations_json,documentation_json,
              documentation_obligations_json,software_verdict,evidence_sha256,
              identity_facts_receipt_digest,content_sha256,provenance_json,use_context,
              commercial_use,payment_required,catalog_sha256,profile_sha256,policy_sha256,
              stage_json,decision,revision,idempotency_key,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,
                     ?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34,?35,?36,?37,?38,?39)",
        )
        .bind(&id)
        .bind(decision.delegation_id())
        .bind(decision.project_id())
        .bind(decision.lead_request_id())
        .bind(decision.lead_reservation_id())
        .bind(decision.lead_actor_sha256())
        .bind(decision.lead_request_sha256())
        .bind(decision.lead_output_sha256())
        .bind(decision.lead_principal())
        .bind(decision.delegation_matter())
        .bind(decision.delegation_option())
        .bind(decision.delegation_option_sha256())
        .bind(decision.delegation_revision() as i64)
        .bind(decision.delegation_pin())
        .bind(decision.delegation_use_context())
        .bind(decision.delegation_criterion())
        .bind(decision.delegation_deadline())
        .bind(&identity_json)
        .bind(decision.license_expression())
        .bind(decision.selected_branch())
        .bind(software_obligations.to_string())
        .bind(documentation.to_string())
        .bind(documentation_obligations.to_string())
        .bind(decision.software_verdict())
        .bind(decision.evidence_sha256())
        .bind(decision.identity_facts_receipt_digest())
        .bind(decision.content_sha256())
        .bind(&provenance)
        .bind(decision.use_context())
        .bind(decision.commercial_use())
        .bind(decision.payment_required())
        .bind(decision.binding().catalog_sha256())
        .bind(decision.binding().profile_sha256())
        .bind(decision.binding().policy_sha256())
        .bind(&stage_json)
        .bind(decision.decision())
        .bind(decision.revision() as i64)
        .bind(idempotency_key)
        .bind(&now)
        .execute(&mut *transaction)
        .await;
        if let Err(error) = result {
            if error
                .as_database_error()
                .is_some_and(|error| error.is_unique_violation())
            {
                transaction.rollback().await?;
                let existing = sqlx::query::query(&format!(
                    "SELECT {COLUMNS} FROM license_decisions
                     WHERE idempotency_key=?1
                        OR (project_id=?2 AND delegation_id=?3 AND lead_request_id=?4
                            AND content_sha256=?5)
                     LIMIT 1"
                ))
                .bind(idempotency_key)
                .bind(decision.project_id())
                .bind(decision.delegation_id())
                .bind(decision.lead_request_id())
                .bind(decision.content_sha256())
                .fetch_optional(&self.pool)
                .await?;
                if let Some(existing) = existing {
                    let existing = decision_from_row(&existing)?;
                    if existing.content_sha256 == decision.content_sha256()
                        && existing.project_id == decision.project_id()
                        && existing.delegation_id == decision.delegation_id()
                    {
                        return Ok(existing);
                    }
                }
                return Err(Error::Conflict(
                    "licence decision idempotency key was used for different evidence",
                ));
            }
            return Err(error.into());
        }
        let row = sqlx::query::query(&format!(
            "SELECT {COLUMNS} FROM license_decisions WHERE id=?1"
        ))
        .bind(&id)
        .fetch_one(&mut *transaction)
        .await?;
        let record = decision_from_row(&row)?;
        transaction.commit().await?;
        Ok(record)
    }

    pub async fn license_decision(&self, id: &str) -> Result<LicenseDecisionRecord, Error> {
        let row = sqlx::query::query(&format!(
            "SELECT {COLUMNS} FROM license_decisions WHERE id=?1"
        ))
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
        let rows = sqlx::query::query(&format!(
            "SELECT {COLUMNS} FROM license_decisions
             WHERE project_id=?1 ORDER BY created_at DESC,id DESC"
        ))
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(decision_from_row).collect()
    }

    pub async fn license_delegation_revoked(&self, delegation_id: &str) -> Result<bool, Error> {
        let row = sqlx::query::query(
            "SELECT 1 FROM license_delegation_revocations WHERE delegation_id=?1",
        )
        .bind(delegation_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    /// Append-only, project/actor-bound revocation of the Owner delegation.
    pub async fn revoke_license_delegation(
        &self,
        delegation_id: &str,
        project_id: &str,
        principal: &str,
        reason: &str,
    ) -> Result<(), Error> {
        if delegation_id.trim().is_empty()
            || project_id.trim().is_empty()
            || principal.trim().is_empty()
            || principal.len() > 200
            || reason.trim().is_empty()
            || reason.len() > 500
        {
            return Err(Error::Invalid("licence delegation revocation fields"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let delegation = sqlx::query::query(
            "SELECT project_id,matter,status FROM agentic_owner_decisions WHERE id=?1",
        )
        .bind(delegation_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(Error::NotFound)?;
        let delegation_project: String = delegation.try_get("project_id")?;
        let delegation_matter: String = delegation.try_get("matter")?;
        let delegation_status: String = delegation.try_get("status")?;
        if delegation_project != project_id || delegation_matter != LICENSE_DELEGATION_MATTER {
            transaction.rollback().await?;
            return Err(Error::NotFound);
        }
        if delegation_status != "APPROVED" && delegation_status != "EXPIRED" {
            transaction.rollback().await?;
            return Err(Error::Conflict("licence delegation is not revocable"));
        }
        let existing: Option<(String, String)> = sqlx::query_as::query_as(
            "SELECT revoked_by,reason FROM license_delegation_revocations WHERE delegation_id=?1",
        )
        .bind(delegation_id)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some((existing_by, existing_reason)) = existing {
            // The durable record is bound to the delegation row, whose project
            // was checked above. A second, different revocation is refused.
            transaction.commit().await?;
            if existing_by == principal && existing_reason == reason {
                return Ok(());
            }
            return Err(Error::Conflict(
                "licence delegation was already revoked by another actor or reason",
            ));
        }
        sqlx::query::query(
            "INSERT INTO license_delegation_revocations(delegation_id,reason,revoked_by,created_at)
             VALUES (?1,?2,?3,?4)",
        )
        .bind(delegation_id)
        .bind(reason)
        .bind(principal)
        .bind(Utc::now().to_rfc3339())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// One audited revocation. The decision content is immutable; only the
    /// revocation marker can be set once, and the append-only table records who
    /// revoked it, for which project and why.
    pub async fn revoke_license_decision(
        &self,
        id: &str,
        project_id: &str,
        principal: &str,
        reason: &str,
    ) -> Result<LicenseDecisionRecord, Error> {
        if project_id.trim().is_empty()
            || principal.trim().is_empty()
            || principal.len() > 200
            || reason.trim().is_empty()
            || reason.len() > 500
        {
            return Err(Error::Invalid("licence decision revocation fields"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query::query(&format!(
            "SELECT {COLUMNS} FROM license_decisions WHERE id=?1"
        ))
        .bind(id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(Error::NotFound)?;
        let existing = decision_from_row(&row)?;
        if existing.project_id != project_id {
            transaction.rollback().await?;
            return Err(Error::NotFound);
        }
        if existing.revoked_at.is_some() {
            transaction.commit().await?;
            if existing.revocation_reason.as_deref() == Some(reason)
                && existing.revoked_by.as_deref() == Some(principal)
            {
                return Ok(existing);
            }
            return Err(Error::Conflict(
                "licence decision was already revoked by another actor or reason",
            ));
        }
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO license_decision_revocations(decision_id,project_id,reason,revoked_by,created_at)
             VALUES (?1,?2,?3,?4,?5)",
        )
        .bind(id)
        .bind(project_id)
        .bind(reason)
        .bind(principal)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        let result = sqlx::query::query(
            "UPDATE license_decisions
             SET revoked_at=?1,revocation_reason=?2,revoked_by=?3
             WHERE id=?4 AND revoked_at IS NULL",
        )
        .bind(&now)
        .bind(reason)
        .bind(principal)
        .bind(id)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() == 0 {
            transaction.rollback().await?;
            return Err(Error::Conflict("licence decision revocation race"));
        }
        let row = sqlx::query::query(&format!(
            "SELECT {COLUMNS} FROM license_decisions WHERE id=?1"
        ))
        .bind(id)
        .fetch_one(&mut *transaction)
        .await?;
        let record = decision_from_row(&row)?;
        transaction.commit().await?;
        Ok(record)
    }
}

fn license_decisions_sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests;
