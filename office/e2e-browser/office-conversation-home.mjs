// Fixture-only visual and interaction capture for the conversational Home.
//
// This harness starts the real Vite Office app and answers /core-api from
// deterministic local fixtures. It never contacts Core, a provider or GitHub.
// Every screenshot carries an explicit fixture banner. The screenshots are
// development evidence, not acceptance or live activity.
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { chromium } from "playwright-core";

const root = resolve(import.meta.dirname, "../..");
const output = resolve(
  process.env.FACTORY_SCREENSHOT_DIR ||
    join(root, "docs/evidence/office-conversation-home-2026-10-29"),
);
const executablePath = process.env.CHROMIUM_EXE;
const port = Number(process.env.FACTORY_FIXTURE_PORT || "5202");
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

const secondProject = {
  id: "project-telemetry",
  repository: "acme/telemetry",
  name: "acme/telemetry",
  base_commit: "89abcdef0123456789abcdef0123456789abcdef",
  created_at: "2026-10-26T09:00:00Z",
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

const roleRoute = {
  provider: "opencode-go",
  model: "deepseek-v4.1-flash",
  reasoning: "medium",
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
    lead: roleRoute,
    implementer: roleRoute,
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

const request = (id, state, objective, updatedAt) => ({
  id,
  projectId: project.id,
  objective,
  mode: "REVIEW_PLAN",
  state,
  stateDetail: "Fixture state. Not live activity.",
  profile: {
    id: balanced.id,
    revisionId: balanced.revisionId,
    revision: balanced.revision,
    name: balanced.name,
    contentHash: balanced.contentHash,
  },
  actionId: `action-${id}`,
  delegationId: `delegation-${id}`,
  leadRequestId: `lead-${id}`,
  campaignId: null,
  authorization: { required: true, available: true, reason: null },
  createdAt: "2026-10-29T09:40:00Z",
  updatedAt,
});

const activeSummary = request(
  "fixture-request-active",
  "AWAITING_APPROVAL",
  "Add a CSV export endpoint to the reporting API",
  "2026-10-29T09:55:00Z",
);
const completedSummary = request(
  "fixture-request-completed",
  "COMPLETED",
  "Harden device session revocation",
  "2026-10-29T08:10:00Z",
);
const executingSummary = request(
  "fixture-request-executing",
  "EXECUTING",
  "Cache provider model discovery results",
  "2026-10-28T17:45:00Z",
);

const recentRequests = [activeSummary, completedSummary, executingSummary];

const activeProjection = {
  ...activeSummary,
  stateDetail: "Fixture plan is frozen and waiting for explicit approval.",
  assessment: {
    schemaRevision: 1,
    projectId: project.id,
    actionId: activeSummary.actionId,
    requestId: activeSummary.leadRequestId,
    evidenceRefs: ["src/export.rs", "tests/export.rs"],
    intent: "Add a CSV export endpoint while preserving the existing API.",
    complexity: "moderate",
    effortEstimate: {
      status: "estimated",
      unit: "person-hours",
      minPersonHours: 3,
      maxPersonHours: 6,
      assumptions: ["Fixture values only"],
      unknownReason: "",
    },
    knowledgeGaps: [],
    changeImpact: ["Adds an export route"],
    validationNeeds: ["Integration test"],
    assumptions: ["Existing clients keep the same contract"],
    uncertainty: [],
    suggestedNextAction: "Review the frozen plan before implementation.",
  },
  proposal: null,
  plan: {
    id: "plan-fixture",
    request_id: activeSummary.leadRequestId,
    proposal_id: "proposal-fixture",
    plan_hash: "f".repeat(64),
    config_hash: "c".repeat(64),
    delegation_hash: "d".repeat(64),
    created_at: "2026-10-29T09:54:00Z",
  },
  campaign: null,
  messages: [
    {
      id: "message-fixture-1",
      role: "operator",
      body: "Keep the existing API backward compatible.",
      createdAt: "2026-10-29T09:52:00Z",
    },
  ],
};

const overview = {
  as_of: "2026-10-29T10:00:00Z",
  source: "Fixture projection",
  counts: {
    projects: 2,
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
    pending_decisions: 2,
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
      target: "/requests/fixture-request-active",
      subject_id: "fixture-request-active",
      updated_at: "2026-10-29T09:55:00Z",
    },
    {
      id: "decision-2",
      kind: "model.usage-unresolved",
      severity: "critical",
      actionable: true,
      title: "Unresolved model usage",
      detail: "One recorded call is AMBIGUOUS and needs an operator.",
      target: "/runtime",
      subject_id: "call-1",
      updated_at: "2026-10-29T09:40:00Z",
    },
  ],
  recent_events: [],
};

const emptyOverview = {
  ...overview,
  counts: {
    ...overview.counts,
    unresolved_sessions: 0,
    unresolved_model_calls: 0,
    pending_decisions: 0,
  },
  recent_decisions: [],
};

function json(route, body, status = 200) {
  return route.fulfill({
    status,
    contentType: "application/json",
    body: JSON.stringify(body),
  });
}

function pathOf(route) {
  return new URL(route.request().url()).pathname.replace(/^\/core-api/, "");
}

async function baseRoutes(route) {
  const path = pathOf(route);
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
  if (path === "/projects") return json(route, [project, secondProject]);
  if (path === "/execution-profiles")
    return json(route, {
      profiles: [balanced, quality],
      supportedRoutes: [
        { provider: "opencode-go", model: "deepseek-v4.1-flash" },
        { provider: "codex", model: "gpt-6-luna" },
      ],
    });
  if (path === "/office/overview") return json(route, overview);
  if (path === "/intake/requests") return json(route, recentRequests);
  if (path === "/intake/requests/fixture-request-active")
    return json(route, activeProjection);
  if (path.startsWith("/intake/requests/"))
    return json(route, { error: "fixture request not found" }, 404);
  return json(route, { error: `unmocked fixture path ${path}` }, 404);
}

async function initialRoutes(route) {
  const path = pathOf(route);
  if (path === "/intake/requests") return json(route, []);
  if (path === "/office/overview") return json(route, emptyOverview);
  return baseRoutes(route);
}

async function emptyProjectRoutes(route) {
  const path = pathOf(route);
  if (path === "/projects") return json(route, []);
  if (path === "/intake/requests") return json(route, []);
  if (path === "/office/overview") return json(route, emptyOverview);
  return baseRoutes(route);
}

async function errorRoutes(route) {
  const path = pathOf(route);
  if (path === "/intake/requests")
    return json(route, { error: "fixture history unavailable" }, 500);
  if (path === "/office/overview")
    return json(route, { error: "fixture status unavailable" }, 500);
  return baseRoutes(route);
}

async function loadingRoutes(route) {
  await new Promise((resolvePromise) => setTimeout(resolvePromise, 4000));
  return baseRoutes(route);
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
    const offset = Math.round(node.getBoundingClientRect().height + 8);
    document.body.style.paddingTop = `${offset}px`;
    const fixedOffset = document.createElement("style");
    fixedOffset.textContent = `@media (max-width: 1000px) { .sidebar { top: ${offset}px !important; height: calc(100dvh - ${offset}px) !important; } }`;
    document.head.append(fixedOffset);
  }, banner);
}

async function overflowReport(page) {
  return page.evaluate(() => {
    const viewport = window.innerWidth;
    const offenders = [];
    for (const element of document.querySelectorAll("body *")) {
      const rect = element.getBoundingClientRect();
      if (rect.width === 0 && rect.height === 0) continue;
      if (rect.right > viewport + 1 || rect.left < -1) {
        offenders.push({
          tag: element.tagName.toLowerCase(),
          className: String(element.className).slice(0, 120),
          left: Math.round(rect.left),
          right: Math.round(rect.right),
        });
      }
    }
    return {
      viewport,
      documentScrollWidth: document.documentElement.scrollWidth,
      bodyScrollWidth: document.body.scrollWidth,
      offenders: offenders.slice(0, 12),
    };
  });
}

const reports = [];

async function capture(page, name, width, height, fullPage = true) {
  await page.setViewportSize({ width, height });
  await page.waitForTimeout(280);
  const report = await overflowReport(page);
  reports.push({ name, ...report });
  if (
    report.documentScrollWidth > report.viewport + 1 ||
    report.bodyScrollWidth > report.viewport + 1 ||
    report.offenders.length > 0
  ) {
    throw new Error(
      `horizontal overflow at ${width}px in ${name}: ${JSON.stringify(report)}`,
    );
  }
  await page.screenshot({ path: join(output, name), fullPage });
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
    viewport: { width: 1440, height: 1000 },
    colorScheme: "light",
  });
  await context.addInitScript(() => {
    localStorage.setItem("factory-office-theme", "light");
    localStorage.setItem("factory-office-density", "comfortable");
    localStorage.setItem("factory-office-locale", "en");
  });
  await context.route("**/core-api/**", baseRoutes);
  const page = await context.newPage();
  await page.goto(`${baseUrl}/#/`);
  // The Home shell appears after App has checked setup, restored the session,
  // and received the health projection. Wait for that Core-backed ready state
  // before asserting on the page heading.
  await page.getByText("Core healthy").waitFor();
  await page.getByRole("heading", { name: "Home" }).waitFor();
  await page
    .getByRole("heading", { name: "What do you want to build today?" })
    .waitFor();
  await page.getByLabel("Execution profile").selectOption("profile-balanced");
  await page.getByTestId("home-thread").waitFor();
  await markFixture(page);

  const threadHref = await page
    .getByRole("link", { name: "Continue conversation" })
    .getAttribute("href");
  if (!threadHref?.includes("/requests/fixture-request-active")) {
    throw new Error(`unexpected thread continuation href: ${threadHref}`);
  }

  await capture(page, "home-1440-thread.png", 1440, 1000);
  await capture(page, "home-834-thread.png", 834, 1112);
  await capture(page, "home-390-thread.png", 390, 844);
  await capture(page, "home-320-thread.png", 320, 720);

  await page.setViewportSize({ width: 1440, height: 1000 });
  await page
    .getByLabel("Your instruction to the Lead")
    .fill("Add a CSV export endpoint");
  await page.getByLabel("Your instruction to the Lead").focus();
  await capture(page, "home-1440-focus.png", 1440, 1000);

  await page
    .getByLabel("Your instruction to the Lead")
    .fill("Describe what you want to build, fix or improve…");
  await page.getByLabel("Your instruction to the Lead").fill("");
  await page.getByRole("button", { name: "Fix a GitHub issue" }).hover();
  await capture(page, "home-1440-hover.png", 1440, 1000);

  await page.getByRole("button", { name: "Improve performance" }).click();
  const suggestionValue = await page
    .getByLabel("Your instruction to the Lead")
    .inputValue();
  if (suggestionValue !== "Improve performance in ") {
    throw new Error(`unexpected suggestion value: ${suggestionValue}`);
  }
  await capture(page, "home-1440-suggestion.png", 1440, 1000);

  const initialContext = await browser.newContext({
    viewport: { width: 1440, height: 1000 },
    colorScheme: "light",
  });
  await initialContext.addInitScript(() => {
    localStorage.setItem("factory-office-theme", "light");
    localStorage.setItem("factory-office-density", "comfortable");
    localStorage.setItem("factory-office-locale", "en");
  });
  await initialContext.route("**/core-api/**", initialRoutes);
  const initialPage = await initialContext.newPage();
  await initialPage.goto(`${baseUrl}/#/`);
  await initialPage.getByRole("heading", { name: "Home" }).waitFor();
  await initialPage
    .getByRole("heading", { name: "What do you want to build today?" })
    .waitFor();
  if ((await initialPage.getByTestId("home-thread").count()) !== 0) {
    throw new Error("initial history must not invent a Lead thread");
  }
  await markFixture(initialPage);
  await capture(initialPage, "home-1440-initial.png", 1440, 1000);
  await capture(initialPage, "home-390-initial.png", 390, 844);
  await initialContext.close();

  const spanishContext = await browser.newContext({
    viewport: { width: 1440, height: 1000 },
    colorScheme: "light",
    locale: "es-ES",
  });
  await spanishContext.addInitScript(() => {
    localStorage.setItem("factory-office-theme", "light");
    localStorage.setItem("factory-office-density", "comfortable");
    localStorage.setItem("factory-office-locale", "es");
  });
  await spanishContext.route("**/core-api/**", baseRoutes);
  const spanishPage = await spanishContext.newPage();
  await spanishPage.goto(`${baseUrl}/#/`);
  await spanishPage.getByRole("heading", { name: "Inicio" }).waitFor();
  await spanishPage
    .getByRole("heading", { name: "¿Qué quieres construir hoy?" })
    .waitFor();
  await spanishPage.getByTestId("home-thread").waitFor();
  await markFixture(spanishPage);
  await capture(spanishPage, "home-1440-es.png", 1440, 1000);
  await capture(spanishPage, "home-390-es.png", 390, 844);
  await spanishContext.close();

  const emptyContext = await browser.newContext({
    viewport: { width: 1440, height: 1000 },
    colorScheme: "light",
  });
  await emptyContext.addInitScript(() => {
    localStorage.setItem("factory-office-theme", "light");
    localStorage.setItem("factory-office-density", "comfortable");
    localStorage.setItem("factory-office-locale", "en");
  });
  await emptyContext.route("**/core-api/**", emptyProjectRoutes);
  const emptyPage = await emptyContext.newPage();
  await emptyPage.goto(`${baseUrl}/#/`);
  await emptyPage.getByRole("heading", { name: "Home" }).waitFor();
  await emptyPage.getByText("Register a Project first").waitFor();
  if ((await emptyPage.getByTestId("home-thread").count()) !== 0) {
    throw new Error("empty Project state must not invent a Lead thread");
  }
  await markFixture(emptyPage);
  await capture(emptyPage, "home-1440-empty.png", 1440, 1000);
  await emptyContext.close();

  const errorContext = await browser.newContext({
    viewport: { width: 1440, height: 1000 },
    colorScheme: "light",
  });
  await errorContext.addInitScript(() => {
    localStorage.setItem("factory-office-theme", "light");
    localStorage.setItem("factory-office-density", "comfortable");
    localStorage.setItem("factory-office-locale", "en");
  });
  await errorContext.route("**/core-api/**", errorRoutes);
  const errorPage = await errorContext.newPage();
  await errorPage.goto(`${baseUrl}/#/`);
  await errorPage.getByRole("heading", { name: "Home" }).waitFor();
  await errorPage.getByText("fixture history unavailable").waitFor();
  await errorPage.getByText(/Factory status is unavailable/).waitFor();
  await markFixture(errorPage);
  await capture(errorPage, "home-1440-error.png", 1440, 1000);
  await errorContext.close();

  const loadingContext = await browser.newContext({
    viewport: { width: 1440, height: 1000 },
    colorScheme: "light",
  });
  await loadingContext.addInitScript(() => {
    localStorage.setItem("factory-office-theme", "light");
    localStorage.setItem("factory-office-density", "comfortable");
    localStorage.setItem("factory-office-locale", "en");
  });
  await loadingContext.route("**/core-api/**", loadingRoutes);
  const loadingPage = await loadingContext.newPage();
  await loadingPage.goto(`${baseUrl}/#/`);
  await loadingPage.getByRole("heading", { name: "Home" }).waitFor();
  await loadingPage.getByText("Loading Projects…").waitFor();
  await markFixture(loadingPage);
  await capture(loadingPage, "home-1440-loading.png", 1440, 1000);
  await loadingContext.close();

  const result = {
    fixture_only: true,
    note: "Mocked Core fixtures only. Not live activity, not visual acceptance.",
    branch: "factory/office-conversation/20261029",
    base: "f7479c0",
    viewports: reports.map((item) => ({
      name: item.name,
      viewport: item.viewport,
      documentScrollWidth: item.documentScrollWidth,
      bodyScrollWidth: item.bodyScrollWidth,
      offenders: item.offenders,
    })),
    interaction_checks: {
      suggestion_fill:
        "PASS — Improve performance inserted 'Improve performance in ' and focused the composer",
      thread_continuation:
        "PASS — the active thread linked to /requests/fixture-request-active",
      initial_no_thread: "PASS — empty history rendered no Lead thread card",
      empty_projects_no_thread:
        "PASS — empty Project state rendered no Lead thread card",
    },
    unverified: [
      "Real Core/Compose behavior, persistence and provider inference",
      "Product Owner visual acceptance",
      "Cross-browser rendering",
      "Quota or subscription balance context (no Core data is exposed to Office)",
    ],
  };
  writeFileSync(
    join(output, "conversation-home-evidence.json"),
    `${JSON.stringify(result, null, 2)}\n`,
  );
  writeFileSync(
    join(output, "overflow-report.json"),
    `${JSON.stringify(
      {
        fixture_only: true,
        note: "Viewport checks passed only for this mocked fixture page; they are not acceptance evidence.",
        results: reports,
      },
      null,
      2,
    )}\n`,
  );
  console.log(
    JSON.stringify({
      ok: true,
      fixture_only: true,
      output,
      captures: reports.map((item) => ({
        name: item.name,
        viewport: item.viewport,
        scrollWidth: item.documentScrollWidth,
      })),
    }),
  );
} finally {
  if (browser) await browser.close();
  vite.kill("SIGTERM");
}
