//! Typed, advisory Intelligent Lead planning domain.
//!
//! This module owns deterministic validation of model-informed proposals and
//! standing delegation snapshots. It performs no I/O, grants no execution
//! authority, and does not activate autonomous start. Core persistence and
//! policy remain responsible for issuance, reservation, revocation and
//! Campaign transitions.
//!
//! Decision mechanisms: semantic assessment is `MODEL` input; proposal,
//! scope, dependency, budget, risk and route checks are `DETERMINISTIC`; the
//! end-to-end planning path is `HYBRID`. Model output never grants authority.

use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_WORK_UNITS: usize = 8;
pub const MAX_PREDECESSORS: usize = 8;
pub const MAX_UNIT_ID_CHARS: usize = 32;
pub const MAX_UNIT_DESCRIPTION_CHARS: usize = 2_000;
pub const MAX_GRAPH_TASKS: usize = 32;
pub const MAX_CAMPAIGN_IMPLEMENTER_RUNS: usize = 64;
pub const MAX_DELEGATION_REQUESTS: u32 = 16;
pub const MAX_CUMULATIVE_SESSIONS: u32 = 256;
pub const MAX_CUMULATIVE_ATTEMPTS: u32 = 512;
pub const MAX_LEAD_SESSIONS: u32 = (MAX_GRAPH_TASKS * MAX_CAMPAIGN_IMPLEMENTER_RUNS) as u32;
pub const MAX_CHECK_TIMEOUT_SECS: u32 = 600;
pub const MAX_ASSUMPTIONS: usize = 16;
pub const MAX_NOTE_CHARS: usize = 500;
pub const MAX_EVIDENCE_REFS: usize = 32;
pub const MAX_EFFORT_PERSON_HOURS: u32 = 10_000;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LeadError {
    #[error("invalid identity: {0}")]
    Identity(String),
    #[error("invalid delegation: {0}")]
    Delegation(String),
    #[error("invalid assessment: {0}")]
    Assessment(String),
    #[error("invalid proposal: {0}")]
    Proposal(String),
    #[error("invalid work graph: {0}")]
    WorkGraph(String),
    #[error("invalid budget: {0}")]
    Budget(String),
    #[error("invalid route: {0}")]
    Route(String),
    #[error("invalid state transition: {0}")]
    State(String),
    #[error("invalid bounded field: {0}")]
    Field(String),
    #[error("invalid path: {0}")]
    Path(String),
    #[error("duplicate value: {0}")]
    Duplicate(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObjectiveClass {
    Feature,
    Maintenance,
    Refactor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RiskClass {
    R0,
    R1,
    R2,
    R3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    RepositoryEdit,
    NodeToolchain,
    GenericToolchain,
    Browser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualityProfile {
    NodeBasic,
    NodeBrowser,
    Generic,
}

impl QualityProfile {
    pub fn required_check_kinds(self) -> &'static [CheckKind] {
        match self {
            Self::NodeBasic => &[
                CheckKind::Format,
                CheckKind::Test,
                CheckKind::Functional,
                CheckKind::Negative,
            ],
            Self::NodeBrowser => &[
                CheckKind::Format,
                CheckKind::Test,
                CheckKind::Functional,
                CheckKind::Negative,
                CheckKind::Browser,
            ],
            Self::Generic => &[
                CheckKind::Build,
                CheckKind::Test,
                CheckKind::Functional,
                CheckKind::Negative,
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckKind {
    Format,
    Lint,
    Build,
    Test,
    Functional,
    Negative,
    Browser,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SpecCheck {
    pub id: String,
    pub kind: CheckKind,
    pub description: String,
    pub command: Vec<String>,
    #[serde(default)]
    pub expect_exit: i32,
    #[serde(default = "default_check_timeout")]
    pub timeout_secs: u32,
}

fn default_check_timeout() -> u32 {
    60
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderRoute {
    pub provider: String,
    pub model: String,
    pub mode: String,
}

impl ProviderRoute {
    fn validate(&self) -> Result<(), LeadError> {
        for (name, value) in [
            ("provider", self.provider.as_str()),
            ("model", self.model.as_str()),
            ("mode", self.mode.as_str()),
        ] {
            if value.is_empty() || value.len() > 200 || value.chars().any(char::is_whitespace) {
                return Err(LeadError::Route(format!(
                    "{name} must be a bounded non-whitespace value"
                )));
            }
        }
        if !matches!(self.provider.as_str(), "codex" | "opencode") {
            return Err(LeadError::Route(format!(
                "unsupported provider {:?}; select codex or opencode explicitly",
                self.provider
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StandingDelegation {
    pub id: String,
    pub principal_id: String,
    pub audit_id: String,
    pub project_id: String,
    pub revision: u32,
    pub content_hash: String,
    pub objective_classes: Vec<ObjectiveClass>,
    pub allowed_paths: Vec<String>,
    pub capabilities: Vec<Capability>,
    pub risk_classes: Vec<RiskClass>,
    pub autonomous_start: bool,
    pub autonomous_risk_classes: Vec<RiskClass>,
    pub route: ProviderRoute,
    pub max_attempts: u32,
    pub max_timeout_secs: u32,
    pub max_repair_cycles: u32,
    pub max_wall_clock_secs: u32,
    pub max_parallel: u32,
    pub max_sessions: u32,
    pub max_requests: u32,
    pub max_cumulative_sessions: u32,
    pub max_cumulative_attempts: u32,
    pub expires_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DelegationContent<'a> {
    id: &'a str,
    principal_id: &'a str,
    audit_id: &'a str,
    project_id: &'a str,
    revision: u32,
    objective_classes: &'a [ObjectiveClass],
    allowed_paths: &'a [String],
    capabilities: &'a [Capability],
    risk_classes: &'a [RiskClass],
    autonomous_start: bool,
    autonomous_risk_classes: &'a [RiskClass],
    route: &'a ProviderRoute,
    max_attempts: u32,
    max_timeout_secs: u32,
    max_repair_cycles: u32,
    max_wall_clock_secs: u32,
    max_parallel: u32,
    max_sessions: u32,
    max_requests: u32,
    max_cumulative_sessions: u32,
    max_cumulative_attempts: u32,
    expires_at: &'a DateTime<Utc>,
}

impl StandingDelegation {
    /// Immutable grant identity. Revocation is a separate one-way transition
    /// and is excluded from the content hash by design.
    pub fn compute_content_hash(&self) -> String {
        let content = DelegationContent {
            id: &self.id,
            principal_id: &self.principal_id,
            audit_id: &self.audit_id,
            project_id: &self.project_id,
            revision: self.revision,
            objective_classes: &self.objective_classes,
            allowed_paths: &self.allowed_paths,
            capabilities: &self.capabilities,
            risk_classes: &self.risk_classes,
            autonomous_start: self.autonomous_start,
            autonomous_risk_classes: &self.autonomous_risk_classes,
            route: &self.route,
            max_attempts: self.max_attempts,
            max_timeout_secs: self.max_timeout_secs,
            max_repair_cycles: self.max_repair_cycles,
            max_wall_clock_secs: self.max_wall_clock_secs,
            max_parallel: self.max_parallel,
            max_sessions: self.max_sessions,
            max_requests: self.max_requests,
            max_cumulative_sessions: self.max_cumulative_sessions,
            max_cumulative_attempts: self.max_cumulative_attempts,
            expires_at: &self.expires_at,
        };
        let raw = serde_json::to_vec(&content).expect("delegation content is serializable");
        sha256_hex(&raw)
    }

    /// Validate an issued grant without applying defaults or broadening it.
    pub fn validate(&self, now: DateTime<Utc>) -> Result<(), LeadError> {
        for (name, value) in [
            ("id", self.id.as_str()),
            ("principalId", self.principal_id.as_str()),
            ("auditId", self.audit_id.as_str()),
            ("projectId", self.project_id.as_str()),
        ] {
            if value.trim().is_empty() || value.len() > 200 {
                return Err(LeadError::Delegation(format!(
                    "{name} is required and bounded"
                )));
            }
        }
        if self.revision == 0 {
            return Err(LeadError::Delegation("revision must be positive".into()));
        }
        if !is_hex(&self.content_hash, 64) || self.content_hash != self.compute_content_hash() {
            return Err(LeadError::Delegation(
                "contentHash must identify the immutable grant content".into(),
            ));
        }
        if self.revoked_at.is_some() {
            return Err(LeadError::Delegation("delegation is revoked".into()));
        }
        if self.expires_at <= now {
            return Err(LeadError::Delegation(
                "delegation is expired or has no future expiry".into(),
            ));
        }
        if self.objective_classes.is_empty()
            || self.allowed_paths.is_empty()
            || self.capabilities.is_empty()
            || self.risk_classes.is_empty()
        {
            return Err(LeadError::Delegation(
                "objective classes, paths, capabilities and risk classes must be explicit".into(),
            ));
        }
        unique_values("objectiveClasses", &self.objective_classes)?;
        unique_values("riskClasses", &self.risk_classes)?;
        unique_values("capabilities", &self.capabilities)?;
        unique_values("autonomousRiskClasses", &self.autonomous_risk_classes)?;
        validate_paths("delegation allowedPaths", &self.allowed_paths)?;
        for risk in &self.autonomous_risk_classes {
            if !self.risk_classes.contains(risk) {
                return Err(LeadError::Delegation(format!(
                    "autonomous risk class {risk:?} is outside the delegation"
                )));
            }
            if !matches!(risk, RiskClass::R0 | RiskClass::R1) {
                return Err(LeadError::Delegation(format!(
                    "autonomous risk class {risk:?} is not permitted"
                )));
            }
        }
        let has_autonomous = !self.autonomous_risk_classes.is_empty();
        if self.autonomous_start != has_autonomous {
            return Err(LeadError::Delegation(
                "autonomousStart must exactly match a nonempty autonomous risk-class list".into(),
            ));
        }
        self.route.validate()?;
        if !(1..=5).contains(&self.max_attempts)
            || !(30..=600).contains(&self.max_timeout_secs)
            || self.max_repair_cycles > 3
            || !(120..=7_200).contains(&self.max_wall_clock_secs)
            || !(1..=2).contains(&self.max_parallel)
            || !(1..=MAX_LEAD_SESSIONS).contains(&self.max_sessions)
        {
            return Err(LeadError::Budget(
                "per-Campaign limits are not explicit or within supported bounds".into(),
            ));
        }
        if !(1..=MAX_DELEGATION_REQUESTS).contains(&self.max_requests)
            || !(self.max_sessions..=MAX_CUMULATIVE_SESSIONS)
                .contains(&self.max_cumulative_sessions)
            || !(self.max_attempts..=MAX_CUMULATIVE_ATTEMPTS)
                .contains(&self.max_cumulative_attempts)
        {
            return Err(LeadError::Budget(
                "cumulative limits must be explicit, ordered and within supported bounds".into(),
            ));
        }
        Ok(())
    }

    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        self.revoked_at.is_none() && self.expires_at > now
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProjectBinding {
    pub id: String,
    pub active: bool,
    pub config_hash: String,
    pub allowed_paths: Vec<String>,
    pub capabilities: Vec<Capability>,
    pub quality_profile: QualityProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Pending,
    InProgress,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ActionBinding {
    pub id: String,
    pub project_id: String,
    pub status: ActionStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SpecBudget {
    pub max_attempts: u32,
    pub timeout_secs: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LeadWorkUnit {
    pub id: String,
    pub kind: WorkUnitKind,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub allowed_paths: Vec<String>,
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub acceptance_criteria: Vec<SpecCheck>,
    #[serde(default)]
    pub negative_cases: Vec<SpecCheck>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkUnitKind {
    Implementation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LeadPlanProposal {
    pub project_id: String,
    pub action_id: String,
    pub base_revision: String,
    pub project_config_hash: String,
    pub objective: String,
    pub class: ObjectiveClass,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
    pub acceptance_criteria: Vec<SpecCheck>,
    pub negative_cases: Vec<SpecCheck>,
    pub quality_profile: QualityProfile,
    pub risk_class: RiskClass,
    pub budget: SpecBudget,
    pub route: ProviderRoute,
    pub max_repair_cycles: u32,
    pub max_wall_clock_secs: u32,
    pub max_parallel: u32,
    pub max_sessions: u32,
    pub work_units: Vec<LeadWorkUnit>,
}

impl LeadPlanProposal {
    pub fn content_hash(&self) -> String {
        let raw = serde_json::to_vec(self).expect("proposal is serializable");
        sha256_hex(&raw)
    }

    pub fn terminal_units(&self) -> Vec<&LeadWorkUnit> {
        let depended_on: HashSet<&str> = self
            .work_units
            .iter()
            .flat_map(|unit| unit.depends_on.iter().map(String::as_str))
            .collect();
        self.work_units
            .iter()
            .filter(|unit| !depended_on.contains(unit.id.as_str()))
            .collect()
    }

    pub fn needs_integration(&self) -> bool {
        self.terminal_units().len() > 1
    }

    /// Number of model-backed implementer executions the frozen plan can
    /// dispatch. Deterministic integration is not counted.
    pub fn implementer_task_count(&self) -> usize {
        self.work_units.len() + self.max_repair_cycles as usize
    }

    /// Effective transitive scope for integration or fan-in units.
    pub fn unit_scope(&self, unit_id: &str) -> Option<(Vec<String>, Vec<Capability>)> {
        let by_id: HashMap<&str, &LeadWorkUnit> = self
            .work_units
            .iter()
            .map(|unit| (unit.id.as_str(), unit))
            .collect();
        let mut paths = BTreeSet::new();
        let mut capabilities = BTreeSet::new();
        let mut seen = HashSet::new();
        let mut stack = vec![unit_id];
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let unit = by_id.get(id)?;
            paths.extend(unit.allowed_paths.iter().cloned());
            capabilities.extend(unit.capabilities.iter().copied());
            stack.extend(unit.depends_on.iter().map(String::as_str));
        }
        Some((
            paths.into_iter().collect(),
            capabilities.into_iter().collect(),
        ))
    }

    /// Deterministic Core validation against the active Project, Action and
    /// delegation. Never broadens authority and never performs I/O.
    pub fn validate(
        &self,
        grant: &StandingDelegation,
        project: &ProjectBinding,
        action: &ActionBinding,
        now: DateTime<Utc>,
    ) -> Result<(), LeadError> {
        grant.validate(now)?;
        if !project.active {
            return Err(LeadError::Proposal(
                "planning requires an active registered project".into(),
            ));
        }
        if !is_hex(&project.config_hash, 64) {
            return Err(LeadError::Proposal(
                "project configuration hash is invalid".into(),
            ));
        }
        if self.project_id != project.id
            || grant.project_id != project.id
            || action.project_id != project.id
        {
            return Err(LeadError::Proposal(
                "proposal, delegation, action and project identities must match".into(),
            ));
        }
        if self.action_id != action.id {
            return Err(LeadError::Proposal(
                "proposal actionId must identify the bound Action".into(),
            ));
        }
        if !matches!(
            action.status,
            ActionStatus::Pending | ActionStatus::InProgress
        ) {
            return Err(LeadError::Proposal(
                "planning requires a nonterminal Action".into(),
            ));
        }
        if self.project_config_hash != project.config_hash {
            return Err(LeadError::Proposal(
                "proposal projectConfigHash is stale or mismatched".into(),
            ));
        }
        if !is_git_object_id(&self.base_revision) {
            return Err(LeadError::Proposal(
                "baseRevision must be a full Git object id".into(),
            ));
        }
        if self.objective.trim().is_empty() || self.objective.chars().count() > 4_000 {
            return Err(LeadError::Proposal(
                "objective is required and bounded".into(),
            ));
        }
        if !grant.objective_classes.contains(&self.class) {
            return Err(LeadError::Proposal(
                "objective class is outside the delegation".into(),
            ));
        }
        validate_notes("assumptions", &self.assumptions)?;
        validate_notes("risks", &self.risks)?;
        if self.quality_profile != project.quality_profile {
            return Err(LeadError::Proposal(
                "proposal quality profile differs from the registered project".into(),
            ));
        }
        validate_checks(&self.acceptance_criteria, false)?;
        validate_checks(&self.negative_cases, true)?;
        if self.acceptance_criteria.is_empty() || self.negative_cases.is_empty() {
            return Err(LeadError::Proposal(
                "at least one acceptance criterion and one negative case are required".into(),
            ));
        }
        require_check_kinds(
            self.acceptance_criteria
                .iter()
                .chain(self.negative_cases.iter()),
            self.quality_profile.required_check_kinds(),
        )?;
        if !grant.risk_classes.contains(&self.risk_class) {
            return Err(LeadError::Proposal(
                "risk class is outside the delegation".into(),
            ));
        }
        if self.budget.max_attempts == 0
            || self.budget.max_attempts > grant.max_attempts
            || !(30..=600).contains(&self.budget.timeout_secs)
            || self.budget.timeout_secs > grant.max_timeout_secs
        {
            return Err(LeadError::Budget(
                "spec attempts or timeout exceed the delegation".into(),
            ));
        }
        if self.route != grant.route {
            return Err(LeadError::Route(
                "provider route must exactly match the delegation".into(),
            ));
        }
        if self.max_repair_cycles > grant.max_repair_cycles
            || !(120..=7_200).contains(&self.max_wall_clock_secs)
            || self.max_wall_clock_secs > grant.max_wall_clock_secs
            || self.max_parallel == 0
            || self.max_parallel > grant.max_parallel
            || self.max_parallel > 2
            || self.max_sessions == 0
            || self.max_sessions > grant.max_sessions
            || self.max_sessions > MAX_LEAD_SESSIONS
        {
            return Err(LeadError::Budget(
                "Campaign budgets exceed the delegation or supported bounds".into(),
            ));
        }
        if self.work_units.is_empty() || self.work_units.len() > MAX_WORK_UNITS {
            return Err(LeadError::WorkGraph(format!(
                "work graph requires 1..={MAX_WORK_UNITS} implementation units"
            )));
        }
        self.validate_work_units(grant, project)?;
        let implementer_attempts = self
            .implementer_task_count()
            .checked_mul(self.budget.max_attempts as usize)
            .ok_or_else(|| LeadError::Budget("implementer attempt count overflow".into()))?;
        if implementer_attempts > MAX_CAMPAIGN_IMPLEMENTER_RUNS {
            return Err(LeadError::Budget(
                "work graph implementer attempts exceed the supported Campaign ceiling".into(),
            ));
        }
        let required_sessions = 1usize
            .checked_add(implementer_attempts)
            .ok_or_else(|| LeadError::Budget("session count overflow".into()))?;
        if required_sessions > self.max_sessions as usize {
            return Err(LeadError::Budget(
                "assessment plus model-backed work-unit/repair attempts exceed the session ceiling"
                    .into(),
            ));
        }
        Ok(())
    }

    fn validate_work_units(
        &self,
        grant: &StandingDelegation,
        project: &ProjectBinding,
    ) -> Result<(), LeadError> {
        let mut by_id: HashMap<&str, &LeadWorkUnit> = HashMap::new();
        let mut paths_by_id: HashMap<&str, &[String]> = HashMap::new();
        for unit in &self.work_units {
            if unit.id.is_empty() || unit.id.len() > MAX_UNIT_ID_CHARS || !is_slug(&unit.id) {
                return Err(LeadError::WorkGraph(format!(
                    "WorkUnit id {:?} must be a lowercase bounded slug",
                    unit.id
                )));
            }
            if !matches!(unit.kind, WorkUnitKind::Implementation) {
                return Err(LeadError::WorkGraph(
                    "only implementation WorkUnits are supported".into(),
                ));
            }
            if unit.description.chars().count() > MAX_UNIT_DESCRIPTION_CHARS {
                return Err(LeadError::WorkGraph(format!(
                    "WorkUnit {} description is too long",
                    unit.id
                )));
            }
            if by_id.insert(unit.id.as_str(), unit).is_some() {
                return Err(LeadError::WorkGraph(format!(
                    "duplicate WorkUnit id {:?}",
                    unit.id
                )));
            }
            if unit.depends_on.len() > MAX_PREDECESSORS {
                return Err(LeadError::WorkGraph(format!(
                    "WorkUnit {} declares more than {MAX_PREDECESSORS} predecessors",
                    unit.id
                )));
            }
            unique_values("dependsOn", &unit.depends_on)?;
            validate_paths(
                &format!("WorkUnit {} allowedPaths", unit.id),
                &unit.allowed_paths,
            )?;
            unique_values("capabilities", &unit.capabilities)?;
            if !unit.allowed_paths.is_empty()
                && !unit
                    .allowed_paths
                    .iter()
                    .all(|path| covered_by(path, &grant.allowed_paths))
            {
                return Err(LeadError::WorkGraph(format!(
                    "WorkUnit {} paths are outside the delegation",
                    unit.id
                )));
            }
            if !unit.allowed_paths.is_empty()
                && !unit
                    .allowed_paths
                    .iter()
                    .all(|path| covered_by(path, &project.allowed_paths))
            {
                return Err(LeadError::WorkGraph(format!(
                    "WorkUnit {} paths are outside the project scope",
                    unit.id
                )));
            }
            if !unit
                .capabilities
                .iter()
                .all(|capability| grant.capabilities.contains(capability))
            {
                return Err(LeadError::WorkGraph(format!(
                    "WorkUnit {} capabilities exceed the delegation",
                    unit.id
                )));
            }
            if !unit
                .capabilities
                .iter()
                .all(|capability| project.capabilities.contains(capability))
            {
                return Err(LeadError::WorkGraph(format!(
                    "WorkUnit {} capabilities exceed the project grant",
                    unit.id
                )));
            }
            validate_checks(&unit.acceptance_criteria, false)?;
            validate_checks(&unit.negative_cases, true)?;
            paths_by_id.insert(unit.id.as_str(), &unit.allowed_paths);
        }
        if by_id.is_empty() {
            return Err(LeadError::WorkGraph("work graph is empty".into()));
        }
        for unit in &self.work_units {
            for dependency in &unit.depends_on {
                if dependency == &unit.id {
                    return Err(LeadError::WorkGraph(format!(
                        "WorkUnit {} depends on itself",
                        unit.id
                    )));
                }
                if !by_id.contains_key(dependency.as_str()) {
                    return Err(LeadError::WorkGraph(format!(
                        "WorkUnit {} references unknown predecessor {dependency:?}",
                        unit.id
                    )));
                }
            }
        }
        self.validate_acyclic(&by_id)?;

        for (index, left) in self.work_units.iter().enumerate() {
            for right in &self.work_units[index + 1..] {
                if self.is_ancestor(&left.id, &right.id, &by_id)
                    || self.is_ancestor(&right.id, &left.id, &by_id)
                {
                    continue;
                }
                let left_paths = paths_by_id
                    .get(left.id.as_str())
                    .copied()
                    .unwrap_or_default();
                let right_paths = paths_by_id
                    .get(right.id.as_str())
                    .copied()
                    .unwrap_or_default();
                if left_paths
                    .iter()
                    .any(|a| right_paths.iter().any(|b| paths_overlap(a, b)))
                {
                    return Err(LeadError::WorkGraph(format!(
                        "parallel WorkUnits {} and {} have overlapping paths",
                        left.id, right.id
                    )));
                }
            }
        }
        Ok(())
    }

    fn validate_acyclic(&self, by_id: &HashMap<&str, &LeadWorkUnit>) -> Result<(), LeadError> {
        let mut state: HashMap<&str, u8> = HashMap::new();
        for unit in &self.work_units {
            visit(unit.id.as_str(), by_id, &mut state)?;
        }
        Ok(())
    }

    fn is_ancestor(
        &self,
        ancestor: &str,
        node: &str,
        by_id: &HashMap<&str, &LeadWorkUnit>,
    ) -> bool {
        let mut seen = HashSet::new();
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            if !seen.insert(current) {
                continue;
            }
            let Some(unit) = by_id.get(current) else {
                continue;
            };
            for dependency in &unit.depends_on {
                if dependency == ancestor {
                    return true;
                }
                stack.push(dependency.as_str());
            }
        }
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Intent {
    Informational,
    Audit,
    Change,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Complexity {
    Small,
    Medium,
    Large,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffortStatus {
    Estimated,
    Unknown,
    NotRequested,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EffortEstimate {
    pub status: EffortStatus,
    pub unit: String,
    pub min_person_hours: u32,
    pub max_person_hours: u32,
    pub assumptions: Vec<String>,
    pub unknown_reason: String,
}

impl EffortEstimate {
    fn validate(&self) -> Result<(), LeadError> {
        match self.status {
            EffortStatus::Estimated => {
                if self.unit != "person-hours"
                    || self.min_person_hours < 1
                    || self.min_person_hours >= self.max_person_hours
                    || self.max_person_hours > MAX_EFFORT_PERSON_HOURS
                    || self.assumptions.is_empty()
                    || !self.unknown_reason.is_empty()
                {
                    return Err(LeadError::Assessment(
                        "estimated effort requires person-hours with 1 <= min < max and assumptions"
                            .into(),
                    ));
                }
                validate_notes("effort assumptions", &self.assumptions)?;
            }
            EffortStatus::Unknown => {
                if !self.unit.is_empty()
                    || self.min_person_hours != 0
                    || self.max_person_hours != 0
                    || self.unknown_reason.trim().is_empty()
                    || self.unknown_reason.chars().count() > MAX_NOTE_CHARS
                {
                    return Err(LeadError::Assessment(
                        "unknown effort requires empty unit/bounds and a bounded reason".into(),
                    ));
                }
                validate_notes("effort assumptions", &self.assumptions)?;
            }
            EffortStatus::NotRequested => {
                if !self.unit.is_empty()
                    || self.min_person_hours != 0
                    || self.max_person_hours != 0
                    || !self.assumptions.is_empty()
                    || !self.unknown_reason.is_empty()
                {
                    return Err(LeadError::Assessment(
                        "not_requested effort must not carry estimates or assumptions".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LeadAssessment {
    pub schema_revision: u32,
    pub project_id: String,
    pub action_id: String,
    pub request_id: String,
    pub evidence_refs: Vec<String>,
    pub intent: Intent,
    pub complexity: Complexity,
    pub effort_estimate: EffortEstimate,
    #[serde(default)]
    pub knowledge_gaps: Vec<String>,
    #[serde(default)]
    pub change_impact: Vec<String>,
    #[serde(default)]
    pub validation_needs: Vec<String>,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub uncertainty: Vec<String>,
    pub suggested_next_action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal: Option<LeadPlanProposal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssessmentInput {
    pub project_id: String,
    pub action_id: String,
    pub request_id: String,
    pub objective: String,
    pub allowed_evidence_refs: BTreeSet<String>,
}

impl LeadAssessment {
    /// Decode the strict advisory assessment. Unknown fields, duplicate
    /// fields, nulls and trailing JSON are rejected by the typed decoder.
    pub fn from_json(raw: &[u8], input: &AssessmentInput) -> Result<Self, LeadError> {
        let assessment: Self = serde_json::from_slice(raw)
            .map_err(|error| LeadError::Assessment(format!("invalid assessment JSON: {error}")))?;
        assessment.validate(input)?;
        Ok(assessment)
    }

    pub fn validate(&self, input: &AssessmentInput) -> Result<(), LeadError> {
        if self.schema_revision != 1 {
            return Err(LeadError::Assessment(
                "unsupported assessment schemaRevision".into(),
            ));
        }
        if self.project_id != input.project_id
            || self.action_id != input.action_id
            || self.request_id != input.request_id
        {
            return Err(LeadError::Assessment(
                "assessment identities do not match the bound request".into(),
            ));
        }
        if self.evidence_refs.is_empty() || self.evidence_refs.len() > MAX_EVIDENCE_REFS {
            return Err(LeadError::Assessment(
                "assessment must cite 1..=32 evidence references".into(),
            ));
        }
        unique_values("evidenceRefs", &self.evidence_refs)?;
        for reference in &self.evidence_refs {
            if !input.allowed_evidence_refs.contains(reference) {
                return Err(LeadError::Assessment(format!(
                    "assessment cites unavailable evidence {reference:?}"
                )));
            }
        }
        for (name, values) in [
            ("knowledgeGaps", &self.knowledge_gaps),
            ("changeImpact", &self.change_impact),
            ("validationNeeds", &self.validation_needs),
            ("assumptions", &self.assumptions),
            ("uncertainty", &self.uncertainty),
        ] {
            validate_notes(name, values)?;
        }
        if self.suggested_next_action.trim().is_empty()
            || self.suggested_next_action.chars().count() > MAX_NOTE_CHARS
        {
            return Err(LeadError::Assessment(
                "suggestedNextAction is required and bounded".into(),
            ));
        }
        self.effort_estimate.validate()?;
        let requires_estimate =
            self.intent == Intent::Change || requests_effort_estimate(&input.objective);
        if requires_estimate && self.effort_estimate.status == EffortStatus::NotRequested {
            return Err(LeadError::Assessment(
                "a change or explicit estimate request requires an effort estimate".into(),
            ));
        }
        if !requires_estimate && self.effort_estimate.status != EffortStatus::NotRequested {
            return Err(LeadError::Assessment(
                "effort must be not_requested when no estimate was requested".into(),
            ));
        }
        if self.intent != Intent::Change
            && let Some(proposal) = &self.proposal
            && !proposal.work_units.is_empty()
        {
            return Err(LeadError::Assessment(
                "a non-change intent cannot include executable WorkUnits".into(),
            ));
        }
        Ok(())
    }
}

/// Deterministic estimate-request trigger. This is intentionally a coarse
/// request marker, not a semantic classifier, and cannot substitute for model
/// understanding of an ambiguous objective.
fn visit<'a>(
    id: &'a str,
    by_id: &HashMap<&'a str, &'a LeadWorkUnit>,
    state: &mut HashMap<&'a str, u8>,
) -> Result<(), LeadError> {
    match state.get(id).copied() {
        Some(1) => {
            return Err(LeadError::WorkGraph(format!(
                "work graph contains a dependency cycle at {id:?}"
            )));
        }
        Some(2) => return Ok(()),
        _ => {}
    }
    state.insert(id, 1);
    if let Some(unit) = by_id.get(id) {
        for dependency in &unit.depends_on {
            visit(dependency.as_str(), by_id, state)?;
        }
    }
    state.insert(id, 2);
    Ok(())
}

pub fn requests_effort_estimate(objective: &str) -> bool {
    let text = objective.to_lowercase();
    text.contains("estimate")
        || text.contains("how long")
        || (text.contains("effort") && text.contains("range"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum LeadRequestState {
    Submitted,
    Assessing,
    Planned,
    Frozen,
    Ready,
    Running,
    Blocked,
    Failed,
}

impl LeadRequestState {
    pub fn can_transition(self, next: Self) -> bool {
        use LeadRequestState::*;
        matches!(
            (self, next),
            (Submitted, Assessing | Blocked)
                | (Assessing, Planned | Blocked | Failed)
                | (Planned, Frozen | Blocked)
                | (Frozen, Ready | Blocked)
                | (Ready, Running | Blocked)
                | (Running, Blocked | Failed)
        )
    }

    pub fn transition(self, next: Self) -> Result<Self, LeadError> {
        if self.can_transition(next) {
            Ok(next)
        } else {
            Err(LeadError::State(format!(
                "illegal Lead request transition {self:?} -> {next:?}"
            )))
        }
    }
}

/// Hash binding the proposal, delegation and project configuration revision.
pub fn plan_hash(
    proposal: &LeadPlanProposal,
    grant: &StandingDelegation,
    config_hash: &str,
) -> String {
    let envelope = serde_json::json!({
        "schema": "s4d-plan-v1",
        "proposal": proposal.content_hash(),
        "delegation": grant.content_hash,
        "projectConfig": config_hash,
    });
    let raw = serde_json::to_vec(&envelope).expect("plan envelope is serializable");
    sha256_hex(&raw)
}

fn validate_notes(name: &str, values: &[String]) -> Result<(), LeadError> {
    if values.len() > MAX_ASSUMPTIONS {
        return Err(LeadError::Field(format!(
            "{name} holds more than {MAX_ASSUMPTIONS} items"
        )));
    }
    for value in values {
        if value.trim().is_empty() || value.chars().count() > MAX_NOTE_CHARS {
            return Err(LeadError::Field(format!(
                "{name} entries must be nonempty and at most {MAX_NOTE_CHARS} characters"
            )));
        }
    }
    Ok(())
}

fn validate_checks(checks: &[SpecCheck], negative: bool) -> Result<(), LeadError> {
    if checks.len() > 32 {
        return Err(LeadError::WorkGraph(
            "a unit may declare at most 32 checks".into(),
        ));
    }
    let mut ids = HashSet::new();
    for check in checks {
        if check.id.trim().is_empty() || check.id.len() > 64 || !ids.insert(check.id.as_str()) {
            return Err(LeadError::WorkGraph(
                "check ids must be unique and bounded".into(),
            ));
        }
        if check.description.trim().is_empty() || check.description.chars().count() > 500 {
            return Err(LeadError::WorkGraph(
                "check description is required and bounded".into(),
            ));
        }
        if check.command.is_empty() || check.command[0].is_empty() {
            return Err(LeadError::WorkGraph(
                "check command must contain an executable".into(),
            ));
        }
        if check.command.len() > 64 || check.command.iter().any(|arg| arg.chars().count() > 4_000) {
            return Err(LeadError::WorkGraph("check argv is unbounded".into()));
        }
        if check.timeout_secs == 0 || check.timeout_secs > MAX_CHECK_TIMEOUT_SECS {
            return Err(LeadError::WorkGraph(
                "check timeout must be between 1 and 600 seconds".into(),
            ));
        }
        if !(0..=255).contains(&check.expect_exit) {
            return Err(LeadError::WorkGraph(
                "check expectExit must be between 0 and 255".into(),
            ));
        }
        if negative {
            if check.kind != CheckKind::Negative {
                return Err(LeadError::WorkGraph(
                    "negative cases use only the negative check kind".into(),
                ));
            }
            if check.expect_exit == 0 {
                return Err(LeadError::WorkGraph(
                    "negative checks must expect a nonzero exit".into(),
                ));
            }
        } else if check.kind == CheckKind::Negative {
            return Err(LeadError::WorkGraph(
                "negative check kind belongs in negativeCases".into(),
            ));
        }
    }
    Ok(())
}

/// Validate a mixed ActionSpec check list used by an independent verifier.
/// Positive checks may expect any bounded exit; negative checks must expect a
/// nonzero exit. This reuses the same shape rules as Lead proposal checks.
pub fn validate_mixed_checks(checks: &[SpecCheck]) -> Result<(), LeadError> {
    if checks.len() > 32 {
        return Err(LeadError::WorkGraph(
            "an ActionSpec may declare at most 32 checks".into(),
        ));
    }
    let mut ids = HashSet::new();
    for check in checks {
        if check.id.trim().is_empty() || check.id.len() > 64 || !ids.insert(check.id.as_str()) {
            return Err(LeadError::WorkGraph(
                "check ids must be unique and bounded".into(),
            ));
        }
    }
    let (negative, positive): (Vec<&SpecCheck>, Vec<&SpecCheck>) = checks
        .iter()
        .partition(|check| check.kind == CheckKind::Negative);
    let positive: Vec<SpecCheck> = positive.into_iter().cloned().collect();
    let negative: Vec<SpecCheck> = negative.into_iter().cloned().collect();
    validate_checks(&positive, false)?;
    validate_checks(&negative, true)?;
    Ok(())
}

fn require_check_kinds<'a>(
    checks: impl Iterator<Item = &'a SpecCheck>,
    required: &[CheckKind],
) -> Result<(), LeadError> {
    let present: BTreeSet<CheckKind> = checks.map(|check| check.kind).collect();
    for kind in required {
        if !present.contains(kind) {
            return Err(LeadError::WorkGraph(format!(
                "quality profile requires a {kind:?} check"
            )));
        }
    }
    Ok(())
}

pub fn validate_repository_paths(name: &str, paths: &[String]) -> Result<(), LeadError> {
    validate_paths(name, paths)
}

fn validate_paths(name: &str, paths: &[String]) -> Result<(), LeadError> {
    if paths.is_empty() {
        return Err(LeadError::Path(format!("{name} must not be empty")));
    }
    if paths.len() > 32 {
        return Err(LeadError::Path(format!("{name} holds too many paths")));
    }
    for path in paths {
        if !is_safe_relative_path(path) {
            return Err(LeadError::Path(format!(
                "{name} contains unsafe path {path:?}"
            )));
        }
    }
    let mut unique = HashSet::new();
    for path in paths {
        if !unique.insert(path) {
            return Err(LeadError::Path(format!(
                "{name} contains duplicate path {path:?}"
            )));
        }
    }
    Ok(())
}

pub fn is_safe_relative_path(path: &str) -> bool {
    if path.is_empty()
        || path.len() > 1_000
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path == ".git"
        || path.starts_with(".git/")
    {
        return false;
    }
    path.split('/')
        .all(|component| !component.is_empty() && component != ".." && component != ".")
}

fn covered_by(path: &str, granted: &[String]) -> bool {
    granted
        .iter()
        .any(|candidate| path == candidate || path.starts_with(&format!("{candidate}/")))
}

fn paths_overlap(left: &str, right: &str) -> bool {
    left == right
        || left.starts_with(&format!("{right}/"))
        || right.starts_with(&format!("{left}/"))
}

fn unique_values<T: PartialEq + std::fmt::Debug>(
    name: &str,
    values: &[T],
) -> Result<(), LeadError> {
    for (index, value) in values.iter().enumerate() {
        if values[index + 1..].contains(value) {
            return Err(LeadError::Duplicate(format!(
                "{name} contains duplicate value {value:?}"
            )));
        }
    }
    Ok(())
}

fn is_slug(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value.as_bytes()[value.len() - 1].is_ascii_alphanumeric()
}

pub fn is_git_object_id(value: &str) -> bool {
    is_hex(value, 40) || is_hex(value, 64)
}

fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha256_hex(raw: &[u8]) -> String {
    let digest = Sha256::digest(raw);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-23T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn project() -> ProjectBinding {
        ProjectBinding {
            id: "project-1".into(),
            active: true,
            config_hash: "a".repeat(64),
            allowed_paths: vec!["src".into(), "docs".into()],
            capabilities: vec![Capability::RepositoryEdit, Capability::GenericToolchain],
            quality_profile: QualityProfile::Generic,
        }
    }

    fn action() -> ActionBinding {
        ActionBinding {
            id: "action-1".into(),
            project_id: "project-1".into(),
            status: ActionStatus::Pending,
        }
    }

    fn grant() -> StandingDelegation {
        let mut grant = StandingDelegation {
            id: "grant-1".into(),
            principal_id: "owner-1".into(),
            audit_id: "audit-1".into(),
            project_id: "project-1".into(),
            revision: 1,
            content_hash: String::new(),
            objective_classes: vec![ObjectiveClass::Feature, ObjectiveClass::Maintenance],
            allowed_paths: vec!["src".into(), "docs".into()],
            capabilities: vec![Capability::RepositoryEdit, Capability::GenericToolchain],
            risk_classes: vec![RiskClass::R0, RiskClass::R1, RiskClass::R2],
            autonomous_start: true,
            autonomous_risk_classes: vec![RiskClass::R0],
            route: ProviderRoute {
                provider: "opencode".into(),
                model: "test/model".into(),
                mode: "build".into(),
            },
            max_attempts: 2,
            max_timeout_secs: 300,
            max_repair_cycles: 1,
            max_wall_clock_secs: 1_800,
            max_parallel: 2,
            max_sessions: 16,
            max_requests: 4,
            max_cumulative_sessions: 32,
            max_cumulative_attempts: 32,
            expires_at: now() + chrono::Duration::days(1),
            revoked_at: None,
        };
        grant.content_hash = grant.compute_content_hash();
        grant
    }

    fn checks() -> (Vec<SpecCheck>, Vec<SpecCheck>) {
        let acceptance = vec![
            SpecCheck {
                id: "format".into(),
                kind: CheckKind::Format,
                description: "format".into(),
                command: vec!["cargo".into(), "fmt".into(), "--check".into()],
                expect_exit: 0,
                timeout_secs: 60,
            },
            SpecCheck {
                id: "build".into(),
                kind: CheckKind::Build,
                description: "build".into(),
                command: vec!["cargo".into(), "build".into()],
                expect_exit: 0,
                timeout_secs: 120,
            },
            SpecCheck {
                id: "test".into(),
                kind: CheckKind::Test,
                description: "test".into(),
                command: vec!["cargo".into(), "test".into()],
                expect_exit: 0,
                timeout_secs: 120,
            },
            SpecCheck {
                id: "functional".into(),
                kind: CheckKind::Functional,
                description: "functional".into(),
                command: vec!["cargo".into(), "run".into()],
                expect_exit: 0,
                timeout_secs: 120,
            },
        ];
        let negative = vec![SpecCheck {
            id: "negative".into(),
            kind: CheckKind::Negative,
            description: "invalid input rejected".into(),
            command: vec!["cargo".into(), "run".into(), "--invalid".into()],
            expect_exit: 1,
            timeout_secs: 60,
        }];
        (acceptance, negative)
    }

    fn unit(id: &str) -> LeadWorkUnit {
        LeadWorkUnit {
            id: id.into(),
            kind: WorkUnitKind::Implementation,
            description: format!("Implement {id}"),
            depends_on: Vec::new(),
            allowed_paths: vec![format!("src/{id}")],
            capabilities: vec![Capability::RepositoryEdit, Capability::GenericToolchain],
            acceptance_criteria: Vec::new(),
            negative_cases: Vec::new(),
        }
    }

    fn proposal(units: Vec<LeadWorkUnit>) -> LeadPlanProposal {
        let (acceptance_criteria, negative_cases) = checks();
        LeadPlanProposal {
            project_id: "project-1".into(),
            action_id: "action-1".into(),
            base_revision: "b".repeat(40),
            project_config_hash: "a".repeat(64),
            objective: "Implement the bounded feature.".into(),
            class: ObjectiveClass::Feature,
            assumptions: vec!["The repository is clean.".into()],
            risks: vec!["Review scope.".into()],
            acceptance_criteria,
            negative_cases,
            quality_profile: QualityProfile::Generic,
            risk_class: RiskClass::R1,
            budget: SpecBudget {
                max_attempts: 2,
                timeout_secs: 300,
            },
            route: grant().route,
            max_repair_cycles: 1,
            max_wall_clock_secs: 1_800,
            max_parallel: 2,
            max_sessions: 12,
            work_units: units,
        }
    }

    fn assert_valid(value: &LeadPlanProposal) {
        value
            .validate(&grant(), &project(), &action(), now())
            .expect("valid proposal");
    }

    #[test]
    fn delegation_requires_explicit_autonomous_policy_and_immutable_hash() {
        let mut value = grant();
        assert!(value.validate(now()).is_ok());
        value.autonomous_start = false;
        assert!(value.validate(now()).is_err());
        value.autonomous_start = true;
        value.autonomous_risk_classes = vec![RiskClass::R2];
        assert!(value.validate(now()).is_err());
        let mut value = grant();
        value.autonomous_risk_classes.clear();
        value.autonomous_start = false;
        value.content_hash = value.compute_content_hash();
        assert!(value.validate(now()).is_ok());
        let mut tampered = grant();
        tampered.max_attempts = 4;
        assert!(tampered.validate(now()).is_err());
    }

    #[test]
    fn delegation_rejects_expired_revoked_and_unordered_limits() {
        let mut expired = grant();
        expired.expires_at = now();
        expired.content_hash = expired.compute_content_hash();
        assert!(expired.validate(now()).is_err());

        let mut revoked = grant();
        revoked.revoked_at = Some(now());
        assert!(revoked.validate(now()).is_err());

        let mut unordered = grant();
        unordered.max_cumulative_sessions = unordered.max_sessions - 1;
        unordered.content_hash = unordered.compute_content_hash();
        assert!(unordered.validate(now()).is_err());
    }

    #[test]
    fn delegation_rejects_unsafe_paths_and_routes() {
        let mut unsafe_path = grant();
        unsafe_path.allowed_paths = vec!["../escape".into()];
        unsafe_path.content_hash = unsafe_path.compute_content_hash();
        assert!(unsafe_path.validate(now()).is_err());

        let mut route = grant();
        route.route.provider = "paseo".into();
        route.content_hash = route.compute_content_hash();
        assert!(route.validate(now()).is_err());
    }

    #[test]
    fn proposal_accepts_sequential_parallel_and_fan_in_graphs() {
        let sequential = proposal(vec![unit("api"), {
            let mut second = unit("ui");
            second.depends_on = vec!["api".into()];
            second
        }]);
        assert_valid(&sequential);

        let parallel = proposal(vec![unit("api"), unit("ui")]);
        assert_valid(&parallel);

        let fan_in = proposal(vec![unit("api"), unit("ui"), {
            let mut wiring = unit("wiring");
            wiring.allowed_paths = vec!["src/wiring".into()];
            wiring.depends_on = vec!["api".into(), "ui".into()];
            wiring
        }]);
        assert_valid(&fan_in);
        assert!(!fan_in.needs_integration());
        assert_eq!(fan_in.terminal_units().len(), 1);
        let separate_terminals = proposal(vec![unit("api"), unit("ui")]);
        assert!(separate_terminals.needs_integration());
        assert_eq!(separate_terminals.terminal_units().len(), 2);
    }

    #[test]
    fn graph_rejects_invalid_shapes_scope_and_budget() {
        let mut cases: Vec<(&str, LeadPlanProposal)> = Vec::new();
        cases.push(("empty", proposal(Vec::new())));
        cases.push((
            "unknown dependency",
            proposal(vec![{
                let mut value = unit("api");
                value.depends_on = vec!["missing".into()];
                value
            }]),
        ));
        cases.push((
            "self dependency",
            proposal(vec![{
                let mut value = unit("api");
                value.depends_on = vec!["api".into()];
                value
            }]),
        ));
        cases.push((
            "cycle",
            proposal(vec![
                {
                    let mut api = unit("api");
                    api.depends_on = vec!["ui".into()];
                    api
                },
                {
                    let mut ui = unit("ui");
                    ui.depends_on = vec!["api".into()];
                    ui
                },
            ]),
        ));
        cases.push(("duplicate", proposal(vec![unit("api"), unit("api")])));
        cases.push((
            "parallel overlap",
            proposal(vec![
                {
                    let mut api = unit("api");
                    api.allowed_paths = vec!["src".into()];
                    api
                },
                {
                    let mut ui = unit("ui");
                    ui.allowed_paths = vec!["src/ui".into()];
                    ui
                },
            ]),
        ));
        cases.push((
            "outside grant",
            proposal(vec![{
                let mut api = unit("api");
                api.allowed_paths = vec!["outside".into()];
                api
            }]),
        ));
        cases.push((
            "capability expansion",
            proposal(vec![{
                let mut api = unit("api");
                api.capabilities.push(Capability::Browser);
                api
            }]),
        ));
        for (name, value) in cases {
            assert!(
                value
                    .validate(&grant(), &project(), &action(), now())
                    .is_err(),
                "{name} should be rejected"
            );
        }
    }

    #[test]
    fn graph_rejects_bad_ids_checks_and_session_escalation() {
        let mut bad_id = proposal(vec![unit("API")]);
        assert!(
            bad_id
                .validate(&grant(), &project(), &action(), now())
                .is_err()
        );
        bad_id = proposal(vec![unit("api")]);
        bad_id.negative_cases[0].expect_exit = 0;
        assert!(
            bad_id
                .validate(&grant(), &project(), &action(), now())
                .is_err()
        );
        let mut escalated = proposal(vec![unit("api")]);
        escalated.max_sessions = grant().max_sessions + 1;
        assert!(
            escalated
                .validate(&grant(), &project(), &action(), now())
                .is_err()
        );
        let mut missing_kind = proposal(vec![unit("api")]);
        missing_kind
            .acceptance_criteria
            .retain(|check| check.kind != CheckKind::Build);
        assert!(
            missing_kind
                .validate(&grant(), &project(), &action(), now())
                .is_err()
        );
    }

    #[test]
    fn proposal_rejects_binding_scope_and_route_violations() {
        let mut wrong_action = proposal(vec![unit("api")]);
        wrong_action.action_id = "action-2".into();
        assert!(
            wrong_action
                .validate(&grant(), &project(), &action(), now())
                .is_err()
        );

        let mut stale = proposal(vec![unit("api")]);
        stale.project_config_hash = "c".repeat(64);
        assert!(
            stale
                .validate(&grant(), &project(), &action(), now())
                .is_err()
        );

        let mut outside = proposal(vec![unit("api")]);
        outside.class = ObjectiveClass::Refactor;
        assert!(
            outside
                .validate(&grant(), &project(), &action(), now())
                .is_err()
        );

        let mut route = proposal(vec![unit("api")]);
        route.route.model = "other".into();
        assert!(
            route
                .validate(&grant(), &project(), &action(), now())
                .is_err()
        );

        let mut bad_base = proposal(vec![unit("api")]);
        bad_base.base_revision = "not-a-sha".into();
        assert!(
            bad_base
                .validate(&grant(), &project(), &action(), now())
                .is_err()
        );
    }

    fn assessment() -> LeadAssessment {
        LeadAssessment {
            schema_revision: 1,
            project_id: "project-1".into(),
            action_id: "action-1".into(),
            request_id: "request-1".into(),
            evidence_refs: vec!["repository-files".into()],
            intent: Intent::Change,
            complexity: Complexity::Small,
            effort_estimate: EffortEstimate {
                status: EffortStatus::Estimated,
                unit: "person-hours".into(),
                min_person_hours: 2,
                max_person_hours: 4,
                assumptions: vec!["One component.".into()],
                unknown_reason: String::new(),
            },
            knowledge_gaps: vec!["No runtime trace.".into()],
            change_impact: vec!["src/api".into()],
            validation_needs: vec!["unit and functional checks".into()],
            assumptions: vec!["Repository remains clean.".into()],
            uncertainty: vec!["External integration untested.".into()],
            suggested_next_action: "Freeze the bounded plan.".into(),
            proposal: Some(proposal(vec![unit("api")])),
        }
    }

    fn assessment_input() -> AssessmentInput {
        AssessmentInput {
            project_id: "project-1".into(),
            action_id: "action-1".into(),
            request_id: "request-1".into(),
            objective: "Implement the bounded feature.".into(),
            allowed_evidence_refs: BTreeSet::from(["repository-files".to_string()]),
        }
    }

    #[test]
    fn assessment_accepts_typed_evidence_and_estimate() {
        assessment().validate(&assessment_input()).unwrap();
    }

    #[test]
    fn assessment_rejects_strict_schema_and_identity_violations() {
        let input = assessment_input();
        let mut wrong = assessment();
        wrong.project_id = "other".into();
        assert!(wrong.validate(&input).is_err());

        let mut evidence = assessment();
        evidence.evidence_refs = vec!["not-supplied".into()];
        assert!(evidence.validate(&input).is_err());

        let mut duplicate = assessment();
        duplicate.evidence_refs.push("repository-files".into());
        assert!(duplicate.validate(&input).is_err());

        let mut effort = assessment();
        effort.effort_estimate.max_person_hours = effort.effort_estimate.min_person_hours;
        assert!(effort.validate(&input).is_err());

        let mut missing = assessment();
        missing.effort_estimate = EffortEstimate {
            status: EffortStatus::NotRequested,
            unit: String::new(),
            min_person_hours: 0,
            max_person_hours: 0,
            assumptions: Vec::new(),
            unknown_reason: String::new(),
        };
        assert!(missing.validate(&input).is_err());
    }

    #[test]
    fn assessment_rejects_unknown_fields_duplicate_keys_and_trailing_json() {
        let input = assessment_input();
        let base = serde_json::to_string(&assessment()).unwrap();
        let unknown = base.replacen('{', "{\"unknownField\":1,", 1);
        assert!(LeadAssessment::from_json(unknown.as_bytes(), &input).is_err());
        let duplicate = base.replacen('{', "{\"intent\":\"change\",", 1);
        assert!(LeadAssessment::from_json(duplicate.as_bytes(), &input).is_err());
        let trailing = format!("{base}{{}}");
        assert!(LeadAssessment::from_json(trailing.as_bytes(), &input).is_err());
    }

    #[test]
    fn non_change_intent_cannot_carry_executable_units() {
        let mut value = assessment();
        value.intent = Intent::Audit;
        value.proposal.as_mut().unwrap().work_units = vec![unit("api")];
        assert!(value.validate(&assessment_input()).is_err());
    }

    #[test]
    fn lead_state_machine_rejects_illegal_transitions() {
        use LeadRequestState::*;
        assert_eq!(Submitted.transition(Assessing).unwrap(), Assessing);
        assert_eq!(Assessing.transition(Planned).unwrap(), Planned);
        assert_eq!(Planned.transition(Frozen).unwrap(), Frozen);
        assert_eq!(Frozen.transition(Ready).unwrap(), Ready);
        assert_eq!(Ready.transition(Running).unwrap(), Running);
        assert!(Submitted.transition(Running).is_err());
        assert!(Blocked.transition(Running).is_err());
        assert!(Running.transition(Ready).is_err());
    }

    #[test]
    fn plan_hash_binds_proposal_grant_and_configuration() {
        let value = proposal(vec![unit("api")]);
        let grant = grant();
        let first = plan_hash(&value, &grant, &project().config_hash);
        assert_eq!(first, plan_hash(&value, &grant, &project().config_hash));
        assert_ne!(first, plan_hash(&value, &grant, &"c".repeat(64)));
        let mut changed = value.clone();
        changed.objective.push('!');
        assert_ne!(first, plan_hash(&changed, &grant, &project().config_hash));
    }

    #[test]
    fn notes_and_paths_are_bounded_and_safe() {
        for path in ["../escape", "/absolute", "a\\b", ".git", "a//b", "a/."] {
            assert!(!is_safe_relative_path(path), "{path}");
        }
        for path in ["src", "src/api", "docs/readme.md"] {
            assert!(is_safe_relative_path(path), "{path}");
        }
        assert!(is_git_object_id(&"a".repeat(40)));
        assert!(!is_git_object_id(&"g".repeat(40)));
    }
}
