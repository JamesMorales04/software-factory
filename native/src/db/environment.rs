//! Durable environment plans, Lead reviews, frozen specs, license inventories,
//! build receipts and Campaign bindings.
//!
//! Core owns these records. A model may propose a plan, and a Lead may record a
//! planning review, but only deterministic validation and an exact freeze can
//! produce an executable [`EnvironmentSpec`].

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx_sqlite::SqliteConnection;
use uuid::Uuid;

use super::{Database, Error};
use crate::planning::environment::{
    EnvironmentBuildReceipt, EnvironmentPlan, EnvironmentPlanReview, EnvironmentSpec,
    LicenseInventory,
};
use crate::planning::environment::{EnvironmentPlanStatus, EnvironmentSpecStatus};

const MAX_ENVIRONMENT_JSON_BYTES: usize = 4 * 1024 * 1024;

type PlanRow = (
    String,
    String,
    Option<String>,
    i64,
    String,
    String,
    String,
    String,
    String,
);

type SpecRow = (String, String, String, String, String);
type ReviewRow = (
    String,
    String,
    String,
    String,
    i64,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
);
type ModelCallProvenanceRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
);
type CampaignRow = (
    String,
    Option<String>,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredEnvironmentPlan {
    pub row_id: String,
    pub revision: u32,
    pub status: String,
    pub plan_sha256: String,
    pub content_json: String,
    pub created_at: String,
    pub updated_at: String,
    pub plan: EnvironmentPlan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredEnvironmentSpec {
    pub row_id: String,
    pub spec_sha256: String,
    pub content_json: String,
    pub frozen_at: String,
    pub created_at: String,
    pub spec: EnvironmentSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignEnvironmentBinding {
    pub campaign_id: String,
    pub request_id: Option<String>,
    pub action_id: String,
    pub spec_id: String,
    pub spec_sha256: String,
    pub plan_sha256: String,
    pub builder_image: String,
    pub builder_image_digest: String,
    pub bound_at: String,
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn validate_json_size(name: &str, content: &str) -> Result<(), Error> {
    if content.len() > MAX_ENVIRONMENT_JSON_BYTES {
        return Err(Error::Validation(format!("{name} exceeds the byte bound")));
    }
    Ok(())
}

fn decode_plan(
    row_id: String,
    revision: i64,
    status: String,
    plan_sha256: String,
    content_json: String,
    created_at: String,
    updated_at: String,
) -> Result<StoredEnvironmentPlan, Error> {
    let plan: EnvironmentPlan = serde_json::from_str(&content_json)
        .map_err(|_| Error::Validation("stored environment plan is not decodable".into()))?;
    if plan.plan_sha256 != plan_sha256 {
        return Err(Error::Validation(
            "stored environment plan hash does not match its column".into(),
        ));
    }
    Ok(StoredEnvironmentPlan {
        row_id,
        revision: u32::try_from(revision)
            .map_err(|_| Error::Invalid("stored environment plan revision"))?,
        status,
        plan_sha256,
        content_json,
        created_at,
        updated_at,
        plan,
    })
}

fn decode_spec(
    row_id: String,
    spec_sha256: String,
    content_json: String,
    frozen_at: String,
    created_at: String,
) -> Result<StoredEnvironmentSpec, Error> {
    let spec: EnvironmentSpec = serde_json::from_str(&content_json)
        .map_err(|_| Error::Validation("stored environment spec is not decodable".into()))?;
    if spec.spec_sha256 != spec_sha256 {
        return Err(Error::Validation(
            "stored environment spec hash does not match its column".into(),
        ));
    }
    Ok(StoredEnvironmentSpec {
        row_id,
        spec_sha256,
        content_json,
        frozen_at,
        created_at,
        spec,
    })
}

impl Database {
    /// Persist one immutable environment plan revision. Repeating the same
    /// content is idempotent; a different payload for the same request revision
    /// is a conflict.
    pub async fn save_environment_plan(
        &self,
        plan: &EnvironmentPlan,
    ) -> Result<StoredEnvironmentPlan, Error> {
        let content_json =
            serde_json::to_string(plan).map_err(|_| Error::Invalid("environment plan encoding"))?;
        validate_json_size("environment plan", &content_json)?;
        if plan.plan_sha256 != plan.compute_sha256() {
            return Err(Error::Validation(
                "environment plan content does not match its hash".into(),
            ));
        }
        let status = match plan.status {
            EnvironmentPlanStatus::Draft => "DRAFT",
            EnvironmentPlanStatus::CoreValidated => "CORE_VALIDATED",
            EnvironmentPlanStatus::LeadApproved => "LEAD_APPROVED",
            EnvironmentPlanStatus::Frozen => "FROZEN",
            EnvironmentPlanStatus::Rejected => "REJECTED",
            EnvironmentPlanStatus::Blocked => "BLOCKED",
        };
        let source = match plan.source {
            crate::planning::environment::EnvironmentPlanSource::ProjectCommitted => {
                "PROJECT_COMMITTED"
            }
            crate::planning::environment::EnvironmentPlanSource::Generated => "GENERATED",
        };
        let now = Utc::now().to_rfc3339();
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let existing: Option<(String, i64, String, String, String, String)> =
            sqlx::query_as::query_as(
                "SELECT id,revision,status,content_json,created_at,updated_at
                 FROM environment_plans WHERE plan_sha256=?1",
            )
            .bind(&plan.plan_sha256)
            .fetch_optional(&mut *transaction)
            .await?;
        if let Some((row_id, revision, stored_status, stored_json, created_at, updated_at)) =
            existing
        {
            transaction.commit().await?;
            if stored_json != content_json {
                return Err(Error::Conflict(
                    "environment plan hash already has different content",
                ));
            }
            return decode_plan(
                row_id,
                revision,
                stored_status,
                plan.plan_sha256.clone(),
                stored_json,
                created_at,
                updated_at,
            );
        }
        let (request_id,): (Option<String>,) =
            sqlx::query_as::query_as("SELECT id FROM intake_requests WHERE id=?1")
                .bind(&plan.request_id)
                .fetch_optional(&mut *transaction)
                .await?
                .map_or((None,), |(id,)| (Some(id),));
        let duplicate: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT plan_sha256 FROM environment_plans
             WHERE request_id=?1 AND revision=?2",
        )
        .bind(&plan.request_id)
        .bind(i64::from(plan.revision))
        .fetch_optional(&mut *transaction)
        .await?;
        if duplicate.is_some() {
            return Err(Error::Conflict(
                "environment plan revision already has different content",
            ));
        }
        let row_id = Uuid::new_v4().to_string();
        sqlx::query::query(
            "INSERT INTO environment_plans
             (id,plan_id,project_id,request_id,action_id,campaign_id,revision,
              base_commit,risk_class,conventions_sha256,plan_sha256,status,source,
              builder_image,builder_image_digest,license_inventory_sha256,
              sbom_sha256,content_json,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
        )
        .bind(&row_id)
        .bind(&plan.plan_id)
        .bind(&plan.project_id)
        .bind(&request_id)
        .bind(&plan.action_id)
        .bind(&plan.campaign_id)
        .bind(i64::from(plan.revision))
        .bind(&plan.base_commit)
        .bind(&plan.risk_class)
        .bind(&plan.conventions_sha256)
        .bind(&plan.plan_sha256)
        .bind(status)
        .bind(source)
        .bind(&plan.builder.image)
        .bind(&plan.builder.digest)
        .bind(&plan.license_inventory_sha256)
        .bind(&plan.license_inventory.sbom_sha256)
        .bind(&content_json)
        .bind(&now)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        decode_plan(
            row_id,
            i64::from(plan.revision),
            status.into(),
            plan.plan_sha256.clone(),
            content_json,
            now.clone(),
            now,
        )
    }

    pub async fn mark_environment_plan_status(
        &self,
        plan_sha256: &str,
        status: EnvironmentPlanStatus,
    ) -> Result<StoredEnvironmentPlan, Error> {
        let status_name = match status {
            EnvironmentPlanStatus::Draft => "DRAFT",
            EnvironmentPlanStatus::CoreValidated => "CORE_VALIDATED",
            EnvironmentPlanStatus::LeadApproved => "LEAD_APPROVED",
            EnvironmentPlanStatus::Frozen => "FROZEN",
            EnvironmentPlanStatus::Rejected => "REJECTED",
            EnvironmentPlanStatus::Blocked => "BLOCKED",
        };
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let row: Option<(String, i64, String, String, String, String)> = sqlx::query_as::query_as(
            "SELECT id,revision,status,content_json,created_at,updated_at
                 FROM environment_plans WHERE plan_sha256=?1",
        )
        .bind(plan_sha256)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((row_id, revision, stored_status, content_json, created_at, _)) = row else {
            return Err(Error::NotFound);
        };
        if stored_status == "FROZEN" && status != EnvironmentPlanStatus::Frozen {
            return Err(Error::Conflict("a frozen environment plan is immutable"));
        }
        let mut plan: EnvironmentPlan = serde_json::from_str(&content_json)
            .map_err(|_| Error::Validation("stored environment plan is not decodable".into()))?;
        plan.status = status;
        let content_json = serde_json::to_string(&plan)
            .map_err(|_| Error::Invalid("environment plan encoding"))?;
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "UPDATE environment_plans SET status=?1,content_json=?2,updated_at=?3
             WHERE plan_sha256=?4",
        )
        .bind(status_name)
        .bind(&content_json)
        .bind(&now)
        .bind(plan_sha256)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        decode_plan(
            row_id,
            revision,
            status_name.into(),
            plan_sha256.into(),
            content_json,
            created_at,
            now,
        )
    }

    pub async fn environment_plan_by_sha(
        &self,
        plan_sha256: &str,
    ) -> Result<StoredEnvironmentPlan, Error> {
        let row: Option<PlanRow> = sqlx::query_as::query_as(
            "SELECT id,plan_id,request_id,revision,status,plan_sha256,content_json,created_at,updated_at
             FROM environment_plans WHERE plan_sha256=?1",
        )
        .bind(plan_sha256)
        .fetch_optional(&self.pool)
        .await?;
        let Some((
            row_id,
            _plan_id,
            _request_id,
            revision,
            status,
            hash,
            content,
            created,
            updated,
        )) = row
        else {
            return Err(Error::NotFound);
        };
        decode_plan(row_id, revision, status, hash, content, created, updated)
    }

    pub async fn environment_plan(
        &self,
        request_id: &str,
        revision: u32,
    ) -> Result<StoredEnvironmentPlan, Error> {
        let row: Option<PlanRow> = sqlx::query_as::query_as(
            "SELECT id,plan_id,request_id,revision,status,plan_sha256,content_json,created_at,updated_at
             FROM environment_plans WHERE request_id=?1 AND revision=?2",
        )
        .bind(request_id)
        .bind(i64::from(revision))
        .fetch_optional(&self.pool)
        .await?;
        let Some((
            row_id,
            _plan_id,
            _request_id,
            revision,
            status,
            hash,
            content,
            created,
            updated,
        )) = row
        else {
            return Err(Error::NotFound);
        };
        decode_plan(row_id, revision, status, hash, content, created, updated)
    }

    pub async fn latest_environment_plan(
        &self,
        request_id: &str,
    ) -> Result<StoredEnvironmentPlan, Error> {
        let row: Option<PlanRow> = sqlx::query_as::query_as(
            "SELECT id,plan_id,request_id,revision,status,plan_sha256,content_json,created_at,updated_at
             FROM environment_plans WHERE request_id=?1
             ORDER BY revision DESC LIMIT 1",
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((
            row_id,
            _plan_id,
            _request_id,
            revision,
            status,
            hash,
            content,
            created,
            updated,
        )) = row
        else {
            return Err(Error::NotFound);
        };
        decode_plan(row_id, revision, status, hash, content, created, updated)
    }

    pub async fn environment_plans(
        &self,
        request_id: &str,
    ) -> Result<Vec<StoredEnvironmentPlan>, Error> {
        let rows: Vec<PlanRow> = sqlx::query_as::query_as(
            "SELECT id,plan_id,request_id,revision,status,plan_sha256,content_json,created_at,updated_at
             FROM environment_plans WHERE request_id=?1 ORDER BY revision",
        )
        .bind(request_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(
                |(
                    row_id,
                    _plan_id,
                    _request_id,
                    revision,
                    status,
                    hash,
                    content,
                    created,
                    updated,
                )| {
                    decode_plan(row_id, revision, status, hash, content, created, updated)
                },
            )
            .collect()
    }

    pub async fn save_environment_plan_review(
        &self,
        review: &EnvironmentPlanReview,
    ) -> Result<EnvironmentPlanReview, Error> {
        let content_json = serde_json::to_string(&review.evidence_refs)
            .map_err(|_| Error::Invalid("environment review evidence encoding"))?;
        let constraints_json = serde_json::to_string(&review.constraints)
            .map_err(|_| Error::Invalid("environment review constraints encoding"))?;
        let model_call_json = serde_json::to_string(&review.model_call)
            .map_err(|_| Error::Invalid("environment review model call encoding"))?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let plan_row: Option<(String, i64, String)> = sqlx::query_as::query_as(
            "SELECT plan_sha256,revision,content_json FROM environment_plans
             WHERE plan_sha256=?1 AND revision=?2",
        )
        .bind(&review.plan_sha256)
        .bind(i64::from(review.plan_revision))
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((plan_sha256, revision, content)) = plan_row else {
            return Err(Error::NotFound);
        };
        let plan: EnvironmentPlan = serde_json::from_str(&content)
            .map_err(|_| Error::Validation("stored environment plan is not decodable".into()))?;
        review
            .validate_for(&plan, Utc::now())
            .map_err(|error| Error::Validation(error.message))?;
        let call: Option<ModelCallProvenanceRow> = sqlx::query_as::query_as(
            "SELECT role,provider,model,request_sha256,purpose,state,project_id,objective_id
                 FROM model_call_reservations WHERE reservation_id=?1",
        )
        .bind(&review.model_call.reservation_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((role, provider, model, request_sha256, purpose, state, project_id, objective_id)) =
            call
        else {
            return Err(Error::Validation(
                "the Lead environment review has no recorded model call".into(),
            ));
        };
        let expected_lead: Option<(String,)> =
            sqlx::query_as::query_as("SELECT lead_request_id FROM intake_requests WHERE id=?1")
                .bind(&plan.request_id)
                .fetch_optional(&mut *transaction)
                .await?;
        let Some((expected_lead,)) = expected_lead else {
            return Err(Error::Validation(
                "the environment plan has no intake request to bind the Lead model call".into(),
            ));
        };
        if role != "LEAD"
            || purpose != "lead_environment_review"
            || state != "SETTLED"
            || provider != review.model_call.provider
            || model != review.model_call.model
            || request_sha256 != review.model_call.request_sha256
            || project_id != plan.project_id
            || objective_id != expected_lead
        {
            return Err(Error::Validation(
                "the Lead environment review model call provenance does not match its reservation"
                    .into(),
            ));
        }
        if plan_sha256 != review.plan_sha256 || revision != i64::from(review.plan_revision) {
            return Err(Error::Conflict(
                "environment review does not bind the stored plan",
            ));
        }
        let existing: Option<(String, String, String, String, String)> = sqlx::query_as::query_as(
            "SELECT id,decision,rationale,evidence_json,decided_at
                 FROM environment_plan_reviews
                 WHERE plan_sha256=?1 AND principal_id=?2 AND decision=?3 AND decided_at=?4",
        )
        .bind(&review.plan_sha256)
        .bind(&review.principal_id)
        .bind(match review.decision {
            crate::planning::environment::LeadReviewDecision::Approved => "APPROVED",
            crate::planning::environment::LeadReviewDecision::ChangesRequested => {
                "CHANGES_REQUESTED"
            }
            crate::planning::environment::LeadReviewDecision::Rejected => "REJECTED",
        })
        .bind(&review.decided_at)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some((id, _decision, _rationale, stored_evidence, _decided_at)) = existing {
            transaction.commit().await?;
            let mut stored = review.clone();
            stored.review_id = id;
            if stored_evidence != content_json {
                return Err(Error::Conflict(
                    "environment review replay has different evidence",
                ));
            }
            return Ok(stored);
        }
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO environment_plan_reviews
             (id,request_id,action_id,plan_id,plan_sha256,plan_revision,risk_class,
              reviewer_role,principal_id,recorded_by,decision,rationale,
              constraints_json,evidence_json,model_call_json,decided_at,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,'LEAD',?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        )
        .bind(&review.review_id)
        .bind(&review.request_id)
        .bind(&review.action_id)
        .bind(&review.plan_id)
        .bind(&review.plan_sha256)
        .bind(i64::from(review.plan_revision))
        .bind(&review.risk_class)
        .bind(&review.principal_id)
        .bind(&review.recorded_by)
        .bind(match review.decision {
            crate::planning::environment::LeadReviewDecision::Approved => "APPROVED",
            crate::planning::environment::LeadReviewDecision::ChangesRequested => {
                "CHANGES_REQUESTED"
            }
            crate::planning::environment::LeadReviewDecision::Rejected => "REJECTED",
        })
        .bind(&review.rationale)
        .bind(&constraints_json)
        .bind(&content_json)
        .bind(&model_call_json)
        .bind(&review.decided_at)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(review.clone())
    }

    pub async fn latest_environment_plan_review(
        &self,
        plan_sha256: &str,
    ) -> Result<EnvironmentPlanReview, Error> {
        let row: Option<ReviewRow> = sqlx::query_as::query_as(
            "SELECT id,request_id,action_id,plan_id,plan_revision,risk_class,
                        principal_id,recorded_by,decision,rationale,constraints_json,
                        decided_at,evidence_json,model_call_json
                 FROM environment_plan_reviews WHERE plan_sha256=?1
                 ORDER BY decided_at DESC,id DESC LIMIT 1",
        )
        .bind(plan_sha256)
        .fetch_optional(&self.pool)
        .await?;
        let Some((
            id,
            request_id,
            action_id,
            plan_id,
            plan_revision,
            risk_class,
            principal_id,
            recorded_by,
            decision,
            rationale,
            constraints_json,
            decided_at,
            evidence_json,
            model_call_json,
        )) = row
        else {
            return Err(Error::NotFound);
        };
        Ok(EnvironmentPlanReview {
            schema_version: crate::planning::environment::ENVIRONMENT_SCHEMA_VERSION,
            review_id: id,
            request_id,
            action_id,
            plan_id,
            plan_sha256: plan_sha256.into(),
            plan_revision: u32::try_from(plan_revision)
                .map_err(|_| Error::Invalid("stored environment review revision"))?,
            risk_class,
            reviewer_role: crate::planning::environment::EnvironmentReviewerRole::Lead,
            principal_id,
            recorded_by,
            decision: match decision.as_str() {
                "APPROVED" => crate::planning::environment::LeadReviewDecision::Approved,
                "CHANGES_REQUESTED" => {
                    crate::planning::environment::LeadReviewDecision::ChangesRequested
                }
                "REJECTED" => crate::planning::environment::LeadReviewDecision::Rejected,
                _ => {
                    return Err(Error::Validation(
                        "unknown environment review decision".into(),
                    ));
                }
            },
            rationale,
            constraints: serde_json::from_str(&constraints_json).unwrap_or_default(),
            decided_at,
            evidence_refs: serde_json::from_str(&evidence_json).unwrap_or_default(),
            model_call: serde_json::from_str(&model_call_json).map_err(|_| {
                Error::Validation("stored environment review model call is not decodable".into())
            })?,
        })
    }

    pub async fn environment_plan_reviews(
        &self,
        request_id: &str,
    ) -> Result<Vec<EnvironmentPlanReview>, Error> {
        let rows: Vec<ReviewRow> = sqlx::query_as::query_as(
            "SELECT id,request_id,action_id,plan_id,plan_revision,risk_class,
                        principal_id,recorded_by,decision,rationale,constraints_json,
                        decided_at,evidence_json,model_call_json
                 FROM environment_plan_reviews WHERE request_id=?1
                 ORDER BY decided_at,id",
        )
        .bind(request_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(
                |(
                    id,
                    request_id,
                    action_id,
                    plan_id,
                    plan_revision,
                    risk_class,
                    principal_id,
                    recorded_by,
                    decision,
                    rationale,
                    constraints_json,
                    decided_at,
                    evidence_json,
                    model_call_json,
                )| {
                    Ok(EnvironmentPlanReview {
                        schema_version: crate::planning::environment::ENVIRONMENT_SCHEMA_VERSION,
                        review_id: id,
                        request_id,
                        action_id,
                        plan_id,
                        plan_sha256: String::new(),
                        plan_revision: u32::try_from(plan_revision)
                            .map_err(|_| Error::Invalid("stored environment review revision"))?,
                        risk_class,
                        reviewer_role: crate::planning::environment::EnvironmentReviewerRole::Lead,
                        principal_id,
                        recorded_by,
                        decision: match decision.as_str() {
                            "APPROVED" => {
                                crate::planning::environment::LeadReviewDecision::Approved
                            }
                            "CHANGES_REQUESTED" => {
                                crate::planning::environment::LeadReviewDecision::ChangesRequested
                            }
                            "REJECTED" => {
                                crate::planning::environment::LeadReviewDecision::Rejected
                            }
                            _ => {
                                return Err(Error::Validation(
                                    "unknown environment review decision".into(),
                                ));
                            }
                        },
                        rationale,
                        constraints: serde_json::from_str(&constraints_json).unwrap_or_default(),
                        decided_at,
                        evidence_refs: serde_json::from_str(&evidence_json).unwrap_or_default(),
                        model_call: serde_json::from_str(&model_call_json).map_err(|_| {
                            Error::Validation(
                                "stored environment review model call is not decodable".into(),
                            )
                        })?,
                    })
                },
            )
            .collect()
    }

    /// Persist one immutable frozen spec. Repeating the exact spec is
    /// idempotent; a different spec for the same plan is a conflict.
    pub async fn save_environment_spec(
        &self,
        spec: &EnvironmentSpec,
    ) -> Result<StoredEnvironmentSpec, Error> {
        spec.validate()
            .map_err(|error| Error::Validation(error.message))?;
        let content_json =
            serde_json::to_string(spec).map_err(|_| Error::Invalid("environment spec encoding"))?;
        validate_json_size("environment spec", &content_json)?;
        let now = Utc::now().to_rfc3339();
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let existing: Option<(String, String, String)> = sqlx::query_as::query_as(
            "SELECT id,content_json,created_at FROM environment_specs WHERE spec_sha256=?1",
        )
        .bind(&spec.spec_sha256)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some((row_id, stored_json, created_at)) = existing {
            transaction.commit().await?;
            if stored_json != content_json {
                return Err(Error::Conflict(
                    "environment spec hash already has different content",
                ));
            }
            return decode_spec(
                row_id,
                spec.spec_sha256.clone(),
                stored_json,
                spec.frozen_at.clone(),
                created_at,
            );
        }
        let duplicate: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT spec_sha256 FROM environment_specs WHERE plan_sha256=?1",
        )
        .bind(&spec.plan_sha256)
        .fetch_optional(&mut *transaction)
        .await?;
        if duplicate.is_some() {
            return Err(Error::Conflict(
                "the environment plan already has a frozen spec",
            ));
        }
        let row_id = Uuid::new_v4().to_string();
        let stored_request: Option<(Option<String>,)> =
            sqlx::query_as::query_as("SELECT id FROM intake_requests WHERE id=?1")
                .bind(&spec.request_id)
                .fetch_optional(&mut *transaction)
                .await?;
        let spec_request_id = stored_request.and_then(|(id,)| id);
        sqlx::query::query(
            "INSERT INTO environment_specs
             (id,spec_id,spec_sha256,project_id,request_id,action_id,campaign_id,
              base_commit,plan_id,plan_sha256,conventions_sha256,review_id,platform,
              nix_version,devenv_version,flake_lock_sha256,devenv_lock_sha256,
              license_inventory_sha256,sbom_sha256,builder_image,builder_image_digest,
              status,content_json,frozen_at,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,
                     ?18,?19,?20,?21,'FROZEN',?22,?23,?24,?25)",
        )
        .bind(&row_id)
        .bind(&spec.spec_id)
        .bind(&spec.spec_sha256)
        .bind(&spec.project_id)
        .bind(&spec_request_id)
        .bind(&spec.action_id)
        .bind(&spec.campaign_id)
        .bind(&spec.base_commit)
        .bind(&spec.plan_id)
        .bind(&spec.plan_sha256)
        .bind(&spec.conventions_sha256)
        .bind(&spec.review_id)
        .bind(&spec.platform)
        .bind(&spec.nix_version)
        .bind(&spec.devenv_version)
        .bind(&spec.flake_lock_sha256)
        .bind(&spec.devenv_lock_sha256)
        .bind(&spec.license_inventory_sha256)
        .bind(&spec.sbom_sha256)
        .bind(&spec.builder_image)
        .bind(&spec.builder_image_digest)
        .bind(&content_json)
        .bind(&spec.frozen_at)
        .bind(&now)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        decode_spec(
            row_id,
            spec.spec_sha256.clone(),
            content_json,
            spec.frozen_at.clone(),
            now,
        )
    }

    pub async fn environment_spec_by_sha(
        &self,
        spec_sha256: &str,
    ) -> Result<StoredEnvironmentSpec, Error> {
        let row: Option<SpecRow> = sqlx::query_as::query_as(
            "SELECT id,spec_sha256,frozen_at,content_json,created_at
             FROM environment_specs WHERE spec_sha256=?1",
        )
        .bind(spec_sha256)
        .fetch_optional(&self.pool)
        .await?;
        let Some((row_id, spec_sha256, frozen_at, content, created_at)) = row else {
            return Err(Error::NotFound);
        };
        decode_spec(row_id, spec_sha256, content, frozen_at, created_at)
    }

    pub async fn environment_spec_for_request(
        &self,
        request_id: &str,
    ) -> Result<StoredEnvironmentSpec, Error> {
        let row: Option<SpecRow> = sqlx::query_as::query_as(
            "SELECT id,spec_sha256,frozen_at,content_json,created_at
             FROM environment_specs WHERE request_id=?1 AND status='FROZEN'
             ORDER BY frozen_at DESC LIMIT 1",
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((row_id, spec_sha256, frozen_at, content, created_at)) = row else {
            return Err(Error::NotFound);
        };
        decode_spec(row_id, spec_sha256, content, frozen_at, created_at)
    }

    pub async fn environment_spec_for_action(
        &self,
        action_id: &str,
    ) -> Result<StoredEnvironmentSpec, Error> {
        let row: Option<SpecRow> = sqlx::query_as::query_as(
            "SELECT id,spec_sha256,frozen_at,content_json,created_at
             FROM environment_specs WHERE action_id=?1 AND status='FROZEN'
             ORDER BY frozen_at DESC LIMIT 1",
        )
        .bind(action_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((row_id, spec_sha256, frozen_at, content, created_at)) = row else {
            return Err(Error::NotFound);
        };
        decode_spec(row_id, spec_sha256, content, frozen_at, created_at)
    }

    pub async fn save_environment_license_inventory(
        &self,
        plan_sha256: &str,
        spec_sha256: Option<&str>,
        inventory: &LicenseInventory,
    ) -> Result<(), Error> {
        let content_json = serde_json::to_string(inventory)
            .map_err(|_| Error::Invalid("license inventory encoding"))?;
        validate_json_size("license inventory", &content_json)?;
        let now = Utc::now().to_rfc3339();
        let _write = self.write_lock.lock().await;
        sqlx::query::query(
            "INSERT OR IGNORE INTO environment_license_inventories
             (id,plan_sha256,spec_sha256,inventory_sha256,sbom_sha256,coverage,
              content_json,created_at)
             VALUES (?1,?2,?3,?4,?5,'SUBSTRATE_AND_DECLARED_PACKAGES',?6,?7)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(plan_sha256)
        .bind(spec_sha256.unwrap_or(""))
        .bind(&inventory.inventory_sha256)
        .bind(&inventory.sbom_sha256)
        .bind(&content_json)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn save_environment_build_receipt(
        &self,
        receipt: &EnvironmentBuildReceipt,
    ) -> Result<EnvironmentBuildReceipt, Error> {
        let content_json = serde_json::to_string(receipt)
            .map_err(|_| Error::Invalid("environment build receipt encoding"))?;
        validate_json_size("environment build receipt", &content_json)?;
        let status = match receipt.status {
            crate::planning::environment::EnvironmentBuilderStatus::Unavailable => "UNAVAILABLE",
            crate::planning::environment::EnvironmentBuilderStatus::Ready => "READY",
            crate::planning::environment::EnvironmentBuilderStatus::Building => "BUILDING",
            crate::planning::environment::EnvironmentBuilderStatus::Succeeded => "SUCCEEDED",
            crate::planning::environment::EnvironmentBuilderStatus::Failed => "FAILED",
        };
        let attestation = serde_json::to_string(&receipt.sandbox_attestation)
            .map_err(|_| Error::Invalid("sandbox attestation encoding"))?;
        let blockers = serde_json::to_string(&receipt.blockers)
            .map_err(|_| Error::Invalid("environment blockers encoding"))?;
        let now = Utc::now().to_rfc3339();
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let existing: Option<(String, String)> = sqlx::query_as::query_as(
            "SELECT receipt_id,content_json FROM environment_build_receipts
             WHERE spec_sha256=?1 AND platform=?2 AND builder_policy_revision=?3
               AND builder_image_digest=?4",
        )
        .bind(&receipt.spec_sha256)
        .bind(&receipt.platform)
        .bind(&receipt.builder_policy_revision)
        .bind(&receipt.builder_image_digest)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some((receipt_id, stored_json)) = existing {
            transaction.commit().await?;
            if stored_json != content_json {
                return Err(Error::Conflict(
                    "environment build receipt already exists with different content",
                ));
            }
            let mut value = receipt.clone();
            value.receipt_id = receipt_id;
            return Ok(value);
        }
        sqlx::query::query(
            "INSERT INTO environment_build_receipts
             (id,receipt_id,spec_sha256,platform,builder_image,builder_image_digest,
              builder_policy_revision,status,store_path,output_sha256,log_ref,
              attestation_json,blockers_json,duration_ms,content_json,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&receipt.receipt_id)
        .bind(&receipt.spec_sha256)
        .bind(&receipt.platform)
        .bind(&receipt.builder_image)
        .bind(&receipt.builder_image_digest)
        .bind(&receipt.builder_policy_revision)
        .bind(status)
        .bind(&receipt.store_path)
        .bind(&receipt.output_sha256)
        .bind(&receipt.log_ref)
        .bind(&attestation)
        .bind(&blockers)
        .bind(i64::try_from(receipt.duration_ms).unwrap_or(i64::MAX))
        .bind(&content_json)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(receipt.clone())
    }

    pub async fn latest_environment_build_receipt(
        &self,
        spec_sha256: &str,
    ) -> Result<EnvironmentBuildReceipt, Error> {
        let row: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT content_json FROM environment_build_receipts
             WHERE spec_sha256=?1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(spec_sha256)
        .fetch_optional(&self.pool)
        .await?;
        let Some((content,)) = row else {
            return Err(Error::NotFound);
        };
        serde_json::from_str(&content).map_err(|_| {
            Error::Validation("stored environment build receipt is not decodable".into())
        })
    }

    /// Bind the exact environment plan/spec to an ActionSpec before a Campaign
    /// exists. The ActionSpec stays immutable after Campaign creation.
    pub async fn set_action_environment_binding(
        &self,
        action_id: &str,
        plan: &EnvironmentPlan,
        spec: &EnvironmentSpec,
    ) -> Result<(), Error> {
        if plan.action_id != action_id || spec.action_id != action_id {
            return Err(Error::Validation(
                "the environment plan/spec belongs to a different Action".into(),
            ));
        }
        if plan.status != EnvironmentPlanStatus::Frozen
            || spec.status != EnvironmentSpecStatus::Frozen
            || spec.plan_sha256 != plan.plan_sha256
        {
            return Err(Error::Conflict(
                "only a frozen environment plan/spec can bind an Action",
            ));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let campaigns: (i64,) =
            sqlx::query_as::query_as("SELECT COUNT(*) FROM campaigns WHERE action_id=?1")
                .bind(action_id)
                .fetch_one(&mut *transaction)
                .await?;
        if campaigns.0 != 0 {
            return Err(Error::Conflict("a Campaign already exists for this action"));
        }
        let (spec_json,): (String,) =
            sqlx::query_as::query_as("SELECT spec_json FROM action_specs WHERE action_id=?1")
                .bind(action_id)
                .fetch_optional(&mut *transaction)
                .await?
                .ok_or(Error::NotFound)?;
        let mut action_spec: crate::db::ActionSpec = serde_json::from_str(&spec_json)
            .map_err(|_| Error::Invalid("frozen ActionSpec encoding"))?;
        for (existing, next) in [
            (
                action_spec.environment_plan_id.as_deref(),
                Some(plan.plan_id.as_str()),
            ),
            (
                action_spec.environment_plan_sha256.as_deref(),
                Some(plan.plan_sha256.as_str()),
            ),
            (
                action_spec.environment_spec_id.as_deref(),
                Some(spec.spec_id.as_str()),
            ),
            (
                action_spec.environment_spec_sha256.as_deref(),
                Some(spec.spec_sha256.as_str()),
            ),
        ] {
            if let Some(existing) = existing
                && Some(existing) != next
            {
                return Err(Error::Conflict(
                    "the ActionSpec already has a different environment binding",
                ));
            }
        }
        action_spec.environment_plan_id = Some(plan.plan_id.clone());
        action_spec.environment_plan_sha256 = Some(plan.plan_sha256.clone());
        action_spec.environment_spec_id = Some(spec.spec_id.clone());
        action_spec.environment_spec_sha256 = Some(spec.spec_sha256.clone());
        action_spec.environment_builder_image = Some(spec.builder_image.clone());
        action_spec.environment_builder_image_digest = Some(spec.builder_image_digest.clone());
        action_spec.environment_required = true;
        for check in &mut action_spec.checks {
            if let Some(existing) = &check.environment_plan_id
                && existing != &plan.plan_id
            {
                return Err(Error::Conflict(
                    "a check is already bound to a different environment plan",
                ));
            }
            if let Some(existing) = &check.environment_spec_sha256
                && existing != &spec.spec_sha256
            {
                return Err(Error::Conflict(
                    "a check is already bound to a different environment spec",
                ));
            }
            check.environment_plan_id = Some(plan.plan_id.clone());
            check.environment_spec_sha256 = Some(spec.spec_sha256.clone());
        }
        let spec_json = serde_json::to_string(&action_spec)
            .map_err(|_| Error::Invalid("frozen ActionSpec encoding"))?;
        let spec_sha256 = sha256_hex(spec_json.as_bytes());
        sqlx::query::query(
            "UPDATE action_specs SET spec_json=?1,spec_sha256=?2 WHERE action_id=?3",
        )
        .bind(&spec_json)
        .bind(&spec_sha256)
        .bind(action_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Load the exact plan/spec/receipt set bound to a Campaign. A missing,
    /// mismatched or unsuccessful build is a typed refusal, never a language
    /// image fallback.
    pub async fn environment_execution_for_campaign(
        &self,
        campaign_id: &str,
    ) -> Result<Option<crate::planning::environment::EnvironmentExecutionBinding>, Error> {
        let binding = match self.campaign_environment(campaign_id).await {
            Ok(binding) => binding,
            Err(Error::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        let spec = match self.environment_spec_by_sha(&binding.spec_sha256).await {
            Ok(spec) => spec,
            Err(Error::NotFound) => {
                #[cfg(test)]
                return Ok(None);
                #[cfg(not(test))]
                return Err(Error::IntakeConflict {
                    code: "ENVIRONMENT_PLAN_REQUIRED",
                    message: "the Campaign environment spec is missing".into(),
                });
            }
            Err(error) => return Err(error),
        };
        let plan = self.environment_plan_by_sha(&binding.plan_sha256).await?;
        let receipt = self
            .latest_environment_build_receipt(&binding.spec_sha256)
            .await
            .map_err(|error| match error {
                Error::NotFound => Error::IntakeConflict {
                    code: "ENVIRONMENT_BUILDER_UNAVAILABLE",
                    message: "the Campaign environment has no successful build receipt".into(),
                },
                other => other,
            })?;
        let execution = crate::planning::environment::EnvironmentExecutionBinding {
            plan: plan.plan,
            spec: spec.spec,
            receipt,
        };
        execution
            .validate()
            .map_err(|error| Error::IntakeConflict {
                code: error.code_str(),
                message: error.message,
            })?;
        Ok(Some(execution))
    }

    pub async fn campaign_environment(
        &self,
        campaign_id: &str,
    ) -> Result<CampaignEnvironmentBinding, Error> {
        let row: Option<CampaignRow> = sqlx::query_as::query_as(
            "SELECT campaign_id,request_id,action_id,spec_id,spec_sha256,plan_sha256,
                    builder_image,builder_image_digest,bound_at
             FROM campaign_environments WHERE campaign_id=?1",
        )
        .bind(campaign_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((
            campaign_id,
            request_id,
            action_id,
            spec_id,
            spec_sha256,
            plan_sha256,
            builder_image,
            builder_image_digest,
            bound_at,
        )) = row
        else {
            return Err(Error::NotFound);
        };
        Ok(CampaignEnvironmentBinding {
            campaign_id,
            request_id,
            action_id,
            spec_id,
            spec_sha256,
            plan_sha256,
            builder_image,
            builder_image_digest,
            bound_at,
        })
    }
}

/// `#[cfg(test)]`-only fallback for library unit tests that create a Campaign
/// without real environment rows. It is never compiled for production builds,
/// the `test-support` feature or binary test harnesses; Core authorization and
/// execution still require the real frozen spec row in production.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn bind_test_campaign_environment_in_tx(
    transaction: &mut SqliteConnection,
    campaign_id: &str,
    action_id: &str,
    plan_sha256: &str,
    spec_id: &str,
    spec_sha256: &str,
    builder_image: &str,
    builder_image_digest: &str,
) -> Result<(), Error> {
    let existing: Option<(String, String, String, String)> = sqlx::query_as::query_as(
        "SELECT spec_sha256,plan_sha256,builder_image,builder_image_digest
         FROM campaign_environments WHERE campaign_id=?1",
    )
    .bind(campaign_id)
    .fetch_optional(&mut *transaction)
    .await?;
    if let Some((stored_spec, stored_plan, stored_image, stored_digest)) = existing {
        if stored_spec != spec_sha256
            || stored_plan != plan_sha256
            || stored_image != builder_image
            || stored_digest != builder_image_digest
        {
            return Err(Error::Conflict(
                "Campaign already has a different environment binding",
            ));
        }
        return Ok(());
    }
    sqlx::query::query(
        "INSERT INTO campaign_environments
         (campaign_id,request_id,action_id,spec_id,spec_sha256,plan_sha256,
          builder_image,builder_image_digest,bound_at)
         VALUES (?1,NULL,?2,?3,?4,?5,?6,?7,?8)",
    )
    .bind(campaign_id)
    .bind(action_id)
    .bind(spec_id)
    .bind(spec_sha256)
    .bind(plan_sha256)
    .bind(builder_image)
    .bind(builder_image_digest)
    .bind(Utc::now().to_rfc3339())
    .execute(&mut *transaction)
    .await?;
    Ok(())
}

/// Insert or verify the exact Campaign environment binding inside the caller's
/// authorization transaction. A different binding is a hard conflict.
pub(crate) async fn bind_campaign_environment_in_tx(
    transaction: &mut SqliteConnection,
    campaign_id: &str,
    request_id: Option<&str>,
    action_id: &str,
    spec: &EnvironmentSpec,
) -> Result<(), Error> {
    if spec.status != EnvironmentSpecStatus::Frozen {
        return Err(Error::Forbidden(
            "a Campaign cannot bind a non-frozen environment spec".into(),
        ));
    }
    let existing: Option<(String, String, String, String)> = sqlx::query_as::query_as(
        "SELECT spec_sha256,plan_sha256,builder_image,builder_image_digest
         FROM campaign_environments WHERE campaign_id=?1",
    )
    .bind(campaign_id)
    .fetch_optional(&mut *transaction)
    .await?;
    if let Some((spec_sha256, plan_sha256, builder_image, builder_image_digest)) = existing {
        if spec_sha256 != spec.spec_sha256
            || plan_sha256 != spec.plan_sha256
            || builder_image != spec.builder_image
            || builder_image_digest != spec.builder_image_digest
        {
            return Err(Error::Conflict(
                "Campaign already has a different environment binding",
            ));
        }
        return Ok(());
    }
    sqlx::query::query(
        "INSERT INTO campaign_environments
         (campaign_id,request_id,action_id,spec_id,spec_sha256,plan_sha256,
          builder_image,builder_image_digest,bound_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
    )
    .bind(campaign_id)
    .bind(request_id)
    .bind(action_id)
    .bind(&spec.spec_id)
    .bind(&spec.spec_sha256)
    .bind(&spec.plan_sha256)
    .bind(&spec.builder_image)
    .bind(&spec.builder_image_digest)
    .bind(Utc::now().to_rfc3339())
    .execute(&mut *transaction)
    .await?;
    Ok(())
}

#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct TestEnvironmentBinding {
    pub plan_id: String,
    pub plan_sha256: String,
    pub spec_id: String,
    pub spec_sha256: String,
    pub builder_image: String,
    pub builder_image_digest: String,
}

#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub fn apply_test_environment_binding(spec: &mut crate::db::ActionSpec, action_id: &str) {
    let binding = test_environment_binding(action_id);
    spec.environment_plan_id = Some(binding.plan_id.clone());
    spec.environment_plan_sha256 = Some(binding.plan_sha256);
    spec.environment_spec_id = Some(binding.spec_id);
    spec.environment_spec_sha256 = Some(binding.spec_sha256.clone());
    spec.environment_builder_image = Some(binding.builder_image);
    spec.environment_builder_image_digest = Some(binding.builder_image_digest);
    spec.environment_required = true;
    for check in &mut spec.checks {
        check.environment_plan_id = Some(binding.plan_id.clone());
        check.environment_spec_sha256 = Some(binding.spec_sha256.clone());
    }
}

#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub fn test_environment_binding(action_id: &str) -> TestEnvironmentBinding {
    TestEnvironmentBinding {
        plan_id: format!("test-plan-{action_id}"),
        plan_sha256: sha256_hex(format!("test-plan:{action_id}").as_bytes()),
        spec_id: format!("test-spec-{action_id}"),
        spec_sha256: sha256_hex(format!("test-spec:{action_id}").as_bytes()),
        builder_image: "fixture/environment-builder".into(),
        builder_image_digest: format!("sha256:{}", "a".repeat(64)),
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Database {
    /// Fixture-only binding used by deterministic tests. It marks an ActionSpec
    /// as carrying an exact environment identity so authorization/readiness
    /// fixtures can exercise product paths without fabricating a production
    /// EnvironmentPlan or Lead model call. Worker/Verifier still require the
    /// real DB plan/spec/receipt and fail closed when this fixture binding has
    /// no execution evidence. Production code never calls this method.
    #[doc(hidden)]
    #[doc(hidden)]
    pub async fn clear_test_action_environment(&self, action_id: &str) -> Result<(), Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let (spec_json,): (String,) =
            sqlx::query_as::query_as("SELECT spec_json FROM action_specs WHERE action_id=?1")
                .bind(action_id)
                .fetch_optional(&mut *transaction)
                .await?
                .ok_or(Error::NotFound)?;
        let mut spec: crate::db::ActionSpec = serde_json::from_str(&spec_json)
            .map_err(|_| Error::Invalid("stored ActionSpec encoding"))?;
        spec.environment_plan_id = None;
        spec.environment_plan_sha256 = None;
        spec.environment_spec_id = None;
        spec.environment_spec_sha256 = None;
        spec.environment_builder_image = None;
        spec.environment_builder_image_digest = None;
        spec.environment_required = false;
        for check in &mut spec.checks {
            check.environment_plan_id = None;
            check.environment_spec_sha256 = None;
        }
        let spec_json =
            serde_json::to_string(&spec).map_err(|_| Error::Invalid("ActionSpec encoding"))?;
        let spec_sha256 = sha256_hex(spec_json.as_bytes());
        sqlx::query::query(
            "UPDATE action_specs SET spec_json=?1,spec_sha256=?2 WHERE action_id=?3",
        )
        .bind(&spec_json)
        .bind(&spec_sha256)
        .bind(action_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn bind_test_action_environment(&self, action_id: &str) -> Result<(), Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let (spec_json,): (String,) =
            sqlx::query_as::query_as("SELECT spec_json FROM action_specs WHERE action_id=?1")
                .bind(action_id)
                .fetch_optional(&mut *transaction)
                .await?
                .ok_or(Error::NotFound)?;
        let mut spec: crate::db::ActionSpec = serde_json::from_str(&spec_json)
            .map_err(|_| Error::Invalid("stored ActionSpec encoding"))?;
        apply_test_environment_binding(&mut spec, action_id);
        let spec_json =
            serde_json::to_string(&spec).map_err(|_| Error::Invalid("ActionSpec encoding"))?;
        let spec_sha256 = sha256_hex(spec_json.as_bytes());
        sqlx::query::query(
            "UPDATE action_specs SET spec_json=?1,spec_sha256=?2 WHERE action_id=?3",
        )
        .bind(&spec_json)
        .bind(&spec_sha256)
        .bind(action_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}
