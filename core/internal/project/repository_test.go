package project

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

func git(t *testing.T, dir string, args ...string) string {
	t.Helper()
	cmd := exec.Command("git", append([]string{"-C", dir}, args...)...)
	cmd.Env = append(os.Environ(), "GIT_TERMINAL_PROMPT=0")
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("git %v: %v (%s)", args, err, out)
	}
	return string(out)
}

func fixtureRepo(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	git(t, dir, "init", "-q", "-b", "main")
	if err := os.WriteFile(filepath.Join(dir, "file.txt"), []byte("one\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	git(t, dir, "add", ".")
	git(t, dir, "-c", "user.name=t", "-c", "user.email=t@example.test", "commit", "-qm", "one")
	return dir
}

func TestInspectIdentityIsPathIndependentAndBaseBound(t *testing.T) {
	dir := fixtureRepo(t)
	repo, err := Inspect(context.Background(), dir, []string{"main"})
	if err != nil {
		t.Fatal(err)
	}
	if len(repo.Identity) != 64 || repo.BaseTips["main"] == "" {
		t.Fatalf("bad identity: %+v", repo)
	}
	alias := filepath.Join(t.TempDir(), "alias")
	if err := os.Symlink(dir, alias); err != nil {
		t.Fatal(err)
	}
	aliased, err := Inspect(context.Background(), alias, []string{"main"})
	if err != nil {
		t.Fatal(err)
	}
	if aliased.Identity != repo.Identity || aliased.Path != repo.Path {
		t.Fatalf("symlink alias changed identity: %+v vs %+v", repo, aliased)
	}
	if err := ValidateBase(repo, repo.BaseTips["main"]); err != nil {
		t.Fatalf("authorized tip rejected: %v", err)
	}
	if err := ValidateBase(repo, "0000000000000000000000000000000000000000"); err == nil {
		t.Fatal("non-tip base accepted")
	}
	if err := RevalidateBound(context.Background(), dir, repo.Identity, repo.BaseTips["main"]); err != nil {
		t.Fatalf("bound repository rejected: %v", err)
	}
	if err := RevalidateBound(context.Background(), dir, repo.BaseTips["main"], repo.BaseTips["main"]); err == nil {
		t.Fatal("wrong identity accepted")
	}
}

func TestInspectRejectsUnrelatedRepository(t *testing.T) {
	first := fixtureRepo(t)
	second := fixtureRepo(t)
	a, err := Inspect(context.Background(), first, []string{"main"})
	if err != nil {
		t.Fatal(err)
	}
	b, err := Inspect(context.Background(), second, []string{"main"})
	if err != nil {
		t.Fatal(err)
	}
	if a.Identity == b.Identity {
		t.Fatal("unrelated repositories share an identity")
	}
	if _, err := Inspect(context.Background(), first, []string{"does-not-exist"}); err == nil {
		t.Fatal("unknown base ref accepted")
	}
	if _, err := Inspect(context.Background(), t.TempDir(), []string{"main"}); err == nil {
		t.Fatal("non-git directory accepted")
	}
}
