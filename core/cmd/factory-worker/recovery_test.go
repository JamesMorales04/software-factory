package main

import (
	"testing"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/paseo"
)

func recoveryClaimForTest() domain.PaseoRecoveryClaim {
	runID := "11111111111111111111111111111111"
	attemptID := "22222222222222222222222222222222"
	key := domain.PaseoCorrelationKey(runID, 2)
	binding := domain.PaseoBinding{
		AttemptID:       attemptID,
		RunID:           runID,
		Seq:             2,
		WorkerID:        "local-1",
		LeaseGeneration: 2,
		Provider:        "codex",
		Model:           "gpt-6-luna",
		Mode:            "full-access",
		ExecMode:        "trusted-local",
		WorkspaceID:     "ws_owned",
		SessionID:       "33333333-3333-4333-8333-333333333333",
		CorrelationKey:  key,
		Status:          domain.PaseoOrphaned,
	}
	return domain.PaseoRecoveryClaim{
		ClaimToken: "claim-token",
		Session:    binding,
		Recovery:   domain.PaseoRecovery{AttemptID: attemptID, RunID: runID, LeaseGeneration: 2, Status: domain.RecoveryClaimed},
	}
}

func matchingFake(claim domain.PaseoRecoveryClaim) *paseo.FakeAdapter {
	f := paseo.NewFakeAdapter()
	labels := domain.PaseoCorrelationLabels(claim.Session.RunID, claim.Session.AttemptID,
		claim.Session.LeaseGeneration, claim.Session.WorkspaceID)
	f.Session = paseo.Identity{
		WorkspaceID: claim.Session.WorkspaceID,
		SessionID:   claim.Session.SessionID,
		Provider:    claim.Session.Provider,
		Model:       claim.Session.Model,
		Mode:        claim.Session.Mode,
		Cwd:         "/tmp/recovery",
		Status:      "idle",
		Title:       claim.Session.CorrelationKey,
	}
	f.Sessions = []paseo.SessionRef{{
		ID: claim.Session.SessionID, Title: claim.Session.CorrelationKey,
		Provider: "codex", Status: "idle", Labels: labels,
	}}
	f.Workspaces = []paseo.WorkspaceRef{{
		ID: claim.Session.WorkspaceID, Title: claim.Session.CorrelationKey, Cwd: "/tmp/recovery",
	}}
	f.Listed[claim.Session.WorkspaceID] = true
	return f
}

func TestReconcilePaseoResourcesConfirmed(t *testing.T) {
	claim := recoveryClaimForTest()
	f := matchingFake(claim)
	result := reconcilePaseoResources(t.Context(), f, claim)
	if result.Outcome != domain.RecoveryOutcomeConfirmed {
		t.Fatalf("outcome = %s (%s), want confirmed", result.Outcome, result.Reason)
	}
	if result.SessionObserved != domain.PaseoObservedArchived || result.WorkspaceObserved != domain.PaseoObservedAbsent {
		t.Fatalf("observations = %s/%s", result.SessionObserved, result.WorkspaceObserved)
	}
	for _, op := range []string{"stop", "archive", "workspace-archive"} {
		if !f.Called(op) {
			t.Fatalf("%s was not called", op)
		}
	}
}

func TestReconcilePaseoResourcesAbsent(t *testing.T) {
	claim := recoveryClaimForTest()
	f := paseo.NewFakeAdapter()
	f.Fail["inspect"] = paseo.FailOp("inspect", "agent not found")
	result := reconcilePaseoResources(t.Context(), f, claim)
	if result.Outcome != domain.RecoveryOutcomeAbsent {
		t.Fatalf("outcome = %s (%s), want absent", result.Outcome, result.Reason)
	}
	if f.Called("stop") || f.Called("archive") || f.Called("workspace-archive") {
		t.Fatal("absence must not issue destructive calls")
	}
}

func TestReconcilePaseoResourcesBlocksIdentityMismatch(t *testing.T) {
	claim := recoveryClaimForTest()
	f := matchingFake(claim)
	f.Session.Provider = "opencode"
	f.Session.Model = "opencode-go/deepseek-v4.1-flash"
	f.Session.Mode = "build"
	result := reconcilePaseoResources(t.Context(), f, claim)
	if result.Outcome != domain.RecoveryOutcomeBlocked {
		t.Fatalf("outcome = %s, want blocked", result.Outcome)
	}
	if f.Called("stop") || f.Called("archive") || f.Called("workspace-archive") {
		t.Fatal("identity mismatch must not issue destructive calls")
	}
}

func TestReconcilePaseoResourcesBlocksWrongTitle(t *testing.T) {
	claim := recoveryClaimForTest()
	f := matchingFake(claim)
	f.Session.Title = domain.PaseoLegacyTitle(claim.Session.RunID)
	result := reconcilePaseoResources(t.Context(), f, claim)
	if result.Outcome != domain.RecoveryOutcomeBlocked {
		t.Fatalf("outcome = %s, want blocked", result.Outcome)
	}
	if f.Called("stop") || f.Called("archive") || f.Called("workspace-archive") {
		t.Fatal("title mismatch must not issue destructive calls")
	}
}

func TestReconcilePaseoResourcesBlocksAmbiguity(t *testing.T) {
	claim := recoveryClaimForTest()
	f := matchingFake(claim)
	f.Sessions = append(f.Sessions, f.Sessions[0])
	f.Sessions[1].ID = "44444444-4444-4444-8444-444444444444"
	result := reconcilePaseoResources(t.Context(), f, claim)
	if result.Outcome != domain.RecoveryOutcomeBlocked {
		t.Fatalf("outcome = %s, want blocked", result.Outcome)
	}
	if f.Called("stop") || f.Called("archive") || f.Called("workspace-archive") {
		t.Fatal("ambiguous identity must not issue destructive calls")
	}
}

func TestReconcilePaseoResourcesBlocksLegacyUnbound(t *testing.T) {
	claim := recoveryClaimForTest()
	claim.Session.CorrelationKey = ""
	claim.Session.SessionID = ""
	claim.Session.WorkspaceID = ""
	f := paseo.NewFakeAdapter()
	result := reconcilePaseoResources(t.Context(), f, claim)
	if result.Outcome != domain.RecoveryOutcomeBlocked {
		t.Fatalf("outcome = %s, want blocked", result.Outcome)
	}
	if f.Called("stop") || f.Called("archive") || f.Called("workspace-archive") {
		t.Fatal("legacy unbound ownership must not issue destructive calls")
	}
}

func TestReconcilePaseoResourcesUnconfirmedOnArchiveFailure(t *testing.T) {
	claim := recoveryClaimForTest()
	f := matchingFake(claim)
	f.Fail["archive"] = paseo.FailOp("archive", "daemon unavailable")
	result := reconcilePaseoResources(t.Context(), f, claim)
	if result.Outcome != domain.RecoveryOutcomeUnconfirmed {
		t.Fatalf("outcome = %s, want unconfirmed", result.Outcome)
	}
	if f.Called("workspace-archive") {
		t.Fatal("workspace must not be archived after unconfirmed session cleanup")
	}
}
