package exec

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestLocalFixtureWritesReceipt(t *testing.T) {
	workDir := t.TempDir()
	spec := Spec{Message: "hello", Steps: 3, StepMs: 1, RunID: "r", AttemptID: "a", Generation: 1}
	res, err := Run(context.Background(), spec, workDir, false)
	if err != nil {
		t.Fatalf("run: %v", err)
	}
	if res.Mode != ModeLocal || res.StepsCompleted != 3 || res.Receipt != "r/a/1" {
		t.Fatalf("unexpected result: %+v", res)
	}
	raw, err := os.ReadFile(filepath.Join(workDir, "receipt.txt"))
	if err != nil || string(raw) != "r/a/1\n" {
		t.Fatalf("receipt: %q err=%v", raw, err)
	}
	if res.MessageDigest == "" {
		t.Fatal("missing message digest")
	}
}

func TestLocalFixtureHonorsCancellation(t *testing.T) {
	workDir := t.TempDir()
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	spec := Spec{Message: "x", Steps: 5, StepMs: 100, RunID: "r", AttemptID: "a", Generation: 1}
	if _, err := Run(ctx, spec, workDir, false); err == nil {
		t.Fatal("expected cancellation error")
	}
}

func TestPodmanFixtureWhenAvailable(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	defer cancel()
	if !PodmanAvailable(ctx) {
		t.Skip("podman unavailable: containerized fixture NOT_TESTED in this environment")
	}
	if err := ensureImage(ctx); err != nil {
		t.Skipf("fixture image unavailable: %v", err)
	}
	workDir := t.TempDir()
	spec := Spec{Message: "podman-proof", Steps: 2, StepMs: 5, RunID: "r1", AttemptID: "a1", Generation: 2}
	res, err := Run(ctx, spec, workDir, true)
	if err != nil {
		t.Fatalf("podman run: %v", err)
	}
	if res.Mode != ModePodman || res.Receipt != "r1/a1/2" {
		t.Fatalf("unexpected result: %+v", res)
	}
	raw, err := os.ReadFile(filepath.Join(workDir, "receipt.txt"))
	if err != nil || string(raw) != "r1/a1/2" {
		t.Fatalf("container receipt: %q err=%v", raw, err)
	}
}

func ensureImage(ctx context.Context) error {
	return PullImage(ctx)
}
