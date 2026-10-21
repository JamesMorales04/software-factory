package domain

import (
	"fmt"
	"testing"
)

func TestValidateWorkloadInputAcceptsMinimal(t *testing.T) {
	out, err := ValidateWorkloadInput(SyntheticEchoKind, map[string]any{"message": "hello"})
	if err != nil {
		t.Fatalf("validate: %v", err)
	}
	if out["message"] != "hello" || out["steps"] != 1 || out["stepMs"] != 0 {
		t.Fatalf("unexpected defaults: %v", out)
	}
}

func TestValidateWorkloadInputRejects(t *testing.T) {
	cases := []struct {
		name  string
		kind  string
		input map[string]any
	}{
		{"wrong kind", "shell-exec", map[string]any{"message": "x"}},
		{"missing message", SyntheticEchoKind, map[string]any{}},
		{"unknown key", SyntheticEchoKind, map[string]any{"message": "x", "cmd": "id"}},
		{"long message", SyntheticEchoKind, map[string]any{"message": string(make([]byte, MaxWorkloadMessage+1))}},
		{"too many steps", SyntheticEchoKind, map[string]any{"message": "x", "steps": MaxWorkloadSteps + 1}},
		{"negative stepMs", SyntheticEchoKind, map[string]any{"message": "x", "stepMs": -1}},
		{"big stepMs", SyntheticEchoKind, map[string]any{"message": "x", "stepMs": MaxWorkloadStepMs + 1}},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if _, err := ValidateWorkloadInput(c.kind, c.input); err == nil {
				t.Fatal("expected rejection")
			}
		})
	}
}

func TestNewRunParamsDefaultsAndLimits(t *testing.T) {
	p, err := NewRunParams{
		WorkloadKind:  SyntheticEchoKind,
		WorkloadInput: map[string]any{"message": "hi"},
	}.Validate()
	if err != nil {
		t.Fatalf("validate: %v", err)
	}
	if p.ProjectID != DefaultProjectID || p.MaxAttempts != DefaultMaxAttempts || p.TimeoutSecs != DefaultTimeoutSecs {
		t.Fatalf("missing defaults: %+v", p)
	}
	if _, err := (NewRunParams{
		WorkloadKind:  SyntheticEchoKind,
		WorkloadInput: map[string]any{"message": "hi"}, MaxAttempts: MaxAttemptsLimit + 1,
	}).Validate(); err == nil {
		t.Fatal("expected maxAttempts rejection")
	}
}

func TestRunTerminalStates(t *testing.T) {
	for _, s := range []RunStatus{RunSucceeded, RunFailed, RunCancelled} {
		if !s.Terminal() {
			t.Fatalf("%s should be terminal", s)
		}
	}
	for _, s := range []RunStatus{RunQueued, RunLeased, RunRunning} {
		if s.Terminal() {
			t.Fatalf("%s should not be terminal", s)
		}
	}
}

func TestValidateWorkerID(t *testing.T) {
	if err := ValidateWorkerID("local-1"); err != nil {
		t.Fatalf("valid id rejected: %v", err)
	}
	for _, bad := range []string{"", "UPPER", "with space", "with/slash", string(make([]byte, MaxWorkerIDLength+1))} {
		if err := ValidateWorkerID(bad); err == nil {
			t.Fatalf("expected rejection for %q", bad)
		}
	}
}

func TestSanitizeResultBounds(t *testing.T) {
	tooMany := map[string]any{}
	for i := 0; i < 29; i++ {
		tooMany[fmt.Sprintf("k%d", i)] = "v"
	}
	if _, err := SanitizeResult(tooMany); err == nil {
		t.Fatal("expected key-count rejection")
	}
	if _, err := SanitizeResult(map[string]any{"ok": true, "count": 2}); err != nil {
		t.Fatalf("valid result rejected: %v", err)
	}
}
