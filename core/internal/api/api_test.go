package api

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/store"
	"github.com/JamesMorales04/software-factory/core/internal/telemetry"
)

const testToken = "test-token-abcdef"

func newTestServer(t *testing.T) (*Server, *store.DB) {
	t.Helper()
	db, err := store.Open(filepath.Join(t.TempDir(), "api.db"))
	if err != nil {
		t.Fatalf("Open: %v", err)
	}
	t.Cleanup(func() { _ = db.Close() })
	logger := telemetry.NewLogger()
	tracer, _ := telemetry.InitTracing(context.Background(), "test")
	srv := New(db, testToken, logger, tracer, NewHub())
	return srv, db
}

func authed(method, url, body, token string) *http.Request {
	var reader *strings.Reader
	if body == "" {
		reader = strings.NewReader("")
	} else {
		reader = strings.NewReader(body)
	}
	req := httptest.NewRequest(method, url, reader)
	if token != "" {
		req.Header.Set("Authorization", "Bearer "+token)
	}
	req.Header.Set("Content-Type", "application/json")
	return req
}

func TestUnauthorizedIsRejected(t *testing.T) {
	srv, _ := newTestServer(t)
	for _, req := range []*http.Request{
		authed("POST", "/api/v1/actions", `{"title":"x"}`, ""),
		authed("POST", "/api/v1/actions", `{"title":"x"}`, "wrong"),
		authed("GET", "/api/v1/actions", "", ""),
		authed("GET", "/api/v1/health", "", ""),
	} {
		rec := httptest.NewRecorder()
		srv.Handler().ServeHTTP(rec, req)
		if rec.Code != http.StatusUnauthorized {
			t.Fatalf("%s %s: got %d, want 401", req.Method, req.URL.Path, rec.Code)
		}
	}
}

func TestInvalidPayloadsRejected(t *testing.T) {
	srv, _ := newTestServer(t)
	bodies := []string{
		`{}`,
		`{"title":""}`,
		`{"title":"x","unknownField":1}`,
		`not json`,
		`{"title":"x"} {"smuggled":true}`,
		`{"title":"x"} trailing-bytes`,
	}
	for _, b := range bodies {
		rec := httptest.NewRecorder()
		srv.Handler().ServeHTTP(rec, authed("POST", "/api/v1/actions", b, testToken))
		if rec.Code != http.StatusBadRequest {
			t.Fatalf("body %q: got %d, want 400", b, rec.Code)
		}
	}
}

func TestActionLifecycleOverHTTP(t *testing.T) {
	srv, _ := newTestServer(t)
	h := srv.Handler()

	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, authed("POST", "/api/v1/actions", `{"title":"HTTP slice"}`, testToken))
	if rec.Code != http.StatusCreated {
		t.Fatalf("create: got %d (%s)", rec.Code, rec.Body.String())
	}
	var created struct {
		Action domain.Action `json:"action"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &created); err != nil {
		t.Fatalf("decode create: %v", err)
	}
	if cid := rec.Header().Get("X-Correlation-ID"); cid == "" {
		t.Fatal("missing X-Correlation-ID response header")
	}

	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, authed("GET", "/api/v1/actions", "", testToken))
	if rec.Code != http.StatusOK || !strings.Contains(rec.Body.String(), "HTTP slice") {
		t.Fatalf("list: got %d (%s)", rec.Code, rec.Body.String())
	}

	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, authed("GET", "/api/v1/actions/"+created.Action.ID, "", testToken))
	if rec.Code != http.StatusOK {
		t.Fatalf("get: got %d", rec.Code)
	}

	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, authed("POST", "/api/v1/actions/"+created.Action.ID+"/transition",
		`{"to":"completed"}`, testToken))
	if rec.Code != http.StatusConflict {
		t.Fatalf("illegal jump: got %d, want 409", rec.Code)
	}

	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, authed("POST", "/api/v1/actions/"+created.Action.ID+"/transition",
		`{"to":"in_progress"}`, testToken))
	if rec.Code != http.StatusOK {
		t.Fatalf("transition: got %d (%s)", rec.Code, rec.Body.String())
	}

	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, authed("GET", "/api/v1/actions/"+created.Action.ID+"/events", "", testToken))
	var ev struct {
		Events []domain.Event `json:"events"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &ev); err != nil || len(ev.Events) != 2 {
		t.Fatalf("action events: %+v %v", ev, err)
	}

	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, authed("GET", "/api/v1/actions/missing", "", testToken))
	if rec.Code != http.StatusNotFound {
		t.Fatalf("missing action: got %d, want 404", rec.Code)
	}
}

func TestIdempotencyKeyOverHTTP(t *testing.T) {
	srv, db := newTestServer(t)
	h := srv.Handler()

	doCreate := func() (int, string, string) {
		req := authed("POST", "/api/v1/actions", `{"title":"dup"}`, testToken)
		req.Header.Set("Idempotency-Key", "dup-key-1")
		rec := httptest.NewRecorder()
		h.ServeHTTP(rec, req)
		var parsed struct {
			Action domain.Action `json:"action"`
		}
		_ = json.Unmarshal(rec.Body.Bytes(), &parsed)
		return rec.Code, parsed.Action.ID, rec.Header().Get("Idempotent-Replayed")
	}

	code1, id1, _ := doCreate()
	code2, id2, replayed := doCreate()
	if code1 != http.StatusCreated || code2 != http.StatusOK {
		t.Fatalf("statuses %d/%d, want 201/200", code1, code2)
	}
	if id1 != id2 || replayed != "true" {
		t.Fatalf("replay mismatch: %q vs %q replayed=%q", id1, id2, replayed)
	}
	if n, _ := db.CountActions(); n != 1 {
		t.Fatalf("duplicate actions: %d", n)
	}
	if n, _ := db.CountEvents(); n != 1 {
		t.Fatalf("duplicate events: %d", n)
	}
}

func TestEventStreamReplaysMissedSuffix(t *testing.T) {
	srv, _ := newTestServer(t)
	h := srv.Handler()

	create := func(title string) {
		rec := httptest.NewRecorder()
		h.ServeHTTP(rec, authed("POST", "/api/v1/actions", `{"title":"`+title+`"}`, testToken))
		if rec.Code != http.StatusCreated {
			t.Fatalf("create %s: %d", title, rec.Code)
		}
	}
	create("s1")
	create("s2")
	// Cursor after the first event must replay exactly the remainder.
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	req := authed("GET", "/api/v1/events/stream?after=1", "", testToken).WithContext(ctx)
	rec := httptest.NewRecorder()
	done := make(chan struct{})
	go func() { h.ServeHTTP(rec, req); close(done) }()

	time.Sleep(300 * time.Millisecond)
	cancel()
	select {
	case <-done:
	case <-time.After(5 * time.Second):
		t.Fatal("stream handler did not exit on cancel")
	}
	body := rec.Body.String()
	if !strings.Contains(body, "event: factory") || !strings.Contains(body, `"title":"s2"`) {
		t.Fatalf("stream did not replay missed event: %.300s", body)
	}
	if strings.Contains(body, `"title":"s1"`) {
		t.Fatalf("stream replayed already-seen event: %.300s", body)
	}
}

func TestBrokenOTLPExporterDoesNotBlockPersistence(t *testing.T) {
	t.Setenv("OTEL_EXPORTER_OTLP_ENDPOINT", "http://127.0.0.1:1")
	srv, db := newTestServer(t)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, authed("POST", "/api/v1/actions", `{"title":"otel-proof"}`, testToken))
	if rec.Code != http.StatusCreated {
		t.Fatalf("create with dead exporter: got %d", rec.Code)
	}
	if n, _ := db.CountActions(); n != 1 {
		t.Fatalf("business write lost: %d actions", n)
	}
}

func TestStreamRequiresAuth(t *testing.T) {
	srv, _ := newTestServer(t)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, authed("GET", "/api/v1/events/stream?after=0", "", ""))
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("unauthenticated stream: got %d, want 401", rec.Code)
	}
}

func TestStaticBundleIsPublicWhileAPIStaysGated(t *testing.T) {
	webDir := t.TempDir()
	index := "<!doctype html><title>office</title>"
	if err := os.WriteFile(filepath.Join(webDir, "index.html"), []byte(index), 0o644); err != nil {
		t.Fatalf("fixture: %v", err)
	}
	srv, _ := newTestServer(t)
	srv.WithWebDir(webDir)
	h := srv.Handler()

	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, httptest.NewRequest("GET", "/", nil))
	if rec.Code != http.StatusOK || !strings.Contains(rec.Body.String(), "office") {
		t.Fatalf("public bundle: got %d (%s)", rec.Code, rec.Body.String())
	}

	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, httptest.NewRequest("GET", "/api/v1/actions", nil))
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("api without token: got %d, want 401", rec.Code)
	}
}

// streamCollect reads SSE data frames from a live server until want events
// arrive or the timeout fires. It uses a real HTTP server (not a
// ResponseRecorder) so handler/recorder races cannot hide stream defects.
func streamCollect(t *testing.T, streamURL string, want int, timeout time.Duration) []domain.Event {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()
	req, err := http.NewRequestWithContext(ctx, "GET", streamURL, nil)
	if err != nil {
		t.Fatalf("stream request: %v", err)
	}
	req.Header.Set("Authorization", "Bearer "+testToken)
	res, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("stream dial: %v", err)
	}
	defer func() { _ = res.Body.Close() }()
	if res.StatusCode != http.StatusOK {
		t.Fatalf("stream status: %d", res.StatusCode)
	}
	var out []domain.Event
	scanner := bufio.NewScanner(res.Body)
	scanner.Buffer(make([]byte, 1<<20), 1<<20)
	for scanner.Scan() {
		line := scanner.Text()
		if !strings.HasPrefix(line, "data:") {
			continue
		}
		var e domain.Event
		if err := json.Unmarshal([]byte(strings.TrimSpace(strings.TrimPrefix(line, "data:"))), &e); err != nil {
			t.Fatalf("decode frame: %v", err)
		}
		out = append(out, e)
		if len(out) >= want {
			return out
		}
	}
	if err := scanner.Err(); err != nil && !errors.Is(err, context.DeadlineExceeded) {
		t.Logf("stream ended early: %v", err)
	}
	return out
}

func contiguousGlobalSeqs(t *testing.T, events []domain.Event) {
	t.Helper()
	for i := 1; i < len(events); i++ {
		if events[i].GlobalSeq != events[i-1].GlobalSeq+1 {
			t.Fatalf("gap or reorder at index %d: %+v", i, events)
		}
	}
}

// TestEventStreamPagesBeyondTheOldCap proves the backlog has no size cap:
// 550 committed events must all replay in commit order.
func TestEventStreamPagesBeyondTheOldCap(t *testing.T) {
	srv, db := newTestServer(t)
	ctx := context.Background()
	const n = 550
	for i := 0; i < n; i++ {
		p, _ := domain.NewActionParams{Title: "bulk"}.Validate()
		if _, _, err := db.CreateAction(ctx, p, "bulk"); err != nil {
			t.Fatalf("seed %d: %v", i, err)
		}
	}
	ts := httptest.NewServer(srv.Handler())
	defer ts.Close()

	got := streamCollect(t, ts.URL+"/api/v1/events/stream?after=0", n, 30*time.Second)
	if len(got) != n {
		t.Fatalf("replayed %d of %d events", len(got), n)
	}
	contiguousGlobalSeqs(t, got)
	if got[0].GlobalSeq != 1 {
		t.Fatalf("replay did not start at 1: %+v", got[0])
	}
}

// TestEventStreamLiveExactlyOnce subscribes, then commits 20 actions
// through the HTTP API (which publishes to the hub) and requires each
// event exactly once, in order — covering the old subscribe/replay race
// and any dropped-wake-up gap.
func TestEventStreamLiveExactlyOnce(t *testing.T) {
	srv, _ := newTestServer(t)
	ts := httptest.NewServer(srv.Handler())
	defer ts.Close()

	const n = 20
	collected := make(chan []domain.Event, 1)
	go func() {
		collected <- streamCollect(t, ts.URL+"/api/v1/events/stream?after=0", n, 30*time.Second)
	}()
	time.Sleep(200 * time.Millisecond) // let the subscription establish
	for i := 0; i < n; i++ {
		title := "live-" + itoa(i)
		req, _ := http.NewRequest("POST", ts.URL+"/api/v1/actions",
			strings.NewReader(`{"title":"`+title+`"}`))
		req.Header.Set("Authorization", "Bearer "+testToken)
		req.Header.Set("Content-Type", "application/json")
		res, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatalf("create %d: %v", i, err)
		}
		_ = res.Body.Close()
		if res.StatusCode != http.StatusCreated {
			t.Fatalf("create %d: status %d", i, res.StatusCode)
		}
	}
	got := <-collected
	if len(got) != n {
		t.Fatalf("live delivered %d of %d events", len(got), n)
	}
	contiguousGlobalSeqs(t, got)
	for i, e := range got {
		if e.EventType != domain.EventActionCreated {
			t.Fatalf("event %d: %+v", i, e)
		}
		if e.Payload["title"] != "live-"+itoa(i) {
			t.Fatalf("event %d out of order or wrong: %+v", i, e.Payload)
		}
	}
}

func itoa(i int) string { return strconv.Itoa(i) }

func TestHubPublishNeverBlocks(t *testing.T) {
	hub := NewHub()
	sub, unsub := hub.Subscribe()
	defer unsub()
	done := make(chan struct{})
	go func() {
		for i := 0; i < 2000; i++ {
			hub.Publish(domain.Event{EventType: domain.EventCoreStarted})
		}
		close(done)
	}()
	select {
	case <-done:
	case <-time.After(10 * time.Second):
		t.Fatal("Publish blocked on a slow consumer")
	}
	_ = sub
}
