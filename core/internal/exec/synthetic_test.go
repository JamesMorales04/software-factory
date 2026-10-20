package exec

import (
	"context"
	"os"
	"path/filepath"
	"strings"
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

func TestPodmanArgsAuditLocksRestrictionSet(t *testing.T) {
	spec := Spec{Message: "audit", Steps: 2, StepMs: 5, RunID: "r", AttemptID: "a123", Generation: 7}
	args := podmanArgs(spec, 2, "/tmp/factory-s2-work", "docker.io/library/alpine:3.20@sha256:abc123")
	// Exact argv: any flag drift, new mount, env change, reordered
	// restriction, STEPMS encoding change, or unpinned image reference
	// fails this test. The fixture script text is locked verbatim.
	script := `printf '%s' "$MESSAGE" > /workspace/message.txt; ` +
		`i=0; while [ "$i" -lt "$STEPS" ]; do echo "step-$i" >> /workspace/steps.txt; i=$((i+1)); sleep "$STEPMS"; done; ` +
		`printf '%s' "$RECEIPT" > /workspace/receipt.txt`
	want := []string{
		"run", "--rm",
		"--network=none",
		"--read-only",
		"--cap-drop=ALL",
		"--security-opt=no-new-privileges",
		"--pids-limit=32",
		"--memory=128m",
		"--cpus=0.5",
		"--userns=keep-id",
		"-v", "/tmp/factory-s2-work:/workspace:rw",
		"--workdir", "/workspace",
		"--env", "MESSAGE=audit",
		"--env", "STEPS=2",
		"--env", "STEPMS=0.005",
		"--env", "RECEIPT=r/a123/7",
		"--name", "factory-s2-a123",
		"docker.io/library/alpine:3.20@sha256:abc123",
		"sh", "-c", script,
	}
	if len(args) != len(want) {
		t.Fatalf("podman argv length %d, want %d:\n got %q", len(args), len(want), args)
	}
	for i := range want {
		if args[i] != want[i] {
			t.Fatalf("podman argv[%d] = %q, want %q (full: %q)", i, args[i], want[i], args)
		}
	}
	joined := " " + strings.Join(args, " ") + " "
	forbidden := []string{
		"--privileged", "--network=host", "/run/podman", "/run/docker",
		"/root", "/home/", "--env HOME=", "--env PATH=",
		"--env FACTORY_", "--env OPENAI", "--env GITHUB", "--volume", ":ro ",
		"alpine:3.20 ",
	}
	for _, bad := range forbidden {
		if strings.Contains(joined, bad) {
			t.Fatalf("podman argv contains forbidden %q in %q", bad, joined)
		}
	}
}

func TestReapSkipsLiveRoots(t *testing.T) {
	// A live Worker holds its root lock: simulate one by taking the lock
	// in this test process, then prove the reaper skips it while it can
	// still collect a released (dead-owner) root.
	live, err := RunsRoot()
	if err != nil {
		t.Fatalf("runs root: %v", err)
	}
	defer func() { _ = live.Close() }()
	deadDir, err := os.MkdirTemp("", "factory-worker-runs-dead-*")
	if err != nil {
		t.Fatalf("mkdir: %v", err)
	}
	decoy, err := os.MkdirTemp("", "factory-s2-harness-*")
	if err != nil {
		t.Fatalf("mkdir: %v", err)
	}
	defer func() { _ = os.RemoveAll(decoy) }()
	n := ReapStaleRunRoots()
	if n != 1 {
		t.Fatalf("reaped %d roots, want exactly the dead one", n)
	}
	if _, err := os.Stat(live.Dir()); err != nil {
		t.Fatalf("live root reaped: %v", err)
	}
	if _, err := os.Stat(deadDir); !os.IsNotExist(err) {
		t.Fatalf("dead root survived: %s", deadDir)
	}
	if _, err := os.Stat(decoy); err != nil {
		t.Fatalf("unrelated dir removed: %v", err)
	}
	// After the owner exits (lock released), the same root is collectable.
	if err := live.Close(); err != nil {
		t.Fatalf("close: %v", err)
	}
	if n := ReapStaleRunRoots(); n != 0 {
		t.Fatalf("closed root should already be gone, reaped %d", n)
	}
}

func TestRunsRootCreatesOwnedParent(t *testing.T) {
	root, err := RunsRoot()
	if err != nil {
		t.Fatalf("runs root: %v", err)
	}
	if base := filepath.Base(root.Dir()); len(base) < len("factory-worker-runs-") || base[:len("factory-worker-runs-")] != "factory-worker-runs-" {
		t.Fatalf("unexpected runs root pattern: %s", root.Dir())
	}
	if err := root.Close(); err != nil {
		t.Fatalf("close: %v", err)
	}
	if _, serr := os.Stat(root.Dir()); !os.IsNotExist(serr) {
		t.Fatalf("closed root survived: %s", root.Dir())
	}
}
