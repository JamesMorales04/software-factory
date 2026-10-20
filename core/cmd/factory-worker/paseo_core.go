package main

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

// Core session protocol client. Every method maps the S3-A fencing codes
// (404 not_found, 409 stale_generation/lease_expired/run_cancelled/
// not_started) to fenceError so the lifecycle abandons the Attempt without
// reporting; 400 invalid and transport failures stay plain errors for the
// honest failed path. Duplicate deliveries replay server-side.

func (c *client) sessionCall(ctx context.Context, route string, payload map[string]any) (domain.PaseoBinding, error) {
	status, raw, _, err := c.do(ctx, http.MethodPost, "/api/v1/workers/session/"+route, payload)
	if err != nil {
		return domain.PaseoBinding{}, err
	}
	code := errorCode(raw)
	switch status {
	case http.StatusOK, http.StatusCreated:
		var out struct {
			Session domain.PaseoBinding `json:"session"`
		}
		if jerr := json.Unmarshal(raw, &out); jerr != nil {
			return domain.PaseoBinding{}, fmt.Errorf("session %s: bad binding payload", route)
		}
		return out.Session, nil
	case http.StatusNotFound, http.StatusConflict:
		return domain.PaseoBinding{}, &fenceError{msg: fmt.Sprintf("session %s fenced (%s)", route, code)}
	default:
		return domain.PaseoBinding{}, fmt.Errorf("session %s status %d (%s): %s", route, status, code, truncate(string(raw)))
	}
}

func (c *client) intendSession(ctx context.Context, workerID, runID, attemptID string, generation int64) (domain.PaseoBinding, error) {
	return c.sessionCall(ctx, "intend", map[string]any{
		"workerId": workerID, "runId": runID, "attemptId": attemptID, "generation": generation,
	})
}

func (c *client) bindSession(ctx context.Context, workerID, runID, attemptID string, generation int64, workspaceID, sessionID string) (domain.PaseoBinding, error) {
	return c.sessionCall(ctx, "bind", map[string]any{
		"workerId": workerID, "runId": runID, "attemptId": attemptID, "generation": generation,
		"workspaceId": workspaceID, "sessionId": sessionID,
	})
}

func (c *client) closeSession(ctx context.Context, workerID, runID, attemptID string, generation int64, outcome, errMsg string) (domain.PaseoBinding, error) {
	return c.sessionCall(ctx, "close", map[string]any{
		"workerId": workerID, "runId": runID, "attemptId": attemptID, "generation": generation,
		"outcome": outcome, "error": errMsg,
	})
}

func (c *client) orphanSession(ctx context.Context, workerID, runID, attemptID string, generation int64, reason string) (domain.PaseoBinding, error) {
	return c.sessionCall(ctx, "orphan", map[string]any{
		"workerId": workerID, "runId": runID, "attemptId": attemptID, "generation": generation,
		"reason": reason,
	})
}
