package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/paseo"
)

// paseoEvidence is the task-specific proof collected from the Attempt
// workspace and the session timeline. Only counts, exit statuses and a
// bounded diff stat cross into Factory evidence: transcript bodies, prompts,
// source file contents and raw tool output never do.
type paseoEvidence struct {
	timelineLines int
	toolSteps     int
	stepsSummary  string
	diffStat      string
	diffBytes     int
	testExit      int
	formatExit    int
}

// paseoTaskPrompt wraps the operator task in the containment instructions
// proven by the trusted-local smoke test: absolute target directory,
// in-scope edits only, format/tests, short reply, no network or secrets.
func paseoTaskPrompt(task, workDir string, run domain.Run, spec *domain.ActionSpec) string {
	browserHint := ""
	if spec != nil && domain.ContainsCapability(spec.Capabilities, domain.CapBrowser) {
		browserHint = " Factory has granted this Run the browser capability. "
		if spec.BrowserTestServer != nil && len(spec.BrowserEndpoints) > 0 {
			browserHint += "The authorized test server is already managed and running at " +
				spec.BrowserEndpoints[0] + "; do not start, restart, or kill it. "
		} else {
			browserHint += "Start the app on an authorized endpoint and keep server logs outside the repository. "
		}
		browserHint += "Use the project skill .agents/skills/factory-browser/SKILL.md and the wrapper " +
			".factory/bin/factory-playwright-cli.mjs. Authorized local endpoints are exactly: " +
			strings.Join(spec.BrowserEndpoints, ", ") + ". MANDATORY: run " +
			"`node .factory/bin/factory-playwright-cli.mjs open " + firstEndpoint(spec.BrowserEndpoints) + "/` " +
			"followed by at least one successful observation/interaction command such as `snapshot`, `find`, " +
			"`fill`, or `click`. The Quality Gate rejects a Run that only invoked `--help` or attempted commands " +
			"without success. Do not edit .agents/ or .factory/ and do not use any other browser target. " +
			"Playwright CLI invocation outcomes are recorded as capability evidence."
	}
	return "Your target directory is exactly: " + workDir + ". It is an authorized " +
		"repository checkout with its own package.json scripts. Task: " + task + " " +
		"Read the existing files first. Modify ONLY files explicitly authorized by " +
		"the task; never modify tests, protected verification scripts (including " +
		"browser-check.mjs), package.json, or .git. Use absolute paths under " +
		workDir + " and do not create, modify, or delete anything outside it. " +
		"Run `npm --prefix " + workDir + " run format` and `npm --prefix " + workDir +
		" test` when present and fix failures you caused. If a protected " +
		"browser-check.mjs is present and browser tooling is available, you may run " +
		"it as read-only evidence; never edit it. Keep your final reply under 10 " +
		"lines listing files changed and test results. Do not access the network, " +
		"secrets, or credentials." + browserHint
}

// paseoNudgePrompt is the single bounded continuation used when the agent
// acted with tools but left no diff.
func paseoNudgePrompt(workDir string) string {
	return "Continue exactly where you left off in " + workDir + ": finish the edit now, " +
		"run format and tests, keep your reply under 10 lines."
}

// collectPaseoEvidence observes the session timeline and verifies
// task-specific proof in workDir: a real git diff plus passing
// format/tests. A missing diff is an error (the caller may nudge once);
// failing checks are an error (failed task, never success).
func collectPaseoEvidence(
	ctx context.Context,
	adapter paseo.Adapter,
	workDir string,
	identity paseo.Identity,
) (paseo.TimelineSummary, paseoEvidence, error) {
	var ev paseoEvidence
	ev.testExit = -1
	ev.formatExit = -1
	summary, err := adapter.ObserveTimeline(ctx, identity.SessionID)
	if err != nil {
		return summary, ev, fmt.Errorf("observe timeline: %w", err)
	}
	ev.timelineLines = summary.Lines
	ev.toolSteps = summary.ToolSteps
	ev.stepsSummary = formatSteps(summary.Steps)

	diffStat, diffBytes, derr := gitDiffEvidence(ctx, workDir)
	if derr != nil {
		return summary, ev, derr
	}
	ev.diffStat = diffStat
	ev.diffBytes = diffBytes
	if diffBytes == 0 {
		return summary, ev, errors.New("no-source-change: agent left no git diff")
	}

	testScript, formatScript, hasPkg := readPkgScripts(workDir)
	if !hasPkg {
		// Generic repository without a Node harness: the diff plus tool
		// activity is the evidence. Acceptance fixtures always seed a
		// package.json, so this path stays exceptional, never the norm.
		if summary.ToolSteps == 0 {
			return summary, ev, errors.New("no-source-change: diff without tool activity")
		}
		return summary, ev, nil
	}
	if formatScript {
		code, cerr := runCheck(ctx, workDir, "npm", "run", "format", "--prefix", workDir)
		ev.formatExit = code
		if cerr != nil {
			return summary, ev, fmt.Errorf("formatter failed: %w", cerr)
		}
	} else {
		ev.formatExit = -1
	}
	if testScript {
		code, cerr := runCheck(ctx, workDir, "npm", "test", "--prefix", workDir)
		ev.testExit = code
		if cerr != nil {
			return summary, ev, fmt.Errorf("tests failed: %w", cerr)
		}
	} else {
		ev.testExit = -1
	}
	return summary, ev, nil
}

// formatSteps renders step counts as a bounded "Name:n,..." summary.
func formatSteps(steps map[string]int) string {
	if len(steps) == 0 {
		return "(none)"
	}
	names := make([]string, 0, len(steps))
	for n := range steps {
		names = append(names, n)
	}
	sort.Strings(names)
	parts := make([]string, 0, len(names))
	for _, n := range names {
		parts = append(parts, fmt.Sprintf("%s:%d", n, steps[n]))
	}
	out := strings.Join(parts, ",")
	if len(out) > 256 {
		out = out[:256]
	}
	return out
}

// gitDiffEvidence returns the workDir diff stat and byte size. The diff
// body is deliberately not retained: S3-C evidence is bounded metadata, not
// source contents. An empty diff also checks git status so
// created-but-uncommitted files are reported honestly instead of mistaken
// for a clean tree.
func gitDiffEvidence(ctx context.Context, workDir string) (string, int, error) {
	ctx, cancel := context.WithTimeout(ctx, paseoCmdTimeout)
	defer cancel()
	diff, err := gitOutput(ctx, workDir, "diff")
	if err != nil {
		return "", 0, fmt.Errorf("git diff: %w", err)
	}
	if len(bytes.TrimSpace(diff)) == 0 {
		status, serr := gitOutput(ctx, workDir, "status", "--short")
		if serr != nil {
			return "", 0, fmt.Errorf("git status: %w", serr)
		}
		if len(bytes.TrimSpace(status)) > 0 {
			lines := strings.Split(strings.TrimSpace(string(status)), "\n")
			if len(lines) > 10 {
				lines = lines[:10]
			}
			return "untracked:" + strings.Join(lines, ","), 0, nil
		}
		return "", 0, nil
	}
	stat, err := gitOutput(ctx, workDir, "diff", "--stat")
	if err != nil {
		return "", 0, fmt.Errorf("git diff --stat: %w", err)
	}
	statLine := strings.TrimSpace(string(stat))
	if len(statLine) > 512 {
		statLine = statLine[:512]
	}
	return statLine, len(diff), nil
}

func gitOutput(ctx context.Context, workDir string, args ...string) ([]byte, error) {
	cmd := exec.CommandContext(ctx, "git", append([]string{"-C", workDir}, args...)...)
	cmd.Env = []string{"PATH=" + os.Getenv("PATH"), "HOME=" + os.Getenv("HOME")}
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		return nil, errors.New("git " + strings.Join(args, " ") + ": exit nonzero")
	}
	out := stdout.Bytes()
	if len(out) > 1<<20 {
		return nil, errors.New("git output exceeded 1MiB bound")
	}
	return out, nil
}

// readPkgScripts reports whether workDir holds a package.json and which of
// the format/test scripts exist.
func readPkgScripts(workDir string) (test, format, hasPkg bool) {
	raw, err := os.ReadFile(filepath.Join(workDir, "package.json"))
	if err != nil {
		return false, false, false
	}
	var pkg struct {
		Scripts map[string]string `json:"scripts"`
	}
	if err := json.Unmarshal(raw, &pkg); err != nil {
		return false, false, false
	}
	_, test = pkg.Scripts["test"]
	_, format = pkg.Scripts["format"]
	return test, format, true
}

// runCheck runs one verification command with a hard timeout and returns
// its exit code. Output bodies are discarded: only the exit status is
// evidence, so tool output can never leak into Factory records.
func runCheck(ctx context.Context, dir, name string, args ...string) (int, error) {
	ctx, cancel := context.WithTimeout(ctx, paseoTestTimeout)
	defer cancel()
	cmd := exec.CommandContext(ctx, name, args...)
	cmd.Dir = dir
	cmd.Env = []string{"PATH=" + os.Getenv("PATH"), "HOME=" + os.Getenv("HOME")}
	var sink bytes.Buffer
	cmd.Stdout = &sink
	cmd.Stderr = &sink
	err := cmd.Run()
	if ctx.Err() != nil {
		return -1, errors.New(name + " timed out or cancelled")
	}
	if err != nil {
		var exitErr *exec.ExitError
		if errors.As(err, &exitErr) {
			return exitErr.ExitCode(), fmt.Errorf("%s exited %d", name, exitErr.ExitCode())
		}
		return -1, errors.New(name + ": failed to start")
	}
	return 0, nil
}

// browserExecutionEvidence reports what the Factory-managed wrapper actually
// observed. An empty log is reported as an unproven invocation, never as a
// model self-report.
func browserExecutionEvidence(workDir string) map[string]any {
	out := map[string]any{"invoked": false, "invocations": 0, "commands": []string{}}
	skillPath := filepath.Join(workDir, ".agents", "skills", "factory-browser", "SKILL.md")
	if raw, err := os.ReadFile(skillPath); err == nil {
		sum := sha256.Sum256(raw)
		out["skillDigest"] = hex.EncodeToString(sum[:])
	}
	if raw, err := os.ReadFile(filepath.Join(workDir, ".factory", "browser-runtime.json")); err == nil {
		var runtime map[string]any
		if json.Unmarshal(raw, &runtime) == nil {
			out["cli"] = runtime["cli"]
			out["cliVersion"] = runtime["cliVersion"]
			out["endpoints"] = runtime["endpoints"]
		}
	}
	raw, err := os.ReadFile(filepath.Join(workDir, ".factory", "bin", "browser-invocations.jsonl"))
	if err != nil {
		return out
	}
	commands := make([]string, 0, 4)
	successful := make([]string, 0, 4)
	count := 0
	successCount := 0
	for _, line := range strings.Split(strings.TrimSpace(string(raw)), "\n") {
		if strings.TrimSpace(line) == "" {
			continue
		}
		var entry struct {
			Command  string `json:"command"`
			ExitCode int    `json:"exitCode"`
			OK       bool   `json:"ok"`
		}
		if json.Unmarshal([]byte(line), &entry) != nil {
			continue
		}
		count++
		if len(commands) < 12 && entry.Command != "" {
			commands = append(commands, entry.Command)
		}
		if entry.OK && entry.ExitCode == 0 {
			successCount++
			if len(successful) < 12 && entry.Command != "" {
				successful = append(successful, entry.Command)
			}
		}
	}
	out["invoked"] = count > 0
	out["invocations"] = count
	out["successfulInvocations"] = successCount
	out["commands"] = commands
	out["successfulCommands"] = successful
	return out
}

// seedPaseoFixture initializes the Attempt workspace with the disposable
// Node baseline when it is empty: package.json (format/test scripts),
// add.js, add.test.js, git repo with one commit. Non-empty workspaces are
// left untouched; a missing git binary fails honestly.
func seedPaseoFixture(ctx context.Context, workDir string) error {
	entries, err := os.ReadDir(workDir)
	if err != nil {
		return fmt.Errorf("read workspace: %w", err)
	}
	if len(entries) > 0 {
		return nil
	}
	if _, err := exec.LookPath("git"); err != nil {
		return errors.New("git is required for paseo-task workspaces")
	}
	files := map[string]string{
		"package.json": `{
  "name": "factory-paseo-task",
  "version": "1.0.0",
  "private": true,
  "scripts": {
    "test": "node --test add.test.js",
    "format": "node --check add.js && node --check add.test.js"
  }
}
`,
		"add.js": `/** Add two numbers. */
function add(a, b) {
  return a + b;
}
module.exports = { add };
`,
		"add.test.js": `const test = require("node:test");
const assert = require("node:assert");
const { add } = require("./add.js");
test("add works", () => {
  assert.strictEqual(add(2, 3), 5);
});
`,
	}
	for name, content := range files {
		if err := os.WriteFile(filepath.Join(workDir, name), []byte(content), 0o600); err != nil {
			return fmt.Errorf("seed %s: %w", name, err)
		}
	}
	ctx, cancel := context.WithTimeout(ctx, paseoCmdTimeout)
	defer cancel()
	git := func(args ...string) error {
		cmd := exec.CommandContext(ctx, "git", args...)
		cmd.Dir = workDir
		cmd.Env = []string{"PATH=" + os.Getenv("PATH"), "HOME=" + os.Getenv("HOME")}
		if out, err := cmd.CombinedOutput(); err != nil {
			return fmt.Errorf("git %s: %s", strings.Join(args, " "), truncate(string(out)))
		}
		return nil
	}
	if err := git("init", "-q"); err != nil {
		return err
	}
	if err := git("-c", "user.email=worker@factory.local", "-c", "user.name=factory-worker", "add", "."); err != nil {
		return err
	}
	if err := git("-c", "user.email=worker@factory.local", "-c", "user.name=factory-worker", "commit", "-qm", "factory paseo-task baseline"); err != nil {
		return err
	}
	return nil
}

// browserCapabilityComplete reports whether the Factory-managed wrapper
// recorded a successful open plus a successful observation/interaction.
// Failed or help-only commands never count.
func browserCapabilityComplete(workDir string) bool {
	evidence := browserExecutionEvidence(workDir)
	commands, _ := evidence["successfulCommands"].([]string)
	seen := map[string]bool{}
	for _, command := range commands {
		seen[strings.TrimSpace(strings.ToLower(command))] = true
	}
	if !seen["open"] {
		return false
	}
	for _, command := range []string{"snapshot", "goto", "find", "fill", "click", "eval", "screenshot", "select", "type", "press", "reload"} {
		if seen[command] {
			return true
		}
	}
	return false
}

// capabilityNudgePrompt is the single bounded same-session continuation
// used when a productive Implementer omitted a mandatory browser
// interaction. It does not grant new scope or authority.
func capabilityNudgePrompt(workDir string, endpoints []string, managedServer bool) string {
	prerequisite := "The authorized test server is already managed and running; do not start or restart it. "
	if !managedServer {
		prerequisite = "Start the authorized app if needed and keep server logs outside the repository. "
	}
	return "Factory capability check: the code change is present, but this Run has no successful " +
		"browser interaction evidence yet. " + prerequisite + "Run " +
		"`node .factory/bin/factory-playwright-cli.mjs open " + strings.TrimRight(firstEndpoint(endpoints), "/") + "/` " +
		"followed by at least one successful `snapshot`, `find`, `fill`, or `click` command against an " +
		"authorized loopback endpoint. `--help` alone or failed commands are not acceptance evidence. " +
		"Stay inside " + workDir + ", do not edit tests, protected browser-check.mjs, package.json, .agents/ or " +
		".factory/, and reply briefly with the observed result."
}

func firstEndpoint(endpoints []string) string {
	if len(endpoints) > 0 {
		return endpoints[0]
	}
	return "http://127.0.0.1:4173"
}
