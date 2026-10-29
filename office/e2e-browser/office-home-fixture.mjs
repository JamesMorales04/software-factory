// Fixture-only visual capture for Home and Execution Profiles.
//
// This harness never contacts Core and never performs provider inference.
// It starts the Vite Office dev server and answers every /core-api request
// from deterministic local fixtures so Chromium can render the real Office
// components at desktop and narrow viewports. The injected banner makes the
// fixture status explicit in every screenshot.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { chromium } from "playwright-core";

const root = resolve(import.meta.dirname, "../..");
const output = resolve(
  process.env.FACTORY_SCREENSHOT_DIR ||
    join(root, "docs/evidence/office-home-simplification-2026-10-29"),
);
const executablePath = process.env.CHROMIUM_EXE;
const port = Number(process.env.FACTORY_FIXTURE_PORT || "5199");
const baseUrl = `http://127.0.0.1:${port}`;

if (!executablePath) {
  throw new Error("CHROMIUM_EXE is required");
}

mkdirSync(output, { recursive: true });

const project = {
  id: "project-office-home",
  repository: "acme/widgets",
  name: "acme/widgets",
  base_commit: "0123456789abcdef0123456789abcdef01234567",
  created_at: "2026-10-27T10:00:00Z",
};

const operations = {
  modelRequestTimeoutSecs: 300,
  toolCallTimeoutSecs: 600,
  maxRequestBytes: 2097152,
  maxResponseBytes: 2097152,
  maxGenerationTokens: 8192,
  maxInFlightOperations: 4,
};

const progress = {
  diagnoseAfterStagnantCheckpoints: 2,
  escalateAfterUnsuccessfulReplans: 1,
  observationTimeoutSecs: 300,
};

const balanced = {
  id: "profile-balanced",
  name: "Balanced",
  description: "Default supported routes",
  category: "BALANCED",
  schemaVersion: 2,
  revision: 3,
  revisionId: "revision-3",
  contentHash: "a".repeat(64),
  globalDefault: true,
  projectDefaults: [project.id],
  roles: {
    lead: {
      provider: "opencode-go",
      model: "deepseek-v4.1-flash",
      reasoning: "medium",
    },
    implementer: {
      provider: "opencode-go",
      model: "deepseek-v4.1-flash",
      reasoning: "medium",
    },
    semanticReviewer: {
      provider: "opencode-go",
      model: "deepseek-v4-pro",
      reasoning: "high",
    },
  },
  operations,
  progress,
  autoStart: { enabled: false, riskClasses: [] },
  executable: true,
  warnings: [],
  createdAt: "2026-10-27T10:00:00Z",
  updatedAt: "2026-10-27T11:00:00Z",
};

const quality = {
  ...balanced,
  id: "profile-quality",
  name: "Quality",
  description: "Stronger reviewer route",
  category: "QUALITY",
  revision: 2,
  revisionId: "revision-2",
  contentHash: "b".repeat(64),
  globalDefault: false,
  projectDefaults: [],
};

const provider = {
  provider: "opencode-go",
  kind: "opencode",
  displayName: "OpenCode Go",
  defaultModel: "deepseek-v4.1-flash",
  defaultBaseUrl: "https://opencode.ai/inference/go/openai/v1",
  authModes: ["API_KEY"],
  chatgptOauth: "NOT_APPLICABLE",
  status: "SUPPORTED_TRUSTED_LOCAL",
  note: "Fixture connection; not live",
  configured: true,
  enabled: true,
  isDefault: true,
  model: "deepseek-v4.1-flash",
  baseUrl: "https://opencode.ai/inference/go/openai/v1",
  credentialFingerprint: "fixture-fingerprint",
  revision: 2,
  updatedAt: "2026-10-27T10:00:00Z",
  maxTurns: 4,
  maxInputTokens: 60000,
  maxOutputTokens: 12000,
  maxCostMicros: 500000,
  inputMicrosPerMillion: 150000,
  outputMicrosPerMillion: 600000,
  modelDiscovery: {
    provider: "opencode-go",
    status: "AVAILABLE",
    source: "OPENCODE_GO_CATALOG_URL",
    catalogUrl: "https://opencode.ai/inference/go/v1/models",
    models: ["deepseek-v4.1-flash", "deepseek-v4-pro", "gpt-6-luna"],
    detail: "3 model(s) discovered",
    discoveredAt: "2026-10-27T10:00:00Z",
    credentialSignal: "NOT_OBSERVED",
    lastKnown: null,
  },
};

const overview = {
  as_of: "2026-10-29T10:00:00Z",
  source: "Fixture projection",
  counts: {
    projects: 1,
    actions: 1,
    campaigns: 2,
    runs: 1,
    attempts: 1,
    candidates: 0,
    workers: 1,
    lead_requests: 1,
    authorized_campaigns: 1,
    campaigns_awaiting_authorization: 0,
    active_agents: 2,
    unresolved_sessions: 0,
    unresolved_model_calls: 1,
    pending_decisions: 1,
    blocked_signals: 0,
  },
  actions_by_status: {},
  campaigns_by_status: {},
  runs_by_status: {},
  gate_outcomes: {},
  budget: {
    measured_session_cost_micros: 0,
    measured_session_tokens: 0,
    measured_model_cost_micros: 0,
    measured_model_tokens: 0,
    unmeasured_sessions: 0,
    unmeasured_model_calls: 1,
    unmeasured_bound_cost_micros: 500000,
    model_calls_sampled: 0,
    note: "Fixture values. Not live usage.",
  },
  runtime: {
    core_healthy: true,
    workers: 1,
    active_agents: 2,
    unresolved_sessions: 0,
    unresolved_model_calls: 1,
    observability: {
      enabled: false,
      cursor: 0,
      exported: 0,
      pending: 0,
      dropped: 0,
      failed_attempts: 0,
      last_error: null,
    },
  },
  recent_accepted: [],
  recent_decisions: [
    {
      id: "decision-1",
      kind: "intake.approval",
      severity: "attention",
      actionable: true,
      title: "CSV export plan is ready",
      detail: "Review the frozen plan before implementation.",
      target: "/requests/fixture-request",
      subject_id: "fixture-request",
      updated_at: "2026-10-29T09:55:00Z",
    },
  ],
  recent_events: [],
};

const request = {
  id: "fixture-request",
  projectId: project.id,
  objective: "Add a CSV export endpoint to the reporting API",
  mode: "REVIEW_PLAN",
  state: "AWAITING_APPROVAL",
  stateDetail: "Frozen plan ready for explicit start",
  profile: {
    id: balanced.id,
    revisionId: balanced.revisionId,
    revision: balanced.revision,
    name: balanced.name,
    contentHash: balanced.contentHash,
  },
  actionId: "action-1",
  delegationId: "delegation-1",
  leadRequestId: "lead-1",
  campaignId: null,
  authorization: { required: true, available: true, reason: null },
  createdAt: "2026-10-29T09:40:00Z",
  updatedAt: "2026-10-29T09:55:00Z",
};

function json(route, body, status = 200) {
  return route.fulfill({
    status,
    contentType: "application/json",
    body: JSON.stringify(body),
  });
}

async function mockCore(route) {
  const url = new URL(route.request().url());
  const path = url.pathname.replace(/^\/core-api/, "");
  if (path === "/setup/status")
    return json(route, { required: false, version: "fixture" });
  if (path === "/auth/session")
    return json(route, {
      authenticated: true,
      status: "ACTIVE",
      bootstrapAvailable: false,
      csrfToken: "fixture-csrf",
    });
  if (path === "/health")
    return json(route, { healthy: true, version: "fixture" });
  if (path === "/projects") return json(route, [project]);
  if (path === "/execution-profiles")
    return json(route, {
      profiles: [balanced, quality],
      supportedRoutes: [
        { provider: "opencode-go", model: "deepseek-v4.1-flash" },
        { provider: "codex", model: "gpt-6-luna" },
      ],
    });
  if (path === "/intake/requests") return json(route, [request]);
  if (path === "/office/overview") return json(route, overview);
  if (path === "/settings/providers")
    return json(route, { providers: [provider] });
  if (path === `/projects/${project.id}/execution-profile`)
    return json(route, {
      projectId: project.id,
      profileId: balanced.id,
      revisionId: balanced.revisionId,
      revision: balanced.revision,
      contentHash: balanced.contentHash,
      source: "PROJECT_DEFAULT",
    });
  if (path === "/settings/providers/opencode-go/models")
    return json(route, {
      ...provider.modelDiscovery,
      adapter: provider.kind,
      credential: {
        presence: "CONFIGURED",
        validation: "NOT_OBSERVED",
        detail: "Fixture only",
      },
      inference: { status: "NOT_TESTED", detail: "Fixture only; no inference" },
    });
  if (path === "/settings/providers/opencode-go/audit")
    return json(route, { audit: [] });
  if (path === "/settings/runtime")
    return json(route, {
      semanticReviewEnabled: false,
      semanticReviewMaxOutputTokens: 8192,
      updatedAt: "2026-10-29T10:00:00Z",
    });
  return json(route, { error: `unmocked fixture path ${path}` }, 404);
}

const vite = spawn(
  process.execPath,
  [
    join(root, "office/node_modules/vite/bin/vite.js"),
    "--host",
    "127.0.0.1",
    "--port",
    String(port),
    "--strictPort",
  ],
  { cwd: join(root, "office"), stdio: "ignore" },
);

async function waitForServer() {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(baseUrl);
      if (response.ok) return;
    } catch {
      // Vite is still starting.
    }
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 200));
  }
  throw new Error("Vite did not start");
}

const banner = `
  position: fixed;
  z-index: 2147483647;
  top: 0;
  left: 0;
  right: 0;
  padding: 6px 12px;
  background: #b45309;
  color: #ffffff;
  font: 600 12px/1.4 ui-sans-serif, system-ui, sans-serif;
  letter-spacing: 0.04em;
  text-align: center;
  pointer-events: none;
`;

async function markFixture(page) {
  await page.evaluate((style) => {
    const node = document.createElement("div");
    node.textContent =
      "TEST FIXTURE — MOCKED CORE — NOT LIVE ACTIVITY OR ACCEPTANCE";
    node.setAttribute("style", style);
    document.body.prepend(node);
    document.body.style.paddingTop = "28px";
  }, banner);
}

async function capture(page, path, width, height) {
  await page.setViewportSize({ width, height });
  await page.waitForTimeout(250);
  await page.screenshot({ path, fullPage: true });
}

let browser;
try {
  await waitForServer();
  browser = await chromium.launch({
    executablePath,
    headless: true,
    args: ["--no-sandbox", "--disable-dev-shm-usage"],
  });
  const context = await browser.newContext({
    viewport: { width: 1440, height: 1100 },
    colorScheme: "light",
  });
  await context.addInitScript(() => {
    localStorage.setItem("factory-office-theme", "light");
    localStorage.setItem("factory-office-density", "comfortable");
  });
  await context.route("**/core-api/**", mockCore);
  const page = await context.newPage();

  await page.goto(`${baseUrl}/#/`);
  await page.getByRole("heading", { name: "Home" }).waitFor();
  await page.getByLabel("What should the Factory do?").waitFor();
  await page.getByLabel("Execution profile").selectOption("profile-balanced");
  await markFixture(page);
  await page.screenshot({
    path: join(output, "home-1440.png"),
    fullPage: true,
  });
  await capture(page, join(output, "home-390.png"), 390, 1200);

  await page.goto(`${baseUrl}/#/settings?tab=profiles`);
  await page.setViewportSize({ width: 1440, height: 1100 });
  await page.getByRole("heading", { name: /Execution profiles/ }).waitFor();
  await page.getByRole("button", { name: "Edit" }).first().click();
  await page.getByRole("heading", { name: "Edit Balanced" }).waitFor();
  await markFixture(page);
  await page.screenshot({
    path: join(output, "execution-profiles-1440.png"),
    fullPage: true,
  });
  await capture(page, join(output, "execution-profiles-390.png"), 390, 1200);

  console.log(JSON.stringify({ ok: true, fixture_only: true, output }));
} finally {
  if (browser) await browser.close();
  vite.kill("SIGTERM");
}
