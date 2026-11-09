//! Deterministic stack decisions.
//!
//! Stack selection is a Core policy decision, not a model privilege. An
//! existing repository stack is preserved. A greenfield request must supply an
//! explicit language/framework choice; Core presents deterministic options and
//! refuses to invent a stack. A stack change that exceeds the R0 autonomy
//! budget still requires owner approval through the existing autonomy policy.

use serde::{Deserialize, Serialize};

use crate::agentic::StackProfile;
use crate::planning::route::UserPlanningConstraints;
use crate::planning::{
    DependencyPlan, DependencyStrategy, PlanningLane, StackDecision, StackOption, StackSource,
};

const MAX_CAPABILITIES: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackSelectionOption {
    pub id: String,
    pub language: String,
    pub framework: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StackBuildError {
    #[error("stack selection is required before planning")]
    SelectionRequired { options: Vec<StackSelectionOption> },
    #[error("invalid stack input: {0}")]
    Invalid(String),
}

/// Deterministic options shown to the owner when a project has no stack. The
/// list is a Core-supported catalog, not a model recommendation.
pub fn selection_options(objective: &str) -> Vec<StackSelectionOption> {
    let lower = objective.to_ascii_lowercase();
    let web = lower.contains("web")
        || lower.contains("ui")
        || lower.contains("browser")
        || lower.contains("frontend")
        || lower.contains("office")
        || lower.contains("dashboard");
    let cli = lower.contains("cli") || lower.contains("command line");
    let mut options = Vec::new();
    if web {
        options.push(StackSelectionOption {
            id: "typescript-react".into(),
            language: "typescript".into(),
            framework: "react".into(),
            summary: "TypeScript, React and Vite for a browser application.".into(),
        });
    }
    if cli {
        options.push(StackSelectionOption {
            id: "rust-cli".into(),
            language: "rust".into(),
            framework: "clap".into(),
            summary: "Rust with clap for a small, portable command-line tool.".into(),
        });
    }
    options.push(StackSelectionOption {
        id: "rust-service".into(),
        language: "rust".into(),
        framework: "axum".into(),
        summary: "Rust, axum and SQLx for a typed local service.".into(),
    });
    options.push(StackSelectionOption {
        id: "python-service".into(),
        language: "python".into(),
        framework: "fastapi".into(),
        summary: "Python and FastAPI for a compact service.".into(),
    });
    options
}

fn option(id: &str, summary: String, evidence_refs: Vec<String>) -> StackOption {
    StackOption {
        id: id.into(),
        summary,
        fit: 5,
        security: 4,
        maintainability: 4,
        operability: 4,
        time_to_value: 4,
        exit_cost: 3,
        pros: vec!["Matches the observed or explicitly selected repository stack.".into()],
        cons: Vec::new(),
        evidence_refs,
    }
}

fn capabilities(detected: &StackProfile, language: &str, framework: &str) -> Vec<String> {
    let mut values = Vec::new();
    for value in detected
        .languages
        .iter()
        .chain(detected.frameworks.iter())
        .chain(detected.package_managers.iter())
        .chain(detected.build_tools.iter())
    {
        if !values.iter().any(|existing| existing == value) {
            values.push(value.clone());
        }
    }
    for value in [language, framework] {
        if !value.is_empty() && !values.iter().any(|existing| existing == value) {
            values.push(value.to_string());
        }
    }
    values.sort();
    values.truncate(MAX_CAPABILITIES);
    values
}

/// Build a stack decision from the observed repository stack or an explicit
/// owner selection. No model call is made.
pub fn build_stack_decision(
    lane: PlanningLane,
    detected: &StackProfile,
    constraints: &UserPlanningConstraints,
    objective: &str,
) -> Result<StackDecision, StackBuildError> {
    let selected_language = constraints
        .language
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let selected_framework = constraints
        .framework
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if !detected.languages.is_empty() {
        let summary = format!(
            "Existing repository stack: {}; frameworks: {}; package managers: {}.",
            detected.languages.join(", "),
            if detected.frameworks.is_empty() {
                "none detected".into()
            } else {
                detected.frameworks.join(", ")
            },
            if detected.package_managers.is_empty() {
                "none detected".into()
            } else {
                detected.package_managers.join(", ")
            }
        );
        let capabilities = capabilities(
            detected,
            selected_language.unwrap_or_default(),
            selected_framework.unwrap_or_default(),
        );
        let dependency_plan = if detected.languages.iter().any(|language| language == "rust") {
            DependencyPlan {
                strategy: DependencyStrategy::VendorCache,
                items: Vec::new(),
                cache_ref: "vendor".into(),
                sbom_ref: "artifacts/sbom.json".into(),
                owner_approval: None,
            }
        } else {
            DependencyPlan::default()
        };
        let decision = StackDecision {
            schema_version: crate::planning::SCHEMA_VERSION,
            source: StackSource::Existing,
            constraints: constraints.dependencies.clone(),
            options: vec![option("existing", summary, detected.evidence_refs.clone())],
            selected: "existing".into(),
            reason: "Preserve the stack already pinned by the repository; no unapproved stack change is introduced."
                .into(),
            confidence: detected.confidence.clone(),
            required_capabilities: capabilities,
            dependency_plan,
            stack_change: false,
            autonomy_points: 0,
            owner_approval: None,
            adr_path: "docs/architecture/decisions/ADR-0001-existing-stack.md".into(),
        };
        decision.validate(lane).map_err(StackBuildError::Invalid)?;
        return Ok(decision);
    }

    let (language, framework) = match (selected_language, selected_framework) {
        (Some(language), Some(framework)) => (language, framework),
        (Some(language), None) => (language, "none"),
        _ => {
            return Err(StackBuildError::SelectionRequired {
                options: selection_options(objective),
            });
        }
    };
    let capabilities = capabilities(detected, language, framework);
    let dependency_plan = if language.eq_ignore_ascii_case("rust") {
        DependencyPlan {
            strategy: DependencyStrategy::VendorCache,
            items: Vec::new(),
            cache_ref: "vendor".into(),
            sbom_ref: "artifacts/sbom.json".into(),
            owner_approval: None,
        }
    } else {
        DependencyPlan::default()
    };
    let decision = StackDecision {
        schema_version: crate::planning::SCHEMA_VERSION,
        source: StackSource::User,
        constraints: constraints.dependencies.clone(),
        options: vec![option(
            "user-selected",
            format!("Owner-selected stack: {language} / {framework}."),
            Vec::new(),
        )],
        selected: "user-selected".into(),
        reason:
            "The owner explicitly selected the language and framework for this greenfield request."
                .into(),
        confidence: "owner-selected".into(),
        required_capabilities: capabilities,
        dependency_plan,
        stack_change: false,
        autonomy_points: 0,
        owner_approval: None,
        adr_path: "docs/architecture/decisions/ADR-0001-stack.md".into(),
    };
    decision.validate(lane).map_err(StackBuildError::Invalid)?;
    Ok(decision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agentic::StackProfile;
    use crate::planning::PlanningLane;

    fn detected() -> StackProfile {
        StackProfile {
            languages: vec!["rust".into()],
            frameworks: vec!["axum".into()],
            package_managers: vec!["cargo".into()],
            build_tools: vec!["cargo".into()],
            evidence_refs: vec!["Cargo.toml".into()],
            confidence: "high".into(),
            ..StackProfile::default()
        }
    }

    #[test]
    fn existing_stack_is_preserved_without_owner_escalation() {
        let decision = build_stack_decision(
            PlanningLane::FormalSpec,
            &detected(),
            &UserPlanningConstraints::default(),
            "add a route",
        )
        .unwrap();
        assert_eq!(decision.source, StackSource::Existing);
        assert_eq!(decision.selected, "existing");
        assert!(!decision.stack_change);
        assert_eq!(decision.autonomy_points, 0);
        assert!(decision.required_capabilities.contains(&"rust".into()));
    }

    #[test]
    fn greenfield_without_selection_fails_closed_with_options() {
        let error = build_stack_decision(
            PlanningLane::FormalSpec,
            &StackProfile::default(),
            &UserPlanningConstraints::default(),
            "build a web dashboard",
        )
        .unwrap_err();
        match error {
            StackBuildError::SelectionRequired { options } => {
                assert!(options.iter().any(|option| option.id == "typescript-react"));
                assert!(options.iter().any(|option| option.id == "rust-service"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn owner_selection_builds_a_user_decision() {
        let constraints = UserPlanningConstraints {
            language: Some("python".into()),
            framework: Some("fastapi".into()),
            ..UserPlanningConstraints::default()
        };
        let decision = build_stack_decision(
            PlanningLane::FormalSpec,
            &StackProfile::default(),
            &constraints,
            "build a service",
        )
        .unwrap();
        assert_eq!(decision.source, StackSource::User);
        assert_eq!(decision.selected, "user-selected");
        assert!(decision.required_capabilities.contains(&"python".into()));
    }
}
