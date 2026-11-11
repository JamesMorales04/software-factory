//! Golden planning-pipeline integration test.
//!
//! This test composes the deterministic contracts without a provider: route,
//! stack, WorkGraph validation, architecture/docs/skills/DoD, OpenSpec rendering
//! and finding routing. It proves the modules agree on identities and bounds.

use factory_native::agentic::{
    SpecAssertionKind, SpecDraftModel, SpecRequirementDraft, StackProfile, WorkGraphModel,
    compile_spec, validate_work_graph,
};
use factory_native::openspec::{OpenSpecChange, OpenSpecRequirement, OpenSpecTask, render};
use factory_native::planning::findings::{open_disposition, route_for_category};
use factory_native::planning::route::{
    PlanningArtifactKind, PlanningRouteInput, UserPlanningConstraints, required_artifacts,
    required_reviewers, route_for,
};
use factory_native::planning::{
    FindingStatus, PlanningLane, PlanningSignals, ReviewerRole, SCHEMA_VERSION,
};
use factory_native::review::{FindingCategory, FindingSeverity, ReviewFinding};

#[test]
fn formal_planning_pipeline_is_deterministic_and_traceable() {
    let signals = PlanningSignals {
        objective: "Add a governed calendar export service with a browser review screen".into(),
        greenfield: true,
        large: true,
        missing_docs: true,
        missing_stack: true,
        architecture_uncertainty: true,
        public_contract: true,
        data_or_schema: false,
        security_boundary: false,
        cross_module: true,
        changed_files: 0,
        expected_units: 3,
        explicit_repair: false,
    };
    let lane = route_for(&PlanningRouteInput {
        signals: signals.clone(),
        user_constraints: UserPlanningConstraints {
            language: Some("rust".into()),
            framework: Some("axum".into()),
            ..UserPlanningConstraints::default()
        },
    })
    .unwrap();
    assert_eq!(lane, PlanningLane::FormalSpec);
    let artifacts = required_artifacts(lane);
    for kind in [
        PlanningArtifactKind::Spec,
        PlanningArtifactKind::Stack,
        PlanningArtifactKind::Architecture,
        PlanningArtifactKind::Documentation,
        PlanningArtifactKind::Skills,
        PlanningArtifactKind::WorkGraph,
        PlanningArtifactKind::DoD,
        PlanningArtifactKind::OpenSpec,
    ] {
        assert!(artifacts.contains(&kind));
    }
    let reviewers = required_reviewers(lane, factory_native::agentic::Assurance::Elevated);
    reviewers.validate().unwrap();
    assert!(reviewers.required.contains(&ReviewerRole::Security));
    assert!(reviewers.roles.contains(&ReviewerRole::Operability));

    let graph_model = WorkGraphModel {
        units: vec![
            factory_native::agentic::PlannedWorkUnit {
                id: "export".into(),
                title: "Implement the export".into(),
                objective: "Add the calendar export endpoint.".into(),
                depends_on: Vec::new(),
                paths: vec!["src/export.rs".into()],
                acceptance: vec!["The endpoint returns a calendar document.".into()],
            },
            factory_native::agentic::PlannedWorkUnit {
                id: "review-ui".into(),
                title: "Add the review screen".into(),
                objective: "Expose the export in Office.".into(),
                depends_on: vec!["export".into()],
                paths: vec!["office/src/views/Export.tsx".into()],
                acceptance: vec!["The screen shows the export state.".into()],
            },
        ],
        assumptions: vec!["The project is greenfield.".into()],
        risks: Vec::new(),
    };
    let graph = validate_work_graph(
        &graph_model,
        &["src/export.rs".into(), "office/src/views/Export.tsx".into()],
    )
    .unwrap();
    assert_eq!(graph.topological_order, vec!["export", "review-ui"]);

    let stack = StackProfile {
        languages: vec!["rust".into(), "typescript".into()],
        frameworks: vec!["axum".into(), "react".into()],
        package_managers: vec!["cargo".into(), "npm".into()],
        build_tools: vec!["cargo".into(), "vite".into()],
        ..StackProfile::default()
    };
    let root = std::env::temp_dir().join(format!(
        "factory-planning-pipeline-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("README.md"), "# Project\n").unwrap();
    std::fs::write(root.join("AGENTS.md"), "# Rules\n").unwrap();
    let docs =
        factory_native::planning::docs::build_documentation_plan(&root, PlanningLane::FormalSpec);
    docs.validate().unwrap();
    let skill_plan =
        factory_native::planning::skills::build_skill_plan(&stack, &[], "2026-11-09T00:00:00Z")
            .unwrap();
    skill_plan.validate().unwrap();
    let architecture = factory_native::planning::architecture::build_architecture_plan(
        &signals.objective,
        &graph,
        &stack,
    );
    architecture.validate().unwrap();
    let dod = factory_native::planning::dod::build_definition_of_done(
        lane,
        &stack,
        docs,
        reviewers.clone(),
    );
    dod.validate().unwrap();
    assert!(dod.all_findings_block);

    let draft = SpecDraftModel {
        summary: "Expose a governed calendar export.".into(),
        requirements: vec![SpecRequirementDraft {
            statement: "The system SHALL return a calendar export.".into(),
            work_unit_id: "export".into(),
            acceptance: vec!["A valid request returns a calendar document.".into()],
            negative: vec!["An invalid request returns a typed error.".into()],
        }],
        assumptions: Vec::new(),
        risks: Vec::new(),
    };
    let spec = compile_spec(&draft, 1).unwrap();
    assert_eq!(spec.requirements[0].id, "REQ-001");
    assert_eq!(
        spec.requirements[0].assertions[0].kind,
        SpecAssertionKind::Acceptance
    );
    let change = OpenSpecChange {
        change_id: "add-calendar-export".into(),
        title: "Add calendar export".into(),
        why: "The product needs a governed export.".into(),
        what_changes: vec!["Add the export endpoint.".into()],
        non_goals: Vec::new(),
        requirements: vec![OpenSpecRequirement {
            id: "REQ-001".into(),
            statement: "The system SHALL return a calendar export.".into(),
            acceptance: vec!["REQ-001 a valid request returns a calendar document.".into()],
            negative: vec!["REQ-001 an invalid request returns a typed error.".into()],
        }],
        design: "## Stack\n\nRust and axum.\n".into(),
        tasks: vec![OpenSpecTask {
            id: "1.1".into(),
            title: "Implement export".into(),
            paths: vec!["src/export.rs".into()],
            acceptance: vec!["REQ-001 the endpoint returns a calendar.".into()],
            evidence: vec!["cargo test".into()],
        }],
        stack_summary: "Rust, axum, React.".into(),
        architecture_summary: "One bounded export endpoint.".into(),
    };
    let openspec = render(&change).unwrap();
    openspec.validate().unwrap();
    assert!(openspec.specs[0].content.contains("## ADDED Requirements"));
    assert!(openspec.tasks.contains("- [ ] 1.1"));

    let finding = ReviewFinding {
        id: "F-001".into(),
        severity: FindingSeverity::Warning,
        category: FindingCategory::Functional,
        message: "The export omits the closing boundary.".into(),
        points: FindingSeverity::Warning.canonical_points(),
        rationale: "The diff shows the missing boundary.".into(),
        evidence_refs: vec!["src/export.rs:42".into()],
    };
    let disposition = open_disposition(&finding, "request-1", "candidate-1", "reviewer-1");
    assert_eq!(route_for_category(finding.category), disposition.route);
    assert_eq!(disposition.status, FindingStatus::Open);
    assert_eq!(disposition.request_id, "request-1");
    assert_eq!(SCHEMA_VERSION, 1);
    let _ = std::fs::remove_dir_all(root);
}
