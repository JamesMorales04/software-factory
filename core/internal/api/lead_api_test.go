package api

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os/exec"
	"strings"
	"testing"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/lead"
	"github.com/JamesMorales04/software-factory/core/internal/store"
)

type fixtureAssessor struct {
	calls     int
	malformed bool
}

type reconciliationFailureAssessor struct{ calls int }

func (a *reconciliationFailureAssessor) Assess(context.Context, lead.Input) (lead.Assessment, lead.Observation, error) {
	a.calls++
	return lead.Assessment{}, lead.Observation{}, errors.New(`lead assessment Paseo run failed; reconciliation={"runCause":"secret=redacted","sessionReconciliation":{"status":"session_reconciled","sessionId":"session_test_1","requestedProvider":"opencode","requestedModel":"opencode-go/deepseek-v4.1-flash","requestedMode":"build","observedProvider":"opencode","observedModel":"opencode-go/deepseek-v4.1-flash","observedMode":"build","observedStatus":"closed","routeStatus":"verified","stop":"ok","archive":"ok"},"workspaceArchive":"failed: /private/path","workspaceCleanup":"failed"}`)
}

func (a *fixtureAssessor) Assess(_ context.Context, input lead.Input) (lead.Assessment, lead.Observation, error) {
	a.calls++
	assessment := lead.Assessment{
		SchemaRevision: 1, ProjectID: input.ProjectID, ActionID: input.ActionID, RequestID: input.RequestID,
		EvidenceRefs: []string{"repository-files"}, Intent: "change", Complexity: "small",
		EffortEstimate: lead.EffortEstimate{
			Status: "estimated", Unit: "person-hours", MinPersonHours: 1, MaxPersonHours: 2,
			Assumptions: []string{"One bounded implementation unit with existing project checks."},
		},
		ValidationNeeds:     []string{"Run syntax, unit, functional and negative checks."},
		Uncertainty:         []string{"The exact implementation has not yet been inspected."},
		SuggestedNextAction: "Review and start the bounded Campaign.",
		Proposal: domain.LeadPlanProposal{
			ProjectID: input.ProjectID, ActionID: input.ActionID, BaseRevision: input.BaseRevision,
			ProjectConfigHash: input.Project.ConfigHash, Objective: input.Objective, Class: domain.LeadFeature,
			AcceptanceCriteria: []domain.SpecCheck{
				{ID: "format", Kind: domain.CheckFormat, Description: "Syntax", Command: []string{"node", "--check", "add.js"}, TimeoutSecs: 30},
				{ID: "test", Kind: domain.CheckTest, Description: "Unit tests", Command: []string{"node", "--test"}, TimeoutSecs: 30},
				{ID: "functional", Kind: domain.CheckFunctional, Description: "Functional behavior", Command: []string{"node", "--test", "tests/functional.test.js"}, TimeoutSecs: 30},
			},
			NegativeCases:  []domain.SpecCheck{{ID: "negative", Kind: domain.CheckNegative, Description: "Invalid input", Command: []string{"node", "--test", "tests/negative.test.js"}, TimeoutSecs: 30}},
			QualityProfile: input.Project.QualityProfile, RiskClass: domain.RiskR0,
			Budget:   domain.SpecBudget{MaxAttempts: 1, TimeoutSecs: 120},
			Provider: input.Grant.Provider, Model: input.Grant.Model, PaseoMode: input.Grant.PaseoMode,
			MaxRepairCycles: 0, MaxWallClockSecs: 600, MaxParallel: 1, MaxSessions: 2,
			WorkUnits: []domain.LeadWorkUnit{{
				ID: "implementation", Kind: "implementation", AllowedPaths: []string{"add.js"},
				Capabilities: []domain.Capability{domain.CapRepositoryEdit, domain.CapNodeToolchain},
			}},
		},
	}
	if a.malformed {
		assessment.EvidenceRefs = []string{"unavailable-evidence"}
	}
	return assessment, lead.Observation{
		Mechanism: "MODEL", Provider: input.Grant.Provider, Model: input.Grant.Model,
		Mode: input.Grant.PaseoMode, SessionID: "fixture-session", InputBytes: 100, OutputBytes: 100,
	}, nil
}

func leadFixture(t *testing.T) (*Server, string, string) {
	t.Helper()
	srv, _ := newQualityTestServer(t)
	repo, base := qualityRepo(t)
	if out, err := exec.Command("git", "-C", repo, "branch", "-M", "main").CombinedOutput(); err != nil {
		t.Fatalf("name fixture branch: %v %s", err, out)
	}
	body := fmt.Sprintf(`{"id":"leadfixture","name":"Lead fixture","repositoryPath":%q,"baseRefs":["main"],"qualityProfile":"node-basic","workspaceScope":".","capabilities":["repository-edit","node-toolchain"],"executionMode":"trusted-local"}`, repo)
	rec := serveRun(t, srv.Handler(), "POST", "/api/v1/projects", body, testToken)
	if rec.Code != http.StatusCreated {
		t.Fatalf("register project: %d %s", rec.Code, rec.Body.String())
	}
	srv.WithLeadPaidSessionAuthorization(LeadPaidSessionAuthorization{
		AuthorizationID: "lead-fixture-auth", ProjectID: "leadfixture", BaseRevision: base, Provider: "opencode",
		Model: "opencode-go/deepseek-v4.1-flash", PaseoMode: "build", MaxSessions: 2,
	})
	expires, _ := json.Marshal(time.Now().UTC().Add(time.Hour).Format(time.RFC3339Nano))
	grantBody := fmt.Sprintf(`{"projectId":"leadfixture","revision":1,"objectiveClasses":["feature"],"allowedPaths":["add.js"],"capabilities":["repository-edit","node-toolchain"],"riskClasses":["R0"],"provider":"opencode","model":"opencode-go/deepseek-v4.1-flash","paseoMode":"build","maxAttempts":1,"maxTimeoutSecs":120,"maxRepairCycles":0,"maxWallClockSecs":600,"maxParallel":1,"maxSessions":2,"expiresAt":%s}`, expires)
	req := authed("POST", "/api/v1/delegations", grantBody, testToken)
	req.Header.Set("Idempotency-Key", "lead-fixture-auth")
	rec = httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusCreated {
		t.Fatalf("create delegation: %d %s", rec.Code, rec.Body.String())
	}
	var result struct {
		Delegation domain.StandingDelegation `json:"delegation"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &result); err != nil {
		t.Fatal(err)
	}
	return srv, result.Delegation.ID, repo
}

func submitLead(t *testing.T, h http.Handler, grantID, key string) *httptest.ResponseRecorder {
	t.Helper()
	body := fmt.Sprintf(`{"projectId":"leadfixture","objective":"Add search to add.js with tests","delegationId":%q}`, grantID)
	req := authed("POST", "/api/v1/lead/requests", body, testToken)
	req.Header.Set("Idempotency-Key", key)
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	return rec
}

func TestLeadObjectiveToFrozenSpecAndAuthorizedCampaign(t *testing.T) {
	srv, grantID, _ := leadFixture(t)
	fake := &fixtureAssessor{}
	srv.WithLeadAssessor(fake)
	h := srv.Handler()
	updates, unsubscribe := srv.hub.Subscribe()
	defer unsubscribe()
	rec := submitLead(t, h, grantID, "lead-test-1")
	if rec.Code != http.StatusCreated {
		t.Fatalf("submit: %d %s", rec.Code, rec.Body.String())
	}
	var out struct {
		Request struct{ ID, Status, ActionID, ActionSpecID, CampaignID, PlanHash, AuthorizationState string } `json:"request"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &out); err != nil {
		t.Fatal(err)
	}
	if out.Request.Status != "ready" || out.Request.AuthorizationState != "permitted" ||
		out.Request.ActionSpecID == "" || out.Request.CampaignID == "" || out.Request.PlanHash == "" {
		t.Fatalf("incomplete Lead projection: %s", rec.Body.String())
	}
	initialEvents := drainHubEvents(updates)
	seen := map[string]bool{}
	for _, event := range initialEvents {
		seen[event.EventType] = true
	}
	for _, eventType := range []string{
		domain.EventLeadRequestCreated, domain.EventLeadRequestStarted,
		domain.EventLeadAssessmentRecorded, domain.EventLeadPlanFrozen,
	} {
		if !seen[eventType] {
			t.Fatalf("live event %q was not published: %#v", eventType, initialEvents)
		}
	}
	rec = serveRun(t, h, "GET", "/api/v1/lead/requests/"+out.Request.ID+"/events", "", testToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("lead event history: %d %s", rec.Code, rec.Body.String())
	}
	var history struct {
		Events []domain.Event `json:"events"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &history); err != nil || len(history.Events) == 0 {
		t.Fatalf("lead event history is empty or malformed: %s", rec.Body.String())
	}
	rec = submitLead(t, h, grantID, "lead-test-1")
	if rec.Code != http.StatusOK || fake.calls != 1 || rec.Header().Get("Idempotent-Replayed") != "true" {
		t.Fatalf("duplicate invoked model or changed request: status=%d calls=%d", rec.Code, fake.calls)
	}
	if replayEvents := drainHubEvents(updates); len(replayEvents) != 0 {
		t.Fatalf("idempotent replay published duplicate live events: %#v", replayEvents)
	}
	changed := authed("POST", "/api/v1/lead/requests", fmt.Sprintf(`{"projectId":"leadfixture","objective":"Different objective","delegationId":%q}`, grantID), testToken)
	changed.Header.Set("Idempotency-Key", "lead-test-1")
	conflict := httptest.NewRecorder()
	h.ServeHTTP(conflict, changed)
	if conflict.Code != http.StatusConflict || fake.calls != 1 {
		t.Fatalf("changed request replay was not rejected before assessment: status=%d calls=%d body=%s", conflict.Code, fake.calls, conflict.Body.String())
	}
	second := submitLead(t, h, grantID, "lead-test-second-campaign")
	if second.Code != http.StatusConflict || fake.calls != 1 {
		t.Fatalf("delegation authorized more than one Campaign: status=%d calls=%d body=%s", second.Code, fake.calls, second.Body.String())
	}
	rec = serveRun(t, h, "POST", "/api/v1/runs", fmt.Sprintf(`{"specId":%q}`, out.Request.ActionSpecID), testToken)
	if rec.Code != http.StatusForbidden {
		t.Fatalf("Lead spec dispatched outside Campaign: %d", rec.Code)
	}
	rec = serveRun(t, h, "POST", "/api/v1/lead/requests/"+out.Request.ID+"/start", `{}`, testToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("start: %d %s", rec.Code, rec.Body.String())
	}
	var started struct {
		Request struct{ Status string } `json:"request"`
	}
	_ = json.Unmarshal(rec.Body.Bytes(), &started)
	if started.Request.Status != "running" {
		t.Fatalf("start status: %s", rec.Body.String())
	}
	startEvents := drainHubEvents(updates)
	startedEvent := false
	for _, event := range startEvents {
		if event.EventType == domain.EventCampaignStarted {
			startedEvent = true
		}
	}
	if !startedEvent {
		t.Fatalf("Campaign start was not published to live updates: %#v", startEvents)
	}
	rec = serveRun(t, h, "POST", "/api/v1/lead/requests/"+out.Request.ID+"/start", `{}`, testToken)
	if rec.Code != http.StatusOK || rec.Header().Get("Idempotent-Replayed") != "true" {
		t.Fatalf("repeat start was not idempotent: %d %s", rec.Code, rec.Body.String())
	}
	if repeatEvents := drainHubEvents(updates); len(repeatEvents) != 0 {
		t.Fatalf("repeated start published duplicate live events: %#v", repeatEvents)
	}
}

func drainHubEvents(ch <-chan domain.Event) []domain.Event {
	var events []domain.Event
	for {
		select {
		case event := <-ch:
			events = append(events, event)
		default:
			return events
		}
	}
}

func TestLeadRejectsMalformedAndUnauthorizedRequests(t *testing.T) {
	srv, grantID, _ := leadFixture(t)
	fake := &fixtureAssessor{}
	srv.WithLeadAssessor(fake)
	h := srv.Handler()
	req := httptest.NewRequest("POST", "/api/v1/lead/requests", strings.NewReader("{"))
	req.Header.Set("Authorization", "Bearer "+testToken)
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	if rec.Code != http.StatusBadRequest {
		t.Fatalf("malformed request: %d %s", rec.Code, rec.Body.String())
	}
	req = httptest.NewRequest("POST", "/api/v1/lead/requests", strings.NewReader(fmt.Sprintf(`{"projectId":"leadfixture","objective":"Do work","delegationId":%q}`, grantID)))
	req.Header.Set("Idempotency-Key", "unauthorized-lead")
	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("unauthorized request: %d %s", rec.Code, rec.Body.String())
	}
	req = authed("POST", "/api/v1/lead/requests", `{"projectId":"leadfixture","objective":"Do work","delegationId":"00000000000000000000000000000000"}`, testToken)
	req.Header.Set("Idempotency-Key", "unknown-delegation")
	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	if rec.Code != http.StatusForbidden || fake.calls != 0 {
		t.Fatalf("unknown delegation reached assessment: status=%d calls=%d body=%s", rec.Code, fake.calls, rec.Body.String())
	}
	req = authed("POST", "/api/v1/lead/requests", `{"projectId":"leadfixture","objective":"Do work"}`, testToken)
	req.Header.Set("Idempotency-Key", "missing-delegation")
	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	if rec.Code != http.StatusForbidden || fake.calls != 0 {
		t.Fatalf("missing delegation reached assessment: status=%d calls=%d body=%s", rec.Code, fake.calls, rec.Body.String())
	}
	req = authed("POST", "/api/v1/lead/requests", fmt.Sprintf(`{"projectId":"leadfixture","objective":"Do work","delegationId":%q}`, grantID), testToken)
	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	if rec.Code != http.StatusBadRequest || fake.calls != 0 {
		t.Fatalf("missing idempotency key reached assessment: status=%d calls=%d body=%s", rec.Code, fake.calls, rec.Body.String())
	}
}

func TestLeadPaidSessionAuthorizationIsClosedAndDurablyBound(t *testing.T) {
	srv, _, _ := leadFixture(t)
	base := srv.leadPaidSessionAuthorization.BaseRevision
	expires, _ := json.Marshal(time.Now().UTC().Add(time.Hour).Format(time.RFC3339Nano))
	postGrant := func(key, body string) *httptest.ResponseRecorder {
		req := authed("POST", "/api/v1/delegations", body, testToken)
		req.Header.Set("Idempotency-Key", key)
		rec := httptest.NewRecorder()
		srv.Handler().ServeHTTP(rec, req)
		return rec
	}
	body := func(revision, sessions int, provider, objectives string) string {
		return fmt.Sprintf(`{"projectId":"leadfixture","revision":%d,"objectiveClasses":%s,"allowedPaths":["add.js"],"capabilities":["repository-edit","node-toolchain"],"riskClasses":["R0"],"provider":%q,"model":"opencode-go/deepseek-v4.1-flash","paseoMode":"build","maxAttempts":1,"maxTimeoutSecs":120,"maxRepairCycles":0,"maxWallClockSecs":600,"maxParallel":1,"maxSessions":%d,"expiresAt":%s}`, revision, objectives, provider, sessions, expires)
	}
	if rec := postGrant("another-auth-id", body(2, 2, "opencode", `["feature"]`)); rec.Code != http.StatusForbidden {
		t.Fatalf("different authorization ID minted another grant: %d %s", rec.Code, rec.Body.String())
	}
	if rec := postGrant("lead-fixture-auth", body(2, 3, "opencode", `["feature"]`)); rec.Code != http.StatusForbidden {
		t.Fatalf("grant exceeded total session ceiling: %d %s", rec.Code, rec.Body.String())
	}
	if rec := postGrant("lead-fixture-auth", body(2, 2, "codex", `["feature"]`)); rec.Code != http.StatusForbidden {
		t.Fatalf("grant substituted provider route: %d %s", rec.Code, rec.Body.String())
	}
	if rec := postGrant("lead-fixture-auth", body(2, 2, "opencode", `["feature","maintenance"]`)); rec.Code != http.StatusConflict {
		t.Fatalf("authorization ID accepted different durable grant content: %d %s", rec.Code, rec.Body.String())
	}
	badBase := srv.leadPaidSessionAuthorization
	badBase.BaseRevision = strings.Repeat("0", 40)
	srv.WithLeadPaidSessionAuthorization(badBase)
	if rec := postGrant("lead-fixture-auth", body(2, 2, "opencode", `["feature"]`)); rec.Code != http.StatusForbidden {
		t.Fatalf("authorization base mismatch minted a grant: %d %s", rec.Code, rec.Body.String())
	}
	srv.WithLeadPaidSessionAuthorization(LeadPaidSessionAuthorization{})
	if rec := postGrant("lead-fixture-auth", body(2, 2, "opencode", `["feature"]`)); rec.Code != http.StatusForbidden {
		t.Fatalf("unconfigured paid authorization minted a grant: %d %s", rec.Code, rec.Body.String())
	}
	if base == "" {
		t.Fatal("fixture authorization did not bind a base revision")
	}
}

func TestLeadPaidSessionAuthorizationMustMatchBeforeAssessor(t *testing.T) {
	srv, grantID, _ := leadFixture(t)
	fake := &fixtureAssessor{}
	srv.WithLeadAssessor(fake).WithLeadPaidSessionAuthorization(LeadPaidSessionAuthorization{})
	rec := submitLead(t, srv.Handler(), grantID, "lead-no-paid-auth")
	if rec.Code != http.StatusCreated {
		t.Fatalf("intake: %d %s", rec.Code, rec.Body.String())
	}
	var out struct {
		Request struct{ Status, FailureReason, ActionSpecID, CampaignID string } `json:"request"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &out); err != nil {
		t.Fatal(err)
	}
	if out.Request.Status != "blocked" || fake.calls != 0 || out.Request.ActionSpecID != "" || out.Request.CampaignID != "" {
		t.Fatalf("closed paid authorization reached assessor or created executable plan: calls=%d body=%s", fake.calls, rec.Body.String())
	}
	if !strings.Contains(out.Request.FailureReason, "No matching bounded authorization") {
		t.Fatalf("missing paid authorization reason: %s", out.Request.FailureReason)
	}
}

func TestLeadProjectConfigDriftBlocksBeforeAssessor(t *testing.T) {
	srv, grantID, _ := leadFixture(t)
	fake := &fixtureAssessor{}
	srv.WithLeadAssessor(fake)
	project, err := srv.db.GetProject("leadfixture")
	if err != nil {
		t.Fatal(err)
	}
	body, _ := json.Marshal(map[string]string{"projectId": "leadfixture", "objective": "Add bounded feature", "delegationId": grantID})
	request, replayed, err := srv.db.CreateLeadRequest(context.Background(), store.NewLeadRequest{
		ProjectID: project.ID, Objective: "Add bounded feature", RequestJSON: string(body),
		IdempotencyKey: "config-drift", DelegationID: grantID, ProjectConfigHash: project.ConfigHash,
	})
	if err != nil || replayed {
		t.Fatalf("reserve lead request: replayed=%t err=%v", replayed, err)
	}
	newName := project.Name + " changed"
	if _, _, _, err := srv.db.UpdateProject(context.Background(), project.ID, domain.ProjectUpdateParams{Name: &newName}, "config-drift-test"); err != nil {
		t.Fatal(err)
	}
	srv.processLeadRequest(context.Background(), request)
	stored, err := srv.db.GetLeadRequest(context.Background(), request.ID)
	if err != nil {
		t.Fatal(err)
	}
	if fake.calls != 0 || stored.Status != "blocked" || !strings.Contains(stored.FailureReason, "configuration changed") {
		t.Fatalf("stale Project reached assessor: calls=%d status=%s reason=%q", fake.calls, stored.Status, stored.FailureReason)
	}
}

func TestLeadBoundActionCannotCreateOrStartCampaignBeforeLeadLink(t *testing.T) {
	srv, grantID, repo := leadFixture(t)
	ctx := context.Background()
	project, err := srv.db.GetProject("leadfixture")
	if err != nil {
		t.Fatal(err)
	}
	grant, err := srv.db.GetStandingDelegation(ctx, grantID)
	if err != nil {
		t.Fatal(err)
	}
	body, _ := json.Marshal(createLeadRequest{ProjectID: project.ID, Objective: "Add a bounded feature", DelegationID: grantID})
	request, replayed, err := srv.db.CreateLeadRequest(ctx, store.NewLeadRequest{
		ProjectID: project.ID, Objective: "Add a bounded feature", RequestJSON: string(body),
		IdempotencyKey: "unlinked-campaign-race", DelegationID: grantID, ProjectConfigHash: project.ConfigHash,
	})
	if err != nil || replayed {
		t.Fatalf("reserve request: replayed=%t err=%v", replayed, err)
	}
	actionParams, err := (domain.NewActionParams{
		ProjectID: project.ID, Title: request.Objective, Description: request.Objective,
		IdempotencyKey: "lead-action-" + request.ID,
	}).Validate()
	if err != nil {
		t.Fatal(err)
	}
	action, replayed, err := srv.db.CreateLeadAction(ctx, request.ID, actionParams, "race-test")
	if err != nil {
		t.Fatalf("create and bind Lead Action atomically: %v", err)
	}
	if replayed {
		t.Fatal("fresh Lead Action was marked replayed")
	}
	managed, err := srv.db.LeadActionManaged(ctx, action.ID)
	if err != nil || !managed {
		t.Fatalf("Action was observable before durable Lead binding: managed=%t err=%v", managed, err)
	}
	baseOut, err := exec.Command("git", "-C", repo, "rev-parse", "main").Output()
	if err != nil {
		t.Fatal(err)
	}
	base := strings.TrimSpace(string(baseOut))
	proposal := domain.LeadPlanProposal{
		ProjectID: project.ID, ActionID: action.ID, BaseRevision: base, ProjectConfigHash: project.ConfigHash,
		Objective: request.Objective, Class: domain.LeadFeature,
		AcceptanceCriteria: []domain.SpecCheck{
			{ID: "format", Kind: domain.CheckFormat, Description: "Syntax", Command: []string{"node", "--check", "add.js"}, TimeoutSecs: 30},
			{ID: "test", Kind: domain.CheckTest, Description: "Unit tests", Command: []string{"node", "--test"}, TimeoutSecs: 30},
			{ID: "functional", Kind: domain.CheckFunctional, Description: "Functional behavior", Command: []string{"node", "--test", "tests/functional.test.js"}, TimeoutSecs: 30},
		},
		NegativeCases:  []domain.SpecCheck{{ID: "negative", Kind: domain.CheckNegative, Description: "Invalid input", Command: []string{"node", "--test", "tests/negative.test.js"}, TimeoutSecs: 30}},
		QualityProfile: project.QualityProfile, RiskClass: domain.RiskR0, Budget: domain.SpecBudget{MaxAttempts: 1, TimeoutSecs: 120},
		Provider: grant.Provider, Model: grant.Model, PaseoMode: grant.PaseoMode,
		WorkUnits: []domain.LeadWorkUnit{{
			ID: "implementation", Kind: "implementation", AllowedPaths: []string{"add.js"},
			Capabilities: []domain.Capability{domain.CapRepositoryEdit, domain.CapNodeToolchain},
		}},
	}
	spec, err := srv.freezeLeadSpec(ctx, project, action, proposal)
	if err != nil {
		t.Fatalf("freeze fixture spec: %v", err)
	}
	// Simulate the old ordering: a Campaign exists before Lead links it.
	campaign, _, _, err := srv.db.CreateCampaign(ctx, domain.NewCampaignParams{SpecID: spec.ID, MaxRepairCycles: 0}, "race-test")
	if err != nil {
		t.Fatalf("create unlinked campaign fixture: %v", err)
	}
	h := srv.Handler()
	rec := serveRun(t, h, "POST", "/api/v1/campaigns", fmt.Sprintf(`{"specId":%q}`, spec.ID), testToken)
	if rec.Code != http.StatusForbidden {
		t.Fatalf("generic campaign creation accepted Lead-bound Action: %d %s", rec.Code, rec.Body.String())
	}
	rec = serveRun(t, h, "POST", "/api/v1/campaigns/"+campaign.ID+"/start", `{}`, testToken)
	if rec.Code != http.StatusForbidden {
		t.Fatalf("unlinked Lead campaign started: %d %s", rec.Code, rec.Body.String())
	}
	stored, err := srv.db.GetCampaign(campaign.ID)
	if err != nil || stored.Status == domain.CampaignRunning {
		t.Fatalf("campaign advanced before Lead authorization: status=%s err=%v", stored.Status, err)
	}
	rec = serveRun(t, h, "POST", "/api/v1/runs", fmt.Sprintf(`{"specId":%q}`, spec.ID), testToken)
	if rec.Code != http.StatusForbidden {
		t.Fatalf("unlinked Lead spec dispatched directly: %d %s", rec.Code, rec.Body.String())
	}
	beforeRuns, err := srv.db.ListRuns(domain.RunStatus(""), 100)
	if err != nil {
		t.Fatal(err)
	}
	rec = serveRun(t, h, "POST", "/api/v1/runs", fmt.Sprintf(`{"actionId":%q,"projectId":%q,"workloadKind":"paseo-task","executionMode":"trusted-local"}`, action.ID, project.ID), testToken)
	if rec.Code != http.StatusForbidden {
		t.Fatalf("Lead-bound Action dispatched directly without specId: %d %s", rec.Code, rec.Body.String())
	}
	afterRuns, err := srv.db.ListRuns(domain.RunStatus(""), 100)
	if err != nil {
		t.Fatal(err)
	}
	if len(afterRuns) != len(beforeRuns) {
		t.Fatalf("direct Lead Run was persisted: before=%d after=%d", len(beforeRuns), len(afterRuns))
	}
}

func TestLeadPersistsBoundedMachineReadableSessionReconciliation(t *testing.T) {
	srv, grantID, _ := leadFixture(t)
	fake := &reconciliationFailureAssessor{}
	srv.WithLeadAssessor(fake)
	rec := submitLead(t, srv.Handler(), grantID, "lead-reconcile-failure")
	if rec.Code != http.StatusCreated {
		t.Fatalf("intake: %d %s", rec.Code, rec.Body.String())
	}
	var out struct {
		Request struct {
			Status        string `json:"status"`
			FailureReason string `json:"failureReason"`
		} `json:"request"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &out); err != nil {
		t.Fatal(err)
	}
	if fake.calls != 1 || out.Request.Status != "blocked" || len(out.Request.FailureReason) > 600 {
		t.Fatalf("assessment error was retried or not bounded: calls=%d status=%s reason=%s", fake.calls, out.Request.Status, out.Request.FailureReason)
	}
	const marker = "reconciliation="
	index := strings.Index(out.Request.FailureReason, marker)
	if index < 0 {
		t.Fatalf("failure reason lost reconciliation marker: %s", out.Request.FailureReason)
	}
	var evidence struct {
		Code                  string `json:"code"`
		WorkspaceArchive      string `json:"workspaceArchive"`
		WorkspaceCleanup      string `json:"workspaceCleanup"`
		SessionReconciliation struct {
			Status           string `json:"status"`
			SessionID        string `json:"sessionId"`
			ObservedProvider string `json:"observedProvider"`
			ObservedModel    string `json:"observedModel"`
			RouteStatus      string `json:"routeStatus"`
			Stop             string `json:"stop"`
			Archive          string `json:"archive"`
		} `json:"sessionReconciliation"`
	}
	if err := json.Unmarshal([]byte(out.Request.FailureReason[index+len(marker):]), &evidence); err != nil {
		t.Fatalf("persisted reconciliation is not parseable JSON: %v (%s)", err, out.Request.FailureReason)
	}
	if evidence.Code != "assessment_run_failed" || evidence.SessionReconciliation.SessionID != "session_test_1" ||
		evidence.SessionReconciliation.Status != "session_reconciled" || evidence.SessionReconciliation.RouteStatus != "verified" ||
		evidence.SessionReconciliation.ObservedProvider != "opencode" || evidence.WorkspaceArchive != "failed" ||
		evidence.WorkspaceCleanup != "failed" {
		t.Fatalf("persisted reconciliation identity/status incomplete: %+v", evidence)
	}
	if strings.Contains(out.Request.FailureReason, "secret") || strings.Contains(out.Request.FailureReason, "/private/path") {
		t.Fatalf("persisted failure leaked raw CLI/cleanup text: %s", out.Request.FailureReason)
	}
}

func TestBoundedAssessmentFailurePreservesCompactedCleanupStatus(t *testing.T) {
	err := errors.New(`lead assessment Paseo run failed; reconciliation={"runCause":"failed","sessionReconciliation":{"status":"evidence_compacted","routeStatus":"unknown","stop":"not_attempted","archive":"not_attempted"},"workspaceArchive":"unknown","workspaceCleanup":"failed"}`)
	reason := boundedAssessmentFailure(err)
	if len(reason) > 600 {
		t.Fatalf("compacted failure reason exceeds its bound: %d", len(reason))
	}
	const marker = "reconciliation="
	index := strings.Index(reason, marker)
	if index < 0 {
		t.Fatalf("compacted failure reason lost its marker: %s", reason)
	}
	var evidence struct {
		SessionReconciliation struct {
			Status string `json:"status"`
		} `json:"sessionReconciliation"`
		WorkspaceCleanup string `json:"workspaceCleanup"`
	}
	if err := json.Unmarshal([]byte(reason[index+len(marker):]), &evidence); err != nil {
		t.Fatalf("compacted failure reason is not valid JSON: %v", err)
	}
	if evidence.SessionReconciliation.Status != "evidence_compacted" || evidence.WorkspaceCleanup != "failed" {
		t.Fatalf("compacted failure reason lost its status: %+v", evidence)
	}
}

func TestLeadFailsClosedWithoutAssessorOrOnMalformedAssessment(t *testing.T) {
	for _, tc := range []struct {
		name     string
		assessor *fixtureAssessor
	}{
		{name: "missing assessor"},
		{name: "malformed assessment", assessor: &fixtureAssessor{malformed: true}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			srv, grantID, _ := leadFixture(t)
			if tc.assessor != nil {
				srv.WithLeadAssessor(tc.assessor)
			}
			rec := submitLead(t, srv.Handler(), grantID, "lead-fail-closed-"+strings.ReplaceAll(tc.name, " ", "-"))
			if rec.Code != http.StatusCreated {
				t.Fatalf("intake: %d %s", rec.Code, rec.Body.String())
			}
			var out struct {
				Request struct {
					ID, Status, ActionID, ActionSpecID, CampaignID string
					Assessment                                     json.RawMessage
					Plan                                           json.RawMessage
					PlanHash                                       string
				} `json:"request"`
			}
			if err := json.Unmarshal(rec.Body.Bytes(), &out); err != nil {
				t.Fatal(err)
			}
			if out.Request.Status != "blocked" || out.Request.ActionID == "" || out.Request.ActionSpecID != "" ||
				out.Request.CampaignID != "" || out.Request.PlanHash != "" || len(out.Request.Assessment) != 0 || len(out.Request.Plan) != 0 {
				t.Fatalf("failure fabricated executable planning state: %s", rec.Body.String())
			}
			if tc.assessor != nil && tc.assessor.calls != 1 {
				t.Fatalf("malformed assessor call count: %d", tc.assessor.calls)
			}
		})
	}
}

func TestLeadRevocationPreventsDispatch(t *testing.T) {
	srv, grantID, _ := leadFixture(t)
	srv.WithLeadAssessor(&fixtureAssessor{})
	h := srv.Handler()
	updates, unsubscribe := srv.hub.Subscribe()
	defer unsubscribe()
	rec := submitLead(t, h, grantID, "lead-test-revoke")
	if rec.Code != http.StatusCreated {
		t.Fatalf("submit: %d %s", rec.Code, rec.Body.String())
	}
	var out struct {
		Request struct{ ID, CampaignID string } `json:"request"`
	}
	_ = json.Unmarshal(rec.Body.Bytes(), &out)
	rec = serveRun(t, h, "POST", "/api/v1/delegations/"+grantID+"/revoke", `{}`, testToken)
	if rec.Code != http.StatusOK {
		t.Fatalf("revoke: %d %s", rec.Code, rec.Body.String())
	}
	revokeEvents := drainHubEvents(updates)
	revokedEvent := false
	for _, event := range revokeEvents {
		if event.EventType == domain.EventDelegationRevoked {
			revokedEvent = true
		}
	}
	if !revokedEvent {
		t.Fatalf("delegation revocation was not published to live updates: %#v", revokeEvents)
	}
	rec = serveRun(t, h, "POST", "/api/v1/delegations/"+grantID+"/revoke", `{}`, testToken)
	if rec.Code != http.StatusOK || rec.Header().Get("Idempotent-Replayed") != "true" {
		t.Fatalf("repeated revoke was not idempotent: %d %s", rec.Code, rec.Body.String())
	}
	if repeatEvents := drainHubEvents(updates); len(repeatEvents) != 0 {
		t.Fatalf("repeated revoke published duplicate live events: %#v", repeatEvents)
	}
	rec = serveRun(t, h, "POST", "/api/v1/lead/requests/"+out.Request.ID+"/start", `{}`, testToken)
	if rec.Code != http.StatusForbidden {
		t.Fatalf("revoked request started: %d %s", rec.Code, rec.Body.String())
	}
	rec = serveRun(t, h, "POST", "/api/v1/campaigns/"+out.Request.CampaignID+"/start", `{}`, testToken)
	if rec.Code != http.StatusForbidden {
		t.Fatalf("revoked Campaign started directly: %d %s", rec.Code, rec.Body.String())
	}
}
