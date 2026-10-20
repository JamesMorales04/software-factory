//go:build unix

package exec

import (
	"os"
	"syscall"
)

// flockNonblock takes an exclusive advisory lock without blocking. It
// reports whether the lock is already held elsewhere; the kernel releases
// locks on process death, including SIGKILL, which is exactly the
// liveness property the run-root reaper depends on.
func flockNonblock(f *os.File) error {
	return syscall.Flock(int(f.Fd()), syscall.LOCK_EX|syscall.LOCK_NB)
}
