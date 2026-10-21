// F1-S4-C complete Office journey: register Project A through the real
// form, create an Action, freeze a Project-bound ActionSpec, start the
// Campaign and inspect the terminal Quality Gate decision. Core-authoritative
// APIs are used only to cross-check what the browser rendered.
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { launchBrowser, withTimeout } from "./harness.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const BASE = (process.env.BASE_URL || "http://127.0.0.1:8492").replace(
  /\/$/,
  "",
);
const TOKEN = process.env.FACTORY_API_TOKEN;
const EXE = process.env.CHROMIUM_EXE;
const VLOG_REPO = process.env.S4C_VLOG_REPO;
const EVIDENCE_DIR =
  process.env.S4C_JOURNEY_EVIDENCE || join(HERE, ".s4c-evidence");
const JOURNEY_FILE = process.env.S4C_JOURNEY_FILE;
if (!TOKEN || !EXE || !existsSync(EXE) || !VLOG_REPO) {
  console.log(
    JSON.stringify({
      ok: false,
      error: "token, Chromium and vlog repo are required",
    }),
  );
  process.exit(1);
}
mkdirSync(EVIDENCE_DIR, { recursive: true });

const checks = [];
const consoleErrors = [];
let page;
let closeBrowser;
const projectId = "vlog";
let actionId = "";
let specId = "";
let campaignId = "";

function check(name, pass, detail = "") {
  checks.push({ name, pass, ...(detail ? { detail } : {}) });
  if (!pass) throw new Error(`${name}${detail ? `: ${detail}` : ""}`);
}

async function shot(name) {
  await withTimeout(
    page.screenshot({
      path: join(EVIDENCE_DIR, `s4c-${name}.png`),
      timeout: 15000,
    }),
    20000,
    `screenshot ${name}`,
  );
}

async function api(path, init = {}) {
  const response = await fetch(BASE + path, {
    ...init,
    headers: {
      Authorization: `Bearer ${TOKEN}`,
      "Content-Type": "application/json",
      ...(init.headers || {}),
    },
    signal: AbortSignal.timeout(20000),
  });
  if (!response.ok) throw new Error(`API ${path} returned ${response.status}`);
  return response.json();
}

async function waitForCampaignTerminal(id, timeout = 420000) {
  const deadline = Date.now() + timeout;
  let latest = null;
  while (Date.now() < deadline) {
    latest = await api(`/api/v1/campaigns/${id}`);
    const status = latest.campaign.status;
    if (
      [
        "accepted",
        "repair_required",
        "blocked",
        "human_required",
        "failed",
        "cancelled",
      ].includes(status)
    ) {
      return latest;
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error(
    `campaign did not reach a terminal state (last ${latest?.campaign?.status ?? "unknown"})`,
  );
}

async function waitForText(locator, text, timeout = 300000) {
  const deadline = Date.now() + timeout;
  let latest = "";
  while (Date.now() < deadline) {
    try {
      latest = (await locator.innerText()).trim();
      if (latest.includes(text)) return latest;
    } catch {
      // element may be re-rendered
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`timed out waiting for "${text}" (last "${latest}")`);
}

async function unlock() {
  await page.goto(`${BASE}/#/projects`, { waitUntil: "domcontentloaded" });
  const gate = page.getByPlaceholder("FACTORY_API_TOKEN");
  if ((await gate.count()) > 0) {
    await gate.fill(TOKEN);
    await page.getByRole("button", { name: "Unlock" }).click();
  }
  await page
    .getByRole("heading", { name: "Projects", exact: true })
    .waitFor({ timeout: 15000 });
}

let ok = false;
let failure = "";
try {
  ({ page, close: closeBrowser } = await launchBrowser(EXE, {
    timeoutMs: 60000,
    headless: true,
  }));
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });

  await unlock();
  await shot("projects-empty");

  await page.getByTestId("project-id").fill(projectId);
  await page.getByTestId("project-name").fill("Field Notes Vlog");
  await page.getByTestId("project-repository").fill(VLOG_REPO);
  await page.getByTestId("project-base-refs").fill("main");
  await page.getByTestId("project-profile").selectOption("node-browser");
  const browserCheckbox = page.getByRole("checkbox", {
    name: "browser",
    exact: true,
  });
  await browserCheckbox.check();
  await page
    .getByTestId("project-browser-endpoints")
    .fill("http://127.0.0.1:4173");
  await page.getByRole("button", { name: "Register Project" }).click();
  await page
    .getByTestId(`project-card-${projectId}`)
    .waitFor({ timeout: 20000 });
  const revision = await page
    .getByTestId(`project-revision-${projectId}`)
    .innerText();
  check(
    "browser registered Project A with a durable configuration",
    revision === "1",
    revision,
  );
  await shot("project-registered");

  await page.goto(`${BASE}/#/actions`, { waitUntil: "domcontentloaded" });
  await page.getByTestId("action-project").selectOption(projectId);
  await page
    .getByPlaceholder("What should the Factory do?")
    .fill("Implement category filtering with a no-results state");
  await page
    .getByPlaceholder("Acceptance sketch, context, constraints…")
    .fill("Preserve navigation, accessibility and the existing vlog entries.");
  await page.getByRole("button", { name: "Create Action" }).click();
  await page
    .getByRole("link", {
      name: "Implement category filtering with a no-results state",
    })
    .click();
  await page
    .getByRole("heading", {
      name: "Implement category filtering with a no-results state",
    })
    .waitFor();
  actionId = page.url().match(/#\/actions\/([0-9a-f]{32})/)?.[1] || "";
  check("browser created a Project-bound Action", actionId !== "", actionId);
  await shot("action-created");

  await page.getByTestId("spec-form").waitFor({ timeout: 20000 });
  await page
    .getByTestId("spec-goal")
    .fill(
      "Implement functional category filtering with a no-results state, preserving navigation, accessibility and the existing vlog entries. Modify only src/filter.js and src/app.js; never edit tests, browser-check.mjs, package.json, data.js or render.js.",
    );
  await page.getByTestId("spec-paths").fill("src/filter.js, src/app.js");
  await page.getByTestId("spec-profile").selectOption("node-browser");
  const baseSelect = page.getByTestId("spec-base");
  await page.waitForFunction(
    () =>
      Array.from(
        document.querySelectorAll('[data-testid="spec-base"] option'),
      ).some((option) => option.value.length === 40),
    { timeout: 20000 },
  );
  await baseSelect.selectOption({ index: 1 });
  await page
    .getByRole("button", {
      name: "Apply visible node-browser command template",
    })
    .click();
  await page
    .getByTestId("acceptance-2-command")
    .fill(
      "node\n--input-type=module\n-e\nimport { filterEntries } from './src/filter.js'; const entries=[{category:'Travel'},{category:'Technology'}]; if(filterEntries(entries,'Travel').length!==1||filterEntries(entries,'Film').length!==0||filterEntries(entries,'All').length!==2) process.exit(1)",
    );
  await page
    .getByTestId("negative-0-command")
    .fill(
      "node\n--input-type=module\n-e\nimport { entries } from './src/data.js'; if(entries.length!==5) process.exit(1)",
    );
  await shot("spec-form");
  await page.getByTestId("freeze-spec").click();
  await page.getByText(/Frozen [0-9a-f]{32}/).waitFor({ timeout: 20000 });
  const specs = await api(`/api/v1/actions/${actionId}/specs`);
  specId = specs.specs[0]?.id || "";
  check("browser froze a Project-bound ActionSpec", specId !== "", specId);
  check(
    "frozen spec records the Project identity and capabilities",
    specs.specs[0]?.repositoryIdentity?.length === 64 &&
      specs.specs[0]?.capabilities?.includes("browser"),
  );
  await shot("spec-frozen");

  await page.goto(`${BASE}/#/campaigns`, { waitUntil: "domcontentloaded" });
  await page.getByRole("heading", { name: "Campaigns", exact: true }).waitFor();
  const selects = page.locator("select");
  await selects.nth(1).selectOption(actionId);
  await page.waitForFunction(
    (wanted) =>
      Array.from(document.querySelectorAll("select")[2]?.options ?? []).some(
        (option) => option.value === wanted,
      ),
    specId,
    { timeout: 20000 },
  );
  await selects.nth(2).selectOption(specId);
  await page.locator('input[type="number"]').fill("0");
  await page.getByRole("button", { name: "Create & start campaign" }).click();
  await page.waitForURL(/#\/campaigns\/[0-9a-f]{32}/, { timeout: 30000 });
  campaignId = page.url().match(/#\/campaigns\/([0-9a-f]{32})/)?.[1] || "";
  check(
    "browser created and started the Campaign",
    campaignId !== "",
    campaignId,
  );

  const projection = await waitForCampaignTerminal(campaignId);
  check(
    "Campaign reaches ACCEPTED through native orchestration",
    projection.campaign.status === "accepted",
    projection.campaign.terminalReason || projection.campaign.status,
  );
  await waitForText(page.getByTestId("campaign-status"), "accepted", 30000);
  check(
    "all native role tasks completed",
    projection.tasks.length === 4 &&
      projection.tasks.every((task) => task.status === "succeeded"),
    JSON.stringify(projection.tasks.map((task) => [task.key, task.status])),
  );
  check(
    "Quality Gate accepted the exact candidate",
    projection.decision?.status === "ACCEPTED" &&
      projection.decision?.candidateSha === projection.candidate?.commitSha,
  );
  const sha = await page.getByTestId("campaign-candidate-sha").innerText();
  const gate = await page.getByTestId("campaign-gate-status").innerText();
  check(
    "browser renders the candidate SHA and ACCEPTED gate",
    sha.includes(projection.candidate.commitSha) && gate.includes("ACCEPTED"),
  );
  await shot("campaign-accepted");

  await page.reload({ waitUntil: "domcontentloaded" });
  await waitForText(page.getByTestId("campaign-status"), "accepted", 30000);
  const reloadedSha = await page
    .getByTestId("campaign-candidate-sha")
    .innerText();
  check(
    "durable state survives reload",
    reloadedSha.includes(projection.candidate.commitSha),
  );
  await shot("campaign-reloaded");

  check(
    "no browser console errors in the Office journey",
    consoleErrors.length === 0,
    consoleErrors.join("; "),
  );
  ok = true;
} catch (error) {
  failure = String(error?.message ?? error);
} finally {
  if (closeBrowser) await closeBrowser();
}

const result = {
  ok,
  failure,
  projectId,
  actionId,
  specId,
  campaignId,
  checks,
  consoleErrors,
};
if (JOURNEY_FILE) writeFileSync(JOURNEY_FILE, JSON.stringify(result, null, 2));
console.log(JSON.stringify(result));
if (!ok) process.exit(1);
