package api

import (
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"testing"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

func queuePaseoRun(t *testing.T, h http.Handler, body string) domain.Run {
	t.Helper()
	if body == "" {
		body = `{"workloadKind":"paseo-task","workloadInput":{"task":"add multiply"},"executionMode":"trusted-local","provider":"codex","model":"gpt-6-luna","paseoMode":"full-access"}`
	}
	rec := serveRun(t, h, "POST", "/api/v1/runs", body, testToken)
	if rec.Code != http.StatusCreated {
		t.Fatalf("queue paseo run: got %d (%s)", rec.Code, rec.Body.String())
	}
	run, _ := decodeRunBody(t, rec)
	if run.Status != domain.RunQueued {
		t.Fatalf("run status = %s, want queued", run.Status)
	}
	if run.ExecutionMode != string(domain.ExecTrustedLocal) || run.Provider != "codex" ||
		run.Model != "gpt-6-luna" || run.PaseoMode != "full-access" {
		t.Fatalf("routing not persisted: %+v", run)
	}
	return run
}

func startPaseoAttempt(t *testing.T, h http.Handler, run domain.Run, worker string) domain.Attempt {
	t.Helper()
	rec := serveRun(t, h, "POST", "/api/v1/workers/register",
		`{"workerId":"`+worker+`","version":"test-s3a"}`, testWorkerToken)
	if rec.Code != http.StatusCreated && rec.Code != http.StatusOK {
		t.Fatalf("register: got %d (%s)", rec.Code, rec.Body.String())
	}
	rec = serveRun(t, h, "POST", "/api/v1/workers/poll", `{"workerId":"`+worker+`"}`, testWorkerToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("poll: got %d (%s)", rec.Code, rec.Body.String())
	}
	_, attempt := decodeRunBody(t, rec)
	hb := `{"workerId":"` + worker + `","runId":"` + run.ID + `","attemptId":"` + attempt.ID + `","generation":1}`
	rec = serveRun(t, h, "POST", "/api/v1/workers/heartbeat", hb, testWorkerToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("heartbeat: got %d (%s)", rec.Code, rec.Body.String())
	}
	return attempt
}

func decodeSession(t *testing.T, body string) domain.PaseoBinding {
	t.Helper()
	var out struct {
		Session domain.PaseoBinding `json:"session"`
	}
	if err := json.Unmarshal([]byte(body), &out); err != nil {
		t.Fatalf("decode session body: %v (%s)", err, body)
	}
	return out.Session
}

func TestPaseoSessionLifecycleOverHTTP(t *testing.T) {
	srv, _ := newRunTestServer(t)
	h := srv.Handler()

	run := queuePaseoRun(t, h, "")
	attempt := startPaseoAttempt(t, h, run, "s3a-1")

	intend := `{"workerId":"s3a-1","runId":"` + run.ID + `","attemptId":"` + attempt.ID + `","generation":1}`
	rec := serveRun(t, h, "POST", "/api/v1/workers/session/intend", intend, testWorkerToken)
	if rec.Code != http.StatusCreated {
		t.Fatalf("intend: got %d (%s)", rec.Code, rec.Body.String())
	}
	session := decodeSession(t, rec.Body.String())
	if session.Status != domain.PaseoIntended || session.SessionID != "" {
		t.Fatalf("bad intended session: %+v", session)
	}

	// Duplicate intend replays.
	rec = serveRun(t, h, "POST", "/api/v1/workers/session/intend", intend, testWorkerToken)
	if rec.Code != http.StatusOK || rec.Header().Get("Idempotent-Replayed") != "true" {
		t.Fatalf("intend replay: got %d replay=%q (%s)", rec.Code, rec.Header().Get("Idempotent-Replayed"), rec.Body.String())
	}

	bind := `{"workerId":"s3a-1","runId":"` + run.ID + `","attemptId":"` + attempt.ID +
		`","generation":1,"workspaceId":"ws_http_1","sessionId":"11111111-1111-1111-1111-111111111111"}`
	rec = serveRun(t, h, "POST", "/api/v1/workers/session/bind", bind, testWorkerToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("bind: got %d (%s)", rec.Code, rec.Body.String())
	}
	session = decodeSession(t, rec.Body.String())
	if session.Status != domain.PaseoActive || session.SessionID != "11111111-1111-1111-1111-111111111111" {
		t.Fatalf("bad bound session: %+v", session)
	}

	// Operator reads the binding; the sessions list is server-authoritative.
	rec = serveRun(t, h, "GET", "/api/v1/runs/"+run.ID+"/sessions", "", testToken)
	if rec.Code != http.StatusOK || !strings.Contains(rec.Body.String(), "11111111-1111-1111-1111-111111111111") {
		t.Fatalf("list sessions: got %d (%s)", rec.Code, rec.Body.String())
	}

	// The run events carry the session lifecycle on the run aggregate.
	rec = serveRun(t, h, "GET", "/api/v1/runs/"+run.ID+"/events", "", testToken)
	for _, want := range []string{"PaseoSessionIntended", "PaseoSessionActive"} {
		if !strings.Contains(rec.Body.String(), want) {
			t.Fatalf("run events miss %s (%s)", want, rec.Body.String())
		}
	}

	closeOK := `{"workerId":"s3a-1","runId":"` + run.ID + `","attemptId":"` + attempt.ID +
		`","generation":1,"outcome":"succeeded"}`
	rec = serveRun(t, h, "POST", "/api/v1/workers/session/close", closeOK, testWorkerToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("close: got %d (%s)", rec.Code, rec.Body.String())
	}
	if got := decodeSession(t, rec.Body.String()); got.Status != domain.PaseoSucceeded {
		t.Fatalf("bad closed session: %+v", got)
	}

	// Duplicate close replays without a new event.
	rec = serveRun(t, h, "POST", "/api/v1/workers/session/close", closeOK, testWorkerToken)
	if rec.Code != http.StatusOK || rec.Header().Get("Idempotent-Replayed") != "true" {
		t.Fatalf("close replay: got %d replay=%q (%s)", rec.Code, rec.Header().Get("Idempotent-Replayed"), rec.Body.String())
	}
}

func TestPaseoReportGatingOverHTTP(t *testing.T) {
	srv, _ := newRunTestServer(t)
	h := srv.Handler()

	run := queuePaseoRun(t, h, "")
	attempt := startPaseoAttempt(t, h, run, "s3a-rep")

	// Success with no session is rejected, not recorded.
	report := `{"workerId":"s3a-rep","runId":"` + run.ID + `","attemptId":"` + attempt.ID +
		`","generation":1,"outcome":"completed","output":{"receipt":"forged"}}`
	rec := serveRun(t, h, "POST", "/api/v1/workers/report", report, testWorkerToken)
	if rec.Code != http.StatusBadRequest || !strings.Contains(rec.Body.String(), "successfully closed session binding") {
		t.Fatalf("success without session: got %d (%s)", rec.Code, rec.Body.String())
	}

	// The honest path — intend → bind → close(succeeded) → report — works.
	intend := `{"workerId":"s3a-rep","runId":"` + run.ID + `","attemptId":"` + attempt.ID + `","generation":1}`
	if rec := serveRun(t, h, "POST", "/api/v1/workers/session/intend", intend, testWorkerToken); rec.Code != http.StatusCreated {
		t.Fatalf("intend: got %d (%s)", rec.Code, rec.Body.String())
	}
	bind := `{"workerId":"s3a-rep","runId":"` + run.ID + `","attemptId":"` + attempt.ID +
		`","generation":1,"workspaceId":"ws_rep","sessionId":"22222222-2222-4222-8222-222222222222"}`
	if rec := serveRun(t, h, "POST", "/api/v1/workers/session/bind", bind, testWorkerToken); rec.Code != http.StatusOK {
		t.Fatalf("bind: got %d (%s)", rec.Code, rec.Body.String())
	}
	closeOK := `{"workerId":"s3a-rep","runId":"` + run.ID + `","attemptId":"` + attempt.ID +
		`","generation":1,"outcome":"succeeded"}`
	if rec := serveRun(t, h, "POST", "/api/v1/workers/session/close", closeOK, testWorkerToken); rec.Code != http.StatusOK {
		t.Fatalf("close: got %d (%s)", rec.Code, rec.Body.String())
	}
	rec = serveRun(t, h, "POST", "/api/v1/workers/report", report, testWorkerToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("honest success: got %d (%s)", rec.Code, rec.Body.String())
	}
	var out struct {
		Run domain.Run `json:"run"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &out); err != nil || out.Run.Status != domain.RunSucceeded {
		t.Fatalf("run not succeeded: %+v (%s)", out, rec.Body.String())
	}
}

func TestPaseoFailedReportOrphansSessionOverHTTP(t *testing.T) {
	srv, _ := newRunTestServer(t)
	h := srv.Handler()

	run := queuePaseoRun(t, h, "")
	attempt := startPaseoAttempt(t, h, run, "s3a-orf")
	intend := `{"workerId":"s3a-orf","runId":"` + run.ID + `","attemptId":"` + attempt.ID + `","generation":1}`
	serveRun(t, h, "POST", "/api/v1/workers/session/intend", intend, testWorkerToken)
	bind := `{"workerId":"s3a-orf","runId":"` + run.ID + `","attemptId":"` + attempt.ID +
		`","generation":1,"workspaceId":"ws_orf","sessionId":"33333333-3333-4333-8333-333333333333"}`
	serveRun(t, h, "POST", "/api/v1/workers/session/bind", bind, testWorkerToken)

	report := `{"workerId":"s3a-orf","runId":"` + run.ID + `","attemptId":"` + attempt.ID +
		`","generation":1,"outcome":"failed","error":"agent errored"}`
	rec := serveRun(t, h, "POST", "/api/v1/workers/report", report, testWorkerToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("failed report: got %d (%s)", rec.Code, rec.Body.String())
	}
	rec = serveRun(t, h, "GET", "/api/v1/runs/"+run.ID+"/sessions", "", testToken)
	if rec.Code != http.StatusOK || !strings.Contains(rec.Body.String(), `"status":"orphaned"`) {
		t.Fatalf("failed attempt must orphan its session: got %d (%s)", rec.Code, rec.Body.String())
	}
}

func TestPaseoDuplicateSessionRejectedOverHTTP(t *testing.T) {
	srv, _ := newRunTestServer(t)
	h := srv.Handler()

	runA := queuePaseoRun(t, h, "")
	attemptA := startPaseoAttempt(t, h, runA, "s3a-dup")
	serveRun(t, h, "POST", "/api/v1/workers/session/intend",
		`{"workerId":"s3a-dup","runId":"`+runA.ID+`","attemptId":"`+attemptA.ID+`","generation":1}`, testWorkerToken)
	bindA := `{"workerId":"s3a-dup","runId":"` + runA.ID + `","attemptId":"` + attemptA.ID +
		`","generation":1,"workspaceId":"ws_dup","sessionId":"44444444-4444-4444-8444-444444444444"}`
	if rec := serveRun(t, h, "POST", "/api/v1/workers/session/bind", bindA, testWorkerToken); rec.Code != http.StatusOK {
		t.Fatalf("bind A: got %d (%s)", rec.Code, rec.Body.String())
	}

	runB := queuePaseoRun(t, h, "")
	attemptB := startPaseoAttempt(t, h, runB, "s3a-dup")
	serveRun(t, h, "POST", "/api/v1/workers/session/intend",
		`{"workerId":"s3a-dup","runId":"`+runB.ID+`","attemptId":"`+attemptB.ID+`","generation":`+fmt.Sprint(attemptB.LeaseGeneration)+`}`, testWorkerToken)
	bindB := `{"workerId":"s3a-dup","runId":"` + runB.ID + `","attemptId":"` + attemptB.ID +
		`","generation":` + fmt.Sprint(attemptB.LeaseGeneration) + `,"workspaceId":"ws_dup","sessionId":"44444444-4444-4444-8444-444444444444"}`
	rec := serveRun(t, h, "POST", "/api/v1/workers/session/bind", bindB, testWorkerToken)
	if rec.Code != http.StatusConflict || !strings.Contains(rec.Body.String(), "stale_generation") {
		t.Fatalf("duplicate session bind: got %d (%s), want 409 stale_generation", rec.Code, rec.Body.String())
	}
}

func TestPaseoRunValidationOverHTTP(t *testing.T) {
	srv, _ := newRunTestServer(t)
	h := srv.Handler()
	bodies := []string{
		// paseo-task without trusted-local.
		`{"workloadKind":"paseo-task","workloadInput":{"task":"x"},"provider":"codex","model":"gpt-6-luna","paseoMode":"full-access"}`,
		// paseo-task with restricted mode.
		`{"workloadKind":"paseo-task","workloadInput":{"task":"x"},"executionMode":"restricted","provider":"codex","model":"gpt-6-luna","paseoMode":"full-access"}`,
		// substituted mode for the same model.
		`{"workloadKind":"paseo-task","workloadInput":{"task":"x"},"executionMode":"trusted-local","provider":"codex","model":"gpt-6-luna","paseoMode":"build"}`,
		// unlisted copilot-backed route.
		`{"workloadKind":"paseo-task","workloadInput":{"task":"x"},"executionMode":"trusted-local","provider":"opencode","model":"github-copilot/gpt-5-mini","paseoMode":"build"}`,
		// missing routing.
		`{"workloadKind":"paseo-task","workloadInput":{"task":"x"},"executionMode":"trusted-local"}`,
		// missing task.
		`{"workloadKind":"paseo-task","workloadInput":{},"executionMode":"trusted-local","provider":"codex","model":"gpt-6-luna","paseoMode":"full-access"}`,
		// trusted-local with synthetic echo.
		`{"workloadKind":"synthetic-echo","workloadInput":{"message":"x"},"executionMode":"trusted-local"}`,
		// synthetic echo carrying routing.
		`{"workloadKind":"synthetic-echo","workloadInput":{"message":"x"},"provider":"codex"}`,
		// isolated mode does not exist in S3-A.
		`{"workloadKind":"paseo-task","workloadInput":{"task":"x"},"executionMode":"isolated","provider":"codex","model":"gpt-6-luna","paseoMode":"full-access"}`,
	}
	for _, b := range bodies {
		rec := serveRun(t, h, "POST", "/api/v1/runs", b, testToken)
		if rec.Code != http.StatusBadRequest {
			t.Fatalf("body %q: got %d, want 400 (%s)", b, rec.Code, rec.Body.String())
		}
	}
	// Both smoke-proven OpenCode Go routes queue successfully.
	for _, model := range []string{"opencode-go/muse-spark-1.3-contributor", "opencode-go/deepseek-v4.1-flash"} {
		body := `{"workloadKind":"paseo-task","workloadInput":{"task":"add multiply"},"executionMode":"trusted-local","provider":"opencode","model":"` + model + `","paseoMode":"build"}`
		rec := serveRun(t, h, "POST", "/api/v1/runs", body, testToken)
		if rec.Code != http.StatusCreated {
			t.Fatalf("model %s: got %d, want 201 (%s)", model, rec.Code, rec.Body.String())
		}
	}
}

func TestPaseoSessionFencingOverHTTP(t *testing.T) {
	srv, db := newRunTestServer(t)
	h := srv.Handler()
	db.SetLeaseTTL(0) // every lease is immediately expired

	run := queuePaseoRun(t, h, "")
	rec := serveRun(t, h, "POST", "/api/v1/workers/register", `{"workerId":"s3a-fence"}`, testWorkerToken)
	if rec.Code != http.StatusCreated {
		t.Fatalf("register: got %d", rec.Code)
	}
	rec = serveRun(t, h, "POST", "/api/v1/workers/poll", `{"workerId":"s3a-fence"}`, testWorkerToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("poll: got %d", rec.Code)
	}
	_, attempt := decodeRunBody(t, rec)

	intend := `{"workerId":"s3a-fence","runId":"` + run.ID + `","attemptId":"` + attempt.ID + `","generation":1}`
	rec = serveRun(t, h, "POST", "/api/v1/workers/session/intend", intend, testWorkerToken)
	if rec.Code != http.StatusConflict || !strings.Contains(rec.Body.String(), "lease_expired") {
		t.Fatalf("expired intend: got %d (%s)", rec.Code, rec.Body.String())
	}

	rec = serveRun(t, h, "POST", "/api/v1/workers/session/bind",
		`{"workerId":"s3a-fence","runId":"`+run.ID+`","attemptId":"`+attempt.ID+`","generation":99,"workspaceId":"ws_1","sessionId":"session-1"}`,
		testWorkerToken)
	if rec.Code != http.StatusConflict || !strings.Contains(rec.Body.String(), "stale_generation") {
		t.Fatalf("stale bind: got %d (%s)", rec.Code, rec.Body.String())
	}

	// Cancel then race: session messages lose to cancellation.
	db.SetLeaseTTL(0) // keep deterministic; cancel works regardless
	cancelled := serveRun(t, h, "POST", "/api/v1/runs/"+run.ID+"/cancel", `{}`, testToken)
	if cancelled.Code != http.StatusOK {
		t.Fatalf("cancel: got %d (%s)", cancelled.Code, cancelled.Body.String())
	}
	rec = serveRun(t, h, "POST", "/api/v1/workers/session/intend", intend, testWorkerToken)
	if rec.Code != http.StatusConflict || !strings.Contains(rec.Body.String(), "run_cancelled") {
		t.Fatalf("intend after cancel: got %d (%s)", rec.Code, rec.Body.String())
	}
	rec = serveRun(t, h, "GET", "/api/v1/runs/"+run.ID+"/sessions", "", testToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("sessions after cancel: got %d", rec.Code)
	}
}

func TestPaseoSessionAuthIsScoped(t *testing.T) {
	srv, _ := newRunTestServer(t)
	h := srv.Handler()

	// Operator token rejected on every Worker session route.
	for _, route := range []string{
		"/api/v1/workers/session/intend", "/api/v1/workers/session/bind",
		"/api/v1/workers/session/close", "/api/v1/workers/session/orphan",
	} {
		rec := serveRun(t, h, "POST", route, `{"workerId":"s3a-1"}`, testToken)
		if rec.Code != http.StatusUnauthorized {
			t.Fatalf("%s with operator token: got %d, want 401", route, rec.Code)
		}
	}
	// Worker token rejected on the operator sessions read.
	rec := serveRun(t, h, "GET", "/api/v1/runs/abc/sessions", "", testWorkerToken)
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("sessions read with worker token: got %d, want 401", rec.Code)
	}
	// Unknown run's sessions read is 404, not 500.
	rec = serveRun(t, h, "GET", "/api/v1/runs/00000000000000000000000000000000/sessions", "", testToken)
	if rec.Code != http.StatusNotFound {
		t.Fatalf("unknown run sessions: got %d, want 404", rec.Code)
	}
}
