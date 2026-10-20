package api

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"strconv"
	"strings"

	"github.com/JamesMorales04/software-factory/core/internal/candidate"
	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/quality"
	"github.com/JamesMorales04/software-factory/core/internal/telemetry"
)

// --- Operator: frozen engineering specifications ---

type createSpecRequest struct {
	ProjectID          string             `json:"projectId"`
	ActionID           string             `json:"actionId"`
	Goal               string             `json:"goal"`
	Repository         string             `json:"repository"`
	BaseRevision       string             `json:"baseRevision"`
	AllowedPaths       []string           `json:"allowedPaths"`
	AcceptanceCriteria []domain.SpecCheck `json:"acceptanceCriteria"`
	NegativeCases      []domain.SpecCheck `json:"negativeCases"`
	RiskClass          string             `json:"riskClass"`
	QualityProfile     string             `json:"qualityProfile"`
	Budget             domain.SpecBudget  `json:"budget"`
	ExecutionMode      string             `json:"executionMode"`
	Provider           string             `json:"provider"`
	Model              string             `json:"model"`
	PaseoMode          string             `json:"paseoMode"`
}

func (s *Server) handleCreateActionSpec(w http.ResponseWriter, r *http.Request) {
	var req createSpecRequest
	if !decodeBody(w, r, &req) {
		return
	}
	params := domain.NewActionSpecParams{
		ProjectID: req.ProjectID, ActionID: req.ActionID, Goal: req.Goal,
		Repository: req.Repository, BaseRevision: req.BaseRevision,
		AllowedPaths: req.AllowedPaths, AcceptanceCriteria: req.AcceptanceCriteria,
		NegativeCases: req.NegativeCases, RiskClass: req.RiskClass,
		QualityProfile: req.QualityProfile, Budget: req.Budget,
		ExecutionMode: req.ExecutionMode, Provider: req.Provider,
		Model: req.Model, PaseoMode: req.PaseoMode,
	}
	spec, err := params.Validate()
	if err != nil {
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
		return
	}
	ctx := r.Context()
	if err := candidate.ValidateRepository(ctx, spec.Repository, spec.BaseRevision); err != nil {
		writeError(w, http.StatusBadRequest, "invalid", "authorized repository/base revision rejected: "+err.Error())
		return
	}
	stored, replayed, event, err := s.db.CreateActionSpec(ctx, spec, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapDomainError(w, err, s.log, "create action spec")
		return
	}
	if replayed {
		w.Header().Set("Idempotent-Replayed", "true")
		writeJSON(w, http.StatusOK, map[string]any{"spec": stored})
		return
	}
	s.hub.Publish(event)
	s.log.Info("action spec frozen",
		"specId", stored.ID, "actionId", stored.ActionID,
		"contentHash", stored.ContentHash,
		"correlationId", telemetry.CorrelationIDFrom(ctx))
	writeJSON(w, http.StatusCreated, map[string]any{"spec": stored})
}

func (s *Server) handleGetActionSpec(w http.ResponseWriter, r *http.Request) {
	spec, err := s.db.GetActionSpec(r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "get action spec")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"spec": spec})
}

func (s *Server) handleListActionSpecs(w http.ResponseWriter, r *http.Request) {
	if _, err := s.db.GetAction(r.PathValue("id")); err != nil {
		mapDomainError(w, err, s.log, "list action specs")
		return
	}
	specs, err := s.db.ListActionSpecs(r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "list action specs")
		return
	}
	if specs == nil {
		specs = []domain.ActionSpec{}
	}
	writeJSON(w, http.StatusOK, map[string]any{"specs": specs})
}

// --- Operator: candidate evidence projections ---

func (s *Server) handleListCandidates(w http.ResponseWriter, r *http.Request) {
	limit := 100
	if raw := r.URL.Query().Get("limit"); raw != "" {
		n, err := strconv.Atoi(raw)
		if err != nil || n <= 0 || n > 200 {
			writeError(w, http.StatusBadRequest, "invalid", "limit must be an integer 1..200")
			return
		}
		limit = n
	}
	candidates, err := s.db.ListCandidates(
		strings.TrimSpace(r.URL.Query().Get("actionId")),
		strings.TrimSpace(r.URL.Query().Get("runId")), limit)
	if err != nil {
		mapDomainError(w, err, s.log, "list candidates")
		return
	}
	if candidates == nil {
		candidates = []domain.CandidateRevision{}
	}
	writeJSON(w, http.StatusOK, map[string]any{"candidates": candidates})
}

func (s *Server) handleGetCandidate(w http.ResponseWriter, r *http.Request) {
	c, err := s.db.GetCandidate(r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "get candidate")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"candidate": c})
}

func (s *Server) handleCandidateEvidence(w http.ResponseWriter, r *http.Request) {
	bundle, err := s.db.GetCandidateBundle(r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "candidate evidence")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"bundle": bundle})
}

func (s *Server) handleRunCandidate(w http.ResponseWriter, r *http.Request) {
	if _, err := s.db.GetRun(r.PathValue("id")); err != nil {
		mapDomainError(w, err, s.log, "run candidate")
		return
	}
	c, err := s.db.LatestRunCandidate(r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "run candidate")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"candidate": c})
}

func (s *Server) handleEvaluateGate(w http.ResponseWriter, r *http.Request) {
	ctx := r.Context()
	decision, final, replayed, err := s.evaluateCandidate(ctx, r.PathValue("id"), telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapDomainError(w, err, s.log, "evaluate quality gate")
		return
	}
	if replayed {
		w.Header().Set("Idempotent-Replayed", "true")
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"decision": decision, "final": final, "replayed": replayed,
	})
}

// evaluateCandidate is the only path that persists a final Quality Gate
// decision. It is deterministic, idempotent and Core-owned. A provisional
// NOT_TESTED evaluation is returned but never stored.
func (s *Server) evaluateCandidate(ctx context.Context, candidateID, correlationID string) (domain.GateDecision, bool, bool, error) {
	for attempt := 0; attempt < 3; attempt++ {
		if existing, err := s.db.GetGateDecision(candidateID); err == nil {
			return existing, true, true, nil
		} else if !errors.Is(err, domain.ErrNotFound) {
			return domain.GateDecision{}, false, false, err
		}
		bundle, err := s.db.GetCandidateBundle(candidateID)
		if err != nil {
			return domain.GateDecision{}, false, false, err
		}
		run, err := s.db.GetRun(bundle.Candidate.ImplementerRunID)
		if err != nil {
			return domain.GateDecision{}, false, false, err
		}
		attemptRecord, err := s.db.GetAttempt(bundle.Candidate.ImplementerRunID, bundle.Candidate.ImplementerAttemptID)
		if err != nil {
			return domain.GateDecision{}, false, false, err
		}
		evaluation := quality.Evaluate(quality.Input{
			Candidate: bundle.Candidate, Spec: bundle.Spec, Run: run, Attempt: attemptRecord,
			Verifications: bundle.Verifications, Reviews: bundle.Reviews,
		})
		if !evaluation.Final {
			return evaluation.Decision, false, false, nil
		}
		decision, replayed, event, err := s.db.SaveGateDecision(ctx, evaluation.Decision, correlationID)
		if errors.Is(err, domain.ErrConflict) {
			// New evidence committed between read and save; re-evaluate the
			// complete durable set instead of persisting a stale decision.
			continue
		}
		if err != nil {
			return domain.GateDecision{}, false, false, err
		}
		if !replayed {
			s.hub.Publish(event)
			s.log.Info("quality gate decided",
				"candidateId", decision.CandidateID,
				"status", string(decision.Status),
				"correlationId", correlationID)
		}
		return decision, true, replayed, nil
	}
	return domain.GateDecision{}, false, false, fmt.Errorf("%w: evidence kept changing during gate evaluation",
		domain.ErrConflict)
}

// --- Worker: frozen spec retrieval and candidate registration ---

func (s *Server) handleWorkerSpec(w http.ResponseWriter, r *http.Request) {
	spec, err := s.db.GetActionSpec(r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "worker spec")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"spec": spec})
}

type workerCandidateRequest struct {
	WorkerID     string `json:"workerId"`
	RunID        string `json:"runId"`
	AttemptID    string `json:"attemptId"`
	Generation   int64  `json:"generation"`
	SpecID       string `json:"specId"`
	CommitSHA    string `json:"commitSha"`
	CandidateRef string `json:"candidateRef"`
}

func (s *Server) handleWorkerCandidate(w http.ResponseWriter, r *http.Request) {
	var req workerCandidateRequest
	if !decodeBody(w, r, &req) {
		return
	}
	req.WorkerID = strings.TrimSpace(req.WorkerID)
	if err := domain.ValidateWorkerID(req.WorkerID); err != nil {
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
		return
	}
	if req.Generation < 1 || strings.TrimSpace(req.RunID) == "" ||
		strings.TrimSpace(req.AttemptID) == "" || strings.TrimSpace(req.SpecID) == "" {
		writeError(w, http.StatusBadRequest, "invalid", "runId, attemptId, specId and a positive generation are required")
		return
	}
	ctx := r.Context()
	run, err := s.db.GetRun(req.RunID)
	if err != nil {
		mapDomainError(w, err, s.log, "candidate run")
		return
	}
	attempt, err := s.db.GetAttempt(req.RunID, req.AttemptID)
	if err != nil {
		mapDomainError(w, err, s.log, "candidate attempt")
		return
	}
	if run.SpecID != req.SpecID || run.Status != domain.RunRunning ||
		attempt.Status != domain.AttemptRunning ||
		attempt.LeaseGeneration != req.Generation ||
		run.LeaseOwner != req.WorkerID || attempt.WorkerID != req.WorkerID {
		writeError(w, http.StatusConflict, "stale_generation", "candidate registration requires the active Implementer Attempt")
		return
	}
	if existing, gerr := s.db.GetCandidateByAttempt(req.AttemptID); gerr == nil {
		if existing.CommitSHA == strings.TrimSpace(req.CommitSHA) &&
			existing.CandidateRef == strings.TrimSpace(req.CandidateRef) {
			w.Header().Set("Idempotent-Replayed", "true")
			writeJSON(w, http.StatusOK, map[string]any{"candidate": existing})
			return
		}
		writeError(w, http.StatusConflict, "identity_conflict", "a different candidate is already frozen for this Attempt")
		return
	} else if !errors.Is(gerr, domain.ErrNotFound) {
		mapDomainError(w, gerr, s.log, "candidate lookup")
		return
	}
	spec, err := s.db.GetActionSpec(req.SpecID)
	if err != nil {
		mapDomainError(w, err, s.log, "candidate spec")
		return
	}
	if spec.ActionID != run.ActionID || spec.ProjectID != run.ProjectID {
		writeError(w, http.StatusForbidden, "unauthorized", "specification does not authorize this Run")
		return
	}
	validation, err := candidate.InspectCommit(ctx, spec.Repository, spec.BaseRevision,
		strings.TrimSpace(req.CommitSHA), strings.TrimSpace(req.CandidateRef), spec.AllowedPaths)
	if err != nil {
		writeError(w, http.StatusBadRequest, "invalid", "candidate rejected: "+err.Error())
		return
	}
	record := domain.CandidateRevision{
		ProjectID: spec.ProjectID, ActionID: spec.ActionID, SpecID: spec.ID,
		SpecHash: spec.ContentHash, PolicyRevision: spec.PolicyRevision,
		BaseRevision: spec.BaseRevision, CommitSHA: validation.CommitSHA,
		TreeSHA: validation.TreeSHA, ContentDigest: validation.ContentDigest,
		CandidateRef:     strings.TrimSpace(req.CandidateRef),
		Repository:       spec.Repository,
		ImplementerRunID: run.ID, ImplementerAttemptID: attempt.ID,
		ImplementerGeneration: attempt.LeaseGeneration,
		ImplementerWorkerID:   req.WorkerID,
		ExecutionMode:         run.ExecutionMode, Provider: run.Provider,
		Model: run.Model, PaseoMode: run.PaseoMode,
		ChangedPaths: validation.ChangedPaths, RiskClass: validation.RiskClass,
	}
	stored, events, err := s.db.CreateCandidate(ctx, record, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapDomainError(w, err, s.log, "create candidate")
		return
	}
	for _, event := range events {
		s.hub.Publish(event)
	}
	s.log.Info("candidate frozen",
		"candidateId", stored.ID, "commitSha", stored.CommitSHA,
		"runId", stored.ImplementerRunID, "riskClass", string(stored.RiskClass),
		"correlationId", telemetry.CorrelationIDFrom(ctx))
	writeJSON(w, http.StatusCreated, map[string]any{"candidate": stored})
}

// --- Verifier: read-only candidate access and evidence ---

func (s *Server) handleVerifierCandidate(w http.ResponseWriter, r *http.Request) {
	c, err := s.db.GetCandidate(r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "verifier candidate")
		return
	}
	spec, err := s.db.GetActionSpec(c.SpecID)
	if err != nil {
		mapDomainError(w, err, s.log, "verifier spec")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"candidate": c, "spec": spec})
}

type verifierEvidenceRequest struct {
	InstanceID     string               `json:"instanceId"`
	CandidateID    string               `json:"candidateId"`
	CandidateSHA   string               `json:"candidateSha"`
	SpecID         string               `json:"specId"`
	SpecHash       string               `json:"specHash"`
	PolicyRevision string               `json:"policyRevision"`
	OracleDigest   string               `json:"oracleDigest"`
	CleanResource  string               `json:"cleanResource"`
	Checks         []domain.CheckResult `json:"checks"`
	Findings       []string             `json:"findings"`
	IdempotencyKey string               `json:"idempotencyKey"`
}

func (s *Server) handleVerifierEvidence(w http.ResponseWriter, r *http.Request) {
	var req verifierEvidenceRequest
	if !decodeBody(w, r, &req) {
		return
	}
	ctx := r.Context()
	evidence := domain.VerificationEvidence{
		CandidateID: strings.TrimSpace(req.CandidateID), CandidateSHA: strings.TrimSpace(req.CandidateSHA),
		SpecID: strings.TrimSpace(req.SpecID), SpecHash: strings.TrimSpace(req.SpecHash),
		PolicyRevision: strings.TrimSpace(req.PolicyRevision), Authority: "verifier",
		Actor: strings.TrimSpace(req.InstanceID), Independent: true,
		CleanResource: strings.TrimSpace(req.CleanResource), OracleDigest: strings.TrimSpace(req.OracleDigest),
		Checks: req.Checks, Findings: req.Findings, IdempotencyKey: strings.TrimSpace(req.IdempotencyKey),
	}
	stored, replayed, event, err := s.db.RecordVerification(ctx, evidence, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapDomainError(w, err, s.log, "record verification")
		return
	}
	if replayed {
		w.Header().Set("Idempotent-Replayed", "true")
		writeJSON(w, http.StatusOK, map[string]any{"evidence": stored})
		return
	}
	s.hub.Publish(event)
	decision, final, _, gerr := s.evaluateCandidate(ctx, stored.CandidateID, telemetry.CorrelationIDFrom(ctx))
	if gerr != nil {
		s.log.Error("gate evaluation after verification failed", "error", gerr.Error())
	}
	writeJSON(w, http.StatusCreated, map[string]any{
		"evidence": stored, "decision": decision, "final": final,
	})
}

// --- Reviewer: read-only candidate access and verdict ---

func (s *Server) handleReviewerCandidate(w http.ResponseWriter, r *http.Request) {
	c, err := s.db.GetCandidate(r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "reviewer candidate")
		return
	}
	spec, err := s.db.GetActionSpec(c.SpecID)
	if err != nil {
		mapDomainError(w, err, s.log, "reviewer spec")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"candidate": c, "spec": spec})
}

type reviewerVerdictRequest struct {
	InstanceID     string                 `json:"instanceId"`
	CandidateID    string                 `json:"candidateId"`
	CandidateSHA   string                 `json:"candidateSha"`
	SpecID         string                 `json:"specId"`
	SpecHash       string                 `json:"specHash"`
	PolicyRevision string                 `json:"policyRevision"`
	Verdict        string                 `json:"verdict"`
	Findings       []domain.ReviewFinding `json:"findings"`
	DiffStat       string                 `json:"diffStat"`
	DiffBytes      int                    `json:"diffBytes"`
	IdempotencyKey string                 `json:"idempotencyKey"`
}

func (s *Server) handleReviewerVerdict(w http.ResponseWriter, r *http.Request) {
	var req reviewerVerdictRequest
	if !decodeBody(w, r, &req) {
		return
	}
	ctx := r.Context()
	review := domain.ReviewEvidence{
		CandidateID: strings.TrimSpace(req.CandidateID), CandidateSHA: strings.TrimSpace(req.CandidateSHA),
		SpecID: strings.TrimSpace(req.SpecID), SpecHash: strings.TrimSpace(req.SpecHash),
		PolicyRevision: strings.TrimSpace(req.PolicyRevision), Authority: "reviewer",
		Actor: strings.TrimSpace(req.InstanceID), Verdict: domain.ReviewVerdictValues(strings.TrimSpace(req.Verdict)),
		Findings: req.Findings, DiffStat: strings.TrimSpace(req.DiffStat), DiffBytes: req.DiffBytes,
		IdempotencyKey: strings.TrimSpace(req.IdempotencyKey),
	}
	stored, replayed, event, err := s.db.RecordReview(ctx, review, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapDomainError(w, err, s.log, "record review")
		return
	}
	if replayed {
		w.Header().Set("Idempotent-Replayed", "true")
		writeJSON(w, http.StatusOK, map[string]any{"review": stored})
		return
	}
	s.hub.Publish(event)
	decision, final, _, gerr := s.evaluateCandidate(ctx, stored.CandidateID, telemetry.CorrelationIDFrom(ctx))
	if gerr != nil {
		s.log.Error("gate evaluation after review failed", "error", gerr.Error())
	}
	writeJSON(w, http.StatusCreated, map[string]any{
		"review": stored, "decision": decision, "final": final,
	})
}
