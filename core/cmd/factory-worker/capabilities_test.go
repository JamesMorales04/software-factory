package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func writeInvocationLog(t *testing.T, dir, contents string) {
	t.Helper()
	bin := filepath.Join(dir, ".factory", "bin")
	if err := os.MkdirAll(bin, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(bin, "browser-invocations.jsonl"), []byte(contents), 0o600); err != nil {
		t.Fatal(err)
	}
}

func TestBrowserCapabilityCompleteRequiresSuccessfulOpenAndInteraction(t *testing.T) {
	cases := []struct {
		name string
		log  string
		want bool
	}{
		{"success", `{"command":"open","exitCode":0,"ok":true}` + "\n" + `{"command":"snapshot","exitCode":0,"ok":true}` + "\n", true},
		{"help only", `{"command":"--help","exitCode":0,"ok":true}` + "\n", false},
		{"open only", `{"command":"open","exitCode":0,"ok":true}` + "\n", false},
		{"failed open", `{"command":"open","exitCode":1,"ok":false}` + "\n" + `{"command":"snapshot","exitCode":0,"ok":true}` + "\n", false},
		{"failed interaction", `{"command":"open","exitCode":0,"ok":true}` + "\n" + `{"command":"snapshot","exitCode":1,"ok":false}` + "\n", false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			dir := t.TempDir()
			writeInvocationLog(t, dir, tc.log)
			if got := browserCapabilityComplete(dir); got != tc.want {
				t.Fatalf("browserCapabilityComplete = %v, want %v", got, tc.want)
			}
		})
	}
}

func TestCapabilityNudgePromptIsBoundedAndSpecific(t *testing.T) {
	got := capabilityNudgePrompt("/tmp/work", []string{"http://127.0.0.1:4173"}, false)
	for _, want := range []string{"open http://127.0.0.1:4173/", "snapshot", "/tmp/work", "do not edit tests", "protected browser-check.mjs", "Start the authorized app"} {
		if !strings.Contains(got, want) {
			t.Fatalf("capability nudge missing %q: %s", want, got)
		}
	}
	if strings.Contains(got, "http://example.com") {
		t.Fatal("capability nudge invented an endpoint")
	}
	managed := capabilityNudgePrompt("/tmp/work", []string{"http://127.0.0.1:4173"}, true)
	if !strings.Contains(managed, "already managed and running") || strings.Contains(managed, "Start the authorized app") {
		t.Fatalf("managed-server nudge is not actionable: %s", managed)
	}
}

func TestBrowserSkillManagedServerInstructions(t *testing.T) {
	browser := browserRuntime{ChromiumPath: "/chromium", CorePath: "/playwright-core", CLIVersion: "0.1.22"}
	managed := browserSkill("/work/.factory/bin/factory-playwright-cli.mjs", browser, []string{"http://127.0.0.1:4173"}, true)
	for _, want := range []string{"already managed and running", "Do not start, restart, or kill it"} {
		if !strings.Contains(managed, want) {
			t.Fatalf("managed skill missing %q", want)
		}
	}
	if strings.Contains(managed, "Start the application on one of the authorized endpoints yourself") {
		t.Fatal("managed skill still tells the model to start the server")
	}
	unmanaged := browserSkill("/work/.factory/bin/factory-playwright-cli.mjs", browser, []string{"http://127.0.0.1:4173"}, false)
	if !strings.Contains(unmanaged, "Start the application on one of the authorized endpoints yourself") {
		t.Fatal("unmanaged skill lost its startup instruction")
	}
}
