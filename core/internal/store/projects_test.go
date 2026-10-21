package store

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

func testProject(t *testing.T, id string, profile domain.QualityProfile, caps []string, endpoints []string) domain.Project {
	t.Helper()
	seed := "c"
	if strings.HasPrefix(id, "a") {
		seed = "a"
	} else if strings.HasPrefix(id, "b") {
		seed = "b"
	}
	params := domain.NewProjectParams{
		ID: id, Name: id, RepositoryPath: "/tmp/" + id,
		RepositoryIdentity: strings.Repeat(seed, 64),
		BaseRefs:           []string{"main"}, QualityProfile: string(profile),
		WorkspaceScope: ".", Capabilities: caps, BrowserEndpoints: endpoints,
		ExecutionMode: "trusted-local",
	}
	project, err := params.Validate()
	if err != nil {
		t.Fatalf("validate %s: %v", id, err)
	}
	return project
}

func TestProjectRegistryCreateUpdateAndLegacy(t *testing.T) {
	db := openTestDB(t)
	projects, err := db.ListProjects()
	if err != nil {
		t.Fatal(err)
	}
	if len(projects) != 1 || projects[0].ID != domain.DefaultProjectID || projects[0].RegistrationStatus != domain.ProjectLegacy {
		t.Fatalf("legacy default missing: %+v", projects)
	}
	project := testProject(t, "alpha", domain.ProfileNodeBasic,
		[]string{"repository-edit", "node-toolchain"}, nil)
	stored, event, err := db.CreateProject(context.Background(), project, "c")
	if err != nil {
		t.Fatal(err)
	}
	if stored.ConfigRevision != 1 || event.EventType != domain.EventProjectRegistered {
		t.Fatalf("bad create: %+v %+v", stored, event)
	}
	duplicate := testProject(t, "beta", domain.ProfileNodeBasic,
		[]string{"repository-edit", "node-toolchain"}, nil)
	duplicate.RepositoryIdentity = stored.RepositoryIdentity
	duplicate.ConfigHash = duplicate.ComputeConfigHash()
	if _, _, err := db.CreateProject(context.Background(), duplicate, "c"); !errors.Is(err, domain.ErrConflict) {
		t.Fatalf("duplicate identity not rejected: %v", err)
	}
	name := "Renamed"
	updated, event, changed, err := db.UpdateProject(context.Background(), stored.ID,
		domain.ProjectUpdateParams{Name: &name}, "c")
	if err != nil || !changed {
		t.Fatalf("update failed: %v changed=%v", err, changed)
	}
	if updated.ConfigRevision != 2 || updated.ConfigHash == stored.ConfigHash ||
		event.EventType != domain.EventProjectUpdated {
		t.Fatalf("bad update: %+v", updated)
	}
	snapshot, err := db.ProjectConfigRevision(stored.ID, 1)
	if err != nil || snapshot["name"] != "alpha" {
		t.Fatalf("revision snapshot missing: %v %+v", err, snapshot)
	}
	paused := "paused"
	statusUpdate, event, changed, err := db.UpdateProject(context.Background(), stored.ID,
		domain.ProjectUpdateParams{Status: &paused}, "c")
	if err != nil || !changed || statusUpdate.Status != domain.ProjectPaused ||
		event.EventType != domain.EventProjectStatusChanged {
		t.Fatalf("status update failed: %v %+v", err, event)
	}
}

func TestRunClaimFiltersCapabilitiesAndRotatesProjects(t *testing.T) {
	db := openTestDB(t)
	ctx := context.Background()
	alpha := testProject(t, "alpha", domain.ProfileNodeBasic,
		[]string{"repository-edit", "node-toolchain"}, nil)
	beta := testProject(t, "beta", domain.ProfileNodeBasic,
		[]string{"repository-edit", "node-toolchain"}, nil)
	if _, _, err := db.CreateProject(ctx, alpha, "c"); err != nil {
		t.Fatal(err)
	}
	if _, _, err := db.CreateProject(ctx, beta, "c"); err != nil {
		t.Fatal(err)
	}
	makeRun := func(project domain.Project, title string) domain.Run {
		action, _, err := db.CreateAction(ctx, domain.NewActionParams{ProjectID: project.ID, Title: title}, "c")
		if err != nil {
			t.Fatal(err)
		}
		checks := []domain.SpecCheck{
			{ID: "format", Kind: domain.CheckFormat, Description: "f", Command: []string{"node", "-e", "0"}},
			{ID: "test", Kind: domain.CheckTest, Description: "t", Command: []string{"node", "-e", "0"}},
			{ID: "functional", Kind: domain.CheckFunctional, Description: "u", Command: []string{"node", "-e", "0"}},
		}
		if project.QualityProfile == domain.ProfileNodeBrowser {
			checks = append(checks, domain.SpecCheck{ID: "browser", Kind: domain.CheckBrowser, Description: "b", Command: []string{"node", "-e", "0"}})
		}
		requiredCaps, _ := domain.RequiredCapabilities(project.QualityProfile)
		capValues := make([]string, len(requiredCaps))
		for i, capability := range requiredCaps {
			capValues[i] = string(capability)
		}
		params := domain.NewActionSpecParams{
			ProjectID: project.ID, ActionID: action.ID, Goal: "do work",
			Repository: project.RepositoryPath, BaseRevision: strings.Repeat("b", 40),
			AllowedPaths: []string{"src"}, RiskClass: "R0", QualityProfile: string(project.QualityProfile),
			AcceptanceCriteria: checks,
			NegativeCases:      []domain.SpecCheck{{ID: "negative", Kind: domain.CheckNegative, Description: "n", Command: []string{"node", "-e", "0"}}},
			Budget:             domain.SpecBudget{MaxAttempts: 1, TimeoutSecs: 60},
			ExecutionMode:      "trusted-local", Provider: "opencode",
			Model: "opencode-go/deepseek-v4.1-flash", PaseoMode: "build",
			ProjectConfigRevision: project.ConfigRevision,
			RepositoryIdentity:    project.RepositoryIdentity,
			Capabilities:          capValues,
			BrowserEndpoints:      project.BrowserEndpoints,
		}
		spec, err := params.Validate()
		if err != nil {
			t.Fatal(err)
		}
		stored, _, _, err := db.CreateActionSpec(ctx, spec, "c")
		if err != nil {
			t.Fatal(err)
		}
		run, _, err := db.CreateRun(ctx, domain.NewRunParams{
			ProjectID: project.ID, ActionID: action.ID, SpecID: stored.ID,
			WorkloadKind:  domain.PaseoTaskKind,
			WorkloadInput: map[string]any{"task": "do work"},
			ExecutionMode: "trusted-local", Provider: "opencode",
			Model: "opencode-go/deepseek-v4.1-flash", PaseoMode: "build",
			MaxAttempts: 1, TimeoutSecs: 60,
		}, "c")
		if err != nil {
			t.Fatal(err)
		}
		return run
	}
	makeRun(alpha, "alpha run")
	makeRun(beta, "beta run")
	if _, _, err := db.RegisterWorker(ctx, "worker", "test", domain.AllCapabilities, []string{"trusted-local"}, "c"); err != nil {
		t.Fatal(err)
	}
	first, _, found, _, err := db.ClaimNextRun(ctx, "worker", "c")
	if err != nil || !found {
		t.Fatalf("first claim: %v %v", err, found)
	}
	second, _, found, _, err := db.ClaimNextRun(ctx, "worker", "c")
	if err != nil || !found {
		t.Fatalf("second claim: %v %v", err, found)
	}
	if first.ProjectID == second.ProjectID {
		t.Fatalf("claim did not rotate projects: %s then %s", first.ProjectID, second.ProjectID)
	}
	if _, _, err := db.RegisterWorker(ctx, "nobrowser", "test",
		[]domain.Capability{domain.CapRepositoryEdit, domain.CapNodeToolchain}, []string{"trusted-local"}, "c"); err != nil {
		t.Fatal(err)
	}
	// Both runs are now leased. Queue a browser-requiring run and confirm a
	// worker without the capability never claims it.
	gamma := testProject(t, "gamma", domain.ProfileNodeBrowser,
		[]string{"repository-edit", "node-toolchain", "browser"}, []string{"http://127.0.0.1:4173"})
	if _, _, err := db.CreateProject(ctx, gamma, "c"); err != nil {
		t.Fatal(err)
	}
	makeRun(gamma, "gamma run")
	for i := 0; i < 3; i++ {
		_, _, found, _, err := db.ClaimNextRun(ctx, "nobrowser", "c")
		if err != nil {
			t.Fatal(err)
		}
		if found {
			t.Fatal("worker without browser capability claimed a browser run")
		}
	}
}
