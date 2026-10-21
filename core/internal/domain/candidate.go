package domain

import (
	"errors"
	"fmt"
	"sort"
	"strings"
	"time"
)

// CandidateState is the durable lifecycle of an immutable candidate.
type CandidateState string

const (
	CandidateFrozen     CandidateState = "frozen"
	CandidateSuperseded CandidateState = "superseded"
	CandidateRejected   CandidateState = "rejected"
)

// CandidateRevision is the Core-authoritative immutable source identity
// produced by one Implementer Attempt. The Core validates the commit and
// provenance; Worker assertions are never trusted on their own.
type CandidateRevision struct {
	ID                    string         `json:"id"`
	ProjectID             string         `json:"projectId"`
	ActionID              string         `json:"actionId"`
	SpecID                string         `json:"specId"`
	SpecHash              string         `json:"specHash"`
	SpecBindingHash       string         `json:"specBindingHash,omitempty"`
	PolicyRevision        string         `json:"policyRevision"`
	BaseRevision          string         `json:"baseRevision"`
	CommitSHA             string         `json:"commitSha"`
	TreeSHA               string         `json:"treeSha"`
	ContentDigest         string         `json:"contentDigest"`
	CandidateRef          string         `json:"candidateRef"`
	ParentCandidateID     string         `json:"parentCandidateId,omitempty"`
	Repository            string         `json:"repository"`
	ImplementerRunID      string         `json:"implementerRunId"`
	ImplementerAttemptID  string         `json:"implementerAttemptId"`
	ImplementerGeneration int64          `json:"implementerGeneration"`
	ImplementerWorkerID   string         `json:"implementerWorkerId"`
	ExecutionMode         string         `json:"executionMode"`
	Provider              string         `json:"provider"`
	Model                 string         `json:"model"`
	PaseoMode             string         `json:"paseoMode"`
	ChangedPaths          []string       `json:"changedPaths"`
	RiskClass             RiskClass      `json:"riskClass"`
	State                 CandidateState `json:"state"`
	CreatedAt             time.Time      `json:"createdAt"`
	FrozenAt              time.Time      `json:"frozenAt"`
	SupersededAt          time.Time      `json:"supersededAt,omitempty"`
}

// ValidateCandidateInput carries the Worker's bounded candidate claim. The
// Core fills tree/content identities from git evidence, never from this
// input alone.
type ValidateCandidateInput struct {
	RunID        string
	AttemptID    string
	Generation   int64
	SpecID       string
	CommitSHA    string
	CandidateRef string
}

// Validate checks a candidate row loaded from durable state.
func (c CandidateRevision) Validate() error {
	if len(c.ID) != 32 || !isLowerHex(c.ID) {
		return errors.New("candidate id must be a 32-character lowercase hex id")
	}
	if err := ValidateProjectID(c.ProjectID); err != nil {
		return err
	}
	if len(c.ActionID) != 32 || !isLowerHex(c.ActionID) {
		return errors.New("candidate actionId must be a 32-character lowercase hex id")
	}
	if len(c.SpecID) != 32 || !isLowerHex(c.SpecID) {
		return errors.New("candidate specId must be a 32-character lowercase hex id")
	}
	if !isLowerHex(c.SpecHash) || len(c.SpecHash) != 64 {
		return errors.New("candidate specHash must be a sha256 hex digest")
	}
	if c.SpecBindingHash != "" && (!isLowerHex(c.SpecBindingHash) || len(c.SpecBindingHash) != 64) {
		return errors.New("candidate specBindingHash must be a sha256 hex digest")
	}
	if c.PolicyRevision != QualityPolicyRevision {
		return fmt.Errorf("unsupported candidate policyRevision %q", c.PolicyRevision)
	}
	if !isGitObjectID(c.BaseRevision) || !isGitObjectID(c.CommitSHA) || !isGitObjectID(c.TreeSHA) {
		return errors.New("candidate git identities must be full object ids")
	}
	if !isLowerHex(c.ContentDigest) || len(c.ContentDigest) != 64 {
		return errors.New("candidate contentDigest must be a sha256 hex digest")
	}
	if !strings.HasPrefix(c.CandidateRef, "refs/factory/candidates/") ||
		len(c.CandidateRef) > MaxCandidateRefChars {
		return errors.New("candidateRef must be a bounded refs/factory/candidates/ ref")
	}
	if len(c.Repository) == 0 || len(c.Repository) > 1024 {
		return errors.New("candidate repository is required and bounded")
	}
	if len(c.ImplementerRunID) != 32 || !isLowerHex(c.ImplementerRunID) {
		return errors.New("implementerRunId must be a 32-character lowercase hex id")
	}
	if len(c.ImplementerAttemptID) != 32 || !isLowerHex(c.ImplementerAttemptID) {
		return errors.New("implementerAttemptId must be a 32-character lowercase hex id")
	}
	if c.ImplementerGeneration < 1 {
		return errors.New("implementerGeneration must be positive")
	}
	if err := ValidateWorkerID(c.ImplementerWorkerID); err != nil {
		return err
	}
	if c.ExecutionMode != string(ExecTrustedLocal) {
		return errors.New("candidate executionMode must be trusted-local (never isolated)")
	}
	if _, err := ValidatePaseoRoute(c.Provider, c.Model, c.PaseoMode); err != nil {
		return err
	}
	if len(c.ChangedPaths) == 0 {
		return errors.New("candidate must change at least one source path")
	}
	if len(c.ChangedPaths) > 512 {
		return errors.New("candidate changed paths exceed the bound")
	}
	for _, changed := range c.ChangedPaths {
		if err := validateRelativePath(changed); err != nil {
			return err
		}
	}
	if _, err := ParseRiskClass(string(c.RiskClass)); err != nil {
		return err
	}
	switch c.State {
	case CandidateFrozen, CandidateSuperseded, CandidateRejected:
	default:
		return fmt.Errorf("unknown candidate state %q", c.State)
	}
	if c.FrozenAt.IsZero() {
		return errors.New("candidate frozenAt is required")
	}
	if c.State == CandidateSuperseded && c.SupersededAt.IsZero() {
		return errors.New("superseded candidate requires supersededAt")
	}
	return nil
}

// CandidateValidation is the Core's independently computed git evidence for
// one candidate commit. The CandidateRevision is built from this, not from
// Worker claims.
type CandidateValidation struct {
	CommitSHA     string
	TreeSHA       string
	ContentDigest string
	ChangedPaths  []string
	RiskClass     RiskClass
}

// VerificationEvidence is one independent verifier bundle bound to an exact
// candidate/spec/policy identity. Authority and actor are assigned by the
// Core from the scoped credential, never trusted from the body.
type VerificationEvidence struct {
	ID             string         `json:"id"`
	CandidateID    string         `json:"candidateId"`
	CandidateSHA   string         `json:"candidateSha"`
	SpecID         string         `json:"specId"`
	SpecHash       string         `json:"specHash"`
	PolicyRevision string         `json:"policyRevision"`
	Authority      string         `json:"authority"`
	Actor          string         `json:"actor"`
	Independent    bool           `json:"independent"`
	CleanResource  string         `json:"cleanResource"`
	OracleDigest   string         `json:"oracleDigest"`
	Status         EvidenceStatus `json:"status"`
	Checks         []CheckResult  `json:"checks"`
	Findings       []string       `json:"findings"`
	IdempotencyKey string         `json:"idempotencyKey"`
	CreatedAt      time.Time      `json:"createdAt"`
}

// CheckResult is one concrete check outcome. For verifier checks the
// command is normalized from the frozen spec by the Core.
type CheckResult struct {
	ID         string      `json:"id"`
	Kind       CheckKind   `json:"kind"`
	Status     CheckStatus `json:"status"`
	Command    []string    `json:"command,omitempty"`
	ExitCode   int         `json:"exitCode"`
	DurationMs int64       `json:"durationMs"`
	Detail     string      `json:"detail,omitempty"`
}

// ReviewFinding is one bounded reviewer finding.
type ReviewFinding struct {
	ID       string `json:"id"`
	Severity string `json:"severity"`
	Message  string `json:"message"`
}

// ReviewVerdictValues is the closed reviewer vocabulary.
type ReviewVerdictValues string

const (
	ReviewApprove        ReviewVerdictValues = "APPROVE"
	ReviewRequestChanges ReviewVerdictValues = "REQUEST_CHANGES"
	ReviewBlocked        ReviewVerdictValues = "BLOCKED"
)

// ReviewEvidence is one read-only reviewer verdict bound to an exact
// candidate/spec/policy identity. The reviewer cannot mutate the candidate.
type ReviewEvidence struct {
	ID             string              `json:"id"`
	CandidateID    string              `json:"candidateId"`
	CandidateSHA   string              `json:"candidateSha"`
	SpecID         string              `json:"specId"`
	SpecHash       string              `json:"specHash"`
	PolicyRevision string              `json:"policyRevision"`
	Authority      string              `json:"authority"`
	Actor          string              `json:"actor"`
	Verdict        ReviewVerdictValues `json:"verdict"`
	Findings       []ReviewFinding     `json:"findings"`
	DiffStat       string              `json:"diffStat"`
	DiffBytes      int                 `json:"diffBytes"`
	IdempotencyKey string              `json:"idempotencyKey"`
	CreatedAt      time.Time           `json:"createdAt"`
}

// GateDecision is the single authoritative Core decision for one candidate.
// It is deterministic, auditable and idempotent.
type GateDecision struct {
	ID             string        `json:"id"`
	CandidateID    string        `json:"candidateId"`
	CandidateSHA   string        `json:"candidateSha"`
	SpecID         string        `json:"specId"`
	SpecHash       string        `json:"specHash"`
	PolicyRevision string        `json:"policyRevision"`
	Status         GateStatus    `json:"status"`
	EvidenceDigest string        `json:"evidenceDigest"`
	Reasons        []string      `json:"reasons"`
	Checks         []CheckResult `json:"checks"`
	Actor          string        `json:"actor"`
	CreatedAt      time.Time     `json:"createdAt"`
}

// CandidateBundle is the authoritative read projection consumed by the
// verifier, reviewer, Office and acceptance tests.
type CandidateBundle struct {
	Candidate     CandidateRevision      `json:"candidate"`
	Spec          ActionSpec             `json:"spec"`
	Verifications []VerificationEvidence `json:"verifications"`
	Reviews       []ReviewEvidence       `json:"reviews"`
	Decision      *GateDecision          `json:"decision,omitempty"`
}

// SortedChangedPaths returns a sorted, de-duplicated path list.
func SortedChangedPaths(paths []string) []string {
	seen := map[string]bool{}
	out := make([]string, 0, len(paths))
	for _, p := range paths {
		p = strings.TrimSpace(p)
		if p == "" || seen[p] {
			continue
		}
		seen[p] = true
		out = append(out, p)
	}
	sort.Strings(out)
	return out
}
