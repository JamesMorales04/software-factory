// Fixture-only visual capture for the Home visual-polish slice.
//
// This harness never contacts Core and never performs provider inference. It
// starts the real Vite Office app and answers /core-api from deterministic
// local fixtures. Every screenshot carries a fixture banner; the reference PNG
// comparisons are style targets, not product data or visual acceptance.
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { chromium } from "playwright-core";

const root = resolve(import.meta.dirname, "../..");
const output = resolve(
  process.env.FACTORY_SCREENSHOT_DIR ||
    join(root, "docs/evidence/office-home-visual-polish-2026-10-29"),
);
const executablePath = process.env.CHROMIUM_EXE;
const port = Number(process.env.FACTORY_FIXTURE_PORT || "5201");
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

const request = (index, state, objective, updatedAt) => ({
  id: `fixture-request-${index}`,
  projectId: project.id,
  objective,
  mode: "REVIEW_PLAN",
  state,
  stateDetail: "Fixture state.",
  profile: {
    id: balanced.id,
    revisionId: balanced.revisionId,
    revision: balanced.revision,
    name: balanced.name,
    contentHash: balanced.contentHash,
  },
  actionId: `action-${index}`,
  delegationId: `delegation-${index}`,
  leadRequestId: `lead-${index}`,
  campaignId: null,
  authorization: { required: true, available: true, reason: null },
  createdAt: "2026-10-29T09:40:00Z",
  updatedAt,
});

const recentRequests = [
  request(
    1,
    "AWAITING_APPROVAL",
    "Add a CSV export endpoint to the reporting API",
    "2026-10-29T09:55:00Z",
  ),
  request(
    2,
    "COMPLETED",
    "Harden device session revocation",
    "2026-10-29T08:10:00Z",
  ),
  request(
    3,
    "EXECUTING",
    "Cache provider model discovery results",
    "2026-10-28T17:45:00Z",
  ),
];

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
      target: "/requests/fixture-request-1",
      subject_id: "fixture-request-1",
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
  if (path === "/projects") return json(route, [project, secondProject]);
  if (path === "/execution-profiles")
    return json(route, {
      profiles: [balanced, quality],
      supportedRoutes: [
        { provider: "opencode-go", model: "deepseek-v4.1-flash" },
        { provider: "codex", model: "gpt-6-luna" },
      ],
    });
  if (path === "/intake/requests") return json(route, recentRequests);
  if (path === "/office/overview") return json(route, overview);
  return json(route, { error: `unmocked fixture path ${path}` }, 404);
}

async function mockEmptyProjects(route) {
  const url = new URL(route.request().url());
  const path = url.pathname.replace(/^\/core-api/, "");
  if (path === "/projects") return json(route, []);
  if (path === "/intake/requests") return json(route, []);
  if (path === "/office/overview") {
    return json(route, { ...overview, recent_decisions: [] });
  }
  return mockCore(route);
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

let browser;
const reports = [];
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
  await context.route("**/core-api/**", mockCore);
  const page = await context.newPage();

  await page.goto(`${baseUrl}/#/`);
  await page.getByRole("heading", { name: "Home" }).waitFor();
  await page.getByLabel("What should the Factory do?").waitFor();
  await page.getByLabel("Execution profile").selectOption("profile-balanced");
  await markFixture(page);

  for (const [name, width, height] of [
    ["home-1440.png", 1440, 1000],
    ["home-834.png", 834, 1112],
    ["home-390.png", 390, 844],
    ["home-320.png", 320, 720],
  ]) {
    await page.setViewportSize({ width, height });
    await page.waitForTimeout(300);
    const report = await overflowReport(page);
    reports.push({ name, ...report });
    if (
      report.documentScrollWidth > report.viewport + 1 ||
      report.bodyScrollWidth > report.viewport + 1 ||
      report.offenders.length > 0
    ) {
      throw new Error(
        `horizontal overflow at ${width}px: ${JSON.stringify(report)}`,
      );
    }
    await page.screenshot({ path: join(output, name), fullPage: true });
  }

  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("button", { name: "Toggle navigation" }).click();
  await page.getByRole("button", { name: "Close navigation drawer" }).waitFor();
  await page.waitForTimeout(300);
  const drawerReport = await overflowReport(page);
  reports.push({ name: "home-390-drawer.png", ...drawerReport });
  await page.screenshot({
    path: join(output, "home-390-drawer.png"),
    fullPage: false,
  });
  await page.getByRole("button", { name: "Close navigation drawer" }).click();

  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.getByLabel("What should the Factory do?").focus();
  await page.waitForTimeout(200);
  await page.screenshot({
    path: join(output, "home-1440-focus.png"),
    fullPage: true,
  });

  await page.getByRole("button", { name: "Collapse navigation" }).click();
  await page.waitForTimeout(300);
  await page.screenshot({
    path: join(output, "home-1440-collapsed.png"),
    fullPage: true,
  });
  await page.getByRole("button", { name: "Toggle navigation" }).click();

  const dark = await browser.newContext({
    viewport: { width: 1440, height: 1000 },
    colorScheme: "dark",
  });
  await dark.addInitScript(() => {
    localStorage.setItem("factory-office-theme", "dark");
    localStorage.setItem("factory-office-density", "comfortable");
    localStorage.setItem("factory-office-locale", "en");
  });
  await dark.route("**/core-api/**", mockCore);
  const darkPage = await dark.newPage();
  await darkPage.goto(`${baseUrl}/#/`);
  await darkPage.getByRole("heading", { name: "Home" }).waitFor();
  await darkPage.getByLabel("What should the Factory do?").waitFor();
  await darkPage
    .getByLabel("Execution profile")
    .selectOption("profile-balanced");
  await markFixture(darkPage);
  const darkReport = await overflowReport(darkPage);
  reports.push({ name: "home-1440-dark.png", ...darkReport });
  if (
    darkReport.documentScrollWidth > darkReport.viewport + 1 ||
    darkReport.bodyScrollWidth > darkReport.viewport + 1 ||
    darkReport.offenders.length > 0
  ) {
    throw new Error(
      `horizontal overflow in dark Home: ${JSON.stringify(darkReport)}`,
    );
  }
  await darkPage.screenshot({
    path: join(output, "home-1440-dark.png"),
    fullPage: true,
  });

  const spanish = await browser.newContext({
    viewport: { width: 390, height: 844 },
    colorScheme: "light",
    locale: "es-ES",
  });
  await spanish.addInitScript(() => {
    localStorage.setItem("factory-office-theme", "light");
    localStorage.setItem("factory-office-density", "comfortable");
    localStorage.setItem("factory-office-locale", "es");
  });
  await spanish.route("**/core-api/**", mockCore);
  const spanishPage = await spanish.newPage();
  await spanishPage.goto(`${baseUrl}/#/`);
  await spanishPage.getByRole("heading", { name: "Inicio" }).waitFor();
  await spanishPage.getByLabel("¿Qué debe hacer la Factory?").waitFor();
  await spanishPage
    .getByLabel("Perfil de ejecución")
    .selectOption("profile-balanced");
  await markFixture(spanishPage);
  const spanishReport = await overflowReport(spanishPage);
  reports.push({ name: "home-390-es.png", ...spanishReport });
  if (
    spanishReport.documentScrollWidth > spanishReport.viewport + 1 ||
    spanishReport.bodyScrollWidth > spanishReport.viewport + 1 ||
    spanishReport.offenders.length > 0
  ) {
    throw new Error(
      `horizontal overflow in Spanish Home: ${JSON.stringify(spanishReport)}`,
    );
  }
  await spanishPage.screenshot({
    path: join(output, "home-390-es.png"),
    fullPage: true,
  });

  const empty = await browser.newContext({
    viewport: { width: 1440, height: 1000 },
    colorScheme: "light",
  });
  await empty.addInitScript(() => {
    localStorage.setItem("factory-office-theme", "light");
    localStorage.setItem("factory-office-density", "comfortable");
    localStorage.setItem("factory-office-locale", "en");
  });
  await empty.route("**/core-api/**", mockEmptyProjects);
  const emptyPage = await empty.newPage();
  await emptyPage.goto(`${baseUrl}/#/`);
  await emptyPage.getByRole("heading", { name: "Home" }).waitFor();
  await emptyPage.getByText("Register a Project first").waitFor();
  await markFixture(emptyPage);
  const emptyReport = await overflowReport(emptyPage);
  reports.push({ name: "home-1440-empty-projects.png", ...emptyReport });
  if (
    emptyReport.documentScrollWidth > emptyReport.viewport + 1 ||
    emptyReport.bodyScrollWidth > emptyReport.viewport + 1 ||
    emptyReport.offenders.length > 0
  ) {
    throw new Error(
      `horizontal overflow in empty Home: ${JSON.stringify(emptyReport)}`,
    );
  }
  await emptyPage.screenshot({
    path: join(output, "home-1440-empty-projects.png"),
    fullPage: true,
  });

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
      viewports: reports.map((item) => ({
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
