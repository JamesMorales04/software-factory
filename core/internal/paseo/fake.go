package paseo

import (
	"context"
	"fmt"
	"strings"
	"sync"
	"time"
)

// FakeAdapter is a deterministic in-memory Adapter for unit/contract tests.
// No subprocess, no daemon, no paid sessions. Every operation is scripted
// through exported fields; every call is recorded for assertions.
type FakeAdapter struct {
	mu sync.Mutex
	// Scripted identities and observations.
	WorkspaceID string
	Session     Identity
	Timeline    TimelineSummary
	Listed      map[string]bool
	Sessions    []SessionRef
	Workspaces  []WorkspaceRef
	// StartLabels records the labels of every StartSession request.
	StartLabels []map[string]string
	// Scripted failures by operation name (create, run, inspect, logs,
	// send, wait, stop, archive, workspace-archive, workspace-ls).
	Fail map[string]error
	// StartErr, when set, is returned by StartSession alongside the
	// scripted Session identity (mirroring the real adapter, which
	// returns the observed identity with a mismatch error).
	StartErr error
	// Calls records operation names in order.
	Calls []string
	// Nudges records SendNudge messages.
	Nudges []string
}

// NewFakeAdapter returns a FakeAdapter with sane defaults.
func NewFakeAdapter() *FakeAdapter {
	return &FakeAdapter{
		WorkspaceID: "ws_fake_1",
		Session: Identity{
			WorkspaceID: "ws_fake_1",
			SessionID:   "aaaaaaaa-1111-4111-8111-aaaaaaaaaaa1",
			Provider:    "codex",
			Model:       "gpt-6-luna",
			Mode:        "full-access",
			Cwd:         "/tmp/fake-proj",
			Status:      "idle",
		},
		Timeline: TimelineSummary{Lines: 11, Steps: map[string]int{"Edit": 1, "Shell": 3}, ToolSteps: 4},
		Listed:   map[string]bool{},
		Fail:     map[string]error{},
	}
}

func (f *FakeAdapter) record(op string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.Calls = append(f.Calls, op)
	if err, ok := f.Fail[op]; ok {
		return err
	}
	return nil
}

// Called reports whether op was invoked.
func (f *FakeAdapter) Called(op string) bool {
	f.mu.Lock()
	defer f.mu.Unlock()
	for _, c := range f.Calls {
		if c == op {
			return true
		}
	}
	return false
}

func (f *FakeAdapter) CreateWorkspace(_ context.Context, _, _ string) (string, error) {
	if err := f.record("create"); err != nil {
		return "", err
	}
	return f.WorkspaceID, nil
}

func (f *FakeAdapter) StartSession(_ context.Context, req StartRequest) (Identity, error) {
	if err := f.record("run"); err != nil {
		return Identity{}, err
	}
	f.mu.Lock()
	f.StartLabels = append(f.StartLabels, req.Labels)
	f.mu.Unlock()
	out := f.Session
	out.WorkspaceID = req.WorkspaceID
	if out.Cwd == "" {
		out.Cwd = "/tmp/fake-proj"
	}
	if out.Title == "" {
		out.Title = req.Title
	}
	if f.StartErr != nil {
		return out, f.StartErr
	}
	return out, nil
}

func (f *FakeAdapter) InspectSession(_ context.Context, sessionID string) (Identity, error) {
	if err := f.record("inspect"); err != nil {
		return Identity{}, err
	}
	out := f.Session
	out.SessionID = sessionID
	return out, nil
}

func (f *FakeAdapter) ListSessions(ctx context.Context) ([]SessionRef, error) {
	return f.ListSessionsByLabels(ctx, nil)
}

func (f *FakeAdapter) ListSessionsByLabels(_ context.Context, labels map[string]string) ([]SessionRef, error) {
	if err := f.record("ls"); err != nil {
		return nil, err
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	out := make([]SessionRef, 0, len(f.Sessions))
	for _, session := range f.Sessions {
		if labelsMatch(session.Labels, labels) {
			out = append(out, session)
		}
	}
	return out, nil
}

func (f *FakeAdapter) ListWorkspaces(_ context.Context) ([]WorkspaceRef, error) {
	if err := f.record("workspace-ls"); err != nil {
		return nil, err
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	out := make([]WorkspaceRef, 0, len(f.Workspaces))
	for _, ws := range f.Workspaces {
		if f.Listed[ws.ID] {
			out = append(out, ws)
		}
	}
	return out, nil
}

func labelsMatch(have, want map[string]string) bool {
	for key, value := range want {
		if have[key] != value {
			return false
		}
	}
	return true
}

func (f *FakeAdapter) ObserveTimeline(_ context.Context, _ string) (TimelineSummary, error) {
	if err := f.record("logs"); err != nil {
		return TimelineSummary{}, err
	}
	steps := make(map[string]int, len(f.Timeline.Steps))
	for k, v := range f.Timeline.Steps {
		steps[k] = v
	}
	return TimelineSummary{Lines: f.Timeline.Lines, Steps: steps, ToolSteps: f.Timeline.ToolSteps}, nil
}

func (f *FakeAdapter) SendNudge(_ context.Context, _, message string, _ time.Duration) error {
	if err := f.record("send"); err != nil {
		return err
	}
	f.mu.Lock()
	f.Nudges = append(f.Nudges, message)
	f.mu.Unlock()
	return nil
}

func (f *FakeAdapter) WaitIdle(_ context.Context, _ string, _ time.Duration) error {
	return f.record("wait")
}

func (f *FakeAdapter) StopSession(_ context.Context, _ string) error {
	return f.record("stop")
}

func (f *FakeAdapter) ArchiveSession(_ context.Context, sessionID string) error {
	if err := f.record("archive"); err != nil {
		return err
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.Session.SessionID == "" || f.Session.SessionID == sessionID {
		f.Session.SessionID = sessionID
		f.Session.Archived = true
	}
	return nil
}

func (f *FakeAdapter) ArchiveWorkspace(_ context.Context, workspaceID string) error {
	if err := f.record("workspace-archive"); err != nil {
		return err
	}
	f.mu.Lock()
	delete(f.Listed, workspaceID)
	f.mu.Unlock()
	return nil
}

func (f *FakeAdapter) WorkspaceListed(_ context.Context, workspaceID string) (bool, error) {
	if err := f.record("workspace-ls"); err != nil {
		return false, err
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.Listed[workspaceID], nil
}

// ScriptMismatch returns a FakeAdapter whose session evidence disagrees
// with the requested route (for mismatch-path tests).
func ScriptMismatch() *FakeAdapter {
	f := NewFakeAdapter()
	f.Session.Provider = "opencode"
	f.Session.Model = "opencode-go/deepseek-v4.1-flash"
	f.Session.Mode = "build"
	return f
}

// FailOp scripts op to fail with a classified adapter error.
func FailOp(op, reason string) error {
	return &Error{Op: op, Kind: KindStartup, Reason: strings.TrimSpace(reason)}
}

// Compile-time check: the fake must satisfy the interface the Worker uses.
var _ Adapter = (*FakeAdapter)(nil)

// Compile-time check: the real CLI must satisfy the same interface.
var _ Adapter = (*CLIAdapter)(nil)

// FakeError formats a deterministic failure reason for negative tests.
func FakeError(op string, kind FailureKind, reason string) *Error {
	return &Error{Op: op, Kind: kind, Reason: fmt.Sprintf("fake %s: %s", op, reason)}
}
