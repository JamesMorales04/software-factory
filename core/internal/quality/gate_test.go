package quality

import (
	"strings"
	"testing"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

func gateFixture(t *testing.T) Input {
	t.Helper()
	params := domain.NewActionSpecParams{
		ActionID: strings.Repeat("a", 32), Goal: "Add multiply",
		Repository: "/tmp/repo.git", BaseRevision: strings.Repeat("b", 40),
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
	now := time.Now().UTC()
	spec.ID = strings.Repeat("f", 32)
	spec.FrozenAt = now.Add(-time.Hour)
	runID := strings.Repeat("1", 32)
	attemptID := strings.Repeat("2", 32)
	candidate := domain.CandidateRevision{
		ID: strings.Repeat("3", 32), ProjectID: spec.ProjectID, ActionID: spec.ActionID,
		SpecID: spec.ID, SpecHash: spec.ContentHash, PolicyRevision: spec.PolicyRevision,
		BaseRevision: spec.BaseRevision, CommitSHA: strings.Repeat("c", 40),
		TreeSHA: strings.Repeat("d", 40), ContentDigest: strings.Repeat("e", 64),
		CandidateRef: "refs/factory/candidates/run-1", Repository: spec.Repository,
		ImplementerRunID: runID, ImplementerAttemptID: attemptID,
		ImplementerGeneration: 1, ImplementerWorkerID: "local-1",
		ExecutionMode: spec.ExecutionMode, Provider: spec.Provider, Model: spec.Model,
		PaseoMode: spec.PaseoMode, ChangedPaths: []string{"add.js"},
		RiskClass: domain.RiskR0, State: domain.CandidateFrozen,
		CreatedAt: now.Add(-30 * time.Minute), FrozenAt: now.Add(-30 * time.Minute),
	}
	verification := domain.VerificationEvidence{
		ID: strings.Repeat("4", 32), CandidateID: candidate.ID, CandidateSHA: candidate.CommitSHA,
		SpecID: spec.ID, SpecHash: spec.ContentHash, PolicyRevision: spec.PolicyRevision,
		Authority: "verifier", Actor: "verifier-1", Independent: true,
		CleanResource: "git-clone:cccccccccccc", OracleDigest: spec.OracleDigest(),
		Status: domain.EvidencePassed, CreatedAt: now.Add(-20 * time.Minute),
	}
	for _, check := range spec.AllChecks() {
		verification.Checks = append(verification.Checks, domain.CheckResult{
			ID: check.ID, Kind: check.Kind, Status: domain.CheckPass,
			Command: check.Command, ExitCode: check.ExpectExit,
		})
	}
	review := domain.ReviewEvidence{
		ID: strings.Repeat("5", 32), CandidateID: candidate.ID, CandidateSHA: candidate.CommitSHA,
		SpecID: spec.ID, SpecHash: spec.ContentHash, PolicyRevision: spec.PolicyRevision,
		Authority: "reviewer", Actor: "reviewer-1", Verdict: domain.ReviewApprove,
		DiffStat: "1 file changed", DiffBytes: 42, CreatedAt: now.Add(-10 * time.Minute),
	}
	return Input{
		Candidate: candidate, Spec: spec,
		Run: domain.Run{
			ID: runID, ProjectID: spec.ProjectID, ActionID: spec.ActionID,
			SpecID: spec.ID, Status: domain.RunSucceeded, ExecutionMode: spec.ExecutionMode,
			Provider: spec.Provider, Model: spec.Model, PaseoMode: spec.PaseoMode,
		},
		Attempt: domain.Attempt{
			ID: attemptID, RunID: runID, WorkerID: "local-1",
			LeaseGeneration: 1, Status: domain.AttemptSucceeded,
		},
		Verifications: []domain.VerificationEvidence{verification},
		Reviews:       []domain.ReviewEvidence{review},
	}
}

func TestEvaluateAcceptsOnlyCompleteConsistentEvidence(t *testing.T) {
	evaluation := Evaluate(gateFixture(t))
	if !evaluation.Final || evaluation.Decision.Status != domain.GateAccepted {
		t.Fatalf("want ACCEPTED final, got final=%v status=%s reasons=%v",
			evaluation.Final, evaluation.Decision.Status, evaluation.Decision.Reasons)
	}
}

func TestEvaluateFailsClosedOnFailingOrMissingEvidence(t *testing.T) {
	failing := gateFixture(t)
	failing.Verifications[0].Checks[1].Status = domain.CheckFail
	if got := Evaluate(failing); got.Decision.Status != domain.GateRepair {
		t.Fatalf("failing mandatory check: %s, want REPAIR_REQUIRED", got.Decision.Status)
	}

	notTested := gateFixture(t)
	notTested.Verifications[0].Checks[2].Status = domain.CheckNotTested
	if got := Evaluate(notTested); got.Decision.Status != domain.GateNotTested {
		t.Fatalf("not-tested check: %s, want NOT_TESTED", got.Decision.Status)
	}

	missingReview := gateFixture(t)
	missingReview.Reviews = nil
	if got := Evaluate(missingReview); got.Final || got.Decision.Status != domain.GateNotTested {
		t.Fatalf("missing review must be provisional NOT_TESTED, got final=%v %s", got.Final, got.Decision.Status)
	}

	changes := gateFixture(t)
	changes.Reviews[0].Verdict = domain.ReviewRequestChanges
	changes.Reviews[0].Findings = []domain.ReviewFinding{{ID: "r1", Severity: "blocking", Message: "x"}}
	if got := Evaluate(changes); got.Decision.Status != domain.GateRepair {
		t.Fatalf("request-changes: %s, want REPAIR_REQUIRED", got.Decision.Status)
	}
}

func TestEvaluateRejectsIdentityMismatchAndProtectedRisk(t *testing.T) {
	mismatch := gateFixture(t)
	mismatch.Verifications[0].CandidateSHA = strings.Repeat("9", 40)
	if got := Evaluate(mismatch); got.Decision.Status != domain.GateBlocked {
		t.Fatalf("identity mismatch: %s, want BLOCKED", got.Decision.Status)
	}

	protected := gateFixture(t)
	protected.Candidate.ChangedPaths = []string{"core/internal/quality/gate.go"}
	protected.Spec.AllowedPaths = []string{"core/internal/quality/gate.go"}
	if got := Evaluate(protected); got.Decision.Status != domain.GateHumanRequired {
		t.Fatalf("protected path: %s, want HUMAN_REQUIRED", got.Decision.Status)
	}
}

func TestClassifyChangeProtectsCredentialAndAuthorityProcessCode(t *testing.T) {
	for _, path := range []string{
		"core/cmd/factory/lifecycle.go",
		"core/cmd/factory-worker/main.go",
		"core/cmd/factory-verifier/main.go",
		"core/cmd/factory-reviewer/main.go",
		"core/internal/candidate/git.go",
	} {
		if got := ClassifyChange([]string{path}); got.RiskClass != "R3" {
			t.Fatalf("%s classified %s, want R3", path, got.RiskClass)
		}
	}
}

func TestReviewerBlockedIsBLOCKEDNotRepair(t *testing.T) {
	fixture := gateFixture(t)
	fixture.Reviews[0].Verdict = domain.ReviewBlocked
	fixture.Reviews[0].Findings = []domain.ReviewFinding{{
		ID: "r1", Severity: "blocking", Message: "cannot review",
	}}
	if got := Evaluate(fixture); got.Decision.Status != domain.GateBlocked {
		t.Fatalf("reviewer BLOCKED: %s, want BLOCKED", got.Decision.Status)
	}
}
