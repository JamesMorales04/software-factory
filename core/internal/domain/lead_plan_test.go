package domain

import (
	"crypto/sha256"
	"encoding/hex"
	"strings"
	"testing"
	"time"
)

func validLeadInputs() (StandingDelegation, Project, Action, LeadPlanProposal, time.Time) {
	now := time.Date(2026, 10, 21, 12, 0, 0, 0, time.UTC)
	project := Project{
		ID: "fixture", Name: "Fixture", RegistrationStatus: ProjectRegistered,
		Status: ProjectActive, RepositoryPath: "/tmp/fixture", RepositoryIdentity: strings.Repeat("a", 64),
		BaseRefs: []string{"main"}, QualityProfile: ProfileGeneric, WorkspaceScope: "src",
		Capabilities:  []Capability{CapRepositoryEdit, CapGenericToolchain},
		ExecutionMode: "trusted-local", SecurityLabel: TrustedLocalSecurityLabel,
		ConfigRevision: 1, ConfigHash: strings.Repeat("b", 64),
	}
	project.ConfigHash = project.ComputeConfigHash()
	action := Action{ID: strings.Repeat("c", 32), ProjectID: project.ID, Status: StatusPending}
	grant := StandingDelegation{
		ID: "grant-1", PrincipalID: "owner-1", AuditID: "approval-1", ProjectID: project.ID,
		Revision: 1, ObjectiveClasses: []LeadObjectiveClass{LeadFeature, LeadMaintenance, LeadRefactor},
		AllowedPaths: []string{"src"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain},
		RiskClasses: []RiskClass{RiskR0, RiskR1}, Provider: "opencode", Model: "opencode-go/deepseek-v4.1-flash", PaseoMode: "build",
		MaxAttempts: 2, MaxTimeoutSecs: 300, MaxRepairCycles: 1, MaxWallClockSecs: 900,
		MaxParallel: 2, MaxSessions: 4, MaxRequests: 2, MaxCumulativeSessions: 8,
		MaxCumulativeAttempts: 8, AutonomousStart: true,
		AutonomousRiskClasses: []RiskClass{RiskR0}, ExpiresAt: now.Add(time.Hour),
	}
	grant.ContentHash = grant.ComputeContentHash()
	proposal := LeadPlanProposal{
		ProjectID: project.ID, ActionID: action.ID, BaseRevision: strings.Repeat("d", 40),
		ProjectConfigHash: project.ConfigHash, Objective: "Add a fixture feature", Class: LeadFeature,
		Assumptions: []string{"The existing package remains the target."}, Risks: []string{"The change may affect callers."},
		AcceptanceCriteria: []SpecCheck{
			{ID: "build", Kind: CheckBuild, Description: "Build succeeds", Command: []string{"go", "build", "./..."}, TimeoutSecs: 60},
			{ID: "test", Kind: CheckTest, Description: "Tests pass", Command: []string{"go", "test", "./..."}, TimeoutSecs: 60},
			{ID: "functional", Kind: CheckFunctional, Description: "Feature works", Command: []string{"go", "run", "./check"}, TimeoutSecs: 60},
		},
		NegativeCases:  []SpecCheck{{ID: "negative", Kind: CheckNegative, Description: "Invalid input rejected", Command: []string{"go", "run", "./check-negative"}, ExpectExit: 1, TimeoutSecs: 60}},
		QualityProfile: ProfileGeneric, RiskClass: RiskR0, Budget: SpecBudget{MaxAttempts: 1, TimeoutSecs: 120},
		Provider: "opencode", Model: "opencode-go/deepseek-v4.1-flash", PaseoMode: "build",
		MaxRepairCycles: 1, MaxWallClockSecs: 600, MaxParallel: 1, MaxSessions: 3,
		WorkUnits: []LeadWorkUnit{{ID: "implement", Kind: string(LeadImplementationUnit), AllowedPaths: []string{"src/feature"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}}},
	}
	return grant, project, action, proposal, now
}

func TestLeadPlanProposalValidatesBoundedDelegation(t *testing.T) {
	grant, project, action, proposal, now := validLeadInputs()
	if err := proposal.Validate(grant, project, action, now); err != nil {
		t.Fatalf("valid proposal rejected: %v", err)
	}
	contentHash := proposal.ContentHash()
	if contentHash == "" || contentHash != proposal.ContentHash() {
		t.Fatal("proposal content hash is empty or unstable")
	}
}

func TestStandingDelegationRejectsExpiredRevokedAndTamperedGrants(t *testing.T) {
	grant, _, _, _, now := validLeadInputs()
	grant.RevokedAt = now.Add(-time.Minute)
	grant.ContentHash = grant.ComputeContentHash()
	if err := grant.Validate(now); err == nil {
		t.Fatal("revoked grant was accepted")
	}
	grant, _, _, _, _ = validLeadInputs()
	if err := grant.Validate(grant.ExpiresAt); err == nil {
		t.Fatal("grant at expiry boundary was accepted")
	}
	grant, _, _, _, now = validLeadInputs()
	grant.MaxAttempts++
	if err := grant.Validate(now); err == nil {
		t.Fatal("tampered immutable grant was accepted")
	}
}

func TestLeadPlanRejectsBindingAndScopeViolations(t *testing.T) {
	cases := []struct {
		name string
		edit func(*StandingDelegation, *Project, *Action, *LeadPlanProposal)
	}{
		{"wrong action", func(_ *StandingDelegation, _ *Project, _ *Action, p *LeadPlanProposal) {
			p.ActionID = strings.Repeat("e", 32)
		}},
		{"stale config", func(_ *StandingDelegation, _ *Project, _ *Action, p *LeadPlanProposal) {
			p.ProjectConfigHash = strings.Repeat("f", 64)
		}},
		{"outside grant paths", func(_ *StandingDelegation, _ *Project, _ *Action, p *LeadPlanProposal) {
			p.WorkUnits[0].AllowedPaths = []string{"docs"}
		}},
		{"outside project scope", func(g *StandingDelegation, _ *Project, _ *Action, p *LeadPlanProposal) {
			g.AllowedPaths = []string{"."}
			g.ContentHash = g.ComputeContentHash()
			p.WorkUnits[0].AllowedPaths = []string{"docs"}
		}},
		{"capability expansion", func(_ *StandingDelegation, _ *Project, _ *Action, p *LeadPlanProposal) {
			p.WorkUnits[0].Capabilities = append(p.WorkUnits[0].Capabilities, CapBrowser)
		}},
		{"unsupported unit", func(_ *StandingDelegation, _ *Project, _ *Action, p *LeadPlanProposal) {
			p.WorkUnits[0].Kind = "review"
		}},
		{"missing dependency", func(_ *StandingDelegation, _ *Project, _ *Action, p *LeadPlanProposal) {
			p.WorkUnits[0].DependsOn = []string{"missing"}
		}},
		{"cycle", func(_ *StandingDelegation, _ *Project, _ *Action, p *LeadPlanProposal) {
			p.WorkUnits[0].DependsOn = []string{"implement"}
		}},
		{"unsupported risk", func(_ *StandingDelegation, _ *Project, _ *Action, p *LeadPlanProposal) { p.RiskClass = RiskR3 }},
		{"route change", func(_ *StandingDelegation, _ *Project, _ *Action, p *LeadPlanProposal) { p.Model = "other-model" }},
		{"attempt escalation", func(_ *StandingDelegation, _ *Project, _ *Action, p *LeadPlanProposal) { p.Budget.MaxAttempts = 3 }},
		{"session escalation", func(_ *StandingDelegation, _ *Project, _ *Action, p *LeadPlanProposal) { p.MaxSessions = 5 }},
		{"assessment session omitted", func(_ *StandingDelegation, _ *Project, _ *Action, p *LeadPlanProposal) { p.MaxSessions = 2 }},
		{"two executable units", func(_ *StandingDelegation, _ *Project, _ *Action, p *LeadPlanProposal) {
			p.WorkUnits = append(p.WorkUnits, p.WorkUnits[0])
		}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			grant, project, action, proposal, now := validLeadInputs()
			tc.edit(&grant, &project, &action, &proposal)
			if err := proposal.Validate(grant, project, action, now); err == nil {
				t.Fatal("invalid proposal was accepted")
			}
		})
	}
}

func TestLeadPlanRequiresActiveRegisteredProjectAndLiveAction(t *testing.T) {
	grant, project, action, proposal, now := validLeadInputs()
	project.Status = ProjectPaused
	if err := proposal.Validate(grant, project, action, now); err == nil {
		t.Fatal("paused project accepted new planning")
	}
	grant, project, action, proposal, now = validLeadInputs()
	project.RegistrationStatus = ProjectLegacy
	if err := proposal.Validate(grant, project, action, now); err == nil {
		t.Fatal("legacy project accepted new planning")
	}
	grant, project, action, proposal, now = validLeadInputs()
	action.Status = StatusCompleted
	if err := proposal.Validate(grant, project, action, now); err == nil {
		t.Fatal("terminal Action accepted new planning")
	}
}

func TestLeadPlanAcceptsSequentialParallelAndFanInGraphs(t *testing.T) {
	cases := []struct {
		name  string
		units []LeadWorkUnit
	}{
		{name: "sequential", units: []LeadWorkUnit{
			{ID: "api", Kind: string(LeadImplementationUnit), Description: "Add backend behavior.", AllowedPaths: []string{"src/api"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}},
			{ID: "ui", Kind: string(LeadImplementationUnit), Description: "Consume the backend behavior.", DependsOn: []string{"api"}, AllowedPaths: []string{"src/ui"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}},
		}},
		{name: "parallel", units: []LeadWorkUnit{
			{ID: "api", Kind: string(LeadImplementationUnit), AllowedPaths: []string{"src/api"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}},
			{ID: "ui", Kind: string(LeadImplementationUnit), AllowedPaths: []string{"src/ui"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}},
		}},
		{name: "parallel-fan-in-integration", units: []LeadWorkUnit{
			{ID: "api", Kind: string(LeadImplementationUnit), AllowedPaths: []string{"src/api"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}},
			{ID: "ui", Kind: string(LeadImplementationUnit), AllowedPaths: []string{"src/ui"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}},
			{ID: "docs", Kind: string(LeadImplementationUnit), AllowedPaths: []string{"src/docs"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}},
		}},
		{name: "ordinary-unit-fan-in", units: []LeadWorkUnit{
			{ID: "api", Kind: string(LeadImplementationUnit), AllowedPaths: []string{"src/api"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}},
			{ID: "ui", Kind: string(LeadImplementationUnit), AllowedPaths: []string{"src/ui"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}},
			{ID: "wiring", Kind: string(LeadImplementationUnit), DependsOn: []string{"api", "ui"}, AllowedPaths: []string{"src/wiring"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}},
		}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			grant, project, action, proposal, now := validLeadInputs()
			proposal.WorkUnits = tc.units
			proposal.MaxSessions = 16
			grant.MaxSessions = 16
			grant.MaxCumulativeSessions = 32
			grant.MaxCumulativeAttempts = 32
			grant.ContentHash = grant.ComputeContentHash()
			if err := proposal.Validate(grant, project, action, now); err != nil {
				t.Fatalf("valid %s graph rejected: %v", tc.name, err)
			}
			unitSpecs := map[string]string{}
			for _, unit := range tc.units {
				unitSpecs[unit.ID] = testSpecID("spec-" + unit.ID)
			}
			terminal := proposal.TerminalUnits()
			campaignSpec := testSpecID("integration")
			if len(terminal) == 1 {
				campaignSpec = unitSpecs[terminal[0].ID]
			}
			plan, err := BuildLeadCampaignPlan(proposal, unitSpecs, campaignSpec)
			if err != nil {
				t.Fatalf("build %s graph: %v", tc.name, err)
			}
			if err := plan.Validate(); err != nil {
				t.Fatalf("validate %s graph: %v", tc.name, err)
			}
			wantSource := LeadUnitTaskKey(terminal[0].ID)
			if proposal.NeedsIntegration() {
				wantSource = IntegrationTaskKey
				if _, ok := plan.TaskPlan(IntegrationTaskKey); !ok {
					t.Fatalf("%s graph has no integration task", tc.name)
				}
			} else if _, ok := plan.TaskPlan(IntegrationTaskKey); ok {
				t.Fatalf("%s graph unexpectedly has an integration task", tc.name)
			}
			gate, ok := plan.TaskPlan(CycleKey(RoleQualityGate, 0))
			if !ok || gate.SourceKey != wantSource {
				t.Fatalf("%s gate source = %+v, want %s", tc.name, gate, wantSource)
			}
			repair, ok := plan.TaskPlan(CycleKey(RoleImplementer, 1))
			if !ok || repair.UnitKind != LeadUnitKindRepair || repair.SpecID != campaignSpec {
				t.Fatalf("%s repair task = %+v", tc.name, repair)
			}
		})
	}
}

func TestLeadPlanRejectsInvalidGraphShapesAndParallelOverlap(t *testing.T) {
	base := []LeadWorkUnit{
		{ID: "api", Kind: string(LeadImplementationUnit), AllowedPaths: []string{"src/api"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}},
		{ID: "ui", Kind: string(LeadImplementationUnit), AllowedPaths: []string{"src/ui"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}},
	}
	cases := []struct {
		name  string
		units []LeadWorkUnit
	}{
		{name: "unknown dependency", units: []LeadWorkUnit{base[0], {ID: "ui", Kind: string(LeadImplementationUnit), DependsOn: []string{"missing"}, AllowedPaths: []string{"src/ui"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}}}},
		{name: "self dependency", units: []LeadWorkUnit{{ID: "api", Kind: string(LeadImplementationUnit), DependsOn: []string{"api"}, AllowedPaths: []string{"src/api"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}}}},
		{name: "cycle", units: []LeadWorkUnit{
			{ID: "api", Kind: string(LeadImplementationUnit), DependsOn: []string{"ui"}, AllowedPaths: []string{"src/api"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}},
			{ID: "ui", Kind: string(LeadImplementationUnit), DependsOn: []string{"api"}, AllowedPaths: []string{"src/ui"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}},
		}},
		{name: "parallel path overlap", units: []LeadWorkUnit{
			{ID: "api", Kind: string(LeadImplementationUnit), AllowedPaths: []string{"src"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}},
			{ID: "ui", Kind: string(LeadImplementationUnit), AllowedPaths: []string{"src/ui"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}},
		}},
		{name: "unauthorized path", units: []LeadWorkUnit{{ID: "api", Kind: string(LeadImplementationUnit), AllowedPaths: []string{"outside"}, Capabilities: []Capability{CapRepositoryEdit, CapGenericToolchain}}}},
		{name: "duplicate ids", units: []LeadWorkUnit{base[0], base[0]}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			grant, project, action, proposal, now := validLeadInputs()
			proposal.WorkUnits = tc.units
			proposal.MaxSessions = 16
			grant.MaxSessions = 16
			grant.MaxCumulativeSessions = 32
			grant.MaxCumulativeAttempts = 32
			grant.ContentHash = grant.ComputeContentHash()
			if err := proposal.Validate(grant, project, action, now); err == nil {
				t.Fatal("invalid graph was accepted")
			}
		})
	}
}

func TestStandingDelegationRequiresExplicitAutonomousPolicy(t *testing.T) {
	grant, _, _, _, now := validLeadInputs()
	grant.AutonomousStart = true
	grant.AutonomousRiskClasses = nil
	grant.ContentHash = grant.ComputeContentHash()
	if err := grant.Validate(now); err == nil {
		t.Fatal("autonomous start without risk classes was accepted")
	}
	grant.AutonomousRiskClasses = []RiskClass{RiskR2}
	grant.ContentHash = grant.ComputeContentHash()
	if err := grant.Validate(now); err == nil {
		t.Fatal("R2 autonomous authority was accepted")
	}
	grant.AutonomousRiskClasses = []RiskClass{RiskR0}
	grant.MaxCumulativeSessions = grant.MaxSessions - 1
	grant.ContentHash = grant.ComputeContentHash()
	if err := grant.Validate(now); err == nil {
		t.Fatal("cumulative session ceiling below per-campaign ceiling was accepted")
	}
}

func testSpecID(value string) string {
	sum := sha256.Sum256([]byte(value))
	return hex.EncodeToString(sum[:16])
}
