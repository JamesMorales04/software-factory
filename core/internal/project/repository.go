// Package project validates local Git repository identity for the Project
// Registry. Identity is derived from the repository's root commit set, not
// from a mutable path, so symlink aliases and relocated checkouts resolve
// to the same canonical identity while a different repository cannot.
package project

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

const gitTimeout = 30 * time.Second

// Repository is the Core-validated canonical repository identity and the
// exact tips of the authorized base refs at inspection time.
type Repository struct {
	Path     string
	Identity string
	Roots    []string
	BaseTips map[string]string
	IsBare   bool
}

func gitEnv() []string {
	env := []string{
		"PATH=" + os.Getenv("PATH"),
		"LC_ALL=C",
		"GIT_TERMINAL_PROMPT=0",
		"GIT_CONFIG_NOSYSTEM=1",
	}
	if home := os.Getenv("HOME"); home != "" {
		env = append(env, "HOME="+home)
	}
	return env
}

func runGit(ctx context.Context, dir string, args ...string) ([]byte, error) {
	ctx, cancel := context.WithTimeout(ctx, gitTimeout)
	defer cancel()
	cmd := exec.CommandContext(ctx, "git", append([]string{"-C", dir}, args...)...)
	cmd.Env = gitEnv()
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		return nil, fmt.Errorf("git %s: %s", strings.Join(args, " "), bounded(stderr.String(), 240))
	}
	return stdout.Bytes(), nil
}

// Inspect canonicalizes a local Git repository (working tree or bare),
// derives its stable identity and resolves every authorized base ref to an
// exact commit. Any failure rejects registration; there is no fallback.
func Inspect(ctx context.Context, repositoryPath string, baseRefs []string) (Repository, error) {
	if strings.TrimSpace(repositoryPath) == "" {
		return Repository{}, errors.New("repository path is required")
	}
	abs, err := filepath.Abs(repositoryPath)
	if err != nil {
		return Repository{}, errors.New("repository path is unresolvable")
	}
	canonical, err := filepath.EvalSymlinks(abs)
	if err != nil {
		return Repository{}, fmt.Errorf("repository path is not accessible: %w", err)
	}
	info, err := os.Stat(canonical)
	if err != nil || !info.IsDir() {
		return Repository{}, errors.New("repository must be a directory")
	}
	if _, err := runGit(ctx, canonical, "rev-parse", "--git-dir"); err != nil {
		return Repository{}, errors.New("path is not a local Git repository")
	}
	bareRaw, err := runGit(ctx, canonical, "rev-parse", "--is-bare-repository")
	if err != nil {
		return Repository{}, err
	}
	isBare := strings.TrimSpace(string(bareRaw)) == "true"
	rootsRaw, err := runGit(ctx, canonical, "rev-list", "--max-parents=0", "HEAD")
	if err != nil {
		return Repository{}, errors.New("repository must have a committed HEAD before registration")
	}
	roots := strings.Fields(string(rootsRaw))
	if len(roots) == 0 {
		return Repository{}, errors.New("repository has no root commit")
	}
	sort.Strings(roots)
	identity := sha256.Sum256([]byte("factory-git-identity-v2\n" + canonical + "\n" + strings.Join(roots, "\n")))
	tips := make(map[string]string, len(baseRefs))
	for _, ref := range baseRefs {
		commit, rerr := runGit(ctx, canonical, "rev-parse", "--verify", ref+"^{commit}")
		if rerr != nil {
			return Repository{}, fmt.Errorf("authorized base ref %q does not resolve to a commit", ref)
		}
		tips[ref] = strings.TrimSpace(string(commit))
	}
	return Repository{
		Path: canonical, Identity: hex.EncodeToString(identity[:]),
		Roots: roots, BaseTips: tips, IsBare: isBare,
	}, nil
}

// ValidateBase proves that baseRevision is exactly one authorized base-ref
// tip. Arbitrary ancestor revisions are not accepted as a base policy.
func ValidateBase(repo Repository, baseRevision string) error {
	for _, ref := range sortedKeys(repo.BaseTips) {
		if repo.BaseTips[ref] == baseRevision {
			return nil
		}
	}
	return fmt.Errorf("base revision %s is not the tip of an authorized base ref", baseRevision)
}

// RevalidateBound proves an existing frozen spec still references the same
// repository identity and an existing commit. Used before dispatch.
func RevalidateBound(ctx context.Context, repositoryPath, repositoryIdentity, baseRevision string) error {
	repo, err := Inspect(ctx, repositoryPath, nil)
	if err != nil {
		return err
	}
	if !strings.EqualFold(repo.Identity, repositoryIdentity) {
		return errors.New("repository identity differs from the frozen project binding")
	}
	if _, err := runGit(ctx, repo.Path, "cat-file", "-e", baseRevision+"^{commit}"); err != nil {
		return errors.New("frozen base revision is no longer present in the repository")
	}
	return nil
}

// CanonicalPath returns the symlink-resolved repository path.
func CanonicalPath(path string) (string, error) {
	abs, err := filepath.Abs(strings.TrimSpace(path))
	if err != nil {
		return "", err
	}
	return filepath.EvalSymlinks(abs)
}

func sortedKeys(values map[string]string) []string {
	keys := make([]string, 0, len(values))
	for key := range values {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	return keys
}

func bounded(value string, limit int) string {
	value = strings.TrimSpace(value)
	if len(value) > limit {
		return value[:limit]
	}
	return value
}
