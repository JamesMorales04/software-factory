//! Typed agentic-flow domain boundary.
//!
//! Everything here is advisory or deterministic policy. Core authorization,
//! budgets, leases, candidate identity and the Quality Gate remain separate.
//! A model may propose a triage, stack interpretation, plan or specification;
//! it cannot select tools, widen scope, accept a candidate or grant authority.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The owner-facing classification of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RequestKind {
    Informational,
    Audit,
    Change,
    Repair,
}

/// The implementation route. Request kind and route are separate facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChangeRoute {
    NoAgent,
    Direct,
    Delegated,
    FormalSpec,
    Repair,
}

/// The assurance floor, independent of the implementation route.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Assurance {
    None,
    Standard,
    Elevated,
    Critical,
}

/// Observable signals from the request, project and bounded semantic assessor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TriageSignals {
    pub objective: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub risk: String,
    #[serde(default)]
    pub expected_work_units: Option<u32>,
    #[serde(default)]
    pub ambiguous: bool,
    #[serde(default)]
    pub architecture_uncertainty: bool,
    #[serde(default)]
    pub cross_module: bool,
    #[serde(default)]
    pub public_contract: bool,
    #[serde(default)]
    pub data_or_schema: bool,
    #[serde(default)]
    pub security_boundary: bool,
    #[serde(default)]
    pub explicit_informational: bool,
    #[serde(default)]
    pub explicit_audit: bool,
    #[serde(default)]
    pub explicit_repair: bool,
}

/// A deterministic triage decision. The semantic assessor may raise signals;
/// this function applies non-weakening floors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriageDecision {
    pub request_kind: RequestKind,
    pub route: ChangeRoute,
    pub assurance: Assurance,
    pub mechanism: String,
    pub reasons: Vec<String>,
    pub unknowns: Vec<String>,
}

pub fn triage(signals: &TriageSignals) -> TriageDecision {
    let mut reasons = Vec::new();
    if signals.explicit_informational {
        return TriageDecision {
            request_kind: RequestKind::Informational,
            route: ChangeRoute::NoAgent,
            assurance: Assurance::None,
            mechanism: "HYBRID".into(),
            reasons: vec![
                "The owner explicitly requested explanation or lookup without mutation.".into(),
            ],
            unknowns: Vec::new(),
        };
    }
    if signals.explicit_audit {
        return TriageDecision {
            request_kind: RequestKind::Audit,
            route: ChangeRoute::Direct,
            assurance: audit_assurance(signals),
            mechanism: "HYBRID".into(),
            reasons: vec!["The request is a read-only engineering audit.".into()],
            unknowns: Vec::new(),
        };
    }
    if signals.explicit_repair {
        return TriageDecision {
            request_kind: RequestKind::Repair,
            route: ChangeRoute::Repair,
            assurance: repair_assurance(signals),
            mechanism: "DETERMINISTIC".into(),
            reasons: vec![
                "The request targets an exact rejected candidate and requires a RepairPacket."
                    .into(),
            ],
            unknowns: vec![
                "The exact candidate and finding packet must be supplied by Core.".into(),
            ],
        };
    }

    let concrete_files: Vec<&str> = signals
        .files
        .iter()
        .map(String::as_str)
        .filter(|path| {
            !path.contains('*') && !path.contains('?') && !path.contains('[') && !path.contains('{')
        })
        .collect();
    if concrete_files.is_empty() && !signals.files.is_empty() {
        reasons.push("The supplied scope contains wildcard or non-concrete paths.".into());
    }
    if signals.files.len() > 5 {
        reasons.push("The scope spans more than five files and needs delegated planning.".into());
    }
    if signals.cross_module {
        reasons.push("The work crosses module boundaries.".into());
    }
    if signals.expected_work_units.is_some_and(|units| units > 1) {
        reasons.push("The expected work has more than one independent unit.".into());
    }
    if signals.ambiguous || signals.architecture_uncertainty {
        reasons
            .push("The semantic assessment reports ambiguity or architectural uncertainty.".into());
    }
    if signals.public_contract {
        reasons.push("The change affects a public contract.".into());
    }
    if signals.data_or_schema {
        reasons.push("The change affects data or schema.".into());
    }
    if signals.security_boundary {
        reasons.push("The change crosses a security boundary.".into());
    }
    if signals.risk.eq_ignore_ascii_case("high") {
        reasons.push("The assessed risk is high.".into());
    }

    let route = if signals.public_contract
        || signals.data_or_schema
        || signals.ambiguous
        || signals.architecture_uncertainty
        || signals.cross_module
        || signals.files.len() > 5
        || signals.expected_work_units.is_some_and(|units| units > 1)
    {
        ChangeRoute::FormalSpec
    } else if signals.files.is_empty()
        || concrete_files.len() != signals.files.len()
        || signals.security_boundary
    {
        ChangeRoute::Delegated
    } else {
        ChangeRoute::Direct
    };

    let assurance =
        if signals.security_boundary || signals.public_contract || signals.data_or_schema {
            Assurance::Critical
        } else if signals.risk.eq_ignore_ascii_case("high") || signals.cross_module {
            Assurance::Elevated
        } else if signals.risk.eq_ignore_ascii_case("medium") {
            Assurance::Standard
        } else {
            Assurance::None
        };

    if reasons.is_empty() {
        reasons.push("The request is bounded and suitable for direct execution.".into());
    }
    TriageDecision {
        request_kind: RequestKind::Change,
        route,
        assurance,
        mechanism: "HYBRID".into(),
        reasons,
        unknowns: Vec::new(),
    }
}

fn route_rank(route: ChangeRoute) -> u8 {
    match route {
        ChangeRoute::NoAgent => 0,
        ChangeRoute::Direct => 1,
        ChangeRoute::Delegated => 2,
        ChangeRoute::FormalSpec => 3,
        ChangeRoute::Repair => 4,
    }
}

/// Merge a semantic recommendation with the deterministic decision. The model
/// may raise assurance or route strength; it can never lower the deterministic
/// floor.
pub fn merge_model_decision(
    decision: TriageDecision,
    model: &AgenticTriageModel,
) -> TriageDecision {
    let mut merged = decision;
    merged.assurance = merged.assurance.max(model.assurance);
    if route_rank(model.route) > route_rank(merged.route) {
        merged.route = model.route;
    }
    merged.mechanism = "HYBRID".into();
    if !model.unknowns.is_empty() {
        merged.unknowns.extend(model.unknowns.clone());
        merged.unknowns.sort();
        merged.unknowns.dedup();
    }
    merged
}

fn audit_assurance(signals: &TriageSignals) -> Assurance {
    if signals.security_boundary || signals.public_contract || signals.data_or_schema {
        Assurance::Elevated
    } else {
        Assurance::Standard
    }
}

fn repair_assurance(signals: &TriageSignals) -> Assurance {
    if signals.security_boundary || signals.public_contract || signals.data_or_schema {
        Assurance::Critical
    } else if signals.risk.eq_ignore_ascii_case("high") {
        Assurance::Elevated
    } else {
        Assurance::Standard
    }
}

/// A typed semantic triage proposal. The model may recommend a route and
/// assurance, but Core applies the deterministic floors in `triage`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgenticTriageModel {
    pub request_kind: RequestKind,
    pub route: ChangeRoute,
    pub assurance: Assurance,
    pub stack: StackProfile,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub risk: String,
    #[serde(default)]
    pub expected_work_units: Option<u32>,
    #[serde(default)]
    pub ambiguous: bool,
    #[serde(default)]
    pub architecture_uncertainty: bool,
    #[serde(default)]
    pub cross_module: bool,
    #[serde(default)]
    pub public_contract: bool,
    #[serde(default)]
    pub data_or_schema: bool,
    #[serde(default)]
    pub security_boundary: bool,
    #[serde(default)]
    pub skill_needs: Vec<String>,
    #[serde(default)]
    pub unknowns: Vec<String>,
}

impl AgenticTriageModel {
    pub fn signals(&self, objective: &str) -> TriageSignals {
        TriageSignals {
            objective: objective.into(),
            files: self.files.clone(),
            domains: self.domains.clone(),
            risk: self.risk.clone(),
            expected_work_units: self.expected_work_units,
            ambiguous: self.ambiguous,
            architecture_uncertainty: self.architecture_uncertainty,
            cross_module: self.cross_module,
            public_contract: self.public_contract,
            data_or_schema: self.data_or_schema,
            security_boundary: self.security_boundary,
            explicit_informational: self.request_kind == RequestKind::Informational,
            explicit_audit: self.request_kind == RequestKind::Audit,
            explicit_repair: self.request_kind == RequestKind::Repair,
        }
    }
}

/// A bounded technology profile. Deterministic evidence is recorded alongside
/// model interpretation; unknowns remain explicit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StackProfile {
    pub languages: Vec<String>,
    pub frameworks: Vec<String>,
    pub runtimes: Vec<String>,
    pub package_managers: Vec<String>,
    pub build_tools: Vec<String>,
    pub data: Vec<String>,
    pub test_frameworks: Vec<String>,
    pub browser: Vec<String>,
    pub infra: Vec<String>,
    pub security_sensitive: Vec<String>,
    pub conventions: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub confidence: String,
    pub unknowns: Vec<String>,
}

impl Default for StackProfile {
    fn default() -> Self {
        Self {
            languages: Vec::new(),
            frameworks: Vec::new(),
            runtimes: Vec::new(),
            package_managers: Vec::new(),
            build_tools: Vec::new(),
            data: Vec::new(),
            test_frameworks: Vec::new(),
            browser: Vec::new(),
            infra: Vec::new(),
            security_sensitive: Vec::new(),
            conventions: Vec::new(),
            evidence_refs: Vec::new(),
            confidence: "low".into(),
            unknowns: Vec::new(),
        }
    }
}

impl StackProfile {
    fn push_unique(target: &mut Vec<String>, value: &str) {
        if !target.iter().any(|item| item == value) {
            target.push(value.to_owned());
        }
    }

    fn finish(mut self) -> Self {
        self.languages.sort();
        self.frameworks.sort();
        self.runtimes.sort();
        self.package_managers.sort();
        self.build_tools.sort();
        self.data.sort();
        self.test_frameworks.sort();
        self.browser.sort();
        self.infra.sort();
        self.security_sensitive.sort();
        self.conventions.sort();
        self.evidence_refs.sort();
        self.unknowns.sort();
        self.confidence = if self.evidence_refs.is_empty() {
            "low".into()
        } else if self.unknowns.is_empty() {
            "high".into()
        } else {
            "medium".into()
        };
        self
    }
}

/// Deterministic manifest and file evidence. The semantic assessor may add
/// interpretation, but this function never invents a technology.
pub fn detect_stack(root: &Path) -> StackProfile {
    let mut stack = StackProfile::default();
    let mut inspect = |relative: &str| -> Option<String> {
        let path = root.join(relative);
        if path.is_file() {
            StackProfile::push_unique(&mut stack.evidence_refs, relative);
            std::fs::read_to_string(&path).ok()
        } else {
            None
        }
    };

    if let Some(cargo) = inspect("Cargo.toml") {
        StackProfile::push_unique(&mut stack.languages, "rust");
        StackProfile::push_unique(&mut stack.package_managers, "cargo");
        StackProfile::push_unique(&mut stack.build_tools, "cargo");
        for (needle, value) in [
            ("axum", "axum"),
            ("actix-web", "actix-web"),
            ("rocket", "rocket"),
            ("sqlx", "sqlx"),
            ("diesel", "diesel"),
            ("tokio", "tokio"),
        ] {
            if cargo.contains(needle) {
                StackProfile::push_unique(&mut stack.frameworks, value);
            }
        }
    }
    if let Some(package) = inspect("package.json") {
        StackProfile::push_unique(&mut stack.languages, "typescript");
        StackProfile::push_unique(&mut stack.runtimes, "node");
        StackProfile::push_unique(&mut stack.package_managers, "npm");
        for (needle, value) in [
            ("react", "react"),
            ("vite", "vite"),
            ("next", "next"),
            ("vitest", "vitest"),
            ("jest", "jest"),
            ("@playwright/test", "playwright"),
        ] {
            if package.contains(needle) {
                match value {
                    "vitest" | "jest" => {
                        StackProfile::push_unique(&mut stack.test_frameworks, value)
                    }
                    "playwright" => {
                        StackProfile::push_unique(&mut stack.test_frameworks, value);
                        StackProfile::push_unique(&mut stack.browser, value);
                    }
                    _ => StackProfile::push_unique(&mut stack.frameworks, value),
                }
            }
        }
    }
    if inspect("pyproject.toml").is_some() || inspect("requirements.txt").is_some() {
        StackProfile::push_unique(&mut stack.languages, "python");
        StackProfile::push_unique(&mut stack.runtimes, "python3");
        StackProfile::push_unique(&mut stack.package_managers, "pip");
    }
    if inspect("go.mod").is_some() {
        StackProfile::push_unique(&mut stack.languages, "go");
        StackProfile::push_unique(&mut stack.package_managers, "go");
        StackProfile::push_unique(&mut stack.build_tools, "go");
    }
    if inspect("compose.yaml").is_some() || inspect("compose.yml").is_some() {
        StackProfile::push_unique(&mut stack.infra, "podman-compose");
        StackProfile::push_unique(&mut stack.build_tools, "podman");
    }
    if inspect("Dockerfile").is_some() || inspect("Containerfile").is_some() {
        StackProfile::push_unique(&mut stack.infra, "container");
    }
    if inspect("playwright.config.ts").is_some() || inspect("playwright.config.js").is_some() {
        StackProfile::push_unique(&mut stack.test_frameworks, "playwright");
        StackProfile::push_unique(&mut stack.browser, "playwright");
    }
    if inspect("migrations").is_some() {
        StackProfile::push_unique(&mut stack.data, "migrations");
    }
    if root.join("migrations").is_dir() {
        StackProfile::push_unique(&mut stack.data, "sqlite");
        StackProfile::push_unique(&mut stack.data, "migrations");
    }
    stack.finish()
}

/// Deterministic participant plan. The model can propose decomposition, but
/// this function decides how many roles are compiled and what they may do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAssignment {
    pub role: String,
    pub specialization: String,
    pub count: u32,
    pub read_only: bool,
    pub tool_pack: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPolicy {
    pub compact_at_percent: u8,
    pub max_consecutive_compactions: u8,
    pub handoff_at_stable_milestone: bool,
    pub require_probe_after_handoff: bool,
}

impl Default for ContextPolicy {
    fn default() -> Self {
        Self {
            compact_at_percent: 70,
            max_consecutive_compactions: 2,
            handoff_at_stable_milestone: true,
            require_probe_after_handoff: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantPlan {
    pub route: ChangeRoute,
    pub assurance: Assurance,
    pub assignments: Vec<AgentAssignment>,
    pub context_policy: ContextPolicy,
    pub reasons: Vec<String>,
    #[serde(default)]
    pub unit_assignments: Vec<AgentUnitAssignment>,
    #[serde(default)]
    pub reviewer_count: u32,
    #[serde(default)]
    pub reviewer_roles: Vec<crate::planning::ReviewerRole>,
    /// Concrete role-to-principal assignments compiled from trusted reviewer
    /// identities. Empty for a route with no reviewer or before Core compiles
    /// them. Model/provider equality is never a reason to reject an assignment.
    #[serde(default)]
    pub reviewer_assignments: Vec<crate::review_roles::ReviewerIdentityAssignment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUnitAssignment {
    pub unit_id: String,
    pub specialization: String,
    pub paths: Vec<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub provider_mode: Option<String>,
}

fn assignment(role: &str, specialization: &str, count: u32, read_only: bool) -> AgentAssignment {
    AgentAssignment {
        role: role.into(),
        specialization: specialization.into(),
        count,
        read_only,
        tool_pack: if read_only {
            vec!["repository-read".into(), "context-read".into()]
        } else {
            vec![
                "repository-read".into(),
                "repository-write".into(),
                "command-execute".into(),
                "context-read".into(),
            ]
        },
    }
}

pub fn participant_plan(
    route: ChangeRoute,
    assurance: Assurance,
    work_units: u32,
    stack: &StackProfile,
) -> ParticipantPlan {
    let specialization = stack
        .languages
        .first()
        .cloned()
        .unwrap_or_else(|| "cross-cutting".into());
    let units = work_units.max(1);
    let lane = lane_for_route(route);
    let reviewer_plan = lane.map(|lane| {
        crate::planning::ReviewerPlan::for_lane(lane, assurance >= Assurance::Elevated)
    });
    let reviewer_roles = reviewer_plan
        .as_ref()
        .map(|plan| plan.required.clone())
        .unwrap_or_default();
    let reviewers = u32::try_from(reviewer_roles.len()).unwrap_or(u32::MAX);
    let mut assignments = Vec::new();
    match route {
        ChangeRoute::NoAgent => {
            assignments.push(assignment("Lead", "cross-cutting", 1, true));
        }
        ChangeRoute::Direct => {
            assignments.push(assignment("Implementer", &specialization, 1, false));
            assignments.push(assignment("Verifier", "deterministic", 1, true));
            assignments.push(assignment("Reviewer", "cross-cutting", reviewers, true));
        }
        ChangeRoute::Delegated => {
            assignments.push(assignment("Operation Manager", "cross-cutting", 1, true));
            assignments.push(assignment("Implementer", &specialization, units, false));
            assignments.push(assignment("Integrator", "integration", 1, false));
            assignments.push(assignment("Verifier", "deterministic", 1, true));
            assignments.push(assignment(
                "Reviewer",
                if assurance >= Assurance::Elevated {
                    "high-assurance"
                } else {
                    "cross-cutting"
                },
                reviewers,
                true,
            ));
            assignments.push(assignment("Cheap Ops", "test-interpretation", 1, true));
        }
        ChangeRoute::FormalSpec => {
            assignments.push(assignment("Operation Manager", "cross-cutting", 1, true));
            assignments.push(assignment("Explorer", "repository-discovery", 1, true));
            assignments.push(assignment("Planner", "decomposition", 1, true));
            assignments.push(assignment("Spec Manager", "formal-specification", 1, true));
            assignments.push(assignment("Implementer", &specialization, units, false));
            assignments.push(assignment("Integrator", "integration", 1, false));
            assignments.push(assignment("Verifier", "deterministic", 1, true));
            assignments.push(assignment(
                "Reviewer",
                if assurance >= Assurance::Elevated {
                    "high-assurance"
                } else {
                    "cross-cutting"
                },
                reviewers,
                true,
            ));
            assignments.push(assignment("Cheap Ops", "test-interpretation", 1, true));
        }
        ChangeRoute::Repair => {
            assignments.push(assignment("Operation Manager", "cross-cutting", 1, true));
            assignments.push(assignment("Repairer", &specialization, 1, false));
            assignments.push(assignment("Verifier", "deterministic", 1, true));
            assignments.push(assignment("Reviewer", "cross-cutting", reviewers, true));
        }
    }
    let mut reasons = vec![format!(
        "The deterministic composer selected {} assignment(s) for route {:?} and assurance {:?}.",
        assignments.len(),
        route,
        assurance
    )];
    if matches!(route, ChangeRoute::FormalSpec) {
        reasons.push(
            "Formal spec requires Explorer, Planner and Spec Manager before implementation.".into(),
        );
    }
    if units > 1 {
        reasons.push(format!(
            "{units} independent work units are compiled; shared paths remain serialized."
        ));
    }
    if reviewers > 1 {
        reasons.push(format!(
            "{reviewers} independent reviewer receipts are required for assurance {assurance:?}."
        ));
    }
    ParticipantPlan {
        route,
        assurance,
        assignments,
        context_policy: ContextPolicy::default(),
        reasons,
        unit_assignments: Vec::new(),
        reviewer_count: reviewers,
        reviewer_roles,
        reviewer_assignments: Vec::new(),
    }
}

/// Planning lane for an executable route. `NoAgent` has no reviewer plan.
pub fn lane_for_route(route: ChangeRoute) -> Option<crate::planning::PlanningLane> {
    match route {
        ChangeRoute::NoAgent => None,
        ChangeRoute::Direct => Some(crate::planning::PlanningLane::Direct),
        ChangeRoute::Delegated => Some(crate::planning::PlanningLane::Delegated),
        ChangeRoute::FormalSpec => Some(crate::planning::PlanningLane::FormalSpec),
        ChangeRoute::Repair => Some(crate::planning::PlanningLane::Repair),
    }
}

/// Compile the deterministic plan for an exact validated WorkGraph. Each
/// independent unit is bound to one implementer specialization; the model
/// route comes from the frozen profile and is identical for the same role.
pub fn participant_plan_for_graph(
    route: ChangeRoute,
    assurance: Assurance,
    graph: &ValidatedWorkGraph,
    stack: &StackProfile,
    provider: &str,
    model: &str,
    provider_mode: &str,
) -> ParticipantPlan {
    let units = u32::try_from(graph.units.len()).unwrap_or(u32::MAX);
    let mut plan = participant_plan(route, assurance, units, stack);
    plan.unit_assignments = graph
        .units
        .iter()
        .map(|unit| AgentUnitAssignment {
            unit_id: unit.id.clone(),
            specialization: unit_specialization(unit, stack),
            paths: unit.paths.clone(),
            provider: Some(provider.to_owned()).filter(|value| !value.is_empty()),
            model: Some(model.to_owned()).filter(|value| !value.is_empty()),
            provider_mode: Some(provider_mode.to_owned()).filter(|value| !value.is_empty()),
        })
        .collect();
    plan.reasons.push(format!(
        "{} independent unit(s) are bound to their own implementer specialization; path conflicts were rejected by WorkGraph validation.",
        plan.unit_assignments.len()
    ));
    plan
}

fn unit_specialization(unit: &PlannedWorkUnit, stack: &StackProfile) -> String {
    for language in &stack.languages {
        let extension = match language.as_str() {
            "rust" => ".rs",
            "typescript" => ".ts",
            "javascript" => ".js",
            "python" => ".py",
            "go" => ".go",
            _ => continue,
        };
        if unit.paths.iter().any(|path| {
            path.ends_with(extension)
                || path.ends_with(&format!("{extension}x"))
                || path.ends_with(".tsx") && language == "typescript"
        }) {
            return language.clone();
        }
    }
    if unit
        .paths
        .iter()
        .all(|path| path.starts_with("docs") || path.ends_with(".md"))
    {
        return "documentation".into();
    }
    stack
        .languages
        .first()
        .cloned()
        .unwrap_or_else(|| "cross-cutting".into())
}

/// A bounded handoff record. The summary is advisory; deterministic identity
/// and evidence references must be copied from Core records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffSummary {
    pub objective: String,
    pub acceptance_criteria: Vec<String>,
    pub constraints: Vec<String>,
    pub repo: String,
    pub branch: String,
    pub commit: String,
    pub candidate_commit: Option<String>,
    pub candidate_tree: Option<String>,
    pub completed: Vec<String>,
    pub in_progress: Vec<String>,
    pub remaining: Vec<String>,
    pub decisions: Vec<String>,
    pub changed_files: Vec<String>,
    pub checks_run: Vec<String>,
    pub failed_attempts: Vec<String>,
    pub risks: Vec<String>,
    pub next_actions: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub facts: Vec<String>,
    pub hypotheses: Vec<String>,
    pub unverified: Vec<String>,
    pub budgets: Vec<String>,
}

impl HandoffSummary {
    pub fn validate(&self) -> Result<(), String> {
        if self.objective.trim().is_empty() {
            return Err("handoff objective is required".into());
        }
        if self.commit.trim().is_empty() {
            return Err("handoff commit is required".into());
        }
        if self.next_actions.is_empty() {
            return Err("handoff must name at least one next action".into());
        }
        if self.evidence_refs.is_empty() {
            return Err("handoff must reference durable evidence".into());
        }
        Ok(())
    }
}

/// Owner charter. Execution, repair, delivery and policy autonomy are separate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerCharter {
    pub id: String,
    pub owner: String,
    pub projects: Vec<String>,
    pub domains: Vec<String>,
    pub risk_appetite: Assurance,
    pub execution_autonomy: bool,
    pub repair_autonomy: bool,
    pub delivery_autonomy: bool,
    pub policy_autonomy: bool,
    pub reserved_matters: BTreeSet<String>,
    pub expires_at: String,
    pub revoked_at: Option<String>,
}

impl OwnerCharter {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() || self.owner.trim().is_empty() {
            return Err("charter id and owner are required".into());
        }
        if self.projects.is_empty() {
            return Err("charter must name at least one project".into());
        }
        if self.policy_autonomy {
            return Err("a charter cannot grant policy autonomy to a model or Director".into());
        }
        if self.reserved_matters.is_empty() {
            return Err("charter must declare reserved matters".into());
        }
        if self.expires_at.trim().is_empty() {
            return Err("charter expiry is required".into());
        }
        Ok(())
    }

    pub fn covers(&self, project: &str, domain: &str, risk: Assurance, now: &str) -> bool {
        if self.revoked_at.is_some() || now >= self.expires_at.as_str() {
            return false;
        }
        self.projects.iter().any(|item| item == project)
            && self.domains.iter().any(|item| item == domain)
            && risk <= self.risk_appetite
    }
}

/// A Planner proposal. Core validates and freezes it; the model cannot widen
/// paths, dependencies or budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PlannedWorkUnit {
    pub id: String,
    pub title: String,
    pub objective: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub acceptance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkGraphModel {
    pub units: Vec<PlannedWorkUnit>,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedWorkGraph {
    pub units: Vec<PlannedWorkUnit>,
    pub assumptions: Vec<String>,
    pub risks: Vec<String>,
    pub topological_order: Vec<String>,
    pub graph_sha256: String,
}

pub const MAX_WORK_UNITS: usize = 24;
pub const MAX_UNIT_PATHS: usize = 32;

/// Deterministic WorkGraph validation. Cycles, unknown predecessors, unsafe
/// paths, out-of-scope paths, duplicate ids, empty units and overlapping
/// independent write scopes fail closed. The returned order is stable.
pub fn validate_work_graph(
    model: &WorkGraphModel,
    allowed_paths: &[String],
) -> Result<ValidatedWorkGraph, String> {
    if model.units.is_empty() {
        return Err("work graph is empty".into());
    }
    if model.units.len() > MAX_WORK_UNITS {
        return Err(format!(
            "work graph exceeds the {MAX_WORK_UNITS}-unit bound"
        ));
    }
    let mut by_id = BTreeMap::new();
    for unit in &model.units {
        if !valid_unit_id(&unit.id) {
            return Err(format!(
                "work unit id {:?} must be a lowercase slug",
                unit.id
            ));
        }
        if unit.title.trim().is_empty() || unit.objective.trim().is_empty() {
            return Err(format!("work unit {} needs a title and objective", unit.id));
        }
        if unit.title.len() > 200 || unit.objective.len() > 4_000 {
            return Err(format!("work unit {} text exceeds the bound", unit.id));
        }
        if unit.depends_on.len() > MAX_WORK_UNITS {
            return Err(format!("work unit {} has too many predecessors", unit.id));
        }
        if unit.paths.is_empty() || unit.paths.len() > MAX_UNIT_PATHS {
            return Err(format!(
                "work unit {} must declare 1-{MAX_UNIT_PATHS} paths",
                unit.id
            ));
        }
        let mut unique_paths = BTreeSet::new();
        for path in &unit.paths {
            if !crate::lead::is_safe_relative_path(path) {
                return Err(format!("work unit {} has unsafe path {:?}", unit.id, path));
            }
            if !allowed_paths.is_empty()
                && !allowed_paths
                    .iter()
                    .any(|granted| crate::lead::path_within(path, granted))
            {
                return Err(format!(
                    "work unit {} path {:?} is outside the project scope",
                    unit.id, path
                ));
            }
            if !unique_paths.insert(path) {
                return Err(format!("work unit {} repeats path {:?}", unit.id, path));
            }
        }
        if unit.acceptance.is_empty() || unit.acceptance.len() > 20 {
            return Err(format!(
                "work unit {} must declare 1-20 acceptance statements",
                unit.id
            ));
        }
        for statement in &unit.acceptance {
            if statement.trim().is_empty() || statement.len() > 2_000 {
                return Err(format!("work unit {} acceptance is invalid", unit.id));
            }
        }
        if by_id.insert(unit.id.as_str(), unit).is_some() {
            return Err(format!("duplicate work unit id {:?}", unit.id));
        }
    }
    for unit in &model.units {
        let mut unique_dependencies = BTreeSet::new();
        for dependency in &unit.depends_on {
            if dependency == &unit.id {
                return Err(format!("work unit {} depends on itself", unit.id));
            }
            if !by_id.contains_key(dependency.as_str()) {
                return Err(format!(
                    "work unit {} references unknown predecessor {dependency:?}",
                    unit.id
                ));
            }
            if !unique_dependencies.insert(dependency) {
                return Err(format!(
                    "work unit {} repeats predecessor {dependency:?}",
                    unit.id
                ));
            }
        }
    }
    let topological_order = topological_order(&model.units, &by_id)?;
    for (index, left) in model.units.iter().enumerate() {
        for right in &model.units[index + 1..] {
            let left_depends = depends_on(&left.id, &right.id, &by_id);
            let right_depends = depends_on(&right.id, &left.id, &by_id);
            if left_depends || right_depends {
                continue;
            }
            if left.paths.iter().any(|left_path| {
                right.paths.iter().any(|right_path| {
                    left_path == right_path
                        || left_path.starts_with(&format!("{right_path}/"))
                        || right_path.starts_with(&format!("{left_path}/"))
                })
            }) {
                return Err(format!(
                    "independent work units {} and {} share a write path",
                    left.id, right.id
                ));
            }
        }
    }
    let canonical = serde_json::to_vec(&serde_json::json!({
        "units": model.units,
        "assumptions": model.assumptions,
        "risks": model.risks,
    }))
    .map_err(|_| "work graph encoding".to_string())?;
    Ok(ValidatedWorkGraph {
        units: model.units.clone(),
        assumptions: model.assumptions.clone(),
        risks: model.risks.clone(),
        topological_order,
        graph_sha256: sha256_hex(&canonical),
    })
}

fn valid_unit_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && id.bytes().next().is_some_and(|byte| byte != b'-')
        && id.bytes().last().is_some_and(|byte| byte != b'-')
}

fn depends_on(start: &str, target: &str, by_id: &BTreeMap<&str, &PlannedWorkUnit>) -> bool {
    let mut stack = vec![start];
    let mut seen = BTreeSet::new();
    while let Some(current) = stack.pop() {
        if !seen.insert(current) {
            continue;
        }
        if let Some(unit) = by_id.get(current) {
            for dependency in &unit.depends_on {
                if dependency == target {
                    return true;
                }
                stack.push(dependency);
            }
        }
    }
    false
}

fn topological_order(
    units: &[PlannedWorkUnit],
    by_id: &BTreeMap<&str, &PlannedWorkUnit>,
) -> Result<Vec<String>, String> {
    let mut remaining: BTreeMap<&str, usize> = by_id
        .iter()
        .map(|(id, unit)| (*id, unit.depends_on.len()))
        .collect();
    let mut ready: BTreeSet<&str> = remaining
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| *id)
        .collect();
    let mut order = Vec::with_capacity(units.len());
    while let Some(id) = ready.pop_first() {
        order.push(id.to_owned());
        for unit in units {
            if unit.depends_on.iter().any(|dependency| dependency == id) {
                let count = remaining
                    .get_mut(unit.id.as_str())
                    .ok_or_else(|| "work graph predecessor state".to_string())?;
                *count -= 1;
                if *count == 0 {
                    ready.insert(unit.id.as_str());
                }
            }
        }
    }
    if order.len() != units.len() {
        return Err("work graph contains a dependency cycle".into());
    }
    Ok(order)
}

/// A Spec Manager proposal. Only FORMAL_SPEC requests use this contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SpecDraftModel {
    pub summary: String,
    pub requirements: Vec<SpecRequirementDraft>,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SpecRequirementDraft {
    pub statement: String,
    #[serde(default)]
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub negative: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrozenSpec {
    pub revision: u32,
    pub summary: String,
    pub requirements: Vec<FrozenRequirement>,
    pub assumptions: Vec<String>,
    pub risks: Vec<String>,
    pub spec_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrozenRequirement {
    pub id: String,
    pub statement: String,
    pub assertions: Vec<FrozenAssertion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SpecAssertionKind {
    Acceptance,
    Negative,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrozenAssertion {
    pub id: String,
    pub kind: SpecAssertionKind,
    pub statement: String,
    pub check_id: String,
}

pub const MAX_SPEC_REQUIREMENTS: usize = 40;
pub const MAX_SPEC_CASES: usize = 12;

/// Deterministic spec compilation. Requirement, assertion and check
/// identities are derived from position and revision, never from model text.
/// Every requirement must carry observable acceptance and negative cases.
pub fn compile_spec(draft: &SpecDraftModel, revision: u32) -> Result<FrozenSpec, String> {
    if revision == 0 {
        return Err("spec revision must be positive".into());
    }
    if draft.summary.trim().is_empty() || draft.summary.len() > 4_000 {
        return Err("spec summary is required".into());
    }
    if draft.requirements.is_empty() || draft.requirements.len() > MAX_SPEC_REQUIREMENTS {
        return Err(format!(
            "spec must declare 1-{MAX_SPEC_REQUIREMENTS} requirements"
        ));
    }
    let mut requirements = Vec::with_capacity(draft.requirements.len());
    for (index, requirement) in draft.requirements.iter().enumerate() {
        let number = index + 1;
        if requirement.statement.trim().is_empty() || requirement.statement.len() > 2_000 {
            return Err(format!("requirement {number} statement is invalid"));
        }
        if requirement.acceptance.is_empty() || requirement.acceptance.len() > MAX_SPEC_CASES {
            return Err(format!(
                "requirement {number} needs 1-{MAX_SPEC_CASES} acceptance cases"
            ));
        }
        if requirement.negative.is_empty() || requirement.negative.len() > MAX_SPEC_CASES {
            return Err(format!(
                "requirement {number} needs 1-{MAX_SPEC_CASES} negative cases"
            ));
        }
        let mut assertions = Vec::new();
        for (case_index, statement) in requirement.acceptance.iter().enumerate() {
            let case = case_index + 1;
            if statement.trim().is_empty() || statement.len() > 2_000 {
                return Err(format!(
                    "requirement {number} acceptance case {case} is invalid"
                ));
            }
            assertions.push(FrozenAssertion {
                id: format!("ASR-{number:03}-ACC-{case:02}"),
                kind: SpecAssertionKind::Acceptance,
                statement: statement.clone(),
                check_id: format!("CHK-{number:03}-ACC-{case:02}"),
            });
        }
        for (case_index, statement) in requirement.negative.iter().enumerate() {
            let case = case_index + 1;
            if statement.trim().is_empty() || statement.len() > 2_000 {
                return Err(format!(
                    "requirement {number} negative case {case} is invalid"
                ));
            }
            assertions.push(FrozenAssertion {
                id: format!("ASR-{number:03}-NEG-{case:02}"),
                kind: SpecAssertionKind::Negative,
                statement: statement.clone(),
                check_id: format!("CHK-{number:03}-NEG-{case:02}"),
            });
        }
        requirements.push(FrozenRequirement {
            id: format!("REQ-{number:03}"),
            statement: requirement.statement.clone(),
            assertions,
        });
    }
    let mut frozen = FrozenSpec {
        revision,
        summary: draft.summary.clone(),
        requirements,
        assumptions: draft.assumptions.clone(),
        risks: draft.risks.clone(),
        spec_sha256: String::new(),
    };
    frozen.spec_sha256 = spec_content_sha256(&frozen);
    Ok(frozen)
}

/// Stable digest of the frozen content, excluding the digest field itself.
pub fn spec_content_sha256(spec: &FrozenSpec) -> String {
    let canonical = serde_json::to_vec(&serde_json::json!({
        "revision": spec.revision,
        "summary": spec.summary,
        "requirements": spec.requirements,
        "assumptions": spec.assumptions,
        "risks": spec.risks,
    }))
    .expect("frozen spec is serializable");
    sha256_hex(&canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("factory-agentic-{}", uuid::Uuid::new_v4()))
    }

    fn signals() -> TriageSignals {
        TriageSignals {
            objective: "Add a bounded endpoint".into(),
            files: vec!["src/lib.rs".into()],
            domains: vec!["api".into()],
            risk: "low".into(),
            expected_work_units: None,
            ambiguous: false,
            architecture_uncertainty: false,
            cross_module: false,
            public_contract: false,
            data_or_schema: false,
            security_boundary: false,
            explicit_informational: false,
            explicit_audit: false,
            explicit_repair: false,
        }
    }

    #[test]
    fn informational_and_audit_never_route_to_implementation() {
        let mut value = signals();
        value.explicit_informational = true;
        let decision = triage(&value);
        assert_eq!(decision.request_kind, RequestKind::Informational);
        assert_eq!(decision.route, ChangeRoute::NoAgent);

        value = signals();
        value.explicit_audit = true;
        let decision = triage(&value);
        assert_eq!(decision.request_kind, RequestKind::Audit);
        assert_eq!(decision.route, ChangeRoute::Direct);
        assert_eq!(decision.assurance, Assurance::Standard);
    }

    #[test]
    fn security_and_public_contract_raise_assurance_and_formal_spec() {
        let mut value = signals();
        value.security_boundary = true;
        value.public_contract = true;
        let decision = triage(&value);
        assert_eq!(decision.route, ChangeRoute::FormalSpec);
        assert_eq!(decision.assurance, Assurance::Critical);
    }

    #[test]
    fn multi_unit_or_cross_module_requires_delegation() {
        let mut value = signals();
        value.expected_work_units = Some(3);
        let decision = triage(&value);
        assert_eq!(decision.route, ChangeRoute::FormalSpec);
        value.expected_work_units = None;
        value.cross_module = true;
        let decision = triage(&value);
        assert_eq!(decision.route, ChangeRoute::FormalSpec);
    }

    #[test]
    fn direct_is_reserved_for_bounded_concrete_scope() {
        let decision = triage(&signals());
        assert_eq!(decision.route, ChangeRoute::Direct);
        assert_eq!(decision.assurance, Assurance::None);
    }

    #[test]
    fn stack_detection_uses_manifests_and_leaves_unknowns_explicit() {
        let root = temp_dir();
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname=\"x\"\n[dependencies]\naxum=\"1\"\nsqlx=\"1\"\n",
        )
        .unwrap();
        fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"react":"1","vite":"1"},"devDependencies":{"vitest":"1","@playwright/test":"1"}}"#,
        )
        .unwrap();
        fs::write(root.join("playwright.config.ts"), "export default {}").unwrap();
        let stack = detect_stack(&root);
        assert!(stack.languages.contains(&"rust".into()));
        assert!(stack.languages.contains(&"typescript".into()));
        assert!(stack.frameworks.contains(&"axum".into()));
        assert!(stack.frameworks.contains(&"react".into()));
        assert!(stack.test_frameworks.contains(&"vitest".into()));
        assert!(stack.browser.contains(&"playwright".into()));
        assert_eq!(stack.confidence, "high");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn model_recommendation_can_raise_but_not_lower_the_floor() {
        let decision = triage(&signals());
        let model = AgenticTriageModel {
            request_kind: RequestKind::Change,
            route: ChangeRoute::FormalSpec,
            assurance: Assurance::Critical,
            stack: StackProfile::default(),
            files: vec!["src/lib.rs".into()],
            domains: vec![],
            risk: "low".into(),
            expected_work_units: Some(1),
            ambiguous: false,
            architecture_uncertainty: false,
            cross_module: false,
            public_contract: false,
            data_or_schema: false,
            security_boundary: false,
            skill_needs: vec![],
            unknowns: vec!["version".into()],
        };
        let merged = merge_model_decision(decision, &model);
        assert_eq!(merged.route, ChangeRoute::FormalSpec);
        assert_eq!(merged.assurance, Assurance::Critical);
        assert!(merged.unknowns.contains(&"version".into()));
    }

    #[test]
    fn participant_composition_is_deterministic_and_minimal() {
        let stack = StackProfile {
            languages: vec!["rust".into()],
            ..StackProfile::default()
        };
        let direct = participant_plan(ChangeRoute::Direct, Assurance::None, 1, &stack);
        assert_eq!(direct.assignments.len(), 3);
        assert!(direct.assignments.iter().any(|a| a.role == "Implementer"));
        assert!(
            direct
                .assignments
                .iter()
                .filter(|a| a.role == "Reviewer")
                .all(|a| a.read_only)
        );
        let formal = participant_plan(ChangeRoute::FormalSpec, Assurance::Elevated, 3, &stack);
        assert!(formal.assignments.iter().any(|a| a.role == "Planner"));
        assert!(formal.assignments.iter().any(|a| a.role == "Spec Manager"));
        assert!(formal.assignments.iter().any(|a| a.count == 3));
    }

    fn unit(id: &str, depends_on: &[&str], paths: &[&str]) -> PlannedWorkUnit {
        PlannedWorkUnit {
            id: id.into(),
            title: format!("Unit {id}"),
            objective: format!("Complete {id}"),
            depends_on: depends_on.iter().map(|value| value.to_string()).collect(),
            paths: paths.iter().map(|value| value.to_string()).collect(),
            acceptance: vec!["observable result".into()],
        }
    }

    #[test]
    fn work_graph_orders_dependencies_and_freezes_identity() {
        let model = WorkGraphModel {
            units: vec![
                unit("api", &["model"], &["src/api.rs"]),
                unit("model", &[], &["src/model.rs"]),
            ],
            assumptions: vec!["rust".into()],
            risks: vec![],
        };
        let graph = validate_work_graph(&model, &["src".into()]).unwrap();
        assert_eq!(graph.topological_order, vec!["model", "api"]);
        assert_eq!(graph.graph_sha256.len(), 64);
        let again = validate_work_graph(&model, &["src".into()]).unwrap();
        assert_eq!(graph.graph_sha256, again.graph_sha256);
    }

    #[test]
    fn work_graph_rejects_cycles_unknown_dependencies_and_scope_escape() {
        let cyclic = WorkGraphModel {
            units: vec![
                unit("a", &["b"], &["src/a.rs"]),
                unit("b", &["a"], &["src/b.rs"]),
            ],
            assumptions: vec![],
            risks: vec![],
        };
        assert!(validate_work_graph(&cyclic, &["src".into()]).is_err());
        let unknown = WorkGraphModel {
            units: vec![unit("a", &["missing"], &["src/a.rs"])],
            assumptions: vec![],
            risks: vec![],
        };
        assert!(validate_work_graph(&unknown, &["src".into()]).is_err());
        let escaped = WorkGraphModel {
            units: vec![unit("a", &[], &["../secret"])],
            assumptions: vec![],
            risks: vec![],
        };
        assert!(validate_work_graph(&escaped, &[]).is_err());
        let outside = WorkGraphModel {
            units: vec![unit("a", &[], &["tests/a.rs"])],
            assumptions: vec![],
            risks: vec![],
        };
        assert!(validate_work_graph(&outside, &["src".into()]).is_err());
    }

    #[test]
    fn independent_units_cannot_share_a_write_path() {
        let model = WorkGraphModel {
            units: vec![
                unit("a", &[], &["src/lib.rs"]),
                unit("b", &[], &["src/lib.rs"]),
            ],
            assumptions: vec![],
            risks: vec![],
        };
        assert!(validate_work_graph(&model, &["src".into()]).is_err());
        let prefix = WorkGraphModel {
            units: vec![unit("a", &[], &["src"]), unit("b", &[], &["src/api.rs"])],
            assumptions: vec![],
            risks: vec![],
        };
        assert!(validate_work_graph(&prefix, &["src".into()]).is_err());
    }

    fn spec_draft() -> SpecDraftModel {
        SpecDraftModel {
            summary: "Bounded calendar validation".into(),
            requirements: vec![SpecRequirementDraft {
                statement: "Invalid dates are rejected.".into(),
                acceptance: vec!["2025-13-01 is rejected".into()],
                negative: vec!["2024-02-29 is accepted".into()],
            }],
            assumptions: vec![],
            risks: vec![],
        }
    }

    #[test]
    fn spec_compilation_is_deterministic_and_traceable() {
        let frozen = compile_spec(&spec_draft(), 1).unwrap();
        assert_eq!(frozen.requirements[0].id, "REQ-001");
        assert_eq!(
            frozen.requirements[0].assertions[0].check_id,
            "CHK-001-ACC-01"
        );
        assert_eq!(
            frozen.requirements[0].assertions[1].check_id,
            "CHK-001-NEG-01"
        );
        assert_eq!(frozen.spec_sha256.len(), 64);
        let again = compile_spec(&spec_draft(), 1).unwrap();
        assert_eq!(frozen.spec_sha256, again.spec_sha256);
        let revision = compile_spec(&spec_draft(), 2).unwrap();
        assert_ne!(frozen.spec_sha256, revision.spec_sha256);
    }

    #[test]
    fn spec_compilation_requires_acceptance_and_negative_cases() {
        let mut draft = spec_draft();
        draft.requirements[0].negative.clear();
        assert!(compile_spec(&draft, 1).is_err());
        draft.requirements[0].negative.push("reject".into());
        draft.requirements[0].acceptance.clear();
        assert!(compile_spec(&draft, 1).is_err());
        assert!(compile_spec(&spec_draft(), 0).is_err());
    }

    #[test]
    fn graph_plan_binds_units_and_two_reviewers_for_elevated_assurance() {
        let model = WorkGraphModel {
            units: vec![
                unit("model", &[], &["src/model.rs"]),
                unit("api", &["model"], &["src/api.rs"]),
            ],
            assumptions: vec![],
            risks: vec![],
        };
        let graph = validate_work_graph(&model, &["src".into()]).unwrap();
        let stack = StackProfile {
            languages: vec!["rust".into()],
            ..StackProfile::default()
        };
        let plan = participant_plan_for_graph(
            ChangeRoute::FormalSpec,
            Assurance::Elevated,
            &graph,
            &stack,
            "opencode",
            "deepseek-v4.1-flash",
            "build",
        );
        assert_eq!(plan.reviewer_count, 6);
        assert_eq!(plan.reviewer_roles.len(), 6);
        assert_eq!(plan.unit_assignments.len(), 2);
        assert_eq!(plan.unit_assignments[0].specialization, "rust");
        assert_eq!(
            plan.unit_assignments[1].model.as_deref(),
            Some("deepseek-v4.1-flash")
        );
        assert!(
            plan.assignments
                .iter()
                .any(|assignment| assignment.role == "Reviewer" && assignment.count == 6)
        );
    }

    #[test]
    fn context_policy_is_bounded() {
        let policy = ContextPolicy::default();
        assert_eq!(policy.compact_at_percent, 70);
        assert_eq!(policy.max_consecutive_compactions, 2);
        assert!(policy.require_probe_after_handoff);
    }

    #[test]
    fn charter_cannot_grant_policy_autonomy_or_outlive_expiry() {
        let mut charter = OwnerCharter {
            id: "charter".into(),
            owner: "owner".into(),
            projects: vec!["project".into()],
            domains: vec!["repo".into()],
            risk_appetite: Assurance::Elevated,
            execution_autonomy: true,
            repair_autonomy: true,
            delivery_autonomy: false,
            policy_autonomy: false,
            reserved_matters: BTreeSet::from(["credentials".into()]),
            expires_at: "2027-01-01T00:00:00Z".into(),
            revoked_at: None,
        };
        assert!(charter.validate().is_ok());
        assert!(charter.covers(
            "project",
            "repo",
            Assurance::Elevated,
            "2026-11-08T00:00:00Z"
        ));
        assert!(!charter.covers(
            "project",
            "repo",
            Assurance::Critical,
            "2026-11-08T00:00:00Z"
        ));
        charter.policy_autonomy = true;
        assert!(charter.validate().is_err());
    }
}
