// Package main: S3-B real Paseo task execution.
//
// runPaseoLifecycle executes one "paseo-task" Attempt through a real Paseo
// session while preserving every S2 guarantee: lease heartbeats, fencing
// (abandon on 404/409/expiry/cancel), intend-before-create, bind-then-close
// ordering, and report-only-after-session-close. A successful result
// requires a real completed session with an accepted binding and
// task-specific evidence (real git diff + passing checks) — never a bare
// CLI exit status.
//
// TRUSTED_LOCAL — broad local access, not isolated. This path runs the
// host Paseo CLI with the operator's HOME/daemon authentication under
// owner-accepted local trust (ADR-0009). It is never labeled isolated,
// sandboxed, certified or SEC-01 PASS. The S2 RESTRICTED synthetic path is
// unchanged.
package main

import (
	"context"
	"errors"
	"log/slog"
	"strings"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/exec"
	"github.com/JamesMorales04/software-factory/core/internal/paseo"
)

// Budgets inside the Attempt execution window. The session must finish its
// first turn with enough reserve left for one bounded nudge, evidence
// verification, session close/report and cleanup.
const (
	paseoReserveAfterSession = 240 * time.Second
	paseoMinFirstWait        = 60 * time.Second
	paseoNudgeWait           = 120 * time.Second
	paseoVerifyReserve       = 120 * time.Second
	paseoCleanupTimeout      = 45 * time.Second
	paseoCmdTimeout          = 30 * time.Second
	paseoTestTimeout         = 120 * time.Second
)

// sessionBackend abstracts the Core session/report calls so the lifecycle
// is deterministically testable without HTTP.
type sessionBackend interface {
	intendSession(ctx context.Context, workerID, runID, attemptID string, generation int64) (domain.PaseoBinding, error)
	bindSession(ctx context.Context, workerID, runID, attemptID string, generation int64, workspaceID, sessionID string) (domain.PaseoBinding, error)
	closeSession(ctx context.Context, workerID, runID, attemptID string, generation int64, outcome, errMsg string) (domain.PaseoBinding, error)
	orphanSession(ctx context.Context, workerID, runID, attemptID string, generation int64, reason string) (domain.PaseoBinding, error)
}

// fenceError marks Core fencing rejections (unknown identity, stale
// generation, expired lease, cancelled run): the Attempt is terminal for
// this Worker and must be abandoned without reporting.
type fenceError struct {
	msg string
}

func (e *fenceError) Error() string { return e.msg }

// runAttemptWorkload dispatches one Attempt to the synthetic fixture or
// the real Paseo lifecycle. The Paseo path runs on the host CLI under
// owner-accepted TRUSTED_LOCAL trust: broad local access, never isolated.
func (c *client) runAttemptWorkload(
	ctx context.Context,
	workerID string,
	run domain.Run,
	attempt domain.Attempt,
	spec exec.Spec,
	workDir string,
	podmanOK bool,
	log *slog.Logger,
) (map[string]any, error) {
	if run.WorkloadKind == domain.PaseoTaskKind {
		return runPaseoLifecycle(ctx, c, workerID, run, attempt, workDir, paseo.NewCLIAdapter(log), log)
	}
	result, err := exec.Run(ctx, spec, workDir, podmanOK)
	if err != nil {
		return nil, err
	}
	output := map[string]any{
		"mode":          string(result.Mode),
		"steps":         result.StepsCompleted,
		"receipt":       result.Receipt,
		"messageDigest": result.MessageDigest,
		"elapsedMs":     result.ElapsedMs,
	}
	if result.ImageDigest != "" {
		output["imageDigest"] = result.ImageDigest
	}
	return output, nil
}

// runPaseoLifecycle runs the real Paseo lifecycle for one Attempt. It
// returns the report output with a nil error only when the session closed
// successfully with verified evidence; any other outcome returns the
// (smaller) failed output and the bounded failure reason.
func runPaseoLifecycle(
	ctx context.Context,
	backend sessionBackend,
	workerID string,
	run domain.Run,
	attempt domain.Attempt,
	workDir string,
	adapter paseo.Adapter,
	log *slog.Logger,
) (map[string]any, error) {
	stage := "validate-routing"
	fail := func(stage, reason string, binding *domain.PaseoBinding) (map[string]any, error) {
		reason = truncate("paseo-task " + stage + ": " + reason)
		log.Error("paseo-task failed", "stage", stage, "reason", reason)
		out := map[string]any{"stage": stage}
		if binding != nil {
			out["sessionId"] = binding.SessionID
			out["workspaceId"] = binding.WorkspaceID
			out["provider"] = binding.Provider
			out["model"] = binding.Model
			out["mode"] = binding.Mode
		}
		return out, errors.New(reason)
	}

	task, _ := run.WorkloadInput["task"].(string)
	if err := domain.ValidateExecutionMode(run.ExecutionMode); err != nil ||
		domain.ExecutionMode(run.ExecutionMode) != domain.ExecTrustedLocal {
		return fail(stage, "executionMode must be trusted-local", nil)
	}
	route, err := domain.ValidatePaseoRoute(run.Provider, run.Model, run.PaseoMode)
	if err != nil {
		return fail(stage, "unauthorized route (no substitution)", nil)
	}
	log.Info("TRUSTED_LOCAL paseo-task: broad local access, not isolated",
		"provider", route.Provider, "model", route.Model, "mode", route.Mode)

	if version, err := adapterProbe(ctx, adapter); err != nil {
		log.Warn("paseo probe failed; continuing to honest startup-failure path", "error", err.Error())
	} else {
		log.Info("paseo CLI available", "version", version)
	}

	// 1. Persist session intent before any non-idempotent external call.
	stage = "intend"
	binding, err := backend.intendSession(ctx, workerID, run.ID, attempt.ID, attempt.LeaseGeneration)
	if err != nil {
		var fe *fenceError
		if errors.As(err, &fe) {
			return nil, fe
		}
		return fail(stage, shortErr(err), nil)
	}

	// 2. Seed a disposable baseline fixture when the Attempt workspace is
	// empty, so acceptance evidence (read → edit → format/tests → diff)
	// is reproducible. Non-empty workspaces are left untouched. A dead
	// context orphans: cancellation owns the outcome, not a failure claim.
	stage = "seed-workspace"
	if err := seedPaseoFixture(ctx, workDir); err != nil {
		if ctx.Err() != nil {
			return orphanExternal(backend, context.Background(), workerID, run, attempt, binding, adapter, identity0(""), stage, "seed: "+shortErr(err)+"; cancelled")
		}
		_, _ = backend.closeSession(context.Background(), workerID, run.ID, attempt.ID, attempt.LeaseGeneration, string(domain.PaseoFailed), "seed: "+shortErr(err))
		return fail(stage, shortErr(err), &binding)
	}

	// 3. Create the real Paseo workspace rooted at the Attempt workspace.
	// The title is the full Run-and-generation correlation key, never a
	// short prefix; recovery uses it for exact ownership discovery.
	stage = "workspace-create"
	correlationKey := binding.CorrelationKey
	if correlationKey == "" {
		correlationKey = domain.PaseoLegacyTitle(run.ID)
	}
	workspaceID, err := adapter.CreateWorkspace(ctx, workDir, correlationKey)
	if err != nil {
		if ctx.Err() != nil {
			return orphanExternal(backend, context.Background(), workerID, run, attempt, binding, adapter, identity0(""), stage, "workspace-create: "+shortErr(err)+"; cancelled")
		}
		_, _ = backend.closeSession(context.Background(), workerID, run.ID, attempt.ID, attempt.LeaseGeneration, string(domain.PaseoFailed), "workspace-create: "+shortErr(err))
		return fail(stage, shortErr(err), &binding)
	}
	log.Info("paseo workspace created", "workspaceId", workspaceID)

	// 4. Bound the session's first turn by the remaining lease budget.
	remaining := time.Until(deadlineOf(ctx)) - paseoReserveAfterSession
	if remaining < paseoMinFirstWait {
		if ctx.Err() != nil {
			return orphanExternal(backend, context.Background(), workerID, run, attempt, binding, adapter, identity0(workspaceID), "budget", "insufficient lease budget; cancelled")
		}
		if cerr := cleanupPaseoExternal(context.Background(), adapter, "", workspaceID); cerr != nil {
			return orphanExternal(backend, context.Background(), workerID, run, attempt, binding, adapter, identity0(workspaceID), "budget", "insufficient lease budget; workspace "+workspaceID+" may need manual archive")
		}
		_, _ = backend.closeSession(context.Background(), workerID, run.ID, attempt.ID, attempt.LeaseGeneration, string(domain.PaseoFailed), "insufficient lease budget for a real session")
		return fail("budget", "insufficient lease budget for a real session", &binding)
	}

	// 5. Start the requested session. The adapter asserts the effective
	// provider/model/mode from inspect evidence; mismatch fails here.
	labels := domain.PaseoCorrelationLabels(run.ID, attempt.ID, attempt.LeaseGeneration, workspaceID)
	identity, err := adapter.StartSession(ctx, paseo.StartRequest{
		Provider:    route.Provider,
		Model:       route.Model,
		Mode:        route.Mode,
		WorkspaceID: workspaceID,
		Title:       correlationKey,
		Labels:      labels,
		Prompt:      paseoTaskPrompt(task, workDir),
		WaitTimeout: remaining,
	})
	if err != nil {
		if identity.SessionID != "" {
			// The session started but its identity is unproven: never
			// adopt it, never claim a clean startup failure.
			identity.WorkspaceID = workspaceID
			return orphanExternal(backend, context.Background(), workerID, run, attempt, binding, adapter, identity, "session-start", "session-start: "+shortErr(err))
		}
		if externalUncertain(err, ctx) {
			return orphanExternal(backend, context.Background(), workerID, run, attempt, binding, adapter, identity0(workspaceID), "session-start", "session-start: "+shortErr(err))
		}
		return abandonExternal(backend, context.Background(), adapter, workerID, run, attempt, binding, workspaceID, "session-start", "session-start: "+shortErr(err))
	}
	identity.WorkspaceID = workspaceID
	if err := paseo.AssertRoute(route.Provider, route.Model, route.Mode, workDir, identity); err != nil {
		// The session exists but its identity is unproven: never adopt
		// it, never claim a clean startup failure. Stop best-effort and
		// keep it discoverable in the orphan queue.
		return orphanExternal(backend, context.Background(), workerID, run, attempt, binding, adapter, identity, "assert-identity", "assert-identity: "+shortErr(err))
	}
	log.Info("paseo session observed", "sessionId", identity.SessionID,
		"provider", identity.Provider, "model", identity.Model, "mode", identity.Mode,
		"cwd", identity.Cwd, "status", identity.Status)

	// 6. Bind the proven identity to this Attempt generation.
	binding, err = backend.bindSession(ctx, workerID, run.ID, attempt.ID, attempt.LeaseGeneration, workspaceID, identity.SessionID)
	if err != nil {
		var fe *fenceError
		if errors.As(err, &fe) {
			_ = cleanupPaseoExternal(context.Background(), adapter, identity.SessionID, workspaceID)
			_, _ = backend.orphanSession(context.Background(), workerID, run.ID, attempt.ID, attempt.LeaseGeneration, "bind fenced; external session state unconfirmed")
			return nil, fe
		}
		// The session exists but was never bound: keep it discoverable
		// rather than claiming a startup failure.
		return orphanExternal(backend, context.Background(), workerID, run, attempt, binding, adapter, identity, "bind", "bind: "+shortErr(err))
	}

	// 7-8. Observe the timeline and collect task evidence. One bounded
	// nudge is allowed when the agent acted but left no diff.
	nudges := 0
	summary, evidence, err := collectPaseoEvidence(ctx, adapter, workDir, identity)
	if err != nil {
		if externalUncertain(err, ctx) {
			return orphanExternal(backend, context.Background(), workerID, run, attempt, binding, adapter, identity, "observe", "observe cancelled; session state unconfirmed")
		}
		if summary.ToolSteps > 0 && ctxBudgetLeft(ctx) > paseoVerifyReserve {
			log.Info("empty diff after productive turn; one bounded nudge", "toolSteps", summary.ToolSteps)
			if nerr := adapter.SendNudge(ctx, identity.SessionID, paseoNudgePrompt(workDir), paseoNudgeWait); nerr == nil {
				nudges = 1
				summary, evidence, err = collectPaseoEvidence(ctx, adapter, workDir, identity)
			} else {
				log.Warn("nudge failed; evaluating partial evidence", "error", shortErr(nerr))
			}
		}
		if err != nil {
			_, _ = backend.closeSession(context.Background(), workerID, run.ID, attempt.ID, attempt.LeaseGeneration, string(domain.PaseoFailed), shortErr(err))
			_ = cleanupPaseoExternal(context.Background(), adapter, identity.SessionID, workspaceID)
			out, ferr := fail("verify", shortErr(err), &binding)
			out["nudges"] = nudges
			return out, ferr
		}
	}

	// 9. Re-assert identity from final evidence (no substitution across
	// the turn), then close the binding with the truthful outcome.
	stage = "reinspect"
	final, err := adapter.InspectSession(ctx, identity.SessionID)
	if err != nil {
		return orphanExternal(backend, context.Background(), workerID, run, attempt, binding, adapter, identity, "reinspect", "reinspect failed; session state unconfirmed")
	}
	final.WorkspaceID = workspaceID
	final.SessionID = identity.SessionID
	if err := paseo.AssertRoute(route.Provider, route.Model, route.Mode, workDir, final); err != nil {
		_, _ = backend.closeSession(context.Background(), workerID, run.ID, attempt.ID, attempt.LeaseGeneration, string(domain.PaseoFailed), "identity drift: "+shortErr(err))
		_ = cleanupPaseoExternal(context.Background(), adapter, identity.SessionID, workspaceID)
		out, ferr := fail(stage, shortErr(err), &binding)
		out["nudges"] = nudges
		return out, ferr
	}

	stage = "close"
	closed, err := backend.closeSession(context.Background(), workerID, run.ID, attempt.ID, attempt.LeaseGeneration, string(domain.PaseoSucceeded), "")
	if err != nil {
		var fe *fenceError
		if errors.As(err, &fe) {
			_ = cleanupPaseoExternal(context.Background(), adapter, identity.SessionID, workspaceID)
			return nil, fe
		}
		_ = cleanupPaseoExternal(context.Background(), adapter, identity.SessionID, workspaceID)
		out, ferr := fail(stage, shortErr(err), &binding)
		out["nudges"] = nudges
		return out, ferr
	}

	// 10. Cleanup owned external resources only after the terminal close.
	// The result stays truthful when cleanup is unconfirmed (the work
	// completed and the close is recorded); the cleanup status is
	// carried in the output so a live artifact never hides.
	cleanupNote := "ok"
	if cerr := cleanupPaseoExternal(context.Background(), adapter, identity.SessionID, workspaceID); cerr != nil {
		cleanupNote = "unconfirmed: " + shortErr(cerr)
		log.Warn("paseo cleanup unconfirmed after successful close", "note", cleanupNote)
	}

	// Report the final observed identity, not the frozen requested binding.
	// AssertRoute above proved these values equal the requested route; the
	// values themselves come from the last Paseo inspect evidence so the
	// projection's "observed route" has real provenance.
	out := map[string]any{
		"sessionId":     closed.SessionID,
		"workspaceId":   closed.WorkspaceID,
		"provider":      final.Provider,
		"model":         final.Model,
		"paseoMode":     final.Mode,
		"cwd":           final.Cwd,
		"timelineLines": evidence.timelineLines,
		"timelineSteps": evidence.stepsSummary,
		"toolSteps":     evidence.toolSteps,
		"diffStat":      evidence.diffStat,
		"diffBytes":     evidence.diffBytes,
		"testExit":      evidence.testExit,
		"formatExit":    evidence.formatExit,
		"nudges":        nudges,
		"cleanup":       cleanupNote,
	}
	return out, nil
}

// abandonExternal stops/archives best-effort and closes the binding
// failed. When cleanup itself fails the workspace may still exist: the
// binding is orphaned instead so the workspace identity stays
// discoverable in the orphan reason rather than leaking silently.
func abandonExternal(backend sessionBackend,
	ctx context.Context,
	adapter paseo.Adapter,
	workerID string,
	run domain.Run,
	attempt domain.Attempt,
	binding domain.PaseoBinding,
	workspaceID, stage, reason string,
) (map[string]any, error) {
	if cerr := cleanupPaseoExternal(ctx, adapter, "", workspaceID); cerr != nil && workspaceID != "" {
		return orphanExternal(backend, ctx, workerID, run, attempt, binding, adapter,
			paseo.Identity{WorkspaceID: workspaceID}, stage,
			reason+"; cleanup: "+shortErr(cerr))
	}
	_, _ = backend.closeSession(ctx, workerID, run.ID, attempt.ID, attempt.LeaseGeneration, string(domain.PaseoFailed), reason)
	out := map[string]any{"stage": stage}
	if binding.AttemptID != "" {
		out["workspaceId"] = workspaceID
		out["provider"] = binding.Provider
		out["model"] = binding.Model
		out["mode"] = binding.Mode
	}
	return out, errors.New(truncate(reason))
}

// orphanExternal records an unprovable session honestly and returns the
// failed output. Used when cancellation, fence races or failed stops
// leave external state uncertain.
func orphanExternal(backend sessionBackend,
	ctx context.Context,
	workerID string,
	run domain.Run,
	attempt domain.Attempt,
	binding domain.PaseoBinding,
	adapter paseo.Adapter,
	identity paseo.Identity,
	stage, reason string,
) (map[string]any, error) {
	_ = cleanupPaseoExternal(ctx, adapter, identity.SessionID, identity.WorkspaceID)
	_, _ = backend.orphanSession(ctx, workerID, run.ID, attempt.ID, attempt.LeaseGeneration, reason)
	out := map[string]any{
		"stage":       stage,
		"sessionId":   identity.SessionID,
		"workspaceId": identity.WorkspaceID,
		"provider":    binding.Provider,
		"model":       binding.Model,
		"mode":        binding.Mode,
	}
	return out, errors.New(truncate(reason))
}

// cleanupPaseoExternal stops, archives the session and archives the owned
// workspace (best effort, bounded). A nil adapter or empty identities are
// no-ops so failure paths stay total. Every failed step is reported in the
// returned error: callers must never claim "ok" when a stop, archive or
// verification step is unconfirmed.
func cleanupPaseoExternal(ctx context.Context, adapter paseo.Adapter, sessionID, workspaceID string) error {
	if adapter == nil {
		return nil
	}
	ctx, cancel := context.WithTimeout(ctx, paseoCleanupTimeout)
	defer cancel()
	var notes []string
	if strings.TrimSpace(sessionID) != "" {
		if err := adapter.StopSession(ctx, sessionID); err != nil {
			notes = append(notes, "stop: "+shortErr(err))
		}
		if err := adapter.ArchiveSession(ctx, sessionID); err != nil {
			notes = append(notes, "archive-session: "+shortErr(err))
		}
	}
	if strings.TrimSpace(workspaceID) != "" {
		if err := adapter.ArchiveWorkspace(ctx, workspaceID); err != nil {
			notes = append(notes, "archive-workspace: "+shortErr(err))
		} else if listed, err := adapter.WorkspaceListed(ctx, workspaceID); err != nil {
			notes = append(notes, "verify-workspace: "+shortErr(err))
		} else if listed {
			notes = append(notes, "workspace still listed after archive")
		}
	}
	if len(notes) > 0 {
		return errors.New(truncate(strings.Join(notes, "; ")))
	}
	return nil
}

// adapterProbe reports the CLI version without failing the Attempt.
func adapterProbe(ctx context.Context, adapter paseo.Adapter) (string, error) {
	type prober interface {
		Probe(ctx context.Context) (string, error)
	}
	p, ok := adapter.(prober)
	if !ok {
		return "unknown (no probe)", nil
	}
	ctx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	return p.Probe(ctx)
}

// deadlineOf returns the context deadline or a near-term fallback so
// budget math stays total.
func deadlineOf(ctx context.Context) time.Time {
	if d, ok := ctx.Deadline(); ok {
		return d
	}
	return time.Now().Add(paseoMinFirstWait)
}

// ctxBudgetLeft reports the remaining context budget or zero.
func ctxBudgetLeft(ctx context.Context) time.Duration {
	left := time.Until(deadlineOf(ctx))
	if left < 0 {
		return 0
	}
	return left
}

// identity0 builds a best-known identity before a session exists.
func identity0(workspaceID string) paseo.Identity {
	return paseo.Identity{WorkspaceID: workspaceID}
}

// externalUncertain reports whether a failed Paseo call leaves external
// session state unconfirmed: the caller's context ended, or the adapter
// classifies the failure as cancellation/timeout. Anything else is a
// clean failure with no proven side effect.
func externalUncertain(err error, ctx context.Context) bool {
	if ctx.Err() != nil {
		return true
	}
	var pe *paseo.Error
	if errors.As(err, &pe) {
		return pe.Kind == paseo.KindCancelled || pe.Kind == paseo.KindTimeout
	}
	return false
}

// shortErr bounds an error for reasons and logs without external bodies.
func shortErr(err error) string {
	if err == nil {
		return ""
	}
	var pe *paseo.Error
	if errors.As(err, &pe) {
		return truncate(pe.Error())
	}
	return truncate(err.Error())
}
