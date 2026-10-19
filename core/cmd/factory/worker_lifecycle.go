package main

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"time"
)

// Local Worker lifecycle: start/stop/status for the single S2 Worker
// process. The Worker executes only the credential-free synthetic fixture
// and is managed separately from the Core so operators never launch
// execution accidentally.

func workerBinary(cfg config) string {
	if explicit := os.Getenv("FACTORY_WORKER_BIN"); explicit != "" {
		return explicit
	}
	return filepath.Join(filepath.Dir(cfg.binary), "factory-worker")
}

func startWorker(cfg config) error {
	if err := ensurePrivateDir(cfg.home); err != nil {
		return err
	}
	lock, err := acquireLock(cfg.home)
	if err != nil {
		return err
	}
	defer releaseLock(lock)

	statePath := filepath.Join(cfg.home, "worker.pid")
	if state, err := readState(statePath); err == nil {
		if processAlive(state.PID) {
			if !matchesWorker(state) {
				return fmt.Errorf("PID %d is alive but is not the recorded Factory Worker; refusing to signal it", state.PID)
			}
			fmt.Printf("Factory worker: already running (PID %d)\n", state.PID)
			return nil
		}
		_ = os.Remove(statePath)
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}

	workerBin := workerBinary(cfg)
	if _, err := os.Stat(workerBin); err != nil {
		return fmt.Errorf("factory Worker executable not found at %s (build the local distribution first)", workerBin)
	}
	workerToken, err := loadOrCreateTokenEnv(filepath.Join(cfg.home, "worker.token"), "FACTORY_WORKER_TOKEN")
	if err != nil {
		return err
	}
	logPath := filepath.Join(cfg.home, "worker.log")
	logFile, err := os.OpenFile(logPath, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
	if err != nil {
		return fmt.Errorf("open Factory worker log: %w", err)
	}
	if err := logFile.Chmod(0o600); err != nil {
		_ = logFile.Close()
		return fmt.Errorf("secure Factory worker log: %w", err)
	}

	cmd := exec.Command(workerBin, "--core", officeURL(cfg.addr))
	cmd.Stdout = logFile
	cmd.Stderr = logFile
	cmd.Env = workerEnvironment(workerToken)
	cmd.SysProcAttr = &syscall.SysProcAttr{Setsid: true}
	if err := cmd.Start(); err != nil {
		_ = logFile.Close()
		return fmt.Errorf("start Factory Worker: %w", err)
	}
	_ = logFile.Close()

	state := serviceState{PID: cmd.Process.Pid, Addr: officeURL(cfg.addr), Executable: workerBin}
	if err := writePrivateJSON(statePath, state); err != nil {
		_ = cmd.Process.Signal(syscall.SIGTERM)
		_ = cmd.Wait()
		return fmt.Errorf("record Factory Worker PID: %w", err)
	}
	fmt.Printf("Factory worker: started (PID %d)\nWorker logs: %s\n", state.PID, logPath)
	return nil
}

func workerEnvironment(workerToken string) []string {
	env := make([]string, 0, len(os.Environ())+1)
	for _, item := range os.Environ() {
		if strings.HasPrefix(item, "FACTORY_WORKER_TOKEN=") {
			continue
		}
		env = append(env, item)
	}
	return append(env, "FACTORY_WORKER_TOKEN="+workerToken)
}

func stopWorker(cfg config) error {
	if _, err := os.Stat(cfg.home); errors.Is(err, os.ErrNotExist) {
		fmt.Println("Factory worker status: stopped")
		return nil
	}
	lock, err := acquireLock(cfg.home)
	if err != nil {
		return err
	}
	defer releaseLock(lock)

	statePath := filepath.Join(cfg.home, "worker.pid")
	state, err := readState(statePath)
	if errors.Is(err, os.ErrNotExist) {
		fmt.Println("Factory worker status: stopped")
		return nil
	}
	if err != nil {
		return err
	}
	if !processAlive(state.PID) {
		_ = os.Remove(statePath)
		fmt.Println("Factory worker status: stopped (stale PID file removed)")
		return nil
	}
	if !matchesWorker(state) {
		return fmt.Errorf("PID %d is alive but is not the recorded Factory Worker; refusing to signal it", state.PID)
	}
	return stopPID(state.PID, statePath, "Factory worker")
}

func stopPID(pid int, statePath, label string) error {
	proc, err := os.FindProcess(pid)
	if err != nil {
		return fmt.Errorf("find %s PID %d: %w", label, pid, err)
	}
	if err := proc.Signal(syscall.SIGTERM); err != nil && !errors.Is(err, os.ErrProcessDone) {
		return fmt.Errorf("stop %s PID %d: %w", label, pid, err)
	}
	deadline := time.Now().Add(stopTimeout)
	for time.Now().Before(deadline) {
		if !processAlive(pid) {
			_ = os.Remove(statePath)
			fmt.Printf("%s status: stopped\n", label)
			return nil
		}
		time.Sleep(100 * time.Millisecond)
	}
	_ = proc.Kill()
	deadline = time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) && processAlive(pid) {
		time.Sleep(100 * time.Millisecond)
	}
	if processAlive(pid) {
		return fmt.Errorf("%s PID %d did not exit after SIGKILL", label, pid)
	}
	_ = os.Remove(statePath)
	fmt.Printf("%s status: stopped (SIGKILL fallback used)\n", label)
	return nil
}

func statusWorker(cfg config) error {
	state, err := readState(filepath.Join(cfg.home, "worker.pid"))
	if errors.Is(err, os.ErrNotExist) {
		fmt.Println("Factory worker status: stopped")
		return nil
	}
	if err != nil {
		return err
	}
	if !processAlive(state.PID) {
		_ = os.Remove(filepath.Join(cfg.home, "worker.pid"))
		fmt.Println("Factory worker status: stopped (stale PID file removed)")
		return nil
	}
	if !matchesWorker(state) {
		return fmt.Errorf("PID %d is alive but is not the recorded Factory Worker", state.PID)
	}
	fmt.Printf("Factory worker status: running (PID %d)\n", state.PID)
	return nil
}

func matchesWorker(state serviceState) bool {
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
