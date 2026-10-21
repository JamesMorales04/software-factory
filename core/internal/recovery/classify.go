// Package recovery classifies pre-candidate Implementer failures with
// deterministic rules. It never calls a model and never grants authority:
// the orchestrator uses the class only to decide whether a bounded retry is
// safe, or whether the campaign must stop.
package recovery

import (
	"strings"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

// Input is the complete observed failure context. CleanupUnconfirmed means
// the Worker reported an unresolved browser/session cleanup; such state may
// never be retried silently.
type Input struct {
	Run                domain.Run
	Spec               domain.ActionSpec
	AttemptsUsed       int
	MaxAttempts        int
	UnresolvedRecovery bool
	UnresolvedReason   string
	CleanupUnconfirmed bool
}

// Result is the classified failure and a bounded, human-readable reason.
type Result struct {
	Class  domain.FailureClass
	Reason string
}

// Classify applies the closed S4-C recovery rules. Unknown failures are
// ambiguous and never retried automatically.
func Classify(in Input) Result {
	failure := strings.TrimSpace(in.Run.Error)
	if failure == "" {
		if detail, ok := in.Run.Result["stage"].(string); ok {
			failure = "paseo-task " + detail
		}
	}
	lower := strings.ToLower(failure)
	if in.UnresolvedRecovery {
		return Result{domain.UnresolvedExternalRecovery, bounded("unresolved external recovery: " + in.UnresolvedReason)}
	}
	if in.CleanupUnconfirmed {
		return Result{domain.UnresolvedExternalRecovery, "external session cleanup is unconfirmed"}
	}
	if strings.Contains(lower, "browser-test-server") ||
		strings.Contains(lower, "capability:") ||
		strings.Contains(lower, "tool unavailable") ||
		strings.Contains(lower, "browser tooling is unavailable") ||
		strings.Contains(lower, "not configured or installed") ||
		strings.Contains(lower, "participant executable") {
		return Result{domain.InfrastructureBlocked, bounded(failure)}
	}
	if strings.Contains(lower, "outside the authorized scope") ||
		strings.Contains(lower, "outside authorized scope") ||
		strings.Contains(lower, "protected acceptance oracle was modified") ||
		strings.Contains(lower, "cross-project") ||
		strings.Contains(lower, "forged") ||
		strings.Contains(lower, "identity mismatch") ||
		strings.Contains(lower, "identity differs") ||
		strings.Contains(lower, "unauthorized") ||
		strings.Contains(lower, "stale generation") ||
		strings.Contains(lower, "candidate rejected") ||
		strings.Contains(lower, "repository identity") ||
		strings.Contains(lower, "base revision") {
		// `candidate rejected` is narrowed below: candidate construction can
		// also fail for benign implementation reasons. Only security-shaped
		// rejections block; the specific rules above take precedence.
		if strings.Contains(lower, "outside the authorized scope") ||
			strings.Contains(lower, "outside authorized scope") ||
			strings.Contains(lower, "protected acceptance oracle was modified") ||
			strings.Contains(lower, "cross-project") ||
			strings.Contains(lower, "forged") ||
			strings.Contains(lower, "unauthorized") ||
			strings.Contains(lower, "identity") ||
			strings.Contains(lower, "repository identity") ||
			strings.Contains(lower, "base revision") {
			return Result{domain.PolicyOrSecurityBlocked, bounded(failure)}
		}
	}
	if strings.Contains(lower, "no-source-change") ||
		strings.Contains(lower, "no source diff") ||
		strings.Contains(lower, "left no source diff") ||
		strings.Contains(lower, "empty candidate") ||
		strings.Contains(lower, "no git diff") ||
		strings.Contains(lower, "tests failed") ||
		strings.Contains(lower, "formatter failed") ||
		strings.Contains(lower, "format failed") ||
		strings.Contains(lower, "syntax") ||
		strings.Contains(lower, "compil") ||
		strings.Contains(lower, "test failure") ||
		strings.Contains(lower, "incomplete change") {
		return Result{domain.RecoverableImplementationFailure, bounded(failure)}
	}
	if strings.Contains(lower, "session-start") ||
		strings.Contains(lower, "workspace-create") ||
		strings.Contains(lower, "startup") ||
		strings.Contains(lower, "timeout") ||
		strings.Contains(lower, "timed out") ||
		strings.Contains(lower, "temporary") ||
		strings.Contains(lower, "connection reset") ||
		strings.Contains(lower, "exit 1") && strings.Contains(lower, "paseo") {
		return Result{domain.RecoverableExecutionFailure, bounded(failure)}
	}
	if strings.Contains(lower, "budget") || strings.Contains(lower, "lease") ||
		strings.Contains(lower, "not authorized") || strings.Contains(lower, "missing") {
		return Result{domain.InfrastructureBlocked, bounded(failure)}
	}
	return Result{domain.OracleDefectSuspected, bounded("ambiguous failure requires independent assessment: " + failure)}
}

// Recoverable reports whether the class authorizes an automatic bounded
// pre-candidate retry under the same frozen spec.
func Recoverable(class domain.FailureClass) bool {
	return class.Recoverable()
}

func bounded(value string) string {
	value = strings.TrimSpace(value)
	if len(value) > domain.MaxReasonLength {
		value = value[:domain.MaxReasonLength]
	}
	return value
}
