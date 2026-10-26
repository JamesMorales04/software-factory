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
        StandingDelegation,
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
    pub grant: &'a StandingDelegation,
    pub now: chrono::DateTime<chrono::Utc>,
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
            "budget": {"maxAttempts": 1, "timeoutSecs": 300},
            "route": request.grant.route,
            "maxRepairCycles": 0,
            "maxWallClockSecs": 1800,
            "maxParallel": 1,
            "maxSessions": 2,
            "workUnits": [unit]
        }
    });
    Ok(format!(
        "You are the bounded Intelligent Lead. Return ONLY one JSON object that validates against the \
JSON Schema below. Do not add prose, markdown, comments or unknown fields. \
The example below is a valid shape for this request; copy its identities, route, qualityProfile, \
checks, paths and capabilities exactly, and use your own judgment for intent, complexity, risks, \
assumptions, uncertainty and the effort estimate. A change request must carry a real estimated \
or unknown effort estimate. Objective: {objective}. Project: {project_id}. Action: {action_id}. \
Request: {request_id}. Allowed evidence refs: [\"repository-files\"]. Allowed paths: {paths:?}. \
Capabilities: {capabilities:?}. Route: {route:?}. ActionSpec: {spec}. Model output is advisory; \
Core validates and freezes it. Hard constraints: schemaRevision=1; echo projectId, actionId and \
requestId exactly; evidenceRefs must be [\"repository-files\"]; proposal.projectId, actionId, \
baseRevision, projectConfigHash and objective must match the provided context exactly; proposal.class \
must be one of {classes:?}; proposal.qualityProfile must be {quality:?}; riskClass must be \"R0\"; \
budget.maxAttempts=1 and budget.timeoutSecs=300; route must be {route:?}; maxRepairCycles=0; \
maxWallClockSecs=1800; maxParallel=1; maxSessions=2; workUnits must contain exactly one implementation \
unit with id \"unit-1\" and the exact allowedPaths, capabilities, acceptanceCriteria and negativeCases \
shown.\n\nJSON Schema:\n{schema}\n\nValid example shape:\n{example}",
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
        schema = schema,
        example = example,
    ))
}

pub async fn assess(
    gateway: &ModelGateway,
    database: &Database,
    request: AssessmentRequest<'_>,
) -> Result<(LeadAssessment, LeadPlanProposal), LeadError> {
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
                max_output_tokens: 8_192,
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
    let proposal = assessment
        .proposal
        .clone()
        .ok_or_else(|| LeadError::Assessment("model assessment has no proposal".into()))?;
    proposal.validate(
        request.grant,
        &request.project.binding(),
        request.action,
        request.now,
    )?;
    Ok((assessment, proposal))
}
