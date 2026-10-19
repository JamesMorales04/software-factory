package api

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"context"
	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/store"
	"github.com/JamesMorales04/software-factory/core/internal/telemetry"
	"path/filepath"
)

const testWorkerToken = "test-worker-token-xyz"

func newRunTestServer(t *testing.T) (*Server, *store.DB) {
	t.Helper()
	db, err := store.Open(filepath.Join(t.TempDir(), "runapi.db"))
	if err != nil {
		t.Fatalf("Open: %v", err)
	}
	t.Cleanup(func() { _ = db.Close() })
	logger := telemetry.NewLogger()
	tracer, _ := telemetry.InitTracing(context.Background(), "test")
	srv := New(db, testToken, logger, tracer, NewHub()).WithWorkerToken(testWorkerToken)
	return srv, db
}

func serveRun(t *testing.T, h http.Handler, method, url, body, token string) *httptest.ResponseRecorder {
	t.Helper()
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, authed(method, url, body, token))
	return rec
}

func decodeRunBody(t *testing.T, rec *httptest.ResponseRecorder) (domain.Run, domain.Attempt) {
	t.Helper()
	var out struct {
		Run     domain.Run     `json:"run"`
		Attempt domain.Attempt `json:"attempt"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &out); err != nil {
		t.Fatalf("decode run body: %v (%s)", err, rec.Body.String())
	}
	return out.Run, out.Attempt
}

func queueSyntheticRun(t *testing.T, h http.Handler, body string) domain.Run {
	t.Helper()
	if body == "" {
		body = `{"workloadKind":"synthetic-echo","workloadInput":{"message":"api-proof","steps":2,"stepMs":5}}`
	}
	rec := serveRun(t, h, "POST", "/api/v1/runs", body, testToken)
	if rec.Code != http.StatusCreated {
		t.Fatalf("queue run: got %d (%s)", rec.Code, rec.Body.String())
	}
	run, _ := decodeRunBody(t, rec)
	if run.Status != domain.RunQueued {
		t.Fatalf("run status = %s, want queued", run.Status)
	}
	return run
}

func registerPoll(t *testing.T, h http.Handler, worker string) (domain.Run, domain.Attempt) {
	t.Helper()
	rec := serveRun(t, h, "POST", "/api/v1/workers/register",
		`{"workerId":"`+worker+`","version":"test-1"}`, testWorkerToken)
	if rec.Code != http.StatusCreated && rec.Code != http.StatusOK {
		t.Fatalf("register: got %d (%s)", rec.Code, rec.Body.String())
	}
	rec = serveRun(t, h, "POST", "/api/v1/workers/poll", `{"workerId":"`+worker+`"}`, testWorkerToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("poll: got %d (%s)", rec.Code, rec.Body.String())
	}
	return decodeRunBody(t, rec)
}

func TestRunLifecycleOverHTTP(t *testing.T) {
	srv, db := newRunTestServer(t)
	h := srv.Handler()

	run := queueSyntheticRun(t, h, "")

	rec := serveRun(t, h, "GET", "/api/v1/runs/"+run.ID, "", testToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("get run: got %d", rec.Code)
	}
	rec = serveRun(t, h, "GET", "/api/v1/runs", "", testToken)
	if rec.Code != http.StatusOK || !strings.Contains(rec.Body.String(), run.ID) {
		t.Fatalf("list runs: got %d (%s)", rec.Code, rec.Body.String())
	}
	rec = serveRun(t, h, "GET", "/api/v1/runs?status=queued", "", testToken)
	if rec.Code != http.StatusOK || !strings.Contains(rec.Body.String(), run.ID) {
		t.Fatalf("filtered list: got %d (%s)", rec.Code, rec.Body.String())
	}

	leased, attempt := registerPoll(t, h, "local-1")
	if leased.ID != run.ID || attempt.Seq != 1 || attempt.LeaseGeneration != 1 {
		t.Fatalf("bad lease: %+v %+v", leased, attempt)
	}

	hb := `{"workerId":"local-1","runId":"` + run.ID + `","attemptId":"` + attempt.ID + `","generation":1}`
	rec = serveRun(t, h, "POST", "/api/v1/workers/heartbeat", hb, testWorkerToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("heartbeat: got %d (%s)", rec.Code, rec.Body.String())
	}
	started, _ := decodeRunBody(t, rec)
	if started.Status != domain.RunRunning {
		t.Fatalf("run status = %s, want running", started.Status)
	}

	report := `{"workerId":"local-1","runId":"` + run.ID + `","attemptId":"` + attempt.ID +
		`","generation":1,"outcome":"completed","output":{"receipt":"ok"}}`
	rec = serveRun(t, h, "POST", "/api/v1/workers/report", report, testWorkerToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("report: got %d (%s)", rec.Code, rec.Body.String())
	}
	finished, _ := decodeRunBody(t, rec)
	if finished.Status != domain.RunSucceeded {
		t.Fatalf("run status = %s, want succeeded", finished.Status)
	}

	// Duplicate report replays without a new event.
	before, _ := db.CountEvents()
	rec = serveRun(t, h, "POST", "/api/v1/workers/report", report, testWorkerToken)
	if rec.Code != http.StatusOK || rec.Header().Get("Idempotent-Replayed") != "true" {
		t.Fatalf("replay: got %d replay=%q (%s)", rec.Code, rec.Header().Get("Idempotent-Replayed"), rec.Body.String())
	}
	after, _ := db.CountEvents()
	if after != before {
		t.Fatalf("replay added events: %d -> %d", before, after)
	}

	rec = serveRun(t, h, "GET", "/api/v1/runs/"+run.ID+"/attempts", "", testToken)
	if rec.Code != http.StatusOK || !strings.Contains(rec.Body.String(), attempt.ID) {
		t.Fatalf("attempts: got %d (%s)", rec.Code, rec.Body.String())
	}
}

func TestWorkerAuthIsScoped(t *testing.T) {
	srv, _ := newRunTestServer(t)
	h := srv.Handler()

	// Operator token rejected on every Worker protocol route.
	for _, route := range []string{"/api/v1/workers/register", "/api/v1/workers/poll", "/api/v1/workers/heartbeat", "/api/v1/workers/report"} {
		rec := serveRun(t, h, "POST", route, `{"workerId":"local-1"}`, testToken)
		if rec.Code != http.StatusUnauthorized {
			t.Fatalf("%s with operator token: got %d, want 401", route, rec.Code)
		}
	}
	// Worker token rejected on operator routes.
	rec := serveRun(t, h, "GET", "/api/v1/runs", "", testWorkerToken)
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("operator route with worker token: got %d, want 401", rec.Code)
	}
	rec = serveRun(t, h, "POST", "/api/v1/runs", `{"workloadKind":"synthetic-echo"}`, testWorkerToken)
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("queue with worker token: got %d, want 401", rec.Code)
	}
	// Missing token rejected on worker routes.
	rec = serveRun(t, h, "POST", "/api/v1/workers/poll", `{"workerId":"local-1"}`, "")
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("worker route without token: got %d, want 401", rec.Code)
	}
}

func TestRunFencingOverHTTP(t *testing.T) {
	srv, db := newRunTestServer(t)
	h := srv.Handler()
	db.SetLeaseTTL(0) // every lease is immediately expired

	run := queueSyntheticRun(t, h, "")
	_, attempt := registerPoll(t, h, "w-fence")

	// Expired lease rejects heartbeats with lease_expired.
	hb := `{"workerId":"w-fence","runId":"` + run.ID + `","attemptId":"` + attempt.ID + `","generation":1}`
	rec := serveRun(t, h, "POST", "/api/v1/workers/heartbeat", hb, testWorkerToken)
	if rec.Code != http.StatusConflict || !strings.Contains(rec.Body.String(), "lease_expired") {
		t.Fatalf("expired heartbeat: got %d (%s)", rec.Code, rec.Body.String())
	}

	// Unknown generation rejected as stale.
	rec = serveRun(t, h, "POST", "/api/v1/workers/report",
		`{"workerId":"w-fence","runId":"`+run.ID+`","attemptId":"`+attempt.ID+`","generation":99,"outcome":"completed"}`,
		testWorkerToken)
	if rec.Code != http.StatusConflict || !strings.Contains(rec.Body.String(), "stale_generation") {
		t.Fatalf("stale report: got %d (%s)", rec.Code, rec.Body.String())
	}

	// Unknown worker rejected as not found.
	rec = serveRun(t, h, "POST", "/api/v1/workers/poll", `{"workerId":"ghost"}`, testWorkerToken)
	if rec.Code != http.StatusNotFound {
		t.Fatalf("unknown worker poll: got %d, want 404", rec.Code)
	}

	// Unknown run rejected as not found.
	rec = serveRun(t, h, "POST", "/api/v1/workers/heartbeat",
		`{"workerId":"w-fence","runId":"00000000000000000000000000000000","attemptId":"`+attempt.ID+`","generation":1}`,
		testWorkerToken)
	if rec.Code != http.StatusNotFound {
		t.Fatalf("unknown run heartbeat: got %d, want 404", rec.Code)
	}

	// Cancel then race a completion: the completion loses.
	cancelled := serveRun(t, h, "POST", "/api/v1/runs/"+run.ID+"/cancel", `{}`, testToken)
	if cancelled.Code != http.StatusOK {
		t.Fatalf("cancel: got %d (%s)", cancelled.Code, cancelled.Body.String())
	}
	rec = serveRun(t, h, "POST", "/api/v1/workers/report",
		`{"workerId":"w-fence","runId":"`+run.ID+`","attemptId":"`+attempt.ID+`","generation":1,"outcome":"completed"}`,
		testWorkerToken)
	if rec.Code != http.StatusConflict || !strings.Contains(rec.Body.String(), "run_cancelled") {
		t.Fatalf("report after cancel: got %d (%s)", rec.Code, rec.Body.String())
	}
}

func TestRunValidationOverHTTP(t *testing.T) {
	srv, _ := newRunTestServer(t)
	h := srv.Handler()
	bodies := []string{
		`{}`,
		`{"workloadKind":"shell-exec","workloadInput":{"message":"x"}}`,
		`{"workloadKind":"synthetic-echo","workloadInput":{"message":"x","cmd":"id"}}`,
		`{"workloadKind":"synthetic-echo","workloadInput":{"message":""}}`,
		`{"workloadKind":"synthetic-echo","workloadInput":{"message":"x"},"maxAttempts":99}`,
		`{"workloadKind":"synthetic-echo","workloadInput":{"message":"x"},"unknownField":1}`,
		`{"workloadKind":"synthetic-echo","workloadInput":{"message":"x"},"actionId":"nope"}`,
		`{"workloadKind":"synthetic-echo","workloadInput":{"message":"x"},"projectId":"BAD"}`,
	}
	for _, b := range bodies {
		rec := serveRun(t, h, "POST", "/api/v1/runs", b, testToken)
		if rec.Code != http.StatusBadRequest {
			t.Fatalf("body %q: got %d, want 400 (%s)", b, rec.Code, rec.Body.String())
		}
	}
}

func TestPollEmptyReturns204(t *testing.T) {
	srv, _ := newRunTestServer(t)
	h := srv.Handler()
	rec := serveRun(t, h, "POST", "/api/v1/workers/register", `{"workerId":"idle-1"}`, testWorkerToken)
	if rec.Code != http.StatusCreated {
		t.Fatalf("register: got %d", rec.Code)
	}
	rec = serveRun(t, h, "POST", "/api/v1/workers/poll", `{"workerId":"idle-1"}`, testWorkerToken)
	if rec.Code != http.StatusNoContent {
		t.Fatalf("empty poll: got %d, want 204", rec.Code)
	}
}

func TestWorkerObservationEndpoints(t *testing.T) {
	srv, _ := newRunTestServer(t)
	h := srv.Handler()
	rec := serveRun(t, h, "POST", "/api/v1/workers/register", `{"workerId":"obs-1","version":"v"}`, testWorkerToken)
	if rec.Code != http.StatusCreated {
		t.Fatalf("register: got %d", rec.Code)
	}
	rec = serveRun(t, h, "GET", "/api/v1/workers", "", testToken)
	if rec.Code != http.StatusOK || !strings.Contains(rec.Body.String(), "obs-1") {
		t.Fatalf("list workers: got %d (%s)", rec.Code, rec.Body.String())
	}
	rec = serveRun(t, h, "GET", "/api/v1/workers/obs-1", "", testToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("get worker: got %d", rec.Code)
	}
	rec = serveRun(t, h, "GET", "/api/v1/workers/ghost", "", testToken)
	if rec.Code != http.StatusNotFound {
		t.Fatalf("unknown worker: got %d, want 404", rec.Code)
	}
}
