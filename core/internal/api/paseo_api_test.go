package api

import (
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
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

func TestPaseoOrphanQueueExposedToOperators(t *testing.T) {
	srv, _ := newRunTestServer(t)
	h := srv.Handler()

	// Empty queue reads as an empty list, never null.
	rec := serveRun(t, h, "GET", "/api/v1/sessions/orphaned", "", testToken)
	if rec.Code != http.StatusOK || !strings.Contains(rec.Body.String(), `"sessions":[]`) {
		t.Fatalf("empty orphan queue: got %d (%s)", rec.Code, rec.Body.String())
	}
	if rec := serveRun(t, h, "GET", "/api/v1/sessions/orphaned?limit=0", "", testToken); rec.Code != http.StatusBadRequest {
		t.Fatalf("bad limit: got %d, want 400", rec.Code)
	}
	if rec := serveRun(t, h, "GET", "/api/v1/sessions/orphaned?limit=201", "", testToken); rec.Code != http.StatusBadRequest {
		t.Fatalf("over-range limit: got %d, want 400", rec.Code)
	}
	if rec := serveRun(t, h, "GET", "/api/v1/sessions/orphaned?limit=abc", "", testToken); rec.Code != http.StatusBadRequest {
		t.Fatalf("non-integer limit: got %d, want 400", rec.Code)
	}

	run := queuePaseoRun(t, h, "")
	attempt := startPaseoAttempt(t, h, run, "s3a-queue")
	serveRun(t, h, "POST", "/api/v1/workers/session/intend",
		`{"workerId":"s3a-queue","runId":"`+run.ID+`","attemptId":"`+attempt.ID+`","generation":1}`, testWorkerToken)
	serveRun(t, h, "POST", "/api/v1/workers/session/bind",
		`{"workerId":"s3a-queue","runId":"`+run.ID+`","attemptId":"`+attempt.ID+`","generation":1,"workspaceId":"ws_q","sessionId":"55555555-5555-4555-8555-555555555555"}`,
		testWorkerToken)
	if rec := serveRun(t, h, "POST", "/api/v1/runs/"+run.ID+"/cancel", `{}`, testToken); rec.Code != http.StatusOK {
		t.Fatalf("cancel: got %d", rec.Code)
	}
	rec = serveRun(t, h, "GET", "/api/v1/sessions/orphaned", "", testToken)
	if rec.Code != http.StatusOK || !strings.Contains(rec.Body.String(), "55555555-5555-4555-8555-555555555555") {
		t.Fatalf("orphaned session missing from operator queue: got %d (%s)", rec.Code, rec.Body.String())
	}
	// Worker token never grants operator observations.
	if rec := serveRun(t, h, "GET", "/api/v1/sessions/orphaned", "", testWorkerToken); rec.Code != http.StatusUnauthorized {
		t.Fatalf("orphan queue with worker token: got %d, want 401", rec.Code)
	}
}

func decodeSessionList(t *testing.T, rec *httptest.ResponseRecorder) ([]domain.PaseoSessionSummary, int64) {
	t.Helper()
	var out struct {
		Sessions    []domain.PaseoSessionSummary `json:"sessions"`
		EventCursor int64                        `json:"eventCursor"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &out); err != nil {
		t.Fatalf("decode session list: %v (%s)", err, rec.Body.String())
	}
	return out.Sessions, out.EventCursor
}

func bindSessionForTest(t *testing.T, h http.Handler, run domain.Run, attempt domain.Attempt, worker, workspaceID, sessionID string) {
	t.Helper()
	intend := `{"workerId":"` + worker + `","runId":"` + run.ID + `","attemptId":"` + attempt.ID + `","generation":1}`
	if rec := serveRun(t, h, "POST", "/api/v1/workers/session/intend", intend, testWorkerToken); rec.Code != http.StatusCreated {
		t.Fatalf("intend: got %d (%s)", rec.Code, rec.Body.String())
	}
	bind := `{"workerId":"` + worker + `","runId":"` + run.ID + `","attemptId":"` + attempt.ID +
		`","generation":1,"workspaceId":"` + workspaceID + `","sessionId":"` + sessionID + `"}`
	if rec := serveRun(t, h, "POST", "/api/v1/workers/session/bind", bind, testWorkerToken); rec.Code != http.StatusOK {
		t.Fatalf("bind: got %d (%s)", rec.Code, rec.Body.String())
	}
}

// TestPaseoSessionListingProjectionOverHTTP covers the S3-C read-only
// cross-Run projection: live-first ordering, exact filters, bounded limits,
// scoped authorization and the event cursor used to start the live stream.
func TestPaseoSessionListingProjectionOverHTTP(t *testing.T) {
	srv, _ := newRunTestServer(t)
	h := srv.Handler()

	rec := serveRun(t, h, "GET", "/api/v1/sessions", "", testToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("empty session list: got %d (%s)", rec.Code, rec.Body.String())
	}
	sessions, cursor := decodeSessionList(t, rec)
	if len(sessions) != 0 {
		t.Fatalf("want empty session list, got %+v", sessions)
	}
	if cursor < 0 {
		t.Fatalf("event cursor must be non-negative, got %d", cursor)
	}

	// Terminal codex session.
	runA := queuePaseoRun(t, h, "")
	attemptA := startPaseoAttempt(t, h, runA, "s3c-a")
	bindSessionForTest(t, h, runA, attemptA, "s3c-a", "ws_a", "aaaaaaaa-1111-4111-8111-aaaaaaaaaaa1")
	closeA := `{"workerId":"s3c-a","runId":"` + runA.ID + `","attemptId":"` + attemptA.ID +
		`","generation":1,"outcome":"succeeded"}`
	if rec := serveRun(t, h, "POST", "/api/v1/workers/session/close", closeA, testWorkerToken); rec.Code != http.StatusOK {
		t.Fatalf("close A: got %d (%s)", rec.Code, rec.Body.String())
	}
	reportA := `{"workerId":"s3c-a","runId":"` + runA.ID + `","attemptId":"` + attemptA.ID +
		`","generation":1,"outcome":"completed","output":{"diffBytes":12,"testExit":0,"formatExit":0}}`
	if rec := serveRun(t, h, "POST", "/api/v1/workers/report", reportA, testWorkerToken); rec.Code != http.StatusOK {
		t.Fatalf("report A: got %d (%s)", rec.Code, rec.Body.String())
	}

	// Live OpenCode session for provider filtering and live-first ordering.
	bodyB := `{"workloadKind":"paseo-task","workloadInput":{"task":"add multiply"},"executionMode":"trusted-local","provider":"opencode","model":"opencode-go/deepseek-v4.1-flash","paseoMode":"build"}`
	rec = serveRun(t, h, "POST", "/api/v1/runs", bodyB, testToken)
	if rec.Code != http.StatusCreated {
		t.Fatalf("queue opencode run: got %d (%s)", rec.Code, rec.Body.String())
	}
	runB, _ := decodeRunBody(t, rec)
	attemptB := startPaseoAttempt(t, h, runB, "s3c-b")
	bindSessionForTest(t, h, runB, attemptB, "s3c-b", "ws_b", "bbbbbbbb-2222-4222-8222-bbbbbbbbbbb2")

	sessions, cursor = decodeSessionList(t, serveRun(t, h, "GET", "/api/v1/sessions", "", testToken))
	if len(sessions) != 2 {
		t.Fatalf("want 2 sessions, got %d (%+v)", len(sessions), sessions)
	}
	if sessions[0].RunID != runB.ID || sessions[0].Status != domain.PaseoActive {
		t.Fatalf("live session must sort first: %+v", sessions[0])
	}
	if sessions[0].RunStatus != domain.RunRunning {
		t.Fatalf("live session runStatus = %s, want running", sessions[0].RunStatus)
	}
	if sessions[1].RunID != runA.ID || sessions[1].Status != domain.PaseoSucceeded ||
		sessions[1].RunStatus != domain.RunSucceeded {
		t.Fatalf("terminal session projection wrong: %+v", sessions[1])
	}
	if cursor < 1 {
		t.Fatalf("event cursor must advance after committed events, got %d", cursor)
	}

	sessions, _ = decodeSessionList(t, serveRun(t, h, "GET", "/api/v1/sessions?provider=codex", "", testToken))
	if len(sessions) != 1 || sessions[0].RunID != runA.ID {
		t.Fatalf("provider filter wrong: %+v", sessions)
	}
	sessions, _ = decodeSessionList(t, serveRun(t, h, "GET", "/api/v1/sessions?provider=opencode", "", testToken))
	if len(sessions) != 1 || sessions[0].RunID != runB.ID {
		t.Fatalf("provider filter wrong: %+v", sessions)
	}
	sessions, _ = decodeSessionList(t, serveRun(t, h, "GET", "/api/v1/sessions?status=active", "", testToken))
	if len(sessions) != 1 || sessions[0].RunID != runB.ID {
		t.Fatalf("status filter wrong: %+v", sessions)
	}
	sessions, _ = decodeSessionList(t, serveRun(t, h, "GET", "/api/v1/sessions?status=succeeded&limit=1", "", testToken))
	if len(sessions) != 1 || sessions[0].RunID != runA.ID {
		t.Fatalf("status+limit filter wrong: %+v", sessions)
	}

	for _, url := range []string{
		"/api/v1/sessions?status=bogus",
		"/api/v1/sessions?limit=0",
		"/api/v1/sessions?limit=201",
		"/api/v1/sessions?limit=abc",
		"/api/v1/sessions?provider=" + strings.Repeat("x", 65),
	} {
		if rec := serveRun(t, h, "GET", url, "", testToken); rec.Code != http.StatusBadRequest {
			t.Fatalf("%s: got %d, want 400", url, rec.Code)
		}
	}
	if rec := serveRun(t, h, "GET", "/api/v1/sessions", "", testWorkerToken); rec.Code != http.StatusUnauthorized {
		t.Fatalf("worker token must not read operator session projection: got %d", rec.Code)
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

func registerPaseoWorker(t *testing.T, h http.Handler, worker string) {
	t.Helper()
	rec := serveRun(t, h, "POST", "/api/v1/workers/register",
		`{"workerId":"`+worker+`","version":"test-s3d"}`, testWorkerToken)
	if rec.Code != http.StatusCreated && rec.Code != http.StatusOK {
		t.Fatalf("register %s: got %d (%s)", worker, rec.Code, rec.Body.String())
	}
}

func orphanPaseoBinding(t *testing.T, h http.Handler, run domain.Run, attempt domain.Attempt, worker string) {
	t.Helper()
	intend := `{"workerId":"` + worker + `","runId":"` + run.ID + `","attemptId":"` + attempt.ID + `","generation":1}`
	if rec := serveRun(t, h, "POST", "/api/v1/workers/session/intend", intend, testWorkerToken); rec.Code != http.StatusCreated {
		t.Fatalf("intend: got %d (%s)", rec.Code, rec.Body.String())
	}
	bind := `{"workerId":"` + worker + `","runId":"` + run.ID + `","attemptId":"` + attempt.ID +
		`","generation":1,"workspaceId":"ws_recovery_http","sessionId":"99999999-9999-4999-8999-999999999999"}`
	if rec := serveRun(t, h, "POST", "/api/v1/workers/session/bind", bind, testWorkerToken); rec.Code != http.StatusOK {
		t.Fatalf("bind: got %d (%s)", rec.Code, rec.Body.String())
	}
	if rec := serveRun(t, h, "POST", "/api/v1/runs/"+run.ID+"/cancel", `{}`, testToken); rec.Code != http.StatusOK {
		t.Fatalf("cancel: got %d (%s)", rec.Code, rec.Body.String())
	}
}

func TestPaseoRecoveryOverHTTP(t *testing.T) {
	srv, _ := newRunTestServer(t)
	h := srv.Handler()
	run := queuePaseoRun(t, h, "")
	attempt := startPaseoAttempt(t, h, run, "s3d-1")
	orphanPaseoBinding(t, h, run, attempt, "s3d-1")
	registerPaseoWorker(t, h, "s3d-2")

	// A materialized pending receipt must not fabricate a year-1 timestamp.
	rec := serveRun(t, h, "GET", "/api/v1/runs/"+run.ID+"/sessions", "", testToken)
	if rec.Code != http.StatusOK || !strings.Contains(rec.Body.String(), `"status":"pending"`) {
		t.Fatalf("pending projection: got %d (%s)", rec.Code, rec.Body.String())
	}
	if strings.Contains(rec.Body.String(), "0001-01-01") {
		t.Fatalf("pending projection fabricated a timestamp: %s", rec.Body.String())
	}

	// Worker token may claim/complete; it may not read the operator projection.
	if rec := serveRun(t, h, "GET", "/api/v1/sessions", "", testWorkerToken); rec.Code != http.StatusUnauthorized {
		t.Fatalf("worker token sessions: got %d, want 401", rec.Code)
	}
	claimBody := `{"workerId":"s3d-1"}`
	rec = serveRun(t, h, "POST", "/api/v1/workers/recovery/claim", claimBody, testWorkerToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("claim: got %d (%s)", rec.Code, rec.Body.String())
	}
	var claimed struct {
		Claim domain.PaseoRecoveryClaim `json:"claim"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &claimed); err != nil {
		t.Fatalf("decode claim: %v", err)
	}
	if claimed.Claim.ClaimToken == "" || claimed.Claim.Session.AttemptID != attempt.ID {
		t.Fatalf("bad claim: %+v", claimed.Claim)
	}
	if claimed.Claim.Session.CorrelationKey == "" {
		t.Fatal("claim must expose the durable correlation key")
	}
	// A second Worker cannot steal a live claim.
	if rec := serveRun(t, h, "POST", "/api/v1/workers/recovery/claim", `{"workerId":"s3d-2"}`, testWorkerToken); rec.Code != http.StatusNoContent {
		t.Fatalf("second claim: got %d, want 204 (%s)", rec.Code, rec.Body.String())
	}
	complete := func(token, outcome, sessionID, workspaceID, sessionObs, workspaceObs string) *httptest.ResponseRecorder {
		body := `{"workerId":"s3d-1","attemptId":"` + attempt.ID + `","claimToken":"` + token +
			`","outcome":"` + outcome + `","sessionId":"` + sessionID + `","workspaceId":"` + workspaceID +
			`","sessionObserved":"` + sessionObs + `","workspaceObserved":"` + workspaceObs + `"}`
		return serveRun(t, h, "POST", "/api/v1/workers/recovery/complete", body, testWorkerToken)
	}
	if rec := complete("wrong-token", "confirmed", "99999999-9999-4999-8999-999999999999", "ws_recovery_http", "archived", "absent"); rec.Code != http.StatusConflict || !strings.Contains(rec.Body.String(), "stale_recovery_claim") {
		t.Fatalf("stale completion: got %d (%s)", rec.Code, rec.Body.String())
	}
	rec = complete(claimed.Claim.ClaimToken, "confirmed", "99999999-9999-4999-8999-999999999999", "ws_recovery_http", "archived", "absent")
	if rec.Code != http.StatusOK {
		t.Fatalf("complete: got %d (%s)", rec.Code, rec.Body.String())
	}
	if !strings.Contains(rec.Body.String(), `"status":"confirmed"`) {
		t.Fatalf("completion receipt: %s", rec.Body.String())
	}
	rec = complete(claimed.Claim.ClaimToken, "confirmed", "99999999-9999-4999-8999-999999999999", "ws_recovery_http", "archived", "absent")
	if rec.Code != http.StatusOK || rec.Header().Get("Idempotent-Replayed") != "true" {
		t.Fatalf("replay: got %d replay=%q", rec.Code, rec.Header().Get("Idempotent-Replayed"))
	}
	// The immutable binding stays orphaned; recovery is additional history.
	rec = serveRun(t, h, "GET", "/api/v1/runs/"+run.ID+"/sessions", "", testToken)
	if rec.Code != http.StatusOK || !strings.Contains(rec.Body.String(), `"status":"orphaned"`) ||
		!strings.Contains(rec.Body.String(), `"recovery":{`) || !strings.Contains(rec.Body.String(), `"status":"confirmed"`) {
		t.Fatalf("run session projection: got %d (%s)", rec.Code, rec.Body.String())
	}
	rec = serveRun(t, h, "GET", "/api/v1/sessions?status=orphaned", "", testToken)
	if rec.Code != http.StatusOK || !strings.Contains(rec.Body.String(), `"recovery":{`) {
		t.Fatalf("cross-run projection: got %d (%s)", rec.Code, rec.Body.String())
	}
}

func TestPaseoRecoveryIdentityConflictOverHTTP(t *testing.T) {
	srv, _ := newRunTestServer(t)
	h := srv.Handler()
	run := queuePaseoRun(t, h, "")
	attempt := startPaseoAttempt(t, h, run, "s3d-c1")
	orphanPaseoBinding(t, h, run, attempt, "s3d-c1")
	rec := serveRun(t, h, "POST", "/api/v1/workers/recovery/claim", `{"workerId":"s3d-c1"}`, testWorkerToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("claim: got %d", rec.Code)
	}
	var claimed struct {
		Claim domain.PaseoRecoveryClaim `json:"claim"`
	}
	_ = json.Unmarshal(rec.Body.Bytes(), &claimed)
	body := `{"workerId":"s3d-c1","attemptId":"` + attempt.ID + `","claimToken":"` + claimed.Claim.ClaimToken +
		`","outcome":"confirmed","sessionId":"aaaaaaaa-0000-4000-8000-000000000000","workspaceId":"ws_recovery_http","sessionObserved":"archived","workspaceObserved":"absent"}`
	rec = serveRun(t, h, "POST", "/api/v1/workers/recovery/complete", body, testWorkerToken)
	if rec.Code != http.StatusConflict || !strings.Contains(rec.Body.String(), "recovery_identity_conflict") {
		t.Fatalf("identity conflict: got %d (%s)", rec.Code, rec.Body.String())
	}
	// Invalid evidence is rejected before touching state.
	bad := `{"workerId":"s3d-c1","attemptId":"` + attempt.ID + `","claimToken":"` + claimed.Claim.ClaimToken +
		`","outcome":"confirmed","sessionObserved":"active","workspaceObserved":"present"}`
	if rec := serveRun(t, h, "POST", "/api/v1/workers/recovery/complete", bad, testWorkerToken); rec.Code != http.StatusBadRequest {
		t.Fatalf("invalid evidence: got %d, want 400 (%s)", rec.Code, rec.Body.String())
	}
}

func TestPaseoRetryGateOverHTTP(t *testing.T) {
	srv, _ := newRunTestServer(t)
	h := srv.Handler()
	run := queuePaseoRun(t, h, `{"workloadKind":"paseo-task","workloadInput":{"task":"add multiply"},"executionMode":"trusted-local","provider":"codex","model":"gpt-6-luna","paseoMode":"full-access","maxAttempts":2}`)
	attempt := startPaseoAttempt(t, h, run, "s3d-gate")
	orphanPaseoBindingViaFailure := func() {
		t.Helper()
		rec := serveRun(t, h, "POST", "/api/v1/workers/session/intend",
			`{"workerId":"s3d-gate","runId":"`+run.ID+`","attemptId":"`+attempt.ID+`","generation":1}`, testWorkerToken)
		if rec.Code != http.StatusCreated {
			t.Fatalf("intend: %d", rec.Code)
		}
		rec = serveRun(t, h, "POST", "/api/v1/workers/session/bind",
			`{"workerId":"s3d-gate","runId":"`+run.ID+`","attemptId":"`+attempt.ID+`","generation":1,"workspaceId":"ws_gate","sessionId":"bbbbbbbb-1111-4111-8111-bbbbbbbbbbbb"}`, testWorkerToken)
		if rec.Code != http.StatusOK {
			t.Fatalf("bind: %d", rec.Code)
		}
		rec = serveRun(t, h, "POST", "/api/v1/workers/report",
			`{"workerId":"s3d-gate","runId":"`+run.ID+`","attemptId":"`+attempt.ID+`","generation":1,"outcome":"failed","error":"agent failed"}`, testWorkerToken)
		if rec.Code != http.StatusOK {
			t.Fatalf("report failed: %d (%s)", rec.Code, rec.Body.String())
		}
	}
	orphanPaseoBindingViaFailure()
	// The requeued Run must not be leased until the orphan is settled.
	if rec := serveRun(t, h, "POST", "/api/v1/workers/poll", `{"workerId":"s3d-gate"}`, testWorkerToken); rec.Code != http.StatusNoContent {
		t.Fatalf("gated poll: got %d, want 204 (%s)", rec.Code, rec.Body.String())
	}
	rec := serveRun(t, h, "POST", "/api/v1/workers/recovery/claim", `{"workerId":"s3d-gate"}`, testWorkerToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("claim: got %d", rec.Code)
	}
	var claimed struct {
		Claim domain.PaseoRecoveryClaim `json:"claim"`
	}
	_ = json.Unmarshal(rec.Body.Bytes(), &claimed)
	body := `{"workerId":"s3d-gate","attemptId":"` + attempt.ID + `","claimToken":"` + claimed.Claim.ClaimToken +
		`","outcome":"confirmed","sessionId":"bbbbbbbb-1111-4111-8111-bbbbbbbbbbbb","workspaceId":"ws_gate","sessionObserved":"archived","workspaceObserved":"absent"}`
	if rec := serveRun(t, h, "POST", "/api/v1/workers/recovery/complete", body, testWorkerToken); rec.Code != http.StatusOK {
		t.Fatalf("complete: got %d (%s)", rec.Code, rec.Body.String())
	}
	rec = serveRun(t, h, "POST", "/api/v1/workers/poll", `{"workerId":"s3d-gate"}`, testWorkerToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("poll after recovery: got %d (%s)", rec.Code, rec.Body.String())
	}
	var leased struct {
		Attempt domain.Attempt `json:"attempt"`
	}
	_ = json.Unmarshal(rec.Body.Bytes(), &leased)
	if leased.Attempt.LeaseGeneration != 2 || leased.Attempt.Seq != 2 {
		t.Fatalf("replacement attempt: %+v", leased.Attempt)
	}
}
