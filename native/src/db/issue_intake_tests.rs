use super::*;

use chrono::{Duration, Utc};
use std::path::PathBuf;

use crate::execution_profile::{
    AutoStartPolicy, CampaignBudgets, ExecutionProfileCategory, ExecutionProfileContent,
    ExecutionProfileRoles, ExecutionRoleRoute, ReasoningEffort,
};
use crate::github::{GitHubIssue, GitHubLabel};
use crate::lead::{Capability, QualityProfile};

fn temp_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{prefix}-{}.sqlite", uuid::Uuid::new_v4()))
}

fn route() -> ExecutionRoleRoute {
    ExecutionRoleRoute {
        provider: "opencode-go".into(),
        model: "deepseek-v4.1-flash".into(),
        reasoning: Some(ReasoningEffort::Medium),
        max_input_tokens: 64_000,
        max_output_tokens: 8_192,
        max_cost_micros: 300_000,
    }
}

fn profile_content(auto_start: bool) -> ExecutionProfileContent {
    ExecutionProfileContent {
        name: "Balanced".into(),
        description: "issue intake test".into(),
        category: ExecutionProfileCategory::Balanced,
        roles: ExecutionProfileRoles {
            lead: route(),
            implementer: route(),
            semantic_reviewer: route(),
        },
        budgets: CampaignBudgets {
            max_attempts: 3,
            max_sessions: 4,
            max_turns: 4,
            max_input_tokens: 40_000,
            max_output_tokens: 12_000,
            max_cost_micros: 500_000,
            deadline_secs: 1_800,
        },
        auto_start: AutoStartPolicy {
            enabled: auto_start,
            risk_classes: if auto_start {
                vec![crate::lead::RiskClass::R0]
            } else {
                Vec::new()
            },
        },
    }
}

async fn configure_provider(db: &Database) {
    db.save_provider_settings(
        &ProviderSettingsWrite {
            provider: "opencode-go".into(),
            kind: "opencode".into(),
            model: "deepseek-v4.1-flash".into(),
            base_url: "https://provider.example/v1".into(),
            auth_mode: "api_key".into(),
            credential_ciphertext: "ciphertext".into(),
            credential_nonce: "nonce".into(),
            credential_fingerprint: "fingerprint".into(),
            enabled: true,
            is_default: true,
            max_turns: 4,
            max_input_tokens: 100_000,
            max_output_tokens: 12_000,
            max_cost_micros: 500_000,
            input_micros_per_million: 1_000,
            output_micros_per_million: 2_000,
        },
        "test-operator",
    )
    .await
    .unwrap();
}

struct Fixture {
    path: PathBuf,
    db: Database,
    config: crate::lead::ProjectConfig,
    profile: ExecutionProfileRecord,
}

async fn fixture(project: &str, repo: &str, repo_id: i64, auto_start: bool) -> Fixture {
    let path = temp_path("factory-issue-intake");
    let db = Database::open(&path).await.unwrap();
    let base = "b".repeat(40);
    sqlx::query::query(
        "INSERT INTO projects(id,repository,base_commit,created_at,identity)
         VALUES (?1,?2,?3,?4,?5)",
    )
    .bind(project)
    .bind(format!("/{repo}"))
    .bind(&base)
    .bind(Utc::now().to_rfc3339())
    .bind(repo)
    .execute(&db.pool)
    .await
    .unwrap();
    let mut transaction = db.pool.begin().await.unwrap();
    db.ensure_project_config(&mut transaction, project)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    let config = db
        .update_project_config(
            project,
            &NewProjectConfig {
                expected_revision: 1,
                allowed_paths: vec!["src".into()],
                capabilities: vec![Capability::RepositoryEdit],
                quality_profile: QualityProfile::Generic,
                active: true,
            },
        )
        .await
        .unwrap();
    configure_provider(&db).await;
    let profile = db
        .create_execution_profile(&profile_content(auto_start), "owner")
        .await
        .unwrap();
    let now = Utc::now().to_rfc3339();
    sqlx::query::query(
        "INSERT INTO github_connections
         (id,login,scopes_json,scopes_reported,credential_ciphertext,credential_nonce,
          credential_fingerprint,enabled,revision,verified_at,revoked_at,rate_limited_until,
          created_at,updated_at)
         VALUES ('default','octo','[]',1,'ciphertext','nonce','fingerprint',1,1,?1,NULL,NULL,?1,?1)",
    )
    .bind(&now)
    .execute(&db.pool)
    .await
    .unwrap();
    db.bind_project_github(
        project,
        NewProjectGitHubBinding {
            owner: "acme",
            repo,
            default_branch: "main",
            repository_github_id: Some(repo_id),
        },
        "tester",
    )
    .await
    .unwrap();
    Fixture {
        path,
        db,
        config,
        profile,
    }
}

fn issue(number: u64, github_id: i64, author_id: i64, association: &str) -> GitHubIssue {
    GitHubIssue {
        number,
        github_id,
        title: format!("Issue {number}"),
        body: format!("Body for issue {number}."),
        body_truncated: false,
        state: "open".into(),
        state_reason: None,
        labels: vec![GitHubLabel {
            name: "factory:r0".into(),
            color: Some("00ff00".into()),
        }],
        author: Some(format!("user{author_id}")),
        author_id: Some(author_id),
        author_association: Some(association.into()),
        html_url: format!("https://github.com/acme/widgets/issues/{number}"),
        comments: 0,
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-02T00:00:00Z".into(),
        closed_at: None,
    }
}

async fn insert_issue(
    db: &Database,
    project: &str,
    repo_full_name: &str,
    repo_id: i64,
    issue: &GitHubIssue,
) {
    let labels = serde_json::to_string(&issue.labels).unwrap();
    sqlx::query::query(
        "INSERT INTO github_issues
         (project_id,repo_full_name,number,github_id,title,body,body_truncated,state,
          state_reason,labels_json,author,author_id,author_association,html_url,comments,
          created_at,updated_at,closed_at,synced_at,sync_mark,content_hash,
          repository_github_id,is_pull_request)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,
                 'test',?20,?21,0)",
    )
    .bind(project)
    .bind(repo_full_name)
    .bind(i64::try_from(issue.number).unwrap())
    .bind(issue.github_id)
    .bind(&issue.title)
    .bind(&issue.body)
    .bind(issue.body_truncated)
    .bind(&issue.state)
    .bind(&issue.state_reason)
    .bind(&labels)
    .bind(&issue.author)
    .bind(issue.author_id)
    .bind(&issue.author_association)
    .bind(&issue.html_url)
    .bind(i64::from(issue.comments))
    .bind(&issue.created_at)
    .bind(&issue.updated_at)
    .bind(&issue.closed_at)
    .bind(Utc::now().to_rfc3339())
    .bind(issue.content_hash())
    .bind(repo_id)
    .execute(&db.pool)
    .await
    .unwrap();
}

fn policy_input(fixture: &Fixture) -> NewIssueIntakePolicy<'static> {
    // Leaked test strings keep the borrowed input concise; the database only
    // reads them during the call.
    let repository_full_name: &'static str = Box::leak("acme/widgets".to_string().into_boxed_str());
    let profile_id: &'static str = Box::leak(fixture.profile.id.clone().into_boxed_str());
    let revision_id: &'static str = Box::leak(fixture.profile.revision_id.clone().into_boxed_str());
    let profile_hash: &'static str =
        Box::leak(fixture.profile.content_hash.clone().into_boxed_str());
    let config_hash: &'static str = Box::leak(fixture.config.config_hash.clone().into_boxed_str());
    let expires_at: &'static str = Box::leak(
        (Utc::now() + Duration::days(7))
            .to_rfc3339()
            .into_boxed_str(),
    );
    let reason: &'static str = "acknowledge governed issue intake";
    let key: &'static str = "issue-policy-1";
    let paths: &'static [String] = Box::leak(vec!["src".to_string()].into_boxed_slice());
    let capabilities: &'static [Capability] =
        Box::leak(vec![Capability::RepositoryEdit].into_boxed_slice());
    let required: &'static [String] = Box::leak(vec!["factory:r0".to_string()].into_boxed_slice());
    let excluded: &'static [String] =
        Box::leak(vec!["factory:blocked".to_string()].into_boxed_slice());
    let author_ids: &'static [i64] = Box::leak(vec![7_i64].into_boxed_slice());
    let associations: &'static [String] = Box::leak(vec!["MEMBER".to_string()].into_boxed_slice());
    NewIssueIntakePolicy {
        project_id: "project",
        repository_full_name,
        repository_github_id: 42,
        required_labels: required,
        excluded_labels: excluded,
        author_ids,
        author_associations: associations,
        profile_id,
        profile_revision_id: revision_id,
        profile_revision: fixture.profile.revision,
        profile_hash,
        project_config_hash: config_hash,
        allowed_paths: paths,
        capabilities,
        max_issues: 5,
        max_concurrent: 1,
        max_cost_micros_per_issue: 100_000,
        max_total_cost_micros: 400_000,
        max_attempts: 2,
        max_sessions: 3,
        max_turns: 4,
        max_wall_clock_secs: 900,
        expires_at,
        acknowledge: true,
        reason,
        idempotency_key: key,
        principal_id: "operator-token",
        principal_label: None,
    }
}

fn batch_items(issue: &GitHubIssue) -> Vec<NewIssueIntakeBatchItem<'static>> {
    vec![NewIssueIntakeBatchItem {
        number: issue.number,
        github_issue_id: issue.github_id,
        content_hash: Box::leak(issue.content_hash().into_boxed_str()),
        updated_at: Box::leak(issue.updated_at.clone().into_boxed_str()),
    }]
}

async fn create_policy(fixture: &Fixture) -> crate::issue_intake::IssueIntakePolicyRecord {
    fixture
        .db
        .create_issue_intake_policy(policy_input(fixture))
        .await
        .unwrap()
        .policy
}

#[tokio::test]
async fn default_state_is_disabled_and_has_no_policy() {
    let fixture = fixture("project", "widgets", 42, false).await;
    let state = fixture.db.issue_intake_state("project").await.unwrap();
    assert!(!state.enabled);
    assert!(state.policy.is_none());
    assert!(state.batches.is_empty());
    assert!(state.queue.is_empty());
    assert!(!state.limits.autonomous_start);
    assert!(!state.limits.campaign_start);
    assert!(!state.limits.provider_calls);
    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.path);
}

#[tokio::test]
async fn policy_is_default_off_explicit_and_never_autonomous_from_profile() {
    let fixture = fixture("project", "widgets", 42, true).await;
    let policy = create_policy(&fixture).await;
    assert!(policy.enabled);
    assert_eq!(policy.max_risk_class, "R0");
    assert_eq!(policy.connection_revision, 1);
    let grants: (i64,) = sqlx::query_as::query_as(
        "SELECT COUNT(*) FROM execution_policy_grants WHERE project_id='project'",
    )
    .fetch_one(&fixture.db.pool)
    .await
    .unwrap();
    assert_eq!(grants.0, 0);
    let state = fixture.db.issue_intake_state("project").await.unwrap();
    assert!(state.enabled);
    assert_eq!(
        state.policy.as_ref().unwrap().content_hash,
        policy.content_hash
    );
    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.path);
}

#[tokio::test]
async fn batch_requires_acknowledgement_and_exact_snapshot_then_creates_finite_queue() {
    let fixture = fixture("project", "widgets", 42, false).await;
    let policy = create_policy(&fixture).await;
    let snapshot_issue = issue(5, 55, 7, "MEMBER");
    insert_issue(&fixture.db, "project", "acme/widgets", 42, &snapshot_issue).await;
    let items = batch_items(&snapshot_issue);
    let mut input = NewIssueIntakeBatch {
        project_id: "project",
        policy_id: &policy.id,
        expected_policy_revision: policy.revision,
        expected_policy_hash: &policy.content_hash,
        items: &items,
        max_total_cost_micros: 200_000,
        expires_at: Box::leak(
            (Utc::now() + Duration::days(1))
                .to_rfc3339()
                .into_boxed_str(),
        ),
        acknowledge: false,
        reason: "select exact issue",
        idempotency_key: "batch-1",
        principal_id: "operator-token",
        principal_label: None,
    };
    let error = fixture
        .db
        .create_issue_intake_batch(input.clone())
        .await
        .unwrap_err();
    assert_eq!(error_code(&error), "ISSUE_BATCH_ACKNOWLEDGEMENT_REQUIRED");
    input.acknowledge = true;
    let result = fixture.db.create_issue_intake_batch(input).await.unwrap();
    assert!(result.created);
    assert_eq!(result.batch.max_issues, 1);
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].github_issue_id, 55);
    let queue: (i64,) = sqlx::query_as::query_as(
        "SELECT COUNT(*) FROM issue_intake_queue WHERE batch_id=?1 AND status='PENDING'",
    )
    .bind(&result.batch.id)
    .fetch_one(&fixture.db.pool)
    .await
    .unwrap();
    assert_eq!(queue.0, 1);
    let delegation = fixture
        .db
        .standing_delegation(&result.batch.delegation_id)
        .await
        .unwrap();
    assert!(!delegation.autonomous_start);
    assert_eq!(delegation.max_requests, 1);
    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.path);
}

#[tokio::test]
async fn stale_snapshot_budget_and_ineligible_issues_are_rejected() {
    let fixture = fixture("project", "widgets", 42, false).await;
    let policy = create_policy(&fixture).await;
    let snapshot_issue = issue(5, 55, 7, "MEMBER");
    insert_issue(&fixture.db, "project", "acme/widgets", 42, &snapshot_issue).await;
    let items = batch_items(&snapshot_issue);
    let uuid = uuid::Uuid::new_v4().to_string();
    let mut stale = NewIssueIntakeBatch {
        project_id: "project",
        policy_id: &policy.id,
        expected_policy_revision: policy.revision,
        expected_policy_hash: &policy.content_hash,
        items: &items,
        max_total_cost_micros: 100_000,
        expires_at: Box::leak(
            (Utc::now() + Duration::days(1))
                .to_rfc3339()
                .into_boxed_str(),
        ),
        acknowledge: true,
        reason: "select exact issue",
        idempotency_key: &uuid,
        principal_id: "operator-token",
        principal_label: None,
    };
    // Budget exhaustion: one issue at 100k cannot fit a 1 micros batch.
    stale.max_total_cost_micros = 1;
    let error = fixture
        .db
        .create_issue_intake_batch(stale.clone())
        .await
        .unwrap_err();
    assert_eq!(error_code(&error), "ISSUE_BATCH_BUDGET_EXCEEDED");

    let mut edited = batch_items(&snapshot_issue);
    let edited_hash = "0".repeat(64);
    edited[0].content_hash = Box::leak(edited_hash.into_boxed_str());
    let error = fixture
        .db
        .create_issue_intake_batch(NewIssueIntakeBatch {
            items: &edited,
            idempotency_key: Box::leak(uuid::Uuid::new_v4().to_string().into_boxed_str()),
            ..stale.clone()
        })
        .await
        .unwrap_err();
    // `..stale` keeps the tiny budget, so assert either the snapshot or budget
    // guard; use a fresh budget to isolate snapshot staleness.
    let _ = error;
    let mut valid = NewIssueIntakeBatch {
        project_id: "project",
        policy_id: &policy.id,
        expected_policy_revision: policy.revision,
        expected_policy_hash: &policy.content_hash,
        items: &edited,
        max_total_cost_micros: 200_000,
        expires_at: Box::leak(
            (Utc::now() + Duration::days(1))
                .to_rfc3339()
                .into_boxed_str(),
        ),
        acknowledge: true,
        reason: "select exact issue",
        idempotency_key: Box::leak(uuid::Uuid::new_v4().to_string().into_boxed_str()),
        principal_id: "operator-token",
        principal_label: None,
    };
    let error = fixture
        .db
        .create_issue_intake_batch(valid.clone())
        .await
        .unwrap_err();
    assert_eq!(error_code(&error), "ISSUE_SNAPSHOT_STALE");

    let closed = GitHubIssue {
        state: "closed".into(),
        ..issue(6, 66, 7, "MEMBER")
    };
    insert_issue(&fixture.db, "project", "acme/widgets", 42, &closed).await;
    let closed_items = batch_items(&closed);
    valid.items = &closed_items;
    let error = fixture
        .db
        .create_issue_intake_batch(valid.clone())
        .await
        .unwrap_err();
    assert_eq!(error_code(&error), "ISSUE_BATCH_INELIGIBLE");
    assert!(error.to_string().contains("ISSUE_NOT_OPEN"));

    let untrusted = issue(7, 77, 99, "MEMBER");
    insert_issue(&fixture.db, "project", "acme/widgets", 42, &untrusted).await;
    let untrusted_items = batch_items(&untrusted);
    let untrusted_input = NewIssueIntakeBatch {
        project_id: "project",
        policy_id: &policy.id,
        expected_policy_revision: policy.revision,
        expected_policy_hash: &policy.content_hash,
        items: &untrusted_items,
        max_total_cost_micros: 200_000,
        expires_at: Box::leak(
            (Utc::now() + Duration::days(1))
                .to_rfc3339()
                .into_boxed_str(),
        ),
        acknowledge: true,
        reason: "select exact issue",
        idempotency_key: Box::leak(uuid::Uuid::new_v4().to_string().into_boxed_str()),
        principal_id: "operator-token",
        principal_label: None,
    };
    let error = fixture
        .db
        .create_issue_intake_batch(untrusted_input.clone())
        .await
        .unwrap_err();
    assert_eq!(error_code(&error), "ISSUE_BATCH_INELIGIBLE");
    assert!(error.to_string().contains("AUTHOR_NOT_ALLOWED"));
    let _ = untrusted_input;
    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.path);
}

#[tokio::test]
async fn policy_edit_revokes_old_batch_and_is_cross_project_isolated() {
    let fixture = fixture("project", "widgets", 42, false).await;
    let policy = create_policy(&fixture).await;
    let snapshot_issue = issue(5, 55, 7, "MEMBER");
    insert_issue(&fixture.db, "project", "acme/widgets", 42, &snapshot_issue).await;
    let items = batch_items(&snapshot_issue);
    let batch = fixture
        .db
        .create_issue_intake_batch(NewIssueIntakeBatch {
            project_id: "project",
            policy_id: &policy.id,
            expected_policy_revision: policy.revision,
            expected_policy_hash: &policy.content_hash,
            items: &items,
            max_total_cost_micros: 100_000,
            expires_at: Box::leak(
                (Utc::now() + Duration::days(1))
                    .to_rfc3339()
                    .into_boxed_str(),
            ),
            acknowledge: true,
            reason: "select exact issue",
            idempotency_key: "batch-cross",
            principal_id: "operator-token",
            principal_label: None,
        })
        .await
        .unwrap();
    let error = fixture
        .db
        .revoke_issue_intake_batch("other", &batch.batch.id, "wrong", "operator-token", None)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::NotFound));
    let edited = fixture
        .db
        .create_issue_intake_policy(NewIssueIntakePolicy {
            idempotency_key: "issue-policy-2",
            reason: "edit policy",
            ..policy_input(&fixture)
        })
        .await
        .unwrap();
    assert_eq!(edited.policy.revision, 2);
    let revoked = fixture
        .db
        .issue_intake_state("project")
        .await
        .unwrap()
        .batches
        .into_iter()
        .find(|state| state.batch.id == batch.batch.id)
        .unwrap();
    assert_eq!(revoked.batch.status, "REVOKED");
    let delegation = fixture
        .db
        .standing_delegation(&batch.batch.delegation_id)
        .await
        .unwrap();
    assert!(delegation.revoked_at.is_some());
    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.path);
}

fn error_code(error: &Error) -> &str {
    match error {
        Error::IntakeConflict { code, .. } => code,
        _ => "",
    }
}
