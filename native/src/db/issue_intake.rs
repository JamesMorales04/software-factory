//! Persistence for default-off Project issue-intake policy revisions, finite
//! acknowledged batches and the deterministic reconciliation queue.
//!
//! The policy and batch records are append-only evidence. A batch creates a
//! non-autonomous standing delegation so ordinary intake records have a concrete
//! policy reference, but no code here starts a Campaign or calls a provider.

use super::*;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use sqlx::row::Row;
use sqlx_sqlite::{SqliteConnection, SqlitePool};
use uuid::Uuid;

use crate::github::GitHubLabel;
use crate::intake::IntakeMode;
use crate::issue_intake::{
    AcknowledgedIssue, BatchFacts, IssueIntakeAuditRecord, IssueIntakeBatchItemRecord,
    IssueIntakeBatchRecord, IssueIntakeEligibility, IssueIntakePolicyRecord,
    IssueIntakeQueueItemRecord, IssueSnapshot, PolicyFacts, ProjectFacts, batch_content_hash,
    evaluate_issue_intake_eligibility, expiry_within_policy, issue_objective, policy_content_hash,
    queue_item_id_for_batch_item, validate_policy_terms,
};
use crate::lead::{
    Capability, MAX_CUMULATIVE_ATTEMPTS, MAX_CUMULATIVE_SESSIONS, ProjectConfig, RiskClass,
    StandingDelegation,
};

use super::delegations::{create_standing_delegation_in_tx, load_delegation};
use super::execution_profiles::load_current_content;
use super::intake::{
    create_intake_request_in_tx, load_exact_intake_profile_in_tx, load_intake_request,
    prepare_intake_request,
};
use super::project_configs::load_project_config;

const POLICY_HISTORY_LIMIT: i64 = 20;
const BATCH_HISTORY_LIMIT: i64 = 20;
const QUEUE_LIMIT: i64 = 200;
const AUDIT_LIMIT: i64 = 100;
const MAX_REASON_CHARS: usize = 2_000;
const MAX_LABELS: usize = 20;
const MAX_AUTHOR_IDS: usize = 100;
const MAX_PATHS: usize = 100;

#[derive(Debug, Clone)]
pub struct NewIssueIntakePolicy<'a> {
    pub project_id: &'a str,
    pub repository_full_name: &'a str,
    pub repository_github_id: i64,
    pub required_labels: &'a [String],
    pub excluded_labels: &'a [String],
    pub author_ids: &'a [i64],
    pub author_associations: &'a [String],
    pub profile_id: &'a str,
    pub profile_revision_id: &'a str,
    pub profile_revision: u32,
    pub profile_hash: &'a str,
    pub project_config_hash: &'a str,
    pub allowed_paths: &'a [String],
    pub capabilities: &'a [Capability],
    pub max_issues: u32,
    pub max_concurrent: u32,
    pub max_cost_micros_per_issue: u64,
    pub max_total_cost_micros: u64,
    pub max_attempts: u32,
    pub max_sessions: u32,
    pub max_turns: u32,
    pub max_wall_clock_secs: u64,
    pub expires_at: &'a str,
    pub acknowledge: bool,
    pub reason: &'a str,
    pub idempotency_key: &'a str,
    pub principal_id: &'a str,
    pub principal_label: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct IssueIntakePolicyResult {
    pub policy: IssueIntakePolicyRecord,
    pub created: bool,
}

#[derive(Debug, Clone)]
pub struct NewIssueIntakeBatchItem<'a> {
    pub number: u64,
    pub github_issue_id: i64,
    pub content_hash: &'a str,
    pub updated_at: &'a str,
}

#[derive(Debug, Clone)]
pub struct NewIssueIntakeBatch<'a> {
    pub project_id: &'a str,
    pub policy_id: &'a str,
    pub expected_policy_revision: u32,
    pub expected_policy_hash: &'a str,
    pub items: &'a [NewIssueIntakeBatchItem<'a>],
    pub max_total_cost_micros: u64,
    pub expires_at: &'a str,
    pub acknowledge: bool,
    pub reason: &'a str,
    pub idempotency_key: &'a str,
    pub principal_id: &'a str,
    pub principal_label: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct IssueIntakeBatchResult {
    pub batch: IssueIntakeBatchRecord,
    pub items: Vec<IssueIntakeBatchItemRecord>,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct IssueIntakeItemState {
    pub item: IssueIntakeBatchItemRecord,
    pub current_eligibility: Option<IssueIntakeEligibility>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IssueIntakeBatchState {
    pub batch: IssueIntakeBatchRecord,
    pub items: Vec<IssueIntakeItemState>,
    pub remaining_issues: u32,
    pub remaining_cost_micros: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct IssueIntakeState {
    pub project_id: String,
    pub enabled: bool,
    pub policy: Option<IssueIntakePolicyRecord>,
    pub policy_history: Vec<IssueIntakePolicyRecord>,
    pub batches: Vec<IssueIntakeBatchState>,
    pub queue: Vec<IssueIntakeQueueItemRecord>,
    pub audit: Vec<IssueIntakeAuditRecord>,
    pub binding: Option<ProjectGitHubBinding>,
    pub connection_enabled: bool,
    pub connection_revision: Option<u32>,
    pub limits: IssueIntakeLimits,
}

#[derive(Debug, Clone)]
pub struct ClaimedIssueIntakeItem {
    pub queue: IssueIntakeQueueItemRecord,
    pub batch: IssueIntakeBatchRecord,
    pub policy: IssueIntakePolicyRecord,
    pub item: IssueIntakeBatchItemRecord,
    pub binding: ProjectGitHubBinding,
    pub lease_token: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IssueIntakeReconcileResult {
    pub status: String,
    pub reasons: Vec<String>,
    pub queue: IssueIntakeQueueItemRecord,
    pub intake: Option<IntakeRequestRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueIntakeQueueFailure {
    Transient,
    Terminal,
}

#[derive(Debug, Clone, Serialize)]
pub struct IssueIntakeLimits {
    pub max_batch_issues: u32,
    pub max_concurrent: u32,
    pub max_queue_attempts: u32,
    pub queue_lease_seconds: i64,
    pub autonomous_start: bool,
    pub campaign_start: bool,
    pub provider_calls: bool,
}

impl Database {
    /// Enable or edit a Project issue-intake policy. Every call creates a new
    /// immutable revision and revokes old active batches without replenishing
    /// their reservations. Absence of a current head is disabled.
    pub async fn create_issue_intake_policy(
        &self,
        input: NewIssueIntakePolicy<'_>,
    ) -> Result<IssueIntakePolicyResult, Error> {
        validate_new_policy(&input)?;
        let required_labels = normalize_labels(input.required_labels);
        let excluded_labels = normalize_labels(input.excluded_labels);
        let mut author_ids = input.author_ids.to_vec();
        author_ids.sort_unstable();
        author_ids.dedup();
        let mut author_associations = input
            .author_associations
            .iter()
            .map(|value| value.to_ascii_uppercase())
            .collect::<Vec<_>>();
        author_associations.sort();
        author_associations.dedup();
        let allowed_paths = normalize_paths(input.allowed_paths);
        let mut capabilities = input.capabilities.to_vec();
        capabilities.sort_by_key(|capability| format!("{capability:?}"));
        capabilities.dedup();
        let digest = policy_request_digest(
            &input,
            &required_labels,
            &excluded_labels,
            &author_ids,
            &author_associations,
            &allowed_paths,
            &capabilities,
        );
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(policy_id) = replay(
            &mut transaction,
            input.idempotency_key,
            "create_issue_intake_policy",
            &digest,
        )
        .await?
        {
            let policy = load_policy_by_id(&mut transaction, input.project_id, &policy_id).await?;
            transaction.commit().await?;
            return Ok(IssueIntakePolicyResult {
                policy,
                created: false,
            });
        }
        let config = load_project_config(&mut transaction, input.project_id).await?;
        if !config.active || config.config_hash != input.project_config_hash {
            return Err(intake_conflict(
                "PROJECT_CONFIG_CHANGED",
                "the Project configuration is inactive or the acknowledged hash is stale",
            ));
        }
        let binding = load_binding(&mut transaction, input.project_id).await?;
        if binding.repo_full_name != input.repository_full_name {
            return Err(intake_conflict(
                "ISSUE_POLICY_REPOSITORY_MISMATCH",
                "the repository does not match the Project GitHub binding",
            ));
        }
        let Some(binding_repository_id) = binding.repository_github_id else {
            return Err(intake_conflict(
                "ISSUE_POLICY_REPOSITORY_ID_UNKNOWN",
                "the Project GitHub binding has no immutable repository ID; rebind from GitHub discovery",
            ));
        };
        if input.repository_github_id != binding_repository_id {
            return Err(intake_conflict(
                "ISSUE_POLICY_REPOSITORY_ID_MISMATCH",
                "the acknowledged immutable repository ID does not match the binding",
            ));
        }
        let (connection_enabled, connection_revision) =
            load_connection_generation(&mut transaction).await?;
        if !connection_enabled {
            return Err(intake_conflict(
                "GITHUB_CONNECTION_REVOKED",
                "GitHub is revoked; issue intake cannot be enabled",
            ));
        }
        let current = load_current_content(&mut transaction, input.profile_id)
            .await
            .map_err(|error| match error {
                Error::NotFound => intake_conflict(
                    "EXECUTION_PROFILE_REQUIRED",
                    "the selected execution profile does not exist",
                ),
                other => other,
            })?;
        if current.revision != input.profile_revision
            || current.revision_id != input.profile_revision_id
        {
            return Err(intake_conflict(
                "EXECUTION_PROFILE_STALE",
                "the acknowledged execution profile revision is not current",
            ));
        }
        let profile = load_exact_intake_profile_in_tx(
            &mut transaction,
            &self.provider_model_cache(),
            input.profile_id,
            input.profile_revision_id,
            input.profile_revision,
            input.profile_hash,
        )
        .await?;
        if let Err(code) = validate_policy_terms(
            &profile.content,
            &config,
            &required_labels,
            &excluded_labels,
            &author_ids,
            &author_associations,
            &allowed_paths,
            &capabilities,
            input.max_issues,
            input.max_concurrent,
            input.max_cost_micros_per_issue,
            input.max_total_cost_micros,
            input.max_attempts,
            input.max_sessions,
            input.max_turns,
            input.max_wall_clock_secs,
        ) {
            return Err(intake_conflict(
                code,
                "the issue-intake policy bounds are not valid for this Project and profile",
            ));
        }
        let now = Utc::now();
        if !expiry_within_policy(input.expires_at, now) {
            return Err(intake_conflict(
                "ISSUE_POLICY_EXPIRY",
                "expiry must be in the future and within 30 days",
            ));
        }
        let revision: i64 = sqlx::query_scalar::query_scalar(
            "SELECT COALESCE(MAX(revision),0)+1 FROM issue_intake_policy_revisions
             WHERE project_id=?1",
        )
        .bind(input.project_id)
        .fetch_one(&mut *transaction)
        .await?;
        let revision = u32::try_from(revision)
            .map_err(|_| Error::Conflict("issue intake policy revision overflow"))?;
        let mut policy = IssueIntakePolicyRecord {
            id: Uuid::new_v4().to_string(),
            project_id: input.project_id.into(),
            revision,
            enabled: true,
            repository_full_name: input.repository_full_name.into(),
            repository_github_id: Some(input.repository_github_id),
            connection_revision,
            required_labels,
            excluded_labels,
            author_ids,
            author_associations,
            profile_id: input.profile_id.into(),
            profile_revision_id: input.profile_revision_id.into(),
            profile_revision: input.profile_revision,
            profile_hash: input.profile_hash.into(),
            project_config_hash: input.project_config_hash.into(),
            allowed_paths,
            capabilities,
            max_risk_class: "R0".into(),
            max_issues: input.max_issues,
            max_concurrent: input.max_concurrent,
            max_cost_micros_per_issue: input.max_cost_micros_per_issue,
            max_total_cost_micros: input.max_total_cost_micros,
            max_attempts: input.max_attempts,
            max_sessions: input.max_sessions,
            max_turns: input.max_turns,
            max_wall_clock_secs: input.max_wall_clock_secs,
            expires_at: input.expires_at.into(),
            acknowledged: true,
            principal_id: input.principal_id.into(),
            principal_label: input.principal_label.map(str::to_owned),
            reason: input.reason.into(),
            idempotency_key: input.idempotency_key.into(),
            content_hash: String::new(),
            created_at: now.to_rfc3339(),
        };
        policy.content_hash = policy_content_hash(&policy);
        insert_policy(&mut transaction, &policy).await?;
        sqlx::query::query(
            "INSERT INTO issue_intake_policy_heads(project_id,policy_id,revision,updated_at)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(project_id) DO UPDATE SET
               policy_id=excluded.policy_id, revision=excluded.revision,
               updated_at=excluded.updated_at",
        )
        .bind(&policy.project_id)
        .bind(&policy.id)
        .bind(i64::from(policy.revision))
        .bind(&policy.created_at)
        .execute(&mut *transaction)
        .await?;
        revoke_active_batches_for_project(
            &mut transaction,
            &policy.project_id,
            "ISSUE_POLICY_SUPERSEDED",
            input.principal_id,
            &policy.created_at,
        )
        .await?;
        insert_issue_intake_audit(
            &mut transaction,
            &policy.project_id,
            Some(&policy.id),
            None,
            None,
            if revision == 1 {
                "policy_enabled"
            } else {
                "policy_updated"
            },
            input.principal_label.unwrap_or(input.principal_id),
            input.principal_id,
            input.reason,
            &format!(
                "revision={} repository={} id={} profile={} revision={}",
                policy.revision,
                policy.repository_full_name,
                input.repository_github_id,
                policy.profile_id,
                policy.profile_revision
            ),
            &policy.created_at,
        )
        .await?;
        record_event(
            &mut transaction,
            &policy.id,
            if revision == 1 {
                "issue_intake.policy_configured"
            } else {
                "issue_intake.policy_updated"
            },
            &format!(
                "project={} revision={} repository={} autonomous_start=false",
                policy.project_id, policy.revision, policy.repository_full_name
            ),
        )
        .await?;
        sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
            .bind(input.idempotency_key)
            .bind("create_issue_intake_policy")
            .bind(&digest)
            .bind(&policy.id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(IssueIntakePolicyResult {
            policy,
            created: true,
        })
    }

    /// Disable issue intake with a new immutable revision. The previous
    /// configuration is retained for audit; every active batch is revoked.
    pub async fn disable_issue_intake_policy(
        &self,
        project_id: &str,
        reason: &str,
        idempotency_key: &str,
        principal_id: &str,
        principal_label: Option<&str>,
    ) -> Result<IssueIntakePolicyResult, Error> {
        validate_reason(reason)?;
        validate_key(idempotency_key)?;
        validate_principal(principal_id, principal_label)?;
        let digest = sha256_hex(
            serde_json::to_string(&(
                "disable-issue-intake-policy-v1",
                project_id,
                reason,
                principal_id,
            ))
            .expect("policy disable digest serializes")
            .as_bytes(),
        );
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(policy_id) = replay(
            &mut transaction,
            idempotency_key,
            "disable_issue_intake_policy",
            &digest,
        )
        .await?
        {
            let policy = load_policy_by_id(&mut transaction, project_id, &policy_id).await?;
            transaction.commit().await?;
            return Ok(IssueIntakePolicyResult {
                policy,
                created: false,
            });
        }
        let previous = load_policy_head(&mut transaction, project_id).await?;
        if !previous.enabled {
            transaction.commit().await?;
            return Ok(IssueIntakePolicyResult {
                policy: previous,
                created: false,
            });
        }
        let now = Utc::now().to_rfc3339();
        let revision: i64 = sqlx::query_scalar::query_scalar(
            "SELECT COALESCE(MAX(revision),0)+1 FROM issue_intake_policy_revisions
             WHERE project_id=?1",
        )
        .bind(project_id)
        .fetch_one(&mut *transaction)
        .await?;
        let mut policy = IssueIntakePolicyRecord {
            id: Uuid::new_v4().to_string(),
            revision: u32::try_from(revision)
                .map_err(|_| Error::Conflict("issue intake policy revision overflow"))?,
            enabled: false,
            reason: reason.into(),
            principal_id: principal_id.into(),
            principal_label: principal_label.map(str::to_owned),
            idempotency_key: idempotency_key.into(),
            created_at: now.clone(),
            content_hash: String::new(),
            ..previous
        };
        policy.content_hash = policy_content_hash(&policy);
        insert_policy(&mut transaction, &policy).await?;
        sqlx::query::query(
            "UPDATE issue_intake_policy_heads
             SET policy_id=?1, revision=?2, updated_at=?3 WHERE project_id=?4",
        )
        .bind(&policy.id)
        .bind(i64::from(policy.revision))
        .bind(&now)
        .bind(project_id)
        .execute(&mut *transaction)
        .await?;
        revoke_active_batches_for_project(
            &mut transaction,
            project_id,
            "ISSUE_POLICY_DISABLED",
            principal_id,
            &now,
        )
        .await?;
        insert_issue_intake_audit(
            &mut transaction,
            project_id,
            Some(&policy.id),
            None,
            None,
            "policy_disabled",
            principal_label.unwrap_or(principal_id),
            principal_id,
            reason,
            &format!("revision={}", policy.revision),
            &now,
        )
        .await?;
        record_event(
            &mut transaction,
            &policy.id,
            "issue_intake.policy_disabled",
            &format!("project={project_id} revision={}", policy.revision),
        )
        .await?;
        sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
            .bind(idempotency_key)
            .bind("disable_issue_intake_policy")
            .bind(&digest)
            .bind(&policy.id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(IssueIntakePolicyResult {
            policy,
            created: true,
        })
    }

    /// Acknowledge a finite set of exact issue snapshots. The server re-reads
    /// every snapshot; client-supplied digests only prove which revision the
    /// operator saw. A mismatch fails the whole acknowledgement.
    pub async fn create_issue_intake_batch(
        &self,
        input: NewIssueIntakeBatch<'_>,
    ) -> Result<IssueIntakeBatchResult, Error> {
        validate_batch_input(&input)?;
        let digest = batch_request_digest(&input);
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(batch_id) = replay(
            &mut transaction,
            input.idempotency_key,
            "create_issue_intake_batch",
            &digest,
        )
        .await?
        {
            let batch = load_batch(&mut transaction, input.project_id, &batch_id).await?;
            let items = load_batch_items(&mut transaction, input.project_id, &batch.id).await?;
            transaction.commit().await?;
            return Ok(IssueIntakeBatchResult {
                batch,
                items,
                created: false,
            });
        }
        let policy = load_policy_head(&mut transaction, input.project_id).await?;
        if !policy.enabled {
            return Err(intake_conflict(
                "ISSUE_INTAKE_DISABLED",
                "the Project issue-intake policy is disabled",
            ));
        }
        if policy.id != input.policy_id
            || policy.revision != input.expected_policy_revision
            || policy.content_hash != input.expected_policy_hash
        {
            return Err(intake_conflict(
                "ISSUE_POLICY_STALE",
                "the acknowledged issue-intake policy is no longer current",
            ));
        }
        if !deadline_in_future(&policy.expires_at, Utc::now()) {
            return Err(intake_conflict(
                "ISSUE_POLICY_EXPIRED",
                "the Project issue-intake policy is expired",
            ));
        }
        let config = load_project_config(&mut transaction, input.project_id).await?;
        if !config.active || config.config_hash != policy.project_config_hash {
            return Err(intake_conflict(
                "PROJECT_CONFIG_CHANGED",
                "the Project configuration changed after the policy was enabled",
            ));
        }
        let binding = load_binding(&mut transaction, input.project_id).await?;
        if binding.repo_full_name != policy.repository_full_name
            || binding.repository_github_id != policy.repository_github_id
        {
            return Err(intake_conflict(
                "ISSUE_POLICY_REPOSITORY_MISMATCH",
                "the Project repository binding changed after the policy was enabled",
            ));
        }
        let (connection_enabled, connection_revision) =
            load_connection_generation(&mut transaction).await?;
        if !connection_enabled || connection_revision != policy.connection_revision {
            return Err(intake_conflict(
                "GITHUB_CONNECTION_CHANGED",
                "the GitHub connection was revoked or replaced after the policy was enabled",
            ));
        }
        let profile = load_exact_intake_profile_in_tx(
            &mut transaction,
            &self.provider_model_cache(),
            &policy.profile_id,
            &policy.profile_revision_id,
            policy.profile_revision,
            &policy.profile_hash,
        )
        .await?;
        let Some(repository_github_id) = policy.repository_github_id else {
            return Err(intake_conflict(
                "ISSUE_POLICY_REPOSITORY_ID_UNKNOWN",
                "the policy has no immutable repository ID",
            ));
        };
        let now = Utc::now();
        if !deadline_in_future(input.expires_at, now)
            || !deadline_at_or_before(input.expires_at, &policy.expires_at)
        {
            return Err(intake_conflict(
                "ISSUE_BATCH_EXPIRY",
                "batch expiry must be in the future and no later than the policy expiry",
            ));
        }
        let selected =
            u32::try_from(input.items.len()).map_err(|_| Error::Invalid("issue batch size"))?;
        if selected == 0 || selected > policy.max_issues {
            return Err(intake_conflict(
                "ISSUE_BATCH_SIZE",
                "the batch must contain between one and the policy maximum number of issues",
            ));
        }
        if input.max_total_cost_micros == 0
            || input.max_total_cost_micros > policy.max_total_cost_micros
            || u64::from(selected).saturating_mul(policy.max_cost_micros_per_issue)
                > input.max_total_cost_micros
        {
            return Err(intake_conflict(
                "ISSUE_BATCH_BUDGET_EXCEEDED",
                "the selected snapshots exceed the acknowledged batch budget",
            ));
        }
        let mut seen_numbers = std::collections::BTreeSet::new();
        let mut seen_ids = std::collections::BTreeSet::new();
        for item in input.items {
            if !seen_numbers.insert(item.number) || !seen_ids.insert(item.github_issue_id) {
                return Err(intake_conflict(
                    "ISSUE_BATCH_DUPLICATE",
                    "the same issue was selected more than once",
                ));
            }
        }
        let batch_id = Uuid::new_v4().to_string();
        let mut items = Vec::with_capacity(input.items.len());
        let policy_facts = policy_facts(&policy, now);
        let project_facts = project_facts_for_claim(
            &config,
            &binding,
            connection_enabled,
            connection_revision,
            &policy.profile_revision_id,
            &policy.profile_hash,
            0,
            false,
        );
        let batch_facts = BatchFacts {
            status: "ACTIVE",
            expired: false,
            repository_full_name: &policy.repository_full_name,
            repository_github_id,
            connection_revision,
            max_issues: selected,
            reserved_issues: 0,
            max_total_cost_micros: input.max_total_cost_micros,
            reserved_cost_micros: 0,
            max_cost_micros_per_issue: policy.max_cost_micros_per_issue,
            profile_revision_id: &policy.profile_revision_id,
            profile_hash: &policy.profile_hash,
            project_config_hash: &policy.project_config_hash,
        };
        for selected_item in input.items {
            let snapshot = load_issue_snapshot(
                &mut transaction,
                input.project_id,
                &policy.repository_full_name,
                selected_item.number,
            )
            .await?
            .ok_or_else(|| {
                intake_conflict(
                    "ISSUE_SNAPSHOT_MISSING",
                    "a selected issue is not in the synchronized snapshot",
                )
            })?;
            if snapshot.github_id != selected_item.github_issue_id
                || snapshot.content_hash != selected_item.content_hash
                || snapshot.updated_at != selected_item.updated_at
            {
                return Err(intake_conflict(
                    "ISSUE_SNAPSHOT_STALE",
                    "a selected issue changed after the operator viewed it; refresh and acknowledge again",
                ));
            }
            let labels = snapshot.label_names();
            let acknowledged = AcknowledgedIssue {
                project_id: &policy.project_id,
                repository_full_name: &policy.repository_full_name,
                repository_github_id,
                issue_number: snapshot.number,
                github_issue_id: snapshot.github_id,
                content_hash: &snapshot.content_hash,
                issue_updated_at: &snapshot.updated_at,
                author_id: snapshot.author_id,
                author_association: snapshot.author_association.as_deref(),
                labels: &labels,
            };
            let eligibility = evaluate_issue_intake_eligibility(
                Some(&policy_facts),
                &batch_facts,
                &acknowledged,
                &snapshot,
                &project_facts,
            );
            if !eligibility.eligible {
                return Err(Error::IntakeConflict {
                    code: "ISSUE_BATCH_INELIGIBLE",
                    message: format!(
                        "issue #{} is not eligible for acknowledgement: {}",
                        snapshot.number,
                        eligibility.reasons.join(", ")
                    ),
                });
            }
            let item_id = Uuid::new_v4().to_string();
            items.push(IssueIntakeBatchItemRecord {
                id: item_id,
                batch_id: batch_id.clone(),
                project_id: policy.project_id.clone(),
                repository_full_name: policy.repository_full_name.clone(),
                repository_github_id,
                issue_number: snapshot.number,
                github_issue_id: snapshot.github_id,
                content_hash: snapshot.content_hash.clone(),
                issue_updated_at: snapshot.updated_at.clone(),
                author_login: snapshot.author.clone(),
                author_id: snapshot.author_id,
                author_association: snapshot.author_association.clone(),
                labels,
                state: "PENDING".into(),
                eligibility_reasons: Vec::new(),
                reserved_cost_micros: 0,
                intake_request_id: None,
                queue_item_id: None,
                created_at: now.to_rfc3339(),
                updated_at: now.to_rfc3339(),
            });
        }
        let batch_hash = batch_content_hash(&policy, &items, selected, input.expires_at);
        let deadline = DateTime::parse_from_rfc3339(&policy.expires_at)
            .map(|value| value.with_timezone(&Utc))
            .map_err(|_| Error::Conflict("stored issue policy expiry is invalid"))?;
        let batch_expiry = DateTime::parse_from_rfc3339(input.expires_at)
            .map(|value| value.with_timezone(&Utc))
            .map_err(|_| Error::Invalid("issue batch expiry"))?;
        let batch_expiry = batch_expiry.min(deadline);
        let mut delegation = StandingDelegation {
            id: Uuid::new_v4().to_string(),
            principal_id: input.principal_id.into(),
            audit_id: format!("issue-intake-batch:{batch_id}"),
            project_id: input.project_id.into(),
            revision: 1,
            content_hash: String::new(),
            objective_classes: vec![
                crate::lead::ObjectiveClass::Feature,
                crate::lead::ObjectiveClass::Maintenance,
                crate::lead::ObjectiveClass::Refactor,
            ],
            allowed_paths: policy.allowed_paths.clone(),
            capabilities: policy.capabilities.clone(),
            risk_classes: vec![RiskClass::R0],
            autonomous_start: false,
            autonomous_risk_classes: Vec::new(),
            route: crate::lead::ProviderRoute {
                provider: profile.content.roles.implementer.provider.clone(),
                model: profile.content.roles.implementer.model.clone(),
                mode: "build".into(),
            },
            max_attempts: policy.max_attempts,
            max_timeout_secs: u32::try_from(policy.max_wall_clock_secs.min(600)).unwrap_or(600),
            max_repair_cycles: 0,
            max_wall_clock_secs: u32::try_from(policy.max_wall_clock_secs).unwrap_or(u32::MAX),
            max_parallel: 1,
            max_sessions: policy.max_sessions,
            max_requests: selected,
            max_cumulative_sessions: policy
                .max_sessions
                .saturating_mul(selected)
                .min(MAX_CUMULATIVE_SESSIONS),
            max_cumulative_attempts: policy
                .max_attempts
                .saturating_mul(selected)
                .min(MAX_CUMULATIVE_ATTEMPTS),
            expires_at: batch_expiry,
            revoked_at: None,
        };
        delegation.content_hash = delegation.compute_content_hash();
        let delegation_key = format!(
            "issue-batch-delegation:{}",
            sha256_hex(input.idempotency_key.as_bytes())
        );
        let (delegation, _) =
            create_standing_delegation_in_tx(&mut transaction, &delegation, &delegation_key)
                .await?;
        insert_batch_policy_grant(
            &mut transaction,
            &policy,
            &delegation,
            input.principal_id,
            input.idempotency_key,
            &now.to_rfc3339(),
        )
        .await?;
        let batch = IssueIntakeBatchRecord {
            id: batch_id.clone(),
            project_id: input.project_id.into(),
            policy_id: policy.id.clone(),
            policy_revision: policy.revision,
            policy_hash: policy.content_hash.clone(),
            delegation_id: delegation.id.clone(),
            repository_full_name: policy.repository_full_name.clone(),
            repository_github_id,
            connection_revision,
            profile_id: policy.profile_id.clone(),
            profile_revision_id: policy.profile_revision_id.clone(),
            profile_revision: policy.profile_revision,
            profile_hash: policy.profile_hash.clone(),
            project_config_hash: policy.project_config_hash.clone(),
            max_issues: selected,
            max_concurrent: 1,
            max_cost_micros_per_issue: policy.max_cost_micros_per_issue,
            max_total_cost_micros: input.max_total_cost_micros,
            reserved_cost_micros: 0,
            reserved_issues: 0,
            status: "ACTIVE".into(),
            acknowledged: true,
            principal_id: input.principal_id.into(),
            principal_label: input.principal_label.map(str::to_owned),
            reason: input.reason.into(),
            idempotency_key: input.idempotency_key.into(),
            batch_hash,
            expires_at: batch_expiry.to_rfc3339(),
            created_at: now.to_rfc3339(),
            updated_at: now.to_rfc3339(),
            revoked_at: None,
        };
        insert_batch(&mut transaction, &batch).await?;
        for item in &mut items {
            insert_batch_item(&mut transaction, item).await?;
            insert_queue_item_for_batch_item(&mut transaction, &batch, item).await?;
            item.queue_item_id = Some(queue_item_id_for_batch_item(&item.id));
        }
        insert_issue_intake_audit(
            &mut transaction,
            &batch.project_id,
            Some(&batch.policy_id),
            Some(&batch.id),
            None,
            "batch_acknowledged",
            input.principal_label.unwrap_or(input.principal_id),
            input.principal_id,
            input.reason,
            &format!(
                "issues={} max_cost_micros={} delegation={} autonomous_start=false",
                batch.max_issues, batch.max_total_cost_micros, batch.delegation_id
            ),
            &batch.created_at,
        )
        .await?;
        record_event(
            &mut transaction,
            &batch.id,
            "issue_intake.batch_acknowledged",
            &format!(
                "project={} issues={} policy_revision={} autonomous_start=false",
                batch.project_id, batch.max_issues, batch.policy_revision
            ),
        )
        .await?;
        sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
            .bind(input.idempotency_key)
            .bind("create_issue_intake_batch")
            .bind(&digest)
            .bind(&batch.id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(IssueIntakeBatchResult {
            batch,
            items,
            created: true,
        })
    }

    /// Revoke one acknowledged batch. Existing reservations are retained and
    /// never replenished; the linked non-autonomous delegation is revoked.
    pub async fn revoke_issue_intake_batch(
        &self,
        project_id: &str,
        batch_id: &str,
        reason: &str,
        principal_id: &str,
        principal_label: Option<&str>,
    ) -> Result<IssueIntakeBatchRecord, Error> {
        validate_reason(reason)?;
        validate_principal(principal_id, principal_label)?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let batch = load_batch(&mut transaction, project_id, batch_id).await?;
        if batch.status == "REVOKED" {
            transaction.commit().await?;
            return Ok(batch);
        }
        let now = Utc::now().to_rfc3339();
        revoke_batch(&mut transaction, &batch, reason, principal_id, &now).await?;
        record_event(
            &mut transaction,
            &batch.id,
            "issue_intake.batch_revoked",
            &format!("project={} reason={reason}", batch.project_id),
        )
        .await?;
        transaction.commit().await?;
        let mut connection = self.pool.acquire().await?;
        load_batch(&mut connection, project_id, batch_id).await
    }

    pub async fn issue_intake_state(&self, project_id: &str) -> Result<IssueIntakeState, Error> {
        let mut connection = self.pool.acquire().await?;
        ensure_project_exists(&mut connection, project_id).await?;
        let policy = load_optional_policy_head(&mut connection, project_id).await?;
        let history = load_policy_history(&mut connection, project_id).await?;
        let binding = match load_binding(&mut connection, project_id).await {
            Ok(binding) => Some(binding),
            Err(Error::NotFound) => None,
            Err(error) => return Err(error),
        };
        let connection_generation = load_optional_connection_generation(&mut connection).await?;
        let config = load_project_config(&mut connection, project_id).await?;
        let batch_rows = load_batches(&mut connection, project_id).await?;
        let mut batches = Vec::with_capacity(batch_rows.len());
        for batch in batch_rows {
            let items = load_batch_items(&mut connection, project_id, &batch.id).await?;
            let policy_for_batch =
                match load_policy_by_id(&mut connection, project_id, &batch.policy_id).await {
                    Ok(policy) => Some(policy),
                    Err(Error::NotFound) => None,
                    Err(error) => return Err(error),
                };
            let current_head = policy.as_ref().map(|policy| policy.id.as_str());
            let profile = load_exact_intake_profile_in_tx(
                &mut connection,
                &self.provider_model_cache(),
                &batch.profile_id,
                &batch.profile_revision_id,
                batch.profile_revision,
                &batch.profile_hash,
            )
            .await;
            let active_requests = active_project_requests(&mut connection, project_id).await?;
            let mut item_states = Vec::with_capacity(items.len());
            for item in items {
                let eligibility = current_batch_item_eligibility(
                    &mut connection,
                    project_id,
                    &config,
                    binding.as_ref(),
                    connection_generation,
                    policy_for_batch.as_ref(),
                    current_head,
                    &batch,
                    &item,
                    profile.as_ref().ok(),
                    active_requests,
                )
                .await?;
                item_states.push(IssueIntakeItemState {
                    item,
                    current_eligibility: Some(eligibility),
                });
            }
            batches.push(IssueIntakeBatchState {
                remaining_issues: batch.max_issues.saturating_sub(batch.reserved_issues),
                remaining_cost_micros: batch
                    .max_total_cost_micros
                    .saturating_sub(batch.reserved_cost_micros),
                batch,
                items: item_states,
            });
        }
        let queue = load_queue_items(&mut connection, project_id).await?;
        let audit = load_audit(&mut connection, project_id).await?;
        let (connection_enabled, connection_revision) = connection_generation
            .map(|(enabled, revision)| (enabled, Some(revision)))
            .unwrap_or((false, None));
        Ok(IssueIntakeState {
            project_id: project_id.into(),
            enabled: policy.as_ref().is_some_and(|policy| {
                policy.enabled && deadline_in_future(&policy.expires_at, Utc::now())
            }),
            policy,
            policy_history: history,
            batches,
            queue,
            audit,
            binding,
            connection_enabled,
            connection_revision,
            limits: IssueIntakeLimits {
                max_batch_issues: crate::issue_intake::MAX_BATCH_ISSUES,
                max_concurrent: 1,
                max_queue_attempts: crate::issue_intake::MAX_QUEUE_ATTEMPTS,
                queue_lease_seconds: crate::issue_intake::QUEUE_LEASE_SECONDS,
                autonomous_start: false,
                campaign_start: false,
                provider_calls: false,
            },
        })
    }

    /// Lease one due queue item. There is no background scheduler: an
    /// authenticated dispatch performs one fresh GitHub read and one
    /// reconciliation attempt. Expired leases are recovered first.
    pub async fn claim_issue_intake_queue_item(
        &self,
        project_id: &str,
        batch_id: Option<&str>,
        lease_seconds: i64,
    ) -> Result<Option<ClaimedIssueIntakeItem>, Error> {
        if !(5..=300).contains(&lease_seconds) {
            return Err(Error::Invalid("issue intake queue lease"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        ensure_project_exists(&mut transaction, project_id).await?;
        let now = Utc::now();
        let now_text = now.to_rfc3339();
        recover_expired_leases_in_tx(&mut transaction, project_id, &now_text).await?;
        let policy = load_policy_head(&mut transaction, project_id).await?;
        if !policy.enabled {
            return Err(intake_conflict(
                "ISSUE_INTAKE_DISABLED",
                "the Project issue-intake policy is disabled",
            ));
        }
        if !deadline_in_future(&policy.expires_at, now) {
            expire_active_batches_for_project(&mut transaction, project_id, "core", &now_text)
                .await?;
            transaction.commit().await?;
            return Err(intake_conflict(
                "ISSUE_POLICY_EXPIRED",
                "the Project issue-intake policy is expired",
            ));
        }
        let (connection_enabled, connection_revision) =
            load_connection_generation(&mut transaction).await?;
        if !connection_enabled || connection_revision != policy.connection_revision {
            revoke_active_batches_for_project(
                &mut transaction,
                project_id,
                "GITHUB_CONNECTION_CHANGED",
                "core",
                &now_text,
            )
            .await?;
            transaction.commit().await?;
            return Err(intake_conflict(
                "GITHUB_CONNECTION_CHANGED",
                "the GitHub connection was revoked or replaced; acknowledge a new policy and batch",
            ));
        }
        let leased: (i64,) = sqlx::query_as::query_as(
            "SELECT COUNT(*) FROM issue_intake_queue
             WHERE project_id=?1 AND status='LEASED'
               AND (lease_expires_at IS NULL OR lease_expires_at > ?2)",
        )
        .bind(project_id)
        .bind(&now_text)
        .fetch_one(&mut *transaction)
        .await?;
        if leased.0 > 0 {
            transaction.commit().await?;
            return Ok(None);
        }
        let queue_id: Option<(String,)> = match batch_id {
            Some(batch_id) => {
                sqlx::query_as::query_as(
                    "SELECT q.id FROM issue_intake_queue q
                 JOIN issue_intake_batches b ON b.id=q.batch_id
                 WHERE q.project_id=?1 AND q.batch_id=?2 AND q.status='PENDING'
                   AND (q.next_attempt_at IS NULL OR q.next_attempt_at <= ?3)
                   AND b.status='ACTIVE'
                 ORDER BY q.created_at,q.id LIMIT 1",
                )
                .bind(project_id)
                .bind(batch_id)
                .bind(&now_text)
                .fetch_optional(&mut *transaction)
                .await?
            }
            None => {
                sqlx::query_as::query_as(
                    "SELECT q.id FROM issue_intake_queue q
                 JOIN issue_intake_batches b ON b.id=q.batch_id
                 WHERE q.project_id=?1 AND q.status='PENDING'
                   AND (q.next_attempt_at IS NULL OR q.next_attempt_at <= ?2)
                   AND b.status='ACTIVE'
                 ORDER BY q.created_at,q.id LIMIT 1",
                )
                .bind(project_id)
                .bind(&now_text)
                .fetch_optional(&mut *transaction)
                .await?
            }
        };
        let Some((queue_id,)) = queue_id else {
            transaction.commit().await?;
            return Ok(None);
        };
        let queue = load_queue_item(&mut transaction, project_id, &queue_id).await?;
        let batch = load_batch(&mut transaction, project_id, &queue.batch_id).await?;
        if batch.policy_id != policy.id
            || batch.policy_revision != policy.revision
            || batch.policy_hash != policy.content_hash
        {
            mark_queue_terminal_in_tx(
                &mut transaction,
                &queue,
                "REVOKED",
                &["POLICY_STALE".to_string()],
                "the batch policy is no longer current",
                &now_text,
            )
            .await?;
            transaction.commit().await?;
            return Err(intake_conflict(
                "ISSUE_POLICY_STALE",
                "the batch policy is no longer current; acknowledge a new batch",
            ));
        }
        if batch.status != "ACTIVE" || !deadline_in_future(&batch.expires_at, now) {
            if batch.status == "ACTIVE" {
                expire_batch(&mut transaction, &batch, "core", &now_text).await?;
            }
            transaction.commit().await?;
            return Err(intake_conflict(
                "ISSUE_BATCH_EXPIRED",
                "the issue batch is no longer active",
            ));
        }
        let binding = load_binding(&mut transaction, project_id).await?;
        if binding.repo_full_name != batch.repository_full_name
            || binding.repository_github_id != Some(batch.repository_github_id)
        {
            revoke_batch(
                &mut transaction,
                &batch,
                "repository binding changed",
                "core",
                &now_text,
            )
            .await?;
            transaction.commit().await?;
            return Err(intake_conflict(
                "ISSUE_POLICY_REPOSITORY_MISMATCH",
                "the Project repository binding changed after the batch was acknowledged",
            ));
        }
        let item = load_batch_item(&mut transaction, project_id, &queue.batch_item_id).await?;
        let lease_token = Uuid::new_v4().to_string();
        let lease_expires = (now + Duration::seconds(lease_seconds)).to_rfc3339();
        let updated = sqlx::query::query(
            "UPDATE issue_intake_queue
             SET status='LEASED',lease_token=?1,lease_expires_at=?2,
                 attempts=attempts+1,reason='leased for explicit reconciliation',
                 updated_at=?3
             WHERE project_id=?4 AND id=?5 AND status='PENDING'",
        )
        .bind(&lease_token)
        .bind(&lease_expires)
        .bind(&now_text)
        .bind(project_id)
        .bind(&queue.id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if updated != 1 {
            transaction.commit().await?;
            return Ok(None);
        }
        let queue = load_queue_item(&mut transaction, project_id, &queue.id).await?;
        transaction.commit().await?;
        Ok(Some(ClaimedIssueIntakeItem {
            queue,
            batch,
            policy,
            item,
            binding,
            lease_token,
        }))
    }

    /// Reconcile one leased queue item into an ordinary REVIEW_PLAN intake
    /// proposal. The observation must be a direct GitHub read made immediately
    /// before this call. No assessment, provider call or Campaign start occurs.
    pub async fn reconcile_issue_intake_queue_item(
        &self,
        project_id: &str,
        queue_id: &str,
        lease_token: &str,
        observed: &IssueSnapshot,
        credential_revision: u32,
    ) -> Result<IssueIntakeReconcileResult, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let queue = load_queue_item(&mut transaction, project_id, queue_id).await?;
        if let Some(intake_request_id) = &queue.intake_request_id {
            let intake = load_intake_request(&mut transaction, intake_request_id).await?;
            transaction.commit().await?;
            return Ok(IssueIntakeReconcileResult {
                status: "RECONCILED".into(),
                reasons: vec!["EXACT_REPLAY".into()],
                queue,
                intake: Some(intake),
            });
        }
        if queue.status != "LEASED" || queue.lease_token.as_deref() != Some(lease_token) {
            return Err(intake_conflict(
                "ISSUE_QUEUE_LEASE_MISMATCH",
                "the queue item is not leased by this dispatch",
            ));
        }
        let now = Utc::now();
        if queue
            .lease_expires_at
            .as_deref()
            .is_none_or(|value| !deadline_in_future(value, now))
        {
            release_lease_in_tx(
                &mut transaction,
                &queue,
                "ISSUE_QUEUE_LEASE_EXPIRED",
                &now.to_rfc3339(),
            )
            .await?;
            transaction.commit().await?;
            return Err(intake_conflict(
                "ISSUE_QUEUE_LEASE_EXPIRED",
                "the queue lease expired; dispatch again",
            ));
        }
        let batch = load_batch(&mut transaction, project_id, &queue.batch_id).await?;
        let item = load_batch_item(&mut transaction, project_id, &queue.batch_item_id).await?;
        let policy = load_policy_head(&mut transaction, project_id).await?;
        let (connection_enabled, connection_revision) =
            load_connection_generation(&mut transaction).await?;
        if !connection_enabled
            || connection_revision != credential_revision
            || connection_revision != policy.connection_revision
            || connection_revision != batch.connection_revision
        {
            revoke_batch(
                &mut transaction,
                &batch,
                "GitHub credential changed during reconciliation",
                "core",
                &now.to_rfc3339(),
            )
            .await?;
            transaction.commit().await?;
            return Err(intake_conflict(
                "GITHUB_CONNECTION_CHANGED",
                "the GitHub credential changed; the batch is revoked",
            ));
        }
        if batch.policy_id != policy.id
            || batch.policy_revision != policy.revision
            || batch.policy_hash != policy.content_hash
            || !policy.enabled
        {
            mark_queue_terminal_in_tx(
                &mut transaction,
                &queue,
                "REVOKED",
                &["POLICY_STALE".to_string()],
                "the batch policy is no longer current",
                &now.to_rfc3339(),
            )
            .await?;
            transaction.commit().await?;
            return Ok(IssueIntakeReconcileResult {
                status: "REVOKED".into(),
                reasons: vec!["POLICY_STALE".into()],
                queue: load_queue_item_pool(&self.pool, project_id, queue_id).await?,
                intake: None,
            });
        }
        if batch.status != "ACTIVE" || !deadline_in_future(&batch.expires_at, now) {
            if batch.status == "ACTIVE" {
                expire_batch(&mut transaction, &batch, "core", &now.to_rfc3339()).await?;
            }
            transaction.commit().await?;
            return Ok(IssueIntakeReconcileResult {
                status: "EXPIRED".into(),
                reasons: vec!["BATCH_EXPIRED".into()],
                queue: load_queue_item_pool(&self.pool, project_id, queue_id).await?,
                intake: None,
            });
        }
        let binding = load_binding(&mut transaction, project_id).await?;
        let config = load_project_config(&mut transaction, project_id).await?;
        let profile = load_exact_intake_profile_in_tx(
            &mut transaction,
            &self.provider_model_cache(),
            &batch.profile_id,
            &batch.profile_revision_id,
            batch.profile_revision,
            &batch.profile_hash,
        )
        .await;
        let Ok(profile) = profile else {
            mark_queue_terminal_in_tx(
                &mut transaction,
                &queue,
                "BLOCKED",
                &["PROFILE_NOT_EXECUTABLE".to_string()],
                "the acknowledged execution profile is no longer executable",
                &now.to_rfc3339(),
            )
            .await?;
            transaction.commit().await?;
            return Ok(IssueIntakeReconcileResult {
                status: "BLOCKED".into(),
                reasons: vec!["PROFILE_NOT_EXECUTABLE".into()],
                queue: load_queue_item_pool(&self.pool, project_id, queue_id).await?,
                intake: None,
            });
        };
        let snapshot = load_issue_snapshot(
            &mut transaction,
            project_id,
            &batch.repository_full_name,
            item.issue_number,
        )
        .await?;
        if snapshot.as_ref().is_none_or(|snapshot| {
            snapshot.content_hash != item.content_hash
                || snapshot.github_id != item.github_issue_id
                || snapshot.updated_at != item.issue_updated_at
        }) || observed.content_hash != item.content_hash
            || observed.github_id != item.github_issue_id
            || observed.number != item.issue_number
            || observed.updated_at != item.issue_updated_at
            || observed.project_id != project_id
            || observed.repository_full_name != batch.repository_full_name
            || observed.repository_github_id != Some(batch.repository_github_id)
        {
            mark_queue_terminal_in_tx(
                &mut transaction,
                &queue,
                "STALE",
                &["SNAPSHOT_STALE".to_string()],
                "the issue changed after acknowledgement; a new batch is required",
                &now.to_rfc3339(),
            )
            .await?;
            sqlx::query::query(
                "UPDATE issue_intake_batch_items
                 SET state='STALE',eligibility_reasons_json=?1,updated_at=?2
                 WHERE project_id=?3 AND id=?4 AND state='PENDING'",
            )
            .bind(json(&vec!["SNAPSHOT_STALE".to_string()])?)
            .bind(now.to_rfc3339())
            .bind(project_id)
            .bind(&item.id)
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
            return Ok(IssueIntakeReconcileResult {
                status: "STALE".into(),
                reasons: vec!["SNAPSHOT_STALE".into()],
                queue: load_queue_item_pool(&self.pool, project_id, queue_id).await?,
                intake: None,
            });
        }
        let active_requests = active_project_requests(&mut transaction, project_id).await?;
        let eligibility = current_batch_item_eligibility(
            &mut transaction,
            project_id,
            &config,
            Some(&binding),
            Some((connection_enabled, connection_revision)),
            Some(&policy),
            Some(&policy.id),
            &batch,
            &item,
            Some(&profile),
            active_requests,
        )
        .await?;
        if !eligibility.eligible {
            let stale = eligibility
                .reasons
                .iter()
                .any(|reason| reason == "SNAPSHOT_STALE");
            let status = if stale { "STALE" } else { "INELIGIBLE" };
            mark_queue_terminal_in_tx(
                &mut transaction,
                &queue,
                status,
                &eligibility.reasons,
                "deterministic issue-intake eligibility rejected the snapshot",
                &now.to_rfc3339(),
            )
            .await?;
            sqlx::query::query(
                "UPDATE issue_intake_batch_items
                 SET state=?1,eligibility_reasons_json=?2,updated_at=?3
                 WHERE project_id=?4 AND id=?5 AND state='PENDING'",
            )
            .bind(status)
            .bind(json(&eligibility.reasons)?)
            .bind(now.to_rfc3339())
            .bind(project_id)
            .bind(&item.id)
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
            return Ok(IssueIntakeReconcileResult {
                status: status.into(),
                reasons: eligibility.reasons,
                queue: load_queue_item_pool(&self.pool, project_id, queue_id).await?,
                intake: None,
            });
        }
        let delegation = load_delegation(&mut transaction, &batch.delegation_id).await?;
        if delegation.revoked_at.is_some()
            || delegation.expires_at <= now
            || delegation.autonomous_start
            || !delegation.autonomous_risk_classes.is_empty()
        {
            mark_queue_terminal_in_tx(
                &mut transaction,
                &queue,
                "BLOCKED",
                &["DELEGATION_NOT_ACTIVE".to_string()],
                "the non-autonomous batch delegation is not active",
                &now.to_rfc3339(),
            )
            .await?;
            transaction.commit().await?;
            return Ok(IssueIntakeReconcileResult {
                status: "BLOCKED".into(),
                reasons: vec!["DELEGATION_NOT_ACTIVE".into()],
                queue: load_queue_item_pool(&self.pool, project_id, queue_id).await?,
                intake: None,
            });
        }
        let objective = issue_objective(observed, &item.content_hash);
        let prepared = prepare_intake_request(
            project_id,
            &objective,
            Some(&batch.profile_id),
            IntakeMode::ReviewPlan,
            &batch.principal_id,
        )?;
        let idempotency_key = format!("issue-intake-proposal:{queue_id}");
        if let Some(intake_id) = replay(
            &mut transaction,
            &idempotency_key,
            "create_intake_request",
            &prepared.submission_hash,
        )
        .await?
        {
            let intake = load_intake_request(&mut transaction, &intake_id).await?;
            link_reconciled_queue(
                &mut transaction,
                project_id,
                &queue,
                &item,
                &intake,
                batch.max_cost_micros_per_issue,
                &now.to_rfc3339(),
            )
            .await?;
            transaction.commit().await?;
            return Ok(IssueIntakeReconcileResult {
                status: "RECONCILED".into(),
                reasons: vec!["EXACT_REPLAY".into()],
                queue: load_queue_item_pool(&self.pool, project_id, queue_id).await?,
                intake: Some(intake),
            });
        }
        let reserved = sqlx::query::query(
            "UPDATE issue_intake_batches
             SET reserved_issues=reserved_issues+1,
                 reserved_cost_micros=reserved_cost_micros+max_cost_micros_per_issue,
                 updated_at=?1
             WHERE id=?2 AND status='ACTIVE'
               AND reserved_issues < max_issues
               AND reserved_cost_micros + max_cost_micros_per_issue <= max_total_cost_micros",
        )
        .bind(now.to_rfc3339())
        .bind(&batch.id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if reserved != 1 {
            mark_queue_terminal_in_tx(
                &mut transaction,
                &queue,
                "BLOCKED",
                &["BUDGET_EXHAUSTED".to_string()],
                "the batch reservation capacity is exhausted",
                &now.to_rfc3339(),
            )
            .await?;
            transaction.commit().await?;
            return Ok(IssueIntakeReconcileResult {
                status: "BLOCKED".into(),
                reasons: vec!["BUDGET_EXHAUSTED".into()],
                queue: load_queue_item_pool(&self.pool, project_id, queue_id).await?,
                intake: None,
            });
        }
        sqlx::query::query(
            "UPDATE issue_intake_batch_items
             SET reserved_cost_micros=?1,updated_at=?2
             WHERE project_id=?3 AND id=?4",
        )
        .bind(batch.max_cost_micros_per_issue as i64)
        .bind(now.to_rfc3339())
        .bind(project_id)
        .bind(&item.id)
        .execute(&mut *transaction)
        .await?;
        let intake = create_intake_request_in_tx(
            &mut transaction,
            project_id,
            &objective,
            IntakeMode::ReviewPlan,
            &idempotency_key,
            profile,
            &config,
            &delegation,
            prepared,
        )
        .await?;
        link_reconciled_queue(
            &mut transaction,
            project_id,
            &queue,
            &item,
            &intake,
            batch.max_cost_micros_per_issue,
            &now.to_rfc3339(),
        )
        .await?;
        let actor = batch
            .principal_label
            .clone()
            .unwrap_or(batch.principal_id.clone());
        insert_issue_intake_audit(
            &mut transaction,
            project_id,
            Some(&policy.id),
            Some(&batch.id),
            Some(&queue.id),
            "queue_reconciled",
            &actor,
            &batch.principal_id,
            "explicit reconciliation",
            &format!(
                "issue={} intake={} lead={} campaign_start=false provider_calls=false",
                item.issue_number, intake.id, intake.lead_request_id
            ),
            &now.to_rfc3339(),
        )
        .await?;
        record_event(
            &mut transaction,
            &queue.id,
            "issue_intake.queue_reconciled",
            &format!(
                "project={project_id} issue={} intake={} campaign_start=false",
                item.issue_number, intake.id
            ),
        )
        .await?;
        transaction.commit().await?;
        Ok(IssueIntakeReconcileResult {
            status: "RECONCILED".into(),
            reasons: Vec::new(),
            queue: load_queue_item_pool(&self.pool, project_id, queue_id).await?,
            intake: Some(intake),
        })
    }

    /// Release a failed direct read. Transient failures retry within the fixed
    /// three-attempt bound; authentication, permission, identity and freshness
    /// failures are terminal and require operator intervention.
    pub async fn fail_issue_intake_queue_item(
        &self,
        project_id: &str,
        queue_id: &str,
        lease_token: &str,
        kind: IssueIntakeQueueFailure,
        reason: &str,
    ) -> Result<IssueIntakeQueueItemRecord, Error> {
        let reason = reason.chars().take(MAX_REASON_CHARS).collect::<String>();
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let queue = load_queue_item(&mut transaction, project_id, queue_id).await?;
        if queue.intake_request_id.is_some() {
            transaction.commit().await?;
            return Ok(queue);
        }
        if queue.status != "LEASED" || queue.lease_token.as_deref() != Some(lease_token) {
            transaction.commit().await?;
            return Ok(queue);
        }
        let now = Utc::now();
        let now_text = now.to_rfc3339();
        let retry =
            kind == IssueIntakeQueueFailure::Transient && queue.attempts < queue.max_attempts;
        if retry {
            let backoff = match queue.attempts {
                0 | 1 => 30,
                _ => 120,
            };
            let next = (now + Duration::seconds(backoff)).to_rfc3339();
            sqlx::query::query(
                "UPDATE issue_intake_queue
                 SET status='PENDING',lease_token=NULL,lease_expires_at=NULL,
                     next_attempt_at=?1,reason=?2,updated_at=?3
                 WHERE project_id=?4 AND id=?5",
            )
            .bind(&next)
            .bind(&reason)
            .bind(&now_text)
            .bind(project_id)
            .bind(&queue.id)
            .execute(&mut *transaction)
            .await?;
            insert_issue_intake_audit(
                &mut transaction,
                project_id,
                None,
                Some(&queue.batch_id),
                Some(&queue.id),
                "queue_retry",
                "core",
                "core",
                &reason,
                &format!("attempt={} next={next}", queue.attempts),
                &now_text,
            )
            .await?;
            record_event(
                &mut transaction,
                &queue.id,
                "issue_intake.queue_retry",
                &format!("project={project_id} attempt={}", queue.attempts),
            )
            .await?;
        } else {
            let reasons = vec![reason.clone()];
            mark_queue_terminal_in_tx(
                &mut transaction,
                &queue,
                "BLOCKED",
                &reasons,
                &reason,
                &now_text,
            )
            .await?;
            sqlx::query::query(
                "UPDATE issue_intake_batch_items
                 SET state='INELIGIBLE',eligibility_reasons_json=?1,updated_at=?2
                 WHERE project_id=?3 AND id=?4 AND state='PENDING'",
            )
            .bind(json(&reasons)?)
            .bind(&now_text)
            .bind(project_id)
            .bind(&queue.batch_item_id)
            .execute(&mut *transaction)
            .await?;
            insert_issue_intake_audit(
                &mut transaction,
                project_id,
                None,
                Some(&queue.batch_id),
                Some(&queue.id),
                "queue_blocked",
                "core",
                "core",
                &reason,
                "direct freshness read failed",
                &now_text,
            )
            .await?;
            record_event(
                &mut transaction,
                &queue.id,
                "issue_intake.queue_blocked",
                &format!("project={project_id} reason={reason}"),
            )
            .await?;
        }
        let updated = load_queue_item(&mut transaction, project_id, &queue.id).await?;
        transaction.commit().await?;
        Ok(updated)
    }
}

async fn load_queue_item(
    connection: &mut SqliteConnection,
    project_id: &str,
    queue_id: &str,
) -> Result<IssueIntakeQueueItemRecord, Error> {
    let row = sqlx::query::query("SELECT * FROM issue_intake_queue WHERE project_id=?1 AND id=?2")
        .bind(project_id)
        .bind(queue_id)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(Error::NotFound)?;
    queue_item_from_row(&row)
}

async fn load_queue_item_pool(
    pool: &SqlitePool,
    project_id: &str,
    queue_id: &str,
) -> Result<IssueIntakeQueueItemRecord, Error> {
    let mut connection = pool.acquire().await?;
    load_queue_item(&mut connection, project_id, queue_id).await
}

async fn load_batch_item(
    connection: &mut SqliteConnection,
    project_id: &str,
    item_id: &str,
) -> Result<IssueIntakeBatchItemRecord, Error> {
    let row =
        sqlx::query::query("SELECT * FROM issue_intake_batch_items WHERE project_id=?1 AND id=?2")
            .bind(project_id)
            .bind(item_id)
            .fetch_optional(&mut *connection)
            .await?
            .ok_or(Error::NotFound)?;
    batch_item_from_row(&row)
}

async fn recover_expired_leases_in_tx(
    connection: &mut SqliteConnection,
    project_id: &str,
    now: &str,
) -> Result<(), Error> {
    let rows = sqlx::query::query(
        "SELECT id,batch_id,batch_item_id,intake_request_id,attempts,max_attempts
         FROM issue_intake_queue
         WHERE project_id=?1 AND status='LEASED'
           AND lease_expires_at IS NOT NULL AND lease_expires_at <= ?2",
    )
    .bind(project_id)
    .bind(now)
    .fetch_all(&mut *connection)
    .await?;
    for row in rows {
        let id: String = row.try_get("id")?;
        let batch_id: String = row.try_get("batch_id")?;
        let batch_item_id: String = row.try_get("batch_item_id")?;
        let intake_request_id: Option<String> = row.try_get("intake_request_id")?;
        let attempts: i64 = row.try_get("attempts")?;
        let max_attempts: i64 = row.try_get("max_attempts")?;
        let (status, reason) = if intake_request_id.is_some() {
            ("RECONCILED", "committed proposal link recovered")
        } else if attempts >= max_attempts {
            ("BLOCKED", "lease expired with no remaining retry budget")
        } else {
            ("PENDING", "lease expired; retry is explicit")
        };
        sqlx::query::query(
            "UPDATE issue_intake_queue
             SET status=?1,lease_token=NULL,lease_expires_at=NULL,reason=?2,updated_at=?3
             WHERE id=?4 AND status='LEASED'",
        )
        .bind(status)
        .bind(reason)
        .bind(now)
        .bind(&id)
        .execute(&mut *connection)
        .await?;
        if intake_request_id.is_some() {
            sqlx::query::query(
                "UPDATE issue_intake_batch_items SET state='RECONCILED',updated_at=?1
                 WHERE id=?2 AND state='PENDING'",
            )
            .bind(now)
            .bind(&batch_item_id)
            .execute(&mut *connection)
            .await?;
        }
        insert_issue_intake_audit(
            connection,
            project_id,
            None,
            Some(&batch_id),
            Some(&id),
            "queue_recovered",
            "core",
            "core",
            "lease expiry recovery",
            &format!("status={status}"),
            now,
        )
        .await?;
    }
    Ok(())
}

async fn release_lease_in_tx(
    connection: &mut SqliteConnection,
    queue: &IssueIntakeQueueItemRecord,
    reason: &str,
    now: &str,
) -> Result<(), Error> {
    sqlx::query::query(
        "UPDATE issue_intake_queue
         SET status='PENDING',lease_token=NULL,lease_expires_at=NULL,reason=?1,updated_at=?2
         WHERE id=?3 AND status='LEASED'",
    )
    .bind(reason)
    .bind(now)
    .bind(&queue.id)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn mark_queue_terminal_in_tx(
    connection: &mut SqliteConnection,
    queue: &IssueIntakeQueueItemRecord,
    status: &str,
    reasons: &[String],
    reason: &str,
    now: &str,
) -> Result<(), Error> {
    sqlx::query::query(
        "UPDATE issue_intake_queue
         SET status=?1,lease_token=NULL,lease_expires_at=NULL,reason=?2,reasons_json=?3,
             updated_at=?4,completed_at=?4
         WHERE id=?5",
    )
    .bind(status)
    .bind(reason)
    .bind(json(&reasons.to_vec())?)
    .bind(now)
    .bind(&queue.id)
    .execute(&mut *connection)
    .await?;
    let item_state = match status {
        "STALE" => "STALE",
        "REVOKED" => "REVOKED",
        _ => "INELIGIBLE",
    };
    sqlx::query::query(
        "UPDATE issue_intake_batch_items
         SET state=?1,eligibility_reasons_json=?2,updated_at=?3
         WHERE project_id=?4 AND id=?5 AND state='PENDING'",
    )
    .bind(item_state)
    .bind(json(&reasons.to_vec())?)
    .bind(now)
    .bind(&queue.project_id)
    .bind(&queue.batch_item_id)
    .execute(&mut *connection)
    .await?;
    insert_issue_intake_audit(
        connection,
        &queue.project_id,
        None,
        Some(&queue.batch_id),
        Some(&queue.id),
        "queue_blocked",
        "core",
        "core",
        reason,
        &format!("status={status} reasons={}", reasons.join(",")),
        now,
    )
    .await?;
    Ok(())
}

async fn link_reconciled_queue(
    connection: &mut SqliteConnection,
    project_id: &str,
    queue: &IssueIntakeQueueItemRecord,
    item: &IssueIntakeBatchItemRecord,
    intake: &IntakeRequestRecord,
    reserved_cost_micros: u64,
    now: &str,
) -> Result<(), Error> {
    sqlx::query::query(
        "UPDATE issue_intake_queue
         SET status='RECONCILED',lease_token=NULL,lease_expires_at=NULL,reason=NULL,
             reasons_json='[]',intake_request_id=?1,lead_request_id=?2,action_id=?3,
             updated_at=?4,completed_at=?4
         WHERE project_id=?5 AND id=?6",
    )
    .bind(&intake.id)
    .bind(&intake.lead_request_id)
    .bind(&intake.action_id)
    .bind(now)
    .bind(project_id)
    .bind(&queue.id)
    .execute(&mut *connection)
    .await?;
    sqlx::query::query(
        "UPDATE issue_intake_batch_items
         SET state='RECONCILED',intake_request_id=?1,
             reserved_cost_micros=CASE WHEN reserved_cost_micros=0 THEN ?2
                                      ELSE reserved_cost_micros END,
             eligibility_reasons_json='[]',updated_at=?3
         WHERE project_id=?4 AND id=?5",
    )
    .bind(&intake.id)
    .bind(reserved_cost_micros as i64)
    .bind(now)
    .bind(project_id)
    .bind(&item.id)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn expire_batch(
    connection: &mut SqliteConnection,
    batch: &IssueIntakeBatchRecord,
    actor: &str,
    now: &str,
) -> Result<(), Error> {
    sqlx::query::query(
        "UPDATE issue_intake_batches
         SET status='EXPIRED',revoked_at=?1,updated_at=?1 WHERE id=?2 AND status='ACTIVE'",
    )
    .bind(now)
    .bind(&batch.id)
    .execute(&mut *connection)
    .await?;
    sqlx::query::query(
        "UPDATE issue_intake_batch_items
         SET state='INELIGIBLE',eligibility_reasons_json=?1,updated_at=?2
         WHERE batch_id=?3 AND state='PENDING'",
    )
    .bind(json(&vec!["BATCH_EXPIRED".to_string()])?)
    .bind(now)
    .bind(&batch.id)
    .execute(&mut *connection)
    .await?;
    sqlx::query::query(
        "UPDATE issue_intake_queue
         SET status='BLOCKED',lease_token=NULL,lease_expires_at=NULL,
             reason='batch expired',reasons_json=?1,updated_at=?2,completed_at=?2
         WHERE batch_id=?3 AND status IN ('PENDING','LEASED')",
    )
    .bind(json(&vec!["BATCH_EXPIRED".to_string()])?)
    .bind(now)
    .bind(&batch.id)
    .execute(&mut *connection)
    .await?;
    sqlx::query::query(
        "UPDATE standing_delegations SET revoked_at=?1 WHERE id=?2 AND revoked_at IS NULL",
    )
    .bind(now)
    .bind(&batch.delegation_id)
    .execute(&mut *connection)
    .await?;
    insert_issue_intake_audit(
        connection,
        &batch.project_id,
        Some(&batch.policy_id),
        Some(&batch.id),
        None,
        "batch_expired",
        actor,
        actor,
        "batch reached its acknowledged expiry",
        "autonomous_start=false",
        now,
    )
    .await?;
    Ok(())
}

async fn expire_active_batches_for_project(
    connection: &mut SqliteConnection,
    project_id: &str,
    actor: &str,
    now: &str,
) -> Result<(), Error> {
    let batches = load_batches(connection, project_id).await?;
    for batch in batches {
        if batch.status == "ACTIVE" {
            expire_batch(connection, &batch, actor, now).await?;
        }
    }
    Ok(())
}

fn validate_new_policy(input: &NewIssueIntakePolicy<'_>) -> Result<(), Error> {
    if input.project_id.trim().is_empty() || input.project_id.len() > 128 {
        return Err(Error::Invalid("issue intake project"));
    }
    let Some((owner, repo)) = input.repository_full_name.split_once('/') else {
        return Err(Error::Invalid("issue intake repository"));
    };
    if repo.contains('/') {
        return Err(Error::Invalid("issue intake repository"));
    }
    crate::github::validate_repo_coordinates(owner, repo)
        .map_err(|_| Error::Invalid("issue intake repository"))?;
    if input.repository_github_id <= 0
        || input.profile_id.trim().is_empty()
        || input.profile_id.len() > 128
        || input.profile_revision_id.trim().is_empty()
        || input.profile_revision == 0
        || !is_sha256(input.profile_hash)
        || !is_sha256(input.project_config_hash)
        || !input.acknowledge
    {
        return Err(intake_conflict(
            "ISSUE_POLICY_ACKNOWLEDGEMENT_REQUIRED",
            "explicit acknowledgement and exact profile/repository identity are required",
        ));
    }
    validate_reason(input.reason)?;
    validate_key(input.idempotency_key)?;
    validate_principal(input.principal_id, input.principal_label)?;
    if input.required_labels.len() > MAX_LABELS
        || input.excluded_labels.len() > MAX_LABELS
        || input.author_ids.len() > MAX_AUTHOR_IDS
        || input.allowed_paths.len() > MAX_PATHS
    {
        return Err(Error::Invalid("issue intake policy bounds"));
    }
    Ok(())
}

fn policy_request_digest(
    input: &NewIssueIntakePolicy<'_>,
    required_labels: &[String],
    excluded_labels: &[String],
    author_ids: &[i64],
    author_associations: &[String],
    allowed_paths: &[String],
    capabilities: &[Capability],
) -> String {
    let payload = serde_json::json!({
        "schema": "issue-intake-policy-request-v1",
        "projectId": input.project_id,
        "repositoryFullName": input.repository_full_name,
        "repositoryGithubId": input.repository_github_id,
        "requiredLabels": required_labels,
        "excludedLabels": excluded_labels,
        "authorIds": author_ids,
        "authorAssociations": author_associations,
        "profileId": input.profile_id,
        "profileRevisionId": input.profile_revision_id,
        "profileRevision": input.profile_revision,
        "profileHash": input.profile_hash,
        "projectConfigHash": input.project_config_hash,
        "allowedPaths": allowed_paths,
        "capabilities": capabilities,
        "maxIssues": input.max_issues,
        "maxConcurrent": input.max_concurrent,
        "maxCostMicrosPerIssue": input.max_cost_micros_per_issue,
        "maxTotalCostMicros": input.max_total_cost_micros,
        "maxAttempts": input.max_attempts,
        "maxSessions": input.max_sessions,
        "maxTurns": input.max_turns,
        "maxWallClockSecs": input.max_wall_clock_secs,
        "expiresAt": input.expires_at,
        "acknowledge": input.acknowledge,
        "principalId": input.principal_id,
        "reason": input.reason,
    });
    sha256_hex(payload.to_string().as_bytes())
}

fn validate_batch_input(input: &NewIssueIntakeBatch<'_>) -> Result<(), Error> {
    if input.project_id.trim().is_empty()
        || input.policy_id.trim().is_empty()
        || input.expected_policy_revision == 0
        || !is_sha256(input.expected_policy_hash)
        || input.items.is_empty()
        || !input.acknowledge
    {
        return Err(intake_conflict(
            "ISSUE_BATCH_ACKNOWLEDGEMENT_REQUIRED",
            "an exact policy revision and explicit batch acknowledgement are required",
        ));
    }
    validate_reason(input.reason)?;
    validate_key(input.idempotency_key)?;
    validate_principal(input.principal_id, input.principal_label)?;
    for item in input.items {
        if item.number == 0 || item.github_issue_id <= 0 || !is_sha256(item.content_hash) {
            return Err(Error::Invalid("issue batch selection"));
        }
        if DateTime::parse_from_rfc3339(item.updated_at).is_err() {
            return Err(Error::Invalid("issue batch selection timestamp"));
        }
    }
    Ok(())
}

fn batch_request_digest(input: &NewIssueIntakeBatch<'_>) -> String {
    let items = input
        .items
        .iter()
        .map(|item| {
            (
                item.number,
                item.github_issue_id,
                item.content_hash,
                item.updated_at,
            )
        })
        .collect::<Vec<_>>();
    sha256_hex(
        serde_json::to_string(&(
            "issue-intake-batch-v1",
            input.project_id,
            input.policy_id,
            input.expected_policy_revision,
            input.expected_policy_hash,
            items,
            input.max_total_cost_micros,
            input.expires_at,
            input.acknowledge,
            input.principal_id,
            input.reason,
        ))
        .expect("issue batch digest serializes")
        .as_bytes(),
    )
}

fn validate_reason(reason: &str) -> Result<(), Error> {
    if reason.trim().is_empty()
        || reason.trim() != reason
        || reason.chars().count() > MAX_REASON_CHARS
    {
        return Err(Error::Invalid("issue intake reason"));
    }
    Ok(())
}

fn validate_key(key: &str) -> Result<(), Error> {
    if key.trim().is_empty() || key.len() > 256 {
        return Err(Error::Invalid("issue intake idempotency key"));
    }
    Ok(())
}

fn validate_principal(principal_id: &str, principal_label: Option<&str>) -> Result<(), Error> {
    if principal_id.trim().is_empty()
        || principal_id.len() > 200
        || principal_label.is_some_and(|label| label.trim().is_empty() || label.len() > 200)
    {
        return Err(Error::Invalid("issue intake principal"));
    }
    Ok(())
}

fn normalize_labels(labels: &[String]) -> Vec<String> {
    let mut labels = labels
        .iter()
        .map(|label| label.trim().to_owned())
        .filter(|label| !label.is_empty())
        .collect::<Vec<_>>();
    labels.sort();
    labels.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    labels
}

fn normalize_paths(paths: &[String]) -> Vec<String> {
    let mut paths = paths
        .iter()
        .map(|path| {
            path.trim()
                .trim_start_matches("./")
                .trim_end_matches('/')
                .to_owned()
        })
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

fn intake_conflict(code: &'static str, message: &str) -> Error {
    Error::IntakeConflict {
        code,
        message: message.into(),
    }
}

fn deadline_in_future(value: &str, now: DateTime<Utc>) -> bool {
    DateTime::parse_from_rfc3339(value)
        .map(|deadline| deadline.with_timezone(&Utc) > now)
        .unwrap_or(false)
}

fn deadline_at_or_before(value: &str, ceiling: &str) -> bool {
    match (
        DateTime::parse_from_rfc3339(value),
        DateTime::parse_from_rfc3339(ceiling),
    ) {
        (Ok(value), Ok(ceiling)) => value <= ceiling,
        _ => false,
    }
}

async fn ensure_project_exists(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> Result<(), Error> {
    let exists: Option<(i64,)> = sqlx::query_as::query_as("SELECT 1 FROM projects WHERE id=?1")
        .bind(project_id)
        .fetch_optional(&mut *connection)
        .await?;
    exists.map(|_| ()).ok_or(Error::NotFound)
}

async fn load_binding(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> Result<ProjectGitHubBinding, Error> {
    let row = sqlx::query::query(
        "SELECT project_id,owner,repo,repo_full_name,default_branch,repository_github_id,
                bound_at,updated_at
         FROM project_github_bindings WHERE project_id=?1",
    )
    .bind(project_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(Error::NotFound)?;
    Ok(ProjectGitHubBinding {
        project_id: row.try_get("project_id")?,
        owner: row.try_get("owner")?,
        repo: row.try_get("repo")?,
        repo_full_name: row.try_get("repo_full_name")?,
        default_branch: row.try_get("default_branch")?,
        repository_github_id: row.try_get("repository_github_id")?,
        bound_at: row.try_get("bound_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

async fn load_connection_generation(
    connection: &mut SqliteConnection,
) -> Result<(bool, u32), Error> {
    let row: Option<(i64, i64, String)> = sqlx::query_as::query_as(
        "SELECT enabled,revision,credential_ciphertext FROM github_connections WHERE id='default'",
    )
    .fetch_optional(&mut *connection)
    .await?;
    let Some((enabled, revision, ciphertext)) = row else {
        return Err(intake_conflict(
            "GITHUB_NOT_CONNECTED",
            "GitHub is not connected",
        ));
    };
    let revision = u32::try_from(revision)
        .map_err(|_| Error::Conflict("stored GitHub connection revision is invalid"))?;
    Ok((enabled != 0 && !ciphertext.is_empty(), revision))
}

async fn load_optional_connection_generation(
    connection: &mut SqliteConnection,
) -> Result<Option<(bool, u32)>, Error> {
    let row: Option<(i64, i64, String)> = sqlx::query_as::query_as(
        "SELECT enabled,revision,credential_ciphertext FROM github_connections WHERE id='default'",
    )
    .fetch_optional(&mut *connection)
    .await?;
    let Some((enabled, revision, ciphertext)) = row else {
        return Ok(None);
    };
    let revision = u32::try_from(revision)
        .map_err(|_| Error::Conflict("stored GitHub connection revision is invalid"))?;
    Ok(Some((enabled != 0 && !ciphertext.is_empty(), revision)))
}

async fn load_policy_head(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> Result<IssueIntakePolicyRecord, Error> {
    let row = sqlx::query::query(
        "SELECT r.* FROM issue_intake_policy_heads h
         JOIN issue_intake_policy_revisions r ON r.id=h.policy_id
         WHERE h.project_id=?1",
    )
    .bind(project_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or_else(|| intake_conflict("ISSUE_INTAKE_DISABLED", "issue intake is not configured"))?;
    policy_from_row(&row)
}

async fn load_optional_policy_head(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> Result<Option<IssueIntakePolicyRecord>, Error> {
    match load_policy_head(connection, project_id).await {
        Ok(policy) => Ok(Some(policy)),
        Err(Error::IntakeConflict { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

async fn load_policy_by_id(
    connection: &mut SqliteConnection,
    project_id: &str,
    policy_id: &str,
) -> Result<IssueIntakePolicyRecord, Error> {
    let row = sqlx::query::query(
        "SELECT * FROM issue_intake_policy_revisions WHERE project_id=?1 AND id=?2",
    )
    .bind(project_id)
    .bind(policy_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(Error::NotFound)?;
    policy_from_row(&row)
}

async fn load_policy_history(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> Result<Vec<IssueIntakePolicyRecord>, Error> {
    let rows = sqlx::query::query(
        "SELECT * FROM issue_intake_policy_revisions WHERE project_id=?1
         ORDER BY revision DESC LIMIT ?2",
    )
    .bind(project_id)
    .bind(POLICY_HISTORY_LIMIT)
    .fetch_all(&mut *connection)
    .await?;
    rows.iter().map(policy_from_row).collect()
}

async fn load_batch(
    connection: &mut SqliteConnection,
    project_id: &str,
    batch_id: &str,
) -> Result<IssueIntakeBatchRecord, Error> {
    let row =
        sqlx::query::query("SELECT * FROM issue_intake_batches WHERE project_id=?1 AND id=?2")
            .bind(project_id)
            .bind(batch_id)
            .fetch_optional(&mut *connection)
            .await?
            .ok_or(Error::NotFound)?;
    batch_from_row(&row)
}

async fn load_batches(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> Result<Vec<IssueIntakeBatchRecord>, Error> {
    let rows = sqlx::query::query(
        "SELECT * FROM issue_intake_batches WHERE project_id=?1
         ORDER BY created_at DESC,id DESC LIMIT ?2",
    )
    .bind(project_id)
    .bind(BATCH_HISTORY_LIMIT)
    .fetch_all(&mut *connection)
    .await?;
    rows.iter().map(batch_from_row).collect()
}

async fn load_batch_items(
    connection: &mut SqliteConnection,
    project_id: &str,
    batch_id: &str,
) -> Result<Vec<IssueIntakeBatchItemRecord>, Error> {
    let rows = sqlx::query::query(
        "SELECT * FROM issue_intake_batch_items WHERE project_id=?1 AND batch_id=?2
         ORDER BY issue_number",
    )
    .bind(project_id)
    .bind(batch_id)
    .fetch_all(&mut *connection)
    .await?;
    rows.iter().map(batch_item_from_row).collect()
}

async fn load_queue_items(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> Result<Vec<IssueIntakeQueueItemRecord>, Error> {
    let rows = sqlx::query::query(
        "SELECT * FROM issue_intake_queue WHERE project_id=?1
         ORDER BY created_at DESC,id DESC LIMIT ?2",
    )
    .bind(project_id)
    .bind(QUEUE_LIMIT)
    .fetch_all(&mut *connection)
    .await?;
    rows.iter().map(queue_item_from_row).collect()
}

async fn load_audit(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> Result<Vec<IssueIntakeAuditRecord>, Error> {
    let rows = sqlx::query::query(
        "SELECT * FROM issue_intake_audit WHERE project_id=?1
         ORDER BY created_at DESC,audit_id DESC LIMIT ?2",
    )
    .bind(project_id)
    .bind(AUDIT_LIMIT)
    .fetch_all(&mut *connection)
    .await?;
    rows.iter().map(audit_from_row).collect()
}

async fn load_issue_snapshot(
    connection: &mut SqliteConnection,
    project_id: &str,
    repository_full_name: &str,
    number: u64,
) -> Result<Option<IssueSnapshot>, Error> {
    let number = i64::try_from(number).map_err(|_| Error::Invalid("issue number"))?;
    let row = sqlx::query::query(
        "SELECT project_id,repo_full_name,repository_github_id,number,github_id,title,body,
                body_truncated,state,state_reason,labels_json,author,author_id,
                author_association,html_url,comments,created_at,updated_at,closed_at,
                content_hash,is_pull_request
         FROM github_issues WHERE project_id=?1 AND repo_full_name=?2 AND number=?3",
    )
    .bind(project_id)
    .bind(repository_full_name)
    .bind(number)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let labels: String = row.try_get("labels_json")?;
    let labels: Vec<GitHubLabel> =
        serde_json::from_str(&labels).map_err(|_| Error::Invalid("GitHub issue labels"))?;
    Ok(Some(IssueSnapshot {
        project_id: row.try_get("project_id")?,
        repository_full_name: row.try_get("repo_full_name")?,
        repository_github_id: row.try_get("repository_github_id")?,
        number: u64::try_from(row.try_get::<i64, _>("number")?)
            .map_err(|_| Error::Conflict("stored issue number is invalid"))?,
        github_id: row.try_get("github_id")?,
        title: row.try_get("title")?,
        body: row.try_get("body")?,
        body_truncated: row.try_get("body_truncated")?,
        state: row.try_get("state")?,
        state_reason: row.try_get("state_reason")?,
        labels,
        author: row.try_get("author")?,
        author_id: row.try_get("author_id")?,
        author_association: row.try_get("author_association")?,
        html_url: row.try_get("html_url")?,
        comments: u32::try_from(row.try_get::<i64, _>("comments")?)
            .map_err(|_| Error::Conflict("stored issue comments are invalid"))?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        closed_at: row.try_get("closed_at")?,
        content_hash: row.try_get("content_hash")?,
        is_pull_request: row.try_get::<i64, _>("is_pull_request")? != 0,
    }))
}

async fn active_project_requests(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> Result<u32, Error> {
    let count: i64 = sqlx::query_scalar::query_scalar(
        "SELECT COUNT(*) FROM lead_requests lr
         LEFT JOIN campaigns c ON c.id=lr.campaign_id
         WHERE lr.project_id=?1
           AND NOT EXISTS (SELECT 1 FROM lead_withdrawals w WHERE w.request_id=lr.id)
           AND (
             (lr.campaign_id IS NULL AND lr.state IN
               ('SUBMITTED','ASSESSING','PLANNED','FROZEN','READY'))
             OR (lr.campaign_id IS NOT NULL AND c.status NOT IN
               ('FAILED','CANCELLED','COMPLETED'))
           )",
    )
    .bind(project_id)
    .fetch_one(&mut *connection)
    .await?;
    Ok(u32::try_from(count.max(0)).unwrap_or(u32::MAX))
}

async fn duplicate_reconciled(
    connection: &mut SqliteConnection,
    project_id: &str,
    repository_github_id: i64,
    issue_number: u64,
    item_id: &str,
) -> Result<bool, Error> {
    let issue_number = i64::try_from(issue_number).map_err(|_| Error::Invalid("issue number"))?;
    let count: i64 = sqlx::query_scalar::query_scalar(
        "SELECT COUNT(*) FROM issue_intake_batch_items
         WHERE project_id=?1 AND repository_github_id=?2 AND issue_number=?3
           AND id<>?4 AND intake_request_id IS NOT NULL",
    )
    .bind(project_id)
    .bind(repository_github_id)
    .bind(issue_number)
    .bind(item_id)
    .fetch_one(&mut *connection)
    .await?;
    Ok(count > 0)
}

fn policy_facts(policy: &IssueIntakePolicyRecord, now: DateTime<Utc>) -> PolicyFacts<'_> {
    PolicyFacts {
        enabled: policy.enabled,
        expired: !deadline_in_future(&policy.expires_at, now),
        repository_full_name: &policy.repository_full_name,
        repository_github_id: policy.repository_github_id,
        connection_revision: policy.connection_revision,
        required_labels: &policy.required_labels,
        excluded_labels: &policy.excluded_labels,
        author_ids: &policy.author_ids,
        author_associations: &policy.author_associations,
        profile_revision_id: &policy.profile_revision_id,
        profile_hash: &policy.profile_hash,
        project_config_hash: &policy.project_config_hash,
        allowed_paths: &policy.allowed_paths,
        capabilities: &policy.capabilities,
        max_risk_class: &policy.max_risk_class,
    }
}

#[allow(clippy::too_many_arguments)]
fn project_facts_for_claim<'a>(
    config: &'a ProjectConfig,
    binding: &'a ProjectGitHubBinding,
    connection_enabled: bool,
    connection_revision: u32,
    profile_revision_id: &'a str,
    profile_hash: &'a str,
    active_project_requests: u32,
    duplicate_reconciled: bool,
) -> ProjectFacts<'a> {
    ProjectFacts {
        project_id: &config.project_id,
        active: config.active,
        config_hash: &config.config_hash,
        allowed_paths: &config.allowed_paths,
        capabilities: &config.capabilities,
        binding_repository_full_name: Some(&binding.repo_full_name),
        binding_repository_github_id: binding.repository_github_id,
        connection_present: true,
        connection_enabled,
        connection_revision,
        profile_executable: true,
        profile_revision_id,
        profile_hash,
        active_project_requests,
        duplicate_reconciled,
    }
}

#[allow(clippy::too_many_arguments)]
async fn current_batch_item_eligibility(
    connection: &mut SqliteConnection,
    project_id: &str,
    config: &ProjectConfig,
    binding: Option<&ProjectGitHubBinding>,
    connection_generation: Option<(bool, u32)>,
    policy: Option<&IssueIntakePolicyRecord>,
    current_head: Option<&str>,
    batch: &IssueIntakeBatchRecord,
    item: &IssueIntakeBatchItemRecord,
    profile: Option<&super::intake::ResolvedIntakeProfile>,
    active_requests: u32,
) -> Result<IssueIntakeEligibility, Error> {
    let Some(policy) = policy else {
        return Ok(IssueIntakeEligibility {
            eligible: false,
            reasons: vec!["POLICY_MISSING".into()],
        });
    };
    let Some(binding) = binding else {
        return Ok(IssueIntakeEligibility {
            eligible: false,
            reasons: vec!["BINDING_MISSING".into()],
        });
    };
    let Some((connection_enabled, connection_revision)) = connection_generation else {
        return Ok(IssueIntakeEligibility {
            eligible: false,
            reasons: vec!["CONNECTION_UNAVAILABLE".into()],
        });
    };
    let Some(profile) = profile else {
        return Ok(IssueIntakeEligibility {
            eligible: false,
            reasons: vec!["PROFILE_NOT_EXECUTABLE".into()],
        });
    };
    let Some(snapshot) = load_issue_snapshot(
        connection,
        project_id,
        &batch.repository_full_name,
        item.issue_number,
    )
    .await?
    else {
        return Ok(IssueIntakeEligibility {
            eligible: false,
            reasons: vec!["SNAPSHOT_MISSING".into()],
        });
    };
    let duplicate = duplicate_reconciled(
        connection,
        project_id,
        batch.repository_github_id,
        item.issue_number,
        &item.id,
    )
    .await?;
    let policy_facts = PolicyFacts {
        enabled: policy.enabled && current_head == Some(policy.id.as_str()),
        expired: !deadline_in_future(&policy.expires_at, Utc::now()),
        repository_full_name: &policy.repository_full_name,
        repository_github_id: policy.repository_github_id,
        connection_revision: policy.connection_revision,
        required_labels: &policy.required_labels,
        excluded_labels: &policy.excluded_labels,
        author_ids: &policy.author_ids,
        author_associations: &policy.author_associations,
        profile_revision_id: &policy.profile_revision_id,
        profile_hash: &policy.profile_hash,
        project_config_hash: &policy.project_config_hash,
        allowed_paths: &policy.allowed_paths,
        capabilities: &policy.capabilities,
        max_risk_class: &policy.max_risk_class,
    };
    let batch_facts = BatchFacts {
        status: &batch.status,
        expired: !deadline_in_future(&batch.expires_at, Utc::now()),
        repository_full_name: &batch.repository_full_name,
        repository_github_id: batch.repository_github_id,
        connection_revision: batch.connection_revision,
        max_issues: batch.max_issues,
        reserved_issues: batch.reserved_issues,
        max_total_cost_micros: batch.max_total_cost_micros,
        reserved_cost_micros: batch.reserved_cost_micros,
        max_cost_micros_per_issue: batch.max_cost_micros_per_issue,
        profile_revision_id: &batch.profile_revision_id,
        profile_hash: &batch.profile_hash,
        project_config_hash: &batch.project_config_hash,
    };
    let labels = item.labels.clone();
    let acknowledged = AcknowledgedIssue {
        project_id: &item.project_id,
        repository_full_name: &item.repository_full_name,
        repository_github_id: item.repository_github_id,
        issue_number: item.issue_number,
        github_issue_id: item.github_issue_id,
        content_hash: &item.content_hash,
        issue_updated_at: &item.issue_updated_at,
        author_id: item.author_id,
        author_association: item.author_association.as_deref(),
        labels: &labels,
    };
    let project_facts = ProjectFacts {
        project_id: &config.project_id,
        active: config.active,
        config_hash: &config.config_hash,
        allowed_paths: &config.allowed_paths,
        capabilities: &config.capabilities,
        binding_repository_full_name: Some(&binding.repo_full_name),
        binding_repository_github_id: binding.repository_github_id,
        connection_present: true,
        connection_enabled,
        connection_revision,
        profile_executable: true,
        profile_revision_id: &profile.revision_id,
        profile_hash: &profile.content_hash,
        active_project_requests: active_requests,
        duplicate_reconciled: duplicate,
    };
    Ok(evaluate_issue_intake_eligibility(
        Some(&policy_facts),
        &batch_facts,
        &acknowledged,
        &snapshot,
        &project_facts,
    ))
}

async fn insert_policy(
    connection: &mut SqliteConnection,
    policy: &IssueIntakePolicyRecord,
) -> Result<(), Error> {
    sqlx::query::query(
        "INSERT INTO issue_intake_policy_revisions
         (id,project_id,revision,enabled,repository_full_name,repository_github_id,
          connection_revision,required_labels_json,excluded_labels_json,author_ids_json,
          author_associations_json,profile_id,profile_revision_id,profile_revision,
          profile_hash,project_config_hash,allowed_paths_json,capabilities_json,
          max_risk_class,max_issues,max_concurrent,max_cost_micros_per_issue,
          max_total_cost_micros,max_attempts,max_sessions,max_turns,max_wall_clock_secs,
          expires_at,acknowledged,principal_id,principal_label,reason,idempotency_key,
          content_hash,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,
                 'R0',?19,?20,?21,?22,?23,?24,?25,?26,?27,1,?28,?29,?30,?31,?32,?33)",
    )
    .bind(&policy.id)
    .bind(&policy.project_id)
    .bind(i64::from(policy.revision))
    .bind(policy.enabled)
    .bind(&policy.repository_full_name)
    .bind(policy.repository_github_id)
    .bind(i64::from(policy.connection_revision))
    .bind(json(&policy.required_labels)?)
    .bind(json(&policy.excluded_labels)?)
    .bind(json(&policy.author_ids)?)
    .bind(json(&policy.author_associations)?)
    .bind(&policy.profile_id)
    .bind(&policy.profile_revision_id)
    .bind(i64::from(policy.profile_revision))
    .bind(&policy.profile_hash)
    .bind(&policy.project_config_hash)
    .bind(json(&policy.allowed_paths)?)
    .bind(json(&policy.capabilities)?)
    .bind(i64::from(policy.max_issues))
    .bind(i64::from(policy.max_concurrent))
    .bind(policy.max_cost_micros_per_issue as i64)
    .bind(policy.max_total_cost_micros as i64)
    .bind(i64::from(policy.max_attempts))
    .bind(i64::from(policy.max_sessions))
    .bind(i64::from(policy.max_turns))
    .bind(policy.max_wall_clock_secs as i64)
    .bind(&policy.expires_at)
    .bind(&policy.principal_id)
    .bind(&policy.principal_label)
    .bind(&policy.reason)
    .bind(&policy.idempotency_key)
    .bind(&policy.content_hash)
    .bind(&policy.created_at)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn insert_batch(
    connection: &mut SqliteConnection,
    batch: &IssueIntakeBatchRecord,
) -> Result<(), Error> {
    sqlx::query::query(
        "INSERT INTO issue_intake_batches
         (id,project_id,policy_id,policy_revision,policy_hash,delegation_id,
          repository_full_name,repository_github_id,connection_revision,profile_id,
          profile_revision_id,profile_revision,profile_hash,project_config_hash,
          max_issues,max_concurrent,max_cost_micros_per_issue,max_total_cost_micros,
          reserved_cost_micros,reserved_issues,status,acknowledged,principal_id,
          principal_label,reason,idempotency_key,batch_hash,expires_at,created_at,
          updated_at,revoked_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,
                 0,0,'ACTIVE',1,?19,?20,?21,?22,?23,?24,?25,?26,NULL)",
    )
    .bind(&batch.id)
    .bind(&batch.project_id)
    .bind(&batch.policy_id)
    .bind(i64::from(batch.policy_revision))
    .bind(&batch.policy_hash)
    .bind(&batch.delegation_id)
    .bind(&batch.repository_full_name)
    .bind(batch.repository_github_id)
    .bind(i64::from(batch.connection_revision))
    .bind(&batch.profile_id)
    .bind(&batch.profile_revision_id)
    .bind(i64::from(batch.profile_revision))
    .bind(&batch.profile_hash)
    .bind(&batch.project_config_hash)
    .bind(i64::from(batch.max_issues))
    .bind(i64::from(batch.max_concurrent))
    .bind(batch.max_cost_micros_per_issue as i64)
    .bind(batch.max_total_cost_micros as i64)
    .bind(&batch.principal_id)
    .bind(&batch.principal_label)
    .bind(&batch.reason)
    .bind(&batch.idempotency_key)
    .bind(&batch.batch_hash)
    .bind(&batch.expires_at)
    .bind(&batch.created_at)
    .bind(&batch.updated_at)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn insert_batch_item(
    connection: &mut SqliteConnection,
    item: &IssueIntakeBatchItemRecord,
) -> Result<(), Error> {
    sqlx::query::query(
        "INSERT INTO issue_intake_batch_items
         (id,batch_id,project_id,repository_full_name,repository_github_id,issue_number,
          github_issue_id,content_hash,issue_updated_at,author_login,author_id,
          author_association,labels_json,state,eligibility_reasons_json,
          reserved_cost_micros,intake_request_id,queue_item_id,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,'PENDING','[]',0,NULL,
                 NULL,?14,?15)",
    )
    .bind(&item.id)
    .bind(&item.batch_id)
    .bind(&item.project_id)
    .bind(&item.repository_full_name)
    .bind(item.repository_github_id)
    .bind(i64::try_from(item.issue_number).map_err(|_| Error::Invalid("issue number"))?)
    .bind(item.github_issue_id)
    .bind(&item.content_hash)
    .bind(&item.issue_updated_at)
    .bind(&item.author_login)
    .bind(item.author_id)
    .bind(&item.author_association)
    .bind(json(&item.labels)?)
    .bind(&item.created_at)
    .bind(&item.updated_at)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn insert_queue_item_for_batch_item(
    connection: &mut SqliteConnection,
    batch: &IssueIntakeBatchRecord,
    item: &IssueIntakeBatchItemRecord,
) -> Result<(), Error> {
    let queue_id = queue_item_id_for_batch_item(&item.id);
    sqlx::query::query(
        "INSERT INTO issue_intake_queue
         (id,project_id,batch_id,batch_item_id,repository_full_name,repository_github_id,
          issue_number,github_issue_id,content_hash,connection_revision,policy_revision,
          status,lease_token,lease_expires_at,attempts,max_attempts,next_attempt_at,
          reason,reasons_json,intake_request_id,lead_request_id,action_id,created_at,
          updated_at,completed_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'PENDING',NULL,NULL,0,?13,NULL,
                 NULL,'[]',NULL,NULL,NULL,?12,?12,NULL)",
    )
    .bind(&queue_id)
    .bind(&batch.project_id)
    .bind(&batch.id)
    .bind(&item.id)
    .bind(&batch.repository_full_name)
    .bind(batch.repository_github_id)
    .bind(i64::try_from(item.issue_number).map_err(|_| Error::Invalid("issue number"))?)
    .bind(item.github_issue_id)
    .bind(&item.content_hash)
    .bind(i64::from(batch.connection_revision))
    .bind(i64::from(batch.policy_revision))
    .bind(&batch.created_at)
    .bind(i64::from(crate::issue_intake::MAX_QUEUE_ATTEMPTS))
    .execute(&mut *connection)
    .await?;
    sqlx::query::query("UPDATE issue_intake_batch_items SET queue_item_id=?1 WHERE id=?2")
        .bind(&queue_id)
        .bind(&item.id)
        .execute(&mut *connection)
        .await?;
    Ok(())
}

async fn insert_batch_policy_grant(
    connection: &mut SqliteConnection,
    policy: &IssueIntakePolicyRecord,
    delegation: &StandingDelegation,
    principal_id: &str,
    idempotency_key: &str,
    created_at: &str,
) -> Result<(), Error> {
    let grant_key = format!(
        "issue-batch-policy:{}",
        sha256_hex(idempotency_key.as_bytes())
    );
    sqlx::query::query(
        "INSERT INTO execution_policy_grants
         (delegation_id,project_id,principal_id,profile_id,profile_revision_id,
          profile_revision,profile_hash,project_config_hash,acknowledged,idempotency_key,
          created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,1,?9,?10)",
    )
    .bind(&delegation.id)
    .bind(&policy.project_id)
    .bind(principal_id)
    .bind(&policy.profile_id)
    .bind(&policy.profile_revision_id)
    .bind(i64::from(policy.profile_revision))
    .bind(&policy.profile_hash)
    .bind(&policy.project_config_hash)
    .bind(&grant_key)
    .bind(created_at)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn revoke_active_batches_for_project(
    connection: &mut SqliteConnection,
    project_id: &str,
    reason: &str,
    actor: &str,
    now: &str,
) -> Result<(), Error> {
    let rows = sqlx::query::query(
        "SELECT id,delegation_id,status FROM issue_intake_batches
         WHERE project_id=?1 AND status='ACTIVE'",
    )
    .bind(project_id)
    .fetch_all(&mut *connection)
    .await?;
    for row in rows {
        let batch = load_batch(connection, project_id, &row.try_get::<String, _>("id")?).await?;
        revoke_batch(connection, &batch, reason, actor, now).await?;
    }
    Ok(())
}

async fn revoke_batch(
    connection: &mut SqliteConnection,
    batch: &IssueIntakeBatchRecord,
    reason: &str,
    actor: &str,
    now: &str,
) -> Result<(), Error> {
    sqlx::query::query(
        "UPDATE issue_intake_batches
         SET status='REVOKED',revoked_at=?1,updated_at=?1 WHERE id=?2 AND status='ACTIVE'",
    )
    .bind(now)
    .bind(&batch.id)
    .execute(&mut *connection)
    .await?;
    sqlx::query::query(
        "UPDATE issue_intake_batch_items
         SET state='REVOKED',updated_at=?1
         WHERE batch_id=?2 AND state='PENDING'",
    )
    .bind(now)
    .bind(&batch.id)
    .execute(&mut *connection)
    .await?;
    sqlx::query::query(
        "UPDATE issue_intake_queue
         SET status='REVOKED',lease_token=NULL,lease_expires_at=NULL,updated_at=?1
         WHERE batch_id=?2 AND status IN ('PENDING','LEASED')",
    )
    .bind(now)
    .bind(&batch.id)
    .execute(&mut *connection)
    .await?;
    sqlx::query::query(
        "UPDATE standing_delegations SET revoked_at=?1 WHERE id=?2 AND revoked_at IS NULL",
    )
    .bind(now)
    .bind(&batch.delegation_id)
    .execute(&mut *connection)
    .await?;
    insert_issue_intake_audit(
        connection,
        &batch.project_id,
        Some(&batch.policy_id),
        Some(&batch.id),
        None,
        "batch_revoked",
        actor,
        actor,
        reason,
        &format!("delegation={} autonomous_start=false", batch.delegation_id),
        now,
    )
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_issue_intake_audit(
    connection: &mut SqliteConnection,
    project_id: &str,
    policy_id: Option<&str>,
    batch_id: Option<&str>,
    queue_item_id: Option<&str>,
    action: &str,
    actor: &str,
    principal_id: &str,
    reason: &str,
    detail: &str,
    created_at: &str,
) -> Result<(), Error> {
    sqlx::query::query(
        "INSERT INTO issue_intake_audit
         (audit_id,project_id,policy_id,batch_id,queue_item_id,action,actor,principal_id,
          reason,detail,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(project_id)
    .bind(policy_id)
    .bind(batch_id)
    .bind(queue_item_id)
    .bind(action)
    .bind(actor)
    .bind(principal_id)
    .bind(reason)
    .bind(detail)
    .bind(created_at)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

fn json<T: Serialize>(value: &T) -> Result<String, Error> {
    serde_json::to_string(value).map_err(|_| Error::Invalid("issue intake JSON encoding"))
}

fn policy_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<IssueIntakePolicyRecord, Error> {
    Ok(IssueIntakePolicyRecord {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        revision: u32::try_from(row.try_get::<i64, _>("revision")?)
            .map_err(|_| Error::Conflict("stored policy revision is invalid"))?,
        enabled: row.try_get::<i64, _>("enabled")? != 0,
        repository_full_name: row.try_get("repository_full_name")?,
        repository_github_id: row.try_get("repository_github_id")?,
        connection_revision: u32::try_from(row.try_get::<i64, _>("connection_revision")?)
            .map_err(|_| Error::Conflict("stored connection revision is invalid"))?,
        required_labels: parse_json(row.try_get("required_labels_json")?)?,
        excluded_labels: parse_json(row.try_get("excluded_labels_json")?)?,
        author_ids: parse_json(row.try_get("author_ids_json")?)?,
        author_associations: parse_json(row.try_get("author_associations_json")?)?,
        profile_id: row.try_get("profile_id")?,
        profile_revision_id: row.try_get("profile_revision_id")?,
        profile_revision: u32::try_from(row.try_get::<i64, _>("profile_revision")?)
            .map_err(|_| Error::Conflict("stored profile revision is invalid"))?,
        profile_hash: row.try_get("profile_hash")?,
        project_config_hash: row.try_get("project_config_hash")?,
        allowed_paths: parse_json(row.try_get("allowed_paths_json")?)?,
        capabilities: parse_json(row.try_get("capabilities_json")?)?,
        max_risk_class: row.try_get("max_risk_class")?,
        max_issues: u32::try_from(row.try_get::<i64, _>("max_issues")?)
            .map_err(|_| Error::Conflict("stored issue limit is invalid"))?,
        max_concurrent: u32::try_from(row.try_get::<i64, _>("max_concurrent")?)
            .map_err(|_| Error::Conflict("stored concurrency limit is invalid"))?,
        max_cost_micros_per_issue: u64::try_from(
            row.try_get::<i64, _>("max_cost_micros_per_issue")?,
        )
        .map_err(|_| Error::Conflict("stored issue cost is invalid"))?,
        max_total_cost_micros: u64::try_from(row.try_get::<i64, _>("max_total_cost_micros")?)
            .map_err(|_| Error::Conflict("stored batch cost is invalid"))?,
        max_attempts: u32::try_from(row.try_get::<i64, _>("max_attempts")?)
            .map_err(|_| Error::Conflict("stored attempt limit is invalid"))?,
        max_sessions: u32::try_from(row.try_get::<i64, _>("max_sessions")?)
            .map_err(|_| Error::Conflict("stored session limit is invalid"))?,
        max_turns: u32::try_from(row.try_get::<i64, _>("max_turns")?)
            .map_err(|_| Error::Conflict("stored turn limit is invalid"))?,
        max_wall_clock_secs: u64::try_from(row.try_get::<i64, _>("max_wall_clock_secs")?)
            .map_err(|_| Error::Conflict("stored wall-clock limit is invalid"))?,
        expires_at: row.try_get("expires_at")?,
        acknowledged: row.try_get::<i64, _>("acknowledged")? != 0,
        principal_id: row.try_get("principal_id")?,
        principal_label: row.try_get("principal_label")?,
        reason: row.try_get("reason")?,
        idempotency_key: row.try_get("idempotency_key")?,
        content_hash: row.try_get("content_hash")?,
        created_at: row.try_get("created_at")?,
    })
}

fn batch_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<IssueIntakeBatchRecord, Error> {
    Ok(IssueIntakeBatchRecord {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        policy_id: row.try_get("policy_id")?,
        policy_revision: u32::try_from(row.try_get::<i64, _>("policy_revision")?)
            .map_err(|_| Error::Conflict("stored policy revision is invalid"))?,
        policy_hash: row.try_get("policy_hash")?,
        delegation_id: row.try_get("delegation_id")?,
        repository_full_name: row.try_get("repository_full_name")?,
        repository_github_id: row.try_get("repository_github_id")?,
        connection_revision: u32::try_from(row.try_get::<i64, _>("connection_revision")?)
            .map_err(|_| Error::Conflict("stored connection revision is invalid"))?,
        profile_id: row.try_get("profile_id")?,
        profile_revision_id: row.try_get("profile_revision_id")?,
        profile_revision: u32::try_from(row.try_get::<i64, _>("profile_revision")?)
            .map_err(|_| Error::Conflict("stored profile revision is invalid"))?,
        profile_hash: row.try_get("profile_hash")?,
        project_config_hash: row.try_get("project_config_hash")?,
        max_issues: u32::try_from(row.try_get::<i64, _>("max_issues")?)
            .map_err(|_| Error::Conflict("stored issue limit is invalid"))?,
        max_concurrent: u32::try_from(row.try_get::<i64, _>("max_concurrent")?)
            .map_err(|_| Error::Conflict("stored concurrency limit is invalid"))?,
        max_cost_micros_per_issue: u64::try_from(
            row.try_get::<i64, _>("max_cost_micros_per_issue")?,
        )
        .map_err(|_| Error::Conflict("stored issue cost is invalid"))?,
        max_total_cost_micros: u64::try_from(row.try_get::<i64, _>("max_total_cost_micros")?)
            .map_err(|_| Error::Conflict("stored batch cost is invalid"))?,
        reserved_cost_micros: u64::try_from(row.try_get::<i64, _>("reserved_cost_micros")?)
            .map_err(|_| Error::Conflict("stored reserved cost is invalid"))?,
        reserved_issues: u32::try_from(row.try_get::<i64, _>("reserved_issues")?)
            .map_err(|_| Error::Conflict("stored reserved issues are invalid"))?,
        status: row.try_get("status")?,
        acknowledged: row.try_get::<i64, _>("acknowledged")? != 0,
        principal_id: row.try_get("principal_id")?,
        principal_label: row.try_get("principal_label")?,
        reason: row.try_get("reason")?,
        idempotency_key: row.try_get("idempotency_key")?,
        batch_hash: row.try_get("batch_hash")?,
        expires_at: row.try_get("expires_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        revoked_at: row.try_get("revoked_at")?,
    })
}

fn batch_item_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<IssueIntakeBatchItemRecord, Error> {
    Ok(IssueIntakeBatchItemRecord {
        id: row.try_get("id")?,
        batch_id: row.try_get("batch_id")?,
        project_id: row.try_get("project_id")?,
        repository_full_name: row.try_get("repository_full_name")?,
        repository_github_id: row.try_get("repository_github_id")?,
        issue_number: u64::try_from(row.try_get::<i64, _>("issue_number")?)
            .map_err(|_| Error::Conflict("stored issue number is invalid"))?,
        github_issue_id: row.try_get("github_issue_id")?,
        content_hash: row.try_get("content_hash")?,
        issue_updated_at: row.try_get("issue_updated_at")?,
        author_login: row.try_get("author_login")?,
        author_id: row.try_get("author_id")?,
        author_association: row.try_get("author_association")?,
        labels: parse_json(row.try_get("labels_json")?)?,
        state: row.try_get("state")?,
        eligibility_reasons: parse_json(row.try_get("eligibility_reasons_json")?)?,
        reserved_cost_micros: u64::try_from(row.try_get::<i64, _>("reserved_cost_micros")?)
            .map_err(|_| Error::Conflict("stored reserved cost is invalid"))?,
        intake_request_id: row.try_get("intake_request_id")?,
        queue_item_id: row.try_get("queue_item_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn queue_item_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<IssueIntakeQueueItemRecord, Error> {
    Ok(IssueIntakeQueueItemRecord {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        batch_id: row.try_get("batch_id")?,
        batch_item_id: row.try_get("batch_item_id")?,
        repository_full_name: row.try_get("repository_full_name")?,
        repository_github_id: row.try_get("repository_github_id")?,
        issue_number: u64::try_from(row.try_get::<i64, _>("issue_number")?)
            .map_err(|_| Error::Conflict("stored issue number is invalid"))?,
        github_issue_id: row.try_get("github_issue_id")?,
        content_hash: row.try_get("content_hash")?,
        connection_revision: u32::try_from(row.try_get::<i64, _>("connection_revision")?)
            .map_err(|_| Error::Conflict("stored connection revision is invalid"))?,
        policy_revision: u32::try_from(row.try_get::<i64, _>("policy_revision")?)
            .map_err(|_| Error::Conflict("stored policy revision is invalid"))?,
        status: row.try_get("status")?,
        lease_token: row.try_get("lease_token")?,
        lease_expires_at: row.try_get("lease_expires_at")?,
        attempts: u32::try_from(row.try_get::<i64, _>("attempts")?)
            .map_err(|_| Error::Conflict("stored queue attempts are invalid"))?,
        max_attempts: u32::try_from(row.try_get::<i64, _>("max_attempts")?)
            .map_err(|_| Error::Conflict("stored queue attempts are invalid"))?,
        next_attempt_at: row.try_get("next_attempt_at")?,
        reason: row.try_get("reason")?,
        reasons: parse_json(row.try_get("reasons_json")?)?,
        intake_request_id: row.try_get("intake_request_id")?,
        lead_request_id: row.try_get("lead_request_id")?,
        action_id: row.try_get("action_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        completed_at: row.try_get("completed_at")?,
    })
}

fn audit_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<IssueIntakeAuditRecord, Error> {
    Ok(IssueIntakeAuditRecord {
        audit_id: row.try_get("audit_id")?,
        project_id: row.try_get("project_id")?,
        policy_id: row.try_get("policy_id")?,
        batch_id: row.try_get("batch_id")?,
        queue_item_id: row.try_get("queue_item_id")?,
        action: row.try_get("action")?,
        actor: row.try_get("actor")?,
        principal_id: row.try_get("principal_id")?,
        reason: row.try_get("reason")?,
        detail: row.try_get("detail")?,
        created_at: row.try_get("created_at")?,
    })
}

fn parse_json<T: serde::de::DeserializeOwned>(value: String) -> Result<T, Error> {
    serde_json::from_str(&value).map_err(|_| Error::Invalid("issue intake JSON value"))
}

/// Recover leases left by a previous Core process. Called once during database
/// open before any request can race. Committed request links are preserved;
/// uncommitted leases return to PENDING or BLOCKED after their retry budget.
pub(super) async fn recover_issue_intake_leases_on_open(pool: &SqlitePool) -> Result<(), Error> {
    let now = Utc::now().to_rfc3339();
    let rows = sqlx::query::query(
        "SELECT id,project_id,batch_id,batch_item_id,status,intake_request_id,attempts,
                max_attempts
         FROM issue_intake_queue WHERE status='LEASED'",
    )
    .fetch_all(pool)
    .await?;
    for row in rows {
        let id: String = row.try_get("id")?;
        let project_id: String = row.try_get("project_id")?;
        let batch_id: String = row.try_get("batch_id")?;
        let batch_item_id: String = row.try_get("batch_item_id")?;
        let intake_request_id: Option<String> = row.try_get("intake_request_id")?;
        let attempts: i64 = row.try_get("attempts")?;
        let max_attempts: i64 = row.try_get("max_attempts")?;
        let (status, reason) = if intake_request_id.is_some() {
            (
                "RECONCILED",
                "committed proposal link recovered on Core restart",
            )
        } else if attempts >= max_attempts {
            ("BLOCKED", "lease expired with no remaining retry budget")
        } else {
            (
                "PENDING",
                "lease expired on Core restart; retry is explicit",
            )
        };
        sqlx::query::query(
            "UPDATE issue_intake_queue SET status=?1,lease_token=NULL,lease_expires_at=NULL,
               reason=?2,updated_at=?3 WHERE id=?4 AND status='LEASED'",
        )
        .bind(status)
        .bind(reason)
        .bind(&now)
        .bind(&id)
        .execute(pool)
        .await?;
        if intake_request_id.is_some() {
            sqlx::query::query(
                "UPDATE issue_intake_batch_items SET state='RECONCILED',updated_at=?1
                 WHERE id=?2 AND state='PENDING'",
            )
            .bind(&now)
            .bind(&batch_item_id)
            .execute(pool)
            .await?;
        }
        insert_issue_intake_audit(
            &mut *pool.acquire().await?,
            &project_id,
            None,
            Some(&batch_id),
            Some(&id),
            "queue_recovered",
            "core-startup",
            "core-startup",
            "Core restart",
            &format!("status={status}"),
            &now,
        )
        .await?;
        record_event(
            &mut *pool.acquire().await?,
            &id,
            "issue_intake.queue_recovered",
            &format!("project={project_id} status={status}"),
        )
        .await?;
    }
    Ok(())
}
