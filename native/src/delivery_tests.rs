#![allow(clippy::await_holding_lock)]

use super::*;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;

use crate::db::{Database, NewDeliveryAttempt, NewDeliveryAttemptEvent, NewProjectGitHubBinding};
use crate::github::GitHubClient;
use crate::github_sync::GitHubCredential;
use crate::issues::{IssueDeliveryOutcome, IssueDeliveryState, IssueLinkState};
use crate::lead::RiskClass;
use crate::telemetry::SecretValue;

const TOKEN: &str = "ghp_test_delivery_token_1234567890";
const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn temp_root(prefix: &str) -> PathBuf {
    let root = PathBuf::from("/tmp/opencode");
    fs::create_dir_all(&root).unwrap();
    root.join(format!("{prefix}-{}", uuid::Uuid::new_v4()))
}

fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn init_repo() -> RepoFixture {
    let root = temp_root("factory-delivery-repo");
    fs::create_dir_all(&root).unwrap();
    let remote = root.join("remote.git");
    fs::create_dir_all(&remote).unwrap();
    let output = Command::new("git")
        .arg("init")
        .arg("--bare")
        .arg(&remote)
        .output()
        .unwrap();
    assert!(output.status.success());
    let work = root.join("work");
    fs::create_dir_all(&work).unwrap();
    run_git(&work, &["init", "-b", "main"]);
    run_git(&work, &["config", "user.email", "fixture@example.test"]);
    run_git(&work, &["config", "user.name", "Factory Fixture"]);
    fs::write(work.join("README.md"), "# Fixture\n").unwrap();
    run_git(&work, &["add", "README.md"]);
    run_git(&work, &["commit", "-m", "base"]);
    let base = run_git(&work, &["rev-parse", "HEAD"]);
    fs::create_dir_all(work.join("src")).unwrap();
    fs::write(work.join("src/lib.rs"), "pub fn value() -> u32 { 1 }\n").unwrap();
    run_git(&work, &["add", "src/lib.rs"]);
    run_git(&work, &["commit", "-m", "candidate"]);
    let candidate = run_git(&work, &["rev-parse", "HEAD"]);
    let tree = run_git(&work, &["rev-parse", "HEAD^{tree}"]);
    let identity = crate::repository::inspect(&work, &[]).unwrap().identity;
    RepoFixture {
        root,
        work,
        remote,
        base,
        candidate,
        tree,
        identity,
    }
}

struct RepoFixture {
    #[allow(dead_code)]
    root: PathBuf,
    work: PathBuf,
    remote: PathBuf,
    base: String,
    candidate: String,
    tree: String,
    identity: String,
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

struct FakeState {
    base_sha: String,
    candidate_sha: String,
    merge_sha: String,
    candidate_branch_sha: String,
    default_branch_sha: String,
    release_branch_sha: String,
    pr_base_ref: String,
    pr_open: bool,
    pr_merged: bool,
    pr_state: String,
    new_pr_state: String,
    pr_head_sha: String,
    create_head_sha: Option<String>,
    pr_base_sha: String,
    mergeable: Option<bool>,
    mergeable_state: String,
    check_status: String,
    check_conclusion: String,
    post_merge_status: String,
    post_merge_conclusion: String,
    protection: bool,
    protection_strict: bool,
    allow_force_push: bool,
    allow_deletions: bool,
    push_permission: bool,
    issue_closed: bool,
    issue_comments: Vec<serde_json::Value>,
    issue_comment_calls: u32,
    issue_close_calls: u32,
    merge_calls: AtomicU32,
    create_calls: AtomicU32,
    list_calls: AtomicU32,
}

impl FakeState {
    fn new(repo: &RepoFixture) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            base_sha: repo.base.clone(),
            candidate_sha: repo.candidate.clone(),
            merge_sha: "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".into(),
            candidate_branch_sha: repo.candidate.clone(),
            default_branch_sha: repo.base.clone(),
            release_branch_sha: repo.base.clone(),
            pr_base_ref: "main".into(),
            pr_open: false,
            pr_merged: false,
            pr_state: "open".into(),
            new_pr_state: "open".into(),
            pr_head_sha: repo.candidate.clone(),
            create_head_sha: None,
            pr_base_sha: repo.base.clone(),
            mergeable: Some(true),
            mergeable_state: "clean".into(),
            check_status: "completed".into(),
            check_conclusion: "success".into(),
            post_merge_status: "completed".into(),
            post_merge_conclusion: "success".into(),
            protection: true,
            protection_strict: true,
            allow_force_push: false,
            allow_deletions: false,
            push_permission: true,
            issue_closed: false,
            issue_comments: Vec::new(),
            issue_comment_calls: 0,
            issue_close_calls: 0,
            merge_calls: AtomicU32::new(0),
            create_calls: AtomicU32::new(0),
            list_calls: AtomicU32::new(0),
        }))
    }
}

#[derive(Clone)]
struct Fake {
    state: Arc<Mutex<FakeState>>,
}

fn pull_request_payload(state: &FakeState) -> serde_json::Value {
    serde_json::json!({
        "number": 42,
        "title": "Factory candidate",
        "state": state.pr_state,
        "draft": false,
        "merged": state.pr_merged,
        "merge_commit_sha": if state.pr_merged { serde_json::Value::String(state.merge_sha.clone()) } else { serde_json::Value::Null },
        "mergeable": state.mergeable,
        "mergeable_state": state.mergeable_state,
        "head": {"ref": format!("factory/campaign-campaign-1/candidate-{}", state.candidate_sha), "sha": state.pr_head_sha},
        "base": {"ref": state.pr_base_ref, "sha": state.pr_base_sha},
        "user": {"id": 1, "login": "octo"},
        "html_url": "https://github.com/acme/widgets/pull/42",
        "changed_files": 1,
        "additions": 1,
        "deletions": 0,
        "comments": 0,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z"
    })
}

async fn repository_handler(State(fake): State<Fake>) -> Response {
    let state = fake.state.lock().unwrap();
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
            "push": state.push_permission,
            "triage": false,
            "pull": true
        }
    }))
    .into_response()
}

async fn branch_or_protection_handler(
    State(fake): State<Fake>,
    AxumPath((_owner, _repo, rest)): AxumPath<(String, String, String)>,
) -> Response {
    if let Some(branch) = rest.strip_suffix("/protection") {
        return protection_response(&fake, branch);
    }
    let branch = rest;
    let state = fake.state.lock().unwrap();
    let sha = if branch == "main" {
        state.default_branch_sha.clone()
    } else if branch == "release" {
        state.release_branch_sha.clone()
    } else {
        state.candidate_branch_sha.clone()
    };
    if sha.is_empty() {
        return StatusCode::NOT_FOUND.into_response();
    }
    Json(serde_json::json!({"name": branch, "commit": {"sha": sha}})).into_response()
}

fn protection_response(fake: &Fake, _branch: &str) -> Response {
    let state = fake.state.lock().unwrap();
    if !state.protection {
        return StatusCode::NOT_FOUND.into_response();
    }
    Json(serde_json::json!({
        "required_status_checks": {
            "strict": state.protection_strict,
            "contexts": ["ci"],
            "checks": [{"context": "ci", "app_id": null}]
        },
        "enforce_admins": {"enabled": false},
        "required_pull_request_reviews": null,
        "allow_force_pushes": {"enabled": state.allow_force_push},
        "allow_deletions": {"enabled": state.allow_deletions},
        "required_conversation_resolution": {"enabled": false}
    }))
    .into_response()
}

async fn list_pulls_handler(
    State(fake): State<Fake>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let state = fake.state.lock().unwrap();
    state.list_calls.fetch_add(1, Ordering::SeqCst);
    if query.get("state").map(String::as_str) == Some("open")
        && state.pr_open
        && state.pr_state == "open"
    {
        return Json(serde_json::json!([pull_request_payload(&state)])).into_response();
    }
    Json(serde_json::json!([])).into_response()
}

async fn create_pull_handler(
    State(fake): State<Fake>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let mut state = fake.state.lock().unwrap();
    state.create_calls.fetch_add(1, Ordering::SeqCst);
    state.pr_open = true;
    state.pr_state = state.new_pr_state.clone();
    state.pr_merged = false;
    state.pr_head_sha = state
        .create_head_sha
        .clone()
        .unwrap_or_else(|| state.candidate_sha.clone());
    state.pr_base_ref = body
        .get("base")
        .and_then(|value| value.as_str())
        .unwrap_or("main")
        .to_owned();
    state.pr_base_sha = if state.pr_base_ref == "release" {
        state.release_branch_sha.clone()
    } else {
        state.default_branch_sha.clone()
    };
    Json(pull_request_payload(&state)).into_response()
}

async fn get_pull_handler(State(fake): State<Fake>) -> Response {
    let state = fake.state.lock().unwrap();
    if !state.pr_open {
        return StatusCode::NOT_FOUND.into_response();
    }
    Json(pull_request_payload(&state)).into_response()
}

async fn merge_handler(State(fake): State<Fake>, Json(body): Json<serde_json::Value>) -> Response {
    let mut state = fake.state.lock().unwrap();
    state.merge_calls.fetch_add(1, Ordering::SeqCst);
    let expected = body.get("sha").and_then(|value| value.as_str());
    if expected != Some(state.candidate_sha.as_str()) {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            Json(serde_json::json!({"message": "head changed"})),
        )
            .into_response();
    }
    if body.get("merge_method").and_then(|value| value.as_str()) != Some("squash") {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            Json(serde_json::json!({"message": "squash only"})),
        )
            .into_response();
    }
    state.pr_merged = true;
    state.pr_state = "closed".into();
    state.default_branch_sha = state.merge_sha.clone();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "merged": true,
            "sha": state.merge_sha,
            "message": "merged"
        })),
    )
        .into_response()
}

async fn check_runs_handler(
    State(fake): State<Fake>,
    AxumPath((_owner, _repo, sha)): AxumPath<(String, String, String)>,
) -> Response {
    let state = fake.state.lock().unwrap();
    let (name, status, conclusion, id) = if sha == state.merge_sha {
        (
            "post-merge",
            state.post_merge_status.clone(),
            state.post_merge_conclusion.clone(),
            2,
        )
    } else {
        (
            "ci",
            state.check_status.clone(),
            state.check_conclusion.clone(),
            1,
        )
    };
    Json(serde_json::json!({
        "total_count": 1,
        "check_runs": [{
            "id": id,
            "name": name,
            "status": status,
            "conclusion": conclusion,
            "details_url": "",
            "started_at": null,
            "completed_at": null,
            "app": {"slug": "fixture"}
        }]
    }))
    .into_response()
}

async fn status_handler(
    State(fake): State<Fake>,
    AxumPath((_owner, _repo, sha)): AxumPath<(String, String, String)>,
) -> Response {
    let state = fake.state.lock().unwrap();
    let combined = if sha == state.merge_sha
        || (state.check_status == "completed" && state.check_conclusion == "success")
    {
        "success"
    } else if state.check_status == "completed" {
        "failure"
    } else {
        "pending"
    };
    Json(serde_json::json!({
        "state": combined,
        "total_count": 0,
        "statuses": []
    }))
    .into_response()
}

fn issue_payload(state: &FakeState) -> serde_json::Value {
    serde_json::json!({
        "id": 1007,
        "number": 7,
        "title": "Fix the widget",
        "body": "body",
        "state": if state.issue_closed { "closed" } else { "open" },
        "state_reason": if state.issue_closed { serde_json::Value::String("completed".into()) } else { serde_json::Value::Null },
        "labels": [],
        "user": {"id": 1, "login": "octo"},
        "author_association": "MEMBER",
        "html_url": "https://github.com/acme/widgets/issues/7",
        "comments": state.issue_comments.len(),
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "closed_at": if state.issue_closed { serde_json::Value::String("2026-01-02T00:00:00Z".into()) } else { serde_json::Value::Null }
    })
}

async fn get_issue_handler(State(fake): State<Fake>) -> Response {
    let state = fake.state.lock().unwrap();
    Json(issue_payload(&state)).into_response()
}

async fn close_issue_handler(State(fake): State<Fake>) -> Response {
    let mut state = fake.state.lock().unwrap();
    state.issue_close_calls += 1;
    state.issue_closed = true;
    Json(issue_payload(&state)).into_response()
}

async fn list_issue_comments_handler(State(fake): State<Fake>) -> Response {
    let state = fake.state.lock().unwrap();
    Json(serde_json::json!(state.issue_comments)).into_response()
}

async fn create_issue_comment_handler(
    State(fake): State<Fake>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let mut state = fake.state.lock().unwrap();
    state.issue_comment_calls += 1;
    let id = 2000 + state.issue_comment_calls;
    let comment = serde_json::json!({
        "id": id,
        "body": body.get("body").cloned().unwrap_or(serde_json::Value::Null),
        "user": {"id": 1, "login": "octo"},
        "html_url": format!("https://github.com/acme/widgets/issues/7#issuecomment-{id}"),
        "created_at": "2026-01-01T00:00:00Z"
    });
    state.issue_comments.push(comment.clone());
    Json(comment).into_response()
}

async fn start_fake(repo: &RepoFixture) -> (String, Arc<Mutex<FakeState>>) {
    let state = FakeState::new(repo);
    let fake = Fake {
        state: Arc::clone(&state),
    };
    let app = Router::new()
        .route("/repos/{owner}/{repo}", get(repository_handler))
        .route(
            "/repos/{owner}/{repo}/branches/{*rest}",
            get(branch_or_protection_handler),
        )
        .route(
            "/repos/{owner}/{repo}/pulls",
            get(list_pulls_handler).post(create_pull_handler),
        )
        .route(
            "/repos/{owner}/{repo}/pulls/{number}",
            get(get_pull_handler),
        )
        .route(
            "/repos/{owner}/{repo}/pulls/{number}/merge",
            put(merge_handler),
        )
        .route(
            "/repos/{owner}/{repo}/commits/{sha}/check-runs",
            get(check_runs_handler),
        )
        .route(
            "/repos/{owner}/{repo}/commits/{sha}/status",
            get(status_handler),
        )
        .route(
            "/repos/{owner}/{repo}/issues/{number}",
            get(get_issue_handler).patch(close_issue_handler),
        )
        .route(
            "/repos/{owner}/{repo}/issues/{number}/comments",
            get(list_issue_comments_handler).post(create_issue_comment_handler),
        )
        .with_state(fake);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{address}"), state)
}

struct Fixture {
    db: Database,
    repo: RepoFixture,
    base_url: String,
    fake: Arc<Mutex<FakeState>>,
    campaign_id: String,
    candidate_id: String,
    candidate_sha: String,
    merge_sha: String,
}

async fn setup_fixture(risk: &str) -> Fixture {
    let repo = init_repo();
    let path = temp_root("factory-delivery-db").with_extension("sqlite");
    let db = Database::open(&path).await.unwrap();
    let now = Utc::now().to_rfc3339();
    sqlx::query::query(
        "INSERT INTO projects
         (id,repository,base_commit,created_at,identity,name,default_branch,remote_origin)
         VALUES ('project',?1,?2,?3,?4,'Widgets','main',?5)",
    )
    .bind(repo.work.to_str().unwrap())
    .bind(&repo.base)
    .bind(&now)
    .bind(&repo.identity)
    .bind(repo.remote.to_str().unwrap())
    .execute(db.test_pool())
    .await
    .unwrap();
    db.test_ensure_project_config("project").await.unwrap();
    let objective = "Deliver the exact candidate";
    let objective_sha = sha256_hex(objective.as_bytes());
    let spec = serde_json::json!({
        "schema_version": 1,
        "project_id": "project",
        "repository": repo.work.to_str().unwrap(),
        "base_commit": repo.base.clone(),
        "objective": objective,
        "objective_sha256": objective_sha,
        "provider": "opencode-go",
        "model": "deepseek-v4.1-flash",
        "allowed_paths": ["src", "docs", "README.md"],
        "checks": []
    });
    let spec_json = spec.to_string();
    let spec_sha = sha256_hex(spec_json.as_bytes());
    sqlx::query::query(
        "INSERT INTO actions
         (id,project_id,objective,provider,model,risk,status,objective_sha256,created_at)
         VALUES ('action-1','project',?1,'opencode-go','deepseek-v4.1-flash',?2,
                 'AUTHORIZED',?3,?4)",
    )
    .bind(objective)
    .bind(risk)
    .bind(&objective_sha)
    .bind(&now)
    .execute(db.test_pool())
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO action_specs(action_id,spec_json,spec_sha256) VALUES ('action-1',?1,?2)",
    )
    .bind(&spec_json)
    .bind(&spec_sha)
    .execute(db.test_pool())
    .await
    .unwrap();
    sqlx::query::query("INSERT INTO campaigns(id,action_id,status,created_at) VALUES ('campaign-1','action-1','AUTHORIZED',?1)")
        .bind(&now)
        .execute(db.test_pool())
        .await
        .unwrap();
    sqlx::query::query("INSERT INTO runs(id,campaign_id,status,created_at) VALUES ('run-1','campaign-1','RUNNING',?1)")
        .bind(&now)
        .execute(db.test_pool())
        .await
        .unwrap();
    sqlx::query::query("INSERT INTO attempts(id,run_id,status,created_at) VALUES ('attempt-1','run-1','RUNNING',?1)")
        .bind(&now)
        .execute(db.test_pool())
        .await
        .unwrap();
    // Pin the exact base the candidate was produced from. Falling back to the
    // Project base would change after a successful merge advance and break
    // idempotent delivery replay.
    sqlx::query::query(
        "INSERT INTO attempt_specs(attempt_id,action_id,base_commit,spec_json,spec_sha256)
         VALUES ('attempt-1','action-1',?1,?2,?3)",
    )
    .bind(&repo.base)
    .bind(&spec_json)
    .bind(&spec_sha)
    .execute(db.test_pool())
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO candidates
         (id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at)
         VALUES ('candidate-1','attempt-1',?1,?2,?3,'worker',?4)",
    )
    .bind(&spec_sha)
    .bind(&repo.candidate)
    .bind(&repo.tree)
    .bind(&now)
    .execute(db.test_pool())
    .await
    .unwrap();
    sqlx::query::query("INSERT INTO gate_decisions(candidate_id,outcome,reasons_json,decided_at) VALUES ('candidate-1','ACCEPTED','[]',?1)")
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
    sqlx::query::query(
        "INSERT INTO github_connections
         (id,login,scopes_json,scopes_reported,credential_ciphertext,credential_nonce,
          credential_fingerprint,enabled,revision,verified_at,revoked_at,created_at,updated_at)
         VALUES ('default','octo','[\"repo\"]',1,'ciphertext','nonce','fingerprint',1,1,?1,NULL,?1,?1)",
    )
    .bind(&now)
    .execute(db.test_pool())
    .await
    .unwrap();
    let policy = DeliveryPolicyInput {
        enabled: true,
        r1_enabled: false,
        auto_merge_base_branch: "main".into(),
        manual_base_branches: vec!["main".into()],
        required_checks: vec!["ci".into()],
        merge_method: "squash".into(),
        post_merge_check_id: "post-merge".into(),
        post_merge_timeout_secs: 3,
        kill_switch: false,
        expires_at: None,
    };
    db.append_delivery_policy("project", "main", &policy, "owner")
        .await
        .unwrap();
    let (base_url, fake) = start_fake(&repo).await;
    let candidate_sha = repo.candidate.clone();
    Fixture {
        db,
        repo,
        base_url,
        fake,
        campaign_id: "campaign-1".into(),
        candidate_id: "candidate-1".into(),
        candidate_sha,
        merge_sha: "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".into(),
    }
}

async fn setup_salvage_fixture(risk: &str, with_evidence: bool) -> Fixture {
    let mut fixture = setup_fixture(risk).await;
    let now = Utc::now().to_rfc3339();
    sqlx::query::query(
        "INSERT INTO attempts(id,run_id,status,created_at)
         VALUES ('attempt-salvage','run-1','RUNNING',?1)",
    )
    .bind(&now)
    .execute(fixture.db.test_pool())
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO candidates
         (id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at,final_source)
         VALUES ('candidate-salvage','attempt-salvage',?1,?2,?3,'worker',?4,
                 'STAGNATION_SALVAGE')",
    )
    .bind("a".repeat(64))
    .bind(&fixture.repo.candidate)
    .bind(&fixture.repo.tree)
    .bind(&now)
    .execute(fixture.db.test_pool())
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO gate_decisions(candidate_id,outcome,reasons_json,decided_at)
         VALUES ('candidate-salvage','ACCEPTED','[]',?1)",
    )
    .bind(&now)
    .execute(fixture.db.test_pool())
    .await
    .unwrap();
    if with_evidence {
        let detail = serde_json::json!({
            "candidateId": "candidate-salvage",
            "commitSha": fixture.repo.candidate,
            "attemptId": "attempt-salvage",
            "campaignId": "campaign-1",
            "workItemId": "unit-1",
            "modelTurns": 2,
            "finalNudges": 1,
            "frozenChecks": 1,
            "passedChecks": 1,
            "written": true,
            "decision": "DIAGNOSE",
            "reason": "DIAGNOSE_WITH_WRITE_AND_ALL_FROZEN_CHECKS_PASS",
            "policyId": "policy-1",
            "authorizationId": "authorization-1",
            "diagnoseCheckpointId": "checkpoint-1",
            "stagnantCheckpoints": 2,
            "unsuccessfulReplans": 0,
            "diff": {
                "changedFiles": 1,
                "changedPaths": ["src/lib.rs"],
                "diffStat": "1 file changed"
            },
            "checkReceipts": [{
                "checkId": "ci",
                "receiptId": "receipt-1",
                "resultSha256": "b".repeat(64)
            }]
        });
        sqlx::query::query(
            "INSERT INTO events(subject_id,kind,detail,recorded_at)
             VALUES ('candidate-salvage','worker.stagnation_finalized',?1,?2)",
        )
        .bind(detail.to_string())
        .bind(&now)
        .execute(fixture.db.test_pool())
        .await
        .unwrap();
    }
    fixture.candidate_id = "candidate-salvage".into();
    fixture
}

fn credential() -> GitHubCredential {
    GitHubCredential::new(1, SecretValue::from_string(TOKEN.into())).unwrap()
}

#[derive(Clone, Default)]
struct RecordingHook {
    calls: Arc<Mutex<Vec<String>>>,
}

impl RecordingHook {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
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

impl DeliveryIssueHook for RecordingHook {
    async fn on_pr_opened(
        &self,
        campaign_id: &str,
        candidate_id: &str,
        _pull_request_number: u64,
        _pull_request_url: &str,
    ) -> Result<IssueDeliveryOutcome, IssueDeliveryError> {
        self.calls.lock().unwrap().push("pr_opened".into());
        Ok(empty_outcome(campaign_id, candidate_id))
    }

    async fn on_merge_succeeded(
        &self,
        campaign_id: &str,
        candidate_id: &str,
        _merge_sha: &str,
    ) -> Result<IssueDeliveryOutcome, IssueDeliveryError> {
        self.calls.lock().unwrap().push("merge_succeeded".into());
        Ok(empty_outcome(campaign_id, candidate_id))
    }

    async fn on_delivery_failed(
        &self,
        campaign_id: &str,
        candidate_id: &str,
        _error: &IssueDeliveryError,
    ) -> Result<IssueDeliveryOutcome, IssueDeliveryError> {
        self.calls.lock().unwrap().push("delivery_failed".into());
        Ok(empty_outcome(campaign_id, candidate_id))
    }
}

fn request<'a>(
    campaign_id: &'a str,
    candidate_id: &'a str,
    operation: DeliveryOperation,
    manual_confirmed: bool,
) -> DeliveryRequest<'a> {
    DeliveryRequest {
        campaign_id,
        candidate_id,
        base_branch: None,
        operation,
        manual_confirmed,
        actor: "tester",
    }
}

fn fake_set(fake: &Arc<Mutex<FakeState>>, update: impl FnOnce(&mut FakeState)) {
    let mut state = fake.lock().unwrap();
    update(&mut state);
}

async fn deliver(
    fixture: &Fixture,
    hook: &RecordingHook,
    manual_confirmed: bool,
) -> Result<DeliveryRun, DeliveryError> {
    let client = GitHubClient::with_base_url(&fixture.base_url, true).unwrap();
    let credential = credential();
    let service = DeliveryService::new(&fixture.db, &client, &credential, hook);
    service
        .deliver(&request(
            &fixture.campaign_id,
            &fixture.candidate_id,
            DeliveryOperation::Merge,
            manual_confirmed,
        ))
        .await
}

fn remote_branch_sha(fixture: &Fixture, branch: &str) -> String {
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(&fixture.repo.remote)
        .args(["rev-parse", &format!("refs/heads/{branch}")])
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

// -------------------------------------------------------------------------
// Deterministic policy matrix
// -------------------------------------------------------------------------

fn policy(mode: DeliveryMode, r1_enabled: bool) -> DeliveryPolicy {
    DeliveryPolicy {
        schema_version: 1,
        project_id: "project".into(),
        revision: 1,
        policy_sha256: "a".repeat(64),
        mode,
        r1_enabled,
        auto_merge_base_branch: "main".into(),
        manual_base_branches: vec!["main".into()],
        required_checks: vec!["ci".into()],
        merge_method: "squash".into(),
        post_merge_check_required: true,
        post_merge_check_id: "post-merge".into(),
        post_merge_timeout_secs: 30,
        kill_switch: false,
        created_by: "owner".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
        expires_at: None,
    }
}

fn facts<'a>(
    policy: &'a DeliveryPolicy,
    risk: DeliveryRisk,
    manual_confirmed: bool,
) -> DeliveryEvaluationInput<'a> {
    let candidate = Box::leak(Box::new(CandidateDeliveryFacts {
        candidate_id: "candidate".into(),
        candidate_sha: SHA_A.into(),
        candidate_tree_sha: SHA_B.into(),
        content_digest: "c".repeat(64),
        base_commit: "base".into(),
        risk_class: risk,
        latest: true,
        salvage_requires_human_review: false,
    }));
    let gate = Box::leak(Box::new(GateDeliveryFacts {
        candidate_id: "candidate".into(),
        candidate_sha: SHA_A.into(),
        gate_revision: "gate-1".into(),
        outcome: GateOutcome::Accepted,
        current: true,
    }));
    let pull_request = Box::leak(Box::new(PullRequestDeliveryFacts {
        number: 42,
        state: "open".into(),
        draft: false,
        merged: false,
        head_sha: SHA_A.into(),
        head_ref: "factory/candidate".into(),
        base_ref: "main".into(),
        base_sha: "base".into(),
        mergeable: Some(true),
        mergeable_state: "clean".into(),
    }));
    let checks = Box::leak(Box::new(ChecksDeliveryFacts {
        head_sha: SHA_A.into(),
        read_at: Utc::now(),
        checks: vec![CheckDeliveryFact {
            name: "ci".into(),
            state: "completed".into(),
            conclusion: "success".into(),
            required_by_protection: true,
        }],
    }));
    let protection = Box::leak(Box::new(ProtectionDeliveryFacts {
        read_at: Utc::now(),
        protected: true,
        allow_force_pushes: false,
        allow_deletions: false,
        required_checks: vec!["ci".into()],
        strict: true,
    }));
    DeliveryEvaluationInput {
        policy: Some(policy),
        candidate,
        gate: Some(gate),
        pull_request: Some(pull_request),
        checks: Some(checks),
        protection: Some(protection),
        charter: None,
        incident_open: false,
        incident_detail: None,
        manual_confirmed,
        base_branch: "main",
        base_is_default: true,
        current_base_sha: Some("base"),
        security_reviewed: false,
        now: Utc::now(),
    }
}

#[test]
fn policy_matrix_r0_auto_r1_opt_in_r2_r3_manual() {
    let risk_tiered = policy(DeliveryMode::RiskTiered, false);
    let input = facts(&risk_tiered, DeliveryRisk::R0, false);
    let decision = evaluate_delivery(&input);
    assert!(decision.allowed());
    assert_eq!(
        decision.authorization,
        Some(DeliveryAuthorization::PolicyR0)
    );

    let input = facts(&risk_tiered, DeliveryRisk::R1, false);
    let decision = evaluate_delivery(&input);
    assert!(!decision.allowed());
    assert_eq!(decision.reasons[0].code, "R1_OPT_IN_REQUIRED");

    let r1 = policy(DeliveryMode::RiskTiered, true);
    let input = facts(&r1, DeliveryRisk::R1, false);
    let decision = evaluate_delivery(&input);
    assert!(!decision.allowed());
    assert_eq!(decision.reasons[0].code, "CHARTER_GRANT_REQUIRED");

    let mut input = facts(&r1, DeliveryRisk::R1, false);
    let charter = Box::leak(Box::new(CharterDeliveryFacts {
        grant_id: "charter-1".into(),
        delivery_autonomy: true,
        covers: true,
        expired_or_revoked: false,
    }));
    input.charter = Some(charter);
    let decision = evaluate_delivery(&input);
    assert!(decision.allowed());
    assert_eq!(
        decision.authorization,
        Some(DeliveryAuthorization::CharterR1)
    );

    for risk in [DeliveryRisk::R2, DeliveryRisk::R3, DeliveryRisk::Unknown] {
        let input = facts(&risk_tiered, risk, false);
        let decision = evaluate_delivery(&input);
        assert!(!decision.allowed());
        assert_eq!(decision.reasons[0].code, "OWNER_APPROVAL_REQUIRED");
    }
    let input = facts(&risk_tiered, DeliveryRisk::R2, true);
    assert!(evaluate_delivery(&input).allowed());
    let input = facts(&risk_tiered, DeliveryRisk::R3, true);
    assert!(!evaluate_delivery(&input).allowed());
}

#[test]
fn manual_mode_requires_an_owner_decision() {
    let manual = policy(DeliveryMode::Manual, false);
    let input = facts(&manual, DeliveryRisk::R0, false);
    let decision = evaluate_delivery(&input);
    assert!(!decision.allowed());
    assert_eq!(decision.reasons[0].code, "DELIVERY_POLICY_DISABLED");
    let input = facts(&manual, DeliveryRisk::R0, true);
    let decision = evaluate_delivery(&input);
    assert!(decision.allowed());
    assert_eq!(
        decision.authorization,
        Some(DeliveryAuthorization::OwnerManual)
    );
}

#[test]
fn delivery_blocks_missing_policy_gate_and_incident() {
    let policy = policy(DeliveryMode::RiskTiered, false);
    let mut input = facts(&policy, DeliveryRisk::R0, false);
    input.policy = None;
    assert_eq!(
        evaluate_delivery(&input).reasons[0].code,
        "DELIVERY_POLICY_REQUIRED"
    );

    let mut input = facts(&policy, DeliveryRisk::R0, false);
    input.gate = None;
    assert_eq!(
        evaluate_delivery(&input).reasons[0].code,
        "GATE_NOT_ACCEPTED"
    );

    let mut input = facts(&policy, DeliveryRisk::R0, false);
    input.incident_open = true;
    input.incident_detail = Some("incident");
    assert_eq!(
        evaluate_delivery(&input).reasons[0].code,
        "DELIVERY_INCIDENT_OPEN"
    );

    let mut input = facts(&policy, DeliveryRisk::R0, false);
    input.base_is_default = false;
    assert_eq!(
        evaluate_delivery(&input).reasons[0].code,
        "BASE_NOT_DEFAULT"
    );
}

#[test]
fn delivery_blocks_stale_head_checks_and_unverified_protection() {
    let policy = policy(DeliveryMode::RiskTiered, false);

    let mut input = facts(&policy, DeliveryRisk::R0, false);
    let mut pull_request = input.pull_request.unwrap().clone();
    pull_request.head_sha = SHA_B.into();
    input.pull_request = Some(&pull_request);
    assert_eq!(evaluate_delivery(&input).reasons[0].code, "HEAD_MISMATCH");

    let mut input = facts(&policy, DeliveryRisk::R0, false);
    let checks = ChecksDeliveryFacts {
        head_sha: SHA_A.into(),
        read_at: input.now - chrono::Duration::seconds(FRESHNESS_WINDOW_SECS + 1),
        checks: vec![CheckDeliveryFact {
            name: "ci".into(),
            state: "completed".into(),
            conclusion: "success".into(),
            required_by_protection: true,
        }],
    };
    input.checks = Some(&checks);
    let decision = evaluate_delivery(&input);
    assert!(!decision.reasons.is_empty(), "{decision:?}");
    assert_eq!(decision.reasons[0].code, "CHECKS_STALE");

    let mut input = facts(&policy, DeliveryRisk::R0, false);
    let protection = ProtectionDeliveryFacts {
        read_at: input.now,
        protected: true,
        allow_force_pushes: true,
        allow_deletions: false,
        required_checks: vec!["ci".into()],
        strict: true,
    };
    input.protection = Some(&protection);
    assert_eq!(
        evaluate_delivery(&input).reasons[0].code,
        "BRANCH_PROTECTION_UNVERIFIED"
    );

    let mut input = facts(&policy, DeliveryRisk::R0, false);
    let checks = ChecksDeliveryFacts {
        head_sha: SHA_A.into(),
        read_at: input.now,
        checks: vec![CheckDeliveryFact {
            name: "ci".into(),
            state: "completed".into(),
            conclusion: "failure".into(),
            required_by_protection: true,
        }],
    };
    input.checks = Some(&checks);
    assert_eq!(evaluate_delivery(&input).reasons[0].code, "CHECKS_FAILED");
}

#[test]
fn risk_revalidation_cannot_downgrade_authorized_risk() {
    assert_eq!(
        revalidate_risk(DeliveryRisk::R2, &["src/lib.rs".into()]),
        DeliveryRisk::R2
    );
    assert_eq!(
        revalidate_risk(DeliveryRisk::R0, &["native/src/secrets.rs".into()]),
        DeliveryRisk::R3
    );
    assert_eq!(
        revalidate_risk(DeliveryRisk::R0, &["migrations/0001_initial.sql".into()]),
        DeliveryRisk::R2
    );
    assert_eq!(
        revalidate_risk(
            DeliveryRisk::R0,
            &["docs/contracts/delivery-policy.md".into()]
        ),
        DeliveryRisk::R1
    );
    assert_eq!(
        revalidate_risk(DeliveryRisk::R0, &[]),
        DeliveryRisk::Unknown
    );
}

#[test]
fn policy_input_requires_squash_and_r1_post_merge_check() {
    let mut input = DeliveryPolicyInput {
        enabled: true,
        r1_enabled: false,
        auto_merge_base_branch: "main".into(),
        manual_base_branches: vec!["main".into()],
        required_checks: vec!["ci".into()],
        merge_method: "merge".into(),
        post_merge_check_id: String::new(),
        post_merge_timeout_secs: 30,
        kill_switch: false,
        expires_at: None,
    };
    assert!(input.validate("main").is_err());
    input.merge_method = "squash".into();
    assert!(input.validate("main").is_err());
    input.post_merge_check_id = "post-merge".into();
    assert!(input.validate("main").is_ok());
    input.r1_enabled = true;
    assert!(input.validate("main").is_ok());
}

// -------------------------------------------------------------------------
// Fake GitHub integration
// -------------------------------------------------------------------------

#[tokio::test]
async fn r0_auto_merge_pushes_exact_candidate_and_merges_once() {
    let fixture = setup_fixture("R0").await;
    let hook = RecordingHook::default();
    let run = deliver(&fixture, &hook, false).await.unwrap();
    assert_eq!(run.attempt.state, DeliveryAttemptState::Merged);
    assert_eq!(
        run.attempt.merge_commit_sha.as_deref(),
        Some(fixture.merge_sha.as_str())
    );
    assert!(!run.replayed);
    assert_eq!(run.attempt.pull_request_number, Some(42));
    assert_eq!(run.decision.decision, "ALLOW");
    let branch = delivery_branch_name(&fixture.campaign_id, &fixture.candidate_sha).unwrap();
    assert_eq!(remote_branch_sha(&fixture, &branch), fixture.candidate_sha);
    let fake = fixture.fake.lock().unwrap();
    assert_eq!(fake.merge_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fake.create_calls.load(Ordering::SeqCst), 1);
    drop(fake);
    assert_eq!(hook.calls(), vec!["pr_opened", "merge_succeeded"]);
    let receipts = fixture
        .db
        .delivery_issue_receipts(&run.attempt.attempt_id)
        .await
        .unwrap();
    assert!(receipts.is_empty());
    // The merged main commit becomes the next Project base for follow-ups.
    let config = fixture.db.project_config("project").await.unwrap();
    assert_eq!(config.base_commit, fixture.merge_sha);
    assert_eq!(config.revision, 2);
}

#[tokio::test]
async fn duplicate_delivery_replays_recorded_attempt_without_another_write() {
    let fixture = setup_fixture("R0").await;
    let hook = RecordingHook::default();
    let first = deliver(&fixture, &hook, false).await.unwrap();
    let replay = deliver(&fixture, &hook, false).await.unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.attempt.attempt_id, first.attempt.attempt_id);
    assert_eq!(
        replay.attempt.merge_commit_sha,
        first.attempt.merge_commit_sha
    );
    let fake = fixture.fake.lock().unwrap();
    assert_eq!(fake.merge_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fake.create_calls.load(Ordering::SeqCst), 1);
    drop(fake);
    assert_eq!(hook.calls(), vec!["pr_opened", "merge_succeeded"]);
    let attempts = fixture
        .db
        .delivery_projection(&fixture.campaign_id, &fixture.candidate_id)
        .await
        .unwrap()
        .attempts;
    assert_eq!(attempts.len(), 1);
}

#[tokio::test]
async fn open_pull_request_is_idempotent_and_delivery_reuses_it() {
    let fixture = setup_fixture("R0").await;
    let hook = RecordingHook::default();
    let client = GitHubClient::with_base_url(&fixture.base_url, true).unwrap();
    let credential = credential();
    let service = DeliveryService::new(&fixture.db, &client, &credential, &hook);
    let req = request(
        &fixture.campaign_id,
        &fixture.candidate_id,
        DeliveryOperation::OpenPullRequest,
        false,
    );
    let opened = service.open_pull_request(&req).await.unwrap();
    assert!(opened.created);
    let again = service.open_pull_request(&req).await.unwrap();
    assert!(!again.created);
    let run = service
        .deliver(&request(
            &fixture.campaign_id,
            &fixture.candidate_id,
            DeliveryOperation::Merge,
            false,
        ))
        .await
        .unwrap();
    assert_eq!(run.attempt.state, DeliveryAttemptState::Merged);
    let fake = fixture.fake.lock().unwrap();
    assert_eq!(fake.create_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fake.merge_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn r1_without_opt_in_is_blocked_before_merge() {
    let fixture = setup_fixture("R1").await;
    let hook = RecordingHook::default();
    let error = deliver(&fixture, &hook, false).await.unwrap_err();
    assert_eq!(error.kind(), "R1_OPT_IN_REQUIRED");
    let fake = fixture.fake.lock().unwrap();
    assert_eq!(fake.merge_calls.load(Ordering::SeqCst), 0);
    drop(fake);
    assert_eq!(hook.calls(), vec!["pr_opened", "delivery_failed"]);
    let attempt = fixture
        .db
        .latest_delivery_attempt(&fixture.campaign_id, &fixture.candidate_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(attempt.state, DeliveryAttemptState::Blocked);
    assert_eq!(attempt.blocker_code.as_deref(), Some("R1_OPT_IN_REQUIRED"));
    // A blocked delivery never advances the Project base.
    let config = fixture.db.project_config("project").await.unwrap();
    assert_eq!(config.base_commit, fixture.repo.base);
    assert_eq!(config.revision, 1);
}

#[tokio::test]
async fn changing_base_for_a_blocked_attempt_is_a_typed_conflict() {
    let fixture = setup_fixture("R1").await;
    let hook = RecordingHook::default();
    let error = deliver(&fixture, &hook, false).await.unwrap_err();
    assert_eq!(error.kind(), "R1_OPT_IN_REQUIRED");
    let blocked = fixture
        .db
        .latest_delivery_attempt(&fixture.campaign_id, &fixture.candidate_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(blocked.base_branch, "main");
    assert_eq!(blocked.pull_request_number, Some(42));
    let fake = fixture.fake.lock().unwrap();
    assert_eq!(fake.create_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fake.merge_calls.load(Ordering::SeqCst), 0);
    drop(fake);

    let client = GitHubClient::with_base_url(&fixture.base_url, true).unwrap();
    let credential = credential();
    let service = DeliveryService::new(&fixture.db, &client, &credential, &hook);
    let mut request = request(
        &fixture.campaign_id,
        &fixture.candidate_id,
        DeliveryOperation::Merge,
        true,
    );
    request.base_branch = Some("release");
    let error = service.deliver(&request).await.unwrap_err();
    assert_eq!(error.kind(), "BASE_MISMATCH");
    let fake = fixture.fake.lock().unwrap();
    assert_eq!(fake.create_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fake.merge_calls.load(Ordering::SeqCst), 0);
    let attempts = fixture
        .db
        .delivery_projection(&fixture.campaign_id, &fixture.candidate_id)
        .await
        .unwrap()
        .attempts;
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].base_branch, "main");
    assert_eq!(attempts[0].pull_request_number, Some(42));
}

#[tokio::test]
async fn concurrent_different_base_delivery_has_one_side_effect_set() {
    let fixture = setup_fixture("R2").await;
    let input = DeliveryPolicyInput {
        enabled: true,
        r1_enabled: false,
        auto_merge_base_branch: "main".into(),
        manual_base_branches: vec!["main".into(), "release".into()],
        required_checks: vec!["ci".into()],
        merge_method: "squash".into(),
        post_merge_check_id: "post-merge".into(),
        post_merge_timeout_secs: 3,
        kill_switch: false,
        expires_at: None,
    };
    fixture
        .db
        .append_delivery_policy("project", "main", &input, "owner")
        .await
        .unwrap();
    let client_main = GitHubClient::with_base_url(&fixture.base_url, true).unwrap();
    let client_release = GitHubClient::with_base_url(&fixture.base_url, true).unwrap();
    let credential_main = credential();
    let credential_release = credential();
    let hook = RecordingHook::default();
    let service_main = DeliveryService::new(&fixture.db, &client_main, &credential_main, &hook);
    let service_release =
        DeliveryService::new(&fixture.db, &client_release, &credential_release, &hook);
    let mut main_request = request(
        &fixture.campaign_id,
        &fixture.candidate_id,
        DeliveryOperation::Merge,
        true,
    );
    main_request.base_branch = Some("main");
    let mut release_request = request(
        &fixture.campaign_id,
        &fixture.candidate_id,
        DeliveryOperation::Merge,
        true,
    );
    release_request.base_branch = Some("release");

    let (main_result, release_result) = tokio::join!(
        service_main.deliver(&main_request),
        service_release.deliver(&release_request)
    );
    let mut successes = Vec::new();
    let mut base_conflicts = 0;
    for result in [main_result, release_result] {
        match result {
            Ok(run) => successes.push(run),
            Err(error) => {
                assert_eq!(error.kind(), "BASE_MISMATCH");
                base_conflicts += 1;
            }
        }
    }
    assert_eq!(successes.len(), 1, "exactly one base may own the attempt");
    assert_eq!(base_conflicts, 1);
    let fake = fixture.fake.lock().unwrap();
    assert_eq!(fake.create_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fake.merge_calls.load(Ordering::SeqCst), 1);
    drop(fake);
    assert_eq!(hook.calls(), vec!["pr_opened", "merge_succeeded"]);
    let attempts = fixture
        .db
        .delivery_projection(&fixture.campaign_id, &fixture.candidate_id)
        .await
        .unwrap()
        .attempts;
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].state, DeliveryAttemptState::Merged);
    assert!(matches!(
        attempts[0].base_branch.as_str(),
        "main" | "release"
    ));
    assert_eq!(successes[0].attempt.base_branch, attempts[0].base_branch);
    // Only the protected default branch may become the next Project base.
    let config = fixture.db.project_config("project").await.unwrap();
    if attempts[0].base_branch == "main" {
        assert_eq!(config.base_commit, fixture.merge_sha);
    } else {
        assert_eq!(config.base_commit, fixture.repo.base);
        assert_eq!(config.revision, 1);
    }
}

#[tokio::test]
async fn r1_with_opt_in_charter_and_post_merge_check_merges() {
    let fixture = setup_fixture("R1").await;
    let input = DeliveryPolicyInput {
        enabled: true,
        r1_enabled: true,
        auto_merge_base_branch: "main".into(),
        manual_base_branches: vec!["main".into()],
        required_checks: vec!["ci".into()],
        merge_method: "squash".into(),
        post_merge_check_id: "post-merge".into(),
        post_merge_timeout_secs: 3,
        kill_switch: false,
        expires_at: None,
    };
    fixture
        .db
        .append_delivery_policy("project", "main", &input, "owner")
        .await
        .unwrap();
    fixture
        .db
        .create_agentic_charter(&crate::db::NewAgenticCharter {
            owner: "owner",
            project_id: "project",
            domains: &["delivery".into()],
            risk_appetite: RiskClass::R1,
            execution_autonomy: false,
            repair_autonomy: false,
            delivery_autonomy: true,
            reserved_matters: &["credentials".into()],
            expires_at: &(Utc::now() + chrono::Duration::days(1)).to_rfc3339(),
        })
        .await
        .unwrap();
    let hook = RecordingHook::default();
    let run = deliver(&fixture, &hook, false).await.unwrap();
    assert_eq!(run.attempt.state, DeliveryAttemptState::Merged);
    assert_eq!(
        run.attempt.authorization,
        Some(DeliveryAuthorization::CharterR1)
    );
    assert!(run.decision.charter_grant_id.is_some());
    let post = fixture
        .db
        .delivery_projection(&fixture.campaign_id, &fixture.candidate_id)
        .await
        .unwrap()
        .post_merge_checks;
    assert_eq!(post.len(), 1);
    assert_eq!(post[0].status, "PASSED");
    assert_eq!(hook.calls(), vec!["pr_opened", "merge_succeeded"]);
}

#[tokio::test]
async fn r1_r2_r3_salvage_never_auto_merges_even_with_owner_confirmation() {
    for risk in ["R1", "R2", "R3"] {
        let fixture = setup_salvage_fixture(risk, true).await;
        let hook = RecordingHook::default();
        let error = deliver(&fixture, &hook, true).await.unwrap_err();
        assert_eq!(error.kind(), "SALVAGE_HUMAN_REVIEW_REQUIRED", "risk {risk}");
        let fake = fixture.fake.lock().unwrap();
        assert_eq!(fake.merge_calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn r1_salvage_requires_exact_core_evidence() {
    let fixture = setup_salvage_fixture("R1", false).await;
    let hook = RecordingHook::default();
    let error = deliver(&fixture, &hook, false).await.unwrap_err();
    assert_eq!(error.kind(), "SALVAGE_EVIDENCE_REQUIRED");
    let fake = fixture.fake.lock().unwrap();
    assert_eq!(fake.merge_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn status_surfaces_missing_r0_salvage_evidence_and_blocks_delivery() {
    let fixture = setup_salvage_fixture("R0", false).await;
    let client = GitHubClient::with_base_url(&fixture.base_url, true).unwrap();
    let credential = credential();
    let status = load_delivery_status(
        &fixture.db,
        Some(&client),
        Some(&credential),
        &fixture.campaign_id,
        &fixture.candidate_id,
    )
    .await
    .unwrap();
    assert_eq!(status.final_source, CandidateFinalSource::StagnationSalvage);
    assert!(!status.salvage_requires_human_review);
    assert!(status.salvage_evidence.is_none());
    assert!(
        status
            .salvage_error
            .as_deref()
            .is_some_and(|error| error.contains("SALVAGE_EVIDENCE_REQUIRED"))
    );
    let hook = RecordingHook::default();
    let error = deliver(&fixture, &hook, false).await.unwrap_err();
    assert_eq!(error.kind(), "SALVAGE_EVIDENCE_REQUIRED");
}

#[tokio::test]
async fn status_surfaces_invalid_r0_salvage_evidence() {
    let fixture = setup_salvage_fixture("R0", true).await;
    let row: (String,) = sqlx::query_as::query_as(
        "SELECT detail FROM events
         WHERE subject_id='candidate-salvage' AND kind='worker.stagnation_finalized'",
    )
    .fetch_one(fixture.db.test_pool())
    .await
    .unwrap();
    let mut detail: serde_json::Value = serde_json::from_str(&row.0).unwrap();
    detail["commitSha"] = serde_json::json!("f".repeat(40));
    sqlx::query::query(
        "UPDATE events SET detail=?1
         WHERE subject_id='candidate-salvage' AND kind='worker.stagnation_finalized'",
    )
    .bind(detail.to_string())
    .execute(fixture.db.test_pool())
    .await
    .unwrap();
    let client = GitHubClient::with_base_url(&fixture.base_url, true).unwrap();
    let credential = credential();
    let status = load_delivery_status(
        &fixture.db,
        Some(&client),
        Some(&credential),
        &fixture.campaign_id,
        &fixture.candidate_id,
    )
    .await
    .unwrap();
    assert!(status.salvage_error.is_some());
    let hook = RecordingHook::default();
    let error = deliver(&fixture, &hook, false).await.unwrap_err();
    assert_eq!(error.kind(), "SALVAGE_EVIDENCE_REQUIRED");
}

#[tokio::test]
async fn r0_salvage_remains_eligible_for_the_ordinary_r0_policy() {
    let fixture = setup_salvage_fixture("R0", true).await;
    let hook = RecordingHook::default();
    let run = deliver(&fixture, &hook, false).await.unwrap();
    assert_eq!(run.attempt.state, DeliveryAttemptState::Merged);
}

#[tokio::test]
async fn manually_confirmed_r1_runs_the_post_merge_check() {
    let fixture = setup_fixture("R1").await;
    let hook = RecordingHook::default();
    let run = deliver(&fixture, &hook, true).await.unwrap();
    assert_eq!(run.attempt.state, DeliveryAttemptState::Merged);
    assert_eq!(
        run.attempt.authorization,
        Some(DeliveryAuthorization::OwnerManual)
    );
    let post = fixture
        .db
        .delivery_projection(&fixture.campaign_id, &fixture.candidate_id)
        .await
        .unwrap()
        .post_merge_checks;
    assert_eq!(post.len(), 1);
    assert_eq!(post[0].status, "PASSED");
}

#[tokio::test]
async fn manually_confirmed_r1_post_merge_failure_opens_incident() {
    let fixture = setup_fixture("R1").await;
    fake_set(&fixture.fake, |state| {
        state.post_merge_conclusion = "failure".into();
    });
    let hook = RecordingHook::default();
    let error = deliver(&fixture, &hook, true).await.unwrap_err();
    assert_eq!(error.kind(), "POST_MERGE_CHECK_FAILED");
    let attempt = fixture
        .db
        .latest_delivery_attempt(&fixture.campaign_id, &fixture.candidate_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(attempt.state, DeliveryAttemptState::NeedsAttention);
    let incident = fixture
        .db
        .active_delivery_incident("project")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(incident.kind, "POST_MERGE_CHECK_FAILED");
}

#[tokio::test]
async fn r2_requires_owner_confirmation_and_unknown_risk_stays_manual() {
    let fixture = setup_fixture("R2").await;
    let hook = RecordingHook::default();
    let error = deliver(&fixture, &hook, false).await.unwrap_err();
    assert_eq!(error.kind(), "OWNER_APPROVAL_REQUIRED");
    let run = deliver(&fixture, &hook, true).await.unwrap();
    assert_eq!(run.attempt.state, DeliveryAttemptState::Merged);
    assert_eq!(
        run.attempt.authorization,
        Some(DeliveryAuthorization::OwnerManual)
    );

    let fixture = setup_fixture("R9").await;
    let hook = RecordingHook::default();
    let error = deliver(&fixture, &hook, false).await.unwrap_err();
    assert_eq!(error.kind(), "OWNER_APPROVAL_REQUIRED");
}

#[tokio::test]
async fn r3_manual_requires_security_review() {
    let fixture = setup_fixture("R3").await;
    let hook = RecordingHook::default();
    let error = deliver(&fixture, &hook, true).await.unwrap_err();
    assert_eq!(error.kind(), "OWNER_APPROVAL_REQUIRED");
    let fake = fixture.fake.lock().unwrap();
    assert_eq!(fake.merge_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn missing_protection_stale_head_and_failed_checks_block() {
    let fixture = setup_fixture("R0").await;
    fake_set(&fixture.fake, |state| state.protection = false);
    let hook = RecordingHook::default();
    let error = deliver(&fixture, &hook, false).await.unwrap_err();
    assert_eq!(error.kind(), "BRANCH_PROTECTION_UNVERIFIED");

    let fixture = setup_fixture("R0").await;
    fake_set(&fixture.fake, |state| {
        state.create_head_sha = Some(state.base_sha.clone());
    });
    let hook = RecordingHook::default();
    let error = deliver(&fixture, &hook, false).await.unwrap_err();
    assert_eq!(error.kind(), "HEAD_MISMATCH");

    let fixture = setup_fixture("R0").await;
    fake_set(&fixture.fake, |state| {
        state.check_conclusion = "failure".into();
    });
    let hook = RecordingHook::default();
    let error = deliver(&fixture, &hook, false).await.unwrap_err();
    assert_eq!(error.kind(), "CHECKS_FAILED");
}

#[tokio::test]
async fn read_only_token_and_existing_branch_conflict_are_typed() {
    let fixture = setup_fixture("R0").await;
    fake_set(&fixture.fake, |state| state.push_permission = false);
    let hook = RecordingHook::default();
    let error = deliver(&fixture, &hook, false).await.unwrap_err();
    assert_eq!(error.kind(), "DELIVERY_PERMISSION_REQUIRED");

    let fixture = setup_fixture("R0").await;
    let branch = delivery_branch_name(&fixture.campaign_id, &fixture.candidate_sha).unwrap();
    let output = Command::new("git")
        .arg("-C")
        .arg(&fixture.repo.work)
        .args([
            "push",
            fixture.repo.remote.to_str().unwrap(),
            &format!("{}:refs/heads/{branch}", fixture.repo.base),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let hook = RecordingHook::default();
    let error = deliver(&fixture, &hook, false).await.unwrap_err();
    assert_eq!(error.kind(), "HEAD_MISMATCH");
    assert_eq!(hook.calls(), Vec::<String>::new());
}

#[tokio::test]
async fn closed_pull_request_is_not_merged() {
    let fixture = setup_fixture("R0").await;
    fake_set(&fixture.fake, |state| state.new_pr_state = "closed".into());
    let hook = RecordingHook::default();
    let error = deliver(&fixture, &hook, false).await.unwrap_err();
    assert_eq!(error.kind(), "PR_NOT_OPEN");
    let fake = fixture.fake.lock().unwrap();
    assert_eq!(fake.merge_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn failed_post_merge_check_opens_incident_and_requires_manual_revert() {
    let fixture = setup_fixture("R1").await;
    let input = DeliveryPolicyInput {
        enabled: true,
        r1_enabled: true,
        auto_merge_base_branch: "main".into(),
        manual_base_branches: vec!["main".into()],
        required_checks: vec!["ci".into()],
        merge_method: "squash".into(),
        post_merge_check_id: "post-merge".into(),
        post_merge_timeout_secs: 2,
        kill_switch: false,
        expires_at: None,
    };
    fixture
        .db
        .append_delivery_policy("project", "main", &input, "owner")
        .await
        .unwrap();
    fixture
        .db
        .create_agentic_charter(&crate::db::NewAgenticCharter {
            owner: "owner",
            project_id: "project",
            domains: &["delivery".into()],
            risk_appetite: RiskClass::R1,
            execution_autonomy: false,
            repair_autonomy: false,
            delivery_autonomy: true,
            reserved_matters: &["credentials".into()],
            expires_at: &(Utc::now() + chrono::Duration::days(1)).to_rfc3339(),
        })
        .await
        .unwrap();
    fake_set(&fixture.fake, |state| {
        state.post_merge_conclusion = "failure".into();
    });
    let hook = RecordingHook::default();
    let error = deliver(&fixture, &hook, false).await.unwrap_err();
    assert_eq!(error.kind(), "POST_MERGE_CHECK_FAILED");
    let attempt = fixture
        .db
        .latest_delivery_attempt(&fixture.campaign_id, &fixture.candidate_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(attempt.state, DeliveryAttemptState::NeedsAttention);
    assert!(attempt.merge_commit_sha.is_some());
    // A merge whose required post-merge check fails never advances the base.
    let config = fixture.db.project_config("project").await.unwrap();
    assert_eq!(config.base_commit, fixture.repo.base);
    assert_eq!(config.revision, 1);
    let incident = fixture
        .db
        .active_delivery_incident("project")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(incident.state, DeliveryIncidentState::Open);
    assert_eq!(incident.kind, "POST_MERGE_CHECK_FAILED");
    assert!(
        fixture
            .db
            .delivery_projection(&fixture.campaign_id, &fixture.candidate_id)
            .await
            .unwrap()
            .reverts
            .is_empty()
    );
    assert_eq!(hook.calls(), vec!["pr_opened", "delivery_failed"]);

    let revert = fixture
        .db
        .record_delivery_revert(
            &incident.incident_id,
            77,
            SHA_B,
            "operator-token",
            "manual revert PR reviewed and merged",
        )
        .await
        .unwrap();
    assert_eq!(revert.revert_pr_number, 77);
    let incident = fixture
        .db
        .active_delivery_incident("project")
        .await
        .unwrap();
    assert!(incident.is_none());
    let attempt = fixture
        .db
        .delivery_attempt(&attempt.attempt_id)
        .await
        .unwrap();
    assert_eq!(attempt.state, DeliveryAttemptState::Reverted);
    let config = fixture.db.project_config("project").await.unwrap();
    assert_eq!(config.base_commit, fixture.repo.base);
}

async fn seed_issue_link(fixture: &Fixture) {
    let now = Utc::now().to_rfc3339();
    for sql in [
        "INSERT INTO execution_profiles(id,current_revision,content_hash,created_at,updated_at)
         VALUES ('profile-1',1,'profile-hash',?1,?1)",
        "INSERT INTO execution_profile_revisions
         (revision_id,profile_id,revision,content_json,content_hash,actor,created_at)
         VALUES ('revision-1','profile-1',1,'{}','profile-hash','owner',?1)",
        "INSERT INTO standing_delegations
         (id,principal_id,audit_id,project_id,revision,content_hash,content_json,revoked_at,created_at)
         VALUES ('delegation-1','owner','audit-1','project',1,'delegation-hash','{}',NULL,?1)",
        "INSERT INTO lead_requests
         (id,project_id,action_id,delegation_id,objective,request_hash,project_config_hash,
          state,failure_reason,created_at,updated_at)
         VALUES ('lead-1','project','action-1','delegation-1','Deliver',
                 'request-hash','config-hash','RUNNING',NULL,?1,?1)",
        "INSERT INTO intake_requests
         (id,project_id,objective,mode,profile_id,profile_revision_id,profile_revision,
          profile_hash,profile_name,action_id,delegation_id,lead_request_id,campaign_id,
          submission_json,submission_hash,idempotency_key,created_at,updated_at)
         VALUES ('request-1','project','Deliver','REVIEW_PLAN','profile-1','revision-1',1,
                 'profile-hash','Fixture','action-1','delegation-1','lead-1','campaign-1',
                 '{}','submission-hash','issue-link-key',?1,?1)",
        "INSERT INTO issue_links
         (id,project_id,request_id,campaign_id,candidate_id,repository_owner,repository_name,
          repository_full_name,issue_number,github_issue_id,issue_title,role,state,
          delivery_state,resolution,work_unit_id,pull_request_number,pull_request_url,
          merge_sha,merged_at,created_at,updated_at)
         VALUES ('link-1','project','request-1','campaign-1','candidate-1','acme','widgets',
                 'acme/widgets',7,1007,'Fix the widget','PRIMARY','QUEUED','NONE',
                 'UNRESOLVED',NULL,NULL,NULL,NULL,NULL,?1,?1)",
    ] {
        sqlx::query::query(sql)
            .bind(&now)
            .execute(fixture.db.test_pool())
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn live_issue_hook_comments_on_open_and_closes_only_after_merge() {
    let fixture = setup_fixture("R0").await;
    seed_issue_link(&fixture).await;
    let client = GitHubClient::with_base_url(&fixture.base_url, true).unwrap();
    let credential = credential();
    let hook = LiveIssueDeliveryHook::new(&fixture.db, &client, &credential)
        .await
        .unwrap();
    let service = DeliveryService::new(&fixture.db, &client, &credential, &hook);
    let run = service
        .deliver(&request(
            &fixture.campaign_id,
            &fixture.candidate_id,
            DeliveryOperation::Merge,
            false,
        ))
        .await
        .unwrap();
    assert_eq!(run.attempt.state, DeliveryAttemptState::Merged);
    let fake = fixture.fake.lock().unwrap();
    assert_eq!(fake.issue_comment_calls, 1);
    assert_eq!(fake.issue_close_calls, 1);
    assert!(fake.issue_closed);
    drop(fake);
    let link = fixture
        .db
        .issue_links_for_campaign(&fixture.campaign_id)
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(link.state, IssueLinkState::Done);
    assert_eq!(link.delivery_state, IssueDeliveryState::Closed);
    assert_eq!(link.merge_sha.as_deref(), Some(fixture.merge_sha.as_str()));
    let receipts = fixture
        .db
        .delivery_issue_receipts(&run.attempt.attempt_id)
        .await
        .unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].state, "CLOSED");
}

#[tokio::test]
async fn policy_revisions_are_append_only_and_attempt_identity_is_immutable() {
    let fixture = setup_fixture("R0").await;
    let same = DeliveryPolicyInput {
        enabled: true,
        r1_enabled: false,
        auto_merge_base_branch: "main".into(),
        manual_base_branches: vec!["main".into()],
        required_checks: vec!["ci".into()],
        merge_method: "squash".into(),
        post_merge_check_id: "post-merge".into(),
        post_merge_timeout_secs: 3,
        kill_switch: false,
        expires_at: None,
    };
    let (policy, created) = fixture
        .db
        .append_delivery_policy("project", "main", &same, "owner")
        .await
        .unwrap();
    assert!(!created);
    assert_eq!(policy.revision, 1);
    let changed = DeliveryPolicyInput {
        required_checks: vec!["ci".into(), "test".into()],
        ..same
    };
    let (policy, created) = fixture
        .db
        .append_delivery_policy("project", "main", &changed, "owner")
        .await
        .unwrap();
    assert!(created);
    assert_eq!(policy.revision, 2);
    assert_eq!(
        fixture.db.delivery_policies("project").await.unwrap().len(),
        2
    );
    assert!(
        sqlx::query::query("UPDATE delivery_policies SET mode='MANUAL' WHERE project_id='project'")
            .execute(fixture.db.test_pool())
            .await
            .is_err(),
        "delivery policies must reject updates"
    );

    let reservation = fixture
        .db
        .create_delivery_attempt(NewDeliveryAttempt {
            idempotency_key: "immutable-key".into(),
            operation: "MERGE",
            project_id: "project".into(),
            campaign_id: fixture.campaign_id.clone(),
            candidate_id: fixture.candidate_id.clone(),
            candidate_sha: fixture.candidate_sha.clone(),
            candidate_tree_sha: fixture.repo.tree.clone(),
            content_digest: "c".repeat(64),
            gate_revision: "gate-1".into(),
            gate_outcome: "ACCEPTED".into(),
            policy_revision: 2,
            policy_sha256: policy.policy_sha256,
            risk_class: "R0".into(),
            base_branch: "main".into(),
            expected_head_sha: fixture.candidate_sha.clone(),
            merge_method: "squash".into(),
        })
        .await
        .unwrap();
    let attempt = reservation.attempt;
    assert!(
        sqlx::query::query("UPDATE delivery_attempts SET candidate_sha=?1 WHERE attempt_id=?2")
            .bind("f".repeat(40))
            .bind(&attempt.attempt_id)
            .execute(fixture.db.test_pool())
            .await
            .is_err(),
        "delivery attempt identity must be immutable"
    );
}

#[tokio::test]
async fn duplicate_attempt_creation_is_idempotent() {
    let fixture = setup_fixture("R0").await;
    let input = NewDeliveryAttempt {
        idempotency_key: "fixed-key".into(),
        operation: "MERGE",
        project_id: "project".into(),
        campaign_id: fixture.campaign_id.clone(),
        candidate_id: fixture.candidate_id.clone(),
        candidate_sha: fixture.candidate_sha.clone(),
        candidate_tree_sha: fixture.repo.tree.clone(),
        content_digest: "c".repeat(64),
        gate_revision: "gate-1".into(),
        gate_outcome: "ACCEPTED".into(),
        policy_revision: 1,
        policy_sha256: "a".repeat(64),
        risk_class: "R0".into(),
        base_branch: "main".into(),
        expected_head_sha: fixture.candidate_sha.clone(),
        merge_method: "squash".into(),
    };
    let first = fixture
        .db
        .create_delivery_attempt(input.clone())
        .await
        .unwrap();
    assert!(first.created);
    assert!(!first.base_conflict);
    let second = fixture.db.create_delivery_attempt(input).await.unwrap();
    assert!(!second.created);
    assert!(!second.base_conflict);
    assert_eq!(first.attempt.attempt_id, second.attempt.attempt_id);
    let (claimed, _) = fixture
        .db
        .claim_delivery_attempt(&first.attempt.attempt_id)
        .await
        .unwrap();
    assert!(claimed);
    let (claimed, attempt) = fixture
        .db
        .claim_delivery_attempt(&first.attempt.attempt_id)
        .await
        .unwrap();
    assert!(!claimed);
    assert_eq!(attempt.state, DeliveryAttemptState::Merging);
    fixture
        .db
        .record_delivery_attempt_event(NewDeliveryAttemptEvent {
            attempt_id: &first.attempt.attempt_id,
            kind: "TEST",
            detail: "event",
        })
        .await
        .unwrap();
    assert_eq!(
        fixture
            .db
            .delivery_projection(&fixture.campaign_id, &fixture.candidate_id)
            .await
            .unwrap()
            .attempts
            .len(),
        1
    );
}

// Keep the issue state types imported so the fixture proves the hook contract
// remains compatible with the WP2 delivery vocabulary.
#[allow(dead_code)]
fn assert_issue_vocabulary() {
    assert_eq!(IssueLinkState::InReview.as_str(), "IN_REVIEW");
    assert_eq!(IssueDeliveryState::Closed.as_str(), "CLOSED");
}
