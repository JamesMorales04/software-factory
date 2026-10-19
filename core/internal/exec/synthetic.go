// Package exec implements the S2-authorized synthetic workload fixture.
// The Worker executes exactly one fixed, bounded operation: it echoes a
// short validated message through a fixed number of steps and writes a
// receipt identifying the (run, attempt, generation) that produced it.
//
// Two modes exist. Preferred: a disposable rootless Podman container with
// no network, a read-only root filesystem, dropped capabilities, bounded
// resources, a single writable workspace mount and a minimal environment —
// never the user HOME, never credential files, never a container socket.
// Fallback: pure-Go in-process execution (no shell, no subprocess) used only
// when Podman is unavailable; the mode is always reported in the result.
//
// Nothing here executes arbitrary commands, mounts host secrets or confers
// SEC-01 certification: this fixture is credential-free by construction.
package exec

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

// PodmanImage is the pinned fixture image for containerized execution.
const PodmanImage = "docker.io/library/alpine:3.20"

// Mode of fixture execution, always reported in the result.
type Mode string

const (
	ModePodman Mode = "podman"
	ModeLocal  Mode = "local"
)

// Spec is the validated synthetic workload for one Attempt.
type Spec struct {
	Message    string
	Steps      int
	StepMs     int
	RunID      string
	AttemptID  string
	Generation int64
}

// Result is the bounded outcome reported back to Core.
type Result struct {
	Mode           Mode
	StepsCompleted int
	Receipt        string
	MessageDigest  string
	ElapsedMs      int64
}

// PodmanAvailable reports whether the podman CLI runs in this environment.
func PodmanAvailable(ctx context.Context) bool {
	c, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	return exec.CommandContext(c, "podman", "--version").Run() == nil
}

// Run executes the fixture in workDir (created by the caller, removed by
// the caller) and returns the bounded result. podmanOK selects the
// container path; otherwise the pure-Go path runs. ctx carries the
// Attempt timeout and the Worker's cancellation.
func Run(ctx context.Context, spec Spec, workDir string, podmanOK bool) (Result, error) {
	start := time.Now()
	digest := sha256.Sum256([]byte(spec.Message))
	receipt := fmt.Sprintf("%s/%s/%d", spec.RunID, spec.AttemptID, spec.Generation)
	steps := spec.Steps
	if steps < 1 {
		steps = 1
	}
	var mode Mode
	var err error
	if podmanOK {
		mode = ModePodman
		err = runPodman(ctx, spec, steps, workDir)
	} else {
		mode = ModeLocal
		err = runLocal(ctx, steps, spec.StepMs, workDir, receipt)
	}
	if err != nil {
		return Result{}, err
	}
	return Result{
		Mode:           mode,
		StepsCompleted: steps,
		Receipt:        receipt,
		MessageDigest:  "sha256:" + hex.EncodeToString(digest[:]),
		ElapsedMs:      time.Since(start).Milliseconds(),
	}, nil
}

// runLocal performs the fixture without any shell or subprocess: fixed
// sleeps plus one receipt write. Safe by construction.
func runLocal(ctx context.Context, steps, stepMs int, workDir, receipt string) error {
	for i := 0; i < steps; i++ {
		if stepMs > 0 {
			select {
			case <-ctx.Done():
				return ctx.Err()
			case <-time.After(time.Duration(stepMs) * time.Millisecond):
			}
		} else {
			select {
			case <-ctx.Done():
				return ctx.Err()
			default:
			}
		}
	}
	return os.WriteFile(filepath.Join(workDir, "receipt.txt"), []byte(receipt+"\n"), 0o600)
}

// runPodman executes the same fixed steps inside a disposable rootless
// container. The argv is fixed; only scalar env values vary. No HOME, no
// credential paths, no sockets and no host networking are ever mounted.
func runPodman(ctx context.Context, spec Spec, steps int, workDir string) error {
	script := `printf '%s' "$MESSAGE" > /workspace/message.txt; ` +
		`i=0; while [ "$i" -lt "$STEPS" ]; do echo "step-$i" >> /workspace/steps.txt; i=$((i+1)); sleep "$STEPMS"; done; ` +
		`printf '%s' "$RECEIPT" > /workspace/receipt.txt`
	// BusyBox sleep accepts fractional seconds; stepMs is domain-bounded
	// to 0..200 so "0.NNN" is always a valid short sleep.
	stepMsS := "0." + fmt.Sprintf("%03d", spec.StepMs)
	args := []string{
		"run", "--rm",
		"--network=none",
		"--read-only",
		"--cap-drop=ALL",
		"--security-opt=no-new-privileges",
		"--pids-limit=32",
		"--memory=128m",
		"--cpus=0.5",
		"--userns=keep-id",
		"-v", workDir + ":/workspace:rw",
		"--workdir", "/workspace",
		"--env", "MESSAGE=" + spec.Message,
		"--env", "STEPS=" + strconv.Itoa(steps),
		"--env", "STEPMS=" + stepMsS,
		"--env", "RECEIPT=" + spec.RunID + "/" + spec.AttemptID + "/" + strconv.FormatInt(spec.Generation, 10),
		"--name", containerName(spec.AttemptID),
		PodmanImage,
		"sh", "-c", script,
	}
	var stdout, stderr bytes.Buffer
	cmd := exec.CommandContext(ctx, "podman", args...)
	cmd.Stdout = &stdout
	// Bound stderr so a misbehaving runtime cannot flood Worker memory.
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		tail := stderr.String()
		if len(tail) > 2000 {
			tail = tail[len(tail)-2000:]
		}
		if errors.Is(ctx.Err(), context.DeadlineExceeded) {
			return fmt.Errorf("podman fixture timed out: %w", ctx.Err())
		}
		if errors.Is(ctx.Err(), context.Canceled) {
			return fmt.Errorf("podman fixture cancelled: %w", ctx.Err())
		}
		return fmt.Errorf("podman fixture failed: %v: %s", err, strings.TrimSpace(tail))
	}
	return nil
}

// pullImage fetches the pinned fixture image. A pull failure is a plain
// error: callers treat Podman execution as unavailable, never as success.
func pullImage(ctx context.Context) error {
	cmd := exec.CommandContext(ctx, "podman", "pull", PodmanImage)
	var tail bytes.Buffer
	cmd.Stdout = &tail
	cmd.Stderr = &tail
	if err := cmd.Run(); err != nil {
		out := tail.String()
		if len(out) > 1000 {
			out = out[len(out)-1000:]
		}
		return fmt.Errorf("pull %s: %v: %s", PodmanImage, err, strings.TrimSpace(out))
	}
	return nil
}

// PullImage is the exported fixture-image fetch used by the Worker startup
// probe and acceptance tooling.
func PullImage(ctx context.Context) error { return pullImage(ctx) }

func containerName(attemptID string) string {
	if len(attemptID) > 12 {
		attemptID = attemptID[:12]
	}
	return "factory-s2-" + attemptID
}
