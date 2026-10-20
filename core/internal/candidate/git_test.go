package candidate

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func gitTestRepo(t *testing.T) (dir, base, commit string) {
	t.Helper()
	dir = t.TempDir()
	run := func(args ...string) string {
		t.Helper()
		cmd := exec.Command("git", append([]string{"-C", dir}, args...)...)
		cmd.Env = append(os.Environ(),
			"GIT_AUTHOR_NAME=test", "GIT_AUTHOR_EMAIL=test@example.test",
			"GIT_COMMITTER_NAME=test", "GIT_COMMITTER_EMAIL=test@example.test")
		out, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("git %s: %v: %s", strings.Join(args, " "), err, out)
		}
		return strings.TrimSpace(string(out))
	}
	run("init", "-q")
	if err := os.WriteFile(filepath.Join(dir, "add.js"), []byte("function add(a,b){return a+b}\nmodule.exports={add};\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	run("add", "add.js")
	run("commit", "-qm", "base")
	base = run("rev-parse", "HEAD")
	if err := os.WriteFile(filepath.Join(dir, "add.js"), []byte("function add(a,b){return a+b}\nfunction multiply(a,b){return a*b}\nmodule.exports={add,multiply};\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	run("add", "add.js")
	run("commit", "-qm", "candidate")
	commit = run("rev-parse", "HEAD")
	run("update-ref", "refs/factory/candidates/run-1", commit)
	return dir, base, commit
}

func TestInspectCommitDerivesIdentityAndScope(t *testing.T) {
	dir, base, commit := gitTestRepo(t)
	got, err := InspectCommit(context.Background(), dir, base, commit, "refs/factory/candidates/run-1", []string{"add.js"})
	if err != nil {
		t.Fatalf("InspectCommit: %v", err)
	}
	if got.CommitSHA != commit || got.TreeSHA == "" || got.ContentDigest == "" {
		t.Fatalf("incomplete identity: %+v", got)
	}
	if len(got.ChangedPaths) != 1 || got.ChangedPaths[0] != "add.js" {
		t.Fatalf("changed paths = %v", got.ChangedPaths)
	}
	if got.RiskClass != "R0" {
		t.Fatalf("risk = %s, want R0", got.RiskClass)
	}
}

func TestInspectCommitRejectsScopeParentEmptyAndRefMismatch(t *testing.T) {
	dir, base, commit := gitTestRepo(t)
	ctx := context.Background()
	if _, err := InspectCommit(ctx, dir, base, commit, "refs/factory/candidates/run-1", []string{"other.js"}); err == nil {
		t.Fatal("out-of-scope change must be rejected")
	}
	if _, err := InspectCommit(ctx, dir, base, commit, "refs/factory/candidates/missing", []string{"add.js"}); err == nil {
		t.Fatal("missing candidate ref must be rejected")
	}
	if _, err := InspectCommit(ctx, dir, base, base, "refs/factory/candidates/run-1", []string{"add.js"}); err == nil {
		t.Fatal("a commit without the frozen base parent must be rejected")
	}
	cmd := exec.Command("git", "-C", dir, "commit", "--allow-empty", "-qm", "empty")
	cmd.Env = append(os.Environ(),
		"GIT_AUTHOR_NAME=test", "GIT_AUTHOR_EMAIL=test@example.test",
		"GIT_COMMITTER_NAME=test", "GIT_COMMITTER_EMAIL=test@example.test")
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("empty commit: %v %s", err, out)
	}
	empty := strings.TrimSpace(mustGit(t, dir, "rev-parse", "HEAD"))
	mustGit(t, dir, "update-ref", "refs/factory/candidates/empty", empty)
	if _, err := InspectCommit(ctx, dir, commit, empty, "refs/factory/candidates/empty", []string{"add.js"}); err == nil {
		t.Fatal("empty diff must be rejected")
	}
}

func TestCloneAtVerifiesContentAndMarksReadOnly(t *testing.T) {
	dir, base, commit := gitTestRepo(t)
	validation, err := InspectCommit(context.Background(), dir, base, commit, "refs/factory/candidates/run-1", []string{"add.js"})
	if err != nil {
		t.Fatal(err)
	}
	dest := t.TempDir()
	if err := CloneAt(context.Background(), dir, commit, validation.TreeSHA, validation.ContentDigest, dest); err != nil {
		t.Fatalf("CloneAt: %v", err)
	}
	checkout := filepath.Join(dest, "candidate")
	if err := MarkReadOnly(checkout); err != nil {
		t.Fatalf("MarkReadOnly: %v", err)
	}
	t.Cleanup(func() { RestoreWritable(dest) })
	info, err := os.Stat(filepath.Join(checkout, "add.js"))
	if err != nil {
		t.Fatal(err)
	}
	if info.Mode().Perm()&0o222 != 0 {
		t.Fatalf("candidate file is writable: %v", info.Mode())
	}
	if err := VerifyCheckout(context.Background(), checkout, commit, validation.TreeSHA, "00"); err == nil {
		t.Fatal("wrong digest must fail checkout verification")
	}
}

func mustGit(t *testing.T, dir string, args ...string) string {
	t.Helper()
	cmd := exec.Command("git", append([]string{"-C", dir}, args...)...)
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("git %s: %v %s", strings.Join(args, " "), err, out)
	}
	return string(out)
}
