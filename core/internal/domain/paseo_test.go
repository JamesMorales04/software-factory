package domain

import (
	"strings"
	"testing"
)

func TestPaseoAllowedRoutesAreExact(t *testing.T) {
	for _, route := range PaseoAllowedRoutes {
		got, err := ValidatePaseoRoute(route.Provider, route.Model, route.Mode)
		if err != nil {
			t.Fatalf("allowlisted route rejected: %+v: %v", route, err)
		}
		if got != route {
			t.Fatalf("route mismatch: got %+v want %+v", got, route)
		}
	}
}

func TestPaseoRouteRejectsSubstitution(t *testing.T) {
	cases := []struct {
		name     string
		provider string
		model    string
		mode     string
	}{
		{"empty provider", "", "gpt-6-luna", "full-access"},
		{"empty model", "codex", "", "full-access"},
		{"empty mode", "codex", "gpt-6-luna", ""},
		{"wrong mode for codex", "codex", "gpt-6-luna", "build"},
		{"wrong mode for opencode", "opencode", "opencode-go/muse-spark-1.3-contributor", "full-access"},
		{"model swapped across providers", "codex", "opencode-go/muse-spark-1.3-contributor", "build"},
		{"provider swapped", "opencode", "gpt-6-luna", "full-access"},
		{"unlisted copilot route", "opencode", "github-copilot/gpt-5-mini", "build"},
		{"unlisted model", "codex", "gpt-7-future", "full-access"},
		{"case differs", "Codex", "gpt-6-luna", "full-access"},
		{"whitespace-only model", "codex", "   ", "full-access"},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if _, err := ValidatePaseoRoute(c.provider, c.model, c.mode); err == nil {
				t.Fatalf("route %q/%q mode %q accepted, want rejection",
					c.provider, c.model, c.mode)
			}
		})
	}
}

func TestPaseoRouteTrimsSurroundingWhitespace(t *testing.T) {
	got, err := ValidatePaseoRoute("  codex  ", "gpt-6-luna", "full-access")
	if err != nil {
		t.Fatalf("trimmed route rejected: %v", err)
	}
	if got.Provider != "codex" || got.Model != "gpt-6-luna" || got.Mode != "full-access" {
		t.Fatalf("unexpected route: %+v", got)
	}
}

func TestPaseoTaskInputBounds(t *testing.T) {
	out, err := ValidateWorkloadInput(PaseoTaskKind, map[string]any{"task": "  add multiply  "})
	if err != nil {
		t.Fatalf("valid task rejected: %v", err)
	}
	if out["task"] != "add multiply" {
		t.Fatalf("task not trimmed: %q", out["task"])
	}
	rejects := []map[string]any{
		nil,
		{},
		{"task": ""},
		{"task": "   "},
		{"task": strings.Repeat("x", MaxPaseoTaskChars+1)},
		{"task": "ok", "cmd": "id"},
		{"wrong": "task"},
	}
	for i, input := range rejects {
		if _, err := ValidateWorkloadInput(PaseoTaskKind, input); err == nil {
			t.Fatalf("case %d accepted, want rejection: %v", i, input)
		}
	}
}

func TestNewRunParamsPaseoTaskRequiresTrustedLocal(t *testing.T) {
	valid := NewRunParams{
		WorkloadKind:  PaseoTaskKind,
		WorkloadInput: map[string]any{"task": "add multiply"},
		ExecutionMode: string(ExecTrustedLocal),
		Provider:      "codex",
		Model:         "gpt-6-luna",
		PaseoMode:     "full-access",
	}
	got, err := valid.Validate()
	if err != nil {
		t.Fatalf("valid paseo-task rejected: %v", err)
	}
	if got.Provider != "codex" || got.Model != "gpt-6-luna" || got.PaseoMode != "full-access" {
		t.Fatalf("routing not preserved: %+v", got)
	}

	invalid := []NewRunParams{
		// Missing execution mode defaults to restricted, which paseo-task forbids.
		{
			WorkloadKind: PaseoTaskKind, WorkloadInput: map[string]any{"task": "x"},
			Provider: "codex", Model: "gpt-6-luna", PaseoMode: "full-access",
		},
		// Explicit restricted with paseo-task.
		{
			WorkloadKind: PaseoTaskKind, WorkloadInput: map[string]any{"task": "x"},
			ExecutionMode: string(ExecRestricted),
			Provider:      "codex", Model: "gpt-6-luna", PaseoMode: "full-access",
		},
		// Trusted-local with synthetic echo.
		{
			WorkloadKind: SyntheticEchoKind, WorkloadInput: map[string]any{"message": "x"},
			ExecutionMode: string(ExecTrustedLocal),
		},
		// Synthetic echo carrying routing.
		{
			WorkloadKind: SyntheticEchoKind, WorkloadInput: map[string]any{"message": "x"},
			Provider: "codex",
		},
		// Unknown mode for an allowlisted model.
		{
			WorkloadKind: PaseoTaskKind, WorkloadInput: map[string]any{"task": "x"},
			ExecutionMode: string(ExecTrustedLocal),
			Provider:      "codex", Model: "gpt-6-luna", PaseoMode: "build",
		},
		// ISOLATED is not a valid S3-A execution mode.
		{
			WorkloadKind: PaseoTaskKind, WorkloadInput: map[string]any{"task": "x"},
			ExecutionMode: "isolated",
			Provider:      "codex", Model: "gpt-6-luna", PaseoMode: "full-access",
		},
	}
	for i, p := range invalid {
		if _, err := p.Validate(); err == nil {
			t.Fatalf("case %d accepted, want rejection: %+v", i, p)
		}
	}
}

func TestUnknownWorkloadKindRejected(t *testing.T) {
	if _, err := ValidateWorkloadInput("shell-exec", map[string]any{"task": "x"}); err == nil {
		t.Fatal("expected rejection of unknown workloadKind")
	}
	if _, err := (NewRunParams{
		WorkloadKind: "shell-exec", WorkloadInput: map[string]any{"task": "x"},
	}).Validate(); err == nil {
		t.Fatal("expected rejection of unknown workloadKind in NewRunParams")
	}
}

func TestValidateExecutionMode(t *testing.T) {
	for _, mode := range []string{"restricted", "trusted-local"} {
		if err := ValidateExecutionMode(mode); err != nil {
			t.Fatalf("mode %q rejected: %v", mode, err)
		}
	}
	for _, mode := range []string{"", "isolated", "sandboxed", "RESTRICTED"} {
		if err := ValidateExecutionMode(mode); err == nil {
			t.Fatalf("mode %q accepted, want rejection", mode)
		}
	}
}

func TestValidatePaseoIdentity(t *testing.T) {
	valid := []string{
		"7632c120-167c-4459-a394-e1ed0f7cc524",
		"ws_abc123",
		"a",
		strings.Repeat("x", MaxPaseoIDLength),
	}
	for _, id := range valid {
		if err := ValidatePaseoIdentity("sessionId", id); err != nil {
			t.Fatalf("identity %q rejected: %v", id, err)
		}
	}
	invalid := []string{
		"",
		"   ",
		strings.Repeat("x", MaxPaseoIDLength+1),
		"has space",
		"has/slash",
		"has$dollar",
	}
	for _, id := range invalid {
		if err := ValidatePaseoIdentity("sessionId", id); err == nil {
			t.Fatalf("identity %q accepted, want rejection", id)
		}
	}
}

func TestPaseoSessionStatusTerminal(t *testing.T) {
	for _, s := range []PaseoSessionStatus{PaseoSucceeded, PaseoFailed, PaseoCancelled, PaseoOrphaned} {
		if !s.Terminal() {
			t.Fatalf("%s should be terminal", s)
		}
	}
	for _, s := range []PaseoSessionStatus{PaseoIntended, PaseoActive} {
		if s.Terminal() {
			t.Fatalf("%s should not be terminal", s)
		}
	}
	if _, err := ParsePaseoSessionStatus("intended"); err != nil {
		t.Fatalf("valid status rejected: %v", err)
	}
	if _, err := ParsePaseoSessionStatus("bogus"); err == nil {
		t.Fatal("expected rejection of unknown session status")
	}
}
