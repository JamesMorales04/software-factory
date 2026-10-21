//go:build !linux

package browserserver

import (
	"errors"
	"os/exec"
)

func configureProcess(_ *exec.Cmd) {}
func processGroup(cmd *exec.Cmd) int {
	if cmd.Process == nil {
		return 0
	}
	return cmd.Process.Pid
}
func processAlive(int) (bool, string)     { return false, "" }
func processCwd(int) (string, error)      { return "", errors.New("process cwd probe unsupported") }
func groupMemberPIDs(int) []int           { return nil }
func withinWorkspace(string, string) bool { return false }
func killProcessGroup(int) error          { return errors.New("managed test server is only supported on Linux") }
func forceKillProcessGroup(pid int) error { return killProcessGroup(pid) }
