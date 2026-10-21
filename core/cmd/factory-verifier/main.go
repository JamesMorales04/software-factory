// Command factory-verifier is the independent functional verifier. It runs
// under a dedicated verifier credential (never the Worker or operator
// token), checks out the exact frozen candidate into a clean read-only
// worktree, executes the frozen acceptance/negative oracle, and submits
// concrete results. It cannot mutate the candidate or author a verdict.
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/candidate"
	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

const verifierVersion = "0.1.0-s4c"

type verifierClient struct {
	base  string
	token string
	http  *http.Client
}

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, "factory-verifier: "+err.Error())
		os.Exit(1)
	}
}

func run(args []string) error {
	flags := flag.NewFlagSet("factory-verifier", flag.ContinueOnError)
	coreAddr := flags.String("core", "http://127.0.0.1:8471", "Factory Core base URL")
	candidateID := flags.String("candidate", "", "frozen candidate id to verify")
	instanceID := flags.String("instance-id", "verifier-1", "independent verifier process identity")
	idempotencyKey := flags.String("idempotency-key", "", "stable idempotency key for evidence delivery")
	showVersion := flags.Bool("version", false, "print verifier version and exit")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if *showVersion {
		fmt.Println(verifierVersion)
		return nil
	}
	if strings.TrimSpace(*candidateID) == "" {
		return errors.New("--candidate is required")
	}
	if err := domain.ValidateWorkerID(*instanceID); err != nil {
		return fmt.Errorf("instance-id: %w", err)
	}
	token := strings.TrimSpace(os.Getenv("FACTORY_VERIFIER_TOKEN"))
	if token == "" {
		return errors.New("FACTORY_VERIFIER_TOKEN is required (scoped verifier credential)")
	}
	client := &verifierClient{
		base: strings.TrimRight(*coreAddr, "/"), token: token,
		http: &http.Client{Timeout: 30 * time.Second},
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Minute)
	defer cancel()

	c, spec, err := client.getCandidate(ctx, *candidateID)
	if err != nil {
		return err
	}
	if c.State != domain.CandidateFrozen {
		return fmt.Errorf("candidate is %s, not frozen", c.State)
	}
	if c.CommitSHA == "" || c.TreeSHA == "" || c.ContentDigest == "" {
		return errors.New("candidate lacks an immutable git identity")
	}

	temp, err := os.MkdirTemp("", "factory-verify-*")
	if err != nil {
		return fmt.Errorf("create verifier workspace: %w", err)
	}
	defer func() {
		candidate.RestoreWritable(temp)
		_ = os.RemoveAll(temp)
	}()
	if err := candidate.CloneAt(ctx, spec.Repository, c.CommitSHA, c.TreeSHA, c.ContentDigest, temp); err != nil {
		return fmt.Errorf("clean candidate checkout rejected: %w", err)
	}
	checkout := filepath.Join(temp, "candidate")
	if err := candidate.MarkReadOnly(checkout); err != nil {
		return fmt.Errorf("protect candidate checkout: %w", err)
	}

	instanceHome := filepath.Join(temp, "home")
	if err := os.MkdirAll(instanceHome, 0o700); err != nil {
		return fmt.Errorf("create verifier home: %w", err)
	}
	results := make([]domain.CheckResult, 0, len(spec.AllChecks()))
	findings := make([]string, 0)
	// A protected oracle script is part of the frozen acceptance identity.
	// The candidate may only change paths explicitly authorized by the
	// spec; any changed command target is rejected before execution.
	if modified := modifiedOracleTargets(checkout, c.ChangedPaths, spec.AllChecks()); len(modified) > 0 {
		return fmt.Errorf("protected acceptance oracle was modified by the candidate: %s", strings.Join(modified, ", "))
	}
	for _, check := range spec.AllChecks() {
		result := executeCheck(ctx, checkout, instanceHome, check)
		results = append(results, result)
		if result.Status == domain.CheckNotTested {
			findings = append(findings, "tool unavailable for check "+check.ID)
		}
		if result.Status == domain.CheckFail {
			findings = append(findings, "check failed: "+check.ID)
		}
	}

	if err := candidate.VerifyCheckout(ctx, checkout, c.CommitSHA, c.TreeSHA, c.ContentDigest); err != nil {
		return fmt.Errorf("candidate identity changed during verification: %w", err)
	}
	modified, _, err := candidate.ChangedSince(ctx, checkout)
	if err != nil {
		return fmt.Errorf("read-only check failed: %w", err)
	}
	if len(modified) > 0 {
		return fmt.Errorf("verification modified the candidate worktree: %s", strings.Join(modified, ", "))
	}
	if *idempotencyKey == "" {
		*idempotencyKey = "verify-" + c.ID + "-" + *instanceID
	}
	evidence := map[string]any{
		"instanceId": *instanceID, "candidateId": c.ID, "candidateSha": c.CommitSHA,
		"specId": spec.ID, "specHash": spec.ContentHash, "policyRevision": spec.PolicyRevision,
		"oracleDigest":  spec.OracleDigest(),
		"cleanResource": "git-clone:" + c.CommitSHA[:12],
		"checks":        results, "findings": findings, "idempotencyKey": *idempotencyKey,
	}
	stored, err := client.postEvidence(ctx, evidence)
	if err != nil {
		return err
	}
	status := domain.CheckPass
	for _, result := range results {
		if result.Status != domain.CheckPass {
			if result.Status == domain.CheckFail {
				status = domain.CheckFail
			} else if status == domain.CheckPass {
				status = result.Status
			}
		}
	}
	summary := map[string]any{
		"candidateId": c.ID, "candidateSha": c.CommitSHA, "checkout": "clean-read-only-git-clone",
		"checks": results, "status": status, "evidenceId": stored,
	}
	raw, _ := json.Marshal(summary)
	fmt.Println(string(raw))
	return nil
}

func modifiedOracleTargets(checkout string, changed []string, checks []domain.SpecCheck) []string {
	changedSet := map[string]bool{}
	for _, path := range changed {
		changedSet[path] = true
	}
	seen := map[string]bool{}
	var out []string
	for _, check := range checks {
		for _, arg := range check.Command {
			target := strings.TrimPrefix(strings.TrimSpace(arg), "./")
			if target == "" || strings.HasPrefix(target, "-") || strings.Contains(target, "/../") {
				continue
			}
			if !changedSet[target] {
				continue
			}
			info, err := os.Stat(filepath.Join(checkout, target))
			if err != nil || info.IsDir() {
				continue
			}
			if !seen[target] {
				seen[target] = true
				out = append(out, target)
			}
		}
	}
	return out
}

func executeCheck(parent context.Context, checkout, home string, check domain.SpecCheck) domain.CheckResult {
	result := domain.CheckResult{ID: check.ID, Kind: check.Kind, Command: check.Command, ExitCode: -1}
	if _, err := exec.LookPath(check.Command[0]); err != nil {
		result.Status = domain.CheckNotTested
		result.Detail = "tool unavailable: " + filepath.Base(check.Command[0])
		return result
	}
	if check.Kind == domain.CheckBrowser {
		if err := browserToolingAvailable(); err != nil {
			result.Status = domain.CheckNotTested
			result.Detail = err.Error()
			return result
		}
	}
	ctx, cancel := context.WithTimeout(parent, time.Duration(check.TimeoutSecs)*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, check.Command[0], check.Command[1:]...)
	cmd.Dir = checkout
	cmd.Env = []string{
		"PATH=" + os.Getenv("PATH"),
		"HOME=" + home,
		"TMPDIR=" + filepath.Join(home, "tmp"),
		"LC_ALL=C",
		"CI=1",
	}
	for _, key := range []string{"CHROMIUM_EXE", "FACTORY_PLAYWRIGHT_CORE", "PLAYWRIGHT_BROWSERS_PATH"} {
		if value := strings.TrimSpace(os.Getenv(key)); value != "" {
			cmd.Env = append(cmd.Env, key+"="+value)
		}
	}
	if err := os.MkdirAll(filepath.Join(home, "tmp"), 0o700); err != nil {
		result.Status = domain.CheckNotTested
		result.Detail = "verifier temp directory unavailable"
		return result
	}
	var browserOutput bytes.Buffer
	if check.Kind == domain.CheckBrowser {
		cmd.Stdout = &browserOutput
		cmd.Stderr = &browserOutput
	} else {
		cmd.Stdout = io.Discard
		cmd.Stderr = io.Discard
	}
	started := time.Now()
	err := cmd.Run()
	result.DurationMs = time.Since(started).Milliseconds()
	if ctx.Err() != nil {
		result.Status = domain.CheckFail
		result.ExitCode = -1
		result.Detail = "check timed out"
		return result
	}
	exitCode := 0
	if err != nil {
		var exitErr *exec.ExitError
		if errors.As(err, &exitErr) {
			exitCode = exitErr.ExitCode()
		} else {
			result.Status = domain.CheckFail
			result.Detail = "check could not start"
			return result
		}
	}
	result.ExitCode = exitCode
	if check.Kind == domain.CheckBrowser {
		result.Detail = browserMetadata(browserOutput.Bytes())
	}
	if exitCode == check.ExpectExit {
		result.Status = domain.CheckPass
		if check.Kind != domain.CheckBrowser {
			result.Detail = fmt.Sprintf("exit %d as expected", exitCode)
		}
		return result
	}
	result.Status = domain.CheckFail
	if check.Kind == domain.CheckBrowser && result.Detail != "" {
		result.Detail = fmt.Sprintf("exit %d, expected %d; %s", exitCode, check.ExpectExit, result.Detail)
	} else {
		result.Detail = fmt.Sprintf("exit %d, expected %d (tool output not retained)", exitCode, check.ExpectExit)
	}
	return result
}

func browserToolingAvailable() error {
	exe := strings.TrimSpace(os.Getenv("CHROMIUM_EXE"))
	if exe == "" {
		return errors.New("browser check NOT_TESTED: CHROMIUM_EXE is not configured")
	}
	if info, err := os.Stat(exe); err != nil || info.IsDir() {
		return errors.New("browser check NOT_TESTED: CHROMIUM_EXE is not an executable file")
	}
	core := strings.TrimSpace(os.Getenv("FACTORY_PLAYWRIGHT_CORE"))
	if core == "" {
		return errors.New("browser check NOT_TESTED: FACTORY_PLAYWRIGHT_CORE is not configured")
	}
	if _, err := os.Stat(core); err != nil {
		return errors.New("browser check NOT_TESTED: FACTORY_PLAYWRIGHT_CORE is unavailable")
	}
	return nil
}

// browserMetadata keeps only bounded tool-provenance fields from the
// browser check's machine-readable final line. Page content, console text,
// URLs with query data and request bodies never enter evidence.
func browserMetadata(raw []byte) string {
	lines := strings.Split(strings.TrimSpace(string(raw)), "\n")
	for i := len(lines) - 1; i >= 0; i-- {
		line := strings.TrimSpace(lines[i])
		if !strings.HasPrefix(line, "{") {
			continue
		}
		var meta struct {
			Browser          string `json:"browser"`
			BrowserVersion   string `json:"browserVersion"`
			ConsoleErrors    int    `json:"consoleErrors"`
			FailedRequests   int    `json:"failedRequests"`
			UserDataIsolated bool   `json:"userDataIsolated"`
		}
		if err := json.Unmarshal([]byte(line), &meta); err != nil {
			continue
		}
		if meta.Browser == "" {
			continue
		}
		value := fmt.Sprintf("%s %s; consoleErrors=%d failedRequests=%d isolatedProfile=%t",
			meta.Browser, meta.BrowserVersion, meta.ConsoleErrors, meta.FailedRequests, meta.UserDataIsolated)
		if len(value) > domain.MaxCheckDescription {
			value = value[:domain.MaxCheckDescription]
		}
		return value
	}
	return "browser check completed (no structured metadata retained)"
}

func (c *verifierClient) getCandidate(ctx context.Context, id string) (domain.CandidateRevision, domain.ActionSpec, error) {
	status, raw, err := c.get(ctx, "/api/v1/verifier/candidates/"+id)
	if err != nil {
		return domain.CandidateRevision{}, domain.ActionSpec{}, err
	}
	if status != http.StatusOK {
		return domain.CandidateRevision{}, domain.ActionSpec{}, fmt.Errorf("candidate fetch status %d: %s", status, bounded(raw))
	}
	var out struct {
		Candidate domain.CandidateRevision `json:"candidate"`
		Spec      domain.ActionSpec        `json:"spec"`
	}
	if err := json.Unmarshal(raw, &out); err != nil {
		return domain.CandidateRevision{}, domain.ActionSpec{}, err
	}
	return out.Candidate, out.Spec, nil
}

func (c *verifierClient) postEvidence(ctx context.Context, body any) (string, error) {
	rawBody, err := json.Marshal(body)
	if err != nil {
		return "", err
	}
	status, raw, err := c.post(ctx, "/api/v1/verifier/evidence", rawBody)
	if err != nil {
		return "", err
	}
	if status != http.StatusCreated && status != http.StatusOK {
		return "", fmt.Errorf("evidence rejected (%d): %s", status, bounded(raw))
	}
	var out struct {
		Evidence struct {
			ID string `json:"id"`
		} `json:"evidence"`
	}
	if err := json.Unmarshal(raw, &out); err != nil {
		return "", err
	}
	return out.Evidence.ID, nil
}

func (c *verifierClient) get(ctx context.Context, path string) (int, []byte, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, c.base+path, nil)
	if err != nil {
		return 0, nil, err
	}
	return c.do(req)
}

func (c *verifierClient) post(ctx context.Context, path string, body []byte) (int, []byte, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.base+path, bytes.NewReader(body))
	if err != nil {
		return 0, nil, err
	}
	req.Header.Set("Content-Type", "application/json")
	return c.do(req)
}

func (c *verifierClient) do(req *http.Request) (int, []byte, error) {
	req.Header.Set("Authorization", "Bearer "+c.token)
	resp, err := c.http.Do(req)
	if err != nil {
		return 0, nil, err
	}
	defer func() { _ = resp.Body.Close() }()
	raw, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return 0, nil, err
	}
	return resp.StatusCode, raw, nil
}

func bounded(raw []byte) string {
	value := strings.TrimSpace(string(raw))
	if len(value) > 300 {
		return value[:300]
	}
	return value
}
