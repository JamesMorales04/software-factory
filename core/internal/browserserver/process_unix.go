//go:build linux

package browserserver

import (
	"errors"
	"os"
	"os/exec"
	"strconv"
	"strings"
	"syscall"

	"golang.org/x/sys/unix"
)

func Supported() error { return nil }

func configureProcess(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
}

func processGroup(cmd *exec.Cmd) int {
	if cmd.Process == nil {
		return 0
	}
	if pgid, err := syscall.Getpgid(cmd.Process.Pid); err == nil {
		return pgid
	}
	return cmd.Process.Pid
}

func processAlive(pid int) (bool, string) {
	if pid <= 0 {
		return false, ""
	}
	if err := syscall.Kill(pid, 0); err != nil {
		return false, ""
	}
	start, _, err := processIdentity(pid)
	if err != nil {
		return false, ""
	}
	return true, start
}

func processIdentity(pid int) (string, string, error) {
	raw, err := os.ReadFile("/proc/" + strconv.Itoa(pid) + "/stat")
	if err != nil {
		return "", "", err
	}
	closeParen := strings.LastIndex(string(raw), ")")
	if closeParen < 0 {
		return "", "", os.ErrInvalid
	}
	fields := strings.Fields(string(raw[closeParen+1:]))
	if len(fields) <= 19 {
		return "", "", os.ErrInvalid
	}
	cwd, err := os.Readlink("/proc/" + strconv.Itoa(pid) + "/cwd")
	if err != nil {
		return "", "", err
	}
	return fields[19], strings.TrimSuffix(cwd, " (deleted)"), nil
}

func processCwd(pid int) (string, error) {
	_, cwd, err := processIdentity(pid)
	return cwd, err
}

func groupMembers(pgid int) ([]processMember, error) {
	entries, err := os.ReadDir("/proc")
	if err != nil {
		return nil, err
	}
	var out []processMember
	for _, entry := range entries {
		pid, err := strconv.Atoi(entry.Name())
		if err != nil || pid <= 0 {
			continue
		}
		start, err := processStartTime(pid)
		if err != nil {
			continue
		}
		raw, err := os.ReadFile("/proc/" + strconv.Itoa(pid) + "/stat")
		if err != nil {
			continue
		}
		closeParen := strings.LastIndex(string(raw), ")")
		if closeParen < 0 {
			continue
		}
		fields := strings.Fields(string(raw[closeParen+1:]))
		if len(fields) <= 2 {
			continue
		}
		memberGroup, err := strconv.Atoi(fields[2])
		if err != nil || memberGroup != pgid {
			continue
		}
		out = append(out, processMember{PID: pid, StartTime: start})
	}
	return out, nil
}

func processStartTime(pid int) (string, error) {
	start, _, err := processIdentity(pid)
	return start, err
}

func openProcessHandle(pid int) (int, error) {
	return unix.PidfdOpen(pid, 0)
}

func signalProcessHandle(fd, sig int) error {
	//nolint:gosec // sig is a fixed SIGTERM/SIGKILL selected by the caller.
	return unix.PidfdSendSignal(fd, unix.Signal(sig), nil, 0)
}

func closeProcessHandle(fd int) { _ = unix.Close(fd) }

func withinWorkspace(cwd, workDir string) bool {
	cwd = strings.TrimSuffix(cwd, " (deleted)")
	return cwd == workDir || strings.HasPrefix(cwd, workDir+"/")
}

func dirOwnedByCurrentUser(info os.FileInfo) bool {
	stat, ok := info.Sys().(*syscall.Stat_t)
	return ok && stat.Uid == uint32(os.Geteuid())
}

func acquireRegistryLock(path string) (func(), error) {
	file, err := os.OpenFile(path, os.O_CREATE|os.O_RDWR, 0o600)
	if err != nil {
		return nil, err
	}
	if err := unix.Flock(int(file.Fd()), unix.LOCK_EX|unix.LOCK_NB); err != nil {
		_ = file.Close()
		return nil, errors.New("another Worker with this ID owns the test-server registry")
	}
	return func() {
		_ = unix.Flock(int(file.Fd()), unix.LOCK_UN)
		_ = file.Close()
	}, nil
}
