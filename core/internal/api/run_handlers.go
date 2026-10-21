package api

import (
	"errors"
	"log/slog"
	"net/http"
	"strconv"
	"strings"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/telemetry"
)

func isDomain(err, target error) bool { return errors.Is(err, target) }

func isNotFound(err error) bool { return errors.Is(err, domain.ErrNotFound) }

// --- Operator: Runs ---

type createRunRequest struct {
	ProjectID     string         `json:"projectId"`
	ActionID      string         `json:"actionId"`
	WorkloadKind  string         `json:"workloadKind"`
	WorkloadInput map[string]any `json:"workloadInput"`
	MaxAttempts   int            `json:"maxAttempts"`
	TimeoutSecs   int            `json:"timeoutSecs"`
	// S3-A: requested execution mode and Paseo routing. Empty means the S2
	// default (restricted synthetic execution).
	ExecutionMode string `json:"executionMode"`
	Provider      string `json:"provider"`
	Model         string `json:"model"`
	PaseoMode     string `json:"paseoMode"`
	// S4-A: the frozen engineering specification is authoritative for
	// the Action, task, routing and execution budget.
	SpecID string `json:"specId"`
}

func (s *Server) handleCreateRun(w http.ResponseWriter, r *http.Request) {
	var req createRunRequest
	if !decodeBody(w, r, &req) {
		return
	}
	if strings.TrimSpace(req.SpecID) != "" {
		spec, err := s.db.GetActionSpec(strings.TrimSpace(req.SpecID))
		if err != nil {
			mapDomainError(w, err, s.log, "create run spec")
			return
		}
		if err := applySpecToRunRequest(&req, spec); err != nil {
			writeError(w, http.StatusBadRequest, "invalid", err.Error())
			return
		}
	}
	params, err := domain.NewRunParams{
		ProjectID:     req.ProjectID,
		ActionID:      strings.TrimSpace(req.ActionID),
		WorkloadKind:  strings.TrimSpace(req.WorkloadKind),
		WorkloadInput: req.WorkloadInput,
		MaxAttempts:   req.MaxAttempts,
		TimeoutSecs:   req.TimeoutSecs,
		ExecutionMode: strings.TrimSpace(req.ExecutionMode),
		Provider:      strings.TrimSpace(req.Provider),
		Model:         strings.TrimSpace(req.Model),
		PaseoMode:     strings.TrimSpace(req.PaseoMode),
		SpecID:        strings.TrimSpace(req.SpecID),
	}.Validate()
	if err != nil {
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
		return
	}
	ctx := r.Context()
	run, event, err := s.db.CreateRun(ctx, params, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapDomainError(w, err, s.log, "create run")
		return
	}
	s.hub.Publish(event)
	s.log.Info("run queued",
		"runId", run.ID,
		"projectId", run.ProjectID,
		"correlationId", telemetry.CorrelationIDFrom(ctx))
	writeJSON(w, http.StatusCreated, map[string]any{"run": run})
}

// applySpecToRunRequest binds a Run to the frozen spec. Conflicting caller
// values are rejected rather than silently reinterpreted; derived values
// come only from the spec and its immutable routing/budget.
func applySpecToRunRequest(req *createRunRequest, spec domain.ActionSpec) error {
	if req.ActionID != "" && req.ActionID != spec.ActionID {
		return errors.New("actionId conflicts with the frozen specification")
	}
	if req.ProjectID != "" && req.ProjectID != spec.ProjectID {
		return errors.New("projectId conflicts with the frozen specification")
	}
	if req.WorkloadKind != "" && req.WorkloadKind != domain.PaseoTaskKind {
		return errors.New("spec-bound runs require workloadKind paseo-task")
	}
	if req.ExecutionMode != "" && req.ExecutionMode != spec.ExecutionMode {
		return errors.New("executionMode conflicts with the frozen specification")
	}
	if req.Provider != "" && req.Provider != spec.Provider {
		return errors.New("provider conflicts with the frozen specification")
	}
	if req.Model != "" && req.Model != spec.Model {
		return errors.New("model conflicts with the frozen specification")
	}
	if req.PaseoMode != "" && req.PaseoMode != spec.PaseoMode {
		return errors.New("paseoMode conflicts with the frozen specification")
	}
	req.ActionID = spec.ActionID
	req.ProjectID = spec.ProjectID
	req.WorkloadKind = domain.PaseoTaskKind
	req.WorkloadInput = map[string]any{"task": spec.Goal}
	req.ExecutionMode = spec.ExecutionMode
	req.Provider = spec.Provider
	req.Model = spec.Model
	req.PaseoMode = spec.PaseoMode
	req.MaxAttempts = spec.Budget.MaxAttempts
	req.TimeoutSecs = spec.Budget.TimeoutSecs
	return nil
}

func (s *Server) handleListRuns(w http.ResponseWriter, r *http.Request) {
	q := r.URL.Query()
	var status domain.RunStatus
	if raw := q.Get("status"); raw != "" {
		var err error
		status, err = domain.ParseRunStatus(raw)
		if err != nil {
			writeError(w, http.StatusBadRequest, "invalid", err.Error())
			return
		}
	}
	limit := 100
	if raw := q.Get("limit"); raw != "" {
		n, err := strconv.Atoi(raw)
		if err != nil || n <= 0 {
			writeError(w, http.StatusBadRequest, "invalid", "limit must be a positive integer")
			return
		}
		limit = n
	}
	runs, err := s.db.ListRuns(status, limit)
	if err != nil {
		mapDomainError(w, err, s.log, "list runs")
		return
	}
	if runs == nil {
		runs = []domain.Run{}
	}
	writeJSON(w, http.StatusOK, map[string]any{"runs": runs})
}

func (s *Server) handleGetRun(w http.ResponseWriter, r *http.Request) {
	run, err := s.db.GetRun(r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "get run")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"run": run})
}

func (s *Server) handleListAttempts(w http.ResponseWriter, r *http.Request) {
	attempts, err := s.db.ListAttempts(r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "list attempts")
		return
	}
	if attempts == nil {
		attempts = []domain.Attempt{}
	}
	writeJSON(w, http.StatusOK, map[string]any{"attempts": attempts})
}

func (s *Server) handleCancelRun(w http.ResponseWriter, r *http.Request) {
	ctx := r.Context()
	run, events, err := s.db.CancelRun(ctx, r.PathValue("id"), telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapDomainError(w, err, s.log, "cancel run")
		return
	}
	for _, event := range events {
		s.hub.Publish(event)
	}
	s.log.Info("run cancelled",
		"runId", run.ID,
		"correlationId", telemetry.CorrelationIDFrom(ctx))
	writeJSON(w, http.StatusOK, map[string]any{"run": run})
}

func (s *Server) handleRunEvents(w http.ResponseWriter, r *http.Request) {
	if _, err := s.db.GetRun(r.PathValue("id")); err != nil {
		mapDomainError(w, err, s.log, "run events lookup")
		return
	}
	events, err := s.db.ListAggregateEvents(domain.AggregateRun, r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "list run events")
		return
	}
	if events == nil {
		events = []domain.Event{}
	}
	writeJSON(w, http.StatusOK, map[string]any{"events": projectEvents(events)})
}

// --- Operator: Worker observations (read-only; never fabricated) ---

func (s *Server) handleListWorkers(w http.ResponseWriter, r *http.Request) {
	workers, err := s.db.ListWorkers()
	if err != nil {
		mapDomainError(w, err, s.log, "list workers")
		return
	}
	if workers == nil {
		workers = []domain.Worker{}
	}
	writeJSON(w, http.StatusOK, map[string]any{"workers": workers})
}

func (s *Server) handleGetWorker(w http.ResponseWriter, r *http.Request) {
	worker, err := s.db.GetWorker(r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "get worker")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"worker": worker})
}

// --- Worker protocol (scoped worker token; operator token rejected) ---

type registerWorkerRequest struct {
	WorkerID     string   `json:"workerId"`
	Version      string   `json:"version"`
	Capabilities []string `json:"capabilities"`
	ExecModes    []string `json:"execModes"`
}

func (s *Server) handleWorkerRegister(w http.ResponseWriter, r *http.Request) {
	var req registerWorkerRequest
	if !decodeBody(w, r, &req) {
		return
	}
	req.WorkerID = strings.TrimSpace(req.WorkerID)
	if err := domain.ValidateWorkerID(req.WorkerID); err != nil {
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
		return
	}
	ctx := r.Context()
	capabilities, err := domain.NormalizeCapabilities(req.Capabilities)
	if err != nil {
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
		return
	}
	execModes := make([]string, 0, len(req.ExecModes))
	for _, mode := range req.ExecModes {
		mode = strings.TrimSpace(mode)
		if mode != "" {
			execModes = append(execModes, mode)
		}
	}
	worker, event, err := s.db.RegisterWorker(ctx, req.WorkerID, strings.TrimSpace(req.Version),
		capabilities, execModes, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapDomainError(w, err, s.log, "register worker")
		return
	}
	if event == nil {
		w.Header().Set("Idempotent-Replayed", "true")
		s.log.Info("worker re-registered (replay)",
			"workerId", worker.ID,
			"correlationId", telemetry.CorrelationIDFrom(ctx))
		writeJSON(w, http.StatusOK, map[string]any{"worker": worker})
		return
	}
	s.hub.Publish(*event)
	s.log.Info("worker registered",
		"workerId", worker.ID,
		"correlationId", telemetry.CorrelationIDFrom(ctx))
	writeJSON(w, http.StatusCreated, map[string]any{"worker": worker})
}

type workerIdentityRequest struct {
	WorkerID string `json:"workerId"`
}

func (s *Server) handleWorkerPoll(w http.ResponseWriter, r *http.Request) {
	var req workerIdentityRequest
	if !decodeBody(w, r, &req) {
		return
	}
	req.WorkerID = strings.TrimSpace(req.WorkerID)
	if err := domain.ValidateWorkerID(req.WorkerID); err != nil {
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
		return
	}
	ctx := r.Context()
	run, attempt, found, event, err := s.db.ClaimNextRun(ctx, req.WorkerID, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapDomainError(w, err, s.log, "worker poll")
		return
	}
	if !found {
		w.WriteHeader(http.StatusNoContent)
		return
	}
	s.hub.Publish(event)
	s.log.Info("run leased",
		"workerId", req.WorkerID,
		"runId", run.ID,
		"attemptId", attempt.ID,
		"generation", attempt.LeaseGeneration,
		"correlationId", telemetry.CorrelationIDFrom(ctx))
	writeJSON(w, http.StatusOK, map[string]any{"run": run, "attempt": attempt})
}

type heartbeatRequest struct {
	WorkerID   string `json:"workerId"`
	RunID      string `json:"runId"`
	AttemptID  string `json:"attemptId"`
	Generation int64  `json:"generation"`
}

func (s *Server) handleWorkerHeartbeat(w http.ResponseWriter, r *http.Request) {
	var req heartbeatRequest
	if !decodeBody(w, r, &req) {
		return
	}
	req.WorkerID = strings.TrimSpace(req.WorkerID)
	if err := domain.ValidateWorkerID(req.WorkerID); err != nil {
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
		return
	}
	if strings.TrimSpace(req.RunID) == "" || strings.TrimSpace(req.AttemptID) == "" || req.Generation < 1 {
		writeError(w, http.StatusBadRequest, "invalid", "runId, attemptId and a positive generation are required")
		return
	}
	ctx := r.Context()
	run, event, err := s.db.HeartbeatLease(ctx, req.WorkerID, req.RunID, req.AttemptID, req.Generation, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapWorkerError(w, err, s.log, "worker heartbeat")
		return
	}
	if event != nil {
		s.hub.Publish(*event)
	}
	writeJSON(w, http.StatusOK, map[string]any{"run": run})
}

type reportRequest struct {
	WorkerID   string         `json:"workerId"`
	RunID      string         `json:"runId"`
	AttemptID  string         `json:"attemptId"`
	Generation int64          `json:"generation"`
	Outcome    string         `json:"outcome"`
	Output     map[string]any `json:"output"`
	Error      string         `json:"error"`
}

func (s *Server) handleWorkerReport(w http.ResponseWriter, r *http.Request) {
	var req reportRequest
	if !decodeBody(w, r, &req) {
		return
	}
	req.WorkerID = strings.TrimSpace(req.WorkerID)
	if err := domain.ValidateWorkerID(req.WorkerID); err != nil {
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
		return
	}
	if strings.TrimSpace(req.RunID) == "" || strings.TrimSpace(req.AttemptID) == "" || req.Generation < 1 {
		writeError(w, http.StatusBadRequest, "invalid", "runId, attemptId and a positive generation are required")
		return
	}
	outcome, err := domain.ParseOutcome(req.Outcome)
	if err != nil {
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
		return
	}
	ctx := r.Context()
	run, replayed, events, err := s.db.ReportAttempt(ctx,
		req.WorkerID, req.RunID, req.AttemptID, req.Generation,
		outcome, req.Output, req.Error, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapWorkerError(w, err, s.log, "worker report")
		return
	}
	if replayed {
		w.Header().Set("Idempotent-Replayed", "true")
		s.log.Info("worker report replayed",
			"workerId", req.WorkerID,
			"runId", req.RunID,
			"attemptId", req.AttemptID,
			"generation", req.Generation,
			"correlationId", telemetry.CorrelationIDFrom(ctx))
		writeJSON(w, http.StatusOK, map[string]any{"run": run})
		return
	}
	for _, event := range events {
		s.hub.Publish(event)
	}
	s.log.Info("worker report recorded",
		"workerId", req.WorkerID,
		"runId", run.ID,
		"outcome", string(outcome),
		"correlationId", telemetry.CorrelationIDFrom(ctx))
	writeJSON(w, http.StatusOK, map[string]any{"run": run})
}

// mapWorkerError translates fencing outcomes to explicit narrow codes so a
// Worker can distinguish a superseded lease from a missing identity.
func mapWorkerError(w http.ResponseWriter, err error, log *slog.Logger, msg string) {
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
		writeError(w, http.StatusConflict, "not_started", "attempt has not started; heartbeat before reporting")
	default:
		mapDomainError(w, err, log, msg)
	}
}
