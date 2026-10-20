package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"path/filepath"
	"strings"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/paseo"
)

// S3-D recovery loop: one Worker-owned coordinator claims orphaned
// bindings from Core, observes the owned external resources, performs only
// idempotent stop/archive operations on positively identified resources,
// and records a bounded receipt. It never rebinds, resumes a superseded
// generation or creates a second authoritative result.
const (
	recoveryOperationTimeout = 2 * time.Minute
	recoverySessionTimeout   = 90 * time.Second
)

type recoveryOutcome struct {
	Outcome           domain.PaseoRecoveryOutcome
	SessionID         string
	WorkspaceID       string
	SessionObserved   domain.PaseoObservation
	WorkspaceObserved domain.PaseoObservation
	Reason            string
}

// recoveryLoop claims at most one orphan per tick. A zero interval disables
// the loop explicitly (unit tests and synthetic-only deployments).
func (c *client) recoveryLoop(ctx context.Context, workerID string, interval time.Duration, log *slog.Logger) {
	if interval <= 0 {
		return
	}
	adapter := paseo.NewCLIAdapter(log)
	c.runRecoveryOnce(ctx, workerID, adapter, log)
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			c.runRecoveryOnce(ctx, workerID, adapter, log)
		}
	}
}

func (c *client) runRecoveryOnce(ctx context.Context, workerID string, adapter paseo.Adapter, log *slog.Logger) {
	claim, found, err := c.claimRecovery(ctx, workerID)
	if err != nil {
		log.Warn("paseo recovery claim failed", "error", err.Error())
		return
	}
	if !found {
		return
	}
	rlog := log.With("attemptId", claim.Session.AttemptID, "runId", claim.Session.RunID,
		"generation", claim.Session.LeaseGeneration)
	opCtx, cancel := context.WithTimeout(ctx, recoveryOperationTimeout)
	defer cancel()
	result := reconcilePaseoResources(opCtx, adapter, claim)
	recovery, replayed, cerr := c.completeRecovery(context.WithoutCancel(ctx), workerID,
		claim.Session.AttemptID, claim.ClaimToken, result)
	if cerr != nil {
		rlog.Warn("paseo recovery completion rejected", "error", cerr.Error())
		return
	}
	rlog.Info("paseo recovery recorded",
		"outcome", string(recovery.Status), "replayed", replayed,
		"sessionId", result.SessionID, "workspaceId", result.WorkspaceID,
		"reason", result.Reason)
}

// reconcilePaseoResources is the deterministic observation/cleanup state
// machine. Every destructive call is preceded by exact ownership checks.
func reconcilePaseoResources(ctx context.Context, adapter paseo.Adapter, claim domain.PaseoRecoveryClaim) recoveryOutcome {
	binding := claim.Session
	key := strings.TrimSpace(binding.CorrelationKey)
	legacy := key == ""
	title := key
	if legacy {
		title = domain.PaseoLegacyTitle(binding.RunID)
	}
	result := recoveryOutcome{
		SessionObserved:   domain.PaseoObservedUnknown,
		WorkspaceObserved: domain.PaseoObservedUnknown,
		SessionID:         binding.SessionID,
		WorkspaceID:       binding.WorkspaceID,
	}
	blocked := func(reason string) recoveryOutcome {
		result.Outcome = domain.RecoveryOutcomeBlocked
		result.Reason = reason
		return result
	}

	// Workspace discovery first: for an unbound session the durable
	// workspace ID is not known yet, so the exact title must establish it
	// before the full label set can be used for session matching.
	workspaces, werr := adapter.ListWorkspaces(ctx)
	if werr != nil {
		result.Outcome = domain.RecoveryOutcomeUnconfirmed
		result.Reason = "workspace listing unavailable: " + shortErr(werr)
		return result
	}
	var workspace *paseo.WorkspaceRef
	if binding.WorkspaceID != "" {
		workspace = findWorkspace(workspaces, binding.WorkspaceID)
		if workspace != nil && workspace.Title != title {
			result.WorkspaceObserved = domain.PaseoObservedPresent
			return blocked("workspace title does not match the durable correlation key")
		}
	} else if legacy {
		return blocked("legacy workspace ownership unproven: no durable workspace identity or correlation key")
	} else {
		matches := findWorkspacesByTitle(workspaces, title)
		if len(matches) > 1 {
			return blocked("ambiguous workspace correlation: multiple exact title matches")
		}
		if len(matches) == 1 {
			workspace = &matches[0]
		}
	}
	if workspace == nil {
		result.WorkspaceObserved = domain.PaseoObservedAbsent
	} else {
		result.WorkspaceObserved = domain.PaseoObservedPresent
		result.WorkspaceID = workspace.ID
	}

	workspaceIDForLabels := binding.WorkspaceID
	if workspaceIDForLabels == "" && workspace != nil {
		workspaceIDForLabels = workspace.ID
	}
	labels := domain.PaseoCorrelationLabels(binding.RunID, binding.AttemptID,
		binding.LeaseGeneration, workspaceIDForLabels)
	var listed []paseo.SessionRef
	var lerr error
	if legacy {
		listed, lerr = adapter.ListSessions(ctx)
	} else {
		listed, lerr = adapter.ListSessionsByLabels(ctx, labels)
	}
	if lerr != nil {
		result.Outcome = domain.RecoveryOutcomeUnconfirmed
		result.Reason = "session listing unavailable: " + shortErr(lerr)
		return result
	}

	var observed *paseo.Identity
	switch {
	case binding.SessionID != "":
		insp, err := adapter.InspectSession(ctx, binding.SessionID)
		if err == nil {
			observed = &insp
			if !legacy && workspace != nil {
				if len(listed) != 1 || listed[0].ID != binding.SessionID {
					return blocked("recorded session does not match the exact correlation labels")
				}
			}
			break
		}
		// Inspect can fail because the agent is gone or because the daemon
		// is unavailable. The successful global listing distinguishes the
		// two: archived agents remain listed.
		if findSession(listed, binding.SessionID) != nil {
			result.Outcome = domain.RecoveryOutcomeUnconfirmed
			result.Reason = "session inspect failed: " + shortErr(err)
			return result
		}
		result.SessionObserved = domain.PaseoObservedAbsent
	case legacy:
		// A legacy intended binding with no session ID has no positive
		// ownership evidence. A short title is not enough.
		return blocked("legacy session ownership unproven: no durable session identity or labels")
	default:
		if workspace == nil {
			// Without the owned workspace there is no way to verify the
			// full five-label identity for an unbound session. Only an
			// authoritative empty four-label listing proves absence.
			if len(listed) > 0 {
				return blocked("workspace identity unavailable for full label verification")
			}
			result.SessionObserved = domain.PaseoObservedAbsent
			break
		}
		if len(listed) > 1 {
			return blocked("ambiguous session correlation: multiple exact label matches")
		}
		if len(listed) == 0 {
			result.SessionObserved = domain.PaseoObservedAbsent
			break
		}
		insp, err := adapter.InspectSession(ctx, listed[0].ID)
		if err != nil {
			result.Outcome = domain.RecoveryOutcomeUnconfirmed
			result.Reason = "discovered session inspect failed: " + shortErr(err)
			return result
		}
		observed = &insp
	}

	if observed != nil {
		result.SessionID = observed.SessionID
		if observed.Title != title {
			result.SessionObserved = sessionObservation(observed)
			return blocked("session title does not match the durable correlation key")
		}
		if err := paseo.AssertRoute(binding.Provider, binding.Model, binding.Mode, "", *observed); err != nil {
			result.SessionObserved = sessionObservation(observed)
			return blocked("session identity mismatch: " + shortErr(err))
		}
		result.SessionObserved = sessionObservation(observed)
	}

	if observed != nil && workspace != nil && observed.Cwd != "" && workspace.Cwd != "" &&
		filepath.Clean(observed.Cwd) != filepath.Clean(workspace.Cwd) {
		return blocked("session cwd does not match the owned workspace")
	}

	cleaned := false
	if observed != nil && !observed.Archived {
		if err := stopAndArchiveSession(ctx, adapter, *observed); err != nil {
			result.SessionObserved = sessionObservation(observed)
			result.Outcome = domain.RecoveryOutcomeUnconfirmed
			result.Reason = "session cleanup unconfirmed: " + shortErr(err)
			return result
		}
		observed.Archived = true
		observed.Status = "archived"
		result.SessionObserved = domain.PaseoObservedArchived
		cleaned = true
	}
	if workspace != nil {
		if err := archiveWorkspace(ctx, adapter, *workspace); err != nil {
			result.WorkspaceObserved = domain.PaseoObservedPresent
			result.Outcome = domain.RecoveryOutcomeUnconfirmed
			result.Reason = "workspace cleanup unconfirmed: " + shortErr(err)
			return result
		}
		result.WorkspaceObserved = domain.PaseoObservedAbsent
		cleaned = true
	}

	// Re-check for a session created during the cleanup window. Archived
	// sessions remain listed, so a new non-archived exact match means the
	// state is not settled.
	if !legacy {
		after, err := adapter.ListSessionsByLabels(ctx, labels)
		if err != nil {
			result.Outcome = domain.RecoveryOutcomeUnconfirmed
			result.Reason = "post-cleanup session listing unavailable: " + shortErr(err)
			return result
		}
		for _, ref := range after {
			insp, ierr := adapter.InspectSession(ctx, ref.ID)
			if ierr != nil || !insp.Archived {
				result.SessionObserved = domain.PaseoObservedActive
				result.Outcome = domain.RecoveryOutcomeUnconfirmed
				result.Reason = "a matching session is still observable after cleanup"
				return result
			}
		}
	}

	switch {
	case result.SessionObserved == domain.PaseoObservedAbsent && result.WorkspaceObserved == domain.PaseoObservedAbsent:
		if cleaned {
			result.Outcome = domain.RecoveryOutcomeConfirmed
		} else {
			result.Outcome = domain.RecoveryOutcomeAbsent
		}
	case result.SessionObserved == domain.PaseoObservedArchived && result.WorkspaceObserved == domain.PaseoObservedAbsent:
		result.Outcome = domain.RecoveryOutcomeConfirmed
	default:
		result.Outcome = domain.RecoveryOutcomeUnconfirmed
		result.Reason = "external state is not fully settled"
	}
	return result
}

func sessionObservation(identity *paseo.Identity) domain.PaseoObservation {
	if identity == nil {
		return domain.PaseoObservedUnknown
	}
	if identity.Archived {
		return domain.PaseoObservedArchived
	}
	switch strings.ToLower(strings.TrimSpace(identity.Status)) {
	case "running", "working", "busy", "starting":
		return domain.PaseoObservedActive
	case "idle", "permission", "error", "completed":
		return domain.PaseoObservedIdle
	default:
		return domain.PaseoObservedUnknown
	}
}

func stopAndArchiveSession(ctx context.Context, adapter paseo.Adapter, identity paseo.Identity) error {
	sctx, cancel := context.WithTimeout(ctx, recoverySessionTimeout)
	defer cancel()
	var notes []string
	if err := adapter.StopSession(sctx, identity.SessionID); err != nil {
		notes = append(notes, "stop: "+shortErr(err))
	}
	if err := adapter.ArchiveSession(sctx, identity.SessionID); err != nil {
		notes = append(notes, "archive: "+shortErr(err))
	}
	// Verify archived/absent. A failed inspect is resolved against the
	// authoritative global listing; an unprovable state stays unconfirmed.
	insp, err := adapter.InspectSession(sctx, identity.SessionID)
	if err == nil && insp.Archived {
		// The terminal observation is authoritative even when a redundant
		// stop reported a harmless no-op failure.
		return nil
	}
	listed, lerr := adapter.ListSessions(ctx)
	if lerr == nil && findSession(listed, identity.SessionID) == nil {
		return nil
	}
	if err != nil {
		notes = append(notes, "verify: "+shortErr(err))
	}
	if len(notes) == 0 {
		notes = append(notes, "session is still present after archive")
	}
	return errors.New(strings.Join(notes, "; "))
}

func archiveWorkspace(ctx context.Context, adapter paseo.Adapter, workspace paseo.WorkspaceRef) error {
	sctx, cancel := context.WithTimeout(ctx, recoverySessionTimeout)
	defer cancel()
	if err := adapter.ArchiveWorkspace(sctx, workspace.ID); err != nil {
		return err
	}
	listed, err := adapter.ListWorkspaces(sctx)
	if err != nil {
		return err
	}
	if findWorkspace(listed, workspace.ID) != nil {
		return errors.New("workspace is still listed after archive")
	}
	return nil
}

func findSession(sessions []paseo.SessionRef, id string) *paseo.SessionRef {
	for i := range sessions {
		if sessions[i].ID == id {
			return &sessions[i]
		}
	}
	return nil
}

func findWorkspace(workspaces []paseo.WorkspaceRef, id string) *paseo.WorkspaceRef {
	for i := range workspaces {
		if workspaces[i].ID == id {
			return &workspaces[i]
		}
	}
	return nil
}

func findWorkspacesByTitle(workspaces []paseo.WorkspaceRef, title string) []paseo.WorkspaceRef {
	var out []paseo.WorkspaceRef
	for _, ws := range workspaces {
		if ws.Title == title {
			out = append(out, ws)
		}
	}
	return out
}

// --- Core recovery protocol ---

type recoveryClaimEnvelope struct {
	Claim domain.PaseoRecoveryClaim `json:"claim"`
}

func (c *client) claimRecovery(ctx context.Context, workerID string) (domain.PaseoRecoveryClaim, bool, error) {
	status, raw, _, err := c.do(ctx, http.MethodPost, "/api/v1/workers/recovery/claim",
		map[string]any{"workerId": workerID})
	if err != nil {
		return domain.PaseoRecoveryClaim{}, false, err
	}
	switch status {
	case http.StatusNoContent:
		return domain.PaseoRecoveryClaim{}, false, nil
	case http.StatusOK:
		var out recoveryClaimEnvelope
		if jerr := json.Unmarshal(raw, &out); jerr != nil {
			return domain.PaseoRecoveryClaim{}, false, errors.New("recovery claim: bad payload")
		}
		if out.Claim.ClaimToken == "" || out.Claim.Session.AttemptID == "" {
			return domain.PaseoRecoveryClaim{}, false, errors.New("recovery claim: incomplete payload")
		}
		return out.Claim, true, nil
	default:
		return domain.PaseoRecoveryClaim{}, false,
			fmt.Errorf("recovery claim status %d (%s)", status, errorCode(raw))
	}
}

func (c *client) completeRecovery(ctx context.Context, workerID, attemptID, token string, result recoveryOutcome) (domain.PaseoRecovery, bool, error) {
	status, raw, headers, err := c.do(ctx, http.MethodPost, "/api/v1/workers/recovery/complete",
		map[string]any{
			"workerId":          workerID,
			"attemptId":         attemptID,
			"claimToken":        token,
			"outcome":           string(result.Outcome),
			"sessionId":         result.SessionID,
			"workspaceId":       result.WorkspaceID,
			"sessionObserved":   string(result.SessionObserved),
			"workspaceObserved": string(result.WorkspaceObserved),
			"reason":            result.Reason,
		})
	if err != nil {
		return domain.PaseoRecovery{}, false, err
	}
	if status != http.StatusOK {
		return domain.PaseoRecovery{}, false,
			fmt.Errorf("recovery complete status %d (%s): %s", status, errorCode(raw), truncate(string(raw)))
	}
	var out struct {
		Recovery domain.PaseoRecovery `json:"recovery"`
	}
	if jerr := json.Unmarshal(raw, &out); jerr != nil {
		return domain.PaseoRecovery{}, false, errors.New("recovery complete: bad payload")
	}
	return out.Recovery, headers.Get("Idempotent-Replayed") == "true", nil
}
