use super::*;

use chrono::Utc;
use rusqlite::params;
use uuid::Uuid;

use super::lifecycle::load_attempt;

#[test]
fn old_experimental_database_is_rejected_without_modification() {
    let path = std::env::temp_dir().join(format!("factory-db-test-{}.sqlite", Uuid::new_v4()));
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE projects (id TEXT PRIMARY KEY); INSERT INTO projects VALUES ('keep');",
        )
        .unwrap();
    drop(connection);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert!(matches!(Database::open(&path), Err(Error::Invalid(_))));
    let connection = Connection::open(&path).unwrap();
    let retained: String = connection
        .query_row("SELECT id FROM projects", [], |row| row.get(0))
        .unwrap();
    assert_eq!(retained, "keep");
    drop(connection);
    let _ = std::fs::remove_file(path);
}

#[cfg(unix)]
#[test]
fn existing_group_readable_database_is_rejected_without_changing_data_or_mode() {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::temp_dir().join(format!("factory-insecure-db-{}.sqlite", Uuid::new_v4()));
    let db = Database::open(&path).unwrap();
    db.0.lock()
        .unwrap()
        .execute_batch("CREATE TABLE retained(value TEXT); INSERT INTO retained VALUES ('keep');")
        .unwrap();
    drop(db);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        Database::open(&path),
        Err(Error::Invalid(
            "existing database file grants group or other access"
        ))
    ));
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o644
    );
    let connection = Connection::open(&path).unwrap();
    let value: String = connection
        .query_row("SELECT value FROM retained", [], |row| row.get(0))
        .unwrap();
    assert_eq!(value, "keep");
    drop(connection);
    let _ = std::fs::remove_file(path);
}

#[cfg(unix)]
#[test]
fn database_symlink_path_is_rejected() {
    let directory = std::env::temp_dir().join(format!("factory-db-link-{}", Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let target = directory.join("target.sqlite");
    let link = directory.join("linked.sqlite");
    let _db = Database::open(&target).unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(matches!(
        Database::open(&link),
        Err(Error::Invalid(
            "database path must be a regular non-symlink file"
        ))
    ));
    drop(_db);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn idempotency_rejects_changed_action_and_cross_operation() {
    let path = std::env::temp_dir().join(format!("factory-db-test-{}.sqlite", Uuid::new_v4()));
    let db = Database::open(&path).unwrap();
    {
        let connection = db.0.lock().unwrap();
        connection
            .execute(
                "INSERT INTO projects VALUES (?1,?2,?3,?4)",
                params!["project", "/fixture", "base", "now"],
            )
            .unwrap();
    }
    let action = db
        .create_action("project", "fix bug", "codex", "model-a", "key-one")
        .unwrap();
    assert_eq!(
        action.id,
        db.create_action("project", "fix bug", "codex", "model-a", "key-one")
            .unwrap()
            .id
    );
    assert!(matches!(
        db.create_action("project", "different", "codex", "model-a", "key-one"),
        Err(Error::Invalid(_))
    ));
    {
        let connection = db.0.lock().unwrap();
        assert!(matches!(
            replay(&connection, "key-one", "register_project", "digest"),
            Err(Error::Invalid(_))
        ));
    }
    drop(db);
    let _ = std::fs::remove_file(path);
}

#[test]
fn action_spec_is_frozen_but_execution_remains_human_required() {
    let path = std::env::temp_dir().join(format!("factory-db-test-{}.sqlite", Uuid::new_v4()));
    let db = Database::open(&path).unwrap();
    {
        let connection = db.0.lock().unwrap();
        connection
            .execute(
                "INSERT INTO projects VALUES (?1,?2,?3,?4)",
                params!["project", "/fixture", "base", "now"],
            )
            .unwrap();
    }
    let action = db
        .create_action(
            "project",
            "fix bug",
            "opencode",
            "provider/model",
            "key-two",
        )
        .unwrap();
    assert_eq!(action.status, "HUMAN_REQUIRED");
    assert_eq!(action.risk, "UNASSESSED");
    assert_eq!(action.spec.base_commit, "base");
    assert_eq!(action.spec.schema_version, 1);
    assert_eq!(action.spec.objective, "fix bug");
    let canonical_spec = serde_json::to_string(&action.spec).unwrap();
    assert_eq!(action.spec_sha256, sha256_hex(canonical_spec.as_bytes()));
    assert_ne!(action.spec_sha256, action.objective_sha256);
    let listed = db.actions().unwrap();
    assert_eq!(listed[0].spec, action.spec);
    assert_eq!(listed[0].spec_sha256, action.spec_sha256);
    drop(db);
    let _ = std::fs::remove_file(path);
}

#[test]
fn campaign_is_durable_but_cannot_create_run_without_authorization() {
    let path = std::env::temp_dir().join(format!("factory-db-test-{}.sqlite", Uuid::new_v4()));
    let db = Database::open(&path).unwrap();
    db.0.lock()
        .unwrap()
        .execute(
            "INSERT INTO projects VALUES (?1,?2,?3,?4)",
            params!["project", "/fixture", "base", "now"],
        )
        .unwrap();
    let action = db
        .create_action("project", "objective", "codex", "model", "action-key")
        .unwrap();
    let campaign = db.create_campaign(&action.id, "campaign-key").unwrap();
    assert_eq!(campaign.status, "BLOCKED_AUTHORIZATION");
    assert_eq!(
        campaign.id,
        db.create_campaign(&action.id, "campaign-key").unwrap().id
    );
    assert_eq!(db.campaigns().unwrap()[0].id, campaign.id);
    assert!(matches!(
        db.create_run(&campaign.id, "run-key"),
        Err(Error::Invalid(_))
    ));
    let events = db.events().unwrap();
    assert!(events.iter().any(|event| event.kind == "campaign.created"));
    drop(db);
    let _ = std::fs::remove_file(path);
}

fn authorized_attempt(db: &Database) -> String {
    db.0.lock()
        .unwrap()
        .execute(
            "INSERT INTO projects VALUES (?1,?2,?3,?4)",
            params!["project", "/fixture", "base", "now"],
        )
        .unwrap();
    let action = db
        .create_action("project", "objective", "codex", "model", "action-key")
        .unwrap();
    let campaign = db.create_campaign(&action.id, "campaign-key").unwrap();
    // Fixture setup only: there is deliberately no public authorization or
    // sandbox attestation route in the product API.
    db.0.lock()
        .unwrap()
        .execute(
            "UPDATE campaigns SET status='AUTHORIZED' WHERE id=?1",
            [&campaign.id],
        )
        .unwrap();
    let run = db.create_run(&campaign.id, "run-key").unwrap();
    let attempt = db.create_attempt(&run.id, "attempt-key").unwrap();
    db.0.lock()
        .unwrap()
        .execute(
            "UPDATE attempts SET sandbox_ready=1 WHERE id=?1",
            [&attempt.id],
        )
        .unwrap();
    attempt.id
}

#[test]
fn worker_claim_is_fenced_expires_and_receipts_survive_restart() {
    let path = std::env::temp_dir().join(format!("factory-lease-test-{}.sqlite", Uuid::new_v4()));
    let db = Database::open(&path).unwrap();
    let attempt_id = authorized_attempt(&db);
    assert_eq!(db.runs().unwrap().len(), 1);
    assert_eq!(db.attempts().unwrap()[0].id, attempt_id);
    let first = db.claim_attempt("worker-a", 100, 50).unwrap().unwrap();
    assert_eq!(first.generation, 1);
    assert!(db.heartbeat(&attempt_id, "worker-a", 1, 120, 50).is_ok());
    assert!(matches!(
        db.heartbeat(&attempt_id, "worker-a", 1, 170, 50),
        Err(Error::Invalid(_))
    ));
    let second = db.claim_attempt("worker-b", 170, 50).unwrap().unwrap();
    assert_eq!(second.generation, 2);
    let report = WorkerReport {
        attempt_id: attempt_id.clone(),
        worker_id: "worker-b".into(),
        generation: 2,
        receipt_id: "receipt-one".into(),
        result_sha256: "a".repeat(64),
        terminal_status: "FAILED".into(),
    };
    assert!(matches!(
        db.report_terminal(
            &WorkerReport {
                terminal_status: "SUCCEEDED".into(),
                ..report.clone()
            },
            180
        ),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        db.report_terminal(
            &WorkerReport {
                worker_id: "worker-a".into(),
                generation: 1,
                ..report.clone()
            },
            180
        ),
        Err(Error::Invalid(_))
    ));
    db.report_terminal(&report, 180).unwrap();
    db.report_terminal(&report, 181).unwrap();
    drop(db);
    let reopened = Database::open(&path).unwrap();
    reopened.report_terminal(&report, 182).unwrap();
    assert_eq!(
        load_attempt(&reopened.0.lock().unwrap(), &attempt_id)
            .unwrap()
            .status,
        "FAILED"
    );
    drop(reopened);
    let _ = std::fs::remove_file(path);
}

#[test]
fn concurrent_workers_claim_distinct_ready_attempts_atomically() {
    let path = std::env::temp_dir().join(format!("factory-claim-test-{}.sqlite", Uuid::new_v4()));
    let db = Database::open(&path).unwrap();
    let first = authorized_attempt(&db);
    db.0.lock()
        .unwrap()
        .execute(
            "INSERT INTO projects VALUES (?1,?2,?3,?4)",
            params!["project-2", "/fixture-2", "base", "now"],
        )
        .unwrap();
    let action = db
        .create_action("project-2", "objective", "codex", "model", "action-key-2")
        .unwrap();
    let campaign = db.create_campaign(&action.id, "campaign-key-2").unwrap();
    db.0.lock()
        .unwrap()
        .execute(
            "UPDATE campaigns SET status='AUTHORIZED' WHERE id=?1",
            [&campaign.id],
        )
        .unwrap();
    let run = db.create_run(&campaign.id, "run-key-2").unwrap();
    let second = db.create_attempt(&run.id, "attempt-key-2").unwrap();
    db.0.lock()
        .unwrap()
        .execute(
            "UPDATE attempts SET sandbox_ready=1 WHERE id=?1",
            [&second.id],
        )
        .unwrap();
    let left = Database::open(&path).unwrap();
    let right = Database::open(&path).unwrap();
    let a = std::thread::spawn(move || left.claim_attempt("worker-a", 100, 1000).unwrap().unwrap());
    let b =
        std::thread::spawn(move || right.claim_attempt("worker-b", 100, 1000).unwrap().unwrap());
    let a = a.join().unwrap();
    let b = b.join().unwrap();
    assert_ne!(a.attempt_id, b.attempt_id);
    assert!(a.attempt_id == first || a.attempt_id == second.id);
    drop(db);
    let _ = std::fs::remove_file(path);
}

#[test]
fn lease_arithmetic_overflow_fails_closed_without_reusing_generation() {
    let path =
        std::env::temp_dir().join(format!("factory-overflow-test-{}.sqlite", Uuid::new_v4()));
    let db = Database::open(&path).unwrap();
    let attempt = authorized_attempt(&db);
    assert!(matches!(
        db.claim_attempt("worker-a", i64::MAX - 5, 50),
        Err(Error::Invalid(_))
    ));
    assert!(db.claim_attempt("worker-a", 100, 50).unwrap().is_some());
    db.0.lock()
        .unwrap()
        .execute(
            "UPDATE worker_leases SET generation=?1,expires_at_ms=0 WHERE attempt_id=?2",
            params![i64::MAX, attempt],
        )
        .unwrap();
    assert!(matches!(
        db.claim_attempt("worker-b", 10, 50),
        Err(Error::Invalid("worker generation exhausted"))
    ));
    let generation: i64 =
        db.0.lock()
            .unwrap()
            .query_row(
                "SELECT generation FROM worker_leases WHERE attempt_id=?1",
                [attempt],
                |row| row.get(0),
            )
            .unwrap();
    assert_eq!(generation, i64::MAX);
    drop(db);
    let _ = std::fs::remove_file(path);
}

#[test]
fn worker_registration_stores_only_hash_and_replay_rotates_session_across_restart() {
    let path =
        std::env::temp_dir().join(format!("factory-worker-session-{}.sqlite", Uuid::new_v4()));
    let db = Database::open(&path).unwrap();
    assert!(
        db.register_worker_session(
            "worker-a",
            "registration-a",
            "bootstrap-secret-12345678901234567890",
            "session-token-aaaaaaaaaaaaaaaaaaaaaaaa"
        )
        .unwrap()
        .created
    );
    assert!(
        db.worker_session_matches("worker-a", "session-token-aaaaaaaaaaaaaaaaaaaaaaaa")
            .unwrap()
    );
    assert!(
        !db.worker_session_matches("worker-b", "session-token-aaaaaaaaaaaaaaaaaaaaaaaa")
            .unwrap()
    );
    assert!(
        !db.register_worker_session(
            "worker-a",
            "registration-a",
            "bootstrap-secret-12345678901234567890",
            "session-token-bbbbbbbbbbbbbbbbbbbbbbbb"
        )
        .unwrap()
        .created
    );
    assert!(
        !db.worker_session_matches("worker-a", "session-token-aaaaaaaaaaaaaaaaaaaaaaaa")
            .unwrap()
    );
    assert!(
        db.worker_session_matches("worker-a", "session-token-bbbbbbbbbbbbbbbbbbbbbbbb")
            .unwrap()
    );
    assert!(matches!(
        db.register_worker_session(
            "worker-a",
            "new-key",
            "bootstrap-secret-12345678901234567890",
            "session-token-cccccccccccccccccccccccc"
        ),
        Err(Error::Conflict(_))
    ));
    let event_details: Vec<String> = {
        let connection = db.0.lock().unwrap();
        let mut query = connection
            .prepare("SELECT detail FROM events WHERE subject_id='worker-a'")
            .unwrap();
        query
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert!(
        event_details
            .iter()
            .all(|detail| !detail.contains("session-token"))
    );
    let persisted_registration_key: String =
        db.0.lock()
            .unwrap()
            .query_row(
                "SELECT key FROM idempotency WHERE operation='register_worker_session'",
                [],
                |row| row.get(0),
            )
            .unwrap();
    assert_ne!(persisted_registration_key, "registration-a");
    assert_eq!(
        persisted_registration_key,
        format!(
            "worker-registration-v1:{}",
            hmac_sha256_hex(b"bootstrap-secret-12345678901234567890", b"registration-a")
        )
    );
    drop(db);
    let persisted_bytes = std::fs::read(&path).unwrap();
    assert!(
        !persisted_bytes
            .windows(b"registration-a".len())
            .any(|window| window == b"registration-a")
    );
    assert!(
        !persisted_bytes
            .windows(b"bootstrap-secret-12345678901234567890".len())
            .any(|window| window == b"bootstrap-secret-12345678901234567890")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let reopened = Database::open(&path).unwrap();
    assert!(
        reopened
            .worker_session_matches("worker-a", "session-token-bbbbbbbbbbbbbbbbbbbbbbbb")
            .unwrap()
    );
    let persisted_hash: String = reopened
        .0
        .lock()
        .unwrap()
        .query_row(
            "SELECT token_sha256 FROM worker_sessions WHERE worker_id='worker-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        persisted_hash,
        sha256_hex("session-token-bbbbbbbbbbbbbbbbbbbbbbbb".as_bytes())
    );
    drop(reopened);
    let _ = std::fs::remove_file(path);
}

struct CandidateFixture {
    root: std::path::PathBuf,
    db: Database,
    attempt_id: String,
    candidate_id: String,
    spec_sha256: String,
    commit_sha: String,
    tree_sha: String,
    worker_id: String,
    generation: i64,
}

impl Drop for CandidateFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn git_fixture(root: &std::path::Path, arguments: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn candidate_fixture(risk: &str) -> CandidateFixture {
    let root = std::env::temp_dir().join(format!("factory-quality-{}", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git_fixture(&repo, &["init", "-q"]);
    git_fixture(&repo, &["config", "user.name", "Fixture"]);
    git_fixture(&repo, &["config", "user.email", "fixture@example.invalid"]);
    std::fs::write(repo.join("seed.txt"), "base\n").unwrap();
    git_fixture(&repo, &["add", "seed.txt"]);
    git_fixture(&repo, &["commit", "-qm", "base"]);

    let db = Database::open(&root.join("factory.sqlite")).unwrap();
    let project = db.register_project(&repo, "project-key").unwrap();
    let action = db
        .create_action(
            &project.id,
            "bounded objective",
            "opencode",
            "test/model",
            "action-key",
        )
        .unwrap();
    let campaign = db.create_campaign(&action.id, "campaign-key").unwrap();
    db.0.lock()
        .unwrap()
        .execute(
            "UPDATE campaigns SET status='AUTHORIZED' WHERE id=?1",
            [&campaign.id],
        )
        .unwrap();
    db.0.lock()
        .unwrap()
        .execute(
            "UPDATE actions SET risk=?1 WHERE id=?2",
            params![risk, action.id],
        )
        .unwrap();
    let run = db.create_run(&campaign.id, "run-key").unwrap();
    let attempt = db.create_attempt(&run.id, "attempt-key").unwrap();
    db.0.lock()
        .unwrap()
        .execute(
            "UPDATE attempts SET sandbox_ready=1 WHERE id=?1",
            [&attempt.id],
        )
        .unwrap();
    let now_ms = Utc::now().timestamp_millis();
    let lease = db
        .claim_attempt("worker-fixture", now_ms, 60_000)
        .unwrap()
        .unwrap();
    assert_eq!(lease.attempt_id, attempt.id);

    let workspace_root = root.join("attempts");
    let allocator = crate::workspace::WorkspaceAllocator::new(&repo, &workspace_root).unwrap();
    let mut workspace = allocator
        .allocate(&attempt.id, &project.base_commit)
        .unwrap();
    std::fs::write(workspace.path().join("seed.txt"), "candidate\n").unwrap();
    git_fixture(workspace.path(), &["add", "seed.txt"]);
    git_fixture(workspace.path(), &["commit", "-qm", "candidate"]);
    let candidate = workspace.candidate_revision().unwrap();
    workspace.cleanup().unwrap();
    let candidate = db
        .record_candidate(
            &NewCandidate {
                attempt_id: attempt.id.clone(),
                worker_id: "worker-fixture".into(),
                generation: lease.generation,
                action_spec_sha256: action.spec_sha256.clone(),
                commit_sha: candidate.commit_sha,
                tree_sha: candidate.tree_sha,
            },
            now_ms,
        )
        .unwrap();
    CandidateFixture {
        root,
        db,
        attempt_id: candidate.attempt_id,
        candidate_id: candidate.id,
        spec_sha256: candidate.action_spec_sha256,
        commit_sha: candidate.commit_sha,
        tree_sha: candidate.tree_sha,
        worker_id: "worker-fixture".into(),
        generation: lease.generation,
    }
}

fn quality_receipt(fixture: &CandidateFixture, id: &str, principal: &str) -> QualityReceiptInput {
    QualityReceiptInput {
        receipt_id: id.into(),
        principal_id: principal.into(),
        action_spec_sha256: fixture.spec_sha256.clone(),
        commit_sha: fixture.commit_sha.clone(),
        tree_sha: fixture.tree_sha.clone(),
        result: ReceiptResult::Pass,
        evidence: EvidenceKind::Executed,
    }
}

#[test]
fn candidate_and_independent_receipts_produce_accepted_gate() {
    let fixture = candidate_fixture("R0");
    let candidate = fixture.db.candidate(&fixture.candidate_id).unwrap();
    assert_eq!(candidate.commit_sha, fixture.commit_sha);
    assert_eq!(candidate.implementer_id, fixture.worker_id);

    let first = fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-receipt-1", "verifier-1"),
        )
        .unwrap();
    assert!(first.gate.is_none());
    assert_eq!(first.receipt.role, QualityRole::Verifier);

    let second = fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Reviewer,
            &QualityReceiptInput {
                result: ReceiptResult::ChangesRequested,
                evidence: EvidenceKind::Executed,
                ..quality_receipt(&fixture, "reviewer-receipt-1", "reviewer-1")
            },
        )
        .unwrap();
    assert_eq!(
        second.gate.as_ref().unwrap().outcome,
        GateOutcome::RepairRequired
    );

    let verifier_retry = fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-receipt-1", "verifier-1"),
        )
        .unwrap();
    assert_eq!(
        verifier_retry.gate.as_ref().unwrap().outcome,
        GateOutcome::RepairRequired
    );
    assert_eq!(
        fixture
            .db
            .quality_receipts(&fixture.candidate_id)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn accepted_gate_requires_pass_verifier_and_reviewer() {
    let fixture = candidate_fixture("R0");
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-pass-1", "verifier-1"),
        )
        .unwrap();
    let result = fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Reviewer,
            &quality_receipt(&fixture, "reviewer-pass-1", "reviewer-1"),
        )
        .unwrap();
    assert_eq!(result.gate.unwrap().outcome, GateOutcome::Accepted);
    assert_eq!(
        fixture
            .db
            .gate_for_candidate(&fixture.candidate_id)
            .unwrap()
            .unwrap()
            .outcome,
        GateOutcome::Accepted
    );
}

#[test]
fn gate_fails_closed_for_human_risk_and_unknown_policy() {
    for (risk, expected) in [
        ("R1", GateOutcome::HumanRequired),
        ("R2", GateOutcome::Blocked),
        ("R3", GateOutcome::Blocked),
        ("UNASSESSED", GateOutcome::Blocked),
    ] {
        let fixture = candidate_fixture(risk);
        fixture
            .db
            .record_quality_receipt(
                &fixture.candidate_id,
                QualityRole::Verifier,
                &quality_receipt(&fixture, "verifier", "verifier-1"),
            )
            .unwrap();
        let result = fixture
            .db
            .record_quality_receipt(
                &fixture.candidate_id,
                QualityRole::Reviewer,
                &quality_receipt(&fixture, "reviewer", "reviewer-1"),
            )
            .unwrap();
        assert_eq!(result.gate.unwrap().outcome, expected, "risk {risk}");
    }
}

#[test]
fn receipt_rejects_implementer_self_review_and_role_collusion() {
    let fixture = candidate_fixture("R0");
    let self_review = quality_receipt(&fixture, "self", &fixture.worker_id);
    assert!(matches!(
        fixture.db.record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &self_review,
        ),
        Err(Error::Forbidden(_))
    ));
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier", "same-principal"),
        )
        .unwrap();
    assert!(matches!(
        fixture.db.record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Reviewer,
            &quality_receipt(&fixture, "reviewer", "same-principal"),
        ),
        Err(Error::Forbidden(_))
    ));
}

#[test]
fn receipt_rejects_conflicts_wrong_binding_and_forged_generation() {
    let fixture = candidate_fixture("R0");
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier", "verifier-1"),
        )
        .unwrap();
    let mut changed = quality_receipt(&fixture, "verifier", "verifier-1");
    changed.result = ReceiptResult::Fail;
    assert!(matches!(
        fixture
            .db
            .record_quality_receipt(&fixture.candidate_id, QualityRole::Verifier, &changed,),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        fixture.db.record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "another-verifier", "verifier-2"),
        ),
        Err(Error::Conflict(_))
    ));
    let mut wrong = quality_receipt(&fixture, "verifier-wrong", "verifier-3");
    wrong.tree_sha = "0".repeat(40);
    assert!(matches!(
        fixture
            .db
            .record_quality_receipt(&fixture.candidate_id, QualityRole::Verifier, &wrong,),
        Err(Error::Forbidden(_))
    ));
    let candidate = fixture.db.candidate(&fixture.candidate_id).unwrap();
    assert!(
        fixture
            .db
            .record_candidate(
                &NewCandidate {
                    attempt_id: fixture.attempt_id.clone(),
                    worker_id: fixture.worker_id.clone(),
                    generation: fixture.generation + 1,
                    action_spec_sha256: fixture.spec_sha256.clone(),
                    commit_sha: candidate.commit_sha,
                    tree_sha: candidate.tree_sha,
                },
                Utc::now().timestamp_millis(),
            )
            .is_err()
    );
}

#[test]
fn candidate_and_gate_rows_are_immutable() {
    let fixture = candidate_fixture("R0");
    assert!(
        fixture
            .db
            .0
            .lock()
            .unwrap()
            .execute(
                "UPDATE candidates SET commit_sha='tampered' WHERE id=?1",
                [&fixture.candidate_id],
            )
            .is_err()
    );
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier", "verifier-1"),
        )
        .unwrap();
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Reviewer,
            &quality_receipt(&fixture, "reviewer", "reviewer-1"),
        )
        .unwrap();
    assert!(
        fixture
            .db
            .0
            .lock()
            .unwrap()
            .execute(
                "UPDATE gate_decisions SET outcome='ACCEPTED' WHERE candidate_id=?1",
                [&fixture.candidate_id],
            )
            .is_err()
    );
}

#[test]
fn action_read_rejects_tampered_frozen_spec() {
    let path = std::env::temp_dir().join(format!("factory-db-test-{}.sqlite", Uuid::new_v4()));
    let db = Database::open(&path).unwrap();
    db.0.lock()
        .unwrap()
        .execute(
            "INSERT INTO projects VALUES (?1,?2,?3,?4)",
            params!["project", "/fixture", "base", "now"],
        )
        .unwrap();
    let action = db
        .create_action("project", "objective", "codex", "model", "action-key")
        .unwrap();
    db.0.lock()
        .unwrap()
        .execute(
            "UPDATE actions SET objective='tampered' WHERE id=?1",
            [&action.id],
        )
        .unwrap();
    assert!(matches!(db.actions(), Err(Error::Invalid(_))));
    drop(db);
    let _ = std::fs::remove_file(path);
}

#[test]
fn project_retry_keeps_first_base_after_repository_advances() {
    let root = std::env::temp_dir().join(format!("factory-project-test-{} ", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let git = |arguments: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(arguments)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q"]);
    git(&["config", "user.name", "Fixture"]);
    git(&["config", "user.email", "fixture@example.invalid"]);
    std::fs::write(root.join("content"), "one").unwrap();
    git(&["add", "content"]);
    git(&["commit", "-qm", "first"]);
    std::fs::create_dir(root.join("nested")).unwrap();
    let db = Database::open(&root.join("factory.sqlite")).unwrap();
    let first = db.register_project(&root, "same-request").unwrap();
    assert_eq!(first.repository, root.to_string_lossy());
    std::fs::write(root.join("content"), "two").unwrap();
    git(&["add", "content"]);
    git(&["commit", "-qm", "second"]);
    let replayed = db
        .register_project(&root.join("nested"), "same-request")
        .unwrap();
    assert_eq!(first.id, replayed.id);
    assert_eq!(first.base_commit, replayed.base_commit);
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}
