package store

import (
	"context"
	"encoding/json"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

func openRunTestDB(t *testing.T) *DB {
	t.Helper()
	db, err := Open(filepath.Join(t.TempDir(), "s2.db"))
	if err != nil {
		t.Fatalf("open: %v", err)
	}
	t.Cleanup(func() { _ = db.Close() })
	return db
}

func queueRun(t *testing.T, db *DB, maxAttempts int) domain.Run {
	t.Helper()
	params, err := domain.NewRunParams{
		WorkloadKind:  domain.SyntheticEchoKind,
		WorkloadInput: map[string]any{"message": "probe"},
		MaxAttempts:   maxAttempts,
	}.Validate()
	if err != nil {
		t.Fatalf("validate: %v", err)
	}
	run, _, err := db.CreateRun(context.Background(), params, "corr-1")
	if err != nil {
		t.Fatalf("create run: %v", err)
	}
	if run.Status != domain.RunQueued {
		t.Fatalf("status = %s, want queued", run.Status)
	}
	return run
}

func registerWorker(t *testing.T, db *DB, id string) domain.Worker {
	t.Helper()
	w, _, err := db.RegisterWorker(context.Background(), id, "test-1", domain.AllCapabilities, []string{"restricted", "trusted-local"}, "corr-1")
	if err != nil {
		t.Fatalf("register: %v", err)
	}
	return w
}

func TestClaimHeartbeatReportLifecycle(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queueRun(t, db, 3)
	registerWorker(t, db, "local-1")

	claimed, attempt, found, _, err := db.ClaimNextRun(ctx, "local-1", "corr-2")
	if err != nil || !found {
		t.Fatalf("claim: found=%v err=%v", found, err)
	}
	if claimed.Status != domain.RunLeased || attempt.Status != domain.AttemptLeased {
		t.Fatalf("claimed=%s attempt=%s", claimed.Status, attempt.Status)
	}
	if claimed.LeaseGeneration != 1 || attempt.LeaseGeneration != 1 {
		t.Fatalf("generation not 1: %+v", claimed)
	}

	updated, started, err := db.HeartbeatLease(ctx, "local-1", run.ID, attempt.ID, 1, "corr-3")
	if err != nil || started == nil {
		t.Fatalf("first heartbeat: err=%v started=%v", err, started)
	}
	if updated.Status != domain.RunRunning || started.EventType != domain.EventRunStarted {
		t.Fatalf("not started: %+v", updated)
	}

	// Renewal extends the lease without a new event.
	updated2, again, err := db.HeartbeatLease(ctx, "local-1", run.ID, attempt.ID, 1, "corr-4")
	if err != nil || again != nil {
		t.Fatalf("renewal: err=%v event=%v", err, again)
	}
	if updated2.LeaseExpiresAt.Before(updated.LeaseExpiresAt) {
		t.Fatal("renewal did not extend the lease")
	}

	final, replayed, _, err := db.ReportAttempt(ctx, "local-1", run.ID, attempt.ID, 1,
		domain.OutcomeCompleted, map[string]any{"output": "ok"}, "", "corr-5")
	if err != nil || replayed {
		t.Fatalf("report: err=%v replayed=%v", err, replayed)
	}
	if final.Status != domain.RunSucceeded {
		t.Fatalf("status = %s, want succeeded", final.Status)
	}

	// Duplicate delivery replays without a new event or state change.
	before, _ := db.CountEvents()
	dup, replayed, _, err := db.ReportAttempt(ctx, "local-1", run.ID, attempt.ID, 1,
		domain.OutcomeCompleted, map[string]any{"output": "ok"}, "", "corr-6")
	if err != nil || !replayed {
		t.Fatalf("duplicate: err=%v replayed=%v", err, replayed)
	}
	after, _ := db.CountEvents()
	if after != before || dup.Status != domain.RunSucceeded {
		t.Fatalf("duplicate changed state: events %d->%d", before, after)
	}
}

func TestStaleGenerationRejected(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queueRun(t, db, 3)
	registerWorker(t, db, "local-1")

	_, attempt, _, _, err := db.ClaimNextRun(ctx, "local-1", "corr-1")
	if err != nil {
		t.Fatalf("claim: %v", err)
	}
	if _, _, err := db.HeartbeatLease(ctx, "local-1", run.ID, attempt.ID, 999, "corr-2"); err != domain.ErrStaleGeneration {
		t.Fatalf("wrong generation: err=%v", err)
	}
	if _, _, _, err := db.ReportAttempt(ctx, "local-1", run.ID, attempt.ID, 999,
		domain.OutcomeCompleted, nil, "", "corr-3"); err != domain.ErrStaleGeneration {
		t.Fatalf("wrong generation report: err=%v", err)
	}
}

func TestUnknownWorkerRejected(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	queueRun(t, db, 1)
	if _, _, _, _, err := db.ClaimNextRun(ctx, "ghost", "corr-1"); err != domain.ErrNotFound {
		t.Fatalf("unknown worker claim: err=%v", err)
	}
	if _, _, err := db.HeartbeatLease(ctx, "ghost", "nope", "nope", 1, "corr-1"); err != domain.ErrNotFound {
		t.Fatalf("unknown worker heartbeat: err=%v", err)
	}
}

func TestExpiryRequeuesAndExhausts(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	base := time.Now().UTC()
	current := base
	db.SetClock(func() time.Time { return current })
	db.SetLeaseTTL(50 * time.Millisecond)

	run := queueRun(t, db, 2)
	registerWorker(t, db, "local-1")
	_, attempt1, _, _, err := db.ClaimNextRun(ctx, "local-1", "corr-1")
	if err != nil {
		t.Fatalf("claim: %v", err)
	}
	// Let the lease lapse, then sweep.
	current = base.Add(200 * time.Millisecond)
	events, err := db.ExpireLeases(ctx)
	if err != nil {
		t.Fatalf("expire: %v", err)
	}
	if len(events) != 1 || events[0].EventType != domain.EventRunExpired {
		t.Fatalf("expected one RunExpired, got %+v", events)
	}
	requeued, err := db.GetRun(run.ID)
	if err != nil || requeued.Status != domain.RunQueued {
		t.Fatalf("run not requeued: %+v err=%v", requeued, err)
	}
	attempts, _ := db.ListAttempts(run.ID)
	if attempts[0].Status != domain.AttemptExpired {
		t.Fatalf("attempt not expired: %+v", attempts[0])
	}

	// The stale Worker can no longer commit for the old generation.
	if _, _, _, err := db.ReportAttempt(ctx, "local-1", run.ID, attempt1.ID, 1,
		domain.OutcomeCompleted, nil, "", "corr-2"); err != domain.ErrStaleGeneration && err != domain.ErrLeaseExpired {
		t.Fatalf("stale report not fenced: err=%v", err)
	}

	// Second attempt exhausts the retry budget.
	_, _, _, _, err = db.ClaimNextRun(ctx, "local-1", "corr-3")
	if err != nil {
		t.Fatalf("reclaim: %v", err)
	}
	current = current.Add(time.Second)
	if _, err := db.ExpireLeases(ctx); err != nil {
		t.Fatalf("expire2: %v", err)
	}
	exhausted, _ := db.GetRun(run.ID)
	if exhausted.Status != domain.RunFailed {
		t.Fatalf("status = %s, want failed after exhaustion", exhausted.Status)
	}
}

func TestCancelRacesCompletion(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queueRun(t, db, 3)
	registerWorker(t, db, "local-1")
	_, attempt, _, _, err := db.ClaimNextRun(ctx, "local-1", "corr-1")
	if err != nil {
		t.Fatalf("claim: %v", err)
	}
	cancelled, _, err := db.CancelRun(ctx, run.ID, "corr-2")
	if err != nil || cancelled.Status != domain.RunCancelled {
		t.Fatalf("cancel: %+v err=%v", cancelled, err)
	}
	if _, _, _, err := db.ReportAttempt(ctx, "local-1", run.ID, attempt.ID, 1,
		domain.OutcomeCompleted, nil, "", "corr-3"); err != domain.ErrRunCancelled {
		t.Fatalf("late completion after cancel: err=%v", err)
	}
	if _, _, err := db.HeartbeatLease(ctx, "local-1", run.ID, attempt.ID, 1, "corr-4"); err != domain.ErrRunCancelled {
		t.Fatalf("heartbeat after cancel: err=%v", err)
	}
	final, _ := db.GetRun(run.ID)
	if final.Status != domain.RunCancelled {
		t.Fatalf("cancel overwritten: %s", final.Status)
	}
}

func TestReconcileOnBoot(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queueRun(t, db, 2)
	registerWorker(t, db, "local-1")
	if _, _, _, _, err := db.ClaimNextRun(ctx, "local-1", "corr-1"); err != nil {
		t.Fatalf("claim: %v", err)
	}
	events, err := db.ReconcileOnBoot(ctx, "boot-1")
	if err != nil {
		t.Fatalf("reconcile: %v", err)
	}
	if len(events) != 2 {
		t.Fatalf("expected RunExpired + WorkerOffline, got %+v", events)
	}
	recovered, _ := db.GetRun(run.ID)
	if recovered.Status != domain.RunQueued {
		t.Fatalf("run not requeued after restart: %s", recovered.Status)
	}
	w, _ := db.GetWorker("local-1")
	if w.Status != domain.WorkerOffline {
		t.Fatalf("worker not offline after restart: %s", w.Status)
	}
}

func TestWorkerLiveness(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	base := time.Now().UTC()
	current := base
	db.SetClock(func() time.Time { return current })
	db.SetWorkerOfflineAfter(100 * time.Millisecond)

	registerWorker(t, db, "local-1")
	current = base.Add(time.Second)
	events, err := db.ExpireLeases(ctx)
	if err != nil {
		t.Fatalf("expire: %v", err)
	}
	if len(events) != 1 || events[0].EventType != domain.EventWorkerOffline {
		t.Fatalf("expected WorkerOffline, got %+v", events)
	}
	// Re-registration reconnects and emits again (not a silent replay).
	_, reconnected, err := db.RegisterWorker(ctx, "local-1", "test-2", domain.AllCapabilities, []string{"restricted", "trusted-local"}, "corr-2")
	if err != nil || reconnected == nil {
		t.Fatalf("reconnect: event=%v err=%v", reconnected, err)
	}
}

func TestReportRequiresRunningAttempt(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queueRun(t, db, 3)
	registerWorker(t, db, "local-1")
	_, attempt, _, _, err := db.ClaimNextRun(ctx, "local-1", "corr-1")
	if err != nil {
		t.Fatalf("claim: %v", err)
	}
	// No heartbeat yet: the Attempt never started, so a completion
	// report must be rejected instead of skipping the running state.
	if _, _, _, err := db.ReportAttempt(ctx, "local-1", run.ID, attempt.ID, 1,
		domain.OutcomeCompleted, nil, "", "corr-2"); err != domain.ErrAttemptNotActive {
		t.Fatalf("report before start: err=%v, want ErrAttemptNotActive", err)
	}
	if _, _, err := db.HeartbeatLease(ctx, "local-1", run.ID, attempt.ID, 1, "corr-3"); err != nil {
		t.Fatalf("heartbeat: %v", err)
	}
	final, _, _, err := db.ReportAttempt(ctx, "local-1", run.ID, attempt.ID, 1,
		domain.OutcomeCompleted, map[string]any{"output": "ok"}, "", "corr-4")
	if err != nil || final.Status != domain.RunSucceeded {
		t.Fatalf("report after start: %+v err=%v", final, err)
	}
	if final.LeaseOwner != "" {
		t.Fatalf("terminal run retains lease owner %q", final.LeaseOwner)
	}
}

func TestCancelledRunRejectsUnknownAttempt(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queueRun(t, db, 1)
	registerWorker(t, db, "local-1")
	if _, _, err := db.CancelRun(ctx, run.ID, "corr-1"); err != nil {
		t.Fatalf("cancel: %v", err)
	}
	// The Run was cancelled before any Attempt existed. Any message for
	// it — even with an unknown Attempt identity — is a cancellation
	// rejection, not a missing resource.
	if _, _, err := db.HeartbeatLease(ctx, "local-1", run.ID, "bogus-attempt", 1, "corr-2"); err != domain.ErrRunCancelled {
		t.Fatalf("heartbeat on cancelled run: err=%v, want ErrRunCancelled", err)
	}
	if _, _, _, err := db.ReportAttempt(ctx, "local-1", run.ID, "bogus-attempt", 1,
		domain.OutcomeCompleted, nil, "", "corr-3"); err != domain.ErrRunCancelled {
		t.Fatalf("report on cancelled run: err=%v, want ErrRunCancelled", err)
	}
}

func TestEmptyPollRecordsLiveness(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	base := time.Now().UTC()
	current := base
	db.SetClock(func() time.Time { return current })
	db.SetWorkerOfflineAfter(100 * time.Millisecond)

	registerWorker(t, db, "idle-9")
	// Advance past the offline threshold, then poll an empty queue: the
	// poll itself must renew liveness so the sweeper keeps the Worker
	// online instead of marking it offline.
	current = base.Add(time.Second)
	if _, _, found, _, err := db.ClaimNextRun(ctx, "idle-9", "corr-1"); err != nil || found {
		t.Fatalf("empty poll: found=%v err=%v", found, err)
	}
	events, err := db.ExpireLeases(ctx)
	if err != nil {
		t.Fatalf("expire: %v", err)
	}
	for _, e := range events {
		if e.EventType == domain.EventWorkerOffline {
			t.Fatalf("idle polling worker marked offline: %+v", e)
		}
	}
	w, err := db.GetWorker("idle-9")
	if err != nil || w.Status != domain.WorkerOnline {
		t.Fatalf("worker not online: %+v err=%v", w, err)
	}
	if w.LastHeartbeat.Before(base.Add(time.Second)) {
		t.Fatalf("empty poll did not renew heartbeat: %v", w.LastHeartbeat)
	}
}

func TestExhaustionClearsLeaseOwner(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	base := time.Now().UTC()
	current := base
	db.SetClock(func() time.Time { return current })
	db.SetLeaseTTL(50 * time.Millisecond)

	run := queueRun(t, db, 1)
	registerWorker(t, db, "local-1")
	if _, _, _, _, err := db.ClaimNextRun(ctx, "local-1", "corr-1"); err != nil {
		t.Fatalf("claim: %v", err)
	}
	current = base.Add(time.Second)
	if _, err := db.ExpireLeases(ctx); err != nil {
		t.Fatalf("expire: %v", err)
	}
	exhausted, err := db.GetRun(run.ID)
	if err != nil || exhausted.Status != domain.RunFailed {
		t.Fatalf("run not failed: %+v err=%v", exhausted, err)
	}
	if exhausted.LeaseOwner != "" {
		t.Fatalf("exhausted run retains lease owner %q", exhausted.LeaseOwner)
	}
}

func TestReportReasonRedactedAtPersistence(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queueRun(t, db, 1)
	registerWorker(t, db, "local-1")

	_, attempt, found, _, err := db.ClaimNextRun(ctx, "local-1", "corr-claim")
	if err != nil || !found {
		t.Fatalf("claim: found=%v err=%v", found, err)
	}
	if _, _, err := db.HeartbeatLease(ctx, "local-1", run.ID, attempt.ID, 1, "corr-hb"); err != nil {
		t.Fatalf("heartbeat: %v", err)
	}
	final, _, events, err := db.ReportAttempt(ctx, "local-1", run.ID, attempt.ID, 1,
		domain.OutcomeFailed, nil, "provider said Bearer abc123 with token=xyz", "corr-report")
	if err != nil {
		t.Fatalf("report: %v", err)
	}
	payload, _ := json.Marshal(events)
	for _, text := range []string{final.Error, string(payload)} {
		if strings.Contains(text, "abc123") || strings.Contains(text, "token=xyz") {
			t.Fatalf("secret shape persisted in report reason: %q", text)
		}
	}
	if !strings.Contains(final.Error, "provider said") {
		t.Fatalf("ordinary reason text lost: %q", final.Error)
	}
}
