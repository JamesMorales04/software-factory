package main

import (
	"errors"
	"fmt"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"time"
)

const (
	startTimeout = 20 * time.Second
	stopTimeout  = 10 * time.Second
)

func start(cfg config) error {
	if err := ensurePrivateDir(cfg.home); err != nil {
		return err
	}
	lock, err := acquireLock(cfg.home)
	if err != nil {
		return err
	}
	defer releaseLock(lock)

	statePath := filepath.Join(cfg.home, "factory.pid")
	if state, err := readState(statePath); err == nil {
		if processAlive(state.PID) {
			if !matchesCore(state) {
				return fmt.Errorf("PID %d is alive but is not the recorded Factory Core; refusing to signal it", state.PID)
			}
			if healthy(state.Addr, filepath.Join(cfg.home, "api.token")) {
				printInfo(cfg, state.PID, "already running")
				return nil
			}
			return fmt.Errorf("factory Core PID %d exists but readiness failed; inspect %s", state.PID, filepath.Join(cfg.home, "factory.log"))
		}
		_ = os.Remove(statePath)
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}

	if _, err := os.Stat(cfg.binary); err != nil {
		return fmt.Errorf("factory Core executable not found at %s (build the local distribution first)", cfg.binary)
	}
	if info, err := os.Stat(filepath.Join(cfg.webDir, "index.html")); err != nil || !info.Mode().IsRegular() {
		return fmt.Errorf("built Factory Office index.html not found in %s", cfg.webDir)
	}

	token, err := loadOrCreateToken(filepath.Join(cfg.home, "api.token"))
	if err != nil {
		return err
	}
	// The scoped worker credential is provisioned alongside the operator
	// token so the Core can authenticate the Worker protocol from boot.
	workerToken, err := loadOrCreateTokenEnv(filepath.Join(cfg.home, "worker.token"), "FACTORY_WORKER_TOKEN")
	if err != nil {
		return err
	}
	logPath := filepath.Join(cfg.home, "factory.log")
	logFile, err := os.OpenFile(logPath, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
	if err != nil {
		return fmt.Errorf("open Factory log: %w", err)
	}
	if err := logFile.Chmod(0o600); err != nil {
		_ = logFile.Close()
		return fmt.Errorf("secure Factory log: %w", err)
	}

	cmd := exec.Command(cfg.binary, "--addr", cfg.addr, "--db", filepath.Join(cfg.home, "factory.db"), "--web-dir", cfg.webDir)
	cmd.Stdout = logFile
	cmd.Stderr = logFile
	cmd.Env = environmentWithTokens(token, workerToken)
	cmd.SysProcAttr = &syscall.SysProcAttr{Setsid: true}
	if err := cmd.Start(); err != nil {
		_ = logFile.Close()
		return fmt.Errorf("start Factory Core: %w", err)
	}
	_ = logFile.Close()

	state := serviceState{PID: cmd.Process.Pid, Addr: cfg.addr, Executable: cfg.binary}
	if err := writePrivateJSON(statePath, state); err != nil {
		_ = cmd.Process.Signal(syscall.SIGTERM)
		_ = cmd.Wait()
		return fmt.Errorf("record Factory PID: %w", err)
	}

	childDone := make(chan error, 1)
	go func() {
		childDone <- cmd.Wait()
	}()
	deadline := time.Now().Add(startTimeout)
	for time.Now().Before(deadline) {
		if healthy(cfg.addr, filepath.Join(cfg.home, "api.token")) {
			printInfo(cfg, state.PID, "ready")
			return nil
		}
		select {
		case childErr := <-childDone:
			_ = os.Remove(statePath)
			return fmt.Errorf("factory Core exited before readiness: %v (inspect %s)", childErr, logPath)
		case <-time.After(100 * time.Millisecond):
		}
	}
	_ = cmd.Process.Signal(syscall.SIGTERM)
	select {
	case <-childDone:
	case <-time.After(2 * time.Second):
		_ = cmd.Process.Kill()
		<-childDone
	}
	_ = os.Remove(statePath)
	return fmt.Errorf("factory Core did not become ready within %s (inspect %s)", startTimeout, logPath)
}

func stop(cfg config) error {
	if _, err := os.Stat(cfg.home); errors.Is(err, os.ErrNotExist) {
		fmt.Println("Factory status: stopped")
		return nil
	}
	lock, err := acquireLock(cfg.home)
	if err != nil {
		return err
	}
	defer releaseLock(lock)

	statePath := filepath.Join(cfg.home, "factory.pid")
	state, err := readState(statePath)
	if errors.Is(err, os.ErrNotExist) {
		fmt.Println("Factory status: stopped")
		return nil
	}
	if err != nil {
		return err
	}
	if !processAlive(state.PID) {
		_ = os.Remove(statePath)
		fmt.Println("Factory status: stopped (stale PID file removed)")
		return nil
	}
	if !matchesCore(state) {
		return fmt.Errorf("PID %d is alive but is not the recorded Factory Core; refusing to signal it", state.PID)
	}
	proc, err := os.FindProcess(state.PID)
	if err != nil {
		return fmt.Errorf("find Factory Core PID %d: %w", state.PID, err)
	}
	if err := proc.Signal(syscall.SIGTERM); err != nil && !errors.Is(err, os.ErrProcessDone) {
		return fmt.Errorf("stop Factory Core PID %d: %w", state.PID, err)
	}
	deadline := time.Now().Add(stopTimeout)
	for time.Now().Before(deadline) {
		if !processAlive(state.PID) {
			_ = os.Remove(statePath)
			fmt.Println("Factory status: stopped")
			return nil
		}
		time.Sleep(100 * time.Millisecond)
	}
	_ = proc.Kill()
	deadline = time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) && processAlive(state.PID) {
		time.Sleep(100 * time.Millisecond)
	}
	if processAlive(state.PID) {
		return fmt.Errorf("factory Core PID %d did not exit after SIGKILL", state.PID)
	}
	_ = os.Remove(statePath)
	fmt.Println("Factory status: stopped (SIGKILL fallback used)")
	return nil
}

func status(cfg config) error {
	state, err := readState(filepath.Join(cfg.home, "factory.pid"))
	if errors.Is(err, os.ErrNotExist) {
		printInfo(cfg, 0, "stopped")
		return nil
	}
	if err != nil {
		return err
	}
	if !processAlive(state.PID) {
		_ = os.Remove(filepath.Join(cfg.home, "factory.pid"))
		printInfo(cfg, 0, "stopped (stale PID file removed)")
		return nil
	}
	if !matchesCore(state) {
		return fmt.Errorf("PID %d is alive but is not the recorded Factory Core", state.PID)
	}
	if !healthy(state.Addr, filepath.Join(cfg.home, "api.token")) {
		printInfo(cfg, state.PID, "running but not ready")
		return errors.New("factory Core readiness check failed")
	}
	printInfo(cfg, state.PID, "running")
	return nil
}

func processAlive(pid int) bool {
	if pid <= 0 {
		return false
	}
	proc, err := os.FindProcess(pid)
	if err != nil {
		return false
	}
	return proc.Signal(syscall.Signal(0)) == nil
}

func matchesCore(state serviceState) bool {
	if !processAlive(state.PID) {
		return false
	}
	expected, err := filepath.EvalSymlinks(state.Executable)
	if err != nil {
		expected = filepath.Clean(state.Executable)
	}
	actual, err := os.Readlink("/proc/" + strconv.Itoa(state.PID) + "/exe")
	if err == nil {
		actual = strings.TrimSuffix(actual, " (deleted)")
		return filepath.Clean(actual) == filepath.Clean(expected)
	}
	return false
}

func healthy(addr, tokenPath string) bool {
	tokenData, err := os.ReadFile(tokenPath)
	if err != nil || strings.TrimSpace(string(tokenData)) == "" {
		return false
	}
	url := "http://" + net.JoinHostPort(hostForURL(addr), portForURL(addr)) + "/api/v1/health"
	client := http.Client{Timeout: 500 * time.Millisecond}
	req, err := http.NewRequest(http.MethodGet, url, nil)
	if err != nil {
		return false
	}
	req.Header.Set("Authorization", "Bearer "+strings.TrimSpace(string(tokenData)))
	resp, err := client.Do(req)
	if err != nil {
		return false
	}
	_ = resp.Body.Close()
	return resp.StatusCode == http.StatusOK
}

func hostForURL(addr string) string {
	host, _, err := net.SplitHostPort(addr)
	if err != nil || host == "" || host == "0.0.0.0" {
		return "127.0.0.1"
	}
	return host
}

func portForURL(addr string) string {
	_, port, err := net.SplitHostPort(addr)
	if err != nil {
		return "8471"
	}
	return port
}

func printInfo(cfg config, pid int, state string) {
	fmt.Printf("Factory status: %s", state)
	if pid > 0 {
		fmt.Printf(" (PID %d)", pid)
	}
	fmt.Printf("\nOffice URL: %s/\n", officeURL(cfg.addr))
	fmt.Printf("Authentication: Bearer token in %s (permissions 0600; value not written to logs)\n", filepath.Join(cfg.home, "api.token"))
	fmt.Printf("Worker credential: scoped token in %s (permissions 0600; never reuse the operator token)\n", filepath.Join(cfg.home, "worker.token"))
	fmt.Printf("Database: %s\n", filepath.Join(cfg.home, "factory.db"))
	fmt.Printf("Logs: %s\n", filepath.Join(cfg.home, "factory.log"))
}

func officeURL(addr string) string {
	return "http://" + net.JoinHostPort(hostForURL(addr), portForURL(addr))
}

func environmentWithTokens(apiToken, workerToken string) []string {
	env := make([]string, 0, len(os.Environ())+2)
	for _, item := range os.Environ() {
		if !strings.HasPrefix(item, "FACTORY_API_TOKEN=") && !strings.HasPrefix(item, "FACTORY_WORKER_TOKEN=") {
			env = append(env, item)
		}
	}
	return append(env, "FACTORY_API_TOKEN="+apiToken, "FACTORY_WORKER_TOKEN="+workerToken)
}
