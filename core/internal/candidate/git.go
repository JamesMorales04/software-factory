// Package candidate validates immutable git candidate identities. It is the
// only place that shells out to git for quality work: Worker claims are
// reduced to a commit id, and the Core recomputes tree/content/scope facts
// from the authorized repository.
package candidate

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
	"regexp"
	"sort"
	"strings"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/quality"
)

// Bounds for git evidence collection.
const (
	gitTimeout      = 30 * time.Second
	maxDiffBytes    = 512 * 1024
	maxChangedPaths = 512
)

var candidateRefPattern = regexp.MustCompile(`^refs/factory/candidates/[A-Za-z0-9][A-Za-z0-9._/-]{0,220}$`)

// gitEnv is the explicit minimal environment for git evidence commands.
// No provider credentials, daemon sockets or operator secrets are exposed.
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
		// Git stderr can include filesystem paths but never source bodies.
		return nil, fmt.Errorf("git %s: %s", strings.Join(args, " "), bounded(stderr.String(), 240))
	}
	return stdout.Bytes(), nil
}

// ValidateRepository verifies that path is a local git repository whose
// base revision resolves to a commit. Path shape is a domain invariant.
func ValidateRepository(ctx context.Context, repository, baseRevision string) error {
	info, err := os.Stat(repository)
	if err != nil {
		return fmt.Errorf("repository unavailable: %w", err)
	}
	if !info.IsDir() {
		return errors.New("repository must be a directory")
	}
	out, err := runGit(ctx, repository, "rev-parse", "--verify", baseRevision+"^{commit}")
	if err != nil {
		return fmt.Errorf("base revision is not a commit in the repository: %w", err)
	}
	if strings.TrimSpace(string(out)) != baseRevision {
		return errors.New("base revision did not resolve exactly")
	}
	return nil
}

// InspectCommit independently derives the candidate identity from the
// authorized repository. It verifies the exact parent, the factory ref,
// nonempty diff, in-scope paths and the content digest. Any mismatch is an
// error; no Worker-provided digest is trusted.
func InspectCommit(ctx context.Context, repository, baseRevision, commitSHA, candidateRef string, allowedPaths []string) (domain.CandidateValidation, error) {
	if _, err := runGit(ctx, repository, "cat-file", "-t", commitSHA); err != nil {
		return domain.CandidateValidation{}, fmt.Errorf("candidate commit is not present in the repository: %w", err)
	}
	kind, err := runGit(ctx, repository, "cat-file", "-t", commitSHA)
	if err != nil || strings.TrimSpace(string(kind)) != "commit" {
		return domain.CandidateValidation{}, errors.New("candidate object is not a commit")
	}
	if err := verifyRef(ctx, repository, candidateRef, commitSHA); err != nil {
		return domain.CandidateValidation{}, err
	}
	parents, err := runGit(ctx, repository, "rev-list", "--parents", "-n", "1", commitSHA)
	if err != nil {
		return domain.CandidateValidation{}, err
	}
	fields := strings.Fields(string(parents))
	if len(fields) != 2 || fields[0] != commitSHA || fields[1] != baseRevision {
		return domain.CandidateValidation{}, errors.New("candidate commit must have exactly the frozen base revision as its parent")
	}
	treeRaw, err := runGit(ctx, repository, "rev-parse", "--verify", commitSHA+"^{tree}")
	if err != nil {
		return domain.CandidateValidation{}, err
	}
	treeSHA := strings.TrimSpace(string(treeRaw))
	if !isObjectID(treeSHA) {
		return domain.CandidateValidation{}, errors.New("candidate tree identity is malformed")
	}
	pathsRaw, err := runGit(ctx, repository, "diff", "--name-only", "--no-renames", baseRevision, commitSHA)
	if err != nil {
		return domain.CandidateValidation{}, err
	}
	changed := domain.SortedChangedPaths(strings.Split(strings.TrimSpace(string(pathsRaw)), "\n"))
	if len(changed) == 0 {
		return domain.CandidateValidation{}, errors.New("candidate has no source change")
	}
	if len(changed) > maxChangedPaths {
		return domain.CandidateValidation{}, errors.New("candidate changes too many paths")
	}
	for _, changedPath := range changed {
		if strings.Contains(changedPath, "..") || strings.HasPrefix(changedPath, "/") {
			return domain.CandidateValidation{}, fmt.Errorf("candidate changed an unsafe path %q", changedPath)
		}
		if !domain.PathAllowed(changedPath, allowedPaths) {
			return domain.CandidateValidation{}, fmt.Errorf("candidate changed path %q outside the authorized scope", changedPath)
		}
	}
	digest, err := ContentDigest(ctx, repository, commitSHA)
	if err != nil {
		return domain.CandidateValidation{}, err
	}
	risk := quality.ClassifyChange(changed)
	return domain.CandidateValidation{
		CommitSHA:     commitSHA,
		TreeSHA:       treeSHA,
		ContentDigest: digest,
		ChangedPaths:  changed,
		RiskClass:     domain.RiskClass(risk.RiskClass),
	}, nil
}

// ContentDigest hashes the exact recursive tree listing of a commit. It is
// stable across clones and independent of the checkout filesystem.
func ContentDigest(ctx context.Context, repository, commitSHA string) (string, error) {
	out, err := runGit(ctx, repository, "ls-tree", "-r", "--full-tree", commitSHA)
	if err != nil {
		return "", err
	}
	sum := sha256.Sum256(out)
	return hex.EncodeToString(sum[:]), nil
}

func verifyRef(ctx context.Context, repository, candidateRef, commitSHA string) error {
	if !candidateRefPattern.MatchString(candidateRef) || strings.Contains(candidateRef, "..") {
		return errors.New("candidateRef has an unsupported shape")
	}
	raw, err := runGit(ctx, repository, "rev-parse", "--verify", candidateRef+"^{commit}")
	if err != nil {
		return fmt.Errorf("candidate ref is not present: %w", err)
	}
	if strings.TrimSpace(string(raw)) != commitSHA {
		return errors.New("candidate ref does not point at the frozen commit")
	}
	return nil
}

// CloneAt creates a clean checkout of exactly commitSHA and verifies the
// tree/content identity against the Core-recorded candidate. The returned
// directory contains only the candidate worktree plus its .git metadata.
func CloneAt(ctx context.Context, repository, commitSHA, treeSHA, contentDigest, dest string) error {
	if err := os.MkdirAll(dest, 0o750); err != nil {
		return fmt.Errorf("create checkout parent: %w", err)
	}
	cloneDir := filepath.Join(dest, "candidate")
	ctx, cancel := context.WithTimeout(ctx, gitTimeout)
	defer cancel()
	cmd := exec.CommandContext(ctx, "git", "clone", "--quiet", "--no-checkout", repository, cloneDir)
	cmd.Env = gitEnv()
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("clone candidate: %s", bounded(stderr.String(), 240))
	}
	if _, err := runGit(ctx, cloneDir, "checkout", "--quiet", "--detach", commitSHA); err != nil {
		return fmt.Errorf("checkout candidate: %w", err)
	}
	return VerifyCheckout(ctx, cloneDir, commitSHA, treeSHA, contentDigest)
}

// VerifyCheckout proves a checkout still represents the exact candidate.
func VerifyCheckout(ctx context.Context, checkout, commitSHA, treeSHA, contentDigest string) error {
	head, err := runGit(ctx, checkout, "rev-parse", "HEAD")
	if err != nil {
		return err
	}
	if strings.TrimSpace(string(head)) != commitSHA {
		return errors.New("checkout HEAD does not match the candidate commit")
	}
	tree, err := runGit(ctx, checkout, "rev-parse", "--verify", "HEAD^{tree}")
	if err != nil {
		return err
	}
	if strings.TrimSpace(string(tree)) != treeSHA {
		return errors.New("checkout tree does not match the candidate tree")
	}
	digest, err := ContentDigest(ctx, checkout, commitSHA)
	if err != nil {
		return err
	}
	if digest != contentDigest {
		return errors.New("checkout content digest does not match the candidate digest")
	}
	return nil
}

// MarkReadOnly makes the candidate worktree non-writable (excluding .git,
// which checkout verification may still inspect). Tests may write only to
// their own temporary directories.
func MarkReadOnly(checkout string) error {
	return filepath.WalkDir(checkout, func(path string, entry os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(checkout, path)
		if err != nil {
			return err
		}
		if rel == ".git" || strings.HasPrefix(rel, ".git"+string(filepath.Separator)) {
			return nil
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		mode := os.FileMode(0o444)
		if info.IsDir() {
			mode = 0o555
		} else if info.Mode()&0o111 != 0 {
			// Preserve the executable bit so read-only protection does not
			// appear as a candidate modification.
			mode = 0o555
		}
		return os.Chmod(path, mode)
	})
}

// RestoreWritable returns a read-only checkout to writable permissions so
// the disposable copy can be removed without leaking temporary storage.
func RestoreWritable(checkout string) {
	_ = filepath.WalkDir(checkout, func(path string, entry os.DirEntry, err error) error {
		if err != nil {
			return nil
		}
		info, ierr := entry.Info()
		if ierr != nil {
			return nil
		}
		if info.IsDir() {
			_ = os.Chmod(path, 0o755)
			return nil
		}
		_ = os.Chmod(path, 0o644)
		return nil
	})
}

// ChangedSince reports tracked worktree modifications, used after checks to
// prove the verifier did not mutate the candidate. Untracked scratch files
// are reported separately and do not change the candidate identity.
func ChangedSince(ctx context.Context, checkout string) (modified []string, untracked []string, err error) {
	out, err := runGit(ctx, checkout, "status", "--porcelain", "--untracked-files=normal")
	if err != nil {
		return nil, nil, err
	}
	for _, line := range strings.Split(strings.TrimSpace(string(out)), "\n") {
		if strings.TrimSpace(line) == "" {
			continue
		}
		if strings.HasPrefix(line, "??") {
			untracked = append(untracked, strings.TrimSpace(strings.TrimPrefix(line, "??")))
			continue
		}
		modified = append(modified, strings.TrimSpace(line))
	}
	return modified, untracked, nil
}

// Diff returns a bounded unified diff between the frozen base and commit.
func Diff(ctx context.Context, repository, baseRevision, commitSHA string) (string, int, error) {
	out, err := runGit(ctx, repository, "diff", "--no-ext-diff", "--no-renames", baseRevision, commitSHA)
	if err != nil {
		return "", 0, err
	}
	if len(out) > maxDiffBytes {
		return "", len(out), fmt.Errorf("candidate diff exceeds %d bytes", maxDiffBytes)
	}
	return string(out), len(out), nil
}

// DiffStat returns a bounded git diff --stat line.
func DiffStat(ctx context.Context, repository, baseRevision, commitSHA string) (string, error) {
	out, err := runGit(ctx, repository, "diff", "--stat", "--no-renames", baseRevision, commitSHA)
	if err != nil {
		return "", err
	}
	lines := strings.Split(strings.TrimSpace(string(out)), "\n")
	if len(lines) == 0 {
		return "", nil
	}
	return bounded(lines[len(lines)-1], 512), nil
}

// AddedLines extracts added diff lines for reviewer static checks. The diff
// itself is never persisted by the Factory; only bounded findings are.
func AddedLines(diff string) []string {
	var lines []string
	for _, line := range strings.Split(diff, "\n") {
		if strings.HasPrefix(line, "+++") || strings.HasPrefix(line, "---") {
			continue
		}
		if strings.HasPrefix(line, "+") {
			lines = append(lines, strings.TrimPrefix(line, "+"))
		}
	}
	return lines
}

// SortedPaths returns a stable copy of a path list.
func SortedPaths(paths []string) []string {
	out := append([]string(nil), paths...)
	sort.Strings(out)
	return out
}

func isObjectID(value string) bool {
	if len(value) != 40 && len(value) != 64 {
		return false
	}
	for _, r := range value {
		if r >= '0' && r <= '9' || r >= 'a' && r <= 'f' {
			continue
		}
		return false
	}
	return true
}

func bounded(value string, limit int) string {
	value = strings.TrimSpace(value)
	if len(value) > limit {
		return value[:limit]
	}
	return value
}
