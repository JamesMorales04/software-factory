package domain

import (
	"errors"
	"fmt"
	"strings"
	"time"
)

// S3-A execution modes. Restricted is the S2 default (credential-free
// synthetic fixture). TRUSTED_LOCAL is owner-authorized local execution
// with broad host access under ADR-0009: never labeled isolated, sandboxed
// or certified. No ISOLATED value exists in S3-A; isolated execution keeps
// its separate certification path and is rejected here.
type ExecutionMode string

const (
	ExecRestricted   ExecutionMode = "restricted"
	ExecTrustedLocal ExecutionMode = "trusted-local"
)

// PaseoTaskKind is the S3-A workload for real Paseo coding sessions. It is
// authorized only with executionMode trusted-local and an exact
// provider/model/mode from the smoke-proven allowlist below.
const PaseoTaskKind = "paseo-task"

// MaxPaseoTaskChars bounds the operator-supplied coding task text.
const MaxPaseoTaskChars = 2000

// MaxPaseoIDLength bounds Paseo workspace and session identities stored in
// the binding row. Real agent IDs are UUIDs; workspace IDs are short
// opaque tokens. The bound keeps history small without second-guessing the
// exact Paseo alphabet beyond a safe charset.
const MaxPaseoIDLength = 128

// PaseoRoute is one smoke-proven provider/model/mode combination. Only
// these exact triples are authorized in S3; anything else — including a
// different mode for the same model — is rejected with no silent
// substitution.
type PaseoRoute struct {
	Provider string
	Model    string
	Mode     string
}

// PaseoAllowedRoutes is the closed S3 allowlist. Codex runs full-access
// (fresh disposable workspaces are untrusted, so mode auto would gate
// shell/edit behind approvals); OpenCode Go routes run build (default
// agent with default-allow workspace tools).
var PaseoAllowedRoutes = []PaseoRoute{
	{Provider: "codex", Model: "gpt-6-luna", Mode: "full-access"},
	{Provider: "opencode", Model: "opencode-go/muse-spark-1.3-contributor", Mode: "build"},
	{Provider: "opencode", Model: "opencode-go/deepseek-v4.1-flash", Mode: "build"},
}

// ValidateExecutionMode checks the requested execution mode.
func ValidateExecutionMode(mode string) error {
	switch ExecutionMode(strings.TrimSpace(mode)) {
	case ExecRestricted, ExecTrustedLocal:
		return nil
	default:
		return fmt.Errorf("unsupported executionMode %q: only %q and %q are authorized",
			mode, ExecRestricted, ExecTrustedLocal)
	}
}

// ValidatePaseoRoute asserts the exact requested triple against the
// allowlist. Every field is trimmed; matching is exact and case-sensitive.
// A pinned model with an empty effective model, or any substitution, is a
// hard failure at the domain boundary — never a fallback.
func ValidatePaseoRoute(provider, model, mode string) (PaseoRoute, error) {
	provider = strings.TrimSpace(provider)
	model = strings.TrimSpace(model)
	mode = strings.TrimSpace(mode)
	if provider == "" || model == "" || mode == "" {
		return PaseoRoute{}, errors.New("provider, model and paseoMode are all required for paseo-task runs")
	}
	for _, r := range PaseoAllowedRoutes {
		if r.Provider == provider && r.Model == model && r.Mode == mode {
			return r, nil
		}
	}
	return PaseoRoute{}, fmt.Errorf("unsupported Paseo route %q/%q mode %q: no silent substitution; authorized routes are exactly the S3 smoke-proven triples",
		provider, model, mode)
}

// validatePaseoTaskInput bounds the operator-supplied coding task. Only the
// task key is accepted; routing lives in top-level Run fields so it stays
// queryable and fenced.
func validatePaseoTaskInput(input map[string]any) (map[string]any, error) {
	if input == nil {
		return nil, errors.New("workloadInput is required")
	}
	if len(input) > 1 {
		return nil, errors.New("paseo-task workloadInput carries only the task key")
	}
	raw, ok := input["task"]
	if !ok {
		return nil, errors.New("workloadInput.task is required for paseo-task runs")
	}
	task, _ := raw.(string)
	task = strings.TrimSpace(task)
	if task == "" {
		return nil, errors.New("workloadInput.task must not be empty")
	}
	if len(task) > MaxPaseoTaskChars {
		return nil, fmt.Errorf("workloadInput.task exceeds %d characters", MaxPaseoTaskChars)
	}
	for k := range input {
		if k != "task" {
			return nil, fmt.Errorf("unknown workloadInput key %q", k)
		}
	}
	return map[string]any{"task": task}, nil
}

// Paseo session binding lifecycle. intended/active are non-terminal;
// succeeded/failed/cancelled/orphaned are terminal. orphaned means a Paseo
// session may still exist but its generation is superseded: it is recorded
// honestly for cleanup and can never become authoritative.
type PaseoSessionStatus string

const (
	PaseoIntended  PaseoSessionStatus = "intended"
	PaseoActive    PaseoSessionStatus = "active"
	PaseoSucceeded PaseoSessionStatus = "succeeded"
	PaseoFailed    PaseoSessionStatus = "failed"
	PaseoCancelled PaseoSessionStatus = "cancelled"
	PaseoOrphaned  PaseoSessionStatus = "orphaned"
)

// Terminal reports whether a session binding status ends the binding.
func (s PaseoSessionStatus) Terminal() bool {
	return s == PaseoSucceeded || s == PaseoFailed || s == PaseoCancelled || s == PaseoOrphaned
}

// ParsePaseoSessionStatus maps an external string to a binding status.
func ParsePaseoSessionStatus(s string) (PaseoSessionStatus, error) {
	st := PaseoSessionStatus(strings.TrimSpace(s))
	switch st {
	case PaseoIntended, PaseoActive, PaseoSucceeded, PaseoFailed, PaseoCancelled, PaseoOrphaned:
		return st, nil
	default:
		return "", fmt.Errorf("unknown paseo session status %q", s)
	}
}

// PaseoBinding is the durable 1:1 link between one Attempt generation and
// one real Paseo session. The row is created (intended) before any
// non-idempotent Paseo call, so a crash between session creation and
// identity persistence is detectable: an intended row without a sessionId,
// or an active row whose generation has been superseded, is reconciled to
// orphaned rather than silently dropped.
type PaseoBinding struct {
	AttemptID       string             `json:"attemptId"`
	RunID           string             `json:"runId"`
	Seq             int                `json:"seq"`
	WorkerID        string             `json:"workerId"`
	LeaseGeneration int64              `json:"leaseGeneration"`
	Provider        string             `json:"provider"`
	Model           string             `json:"model"`
	Mode            string             `json:"mode"`
	ExecMode        string             `json:"execMode"`
	WorkspaceID     string             `json:"workspaceId,omitempty"`
	SessionID       string             `json:"sessionId,omitempty"`
	Status          PaseoSessionStatus `json:"status"`
	Error           string             `json:"error,omitempty"`
	CreatedAt       time.Time          `json:"createdAt"`
	UpdatedAt       time.Time          `json:"updatedAt"`
}

// ValidatePaseoIdentity checks a Paseo workspace/session identifier shape:
// non-empty, bounded, opaque token charset. It does not interpret Paseo's
// internal ID allocation.
func ValidatePaseoIdentity(kind, id string) error {
	id = strings.TrimSpace(id)
	if id == "" {
		return fmt.Errorf("%s is required", kind)
	}
	if len(id) > MaxPaseoIDLength {
		return fmt.Errorf("%s exceeds %d characters", kind, MaxPaseoIDLength)
	}
	for _, r := range id {
		ok := r >= 'a' && r <= 'z' || r >= 'A' && r <= 'Z' || r >= '0' && r <= '9' ||
			r == '-' || r == '_' || r == ':' || r == '.'
		if !ok {
			return fmt.Errorf("%s carries an unsupported character", kind)
		}
	}
	return nil
}
