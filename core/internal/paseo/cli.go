package paseo

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"maps"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/telemetry"
)

// CLIAdapter drives the installed `paseo` CLI. It implements exactly the
// smoke-proven invocation pattern (workspace create --isolation local;
// run --provider --workspace --mode --model --wait-timeout; inspect;
// logs; send --no-wait + wait; stop; archive; workspace archive) and
// nothing else.
type CLIAdapter struct {
	bin    string
	logger *slog.Logger
}

// Option configures a CLIAdapter.
type Option func(*CLIAdapter)

// WithBinary overrides the paseo executable lookup (tests, custom homes).
func WithBinary(path string) Option {
	return func(a *CLIAdapter) {
		if path != "" {
			a.bin = path
		}
	}
}

// NewCLIAdapter builds an adapter around the installed CLI. The binary is
// resolved from PASEO_BIN when set, otherwise from PATH. Availability is
// checked lazily by Probe so construction never shells out.
func NewCLIAdapter(logger *slog.Logger, opts ...Option) *CLIAdapter {
	a := &CLIAdapter{bin: "paseo", logger: logger}
	for _, opt := range opts {
		opt(a)
	}
	if env, ok := os.LookupEnv("PASEO_BIN"); ok && strings.TrimSpace(env) != "" && a.bin == "paseo" {
		a.bin = strings.TrimSpace(env)
	}
	return a
}

// Probe reports the installed CLI version (names only, no secret values).
func (a *CLIAdapter) Probe(ctx context.Context) (string, error) {
	ctx, cancel := context.WithTimeout(ctx, ProbeTimeout)
	defer cancel()
	out, err := a.run(ctx, "--version")
	if err != nil {
		return "", err
	}
	version := strings.TrimSpace(string(out))
	if version == "" {
		return "", &Error{Op: "version", Kind: KindStartup, Reason: "empty version output"}
	}
	return version, nil
}

// CreateWorkspace creates a real local workspace rooted at path.
func (a *CLIAdapter) CreateWorkspace(ctx context.Context, path, title string) (string, error) {
	abs, err := filepath.Abs(path)
	if err != nil {
		return "", &Error{Op: "workspace-create", Kind: KindStartup, Reason: "unresolvable path"}
	}
	ctx, cancel := context.WithTimeout(ctx, CreateWorkspaceTimeout)
	defer cancel()
	args := []string{"workspace", "create", "--isolation", "local", "--path", abs, "--json"}
	if strings.TrimSpace(title) != "" {
		args = append(args, "--title", strings.TrimSpace(title))
	}
	out, err := a.run(ctx, args...)
	if err != nil {
		return "", err
	}
	id, keys, perr := parseOpaqueID(out, "workspaceId", "id")
	if perr != nil {
		return "", &Error{Op: "workspace-create", Kind: KindIdentity, Reason: "unparseable workspace identity (keys: " + keys + ")"}
	}
	if verr := domain.ValidatePaseoIdentity("workspaceId", id); verr != nil {
		return "", &Error{Op: "workspace-create", Kind: KindIdentity, Reason: "workspace identity failed shape validation"}
	}
	return id, nil
}

// StartSession starts one session and waits (bounded) for its first turn.
func (a *CLIAdapter) StartSession(ctx context.Context, req StartRequest) (Identity, error) {
	if strings.TrimSpace(req.WorkspaceID) == "" {
		return Identity{}, &Error{Op: "run", Kind: KindStartup, Reason: "workspace is required"}
	}
	if strings.TrimSpace(req.Prompt) == "" {
		return Identity{}, &Error{Op: "run", Kind: KindStartup, Reason: "prompt is required"}
	}
	if req.WaitTimeout <= 0 {
		return Identity{}, &Error{Op: "run", Kind: KindStartup, Reason: "wait timeout must be positive"}
	}
	if _, err := domain.ValidatePaseoRoute(req.Provider, req.Model, req.Mode); err != nil {
		return Identity{}, &Error{Op: "run", Kind: KindIdentity, Reason: "request is not an authorized route"}
	}
	// Upper bound even when the caller passes an unbounded context: the
	// parent deadline (lease budget) still applies, this only caps a
	// forgotten one. Grace covers stop/archive later, not the session.
	ctx, cancel := context.WithTimeout(ctx, req.WaitTimeout+2*time.Minute)
	defer cancel()
	wait := formatWaitTimeout(req.WaitTimeout)
	args := []string{
		"run",
		"--provider", strings.TrimSpace(req.Provider),
		"--workspace", strings.TrimSpace(req.WorkspaceID),
		"--mode", strings.TrimSpace(req.Mode),
		"--model", strings.TrimSpace(req.Model),
		"--wait-timeout", wait,
		"--json",
	}
	if strings.TrimSpace(req.Title) != "" {
		args = append(args, "--title", strings.TrimSpace(req.Title))
	}
	for _, key := range sortedLabelKeys(req.Labels) {
		args = append(args, "--label", key+"="+req.Labels[key])
	}
	args = append(args, req.Prompt)
	out, err := a.run(ctx, args...)
	if err != nil {
		return Identity{}, err
	}
	sessionID, keys, perr := parseOpaqueID(out, "id", "agentId")
	if perr != nil {
		return Identity{}, &Error{Op: "run", Kind: KindIdentity, Reason: "unparseable session identity (keys: " + keys + ")"}
	}
	if verr := domain.ValidatePaseoIdentity("sessionId", sessionID); verr != nil {
		return Identity{}, &Error{Op: "run", Kind: KindIdentity, Reason: "session identity failed shape validation"}
	}
	observed, err := a.InspectSession(ctx, sessionID)
	if err != nil {
		return Identity{}, err
	}
	observed.WorkspaceID = strings.TrimSpace(req.WorkspaceID)
	observed.SessionID = sessionID
	if aerr := AssertRoute(req.Provider, req.Model, req.Mode, "", observed); aerr != nil {
		// Return the observed identity alongside the mismatch: a
		// session exists externally but is unproven, so the caller
		// must orphan it rather than claim a clean startup failure.
		return observed, aerr
	}
	return observed, nil
}

// InspectSession returns the current observed identity of sessionID.
func (a *CLIAdapter) InspectSession(ctx context.Context, sessionID string) (Identity, error) {
	if verr := domain.ValidatePaseoIdentity("sessionId", sessionID); verr != nil {
		return Identity{}, &Error{Op: "inspect", Kind: KindStartup, Reason: "invalid session identity shape"}
	}
	ctx, cancel := context.WithTimeout(ctx, InspectTimeout)
	defer cancel()
	out, err := a.run(ctx, "inspect", sessionID, "--json")
	if err != nil {
		return Identity{}, err
	}
	obj, keys, perr := firstObject(out)
	if perr != nil {
		return Identity{}, &Error{Op: "inspect", Kind: KindIdentity, Reason: "unparseable inspect output (keys: " + keys + ")"}
	}
	observed := Identity{
		SessionID: sessionID,
		Provider:  normalizeProvider(ciString(obj, "provider", "providerId")),
		Model:     strings.TrimSpace(ciString(obj, "model")),
		Mode:      strings.TrimSpace(ciString(obj, "mode")),
		Cwd:       strings.TrimSpace(ciString(obj, "cwd")),
		Status:    strings.TrimSpace(ciString(obj, "status")),
		Title:     strings.TrimSpace(ciString(obj, "name", "title")),
		Archived:  ciBool(obj, "archived"),
	}
	if observed.Provider == "" || observed.Model == "" {
		return Identity{}, &Error{Op: "inspect", Kind: KindIdentity, Reason: "session evidence missing provider or model"}
	}
	return observed, nil
}

// ObserveTimeline returns the redacted step summary of sessionID. `paseo
// logs` emits human-readable timeline text, so only bracketed step headers
// and line counts are summarized; bodies are never retained.
func (a *CLIAdapter) ObserveTimeline(ctx context.Context, sessionID string) (TimelineSummary, error) {
	if verr := domain.ValidatePaseoIdentity("sessionId", sessionID); verr != nil {
		return TimelineSummary{}, &Error{Op: "logs", Kind: KindStartup, Reason: "invalid session identity shape"}
	}
	ctx, cancel := context.WithTimeout(ctx, LogsTimeout)
	defer cancel()
	out, err := a.run(ctx, "logs", sessionID)
	if err != nil {
		return TimelineSummary{}, err
	}
	summary := TimelineSummary{Steps: map[string]int{}}
	for _, line := range strings.Split(string(out), "\n") {
		if strings.TrimSpace(line) == "" {
			continue
		}
		summary.Lines++
		if step := stepHeader.FindStringSubmatch(line); step != nil {
			name := step[1]
			summary.Steps[name]++
			if toolStep[name] {
				summary.ToolSteps++
			}
		}
	}
	return summary, nil
}

var stepHeader = regexp.MustCompile(`^\[([A-Za-z]+)\]`)

// toolStep marks timeline step types that prove the agent acted with
// tools (reads, edits, shells), as opposed to text-only turns.
var toolStep = map[string]bool{
	"Read": true, "Glob": true, "Grep": true, "List": true,
	"Shell": true, "Edit": true, "Write": true, "Create": true, "Apply": true,
}

// SendNudge delivers one bounded continuation message without waiting, then
// waits (bounded) for the session to settle again.
func (a *CLIAdapter) SendNudge(ctx context.Context, sessionID, message string, timeout time.Duration) error {
	if verr := domain.ValidatePaseoIdentity("sessionId", sessionID); verr != nil {
		return &Error{Op: "send", Kind: KindStartup, Reason: "invalid session identity shape"}
	}
	if strings.TrimSpace(message) == "" {
		return &Error{Op: "send", Kind: KindStartup, Reason: "message is required"}
	}
	if timeout <= 0 {
		return &Error{Op: "send", Kind: KindStartup, Reason: "wait timeout must be positive"}
	}
	ctx, cancel := context.WithTimeout(ctx, SendTimeout+timeout+10*time.Second)
	defer cancel()
	if _, err := a.run(ctx, "send", "--no-wait", sessionID, message); err != nil {
		return err
	}
	return a.WaitIdle(ctx, sessionID, timeout)
}

// WaitIdle waits (bounded) for sessionID to become idle.
func (a *CLIAdapter) WaitIdle(ctx context.Context, sessionID string, timeout time.Duration) error {
	if verr := domain.ValidatePaseoIdentity("sessionId", sessionID); verr != nil {
		return &Error{Op: "wait", Kind: KindStartup, Reason: "invalid session identity shape"}
	}
	if timeout <= 0 {
		return &Error{Op: "wait", Kind: KindStartup, Reason: "wait timeout must be positive"}
	}
	ctx, cancel := context.WithTimeout(ctx, timeout+10*time.Second)
	defer cancel()
	secs := int(timeout / time.Second)
	if secs < 1 {
		secs = 1
	}
	_, err := a.run(ctx, "wait", "--timeout", fmt.Sprint(secs), sessionID)
	return err
}

// StopSession interrupts sessionID (no-op when already idle).
func (a *CLIAdapter) StopSession(ctx context.Context, sessionID string) error {
	if verr := domain.ValidatePaseoIdentity("sessionId", sessionID); verr != nil {
		return &Error{Op: "stop", Kind: KindStartup, Reason: "invalid session identity shape"}
	}
	ctx, cancel := context.WithTimeout(ctx, StopTimeout)
	defer cancel()
	_, err := a.run(ctx, "stop", sessionID)
	return err
}

// ArchiveSession soft-deletes sessionID.
func (a *CLIAdapter) ArchiveSession(ctx context.Context, sessionID string) error {
	if verr := domain.ValidatePaseoIdentity("sessionId", sessionID); verr != nil {
		return &Error{Op: "archive", Kind: KindStartup, Reason: "invalid session identity shape"}
	}
	ctx, cancel := context.WithTimeout(ctx, ArchiveTimeout)
	defer cancel()
	_, err := a.run(ctx, "archive", sessionID)
	return err
}

// ArchiveWorkspace archives workspaceID and everything it owns.
func (a *CLIAdapter) ArchiveWorkspace(ctx context.Context, workspaceID string) error {
	if verr := domain.ValidatePaseoIdentity("workspaceId", workspaceID); verr != nil {
		return &Error{Op: "workspace-archive", Kind: KindStartup, Reason: "invalid workspace identity shape"}
	}
	ctx, cancel := context.WithTimeout(ctx, ArchiveTimeout)
	defer cancel()
	_, err := a.run(ctx, "workspace", "archive", workspaceID)
	return err
}

// ListSessions returns global agents including archived ones.
func (a *CLIAdapter) ListSessions(ctx context.Context) ([]SessionRef, error) {
	return a.listSessions(ctx, nil)
}

// ListSessionsByLabels filters global agents (including archived) by every
// exact label pair. An empty result is an authoritative "no matching agent"
// only when the CLI call itself succeeded.
func (a *CLIAdapter) ListSessionsByLabels(ctx context.Context, labels map[string]string) ([]SessionRef, error) {
	return a.listSessions(ctx, labels)
}

func (a *CLIAdapter) listSessions(ctx context.Context, labels map[string]string) ([]SessionRef, error) {
	ctx, cancel := context.WithTimeout(ctx, InspectTimeout)
	defer cancel()
	args := []string{"ls", "-g", "-a", "--json"}
	for _, key := range sortedLabelKeys(labels) {
		args = append(args, "--label", key+"="+labels[key])
	}
	out, err := a.run(ctx, args...)
	if err != nil {
		return nil, err
	}
	docs, err := decodeObjectList(out)
	if err != nil {
		return nil, &Error{Op: "ls", Kind: KindIdentity, Reason: "unparseable agent listing"}
	}
	refs := make([]SessionRef, 0, len(docs))
	for _, doc := range docs {
		refs = append(refs, SessionRef{
			ID:       strings.TrimSpace(ciString(doc, "id", "agentId")),
			Title:    strings.TrimSpace(ciString(doc, "name", "title")),
			Provider: normalizeProvider(ciString(doc, "provider", "providerId")),
			Status:   strings.TrimSpace(ciString(doc, "status")),
			Cwd:      strings.TrimSpace(ciString(doc, "cwd")),
		})
	}
	return refs, nil
}

// ListWorkspaces returns active workspaces. Archived workspaces are absent
// from this listing; that is the authoritative workspace-absence check.
func (a *CLIAdapter) ListWorkspaces(ctx context.Context) ([]WorkspaceRef, error) {
	ctx, cancel := context.WithTimeout(ctx, InspectTimeout)
	defer cancel()
	out, err := a.run(ctx, "workspace", "ls", "--json")
	if err != nil {
		return nil, err
	}
	docs, err := decodeObjectList(out)
	if err != nil {
		return nil, &Error{Op: "workspace-ls", Kind: KindIdentity, Reason: "unparseable workspace listing"}
	}
	refs := make([]WorkspaceRef, 0, len(docs))
	for _, doc := range docs {
		refs = append(refs, WorkspaceRef{
			ID:        strings.TrimSpace(ciString(doc, "workspaceId", "id")),
			Title:     strings.TrimSpace(ciString(doc, "name", "title")),
			Cwd:       strings.TrimSpace(ciString(doc, "cwd")),
			Isolation: strings.TrimSpace(ciString(doc, "isolation")),
		})
	}
	return refs, nil
}

// WorkspaceListed reports whether workspaceID is still active/listed.
func (a *CLIAdapter) WorkspaceListed(ctx context.Context, workspaceID string) (bool, error) {
	if verr := domain.ValidatePaseoIdentity("workspaceId", workspaceID); verr != nil {
		return false, &Error{Op: "workspace-ls", Kind: KindStartup, Reason: "invalid workspace identity shape"}
	}
	workspaces, err := a.ListWorkspaces(ctx)
	if err != nil {
		return false, err
	}
	for _, ws := range workspaces {
		if ws.ID == workspaceID {
			return true, nil
		}
	}
	return false, nil
}

// decodeObjectList accepts an empty output, one object, or an array of
// objects. Shape-only failures never include values.
func decodeObjectList(raw []byte) ([]map[string]any, error) {
	raw = bytes.TrimSpace(raw)
	if len(raw) == 0 {
		return nil, nil
	}
	var docs []map[string]any
	if err := json.Unmarshal(raw, &docs); err == nil {
		return docs, nil
	}
	var single map[string]any
	if err := json.Unmarshal(raw, &single); err != nil {
		return nil, err
	}
	return []map[string]any{single}, nil
}

func sortedLabelKeys(labels map[string]string) []string {
	keys := make([]string, 0, len(labels))
	for key, value := range labels {
		if strings.TrimSpace(key) != "" && strings.TrimSpace(value) != "" {
			keys = append(keys, key)
		}
	}
	slices.Sort(keys)
	return keys
}

// AssertRoute checks observed session evidence against the frozen request.
// expectedCwd is empty when the caller has no path to compare. Matching is
// exact after trimming; any mismatch or missing identity is a hard
// KindIdentity failure — never a silent substitution. Returned values are
// operator-supplied routing (never credentials), so naming the mismatched
// field is safe.
func AssertRoute(provider, model, mode, expectedCwd string, observed Identity) error {
	provider = strings.TrimSpace(provider)
	model = strings.TrimSpace(model)
	mode = strings.TrimSpace(mode)
	if observed.Provider == "" || observed.Model == "" {
		return &Error{Op: "assert", Kind: KindIdentity, Reason: "session evidence missing provider or model"}
	}
	if observed.Provider != provider {
		return &Error{Op: "assert", Kind: KindIdentity, Reason: fmt.Sprintf("effective provider %q != requested %q", observed.Provider, provider)}
	}
	if observed.Model != model {
		return &Error{Op: "assert", Kind: KindIdentity, Reason: fmt.Sprintf("effective model %q != requested %q", observed.Model, model)}
	}
	if mode != "" && observed.Mode != mode {
		return &Error{Op: "assert", Kind: KindIdentity, Reason: fmt.Sprintf("effective mode %q != requested %q", observed.Mode, mode)}
	}
	if expectedCwd != "" && normalizePath(observed.Cwd) != normalizePath(expectedCwd) {
		return &Error{Op: "assert", Kind: KindIdentity, Reason: "session Cwd is not the assigned workspace"}
	}
	return nil
}

// normalizeProvider strips combined "provider/..." forms before comparing,
// so the effective route still matches the requested provider exactly.
func normalizeProvider(raw string) string {
	raw = strings.TrimSpace(raw)
	if i := strings.Index(raw, "/"); i >= 0 {
		raw = raw[:i]
	}
	return raw
}

// normalizePath cleans a session Cwd for comparison: trailing slashes,
// redundant separators and a leading ~/ display form are normalized.
func normalizePath(p string) string {
	p = strings.TrimSpace(p)
	if strings.HasPrefix(p, "~") {
		if home, err := os.UserHomeDir(); err == nil {
			p = home + p[1:]
		}
	}
	return filepath.Clean(p)
}

// formatWaitTimeout renders a Go duration the way `paseo run
// --wait-timeout` accepts it (smoke-proven `300s` form).
func formatWaitTimeout(d time.Duration) string {
	secs := int(d / time.Second)
	if secs < 1 {
		secs = 1
	}
	return fmt.Sprintf("%ds", secs)
}

// Output bounds for one CLI invocation. The daemon is trusted local
// tooling, but a misbehaving output must not grow Worker memory through
// the error path: writes past the bound fail the call instead.
const (
	maxCLIOutputBytes = 1 << 20
	maxStderrBytes    = 1 << 16
)

// WaitDelay backstop for captured pipes. Group kill normally closes
// every inheritor at once; if anything ever escapes the group
// (double-fork, setsid), Wait still stops waiting after this bound
// instead of stalling the Worker slot indefinitely.
const paseoPipeWaitDelay = 10 * time.Second

// boundedWriter caps captured subprocess output. Grow-style hints do not
// bound anything; this writer refuses writes past the limit so capture
// itself is the bound, checked before any content is retained.
type boundedWriter struct {
	buf      bytes.Buffer
	limit    int
	exceeded bool
}

func (w *boundedWriter) Write(p []byte) (int, error) {
	if w.exceeded {
		return 0, errors.New("output exceeded bound")
	}
	if w.buf.Len()+len(p) > w.limit {
		w.exceeded = true
		return 0, errors.New("output exceeded bound")
	}
	return w.buf.Write(p)
}

// run executes one paseo invocation under ctx. Cancellation kills the
// whole process group (see isolateChild): the CLI's descendants inherit
// the captured pipes, and killing only the direct child would leave
// orphans holding them, blocking Wait on EOF until they exit. WaitDelay
// bounds even that escape. Failures report only the operation, the exit
// status and a redacted shape — never command output bodies.
func (a *CLIAdapter) run(ctx context.Context, args ...string) ([]byte, error) {
	op := args[0]
	if op == "workspace" && len(args) > 1 {
		op = "workspace-" + args[1]
	}
	cmd := exec.CommandContext(ctx, a.bin, args...)
	isolateChild(cmd)
	cmd.Cancel = func() error {
		return killGroup(cmd.Process)
	}
	cmd.WaitDelay = paseoPipeWaitDelay
	// Controlled environment: inherit only what the local daemon and
	// provider CLIs need (PATH/HOME/TMPDIR). Tokens are never passed as
	// arguments or captured into output.
	cmd.Env = []string{
		"PATH=" + os.Getenv("PATH"),
		"HOME=" + os.Getenv("HOME"),
		"TMPDIR=" + os.Getenv("TMPDIR"),
	}
	stdout := &boundedWriter{limit: maxCLIOutputBytes}
	stderr := &boundedWriter{limit: maxStderrBytes}
	cmd.Stdout = stdout
	cmd.Stderr = stderr
	err := cmd.Run()
	if ctxErr := ctx.Err(); ctxErr != nil {
		kind := KindCancelled
		if errors.Is(ctxErr, context.DeadlineExceeded) {
			kind = KindTimeout
		}
		return nil, &Error{Op: op, Kind: kind, Reason: "operation " + string(kind)}
	}
	if stdout.exceeded || stderr.exceeded {
		return nil, &Error{Op: op, Kind: KindExternal, Reason: "output exceeded bound"}
	}
	if err != nil {
		var exitErr *exec.ExitError
		reason := "nonzero exit"
		if errors.As(err, &exitErr) {
			reason = fmt.Sprintf("exit %d", exitErr.ExitCode())
		}
		return nil, &Error{Op: op, Kind: KindStartup, Reason: telemetry.Redact(reason)}
	}
	return stdout.buf.Bytes(), nil
}

// firstObject decodes CLI JSON that may be an object or a single-element
// list, with case-insensitive keys. It returns the object and the sorted
// top-level key list (for shape-only failure reports, never values).
func firstObject(raw []byte) (map[string]any, string, error) {
	raw = bytes.TrimSpace(raw)
	if len(raw) == 0 {
		return nil, "(empty)", errors.New("empty output")
	}
	var obj map[string]any
	if err := json.Unmarshal(raw, &obj); err == nil {
		return obj, keyList(obj), nil
	}
	var list []map[string]any
	if err := json.Unmarshal(raw, &list); err != nil || len(list) == 0 {
		return nil, "(unparseable)", errors.New("not an object or list")
	}
	return list[0], keyList(list[0]), nil
}

// parseOpaqueID extracts one opaque identity from CLI JSON output,
// accepting the documented key spellings (workspaceId/id, id/agentId).
func parseOpaqueID(raw []byte, keys ...string) (string, string, error) {
	obj, keyNames, err := firstObject(raw)
	if err != nil {
		return "", keyNames, err
	}
	for _, k := range keys {
		if v := ciString(obj, k); v != "" {
			return strings.TrimSpace(v), keyNames, nil
		}
	}
	return "", keyNames, errors.New("identity key missing")
}

// ciString returns the first non-empty string value whose key matches any
// of names case-insensitively.
func ciString(obj map[string]any, names ...string) string {
	lowered := make(map[string]any, len(obj))
	for k, v := range obj {
		lowered[strings.ToLower(k)] = v
	}
	for _, n := range names {
		if v, ok := lowered[strings.ToLower(n)]; ok {
			if s, ok := v.(string); ok && strings.TrimSpace(s) != "" {
				return s
			}
		}
	}
	return ""
}

// ciBool reads a case-insensitive boolean field; missing or non-bool values
// report false rather than inventing archived state.
func ciBool(obj map[string]any, name string) bool {
	for k, v := range obj {
		if strings.EqualFold(k, name) {
			b, ok := v.(bool)
			return ok && b
		}
	}
	return false
}

// keyList renders the sorted top-level JSON keys for shape-only reports.
func keyList(obj map[string]any) string {
	keys := slices.Sorted(maps.Keys(obj))
	if len(keys) > 12 {
		keys = keys[:12]
	}
	return "(" + strings.Join(keys, ",") + ")"
}
