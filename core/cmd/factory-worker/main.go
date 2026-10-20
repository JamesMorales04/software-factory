// Command factory-worker runs the S2 local Worker: it registers with the
// Factory Core, polls for leased Runs, executes exactly one fixed
// credential-free synthetic fixture per Attempt, renews its lease with
// heartbeats and reports bounded results. It never executes arbitrary
// commands, never touches provider credentials and never self-authorizes
// merges or policy changes.
package main

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"strings"
	"sync"
	"syscall"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/exec"
)

const workerVersion = "0.5.0-s3d"

type client struct {
	base   string
	token  string
	http   *http.Client
	logger *slog.Logger
}

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, "factory-worker: "+err.Error())
		os.Exit(1)
	}
}

func run(args []string) error {
	flags := flag.NewFlagSet("factory-worker", flag.ContinueOnError)
	coreAddr := flags.String("core", "http://127.0.0.1:8471", "Factory Core base URL")
	workerID := flags.String("worker-id", "local-1", "Worker identity registered with Core")
	pollInterval := flags.Duration("poll-interval", time.Second, "delay between empty polls")
	heartbeatInterval := flags.Duration("heartbeat-interval", 2*time.Second, "lease renewal period")
	maxConcurrent := flags.Int("max-concurrent", 1, "simultaneous Attempts (bounded)")
	execMode := flags.String("exec-mode", "podman", "fixture execution: podman (fail-closed default) or local (explicit pure-Go synthetic fixture for tests/dev only, NOT a Paseo TRUSTED_LOCAL path)")
	recoveryInterval := flags.Duration("recovery-interval", 10*time.Second, "interval between bounded Paseo orphan-recovery claims (0 disables)")
	recoveryOnly := flags.Bool("recovery-only", false, "run only the bounded Paseo recovery coordinator (no Workload polling)")
	showVersion := flags.Bool("version", false, "print worker version and exit")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if *showVersion {
		fmt.Println(workerVersion)
		return nil
	}
	if err := domain.ValidateWorkerID(*workerID); err != nil {
		return err
	}
	token := strings.TrimSpace(os.Getenv("FACTORY_WORKER_TOKEN"))
	if token == "" {
		return errors.New("FACTORY_WORKER_TOKEN is required (scoped worker credential; never reuse the operator token)")
	}
	if *maxConcurrent < 1 || *maxConcurrent > 4 {
		return errors.New("max-concurrent must be 1..4")
	}
	if *recoveryInterval < 0 || *recoveryInterval > 10*time.Minute {
		return errors.New("recovery-interval must be 0..10m")
	}
	if *recoveryOnly && *recoveryInterval <= 0 {
		return errors.New("recovery-only requires a positive recovery-interval")
	}
	switch *execMode {
	case "podman", "local":
	default:
		return errors.New(`exec-mode must be "podman" or "local"`)
	}

	logger := slog.New(slog.NewJSONHandler(os.Stderr, nil))
	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()

	c := &client{
		base:   strings.TrimRight(*coreAddr, "/"),
		token:  token,
		http:   &http.Client{Timeout: 15 * time.Second},
		logger: logger,
	}
	podmanOK, err := resolveExecMode(ctx, *execMode, logger)
	if err != nil {
		return err
	}

	// Orphan cleanup: a previous Worker killed with SIGKILL bypasses
	// in-process workdir removal; sweep only our own run-root pattern.
	if reaped := exec.ReapStaleRunRoots(); reaped > 0 {
		logger.Info("reaped stale fixture workspaces", "count", reaped)
	}
	runsRoot, err := exec.RunsRoot()
	if err != nil {
		return fmt.Errorf("create fixture runs root: %w", err)
	}
	// Shutdown cleanup: the graceful path closes the run root after all
	// slots exit (below). On the shutdown-timeout path the root is
	// deliberately left locked: slots may still be live, and the kernel
	// releases the lock on process exit so the next Worker reaps it as
	// stale. Never remove a root that may still be in use.

	if err := c.register(ctx, *workerID); err != nil {
		_ = runsRoot.Close()
		return fmt.Errorf("register worker: %w", err)
	}
	logger.Info("worker registered", "workerId", *workerID, "mode", map[bool]string{true: "podman", false: "local"}[podmanOK])

	// S3-D recovery coordinator: one bounded claim at a time, independent
	// of execution slots. A dedicated recovery-only process is useful when
	// an execution Worker was killed and must not be restarted as an
	// executor merely to clean up after itself.
	if *recoveryOnly {
		go c.recoveryLoop(ctx, *workerID, *recoveryInterval, logger)
		<-ctx.Done()
		_ = runsRoot.Close()
		return nil
	}
	if *recoveryInterval > 0 {
		go c.recoveryLoop(ctx, *workerID, *recoveryInterval, logger)
		logger.Info("paseo recovery loop enabled", "interval", recoveryInterval.String())
	}

	var wg sync.WaitGroup
	for i := 0; i < *maxConcurrent; i++ {
		wg.Add(1)
		go func(slot int) {
			defer wg.Done()
			c.slotLoop(ctx, slot, *workerID, *pollInterval, *heartbeatInterval, podmanOK, runsRoot.Dir())
		}(i)
	}
	<-ctx.Done()
	logger.Info("worker shutting down")
	done := make(chan struct{})
	go func() { wg.Wait(); close(done) }()
	select {
	case <-done:
		// All slots exited: no live user of the root remains.
		if cerr := runsRoot.Close(); cerr != nil {
			logger.Warn("runs root cleanup failed", "error", cerr.Error())
		}
		return nil
	case <-time.After(15 * time.Second):
		// Slots may still be live: leave the root locked for stale
		// reaping instead of removing a directory in use.
		logger.Warn("worker shutdown timed out waiting for slots; leaving run root locked for stale reaping")
		return errors.New("worker shutdown timed out waiting for slots")
	}
}

// resolveExecMode probes the execution boundary once at startup and fails
// closed: podman mode without a working Podman or fixture image refuses to
// start rather than silently downgrading. The explicit local mode (pure-Go,
// no subprocess, no container) is a synthetic fixture for tests and
// development only — NOT a Paseo TRUSTED_LOCAL path; it is never selected
// implicitly.
func resolveExecMode(ctx context.Context, mode string, logger *slog.Logger) (bool, error) {
	probe, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	available := exec.PodmanAvailable(probe)
	switch mode {
	case "podman":
		if !available {
			return false, errors.New("exec-mode=podman but the podman CLI is unavailable")
		}
		if err := exec.PullImage(probe); err != nil {
			return false, fmt.Errorf("exec-mode=podman but fixture image unavailable: %w", err)
		}
		return true, nil
	case "local":
		logger.Info("fixture execution using explicit local mode (synthetic pure-Go fixture for tests/dev only, no subprocess, no container; NOT a Paseo TRUSTED_LOCAL path)")
		return false, nil
	default:
		return false, fmt.Errorf("unknown exec-mode %q", mode)
	}
}

// slotLoop polls for work and handles one Attempt at a time.
func (c *client) slotLoop(ctx context.Context, slot int, workerID string, poll, heartbeat time.Duration, podmanOK bool, runsRoot string) {
	for {
		if ctx.Err() != nil {
			return
		}
		run, attempt, status, err := c.poll(ctx, workerID)
		if err != nil {
			if errors.Is(err, errWorkerUnknown) {
				if rerr := c.register(ctx, workerID); rerr != nil {
					c.logger.Error("re-register failed", "slot", slot, "error", rerr.Error())
					if !sleepCtx(ctx, poll) {
						return
					}
					continue
				}
				continue
			}
			c.logger.Error("poll failed", "slot", slot, "error", err.Error())
			if !sleepCtx(ctx, poll) {
				return
			}
			continue
		}
		if status == http.StatusNoContent {
			if !sleepCtx(ctx, poll) {
				return
			}
			continue
		}
		c.handleAttempt(ctx, workerID, run, attempt, heartbeat, podmanOK, runsRoot)
	}
}

// errWorkerUnknown signals a 404 for a known-shaped identity: the Core
// does not know this Worker (fresh DB, wiped state) so re-registration is
// the correct recovery, not an error loop.
var errWorkerUnknown = errors.New("worker unknown")

func (c *client) handleAttempt(ctx context.Context, workerID string, run domain.Run, attempt domain.Attempt, heartbeat time.Duration, podmanOK bool, runsRoot string) {
	log := c.logger.With("workerId", workerID, "runId", run.ID, "attemptId", attempt.ID, "generation", attempt.LeaseGeneration)
	execTimeout := runTimeout(run.TimeoutSecs)
	execCtx, cancel := context.WithTimeout(ctx, execTimeout)
	defer cancel()

	// Lease heartbeat loop: a fencing rejection is authoritative — abandon
	// the Attempt immediately (cancelled, expired or superseded) and never
	// report for it afterwards.
	hbDone := make(chan struct{})
	fenced := make(chan error, 1)
	go func() {
		defer close(hbDone)
		ticker := time.NewTicker(heartbeat)
		defer ticker.Stop()
		for {
			select {
			case <-execCtx.Done():
				return
			case <-ticker.C:
				if herr := c.heartbeat(execCtx, workerID, run.ID, attempt.ID, attempt.LeaseGeneration); herr != nil {
					select {
					case fenced <- herr:
					default:
					}
					cancel()
					return
				}
			}
		}
	}()

	spec, err := fixtureSpec(run, attempt)
	if err != nil && run.WorkloadKind != domain.PaseoTaskKind {
		cancel() // stop the heartbeat loop before waiting for it
		<-hbDone
		c.bestEffortReport(ctx, workerID, run, attempt, domain.OutcomeFailed, nil, "invalid-workload: "+err.Error(), log)
		return
	}
	workDir, err := os.MkdirTemp(runsRoot, "attempt-*")
	if err != nil {
		cancel() // stop the heartbeat loop before waiting for it
		<-hbDone
		c.bestEffortReport(ctx, workerID, run, attempt, domain.OutcomeFailed, nil, "workspace: "+err.Error(), log)
		return
	}
	// Paseo-task workspaces are retained on reported failure so the actual
	// result stays inspectable; synthetic fixtures are always removed.
	retainWorkDir := false
	defer func() {
		if !retainWorkDir {
			_ = os.RemoveAll(workDir)
		}
	}()

	// Synchronous start heartbeat: records execution start (leased→
	// running, RunStarted) before the first renewal interval elapses, so
	// even fast fixtures leave a complete lineage. A fencing rejection
	// here is authoritative — abandon without executing or reporting.
	if herr := c.heartbeat(execCtx, workerID, run.ID, attempt.ID, attempt.LeaseGeneration); herr != nil {
		select {
		case fenced <- herr:
		default:
		}
		cancel() // stop the heartbeat loop before waiting for it
		<-hbDone
		log.Info("attempt fenced before execution; abandoning without report", "error", herr.Error())
		return
	}

	result, err := c.runAttemptWorkload(execCtx, workerID, run, attempt, spec, workDir, podmanOK, log)
	cancel() // execution finished: stop the heartbeat loop before joining it
	<-hbDone
	select {
	case ferr := <-fenced:
		// The lease is gone: Core owns the outcome now (expiry, retry or
		// cancel). Reporting would be rejected; abandon instead.
		log.Info("attempt fenced during execution; abandoning without report", "error", ferr.Error())
		return
	default:
	}
	var fenceErr *fenceError
	if errors.As(err, &fenceErr) {
		// Core fenced the session lifecycle itself (stale generation,
		// expiry, cancel): abandon without reporting, same as a lease
		// fence. Core-side expiry/restart reconciliation owns the orphan.
		log.Info("attempt session fenced; abandoning without report", "error", fenceErr.Error())
		return
	}
	if err != nil {
		reason := "execution-failed"
		if errors.Is(err, context.DeadlineExceeded) {
			reason = "timeout"
		} else if errors.Is(err, context.Canceled) && ctx.Err() == nil {
			// Attempt context expired but the Worker itself is alive:
			// our own deadline. Anything else is a shutdown.
			reason = "timeout"
		} else if ctx.Err() != nil {
			reason = "worker-shutdown"
		}
		if run.WorkloadKind == domain.PaseoTaskKind {
			// A reported paseo-task failure keeps its workspace for
			// inspection; the reason already names the failed stage.
			retainWorkDir = true
			log.Info("retaining paseo-task workspace for inspection", "workDir", workDir)
			c.bestEffortReport(ctx, workerID, run, attempt, domain.OutcomeFailed, result, truncate(err.Error()), log)
			return
		}
		c.bestEffortReport(ctx, workerID, run, attempt, domain.OutcomeFailed, nil, reason+": "+truncate(err.Error()), log)
		return
	}
	output := result
	c.bestEffortReport(ctx, workerID, run, attempt, domain.OutcomeCompleted, output, "", log)
}

// bestEffortReport delivers the terminal observation unless the Worker is
// shutting down (short budget) or Core already fenced the Attempt — both
// are handled by Core-side expiry, never by inventing success.
func (c *client) bestEffortReport(ctx context.Context, workerID string, run domain.Run, attempt domain.Attempt, outcome domain.ReportOutcome, output map[string]any, errMsg string, log *slog.Logger) {
	rctx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	if rerr := c.report(rctx, workerID, run.ID, attempt.ID, attempt.LeaseGeneration, outcome, output, errMsg); rerr != nil {
		log.Error("report failed; Core expiry owns recovery", "outcome", string(outcome), "error", rerr.Error())
		return
	}
	log.Info("report accepted", "outcome", string(outcome))
}

func runTimeout(timeoutSecs int) time.Duration {
	if timeoutSecs > 3 {
		return time.Duration(timeoutSecs-2) * time.Second
	}
	if timeoutSecs > 1 {
		return time.Duration(timeoutSecs-1) * time.Second
	}
	return time.Second
}

func fixtureSpec(run domain.Run, attempt domain.Attempt) (exec.Spec, error) {
	if run.WorkloadKind != domain.SyntheticEchoKind {
		return exec.Spec{}, fmt.Errorf("unsupported workloadKind %q", run.WorkloadKind)
	}
	msg, _ := run.WorkloadInput["message"].(string)
	steps, err := specInt(run.WorkloadInput["steps"])
	if err != nil {
		return exec.Spec{}, err
	}
	stepMs, err := specInt(run.WorkloadInput["stepMs"])
	if err != nil {
		return exec.Spec{}, err
	}
	return exec.Spec{
		Message:    msg,
		Steps:      steps,
		StepMs:     stepMs,
		RunID:      run.ID,
		AttemptID:  attempt.ID,
		Generation: attempt.LeaseGeneration,
	}, nil
}

func specInt(v any) (int, error) {
	switch n := v.(type) {
	case float64:
		return int(n), nil
	case int:
		return n, nil
	case int64:
		return int(n), nil
	default:
		return 0, fmt.Errorf("workload integer has unexpected type %T", v)
	}
}

func truncate(s string) string {
	if len(s) > domain.MaxReasonLength {
		return s[:domain.MaxReasonLength]
	}
	return s
}

func sleepCtx(ctx context.Context, d time.Duration) bool {
	select {
	case <-ctx.Done():
		return false
	case <-time.After(d):
		return true
	}
}

func newCorrelationID() string {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		return "worker-fallback-corr"
	}
	return hex.EncodeToString(b[:])
}

// --- HTTP protocol ---

func (c *client) do(ctx context.Context, method, path string, body any) (int, []byte, http.Header, error) {
	var reader io.Reader
	if body != nil {
		raw, err := json.Marshal(body)
		if err != nil {
			return 0, nil, nil, err
		}
		reader = bytes.NewReader(raw)
	}
	req, err := http.NewRequestWithContext(ctx, method, c.base+path, reader)
	if err != nil {
		return 0, nil, nil, err
	}
	req.Header.Set("Authorization", "Bearer "+c.token)
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Correlation-ID", newCorrelationID())
	resp, err := c.http.Do(req)
	if err != nil {
		return 0, nil, nil, err
	}
	defer func() { _ = resp.Body.Close() }()
	raw, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return 0, nil, nil, err
	}
	return resp.StatusCode, raw, resp.Header, nil
}

type apiError struct {
	Error struct {
		Code    string `json:"code"`
		Message string `json:"message"`
	} `json:"error"`
}

func errorCode(raw []byte) string {
	var e apiError
	if err := json.Unmarshal(raw, &e); err != nil {
		return ""
	}
	return e.Error.Code
}

func (c *client) register(ctx context.Context, workerID string) error {
	status, raw, _, err := c.do(ctx, http.MethodPost, "/api/v1/workers/register",
		map[string]any{"workerId": workerID, "version": workerVersion})
	if err != nil {
		return err
	}
	if status != http.StatusCreated && status != http.StatusOK {
		return fmt.Errorf("register status %d: %s", status, truncate(string(raw)))
	}
	return nil
}

func (c *client) poll(ctx context.Context, workerID string) (domain.Run, domain.Attempt, int, error) {
	status, raw, _, err := c.do(ctx, http.MethodPost, "/api/v1/workers/poll", map[string]any{"workerId": workerID})
	if err != nil {
		return domain.Run{}, domain.Attempt{}, 0, err
	}
	switch status {
	case http.StatusNoContent:
		return domain.Run{}, domain.Attempt{}, status, nil
	case http.StatusNotFound:
		return domain.Run{}, domain.Attempt{}, status, errWorkerUnknown
	case http.StatusOK:
		var out struct {
			Run     domain.Run     `json:"run"`
			Attempt domain.Attempt `json:"attempt"`
		}
		if err := json.Unmarshal(raw, &out); err != nil {
			return domain.Run{}, domain.Attempt{}, 0, err
		}
		return out.Run, out.Attempt, status, nil
	default:
		return domain.Run{}, domain.Attempt{}, 0, fmt.Errorf("poll status %d: %s", status, truncate(string(raw)))
	}
}

func (c *client) heartbeat(ctx context.Context, workerID, runID, attemptID string, generation int64) error {
	status, raw, _, err := c.do(ctx, http.MethodPost, "/api/v1/workers/heartbeat",
		map[string]any{"workerId": workerID, "runId": runID, "attemptId": attemptID, "generation": generation})
	if err != nil {
		return err
	}
	if status == http.StatusOK {
		return nil
	}
	if status == http.StatusConflict || status == http.StatusNotFound {
		return fmt.Errorf("heartbeat fenced (%s): status %d", errorCode(raw), status)
	}
	return fmt.Errorf("heartbeat status %d: %s", status, truncate(string(raw)))
}

func (c *client) report(ctx context.Context, workerID, runID, attemptID string, generation int64, outcome domain.ReportOutcome, output map[string]any, errMsg string) error {
	status, raw, _, err := c.do(ctx, http.MethodPost, "/api/v1/workers/report",
		map[string]any{
			"workerId": workerID, "runId": runID, "attemptId": attemptID,
			"generation": generation, "outcome": string(outcome),
			"output": output, "error": errMsg,
		})
	if err != nil {
		return err
	}
	if status == http.StatusOK {
		return nil
	}
	return fmt.Errorf("report status %d (%s): %s", status, errorCode(raw), truncate(string(raw)))
}
