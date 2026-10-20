package api

import (
	"errors"
	"log/slog"
	"net/http"
	"strconv"
	"strings"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/store"
	"github.com/JamesMorales04/software-factory/core/internal/telemetry"
)

// --- Operator: Paseo session observations (read-only; never fabricated) ---

func (s *Server) handleListSessions(w http.ResponseWriter, r *http.Request) {
	runID := r.PathValue("id")
	run, err := s.db.GetRun(runID)
	if err != nil {
		mapDomainError(w, err, s.log, "list sessions")
		return
	}
	bindings, err := s.db.ListPaseoBindings(runID)
	if err != nil {
		mapDomainError(w, err, s.log, "list sessions")
		return
	}
	sessions := make([]domain.PaseoSessionSummary, 0, len(bindings))
	for _, binding := range bindings {
		summary := domain.PaseoSessionSummary{PaseoBinding: binding, RunStatus: run.Status}
		if recovery, rerr := s.db.GetPaseoRecovery(binding.AttemptID); rerr == nil {
			summary.Recovery = &recovery
		} else if !errors.Is(rerr, domain.ErrNotFound) {
			mapDomainError(w, rerr, s.log, "list sessions recovery")
			return
		}
		sessions = append(sessions, summary)
	}
	writeJSON(w, http.StatusOK, map[string]any{"sessions": sessions})
}

// handleListOrphanedSessions exposes the cross-run orphan queue: sessions
// that may still exist externally while their generation is superseded or
// their outcome unconfirmed. S3-B/D cleanup stops them by sessionId (or by
// run/attempt/generation labels when no session was ever bound).
func (s *Server) handleListOrphanedSessions(w http.ResponseWriter, r *http.Request) {
	limit := 100
	if raw := r.URL.Query().Get("limit"); raw != "" {
		n, err := strconv.Atoi(raw)
		if err != nil || n <= 0 || n > 200 {
			writeError(w, http.StatusBadRequest, "invalid", "limit must be an integer 1..200")
			return
		}
		limit = n
	}
	sessions, err := s.db.ListOrphanedBindings(limit)
	if err != nil {
		mapDomainError(w, err, s.log, "list orphaned sessions")
		return
	}
	if sessions == nil {
		sessions = []domain.PaseoBinding{}
	}
	writeJSON(w, http.StatusOK, map[string]any{"sessions": sessions})
}

// handleListAllSessions is the read-only cross-Run projection behind the
// Office Agents view. It returns bounded binding snapshots with the owning
// Run status and the ledger head as an SSE start cursor. No prompt,
// transcript, credential or source body is exposed: the projection is built
// from the durable binding row, not from external Paseo output.
func (s *Server) handleListAllSessions(w http.ResponseWriter, r *http.Request) {
	q := r.URL.Query()
	var status domain.PaseoSessionStatus
	if raw := strings.TrimSpace(q.Get("status")); raw != "" {
		parsed, err := domain.ParsePaseoSessionStatus(raw)
		if err != nil {
			writeError(w, http.StatusBadRequest, "invalid", "status must be one of intended|active|succeeded|failed|cancelled|orphaned")
			return
		}
		status = parsed
	}
	provider := strings.TrimSpace(q.Get("provider"))
	if len(provider) > 64 {
		writeError(w, http.StatusBadRequest, "invalid", "provider filter exceeds 64 characters")
		return
	}
	limit := 100
	if raw := q.Get("limit"); raw != "" {
		n, err := strconv.Atoi(raw)
		if err != nil || n <= 0 || n > 200 {
			writeError(w, http.StatusBadRequest, "invalid", "limit must be an integer 1..200")
			return
		}
		limit = n
	}
	// Read the cursor before the snapshot. Any event committed after this
	// point is delivered by the stream, so a session update cannot fall
	// into the gap between "snapshot read" and "stream start".
	cursor, err := s.db.LatestEventSeq()
	if err != nil {
		mapDomainError(w, err, s.log, "read event cursor")
		return
	}
	sessions, err := s.db.ListPaseoSessionSummaries(status, provider, limit)
	if err != nil {
		mapDomainError(w, err, s.log, "list sessions")
		return
	}
	if sessions == nil {
		sessions = []domain.PaseoSessionSummary{}
	}
	writeJSON(w, http.StatusOK, map[string]any{"sessions": sessions, "eventCursor": cursor})
}

// --- Worker protocol: durable Paseo session identity (scoped worker token) ---

type sessionIdentityRequest struct {
	WorkerID   string `json:"workerId"`
	RunID      string `json:"runId"`
	AttemptID  string `json:"attemptId"`
	Generation int64  `json:"generation"`
}

func (r sessionIdentityRequest) valid() (string, bool) {
	workerID := strings.TrimSpace(r.WorkerID)
	if err := domain.ValidateWorkerID(workerID); err != nil {
		return "", false
	}
	if strings.TrimSpace(r.RunID) == "" || strings.TrimSpace(r.AttemptID) == "" || r.Generation < 1 {
		return "", false
	}
	return workerID, true
}

// validSessionIdentity shares the fencing identity check across the session
// routes without coupling their distinct request payloads.
func validSessionIdentity(workerID, runID, attemptID string, generation int64) (string, bool) {
	return sessionIdentityRequest{
		WorkerID: workerID, RunID: runID, AttemptID: attemptID, Generation: generation,
	}.valid()
}

func (s *Server) handleSessionIntend(w http.ResponseWriter, r *http.Request) {
	var req sessionIdentityRequest
	if !decodeBody(w, r, &req) {
		return
	}
	workerID, ok := req.valid()
	if !ok {
		writeError(w, http.StatusBadRequest, "invalid", "workerId, runId, attemptId and a positive generation are required")
		return
	}
	ctx := r.Context()
	binding, replayed, event, err := s.db.IntendPaseoSession(ctx,
		workerID, req.RunID, req.AttemptID, req.Generation, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapSessionError(w, err, s.log, "session intend")
		return
	}
	if replayed {
		w.Header().Set("Idempotent-Replayed", "true")
		s.log.Info("session intend replayed",
			"workerId", workerID,
			"runId", req.RunID,
			"attemptId", req.AttemptID,
			"generation", req.Generation,
			"correlationId", telemetry.CorrelationIDFrom(ctx))
		writeJSON(w, http.StatusOK, map[string]any{"session": binding})
		return
	}
	s.hub.Publish(event)
	writeJSON(w, http.StatusCreated, map[string]any{"session": binding})
}

type sessionBindRequest struct {
	WorkerID    string `json:"workerId"`
	RunID       string `json:"runId"`
	AttemptID   string `json:"attemptId"`
	Generation  int64  `json:"generation"`
	WorkspaceID string `json:"workspaceId"`
	SessionID   string `json:"sessionId"`
}

func (s *Server) handleSessionBind(w http.ResponseWriter, r *http.Request) {
	var req sessionBindRequest
	if !decodeBody(w, r, &req) {
		return
	}
	workerID, ok := validSessionIdentity(req.WorkerID, req.RunID, req.AttemptID, req.Generation)
	if !ok {
		writeError(w, http.StatusBadRequest, "invalid", "workerId, runId, attemptId and a positive generation are required")
		return
	}
	ctx := r.Context()
	binding, replayed, event, err := s.db.BindPaseoSession(ctx,
		workerID, req.RunID, req.AttemptID, req.Generation,
		strings.TrimSpace(req.WorkspaceID), strings.TrimSpace(req.SessionID),
		telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapSessionError(w, err, s.log, "session bind")
		return
	}
	if replayed {
		w.Header().Set("Idempotent-Replayed", "true")
		writeJSON(w, http.StatusOK, map[string]any{"session": binding})
		return
	}
	s.hub.Publish(event)
	writeJSON(w, http.StatusOK, map[string]any{"session": binding})
}

type sessionCloseRequest struct {
	WorkerID   string `json:"workerId"`
	RunID      string `json:"runId"`
	AttemptID  string `json:"attemptId"`
	Generation int64  `json:"generation"`
	Outcome    string `json:"outcome"`
	Error      string `json:"error"`
}

func (s *Server) handleSessionClose(w http.ResponseWriter, r *http.Request) {
	var req sessionCloseRequest
	if !decodeBody(w, r, &req) {
		return
	}
	workerID, ok := validSessionIdentity(req.WorkerID, req.RunID, req.AttemptID, req.Generation)
	if !ok {
		writeError(w, http.StatusBadRequest, "invalid", "workerId, runId, attemptId and a positive generation are required")
		return
	}
	if strings.TrimSpace(req.Outcome) == "" {
		writeError(w, http.StatusBadRequest, "invalid", "outcome is required (succeeded|failed|cancelled)")
		return
	}
	ctx := r.Context()
	binding, replayed, event, err := s.db.ClosePaseoSession(ctx,
		workerID, req.RunID, req.AttemptID, req.Generation,
		strings.TrimSpace(req.Outcome), req.Error, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapSessionError(w, err, s.log, "session close")
		return
	}
	if replayed {
		w.Header().Set("Idempotent-Replayed", "true")
		writeJSON(w, http.StatusOK, map[string]any{"session": binding})
		return
	}
	s.hub.Publish(event)
	writeJSON(w, http.StatusOK, map[string]any{"session": binding})
}

type sessionOrphanRequest struct {
	WorkerID   string `json:"workerId"`
	RunID      string `json:"runId"`
	AttemptID  string `json:"attemptId"`
	Generation int64  `json:"generation"`
	Reason     string `json:"reason"`
}

func (s *Server) handleSessionOrphan(w http.ResponseWriter, r *http.Request) {
	var req sessionOrphanRequest
	if !decodeBody(w, r, &req) {
		return
	}
	workerID, ok := validSessionIdentity(req.WorkerID, req.RunID, req.AttemptID, req.Generation)
	if !ok {
		writeError(w, http.StatusBadRequest, "invalid", "workerId, runId, attemptId and a positive generation are required")
		return
	}
	ctx := r.Context()
	binding, replayed, event, err := s.db.MarkWorkerSessionOrphaned(ctx,
		workerID, req.RunID, req.AttemptID, req.Generation,
		req.Reason, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapSessionError(w, err, s.log, "session orphan")
		return
	}
	if replayed {
		w.Header().Set("Idempotent-Replayed", "true")
		writeJSON(w, http.StatusOK, map[string]any{"session": binding})
		return
	}
	s.hub.Publish(event)
	writeJSON(w, http.StatusOK, map[string]any{"session": binding})
}

// --- Worker protocol: S3-D recovery claims and receipts ---

type recoveryClaimRequest struct {
	WorkerID string `json:"workerId"`
}

func (s *Server) handleRecoveryClaim(w http.ResponseWriter, r *http.Request) {
	var req recoveryClaimRequest
	if !decodeBody(w, r, &req) {
		return
	}
	workerID := strings.TrimSpace(req.WorkerID)
	if err := domain.ValidateWorkerID(workerID); err != nil {
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
		return
	}
	claim, found, err := s.db.ClaimPaseoRecovery(r.Context(), workerID)
	if err != nil {
		mapRecoveryError(w, err, s.log, "claim paseo recovery")
		return
	}
	if !found {
		w.WriteHeader(http.StatusNoContent)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"claim": claim})
}

type recoveryCompleteRequest struct {
	WorkerID          string `json:"workerId"`
	AttemptID         string `json:"attemptId"`
	ClaimToken        string `json:"claimToken"`
	Outcome           string `json:"outcome"`
	SessionID         string `json:"sessionId"`
	WorkspaceID       string `json:"workspaceId"`
	SessionObserved   string `json:"sessionObserved"`
	WorkspaceObserved string `json:"workspaceObserved"`
	Reason            string `json:"reason"`
}

func (s *Server) handleRecoveryComplete(w http.ResponseWriter, r *http.Request) {
	var req recoveryCompleteRequest
	if !decodeBody(w, r, &req) {
		return
	}
	workerID := strings.TrimSpace(req.WorkerID)
	attemptID := strings.TrimSpace(req.AttemptID)
	claimToken := strings.TrimSpace(req.ClaimToken)
	if err := domain.ValidateWorkerID(workerID); err != nil {
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
		return
	}
	if attemptID == "" || claimToken == "" {
		writeError(w, http.StatusBadRequest, "invalid", "attemptId and claimToken are required")
		return
	}
	if raw := strings.TrimSpace(req.SessionID); raw != "" {
		if err := domain.ValidatePaseoIdentity("sessionId", raw); err != nil {
			writeError(w, http.StatusBadRequest, "invalid", err.Error())
			return
		}
	}
	if raw := strings.TrimSpace(req.WorkspaceID); raw != "" {
		if err := domain.ValidatePaseoIdentity("workspaceId", raw); err != nil {
			writeError(w, http.StatusBadRequest, "invalid", err.Error())
			return
		}
	}
	outcome, err := domain.ParsePaseoRecoveryOutcome(req.Outcome)
	if err != nil {
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
		return
	}
	sessionObs := domain.PaseoObservation(strings.TrimSpace(req.SessionObserved))
	if sessionObs == "" {
		sessionObs = domain.PaseoObservedUnknown
	}
	if err := domain.ValidateSessionObservation(sessionObs); err != nil {
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
		return
	}
	workspaceObs := domain.PaseoObservation(strings.TrimSpace(req.WorkspaceObserved))
	if workspaceObs == "" {
		workspaceObs = domain.PaseoObservedUnknown
	}
	if err := domain.ValidateWorkspaceObservation(workspaceObs); err != nil {
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
		return
	}
	if err := validateRecoveryOutcome(outcome, sessionObs, workspaceObs); err != nil {
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
		return
	}
	ctx := r.Context()
	recovery, replayed, event, err := s.db.CompletePaseoRecovery(ctx, store.RecoveryCompletion{
		WorkerID:          workerID,
		AttemptID:         attemptID,
		ClaimToken:        claimToken,
		Outcome:           outcome,
		SessionID:         strings.TrimSpace(req.SessionID),
		WorkspaceID:       strings.TrimSpace(req.WorkspaceID),
		SessionObserved:   sessionObs,
		WorkspaceObserved: workspaceObs,
		Reason:            req.Reason,
	}, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapRecoveryError(w, err, s.log, "complete paseo recovery")
		return
	}
	if replayed {
		w.Header().Set("Idempotent-Replayed", "true")
		writeJSON(w, http.StatusOK, map[string]any{"recovery": recovery})
		return
	}
	if event.EventID != "" {
		s.hub.Publish(event)
	}
	writeJSON(w, http.StatusOK, map[string]any{"recovery": recovery})
}

// validateRecoveryOutcome keeps the receipt honest: confirmed requires a
// terminal session observation plus workspace absence; absent requires both
// authoritative absence checks. A transient or blocked observation can be
// any bounded value.
func validateRecoveryOutcome(outcome domain.PaseoRecoveryOutcome, session, workspace domain.PaseoObservation) error {
	switch outcome {
	case domain.RecoveryOutcomeConfirmed:
		if session != domain.PaseoObservedArchived && session != domain.PaseoObservedAbsent {
			return errors.New("confirmed recovery requires sessionObserved archived|absent")
		}
		if workspace != domain.PaseoObservedAbsent {
			return errors.New("confirmed recovery requires workspaceObserved absent")
		}
	case domain.RecoveryOutcomeAbsent:
		if session != domain.PaseoObservedAbsent || workspace != domain.PaseoObservedAbsent {
			return errors.New("absent recovery requires sessionObserved and workspaceObserved both absent")
		}
	}
	return nil
}

// mapRecoveryError adds the S3-D claim/identity codes without changing the
// existing session fencing codes.
func mapRecoveryError(w http.ResponseWriter, err error, log *slog.Logger, msg string) {
	switch {
	case errors.Is(err, domain.ErrStaleRecoveryClaim):
		writeError(w, http.StatusConflict, "stale_recovery_claim", "cleanup claim is superseded or expired; abandon this receipt")
	case errors.Is(err, domain.ErrRecoveryIdentityConflict):
		writeError(w, http.StatusConflict, "recovery_identity_conflict", "observed resource contradicts the recorded binding")
	case errors.Is(err, domain.ErrStaleGeneration):
		writeError(w, http.StatusConflict, "stale_generation", "binding is not orphaned or is already superseded")
	default:
		mapDomainError(w, err, log, msg)
	}
}

// mapSessionError translates session fencing outcomes to the same narrow
// codes the Worker already handles for leases, so a superseded session can
// never overwrite authoritative state.
func mapSessionError(w http.ResponseWriter, err error, log *slog.Logger, msg string) {
	switch {
	case isNotFound(err):
		writeError(w, http.StatusNotFound, "not_found", "resource not found")
	case isDomain(err, domain.ErrStaleGeneration):
		writeError(w, http.StatusConflict, "stale_generation", "lease generation is superseded; abandon this attempt")
	case isDomain(err, domain.ErrLeaseExpired):
		writeError(w, http.StatusConflict, "lease_expired", "lease expired; abandon this attempt")
	case isDomain(err, domain.ErrRunCancelled):
		writeError(w, http.StatusConflict, "run_cancelled", "run was cancelled; abandon this attempt")
	case isDomain(err, domain.ErrAttemptNotActive):
		writeError(w, http.StatusConflict, "not_started", "attempt has not started; heartbeat before binding")
	default:
		mapDomainError(w, err, log, msg)
	}
}
