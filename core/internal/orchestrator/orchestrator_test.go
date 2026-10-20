package orchestrator

import (
	"strings"
	"testing"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
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
