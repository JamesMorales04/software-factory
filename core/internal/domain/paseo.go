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

// S3-D external ownership correlation. Paseo 0.9.1 supports exact agent
// labels and workspace/session titles, but workspaces have no label API.
// The full Run ID plus generation is therefore the title identity; the
// labels add the Attempt, generation and workspace for exact session
// filtering. Short Run prefixes are never an ownership identity.
const (
	PaseoLabelOwner      = "factory.owner"
	PaseoLabelRun        = "factory.run"
	PaseoLabelAttempt    = "factory.attempt"
	PaseoLabelGeneration = "factory.generation"
	PaseoLabelWorkspace  = "factory.workspace"
	PaseoOwnerValue      = "software-factory"
)

// PaseoCorrelationKey is the deterministic, collision-free-by-content title
// for resources created by one Attempt generation. It is persisted on the
// binding before any external call.
func PaseoCorrelationKey(runID string, generation int64) string {
	return fmt.Sprintf("factory-s3d:%s:g%d", strings.TrimSpace(runID), generation)
}

// PaseoLegacyTitle is the S3-B title convention. It is not generation
// safe; it is used only as an additional marker for already-bound legacy
// resources that are cleaned by their durable Paseo ID.
func PaseoLegacyTitle(runID string) string {
	id := strings.TrimSpace(runID)
	if len(id) > 8 {
		id = id[:8]
	}
	return "factory-" + id
}

// PaseoCorrelationLabels returns the exact label set for one session. The
// workspace ID is known before the session starts; empty values are
// omitted rather than matched loosely.
func PaseoCorrelationLabels(runID, attemptID string, generation int64, workspaceID string) map[string]string {
	labels := map[string]string{
		PaseoLabelOwner:      PaseoOwnerValue,
		PaseoLabelRun:        strings.TrimSpace(runID),
		PaseoLabelAttempt:    strings.TrimSpace(attemptID),
		PaseoLabelGeneration: fmt.Sprintf("%d", generation),
	}
	if strings.TrimSpace(workspaceID) != "" {
		labels[PaseoLabelWorkspace] = strings.TrimSpace(workspaceID)
	}
	return labels
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
	CorrelationKey  string             `json:"correlationKey,omitempty"`
	Status          PaseoSessionStatus `json:"status"`
	Error           string             `json:"error,omitempty"`
	CreatedAt       time.Time          `json:"createdAt"`
	UpdatedAt       time.Time          `json:"updatedAt"`
}

// PaseoSessionSummary is the read-only cross-Run projection the Office
// Agents view renders. It embeds the authoritative binding row and adds the
// owning Run's current status plus the optional S3-D recovery receipt; it is
// a snapshot, never a second source of truth. No prompt, transcript,
// credential or source body is part of this projection.
type PaseoSessionSummary struct {
	PaseoBinding
	RunStatus RunStatus      `json:"runStatus"`
	Recovery  *PaseoRecovery `json:"recovery,omitempty"`
}

// PaseoRecoveryStatus is the durable recovery-operation state for one
// orphaned binding. confirmed/absent/blocked are terminal; unconfirmed is
// retryable within the bounded budget. It never changes the binding status.
type PaseoRecoveryStatus string

const (
	RecoveryPending     PaseoRecoveryStatus = "pending"
	RecoveryClaimed     PaseoRecoveryStatus = "claimed"
	RecoveryUnconfirmed PaseoRecoveryStatus = "unconfirmed"
	RecoveryConfirmed   PaseoRecoveryStatus = "confirmed"
	RecoveryAbsent      PaseoRecoveryStatus = "absent"
	RecoveryBlocked     PaseoRecoveryStatus = "blocked"
)

// Terminal reports whether automatic recovery has finished for this row.
func (s PaseoRecoveryStatus) Terminal() bool {
	return s == RecoveryConfirmed || s == RecoveryAbsent || s == RecoveryBlocked
}

// SafeForReplacement reports whether a prior orphaned generation is settled
// enough to permit a new Attempt of the same Run.
func (s PaseoRecoveryStatus) SafeForReplacement() bool {
	return s == RecoveryConfirmed || s == RecoveryAbsent
}

// ParsePaseoRecoveryStatus maps an operator/API string to a recovery state.
func ParsePaseoRecoveryStatus(s string) (PaseoRecoveryStatus, error) {
	st := PaseoRecoveryStatus(strings.TrimSpace(s))
	switch st {
	case RecoveryPending, RecoveryClaimed, RecoveryUnconfirmed, RecoveryConfirmed, RecoveryAbsent, RecoveryBlocked:
		return st, nil
	default:
		return "", fmt.Errorf("unknown recovery status %q", s)
	}
}

// PaseoObservation is a bounded external observation. Workspace values use
// absent/present/unknown; session values use absent/archived/active/idle/
// unknown.
type PaseoObservation string

const (
	PaseoObservedAbsent   PaseoObservation = "absent"
	PaseoObservedArchived PaseoObservation = "archived"
	PaseoObservedActive   PaseoObservation = "active"
	PaseoObservedIdle     PaseoObservation = "idle"
	PaseoObservedPresent  PaseoObservation = "present"
	PaseoObservedUnknown  PaseoObservation = "unknown"
)

// PaseoRecoveryOutcome is the Worker's completed observation class.
type PaseoRecoveryOutcome string

const (
	RecoveryOutcomeConfirmed   PaseoRecoveryOutcome = "confirmed"
	RecoveryOutcomeAbsent      PaseoRecoveryOutcome = "absent"
	RecoveryOutcomeUnconfirmed PaseoRecoveryOutcome = "unconfirmed"
	RecoveryOutcomeBlocked     PaseoRecoveryOutcome = "blocked"
)

// ParsePaseoRecoveryOutcome validates a completion outcome.
func ParsePaseoRecoveryOutcome(s string) (PaseoRecoveryOutcome, error) {
	o := PaseoRecoveryOutcome(strings.TrimSpace(s))
	switch o {
	case RecoveryOutcomeConfirmed, RecoveryOutcomeAbsent, RecoveryOutcomeUnconfirmed, RecoveryOutcomeBlocked:
		return o, nil
	default:
		return "", fmt.Errorf("unknown recovery outcome %q (want confirmed|absent|unconfirmed|blocked)", s)
	}
}

// ValidateSessionObservation checks the session observation vocabulary.
func ValidateSessionObservation(o PaseoObservation) error {
	switch o {
	case PaseoObservedAbsent, PaseoObservedArchived, PaseoObservedActive, PaseoObservedIdle, PaseoObservedUnknown:
		return nil
	default:
		return fmt.Errorf("unknown session observation %q", o)
	}
}

// ValidateWorkspaceObservation checks the workspace observation vocabulary.
func ValidateWorkspaceObservation(o PaseoObservation) error {
	switch o {
	case PaseoObservedAbsent, PaseoObservedPresent, PaseoObservedUnknown:
		return nil
	default:
		return fmt.Errorf("unknown workspace observation %q", o)
	}
}

// PaseoRecovery is the durable cleanup/reconciliation receipt for one
// orphaned binding. It is separate from the immutable binding row: an
// orphaned binding stays orphaned even after cleanup is confirmed.
type PaseoRecovery struct {
	AttemptID         string               `json:"attemptId"`
	RunID             string               `json:"runId"`
	LeaseGeneration   int64                `json:"leaseGeneration"`
	Status            PaseoRecoveryStatus  `json:"status"`
	ClaimOwner        string               `json:"claimOwner,omitempty"`
	ClaimToken        string               `json:"-"`
	ClaimExpiresAt    time.Time            `json:"claimExpiresAt,omitempty"`
	Attempts          int                  `json:"attempts"`
	NextAttemptAt     time.Time            `json:"nextAttemptAt,omitempty"`
	LastObservedAt    time.Time            `json:"lastObservedAt,omitempty"`
	SessionID         string               `json:"sessionId,omitempty"`
	WorkspaceID       string               `json:"workspaceId,omitempty"`
	SessionObserved   PaseoObservation     `json:"sessionObserved,omitempty"`
	WorkspaceObserved PaseoObservation     `json:"workspaceObserved,omitempty"`
	LastOutcome       PaseoRecoveryOutcome `json:"lastOutcome,omitempty"`
	Reason            string               `json:"reason,omitempty"`
	CreatedAt         time.Time            `json:"createdAt"`
	UpdatedAt         time.Time            `json:"updatedAt"`
}

// PaseoRecoveryClaim is the Worker-only claim response. ClaimToken is a
// cleanup fencing token and is never part of the operator projection.
type PaseoRecoveryClaim struct {
	Recovery   PaseoRecovery `json:"recovery"`
	ClaimToken string        `json:"claimToken"`
	Session    PaseoBinding  `json:"session"`
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
