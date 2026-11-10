// Real Chromium acceptance for the integrated Quality-first Wave 1/2 Office
// surfaces. It starts an isolated real Rust Core with the debug/test-support
// feature, seeds labelled fixture rows (no paid inference), starts the real
// Office Vite dev server, and drives every control and state through the
// browser. This is implementer self-test evidence, never independent
// acceptance or certification.
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:net";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { launchBrowser } from "./harness.mjs";

const root = resolve(import.meta.dirname, "../..");
const targetDir = process.env.CARGO_TARGET_DIR || join(root, "target");
const coreBin =
  process.env.FACTORY_CORE_BIN || join(targetDir, "debug", "factory-core");
const evidenceDir =
  process.env.WP10_EVIDENCE_DIR ||
  join(root, "docs/evidence/wave12-office-acceptance-2026-11-10");
const temp = join(
  process.env.TMPDIR || "/tmp/opencode",
  `factory-wp10-browser-${randomBytes(5).toString("hex")}`,
);
const processes = [];
const results = [];
const screenshots = [];
const consoleErrors = [];
const pageErrors = [];
const httpFailures = [];
const expectedHttpFailures = [];
let currentWidth = 1440;

mkdirSync(temp, { recursive: true });
mkdirSync(evidenceDir, { recursive: true });
const repository = join(temp, "repository");
mkdirSync(join(repository, "src"), { recursive: true });
writeFileSync(join(repository, "src", "lib.rs"), "// fixture repository\n");

const token = () => randomBytes(32).toString("hex");
const operator = token();
const workerBootstrap = token();
const verifier = token();
const attemptKey = token();
const masterKeyFile = join(temp, "master.key");
writeFileSync(masterKeyFile, token(), { mode: 0o600 });
const enrollmentCode = randomBytes(16).toString("hex");
const enrollmentFile = join(temp, "enrollment.json");
writeFileSync(
  enrollmentFile,
  JSON.stringify({
    codeSha256: createHash("sha256").update(enrollmentCode).digest("hex"),
    expiresAtMs: Date.now() + 30 * 60 * 1000,
  }),
  { mode: 0o600 },
);
const reviewerRoles = [
  "FUNCTIONAL",
  "ARCHITECTURE",
  "CODE_QUALITY",
  "SECURITY",
  "TESTING",
  "OPERABILITY",
];
const reviewerIdentities = reviewerRoles.map((role) => {
  const principal = `reviewer-${role.toLowerCase().replaceAll("_", "-")}`;
  const tokenFile = join(temp, `reviewer-${role.toLowerCase()}.token`);
  writeFileSync(tokenFile, token(), { mode: 0o600 });
  return { principalId: principal, role, tokenFile };
});
const reviewerIdentitiesJson = JSON.stringify(reviewerIdentities);

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

const LOG_TAIL = 12_000;
function launch(command, args, options = {}) {
  const child = spawn(command, args, {
    stdio: ["ignore", "pipe", "pipe"],
    ...options,
  });
  processes.push(child);
  child.log = "";
  const capture = (chunk) => {
    child.log = (child.log + chunk.toString()).slice(-LOG_TAIL);
  };
  child.stdout.on("data", capture);
  child.stderr.on("data", capture);
  return child;
}

function expectHttpFailure(pattern, status) {
  expectedHttpFailures.push({ pattern, status });
}

async function waitFor(url, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url, { signal: AbortSignal.timeout(1_000) });
      if (response.ok) return;
    } catch {
      // Not listening yet.
    }
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 100));
  }
  throw new Error(`timed out waiting for ${url}`);
}

function findChromium() {
  if (process.env.CHROMIUM_EXE) return process.env.CHROMIUM_EXE;
  const base = join(homedir(), ".cache/ms-playwright");
  const candidates = readdirSync(base)
    .filter((name) => name.startsWith("chromium-"))
    .sort()
    .reverse();
  for (const name of candidates) {
    for (const relative of ["chrome-linux64/chrome", "chrome-linux/chrome"]) {
      const path = join(base, name, relative);
      if (existsSync(path)) return path;
    }
  }
  throw new Error("no Chromium executable found in the Playwright cache");
}

async function screenshot(id, label, width = currentWidth) {
  const name = `${id}-${label}-${width}.png`;
  await page.screenshot({ path: join(evidenceDir, name) });
  screenshots.push(name);
}

async function screenshotAt(id, label, locator, width = currentWidth) {
  await locator.scrollIntoViewIfNeeded();
  await page.waitForTimeout(120);
  await screenshot(id, label, width);
}

async function setLanguage(language) {
  // The visible label follows the active locale, so target the compact header
  // select structurally rather than by its translated accessible name.
  await page
    .locator(".language-switcher-compact select")
    .selectOption(language);
  await page.evaluate(() => document.documentElement.lang);
}

async function assertNoOverflow(label) {
  const viewport = await page.evaluate(() => ({
    scrollWidth: document.documentElement.scrollWidth,
    clientWidth: document.documentElement.clientWidth,
  }));
  assert.ok(
    viewport.scrollWidth <= viewport.clientWidth + 1,
    `${label} overflows at ${currentWidth}px: ${viewport.scrollWidth} > ${viewport.clientWidth}`,
  );
}

async function check(id, control, expected, fn) {
  try {
    await fn();
    results.push({ id, control, expected, result: "PASS" });
    console.log(`PASS ${id} ${control}`);
    return true;
  } catch (error) {
    results.push({
      id,
      control,
      expected,
      result: "FAIL",
      error: error instanceof Error ? error.message : String(error),
    });
    console.error(
      `FAIL ${id} ${control}: ${error instanceof Error ? error.message : error}`,
    );
    try {
      await screenshot(id, "failure");
    } catch {
      // A screenshot failure must not hide the assertion failure.
    }
    return false;
  }
}

async function seed() {
  const cookies = await page.context().cookies();
  const csrf = cookies.find((cookie) => cookie.name === "factory_csrf")?.value;
  assert.ok(csrf, "the authenticated browser must hold a CSRF cookie");
  const response = await page.evaluate(async (tokenValue) => {
    const result = await fetch("/core-api/test-support/wave12/seed", {
      method: "POST",
      credentials: "include",
      headers: {
        "content-type": "application/json",
        "x-factory-csrf": tokenValue,
      },
      body: "{}",
    });
    return { status: result.status, body: await result.json() };
  }, csrf);
  assert.equal(response.status, 200, JSON.stringify(response.body));
  return response.body;
}

async function armTraceFault(requestId) {
  const cookies = await page.context().cookies();
  const csrf = cookies.find((cookie) => cookie.name === "factory_csrf")?.value;
  const response = await page.evaluate(
    async ({ id, tokenValue }) => {
      const result = await fetch(
        `/core-api/test-support/wave12/trace-fault/${encodeURIComponent(id)}`,
        {
          method: "POST",
          credentials: "include",
          headers: {
            "content-type": "application/json",
            "x-factory-csrf": tokenValue,
          },
        },
      );
      return result.status;
    },
    { id: requestId, tokenValue: csrf },
  );
  assert.equal(response, 200);
}

function panel(name) {
  return page
    .locator(".ui-panel")
    .filter({ has: page.getByRole("heading", { name, exact: true }) });
}

async function gotoDirector(requestId) {
  await page.goto(
    `${viteUrl}/#/director?request=${encodeURIComponent(requestId)}`,
  );
  await page.getByRole("heading", { name: "Director", level: 1 }).waitFor();
  await page
    .getByRole("heading", { name: "Plan review", exact: true })
    .waitFor({ timeout: 20_000 });
}

async function gotoRequest(requestId) {
  await page.goto(`${viteUrl}/#/requests/${encodeURIComponent(requestId)}`);
  await page
    .getByRole("heading", { name: "Workflow trace", exact: true })
    .waitFor({ timeout: 20_000 });
}

async function gotoSkills() {
  await page.goto(`${viteUrl}/#/skills`);
  await page
    .getByRole("heading", { name: "Technology knowledge packs", exact: true })
    .waitFor({ timeout: 20_000 });
}

let page;
let viteUrl;
let browser;
let launchHandle;

try {
  assert.ok(
    existsSync(coreBin),
    `missing ${coreBin}; build factory-core first`,
  );
  assert.ok(
    existsSync(join(root, "office/node_modules/.bin/vite")),
    "missing Office Vite; run npm ci in office/",
  );

  const corePort = await freePort();
  const coreUrl = `http://127.0.0.1:${corePort}`;
  const vitePort = await freePort();
  viteUrl = `http://127.0.0.1:${vitePort}`;

  const core = launch(coreBin, [], {
    cwd: temp,
    env: {
      ...process.env,
      FACTORY_DATABASE: join(temp, "factory.sqlite"),
      FACTORY_BIND: `127.0.0.1:${corePort}`,
      FACTORY_OPERATOR_TOKEN: operator,
      FACTORY_WORKER_BOOTSTRAP_TOKEN: workerBootstrap,
      FACTORY_VERIFIER_TOKEN: verifier,
      FACTORY_ATTEMPT_GRANT_KEY: attemptKey,
      FACTORY_MASTER_KEY_FILE: masterKeyFile,
      FACTORY_ENROLLMENT_CODE_FILE: enrollmentFile,
      FACTORY_REVIEWER_IDENTITIES_JSON: reviewerIdentitiesJson,
      FACTORY_PROJECTS_ROOT: temp,
    },
  });
  await waitFor(`${coreUrl}/health`);
  assert.equal(core.exitCode, null, `Core exited: ${core.log}`);

  const vite = launch(
    join(root, "office/node_modules/.bin/vite"),
    ["--host", "127.0.0.1", "--port", String(vitePort), "--strictPort"],
    {
      cwd: join(root, "office"),
      env: { ...process.env, FACTORY_CORE_URL: coreUrl },
    },
  );
  await waitFor(viteUrl);
  assert.equal(vite.exitCode, null, `Vite exited: ${vite.log}`);

  launchHandle = await launchBrowser(findChromium(), { headless: true });
  browser = launchHandle.browser;
  page = launchHandle.page;
  page.setDefaultTimeout(20_000);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.addInitScript(() => {
    const original = console.error;
    console.error = function (...args) {
      if (String(args[0]).includes("same key")) {
        window.__wp10KeyWarnings = (window.__wp10KeyWarnings || []).concat({
          args: args.map((argument) => {
            try {
              return typeof argument === "string"
                ? argument
                : JSON.stringify(argument);
            } catch {
              return String(argument);
            }
          }),
          stack: new Error().stack,
        });
      }
      return original.apply(this, args);
    };
  });

  page.on("console", (message) => {
    if (message.type() !== "error") return;
    const record = { text: message.text(), url: page.url(), args: [] };
    consoleErrors.push(record);
    void Promise.all(
      message.args().map(async (argument) => {
        try {
          return await argument.jsonValue();
        } catch {
          return null;
        }
      }),
    ).then((args) => {
      record.args = args;
    });
  });
  page.on("pageerror", (error) => pageErrors.push(String(error)));
  page.on("response", (response) => {
    if (response.status() >= 400) {
      const url = response.url();
      const expected = expectedHttpFailures.some(
        (entry) =>
          entry.status === response.status() && entry.pattern.test(url),
      );
      httpFailures.push({ status: response.status(), url, expected });
    }
  });

  await page.goto(viteUrl);
  await page
    .getByRole("heading", { name: "Set up this Factory" })
    .waitFor({ timeout: 20_000 });
  await page.getByLabel("One-time owner confirmation").fill(enrollmentCode);
  await page.getByLabel("This device name").fill("WP10 browser acceptance");
  await page.getByRole("button", { name: "Enroll this device" }).click();
  await page.getByRole("heading", { name: "Home", level: 1 }).waitFor();
  const seeded = await seed();
  assert.equal(seeded.seeded, true);

  await check(
    "WP10-UI-042",
    "Unauthorized Core read",
    "an operator projection without the device session is rejected with 401",
    async () => {
      expectHttpFailure(/\/core-api\/agentic\/portfolio$/, 401);
      const status = await page.evaluate(async () => {
        const response = await fetch("/core-api/agentic/portfolio", {
          credentials: "omit",
        });
        return response.status;
      });
      assert.equal(status, 401);
    },
  );

  const ids = seeded.requests;
  const tracePopulated = ids.tracePopulated;
  const traceEmpty = ids.traceEmpty;
  const traceLong = ids.traceLong;
  const traceRejected = ids.traceRejected;
  const traceDuplicate = ids.traceDuplicate;
  const traceRetry = ids.traceRetry;
  const traceGap = ids.traceGap;

  // ---------------------------------------------------------------- Director
  await gotoDirector(ids.planReady);
  await check(
    "WP10-UI-001",
    "Director plan lane, artifacts, stack, architecture, docs, skills, DoD and OpenSpec",
    "all summary facts come from the seeded Core planning projection",
    async () => {
      const planningPanel = panel("Plan review");
      const text = await planningPanel.innerText();
      assert.match(text, /FORMAL_SPEC/);
      assert.match(text, /rust · EXISTING/);
      assert.match(text, /2 component\(s\)/);
      assert.match(text, /2 required document\(s\)/);
      assert.match(text, /rust, react/);
      assert.match(text, /2 check\(s\)/);
      assert.match(text, /add-wave12-plan-review/);
      assert.match(text, /PENDING/);
      await screenshotAt("WP10-UI-001", "ready", planningPanel);
    },
  );

  await check(
    "WP10-UI-010",
    "Reviewer independence required roles and assigned principals",
    "five roles and five distinct assigned principals",
    async () => {
      const planPanel = panel("Planner WorkGraph");
      const roles = planPanel.locator(".director-reviewers li");
      assert.equal(await roles.count(), 5);
      const text = await planPanel.innerText();
      for (const principal of [
        "reviewer-functional",
        "reviewer-architecture",
        "reviewer-code-quality",
        "reviewer-security",
        "reviewer-testing",
      ]) {
        assert.match(text, new RegExp(principal));
      }
      assert.match(text, /Required reviewer roles/);
    },
  );

  await check(
    "WP10-UI-011",
    "Per-role reviewer receipts",
    "two PASS, one CHANGES_REQUESTED and one missing receipt are visible with evidence",
    async () => {
      const planPanel = panel("Planner WorkGraph");
      const text = await planPanel.innerText();
      assert.equal((text.match(/PASS · EXECUTED/g) ?? []).length, 2);
      assert.match(text, /CHANGES_REQUESTED · EXECUTED/);
      assert.match(text, /FAIL · EXECUTED/);
      assert.match(text, /No receipt/);
      assert.match(text, /2 of 5 independent reviewer receipts recorded/);
    },
  );

  await check(
    "WP10-UI-002",
    "Plan review rationale validation",
    "an empty rationale is rejected before any Core write",
    async () => {
      const reviewPanel = panel("Plan review");
      await reviewPanel.getByRole("button", { name: "Approve plan" }).click();
      await reviewPanel
        .getByText("Enter a rationale before reviewing the plan.", {
          exact: true,
        })
        .waitFor();
    },
  );

  await check(
    "WP10-UI-039",
    "Keyboard focus order and visible focus on plan review controls",
    "Tab moves from the rationale to each review button with a visible focus style",
    async () => {
      const reviewPanel = panel("Plan review");
      await reviewPanel.getByLabel("Review rationale").focus();
      const order = [];
      for (let index = 0; index < 3; index += 1) {
        await page.keyboard.press("Tab");
        const focused = await page.evaluate(() => {
          const element = document.activeElement;
          const style = element ? getComputedStyle(element) : null;
          return {
            text: element?.textContent?.trim() ?? "",
            outline: style?.outlineStyle ?? "none",
            outlineWidth: style?.outlineWidth ?? "0px",
            boxShadow: style?.boxShadow ?? "none",
          };
        });
        assert.ok(
          focused.outline !== "none" ||
            focused.outlineWidth !== "0px" ||
            focused.boxShadow !== "none",
          `focused control ${focused.text} has no visible focus style`,
        );
        order.push(focused.text);
      }
      assert.deepEqual(order, [
        "Approve plan",
        "Request changes",
        "Reject plan",
      ]);
    },
  );

  await check(
    "WP10-UI-003",
    "Approve the exact plan revision",
    "OWNER_APPROVED review and FROZEN workflow state",
    async () => {
      const reviewPanel = panel("Plan review");
      await reviewPanel
        .getByLabel("Review rationale")
        .fill("Owner approved the seeded plan for browser acceptance.");
      await reviewPanel.getByRole("button", { name: "Approve plan" }).click();
      await reviewPanel.getByText("OWNER_APPROVED", { exact: true }).waitFor();
      await reviewPanel.getByText("FROZEN", { exact: true }).waitFor();
      await reviewPanel
        .getByText("The plan review was recorded.", { exact: true })
        .waitFor();
      await screenshotAt("WP10-UI-003", "approved", reviewPanel);
    },
  );

  await check(
    "WP10-UI-004",
    "Missing Definition of Done artifact",
    "the missing artifact is named and Approve is disabled while request-changes remains available",
    async () => {
      await gotoDirector(ids.planMissing);
      const reviewPanel = panel("Plan review");
      await reviewPanel
        .getByText("Required artifacts are missing: DOD.", { exact: true })
        .waitFor();
      assert.equal(
        await reviewPanel
          .getByRole("button", { name: "Approve plan" })
          .isDisabled(),
        true,
      );
      assert.equal(
        await reviewPanel
          .getByRole("button", { name: "Request changes" })
          .isDisabled(),
        false,
      );
      // A success notice from the previously selected request must not leak
      // into this request's review panel.
      assert.equal(
        await reviewPanel
          .getByText("The plan review was recorded.", { exact: true })
          .count(),
        0,
      );
      await screenshotAt("WP10-UI-004", "missing", reviewPanel);
    },
  );

  await check(
    "WP10-UI-005",
    "Request changes on the plan",
    "CHANGES_REQUESTED review and REPLANNING workflow state",
    async () => {
      const reviewPanel = panel("Plan review");
      await reviewPanel
        .getByLabel("Review rationale")
        .fill("The owner requires the missing Definition of Done.");
      await reviewPanel
        .getByRole("button", { name: "Request changes" })
        .click();
      await reviewPanel
        .getByText("CHANGES_REQUESTED", { exact: true })
        .waitFor();
      await reviewPanel.getByText("REPLANNING", { exact: true }).waitFor();
    },
  );

  await check(
    "WP10-UI-006",
    "Reject the plan",
    "REJECTED review and BLOCKED workflow state",
    async () => {
      await gotoDirector(ids.planReject);
      const reviewPanel = panel("Plan review");
      await reviewPanel
        .getByLabel("Review rationale")
        .fill("The owner rejects the seeded plan for browser acceptance.");
      await reviewPanel.getByRole("button", { name: "Reject plan" }).click();
      await reviewPanel.getByText("REJECTED", { exact: true }).waitFor();
      await reviewPanel.getByText("BLOCKED", { exact: true }).waitFor();
      await screenshotAt("WP10-UI-006", "rejected", reviewPanel);
    },
  );

  await check(
    "WP10-UI-007",
    "Exact revision/hash guard",
    "a stale plan hash is refused by Core and the review stays pending",
    async () => {
      await gotoDirector(ids.planStale);
      expectHttpFailure(/\/plan-review$/, 409);
      const reviewPanel = panel("Plan review");
      await reviewPanel
        .getByLabel("Review rationale")
        .fill("This review binds an intentionally stale plan hash.");
      await reviewPanel.getByRole("button", { name: "Approve plan" }).click();
      await reviewPanel
        .getByText(/does not bind the current plan hash/)
        .waitFor();
      await reviewPanel.getByText("PENDING", { exact: true }).waitFor();
      await screenshotAt("WP10-UI-007", "stale-error", reviewPanel);
    },
  );

  await check(
    "WP10-UI-014",
    "Frozen reviewer assignment mismatch",
    "the alert is shown when required roles have no exact principal assignment",
    async () => {
      await gotoDirector(ids.reviewMismatch);
      const planPanel = panel("Planner WorkGraph");
      await planPanel
        .getByText(
          /The frozen reviewer plan does not bind the required roles to principals/,
        )
        .waitFor();
      assert.match(await planPanel.innerText(), /Not assigned/);
      const specPanel = panel("Formal specification");
      const specText = await specPanel.innerText();
      assert.match(
        specText,
        /No specification draft or frozen revision is recorded/,
      );
      assert.doesNotMatch(specText, /does not require a formal specification/);
      await screenshotAt("WP10-UI-014", "mismatch", planPanel);
    },
  );

  await check(
    "WP10-UI-015",
    "Resolve a reviewer finding with evidence",
    "the OPEN finding becomes RESOLVED and the Core disposition is projected",
    async () => {
      await gotoDirector(ids.planReady);
      const findingsPanel = panel("Reviewer findings");
      const resolve = findingsPanel.getByRole("button", {
        name: "Resolve finding",
      });
      assert.equal(await resolve.isDisabled(), true);
      await findingsPanel
        .getByLabel("Finding")
        .selectOption("finding-resolve-001");
      await findingsPanel
        .getByLabel("Evidence references (one per line or comma-separated)")
        .fill("");
      await resolve.click();
      await findingsPanel
        .getByText("Enter at least one evidence reference.", { exact: true })
        .waitFor();
      await findingsPanel
        .getByLabel("Evidence references (one per line or comma-separated)")
        .fill("test:wp10-browser-resolve");
      await resolve.click();
      await findingsPanel
        .getByText("The finding disposition was recorded.", { exact: true })
        .waitFor();
      const row = findingsPanel
        .locator(".director-findings li")
        .filter({ hasText: "finding-resolve-001" });
      await row.getByText("RESOLVED", { exact: true }).waitFor();
      await screenshotAt("WP10-UI-015", "resolved", findingsPanel);
    },
  );

  await check(
    "WP10-UI-017",
    "Refute requires an owner decision",
    "the Refute control stays disabled until a decision reference is entered",
    async () => {
      const findingsPanel = panel("Reviewer findings");
      await findingsPanel
        .getByLabel("Finding")
        .selectOption("finding-refute-001");
      const refute = findingsPanel.getByRole("button", {
        name: "Refute finding",
      });
      assert.equal(await refute.isDisabled(), true);
      await findingsPanel
        .getByLabel("Evidence references (one per line or comma-separated)")
        .fill("test:wp10-browser-refute");
      assert.equal(await refute.isDisabled(), true);
    },
  );

  await check(
    "WP10-UI-018",
    "Invalid owner decision reference",
    "Core rejects the refutation and the Office shows the error",
    async () => {
      const findingsPanel = panel("Reviewer findings");
      expectHttpFailure(/\/findings\/[^/]+\/disposition$/, 400);
      await findingsPanel
        .getByLabel("Owner decision reference (refutation only)")
        .fill("missing-owner-decision");
      await findingsPanel
        .getByRole("button", { name: "Refute finding" })
        .click();
      await findingsPanel
        .getByText(/owner decision reference does not exist/)
        .waitFor();
    },
  );

  await check(
    "WP10-UI-019",
    "Refute with the approved owner decision",
    "the OPEN finding becomes REFUTED with the authenticated owner principal",
    async () => {
      const findingsPanel = panel("Reviewer findings");
      await findingsPanel
        .getByLabel("Owner decision reference (refutation only)")
        .fill(seeded.ownerDecision);
      await findingsPanel
        .getByRole("button", { name: "Refute finding" })
        .click();
      await findingsPanel
        .getByText("The finding disposition was recorded.", { exact: true })
        .waitFor();
      const row = findingsPanel
        .locator(".director-findings li")
        .filter({ hasText: "finding-refute-001" });
      await row.getByText("REFUTED", { exact: true }).waitFor();
      await findingsPanel.getByText("0 open", { exact: true }).waitFor();
      await screenshotAt("WP10-UI-019", "refuted", findingsPanel);
    },
  );

  // ------------------------------------------------------------------- Trace
  await check(
    "WP10-UI-020",
    "Workflow trace loading state",
    "the loading label appears while the Core read is pending",
    async () => {
      let release;
      const gate = new Promise((resolvePromise) => {
        release = resolvePromise;
      });
      const pattern = `**/core-api/agentic/requests/${tracePopulated}/trace*`;
      await page.route(pattern, async (route) => {
        await gate;
        await route.continue();
      });
      const navigation = page.goto(`${viteUrl}/#/requests/${tracePopulated}`);
      await page
        .getByText("Loading workflow trace…", { exact: true })
        .waitFor({ timeout: 15_000 });
      release();
      await navigation;
      await page
        .getByRole("heading", { name: "Workflow trace", exact: true })
        .waitFor();
      await page.unroute(pattern);
    },
  );

  await check(
    "WP10-UI-023",
    "Populated run facts",
    "run identity, durable state, revision, replayed state, terminal outcome and counts",
    async () => {
      const tracePanel = page.locator(".request-trace");
      const facts = tracePanel.locator(".request-facts");
      const text = await facts.innerText();
      assert.match(text, /Durable state/);
      assert.match(text, /Accepted/);
      assert.match(text, /Replayed final state/);
      assert.match(text, /Terminal/);
      assert.match(text, /15 shown · limit 200/);
      await screenshotAt("WP10-UI-023", "populated", tracePanel);
    },
  );

  await check(
    "WP10-UI-024",
    "Trace alerts: malformed, open step and terminal",
    "each structured alert is labelled and carries its sequence or step detail",
    async () => {
      const tracePanel = page.locator(".request-trace");
      const text = await tracePanel.innerText();
      assert.match(text, /Malformed event/);
      assert.match(text, /Open step/);
      assert.match(text, /Terminal state/);
      assert.match(text, /still pending or running/);
    },
  );

  await check(
    "WP10-UI-028",
    "Step trace and event log",
    "durable steps, open marker, ordered events and redacted payloads",
    async () => {
      const tracePanel = page.locator(".request-trace");
      assert.equal(await tracePanel.locator(".request-trace-step").count(), 2);
      const stepLabels = await tracePanel
        .locator(".request-trace-step-head code")
        .allInnerTexts();
      assert.equal(
        new Set(stepLabels).size,
        stepLabels.length,
        `step labels must be distinguishable: ${stepLabels.join(", ")}`,
      );
      const text = await tracePanel.innerText();
      assert.match(text, /succeeded/i);
      assert.match(text, /running/i);
      assert.match(text, /Open in replay/);
      await tracePanel.locator(".request-trace-event details").first().click();
      await tracePanel
        .locator(".request-trace-event")
        .first()
        .getByText("[REDACTED]", { exact: true })
        .first()
        .waitFor();
      await screenshotAt("WP10-UI-028", "event-log", tracePanel);
    },
  );

  await check(
    "WP10-UI-029",
    "Negative redaction",
    "tokens, prompts, content, API keys and unknown values never reach the DOM",
    async () => {
      const bodyText = await page.locator("body").innerText();
      for (const marker of [
        "sk-proj-wp10-never-render-this",
        "raw prompt never rendered",
        "secret content never rendered",
        "api-key-never-rendered",
        "unknown-value-never-rendered",
        "raw prompt or token",
      ]) {
        assert.equal(
          bodyText.includes(marker),
          false,
          `redaction failed for ${marker}`,
        );
      }
    },
  );

  await check(
    "WP10-UI-025",
    "Missing-transition alert",
    "a gap in the durable state chain is reported",
    async () => {
      await gotoRequest(traceGap);
      const tracePanel = page.locator(".request-trace");
      await tracePanel
        .getByText("Missing transition", { exact: true })
        .waitFor();
      assert.match(await tracePanel.innerText(), /Expected|expected/);
      await screenshotAt("WP10-UI-025", "gap", tracePanel);
    },
  );

  await check(
    "WP10-UI-026",
    "Duplicate-sequence alert",
    "the test-support projection surfaces a duplicate sequence without inventing state",
    async () => {
      await gotoRequest(traceDuplicate);
      const tracePanel = page.locator(".request-trace");
      await tracePanel
        .getByText("Duplicate sequence", { exact: true })
        .waitFor();
      await screenshotAt("WP10-UI-026", "duplicate", tracePanel);
    },
  );

  await check(
    "WP10-UI-027",
    "Long lists and truncation",
    "200 events and 500 steps are shown with explicit truncation alerts",
    async () => {
      await page.evaluate(() => {
        window.__wp10KeyWarnings = [];
      });
      await gotoRequest(traceLong);
      const tracePanel = page.locator(".request-trace");
      await tracePanel.getByText("Trace truncated", { exact: true }).waitFor();
      await tracePanel
        .getByText("Step trace truncated", { exact: true })
        .waitFor();
      assert.equal(
        await tracePanel.locator(".request-trace-event").count(),
        200,
      );
      assert.equal(
        await tracePanel.locator(".request-trace-step").count(),
        500,
      );
      const text = await tracePanel.innerText();
      assert.match(text, /200 shown · limit 200/);
      assert.match(text, /500 step\(s\) · limit 500/);
      await screenshotAt("WP10-UI-027", "truncated", tracePanel);
    },
  );

  const keyWarnings = await page.evaluate(() => window.__wp10KeyWarnings ?? []);
  if (keyWarnings.length > 0) {
    console.log(`KEY_WARNINGS ${JSON.stringify(keyWarnings, null, 2)}`);
  }

  await check(
    "WP10-UI-030",
    "Rejected replay state",
    "a rejected replay is shown as unknown/rejected, never as in progress",
    async () => {
      await gotoRequest(traceRejected);
      const tracePanel = page.locator(".request-trace");
      await tracePanel
        .getByText("Unknown — replay rejected", { exact: true })
        .waitFor();
      await tracePanel.getByText("Replay rejected.", { exact: true }).waitFor();
      await screenshotAt("WP10-UI-030", "replay-rejected", tracePanel);
    },
  );

  await check(
    "WP10-UI-021",
    "No-run empty state",
    "the trace explains that no durable run exists",
    async () => {
      await gotoRequest(traceEmpty);
      await page
        .locator(".request-trace")
        .getByText("No workflow run yet", { exact: true })
        .waitFor();
    },
  );

  await check(
    "WP10-UI-022",
    "Core error and retry",
    "a Core failure shows the error state and Retry reads the real projection",
    async () => {
      expectHttpFailure(new RegExp(`/requests/${traceRetry}/trace`), 503);
      await armTraceFault(traceRetry);
      const rawStatus = await page.evaluate(
        async (id) =>
          (
            await fetch(`/core-api/agentic/requests/${id}/trace?limit=200`, {
              credentials: "include",
            })
          ).status,
        traceRetry,
      );
      assert.equal(rawStatus, 503, "the armed Core fault must return 503");
      await armTraceFault(traceRetry);
      await gotoRequest(traceRetry);
      const tracePanel = page.locator(".request-trace");
      try {
        await tracePanel
          .getByText("Workflow trace unavailable", { exact: true })
          .waitFor({ timeout: 10_000 });
      } catch (error) {
        const text = await tracePanel.innerText().catch(() => "(no panel)");
        throw new Error(`${error.message}\nTRACE PANEL:\n${text}`);
      }
      await tracePanel.getByText(/seeded trace fault/).waitFor();
      await screenshotAt("WP10-UI-022", "error", tracePanel);
      await tracePanel.getByRole("button", { name: "Retry trace" }).click();
      await tracePanel.locator(".request-facts").waitFor();
      await tracePanel
        .getByText(/plan review/i)
        .first()
        .waitFor();
      await screenshotAt("WP10-UI-022", "retry", tracePanel);
    },
  );

  // ------------------------------------------------------------------ Skills
  await gotoSkills();
  await check(
    "WP10-UI-032",
    "Technology pack due, refreshable and blocked counts",
    "Core reports three due/refreshable packs and a non-zero blocking count",
    async () => {
      const refreshPanel = page.locator(".skills-card").first();
      const facts = refreshPanel.locator("dl.knowledge-facts");
      const text = await facts.innerText();
      assert.match(text, /DUE OR REFRESHABLE\s*3/i);
      const blocked = Number((text.match(/BLOCKING\s*(\d+)/i) ?? [])[1] ?? "0");
      assert.ok(blocked > 0, "blocking pack count must be positive");
      await screenshot("WP10-UI-032", "counts");
    },
  );

  await check(
    "WP10-UI-033",
    "Source-only content-review requirement",
    "a source-only pack is labelled and its reason is visible",
    async () => {
      const packs = page.locator(".skills-pack");
      const typescript = packs.filter({ hasText: "typescript" });
      await typescript.waitFor();
      const text = await typescript.innerText();
      assert.match(text, /REFRESHABLE/);
      assert.match(text, /Source only; content review required/);
      const legacy = packs.filter({ hasText: "legacy-tool" });
      await legacy.waitFor();
      assert.match(await legacy.innerText(), /DUE/);
      assert.match(
        await legacy.innerText(),
        /no builtin refresh source exists/,
      );
      await screenshot("WP10-UI-033", "source-only");
    },
  );

  await check(
    "WP10-UI-035",
    "Skills loading state",
    "the loading label appears while the Core reads are pending",
    async () => {
      let release;
      const gate = new Promise((resolvePromise) => {
        release = resolvePromise;
      });
      const pattern = "**/core-api/agentic/skills";
      await page.route(pattern, async (route) => {
        await gate;
        await route.continue();
      });
      try {
        const navigation = page.reload();
        await page
          .getByText("Loading skills", { exact: true })
          .waitFor({ timeout: 15_000 });
        release();
        await navigation;
        await page
          .getByRole("heading", {
            name: "Technology knowledge packs",
            exact: true,
          })
          .waitFor();
      } finally {
        release();
        await page.unroute(pattern);
      }
    },
  );

  await check(
    "WP10-UI-036",
    "Skills error and retry",
    "a failed read shows the error state and Retry recovers from real Core",
    async () => {
      expectHttpFailure(/\/core-api\/agentic\/skills$/, 503);
      const pattern = "**/core-api/agentic/skills";
      await page.route(pattern, async (route) => {
        await route.fulfill({
          status: 503,
          contentType: "application/json",
          body: JSON.stringify({ error: "seeded browser network failure" }),
        });
      });
      try {
        await page.reload();
        await page
          .getByText("seeded browser network failure", { exact: true })
          .waitFor();
      } finally {
        await page.unroute(pattern);
      }
      await page.getByRole("button", { name: "Retry" }).click();
      await page
        .getByRole("heading", {
          name: "Technology knowledge packs",
          exact: true,
        })
        .waitFor();
      await screenshot("WP10-UI-036", "error-retry");
    },
  );

  await check(
    "WP10-UI-037",
    "No registered skills empty state",
    "the registry explains that no skill has been recorded",
    async () => {
      await page.getByText("No skills recorded", { exact: true }).waitFor();
    },
  );

  await check(
    "WP10-UI-034",
    "Run the explicit refresh pass",
    "Core refreshes exactly the due/refreshable persisted packs and keeps the blocked pack visible",
    async () => {
      const refreshPanel = page.locator(".skills-card").first();
      await refreshPanel
        .getByRole("button", { name: "Run refresh pass" })
        .click();
      await page
        .getByText(/Refresh pass complete: 2 refreshed, \d+ blocked/)
        .waitFor({ timeout: 20_000 });
      await page
        .locator(".skills-pack")
        .filter({ hasText: "legacy-tool" })
        .waitFor();
      const packs = page.locator(".skills-pack");
      assert.equal(
        await packs.filter({ hasText: "rust" }).count(),
        0,
        "rust should be fresh after the refresh",
      );
      assert.equal(
        await packs.filter({ hasText: "typescript" }).count(),
        0,
        "typescript should be fresh after the refresh",
      );
      await screenshot("WP10-UI-034", "refreshed");
    },
  );

  // ---------------------------------------------------- English/Spanish and widths
  await check(
    "WP10-UI-008",
    "Director English/Spanish responsive layout",
    "all review controls remain reachable without document overflow at 1440/834/390/320",
    async () => {
      await gotoDirector(ids.planStale);
      await setLanguage("es");
      await page
        .getByRole("heading", { name: "Revisión del plan", exact: true })
        .waitFor();
      const reviewPanel = panel("Revisión del plan");
      await reviewPanel.getByRole("button", { name: "Aprobar plan" }).waitFor();
      for (const width of [1440, 834, 390, 320]) {
        currentWidth = width;
        await page.setViewportSize({ width, height: 1000 });
        await assertNoOverflow(`Spanish Director ${width}`);
      }
      await screenshotAt("WP10-UI-008", "es", reviewPanel, 390);
      await setLanguage("en");
      await page
        .getByRole("heading", { name: "Plan review", exact: true })
        .waitFor();
      currentWidth = 1440;
      await page.setViewportSize({ width: 1440, height: 1000 });
    },
  );

  await check(
    "WP10-UI-031",
    "Trace English/Spanish responsive layout",
    "the trace panel has no document overflow and keeps the event log readable at all widths",
    async () => {
      await gotoRequest(tracePopulated);
      await setLanguage("es");
      await page
        .getByRole("heading", {
          name: "Traza del flujo de trabajo",
          exact: true,
        })
        .waitFor();
      for (const width of [1440, 834, 390, 320]) {
        currentWidth = width;
        await page.setViewportSize({ width, height: 1000 });
        await assertNoOverflow(`Spanish trace ${width}`);
      }
      await screenshotAt(
        "WP10-UI-031",
        "es",
        page.locator(".request-trace"),
        390,
      );
      await setLanguage("en");
      await page
        .getByRole("heading", { name: "Workflow trace", exact: true })
        .waitFor();
      currentWidth = 1440;
      await page.setViewportSize({ width: 1440, height: 1000 });
    },
  );

  await check(
    "WP10-UI-038",
    "Skills English/Spanish responsive layout",
    "the refresh panel and pack list have no document overflow at all widths",
    async () => {
      await gotoSkills();
      await setLanguage("es");
      await page
        .getByRole("heading", {
          name: "Paquetes de conocimiento tecnológico",
          exact: true,
        })
        .waitFor();
      for (const width of [1440, 834, 390, 320]) {
        currentWidth = width;
        await page.setViewportSize({ width, height: 1000 });
        await assertNoOverflow(`Spanish skills ${width}`);
      }
      await screenshotAt(
        "WP10-UI-038",
        "es",
        page.locator(".skills-card").first(),
        390,
      );
      await setLanguage("en");
      await page
        .getByRole("heading", {
          name: "Technology knowledge packs",
          exact: true,
        })
        .waitFor();
      currentWidth = 1440;
      await page.setViewportSize({ width: 1440, height: 1000 });
    },
  );

  await check(
    "WP10-UI-041",
    "No literal translation placeholders",
    "visible EN and ES surfaces do not render unresolved {placeholder} tokens",
    async () => {
      for (const visit of [
        async () => gotoDirector(ids.planReady),
        async () => gotoRequest(tracePopulated),
        async () => gotoSkills(),
      ]) {
        await visit();
        const bodyText = await page.locator("body").innerText();
        assert.doesNotMatch(
          bodyText,
          /\{[a-z][a-zA-Z]*\}/,
          `unresolved placeholder in ${page.url()}`,
        );
      }
    },
  );

  await check(
    "WP10-UI-009",
    "Keyboard and naming semantics",
    "interactive controls have accessible names and the document language follows the switch",
    async () => {
      await page
        .getByRole("heading", {
          name: "Technology knowledge packs",
          exact: true,
        })
        .waitFor();
      const unnamed = await page.evaluate(
        () =>
          Array.from(document.querySelectorAll("button"))
            .filter((element) => element.offsetParent !== null)
            .filter(
              (element) =>
                !element.getAttribute("aria-label") &&
                (element.textContent ?? "").trim() === "" &&
                !element.getAttribute("title"),
            ).length,
      );
      assert.equal(unnamed, 0, "visible buttons must have accessible names");
      assert.equal(
        await page.evaluate(() => document.documentElement.lang),
        "en",
      );
    },
  );

  await check(
    "WP10-UI-040",
    "Browser console, page and network cleanliness",
    "no page errors and no unexpected 4xx/5xx responses",
    async () => {
      assert.deepEqual(pageErrors, []);
      const unexpected = httpFailures.filter(
        (failure) =>
          !failure.expected &&
          !/\/favicon\.ico$/.test(failure.url) &&
          !/\/@vite\/client/.test(failure.url),
      );
      assert.deepEqual(unexpected, [], JSON.stringify(unexpected, null, 2));
    },
  );

  const unexpectedConsole = consoleErrors.filter(
    (message) => !message.text.startsWith("Failed to load resource:"),
  );
  assert.deepEqual(
    unexpectedConsole,
    [],
    `unexpected browser console errors: ${unexpectedConsole
      .map((message) => `${message.text} @ ${message.url}`)
      .join("; ")}`,
  );

  const candidateSha = execFileSync("git", ["rev-parse", "HEAD"], {
    cwd: root,
    encoding: "utf8",
  }).trim();
  const branch = execFileSync("git", ["branch", "--show-current"], {
    cwd: root,
    encoding: "utf8",
  }).trim();
  const manifest = {
    schemaVersion: 1,
    kind: "implementer-self-test",
    independentAcceptance: false,
    candidate: { sha: candidateSha, branch, baseCommit: "452bb40" },
    environment: {
      coreBinary: coreBin,
      coreMode: "debug/test-support",
      office: "real Vite dev server",
      browser: browser.version(),
      node: process.version,
      widths: [1440, 834, 390, 320],
      languages: ["en", "es"],
    },
    seededState:
      "Exact request/plan/artifact/participant/reviewer-receipt/finding/owner-decision/workflow-trace rows were written by the feature-gated test-support seed endpoint. The browser used the real Core HTTP API and SQLite projection. No paid model inference was used and the fixture grants no authority.",
    seededRequests: ids,
    matrix: results,
    screenshots,
    network: {
      httpFailures,
      unexpected: httpFailures.filter((failure) => !failure.expected),
    },
    consoleErrors,
    pageErrors,
    limitations: [
      "Seeded Core state, not a model-produced planning campaign.",
      "The duplicate-sequence projection is injected by the test-support endpoint because the durable event primary key cannot hold a duplicate sequence; the projection still runs through the real trace module.",
      "The skill loading and read-error states use browser route delays/failures; the successful reads and refresh action use real Core.",
      "Implementer self-test only; a separate independent verifier and owner visual approval remain required.",
    ],
  };
  writeFileSync(
    join(evidenceDir, "manifest.json"),
    `${JSON.stringify(manifest, null, 2)}\n`,
  );

  const failed = results.filter((entry) => entry.result === "FAIL");
  console.log(
    JSON.stringify({
      ok: failed.length === 0,
      checks: results.length,
      failed: failed.map((entry) => entry.id),
      manifest: join(evidenceDir, "manifest.json"),
    }),
  );
  if (failed.length > 0) process.exitCode = 1;
} catch (error) {
  console.error(error);
  try {
    if (page) await screenshot("WP10-FATAL", "fatal");
  } catch {
    // Best effort.
  }
  process.exitCode = 1;
} finally {
  try {
    if (launchHandle) await launchHandle.close();
  } catch {
    // Bounded close already reports failure through its return value.
  }
  for (const child of processes.reverse()) {
    try {
      child.kill("SIGKILL");
    } catch {
      // Already dead.
    }
  }
  rmSync(temp, { recursive: true, force: true });
}
