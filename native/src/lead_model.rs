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
    model_gateway::{ModelCallRequest, ModelGateway},
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
    let prompt = format!(
        "You are the bounded Intelligent Lead. Return ONLY one JSON object matching this schema. \
Do not add prose, markdown or unknown fields. Objective: {objective}. Project: {project_id}. \
Action: {action_id}. Request: {request_id}. Allowed evidence refs: [\"repository-files\"]. \
Allowed paths: {paths:?}. Capabilities: {capabilities:?}. Route: {route:?}. \
ActionSpec: {spec}. The proposal must reference only the frozen project/action/base revision/config hash, \
the delegation route and bounded work units. Model output is advisory; Core validates and freezes it.",
        objective = request.objective,
        project_id = request.project.project_id,
        action_id = request.action.id,
        request_id = request.request_id,
        paths = request.project.allowed_paths,
        capabilities = request.project.capabilities,
        route = request.grant.route,
        spec = serde_json::to_string(request.action_spec).unwrap_or_default(),
    );
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
                max_output_tokens: 4096,
            },
        )
        .await
        .map_err(|error| LeadError::Assessment(error.to_string()))?;
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
