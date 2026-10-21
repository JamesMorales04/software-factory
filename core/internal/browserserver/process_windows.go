//go:build windows

package browserserver

import (
	"os"
	"os/exec"
)

func configureProcess(_ *exec.Cmd) {}
func processGroup(cmd *exec.Cmd) int {
	if cmd.Process == nil {
		return 0
	}
	return cmd.Process.Pid
}

func processAlive(pid int) (bool, string) {
	p, err := os.FindProcess(pid)
	if err != nil {
		return false, ""
	}
	return p.Signal(os.Signal(nil)) == nil, ""
}

func killProcessGroup(pid int) error {
	p, err := os.FindProcess(pid)
	if err != nil {
		return err
	}
	return p.Kill()
}
func forceKillProcessGroup(pid int) error { return killProcessGroup(pid) }
