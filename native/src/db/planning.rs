//! Durable planning artifacts and plan reviews.
//!
//! Core owns these records. A model may propose artifact content, but only this
//! persistence layer binds the exact bytes to a request revision and a SHA-256.
//! Plan reviews bind the exact plan hash they reviewed; a stale review can never
//! approve a newer plan.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{Database, Error};

const MAX_ARTIFACT_BYTES: usize = 2 * 1024 * 1024;
const MAX_REVIEW_RATIONALE_CHARS: usize = 4_000;
const MAX_PRINCIPAL_CHARS: usize = 200;
const PLAN_KINDS: &[&str] = &[
    "PLAN",
    "STACK",
    "ARCHITECTURE",
    "DOCUMENTATION",
    "SKILL",
    "DOD",
    "OPENSPEC",
];

type ArtifactRow = (String, String, i64, String, String, String);
type ReviewRow = (
    String,
    String,
    i64,
    String,
    String,
    String,
    String,
    i64,
    String,
);
type SkillPackRow = (
    String,
    String,
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
type OpenSpecRow = (
    String,
    String,
    String,
    i64,
    String,
    String,
    String,
    String,
    i64,
    i64,
    String,
    String,
);
type OpenSpecExistingRow = (
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    String,
    String,
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanningArtifactRecord {
    pub request_id: String,
    pub kind: String,
    pub revision: u32,
    pub content_json: String,
    pub content_sha256: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanReviewRecord {
    pub id: String,
    pub request_id: String,
    pub plan_revision: u32,
    pub plan_sha256: String,
    pub status: String,
    pub principal_id: String,
    pub rationale: String,
    pub points: u16,
    pub reviewed_at: String,
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn validate_kind(kind: &str) -> Result<(), Error> {
    if !PLAN_KINDS.contains(&kind) {
        return Err(Error::Validation("unknown planning artifact kind".into()));
    }
    Ok(())
}

fn validate_text(name: &str, value: &str, max: usize) -> Result<(), Error> {
    if value.trim().is_empty() || value.chars().count() > max || value.chars().any(char::is_control)
    {
        return Err(Error::Validation(format!(
            "{name} is required, bounded and control-free"
        )));
    }
    Ok(())
}

pub struct NewPlanReview<'a> {
    pub request_id: &'a str,
    pub plan_revision: u32,
    pub plan_sha256: &'a str,
    pub status: &'a str,
    pub principal_id: &'a str,
    pub rationale: &'a str,
    pub points: u16,
}

impl Database {
    /// Persist one exact planning artifact revision. Repeating the same bytes is
    /// idempotent; a different payload for an existing revision is a conflict.
    pub async fn save_planning_artifact(
        &self,
        request_id: &str,
        kind: &str,
        revision: u32,
        content_json: &str,
    ) -> Result<PlanningArtifactRecord, Error> {
        validate_kind(kind)?;
        validate_text("planning request id", request_id, 200)?;
        if revision == 0 {
            return Err(Error::Validation(
                "planning artifact revision must be positive".into(),
            ));
        }
        if content_json.len() > MAX_ARTIFACT_BYTES {
            return Err(Error::Validation(
                "planning artifact exceeds the byte bound".into(),
            ));
        }
        let _: serde_json::Value = serde_json::from_str(content_json)
            .map_err(|_| Error::Validation("planning artifact is not valid JSON".into()))?;
        let content_sha256 = sha256_hex(content_json.as_bytes());
        let now = Utc::now().to_rfc3339();

        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let existing: Option<(String, String)> = sqlx::query_as::query_as(
            "SELECT content_json,content_sha256 FROM planning_artifacts
             WHERE request_id=?1 AND kind=?2 AND revision=?3",
        )
        .bind(request_id)
        .bind(kind)
        .bind(i64::from(revision))
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some((stored_json, stored_sha256)) = existing {
            transaction.commit().await?;
            if stored_sha256 != content_sha256 || stored_json != content_json {
                return Err(Error::Conflict(
                    "planning artifact revision already has different content",
                ));
            }
            return Ok(PlanningArtifactRecord {
                request_id: request_id.to_string(),
                kind: kind.to_string(),
                revision,
                content_json: stored_json,
                content_sha256: stored_sha256,
                created_at: now,
            });
        }
        sqlx::query::query(
            "INSERT INTO planning_artifacts
             (request_id,kind,revision,content_json,content_sha256,created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
        )
        .bind(request_id)
        .bind(kind)
        .bind(i64::from(revision))
        .bind(content_json)
        .bind(&content_sha256)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(PlanningArtifactRecord {
            request_id: request_id.to_string(),
            kind: kind.to_string(),
            revision,
            content_json: content_json.to_string(),
            content_sha256,
            created_at: now,
        })
    }

    pub async fn planning_artifact(
        &self,
        request_id: &str,
        kind: &str,
        revision: u32,
    ) -> Result<PlanningArtifactRecord, Error> {
        validate_kind(kind)?;
        let row: Option<ArtifactRow> = sqlx::query_as::query_as(
            "SELECT request_id,kind,revision,content_json,content_sha256,created_at
                 FROM planning_artifacts
                 WHERE request_id=?1 AND kind=?2 AND revision=?3",
        )
        .bind(request_id)
        .bind(kind)
        .bind(i64::from(revision))
        .fetch_optional(&self.pool)
        .await?;
        row.ok_or(Error::NotFound).and_then(artifact_from_row)
    }

    pub async fn latest_planning_artifact(
        &self,
        request_id: &str,
        kind: &str,
    ) -> Result<PlanningArtifactRecord, Error> {
        validate_kind(kind)?;
        let row: Option<ArtifactRow> = sqlx::query_as::query_as(
            "SELECT request_id,kind,revision,content_json,content_sha256,created_at
                 FROM planning_artifacts
                 WHERE request_id=?1 AND kind=?2
                 ORDER BY revision DESC LIMIT 1",
        )
        .bind(request_id)
        .bind(kind)
        .fetch_optional(&self.pool)
        .await?;
        row.ok_or(Error::NotFound).and_then(artifact_from_row)
    }

    pub async fn planning_artifacts(
        &self,
        request_id: &str,
    ) -> Result<Vec<PlanningArtifactRecord>, Error> {
        let rows: Vec<ArtifactRow> = sqlx::query_as::query_as(
            "SELECT request_id,kind,revision,content_json,content_sha256,created_at
                 FROM planning_artifacts WHERE request_id=?1
                 ORDER BY kind,revision",
        )
        .bind(request_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(artifact_from_row).collect()
    }

    /// Persist an authenticated plan review. The review must bind the current
    /// PLAN artifact hash; a review of a stale plan is rejected.
    pub async fn save_plan_review(
        &self,
        input: NewPlanReview<'_>,
    ) -> Result<PlanReviewRecord, Error> {
        let NewPlanReview {
            request_id,
            plan_revision,
            plan_sha256,
            status,
            principal_id,
            rationale,
            points,
        } = input;
        validate_text("plan review request id", request_id, 200)?;
        validate_text("plan review principal", principal_id, MAX_PRINCIPAL_CHARS)?;
        validate_text(
            "plan review rationale",
            rationale,
            MAX_REVIEW_RATIONALE_CHARS,
        )?;
        if plan_revision == 0 || plan_sha256.len() != 64 {
            return Err(Error::Validation(
                "plan review requires a positive revision and SHA-256".into(),
            ));
        }
        if !matches!(
            status,
            "PENDING" | "AUTO_APPROVED" | "OWNER_APPROVED" | "CHANGES_REQUESTED" | "REJECTED"
        ) {
            return Err(Error::Validation("unknown plan review status".into()));
        }
        let plan = self
            .planning_artifact(request_id, "PLAN", plan_revision)
            .await?;
        if plan.content_sha256 != plan_sha256 {
            return Err(Error::Conflict(
                "plan review does not bind the current plan revision",
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let reviewed_at = Utc::now().to_rfc3339();
        let _write = self.write_lock.lock().await;
        sqlx::query::query(
            "INSERT INTO plan_reviews
             (id,request_id,plan_revision,plan_sha256,status,principal_id,rationale,points,reviewed_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        )
        .bind(&id)
        .bind(request_id)
        .bind(i64::from(plan_revision))
        .bind(plan_sha256)
        .bind(status)
        .bind(principal_id)
        .bind(rationale)
        .bind(i64::from(points))
        .bind(&reviewed_at)
        .execute(&self.pool)
        .await?;
        Ok(PlanReviewRecord {
            id,
            request_id: request_id.to_string(),
            plan_revision,
            plan_sha256: plan_sha256.to_string(),
            status: status.to_string(),
            principal_id: principal_id.to_string(),
            rationale: rationale.to_string(),
            points,
            reviewed_at,
        })
    }

    pub async fn latest_plan_review(&self, request_id: &str) -> Result<PlanReviewRecord, Error> {
        let row: Option<ReviewRow> = sqlx::query_as::query_as(
            "SELECT id,request_id,plan_revision,plan_sha256,status,principal_id,rationale,points,reviewed_at
             FROM plan_reviews WHERE request_id=?1
             ORDER BY reviewed_at DESC,id DESC LIMIT 1",
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?;
        row.ok_or(Error::NotFound).and_then(review_from_row)
    }

    pub async fn plan_reviews(&self, request_id: &str) -> Result<Vec<PlanReviewRecord>, Error> {
        let rows: Vec<ReviewRow> = sqlx::query_as::query_as(
            "SELECT id,request_id,plan_revision,plan_sha256,status,principal_id,rationale,points,reviewed_at
             FROM plan_reviews WHERE request_id=?1
             ORDER BY reviewed_at,id",
        )
        .bind(request_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(review_from_row).collect()
    }
}

fn artifact_from_row(row: ArtifactRow) -> Result<PlanningArtifactRecord, Error> {
    Ok(PlanningArtifactRecord {
        request_id: row.0,
        kind: row.1,
        revision: u32::try_from(row.2)
            .map_err(|_| Error::Invalid("stored planning artifact revision"))?,
        content_json: row.3,
        content_sha256: row.4,
        created_at: row.5,
    })
}

fn review_from_row(row: ReviewRow) -> Result<PlanReviewRecord, Error> {
    Ok(PlanReviewRecord {
        id: row.0,
        request_id: row.1,
        plan_revision: u32::try_from(row.2)
            .map_err(|_| Error::Invalid("stored plan review revision"))?,
        plan_sha256: row.3,
        status: row.4,
        principal_id: row.5,
        rationale: row.6,
        points: u16::try_from(row.7).map_err(|_| Error::Invalid("stored plan review points"))?,
        reviewed_at: row.8,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPackRecord {
    pub id: String,
    pub technology: String,
    pub version: String,
    pub content_json: String,
    pub content_sha256: String,
    pub source_urls_json: String,
    pub last_verified_at: String,
    pub review_after: String,
    pub license: String,
    pub provenance: String,
    pub created_at: String,
}

impl Database {
    /// Persist a validated, source-cited skill pack. The content hash makes a
    /// refresh a new immutable record rather than an in-place mutation.
    pub async fn save_skill_pack(
        &self,
        pack: &crate::planning::SkillPack,
    ) -> Result<SkillPackRecord, Error> {
        pack.validate()
            .map_err(|error| Error::Validation(format!("skill pack: {error}")))?;
        let content_json = serde_json::to_string(pack)
            .map_err(|_| Error::Validation("skill pack encoding".into()))?;
        let content_sha256 = sha256_hex(content_json.as_bytes());
        let source_urls_json = serde_json::to_string(&pack.source_urls)
            .map_err(|_| Error::Validation("skill pack sources".into()))?;
        let now = Utc::now().to_rfc3339();
        let _write = self.write_lock.lock().await;
        let existing: Option<(String, String)> = sqlx::query_as::query_as(
            "SELECT id,created_at FROM skill_packs
             WHERE technology=?1 AND version=?2 AND content_sha256=?3",
        )
        .bind(&pack.technology)
        .bind(&pack.version)
        .bind(&content_sha256)
        .fetch_optional(&self.pool)
        .await?;
        if let Some((id, created_at)) = existing {
            return Ok(SkillPackRecord {
                id,
                technology: pack.technology.clone(),
                version: pack.version.clone(),
                content_json,
                content_sha256,
                source_urls_json,
                last_verified_at: pack.last_verified_at.clone(),
                review_after: pack.review_after.clone(),
                license: pack.license.clone(),
                provenance: pack.provenance.clone(),
                created_at,
            });
        }
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query::query(
            "INSERT INTO skill_packs
             (id,technology,version,content_json,content_sha256,source_urls_json,
              last_verified_at,review_after,license,provenance,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        )
        .bind(&id)
        .bind(&pack.technology)
        .bind(&pack.version)
        .bind(&content_json)
        .bind(&content_sha256)
        .bind(&source_urls_json)
        .bind(&pack.last_verified_at)
        .bind(&pack.review_after)
        .bind(&pack.license)
        .bind(&pack.provenance)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(SkillPackRecord {
            id,
            technology: pack.technology.clone(),
            version: pack.version.clone(),
            content_json,
            content_sha256,
            source_urls_json,
            last_verified_at: pack.last_verified_at.clone(),
            review_after: pack.review_after.clone(),
            license: pack.license.clone(),
            provenance: pack.provenance.clone(),
            created_at: now,
        })
    }

    pub async fn latest_skill_pack(&self, technology: &str) -> Result<SkillPackRecord, Error> {
        let row: Option<SkillPackRow> = sqlx::query_as::query_as(
            "SELECT id,technology,version,content_json,content_sha256,source_urls_json,
                    last_verified_at,review_after,license,provenance,created_at
             FROM skill_packs WHERE technology=?1
             ORDER BY created_at DESC,id DESC LIMIT 1",
        )
        .bind(technology)
        .fetch_optional(&self.pool)
        .await?;
        row.ok_or(Error::NotFound).and_then(skill_pack_from_row)
    }

    pub async fn skill_packs(&self) -> Result<Vec<SkillPackRecord>, Error> {
        let rows: Vec<SkillPackRow> = sqlx::query_as::query_as(
            "SELECT id,technology,version,content_json,content_sha256,source_urls_json,
                    last_verified_at,review_after,license,provenance,created_at
             FROM skill_packs ORDER BY technology,created_at,id",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(skill_pack_from_row).collect()
    }
}

fn skill_pack_from_row(row: SkillPackRow) -> Result<SkillPackRecord, Error> {
    Ok(SkillPackRecord {
        id: row.0,
        technology: row.1,
        version: row.2,
        content_json: row.3,
        content_sha256: row.4,
        source_urls_json: row.5,
        last_verified_at: row.6,
        review_after: row.7,
        license: row.8,
        provenance: row.9,
        created_at: row.10,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("factory-planning-{}.sqlite", uuid::Uuid::new_v4()))
    }

    async fn seeded_db(request_id: &str) -> Database {
        let db = Database::open(&temp_path()).await.unwrap();
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO projects(id,repository,base_commit,created_at,identity)
             VALUES ('project','/repo',?1,?2,'')",
        )
        .bind("a".repeat(40))
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO actions
             (id,project_id,objective,provider,model,risk,status,objective_sha256,created_at)
             VALUES ('action','project','objective','opencode','model','R0','AUTHORIZED','hash',?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO standing_delegations
             (id,principal_id,audit_id,project_id,revision,content_hash,content_json,created_at)
             VALUES ('delegation','owner','audit','project',1,'hash','{}',?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO lead_requests
             (id,project_id,action_id,delegation_id,objective,request_hash,
              project_config_hash,state,created_at,updated_at)
             VALUES ('lead','project','action','delegation','objective','hash','hash',
                     'FROZEN',?1,?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO execution_profiles(id,current_revision,content_hash,created_at,updated_at)
             VALUES ('profile',1,'hash',?1,?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO execution_profile_revisions
             (revision_id,profile_id,revision,content_json,content_hash,actor,created_at)
             VALUES ('profile-revision','profile',1,'{}','hash','owner',?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO intake_requests
             (id,project_id,objective,mode,profile_id,profile_revision_id,profile_revision,
              profile_hash,profile_name,action_id,delegation_id,lead_request_id,campaign_id,
              submission_json,submission_hash,idempotency_key,created_at,updated_at)
             VALUES (?1,'project','objective','REVIEW_PLAN','profile','profile-revision',1,
                     'hash','profile','action','delegation','lead',NULL,'{}','hash',?2,?3,?3)",
        )
        .bind(request_id)
        .bind(format!("intake-{request_id}"))
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        db
    }

    #[tokio::test]
    async fn artifact_revisions_are_exact_and_idempotent() {
        let db = seeded_db("request-artifact").await;
        let first = db
            .save_planning_artifact("request-artifact", "PLAN", 1, r#"{"lane":"DIRECT"}"#)
            .await
            .unwrap();
        assert_eq!(first.revision, 1);
        assert_eq!(first.content_sha256.len(), 64);
        let replay = db
            .save_planning_artifact("request-artifact", "PLAN", 1, r#"{"lane":"DIRECT"}"#)
            .await
            .unwrap();
        assert_eq!(replay.content_sha256, first.content_sha256);
        assert!(
            db.save_planning_artifact("request-artifact", "PLAN", 1, r#"{"lane":"FORMAL_SPEC"}"#)
                .await
                .is_err()
        );
        assert!(
            db.save_planning_artifact("request-artifact", "UNKNOWN", 1, "{}")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn plan_review_binds_the_exact_plan_hash() {
        let db = seeded_db("request-review").await;
        let plan = db
            .save_planning_artifact("request-review", "PLAN", 1, r#"{"lane":"DIRECT"}"#)
            .await
            .unwrap();
        let review = db
            .save_plan_review(NewPlanReview {
                request_id: "request-review",
                plan_revision: 1,
                plan_sha256: &plan.content_sha256,
                status: "AUTO_APPROVED",
                principal_id: "core:auto-plan-review",
                rationale: "DIRECT plan satisfies the deterministic policy.",
                points: 0,
            })
            .await
            .unwrap();
        assert_eq!(review.status, "AUTO_APPROVED");
        assert_eq!(review.plan_sha256, plan.content_sha256);
        assert!(
            db.save_plan_review(NewPlanReview {
                request_id: "request-review",
                plan_revision: 1,
                plan_sha256: &"b".repeat(64),
                status: "AUTO_APPROVED",
                principal_id: "core:auto-plan-review",
                rationale: "stale",
                points: 0,
            })
            .await
            .is_err()
        );
        let latest = db.latest_plan_review("request-review").await.unwrap();
        assert_eq!(latest.id, review.id);
    }

    #[tokio::test]
    async fn finding_dispositions_are_immutable_and_close_with_evidence() {
        use crate::planning::{FindingDisposition, FindingRoute, FindingStatus};
        let db = seeded_db("request-finding").await;
        let open = FindingDisposition {
            request_id: "request-finding".into(),
            finding_id: "finding-1".into(),
            route: FindingRoute::Repairer,
            status: FindingStatus::Open,
            candidate_id: "candidate-1".into(),
            reviewer_principal: "reviewer-1".into(),
            resolved_by: String::new(),
            evidence_refs: Vec::new(),
            owner_principal: None,
            owner_decision: None,
        };
        let saved = db.save_finding_disposition(&open).await.unwrap();
        assert_eq!(saved.status, "OPEN");
        assert_eq!(saved.reviewer_principal, "reviewer-1");
        let mut resolved = open.clone();
        resolved.status = FindingStatus::Resolved;
        resolved.resolved_by = "operator".into();
        resolved.evidence_refs = vec!["test:export_closes_file".into()];
        let saved = db.save_finding_disposition(&resolved).await.unwrap();
        assert_eq!(saved.status, "RESOLVED");
        assert_eq!(saved.evidence_refs.len(), 1);
        let replay = db.save_finding_disposition(&resolved).await.unwrap();
        assert_eq!(replay.id, saved.id);
        let mut changed_evidence = resolved.clone();
        changed_evidence.evidence_refs = vec!["different".into()];
        assert!(
            db.save_finding_disposition(&changed_evidence)
                .await
                .is_err()
        );
        let mut changed_route = resolved.clone();
        changed_route.route = FindingRoute::Planner;
        assert!(db.save_finding_disposition(&changed_route).await.is_err());
        assert_eq!(
            db.finding_dispositions("request-finding")
                .await
                .unwrap()
                .len(),
            1
        );

        // Refuting an existing OPEN finding follows the update path and must
        // persist both the authenticated owner principal and the decision.
        let open_again = FindingDisposition {
            request_id: "request-finding".into(),
            finding_id: "finding-refute".into(),
            route: FindingRoute::SecurityReviewer,
            status: FindingStatus::Open,
            candidate_id: "candidate-1".into(),
            reviewer_principal: "reviewer-1".into(),
            resolved_by: String::new(),
            evidence_refs: Vec::new(),
            owner_principal: None,
            owner_decision: None,
        };
        db.save_finding_disposition(&open_again).await.unwrap();
        let mut refuted = open_again.clone();
        refuted.status = FindingStatus::Refuted;
        refuted.evidence_refs = vec!["owner-evidence".into()];
        refuted.owner_principal = Some("owner-1".into());
        refuted.owner_decision = Some("decision-1".into());
        let saved = db.save_finding_disposition(&refuted).await.unwrap();
        assert_eq!(saved.status, "REFUTED");
        assert_eq!(saved.owner_principal.as_deref(), Some("owner-1"));
        assert_eq!(saved.owner_decision.as_deref(), Some("decision-1"));
    }

    #[tokio::test]
    async fn openspec_archive_is_idempotent_and_immutable() {
        let db = seeded_db("request-archive").await;
        let projection = crate::planning::OpenSpecProjection {
            change_id: "add-validator".into(),
            revision: 1,
            proposal_sha256: "a".repeat(64),
            specs_sha256: "b".repeat(64),
            design_sha256: "c".repeat(64),
            tasks_sha256: "d".repeat(64),
            validated: true,
            archived: false,
        };
        db.save_openspec_projection("request-archive", &projection)
            .await
            .unwrap();
        let first = db
            .archive_openspec_projection("request-archive", &projection)
            .await
            .unwrap();
        assert!(first.record.archived);
        assert!(!first.already_archived);
        let replay = db
            .archive_openspec_projection("request-archive", &projection)
            .await
            .unwrap();
        assert_eq!(replay.record.id, first.record.id);
        assert_eq!(replay.record.updated_at, first.record.updated_at);
        assert!(replay.already_archived);

        let mut changed = projection.clone();
        changed.tasks_sha256 = "e".repeat(64);
        assert!(
            db.archive_openspec_projection("request-archive", &changed)
                .await
                .is_err()
        );

        let mut unvalidated = projection.clone();
        unvalidated.validated = false;
        assert!(
            db.archive_openspec_projection("request-archive", &unvalidated)
                .await
                .is_err()
        );

        let mut different_revision = projection.clone();
        different_revision.revision = 2;
        assert!(
            db.archive_openspec_projection("request-archive", &different_revision)
                .await
                .is_err()
        );

        // A changed projection for an already archived revision remains a
        // conflict, and the archived row is unchanged.
        let mut changed_identity = projection.clone();
        changed_identity.change_id = "add-other".into();
        assert!(
            db.archive_openspec_projection("request-archive", &changed_identity)
                .await
                .is_err()
        );
        let latest = db
            .latest_openspec_projection("request-archive")
            .await
            .unwrap();
        assert!(latest.archived);
        assert_eq!(latest.change_id, "add-validator");
        assert_eq!(latest.updated_at, first.record.updated_at);
    }

    #[tokio::test]
    async fn skill_packs_are_immutable_and_fresh() {
        let db = seeded_db("request-skill").await;
        let pack = crate::planning::skills::builtin_skill_pack("rust").unwrap();
        let first = db.save_skill_pack(&pack).await.unwrap();
        let replay = db.save_skill_pack(&pack).await.unwrap();
        assert_eq!(first.id, replay.id);
        assert_eq!(first.content_sha256.len(), 64);
        let latest = db.latest_skill_pack("rust").await.unwrap();
        assert_eq!(latest.id, first.id);
        assert_eq!(db.skill_packs().await.unwrap().len(), 1);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenSpecProjectionRecord {
    pub id: String,
    pub request_id: String,
    pub change_id: String,
    pub revision: u32,
    pub proposal_sha256: String,
    pub specs_sha256: String,
    pub design_sha256: String,
    pub tasks_sha256: String,
    pub validated: bool,
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSpecArchiveTransition {
    pub record: OpenSpecProjectionRecord,
    pub already_archived: bool,
}

impl Database {
    pub async fn save_openspec_projection(
        &self,
        request_id: &str,
        projection: &crate::planning::OpenSpecProjection,
    ) -> Result<OpenSpecProjectionRecord, Error> {
        projection
            .validate()
            .map_err(|error| Error::Validation(format!("OpenSpec projection: {error}")))?;
        let now = Utc::now().to_rfc3339();
        let _write = self.write_lock.lock().await;
        let existing: Option<OpenSpecExistingRow> = sqlx::query_as::query_as(
            "SELECT id,proposal_sha256,specs_sha256,design_sha256,tasks_sha256,validated,archived,created_at,updated_at
             FROM openspec_projections WHERE request_id=?1 AND revision=?2",
        )
        .bind(request_id)
        .bind(i64::from(projection.revision))
        .fetch_optional(&self.pool)
        .await?;
        if let Some((id, proposal, specs, design, tasks, validated, archived, created, updated)) =
            existing
        {
            if proposal != projection.proposal_sha256
                || specs != projection.specs_sha256
                || design != projection.design_sha256
                || tasks != projection.tasks_sha256
            {
                return Err(Error::Conflict(
                    "OpenSpec revision already has different content",
                ));
            }
            return Ok(OpenSpecProjectionRecord {
                id,
                request_id: request_id.to_string(),
                change_id: projection.change_id.clone(),
                revision: projection.revision,
                proposal_sha256: proposal,
                specs_sha256: specs,
                design_sha256: design,
                tasks_sha256: tasks,
                validated: validated != 0,
                archived: archived != 0,
                created_at: created,
                updated_at: updated,
            });
        }
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query::query(
            "INSERT INTO openspec_projections
             (id,request_id,change_id,revision,proposal_sha256,specs_sha256,design_sha256,
              tasks_sha256,validated,archived,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)",
        )
        .bind(&id)
        .bind(request_id)
        .bind(&projection.change_id)
        .bind(i64::from(projection.revision))
        .bind(&projection.proposal_sha256)
        .bind(&projection.specs_sha256)
        .bind(&projection.design_sha256)
        .bind(&projection.tasks_sha256)
        .bind(i64::from(projection.validated))
        .bind(i64::from(projection.archived))
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(OpenSpecProjectionRecord {
            id,
            request_id: request_id.to_string(),
            change_id: projection.change_id.clone(),
            revision: projection.revision,
            proposal_sha256: projection.proposal_sha256.clone(),
            specs_sha256: projection.specs_sha256.clone(),
            design_sha256: projection.design_sha256.clone(),
            tasks_sha256: projection.tasks_sha256.clone(),
            validated: projection.validated,
            archived: projection.archived,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub async fn latest_openspec_projection(
        &self,
        request_id: &str,
    ) -> Result<OpenSpecProjectionRecord, Error> {
        let row: Option<OpenSpecRow> = sqlx::query_as::query_as(
            "SELECT id,request_id,change_id,revision,proposal_sha256,specs_sha256,
                    design_sha256,tasks_sha256,validated,archived,created_at,updated_at
             FROM openspec_projections WHERE request_id=?1
             ORDER BY revision DESC,created_at DESC LIMIT 1",
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?;
        row.ok_or(Error::NotFound).and_then(openspec_from_row)
    }

    /// Move one validated OpenSpec projection to its archive state exactly once.
    ///
    /// The expected projection must match the durable row byte-for-byte in its
    /// identity and hashes. Replaying the same archive binding returns the same
    /// archived record without changing `updated_at`; a different binding,
    /// an unvalidated row, or a stale pre-archive binding is rejected. This is a
    /// projection transition only; it never accepts a candidate or closes a
    /// finding.
    pub async fn archive_openspec_projection(
        &self,
        request_id: &str,
        expected: &crate::planning::OpenSpecProjection,
    ) -> Result<OpenSpecArchiveTransition, Error> {
        expected
            .validate()
            .map_err(|error| Error::Validation(format!("OpenSpec projection: {error}")))?;
        if !expected.validated {
            return Err(Error::Conflict(
                "only a validated OpenSpec projection can be archived",
            ));
        }
        let now = Utc::now().to_rfc3339();
        let _write = self.write_lock.lock().await;
        let update = sqlx::query::query(
            "UPDATE openspec_projections SET archived=1,updated_at=?3
             WHERE request_id=?1 AND revision=?2 AND validated=1 AND archived=0
               AND change_id=?4 AND proposal_sha256=?5 AND specs_sha256=?6
               AND design_sha256=?7 AND tasks_sha256=?8",
        )
        .bind(request_id)
        .bind(i64::from(expected.revision))
        .bind(&now)
        .bind(&expected.change_id)
        .bind(&expected.proposal_sha256)
        .bind(&expected.specs_sha256)
        .bind(&expected.design_sha256)
        .bind(&expected.tasks_sha256)
        .execute(&self.pool)
        .await?;
        let row: Option<OpenSpecRow> = sqlx::query_as::query_as(
            "SELECT id,request_id,change_id,revision,proposal_sha256,specs_sha256,
                    design_sha256,tasks_sha256,validated,archived,created_at,updated_at
             FROM openspec_projections WHERE request_id=?1 AND revision=?2",
        )
        .bind(request_id)
        .bind(i64::from(expected.revision))
        .fetch_optional(&self.pool)
        .await?;
        let record = openspec_from_row(row.ok_or(Error::NotFound)?)?;
        if update.rows_affected() == 0 {
            if !record.validated {
                return Err(Error::Conflict(
                    "only a validated OpenSpec projection can be archived",
                ));
            }
            if !record.archived {
                return Err(Error::Conflict(
                    "OpenSpec projection changed since the archive binding",
                ));
            }
            if record.change_id != expected.change_id
                || record.proposal_sha256 != expected.proposal_sha256
                || record.specs_sha256 != expected.specs_sha256
                || record.design_sha256 != expected.design_sha256
                || record.tasks_sha256 != expected.tasks_sha256
            {
                return Err(Error::Conflict("archived OpenSpec projection is immutable"));
            }
            return Ok(OpenSpecArchiveTransition {
                record,
                already_archived: true,
            });
        }
        Ok(OpenSpecArchiveTransition {
            record,
            already_archived: false,
        })
    }
}

fn openspec_from_row(row: OpenSpecRow) -> Result<OpenSpecProjectionRecord, Error> {
    Ok(OpenSpecProjectionRecord {
        id: row.0,
        request_id: row.1,
        change_id: row.2,
        revision: u32::try_from(row.3).map_err(|_| Error::Invalid("stored OpenSpec revision"))?,
        proposal_sha256: row.4,
        specs_sha256: row.5,
        design_sha256: row.6,
        tasks_sha256: row.7,
        validated: row.8 != 0,
        archived: row.9 != 0,
        created_at: row.10,
        updated_at: row.11,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingDispositionRecord {
    pub id: String,
    pub request_id: String,
    pub candidate_id: Option<String>,
    pub finding_id: String,
    pub route: String,
    pub status: String,
    pub reviewer_principal: String,
    pub resolved_by: String,
    pub evidence_refs: Vec<String>,
    pub owner_principal: Option<String>,
    pub owner_decision: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Database {
    /// Persist a finding disposition. The first write creates the durable OPEN
    /// record; later writes may only close it, and the route/reviewer identity
    /// cannot be replaced by a caller.
    pub async fn save_finding_disposition(
        &self,
        disposition: &crate::planning::FindingDisposition,
    ) -> Result<FindingDispositionRecord, Error> {
        disposition
            .validate()
            .map_err(|error| Error::Validation(format!("finding disposition: {error}")))?;
        if disposition.request_id.is_empty() {
            return Err(Error::Validation(
                "finding disposition request id is required".into(),
            ));
        }
        let request_id = disposition.request_id.as_str();
        let evidence_refs_json = serde_json::to_string(&disposition.evidence_refs)
            .map_err(|_| Error::Validation("finding evidence encoding".into()))?;
        let now = Utc::now().to_rfc3339();
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let existing: Option<FindingDispositionRow> = sqlx::query_as::query_as(
            "SELECT id,request_id,candidate_id,finding_id,route,status,reviewer_principal,
                    resolved_by,evidence_refs_json,owner_principal,owner_decision,created_at,updated_at
             FROM finding_dispositions
             WHERE request_id=?1 AND finding_id=?2",
        )
        .bind(request_id)
        .bind(&disposition.finding_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let id = if let Some(existing_row) = existing {
            let existing = finding_from_row(existing_row)?;
            if existing.route != finding_route_code(disposition.route) {
                return Err(Error::Conflict("finding disposition route is immutable"));
            }
            if !disposition.reviewer_principal.is_empty()
                && existing.reviewer_principal != disposition.reviewer_principal
            {
                return Err(Error::Conflict(
                    "finding disposition reviewer identity is immutable",
                ));
            }
            if matches!(existing.status.as_str(), "RESOLVED" | "REFUTED") {
                let same = existing.status == finding_status_code(disposition.status)
                    && existing.resolved_by == disposition.resolved_by
                    && existing.evidence_refs == disposition.evidence_refs
                    && existing.owner_principal == disposition.owner_principal
                    && existing.owner_decision == disposition.owner_decision;
                if same {
                    transaction.commit().await?;
                    return Ok(existing);
                }
                return Err(Error::Conflict("a closed finding disposition is immutable"));
            }
            if disposition.status == crate::planning::FindingStatus::Open {
                transaction.commit().await?;
                return Ok(existing);
            }
            let updated = sqlx::query::query(
                "UPDATE finding_dispositions
                 SET status=?1,resolved_by=?2,evidence_refs_json=?3,owner_principal=?4,
                     owner_decision=?5,updated_at=?6
                 WHERE id=?7 AND status='OPEN'",
            )
            .bind(finding_status_code(disposition.status))
            .bind(&disposition.resolved_by)
            .bind(&evidence_refs_json)
            .bind(disposition.owner_principal.as_deref())
            .bind(disposition.owner_decision.as_deref())
            .bind(&now)
            .bind(&existing.id)
            .execute(&mut *transaction)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(Error::Conflict("finding disposition was already closed"));
            }
            existing.id
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            sqlx::query::query(
                "INSERT INTO finding_dispositions
                 (id,request_id,candidate_id,finding_id,route,status,reviewer_principal,
                  resolved_by,evidence_refs_json,owner_principal,owner_decision,created_at,updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?12)",
            )
            .bind(&id)
            .bind(request_id)
            .bind(if disposition.candidate_id.is_empty() {
                None
            } else {
                Some(disposition.candidate_id.as_str())
            })
            .bind(&disposition.finding_id)
            .bind(finding_route_code(disposition.route))
            .bind(finding_status_code(disposition.status))
            .bind(&disposition.reviewer_principal)
            .bind(&disposition.resolved_by)
            .bind(&evidence_refs_json)
            .bind(disposition.owner_principal.as_deref())
            .bind(disposition.owner_decision.as_deref())
            .bind(&now)
            .execute(&mut *transaction)
            .await?;
            id
        };
        let row: FindingDispositionRow = sqlx::query_as::query_as(
            "SELECT id,request_id,candidate_id,finding_id,route,status,reviewer_principal,
                    resolved_by,evidence_refs_json,owner_principal,owner_decision,created_at,updated_at
             FROM finding_dispositions WHERE id=?1",
        )
        .bind(&id)
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        finding_from_row(row)
    }

    pub async fn finding_dispositions(
        &self,
        request_id: &str,
    ) -> Result<Vec<FindingDispositionRecord>, Error> {
        let rows: Vec<FindingDispositionRow> = sqlx::query_as::query_as(
            "SELECT id,request_id,candidate_id,finding_id,route,status,reviewer_principal,
                    resolved_by,evidence_refs_json,owner_principal,owner_decision,created_at,updated_at
             FROM finding_dispositions WHERE request_id=?1
             ORDER BY created_at,finding_id",
        )
        .bind(request_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(finding_from_row).collect()
    }

    pub async fn finding_disposition(
        &self,
        request_id: &str,
        finding_id: &str,
    ) -> Result<FindingDispositionRecord, Error> {
        let row: Option<FindingDispositionRow> = sqlx::query_as::query_as(
            "SELECT id,request_id,candidate_id,finding_id,route,status,reviewer_principal,
                    resolved_by,evidence_refs_json,owner_principal,owner_decision,created_at,updated_at
             FROM finding_dispositions WHERE request_id=?1 AND finding_id=?2",
        )
        .bind(request_id)
        .bind(finding_id)
        .fetch_optional(&self.pool)
        .await?;
        row.ok_or(Error::NotFound).and_then(finding_from_row)
    }
}

type FindingDispositionRow = (
    String,
    String,
    Option<String>,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
);

fn finding_route_code(route: crate::planning::FindingRoute) -> &'static str {
    crate::planning::findings::route_code(route)
}

fn finding_status_code(status: crate::planning::FindingStatus) -> &'static str {
    use crate::planning::FindingStatus;
    match status {
        FindingStatus::Open => "OPEN",
        FindingStatus::Resolved => "RESOLVED",
        FindingStatus::Refuted => "REFUTED",
    }
}

fn finding_from_row(row: FindingDispositionRow) -> Result<FindingDispositionRecord, Error> {
    Ok(FindingDispositionRecord {
        id: row.0,
        request_id: row.1,
        candidate_id: row.2,
        finding_id: row.3,
        route: row.4,
        status: row.5,
        reviewer_principal: row.6,
        resolved_by: row.7,
        evidence_refs: serde_json::from_str(&row.8)
            .map_err(|_| Error::Invalid("stored finding evidence is invalid"))?,
        owner_principal: row.9,
        owner_decision: row.10,
        created_at: row.11,
        updated_at: row.12,
    })
}
