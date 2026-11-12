//! Deterministic stack decisions.
//!
//! Stack selection is a Core policy decision, not a model privilege. An
//! existing repository stack is preserved. For a greenfield request, one
//! bounded Lead-route model call may select an exact deterministic option (or a
//! language/framework that maps to one); Core validates the catalog entry and
//! the required project capability and records `StackSource::Model`. An unknown,
//! empty or unauthorized selection fails closed; there is no per-language
//! default. A stack change that exceeds the R0 autonomy budget still requires
//! owner approval through the existing autonomy policy.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::agentic::StackProfile;
use crate::lead::Capability;
use crate::planning::environment::EnvironmentProposal;
use crate::planning::route::UserPlanningConstraints;
use crate::planning::{
    DependencyPlan, DependencyStrategy, PlanningLane, StackDecision, StackOption, StackSource,
};

const MAX_CAPABILITIES: usize = 16;
const MAX_SELECTION_RATIONALE_CHARS: usize = 500;

/// One deterministic Core-supported stack option. The catalog is a bounded
/// decision surface, not a per-language package or image default. A model may
/// select one of these options for a greenfield request; it cannot add options,
/// capabilities or paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StackSelectionOption {
    pub id: String,
    pub language: String,
    pub framework: String,
    pub summary: String,
    /// Project capability required to execute the selected stack. This is an
    /// authorization fact from the deterministic catalog, never a model grant.
    pub required_capability: Capability,
}

/// Typed, untrusted stack-selection model output. `optionId` is the exact
/// deterministic option; language and framework must agree with it. A model may
/// also omit `optionId` and return a concrete language/framework that Core maps
/// to exactly one option. The optional environment proposal is used only when
/// the recorded Lead plan did not already carry one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StackSelectionModel {
    #[serde(default)]
    pub option_id: String,
    pub language: String,
    pub framework: String,
    pub rationale: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_proposal: Option<EnvironmentProposal>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StackSelectionError {
    #[error("the stack selection is empty or malformed: {0}")]
    Invalid(String),
    #[error("the stack selection {0:?} is not one of the deterministic options")]
    Unknown(String),
    #[error("the selected stack requires an unauthorized project capability: {0:?}")]
    Unauthorized(Capability),
}

/// Deterministically resolve one model stack selection against the exact
/// catalog and the project's authorized capabilities.
pub fn resolve_model_stack_selection(
    options: &[StackSelectionOption],
    selection: &StackSelectionModel,
    project_capabilities: &[Capability],
) -> Result<StackSelectionOption, StackSelectionError> {
    let language = selection.language.trim();
    let framework = selection.framework.trim();
    let option_id = selection.option_id.trim();
    if language.is_empty() || framework.is_empty() {
        return Err(StackSelectionError::Invalid(
            "language and framework are required".into(),
        ));
    }
    if selection.rationale.trim().is_empty()
        || selection.rationale.chars().count() > MAX_SELECTION_RATIONALE_CHARS
    {
        return Err(StackSelectionError::Invalid(
            "rationale must be non-empty and bounded".into(),
        ));
    }
    let selected = if option_id.is_empty() {
        let mut matches = options.iter().filter(|option| {
            option.language.eq_ignore_ascii_case(language)
                && option.framework.eq_ignore_ascii_case(framework)
        });
        let selected = matches
            .next()
            .ok_or_else(|| StackSelectionError::Unknown(format!("{language}/{framework}")))?;
        if matches.next().is_some() {
            return Err(StackSelectionError::Invalid(
                "language and framework identify more than one deterministic option".into(),
            ));
        }
        selected
    } else {
        let selected = options
            .iter()
            .find(|option| option.id == option_id)
            .ok_or_else(|| StackSelectionError::Unknown(option_id.into()))?;
        if !selected.language.eq_ignore_ascii_case(language)
            || !selected.framework.eq_ignore_ascii_case(framework)
        {
            return Err(StackSelectionError::Invalid(format!(
                "option {option_id:?} does not match the returned language/framework"
            )));
        }
        selected
    };
    if !project_capabilities.contains(&selected.required_capability) {
        return Err(StackSelectionError::Unauthorized(
            selected.required_capability,
        ));
    }
    Ok(selected.clone())
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
    let words = lower
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    let web = ["web", "ui", "browser", "frontend", "office", "dashboard"]
        .iter()
        .any(|word| words.contains(word));
    let cli = words.contains("cli") || lower.contains("command line");
    let mut options = Vec::new();
    if web {
        options.push(StackSelectionOption {
            id: "typescript-react".into(),
            language: "typescript".into(),
            framework: "react".into(),
            summary: "TypeScript, React and Vite for a browser application.".into(),
            required_capability: Capability::NodeToolchain,
        });
    }
    if cli {
        options.push(StackSelectionOption {
            id: "rust-cli".into(),
            language: "rust".into(),
            framework: "clap".into(),
            summary: "Rust with clap for a small, portable command-line tool.".into(),
            required_capability: Capability::GenericToolchain,
        });
    }
    options.push(StackSelectionOption {
        id: "rust-service".into(),
        language: "rust".into(),
        framework: "axum".into(),
        summary: "Rust, axum and SQLx for a typed local service.".into(),
        required_capability: Capability::GenericToolchain,
    });
    options.push(StackSelectionOption {
        id: "python-service".into(),
        language: "python".into(),
        framework: "fastapi".into(),
        summary: "Python and FastAPI for a compact service.".into(),
        required_capability: Capability::GenericToolchain,
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
        let dependency_plan = dependency_plan_for(
            if detected.languages.iter().any(|language| language == "rust") {
                "rust"
            } else {
                ""
            },
        );
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
    let dependency_plan = dependency_plan_for(language);
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

fn dependency_plan_for(language: &str) -> DependencyPlan {
    if language.eq_ignore_ascii_case("rust") {
        DependencyPlan {
            strategy: DependencyStrategy::VendorCache,
            items: Vec::new(),
            cache_ref: "vendor".into(),
            sbom_ref: "artifacts/sbom.json".into(),
            owner_approval: None,
        }
    } else {
        DependencyPlan::default()
    }
}

fn catalog_option(candidate: &StackSelectionOption, selected_id: &str) -> StackOption {
    let selected = candidate.id == selected_id;
    StackOption {
        id: candidate.id.clone(),
        summary: candidate.summary.clone(),
        fit: 5,
        security: 4,
        maintainability: 4,
        operability: 4,
        time_to_value: 4,
        exit_cost: 3,
        pros: vec![if selected {
            "Selected by the bounded Lead planner model and validated by Core.".into()
        } else {
            "Deterministic Core candidate considered by the Lead planner.".into()
        }],
        cons: Vec::new(),
        evidence_refs: vec!["deterministic-stack-options".into()],
    }
}

/// Build a stack decision after Core has resolved and validated one
/// model-selected deterministic option. The recorded decision names every
/// candidate, the exact selected id and the model-selected source. The model
/// never supplies capabilities, paths or a package mapping.
pub fn build_model_stack_decision(
    lane: PlanningLane,
    detected: &StackProfile,
    selected: &StackSelectionOption,
    candidates: &[StackSelectionOption],
    _objective: &str,
) -> Result<StackDecision, StackBuildError> {
    if candidates.is_empty() || !candidates.iter().any(|option| option.id == selected.id) {
        return Err(StackBuildError::Invalid(
            "the selected stack is not one of the deterministic options".into(),
        ));
    }
    let decision = StackDecision {
        schema_version: crate::planning::SCHEMA_VERSION,
        source: StackSource::Model,
        constraints: Vec::new(),
        options: candidates
            .iter()
            .map(|option| catalog_option(option, &selected.id))
            .collect(),
        selected: selected.id.clone(),
        reason: format!(
            "The semantic triage and deterministic stack were empty. The bounded Lead planner selected deterministic option {}; Core validated it against the catalog and project capabilities. Model output granted no capabilities or paths.",
            selected.id
        ),
        confidence: "model-selected".into(),
        required_capabilities: capabilities(detected, &selected.language, &selected.framework),
        dependency_plan: dependency_plan_for(&selected.language),
        stack_change: false,
        autonomy_points: 0,
        owner_approval: None,
        adr_path: "docs/architecture/decisions/ADR-0001-stack.md".into(),
    };
    decision.validate(lane).map_err(StackBuildError::Invalid)?;
    Ok(decision)
}

/// Project the selected stack decision into the `StackProfile` consumed by the
/// architecture, skill and Definition of Done artifacts. Detected evidence is
/// preserved; the selected language/framework is added from the exact catalog.
pub fn selected_stack_profile(
    detected: &StackProfile,
    decision: &StackDecision,
    objective: &str,
) -> StackProfile {
    let mut profile = detected.clone();
    let Some(selected) = selection_options(objective)
        .into_iter()
        .find(|option| option.id == decision.selected)
    else {
        return profile;
    };
    for (target, value) in [
        (&mut profile.languages, selected.language.as_str()),
        (&mut profile.frameworks, selected.framework.as_str()),
    ] {
        if !value.is_empty() && !target.iter().any(|item| item.eq_ignore_ascii_case(value)) {
            target.push(value.to_owned());
        }
    }
    if profile.confidence.trim().is_empty() || profile.confidence == "low" {
        profile.confidence = "model-selected".into();
    }
    profile
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
    fn selection_options_do_not_treat_build_as_a_ui_word() {
        let options = selection_options("Build a calculator application. You choose the stack.");
        assert!(!options.iter().any(|option| option.id == "typescript-react"));
        assert!(options.iter().any(|option| option.id == "rust-service"));
        assert!(options.iter().any(|option| option.id == "python-service"));
    }

    #[test]
    fn model_selection_resolves_an_exact_option_and_fails_closed() {
        let options = selection_options("Build a calculator application. You choose the stack.");
        let selection = StackSelectionModel {
            option_id: "python-service".into(),
            language: "python".into(),
            framework: "fastapi".into(),
            rationale: "A small calculator is quickly testable as a Python service.".into(),
            environment_proposal: None,
        };
        let selected =
            resolve_model_stack_selection(&options, &selection, &[Capability::GenericToolchain])
                .unwrap();
        assert_eq!(selected.id, "python-service");

        let unknown = StackSelectionModel {
            option_id: "go-service".into(),
            language: "go".into(),
            framework: "gin".into(),
            rationale: "A Go service is not in the deterministic catalog.".into(),
            environment_proposal: None,
        };
        assert!(matches!(
            resolve_model_stack_selection(&options, &unknown, &[Capability::GenericToolchain]),
            Err(StackSelectionError::Unknown(_))
        ));

        let unauthorized = StackSelectionModel {
            option_id: "python-service".into(),
            language: "python".into(),
            framework: "fastapi".into(),
            rationale: "A small calculator is quickly testable as a Python service.".into(),
            environment_proposal: None,
        };
        assert_eq!(
            resolve_model_stack_selection(&options, &unauthorized, &[Capability::RepositoryEdit])
                .unwrap_err(),
            StackSelectionError::Unauthorized(Capability::GenericToolchain)
        );

        let mismatched = StackSelectionModel {
            option_id: "python-service".into(),
            language: "rust".into(),
            framework: "axum".into(),
            rationale: "Mismatched fields must be rejected.".into(),
            environment_proposal: None,
        };
        assert!(matches!(
            resolve_model_stack_selection(&options, &mismatched, &[Capability::GenericToolchain]),
            Err(StackSelectionError::Invalid(_))
        ));
    }

    #[test]
    fn model_selected_stack_decision_records_candidates_and_model_source() {
        let options = selection_options("Build a calculator application. You choose the stack.");
        let selected = options
            .iter()
            .find(|option| option.id == "rust-service")
            .unwrap()
            .clone();
        let decision = build_model_stack_decision(
            PlanningLane::FormalSpec,
            &StackProfile::default(),
            &selected,
            &options,
            "Build a calculator application. You choose the stack.",
        )
        .unwrap();
        assert_eq!(decision.source, StackSource::Model);
        assert_eq!(decision.selected, "rust-service");
        assert_eq!(decision.options.len(), options.len());
        assert!(!decision.stack_change);
        assert!(
            decision
                .required_capabilities
                .iter()
                .any(|value| value == "rust")
        );
    }

    #[test]
    fn selected_stack_profile_carries_the_model_choice() {
        let options = selection_options("Build a calculator application. You choose the stack.");
        let selected = options
            .iter()
            .find(|option| option.id == "python-service")
            .unwrap()
            .clone();
        let decision = build_model_stack_decision(
            PlanningLane::FormalSpec,
            &StackProfile::default(),
            &selected,
            &options,
            "Build a calculator application. You choose the stack.",
        )
        .unwrap();
        let profile = selected_stack_profile(
            &StackProfile::default(),
            &decision,
            "Build a calculator application. You choose the stack.",
        );
        assert!(profile.languages.iter().any(|value| value == "python"));
        assert!(profile.frameworks.iter().any(|value| value == "fastapi"));
        assert_eq!(profile.confidence, "model-selected");
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
