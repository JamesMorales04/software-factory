//go:build linux

package browserserver

import (
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
)

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
	raw, err := os.ReadFile("/proc/" + strconv.Itoa(pid) + "/stat")
	if err != nil {
		return false, ""
	}
	// The comm field is parenthesized and may contain spaces, so split only
	// after the final ')'. starttime is field 22 overall; it is index 19 of
	// the suffix beginning at state (field 3).
	closeParen := strings.LastIndex(string(raw), ")")
	if closeParen < 0 {
		return false, ""
	}
	fields := strings.Fields(string(raw[closeParen+1:]))
	if len(fields) <= 19 {
		return false, ""
	}
	return true, fields[19]
}

func processCwd(pid int) (string, error) {
	cwd, err := os.Readlink("/proc/" + strconv.Itoa(pid) + "/cwd")
	if err != nil {
		return "", err
	}
	return strings.TrimSuffix(cwd, " (deleted)"), nil
}

func groupMemberPIDs(pgid int) []int {
	entries, err := os.ReadDir("/proc")
	if err != nil {
		return nil
	}
	var out []int
	for _, entry := range entries {
		pid, err := strconv.Atoi(entry.Name())
		if err != nil || pid <= 0 {
			continue
		}
		member, err := processGroupOf(pid)
		if err != nil || member != pgid {
			continue
		}
		out = append(out, pid)
	}
	return out
}

func processGroupOf(pid int) (int, error) {
	raw, err := os.ReadFile("/proc/" + strconv.Itoa(pid) + "/stat")
	if err != nil {
		return 0, err
	}
	closeParen := strings.LastIndex(string(raw), ")")
	if closeParen < 0 {
		return 0, os.ErrInvalid
	}
	fields := strings.Fields(string(raw[closeParen+1:]))
	if len(fields) <= 2 {
		return 0, os.ErrInvalid
	}
	return strconv.Atoi(fields[2])
}

func withinWorkspace(cwd, workDir string) bool {
	cwd = filepath.Clean(cwd)
	workDir = filepath.Clean(workDir)
	return cwd == workDir || strings.HasPrefix(cwd, workDir+string(filepath.Separator))
}

func killProcessGroup(pid int) error {
	return syscall.Kill(-pid, syscall.SIGTERM)
}

func forceKillProcessGroup(pid int) error {
	return syscall.Kill(-pid, syscall.SIGKILL)
}
