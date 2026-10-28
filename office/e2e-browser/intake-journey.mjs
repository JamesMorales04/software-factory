// Real Chromium journey for the unified intake path.
//
// Starts the real Rust Core with a local OpenAI-compatible fake model gateway.
// The model response is deterministic test data, not live inference. Core,
// authorization, freezing, operator approval, SQLite and the browser are real.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { createServer as createTcpServer } from "node:net";
import { homedir, tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { chromium } from "playwright-core";

const root = resolve(import.meta.dirname, "../..");
const targetDir = process.env.CARGO_TARGET_DIR || join(root, "target");
const coreBin =
  process.env.FACTORY_CORE_BIN || join(targetDir, "debug", "factory-core");
const token = () => randomBytes(32).toString("hex");
const operator = token();
const enrollmentCode = randomBytes(16).toString("hex");
const bootstrap = token();
const verifier = token();
const reviewer = token();
const attemptGrantKey = token();
const masterKey = randomBytes(32).toString("hex");
const temp = join(tmpdir(), `factory-intake-${randomBytes(6).toString("hex")}`);
const repository = join(temp, "repo");
const database = join(temp, "factory.sqlite");
const enrollmentCodeFile = join(temp, "enrollment-code");
const masterKeyFile = join(temp, "master-key");
const processes = [];

mkdirSync(repository, { recursive: true });
writeFileSync(
  enrollmentCodeFile,
  JSON.stringify({
    codeSha256: createHash("sha256").update(enrollmentCode).digest("hex"),
    expiresAtMs: Date.now() + 10 * 60 * 1000,
  }),
  { mode: 0o600 },
);
writeFileSync(masterKeyFile, masterKey, { mode: 0o600 });

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
await git(["config", "user.name", "Intake E2E"]);
await git(["config", "user.email", "intake-e2e@example.invalid"]);
writeFileSync(join(repository, "seed.txt"), "seed\n");
await git(["add", "seed.txt"]);
await git(["commit", "-qm", "seed"]);

function freePort() {
  return new Promise((resolvePromise, reject) => {
    const server = createTcpServer();
    server.listen(0, "127.0.0.1", () => {
      const port = server.address().port;
      server.close(() => resolvePromise(port));
    });
    server.on("error", reject);
  });
}

async function waitFor(url, timeoutMs = 20000) {
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

const corePort = await freePort();
const coreUrl = `http://127.0.0.1:${corePort}`;
const vitePort = await freePort();
const modelPort = await freePort();
const modelUrl = `http://127.0.0.1:${modelPort}/v1`;

async function coreApi(path, options = {}, bearer = operator) {
  const response = await fetch(`${coreUrl}${path}`, {
    ...options,
    headers: {
      authorization: `Bearer ${bearer}`,
      "content-type": "application/json",
      ...(options.headers || {}),
    },
  });
  const text = await response.text();
  const body = text ? JSON.parse(text) : null;
  if (!response.ok) {
    throw new Error(
      `${options.method || "GET"} ${path}: ${response.status} ${text}`,
    );
  }
  return body;
}

function checks() {
  return [
    {
      id: "build",
      kind: "build",
      description: "deterministic build fixture",
      command: ["true"],
      expectExit: 0,
      timeoutSecs: 30,
    },
    {
      id: "test",
      kind: "test",
      description: "deterministic test fixture",
      command: ["true"],
      expectExit: 0,
      timeoutSecs: 30,
    },
    {
      id: "functional",
      kind: "functional",
      description: "deterministic functional fixture",
      command: ["true"],
      expectExit: 0,
      timeoutSecs: 30,
    },
    {
      id: "negative",
      kind: "negative",
      description: "deterministic negative fixture",
      command: ["false"],
      expectExit: 1,
      timeoutSecs: 30,
    },
  ];
}

// Local OpenAI-compatible model endpoint. It returns deterministic assessment
// data with the exact identities from the prompt; it never calls a provider.
const modelServer = createServer(async (request, response) => {
  if (request.method !== "POST") {
    response.writeHead(404).end();
    return;
  }
  let raw = "";
  for await (const chunk of request) raw += chunk;
  try {
    const body = JSON.parse(raw);
    const user =
      body.messages?.find((message) => message.role === "user")?.content ?? "";
    const projectId = user.match(/Project: ([^\s.]+)/)?.[1];
    const actionId = user.match(/Action: ([^\s.]+)/)?.[1];
    const requestId = user.match(/Request: ([^\s.]+)/)?.[1];
    const objective = user.match(/Objective: ([\s\S]*?)\. Project: /)?.[1];
    assert.ok(
      projectId && actionId && requestId && objective,
      "model prompt identities",
    );
    const config = await coreApi(
      `/projects/${encodeURIComponent(projectId)}/config`,
    );
    const maxSessions = Number(user.match(/maxSessions=(\d+)/)?.[1] ?? 2);
    const maxWallClockSecs = Number(
      user.match(/maxWallClockSecs=(\d+)/)?.[1] ?? 600,
    );
    const timeoutSecs = Number(
      user.match(/budget\.timeoutSecs=(\d+)/)?.[1] ?? 300,
    );
    const allChecks = checks();
    const acceptance = allChecks.filter((check) => check.kind !== "negative");
    const negative = allChecks.filter((check) => check.kind === "negative");
    const proposal = {
      projectId,
      actionId,
      baseRevision: config.baseCommit,
      projectConfigHash: config.configHash,
      objective,
      class: "feature",
      assumptions: ["Deterministic fake model gateway; not live inference."],
      risks: ["The model response is test data."],
      acceptanceCriteria: acceptance,
      negativeCases: negative,
      qualityProfile: config.qualityProfile,
      riskClass: "R0",
      budget: { maxAttempts: 1, timeoutSecs },
      route: {
        provider: "opencode-go",
        model: "deepseek-v4.1-flash",
        mode: "build",
      },
      maxRepairCycles: 0,
      maxWallClockSecs,
      maxParallel: 1,
      maxSessions,
      workUnits: [
        {
          id: "unit-1",
          kind: "implementation",
          description: objective,
          dependsOn: [],
          allowedPaths: config.allowedPaths,
          capabilities: config.capabilities,
          acceptanceCriteria: [],
          negativeCases: [],
        },
      ],
    };
    const assessment = {
      schemaRevision: 1,
      projectId,
      actionId,
      requestId,
      evidenceRefs: ["repository-files"],
      intent: "change",
      complexity: "small",
      effortEstimate: {
        status: "estimated",
        unit: "person-hours",
        minPersonHours: 1,
        maxPersonHours: 2,
        assumptions: ["Single deterministic unit."],
        unknownReason: "",
      },
      knowledgeGaps: [],
      changeImpact: ["src"],
      validationNeeds: ["Run the frozen checks."],
      assumptions: ["Deterministic fake model gateway."],
      uncertainty: ["Not live provider inference."],
      suggestedNextAction: "Freeze and approve the plan.",
      proposal,
    };
    response.writeHead(200, { "content-type": "application/json" });
    response.end(
      JSON.stringify({
        choices: [
          {
            message: { role: "assistant", content: JSON.stringify(assessment) },
          },
        ],
        usage: {
          prompt_tokens: 500,
          completion_tokens: 500,
          total_tokens: 1000,
        },
      }),
    );
  } catch (error) {
    console.error("FAKE MODEL ERROR", error);
    response.writeHead(400, { "content-type": "application/json" });
    response.end(JSON.stringify({ error: String(error) }));
  }
});
await new Promise((resolvePromise) =>
  modelServer.listen(modelPort, "127.0.0.1", resolvePromise),
);

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
      FACTORY_MASTER_KEY_FILE: masterKeyFile,
      FACTORY_MODEL_URL: modelUrl,
      FACTORY_MODEL_TOKEN: "fake-model-token-0123456789abcdef",
      FACTORY_MODEL_PROVIDER: "opencode-go",
      FACTORY_MODEL_NAME: "deepseek-v4.1-flash",
      FACTORY_MODEL_TIMEOUT_SECS: "30",
      FACTORY_ALLOW_TEST_UPSTREAM: "1",
      FACTORY_ENABLE_AUTONOMOUS_START: "0",
      FACTORY_LEAD_FIXTURE_ASSESSOR: "0",
    },
  });
  core.stderr.on("data", (chunk) => process.stderr.write(chunk));
  await waitFor(`${coreUrl}/health`);
  assert.equal(core.exitCode, null);

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
  browser = await chromium.launch({
    headless: true,
    executablePath,
    ...(process.env.FACTORY_BROWSER_NO_SANDBOX === "1"
      ? { args: ["--no-sandbox", "--disable-dev-shm-usage"] }
      : {}),
  });
  const page = await browser.newPage();
  const consoleErrors = [];
  const pageErrors = [];
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });
  page.on("pageerror", (error) => pageErrors.push(String(error)));

  await page.goto(viteUrl);
  await page.getByRole("heading", { name: "Set up this Factory" }).waitFor();
  await page.getByLabel("One-time owner confirmation").fill(enrollmentCode);
  await page.getByLabel("This device name").fill("Intake E2E");
  await page.getByRole("button", { name: "Enroll this device" }).click();
  await page.getByRole("heading", { name: "Home", level: 1 }).waitFor();

  const project = await coreApi("/projects", {
    method: "POST",
    body: JSON.stringify({
      repository,
      idempotency_key: "intake-e2e-project",
    }),
  });
  const config = await coreApi(`/projects/${project.id}/config`, {
    method: "PUT",
    body: JSON.stringify({
      expectedRevision: 1,
      allowedPaths: ["src"],
      capabilities: ["repository-edit", "generic-toolchain"],
      qualityProfile: "generic",
      active: true,
    }),
  });
  await coreApi("/settings/providers/opencode-go", {
    method: "PUT",
    body: JSON.stringify({
      model: "deepseek-v4.1-flash",
      baseUrl: modelUrl,
      credential: "fake-provider-credential-0123456789",
      enabled: true,
      isDefault: true,
      maxTurns: 4,
      maxInputTokens: 100000,
      maxOutputTokens: 12000,
      maxCostMicros: 500000,
      inputMicrosPerMillion: 1000,
      outputMicrosPerMillion: 2000,
    }),
  });
  const role = {
    provider: "opencode-go",
    model: "deepseek-v4.1-flash",
    reasoning: "medium",
    maxInputTokens: 64000,
    maxOutputTokens: 8192,
    maxCostMicros: 300000,
  };
  const profile = await coreApi("/execution-profiles", {
    method: "POST",
    body: JSON.stringify({
      name: "Intake E2E Balanced",
      description: "Deterministic intake journey profile",
      category: "BALANCED",
      roles: {
        lead: role,
        implementer: {
          ...role,
          maxInputTokens: 40000,
          maxOutputTokens: 12000,
          maxCostMicros: 500000,
        },
        semanticReviewer: role,
      },
      budgets: {
        maxAttempts: 3,
        maxSessions: 4,
        maxTurns: 4,
        maxInputTokens: 40000,
        maxOutputTokens: 12000,
        maxCostMicros: 500000,
        deadlineSecs: 600,
      },
      auto_start: { enabled: false, risk_classes: [] },
    }),
  });
  await coreApi(`/execution-profiles/${profile.id}/default`, {
    method: "POST",
    body: JSON.stringify({ scope: "global" }),
  });
  assert.equal(
    profile.executable,
    true,
    "profile must be executable for the fake default route",
  );

  await page.reload();
  await page.getByRole("heading", { name: "Home", level: 1 }).waitFor();
  await page.getByLabel("Project", { exact: true }).selectOption(project.id);
  await page
    .getByLabel("Execution profile", { exact: true })
    .selectOption(profile.id);
  await page
    .getByLabel("What should the Factory do?")
    .fill("Add a deterministic CSV export endpoint and cover it with tests.");
  await page.getByRole("button", { name: "Submit request" }).click();
  await page
    .getByRole("heading", { name: "Enable governed execution" })
    .waitFor();
  await page
    .getByLabel(/I authorize this bounded non-autonomous R0 execution policy/)
    .check();
  await page.getByRole("button", { name: "Enable governed execution" }).click();
  await page.waitForURL(/#\/requests\//);
  const requestId = page.url().match(/#\/requests\/([^/?]+)/)?.[1];
  assert.ok(requestId, "request id in URL");

  await page.getByRole("button", { name: "Run assessment" }).click();
  try {
    await page
      .getByRole("button", { name: "Approve frozen plan and start" })
      .waitFor({ timeout: 30000 });
  } catch (error) {
    console.error(
      "INTAKE DEBUG PROJECTION",
      JSON.stringify(
        await coreApi(`/intake/requests/${encodeURIComponent(requestId)}`),
      ),
    );
    console.error("INTAKE DEBUG PAGE", await page.locator("body").innerText());
    throw error;
  }
  const planHash = await coreApi(
    `/intake/requests/${encodeURIComponent(requestId)}`,
  );
  assert.equal(planHash.state, "AWAITING_APPROVAL");
  assert.ok(planHash.plan?.plan_hash, "frozen plan hash");

  await page
    .getByLabel("Approval rationale")
    .fill("Reviewed the exact frozen plan.");
  await page.getByLabel("I reviewed the frozen scope").check();
  await page
    .getByRole("button", { name: "Approve frozen plan and start" })
    .click();
  await page
    .getByRole("link", { name: /Open Campaign/ })
    .first()
    .waitFor({ timeout: 30000 });

  const projection = await coreApi(
    `/intake/requests/${encodeURIComponent(requestId)}`,
  );
  assert.equal(projection.state, "EXECUTING");
  assert.ok(projection.campaignId, "campaign id");
  const campaign = await coreApi(
    `/campaigns/${encodeURIComponent(projection.campaignId)}`,
  );
  assert.ok(["AUTHORIZED", "RUNNING"].includes(campaign.campaign.status));
  assert.equal(campaign.authorization.execution_mode, "TRUSTED_LOCAL_PROVIDER");
  assert.equal(campaign.campaign.action_id, projection.actionId);
  assert.equal(projection.profile.revisionId, profile.revisionId);
  assert.equal(projection.profile.contentHash, profile.contentHash);
  assert.deepEqual(
    pageErrors,
    [],
    `browser page errors: ${pageErrors.join("; ")}`,
  );
  // The first submit intentionally receives the typed 409 that asks the
  // operator to create the bounded execution policy. It is a product state,
  // not an unexplained browser error.
  const expectedPolicyConflict =
    "Failed to load resource: the server responded with a status of 409 (Conflict)";
  const unexpectedConsole = consoleErrors.filter(
    (message) =>
      message !==
        "Failed to load resource: the server responded with a status of 404 (Not Found)" &&
      message !== expectedPolicyConflict,
  );
  assert.deepEqual(
    unexpectedConsole,
    [],
    `console errors: ${unexpectedConsole.join("; ")}`,
  );
  console.log(
    JSON.stringify({
      ok: true,
      request: requestId,
      campaign: projection.campaignId,
      profileRevision: projection.profile.revisionId,
      planHash: planHash.plan.plan_hash,
      mode: "DETERMINISTIC_FAKE_MODEL",
    }),
  );
} finally {
  if (browser) await browser.close();
  for (const process of processes) process.kill("SIGTERM");
  modelServer.close();
  rmSync(temp, { recursive: true, force: true });
}
