package paseo

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// writeStub installs an executable shell script acting as the paseo CLI.
func writeStub(t *testing.T, body string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "paseo-stub")
	if err := os.WriteFile(path, []byte(body), 0o700); err != nil {
		t.Fatal(err)
	}
	return path
}

const passInspect = `{"Id":"AGENT1","Provider":"codex","Model":"gpt-6-luna","Mode":"full-access","Cwd":"/tmp/proj","Status":"idle"}`

// passStub answers every operation the smoke-proven way.
func passStub() string {
	return `#!/bin/sh
case "$1" in
  --version) echo "paseo-stub 0.0.0" ;;
  workspace)
    case "$2" in
      create) echo '{"workspaceId":"ws_stub_1"}' ;;
      ls) echo '[{"workspaceId":"ws_stub_1"}]' ;;
      archive) exit 0 ;;
    esac ;;
  run) echo '{"Id":"AGENT1"}' ;;
  inspect) echo '` + passInspect + `' ;;
  logs) printf '[User] do it\n[Shell] $ ls /tmp/proj\n[Shell] $ npm test\n[Edit] /tmp/proj/add.js\n[Text] done\n' ;;
  send|wait|stop|archive) exit 0 ;;
  *) exit 3 ;;
esac`
}

func newStubAdapter(t *testing.T, body string) *CLIAdapter {
	t.Helper()
	return NewCLIAdapter(nil, WithBinary(writeStub(t, body)))
}

func TestProbeReportsVersion(t *testing.T) {
	a := newStubAdapter(t, passStub())
	v, err := a.Probe(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(v, "paseo-stub") {
		t.Fatalf("unexpected version %q", v)
	}
}

func TestCreateWorkspaceParsesOpaqueID(t *testing.T) {
	a := newStubAdapter(t, passStub())
	id, err := a.CreateWorkspace(context.Background(), t.TempDir(), "title")
	if err != nil {
		t.Fatal(err)
	}
	if id != "ws_stub_1" {
		t.Fatalf("unexpected workspace id %q", id)
	}
}

func TestCreateWorkspaceListForm(t *testing.T) {
	a := newStubAdapter(t, "#!/bin/sh\necho '[{\"id\":\"ws_list_9\"}]'\n")
	id, err := a.CreateWorkspace(context.Background(), t.TempDir(), "")
	if err != nil {
		t.Fatal(err)
	}
	if id != "ws_list_9" {
		t.Fatalf("unexpected workspace id %q", id)
	}
}

func TestStartSessionAssertsRouteFromEvidence(t *testing.T) {
	a := newStubAdapter(t, passStub())
	got, err := a.StartSession(context.Background(), StartRequest{
		Provider:    "codex",
		Model:       "gpt-6-luna",
		Mode:        "full-access",
		WorkspaceID: "ws_stub_1",
		Title:       "s3b-test",
		Prompt:      "do the thing",
		WaitTimeout: 5 * time.Second,
	})
	if err != nil {
		t.Fatal(err)
	}
	if got.SessionID != "AGENT1" || got.Provider != "codex" || got.Model != "gpt-6-luna" || got.Mode != "full-access" {
		t.Fatalf("unexpected identity %+v", got)
	}
	if got.WorkspaceID != "ws_stub_1" {
		t.Fatalf("workspace not carried: %+v", got)
	}
}

func TestStartSessionRejectsUnauthorizedRoute(t *testing.T) {
	a := newStubAdapter(t, passStub())
	_, err := a.StartSession(context.Background(), StartRequest{
		Provider:    "codex",
		Model:       "gpt-6-luna",
		Mode:        "build",
		WorkspaceID: "ws_stub_1",
		Prompt:      "x",
		WaitTimeout: time.Second,
	})
	perr, ok := err.(*Error)
	if !ok || perr.Kind != KindIdentity {
		t.Fatalf("want KindIdentity, got %v", err)
	}
}

func TestStartSessionMismatchFailsExplicitly(t *testing.T) {
	stub := `#!/bin/sh
case "$1" in
  run) echo '{"Id":"AGENT9"}' ;;
  inspect) echo '{"Provider":"opencode","Model":"opencode-go/deepseek-v4.1-flash","Mode":"build","Cwd":"/tmp/proj","Status":"idle"}' ;;
esac`
	a := newStubAdapter(t, stub)
	_, err := a.StartSession(context.Background(), StartRequest{
		Provider:    "codex",
		Model:       "gpt-6-luna",
		Mode:        "full-access",
		WorkspaceID: "ws_stub_1",
		Prompt:      "x",
		WaitTimeout: time.Second,
	})
	perr, ok := err.(*Error)
	if !ok || perr.Kind != KindIdentity {
		t.Fatalf("want KindIdentity mismatch, got %v", err)
	}
	if !strings.Contains(perr.Reason, "effective provider") {
		t.Fatalf("mismatch must name the field, got %q", perr.Reason)
	}
}

func TestInspectMissingProviderFails(t *testing.T) {
	a := newStubAdapter(t, "#!/bin/sh\necho '{\"Id\":\"A\",\"Status\":\"idle\"}'\n")
	_, err := a.InspectSession(context.Background(), "aaaaaaaa-1111-4111-8111-aaaaaaaaaaa1")
	perr, ok := err.(*Error)
	if !ok || perr.Kind != KindIdentity {
		t.Fatalf("want KindIdentity, got %v", err)
	}
}

func TestObserveTimelineCountsOnly(t *testing.T) {
	a := newStubAdapter(t, passStub())
	sum, err := a.ObserveTimeline(context.Background(), "aaaaaaaa-1111-4111-8111-aaaaaaaaaaa1")
	if err != nil {
		t.Fatal(err)
	}
	if sum.Lines != 5 {
		t.Fatalf("want 5 lines, got %+v", sum)
	}
	if sum.Steps["Shell"] != 2 || sum.Steps["Edit"] != 1 {
		t.Fatalf("bad step counts %+v", sum.Steps)
	}
	if sum.ToolSteps != 3 {
		t.Fatalf("want 3 tool steps, got %+v", sum)
	}
}

func TestFailuresNeverCarrySecrets(t *testing.T) {
	stub := "#!/bin/sh\necho 'boom sk-SECRET123 bearer abc' >&2\nexit 1\n"
	a := newStubAdapter(t, stub)
	_, err := a.CreateWorkspace(context.Background(), t.TempDir(), "")
	if err == nil {
		t.Fatal("want failure")
	}
	if strings.Contains(err.Error(), "SECRET123") {
		t.Fatalf("error leaked output body: %v", err)
	}
}

func TestCancellationIsExplicit(t *testing.T) {
	stub := "#!/bin/sh\nsleep 30\n"
	a := newStubAdapter(t, stub)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	_, err := a.CreateWorkspace(ctx, t.TempDir(), "")
	perr, ok := err.(*Error)
	if !ok || (perr.Kind != KindCancelled && perr.Kind != KindTimeout) {
		t.Fatalf("want cancelled/timeout, got %v", err)
	}
}

func TestWorkspaceListedRoundTrip(t *testing.T) {
	a := newStubAdapter(t, passStub())
	ok, err := a.WorkspaceListed(context.Background(), "ws_stub_1")
	if err != nil || !ok {
		t.Fatalf("want listed, got %v %v", ok, err)
	}
	ok, err = a.WorkspaceListed(context.Background(), "ws_gone")
	if err != nil || ok {
		t.Fatalf("want absent, got %v %v", ok, err)
	}
}

func TestAssertRouteTable(t *testing.T) {
	observed := Identity{Provider: "codex", Model: "gpt-6-luna", Mode: "full-access", Cwd: "/tmp/proj/"}
	cases := []struct {
		name        string
		provider    string
		model       string
		mode        string
		cwd         string
		observed    Identity
		wantKind    FailureKind
		wantSnippet string
	}{
		{"match", "codex", "gpt-6-luna", "full-access", "/tmp/proj", observed, "", ""},
		{"provider-substitution", "opencode", "gpt-6-luna", "full-access", "", observed, KindIdentity, "effective provider"},
		{"model-substitution", "codex", "gpt-6-astra", "full-access", "", observed, KindIdentity, "effective model"},
		{"mode-substitution", "codex", "gpt-6-luna", "build", "", observed, KindIdentity, "effective mode"},
		{"cwd-mismatch", "codex", "gpt-6-luna", "full-access", "/tmp/other", observed, KindIdentity, "Cwd"},
		{"missing-evidence", "codex", "gpt-6-luna", "full-access", "", Identity{}, KindIdentity, "missing"},
		{"combined-provider-form", "codex", "gpt-6-luna", "", "", Identity{Provider: normalizeProvider("codex/gpt-6-luna"), Model: "gpt-6-luna"}, "", ""},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			err := AssertRoute(tc.provider, tc.model, tc.mode, tc.cwd, tc.observed)
			if tc.wantKind == "" {
				if err != nil {
					t.Fatalf("want match, got %v", err)
				}
				return
			}
			perr, ok := err.(*Error)
			if !ok || perr.Kind != tc.wantKind {
				t.Fatalf("want %s, got %v", tc.wantKind, err)
			}
			if !strings.Contains(perr.Reason, tc.wantSnippet) {
				t.Fatalf("reason %q must mention %q", perr.Reason, tc.wantSnippet)
			}
		})
	}
}

func TestFakeAdapterScripting(t *testing.T) {
	f := NewFakeAdapter()
	if _, err := f.CreateWorkspace(context.Background(), "/x", "t"); err != nil {
		t.Fatal(err)
	}
	if !f.Called("create") || f.Called("run") {
		t.Fatalf("bad call record %v", f.Calls)
	}
	f.Fail["run"] = FailOp("run", "daemon down")
	if _, err := f.StartSession(context.Background(), StartRequest{}); err == nil {
		t.Fatal("want scripted failure")
	}
	m := ScriptMismatch()
	if m.Session.Provider != "opencode" {
		t.Fatal("mismatch fake must disagree")
	}
}
