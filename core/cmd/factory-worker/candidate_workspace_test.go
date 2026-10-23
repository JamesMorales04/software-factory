package main

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/JamesMorales04/software-factory/core/internal/candidate"
	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

func gitTest(t *testing.T, dir string, args ...string) string {
	t.Helper()
	base := []string{"-C", dir, "-c", "user.name=test", "-c", "user.email=test@localhost"}
	cmd := exec.Command("git", append(base, args...)...)
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("git %v: %v %s", args, err, out)
	}
	return strings.TrimSpace(string(out))
}

func writeFile(t *testing.T, dir, name, body string) {
	t.Helper()
	path := filepath.Join(dir, name)
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(body), 0o644); err != nil {
		t.Fatal(err)
	}
}

func candidateRepo(t *testing.T) (string, string, string, string) {
	t.Helper()
	repo := t.TempDir()
	gitTest(t, repo, "init", "-q", "-b", "main")
	writeFile(t, repo, "base.txt", "base\n")
	gitTest(t, repo, "add", ".")
	gitTest(t, repo, "commit", "-qm", "base")
	base := gitTest(t, repo, "rev-parse", "HEAD")
	gitTest(t, repo, "checkout", "-qb", "api")
	writeFile(t, repo, "src/api.txt", "api\n")
	gitTest(t, repo, "add", ".")
	gitTest(t, repo, "commit", "-qm", "api")
	api := gitTest(t, repo, "rev-parse", "HEAD")
	gitTest(t, repo, "checkout", "-q", "main")
	gitTest(t, repo, "checkout", "-qb", "ui")
	writeFile(t, repo, "src/ui.txt", "ui\n")
	gitTest(t, repo, "add", ".")
	gitTest(t, repo, "commit", "-qm", "ui")
	ui := gitTest(t, repo, "rev-parse", "HEAD")
	gitTest(t, repo, "update-ref", "refs/factory/candidates/api", api)
	gitTest(t, repo, "update-ref", "refs/factory/candidates/ui", ui)
	return repo, base, api, ui
}

func TestPrepareSpecWorkspaceSequentialAndIntegration(t *testing.T) {
	repo, base, api, ui := candidateRepo(t)
	spec := domain.ActionSpec{Repository: repo, BaseRevision: base}

	sequential := t.TempDir()
	run := domain.Run{WorkloadInput: map[string]any{"baseRevision": api}}
	if err := prepareSpecWorkspace(context.Background(), spec, run, sequential); err != nil {
		t.Fatalf("sequential workspace: %v", err)
	}
	if _, err := os.Stat(filepath.Join(sequential, "src/api.txt")); err != nil {
		t.Fatalf("sequential base did not include api candidate: %v", err)
	}
	if _, err := os.Stat(filepath.Join(sequential, "src/ui.txt")); err == nil {
		t.Fatal("sequential workspace unexpectedly included the independent ui candidate")
	}

	integrated := t.TempDir()
	run = domain.Run{WorkloadInput: map[string]any{
		"baseRevision":    base,
		"mergeCandidates": []any{api, ui},
	}}
	if err := prepareSpecWorkspace(context.Background(), spec, run, integrated); err != nil {
		t.Fatalf("integration workspace: %v", err)
	}
	for _, name := range []string{"src/api.txt", "src/ui.txt"} {
		if _, err := os.Stat(filepath.Join(integrated, name)); err != nil {
			t.Fatalf("integration workspace missing %s: %v", name, err)
		}
	}
	if head := gitTest(t, integrated, "rev-parse", "HEAD"); head != base {
		t.Fatalf("integration worktree HEAD = %s, want frozen base %s", head, base)
	}
	gitTest(t, integrated, "commit", "-qm", "integrated candidate")
	commit := gitTest(t, integrated, "rev-parse", "HEAD")
	gitTest(t, integrated, "push", "-q", "origin", "HEAD:refs/factory/candidates/integration")
	validation, err := candidate.InspectCommit(context.Background(), repo, base, commit,
		"refs/factory/candidates/integration", []string{"src/api.txt", "src/ui.txt"})
	if err != nil {
		t.Fatalf("Core rejected the integrated candidate lineage: %v", err)
	}
	if len(validation.ChangedPaths) != 2 {
		t.Fatalf("integrated candidate changed paths = %v", validation.ChangedPaths)
	}
}

func TestPrepareSpecWorkspaceRejectsIntegrationConflict(t *testing.T) {
	repo := t.TempDir()
	gitTest(t, repo, "init", "-q", "-b", "main")
	writeFile(t, repo, "shared.txt", "base\n")
	gitTest(t, repo, "add", ".")
	gitTest(t, repo, "commit", "-qm", "base")
	base := gitTest(t, repo, "rev-parse", "HEAD")
	gitTest(t, repo, "checkout", "-qb", "left")
	writeFile(t, repo, "shared.txt", "left\n")
	gitTest(t, repo, "commit", "-qam", "left")
	left := gitTest(t, repo, "rev-parse", "HEAD")
	gitTest(t, repo, "checkout", "-q", "main")
	gitTest(t, repo, "checkout", "-qb", "right")
	writeFile(t, repo, "shared.txt", "right\n")
	gitTest(t, repo, "commit", "-qam", "right")
	right := gitTest(t, repo, "rev-parse", "HEAD")
	gitTest(t, repo, "update-ref", "refs/factory/candidates/left", left)
	gitTest(t, repo, "update-ref", "refs/factory/candidates/right", right)

	run := domain.Run{WorkloadInput: map[string]any{
		"baseRevision":    base,
		"mergeCandidates": []any{left, right},
	}}
	err := prepareSpecWorkspace(context.Background(),
		domain.ActionSpec{Repository: repo, BaseRevision: base}, run, t.TempDir())
	if err == nil || !strings.Contains(err.Error(), "integrate dependency candidate") {
		t.Fatalf("expected integration conflict, got %v", err)
	}
}
