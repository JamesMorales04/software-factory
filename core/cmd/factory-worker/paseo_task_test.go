package main

import (
	"context"
	"errors"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/paseo"
)

// fakeBackend is a deterministic Core stand-in. It enforces the S3-A
// ordering the Worker must respect: bind requires intend, and a
// succeeded close requires a bound session. Every call is recorded.
type fakeBackend struct {
	mu       sync.Mutex
	calls    []string
	intended bool
	bound    bool
	closed   string
	orphaned bool

	intendErr error
	bindErr   error
	closeErr  error

	binding domain.PaseoBinding
}

func newFakeBackend() *fakeBackend {
	return &fakeBackend{
		binding: domain.PaseoBinding{
			AttemptID: "attempt-1", RunID: "run-1", Seq: 1,
			WorkerID: "local-1", LeaseGeneration: 1,
			Provider: "codex", Model: "gpt-6-luna", Mode: "full-access",
			ExecMode: "trusted-local", Status: domain.PaseoIntended,
		},
	}
}

func (f *fakeBackend) record(call string) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls = append(f.calls, call)
}

func (f *fakeBackend) saw(call string) bool {
	f.mu.Lock()
	defer f.mu.Unlock()
	for _, c := range f.calls {
		if c == call {
			return true
		}
	}
	return false
}

func (f *fakeBackend) intendSession(_ context.Context, _, _, _ string, _ int64) (domain.PaseoBinding, error) {
	f.record("intend")
	if f.intendErr != nil {
		return domain.PaseoBinding{}, f.intendErr
	}
	f.mu.Lock()
	f.intended = true
	b := f.binding
	f.mu.Unlock()
	return b, nil
}

func (f *fakeBackend) bindSession(_ context.Context, _, _, _ string, _ int64, workspaceID, sessionID string) (domain.PaseoBinding, error) {
	f.record("bind")
	if f.bindErr != nil {
		return domain.PaseoBinding{}, f.bindErr
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	if !f.intended {
		return domain.PaseoBinding{}, errors.New("invalid: no intended session for this attempt")
	}
	f.bound = true
	f.binding.WorkspaceID = workspaceID
	f.binding.SessionID = sessionID
	f.binding.Status = domain.PaseoActive
	return f.binding, nil
}

func (f *fakeBackend) closeSession(_ context.Context, _, _, _ string, _ int64, outcome, _ string) (domain.PaseoBinding, error) {
	f.record("close:" + outcome)
	if f.closeErr != nil {
		return domain.PaseoBinding{}, f.closeErr
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	if outcome == string(domain.PaseoSucceeded) && !f.bound {
		return domain.PaseoBinding{}, errors.New("invalid: succeed-without-bind")
	}
	if !f.intended {
		return domain.PaseoBinding{}, errors.New("invalid: no intended session for this attempt")
	}
	f.closed = outcome
	f.binding.Status = domain.PaseoSessionStatus(outcome)
	return f.binding, nil
}

func (f *fakeBackend) orphanSession(_ context.Context, _, _, _ string, _ int64, _ string) (domain.PaseoBinding, error) {
	f.record("orphan")
	f.mu.Lock()
	defer f.mu.Unlock()
	f.orphaned = true
	f.binding.Status = domain.PaseoOrphaned
	return f.binding, nil
}

func testRun(attemptGen int64) (domain.Run, domain.Attempt) {
	run := domain.Run{
		ID: "run-1", ProjectID: "default", WorkloadKind: domain.PaseoTaskKind,
		WorkloadInput: map[string]any{"task": "Append a pure function multiply(a, b) and export it."},
		Status:        domain.RunRunning, MaxAttempts: 3, TimeoutSecs: 600,
		ExecutionMode: string(domain.ExecTrustedLocal),
		Provider:      "codex", Model: "gpt-6-luna", PaseoMode: "full-access",
	}
	attempt := domain.Attempt{
		ID: "attempt-1", RunID: "run-1", Seq: 1,
		WorkerID: "local-1", LeaseGeneration: attemptGen, Status: domain.AttemptRunning,
	}
	return run, attempt
}

func budgetedCtx() (context.Context, context.CancelFunc) {
	return context.WithTimeout(context.Background(), 600*time.Second)
}

func testLogger() *slog.Logger { return slog.New(slog.NewTextHandler(&strings.Builder{}, nil)) }

// simulateAgentEdit appends the multiply function the seeded prompt asks
// for, standing in for the real Paseo session's file modification.
func simulateAgentEdit(t *testing.T, workDir string) {
	t.Helper()
	addJS, err := os.ReadFile(filepath.Join(workDir, "add.js"))
	if err != nil {
		t.Fatal(err)
	}
	edited := string(addJS) + "\n/** Multiply two numbers. */\nfunction multiply(a, b) {\n  return a * b;\n}\nmodule.exports.multiply = multiply;\n"
	if err := os.WriteFile(filepath.Join(workDir, "add.js"), []byte(edited), 0o600); err != nil {
		t.Fatal(err)
	}
}

func TestPaseoLifecycleSuccess(t *testing.T) {
	workDir := t.TempDir()
	if err := seedPaseoFixture(context.Background(), workDir); err != nil {
		t.Fatal(err)
	}
	// Generic hermetic path: no Node harness, diff plus tool activity is
	// the evidence. The npm wiring is covered by runCheck tests and live e2e.
	if err := os.Remove(filepath.Join(workDir, "package.json")); err != nil {
		t.Fatal(err)
	}
	simulateAgentEdit(t, workDir)

	backend := newFakeBackend()
	adapter := paseo.NewFakeAdapter()
	adapter.Session.Cwd = workDir
	adapter.Session.Provider = "codex"
	adapter.Session.Model = "gpt-6-luna"
	adapter.Session.Mode = "full-access"

	run, attempt := testRun(1)
	ctx, cancel := budgetedCtx()
	defer cancel()
	out, err := runPaseoLifecycle(ctx, backend, "local-1", run, attempt, workDir, adapter, testLogger())
	if err != nil {
		t.Fatalf("want success, got %v", err)
	}
	for _, want := range []string{"intend", "bind", "close:succeeded"} {
		if !backend.saw(want) {
			t.Fatalf("backend missing %s: %v", want, backend.calls)
		}
	}
	if backend.saw("orphan") {
		t.Fatal("success must not orphan")
	}
	if out["sessionId"] == "" || out["workspaceId"] == "" {
		t.Fatalf("output missing identity: %v", out)
	}
	if out["provider"] != "codex" || out["model"] != "gpt-6-luna" {
		t.Fatalf("output missing routing: %v", out)
	}
	if out["diffBytes"] == 0 {
		t.Fatalf("output missing diff evidence: %v", out)
	}
	if !adapter.Called("stop") || !adapter.Called("archive") || !adapter.Called("workspace-archive") {
		t.Fatalf("cleanup incomplete: %v", adapter.Calls)
	}
}

func TestPaseoLifecycleNoFalseSuccess(t *testing.T) {
	workDir := t.TempDir()
	if err := seedPaseoFixture(context.Background(), workDir); err != nil {
		t.Fatal(err)
	}
	// No agent edit: empty diff. Timeline still shows tool activity, so
	// exactly one nudge fires and the Attempt still fails honestly.
	backend := newFakeBackend()
	adapter := paseo.NewFakeAdapter()
	adapter.Session.Cwd = workDir

	run, attempt := testRun(1)
	ctx, cancel := budgetedCtx()
	defer cancel()
	_, err := runPaseoLifecycle(ctx, backend, "local-1", run, attempt, workDir, adapter, testLogger())
	if err == nil {
		t.Fatal("empty diff must fail, never succeed")
	}
	if !backend.saw("close:failed") {
		t.Fatalf("failure must close failed: %v", backend.calls)
	}
	if backend.saw("close:succeeded") {
		t.Fatal("failure must never record a succeeded close")
	}
	if len(adapter.Nudges) != 1 {
		t.Fatalf("want exactly one nudge, got %d", len(adapter.Nudges))
	}
}

func TestPaseoLifecycleNoNudgeWithoutToolActivity(t *testing.T) {
	workDir := t.TempDir()
	if err := seedPaseoFixture(context.Background(), workDir); err != nil {
		t.Fatal(err)
	}
	backend := newFakeBackend()
	adapter := paseo.NewFakeAdapter()
	adapter.Session.Cwd = workDir
	adapter.Timeline = paseo.TimelineSummary{Lines: 2, Steps: map[string]int{"Text": 2}}

	run, attempt := testRun(1)
	ctx, cancel := budgetedCtx()
	defer cancel()
	_, err := runPaseoLifecycle(ctx, backend, "local-1", run, attempt, workDir, adapter, testLogger())
	if err == nil {
		t.Fatal("want failure")
	}
	if len(adapter.Nudges) != 0 {
		t.Fatal("no nudge without productive tool activity")
	}
}

func TestPaseoLifecycleStartupFailure(t *testing.T) {
	workDir := t.TempDir()
	backend := newFakeBackend()
	adapter := paseo.NewFakeAdapter()
	adapter.Fail["run"] = paseo.FailOp("run", "daemon unreachable")

	run, attempt := testRun(1)
	ctx, cancel := budgetedCtx()
	defer cancel()
	_, err := runPaseoLifecycle(ctx, backend, "local-1", run, attempt, workDir, adapter, testLogger())
	if err == nil {
		t.Fatal("startup failure must fail")
	}
	if !backend.saw("close:failed") {
		t.Fatalf("startup failure must close failed: %v", backend.calls)
	}
	if backend.saw("bind") || backend.saw("close:succeeded") {
		t.Fatal("failed startup must not bind or succeed")
	}
}

func TestPaseoLifecycleIdentityMismatch(t *testing.T) {
	workDir := t.TempDir()
	if err := seedPaseoFixture(context.Background(), workDir); err != nil {
		t.Fatal(err)
	}
	backend := newFakeBackend()
	adapter := paseo.ScriptMismatch()
	adapter.Session.Cwd = workDir

	run, attempt := testRun(1)
	ctx, cancel := budgetedCtx()
	defer cancel()
	_, err := runPaseoLifecycle(ctx, backend, "local-1", run, attempt, workDir, adapter, testLogger())
	if err == nil {
		t.Fatal("identity mismatch must fail with no substitution")
	}
	if !backend.saw("orphan") {
		t.Fatalf("unproven session identity must be orphaned, never adopted: %v", backend.calls)
	}
	if backend.saw("close:succeeded") {
		t.Fatal("mismatch must never record a succeeded close")
	}
}

func TestPaseoLifecycleStartMismatchOrphans(t *testing.T) {
	workDir := t.TempDir()
	if err := seedPaseoFixture(context.Background(), workDir); err != nil {
		t.Fatal(err)
	}
	backend := newFakeBackend()
	adapter := paseo.ScriptMismatch()
	adapter.Session.Cwd = workDir
	// The real adapter returns the observed identity with a mismatch
	// error; the Worker must orphan the live session, never adopt it.
	adapter.StartErr = paseo.FakeError("run", paseo.KindIdentity, "effective provider mismatch")

	run, attempt := testRun(1)
	ctx, cancel := budgetedCtx()
	defer cancel()
	_, err := runPaseoLifecycle(ctx, backend, "local-1", run, attempt, workDir, adapter, testLogger())
	if err == nil {
		t.Fatal("identity mismatch must fail with no substitution")
	}
	if !backend.saw("orphan") {
		t.Fatalf("started-but-unproven session must be orphaned: %v", backend.calls)
	}
	if backend.saw("bind") || backend.saw("close:succeeded") {
		t.Fatal("mismatched session must never be bound or succeeded")
	}
}

func TestPaseoLifecycleIntendFencingAbandons(t *testing.T) {
	workDir := t.TempDir()
	backend := newFakeBackend()
	backend.intendErr = &fenceError{msg: "stale_generation"}
	adapter := paseo.NewFakeAdapter()

	run, attempt := testRun(1)
	ctx, cancel := budgetedCtx()
	defer cancel()
	_, err := runPaseoLifecycle(ctx, backend, "local-1", run, attempt, workDir, adapter, testLogger())
	var fe *fenceError
	if !errors.As(err, &fe) {
		t.Fatalf("fencing must propagate for abandon, got %v", err)
	}
	if backend.saw("close:failed") || backend.saw("orphan") {
		t.Fatal("fenced intend owns no external state; Core reconciles")
	}
}

func TestPaseoLifecycleBindFencingOrphans(t *testing.T) {
	workDir := t.TempDir()
	if err := seedPaseoFixture(context.Background(), workDir); err != nil {
		t.Fatal(err)
	}
	backend := newFakeBackend()
	backend.bindErr = &fenceError{msg: "stale_generation"}
	adapter := paseo.NewFakeAdapter()
	adapter.Session.Cwd = workDir

	run, attempt := testRun(1)
	ctx, cancel := budgetedCtx()
	defer cancel()
	_, err := runPaseoLifecycle(ctx, backend, "local-1", run, attempt, workDir, adapter, testLogger())
	var fe *fenceError
	if !errors.As(err, &fe) {
		t.Fatalf("bind fencing must propagate for abandon, got %v", err)
	}
	if !backend.saw("orphan") {
		t.Fatalf("unconfirmed external session must be orphaned: %v", backend.calls)
	}
}

func TestPaseoLifecycleObserveFailureClosesFailed(t *testing.T) {
	workDir := t.TempDir()
	if err := seedPaseoFixture(context.Background(), workDir); err != nil {
		t.Fatal(err)
	}
	simulateAgentEdit(t, workDir)
	if err := os.Remove(filepath.Join(workDir, "package.json")); err != nil {
		t.Fatal(err)
	}
	backend := newFakeBackend()
	adapter := paseo.NewFakeAdapter()
	adapter.Session.Cwd = workDir
	adapter.Fail["logs"] = paseo.FailOp("logs", context.Canceled.Error())

	run, attempt := testRun(1)
	ctx, cancel := budgetedCtx()
	defer cancel()
	_, err := runPaseoLifecycle(ctx, backend, "local-1", run, attempt, workDir, adapter, testLogger())
	if err == nil {
		t.Fatal("cancelled observe must fail")
	}
	// The observe error here is a plain failure (scripted), so it closes
	// failed rather than orphaning: only context cancellation orphans.
	if !backend.saw("close:failed") {
		t.Fatalf("want close failed: %v", backend.calls)
	}
}

func TestPaseoLifecycleCancelledStartOrphans(t *testing.T) {
	workDir := t.TempDir()
	backend := newFakeBackend()
	adapter := paseo.NewFakeAdapter()
	adapter.Session.Cwd = workDir

	run, attempt := testRun(1)
	ctx, cancel := budgetedCtx()
	cancel() // operator cancel / shutdown before the session starts
	_, err := runPaseoLifecycle(ctx, backend, "local-1", run, attempt, workDir, adapter, testLogger())
	if err == nil {
		t.Fatal("cancelled start must fail")
	}
	if !backend.saw("orphan") {
		t.Fatalf("cancelled start leaves state unconfirmed; must orphan: %v", backend.calls)
	}
	if backend.saw("close:succeeded") || backend.saw("close:failed") {
		t.Fatal("cancelled start must not claim a terminal session outcome")
	}
}

func TestPaseoLifecycleBudgetGateFailsClosed(t *testing.T) {
	workDir := t.TempDir()
	backend := newFakeBackend()
	adapter := paseo.NewFakeAdapter()

	run, attempt := testRun(1)
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	_, err := runPaseoLifecycle(ctx, backend, "local-1", run, attempt, workDir, adapter, testLogger())
	if err == nil {
		t.Fatal("tiny budget must fail before spending")
	}
	if adapter.Called("run") {
		t.Fatal("budget gate must fire before any session starts")
	}
	if !backend.saw("close:failed") {
		t.Fatalf("budget failure must close failed: %v", backend.calls)
	}
}

func TestPaseoLifecycleSuccessCleanupUnconfirmed(t *testing.T) {
	workDir := t.TempDir()
	if err := seedPaseoFixture(context.Background(), workDir); err != nil {
		t.Fatal(err)
	}
	if err := os.Remove(filepath.Join(workDir, "package.json")); err != nil {
		t.Fatal(err)
	}
	simulateAgentEdit(t, workDir)

	backend := newFakeBackend()
	adapter := paseo.NewFakeAdapter()
	adapter.Session.Cwd = workDir
	// Workspace archive fails after a fully proven session: the result
	// stays truthful, but the output must say cleanup is unconfirmed.
	adapter.Fail["workspace-archive"] = paseo.FailOp("workspace-archive", "daemon busy")
	adapter.Fail["stop"] = paseo.FailOp("stop", "daemon busy")

	run, attempt := testRun(1)
	ctx, cancel := budgetedCtx()
	defer cancel()
	out, err := runPaseoLifecycle(ctx, backend, "local-1", run, attempt, workDir, adapter, testLogger())
	if err != nil {
		t.Fatalf("proven work stays successful: %v", err)
	}
	if !backend.saw("close:succeeded") {
		t.Fatalf("close must stay succeeded: %v", backend.calls)
	}
	note, _ := out["cleanup"].(string)
	if !strings.HasPrefix(note, "unconfirmed:") {
		t.Fatalf("output must flag unconfirmed cleanup, got %q", note)
	}
	if !strings.Contains(note, "stop:") || !strings.Contains(note, "archive-workspace:") {
		t.Fatalf("output must name each unconfirmed step, got %q", note)
	}
	if len(out) > 16 {
		t.Fatalf("result exceeds bound: %d keys", len(out))
	}
}

func TestPaseoLifecycleAbandonCleanupFailureOrphans(t *testing.T) {
	workDir := t.TempDir()
	backend := newFakeBackend()
	adapter := paseo.NewFakeAdapter()
	adapter.Fail["run"] = paseo.FailOp("run", "daemon unreachable")
	adapter.Fail["workspace-archive"] = paseo.FailOp("workspace-archive", "daemon busy")

	run, attempt := testRun(1)
	ctx, cancel := budgetedCtx()
	defer cancel()
	_, err := runPaseoLifecycle(ctx, backend, "local-1", run, attempt, workDir, adapter, testLogger())
	if err == nil {
		t.Fatal("startup failure must fail")
	}
	if !backend.saw("orphan") {
		t.Fatalf("workspace may leak; must stay discoverable via orphan: %v", backend.calls)
	}
	if backend.saw("close:failed") {
		t.Fatal("must not claim a clean failed close when cleanup failed")
	}
}

func TestSeedFixtureIdempotent(t *testing.T) {
	workDir := t.TempDir()
	ctx := context.Background()
	if err := seedPaseoFixture(ctx, workDir); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"package.json", "add.js", "add.test.js", ".git"} {
		if _, err := os.Stat(filepath.Join(workDir, name)); err != nil {
			t.Fatalf("missing seeded %s: %v", name, err)
		}
	}
	marker := filepath.Join(workDir, "custom.txt")
	if err := os.WriteFile(marker, []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := seedPaseoFixture(ctx, workDir); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(marker); err != nil {
		t.Fatal("non-empty workspace must be left untouched")
	}
}

func TestRunCheckPlumbing(t *testing.T) {
	dir := t.TempDir()
	if code, err := runCheck(context.Background(), dir, "true"); err != nil || code != 0 {
		t.Fatalf("true: %d %v", code, err)
	}
	if _, err := runCheck(context.Background(), dir, "false"); err == nil {
		t.Fatal("false must fail")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	if _, err := runCheck(ctx, dir, "sleep", "30"); err == nil {
		t.Fatal("expired check must fail")
	}
}

func TestPromptCarriesTaskAndCwd(t *testing.T) {
	p := paseoTaskPrompt("Do the thing.", "/tmp/wd-1")
	if !strings.Contains(p, "Do the thing.") || !strings.Contains(p, "/tmp/wd-1") {
		t.Fatalf("prompt must carry task and absolute dir: %q", p)
	}
	if !strings.Contains(p, "ONLY") {
		t.Fatalf("prompt must scope edits to the target dir: %q", p)
	}
}

func TestGitDiffEvidenceHonest(t *testing.T) {
	workDir := t.TempDir()
	if err := seedPaseoFixture(context.Background(), workDir); err != nil {
		t.Fatal(err)
	}
	stat, n, _, err := gitDiffEvidence(context.Background(), workDir)
	if err != nil || n != 0 || stat != "" {
		t.Fatalf("clean tree: %q %d %v", stat, n, err)
	}
	simulateAgentEdit(t, workDir)
	stat, n, excerpt, err := gitDiffEvidence(context.Background(), workDir)
	if err != nil || n == 0 {
		t.Fatalf("edited tree must diff: %q %d %v", stat, n, err)
	}
	if !strings.Contains(excerpt, "multiply") {
		t.Fatalf("excerpt must carry the change: %q", excerpt)
	}
}
