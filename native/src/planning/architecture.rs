//! Deterministic architecture plan projection from a validated WorkGraph.
//!
//! The Planner proposes the work graph; this module renders the architecture
//! view Core freezes for review. It never invents components outside the graph
//! and never grants scope.

use crate::agentic::{StackProfile, ValidatedWorkGraph};
use crate::planning::ArchitecturePlan;

const MAX_ITEMS: usize = 32;
const FALLBACK_CONTEXT: &str = "Objective context withheld: see the request objective.";

/// Normalize arbitrary graph text into a single, control-free display line.
/// Returns `None` when the text has no non-whitespace content. The caller
/// decides the fallback; the raw text is never mutated.
fn normalized_text(value: &str, max_chars: usize) -> Option<String> {
    let mut normalized = String::new();
    let mut count = 0usize;
    let mut pending_space = false;
    for character in value.chars() {
        if character.is_control() || character.is_whitespace() {
            pending_space = count > 0;
            continue;
        }
        if pending_space {
            if count >= max_chars {
                break;
            }
            normalized.push(' ');
            count += 1;
            pending_space = false;
        }
        if count >= max_chars {
            break;
        }
        normalized.push(character);
        count += 1;
    }
    let trimmed = normalized.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// Normalize the request objective into a single, bounded, control-free display
/// line for the architecture artifact. The raw objective is not mutated.
fn normalized_context(objective: &str) -> String {
    normalized_text(objective, super::MAX_TEXT_CHARS).unwrap_or_else(|| FALLBACK_CONTEXT.into())
}

fn short_sha256(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(value.as_bytes());
    let mut encoded = String::with_capacity(16);
    for byte in &digest[..8] {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

/// Render one bounded structural list item. Short text keeps its readable
/// value. Text that would exceed `MAX_SHORT_CHARS` is never truncated into a
/// misleading fragment: the item carries a deterministic reference and digest,
/// while the full value remains in the WorkGraph/stack decision.
fn bounded_structural(prefix: &str, value: &str, reference: &str, fallback: &str) -> String {
    match normalized_text(value, super::MAX_TEXT_CHARS) {
        Some(normalized) => {
            let candidate = format!("{prefix}{normalized}");
            if candidate.chars().count() <= super::MAX_SHORT_CHARS {
                candidate
            } else {
                format!("{prefix}{reference} (sha256:{})", short_sha256(value))
            }
        }
        None => format!("{prefix}{fallback}"),
    }
}

/// Render the architecture artifact for one validated plan.
pub fn build_architecture_plan(
    objective: &str,
    graph: &ValidatedWorkGraph,
    stack: &StackProfile,
) -> ArchitecturePlan {
    // A component is a structural label. The WorkGraph unit title is the
    // bounded summary; the full unit objective stays in the WorkGraph and the
    // architecture context. A model that copies the whole request objective
    // into a unit objective therefore cannot break the architecture artifact.
    let components = graph
        .units
        .iter()
        .take(MAX_ITEMS)
        .map(|unit| {
            bounded_structural(
                &format!("{}: ", unit.id),
                &unit.title,
                &format!("{} title", unit.id),
                "work unit",
            )
        })
        .collect::<Vec<_>>();
    let boundaries = graph
        .units
        .iter()
        .flat_map(|unit| {
            unit.paths.iter().enumerate().map(move |(index, path)| {
                bounded_structural(
                    "Write boundary: ",
                    path,
                    &format!("{} path {}", unit.id, index + 1),
                    "path recorded in the WorkGraph",
                )
            })
        })
        .take(MAX_ITEMS)
        .collect::<Vec<_>>();
    let interfaces = graph
        .units
        .iter()
        .flat_map(|unit| {
            unit.acceptance
                .iter()
                .enumerate()
                .map(move |(index, statement)| {
                    bounded_structural(
                        &format!("{} acceptance {}: ", unit.id, index + 1),
                        statement,
                        &format!("{} acceptance {}", unit.id, index + 1),
                        "acceptance recorded in the WorkGraph",
                    )
                })
        })
        .take(MAX_ITEMS)
        .collect::<Vec<_>>();
    let data = stack
        .data
        .iter()
        .take(MAX_ITEMS)
        .map(|value| {
            bounded_structural(
                "Observed data technology: ",
                value,
                "observed data technology",
                "data technology recorded in the stack decision",
            )
        })
        .collect::<Vec<_>>();
    let deployment = stack
        .infra
        .iter()
        .take(MAX_ITEMS)
        .map(|value| {
            bounded_structural(
                "Observed deployment technology: ",
                value,
                "observed deployment technology",
                "deployment technology recorded in the stack decision",
            )
        })
        .collect::<Vec<_>>();
    let selected_stack = stack
        .languages
        .iter()
        .chain(stack.frameworks.iter())
        .take(MAX_ITEMS)
        .map(|value| {
            bounded_structural(
                "",
                value,
                "stack selection",
                "stack value recorded in the stack decision",
            )
        })
        .collect::<Vec<_>>();
    let quality_attributes = vec![
        "Deterministic checks must pass before a candidate is accepted.".into(),
        "Every finding blocks the Quality Gate until resolved or refuted with evidence.".into(),
        "Core remains the sole authority for scope, budgets and delivery.".into(),
    ];
    let risks = if stack.security_sensitive.is_empty() {
        Vec::new()
    } else {
        vec![bounded_structural(
            "Security-sensitive surfaces observed: ",
            &stack.security_sensitive.join(", "),
            "security-sensitive surfaces",
            "security-sensitive surfaces recorded in the stack decision",
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

    #[test]
    fn a_long_unit_objective_keeps_components_bounded_and_the_context_complete() {
        let mut objective = String::new();
        while objective.chars().count() < 996 {
            objective.push_str("Implement a deterministic bounded endpoint that validates the owner request and returns a typed result. ");
        }
        let objective = objective.chars().take(996).collect::<String>();
        let objective = objective.trim_end().to_owned();
        let graph = ValidatedWorkGraph {
            units: vec![PlannedWorkUnit {
                id: "bounded-endpoint".into(),
                title: "Implement the bounded endpoint.".into(),
                objective: objective.clone(),
                depends_on: Vec::new(),
                paths: vec!["src".into()],
                acceptance: vec!["A valid request returns a bounded response.".into()],
            }],
            assumptions: Vec::new(),
            risks: Vec::new(),
            topological_order: vec!["bounded-endpoint".into()],
            graph_sha256: "a".repeat(64),
        };
        let plan = build_architecture_plan(&objective, &graph, &StackProfile::default());
        plan.validate().unwrap();
        assert_eq!(
            plan.components,
            vec!["bounded-endpoint: Implement the bounded endpoint.".to_string()]
        );
        assert_eq!(plan.context, objective);
        assert!(
            plan.components
                .iter()
                .all(|component| component.chars().count() <= crate::planning::MAX_SHORT_CHARS)
        );
    }

    #[test]
    fn unicode_and_control_characters_in_unit_text_stay_control_free() {
        let graph = ValidatedWorkGraph {
            units: vec![PlannedWorkUnit {
                id: "unicode-unit".into(),
                title: "Café 🚀\u{0}\nTitle".into(),
                objective: "Build it.".into(),
                depends_on: Vec::new(),
                paths: vec!["src".into()],
                acceptance: vec!["Returns a bounded response.\u{0}\u{7}".into()],
            }],
            assumptions: Vec::new(),
            risks: Vec::new(),
            topological_order: vec!["unicode-unit".into()],
            graph_sha256: "a".repeat(64),
        };
        let plan = build_architecture_plan("Build it.", &graph, &StackProfile::default());
        plan.validate().unwrap();
        assert_eq!(
            plan.components,
            vec!["unicode-unit: Café 🚀 Title".to_string()]
        );
        assert!(
            plan.interfaces
                .iter()
                .all(|value| !value.chars().any(char::is_control))
        );
        assert!(
            plan.components
                .iter()
                .all(|value| !value.chars().any(char::is_control))
        );
    }

    #[test]
    fn long_acceptance_and_path_use_a_deterministic_reference_instead_of_a_truncation() {
        let acceptance = "A".repeat(800);
        let path = format!("src/{}", "a".repeat(800));
        let graph = ValidatedWorkGraph {
            units: vec![PlannedWorkUnit {
                id: "long-text".into(),
                title: "Long text".into(),
                objective: "Implement it.".into(),
                depends_on: Vec::new(),
                paths: vec![path],
                acceptance: vec![acceptance.clone()],
            }],
            assumptions: Vec::new(),
            risks: Vec::new(),
            topological_order: vec!["long-text".into()],
            graph_sha256: "a".repeat(64),
        };
        let plan = build_architecture_plan("Implement it.", &graph, &StackProfile::default());
        plan.validate().unwrap();
        assert!(plan.interfaces[0].chars().count() <= crate::planning::MAX_SHORT_CHARS);
        assert!(plan.interfaces[0].contains("sha256:"));
        assert!(plan.interfaces[0].contains("long-text acceptance 1"));
        assert!(plan.boundaries[0].chars().count() <= crate::planning::MAX_SHORT_CHARS);
        assert!(plan.boundaries[0].contains("sha256:"));
        assert!(plan.boundaries[0].contains("long-text path 1"));
        // The full value is preserved in the graph, not silently invented.
        assert_eq!(graph.units[0].acceptance[0], acceptance);
    }
}
