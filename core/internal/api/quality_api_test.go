package api

import (
	"context"
	"encoding/json"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/store"
	"github.com/JamesMorales04/software-factory/core/internal/telemetry"
)

const (
	testVerifierToken = "test-verifier-token"
	testReviewerToken = "test-reviewer-token"
)

func newQualityTestServer(t *testing.T) (*Server, *store.DB) {
	t.Helper()
	db, err := store.Open(filepath.Join(t.TempDir(), "quality-api.db"))
	if err != nil {
		t.Fatalf("Open: %v", err)
	}
	t.Cleanup(func() { _ = db.Close() })
	logger := telemetry.NewLogger()
	tracer, _ := telemetry.InitTracing(context.Background(), "test")
	srv := New(db, testToken, logger, tracer, NewHub()).
		WithWorkerToken(testWorkerToken).
		WithVerifierToken(testVerifierToken).
		WithReviewerToken(testReviewerToken)
	return srv, db
}

func qualityRepo(t *testing.T) (string, string) {
	t.Helper()
	dir := t.TempDir()
	run := func(args ...string) string {
		t.Helper()
		cmd := exec.Command("git", append([]string{"-C", dir}, args...)...)
		cmd.Env = append(os.Environ(),
			"GIT_AUTHOR_NAME=test", "GIT_AUTHOR_EMAIL=test@example.test",
			"GIT_COMMITTER_NAME=test", "GIT_COMMITTER_EMAIL=test@example.test")
		out, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("git %s: %v %s", strings.Join(args, " "), err, out)
		}
		return strings.TrimSpace(string(out))
	}
	run("init", "-q")
	if err := os.WriteFile(filepath.Join(dir, "add.js"), []byte("module.exports={add:(a,b)=>a+b};\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	run("add", "add.js")
	run("commit", "-qm", "base")
	return dir, run("rev-parse", "HEAD")
}

func TestCreateSpecAndSpecBoundRunOverHTTP(t *testing.T) {
	srv, _ := newQualityTestServer(t)
	h := srv.Handler()
	repo, base := qualityRepo(t)

	rec := serveRun(t, h, "POST", "/api/v1/actions", `{"title":"S4-A"}`, testToken)
	if rec.Code != http.StatusCreated {
		t.Fatalf("create action: %d %s", rec.Code, rec.Body.String())
	}
	var created struct {
		Action domain.Action `json:"action"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &created); err != nil {
		t.Fatal(err)
	}
	specBody := `{
		"actionId":"` + created.Action.ID + `",
		"goal":"Append multiply to add.js",
		"repository":"` + repo + `",
		"baseRevision":"` + base + `",
		"allowedPaths":["add.js"],
		"acceptanceCriteria":[
			{"id":"format","kind":"format","description":"format","command":["node","--check","add.js"],"expectExit":0,"timeoutSecs":30},
			{"id":"test","kind":"test","description":"tests","command":["node","-e","0"],"expectExit":0,"timeoutSecs":30},
			{"id":"functional","kind":"functional","description":"multiply","command":["node","-e","0"],"expectExit":0,"timeoutSecs":30}
		],
		"negativeCases":[
			{"id":"negative","kind":"negative","description":"invalid","command":["node","-e","1"],"expectExit":1,"timeoutSecs":30}
		],
		"riskClass":"R0","qualityProfile":"node-basic",
		"budget":{"maxAttempts":1,"timeoutSecs":120},
		"executionMode":"trusted-local","provider":"opencode",
		"model":"opencode-go/deepseek-v4.1-flash","paseoMode":"build"
	}`
	rec = serveRun(t, h, "POST", "/api/v1/action-specs", specBody, testToken)
	if rec.Code != http.StatusCreated {
		t.Fatalf("create spec: %d %s", rec.Code, rec.Body.String())
	}
	var specOut struct {
		Spec domain.ActionSpec `json:"spec"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &specOut); err != nil {
		t.Fatal(err)
	}
	if specOut.Spec.ContentHash == "" || specOut.Spec.PolicyRevision != domain.QualityPolicyRevision {
		t.Fatalf("spec not frozen: %+v", specOut.Spec)
	}
	// Repeated identical content replays without a new specification.
	rec = serveRun(t, h, "POST", "/api/v1/action-specs", specBody, testToken)
	if rec.Code != http.StatusOK || rec.Header().Get("Idempotent-Replayed") != "true" {
		t.Fatalf("spec replay: %d %v", rec.Code, rec.Header())
	}

	runBody := `{"specId":"` + specOut.Spec.ID + `"}`
	rec = serveRun(t, h, "POST", "/api/v1/runs", runBody, testToken)
	if rec.Code != http.StatusCreated {
		t.Fatalf("spec-bound run: %d %s", rec.Code, rec.Body.String())
	}
	run, _ := decodeRunBody(t, rec)
	if run.SpecID != specOut.Spec.ID || run.ActionID != created.Action.ID ||
		run.Provider != specOut.Spec.Provider || run.WorkloadKind != domain.PaseoTaskKind {
		t.Fatalf("run not bound to spec: %+v", run)
	}
	if run.WorkloadInput["task"] != specOut.Spec.Goal {
		t.Fatalf("task not derived from spec: %+v", run.WorkloadInput)
	}

	conflict := `{"specId":"` + specOut.Spec.ID + `","provider":"codex"}`
	rec = serveRun(t, h, "POST", "/api/v1/runs", conflict, testToken)
	if rec.Code != http.StatusBadRequest {
		t.Fatalf("conflicting route: %d %s", rec.Code, rec.Body.String())
	}
}

func TestQualityRoutesEnforceScopedAuthorities(t *testing.T) {
	srv, _ := newQualityTestServer(t)
	h := srv.Handler()
	body := `{"candidateId":"x"}`
	cases := []struct {
		method, path, token string
		want                int
	}{
		{"POST", "/api/v1/verifier/evidence", testWorkerToken, http.StatusUnauthorized},
		{"POST", "/api/v1/verifier/evidence", testToken, http.StatusUnauthorized},
		{"POST", "/api/v1/verifier/evidence", testReviewerToken, http.StatusUnauthorized},
		{"POST", "/api/v1/reviewer/verdict", testWorkerToken, http.StatusUnauthorized},
		{"POST", "/api/v1/reviewer/verdict", testVerifierToken, http.StatusUnauthorized},
		{"GET", "/api/v1/verifier/candidates/nope", testVerifierToken, http.StatusNotFound},
		{"GET", "/api/v1/reviewer/candidates/nope", testReviewerToken, http.StatusNotFound},
	}
	for _, tc := range cases {
		rec := serveRun(t, h, tc.method, tc.path, body, tc.token)
		if rec.Code != tc.want {
			t.Fatalf("%s %s token=%s: got %d (%s), want %d",
				tc.method, tc.path, tc.token, rec.Code, rec.Body.String(), tc.want)
		}
	}
	rec := serveRun(t, h, "GET", "/api/v1/verifier/candidates/x", "", "")
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("missing verifier credential: %d", rec.Code)
	}
}
