package main

import (
	"context"
	"log/slog"
	"net/http/httptest"
	"path/filepath"
	"testing"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/api"
	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/store"
	"github.com/JamesMorales04/software-factory/core/internal/telemetry"
)

// TestHandleAttemptCompletesPromptly is a regression test for the
// deferred-cancel deadlock: the heartbeat loop must stop when execution
// finishes, so handleAttempt returns (and reports) promptly instead of
// waiting out the full execution timeout.
func TestHandleAttemptCompletesPromptly(t *testing.T) {
	ctx := context.Background()
	db, err := store.Open(filepath.Join(t.TempDir(), "worker-reg.db"))
	if err != nil {
		t.Fatalf("open: %v", err)
	}
	defer func() { _ = db.Close() }()
	logger := slog.New(slog.DiscardHandler)
	tracer, shutdown := telemetry.InitTracing(ctx, "test")
	defer shutdown(context.Background())
	hub := api.NewHub()
	srv := api.New(db, "op-token", logger, tracer, hub).WithWorkerToken("w-token")
	httpSrv := httptest.NewServer(srv.Handler())
	defer httpSrv.Close()

	c := &client{base: httpSrv.URL, token: "w-token", http: httpSrv.Client(), logger: logger}

	if _, _, err := db.RegisterWorker(ctx, "reg-1", "test", "corr"); err != nil {
		t.Fatalf("register: %v", err)
	}
	params, err := domain.NewRunParams{
		WorkloadKind:  domain.SyntheticEchoKind,
		WorkloadInput: map[string]any{"message": "reg", "steps": 1, "stepMs": 1},
		TimeoutSecs:   30,
	}.Validate()
	if err != nil {
		t.Fatalf("validate: %v", err)
	}
	run, _, err := db.CreateRun(ctx, params, "corr")
	if err != nil {
		t.Fatalf("create: %v", err)
	}
	claimed, attempt, found, _, err := db.ClaimNextRun(ctx, "reg-1", "corr")
	if err != nil || !found {
		t.Fatalf("claim: found=%v err=%v", found, err)
	}

	done := make(chan struct{})
	go func() {
		defer close(done)
		c.handleAttempt(ctx, "reg-1", claimed, attempt, 100*time.Millisecond, false)
	}()
	select {
	case <-done:
	case <-time.After(20 * time.Second):
		t.Fatal("handleAttempt did not return within 20s (heartbeat-join deadlock?)")
	}

	final, err := db.GetRun(run.ID)
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	if final.Status != domain.RunSucceeded {
		t.Fatalf("run status = %s, want succeeded", final.Status)
	}
}
