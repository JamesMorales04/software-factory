//! Deterministic architecture plan projection from a validated WorkGraph.
//!
//! The Planner proposes the work graph; this module renders the architecture
//! view Core freezes for review. It never invents components outside the graph
//! and never grants scope.

use crate::agentic::{StackProfile, ValidatedWorkGraph};
use crate::planning::ArchitecturePlan;

const MAX_ITEMS: usize = 32;

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
        context: objective.to_string(),
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
