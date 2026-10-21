package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

// runtimeCapabilities is the truthful capability observation for this
// Worker process. A capability is advertised only when its executable
// tooling is present; a missing tool is never replaced by a prompt claim.
type runtimeCapabilities struct {
	Capabilities  []domain.Capability
	Browser       browserRuntime
	PlaywrightCLI string
}

type browserRuntime struct {
	CLIPath      string
	CLIVersion   string
	ChromiumPath string
	CorePath     string
}

func (r browserRuntime) Available() bool {
	if r.CLIPath == "" || r.ChromiumPath == "" || r.CorePath == "" {
		return false
	}
	if info, err := os.Stat(r.CLIPath); err != nil || info.IsDir() {
		return false
	}
	if info, err := os.Stat(r.ChromiumPath); err != nil || info.IsDir() {
		return false
	}
	if info, err := os.Stat(r.CorePath); err != nil || !info.IsDir() {
		return false
	}
	return true
}

// detectRuntimeCapabilities probes only tools this Worker can actually
// invoke. Node is required for the Node quality profile; browser requires
// the pinned CLI, Chromium and playwright-core paths.
func detectRuntimeCapabilities(ctx context.Context) (runtimeCapabilities, error) {
	result := runtimeCapabilities{}
	if _, err := exec.LookPath("git"); err == nil {
		result.Capabilities = append(result.Capabilities, domain.CapRepositoryEdit)
	}
	if _, err := exec.LookPath("node"); err == nil {
		result.Capabilities = append(result.Capabilities, domain.CapNodeToolchain)
	}
	if _, err := exec.LookPath("make"); err == nil {
		result.Capabilities = append(result.Capabilities, domain.CapGenericToolchain)
	}
	browser := browserRuntime{
		CLIPath:      strings.TrimSpace(os.Getenv("FACTORY_BROWSER_CLI")),
		ChromiumPath: strings.TrimSpace(os.Getenv("CHROMIUM_EXE")),
		CorePath:     strings.TrimSpace(os.Getenv("FACTORY_PLAYWRIGHT_CORE")),
	}
	if browser.Available() {
		probe, cancel := context.WithTimeout(ctx, 20*time.Second)
		defer cancel()
		if out, err := exec.CommandContext(probe, browser.CLIPath, "--version").CombinedOutput(); err == nil {
			browser.CLIVersion = strings.TrimSpace(string(out))
		}
		result.Browser = browser
		result.PlaywrightCLI = browser.CLIPath
		result.Capabilities = append(result.Capabilities, domain.CapBrowser)
	}
	if len(result.Capabilities) == 0 {
		return result, fmt.Errorf("worker exposes no supported capability")
	}
	return result, nil
}

// prepareBrowserCapability writes the Factory browser skill and a narrow
// wrapper into the disposable checkout. Both live under paths excluded from
// the candidate commit; the skill's described workflow is instruction-level
// scoping, not OS isolation.
func prepareBrowserCapability(ctx context.Context, workDir, runID string, spec *domain.ActionSpec, browser browserRuntime) (string, error) {
	if spec == nil || !domain.ContainsCapability(spec.Capabilities, domain.CapBrowser) {
		return "", nil
	}
	if !browser.Available() {
		return "", fmt.Errorf("browser capability is required but Playwright CLI/Chromium/playwright-core tooling is unavailable")
	}
	if err := excludeRuntimePaths(ctx, workDir); err != nil {
		return "", err
	}
	skillPath := filepath.Join(workDir, ".agents", "skills", "factory-browser", "SKILL.md")
	if err := os.MkdirAll(filepath.Dir(skillPath), 0o700); err != nil {
		return "", fmt.Errorf("create browser skill directory: %w", err)
	}
	wrapperPath := filepath.Join(workDir, ".factory", "bin", "factory-playwright-cli.mjs")
	if err := os.MkdirAll(filepath.Dir(wrapperPath), 0o700); err != nil {
		return "", fmt.Errorf("create browser wrapper directory: %w", err)
	}
	session := "factory-" + runID
	configPath := filepath.Join(workDir, ".factory", "playwright-cli.config.json")
	config := map[string]any{
		"browser": map[string]any{
			"browserName": "chromium",
			"launchOptions": map[string]any{
				"executablePath": browser.ChromiumPath,
				"headless":       true,
				"args":           []string{"--no-sandbox", "--disable-dev-shm-usage"},
			},
		},
	}
	configRaw, _ := json.MarshalIndent(config, "", "  ")
	if err := os.WriteFile(configPath, configRaw, 0o600); err != nil {
		return "", fmt.Errorf("write playwright CLI config: %w", err)
	}
	wrapper := browserWrapper(session, browser.CLIPath, spec.BrowserEndpoints, configPath,
		filepath.Join(workDir, ".factory", "browser-home"))
	if err := os.WriteFile(wrapperPath, []byte(wrapper), 0o700); err != nil {
		return "", fmt.Errorf("write browser wrapper: %w", err)
	}
	skill := browserSkill(wrapperPath, browser, spec.BrowserEndpoints)
	if err := os.WriteFile(skillPath, []byte(skill), 0o600); err != nil {
		return "", fmt.Errorf("write browser skill: %w", err)
	}
	runtimeJSON, _ := json.Marshal(map[string]any{
		"cli": browser.CLIPath, "cliVersion": browser.CLIVersion,
		"chromium": browser.ChromiumPath, "playwrightCore": browser.CorePath,
		"endpoints": spec.BrowserEndpoints,
	})
	if err := os.WriteFile(filepath.Join(workDir, ".factory", "browser-runtime.json"), runtimeJSON, 0o600); err != nil {
		return "", fmt.Errorf("write browser runtime evidence: %w", err)
	}
	return skillPath, nil
}

func excludeRuntimePaths(ctx context.Context, workDir string) error {
	out, err := exec.CommandContext(ctx, "git", "-C", workDir, "rev-parse", "--git-path", "info/exclude").Output()
	if err != nil {
		return fmt.Errorf("resolve git exclude path: %w", err)
	}
	path := strings.TrimSpace(string(out))
	if !filepath.IsAbs(path) {
		path = filepath.Join(workDir, path)
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return err
	}
	existing, err := os.ReadFile(path)
	if err != nil && !os.IsNotExist(err) {
		return err
	}
	content := string(existing)
	for _, entry := range []string{
		".agents/", ".factory/", ".playwright-cli/",
		"*.log", "node_modules/", "coverage/", ".cache/", "dist/", "build/",
	} {
		if !strings.Contains(content, entry) {
			content += "\n" + entry + "\n"
		}
	}
	return os.WriteFile(path, []byte(content), 0o600)
}

func browserSkill(wrapper string, browser browserRuntime, endpoints []string) string {
	allowed := strings.Join(endpoints, ", ")
	return fmt.Sprintf(`---
name: factory-browser
description: Exercise the authorized local web application with the pinned Factory Playwright CLI. Use only for implementer self-tests inside this Factory Run; it is not independent acceptance evidence.
allowed-tools: Bash(%s:*)
---

# Factory-managed browser capability

Use exactly this wrapper: %s

- Authorized local endpoints: %s
- Chromium: %s
- playwright-core: %s
- The wrapper runs the pinned Playwright CLI (%s) with a disposable,
  run-scoped session and records bounded invocation evidence.
- Only loopback URLs inside the authorized endpoints are accepted. Never
  access the network, credentials, cookies, production data or other
  projects.
- Never edit .agents/, .factory/, protected acceptance scripts, tests or
  package manifests while acting as the Implementer.
- Start the application on one of the authorized endpoints yourself if it is
  not already running. Write server logs outside the repository (for example
  under /tmp) so runtime artifacts never become candidate changes. Exercise
  the real UI, then close the browser.
- A screenshot or CLI exit alone is not PASS: assert the observable UI state
  and report concrete results. Independent verification is performed later
  by Factory's Verifier, not by you.
`, wrapper, wrapper, allowed, browser.ChromiumPath, browser.CorePath,
		browser.CLIVersion)
}

// browserWrapper is a generated, disposable Node guard around the pinned
// CLI. It inserts a run-scoped session name, rejects non-loopback HTTP
// targets and records only bounded invocation metadata.
func browserWrapper(session, cli string, endpoints []string, config, home string) string {
	rawEndpoints, _ := json.Marshal(endpoints)
	return fmt.Sprintf(`#!/usr/bin/env node
// Generated by Factory for one disposable Run. Not a security boundary.
import { spawn } from "node:child_process";
import { appendFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
const CLI = %q;
const SESSION = %q;
const ENDPOINTS = %s;
const CONFIG = %q;
const HOME = %q;
const log = new URL("./browser-invocations.jsonl", import.meta.url).pathname;
const args = process.argv.slice(2);
mkdirSync(HOME, { recursive: true });
const urlArgs = args.filter((a) => /^https?:\/\//i.test(a));
for (const raw of urlArgs) {
  let parsed;
  try { parsed = new URL(raw); } catch { console.error("invalid URL"); process.exit(64); }
  const host = parsed.hostname;
  const allowedHost = host === "127.0.0.1" || host === "localhost" || host === "::1";
  const allowedEndpoint = ENDPOINTS.some((e) => raw === e || raw.startsWith(e.replace(/\/$/, "") + "/"));
  if (parsed.protocol !== "http:" || !allowedHost || !allowedEndpoint) {
    console.error("browser target is not an authorized loopback endpoint");
    process.exit(64);
  }
}
mkdirSync(dirname(log), { recursive: true });
appendFileSync(log, JSON.stringify({ at: new Date().toISOString(), command: args[0] || "", argc: args.length, targetCount: urlArgs.length }) + "\n");
const effective = args[0] === "open" ? [args[0], "--config", CONFIG, ...args.slice(1)] : args;
const child = spawn(CLI, ["-s=" + SESSION, ...effective], {
  stdio: "inherit",
  env: { ...process.env, HOME },
});
child.on("exit", (code) => process.exit(code ?? 1));
`, cli, session, string(rawEndpoints), config, home)
}

// cleanupBrowserSession closes the run-scoped browser and removes the
// disposable profile. Failures are returned for truthful evidence, never
// promoted to success.
func cleanupBrowserSession(ctx context.Context, workDir string) error {
	wrapper := filepath.Join(workDir, ".factory", "bin", "factory-playwright-cli.mjs")
	if info, err := os.Stat(wrapper); err != nil || info.IsDir() {
		return nil
	}
	runCtx, cancel := context.WithTimeout(ctx, 45*time.Second)
	defer cancel()
	cmd := exec.CommandContext(runCtx, "node", wrapper, "close")
	cmd.Dir = workDir
	if out, err := cmd.CombinedOutput(); err != nil {
		return fmt.Errorf("browser close: %s", boundedOutput(string(out)))
	}
	_ = os.RemoveAll(filepath.Join(workDir, ".factory", "browser-home"))
	_ = os.RemoveAll(filepath.Join(workDir, ".playwright-cli"))
	return nil
}

func boundedOutput(value string) string {
	value = strings.TrimSpace(value)
	if len(value) > 240 {
		return value[:240]
	}
	return value
}
