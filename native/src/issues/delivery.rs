//! Core-owned GitHub issue delivery effects.
//!
//! The service is the delivery hook WP3 calls after an authorized merge. It
//! never pushes, opens or merges a pull request. Its only upstream writes are
//! issue comments, labels and closes, each guarded by a deterministic ledger
//! key and an exact-marker search before a retry.

use std::time::Duration;

use crate::db::{Database, Error as DbError, IssueDeliveryEventCompletion, NewIssueDeliveryEvent};
use crate::github::{GitHubClient, GitHubError, GitHubIssue, GitHubPullRequest};
use crate::github_sync::GitHubCredential;
use crate::issues::{
    IssueDeliveryAction, IssueDeliveryEventRecord, IssueDeliveryEventStatus, IssueDeliveryFailure,
    IssueDeliveryOutcome, IssueDeliveryState, IssueLinkProjection, IssueLinkRecord,
    IssueLinkResolution, IssueLinkState, IssueResolution, IssueRole, delivery_comment,
    delivery_marker, resolve_issue_link,
};

const MAX_DELIVERY_ATTEMPTS: u32 = 3;
const RETRY_BACKOFF: Duration = Duration::from_millis(40);

#[derive(Debug, thiserror::Error)]
pub enum IssueDeliveryError {
    #[error("issue delivery database error: {0}")]
    Database(#[from] DbError),
    #[error("GitHub issue write permission is required: {0}")]
    WritePermissionRequired(String),
    #[error("{kind}: {detail}")]
    Upstream {
        kind: &'static str,
        detail: String,
        retryable: bool,
    },
    #[error("issue delivery conflict: {0}")]
    Conflict(&'static str),
    #[error("issue delivery record was not found")]
    NotFound,
}

impl IssueDeliveryError {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Database(_) => "DATABASE",
            Self::WritePermissionRequired(_) => "GITHUB_WRITE_PERMISSION_REQUIRED",
            Self::Upstream { kind, .. } => kind,
            Self::Conflict(_) => "DELIVERY_CONFLICT",
            Self::NotFound => "NOT_FOUND",
        }
    }

    pub fn retryable(&self) -> bool {
        match self {
            Self::Upstream { retryable, .. } => *retryable,
            Self::Database(_) => true,
            Self::WritePermissionRequired(_) | Self::Conflict(_) | Self::NotFound => false,
        }
    }

    pub fn detail(&self) -> String {
        self.to_string()
    }

    pub fn failure(&self) -> IssueDeliveryFailure {
        IssueDeliveryFailure {
            kind: self.kind().into(),
            detail: self.detail(),
            retryable: self.retryable(),
        }
    }

    fn blocked(kind: &'static str, detail: impl Into<String>) -> Self {
        Self::Upstream {
            kind,
            detail: detail.into(),
            retryable: false,
        }
    }

    fn transient(kind: &'static str, detail: impl Into<String>) -> Self {
        Self::Upstream {
            kind,
            detail: detail.into(),
            retryable: true,
        }
    }
}

impl From<GitHubError> for IssueDeliveryError {
    fn from(error: GitHubError) -> Self {
        let (kind, retryable) = match &error {
            GitHubError::Unauthorized => ("GITHUB_UNAUTHORIZED", false),
            GitHubError::Forbidden(_) => ("GITHUB_FORBIDDEN", false),
            GitHubError::NotFound => ("GITHUB_NOT_FOUND", false),
            GitHubError::RateLimited { .. } => ("GITHUB_RATE_LIMITED", true),
            GitHubError::Redirected => ("GITHUB_REDIRECTED", false),
            GitHubError::Rejected { .. } => ("GITHUB_REJECTED", false),
            GitHubError::MergeBlocked { .. } => ("GITHUB_BLOCKED", false),
            GitHubError::Invalid(_) => ("GITHUB_INVALID", false),
            GitHubError::Upstream(_) => ("GITHUB_UPSTREAM", true),
            GitHubError::InvalidResponse(_) => ("GITHUB_INVALID_RESPONSE", true),
        };
        Self::Upstream {
            kind,
            detail: error.detail(),
            retryable,
        }
    }
}

pub struct IssueDeliveryService<'a> {
    db: &'a Database,
    github: &'a GitHubClient,
    credential: &'a GitHubCredential,
    scopes: Vec<String>,
    scopes_reported: bool,
}

impl<'a> IssueDeliveryService<'a> {
    /// Load the exact GitHub connection generation and refuse a revoked or
    /// rate-limited credential before any issue effect is attempted.
    pub async fn new(
        db: &'a Database,
        github: &'a GitHubClient,
        credential: &'a GitHubCredential,
    ) -> Result<Self, IssueDeliveryError> {
        let connection = match db.github_connection().await {
            Ok(connection) => connection,
            Err(DbError::NotFound) => {
                return Err(IssueDeliveryError::blocked(
                    "GITHUB_NOT_CONNECTED",
                    "GitHub is not connected; issue delivery is disabled",
                ));
            }
            Err(error) => return Err(error.into()),
        };
        if !connection.enabled || connection.credential_ciphertext.is_empty() {
            return Err(IssueDeliveryError::blocked(
                "GITHUB_NOT_CONNECTED",
                "GitHub connection is revoked; issue delivery is disabled",
            ));
        }
        db.require_github_connection_revision(credential.revision())
            .await?;
        if let Some(until) = db.github_backoff_until().await? {
            return Err(IssueDeliveryError::transient(
                "GITHUB_RATE_LIMITED",
                format!("GitHub rate-limit backoff is active until {until}"),
            ));
        }
        Ok(Self {
            db,
            github,
            credential,
            scopes: connection.scopes,
            scopes_reported: connection.scopes_reported,
        })
    }

    /// Record a PR-opened fact on every frozen link and post one deterministic
    /// comment per issue. A replay returns the existing receipt without a
    /// second upstream write.
    pub async fn on_pr_opened(
        &self,
        campaign_id: &str,
        candidate_id: &str,
        pull_request: &GitHubPullRequest,
    ) -> Result<IssueDeliveryOutcome, IssueDeliveryError> {
        self.on_pr_opened_identity(
            campaign_id,
            candidate_id,
            pull_request.number,
            &pull_request.html_url,
        )
        .await
    }

    /// Same PR-opened effect from the exact recorded identity. Retry uses this
    /// entry point when the original write failed before the issue event was
    /// created.
    pub async fn on_pr_opened_identity(
        &self,
        campaign_id: &str,
        candidate_id: &str,
        pull_request_number: u64,
        pull_request_url: &str,
    ) -> Result<IssueDeliveryOutcome, IssueDeliveryError> {
        let links = self.db.issue_links_for_campaign(campaign_id).await?;
        if links.is_empty() {
            return Ok(empty_outcome(campaign_id, candidate_id));
        }
        // Record the PR fact before attempting any issue write. A read-only
        // token must not hide that the pull request already exists.
        for link in &links {
            self.db
                .mark_issue_link_review(
                    &link.id,
                    campaign_id,
                    candidate_id,
                    pull_request_number,
                    pull_request_url,
                )
                .await?;
        }
        if let Err(error) = self.ensure_write_access(&links).await {
            self.record_link_failures(&links, campaign_id, candidate_id, "pr-opened", &error)
                .await?;
            return Err(error);
        }
        for link in links {
            let marker = delivery_marker(
                &link.id,
                "OPENED",
                &format!("{candidate_id}:{pull_request_number}"),
            );
            let body = delivery_comment(
                "OPENED",
                &marker,
                &link,
                campaign_id,
                candidate_id,
                Some(pull_request_url),
                None,
            );
            let (event, _) = self
                .db
                .record_issue_delivery_event(NewIssueDeliveryEvent {
                    issue_link_id: &link.id,
                    campaign_id: Some(campaign_id),
                    candidate_id: Some(candidate_id),
                    action: IssueDeliveryAction::Comment,
                    event_key: &format!("comment:opened:{candidate_id}:{pull_request_number}"),
                    marker: Some(&marker),
                    body: Some(&body),
                    label_name: None,
                })
                .await?;
            match self.apply_event(&event, &link).await {
                Ok(_) => {
                    self.db
                        .set_issue_link_delivery_state(
                            &link.id,
                            IssueLinkState::InReview,
                            IssueDeliveryState::Commented,
                        )
                        .await?;
                }
                Err(error) => {
                    self.set_delivery_failure(&link, campaign_id, candidate_id, &error)
                        .await?;
                }
            }
        }
        self.outcome(campaign_id, candidate_id).await
    }

    /// Record a successful merge, close fully resolved issues, and comment on
    /// partially resolved issues without closing them.
    pub async fn on_merge_succeeded(
        &self,
        campaign_id: &str,
        candidate_id: &str,
        merge_sha: &str,
    ) -> Result<IssueDeliveryOutcome, IssueDeliveryError> {
        let links = self.db.issue_links_for_campaign(campaign_id).await?;
        if links.is_empty() {
            return Ok(empty_outcome(campaign_id, candidate_id));
        }
        let candidate_unit = self
            .db
            .candidate_work_unit(campaign_id, candidate_id)
            .await?;
        let primary_count = links
            .iter()
            .filter(|link| link.role == IssueRole::Primary)
            .count();
        let campaign_work_complete = self.db.campaign_work_complete(campaign_id).await?;
        let merged_at = chrono::Utc::now().to_rfc3339();
        let failure_links = links.clone();
        let mut resolved_links = Vec::with_capacity(links.len());
        for link in links {
            let resolution = resolve_issue_link(
                &link,
                candidate_unit.as_deref(),
                primary_count,
                campaign_work_complete,
            );
            let (initial_state, link_resolution) = match resolution {
                IssueResolution::Resolved => (IssueLinkState::Done, IssueLinkResolution::Resolved),
                IssueResolution::Partial => {
                    (IssueLinkState::NeedsAttention, IssueLinkResolution::Partial)
                }
            };
            self.db
                .mark_issue_link_merged(
                    &link.id,
                    campaign_id,
                    candidate_id,
                    merge_sha,
                    &merged_at,
                    initial_state,
                    IssueDeliveryState::Pending,
                    link_resolution,
                )
                .await?;
            resolved_links.push((link, resolution));
        }
        if let Err(error) = self.ensure_write_access(&failure_links).await {
            self.record_link_failures(
                &failure_links,
                campaign_id,
                candidate_id,
                "merge-succeeded",
                &error,
            )
            .await?;
            return Err(error);
        }
        for (link, resolution) in resolved_links {
            match resolution {
                IssueResolution::Resolved => {
                    let (event, _) = self
                        .db
                        .record_issue_delivery_event(NewIssueDeliveryEvent {
                            issue_link_id: &link.id,
                            campaign_id: Some(campaign_id),
                            candidate_id: Some(candidate_id),
                            action: IssueDeliveryAction::Close,
                            event_key: &format!("close:merge:{merge_sha}"),
                            marker: None,
                            body: None,
                            label_name: None,
                        })
                        .await?;
                    match self.apply_event(&event, &link).await {
                        Ok(_) => {
                            self.db
                                .set_issue_link_delivery_state(
                                    &link.id,
                                    IssueLinkState::Done,
                                    IssueDeliveryState::Closed,
                                )
                                .await?;
                        }
                        Err(error) => {
                            self.db
                                .set_issue_link_delivery_state(
                                    &link.id,
                                    IssueLinkState::Done,
                                    failure_delivery_state(&error),
                                )
                                .await?;
                        }
                    }
                }
                IssueResolution::Partial => {
                    let marker = delivery_marker(&link.id, "PARTIAL", campaign_id);
                    let body = delivery_comment(
                        "PARTIAL",
                        &marker,
                        &link,
                        campaign_id,
                        candidate_id,
                        link.pull_request_url.as_deref(),
                        Some(merge_sha),
                    );
                    let (event, _) = self
                        .db
                        .record_issue_delivery_event(NewIssueDeliveryEvent {
                            issue_link_id: &link.id,
                            campaign_id: Some(campaign_id),
                            candidate_id: Some(candidate_id),
                            action: IssueDeliveryAction::Comment,
                            event_key: "comment:partial",
                            marker: Some(&marker),
                            body: Some(&body),
                            label_name: None,
                        })
                        .await?;
                    match self.apply_event(&event, &link).await {
                        Ok(_) => {
                            self.db
                                .set_issue_link_delivery_state(
                                    &link.id,
                                    IssueLinkState::NeedsAttention,
                                    IssueDeliveryState::Partial,
                                )
                                .await?;
                        }
                        Err(error) => {
                            self.db
                                .set_issue_link_delivery_state(
                                    &link.id,
                                    IssueLinkState::NeedsAttention,
                                    failure_delivery_state(&error),
                                )
                                .await?;
                        }
                    }
                }
            }
        }
        self.outcome(campaign_id, candidate_id).await
    }

    /// Record a delivery failure without inventing GitHub state. Callers use
    /// this when a later Campaign step fails; the issue stays open.
    pub async fn on_delivery_failed(
        &self,
        campaign_id: &str,
        candidate_id: &str,
        error: &IssueDeliveryError,
    ) -> Result<IssueDeliveryOutcome, IssueDeliveryError> {
        let links = self.db.issue_links_for_campaign(campaign_id).await?;
        if links.is_empty() {
            return Ok(empty_outcome(campaign_id, candidate_id));
        }
        self.record_link_failures(&links, campaign_id, candidate_id, "delivery", error)
            .await?;
        self.outcome(campaign_id, candidate_id).await
    }

    /// Retry one failed comment, label or close event. Applied events return
    /// their existing receipt; failure summary events require the original
    /// delivery hook and return a typed conflict.
    pub async fn retry_event(
        &self,
        event_id: &str,
        actor: &str,
    ) -> Result<IssueDeliveryOutcome, IssueDeliveryError> {
        let event = self.db.issue_delivery_event(event_id).await?;
        let link = self.db.issue_link(&event.issue_link_id).await?;
        if event.status == IssueDeliveryEventStatus::Applied {
            return self.outcome_for_link(link).await;
        }
        if event.action == IssueDeliveryAction::Failure {
            self.db.record_issue_delivery_retry(event_id, actor).await?;
            return self.retry_failure_event(&link).await;
        }
        self.ensure_write_access(std::slice::from_ref(&link))
            .await?;
        self.db.record_issue_delivery_retry(event_id, actor).await?;
        match self.apply_event(&event, &link).await {
            Ok(_) => {
                let (state, delivery_state) = match event.action {
                    IssueDeliveryAction::Close => {
                        (IssueLinkState::Done, IssueDeliveryState::Closed)
                    }
                    IssueDeliveryAction::Comment
                        if event.event_key.starts_with("comment:partial") =>
                    {
                        (IssueLinkState::NeedsAttention, IssueDeliveryState::Partial)
                    }
                    IssueDeliveryAction::Comment => {
                        let state = if link.merge_sha.is_some() {
                            IssueLinkState::Done
                        } else {
                            IssueLinkState::InReview
                        };
                        (state, IssueDeliveryState::Commented)
                    }
                    IssueDeliveryAction::Label => (link.state, link.delivery_state),
                    IssueDeliveryAction::Failure => unreachable!(),
                };
                self.db
                    .set_issue_link_delivery_state(&link.id, state, delivery_state)
                    .await?;
            }
            Err(error) => {
                self.db
                    .set_issue_link_delivery_state(
                        &link.id,
                        link.state,
                        failure_delivery_state(&error),
                    )
                    .await?;
                return Err(error);
            }
        }
        self.outcome_for_link(self.db.issue_link(&link.id).await?)
            .await
    }

    async fn retry_failure_event(
        &self,
        link: &IssueLinkRecord,
    ) -> Result<IssueDeliveryOutcome, IssueDeliveryError> {
        let campaign_id = link
            .campaign_id
            .as_deref()
            .ok_or(IssueDeliveryError::Conflict(
                "the issue link has no recorded Campaign to retry",
            ))?;
        let candidate_id = link
            .candidate_id
            .as_deref()
            .ok_or(IssueDeliveryError::Conflict(
                "the issue link has no recorded candidate to retry",
            ))?;
        if let Some(merge_sha) = link.merge_sha.as_deref() {
            return self
                .on_merge_succeeded(campaign_id, candidate_id, merge_sha)
                .await;
        }
        if let (Some(number), Some(url)) =
            (link.pull_request_number, link.pull_request_url.as_deref())
        {
            return self
                .on_pr_opened_identity(campaign_id, candidate_id, number, url)
                .await;
        }
        Err(IssueDeliveryError::Conflict(
            "no recorded pull request or merge fact can reconstruct this delivery attempt",
        ))
    }

    async fn ensure_write_access(
        &self,
        links: &[IssueLinkRecord],
    ) -> Result<(), IssueDeliveryError> {
        let Some(link) = links.first() else {
            return Ok(());
        };
        let repository = self
            .github
            .repository(
                self.credential.token(),
                &link.repository_owner,
                &link.repository_name,
            )
            .await?;
        if let Some(permissions) = repository.permissions {
            if permissions.admin || permissions.maintain || permissions.push || permissions.triage {
                return Ok(());
            }
            return Err(IssueDeliveryError::WritePermissionRequired(format!(
                "the token has read-only repository permissions for {}",
                link.repository_full_name
            )));
        }
        if self.scopes_reported
            && !self.scopes.iter().any(|scope| {
                scope.eq_ignore_ascii_case("repo") || scope.eq_ignore_ascii_case("public_repo")
            })
        {
            return Err(IssueDeliveryError::WritePermissionRequired(format!(
                "reported token scopes do not include issue write access for {}",
                link.repository_full_name
            )));
        }
        // Fine-grained tokens do not report OAuth scopes. When repository
        // permissions are absent Core cannot prove write access locally; the
        // first upstream write remains authoritative and a 403 is typed.
        Ok(())
    }

    async fn apply_event(
        &self,
        event: &IssueDeliveryEventRecord,
        link: &IssueLinkRecord,
    ) -> Result<IssueDeliveryEventRecord, IssueDeliveryError> {
        if event.status == IssueDeliveryEventStatus::Applied {
            return Ok(event.clone());
        }
        let mut last_error =
            IssueDeliveryError::transient("GITHUB_DELIVERY_UNKNOWN", "issue delivery did not run");
        for _ in 0..MAX_DELIVERY_ATTEMPTS {
            let attempt = self.db.start_issue_delivery_attempt(&event.id).await?;
            let result = match event.action {
                IssueDeliveryAction::Comment => self.comment_once(link, event).await,
                IssueDeliveryAction::Label => self.label_once(link, event).await,
                IssueDeliveryAction::Close => self.close_once(link).await,
                IssueDeliveryAction::Failure => Err(IssueDeliveryError::Conflict(
                    "failure summary events do not perform GitHub writes",
                )),
            };
            match result {
                Ok(external_id) => {
                    return self
                        .db
                        .complete_issue_delivery_event(
                            &event.id,
                            IssueDeliveryEventCompletion {
                                status: IssueDeliveryEventStatus::Applied,
                                external_id: Some(&external_id),
                                error_kind: None,
                                error_detail: None,
                            },
                        )
                        .await
                        .map_err(Into::into);
                }
                Err(error) => {
                    let retry = error.retryable() && attempt.attempts < MAX_DELIVERY_ATTEMPTS;
                    last_error = error;
                    if retry {
                        tokio::time::sleep(RETRY_BACKOFF * attempt.attempts).await;
                        continue;
                    }
                    let status = if last_error.retryable() {
                        IssueDeliveryEventStatus::Failed
                    } else {
                        IssueDeliveryEventStatus::Blocked
                    };
                    self.db
                        .complete_issue_delivery_event(
                            &event.id,
                            IssueDeliveryEventCompletion {
                                status,
                                external_id: None,
                                error_kind: Some(last_error.kind()),
                                error_detail: Some(&last_error.detail()),
                            },
                        )
                        .await?;
                    return Err(last_error);
                }
            }
        }
        Err(last_error)
    }

    async fn comment_once(
        &self,
        link: &IssueLinkRecord,
        event: &IssueDeliveryEventRecord,
    ) -> Result<String, IssueDeliveryError> {
        let marker = event
            .marker
            .as_deref()
            .ok_or(IssueDeliveryError::Conflict("comment event has no marker"))?;
        let body = event
            .body
            .as_deref()
            .ok_or(IssueDeliveryError::Conflict("comment event has no body"))?;
        let comments = self
            .github
            .issue_comments(
                self.credential.token(),
                &link.repository_owner,
                &link.repository_name,
                link.issue_number,
            )
            .await?;
        if let Some(existing) = comments
            .comments
            .iter()
            .find(|comment| comment.body.contains(marker))
        {
            return Ok(existing.id.to_string());
        }
        if comments.truncated {
            return Err(IssueDeliveryError::blocked(
                "ISSUE_COMMENT_WINDOW_TRUNCATED",
                "the bounded GitHub comment window is full and the deterministic marker was not observed; resolve manually before retrying",
            ));
        }
        let comment = self
            .github
            .create_issue_comment(
                self.credential.token(),
                &link.repository_owner,
                &link.repository_name,
                link.issue_number,
                body,
            )
            .await?;
        Ok(comment.id.to_string())
    }

    async fn label_once(
        &self,
        link: &IssueLinkRecord,
        event: &IssueDeliveryEventRecord,
    ) -> Result<String, IssueDeliveryError> {
        let label = event
            .label_name
            .as_deref()
            .ok_or(IssueDeliveryError::Conflict("label event has no label"))?;
        let issue = self
            .github
            .get_issue(
                self.credential.token(),
                &link.repository_owner,
                &link.repository_name,
                link.issue_number,
            )
            .await?;
        if issue
            .labels
            .iter()
            .any(|existing| existing.name.eq_ignore_ascii_case(label))
        {
            return Ok(label.to_owned());
        }
        let labels = self
            .github
            .add_issue_labels(
                self.credential.token(),
                &link.repository_owner,
                &link.repository_name,
                link.issue_number,
                &[label.to_owned()],
            )
            .await?;
        Ok(labels
            .first()
            .map(|label| label.name.clone())
            .unwrap_or_else(|| label.to_owned()))
    }

    async fn close_once(&self, link: &IssueLinkRecord) -> Result<String, IssueDeliveryError> {
        let issue = self
            .github
            .get_issue(
                self.credential.token(),
                &link.repository_owner,
                &link.repository_name,
                link.issue_number,
            )
            .await?;
        if issue.state == "closed" {
            return Ok(issue.number.to_string());
        }
        let closed: GitHubIssue = self
            .github
            .close_issue(
                self.credential.token(),
                &link.repository_owner,
                &link.repository_name,
                link.issue_number,
                Some("completed"),
            )
            .await?;
        Ok(closed.number.to_string())
    }

    async fn record_link_failures(
        &self,
        links: &[IssueLinkRecord],
        campaign_id: &str,
        candidate_id: &str,
        stage: &str,
        error: &IssueDeliveryError,
    ) -> Result<(), IssueDeliveryError> {
        for link in links {
            let event_key = format!(
                "failed:{stage}:{candidate_id}:{}",
                error.kind().to_ascii_lowercase()
            );
            let (event, _) = self
                .db
                .record_issue_delivery_event(NewIssueDeliveryEvent {
                    issue_link_id: &link.id,
                    campaign_id: Some(campaign_id),
                    candidate_id: Some(candidate_id),
                    action: IssueDeliveryAction::Failure,
                    event_key: &event_key,
                    marker: None,
                    body: None,
                    label_name: None,
                })
                .await?;
            self.db
                .complete_issue_delivery_event(
                    &event.id,
                    IssueDeliveryEventCompletion {
                        status: if error.retryable() {
                            IssueDeliveryEventStatus::Failed
                        } else {
                            IssueDeliveryEventStatus::Blocked
                        },
                        external_id: None,
                        error_kind: Some(error.kind()),
                        error_detail: Some(&error.detail()),
                    },
                )
                .await?;
            self.db
                .set_issue_link_delivery_state(
                    &link.id,
                    IssueLinkState::NeedsAttention,
                    failure_delivery_state(error),
                )
                .await?;
        }
        Ok(())
    }

    async fn set_delivery_failure(
        &self,
        link: &IssueLinkRecord,
        campaign_id: &str,
        candidate_id: &str,
        error: &IssueDeliveryError,
    ) -> Result<(), IssueDeliveryError> {
        self.record_link_failures(
            std::slice::from_ref(link),
            campaign_id,
            candidate_id,
            "issue-effect",
            error,
        )
        .await
    }

    async fn outcome(
        &self,
        campaign_id: &str,
        candidate_id: &str,
    ) -> Result<IssueDeliveryOutcome, IssueDeliveryError> {
        let links = self
            .db
            .issue_link_projections_for_campaign(campaign_id)
            .await?;
        Ok(outcome_from_links(campaign_id, candidate_id, links))
    }

    async fn outcome_for_link(
        &self,
        link: IssueLinkRecord,
    ) -> Result<IssueDeliveryOutcome, IssueDeliveryError> {
        let events = self.db.issue_delivery_events_for_link(&link.id).await?;
        let campaign_id = link.campaign_id.clone().unwrap_or_default();
        let candidate_id = link.candidate_id.clone().unwrap_or_default();
        Ok(outcome_from_links(
            &campaign_id,
            &candidate_id,
            vec![IssueLinkProjection { link, events }],
        ))
    }
}

fn empty_outcome(campaign_id: &str, candidate_id: &str) -> IssueDeliveryOutcome {
    IssueDeliveryOutcome {
        campaign_id: campaign_id.into(),
        candidate_id: candidate_id.into(),
        attempted: 0,
        applied: 0,
        partial: 0,
        failed: 0,
        failures: Vec::new(),
        links: Vec::new(),
    }
}

fn outcome_from_links(
    campaign_id: &str,
    candidate_id: &str,
    links: Vec<IssueLinkProjection>,
) -> IssueDeliveryOutcome {
    let attempted = links.len() as u32;
    let partial = links
        .iter()
        .filter(|link| link.link.delivery_state == IssueDeliveryState::Partial)
        .count() as u32;
    let failed = links
        .iter()
        .filter(|link| {
            matches!(
                link.link.delivery_state,
                IssueDeliveryState::Failed | IssueDeliveryState::Blocked
            )
        })
        .count() as u32;
    let applied = links
        .iter()
        .filter(|link| {
            matches!(
                link.link.delivery_state,
                IssueDeliveryState::Commented | IssueDeliveryState::Closed
            )
        })
        .count() as u32;
    let failures = links
        .iter()
        .filter_map(|link| link.link.latest_error.clone())
        .collect();
    IssueDeliveryOutcome {
        campaign_id: campaign_id.into(),
        candidate_id: candidate_id.into(),
        attempted,
        applied,
        partial,
        failed,
        failures,
        links,
    }
}

fn failure_delivery_state(error: &IssueDeliveryError) -> IssueDeliveryState {
    if error.retryable() {
        IssueDeliveryState::Failed
    } else {
        IssueDeliveryState::Blocked
    }
}
