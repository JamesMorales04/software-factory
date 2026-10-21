package main

import (
	"bytes"
	"context"
	"fmt"
	"log/slog"
	"net/http/httptest"
	"path/filepath"
	"strings"
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

	if _, _, err := db.RegisterWorker(ctx, "reg-1", "test", domain.AllCapabilities, []string{"restricted", "trusted-local"}, "corr"); err != nil {
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
		c.handleAttempt(ctx, "reg-1", claimed, attempt, 100*time.Millisecond, false, t.TempDir())
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

func TestResolveExecModeLocalIsExplicit(t *testing.T) {
	logger := slog.New(slog.DiscardHandler)
	podmanOK, err := resolveExecMode(context.Background(), "local", logger)
	if err != nil || podmanOK {
		t.Fatalf("local mode: podmanOK=%v err=%v", podmanOK, err)
	}
}

func TestResolveExecModePodmanFailsClosedWithoutPodman(t *testing.T) {
	logger := slog.New(slog.DiscardHandler)
	// An empty PATH hides the podman CLI however it is installed.
	t.Setenv("PATH", t.TempDir())
	podmanOK, err := resolveExecMode(context.Background(), "podman", logger)
	if err == nil || podmanOK {
		t.Fatalf("podman mode without podman: podmanOK=%v err=%v", podmanOK, err)
	}
}

func TestResolveExecModeRejectsUnknown(t *testing.T) {
	logger := slog.New(slog.DiscardHandler)
	if _, err := resolveExecMode(context.Background(), "auto", logger); err == nil {
		t.Fatal("expected unknown exec-mode to be rejected")
	}
}

// TestRunRejectsUnknownExecMode proves the --exec-mode flag itself never
// silently downgrades: an unknown value fails at argument parsing, before
// any execution boundary is probed.
func TestRunRejectsUnknownExecMode(t *testing.T) {
	t.Setenv("FACTORY_WORKER_TOKEN", "test-token")
	if err := run([]string{"--exec-mode", "bogus-trusted"}); err == nil {
		t.Fatal("expected run() to reject unknown --exec-mode")
	} else if !strings.Contains(err.Error(), "exec-mode") {
		t.Fatalf("unknown exec-mode error must mention exec-mode, got: %v", err)
	}
}

// TestResolveExecModeLocalLogsExplicitMode proves the effective mode is
// reported on the worker startup path: explicit local logs its identity
// (synthetic fixture for tests/dev, explicitly NOT a Paseo TRUSTED_LOCAL
// path). The full distinction phrase is asserted so a regression that
// drops the warning fails the test.
func TestResolveExecModeLocalLogsExplicitMode(t *testing.T) {
	var buf bytes.Buffer
	logger := slog.New(slog.NewJSONHandler(&buf, nil))
	podmanOK, err := resolveExecMode(context.Background(), "local", logger)
	if err != nil || podmanOK {
		t.Fatalf("local mode: podmanOK=%v err=%v", podmanOK, err)
	}
	out := buf.String()
	for _, want := range []string{"local mode", "synthetic", "NOT a Paseo TRUSTED_LOCAL path"} {
		if !strings.Contains(out, want) {
			t.Fatalf("startup log must report %q, got: %q", want, out)
		}
	}
}

// TestHandleAttemptReportsEffectiveModeAndReceipt proves the attempt result
// carries both the effective execution mode and the receipt binding the
// (run, attempt, generation) that produced it.
func TestHandleAttemptReportsEffectiveModeAndReceipt(t *testing.T) {
	ctx := context.Background()
	db, err := store.Open(filepath.Join(t.TempDir(), "worker-mode.db"))
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

	if _, _, err := db.RegisterWorker(ctx, "reg-mode", "test", domain.AllCapabilities, []string{"restricted", "trusted-local"}, "corr"); err != nil {
		t.Fatalf("register: %v", err)
	}
	params, err := domain.NewRunParams{
		WorkloadKind:  domain.SyntheticEchoKind,
		WorkloadInput: map[string]any{"message": "mode-proof", "steps": 1, "stepMs": 1},
		TimeoutSecs:   30,
	}.Validate()
	if err != nil {
		t.Fatalf("validate: %v", err)
	}
	run, _, err := db.CreateRun(ctx, params, "corr")
	if err != nil {
		t.Fatalf("create: %v", err)
	}
	claimed, attempt, found, _, err := db.ClaimNextRun(ctx, "reg-mode", "corr")
	if err != nil || !found {
		t.Fatalf("claim: found=%v err=%v", found, err)
	}

	// podmanOK=false selects the explicit local fixture path.
	c.handleAttempt(ctx, "reg-mode", claimed, attempt, 100*time.Millisecond, false, t.TempDir())

	final, err := db.GetRun(run.ID)
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	if final.Status != domain.RunSucceeded {
		t.Fatalf("run status = %s, want succeeded", final.Status)
	}
	wantReceipt := fmt.Sprintf("%s/%s/%d", claimed.ID, attempt.ID, attempt.LeaseGeneration)
	if got := final.Result["mode"]; got != "local" {
		t.Fatalf("run result mode = %v, want local (full result: %v)", got, final.Result)
	}
	if got := final.Result["receipt"]; got != wantReceipt {
		t.Fatalf("run result receipt = %v, want %q (full result: %v)", got, wantReceipt, final.Result)
	}
	attempts, err := db.ListAttempts(run.ID)
	if err != nil {
		t.Fatalf("list attempts: %v", err)
	}
	if len(attempts) != 1 {
		t.Fatalf("attempts = %d, want 1", len(attempts))
	}
	if got := attempts[0].Result["mode"]; got != "local" {
		t.Fatalf("attempt result mode = %v, want local (full result: %v)", got, attempts[0].Result)
	}
	if got := attempts[0].Result["receipt"]; got != wantReceipt {
		t.Fatalf("attempt result receipt = %v, want %q", got, wantReceipt)
	}
}
