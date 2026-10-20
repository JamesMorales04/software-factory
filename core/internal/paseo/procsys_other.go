//go:build !unix

package paseo

import (
	"os"
	"os/exec"
)

// isolateChild has no process-group primitive off Unix. The Worker is a
// Linux-first process; rather than pretending to isolate, the fallback
// kills the direct child only (see killGroup).
func isolateChild(_ *exec.Cmd) {}

// killGroup falls back to killing the direct child. Descendants holding
// stdio pipes can delay Wait past cancellation on such platforms; the
// WaitDelay bound still applies.
func killGroup(p *os.Process) error {
	if p == nil {
		return nil
	}
	return p.Kill()
}
