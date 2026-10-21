// Package domain holds the S2 authoritative business rules for Runs,
// Attempts, Workers, leases and fencing. It depends on nothing outside the
// standard library so the rules have a single source of truth, independent
// of persistence, transport and telemetry adapters.
package domain

import (
	"errors"
	"fmt"
	"strings"
	"time"
)

// S2 Run lifecycle states. queued/leased/running are non-terminal;
// succeeded/failed/cancelled are terminal.
type RunStatus string

const (
	RunQueued    RunStatus = "queued"
	RunLeased    RunStatus = "leased"
	RunRunning   RunStatus = "running"
	RunSucceeded RunStatus = "succeeded"
	RunFailed    RunStatus = "failed"
	RunCancelled RunStatus = "cancelled"
)

// S2 Attempt lifecycle states. leased/running are non-terminal;
// succeeded/failed/expired/cancelled are terminal.
type AttemptStatus string

const (
	AttemptLeased    AttemptStatus = "leased"
	AttemptRunning   AttemptStatus = "running"
	AttemptSucceeded AttemptStatus = "succeeded"
	AttemptFailed    AttemptStatus = "failed"
	AttemptExpired   AttemptStatus = "expired"
	AttemptCancelled AttemptStatus = "cancelled"
)

// Worker liveness as observed by Core. Unknown workers have no record at
// all; offline means registered but heartbeat overdue or never seen.
type WorkerStatus string

const (
	WorkerOnline  WorkerStatus = "online"
	WorkerOffline WorkerStatus = "offline"
)

// Synthetic workload bounds: the only S2-authorized execution. Inputs are
// validated at the domain boundary, never in handlers or storage.
const (
	SyntheticEchoKind  = "synthetic-echo"
	MaxWorkloadBytes   = 4096
	MaxWorkloadMessage = 256
	MaxWorkloadSteps   = 10
	MaxWorkloadStepMs  = 200
	MaxWorkerIDLength  = 64
	MaxReasonLength    = 500
	MaxOutputBytes     = 4096
	MaxAttemptsLimit   = 5
	MinTimeoutSecs     = 5
	MaxTimeoutSecs     = 600
	DefaultMaxAttempts = 3
	DefaultTimeoutSecs = 120
	DefaultLeaseTTL    = 10 * time.Second
	WorkerOfflineAfter = 30 * time.Second
	MaxMessageKeys     = 3
)

// Run is the S2 durable execution record. Retries create new Attempts;
// this row is the single authoritative outcome for the Run.
type Run struct {
	ID              string         `json:"id"`
	ProjectID       string         `json:"projectId"`
	ActionID        string         `json:"actionId,omitempty"`
	WorkloadKind    string         `json:"workloadKind"`
	WorkloadInput   map[string]any `json:"workloadInput"`
	Status          RunStatus      `json:"status"`
	AttemptCount    int            `json:"attemptCount"`
	MaxAttempts     int            `json:"maxAttempts"`
	TimeoutSecs     int            `json:"timeoutSecs"`
	LeaseOwner      string         `json:"leaseOwner,omitempty"`
	LeaseGeneration int64          `json:"leaseGeneration"`
	LeaseExpiresAt  time.Time      `json:"leaseExpiresAt,omitempty"`
	Result          map[string]any `json:"result,omitempty"`
	Error           string         `json:"error,omitempty"`
	// S3-A: requested execution mode and pinned Paseo routing. Empty for
	// S2 synthetic runs (restricted). TRUSTED_LOCAL runs carry the exact
	// provider/model/mode the Worker must assert from session evidence.
	ExecutionMode string `json:"executionMode"`
	Provider      string `json:"provider,omitempty"`
	Model         string `json:"model,omitempty"`
	PaseoMode     string `json:"paseoMode,omitempty"`
	// S4-A: frozen ActionSpec bound to this Run. Empty for legacy S2/S3
	// runs without an engineering specification.
	SpecID string `json:"specId,omitempty"`
	// S4-C: immutable capability requirements and Project binding snapshot
	// derived from the frozen spec by the Core, never by the Worker.
	Capabilities    []Capability `json:"capabilities"`
	SpecBindingHash string       `json:"specBindingHash,omitempty"`
	CreatedAt       time.Time    `json:"createdAt"`
	UpdatedAt       time.Time    `json:"updatedAt"`
}

// Attempt is one lease grant for a Run. History is append-only.
type Attempt struct {
	ID              string         `json:"id"`
	RunID           string         `json:"runId"`
	Seq             int            `json:"seq"`
	WorkerID        string         `json:"workerId"`
	LeaseGeneration int64          `json:"leaseGeneration"`
	Status          AttemptStatus  `json:"status"`
	Heartbeats      int            `json:"heartbeats"`
	Result          map[string]any `json:"result,omitempty"`
	Error           string         `json:"error,omitempty"`
	CreatedAt       time.Time      `json:"createdAt"`
	UpdatedAt       time.Time      `json:"updatedAt"`
}

// Worker is Core's observation of one registered Worker process.
type Worker struct {
	ID            string       `json:"id"`
	Version       string       `json:"version,omitempty"`
	Status        WorkerStatus `json:"status"`
	LastHeartbeat time.Time    `json:"lastHeartbeat,omitempty"`
	LastRunID     string       `json:"lastRunId,omitempty"`
	Capabilities  []Capability `json:"capabilities"`
	ExecModes     []string     `json:"execModes"`
	CreatedAt     time.Time    `json:"createdAt"`
	UpdatedAt     time.Time    `json:"updatedAt"`
}

// NewRunParams carries validated operator input for queueing a Run.
type NewRunParams struct {
	ProjectID     string
	ActionID      string
	WorkloadKind  string
	WorkloadInput map[string]any
	MaxAttempts   int
	TimeoutSecs   int
	// S3-A: requested execution mode and Paseo routing. Empty means the
	// S2 default (restricted synthetic execution).
	ExecutionMode string
	Provider      string
	Model         string
	PaseoMode     string
	// S4-A: when set, the Core derives the Action, project, task text,
	// routing and execution budget from the frozen spec and ignores
	// conflicting caller values.
	SpecID string
}

// ValidateWorkloadInput enforces the authorized workload bounds so untrusted
// input cannot smuggle arbitrary operations past the domain boundary. S2
// authorizes only synthetic-echo; S3-A additionally authorizes paseo-task
// under TRUSTED_LOCAL routing (validated in NewRunParams.Validate).
func ValidateWorkloadInput(kind string, input map[string]any) (map[string]any, error) {
	switch kind {
	case SyntheticEchoKind:
		return validateSyntheticInput(input)
	case PaseoTaskKind:
		return validatePaseoTaskInput(input)
	default:
		return nil, fmt.Errorf("unsupported workloadKind %q: only %q and %q are authorized",
			kind, SyntheticEchoKind, PaseoTaskKind)
	}
}

func validateSyntheticInput(input map[string]any) (map[string]any, error) {
	if input == nil {
		return nil, errors.New("workloadInput is required")
	}
	if len(input) > MaxMessageKeys {
		return nil, fmt.Errorf("workloadInput carries at most %d keys", MaxMessageKeys)
	}
	out := make(map[string]any, len(input))
	for k, v := range input {
		switch k {
		case "message", "steps", "stepMs":
			out[k] = v
		default:
			return nil, fmt.Errorf("unknown workloadInput key %q", k)
		}
	}
	msg, _ := out["message"].(string)
	msg = strings.TrimSpace(msg)
	if msg == "" {
		return nil, errors.New("workloadInput.message is required")
	}
	if len(msg) > MaxWorkloadMessage {
		return nil, fmt.Errorf("workloadInput.message exceeds %d characters", MaxWorkloadMessage)
	}
	out["message"] = msg
	steps, err := workloadInt(out["steps"], 1)
	if err != nil || steps < 1 || steps > MaxWorkloadSteps {
		return nil, fmt.Errorf("workloadInput.steps must be an integer 1..%d", MaxWorkloadSteps)
	}
	out["steps"] = steps
	stepMs, err := workloadInt(out["stepMs"], 0)
	if err != nil || stepMs < 0 || stepMs > MaxWorkloadStepMs {
		return nil, fmt.Errorf("workloadInput.stepMs must be an integer 0..%d", MaxWorkloadStepMs)
	}
	out["stepMs"] = stepMs
	return out, nil
}

func workloadInt(v any, def int) (int, error) {
	if v == nil {
		return def, nil
	}
	switch n := v.(type) {
	case int:
		return n, nil
	case int64:
		return int(n), nil
	case float64:
		if n != float64(int(n)) {
			return 0, errors.New("not an integer")
		}
		return int(n), nil
	default:
		return 0, errors.New("not an integer")
	}
}

// Validate normalizes and checks Run creation input.
func (p NewRunParams) Validate() (NewRunParams, error) {
	if p.ProjectID == "" {
		p.ProjectID = DefaultProjectID
	}
	if err := ValidateProjectID(p.ProjectID); err != nil {
		return p, err
	}
	if p.ActionID != "" && len(p.ActionID) != 32 {
		return p, errors.New("actionId must be a 32-character hex id when present")
	}
	if p.SpecID != "" && (len(p.SpecID) != 32 || !isLowerHex(p.SpecID)) {
		return p, errors.New("specId must be a 32-character lowercase hex id when present")
	}
	if p.ExecutionMode == "" {
		p.ExecutionMode = string(ExecRestricted)
	}
	if err := ValidateExecutionMode(p.ExecutionMode); err != nil {
		return p, err
	}
	if p.SpecID != "" && p.WorkloadKind != PaseoTaskKind {
		return p, errors.New("specId requires workloadKind paseo-task")
	}
	switch p.WorkloadKind {
	case SyntheticEchoKind:
		if p.ExecutionMode != string(ExecRestricted) {
			return p, fmt.Errorf("workloadKind %q requires executionMode %q",
				SyntheticEchoKind, ExecRestricted)
		}
		if p.Provider != "" || p.Model != "" || p.PaseoMode != "" {
			return p, errors.New("provider/model/paseoMode are only valid for paseo-task runs")
		}
	case PaseoTaskKind:
		if p.ExecutionMode != string(ExecTrustedLocal) {
			return p, fmt.Errorf("workloadKind %q requires executionMode %q",
				PaseoTaskKind, ExecTrustedLocal)
		}
		route, err := ValidatePaseoRoute(p.Provider, p.Model, p.PaseoMode)
		if err != nil {
			return p, err
		}
		p.Provider = route.Provider
		p.Model = route.Model
		p.PaseoMode = route.Mode
	default:
		return p, fmt.Errorf("unsupported workloadKind %q: only %q and %q are authorized",
			p.WorkloadKind, SyntheticEchoKind, PaseoTaskKind)
	}
	input, err := ValidateWorkloadInput(p.WorkloadKind, p.WorkloadInput)
	if err != nil {
		return p, err
	}
	p.WorkloadInput = input
	if p.MaxAttempts == 0 {
		p.MaxAttempts = DefaultMaxAttempts
	}
	if p.MaxAttempts < 1 || p.MaxAttempts > MaxAttemptsLimit {
		return p, fmt.Errorf("maxAttempts must be 1..%d", MaxAttemptsLimit)
	}
	if p.TimeoutSecs == 0 {
		p.TimeoutSecs = DefaultTimeoutSecs
	}
	if p.TimeoutSecs < MinTimeoutSecs || p.TimeoutSecs > MaxTimeoutSecs {
		return p, fmt.Errorf("timeoutSecs must be %d..%d", MinTimeoutSecs, MaxTimeoutSecs)
	}
	return p, nil
}

// ValidateWorkerID enforces the worker slug shape.
func ValidateWorkerID(id string) error {
	id = strings.TrimSpace(id)
	if id == "" {
		return errors.New("workerId is required")
	}
	if len(id) > MaxWorkerIDLength {
		return fmt.Errorf("workerId exceeds %d characters", MaxWorkerIDLength)
	}
	for _, r := range id {
		ok := r >= 'a' && r <= 'z' || r >= '0' && r <= '9' || r == '-' || r == '_'
		if !ok {
			return errors.New("workerId must match [a-z0-9-_]")
		}
	}
	return nil
}

// Terminal reports whether a Run status ends the Run's lifecycle.
func (s RunStatus) Terminal() bool {
	return s == RunSucceeded || s == RunFailed || s == RunCancelled
}

// Terminal reports whether an Attempt status ends the Attempt's lifecycle.
func (s AttemptStatus) Terminal() bool {
	return s == AttemptSucceeded || s == AttemptFailed || s == AttemptExpired || s == AttemptCancelled
}

// ParseRunStatus maps an external string to a Run status.
func ParseRunStatus(s string) (RunStatus, error) {
	st := RunStatus(strings.TrimSpace(s))
	switch st {
	case RunQueued, RunLeased, RunRunning, RunSucceeded, RunFailed, RunCancelled:
		return st, nil
	default:
		return "", fmt.Errorf("unknown run status %q", s)
	}
}

// ReportOutcome is the Worker's declared result for one Attempt.
type ReportOutcome string

const (
	OutcomeCompleted ReportOutcome = "completed"
	OutcomeFailed    ReportOutcome = "failed"
)

// ParseOutcome maps an external string to a report outcome.
func ParseOutcome(s string) (ReportOutcome, error) {
	o := ReportOutcome(strings.TrimSpace(s))
	switch o {
	case OutcomeCompleted, OutcomeFailed:
		return o, nil
	default:
		return "", fmt.Errorf("unknown outcome %q", s)
	}
}

// SanitizeResult bounds a Worker-supplied result/output object so history
// stays small and cannot carry credentials by size or shape.
func SanitizeResult(v map[string]any) (map[string]any, error) {
	if v == nil {
		return map[string]any{}, nil
	}
	if len(v) > 28 {
		return nil, errors.New("result carries at most 28 keys")
	}
	out := make(map[string]any, len(v))
	for k, val := range v {
		if len(k) > 64 {
			return nil, errors.New("result key exceeds 64 characters")
		}
		switch t := val.(type) {
		case string:
			if len(t) > 1024 {
				return nil, fmt.Errorf("result value for %q exceeds 1024 characters", k)
			}
			out[k] = val
		case bool, float64, int, int64, nil:
			out[k] = val
		default:
			return nil, fmt.Errorf("result value for %q has unsupported type", k)
		}
	}
	return out, nil
}

// SanitizeReason bounds a Worker/Core failure reason.
func SanitizeReason(s string) string {
	s = strings.TrimSpace(s)
	if len(s) > MaxReasonLength {
		s = s[:MaxReasonLength]
	}
	return s
}
