package domain

import (
	"strings"
	"testing"
)

func validProjectParams() NewProjectParams {
	return NewProjectParams{
		ID: "vlog", Name: "Vlog", RepositoryPath: "/tmp/vlog",
		RepositoryIdentity: strings.Repeat("a", 64), BaseRefs: []string{"main"},
		QualityProfile: "node-browser", WorkspaceScope: ".",
		Capabilities:     []string{"repository-edit", "node-toolchain", "browser"},
		BrowserEndpoints: []string{"http://127.0.0.1:4173"},
		ExecutionMode:    "trusted-local",
	}
}

func TestProjectRegistrationValidation(t *testing.T) {
	project, err := validProjectParams().Validate()
	if err != nil {
		t.Fatalf("valid project rejected: %v", err)
	}
	if project.RegistrationStatus != ProjectRegistered || project.ConfigRevision != 1 ||
		project.ConfigHash == "" || project.SecurityLabel != TrustedLocalSecurityLabel {
		t.Fatalf("unexpected project: %+v", project)
	}
	cases := []struct {
		name   string
		mutate func(*NewProjectParams)
	}{
		{"browser profile needs browser capability", func(p *NewProjectParams) {
			p.Capabilities = []string{"repository-edit", "node-toolchain"}
			p.BrowserEndpoints = nil
		}},
		{"browser capability needs loopback endpoint", func(p *NewProjectParams) {
			p.BrowserEndpoints = nil
		}},
		{"non-loopback endpoint rejected", func(p *NewProjectParams) {
			p.BrowserEndpoints = []string{"http://example.com:4173"}
		}},
		{"scheme must be http", func(p *NewProjectParams) {
			p.BrowserEndpoints = []string{"https://127.0.0.1:4173"}
		}},
		{"unknown capability rejected", func(p *NewProjectParams) {
			p.Capabilities = append(p.Capabilities, "root-shell")
		}},
		{"unsafe base ref rejected", func(p *NewProjectParams) {
			p.BaseRefs = []string{"main..evil"}
		}},
		{"workspace traversal rejected", func(p *NewProjectParams) {
			p.WorkspaceScope = "../other"
		}},
		{"isolated mode rejected", func(p *NewProjectParams) {
			p.ExecutionMode = "isolated"
		}},
		{"duplicate capabilities rejected", func(p *NewProjectParams) {
			p.Capabilities = append(p.Capabilities, "browser")
		}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			params := validProjectParams()
			tc.mutate(&params)
			if _, err := params.Validate(); err == nil {
				t.Fatal("invalid project accepted")
			}
		})
	}
}

func TestProjectUpdateCannotDropSpecCapability(t *testing.T) {
	project, err := validProjectParams().Validate()
	if err != nil {
		t.Fatal(err)
	}
	status := "paused"
	next, changed, err := ProjectUpdateParams{Status: &status}.Apply(project)
	if err != nil || !changed {
		t.Fatalf("valid update rejected: %v changed=%v", err, changed)
	}
	if next.Status != ProjectPaused || next.ConfigRevision != 2 || next.ConfigHash == project.ConfigHash {
		t.Fatalf("update did not advance configuration: %+v", next)
	}
	caps := []string{"repository-edit", "node-toolchain"}
	params := NewActionSpecParams{
		ProjectID: project.ID, ActionID: strings.Repeat("1", 32), Goal: "goal",
		Repository: project.RepositoryPath, BaseRevision: strings.Repeat("b", 40),
		AllowedPaths: []string{"src"}, RiskClass: "R0", QualityProfile: "node-browser",
		AcceptanceCriteria: []SpecCheck{
			{ID: "format", Kind: CheckFormat, Description: "f", Command: []string{"node", "-e", "0"}},
			{ID: "test", Kind: CheckTest, Description: "t", Command: []string{"node", "-e", "0"}},
			{ID: "functional", Kind: CheckFunctional, Description: "u", Command: []string{"node", "-e", "0"}},
			{ID: "browser", Kind: CheckBrowser, Description: "b", Command: []string{"node", "-e", "0"}},
		},
		NegativeCases: []SpecCheck{{ID: "negative", Kind: CheckNegative, Description: "n", Command: []string{"node", "-e", "0"}}},
		Budget:        SpecBudget{MaxAttempts: 1, TimeoutSecs: 60},
		ExecutionMode: "trusted-local", Provider: "opencode",
		Model: "opencode-go/deepseek-v4.1-flash", PaseoMode: "build",
		ProjectConfigRevision: project.ConfigRevision, RepositoryIdentity: project.RepositoryIdentity,
		Capabilities:     []string{"repository-edit", "node-toolchain", "browser"},
		BrowserEndpoints: project.BrowserEndpoints,
	}
	spec, err := params.Validate()
	if err != nil {
		t.Fatalf("valid bound spec rejected: %v", err)
	}
	update := ProjectUpdateParams{Capabilities: &caps}
	next.Capabilities = project.Capabilities
	next.ConfigRevision = project.ConfigRevision
	next.ConfigHash = project.ComputeConfigHash()
	_ = spec
	if _, _, err := update.Apply(next); err == nil {
		t.Fatal("capability removal for a browser profile must be rejected")
	}
}

func TestSpecProjectBindingRejectsDifferentRepository(t *testing.T) {
	project, err := validProjectParams().Validate()
	if err != nil {
		t.Fatal(err)
	}
	spec := ActionSpec{
		ProjectID: project.ID, Repository: project.RepositoryPath,
		RepositoryIdentity: strings.Repeat("b", 64), ProjectConfigRevision: 1,
		Capabilities: []Capability{CapRepositoryEdit}, BindingHash: strings.Repeat("c", 64),
	}
	if err := spec.ValidateProjectBinding(project); err == nil {
		t.Fatal("different repository identity accepted")
	}
}
