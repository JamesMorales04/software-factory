package store

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

func createLeadFixture(t *testing.T, db *DB) (domain.Project, domain.Action, domain.StandingDelegation) {
	t.Helper()
	ctx := context.Background()
	project := testProject(t, "alpha", domain.ProfileNodeBasic,
		[]string{"repository-edit", "node-toolchain"}, nil)
	created, _, err := db.CreateProject(ctx, project, "test")
	if err != nil {
		t.Fatal(err)
	}
	if err := created.Validate(); err != nil {
		t.Fatalf("stored fixture project invalid: %+v: %v", created, err)
	}
	params, err := domain.NewActionParams{ProjectID: created.ID, Title: "Add filter"}.Validate()
	if err != nil {
		t.Fatal(err)
	}
	action, _, err := db.CreateAction(ctx, params, "test")
	if err != nil {
		t.Fatal(err)
	}
	grant := domain.StandingDelegation{
		ID: strings.Repeat("d", 32), PrincipalID: "owner", AuditID: "approval-1",
		ProjectID: created.ID, Revision: 1,
		ObjectiveClasses: []domain.LeadObjectiveClass{domain.LeadFeature},
		AllowedPaths:     []string{"src"},
		Capabilities:     []domain.Capability{domain.CapRepositoryEdit, domain.CapNodeToolchain},
		RiskClasses:      []domain.RiskClass{domain.RiskR0},
		Provider:         "opencode", Model: "opencode-go/deepseek-v4.1-flash", PaseoMode: "build",
		MaxAttempts: 1, MaxTimeoutSecs: 120, MaxRepairCycles: 1,
		MaxWallClockSecs: 1800, MaxParallel: 1, MaxSessions: 2,
		ExpiresAt: db.now().Add(time.Hour),
	}
	storedGrant, _, err := db.CreateStandingDelegation(ctx, grant, "grant-key-1")
	if err != nil {
		t.Fatal(err)
	}
	return created, action, storedGrant
}

func TestLeadRequestReservationIsIdempotentAndClaimedOnce(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	project, action, grant := createLeadFixture(t, db)
	input := NewLeadRequest{
		ProjectID: project.ID, Objective: "Add filtering", RequestJSON: `{"objective":"Add filtering"}`,
		IdempotencyKey: "lead-request-1", DelegationID: grant.ID, ProjectConfigHash: project.ConfigHash,
	}
	created, replayed, err := db.CreateLeadRequest(ctx, input)
	if err != nil || replayed || created.Status != "submitted" {
		t.Fatalf("create reservation: replayed=%v request=%+v err=%v", replayed, created, err)
	}
	stored, err := db.GetLeadRequest(ctx, created.ID)
	if err != nil || stored.RequestHash != created.RequestHash || stored.RequestJSON != created.RequestJSON {
		t.Fatalf("reservation did not round trip: %+v err=%v", stored, err)
	}
	managed, err := db.LeadActionManaged(ctx, action.ID)
	if err != nil || managed {
		t.Fatalf("unbound Action must not yet be Lead-managed: managed=%v err=%v", managed, err)
	}
	replay, replayed, err := db.CreateLeadRequest(ctx, input)
	if err != nil || !replayed || replay.ID != created.ID {
		t.Fatalf("identical create should replay: replayed=%v request=%+v err=%v", replayed, replay, err)
	}
	created, linked, err := db.SetLeadActionID(ctx, created.ID, action.ID)
	if err != nil || !linked || created.ActionID != action.ID {
		t.Fatalf("bind Action: linked=%v request=%+v err=%v", linked, created, err)
	}
	managed, err = db.LeadActionManaged(ctx, action.ID)
	if err != nil || !managed {
		t.Fatalf("Action must be Lead-managed before spec linkage: managed=%v err=%v", managed, err)
	}
	input.Objective = "Different objective"
	if _, _, err := db.CreateLeadRequest(ctx, input); !errors.Is(err, domain.ErrConflict) {
		t.Fatalf("different request with same key should conflict: %v", err)
	}
	claimed, didClaim, err := db.ClaimLeadRequest(ctx, created.ID)
	if err != nil || !didClaim || claimed.Status != "assessing" {
		t.Fatalf("claim reservation: claimed=%v request=%+v err=%v", didClaim, claimed, err)
	}
	claimed, didClaim, err = db.ClaimLeadRequest(ctx, created.ID)
	if err != nil || didClaim || claimed.Status != "assessing" {
		t.Fatalf("duplicate assessment claim must not succeed: claimed=%v request=%+v err=%v", didClaim, claimed, err)
	}
}

func TestCreateLeadActionAtomicallyBindsReservation(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	project, _, grant := createLeadFixture(t, db)
	request, replayed, err := db.CreateLeadRequest(ctx, NewLeadRequest{
		ProjectID: project.ID, Objective: "Bound Lead Action", RequestJSON: `{"objective":"Bound Lead Action"}`,
		IdempotencyKey: "lead-action-reservation", DelegationID: grant.ID, ProjectConfigHash: project.ConfigHash,
	})
	if err != nil || replayed {
		t.Fatalf("reserve Lead request: replayed=%v err=%v", replayed, err)
	}
	params := domain.NewActionParams{ProjectID: project.ID, Title: "Bound Lead Action",
		Description: "Bound Lead Action", IdempotencyKey: "lead-action-" + request.ID}
	action, replayed, err := db.CreateLeadAction(ctx, request.ID, params, "lead-correlation")
	if err != nil || replayed {
		t.Fatalf("create and bind Lead Action: replayed=%v action=%+v err=%v", replayed, action, err)
	}
	managed, err := db.LeadActionManaged(ctx, action.ID)
	if err != nil || !managed {
		t.Fatalf("Action must be managed as soon as returned: managed=%v err=%v", managed, err)
	}
	storedRequest, err := db.GetLeadRequest(ctx, request.ID)
	if err != nil || storedRequest.ActionID != action.ID {
		t.Fatalf("Lead request binding not durable: request=%+v err=%v", storedRequest, err)
	}
	events, err := db.ListAggregateEvents(domain.AggregateAction, action.ID)
	if err != nil || len(events) != 1 || events[0].EventType != domain.EventActionCreated {
		t.Fatalf("ActionCreated event not atomically recorded: events=%+v err=%v", events, err)
	}
	again, replayed, err := db.CreateLeadAction(ctx, request.ID, params, "lead-correlation")
	if err != nil || !replayed || again.ID != action.ID {
		t.Fatalf("Lead Action replay changed identity: replayed=%v action=%+v err=%v", replayed, again, err)
	}
	events, err = db.ListAggregateEvents(domain.AggregateAction, action.ID)
	if err != nil || len(events) != 1 {
		t.Fatalf("idempotent replay appended another Action event: events=%+v err=%v", events, err)
	}
}

func TestLeadRequestRejectsRevokedOrStaleDelegation(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	project, _, grant := createLeadFixture(t, db)
	first, replayed, err := db.CreateLeadRequest(ctx, NewLeadRequest{
		ProjectID: project.ID, Objective: "First bounded campaign", RequestJSON: `{"objective":"First bounded campaign"}`,
		IdempotencyKey: "first-grant-request", DelegationID: grant.ID, ProjectConfigHash: project.ConfigHash,
	})
	if err != nil || replayed {
		t.Fatalf("first request should consume grant: replayed=%v err=%v", replayed, err)
	}
	if replay, replayed, err := db.CreateLeadRequest(ctx, NewLeadRequest{
		ProjectID: project.ID, Objective: "First bounded campaign", RequestJSON: `{"objective":"First bounded campaign"}`,
		IdempotencyKey: "first-grant-request", DelegationID: grant.ID, ProjectConfigHash: project.ConfigHash,
	}); err != nil || !replayed || replay.ID != first.ID {
		t.Fatalf("same request should replay on consumed grant: replayed=%v request=%+v err=%v", replayed, replay, err)
	}
	if _, _, err := db.CreateLeadRequest(ctx, NewLeadRequest{
		ProjectID: project.ID, Objective: "Second campaign", RequestJSON: `{"objective":"Second campaign"}`,
		IdempotencyKey: "second-grant-request", DelegationID: grant.ID, ProjectConfigHash: project.ConfigHash,
	}); !errors.Is(err, domain.ErrConflict) {
		t.Fatalf("grant must not authorize a second request: %v", err)
	}
	if _, changed, err := db.RevokeStandingDelegation(ctx, grant.ID, db.now()); err != nil || !changed {
		t.Fatalf("revoke delegation: changed=%v err=%v", changed, err)
	}
	if _, _, err := db.CreateLeadRequest(ctx, NewLeadRequest{
		ProjectID: project.ID, Objective: "Add filtering", RequestJSON: `{"objective":"Add filtering"}`,
		IdempotencyKey: "revoked-grant-request", DelegationID: grant.ID, ProjectConfigHash: project.ConfigHash,
	}); !errors.Is(err, domain.ErrUnauthorized) {
		t.Fatalf("request with revoked delegation must be rejected: %v", err)
	}
	if _, _, err := db.CreateLeadRequest(ctx, NewLeadRequest{
		ProjectID: project.ID, Objective: "Add filtering", RequestJSON: `{"objective":"Add filtering"}`,
		IdempotencyKey: "forged-grant-request", DelegationID: strings.Repeat("e", 32), ProjectConfigHash: project.ConfigHash,
	}); !errors.Is(err, domain.ErrUnauthorized) {
		t.Fatalf("request with forged delegation identity must be rejected: %v", err)
	}
	delegationEvents, err := db.ListAggregateEvents(domain.AggregateDelegation, grant.ID)
	if err != nil || len(delegationEvents) != 2 ||
		delegationEvents[0].EventType != domain.EventDelegationIssued ||
		delegationEvents[1].EventType != domain.EventDelegationRevoked {
		t.Fatalf("delegation issue/revoke history: %+v err=%v", delegationEvents, err)
	}
}

func TestBlockedLeadRequestEventOmitsFailureBody(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	project, _, grant := createLeadFixture(t, db)
	request, _, err := db.CreateLeadRequest(ctx, NewLeadRequest{
		ProjectID: project.ID, Objective: "Sensitive objective", RequestJSON: `{"objective":"Sensitive objective"}`,
		IdempotencyKey: "blocked-lead-request", DelegationID: grant.ID, ProjectConfigHash: project.ConfigHash,
	})
	if err != nil {
		t.Fatal(err)
	}
	if _, changed, err := db.BlockLeadRequest(ctx, request.ID, "submitted", "sensitive raw failure text"); err != nil || !changed {
		t.Fatalf("block lead request: changed=%v err=%v", changed, err)
	}
	events, err := db.ListAggregateEvents(domain.AggregateLeadRequest, request.ID)
	if err != nil || len(events) != 2 || events[1].EventType != domain.EventLeadRequestBlocked {
		t.Fatalf("blocked request events: %+v err=%v", events, err)
	}
	if _, ok := events[0].Payload["objective"]; ok {
		t.Fatal("request event persisted the objective body")
	}
	if _, ok := events[1].Payload["failureReason"]; ok {
		t.Fatal("blocked event persisted failure text")
	}
}

func TestLeadAssessmentProposalAndPlanPersistIdentity(t *testing.T) {
	path := filepath.Join(t.TempDir(), "lead.db")
	db, err := Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = db.Close() }()
	ctx := context.Background()
	project, action, grant := createLeadFixture(t, db)
	request, _, err := db.CreateLeadRequest(ctx, NewLeadRequest{
		ProjectID: project.ID, Objective: "Add filtering", RequestJSON: `{"objective":"Add filtering"}`,
		IdempotencyKey: "lead-request-2", DelegationID: grant.ID, ProjectConfigHash: project.ConfigHash,
	})
	if err != nil {
		t.Fatal(err)
	}
	request, linked, err := db.SetLeadActionID(ctx, request.ID, action.ID)
	if err != nil || !linked {
		t.Fatalf("bind Action: linked=%v err=%v", linked, err)
	}
	if _, claimed, err := db.ClaimLeadRequest(ctx, request.ID); err != nil || !claimed {
		t.Fatalf("claim: %v %v", claimed, err)
	}
	assessment, replayed, err := db.CreateLeadAssessment(ctx, request.ID, "HYBRID",
		`{"class":"feature","confidence":0.8}`, `{"provider":"opencode","model":"opencode-go/deepseek-v4.1-flash","mode":"build"}`, "assessment-key")
	if err != nil || replayed {
		t.Fatalf("assessment create: replayed=%v err=%v", replayed, err)
	}
	assessmentReplay, replayed, err := db.CreateLeadAssessment(ctx, request.ID, "HYBRID",
		`{"confidence":0.8,"class":"feature"}`, `{"mode":"build","model":"opencode-go/deepseek-v4.1-flash","provider":"opencode"}`, "assessment-key")
	if err != nil || !replayed || assessmentReplay.ID != assessment.ID {
		t.Fatalf("assessment replay: replayed=%v record=%+v err=%v", replayed, assessmentReplay, err)
	}
	proposal := domain.LeadPlanProposal{
		ProjectID: project.ID, ActionID: action.ID, BaseRevision: strings.Repeat("b", 40),
		ProjectConfigHash: project.ConfigHash, Objective: "Add filtering", Class: domain.LeadFeature,
		Assumptions: []string{}, Risks: []string{},
		AcceptanceCriteria: []domain.SpecCheck{
			{ID: "format", Kind: domain.CheckFormat, Description: "format", Command: []string{"node", "--check", "src/filter.js"}},
			{ID: "test", Kind: domain.CheckTest, Description: "tests", Command: []string{"node", "--test"}},
			{ID: "functional", Kind: domain.CheckFunctional, Description: "filter", Command: []string{"node", "-e", "0"}},
		},
		NegativeCases:  []domain.SpecCheck{{ID: "negative", Kind: domain.CheckNegative, Description: "invalid input", Command: []string{"node", "-e", "1"}, ExpectExit: 1}},
		QualityProfile: domain.ProfileNodeBasic, RiskClass: domain.RiskR0,
		Budget:   domain.SpecBudget{MaxAttempts: 1, TimeoutSecs: 120},
		Provider: grant.Provider, Model: grant.Model, PaseoMode: grant.PaseoMode,
		MaxRepairCycles: 0, MaxWallClockSecs: 1800, MaxParallel: 1, MaxSessions: 2,
		WorkUnits: []domain.LeadWorkUnit{{ID: "implement", Kind: string(domain.LeadImplementationUnit),
			DependsOn: []string{}, AllowedPaths: []string{"src"},
			Capabilities: []domain.Capability{domain.CapRepositoryEdit, domain.CapNodeToolchain}}},
	}
	proposalRecord, replayed, err := db.CreateLeadProposal(ctx, request.ID, assessment.ID, proposal, "proposal-key")
	if err != nil || replayed {
		t.Fatalf("proposal create: replayed=%v err=%v", replayed, err)
	}
	if _, err := db.GetLeadProposal(ctx, proposalRecord.ID); err != nil {
		t.Fatalf("proposal does not round trip: %v", err)
	}
	frozen, replayed, err := db.FreezeLeadPlan(ctx, request.ID, proposalRecord.ID, "freeze-key")
	if err != nil || replayed || frozen.PlanHash == "" || frozen.ProposalHash != proposalRecord.ContentHash {
		t.Fatalf("freeze: replayed=%v record=%+v err=%v", replayed, frozen, err)
	}
	frozenReplay, replayed, err := db.FreezeLeadPlan(ctx, request.ID, proposalRecord.ID, "freeze-key")
	if err != nil || !replayed || frozenReplay.ID != frozen.ID {
		t.Fatalf("freeze replay: replayed=%v record=%+v err=%v", replayed, frozenReplay, err)
	}
	storedRequest, err := db.GetLeadRequest(ctx, request.ID)
	if err != nil || storedRequest.Status != "frozen" || storedRequest.AssessmentID != assessment.ID ||
		storedRequest.ProposalID != proposalRecord.ID || storedRequest.FrozenPlanID != frozen.ID {
		t.Fatalf("frozen projection links missing: %+v err=%v", storedRequest, err)
	}
	if err := db.Close(); err != nil {
		t.Fatal(err)
	}
	reopened, err := Open(path)
	if err != nil {
		t.Fatalf("reopen lead store: %v", err)
	}
	defer func() { _ = reopened.Close() }()
	storedRequest, err = reopened.GetLeadRequest(ctx, request.ID)
	if err != nil || storedRequest.Status != "frozen" || storedRequest.AssessmentID != assessment.ID ||
		storedRequest.ProposalID != proposalRecord.ID || storedRequest.FrozenPlanID != frozen.ID {
		t.Fatalf("lead request identity did not survive restart: %+v err=%v", storedRequest, err)
	}
	if _, err := reopened.GetLeadAssessment(ctx, assessment.ID); err != nil {
		t.Fatalf("assessment did not survive restart: %v", err)
	}
	if _, err := reopened.GetLeadProposal(ctx, proposalRecord.ID); err != nil {
		t.Fatalf("proposal did not survive restart: %v", err)
	}
	if _, err := reopened.GetFrozenLeadPlan(ctx, frozen.ID); err != nil {
		t.Fatalf("frozen plan did not survive restart: %v", err)
	}
	events, err := reopened.ListAggregateEvents(domain.AggregateLeadRequest, request.ID)
	wantTypes := []string{domain.EventLeadRequestCreated, domain.EventLeadRequestStarted,
		domain.EventLeadAssessmentRecorded, domain.EventLeadPlanFrozen}
	if err != nil || len(events) != len(wantTypes) {
		t.Fatalf("lead event history did not survive restart: %d events err=%v", len(events), err)
	}
	for i, want := range wantTypes {
		if events[i].EventType != want || events[i].Sequence != int64(i+1) {
			t.Fatalf("lead event history[%d] = %s/%d, want %s/%d", i, events[i].EventType, events[i].Sequence, want, i+1)
		}
	}
	for _, event := range events {
		if _, ok := event.Payload["objective"]; ok {
			t.Fatalf("event %s persisted an objective body", event.EventType)
		}
		if _, ok := event.Payload["assessment"]; ok {
			t.Fatalf("event %s persisted assessment body", event.EventType)
		}
	}
	delegationEvents, err := reopened.ListAggregateEvents(domain.AggregateDelegation, grant.ID)
	if err != nil || len(delegationEvents) != 1 || delegationEvents[0].EventType != domain.EventDelegationIssued {
		t.Fatalf("delegation issuance history did not survive restart: %+v err=%v", delegationEvents, err)
	}
}
