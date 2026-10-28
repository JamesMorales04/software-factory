// Real Chromium acceptance for the unified Rust-backed Office.
// Starts the real Rust Core, Worker and Vite dev server in disposable state.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { homedir, tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { chromium } from "playwright-core";

const root = resolve(import.meta.dirname, "../..");
const targetDir = process.env.CARGO_TARGET_DIR || join(root, "target");
const debugDir = join(targetDir, "debug");
const coreBin = process.env.FACTORY_CORE_BIN || join(debugDir, "factory-core");
const workerBin =
  process.env.FACTORY_WORKER_BIN || join(debugDir, "factory-worker");
const supervisorBin =
  process.env.FACTORY_SUPERVISOR_BIN || join(debugDir, "factory-supervisor");
const reviewerBin =
  process.env.FACTORY_REVIEWER_BIN || join(debugDir, "factory-reviewer");
const token = () => randomBytes(32).toString("hex");
const operator = token();
const enrollmentCode = randomBytes(16).toString("hex");
const bootstrap = token();
const verifier = token();
const reviewer = token();
const supervisorToken = token();
const attemptGrantKey = token();
const supervisorCapabilityKey = token();
const temp = join(
  tmpdir(),
  `factory-rust-office-${randomBytes(6).toString("hex")}`,
);
const repository = join(temp, "repo");
const database = join(temp, "factory.sqlite");
const enrollmentCodeFile = join(temp, "enrollment-code");
const workspace = join(temp, "workspaces");
const supervisorWork = join(temp, "supervisor-work");
const supervisorState = join(temp, "supervisor-state");
const supervisorVolumes = join(temp, "supervisor-volumes");
const processes = [];

mkdirSync(repository, { recursive: true });
mkdirSync(workspace, { recursive: true });
writeFileSync(
  enrollmentCodeFile,
  JSON.stringify({
    codeSha256: createHash("sha256").update(enrollmentCode).digest("hex"),
    expiresAtMs: Date.now() + 5 * 60 * 1000,
  }),
  { mode: 0o600 },
);

function git(args) {
  const result = spawn("git", ["-C", repository, ...args], { stdio: "ignore" });
  return new Promise((resolvePromise, reject) => {
    result.on("exit", (code) =>
      code === 0
        ? resolvePromise()
        : reject(new Error(`git ${args.join(" ")} failed`)),
    );
  });
}
await git(["init", "-q"]);
await git(["config", "user.name", "Office E2E"]);
await git(["config", "user.email", "office-e2e@example.invalid"]);
writeFileSync(join(repository, "seed.txt"), "seed\n");
await git(["add", "seed.txt"]);
await git(["commit", "-qm", "seed"]);

function freePort() {
  return new Promise((resolvePromise, reject) => {
    const server = createServer();
    server.listen(0, "127.0.0.1", () => {
      const port = server.address().port;
      server.close(() => resolvePromise(port));
    });
    server.on("error", reject);
  });
}

async function waitFor(url, timeoutMs = 15000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) return;
    } catch {}
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`timed out waiting for ${url}`);
}

async function api(path, options = {}, bearer = operator) {
  for (let attempt = 0; ; attempt += 1) {
    const response = await fetch(`${coreUrl}${path}`, {
      ...options,
      headers: {
        authorization: `Bearer ${bearer}`,
        "content-type": "application/json",
        ...(options.headers || {}),
      },
    });
    const text = await response.text();
    if (response.ok) return text ? JSON.parse(text) : null;
    // Core deliberately reports SQLite writer contention as a retryable 503.
    // Fixture mutations carry idempotency keys, so a bounded retry is safe.
    if (
      response.status === 503 &&
      text.includes("database is busy") &&
      attempt < 5
    ) {
      await new Promise((resolve) => setTimeout(resolve, 100 * 2 ** attempt));
      continue;
    }
    throw new Error(
      `${options.method || "GET"} ${path}: ${response.status} ${text}`,
    );
  }
}

const corePort = await freePort();
const coreUrl = `http://127.0.0.1:${corePort}`;
const supervisorPort = await freePort();
const supervisorUrl = `http://127.0.0.1:${supervisorPort}`;
const vitePort = await freePort();

function launch(command, args, options = {}) {
  const child = spawn(command, args, {
    stdio: ["ignore", "pipe", "pipe"],
    ...options,
  });
  processes.push(child);
  child.stdout.on("data", () => {});
  child.stderr.on("data", () => {});
  return child;
}

let browser;
try {
  const core = launch(coreBin, [], {
    cwd: temp,
    env: {
      ...process.env,
      FACTORY_DATABASE: database,
      FACTORY_BIND: `127.0.0.1:${corePort}`,
      FACTORY_OPERATOR_TOKEN: operator,
      FACTORY_ENROLLMENT_CODE_FILE: enrollmentCodeFile,
      FACTORY_ATTEMPT_GRANT_KEY: attemptGrantKey,
      FACTORY_WORKER_BOOTSTRAP_TOKEN: bootstrap,
      FACTORY_VERIFIER_TOKEN: verifier,
      FACTORY_REVIEWER_TOKEN: reviewer,
      FACTORY_LEAD_FIXTURE_ASSESSOR: "1",
      FACTORY_ENABLE_AUTONOMOUS_START: "1",
    },
  });
  await waitFor(`${coreUrl}/health`);
  assert.equal(core.exitCode, null);

  const supervisor = launch(supervisorBin, [], {
    cwd: temp,
    env: {
      ...process.env,
      FACTORY_SUPERVISOR_TOKEN: supervisorToken,
      FACTORY_CORE_URL: coreUrl,
      FACTORY_ATTEMPT_GRANT_KEY: attemptGrantKey,
      FACTORY_SUPERVISOR_CAPABILITY_KEY: supervisorCapabilityKey,
      FACTORY_SUPERVISOR_BIND: `127.0.0.1:${supervisorPort}`,
      FACTORY_SUPERVISOR_WORKSPACE_ROOT: supervisorWork,
      FACTORY_SUPERVISOR_STATE_ROOT: supervisorState,
      FACTORY_SUPERVISOR_PROJECT_ROOTS: repository,
      FACTORY_SUPERVISOR_IMAGES: "test/image:1",
      FACTORY_SUPERVISOR_TEST_BACKEND: "local",
      FACTORY_SUPERVISOR_TEST_VOLUME_ROOT: supervisorVolumes,
    },
  });
  await waitFor(`${supervisorUrl}/health`);
  assert.equal(supervisor.exitCode, null);

  const vite = launch(
    join(root, "office/node_modules/.bin/vite"),
    ["--host", "127.0.0.1", "--port", String(vitePort), "--strictPort"],
    {
      cwd: join(root, "office"),
      env: { ...process.env, FACTORY_CORE_URL: coreUrl },
    },
  );
  vite.stderr.on("data", (chunk) => process.stderr.write(chunk));
  const viteUrl = `http://127.0.0.1:${vitePort}`;
  await waitFor(viteUrl);
  assert.equal(vite.exitCode, null);

  const executablePath =
    process.env.CHROMIUM_EXE ||
    (() => {
      const base = join(homedir(), ".cache/ms-playwright");
      const candidates = readdirSync(base).filter((name) =>
        name.startsWith("chromium-"),
      );
      for (const name of candidates.reverse()) {
        const path = join(base, name, "chrome-linux64/chrome");
        try {
          readdirSync(join(base, name));
        } catch {
          continue;
        }
        return path;
      }
      throw new Error("no Chromium executable found");
    })();
  const launchArgs =
    process.env.FACTORY_BROWSER_NO_SANDBOX === "1"
      ? ["--no-sandbox", "--disable-dev-shm-usage"]
      : [];
  browser = await chromium.launch({
    headless: true,
    executablePath,
    ...(launchArgs.length > 0 ? { args: launchArgs } : {}),
  });
  const page = await browser.newPage();
  const consoleErrors = [];
  const pageErrors = [];
  const notFound = [];
  page.on("response", (response) => {
    if (response.status() === 404) notFound.push(response.url());
  });
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });
  page.on("pageerror", (error) => pageErrors.push(String(error)));

  await page.goto(viteUrl);
  await page.getByRole("heading", { name: "Set up this Factory" }).waitFor();
  await page.getByLabel("One-time owner confirmation").fill(enrollmentCode);
  await page.getByLabel("This device name").fill("Rust Office E2E");
  await page.getByRole("button", { name: "Enroll this device" }).click();
  await page.getByRole("heading", { name: "Overview", level: 1 }).waitFor();
  await page.getByRole("link", { name: "Decision Inbox", exact: true }).click();
  await page
    .getByRole("heading", { name: "Decision Inbox", exact: true, level: 1 })
    .waitFor();
  await page.getByRole("link", { name: "Overview", exact: true }).click();
  await page
    .getByRole("heading", { name: "Overview", exact: true, level: 1 })
    .waitFor();

  await page.getByRole("link", { name: "Projects", exact: true }).click();
  await page.getByRole("heading", { name: "Projects", exact: true }).waitFor();
  await page.getByTestId("project-repository").fill(repository);
  await page.getByRole("button", { name: "Register Project" }).click();
  await page.getByText(repository).first().waitFor();

  await page.getByRole("link", { name: "Actions", exact: true }).click();
  await page.getByRole("heading", { name: "Actions", exact: true }).waitFor();
  await page
    .getByTestId("action-objective")
    .fill("Rust Office fixture journey");
  await page.getByRole("button", { name: "Create Action" }).click();
  await page.getByText("Created Action", { exact: false }).waitFor();
  await page.locator("tbody tr").first().getByRole("link").click();
  await page.getByRole("heading", { name: /^Action /, level: 1 }).waitFor();
  await page.getByText("Frozen checks", { exact: false }).waitFor();
  await page.getByRole("link", { name: "Back to Actions" }).click();
  await page.getByRole("heading", { name: "Actions", exact: true }).waitFor();

  await page.getByRole("link", { name: "Campaigns", exact: true }).click();
  await page.getByRole("heading", { name: "Campaigns", exact: true }).waitFor();
  await page.getByTestId("campaign-action").selectOption({ index: 0 });
  await page.getByRole("button", { name: "Create Campaign" }).click();
  await page.getByRole("link", { name: "Decision Inbox", exact: true }).click();
  await page.getByText("Campaign awaiting authorization").waitFor();
  await page.getByRole("link", { name: "Campaigns", exact: true }).click();
  await page.getByRole("heading", { name: "Campaigns", exact: true }).waitFor();
  await page
    .getByRole("button", { name: "Authorize R0 fixture" })
    .first()
    .click();
  await page.getByRole("button", { name: "Queue Run" }).first().click();

  await page.getByRole("link", { name: "Runs", exact: true }).click();
  await page.getByRole("heading", { name: "Runs", exact: true }).waitFor();
  await page.getByRole("button", { name: "Queue Attempt" }).first().click();
  await page.getByText("All Attempts (1)").waitFor();

  const worker = launch(workerBin, [], {
    cwd: temp,
    env: {
      ...process.env,
      FACTORY_CORE_URL: coreUrl,
      FACTORY_WORKER_BOOTSTRAP_TOKEN: bootstrap,
      FACTORY_WORKER_ID: "office-e2e-worker",
      FACTORY_WORKER_REGISTRATION_KEY: "office-e2e-registration",
      FACTORY_SUPERVISOR_URL: supervisorUrl,
      FACTORY_SUPERVISOR_TOKEN: supervisorToken,
      FACTORY_WORKER_FIXTURE_IMAGE: "test/image:1",
      FACTORY_WORKER_ONCE: "1",
      FACTORY_FIXTURE_FILE: "src/fixture.txt",
      FACTORY_FIXTURE_CONTENT: "fixture candidate\n",
    },
  });
  const workerExit = await new Promise((resolvePromise) =>
    worker.on("exit", resolvePromise),
  );
  assert.equal(workerExit, 0, "fixture Worker must succeed");

  const candidates = await api("/candidates");
  assert.equal(candidates.length, 1);
  const candidate = candidates[0];
  await api(
    "/quality/receipts",
    {
      method: "POST",
      body: JSON.stringify({
        candidate_id: candidate.id,
        receipt_id: "verifier-office-e2e",
        principal_id: "verifier-office-e2e",
        actionSpecSha256: candidate.action_spec_sha256,
        commitSha: candidate.commit_sha,
        treeSha: candidate.tree_sha,
        result: "PASS",
        evidence: "EXECUTED",
      }),
    },
    verifier,
  );
  const officeCampaigns = await api("/campaigns");
  const fixtureCampaign = officeCampaigns[0];
  const officeRuns = await api("/runs");
  const fixtureRun = officeRuns.find(
    (run) => run.campaign_id === fixtureCampaign.id,
  );
  assert.ok(fixtureRun, "the fixture Campaign must have a Run");
  await page.getByRole("link", { name: "Runs", exact: true }).click();
  await page.getByRole("heading", { name: "Runs", exact: true }).waitFor();
  const fixtureRunRow = page.getByRole("row", {
    name: new RegExp(fixtureRun.id.slice(0, 8)),
  });
  await fixtureRunRow
    .locator(`a[href="#/campaigns/${fixtureCampaign.id}"]`)
    .waitFor();
  await page.getByRole("link", { name: "Campaigns", exact: true }).click();
  await page.getByRole("heading", { name: "Campaigns", exact: true }).waitFor();
  const fixtureCandidateRow = page.getByRole("row", {
    name: new RegExp(candidate.id.slice(0, 8)),
  });
  await fixtureCandidateRow
    .locator(`a[href="#/campaigns/${fixtureCampaign.id}"]`)
    .waitFor();
  await page
    .locator(`a[href="#/campaigns/${fixtureCampaign.id}"]`)
    .first()
    .click();
  await page
    .getByRole("heading", {
      name: `Campaign ${fixtureCampaign.id.slice(0, 8)}`,
    })
    .waitFor();
  const workflow = page.getByTestId("workflow-projection");
  await workflow.getByText("WorkGraph").waitFor();
  await workflow.getByText("Verifier").waitFor();
  await workflow.getByText("Reviewer").waitFor();
  await page.getByRole("tab", { name: /WorkGraph/ }).click();
  await page.getByRole("heading", { name: /WorkGraph \(/ }).waitFor();
  await page.getByRole("tab", { name: /Execution/ }).click();
  await page.getByRole("heading", { name: "Runs and attempts" }).waitFor();
  await page.getByRole("heading", { name: "Participant sessions" }).waitFor();
  await page.getByRole("tab", { name: /Budget & usage/ }).click();
  await page
    .getByRole("heading", { name: "Authorization, budget and observed usage" })
    .waitFor();
  await page.getByText("Unmeasured sessions", { exact: true }).waitFor();

  await page.getByRole("tab", { name: /Candidates/ }).click();
  const pendingCandidate = page.getByTestId("candidate-panel");
  await pendingCandidate.getByText("GATE PENDING").waitFor();
  await pendingCandidate.getByTestId("candidate-diff").waitFor();
  await pendingCandidate.getByText(/Changed paths/).waitFor();
  assert.equal(
    await pendingCandidate.getByText("ACCEPTED", { exact: true }).count(),
    0,
    "a reviewer-less candidate must not render ACCEPTED",
  );
  await page.getByRole("tab", { name: /Evidence/ }).click();
  const pendingReceipts = page.getByTestId("candidate-receipts");
  await pendingReceipts
    .getByRole("row", { name: /verifier\s+PASS\s+EXECUTED/i })
    .waitFor();
  await pendingReceipts
    .getByRole("row", { name: /reviewer\s+PENDING/i })
    .waitFor();
  await page.getByRole("tab", { name: /Quality Gate/ }).click();
  await page
    .getByText("no Core gate decision is recorded", { exact: false })
    .waitFor();

  const reviewerProcess = launch(reviewerBin, [], {
    cwd: temp,
    env: {
      ...process.env,
      FACTORY_CORE_URL: coreUrl,
      FACTORY_REVIEWER_TOKEN: reviewer,
      FACTORY_CANDIDATE_ID: candidate.id,
      FACTORY_REVIEWER_PRINCIPAL: "reviewer-office-e2e",
    },
  });
  const reviewerExit = await new Promise((resolvePromise) =>
    reviewerProcess.on("exit", resolvePromise),
  );
  assert.equal(reviewerExit, 0, "independent reviewer process must pass");

  await page.reload();
  await page
    .getByRole("heading", {
      name: `Campaign ${fixtureCampaign.id.slice(0, 8)}`,
    })
    .waitFor();
  assert.equal(
    await page
      .getByRole("tab", { name: /Quality Gate/ })
      .getAttribute("aria-selected"),
    "true",
    "the selected Campaign tab must survive reload",
  );
  await page.getByRole("tab", { name: /Candidates/ }).click();
  const acceptedCandidate = page.getByTestId("candidate-panel");
  await acceptedCandidate
    .getByText("ACCEPTED", { exact: true })
    .first()
    .waitFor({ timeout: 15000 });
  await page.getByRole("tab", { name: /Evidence/ }).click();
  await page
    .getByTestId("candidate-receipts")
    .getByRole("row", { name: /reviewer\s+PASS\s+EXECUTED/i })
    .waitFor({ timeout: 15000 });
  await page.getByRole("tab", { name: /Events/ }).click();
  await page
    .getByText("No unresolved provider session is recorded", { exact: false })
    .waitFor();
  for (const width of [834, 390]) {
    await page.setViewportSize({ width, height: 900 });
    const viewport = await page.evaluate(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    }));
    assert.equal(
      viewport.scrollWidth,
      viewport.clientWidth,
      `Campaign Events workspace must not overflow the document at ${width}px`,
    );
  }
  await page.setViewportSize({ width: 1440, height: 1000 });

  await page.getByRole("link", { name: "Overview", exact: true }).click();
  await page.getByRole("heading", { name: "Overview", exact: true }).waitFor();
  await page
    .locator(`a[href="#/campaigns/${fixtureCampaign.id}"]`)
    .first()
    .click();
  await page
    .getByRole("heading", {
      name: `Campaign ${fixtureCampaign.id.slice(0, 8)}`,
    })
    .waitFor();

  await page.getByRole("link", { name: "Workers", exact: true }).click();
  await page.getByRole("heading", { name: "Workers", exact: true }).waitFor();
  await page.getByText("office-e2e-worker").waitFor();

  await page.getByRole("link", { name: "Runtime", exact: true }).click();
  await page.getByRole("heading", { name: "Runtime", exact: true }).waitFor();
  await page.getByRole("tab", { name: /Sessions/ }).click();
  await page
    .getByRole("heading", { name: "Provider sessions (0 shown)" })
    .waitFor();
  await page.getByText("No matching sessions").waitFor();
  await page.getByRole("tab", { name: /Model calls/ }).click();
  await page.getByRole("heading", { name: /Core-owned model calls/ }).waitFor();
  await page.getByRole("tab", { name: /Diagnostics/ }).click();
  await page.getByRole("heading", { name: "Service boundaries" }).waitFor();
  await page.getByText("Not exposed by the current Core API").first().waitFor();
  for (const width of [834, 390]) {
    await page.setViewportSize({ width, height: 900 });
    const viewport = await page.evaluate(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    }));
    assert.equal(
      viewport.scrollWidth,
      viewport.clientWidth,
      `Runtime diagnostics must not overflow the document at ${width}px`,
    );
  }
  await page.setViewportSize({ width: 1440, height: 1000 });

  await page
    .locator(".sidebar")
    .getByRole("link", { name: "Settings", exact: true })
    .click();
  await page.getByRole("heading", { name: "Settings", exact: true }).waitFor();
  await page.getByText("OpenCode Go").waitFor();
  await page.getByText("Codex App Server").waitFor();
  await page.getByText("Coding inference").first().waitFor();
  await page.getByText("BLOCKED").first().waitFor();
  for (const width of [834, 390]) {
    await page.setViewportSize({ width, height: 900 });
    const viewport = await page.evaluate(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    }));
    assert.equal(
      viewport.scrollWidth,
      viewport.clientWidth,
      `Settings providers must not overflow the document at ${width}px`,
    );
  }
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.getByRole("tab", { name: /Runtime policy/ }).click();
  await page
    .getByRole("heading", { name: "Review and runtime policy" })
    .waitFor();
  await page.getByText(/Current: DETERMINISTIC/).waitFor();
  for (const width of [834, 390]) {
    await page.setViewportSize({ width, height: 900 });
    const viewport = await page.evaluate(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    }));
    assert.equal(
      viewport.scrollWidth,
      viewport.clientWidth,
      `Settings must not overflow the document at ${width}px`,
    );
  }
  await page.setViewportSize({ width: 1440, height: 1000 });

  const _projects = await api("/projects");
  const _leadProject = _projects[0];
  const _leadConfig = await api(`/projects/${_leadProject.id}/config`, {
    method: "PUT",
    body: JSON.stringify({
      expectedRevision: 1,
      allowedPaths: ["src"],
      capabilities: ["repository-edit", "generic-toolchain"],
      qualityProfile: "generic",
      active: true,
    }),
  });
  const _leadAction = await api("/actions", {
    method: "POST",
    body: JSON.stringify({
      project_id: _leadProject.id,
      objective: "Lead fixture objective",
      provider: "opencode",
      model: "deepseek-v4.1-flash",
      idempotency_key: "office-lead-action",
      allowed_paths: ["src"],
      checks: [
        {
          id: "build",
          kind: "build",
          description: "build",
          command: ["true"],
          expectExit: 0,
          timeoutSecs: 30,
        },
        {
          id: "test",
          kind: "test",
          description: "test",
          command: ["true"],
          expectExit: 0,
          timeoutSecs: 30,
        },
        {
          id: "functional",
          kind: "functional",
          description: "fixture",
          command: ["grep", "-q", "fixture candidate", "src/fixture.txt"],
          expectExit: 0,
          timeoutSecs: 30,
        },
        {
          id: "negative",
          kind: "negative",
          description: "absent",
          command: ["grep", "-q", "absent", "src/fixture.txt"],
          expectExit: 1,
          timeoutSecs: 30,
        },
      ],
    }),
  });
  const _providerAction = await api("/actions", {
    method: "POST",
    body: JSON.stringify({
      project_id: _leadProject.id,
      objective: "Provider session fixture objective",
      provider: "opencode",
      model: "deepseek-v4.1-flash",
      idempotency_key: "office-provider-action",
      allowed_paths: ["src"],
      checks: _leadAction.spec.checks,
    }),
  });
  const _cancelAction = await api("/actions", {
    method: "POST",
    body: JSON.stringify({
      project_id: _leadProject.id,
      objective: "Cancellation fixture objective",
      provider: "opencode",
      model: "deepseek-v4.1-flash",
      idempotency_key: "office-cancel-action",
      allowed_paths: ["src"],
      checks: _leadAction.spec.checks,
    }),
  });
  const _leadDelegation = await api("/delegations", {
    method: "POST",
    body: JSON.stringify({
      idempotency_key: "office-lead-delegation",
      delegation: {
        id: "office-lead-grant",
        principalId: "office-owner",
        auditId: "office-audit",
        projectId: _leadProject.id,
        revision: 1,
        objectiveClasses: ["feature"],
        allowedPaths: ["src"],
        capabilities: ["repository-edit", "generic-toolchain"],
        riskClasses: ["R0", "R1"],
        autonomousStart: true,
        autonomousRiskClasses: ["R0"],
        route: {
          provider: "opencode",
          model: "deepseek-v4.1-flash",
          mode: "build",
        },
        maxAttempts: 2,
        maxTimeoutSecs: 300,
        maxRepairCycles: 0,
        maxWallClockSecs: 1800,
        maxParallel: 1,
        maxSessions: 4,
        maxRequests: 2,
        maxCumulativeSessions: 8,
        maxCumulativeAttempts: 16,
        expiresAt: new Date(Date.now() + 86_400_000).toISOString(),
      },
    }),
  });
  await page.getByRole("link", { name: "Lead", exact: true }).click();
  await page.getByRole("heading", { name: "Lead", exact: true }).waitFor();
  await page.getByTestId("lead-project").selectOption(_leadProject.id);
  await page.getByTestId("lead-action").selectOption(_leadAction.id);
  await page.getByTestId("lead-delegation").selectOption(_leadDelegation.id);
  await page.getByTestId("lead-objective").fill("Lead fixture objective");
  await page.getByRole("button", { name: "Submit Lead request" }).click();
  let _leadRequest;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    const records = await api("/lead/requests");
    _leadRequest = records.find(
      (record) => record.action_id === _leadAction.id,
    );
    if (_leadRequest) break;
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 100));
  }
  assert.ok(_leadRequest, "the Lead form must persist a request");
  await page
    .getByText(_leadRequest.id.slice(0, 8), { exact: false })
    .first()
    .waitFor();
  await page
    .getByRole("button", { name: "Assess", exact: true })
    .first()
    .click();
  await page.getByText("PLANNED", { exact: true }).first().waitFor();
  await page.getByRole("button", { name: "Freeze plan" }).first().click();
  await page.getByText("READY", { exact: true }).first().waitFor();
  const leadAssessment = page.getByTestId("lead-assessment");
  await leadAssessment.getByText("change", { exact: true }).waitFor();
  await leadAssessment.getByText("small", { exact: true }).waitFor();
  const leadProposal = page.getByTestId("lead-proposal");
  await leadProposal
    .getByText("implementation", { exact: true })
    .first()
    .waitFor();
  await page.getByTestId("lead-plan").waitFor();
  await page.reload();
  await page.getByRole("heading", { name: "Lead", exact: true }).waitFor();
  await page
    .getByText("READY", { exact: true })
    .first()
    .waitFor({ timeout: 15000 });

  await page.getByRole("link", { name: "Agent Office", exact: true }).click();
  await page
    .getByRole("heading", { name: "Agent Office", exact: true, level: 1 })
    .waitFor();
  await page
    .getByRole("button", {
      name: new RegExp(`Inspect Lead lead:${_leadRequest.id}`),
    })
    .waitFor();
  await page
    .getByRole("button", {
      name: new RegExp(`Inspect Lead lead:${_leadRequest.id}`),
    })
    .click();
  const participantInspector = page.getByTestId("participant-inspector");
  await participantInspector
    .getByText("A Lead request is a Core-recorded planning")
    .waitFor();
  await participantInspector.getByText("LEAD_REQUEST").waitFor();
  await page.getByRole("tab", { name: /Table/ }).click();
  await page.getByTestId("agent-table").waitFor();
  await page.getByRole("tab", { name: /Timeline/ }).click();
  await page.getByTestId("agent-timeline").waitFor();
  await page.getByTestId("participant-inspector").waitFor();
  await page.getByRole("tab", { name: /Visual Office/ }).click();
  await page.getByTestId("agent-floor").waitFor();
  await page
    .getByTestId("agent-filters")
    .getByLabel("Role", { exact: true })
    .selectOption("LEAD");
  await page.waitForURL(/role=LEAD/);
  await page
    .getByTestId("agent-filters")
    .getByLabel("Role", { exact: true })
    .selectOption("");
  for (const width of [834, 390]) {
    await page.setViewportSize({ width, height: 900 });
    const viewport = await page.evaluate(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    }));
    assert.equal(
      viewport.scrollWidth,
      viewport.clientWidth,
      `Agent Office must not overflow the document at ${width}px`,
    );
  }
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.getByRole("link", { name: "Lead", exact: true }).click();
  await page.getByRole("heading", { name: "Lead", exact: true }).waitFor();

  await api(`/actions/${_leadAction.id}/classify`, {
    method: "POST",
    body: JSON.stringify({
      risk_class: "R0",
      actor: "office-e2e",
      rationale: "deterministic fixture classification",
      idempotency_key: "office-lead-classify",
    }),
  });

  await api(`/actions/${_providerAction.id}/classify`, {
    method: "POST",
    body: JSON.stringify({
      risk_class: "R0",
      actor: "office-e2e",
      rationale: "deterministic provider-session fixture classification",
      idempotency_key: "office-provider-classify",
    }),
  });
  const providerCampaign = await api("/campaigns", {
    method: "POST",
    body: JSON.stringify({
      action_id: _providerAction.id,
      idempotency_key: "office-provider-session-campaign",
    }),
  });
  await api(`/campaigns/${providerCampaign.id}/authorize`, {
    method: "POST",
    body: JSON.stringify({
      authorization_id: "office-provider-session-auth",
      risk_class: "R0",
      execution_mode: "TRUSTED_LOCAL_PROVIDER",
      actor: "office-e2e",
      rationale: "deterministic worker-protocol fixture for Agent Office",
      authorized_paths: ["src"],
      max_attempts: 1,
      provider: "opencode",
      model: "deepseek-v4.1-flash",
      provider_mode: "build",
      delegation_id: _leadDelegation.id,
      max_sessions: 2,
      max_turns: 4,
      max_input_tokens: 100000,
      max_output_tokens: 100000,
      max_cost_micros: 100000,
      deadline_secs: 300,
      allowed_tools: ["write_file"],
    }),
  });
  const providerRun = await api(`/campaigns/${providerCampaign.id}/runs`, {
    method: "POST",
  });
  await api(`/runs/${providerRun.id}/attempts`, { method: "POST" });
  const registered = await api(
    "/worker/register",
    {
      method: "POST",
      body: JSON.stringify({
        worker_id: "agent-office-fixture",
        registration_key: "agent-office-fixture",
      }),
    },
    bootstrap,
  );
  const workerToken = registered.session_token;
  const claim = await api(
    "/worker/claim",
    {
      method: "POST",
      body: JSON.stringify({ worker_id: "agent-office-fixture" }),
    },
    workerToken,
  );
  assert.ok(
    claim.assignment,
    "the provider fixture Worker must claim an Attempt",
  );
  const providerIntent = await api(
    "/worker/provider_intent",
    {
      method: "POST",
      body: JSON.stringify({
        attempt_id: claim.assignment.lease.attempt_id,
        worker_id: "agent-office-fixture",
        generation: claim.assignment.lease.generation,
        provider: "opencode",
        model: "deepseek-v4.1-flash",
        provider_mode: "build",
      }),
    },
    workerToken,
  );
  await api(
    "/worker/provider_active",
    {
      method: "POST",
      body: JSON.stringify({
        session_record_id: providerIntent.session_record_id,
        worker_id: "agent-office-fixture",
        generation: claim.assignment.lease.generation,
        provider_session_id: "ses-office-fixture",
      }),
    },
    workerToken,
  );
  for (const [callId, tool, decision, detail] of [
    ["office-call-1", "write_file", "REQUEST", "tool requested"],
    ["office-call-1", "write_file", "ALLOW", "path authorized"],
    ["office-call-1", "write_file", "RESULT", "write applied"],
    [
      "office-call-2",
      "final",
      "RESULT",
      "free-form explanation that must never render: api_key=canary-secret",
    ],
  ]) {
    await api(
      "/worker/provider_tool_event",
      {
        method: "POST",
        body: JSON.stringify({
          session_record_id: providerIntent.session_record_id,
          worker_id: "agent-office-fixture",
          generation: claim.assignment.lease.generation,
          call_id: callId,
          tool,
          decision,
          detail,
        }),
      },
      workerToken,
    );
  }

  await page.getByRole("link", { name: "Agent Office", exact: true }).click();
  await page
    .getByRole("heading", { name: "Agent Office", exact: true, level: 1 })
    .waitFor();
  await page
    .getByTestId("agent-filters")
    .getByLabel("Campaign", { exact: true })
    .selectOption(providerCampaign.id);
  const activeImplementer = page.getByRole("button", {
    name: /Inspect Implementer implementer:/,
  });
  await activeImplementer.waitFor();
  await activeImplementer.click();
  const activeInspector = page.getByTestId("participant-inspector");
  await activeInspector.getByText("ACTIVE", { exact: true }).first().waitFor();
  await activeInspector.getByText("CURRENT", { exact: true }).waitFor();
  await activeInspector.getByText("ses-office-fixture").waitFor();
  const toolTable = page.getByTestId("tool-event-table");
  await toolTable.getByText("write_file").first().waitFor();
  await toolTable.getByText("tool requested").waitFor();
  await toolTable.getByText("write applied").waitFor();
  await toolTable.getByText("provider content withheld").waitFor();
  assert.equal(
    await page.getByText("canary-secret", { exact: false }).count(),
    0,
    "free-form provider detail must never render",
  );
  const activeParticipantUrl = page.url();
  await page.reload();
  await page
    .getByRole("heading", { name: "Agent Office", exact: true, level: 1 })
    .waitFor();
  await page.getByTestId("participant-inspector").waitFor();
  await page
    .getByTestId("tool-event-table")
    .getByText("write applied")
    .waitFor();
  assert.equal(
    page.url(),
    activeParticipantUrl,
    "the selected participant and filters must survive reload",
  );
  await page
    .getByTestId("agent-filters")
    .getByLabel("Campaign", { exact: true })
    .selectOption("");
  await api(`/campaigns/${providerCampaign.id}/cancel`, {
    method: "POST",
    body: JSON.stringify({ reason: "deterministic browser fixture cleanup" }),
  });

  await page.getByRole("link", { name: "Lead", exact: true }).click();
  await page.getByRole("heading", { name: "Lead", exact: true }).waitFor();
  await page.getByRole("heading", { name: "Lead", exact: true }).waitFor();
  const leadRow = page.getByRole("row", {
    name: new RegExp(_leadRequest.id.slice(0, 8)),
  });
  await leadRow.getByRole("button", { name: "Start Campaign" }).waitFor();
  await leadRow.getByRole("button", { name: "Start Campaign" }).click();
  await page.getByText(/Started Campaign/).waitFor();
  let leadCampaign;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    const records = await api("/lead/requests");
    const record = records.find((item) => item.id === _leadRequest.id);
    if (record?.campaign_id) {
      leadCampaign = { id: record.campaign_id };
      break;
    }
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 100));
  }
  assert.ok(leadCampaign, "the Office Start control must create a Campaign");

  await page.reload();
  await page.getByRole("heading", { name: "Lead", exact: true }).waitFor();
  const runningRow = page.getByRole("row", {
    name: new RegExp(_leadRequest.id.slice(0, 8)),
  });
  await runningRow.getByRole("button", { name: "View" }).click();
  await page.getByText("RUNNING", { exact: true }).first().waitFor();
  await page
    .getByRole("link", {
      name: `Open Campaign ${leadCampaign.id.slice(0, 8)}`,
    })
    .waitFor();

  await page.getByRole("link", { name: "Campaigns", exact: true }).click();
  await page.getByRole("heading", { name: "Campaigns", exact: true }).waitFor();
  await page
    .locator(`a[href="#/campaigns/${leadCampaign.id}"]`)
    .first()
    .click();
  await page
    .getByRole("heading", { name: `Campaign ${leadCampaign.id.slice(0, 8)}` })
    .waitFor();
  const leadWorkflow = page.getByTestId("workflow-projection");
  await leadWorkflow
    .getByRole("row", { name: /Lead request.*RUNNING/i })
    .waitFor();
  await leadWorkflow.getByText("FIXTURE mechanism", { exact: false }).waitFor();
  await leadWorkflow.getByText("1 unit", { exact: false }).waitFor();
  await page
    .getByText("FIXTURE (deterministic, no provider)", { exact: false })
    .first()
    .waitFor();
  await page.getByRole("tab", { name: /Budget & usage/ }).click();
  await page
    .getByText("opencode/deepseek-v4.1-flash", { exact: false })
    .first()
    .waitFor();

  await page.getByRole("tab", { name: /WorkGraph/ }).click();
  await page.getByRole("heading", { name: /WorkGraph \(1 units\)/ }).waitFor();
  const workGraphNode = page
    .locator('[data-testid^="workgraph-node-"]')
    .first();
  await workGraphNode.waitFor();
  await page.getByTestId("workgraph-table").waitFor();
  const workGraphUnit = await workGraphNode.getAttribute("data-testid");
  assert.ok(workGraphUnit, "the WorkGraph must render its recorded unit");
  await workGraphNode.click();
  const workUnitInspector = page.getByTestId("workgraph-inspector");
  await workUnitInspector.waitFor();
  await workUnitInspector.getByText("Frozen sequence").waitFor();
  await page.waitForURL(/tab=workgraph&unit=/);
  const workGraphUrl = page.url();
  await page.reload();
  await page
    .getByRole("heading", { name: `Campaign ${leadCampaign.id.slice(0, 8)}` })
    .waitFor();
  await page.getByTestId("workgraph-inspector").waitFor();
  assert.equal(
    page.url(),
    workGraphUrl,
    "WorkUnit selection must survive reload",
  );
  for (const width of [834, 390]) {
    await page.setViewportSize({ width, height: 900 });
    const viewport = await page.evaluate(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    }));
    assert.equal(
      viewport.scrollWidth,
      viewport.clientWidth,
      `Campaign WorkGraph must not overflow the document at ${width}px`,
    );
  }
  await page.setViewportSize({ width: 1440, height: 1000 });

  await page.getByRole("link", { name: "Agent Office", exact: true }).click();
  await page
    .getByRole("heading", { name: "Agent Office", exact: true, level: 1 })
    .waitFor();
  await page
    .getByTestId("agent-filters")
    .getByLabel("Campaign", { exact: true })
    .selectOption(leadCampaign.id);
  await page
    .getByRole("button", {
      name: new RegExp(`Inspect Lead lead:${_leadRequest.id}`),
    })
    .waitFor();
  await page
    .getByTestId("agent-filters")
    .getByLabel("Campaign", { exact: true })
    .selectOption("");

  await page.goto(`${viteUrl}/#/campaigns/${leadCampaign.id}`);
  await page
    .getByRole("heading", { name: `Campaign ${leadCampaign.id.slice(0, 8)}` })
    .waitFor();
  await page.getByTestId("workflow-projection").waitFor();

  await api(`/actions/${_cancelAction.id}/classify`, {
    method: "POST",
    body: JSON.stringify({
      risk_class: "R0",
      actor: "office-e2e",
      rationale: "deterministic cancellation fixture classification",
      idempotency_key: "office-cancel-classify",
    }),
  });
  const cancelCampaign = await api("/campaigns", {
    method: "POST",
    body: JSON.stringify({
      action_id: _cancelAction.id,
      idempotency_key: "office-cancel-campaign",
    }),
  });
  await page.getByRole("link", { name: "Campaigns", exact: true }).click();
  await page.getByRole("heading", { name: "Campaigns", exact: true }).waitFor();
  const cancelRow = page.getByRole("row", {
    name: new RegExp(cancelCampaign.id.slice(0, 8)),
  });
  await cancelRow.getByRole("button", { name: "Cancel Campaign" }).click();
  await page.getByTestId("cancel-reason").fill("operator e2e cancellation");
  await page.getByRole("button", { name: "Confirm cancellation" }).click();
  await page
    .getByText(`Campaign ${cancelCampaign.id.slice(0, 8)} is CANCELLED`, {
      exact: false,
    })
    .waitFor();
  await page
    .locator(`a[href="#/campaigns/${cancelCampaign.id}"]`)
    .first()
    .click();
  await page
    .getByRole("heading", { name: `Campaign ${cancelCampaign.id.slice(0, 8)}` })
    .waitFor();
  await page.getByRole("tab", { name: /Events/ }).click();
  await page.getByText("campaign.cancelled").first().waitFor();
  await page
    .getByText("operator e2e cancellation", { exact: false })
    .first()
    .waitFor();
  await page.getByText("Campaign state:", { exact: false }).waitFor();

  await page.getByRole("link", { name: "Campaigns", exact: true }).click();
  await page.getByRole("heading", { name: "Campaigns", exact: true }).waitFor();
  await page.reload();
  await page
    .getByText("ACCEPTED", { exact: true })
    .first()
    .waitFor({ timeout: 15000 });
  assert.deepEqual(
    pageErrors,
    [],
    `browser page errors: ${pageErrors.join("; ")}`,
  );
  // A missing gate is a normal pending projection. The UI intentionally probes
  // that endpoint and renders PENDING; all other 404s are broken resources.
  const unexpectedNotFound = notFound.filter(
    (url) => !/\/core-api\/quality\/candidates\/[^/]+\/gate$/.test(url),
  );
  assert.deepEqual(
    unexpectedNotFound,
    [],
    `unexpected 404 responses: ${unexpectedNotFound.join("; ")}`,
  );
  const unexpectedConsole = consoleErrors.filter(
    (message) =>
      message !==
      "Failed to load resource: the server responded with a status of 404 (Not Found)",
  );
  assert.deepEqual(
    unexpectedConsole,
    [],
    `unexpected browser console errors: ${unexpectedConsole.join("; ")}`,
  );
  if (consoleErrors.length > 0) {
    console.error(
      "browser console diagnostics: expected pending-gate 404 probes",
    );
  }
  console.log(
    JSON.stringify({
      ok: true,
      candidate: candidate.commit_sha,
      gate: "ACCEPTED",
    }),
  );
} finally {
  if (browser) await browser.close().catch(() => {});
  for (const child of processes.reverse()) child.kill("SIGKILL");
  rmSync(temp, { recursive: true, force: true });
}
