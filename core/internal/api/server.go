package api

import (
	"crypto/subtle"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net/http"
	"strings"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/store"
	"github.com/JamesMorales04/software-factory/core/internal/telemetry"
	"go.opentelemetry.io/otel/trace"
)

// Version is the S1 Core version string, overridden at release time.
var Version = "0.1.0-s4c"

// Server wires persistence, auth and telemetry into one HTTP boundary.
// Operator routes authenticate with the admin token; Worker protocol routes
// authenticate with a separate scoped worker token that grants nothing else.
type Server struct {
	db            *store.DB
	token         string
	workerToken   string
	verifierToken string
	reviewerToken string
	log           *slog.Logger
	tracer        trace.Tracer
	hub           *Hub
	webDir        string
	campaigns     CampaignCoordinator
}

// New builds a Server. Token must be non-empty; startup refuses to run
// an unauthenticated Core.
func New(db *store.DB, token string, logger *slog.Logger, tracer trace.Tracer, hub *Hub) *Server {
	return &Server{db: db, token: token, log: logger, tracer: tracer, hub: hub}
}

// WithWorkerToken scopes the Worker protocol to a dedicated credential.
// Without it, Worker routes fail closed with 401.
func (s *Server) WithWorkerToken(token string) *Server {
	s.workerToken = token
	return s
}

// WithVerifierToken scopes the independent verifier evidence protocol.
// Implementers cannot submit verification authority through the Worker
// credential; an empty token fails closed.
func (s *Server) WithVerifierToken(token string) *Server {
	s.verifierToken = token
	return s
}

// WithReviewerToken scopes the read-only reviewer verdict protocol.
func (s *Server) WithReviewerToken(token string) *Server {
	s.reviewerToken = token
	return s
}

// workerRoute reports whether a path is part of the scoped Worker protocol.
// Operator Worker observations (GET /api/v1/workers...) stay on the admin
// token; only the protocol endpoints below accept the worker token.
func workerRoute(path string) bool {
	switch path {
	case "/api/v1/workers/register", "/api/v1/workers/poll", "/api/v1/workers/heartbeat", "/api/v1/workers/report",
		"/api/v1/workers/session/intend", "/api/v1/workers/session/bind",
		"/api/v1/workers/session/close", "/api/v1/workers/session/orphan",
		"/api/v1/workers/recovery/claim", "/api/v1/workers/recovery/complete",
		"/api/v1/workers/candidates":
		return true
	default:
		return strings.HasPrefix(path, "/api/v1/workers/specs/")
	}
}

func verifierRoute(path string) bool {
	return strings.HasPrefix(path, "/api/v1/verifier/")
}

func reviewerRoute(path string) bool {
	return strings.HasPrefix(path, "/api/v1/reviewer/")
}

// WithWebDir serves a static Factory Office build with SPA fallback for
// non-API paths. Empty disables static serving (API-only mode).
func (s *Server) WithWebDir(dir string) *Server {
	s.webDir = dir
	return s
}

// Handler assembles routes and the middleware chain. Only /api/* requires
// the Bearer token: the static Office bundle carries no secrets and must
// be fetchable so the in-app token gate can bootstrap.
func (s *Server) Handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /api/v1/health", s.handleHealth)
	mux.HandleFunc("GET /api/v1/projects", s.handleListProjects)
	mux.HandleFunc("POST /api/v1/projects", s.handleRegisterProject)
	mux.HandleFunc("GET /api/v1/projects/{id}", s.handleGetProject)
	mux.HandleFunc("PATCH /api/v1/projects/{id}", s.handleUpdateProject)
	mux.HandleFunc("GET /api/v1/projects/{id}/refs", s.handleProjectRefs)
	mux.HandleFunc("GET /api/v1/projects/{id}/events", s.handleProjectEvents)
	mux.HandleFunc("GET /api/v1/projects/{id}/config", s.handleProjectConfigRevision)
	mux.HandleFunc("POST /api/v1/actions", s.handleCreateAction)
	mux.HandleFunc("GET /api/v1/actions", s.handleListActions)
	mux.HandleFunc("GET /api/v1/actions/{id}", s.handleGetAction)
	mux.HandleFunc("POST /api/v1/actions/{id}/transition", s.handleTransitionAction)
	mux.HandleFunc("GET /api/v1/actions/{id}/events", s.handleActionEvents)
	mux.HandleFunc("POST /api/v1/campaigns", s.handleCreateCampaign)
	mux.HandleFunc("GET /api/v1/campaigns", s.handleListCampaigns)
	mux.HandleFunc("GET /api/v1/campaigns/{id}", s.handleGetCampaign)
	mux.HandleFunc("GET /api/v1/campaigns/{id}/events", s.handleCampaignEvents)
	mux.HandleFunc("POST /api/v1/campaigns/{id}/start", s.handleStartCampaign)
	mux.HandleFunc("POST /api/v1/campaigns/{id}/cancel", s.handleCancelCampaign)
	mux.HandleFunc("POST /api/v1/action-specs", s.handleCreateActionSpec)
	mux.HandleFunc("GET /api/v1/action-specs/{id}", s.handleGetActionSpec)
	mux.HandleFunc("GET /api/v1/actions/{id}/specs", s.handleListActionSpecs)
	mux.HandleFunc("GET /api/v1/candidates", s.handleListCandidates)
	mux.HandleFunc("GET /api/v1/candidates/{id}", s.handleGetCandidate)
	mux.HandleFunc("GET /api/v1/candidates/{id}/evidence", s.handleCandidateEvidence)
	mux.HandleFunc("POST /api/v1/candidates/{id}/gate", s.handleEvaluateGate)
	mux.HandleFunc("GET /api/v1/runs/{id}/candidate", s.handleRunCandidate)
	mux.HandleFunc("GET /api/v1/events", s.handleListEvents)
	mux.HandleFunc("GET /api/v1/events/stream", s.handleEventStream)
	mux.HandleFunc("POST /api/v1/runs", s.handleCreateRun)
	mux.HandleFunc("GET /api/v1/runs", s.handleListRuns)
	mux.HandleFunc("GET /api/v1/runs/{id}", s.handleGetRun)
	mux.HandleFunc("GET /api/v1/runs/{id}/attempts", s.handleListAttempts)
	mux.HandleFunc("GET /api/v1/runs/{id}/sessions", s.handleListSessions)
	mux.HandleFunc("GET /api/v1/sessions", s.handleListAllSessions)
	mux.HandleFunc("GET /api/v1/sessions/orphaned", s.handleListOrphanedSessions)
	mux.HandleFunc("GET /api/v1/runs/{id}/events", s.handleRunEvents)
	mux.HandleFunc("POST /api/v1/runs/{id}/cancel", s.handleCancelRun)
	mux.HandleFunc("GET /api/v1/workers", s.handleListWorkers)
	mux.HandleFunc("GET /api/v1/workers/{id}", s.handleGetWorker)
	mux.HandleFunc("POST /api/v1/workers/register", s.handleWorkerRegister)
	mux.HandleFunc("POST /api/v1/workers/poll", s.handleWorkerPoll)
	mux.HandleFunc("POST /api/v1/workers/heartbeat", s.handleWorkerHeartbeat)
	mux.HandleFunc("POST /api/v1/workers/report", s.handleWorkerReport)
	mux.HandleFunc("POST /api/v1/workers/session/intend", s.handleSessionIntend)
	mux.HandleFunc("POST /api/v1/workers/session/bind", s.handleSessionBind)
	mux.HandleFunc("POST /api/v1/workers/session/close", s.handleSessionClose)
	mux.HandleFunc("POST /api/v1/workers/session/orphan", s.handleSessionOrphan)
	mux.HandleFunc("POST /api/v1/workers/recovery/claim", s.handleRecoveryClaim)
	mux.HandleFunc("POST /api/v1/workers/recovery/complete", s.handleRecoveryComplete)
	mux.HandleFunc("GET /api/v1/workers/specs/{id}", s.handleWorkerSpec)
	mux.HandleFunc("POST /api/v1/workers/candidates", s.handleWorkerCandidate)
	mux.HandleFunc("GET /api/v1/verifier/candidates/{id}", s.handleVerifierCandidate)
	mux.HandleFunc("POST /api/v1/verifier/evidence", s.handleVerifierEvidence)
	mux.HandleFunc("GET /api/v1/reviewer/candidates/{id}", s.handleReviewerCandidate)
	mux.HandleFunc("POST /api/v1/reviewer/verdict", s.handleReviewerVerdict)

	authed := s.withAuth(mux)
	root := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.HasPrefix(r.URL.Path, "/api/") {
			authed.ServeHTTP(w, r)
			return
		}
		if s.webDir != "" && serveStaticFile(w, r, s.webDir) {
			return
		}
		http.NotFound(w, r)
	})
	return s.withCorrelation(s.withTracing(root))
}

// --- middleware ---

func (s *Server) withAuth(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case workerRoute(r.URL.Path):
			s.withScopedAuth(next, s.workerToken, "worker").ServeHTTP(w, r)
		case verifierRoute(r.URL.Path):
			s.withScopedAuth(next, s.verifierToken, "verifier").ServeHTTP(w, r)
		case reviewerRoute(r.URL.Path):
			s.withScopedAuth(next, s.reviewerToken, "reviewer").ServeHTTP(w, r)
		default:
			s.withOperatorAuth(next).ServeHTTP(w, r)
		}
	})
}

// withOperatorAuth guards operator routes (Actions, Runs, observations)
// with the admin token. The worker token is never accepted here.
func (s *Server) withOperatorAuth(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		want := "Bearer " + s.token
		got := r.Header.Get("Authorization")
		if got == "" && r.URL.Path == "/api/v1/events/stream" {
			// EventSource cannot set headers; the stream endpoint also
			// accepts the token as a query parameter. The value is never
			// logged (see redaction rules and access-log field choice).
			if q := r.URL.Query().Get("access_token"); q != "" {
				got = "Bearer " + q
			}
		}
		if subtle.ConstantTimeCompare([]byte(got), []byte(want)) != 1 {
			writeError(w, http.StatusUnauthorized, "unauthorized", "valid Bearer token required")
			return
		}
		next.ServeHTTP(w, r)
	})
}

// withScopedAuth guards a scoped protocol with its dedicated token. One
// credential class is never accepted on another class's routes, and an
// empty configured token fails closed.
func (s *Server) withScopedAuth(next http.Handler, token, role string) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		want := "Bearer " + token
		got := r.Header.Get("Authorization")
		if token == "" || subtle.ConstantTimeCompare([]byte(got), []byte(want)) != 1 {
			writeError(w, http.StatusUnauthorized, "unauthorized", "valid "+role+" Bearer token required")
			return
		}
		next.ServeHTTP(w, r)
	})
}

func (s *Server) withCorrelation(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		id := r.Header.Get("X-Correlation-ID")
		if id == "" {
			id = telemetry.NewCorrelationID()
		}
		w.Header().Set("X-Correlation-ID", id)
		next.ServeHTTP(w, r.WithContext(telemetry.WithCorrelationID(r.Context(), id)))
	})
}

func (s *Server) withTracing(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// Telemetry is best-effort: span errors are never propagated.
		ctx, span := s.tracer.Start(r.Context(), "factory.http."+r.Method)
		defer span.End()
		next.ServeHTTP(w, r.WithContext(ctx))
	})
}

// --- JSON helpers ---

type apiError struct {
	Error struct {
		Code    string `json:"code"`
		Message string `json:"message"`
	} `json:"error"`
}

func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}

func writeError(w http.ResponseWriter, status int, code, msg string) {
	var e apiError
	e.Error.Code = code
	e.Error.Message = msg
	writeJSON(w, status, e)
}

// decodeBody parses at most 1 MiB of JSON and rejects unknown fields so
// contract drift is caught instead of silently ignored. Exactly one JSON
// value must be present: trailing data after the first value is rejected,
// otherwise `{"title":"x"} {"smuggled":true}` would parse as valid.
func decodeBody(w http.ResponseWriter, r *http.Request, v any) bool {
	r.Body = http.MaxBytesReader(w, r.Body, 1<<20)
	dec := json.NewDecoder(r.Body)
	dec.DisallowUnknownFields()
	if err := dec.Decode(v); err != nil {
		var syntax *json.SyntaxError
		var unmarshalType *json.UnmarshalTypeError
		switch {
		case errors.As(err, &syntax), errors.As(err, &unmarshalType), errors.Is(err, io.ErrUnexpectedEOF):
			writeError(w, http.StatusBadRequest, "invalid", "malformed JSON body")
		case strings.HasPrefix(err.Error(), "json: unknown field"):
			writeError(w, http.StatusBadRequest, "invalid", "unknown field: "+strings.TrimPrefix(err.Error(), "json: unknown field "))
		case errors.Is(err, io.EOF):
			writeError(w, http.StatusBadRequest, "invalid", "empty JSON body")
		default:
			writeError(w, http.StatusBadRequest, "invalid", "unreadable JSON body")
		}
		return false
	}
	var extra any
	if err := dec.Decode(&extra); err != io.EOF {
		writeError(w, http.StatusBadRequest, "invalid", "unexpected trailing data after JSON body")
		return false
	}
	return true
}

func mapDomainError(w http.ResponseWriter, err error, log *slog.Logger, msg string) {
	switch {
	case errors.Is(err, domain.ErrNotFound):
		writeError(w, http.StatusNotFound, "not_found", "resource not found")
	case errors.Is(err, domain.ErrConflict):
		writeError(w, http.StatusConflict, "conflict", err.Error())
	case errors.Is(err, domain.ErrInvalidTransition):
		writeError(w, http.StatusConflict, "conflict", err.Error())
	case errors.Is(err, domain.ErrInvalidInput):
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
	case errors.Is(err, domain.ErrUnauthorized):
		writeError(w, http.StatusForbidden, "forbidden", err.Error())
	default:
		log.Error(msg, "error", err.Error())
		writeError(w, http.StatusInternalServerError, "internal", "internal error")
	}
}
