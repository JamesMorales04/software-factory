#!/usr/bin/env node
// Agent-native browser capability for Factory engineering agents.
//
// This drives the pinned Microsoft `playwright-cli` (office/e2e-browser
// package) through real browser interactions against the running Factory
// Office: unlock, navigate, create an Action through the form, open its
// detail, capture console/network diagnostics and a bounded screenshot, and
// verify the resulting state against the Core API.
//
// It is an Implementer/development capability, not independent evidence.
// The Independent Verifier uses its own clean checkout and protected oracle.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..");
const CLI = join(ROOT, "office", "e2e-browser", "node_modules", ".bin", "playwright-cli");
const BASE = (process.env.BASE_URL || "http://127.0.0.1:8480").replace(/\/$/, "");
const TOKEN = process.env.FACTORY_API_TOKEN || "";
const CHROMIUM = process.env.CHROMIUM_EXE || "";
const EVIDENCE_DIR =
  process.env.S4A_BROWSER_EVIDENCE_DIR || join(HERE, "..", "tmp", "agent-browser");
const CANDIDATE_ID = process.env.S4A_CANDIDATE_ID || "";
const TITLE = process.env.S4A_BROWSER_ACTION_TITLE || `Agent browser ${Date.now()}`;

if (!TOKEN) {
  console.log(JSON.stringify({ ok: false, error: "FACTORY_API_TOKEN required" }));
  process.exit(1);
}
if (!CHROMIUM || !existsSync(CHROMIUM)) {
  console.log(
    JSON.stringify({ ok: false, error: "CHROMIUM_EXE must point to Chromium" }),
  );
  process.exit(1);
}
if (!existsSync(CLI)) {
  console.log(
    JSON.stringify({ ok: false, error: "pinned playwright-cli is not installed" }),
  );
  process.exit(1);
}
mkdirSync(EVIDENCE_DIR, { recursive: true });
const workspace = mkdtempSync(join(tmpdir(), "factory-agent-browser-"));
const configPath = join(workspace, "cli.config.json");
writeFileSync(
  configPath,
  JSON.stringify({
    browser: {
      browserName: "chromium",
      launchOptions: {
        executablePath: CHROMIUM,
        headless: true,
        args: ["--no-sandbox", "--disable-dev-shm-usage"],
      },
    },
  }),
);

const checks = [];
function check(name, pass, detail = "") {
  checks.push({ name, pass, ...(detail ? { detail } : {}) });
  if (!pass) throw new Error(`${name}${detail ? `: ${detail}` : ""}`);
}

function childEnv() {
  const env = { ...process.env };
  for (const key of Object.keys(env)) {
    if (/TOKEN/i.test(key)) delete env[key];
  }
  env.HOME = workspace;
  return env;
}

function cli(args, { timeout = 60000 } = {}) {
  const result = spawnSync(CLI, args, {
    cwd: workspace,
    env: childEnv(),
    encoding: "utf8",
    timeout,
    maxBuffer: 4 * 1024 * 1024,
  });
  if (result.error) throw result.error;
  const combined = `${result.stdout || ""}\n${result.stderr || ""}`;
  if (/Daemon process exited|Error:/i.test(combined)) {
    throw new Error(
      `playwright-cli ${args[0]} failed: ${combined.slice(0, 500)}`,
    );
  }
  if (result.status !== 0) {
    throw new Error(
      `playwright-cli ${args[0]} exited ${result.status}: ${(result.stderr || result.stdout || "").slice(0, 500)}`,
    );
  }
  return result.stdout || "";
}

function find(text, timeoutMs = 15000) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    try {
      const output = cli(["find", text], { timeout: 15000 });
      if (!/No matches found/i.test(output) && output.toLowerCase().includes(text.toLowerCase())) {
        return output;
      }
    } catch (e) {
      if (Date.now() >= deadline) throw e;
    }
    if (Date.now() >= deadline) {
      throw new Error(`timed out waiting for page text: ${text}`);
    }
    spawnSync("sleep", ["0.3"]);
  }
}

async function api(path, init = {}) {
  const response = await fetch(BASE + path, {
    ...init,
    headers: {
      Authorization: `Bearer ${TOKEN}`,
      "Content-Type": "application/json",
      ...init.headers,
    },
    signal: AbortSignal.timeout(20000),
  });
  if (!response.ok) throw new Error(`API ${path} returned HTTP ${response.status}`);
  return response.json();
}

function failedRequests(output) {
  return output
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => /=> \[(4\d\d|5\d\d)\]/.test(line) && !line.includes("favicon"));
}

function consoleErrorCount(output) {
  const match = /Errors:\s*(\d+)/i.exec(output);
  return match ? Number(match[1]) : 0;
}

let ok = false;
let failure = "";
let evidence = {};
try {
  const version = cli(["--version"]).trim();
  cli(["open", "--config", configPath, BASE + "/"], { timeout: 60000 });
  find("Factory Office");
  check("real Chromium opened the Factory Office token gate", true);

  cli(["fill", 'input[placeholder="FACTORY_API_TOKEN"]', TOKEN]);
  cli(["click", 'button[type="submit"]']);
  find("Dashboard");
  check("agent unlocked Office and reached Dashboard", true);

  cli(["goto", BASE + "/#/actions"]);
  find("New Action");
  cli(["fill", 'input[placeholder="What should the Factory do?"]', TITLE]);
  cli(["fill", 'textarea[placeholder^="Acceptance sketch"]', "Agent browser acceptance"]);
  cli(["click", 'button:has-text("Create Action")']);
  find("Created");
  check("agent created an Action through the real UI form", true);

  const actions = (await api("/api/v1/actions?limit=50")).actions;
  const created = actions.find((action) => action.title === TITLE);
  check(
    "Core-authoritative Action exists with the UI-submitted title",
    Boolean(created),
    created ? created.id : "not found",
  );
  cli(["goto", BASE + `/#/actions/${created.id}`]);
  find(TITLE);
  check("agent navigated to the authoritative Action detail", true);

  const userAgentRaw = cli(["eval", "() => navigator.userAgent"]);
  const userAgent =
    /"([^"]*Mozilla[^"]*)"/.exec(userAgentRaw)?.[1] || "chromium";
  const consoleOutput = cli(["console", "error"]);
  const consoleErrors = consoleErrorCount(consoleOutput);
  const requests = cli(["requests"]);
  const failures = failedRequests(requests);
  check(
    "no browser console errors during the journey",
    consoleErrors === 0,
    consoleOutput.slice(0, 200),
  );
  check("no failed application requests during the journey", failures.length === 0, failures.join(" | ").slice(0, 300));

  const screenshot = join(EVIDENCE_DIR, "agent-browser-office.png");
  cli(["screenshot", "--filename", screenshot, "--full-page"]);
  check("bounded browser screenshot captured", existsSync(screenshot));

  evidence = {
    ok: true,
    tool: "playwright-cli",
    toolVersion: version,
    browser: "chromium",
    userAgent,
    candidateId: CANDIDATE_ID || null,
    actionId: created.id,
    title: TITLE,
    consoleErrors,
    failedRequests: failures,
    screenshot,
    checks,
    finishedAt: new Date().toISOString(),
  };
  ok = true;
} catch (e) {
  failure = String(e?.message ?? e);
} finally {
  try {
    cli(["close"], { timeout: 20000 });
  } catch {
    // Session cleanup is best-effort; the evidence records the primary result.
  }
  rmSync(workspace, { recursive: true, force: true });
}
if (!ok) {
  evidence = {
    ok: false,
    error: failure,
    tool: "playwright-cli",
    candidateId: CANDIDATE_ID || null,
    checks,
    finishedAt: new Date().toISOString(),
  };
}
writeFileSync(
  join(EVIDENCE_DIR, "agent-browser-evidence.json"),
  JSON.stringify(evidence, null, 2),
);
console.log(JSON.stringify({ ok, error: failure || undefined, actionId: evidence.actionId, evidence: join(EVIDENCE_DIR, "agent-browser-evidence.json") }));
process.exit(ok ? 0 : 1);
