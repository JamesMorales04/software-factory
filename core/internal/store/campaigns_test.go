package store

import (
	"context"
	"errors"
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

func driveImplementer(t *testing.T, db *DB, campaign domain.Campaign, task domain.CampaignTask) domain.CandidateRevision {
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
	if _, _, _, err := db.ReportAttempt(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration,
		domain.OutcomeCompleted, map[string]any{}, "", "c"); err != nil {
		t.Fatal(err)
	}
	return candidate
}

func evidenceFor(t *testing.T, db *DB, spec domain.ActionSpec, candidate domain.CandidateRevision, key string, failID string) domain.VerificationEvidence {
	t.Helper()
	evidence := domain.VerificationEvidence{
		CandidateID: candidate.ID, CandidateSHA: candidate.CommitSHA, SpecID: spec.ID,
		SpecHash: spec.ContentHash, PolicyRevision: spec.PolicyRevision,
		Authority: "verifier", Actor: "verifier-1", Independent: true,
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

func reviewFor(t *testing.T, db *DB, spec domain.ActionSpec, candidate domain.CandidateRevision, key string, verdict domain.ReviewVerdictValues) domain.ReviewEvidence {
	t.Helper()
	stored, _, _, err := db.RecordReview(context.Background(), domain.ReviewEvidence{
		CandidateID: candidate.ID, CandidateSHA: candidate.CommitSHA, SpecID: spec.ID,
		SpecHash: spec.ContentHash, PolicyRevision: spec.PolicyRevision,
		Authority: "reviewer", Actor: "reviewer-1", Verdict: verdict,
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
	completed, replayed, _, err := db.CompleteCampaignTask(ctx, campaign.ID, impl.ID, "succeeded", "c")
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
	evidenceFor(t, db, spec, candidate, verifierTask.IdempotencyKey, "")
	reviewFor(t, db, spec, candidate, reviewerTask.IdempotencyKey, domain.ReviewApprove)
	for _, task := range []domain.CampaignTask{verifierTask, reviewerTask} {
		outcome, ok, err := db.TaskEvidence(task)
		if err != nil || !ok {
			t.Fatalf("evidence for %s: %v ok=%v", task.Key, err, ok)
		}
		if _, _, _, err := db.CompleteCampaignTask(ctx, campaign.ID, task.ID, outcome, "c"); err != nil {
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
	if _, _, _, err := db.CompleteCampaignTask(ctx, campaign.ID, impl.ID, "succeeded", "c"); err != nil {
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
	evidenceFor(t, db, spec, candidate, verifierTask.IdempotencyKey, "functional")
	reviewFor(t, db, spec, candidate, reviewerTask.IdempotencyKey, domain.ReviewApprove)
	if _, _, _, err := db.CompleteCampaignTask(ctx, campaign.ID, verifierTask.ID, "failed", "c"); err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := db.CompleteCampaignTask(ctx, campaign.ID, reviewerTask.ID, "APPROVE", "c"); err != nil {
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
	if _, _, _, err := db.CompleteCampaignTask(ctx, campaign.ID, impl.ID, "succeeded", "c"); err != nil {
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
	// First failure retries with the same candidate and idempotency key.
	failed, events, err := db.FailParticipantTask(ctx, campaign.ID, verifierTask.ID, "worker missing", "c")
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
	failed, _, err = db.FailParticipantTask(ctx, campaign.ID, verifierTask.ID, "worker missing again", "c")
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
