package orchestrator

import (
	"context"
	"path/filepath"
	"strings"
	"testing"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/store"
)

func TestParticipantEnvironmentIsScopedAndCredentialSeparated(t *testing.T) {
	t.Setenv("FACTORY_API_TOKEN", "operator-secret")
	t.Setenv("FACTORY_WORKER_TOKEN", "worker-secret")
	t.Setenv("FACTORY_VERIFIER_TOKEN", "ambient-verifier")
	t.Setenv("FACTORY_REVIEWER_TOKEN", "ambient-reviewer")
	t.Setenv("CHROMIUM_EXE", "/tmp/chromium")
	t.Setenv("UNRELATED_PROVIDER_KEY", "must-not-propagate")
	engine := New(nil, nil, Config{VerifierToken: "scoped-verifier", ReviewerToken: "scoped-reviewer"})

	verifierEnv := strings.Join(engine.participantEnv(domain.RoleVerifier), "\n")
	for _, forbidden := range []string{"operator-secret", "worker-secret", "UNRELATED_PROVIDER_KEY", "scoped-reviewer"} {
		if strings.Contains(verifierEnv, forbidden) {
			t.Fatalf("verifier environment leaked %q: %s", forbidden, verifierEnv)
		}
	}
	if !strings.Contains(verifierEnv, "FACTORY_VERIFIER_TOKEN=scoped-verifier") ||
		!strings.Contains(verifierEnv, "CHROMIUM_EXE=/tmp/chromium") {
		t.Fatalf("verifier environment missing scoped fields: %s", verifierEnv)
	}

	reviewerEnv := strings.Join(engine.participantEnv(domain.RoleReviewer), "\n")
	if strings.Contains(reviewerEnv, "scoped-verifier") ||
		!strings.Contains(reviewerEnv, "FACTORY_REVIEWER_TOKEN=scoped-reviewer") {
		t.Fatalf("reviewer credential separation failed: %s", reviewerEnv)
	}
}

func TestParticipantBinaryFailsClosed(t *testing.T) {
	t.Setenv("FACTORY_VERIFIER_BIN", "/definitely/not/a/verifier")
	engine := New(nil, nil, Config{})
	if _, err := engine.participantBinary(domain.RoleVerifier); err == nil {
		t.Fatal("missing explicit verifier binary must fail closed")
	}
}

func TestTaskTimeoutUsesImmutablePlan(t *testing.T) {
	plan := domain.BuildCampaignPlan(0, 600, 1)
	campaign := domain.Campaign{Plan: plan}
	task := domain.CampaignTask{Key: "verifier-0"}
	if got := taskTimeout(campaign, task); got != 900 {
		t.Fatalf("verifier timeout = %d, want 900", got)
	}
	if got := taskTimeout(campaign, domain.CampaignTask{Key: "unknown"}); got != 900 {
		t.Fatalf("fallback timeout = %d", got)
	}
}

func TestBoundedOutputCannotBecomeUnboundedEvidence(t *testing.T) {
	value := sanitizeOutput(strings.Repeat("x", 1000))
	if len(value) != 300 {
		t.Fatalf("sanitized length = %d, want 300", len(value))
	}
}

type discardPublisher struct{}

func (discardPublisher) Publish(domain.Event) {}

func TestMissingParticipantBinaryFailsTheClaimedTask(t *testing.T) {
	ctx := context.Background()
	db, err := store.Open(filepath.Join(t.TempDir(), "orchestrator.db"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = db.Close() })
	actionParams, _ := domain.NewActionParams{Title: "missing binary"}.Validate()
	action, _, err := db.CreateAction(ctx, actionParams, "c")
	if err != nil {
		t.Fatal(err)
	}
	specParams := domain.NewActionSpecParams{
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
	spec, err := specParams.Validate()
	if err != nil {
		t.Fatal(err)
	}
	spec, _, _, err = db.CreateActionSpec(ctx, spec, "c")
	if err != nil {
		t.Fatal(err)
	}
	campaign, _, _, err := db.CreateCampaign(ctx, domain.NewCampaignParams{
		SpecID: spec.ID, MaxRepairCycles: 0, MaxWallClockSecs: 600, MaxParallel: 2,
	}, "c")
	if err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := db.StartCampaign(ctx, campaign.ID, "c"); err != nil {
		t.Fatal(err)
	}
	tasks, _ := db.CampaignTasks(campaign.ID)
	impl := tasks[0]
	if _, _, _, err := db.DispatchImplementer(ctx, campaign.ID, impl.ID, spec.Goal, "c"); err != nil {
		t.Fatal(err)
	}
	tasks, _ = db.CampaignTasks(campaign.ID)
	impl = tasks[0]
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
	if _, _, _, err := db.ClosePaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "succeeded", "", "c"); err != nil {
		t.Fatal(err)
	}
	candidate, _, err := db.CreateCandidate(ctx, domain.CandidateRevision{
		ProjectID: spec.ProjectID, ActionID: spec.ActionID, SpecID: spec.ID,
		SpecHash: spec.ContentHash, PolicyRevision: spec.PolicyRevision,
		BaseRevision: spec.BaseRevision, CommitSHA: strings.Repeat("c", 40),
		TreeSHA: strings.Repeat("d", 40), ContentDigest: strings.Repeat("e", 64),
		CandidateRef: "refs/factory/candidates/run", Repository: spec.Repository,
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
	if _, _, _, err := db.CompleteCampaignTask(ctx, campaign.ID, impl.ID, "succeeded", impl.DispatchToken, "c"); err != nil {
		t.Fatal(err)
	}
	if _, err := db.PromoteReadyTasks(ctx, campaign.ID, "c"); err != nil {
		t.Fatal(err)
	}
	tasks, _ = db.CampaignTasks(campaign.ID)
	var verifierTask domain.CampaignTask
	for _, task := range tasks {
		if task.Key == "verifier-0" {
			verifierTask = task
		}
	}
	if verifierTask.Status != domain.TaskReady || verifierTask.CandidateID != candidate.ID {
		t.Fatalf("verifier task = %+v", verifierTask)
	}
	engine := New(db, discardPublisher{}, Config{
		CoreURL: "http://127.0.0.1:1", VerifierToken: "v", ReviewerToken: "r",
		VerifierBin: filepath.Join(t.TempDir(), "missing-verifier"),
	})
	engine.dispatchParticipant(ctx, campaign, verifierTask)
	tasks, _ = db.CampaignTasks(campaign.ID)
	for _, task := range tasks {
		if task.Key == "verifier-0" {
			verifierTask = task
		}
	}
	if verifierTask.Status != domain.TaskReady || !strings.Contains(verifierTask.Error, "unavailable") {
		t.Fatalf("missing binary did not produce a retryable failure: %+v", verifierTask)
	}
	engine.dispatchParticipant(ctx, campaign, verifierTask)
	tasks, _ = db.CampaignTasks(campaign.ID)
	for _, task := range tasks {
		if task.Key == "verifier-0" {
			verifierTask = task
		}
	}
	if verifierTask.Status != domain.TaskFailed || !strings.Contains(verifierTask.Error, "unavailable") {
		t.Fatalf("missing binary did not fail closed: %+v", verifierTask)
	}
	final, _ := db.GetCampaign(campaign.ID)
	if final.Status != domain.CampaignBlocked {
		t.Fatalf("campaign status = %s, want blocked", final.Status)
	}
}
