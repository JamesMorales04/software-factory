//! Deterministic progress supervision for a Campaign work item.
//!
//! Execution Policy V2 does not cap a legitimate Campaign with turns, attempts,
//! sessions, repair cycles or a universal wall-clock deadline. Instead Core
//! observes durable engineering progress and decides whether to continue,
//! diagnose, replan, escalate, wait or complete.
//!
//! Decision mechanism: `DETERMINISTIC`. Classification, stagnation arithmetic,
//! deduplication and continuation decisions are mechanical. Diagnosis may use a
//! model (`MODEL`) and Core validates any proposed replan (`HYBRID`), but model
//! output never grants authority or acceptance.
//!
//! The engine is persistence-agnostic. `ProgressSupervisorState` is the durable
//! snapshot; replaying the same evidence after a restart never earns progress
//! because evidence identity, outcome digests, candidate history, blocker keys
//! and resolved findings are part of that snapshot.
//!
//! Core integration contract (Director owns `bin/core.rs` wiring):
//! - `POST /worker/progress_checkpoint` (Worker session auth): body
//!   `{ attempt_id, worker_id, generation, campaign_id, work_item_id,
//!   checkpoint, evidence, gate }`; call
//!   `Database::record_worker_progress_checkpoint` and return
//!   [`ProgressCheckpointResponse`].
//! - `POST /api/campaigns/{campaign_id}/progress/{work_item_id}/resolve`
//!   (operator auth): call `Database::resolve_progress_hold`.
//! - `GET /api/campaigns/{campaign_id}/progress` (operator projection): return
//!   the durable snapshot and open hold from
//!   `Database::campaign_progress_state` / `Database::progress_hold`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::execution_policy::{
    ContinuationDecision, ProgressCheckpoint, ProgressEvidence, ProgressKind, ProgressPolicy,
};

/// Upper bound on concurrently supervised work items in one Campaign snapshot.
/// This is a representation bound, not a lifetime work limit: completing a
/// WorkGraph and validating a new graph revision authorizes more units.
pub const MAX_SUPERVISED_WORK_ITEMS: usize = 512;
/// Bound on evidence attached to one checkpoint.
pub const MAX_EVIDENCE_PER_CHECKPOINT: usize = 32;
/// Bound on remembered candidate artifacts per work item. Older entries are
/// dropped; the digests relevant to oscillation detection are the recent ones.
pub const MAX_TRACKED_ARTIFACTS: usize = 128;
/// Bound on remembered evidence keys in the active projection. Older keys are
/// evicted FIFO; durable `progress_evidence` uniqueness still rejects replays
/// after eviction.
pub const MAX_TRACKED_EVIDENCE: usize = 4_096;

const STATE_SCHEMA_VERSION: u32 = 1;
const PROVISIONAL_VALIDATION_WINDOW: u32 = 2;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProgressError {
    #[error("invalid progress policy: {0}")]
    InvalidPolicy(String),
    #[error("invalid progress evidence: {0}")]
    InvalidEvidence(String),
    #[error("progress state overflow")]
    Overflow,
    #[error("progress state is invalid: {0}")]
    InvalidState(String),
    #[error(
        "progress work-item capacity reached ({active} active); retire gate-accepted work items"
    )]
    WorkItemRetirementRequired { active: usize },
}

/// A durable Core Quality Gate record for one exact candidate. Acceptance can
/// only come from existing deterministic gate evidence; a Worker, model or
/// progress checkpoint cannot accept work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GateRecord {
    pub gate_id: String,
    pub campaign_id: String,
    pub work_item_id: String,
    pub candidate_sha256: String,
    /// Only `ACCEPTED` completes work. Other outcomes are evidence of no
    /// acceptance and never complete a work item.
    pub outcome: String,
}

impl GateRecord {
    fn accepts(&self, campaign_id: &str, work_item_id: &str, candidate_sha256: &str) -> bool {
        self.outcome == "ACCEPTED"
            && !self.gate_id.trim().is_empty()
            && self.campaign_id == campaign_id
            && self.work_item_id == work_item_id
            && candidate_identity_matches(&self.candidate_sha256, candidate_sha256)
    }
}

/// Progress evidence always carries the 64-character SHA-256 digest. A Core
/// gate may carry either that digest or the raw Git commit id (40 characters
/// for SHA-1 object ids, 64 for SHA-256 object ids), so both forms are
/// accepted without weakening the evidence digest check.
fn candidate_identity_matches(candidate: &str, progress_digest: &str) -> bool {
    let candidate = candidate.to_ascii_lowercase();
    candidate == progress_digest || sha256_hex(candidate.as_bytes()) == progress_digest
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckpointClassification {
    /// Durable advancement that resets stagnation.
    Verified,
    /// A changed candidate that still needs validated advancement within the
    /// next two checkpoints.
    Provisional,
    /// No durable advancement; the stagnation counter advanced.
    Stagnant,
    /// The exact outcome digest was already recorded; restart/replay earns
    /// nothing and never changes counters.
    Replayed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RejectedEvidence {
    pub evidence_id: String,
    pub reason: String,
}

/// Result of one terminal operation checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CheckpointOutcome {
    pub checkpoint_id: String,
    pub work_item_id: String,
    pub classification: CheckpointClassification,
    pub decision: ContinuationDecision,
    pub rejected: Vec<RejectedEvidence>,
    pub stagnant_checkpoints: u32,
    pub unsuccessful_replans: u32,
    pub verified_advances: u64,
}

/// Durable counters for one supervised work item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkItemProgressState {
    pub work_item_id: String,
    pub stagnant_checkpoints: u32,
    pub unsuccessful_replans: u32,
    pub verified_advances: u64,
    pub last_sequence: u64,
    pub provisional_candidate: Option<String>,
    pub provisional_age: u32,
    pub candidate_history: Vec<String>,
    pub test_suite: Option<String>,
    pub test_result: Option<String>,
    #[serde(default)]
    pub test_outcome: Option<String>,
    pub graph_revision: Option<String>,
    pub resolved_findings: BTreeSet<String>,
    pub repaired_episodes: BTreeSet<String>,
    pub accepted_units: BTreeSet<String>,
    pub seen_evidence: BTreeSet<String>,
    #[serde(default)]
    pub evidence_order: VecDeque<String>,
    pub blocker_keys: BTreeSet<String>,
    pub seen_outcomes: BTreeSet<String>,
}

impl WorkItemProgressState {
    pub(crate) fn new(work_item_id: &str) -> Self {
        Self {
            work_item_id: work_item_id.to_owned(),
            stagnant_checkpoints: 0,
            unsuccessful_replans: 0,
            verified_advances: 0,
            last_sequence: 0,
            provisional_candidate: None,
            provisional_age: 0,
            candidate_history: Vec::new(),
            test_suite: None,
            test_result: None,
            test_outcome: None,
            graph_revision: None,
            resolved_findings: BTreeSet::new(),
            repaired_episodes: BTreeSet::new(),
            accepted_units: BTreeSet::new(),
            seen_evidence: BTreeSet::new(),
            evidence_order: VecDeque::new(),
            blocker_keys: BTreeSet::new(),
            seen_outcomes: BTreeSet::new(),
        }
    }

    /// Remember one evidence key, evicting the oldest key when the active
    /// projection is full. The durable evidence table remains the authoritative
    /// replay guard.
    fn remember_evidence(&mut self, key: String) {
        if !self.seen_evidence.insert(key.clone()) {
            return;
        }
        self.evidence_order.push_back(key);
        while self.evidence_order.len() > MAX_TRACKED_EVIDENCE {
            if let Some(oldest) = self.evidence_order.pop_front() {
                self.seen_evidence.remove(&oldest);
            }
        }
    }

    fn remember_candidate(&mut self, sha: &str) {
        if !self.candidate_history.iter().any(|seen| seen == sha) {
            self.candidate_history.push(sha.to_owned());
            if self.candidate_history.len() > MAX_TRACKED_ARTIFACTS {
                self.candidate_history.remove(0);
            }
        }
    }
}

/// Serialized supervisor snapshot. Persist this atomically with the checkpoint
/// rows so restart cannot reset stagnation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProgressSupervisorState {
    pub schema_version: u32,
    pub policy: ProgressPolicy,
    pub work_items: BTreeMap<String, WorkItemProgressState>,
}

/// Worker -> Core checkpoint request. Core validates the Worker lease, loads
/// the frozen Campaign policy and persists the decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProgressCheckpointRequest {
    pub campaign_id: String,
    pub work_item_id: String,
    pub checkpoint: ProgressCheckpoint,
    pub evidence: Vec<ProgressEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<GateRecord>,
}

/// Core's authoritative continuation decision for one checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProgressCheckpointResponse {
    pub checkpoint_id: String,
    pub decision: ContinuationDecision,
    pub classification: CheckpointClassification,
    pub stagnant_checkpoints: u32,
    pub unsuccessful_replans: u32,
    pub hold_open: bool,
}

/// A durable campaign-level continuation hold created from a deterministic
/// Diagnose/Escalate decision. While unresolved, Core does not dispatch another
/// attempt for the work item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProgressHoldRecord {
    pub hold_id: String,
    pub campaign_id: String,
    pub work_item_id: String,
    pub state: String,
    pub reason: String,
    pub checkpoint_id: Option<String>,
    pub created_at: String,
    pub resolved_at: Option<String>,
    pub resolution: Option<String>,
    pub resolved_by: Option<String>,
}

/// Deterministic supervisor for one Campaign.
#[derive(Debug, Clone)]
pub struct ProgressSupervisor {
    policy: ProgressPolicy,
    work_items: BTreeMap<String, WorkItemProgressState>,
}

impl ProgressSupervisor {
    pub fn new(policy: ProgressPolicy) -> Result<Self, ProgressError> {
        validate_policy(&policy)?;
        Ok(Self {
            policy,
            work_items: BTreeMap::new(),
        })
    }

    pub fn policy(&self) -> ProgressPolicy {
        self.policy
    }

    pub fn restore(
        policy: ProgressPolicy,
        state: ProgressSupervisorState,
    ) -> Result<Self, ProgressError> {
        validate_policy(&policy)?;
        if state.schema_version != STATE_SCHEMA_VERSION {
            return Err(ProgressError::InvalidState(
                "unsupported progress supervisor schema".into(),
            ));
        }
        if state.policy != policy {
            return Err(ProgressError::InvalidState(
                "stored progress policy differs from the frozen policy".into(),
            ));
        }
        if state.work_items.len() > MAX_SUPERVISED_WORK_ITEMS {
            return Err(ProgressError::InvalidState(
                "stored progress snapshot has too many work items".into(),
            ));
        }
        let mut state = state;
        for item in state.work_items.values_mut() {
            if item.evidence_order.is_empty() {
                item.evidence_order = item.seen_evidence.iter().cloned().collect::<VecDeque<_>>();
            }
        }
        for (id, item) in &state.work_items {
            if id != &item.work_item_id {
                return Err(ProgressError::InvalidState(
                    "stored progress work item identity mismatch".into(),
                ));
            }
            if item.stagnant_checkpoints > u32::MAX / 2 || item.unsuccessful_replans > u32::MAX / 2
            {
                return Err(ProgressError::InvalidState(
                    "stored progress counters are implausible".into(),
                ));
            }
        }
        Ok(Self {
            policy,
            work_items: state.work_items,
        })
    }

    pub fn snapshot(&self) -> ProgressSupervisorState {
        ProgressSupervisorState {
            schema_version: STATE_SCHEMA_VERSION,
            policy: self.policy,
            work_items: self.work_items.clone(),
        }
    }

    pub fn work_item(&self, work_item_id: &str) -> Option<&WorkItemProgressState> {
        self.work_items.get(work_item_id)
    }

    /// Remove one gate-accepted work item from the active projection. Durable
    /// evidence and checkpoint rows remain the replay authority.
    pub fn retire_accepted_work_item(&mut self, work_item_id: &str) -> bool {
        let accepted = self
            .work_items
            .get(work_item_id)
            .is_some_and(|item| !item.accepted_units.is_empty());
        if accepted {
            self.work_items.remove(work_item_id);
        }
        accepted
    }

    /// Retire gate-accepted work items when admitting a new WorkGraph unit
    /// would otherwise exceed the active representation bound. Durable
    /// evidence and checkpoint rows remain authoritative in Core; only the
    /// active projection makes room for the next unit.
    pub fn retire_accepted_work_items_at_capacity(&mut self, admitting: &str) -> Vec<String> {
        if self.work_items.contains_key(admitting)
            || self.work_items.len() < MAX_SUPERVISED_WORK_ITEMS
        {
            return Vec::new();
        }
        let retired: Vec<String> = self
            .work_items
            .iter()
            .filter(|(_, item)| !item.accepted_units.is_empty())
            .map(|(id, _)| id.clone())
            .collect();
        for id in &retired {
            self.work_items.remove(id);
        }
        retired
    }

    /// Evaluate one terminal operation. Repeated calls with the same outcome
    /// digest are idempotent and never advance or reset a counter.
    pub fn checkpoint(
        &mut self,
        campaign_id: &str,
        checkpoint: &ProgressCheckpoint,
        evidence: &[ProgressEvidence],
        gate: Option<&GateRecord>,
    ) -> Result<CheckpointOutcome, ProgressError> {
        validate_checkpoint(campaign_id, checkpoint, evidence)?;
        let work_item_id = checkpoint.work_item_id.clone();
        if !self.work_items.contains_key(&work_item_id)
            && self.work_items.len() >= MAX_SUPERVISED_WORK_ITEMS
        {
            return Err(ProgressError::WorkItemRetirementRequired {
                active: self.work_items.len(),
            });
        }
        let policy = self.policy;
        let state = self
            .work_items
            .entry(work_item_id.clone())
            .or_insert_with(|| WorkItemProgressState::new(&work_item_id));

        if state.seen_outcomes.contains(&checkpoint.outcome_sha256) {
            return Ok(outcome(
                checkpoint,
                CheckpointClassification::Replayed,
                threshold_decision(&policy, state),
                Vec::new(),
                state,
            ));
        }

        let mut ordered: Vec<&ProgressEvidence> = evidence.iter().collect();
        ordered.sort_by_key(|item| (item.observed_sequence, item.evidence_id.as_str()));
        let mut rejected = Vec::new();
        let mut verified = false;
        let mut provisional: Option<String> = None;
        let mut blocked = false;
        let mut accepted_candidate = None;

        for item in ordered {
            match classify_evidence(&policy, state, campaign_id, checkpoint, item, gate) {
                Ok(Classification::Verified) => {
                    verified = true;
                    state.remember_evidence(evidence_key(item));
                    state.last_sequence = state.last_sequence.max(item.observed_sequence);
                }
                Ok(Classification::Provisional { candidate }) => {
                    provisional = Some(candidate);
                    state.remember_evidence(evidence_key(item));
                    state.last_sequence = state.last_sequence.max(item.observed_sequence);
                }
                Ok(Classification::Observed) => {
                    state.remember_evidence(evidence_key(item));
                    state.last_sequence = state.last_sequence.max(item.observed_sequence);
                }
                Ok(Classification::Blocked) => {
                    blocked = true;
                    state.remember_evidence(evidence_key(item));
                    state.last_sequence = state.last_sequence.max(item.observed_sequence);
                }
                Ok(Classification::Accepted { candidate }) => {
                    verified = true;
                    accepted_candidate = Some(candidate);
                    state.remember_evidence(evidence_key(item));
                    state.last_sequence = state.last_sequence.max(item.observed_sequence);
                }
                Err(reason) => rejected.push(RejectedEvidence {
                    evidence_id: item.evidence_id.clone(),
                    reason,
                }),
            }
        }

        let complete = accepted_candidate.is_some()
            && gate.is_some_and(|gate| {
                accepted_candidate
                    .as_deref()
                    .is_some_and(|sha| gate.accepts(campaign_id, &work_item_id, sha))
            });

        let classification;
        if complete {
            classification = CheckpointClassification::Verified;
            state.stagnant_checkpoints = 0;
            state.unsuccessful_replans = 0;
            state.provisional_candidate = None;
            state.provisional_age = 0;
            state.verified_advances = state
                .verified_advances
                .checked_add(1)
                .ok_or(ProgressError::Overflow)?;
            if let Some(sha) = accepted_candidate {
                state.accepted_units.insert(sha);
            }
        } else if blocked {
            classification = CheckpointClassification::Stagnant;
            state.stagnant_checkpoints = state
                .stagnant_checkpoints
                .checked_add(1)
                .ok_or(ProgressError::Overflow)?;
        } else if verified {
            classification = CheckpointClassification::Verified;
            state.stagnant_checkpoints = 0;
            state.unsuccessful_replans = 0;
            state.provisional_candidate = None;
            state.provisional_age = 0;
            state.verified_advances = state
                .verified_advances
                .checked_add(1)
                .ok_or(ProgressError::Overflow)?;
        } else if let Some(candidate) = provisional {
            // A changed diff is provisional: it is not stagnation on the
            // checkpoint it happens, but it does not reset the validation
            // window either. Repeated changed diffs without validated
            // advancement cannot renew the window indefinitely.
            if state.provisional_candidate.is_none() {
                state.provisional_candidate = Some(candidate);
                state.provisional_age = 0;
                classification = CheckpointClassification::Provisional;
            } else {
                state.provisional_age = state
                    .provisional_age
                    .checked_add(1)
                    .ok_or(ProgressError::Overflow)?;
                if state.provisional_age >= PROVISIONAL_VALIDATION_WINDOW {
                    state.provisional_candidate = None;
                    state.provisional_age = 0;
                    state.stagnant_checkpoints = state
                        .stagnant_checkpoints
                        .checked_add(1)
                        .ok_or(ProgressError::Overflow)?;
                    classification = CheckpointClassification::Stagnant;
                } else {
                    classification = CheckpointClassification::Provisional;
                }
            }
        } else if state.provisional_candidate.is_some() {
            state.provisional_age = state
                .provisional_age
                .checked_add(1)
                .ok_or(ProgressError::Overflow)?;
            if state.provisional_age >= PROVISIONAL_VALIDATION_WINDOW {
                state.provisional_candidate = None;
                state.provisional_age = 0;
                state.stagnant_checkpoints = state
                    .stagnant_checkpoints
                    .checked_add(1)
                    .ok_or(ProgressError::Overflow)?;
                classification = CheckpointClassification::Stagnant;
            } else {
                classification = CheckpointClassification::Provisional;
            }
        } else {
            classification = CheckpointClassification::Stagnant;
            state.stagnant_checkpoints = state
                .stagnant_checkpoints
                .checked_add(1)
                .ok_or(ProgressError::Overflow)?;
        }

        state
            .seen_outcomes
            .insert(checkpoint.outcome_sha256.clone());
        let decision = if complete {
            ContinuationDecision::Complete
        } else {
            threshold_decision(&policy, state)
        };
        Ok(outcome(
            checkpoint,
            classification,
            decision,
            rejected,
            state,
        ))
    }

    /// Record the operator-facing diagnosis result. A successful replan must be
    /// backed by verified external change; an unsuccessful replan restarts the
    /// stagnation window so escalation requires two further stagnant
    /// checkpoints.
    pub fn resolve_replan(
        &mut self,
        work_item_id: &str,
        verified_change: Option<&ProgressEvidence>,
        campaign_id: &str,
    ) -> Result<ContinuationDecision, ProgressError> {
        let state = self
            .work_items
            .get_mut(work_item_id)
            .ok_or_else(|| ProgressError::InvalidEvidence("unknown work item replan".into()))?;
        if let Some(evidence) = verified_change {
            validate_evidence(campaign_id, work_item_id, evidence)?;
            if state.seen_evidence.contains(&evidence_key(evidence)) {
                return Ok(threshold_decision(&self.policy, state));
            }
            state.remember_evidence(evidence_key(evidence));
            state.last_sequence = state.last_sequence.max(evidence.observed_sequence);
            state.stagnant_checkpoints = 0;
            state.unsuccessful_replans = 0;
            state.provisional_candidate = None;
            state.provisional_age = 0;
            state.verified_advances = state
                .verified_advances
                .checked_add(1)
                .ok_or(ProgressError::Overflow)?;
            return Ok(ContinuationDecision::Continue);
        }
        state.unsuccessful_replans = state
            .unsuccessful_replans
            .checked_add(1)
            .ok_or(ProgressError::Overflow)?;
        state.stagnant_checkpoints = 0;
        Ok(ContinuationDecision::Diagnose)
    }

    /// Decision for a work item that received an explicit operator or
    /// infrastructure event instead of a checkpoint.
    pub fn decision_for(&self, work_item_id: &str) -> ContinuationDecision {
        self.work_items
            .get(work_item_id)
            .map(|state| threshold_decision(&self.policy, state))
            .unwrap_or(ContinuationDecision::Continue)
    }
}

enum Classification {
    Verified,
    /// A relevant negative observation (for example a failing frozen check).
    /// It updates the baseline but earns no progress by itself.
    Observed,
    Provisional {
        candidate: String,
    },
    Blocked,
    Accepted {
        candidate: String,
    },
}

fn classify_evidence(
    _policy: &ProgressPolicy,
    state: &mut WorkItemProgressState,
    campaign_id: &str,
    checkpoint: &ProgressCheckpoint,
    evidence: &ProgressEvidence,
    gate: Option<&GateRecord>,
) -> Result<Classification, String> {
    validate_evidence(campaign_id, &checkpoint.work_item_id, evidence)
        .map_err(|error| error.to_string())?;
    if !checkpoint
        .evidence_ids
        .iter()
        .any(|id| id == &evidence.evidence_id)
    {
        return Err("evidence is not declared by the checkpoint".into());
    }
    if evidence.observed_sequence < state.last_sequence {
        return Err("evidence sequence moved backwards".into());
    }
    let key = evidence_key(evidence);
    if state.seen_evidence.contains(&key) {
        return Err("progress receipt was already recorded".into());
    }
    match evidence.kind {
        ProgressKind::CandidateChanged => {
            if evidence.artifact_sha256 == evidence.baseline_ref {
                return Err("empty candidate change".into());
            }
            if state
                .candidate_history
                .iter()
                .any(|sha| sha == &evidence.artifact_sha256)
            {
                return Err("candidate diff oscillated back to a known artifact".into());
            }
            if state.provisional_candidate.as_deref() == Some(&evidence.artifact_sha256) {
                return Err("candidate change repeats the provisional artifact".into());
            }
            state.remember_candidate(&evidence.artifact_sha256);
            Ok(Classification::Provisional {
                candidate: evidence.artifact_sha256.clone(),
            })
        }
        ProgressKind::TestsImproved => {
            // A changed receipt hash alone is not an improvement. The artifact
            // must identify the same suite/environment and an explicit
            // PASS/FAIL transition; only FAIL -> PASS under the same suite is
            // verified progress.
            let (suite, outcome) = parse_test_artifact(&evidence.artifact_ref)
                .ok_or("test evidence must identify suite/environment and PASS/FAIL outcome")?;
            if suite != evidence.baseline_ref {
                return Err("test suite artifact and baseline identity differ".into());
            }
            if outcome == TestOutcome::Failed {
                state.test_suite = Some(suite.to_owned());
                state.test_result = Some(evidence.artifact_sha256.clone());
                state.test_outcome = Some("FAIL".into());
                return Ok(Classification::Observed);
            }
            match (
                state.test_suite.as_deref(),
                state.test_outcome.as_deref(),
                state.test_result.as_deref(),
            ) {
                (Some(previous_suite), Some("FAIL"), Some(previous_receipt))
                    if previous_suite == suite && previous_receipt != evidence.artifact_sha256 =>
                {
                    state.test_suite = Some(suite.to_owned());
                    state.test_result = Some(evidence.artifact_sha256.clone());
                    state.test_outcome = Some("PASS".into());
                    Ok(Classification::Verified)
                }
                (Some(previous_suite), _, _) if previous_suite != suite => {
                    Err("test improvement changed the suite/environment".into())
                }
                (None, _, _) => Err("test pass has no failing baseline for this suite".into()),
                _ => Err("test pass is unchanged or not a FAIL-to-PASS transition".into()),
            }
        }
        ProgressKind::FindingsResolved => {
            if evidence.artifact_ref.trim().is_empty() {
                return Err("resolved finding has no stable finding id".into());
            }
            if !state
                .resolved_findings
                .insert(evidence.artifact_ref.clone())
            {
                return Err("finding was already resolved".into());
            }
            Ok(Classification::Verified)
        }
        ProgressKind::WorkGraphAdvanced => {
            if evidence.artifact_ref.trim().is_empty() {
                return Err("work graph advancement cites no revision".into());
            }
            if evidence.artifact_ref == evidence.baseline_ref {
                return Err("work graph revision did not advance".into());
            }
            if state.graph_revision.as_deref() == Some(evidence.artifact_ref.as_str()) {
                return Err("work graph revision was replayed".into());
            }
            state.graph_revision = Some(evidence.artifact_ref.clone());
            Ok(Classification::Verified)
        }
        ProgressKind::UnitAccepted => {
            let Some(gate) = gate else {
                return Err("unit acceptance requires a Core gate record".into());
            };
            if !gate.accepts(
                campaign_id,
                &checkpoint.work_item_id,
                &evidence.artifact_sha256,
            ) {
                return Err("unit acceptance is not backed by an accepting gate".into());
            }
            if !state
                .accepted_units
                .insert(evidence.artifact_sha256.clone())
            {
                return Err("unit was already accepted".into());
            }
            Ok(Classification::Accepted {
                candidate: evidence.artifact_sha256.clone(),
            })
        }
        ProgressKind::RepairValidated => {
            if evidence.artifact_ref.trim().is_empty() {
                return Err("repair validation cites no repair episode".into());
            }
            if !state
                .repaired_episodes
                .insert(evidence.artifact_ref.clone())
            {
                return Err("repair validation was replayed".into());
            }
            Ok(Classification::Verified)
        }
        ProgressKind::BlockerIdentified => {
            if evidence.artifact_ref.trim().is_empty() || evidence.baseline_ref.trim().is_empty() {
                return Err("blocker cites no reproducible failure reference".into());
            }
            let blocker = format!("{}:{}", evidence.baseline_ref, evidence.artifact_ref);
            if !state.blocker_keys.insert(blocker) {
                return Err("blocker was already identified".into());
            }
            Ok(Classification::Blocked)
        }
    }
}

fn threshold_decision(
    policy: &ProgressPolicy,
    state: &WorkItemProgressState,
) -> ContinuationDecision {
    if state.stagnant_checkpoints < policy.diagnose_after_stagnant_checkpoints.get() {
        return ContinuationDecision::Continue;
    }
    if state.unsuccessful_replans >= policy.escalate_after_unsuccessful_replans.get() {
        ContinuationDecision::Escalate
    } else {
        ContinuationDecision::Diagnose
    }
}

fn outcome(
    checkpoint: &ProgressCheckpoint,
    classification: CheckpointClassification,
    decision: ContinuationDecision,
    rejected: Vec<RejectedEvidence>,
    state: &WorkItemProgressState,
) -> CheckpointOutcome {
    CheckpointOutcome {
        checkpoint_id: checkpoint.checkpoint_id.clone(),
        work_item_id: checkpoint.work_item_id.clone(),
        classification,
        decision,
        rejected,
        stagnant_checkpoints: state.stagnant_checkpoints,
        unsuccessful_replans: state.unsuccessful_replans,
        verified_advances: state.verified_advances,
    }
}

fn evidence_key(evidence: &ProgressEvidence) -> String {
    format!(
        "{:?}|{}|{}|{}",
        evidence.kind, evidence.artifact_ref, evidence.artifact_sha256, evidence.baseline_ref
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestOutcome {
    Passed,
    Failed,
}

/// Test evidence identity is `suite:<suite-id>:PASS|FAIL`. The suite id is the
/// stable command/environment identity; the outcome is the observed result.
fn parse_test_artifact(artifact_ref: &str) -> Option<(&str, TestOutcome)> {
    let rest = artifact_ref.strip_prefix("suite:")?;
    // Worker receipts carry a Core tool-call id: suite:<suite>|PASS|call-id.
    let (suite, outcome) = match rest.rsplit_once('|') {
        Some((before, call_id)) if !call_id.trim().is_empty() => before.rsplit_once('|')?,
        // Unit tests and historical receipts may omit the call id.
        Some(_) => return None,
        None => rest.rsplit_once(':')?,
    };
    if suite.trim().is_empty() {
        return None;
    }
    let outcome = match outcome {
        "PASS" => TestOutcome::Passed,
        "FAIL" => TestOutcome::Failed,
        _ => return None,
    };
    Some((suite, outcome))
}

fn validate_policy(policy: &ProgressPolicy) -> Result<(), ProgressError> {
    if policy.diagnose_after_stagnant_checkpoints.get() < 1 {
        return Err(ProgressError::InvalidPolicy(
            "diagnose threshold must be positive".into(),
        ));
    }
    if policy.escalate_after_unsuccessful_replans.get() < 1 {
        return Err(ProgressError::InvalidPolicy(
            "escalation threshold must be positive".into(),
        ));
    }
    if policy.observation_timeout_secs.get() == 0 {
        return Err(ProgressError::InvalidPolicy(
            "observation timeout must be positive".into(),
        ));
    }
    Ok(())
}

fn validate_checkpoint(
    campaign_id: &str,
    checkpoint: &ProgressCheckpoint,
    evidence: &[ProgressEvidence],
) -> Result<(), ProgressError> {
    for (name, value) in [
        ("campaignId", campaign_id),
        ("checkpointId", checkpoint.checkpoint_id.as_str()),
        ("workItemId", checkpoint.work_item_id.as_str()),
        ("operationId", checkpoint.operation_id.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(ProgressError::InvalidEvidence(format!("{name} is empty")));
        }
    }
    if !is_sha256(&checkpoint.outcome_sha256) {
        return Err(ProgressError::InvalidEvidence(
            "outcomeSha256 is not a lowercase SHA-256".into(),
        ));
    }
    if checkpoint.evidence_ids.len() > MAX_EVIDENCE_PER_CHECKPOINT {
        return Err(ProgressError::InvalidEvidence(
            "checkpoint cites too many evidence receipts".into(),
        ));
    }
    if evidence.len() > MAX_EVIDENCE_PER_CHECKPOINT {
        return Err(ProgressError::InvalidEvidence(
            "checkpoint carries too many evidence receipts".into(),
        ));
    }
    let mut ids = BTreeSet::new();
    for item in evidence {
        if !ids.insert(item.evidence_id.as_str()) {
            return Err(ProgressError::InvalidEvidence(
                "checkpoint repeats an evidence id".into(),
            ));
        }
    }
    Ok(())
}

fn validate_evidence(
    campaign_id: &str,
    work_item_id: &str,
    evidence: &ProgressEvidence,
) -> Result<(), ProgressError> {
    for (name, value) in [
        ("evidenceId", evidence.evidence_id.as_str()),
        ("artifactRef", evidence.artifact_ref.as_str()),
        ("baselineRef", evidence.baseline_ref.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(ProgressError::InvalidEvidence(format!("{name} is empty")));
        }
    }
    if evidence.campaign_id != campaign_id {
        return Err(ProgressError::InvalidEvidence(
            "evidence campaign differs from the supervised campaign".into(),
        ));
    }
    if evidence.work_item_id != work_item_id {
        return Err(ProgressError::InvalidEvidence(
            "evidence work item differs from the checkpoint".into(),
        ));
    }
    if !is_sha256(&evidence.artifact_sha256) {
        return Err(ProgressError::InvalidEvidence(
            "artifactSha256 is not a lowercase SHA-256".into(),
        ));
    }
    if evidence.observed_sequence == 0 {
        return Err(ProgressError::InvalidEvidence(
            "observedSequence must be positive".into(),
        ));
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU32;

    fn policy() -> ProgressPolicy {
        ProgressPolicy {
            diagnose_after_stagnant_checkpoints: NonZeroU32::new(2).unwrap(),
            escalate_after_unsuccessful_replans: NonZeroU32::new(1).unwrap(),
            observation_timeout_secs: NonZeroU32::new(300).unwrap(),
        }
    }

    fn supervisor() -> ProgressSupervisor {
        ProgressSupervisor::new(policy()).unwrap()
    }

    fn sha(seed: &str) -> String {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(seed.as_bytes());
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn checkpoint(
        operation: &str,
        outcome: &str,
        evidence: &[ProgressEvidence],
    ) -> ProgressCheckpoint {
        ProgressCheckpoint {
            checkpoint_id: format!("cp-{operation}"),
            work_item_id: "unit-1".into(),
            operation_id: operation.into(),
            evidence_ids: evidence
                .iter()
                .map(|item| item.evidence_id.clone())
                .collect(),
            outcome_sha256: sha(outcome),
        }
    }

    fn evidence(
        kind: ProgressKind,
        artifact: &str,
        baseline: &str,
        sequence: u64,
    ) -> ProgressEvidence {
        ProgressEvidence {
            evidence_id: format!("ev-{sequence}-{kind:?}"),
            campaign_id: "campaign".into(),
            work_item_id: "unit-1".into(),
            kind,
            artifact_ref: format!("artifact-{sequence}"),
            artifact_sha256: sha(artifact),
            baseline_ref: baseline.into(),
            observed_sequence: sequence,
        }
    }

    fn test_evidence(sequence: u64, suite: &str, outcome: &str) -> ProgressEvidence {
        ProgressEvidence {
            evidence_id: format!("ev-{sequence}-{outcome}"),
            campaign_id: "campaign".into(),
            work_item_id: "unit-1".into(),
            kind: ProgressKind::TestsImproved,
            artifact_ref: format!("suite:{suite}:{outcome}"),
            artifact_sha256: sha(&format!("{suite}-{sequence}-{outcome}")),
            baseline_ref: suite.into(),
            observed_sequence: sequence,
        }
    }

    fn gate(artifact: &str) -> GateRecord {
        GateRecord {
            gate_id: "gate-1".into(),
            campaign_id: "campaign".into(),
            work_item_id: "unit-1".into(),
            candidate_sha256: sha(artifact),
            outcome: "ACCEPTED".into(),
        }
    }

    fn run(
        supervisor: &mut ProgressSupervisor,
        operation: &str,
        evidence: &[ProgressEvidence],
        gate: Option<&GateRecord>,
    ) -> CheckpointOutcome {
        supervisor
            .checkpoint(
                "campaign",
                &checkpoint(operation, operation, evidence),
                evidence,
                gate,
            )
            .unwrap()
    }

    #[test]
    fn two_stagnant_checkpoints_diagnose_and_verified_progress_resets() {
        let mut supervisor = supervisor();
        let first = run(&mut supervisor, "op-1", &[], None);
        assert_eq!(first.decision, ContinuationDecision::Continue);
        assert_eq!(first.stagnant_checkpoints, 1);

        let second = run(&mut supervisor, "op-2", &[], None);
        assert_eq!(second.decision, ContinuationDecision::Diagnose);
        assert_eq!(second.stagnant_checkpoints, 2);

        let failed = test_evidence(3, "suite-a", "FAIL");
        let passed = test_evidence(4, "suite-a", "PASS");
        let third = run(&mut supervisor, "op-3", &[failed, passed], None);
        assert_eq!(
            third.classification,
            CheckpointClassification::Verified,
            "{third:?}"
        );
        assert_eq!(third.decision, ContinuationDecision::Continue);
        assert_eq!(third.stagnant_checkpoints, 0);
    }

    #[test]
    fn unsuccessful_replan_plus_two_stagnant_checkpoints_escalates() {
        let mut supervisor = supervisor();
        run(&mut supervisor, "op-1", &[], None);
        let diagnosed = run(&mut supervisor, "op-2", &[], None);
        assert_eq!(diagnosed.decision, ContinuationDecision::Diagnose);

        let decision = supervisor
            .resolve_replan("unit-1", None, "campaign")
            .unwrap();
        assert_eq!(decision, ContinuationDecision::Diagnose);
        assert_eq!(
            supervisor.work_item("unit-1").unwrap().stagnant_checkpoints,
            0
        );

        run(&mut supervisor, "op-3", &[], None);
        let escalated = run(&mut supervisor, "op-4", &[], None);
        assert_eq!(escalated.decision, ContinuationDecision::Escalate);
        assert_eq!(escalated.unsuccessful_replans, 1);
    }

    #[test]
    fn successful_replan_requires_verified_change() {
        let mut supervisor = supervisor();
        run(&mut supervisor, "op-1", &[], None);
        run(&mut supervisor, "op-2", &[], None);
        let change = evidence(ProgressKind::WorkGraphAdvanced, "graph-2", "graph-1", 3);
        let decision = supervisor
            .resolve_replan("unit-1", Some(&change), "campaign")
            .unwrap();
        assert_eq!(decision, ContinuationDecision::Continue);
        let state = supervisor.work_item("unit-1").unwrap();
        assert_eq!(state.stagnant_checkpoints, 0);
        assert_eq!(state.unsuccessful_replans, 0);
    }

    #[test]
    fn empty_commits_and_oscillating_diffs_earn_no_progress() {
        let mut supervisor = supervisor();
        let empty = ProgressEvidence {
            artifact_sha256: sha("baseline"),
            baseline_ref: sha("baseline"),
            ..evidence(ProgressKind::CandidateChanged, "x", "y", 1)
        };
        let result = run(&mut supervisor, "op-1", std::slice::from_ref(&empty), None);
        assert_eq!(result.classification, CheckpointClassification::Stagnant);
        assert_eq!(result.rejected.len(), 1);

        let a = evidence(ProgressKind::CandidateChanged, "diff-a", "commit-0", 2);
        run(&mut supervisor, "op-2", std::slice::from_ref(&a), None);
        let b = evidence(ProgressKind::CandidateChanged, "diff-b", "commit-a", 3);
        run(&mut supervisor, "op-3", std::slice::from_ref(&b), None);
        let again = ProgressEvidence {
            observed_sequence: 4,
            evidence_id: "ev-4-oscillate".into(),
            artifact_ref: "artifact-4".into(),
            ..a.clone()
        };
        let oscillated = run(&mut supervisor, "op-4", std::slice::from_ref(&again), None);
        assert_eq!(oscillated.rejected.len(), 1);
        assert!(
            oscillated.rejected[0].reason.contains("oscillated"),
            "{:?}",
            oscillated.rejected
        );
    }

    #[test]
    fn provisional_change_must_be_validated_within_two_checkpoints() {
        let mut supervisor = supervisor();
        let change = evidence(ProgressKind::CandidateChanged, "diff-a", "commit-0", 1);
        let first = run(&mut supervisor, "op-1", std::slice::from_ref(&change), None);
        assert_eq!(first.classification, CheckpointClassification::Provisional);
        assert_eq!(first.stagnant_checkpoints, 0);

        let second = run(&mut supervisor, "op-2", &[], None);
        assert_eq!(second.classification, CheckpointClassification::Provisional);
        assert_eq!(second.stagnant_checkpoints, 0);

        let third = run(&mut supervisor, "op-3", &[], None);
        assert_eq!(third.classification, CheckpointClassification::Stagnant);
        assert_eq!(third.stagnant_checkpoints, 1);
    }

    #[test]
    fn replayed_receipts_and_changed_test_suites_are_rejected() {
        let mut supervisor = supervisor();
        let failed = test_evidence(1, "suite-a", "FAIL");
        run(&mut supervisor, "op-1", std::slice::from_ref(&failed), None);
        let improved = test_evidence(2, "suite-a", "PASS");
        let verified = run(
            &mut supervisor,
            "op-2",
            std::slice::from_ref(&improved),
            None,
        );
        assert_eq!(verified.classification, CheckpointClassification::Verified);

        let replay = ProgressEvidence {
            observed_sequence: 3,
            evidence_id: "ev-3-replay".into(),
            ..improved.clone()
        };
        let replayed = run(&mut supervisor, "op-3", std::slice::from_ref(&replay), None);
        assert_eq!(replayed.rejected.len(), 1);

        let other_suite = test_evidence(4, "suite-b", "PASS");
        let changed = run(
            &mut supervisor,
            "op-4",
            std::slice::from_ref(&other_suite),
            None,
        );
        assert_eq!(changed.rejected.len(), 1);
        assert!(
            changed.rejected[0].reason.contains("suite")
                || changed.rejected[0].reason.contains("baseline"),
            "{:?}",
            changed.rejected
        );
    }

    #[test]
    fn a_pass_without_a_failing_baseline_is_not_progress() {
        let mut supervisor = supervisor();
        let passed = test_evidence(1, "suite-a", "PASS");
        let outcome = run(&mut supervisor, "op-1", std::slice::from_ref(&passed), None);
        assert_eq!(outcome.classification, CheckpointClassification::Stagnant);
        assert_eq!(outcome.stagnant_checkpoints, 1);
        assert!(!outcome.rejected.is_empty());
    }

    #[test]
    fn repeated_candidate_changes_cannot_renew_the_validation_window() {
        let mut supervisor = supervisor();
        let first = evidence(ProgressKind::CandidateChanged, "diff-a", "commit-0", 1);
        assert_eq!(
            run(&mut supervisor, "op-1", std::slice::from_ref(&first), None).classification,
            CheckpointClassification::Provisional
        );
        let second = evidence(ProgressKind::CandidateChanged, "diff-b", "commit-a", 2);
        assert_eq!(
            run(&mut supervisor, "op-2", std::slice::from_ref(&second), None).classification,
            CheckpointClassification::Provisional
        );
        let third = evidence(ProgressKind::CandidateChanged, "diff-c", "commit-b", 3);
        let exhausted = run(&mut supervisor, "op-3", std::slice::from_ref(&third), None);
        assert_eq!(
            exhausted.classification,
            CheckpointClassification::Stagnant,
            "two changed diffs without validated advancement must expire the window"
        );
        assert_eq!(exhausted.stagnant_checkpoints, 1);
    }

    #[test]
    fn repeated_blocker_does_not_renew_a_loop() {
        let mut supervisor = supervisor();
        let blocker = evidence(ProgressKind::BlockerIdentified, "network", "check-run", 1);
        let first = run(
            &mut supervisor,
            "op-1",
            std::slice::from_ref(&blocker),
            None,
        );
        assert_eq!(first.classification, CheckpointClassification::Stagnant);
        assert_eq!(first.decision, ContinuationDecision::Continue);

        let repeat = ProgressEvidence {
            observed_sequence: 2,
            evidence_id: "ev-2-blocker".into(),
            ..blocker.clone()
        };
        let again = run(&mut supervisor, "op-2", std::slice::from_ref(&repeat), None);
        assert_eq!(again.rejected.len(), 1);
        assert_eq!(again.stagnant_checkpoints, 2);
        assert_eq!(again.decision, ContinuationDecision::Diagnose);
    }

    #[test]
    fn acceptance_requires_a_core_gate_for_the_exact_candidate() {
        let mut supervisor = supervisor();
        let accepted = evidence(ProgressKind::UnitAccepted, "candidate", "unit-1", 1);
        let without_gate = run(
            &mut supervisor,
            "op-1",
            std::slice::from_ref(&accepted),
            None,
        );
        assert_eq!(without_gate.rejected.len(), 1);
        assert_ne!(without_gate.decision, ContinuationDecision::Complete);

        let wrong = GateRecord {
            candidate_sha256: sha("another-candidate"),
            ..gate("candidate")
        };
        let mismatched = run(
            &mut supervisor,
            "op-2",
            std::slice::from_ref(&accepted),
            Some(&wrong),
        );
        assert_ne!(mismatched.decision, ContinuationDecision::Complete);

        let accepted_again = ProgressEvidence {
            observed_sequence: 3,
            evidence_id: "ev-3-accepted".into(),
            ..accepted.clone()
        };
        let complete = run(
            &mut supervisor,
            "op-3",
            std::slice::from_ref(&accepted_again),
            Some(&gate("candidate")),
        );
        assert_eq!(complete.decision, ContinuationDecision::Complete);
    }

    #[test]
    fn gate_accepts_a_raw_git_commit_against_the_progress_digest() {
        let mut supervisor = supervisor();
        let commit = "0123456789abcdef0123456789abcdef01234567";
        let accepted = ProgressEvidence {
            artifact_sha256: sha256_hex(commit.as_bytes()),
            ..evidence(ProgressKind::UnitAccepted, "candidate", "unit-1", 1)
        };
        let raw_gate = GateRecord {
            candidate_sha256: commit.into(),
            ..gate("candidate")
        };
        let outcome = run(
            &mut supervisor,
            "op-1",
            std::slice::from_ref(&accepted),
            Some(&raw_gate),
        );
        assert_eq!(outcome.decision, ContinuationDecision::Complete);
        assert_eq!(outcome.classification, CheckpointClassification::Verified);
    }

    #[test]
    fn restart_preserves_counters_and_deduplicates_replays() {
        let mut supervisor = supervisor();
        let first = evidence(ProgressKind::CandidateChanged, "diff-a", "commit-0", 1);
        run(&mut supervisor, "op-1", std::slice::from_ref(&first), None);
        run(&mut supervisor, "op-2", &[], None);

        let state = supervisor.snapshot();
        let encoded = serde_json::to_string(&state).unwrap();
        let restored_state: ProgressSupervisorState = serde_json::from_str(&encoded).unwrap();
        let restored = ProgressSupervisor::restore(policy(), restored_state).unwrap();
        assert_eq!(
            restored.work_item("unit-1").unwrap().stagnant_checkpoints,
            0
        );

        // The same operation outcome is a replay after restart, not new work.
        let mut replay = ProgressSupervisor::restore(policy(), supervisor.snapshot()).unwrap();
        let failed = test_evidence(2, "suite-a", "FAIL");
        let passed = test_evidence(3, "suite-a", "PASS");
        let counted = run(&mut replay, "op-3", &[failed, passed.clone()], None);
        assert_eq!(counted.classification, CheckpointClassification::Verified);
        let snapshot = replay.snapshot();
        let mut after_restart = ProgressSupervisor::restore(policy(), snapshot).unwrap();
        let replayed = run(
            &mut after_restart,
            "op-3",
            std::slice::from_ref(&passed),
            None,
        );
        assert_eq!(replayed.classification, CheckpointClassification::Replayed);
        assert_eq!(replayed.stagnant_checkpoints, counted.stagnant_checkpoints);
    }

    #[test]
    fn work_item_capacity_is_a_typed_retirement_requirement() {
        let mut supervisor = supervisor();
        for index in 0..MAX_SUPERVISED_WORK_ITEMS {
            let work_item = format!("unit-{index}");
            let checkpoint = ProgressCheckpoint {
                checkpoint_id: format!("cp-{index}"),
                work_item_id: work_item,
                operation_id: format!("op-{index}"),
                evidence_ids: Vec::new(),
                outcome_sha256: sha(&format!("outcome-{index}")),
            };
            supervisor
                .checkpoint("campaign", &checkpoint, &[], None)
                .unwrap();
        }
        let extra = ProgressCheckpoint {
            checkpoint_id: "cp-extra".into(),
            work_item_id: "unit-extra".into(),
            operation_id: "op-extra".into(),
            evidence_ids: Vec::new(),
            outcome_sha256: sha("outcome-extra"),
        };
        let error = supervisor
            .checkpoint("campaign", &extra, &[], None)
            .unwrap_err();
        assert!(matches!(
            error,
            ProgressError::WorkItemRetirementRequired { active }
                if active == MAX_SUPERVISED_WORK_ITEMS
        ));
    }

    #[test]
    fn evidence_projection_is_bounded_and_evicts_oldest() {
        let mut state = WorkItemProgressState::new("unit-1");
        for index in 0..(MAX_TRACKED_EVIDENCE + 16) {
            state.remember_evidence(format!("evidence-{index}"));
        }
        assert_eq!(state.seen_evidence.len(), MAX_TRACKED_EVIDENCE);
        assert_eq!(state.evidence_order.len(), MAX_TRACKED_EVIDENCE);
        assert!(!state.seen_evidence.contains("evidence-0"));
        assert!(
            state
                .seen_evidence
                .contains(&format!("evidence-{}", MAX_TRACKED_EVIDENCE + 15))
        );
    }

    #[test]
    fn a_different_policy_cannot_reinterpret_a_persisted_snapshot() {
        let supervisor = supervisor();
        let mut changed = policy();
        changed.diagnose_after_stagnant_checkpoints = NonZeroU32::new(3).unwrap();
        assert_eq!(
            ProgressSupervisor::restore(changed, supervisor.snapshot()).unwrap_err(),
            ProgressError::InvalidState(
                "stored progress policy differs from the frozen policy".into()
            )
        );
    }
}
