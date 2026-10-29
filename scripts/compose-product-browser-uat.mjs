// Real-product Compose browser UAT.
//
// This harness drives the Office served by the real Compose Core. It never
// intercepts or rewrites /core-api requests: every read and mutation below is
// an authenticated Core call made by the real page origin. The deterministic
// fixture provider executes the WorkUnit, and the Compose model fixture serves
// Lead assessment and semantic review. No paid inference is performed.
//
// Required environment:
//   FACTORY_OFFICE_URL       published Office URL, for example http://127.0.0.1:8898
//   FACTORY_BOOTSTRAP_CODE   one-time owner confirmation from the Core container
//   CHROMIUM_EXE             real Chromium/Chrome executable
//   FACTORY_EVIDENCE_DIR     directory for screenshots and the JSON result
//   FACTORY_MODEL_CONTEXT    host path of the model-fixture context.json
// Optional:
//   FACTORY_GIT_REPOSITORY   fixture Git URL (default http://git-fixture:8080/repo.git)
//   FACTORY_UAT_OBJECTIVE    exact objective (default includes a timestamp)
//   FACTORY_UAT_CANDIDATE    candidate SHA under test (recorded only)
//   FACTORY_UAT_COMPOSE_PROJECT / FACTORY_UAT_COMPOSE_FILES (recorded only)
//   FACTORY_UAT_TIMEOUT_SECS gate wait bound (default 900)
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "../office/e2e-browser/node_modules/playwright-core/index.mjs";

const base = process.env.FACTORY_OFFICE_URL || "http://127.0.0.1:8893";
const bootstrapCode = process.env.FACTORY_BOOTSTRAP_CODE;
const executablePath = process.env.CHROMIUM_EXE;
const evidenceDir =
  process.env.FACTORY_EVIDENCE_DIR ||
  join(process.cwd(), "docs/evidence/compose-browser-uat-2026-10-29");
const modelContextPath = process.env.FACTORY_MODEL_CONTEXT;
const gitRepository =
  process.env.FACTORY_GIT_REPOSITORY || "http://git-fixture:8080/repo.git";
const objective =
  process.env.FACTORY_UAT_OBJECTIVE ||
  `Add the deterministic Compose browser UAT candidate ${Date.now()} and cover it with frozen checks.`;
const gateTimeoutMs =
  Number(process.env.FACTORY_UAT_TIMEOUT_SECS || "900") * 1000;
const candidateSha = process.env.FACTORY_UAT_CANDIDATE || "";
const composeProject =
  process.env.FACTORY_UAT_COMPOSE_PROJECT || "software-factory-uat60";
const composeFiles =
  process.env.FACTORY_UAT_COMPOSE_FILES || "compose.yaml compose.e2e.yaml";

for (const [name, value] of [
  ["FACTORY_BOOTSTRAP_CODE", bootstrapCode],
  ["CHROMIUM_EXE", executablePath],
  ["FACTORY_MODEL_CONTEXT", modelContextPath],
]) {
  if (!value) throw new Error(`${name} is required`);
}
mkdirSync(evidenceDir, { recursive: true });

const record = {
  schema: "factory.compose-browser-uat.v1",
  result: "NOT_RUN",
  started_at: new Date().toISOString(),
  finished_at: null,
  candidate_sha: candidateSha,
  office_url: base,
  objective,
  runtime: {
    compose_project: composeProject,
    compose_files: composeFiles.split(/\s+/),
    provider_kind: "fixture",
    paid_inference: false,
    deterministic_model_gateway: "http://model-fixture:8080/v1",
    core_api_interception: false,
  },
  ids: {
    project: null,
    intake_request: null,
    action: null,
    lead_request: null,
    execution_profile: null,
    campaign: null,
    run: null,
    attempt: null,
    candidate: null,
    candidate_commit: null,
    verifier_receipt: null,
    reviewer_receipt: null,
    gate: null,
  },
  assertions: {},
  screenshots: [],
  console: { errors: [], page_errors: [], failed_requests: [], http_404: [] },
  findings: [],
  commands: [
    `podman compose -p ${composeProject} -f compose.yaml -f compose.e2e.yaml up -d --build`,
    `podman exec <core> /usr/local/bin/factory-core issue-bootstrap-code`,
    `FACTORY_OFFICE_URL=${base} node scripts/compose-product-browser-uat.mjs`,
  ],
  boundary: {
    real_core_http_api: true,
    real_chromium: true,
    intercepted_core_api: false,
    deterministic_fixture_provider: true,
    deterministic_model_fixture: true,
    live_provider_inference: "NOT_TESTED",
    github_pr_created_or_merged: false,
    github_pr_projection: "UNBOUND_NOT_TESTED",
    independent_acceptance: "NOT_TESTED",
    isolation: "NOT_CLAIMED",
  },
};

const consoleErrors = record.console.errors;
const pageErrors = record.console.page_errors;
const failedRequests = record.console.failed_requests;
const notFound = record.console.http_404;

let browser;
let page;
let stage = "launch";

function errorText(cause) {
  if (cause instanceof Error) return cause.message;
  return String(cause);
}

async function shot(name) {
  const path = join(evidenceDir, name);
  await page.screenshot({ path, fullPage: true });
  record.screenshots.push({ name, path });
  return path;
}

async function capturePair(name) {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.waitForTimeout(250);
  await shot(`${name}-1440.png`);
  await page.setViewportSize({ width: 390, height: 900 });
  await page.waitForTimeout(350);
  const layout = await page.evaluate(() => ({
    scrollWidth: document.documentElement.scrollWidth,
    clientWidth: document.documentElement.clientWidth,
  }));
  await shot(`${name}-390.png`);
  if (layout.scrollWidth > layout.clientWidth + 1) {
    record.findings.push({
      kind: "horizontal-overflow",
      screen: name,
      width: 390,
      scrollWidth: layout.scrollWidth,
      clientWidth: layout.clientWidth,
    });
  }
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.waitForTimeout(150);
}

// Real same-origin Core request from the enrolled browser. The Core CSRF
// cookie is readable by the page; the session cookie stays HttpOnly.
async function core(method, path, body) {
  const result = await page.evaluate(
    async ({ method, path, body }) => {
      const csrf = document.cookie
        .split("; ")
        .find((item) => item.startsWith("factory_csrf="))
        ?.slice("factory_csrf=".length);
      const headers = { "content-type": "application/json" };
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
    { method, path, body },
  );
  if (result.status >= 400) {
    throw new Error(
      `Core ${method} ${path} -> ${result.status}: ${JSON.stringify(
        result.payload,
      ).slice(0, 600)}`,
    );
  }
  return result.payload;
}

async function waitFor(label, operation, timeoutMs = 120_000, intervalMs = 1000) {
  const deadline = Date.now() + timeoutMs;
  let lastError = null;
  while (Date.now() < deadline) {
    try {
      const value = await operation();
      if (value) return value;
    } catch (cause) {
      lastError = cause;
    }
    await page.waitForTimeout(intervalMs);
  }
  throw new Error(
    `${label} timed out after ${timeoutMs}ms${
      lastError ? `: ${errorText(lastError)}` : ""
    }`,
  );
}

try {
  browser = await chromium.launch({
    executablePath,
    headless: true,
    args: ["--no-sandbox", "--disable-dev-shm-usage"],
  });
  page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });
  page.on("pageerror", (error) => pageErrors.push(String(error)));
  page.on("requestfailed", (request) => {
    failedRequests.push(
      `${request.method()} ${request.url()} ${request.failure()?.errorText ?? ""}`,
    );
  });
  page.on("response", (response) => {
    if (response.status() === 404) notFound.push(response.url());
  });

  stage = "first-run-enrollment";
  await page.goto(base, { waitUntil: "domcontentloaded" });
  const gate = page.locator(".gate");
  const gateVisible = await gate.isVisible().catch(() => false);
  assert.equal(gateVisible, true, "the stack must be in first-run setup");
  await page
    .getByRole("heading", { name: "Set up this Factory", level: 1 })
    .waitFor({ timeout: 20_000 });
  await page
    .getByLabel("One-time owner confirmation")
    .fill(bootstrapCode);
  await page.getByLabel("This device name").fill("Wave 3 Compose Browser UAT");
  await page.getByRole("button", { name: "Enroll this device" }).click();
  await page.getByRole("heading", { name: "Home", level: 1 }).waitFor({
    timeout: 30_000,
  });
  record.assertions.owner_enrollment = "PASS";
  await shot("home-first-run-1440.png");

  stage = "project-import";
  await page.getByRole("link", { name: "Projects", exact: true }).click();
  await page.getByRole("heading", { name: "Projects", level: 1 }).waitFor();
  await page.getByRole("tab", { name: "Public Git URL" }).click();
  await page.getByTestId("project-repository").fill(gitRepository);
  await page.getByRole("button", { name: "Register Project" }).click();
  await page.waitForURL(/#\/projects\/[^/?]+/, { timeout: 120_000 });
  const projectId = page.url().match(/#\/projects\/([^/?]+)/)?.[1];
  assert.ok(projectId, "project import must navigate to the imported Project");
  record.ids.project = projectId;
  record.assertions.project_import = "PASS";

  stage = "project-configuration";
  const initialConfig = await core("GET", `/projects/${projectId}/config`);
  const projectConfig = await core(
    "PUT",
    `/projects/${projectId}/config`,
    {
      expectedRevision: initialConfig.revision,
      allowedPaths: ["src"],
      capabilities: ["repository-edit", "generic-toolchain"],
      qualityProfile: "generic",
      active: true,
    },
  );
  assert.equal(projectConfig.active, true);
  assert.deepEqual(projectConfig.allowedPaths, ["src"]);
  record.assertions.project_configuration = "PASS";

  stage = "provider-and-profile-setup";
  await core("PUT", "/settings/providers/opencode", {
    model: "deepseek-v4.1-flash",
    baseUrl: "https://opencode.ai/inference/go/openai/v1",
    credential: "compose-fixture-credential-0123456789",
    enabled: true,
    isDefault: false,
    maxTurns: 4,
    maxInputTokens: 20000,
    maxOutputTokens: 8000,
    maxCostMicros: 500000,
    inputMicrosPerMillion: 1000,
    outputMicrosPerMillion: 2000,
  });
  await core("PUT", "/settings/providers/opencode-go", {
    model: "deepseek-v4.1-flash",
    baseUrl: "http://model-fixture:8080/v1",
    credential: "compose-fixture-credential-0123456789",
    enabled: true,
    isDefault: true,
    maxTurns: 4,
    maxInputTokens: 100000,
    maxOutputTokens: 12000,
    maxCostMicros: 500000,
    inputMicrosPerMillion: 1000,
    outputMicrosPerMillion: 2000,
  });
  await core("PUT", "/settings/runtime", {
    semanticReviewEnabled: true,
    semanticReviewMaxOutputTokens: 8192,
  });
  const role = {
    provider: "opencode-go",
    model: "deepseek-v4.1-flash",
    reasoning: "medium",
  };
  const profile = await core("POST", "/execution-profiles", {
    name: "Compose Browser UAT Balanced",
    description: "Deterministic Compose browser UAT profile",
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
    auto_start: { enabled: false, risk_classes: [] },
  });
  assert.equal(profile.executable, true, "execution profile must be executable");
  record.ids.execution_profile = profile.id;
  await core("POST", `/execution-profiles/${profile.id}/default`, {
    scope: "global",
  });
  const policy = await core(
    "POST",
    `/projects/${projectId}/execution-policy`,
    {
      execution_profile_id: profile.id,
      expected_revision: profile.revision,
      expected_revision_hash: profile.contentHash,
      project_config_hash: projectConfig.configHash,
      acknowledge: true,
      idempotency_key: "compose-browser-uat-policy",
    },
  );
  assert.ok(policy, "execution policy must be recorded");
  record.assertions.provider_and_profile_setup = "PASS";

  stage = "home-request";
  await page.getByRole("link", { name: "Home", exact: true }).click();
  await page.getByRole("heading", { name: "Home", level: 1 }).waitFor();
  await page.locator("#home-project").selectOption(projectId);
  await page.locator("#home-objective").fill(objective);
  await page.locator("#home-spend").fill("5.00");
  await page.locator("#home-profile").selectOption(profile.id);
  await capturePair("home");
  await page.getByRole("button", { name: "Submit request" }).click();
  await page.waitForURL(/#\/requests\/[^/?]+/, { timeout: 120_000 });
  const intakeId = page.url().match(/#\/requests\/([^/?]+)/)?.[1];
  assert.ok(intakeId, "request creation must navigate to the request workspace");
  record.ids.intake_request = intakeId;
  const created = await core("GET", `/intake/requests/${intakeId}`);
  record.ids.action = created.actionId;
  record.ids.lead_request = created.leadRequestId;
  assert.equal(created.state, "SUBMITTED");
  assert.equal(created.mode, "REVIEW_PLAN");
  assert.equal(created.objective, objective);
  record.assertions.home_request_creation = "PASS";
  record.spend = {
    requested_usd: "5.00",
    authorized_micros: 5_000_000,
    billable_gateway: "deterministic model fixture only",
    paid_inference: false,
  };
  await shot("request-created-1440.png");

  stage = "assessment";
  const context = {
    schema: "factory-compose-model-fixture-v1",
    projectId,
    intakeRequestId: created.id,
    requestId: created.leadRequestId,
    actionId: created.actionId,
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
  writeFileSync(modelContextPath, `${JSON.stringify(context, null, 2)}\n`);
  await page.getByRole("button", { name: "Run assessment" }).click();
  await page
    .getByRole("button", { name: "Approve frozen plan and start" })
    .waitFor({ timeout: 180_000 });
  const assessed = await core("GET", `/intake/requests/${intakeId}`);
  assert.equal(assessed.state, "AWAITING_APPROVAL");
  assert.ok(assessed.plan?.plan_hash, "Core must freeze an exact plan hash");
  record.assertions.assessment = "PASS";
  record.plan_hash = assessed.plan.plan_hash;
  await capturePair("request-assessment");

  stage = "plan-approval";
  await page
    .getByLabel("Approval rationale")
    .fill("Reviewed the exact frozen Compose browser UAT plan.");
  await page
    .getByLabel("I reviewed the frozen scope, route, budget and acceptance plan.")
    .check();
  await page.getByRole("button", { name: "Approve frozen plan and start" }).click();
  await page
    .getByRole("link", { name: "Open Campaign", exact: false })
    .first()
    .waitFor({ timeout: 120_000 });
  const approved = await core("GET", `/intake/requests/${intakeId}`);
  assert.ok(approved.campaignId, "approval must start a Campaign");
  record.ids.campaign = approved.campaignId;
  record.assertions.plan_approval_and_campaign_start = "PASS";

  stage = "campaign-execution";
  const campaign = await waitFor(
    "Campaign candidate",
    async () => {
      const detail = await core("GET", `/campaigns/${approved.campaignId}`);
      return detail.candidates.length > 0 && detail.attempts.length > 0
        ? detail
        : null;
    },
    gateTimeoutMs,
    2000,
  );
  const candidate = campaign.candidates.at(-1);
  const attempt = campaign.attempts.at(-1);
  record.ids.candidate = candidate.id;
  record.ids.candidate_commit = candidate.commit_sha;
  record.ids.attempt = attempt.id;
  record.ids.run = attempt.run_id;
  assert.ok(campaign.authorization, "Campaign authorization must be recorded");
  record.campaign_authorization = {
    execution_mode:
      campaign.authorization.executionMode ??
      campaign.authorization.execution_mode,
    risk_class:
      campaign.authorization.riskClass ?? campaign.authorization.risk_class,
    provider: campaign.authorization.provider,
    model: campaign.authorization.model,
    actor: campaign.authorization.actor,
  };
  assert.equal(record.campaign_authorization.risk_class, "R0");
  assert.equal(record.campaign_authorization.provider, "opencode-go");

  const participantProjection = await core(
    "GET",
    `/office/participants?campaign_id=${approved.campaignId}&limit=50`,
  );
  const implementerParticipant = participantProjection.participants.find(
    (item) => item.role === "IMPLEMENTER",
  );
  assert.ok(implementerParticipant, "the implementer participant is required");
  assert.equal(implementerParticipant.session?.state, "COMPLETED");
  assert.equal(implementerParticipant.session?.worker_id, "compose-worker");
  assert.match(
    String(implementerParticipant.session?.provider_session_id ?? ""),
    /fixture/,
    "the Worker provider session must come from the deterministic fixture adapter",
  );
  record.worker_execution = {
    worker_id: implementerParticipant.session.worker_id,
    attempt_generation: implementerParticipant.attempt_generation,
    provider_session_id: implementerParticipant.session.provider_session_id,
    session_state: implementerParticipant.session.state,
    cleanup_receipt: implementerParticipant.session.cleanup_receipt,
    usage: {
      input_tokens: implementerParticipant.session.input_tokens,
      output_tokens: implementerParticipant.session.output_tokens,
      cost_micros: implementerParticipant.session.cost_micros,
    },
  };
  record.assertions.fixture_worker_session = "PASS";

  const gateDecision = await waitFor(
    "deterministic Quality Gate",
    async () => {
      const gate = await core(
        "GET",
        `/quality/candidates/${candidate.id}/gate`,
      ).catch(() => null);
      return gate && ["ACCEPTED", "REJECTED"].includes(gate.outcome)
        ? gate
        : null;
    },
    gateTimeoutMs,
    2000,
  );
  const receipts = await core(
    "GET",
    `/quality/candidates/${candidate.id}/receipts`,
  );
  const verifierReceipt = receipts.find((item) => item.role === "verifier");
  const reviewerReceipt = receipts.find((item) => item.role === "reviewer");
  assert.ok(verifierReceipt, "an independent verifier receipt is required");
  assert.ok(reviewerReceipt, "an independent reviewer receipt is required");
  record.ids.verifier_receipt = verifierReceipt.receipt_id;
  record.ids.reviewer_receipt = reviewerReceipt.receipt_id;
  record.ids.gate = gateDecision.outcome;
  assert.equal(
    gateDecision.outcome,
    "ACCEPTED",
    `Quality Gate did not accept: ${JSON.stringify(gateDecision)}`,
  );
  assert.equal(verifierReceipt.result, "PASS");
  assert.equal(reviewerReceipt.result, "PASS");
  assert.equal(reviewerReceipt.mechanism, "MODEL");
  const diff = await core(
    "GET",
    `/campaigns/${approved.campaignId}/candidates/${candidate.id}/diff`,
  );
  const changedPaths = diff.changed_paths ?? diff.changedPaths ?? [];
  assert.ok(
    changedPaths.includes("src/provider.txt"),
    `candidate diff must contain src/provider.txt: ${JSON.stringify(changedPaths)}`,
  );
  const task = campaign.tasks.find((item) => item.candidate_id === candidate.id);
  assert.equal(task?.status, "SUCCEEDED");
  record.assertions.worker_fixture_execution = "PASS";
  record.assertions.independent_receipts = "PASS";
  record.assertions.deterministic_quality_gate = "PASS";
  record.assertions.candidate_lineage = "PASS";
  record.candidate_diff = {
    changed_paths: changedPaths,
    base_commit: diff.base_commit ?? diff.baseCommit ?? null,
    candidate_commit: candidate.commit_sha,
  };

  stage = "request-workspace-completed";
  await waitFor(
    "completed request projection",
    async () => {
      await page.waitForTimeout(1000);
      const projection = await core("GET", `/intake/requests/${intakeId}`);
      return projection.state === "COMPLETED" ? projection : null;
    },
    60_000,
  );
  await page.getByRole("heading", { name: "Completed", level: 2 }).waitFor({
    timeout: 30_000,
  });
  const requestBodyText = await page.locator("body").innerText();
  if (/Authorization\s+undefined/i.test(requestBodyText)) {
    record.findings.push({
      kind: "ui-contract-mismatch",
      component: "RequestWorkspace Campaign panel",
      observed: "Authorization undefined · undefined",
      expected: "the Core V2 authorization risk class and execution mode",
      cause:
        "office/src/coreApi.ts CampaignAuthorization declares snake_case legacy fields while Core serializes CampaignAuthorizationRecord as camelCase V2",
      impact:
        "an operator cannot read the authorization class/route from the completed request workspace",
      disposition:
        "REPORTED_NOT_FIXED: Wave 3 execution-policy/Office budget lane owns the V2 projection contract",
      evidence: "request-completed-390.png",
    });
  }
  await capturePair("request-completed");

  stage = "campaign-workspace";
  await page
    .getByRole("link", { name: "Open Campaign", exact: false })
    .first()
    .click();
  await page
    .getByRole("heading", { name: /^Campaign /, level: 1 })
    .waitFor({ timeout: 30_000 });
  await capturePair("campaign-overview");

  await page.getByRole("tab", { name: /Candidates/ }).click();
  const candidatePanel = page.getByTestId("candidate-panel").first();
  await candidatePanel.waitFor({ timeout: 30_000 });
  await page
    .getByTestId("candidate-diff")
    .getByText("src/provider.txt", { exact: false })
    .first()
    .waitFor({ timeout: 30_000 });
  const prPanel = page.getByTestId("candidate-pr").first();
  await prPanel.waitFor({ timeout: 30_000 });
  await prPanel
    .getByText(/not bound to a GitHub repository/i)
    .waitFor({ timeout: 30_000 });
  record.assertions.candidate_diff_projection = "PASS";
  record.assertions.github_pr_projection_unbound = "PASS";
  record.pr_projection = "UNBOUND_NO_GITHUB_WRITE";
  await capturePair("campaign-candidates");

  await page.getByRole("tab", { name: /Evidence/ }).click();
  const evidenceTab = page.getByRole("tabpanel");
  await evidenceTab.getByText("verifier", { exact: false }).first().waitFor({
    timeout: 30_000,
  });
  await evidenceTab.getByText("reviewer", { exact: false }).first().waitFor({
    timeout: 30_000,
  });
  await capturePair("campaign-evidence");

  await page.getByRole("tab", { name: /Quality Gate/ }).click();
  await page.getByText("ACCEPTED", { exact: true }).first().waitFor({
    timeout: 30_000,
  });
  await capturePair("campaign-gate");

  stage = "agent-observability";
  await page.getByRole("link", { name: "Agents", exact: true }).click();
  await page.getByRole("heading", { name: "Agent Office", level: 1 }).waitFor({
    timeout: 30_000,
  });
  await page
    .getByTestId("agent-filters")
    .getByLabel("Campaign", { exact: true })
    .selectOption(approved.campaignId);
  await page.getByRole("tab", { name: /Visual Office/ }).click();
  await page.getByTestId("agent-floor").waitFor({ timeout: 30_000 });
  const avatars = page.locator('[data-testid^="office-avatar-"]');
  await waitFor(
    "Agent Office participants",
    async () => ((await avatars.count()) > 0 ? true : null),
    60_000,
  );
  const participantCount = await avatars.count();
  const roleNames = await page
    .locator('[data-testid^="office-avatar-"]')
    .evaluateAll((nodes) =>
      nodes.map((node) => node.getAttribute("aria-label") ?? ""),
    );
  record.agent_office = {
    participant_count: participantCount,
    avatar_labels: roleNames,
    campaign_filter: approved.campaignId,
  };
  assert.ok(participantCount >= 1, "Agent Office must project participants");
  record.assertions.agent_observability = "PASS";
  await capturePair("agent-office");

  stage = "final-home";
  await page.getByRole("link", { name: "Home", exact: true }).click();
  await page.getByRole("heading", { name: "Home", level: 1 }).waitFor();
  await page.getByText(objective, { exact: false }).first().waitFor({
    timeout: 30_000,
  });
  await capturePair("home-completed");

  // CampaignDetail (and this harness) legitimately reads a Gate that does not
  // exist yet while a candidate is pending. Core answers 404 until the
  // deterministic Gate is recorded; that pending state is not an error.
  const expectedGatePoll = (url) =>
    /\/core-api\/quality\/candidates\/[^/]+\/gate$/.test(url);
  const unexpected404 = notFound.filter((url) => !expectedGatePoll(url));
  const expectedGatePolls = notFound.length - unexpected404.length;
  const genericNotFoundConsole =
    "Failed to load resource: the server responded with a status of 404 (Not Found)";
  const unexpectedConsole = consoleErrors.filter(
    (message) => message !== genericNotFoundConsole,
  );
  record.console.expected_gate_polls = expectedGatePolls;
  record.console.unexpected_404 = unexpected404;
  record.console.unexpected_console_errors = unexpectedConsole;
  record.assertions.no_unexpected_browser_errors =
    unexpected404.length === 0 &&
    unexpectedConsole.length === 0 &&
    pageErrors.length === 0 &&
    failedRequests.length === 0;
  assert.equal(pageErrors.length, 0, `page errors: ${pageErrors.join(" | ")}`);
  assert.equal(
    failedRequests.length,
    0,
    `failed requests: ${failedRequests.join(" | ")}`,
  );
  assert.deepEqual(
    unexpected404,
    [],
    `unexpected 404 responses: ${unexpected404.join(" | ")}`,
  );
  assert.deepEqual(
    unexpectedConsole,
    [],
    `unexpected console errors: ${unexpectedConsole.join(" | ")}`,
  );

  // Extra UAT probe: the Campaign budget surface is a separate operator view.
  // A crash here is recorded as a finding and does not erase the required
  // journey evidence captured above.
  stage = "campaign-budget-probe";
  const budgetErrorCursor = pageErrors.length;
  await page.goto(
    `${base}/#/campaigns/${encodeURIComponent(approved.campaignId)}?tab=budget`,
    { waitUntil: "domcontentloaded" },
  );
  await page.waitForTimeout(2500);
  const budgetBody = await page.locator("body").innerText().catch(() => "");
  const budgetPageErrors = pageErrors.slice(budgetErrorCursor);
  const budgetCrashed =
    budgetPageErrors.length > 0 || /undefined/i.test(budgetBody);
  await capturePair("campaign-budget");
  if (budgetCrashed) {
    record.findings.push({
      kind: "ui-contract-mismatch",
      component: "CampaignDetail Budget & usage tab",
      observed:
        budgetPageErrors[0] ??
        "authorization metadata renders as undefined or empty",
      expected:
        "V2 risk class, execution mode, provider route, delegation and bounded policy",
      cause:
        "CampaignDetail consumes the legacy snake_case CampaignAuthorization shape; Core V2 returns camelCase policy/operations fields",
      impact:
        "the operator budget/authorization view crashes or omits the authoritative V2 limits",
      disposition:
        "REPORTED_NOT_FIXED: Wave 3 execution-policy/Office budget lane owns the V2 projection contract",
      evidence: "campaign-budget-1440.png",
    });
    record.assertions.campaign_budget_projection = "FAILED_UI_CONTRACT";
  } else {
    record.assertions.campaign_budget_projection = "PASS";
  }
  record.console.budget_probe_page_errors = budgetPageErrors;

  record.result = record.findings.length === 0 ? "PASS" : "PASS_WITH_FINDINGS";
  record.acceptance = "NOT_CLAIMED_FIXTURE_ONLY_UAT";
} catch (cause) {
  record.result = "FAILED";
  record.error = { stage, message: errorText(cause) };
  record.assertions[`${stage.replaceAll(/[^a-z0-9]+/gi, "_")}_failed`] =
    "FAILED";
  if (page) {
    await page
      .screenshot({
        path: join(evidenceDir, `failure-${stage.replaceAll(/[^a-z0-9]+/gi, "_")}.png`),
        fullPage: true,
      })
      .catch(() => undefined);
  }
  process.exitCode = 1;
} finally {
  record.finished_at = new Date().toISOString();
  record.console = {
    ...record.console,
    errors: consoleErrors,
    page_errors: pageErrors,
    failed_requests: failedRequests,
    http_404: notFound,
  };
  if (browser) await browser.close().catch(() => undefined);
  writeFileSync(
    join(evidenceDir, "compose-browser-uat.json"),
    `${JSON.stringify(record, null, 2)}\n`,
  );
  console.log(JSON.stringify(record));
}
