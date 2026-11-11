//! Deterministic architecture plan projection from a validated WorkGraph.
//!
//! The Planner proposes the work graph; this module renders the architecture
//! view Core freezes for review. It never invents components outside the graph
//! and never grants scope.

use crate::agentic::{StackProfile, ValidatedWorkGraph};
use crate::planning::ArchitecturePlan;

const MAX_ITEMS: usize = 32;
const FALLBACK_CONTEXT: &str = "Objective context withheld: see the request objective.";

/// Normalize the request objective into a single, bounded, control-free display
/// line for the architecture artifact. The raw objective is not mutated.
fn normalized_context(objective: &str) -> String {
    let mut normalized = String::new();
    let mut count = 0usize;
    let mut pending_space = false;
    for character in objective.chars() {
        if character.is_control() || character.is_whitespace() {
            pending_space = count > 0;
            continue;
        }
        if pending_space {
            if count >= super::MAX_TEXT_CHARS {
                break;
            }
            normalized.push(' ');
            count += 1;
            pending_space = false;
        }
        if count >= super::MAX_TEXT_CHARS {
            break;
        }
        normalized.push(character);
        count += 1;
    }
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        FALLBACK_CONTEXT.into()
    } else {
        trimmed.to_owned()
    }
}

/// Render the architecture artifact for one validated plan.
pub fn build_architecture_plan(
    objective: &str,
    graph: &ValidatedWorkGraph,
    stack: &StackProfile,
) -> ArchitecturePlan {
    let components = graph
        .units
        .iter()
        .take(MAX_ITEMS)
        .map(|unit| format!("{}: {}", unit.id, unit.objective))
        .collect::<Vec<_>>();
    let boundaries = graph
        .units
        .iter()
        .flat_map(|unit| unit.paths.iter())
        .take(MAX_ITEMS)
        .map(|path| format!("Write boundary: {path}"))
        .collect::<Vec<_>>();
    let interfaces = graph
        .units
        .iter()
        .flat_map(|unit| unit.acceptance.iter())
        .take(MAX_ITEMS)
        .cloned()
        .collect::<Vec<_>>();
    let data = stack
        .data
        .iter()
        .take(MAX_ITEMS)
        .map(|value| format!("Observed data technology: {value}"))
        .collect::<Vec<_>>();
    let deployment = stack
        .infra
        .iter()
        .take(MAX_ITEMS)
        .map(|value| format!("Observed deployment technology: {value}"))
        .collect::<Vec<_>>();
    let selected_stack = stack
        .languages
        .iter()
        .chain(stack.frameworks.iter())
        .take(MAX_ITEMS)
        .cloned()
        .collect::<Vec<_>>();
    let quality_attributes = vec![
        "Deterministic checks must pass before a candidate is accepted.".into(),
        "Every finding blocks the Quality Gate until resolved or refuted with evidence.".into(),
        "Core remains the sole authority for scope, budgets and delivery.".into(),
    ];
    let risks = if stack.security_sensitive.is_empty() {
        Vec::new()
    } else {
        vec![format!(
            "Security-sensitive surfaces observed: {}.",
            stack.security_sensitive.join(", ")
        )]
    };
    ArchitecturePlan {
        context: normalized_context(objective),
        stack: selected_stack,
        components,
        boundaries,
        interfaces,
        data,
        deployment,
        quality_attributes,
        risks,
        tradeoffs: vec![
            "Prefer the smallest coherent change that satisfies the frozen plan.".into(),
        ],
        adr_paths: vec!["docs/architecture/decisions/ADR-0001-existing-stack.md".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agentic::PlannedWorkUnit;

    fn graph() -> ValidatedWorkGraph {
        ValidatedWorkGraph {
            units: vec![PlannedWorkUnit {
                id: "calculator".into(),
                title: "Calculator".into(),
                objective: "Implement the calculator.".into(),
                depends_on: Vec::new(),
                paths: vec!["src".into()],
                acceptance: vec!["The calculator adds two numbers.".into()],
            }],
            assumptions: Vec::new(),
            risks: Vec::new(),
            topological_order: vec!["calculator".into()],
            graph_sha256: "a".repeat(64),
        }
    }

    #[test]
    fn multi_line_issue_body_becomes_a_control_free_architecture_context() {
        let objective =
            "Build a Pokemon calculator.\r\n\r\n## Requirements\n- add\tand subtract\n- no tabs\n";
        let plan = build_architecture_plan(objective, &graph(), &StackProfile::default());
        plan.validate().unwrap();
        assert_eq!(
            plan.context,
            "Build a Pokemon calculator. ## Requirements - add and subtract - no tabs"
        );
        assert!(!plan.context.chars().any(char::is_control));
    }

    #[test]
    fn architecture_records_the_selected_stack_projection() {
        let stack = StackProfile {
            languages: vec!["python".into()],
            frameworks: vec!["fastapi".into()],
            ..StackProfile::default()
        };
        let plan = build_architecture_plan("Build a calculator.", &graph(), &stack);
        plan.validate().unwrap();
        assert_eq!(
            plan.stack,
            vec!["python".to_string(), "fastapi".to_string()]
        );
    }

    #[test]
    fn architecture_context_is_bounded_and_has_a_nonempty_fallback() {
        let long = "calculator ".repeat(1_000);
        let plan = build_architecture_plan(&long, &graph(), &StackProfile::default());
        plan.validate().unwrap();
        assert!(plan.context.chars().count() <= crate::planning::MAX_TEXT_CHARS);
        assert!(!plan.context.ends_with(' '));

        let empty = build_architecture_plan("\n\r\t\u{0}", &graph(), &StackProfile::default());
        empty.validate().unwrap();
        assert_eq!(empty.context, FALLBACK_CONTEXT);
    }
}
