//! Bounded provider-backed Intelligent Lead assessment.
//!
//! The model receives a frozen objective/configuration context and must return
//! the strict LeadAssessment JSON object. Core validates the schema, identities,
//! evidence references, WorkGraph, scope, route and budget before persistence.
//! Model output never grants authority.

use std::time::Duration;

use serde_json::json;

use crate::{
    db::ActionSpec,
    lead::{
        ActionBinding, AssessmentInput, LeadAssessment, LeadError, LeadPlanProposal, ProjectConfig,
        StandingDelegation,
    },
    model_output::decode_lead_assessment,
};

#[derive(Clone)]
pub struct ModelLeadConfig {
    pub url: String,
    pub token: String,
    pub model: String,
    pub timeout: Duration,
}

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
    config: &ModelLeadConfig,
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
    let client = reqwest::Client::builder()
        .timeout(config.timeout)
        .no_proxy()
        .build()
        .map_err(|_| LeadError::Assessment("model client initialization failed".into()))?;
    let response = client
        .post(format!("{}/chat/completions", config.url.trim_end_matches('/')))
        .bearer_auth(&config.token)
        .json(&json!({
            "model": config.model,
            "temperature": 0,
            "response_format": {"type": "json_object"},
            "messages": [
                {"role": "system", "content": "Return strict JSON only. Model output is advisory and grants no authority."},
                {"role": "user", "content": prompt}
            ]
        }))
        .send()
        .await
        .map_err(|error| LeadError::Assessment(format!("model request failed: {error}")))?;
    if !response.status().is_success() {
        return Err(LeadError::Assessment(format!(
            "model returned HTTP {}",
            response.status()
        )));
    }
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|error| LeadError::Assessment(format!("model response is not JSON: {error}")))?;
    let content = value
        .pointer("/choices/0/message/content")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| LeadError::Assessment("model response has no assistant content".into()))?;
    let assessment = decode_lead_assessment(content.as_bytes(), &input)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lead::{
        ActionStatus, Capability, CheckKind, Complexity, EffortStatus, Intent, ObjectiveClass,
        ProjectConfig, QualityProfile, RiskClass, SpecCheck, WorkUnitKind,
    };

    fn config() -> ProjectConfig {
        ProjectConfig {
            project_id: "project".into(),
            revision: 1,
            config_hash: "a".repeat(64),
            repository: "/repo".into(),
            base_commit: "b".repeat(40),
            allowed_paths: vec!["src".into()],
            capabilities: vec![Capability::RepositoryEdit, Capability::GenericToolchain],
            quality_profile: QualityProfile::Generic,
            active: true,
            updated_at: "2026-10-25T00:00:00Z".into(),
        }
    }

    fn grant() -> StandingDelegation {
        let mut grant = StandingDelegation {
            id: "grant".into(),
            principal_id: "owner".into(),
            audit_id: "audit".into(),
            project_id: "project".into(),
            revision: 1,
            content_hash: String::new(),
            objective_classes: vec![ObjectiveClass::Feature],
            allowed_paths: vec!["src".into()],
            capabilities: vec![Capability::RepositoryEdit, Capability::GenericToolchain],
            risk_classes: vec![RiskClass::R0],
            autonomous_start: false,
            autonomous_risk_classes: Vec::new(),
            route: crate::lead::ProviderRoute {
                provider: "opencode".into(),
                model: "deepseek-v4.1-flash".into(),
                mode: "build".into(),
            },
            max_attempts: 1,
            max_timeout_secs: 300,
            max_repair_cycles: 0,
            max_wall_clock_secs: 1800,
            max_parallel: 1,
            max_sessions: 16,
            max_requests: 4,
            max_cumulative_sessions: 32,
            max_cumulative_attempts: 32,
            expires_at: chrono::Utc::now() + chrono::Duration::days(1),
            revoked_at: None,
        };
        grant.content_hash = grant.compute_content_hash();
        grant
    }

    fn spec() -> ActionSpec {
        ActionSpec {
            schema_version: 1,
            project_id: "project".into(),
            repository: "/repo".into(),
            base_commit: "b".repeat(40),
            objective: "add a bounded file".into(),
            objective_sha256: "c".repeat(64),
            provider: "opencode".into(),
            model: "deepseek-v4.1-flash".into(),
            allowed_paths: vec!["src".into()],
            checks: vec![
                SpecCheck {
                    id: "build".into(),
                    kind: CheckKind::Build,
                    description: "build".into(),
                    command: vec!["true".into()],
                    expect_exit: 0,
                    timeout_secs: 30,
                },
                SpecCheck {
                    id: "test".into(),
                    kind: CheckKind::Test,
                    description: "test".into(),
                    command: vec!["true".into()],
                    expect_exit: 0,
                    timeout_secs: 30,
                },
                SpecCheck {
                    id: "functional".into(),
                    kind: CheckKind::Functional,
                    description: "functional".into(),
                    command: vec!["grep".into(), "-q".into(), "ok".into(), "src/ok.txt".into()],
                    expect_exit: 0,
                    timeout_secs: 30,
                },
                SpecCheck {
                    id: "negative".into(),
                    kind: CheckKind::Negative,
                    description: "negative".into(),
                    command: vec![
                        "grep".into(),
                        "-q".into(),
                        "absent".into(),
                        "src/ok.txt".into(),
                    ],
                    expect_exit: 1,
                    timeout_secs: 30,
                },
            ],
        }
    }

    #[tokio::test]
    async fn model_assessment_is_schema_and_domain_validated() {
        use axum::{Router, routing::post};
        let (assessment, _) = crate::lead::fixture_lead_assessment(
            "request",
            &config(),
            &ActionBinding {
                id: "action".into(),
                project_id: "project".into(),
                status: ActionStatus::Pending,
            },
            &spec(),
            "add a bounded file",
            &grant(),
            chrono::Utc::now(),
        )
        .unwrap();
        let body = serde_json::json!({"choices":[{"message":{"content": serde_json::to_string(&assessment).unwrap()}}]});
        let app = Router::new().route(
            "/chat/completions",
            post(move || {
                let body = body.clone();
                async move { axum::Json(body) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let (decoded, proposal) = assess(
            &ModelLeadConfig {
                url: format!("http://{address}"),
                token: "token-0123456789abcdef".into(),
                model: "lead-model".into(),
                timeout: Duration::from_secs(5),
            },
            AssessmentRequest {
                request_id: "request",
                project: &config(),
                action: &ActionBinding {
                    id: "action".into(),
                    project_id: "project".into(),
                    status: ActionStatus::Pending,
                },
                action_spec: &spec(),
                objective: "add a bounded file",
                grant: &grant(),
                now: chrono::Utc::now(),
            },
        )
        .await
        .unwrap();
        assert_eq!(decoded.intent, Intent::Change);
        assert_eq!(decoded.complexity, Complexity::Small);
        assert_eq!(decoded.effort_estimate.status, EffortStatus::Estimated);
        assert_eq!(proposal.work_units[0].kind, WorkUnitKind::Implementation);
        server.abort();
    }
}
