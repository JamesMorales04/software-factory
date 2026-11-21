//! Bounded provider-backed Intelligent Lead assessment.
//!
//! The model receives a frozen objective/configuration context and must return
//! the strict LeadAssessment JSON object. Core validates the schema, identities,
//! evidence references, WorkGraph, scope, route and budget before persistence.
//! Model output never grants authority.

use crate::planning::license_decision::{
    InventoryEvidenceBundle, LicenseDecisionReview, VERSIONED_SOURCE_EVIDENCE_JSON,
    identity_facts_receipt_digest,
};
use crate::{
    db::{ActionSpec, Database, ModelCallRole},
    lead::{
        ActionBinding, AssessmentInput, LeadAssessment, LeadError, LeadPlanProposal, ProjectConfig,
        StandingDelegationV2,
    },
    model_gateway::{ModelCallRequest, ModelGateway, ModelGatewayError},
    model_output::decode_lead_assessment,
};

pub struct AssessmentRequest<'a> {
    pub request_id: &'a str,
    pub project: &'a ProjectConfig,
    pub action: &'a ActionBinding,
    pub action_spec: &'a ActionSpec,
    pub objective: &'a str,
    pub grant: &'a StandingDelegationV2,
    pub now: chrono::DateTime<chrono::Utc>,
    /// True when Core detected no committed project stack. The prompt then
    /// requires a concrete environment proposal instead of a hidden
    /// language/toolchain default.
    pub greenfield: bool,
}

fn assessment_prompt(request: &AssessmentRequest<'_>) -> Result<String, LeadError> {
    let schema =
        serde_json::to_string(&crate::model_output::lead_assessment_schema()).map_err(|error| {
            LeadError::Assessment(format!("cannot encode assessment schema: {error}"))
        })?;
    let mut acceptance_criteria = Vec::new();
    let mut negative_cases = Vec::new();
    for check in &request.action_spec.checks {
        if check.kind == crate::lead::CheckKind::Negative {
            negative_cases.push(check.clone());
        } else {
            acceptance_criteria.push(check.clone());
        }
    }
    let checks_are_frozen = !request.action_spec.checks.is_empty();
    let timeout_secs = request.grant.max_timeout_secs;
    let example = serde_json::json!({
        "schemaRevision": 1,
        "projectId": request.project.project_id,
        "actionId": request.action.id,
        "requestId": request.request_id,
        "evidenceRefs": ["repository-files"],
        "intent": "change",
        "complexity": "small",
        "effortEstimate": {
            "status": "estimated",
            "unit": "person-hours",
            "minPersonHours": 1,
            "maxPersonHours": 3,
            "assumptions": ["Bounded single-module repair and test update."],
            "unknownReason": ""
        },
        "knowledgeGaps": [],
        "changeImpact": [],
        "validationNeeds": ["Run the frozen ActionSpec checks."],
        "assumptions": [],
        "uncertainty": [],
        "suggestedNextAction": "Freeze the plan and let the Planner decompose the objective.",
        "proposal": {
            "projectId": request.project.project_id,
            "actionId": request.action.id,
            "baseRevision": request.project.base_commit,
            "projectConfigHash": request.project.config_hash,
            "objective": request.objective,
            "class": "feature",
            "assumptions": [],
            "risks": [],
            "acceptanceCriteria": acceptance_criteria,
            "negativeCases": negative_cases,
            "qualityProfile": request.project.quality_profile,
            "riskClass": "R0",
            "budget": {"timeoutSecs": timeout_secs},
            "route": request.grant.route,
            "maxParallel": 1,
            "workUnits": []
        }
    });
    let non_change_example = serde_json::json!({
        "schemaRevision": 1,
        "projectId": request.project.project_id,
        "actionId": request.action.id,
        "requestId": request.request_id,
        "evidenceRefs": ["repository-files"],
        "intent": "informational",
        "complexity": "small",
        "effortEstimate": {
            "status": "not_requested",
            "unit": "",
            "minPersonHours": 0,
            "maxPersonHours": 0,
            "assumptions": [],
            "unknownReason": ""
        },
        "knowledgeGaps": [],
        "changeImpact": [],
        "validationNeeds": [],
        "assumptions": [],
        "uncertainty": [],
        "suggestedNextAction": crate::lead::SUGGESTED_NEXT_ACTION_NO_ACTION
    });
    let required_kinds = request
        .project
        .quality_profile
        .required_check_kinds()
        .iter()
        .map(|kind| format!("{kind:?}").to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(", ");
    let checks_instruction = if checks_are_frozen {
        "copy the acceptanceCriteria and negativeCases from the ActionSpec exactly;".to_string()
    } else {
        format!(
            "the ActionSpec has no pre-frozen checks. The active quality profile requires at least one check of every kind: {required_kinds}. Propose those checks with real, non-shell argv commands. Put non-negative checks in acceptanceCriteria and negative checks in negativeCases; negative checks must expect a nonzero exit;"
        )
    };
    let environment_instruction = if request.greenfield {
        "This is a greenfield request, so proposal.environmentProposal is REQUIRED and must not be null or empty. Derive a concrete stack and environment from the objective: an exact pinned nixpkgs revision (or omit nixpkgs to use the builder's documented default), toolchains with exact versions, nixpkgs packages with exact versions and SPDX license identifiers, and build/test/lint/format/typecheck/run commands as argv vectors. There is no language default in this prompt or in Core, so do not copy a language from an example. Every check executable must be provided by the proposed packages; if you cannot name one, record the missing package as an explicit blocker instead of assuming cargo, rustc, python3, node or npm."
    } else {
        "For an existing project, prefer the committed toolchain and commands. proposal.environmentProposal may be null; if you supply one it is untrusted, must be exact and may not assume an executable the project does not provide."
    };
    Ok(format!(
        "You are the bounded Intelligent Lead. Return ONLY one JSON object that validates against the \
JSON Schema below. Do not add prose, markdown, comments or unknown fields. \
The example below is a valid CHANGE-intent shape for this request; copy the top-level identities, \
route, qualityProfile, paths and capabilities exactly, and use your own judgment for intent, \
complexity, risks, assumptions, uncertainty and the effort estimate. The example's `proposal` and \
`workUnits` apply ONLY to a change intent; if you classify the intent as informational or audit, \
use the second example shape instead: omit the `proposal` field entirely and never copy the \
change example's proposal or WorkUnits. Classify a \
question or a read-only assessment as informational or audit; never propose executable WorkUnits for \
those intents. For informational or \
audit without an explicit estimate request, effortEstimate.status must be not_requested with empty \
unit, bounds, assumptions and unknownReason. Every intent must include a non-empty \
suggestedNextAction of at most {note_chars} characters: for a change intent, state a concrete bounded \
next step; for informational or audit without change, use exactly {no_action:?}. \
{checks_instruction} A change request must carry \
a real estimated or unknown effort estimate. For status=estimated, unit must be \"person-hours\", \
0 <= minPersonHours <= maxPersonHours, maxPersonHours >= 1, assumptions must be non-empty, and \
unknownReason must be empty. Objective: {objective}\nProject: {project_id}. Action: \
{action_id}. Request: {request_id}. Allowed evidence refs: [\"repository-files\"]. Outer allowed paths (choose the smallest needed subset): \
{paths:?}. Outer capabilities (choose the smallest needed subset): {capabilities:?}. Route: {route:?}. ActionSpec: {spec}. Model output is \
advisory; Core validates and freezes it. Hard constraints: schemaRevision=1; echo projectId, actionId \
and requestId exactly; evidenceRefs must be [\"repository-files\"]; proposal.projectId, actionId, \
baseRevision, projectConfigHash and objective must match the provided context exactly. Copy \
proposal.objective byte-for-byte from the frozen Objective, including its final punctuation and any \
trailing period: do not append, trim, normalize, paraphrase or reformat it. proposal.class \
must be one of {classes:?}; proposal.qualityProfile must be {quality:?}; riskClass must be \"R0\"; \
{environment_instruction} The verifier executes checks in the frozen project environment, not a \
global language image. Do not assume cargo, rustc, python3, node, npm or any other executable exists \
unless the environment proposal or committed conventions provide it. \
budget.timeoutSecs={timeout_secs}; route must be {route:?}; maxParallel=1; \
for a change intent, `proposal` is required and workUnits must be an EMPTY array: the bounded \
Core Planner owns decomposition, so do not invent implementation units, ids or per-unit paths. \
Check ids must be unique. For informational or audit, omit \
`proposal` entirely.\n\nJSON Schema:\n{schema}\n\nValid CHANGE-intent example shape (with proposal):\n{example}\n\nValid non-change (informational or audit) example shape WITHOUT proposal:\n{non_change_example}",
        checks_instruction = checks_instruction,
        environment_instruction = environment_instruction,
        objective = request.objective,
        classes = request.grant.objective_classes,
        quality = request.project.quality_profile,
        project_id = request.project.project_id,
        action_id = request.action.id,
        request_id = request.request_id,
        paths = request.project.allowed_paths,
        capabilities = request.project.capabilities,
        route = request.grant.route,
        spec = serde_json::to_string(request.action_spec).unwrap_or_default(),
        timeout_secs = timeout_secs,
        schema = schema,
        example = example,
        non_change_example = non_change_example,
        note_chars = crate::lead::MAX_NOTE_CHARS,
        no_action = crate::lead::SUGGESTED_NEXT_ACTION_NO_ACTION,
    ))
}

pub async fn assess(
    gateway: &ModelGateway,
    database: &Database,
    request: AssessmentRequest<'_>,
) -> Result<(LeadAssessment, Option<LeadPlanProposal>), LeadError> {
    let input = AssessmentInput {
        project_id: request.project.project_id.clone(),
        action_id: request.action.id.clone(),
        request_id: request.request_id.into(),
        objective: request.objective.into(),
        allowed_evidence_refs: std::collections::BTreeSet::from(["repository-files".into()]),
    };
    let prompt = assessment_prompt(&request)?;
    let result = gateway
        .call(
            database,
            ModelCallRequest {
                role: ModelCallRole::Lead,
                principal_id: &request.grant.principal_id,
                project_id: &request.project.project_id,
                objective_id: request.request_id,
                scope_id: &request.grant.id,
                purpose: "lead_assessment",
                system_prompt: "Return strict JSON only. Model output is advisory and grants no authority.",
                user_prompt: &prompt,
                max_output_tokens: 0,
            },
        )
        .await
        .map_err(|error| match error {
            ModelGatewayError::Ambiguous(_)
            | ModelGatewayError::Upstream(_)
            | ModelGatewayError::Usage(_) => {
                LeadError::TransientAssessment(error.to_string())
            }
            ModelGatewayError::Database(
                crate::db::Error::Forbidden(_) | crate::db::Error::Conflict(_),
            ) => LeadError::TransientAssessment(error.to_string()),
            other => LeadError::Assessment(other.to_string()),
        })?;
    let assessment = decode_lead_assessment(result.content.as_bytes(), &input)
        .map_err(|error| LeadError::Assessment(error.to_string()))?;
    let proposal = assessment.proposal.clone();
    if let Some(proposal) = &proposal {
        proposal.validate_v2(
            request.grant,
            &request.project.binding(),
            request.action,
            request.now,
        )?;
    }
    Ok((assessment, proposal))
}

pub struct EnvironmentReviewRequest<'a> {
    pub request_id: &'a str,
    pub objective_id: &'a str,
    pub project: &'a ProjectConfig,
    pub plan: &'a crate::planning::environment::EnvironmentPlan,
    pub grant: &'a StandingDelegationV2,
    pub now: chrono::DateTime<chrono::Utc>,
}

fn environment_review_prompt(plan_json: &str, schema: &str) -> String {
    format!(
        "You are the bounded Intelligent Lead. Review exactly one frozen Candidate EnvironmentPlan \
and return ONLY one JSON object that validates against the JSON Schema below. Do not add prose, \
markdown, comments or unknown fields. Approve only when the conventions evidence, pinned nixpkgs, \
licences (allowUnfree must remain false), resources, network policy and builder identity are \
sufficient for execution. Any unknown or denied licence, missing pin, conflicting convention, \
unsafe sandbox/network policy or unresolved blocker requires CHANGES_REQUESTED or REJECT. You may \
not approve an unfree package without an exact owner override and you may not waive a Core \
validation. Echo requestId, actionId, planSha256, planRevision and riskClass exactly. Rationale \
must be 8..4000 characters. Constraints are bounded follow-up conditions for the Planner; they do \
not grant authority. The EnvironmentPlan JSON and JSON Schema follow.\n\nEnvironmentPlan:\n{}\
\n\nJSON Schema:\n{}",
        plan_json.chars().take(300_000).collect::<String>(),
        schema
    )
}

/// Ask the frozen Lead route to review one exact EnvironmentPlan. The model
/// output is schema-validated and must echo the plan identity and risk; Core
/// still validates the plan, licenses and freeze. The returned model-call
/// result is the provenance recorded with the Lead decision.
pub async fn review_environment(
    gateway: &ModelGateway,
    database: &Database,
    request: EnvironmentReviewRequest<'_>,
) -> Result<
    (
        crate::planning::environment::EnvironmentLeadReviewModel,
        crate::db::ModelCallResult,
    ),
    LeadError,
> {
    let schema = serde_json::to_string(&crate::model_output::environment_lead_review_schema())
        .map_err(|error| {
            LeadError::Assessment(format!("cannot encode environment review schema: {error}"))
        })?;
    let plan_json = serde_json::to_string(request.plan).map_err(|error| {
        LeadError::Assessment(format!("cannot encode environment plan: {error}"))
    })?;
    let prompt = environment_review_prompt(&plan_json, &schema);
    let result = gateway
        .call(
            database,
            ModelCallRequest {
                role: ModelCallRole::Lead,
                principal_id: &request.grant.principal_id,
                project_id: &request.project.project_id,
                objective_id: request.objective_id,
                scope_id: &request.grant.id,
                purpose: "lead_environment_review",
                system_prompt:
                    "Return strict JSON only. A Lead review is a recorded planning decision, not execution authority.",
                user_prompt: &prompt,
                max_output_tokens: 0,
            },
        )
        .await
        .map_err(|error| match error {
            ModelGatewayError::Ambiguous(_)
            | ModelGatewayError::Upstream(_)
            | ModelGatewayError::Usage(_) => {
                LeadError::TransientAssessment(error.to_string())
            }
            ModelGatewayError::Database(
                crate::db::Error::Forbidden(_) | crate::db::Error::Conflict(_),
            ) => LeadError::TransientAssessment(error.to_string()),
            other => LeadError::Assessment(other.to_string()),
        })?;
    let review = crate::model_output::decode_environment_lead_review(result.content.as_bytes())
        .map_err(|error| LeadError::Assessment(error.to_string()))?;
    review
        .validate_for(request.plan, request.now)
        .map_err(|error| LeadError::Assessment(error.message))?;
    Ok((review, result))
}

pub struct LicenseEvidenceReviewRequest<'a> {
    pub request_id: &'a str,
    pub objective_id: &'a str,
    pub project: &'a crate::lead::ProjectConfig,
    pub plan: &'a crate::db::StoredEnvironmentPlan,
    pub spec: Option<&'a crate::db::StoredEnvironmentSpec>,
    pub inventory: &'a InventoryEvidenceBundle,
    pub source: &'a crate::planning::license_decision::VerifiedSourceEvidence,
    pub identity_facts: &'a crate::planning::identity_facts::BoundIdentityFacts,
    pub delegation: &'a crate::db::AgenticOwnerDecisionRecord,
    pub grant: &'a crate::lead::StandingDelegationV2,
    pub now: chrono::DateTime<chrono::Utc>,
}

fn license_evidence_review_prompt(
    request: &LicenseEvidenceReviewRequest<'_>,
    schema: &str,
) -> Result<String, LeadError> {
    let plan_json = serde_json::to_string(request.plan).map_err(|error| {
        LeadError::Assessment(format!("cannot encode environment plan: {error}"))
    })?;
    let inventory = serde_json::to_value(request.inventory).map_err(|error| {
        LeadError::Assessment(format!("cannot encode inventory evidence: {error}"))
    })?;
    let evidence: serde_json::Value = serde_json::from_str(VERSIONED_SOURCE_EVIDENCE_JSON)
        .map_err(|error| {
            LeadError::Assessment(format!("cannot decode source evidence: {error}"))
        })?;
    let context = serde_json::json!({
        "requestId": request.request_id,
        "objectiveId": request.objective_id,
        "projectId": request.project.project_id,
        "principalId": request.grant.principal_id,
        "planSha256": request.plan.plan_sha256,
        "planRevision": request.plan.revision,
        "planStatus": request.plan.status,
        "specSha256": request.spec.map(|spec| spec.spec_sha256.clone()),
        "inventoryEvidence": inventory,
        "evidenceBundleSha256": crate::planning::license_decision::canonical_json_sha256(
            &serde_json::json!({"entries": request.inventory.entries})
        ),
        "sourceEvidence": request.source.to_json(),
        "sourceEvidenceSha256": request.source.canonical_sha256(),
        "identityFactsReceiptDigest": identity_facts_receipt_digest(request.identity_facts.verified()),
        "identityFactsManifestObservedRoot": request.identity_facts.receipt().envelope.manifest.observed_root,
        "licenseEvidenceAsset": evidence,
        "delegation": {
            "id": request.delegation.id,
            "matter": request.delegation.matter,
            "status": request.delegation.status,
            "selectedOption": request.delegation.selected_option,
            "evidence": request.delegation.evidence,
            "deadline": request.delegation.deadline,
        },
        "policy": {
            "catalogSha256": crate::planning::license_policy::default_catalog_sha256(),
            "profileSha256": crate::planning::license_policy::default_profile_sha256(),
            "bindingSha256": crate::planning::license_policy::default_binding_sha256(),
        },
    });
    let context = serde_json::to_string_pretty(&context)
        .map_err(|error| LeadError::Assessment(format!("cannot encode review context: {error}")))?;
    Ok(format!(
        "You are the bounded Intelligent Lead reviewing the exact licence identity evidence for one environment stage. Return ONLY one JSON object that validates against the JSON Schema below; no prose, markdown, comments or unknown fields. You have no authority: Core verifies every field, the exact output/source facts, the signed source proof, the plan/inventory stage, the Owner delegation, the Lead actor and the current catalogue/profile/policy. Do not invent an identity, licence, text, digest, source path, obligation, delegation field or plan/spec hash. ACCEPT only when the reviewed expression is the exact identity licensing fact, the selected branch is a complete AST selection (AND keeps all conjuncts, OR chooses one alternative, WITH keeps base plus exception), every selected atom has an exact licence text and licensing fact, documentation has exact outputSha256 and licenseTextPath/licence/text digest, obligations are exact for software and documentation, and the commercial/no-mandatory-fee reading is supported by the authenticated text. GFDL/classified-UNKNOWN documentation needs the active delegation and must be recorded, never ignored. A prior copyleft DENY may be reconciled only by exact trusted text, exact obligations and the commercial/no-fee predicate; source-available/BUSL, NC and paid licences stay denied. Do not emit requestSha256 or responseSha256: Core fills them. Echo the exact pin/name/version/output/source fields from the context. Use softwareTexts with one exact {{path,license,sha256}} per selected atom; for a single atom softwareTextSha256 may carry the legacy digest. Documentation entries require outputSha256, licenseTextPath, licenseTextSha256 and exact obligations. The Lead is the semantic reader of the authenticated text; Core applies the deterministic limits. If evidence is incomplete, return NEEDS_EVIDENCE; if policy requires denial, return DENY.

Review context:
{}

EnvironmentPlan (exact):
{}

JSON Schema:
{}",
        context,
        plan_json.chars().take(300_000).collect::<String>(),
        schema
    ))
}

/// Narrow dispatch seam for the licence review producer. Production always
/// dispatches through the role-scoped `ModelGateway`; tests use it to prove
/// that every refusal happens before any dispatch and without a reservation.
trait LicenseReviewDispatch {
    async fn dispatch(
        &self,
        database: &Database,
        request: ModelCallRequest<'_>,
    ) -> Result<crate::db::ModelCallResult, ModelGatewayError>;
}

impl LicenseReviewDispatch for ModelGateway {
    async fn dispatch(
        &self,
        database: &Database,
        request: ModelCallRequest<'_>,
    ) -> Result<crate::db::ModelCallResult, ModelGatewayError> {
        self.call(database, request).await
    }
}

/// Execute the authorized bounded Lead `license_evidence_review` call. The
/// returned review is schema/domain checked but remains advisory until Core
/// verifies it and persists a decision. A failed/unknown model call returns no
/// review and therefore grants nothing.
pub async fn review_license_evidence(
    gateway: &ModelGateway,
    database: &Database,
    request: LicenseEvidenceReviewRequest<'_>,
) -> Result<(LicenseDecisionReview, crate::db::ModelCallResult), LeadError> {
    review_license_evidence_with(gateway, database, request).await
}

async fn review_license_evidence_with<G: LicenseReviewDispatch>(
    gateway: &G,
    database: &Database,
    request: LicenseEvidenceReviewRequest<'_>,
) -> Result<(LicenseDecisionReview, crate::db::ModelCallResult), LeadError> {
    if request.grant.revoked_at.is_some() || request.grant.expires_at <= request.now {
        return Err(LeadError::Assessment(
            "the Lead grant is expired or revoked".into(),
        ));
    }
    if request.delegation.project_id != request.project.project_id
        || request.delegation.matter != crate::db::LICENSE_DELEGATION_MATTER
        || request.delegation.status != "APPROVED"
        || request.delegation.selected_option.as_deref()
            != Some(crate::db::LICENSE_DELEGATION_OPTION)
        || chrono::DateTime::parse_from_rfc3339(&request.delegation.deadline)
            .map(|deadline| deadline <= request.now)
            .unwrap_or(true)
    {
        return Err(LeadError::Assessment(
            "the Owner licence delegation is not active for this call".into(),
        ));
    }
    if request.inventory.entries != request.plan.plan.license_inventory.entries {
        return Err(LeadError::Assessment(
            "the inventory evidence is not the exact plan inventory".into(),
        ));
    }
    if request.plan.plan_sha256 != request.plan.plan.plan_sha256
        || request.plan.revision != request.plan.plan.revision
    {
        return Err(LeadError::Assessment(
            "the stored plan row is not its exact plan content".into(),
        ));
    }
    if !matches!(
        request.plan.plan.status,
        crate::planning::environment::EnvironmentPlanStatus::CoreValidated
            | crate::planning::environment::EnvironmentPlanStatus::LeadApproved
            | crate::planning::environment::EnvironmentPlanStatus::Frozen
    ) || request.plan.plan.license_inventory.catalog_sha256
        != crate::planning::license_policy::default_catalog_sha256()
        || request.plan.plan.license_inventory.profile_sha256
            != crate::planning::license_policy::default_profile_sha256()
    {
        return Err(LeadError::Assessment(
            "the plan is stale or outside the current licence policy".into(),
        ));
    }
    if let Some(spec) = request.spec
        && (spec.spec.status != crate::planning::environment::EnvironmentSpecStatus::Frozen
            || spec.spec_sha256 != spec.spec.spec_sha256
            || spec.spec.plan_sha256 != request.plan.plan_sha256
            || spec.spec.license_inventory_sha256 != request.plan.plan.license_inventory_sha256
            || spec.spec.base_commit != request.plan.plan.base_commit)
    {
        return Err(LeadError::Assessment(
            "the spec is not the exact frozen stage for this plan".into(),
        ));
    }
    let expected_binding = if request.spec.is_some() {
        crate::planning::identity_facts::IdentityFactsRootBinding::BuildReceipt
    } else {
        crate::planning::identity_facts::IdentityFactsRootBinding::SpecCacheObservation
    };
    if request.identity_facts.binding() != expected_binding {
        return Err(LeadError::Assessment(
            "the identity-facts root binding does not match the stage".into(),
        ));
    }
    let evidence = &request.delegation.evidence;
    if evidence.get("domain").and_then(serde_json::Value::as_str)
        != Some("licenses-identity-evidence")
        || evidence
            .get("pin")
            .and_then(serde_json::Value::as_str)
            .is_none()
        || evidence
            .get("useContext")
            .and_then(serde_json::Value::as_str)
            .is_none()
        || evidence
            .get("economicCriterion")
            .and_then(serde_json::Value::as_str)
            != Some("no-mandatory-fee-commercial-use")
        || evidence
            .get("revision")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            == 0
    {
        return Err(LeadError::Assessment(
            "the Owner delegation evidence is incomplete".into(),
        ));
    }
    let challenge = &request.identity_facts.receipt().envelope.challenge;
    if challenge.plan_sha256 != request.plan.plan_sha256 {
        return Err(LeadError::Assessment(
            "the identity-facts receipt does not bind the exact plan".into(),
        ));
    }
    if challenge.pin != request.plan.plan.nix.nixpkgs {
        return Err(LeadError::Assessment(
            "the identity-facts receipt does not bind the exact plan pin".into(),
        ));
    }
    if let Some(spec) = request.spec
        && challenge.spec_sha256 != spec.spec_sha256
    {
        return Err(LeadError::Assessment(
            "the identity-facts receipt does not bind the exact spec".into(),
        ));
    }
    if !crate::planning::license_decision::source_evidence_belongs_to_pin(
        request.source,
        &challenge.pin,
    ) {
        return Err(LeadError::Assessment(
            "the source proof is not a shared-asset identity under the plan pin".into(),
        ));
    }
    let schema = serde_json::to_string(&crate::model_output::license_evidence_review_schema())
        .map_err(|error| {
            LeadError::Assessment(format!("cannot encode licence review schema: {error}"))
        })?;
    let prompt = license_evidence_review_prompt(&request, &schema)?;
    let result = gateway
        .dispatch(
            database,
            ModelCallRequest {
                role: ModelCallRole::Lead,
                principal_id: &request.grant.principal_id,
                project_id: &request.project.project_id,
                objective_id: request.objective_id,
                scope_id: &request.grant.id,
                purpose: "license_evidence_review",
                system_prompt:
                    "Return strict JSON only. The licence review is evidence, never authority; Core verifies and persists.",
                user_prompt: &prompt,
                max_output_tokens: 0,
            },
        )
        .await
        .map_err(|error| match error {
            ModelGatewayError::Ambiguous(_)
            | ModelGatewayError::Upstream(_)
            | ModelGatewayError::Usage(_) => {
                LeadError::TransientAssessment(error.to_string())
            }
            ModelGatewayError::Database(
                crate::db::Error::Forbidden(_) | crate::db::Error::Conflict(_),
            ) => LeadError::TransientAssessment(error.to_string()),
            other => LeadError::Assessment(other.to_string()),
        })?;
    let review = crate::model_output::decode_license_evidence_review(result.content.as_bytes())
        .map_err(|error| LeadError::Assessment(error.to_string()))?;
    Ok((review, result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ActionSpec;
    use crate::execution_policy::{OperationLimits, ProgressPolicy};
    use crate::lead::{
        ActionBinding, ActionStatus, AssessmentInput, Capability, CheckKind, ObjectiveClass,
        ProjectConfig, ProviderRoute, QualityProfile, RiskClass, SpecCheck, StandingDelegationV2,
    };

    fn request_stub() -> (
        ProjectConfig,
        ActionBinding,
        ActionSpec,
        StandingDelegationV2,
    ) {
        let project = ProjectConfig {
            project_id: "project-1".into(),
            revision: 1,
            config_hash: "a".repeat(64),
            repository: "/repo".into(),
            base_commit: "b".repeat(40),
            allowed_paths: vec!["src".into()],
            capabilities: vec![Capability::RepositoryEdit, Capability::GenericToolchain],
            quality_profile: QualityProfile::Generic,
            active: true,
            updated_at: "2026-11-16T00:00:00Z".into(),
        };
        let action = ActionBinding {
            id: "action-1".into(),
            project_id: "project-1".into(),
            status: ActionStatus::Pending,
        };
        let spec = ActionSpec {
            schema_version: 1,
            project_id: "project-1".into(),
            repository: "/repo".into(),
            base_commit: project.base_commit.clone(),
            objective: "Review the repository without changing it.".into(),
            objective_sha256: "c".repeat(64),
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            allowed_paths: vec!["src".into()],
            checks: vec![SpecCheck {
                id: "format".into(),
                kind: CheckKind::Format,
                description: "format".into(),
                command: vec!["cargo".into(), "fmt".into(), "--check".into()],
                expect_exit: 0,
                timeout_secs: 60,
                environment_plan_id: None,
                environment_spec_sha256: None,
            }],
            dependency_plan: None,
            dependencies_required: false,
            environment_plan_id: None,
            environment_plan_sha256: None,
            environment_spec_id: None,
            environment_spec_sha256: None,
            environment_builder_image: None,
            environment_builder_image_digest: None,
            environment_required: false,
        };
        let grant = StandingDelegationV2 {
            id: "grant-1".into(),
            principal_id: "owner-1".into(),
            audit_id: "audit-1".into(),
            project_id: "project-1".into(),
            revision: 1,
            content_hash: "d".repeat(64),
            objective_classes: vec![ObjectiveClass::Feature, ObjectiveClass::Maintenance],
            allowed_paths: vec!["src".into()],
            capabilities: vec![Capability::RepositoryEdit, Capability::GenericToolchain],
            risk_classes: vec![RiskClass::R0],
            autonomous_start: false,
            autonomous_risk_classes: Vec::new(),
            route: ProviderRoute {
                provider: "opencode-go".into(),
                model: "deepseek-v4.1-flash".into(),
                mode: "build".into(),
            },
            max_timeout_secs: 300,
            max_parallel: 1,
            operations: OperationLimits {
                model_request_timeout_secs: std::num::NonZeroU32::new(300).unwrap(),
                tool_call_timeout_secs: std::num::NonZeroU32::new(600).unwrap(),
                max_request_bytes: std::num::NonZeroU32::new(2 * 1024 * 1024).unwrap(),
                max_response_bytes: std::num::NonZeroU32::new(2 * 1024 * 1024).unwrap(),
                max_generation_tokens: std::num::NonZeroU32::new(8_192).unwrap(),
                max_in_flight_operations: std::num::NonZeroU32::new(4).unwrap(),
            },
            progress: ProgressPolicy {
                diagnose_after_stagnant_checkpoints: std::num::NonZeroU32::new(2).unwrap(),
                escalate_after_unsuccessful_replans: std::num::NonZeroU32::new(1).unwrap(),
                observation_timeout_secs: std::num::NonZeroU32::new(300).unwrap(),
                autonomy: Default::default(),
            },
            expires_at: chrono::Utc::now() + chrono::Duration::days(1),
            revoked_at: None,
        };
        (project, action, spec, grant)
    }

    fn assessment_prompt_for_test() -> String {
        let (project, action, spec, grant) = request_stub();
        let request = AssessmentRequest {
            request_id: "request-1",
            project: &project,
            action: &action,
            action_spec: &spec,
            objective: "Review the repository without changing it.",
            grant: &grant,
            now: chrono::Utc::now(),
            greenfield: false,
        };
        assessment_prompt(&request).unwrap()
    }

    #[test]
    fn prompt_requires_byte_for_byte_objective_copying() {
        let prompt = assessment_prompt_for_test();
        assert!(
            prompt.contains("Copy proposal.objective byte-for-byte"),
            "{prompt}"
        );
        assert!(
            prompt.contains("including its final punctuation"),
            "{prompt}"
        );
        assert!(
            prompt.contains("do not append, trim, normalize, paraphrase or reformat"),
            "{prompt}"
        );
        assert!(
            prompt.contains("Objective: Review the repository without changing it.\nProject:"),
            "the prompt must not append a second period to the objective"
        );
        assert!(
            !prompt.contains("Review the repository without changing it.."),
            "{prompt}"
        );
    }

    #[test]
    fn prompt_does_not_require_a_lead_work_unit() {
        let prompt = assessment_prompt_for_test();
        assert!(
            !prompt.contains("workUnits must contain exactly one implementation unit"),
            "the Lead is still required to author a WorkUnit"
        );
        assert!(!prompt.contains("dispatch unit-1"), "{prompt}");
        assert!(
            prompt.contains("workUnits must be an EMPTY array"),
            "{prompt}"
        );
        assert!(
            prompt.contains("for a change intent, `proposal` is required"),
            "{prompt}"
        );
        assert!(prompt.contains("For informational or audit, omit `proposal` entirely."));
        assert!(
            prompt.contains(
                "Valid non-change (informational or audit) example shape WITHOUT proposal"
            )
        );
    }

    #[test]
    fn prompt_examples_parse_through_the_real_decoder() {
        let prompt = assessment_prompt_for_test();
        let change = prompt
            .split("Valid CHANGE-intent example shape (with proposal):\n")
            .nth(1)
            .and_then(|rest| rest.split("\n\nValid non-change").next())
            .expect("change example");
        let non_change = prompt
            .split("Valid non-change (informational or audit) example shape WITHOUT proposal:\n")
            .nth(1)
            .expect("non-change example");
        let change: serde_json::Value = serde_json::from_str(change).unwrap();
        let non_change: serde_json::Value = serde_json::from_str(non_change).unwrap();
        assert!(change.get("proposal").is_some());
        assert_eq!(
            change["proposal"]["workUnits"],
            serde_json::json!([]),
            "the change example must not author executable units"
        );
        assert!(non_change.get("proposal").is_none());
        let input = AssessmentInput {
            project_id: "project-1".into(),
            action_id: "action-1".into(),
            request_id: "request-1".into(),
            objective: "Review the repository without changing it.".into(),
            allowed_evidence_refs: std::collections::BTreeSet::from([
                "repository-files".to_string()
            ]),
        };
        crate::model_output::decode_lead_assessment(
            &serde_json::to_vec(&non_change).unwrap(),
            &input,
        )
        .expect("non-change example must pass schema and domain validation");
        crate::model_output::decode_lead_assessment(&serde_json::to_vec(&change).unwrap(), &input)
            .expect("change example must pass schema and domain validation");
    }

    #[test]
    fn environment_review_prompt_carries_plan_identities_in_the_user_prompt() {
        let plan = r#"{"requestId":"request-1","actionId":"action-1","planSha256":"abc","revision":2,"riskClass":"R0"}"#;
        let prompt = environment_review_prompt(plan, "{}");
        assert!(prompt.contains("Review exactly one frozen Candidate EnvironmentPlan"));
        for marker in [
            "\"requestId\":\"request-1\"",
            "\"actionId\":\"action-1\"",
            "\"planSha256\":\"abc\"",
            "\"revision\":2",
            "\"riskClass\":\"R0\"",
        ] {
            assert!(prompt.contains(marker), "missing {marker}");
        }
        assert!(prompt.contains("EnvironmentPlan:\n"));
        assert!(prompt.contains("\n\nJSON Schema:\n"));
    }

    mod license_review {
        use super::*;
        use crate::db::{AgenticOwnerDecisionRecord, StoredEnvironmentPlan, StoredEnvironmentSpec};
        use crate::lead::RiskClass;
        use crate::planning::environment::{
            EnvironmentBuildReceipt, EnvironmentBuilderStatus, EnvironmentPlan,
            EnvironmentPlanStatus, EnvironmentSpec, EnvironmentSpecStatus, SandboxAttestation,
        };
        use crate::planning::identity_facts::{
            BoundIdentityFacts, IdentityFact, IdentityFactsChallenge, IdentityFactsEnvelope,
            IdentityFactsManifest, IdentityFactsMode, IdentityFactsReceipt, OperationRoot,
            VerifiedIdentityFacts, closure_manifest_sha256, sign, verify,
        };
        use crate::planning::license_decision::{
            InventoryEvidenceBundle, SourceEvidenceProof, VerifiedSourceEvidence,
        };
        use std::sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        };

        const BEARER: &[u8] = b"scoped-environment-bearer-0123456789abcdef";
        const PIN: &str = "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d";
        const OBSERVED_SPEC: &str =
            "067f8c8948e570e3ac2c8b8c8a9c47a0120759916b19e3c420308ff509d1b901";
        const OUTPUT_PATH: &str = "/nix/store/36qzlljldgb41snr4l38pfpcy19w7s2j-ed-1.21.1";
        const OUTPUT_DRV: &str = "/nix/store/frkdwgpbgh17hz57giw5rm3bs8bsflfb-ed-1.21.1.drv";
        const OUTPUT_NAR: &str = "sha256-humgomxjPuMnaa5VWRl8ondk5lDth9N3px977TJOpG4=";
        const SOURCE_DRV: &str = "/nix/store/j151nb5vh176kdrrh1rkilxkdkmvm47w-ed-1.21.1.tar.lz.drv";
        const SOURCE_PATH: &str = "/nix/store/wa88qds9615j2r5imfryzbyg86c97iga-ed-1.21.1.tar.lz";
        const SOURCE_NAR: &str = "sha256-dEsPDHFVSahejFEnqz5gAMxx+ls+idLXQ6qOovYOecE=";
        const SOURCE_FLAT: &str =
            "d6d0c7192b02b0519c902a93719053e865ade5a784a3b327d93d888457b23c4b";
        const SOURCE_SIZE: u64 = 70136;
        const SOURCE_SIGNATURE: &str = "cache.nixos.org-1:xYwC8Vo2buDIPQNzCyHhC8Kz7Y6riU596Zp3tnRTIicdLtih5xQzAPlip25FtXIsShiTfkl4eABEg4TX0c+ZDA==";

        fn current_plan() -> EnvironmentPlan {
            let mut plan: EnvironmentPlan = serde_json::from_str(include_str!(
                "planning/license_decision/fixtures/retained-plan.json"
            ))
            .expect("the retained plan fixture is valid");
            plan.license_inventory.catalog_sha256 =
                crate::planning::license_policy::default_catalog_sha256().into();
            plan.license_inventory.profile_sha256 =
                crate::planning::license_policy::default_profile_sha256().into();
            plan.license_inventory.inventory_sha256 =
                plan.license_inventory.compute_sha256(&plan.plan_id);
            plan.license_inventory_sha256 = plan.license_inventory.inventory_sha256.clone();
            plan.plan_sha256 = plan.compute_sha256();
            plan.status = EnvironmentPlanStatus::CoreValidated;
            plan
        }

        fn stored_plan(plan: &EnvironmentPlan) -> StoredEnvironmentPlan {
            StoredEnvironmentPlan {
                row_id: "plan-row".into(),
                revision: plan.revision,
                status: "CORE_VALIDATED".into(),
                plan_sha256: plan.plan_sha256.clone(),
                content_json: serde_json::to_string(plan).expect("plan serializes"),
                created_at: "2026-11-20T19:24:23Z".into(),
                updated_at: "2026-11-20T19:24:23Z".into(),
                plan: plan.clone(),
            }
        }

        fn environment_spec(plan: &EnvironmentPlan, spec_sha256: &str) -> EnvironmentSpec {
            EnvironmentSpec {
                schema_version: 1,
                spec_id: "spec-1".into(),
                spec_sha256: spec_sha256.into(),
                project_id: plan.project_id.clone(),
                request_id: plan.request_id.clone(),
                action_id: plan.action_id.clone(),
                campaign_id: None,
                base_commit: plan.base_commit.clone(),
                plan_id: plan.plan_id.clone(),
                plan_sha256: plan.plan_sha256.clone(),
                conventions_sha256: plan.conventions_sha256.clone(),
                review_id: "review-1".into(),
                platform: "x86_64-linux".into(),
                nix_version: "2.24.0".into(),
                devenv_version: "1.0.0".into(),
                flake_lock_sha256: String::new(),
                devenv_lock_sha256: String::new(),
                license_inventory_sha256: plan.license_inventory_sha256.clone(),
                sbom_sha256: plan.license_inventory.sbom_sha256.clone(),
                builder_image: plan.builder.image.clone(),
                builder_image_digest: plan.builder.digest.clone(),
                resource_policy: plan.resources.clone(),
                network_policy: plan.network.clone(),
                status: EnvironmentSpecStatus::Frozen,
                frozen_at: "2026-11-20T20:00:00Z".into(),
            }
        }

        fn stored_spec(spec: EnvironmentSpec) -> StoredEnvironmentSpec {
            StoredEnvironmentSpec {
                row_id: "spec-row".into(),
                spec_sha256: spec.spec_sha256.clone(),
                content_json: serde_json::to_string(&spec).expect("spec serializes"),
                frozen_at: spec.frozen_at.clone(),
                created_at: "2026-11-20T20:00:00Z".into(),
                spec,
            }
        }

        fn receipt_for(plan_sha256: &str, spec_sha256: &str, pin: &str) -> IdentityFactsReceipt {
            let challenge = IdentityFactsChallenge {
                operation_id: "operation-1".into(),
                runtime_id: "runtime-1".into(),
                nonce: format!(
                    "identity-facts:{}",
                    plan_sha256.get(..8).unwrap_or_default()
                ),
                pin: pin.into(),
                plan_sha256: plan_sha256.into(),
                spec_sha256: spec_sha256.into(),
                mode: IdentityFactsMode::Questions,
                root: OperationRoot::Operation,
                questions: vec![
                    crate::planning::identity_facts::IdentityQuestion {
                        store_path: OUTPUT_PATH.into(),
                        deriver: Some(OUTPUT_DRV.into()),
                        nar_hash: OUTPUT_NAR.into(),
                        outputs: vec![OUTPUT_PATH.into()],
                    },
                    crate::planning::identity_facts::IdentityQuestion {
                        store_path: SOURCE_PATH.into(),
                        deriver: Some(SOURCE_DRV.into()),
                        nar_hash: SOURCE_NAR.into(),
                        outputs: vec![SOURCE_PATH.into()],
                    },
                ],
                stage: crate::planning::identity_facts::IdentityFactsStage::Spec,
                resolution_key: String::new(),
            };
            let facts = vec![
                IdentityFact {
                    store_path: OUTPUT_PATH.into(),
                    deriver: Some(OUTPUT_DRV.into()),
                    nar_hash: OUTPUT_NAR.into(),
                    outputs: vec![OUTPUT_PATH.into()],
                },
                IdentityFact {
                    store_path: SOURCE_PATH.into(),
                    deriver: Some(SOURCE_DRV.into()),
                    nar_hash: SOURCE_NAR.into(),
                    outputs: vec![SOURCE_PATH.into()],
                },
            ];
            let envelope = IdentityFactsEnvelope {
                schema_version: 1,
                context: crate::planning::identity_facts::IDENTITY_FACTS_CONTEXT.into(),
                challenge,
                manifest: IdentityFactsManifest {
                    observed_root: OUTPUT_PATH.into(),
                    fact_count: facts.len(),
                    closure_manifest_sha256: closure_manifest_sha256(&facts),
                    facts,
                    truncated: false,
                },
            };
            let signature = sign(&envelope, BEARER).expect("the fixture receipt signs");
            IdentityFactsReceipt {
                envelope,
                signature,
            }
        }

        fn verified_facts(
            plan_sha256: &str,
            spec_sha256: &str,
            pin: &str,
        ) -> VerifiedIdentityFacts {
            let receipt = receipt_for(plan_sha256, spec_sha256, pin);
            verify(&receipt, BEARER, &receipt.envelope.challenge)
                .expect("the fixture receipt verifies")
        }

        fn source_proof() -> SourceEvidenceProof {
            SourceEvidenceProof {
                source_store_path: SOURCE_PATH.into(),
                source_drv_path: SOURCE_DRV.into(),
                flat_sha256: SOURCE_FLAT.into(),
                nar_sri: SOURCE_NAR.into(),
                nar_size: SOURCE_SIZE,
                signature: SOURCE_SIGNATURE.into(),
                output_drv_path: OUTPUT_DRV.into(),
                output_store_path: OUTPUT_PATH.into(),
                output_drv_input_drvs: vec![SOURCE_DRV.into()],
                nar_nix32: None,
                fingerprint: None,
                ca: None,
            }
        }

        fn delegation(project_id: &str) -> AgenticOwnerDecisionRecord {
            let now = chrono::Utc::now();
            AgenticOwnerDecisionRecord {
                id: "delegation-1".into(),
                project_id: project_id.into(),
                request_id: None,
                campaign_id: None,
                matter: crate::db::LICENSE_DELEGATION_MATTER.into(),
                risk: RiskClass::R0,
                status: "APPROVED".into(),
                options: vec![
                    crate::db::LICENSE_DELEGATION_OPTION.into(),
                    "DENY_DELEGATION".into(),
                ],
                evidence: serde_json::json!({
                    "domain": "licenses-identity-evidence",
                    "pin": PIN,
                    "useContext": "commercial-development",
                    "economicCriterion": "no-mandatory-fee-commercial-use",
                    "revision": 7,
                }),
                recommendation: "Delegate the exact licence decision.".into(),
                deadline: (now + chrono::Duration::hours(1)).to_rfc3339(),
                selected_option: Some(crate::db::LICENSE_DELEGATION_OPTION.into()),
                rationale: Some("Owner delegated the exact decision.".into()),
                resolved_by: Some("operator".into()),
                created_at: now.to_rfc3339(),
                updated_at: now.to_rfc3339(),
                resolved_at: Some(now.to_rfc3339()),
            }
        }

        struct Fixture {
            plan: EnvironmentPlan,
            stored: StoredEnvironmentPlan,
            inventory: InventoryEvidenceBundle,
            facts: BoundIdentityFacts,
            source: VerifiedSourceEvidence,
            project: ProjectConfig,
            delegation: AgenticOwnerDecisionRecord,
            grant: StandingDelegationV2,
            now: chrono::DateTime<chrono::Utc>,
        }

        impl Fixture {
            fn new() -> Self {
                let plan = current_plan();
                let inventory = InventoryEvidenceBundle::from_plan(&plan);
                let facts = BoundIdentityFacts::spec_cache_observation(verified_facts(
                    &plan.plan_sha256,
                    OBSERVED_SPEC,
                    PIN,
                ));
                let (project, _, _, grant) = request_stub();
                Self {
                    stored: stored_plan(&plan),
                    plan,
                    inventory,
                    facts,
                    source: VerifiedSourceEvidence::verify(&source_proof())
                        .expect("the pinned cache signature verifies"),
                    project,
                    delegation: delegation("project-1"),
                    grant,
                    now: chrono::Utc::now(),
                }
            }

            fn request<'a>(
                &'a self,
                spec: Option<&'a StoredEnvironmentSpec>,
            ) -> LicenseEvidenceReviewRequest<'a> {
                LicenseEvidenceReviewRequest {
                    request_id: "lead-1",
                    objective_id: "objective-1",
                    project: &self.project,
                    plan: &self.stored,
                    spec,
                    inventory: &self.inventory,
                    source: &self.source,
                    identity_facts: &self.facts,
                    delegation: &self.delegation,
                    grant: &self.grant,
                    now: self.now,
                }
            }
        }

        #[derive(Default)]
        struct Captured {
            purpose: String,
            scope_id: String,
            prompt: String,
        }

        #[derive(Default)]
        struct FakeGateway {
            calls: AtomicUsize,
            captured: Mutex<Vec<Captured>>,
            content: Mutex<Option<String>>,
        }

        impl FakeGateway {
            fn with_review() -> Self {
                Self {
                    content: Mutex::new(Some(valid_review().to_string())),
                    ..Self::default()
                }
            }

            fn calls(&self) -> usize {
                self.calls.load(Ordering::SeqCst)
            }

            fn captured(&self) -> std::sync::MutexGuard<'_, Vec<Captured>> {
                self.captured.lock().expect("fake gateway capture lock")
            }
        }

        impl LicenseReviewDispatch for FakeGateway {
            async fn dispatch(
                &self,
                _database: &Database,
                request: ModelCallRequest<'_>,
            ) -> Result<crate::db::ModelCallResult, ModelGatewayError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                assert_eq!(request.role, ModelCallRole::Lead);
                assert_eq!(request.purpose, "license_evidence_review");
                self.captured
                    .lock()
                    .expect("fake gateway capture lock")
                    .push(Captured {
                        purpose: request.purpose.into(),
                        scope_id: request.scope_id.into(),
                        prompt: request.user_prompt.into(),
                    });
                let content = self
                    .content
                    .lock()
                    .expect("fake gateway content lock")
                    .clone()
                    .unwrap_or_else(|| "{}".into());
                Ok(crate::db::ModelCallResult {
                    reservation_id: "reservation-test".into(),
                    role: ModelCallRole::Lead,
                    provider: "opencode-go".into(),
                    model: "deepseek-v4.1-flash".into(),
                    request_sha256: "a".repeat(64),
                    content,
                    input_tokens: 1,
                    output_tokens: 1,
                    total_tokens: 2,
                    cost_micros: 0,
                    usage_known: true,
                })
            }
        }

        fn valid_review() -> serde_json::Value {
            serde_json::json!({
                "decision": "ACCEPT",
                "pin": PIN,
                "name": "ed",
                "version": "1.21.1",
                "outputDrvPath": OUTPUT_DRV,
                "outputStorePath": OUTPUT_PATH,
                "outputNarHash": OUTPUT_NAR,
                "sourceDrvPath": SOURCE_DRV,
                "sourceStorePath": SOURCE_PATH,
                "sourceOutputHash": "sha256-1tDHGSsCsFGckCqTcZBT6GWt5aeEo7Mn2T2IhFeyPEs=",
                "licenseExpression": "GPL-2.0-or-later",
                "softwareTextSha256": "022e9c2e9cc1aac3622313df6f971fbb39c1d1b0cb45324c16b8a1fe231c5ea6",
                "softwareTexts": [],
                "selectedBranch": "GPL-2.0-or-later",
                "documentation": [{
                    "path": "share/doc",
                    "license": "GFDL-1.3-or-later",
                    "outputSha256": "b".repeat(64),
                    "licenseTextPath": "doc/fdl.texi",
                    "licenseTextSha256": "c".repeat(64),
                    "obligations": ["notice", "source"]
                }],
                "useContext": "commercial-development",
                "commercialUse": true,
                "paymentRequired": false,
                "obligationsReviewed": true,
                "softwareObligations": ["notice", "source"],
                "documentationObligations": ["notice", "source"]
            })
        }

        async fn database() -> Database {
            let path = std::env::temp_dir().join(format!(
                "factory-lead-license-review-{}-{}.sqlite",
                std::process::id(),
                uuid::Uuid::new_v4()
            ));
            Database::open(&path).await.expect("fixture database")
        }

        async fn assert_refused(
            fixture: &Fixture,
            database: &Database,
            gateway: &FakeGateway,
            spec: Option<&StoredEnvironmentSpec>,
            needle: &str,
        ) {
            let before = gateway.calls();
            let error = review_license_evidence_with(gateway, database, fixture.request(spec))
                .await
                .err()
                .unwrap_or_else(|| panic!("the request must be refused: {needle}"));
            assert!(
                error.to_string().contains(needle),
                "unexpected refusal `{error}`; expected `{needle}`"
            );
            assert_eq!(
                gateway.calls(),
                before,
                "a refused review must not dispatch a model call"
            );
            assert!(
                database
                    .model_calls_for_scope(&fixture.grant.id)
                    .await
                    .expect("scope read")
                    .is_empty(),
                "a refused review must not create a reservation"
            );
        }

        #[tokio::test]
        async fn every_pre_dispatch_guard_refuses_without_a_call_or_reservation() {
            let database = database().await;
            let gateway = FakeGateway::with_review();

            let mut revoked = Fixture::new();
            revoked.grant.revoked_at = Some(chrono::Utc::now());
            assert_refused(&revoked, &database, &gateway, None, "expired or revoked").await;

            let mut expired = Fixture::new();
            expired.grant.expires_at = chrono::Utc::now() - chrono::Duration::seconds(1);
            assert_refused(&expired, &database, &gateway, None, "expired or revoked").await;

            let mut unapproved = Fixture::new();
            unapproved.delegation.status = "PENDING".into();
            unapproved.delegation.selected_option = None;
            assert_refused(&unapproved, &database, &gateway, None, "not active").await;

            let mut denied = Fixture::new();
            denied.delegation.selected_option = Some("DENY_DELEGATION".into());
            assert_refused(&denied, &database, &gateway, None, "not active").await;

            let mut foreign_project = Fixture::new();
            foreign_project.delegation.project_id = "project-2".into();
            assert_refused(&foreign_project, &database, &gateway, None, "not active").await;

            let mut lapsed = Fixture::new();
            lapsed.delegation.deadline =
                (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
            assert_refused(&lapsed, &database, &gateway, None, "not active").await;

            let mut widened_inventory = Fixture::new();
            widened_inventory.inventory = InventoryEvidenceBundle { entries: vec![] };
            assert_refused(
                &widened_inventory,
                &database,
                &gateway,
                None,
                "not the exact plan inventory",
            )
            .await;

            let mut stale_plan = Fixture::new();
            stale_plan.stored.plan.license_inventory.catalog_sha256 = "f".repeat(64);
            assert_refused(
                &stale_plan,
                &database,
                &gateway,
                None,
                "stale or outside the current licence policy",
            )
            .await;

            let mut unvalidated = Fixture::new();
            unvalidated.stored.plan.status = EnvironmentPlanStatus::Draft;
            assert_refused(
                &unvalidated,
                &database,
                &gateway,
                None,
                "stale or outside the current licence policy",
            )
            .await;

            let mut incomplete_evidence = Fixture::new();
            incomplete_evidence.delegation.evidence["economicCriterion"] =
                serde_json::json!("mandatory-fee");
            assert_refused(
                &incomplete_evidence,
                &database,
                &gateway,
                None,
                "Owner delegation evidence is incomplete",
            )
            .await;

            let mut foreign_receipt = Fixture::new();
            foreign_receipt.facts = BoundIdentityFacts::spec_cache_observation(verified_facts(
                &"e".repeat(64),
                OBSERVED_SPEC,
                PIN,
            ));
            assert_refused(
                &foreign_receipt,
                &database,
                &gateway,
                None,
                "does not bind the exact plan",
            )
            .await;

            let mut forged_row = Fixture::new();
            forged_row.stored.plan_sha256 = "f".repeat(64);
            assert_refused(
                &forged_row,
                &database,
                &gateway,
                None,
                "not its exact plan content",
            )
            .await;

            let mut foreign_pin = Fixture::new();
            foreign_pin.facts = BoundIdentityFacts::spec_cache_observation(verified_facts(
                &foreign_pin.plan.plan_sha256,
                OBSERVED_SPEC,
                &format!("github:NixOS/nixpkgs/{}", "1".repeat(40)),
            ));
            assert_refused(
                &foreign_pin,
                &database,
                &gateway,
                None,
                "does not bind the exact plan pin",
            )
            .await;

            let mut foreign_source = Fixture::new();
            let mut proof = source_proof();
            proof.output_store_path = format!("/nix/store/{}-other", "b".repeat(32));
            foreign_source.source =
                VerifiedSourceEvidence::verify(&proof).expect("the signature covers the source");
            assert_refused(
                &foreign_source,
                &database,
                &gateway,
                None,
                "not a shared-asset identity under the plan pin",
            )
            .await;
        }

        fn build_receipt(spec: &EnvironmentSpec) -> EnvironmentBuildReceipt {
            EnvironmentBuildReceipt {
                schema_version: 1,
                receipt_id: "receipt-1".into(),
                spec_sha256: spec.spec_sha256.clone(),
                platform: spec.platform.clone(),
                builder_image: spec.builder_image.clone(),
                builder_image_digest: spec.builder_image_digest.clone(),
                builder_policy_revision: "nix-devenv-environment-v1".into(),
                status: EnvironmentBuilderStatus::Succeeded,
                store_path: OUTPUT_PATH.into(),
                output_sha256: "e".repeat(64),
                sbom_sha256: spec.sbom_sha256.clone(),
                log_ref: "log".into(),
                sandbox_attestation: SandboxAttestation {
                    non_root: true,
                    no_secrets: true,
                    no_host_network: true,
                    no_host_socket: true,
                    no_host_mount: true,
                    sandbox: true,
                    impure: false,
                    bounded_resources: true,
                },
                blockers: vec![],
                duration_ms: 1,
                created_at: "2026-11-20T20:00:01Z".into(),
            }
        }

        fn with_build_receipt(fixture: &mut Fixture, spec: &StoredEnvironmentSpec) {
            fixture.facts = BoundIdentityFacts::bind_build_receipt(
                verified_facts(&fixture.plan.plan_sha256, &spec.spec.spec_sha256, PIN),
                &build_receipt(&spec.spec),
                &spec.spec,
            )
            .expect("the build receipt binds the exact frozen spec");
        }

        #[tokio::test]
        async fn a_spec_stage_review_requires_the_build_receipt_root_and_the_exact_spec() {
            let database = database().await;
            let gateway = FakeGateway::with_review();
            let fixture = Fixture::new();
            let spec = stored_spec(environment_spec(&fixture.plan, &"d".repeat(64)));

            // A spec stage with a spec-cache root is refused before dispatch.
            assert_refused(
                &fixture,
                &database,
                &gateway,
                Some(&spec),
                "root binding does not match the stage",
            )
            .await;

            // A build receipt bound to another spec cannot authorise this spec.
            let mut foreign = Fixture::new();
            let foreign_spec = stored_spec(environment_spec(&foreign.plan, &"e".repeat(64)));
            with_build_receipt(&mut foreign, &foreign_spec);
            assert_refused(
                &foreign,
                &database,
                &gateway,
                Some(&spec),
                "does not bind the exact spec",
            )
            .await;

            // A stored row that is not its content cannot stand in for it.
            let mut forged_row = Fixture::new();
            let mut forged_spec = stored_spec(environment_spec(&forged_row.plan, &"d".repeat(64)));
            forged_spec.spec_sha256 = "f".repeat(64);
            with_build_receipt(&mut forged_row, &forged_spec);
            assert_refused(
                &forged_row,
                &database,
                &gateway,
                Some(&forged_spec),
                "not the exact frozen stage for this plan",
            )
            .await;

            let mut unfrozen = Fixture::new();
            let mut unfrozen_spec = stored_spec(environment_spec(&unfrozen.plan, &"d".repeat(64)));
            unfrozen_spec.spec.status = EnvironmentSpecStatus::Superseded;
            with_build_receipt(&mut unfrozen, &unfrozen_spec);
            assert_refused(
                &unfrozen,
                &database,
                &gateway,
                Some(&unfrozen_spec),
                "not the exact frozen stage for this plan",
            )
            .await;
        }

        #[tokio::test]
        async fn a_valid_plan_review_dispatches_exactly_once_with_a_stable_exact_request() {
            let database = database().await;
            let gateway = FakeGateway::with_review();
            let fixture = Fixture::new();

            let (review, result) =
                review_license_evidence_with(&gateway, &database, fixture.request(None))
                    .await
                    .expect("the valid plan review dispatches");
            assert_eq!(review.decision, "ACCEPT");
            assert_eq!(result.role, ModelCallRole::Lead);
            assert_eq!(gateway.calls(), 1);

            // The same evidence produces the same exact request, which is what
            // the gateway's request digest dedupes on replay.
            let (_, _) = review_license_evidence_with(&gateway, &database, fixture.request(None))
                .await
                .expect("the replay dispatches the identical request");
            assert_eq!(gateway.calls(), 2);
            let captured = gateway.captured();
            assert_eq!(captured[0].purpose, "license_evidence_review");
            assert_eq!(captured[0].scope_id, "grant-1");
            assert_eq!(
                captured[0].prompt, captured[1].prompt,
                "the producer must build one deterministic request from one evidence set"
            );
            assert!(captured[0].prompt.contains(&fixture.plan.plan_sha256));
            assert!(captured[0].prompt.contains(&fixture.stored.plan_sha256));
            assert!(
                captured[0]
                    .prompt
                    .contains(&fixture.source.canonical_sha256())
            );
        }

        #[tokio::test]
        async fn a_valid_frozen_spec_review_dispatches_exactly_once() {
            let database = database().await;
            let gateway = FakeGateway::with_review();
            let mut fixture = Fixture::new();
            let spec = stored_spec(environment_spec(&fixture.plan, &"d".repeat(64)));
            with_build_receipt(&mut fixture, &spec);

            review_license_evidence_with(&gateway, &database, fixture.request(Some(&spec)))
                .await
                .expect("the valid spec review dispatches");
            assert_eq!(gateway.calls(), 1);
            assert!(
                gateway.captured()[0]
                    .prompt
                    .contains(&spec.spec.spec_sha256)
            );
        }

        #[tokio::test]
        async fn an_invalid_model_payload_is_refused_after_exactly_one_dispatch() {
            let database = database().await;
            let gateway = FakeGateway::default();
            *gateway.content.lock().expect("content lock") = Some("{}".into());
            let fixture = Fixture::new();
            let error = review_license_evidence_with(&gateway, &database, fixture.request(None))
                .await
                .err()
                .expect("an untyped payload is not a review");
            assert!(error.to_string().contains("invalid assessment"));
            assert_eq!(gateway.calls(), 1);
        }
    }
}
