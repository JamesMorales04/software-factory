// Real S4-D Graph Office journey. PHASE=submit enters the objective through
// the actual Lead UI; PHASE=verify reloads the accepted projection after a
// Core restart. No API mocking.
import { existsSync, writeFileSync } from "node:fs";
import { launchBrowser } from "./harness.mjs";

const BASE = process.env.BASE_URL || "";
const TOKEN = process.env.FACTORY_API_TOKEN || "";
const EXE = process.env.CHROMIUM_EXE || "";
const PHASE = process.env.PHASE || "submit";
const PROJECT_ID = process.env.PROJECT_ID || "";
const DELEGATION_ID = process.env.DELEGATION_ID || "";
const OBJECTIVE = process.env.OBJECTIVE || "";
const REQUEST_ID = process.env.REQUEST_ID || "";
const RESULT_FILE = process.env.RESULT_FILE || "/tmp/s4d-graph-browser.json";

if (!BASE || !TOKEN || !EXE || !existsSync(EXE)) {
  throw new Error("BASE_URL, FACTORY_API_TOKEN and CHROMIUM_EXE are required");
}
if (
  !(
    BASE.startsWith("http://127.0.0.1:") || BASE.startsWith("http://localhost:")
  )
) {
  throw new Error("S4-D live journey requires a loopback Core");
}

const result = { phase: PHASE, ok: false, checks: [] };
function check(name, pass, detail = "") {
  result.checks.push({ name, pass, ...(detail ? { detail } : {}) });
  if (!pass) throw new Error(name);
}

let browser;
try {
  ({ browser } = await launchBrowser(EXE, {
    timeoutMs: 60000,
    headless: true,
  }));
  const context = await browser.newContext();
  await context.addInitScript((token) => {
    sessionStorage.setItem("factory-api-token", token);
  }, TOKEN);
  const page = await context.newPage();
  const consoleErrors = [];
  page.on("console", (m) => {
    if (m.type() === "error") consoleErrors.push(m.text());
  });
  page.on("pageerror", (e) => consoleErrors.push(String(e)));

  if (PHASE === "submit") {
    await page.goto(`${BASE}/#/lead`, { waitUntil: "load" });
    await page.locator('[data-testid="lead-project"]').selectOption(PROJECT_ID);
    await page.locator('[data-testid="lead-objective"]').fill(OBJECTIVE);
    await page.locator('[data-testid="lead-delegation"]').fill(DELEGATION_ID);
    await page.getByRole("button", { name: "Submit objective" }).click();
    await page.waitForURL(/#\/lead\?request=/, { timeout: 360000 });
    const url = new URL(page.url());
    const requestId = url.hash.split("request=")[1];
    check(
      "UI submitted the objective and bound a request ID",
      Boolean(requestId),
      requestId,
    );
    await page.waitForFunction(
      () => /running|accepted|failed|blocked/i.test(document.body.innerText),
      undefined,
      { timeout: 360000 },
    );
    const body = await page.locator("body").innerText();
    check(
      "UI reflects an authorized autonomous state",
      /running|accepted/i.test(body),
      body.slice(0, 240),
    );
    result.requestId = requestId;
  } else {
    await page.goto(`${BASE}/#/lead?request=${REQUEST_ID}`, {
      waitUntil: "load",
    });
    await page.waitForFunction(
      () => /accepted/i.test(document.body.innerText),
      undefined,
      { timeout: 180000 },
    );
    const body = await page.locator("body").innerText();
    check("reloaded Office projection shows ACCEPTED", /accepted/i.test(body));
    check(
      "reloaded Office projection retains the request identity",
      body.includes(REQUEST_ID),
    );
    check(
      "event history is rendered",
      body.includes("LeadPlanFrozen") ||
        body.includes("LeadRequestAutoStarted") ||
        body.includes("LeadRequestCreated"),
    );
    await page.reload({ waitUntil: "load" });
    await page.waitForFunction(
      () => /accepted/i.test(document.body.innerText),
      undefined,
      { timeout: 180000 },
    );
    const afterReload = await page.locator("body").innerText();
    check(
      "accepted state survives a second reload",
      /accepted/i.test(afterReload),
    );
  }
  check(
    "browser produced no console errors",
    consoleErrors.length === 0,
    consoleErrors.join("; "),
  );
  result.ok = true;
} catch (error) {
  result.error = String(error);
} finally {
  result.consoleErrors = result.consoleErrors || [];
  writeFileSync(RESULT_FILE, `${JSON.stringify(result, null, 2)}\n`);
  if (browser) await browser.close();
}
if (!result.ok) process.exit(1);
