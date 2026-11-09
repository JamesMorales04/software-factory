//! Deterministic Definition of Done construction.
//!
//! The DoD is a Core-owned contract. It binds the project checks, the
//! documentation baseline and the reviewer plan. A model may propose extra
//! checks, but it cannot remove these checks or weaken `all_findings_block`.

use crate::agentic::StackProfile;
use crate::lead::{CheckKind, SpecCheck};
use crate::planning::{DefinitionOfDone, DocumentationPlan, PlanningLane, ReviewerPlan};

fn check(
    id: &str,
    kind: CheckKind,
    description: &str,
    command: &[&str],
    timeout_secs: u32,
) -> SpecCheck {
    SpecCheck {
        id: id.into(),
        kind,
        description: description.into(),
        command: command.iter().map(|value| (*value).to_string()).collect(),
        expect_exit: 0,
        timeout_secs,
    }
}

fn stack_checks(stack: &StackProfile) -> Vec<SpecCheck> {
    let mut checks = Vec::new();
    for language in &stack.languages {
        match language.as_str() {
            "rust" => {
                checks.push(check(
                    "rust-test",
                    CheckKind::Test,
                    "Run the Rust test suite against the locked dependency graph.",
                    &["cargo", "test", "--locked", "--offline"],
                    900,
                ));
                checks.push(check(
                    "rust-clippy",
                    CheckKind::Lint,
                    "Reject Rust warnings.",
                    &["cargo", "clippy", "--all-targets", "--", "-D", "warnings"],
                    900,
                ));
            }
            "typescript" => {
                checks.push(check(
                    "typescript-test",
                    CheckKind::Test,
                    "Run the TypeScript test suite.",
                    &["npm", "test", "--", "--run"],
                    900,
                ));
                checks.push(check(
                    "typescript-lint",
                    CheckKind::Lint,
                    "Run the TypeScript lint check.",
                    &["npm", "run", "lint"],
                    600,
                ));
            }
            "python" => {
                checks.push(check(
                    "python-test",
                    CheckKind::Test,
                    "Run the Python test suite.",
                    &["python3", "-m", "pytest"],
                    900,
                ));
                checks.push(check(
                    "python-lint",
                    CheckKind::Lint,
                    "Run the Python lint check.",
                    &["python3", "-m", "ruff", "check", "."],
                    600,
                ));
            }
            "go" => {
                checks.push(check(
                    "go-test",
                    CheckKind::Test,
                    "Run the Go test suite.",
                    &["go", "test", "./..."],
                    900,
                ));
                checks.push(check(
                    "go-vet",
                    CheckKind::Lint,
                    "Run the Go vet check.",
                    &["go", "vet", "./..."],
                    600,
                ));
            }
            _ => {}
        }
    }
    if checks.is_empty() {
        checks.push(check(
            "project-test",
            CheckKind::Test,
            "Run the project test command declared by the repository.",
            &["test", "-s", "README.md"],
            60,
        ));
    }
    checks
}

/// Build the minimum DoD for a lane. `Direct` still gets a real test and lint
/// check; `FormalSpec` additionally requires the documentation and reviewer
/// contracts.
pub fn build_definition_of_done(
    lane: PlanningLane,
    stack: &StackProfile,
    documentation: DocumentationPlan,
    reviewer_plan: ReviewerPlan,
) -> DefinitionOfDone {
    let mut project_checks = stack_checks(stack);
    for (index, item) in documentation.required.iter().enumerate() {
        let slug: String = item
            .path
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '-'
                }
            })
            .collect();
        let command = item
            .check_command
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        project_checks.push(check(
            &format!("doc-{}-{}", index + 1, slug.trim_matches('-')),
            CheckKind::Functional,
            &format!(
                "Required documentation exists and has a heading: {}",
                item.path
            ),
            &command,
            60,
        ));
    }
    if matches!(lane, PlanningLane::Delegated | PlanningLane::FormalSpec) {
        project_checks.push(check(
            "agents-contract",
            CheckKind::Functional,
            "Verify the repository agent contract exists.",
            &["test", "-s", "AGENTS.md"],
            60,
        ));
    }
    DefinitionOfDone {
        project_checks,
        unit_checks: Vec::new(),
        documentation,
        reviewer_plan,
        all_findings_block: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agentic::StackProfile;
    use crate::planning::{DocumentationPlan, PlanningLane, ReviewerPlan};

    #[test]
    fn rust_lane_has_test_and_lint_checks_and_blocks_findings() {
        let stack = StackProfile {
            languages: vec!["rust".into()],
            ..StackProfile::default()
        };
        let dod = build_definition_of_done(
            PlanningLane::FormalSpec,
            &stack,
            DocumentationPlan::default(),
            ReviewerPlan::for_lane(PlanningLane::FormalSpec, true),
        );
        dod.validate().unwrap();
        assert!(
            dod.project_checks
                .iter()
                .any(|check| check.id == "rust-test")
        );
        assert!(
            dod.project_checks
                .iter()
                .any(|check| check.id == "rust-clippy")
        );
        assert!(dod.all_findings_block);
    }
}
