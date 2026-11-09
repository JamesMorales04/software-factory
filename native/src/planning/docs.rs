//! Deterministic documentation baseline planning.
//!
//! The plan records which baseline documents already exist and which the
//! delivery must create. Every required document carries a deterministic check
//! command so "documented" is an observable fact, not a prose claim.

use std::path::Path;

use crate::planning::{DocumentationItem, DocumentationPlan, PlanningLane};

const BASELINE: &[(&str, &str)] = &[
    ("README.md", "Project purpose, setup and run instructions."),
    (
        "AGENTS.md",
        "Repository-specific engineering rules for coding agents.",
    ),
    (
        "docs/architecture/README.md",
        "C4/arc42 architecture overview and component boundaries.",
    ),
    (
        "docs/architecture/decisions/README.md",
        "Architecture decision record index and status.",
    ),
    (
        "docs/quality/definition-of-done.md",
        "Definition of Done, test strategy and evidence requirements.",
    ),
];

fn check_command(path: &str) -> Vec<String> {
    // A baseline document must exist, be non-empty and carry a Markdown
    // heading. This is a structural check, not a claim that prose is correct.
    vec!["grep".into(), "-q".into(), "^#".into(), path.into()]
}

/// Build the documentation plan for the lane. `Direct` still requires the
/// README and AGENTS baseline; deeper lanes add architecture and quality docs.
pub fn build_documentation_plan(repository: &Path, lane: PlanningLane) -> DocumentationPlan {
    let mut existing = Vec::new();
    let mut required = Vec::new();
    let baseline = match lane {
        PlanningLane::Direct | PlanningLane::Repair => &BASELINE[..2],
        PlanningLane::Delegated | PlanningLane::FormalSpec => BASELINE,
    };
    for (path, purpose) in baseline {
        if repository.join(path).is_file() {
            existing.push((*path).to_string());
        } else {
            required.push(DocumentationItem {
                path: (*path).to_string(),
                purpose: (*purpose).to_string(),
                required: true,
                check_command: check_command(path),
            });
        }
    }
    DocumentationPlan {
        existing,
        required,
        owner: "Planner".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::PlanningLane;

    #[test]
    fn direct_requires_readme_and_agents_but_not_architecture_docs() {
        let root = std::env::temp_dir().join(format!("factory-docs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("README.md"), "# Project\n").unwrap();
        std::fs::write(root.join("AGENTS.md"), "# Rules\n").unwrap();
        let plan = build_documentation_plan(&root, PlanningLane::Direct);
        assert!(plan.required.is_empty());
        assert_eq!(plan.existing.len(), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn delegated_requires_architecture_and_quality_docs() {
        let root = std::env::temp_dir().join(format!("factory-docs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("README.md"), "# Project\n").unwrap();
        std::fs::write(root.join("AGENTS.md"), "# Rules\n").unwrap();
        let plan = build_documentation_plan(&root, PlanningLane::Delegated);
        assert_eq!(plan.required.len(), 3);
        assert!(
            plan.required
                .iter()
                .all(|item| !item.check_command.is_empty())
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
