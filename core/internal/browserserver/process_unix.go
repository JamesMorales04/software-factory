//go:build !windows

package browserserver

import (
	"os"
	"os/exec"
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

func killProcessGroup(pid int) error {
	return syscall.Kill(-pid, syscall.SIGTERM)
}

func forceKillProcessGroup(pid int) error {
	return syscall.Kill(-pid, syscall.SIGKILL)
}
