//! Bounded provider-backed Intelligent Lead assessment.
//!
//! The model receives a frozen objective/configuration context and must return
//! the strict LeadAssessment JSON object. Core validates the schema, identities,
//! evidence references, WorkGraph, scope, route and budget before persistence.
//! Model output never grants authority.

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
    let description: String = request.objective.chars().take(500).collect();
    let unit = serde_json::json!({
        "id": "unit-1",
        "kind": "implementation",
        "description": description,
        "dependsOn": [],
        "allowedPaths": request.project.allowed_paths,
        "capabilities": request.project.capabilities,
        "acceptanceCriteria": acceptance_criteria,
        "negativeCases": negative_cases,
    });
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
        "suggestedNextAction": "Freeze the plan and dispatch unit-1.",
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
            "workUnits": [unit]
        }
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
The example below is a valid shape for this request; copy its identities, route, qualityProfile, \
paths and capabilities exactly, and use your own judgment for intent, complexity, risks, \
assumptions, uncertainty and the effort estimate. Classify a question or a read-only assessment as \
informational or audit; never propose executable WorkUnits for those intents. For informational or \
audit without an explicit estimate request, effortEstimate.status must be not_requested with empty \
unit, bounds, assumptions and unknownReason. Every intent must include a non-empty \
suggestedNextAction of at most {note_chars} characters: for a change intent, state a concrete bounded \
next step; for informational or audit without change, use exactly {no_action:?}. \
{checks_instruction} A change request must carry \
a real estimated or unknown effort estimate. For status=estimated, unit must be \"person-hours\", \
0 <= minPersonHours <= maxPersonHours, maxPersonHours >= 1, assumptions must be non-empty, and \
unknownReason must be empty. Objective: {objective}. Project: {project_id}. Action: \
{action_id}. Request: {request_id}. Allowed evidence refs: [\"repository-files\"]. Outer allowed paths (choose the smallest needed subset): \
{paths:?}. Outer capabilities (choose the smallest needed subset): {capabilities:?}. Route: {route:?}. ActionSpec: {spec}. Model output is \
advisory; Core validates and freezes it. Hard constraints: schemaRevision=1; echo projectId, actionId \
and requestId exactly; evidenceRefs must be [\"repository-files\"]; proposal.projectId, actionId, \
baseRevision, projectConfigHash and objective must match the provided context exactly; proposal.class \
must be one of {classes:?}; proposal.qualityProfile must be {quality:?}; riskClass must be \"R0\"; \
{environment_instruction} The verifier executes checks in the frozen project environment, not a \
global language image. Do not assume cargo, rustc, python3, node, npm or any other executable exists \
unless the environment proposal or committed conventions provide it. \
budget.timeoutSecs={timeout_secs}; route must be {route:?}; maxParallel=1; \
workUnits must contain exactly one implementation unit with id \"unit-1\"; its allowedPaths and \
capabilities must be subsets of the outer lists shown and should reflect the plan reasoning; check ids \
must be unique.\n\nJSON Schema:\n{schema}\n\nValid example shape:\n{example}",
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
    let prompt = format!(
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
    );
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
