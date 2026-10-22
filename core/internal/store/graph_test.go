package store

import (
	"context"
	"fmt"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

func graphSpec(t *testing.T, db *DB, action domain.Action, goal string, paths []string) domain.ActionSpec {
	t.Helper()
	params := domain.NewActionSpecParams{
		ProjectID: "default", ActionID: action.ID, Goal: goal,
		Repository: t.TempDir(), BaseRevision: strings.Repeat("b", 40), AllowedPaths: paths,
		AcceptanceCriteria: []domain.SpecCheck{
			{ID: "format", Kind: domain.CheckFormat, Description: "format", Command: []string{"node", "--check", "add.js"}},
			{ID: "test", Kind: domain.CheckTest, Description: "tests", Command: []string{"node", "--test"}},
			{ID: "functional", Kind: domain.CheckFunctional, Description: "functional", Command: []string{"node", "-e", "0"}},
		},
		NegativeCases: []domain.SpecCheck{
			{ID: "negative", Kind: domain.CheckNegative, Description: "negative", Command: []string{"node", "-e", "1"}, ExpectExit: 1},
		},
		RiskClass: "R0", QualityProfile: "node-basic",
		Budget:        domain.SpecBudget{MaxAttempts: 1, TimeoutSecs: 120},
		ExecutionMode: "trusted-local", Provider: "opencode",
		Model: "opencode-go/deepseek-v4.1-flash", PaseoMode: "build",
	}
	spec, err := params.Validate()
	if err != nil {
		t.Fatal(err)
	}
	spec, _, _, err = db.CreateActionSpec(context.Background(), spec, "graph")
	if err != nil {
		t.Fatal(err)
	}
	return spec
}

type graphFixture struct {
	campaign    domain.Campaign
	unitSpecs   map[string]domain.ActionSpec
	integration domain.ActionSpec
}

func createGraphCampaign(t *testing.T, db *DB, units []domain.LeadWorkUnit, repairs, parallel int) graphFixture {
	t.Helper()
	action, _, err := db.CreateAction(context.Background(), mustActionParams(t, "graph campaign"), "graph")
	if err != nil {
		t.Fatal(err)
	}
	unitSpecs := map[string]domain.ActionSpec{}
	specIDs := map[string]string{}
	for _, unit := range units {
		spec := graphSpec(t, db, action, unit.Goal("graph campaign"), unit.AllowedPaths)
		unitSpecs[unit.ID] = spec
		specIDs[unit.ID] = spec.ID
	}
	paths := []string{}
	capabilities := []domain.Capability{domain.CapRepositoryEdit, domain.CapNodeToolchain}
	for _, unit := range units {
		paths = append(paths, unit.AllowedPaths...)
	}
	integration := graphSpec(t, db, action, "integrate", paths)
	_ = capabilities
	proposal := domain.LeadPlanProposal{
		MaxRepairCycles: repairs, MaxWallClockSecs: 600, MaxParallel: parallel,
		Budget:    domain.SpecBudget{MaxAttempts: 1, TimeoutSecs: 120},
		WorkUnits: units,
	}
	plan, err := domain.BuildLeadCampaignPlan(proposal, specIDs, integration.ID)
	if err != nil {
		t.Fatal(err)
	}
	campaign, _, _, err := db.CreateCampaign(context.Background(), domain.NewCampaignParams{
		SpecID: integration.ID, MaxRepairCycles: repairs, MaxWallClockSecs: 600,
		MaxParallel: parallel, MaxImplementerAttempts: 8, Plan: &plan,
	}, "graph")
	if err != nil {
		t.Fatal(err)
	}
	return graphFixture{campaign: campaign, unitSpecs: unitSpecs, integration: integration}
}

func mustActionParams(t *testing.T, title string) domain.NewActionParams {
	t.Helper()
	params, err := domain.NewActionParams{Title: title}.Validate()
	if err != nil {
		t.Fatal(err)
	}
	return params
}

func scopeUnits(ids ...string) []domain.LeadWorkUnit {
	units := make([]domain.LeadWorkUnit, 0, len(ids))
	for _, id := range ids {
		units = append(units, domain.LeadWorkUnit{
			ID: id, Kind: string(domain.LeadImplementationUnit), AllowedPaths: []string{"src/" + id},
			Capabilities: []domain.Capability{domain.CapRepositoryEdit, domain.CapNodeToolchain},
		})
	}
	return units
}

func driveGraphUnit(t *testing.T, db *DB, fixture graphFixture, task domain.CampaignTask, seq int) (domain.CandidateRevision, domain.Run) {
	t.Helper()
	ctx := context.Background()
	_, run, _, err := db.DispatchImplementer(ctx, fixture.campaign.ID, task.ID, "unit "+task.Key, "graph")
	if err != nil {
		t.Fatal(err)
	}
	registerWorker(t, db, "graph-worker")
	claimed, attempt, found, _, err := db.ClaimNextRun(ctx, "graph-worker", "graph")
	if err != nil || !found || claimed.ID != run.ID {
		t.Fatalf("claim graph run: found=%v err=%v", found, err)
	}
	if _, _, err := db.HeartbeatLease(ctx, "graph-worker", claimed.ID, attempt.ID, attempt.LeaseGeneration, "graph"); err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := db.IntendPaseoSession(ctx, "graph-worker", claimed.ID, attempt.ID, attempt.LeaseGeneration, "graph"); err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "graph-worker", claimed.ID, attempt.ID, attempt.LeaseGeneration, "ws-"+task.Key, "sess-"+task.Key, "graph"); err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := db.ClosePaseoSession(ctx, "graph-worker", claimed.ID, attempt.ID, attempt.LeaseGeneration, "succeeded", "", "graph"); err != nil {
		t.Fatal(err)
	}
	node, _ := fixture.campaign.Plan.TaskPlan(task.Key)
	spec, err := db.GetActionSpec(node.SpecID)
	if err != nil {
		t.Fatal(err)
	}
	base, _ := claimed.WorkloadInput["baseRevision"].(string)
	parent, _ := claimed.WorkloadInput["parentCandidateId"].(string)
	candidate, _, err := db.CreateCandidate(ctx, domain.CandidateRevision{
		ProjectID: spec.ProjectID, ActionID: spec.ActionID, SpecID: spec.ID,
		SpecHash: spec.ContentHash, PolicyRevision: spec.PolicyRevision,
		BaseRevision: base, CommitSHA: fmt.Sprintf("%040x", seq),
		TreeSHA: fmt.Sprintf("%040x", seq+100), ContentDigest: fmt.Sprintf("%064x", seq),
		CandidateRef:      fmt.Sprintf("refs/factory/candidates/graph-%d", seq),
		ParentCandidateID: parent, Repository: spec.Repository,
		ImplementerRunID: claimed.ID, ImplementerAttemptID: attempt.ID,
		ImplementerGeneration: attempt.LeaseGeneration, ImplementerWorkerID: "graph-worker",
		ExecutionMode: claimed.ExecutionMode, Provider: claimed.Provider, Model: claimed.Model,
		PaseoMode: claimed.PaseoMode, ChangedPaths: spec.AllowedPaths, RiskClass: domain.RiskR0,
	}, "graph")
	if err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := db.ReportAttempt(ctx, "graph-worker", claimed.ID, attempt.ID,
		attempt.LeaseGeneration, domain.OutcomeCompleted, map[string]any{}, "", "graph"); err != nil {
		t.Fatal(err)
	}
	tasks, err := db.CampaignTasks(fixture.campaign.ID)
	if err != nil {
		t.Fatal(err)
	}
	for _, current := range tasks {
		if current.ID != task.ID {
			continue
		}
		if _, _, _, err := db.CompleteCampaignTask(ctx, fixture.campaign.ID, task.ID,
			"", current.DispatchToken, "graph"); err != nil {
			t.Fatal(err)
		}
		return candidate, claimed
	}
	t.Fatalf("task %s disappeared", task.Key)
	return domain.CandidateRevision{}, domain.Run{}
}

func driveGraphRunOnly(t *testing.T, db *DB, fixture graphFixture, task domain.CampaignTask, seq int) (domain.CandidateRevision, domain.Run, string) {
	t.Helper()
	ctx := context.Background()
	_, run, _, err := db.DispatchImplementer(ctx, fixture.campaign.ID, task.ID, "unit "+task.Key, "graph")
	if err != nil {
		t.Fatal(err)
	}
	registerWorker(t, db, "graph-worker")
	claimed, attempt, found, _, err := db.ClaimNextRun(ctx, "graph-worker", "graph")
	if err != nil || !found || claimed.ID != run.ID {
		t.Fatalf("claim graph run: found=%v err=%v", found, err)
	}
	if _, _, err := db.HeartbeatLease(ctx, "graph-worker", claimed.ID, attempt.ID, attempt.LeaseGeneration, "graph"); err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := db.IntendPaseoSession(ctx, "graph-worker", claimed.ID, attempt.ID, attempt.LeaseGeneration, "graph"); err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := db.BindPaseoSession(ctx, "graph-worker", claimed.ID, attempt.ID, attempt.LeaseGeneration, "ws-"+task.Key, "sess-"+task.Key, "graph"); err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := db.ClosePaseoSession(ctx, "graph-worker", claimed.ID, attempt.ID, attempt.LeaseGeneration, "succeeded", "", "graph"); err != nil {
		t.Fatal(err)
	}
	node, _ := fixture.campaign.Plan.TaskPlan(task.Key)
	spec, err := db.GetActionSpec(node.SpecID)
	if err != nil {
		t.Fatal(err)
	}
	base, _ := claimed.WorkloadInput["baseRevision"].(string)
	parent, _ := claimed.WorkloadInput["parentCandidateId"].(string)
	candidate, _, err := db.CreateCandidate(ctx, domain.CandidateRevision{
		ProjectID: spec.ProjectID, ActionID: spec.ActionID, SpecID: spec.ID,
		SpecHash: spec.ContentHash, PolicyRevision: spec.PolicyRevision,
		BaseRevision: base, CommitSHA: fmt.Sprintf("%040x", seq),
		TreeSHA: fmt.Sprintf("%040x", seq+100), ContentDigest: fmt.Sprintf("%064x", seq),
		CandidateRef:      fmt.Sprintf("refs/factory/candidates/graph-%d", seq),
		ParentCandidateID: parent, Repository: spec.Repository,
		ImplementerRunID: claimed.ID, ImplementerAttemptID: attempt.ID,
		ImplementerGeneration: attempt.LeaseGeneration, ImplementerWorkerID: "graph-worker",
		ExecutionMode: claimed.ExecutionMode, Provider: claimed.Provider, Model: claimed.Model,
		PaseoMode: claimed.PaseoMode, ChangedPaths: spec.AllowedPaths, RiskClass: domain.RiskR0,
	}, "graph")
	if err != nil {
		t.Fatal(err)
	}
	if _, _, _, err := db.ReportAttempt(ctx, "graph-worker", claimed.ID, attempt.ID,
		attempt.LeaseGeneration, domain.OutcomeCompleted, map[string]any{}, "", "graph"); err != nil {
		t.Fatal(err)
	}
	current := taskByKey(t, db, fixture.campaign.ID, task.Key)
	return candidate, claimed, current.DispatchToken
}

func TestWorkGraphParallelUnitCompletionDoesNotInvalidateSiblingCandidate(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	fixture := createGraphCampaign(t, db, scopeUnits("api", "ui"), 0, 2)
	if _, _, _, err := db.StartCampaign(ctx, fixture.campaign.ID, "graph"); err != nil {
		t.Fatal(err)
	}
	apiTask := taskByKey(t, db, fixture.campaign.ID, domain.LeadUnitTaskKey("api"))
	uiTask := taskByKey(t, db, fixture.campaign.ID, domain.LeadUnitTaskKey("ui"))
	apiCandidate, _, apiToken := driveGraphRunOnly(t, db, fixture, apiTask, 1)
	uiCandidate, _, uiToken := driveGraphRunOnly(t, db, fixture, uiTask, 2)
	apiStored, err := db.GetCandidate(apiCandidate.ID)
	if err != nil {
		t.Fatal(err)
	}
	if apiStored.State != domain.CandidateFrozen {
		t.Fatalf("parallel sibling success invalidated api candidate: %s", apiStored.State)
	}
	if _, _, _, err := db.CompleteCampaignTask(ctx, fixture.campaign.ID, apiTask.ID, "", apiToken, "graph"); err != nil {
		t.Fatalf("complete api unit after sibling success: %v", err)
	}
	if _, _, _, err := db.CompleteCampaignTask(ctx, fixture.campaign.ID, uiTask.ID, "", uiToken, "graph"); err != nil {
		t.Fatalf("complete ui unit: %v", err)
	}
	if _, err := db.PromoteReadyTasks(ctx, fixture.campaign.ID, "graph"); err != nil {
		t.Fatal(err)
	}
	integration := taskByKey(t, db, fixture.campaign.ID, domain.IntegrationTaskKey)
	if integration.Status != domain.TaskReady {
		t.Fatalf("integration status = %s", integration.Status)
	}
	if uiCandidate.SpecID == apiCandidate.SpecID {
		t.Fatal("parallel units shared a spec unexpectedly")
	}
}

func TestWorkGraphSequentialBaseChaining(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	units := scopeUnits("api", "ui")
	units[1].DependsOn = []string{"api"}
	fixture := createGraphCampaign(t, db, units, 0, 2)
	if _, _, _, err := db.StartCampaign(ctx, fixture.campaign.ID, "graph"); err != nil {
		t.Fatal(err)
	}
	tasks, _ := db.CampaignTasks(fixture.campaign.ID)
	byKey := map[string]domain.CampaignTask{}
	for _, task := range tasks {
		byKey[task.Key] = task
	}
	apiTask := byKey[domain.LeadUnitTaskKey("api")]
	uiTask := byKey[domain.LeadUnitTaskKey("ui")]
	apiCandidate, _ := driveGraphUnit(t, db, fixture, apiTask, 1)
	if _, err := db.PromoteReadyTasks(ctx, fixture.campaign.ID, "graph"); err != nil {
		t.Fatal(err)
	}
	uiTask = taskByKey(t, db, fixture.campaign.ID, uiTask.Key)
	if uiTask.Status != domain.TaskReady {
		t.Fatalf("dependent unit status = %s", uiTask.Status)
	}
	uiCandidate, uiRun := driveGraphUnit(t, db, fixture, uiTask, 2)
	if base, _ := uiRun.WorkloadInput["baseRevision"].(string); base != apiCandidate.CommitSHA {
		t.Fatalf("dependent unit base = %s, want %s", base, apiCandidate.CommitSHA)
	}
	if parent, _ := uiRun.WorkloadInput["parentCandidateId"].(string); parent != apiCandidate.ID {
		t.Fatalf("dependent parent = %s, want %s", parent, apiCandidate.ID)
	}
	if uiCandidate.BaseRevision != apiCandidate.CommitSHA {
		t.Fatal("candidate lineage lost the dependency base")
	}
}

func TestWorkGraphParallelFanInIntegration(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	fixture := createGraphCampaign(t, db, scopeUnits("api", "ui"), 0, 2)
	if _, _, _, err := db.StartCampaign(ctx, fixture.campaign.ID, "graph"); err != nil {
		t.Fatal(err)
	}
	tasks, _ := db.CampaignTasks(fixture.campaign.ID)
	byKey := map[string]domain.CampaignTask{}
	for _, task := range tasks {
		byKey[task.Key] = task
	}
	apiCandidate, _ := driveGraphUnit(t, db, fixture, byKey[domain.LeadUnitTaskKey("api")], 1)
	uiCandidate, _ := driveGraphUnit(t, db, fixture, byKey[domain.LeadUnitTaskKey("ui")], 2)
	if _, err := db.PromoteReadyTasks(ctx, fixture.campaign.ID, "graph"); err != nil {
		t.Fatal(err)
	}
	integration := taskByKey(t, db, fixture.campaign.ID, domain.IntegrationTaskKey)
	if integration.Status != domain.TaskReady {
		t.Fatalf("integration status = %s", integration.Status)
	}
	candidate, run := driveGraphUnit(t, db, fixture, integration, 3)
	merges, _ := run.WorkloadInput["mergeCandidates"].([]any)
	if len(merges) != 2 {
		t.Fatalf("integration merge candidates = %#v", run.WorkloadInput["mergeCandidates"])
	}
	got := map[string]bool{}
	for _, value := range merges {
		got[fmt.Sprint(value)] = true
	}
	if !got[apiCandidate.CommitSHA] || !got[uiCandidate.CommitSHA] {
		t.Fatalf("integration did not consume both unit commits: %#v", merges)
	}
	node, _ := fixture.campaign.Plan.TaskPlan(domain.IntegrationTaskKey)
	if node.SourceKey != "" || node.Role != domain.RoleImplementer || node.SpecID != fixture.integration.ID {
		t.Fatalf("integration node = %+v", node)
	}
	if candidate.SpecID != fixture.integration.ID {
		t.Fatalf("integration candidate spec = %s", candidate.SpecID)
	}
	if _, err := db.PromoteReadyTasks(ctx, fixture.campaign.ID, "graph"); err != nil {
		t.Fatal(err)
	}
	verifierTask := taskByKey(t, db, fixture.campaign.ID, "verifier-0")
	verifier, claimed, _, err := db.ClaimParticipantTask(ctx, fixture.campaign.ID, verifierTask.ID, "graph-orchestrator", 30*time.Second, "graph")
	if err != nil || !claimed {
		t.Fatalf("claim verifier: claimed=%v err=%v", claimed, err)
	}
	evidenceFor(t, db, fixture.integration, candidate, verifier.IdempotencyKey, verifier.ParticipantID, "")
	if _, _, _, err := db.CompleteCampaignTask(ctx, fixture.campaign.ID, verifier.ID, "PASS", verifier.DispatchToken, "graph"); err != nil {
		t.Fatal(err)
	}
	reviewerTask := taskByKey(t, db, fixture.campaign.ID, "reviewer-0")
	reviewer, claimed, _, err := db.ClaimParticipantTask(ctx, fixture.campaign.ID, reviewerTask.ID, "graph-orchestrator", 30*time.Second, "graph")
	if err != nil || !claimed {
		t.Fatalf("claim reviewer: claimed=%v err=%v", claimed, err)
	}
	reviewFor(t, db, fixture.integration, candidate, reviewer.IdempotencyKey, reviewer.ParticipantID, domain.ReviewApprove)
	if _, _, _, err := db.CompleteCampaignTask(ctx, fixture.campaign.ID, reviewer.ID, "APPROVE", reviewer.DispatchToken, "graph"); err != nil {
		t.Fatal(err)
	}
	if _, err := db.PromoteReadyTasks(ctx, fixture.campaign.ID, "graph"); err != nil {
		t.Fatal(err)
	}
	gateTask := taskByKey(t, db, fixture.campaign.ID, "gate-0")
	gate, claimed, _, err := db.ClaimGateTask(ctx, fixture.campaign.ID, gateTask.ID, "graph-orchestrator", 30*time.Second, "graph")
	if err != nil || !claimed {
		t.Fatalf("claim gate: claimed=%v err=%v", claimed, err)
	}
	decision, final, _, err := db.EvaluateCampaignGate(ctx, fixture.campaign.ID, gate.ID, "graph")
	if err != nil || !final || decision.Status != domain.GateAccepted {
		t.Fatalf("final gate: final=%v decision=%+v err=%v", final, decision, err)
	}
	campaign, err := db.GetCampaign(fixture.campaign.ID)
	if err != nil || campaign.Status != domain.CampaignAccepted {
		t.Fatalf("campaign accepted: status=%s err=%v", campaign.Status, err)
	}
}

func taskByKey(t *testing.T, db *DB, campaignID, key string) domain.CampaignTask {
	t.Helper()
	tasks, err := db.CampaignTasks(campaignID)
	if err != nil {
		t.Fatal(err)
	}
	for _, task := range tasks {
		if task.Key == key {
			return task
		}
	}
	t.Fatalf("task %s not found", key)
	return domain.CampaignTask{}
}

func TestWorkGraphDependencyFailureStopsDownstreamDispatch(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	units := scopeUnits("api", "ui")
	units[1].DependsOn = []string{"api"}
	fixture := createGraphCampaign(t, db, units, 0, 2)
	if _, _, _, err := db.StartCampaign(ctx, fixture.campaign.ID, "graph"); err != nil {
		t.Fatal(err)
	}
	apiTask := taskByKey(t, db, fixture.campaign.ID, domain.LeadUnitTaskKey("api"))
	_, _, _, err := db.DispatchImplementer(ctx, fixture.campaign.ID, apiTask.ID, "fail api", "graph")
	if err != nil {
		t.Fatal(err)
	}
	apiTask = taskByKey(t, db, fixture.campaign.ID, apiTask.Key)
	if _, _, err := db.FailImplementerTask(ctx, fixture.campaign.ID, apiTask.ID,
		"unit rejected its own output", apiTask.DispatchToken, "graph"); err != nil {
		t.Fatal(err)
	}
	if _, err := db.PromoteReadyTasks(ctx, fixture.campaign.ID, "graph"); err != nil {
		t.Fatal(err)
	}
	uiTask := taskByKey(t, db, fixture.campaign.ID, domain.LeadUnitTaskKey("ui"))
	if uiTask.Status != domain.TaskCancelled && uiTask.Status != domain.TaskPending {
		t.Fatalf("downstream unit advanced after dependency failure: %s", uiTask.Status)
	}
	integration := taskByKey(t, db, fixture.campaign.ID, domain.IntegrationTaskKey)
	if integration.Status != domain.TaskCancelled && integration.Status != domain.TaskPending {
		t.Fatalf("integration advanced after dependency failure: %s", integration.Status)
	}
	campaign, err := db.GetCampaign(fixture.campaign.ID)
	if err != nil || campaign.Status != domain.CampaignFailed {
		t.Fatalf("campaign status = %s err=%v", campaign.Status, err)
	}
}

func TestWorkGraphRestartPreservesDependencyAndDispatchIdempotency(t *testing.T) {
	path := filepath.Join(t.TempDir(), "graph-restart.db")
	db, err := Open(path)
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	units := scopeUnits("api", "ui")
	units[1].DependsOn = []string{"api"}
	fixture := createGraphCampaign(t, db, units, 0, 2)
	if _, _, _, err := db.StartCampaign(ctx, fixture.campaign.ID, "graph"); err != nil {
		t.Fatal(err)
	}
	apiTask := taskByKey(t, db, fixture.campaign.ID, domain.LeadUnitTaskKey("api"))
	apiCandidate, _ := driveGraphUnit(t, db, fixture, apiTask, 1)
	if _, err := db.PromoteReadyTasks(ctx, fixture.campaign.ID, "graph"); err != nil {
		t.Fatal(err)
	}
	if err := db.Close(); err != nil {
		t.Fatal(err)
	}
	reopened, err := Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = reopened.Close() }()
	uiTask := taskByKey(t, reopened, fixture.campaign.ID, domain.LeadUnitTaskKey("ui"))
	if uiTask.Status != domain.TaskReady {
		t.Fatalf("restart lost dependent readiness: %s", uiTask.Status)
	}
	_, firstRun, _, err := reopened.DispatchImplementer(ctx, fixture.campaign.ID, uiTask.ID, "unit ui", "graph")
	if err != nil {
		t.Fatal(err)
	}
	_, secondRun, _, err := reopened.DispatchImplementer(ctx, fixture.campaign.ID, uiTask.ID, "unit ui", "graph")
	if err != nil {
		t.Fatal(err)
	}
	if firstRun.ID != secondRun.ID {
		t.Fatalf("dispatch replay created a duplicate Run: %s vs %s", firstRun.ID, secondRun.ID)
	}
	if base, _ := secondRun.WorkloadInput["baseRevision"].(string); base != apiCandidate.CommitSHA {
		t.Fatalf("restart lost dependency base: %s", base)
	}
}
