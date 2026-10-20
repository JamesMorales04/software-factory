package store

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

func orphanedPaseoBinding(t *testing.T, db *DB, worker string) (domain.Run, domain.Attempt, domain.PaseoBinding, time.Time) {
	t.Helper()
	ctx := context.Background()
	base := time.Now().UTC()
	current := base
	db.SetClock(func() time.Time { return current })
	db.SetLeaseTTL(50 * time.Millisecond)
	run := queuePaseoRun(t, db, 2)
	registerWorker(t, db, worker)
	attempt := claimAndStart(t, db, run, worker)
	if _, _, _, err := db.IntendPaseoSession(ctx, worker, run.ID, attempt.ID, attempt.LeaseGeneration, "corr-intend"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	binding, _, _, err := db.BindPaseoSession(ctx, worker, run.ID, attempt.ID, attempt.LeaseGeneration,
		"ws_recovery", "77777777-7777-4777-8777-777777777777", "corr-bind")
	if err != nil {
		t.Fatalf("bind: %v", err)
	}
	current = base.Add(time.Second)
	if _, err := db.ExpireLeases(ctx); err != nil {
		t.Fatalf("expire: %v", err)
	}
	orphaned, err := db.GetPaseoBinding(attempt.ID)
	if err != nil || orphaned.Status != domain.PaseoOrphaned {
		t.Fatalf("binding not orphaned: %+v err=%v", orphaned, err)
	}
	return run, attempt, binding, current
}

func confirmedCompletion(attempt domain.Attempt, token, sessionID, workspaceID string) RecoveryCompletion {
	return confirmedCompletionFor("local-1", attempt, token, sessionID, workspaceID)
}

func confirmedCompletionFor(worker string, attempt domain.Attempt, token, sessionID, workspaceID string) RecoveryCompletion {
	return RecoveryCompletion{
		WorkerID: worker, AttemptID: attempt.ID, ClaimToken: token,
		Outcome:         domain.RecoveryOutcomeConfirmed,
		SessionID:       sessionID,
		WorkspaceID:     workspaceID,
		SessionObserved: domain.PaseoObservedArchived, WorkspaceObserved: domain.PaseoObservedAbsent,
	}
}

func TestPaseoRecoveryConfirmedReceiptAndRetryGate(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run, attempt, binding, _ := orphanedPaseoBinding(t, db, "local-1")

	// The replacement generation waits while the old session is unresolved.
	if _, _, found, _, err := db.ClaimNextRun(ctx, "local-1", "corr-gated"); err != nil || found {
		t.Fatalf("retry gate: found=%v err=%v", found, err)
	}

	registerWorker(t, db, "local-2")
	claim, found, err := db.ClaimPaseoRecovery(ctx, "local-1")
	if err != nil || !found {
		t.Fatalf("claim recovery: found=%v err=%v", found, err)
	}
	if claim.Session.AttemptID != attempt.ID || claim.ClaimToken == "" {
		t.Fatalf("bad claim: %+v", claim)
	}
	if claim.Session.CorrelationKey != domain.PaseoCorrelationKey(run.ID, attempt.LeaseGeneration) {
		t.Fatalf("correlation key = %q", claim.Session.CorrelationKey)
	}
	if _, found, err := db.ClaimPaseoRecovery(ctx, "local-2"); err != nil || found {
		t.Fatalf("second live claim: found=%v err=%v", found, err)
	}

	before, _ := db.CountEvents()
	receipt, replayed, event, err := db.CompletePaseoRecovery(ctx, confirmedCompletion(attempt, claim.ClaimToken, binding.SessionID, binding.WorkspaceID), "corr-complete")
	if err != nil || replayed {
		t.Fatalf("complete: replayed=%v err=%v", replayed, err)
	}
	if receipt.Status != domain.RecoveryConfirmed || event.EventType != domain.EventPaseoSessionRecoveryRecorded {
		t.Fatalf("bad receipt/event: %+v %s", receipt, event.EventType)
	}
	if _, ok := event.Payload["retryCount"]; !ok {
		t.Fatalf("recovery event lacks retryCount: %+v", event.Payload)
	}
	after, _ := db.CountEvents()
	if after != before+1 {
		t.Fatalf("recovery event count = %d -> %d, want +1", before, after)
	}
	// The immutable binding keeps its original orphan status and reason.
	stored, err := db.GetPaseoBinding(attempt.ID)
	if err != nil || stored.Status != domain.PaseoOrphaned {
		t.Fatalf("binding rewritten: %+v err=%v", stored, err)
	}

	// Identical completion is an idempotent replay with no new event.
	again, replayed, _, err := db.CompletePaseoRecovery(ctx, confirmedCompletion(attempt, claim.ClaimToken, binding.SessionID, binding.WorkspaceID), "corr-replay")
	if err != nil || !replayed || again.Status != domain.RecoveryConfirmed {
		t.Fatalf("replay: replayed=%v err=%v status=%s", replayed, err, again.Status)
	}
	if got, _ := db.CountEvents(); got != after {
		t.Fatalf("replay added an event: %d -> %d", after, got)
	}

	// Now the replacement Attempt may start with a new generation and key.
	_, next, found, _, err := db.ClaimNextRun(ctx, "local-1", "corr-retry")
	if err != nil || !found {
		t.Fatalf("retry after confirmed: found=%v err=%v", found, err)
	}
	if next.LeaseGeneration != attempt.LeaseGeneration+1 {
		t.Fatalf("generation = %d, want %d", next.LeaseGeneration, attempt.LeaseGeneration+1)
	}
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, next.ID, next.LeaseGeneration, "corr-new"); err != nil {
		t.Fatalf("intend new generation: %v", err)
	}
	newBinding, err := db.GetPaseoBinding(next.ID)
	if err != nil {
		t.Fatalf("get new binding: %v", err)
	}
	if newBinding.CorrelationKey == claim.Session.CorrelationKey {
		t.Fatalf("new generation reused the old correlation key %q", newBinding.CorrelationKey)
	}
}

func TestPaseoRecoveryClaimExpiryAndStaleCompletion(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	_, attempt, binding, base := orphanedPaseoBinding(t, db, "local-1")
	registerWorker(t, db, "local-2")

	first, found, err := db.ClaimPaseoRecovery(ctx, "local-1")
	if err != nil || !found {
		t.Fatalf("first claim: found=%v err=%v", found, err)
	}
	db.SetClock(func() time.Time { return base.Add(2 * RecoveryClaimTTL) })
	second, found, err := db.ClaimPaseoRecovery(ctx, "local-2")
	if err != nil || !found {
		t.Fatalf("expired claim should be reclaimable: found=%v err=%v", found, err)
	}
	if second.ClaimToken == first.ClaimToken {
		t.Fatal("reclaim reused the old claim token")
	}
	if _, _, _, err := db.CompletePaseoRecovery(ctx,
		confirmedCompletion(attempt, first.ClaimToken, binding.SessionID, binding.WorkspaceID), "corr-stale"); !errors.Is(err, domain.ErrStaleRecoveryClaim) {
		t.Fatalf("stale completion err = %v, want ErrStaleRecoveryClaim", err)
	}
	if _, _, _, err := db.CompletePaseoRecovery(ctx,
		confirmedCompletionFor("local-2", attempt, second.ClaimToken, binding.SessionID, binding.WorkspaceID), "corr-current"); err != nil {
		t.Fatalf("current completion: %v", err)
	}
}

func TestPaseoRecoveryAbsenceRequiresQuietPeriod(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	_, attempt, _, base := orphanedPaseoBinding(t, db, "local-1")
	absent := RecoveryCompletion{
		WorkerID: "local-1", AttemptID: attempt.ID,
		Outcome:         domain.RecoveryOutcomeAbsent,
		SessionObserved: domain.PaseoObservedAbsent, WorkspaceObserved: domain.PaseoObservedAbsent,
	}
	claim, found, err := db.ClaimPaseoRecovery(ctx, "local-1")
	if err != nil || !found {
		t.Fatalf("claim: found=%v err=%v", found, err)
	}
	absent.ClaimToken = claim.ClaimToken
	first, _, _, err := db.CompletePaseoRecovery(ctx, absent, "corr-a1")
	if err != nil {
		t.Fatalf("first absence: %v", err)
	}
	if first.Status != domain.RecoveryUnconfirmed {
		t.Fatalf("first absence status = %s, want unconfirmed", first.Status)
	}
	db.SetClock(func() time.Time { return base.Add(RecoveryAbsenceQuiet - time.Second) })
	if _, found, err := db.ClaimPaseoRecovery(ctx, "local-1"); err != nil || found {
		t.Fatalf("absence retry before quiet period: found=%v err=%v", found, err)
	}
	db.SetClock(func() time.Time { return base.Add(RecoveryAbsenceQuiet) })
	claim, found, err = db.ClaimPaseoRecovery(ctx, "local-1")
	if err != nil || !found {
		t.Fatalf("second claim: found=%v err=%v", found, err)
	}
	absent.ClaimToken = claim.ClaimToken
	second, _, _, err := db.CompletePaseoRecovery(ctx, absent, "corr-a2")
	if err != nil {
		t.Fatalf("second absence: %v", err)
	}
	if second.Status != domain.RecoveryAbsent {
		t.Fatalf("second absence status = %s, want absent", second.Status)
	}
	// Absence is safe for a replacement Attempt.
	if _, _, found, _, err := db.ClaimNextRun(ctx, "local-1", "corr-retry"); err != nil || !found {
		t.Fatalf("retry after absence: found=%v err=%v", found, err)
	}
}

func TestPaseoRecoveryIdentityConflictAndBlockedRetry(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run, attempt, binding, base := orphanedPaseoBinding(t, db, "local-1")

	// A discovered session owned by another binding is a hard conflict.
	otherRun := queuePaseoRun(t, db, 1)
	otherAttempt := claimAndStart(t, db, otherRun, "local-1")
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", otherRun.ID, otherAttempt.ID, otherAttempt.LeaseGeneration, "corr-other"); err != nil {
		t.Fatalf("other intend: %v", err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", otherRun.ID, otherAttempt.ID, otherAttempt.LeaseGeneration,
		"ws_other", "88888888-8888-4888-8888-888888888888", "corr-other-bind"); err != nil {
		t.Fatalf("other bind: %v", err)
	}

	claim, found, err := db.ClaimPaseoRecovery(ctx, "local-1")
	if err != nil || !found {
		t.Fatalf("claim: found=%v err=%v", found, err)
	}
	if _, _, _, err := db.CompletePaseoRecovery(ctx, confirmedCompletion(attempt, claim.ClaimToken,
		"88888888-8888-4888-8888-888888888888", binding.WorkspaceID), "corr-conflict"); !errors.Is(err, domain.ErrRecoveryIdentityConflict) {
		t.Fatalf("cross-binding completion err = %v, want identity conflict", err)
	}

	// A mismatch against the binding's own recorded ID is also rejected.
	if _, _, _, err := db.CompletePaseoRecovery(ctx, confirmedCompletion(attempt, claim.ClaimToken,
		"99999999-9999-4999-8999-999999999999", binding.WorkspaceID), "corr-conflict2"); !errors.Is(err, domain.ErrRecoveryIdentityConflict) {
		t.Fatalf("wrong identity err = %v, want identity conflict", err)
	}
	// The rejected attempts changed nothing and kept the live claim.
	stored, err := db.GetPaseoRecovery(attempt.ID)
	if err != nil || stored.Status != domain.RecoveryClaimed {
		t.Fatalf("recovery after conflicts: %+v err=%v", stored, err)
	}

	// Drive the bounded retry budget to exhaustion; the Run must then be
	// failed closed, not retried concurrently.
	for i := 0; i < RecoveryMaxAttempts+1; i++ {
		db.SetClock(func() time.Time { return base.Add(time.Duration(i+2) * time.Hour) })
		current, found, cerr := db.ClaimPaseoRecovery(ctx, "local-1")
		if cerr != nil {
			t.Fatalf("retry claim %d: %v", i, cerr)
		}
		if !found {
			continue
		}
		if _, _, _, cerr := db.CompletePaseoRecovery(ctx, RecoveryCompletion{
			WorkerID: "local-1", AttemptID: attempt.ID, ClaimToken: current.ClaimToken,
			Outcome:         domain.RecoveryOutcomeUnconfirmed,
			SessionObserved: domain.PaseoObservedUnknown, WorkspaceObserved: domain.PaseoObservedUnknown,
			Reason: "simulated daemon outage",
		}, "corr-retry"); cerr != nil {
			t.Fatalf("retry complete %d: %v", i, cerr)
		}
	}
	blocked, err := db.GetPaseoRecovery(attempt.ID)
	if err != nil || blocked.Status != domain.RecoveryBlocked {
		t.Fatalf("recovery did not become blocked: %+v err=%v", blocked, err)
	}
	if !strings.HasPrefix(blocked.Reason, "retry-exhausted") {
		t.Fatalf("blocked reason = %q, want retry-exhausted prefix", blocked.Reason)
	}
	events, err := db.ResolveBlockedPaseoRetries(ctx)
	if err != nil {
		t.Fatalf("resolve blocked: %v", err)
	}
	if len(events) != 1 || events[0].EventType != domain.EventRunFailed {
		t.Fatalf("blocked retry events = %+v", events)
	}
	failed, err := db.GetRun(run.ID)
	if err != nil || failed.Status != domain.RunFailed {
		t.Fatalf("run not failed closed: %+v err=%v", failed, err)
	}
	if failed.Error == "" || failed.LeaseOwner != "" {
		t.Fatalf("failed run reason/lease = %q/%q", failed.Error, failed.LeaseOwner)
	}
}

func TestPaseoRecoverySucceededBindingAfterCrashBeforeReport(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	base := time.Now().UTC()
	current := base
	db.SetClock(func() time.Time { return current })
	db.SetLeaseTTL(50 * time.Millisecond)
	run := queuePaseoRun(t, db, 2)
	registerWorker(t, db, "local-1")
	attempt := claimAndStart(t, db, run, "local-1")
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "corr-intend"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration,
		"ws_succ", "cccccccc-1111-4111-8111-cccccccccccc", "corr-bind"); err != nil {
		t.Fatalf("bind: %v", err)
	}
	if _, _, _, err := db.ClosePaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration,
		string(domain.PaseoSucceeded), "", "corr-close"); err != nil {
		t.Fatalf("close: %v", err)
	}
	// Crash before the completed report: the lease expires and the Run
	// requeues, but the session already closed. Cleanup verification is
	// scheduled without rewriting the succeeded binding.
	current = base.Add(time.Second)
	if _, err := db.ExpireLeases(ctx); err != nil {
		t.Fatalf("expire: %v", err)
	}
	queued, err := db.GetRun(run.ID)
	if err != nil || queued.Status != domain.RunQueued {
		t.Fatalf("run status = %+v err=%v, want queued", queued, err)
	}
	binding, err := db.GetPaseoBinding(attempt.ID)
	if err != nil || binding.Status != domain.PaseoSucceeded {
		t.Fatalf("binding rewritten: %+v err=%v", binding, err)
	}
	claim, found, err := db.ClaimPaseoRecovery(ctx, "local-1")
	if err != nil || !found {
		t.Fatalf("claim cleanup: found=%v err=%v", found, err)
	}
	if _, _, _, err := db.CompletePaseoRecovery(ctx, confirmedCompletion(attempt, claim.ClaimToken,
		binding.SessionID, binding.WorkspaceID), "corr-recovery"); err != nil {
		t.Fatalf("complete cleanup: %v", err)
	}
	// The replacement Attempt is allowed; the succeeded binding is the
	// authoritative completion and remains untouched.
	if _, next, found, _, err := db.ClaimNextRun(ctx, "local-1", "corr-retry"); err != nil || !found || next.Seq != 2 {
		t.Fatalf("retry after succeeded cleanup: found=%v err=%v next=%+v", found, err, next)
	}
}

func TestPaseoCompletedCleanupUnconfirmedSchedulesRecovery(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queuePaseoRun(t, db, 1)
	registerWorker(t, db, "local-1")
	attempt := claimAndStart(t, db, run, "local-1")
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "corr-intend"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration,
		"ws_cleanup", "dddddddd-1111-4111-8111-dddddddddddd", "corr-bind"); err != nil {
		t.Fatalf("bind: %v", err)
	}
	if _, _, _, err := db.ClosePaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration,
		string(domain.PaseoSucceeded), "", "corr-close"); err != nil {
		t.Fatalf("close: %v", err)
	}
	completed, _, _, err := db.ReportAttempt(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration,
		domain.OutcomeCompleted, map[string]any{"cleanup": "unconfirmed: archive failed"}, "", "corr-report")
	if err != nil || completed.Status != domain.RunSucceeded {
		t.Fatalf("completed report: %+v err=%v", completed, err)
	}
	claim, found, err := db.ClaimPaseoRecovery(ctx, "local-1")
	if err != nil || !found {
		t.Fatalf("claim cleanup: found=%v err=%v", found, err)
	}
	if claim.Session.Status != domain.PaseoSucceeded {
		t.Fatalf("claim binding status = %s, want succeeded", claim.Session.Status)
	}
	if _, _, _, err := db.CompletePaseoRecovery(ctx, confirmedCompletion(attempt, claim.ClaimToken,
		claim.Session.SessionID, claim.Session.WorkspaceID), "corr-recovery"); err != nil {
		t.Fatalf("complete cleanup: %v", err)
	}
}

func TestPaseoRecoveryCompletionWithoutReceiptIsNotFound(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	run := queuePaseoRun(t, db, 1)
	registerWorker(t, db, "local-1")
	attempt := claimAndStart(t, db, run, "local-1")
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "corr-intend"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration,
		"ws_noreceipt", "eeeeeeee-1111-4111-8111-eeeeeeeeeeee", "corr-bind"); err != nil {
		t.Fatalf("bind: %v", err)
	}
	if _, _, _, err := db.ClosePaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration,
		string(domain.PaseoSucceeded), "", "corr-close"); err != nil {
		t.Fatalf("close: %v", err)
	}
	if _, _, _, err := db.ReportAttempt(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration,
		domain.OutcomeCompleted, map[string]any{"cleanup": "ok"}, "", "corr-report"); err != nil {
		t.Fatalf("report: %v", err)
	}
	if _, _, _, err := db.CompletePaseoRecovery(ctx, confirmedCompletion(attempt, "fabricated",
		"eeeeeeee-1111-4111-8111-eeeeeeeeeeee", "ws_noreceipt"), "corr-bad"); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("completion without receipt err = %v, want ErrNotFound", err)
	}
}

func TestPaseoRecoveryClaimBudgetIsBounded(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	_, attempt, _, base := orphanedPaseoBinding(t, db, "local-1")
	for i := 0; i < RecoveryMaxAttempts; i++ {
		db.SetClock(func() time.Time { return base.Add(time.Duration(i) * (RecoveryClaimTTL + time.Second)) })
		claim, found, err := db.ClaimPaseoRecovery(ctx, "local-1")
		if err != nil || !found {
			t.Fatalf("claim %d: found=%v err=%v", i+1, found, err)
		}
		if claim.Recovery.Attempts != i+1 {
			t.Fatalf("claim %d attempts = %d", i+1, claim.Recovery.Attempts)
		}
	}
	db.SetClock(func() time.Time {
		return base.Add(time.Duration(RecoveryMaxAttempts) * (RecoveryClaimTTL + time.Second))
	})
	if _, found, err := db.ClaimPaseoRecovery(ctx, "local-1"); err != nil || found {
		t.Fatalf("claim beyond budget: found=%v err=%v", found, err)
	}
	blocked, err := db.GetPaseoRecovery(attempt.ID)
	if err != nil || blocked.Status != domain.RecoveryBlocked {
		t.Fatalf("exhausted receipt: %+v err=%v", blocked, err)
	}
	if blocked.Attempts != RecoveryMaxAttempts || !strings.HasPrefix(blocked.Reason, "retry-exhausted") {
		t.Fatalf("exhausted receipt attempts/reason = %d/%q", blocked.Attempts, blocked.Reason)
	}
}

func TestPaseoRetryGateDoesNotStarveLaterRuns(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	base := time.Now().UTC()
	current := base
	db.SetClock(func() time.Time { return current })
	db.SetLeaseTTL(50 * time.Millisecond)
	registerWorker(t, db, "local-1")
	// More gated paseo Runs than the old fixed scan window.
	for i := 0; i < 205; i++ {
		run := queuePaseoRun(t, db, 2)
		attempt := claimAndStart(t, db, run, "local-1")
		if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "corr-intend"); err != nil {
			t.Fatalf("intend %d: %v", i, err)
		}
	}
	current = base.Add(time.Second)
	if _, err := db.ExpireLeases(ctx); err != nil {
		t.Fatalf("expire: %v", err)
	}
	var queuedPaseo int
	if err := db.sql.QueryRow(`SELECT COUNT(*) FROM runs WHERE status=?`, string(domain.RunQueued)).Scan(&queuedPaseo); err != nil {
		t.Fatalf("count queued: %v", err)
	}
	if queuedPaseo != 205 {
		t.Fatalf("queued paseo runs = %d, want 205", queuedPaseo)
	}
	synthetic := queueRun(t, db, 1)
	claimed, _, found, _, err := db.ClaimNextRun(ctx, "local-1", "corr-synthetic")
	if err != nil || !found {
		t.Fatalf("claim synthetic: found=%v err=%v", found, err)
	}
	if claimed.ID != synthetic.ID || claimed.WorkloadKind != domain.SyntheticEchoKind {
		t.Fatalf("gated paseo runs starved the synthetic run: got %s/%s", claimed.ID, claimed.WorkloadKind)
	}
}

func TestPaseoRecoveryConcurrentClaimsHaveOneWinner(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	_, attempt, _, _ := orphanedPaseoBinding(t, db, "local-1")
	registerWorker(t, db, "local-2")
	type result struct {
		claim domain.PaseoRecoveryClaim
		found bool
		err   error
	}
	results := make(chan result, 2)
	start := make(chan struct{})
	for _, worker := range []string{"local-1", "local-2"} {
		go func(workerID string) {
			<-start
			claim, found, err := db.ClaimPaseoRecovery(ctx, workerID)
			results <- result{claim: claim, found: found, err: err}
		}(worker)
	}
	close(start)
	winners := 0
	for i := 0; i < 2; i++ {
		r := <-results
		if r.err != nil {
			t.Fatalf("claim error: %v", r.err)
		}
		if r.found {
			winners++
			if r.claim.Session.AttemptID != attempt.ID || r.claim.ClaimToken == "" {
				t.Fatalf("bad winning claim: %+v", r.claim)
			}
		}
	}
	if winners != 1 {
		t.Fatalf("concurrent claims found=%d, want exactly 1", winners)
	}
}

func TestResolveBlockedPaseoRetriesIsNotStarvedByPendingRows(t *testing.T) {
	db := openRunTestDB(t)
	ctx := context.Background()
	base := time.Now().UTC()
	current := base
	db.SetClock(func() time.Time { return current })
	db.SetLeaseTTL(50 * time.Millisecond)
	registerWorker(t, db, "local-1")

	// 200 older queued paseo Runs remain in pending recovery.
	for i := 0; i < 200; i++ {
		current = base.Add(time.Duration(i) * time.Millisecond)
		run := queuePaseoRun(t, db, 2)
		attempt := claimAndStart(t, db, run, "local-1")
		if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "corr-intend"); err != nil {
			t.Fatalf("intend %d: %v", i, err)
		}
	}
	current = base.Add(500 * time.Millisecond)
	if _, err := db.ExpireLeases(ctx); err != nil {
		t.Fatalf("expire pending runs: %v", err)
	}

	// One later Run has a terminal blocked receipt and must be failed closed.
	current = base.Add(time.Second)
	blockedRun := queuePaseoRun(t, db, 2)
	blockedAttempt := claimAndStart(t, db, blockedRun, "local-1")
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", blockedRun.ID, blockedAttempt.ID, blockedAttempt.LeaseGeneration, "corr-blocked-intend"); err != nil {
		t.Fatalf("blocked intend: %v", err)
	}
	current = base.Add(2 * time.Second)
	if _, err := db.ExpireLeases(ctx); err != nil {
		t.Fatalf("expire blocked run: %v", err)
	}
	if _, err := db.sql.ExecContext(ctx,
		`UPDATE paseo_recovery SET status=?, reason=? WHERE attempt_id=?`,
		string(domain.RecoveryBlocked), "retry-exhausted: deterministic test", blockedAttempt.ID,
	); err != nil {
		t.Fatalf("mark blocked: %v", err)
	}

	events, err := db.ResolveBlockedPaseoRetries(ctx)
	if err != nil {
		t.Fatalf("resolve blocked: %v", err)
	}
	if len(events) != 1 || events[0].EventType != domain.EventRunFailed || events[0].AggregateID != blockedRun.ID {
		t.Fatalf("blocked later Run was not failed closed: %+v", events)
	}
	failed, err := db.GetRun(blockedRun.ID)
	if err != nil || failed.Status != domain.RunFailed || !strings.HasPrefix(failed.Error, "recovery-unresolved") {
		t.Fatalf("blocked run state = %+v err=%v", failed, err)
	}

	var pendingQueued int
	if err := db.sql.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM runs WHERE status=? AND workload_kind=?`,
		string(domain.RunQueued), domain.PaseoTaskKind).Scan(&pendingQueued); err != nil {
		t.Fatalf("count pending queued: %v", err)
	}
	if pendingQueued != 200 {
		t.Fatalf("pending queued paseo runs = %d, want 200 (not failed)", pendingQueued)
	}

	synthetic := queueRun(t, db, 1)
	claimed, _, found, _, err := db.ClaimNextRun(ctx, "local-1", "corr-synthetic")
	if err != nil || !found || claimed.ID != synthetic.ID {
		t.Fatalf("unrelated synthetic dispatch affected: found=%v err=%v claimed=%+v", found, err, claimed)
	}
}
