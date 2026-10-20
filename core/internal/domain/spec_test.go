package domain

import (
	"strings"
	"testing"
)

func validSpecParams(t *testing.T) NewActionSpecParams {
	t.Helper()
	return NewActionSpecParams{
		ActionID:     strings.Repeat("a", 32),
		Goal:         "Add multiply to add.js",
		Repository:   "/tmp/factory-repo.git",
		BaseRevision: strings.Repeat("b", 40),
		AllowedPaths: []string{"add.js", "src/**"},
		AcceptanceCriteria: []SpecCheck{
			{ID: "format", Kind: CheckFormat, Description: "format", Command: []string{"node", "--check", "add.js"}},
			{ID: "test", Kind: CheckTest, Description: "tests", Command: []string{"node", "--test"}, TimeoutSecs: 30},
			{ID: "functional", Kind: CheckFunctional, Description: "multiply works", Command: []string{"node", "-e", "process.exit(0)"}},
		},
		NegativeCases: []SpecCheck{
			{ID: "negative", Kind: CheckNegative, Description: "invalid input", Command: []string{"node", "-e", "process.exit(1)"}, ExpectExit: 1},
		},
		RiskClass:      "R0",
		QualityProfile: "node-basic",
		Budget:         SpecBudget{MaxAttempts: 1, TimeoutSecs: 120},
		ExecutionMode:  "trusted-local",
		Provider:       "opencode",
		Model:          "opencode-go/deepseek-v4.1-flash",
		PaseoMode:      "build",
	}
}

func TestNewActionSpecFreezesDeterministicHash(t *testing.T) {
	params := validSpecParams(t)
	first, err := params.Validate()
	if err != nil {
		t.Fatalf("Validate: %v", err)
	}
	second, err := params.Validate()
	if err != nil {
		t.Fatalf("Validate: %v", err)
	}
	if first.ContentHash == "" || first.ContentHash != second.ContentHash {
		t.Fatalf("content hash is not deterministic: %q vs %q", first.ContentHash, second.ContentHash)
	}
	if first.PolicyRevision != QualityPolicyRevision {
		t.Fatalf("policy revision not bound: %q", first.PolicyRevision)
	}
	params.Goal = "different goal"
	changed, err := params.Validate()
	if err != nil {
		t.Fatalf("Validate: %v", err)
	}
	if changed.ContentHash == first.ContentHash {
		t.Fatal("changing the goal must change the frozen content identity")
	}
}

func TestSpecValidationRejectsWeakOrUnsafeInput(t *testing.T) {
	cases := []struct {
		name   string
		mutate func(*NewActionSpecParams)
	}{
		{"profile cannot remove required kinds", func(p *NewActionSpecParams) {
			p.AcceptanceCriteria = p.AcceptanceCriteria[:2]
		}},
		{"negative case must be negative", func(p *NewActionSpecParams) {
			p.NegativeCases[0].Kind = CheckFunctional
		}},
		{"shell metacharacters are not special but empty argv is invalid", func(p *NewActionSpecParams) {
			p.AcceptanceCriteria[0].Command = nil
		}},
		{"executable allowlist", func(p *NewActionSpecParams) {
			p.AcceptanceCriteria[0].Command = []string{"rm", "-rf", "/"}
		}},
		{"path traversal", func(p *NewActionSpecParams) {
			p.AllowedPaths = []string{"../secrets"}
		}},
		{"relative repository", func(p *NewActionSpecParams) {
			p.Repository = "repo"
		}},
		{"non trusted-local mode", func(p *NewActionSpecParams) {
			p.ExecutionMode = "isolated"
		}},
		{"unknown risk", func(p *NewActionSpecParams) {
			p.RiskClass = "R9"
		}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			params := validSpecParams(t)
			tc.mutate(&params)
			if _, err := params.Validate(); err == nil {
				t.Fatal("expected validation failure")
			}
		})
	}
}

func TestPathAllowedHonorsScope(t *testing.T) {
	allowed := []string{"add.js", "src/**", "*.test.js"}
	for _, path := range []string{"add.js", "src/a.js", "src/deep/b.js", "add.test.js"} {
		if !PathAllowed(path, allowed) {
			t.Fatalf("expected %q to be allowed", path)
		}
	}
	for _, path := range []string{"other.js", "src/../secret", "src/a/.git/config"} {
		if PathAllowed(path, allowed) {
			t.Fatalf("expected %q to be rejected", path)
		}
	}
}

func TestNodeBrowserProfileRequiresBrowserCheck(t *testing.T) {
	params := validSpecParams(t)
	params.QualityProfile = "node-browser"
	if _, err := params.Validate(); err == nil {
		t.Fatal("node-browser profile must require a browser check")
	}
	params.AcceptanceCriteria = append(params.AcceptanceCriteria, SpecCheck{
		ID: "browser", Kind: CheckBrowser, Description: "ui",
		Command: []string{"node", "browser-check.mjs"}, TimeoutSecs: 60,
	})
	if _, err := params.Validate(); err != nil {
		t.Fatalf("valid browser spec rejected: %v", err)
	}
}
