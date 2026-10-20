package store

import (
	"context"
	"strings"
	"testing"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/quality"
)

func createQualityFixture(t *testing.T, db *DB) (domain.Action, domain.ActionSpec, domain.Run, domain.Attempt, domain.CandidateRevision) {
	t.Helper()
	ctx := context.Background()
	actionParams, err := domain.NewActionParams{Title: "quality"}.Validate()
	if err != nil {
		t.Fatalf("validate action: %v", err)
	}
	action, _, err := db.CreateAction(ctx, actionParams, "c")
	if err != nil {
		t.Fatalf("create action: %v", err)
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
		t.Fatalf("validate spec: %v", err)
	}
	spec, _, _, err = db.CreateActionSpec(ctx, spec, "c")
	if err != nil {
		t.Fatalf("create spec: %v", err)
	}
	runParams, err := domain.NewRunParams{
		ProjectID: spec.ProjectID, ActionID: action.ID, SpecID: spec.ID,
		WorkloadKind:  domain.PaseoTaskKind,
		WorkloadInput: map[string]any{"task": spec.Goal},
		MaxAttempts:   1, TimeoutSecs: 120, ExecutionMode: "trusted-local",
		Provider: spec.Provider, Model: spec.Model, PaseoMode: spec.PaseoMode,
	}.Validate()
	if err != nil {
		t.Fatalf("validate run: %v", err)
	}
	run, _, err := db.CreateRun(ctx, runParams, "c")
	if err != nil {
		t.Fatalf("create run: %v", err)
	}
	registerWorker(t, db, "local-1")
	run, attempt, found, _, err := db.ClaimNextRun(ctx, "local-1", "c")
	if err != nil || !found {
		t.Fatalf("claim: %v %v", found, err)
	}
	if _, _, err := db.HeartbeatLease(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "c"); err != nil {
		t.Fatalf("heartbeat: %v", err)
	}
	if _, _, _, err := db.IntendPaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "c"); err != nil {
		t.Fatalf("intend: %v", err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "ws-1", "sess-1", "c"); err != nil {
		t.Fatalf("bind: %v", err)
	}
	if _, _, _, err := db.ClosePaseoSession(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "succeeded", "", "c"); err != nil {
		t.Fatalf("close: %v", err)
	}
	candidate, _, err := db.CreateCandidate(ctx, domain.CandidateRevision{
		ProjectID: spec.ProjectID, ActionID: action.ID, SpecID: spec.ID,
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
		t.Fatalf("create candidate: %v", err)
	}
	if _, _, _, err := db.ReportAttempt(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration,
		domain.OutcomeCompleted, map[string]any{}, "", "c"); err != nil {
		t.Fatalf("report: %v", err)
	}
	return action, spec, run, attempt, candidate
}

func TestQualityPipelinePersistsOneAuthoritativeDecision(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	_, spec, run, attempt, candidate := createQualityFixture(t, db)

	evidence := domain.VerificationEvidence{
		CandidateID: candidate.ID, CandidateSHA: candidate.CommitSHA, SpecID: spec.ID,
		SpecHash: spec.ContentHash, PolicyRevision: spec.PolicyRevision,
		Authority: "verifier", Actor: "verifier-1", Independent: true,
		CleanResource: "git-clone:cccccccccccc", OracleDigest: spec.OracleDigest(),
		IdempotencyKey: "verify-1",
	}
	for _, check := range spec.AllChecks() {
		evidence.Checks = append(evidence.Checks, domain.CheckResult{
			ID: check.ID, Kind: check.Kind, Status: domain.CheckPass, ExitCode: check.ExpectExit,
		})
	}
	storedEvidence, replayed, _, err := db.RecordVerification(ctx, evidence, "c")
	if err != nil || replayed {
		t.Fatalf("record verification: %v replayed=%v", err, replayed)
	}
	again, replayed, _, err := db.RecordVerification(ctx, evidence, "c")
	if err != nil || !replayed || again.ID != storedEvidence.ID {
		t.Fatalf("verification replay: %v replayed=%v", err, replayed)
	}
	review, replayed, _, err := db.RecordReview(ctx, domain.ReviewEvidence{
		CandidateID: candidate.ID, CandidateSHA: candidate.CommitSHA, SpecID: spec.ID,
		SpecHash: spec.ContentHash, PolicyRevision: spec.PolicyRevision,
		Authority: "reviewer", Actor: "reviewer-1", Verdict: domain.ReviewApprove,
		IdempotencyKey: "review-1",
	}, "c")
	if err != nil || replayed {
		t.Fatalf("record review: %v replayed=%v", err, replayed)
	}

	run, err = db.GetRun(run.ID)
	if err != nil {
		t.Fatal(err)
	}
	attempt, err = db.GetAttempt(run.ID, attempt.ID)
	if err != nil {
		t.Fatal(err)
	}
	evaluation := quality.Evaluate(quality.Input{
		Candidate: candidate, Spec: spec, Run: run, Attempt: attempt,
		Verifications: []domain.VerificationEvidence{storedEvidence},
		Reviews:       []domain.ReviewEvidence{review},
	})
	if !evaluation.Final || evaluation.Decision.Status != domain.GateAccepted {
		t.Fatalf("evaluation = %+v", evaluation)
	}
	decision, replayed, event, err := db.SaveGateDecision(ctx, evaluation.Decision, "c")
	if err != nil || replayed {
		t.Fatalf("save decision: %v replayed=%v", err, replayed)
	}
	if event.EventType != domain.EventQualityGateDecided {
		t.Fatalf("event type = %s", event.EventType)
	}
	second, replayed, _, err := db.SaveGateDecision(ctx, evaluation.Decision, "c")
	if err != nil || !replayed || second.ID != decision.ID {
		t.Fatalf("decision replay: %v replayed=%v", err, replayed)
	}
	bundle, err := db.GetCandidateBundle(candidate.ID)
	if err != nil {
		t.Fatal(err)
	}
	if bundle.Decision == nil || bundle.Decision.Status != domain.GateAccepted {
		t.Fatalf("bundle decision = %+v", bundle.Decision)
	}
}

func TestQualityEvidenceRejectsImplementerIdentityAndWrongOracle(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	_, spec, _, _, candidate := createQualityFixture(t, db)
	base := domain.VerificationEvidence{
		CandidateID: candidate.ID, CandidateSHA: candidate.CommitSHA, SpecID: spec.ID,
		SpecHash: spec.ContentHash, PolicyRevision: spec.PolicyRevision,
		Authority: "verifier", Actor: candidate.ImplementerWorkerID, Independent: true,
		CleanResource: "clean", OracleDigest: spec.OracleDigest(), IdempotencyKey: "v",
		Checks: []domain.CheckResult{{ID: "format", Kind: domain.CheckFormat, Status: domain.CheckPass}},
	}
	if _, _, _, err := db.RecordVerification(ctx, base, "c"); err == nil {
		t.Fatal("implementer identity must not submit verifier evidence")
	}
	base.Actor = "verifier-1"
	base.OracleDigest = strings.Repeat("0", 64)
	if _, _, _, err := db.RecordVerification(ctx, base, "c"); err == nil {
		t.Fatal("wrong oracle digest must be rejected")
	}
	base.OracleDigest = spec.OracleDigest()
	base.CandidateSHA = strings.Repeat("9", 40)
	if _, _, _, err := db.RecordVerification(ctx, base, "c"); err == nil {
		t.Fatal("wrong candidate identity must be rejected")
	}
}

func TestNewCandidateSupersedesPriorFrozenCandidate(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	_, spec, _, _, first := createQualityFixture(t, db)
	// A repaired candidate comes from a new Run/Attempt generation.
	runParams, err := domain.NewRunParams{
		ProjectID: spec.ProjectID, ActionID: spec.ActionID, SpecID: spec.ID,
		WorkloadKind:  domain.PaseoTaskKind,
		WorkloadInput: map[string]any{"task": spec.Goal},
		MaxAttempts:   1, TimeoutSecs: 120, ExecutionMode: "trusted-local",
		Provider: spec.Provider, Model: spec.Model, PaseoMode: spec.PaseoMode,
	}.Validate()
	if err != nil {
		t.Fatal(err)
	}
	run, _, err := db.CreateRun(ctx, runParams, "c")
	if err != nil {
		t.Fatal(err)
	}
	run, attempt, found, _, err := db.ClaimNextRun(ctx, "local-1", "c")
	if err != nil || !found {
		t.Fatalf("claim: %v %v", found, err)
	}
	if _, _, err := db.HeartbeatLease(ctx, "local-1", run.ID, attempt.ID, attempt.LeaseGeneration, "c"); err != nil {
		t.Fatal(err)
	}
	second, events, err := db.CreateCandidate(ctx, domain.CandidateRevision{
		ProjectID: spec.ProjectID, ActionID: spec.ActionID, SpecID: spec.ID,
		SpecHash: spec.ContentHash, PolicyRevision: spec.PolicyRevision,
		BaseRevision: spec.BaseRevision, CommitSHA: strings.Repeat("1", 40),
		TreeSHA: strings.Repeat("2", 40), ContentDigest: strings.Repeat("3", 64),
		CandidateRef: "refs/factory/candidates/run-2", Repository: spec.Repository,
		ImplementerRunID: run.ID, ImplementerAttemptID: attempt.ID,
		ImplementerGeneration: attempt.LeaseGeneration, ImplementerWorkerID: "local-1",
		ExecutionMode: run.ExecutionMode, Provider: run.Provider, Model: run.Model,
		PaseoMode: run.PaseoMode, ChangedPaths: []string{"add.js"}, RiskClass: domain.RiskR0,
	}, "c")
	if err != nil {
		t.Fatalf("second candidate: %v", err)
	}
	if second.ParentCandidateID != first.ID {
		t.Fatalf("parent = %q, want %q", second.ParentCandidateID, first.ID)
	}
	var sawSupersede bool
	for _, event := range events {
		if event.EventType == domain.EventCandidateSuperseded {
			sawSupersede = true
		}
	}
	if !sawSupersede {
		t.Fatal("missing CandidateSuperseded event")
	}
	old, err := db.GetCandidate(first.ID)
	if err != nil {
		t.Fatal(err)
	}
	if old.State != domain.CandidateSuperseded || old.SupersededAt.IsZero() {
		t.Fatalf("prior candidate not superseded: %+v", old)
	}
}
