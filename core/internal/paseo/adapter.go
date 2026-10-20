// Package paseo is the minimal Go-to-Paseo integration for Factory S3-B.
//
// It covers exactly the TRUSTED_LOCAL workflow proven by
// scripts/paseo-trusted-local-smoke.sh: create a real Paseo workspace rooted
// at an Attempt workspace, start one session with the pinned
// provider/model/mode, observe its identity and timeline, and stop/archive
// it. It never re-implements Paseo's agent runtime, tool execution, model
// routing or session persistence.
//
// All operations run the supported installed `paseo` CLI as a subprocess
// with explicit timeouts and context cancellation. Raw CLI output is never
// embedded in errors, logs or evidence: failures report only the operation,
// the exit status and a redacted shape (top-level keys, never values).
package paseo

import (
	"context"
	"fmt"
	"time"
)

// Operation bounds for the real CLI. The session run itself is bounded by
// the caller's StartRequest.WaitTimeout; every other call has a fixed
// ceiling so a hung daemon cannot stall the Worker lease loop forever.
const (
	CreateWorkspaceTimeout = 60 * time.Second
	InspectTimeout         = 30 * time.Second
	LogsTimeout            = 30 * time.Second
	SendTimeout            = 30 * time.Second
	StopTimeout            = 60 * time.Second
	ArchiveTimeout         = 60 * time.Second
	ProbeTimeout           = 15 * time.Second
)

// FailureKind classifies adapter errors without carrying external output.
type FailureKind string

const (
	// KindStartup means Paseo could not start or reach the requested state
	// (missing CLI, daemon unreachable, nonzero exit before identity).
	KindStartup FailureKind = "startup"
	// KindIdentity means observed session evidence did not match the
	// requested route or workspace (provider/model/mode/Cwd mismatch or
	// missing identity). Never a silent substitution.
	KindIdentity FailureKind = "identity"
	// KindTimeout means an explicit operation bound elapsed.
	KindTimeout FailureKind = "timeout"
	// KindCancelled means the caller's context ended (operator cancel or
	// Worker shutdown): external session state is unconfirmed.
	KindCancelled FailureKind = "cancelled"
	// KindExternal means a Paseo-side failure after a session existed
	// (stop/archive/observe failed with the session identity known).
	KindExternal FailureKind = "external"
)

// Error is a structured adapter failure. It carries the operation, the
// classification and a bounded redacted reason — never CLI output bodies,
// prompts, tokens or session transcripts.
type Error struct {
	Op     string
	Kind   FailureKind
	Reason string
}

func (e *Error) Error() string {
	return fmt.Sprintf("paseo %s failed (%s): %s", e.Op, e.Kind, e.Reason)
}

// Identity is the proven external session identity observed from Paseo
// evidence. WorkspaceID comes from the workspace-create call; the rest is
// asserted from `paseo inspect` on every start and resume.
type Identity struct {
	WorkspaceID string
	SessionID   string
	Provider    string
	Model       string
	Mode        string
	Cwd         string
	Status      string
}

// StartRequest pins one real Paseo session. Provider/Model/Mode must be an
// exact S3 allowlist triple; the Worker freezes them from the Run before
// calling StartSession and asserts them from evidence afterwards.
type StartRequest struct {
	Provider    string
	Model       string
	Mode        string
	WorkspaceID string
	Title       string
	Prompt      string
	WaitTimeout time.Duration
}

// TimelineSummary is the bounded session observation. Step bodies and the
// raw transcript are never retained: only line/step counts cross into
// Factory evidence, so prompts and tool output cannot leak credentials or
// source bodies into the event store.
type TimelineSummary struct {
	Lines     int
	Steps     map[string]int
	ToolSteps int
}

// Adapter is the narrow Paseo surface the Worker needs. Implementations
// must honor context cancellation (abandoning in-flight subprocess calls)
// and must never substitute routes or fabricate identities.
type Adapter interface {
	// CreateWorkspace creates a real local Paseo workspace rooted at path.
	CreateWorkspace(ctx context.Context, path, title string) (string, error)
	// StartSession starts one session in workspaceID and waits (bounded by
	// req.WaitTimeout) for its first turn to settle. The returned
	// Identity is parsed from Paseo evidence, not echoed from the request.
	StartSession(ctx context.Context, req StartRequest) (Identity, error)
	// InspectSession returns the current observed identity of sessionID.
	InspectSession(ctx context.Context, sessionID string) (Identity, error)
	// ObserveTimeline returns the redacted step summary of sessionID.
	ObserveTimeline(ctx context.Context, sessionID string) (TimelineSummary, error)
	// SendNudge delivers one bounded continuation message and waits for
	// the session to settle again. At most one nudge per Attempt.
	SendNudge(ctx context.Context, sessionID, message string, timeout time.Duration) error
	// WaitIdle waits (bounded) for sessionID to become idle.
	WaitIdle(ctx context.Context, sessionID string, timeout time.Duration) error
	// StopSession interrupts sessionID (no-op when already idle).
	StopSession(ctx context.Context, sessionID string) error
	// ArchiveSession soft-deletes sessionID.
	ArchiveSession(ctx context.Context, sessionID string) error
	// ArchiveWorkspace archives workspaceID and everything it owns.
	ArchiveWorkspace(ctx context.Context, workspaceID string) error
	// WorkspaceListed reports whether workspaceID is still listed
	// (archive verification).
	WorkspaceListed(ctx context.Context, workspaceID string) (bool, error)
}
