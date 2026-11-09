//! Quality-first planning contracts and deterministic validation.
//!
//! These records are advisory inputs or frozen Core-owned artifacts. A model
//! may propose them; Core validates and freezes them. No record here grants
//! authority, tools, scope or delivery rights.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::lead::SpecCheck;

pub mod architecture;
pub mod docs;
pub mod dod;
pub mod prompts;
pub mod route;
pub mod skills;
pub mod stack;

pub const SCHEMA_VERSION: u32 = 1;
pub const R0_AUTONOMY_POINTS: u16 = 8;
pub const MAX_OPTIONS: usize = 8;
pub const MAX_TEXT_CHARS: usize = 4_000;
pub const MAX_SHORT_CHARS: usize = 500;
pub const MAX_LIST_ITEMS: usize = 64;
pub const MAX_REFS: usize = 16;
pub const MAX_CHECKS: usize = 32;
pub const MAX_DOCS: usize = 32;
pub const MAX_SKILLS: usize = 16;
pub const MAX_DEPENDENCIES: usize = 64;

fn bounded_text(name: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.chars().count() > max || value.chars().any(char::is_control)
    {
        return Err(format!("{name} is required, bounded and control-free"));
    }
    Ok(())
}

fn bounded_list(name: &str, values: &[String], max: usize) -> Result<(), String> {
    if values.len() > max {
        return Err(format!("{name} exceeds its bound"));
    }
    for value in values {
        bounded_text(name, value, MAX_SHORT_CHARS)?;
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_rfc3339(value: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(value).is_ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlanningLane {
    Direct,
    Delegated,
    FormalSpec,
    Repair,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PlanningSignals {
    pub objective: String,
    pub greenfield: bool,
    pub large: bool,
    pub missing_docs: bool,
    pub missing_stack: bool,
    pub architecture_uncertainty: bool,
    pub public_contract: bool,
    pub data_or_schema: bool,
    pub security_boundary: bool,
    pub cross_module: bool,
    pub changed_files: u32,
    pub expected_units: u32,
    pub explicit_repair: bool,
}

impl PlanningSignals {
    pub fn validate(&self) -> Result<(), String> {
        if self.objective.trim().is_empty() || self.objective.chars().count() > 4_000 {
            return Err("objective is required and bounded".into());
        }
        if self.expected_units > 24 {
            return Err("expected units exceed the supported bound".into());
        }
        Ok(())
    }
}

/// Deterministic lane floor. A model may raise this floor but never lower it.
pub fn deterministic_lane(signals: &PlanningSignals) -> Result<PlanningLane, String> {
    signals.validate()?;
    if signals.explicit_repair {
        return Ok(PlanningLane::Repair);
    }
    if signals.greenfield
        || signals.large
        || signals.missing_docs
        || signals.missing_stack
        || signals.architecture_uncertainty
        || signals.public_contract
        || signals.data_or_schema
        || signals.security_boundary
        || signals.cross_module
        || signals.changed_files > 5
        || signals.expected_units > 1
    {
        return Ok(PlanningLane::FormalSpec);
    }
    if signals.expected_units == 0 {
        return Ok(PlanningLane::Direct);
    }
    Ok(PlanningLane::Delegated)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlanReviewStatus {
    Pending,
    AutoApproved,
    OwnerApproved,
    ChangesRequested,
    Rejected,
}

impl PlanReviewStatus {
    pub fn is_approved(self) -> bool {
        matches!(self, Self::AutoApproved | Self::OwnerApproved)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PlanReview {
    pub request_id: String,
    pub plan_revision: u32,
    #[serde(default)]
    pub plan_sha256: String,
    pub status: PlanReviewStatus,
    pub principal_id: String,
    pub rationale: String,
    pub points: u16,
    pub reviewed_at: String,
}

impl PlanReview {
    pub fn validate(&self) -> Result<(), String> {
        if self.request_id.trim().is_empty()
            || self.plan_revision == 0
            || self.principal_id.trim().is_empty()
            || self.rationale.trim().is_empty()
            || self.reviewed_at.trim().is_empty()
        {
            return Err("plan review identity and rationale are required".into());
        }
        if !is_sha256(&self.plan_sha256) {
            return Err("plan review must bind the exact plan SHA-256".into());
        }
        if !is_rfc3339(&self.reviewed_at) {
            return Err("plan review timestamp must be RFC 3339".into());
        }
        if self.status == PlanReviewStatus::ChangesRequested && self.rationale.trim().is_empty() {
            return Err("a changes-requested review requires a rationale".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StackSource {
    User,
    Existing,
    Model,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StackOption {
    pub id: String,
    pub summary: String,
    #[schemars(range(min = 1, max = 5))]
    pub fit: u8,
    #[schemars(range(min = 1, max = 5))]
    pub security: u8,
    #[schemars(range(min = 1, max = 5))]
    pub maintainability: u8,
    #[schemars(range(min = 1, max = 5))]
    pub operability: u8,
    #[schemars(range(min = 1, max = 5))]
    pub time_to_value: u8,
    #[schemars(range(min = 1, max = 5))]
    pub exit_cost: u8,
    #[serde(default)]
    pub pros: Vec<String>,
    #[serde(default)]
    pub cons: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

impl StackOption {
    fn validate(&self) -> Result<(), String> {
        bounded_text("stack option id", &self.id, 64)?;
        bounded_text("stack option summary", &self.summary, MAX_SHORT_CHARS)?;
        bounded_list("stack option pros", &self.pros, MAX_LIST_ITEMS)?;
        bounded_list("stack option cons", &self.cons, MAX_LIST_ITEMS)?;
        bounded_list("stack option evidence", &self.evidence_refs, MAX_REFS)?;
        for (name, score) in [
            ("fit", self.fit),
            ("security", self.security),
            ("maintainability", self.maintainability),
            ("operability", self.operability),
            ("timeToValue", self.time_to_value),
            ("exitCost", self.exit_cost),
        ] {
            if !(1..=5).contains(&score) {
                return Err(format!("stack option {name} must be between 1 and 5"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DependencyStrategy {
    NotRequired,
    VendorCache,
    BrokeredFetch,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DependencyItem {
    pub name: String,
    pub version: String,
    pub source: String,
    pub license: String,
    #[serde(default)]
    pub checksum: String,
    #[serde(default)]
    pub advisory_status: String,
    #[serde(default)]
    pub sbom_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DependencyPlan {
    pub strategy: DependencyStrategy,
    #[serde(default)]
    pub items: Vec<DependencyItem>,
    #[serde(default)]
    pub cache_ref: String,
    #[serde(default)]
    pub sbom_ref: String,
    #[serde(default)]
    pub owner_approval: Option<String>,
}

impl DependencyPlan {
    pub fn validate(&self) -> Result<(), String> {
        if self.items.len() > MAX_DEPENDENCIES {
            return Err("dependency plan exceeds its item bound".into());
        }
        for item in &self.items {
            bounded_text("dependency name", &item.name, 200)?;
            bounded_text("dependency version", &item.version, 100)?;
            bounded_text("dependency source", &item.source, MAX_SHORT_CHARS)?;
            bounded_text("dependency license", &item.license, 200)?;
            if !item.checksum.is_empty() && !is_sha256(&item.checksum) {
                return Err("dependency checksum must be SHA-256".into());
            }
        }
        if matches!(self.strategy, DependencyStrategy::NotRequired) && !self.items.is_empty() {
            return Err("a not-required dependency plan cannot list dependencies".into());
        }
        if matches!(
            self.strategy,
            DependencyStrategy::VendorCache | DependencyStrategy::Both
        ) && self.cache_ref.trim().is_empty()
        {
            return Err("vendor/cache dependency plans require a cache reference".into());
        }
        if !self.sbom_ref.is_empty() {
            bounded_text("dependency SBOM reference", &self.sbom_ref, MAX_SHORT_CHARS)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StackDecision {
    pub schema_version: u32,
    pub source: StackSource,
    #[serde(default)]
    pub constraints: Vec<String>,
    pub options: Vec<StackOption>,
    pub selected: String,
    pub reason: String,
    pub confidence: String,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub dependency_plan: DependencyPlan,
    pub stack_change: bool,
    #[serde(default)]
    pub autonomy_points: u16,
    #[serde(default)]
    pub owner_approval: Option<String>,
    #[serde(default)]
    pub adr_path: String,
}

impl StackDecision {
    pub fn validate(&self, lane: PlanningLane) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err("unsupported stack decision schema version".into());
        }
        if self.options.is_empty() || self.options.len() > MAX_OPTIONS {
            return Err("stack decision options are missing or exceed the bound".into());
        }
        if self.options.len() < 2
            && lane == PlanningLane::FormalSpec
            && self.source == StackSource::Model
        {
            return Err("a formal model-selected stack requires at least two options".into());
        }
        bounded_list("stack constraints", &self.constraints, MAX_LIST_ITEMS)?;
        bounded_list(
            "stack required capabilities",
            &self.required_capabilities,
            MAX_LIST_ITEMS,
        )?;
        bounded_text("stack reason", &self.reason, MAX_TEXT_CHARS)?;
        bounded_text("stack confidence", &self.confidence, 32)?;
        bounded_text("stack ADR path", &self.adr_path, MAX_SHORT_CHARS)?;
        let mut ids = BTreeSet::new();
        for option in &self.options {
            option.validate()?;
            if !ids.insert(option.id.as_str()) {
                return Err("duplicate stack option id".into());
            }
        }
        if !ids.contains(self.selected.as_str()) {
            return Err("selected stack is not one of the proposed options".into());
        }
        if self.reason.trim().is_empty()
            || self.confidence.trim().is_empty()
            || self.required_capabilities.is_empty()
            || self.adr_path.trim().is_empty()
        {
            return Err(
                "stack decision reason, confidence, capabilities and ADR are required".into(),
            );
        }
        self.dependency_plan.validate()?;
        if self.stack_change
            && self.autonomy_points > R0_AUTONOMY_POINTS
            && self.owner_approval.as_deref().is_none_or(str::is_empty)
        {
            return Err("a stack change above the R0 point budget requires owner approval".into());
        }
        Ok(())
    }
}

impl Default for DependencyPlan {
    fn default() -> Self {
        Self {
            strategy: DependencyStrategy::NotRequired,
            items: Vec::new(),
            cache_ref: String::new(),
            sbom_ref: String::new(),
            owner_approval: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ArchitecturePlan {
    pub context: String,
    #[serde(default)]
    pub components: Vec<String>,
    #[serde(default)]
    pub boundaries: Vec<String>,
    #[serde(default)]
    pub interfaces: Vec<String>,
    #[serde(default)]
    pub data: Vec<String>,
    #[serde(default)]
    pub deployment: Vec<String>,
    #[serde(default)]
    pub quality_attributes: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
    #[serde(default)]
    pub tradeoffs: Vec<String>,
    #[serde(default)]
    pub adr_paths: Vec<String>,
}

impl ArchitecturePlan {
    pub fn validate(&self) -> Result<(), String> {
        bounded_text("architecture context", &self.context, MAX_TEXT_CHARS)?;
        if self.components.is_empty() || self.components.len() > MAX_LIST_ITEMS {
            return Err("architecture requires bounded components".into());
        }
        bounded_list("architecture components", &self.components, MAX_LIST_ITEMS)?;
        bounded_list("architecture boundaries", &self.boundaries, MAX_LIST_ITEMS)?;
        bounded_list("architecture interfaces", &self.interfaces, MAX_LIST_ITEMS)?;
        bounded_list("architecture data", &self.data, MAX_LIST_ITEMS)?;
        bounded_list("architecture deployment", &self.deployment, MAX_LIST_ITEMS)?;
        bounded_list(
            "architecture quality attributes",
            &self.quality_attributes,
            MAX_LIST_ITEMS,
        )?;
        bounded_list("architecture risks", &self.risks, MAX_LIST_ITEMS)?;
        bounded_list("architecture tradeoffs", &self.tradeoffs, MAX_LIST_ITEMS)?;
        bounded_list("architecture ADR paths", &self.adr_paths, MAX_LIST_ITEMS)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DocumentationItem {
    pub path: String,
    pub purpose: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub check_command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DocumentationPlan {
    #[serde(default)]
    pub existing: Vec<String>,
    #[serde(default)]
    pub required: Vec<DocumentationItem>,
    #[serde(default)]
    pub owner: String,
}

impl DocumentationPlan {
    pub fn validate(&self) -> Result<(), String> {
        if self.required.len() > MAX_DOCS || self.existing.len() > MAX_DOCS {
            return Err("documentation plan exceeds its item bound".into());
        }
        bounded_list("existing documentation paths", &self.existing, MAX_DOCS)?;
        for item in &self.required {
            bounded_text("documentation path", &item.path, MAX_SHORT_CHARS)?;
            bounded_text("documentation purpose", &item.purpose, MAX_TEXT_CHARS)?;
            if item.required && item.check_command.is_empty() {
                return Err("required documentation needs a deterministic check command".into());
            }
            if item.check_command.len() > MAX_LIST_ITEMS {
                return Err("documentation check command exceeds its bound".into());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SkillPack {
    pub technology: String,
    pub version: String,
    #[serde(default)]
    pub source_urls: Vec<String>,
    pub last_verified_at: String,
    pub review_after: String,
    #[serde(default)]
    pub best_practices: Vec<String>,
    #[serde(default)]
    pub anti_patterns: Vec<String>,
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub security_notes: Vec<String>,
    #[serde(default)]
    pub deprecations: Vec<String>,
    pub license: String,
    #[serde(default)]
    pub provenance: String,
    #[serde(default)]
    pub content_sha256: String,
}

impl SkillPack {
    pub fn validate(&self) -> Result<(), String> {
        bounded_text("skill technology", &self.technology, 200)?;
        bounded_text("skill version", &self.version, 100)?;
        bounded_text("skill license", &self.license, 200)?;
        if self.source_urls.is_empty() || self.source_urls.len() > MAX_REFS {
            return Err("skill pack requires bounded official sources".into());
        }
        for url in &self.source_urls {
            bounded_text("skill source URL", url, MAX_SHORT_CHARS)?;
            if !url.starts_with("https://") {
                return Err("skill source URLs must use HTTPS".into());
            }
        }
        let last_verified = chrono::DateTime::parse_from_rfc3339(&self.last_verified_at)
            .map_err(|_| "skill last_verified_at must be RFC 3339")?;
        let review_after = chrono::DateTime::parse_from_rfc3339(&self.review_after)
            .map_err(|_| "skill review_after must be RFC 3339")?;
        if review_after <= last_verified {
            return Err("skill pack review_after must be after last_verified_at".into());
        }
        bounded_list("skill best practices", &self.best_practices, MAX_LIST_ITEMS)?;
        bounded_list("skill anti-patterns", &self.anti_patterns, MAX_LIST_ITEMS)?;
        bounded_list("skill commands", &self.commands, MAX_LIST_ITEMS)?;
        bounded_list("skill security notes", &self.security_notes, MAX_LIST_ITEMS)?;
        bounded_list("skill deprecations", &self.deprecations, MAX_LIST_ITEMS)?;
        if self.best_practices.is_empty() {
            return Err("skill pack requires best practices".into());
        }
        Ok(())
    }

    pub fn is_stale(&self, now: &str) -> bool {
        let (Ok(now), Ok(review_after)) = (
            chrono::DateTime::parse_from_rfc3339(now),
            chrono::DateTime::parse_from_rfc3339(&self.review_after),
        ) else {
            return true;
        };
        now >= review_after
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SkillPlan {
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub existing: Vec<String>,
    #[serde(default)]
    pub proposed: Vec<SkillPack>,
    #[serde(default)]
    pub owner: String,
}

impl SkillPlan {
    pub fn validate(&self) -> Result<(), String> {
        if self.proposed.len() > MAX_SKILLS {
            return Err("skill plan exceeds its proposed-skill bound".into());
        }
        bounded_list("skill needs", &self.needs, MAX_LIST_ITEMS)?;
        bounded_list("existing skills", &self.existing, MAX_LIST_ITEMS)?;
        for pack in &self.proposed {
            pack.validate()?;
        }
        Ok(())
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewerRole {
    Functional,
    Architecture,
    CodeQuality,
    Security,
    Testing,
    Operability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReviewerPlan {
    pub roles: Vec<ReviewerRole>,
    #[serde(default)]
    pub required: Vec<ReviewerRole>,
    #[serde(default)]
    pub independent_principals: bool,
}

impl ReviewerPlan {
    pub fn for_lane(lane: PlanningLane, elevated: bool) -> Self {
        let required = match lane {
            PlanningLane::Direct => vec![ReviewerRole::Functional, ReviewerRole::CodeQuality],
            PlanningLane::Delegated => vec![
                ReviewerRole::Functional,
                ReviewerRole::Architecture,
                ReviewerRole::CodeQuality,
                ReviewerRole::Testing,
            ],
            PlanningLane::FormalSpec => vec![
                ReviewerRole::Functional,
                ReviewerRole::Architecture,
                ReviewerRole::CodeQuality,
                ReviewerRole::Security,
                ReviewerRole::Testing,
            ],
            PlanningLane::Repair => vec![ReviewerRole::Functional, ReviewerRole::CodeQuality],
        };
        let mut roles = required.clone();
        if elevated && !roles.contains(&ReviewerRole::Operability) {
            roles.push(ReviewerRole::Operability);
        }
        Self {
            roles,
            required,
            independent_principals: true,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for role in &self.roles {
            if !seen.insert(*role) {
                return Err("duplicate reviewer role".into());
            }
        }
        for role in &self.required {
            if !seen.contains(role) {
                return Err("required reviewer role is not in the plan".into());
            }
        }
        let mut required = BTreeSet::new();
        for role in &self.required {
            if !required.insert(*role) {
                return Err("duplicate required reviewer role".into());
            }
        }
        if self.required.is_empty() || !self.independent_principals {
            return Err("reviewer plan needs required roles and independent principals".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FindingRoute {
    Repairer,
    TestPlanner,
    Planner,
    SpecManager,
    SecurityReviewer,
    Documentation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FindingStatus {
    Open,
    Resolved,
    Refuted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FindingDisposition {
    pub finding_id: String,
    pub route: FindingRoute,
    pub status: FindingStatus,
    #[serde(default)]
    pub candidate_id: String,
    #[serde(default)]
    pub reviewer_principal: String,
    #[serde(default)]
    pub resolved_by: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub owner_principal: Option<String>,
    #[serde(default)]
    pub owner_decision: Option<String>,
}

impl FindingDisposition {
    pub fn validate(&self) -> Result<(), String> {
        bounded_text("finding disposition id", &self.finding_id, 128)?;
        if !self.candidate_id.is_empty() {
            bounded_text("finding candidate id", &self.candidate_id, 128)?;
        }
        if self.evidence_refs.len() > MAX_REFS {
            return Err("finding disposition evidence exceeds its bound".into());
        }
        for reference in &self.evidence_refs {
            bounded_text("finding evidence reference", reference, MAX_SHORT_CHARS)?;
        }
        match self.status {
            FindingStatus::Open => {}
            FindingStatus::Resolved => {
                bounded_text("finding resolver", &self.resolved_by, 128)?;
                if self.evidence_refs.is_empty() {
                    return Err("a resolved finding requires evidence references".into());
                }
            }
            FindingStatus::Refuted => {
                let Some(owner) = self.owner_principal.as_deref() else {
                    return Err(
                        "a refuted finding requires an authenticated owner principal".into(),
                    );
                };
                let Some(decision) = self.owner_decision.as_deref() else {
                    return Err("a refuted finding requires an owner decision reference".into());
                };
                bounded_text("finding owner principal", owner, 128)?;
                bounded_text("finding owner decision", decision, 128)?;
                bounded_text("finding reviewer principal", &self.reviewer_principal, 128)?;
                if owner == self.reviewer_principal {
                    return Err("a reviewer cannot waive its own finding".into());
                }
                if self.evidence_refs.is_empty() {
                    return Err("a refuted finding requires evidence references".into());
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DefinitionOfDone {
    #[serde(default)]
    pub project_checks: Vec<SpecCheck>,
    #[serde(default)]
    pub unit_checks: Vec<SpecCheck>,
    #[serde(default)]
    pub documentation: DocumentationPlan,
    #[serde(default)]
    pub reviewer_plan: ReviewerPlan,
    #[serde(default)]
    pub all_findings_block: bool,
}

impl DefinitionOfDone {
    pub fn validate(&self) -> Result<(), String> {
        if self.project_checks.is_empty() || self.project_checks.len() > MAX_CHECKS {
            return Err("definition of done requires bounded project checks".into());
        }
        if self.unit_checks.len() > MAX_CHECKS {
            return Err("definition of done unit checks exceed the bound".into());
        }
        for check in self.project_checks.iter().chain(self.unit_checks.iter()) {
            bounded_text("DoD check id", &check.id, 128)?;
            if check.command.is_empty() || check.timeout_secs == 0 {
                return Err("DoD checks require a command and timeout".into());
            }
        }
        self.documentation.validate()?;
        self.reviewer_plan.validate()?;
        if !self.all_findings_block {
            return Err("the current policy requires every finding to block".into());
        }
        Ok(())
    }
}

impl Default for ReviewerPlan {
    fn default() -> Self {
        Self {
            roles: Vec::new(),
            required: Vec::new(),
            independent_principals: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OpenSpecProjection {
    pub change_id: String,
    pub revision: u32,
    pub proposal_sha256: String,
    pub specs_sha256: String,
    #[serde(default)]
    pub design_sha256: String,
    pub tasks_sha256: String,
    #[serde(default)]
    pub validated: bool,
    #[serde(default)]
    pub archived: bool,
}

impl OpenSpecProjection {
    pub fn validate(&self) -> Result<(), String> {
        bounded_text("OpenSpec change id", &self.change_id, 64)?;
        if self.revision == 0 {
            return Err("OpenSpec revision is required".into());
        }
        for (name, hash) in [
            ("proposal", &self.proposal_sha256),
            ("specs", &self.specs_sha256),
            ("tasks", &self.tasks_sha256),
        ] {
            if !is_sha256(hash) {
                return Err(format!("OpenSpec {name} hash must be SHA-256"));
            }
        }
        if !self.design_sha256.is_empty() && !is_sha256(&self.design_sha256) {
            return Err("OpenSpec design hash must be SHA-256".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkflowState {
    Submitted,
    Assessing,
    Planned,
    PlanReview,
    Frozen,
    Executing,
    Verifying,
    Reviewing,
    Gate,
    Repairing,
    Replanning,
    Converging,
    Accepted,
    Blocked,
    Failed,
    Cancelled,
}

impl WorkflowState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Accepted | Self::Blocked | Self::Failed | Self::Cancelled
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkflowStepStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Blocked,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack_option(id: &str) -> StackOption {
        StackOption {
            id: id.into(),
            summary: "candidate".into(),
            fit: 4,
            security: 4,
            maintainability: 4,
            operability: 4,
            time_to_value: 4,
            exit_cost: 4,
            pros: vec!["simple".into()],
            cons: vec!["bounded".into()],
            evidence_refs: vec!["Cargo.toml".into()],
        }
    }

    fn decision() -> StackDecision {
        StackDecision {
            schema_version: SCHEMA_VERSION,
            source: StackSource::Model,
            constraints: vec!["container-native".into()],
            options: vec![stack_option("rust-std"), stack_option("rust-axum")],
            selected: "rust-std".into(),
            reason: "bounded CLI".into(),
            confidence: "high".into(),
            required_capabilities: vec!["generic-toolchain".into()],
            dependency_plan: DependencyPlan::default(),
            stack_change: false,
            autonomy_points: 2,
            owner_approval: None,
            adr_path: "docs/adr/0001-stack.md".into(),
        }
    }

    #[test]
    fn formal_model_stack_requires_two_options_and_selected_id() {
        assert!(decision().validate(PlanningLane::FormalSpec).is_ok());
        let mut single = decision();
        single.options = vec![stack_option("rust-std")];
        assert!(single.validate(PlanningLane::FormalSpec).is_err());
        let mut missing = decision();
        missing.selected = "missing".into();
        assert!(missing.validate(PlanningLane::FormalSpec).is_err());
    }

    #[test]
    fn stack_change_above_budget_requires_owner_approval() {
        let mut change = decision();
        change.stack_change = true;
        change.autonomy_points = R0_AUTONOMY_POINTS + 1;
        assert!(change.validate(PlanningLane::FormalSpec).is_err());
        change.owner_approval = Some("owner".into());
        assert!(change.validate(PlanningLane::FormalSpec).is_ok());
    }

    #[test]
    fn greenfield_and_missing_docs_force_formal_spec() {
        let signals = PlanningSignals {
            objective: "build a gym manager".into(),
            greenfield: true,
            large: false,
            missing_docs: true,
            missing_stack: true,
            architecture_uncertainty: false,
            public_contract: false,
            data_or_schema: false,
            security_boundary: false,
            cross_module: false,
            changed_files: 0,
            expected_units: 1,
            explicit_repair: false,
        };
        assert_eq!(
            deterministic_lane(&signals).unwrap(),
            PlanningLane::FormalSpec
        );
    }

    #[test]
    fn direct_is_only_for_bounded_existing_work() {
        let signals = PlanningSignals {
            objective: "fix a typo".into(),
            greenfield: false,
            large: false,
            missing_docs: false,
            missing_stack: false,
            architecture_uncertainty: false,
            public_contract: false,
            data_or_schema: false,
            security_boundary: false,
            cross_module: false,
            changed_files: 1,
            expected_units: 0,
            explicit_repair: false,
        };
        assert_eq!(deterministic_lane(&signals).unwrap(), PlanningLane::Direct);
    }

    #[test]
    fn reviewer_plan_requires_unique_required_roles() {
        let plan = ReviewerPlan::for_lane(PlanningLane::FormalSpec, true);
        assert!(plan.validate().is_ok());
        let mut invalid = plan.clone();
        invalid.required.push(ReviewerRole::Security);
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn closed_findings_need_evidence_and_stale_skill_blocks() {
        let finding = FindingDisposition {
            finding_id: "f1".into(),
            route: FindingRoute::Repairer,
            status: FindingStatus::Resolved,
            candidate_id: "candidate-2".into(),
            reviewer_principal: "reviewer-code-quality".into(),
            resolved_by: "candidate-2".into(),
            evidence_refs: vec!["receipt-1".into()],
            owner_principal: None,
            owner_decision: None,
        };
        assert!(finding.validate().is_ok());
        let mut missing = finding;
        missing.evidence_refs.clear();
        assert!(missing.validate().is_err());

        let pack = SkillPack {
            technology: "rust".into(),
            version: "1.90".into(),
            source_urls: vec!["https://doc.rust-lang.org/".into()],
            last_verified_at: "2026-11-01T00:00:00Z".into(),
            review_after: "2026-12-01T00:00:00Z".into(),
            best_practices: vec!["use cargo test".into()],
            anti_patterns: vec![],
            commands: vec![],
            security_notes: vec![],
            deprecations: vec![],
            license: "CC-BY-4.0".into(),
            provenance: "official".into(),
            content_sha256: "a".repeat(64),
        };
        assert!(pack.validate().is_ok());
        assert!(pack.is_stale("2026-12-02T00:00:00Z"));
    }
}
