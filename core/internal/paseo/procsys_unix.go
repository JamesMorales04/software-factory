//go:build unix

package paseo

import (
	"os"
	"os/exec"
	"syscall"
)

// isolateChild puts the subprocess in its own process group so
// cancellation can reap the whole tree. Provider CLIs, shells and their
// children inherit the captured stdio pipes; killing only the direct
// child leaves orphans holding those pipes, and Wait then blocks on EOF
// until the orphan exits — stalling the Worker slot long after the lease
// is gone. The group kill below closes that hole.
func isolateChild(cmd *exec.Cmd) {
	if cmd.SysProcAttr == nil {
		cmd.SysProcAttr = &syscall.SysProcAttr{}
	}
	cmd.SysProcAttr.Setpgid = true
}

// killGroup SIGKILLs the process group led by p. Negative PIDs address
// the group; Setpgid (see isolateChild) guarantees p is its leader.
func killGroup(p *os.Process) error {
	if p == nil || p.Pid <= 0 {
		return nil
	}
	return syscall.Kill(-p.Pid, syscall.SIGKILL)
}
