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
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { chromium } from "playwright-core";

const root = resolve(import.meta.dirname, "../..");
const targetDir = process.env.CARGO_TARGET_DIR || join(root, "target");
const coreBin =
  process.env.FACTORY_CORE_BIN || join(targetDir, "debug", "factory-core");
const workerBin =
  process.env.FACTORY_WORKER_BIN || join(targetDir, "debug", "factory-worker");
const supervisorBin =
  process.env.FACTORY_SUPERVISOR_BIN ||
  join(targetDir, "debug", "factory-supervisor");
const verifierBin =
  process.env.FACTORY_VERIFIER_BIN ||
  join(targetDir, "debug", "factory-verifier");
const reviewerBin =
  process.env.FACTORY_REVIEWER_BIN ||
  join(targetDir, "debug", "factory-reviewer");
const fixtureProviderBin =
  process.env.FACTORY_FIXTURE_PROVIDER_BIN ||
  join(targetDir, "debug", "factory-fixture-provider");
const full = process.env.FACTORY_INTAKE_FULL === "1";
const live = process.env.FACTORY_LIVE_PROVIDER === "1";
const token = () => randomBytes(32).toString("hex");
const operator = token();
const enrollmentCode = randomBytes(16).toString("hex");
const bootstrap = token();
const verifier = token();
const reviewer = token();
const supervisorToken = token();
const attemptGrantKey = token();
const supervisorCapabilityKey = token();
const masterKey = randomBytes(32).toString("hex");
const workerId = "intake-full-worker";
const verifierPrincipal = "verifier-intake-full";
const reviewerPrincipal = "reviewer-intake-full";
const tempBase = process.env.FACTORY_E2E_TMP_BASE || "/tmp/opencode";
mkdirSync(tempBase, { recursive: true });
const temp = join(tempBase, `f-${randomBytes(4).toString("hex")}`);
const repository = join(temp, "repo");
const database = join(temp, "factory.sqlite");
const enrollmentCodeFile = join(temp, "enrollment-code");
const masterKeyFile = join(temp, "master-key");
const supervisorWork = join(temp, "sw");
const supervisorState = join(temp, "ss");
const supervisorVolumes = join(temp, "sv");
// Keep provider sockets beneath the UNIX sun_path limit (108 bytes).
const providerSessionRoot = join(temp, "p");
const processes = [];

function blockedLiveConfiguration(missing) {
  console.log(
    JSON.stringify({
      ok: false,
      status: "BLOCKED",
      mode: "LIVE_PROVIDER",
      reason:
        "FACTORY_LIVE_PROVIDER=1 requires an authorized provider route and execution environment; nothing was called.",
      missing,
    }),
  );
  process.exit(2);
}

function liveProviderConfiguration() {
  if (!full) {
    blockedLiveConfiguration([
      "FACTORY_INTAKE_FULL=1 (live provider execution requires the full journey)",
    ]);
  }
  const missing = [];
  const read = (name) => {
    const value = process.env[name]?.trim();
    if (!value) missing.push(name);
    return value ?? "";
  };
  const config = {
    provider: read("FACTORY_LIVE_PROVIDER_ID"),
    model: read("FACTORY_LIVE_MODEL"),
    modelUrl: read("FACTORY_LIVE_MODEL_URL"),
    modelToken: read("FACTORY_LIVE_MODEL_TOKEN"),
    providerBaseUrl:
      process.env.FACTORY_LIVE_PROVIDER_BASE_URL?.trim() ||
      process.env.FACTORY_LIVE_MODEL_URL?.trim() ||
      "",
    providerCredential:
      process.env.FACTORY_LIVE_PROVIDER_CREDENTIAL?.trim() ||
      process.env.FACTORY_LIVE_MODEL_TOKEN?.trim() ||
      "",
    providerSandbox: read("FACTORY_PROVIDER_SANDBOX"),
  };
  if (!config.providerBaseUrl) missing.push("FACTORY_LIVE_PROVIDER_BASE_URL");
  if (!config.providerCredential) {
    missing.push("FACTORY_LIVE_PROVIDER_CREDENTIAL");
  }
  if (config.providerSandbox === "bwrap") {
    read("FACTORY_PROVIDER_BINARY");
  }
  if (missing.length > 0) blockedLiveConfiguration([...new Set(missing)]);
  return config;
}

const liveConfig = live ? liveProviderConfiguration() : null;
const provider = liveConfig?.provider ?? "opencode-go";
const model = liveConfig?.model ?? "deepseek-v4.1-flash";

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
const supervisorPort = full ? await freePort() : null;
const supervisorUrl = supervisorPort
  ? `http://127.0.0.1:${supervisorPort}`
  : null;
const vitePort = await freePort();
const modelPort = live ? null : await freePort();
const modelUrl = liveConfig?.modelUrl ?? `http://127.0.0.1:${modelPort}/v1`;

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
      description: full
        ? "deterministic fixture provider content exists"
        : "deterministic functional fixture",
      command: full
        ? ["grep", "-q", "provider candidate", "src/provider.txt"]
        : ["true"],
      expectExit: 0,
      timeoutSecs: 30,
    },
    {
      id: "negative",
      kind: "negative",
      description: "deterministic negative fixture",
      command: full ? ["grep", "-q", "absent", "src/provider.txt"] : ["false"],
      expectExit: 1,
      timeoutSecs: 30,
    },
  ];
}

// Local OpenAI-compatible model endpoint. It returns deterministic assessment
// and semantic-review data with the exact identities from the prompt; it never
// calls a provider. When FACTORY_LIVE_PROVIDER=1 the journey uses the supplied
// real gateway instead and this server is not started.
let modelServer = null;
if (!live) {
  modelServer = createServer(async (request, response) => {
    if (request.method !== "POST") {
      response.writeHead(404).end();
      return;
    }
    let raw = "";
    for await (const chunk of request) raw += chunk;
    try {
      const body = JSON.parse(raw);
      const user =
        body.messages?.find((message) => message.role === "user")?.content ??
        "";
      const system =
        body.messages?.find((message) => message.role === "system")?.content ??
        "";
      if (system.includes("independent semantic code reviewer")) {
        const candidateCommitSha = user.match(
          /candidateCommitSha: ([0-9a-f]+)/,
        )?.[1];
        const candidateTreeSha = user.match(
          /candidateTreeSha: ([0-9a-f]+)/,
        )?.[1];
        const actionSpecSha256 = user.match(
          /actionSpecSha256: ([0-9a-f]+)/,
        )?.[1];
        assert.ok(
          candidateCommitSha && candidateTreeSha && actionSpecSha256,
          "semantic review prompt identities",
        );
        response.writeHead(200, { "content-type": "application/json" });
        response.end(
          JSON.stringify({
            choices: [
              {
                message: {
                  role: "assistant",
                  content: JSON.stringify({
                    schemaRevision: 1,
                    candidateCommitSha,
                    candidateTreeSha,
                    actionSpecSha256,
                    verdict: "approve",
                    summary:
                      "The deterministic fixture candidate satisfies the frozen objective.",
                    confidence: 90,
                    findings: [],
                  }),
                },
              },
            ],
            usage: {
              prompt_tokens: 300,
              completion_tokens: 100,
              total_tokens: 400,
            },
          }),
        );
        return;
      }
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
          provider,
          model,
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
              message: {
                role: "assistant",
                content: JSON.stringify(assessment),
              },
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
}

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

async function runToCompletion(command, args, options, label) {
  const child = launch(command, args, options);
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk) => {
    stdout += chunk;
  });
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });
  const code = await new Promise((resolvePromise) =>
    child.on("exit", resolvePromise),
  );
  assert.equal(
    code,
    0,
    `${label} failed with ${code}: ${stderr.slice(-4000) || stdout.slice(-4000)}`,
  );
  return { stdout, stderr };
}

async function stopProcess(child, label) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const exited = new Promise((resolvePromise) =>
    child.once("exit", resolvePromise),
  );
  child.kill("SIGTERM");
  const timeout = new Promise((resolvePromise) =>
    setTimeout(() => resolvePromise("timeout"), 10_000),
  );
  if ((await Promise.race([exited, timeout])) === "timeout") {
    child.kill("SIGKILL");
    await exited;
    console.error(`${label} required SIGKILL after SIGTERM timeout`);
  }
}

function gitOutput(args) {
  const child = spawn("git", ["-C", repository, ...args], {
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk) => {
    stdout += chunk;
  });
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });
  return new Promise((resolvePromise, reject) => {
    child.on("exit", (code) =>
      code === 0
        ? resolvePromise(stdout.trim())
        : reject(new Error(`git ${args.join(" ")} failed: ${stderr.trim()}`)),
    );
  });
}

async function waitForValue(load, accept, label, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  let last;
  while (Date.now() < deadline) {
    try {
      last = await load();
      if (accept(last)) return last;
    } catch (error) {
      last = error;
    }
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 150));
  }
  throw new Error(`${label} did not reach the expected state: ${last}`);
}

function coreEnvironment() {
  return {
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
    // Managed project registration is permitted only inside the operator
    // approved root. The fixture repository lives under this temp root.
    FACTORY_PROJECTS_ROOT: process.env.FACTORY_PROJECTS_ROOT?.trim() || temp,
    FACTORY_MODEL_URL: modelUrl,
    FACTORY_MODEL_TOKEN:
      liveConfig?.modelToken ?? "fake-model-token-0123456789abcdef",
    FACTORY_MODEL_PROVIDER: provider,
    FACTORY_MODEL_NAME: model,
    FACTORY_MODEL_TIMEOUT_SECS: "30",
    ...(live ? {} : { FACTORY_ALLOW_TEST_UPSTREAM: "1" }),
    FACTORY_ENABLE_AUTONOMOUS_START: "0",
    FACTORY_LEAD_FIXTURE_ASSESSOR: "0",
  };
}

function startCoreProcess() {
  const child = launch(coreBin, [], { cwd: temp, env: coreEnvironment() });
  child.stderr.on("data", (chunk) => process.stderr.write(chunk));
  return child;
}

function workerEnvironment() {
  const env = {
    ...process.env,
    FACTORY_CORE_URL: coreUrl,
    FACTORY_WORKER_BOOTSTRAP_TOKEN: bootstrap,
    FACTORY_WORKER_ID: workerId,
    FACTORY_WORKER_REGISTRATION_KEY: "intake-full-registration",
    FACTORY_SUPERVISOR_URL: supervisorUrl,
    FACTORY_SUPERVISOR_TOKEN: supervisorToken,
    FACTORY_WORKER_FIXTURE_IMAGE: "test/image:1",
    FACTORY_WORKER_ONCE: "1",
  };
  if (!live) {
    Object.assign(env, {
      FACTORY_PROVIDER_SANDBOX: "bwrap",
      FACTORY_PROVIDER_BINARY: fixtureProviderBin,
      FACTORY_PROVIDER_SESSION_ROOT: providerSessionRoot,
      FACTORY_PROVIDER_KEEP_SESSION: "1",
    });
  }
  return env;
}

let browser;
try {
  let core = startCoreProcess();
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
  await coreApi(`/projects/${project.id}/config`, {
    method: "PUT",
    body: JSON.stringify({
      expectedRevision: 1,
      allowedPaths: ["src"],
      capabilities: ["repository-edit", "generic-toolchain"],
      qualityProfile: "generic",
      active: true,
    }),
  });
  await coreApi(`/settings/providers/${encodeURIComponent(provider)}`, {
    method: "PUT",
    body: JSON.stringify({
      model,
      baseUrl: liveConfig?.providerBaseUrl ?? modelUrl,
      credential:
        liveConfig?.providerCredential ?? "fake-provider-credential-0123456789",
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
  if (full) {
    await coreApi("/settings/runtime", {
      method: "PUT",
      body: JSON.stringify({
        semanticReviewEnabled: true,
        semanticReviewMaxOutputTokens: 8192,
      }),
    });
  }
  const role = {
    provider,
    model,
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
  const objective =
    "Add a deterministic CSV export endpoint and cover it with tests.";
  await page.getByLabel("What should the Factory do?").fill(objective);
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
  assert.equal(campaign.authorization.provider, provider);
  assert.equal(campaign.authorization.model, model);
  assert.deepEqual(campaign.authorization.authorized_paths, ["src"]);
  assert.equal(campaign.authorization.risk_class, "R0");
  assert.equal(campaign.authorization.delegation_id, projection.delegationId);
  assert.equal(campaign.authorization.max_attempts, 1);
  assert.equal(projection.actionId, campaign.campaign.action_id);
  assert.equal(campaign.tasks.length, 1, "the frozen plan has one unit");
  const frozenTask = campaign.tasks[0];
  assert.equal(frozenTask.unit_id, "unit-1");
  assert.equal(frozenTask.request_id, projection.leadRequestId);
  assert.equal(frozenTask.base_commit.length, 40);
  const frozenRun = campaign.runs.find((run) => run.id === frozenTask.run_id);
  assert.ok(frozenRun, "the frozen WorkGraph has a Run");
  const frozenAttempt = campaign.attempts.find(
    (attempt) => attempt.id === frozenTask.attempt_id,
  );
  assert.ok(frozenAttempt, "the frozen WorkGraph has a Lead Attempt");

  let fullResult = null;
  if (full) {
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

    await runToCompletion(
      workerBin,
      [],
      { cwd: temp, env: workerEnvironment() },
      "fixture Worker",
    );

    const candidate = await waitForValue(
      () => coreApi("/candidates"),
      (items) => items.length === 1,
      "Worker candidate",
    ).then((items) => items[0]);
    assert.equal(
      candidate.attempt_id,
      frozenAttempt.id,
      "candidate belongs to the Lead Attempt",
    );
    assert.equal(candidate.implementer_id, workerId);
    const inspection = await coreApi(
      `/quality/candidates/${encodeURIComponent(candidate.id)}/inspection`,
    );
    assert.equal(
      candidate.action_spec_sha256,
      inspection.action_spec_sha256,
      "candidate ActionSpec identity",
    );
    assert.equal(inspection.action_spec.provider, provider);
    assert.equal(inspection.action_spec.model, model);
    assert.deepEqual(inspection.action_spec.allowed_paths, ["src"]);
    assert.equal(inspection.action_spec.objective, objective);
    assert.equal(
      await gitOutput(["rev-parse", `${candidate.commit_sha}^`]),
      frozenTask.base_commit,
      "candidate commit is based on the frozen base",
    );
    assert.equal(
      await gitOutput(["show", "-s", "--format=%T", candidate.commit_sha]),
      candidate.tree_sha,
      "candidate tree identity",
    );
    assert.equal(
      await gitOutput(["show", `${candidate.commit_sha}:src/provider.txt`]),
      "provider candidate",
      "the recorded candidate contains the fixture change",
    );

    const events = await coreApi("/events");
    const claimEvent = events.find(
      (event) =>
        event.kind === "worker.claimed" &&
        event.subject_id === frozenAttempt.id,
    );
    assert.ok(
      claimEvent,
      "Core must record the Worker claim for the Lead Attempt",
    );
    assert.match(claimEvent.detail, /generation=1/);
    const claimedAttempt = (
      await coreApi(`/campaigns/${encodeURIComponent(campaign.campaign.id)}`)
    ).attempts.find((attempt) => attempt.id === frozenAttempt.id);
    assert.equal(claimedAttempt.status, "CANDIDATE_READY");

    const diff = await coreApi(
      `/campaigns/${encodeURIComponent(campaign.campaign.id)}/candidates/${encodeURIComponent(candidate.id)}/diff`,
    );
    assert.equal(diff.commit_sha, candidate.commit_sha);
    assert.equal(diff.tree_sha, candidate.tree_sha);
    if (live) {
      assert.ok(
        diff.changed_paths.length > 0 &&
          diff.changed_paths.every((path) => path.startsWith("src/")),
        `live changed paths must stay inside src: ${diff.changed_paths}`,
      );
    } else {
      assert.deepEqual(diff.changed_paths, ["src/provider.txt"]);
    }

    await runToCompletion(
      verifierBin,
      [],
      {
        cwd: temp,
        env: {
          ...process.env,
          FACTORY_CORE_URL: coreUrl,
          FACTORY_VERIFIER_TOKEN: verifier,
          FACTORY_CANDIDATE_ID: candidate.id,
          FACTORY_VERIFIER_PRINCIPAL: verifierPrincipal,
          FACTORY_VERIFIER_WORKSPACE: join(temp, "vw"),
          FACTORY_VERIFIER_IMAGE: "test/image:1",
          FACTORY_VERIFIER_TEST_BACKEND: "local",
          FACTORY_VERIFIER_TEST_VOLUME_ROOT: join(temp, "vv"),
        },
      },
      "independent verifier",
    );
    await runToCompletion(
      reviewerBin,
      [],
      {
        cwd: temp,
        env: {
          ...process.env,
          FACTORY_CORE_URL: coreUrl,
          FACTORY_REVIEWER_TOKEN: reviewer,
          FACTORY_CANDIDATE_ID: candidate.id,
          FACTORY_REVIEWER_PRINCIPAL: reviewerPrincipal,
          FACTORY_REVIEW_MECHANISM: "MODEL",
        },
      },
      "independent semantic reviewer",
    );

    const gate = await waitForValue(
      () =>
        coreApi(`/quality/candidates/${encodeURIComponent(candidate.id)}/gate`),
      (value) => value.outcome === "ACCEPTED",
      "Quality Gate ACCEPTED",
    );
    assert.ok(Array.isArray(gate.reasons), "gate reasons are an array");
    const receipts = await coreApi(
      `/quality/candidates/${encodeURIComponent(candidate.id)}/receipts`,
    );
    const verifierReceipt = receipts.find(
      (receipt) => receipt.role === "verifier",
    );
    const reviewerReceipt = receipts.find(
      (receipt) => receipt.role === "reviewer",
    );
    assert.ok(verifierReceipt, "verifier receipt");
    assert.equal(verifierReceipt.principal_id, verifierPrincipal);
    assert.equal(verifierReceipt.result, "PASS");
    assert.equal(verifierReceipt.evidence, "EXECUTED");
    assert.equal(verifierReceipt.commit_sha, candidate.commit_sha);
    assert.equal(verifierReceipt.tree_sha, candidate.tree_sha);
    assert.equal(
      verifierReceipt.action_spec_sha256,
      candidate.action_spec_sha256,
    );
    assert.ok(reviewerReceipt, "reviewer receipt");
    assert.equal(reviewerReceipt.principal_id, reviewerPrincipal);
    assert.equal(reviewerReceipt.result, "PASS");
    assert.equal(reviewerReceipt.evidence, "EXECUTED");
    assert.equal(reviewerReceipt.mechanism, "MODEL");
    assert.equal(reviewerReceipt.model_provider, provider);
    assert.equal(reviewerReceipt.model_name, model);
    assert.equal(reviewerReceipt.usage_known, true);
    assert.ok(reviewerReceipt.total_tokens > 0);
    assert.equal(reviewerReceipt.commit_sha, candidate.commit_sha);
    assert.equal(reviewerReceipt.tree_sha, candidate.tree_sha);

    const acceptedCampaign = await coreApi(
      `/campaigns/${encodeURIComponent(campaign.campaign.id)}`,
    );
    assert.equal(acceptedCampaign.campaign.status, "AUTHORIZED");
    assert.equal(
      acceptedCampaign.runs.find((run) => run.id === frozenRun.id).status,
      "ACCEPTED",
    );
    const acceptedTask = acceptedCampaign.tasks.find(
      (task) => task.task_id === frozenTask.task_id,
    );
    assert.equal(acceptedTask.status, "SUCCEEDED");
    assert.equal(acceptedTask.candidate_id, candidate.id);
    assert.equal(acceptedTask.candidate_commit, candidate.commit_sha);
    assert.equal(acceptedTask.candidate_tree, candidate.tree_sha);

    const completed = await coreApi(
      `/intake/requests/${encodeURIComponent(requestId)}`,
    );
    assert.equal(completed.state, "COMPLETED");
    assert.equal(completed.gateOutcome, "ACCEPTED");
    assert.equal(completed.campaignStatus, "AUTHORIZED");
    assert.equal(completed.plan.plan_hash, planHash.plan.plan_hash);
    assert.equal(completed.profile.revisionId, profile.revisionId);
    assert.equal(completed.profile.contentHash, profile.contentHash);

    // Browser projection: the Campaign workspace must show the exact
    // WorkGraph, candidate diff, independent receipts and Gate outcome.
    await page
      .getByRole("link", { name: /Open Campaign/ })
      .first()
      .click();
    await page
      .getByRole("heading", {
        name: `Campaign ${campaign.campaign.id.slice(0, 8)}`,
      })
      .waitFor();
    await page.getByRole("tab", { name: /WorkGraph/ }).click();
    await page.getByRole("heading", { name: /WorkGraph \(1 unit/ }).waitFor();
    const workGraphNode = page.getByTestId("workgraph-node-unit-1");
    await workGraphNode.waitFor();
    await workGraphNode
      .getByText("SUCCEEDED", { exact: false })
      .first()
      .waitFor();
    await workGraphNode
      .getByText(/gate ACCEPTED/)
      .first()
      .waitFor();
    await page
      .getByTestId("workgraph-table")
      .getByText("SUCCEEDED", { exact: false })
      .first()
      .waitFor();

    await page.getByRole("tab", { name: /Candidates/ }).click();
    const candidatePanel = page.getByTestId("candidate-panel");
    await candidatePanel
      .getByText("ACCEPTED", { exact: true })
      .first()
      .waitFor();
    await candidatePanel
      .getByText(candidate.commit_sha.slice(0, 12), { exact: true })
      .first()
      .waitFor();
    await candidatePanel
      .getByText(candidate.tree_sha.slice(0, 12), { exact: true })
      .first()
      .waitFor();
    const candidateDiff = candidatePanel.getByTestId("candidate-diff");
    await candidateDiff.getByText(/Changed files/).waitFor();
    await candidateDiff
      .getByText("src/provider.txt", { exact: true })
      .first()
      .waitFor();

    await page.getByRole("tab", { name: /Evidence/ }).click();
    const receiptTable = page.getByTestId("candidate-receipts");
    await receiptTable
      .getByRole("row", { name: /verifier\s+PASS\s+EXECUTED/i })
      .waitFor();
    await receiptTable
      .getByRole("row", { name: /reviewer\s+PASS\s+EXECUTED/i })
      .waitFor();
    await page
      .getByTestId("receipt-model-provenance")
      .getByText(`${provider}/${model}`, { exact: false })
      .waitFor();

    await page.getByRole("tab", { name: /Quality Gate/ }).click();
    const gatePanel = page.getByTestId("quality-gate");
    await gatePanel.getByText("ACCEPTED", { exact: true }).waitFor();

    await page.getByRole("tab", { name: /Execution/ }).click();
    await page.getByRole("heading", { name: "Runs and attempts" }).waitFor();
    await page
      .getByText(frozenRun.id.slice(0, 8), { exact: true })
      .first()
      .waitFor();
    await page
      .getByText(frozenAttempt.id.slice(0, 8), { exact: true })
      .first()
      .waitFor();

    // Request projection: completed, Gate badge, exact plan and Campaign
    // facts, with no invented acceptance.
    await page.goto(`${viteUrl}/#/requests/${encodeURIComponent(requestId)}`);
    await page.getByRole("heading", { name: objective }).waitFor();
    await page.getByText("Completed", { exact: true }).first().waitFor();
    await page.getByText("Gate ACCEPTED", { exact: true }).waitFor();
    await page.getByText("AUTHORIZED", { exact: true }).first().waitFor();
    await page.getByText("1 / 1", { exact: true }).waitFor();
    await page
      .getByText(planHash.plan.plan_hash, { exact: true })
      .first()
      .waitFor();

    // Restart Core against the same durable database and reload the browser.
    // Request, Campaign, candidate, receipts and Gate must persist.
    await page.goto("about:blank");
    await stopProcess(core, "Core");
    core = startCoreProcess();
    await waitFor(`${coreUrl}/health`);
    assert.equal(core.exitCode, null);

    const persistedRequest = await coreApi(
      `/intake/requests/${encodeURIComponent(requestId)}`,
    );
    assert.equal(persistedRequest.state, "COMPLETED");
    assert.equal(persistedRequest.gateOutcome, "ACCEPTED");
    assert.equal(persistedRequest.campaignId, campaign.campaign.id);
    assert.equal(persistedRequest.plan.plan_hash, planHash.plan.plan_hash);
    assert.equal(persistedRequest.profile.revisionId, profile.revisionId);
    const persistedCampaign = await coreApi(
      `/campaigns/${encodeURIComponent(campaign.campaign.id)}`,
    );
    assert.equal(persistedCampaign.campaign.status, "AUTHORIZED");
    assert.deepEqual(
      persistedCampaign.tasks.map((task) => task.candidate_id),
      [candidate.id],
    );
    assert.deepEqual(
      persistedCampaign.candidates.map((item) => item.id),
      [candidate.id],
    );
    const persistedGate = await coreApi(
      `/quality/candidates/${encodeURIComponent(candidate.id)}/gate`,
    );
    assert.equal(persistedGate.outcome, "ACCEPTED");
    const persistedReceipts = await coreApi(
      `/quality/candidates/${encodeURIComponent(candidate.id)}/receipts`,
    );
    assert.deepEqual(
      persistedReceipts.map((receipt) => [receipt.role, receipt.result]),
      [
        ["reviewer", "PASS"],
        ["verifier", "PASS"],
      ],
    );
    const persistedDiff = await coreApi(
      `/campaigns/${encodeURIComponent(campaign.campaign.id)}/candidates/${encodeURIComponent(candidate.id)}/diff`,
    );
    assert.equal(persistedDiff.commit_sha, candidate.commit_sha);

    await page.goto(`${viteUrl}/#/requests/${encodeURIComponent(requestId)}`);
    await page.getByRole("heading", { name: objective }).waitFor();
    await page.getByText("Completed", { exact: true }).first().waitFor();
    await page.getByText("Gate ACCEPTED", { exact: true }).waitFor();
    await page.getByText("AUTHORIZED", { exact: true }).first().waitFor();
    await page.getByText("1 / 1", { exact: true }).waitFor();
    await page.goto(
      `${viteUrl}/#/campaigns/${encodeURIComponent(campaign.campaign.id)}`,
    );
    await page
      .getByRole("heading", {
        name: `Campaign ${campaign.campaign.id.slice(0, 8)}`,
      })
      .waitFor();
    await page.getByRole("tab", { name: /Candidates/ }).click();
    const persistedPanel = page.getByTestId("candidate-panel");
    await persistedPanel
      .getByText("ACCEPTED", { exact: true })
      .first()
      .waitFor();
    await persistedPanel.getByTestId("candidate-diff").waitFor();
    await page.getByRole("tab", { name: /Evidence/ }).click();
    await page
      .getByTestId("candidate-receipts")
      .getByRole("row", { name: /reviewer\s+PASS\s+EXECUTED/i })
      .waitFor();

    fullResult = {
      journey: "intake-full",
      request: requestId,
      campaign: campaign.campaign.id,
      profileRevision: projection.profile.revisionId,
      profileContentHash: projection.profile.contentHash,
      planHash: planHash.plan.plan_hash,
      actionId: projection.actionId,
      delegationId: projection.delegationId,
      leadRequestId: projection.leadRequestId,
      taskId: frozenTask.task_id,
      runId: frozenRun.id,
      attemptId: frozenAttempt.id,
      workerId,
      candidateId: candidate.id,
      candidateCommit: candidate.commit_sha,
      candidateTree: candidate.tree_sha,
      changedPaths: diff.changed_paths,
      verifierReceiptId: verifierReceipt.receipt_id,
      reviewerReceiptId: reviewerReceipt.receipt_id,
      reviewerMechanism: reviewerReceipt.mechanism,
      reviewerReservationId: reviewerReceipt.model_reservation_id,
      gate: gate.outcome,
      gateReasons: gate.reasons,
      campaignStatus: acceptedCampaign.campaign.status,
      persistence: "CORE_RESTART_AND_BROWSER_RELOAD",
    };
  }

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
      mode: live
        ? "LIVE_PROVIDER"
        : full
          ? "DETERMINISTIC_FIXTURE"
          : "DETERMINISTIC_FAKE_MODEL",
      journey: full ? "intake-full" : "intake",
      ...(fullResult ?? {}),
    }),
  );
} finally {
  if (browser) await browser.close().catch(() => {});
  for (const process of processes.reverse()) process.kill("SIGKILL");
  if (modelServer) modelServer.close();
  rmSync(temp, { recursive: true, force: true });
}
