package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"os"
	"os/exec"
	"strings"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

// S4-A Implementer candidate finalization. The Worker prepares the
// authorized repository at its frozen base revision, lets Paseo edit the
// disposable checkout, commits the exact worktree and pushes only a
// factory candidate ref. Core independently revalidates the commit, tree,
// content digest and scope before freezing the CandidateRevision.
const candidatePushTimeout = 60 * time.Second

type candidateRegistration struct {
	WorkerID     string `json:"workerId"`
	RunID        string `json:"runId"`
	AttemptID    string `json:"attemptId"`
	Generation   int64  `json:"generation"`
	SpecID       string `json:"specId"`
	CommitSHA    string `json:"commitSha"`
	CandidateRef string `json:"candidateRef"`
}

// prepareSpecWorkspace clones the authorized repository at the frozen base
// revision into the disposable Attempt workspace.
func prepareSpecWorkspace(ctx context.Context, spec domain.ActionSpec, workDir string) error {
	ctx, cancel := context.WithTimeout(ctx, candidatePushTimeout)
	defer cancel()
	clone := exec.CommandContext(ctx, "git", "clone", "--quiet", spec.Repository, workDir)
	clone.Env = candidateGitEnv()
	var stderr bytes.Buffer
	clone.Stderr = &stderr
	if err := clone.Run(); err != nil {
		return fmt.Errorf("clone authorized repository: %s", truncate(strings.TrimSpace(stderr.String())))
	}
	if err := candidateGit(ctx, workDir, "checkout", "--quiet", "--detach", spec.BaseRevision); err != nil {
		return err
	}
	return nil
}

// finalizeCandidate commits the exact Paseo-modified worktree and pushes
// the immutable commit under a factory candidate ref. A nonempty source
// change is required; an empty diff is never reported as success.
func (c *client) finalizeCandidate(
	ctx context.Context,
	workerID string,
	run domain.Run,
	attempt domain.Attempt,
	workDir string,
	spec domain.ActionSpec,
	log *slog.Logger,
) (domain.CandidateRevision, error) {
	ctx, cancel := context.WithTimeout(ctx, candidatePushTimeout)
	defer cancel()
	if err := candidateGit(ctx, workDir, "add", "-A"); err != nil {
		return domain.CandidateRevision{}, err
	}
	changed, err := candidateGitOutput(ctx, workDir, "diff", "--cached", "--name-only", "--no-renames")
	if err != nil {
		return domain.CandidateRevision{}, err
	}
	if strings.TrimSpace(string(changed)) == "" {
		return domain.CandidateRevision{}, errors.New("implementer left no source diff; an empty candidate is not success")
	}
	message := fmt.Sprintf("factory candidate %s attempt %d", run.ID, attempt.Seq)
	if err := candidateGit(ctx, workDir,
		"-c", "user.name=factory-worker", "-c", "user.email=worker@factory.local",
		"commit", "--quiet", "-m", message); err != nil {
		return domain.CandidateRevision{}, err
	}
	commitRaw, err := candidateGitOutput(ctx, workDir, "rev-parse", "HEAD")
	if err != nil {
		return domain.CandidateRevision{}, err
	}
	commitSHA := strings.TrimSpace(string(commitRaw))
	ref := fmt.Sprintf("refs/factory/candidates/%s-%d", run.ID, attempt.Seq)
	if err := candidateGit(ctx, workDir, "push", "--quiet", "origin", "HEAD:"+ref); err != nil {
		return domain.CandidateRevision{}, fmt.Errorf("publish candidate ref: %w", err)
	}
	candidate, err := c.registerCandidate(ctx, candidateRegistration{
		WorkerID: workerID, RunID: run.ID, AttemptID: attempt.ID,
		Generation: attempt.LeaseGeneration, SpecID: spec.ID,
		CommitSHA: commitSHA, CandidateRef: ref,
	})
	if err != nil {
		return domain.CandidateRevision{}, err
	}
	log.Info("candidate frozen by Core",
		"candidateId", candidate.ID, "commitSha", candidate.CommitSHA,
		"riskClass", string(candidate.RiskClass), "changedPaths", len(candidate.ChangedPaths))
	return candidate, nil
}

func candidateGit(ctx context.Context, workDir string, args ...string) error {
	_, err := candidateGitOutput(ctx, workDir, args...)
	return err
}

func candidateGitOutput(ctx context.Context, workDir string, args ...string) ([]byte, error) {
	cmd := exec.CommandContext(ctx, "git", append([]string{"-C", workDir}, args...)...)
	cmd.Env = candidateGitEnv()
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		return nil, fmt.Errorf("git %s: %s", strings.Join(args, " "), truncate(strings.TrimSpace(stderr.String())))
	}
	return stdout.Bytes(), nil
}

func candidateGitEnv() []string {
	env := []string{
		"PATH=" + os.Getenv("PATH"),
		"LC_ALL=C",
		"GIT_TERMINAL_PROMPT=0",
	}
	if home := os.Getenv("HOME"); home != "" {
		env = append(env, "HOME="+home)
	}
	return env
}

func (c *client) getSpec(ctx context.Context, specID string) (domain.ActionSpec, error) {
	status, raw, _, err := c.do(ctx, http.MethodGet, "/api/v1/workers/specs/"+specID, nil)
	if err != nil {
		return domain.ActionSpec{}, err
	}
	if status != http.StatusOK {
		return domain.ActionSpec{}, fmt.Errorf("spec status %d: %s", status, truncate(string(raw)))
	}
	var out struct {
		Spec domain.ActionSpec `json:"spec"`
	}
	if err := json.Unmarshal(raw, &out); err != nil {
		return domain.ActionSpec{}, err
	}
	return out.Spec, nil
}

func (c *client) registerCandidate(ctx context.Context, input candidateRegistration) (domain.CandidateRevision, error) {
	status, raw, _, err := c.do(ctx, http.MethodPost, "/api/v1/workers/candidates", input)
	if err != nil {
		return domain.CandidateRevision{}, err
	}
	if status != http.StatusCreated && status != http.StatusOK {
		return domain.CandidateRevision{}, fmt.Errorf("candidate status %d (%s): %s",
			status, errorCode(raw), truncate(string(raw)))
	}
	var out struct {
		Candidate domain.CandidateRevision `json:"candidate"`
	}
	if err := json.Unmarshal(raw, &out); err != nil {
		return domain.CandidateRevision{}, err
	}
	if out.Candidate.ID == "" {
		return domain.CandidateRevision{}, errors.New("core returned no frozen candidate")
	}
	return out.Candidate, nil
}
