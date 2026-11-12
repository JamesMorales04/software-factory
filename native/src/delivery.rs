//! Core-owned risk-tiered delivery and auto-merge.
//!
//! Quality Gate acceptance is necessary but never sufficient for a repository
//! write. This module owns the deterministic delivery policy, the exact
//! candidate/head/base/policy binding, fresh pull-request/check/protection
//! reads, idempotent attempts, typed failures, post-merge checks and incidents.
//!
//! The GitHub write credential is passed only to this Core service. It is never
//! placed in a branch name, commit, pull-request body, Office response, log or
//! evidence record.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::{
    CandidateRecord, Database, DeliveryAttemptUpdate, Error as DbError, NewDeliveryAttempt,
    NewDeliveryAttemptEvent, NewDeliveryDecision, NewDeliveryIncident, Project,
    ProjectGitHubBinding,
};
use crate::github::{
    GitHubBranchProtection, GitHubClient, GitHubError, GitHubMergeMethod, GitHubPullRequest,
    GitHubPullRequestChecks, MergePullRequest, NewPullRequest,
};
use crate::github_sync::GitHubCredential;
use crate::issues::{IssueDeliveryError, IssueDeliveryOutcome};
use crate::planning::ReviewerRole;
use crate::quality::{CandidateFinalSource, GateOutcome, ReceiptResult};
use crate::workspace::verify_candidate_commit;

pub const FRESHNESS_WINDOW_SECS: i64 = 120;
pub const MAX_REQUIRED_CHECKS: usize = 50;
pub const MAX_POST_MERGE_TIMEOUT_SECS: u64 = 1_800;
pub const MERGE_METHOD: &str = "squash";
const MAX_CHECK_NAME_CHARS: usize = 256;
const MAX_POLICY_BRANCH_CHARS: usize = 255;
const POST_MERGE_POLL_INTERVAL: Duration = Duration::from_millis(200);
const MAX_PUSH_STDERR_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeliveryMode {
    Manual,
    RiskTiered,
}

impl DeliveryMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "MANUAL",
            Self::RiskTiered => "RISK_TIERED",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "MANUAL" => Some(Self::Manual),
            "RISK_TIERED" => Some(Self::RiskTiered),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeliveryRisk {
    R0,
    R1,
    R2,
    R3,
    Unknown,
}

impl DeliveryRisk {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::R0 => "R0",
            Self::R1 => "R1",
            Self::R2 => "R2",
            Self::R3 => "R3",
            Self::Unknown => "UNKNOWN",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "R0" => Some(Self::R0),
            "R1" => Some(Self::R1),
            "R2" => Some(Self::R2),
            "R3" => Some(Self::R3),
            "UNKNOWN" | "" => Some(Self::Unknown),
            _ => None,
        }
    }

    pub fn is_auto_tier(self) -> bool {
        matches!(self, Self::R0 | Self::R1)
    }
}

impl From<crate::lead::RiskClass> for DeliveryRisk {
    fn from(value: crate::lead::RiskClass) -> Self {
        match value {
            crate::lead::RiskClass::R0 => Self::R0,
            crate::lead::RiskClass::R1 => Self::R1,
            crate::lead::RiskClass::R2 => Self::R2,
            crate::lead::RiskClass::R3 => Self::R3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeliveryAuthorization {
    PolicyR0,
    CharterR1,
    OwnerManual,
}

impl DeliveryAuthorization {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PolicyR0 => "POLICY_R0",
            Self::CharterR1 => "CHARTER_R1",
            Self::OwnerManual => "OWNER_MANUAL",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "POLICY_R0" => Some(Self::PolicyR0),
            "CHARTER_R1" => Some(Self::CharterR1),
            "OWNER_MANUAL" => Some(Self::OwnerManual),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeliveryAttemptState {
    Pending,
    Authorized,
    Merging,
    Merged,
    Blocked,
    NeedsAttention,
    Reverted,
}

impl DeliveryAttemptState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Authorized => "AUTHORIZED",
            Self::Merging => "MERGING",
            Self::Merged => "MERGED",
            Self::Blocked => "BLOCKED",
            Self::NeedsAttention => "NEEDS_ATTENTION",
            Self::Reverted => "REVERTED",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "PENDING" => Some(Self::Pending),
            "AUTHORIZED" => Some(Self::Authorized),
            "MERGING" => Some(Self::Merging),
            "MERGED" => Some(Self::Merged),
            "BLOCKED" => Some(Self::Blocked),
            "NEEDS_ATTENTION" => Some(Self::NeedsAttention),
            "REVERTED" => Some(Self::Reverted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeliveryIncidentState {
    Open,
    Acknowledged,
    RevertPending,
    Reverted,
    Closed,
}

impl DeliveryIncidentState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::Acknowledged => "ACKNOWLEDGED",
            Self::RevertPending => "REVERT_PENDING",
            Self::Reverted => "REVERTED",
            Self::Closed => "CLOSED",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "OPEN" => Some(Self::Open),
            "ACKNOWLEDGED" => Some(Self::Acknowledged),
            "REVERT_PENDING" => Some(Self::RevertPending),
            "REVERTED" => Some(Self::Reverted),
            "CLOSED" => Some(Self::Closed),
            _ => None,
        }
    }

    pub fn freezes_auto_merge(self) -> bool {
        matches!(self, Self::Open | Self::Acknowledged | Self::RevertPending)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryPolicy {
    pub schema_version: u32,
    pub project_id: String,
    pub revision: u32,
    pub policy_sha256: String,
    pub mode: DeliveryMode,
    pub r1_enabled: bool,
    pub auto_merge_base_branch: String,
    pub manual_base_branches: Vec<String>,
    pub required_checks: Vec<String>,
    pub merge_method: String,
    pub post_merge_check_required: bool,
    pub post_merge_check_id: String,
    pub post_merge_timeout_secs: u64,
    pub kill_switch: bool,
    pub created_by: String,
    pub created_at: String,
    pub expires_at: Option<String>,
}

impl DeliveryPolicy {
    pub fn auto_merge_enabled(&self, now: DateTime<Utc>) -> bool {
        self.mode == DeliveryMode::RiskTiered
            && !self.kill_switch
            && self
                .expires_at
                .as_deref()
                .is_none_or(|expires| !expired(expires, now))
    }

    pub fn auto_merge_enabled_for(&self, risk: DeliveryRisk, now: DateTime<Utc>) -> bool {
        self.auto_merge_enabled(now)
            && match risk {
                DeliveryRisk::R0 => true,
                DeliveryRisk::R1 => self.r1_enabled,
                DeliveryRisk::R2 | DeliveryRisk::R3 | DeliveryRisk::Unknown => false,
            }
    }
}

#[derive(Debug, Clone)]
pub struct DeliveryPolicyInput {
    pub enabled: bool,
    pub r1_enabled: bool,
    pub auto_merge_base_branch: String,
    pub manual_base_branches: Vec<String>,
    pub required_checks: Vec<String>,
    pub merge_method: String,
    pub post_merge_check_id: String,
    pub post_merge_timeout_secs: u64,
    pub kill_switch: bool,
    pub expires_at: Option<String>,
}

impl DeliveryPolicyInput {
    pub fn validate(&self, binding_default_branch: &str) -> Result<(), String> {
        if self.r1_enabled && !self.enabled {
            return Err("R1 auto-merge requires the risk-tiered policy to be enabled".into());
        }
        if self.merge_method != MERGE_METHOD {
            return Err("delivery merge method must be squash".into());
        }
        if self.auto_merge_base_branch != binding_default_branch {
            return Err("auto-merge base branch must equal the protected default branch".into());
        }
        if self.auto_merge_base_branch.trim().is_empty()
            || self.auto_merge_base_branch.len() > MAX_POLICY_BRANCH_CHARS
        {
            return Err("auto-merge base branch is invalid".into());
        }
        if self.enabled && self.required_checks.is_empty() {
            return Err("risk-tiered delivery requires at least one required check".into());
        }
        if self.required_checks.len() > MAX_REQUIRED_CHECKS {
            return Err(format!(
                "at most {MAX_REQUIRED_CHECKS} required checks may be configured"
            ));
        }
        for check in &self.required_checks {
            if check.trim().is_empty()
                || check.len() > MAX_CHECK_NAME_CHARS
                || check.contains('\n')
                || check.contains('\r')
            {
                return Err("required check names must be 1-256 single-line characters".into());
            }
        }
        if self.post_merge_check_id.trim().is_empty()
            || self.post_merge_check_id.len() > MAX_CHECK_NAME_CHARS
            || self.post_merge_check_id.contains('\n')
            || self.post_merge_check_id.contains('\r')
        {
            return Err(
                "delivery requires a bounded post-merge check id; every R1 merge must run it"
                    .into(),
            );
        }
        if !(1..=MAX_POST_MERGE_TIMEOUT_SECS).contains(&self.post_merge_timeout_secs) {
            return Err(format!(
                "post-merge timeout must be between 1 and {MAX_POST_MERGE_TIMEOUT_SECS} seconds"
            ));
        }
        if self.manual_base_branches.len() > 20
            || self
                .manual_base_branches
                .iter()
                .any(|branch| branch.trim().is_empty() || branch.len() > MAX_POLICY_BRANCH_CHARS)
        {
            return Err("manual base branches are invalid".into());
        }
        if let Some(expires) = self.expires_at.as_deref()
            && DateTime::parse_from_rfc3339(expires).is_err()
        {
            return Err("policy expiry must be RFC3339".into());
        }
        Ok(())
    }
}

pub fn policy_sha256(
    project_id: &str,
    revision: u32,
    mode: DeliveryMode,
    input: &DeliveryPolicyInput,
) -> String {
    let payload = serde_json::json!({
        "schemaVersion": 1,
        "projectId": project_id,
        "revision": revision,
        "mode": mode.as_str(),
        "r1Enabled": input.r1_enabled,
        "autoMergeBaseBranch": input.auto_merge_base_branch,
        "manualBaseBranches": input.manual_base_branches,
        "requiredChecks": input.required_checks,
        "mergeMethod": input.merge_method,
        "postMergeCheckRequired": true,
        "postMergeCheckId": input.post_merge_check_id,
        "postMergeTimeoutSecs": input.post_merge_timeout_secs,
        "killSwitch": input.kill_switch,
        "expiresAt": input.expires_at,
    });
    sha256_hex(payload.to_string().as_bytes())
}

pub fn content_digest(project_id: &str, campaign_id: &str, candidate: &CandidateRecord) -> String {
    let payload = serde_json::json!({
        "projectId": project_id,
        "campaignId": campaign_id,
        "candidateId": candidate.id,
        "commitSha": candidate.commit_sha,
        "treeSha": candidate.tree_sha,
        "actionSpecSha256": candidate.action_spec_sha256,
    });
    sha256_hex(payload.to_string().as_bytes())
}

pub fn idempotency_key(
    project_id: &str,
    candidate_id: &str,
    policy_revision: u32,
    expected_head_sha: &str,
    operation: &str,
) -> String {
    sha256_hex(
        format!(
            "{project_id}\u{0}{candidate_id}\u{0}{policy_revision}\u{0}{expected_head_sha}\u{0}{operation}"
        )
        .as_bytes(),
    )
}

pub fn delivery_branch_name(
    campaign_id: &str,
    candidate_sha: &str,
) -> Result<String, DeliveryError> {
    let branch = format!("factory/campaign-{campaign_id}/candidate-{candidate_sha}");
    crate::github::validate_default_branch(&branch)
        .map_err(|_| DeliveryError::blocked("INVALID_BRANCH", "delivery branch name is unsafe"))?;
    if branch.len() > MAX_POLICY_BRANCH_CHARS {
        return Err(DeliveryError::blocked(
            "INVALID_BRANCH",
            "delivery branch name is too long",
        ));
    }
    Ok(branch)
}

/// Revalidate the risk tier from the exact candidate diff. The recorded
/// authorization tier is a floor: a model, Office client or historical Gate
/// label can never downgrade it. Security-sensitive surfaces escalate.
pub fn revalidate_risk(authorized: DeliveryRisk, changed_paths: &[String]) -> DeliveryRisk {
    if changed_paths.is_empty() {
        return DeliveryRisk::Unknown;
    }
    let mut floor = DeliveryRisk::R0;
    for path in changed_paths {
        floor = floor.max(path_risk_floor(path));
    }
    authorized.max(floor)
}

fn path_risk_floor(path: &str) -> DeliveryRisk {
    let lower = path.to_ascii_lowercase();
    if path.starts_with("native/src/db/authorization.rs")
        || path.starts_with("native/src/db/delegations.rs")
        || path.starts_with("native/src/db/provider_secrets.rs")
        || path.starts_with("native/src/secrets.rs")
        || path.starts_with("native/src/provider_secrets.rs")
        || path.starts_with("native/src/sandbox.rs")
        || path.starts_with("native/src/delivery.rs")
        || path.starts_with("native/src/db/delivery.rs")
        || path.starts_with("native/src/github.rs")
        || lower.contains("credential")
        || lower.contains("auth")
        || lower.contains("crypto")
        || lower.contains("isolation")
        || lower.contains("permission")
    {
        return DeliveryRisk::R3;
    }
    if path.starts_with("migrations/")
        || path.starts_with(".github/")
        || path.starts_with("deploy/")
        || path.starts_with("compose")
        || path.starts_with("Dockerfile")
        || path.ends_with("Cargo.toml")
        || path.ends_with("Cargo.lock")
        || path.ends_with("package.json")
        || path.ends_with("package-lock.json")
        || path.starts_with("native/src/bin/worker.rs")
        || path.starts_with("native/src/runtime")
    {
        return DeliveryRisk::R2;
    }
    if path.starts_with("docs/contracts/") || path.starts_with("docs/architecture/decisions/") {
        return DeliveryRisk::R1;
    }
    DeliveryRisk::R0
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryBlock {
    pub code: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryDecisionRecord {
    pub decision_id: String,
    pub attempt_id: String,
    pub candidate_id: String,
    pub candidate_sha: String,
    pub gate_revision: String,
    pub gate_outcome: String,
    pub policy_revision: u32,
    pub policy_sha256: String,
    pub charter_grant_id: Option<String>,
    pub risk_class: DeliveryRisk,
    pub expected_head_sha: String,
    pub base_branch: String,
    pub checks_read_at: String,
    pub branch_protection_read_at: String,
    pub decision: String,
    pub reasons: Vec<DeliveryBlock>,
    pub decided_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryDecision {
    pub decision: String,
    pub authorization: Option<DeliveryAuthorization>,
    pub reasons: Vec<DeliveryBlock>,
}

impl DeliveryDecision {
    pub fn allowed(&self) -> bool {
        self.decision == "ALLOW"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateDeliveryFacts {
    pub candidate_id: String,
    pub candidate_sha: String,
    pub gate_revision: String,
    pub outcome: GateOutcome,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateDeliveryFacts {
    pub candidate_id: String,
    pub candidate_sha: String,
    pub candidate_tree_sha: String,
    pub content_digest: String,
    pub base_commit: String,
    pub risk_class: DeliveryRisk,
    pub latest: bool,
    pub salvage_requires_human_review: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestDeliveryFacts {
    pub number: u64,
    pub state: String,
    pub draft: bool,
    pub merged: bool,
    pub head_sha: String,
    pub head_ref: String,
    pub base_ref: String,
    pub base_sha: String,
    pub mergeable: Option<bool>,
    pub mergeable_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckDeliveryFact {
    pub name: String,
    pub state: String,
    pub conclusion: String,
    pub required_by_protection: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecksDeliveryFacts {
    pub head_sha: String,
    pub read_at: DateTime<Utc>,
    pub checks: Vec<CheckDeliveryFact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectionDeliveryFacts {
    pub read_at: DateTime<Utc>,
    pub protected: bool,
    pub allow_force_pushes: bool,
    pub allow_deletions: bool,
    pub required_checks: Vec<String>,
    pub strict: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharterDeliveryFacts {
    pub grant_id: String,
    pub delivery_autonomy: bool,
    pub covers: bool,
    pub expired_or_revoked: bool,
}

#[derive(Debug, Clone)]
pub struct DeliveryEvaluationInput<'a> {
    pub policy: Option<&'a DeliveryPolicy>,
    pub candidate: &'a CandidateDeliveryFacts,
    pub gate: Option<&'a GateDeliveryFacts>,
    pub pull_request: Option<&'a PullRequestDeliveryFacts>,
    pub checks: Option<&'a ChecksDeliveryFacts>,
    pub protection: Option<&'a ProtectionDeliveryFacts>,
    pub charter: Option<&'a CharterDeliveryFacts>,
    pub incident_open: bool,
    pub incident_detail: Option<&'a str>,
    pub manual_confirmed: bool,
    pub base_branch: &'a str,
    pub base_is_default: bool,
    pub current_base_sha: Option<&'a str>,
    pub security_reviewed: bool,
    pub now: DateTime<Utc>,
}

/// Deterministic delivery authorization. No model output is accepted here.
fn denied(code: &str, detail: impl Into<String>) -> DeliveryDecision {
    DeliveryDecision {
        decision: "DENY".into(),
        authorization: None,
        reasons: vec![DeliveryBlock {
            code: code.into(),
            detail: detail.into(),
        }],
    }
}

pub fn evaluate_delivery(input: &DeliveryEvaluationInput<'_>) -> DeliveryDecision {
    let Some(policy) = input.policy else {
        return denied(
            "DELIVERY_POLICY_REQUIRED",
            "Project has no delivery policy; append one before delivery",
        );
    };
    if policy.kill_switch {
        return denied(
            "DELIVERY_INCIDENT_OPEN",
            "the Project delivery kill switch is active",
        );
    }
    if let Some(expires) = policy.expires_at.as_deref()
        && expired(expires, input.now)
    {
        return denied(
            "DELIVERY_POLICY_STALE",
            "the Project delivery policy has expired",
        );
    }
    if input.incident_open {
        return denied(
            "DELIVERY_INCIDENT_OPEN",
            input
                .incident_detail
                .unwrap_or("an open delivery incident freezes auto-merge"),
        );
    }
    let Some(gate) = input.gate else {
        return denied(
            "GATE_NOT_ACCEPTED",
            "the exact candidate has no Quality Gate decision",
        );
    };
    if gate.outcome != GateOutcome::Accepted {
        return denied(
            "GATE_NOT_ACCEPTED",
            format!("Quality Gate outcome is {:?}", gate.outcome),
        );
    }
    if !gate.current {
        return denied(
            "GATE_NOT_ACCEPTED",
            "the Quality Gate decision is not the current revision",
        );
    }
    if gate.candidate_id != input.candidate.candidate_id
        || !gate
            .candidate_sha
            .eq_ignore_ascii_case(&input.candidate.candidate_sha)
    {
        return denied(
            "GATE_NOT_ACCEPTED",
            "the Quality Gate decision is bound to a different candidate revision",
        );
    }
    if !input.candidate.latest {
        return denied(
            "CANDIDATE_SUPERSEDED",
            "the candidate is not the latest eligible candidate for its Attempt",
        );
    }
    if input.candidate.salvage_requires_human_review {
        return denied(
            "SALVAGE_HUMAN_REVIEW_REQUIRED",
            "R1/R2/R3 stagnation salvage requires authenticated human review of the final-source and salvage evidence and is never auto-merged",
        );
    }

    if input.candidate.risk_class == DeliveryRisk::R1
        && policy.post_merge_check_id.trim().is_empty()
    {
        return denied(
            "POST_MERGE_CHECK_MISSING",
            "every R1 merge requires a configured post-merge check",
        );
    }

    let authorization = match policy.mode {
        DeliveryMode::Manual => {
            if !input.manual_confirmed {
                return denied(
                    "DELIVERY_POLICY_DISABLED",
                    "the Project policy is MANUAL and requires an owner merge decision",
                );
            }
            DeliveryAuthorization::OwnerManual
        }
        DeliveryMode::RiskTiered => match input.candidate.risk_class {
            DeliveryRisk::R0 => {
                if input.manual_confirmed {
                    DeliveryAuthorization::OwnerManual
                } else {
                    DeliveryAuthorization::PolicyR0
                }
            }
            DeliveryRisk::R1 => {
                if input.manual_confirmed {
                    DeliveryAuthorization::OwnerManual
                } else if !policy.r1_enabled {
                    return denied(
                        "R1_OPT_IN_REQUIRED",
                        "R1 auto-merge requires an explicit Project opt-in",
                    );
                } else if let Some(charter) = input.charter {
                    if charter.expired_or_revoked || !charter.covers {
                        return denied(
                            "CHARTER_GRANT_EXPIRED",
                            "the Owner Charter delivery grant is expired, revoked or out of scope",
                        );
                    }
                    if !charter.delivery_autonomy {
                        return denied(
                            "CHARTER_GRANT_REQUIRED",
                            "the Owner Charter does not grant delivery autonomy",
                        );
                    }
                    if policy.post_merge_check_id.trim().is_empty() {
                        return denied(
                            "POST_MERGE_CHECK_MISSING",
                            "R1 auto-merge requires a configured post-merge check",
                        );
                    }
                    DeliveryAuthorization::CharterR1
                } else {
                    return denied(
                        "CHARTER_GRANT_REQUIRED",
                        "R1 auto-merge requires a current matching Owner Charter delivery grant",
                    );
                }
            }
            DeliveryRisk::R2 | DeliveryRisk::R3 | DeliveryRisk::Unknown => {
                if !input.manual_confirmed {
                    return denied(
                        "OWNER_APPROVAL_REQUIRED",
                        "R2, R3 and unknown risk are manual delivery only",
                    );
                }
                DeliveryAuthorization::OwnerManual
            }
        },
    };

    let auto = matches!(
        authorization,
        DeliveryAuthorization::PolicyR0 | DeliveryAuthorization::CharterR1
    );
    if auto && !input.base_is_default {
        return denied(
            "BASE_NOT_DEFAULT",
            "auto-merge is permitted only on the protected default branch",
        );
    }
    if !auto
        && !input.base_is_default
        && !policy
            .manual_base_branches
            .iter()
            .any(|branch| branch == input.base_branch)
    {
        return denied(
            "BASE_MISMATCH",
            "the base branch is not an allowed protected manual base",
        );
    }
    if input.candidate.risk_class == DeliveryRisk::R3 && !input.security_reviewed {
        return denied(
            "OWNER_APPROVAL_REQUIRED",
            "R3 delivery requires an independent security review receipt",
        );
    }

    let Some(pull_request) = input.pull_request else {
        return denied("PR_NOT_OPEN", "no pull request is recorded for delivery");
    };
    if pull_request.merged {
        return denied(
            "PR_ALREADY_MERGED",
            "the pull request is already merged; replay must reconcile the attempt",
        );
    }
    if pull_request.state != "open" {
        return denied(
            "PR_NOT_OPEN",
            format!("the pull request is {}", pull_request.state),
        );
    }
    if pull_request.draft {
        return denied("PR_DRAFT", "a draft pull request is never merged");
    }
    if !pull_request
        .head_sha
        .eq_ignore_ascii_case(&input.candidate.candidate_sha)
    {
        return denied(
            "HEAD_MISMATCH",
            format!(
                "pull request head {} does not equal candidate {}",
                pull_request.head_sha, input.candidate.candidate_sha
            ),
        );
    }
    if pull_request.base_ref != input.base_branch {
        return denied(
            "BASE_MISMATCH",
            format!(
                "pull request base {} does not equal requested base {}",
                pull_request.base_ref, input.base_branch
            ),
        );
    }
    if let Some(current_base) = input.current_base_sha
        && !pull_request.base_sha.eq_ignore_ascii_case(current_base)
    {
        return denied(
            "BASE_MISMATCH",
            "the pull request base is stale against the protected default branch",
        );
    }
    if pull_request.mergeable != Some(true) {
        return denied(
            "MERGE_BLOCKED",
            match pull_request.mergeable {
                Some(false) => "GitHub reports the pull request as not mergeable",
                None => "GitHub has not computed mergeability yet",
                Some(true) => unreachable!(),
            },
        );
    }
    if pull_request.mergeable_state != "clean" {
        return denied(
            "MERGE_BLOCKED",
            format!(
                "GitHub merge state is {} rather than clean",
                pull_request.mergeable_state
            ),
        );
    }

    let Some(checks) = input.checks else {
        return denied("CHECKS_NOT_READY", "required checks were not read");
    };
    if !fresh(checks.read_at, input.now) {
        return denied(
            "CHECKS_STALE",
            "the check read is older than the freshness bound",
        );
    }
    if !checks
        .head_sha
        .eq_ignore_ascii_case(&input.candidate.candidate_sha)
    {
        return denied(
            "CHECKS_STALE",
            "required checks are not bound to the exact candidate head",
        );
    }
    let protection = input.protection;
    let protection_required = protection
        .map(|facts| facts.required_checks.clone())
        .unwrap_or_default();
    for required in &policy.required_checks {
        if !protection_required.iter().any(|name| name == required) {
            return denied(
                "BRANCH_PROTECTION_UNVERIFIED",
                format!("required check {required:?} is not required by branch protection"),
            );
        }
    }
    for required in protection_required
        .iter()
        .chain(policy.required_checks.iter())
    {
        match checks.checks.iter().find(|check| &check.name == required) {
            None => {
                return denied(
                    "CHECKS_NOT_READY",
                    format!("required check {required:?} has no receipt on the exact head"),
                );
            }
            Some(check) if check.conclusion == "success" || check.state == "success" => {}
            Some(check)
                if matches!(check.conclusion.as_str(), "neutral" | "skipped")
                    || matches!(check.state.as_str(), "success" | "neutral" | "skipped") => {}
            Some(check)
                if check.state == "pending"
                    || check.state == "queued"
                    || check.state == "in_progress"
                    || check.state == "missing" =>
            {
                return denied(
                    "CHECKS_NOT_READY",
                    format!("required check {required:?} is {}", check.state),
                );
            }
            Some(check) => {
                return denied(
                    "CHECKS_FAILED",
                    format!(
                        "required check {required:?} concluded {}",
                        if check.conclusion.is_empty() {
                            &check.state
                        } else {
                            &check.conclusion
                        }
                    ),
                );
            }
        }
    }

    let Some(protection) = protection else {
        return denied(
            "BRANCH_PROTECTION_UNVERIFIED",
            "branch protection could not be proven",
        );
    };
    if !fresh(protection.read_at, input.now) {
        return denied(
            "BRANCH_PROTECTION_UNVERIFIED",
            "the branch-protection read is older than the freshness bound",
        );
    }
    if !protection.protected {
        return denied(
            "BRANCH_PROTECTION_UNVERIFIED",
            "the target branch is not protected",
        );
    }
    if protection.allow_force_pushes || protection.allow_deletions {
        return denied(
            "BRANCH_PROTECTION_UNVERIFIED",
            "branch protection permits force-push or deletion",
        );
    }
    if !protection.strict {
        return denied(
            "BRANCH_PROTECTION_UNVERIFIED",
            "branch protection does not require an up-to-date branch",
        );
    }
    if protection.required_checks.is_empty() {
        return denied(
            "BRANCH_PROTECTION_UNVERIFIED",
            "branch protection has no required status checks",
        );
    }

    DeliveryDecision {
        decision: "ALLOW".into(),
        authorization: Some(authorization),
        reasons: Vec::new(),
    }
}

fn expired(expires_at: &str, now: DateTime<Utc>) -> bool {
    DateTime::parse_from_rfc3339(expires_at)
        .map(|expires| now >= expires.with_timezone(&Utc))
        .unwrap_or(true)
}

fn fresh(read_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now.signed_duration_since(read_at) <= chrono::Duration::seconds(FRESHNESS_WINDOW_SECS)
        && read_at <= now
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryRun {
    pub attempt: DeliveryAttemptRecord,
    pub policy: DeliveryPolicy,
    pub decision: DeliveryDecisionRecord,
    pub pull_request: Option<GitHubPullRequest>,
    pub checks: Option<GitHubPullRequestChecks>,
    pub protection: Option<GitHubBranchProtection>,
    pub issue_outcome: Option<IssueDeliveryOutcome>,
    pub replayed: bool,
}

#[derive(Debug, Clone)]
pub struct OpenedPullRequest {
    pub pull_request: GitHubPullRequest,
    pub created: bool,
    pub issue_outcome: Option<IssueDeliveryOutcome>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryOperation {
    OpenPullRequest,
    Merge,
}

impl DeliveryOperation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenPullRequest => "OPEN_PR",
            Self::Merge => "MERGE",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeliveryRequest<'a> {
    pub campaign_id: &'a str,
    pub candidate_id: &'a str,
    pub base_branch: Option<&'a str>,
    pub operation: DeliveryOperation,
    pub manual_confirmed: bool,
    pub actor: &'a str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryStatus {
    pub campaign_id: String,
    pub candidate_id: String,
    pub candidate_sha: String,
    pub project_id: String,
    pub risk_class: DeliveryRisk,
    pub final_source: CandidateFinalSource,
    pub salvage_requires_human_review: bool,
    pub salvage_evidence: Option<CandidateSalvageEvidenceRecord>,
    pub salvage_error: Option<String>,
    pub gate_outcome: Option<GateOutcome>,
    pub gate_revision: Option<String>,
    pub policy: Option<DeliveryPolicy>,
    pub binding: Option<ProjectGitHubBinding>,
    pub projection: DeliveryProjection,
    pub pull_request: Option<GitHubPullRequest>,
    pub checks: Option<GitHubPullRequestChecks>,
    pub protection: Option<GitHubBranchProtection>,
    pub eligibility: Option<DeliveryDecision>,
    pub live_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryAttemptRecord {
    pub attempt_id: String,
    pub idempotency_key: String,
    pub schema_version: u32,
    pub operation: String,
    pub project_id: String,
    pub campaign_id: String,
    pub candidate_id: String,
    pub candidate_sha: String,
    pub candidate_tree_sha: String,
    pub content_digest: String,
    pub gate_revision: String,
    pub gate_outcome: String,
    pub policy_revision: u32,
    pub policy_sha256: String,
    pub risk_class: DeliveryRisk,
    pub authorization: Option<DeliveryAuthorization>,
    pub charter_grant_id: Option<String>,
    pub pull_request_number: Option<u64>,
    pub base_branch: String,
    pub expected_head_sha: String,
    pub observed_head_sha: Option<String>,
    pub observed_base_sha: Option<String>,
    pub merge_method: String,
    pub state: DeliveryAttemptState,
    pub merge_commit_sha: Option<String>,
    pub incident_id: Option<String>,
    pub post_merge_check_id: Option<String>,
    pub blocker_code: Option<String>,
    pub blocker_detail: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub merged_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryCheckReceiptRecord {
    pub attempt_id: String,
    pub check_name: String,
    pub head_sha: String,
    pub state: String,
    pub conclusion: String,
    pub required_by_protection: bool,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryPostMergeCheckRecord {
    pub attempt_id: String,
    pub check_id: String,
    pub merge_commit_sha: String,
    pub status: String,
    pub detail: String,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryIncidentRecord {
    pub incident_id: String,
    pub project_id: String,
    pub attempt_id: String,
    pub candidate_id: String,
    pub merge_commit_sha: String,
    pub kind: String,
    pub state: DeliveryIncidentState,
    pub detail: String,
    pub created_at: String,
    pub updated_at: String,
    pub acknowledged_by: Option<String>,
    pub acknowledged_at: Option<String>,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryRevertRecord {
    pub revert_id: String,
    pub incident_id: String,
    pub revert_pr_number: u64,
    pub revert_merge_sha: String,
    pub actor: String,
    pub rationale: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryIssueReceiptRecord {
    pub receipt_id: String,
    pub attempt_id: String,
    pub issue_link_id: Option<String>,
    pub action: String,
    pub state: String,
    pub external_id: Option<String>,
    pub detail: Option<String>,
    pub recorded_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryProjection {
    pub policy: Option<DeliveryPolicy>,
    pub attempts: Vec<DeliveryAttemptRecord>,
    pub decisions: Vec<DeliveryDecisionRecord>,
    pub check_receipts: Vec<DeliveryCheckReceiptRecord>,
    pub post_merge_checks: Vec<DeliveryPostMergeCheckRecord>,
    pub incidents: Vec<DeliveryIncidentRecord>,
    pub reverts: Vec<DeliveryRevertRecord>,
    pub issue_receipts: Vec<DeliveryIssueReceiptRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateSalvageEvidenceRecord {
    pub candidate_id: String,
    pub commit_sha: String,
    pub attempt_id: String,
    pub work_item_id: String,
    pub frozen_checks: u64,
    pub passed_checks: u64,
    pub written: bool,
    pub decision: String,
    pub changed_paths: Vec<String>,
    pub check_receipt_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateSalvageRecord {
    pub final_source: CandidateFinalSource,
    pub evidence: Option<CandidateSalvageEvidenceRecord>,
}

impl CandidateSalvageRecord {
    pub fn requires_human_review(&self) -> bool {
        self.final_source == CandidateFinalSource::StagnationSalvage
    }
}

#[derive(Debug)]
pub struct DeliveryError {
    kind: &'static str,
    code: &'static str,
    detail: String,
    blockers: Vec<String>,
    retryable: bool,
    attempt_id: Option<String>,
}

impl DeliveryError {
    pub fn new(kind: &'static str, code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            kind,
            code,
            detail: detail.into(),
            blockers: Vec::new(),
            retryable: false,
            attempt_id: None,
        }
    }

    pub fn blocked(kind: &'static str, detail: impl Into<String>) -> Self {
        Self::new(kind, "DELIVERY_BLOCKED", detail)
    }

    pub fn policy_required(detail: impl Into<String>) -> Self {
        Self::new(
            "DELIVERY_POLICY_REQUIRED",
            "DELIVERY_POLICY_REQUIRED",
            detail,
        )
    }

    pub fn permission_required(detail: impl Into<String>) -> Self {
        Self::new(
            "DELIVERY_PERMISSION_REQUIRED",
            "DELIVERY_PERMISSION_REQUIRED",
            detail,
        )
    }

    pub fn with_blockers(mut self, blockers: Vec<DeliveryBlock>) -> Self {
        self.blockers = blockers
            .into_iter()
            .map(|block| format!("{}: {}", block.code, block.detail))
            .collect();
        self
    }

    pub fn with_attempt(mut self, attempt_id: impl Into<String>) -> Self {
        self.attempt_id = Some(attempt_id.into());
        self
    }

    pub fn transient(mut self) -> Self {
        self.retryable = true;
        self
    }

    pub fn kind(&self) -> &'static str {
        self.kind
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }

    pub fn blockers(&self) -> &[String] {
        &self.blockers
    }

    pub fn retryable(&self) -> bool {
        self.retryable
    }

    pub fn attempt_id(&self) -> Option<&str> {
        self.attempt_id.as_deref()
    }
}

impl std::fmt::Display for DeliveryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for DeliveryError {}

impl From<DbError> for DeliveryError {
    fn from(error: DbError) -> Self {
        match error {
            DbError::NotFound => Self::blocked("NOT_FOUND", "delivery record was not found"),
            DbError::Conflict(detail) => Self::blocked("IDEMPOTENCY_CONFLICT", detail),
            error => Self::new("DATABASE", "DELIVERY_BLOCKED", error.to_string()).transient(),
        }
    }
}

impl From<GitHubError> for DeliveryError {
    fn from(error: GitHubError) -> Self {
        let detail = error.detail();
        match error {
            GitHubError::Unauthorized | GitHubError::Forbidden(_) => {
                Self::permission_required(detail)
            }
            GitHubError::MergeBlocked { .. } => Self::blocked("MERGE_BLOCKED", detail),
            GitHubError::RateLimited { .. }
            | GitHubError::Upstream(_)
            | GitHubError::InvalidResponse(_) => {
                Self::blocked("GITHUB_UNAVAILABLE", detail).transient()
            }
            GitHubError::NotFound => Self::blocked("NOT_FOUND", detail),
            GitHubError::Redirected | GitHubError::Invalid(_) | GitHubError::Rejected { .. } => {
                Self::blocked("GITHUB_UNAVAILABLE", detail)
            }
        }
    }
}

impl From<IssueDeliveryError> for DeliveryError {
    fn from(error: IssueDeliveryError) -> Self {
        Self::blocked("ISSUE_DELIVERY_FAILED", error.detail())
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

struct DeliveryContext {
    campaign: crate::db::Campaign,
    candidate: CandidateRecord,
    project: Project,
    binding: ProjectGitHubBinding,
    gate: Option<GateDeliveryFacts>,
    risk: DeliveryRisk,
    salvage: CandidateSalvageRecord,
    candidate_base: String,
    base_branch: String,
    policy: Option<DeliveryPolicy>,
    incident: Option<DeliveryIncidentRecord>,
}

struct RepositoryCheck {
    commit_sha: String,
    tree_sha: String,
}

impl RepositoryCheck {
    fn from_verified(value: crate::workspace::VerifiedCandidate) -> Self {
        Self {
            commit_sha: value.commit_sha,
            tree_sha: value.tree_sha,
        }
    }
}

async fn verify_candidate(
    project: &Project,
    base_commit: &str,
    candidate: &CandidateRecord,
) -> Result<(RepositoryCheck, Vec<String>), DeliveryError> {
    let repository = PathBuf::from(&project.repository);
    let expected_identity = project.identity.clone();
    let base = base_commit.to_owned();
    let commit = candidate.commit_sha.clone();
    let tree = candidate.tree_sha.clone();
    let (verified, paths) = tokio::task::spawn_blocking(move || {
        let inspected = crate::repository::inspect(&repository, &[])
            .map_err(|error| format!("repository identity check failed: {error}"))?;
        if expected_identity.is_empty()
            || !inspected
                .identity
                .eq_ignore_ascii_case(expected_identity.trim())
        {
            return Err("project repository identity changed".to_string());
        }
        let verified = verify_candidate_commit(&repository, &base, &commit, &tree)
            .map_err(|error| format!("candidate revision rejected: {error}"))?;
        let paths = crate::workspace::changed_paths(&repository, &base, &commit)
            .map_err(|error| format!("candidate diff could not be read: {error}"))?;
        Ok((RepositoryCheck::from_verified(verified), paths))
    })
    .await
    .map_err(|error| {
        DeliveryError::blocked(
            "GIT",
            format!("candidate verification task failed: {error}"),
        )
    })?
    .map_err(|detail| DeliveryError::blocked("CANDIDATE_NOT_CURRENT", detail))?;
    Ok((verified, paths))
}

fn branch_permission_ok(permissions: Option<crate::github::GitHubRepositoryPermissions>) -> bool {
    permissions
        .is_none_or(|permissions| permissions.admin || permissions.maintain || permissions.push)
}

fn sanitize_git_output(value: &[u8], token: &str) -> String {
    let text = String::from_utf8_lossy(value);
    let text = if token.is_empty() {
        text.into_owned()
    } else {
        text.replace(token, "<redacted>")
    };
    let bounded = text.chars().take(MAX_PUSH_STDERR_BYTES).collect::<String>();
    bounded.replace('\n', " ")
}

fn validate_remote(remote: &str) -> Result<RemoteKind, DeliveryError> {
    if remote.starts_with('/') {
        #[cfg(test)]
        {
            return Ok(RemoteKind::Local);
        }
        #[cfg(not(test))]
        {
            return Err(DeliveryError::blocked(
                "INVALID_REMOTE",
                "production delivery requires an HTTPS GitHub remote",
            ));
        }
    }
    let url = reqwest::Url::parse(remote)
        .map_err(|_| DeliveryError::blocked("INVALID_REMOTE", "git remote is not a valid URL"))?;
    if url.username() != "" || url.password().is_some() {
        return Err(DeliveryError::blocked(
            "INVALID_REMOTE",
            "git remote must not contain credentials",
        ));
    }
    if url.scheme() == "file" {
        #[cfg(test)]
        {
            return Ok(RemoteKind::Local);
        }
        #[cfg(not(test))]
        {
            return Err(DeliveryError::blocked(
                "INVALID_REMOTE",
                "production delivery requires an HTTPS GitHub remote",
            ));
        }
    }
    if url.scheme() == "https" {
        return Ok(RemoteKind::Https);
    }
    Err(DeliveryError::blocked(
        "INVALID_REMOTE",
        "git remote must use HTTPS or a local fixture path",
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteKind {
    #[cfg(test)]
    Local,
    Https,
}

fn remote_for(project: &Project, binding: &ProjectGitHubBinding) -> Result<String, DeliveryError> {
    let configured = project.remote_origin.trim();
    if !configured.is_empty() && validate_remote(configured).is_ok() {
        return Ok(configured.to_owned());
    }
    // SSH or other non-HTTPS remotes are never used with a host credential.
    // Fall back to the Core-bound GitHub repository over HTTPS.
    Ok(format!(
        "https://github.com/{}/{}.git",
        binding.owner, binding.repo
    ))
}

fn git_command(repository: &Path, token: &str, remote: &str) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(repository)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never");
    if validate_remote(remote).ok() == Some(RemoteKind::Https) {
        command
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "http.extraHeader")
            .env(
                "GIT_CONFIG_VALUE_0",
                format!("Authorization: Bearer {token}"),
            );
    }
    command
}

fn ls_remote_branch(
    repository: &Path,
    remote: &str,
    token: &str,
    branch: &str,
) -> Result<Option<String>, DeliveryError> {
    let output = git_command(repository, token, remote)
        .args([
            "ls-remote",
            "--heads",
            remote,
            &format!("refs/heads/{branch}"),
        ])
        .output()
        .map_err(|error| DeliveryError::blocked("GIT_UNAVAILABLE", error.to_string()))?;
    if !output.status.success() {
        return Err(DeliveryError::blocked(
            "GIT_UNAVAILABLE",
            format!(
                "git ls-remote failed: {}",
                sanitize_git_output(&output.stderr, token)
            ),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut values = text.lines().filter_map(|line| {
        let mut fields = line.split_whitespace();
        let sha = fields.next()?;
        let reference = fields.next()?;
        (reference == format!("refs/heads/{branch}")).then(|| sha.to_ascii_lowercase())
    });
    let first = values.next();
    if values.next().is_some() {
        return Err(DeliveryError::blocked(
            "GIT_UNAVAILABLE",
            "remote branch lookup returned multiple revisions",
        ));
    }
    Ok(first)
}

/// Push the exact candidate commit to the deterministic branch. The function
/// never forces, never rewrites an existing different branch and never puts the
/// credential in the remote URL or command arguments.
fn push_exact_candidate(
    repository: &Path,
    remote: &str,
    token: &str,
    branch: &str,
    candidate_sha: &str,
) -> Result<(), DeliveryError> {
    let existing = ls_remote_branch(repository, remote, token, branch)?;
    if let Some(sha) = existing {
        if sha.eq_ignore_ascii_case(candidate_sha) {
            return Ok(());
        }
        return Err(DeliveryError::blocked(
            "HEAD_MISMATCH",
            format!(
                "remote delivery branch {branch} already points to {sha}, not the exact candidate"
            ),
        ));
    }
    let output = git_command(repository, token, remote)
        .args([
            "push",
            "--porcelain",
            remote,
            &format!("{candidate_sha}:refs/heads/{branch}"),
        ])
        .output()
        .map_err(|error| DeliveryError::blocked("GIT_UNAVAILABLE", error.to_string()))?;
    if !output.status.success() {
        // A concurrent identical push is an idempotent success. Anything else
        // is a conflict; Core never retries with force.
        if ls_remote_branch(repository, remote, token, branch)?
            .is_some_and(|sha| sha.eq_ignore_ascii_case(candidate_sha))
        {
            return Ok(());
        }
        return Err(DeliveryError::blocked(
            "HEAD_MISMATCH",
            format!(
                "git push refused without force-push: {}",
                sanitize_git_output(&output.stderr, token)
            ),
        ));
    }
    let Some(sha) = ls_remote_branch(repository, remote, token, branch)? else {
        return Err(DeliveryError::blocked(
            "GIT_UNAVAILABLE",
            "the pushed delivery branch is not visible on the remote",
        ));
    };
    if !sha.eq_ignore_ascii_case(candidate_sha) {
        return Err(DeliveryError::blocked(
            "HEAD_MISMATCH",
            "the remote delivery branch does not resolve to the candidate",
        ));
    }
    Ok(())
}

fn pull_request_facts(pull_request: &GitHubPullRequest) -> PullRequestDeliveryFacts {
    PullRequestDeliveryFacts {
        number: pull_request.number,
        state: pull_request.state.clone(),
        draft: pull_request.draft,
        merged: pull_request.merged,
        head_sha: pull_request.head_sha.clone(),
        head_ref: pull_request.head_ref.clone(),
        base_ref: pull_request.base_ref.clone(),
        base_sha: pull_request.base_sha.clone(),
        mergeable: pull_request.mergeable,
        mergeable_state: pull_request.mergeable_state.clone(),
    }
}

fn protection_facts(
    protection: &GitHubBranchProtection,
    read_at: DateTime<Utc>,
) -> ProtectionDeliveryFacts {
    ProtectionDeliveryFacts {
        read_at,
        protected: protection.protected,
        allow_force_pushes: protection.allow_force_pushes,
        allow_deletions: protection.allow_deletions,
        required_checks: protection.required_checks.clone(),
        strict: protection.required_status_checks_strict,
    }
}

fn check_facts(checks: &GitHubPullRequestChecks, read_at: DateTime<Utc>) -> ChecksDeliveryFacts {
    let mut values = BTreeMap::new();
    for run in &checks.check_runs {
        values.insert(
            run.name.clone(),
            CheckDeliveryFact {
                name: run.name.clone(),
                state: run.status.clone(),
                conclusion: run.conclusion.clone(),
                required_by_protection: false,
            },
        );
    }
    for status in &checks.statuses {
        values.insert(
            status.context.clone(),
            CheckDeliveryFact {
                name: status.context.clone(),
                state: status.state.clone(),
                conclusion: status.state.clone(),
                required_by_protection: false,
            },
        );
    }
    for value in values.values_mut() {
        if value.state == "completed" && value.conclusion.is_empty() {
            value.conclusion = "unknown".into();
        }
    }
    ChecksDeliveryFacts {
        head_sha: checks.head_sha.clone(),
        read_at,
        checks: values.into_values().collect(),
    }
}

fn mark_protection_checks(
    mut checks: ChecksDeliveryFacts,
    protection: &ProtectionDeliveryFacts,
) -> ChecksDeliveryFacts {
    for check in &mut checks.checks {
        check.required_by_protection = protection
            .required_checks
            .iter()
            .any(|name| name == &check.name);
    }
    checks
}

// The concrete GitHub issue hook is implemented in Core. The generic trait
// keeps the delivery engine deterministically testable and makes the exact
// transition points explicit.
#[allow(async_fn_in_trait)]
pub trait DeliveryIssueHook {
    async fn on_pr_opened(
        &self,
        campaign_id: &str,
        candidate_id: &str,
        pull_request_number: u64,
        pull_request_url: &str,
    ) -> Result<IssueDeliveryOutcome, IssueDeliveryError>;

    async fn on_merge_succeeded(
        &self,
        campaign_id: &str,
        candidate_id: &str,
        merge_sha: &str,
    ) -> Result<IssueDeliveryOutcome, IssueDeliveryError>;

    async fn on_delivery_failed(
        &self,
        campaign_id: &str,
        candidate_id: &str,
        error: &IssueDeliveryError,
    ) -> Result<IssueDeliveryOutcome, IssueDeliveryError>;
}

pub struct LiveIssueDeliveryHook<'a> {
    service: crate::issues::IssueDeliveryService<'a>,
}

impl<'a> LiveIssueDeliveryHook<'a> {
    pub async fn new(
        db: &'a Database,
        github: &'a GitHubClient,
        credential: &'a GitHubCredential,
    ) -> Result<Self, IssueDeliveryError> {
        Ok(Self {
            service: crate::issues::IssueDeliveryService::new(db, github, credential).await?,
        })
    }
}

impl DeliveryIssueHook for LiveIssueDeliveryHook<'_> {
    async fn on_pr_opened(
        &self,
        campaign_id: &str,
        candidate_id: &str,
        pull_request_number: u64,
        pull_request_url: &str,
    ) -> Result<IssueDeliveryOutcome, IssueDeliveryError> {
        self.service
            .on_pr_opened_identity(
                campaign_id,
                candidate_id,
                pull_request_number,
                pull_request_url,
            )
            .await
    }

    async fn on_merge_succeeded(
        &self,
        campaign_id: &str,
        candidate_id: &str,
        merge_sha: &str,
    ) -> Result<IssueDeliveryOutcome, IssueDeliveryError> {
        self.service
            .on_merge_succeeded(campaign_id, candidate_id, merge_sha)
            .await
    }

    async fn on_delivery_failed(
        &self,
        campaign_id: &str,
        candidate_id: &str,
        error: &IssueDeliveryError,
    ) -> Result<IssueDeliveryOutcome, IssueDeliveryError> {
        self.service
            .on_delivery_failed(campaign_id, candidate_id, error)
            .await
    }
}

pub async fn load_delivery_status(
    db: &Database,
    github: Option<&GitHubClient>,
    credential: Option<&GitHubCredential>,
    campaign_id: &str,
    candidate_id: &str,
) -> Result<DeliveryStatus, DeliveryError> {
    let campaign = db.campaign(campaign_id).await?;
    let candidate = db.candidate(candidate_id).await?;
    let projection = db.delivery_projection(campaign_id, candidate_id).await?;
    let action = db.action(&campaign.action_id).await?;
    let project = db.project(&action.project_id).await?;
    let binding = db.project_github_binding(&project.id).await.ok();
    let gate = db.gate_for_candidate(&candidate.id).await.ok().flatten();
    let recorded_risk = DeliveryRisk::parse(&action.risk).unwrap_or(DeliveryRisk::Unknown);
    let candidate_base = db
        .candidate_base_commit(&candidate.id)
        .await
        .unwrap_or_else(|_| project.base_commit.clone());
    let risk = match verify_candidate(&project, &candidate_base, &candidate).await {
        Ok((_, changed_paths)) => revalidate_risk(recorded_risk, &changed_paths),
        Err(_) => DeliveryRisk::Unknown,
    };
    let (salvage, salvage_error) = match db.candidate_salvage_record(&candidate.id).await {
        Ok(record) => (Some(record), None),
        Err(DbError::Invalid(detail)) => {
            (None, Some(format!("SALVAGE_EVIDENCE_REQUIRED: {detail}")))
        }
        Err(error) => (None, Some(format!("SALVAGE_EVIDENCE_REQUIRED: {error}"))),
    };
    let final_source = salvage
        .as_ref()
        .map(|record| record.final_source)
        .unwrap_or(candidate.final_source);
    let salvage_requires_human_review =
        final_source == CandidateFinalSource::StagnationSalvage && risk != DeliveryRisk::R0;
    let salvage_evidence = salvage.and_then(|record| record.evidence);
    let mut pull_request = None;
    let mut checks = None;
    let mut protection = None;
    let mut live_error = None;
    if let (Some(github), Some(credential), Some(binding), Some(latest)) = (
        github,
        credential,
        binding.as_ref(),
        projection.attempts.first(),
    ) && let Some(number) = latest.pull_request_number
    {
        match github
            .pull_request(credential.token(), &binding.owner, &binding.repo, number)
            .await
        {
            Ok(pr) => {
                pull_request = Some(pr.clone());
                if let Ok(next) = github
                    .pull_request_checks(
                        credential.token(),
                        &binding.owner,
                        &binding.repo,
                        &pr.head_sha,
                    )
                    .await
                {
                    checks = Some(next);
                }
                if let Ok(next) = github
                    .branch_protection(
                        credential.token(),
                        &binding.owner,
                        &binding.repo,
                        &latest.base_branch,
                    )
                    .await
                {
                    protection = Some(next);
                }
            }
            Err(error) => live_error = Some(error.detail()),
        }
    }
    if binding.is_some() && (github.is_none() || credential.is_none()) {
        live_error = Some(
            "the GitHub connection credential is unavailable; showing recorded delivery state only"
                .into(),
        );
    }
    if pull_request.is_none()
        && let (Some(github), Some(credential), Some(binding)) =
            (github, credential, binding.as_ref())
        && let Ok(branch) = delivery_branch_name(&campaign.id, &candidate.commit_sha)
    {
        match github
            .open_pull_requests_for_head(credential.token(), &binding.owner, &binding.repo, &branch)
            .await
        {
            Ok(open) => pull_request = open.into_iter().next(),
            Err(error) => live_error = Some(error.detail()),
        }
    }
    Ok(DeliveryStatus {
        campaign_id: campaign.id,
        candidate_id: candidate.id,
        candidate_sha: candidate.commit_sha,
        project_id: project.id,
        risk_class: risk,
        final_source,
        salvage_requires_human_review,
        salvage_evidence,
        salvage_error,
        gate_revision: gate.as_ref().map(|record| record.decided_at.clone()),
        gate_outcome: gate.map(|record| record.outcome),
        policy: projection.policy.clone(),
        binding,
        projection,
        pull_request,
        checks,
        protection,
        eligibility: None,
        live_error,
    })
}

pub struct DeliveryService<'a, H: DeliveryIssueHook> {
    db: &'a Database,
    github: &'a GitHubClient,
    credential: &'a GitHubCredential,
    hook: &'a H,
}

impl<'a, H: DeliveryIssueHook> DeliveryService<'a, H> {
    pub fn new(
        db: &'a Database,
        github: &'a GitHubClient,
        credential: &'a GitHubCredential,
        hook: &'a H,
    ) -> Self {
        Self {
            db,
            github,
            credential,
            hook,
        }
    }

    pub async fn status(
        &self,
        campaign_id: &str,
        candidate_id: &str,
    ) -> Result<DeliveryStatus, DeliveryError> {
        load_delivery_status(
            self.db,
            Some(self.github),
            Some(self.credential),
            campaign_id,
            candidate_id,
        )
        .await
    }

    pub async fn open_pull_request(
        &self,
        request: &DeliveryRequest<'_>,
    ) -> Result<OpenedPullRequest, DeliveryError> {
        let context = self.load_context(request).await?;
        self.ensure_credential_and_permission(&context).await?;
        let branch = delivery_branch_name(&context.campaign.id, &context.candidate.commit_sha)?;
        let remote = remote_for(&context.project, &context.binding)?;
        push_exact_candidate(
            Path::new(&context.project.repository),
            &remote,
            self.credential.token(),
            &branch,
            &context.candidate.commit_sha,
        )?;
        let opened = self.open_or_reuse_pull_request(&context, &branch).await?;
        let issue_outcome = match self
            .hook
            .on_pr_opened(
                &context.campaign.id,
                &context.candidate.id,
                opened.pull_request.number,
                &opened.pull_request.html_url,
            )
            .await
        {
            Ok(outcome) => Some(outcome),
            Err(error) => {
                let _ = self
                    .hook
                    .on_delivery_failed(&context.campaign.id, &context.candidate.id, &error)
                    .await;
                return Err(DeliveryError::from(error));
            }
        };
        Ok(OpenedPullRequest {
            pull_request: opened.pull_request,
            created: opened.created,
            issue_outcome,
        })
    }

    pub async fn deliver(
        &self,
        request: &DeliveryRequest<'_>,
    ) -> Result<DeliveryRun, DeliveryError> {
        let context = self.load_context(request).await?;
        let Some(policy) = context.policy.clone() else {
            return Err(DeliveryError::policy_required(
                "Project has no delivery policy; append one before delivery",
            ));
        };
        let Some(gate) = context.gate.as_ref() else {
            return Err(DeliveryError::blocked(
                "GATE_NOT_ACCEPTED",
                "the exact candidate has no Quality Gate decision",
            ));
        };
        if policy.auto_merge_base_branch != context.binding.default_branch {
            return Err(DeliveryError::blocked(
                "DELIVERY_POLICY_STALE",
                "the policy auto-merge base no longer matches the Project default branch",
            ));
        }
        if request.operation != DeliveryOperation::Merge {
            return Err(DeliveryError::blocked(
                "OPERATION_MISMATCH",
                "open-pull-request operation cannot record a merge attempt",
            ));
        }
        let expected_head_sha = context.candidate.commit_sha.clone();
        let key = idempotency_key(
            &context.project.id,
            &context.candidate.id,
            policy.revision,
            &expected_head_sha,
            DeliveryOperation::Merge.as_str(),
        );
        let digest = content_digest(
            &context.project.id,
            &context.campaign.id,
            &context.candidate,
        );
        let new_attempt = NewDeliveryAttempt {
            idempotency_key: key,
            operation: "MERGE",
            project_id: context.project.id.clone(),
            campaign_id: context.campaign.id.clone(),
            candidate_id: context.candidate.id.clone(),
            candidate_sha: context.candidate.commit_sha.clone(),
            candidate_tree_sha: context.candidate.tree_sha.clone(),
            content_digest: digest,
            gate_revision: gate.gate_revision.clone(),
            gate_outcome: gate_outcome_str(gate.outcome).into(),
            policy_revision: policy.revision,
            policy_sha256: policy.policy_sha256.clone(),
            risk_class: context.risk.as_str().into(),
            base_branch: context.base_branch.clone(),
            expected_head_sha: expected_head_sha.clone(),
            merge_method: MERGE_METHOD.into(),
        };
        // The durable attempt reservation is atomic and happens before any
        // GitHub push, PR or issue-hook side effect. A different base loses
        // here with zero external effects.
        let reservation = self.db.create_delivery_attempt(new_attempt).await?;
        if reservation.base_conflict {
            return Err(DeliveryError::blocked(
                "BASE_MISMATCH",
                format!(
                    "the durable delivery attempt is bound to base {}; changing to {} requires an explicit new delivery decision",
                    reservation.attempt.base_branch, context.base_branch
                ),
            )
            .with_attempt(reservation.attempt.attempt_id));
        }
        let attempt = reservation.attempt;
        let attempt_id = attempt.attempt_id.clone();
        if attempt.state == DeliveryAttemptState::Merged {
            if let Some(merge_sha) = attempt.merge_commit_sha.as_deref() {
                self.revalidate_merged_replay(&attempt, &context, merge_sha)
                    .await?;
            }
            let _receipts = self.db.delivery_issue_receipts(&attempt_id).await?;
            let replay_policy = self
                .db
                .delivery_policy_revision(&context.project.id, attempt.policy_revision)
                .await
                .unwrap_or_else(|_| policy.clone());
            let decision = match self.db.latest_delivery_decision(&attempt_id).await? {
                Some(decision) => decision,
                None => {
                    self.record_allow_decision(
                        &attempt,
                        &policy,
                        &context,
                        None,
                        now_utc(),
                        now_utc(),
                    )
                    .await?
                }
            };
            return Ok(DeliveryRun {
                attempt,
                policy: replay_policy,
                decision,
                pull_request: None,
                checks: None,
                protection: None,
                issue_outcome: None,
                replayed: true,
            });
        }
        if attempt.state == DeliveryAttemptState::NeedsAttention {
            return Err(DeliveryError::blocked(
                "DELIVERY_INCIDENT_OPEN",
                "delivery needs owner attention",
            )
            .with_attempt(attempt_id));
        }
        if attempt.state == DeliveryAttemptState::Reverted {
            return Err(DeliveryError::blocked(
                "DELIVERY_REVERTED",
                "the delivery attempt was reverted; a new reviewed change is required",
            )
            .with_attempt(attempt_id));
        }
        if !reservation.created && attempt.gate_revision != gate.gate_revision {
            return Err(DeliveryError::blocked(
                "GATE_NOT_ACCEPTED",
                "the attempt is bound to a different Quality Gate revision",
            )
            .with_attempt(attempt_id));
        }
        if !reservation.created && attempt.policy_revision != policy.revision {
            return Err(DeliveryError::blocked(
                "DELIVERY_POLICY_STALE",
                "the attempt used another policy revision",
            )
            .with_attempt(attempt_id));
        }
        self.ensure_credential_and_permission(&context).await?;
        let branch = delivery_branch_name(&context.campaign.id, &context.candidate.commit_sha)?;
        let remote = remote_for(&context.project, &context.binding)?;
        push_exact_candidate(
            Path::new(&context.project.repository),
            &remote,
            self.credential.token(),
            &branch,
            &context.candidate.commit_sha,
        )?;
        let opened = self.open_or_reuse_pull_request(&context, &branch).await?;
        if let Err(error) = self
            .hook
            .on_pr_opened(
                &context.campaign.id,
                &context.candidate.id,
                opened.pull_request.number,
                &opened.pull_request.html_url,
            )
            .await
        {
            let _ = self
                .hook
                .on_delivery_failed(&context.campaign.id, &context.candidate.id, &error)
                .await;
            return Err(DeliveryError::from(error));
        }
        let attempt = if attempt.pull_request_number.is_none() {
            self.db
                .update_delivery_attempt(
                    &attempt_id,
                    DeliveryAttemptUpdate {
                        pull_request_number: Some(opened.pull_request.number),
                        ..Default::default()
                    },
                )
                .await?
        } else {
            attempt
        };
        if attempt.state == DeliveryAttemptState::Merged {
            if let Some(merge_sha) = attempt.merge_commit_sha.as_deref() {
                self.revalidate_merged_replay(&attempt, &context, merge_sha)
                    .await?;
            }
            let _receipts = self.db.delivery_issue_receipts(&attempt_id).await?;
            let replay_policy = self
                .db
                .delivery_policy_revision(&context.project.id, attempt.policy_revision)
                .await
                .unwrap_or_else(|_| policy.clone());
            let decision = match self.db.latest_delivery_decision(&attempt_id).await? {
                Some(decision) => decision,
                None => {
                    self.record_allow_decision(
                        &attempt,
                        &policy,
                        &context,
                        Some(&opened.pull_request),
                        now_utc(),
                        now_utc(),
                    )
                    .await?
                }
            };
            return Ok(DeliveryRun {
                attempt,
                policy: replay_policy,
                decision,
                pull_request: Some(opened.pull_request),
                checks: None,
                protection: None,
                issue_outcome: None,
                replayed: true,
            });
        }

        let pull_request = match self
            .github
            .pull_request(
                self.credential.token(),
                &context.binding.owner,
                &context.binding.repo,
                opened.pull_request.number,
            )
            .await
        {
            Ok(pull_request) => pull_request,
            Err(error) => {
                let error = DeliveryError::from(error).with_attempt(attempt_id.clone());
                self.fail_attempt(&attempt, &context, &error).await;
                return Err(error);
            }
        };
        let checks = match self
            .github
            .pull_request_checks(
                self.credential.token(),
                &context.binding.owner,
                &context.binding.repo,
                &pull_request.head_sha,
            )
            .await
        {
            Ok(checks) => checks,
            Err(error) => {
                let error = DeliveryError::from(error).with_attempt(attempt_id.clone());
                self.fail_attempt(&attempt, &context, &error).await;
                return Err(error);
            }
        };
        let protection = match self
            .github
            .branch_protection(
                self.credential.token(),
                &context.binding.owner,
                &context.binding.repo,
                &context.base_branch,
            )
            .await
        {
            Ok(protection) => protection,
            Err(GitHubError::NotFound) => {
                let error = DeliveryError::blocked(
                    "BRANCH_PROTECTION_UNVERIFIED",
                    "GitHub did not expose branch protection for the delivery base branch",
                )
                .with_attempt(attempt_id.clone());
                self.fail_attempt(&attempt, &context, &error).await;
                return Err(error);
            }
            Err(error) => {
                let error = DeliveryError::from(error).with_attempt(attempt_id.clone());
                self.fail_attempt(&attempt, &context, &error).await;
                return Err(error);
            }
        };
        let current_base = match self
            .github
            .branch_commit(
                self.credential.token(),
                &context.binding.owner,
                &context.binding.repo,
                &context.base_branch,
            )
            .await
        {
            Ok(branch) => branch.commit_sha,
            Err(error) => {
                let error = DeliveryError::from(error).with_attempt(attempt_id.clone());
                self.fail_attempt(&attempt, &context, &error).await;
                return Err(error);
            }
        };
        let current_policy = self.db.delivery_policy(&context.project.id).await?;
        if current_policy.as_ref().map(|policy| policy.revision) != Some(policy.revision) {
            let error = DeliveryError::blocked(
                "DELIVERY_POLICY_STALE",
                "the Project delivery policy changed before the merge",
            )
            .with_attempt(attempt_id.clone());
            self.block_attempt(&attempt, &context, &error).await;
            return Err(error);
        }
        let read_at = now_utc();
        let protection_facts = protection_facts(&protection, read_at);
        let mut checks_facts = check_facts(&checks, read_at);
        checks_facts = mark_protection_checks(checks_facts, &protection_facts);
        let pr_facts = pull_request_facts(&pull_request);
        let candidate_facts = CandidateDeliveryFacts {
            candidate_id: context.candidate.id.clone(),
            candidate_sha: context.candidate.commit_sha.clone(),
            candidate_tree_sha: context.candidate.tree_sha.clone(),
            content_digest: attempt.content_digest.clone(),
            base_commit: context.candidate_base.clone(),
            risk_class: context.risk,
            latest: true,
            salvage_requires_human_review: context.salvage.requires_human_review()
                && context.risk != DeliveryRisk::R0,
        };
        let security_reviewed = self.security_reviewed(&context.candidate.id).await?;
        let charter = self
            .charter_facts(&context.project.id, context.risk)
            .await?;
        let decision_input = DeliveryEvaluationInput {
            policy: Some(&policy),
            candidate: &candidate_facts,
            gate: context.gate.as_ref(),
            pull_request: Some(&pr_facts),
            checks: Some(&checks_facts),
            protection: Some(&protection_facts),
            charter: charter.as_ref(),
            incident_open: context.incident.is_some(),
            incident_detail: context
                .incident
                .as_ref()
                .map(|record| record.detail.as_str()),
            manual_confirmed: request.manual_confirmed,
            base_branch: &context.base_branch,
            base_is_default: context.base_branch == context.binding.default_branch,
            current_base_sha: Some(&current_base),
            security_reviewed,
            now: read_at,
        };
        let decision = evaluate_delivery(&decision_input);
        let decision_record = self
            .record_decision(
                &attempt,
                &policy,
                &context,
                &decision,
                read_at,
                read_at,
                charter.as_ref().map(|facts| facts.grant_id.as_str()),
            )
            .await?;
        self.record_checks(&attempt_id, &checks_facts).await?;
        if !decision.allowed() {
            let error = DeliveryError::blocked(
                delivery_block_kind(
                    decision
                        .reasons
                        .first()
                        .map(|reason| reason.code.as_str())
                        .unwrap_or("DELIVERY_BLOCKED"),
                ),
                decision
                    .reasons
                    .first()
                    .map(|reason| reason.detail.clone())
                    .unwrap_or_else(|| "delivery was denied".into()),
            )
            .with_blockers(decision.reasons.clone())
            .with_attempt(attempt_id.clone());
            self.block_attempt(&attempt, &context, &error).await;
            return Err(error);
        }

        let authorization = decision
            .authorization
            .unwrap_or(DeliveryAuthorization::OwnerManual);
        let (claimed, claimed_attempt) = self.db.claim_delivery_attempt(&attempt_id).await?;
        if !claimed {
            return match claimed_attempt.state {
                DeliveryAttemptState::Merged => {
                    if let Some(merge_sha) = claimed_attempt.merge_commit_sha.as_deref() {
                        self.revalidate_merged_replay(&claimed_attempt, &context, merge_sha)
                            .await?;
                    }
                    Ok(DeliveryRun {
                        attempt: claimed_attempt,
                        policy,
                        decision: decision_record,
                        pull_request: Some(pull_request),
                        checks: Some(checks),
                        protection: Some(protection),
                        issue_outcome: None,
                        replayed: true,
                    })
                }
                DeliveryAttemptState::NeedsAttention => Err(DeliveryError::blocked(
                    "DELIVERY_INCIDENT_OPEN",
                    "delivery needs owner attention",
                )
                .with_attempt(attempt_id)),
                DeliveryAttemptState::Merging => {
                    // A Core restart can leave MERGING durable. Reconcile the
                    // provider before refusing another write.
                    if let Ok(state) = self
                        .github
                        .pull_request_merge_state(
                            self.credential.token(),
                            &context.binding.owner,
                            &context.binding.repo,
                            pull_request.number,
                        )
                        .await
                        && state.merged
                        && state
                            .head_sha
                            .eq_ignore_ascii_case(&context.candidate.commit_sha)
                        && let Some(merge_sha) = state.merge_commit_sha
                    {
                        let attempt = self
                            .db
                            .update_delivery_attempt(
                                &attempt_id,
                                DeliveryAttemptUpdate {
                                    state: Some(DeliveryAttemptState::Merged),
                                    merge_commit_sha: Some(merge_sha.clone()),
                                    merged_at: Some(now_utc().to_rfc3339()),
                                    observed_head_sha: Some(state.head_sha),
                                    observed_base_sha: Some(pull_request.base_sha.clone()),
                                    clear_blocker: true,
                                    ..Default::default()
                                },
                            )
                            .await?;
                        self.revalidate_merged_replay(&attempt, &context, &merge_sha)
                            .await?;
                        return Ok(DeliveryRun {
                            attempt,
                            policy,
                            decision: decision_record,
                            pull_request: Some(pull_request),
                            checks: Some(checks),
                            protection: Some(protection),
                            issue_outcome: None,
                            replayed: true,
                        });
                    }
                    Err(DeliveryError::blocked(
                        "DELIVERY_IN_PROGRESS",
                        "another Core delivery operation already claimed this merge",
                    )
                    .with_attempt(attempt_id))
                }
                _ => Err(DeliveryError::blocked(
                    "DELIVERY_STATE_CONFLICT",
                    "the delivery attempt is not in a claimable state",
                )
                .with_attempt(attempt_id)),
            };
        }
        let attempt = self
            .db
            .update_delivery_attempt(
                &attempt_id,
                DeliveryAttemptUpdate {
                    state: Some(DeliveryAttemptState::Merging),
                    authorization: Some(authorization),
                    charter_grant_id: charter.as_ref().map(|facts| facts.grant_id.clone()),
                    observed_head_sha: Some(pull_request.head_sha.clone()),
                    observed_base_sha: Some(pull_request.base_sha.clone()),
                    clear_blocker: true,
                    ..Default::default()
                },
            )
            .await?;
        self.db
            .record_delivery_attempt_event(NewDeliveryAttemptEvent {
                attempt_id: &attempt_id,
                kind: "AUTHORIZED",
                detail: authorization.as_str(),
            })
            .await?;
        self.db
            .record_delivery_attempt_event(NewDeliveryAttemptEvent {
                attempt_id: &attempt_id,
                kind: "MERGING",
                detail: "merge requested with the exact expected head",
            })
            .await?;
        let merge = match self
            .github
            .merge_pull_request(
                self.credential.token(),
                &context.binding.owner,
                &context.binding.repo,
                MergePullRequest {
                    number: pull_request.number,
                    expected_head_sha: &expected_head_sha,
                    merge_method: GitHubMergeMethod::Squash,
                    commit_title: Some(&format!(
                        "Factory candidate {}",
                        short_sha(&expected_head_sha)
                    )),
                    commit_message: Some(&format!(
                        "Campaign {} candidate {} Gate {}",
                        context.campaign.id, context.candidate.id, gate.gate_revision
                    )),
                },
            )
            .await
        {
            Ok(outcome) => outcome,
            Err(error) => {
                let error = DeliveryError::from(error).with_attempt(attempt_id.clone());
                self.block_attempt(&attempt, &context, &error).await;
                return Err(error);
            }
        };
        let mut merge_sha = merge.merge_commit_sha.clone();
        if merge_sha.is_empty() {
            match self
                .github
                .pull_request_merge_state(
                    self.credential.token(),
                    &context.binding.owner,
                    &context.binding.repo,
                    pull_request.number,
                )
                .await
            {
                Ok(reconciled) if reconciled.merged => {
                    merge_sha = reconciled.merge_commit_sha.unwrap_or_default();
                }
                Ok(_) => {}
                Err(error) => {
                    let error = DeliveryError::from(error).with_attempt(attempt_id.clone());
                    self.block_attempt(&attempt, &context, &error).await;
                    return Err(error);
                }
            }
        }
        if merge_sha.is_empty() {
            let error = DeliveryError::blocked(
                "MERGE_IDENTITY_MISMATCH",
                "GitHub merged but did not return a merge commit SHA",
            )
            .with_attempt(attempt_id.clone());
            let incident = self
                .open_incident(
                    &attempt,
                    &context,
                    "MERGE_IDENTITY_MISMATCH",
                    error.detail(),
                )
                .await?;
            return Err(error.with_attempt(incident.attempt_id));
        }
        if let Ok(pr) = self
            .github
            .pull_request_merge_state(
                self.credential.token(),
                &context.binding.owner,
                &context.binding.repo,
                pull_request.number,
            )
            .await
            && (!pr.merged
                || !pr.head_sha.eq_ignore_ascii_case(&expected_head_sha)
                || pr
                    .merge_commit_sha
                    .as_deref()
                    .is_some_and(|sha| !sha.eq_ignore_ascii_case(&merge_sha)))
        {
            let incident = self
                .open_incident(
                    &attempt,
                    &context,
                    "MERGE_IDENTITY_MISMATCH",
                    "GitHub reported a different merged state, head or merge commit",
                )
                .await?;
            return Err(DeliveryError::blocked(
                "MERGE_IDENTITY_MISMATCH",
                "GitHub reported a different merged state or merge commit",
            )
            .with_attempt(incident.attempt_id));
        }
        let merged_at = now_utc().to_rfc3339();
        let attempt = self
            .db
            .update_delivery_attempt(
                &attempt_id,
                DeliveryAttemptUpdate {
                    state: Some(DeliveryAttemptState::Merged),
                    merge_commit_sha: Some(merge_sha.clone()),
                    merged_at: Some(merged_at.clone()),
                    observed_head_sha: Some(pull_request.head_sha.clone()),
                    observed_base_sha: Some(pull_request.base_sha.clone()),
                    clear_blocker: true,
                    ..Default::default()
                },
            )
            .await?;
        self.db
            .record_delivery_attempt_event(NewDeliveryAttemptEvent {
                attempt_id: &attempt_id,
                kind: "MERGED",
                detail: &merge_sha,
            })
            .await?;
        let branch = match self
            .github
            .branch_commit(
                self.credential.token(),
                &context.binding.owner,
                &context.binding.repo,
                &context.binding.default_branch,
            )
            .await
        {
            Ok(branch) => branch,
            Err(error) => {
                let error = DeliveryError::from(error);
                let incident = self
                    .open_incident(
                        &attempt,
                        &context,
                        "DEFAULT_BRANCH_UNCONFIRMED",
                        error.detail(),
                    )
                    .await?;
                return Err(DeliveryError::blocked(
                    "DEFAULT_BRANCH_UNCONFIRMED",
                    format!(
                        "the protected default branch could not be read after merge: {}",
                        error.detail()
                    ),
                )
                .with_attempt(incident.attempt_id));
            }
        };
        if !branch.commit_sha.eq_ignore_ascii_case(&merge_sha) {
            let incident = self
                .open_incident(
                    &attempt,
                    &context,
                    "MERGE_IDENTITY_MISMATCH",
                    "the protected default branch does not point at the recorded merge commit",
                )
                .await?;
            return Err(DeliveryError::blocked(
                "MERGE_IDENTITY_MISMATCH",
                "the protected default branch does not point at the recorded merge commit",
            )
            .with_attempt(incident.attempt_id));
        }
        if context.risk == DeliveryRisk::R1 {
            match self
                .await_post_merge_check(&attempt, &context, &policy, &merge_sha)
                .await
            {
                Ok(()) => {}
                Err(error) => {
                    let incident = self
                        .open_incident(&attempt, &context, error.kind(), error.detail())
                        .await?;
                    let error = error.with_attempt(incident.attempt_id);
                    let issue_error = IssueDeliveryError::Upstream {
                        kind: error.kind(),
                        detail: error.detail().to_owned(),
                        retryable: false,
                    };
                    let _ = self
                        .hook
                        .on_delivery_failed(
                            &context.campaign.id,
                            &context.candidate.id,
                            &issue_error,
                        )
                        .await;
                    return Err(error);
                }
            }
        }

        if context.base_branch == context.binding.default_branch
            && let Err(error) = self
                .advance_project_base_after_merge(&context, &merge_sha)
                .await
        {
            let incident = self
                .open_incident(
                    &attempt,
                    &context,
                    "PROJECT_BASE_UPDATE_FAILED",
                    error.detail(),
                )
                .await?;
            return Err(error.with_attempt(incident.attempt_id));
        }

        let issue_outcome = match self
            .hook
            .on_merge_succeeded(&context.campaign.id, &context.candidate.id, &merge_sha)
            .await
        {
            Ok(outcome) => {
                self.record_issue_receipts(&attempt_id, &outcome).await?;
                Some(outcome)
            }
            Err(error) => {
                let _ = self
                    .hook
                    .on_delivery_failed(&context.campaign.id, &context.candidate.id, &error)
                    .await;
                None
            }
        };
        Ok(DeliveryRun {
            attempt,
            policy,
            decision: decision_record,
            pull_request: Some(pull_request),
            checks: Some(checks),
            protection: Some(protection),
            issue_outcome,
            replayed: false,
        })
    }

    /// The protected default branch was verified to point at the exact merge
    /// commit after a successful delivery. Advance the Project config base so a
    /// follow-up request plans from merged main, never from the old base. A
    /// concurrent config update is retried with its fresh revision fence; a
    /// non-default branch or invalid config fails closed.
    async fn advance_project_base_after_merge(
        &self,
        context: &DeliveryContext,
        merge_sha: &str,
    ) -> Result<(), DeliveryError> {
        let mut last_conflict: Option<DeliveryError> = None;
        for _ in 0..3 {
            let expected_revision = match self.db.project_config(&context.project.id).await {
                Ok(config) => config.revision,
                Err(DbError::NotFound) => 1,
                Err(error) => return Err(error.into()),
            };
            match self
                .db
                .advance_project_base_after_merged_delivery(
                    &context.project.id,
                    merge_sha,
                    &context.binding.default_branch,
                    expected_revision,
                )
                .await
            {
                Ok(_) => return Ok(()),
                Err(DbError::Conflict(detail)) => {
                    last_conflict = Some(DeliveryError::blocked(
                        "PROJECT_BASE_UPDATE_CONFLICT",
                        detail,
                    ));
                }
                Err(error) => return Err(error.into()),
            }
        }
        Err(last_conflict.unwrap_or_else(|| {
            DeliveryError::blocked(
                "PROJECT_BASE_UPDATE_FAILED",
                "the Project base could not be advanced to the merge commit",
            )
        }))
    }

    /// A replay of a recorded MERGED attempt may repair a missing Project base
    /// advance only after re-proving the original delivery evidence: the
    /// protected default branch still names the exact merge commit, and any
    /// required post-merge check is recorded PASSED for that same merge. A read
    /// error or missing/failed evidence leaves the base untouched.
    async fn revalidate_merged_replay(
        &self,
        attempt: &DeliveryAttemptRecord,
        context: &DeliveryContext,
        merge_sha: &str,
    ) -> Result<(), DeliveryError> {
        if context.base_branch != context.binding.default_branch {
            return Ok(());
        }
        let branch = match self
            .github
            .branch_commit(
                self.credential.token(),
                &context.binding.owner,
                &context.binding.repo,
                &context.binding.default_branch,
            )
            .await
        {
            Ok(branch) => branch,
            Err(error) => {
                let error = DeliveryError::from(error);
                return Err(DeliveryError::blocked(
                    "DEFAULT_BRANCH_UNCONFIRMED",
                    format!(
                        "the protected default branch could not be read during replay: {}",
                        error.detail()
                    ),
                ));
            }
        };
        if !branch.commit_sha.eq_ignore_ascii_case(merge_sha) {
            return Err(DeliveryError::blocked(
                "MERGE_IDENTITY_MISMATCH",
                "the protected default branch no longer points at the recorded merge commit",
            ));
        }
        let policy = self
            .db
            .delivery_policy_revision(&context.project.id, attempt.policy_revision)
            .await?;
        if context.risk == DeliveryRisk::R1 {
            let check_id = policy.post_merge_check_id.trim();
            if check_id.is_empty() {
                return Err(DeliveryError::blocked(
                    "POST_MERGE_CHECK_MISSING",
                    "R1 replay requires a configured post-merge check",
                ));
            }
            let record = match self
                .db
                .delivery_post_merge_check(&attempt.attempt_id, check_id)
                .await
            {
                Ok(record) => record,
                Err(DbError::NotFound) => {
                    return Err(DeliveryError::blocked(
                        "POST_MERGE_CHECK_MISSING",
                        "the required post-merge check has no recorded result",
                    ));
                }
                Err(error) => return Err(error.into()),
            };
            if !record.merge_commit_sha.eq_ignore_ascii_case(merge_sha) {
                return Err(DeliveryError::blocked(
                    "MERGE_IDENTITY_MISMATCH",
                    "the recorded post-merge check is bound to another merge commit",
                ));
            }
            match record.status.as_str() {
                "PASSED" => {}
                "FAILED" => {
                    return Err(DeliveryError::blocked(
                        "POST_MERGE_CHECK_FAILED",
                        "the recorded post-merge check failed",
                    ));
                }
                _ => {
                    return Err(DeliveryError::blocked(
                        "POST_MERGE_CHECK_MISSING",
                        "the recorded post-merge check did not pass",
                    ));
                }
            }
        }
        self.advance_project_base_after_merge(context, merge_sha)
            .await
    }

    pub async fn record_manual_revert(
        &self,
        incident_id: &str,
        revert_pr_number: u64,
        revert_merge_sha: &str,
        actor: &str,
        rationale: &str,
    ) -> Result<DeliveryRevertRecord, DeliveryError> {
        self.db
            .record_delivery_revert(
                incident_id,
                revert_pr_number,
                revert_merge_sha,
                actor,
                rationale,
            )
            .await
            .map_err(Into::into)
    }

    async fn load_context(
        &self,
        request: &DeliveryRequest<'_>,
    ) -> Result<DeliveryContext, DeliveryError> {
        let campaign = self.db.campaign(request.campaign_id).await?;
        if matches!(campaign.status.as_str(), "CANCELLED" | "FAILED") {
            return Err(DeliveryError::blocked(
                "CANDIDATE_SUPERSEDED",
                "the Campaign is no longer deliverable",
            ));
        }
        let candidate = self.db.candidate(request.candidate_id).await?;
        let attempts = self.db.attempts().await?;
        let attempt = attempts
            .iter()
            .find(|attempt| attempt.id == candidate.attempt_id)
            .ok_or_else(|| {
                DeliveryError::blocked("NOT_FOUND", "candidate Attempt was not found")
            })?;
        if matches!(attempt.status.as_str(), "FAILED" | "CANCELLED") {
            return Err(DeliveryError::blocked(
                "CANDIDATE_SUPERSEDED",
                "the candidate Attempt is no longer deliverable",
            ));
        }
        let runs = self.db.runs().await?;
        let run = runs
            .iter()
            .find(|run| run.id == attempt.run_id)
            .ok_or_else(|| DeliveryError::blocked("NOT_FOUND", "candidate Run was not found"))?;
        if run.campaign_id != campaign.id {
            return Err(DeliveryError::blocked(
                "CANDIDATE_SUPERSEDED",
                "candidate Run is bound to another Campaign",
            ));
        }
        let action = self.db.action(&campaign.action_id).await?;
        let project = self.db.project(&action.project_id).await?;
        let binding =
            self.db
                .project_github_binding(&project.id)
                .await
                .map_err(|error| match error {
                    DbError::NotFound => DeliveryError::blocked(
                        "NOT_BOUND",
                        "Project is not bound to a GitHub repository",
                    ),
                    other => other.into(),
                })?;
        let base_branch = request
            .base_branch
            .map(str::to_owned)
            .unwrap_or_else(|| binding.default_branch.clone());
        crate::github::validate_default_branch(&base_branch)
            .map_err(|_| DeliveryError::blocked("BASE_MISMATCH", "base branch is invalid"))?;
        let candidate_base = self.db.candidate_base_commit(&candidate.id).await?;
        let salvage = self
            .db
            .candidate_salvage_record(&candidate.id)
            .await
            .map_err(|error| match error {
                DbError::Invalid(_) => DeliveryError::blocked(
                    "SALVAGE_EVIDENCE_REQUIRED",
                    "stagnation salvage evidence is missing or malformed",
                ),
                other => other.into(),
            })?;
        if salvage.final_source == CandidateFinalSource::StagnationSalvage
            && salvage.evidence.is_none()
        {
            return Err(DeliveryError::blocked(
                "SALVAGE_EVIDENCE_REQUIRED",
                "a stagnation salvage candidate has no exact salvage evidence",
            ));
        }
        let (verified, changed_paths) =
            verify_candidate(&project, &candidate_base, &candidate).await?;
        if !verified
            .commit_sha
            .eq_ignore_ascii_case(&candidate.commit_sha)
            || !verified.tree_sha.eq_ignore_ascii_case(&candidate.tree_sha)
        {
            return Err(DeliveryError::blocked(
                "CANDIDATE_SUPERSEDED",
                "candidate commit or tree no longer resolves exactly",
            ));
        }
        if let Some(evidence) = salvage.evidence.as_ref() {
            let mut recorded = evidence.changed_paths.clone();
            let mut actual = changed_paths.clone();
            recorded.sort();
            actual.sort();
            if recorded != actual {
                return Err(DeliveryError::blocked(
                    "SALVAGE_EVIDENCE_REQUIRED",
                    "stagnation salvage evidence does not match the exact candidate diff",
                ));
            }
        }
        let authorized = match DeliveryRisk::parse(&action.risk) {
            Some(risk) => risk,
            None => self
                .db
                .campaign_authorization(&campaign.id)
                .await
                .ok()
                .and_then(|record| DeliveryRisk::parse(&record.risk_class))
                .unwrap_or(DeliveryRisk::Unknown),
        };
        let risk = revalidate_risk(authorized, &changed_paths);
        if salvage.requires_human_review() && risk != DeliveryRisk::R0 {
            return Err(DeliveryError::blocked(
                "SALVAGE_HUMAN_REVIEW_REQUIRED",
                "R1/R2/R3 stagnation salvage requires authenticated human review and is never auto-merged",
            ));
        }
        let gate_record = self.db.gate_for_candidate(&candidate.id).await?;
        let gate = gate_record.map(|record| GateDeliveryFacts {
            candidate_id: record.candidate_id,
            candidate_sha: candidate.commit_sha.clone(),
            gate_revision: record.decided_at,
            outcome: record.outcome,
            current: true,
        });
        let policy = self.db.delivery_policy(&project.id).await?;
        let incident = self.db.active_delivery_incident(&project.id).await?;
        Ok(DeliveryContext {
            campaign,
            candidate,
            project,
            binding,
            gate,
            risk,
            salvage,
            candidate_base,
            base_branch,
            policy,
            incident,
        })
    }

    async fn ensure_credential_and_permission(
        &self,
        context: &DeliveryContext,
    ) -> Result<(), DeliveryError> {
        let connection = self
            .db
            .github_connection()
            .await
            .map_err(|error| match error {
                DbError::NotFound => DeliveryError::new(
                    "DELIVERY_CREDENTIAL_UNAVAILABLE",
                    "DELIVERY_PERMISSION_REQUIRED",
                    "GitHub is not connected; delivery is disabled",
                ),
                other => other.into(),
            })?;
        if !connection.enabled || connection.credential_ciphertext.is_empty() {
            return Err(DeliveryError::new(
                "DELIVERY_CREDENTIAL_UNAVAILABLE",
                "DELIVERY_PERMISSION_REQUIRED",
                "the GitHub connection is revoked; delivery is disabled",
            ));
        }
        self.db
            .require_github_connection_revision(self.credential.revision())
            .await
            .map_err(|_| {
                DeliveryError::new(
                    "DELIVERY_CREDENTIAL_UNAVAILABLE",
                    "DELIVERY_PERMISSION_REQUIRED",
                    "the GitHub credential revision is no longer current",
                )
            })?;
        let repository = self
            .github
            .repository(
                self.credential.token(),
                &context.binding.owner,
                &context.binding.repo,
            )
            .await?;
        if !branch_permission_ok(repository.permissions) {
            return Err(DeliveryError::permission_required(format!(
                "the Core GitHub token has read-only permissions for {}",
                context.binding.repo_full_name
            )));
        }
        if repository.permissions.is_none()
            && connection.scopes_reported
            && !connection.scopes.iter().any(|scope| {
                scope.eq_ignore_ascii_case("repo") || scope.eq_ignore_ascii_case("public_repo")
            })
        {
            return Err(DeliveryError::permission_required(format!(
                "the Core GitHub token scopes do not include write access for {}",
                context.binding.repo_full_name
            )));
        }
        Ok(())
    }

    async fn open_or_reuse_pull_request(
        &self,
        context: &DeliveryContext,
        branch: &str,
    ) -> Result<OpenedPullRequest, DeliveryError> {
        let branch_commit = self
            .github
            .branch_commit(
                self.credential.token(),
                &context.binding.owner,
                &context.binding.repo,
                branch,
            )
            .await?;
        if !branch_commit
            .commit_sha
            .eq_ignore_ascii_case(&context.candidate.commit_sha)
        {
            return Err(DeliveryError::blocked(
                "HEAD_MISMATCH",
                "the remote branch does not resolve to the exact candidate commit",
            ));
        }
        let open = self
            .github
            .open_pull_requests_for_head(
                self.credential.token(),
                &context.binding.owner,
                &context.binding.repo,
                branch,
            )
            .await?;
        if let Some(existing) = open.into_iter().find(|pull_request| {
            pull_request
                .head_sha
                .eq_ignore_ascii_case(&context.candidate.commit_sha)
                && pull_request.base_ref == context.base_branch
        }) {
            return Ok(OpenedPullRequest {
                pull_request: existing,
                created: false,
                issue_outcome: None,
            });
        }
        let issue_numbers = self
            .db
            .issue_links_for_campaign(&context.campaign.id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|link| link.issue_number)
            .collect::<Vec<_>>();
        let body = pull_request_body(context, &issue_numbers);
        let title = format!(
            "Factory Campaign {} candidate {}",
            short_id_text(&context.campaign.id),
            short_sha(&context.candidate.commit_sha)
        );
        let pull_request = match self
            .github
            .create_pull_request(
                self.credential.token(),
                &context.binding.owner,
                &context.binding.repo,
                NewPullRequest {
                    title: &title,
                    body: &body,
                    head: branch,
                    base: &context.base_branch,
                    draft: false,
                },
            )
            .await
        {
            Ok(pull_request) => pull_request,
            Err(error @ GitHubError::Rejected { .. }) => {
                // A concurrent open is an idempotent success when the exact
                // head/base pair now exists.
                let open = self
                    .github
                    .open_pull_requests_for_head(
                        self.credential.token(),
                        &context.binding.owner,
                        &context.binding.repo,
                        branch,
                    )
                    .await?;
                if let Some(existing) = open.into_iter().find(|pull_request| {
                    pull_request
                        .head_sha
                        .eq_ignore_ascii_case(&context.candidate.commit_sha)
                        && pull_request.base_ref == context.base_branch
                }) {
                    return Ok(OpenedPullRequest {
                        pull_request: existing,
                        created: false,
                        issue_outcome: None,
                    });
                }
                return Err(error.into());
            }
            Err(error) => return Err(error.into()),
        };
        if !pull_request
            .head_sha
            .eq_ignore_ascii_case(&context.candidate.commit_sha)
        {
            return Err(DeliveryError::blocked(
                "HEAD_MISMATCH",
                "GitHub created a pull request whose head is not the exact candidate",
            ));
        }
        if pull_request.base_ref != context.base_branch {
            return Err(DeliveryError::blocked(
                "BASE_MISMATCH",
                "GitHub created a pull request against a different base branch",
            ));
        }
        Ok(OpenedPullRequest {
            pull_request,
            created: true,
            issue_outcome: None,
        })
    }

    async fn security_reviewed(&self, candidate_id: &str) -> Result<bool, DeliveryError> {
        let candidate = self.db.candidate(candidate_id).await?;
        let receipts = self
            .db
            .quality_receipts(candidate_id)
            .await
            .unwrap_or_default();
        Ok(receipts.iter().any(|receipt| {
            receipt.reviewer_role == Some(ReviewerRole::Security)
                && receipt.result == ReceiptResult::Pass
                && receipt
                    .commit_sha
                    .eq_ignore_ascii_case(&candidate.commit_sha)
        }))
    }

    async fn charter_facts(
        &self,
        project_id: &str,
        risk: DeliveryRisk,
    ) -> Result<Option<CharterDeliveryFacts>, DeliveryError> {
        let Some(charter) = self.db.active_agentic_charter(project_id).await? else {
            return Ok(None);
        };
        let now = Utc::now().to_rfc3339();
        let assurance = match risk {
            DeliveryRisk::R0 => crate::agentic::Assurance::None,
            DeliveryRisk::R1 => crate::agentic::Assurance::Standard,
            DeliveryRisk::R2 => crate::agentic::Assurance::Elevated,
            DeliveryRisk::R3 | DeliveryRisk::Unknown => crate::agentic::Assurance::Critical,
        };
        let covers = charter
            .to_owner_charter()
            .covers(project_id, "delivery", assurance, &now)
            && !charter
                .reserved_matters
                .iter()
                .any(|matter| matter == "delivery");
        let expired_or_revoked = charter.revoked_at.is_some() || now >= charter.expires_at;
        Ok(Some(CharterDeliveryFacts {
            grant_id: charter.id,
            delivery_autonomy: charter.delivery_autonomy,
            covers,
            expired_or_revoked,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    async fn record_decision(
        &self,
        attempt: &DeliveryAttemptRecord,
        policy: &DeliveryPolicy,
        context: &DeliveryContext,
        decision: &DeliveryDecision,
        checks_read_at: DateTime<Utc>,
        protection_read_at: DateTime<Utc>,
        charter_grant_id: Option<&str>,
    ) -> Result<DeliveryDecisionRecord, DeliveryError> {
        self.db
            .record_delivery_decision(NewDeliveryDecision {
                attempt_id: &attempt.attempt_id,
                candidate_id: &context.candidate.id,
                candidate_sha: &context.candidate.commit_sha,
                gate_revision: &attempt.gate_revision,
                gate_outcome: &attempt.gate_outcome,
                policy_revision: policy.revision,
                policy_sha256: &policy.policy_sha256,
                charter_grant_id,
                risk_class: context.risk.as_str(),
                expected_head_sha: &attempt.expected_head_sha,
                base_branch: &attempt.base_branch,
                checks_read_at: &checks_read_at.to_rfc3339(),
                branch_protection_read_at: &protection_read_at.to_rfc3339(),
                decision: &decision.decision,
                reasons: &decision.reasons,
            })
            .await
            .map_err(Into::into)
    }

    async fn record_allow_decision(
        &self,
        attempt: &DeliveryAttemptRecord,
        policy: &DeliveryPolicy,
        context: &DeliveryContext,
        _pull_request: Option<&GitHubPullRequest>,
        checks_read_at: DateTime<Utc>,
        protection_read_at: DateTime<Utc>,
    ) -> Result<DeliveryDecisionRecord, DeliveryError> {
        self.record_decision(
            attempt,
            policy,
            context,
            &DeliveryDecision {
                decision: "ALLOW".into(),
                authorization: attempt.authorization,
                reasons: Vec::new(),
            },
            checks_read_at,
            protection_read_at,
            None,
        )
        .await
    }

    async fn record_checks(
        &self,
        attempt_id: &str,
        checks: &ChecksDeliveryFacts,
    ) -> Result<(), DeliveryError> {
        let receipts = checks
            .checks
            .iter()
            .map(|check| DeliveryCheckReceiptRecord {
                attempt_id: attempt_id.to_owned(),
                check_name: check.name.clone(),
                head_sha: checks.head_sha.clone(),
                state: check.state.clone(),
                conclusion: check.conclusion.clone(),
                required_by_protection: check.required_by_protection,
                observed_at: checks.read_at.to_rfc3339(),
            })
            .collect::<Vec<_>>();
        self.db
            .record_delivery_check_receipts(&receipts)
            .await
            .map_err(Into::into)
    }

    async fn record_issue_receipts(
        &self,
        attempt_id: &str,
        outcome: &IssueDeliveryOutcome,
    ) -> Result<(), DeliveryError> {
        let mut receipts = Vec::new();
        for link in &outcome.links {
            receipts.push(DeliveryIssueReceiptRecord {
                receipt_id: format!("{attempt_id}:{}:summary", link.link.id),
                attempt_id: attempt_id.to_owned(),
                issue_link_id: Some(link.link.id.clone()),
                action: "SUMMARY".into(),
                state: link.link.delivery_state.as_str().into(),
                external_id: link
                    .events
                    .iter()
                    .rev()
                    .find_map(|event| event.external_id.clone()),
                detail: link
                    .link
                    .latest_error
                    .as_ref()
                    .map(|error| error.detail.clone()),
                recorded_at: now_utc().to_rfc3339(),
            });
        }
        self.db
            .record_delivery_issue_receipts(&receipts)
            .await
            .map_err(Into::into)
    }

    async fn block_attempt(
        &self,
        attempt: &DeliveryAttemptRecord,
        context: &DeliveryContext,
        error: &DeliveryError,
    ) {
        let _ = self
            .db
            .update_delivery_attempt(
                &attempt.attempt_id,
                DeliveryAttemptUpdate {
                    state: Some(DeliveryAttemptState::Blocked),
                    blocker_code: Some(error.kind().into()),
                    blocker_detail: Some(error.detail().into()),
                    ..Default::default()
                },
            )
            .await;
        let _ = self
            .db
            .record_delivery_attempt_event(NewDeliveryAttemptEvent {
                attempt_id: &attempt.attempt_id,
                kind: "BLOCKED",
                detail: error.kind(),
            })
            .await;
        let issue_error = IssueDeliveryError::Upstream {
            kind: error.kind(),
            detail: error.detail().to_owned(),
            retryable: false,
        };
        let _ = self
            .hook
            .on_delivery_failed(&context.campaign.id, &context.candidate.id, &issue_error)
            .await;
    }

    async fn fail_attempt(
        &self,
        attempt: &DeliveryAttemptRecord,
        context: &DeliveryContext,
        error: &DeliveryError,
    ) {
        self.block_attempt(attempt, context, error).await;
    }

    async fn open_incident(
        &self,
        attempt: &DeliveryAttemptRecord,
        context: &DeliveryContext,
        kind: &str,
        detail: &str,
    ) -> Result<DeliveryIncidentRecord, DeliveryError> {
        let incident = self
            .db
            .open_delivery_incident(NewDeliveryIncident {
                project_id: &context.project.id,
                attempt_id: &attempt.attempt_id,
                candidate_id: &context.candidate.id,
                merge_commit_sha: attempt.merge_commit_sha.as_deref().unwrap_or_default(),
                kind,
                detail,
            })
            .await?;
        let _ = self
            .db
            .update_delivery_attempt(
                &attempt.attempt_id,
                DeliveryAttemptUpdate {
                    state: Some(DeliveryAttemptState::NeedsAttention),
                    incident_id: Some(incident.incident_id.clone()),
                    blocker_code: Some(kind.into()),
                    blocker_detail: Some(detail.into()),
                    ..Default::default()
                },
            )
            .await;
        let _ = self
            .db
            .record_delivery_attempt_event(NewDeliveryAttemptEvent {
                attempt_id: &attempt.attempt_id,
                kind: "INCIDENT_OPENED",
                detail: kind,
            })
            .await;
        Ok(incident)
    }

    async fn await_post_merge_check(
        &self,
        attempt: &DeliveryAttemptRecord,
        context: &DeliveryContext,
        policy: &DeliveryPolicy,
        merge_sha: &str,
    ) -> Result<(), DeliveryError> {
        let check_id = policy.post_merge_check_id.trim();
        if check_id.is_empty() {
            return Err(DeliveryError::blocked(
                "POST_MERGE_CHECK_MISSING",
                "R1 delivery requires a configured post-merge check",
            ));
        }
        let deadline = tokio::time::Instant::now()
            + Duration::from_secs(
                policy
                    .post_merge_timeout_secs
                    .min(MAX_POST_MERGE_TIMEOUT_SECS),
            );
        loop {
            match self
                .github
                .pull_request_checks(
                    self.credential.token(),
                    &context.binding.owner,
                    &context.binding.repo,
                    merge_sha,
                )
                .await
            {
                Ok(checks) => {
                    if let Some(result) = post_merge_result(&checks, check_id) {
                        if result.0 {
                            self.db
                                .record_delivery_post_merge_check(
                                    &attempt.attempt_id,
                                    check_id,
                                    merge_sha,
                                    "PASSED",
                                    &result.1,
                                )
                                .await?;
                            return Ok(());
                        }
                        if result.2 {
                            self.db
                                .record_delivery_post_merge_check(
                                    &attempt.attempt_id,
                                    check_id,
                                    merge_sha,
                                    "FAILED",
                                    &result.1,
                                )
                                .await?;
                            return Err(DeliveryError::blocked(
                                "POST_MERGE_CHECK_FAILED",
                                format!(
                                    "post-merge check {check_id:?} failed after merge: {}",
                                    result.1
                                ),
                            ));
                        }
                    }
                }
                Err(error) => {
                    let _ = self
                        .db
                        .record_delivery_post_merge_check(
                            &attempt.attempt_id,
                            check_id,
                            merge_sha,
                            "MISSING",
                            &error.detail(),
                        )
                        .await;
                }
            }
            if tokio::time::Instant::now() >= deadline {
                self.db
                    .record_delivery_post_merge_check(
                        &attempt.attempt_id,
                        check_id,
                        merge_sha,
                        "MISSING",
                        "post-merge check did not report success before the timeout",
                    )
                    .await?;
                return Err(DeliveryError::blocked(
                    "POST_MERGE_CHECK_MISSING",
                    format!(
                        "post-merge check {check_id:?} did not report success before the timeout"
                    ),
                ));
            }
            tokio::time::sleep(POST_MERGE_POLL_INTERVAL).await;
        }
    }
}

fn post_merge_result(
    checks: &GitHubPullRequestChecks,
    check_id: &str,
) -> Option<(bool, String, bool)> {
    if let Some(run) = checks.check_runs.iter().find(|run| run.name == check_id) {
        let success = run.status == "completed"
            && matches!(run.conclusion.as_str(), "success" | "neutral" | "skipped");
        let failed = run.status == "completed" && !success;
        return Some((
            success,
            format!("{} {}", run.status, run.conclusion),
            failed,
        ));
    }
    if let Some(status) = checks
        .statuses
        .iter()
        .find(|status| status.context == check_id)
    {
        let success = status.state == "success";
        let failed = matches!(status.state.as_str(), "failure" | "error");
        return Some((success, status.state.clone(), failed));
    }
    None
}

fn delivery_block_kind(code: &str) -> &'static str {
    match code {
        "DELIVERY_POLICY_REQUIRED" => "DELIVERY_POLICY_REQUIRED",
        "DELIVERY_POLICY_DISABLED" => "DELIVERY_POLICY_DISABLED",
        "DELIVERY_POLICY_STALE" => "DELIVERY_POLICY_STALE",
        "GATE_NOT_ACCEPTED" => "GATE_NOT_ACCEPTED",
        "RISK_NOT_ELIGIBLE" => "RISK_NOT_ELIGIBLE",
        "R1_OPT_IN_REQUIRED" => "R1_OPT_IN_REQUIRED",
        "CHARTER_GRANT_REQUIRED" => "CHARTER_GRANT_REQUIRED",
        "CHARTER_GRANT_EXPIRED" => "CHARTER_GRANT_EXPIRED",
        "CANDIDATE_SUPERSEDED" => "CANDIDATE_SUPERSEDED",
        "HEAD_MISMATCH" => "HEAD_MISMATCH",
        "BASE_MISMATCH" => "BASE_MISMATCH",
        "BASE_NOT_DEFAULT" => "BASE_NOT_DEFAULT",
        "PR_NOT_OPEN" => "PR_NOT_OPEN",
        "PR_DRAFT" => "PR_DRAFT",
        "PR_ALREADY_MERGED" => "PR_ALREADY_MERGED",
        "CHECKS_NOT_READY" => "CHECKS_NOT_READY",
        "CHECKS_FAILED" => "CHECKS_FAILED",
        "CHECKS_STALE" => "CHECKS_STALE",
        "BRANCH_PROTECTION_UNVERIFIED" => "BRANCH_PROTECTION_UNVERIFIED",
        "MERGE_BLOCKED" => "MERGE_BLOCKED",
        "MERGE_IDENTITY_MISMATCH" => "MERGE_IDENTITY_MISMATCH",
        "POST_MERGE_CHECK_MISSING" => "POST_MERGE_CHECK_MISSING",
        "POST_MERGE_CHECK_FAILED" => "POST_MERGE_CHECK_FAILED",
        "DELIVERY_INCIDENT_OPEN" => "DELIVERY_INCIDENT_OPEN",
        "DELIVERY_CREDENTIAL_UNAVAILABLE" => "DELIVERY_CREDENTIAL_UNAVAILABLE",
        "IDEMPOTENCY_CONFLICT" => "IDEMPOTENCY_CONFLICT",
        "OWNER_APPROVAL_REQUIRED" => "OWNER_APPROVAL_REQUIRED",
        "SALVAGE_HUMAN_REVIEW_REQUIRED" => "SALVAGE_HUMAN_REVIEW_REQUIRED",
        "SALVAGE_EVIDENCE_REQUIRED" => "SALVAGE_EVIDENCE_REQUIRED",
        "FINAL_SOURCE_REQUIRED" => "FINAL_SOURCE_REQUIRED",
        _ => "DELIVERY_BLOCKED",
    }
}

fn gate_outcome_str(outcome: GateOutcome) -> &'static str {
    match outcome {
        GateOutcome::Accepted => "ACCEPTED",
        GateOutcome::RepairRequired => "REPAIR_REQUIRED",
        GateOutcome::Blocked => "BLOCKED",
        GateOutcome::HumanRequired => "HUMAN_REQUIRED",
        GateOutcome::NotTested => "NOT_TESTED",
    }
}

fn pull_request_body(context: &DeliveryContext, issue_numbers: &[u64]) -> String {
    let issues = if issue_numbers.is_empty() {
        "none selected".to_owned()
    } else {
        issue_numbers
            .iter()
            .map(|number| format!("#{number}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "Factory delivery for Campaign `{}`.\n\n- Candidate: `{}`\n- Project: `{}/{}`\n- Base branch: `{}`\n- Quality Gate: `ACCEPTED` at `{}`\n- Issue references: {}\n- Refs: campaign/{} candidate/{}\n\nThis pull request is a Core delivery receipt and does not grant merge authority. It intentionally uses no closing keywords; Factory closes an issue only after a Core-verified merge and the issue delivery hook.\n",
        context.campaign.id,
        context.candidate.commit_sha,
        context.binding.owner,
        context.binding.repo,
        context.base_branch,
        context
            .gate
            .as_ref()
            .map(|gate| gate.gate_revision.as_str())
            .unwrap_or("missing"),
        issues,
        context.campaign.id,
        context.candidate.id,
    )
}

fn short_sha(sha: &str) -> &str {
    sha.get(..12).unwrap_or(sha)
}

fn short_id_text(value: &str) -> String {
    value.chars().take(12).collect()
}

fn now_utc() -> DateTime<Utc> {
    Utc::now()
}

#[cfg(test)]
#[path = "delivery_tests.rs"]
mod delivery_tests;
