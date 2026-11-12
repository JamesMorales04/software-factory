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
pub const MAX_SELECTION_RATIONALE_CHARS: usize = 500;

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
/// to exactly one option. A greenfield selection must include a concrete
/// environment proposal covering the selected language/framework; Core
/// validates it with the existing environment compiler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StackSelectionModel {
    #[serde(default)]
    #[schemars(length(max = 64))]
    pub option_id: String,
    #[schemars(length(min = 1, max = 64))]
    pub language: String,
    #[schemars(length(min = 1, max = 64))]
    pub framework: String,
    /// One concise sentence explaining the selection; bounded to 500
    /// characters so provider structured output advertises the exact policy.
    #[schemars(length(min = 1, max = MAX_SELECTION_RATIONALE_CHARS))]
    pub rationale: String,
    pub environment_proposal: EnvironmentProposal,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StackSelectionError {
    #[error("the stack selection is empty or malformed: {0}")]
    Invalid(String),
    #[error("the stack selection requires a concrete environment proposal: {0}")]
    EnvironmentRequired(String),
    #[error("the environment proposal does not cover the selected stack: {0}")]
    EnvironmentMismatch(String),
    #[error("the stack selection {0:?} is not one of the deterministic options")]
    Unknown(String),
    #[error("the selected stack requires an unauthorized project capability: {0:?}")]
    Unauthorized(Capability),
}

/// Normalize a technology token to the family used for coverage checks. The
/// aliases match the existing `normalize_tool_name` conventions and add the
/// framework names used by the deterministic catalog.
fn stack_technology_kind(value: &str) -> Option<&'static str> {
    let lower = value.trim().to_ascii_lowercase();
    let lower = lower.trim_start_matches(|character: char| !character.is_ascii_alphanumeric());
    const PREFIXES: &[(&str, &str)] = &[
        ("nodejs", "node"),
        ("node", "node"),
        ("typescript", "node"),
        ("javascript", "node"),
        ("npm", "node"),
        ("pnpm", "node"),
        ("yarn", "node"),
        ("bun", "node"),
        ("python", "python"),
        ("pip", "python"),
        ("pytest", "python"),
        ("rust", "rust"),
        ("cargo", "rust"),
        ("fastapi", "fastapi"),
        ("axum", "axum"),
        ("react", "react"),
        ("next", "next"),
        ("vue", "vue"),
        ("angular", "angular"),
    ];
    PREFIXES
        .iter()
        .find(|(prefix, _)| lower.starts_with(prefix))
        .map(|(_, kind)| *kind)
}

fn proposal_covers_selected_stack(
    selected: &StackSelectionOption,
    proposal: &EnvironmentProposal,
) -> bool {
    let language_kind = stack_technology_kind(&selected.language)
        .map(str::to_owned)
        .unwrap_or_else(|| selected.language.to_ascii_lowercase());
    let framework_kind = stack_technology_kind(&selected.framework)
        .map(str::to_owned)
        .unwrap_or_else(|| selected.framework.to_ascii_lowercase());
    let matches = |candidate: &str| {
        stack_technology_kind(candidate)
            .is_some_and(|kind| kind == language_kind.as_str() || kind == framework_kind.as_str())
    };
    proposal
        .toolchains
        .iter()
        .any(|toolchain| matches(&toolchain.id))
        || proposal.packages.iter().any(|package| {
            matches(&package.name)
                || matches(&package.attr)
                || package.attr.split('.').any(&matches)
        })
        || proposal.commands.values().any(|command| {
            command
                .argv
                .iter()
                .take(2)
                .any(|argument| matches(argument))
        })
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
    selection
        .environment_proposal
        .validate()
        .map_err(|error| StackSelectionError::Invalid(error.to_string()))?;
    let proposal = &selection.environment_proposal;
    if proposal.toolchains.is_empty() && proposal.packages.is_empty() {
        return Err(StackSelectionError::EnvironmentRequired(
            "the selection must name at least one toolchain or package".into(),
        ));
    }
    if proposal.commands.is_empty() {
        return Err(StackSelectionError::EnvironmentRequired(
            "the selection must provide build/test/lint/format/typecheck/run commands".into(),
        ));
    }
    if proposal.packages.iter().any(|package| package.unfree) {
        return Err(StackSelectionError::EnvironmentRequired(
            "a greenfield proposal cannot include an unfree package without an owner override"
                .into(),
        ));
    }
    if !proposal_covers_selected_stack(selected, proposal) {
        return Err(StackSelectionError::EnvironmentMismatch(format!(
            "{}/{} is not covered by a matching toolchain, package or command",
            selected.language, selected.framework
        )));
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

    fn python_proposal() -> EnvironmentProposal {
        use crate::planning::environment::{
            CommandRequirement, PackageRequirement, ToolchainRequirement,
        };
        EnvironmentProposal {
            toolchains: vec![ToolchainRequirement {
                id: "python".into(),
                version: "3.12.7".into(),
                source: "stack-selection".into(),
                evidence: vec!["selected python-service".into()],
            }],
            packages: vec![
                PackageRequirement {
                    name: "fastapi".into(),
                    version: "0.115".into(),
                    license: "MIT".into(),
                    source: "nixpkgs".into(),
                    attr: "python312Packages.fastapi".into(),
                    sha256: String::new(),
                    unfree: false,
                    attribution: "FastAPI is MIT licensed.".into(),
                },
                PackageRequirement {
                    name: "pytest".into(),
                    version: "8".into(),
                    license: "MIT".into(),
                    source: "nixpkgs".into(),
                    attr: "python312Packages.pytest".into(),
                    sha256: String::new(),
                    unfree: false,
                    attribution: "pytest is MIT licensed.".into(),
                },
            ],
            commands: std::collections::BTreeMap::from([(
                "test".into(),
                CommandRequirement {
                    argv: vec!["python3".into(), "-m".into(), "pytest".into()],
                    cwd: ".".into(),
                    timeout_secs: 300,
                    expect_exit: 0,
                },
            )]),
            nixpkgs: Some("github:NixOS/nixpkgs/2222222222222222222222222222222222222222".into()),
            ..EnvironmentProposal::default()
        }
    }

    #[test]
    fn greenfield_selection_requires_a_concrete_environment_proposal() {
        let options = selection_options("Build a calculator application. You choose the stack.");
        let missing = StackSelectionModel {
            option_id: "python-service".into(),
            language: "python".into(),
            framework: "fastapi".into(),
            rationale: "A small calculator is quickly testable as a Python service.".into(),
            environment_proposal: EnvironmentProposal::default(),
        };
        assert!(matches!(
            resolve_model_stack_selection(&options, &missing, &[Capability::GenericToolchain]),
            Err(StackSelectionError::EnvironmentRequired(_))
        ));

        let mut no_commands = python_proposal();
        no_commands.commands.clear();
        let selection = StackSelectionModel {
            option_id: "python-service".into(),
            language: "python".into(),
            framework: "fastapi".into(),
            rationale: "A small calculator is quickly testable as a Python service.".into(),
            environment_proposal: no_commands,
        };
        assert!(matches!(
            resolve_model_stack_selection(&options, &selection, &[Capability::GenericToolchain]),
            Err(StackSelectionError::EnvironmentRequired(_))
        ));
    }

    fn rust_proposal() -> EnvironmentProposal {
        use crate::planning::environment::{
            CommandRequirement, PackageRequirement, ToolchainRequirement,
        };
        EnvironmentProposal {
            toolchains: vec![ToolchainRequirement {
                id: "rust".into(),
                version: "1.90.0".into(),
                source: "stack-selection".into(),
                evidence: vec!["selected rust-service".into()],
            }],
            packages: vec![PackageRequirement {
                name: "cargo".into(),
                version: "1.90.0".into(),
                license: "MIT".into(),
                source: "nixpkgs".into(),
                attr: "cargo".into(),
                sha256: String::new(),
                unfree: false,
                attribution: "Cargo is MIT licensed.".into(),
            }],
            commands: std::collections::BTreeMap::from([(
                "test".into(),
                CommandRequirement {
                    argv: vec!["cargo".into(), "test".into()],
                    cwd: ".".into(),
                    timeout_secs: 300,
                    expect_exit: 0,
                },
            )]),
            nixpkgs: Some("github:NixOS/nixpkgs/2222222222222222222222222222222222222222".into()),
            ..EnvironmentProposal::default()
        }
    }

    fn typescript_proposal() -> EnvironmentProposal {
        use crate::planning::environment::{
            CommandRequirement, PackageRequirement, ToolchainRequirement,
        };
        EnvironmentProposal {
            toolchains: vec![ToolchainRequirement {
                id: "node".into(),
                version: "22.14.0".into(),
                source: "stack-selection".into(),
                evidence: vec!["selected typescript-react".into()],
            }],
            packages: vec![PackageRequirement {
                name: "react".into(),
                version: "19".into(),
                license: "MIT".into(),
                source: "nixpkgs".into(),
                attr: "nodePackages.react".into(),
                sha256: String::new(),
                unfree: false,
                attribution: "React is MIT licensed.".into(),
            }],
            commands: std::collections::BTreeMap::from([(
                "test".into(),
                CommandRequirement {
                    argv: vec!["npm".into(), "test".into()],
                    cwd: ".".into(),
                    timeout_secs: 300,
                    expect_exit: 0,
                },
            )]),
            nixpkgs: Some("github:NixOS/nixpkgs/2222222222222222222222222222222222222222".into()),
            ..EnvironmentProposal::default()
        }
    }

    #[test]
    fn greenfield_proposal_must_cover_the_selected_stack() {
        let options = selection_options("Build a calculator application. You choose the stack.");
        let python = StackSelectionModel {
            option_id: "python-service".into(),
            language: "python".into(),
            framework: "fastapi".into(),
            rationale: "A small calculator is quickly testable as a Python service.".into(),
            environment_proposal: python_proposal(),
        };
        assert_eq!(
            resolve_model_stack_selection(&options, &python, &[Capability::GenericToolchain])
                .unwrap()
                .id,
            "python-service"
        );

        let rust = StackSelectionModel {
            option_id: "rust-service".into(),
            language: "rust".into(),
            framework: "axum".into(),
            rationale: "A typed local service fits the bounded objective.".into(),
            environment_proposal: rust_proposal(),
        };
        assert_eq!(
            resolve_model_stack_selection(&options, &rust, &[Capability::GenericToolchain])
                .unwrap()
                .id,
            "rust-service"
        );

        let mismatch = StackSelectionModel {
            option_id: "python-service".into(),
            language: "python".into(),
            framework: "fastapi".into(),
            rationale: "A small calculator is quickly testable as a Python service.".into(),
            environment_proposal: rust_proposal(),
        };
        assert!(matches!(
            resolve_model_stack_selection(&options, &mismatch, &[Capability::GenericToolchain]),
            Err(StackSelectionError::EnvironmentMismatch(_))
        ));

        let web_options = selection_options("Build a web dashboard. You choose the stack.");
        let typescript = StackSelectionModel {
            option_id: "typescript-react".into(),
            language: "typescript".into(),
            framework: "react".into(),
            rationale: "A browser dashboard fits the requested web interface.".into(),
            environment_proposal: typescript_proposal(),
        };
        assert_eq!(
            resolve_model_stack_selection(&web_options, &typescript, &[Capability::NodeToolchain])
                .unwrap()
                .id,
            "typescript-react"
        );
    }

    #[test]
    fn model_selection_resolves_an_exact_option_and_fails_closed() {
        let options = selection_options("Build a calculator application. You choose the stack.");
        let selection = StackSelectionModel {
            option_id: "python-service".into(),
            language: "python".into(),
            framework: "fastapi".into(),
            rationale: "A small calculator is quickly testable as a Python service.".into(),
            environment_proposal: python_proposal(),
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
            environment_proposal: python_proposal(),
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
            environment_proposal: python_proposal(),
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
            environment_proposal: python_proposal(),
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
    fn rationale_bound_is_exactly_500_characters() {
        let options = selection_options("Build a calculator application. You choose the stack.");
        let mut selection = StackSelectionModel {
            option_id: "python-service".into(),
            language: "python".into(),
            framework: "fastapi".into(),
            rationale: "a".repeat(MAX_SELECTION_RATIONALE_CHARS),
            environment_proposal: python_proposal(),
        };
        resolve_model_stack_selection(&options, &selection, &[Capability::GenericToolchain])
            .expect("a 500-character rationale is valid");

        selection.rationale.push('a');
        assert!(matches!(
            resolve_model_stack_selection(&options, &selection, &[Capability::GenericToolchain]),
            Err(StackSelectionError::Invalid(_))
        ));
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
