package api

import (
	"log/slog"
	"net/http"
	"strconv"
	"strings"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/telemetry"
)

// --- Operator: Paseo session observations (read-only; never fabricated) ---

func (s *Server) handleListSessions(w http.ResponseWriter, r *http.Request) {
	sessions, err := s.db.ListPaseoBindings(r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "list sessions")
		return
	}
	if sessions == nil {
		sessions = []domain.PaseoBinding{}
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
