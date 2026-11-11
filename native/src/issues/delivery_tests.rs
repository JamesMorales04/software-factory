use super::*;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;

use crate::db::{
    Database, ExecutionProfileRecord, NewExecutionPolicyGrant, NewProjectConfig,
    NewProjectGitHubBinding, ProviderSettingsWrite,
};
use crate::execution_policy::{OperationLimits, ProgressPolicy};
use crate::execution_profile::{
    AutoStartPolicy, ExecutionProfileCategory, ExecutionProfileContent, ExecutionProfileRoles,
    ExecutionRoleRoute, ReasoningEffort,
};
use crate::github::{GitHubClient, GitHubPullRequest};
use crate::github_sync::GitHubCredential;
use crate::intake::IntakeMode;
use crate::issues::{IssueDeliveryState, IssueLinkState, IssueRole, NewIssueSelection};
use crate::lead::{Capability, QualityProfile};
use crate::telemetry::SecretValue;

const TOKEN: &str = "ghp_test_token_1234567890";

fn temp_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{prefix}-{}.sqlite", uuid::Uuid::new_v4()))
}

fn route() -> ExecutionRoleRoute {
    ExecutionRoleRoute {
        provider: "opencode-go".into(),
        model: "deepseek-v4.1-flash".into(),
        reasoning: Some(ReasoningEffort::Medium),
    }
}

fn profile_content() -> ExecutionProfileContent {
    ExecutionProfileContent {
        name: "Balanced".into(),
        description: "issue delivery test".into(),
        category: ExecutionProfileCategory::Balanced,
        roles: ExecutionProfileRoles {
            lead: route(),
            implementer: route(),
            semantic_reviewer: route(),
        },
        operations: OperationLimits {
            model_request_timeout_secs: std::num::NonZeroU32::new(300).unwrap(),
            tool_call_timeout_secs: std::num::NonZeroU32::new(600).unwrap(),
            max_request_bytes: std::num::NonZeroU32::new(2 * 1024 * 1024).unwrap(),
            max_response_bytes: std::num::NonZeroU32::new(2 * 1024 * 1024).unwrap(),
            max_generation_tokens: std::num::NonZeroU32::new(8_192).unwrap(),
            max_in_flight_operations: std::num::NonZeroU32::new(4).unwrap(),
        },
        progress: ProgressPolicy {
            diagnose_after_stagnant_checkpoints: std::num::NonZeroU32::new(2).unwrap(),
            escalate_after_unsuccessful_replans: std::num::NonZeroU32::new(1).unwrap(),
            observation_timeout_secs: std::num::NonZeroU32::new(300).unwrap(),
            autonomy: Default::default(),
        },
        auto_start: AutoStartPolicy {
            enabled: false,
            risk_classes: Vec::new(),
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
    profile: ExecutionProfileRecord,
}

async fn fixture() -> Fixture {
    let path = temp_path("factory-issue-delivery");
    let db = Database::open(&path).await.unwrap();
    let base = "b".repeat(40);
    sqlx::query::query(
        "INSERT INTO projects(id,repository,base_commit,created_at,identity)
         VALUES ('project','/fixture',?1,?2,'fixture')",
    )
    .bind(&base)
    .bind(Utc::now().to_rfc3339())
    .execute(db.test_pool())
    .await
    .unwrap();
    db.test_ensure_project_config("project").await.unwrap();
    let config = db
        .update_project_config(
            "project",
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
        .create_execution_profile(&profile_content(), "owner")
        .await
        .unwrap();
    let _grant = db
        .create_execution_policy_grant(NewExecutionPolicyGrant {
            project_id: "project",
            profile_id: &profile.id,
            expected_revision: profile.revision,
            expected_revision_hash: &profile.content_hash,
            project_config_hash: &config.config_hash,
            acknowledge: true,
            autonomous: false,
            subscription_confirmed: false,
            idempotency_key: "issue-delivery-policy",
            principal_id: "operator-token",
            principal_label: None,
        })
        .await
        .unwrap();
    let now = Utc::now().to_rfc3339();
    sqlx::query::query(
        "INSERT INTO github_connections
         (id,login,scopes_json,scopes_reported,credential_ciphertext,credential_nonce,
          credential_fingerprint,enabled,revision,verified_at,revoked_at,rate_limited_until,
          created_at,updated_at)
         VALUES ('default','octo','[\"repo\"]',1,'ciphertext','nonce','fingerprint',1,1,?1,NULL,NULL,?1,?1)",
    )
    .bind(&now)
    .execute(db.test_pool())
    .await
    .unwrap();
    db.bind_project_github(
        "project",
        NewProjectGitHubBinding {
            owner: "acme",
            repo: "widgets",
            default_branch: "main",
            repository_github_id: Some(42),
        },
        "tester",
    )
    .await
    .unwrap();
    for (number, github_id, title) in [(7, 70, "Fix widget"), (8, 80, "Related widget")] {
        sqlx::query::query(
            "INSERT INTO github_issues
             (project_id,repo_full_name,number,github_id,title,body,body_truncated,state,
              state_reason,labels_json,author,author_id,author_association,html_url,comments,
              created_at,updated_at,closed_at,synced_at,sync_mark,content_hash,
              repository_github_id,is_pull_request)
             VALUES ('project','acme/widgets',?1,?2,?3,'body',0,'open',NULL,'[]','octo',1,
                     'MEMBER','https://github.com/acme/widgets/issues/'||?1,0,?4,?4,NULL,?4,'sync',
                     'hash',42,0)",
        )
        .bind(number)
        .bind(github_id)
        .bind(title)
        .bind(&now)
        .execute(db.test_pool())
        .await
        .unwrap();
    }
    Fixture { path, db, profile }
}

async fn create_request(fixture: &Fixture, key: &str, numbers: &[(u64, IssueRole)]) -> String {
    let selections = numbers
        .iter()
        .map(|(number, role)| NewIssueSelection {
            owner: "acme".into(),
            repo: "widgets".into(),
            number: *number,
            role: *role,
        })
        .collect::<Vec<_>>();
    fixture
        .db
        .create_intake_request(
            "project",
            "Deliver the selected issues.",
            Some(&fixture.profile.id),
            IntakeMode::ReviewPlan,
            key,
            "operator-token",
            &selections,
        )
        .await
        .unwrap()
        .0
        .id
}

struct SeededCampaign {
    campaign_id: String,
    candidate_id: String,
}

async fn seed_campaign(db: &Database, request_id: &str, key: &str) -> SeededCampaign {
    let request = db.intake_request(request_id).await.unwrap();
    let now = Utc::now().to_rfc3339();
    let campaign_id = format!("campaign-{key}");
    let run_id = format!("run-{key}");
    let attempt_id = format!("attempt-{key}");
    let candidate_id = format!("candidate-{key}");
    sqlx::query::query("INSERT INTO campaigns VALUES (?1,?2,'AUTHORIZED',?3)")
        .bind(&campaign_id)
        .bind(&request.action_id)
        .bind(&now)
        .execute(db.test_pool())
        .await
        .unwrap();
    sqlx::query::query("UPDATE intake_requests SET campaign_id=?1 WHERE id=?2")
        .bind(&campaign_id)
        .bind(&request.id)
        .execute(db.test_pool())
        .await
        .unwrap();
    sqlx::query::query("UPDATE lead_requests SET campaign_id=?1,state='RUNNING' WHERE id=?2")
        .bind(&campaign_id)
        .bind(&request.lead_request_id)
        .execute(db.test_pool())
        .await
        .unwrap();
    sqlx::query::query("INSERT INTO runs VALUES (?1,?2,'RUNNING',?3)")
        .bind(&run_id)
        .bind(&campaign_id)
        .bind(&now)
        .execute(db.test_pool())
        .await
        .unwrap();
    sqlx::query::query(
        "INSERT INTO attempts(id,run_id,status,created_at) VALUES (?1,?2,'RUNNING',?3)",
    )
    .bind(&attempt_id)
    .bind(&run_id)
    .bind(&now)
    .execute(db.test_pool())
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO candidates
         (id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at)
         VALUES (?1,?2,?3,?4,?5,'worker',?6)",
    )
    .bind(&candidate_id)
    .bind(&attempt_id)
    .bind("a".repeat(64))
    .bind("c".repeat(40))
    .bind("d".repeat(40))
    .bind(&now)
    .execute(db.test_pool())
    .await
    .unwrap();
    sqlx::query::query("INSERT INTO gate_decisions VALUES (?1,'ACCEPTED','[]',?2)")
        .bind(&candidate_id)
        .bind(&now)
        .execute(db.test_pool())
        .await
        .unwrap();
    SeededCampaign {
        campaign_id,
        candidate_id,
    }
}

async fn map_work_unit(
    db: &Database,
    request_id: &str,
    campaign: &SeededCampaign,
    number: u64,
    unit: &str,
) {
    let request = db.intake_request(request_id).await.unwrap();
    let now = Utc::now().to_rfc3339();
    sqlx::query::query(
        "UPDATE issue_links SET work_unit_id=?1 WHERE request_id=?2 AND issue_number=?3",
    )
    .bind(unit)
    .bind(request_id)
    .bind(number as i64)
    .execute(db.test_pool())
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO lead_tasks
         (task_id,campaign_id,request_id,unit_id,sequence,status,base_commit,spec_json,
          spec_sha256,depends_on_json,run_id,attempt_id,candidate_id,created_at,updated_at)
         VALUES (?1,?2,?3,?4,0,'RUNNING',?5,'{}','spec','[]',?6,?7,?8,?9,?9)",
    )
    .bind(format!("task-{unit}"))
    .bind(&campaign.campaign_id)
    .bind(&request.lead_request_id)
    .bind(unit)
    .bind("b".repeat(40))
    .bind(format!("run-{}", campaign.campaign_id))
    .bind(format!("attempt-{}", campaign.campaign_id))
    .bind(&campaign.candidate_id)
    .bind(&now)
    .execute(db.test_pool())
    .await
    .unwrap();
}

#[derive(Default)]
struct FakeState {
    write_permission: bool,
    unauthorized: bool,
    issue_closed: HashMap<u64, bool>,
    comments: Vec<(u64, String, u64)>,
    comment_create_calls: u32,
    successful_comments: u32,
    fail_next_comment_after_store: bool,
    truncated_comments: bool,
    fail_close: bool,
    close_patch_calls: u32,
    successful_closes: u32,
    next_id: u64,
    labels: Vec<String>,
}

#[derive(Clone)]
struct Fake {
    state: Arc<Mutex<FakeState>>,
}

fn issue_payload(number: u64, closed: bool) -> serde_json::Value {
    serde_json::json!({
        "id": 1000 + number,
        "number": number,
        "title": format!("Issue {number}"),
        "body": "body",
        "state": if closed { "closed" } else { "open" },
        "state_reason": if closed { serde_json::Value::String("completed".into()) } else { serde_json::Value::Null },
        "labels": [],
        "user": {"id": 1, "login": "octo"},
        "author_association": "MEMBER",
        "html_url": format!("https://github.com/acme/widgets/issues/{number}"),
        "comments": 0,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "closed_at": if closed { serde_json::Value::String("2026-01-02T00:00:00Z".into()) } else { serde_json::Value::Null }
    })
}

async fn repository_handler(State(fake): State<Fake>, _headers: HeaderMap) -> Response {
    let state = fake.state.lock().unwrap();
    if state.unauthorized {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let write = state.write_permission;
    Json(serde_json::json!({
        "id": 42,
        "full_name": "acme/widgets",
        "private": false,
        "default_branch": "main",
        "html_url": "https://github.com/acme/widgets",
        "pushed_at": null,
        "permissions": {
            "admin": false,
            "maintain": false,
            "push": write,
            "triage": false,
            "pull": true
        }
    }))
    .into_response()
}

async fn get_issue_handler(
    State(fake): State<Fake>,
    Path((_owner, _repo, number)): Path<(String, String, u64)>,
) -> Response {
    let state = fake.state.lock().unwrap();
    let Some(closed) = state.issue_closed.get(&number).copied() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    Json(issue_payload(number, closed)).into_response()
}

async fn comments_handler(
    State(fake): State<Fake>,
    Path((_owner, _repo, number)): Path<(String, String, u64)>,
) -> Response {
    let state = fake.state.lock().unwrap();
    if state.truncated_comments {
        return Json(
            (1..=100)
                .map(|id| {
                    serde_json::json!({
                        "id": id,
                        "body": format!("unrelated comment {id}"),
                        "user": {"login": "octo"},
                        "html_url": format!("https://github.com/acme/widgets/issues/{number}#issuecomment-{id}"),
                        "created_at": "2026-01-02T00:00:00Z"
                    })
                })
                .collect::<Vec<_>>(),
        )
        .into_response();
    }
    let comments = state
        .comments
        .iter()
        .filter(|(issue, _, _)| *issue == number)
        .map(|(_, body, id)| {
            serde_json::json!({
                "id": id,
                "body": body,
                "user": {"login": "octo"},
                "html_url": format!("https://github.com/acme/widgets/issues/{number}#issuecomment-{id}"),
                "created_at": "2026-01-02T00:00:00Z"
            })
        })
        .collect::<Vec<_>>();
    Json(comments).into_response()
}

async fn create_comment_handler(
    State(fake): State<Fake>,
    Path((_owner, _repo, number)): Path<(String, String, u64)>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let mut state = fake.state.lock().unwrap();
    if !state.issue_closed.contains_key(&number) {
        return StatusCode::NOT_FOUND.into_response();
    }
    state.next_id += 1;
    let id = state.next_id;
    let text = body["body"].as_str().unwrap_or_default().to_owned();
    state.comments.push((number, text, id));
    state.comment_create_calls += 1;
    if state.fail_next_comment_after_store {
        state.fail_next_comment_after_store = false;
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    state.successful_comments += 1;
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": id,
            "body": body["body"],
            "user": {"login": "octo"},
            "html_url": format!("https://github.com/acme/widgets/issues/{number}#issuecomment-{id}"),
            "created_at": "2026-01-02T00:00:00Z"
        })),
    )
        .into_response()
}

async fn close_handler(
    State(fake): State<Fake>,
    Path((_owner, _repo, number)): Path<(String, String, u64)>,
) -> Response {
    let mut state = fake.state.lock().unwrap();
    state.close_patch_calls += 1;
    if state.fail_close {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    state.issue_closed.insert(number, true);
    state.successful_closes += 1;
    Json(issue_payload(number, true)).into_response()
}

async fn labels_handler(
    State(fake): State<Fake>,
    Path((_owner, _repo, _number)): Path<(String, String, u64)>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let mut state = fake.state.lock().unwrap();
    let labels = body["labels"]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .map(|name| serde_json::json!({"name": name, "color": "00ff00"}))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for label in &labels {
        state
            .labels
            .push(label["name"].as_str().unwrap_or_default().to_owned());
    }
    Json(labels).into_response()
}

async fn start_fake(write_permission: bool) -> (String, Fake) {
    let fake = Fake {
        state: Arc::new(Mutex::new(FakeState {
            write_permission,
            issue_closed: HashMap::from([(7, false), (8, false)]),
            ..Default::default()
        })),
    };
    let app = Router::new()
        .route("/repos/{owner}/{repo}", get(repository_handler))
        .route(
            "/repos/{owner}/{repo}/issues/{number}",
            get(get_issue_handler).patch(close_handler),
        )
        .route(
            "/repos/{owner}/{repo}/issues/{number}/comments",
            get(comments_handler).post(create_comment_handler),
        )
        .route(
            "/repos/{owner}/{repo}/issues/{number}/labels",
            post(labels_handler),
        )
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, fake)
}

fn credential() -> GitHubCredential {
    GitHubCredential::new(
        1,
        SecretValue::from_utf8(TOKEN.as_bytes().to_vec()).unwrap(),
    )
    .unwrap()
}

fn pull_request() -> GitHubPullRequest {
    GitHubPullRequest {
        number: 42,
        title: "Candidate 42".into(),
        state: "open".into(),
        draft: false,
        merged: false,
        mergeable: Some(true),
        mergeable_state: "clean".into(),
        head_ref: "candidate/issue".into(),
        head_sha: "c".repeat(40),
        base_ref: "main".into(),
        base_sha: "b".repeat(40),
        author: Some("octo".into()),
        html_url: "https://github.com/acme/widgets/pull/42".into(),
        changed_files: 1,
        additions: 1,
        deletions: 0,
        comments: 0,
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
    }
}

async fn link_for_request(db: &Database, request_id: &str, number: u64) -> IssueLinkRecord {
    db.issue_links_for_request(request_id)
        .await
        .unwrap()
        .into_iter()
        .find(|link| link.issue_number == number)
        .unwrap()
}

#[tokio::test]
async fn comments_once_and_closes_only_after_merge() {
    let fixture = fixture().await;
    let request_id = create_request(&fixture, "delivery-single", &[(7, IssueRole::Primary)]).await;
    let campaign = seed_campaign(&fixture.db, &request_id, "single").await;
    let (url, fake) = start_fake(true).await;
    let github = GitHubClient::with_base_url(&url, true).unwrap();
    let credential = credential();
    let service = IssueDeliveryService::new(&fixture.db, &github, &credential)
        .await
        .unwrap();

    let pr = pull_request();
    let opened = service
        .on_pr_opened(&campaign.campaign_id, &campaign.candidate_id, &pr)
        .await
        .unwrap();
    assert_eq!(opened.attempted, 1);
    assert_eq!(opened.applied, 1);
    {
        let state = fake.state.lock().unwrap();
        assert_eq!(state.successful_comments, 1);
        assert_eq!(state.comments.len(), 1);
    }
    assert!(
        !*fake.state.lock().unwrap().issue_closed.get(&7).unwrap(),
        "opening a pull request must not close the issue"
    );
    let link = link_for_request(&fixture.db, &request_id, 7).await;
    assert_eq!(link.state, IssueLinkState::InReview);
    assert_eq!(link.delivery_state, IssueDeliveryState::Commented);
    assert_eq!(link.pull_request_number, Some(42));

    service
        .on_pr_opened(&campaign.campaign_id, &campaign.candidate_id, &pr)
        .await
        .unwrap();
    assert_eq!(fake.state.lock().unwrap().comments.len(), 1);

    let merged = service
        .on_merge_succeeded(
            &campaign.campaign_id,
            &campaign.candidate_id,
            &"e".repeat(40),
        )
        .await
        .unwrap();
    assert_eq!(merged.applied, 1);
    assert_eq!(merged.failed, 0);
    let link = link_for_request(&fixture.db, &request_id, 7).await;
    assert_eq!(link.state, IssueLinkState::Done);
    assert_eq!(link.delivery_state, IssueDeliveryState::Closed);
    assert_eq!(link.merge_sha.as_deref(), Some("e".repeat(40).as_str()));
    assert!(*fake.state.lock().unwrap().issue_closed.get(&7).unwrap());
    assert_eq!(fake.state.lock().unwrap().successful_closes, 1);

    // A later blocking candidate in the same Campaign must not rewrite the
    // board state of the already-merged exact candidate.
    let later = "2099-01-01T00:00:00Z";
    sqlx::query::query("INSERT INTO runs VALUES ('run-later',?1,'RUNNING',?2)")
        .bind(&campaign.campaign_id)
        .bind(later)
        .execute(fixture.db.test_pool())
        .await
        .unwrap();
    sqlx::query::query(
        "INSERT INTO attempts(id,run_id,status,created_at) VALUES ('attempt-later','run-later','RUNNING',?1)",
    )
    .bind(later)
    .execute(fixture.db.test_pool())
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO candidates
         (id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at)
         VALUES ('candidate-later','attempt-later',?1,?2,?3,'worker',?4)",
    )
    .bind("9".repeat(64))
    .bind("8".repeat(40))
    .bind("7".repeat(40))
    .bind(later)
    .execute(fixture.db.test_pool())
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO gate_decisions VALUES ('candidate-later','BLOCKED','[\"later\"]',?1)",
    )
    .bind(later)
    .execute(fixture.db.test_pool())
    .await
    .unwrap();
    let link = link_for_request(&fixture.db, &request_id, 7).await;
    assert_eq!(
        link.state,
        IssueLinkState::Done,
        "a later blocking candidate must not change the exact merged issue link"
    );

    service
        .on_merge_succeeded(
            &campaign.campaign_id,
            &campaign.candidate_id,
            &"e".repeat(40),
        )
        .await
        .unwrap();
    assert_eq!(fake.state.lock().unwrap().successful_closes, 1);
    assert_eq!(fake.state.lock().unwrap().comments.len(), 1);

    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.path);
}

#[tokio::test]
async fn partial_issue_comments_and_stays_open() {
    let fixture = fixture().await;
    let request_id = create_request(
        &fixture,
        "delivery-partial",
        &[(7, IssueRole::Primary), (8, IssueRole::Primary)],
    )
    .await;
    let campaign = seed_campaign(&fixture.db, &request_id, "partial").await;
    map_work_unit(&fixture.db, &request_id, &campaign, 7, "unit-a").await;
    map_work_unit(&fixture.db, &request_id, &campaign, 8, "unit-b").await;
    let (url, fake) = start_fake(true).await;
    let github = GitHubClient::with_base_url(&url, true).unwrap();
    let credential = credential();
    let service = IssueDeliveryService::new(&fixture.db, &github, &credential)
        .await
        .unwrap();

    let pr = pull_request();
    service
        .on_pr_opened(&campaign.campaign_id, &campaign.candidate_id, &pr)
        .await
        .unwrap();
    assert_eq!(fake.state.lock().unwrap().successful_comments, 2);

    let merged = service
        .on_merge_succeeded(
            &campaign.campaign_id,
            &campaign.candidate_id,
            &"f".repeat(40),
        )
        .await
        .unwrap();
    assert_eq!(merged.applied, 1);
    assert_eq!(merged.partial, 1);
    assert_eq!(merged.failed, 0);

    let resolved = link_for_request(&fixture.db, &request_id, 7).await;
    assert_eq!(resolved.state, IssueLinkState::Done);
    assert_eq!(resolved.delivery_state, IssueDeliveryState::Closed);
    let partial = link_for_request(&fixture.db, &request_id, 8).await;
    assert_eq!(partial.state, IssueLinkState::NeedsAttention);
    assert_eq!(partial.delivery_state, IssueDeliveryState::Partial);

    {
        let state = fake.state.lock().unwrap();
        assert!(*state.issue_closed.get(&7).unwrap());
        assert!(!*state.issue_closed.get(&8).unwrap());
        assert_eq!(state.successful_closes, 1);
        let issue_eight = state
            .comments
            .iter()
            .filter(|(number, _, _)| *number == 8)
            .collect::<Vec<_>>();
        assert_eq!(issue_eight.len(), 2);
        assert!(
            issue_eight
                .iter()
                .any(|(_, body, _)| body.contains("PARTIAL"))
        );
    }

    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.path);
}

#[tokio::test]
async fn retry_reuses_the_marker_and_does_not_duplicate_comments_or_closes() {
    let fixture = fixture().await;
    let request_id = create_request(&fixture, "delivery-retry", &[(7, IssueRole::Primary)]).await;
    let campaign = seed_campaign(&fixture.db, &request_id, "retry").await;
    let (url, fake) = start_fake(true).await;
    fake.state.lock().unwrap().fail_next_comment_after_store = true;
    fake.state.lock().unwrap().fail_close = true;
    let github = GitHubClient::with_base_url(&url, true).unwrap();
    let credential = credential();
    let service = IssueDeliveryService::new(&fixture.db, &github, &credential)
        .await
        .unwrap();

    let pr = pull_request();
    let opened = service
        .on_pr_opened(&campaign.campaign_id, &campaign.candidate_id, &pr)
        .await
        .unwrap();
    assert_eq!(opened.failed, 0);
    {
        let state = fake.state.lock().unwrap();
        assert_eq!(
            state.comment_create_calls, 1,
            "the retry must find the marker instead of posting twice"
        );
        assert_eq!(state.comments.len(), 1);
    }

    let merged = service
        .on_merge_succeeded(
            &campaign.campaign_id,
            &campaign.candidate_id,
            &"1".repeat(40),
        )
        .await
        .unwrap();
    assert_eq!(merged.failed, 1);
    let link = link_for_request(&fixture.db, &request_id, 7).await;
    assert_eq!(link.state, IssueLinkState::Done);
    assert_eq!(link.delivery_state, IssueDeliveryState::Failed);
    let close_event = fixture
        .db
        .issue_delivery_events_for_link(&link.id)
        .await
        .unwrap()
        .into_iter()
        .find(|event| event.action == IssueDeliveryAction::Close)
        .unwrap();
    assert_eq!(close_event.status, IssueDeliveryEventStatus::Failed);

    fake.state.lock().unwrap().fail_close = false;
    let retried = service.retry_event(&close_event.id, "owner").await.unwrap();
    assert_eq!(retried.applied, 1);
    {
        let state = fake.state.lock().unwrap();
        assert_eq!(state.successful_closes, 1);
        assert_eq!(
            state.comments.len(),
            1,
            "retrying close must not post another comment"
        );
    }
    let link = link_for_request(&fixture.db, &request_id, 7).await;
    assert_eq!(link.delivery_state, IssueDeliveryState::Closed);
    assert_eq!(link.state, IssueLinkState::Done);

    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.path);
}

#[tokio::test]
async fn read_only_token_is_typed_blocked_before_any_write() {
    let fixture = fixture().await;
    let request_id =
        create_request(&fixture, "delivery-readonly", &[(7, IssueRole::Primary)]).await;
    let campaign = seed_campaign(&fixture.db, &request_id, "readonly").await;
    let (url, fake) = start_fake(false).await;
    let github = GitHubClient::with_base_url(&url, true).unwrap();
    let credential = credential();
    let service = IssueDeliveryService::new(&fixture.db, &github, &credential)
        .await
        .unwrap();

    let error = service
        .on_pr_opened(
            &campaign.campaign_id,
            &campaign.candidate_id,
            &pull_request(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind(), "GITHUB_WRITE_PERMISSION_REQUIRED");
    assert!(!error.retryable());
    {
        let state = fake.state.lock().unwrap();
        assert_eq!(state.comment_create_calls, 0);
        assert_eq!(state.close_patch_calls, 0);
    }
    let link = link_for_request(&fixture.db, &request_id, 7).await;
    assert_eq!(link.state, IssueLinkState::NeedsAttention);
    assert_eq!(link.delivery_state, IssueDeliveryState::Blocked);
    assert_eq!(link.pull_request_number, Some(42));
    assert_eq!(
        link.latest_error.as_ref().unwrap().kind,
        "GITHUB_WRITE_PERMISSION_REQUIRED"
    );
    let failure = fixture
        .db
        .issue_delivery_events_for_link(&link.id)
        .await
        .unwrap()
        .into_iter()
        .find(|event| event.action == IssueDeliveryAction::Failure)
        .unwrap();

    // Correcting the credential and retrying the failure summary re-runs the
    // original delivery hook; it does not invent a success locally.
    fake.state.lock().unwrap().write_permission = true;
    let retried = service.retry_event(&failure.id, "owner").await.unwrap();
    assert_eq!(retried.applied, 1);
    assert_eq!(fake.state.lock().unwrap().successful_comments, 1);
    let link = link_for_request(&fixture.db, &request_id, 7).await;
    assert_eq!(link.state, IssueLinkState::InReview);
    assert_eq!(link.delivery_state, IssueDeliveryState::Commented);
    assert!(link.latest_error.is_none());

    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.path);
}

#[tokio::test]
async fn unauthorized_token_is_typed_and_writes_nothing() {
    let fixture = fixture().await;
    let request_id = create_request(
        &fixture,
        "delivery-unauthorized",
        &[(7, IssueRole::Primary)],
    )
    .await;
    let campaign = seed_campaign(&fixture.db, &request_id, "unauthorized").await;
    let (url, fake) = start_fake(true).await;
    fake.state.lock().unwrap().unauthorized = true;
    let github = GitHubClient::with_base_url(&url, true).unwrap();
    let credential = credential();
    let service = IssueDeliveryService::new(&fixture.db, &github, &credential)
        .await
        .unwrap();

    let error = service
        .on_merge_succeeded(
            &campaign.campaign_id,
            &campaign.candidate_id,
            &"2".repeat(40),
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind(), "GITHUB_UNAUTHORIZED");
    {
        let state = fake.state.lock().unwrap();
        assert_eq!(state.successful_comments, 0);
        assert_eq!(state.successful_closes, 0);
    }

    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.path);
}

#[tokio::test]
async fn truncated_comment_window_blocks_without_posting() {
    let fixture = fixture().await;
    let request_id =
        create_request(&fixture, "delivery-truncated", &[(7, IssueRole::Primary)]).await;
    let campaign = seed_campaign(&fixture.db, &request_id, "truncated").await;
    let (url, fake) = start_fake(true).await;
    fake.state.lock().unwrap().truncated_comments = true;
    let github = GitHubClient::with_base_url(&url, true).unwrap();
    let credential = credential();
    let service = IssueDeliveryService::new(&fixture.db, &github, &credential)
        .await
        .unwrap();

    let outcome = service
        .on_pr_opened(
            &campaign.campaign_id,
            &campaign.candidate_id,
            &pull_request(),
        )
        .await
        .unwrap();
    assert_eq!(outcome.failed, 1);
    assert_eq!(outcome.applied, 0);
    {
        let state = fake.state.lock().unwrap();
        assert_eq!(
            state.comment_create_calls, 0,
            "Core must not post when the marker may be outside the bounded window"
        );
    }
    let link = link_for_request(&fixture.db, &request_id, 7).await;
    assert_eq!(
        link.latest_error.as_ref().unwrap().kind,
        "ISSUE_COMMENT_WINDOW_TRUNCATED"
    );
    let comment = fixture
        .db
        .issue_delivery_events_for_link(&link.id)
        .await
        .unwrap()
        .into_iter()
        .find(|event| event.action == IssueDeliveryAction::Comment)
        .unwrap();
    assert_eq!(comment.status, IssueDeliveryEventStatus::Blocked);

    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.path);
}
