package api

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/telemetry"
)

func createCampaignSpec(t *testing.T, h http.Handler) (domain.Action, domain.ActionSpec) {
	t.Helper()
	repo, base := qualityRepo(t)
	rec := serveRun(t, h, "POST", "/api/v1/actions", `{"title":"S4-B"}`, testToken)
	if rec.Code != http.StatusCreated {
		t.Fatalf("create action: %d %s", rec.Code, rec.Body.String())
	}
	var created struct {
		Action domain.Action `json:"action"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &created); err != nil {
		t.Fatal(err)
	}
	body := fmt.Sprintf(`{
		"actionId":%q,"goal":"Append multiply",
		"repository":%q,"baseRevision":%q,"allowedPaths":["add.js"],
		"acceptanceCriteria":[
			{"id":"format","kind":"format","description":"format","command":["node","--check","add.js"],"expectExit":0,"timeoutSecs":30},
			{"id":"test","kind":"test","description":"tests","command":["node","-e","0"],"expectExit":0,"timeoutSecs":30},
			{"id":"functional","kind":"functional","description":"multiply","command":["node","-e","0"],"expectExit":0,"timeoutSecs":30}
		],"negativeCases":[
			{"id":"negative","kind":"negative","description":"invalid","command":["node","-e","1"],"expectExit":1,"timeoutSecs":30}
		],"riskClass":"R0","qualityProfile":"node-basic",
		"budget":{"maxAttempts":1,"timeoutSecs":120},"executionMode":"trusted-local",
		"provider":"opencode","model":"opencode-go/deepseek-v4.1-flash","paseoMode":"build"
	}`, created.Action.ID, repo, base)
	rec = serveRun(t, h, "POST", "/api/v1/action-specs", body, testToken)
	if rec.Code != http.StatusCreated {
		t.Fatalf("create spec: %d %s", rec.Code, rec.Body.String())
	}
	var specOut struct {
		Spec domain.ActionSpec `json:"spec"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &specOut); err != nil {
		t.Fatal(err)
	}
	return created.Action, specOut.Spec
}

func TestCampaignCreationStartAndReplayOverHTTP(t *testing.T) {
	srv, _ := newTestServer(t)
	h := srv.Handler()
	action, spec := createCampaignSpec(t, h)

	body := fmt.Sprintf(`{"specId":%q,"maxRepairCycles":0,"maxWallClockSecs":600,"maxParallel":2}`, spec.ID)
	req := authed("POST", "/api/v1/campaigns", body, testToken)
	req.Header.Set("Idempotency-Key", "campaign-key-1")
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	if rec.Code != http.StatusCreated {
		t.Fatalf("create campaign: %d %s", rec.Code, rec.Body.String())
	}
	var projection domain.CampaignProjection
	if err := json.Unmarshal(rec.Body.Bytes(), &projection); err != nil {
		t.Fatal(err)
	}
	if projection.Campaign.ActionID != action.ID || projection.Campaign.SpecID != spec.ID ||
		projection.Campaign.Status != domain.CampaignPending || len(projection.Tasks) != 4 {
		t.Fatalf("campaign projection = %+v", projection)
	}
	if projection.Budget.ProviderBudget != "unknown" {
		t.Fatalf("provider budget = %q, want unknown", projection.Budget.ProviderBudget)
	}
	if projection.Campaign.PlanHash == "" || projection.Campaign.Plan.Hash() != projection.Campaign.PlanHash {
		t.Fatal("plan hash is not authoritative")
	}

	// Duplicate create is an idempotent replay.
	req = authed("POST", "/api/v1/campaigns", body, testToken)
	req.Header.Set("Idempotency-Key", "campaign-key-1")
	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	if rec.Code != http.StatusOK || rec.Header().Get("Idempotent-Replayed") != "true" {
		t.Fatalf("create replay: %d replayed=%q", rec.Code, rec.Header().Get("Idempotent-Replayed"))
	}

	rec = serveRun(t, h, "POST", "/api/v1/campaigns/"+projection.Campaign.ID+"/start", `{}`, testToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("start campaign: %d %s", rec.Code, rec.Body.String())
	}
	var started domain.CampaignProjection
	if err := json.Unmarshal(rec.Body.Bytes(), &started); err != nil {
		t.Fatal(err)
	}
	if started.Campaign.Status != domain.CampaignRunning {
		t.Fatalf("started status = %s", started.Campaign.Status)
	}
	if started.Tasks[0].Role != domain.RoleImplementer || started.Tasks[0].Status != domain.TaskReady {
		t.Fatalf("first task = %+v", started.Tasks[0])
	}

	rec = serveRun(t, h, "GET", "/api/v1/campaigns", "", testToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("list campaigns: %d", rec.Code)
	}
	rec = serveRun(t, h, "GET", "/api/v1/campaigns/"+projection.Campaign.ID+"/events", "", testToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("campaign events: %d", rec.Code)
	}
}

func TestCampaignAuthorityAndBudgetEscalationAreRejected(t *testing.T) {
	srv, _ := newTestServer(t)
	h := srv.Handler()
	_, spec := createCampaignSpec(t, h)

	body := fmt.Sprintf(`{"specId":%q,"maxRepairCycles":0}`, spec.ID)
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, authed("POST", "/api/v1/campaigns", body, "not-the-operator"))
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("wrong token create: %d", rec.Code)
	}
	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, authed("POST", "/api/v1/campaigns",
		fmt.Sprintf(`{"specId":%q,"maxRepairCycles":99}`, spec.ID), testToken))
	if rec.Code != http.StatusBadRequest {
		t.Fatalf("repair escalation: %d %s", rec.Code, rec.Body.String())
	}
	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, authed("POST", "/api/v1/campaigns",
		fmt.Sprintf(`{"specId":%q,"maxWallClockSecs":999999}`, spec.ID), testToken))
	if rec.Code != http.StatusBadRequest {
		t.Fatalf("wall clock escalation: %d", rec.Code)
	}
	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, authed("POST", "/api/v1/campaigns",
		`{"specId":"00000000000000000000000000000000"}`, testToken))
	if rec.Code != http.StatusBadRequest && rec.Code != http.StatusNotFound {
		t.Fatalf("unknown spec: %d", rec.Code)
	}
}

func TestDomainConflictMapsTo409Not500(t *testing.T) {
	logger := telemetry.NewLogger()
	rec := httptest.NewRecorder()
	mapDomainError(rec, fmt.Errorf("%w: evidence changed", domain.ErrConflict), logger, "test")
	if rec.Code != http.StatusConflict {
		t.Fatalf("conflict status = %d, want 409", rec.Code)
	}
	if errors.Is(domain.ErrConflict, domain.ErrNotFound) {
		t.Fatal("unreachable")
	}
}

func TestCampaignCancelIsAuthoritative(t *testing.T) {
	srv, _ := newTestServer(t)
	h := srv.Handler()
	_, spec := createCampaignSpec(t, h)
	rec := serveRun(t, h, "POST", "/api/v1/campaigns",
		fmt.Sprintf(`{"specId":%q,"maxRepairCycles":0}`, spec.ID), testToken)
	var projection domain.CampaignProjection
	_ = json.Unmarshal(rec.Body.Bytes(), &projection)
	rec = serveRun(t, h, "POST", "/api/v1/campaigns/"+projection.Campaign.ID+"/cancel", `{}`, testToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("cancel: %d %s", rec.Code, rec.Body.String())
	}
	var cancelled domain.CampaignProjection
	_ = json.Unmarshal(rec.Body.Bytes(), &cancelled)
	if cancelled.Campaign.Status != domain.CampaignCancelled {
		t.Fatalf("cancelled status = %s", cancelled.Campaign.Status)
	}
	rec = serveRun(t, h, "POST", "/api/v1/campaigns/"+projection.Campaign.ID+"/start", `{}`, testToken)
	if rec.Code != http.StatusConflict {
		t.Fatalf("start cancelled campaign: %d", rec.Code)
	}
	_ = context.Background()
}
