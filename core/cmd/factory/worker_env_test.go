package main

import (
	"strings"
	"testing"
)

// Synthetic canaries only: these values never touch real credentials, they
// prove the Worker environment builder drops secret-shaped input.
func TestWorkerEnvironmentDropsSecrets(t *testing.T) {
	canaries := map[string]string{
		"OPENAI_API_KEY":     "sk-canary-openai",
		"CODEX_API_KEY":      "canary-codex",
		"GITHUB_TOKEN":       "ghp_canary",
		"GH_TOKEN":           "gho_canary",
		"FACTORY_API_TOKEN":  "operator-canary",
		"AWS_SECRET_KEY":     "aws-canary",
		"GOOGLE_CREDENTIALS": "google-canary",
		"PASEO_SESSION":      "paseo-canary",
	}
	for key, value := range canaries {
		t.Setenv(key, value)
	}
	t.Setenv("PATH", "/usr/bin:/bin")
	t.Setenv("HOME", "/tmp/s2-home")
	t.Setenv("XDG_RUNTIME_DIR", "/run/user/1000")
	t.Setenv("TMPDIR", "/tmp")

	env := workerEnvironment("worker-canary")
	joined := strings.Join(env, "\n")
	for key, value := range canaries {
		if strings.Contains(joined, key) || strings.Contains(joined, value) {
			t.Fatalf("worker environment propagates %s", key)
		}
	}
	want := map[string]string{
		"PATH":                 "/usr/bin:/bin",
		"HOME":                 "/tmp/s2-home",
		"XDG_RUNTIME_DIR":      "/run/user/1000",
		"TMPDIR":               "/tmp",
		"FACTORY_WORKER_TOKEN": "worker-canary",
	}
	got := map[string]string{}
	for _, item := range env {
		key, value, _ := strings.Cut(item, "=")
		got[key] = value
	}
	for key, value := range want {
		if got[key] != value {
			t.Fatalf("env[%s] = %q, want %q (full env: %q)", key, got[key], value, env)
		}
	}
	if len(env) != len(want) {
		t.Fatalf("worker environment has %d entries, want exactly %d: %q", len(env), len(want), env)
	}
}

func TestWorkerEnvironmentFromIgnoresEmpty(t *testing.T) {
	parent := []string{"PATH=", "HOME=/home/op", "UNRELATED=yes"}
	env := workerEnvironmentFrom(parent, "wt")
	joined := strings.Join(env, "\n")
	if strings.Contains(joined, "PATH=") {
		t.Fatalf("empty allowed value must be dropped: %q", env)
	}
	if !strings.Contains(joined, "HOME=/home/op") || !strings.Contains(joined, "FACTORY_WORKER_TOKEN=wt") {
		t.Fatalf("kept values missing: %q", env)
	}
	if strings.Contains(joined, "UNRELATED") {
		t.Fatalf("unlisted value propagated: %q", env)
	}
}
