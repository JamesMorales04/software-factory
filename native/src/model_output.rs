//! Typed model-output boundary: Rust-derived JSON Schema, provider compatibility
//! checks, Serde decoding and deterministic domain validation.
//!
//! A schema-valid model result is still advisory. Core authorization, budgets,
//! risk, WorkGraph validation and Quality Gate decisions remain separate.

use schemars::schema_for;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::lead::{AssessmentInput, Intent, LeadAssessment, bound_suggested_next_action};
use crate::planning::environment::EnvironmentLeadReviewModel;
use crate::planning::stack::StackSelectionModel;
use crate::semantic_review::{SemanticReview, SemanticReviewError, SemanticReviewInput};

#[derive(Debug, thiserror::Error)]
pub enum ModelOutputError {
    #[error("model output is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("model output does not satisfy the schema: {0}")]
    Schema(String),
    #[error("model output failed domain validation: {0}")]
    Domain(String),
    #[error("schema uses a provider-unsupported keyword: {0}")]
    UnsupportedSchema(String),
}

pub fn lead_assessment_schema() -> Value {
    serde_json::to_value(schema_for!(LeadAssessment))
        .expect("generated assessment schema is serializable")
}

pub fn lead_assessment_schema_sha256() -> String {
    let bytes = serde_json::to_vec(&lead_assessment_schema())
        .expect("generated assessment schema is serializable");
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn environment_lead_review_schema() -> Value {
    serde_json::to_value(schema_for!(EnvironmentLeadReviewModel))
        .expect("generated environment Lead review schema is serializable")
}

pub fn environment_lead_review_schema_sha256() -> String {
    let bytes = serde_json::to_vec(&environment_lead_review_schema())
        .expect("generated environment Lead review schema is serializable");
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn decode_environment_lead_review(
    raw: &[u8],
) -> Result<EnvironmentLeadReviewModel, ModelOutputError> {
    let value: Value = serde_json::from_slice(raw)?;
    let schema = environment_lead_review_schema();
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| ModelOutputError::Schema(error.to_string()))?;
    validator
        .validate(&value)
        .map_err(|error| ModelOutputError::Schema(error.to_string()))?;
    serde_json::from_value(value).map_err(ModelOutputError::Json)
}

pub fn agentic_triage_schema() -> Value {
    serde_json::to_value(schema_for!(crate::agentic::AgenticTriageModel))
        .expect("generated agentic triage schema is serializable")
}

pub fn agentic_triage_schema_sha256() -> String {
    let bytes = serde_json::to_vec(&agentic_triage_schema())
        .expect("generated agentic triage schema is serializable");
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn decode_agentic_triage(
    raw: &[u8],
) -> Result<crate::agentic::AgenticTriageModel, ModelOutputError> {
    let value: Value = serde_json::from_slice(raw)?;
    let schema = agentic_triage_schema();
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| ModelOutputError::Schema(error.to_string()))?;
    validator
        .validate(&value)
        .map_err(|error| ModelOutputError::Schema(error.to_string()))?;
    serde_json::from_value(value).map_err(ModelOutputError::Json)
}

pub fn stack_selection_schema() -> Value {
    serde_json::to_value(schema_for!(StackSelectionModel))
        .expect("generated stack selection schema is serializable")
}

pub fn stack_selection_schema_sha256() -> String {
    let bytes = serde_json::to_vec(&stack_selection_schema())
        .expect("generated stack selection schema is serializable");
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Schema-validate the bounded stack-selection model output. Core still
/// resolves the selection against the deterministic catalog and project
/// capabilities; schema validity is not a selection grant.
pub fn decode_stack_selection(raw: &[u8]) -> Result<StackSelectionModel, ModelOutputError> {
    let value: Value = serde_json::from_slice(raw)?;
    let schema = stack_selection_schema();
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| ModelOutputError::Schema(error.to_string()))?;
    validator
        .validate(&value)
        .map_err(|error| ModelOutputError::Schema(error.to_string()))?;
    serde_json::from_value(value).map_err(ModelOutputError::Json)
}

pub fn work_graph_schema() -> Value {
    serde_json::to_value(schema_for!(crate::agentic::WorkGraphModel))
        .expect("generated work graph schema is serializable")
}

pub fn work_graph_schema_sha256() -> String {
    let bytes = serde_json::to_vec(&work_graph_schema())
        .expect("generated work graph schema is serializable");
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Schema-validate a Planner proposal. Core validates graph semantics
/// separately with `agentic::validate_work_graph`.
pub fn decode_work_graph(raw: &[u8]) -> Result<crate::agentic::WorkGraphModel, ModelOutputError> {
    let value: Value = serde_json::from_slice(raw)?;
    let schema = work_graph_schema();
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| ModelOutputError::Schema(error.to_string()))?;
    validator
        .validate(&value)
        .map_err(|error| ModelOutputError::Schema(error.to_string()))?;
    serde_json::from_value(value).map_err(ModelOutputError::Json)
}

pub fn spec_draft_schema() -> Value {
    serde_json::to_value(schema_for!(crate::agentic::SpecDraftModel))
        .expect("generated spec draft schema is serializable")
}

pub fn spec_draft_schema_sha256() -> String {
    let bytes = serde_json::to_vec(&spec_draft_schema())
        .expect("generated spec draft schema is serializable");
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Schema-validate a Spec Manager draft. Deterministic compilation and
/// freezing are separate Core steps.
pub fn decode_spec_draft(raw: &[u8]) -> Result<crate::agentic::SpecDraftModel, ModelOutputError> {
    let value: Value = serde_json::from_slice(raw)?;
    let schema = spec_draft_schema();
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| ModelOutputError::Schema(error.to_string()))?;
    validator
        .validate(&value)
        .map_err(|error| ModelOutputError::Schema(error.to_string()))?;
    serde_json::from_value(value).map_err(ModelOutputError::Json)
}

pub fn semantic_review_schema() -> Value {
    serde_json::to_value(schema_for!(SemanticReview))
        .expect("generated semantic review schema is serializable")
}

pub fn semantic_review_schema_sha256() -> String {
    let bytes = serde_json::to_vec(&semantic_review_schema())
        .expect("generated semantic review schema is serializable");
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Schema-validate then apply the strict semantic-review domain invariants.
/// Malformed, off-identity or internally inconsistent output is rejected; it is
/// never converted into an approval.
pub fn decode_semantic_review(
    raw: &[u8],
    input: &SemanticReviewInput,
) -> Result<SemanticReview, ModelOutputError> {
    validate_semantic_review_schema(raw)?;
    SemanticReview::from_json(raw, input)
        .map_err(|error: SemanticReviewError| ModelOutputError::Domain(error.to_string()))
}

pub fn validate_semantic_review_schema(raw: &[u8]) -> Result<(), ModelOutputError> {
    let value: Value = serde_json::from_slice(raw)?;
    let schema = semantic_review_schema();
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| ModelOutputError::Schema(error.to_string()))?;
    validator
        .validate(&value)
        .map_err(|error| ModelOutputError::Schema(error.to_string()))
}

/// Validate the raw value against the generated schema, then deserialize and
/// apply the Lead domain invariants. Provider adapters must call this instead
/// of decoding free-form prose.
pub fn decode_lead_assessment(
    raw: &[u8],
    input: &AssessmentInput,
) -> Result<LeadAssessment, ModelOutputError> {
    let mut value: Value = serde_json::from_slice(raw)?;
    // The schema requires 1..=MAX_NOTE_CHARS characters so provider-native
    // structured output is guided correctly. The advisory field is also
    // deterministically bounded before schema validation so an otherwise safe
    // assessment is not blocked by a missing or overlong next action. Unknown
    // fields and non-string values are left untouched and still fail the schema.
    let intent = value
        .get("intent")
        .cloned()
        .and_then(|raw| serde_json::from_value::<Intent>(raw).ok());
    if let Some(intent) = intent
        && let Some(action) = value.get_mut("suggestedNextAction")
        && let Some(text) = action.as_str()
    {
        let mut bounded = text.to_owned();
        if bound_suggested_next_action(&mut bounded, intent) {
            *action = Value::String(bounded);
        }
    }
    let schema = lead_assessment_schema();
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| ModelOutputError::Schema(error.to_string()))?;
    validator
        .validate(&value)
        .map_err(|error| ModelOutputError::Schema(error.to_string()))?;
    LeadAssessment::from_json(raw, input)
        .map_err(|error| ModelOutputError::Domain(error.to_string()))
}

pub fn validate_lead_assessment_schema(raw: &[u8]) -> Result<(), ModelOutputError> {
    let value: Value = serde_json::from_slice(raw)?;
    let schema = lead_assessment_schema();
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| ModelOutputError::Schema(error.to_string()))?;
    validator
        .validate(&value)
        .map_err(|error| ModelOutputError::Schema(error.to_string()))
}

/// Conservative compatibility gate for the JSON Schema subset we send to Codex
/// App Server and OpenCode. This does not replace provider version testing; it
/// fails closed on a schema keyword we have not deliberately classified.
pub fn validate_provider_schema(schema: &Value) -> Result<(), ModelOutputError> {
    const ALLOWED: &[&str] = &[
        "$schema",
        "$defs",
        "$ref",
        "title",
        "description",
        "default",
        "examples",
        "deprecated",
        "readOnly",
        "writeOnly",
        "type",
        "properties",
        "required",
        "additionalProperties",
        "items",
        "prefixItems",
        "anyOf",
        "oneOf",
        "allOf",
        "enum",
        "const",
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "minLength",
        "maxLength",
        "pattern",
        "minItems",
        "maxItems",
        "uniqueItems",
        "format",
    ];
    visit_schema(schema, ALLOWED)
}

fn visit_schema(schema: &Value, allowed: &[&str]) -> Result<(), ModelOutputError> {
    let Some(object) = schema.as_object() else {
        return Ok(());
    };
    for (key, value) in object {
        if !allowed.contains(&key.as_str()) {
            return Err(ModelOutputError::UnsupportedSchema(format!(
                "keyword {key:?}"
            )));
        }
        match key.as_str() {
            "properties" | "$defs" => {
                let Some(map) = value.as_object() else {
                    return Err(ModelOutputError::UnsupportedSchema(format!(
                        "{key} must be an object"
                    )));
                };
                for nested in map.values() {
                    visit_schema(nested, allowed)?;
                }
            }
            "items" | "additionalProperties" | "not" => visit_schema(value, allowed)?,
            "prefixItems" | "anyOf" | "oneOf" | "allOf" => {
                let Some(items) = value.as_array() else {
                    return Err(ModelOutputError::UnsupportedSchema(format!(
                        "{key} must be an array"
                    )));
                };
                for nested in items {
                    visit_schema(nested, allowed)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lead::{
        AssessmentInput, Complexity, EffortEstimate, EffortStatus, Intent, LeadAssessment,
    };
    use std::collections::BTreeSet;

    fn input() -> AssessmentInput {
        AssessmentInput {
            project_id: "project".into(),
            action_id: "action".into(),
            request_id: "request".into(),
            objective: "audit the change".into(),
            allowed_evidence_refs: BTreeSet::from(["repository-files".to_string()]),
        }
    }

    fn assessment() -> LeadAssessment {
        LeadAssessment {
            schema_revision: 1,
            project_id: "project".into(),
            action_id: "action".into(),
            request_id: "request".into(),
            evidence_refs: vec!["repository-files".into()],
            intent: Intent::Audit,
            complexity: Complexity::Small,
            effort_estimate: EffortEstimate {
                status: EffortStatus::NotRequested,
                unit: String::new(),
                min_person_hours: 0,
                max_person_hours: 0,
                assumptions: Vec::new(),
                unknown_reason: String::new(),
            },
            knowledge_gaps: Vec::new(),
            change_impact: Vec::new(),
            validation_needs: Vec::new(),
            assumptions: Vec::new(),
            uncertainty: Vec::new(),
            suggested_next_action: "Ask for a bounded scope.".into(),
            proposal: None,
        }
    }

    #[test]
    fn generated_schema_is_closed_and_provider_compatible() {
        let schema = lead_assessment_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(
            schema["properties"]["suggestedNextAction"]["type"],
            "string"
        );
        assert_eq!(
            schema["properties"]["suggestedNextAction"]["minLength"],
            serde_json::json!(1)
        );
        assert_eq!(
            schema["properties"]["suggestedNextAction"]["maxLength"],
            serde_json::json!(crate::lead::MAX_NOTE_CHARS)
        );
        validate_provider_schema(&schema).unwrap();
        assert_eq!(lead_assessment_schema_sha256().len(), 64);
    }

    #[test]
    fn schema_validation_rejects_unknown_fields_before_domain_validation() {
        let raw = serde_json::to_vec(&assessment()).unwrap();
        decode_lead_assessment(&raw, &input()).unwrap();
        let mut value: Value = serde_json::from_slice(&raw).unwrap();
        value["unexpected"] = serde_json::json!(true);
        let raw = serde_json::to_vec(&value).unwrap();
        assert!(matches!(
            decode_lead_assessment(&raw, &input()),
            Err(ModelOutputError::Schema(_))
        ));

        let mut wrong_type: Value =
            serde_json::from_slice(&serde_json::to_vec(&assessment()).unwrap()).unwrap();
        wrong_type["suggestedNextAction"] = serde_json::json!(7);
        assert!(matches!(
            decode_lead_assessment(&serde_json::to_vec(&wrong_type).unwrap(), &input()),
            Err(ModelOutputError::Schema(_))
        ));
    }

    #[test]
    fn informational_assessment_next_action_is_bounded_and_planless() {
        let mut value = serde_json::to_value(assessment()).unwrap();
        value["intent"] = serde_json::json!("informational");
        for action in [serde_json::json!(""), serde_json::json!("   ")] {
            value["suggestedNextAction"] = action;
            let decoded =
                decode_lead_assessment(&serde_json::to_vec(&value).unwrap(), &input()).unwrap();
            assert_eq!(decoded.intent, Intent::Informational);
            assert_eq!(
                decoded.suggested_next_action,
                crate::lead::SUGGESTED_NEXT_ACTION_NO_ACTION
            );
            assert!(decoded.proposal.is_none());
        }
    }

    #[test]
    fn overlong_next_action_is_truncated_on_a_character_boundary() {
        let mut value = serde_json::to_value(assessment()).unwrap();
        value["intent"] = serde_json::json!("informational");
        value["suggestedNextAction"] =
            serde_json::json!("é".repeat(crate::lead::MAX_NOTE_CHARS + 25));
        let decoded =
            decode_lead_assessment(&serde_json::to_vec(&value).unwrap(), &input()).unwrap();
        assert_eq!(
            decoded.suggested_next_action.chars().count(),
            crate::lead::MAX_NOTE_CHARS
        );
        assert!(decoded.suggested_next_action.starts_with('é'));
        assert!(decoded.proposal.is_none());
    }

    #[test]
    fn stack_selection_schema_is_provider_compatible_and_round_trips() {
        use crate::planning::environment::{
            EnvironmentProposal, PackageRequirement, ToolchainRequirement,
        };
        let schema = stack_selection_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        validate_provider_schema(&schema).unwrap();
        assert_eq!(stack_selection_schema_sha256().len(), 64);
        assert!(schema["properties"]["environmentProposal"].is_object());

        let selection = StackSelectionModel {
            option_id: "python-service".into(),
            language: "python".into(),
            framework: "fastapi".into(),
            rationale: "A calculator is a compact, testable Python service.".into(),
            environment_proposal: Some(EnvironmentProposal {
                nixpkgs: Some(
                    "github:NixOS/nixpkgs/2222222222222222222222222222222222222222".into(),
                ),
                toolchains: vec![ToolchainRequirement {
                    id: "python".into(),
                    version: "3.12.7".into(),
                    source: "lead-proposal".into(),
                    evidence: Vec::new(),
                }],
                packages: vec![PackageRequirement {
                    name: "python3".into(),
                    version: "3.12.7".into(),
                    license: "MIT".into(),
                    source: "nixpkgs".into(),
                    attr: "python312".into(),
                    sha256: String::new(),
                    unfree: false,
                    attribution: "Python is MIT licensed.".into(),
                }],
                ..EnvironmentProposal::default()
            }),
        };
        let raw = serde_json::to_vec(&selection).unwrap();
        let decoded = decode_stack_selection(&raw).unwrap();
        assert_eq!(decoded, selection);
        let mut unknown: Value = serde_json::from_slice(&raw).unwrap();
        unknown["unexpected"] = serde_json::json!(true);
        assert!(matches!(
            decode_stack_selection(&serde_json::to_vec(&unknown).unwrap()),
            Err(ModelOutputError::Schema(_))
        ));
    }

    #[test]
    fn generated_semantic_schema_is_closed_and_provider_compatible() {
        let schema = semantic_review_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        validate_provider_schema(&schema).unwrap();
        assert_eq!(semantic_review_schema_sha256().len(), 64);
        let required = schema["required"].as_array().unwrap();
        for field in [
            "schemaRevision",
            "candidateCommitSha",
            "candidateTreeSha",
            "actionSpecSha256",
            "verdict",
            "summary",
            "confidence",
            "findings",
        ] {
            assert!(required.iter().any(|value| value == field), "{field}");
        }
    }

    #[test]
    fn semantic_decode_rejects_unknown_fields_wrong_candidate_and_blocking_approval() {
        use crate::review::{FindingCategory, FindingSeverity, ReviewFinding};
        use crate::semantic_review::{SemanticReview, SemanticReviewInput, SemanticVerdict};

        let input = SemanticReviewInput {
            candidate_commit_sha: "a".repeat(40),
            candidate_tree_sha: "b".repeat(40),
            action_spec_sha256: "c".repeat(64),
        };
        let review = SemanticReview {
            schema_revision: 1,
            candidate_commit_sha: "a".repeat(40),
            candidate_tree_sha: "b".repeat(40),
            action_spec_sha256: "c".repeat(64),
            verdict: SemanticVerdict::Approve,
            summary: "plausible".into(),
            confidence: 90,
            findings: Vec::new(),
        };
        let raw = serde_json::to_vec(&review).unwrap();
        decode_semantic_review(&raw, &input).unwrap();

        let mut medical: Value = serde_json::from_slice(&raw).unwrap();
        medical["unexpected"] = serde_json::json!(true);
        assert!(matches!(
            decode_semantic_review(&serde_json::to_vec(&medical).unwrap(), &input),
            Err(ModelOutputError::Schema(_))
        ));

        let mut wrong: Value = serde_json::from_slice(&raw).unwrap();
        wrong["candidateCommitSha"] = serde_json::json!("d".repeat(40));
        assert!(matches!(
            decode_semantic_review(&serde_json::to_vec(&wrong).unwrap(), &input),
            Err(ModelOutputError::Domain(_))
        ));

        let mut blocking: Value = serde_json::from_slice(&raw).unwrap();
        blocking["findings"] = serde_json::to_value(vec![ReviewFinding {
            id: "f1".into(),
            severity: FindingSeverity::Blocking,
            category: FindingCategory::Security,
            message: "credential path".into(),
            points: 5,
            rationale: "The diff touches a credential path.".into(),
            evidence_refs: vec!["src/credential.rs".into()],
        }])
        .unwrap();
        assert!(matches!(
            decode_semantic_review(&serde_json::to_vec(&blocking).unwrap(), &input),
            Err(ModelOutputError::Domain(_))
        ));
    }

    #[test]
    fn unsupported_provider_keyword_fails_closed() {
        let schema = serde_json::json!({"type":"object","dependentSchemas":{}});
        assert!(matches!(
            validate_provider_schema(&schema),
            Err(ModelOutputError::UnsupportedSchema(_))
        ));
    }
}
