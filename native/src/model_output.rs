//! Typed model-output boundary: Rust-derived JSON Schema, provider compatibility
//! checks, Serde decoding and deterministic domain validation.
//!
//! A schema-valid model result is still advisory. Core authorization, budgets,
//! risk, WorkGraph validation and Quality Gate decisions remain separate.

use schemars::schema_for;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::lead::{AssessmentInput, LeadAssessment};

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

/// Validate the raw value against the generated schema, then deserialize and
/// apply the Lead domain invariants. Provider adapters must call this instead
/// of decoding free-form prose.
pub fn decode_lead_assessment(
    raw: &[u8],
    input: &AssessmentInput,
) -> Result<LeadAssessment, ModelOutputError> {
    validate_lead_assessment_schema(raw)?;
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
