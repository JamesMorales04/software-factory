// F1-S4-B real-browser acceptance: a user starts an authorized campaign in
// Factory Office, observes the Core-owned task graph, candidate/evidence and
// final Quality Gate outcome, then reloads and confirms durable state.
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { launchBrowser, withTimeout } from "./harness.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const BASE = (process.env.BASE_URL || "http://127.0.0.1:8490").replace(
  /\/$/,
  "",
);
const TOKEN = process.env.FACTORY_API_TOKEN;
const EXE = process.env.CHROMIUM_EXE;
const EVIDENCE_DIR =
  process.env.S4B_EVIDENCE_DIR || join(HERE, ".s4b-evidence");
const ACTION_ID = process.env.S4B_ACTION_ID || "";
const SPEC_ID = process.env.S4B_SPEC_ID || "";

if (!TOKEN || !EXE || !existsSync(EXE)) {
  console.log(
    JSON.stringify({ ok: false, error: "token and Chromium are required" }),
  );
  process.exit(1);
}
if (!ACTION_ID || !SPEC_ID) {
  console.log(
    JSON.stringify({ ok: false, error: "action and spec ids are required" }),
  );
  process.exit(1);
}
mkdirSync(EVIDENCE_DIR, { recursive: true });

const checks = [];
const consoleErrors = [];
let page;
let closeBrowser;
let campaignId = "";

function check(name, pass, detail = "") {
  checks.push({ name, pass, ...(detail ? { detail } : {}) });
  if (!pass) throw new Error(`${name}${detail ? `: ${detail}` : ""}`);
}

async function shot(name) {
  await withTimeout(
    page.screenshot({
      path: join(EVIDENCE_DIR, `s4b-${name}.png`),
      timeout: 15000,
    }),
    20000,
    `screenshot ${name}`,
  );
}

async function api(path) {
  const response = await fetch(BASE + path, {
    headers: { Authorization: `Bearer ${TOKEN}` },
    signal: AbortSignal.timeout(20000),
  });
  if (!response.ok) throw new Error(`API ${path} returned ${response.status}`);
  return response.json();
}

async function waitForText(locator, text, timeout = 240000) {
  const deadline = Date.now() + timeout;
  let latest = "";
  while (Date.now() < deadline) {
    try {
      latest = (await locator.innerText()).trim();
      if (latest.includes(text)) return latest;
    } catch {
      // The element may not be attached yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`timed out waiting for "${text}" (last "${latest}")`);
}

function campaignIdFromURL() {
  const match = page.url().match(/#\/campaigns\/([0-9a-f]{32})/);
  return match ? match[1] : "";
}

async function unlock() {
  await page.goto(`${BASE}/#/campaigns`, { waitUntil: "domcontentloaded" });
  const gate = page.getByPlaceholder("FACTORY_API_TOKEN");
  if ((await gate.count()) > 0) {
    await gate.fill(TOKEN);
    await page.getByRole("button", { name: "Unlock" }).click();
  }
  await page
    .getByRole("heading", { name: "Campaigns", exact: true })
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
  await shot("campaigns-empty");

  const actionSelect = page.getByTestId("campaign-action");
  await actionSelect.waitFor({ timeout: 20000 });
  await actionSelect.selectOption(ACTION_ID);
  await page.waitForFunction(
    (specId) =>
      Array.from(
        document.querySelectorAll('[data-testid="campaign-spec"] option'),
      ).some((option) => option.value === specId),
    SPEC_ID,
    { timeout: 20000 },
  );
  await page.getByTestId("campaign-spec").selectOption(SPEC_ID);
  await page.locator('input[type="number"]').fill("0");
  await page.getByRole("button", { name: "Create & start campaign" }).click();

  await page.waitForURL(/#\/campaigns\/[0-9a-f]{32}/, { timeout: 30000 });
  campaignId = campaignIdFromURL();
  check(
    "browser created and started a campaign",
    campaignId !== "",
    campaignId,
  );

  const status = await waitForText(
    page.getByTestId("campaign-status"),
    "accepted",
  );
  check(
    "campaign reaches ACCEPTED from the browser journey",
    status.includes("accepted"),
    status,
  );

  const projection = await api(`/api/v1/campaigns/${campaignId}`);
  check(
    "campaign is bound to the authorized Action and frozen spec",
    projection.campaign.actionId === ACTION_ID &&
      projection.campaign.specId === SPEC_ID,
  );
  check(
    "all four native role tasks completed",
    projection.tasks.length === 4 &&
      projection.tasks.every((task) => task.status === "succeeded"),
    JSON.stringify(projection.tasks.map((task) => [task.key, task.status])),
  );
  const runner = projection.tasks.find((task) => task.key === "implementer-0");
  const verifier = projection.tasks.find((task) => task.key === "verifier-0");
  const reviewer = projection.tasks.find((task) => task.key === "reviewer-0");
  check(
    "browser shows distinct Factory-assigned participant identities",
    runner?.runId &&
      verifier?.participantId &&
      reviewer?.participantId &&
      verifier.participantId !== reviewer.participantId,
  );
  check(
    "Quality Gate accepted the exact frozen candidate",
    projection.decision?.status === "ACCEPTED" &&
      projection.decision?.candidateSha === projection.candidate?.commitSha,
  );

  const sha = await page.getByTestId("campaign-candidate-sha").innerText();
  const gate = await page.getByTestId("campaign-gate-status").innerText();
  const evidence = await page.getByTestId("campaign-evidence").innerText();
  check(
    "browser renders the candidate SHA and ACCEPTED gate",
    sha.includes(projection.candidate.commitSha) && gate.includes("ACCEPTED"),
  );
  check(
    "browser renders independent verifier/reviewer actors",
    evidence.includes(verifier.participantId) &&
      evidence.includes(reviewer.participantId) &&
      evidence.includes("APPROVE"),
    evidence.slice(0, 200),
  );
  const taskRow = await page
    .getByTestId("campaign-task-implementer-0")
    .innerText();
  check(
    "browser renders the real task graph row",
    taskRow.includes("succeeded"),
    taskRow,
  );
  await shot("accepted");

  await page.reload({ waitUntil: "domcontentloaded" });
  await waitForText(page.getByTestId("campaign-status"), "accepted", 30000);
  const afterReload = await page
    .getByTestId("campaign-candidate-sha")
    .innerText();
  const gateAfterReload = await page
    .getByTestId("campaign-gate-status")
    .innerText();
  check(
    "campaign state survives reload without invention",
    afterReload.includes(projection.candidate.commitSha) &&
      gateAfterReload.includes("ACCEPTED"),
  );

  check(
    "browser journey produced no console errors",
    consoleErrors.length === 0,
    consoleErrors.join(" | ").slice(0, 300),
  );

  writeFileSync(
    join(EVIDENCE_DIR, "s4b-browser-evidence.json"),
    JSON.stringify(
      {
        ok: true,
        campaignId,
        actionId: ACTION_ID,
        specId: SPEC_ID,
        candidateId: projection.candidate.id,
        candidateSha: projection.candidate.commitSha,
        verifier: verifier.participantId,
        reviewer: reviewer.participantId,
        decision: projection.decision.status,
        taskCount: projection.tasks.length,
        accepted: true,
        checks,
        consoleErrors,
        finishedAt: new Date().toISOString(),
      },
      null,
      2,
    ),
  );
  ok = true;
} catch (e) {
  failure = String(e?.message ?? e);
} finally {
  if (closeBrowser) await closeBrowser();
}
console.log(JSON.stringify({ ok, error: failure || undefined, checks }));
process.exit(ok ? 0 : 1);
