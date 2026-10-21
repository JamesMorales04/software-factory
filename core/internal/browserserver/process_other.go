//go:build !linux

package browserserver

import (
	"errors"
	"os"
	"os/exec"
)

var errManagedServerUnsupported = errors.New("managed test server is only supported on Linux")

func Supported() error             { return errManagedServerUnsupported }
func configureProcess(_ *exec.Cmd) {}
func processGroup(cmd *exec.Cmd) int {
	if cmd.Process == nil {
		return 0
	}
	return cmd.Process.Pid
}
func processAlive(int) (bool, string) { return false, "" }
func processIdentity(int) (string, string, error) {
	return "", "", errManagedServerUnsupported
}
func processCwd(int) (string, error) { return "", errManagedServerUnsupported }
func groupMembers(int) ([]processMember, error) {
	return nil, errManagedServerUnsupported
}
func processStartTime(int) (string, error) { return "", errManagedServerUnsupported }
func openProcessHandle(int) (int, error)   { return -1, errManagedServerUnsupported }
func signalProcessHandle(int, int) error   { return errManagedServerUnsupported }
func closeProcessHandle(int)               {}
func withinWorkspace(string, string) bool  { return false }

func dirOwnedByCurrentUser(os.FileInfo) bool { return false }
func acquireRegistryLock(string) (func(), error) {
	return nil, errManagedServerUnsupported
}
