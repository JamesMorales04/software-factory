//! Test-support fixture loader for WP10 real-browser acceptance.
//!
//! This module exists only in debug builds compiled with the `test-support`
//! feature. It creates exact durable rows (requests, planning artifacts, a
//! frozen spec, reviewer receipts, findings, owner decision, workflow trace)
//! so Chromium can exercise the real Core HTTP API without paid inference.
//!
//! It never runs in a release build: `native/src/lib.rs` rejects the feature
//! outside debug builds. Seeded state is explicitly labelled in the WP10
//! evidence; it is not model output and it grants no authority.

use chrono::{Duration, Utc};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{Database, Error};
use crate::agentic::{self, PlannedWorkUnit, SpecDraftModel, SpecRequirementDraft, WorkGraphModel};
use crate::db::NewAgenticSpecDraft;
use crate::db::NewAgenticWorkGraph;
use crate::planning::{FindingDisposition, FindingRoute, FindingStatus, OpenSpecProjection};

const PROJECT_ID: &str = "project-wp10-browser";
const REPOSITORY: &str = "/tmp/factory-wp10-browser-repository";
const PROFILE_ID: &str = "profile-wp10-browser-r2";

pub const READY_REQUEST: &str = "11111111-1111-4111-8111-111111111111";
pub const MISSING_REQUEST: &str = "22222222-2222-4222-8222-222222222222";
pub const REJECT_REQUEST: &str = "33333333-3333-4333-8333-333333333333";
pub const STALE_REQUEST: &str = "44444444-4444-4444-8444-444444444444";
pub const MISMATCH_REQUEST: &str = "55555555-5555-4555-8555-555555555555";
pub const TRACE_POPULATED_REQUEST: &str = "66666666-6666-4666-8666-666666666666";
pub const TRACE_EMPTY_REQUEST: &str = "77777777-7777-4777-8777-777777777777";
pub const TRACE_LONG_REQUEST: &str = "88888888-8888-4888-8888-888888888888";
pub const TRACE_REJECTED_REQUEST: &str = "99999999-9999-4999-8999-999999999999";
pub const TRACE_DUPLICATE_REQUEST: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
pub const TRACE_RETRY_REQUEST: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
pub const TRACE_GAP_REQUEST: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

fn sha256_hex(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn words(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlanVariant {
    Ready,
    MissingDod,
    Reject,
    Stale,
    ReviewMismatch,
}

impl Database {
    /// Seed every WP10 browser fixture exactly once. The owner principal is the
    /// authenticated browser device principal, used for the resolved owner
    /// decision that permits a real finding refutation through Core.
    pub async fn seed_wave12_fixtures(&self, owner_principal: &str) -> Result<Value, Error> {
        if owner_principal.trim().is_empty() {
            return Err(Error::Validation(
                "wave12 fixture owner principal is required".into(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        self.ensure_wave12_project(&now).await?;
        self.ensure_wave12_profile_v2(&now).await?;
        self.ensure_wave12_profile_invalid(&now).await?;

        self.seed_plan_request(
            READY_REQUEST,
            "Approve the formal plan with reviewer independence and open findings",
            PlanVariant::Ready,
            owner_principal,
            &now,
        )
        .await?;
        self.seed_plan_request(
            MISSING_REQUEST,
            "Show a missing Definition of Done artifact",
            PlanVariant::MissingDod,
            owner_principal,
            &now,
        )
        .await?;
        self.seed_plan_request(
            REJECT_REQUEST,
            "Reject the plan through the owner review",
            PlanVariant::Reject,
            owner_principal,
            &now,
        )
        .await?;
        self.seed_plan_request(
            STALE_REQUEST,
            "Refuse a stale plan hash at the exact-review guard",
            PlanVariant::Stale,
            owner_principal,
            &now,
        )
        .await?;
        self.seed_plan_request(
            MISMATCH_REQUEST,
            "Show the frozen reviewer assignment mismatch",
            PlanVariant::ReviewMismatch,
            owner_principal,
            &now,
        )
        .await?;

        self.seed_wave12_traces(&now).await?;
        self.seed_wave12_skills(&now).await?;

        Ok(json!({
            "seeded": true,
            "authority": "test-support fixture; no paid inference and no model authority",
            "ownerPrincipal": owner_principal,
            "requests": {
                "planReady": READY_REQUEST,
                "planMissing": MISSING_REQUEST,
                "planReject": REJECT_REQUEST,
                "planStale": STALE_REQUEST,
                "reviewMismatch": MISMATCH_REQUEST,
                "tracePopulated": TRACE_POPULATED_REQUEST,
                "traceEmpty": TRACE_EMPTY_REQUEST,
                "traceLong": TRACE_LONG_REQUEST,
                "traceRejected": TRACE_REJECTED_REQUEST,
                "traceDuplicate": TRACE_DUPLICATE_REQUEST,
                "traceRetry": TRACE_RETRY_REQUEST,
                "traceGap": TRACE_GAP_REQUEST,
            },
            "findings": {
                "resolve": "finding-resolve-001",
                "refute": "finding-refute-001",
            },
            "ownerDecision": "decision-refute-001",
            "reviewerRoles": [
                "FUNCTIONAL", "ARCHITECTURE", "CODE_QUALITY", "SECURITY", "TESTING"
            ],
            "missingReceiptRole": "TESTING",
            "changesRequestedRole": "CODE_QUALITY",
            "failingRole": "SECURITY",
            "skills": {
                "refreshable": ["rust", "typescript"],
                "blocked": ["legacy-tool"]
            }
        }))
    }

    async fn ensure_wave12_project(&self, now: &str) -> Result<(), Error> {
        let base_commit = "a".repeat(40);
        let config_hash = sha256_hex("wave12 project config");
        sqlx::query::query(
            "INSERT OR IGNORE INTO projects
             (id,repository,base_commit,created_at,identity,name,source_kind,source_url,
              source_key,source_visibility,default_branch,remote_origin)
             VALUES (?1,?2,?3,?4,'fixture','WP10 browser fixture','local','',
                     'wp10-browser-fixture','','main','')",
        )
        .bind(PROJECT_ID)
        .bind(REPOSITORY)
        .bind(&base_commit)
        .bind(now)
        .execute(&self.pool)
        .await?;
        sqlx::query::query(
            "INSERT OR IGNORE INTO project_configs
             (project_id,revision,config_hash,repository,base_commit,allowed_paths_json,
              capabilities_json,quality_profile,active,updated_at)
             VALUES (?1,1,?2,?3,?4,'[]','[\"repository-edit\"]','generic',1,?5)",
        )
        .bind(PROJECT_ID)
        .bind(&config_hash)
        .bind(REPOSITORY)
        .bind(&base_commit)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn ensure_wave12_profile_v2(&self, now: &str) -> Result<(), Error> {
        use std::num::NonZeroU32;

        use crate::execution_policy::{OperationLimits, ProgressPolicy};
        use crate::execution_profile::{
            AutoStartPolicy, ExecutionProfileCategory, ExecutionProfileContent,
            ExecutionProfileRoles, ExecutionRoleRoute,
        };

        let route = |provider: &str, model: &str| ExecutionRoleRoute {
            provider: provider.into(),
            model: model.into(),
            reasoning: None,
        };
        let content = ExecutionProfileContent {
            name: "WP10 browser fixture".into(),
            description: "Deterministic profile content for browser acceptance".into(),
            category: ExecutionProfileCategory::Balanced,
            roles: ExecutionProfileRoles {
                lead: route("opencode-go", "deepseek-v4.1-flash"),
                implementer: route("opencode-go", "deepseek-v4.1-flash"),
                semantic_reviewer: route("codex", "gpt-6-luna"),
            },
            operations: OperationLimits {
                model_request_timeout_secs: NonZeroU32::new(300).unwrap(),
                tool_call_timeout_secs: NonZeroU32::new(600).unwrap(),
                max_request_bytes: NonZeroU32::new(2 * 1024 * 1024).unwrap(),
                max_response_bytes: NonZeroU32::new(2 * 1024 * 1024).unwrap(),
                max_generation_tokens: NonZeroU32::new(8_192).unwrap(),
                max_in_flight_operations: NonZeroU32::new(4).unwrap(),
            },
            progress: ProgressPolicy {
                diagnose_after_stagnant_checkpoints: NonZeroU32::new(2).unwrap(),
                escalate_after_unsuccessful_replans: NonZeroU32::new(1).unwrap(),
                observation_timeout_secs: NonZeroU32::new(300).unwrap(),
                autonomy: Default::default(),
            },
            auto_start: AutoStartPolicy {
                enabled: false,
                risk_classes: Vec::new(),
            },
        };
        content
            .validate()
            .map_err(|_| Error::Validation("wave12 profile content is invalid".into()))?;
        let content = serde_json::to_string(&content)
            .map_err(|_| Error::Validation("wave12 profile encoding".into()))?;
        let content_hash = sha256_hex(&content);
        sqlx::query::query(
            "INSERT OR IGNORE INTO execution_profiles
             (id,current_revision,content_hash,created_at,updated_at)
             VALUES (?1,1,?2,?3,?3)",
        )
        .bind(PROFILE_ID)
        .bind(&content_hash)
        .bind(now)
        .execute(&self.pool)
        .await?;
        sqlx::query::query(
            "INSERT OR IGNORE INTO execution_profile_revisions
             (revision_id,profile_id,revision,content_json,content_hash,actor,created_at,
              schema_version)
             VALUES (?1,?2,1,?3,?4,'fixture',?5,2)",
        )
        .bind(format!("{PROFILE_ID}-r1"))
        .bind(PROFILE_ID)
        .bind(&content)
        .bind(&content_hash)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// A deliberately undecodable profile revision. It makes the deterministic
    /// participant compiler fail so the projection falls back to a plan with
    /// required reviewer roles but no principal assignments; the Office must
    /// render the fail-closed mismatch warning.
    async fn ensure_wave12_profile_invalid(&self, now: &str) -> Result<(), Error> {
        let content = "{}".to_string();
        let content_hash = sha256_hex(&content);
        sqlx::query::query(
            "INSERT OR IGNORE INTO execution_profiles
             (id,current_revision,content_hash,created_at,updated_at)
             VALUES ('profile-wp10-browser-invalid',1,?1,?2,?2)",
        )
        .bind(&content_hash)
        .bind(now)
        .execute(&self.pool)
        .await?;
        sqlx::query::query(
            "INSERT OR IGNORE INTO execution_profile_revisions
             (revision_id,profile_id,revision,content_json,content_hash,actor,created_at,
              schema_version)
             VALUES ('profile-wp10-browser-invalid-r1','profile-wp10-browser-invalid',1,
                     ?1,?2,'fixture',?3,1)",
        )
        .bind(&content)
        .bind(&content_hash)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn seed_plan_request(
        &self,
        request_id: &str,
        objective: &str,
        variant: PlanVariant,
        owner_principal: &str,
        now: &str,
    ) -> Result<(), Error> {
        let profile_id = if variant == PlanVariant::ReviewMismatch {
            "profile-wp10-browser-invalid"
        } else {
            PROFILE_ID
        };
        let profile_revision_id = format!("{profile_id}-r1");
        let profile_hash = sha256_hex(profile_id);
        self.insert_request_base(
            request_id,
            objective,
            profile_id,
            &profile_revision_id,
            &profile_hash,
            now,
        )
        .await?;

        let graph = self.insert_work_graph(request_id, objective, now).await?;
        if variant != PlanVariant::ReviewMismatch {
            self.insert_artifacts(request_id, variant, now).await?;
            self.insert_frozen_spec(request_id, objective, &graph, now)
                .await?;
            self.insert_openspec(request_id, now).await?;
        }
        let plan_hash = if variant == PlanVariant::ReviewMismatch {
            sha256_hex(&format!("{request_id}-unused-plan"))
        } else {
            let plan = self
                .planning_artifact(request_id, "PLAN", 1)
                .await
                .map_err(|_| Error::NotFound)?;
            let stale_hash = "0".repeat(64);
            self.insert_pending_review(
                request_id,
                plan.revision,
                if variant == PlanVariant::Stale {
                    // A corrupted/stale review row that does not bind the
                    // persisted PLAN hash. Core must reject the approval.
                    &stale_hash
                } else {
                    &plan.content_sha256
                },
                now,
            )
            .await?;
            plan.content_sha256
        };
        self.insert_workflow_run(
            request_id,
            &format!("workflow-{request_id}"),
            "PLAN_REVIEW",
            &plan_hash,
            None,
            now,
        )
        .await?;

        if variant != PlanVariant::ReviewMismatch {
            self.insert_campaign_receipts(request_id, now).await?;
        }
        if variant == PlanVariant::Ready {
            self.insert_findings(request_id, owner_principal, now)
                .await?;
        }
        Ok(())
    }

    async fn insert_request_base(
        &self,
        request_id: &str,
        objective: &str,
        profile_id: &str,
        profile_revision_id: &str,
        profile_hash: &str,
        now: &str,
    ) -> Result<(), Error> {
        let action_id = format!("action-{request_id}");
        let delegation_id = format!("delegation-{request_id}");
        let lead_id = format!("lead-{request_id}");
        let submission = json!({"mode": "REVIEW_PLAN", "fixture": "wp10-browser"}).to_string();
        let submission_hash = sha256_hex(&submission);
        sqlx::query::query(
            "INSERT OR IGNORE INTO actions
             (id,project_id,objective,provider,model,risk,status,objective_sha256,created_at)
             VALUES (?1,?2,?3,'opencode-go','deepseek-v4.1-flash','R1','AUTHORIZED',?4,?5)",
        )
        .bind(&action_id)
        .bind(PROJECT_ID)
        .bind(objective)
        .bind(sha256_hex(objective))
        .bind(now)
        .execute(&self.pool)
        .await?;
        sqlx::query::query(
            "INSERT OR IGNORE INTO standing_delegations
             (id,principal_id,audit_id,project_id,revision,content_hash,content_json,created_at)
             VALUES (?1,'owner','fixture',?2,1,?3,'{}',?4)",
        )
        .bind(&delegation_id)
        .bind(PROJECT_ID)
        .bind(sha256_hex(&delegation_id))
        .bind(now)
        .execute(&self.pool)
        .await?;
        sqlx::query::query(
            "INSERT OR IGNORE INTO lead_requests
             (id,project_id,action_id,delegation_id,objective,request_hash,
              project_config_hash,state,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,'FROZEN',?8,?8)",
        )
        .bind(&lead_id)
        .bind(PROJECT_ID)
        .bind(&action_id)
        .bind(&delegation_id)
        .bind(objective)
        .bind(sha256_hex(&format!("{request_id}-request")))
        .bind(sha256_hex(&format!("{request_id}-config")))
        .bind(now)
        .execute(&self.pool)
        .await?;
        sqlx::query::query(
            "INSERT OR IGNORE INTO intake_requests
             (id,project_id,objective,mode,profile_id,profile_revision_id,profile_revision,
              profile_hash,profile_name,action_id,delegation_id,lead_request_id,campaign_id,
              submission_json,submission_hash,idempotency_key,created_at,updated_at)
             VALUES (?1,?2,?3,'REVIEW_PLAN',?4,?5,1,?6,'WP10 browser fixture',?7,?8,?9,
                     NULL,?10,?11,?12,?13,?13)",
        )
        .bind(request_id)
        .bind(PROJECT_ID)
        .bind(objective)
        .bind(profile_id)
        .bind(profile_revision_id)
        .bind(profile_hash)
        .bind(&action_id)
        .bind(&delegation_id)
        .bind(&lead_id)
        .bind(&submission)
        .bind(&submission_hash)
        .bind(format!("idempotency-{request_id}"))
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn insert_work_graph(
        &self,
        request_id: &str,
        objective: &str,
        now: &str,
    ) -> Result<agentic::ValidatedWorkGraph, Error> {
        let model = WorkGraphModel {
            units: vec![PlannedWorkUnit {
                id: "unit-review".into(),
                title: "Reviewer-visible plan unit".into(),
                objective: words(objective, 400),
                depends_on: Vec::new(),
                paths: vec!["src/lib.rs".into()],
                acceptance: vec!["The browser can inspect the exact plan revision.".into()],
            }],
            assumptions: vec!["Seeded fixture data, not model output.".into()],
            risks: vec!["Fixture content is labelled.".into()],
        };
        let graph = agentic::validate_work_graph(&model, &[])
            .map_err(|error| Error::Validation(format!("wave12 work graph: {error}")))?;
        self.save_agentic_work_graph(&NewAgenticWorkGraph {
            request_id,
            graph: &graph,
            provider: "fixture",
            model: "wp10-browser",
            schema_sha256: &sha256_hex("wave12 work graph schema"),
            request_sha256: &sha256_hex(request_id),
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            cost_micros: 0,
            usage_known: false,
        })
        .await?;
        let _ = now;
        Ok(graph)
    }

    async fn insert_artifacts(
        &self,
        request_id: &str,
        variant: PlanVariant,
        _now: &str,
    ) -> Result<(), Error> {
        let plan = json!({
            "summary": "FORMAL_SPEC plan reviewed by the Product Owner.",
            "lane": "FORMAL_SPEC",
            "units": ["unit-review"]
        })
        .to_string();
        let artifacts: Vec<(&str, String)> = vec![
            ("PLAN", plan),
            (
                "STACK",
                json!({
                    "selected": "rust",
                    "source": "EXISTING",
                    "requiredCapabilities": ["repository-edit"]
                })
                .to_string(),
            ),
            (
                "ARCHITECTURE",
                json!({
                    "components": ["Core", "Office"],
                    "decisions": ["ADR-0039"]
                })
                .to_string(),
            ),
            (
                "DOCUMENTATION",
                json!({
                    "required": [
                        {"document": "README.md", "tier": "baseline"},
                        {"document": "docs/architecture.md", "tier": "architecture"}
                    ]
                })
                .to_string(),
            ),
            (
                "SKILL",
                json!({"needs": ["rust", "react"], "proposed": []}).to_string(),
            ),
            (
                "DOD",
                json!({
                    "projectChecks": [
                        {"id": "fmt", "description": "cargo fmt --check"},
                        {"id": "test", "description": "cargo test"}
                    ]
                })
                .to_string(),
            ),
            (
                "OPENSPEC",
                json!({"changeId": "add-wave12-plan-review", "revision": 1}).to_string(),
            ),
        ];
        for (kind, content) in artifacts {
            if variant == PlanVariant::MissingDod && kind == "DOD" {
                continue;
            }
            self.save_planning_artifact(request_id, kind, 1, &content)
                .await?;
        }
        Ok(())
    }

    async fn insert_frozen_spec(
        &self,
        request_id: &str,
        objective: &str,
        graph: &agentic::ValidatedWorkGraph,
        now: &str,
    ) -> Result<(), Error> {
        let draft = SpecDraftModel {
            summary: words(objective, 400),
            requirements: vec![SpecRequirementDraft {
                statement: "The Office shows the exact plan revision and review controls.".into(),
                work_unit_id: graph
                    .units
                    .first()
                    .map(|unit| unit.id.clone())
                    .unwrap_or_default(),
                acceptance: vec!["The plan review panel renders the revision.".into()],
                negative: vec!["A stale plan hash is rejected.".into()],
            }],
            assumptions: vec!["Seeded spec fixture.".into()],
            risks: vec![],
        };
        let frozen = agentic::compile_spec(&draft, 1)
            .map_err(|error| Error::Validation(format!("wave12 spec: {error}")))?;
        let draft_json = serde_json::to_string(&draft)
            .map_err(|_| Error::Validation("wave12 spec draft encoding".into()))?;
        let record = self
            .save_agentic_spec_draft(&NewAgenticSpecDraft {
                request_id,
                draft_json: &draft_json,
                graph_sha256: Some(&graph.graph_sha256),
                provider: "fixture",
                model: "wp10-browser",
                schema_sha256: &sha256_hex("wave12 spec schema"),
                request_sha256: &sha256_hex(request_id),
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
                cost_micros: 0,
                usage_known: false,
            })
            .await?;
        self.freeze_agentic_spec(&record.id, &frozen, graph).await?;
        let _ = now;
        Ok(())
    }

    async fn insert_openspec(&self, request_id: &str, _now: &str) -> Result<(), Error> {
        let projection = OpenSpecProjection {
            change_id: "add-wave12-plan-review".into(),
            revision: 1,
            proposal_sha256: "1".repeat(64),
            specs_sha256: "2".repeat(64),
            design_sha256: "3".repeat(64),
            tasks_sha256: "4".repeat(64),
            validated: true,
            archived: false,
        };
        self.save_openspec_projection(request_id, &projection)
            .await?;
        Ok(())
    }

    async fn insert_pending_review(
        &self,
        request_id: &str,
        revision: u32,
        plan_sha256: &str,
        now: &str,
    ) -> Result<(), Error> {
        sqlx::query::query(
            "INSERT OR IGNORE INTO plan_reviews
             (id,request_id,plan_revision,plan_sha256,status,principal_id,rationale,points,
              reviewed_at)
             VALUES (?1,?2,?3,?4,'PENDING','core:fixture',
                     'Awaiting the authenticated Product Owner review.',0,?5)",
        )
        .bind(format!("review-{request_id}"))
        .bind(request_id)
        .bind(i64::from(revision))
        .bind(plan_sha256)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn insert_workflow_run(
        &self,
        request_id: &str,
        run_id: &str,
        state: &str,
        plan_sha256: &str,
        campaign_id: Option<&str>,
        now: &str,
    ) -> Result<(), Error> {
        sqlx::query::query(
            "INSERT OR IGNORE INTO workflow_runs
             (id,request_id,campaign_id,state,revision,plan_sha256,created_at,updated_at)
             VALUES (?1,?2,?3,?4,1,?5,?6,?6)",
        )
        .bind(run_id)
        .bind(request_id)
        .bind(campaign_id)
        .bind(state)
        .bind(plan_sha256)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn insert_campaign_receipts(&self, request_id: &str, now: &str) -> Result<(), Error> {
        let campaign_id = format!("campaign-{request_id}");
        let run_id = format!("run-{request_id}");
        let attempt_id = format!("attempt-{request_id}");
        let candidate_id = format!("candidate-{request_id}");
        let action_id = format!("action-{request_id}");
        sqlx::query::query(
            "INSERT OR IGNORE INTO campaigns(id,action_id,status,created_at)
             VALUES (?1,?2,'RUNNING',?3)",
        )
        .bind(&campaign_id)
        .bind(&action_id)
        .bind(now)
        .execute(&self.pool)
        .await?;
        sqlx::query::query(
            "INSERT OR IGNORE INTO runs(id,campaign_id,status,created_at)
             VALUES (?1,?2,'COMPLETED',?3)",
        )
        .bind(&run_id)
        .bind(&campaign_id)
        .bind(now)
        .execute(&self.pool)
        .await?;
        sqlx::query::query(
            "INSERT OR IGNORE INTO attempts(id,run_id,status,created_at,sandbox_ready)
             VALUES (?1,?2,'SUCCEEDED',?3,1)",
        )
        .bind(&attempt_id)
        .bind(&run_id)
        .bind(now)
        .execute(&self.pool)
        .await?;
        sqlx::query::query(
            "INSERT OR IGNORE INTO candidates
             (id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at)
             VALUES (?1,?2,?3,?4,?5,'implementer-fixture',?6)",
        )
        .bind(&candidate_id)
        .bind(&attempt_id)
        .bind(sha256_hex(&format!("{request_id}-action-spec")))
        .bind("b".repeat(40))
        .bind("c".repeat(40))
        .bind(now)
        .execute(&self.pool)
        .await?;
        sqlx::query::query("UPDATE lead_requests SET campaign_id=?1 WHERE id=?2")
            .bind(&campaign_id)
            .bind(format!("lead-{request_id}"))
            .execute(&self.pool)
            .await?;
        sqlx::query::query("UPDATE intake_requests SET campaign_id=?1 WHERE id=?2")
            .bind(&campaign_id)
            .bind(request_id)
            .execute(&self.pool)
            .await?;

        // Five reviewer identities. TESTING intentionally has no receipt;
        // CODE_QUALITY requests changes and SECURITY fails so the Director can
        // render result, evidence and tone for every receipt state.
        let receipts = [
            ("FUNCTIONAL", "reviewer-functional", "PASS"),
            ("ARCHITECTURE", "reviewer-architecture", "PASS"),
            ("CODE_QUALITY", "reviewer-code-quality", "CHANGES_REQUESTED"),
            ("SECURITY", "reviewer-security", "FAIL"),
        ];
        for (role, principal, result) in receipts {
            sqlx::query::query(
                "INSERT OR IGNORE INTO quality_receipts
                 (receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,
                  tree_sha,result,evidence,received_at,findings_json,mechanism,reviewer_role)
                 VALUES (?1,?2,'reviewer',?3,?4,?5,?6,?7,'EXECUTED',?8,'[]',
                         'DETERMINISTIC',?9)",
            )
            .bind(format!("receipt-{request_id}-{role}"))
            .bind(&candidate_id)
            .bind(principal)
            .bind(sha256_hex(&format!("{request_id}-action-spec")))
            .bind("b".repeat(40))
            .bind("c".repeat(40))
            .bind(result)
            .bind(now)
            .bind(role)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn insert_findings(
        &self,
        request_id: &str,
        owner_principal: &str,
        now: &str,
    ) -> Result<(), Error> {
        let candidate_id = format!("candidate-{request_id}");
        for (finding_id, route) in [
            ("finding-resolve-001", FindingRoute::Repairer),
            ("finding-refute-001", FindingRoute::SecurityReviewer),
        ] {
            self.save_finding_disposition(&FindingDisposition {
                request_id: request_id.into(),
                finding_id: finding_id.into(),
                route,
                status: FindingStatus::Open,
                candidate_id: candidate_id.clone(),
                reviewer_principal: "reviewer-functional".into(),
                resolved_by: String::new(),
                evidence_refs: Vec::new(),
                owner_principal: None,
                owner_decision: None,
            })
            .await?;
        }
        let deadline = (Utc::now() + Duration::days(1)).to_rfc3339();
        sqlx::query::query(
            "INSERT OR IGNORE INTO agentic_owner_decisions
             (id,project_id,request_id,campaign_id,matter,risk,status,options_json,
              evidence_json,recommendation,deadline,selected_option,rationale,resolved_by,
              created_at,updated_at,resolved_at)
             VALUES (?1,?2,?3,NULL,'FINDING_REFUTATION','R1','APPROVED','[\"APPROVE_REFUTATION\"]','[]',
                     'Refute the finding with the recorded owner decision.',?4,
                     'APPROVE_REFUTATION',
                     'The owner confirmed the reviewer finding is invalid.',
                     ?5,?6,?6,?6)",
        )
        .bind("decision-refute-001")
        .bind(PROJECT_ID)
        .bind(request_id)
        .bind(&deadline)
        .bind(owner_principal)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn seed_wave12_traces(&self, now: &str) -> Result<(), Error> {
        for (request, objective) in [
            (TRACE_POPULATED_REQUEST, "Workflow trace populated fixture"),
            (TRACE_LONG_REQUEST, "Workflow trace long fixture"),
            (TRACE_REJECTED_REQUEST, "Workflow trace rejected fixture"),
            (TRACE_RETRY_REQUEST, "Workflow trace retry fixture"),
        ] {
            let profile_hash = sha256_hex(PROFILE_ID);
            self.insert_request_base(
                request,
                objective,
                PROFILE_ID,
                &format!("{PROFILE_ID}-r1"),
                &profile_hash,
                now,
            )
            .await?;
        }
        // Empty trace: the request exists but has no durable workflow run.
        self.insert_request_base(
            TRACE_EMPTY_REQUEST,
            "Workflow trace empty fixture",
            PROFILE_ID,
            &format!("{PROFILE_ID}-r1"),
            &sha256_hex(PROFILE_ID),
            now,
        )
        .await?;
        self.insert_request_base(
            TRACE_GAP_REQUEST,
            "Workflow trace missing-transition fixture",
            PROFILE_ID,
            &format!("{PROFILE_ID}-r1"),
            &sha256_hex(PROFILE_ID),
            now,
        )
        .await?;
        // Duplicate-sequence trace is returned by the test-support projection
        // override because the durable ledger primary key cannot hold a
        // duplicate sequence. The request itself is still a real Core row.
        self.insert_request_base(
            TRACE_DUPLICATE_REQUEST,
            "Workflow trace duplicate-sequence fixture",
            PROFILE_ID,
            &format!("{PROFILE_ID}-r1"),
            &sha256_hex(PROFILE_ID),
            now,
        )
        .await?;

        let populated = format!("workflow-{TRACE_POPULATED_REQUEST}");
        self.insert_workflow_run(
            TRACE_POPULATED_REQUEST,
            &populated,
            "ACCEPTED",
            &"d".repeat(64),
            None,
            now,
        )
        .await?;
        let transitions = [
            ("SUBMITTED", "ASSESSING"),
            ("ASSESSING", "PLANNED"),
            ("PLANNED", "PLAN_REVIEW"),
            ("PLAN_REVIEW", "FROZEN"),
            ("FROZEN", "EXECUTING"),
            ("EXECUTING", "VERIFYING"),
            ("VERIFYING", "REVIEWING"),
            ("REVIEWING", "GATE"),
            ("GATE", "CONVERGING"),
            ("CONVERGING", "ACCEPTED"),
        ];
        let mut sequence = 100_000_i64;
        for (from, to) in transitions {
            let detail = json!({"from": from, "to": to, "fixture": "wp10-browser"}).to_string();
            self.insert_event(sequence, &populated, "workflow.transitioned", &detail, now)
                .await?;
            sequence += 1;
            if from == "FROZEN" {
                let step_one = "step-66666666-6666-4666-8666-666666666661".to_string();
                let step_two = "step-66666666-6666-4666-8666-666666666662".to_string();
                self.insert_step(&step_one, &populated, "browser-check", "SUCCEEDED", 1, now)
                    .await?;
                self.insert_step(&step_two, &populated, "browser-check", "RUNNING", 2, now)
                    .await?;
                self.insert_event(
                    sequence,
                    &populated,
                    "workflow.step.started",
                    &json!({"stepId": step_one, "kind": "browser-check"}).to_string(),
                    now,
                )
                .await?;
                sequence += 1;
                self.insert_event(
                    sequence,
                    &populated,
                    "workflow.step.succeeded",
                    &json!({"stepId": step_one, "status": "SUCCEEDED"}).to_string(),
                    now,
                )
                .await?;
                sequence += 1;
                self.insert_event(
                    sequence,
                    &populated,
                    "workflow.step.started",
                    &json!({"stepId": step_two, "kind": "browser-check"}).to_string(),
                    now,
                )
                .await?;
                sequence += 1;
            }
        }
        // A malformed transition row and a secret/prompt-shaped payload. Core
        // must redact every value before the browser sees it.
        self.insert_event(
            sequence,
            &populated,
            "workflow.transitioned",
            "{\"from\": \"ACCEPTED\", ",
            now,
        )
        .await?;
        sequence += 1;
        self.insert_event(
            sequence,
            &populated,
            "workflow.checkpoint.recorded",
            &json!({
                "token": "sk-proj-wp10-never-render-this",
                "prompt": "raw prompt never rendered",
                "content": "secret content never rendered",
                "apiKey": "api-key-never-rendered",
                "unknownField": "unknown-value-never-rendered",
                "state": "raw prompt or token",
                "sequence": 123,
                "terminal": true
            })
            .to_string(),
            now,
        )
        .await?;

        let long_run = format!("workflow-{TRACE_LONG_REQUEST}");
        self.insert_workflow_run(
            TRACE_LONG_REQUEST,
            &long_run,
            "ACCEPTED",
            &"e".repeat(64),
            None,
            now,
        )
        .await?;
        let mut long_sequence = 200_000_i64;
        for (from, to) in transitions {
            self.insert_event(
                long_sequence,
                &long_run,
                "workflow.transitioned",
                &json!({"from": from, "to": to}).to_string(),
                now,
            )
            .await?;
            long_sequence += 1;
        }
        for index in 0..203 {
            self.insert_event(
                long_sequence,
                &long_run,
                "workflow.checkpoint.recorded",
                &json!({"sequence": index, "terminal": false}).to_string(),
                now,
            )
            .await?;
            long_sequence += 1;
        }
        for index in 0..505 {
            let step_id = format!("step-{:08x}-0000-4000-8000-{:012x}", index, index);
            self.insert_step(
                &step_id,
                &long_run,
                "browser-check",
                "SUCCEEDED",
                index,
                now,
            )
            .await?;
        }

        let rejected_run = format!("workflow-{TRACE_REJECTED_REQUEST}");
        self.insert_workflow_run(
            TRACE_REJECTED_REQUEST,
            &rejected_run,
            "EXECUTING",
            &"f".repeat(64),
            None,
            now,
        )
        .await?;
        self.insert_event(300_000, &rejected_run, &"x".repeat(200), "{}", now)
            .await?;

        let retry_run = format!("workflow-{TRACE_RETRY_REQUEST}");
        self.insert_workflow_run(
            TRACE_RETRY_REQUEST,
            &retry_run,
            "PLAN_REVIEW",
            &"1".repeat(64),
            None,
            now,
        )
        .await?;
        self.insert_event(
            400_000,
            &retry_run,
            "workflow.transitioned",
            &json!({"from": "PLANNED", "to": "PLAN_REVIEW"}).to_string(),
            now,
        )
        .await?;

        let gap_run = format!("workflow-{TRACE_GAP_REQUEST}");
        self.insert_workflow_run(
            TRACE_GAP_REQUEST,
            &gap_run,
            "PLAN_REVIEW",
            &"2".repeat(64),
            None,
            now,
        )
        .await?;
        self.insert_event(
            500_000,
            &gap_run,
            "workflow.transitioned",
            &json!({"from": "SUBMITTED", "to": "ASSESSING"}).to_string(),
            now,
        )
        .await?;
        self.insert_event(
            500_001,
            &gap_run,
            "workflow.transitioned",
            &json!({"from": "PLANNED", "to": "PLAN_REVIEW"}).to_string(),
            now,
        )
        .await?;
        Ok(())
    }

    async fn insert_event(
        &self,
        sequence: i64,
        subject_id: &str,
        kind: &str,
        detail: &str,
        now: &str,
    ) -> Result<(), Error> {
        sqlx::query::query(
            "INSERT OR IGNORE INTO events(sequence,subject_id,kind,detail,recorded_at)
             VALUES (?1,?2,?3,?4,?5)",
        )
        .bind(sequence)
        .bind(subject_id)
        .bind(kind)
        .bind(detail)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn insert_step(
        &self,
        id: &str,
        run_id: &str,
        kind: &str,
        status: &str,
        attempt: i64,
        now: &str,
    ) -> Result<(), Error> {
        sqlx::query::query(
            "INSERT OR IGNORE INTO workflow_steps
             (id,run_id,kind,status,attempt,input_sha256,output_sha256,detail,created_at,
              updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,'',?8,?8)",
        )
        .bind(id)
        .bind(run_id)
        .bind(kind)
        .bind(status)
        .bind(attempt)
        .bind("a".repeat(64))
        .bind("b".repeat(64))
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn seed_wave12_skills(&self, now: &str) -> Result<(), Error> {
        let builtin = crate::planning::skills::builtin_skill_pack("rust")
            .ok_or(Error::Invalid("builtin rust skill pack"))?;

        let mut due_rust = builtin.clone();
        due_rust.content_reviewed_at = "2026-01-01T00:00:00Z".into();
        due_rust.review_after = "2026-04-01T00:00:00Z".into();
        due_rust
            .best_practices
            .push("persisted due revision".into());
        self.save_skill_pack(&due_rust).await?;

        let mut legacy_due = builtin.clone();
        legacy_due.technology = "legacy-tool".into();
        legacy_due.content_reviewed_at = "2026-01-01T00:00:00Z".into();
        legacy_due.review_after = "2026-04-01T00:00:00Z".into();
        legacy_due
            .best_practices
            .push("persisted due revision without a builtin source".into());
        self.save_skill_pack(&legacy_due).await?;

        let typescript_builtin = crate::planning::skills::builtin_skill_pack("typescript")
            .ok_or(Error::Invalid("builtin typescript skill pack"))?;
        let mut source_only = typescript_builtin.clone();
        source_only.source_checked_at = "2026-01-01T00:00:00Z".into();
        source_only.content_reviewed_at = String::new();
        source_only.review_basis = crate::planning::SkillReviewBasis::SourceAvailabilityOnly;
        source_only.review_after = String::new();
        source_only.review_scope = String::new();
        source_only.reviewed_sections = Vec::new();
        source_only
            .best_practices
            .push("source availability only fixture".into());
        let content_json = serde_json::to_string(&source_only)
            .map_err(|_| Error::Validation("wave12 source-only skill encoding".into()))?;
        let content_sha256 = sha256_hex(&content_json);
        sqlx::query::query(
            "INSERT OR IGNORE INTO skill_packs
             (id,technology,version,content_json,content_sha256,source_urls_json,
              source_checked_at,content_reviewed_at,review_after,license,provenance,created_at)
             VALUES (?1,'typescript',?2,?3,?4,?5,?6,'','','MIT','wp10 source-only fixture',?7)",
        )
        .bind(format!(
            "skill-pack-typescript-source-only-{content_sha256}"
        ))
        .bind(&source_only.version)
        .bind(&content_json)
        .bind(&content_sha256)
        .bind(serde_json::to_string(&source_only.source_urls).unwrap_or_else(|_| "[]".into()))
        .bind(&source_only.source_checked_at)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn seed_creates_exact_plan_trace_review_and_skill_state() {
        let path = std::env::temp_dir().join(format!(
            "factory-wave12-fixture-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let db = Database::open(&path).await.unwrap();
        let manifest = db.seed_wave12_fixtures("device:owner").await.unwrap();
        assert_eq!(manifest["seeded"], true);
        assert_eq!(manifest["ownerPrincipal"], "device:owner");

        let artifacts = db.planning_artifacts(READY_REQUEST).await.unwrap();
        let kinds = artifacts
            .iter()
            .map(|artifact| artifact.kind.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            kinds,
            [
                "ARCHITECTURE",
                "DOCUMENTATION",
                "DOD",
                "OPENSPEC",
                "PLAN",
                "SKILL",
                "STACK"
            ]
            .into_iter()
            .collect()
        );
        let review = db.latest_plan_review(READY_REQUEST).await.unwrap();
        assert_eq!(review.status, "PENDING");
        assert_eq!(review.plan_revision, 1);
        let spec = db.agentic_spec(READY_REQUEST).await.unwrap();
        assert_eq!(spec.state, "FROZEN");
        assert!(db.latest_openspec_projection(READY_REQUEST).await.is_ok());
        assert!(db.agentic_work_graph(READY_REQUEST).await.is_ok());

        let missing = db.planning_artifacts(MISSING_REQUEST).await.unwrap();
        assert!(
            missing.iter().all(|artifact| artifact.kind != "DOD"),
            "the missing-artifact scenario must omit DOD"
        );
        assert!(
            db.agentic_work_graph(MISMATCH_REQUEST).await.is_ok(),
            "the mismatch scenario must still render the WorkGraph"
        );
        assert!(
            db.execution_profile_revision(
                "profile-wp10-browser-invalid",
                "profile-wp10-browser-invalid-r1"
            )
            .await
            .is_err(),
            "the invalid profile must fail closed so the assignment fallback is exercised"
        );

        let candidate = format!("candidate-{READY_REQUEST}");
        let receipts = db.quality_receipts(&candidate).await.unwrap();
        assert_eq!(receipts.len(), 4);
        assert!(
            receipts
                .iter()
                .any(|receipt| receipt.result == crate::quality::ReceiptResult::ChangesRequested)
        );
        let findings = db.finding_dispositions(READY_REQUEST).await.unwrap();
        assert_eq!(findings.len(), 2);
        let decision = db
            .agentic_owner_decision("decision-refute-001")
            .await
            .unwrap();
        assert_eq!(decision.status, "APPROVED");
        assert_eq!(decision.resolved_by.as_deref(), Some("device:owner"));
        assert_eq!(decision.matter, "FINDING_REFUTATION");
        assert_eq!(
            decision.selected_option.as_deref(),
            Some("APPROVE_REFUTATION")
        );
        assert!(
            decision
                .options
                .iter()
                .any(|option| option == "APPROVE_REFUTATION")
        );

        let populated = db
            .workflow_trace(TRACE_POPULATED_REQUEST, 200)
            .await
            .unwrap();
        assert_eq!(populated.events.len(), 15);
        assert_eq!(populated.steps.len(), 2);
        assert!(
            populated
                .alerts
                .iter()
                .any(|alert| { alert.kind == crate::trace::TraceAlertKind::MalformedEvent })
        );
        assert!(
            populated
                .alerts
                .iter()
                .any(|alert| { alert.kind == crate::trace::TraceAlertKind::OpenStep })
        );
        assert!(
            populated
                .alerts
                .iter()
                .any(|alert| { alert.kind == crate::trace::TraceAlertKind::TerminalState })
        );

        let long = db.workflow_trace(TRACE_LONG_REQUEST, 200).await.unwrap();
        assert!(long.truncated);
        assert_eq!(long.events.len(), 200);
        assert!(long.steps_truncated);
        assert_eq!(long.steps.len(), 500);

        let status = crate::planning::skill_refresh::status(&db, "2026-11-09T00:00:00Z")
            .await
            .unwrap();
        assert_eq!(status.due, 3);
        assert!(status.blocked > 0);
        for technology in ["rust", "typescript"] {
            let pack = status
                .packs
                .iter()
                .find(|pack| pack.technology == technology)
                .unwrap();
            assert_eq!(
                pack.state,
                crate::planning::skills::SkillPackState::Refreshable
            );
        }
        let legacy = status
            .packs
            .iter()
            .find(|pack| pack.technology == "legacy-tool")
            .unwrap();
        assert_eq!(legacy.state, crate::planning::skills::SkillPackState::Due);

        db.close().await;
        let _ = std::fs::remove_file(path);
    }
}
