//go:build !unix

package exec

import (
	"errors"
	"os"
)

// flockNonblock is unavailable without Unix advisory locks. The Worker is
// a Linux-first process (rootless Podman, mount namespaces); rather than
// silently running without the ownership signal the reaper depends on,
// workspace ownership fails closed on unsupported platforms.
func flockNonblock(_ *os.File) error {
	return errors.New("advisory file locks require a Unix platform")
}
