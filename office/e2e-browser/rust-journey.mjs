// Real Chromium acceptance for the unified Rust-backed Office.
// Starts the real Rust Core, Worker and Vite dev server in disposable state.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { homedir, tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { chromium } from "playwright-core";

const root = resolve(import.meta.dirname, "../..");
const coreBin =
  process.env.FACTORY_CORE_BIN || join(root, "target/debug/factory-core");
const workerBin =
  process.env.FACTORY_WORKER_BIN || join(root, "target/debug/factory-worker");
const supervisorBin =
  process.env.FACTORY_SUPERVISOR_BIN ||
  join(root, "target/debug/factory-supervisor");
const token = () => randomBytes(32).toString("hex");
const operator = token();
const bootstrap = token();
const verifier = token();
const reviewer = token();
const supervisorToken = token();
const temp = join(
  tmpdir(),
  `factory-rust-office-${randomBytes(6).toString("hex")}`,
);
const repository = join(temp, "repo");
const database = join(temp, "factory.sqlite");
const workspace = join(temp, "workspaces");
const supervisorWork = join(temp, "supervisor-work");
const supervisorState = join(temp, "supervisor-state");
const supervisorVolumes = join(temp, "supervisor-volumes");
const processes = [];

mkdirSync(repository, { recursive: true });
mkdirSync(workspace, { recursive: true });

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
  const response = await fetch(`${coreUrl}${path}`, {
    ...options,
    headers: {
      authorization: `Bearer ${bearer}`,
      "content-type": "application/json",
      ...(options.headers || {}),
    },
  });
  const text = await response.text();
  if (!response.ok)
    throw new Error(
      `${options.method || "GET"} ${path}: ${response.status} ${text}`,
    );
  return text ? JSON.parse(text) : null;
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
      FACTORY_WORKER_BOOTSTRAP_TOKEN: bootstrap,
      FACTORY_VERIFIER_TOKEN: verifier,
      FACTORY_REVIEWER_TOKEN: reviewer,
    },
  });
  await waitFor(`${coreUrl}/health`);
  assert.equal(core.exitCode, null);

  const supervisor = launch(supervisorBin, [], {
    cwd: temp,
    env: {
      ...process.env,
      FACTORY_SUPERVISOR_TOKEN: supervisorToken,
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
  browser = await chromium.launch({ headless: true, executablePath });
  const page = await browser.newPage();
  const consoleErrors = [];
  const pageErrors = [];
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });
  page.on("pageerror", (error) => pageErrors.push(String(error)));

  await page.goto(viteUrl);
  await page.getByPlaceholder("FACTORY_OPERATOR_TOKEN").fill(operator);
  await page.getByRole("button", { name: "Unlock" }).click();
  await page.getByRole("heading", { name: "Dashboard" }).waitFor();

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

  await page.getByRole("link", { name: "Campaigns", exact: true }).click();
  await page.getByRole("heading", { name: "Campaigns", exact: true }).waitFor();
  await page.getByTestId("campaign-action").selectOption({ index: 0 });
  await page.getByRole("button", { name: "Create Campaign" }).click();
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
  for (const [bearer, role] of [
    [verifier, "verifier"],
    [reviewer, "reviewer"],
  ]) {
    await api(
      "/quality/receipts",
      {
        method: "POST",
        body: JSON.stringify({
          candidate_id: candidate.id,
          receipt_id: `${role}-office-e2e`,
          principal_id: `${role}-office-e2e`,
          actionSpecSha256: candidate.action_spec_sha256,
          commitSha: candidate.commit_sha,
          treeSha: candidate.tree_sha,
          result: "PASS",
          evidence: "EXECUTED",
        }),
      },
      bearer,
    );
  }

  await page.getByRole("link", { name: "Campaigns", exact: true }).click();
  await page.getByRole("heading", { name: "Campaigns", exact: true }).waitFor();
  await page.reload();
  await page.getByText("ACCEPTED").waitFor({ timeout: 15000 });
  assert.deepEqual(
    pageErrors,
    [],
    `browser page errors: ${pageErrors.join("; ")}`,
  );
  if (consoleErrors.length > 0) {
    console.error("browser console diagnostics:", consoleErrors.join("; "));
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
