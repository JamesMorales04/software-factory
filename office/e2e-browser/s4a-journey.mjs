// F1-S4-A real-browser acceptance: candidate, independent verification,
// independent review and the deterministic Quality Gate are visible from
// Core-authoritative projections for one ACCEPTED and one REPAIR_REQUIRED
// candidate. Uses the existing playwright-core harness and a real Chromium.
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { launchBrowser, withTimeout } from "./harness.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const BASE = (process.env.BASE_URL || "http://127.0.0.1:8480").replace(
  /\/$/,
  "",
);
const TOKEN = process.env.FACTORY_API_TOKEN;
const EXE = process.env.CHROMIUM_EXE;
const EVIDENCE_DIR =
  process.env.S4A_EVIDENCE_DIR || join(HERE, ".s4a-evidence");
const ACCEPTED_RUN = process.env.S4A_ACCEPTED_RUN || "";
const ACCEPTED_CANDIDATE = process.env.S4A_ACCEPTED_CANDIDATE || "";
const ACCEPTED_SHA = process.env.S4A_ACCEPTED_SHA || "";
const REJECTED_RUN = process.env.S4A_REJECTED_RUN || "";
const REJECTED_CANDIDATE = process.env.S4A_REJECTED_CANDIDATE || "";
const REJECTED_SHA = process.env.S4A_REJECTED_SHA || "";

if (!TOKEN || !EXE || !existsSync(EXE)) {
  console.log(
    JSON.stringify({ ok: false, error: "token and Chromium are required" }),
  );
  process.exit(1);
}
if (!ACCEPTED_RUN || !REJECTED_RUN) {
  console.log(
    JSON.stringify({
      ok: false,
      error: "accepted/rejected run ids are required",
    }),
  );
  process.exit(1);
}
mkdirSync(EVIDENCE_DIR, { recursive: true });

const checks = [];
let page;
let closeBrowser;
const consoleErrors = [];

function check(name, pass, detail = "") {
  checks.push({ name, pass, ...(detail ? { detail } : {}) });
  if (!pass) throw new Error(`${name}${detail ? `: ${detail}` : ""}`);
}

async function shot(name) {
  await withTimeout(
    page.screenshot({
      path: join(EVIDENCE_DIR, `s4a-${name}.png`),
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

async function openRun(runId) {
  await page.goto(`${BASE}/#/runs/${runId}`, {
    waitUntil: "domcontentloaded",
  });
  await page.getByTestId("candidate-panel").waitFor({ timeout: 20000 });
  await page.getByTestId("candidate-sha").waitFor({ timeout: 20000 });
}

async function gateText() {
  return (await page.getByTestId("gate-status").innerText()).trim();
}

async function checkRowStatus(checkId) {
  const row = page.locator('[data-testid="verification-checks"] tbody tr', {
    hasText: checkId,
  });
  await row.waitFor({ timeout: 10000 });
  return (await row.innerText()).trim();
}

async function unlock() {
  await page.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });
  const gate = page.getByPlaceholder("FACTORY_API_TOKEN");
  if ((await gate.count()) > 0) {
    await gate.fill(TOKEN);
    await page.getByRole("button", { name: "Unlock" }).click();
  }
  await page
    .getByRole("heading", { name: "Dashboard" })
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
  await shot("dashboard");

  await openRun(ACCEPTED_RUN);
  const acceptedSha = await page.getByTestId("candidate-sha").innerText();
  check(
    "accepted run renders the exact frozen candidate SHA",
    acceptedSha.includes(ACCEPTED_SHA),
    acceptedSha,
  );
  const acceptedGate = await gateText();
  check(
    "accepted candidate shows ACCEPTED from the Core gate",
    acceptedGate.includes("ACCEPTED"),
    acceptedGate,
  );
  const browserRow = await checkRowStatus("browser");
  check(
    "independent verifier browser check is PASS",
    browserRow.includes("PASS"),
    browserRow,
  );
  const functionalRow = await checkRowStatus("functional");
  check(
    "independent functional check is PASS",
    functionalRow.includes("PASS"),
    functionalRow,
  );
  const reviewer = await page
    .getByTestId("reviewer-verdict")
    .first()
    .innerText();
  check(
    "independent reviewer verdict is APPROVE",
    reviewer.includes("APPROVE"),
    reviewer,
  );
  const specIdentity = await page.getByTestId("spec-identity").innerText();
  check(
    "frozen spec identity and policy are visible",
    specIdentity.includes("quality-baseline-s4a-v1"),
    specIdentity.slice(0, 200),
  );
  await shot("accepted");

  await page.reload({ waitUntil: "domcontentloaded" });
  await page.getByTestId("candidate-sha").waitFor({ timeout: 20000 });
  const afterReload = await page.getByTestId("candidate-sha").innerText();
  check(
    "candidate state survives browser reload without invention",
    afterReload.includes(ACCEPTED_SHA),
    afterReload,
  );
  const reloadedGate = await gateText();
  check(
    "Quality Gate decision survives browser reload",
    reloadedGate.includes("ACCEPTED"),
    reloadedGate,
  );

  await openRun(REJECTED_RUN);
  const rejectedSha = await page.getByTestId("candidate-sha").innerText();
  check(
    "rejected run renders its own candidate SHA",
    rejectedSha.includes(REJECTED_SHA),
    rejectedSha,
  );
  check(
    "rejected candidate has a different identity from accepted",
    !rejectedSha.includes(ACCEPTED_SHA),
  );
  const rejectedGate = await gateText();
  check(
    "rejected candidate shows REPAIR_REQUIRED",
    rejectedGate.includes("REPAIR_REQUIRED"),
    rejectedGate,
  );
  const failedBrowserRow = await checkRowStatus("browser");
  check(
    "browser regression is visible as FAIL",
    failedBrowserRow.includes("FAIL"),
    failedBrowserRow,
  );
  const rejectedReviewer = await page
    .getByTestId("reviewer-verdict")
    .first()
    .innerText();
  check(
    "reviewer APPROVE did not override a failing mandatory check",
    rejectedReviewer.includes("APPROVE"),
    rejectedReviewer,
  );
  await shot("rejected");

  const acceptedApi = await api(
    `/api/v1/candidates/${ACCEPTED_CANDIDATE}/evidence`,
  );
  const rejectedApi = await api(
    `/api/v1/candidates/${REJECTED_CANDIDATE}/evidence`,
  );
  check(
    "UI accepted state matches Core decision",
    acceptedApi.bundle.decision.status === "ACCEPTED",
  );
  check(
    "UI rejected state matches Core decision",
    rejectedApi.bundle.decision.status === "REPAIR_REQUIRED",
  );
  check(
    "browser journey produced no console errors",
    consoleErrors.length === 0,
    consoleErrors.join(" | ").slice(0, 300),
  );

  writeFileSync(
    join(EVIDENCE_DIR, "s4a-browser-evidence.json"),
    JSON.stringify(
      {
        ok: true,
        acceptedRun: ACCEPTED_RUN,
        acceptedCandidate: ACCEPTED_CANDIDATE,
        acceptedSha: ACCEPTED_SHA,
        rejectedRun: REJECTED_RUN,
        rejectedCandidate: REJECTED_CANDIDATE,
        rejectedSha: REJECTED_SHA,
        acceptedGate: acceptedGate.replace(/\s+/g, " ").slice(0, 200),
        rejectedGate: rejectedGate.replace(/\s+/g, " ").slice(0, 200),
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
