package store

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

func queuePaseoRun(t *testing.T, db *DB, maxAttempts int) domain.Run {
	t.Helper()
	params, err := domain.NewRunParams{
		WorkloadKind:  domain.PaseoTaskKind,
		WorkloadInput: map[string]any{"task": "add a multiply function"},
		ExecutionMode: string(domain.ExecTrustedLocal),
		Provider:      "codex",
		Model:         "gpt-6-luna",
		PaseoMode:     "full-access",
		MaxAttempts:   maxAttempts,
	}.Validate()
	if err != nil {
		t.Fatalf("validate: %v", err)
	}
	run, _, err := db.CreateRun(context.Background(), params, "corr-1")
	if err != nil {
		t.Fatalf("create run: %v", err)
	}
	if run.ExecutionMode != string(domain.ExecTrustedLocal) || run.Provider != "codex" ||
		run.Model != "gpt-6-luna" || run.PaseoMode != "full-access" {
		t.Fatalf("routing not persisted: %+v", run)
	}
	return run
}

func claimAndStart(t *testing.T, db *DB, run domain.Run, worker string) domain.Attempt {
	t.Helper()
	ctx := context.Background()
	_, attempt, found, _, err := db.ClaimNextRun(ctx, worker, "corr-claim")
	if err != nil || !found {
		t.Fatalf("claim: found=%v err=%v", found, err)
	}
	if _, _, err := db.HeartbeatLease(ctx, worker, run.ID, attempt.ID, attempt.LeaseGeneration, "corr-hb"); err != nil {
		t.Fatalf("heartbeat: %v", err)
	}
	return attempt
}

func TestPaseoIntendBindCloseLifecycle(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queuePaseoRun(t, db, 3)
	registerWorker(t, db, "local-1")
	attempt := claimAndStart(t, db, run, "local-1")

	intended, replayed, event, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1, "corr-intend")
	if err != nil || replayed {
		t.Fatalf("intend: replayed=%v err=%v", replayed, err)
	}
	if event.EventType != domain.EventPaseoSessionIntended {
		t.Fatalf("event = %s, want PaseoSessionIntended", event.EventType)
	}
	if intended.Status != domain.PaseoIntended || intended.SessionID != "" {
		t.Fatalf("bad intended row: %+v", intended)
	}
	if intended.Provider != "codex" || intended.Model != "gpt-6-luna" || intended.Mode != "full-access" {
		t.Fatalf("routing not frozen: %+v", intended)
	}

	// Duplicate intend replays without a new event.
	before, _ := db.CountEvents()
	dup, replayed, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1, "corr-dup")
	if err != nil || !replayed {
		t.Fatalf("duplicate intend: replayed=%v err=%v", replayed, err)
	}
	after, _ := db.CountEvents()
	if after != before || dup.Status != domain.PaseoIntended {
		t.Fatalf("duplicate intend changed state: events %d->%d", before, after)
	}

	bound, replayed, event, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"ws_test_1", "7632c120-167c-4459-a394-e1ed0f7cc524", "corr-bind")
	if err != nil || replayed {
		t.Fatalf("bind: replayed=%v err=%v", replayed, err)
	}
	if event.EventType != domain.EventPaseoSessionActive {
		t.Fatalf("event = %s, want PaseoSessionActive", event.EventType)
	}
	if bound.Status != domain.PaseoActive || bound.WorkspaceID != "ws_test_1" {
		t.Fatalf("bad bound row: %+v", bound)
	}

	// Identical re-bind replays; a different session for the same
	// generation is fenced.
	before, _ = db.CountEvents()
	rebound, replayed, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"ws_test_1", "7632c120-167c-4459-a394-e1ed0f7cc524", "corr-rebind")
	if err != nil || !replayed {
		t.Fatalf("duplicate bind: replayed=%v err=%v", replayed, err)
	}
	after, _ = db.CountEvents()
	if after != before || rebound.SessionID != bound.SessionID {
		t.Fatalf("duplicate bind changed state: events %d->%d", before, after)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"ws_other", "00000000-0000-0000-0000-000000000000", "corr-sub"); err != domain.ErrStaleGeneration {
		t.Fatalf("substituted bind not fenced: err=%v", err)
	}

	closed, replayed, event, err := db.ClosePaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"succeeded", "", "corr-close")
	if err != nil || replayed {
		t.Fatalf("close: replayed=%v err=%v", replayed, err)
	}
	if event.EventType != domain.EventPaseoSessionSucceeded || closed.Status != domain.PaseoSucceeded {
		t.Fatalf("bad close: %+v event=%s", closed, event.EventType)
	}

	// Duplicate close replays; a conflicting outcome is rejected.
	before, _ = db.CountEvents()
	redup, replayed, _, err := db.ClosePaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"succeeded", "", "corr-redup")
	if err != nil || !replayed {
		t.Fatalf("duplicate close: replayed=%v err=%v", replayed, err)
	}
	after, _ = db.CountEvents()
	if after != before || redup.Status != domain.PaseoSucceeded {
		t.Fatalf("duplicate close changed state")
	}
	if _, _, _, err := db.ClosePaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"failed", "late", "corr-conflict"); err != domain.ErrStaleGeneration {
		t.Fatalf("conflicting close not fenced: err=%v", err)
	}
}

func TestPaseoSyntheticRunRejectsSession(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queueRun(t, db, 3)
	registerWorker(t, db, "local-1")
	_, attempt, _, _, err := db.ClaimNextRun(ctx, "local-1", "corr-1")
	if err != nil {
		t.Fatalf("claim: %v", err)
	}
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1, "corr-2"); err == nil {
		t.Fatal("expected synthetic run to reject session intend")
	}
}

func TestPaseoBindRequiresStartAndIntent(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queuePaseoRun(t, db, 3)
	registerWorker(t, db, "local-1")
	_, attempt, _, _, err := db.ClaimNextRun(ctx, "local-1", "corr-1")
	if err != nil {
		t.Fatalf("claim: %v", err)
	}
	// Bind without intend.
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"ws_1", "session-1", "corr-2"); err == nil {
		t.Fatal("expected bind without intend to fail")
	}
	// Intend, then bind before the Attempt started (no heartbeat yet).
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1, "corr-3"); err != nil {
		t.Fatalf("intend on leased attempt: %v", err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"ws_1", "session-1", "corr-4"); err != domain.ErrAttemptNotActive {
		t.Fatalf("bind before start: err=%v, want ErrAttemptNotActive", err)
	}
	// Bad identities rejected at the boundary.
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"", "session-1", "corr-5"); err == nil {
		t.Fatal("expected empty workspace to fail")
	}
	if _, _, _, err := db.ClosePaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"bogus", "", "corr-6"); err == nil {
		t.Fatal("expected unknown outcome to fail")
	}
	// Succeeding a session that was never bound is invalid; failing it
	// (startup failure) and cancelling it are recorded honestly.
	if _, _, _, err := db.ClosePaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"succeeded", "", "corr-7"); err == nil {
		t.Fatal("expected succeed-without-bind to fail")
	}
	failed, _, event, err := db.ClosePaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"failed", "provider unavailable", "corr-8")
	if err != nil || failed.Status != domain.PaseoFailed {
		t.Fatalf("startup failure not recorded: %+v err=%v", failed, err)
	}
	if event.EventType != domain.EventPaseoSessionFailed {
		t.Fatalf("event = %s, want PaseoSessionFailed", event.EventType)
	}
}

func TestPaseoStaleGenerationFenced(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queuePaseoRun(t, db, 3)
	registerWorker(t, db, "local-1")
	attempt := claimAndStart(t, db, run, "local-1")
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1, "corr-1"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, 999, "corr-2"); err != domain.ErrStaleGeneration {
		t.Fatalf("wrong generation intend: err=%v", err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, 999,
		"ws_1", "session-1", "corr-3"); err != domain.ErrStaleGeneration {
		t.Fatalf("wrong generation bind: err=%v", err)
	}
	if _, _, _, err := db.ClosePaseoSession(ctx, "local-1", run.ID, attempt.ID, 999,
		"failed", "x", "corr-4"); err != domain.ErrStaleGeneration {
		t.Fatalf("wrong generation close: err=%v", err)
	}
	if _, _, _, err := db.MarkWorkerSessionOrphaned(ctx, "local-1", run.ID, attempt.ID, 999,
		"x", "corr-5"); err != domain.ErrStaleGeneration {
		t.Fatalf("wrong generation orphan: err=%v", err)
	}
}

func TestPaseoCancelRunOrphansBinding(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()

	// Active binding (session proven to exist): Core cancellation must not
	// claim the session stopped. The binding is orphaned — discoverable
	// for later stop/cleanup — while the Run outcome stays authoritative.
	run := queuePaseoRun(t, db, 3)
	registerWorker(t, db, "local-1")
	attempt := claimAndStart(t, db, run, "local-1")
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1, "corr-1"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"ws_1", "session-1", "corr-2"); err != nil {
		t.Fatalf("bind: %v", err)
	}
	cancelled, events, err := db.CancelRun(ctx, run.ID, "corr-cancel")
	if err != nil || cancelled.Status != domain.RunCancelled {
		t.Fatalf("cancel: %+v err=%v", cancelled, err)
	}
	if len(events) != 2 {
		t.Fatalf("expected RunCancelled + PaseoSessionOrphaned, got %d events", len(events))
	}
	types := map[string]bool{}
	for _, e := range events {
		types[e.EventType] = true
	}
	if !types[domain.EventRunCancelled] || !types[domain.EventPaseoSessionOrphaned] {
		t.Fatalf("missing cancel events: %v", types)
	}
	if types[domain.EventPaseoSessionCancelled] {
		t.Fatalf("Core cancellation must never claim the session cancelled/stopped")
	}
	binding, err := db.GetPaseoBinding(attempt.ID)
	if err != nil || binding.Status != domain.PaseoOrphaned {
		t.Fatalf("binding not orphaned: %+v err=%v", binding, err)
	}
	if !strings.Contains(binding.Error, "unconfirmed") {
		t.Fatalf("orphan reason must record the unconfirmed stop: %q", binding.Error)
	}
	orphans, err := db.ListOrphanedBindings(10)
	if err != nil || len(orphans) != 1 || orphans[0].SessionID != "session-1" {
		t.Fatalf("cancelled session not discoverable for cleanup: %+v err=%v", orphans, err)
	}
}

func TestPaseoCancelRunOrphansIntendedBinding(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queuePaseoRun(t, db, 3)
	registerWorker(t, db, "local-1")
	_, attempt, _, _, err := db.ClaimNextRun(ctx, "local-1", "corr-1")
	if err != nil {
		t.Fatalf("claim: %v", err)
	}
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1, "corr-2"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	if _, _, err := db.CancelRun(ctx, run.ID, "corr-cancel"); err != nil {
		t.Fatalf("cancel: %v", err)
	}
	binding, err := db.GetPaseoBinding(attempt.ID)
	if err != nil || binding.Status != domain.PaseoOrphaned {
		t.Fatalf("intended binding not orphaned: %+v err=%v", binding, err)
	}
	if !strings.Contains(binding.Error, "before session bind") {
		t.Fatalf("orphan reason must record that no session was bound: %q", binding.Error)
	}
	// Any late Worker message for the cancelled Run is a cancellation
	// rejection, even with an unknown Attempt identity.
	if _, _, _, err := db.ClosePaseoSession(ctx, "local-1", run.ID, "bogus-attempt", 1,
		"succeeded", "", "corr-3"); err != domain.ErrRunCancelled {
		t.Fatalf("close after cancel: err=%v, want ErrRunCancelled", err)
	}
}

func TestPaseoExpiryOrphansBinding(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	base := time.Now().UTC()
	current := base
	db.SetClock(func() time.Time { return current })
	db.SetLeaseTTL(50 * time.Millisecond)

	run := queuePaseoRun(t, db, 2)
	registerWorker(t, db, "local-1")
	_, attempt, _, _, err := db.ClaimNextRun(ctx, "local-1", "corr-1")
	if err != nil {
		t.Fatalf("claim: %v", err)
	}
	if _, _, err := db.HeartbeatLease(ctx, "local-1", run.ID, attempt.ID, 1, "corr-2"); err != nil {
		t.Fatalf("heartbeat: %v", err)
	}
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1, "corr-3"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"ws_1", "session-1", "corr-4"); err != nil {
		t.Fatalf("bind: %v", err)
	}

	current = base.Add(200 * time.Millisecond)
	events, err := db.ExpireLeases(ctx)
	if err != nil {
		t.Fatalf("expire: %v", err)
	}
	types := map[string]bool{}
	for _, e := range events {
		types[e.EventType] = true
	}
	if !types[domain.EventRunExpired] || !types[domain.EventPaseoSessionOrphaned] {
		t.Fatalf("expected RunExpired + PaseoSessionOrphaned, got %v", types)
	}
	binding, err := db.GetPaseoBinding(attempt.ID)
	if err != nil || binding.Status != domain.PaseoOrphaned {
		t.Fatalf("binding not orphaned: %+v err=%v", binding, err)
	}
	// The orphaned generation can never become authoritative.
	if _, _, _, err := db.ClosePaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"succeeded", "", "corr-5"); err != domain.ErrStaleGeneration {
		t.Fatalf("close of orphaned generation: err=%v, want stale", err)
	}
	orphans, err := db.ListOrphanedBindings(10)
	if err != nil || len(orphans) != 1 || orphans[0].AttemptID != attempt.ID {
		t.Fatalf("orphan listing wrong: %+v err=%v", orphans, err)
	}
}

func TestPaseoReconcileOnBootOrphansBinding(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queuePaseoRun(t, db, 2)
	registerWorker(t, db, "local-1")
	attempt := claimAndStart(t, db, run, "local-1")
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1, "corr-1"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	events, err := db.ReconcileOnBoot(ctx, "boot-1")
	if err != nil {
		t.Fatalf("reconcile: %v", err)
	}
	types := map[string]bool{}
	for _, e := range events {
		types[e.EventType] = true
	}
	if !types[domain.EventRunExpired] || !types[domain.EventPaseoSessionOrphaned] {
		t.Fatalf("expected RunExpired + PaseoSessionOrphaned after restart, got %v", types)
	}
}

func TestPaseoTimeoutOrphansBinding(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	base := time.Now().UTC()
	current := base
	db.SetClock(func() time.Time { return current })
	// A long lease TTL isolates the timeout path: the lease must still be
	// valid when the execution timeout fires. (The TTL applies to future
	// grants/renewals, so it is set before claiming.)
	db.SetLeaseTTL(time.Hour)

	run := queuePaseoRun(t, db, 1)
	registerWorker(t, db, "local-1")
	attempt := claimAndStart(t, db, run, "local-1")
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1, "corr-1"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"ws_t", "session-t-1", "corr-2"); err != nil {
		t.Fatalf("bind: %v", err)
	}

	// Advance past timeoutSecs (default 120s) while the lease TTL is long
	// enough that expiry does not fire first.
	current = base.Add(130 * time.Second)
	events, err := db.ExpireLeases(ctx)
	if err != nil {
		t.Fatalf("expire: %v", err)
	}
	types := map[string]bool{}
	for _, e := range events {
		types[e.EventType] = true
	}
	if !types[domain.EventRunFailed] || !types[domain.EventPaseoSessionOrphaned] {
		t.Fatalf("expected RunFailed + PaseoSessionOrphaned on timeout, got %v", types)
	}
	binding, err := db.GetPaseoBinding(attempt.ID)
	if err != nil || binding.Status != domain.PaseoOrphaned {
		t.Fatalf("binding not orphaned after timeout: %+v err=%v", binding, err)
	}
}

func TestPaseoCompletedRequiresClosedBinding(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queuePaseoRun(t, db, 3)
	registerWorker(t, db, "local-1")
	attempt := claimAndStart(t, db, run, "local-1")

	// No binding at all: success without a session is rejected, and the
	// Run stays non-terminal so the Worker can still do the work.
	if _, _, _, err := db.ReportAttempt(ctx, "local-1", run.ID, attempt.ID, 1,
		domain.OutcomeCompleted, map[string]any{"receipt": "forged"}, "", "corr-1"); !errors.Is(err, domain.ErrInvalidInput) {
		t.Fatalf("success without session: err=%v, want ErrInvalidInput", err)
	}
	stored, _ := db.GetRun(run.ID)
	if stored.Status != domain.RunRunning {
		t.Fatalf("run must stay running after rejected success: %s", stored.Status)
	}

	// Intended/active bindings are not success evidence either.
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1, "corr-2"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	if _, _, _, err := db.ReportAttempt(ctx, "local-1", run.ID, attempt.ID, 1,
		domain.OutcomeCompleted, nil, "", "corr-3"); !errors.Is(err, domain.ErrInvalidInput) {
		t.Fatalf("success with intended binding: err=%v, want ErrInvalidInput", err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"ws_x", "session-x-1", "corr-4"); err != nil {
		t.Fatalf("bind: %v", err)
	}
	if _, _, _, err := db.ReportAttempt(ctx, "local-1", run.ID, attempt.ID, 1,
		domain.OutcomeCompleted, nil, "", "corr-5"); !errors.Is(err, domain.ErrInvalidInput) {
		t.Fatalf("success with active binding: err=%v, want ErrInvalidInput", err)
	}

	// A failed (non-succeeded terminal) binding is not success evidence.
	if _, _, _, err := db.ClosePaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"failed", "model blew up", "corr-6"); err != nil {
		t.Fatalf("close failed: %v", err)
	}
	if _, _, _, err := db.ReportAttempt(ctx, "local-1", run.ID, attempt.ID, 1,
		domain.OutcomeCompleted, nil, "", "corr-7"); !errors.Is(err, domain.ErrInvalidInput) {
		t.Fatalf("success with failed binding: err=%v, want ErrInvalidInput", err)
	}

	// The full honest path — intend → bind → close(succeeded) → report —
	// is accepted and stays accepted exactly once.
	run2 := queuePaseoRun(t, db, 3)
	_, attempt2, _, _, err := db.ClaimNextRun(ctx, "local-1", "corr-8")
	if err != nil {
		t.Fatalf("claim: %v", err)
	}
	if _, _, err := db.HeartbeatLease(ctx, "local-1", run2.ID, attempt2.ID, attempt2.LeaseGeneration, "corr-9"); err != nil {
		t.Fatalf("heartbeat: %v", err)
	}
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run2.ID, attempt2.ID, attempt2.LeaseGeneration, "corr-10"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run2.ID, attempt2.ID, attempt2.LeaseGeneration,
		"ws_ok", "session-ok-1", "corr-11"); err != nil {
		t.Fatalf("bind: %v", err)
	}
	if _, _, _, err := db.ClosePaseoSession(ctx, "local-1", run2.ID, attempt2.ID, attempt2.LeaseGeneration,
		"succeeded", "", "corr-12"); err != nil {
		t.Fatalf("close: %v", err)
	}
	final, replayed, events, err := db.ReportAttempt(ctx, "local-1", run2.ID, attempt2.ID, attempt2.LeaseGeneration,
		domain.OutcomeCompleted, map[string]any{"receipt": "ok"}, "", "corr-13")
	if err != nil || replayed || final.Status != domain.RunSucceeded {
		t.Fatalf("honest success rejected: %+v replayed=%v err=%v", final, replayed, err)
	}
	if len(events) != 1 {
		t.Fatalf("expected exactly the completion event, got %d", len(events))
	}
}

func TestPaseoFailedReportSettlesLiveBinding(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()

	// Active binding + failed report: the session may still exist, so it
	// is orphaned for cleanup in the same transaction as the failure.
	run := queuePaseoRun(t, db, 3)
	registerWorker(t, db, "local-1")
	attempt := claimAndStart(t, db, run, "local-1")
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1, "corr-1"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"ws_f", "session-f-1", "corr-2"); err != nil {
		t.Fatalf("bind: %v", err)
	}
	final, _, events, err := db.ReportAttempt(ctx, "local-1", run.ID, attempt.ID, 1,
		domain.OutcomeFailed, nil, "agent errored", "corr-3")
	if err != nil {
		t.Fatalf("failed report: %v", err)
	}
	if final.Status != domain.RunQueued {
		t.Fatalf("run must requeue for retry, got %s", final.Status)
	}
	if len(events) != 2 {
		t.Fatalf("expected orphan + failure events, got %d", len(events))
	}
	if events[0].EventType != domain.EventPaseoSessionOrphaned || events[1].EventType != domain.EventRunFailed {
		t.Fatalf("wrong event order: %s, %s", events[0].EventType, events[1].EventType)
	}
	binding, err := db.GetPaseoBinding(attempt.ID)
	if err != nil || binding.Status != domain.PaseoOrphaned {
		t.Fatalf("binding not orphaned: %+v err=%v", binding, err)
	}

	// Intended-only binding + failed report: the report alone never proves
	// external state (the intend-then-crash window may have created a
	// session Core never saw), so the intent is orphaned for label-based
	// reconciliation rather than cancelled away. The queue holds the
	// requeued run above, so drive whichever run is leased.
	_ = queuePaseoRun(t, db, 3)
	leased2, attempt2, _, _, err := db.ClaimNextRun(ctx, "local-1", "corr-4")
	if err != nil {
		t.Fatalf("claim: %v", err)
	}
	if _, _, err := db.HeartbeatLease(ctx, "local-1", leased2.ID, attempt2.ID, attempt2.LeaseGeneration, "corr-5"); err != nil {
		t.Fatalf("heartbeat: %v", err)
	}
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", leased2.ID, attempt2.ID, attempt2.LeaseGeneration, "corr-6"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	if _, _, events, err := db.ReportAttempt(ctx, "local-1", leased2.ID, attempt2.ID, attempt2.LeaseGeneration,
		domain.OutcomeFailed, nil, "worker gave up", "corr-7"); err != nil {
		t.Fatalf("failed report: %v", err)
	} else if len(events) != 2 || events[0].EventType != domain.EventPaseoSessionOrphaned {
		t.Fatalf("expected orphan + failure events, got %+v", events)
	}
	intended, err := db.GetPaseoBinding(attempt2.ID)
	if err != nil || intended.Status != domain.PaseoOrphaned {
		t.Fatalf("intended binding not orphaned: %+v err=%v", intended, err)
	}

	// No binding at all (S3-A Worker rejection, startup failure): failure
	// records normally with exactly one event. The queue may hold requeued
	// runs from the scenarios above, so drive whichever run is leased.
	_ = queuePaseoRun(t, db, 1)
	leased, attempt3, _, _, err := db.ClaimNextRun(ctx, "local-1", "corr-8")
	if err != nil {
		t.Fatalf("claim: %v", err)
	}
	if _, _, err := db.HeartbeatLease(ctx, "local-1", leased.ID, attempt3.ID, attempt3.LeaseGeneration, "corr-9"); err != nil {
		t.Fatalf("heartbeat: %v", err)
	}
	final3, _, events3, err := db.ReportAttempt(ctx, "local-1", leased.ID, attempt3.ID, attempt3.LeaseGeneration,
		domain.OutcomeFailed, nil, "unsupported workload", "corr-10")
	if err != nil {
		t.Fatalf("bindingless failure: err=%v", err)
	}
	_ = final3
	if len(events3) != 1 {
		t.Fatalf("expected exactly the failure event, got %d", len(events3))
	}
}

func TestPaseoSessionIdCannotSpanAttempts(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	base := time.Now().UTC()
	current := base
	db.SetClock(func() time.Time { return current })
	db.SetLeaseTTL(50 * time.Millisecond)

	run := queuePaseoRun(t, db, 3)
	registerWorker(t, db, "local-1")
	attempt1 := claimAndStart(t, db, run, "local-1")
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt1.ID, 1, "corr-1"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt1.ID, 1,
		"ws_dup", "session-dup-1", "corr-2"); err != nil {
		t.Fatalf("bind: %v", err)
	}

	// Expire the first generation (its binding orphans), then retry: the
	// new generation must create a new session, never re-adopt the old id.
	current = base.Add(time.Second)
	if _, err := db.ExpireLeases(ctx); err != nil {
		t.Fatalf("expire: %v", err)
	}
	// S3-D: a replacement generation waits until the orphaned session is
	// reconciled. Record the verified cleanup receipt first.
	claim, found, err := db.ClaimPaseoRecovery(ctx, "local-1")
	if err != nil || !found {
		t.Fatalf("claim recovery: found=%v err=%v", found, err)
	}
	if _, _, _, err := db.CompletePaseoRecovery(ctx, RecoveryCompletion{
		WorkerID: "local-1", AttemptID: attempt1.ID, ClaimToken: claim.ClaimToken,
		Outcome:   domain.RecoveryOutcomeConfirmed,
		SessionID: "session-dup-1", WorkspaceID: "ws_dup",
		SessionObserved: domain.PaseoObservedArchived, WorkspaceObserved: domain.PaseoObservedAbsent,
	}, "corr-recovery"); err != nil {
		t.Fatalf("complete recovery: %v", err)
	}
	_, attempt2, _, _, err := db.ClaimNextRun(ctx, "local-1", "corr-3")
	if err != nil {
		t.Fatalf("reclaim: %v", err)
	}
	if _, _, err := db.HeartbeatLease(ctx, "local-1", run.ID, attempt2.ID, attempt2.LeaseGeneration, "corr-4"); err != nil {
		t.Fatalf("heartbeat: %v", err)
	}
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt2.ID, attempt2.LeaseGeneration, "corr-5"); err != nil {
		t.Fatalf("intend gen2: %v", err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt2.ID, attempt2.LeaseGeneration,
		"ws_dup", "session-dup-1", "corr-6"); err != domain.ErrStaleGeneration {
		t.Fatalf("duplicate session bind: err=%v, want stale", err)
	}
	// The first generation's binding is untouched by the rejected bind.
	first, err := db.GetPaseoBinding(attempt1.ID)
	if err != nil || first.SessionID != "session-dup-1" || first.Status != domain.PaseoOrphaned {
		t.Fatalf("first binding disturbed: %+v err=%v", first, err)
	}
	// A fresh session id binds fine on the new generation.
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt2.ID, attempt2.LeaseGeneration,
		"ws_dup", "session-dup-2", "corr-7"); err != nil {
		t.Fatalf("fresh bind on gen2: %v", err)
	}

	// Cross-run adoption is rejected the same way. (The requeued first
	// run already holds its gen-2 lease above, so the poll grants runB.)
	runB := queuePaseoRun(t, db, 3)
	_, attemptB, _, _, err := db.ClaimNextRun(ctx, "local-1", "corr-8")
	if err != nil {
		t.Fatalf("claim B: %v", err)
	}
	if attemptB.RunID != runB.ID {
		t.Fatalf("expected runB lease, got run %s", attemptB.RunID)
	}
	if _, _, err := db.HeartbeatLease(ctx, "local-1", runB.ID, attemptB.ID, attemptB.LeaseGeneration, "corr-9"); err != nil {
		t.Fatalf("heartbeat B: %v", err)
	}
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", runB.ID, attemptB.ID, attemptB.LeaseGeneration, "corr-10"); err != nil {
		t.Fatalf("intend B: %v", err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", runB.ID, attemptB.ID, attemptB.LeaseGeneration,
		"ws_dup", "session-dup-1", "corr-11"); err != domain.ErrStaleGeneration {
		t.Fatalf("cross-run duplicate session bind: err=%v, want stale", err)
	}
}

func TestPaseoWorkerCloseCancelledOnUnactedIntent(t *testing.T) {
	// The owning Worker may mark an intent it never acted on cancelled:
	// unlike Core-side cancellation or failure reports, the actor knows
	// no external call was made. This is the only path that records
	// cancelled for a session binding.
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queuePaseoRun(t, db, 3)
	registerWorker(t, db, "local-1")
	_, attempt, _, _, err := db.ClaimNextRun(ctx, "local-1", "corr-1")
	if err != nil {
		t.Fatalf("claim: %v", err)
	}
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1, "corr-2"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	closed, _, event, err := db.ClosePaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"cancelled", "operator aborted before start", "corr-3")
	if err != nil || closed.Status != domain.PaseoCancelled {
		t.Fatalf("worker close-cancelled: %+v err=%v", closed, err)
	}
	if event.EventType != domain.EventPaseoSessionCancelled {
		t.Fatalf("event = %s, want PaseoSessionCancelled", event.EventType)
	}
	orphans, err := db.ListOrphanedBindings(10)
	if err != nil {
		t.Fatalf("orphan list: %v", err)
	}
	for _, o := range orphans {
		if o.AttemptID == attempt.ID {
			t.Fatalf("cleanly cancelled intent must not enter the cleanup queue")
		}
	}
}

func TestPaseoReasonRedactedAtPersistence(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queuePaseoRun(t, db, 3)
	registerWorker(t, db, "local-1")
	attempt := claimAndStart(t, db, run, "local-1")
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1, "corr-1"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"ws_r", "session-r-1", "corr-2"); err != nil {
		t.Fatalf("bind: %v", err)
	}
	// A reason carrying secret shapes must be masked in the durable row
	// and its event; an ordinary reason passes through unchanged.
	closed, _, event, err := db.ClosePaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"failed", "provider said Bearer abc123 with token=xyz", "corr-3")
	if err != nil {
		t.Fatalf("close: %v", err)
	}
	payloadRaw, _ := json.Marshal(event.Payload)
	for _, s := range []string{closed.Error, string(payloadRaw)} {
		if strings.Contains(s, "abc123") || strings.Contains(s, "token=xyz") {
			t.Fatalf("secret shape persisted: %q", s)
		}
	}
	if !strings.Contains(closed.Error, "provider said") {
		t.Fatalf("ordinary reason text lost: %q", closed.Error)
	}
}

func TestPaseoBindingQueries(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queuePaseoRun(t, db, 3)
	registerWorker(t, db, "local-1")
	attempt := claimAndStart(t, db, run, "local-1")
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1, "corr-1"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"ws_q", "session-q-1", "corr-2"); err != nil {
		t.Fatalf("bind: %v", err)
	}
	listed, err := db.ListPaseoBindings(run.ID)
	if err != nil || len(listed) != 1 || listed[0].SessionID != "session-q-1" {
		t.Fatalf("list wrong: %+v err=%v", listed, err)
	}
	bySession, err := db.GetPaseoBindingBySession("session-q-1")
	if err != nil || bySession.AttemptID != attempt.ID {
		t.Fatalf("by-session wrong: %+v err=%v", bySession, err)
	}
	if _, err := db.GetPaseoBindingBySession("no-such-session"); err != domain.ErrNotFound {
		t.Fatalf("unknown session: err=%v, want ErrNotFound", err)
	}
	if _, err := db.GetPaseoBinding("no-such-attempt"); err != domain.ErrNotFound {
		t.Fatalf("unknown attempt: err=%v, want ErrNotFound", err)
	}
	if _, err := db.ListPaseoBindings("no-such-run"); err != domain.ErrNotFound {
		t.Fatalf("unknown run: err=%v, want ErrNotFound", err)
	}
}

func TestPaseoWorkerOrphanRoute(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queuePaseoRun(t, db, 3)
	registerWorker(t, db, "local-1")
	attempt := claimAndStart(t, db, run, "local-1")
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1, "corr-1"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, 1,
		"ws_o", "session-o-1", "corr-2"); err != nil {
		t.Fatalf("bind: %v", err)
	}
	orphaned, replayed, event, err := db.MarkWorkerSessionOrphaned(ctx, "local-1", run.ID, attempt.ID, 1,
		"connection lost", "corr-3")
	if err != nil || replayed {
		t.Fatalf("orphan: replayed=%v err=%v", replayed, err)
	}
	if event.EventType != domain.EventPaseoSessionOrphaned || orphaned.Status != domain.PaseoOrphaned {
		t.Fatalf("bad orphan: %+v event=%s", orphaned, event.EventType)
	}
	// Redundant orphan replays.
	_, replayed, _, err = db.MarkWorkerSessionOrphaned(ctx, "local-1", run.ID, attempt.ID, 1,
		"connection lost", "corr-4")
	if err != nil || !replayed {
		t.Fatalf("duplicate orphan: replayed=%v err=%v", replayed, err)
	}
}

func TestPaseoOldDatabaseMigrates(t *testing.T) {
	path := filepath.Join(t.TempDir(), "old.db")
	db, err := Open(path)
	if err != nil {
		t.Fatalf("open: %v", err)
	}
	// A synthetic Run created before S3-A must survive the migration with
	// the restricted default.
	legacy := queueRun(t, db, 3)
	_ = db.Close()

	db2, err := Open(path)
	if err != nil {
		t.Fatalf("reopen: %v", err)
	}
	defer func() { _ = db2.Close() }()
	stored, err := db2.GetRun(legacy.ID)
	if err != nil || stored.Status != domain.RunQueued {
		t.Fatalf("run lost across reopen: %+v err=%v", stored, err)
	}
	if stored.ExecutionMode != string(domain.ExecRestricted) {
		t.Fatalf("legacy default mode wrong: %q", stored.ExecutionMode)
	}
	if _, err := db2.ListPaseoBindings(legacy.ID); err != nil {
		t.Fatalf("bindings table missing after reopen: %v", err)
	}
}
