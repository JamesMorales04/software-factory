//! Issue-linked delivery lifecycle.
//!
//! A request freezes the exact repository issues it intends to resolve. Core
//! then projects each link through Campaign, candidate, pull request and merge
//! facts. Model output and Office never set this state.
//!
//! This module owns the shared issue-link domain used by WP2 and the delivery
//! hook WP3 calls after an authorized merge. It never merges, pushes or grants
//! work: GitHub issue comments, labels and closes are the only effects.

pub mod delivery;
#[cfg(test)]
mod delivery_tests;

use serde::{Deserialize, Serialize};

pub use delivery::{IssueDeliveryError, IssueDeliveryService};

/// Maximum selected issues per request. Keeps a single request's delivery
/// ledger and Office detail bounded.
pub const MAX_ISSUES_PER_REQUEST: usize = 20;
pub const MAX_EVENT_KEY_CHARS: usize = 200;
pub const MAX_MARKER_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IssueRole {
    Primary,
    Related,
}

impl IssueRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "PRIMARY",
            Self::Related => "RELATED",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "PRIMARY" => Some(Self::Primary),
            "RELATED" => Some(Self::Related),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IssueLinkState {
    Ready,
    Queued,
    InProgress,
    NeedsAttention,
    InReview,
    Done,
}

impl IssueLinkState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "READY",
            Self::Queued => "QUEUED",
            Self::InProgress => "IN_PROGRESS",
            Self::NeedsAttention => "NEEDS_ATTENTION",
            Self::InReview => "IN_REVIEW",
            Self::Done => "DONE",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "READY" => Some(Self::Ready),
            "QUEUED" => Some(Self::Queued),
            "IN_PROGRESS" => Some(Self::InProgress),
            "NEEDS_ATTENTION" => Some(Self::NeedsAttention),
            "IN_REVIEW" => Some(Self::InReview),
            "DONE" => Some(Self::Done),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IssueDeliveryState {
    None,
    Pending,
    Commented,
    Closed,
    Partial,
    Failed,
    Blocked,
}

impl IssueDeliveryState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::Pending => "PENDING",
            Self::Commented => "COMMENTED",
            Self::Closed => "CLOSED",
            Self::Partial => "PARTIAL",
            Self::Failed => "FAILED",
            Self::Blocked => "BLOCKED",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "NONE" => Some(Self::None),
            "PENDING" => Some(Self::Pending),
            "COMMENTED" => Some(Self::Commented),
            "CLOSED" => Some(Self::Closed),
            "PARTIAL" => Some(Self::Partial),
            "FAILED" => Some(Self::Failed),
            "BLOCKED" => Some(Self::Blocked),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IssueLinkResolution {
    Unresolved,
    Resolved,
    Partial,
}

impl IssueLinkResolution {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unresolved => "UNRESOLVED",
            Self::Resolved => "RESOLVED",
            Self::Partial => "PARTIAL",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "UNRESOLVED" => Some(Self::Unresolved),
            "RESOLVED" => Some(Self::Resolved),
            "PARTIAL" => Some(Self::Partial),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IssueDeliveryAction {
    Comment,
    Label,
    Close,
    Failure,
}

impl IssueDeliveryAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Comment => "COMMENT",
            Self::Label => "LABEL",
            Self::Close => "CLOSE",
            Self::Failure => "FAILURE",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "COMMENT" => Some(Self::Comment),
            "LABEL" => Some(Self::Label),
            "CLOSE" => Some(Self::Close),
            "FAILURE" => Some(Self::Failure),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IssueDeliveryEventStatus {
    Pending,
    Applied,
    Failed,
    Blocked,
}

impl IssueDeliveryEventStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Applied => "APPLIED",
            Self::Failed => "FAILED",
            Self::Blocked => "BLOCKED",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "PENDING" => Some(Self::Pending),
            "APPLIED" => Some(Self::Applied),
            "FAILED" => Some(Self::Failed),
            "BLOCKED" => Some(Self::Blocked),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewIssueSelection {
    pub owner: String,
    pub repo: String,
    pub number: u64,
    pub role: IssueRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewIssueLinkSpec {
    pub owner: String,
    pub repo: String,
    pub number: u64,
    pub github_issue_id: i64,
    pub title: String,
    pub role: IssueRole,
    pub work_unit_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueDeliveryFailure {
    pub kind: String,
    pub detail: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueLinkRecord {
    pub id: String,
    pub project_id: String,
    pub request_id: String,
    pub campaign_id: Option<String>,
    pub candidate_id: Option<String>,
    pub repository_owner: String,
    pub repository_name: String,
    pub repository_full_name: String,
    pub issue_number: u64,
    pub github_issue_id: Option<i64>,
    pub issue_title: String,
    pub role: IssueRole,
    pub state: IssueLinkState,
    pub delivery_state: IssueDeliveryState,
    pub resolution: IssueLinkResolution,
    pub work_unit_id: Option<String>,
    pub pull_request_number: Option<u64>,
    pub pull_request_url: Option<String>,
    pub merge_sha: Option<String>,
    pub merged_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub latest_error: Option<IssueDeliveryFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueDeliveryEventRecord {
    pub id: String,
    pub issue_link_id: String,
    pub campaign_id: Option<String>,
    pub candidate_id: Option<String>,
    pub action: IssueDeliveryAction,
    pub event_key: String,
    pub marker: Option<String>,
    pub body: Option<String>,
    pub label_name: Option<String>,
    pub status: IssueDeliveryEventStatus,
    pub attempts: u32,
    pub external_id: Option<String>,
    pub error_kind: Option<String>,
    pub error_detail: Option<String>,
    pub last_attempt_at: Option<String>,
    pub applied_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueLinkProjection {
    #[serde(flatten)]
    pub link: IssueLinkRecord,
    pub events: Vec<IssueDeliveryEventRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueLinkSummary {
    pub id: String,
    pub request_id: String,
    pub campaign_id: Option<String>,
    pub candidate_id: Option<String>,
    pub role: IssueRole,
    pub state: IssueLinkState,
    pub delivery_state: IssueDeliveryState,
    pub resolution: IssueLinkResolution,
    pub pull_request_number: Option<u64>,
    pub pull_request_url: Option<String>,
    pub merge_sha: Option<String>,
    pub merged_at: Option<String>,
    pub latest_error_kind: Option<String>,
    pub latest_error_detail: Option<String>,
    pub latest_error_event_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueDeliveryOutcome {
    pub campaign_id: String,
    pub candidate_id: String,
    pub attempted: u32,
    pub applied: u32,
    pub partial: u32,
    pub failed: u32,
    pub failures: Vec<IssueDeliveryFailure>,
    pub links: Vec<IssueLinkProjection>,
}

/// Facts available to the deterministic issue-state projection. Every field is
/// recorded Core state; model input is never accepted here.
#[derive(Debug, Clone, Copy)]
pub struct IssueStateFacts<'a> {
    pub lead_state: Option<&'a str>,
    pub campaign_status: Option<&'a str>,
    pub gate_outcome: Option<&'a str>,
    pub pull_request_opened: bool,
    pub merge_sha: Option<&'a str>,
    pub delivery_state: IssueDeliveryState,
    pub resolution: IssueLinkResolution,
}

/// Project Issue, Campaign, Gate, PR and merge facts into the board state.
///
/// The Gate may accept a candidate, but acceptance alone never completes an
/// issue: only a recorded merge can produce `DONE`. A merged issue whose frozen
/// mapping was only partially resolved returns to `NEEDS_ATTENTION` and stays
/// open.
pub fn derive_issue_link_state(facts: IssueStateFacts<'_>) -> IssueLinkState {
    if matches!(facts.lead_state, Some("BLOCKED" | "FAILED"))
        || matches!(facts.campaign_status, Some("CANCELLED" | "FAILED"))
    {
        return IssueLinkState::NeedsAttention;
    }
    if matches!(
        facts.gate_outcome,
        Some("REPAIR_REQUIRED" | "BLOCKED" | "HUMAN_REQUIRED" | "NOT_TESTED")
    ) {
        return IssueLinkState::NeedsAttention;
    }
    if facts.merge_sha.is_some() {
        return if facts.resolution == IssueLinkResolution::Resolved {
            IssueLinkState::Done
        } else {
            IssueLinkState::NeedsAttention
        };
    }
    if matches!(
        facts.delivery_state,
        IssueDeliveryState::Failed | IssueDeliveryState::Blocked
    ) {
        return IssueLinkState::NeedsAttention;
    }
    if facts.pull_request_opened {
        return IssueLinkState::InReview;
    }
    if facts.campaign_status == Some("BLOCKED_AUTHORIZATION") {
        return IssueLinkState::Queued;
    }
    if facts.campaign_status.is_some() {
        return IssueLinkState::InProgress;
    }
    IssueLinkState::Ready
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueResolution {
    Resolved,
    Partial,
}

/// Decide whether a merged candidate fully resolves one frozen issue link.
///
/// A link explicitly mapped to a work unit resolves only for that unit. An
/// unmapped `PRIMARY` link resolves only when it is the single primary issue of
/// the request; otherwise Core cannot prove that the merge completed every
/// selected issue and keeps it open. `RELATED` links always require an explicit
/// work-unit mapping.
pub fn resolve_issue_link(
    link: &IssueLinkRecord,
    candidate_work_unit: Option<&str>,
    primary_link_count: usize,
    campaign_work_complete: bool,
) -> IssueResolution {
    if let Some(unit) = link.work_unit_id.as_deref() {
        return if candidate_work_unit == Some(unit) {
            IssueResolution::Resolved
        } else {
            IssueResolution::Partial
        };
    }
    match link.role {
        IssueRole::Primary if primary_link_count == 1 && campaign_work_complete => {
            IssueResolution::Resolved
        }
        IssueRole::Primary | IssueRole::Related => IssueResolution::Partial,
    }
}

/// Deterministic marker embedded in every issue comment. A retry searches for
/// this exact marker before posting, so a crash between the upstream write and
/// the local receipt cannot duplicate the comment.
pub fn delivery_marker(link_id: &str, stage: &str, identity: &str) -> String {
    format!("<!-- factory:issue-link:{link_id}:{stage}:{identity} -->")
}

pub fn valid_delivery_marker(marker: &str) -> bool {
    !marker.is_empty()
        && marker.len() <= MAX_MARKER_CHARS
        && marker.starts_with("<!-- factory:")
        && marker.ends_with(" -->")
        && !marker.contains('\n')
        && !marker.contains('\r')
}

pub fn delivery_comment(
    stage: &str,
    marker: &str,
    link: &IssueLinkRecord,
    campaign_id: &str,
    candidate_id: &str,
    pull_request_url: Option<&str>,
    merge_sha: Option<&str>,
) -> String {
    let mut body = format!(
        "{marker}\n\n**Factory delivery update**\n\n- Request: `{}`\n- Campaign: `{}`\n- Candidate: `{}`\n- Issue: `{}#{}` ({})\n- Stage: **{}**\n",
        link.request_id,
        campaign_id,
        candidate_id,
        link.repository_full_name,
        link.issue_number,
        link.role.as_str(),
        stage
    );
    if let Some(url) = pull_request_url {
        body.push_str(&format!("- Pull request: {url}\n"));
    }
    if let Some(sha) = merge_sha {
        body.push_str(&format!("- Merge commit: `{sha}`\n"));
    }
    if stage == "PARTIAL" {
        body.push_str(
            "\nThe Candidate merged, but this issue's frozen scope was not fully resolved. The issue remains open for follow-up. This comment is a Core delivery receipt and does not accept work or change the Quality Gate.\n",
        );
    } else {
        body.push_str(
            "\nThis comment is a Core delivery receipt. It does not accept work or change the Quality Gate.\n",
        );
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(role: IssueRole, work_unit_id: Option<&str>) -> IssueLinkRecord {
        IssueLinkRecord {
            id: "link-1".into(),
            project_id: "project".into(),
            request_id: "request-1".into(),
            campaign_id: Some("campaign-1".into()),
            candidate_id: None,
            repository_owner: "acme".into(),
            repository_name: "widgets".into(),
            repository_full_name: "acme/widgets".into(),
            issue_number: 7,
            github_issue_id: Some(70),
            issue_title: "Fix the widget".into(),
            role,
            state: IssueLinkState::Ready,
            delivery_state: IssueDeliveryState::None,
            resolution: IssueLinkResolution::Unresolved,
            work_unit_id: work_unit_id.map(str::to_owned),
            pull_request_number: None,
            pull_request_url: None,
            merge_sha: None,
            merged_at: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            latest_error: None,
        }
    }

    #[test]
    fn state_projection_never_completes_at_gate_acceptance() {
        assert_eq!(
            derive_issue_link_state(IssueStateFacts {
                lead_state: Some("RUNNING"),
                campaign_status: Some("AUTHORIZED"),
                gate_outcome: Some("ACCEPTED"),
                pull_request_opened: false,
                merge_sha: None,
                delivery_state: IssueDeliveryState::None,
                resolution: IssueLinkResolution::Unresolved,
            }),
            IssueLinkState::InProgress
        );
        assert_eq!(
            derive_issue_link_state(IssueStateFacts {
                lead_state: Some("RUNNING"),
                campaign_status: Some("AUTHORIZED"),
                gate_outcome: Some("ACCEPTED"),
                pull_request_opened: true,
                merge_sha: None,
                delivery_state: IssueDeliveryState::Commented,
                resolution: IssueLinkResolution::Unresolved,
            }),
            IssueLinkState::InReview
        );
        assert_eq!(
            derive_issue_link_state(IssueStateFacts {
                lead_state: Some("RUNNING"),
                campaign_status: Some("AUTHORIZED"),
                gate_outcome: Some("ACCEPTED"),
                pull_request_opened: true,
                merge_sha: Some("abc"),
                delivery_state: IssueDeliveryState::Closed,
                resolution: IssueLinkResolution::Resolved,
            }),
            IssueLinkState::Done
        );
    }

    #[test]
    fn partial_merge_stays_in_attention_and_failures_are_attention() {
        assert_eq!(
            derive_issue_link_state(IssueStateFacts {
                lead_state: Some("RUNNING"),
                campaign_status: Some("AUTHORIZED"),
                gate_outcome: Some("ACCEPTED"),
                pull_request_opened: true,
                merge_sha: Some("abc"),
                delivery_state: IssueDeliveryState::Partial,
                resolution: IssueLinkResolution::Partial,
            }),
            IssueLinkState::NeedsAttention
        );
        assert_eq!(
            derive_issue_link_state(IssueStateFacts {
                lead_state: Some("FAILED"),
                campaign_status: Some("FAILED"),
                gate_outcome: None,
                pull_request_opened: false,
                merge_sha: None,
                delivery_state: IssueDeliveryState::None,
                resolution: IssueLinkResolution::Unresolved,
            }),
            IssueLinkState::NeedsAttention
        );
        assert_eq!(
            derive_issue_link_state(IssueStateFacts {
                lead_state: Some("BLOCKED"),
                campaign_status: None,
                gate_outcome: None,
                pull_request_opened: false,
                merge_sha: None,
                delivery_state: IssueDeliveryState::None,
                resolution: IssueLinkResolution::Unresolved,
            }),
            IssueLinkState::NeedsAttention
        );
    }

    #[test]
    fn queue_and_ready_states_follow_recorded_lifecycle() {
        assert_eq!(
            derive_issue_link_state(IssueStateFacts {
                lead_state: Some("READY"),
                campaign_status: Some("BLOCKED_AUTHORIZATION"),
                gate_outcome: None,
                pull_request_opened: false,
                merge_sha: None,
                delivery_state: IssueDeliveryState::None,
                resolution: IssueLinkResolution::Unresolved,
            }),
            IssueLinkState::Queued
        );
        assert_eq!(
            derive_issue_link_state(IssueStateFacts {
                lead_state: Some("READY"),
                campaign_status: None,
                gate_outcome: None,
                pull_request_opened: false,
                merge_sha: None,
                delivery_state: IssueDeliveryState::None,
                resolution: IssueLinkResolution::Unresolved,
            }),
            IssueLinkState::Ready
        );
    }

    #[test]
    fn resolution_requires_exact_work_unit_or_single_primary() {
        let primary = link(IssueRole::Primary, None);
        assert_eq!(
            resolve_issue_link(&primary, Some("unit-a"), 1, true),
            IssueResolution::Resolved
        );
        assert_eq!(
            resolve_issue_link(&primary, Some("unit-a"), 2, true),
            IssueResolution::Partial
        );
        assert_eq!(
            resolve_issue_link(&primary, Some("unit-a"), 1, false),
            IssueResolution::Partial
        );

        let mapped = link(IssueRole::Primary, Some("unit-a"));
        assert_eq!(
            resolve_issue_link(&mapped, Some("unit-a"), 2, false),
            IssueResolution::Resolved
        );
        assert_eq!(
            resolve_issue_link(&mapped, Some("unit-b"), 1, true),
            IssueResolution::Partial
        );

        let related = link(IssueRole::Related, None);
        assert_eq!(
            resolve_issue_link(&related, Some("unit-a"), 1, true),
            IssueResolution::Partial
        );
        let related_mapped = link(IssueRole::Related, Some("unit-a"));
        assert_eq!(
            resolve_issue_link(&related_mapped, Some("unit-a"), 1, false),
            IssueResolution::Resolved
        );
    }

    #[test]
    fn markers_are_deterministic_and_bounded() {
        let marker = delivery_marker("link-1", "OPENED", "candidate-1:42");
        assert_eq!(
            marker,
            "<!-- factory:issue-link:link-1:OPENED:candidate-1:42 -->"
        );
        assert!(valid_delivery_marker(&marker));
        assert!(!valid_delivery_marker(
            "<!-- factory:issue-link:link-1\n:OPENED -->"
        ));
        assert_eq!(
            marker,
            delivery_marker("link-1", "OPENED", "candidate-1:42"),
            "the same logical event must produce the same marker"
        );
    }
}
