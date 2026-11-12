// Real Chromium acceptance for the unified Rust-backed Office.
// Starts the real Rust Core, Worker and Vite dev server in disposable state.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { homedir, tmpdir } from "node:os";
import { join, resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import { chromium } from "playwright";

const root = resolve(import.meta.dirname, "../..");
const targetDir = process.env.CARGO_TARGET_DIR || join(root, "target");
const debugDir = join(targetDir, "debug");
const coreBin = process.env.FACTORY_CORE_BIN || join(debugDir, "factory-core");
const supervisorBin =
  process.env.FACTORY_SUPERVISOR_BIN || join(debugDir, "factory-supervisor");
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
const masterKeyFile = join(temp, "master-key");
const gatewayConfig = join(temp, "tool-gateway.json");
const workspace = join(temp, "workspaces");
const supervisorWork = join(temp, "supervisor-work");
const supervisorState = join(temp, "supervisor-state");
const supervisorVolumes = join(temp, "supervisor-volumes");
const processes = [];

mkdirSync(repository, { recursive: true });
mkdirSync(workspace, { recursive: true });
writeFileSync(masterKeyFile, randomBytes(32).toString("hex"), { mode: 0o600 });
writeFileSync(
  gatewayConfig,
  JSON.stringify({
    maxCallsPerMinute: 60,
    servers: [
      {
        transport: "http",
        id: "context7",
        endpoint: "https://mcp.context7.com/mcp",
      },
    ],
    tools: [
      {
        id: "context7.query-docs",
        server: "context7",
        tool: "query-docs",
        roles: ["IMPLEMENTER"],
        readOnly: true,
        timeoutSecs: 30,
        maxResponseBytes: 262144,
      },
    ],
  }),
  { mode: 0o600 },
);
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

const corePort = await freePort();
const coreUrl = `http://127.0.0.1:${corePort}`;
const supervisorPort = await freePort();
const supervisorUrl = `http://127.0.0.1:${supervisorPort}`;
const vitePort = await freePort();

// Child processes are piped so a fixture failure can report the real error.
// Only the tail is retained to keep long-running services bounded in memory.
const LOG_TAIL_CHARS = 8000;

function launch(command, args, options = {}) {
  const child = spawn(command, args, {
    stdio: ["ignore", "pipe", "pipe"],
    ...options,
  });
  processes.push(child);
  child.log = "";
  const capture = (chunk) => {
    child.log = (child.log + chunk.toString()).slice(-LOG_TAIL_CHARS);
  };
  child.stdout.on("data", capture);
  child.stderr.on("data", capture);
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
      // Managed project registration is permitted only inside the operator
      // approved root. The fixture repository lives under this temp root.
      FACTORY_PROJECTS_ROOT: process.env.FACTORY_PROJECTS_ROOT?.trim() || temp,
      FACTORY_LEAD_FIXTURE_ASSESSOR: "1",
      FACTORY_ENABLE_AUTONOMOUS_START: "1",
      FACTORY_TOOL_GATEWAY_CONFIG: gatewayConfig,
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
      FACTORY_SUPERVISOR_PROJECT_ROOTS: temp,
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
  const context = await browser.newContext();
  const page = await context.newPage();
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
  await page.getByRole("heading", { name: "Home", level: 1 }).waitFor();
  // Spanish shell smoke: global switch, document language, persisted locale,
  // translated navigation, narrow drawer and document overflow. Switch back to
  // English so the rest of the deterministic journey keeps its English oracles.
  await page.getByLabel("Display language").selectOption("es");
  await page.getByRole("heading", { name: "Inicio", level: 1 }).waitFor();
  assert.equal(await page.evaluate(() => document.documentElement.lang), "es");
  assert.equal(
    await page.evaluate(() => localStorage.getItem("factory-office-locale")),
    "es",
  );
  await page
    .locator(".sidebar-nav")
    .getByRole("link", { name: "Campañas", exact: true })
    .waitFor();
  await page
    .locator(".sidebar-nav")
    .getByRole("link", { name: "Incidencias", exact: true })
    .waitFor();
  for (const width of [1440, 834, 390, 320]) {
    await page.setViewportSize({ width, height: 900 });
    const viewport = await page.evaluate(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    }));
    assert.equal(
      viewport.scrollWidth,
      viewport.clientWidth,
      `Spanish Home must not overflow the document at ${width}px`,
    );
  }
  await page
    .getByRole("button", { name: "Mostrar u ocultar la navegación" })
    .click();
  await page
    .locator(".sidebar-nav")
    .getByRole("link", { name: "Campañas", exact: true })
    .waitFor();
  await page
    .getByRole("button", { name: "Cerrar el panel de navegación" })
    .click();
  await page.reload();
  await page.getByRole("heading", { name: "Inicio", level: 1 }).waitFor();
  assert.equal(await page.evaluate(() => document.documentElement.lang), "es");
  await page.getByLabel("Idioma de visualización").selectOption("en");
  await page.getByRole("heading", { name: "Home", level: 1 }).waitFor();
  for (const width of [1440, 834, 390, 320]) {
    await page.setViewportSize({ width, height: 900 });
    const viewport = await page.evaluate(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    }));
    assert.equal(
      viewport.scrollWidth,
      viewport.clientWidth,
      `Home must not overflow the document at ${width}px`,
    );
  }
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto(`${viteUrl}/#/inbox`);
  await page
    .getByRole("heading", { name: "Decision Inbox", exact: true, level: 1 })
    .waitFor();
  await page.goto(`${viteUrl}/#/requests`);
  await page.getByRole("heading", { name: "Requests", level: 1 }).waitFor();

  await page.getByRole("link", { name: "Projects", exact: true }).click();
  await page.getByRole("heading", { name: "Projects", exact: true }).waitFor();
  await page
    .getByRole("tab", { name: "Local repository", exact: true })
    .click();
  // Playwright cannot drive the operating-system picker. A fake directory
  // handle exercises the same browser traversal, ZIP creation and Core upload
  // boundary as a human selection.
  await page.evaluate(() => {
    const file = new File(["seed\n"], "seed.txt", { type: "text/plain" });
    const entry = {
      kind: "file",
      name: "seed.txt",
      getFile: async () => file,
    };
    window.showDirectoryPicker = async () => ({
      kind: "directory",
      name: "browser-folder",
      entries: async function* () {
        yield ["seed.txt", entry];
      },
    });
  });
  await page
    .getByRole("button", { name: "Select folder from this computer" })
    .click();
  await page.waitForURL(/#\/projects\/[^/?]+/, { timeout: 60_000 });
  await page.getByRole("link", { name: "Back to Projects" }).waitFor();

  await page.getByRole("link", { name: "New request", exact: true }).click();
  await page.getByRole("heading", { name: "Home", level: 1 }).waitFor();
  for (const width of [1440, 834, 390, 320]) {
    await page.setViewportSize({ width, height: 900 });
    assert.equal(
      await page.getByLabel("Your instruction to the Lead").isVisible(),
      true,
      `Home composer must remain visible at ${width}px`,
    );
  }
  await page.setViewportSize({ width: 1440, height: 1000 });

  await page.getByRole("link", { name: "Issues", exact: true }).click();
  await page.getByRole("heading", { name: "Issues", exact: true }).waitFor();
  await page.getByText("GitHub setup").waitFor();

  await page.goto(`${viteUrl}/#/runtime`);
  await page
    .getByRole("heading", { name: "Activity & telemetry", exact: true })
    .waitFor();
  await page.getByRole("tab", { name: /Sessions/ }).click();
  await page
    .getByRole("heading", { name: "Provider sessions (0 shown)" })
    .waitFor();
  await page.getByText("No matching sessions").waitFor();
  await page.getByRole("tab", { name: /Model calls/ }).click();
  await page.getByRole("heading", { name: /Core-owned model calls/ }).waitFor();
  await page.getByRole("tab", { name: /Diagnostics/ }).click();
  await page.getByRole("heading", { name: "Service boundaries" }).waitFor();
  await page.getByRole("heading", { name: "MCP tool gateway" }).waitFor();
  await page.getByText("context7.query-docs").first().waitFor();
  await page.getByText("Read-only").first().waitFor();
  await page.getByText("No mediated MCP call has been recorded yet.").waitFor();
  const accessibility = await new AxeBuilder({ page })
    .include('[data-testid="tool-gateway-panel"]')
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"])
    .analyze();
  const blocking = accessibility.violations.filter(
    (violation) =>
      violation.impact === "critical" || violation.impact === "serious",
  );
  assert.deepEqual(
    blocking.map((violation) => violation.id),
    [],
    `Runtime MCP gateway accessibility violations: ${JSON.stringify(
      blocking.map((violation) => ({
        id: violation.id,
        nodes: violation.nodes.map((node) => node.target),
      })),
    )}`,
  );
  if (process.env.FACTORY_EVIDENCE_DIR) {
    mkdirSync(process.env.FACTORY_EVIDENCE_DIR, { recursive: true });
    await page.screenshot({
      path: join(
        process.env.FACTORY_EVIDENCE_DIR,
        "runtime-gateway-enabled-1440.png",
      ),
      fullPage: true,
    });
  }
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
    if (width === 390 && process.env.FACTORY_EVIDENCE_DIR) {
      await page.screenshot({
        path: join(
          process.env.FACTORY_EVIDENCE_DIR,
          "runtime-gateway-enabled-390.png",
        ),
        fullPage: true,
      });
    }
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

  await page.goto(`${viteUrl}/#/agents`);
  await page
    .getByRole("heading", { name: "Agent Office", exact: true, level: 1 })
    .waitFor();

  assert.deepEqual(
    pageErrors,
    [],
    `browser page errors: ${pageErrors.join("; ")}`,
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
  const unexpectedNotFound = notFound.filter(
    (url) => !/\/core-api\/quality\/candidates\/[^/]+\/gate$/.test(url),
  );
  assert.deepEqual(
    unexpectedNotFound,
    [],
    `unexpected 404 responses: ${unexpectedNotFound.join("; ")}`,
  );
  console.log(JSON.stringify({ ok: true, journey: "office" }));
} finally {
  if (browser) await browser.close().catch(() => {});
  for (const child of processes.reverse()) child.kill("SIGKILL");
  rmSync(temp, { recursive: true, force: true });
}
