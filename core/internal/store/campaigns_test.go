package store

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

func createCampaignFixture(t *testing.T, db *DB, repairs int) (domain.Action, domain.ActionSpec, domain.Campaign) {
	t.Helper()
	ctx := context.Background()
	actionParams, err := domain.NewActionParams{Title: "campaign"}.Validate()
	if err != nil {
		t.Fatal(err)
	}
	action, _, err := db.CreateAction(ctx, actionParams, "c")
	if err != nil {
		t.Fatal(err)
	}
	params := domain.NewActionSpecParams{
		ActionID: action.ID, Goal: "Add multiply",
		Repository: t.TempDir(), BaseRevision: strings.Repeat("b", 40),
		AllowedPaths: []string{"add.js"},
		AcceptanceCriteria: []domain.SpecCheck{
			{ID: "format", Kind: domain.CheckFormat, Description: "format", Command: []string{"node", "--check", "add.js"}},
			{ID: "test", Kind: domain.CheckTest, Description: "tests", Command: []string{"node", "--test"}},
			{ID: "functional", Kind: domain.CheckFunctional, Description: "multiply", Command: []string{"node", "-e", "0"}},
		},
		NegativeCases: []domain.SpecCheck{
			{ID: "negative", Kind: domain.CheckNegative, Description: "invalid", Command: []string{"node", "-e", "1"}, ExpectExit: 1},
		},
		RiskClass: "R0", QualityProfile: "node-basic",
		Budget:        domain.SpecBudget{MaxAttempts: 1, TimeoutSecs: 120},
		ExecutionMode: "trusted-local", Provider: "opencode",
		Model: "opencode-go/deepseek-v4.1-flash", PaseoMode: "build",
	}
	spec, err := params.Validate()
	if err != nil {
		t.Fatal(err)
	}
	spec, _, _, err = db.CreateActionSpec(ctx, spec, "c")
	if err != nil {
		t.Fatal(err)
	}
	campaign, replayed, _, err := db.CreateCampaign(ctx, domain.NewCampaignParams{
		SpecID: spec.ID, MaxRepairCycles: repairs, MaxWallClockSecs: 600, MaxParallel: 2,
	}, "c")
	if err != nil || replayed {
		t.Fatalf("create campaign: %v replayed=%v", err, replayed)
	}
	return action, spec, campaign
}

func driveImplementerUntilCandidate(t *testing.T, db *DB, campaign domain.Campaign, task domain.CampaignTask) (domain.CandidateRevision, domain.Run, domain.Attempt) {
	t.Helper()
	ctx := context.Background()
	registerWorker(t, db, "local-1")
	run, attempt, found, _, err := db.ClaimNextRun(ctx, "local-1", "c")
	if err != nil || !found {
		t.Fatalf("claim run: %v found=%v", err, found)
	}
	if run.ID != task.RunID {
		t.Fatalf("claimed run %s, task %s", run.ID, task.RunID)
	}
	if _, _, err := db.HeartbeatLease(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "c"); err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "c"); err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "ws-1", "sess-1", "c"); err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := db.ClosePaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "succeeded", "", "c"); err != nil {
		t.Fatal(err)
	}
	spec, err := db.GetActionSpec(campaign.SpecID)
	if err != nil {
		t.Fatal(err)
	}
	candidate, _, err := db.CreateCandidate(ctx, domain.CandidateRevision{
		ProjectID: spec.ProjectID, ActionID: spec.ActionID, SpecID: spec.ID,
		SpecHash: spec.ContentHash, PolicyRevision: spec.PolicyRevision,
		BaseRevision: spec.BaseRevision, CommitSHA: strings.Repeat("c", 40),
		TreeSHA: strings.Repeat("d", 40), ContentDigest: strings.Repeat("e", 64),
		CandidateRef: "refs/factory/candidates/run-1", Repository: spec.Repository,
		ImplementerRunID: run.ID, ImplementerAttemptID: attempt.ID,
		ImplementerGeneration: attempt.LeaseGeneration, ImplementerWorkerID: "local-1",
		ExecutionMode: run.ExecutionMode, Provider: run.Provider, Model: run.Model,
		PaseoMode: run.PaseoMode, ChangedPaths: []string{"add.js"}, RiskClass: domain.RiskR0,
	}, "c")
	if err != nil {
		t.Fatal(err)
	}
	return candidate, run, attempt
}

func driveImplementer(t *testing.T, db *DB, campaign domain.Campaign, task domain.CampaignTask) domain.CandidateRevision {
	t.Helper()
	candidate, run, attempt := driveImplementerUntilCandidate(t, db, campaign, task)
	if _, _, _, err := db.ReportAttempt(context.Background(), "local-1", run.ID, attempt.ID,
		attempt.LeaseGeneration, domain.OutcomeCompleted, map[string]any{}, "", "c"); err != nil {
		t.Fatal(err)
	}
	return candidate
}

func evidenceFor(t *testing.T, db *DB, spec domain.ActionSpec, candidate domain.CandidateRevision, key, actor, failID string) domain.VerificationEvidence {
	t.Helper()
	evidence := domain.VerificationEvidence{
		CandidateID: candidate.ID, CandidateSHA: candidate.CommitSHA, SpecID: spec.ID,
		SpecHash: spec.ContentHash, PolicyRevision: spec.PolicyRevision,
		Authority: "verifier", Actor: actor, Independent: true,
		CleanResource: "git-clone:cccccccccccc", OracleDigest: spec.OracleDigest(),
		IdempotencyKey: key,
	}
	for _, check := range spec.AllChecks() {
		status := domain.CheckPass
		if check.ID == failID {
			status = domain.CheckFail
		}
		evidence.Checks = append(evidence.Checks, domain.CheckResult{
			ID: check.ID, Kind: check.Kind, Status: status, ExitCode: check.ExpectExit,
		})
	}
	stored, _, _, err := db.RecordVerification(context.Background(), evidence, "c")
	if err != nil {
		t.Fatal(err)
	}
	return stored
}

func reviewFor(t *testing.T, db *DB, spec domain.ActionSpec, candidate domain.CandidateRevision, key, actor string, verdict domain.ReviewVerdictValues) domain.ReviewEvidence {
	t.Helper()
	stored, _, _, err := db.RecordReview(context.Background(), domain.ReviewEvidence{
		CandidateID: candidate.ID, CandidateSHA: candidate.CommitSHA, SpecID: spec.ID,
		SpecHash: spec.ContentHash, PolicyRevision: spec.PolicyRevision,
		Authority: "reviewer", Actor: actor, Verdict: verdict,
		IdempotencyKey: key,
	}, "c")
	if err != nil {
		t.Fatal(err)
	}
	return stored
}

func TestCampaignAcceptedEndToEndAndClaimFencing(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	_, spec, campaign := createCampaignFixture(t, db, 0)
	if campaign.Status != domain.CampaignPending {
		t.Fatalf("campaign status = %s", campaign.Status)
	}
	// Creation is idempotent per spec and per operator key.
	again, replayed, events, err := db.CreateCampaign(ctx, domain.NewCampaignParams{
		SpecID: spec.ID, MaxRepairCycles: 0, MaxWallClockSecs: 600, MaxParallel: 2,
	}, "c")
	if err != nil || !replayed || again.ID != campaign.ID || len(events) != 0 {
		t.Fatalf("campaign replay: %v replayed=%v events=%d", err, replayed, len(events))
	}
	started, replayed, events, err := db.StartCampaign(ctx, campaign.ID, "c")
	if err != nil || replayed || started.Status != domain.CampaignRunning || len(events) < 2 {
		t.Fatalf("start: %v replayed=%v events=%d", err, replayed, len(events))
	}
	if _, replayed, _, err := db.StartCampaign(ctx, campaign.ID, "c"); err != nil || !replayed {
		t.Fatalf("start replay: %v replayed=%v", err, replayed)
	}
	tasks, err := db.CampaignTasks(campaign.ID)
	if err != nil {
		t.Fatal(err)
	}
	impl := tasks[0]
	if impl.Role != domain.RoleImplementer || impl.Status != domain.TaskReady {
		t.Fatalf("implementer task = %+v", impl)
	}
	_, run, dispatchEvents, err := db.DispatchImplementer(ctx, campaign.ID, impl.ID, spec.Goal, "c")
	if err != nil {
		t.Fatal(err)
	}
	impl, err = db.getCampaignTask(campaign.ID, impl.ID)
	if err != nil {
		t.Fatal(err)
	}
	if len(dispatchEvents) != 2 {
		t.Fatalf("dispatch events = %d", len(dispatchEvents))
	}
	// A second dispatch is an idempotent replay and cannot create a second Run.
	_, replayRun, replayEvents, err := db.DispatchImplementer(ctx, campaign.ID, impl.ID, spec.Goal, "c")
	if err != nil || replayRun.ID != run.ID || len(replayEvents) != 0 {
		t.Fatalf("dispatch replay: %v run=%s events=%d", err, replayRun.ID, len(replayEvents))
	}
	candidate := driveImplementer(t, db, campaign, impl)
	completed, replayed, _, err := db.CompleteCampaignTask(ctx, campaign.ID, impl.ID, "succeeded", impl.DispatchToken, "c")
	if err != nil || replayed || completed.CandidateID != candidate.ID {
		t.Fatalf("complete implementer: %v replayed=%v candidate=%s", err, replayed, completed.CandidateID)
	}
	if _, err := db.PromoteReadyTasks(ctx, campaign.ID, "c"); err != nil {
		t.Fatal(err)
	}
	tasks, _ = db.CampaignTasks(campaign.ID)
	var verifierTask, reviewerTask domain.CampaignTask
	for _, task := range tasks {
		switch task.Key {
		case "verifier-0":
			verifierTask = task
		case "reviewer-0":
			reviewerTask = task
		}
	}
	if verifierTask.Status != domain.TaskReady || reviewerTask.Status != domain.TaskReady ||
		verifierTask.CandidateID != candidate.ID || reviewerTask.CandidateID != candidate.ID {
		t.Fatalf("dependent tasks not candidate-bound: %+v %+v", verifierTask, reviewerTask)
	}
	makeEvidence := func(key, actor string) domain.VerificationEvidence {
		evidence := domain.VerificationEvidence{
			CandidateID: candidate.ID, CandidateSHA: candidate.CommitSHA, SpecID: spec.ID,
			SpecHash: spec.ContentHash, PolicyRevision: spec.PolicyRevision,
			Authority: "verifier", Actor: actor, Independent: true,
			CleanResource: "git-clone:test", OracleDigest: spec.OracleDigest(),
			IdempotencyKey: key,
		}
		for _, check := range spec.AllChecks() {
			evidence.Checks = append(evidence.Checks, domain.CheckResult{
				ID: check.ID, Kind: check.Kind, Status: domain.CheckPass, ExitCode: check.ExpectExit,
			})
		}
		return evidence
	}
	deterministicKey := "campaign-" + campaign.ID + "-verifier-0-" + candidate.ID
	if _, _, _, err := db.RecordVerification(ctx,
		makeEvidence(deterministicKey, verifierTask.ParticipantID), "c"); !errors.Is(err, domain.ErrUnauthorized) {
		t.Fatalf("pre-claim evidence = %v, want unauthorized", err)
	}
	var wg sync.WaitGroup
	claims := make([]bool, 2)
	for i, task := range []domain.CampaignTask{verifierTask, reviewerTask} {
		wg.Add(1)
		go func(index int, task domain.CampaignTask) {
			defer wg.Done()
			_, ok, _, cerr := db.ClaimParticipantTask(ctx, campaign.ID, task.ID, "orchestrator-1", time.Minute, "c")
			if cerr != nil {
				t.Errorf("claim %s: %v", task.Key, cerr)
			}
			claims[index] = ok
		}(i, task)
	}
	wg.Wait()
	if !claims[0] || !claims[1] {
		t.Fatalf("concurrent claims = %v", claims)
	}
	// A third claim for the same task is rejected.
	if _, ok, _, err := db.ClaimParticipantTask(ctx, campaign.ID, verifierTask.ID, "orchestrator-2", time.Minute, "c"); err == nil || ok {
		t.Fatal("second claim of the same participant task must fail")
	}
	verifierTask, _ = db.getCampaignTask(campaign.ID, verifierTask.ID)
	reviewerTask, _ = db.getCampaignTask(campaign.ID, reviewerTask.ID)
	if _, _, _, err := db.RecordVerification(ctx,
		makeEvidence("alternate-key", verifierTask.ParticipantID), "c"); !errors.Is(err, domain.ErrUnauthorized) {
		t.Fatalf("alternate-key evidence = %v, want unauthorized", err)
	}
	if _, _, _, err := db.RecordVerification(ctx,
		makeEvidence(verifierTask.IdempotencyKey, "not-the-assigned-participant"), "c"); !errors.Is(err, domain.ErrUnauthorized) {
		t.Fatalf("mismatched participant evidence = %v, want unauthorized", err)
	}
	evidenceFor(t, db, spec, candidate, verifierTask.IdempotencyKey, verifierTask.ParticipantID, "")
	reviewFor(t, db, spec, candidate, reviewerTask.IdempotencyKey, reviewerTask.ParticipantID, domain.ReviewApprove)
	for _, task := range []domain.CampaignTask{verifierTask, reviewerTask} {
		outcome, ok, err := db.TaskEvidence(task)
		if err != nil || !ok {
			t.Fatalf("evidence for %s: %v ok=%v", task.Key, err, ok)
		}
		if _, _, _, err := db.CompleteCampaignTask(ctx, campaign.ID, task.ID, outcome, "stale-token", "c"); !errors.Is(err, domain.ErrStaleGeneration) {
			t.Fatalf("stale complete token = %v, want stale generation", err)
		}
		if _, _, _, err := db.CompleteCampaignTask(ctx, campaign.ID, task.ID, outcome, task.DispatchToken, "c"); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := db.PromoteReadyTasks(ctx, campaign.ID, "c"); err != nil {
		t.Fatal(err)
	}
	tasks, _ = db.CampaignTasks(campaign.ID)
	var gate domain.CampaignTask
	for _, task := range tasks {
		if task.Key == "gate-0" {
			gate = task
		}
	}
	if gate.Status != domain.TaskReady || gate.CandidateID != candidate.ID {
		t.Fatalf("gate task = %+v", gate)
	}
	if _, ok, _, err := db.ClaimGateTask(ctx, campaign.ID, gate.ID, "orchestrator-1", time.Minute, "c"); err != nil || !ok {
		t.Fatalf("claim gate: %v ok=%v", err, ok)
	}
	if _, ok, _, err := db.ClaimGateTask(ctx, campaign.ID, gate.ID, "orchestrator-2", time.Minute, "c"); err == nil || ok {
		t.Fatal("concurrent gate claim must fail")
	}
	if _, _, err := db.ApplyGateOutcome(ctx, campaign.ID, gate.ID, "stale-token",
		domain.GateDecision{CandidateID: candidate.ID}, "c"); !errors.Is(err, domain.ErrStaleGeneration) {
		t.Fatalf("stale gate token = %v, want stale generation", err)
	}
	decision, final, gateEvents, err := db.EvaluateCampaignGate(ctx, campaign.ID, gate.ID, "c")
	if err != nil || !final || decision.Status != domain.GateAccepted {
		t.Fatalf("gate: %v final=%v status=%s", err, final, decision.Status)
	}
	if len(gateEvents) == 0 {
		t.Fatal("gate produced no events")
	}
	finalCampaign, err := db.GetCampaign(campaign.ID)
	if err != nil {
		t.Fatal(err)
	}
	if finalCampaign.Status != domain.CampaignAccepted || finalCampaign.CompletedAt.IsZero() {
		t.Fatalf("final campaign = %+v", finalCampaign)
	}
	events, err = db.ListAggregateEvents(domain.AggregateCampaign, campaign.ID)
	if err != nil {
		t.Fatal(err)
	}
	gateDecisions := 0
	for _, event := range events {
		if event.EventType == domain.EventCampaignFinished {
			gateDecisions++
		}
	}
	if gateDecisions != 1 {
		t.Fatalf("campaign finished events = %d", gateDecisions)
	}
}

func TestCampaignRepairRequiresGateAndCreatesFreshCycle(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	_, spec, campaign := createCampaignFixture(t, db, 1)
	if _, _, _, err := db.StartCampaign(ctx, campaign.ID, "c"); err != nil {
		t.Fatal(err)
	}
	tasks, _ := db.CampaignTasks(campaign.ID)
	impl := tasks[0]
	if _, _, _, err := db.DispatchImplementer(ctx, campaign.ID, impl.ID, spec.Goal, "c"); err != nil {
		t.Fatal(err)
	}
	reloaded, rerr := db.getCampaignTask(campaign.ID, impl.ID)
	if rerr != nil {
		t.Fatal(rerr)
	}
	impl = reloaded
	candidate := driveImplementer(t, db, campaign, impl)
	if _, _, _, err := db.CompleteCampaignTask(ctx, campaign.ID, impl.ID, "succeeded", impl.DispatchToken, "c"); err != nil {
		t.Fatal(err)
	}
	_, _ = db.PromoteReadyTasks(ctx, campaign.ID, "c")
	tasks, _ = db.CampaignTasks(campaign.ID)
	var verifierTask, reviewerTask domain.CampaignTask
	for _, task := range tasks {
		if task.Key == "verifier-0" {
			verifierTask = task
		}
		if task.Key == "reviewer-0" {
			reviewerTask = task
		}
	}
	if _, _, _, err := db.ClaimParticipantTask(ctx, campaign.ID, verifierTask.ID, "o", time.Minute, "c"); err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := db.ClaimParticipantTask(ctx, campaign.ID, reviewerTask.ID, "o", time.Minute, "c"); err != nil {
		t.Fatal(err)
	}
	verifierTask, _ = db.getCampaignTask(campaign.ID, verifierTask.ID)
	reviewerTask, _ = db.getCampaignTask(campaign.ID, reviewerTask.ID)
	evidenceFor(t, db, spec, candidate, verifierTask.IdempotencyKey, verifierTask.ParticipantID, "functional")
	reviewFor(t, db, spec, candidate, reviewerTask.IdempotencyKey, reviewerTask.ParticipantID, domain.ReviewApprove)
	if _, _, _, err := db.CompleteCampaignTask(ctx, campaign.ID, verifierTask.ID, "failed", verifierTask.DispatchToken, "c"); err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := db.CompleteCampaignTask(ctx, campaign.ID, reviewerTask.ID, "APPROVE", reviewerTask.DispatchToken, "c"); err != nil {
		t.Fatal(err)
	}
	_, _ = db.PromoteReadyTasks(ctx, campaign.ID, "c")
	tasks, _ = db.CampaignTasks(campaign.ID)
	var gate domain.CampaignTask
	for _, task := range tasks {
		if task.Key == "gate-0" {
			gate = task
		}
	}
	if _, _, _, err := db.ClaimGateTask(ctx, campaign.ID, gate.ID, "o", time.Minute, "c"); err != nil {
		t.Fatal(err)
	}
	decision, final, _, err := db.EvaluateCampaignGate(ctx, campaign.ID, gate.ID, "c")
	if err != nil || !final || decision.Status != domain.GateRepair {
		t.Fatalf("gate: %v final=%v status=%s", err, final, decision.Status)
	}
	repaired, _ := db.GetCampaign(campaign.ID)
	if repaired.Status != domain.CampaignRunning || repaired.RepairCycles != 1 {
		t.Fatalf("repair campaign = %+v", repaired)
	}
	if _, err := db.PromoteReadyTasks(ctx, campaign.ID, "c"); err != nil {
		t.Fatal(err)
	}
	tasks, _ = db.CampaignTasks(campaign.ID)
	var repairImpl domain.CampaignTask
	for _, task := range tasks {
		if task.Key == "implementer-1" {
			repairImpl = task
		}
	}
	if repairImpl.Status != domain.TaskReady {
		t.Fatalf("repair implementer status = %s", repairImpl.Status)
	}
	// No later cycle can become ready without its own repair authorization.
	for _, task := range tasks {
		if task.Key == "verifier-1" && task.Status != domain.TaskPending {
			t.Fatalf("verifier-1 advanced before repair candidate: %s", task.Status)
		}
	}
}

func TestCampaignBlockedDoesNotAuthorizeRepair(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	_, _, campaign := createCampaignFixture(t, db, 2)
	if _, _, _, err := db.StartCampaign(ctx, campaign.ID, "c"); err != nil {
		t.Fatal(err)
	}
	blocked, events, err := db.BlockCampaign(ctx, campaign.ID, "unresolved paseo recovery", "c")
	if err != nil || blocked.Status != domain.CampaignBlocked || len(events) == 0 {
		t.Fatalf("block: %v status=%s events=%d", err, blocked.Status, len(events))
	}
	tasks, _ := db.CampaignTasks(campaign.ID)
	for _, task := range tasks {
		if task.Cycle > 0 && task.Status != domain.TaskCancelled {
			t.Fatalf("repair task %s advanced after BLOCKED: %s", task.Key, task.Status)
		}
	}
	if _, _, _, err := db.StartCampaign(ctx, campaign.ID, "c"); !errors.Is(err, domain.ErrInvalidTransition) {
		t.Fatalf("terminal campaign start = %v", err)
	}
}

func TestCampaignParticipantRetryIsBounded(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	_, spec, campaign := createCampaignFixture(t, db, 0)
	if _, _, _, err := db.StartCampaign(ctx, campaign.ID, "c"); err != nil {
		t.Fatal(err)
	}
	tasks, _ := db.CampaignTasks(campaign.ID)
	impl := tasks[0]
	_, run, _, err := db.DispatchImplementer(ctx, campaign.ID, impl.ID, spec.Goal, "c")
	if err != nil {
		t.Fatal(err)
	}
	impl, err = db.getCampaignTask(campaign.ID, impl.ID)
	if err != nil {
		t.Fatal(err)
	}
	candidate := driveImplementer(t, db, campaign, impl)
	if _, _, _, err := db.CompleteCampaignTask(ctx, campaign.ID, impl.ID, "succeeded", impl.DispatchToken, "c"); err != nil {
		t.Fatal(err)
	}
	_, _ = db.PromoteReadyTasks(ctx, campaign.ID, "c")
	tasks, _ = db.CampaignTasks(campaign.ID)
	var verifierTask domain.CampaignTask
	for _, task := range tasks {
		if task.Key == "verifier-0" {
			verifierTask = task
		}
	}
	if _, _, _, err := db.ClaimParticipantTask(ctx, campaign.ID, verifierTask.ID, "o", time.Minute, "c"); err != nil {
		t.Fatal(err)
	}
	verifierTask, _ = db.getCampaignTask(campaign.ID, verifierTask.ID)
	oldToken := verifierTask.DispatchToken
	// First failure retries with the same candidate and idempotency key.
	failed, events, err := db.FailParticipantTask(ctx, campaign.ID, verifierTask.ID, "worker missing", oldToken, "c")
	if err != nil || failed.Status != domain.TaskReady || len(events) == 0 {
		t.Fatalf("retry: %v status=%s", err, failed.Status)
	}
	if failed.CandidateID != candidate.ID {
		t.Fatalf("retry changed candidate: %s", failed.CandidateID)
	}
	key := failed.IdempotencyKey
	if _, _, _, err := db.ClaimParticipantTask(ctx, campaign.ID, verifierTask.ID, "o", time.Minute, "c"); err != nil {
		t.Fatal(err)
	}
	verifierTask, _ = db.getCampaignTask(campaign.ID, verifierTask.ID)
	if _, _, err := db.FailParticipantTask(ctx, campaign.ID, verifierTask.ID,
		"stale owner", oldToken, "c"); !errors.Is(err, domain.ErrStaleGeneration) {
		t.Fatalf("stale dispatch token = %v, want stale generation", err)
	}
	failed, _, err = db.FailParticipantTask(ctx, campaign.ID, verifierTask.ID, "worker missing again", verifierTask.DispatchToken, "c")
	if err != nil || failed.Status != domain.TaskFailed {
		t.Fatalf("exhaust: %v status=%s", err, failed.Status)
	}
	final, _ := db.GetCampaign(campaign.ID)
	if final.Status != domain.CampaignBlocked {
		t.Fatalf("campaign after participant exhaustion = %s", final.Status)
	}
	if key == "" {
		t.Fatal("idempotency key was not assigned")
	}
	_ = run
}

func TestCampaignSchedulerIsNotStarvedByIneligibleTasks(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	_, spec, campaign := createCampaignFixture(t, db, 0)
	now := stamp(db.now())
	for i := 0; i < 300; i++ {
		if _, err := db.sql.ExecContext(ctx,
			`INSERT INTO campaign_tasks(id, campaign_id, task_key, role, cycle, status,
			 depends_on, participant_id, candidate_id, run_id, outcome, error,
			 execution_count, max_executions, idempotency_key, dispatch_owner,
			 dispatch_token, dispatch_expires_at, created_at, updated_at, started_at, completed_at)
			 VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)`,
			newID(), campaign.ID, "blocked-"+time.Now().Add(time.Duration(i)*time.Nanosecond).Format("150405.000000000"), string(domain.RoleVerifier), 0,
			string(domain.TaskPending), `["missing"]`, "", "", "", "", "",
			0, 1, "", "", "", "", now, now, "", ""); err != nil {
			t.Fatal(err)
		}
	}
	if _, _, _, err := db.StartCampaign(ctx, campaign.ID, "c"); err != nil {
		t.Fatal(err)
	}
	if _, err := db.PromoteReadyTasks(ctx, campaign.ID, "c"); err != nil {
		t.Fatal(err)
	}
	tasks, err := db.CampaignTasks(campaign.ID)
	if err != nil {
		t.Fatal(err)
	}
	impl := tasks[0]
	if impl.Key != "implementer-0" || impl.Status != domain.TaskReady {
		t.Fatalf("eligible implementer was starved: %+v", impl)
	}
	if _, _, _, err := db.DispatchImplementer(ctx, campaign.ID, impl.ID, spec.Goal, "c"); err != nil {
		t.Fatalf("dispatch after ineligible tasks: %v", err)
	}
}

func TestCampaignBlocksCancelLiveImplementerRun(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	_, spec, campaign := createCampaignFixture(t, db, 0)
	if _, _, _, err := db.StartCampaign(ctx, campaign.ID, "c"); err != nil {
		t.Fatal(err)
	}
	tasks, _ := db.CampaignTasks(campaign.ID)
	impl := tasks[0]
	_, run, _, err := db.DispatchImplementer(ctx, campaign.ID, impl.ID, spec.Goal, "c")
	if err != nil {
		t.Fatal(err)
	}
	blocked, events, err := db.BlockCampaign(ctx, campaign.ID, "test block", "c")
	if err != nil || blocked.Status != domain.CampaignBlocked || len(events) == 0 {
		t.Fatalf("block: %v status=%s", err, blocked.Status)
	}
	stored, err := db.GetRun(run.ID)
	if err != nil {
		t.Fatal(err)
	}
	if stored.Status != domain.RunCancelled {
		t.Fatalf("live Run status = %s, want cancelled", stored.Status)
	}
}

func TestCampaignRejectsSecondActiveCampaignForAction(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	_, spec, campaign := createCampaignFixture(t, db, 1)
	if campaign.MaxAttempts != 2 {
		t.Fatalf("campaign attempt budget = %d, want two-cycle total 2", campaign.MaxAttempts)
	}
	if _, _, _, err := db.StartCampaign(ctx, campaign.ID, "c"); err != nil {
		t.Fatal(err)
	}
	params := domain.NewActionSpecParams{
		ActionID: spec.ActionID, Goal: "A different spec",
		Repository: spec.Repository, BaseRevision: spec.BaseRevision,
		AllowedPaths:       spec.AllowedPaths,
		AcceptanceCriteria: spec.AcceptanceCriteria, NegativeCases: spec.NegativeCases,
		RiskClass: "R0", QualityProfile: "node-basic",
		Budget: spec.Budget, ExecutionMode: "trusted-local",
		Provider: spec.Provider, Model: spec.Model, PaseoMode: spec.PaseoMode,
	}
	second, err := params.Validate()
	if err != nil {
		t.Fatal(err)
	}
	second, _, _, err = db.CreateActionSpec(ctx, second, "c")
	if err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := db.CreateCampaign(ctx, domain.NewCampaignParams{
		SpecID: second.ID, MaxRepairCycles: 0, MaxWallClockSecs: 600, MaxParallel: 2,
	}, "c"); !errors.Is(err, domain.ErrConflict) {
		t.Fatalf("second active campaign = %v, want conflict", err)
	}
}

func TestMigrateS4BBackfillsLegacyAttemptBudget(t *testing.T) {
	path := filepath.Join(t.TempDir(), "legacy.db")
	db, err := Open(path)
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	_, _, campaign := createCampaignFixture(t, db, 1)
	if _, err := db.sql.ExecContext(ctx,
		`UPDATE campaigns SET max_attempts=1 WHERE id=?`, campaign.ID); err != nil {
		t.Fatal(err)
	}
	if _, err := db.sql.Exec(`ALTER TABLE campaigns DROP COLUMN attempt_budget_version`); err != nil {
		t.Fatalf("simulate pre-version schema: %v", err)
	}
	if err := db.Close(); err != nil {
		t.Fatal(err)
	}
	reopened, err := Open(path)
	if err != nil {
		t.Fatal(err)
	}
	migrated, err := reopened.GetCampaign(campaign.ID)
	if err != nil {
		t.Fatal(err)
	}
	if migrated.MaxAttempts != 2 {
		t.Fatalf("migrated attempt budget = %d, want 2", migrated.MaxAttempts)
	}
	_, _, fresh := createCampaignFixture(t, reopened, 1)
	if fresh.MaxAttempts != 2 {
		t.Fatalf("fresh campaign budget = %d, want 2", fresh.MaxAttempts)
	}
	if err := reopened.Close(); err != nil {
		t.Fatal(err)
	}
	reopened2, err := Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = reopened2.Close() }()
	fresh, err = reopened2.GetCampaign(fresh.ID)
	if err != nil {
		t.Fatal(err)
	}
	if fresh.MaxAttempts != 2 {
		t.Fatalf("fresh campaign budget after restart = %d, want 2", fresh.MaxAttempts)
	}
}

func TestMigrateS4BBlocksDuplicateActiveCampaignAndCancelsRun(t *testing.T) {
	path := filepath.Join(t.TempDir(), "duplicate.db")
	db, err := Open(path)
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	_, spec1, campaign1 := createCampaignFixture(t, db, 0)
	if _, _, _, err := db.StartCampaign(ctx, campaign1.ID, "c"); err != nil {
		t.Fatal(err)
	}
	tasks, _ := db.CampaignTasks(campaign1.ID)
	if _, _, _, err := db.DispatchImplementer(ctx, campaign1.ID, tasks[0].ID, spec1.Goal, "c"); err != nil {
		t.Fatal(err)
	}
	_, spec2, campaign2 := createCampaignFixture(t, db, 0)
	if _, _, _, err := db.StartCampaign(ctx, campaign2.ID, "c"); err != nil {
		t.Fatal(err)
	}
	tasks2, _ := db.CampaignTasks(campaign2.ID)
	_, run2, _, err := db.DispatchImplementer(ctx, campaign2.ID, tasks2[0].ID, spec2.Goal, "c")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := db.sql.Exec(`DROP INDEX idx_campaigns_active_action`); err != nil {
		t.Fatal(err)
	}
	if _, err := db.sql.ExecContext(ctx,
		`UPDATE campaigns SET action_id=? WHERE id=?`, campaign1.ActionID, campaign2.ID); err != nil {
		t.Fatal(err)
	}
	if err := db.Close(); err != nil {
		t.Fatal(err)
	}
	reopened, err := Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = reopened.Close() }()
	blocked, err := reopened.GetCampaign(campaign2.ID)
	if err != nil {
		t.Fatal(err)
	}
	if blocked.Status != domain.CampaignBlocked {
		t.Fatalf("duplicate campaign status = %s, want blocked", blocked.Status)
	}
	storedRun, err := reopened.GetRun(run2.ID)
	if err != nil {
		t.Fatal(err)
	}
	if storedRun.Status != domain.RunCancelled {
		t.Fatalf("duplicate campaign Run = %s, want cancelled", storedRun.Status)
	}
	events, err := reopened.ListAggregateEvents(domain.AggregateCampaign, campaign2.ID)
	if err != nil {
		t.Fatal(err)
	}
	finished := 0
	for _, event := range events {
		if event.EventType == domain.EventCampaignFinished {
			finished++
		}
	}
	if finished != 1 {
		t.Fatalf("migration CampaignFinished events = %d, want 1", finished)
	}
}

func TestCampaignCreateReplaySurvivesActionCancellation(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	action, spec, campaign := createCampaignFixture(t, db, 0)
	if _, _, err := db.TransitionAction(ctx, action.ID, domain.StatusCancelled, "c"); err != nil {
		t.Fatal(err)
	}
	replayed, replay, events, err := db.CreateCampaign(ctx, domain.NewCampaignParams{
		SpecID: spec.ID, MaxRepairCycles: 0, MaxWallClockSecs: 600, MaxParallel: 2,
	}, "c")
	if err != nil || !replay || replayed.ID != campaign.ID || len(events) != 0 {
		t.Fatalf("terminal-action replay: %v replay=%v events=%d", err, replay, len(events))
	}
	if _, _, _, err := db.StartCampaign(ctx, campaign.ID, "c"); !errors.Is(err, domain.ErrInvalidTransition) {
		t.Fatalf("start after action cancellation = %v, want invalid transition", err)
	}
}

func TestCampaignAttemptBudgetExhaustionIsExplicit(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	_, spec, campaign := createCampaignFixture(t, db, 0)
	if _, _, _, err := db.StartCampaign(ctx, campaign.ID, "c"); err != nil {
		t.Fatal(err)
	}
	if _, err := db.sql.ExecContext(ctx,
		`UPDATE campaigns SET max_attempts=0 WHERE id=?`, campaign.ID); err != nil {
		t.Fatal(err)
	}
	tasks, _ := db.CampaignTasks(campaign.ID)
	if _, _, _, err := db.DispatchImplementer(ctx, campaign.ID, tasks[0].ID, spec.Goal, "c"); !errors.Is(err, domain.ErrBudgetExhausted) {
		t.Fatalf("dispatch at zero budget = %v, want budget exhausted", err)
	}
}

func TestCampaignWallClockExpiryBlocksAndCancels(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	_, spec, campaign := createCampaignFixture(t, db, 0)
	if _, _, _, err := db.StartCampaign(ctx, campaign.ID, "c"); err != nil {
		t.Fatal(err)
	}
	tasks, _ := db.CampaignTasks(campaign.ID)
	_, run, _, err := db.DispatchImplementer(ctx, campaign.ID, tasks[0].ID, spec.Goal, "c")
	if err != nil {
		t.Fatal(err)
	}
	old := stamp(db.now().Add(-time.Hour))
	if _, err := db.sql.ExecContext(ctx,
		`UPDATE campaigns SET started_at=? WHERE id=?`, old, campaign.ID); err != nil {
		t.Fatal(err)
	}
	events, err := db.ExpireCampaigns(ctx, "c")
	if err != nil || len(events) == 0 {
		t.Fatalf("expire: %v events=%d", err, len(events))
	}
	expired, _ := db.GetCampaign(campaign.ID)
	if expired.Status != domain.CampaignBlocked || !strings.Contains(expired.TerminalReason, "wall-clock") {
		t.Fatalf("expired campaign = %+v", expired)
	}
	storedRun, _ := db.GetRun(run.ID)
	if storedRun.Status != domain.RunCancelled {
		t.Fatalf("expired campaign Run = %s, want cancelled", storedRun.Status)
	}
}

func TestCampaignUnresolvedPaseoRecoveryIsDetected(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	_, spec, campaign := createCampaignFixture(t, db, 0)
	if _, _, _, err := db.StartCampaign(ctx, campaign.ID, "c"); err != nil {
		t.Fatal(err)
	}
	tasks, _ := db.CampaignTasks(campaign.ID)
	_, run, _, err := db.DispatchImplementer(ctx, campaign.ID, tasks[0].ID, spec.Goal, "c")
	if err != nil {
		t.Fatal(err)
	}
	if _, _, err := db.RegisterWorker(ctx, "local-1", "test", domain.AllCapabilities, []string{"restricted", "trusted-local"}, "c"); err != nil {
		t.Fatal(err)
	}
	run, attempt, found, _, err := db.ClaimNextRun(ctx, "local-1", "c")
	if err != nil || !found {
		t.Fatalf("claim: %v %v", err, found)
	}
	if _, _, err := db.HeartbeatLease(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "c"); err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "c"); err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "ws", "sess", "c"); err != nil {
		t.Fatal(err)
	}
	unresolved, reason, err := db.RunRecoveryUnresolved(run.ID)
	if err != nil || !unresolved || reason == "" {
		t.Fatalf("active binding recovery = %v %q %v", unresolved, reason, err)
	}
	if _, _, _, err := db.ClosePaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "succeeded", "", "c"); err != nil {
		t.Fatal(err)
	}
	unresolved, _, err = db.RunRecoveryUnresolved(run.ID)
	if err != nil || unresolved {
		t.Fatalf("closed binding recovery = %v %v", unresolved, err)
	}
}

func TestCampaignRejectsEvidenceBeforeImplementerBinding(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	_, spec, campaign := createCampaignFixture(t, db, 0)
	if _, _, _, err := db.StartCampaign(ctx, campaign.ID, "c"); err != nil {
		t.Fatal(err)
	}
	tasks, _ := db.CampaignTasks(campaign.ID)
	impl := tasks[0]
	if _, _, _, err := db.DispatchImplementer(ctx, campaign.ID, impl.ID, spec.Goal, "c"); err != nil {
		t.Fatal(err)
	}
	impl, _ = db.getCampaignTask(campaign.ID, impl.ID)
	candidate, _, _ := driveImplementerUntilCandidate(t, db, campaign, impl)
	// The candidate exists but the implementer task has not completed; no
	// verifier task has been claimed, so there is no assigned task key.
	evidence := domain.VerificationEvidence{
		CandidateID: candidate.ID, CandidateSHA: candidate.CommitSHA, SpecID: spec.ID,
		SpecHash: spec.ContentHash, PolicyRevision: spec.PolicyRevision,
		Authority: "verifier", Actor: "verifier-" + campaign.ID[:8] + "-0", Independent: true,
		CleanResource: "git-clone:prebind", OracleDigest: spec.OracleDigest(),
		IdempotencyKey: "campaign-" + campaign.ID + "-verifier-0-" + candidate.ID,
	}
	for _, check := range spec.AllChecks() {
		evidence.Checks = append(evidence.Checks, domain.CheckResult{
			ID: check.ID, Kind: check.Kind, Status: domain.CheckPass, ExitCode: check.ExpectExit,
		})
	}
	if _, _, _, err := db.RecordVerification(ctx, evidence, "c"); !errors.Is(err, domain.ErrUnauthorized) {
		t.Fatalf("pre-bind evidence = %v, want unauthorized", err)
	}
}
