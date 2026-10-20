// Command factory-reviewer is the independent read-only Reviewer. It runs
// under a dedicated reviewer credential, revalidates the exact candidate
// identity, inspects the final diff and submits a candidate-bound verdict.
// It has no route that can mutate the candidate or its oracle.
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
	"strings"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/candidate"
	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/quality"
)

const reviewerVersion = "0.1.0-s4a"

type reviewerClient struct {
	base  string
	token string
	http  *http.Client
}

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, "factory-reviewer: "+err.Error())
		os.Exit(1)
	}
}

func run(args []string) error {
	flags := flag.NewFlagSet("factory-reviewer", flag.ContinueOnError)
	coreAddr := flags.String("core", "http://127.0.0.1:8471", "Factory Core base URL")
	candidateID := flags.String("candidate", "", "frozen candidate id to review")
	instanceID := flags.String("instance-id", "reviewer-1", "independent reviewer process identity")
	idempotencyKey := flags.String("idempotency-key", "", "stable idempotency key for verdict delivery")
	showVersion := flags.Bool("version", false, "print reviewer version and exit")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if *showVersion {
		fmt.Println(reviewerVersion)
		return nil
	}
	if strings.TrimSpace(*candidateID) == "" {
		return errors.New("--candidate is required")
	}
	if err := domain.ValidateWorkerID(*instanceID); err != nil {
		return fmt.Errorf("instance-id: %w", err)
	}
	token := strings.TrimSpace(os.Getenv("FACTORY_REVIEWER_TOKEN"))
	if token == "" {
		return errors.New("FACTORY_REVIEWER_TOKEN is required (scoped reviewer credential)")
	}
	c := &reviewerClient{
		base: strings.TrimRight(*coreAddr, "/"), token: token,
		http: &http.Client{Timeout: 30 * time.Second},
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Minute)
	defer cancel()

	record, spec, err := c.getCandidate(ctx, *candidateID)
	if err != nil {
		return err
	}
	if record.State != domain.CandidateFrozen {
		return fmt.Errorf("candidate is %s, not frozen", record.State)
	}
	findings, verdict, diffStat, diffBytes := review(ctx, record, spec)
	if *idempotencyKey == "" {
		*idempotencyKey = "review-" + record.ID + "-" + *instanceID
	}
	body := map[string]any{
		"instanceId": *instanceID, "candidateId": record.ID,
		"candidateSha": record.CommitSHA, "specId": spec.ID,
		"specHash": spec.ContentHash, "policyRevision": spec.PolicyRevision,
		"verdict": verdict, "findings": findings,
		"diffStat": diffStat, "diffBytes": diffBytes, "idempotencyKey": *idempotencyKey,
	}
	reviewID, err := c.postVerdict(ctx, body)
	if err != nil {
		return err
	}
	summary := map[string]any{
		"candidateId": record.ID, "candidateSha": record.CommitSHA,
		"verdict": verdict, "findings": findings, "reviewId": reviewID,
	}
	raw, _ := json.Marshal(summary)
	fmt.Println(string(raw))
	return nil
}

func review(ctx context.Context, record domain.CandidateRevision, spec domain.ActionSpec) ([]domain.ReviewFinding, domain.ReviewVerdictValues, string, int) {
	findings := make([]domain.ReviewFinding, 0)
	add := func(severity, message string) {
		if len(findings) >= domain.MaxFindings {
			return
		}
		findings = append(findings, domain.ReviewFinding{
			ID: fmt.Sprintf("review-%d", len(findings)+1), Severity: severity, Message: message,
		})
	}
	validation, err := candidate.InspectCommit(ctx, spec.Repository, record.BaseRevision,
		record.CommitSHA, record.CandidateRef, spec.AllowedPaths)
	if err != nil {
		add("blocking", "candidate identity/scope could not be revalidated")
		return findings, domain.ReviewBlocked, "", 0
	}
	if validation.TreeSHA != record.TreeSHA || validation.ContentDigest != record.ContentDigest ||
		validation.RiskClass != record.RiskClass ||
		strings.Join(validation.ChangedPaths, "\x00") != strings.Join(record.ChangedPaths, "\x00") {
		add("blocking", "Core candidate identity differs from the recomputed git evidence")
		return findings, domain.ReviewBlocked, "", 0
	}
	diff, diffBytes, err := candidate.Diff(ctx, spec.Repository, record.BaseRevision, record.CommitSHA)
	if err != nil {
		add("blocking", "final diff could not be read within the review bound")
		return findings, domain.ReviewBlocked, "", 0
	}
	diffStat, err := candidate.DiffStat(ctx, spec.Repository, record.BaseRevision, record.CommitSHA)
	if err != nil {
		diffStat = ""
	}
	risk := quality.ClassifyChange(record.ChangedPaths)
	if risk.RiskClass == "R2" || risk.RiskClass == "R3" {
		add("blocking", "final diff touches protected or contract paths requiring human approval")
	}
	if len(record.ChangedPaths) > 50 {
		add("blocking", "final diff changes too many paths for this bounded task")
	}
	if strings.Contains(diff, "Binary files ") {
		add("warning", "final diff contains a binary file change")
	}
	sourceChange := false
	for _, changed := range record.ChangedPaths {
		if !strings.HasSuffix(changed, "_test.go") && !strings.Contains(changed, ".test.") &&
			!strings.HasPrefix(changed, "docs/") {
			sourceChange = true
			break
		}
	}
	if !sourceChange {
		add("blocking", "candidate changes no production source file")
	}
	for _, line := range candidate.AddedLines(diff) {
		lower := strings.ToLower(line)
		switch {
		case strings.Contains(lower, "begin private key"), strings.Contains(lower, "aws_secret"),
			strings.Contains(lower, "ghp_"), strings.Contains(lower, "api_key="),
			strings.Contains(lower, "password="), strings.Contains(lower, "secret="):
			add("blocking", "added line contains a secret-shaped value")
		case strings.Contains(lower, "child_process"), strings.Contains(lower, "eval("):
			add("warning", "added line introduces a high-risk dynamic execution primitive")
		}
	}
	verdict := domain.ReviewApprove
	for _, finding := range findings {
		if finding.Severity == "blocking" {
			verdict = domain.ReviewRequestChanges
			break
		}
	}
	return findings, verdict, diffStat, diffBytes
}

func (c *reviewerClient) getCandidate(ctx context.Context, id string) (domain.CandidateRevision, domain.ActionSpec, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, c.base+"/api/v1/reviewer/candidates/"+id, nil)
	if err != nil {
		return domain.CandidateRevision{}, domain.ActionSpec{}, err
	}
	status, raw, err := c.do(req)
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

func (c *reviewerClient) postVerdict(ctx context.Context, body any) (string, error) {
	rawBody, err := json.Marshal(body)
	if err != nil {
		return "", err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.base+"/api/v1/reviewer/verdict", bytes.NewReader(rawBody))
	if err != nil {
		return "", err
	}
	req.Header.Set("Content-Type", "application/json")
	status, raw, err := c.do(req)
	if err != nil {
		return "", err
	}
	if status != http.StatusCreated && status != http.StatusOK {
		return "", fmt.Errorf("verdict rejected (%d): %s", status, bounded(raw))
	}
	var out struct {
		Review struct {
			ID string `json:"id"`
		} `json:"review"`
	}
	if err := json.Unmarshal(raw, &out); err != nil {
		return "", err
	}
	return out.Review.ID, nil
}

func (c *reviewerClient) do(req *http.Request) (int, []byte, error) {
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
