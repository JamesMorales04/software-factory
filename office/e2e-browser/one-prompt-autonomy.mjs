// One-prompt autonomy browser lane (harness, not independent acceptance).
//
// Positive lane: one real Office prompt + at most one real UI policy
// confirmation creates an AUTONOMOUS request; Core prepares the plan, spec,
// environment and Campaign; a real candidate reaches the deterministic
// Quality Gate; then the shell restarts Core and this lane verifies identical
// IDs/state and zero new fixture counters or model reservations.
//
// This file is owned by the harness lane. It is not product code and does not
// grant authority, acceptance or isolation.
//
// Required environment (phase=run):
//   FACTORY_OFFICE_URL        upstream Office/Core URL, e.g. http://core:8765
//   FACTORY_BOOTSTRAP_CODE    one-time owner confirmation
//   FACTORY_EVIDENCE_DIR      evidence directory (private)
//   FACTORY_MODEL_CONTEXT     path inside the browser container, e.g. /model/context.json
//   FACTORY_GIT_REPOSITORY    fixture Git URL, e.g. http://git-fixture:8080/repo.git
//   CHROMIUM_EXE              pinned Chromium executable
//   FACTORY_PLAYWRIGHT_CORE   playwright-core package directory (optional if resolvable)
//   FACTORY_MODEL_FIXTURE_URL fixture URL for /counters, default http://model-fixture:8080
//   FACTORY_UAT_PHASE         run | verify-restart
// Optional:
//   FACTORY_UAT_NEGATIVE      no-policy | license-header | funding-false
//   FACTORY_UAT_RELOAD_PREPARATION=1
//   FACTORY_UAT_ALLOW_ENROLLED=1
//   FACTORY_STORAGE_STATE     private storageState path (run writes, verify reads)
//   FACTORY_UAT_SNAPSHOT      private snapshot path (run writes, verify reads)
//   FACTORY_UAT_TIMEOUT_SECS  gate/preparation bound (default 1800)
//
// Evidence is SIMULATED (deterministic fixture model), never live inference,
// never independent acceptance and never certified isolation.

import assert from "node:assert/strict";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createServer as createTcpServer } from "node:net";
import { connect } from "node:net";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import {
  assertCampaignAuthorization,
  assertCounterDelta,
  assertCountersBaseline,
  assertFundingProjection,
  assertLeaseProjection,
  assertNoFixtureErrors,
  assertNoManualControls,
  assertNoPreKnownRequestId,
  assertStableSnapshot,
  isWorkflowPath,
  normalizeCorePath,
  validateDriverCalls,
  validatePageMutations,
} from "./one-prompt-autonomy-contract.mjs";

const repoRoot = resolve(import.meta.dirname, "../..");
const phase = process.env.FACTORY_UAT_PHASE?.trim() || "run";
const negative = process.env.FACTORY_UAT_NEGATIVE?.trim() || "";
const upstream = required("FACTORY_OFFICE_URL");
const evidenceDir = required("FACTORY_EVIDENCE_DIR");
const modelContextPath = required("FACTORY_MODEL_CONTEXT");
const gitRepository =
  process.env.FACTORY_GIT_REPOSITORY?.trim() ||
  "http://git-fixture:8080/repo.git";
const fixtureUrl =
  process.env.FACTORY_MODEL_FIXTURE_URL?.trim() || "http://model-fixture:8080";
const timeoutMs =
  Number(process.env.FACTORY_UAT_TIMEOUT_SECS || "1800") * 1000;
const snapshotPath =
  process.env.FACTORY_UAT_SNAPSHOT?.trim() ||
  join(evidenceDir, "oneprompt-snapshot.json");
const storageStatePath =
  process.env.FACTORY_STORAGE_STATE?.trim() ||
  join(evidenceDir, "oneprompt-storage-state.json");
const objective =
  process.env.FACTORY_UAT_OBJECTIVE?.trim() ||
  "Add a deterministic one-prompt candidate in src/provider.txt and cover it with the frozen checks.";
const candidateSha = process.env.FACTORY_UAT_CANDIDATE?.trim() || "";
const composeProject =
  process.env.FACTORY_UAT_COMPOSE_PROJECT?.trim() || "unknown-oneprompt-project";

assertNoPreKnownRequestId(process.env);
if (process.env.FACTORY_COOKIE_JAR) {
  fail("FACTORY_COOKIE_JAR is not part of this contract; use FACTORY_STORAGE_STATE");
}
if (!["run", "verify-restart"].includes(phase)) {
  fail(`unsupported FACTORY_UAT_PHASE: ${phase}`);
}
if (
  ![
    "",
    "no-policy",
    "license-header",
    "funding-false",
    "lease-vs-request",
  ].includes(negative)
) {
  fail(`unsupported FACTORY_UAT_NEGATIVE: ${negative}`);
}
mkdirSync(evidenceDir, { recursive: true, mode: 0o700 });
mkdirSync(dirname(storageStatePath), { recursive: true, mode: 0o700 });
mkdirSync(dirname(snapshotPath), { recursive: true, mode: 0o700 });

const MANUAL_CONTROL_LABELS = [
  "Run assessment",
  "Ejecutar evaluación",
  "Approval rationale",
  "Justificación de la aprobación",
  "Approve frozen plan and start",
  "Aprobar el plan congelado e iniciar",
  "Enable governed execution",
  "Habilitar ejecución gobernada",
  "Open Director plan review (Compile plan / Record participant plan)",
  "Abrir la revisión del plan en el Director (Compilar plan / Registrar plan de participantes)",
];
const AUTOPILOT_LABELS = [
  "Autopilot owns these deterministic steps in Core.",
  "El autopilot posee estos pasos deterministas en Core.",
];
const OPEN_CAMPAIGN_LABELS = ["Open Campaign", "Abrir Campaña"];
const WAITING_OWNER_LABELS = [
  "Core has finished the planning materials and is waiting for the owner plan decision.",
  "Core terminó los materiales del plan y espera la decisión del owner.",
];

const record = {
  schema: "factory.oneprompt-autonomy-lane.v1",
  result: "NOT_RUN",
  phase,
  negative: negative || null,
  started_at: new Date().toISOString(),
  finished_at: null,
  candidate_sha: candidateSha,
  compose_project: composeProject,
  objective,
  office_upstream: redactUrl(upstream),
  browser: {
    image: process.env.FACTORY_UAT_BROWSER_IMAGE || "unknown",
    chromium: process.env.CHROMIUM_EXE || "auto",
    playwright_core: process.env.FACTORY_PLAYWRIGHT_CORE || "auto",
  },
  evidence: {
    paid_inference: false,
    deterministic_fixture_model: true,
    live_provider_inference: "NOT_TESTED",
    independent_acceptance: "NOT_TESTED",
    isolation: "NOT_CLAIMED",
    certified: false,
    simulated: true,
  },
  ids: {},
  plan: {},
  counters: { before: null, after: null, delta: null },
  assertions: {},
  console: { errors: [], page_errors: [], failed_requests: [], http_404: [] },
  driver_calls: [],
  page_mutations: [],
  findings: [],
  screenshots: [],
};

let stage = "start";
let browser = null;
let page = null;
let proxy = null;
let context = null;

function required(name) {
  const value = process.env[name]?.trim();
  if (!value) throw new Error(`${name} is required`);
  return value;
}

function fail(message) {
  throw new Error(message);
}

function redactUrl(value) {
  try {
    const url = new URL(value);
    return `${url.protocol}//${url.host}`;
  } catch {
    return "invalid";
  }
}

function writeJson(path, value) {
  mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

async function loadChromium() {
  const candidates = [
    process.env.FACTORY_PLAYWRIGHT_CORE,
    "/opt/factory-browser/node_modules/playwright-core",
    "/usr/local/lib/node_modules/playwright/node_modules/playwright-core",
    join(repoRoot, "office/e2e-browser/node_modules/playwright-core"),
  ].filter(Boolean);
  for (const candidate of candidates) {
    const entry = candidate.endsWith(".mjs")
      ? candidate
      : join(candidate, "index.mjs");
    if (!existsSync(entry)) continue;
    try {
      const module = await import(pathToFileURL(entry).href);
      if (typeof module.chromium?.launch === "function") return module.chromium;
    } catch {
      // Try the next pinned candidate.
    }
  }
  fail(
    "playwright-core is not resolvable; set FACTORY_PLAYWRIGHT_CORE to the pinned package directory",
  );
}

function chromiumExecutable() {
  const candidates = [
    process.env.CHROMIUM_EXE,
    "/home/factory/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome",
    "/home/factory/.nix-profile/bin/chromium",
  ].filter(Boolean);
  const found = candidates.find((candidate) => existsSync(candidate));
  if (!found) fail("CHROMIUM_EXE is required and no pinned Chromium was found");
  return found;
}

async function startLoopbackProxy(targetUrl) {
  const target = new URL(targetUrl);
  const port = Number(target.port || (target.protocol === "https:" ? 443 : 80));
  const server = createTcpServer((client) => {
    const upstreamSocket = connect(port, target.hostname);
    client.on("error", () => upstreamSocket.destroy());
    upstreamSocket.on("error", () => client.destroy());
    client.pipe(upstreamSocket);
    upstreamSocket.pipe(client);
  });
  await new Promise((resolvePromise, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolvePromise);
  });
  const address = server.address();
  return {
    url: `http://127.0.0.1:${address.port}`,
    close: () =>
      new Promise((resolvePromise) => {
        server.close(() => resolvePromise());
      }),
  };
}

function isVisible(locator) {
  return locator.isVisible().catch(() => false);
}

async function waitFor(label, operation, timeout = timeoutMs, interval = 1000) {
  const deadline = Date.now() + timeout;
  let lastError = null;
  while (Date.now() < deadline) {
    try {
      const value = await operation();
      if (value) return value;
    } catch (error) {
      lastError = error;
    }
    await page.waitForTimeout(interval);
  }
  fail(
    `${label} timed out after ${timeout}ms${lastError ? `: ${lastError.message}` : ""}`,
  );
}

async function bodyText() {
  return page.locator("body").innerText();
}

function requireAnyLabel(text, labels, description) {
  const match = labels.find((label) => text.includes(label));
  if (!match) fail(`${description}: expected one of ${labels.join(" | ")}`);
  record.assertions[description] = match;
}

async function shot(name) {
  const path = join(evidenceDir, name);
  await page.screenshot({ path, fullPage: true });
  record.screenshots.push({ name, path });
}

// A real same-origin Core request from the enrolled page. Setup calls carry the
// driver marker so the constructive contract can prove the driver did not seed
// workflow state. The page UI's own calls never carry this marker.
async function core(method, path, body, { driver = true } = {}) {
  const marker = driver ? "1" : "";
  const result = await page.evaluate(
    async ({ method, path, body, marker }) => {
      const csrf = document.cookie
        .split("; ")
        .find((item) => item.startsWith("factory_csrf="))
        ?.slice("factory_csrf=".length);
      const headers = { "content-type": "application/json" };
      if (marker) headers["x-oneprompt-driver"] = marker;
      if (!["GET", "HEAD"].includes(method) && csrf) {
        headers["x-factory-csrf"] = decodeURIComponent(csrf);
      }
      const response = await fetch(`/core-api${path}`, {
        method,
        credentials: "same-origin",
        headers,
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      const text = await response.text();
      let payload;
      try {
        payload = text ? JSON.parse(text) : null;
      } catch {
        payload = text;
      }
      return { status: response.status, payload };
    },
    { method, path, body, marker },
  );
  record.driver_calls.push({ method, path: normalizeCorePath(path) });
  if (result.status >= 400) {
    fail(`Core ${method} ${path} -> ${result.status}: ${JSON.stringify(result.payload).slice(0, 700)}`);
  }
  return result.payload;
}

async function readFixtureDiagnostics() {
  const response = await fetch(`${fixtureUrl}/counters`, {
    signal: AbortSignal.timeout(10_000),
  });
  if (!response.ok) fail(`fixture /counters -> ${response.status}`);
  return response.json();
}

async function requireLicenseFixtureOrBlock() {
  const diagnostics = await readFixtureDiagnostics();
  if (!Object.prototype.hasOwnProperty.call(diagnostics.counters ?? {}, "license_evidence_review")) {
    record.result = "BLOCKED";
    record.stage = "license-fixture";
    record.error = "LICENSE_FIXTURE_PHASE_MISSING";
    record.findings.push({
      kind: "license-fixture-phase-missing",
      detail:
        "Core98 fixture has no license_evidence_review phase/counter; positive and license lanes are BLOCKED, not PASS/PARTIAL",
      required_owner: "98e2f199-2100-4907-8f16-a93e3038c321",
    });
    return false;
  }
  return true;
}

async function enrollIfNeeded({ requireFresh }) {
  await page.goto(`${proxy.url}/`, { waitUntil: "domcontentloaded" });
  const setup = page.getByRole("heading", { name: "Set up this Factory" });
  const home = page.getByRole("heading", { name: "Home", level: 1 });
  if (await isVisible(setup)) {
    const bootstrap = process.env.FACTORY_BOOTSTRAP_CODE?.trim();
    if (!bootstrap) fail("FACTORY_BOOTSTRAP_CODE is required for first-run enrollment");
    await setup.waitFor({ timeout: 30_000 });
    await page.getByLabel("One-time owner confirmation").fill(bootstrap);
    await page.getByLabel("This device name").fill("One-prompt autonomy lane");
    await page.getByRole("button", { name: "Enroll this device" }).click();
    await home.waitFor({ timeout: 30_000 });
    record.assertions.owner_enrollment = "PASS";
    return;
  }
  if (requireFresh && process.env.FACTORY_UAT_ALLOW_ENROLLED !== "1") {
    fail("the stack is already enrolled; the one-prompt lane requires a fresh disposable stack");
  }
  await home.waitFor({ timeout: 30_000 });
}

async function importProject() {
  await page.getByRole("link", { name: "Projects", exact: true }).click();
  await page.getByRole("heading", { name: "Projects", level: 1 }).waitFor();
  await page.getByRole("tab", { name: "Public Git URL" }).click();
  await page.getByTestId("project-repository").fill(gitRepository);
  await page.getByRole("button", { name: "Register Project" }).click();
  await page.waitForURL(/#\/projects\/[^/?]+/, { timeout: 180_000 });
  const projectId = page.url().match(/#\/projects\/([^/?]+)/)?.[1];
  if (!projectId) fail("project import did not expose a Project identity");
  record.ids.project_id = projectId;
  record.assertions.project_discovery = "PASS";
  return projectId;
}

async function configureProjectAndProfile(projectId) {
  const initial = await core("GET", `/projects/${projectId}/config`);
  const projectConfig = await core("PUT", `/projects/${projectId}/config`, {
    expectedRevision: initial.revision,
    allowedPaths: ["src"],
    capabilities: ["repository-edit", "generic-toolchain"],
    qualityProfile: "generic",
    active: true,
  });
  if (!projectConfig.active) fail("project configuration is not active");
  if (JSON.stringify(projectConfig.allowedPaths) !== JSON.stringify(["src"])) {
    fail(`unexpected allowed paths: ${JSON.stringify(projectConfig.allowedPaths)}`);
  }
  record.assertions.project_configuration = "PASS";

  await core("PUT", "/settings/providers/opencode-go", {
    model: "deepseek-v4.1-flash",
    baseUrl: `${fixtureUrl}/v1`,
    credential: "compose-fixture-credential-0123456789",
    enabled: true,
    isDefault: true,
    maxTurns: 16,
    maxInputTokens: 100000,
    maxOutputTokens: 12000,
    maxCostMicros: 500000,
    inputMicrosPerMillion: 1000,
    outputMicrosPerMillion: 2000,
  });
  await core("PUT", "/settings/runtime", { semanticReviewEnabled: true });
  record.assertions.provider_configuration = "PASS";

  const role = {
    provider: "opencode-go",
    model: "deepseek-v4.1-flash",
    reasoning: "medium",
  };
  const profile = await core("POST", "/execution-profiles", {
    name: "One-prompt autonomous R0",
    description: "Deterministic one-prompt autonomy lane profile",
    category: "BALANCED",
    roles: {
      lead: role,
      implementer: role,
      semanticReviewer: role,
    },
    operations: {
      modelRequestTimeoutSecs: 300,
      toolCallTimeoutSecs: 600,
      maxRequestBytes: 2097152,
      maxResponseBytes: 2097152,
      maxGenerationTokens: 8192,
      maxInFlightOperations: 2,
    },
    progress: {
      diagnoseAfterStagnantCheckpoints: 2,
      escalateAfterUnsuccessfulReplans: 1,
      observationTimeoutSecs: 300,
    },
    auto_start: { enabled: true, risk_classes: ["R0"] },
  });
  const autoStart = profile.autoStart ?? profile.auto_start;
  if (autoStart?.enabled !== true) fail("the execution profile is not autonomous");
  if (!(autoStart.riskClasses ?? autoStart.risk_classes ?? []).includes("R0")) {
    fail("the execution profile does not enable R0 auto-start");
  }
  await core("POST", `/execution-profiles/${profile.id}/default`, { scope: "global" });
  record.ids.execution_profile = profile.id;
  record.assertions.execution_profile_discovery = "PASS";
  return { projectConfig, profile };
}

function writeModelContext(projectId, projectConfig) {
  const context = {
    schema: "factory-compose-model-fixture-v1",
    projectId,
    intakeRequestId: "",
    requestId: "",
    actionId: "",
    // Per-request fixture variant read by Core98's fixture for the negative
    // licence-header lane. Positive/default is explicit, never inferred.
    fixtureVariant: negative === "license-header" ? "license-header" : "positive",
    objective,
    baseRevision: projectConfig.baseCommit,
    projectConfigHash: projectConfig.configHash,
    qualityProfile: "generic",
    allowedPaths: ["src"],
    capabilities: ["repository-edit", "generic-toolchain"],
    provider: "opencode-go",
    model: "deepseek-v4.1-flash",
    checks: [
      {
        id: "build",
        kind: "build",
        description: "deterministic build",
        command: ["true"],
        expectExit: 0,
        timeoutSecs: 30,
      },
      {
        id: "test",
        kind: "test",
        description: "deterministic test",
        command: ["true"],
        expectExit: 0,
        timeoutSecs: 30,
      },
      {
        id: "functional",
        kind: "functional",
        description: "provider candidate content exists",
        command: ["grep", "-q", "provider candidate", "src/provider.txt"],
        expectExit: 0,
        timeoutSecs: 30,
      },
      {
        id: "negative",
        kind: "negative",
        description: "absent content is rejected",
        command: ["grep", "-q", "absent", "src/provider.txt"],
        expectExit: 1,
        timeoutSecs: 30,
      },
    ],
  };
  mkdirSync(dirname(modelContextPath), { recursive: true, mode: 0o755 });
  writeFileSync(modelContextPath, `${JSON.stringify(context, null, 2)}\n`, {
    mode: 0o644,
  });
  return context;
}

function writeRequestContext(requestId, created, template) {
  const contextsDir = join(dirname(modelContextPath), "contexts");
  mkdirSync(contextsDir, { recursive: true, mode: 0o755 });
  writeFileSync(
    join(contextsDir, `${requestId}.json`),
    `${JSON.stringify(
      {
        ...template,
        intakeRequestId: requestId,
        requestId: created.leadRequestId,
        actionId: created.actionId,
      },
      null,
      2,
    )}\n`,
    { mode: 0o644 },
  );
}

async function homeAndSubmit(projectId, profile) {
  await page.getByRole("link", { name: "New request", exact: true }).click();
  await page.getByRole("heading", { name: "Home", level: 1 }).waitFor();
  const projectOptions = await page.locator("#home-project option").allTextContents();
  if (!projectOptions.some((text) => text.includes(projectId))) {
    fail("the imported Project is not discoverable in Home");
  }
  const profileOptions = await page.locator("#home-profile option").allTextContents();
  if (!profileOptions.some((text) => text.includes(profile.name))) {
    fail("the autonomous profile is not discoverable in Home");
  }
  await page.locator("#home-project").selectOption(projectId);
  await page.locator("#home-profile").selectOption(profile.id);
  await page.locator("#home-objective").fill(objective);
  await shot("home-one-prompt.png");
  await page.getByRole("button", { name: "Send to Lead" }).click();
  const dialog = page.getByRole("heading", {
    name: "Enable autonomous R0 execution",
    level: 3,
  });
  await dialog.waitFor({ timeout: 30_000 });
  const policyText = await page.locator(".home-policy").innerText();
  if (!policyText.includes("src")) fail("policy scope does not show the src allowed path");
  if (!policyText.toLowerCase().includes("autonomous start enabled")) {
    fail("policy scope does not show autonomous start enabled");
  }
  record.assertions.policy_dialog = "PASS";
  await shot("policy-dialog.png");
  return policyText;
}

async function deferPolicyAndAssertNoState() {
  const intakePosts = record.page_mutations.filter(
    (entry) => entry.method === "POST" && entry.path === "/intake/requests",
  ).length;
  if (intakePosts !== 0) {
    fail(`no-policy lane created ${intakePosts} intake request(s)`);
  }
  const diagnostics = await readFixtureDiagnostics();
  assertCountersBaseline(diagnostics.counters);
  await page.getByRole("button", { name: "Not now" }).click();
  await page.getByRole("heading", { name: "Home", level: 1 }).waitFor();
  await page.waitForTimeout(1500);
  const after = record.page_mutations.filter(
    (entry) => entry.method === "POST" && entry.path === "/intake/requests",
  ).length;
  if (after !== 0) fail("dismissing the policy dialog created a request");
  record.assertions.no_policy = "PASS";
  record.result = "PASS";
}

async function enablePolicyAndOpenRequest() {
  await page
    .getByRole("checkbox", { name: /I authorize automatic R0 execution/ })
    .check();
  await page
    .getByRole("button", { name: "Enable autonomous R0 execution" })
    .click();
  await page.waitForURL(/#\/requests\/[^/?]+/, { timeout: 180_000 });
  const requestId = page.url().match(/#\/requests\/([^/?]+)/)?.[1];
  if (!requestId) fail("request creation did not expose a request identity");
  record.ids.request_id = requestId;
  record.assertions.request_discovery = "PASS";
  return requestId;
}

async function capturePlanAndEnvironment(requestId) {
  const request = await waitFor(
    "frozen plan hash",
    async () => {
      const value = await core("GET", `/intake/requests/${requestId}`);
      return value.plan?.plan_hash ? value : null;
    },
    timeoutMs,
    1000,
  );
  record.ids.action_id = request.actionId;
  record.ids.lead_id = request.leadRequestId;
  record.plan.plan_hash = request.plan.plan_hash;
  const planning = await waitFor(
    "completed planning materials",
    async () => {
      const value = await core("GET", `/agentic/requests/${requestId}/planning`);
      if (value.reviewStatus === "PENDING" && value.materialsReady === true) {
        fail(`owner-required plan review blocked the one-prompt positive lane: ${JSON.stringify(value)}`);
      }
      if (
        value.materialsReady === true &&
        value.reviewApproved === true &&
        (value.missingArtifacts ?? []).length === 0 &&
        value.participantPlan?.ready === true
      ) {
        return value;
      }
      return null;
    },
    timeoutMs,
    1000,
  );
  if (planning.materialsReady !== true) fail("planning materials are not ready");
  if (planning.reviewApproved !== true) fail("current plan review is not approved");
  if (planning.reviewCurrent === false) fail("plan review is stale for the current revision");
  if ((planning.missingArtifacts ?? []).length > 0) {
    fail(`planning still misses ${planning.missingArtifacts.join(", ")}`);
  }
  if (planning.participantPlan?.ready !== true) fail("participant plan is not ready");
  record.assertions.start_ready = planning.startReady === true
    ? "PASS"
    : "OBSERVED_VIA_CAMPAIGN";
  record.assertions.review_current = planning.reviewCurrent ?? "NOT_EXPOSED";
  record.plan.graph_sha256 = (await core("GET", `/agentic/requests/${requestId}/plan`))
    .graph?.graph_sha256;
  if (!record.plan.graph_sha256) fail("WorkGraph is missing");
  const spec = await waitFor(
    "frozen specification",
    async () => {
      const value = await core("GET", `/agentic/requests/${requestId}/spec`);
      const sha =
        value.frozen?.record?.spec_sha256 ?? value.frozen?.spec_sha256 ?? null;
      if (planning.lane === "FORMAL_SPEC" && !sha) return null;
      return value;
    },
    timeoutMs,
    1000,
  );
  record.plan.spec_sha256 =
    spec.frozen?.record?.spec_sha256 ?? spec.frozen?.spec_sha256 ?? null;
  if (planning.lane === "FORMAL_SPEC" && !record.plan.spec_sha256) {
    fail("FORMAL_SPEC did not freeze a specification");
  }
  const environment = await waitFor(
    "frozen environment",
    async () => {
      const value = await core(
        "GET",
        `/agentic/requests/${requestId}/environment`,
      );
      return value.status === "FROZEN" ? value : null;
    },
    timeoutMs,
    1000,
  );
  record.plan.environment_spec_sha256 = environment.specSha256 ?? null;
  record.plan.environment_plan_sha256 = environment.planSha256 ?? null;
  record.assertions.environment_frozen = "PASS";
  record.assertions.spec_frozen = record.plan.spec_sha256 ? "PASS" : "NOT_REQUIRED";
  return { request, planning };
}

async function waitForRequestExecution(requestId) {
  return waitFor(
    "AUTONOMOUS request execution",
    async () => {
      const projection = await core("GET", `/intake/requests/${requestId}`);
      if (["BLOCKED", "FAILED"].includes(projection.state)) {
        fail(
          `request ${projection.state}: ${projection.failureReason ?? projection.failure_reason ?? "unknown"}`,
        );
      }
      if (projection.campaignId) return projection;
      return null;
    },
    timeoutMs,
    1000,
  );
}

async function waitForCampaignAndGate(campaignId) {
  const candidateCampaign = await waitFor(
    "Campaign candidate",
    async () => {
      const detail = await core("GET", `/campaigns/${campaignId}`);
      return detail.candidates?.length > 0 && detail.attempts?.length > 0
        ? detail
        : null;
    },
    timeoutMs,
    2000,
  );
  const campaign = candidateCampaign.campaign;
  const authorization = assertCampaignAuthorization(candidateCampaign.authorization);
  if (!["AUTHORIZED", "RUNNING", "COMPLETED"].includes(campaign.status)) {
    fail(`Campaign status is not authorized: ${campaign.status}`);
  }
  const candidate = candidateCampaign.candidates.at(-1);
  const attempt = candidateCampaign.attempts.at(-1);
  record.ids.campaign_id = campaignId;
  record.ids.run_id = attempt.run_id;
  record.ids.attempt_id = attempt.id;
  record.ids.candidate_id = candidate.id;
  record.ids.candidate_commit = candidate.commit_sha;
  record.campaign = {
    status: campaign.status,
    authorization,
    action_id: campaign.action_id,
    task_count: candidateCampaign.tasks?.length ?? 0,
    attempt_count: candidateCampaign.attempts.length,
  };
  record.assertions.campaign_authorization = "PASS";

  const gate = await waitFor(
    "Quality Gate",
    async () => {
      const value = await core(
        "GET",
        `/quality/candidates/${candidate.id}/gate`,
      ).catch(() => null);
      return value && ["ACCEPTED", "REJECTED"].includes(value.outcome)
        ? value
        : null;
    },
    timeoutMs,
    2000,
  );
  if (gate.outcome !== "ACCEPTED") {
    fail(`Quality Gate outcome is ${gate.outcome}: ${JSON.stringify(gate)}`);
  }
  const receipts = await core(
    "GET",
    `/quality/candidates/${candidate.id}/receipts`,
  );
  const verifier = receipts.find((receipt) => receipt.role === "verifier");
  const reviewer = receipts.find((receipt) => receipt.role === "reviewer");
  if (!verifier) fail("verifier receipt is missing");
  if (!reviewer) fail("reviewer receipt is missing");
  if (verifier.result !== "PASS") fail(`verifier result is ${verifier.result}`);
  if (reviewer.result !== "PASS") fail(`reviewer result is ${reviewer.result}`);
  if (reviewer.mechanism !== "MODEL") fail("reviewer receipt is not MODEL");
  record.ids.verifier_receipt = verifier.receipt_id;
  record.ids.reviewer_receipt = reviewer.receipt_id;
  record.ids.gate = gate.outcome;
  record.assertions.verifier_receipt = "PASS";
  record.assertions.reviewer_receipt = "PASS";
  record.assertions.quality_gate = "PASS";

  const diff = await core(
    "GET",
    `/campaigns/${campaignId}/candidates/${candidate.id}/diff`,
  );
  const changedPaths = diff.changed_paths ?? diff.changedPaths ?? [];
  if (!changedPaths.includes("src/provider.txt")) {
    fail(`candidate diff does not include src/provider.txt: ${JSON.stringify(changedPaths)}`);
  }
  const task = (candidateCampaign.tasks ?? []).find(
    (item) => item.candidate_id === candidate.id,
  );
  if (task?.status !== "SUCCEEDED") fail(`Campaign task is ${task?.status}`);
  record.assertions.candidate_lineage = "PASS";
  return { campaign: candidateCampaign, candidate, attempt, gate, verifier, reviewer };
}

async function captureProjectionAndBody(requestId) {
  const projection = await waitFor(
    "completed request projection",
    async () => {
      const value = await core("GET", `/intake/requests/${requestId}`);
      return value.state === "COMPLETED" ? value : null;
    },
    120_000,
    1000,
  );
  const body = await bodyText();
  assertNoManualControls(body, MANUAL_CONTROL_LABELS);
  requireAnyLabel(body, OPEN_CAMPAIGN_LABELS, "open campaign link");
  const participants = await core(
    "GET",
    `/office/participants?campaign_id=${record.ids.campaign_id}&limit=50`,
  );
  if (!participants.participants?.length) {
    fail("participants projection is empty for a completed Campaign");
  }
  record.lease = assertLeaseProjection(
    projection.state,
    participants.participants,
  );
  record.assertions.lease_vs_request = "PASS";
  record.assertions.no_routine_manual_controls = "PASS";
  return { projection, body };
}

async function collectSnapshot(requestId, campaignId, requestProjection) {
  const diagnostics = await readFixtureDiagnostics();
  assertNoFixtureErrors(diagnostics);
  const reservations = await core("GET", "/provider/turn_reservations");
  if (!Array.isArray(reservations)) fail("provider reservations are not a list");
  const campaignDetail = await core("GET", `/campaigns/${campaignId}`);
  const gate = await core(
    "GET",
    `/quality/candidates/${record.ids.candidate_id}/gate`,
  );
  const reservationsCanonical = reservations.map((reservation) => ({
    reservation_id: reservation.reservation_id,
    state: reservation.state,
    updated_at: reservation.updated_at,
    input_tokens: reservation.input_tokens,
    output_tokens: reservation.output_tokens,
  }));
  return {
    ids: { ...record.ids },
    plan: { ...record.plan },
    request_state: requestProjection.state,
    campaign_status: campaignDetail.campaign.status,
    attempt_count: campaignDetail.attempts?.length ?? 0,
    counters: diagnostics.counters,
    reservations: reservationsCanonical,
    gate: gate.outcome,
  };
}

async function runPositive() {
  stage = "license-fixture";
  if (negative !== "no-policy" && !(await requireLicenseFixtureOrBlock())) return;
  stage = "first-run-enrollment";
  await enrollIfNeeded({ requireFresh: true });
  stage = "project-import";
  const projectId = await importProject();
  stage = "project-profile-setup";
  const { projectConfig, profile } = await configureProjectAndProfile(projectId);
  stage = "model-context";
  const template = writeModelContext(projectId, projectConfig);
  const before = await readFixtureDiagnostics();
  assertCountersBaseline(before.counters);
  record.counters.before = before.counters;
  stage = "one-prompt";
  await homeAndSubmit(projectId, profile);
  if (negative === "no-policy") {
    stage = "no-policy";
    await deferPolicyAndAssertNoState();
    return;
  }
  stage = "policy-enable";
  const requestId = await enablePolicyAndOpenRequest();
  const created = await core("GET", `/intake/requests/${requestId}`);
  writeRequestContext(requestId, created, template);
  if (process.env.FACTORY_UAT_RELOAD_PREPARATION === "1") {
    stage = "reload-preparation";
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.waitForURL(new RegExp(`#/requests/${requestId}`), {
      timeout: 30_000,
    });
    const reloaded = await bodyText();
    assertNoManualControls(reloaded, MANUAL_CONTROL_LABELS);
    record.assertions.reload_same_request = "PASS";
  }
  stage = "plan-environment-spec";
  await capturePlanAndEnvironment(requestId);
  stage = "autopilot-execution";
  const projection = await waitForRequestExecution(requestId);
  const body = await bodyText();
  assertNoManualControls(body, MANUAL_CONTROL_LABELS);
  if (body.includes(WAITING_OWNER_LABELS[0]) || body.includes(WAITING_OWNER_LABELS[1])) {
    fail("positive one-prompt lane reached waiting-owner instead of auto-start");
  }
  stage = "campaign-gate";
  const campaignId = projection.campaignId;
  if (!campaignId) fail("request has no Campaign id");
  const executed = await waitForCampaignAndGate(campaignId);
  stage = "request-completed";
  const completed = await captureProjectionAndBody(requestId);
  stage = "snapshot";
  const after = await readFixtureDiagnostics();
  assertNoFixtureErrors(after);
  record.counters.after = after.counters;
  record.counters.delta = assertCounterDelta(before.counters, after.counters);
  const snapshot = await collectSnapshot(
    requestId,
    campaignId,
    completed.projection,
  );
  writeJson(snapshotPath, snapshot);
  const state = await context.storageState();
  writeJson(storageStatePath, state);
  record.ids = { ...record.ids, ...snapshot.ids };
  record.plan = { ...record.plan, ...snapshot.plan };
  record.assertions.fixture_counters = "PASS";
  record.assertions.restart_snapshot = "PASS";
  record.campaign_execution = {
    candidate: executed.candidate.id,
    attempt: executed.attempt.id,
    gate: executed.gate.outcome,
  };
  record.result = "PASS";
}

async function runVerifyRestart() {
  if (!existsSync(snapshotPath)) fail("restart snapshot is missing");
  if (!existsSync(storageStatePath)) fail("restart storageState is missing");
  const before = readJson(snapshotPath);
  stage = "restart-reauthentication";
  await page.goto(`${proxy.url}/#/requests/${before.ids.request_id}`, {
    waitUntil: "domcontentloaded",
  });
  if (await isVisible(page.getByRole("heading", { name: "Set up this Factory" }))) {
    fail("the enrolled session did not survive the Core restart; no second bootstrap is allowed");
  }
  await page
    .getByRole("heading", { name: "Request", level: 1 })
    .waitFor({ timeout: 30_000 })
    .catch(() => {});
  stage = "restart-state";
  const projection = await core("GET", `/intake/requests/${before.ids.request_id}`);
  const campaign = await core("GET", `/campaigns/${before.ids.campaign_id}`);
  const gate = await core(
    "GET",
    `/quality/candidates/${before.ids.candidate_id}/gate`,
  );
  const receipts = await core(
    "GET",
    `/quality/candidates/${before.ids.candidate_id}/receipts`,
  );
  const verifier = receipts.find((receipt) => receipt.role === "verifier");
  const reviewer = receipts.find((receipt) => receipt.role === "reviewer");
  if (!verifier || !reviewer) fail("receipts did not survive the Core restart");
  const graph = await core(
    "GET",
    `/agentic/requests/${before.ids.request_id}/plan`,
  );
  const spec = await core(
    "GET",
    `/agentic/requests/${before.ids.request_id}/spec`,
  );
  const environment = await core(
    "GET",
    `/agentic/requests/${before.ids.request_id}/environment`,
  );
  const diagnostics = await readFixtureDiagnostics();
  assertNoFixtureErrors(diagnostics);
  const reservations = await core("GET", "/provider/turn_reservations");
  const attempt = campaign.attempts?.at(-1);
  const candidate = campaign.candidates?.at(-1);
  const after = {
    ids: {
      ...before.ids,
      request_id: projection.id ?? before.ids.request_id,
      action_id: projection.actionId ?? before.ids.action_id,
      lead_id: projection.leadRequestId ?? before.ids.lead_id,
      campaign_id: projection.campaignId ?? before.ids.campaign_id,
      run_id: attempt?.run_id ?? before.ids.run_id,
      attempt_id: attempt?.id ?? before.ids.attempt_id,
      candidate_id: candidate?.id ?? before.ids.candidate_id,
      candidate_commit: candidate?.commit_sha ?? before.ids.candidate_commit,
      verifier_receipt: verifier.receipt_id,
      reviewer_receipt: reviewer.receipt_id,
      gate: gate.outcome,
    },
    plan: {
      ...before.plan,
      plan_hash: projection.plan?.plan_hash ?? before.plan.plan_hash,
      graph_sha256: graph.graph?.graph_sha256 ?? before.plan.graph_sha256,
      spec_sha256:
        spec.frozen?.record?.spec_sha256 ??
        spec.frozen?.spec_sha256 ??
        before.plan.spec_sha256,
      environment_spec_sha256:
        environment.specSha256 ?? before.plan.environment_spec_sha256,
    },
    request_state: projection.state,
    campaign_status: campaign.campaign.status,
    attempt_count: campaign.attempts?.length ?? 0,
    counters: diagnostics.counters,
    reservations: reservations.map((reservation) => ({
      reservation_id: reservation.reservation_id,
      state: reservation.state,
      updated_at: reservation.updated_at,
      input_tokens: reservation.input_tokens,
      output_tokens: reservation.output_tokens,
    })),
    gate: gate.outcome,
  };
  assertStableSnapshot(before, after);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForTimeout(1000);
  const body = await bodyText();
  if (await isVisible(page.getByRole("heading", { name: "Enable autonomous R0 execution", level: 3 }))) {
    fail("the policy confirmation was requested again after restart");
  }
  assertNoManualControls(body, MANUAL_CONTROL_LABELS);
  requireAnyLabel(body, OPEN_CAMPAIGN_LABELS, "open campaign after restart");
  record.ids = { ...after.ids };
  record.plan = { ...after.plan };
  record.counters.after = after.counters;
  record.counters.before = before.counters;
  assert.deepEqual(after.counters, before.counters, "counter changed after restart");
  record.counters.delta = Object.fromEntries(
    Object.keys(after.counters).map((key) => [
      key,
      (after.counters[key] ?? 0) - (before.counters[key] ?? 0),
    ]),
  );
  record.assertions.restart_identity = "PASS";
  record.assertions.restart_no_second_model = "PASS";
  record.result = "PASS";
  if (process.env.FACTORY_UAT_KEEP_STORAGE !== "1") {
    rmSync(storageStatePath, { force: true });
  }
}

async function runNegativeLicenseHeader() {
  // This lane depends on the Core98 fixture variant. It never mutates product
  // state itself; it only submits one prompt and reads the typed blocker.
  if (negative !== "license-header") fail("license lane requires FACTORY_UAT_NEGATIVE=license-header");
  stage = "license-fixture";
  if (!(await requireLicenseFixtureOrBlock())) return;
  await enrollIfNeeded({ requireFresh: true });
  const projectId = await importProject();
  const { projectConfig, profile } = await configureProjectAndProfile(projectId);
  writeModelContext(projectId, projectConfig);
  const before = await readFixtureDiagnostics();
  assertCountersBaseline(before.counters);
  await homeAndSubmit(projectId, profile);
  const requestId = await enablePolicyAndOpenRequest();
  stage = "license-header-stop";
  const blocked = await waitFor(
    "typed licence blocker",
    async () => {
      const value = await core("GET", `/intake/requests/${requestId}`);
      if (["BLOCKED", "FAILED"].includes(value.state)) return value;
      if (value.campaignId) fail("licence-header negative created a Campaign");
      return null;
    },
    timeoutMs,
    1000,
  );
  const body = await bodyText();
  const blocker = `${blocked.failureReason ?? blocked.failure_reason ?? ""} ${body}`;
  if (!/LICENSE|LICENCE|ENVIRONMENT_LICENSE|ed\.h/i.test(blocker)) {
    fail(`typed licence blocker is not visible: ${blocker.slice(0, 700)}`);
  }
  const diagnostics = await readFixtureDiagnostics();
  assertNoFixtureErrors(diagnostics);
  const delta = Object.fromEntries(
    Object.keys(diagnostics.counters).map((key) => [
      key,
      (diagnostics.counters[key] ?? 0) - (before.counters[key] ?? 0),
    ]),
  );
  if ((delta.license_evidence_review ?? 0) !== 1) {
    fail("licence-header negative did not execute exactly one licence evidence review");
  }
  if ((delta.other ?? 0) !== 0) fail("licence-header negative produced an unknown prompt");
  record.assertions.typed_license_stop = "PASS";
  record.result = "PASS";
}

async function runLeaseNegative() {
  stage = "license-fixture";
  if (!(await requireLicenseFixtureOrBlock())) return;
  await enrollIfNeeded({ requireFresh: true });
  const projectId = await importProject();
  const { projectConfig, profile } = await configureProjectAndProfile(projectId);
  const template = writeModelContext(projectId, projectConfig);
  const before = await readFixtureDiagnostics();
  assertCountersBaseline(before.counters);
  await homeAndSubmit(projectId, profile);
  const requestId = await enablePolicyAndOpenRequest();
  const created = await core("GET", `/intake/requests/${requestId}`);
  writeRequestContext(requestId, created, template);
  stage = "lease-vs-request";
  const projection = await waitForRequestExecution(requestId);
  const participants = await waitFor(
    "participant lease projection",
    async () => {
      const page = await core(
        "GET",
        `/office/participants?campaign_id=${projection.campaignId}&limit=50`,
      );
      return page.participants?.length ? page : null;
    },
    120_000,
    1000,
  );
  const current = await core("GET", `/intake/requests/${requestId}`);
  record.lease = assertLeaseProjection(current.state, participants.participants);
  record.assertions.lease_vs_request = "PASS";
  record.result = "PASS";
}

async function main() {
  const chromium = await loadChromium();
  const executablePath = chromiumExecutable();
  proxy = await startLoopbackProxy(upstream);
  browser = await chromium.launch({
    executablePath,
    headless: true,
    args: ["--no-sandbox", "--disable-dev-shm-usage"],
  });
  const options = { viewport: { width: 1440, height: 1000 } };
  if (phase === "verify-restart") {
    if (!existsSync(storageStatePath)) fail("restart storageState is missing");
    options.storageState = readJson(storageStatePath);
  }
  context = await browser.newContext(options);
  context.setDefaultTimeout(15_000);
  context.setDefaultNavigationTimeout(30_000);
  page = await context.newPage();
  page.on("console", (message) => {
    if (message.type() === "error") record.console.errors.push(message.text());
  });
  page.on("pageerror", (error) => record.console.page_errors.push(String(error)));
  page.on("requestfailed", (request) => {
    record.console.failed_requests.push(
      `${request.method()} ${request.url()} ${request.failure()?.errorText ?? ""}`,
    );
  });
  page.on("response", (response) => {
    if (response.status() === 404) record.console.http_404.push(response.url());
  });
  page.on("request", (request) => {
    const url = request.url();
    if (!url.includes("/core-api/")) return;
    record.page_mutations.push({
      method: request.method(),
      path: normalizeCorePath(url),
      driver: request.headers()["x-oneprompt-driver"] === "1",
    });
  });

  if (phase === "run") {
    if (negative === "license-header") {
      await runNegativeLicenseHeader();
    } else if (negative === "funding-false") {
      await runFundingObservation();
    } else if (negative === "lease-vs-request") {
      await runLeaseNegative();
    } else {
      await runPositive();
    }
  } else {
    await runVerifyRestart();
  }

  if (record.result === "BLOCKED") {
    record.assertions.driver_calls = "BLOCKED_BEFORE_VALIDATION";
    record.assertions.page_mutations = "BLOCKED_BEFORE_VALIDATION";
  } else {
    record.assertions.driver_calls = validateDriverCalls(record.driver_calls);
    const pageMutations = record.page_mutations.filter((entry) => !entry.driver);
    if (
      phase === "verify-restart" ||
      negative === "no-policy" ||
      negative === "funding-false"
    ) {
      const unexpected = pageMutations.filter(
        (entry) =>
          !["GET", "HEAD", "OPTIONS"].includes(entry.method) &&
          isWorkflowPath(entry.path),
      );
      if (unexpected.length > 0) {
        fail(
          `negative/restart lane performed workflow UI mutation(s): ${unexpected
            .map((entry) => `${entry.method} ${entry.path}`)
            .join("; ")}`,
        );
      }
      record.assertions.page_mutations = "NO_WORKFLOW_MUTATION";
    } else {
      record.assertions.page_mutations = validatePageMutations(pageMutations);
    }
  }
  if (record.console.errors.length > 0) {
    fail(`browser console errors: ${record.console.errors.slice(0, 3).join(" | ")}`);
  }
  if (record.console.page_errors.length > 0) {
    fail(`browser page errors: ${record.console.page_errors.slice(0, 3).join(" | ")}`);
  }
  const expectedGatePoll = (url) =>
    /\/core-api\/quality\/candidates\/[^/]+\/gate$/.test(url);
  const unexpected404 = record.console.http_404.filter(
    (url) => !expectedGatePoll(url),
  );
  if (unexpected404.length > 0) {
    fail(`unexpected browser 404(s): ${unexpected404.slice(0, 3).join(" | ")}`);
  }
  record.assertions.console = "PASS";
}

async function runFundingObservation() {
  await enrollIfNeeded({ requireFresh: false });
  stage = "funding-configuration";
  await core("PUT", "/settings/providers/opencode-go", {
    model: "deepseek-v4.1-flash",
    baseUrl: `${fixtureUrl}/v1`,
    credential: "compose-fixture-credential-0123456789",
    enabled: true,
    isDefault: true,
  });
  stage = "funding-observation";
  const funding = await core("GET", "/funding/subscription/opencode-go").catch(
    () => null,
  );
  if (funding === null) {
    record.assertions.funding_projection = "BLOCKED";
    record.findings.push({
      kind: "funding-endpoint-unavailable",
      detail: "GET /funding/subscription/opencode-go did not return a projection",
      required_owner: "b786195c",
    });
    record.result = "BLOCKED";
    return;
  }
  let projection;
  try {
    projection = assertFundingProjection(funding);
  } catch (error) {
    record.assertions.funding_projection = "FAIL";
    record.error = error instanceof Error ? error.message : String(error);
    record.findings.push({
      kind: "funding-projection-not-truthful",
      detail: record.error,
      required_owner: "b786195c",
    });
    record.result = "BLOCKED";
    return;
  }
  const body = await bodyText();
  const showsFunding = /funding/i.test(body);
  const showsTruth =
    /admissib/i.test(body) &&
    /(missing evidence|entitlement|core.?verif)/i.test(body);
  if (!(showsFunding && showsTruth)) {
    record.assertions.funding_projection = "BLOCKED";
    record.findings.push({
      kind: "office-funding-projection-missing",
      detail:
        "Core exposes the truthful funding fields but Office does not display funding + admissibility + missing evidence/entitlement/core-verifiable",
      required_owner: "b786195c",
      observed: { showsFunding, showsTruth },
    });
    record.result = "BLOCKED";
    return;
  }
  record.assertions.funding_projection = "PASS";
  record.funding = projection;
  record.result = "PASS";
}

try {
  await main();
  record.finished_at = new Date().toISOString();
  writeJson(join(evidenceDir, `oneprompt-${phase}.json`), record);
  process.stdout.write(
    `${JSON.stringify({
      result: record.result,
      phase,
      negative: record.negative || null,
      ids: record.ids,
      evidence: join(evidenceDir, `oneprompt-${phase}.json`),
    })}\n`,
  );
  if (!["PASS", "PARTIAL", "NOT_TESTED"].includes(record.result)) {
    process.exitCode = 1;
  }
} catch (error) {
  record.result = "FAIL";
  record.stage = stage;
  record.error = error instanceof Error ? error.message : String(error);
  record.finished_at = new Date().toISOString();
  if (page) {
    await shot(`failure-${phase}-${stage.replace(/[^a-z0-9-]+/gi, "-")}.png`).catch(
      () => {},
    );
  }
  writeJson(join(evidenceDir, `oneprompt-${phase}.json`), record);
  process.stderr.write(`one-prompt lane failed at ${stage}: ${record.error}\n`);
  process.exitCode = 1;
} finally {
  if (browser) {
    await browser.close().catch(() => {});
  }
  if (proxy) {
    await proxy.close().catch(() => {});
  }
}
