// Package browserserver owns the disposable test-server lifecycle for
// browser-capable Runs. It starts an explicitly authorized Project command in
// the Attempt workspace, waits for the authorized loopback endpoint and owns
// termination. It never creates browser-capability evidence: only the
// Implementer's Factory-managed wrapper can do that.
package browserserver

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/telemetry"
)

// ErrPortOccupied means the authorized endpoint is healthy but is not owned
// by this Attempt. The Worker never adopts an unrelated process.
var ErrPortOccupied = errors.New("authorized endpoint is occupied by an unowned process")

// Config is one Attempt-scoped test-server request.
type Config struct {
	ID        string
	WorkDir   string
	Endpoint  string
	ReadyPath string
	Command   []string
	Timeout   time.Duration
}

type processMember struct {
	PID       int
	StartTime string
}

const (
	signalTerminate = 15
	signalKill      = 9
)

type processState struct {
	ID        string    `json:"id"`
	PID       int       `json:"pid"`
	PGID      int       `json:"pgid"`
	StartTime string    `json:"startTime"`
	WorkDir   string    `json:"workDir"`
	Endpoint  string    `json:"endpoint"`
	Command   []string  `json:"command"`
	LogPath   string    `json:"logPath"`
	CreatedAt time.Time `json:"createdAt"`
}

// Server is one managed test server. Stop is idempotent.
type Server struct {
	registryDir string
	state       processState
	logPath     string
	reused      bool
}

// RegistryDir is the base directory for managed test-server state.
func RegistryDir() string {
	return filepath.Join(os.TempDir(), "factory-test-servers")
}

// WorkerRegistryDir is the per-Worker registry. Per-Worker ownership prevents
// startup reaping in one Worker from killing another Worker's live server.
func WorkerRegistryDir(workerID string) string {
	return filepath.Join(RegistryDir(), "worker-"+stateKey(workerID))
}

// AcquireWorkerRegistry locks the per-Worker registry for this process. It
// prevents two Worker processes with the same logical ID from sharing and
// reaping each other's live test servers.
func AcquireWorkerRegistry(workerID string) (string, func(), error) {
	if err := Supported(); err != nil {
		return "", nil, err
	}
	base := RegistryDir()
	if err := ensureSecureDir(base); err != nil {
		return "", nil, err
	}
	dir := WorkerRegistryDir(workerID)
	if err := ensureSecureDir(dir); err != nil {
		return "", nil, err
	}
	lockPath := filepath.Join(base, "worker-"+stateKey(workerID)+".lock")
	release, err := acquireRegistryLock(lockPath)
	if err != nil {
		return "", nil, err
	}
	return dir, release, nil
}

// Reused reports whether an already-owned healthy server was reused.
func (s *Server) Reused() bool { return s != nil && s.reused }

// Endpoint returns the authorized loopback endpoint.
func (s *Server) Endpoint() string {
	if s == nil {
		return ""
	}
	return s.state.Endpoint
}

// Start ensures exactly one healthy managed server for this Attempt. If a
// state row for the same workspace/endpoint is alive and healthy it is
// reused. A healthy unowned endpoint is rejected rather than adopted.
func Start(ctx context.Context, cfg Config, registryDir string) (*Server, error) {
	if err := validate(cfg); err != nil {
		return nil, err
	}
	if err := Supported(); err != nil {
		return nil, err
	}
	if registryDir == "" {
		registryDir = RegistryDir()
	}
	if err := ensureRegistry(registryDir); err != nil {
		return nil, err
	}
	statePath := filepath.Join(registryDir, stateKey(cfg.ID)+".json")
	state, err := loadState(statePath)
	if err == nil {
		if state.WorkDir != cfg.WorkDir || state.Endpoint != cfg.Endpoint {
			return nil, fmt.Errorf("test-server state belongs to a different Attempt workspace/endpoint")
		}
		if alive, startTime := processAlive(state.PID); alive && startTime == state.StartTime {
			if _, owned := ownedGroup(state); owned && healthy(ctx, cfg.Endpoint, cfg.ReadyPath) {
				server := &Server{registryDir: registryDir, state: state, logPath: state.LogPath, reused: true}
				return server, nil
			}
		}
		// The leader may be gone while descendants remain. Always let
		// stopState verify/kill the recorded group; it retains state when
		// ownership cannot be proven.
		if err := stopState(state, registryDir); err != nil {
			return nil, fmt.Errorf("stale managed test server cleanup is unverified: %w", err)
		}
	}
	if healthy(ctx, cfg.Endpoint, cfg.ReadyPath) {
		return nil, ErrPortOccupied
	}
	if err := start(cfg, statePath, registryDir); err != nil {
		return nil, err
	}
	state, err = loadState(statePath)
	if err != nil {
		return nil, err
	}
	server := &Server{registryDir: registryDir, state: state, logPath: state.LogPath}
	if err := waitReady(ctx, cfg, registryDir); err != nil {
		_ = server.Stop(context.Background())
		return nil, err
	}
	return server, nil
}

func start(cfg Config, statePath, registryDir string) error {
	logPath := filepath.Join(registryDir, stateKey(cfg.ID)+".log")
	logFile, err := os.OpenFile(logPath, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0o600)
	if err != nil {
		return fmt.Errorf("open test-server log: %w", err)
	}
	defer func() { _ = logFile.Close() }()
	command := append([]string(nil), cfg.Command...)
	cmd := exec.Command(command[0], command[1:]...)
	cmd.Dir = cfg.WorkDir
	port, _ := endpointPort(cfg.Endpoint)
	cmd.Env = []string{
		"PATH=" + os.Getenv("PATH"),
		"HOME=" + os.Getenv("HOME"),
		"TMPDIR=" + os.Getenv("TMPDIR"),
		"LC_ALL=C",
		"HOST=127.0.0.1",
		"PORT=" + port,
		"FACTORY_TEST_ENDPOINT=" + cfg.Endpoint,
	}
	cmd.Stdout = logFile
	cmd.Stderr = logFile
	configureProcess(cmd)
	if err := cmd.Start(); err != nil {
		return fmt.Errorf("start test server: %w", err)
	}
	pgid := processGroup(cmd)
	alive, startTime := processAlive(cmd.Process.Pid)
	if !alive || startTime == "" {
		_ = cmd.Process.Kill()
		return errors.New("test server exited before ownership could be established")
	}
	state := processState{
		ID: cfg.ID, PID: cmd.Process.Pid, PGID: pgid, StartTime: startTime,
		WorkDir: cfg.WorkDir, Endpoint: cfg.Endpoint, Command: command,
		LogPath: logPath, CreatedAt: time.Now().UTC(),
	}
	raw, _ := json.Marshal(state)
	if err := os.WriteFile(statePath, raw, 0o600); err != nil {
		// The child is ours and just started; kill the whole group with a
		// direct group signal (no PID-reuse concern for a just-created child
		// group). No state row exists yet, so this is the only cleanup path.
		_ = killNewGroup(pgid)
		return fmt.Errorf("record test-server state: %w", err)
	}
	// Detach from cmd.Wait: the server is supervised by Stop/Reap, not by
	// this context. The process is still our owned child.
	go func() { _ = cmd.Wait() }()
	return nil
}

func waitReady(ctx context.Context, cfg Config, registryDir string) error {
	deadline := time.Now().Add(cfg.Timeout)
	for {
		if healthy(ctx, cfg.Endpoint, cfg.ReadyPath) {
			return nil
		}
		if time.Now().After(deadline) {
			return fmt.Errorf("browser-test-server did not become ready within %s: %s", cfg.Timeout, logTail(cfg.ID, registryDir))
		}
		select {
		case <-ctx.Done():
			return fmt.Errorf("browser-test-server readiness cancelled: %w", ctx.Err())
		case <-time.After(200 * time.Millisecond):
		}
	}
}

func healthy(ctx context.Context, endpoint, readyPath string) bool {
	target := strings.TrimRight(endpoint, "/") + readyPath
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, target, nil)
	if err != nil {
		return false
	}
	client := &http.Client{Timeout: 1 * time.Second, Transport: &http.Transport{Proxy: nil}}
	resp, err := client.Do(req)
	if err != nil {
		return false
	}
	defer func() { _ = resp.Body.Close() }()
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 4096))
	return resp.StatusCode >= 200 && resp.StatusCode < 300
}

// Stop terminates the owned process group and removes its state/log.
func (s *Server) Stop(ctx context.Context) error {
	if s == nil {
		return nil
	}
	_ = ctx
	return stopState(s.state, s.registryDir)
}

func stopState(state processState, registryDir string) error {
	members, safe := ownedGroup(state)
	if !safe {
		return errors.New("test-server process identity is unverified; refusing to signal")
	}
	if len(members) > 0 {
		signalOwnedMembers(state, members, signalTerminate)
		deadline := time.Now().Add(5 * time.Second)
		for time.Now().Before(deadline) {
			remaining, err := groupMembers(state.PGID)
			if err != nil || len(remaining) == 0 {
				break
			}
			time.Sleep(100 * time.Millisecond)
		}
		remaining, err := groupMembers(state.PGID)
		if err == nil && len(remaining) > 0 {
			if checked, ok := ownedGroup(state); ok {
				signalOwnedMembers(state, checked, signalKill)
			}
		}
		time.Sleep(100 * time.Millisecond)
		remaining, err = groupMembers(state.PGID)
		if err != nil {
			return fmt.Errorf("test-server termination unverified: %w", err)
		}
		if len(remaining) > 0 {
			return fmt.Errorf("test-server process group still has %d member(s) after termination", len(remaining))
		}
	}
	_ = os.Remove(filepath.Join(registryDir, stateKey(state.ID)+".json"))
	_ = os.Remove(state.LogPath)
	return nil
}

func ownedGroup(state processState) ([]processMember, bool) {
	if state.PGID <= 0 || state.StartTime == "" || state.WorkDir == "" {
		return nil, false
	}
	if alive, startTime := processAlive(state.PID); alive && startTime != state.StartTime {
		return nil, false
	}
	members, err := groupMembers(state.PGID)
	if err != nil {
		return nil, false
	}
	if len(members) == 0 {
		return nil, true
	}
	for _, member := range members {
		cwd, err := processCwd(member.PID)
		if err != nil {
			if !os.IsNotExist(err) {
				return nil, false
			}
			// The Attempt workspace may already be removed. Group membership
			// plus the recorded start time remains the identity anchor.
		} else if !withinWorkspace(cwd, state.WorkDir) {
			return nil, false
		}
		if member.PID == state.PID && member.StartTime != state.StartTime {
			return nil, false
		}
	}
	return members, true
}

func signalOwnedMembers(state processState, members []processMember, signal int) {
	for _, member := range members {
		fd, err := openProcessHandle(member.PID)
		if err != nil {
			continue
		}
		start, cwd, err := processIdentity(member.PID)
		if err != nil {
			if !os.IsNotExist(err) {
				closeProcessHandle(fd)
				continue
			}
			start = member.StartTime
		} else if !withinWorkspace(cwd, state.WorkDir) {
			closeProcessHandle(fd)
			continue
		}
		if start != member.StartTime || (member.PID == state.PID && start != state.StartTime) {
			closeProcessHandle(fd)
			continue
		}
		_ = signalProcessHandle(fd, signal)
		closeProcessHandle(fd)
	}
}

func ensureRegistry(path string) error { return ensureSecureDir(path) }

func ensureSecureDir(path string) error {
	if err := ensureNoSymlinkAncestors(path); err != nil {
		return err
	}
	info, err := os.Lstat(path)
	if err == nil {
		if info.Mode()&os.ModeSymlink != 0 || !info.IsDir() {
			return fmt.Errorf("test-server registry path is not a safe directory")
		}
		if !dirOwnedByCurrentUser(info) {
			return fmt.Errorf("test-server registry is not owned by the current user")
		}
		if info.Mode().Perm() != 0o700 {
			if err := os.Chmod(path, 0o700); err != nil {
				return fmt.Errorf("secure test-server registry: %w", err)
			}
		}
		return nil
	}
	if !os.IsNotExist(err) {
		return err
	}
	if err := os.MkdirAll(path, 0o700); err != nil {
		return fmt.Errorf("create test-server registry: %w", err)
	}
	info, err = os.Lstat(path)
	if err != nil || !info.IsDir() || !dirOwnedByCurrentUser(info) {
		return fmt.Errorf("created test-server registry has unsafe ownership")
	}
	return os.Chmod(path, 0o700)
}

func ensureNoSymlinkAncestors(path string) error {
	abs, err := filepath.Abs(path)
	if err != nil {
		return err
	}
	current := string(filepath.Separator)
	parts := strings.Split(strings.TrimPrefix(abs, string(filepath.Separator)), string(filepath.Separator))
	for _, part := range parts {
		current = filepath.Join(current, part)
		info, err := os.Lstat(current)
		if os.IsNotExist(err) {
			return nil
		}
		if err != nil {
			return err
		}
		if info.Mode()&os.ModeSymlink != 0 {
			return fmt.Errorf("test-server registry path has a symlinked ancestor")
		}
	}
	return nil
}

// Reap kills managed test servers left by a previous Worker process.
func Reap(registryDir string) (int, error) {
	if registryDir == "" {
		registryDir = RegistryDir()
	}
	entries, err := os.ReadDir(registryDir)
	if err != nil {
		if os.IsNotExist(err) {
			return 0, nil
		}
		return 0, err
	}
	killed := 0
	for _, entry := range entries {
		if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".json") {
			continue
		}
		state, err := loadState(filepath.Join(registryDir, entry.Name()))
		if err != nil {
			_ = os.Remove(filepath.Join(registryDir, entry.Name()))
			continue
		}
		members, safe := ownedGroup(state)
		if !safe {
			// Do not signal an unverified PID/group and do not discard its
			// state: a later Stop/Reap can retry when identity is provable.
			continue
		}
		if len(members) > 0 {
			signalOwnedMembers(state, members, signalTerminate)
			deadline := time.Now().Add(3 * time.Second)
			for time.Now().Before(deadline) {
				remaining, err := groupMembers(state.PGID)
				if err != nil || len(remaining) == 0 {
					break
				}
				time.Sleep(50 * time.Millisecond)
			}
			remaining, err := groupMembers(state.PGID)
			if err != nil {
				continue
			}
			if len(remaining) > 0 {
				if checked, ok := ownedGroup(state); ok {
					signalOwnedMembers(state, checked, signalKill)
				}
				time.Sleep(100 * time.Millisecond)
				remaining, err = groupMembers(state.PGID)
				if err != nil || len(remaining) > 0 {
					continue
				}
			}
			killed++
		}
		_ = os.Remove(filepath.Join(registryDir, entry.Name()))
		_ = os.Remove(state.LogPath)
	}
	return killed, nil
}

func validate(cfg Config) error {
	if strings.TrimSpace(cfg.ID) == "" || strings.TrimSpace(cfg.WorkDir) == "" {
		return errors.New("test-server ID and work directory are required")
	}
	if len(cfg.Command) == 0 {
		return errors.New("test-server command is required")
	}
	if cfg.Timeout < 5*time.Second || cfg.Timeout > 10*time.Minute {
		return errors.New("test-server timeout is out of range")
	}
	parsed, err := url.Parse(cfg.Endpoint)
	if err != nil || parsed.Scheme != "http" || parsed.User != nil {
		return errors.New("test-server endpoint must be a plain http URL")
	}
	host := parsed.Hostname()
	if host != "127.0.0.1" && host != "localhost" && host != "::1" {
		return errors.New("test-server endpoint must be loopback")
	}
	if _, err := endpointPort(cfg.Endpoint); err != nil {
		return err
	}
	if !filepath.IsAbs(cfg.WorkDir) {
		return errors.New("test-server work directory must be absolute")
	}
	if !strings.HasPrefix(cfg.ReadyPath, "/") || strings.Contains(cfg.ReadyPath, "..") {
		return errors.New("test-server ready path must be a safe URL path")
	}
	return nil
}

func endpointPort(endpoint string) (string, error) {
	parsed, err := url.Parse(endpoint)
	if err != nil || parsed.Port() == "" {
		return "", errors.New("test-server endpoint must include an explicit port")
	}
	if _, err := strconv.Atoi(parsed.Port()); err != nil {
		return "", errors.New("test-server endpoint port is invalid")
	}
	return parsed.Port(), nil
}

func stateKey(id string) string {
	sum := sha256.Sum256([]byte(id))
	return hex.EncodeToString(sum[:16])
}

func loadState(path string) (processState, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		return processState{}, err
	}
	var state processState
	if err := json.Unmarshal(raw, &state); err != nil {
		return processState{}, err
	}
	return state, nil
}

func logTail(id, registryDir string) string {
	if registryDir == "" {
		registryDir = RegistryDir()
	}
	path := filepath.Join(registryDir, stateKey(id)+".log")
	raw, err := os.ReadFile(path)
	if err != nil {
		return "test-server log unavailable"
	}
	if len(raw) > 4000 {
		raw = raw[len(raw)-4000:]
	}
	value := telemetry.Redact(strings.Join(strings.Fields(string(raw)), " "))
	if len(value) > 500 {
		value = value[:500]
	}
	if value == "" {
		return "test-server produced no bounded output"
	}
	return value
}
