package recovery

import (
	"testing"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

func input(err string) Input {
	return Input{Run: domain.Run{Error: err}, AttemptsUsed: 1, MaxAttempts: 4}
}

func TestClassifyRecoverableFailures(t *testing.T) {
	cases := []string{
		"paseo-task verify: no-source-change: agent left no git diff",
		"candidate: implementer left no source diff; an empty candidate is not success",
		"paseo-task verify: tests failed: npm exited 1",
		"paseo-task verify: formatter failed: npm exited 1",
		"paseo-task session-start: paseo run failed (startup): exit 1",
	}
	for _, failure := range cases {
		got := Classify(input(failure))
		if !Recoverable(got.Class) {
			t.Fatalf("%q classified as %s, want recoverable", failure, got.Class)
		}
	}
}

func TestClassifyBlockingFailures(t *testing.T) {
	cases := []string{
		"candidate: candidate rejected: candidate changed path \"tests/browser-check.mjs\" outside the authorized scope",
		"paseo-task verify: protected acceptance oracle was modified by the candidate: browser-check.mjs",
		"candidate: cross-project evidence mismatch",
		"candidate: repository identity differs from the frozen project binding",
		"capability: browser capability is required but Playwright CLI/Chromium unavailable",
		"browser-test-server: authorized endpoint is occupied by an unowned process",
	}
	for _, failure := range cases {
		got := Classify(input(failure))
		if got.Class != domain.PolicyOrSecurityBlocked && got.Class != domain.InfrastructureBlocked {
			t.Fatalf("%q classified as %s, want blocked", failure, got.Class)
		}
	}
}

func TestClassifyUnresolvedAndAmbiguous(t *testing.T) {
	got := Classify(Input{
		Run:                domain.Run{Error: "paseo-task observe: cancelled"},
		UnresolvedRecovery: true, UnresolvedReason: "paseo session binding active is not terminal",
		AttemptsUsed: 1, MaxAttempts: 4,
	})
	if got.Class != domain.UnresolvedExternalRecovery {
		t.Fatalf("unresolved recovery classified as %s", got.Class)
	}
	got = Classify(Input{
		Run:          domain.Run{Error: "something completely unexpected"},
		AttemptsUsed: 1, MaxAttempts: 4,
	})
	if got.Class != domain.OracleDefectSuspected {
		t.Fatalf("ambiguous failure classified as %s", got.Class)
	}
	got = Classify(Input{
		Run:          domain.Run{Error: "paseo-task verify: no-source-change"},
		AttemptsUsed: 4, MaxAttempts: 4,
	})
	if got.Class != domain.RecoverableImplementationFailure {
		t.Fatal("recoverable failure lost its classification at the budget boundary")
	}
}
