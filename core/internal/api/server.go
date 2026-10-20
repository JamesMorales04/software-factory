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
var Version = "0.1.0-s1"

// Server wires persistence, auth and telemetry into one HTTP boundary.
// Operator routes authenticate with the admin token; Worker protocol routes
// authenticate with a separate scoped worker token that grants nothing else.
type Server struct {
	db          *store.DB
	token       string
	workerToken string
	log         *slog.Logger
	tracer      trace.Tracer
	hub         *Hub
	webDir      string
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

// workerRoute reports whether a path is part of the scoped Worker protocol.
// Operator Worker observations (GET /api/v1/workers...) stay on the admin
// token; only the protocol endpoints below accept the worker token.
func workerRoute(path string) bool {
	switch path {
	case "/api/v1/workers/register", "/api/v1/workers/poll", "/api/v1/workers/heartbeat", "/api/v1/workers/report",
		"/api/v1/workers/session/intend", "/api/v1/workers/session/bind",
		"/api/v1/workers/session/close", "/api/v1/workers/session/orphan":
		return true
	default:
		return false
	}
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
	mux.HandleFunc("POST /api/v1/actions", s.handleCreateAction)
	mux.HandleFunc("GET /api/v1/actions", s.handleListActions)
	mux.HandleFunc("GET /api/v1/actions/{id}", s.handleGetAction)
	mux.HandleFunc("POST /api/v1/actions/{id}/transition", s.handleTransitionAction)
	mux.HandleFunc("GET /api/v1/actions/{id}/events", s.handleActionEvents)
	mux.HandleFunc("GET /api/v1/events", s.handleListEvents)
	mux.HandleFunc("GET /api/v1/events/stream", s.handleEventStream)
	mux.HandleFunc("POST /api/v1/runs", s.handleCreateRun)
	mux.HandleFunc("GET /api/v1/runs", s.handleListRuns)
	mux.HandleFunc("GET /api/v1/runs/{id}", s.handleGetRun)
	mux.HandleFunc("GET /api/v1/runs/{id}/attempts", s.handleListAttempts)
	mux.HandleFunc("GET /api/v1/runs/{id}/sessions", s.handleListSessions)
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
		if workerRoute(r.URL.Path) {
			s.withWorkerAuth(next).ServeHTTP(w, r)
			return
		}
		s.withOperatorAuth(next).ServeHTTP(w, r)
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

// withWorkerAuth guards the Core–Worker protocol with the scoped worker
// token. The admin token is never accepted here: a compromised Worker
// credential must not become an operator credential and vice versa. An
// empty worker token fails closed.
func (s *Server) withWorkerAuth(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		want := "Bearer " + s.workerToken
		got := r.Header.Get("Authorization")
		if s.workerToken == "" || subtle.ConstantTimeCompare([]byte(got), []byte(want)) != 1 {
			writeError(w, http.StatusUnauthorized, "unauthorized", "valid worker Bearer token required")
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
	case errors.Is(err, domain.ErrInvalidTransition):
		writeError(w, http.StatusConflict, "conflict", err.Error())
	case errors.Is(err, domain.ErrInvalidInput):
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
	default:
		log.Error(msg, "error", err.Error())
		writeError(w, http.StatusInternalServerError, "internal", "internal error")
	}
}
