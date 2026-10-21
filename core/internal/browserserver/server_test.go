package browserserver

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func freePort(t *testing.T) int {
	t.Helper()
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = listener.Close() }()
	return listener.Addr().(*net.TCPAddr).Port
}

func pythonServerCommand() []string {
	return []string{"python3", "-c", "import http.server,os; http.server.HTTPServer(('127.0.0.1', int(os.environ['PORT'])), http.server.SimpleHTTPRequestHandler).serve_forever()"}
}

func TestManagedServerStartReadyReuseAndStop(t *testing.T) {
	ctx := context.Background()
	work := t.TempDir()
	registry := t.TempDir()
	if err := os.WriteFile(filepath.Join(work, "marker.txt"), []byte("candidate-revision-1"), 0o600); err != nil {
		t.Fatal(err)
	}
	port := freePort(t)
	cfg := Config{
		ID: "attempt-1", WorkDir: work, Endpoint: fmt.Sprintf("http://127.0.0.1:%d", port),
		ReadyPath: "/", Command: pythonServerCommand(), Timeout: 10 * time.Second,
	}
	server, err := Start(ctx, cfg, registry)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = server.Stop(context.Background()) }()
	if server.Reused() {
		t.Fatal("fresh server reported reused")
	}
	resp, err := http.Get(cfg.Endpoint + "/marker.txt")
	if err != nil {
		t.Fatal(err)
	}
	body, _ := io.ReadAll(resp.Body)
	_ = resp.Body.Close()
	if string(body) != "candidate-revision-1" {
		t.Fatalf("server did not serve the Attempt workspace: %q", body)
	}
	reused, err := Start(ctx, cfg, registry)
	if err != nil {
		t.Fatal(err)
	}
	if !reused.Reused() {
		t.Fatal("healthy owned server was not reused")
	}
	if reused.state.PID != server.state.PID {
		t.Fatalf("reuse changed PID: %d vs %d", reused.state.PID, server.state.PID)
	}
	if err := server.Stop(context.Background()); err != nil {
		t.Fatal(err)
	}
	client := &http.Client{Timeout: 300 * time.Millisecond}
	if _, err := client.Get(cfg.Endpoint); err == nil {
		t.Fatal("server still reachable after Stop")
	}
	entries, _ := os.ReadDir(registry)
	if len(entries) != 0 {
		t.Fatalf("registry not cleaned: %v", entries)
	}
}

func TestUnrelatedEndpointIsNotAdopted(t *testing.T) {
	ctx := context.Background()
	work := t.TempDir()
	registry := t.TempDir()
	port := freePort(t)
	endpoint := fmt.Sprintf("http://127.0.0.1:%d", port)
	external := &Server{}
	_ = external
	externalCmd := pythonServerCommand()
	cfg := Config{ID: "external", WorkDir: work, Endpoint: endpoint, ReadyPath: "/", Command: externalCmd, Timeout: 10 * time.Second}
	owned, err := Start(ctx, cfg, registry)
	if err != nil {
		t.Fatal(err)
	}
	// Remove the ownership row: the endpoint is healthy but unowned.
	entries, _ := os.ReadDir(registry)
	for _, entry := range entries {
		_ = os.Remove(filepath.Join(registry, entry.Name()))
	}
	request := cfg
	request.ID = "attempt-2"
	if _, err := Start(ctx, request, registry); err == nil || !strings.Contains(err.Error(), "unowned process") {
		t.Fatalf("unowned endpoint not rejected: %v", err)
	}
	if err := owned.Stop(context.Background()); err != nil {
		t.Fatal(err)
	}
}

func TestStartupFailureIsDiagnosedAndCleaned(t *testing.T) {
	ctx := context.Background()
	work := t.TempDir()
	registry := t.TempDir()
	port := freePort(t)
	cfg := Config{
		ID: "bad", WorkDir: work, Endpoint: fmt.Sprintf("http://127.0.0.1:%d", port),
		ReadyPath: "/", Command: []string{"python3", "-c", "import sys; sys.exit(7)"},
		Timeout: 5 * time.Second,
	}
	start := time.Now()
	if _, err := Start(ctx, cfg, registry); err == nil {
		t.Fatal("startup failure accepted")
	} else if !strings.Contains(err.Error(), "did not become ready") {
		t.Fatalf("startup failure not diagnosed: %v", err)
	}
	if time.Since(start) > 8*time.Second {
		t.Fatal("startup failure exceeded readiness bound")
	}
	entries, _ := os.ReadDir(registry)
	if len(entries) != 0 {
		t.Fatalf("failed server left registry entries: %v", entries)
	}
}

func TestReapKillsManagedServer(t *testing.T) {
	ctx := context.Background()
	work := t.TempDir()
	registry := t.TempDir()
	port := freePort(t)
	cfg := Config{
		ID: "reap", WorkDir: work, Endpoint: fmt.Sprintf("http://127.0.0.1:%d", port),
		ReadyPath: "/", Command: pythonServerCommand(), Timeout: 10 * time.Second,
	}
	server, err := Start(ctx, cfg, registry)
	if err != nil {
		t.Fatal(err)
	}
	if killed, err := Reap(registry); err != nil || killed != 1 {
		t.Fatalf("reap = %d, %v", killed, err)
	}
	client := &http.Client{Timeout: 300 * time.Millisecond}
	if _, err := client.Get(cfg.Endpoint); err == nil {
		t.Fatal("reaped server still reachable")
	}
	_ = server
	entries, _ := os.ReadDir(registry)
	if len(entries) != 0 {
		t.Fatalf("reap left registry entries: %v", entries)
	}
}

func TestProcessProbeHandlesSpacedCommandName(t *testing.T) {
	cmd := exec.Command("bash", "-c", `exec -a "npm start" sleep 5`)
	configureProcess(cmd)
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() {
		_ = cmd.Process.Kill()
		_, _ = cmd.Process.Wait()
	}()
	alive, startTime := processAlive(cmd.Process.Pid)
	if !alive || startTime == "" {
		t.Fatalf("spaced command name not probed: alive=%v start=%q", alive, startTime)
	}
}

func TestStopKillsDescendantAfterLeaderExit(t *testing.T) {
	ctx := context.Background()
	work := t.TempDir()
	registry := t.TempDir()
	port := freePort(t)
	cfg := Config{
		ID: "descendant", WorkDir: work, Endpoint: fmt.Sprintf("http://127.0.0.1:%d", port),
		ReadyPath: "/",
		Command: []string{
			"bash", "-c",
			"python3 -m http.server \"$PORT\" --bind 127.0.0.1 >/dev/null 2>&1 & sleep 1; exit 0",
		},
		Timeout: 10 * time.Second,
	}
	server, err := Start(ctx, cfg, registry)
	if err != nil {
		t.Fatal(err)
	}
	time.Sleep(2 * time.Second) // leader exits; descendant remains in the group
	if err := server.Stop(context.Background()); err != nil {
		t.Fatal(err)
	}
	client := &http.Client{Timeout: 300 * time.Millisecond}
	if _, err := client.Get(cfg.Endpoint); err == nil {
		t.Fatal("descendant server still reachable after Stop")
	}
}

func TestWorkerRegistryLockAndSymlinkRefusal(t *testing.T) {
	t.Setenv("TMPDIR", t.TempDir())
	dir, release, err := AcquireWorkerRegistry("worker-1")
	if err != nil {
		t.Fatal(err)
	}
	if _, _, err := AcquireWorkerRegistry("worker-1"); err == nil {
		t.Fatal("second same-ID Worker registry lock was not refused")
	}
	release()
	if _, release2, err := AcquireWorkerRegistry("worker-1"); err != nil {
		t.Fatalf("registry not reusable after release: %v", err)
	} else {
		release2()
	}
	_ = dir

	other := t.TempDir()
	base := RegistryDir()
	_ = os.RemoveAll(base)
	if err := os.Symlink(other, base); err != nil {
		t.Skip("symlink creation unavailable")
	}
	if _, _, err := AcquireWorkerRegistry("worker-2"); err == nil {
		t.Fatal("symlinked registry base was accepted")
	}
}

func TestEmptyProcessIdentityIsRefused(t *testing.T) {
	state := processState{PGID: 123, StartTime: "", WorkDir: t.TempDir()}
	if _, ok := ownedGroup(state); ok {
		t.Fatal("empty process identity was treated as owned")
	}
}

func TestSymlinkedAncestorIsRefused(t *testing.T) {
	base := t.TempDir()
	target := filepath.Join(base, "real")
	if err := os.Mkdir(target, 0o750); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(base, "link")
	if err := os.Symlink(target, link); err != nil {
		t.Skip("symlink creation unavailable")
	}
	if err := ensureSecureDir(filepath.Join(link, "registry")); err == nil {
		t.Fatal("symlinked ancestor accepted")
	}
}

func TestStopAfterWorkspaceDeleted(t *testing.T) {
	ctx := context.Background()
	work := t.TempDir()
	registry := t.TempDir()
	port := freePort(t)
	cfg := Config{
		ID: "deleted-workspace", WorkDir: work, Endpoint: fmt.Sprintf("http://127.0.0.1:%d", port),
		ReadyPath: "/", Command: pythonServerCommand(), Timeout: 10 * time.Second,
	}
	server, err := Start(ctx, cfg, registry)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.RemoveAll(work); err != nil {
		t.Fatal(err)
	}
	if err := server.Stop(context.Background()); err != nil {
		t.Fatalf("stop after workspace deletion: %v", err)
	}
	client := &http.Client{Timeout: 300 * time.Millisecond}
	if _, err := client.Get(cfg.Endpoint); err == nil {
		t.Fatal("server survived workspace deletion cleanup")
	}
}

func TestStaleStateDoesNotReuseMismatchedWorkspace(t *testing.T) {
	ctx := context.Background()
	work := t.TempDir()
	other := t.TempDir()
	registry := t.TempDir()
	port := freePort(t)
	id := "stale-state"
	cfg := Config{ID: id, WorkDir: work, Endpoint: fmt.Sprintf("http://127.0.0.1:%d", port), ReadyPath: "/", Command: pythonServerCommand(), Timeout: 10 * time.Second}
	server, err := Start(ctx, cfg, registry)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = server.Stop(context.Background()) }()
	statePath := filepath.Join(registry, stateKey(id)+".json")
	raw, err := os.ReadFile(statePath)
	if err != nil {
		t.Fatal(err)
	}
	var state map[string]any
	if err := json.Unmarshal(raw, &state); err != nil {
		t.Fatal(err)
	}
	state["workDir"] = other
	raw, _ = json.Marshal(state)
	if err := os.WriteFile(statePath, raw, 0o600); err != nil {
		t.Fatal(err)
	}
	otherCfg := cfg
	otherCfg.WorkDir = other
	if _, err := Start(ctx, otherCfg, registry); err == nil {
		t.Fatal("stale state with mismatched workspace was reused")
	}
	if _, err := os.Stat(statePath); err != nil {
		t.Fatal("unverified stale state was removed")
	}
}

func TestStaleStateKillsDescendantAfterLeaderExit(t *testing.T) {
	ctx := context.Background()
	work := t.TempDir()
	registry := t.TempDir()
	port := freePort(t)
	cfg := Config{
		ID: "stale-descendant", WorkDir: work,
		Endpoint: fmt.Sprintf("http://127.0.0.1:%d", port), ReadyPath: "/",
		Command: []string{
			"bash", "-c",
			"python3 -m http.server \"$PORT\" --bind 127.0.0.1 >/dev/null 2>&1 & sleep 1; exit 0",
		},
		Timeout: 10 * time.Second,
	}
	first, err := Start(ctx, cfg, registry)
	if err != nil {
		t.Fatal(err)
	}
	time.Sleep(2 * time.Second) // leader exits, descendant remains
	second, err := Start(ctx, cfg, registry)
	if err != nil {
		t.Fatalf("restart over stale descendant: %v", err)
	}
	defer func() { _ = second.Stop(context.Background()) }()
	if second.state.PID == first.state.PID {
		t.Fatal("stale state was reused instead of cleaned")
	}
	if healthy(ctx, cfg.Endpoint, cfg.ReadyPath) == false {
		t.Fatal("new managed server is not healthy")
	}
}
