package api

import (
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/telemetry"
)

func (s *Server) handleHealth(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]any{
		"status":  "ok",
		"version": Version,
		"time":    time.Now().UTC().Format(time.RFC3339Nano),
	})
}

func (s *Server) handleListProjects(w http.ResponseWriter, r *http.Request) {
	projects, err := s.db.ListProjects()
	if err != nil {
		mapDomainError(w, err, s.log, "list projects")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"projects": projects})
}

type createActionRequest struct {
	ProjectID   string `json:"projectId"`
	Title       string `json:"title"`
	Description string `json:"description"`
}

func (s *Server) handleCreateAction(w http.ResponseWriter, r *http.Request) {
	var req createActionRequest
	if !decodeBody(w, r, &req) {
		return
	}
	params, err := domain.NewActionParams{
		ProjectID:      req.ProjectID,
		Title:          req.Title,
		Description:    req.Description,
		IdempotencyKey: strings.TrimSpace(r.Header.Get("Idempotency-Key")),
	}.Validate()
	if err != nil {
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
		return
	}
	ctx := r.Context()
	action, replayed, err := s.db.CreateAction(ctx, params, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapDomainError(w, err, s.log, "create action")
		return
	}
	if replayed {
		w.Header().Set("Idempotent-Replayed", "true")
		s.log.Info("action create replayed via idempotency key",
			"actionId", action.ID,
			"correlationId", telemetry.CorrelationIDFrom(ctx))
		writeJSON(w, http.StatusOK, map[string]any{"action": action})
		return
	}
	events, err := s.db.ListAggregateEvents(domain.AggregateAction, action.ID)
	if err != nil {
		mapDomainError(w, err, s.log, "read created action events")
		return
	}
	for _, e := range events {
		s.hub.Publish(e)
	}
	s.log.Info("action created",
		"actionId", action.ID,
		"projectId", action.ProjectID,
		"correlationId", telemetry.CorrelationIDFrom(ctx))
	writeJSON(w, http.StatusCreated, map[string]any{"action": action})
}

func (s *Server) handleListActions(w http.ResponseWriter, r *http.Request) {
	q := r.URL.Query()
	var status domain.ActionStatus
	if raw := q.Get("status"); raw != "" {
		var err error
		status, err = domain.ParseStatus(raw)
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
	actions, err := s.db.ListActions(q.Get("projectId"), status, limit)
	if err != nil {
		mapDomainError(w, err, s.log, "list actions")
		return
	}
	if actions == nil {
		actions = []domain.Action{}
	}
	writeJSON(w, http.StatusOK, map[string]any{"actions": actions})
}

func (s *Server) handleGetAction(w http.ResponseWriter, r *http.Request) {
	action, err := s.db.GetAction(r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "get action")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"action": action})
}

type transitionRequest struct {
	To string `json:"to"`
}

func (s *Server) handleTransitionAction(w http.ResponseWriter, r *http.Request) {
	var req transitionRequest
	if !decodeBody(w, r, &req) {
		return
	}
	to, err := domain.ParseStatus(req.To)
	if err != nil {
		writeError(w, http.StatusBadRequest, "invalid", err.Error())
		return
	}
	ctx := r.Context()
	action, event, err := s.db.TransitionAction(ctx, r.PathValue("id"), to, telemetry.CorrelationIDFrom(ctx))
	if err != nil {
		mapDomainError(w, err, s.log, "transition action")
		return
	}
	s.hub.Publish(event)
	s.log.Info("action transitioned",
		"actionId", action.ID,
		"to", string(to),
		"correlationId", telemetry.CorrelationIDFrom(ctx))
	writeJSON(w, http.StatusOK, map[string]any{"action": action})
}

func (s *Server) handleActionEvents(w http.ResponseWriter, r *http.Request) {
	if _, err := s.db.GetAction(r.PathValue("id")); err != nil {
		mapDomainError(w, err, s.log, "action events lookup")
		return
	}
	events, err := s.db.ListAggregateEvents(domain.AggregateAction, r.PathValue("id"))
	if err != nil {
		mapDomainError(w, err, s.log, "list action events")
		return
	}
	if events == nil {
		events = []domain.Event{}
	}
	writeJSON(w, http.StatusOK, map[string]any{"events": projectEvents(events)})
}

func (s *Server) handleListEvents(w http.ResponseWriter, r *http.Request) {
	after, limit, ok := parseReplayParams(w, r)
	if !ok {
		return
	}
	events, err := s.db.ListEvents(after, limit)
	if err != nil {
		mapDomainError(w, err, s.log, "list events")
		return
	}
	if events == nil {
		events = []domain.Event{}
	}
	nextAfter := after
	for _, e := range events {
		if e.GlobalSeq > nextAfter {
			nextAfter = e.GlobalSeq
		}
	}
	writeJSON(w, http.StatusOK, map[string]any{"events": projectEvents(events), "nextAfter": nextAfter})
}

func parseReplayParams(w http.ResponseWriter, r *http.Request) (int64, int, bool) {
	q := r.URL.Query()
	var after int64
	if raw := q.Get("after"); raw != "" {
		n, err := strconv.ParseInt(raw, 10, 64)
		if err != nil || n < 0 {
			writeError(w, http.StatusBadRequest, "invalid", "after must be a non-negative integer")
			return 0, 0, false
		}
		after = n
	}
	limit := 100
	if raw := q.Get("limit"); raw != "" {
		n, err := strconv.Atoi(raw)
		if err != nil || n <= 0 {
			writeError(w, http.StatusBadRequest, "invalid", "limit must be a positive integer")
			return 0, 0, false
		}
		limit = n
	}
	return after, limit, true
}

// handleEventStream implements the S1 replay transport. The durable ledger
// is the source of truth; the hub subscription is only a wake-up signal.
// This closes two races by construction:
//
//  1. Subscribe-before-replay: events committed between subscription and
//     the backlog read are observed twice (once via the channel wake-up,
//     once via the ledger) and deduplicated by the lastSent cursor —
//     never missed.
//  2. Dropped wake-ups: if the hub drops a notification for a slow
//     consumer, the periodic ledger poll still advances the cursor, so a
//     gap can lag at most one poll interval and is always recovered.
//
// The backlog is paged, so there is no cap on missed events. Reconnecting
// with the last seen globalSeq yields exactly the missed suffix.
func (s *Server) handleEventStream(w http.ResponseWriter, r *http.Request) {
	after, _, ok := parseReplayParams(w, r)
	if !ok {
		return
	}
	flusher, ok := w.(http.Flusher)
	if !ok {
		writeError(w, http.StatusInternalServerError, "internal", "streaming unsupported")
		return
	}
	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("Connection", "keep-alive")

	sub, unsub := s.hub.Subscribe()
	defer unsub()

	w.WriteHeader(http.StatusOK)
	// Make the streaming response visible to fetch() before the first event;
	// otherwise empty ledgers look disconnected until the first heartbeat.
	flusher.Flush()

	lastSent := after
	drain := func() bool {
		for {
			page, err := s.db.ListEvents(lastSent, 500)
			if err != nil {
				s.log.Error("stream replay failed", "error", err.Error())
				return false
			}
			if len(page) == 0 {
				return true
			}
			for _, e := range page {
				if !writeSSE(w, flusher, e) {
					return false
				}
				lastSent = e.GlobalSeq
			}
			if len(page) < 500 {
				return true
			}
		}
	}
	if !drain() {
		return
	}

	ticker := time.NewTicker(1 * time.Second)
	defer ticker.Stop()
	heartbeats := 0
	for {
		select {
		case <-r.Context().Done():
			return
		case <-sub:
			if !drain() {
				return
			}
		case <-ticker.C:
			if !drain() {
				return
			}
			heartbeats++
			if heartbeats >= 20 {
				heartbeats = 0
				if _, err := fmt.Fprintf(w, ": ping\n\n"); err != nil {
					return
				}
				flusher.Flush()
			}
		}
	}
}

func writeSSE(w http.ResponseWriter, flusher http.Flusher, e domain.Event) bool {
	payload, err := marshalEvent(e)
	if err != nil {
		return true
	}
	if _, err := fmt.Fprintf(w, "event: factory\ndata: %s\n\n", payload); err != nil {
		return false
	}
	flusher.Flush()
	return true
}
