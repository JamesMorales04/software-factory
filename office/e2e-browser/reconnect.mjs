// Real-browser continuity check across an actual Core stop/start outage.
// Env: BASE_URL, FACTORY_HOME, FACTORY_BIN, FACTORY_ADDR, CHROMIUM_EXE.
import { execFile } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { promisify } from "node:util";
import { launchBrowser } from "./harness.mjs";

const runFile = promisify(execFile);
const BASE = process.env.BASE_URL || "http://127.0.0.1:8471";
const HOME = process.env.FACTORY_HOME;
const FACTORY = process.env.FACTORY_BIN;
const EXE = process.env.CHROMIUM_EXE;
const TIMEOUT = 15000;
if (!HOME || !FACTORY || !EXE || !existsSync(EXE)) {
  console.log(
    JSON.stringify({
      ok: false,
      error: "FACTORY_HOME, FACTORY_BIN and CHROMIUM_EXE are required",
    }),
  );
  process.exit(1);
}

const factoryEnv = { ...process.env };
delete factoryEnv.FACTORY_API_TOKEN;
let token = "";

async function factory(command) {
  const result = await runFile(FACTORY, [command], {
    env: factoryEnv,
    timeout: 20000,
  });
  return result.stdout.trim();
}

async function api(path, init = {}) {
  const signal = AbortSignal.timeout(TIMEOUT);
  const response = await fetch(BASE + path, {
    ...init,
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
      ...init.headers,
    },
    signal,
  });
  if (!response.ok) {
    throw new Error(`API ${path} returned HTTP ${response.status}`);
  }
  return response.json();
}

let app;
let pageRef;
let streamRequests = 0;
const networkFailures = [];
const streamResponses = [];
const result = { ok: false, checks: [] };
function check(name, pass, detail = "") {
  result.checks.push({ name, pass, ...(detail ? { detail } : {}) });
  if (!pass) throw new Error(name + (detail ? `: ${detail}` : ""));
}

try {
  await factory("stop");
  const startOutput = await factory("start");
  check(
    "CLI starts Core and Office",
    startOutput.includes("Factory status: ready"),
  );
  token = readFileSync(`${HOME}/api.token`, "utf8").trim();
  check("CLI created private API token", token.length >= 32);
  app = await launchBrowser(EXE);
  const { page } = app;
  pageRef = page;
  const pageErrors = [];
  let malformedInjected = false;
  let signalMalformed;
  let releaseRecovery;
  const malformedResponse = new Promise((resolve) => {
    signalMalformed = resolve;
  });
  const waitForRecovery = new Promise((resolve) => {
    releaseRecovery = resolve;
  });
  page.on("pageerror", (error) => pageErrors.push(String(error)));
  page.on("request", (request) => {
    if (request.url().includes("/api/v1/events/stream")) streamRequests += 1;
  });
  page.on("requestfailed", (request) =>
    networkFailures.push(`${request.url()}: ${request.failure()?.errorText}`),
  );
  page.on("response", (response) => {
    if (response.url().includes("/api/v1/events/stream")) {
      streamResponses.push(response.status());
    }
  });
  await page.route("**/api/v1/events/stream**", async (route) => {
    if (!malformedInjected) {
      malformedInjected = true;
      signalMalformed();
      await route.fulfill({
        status: 200,
        contentType: "text/event-stream",
        body: "data: this-is-not-json\n\n",
      });
      return;
    }
    await waitForRecovery;
    await route.continue();
  });

  await page.goto(BASE, { waitUntil: "networkidle" });
  await page.getByPlaceholder("FACTORY_API_TOKEN").fill(token);
  await page.getByRole("button", { name: "Unlock" }).click();
  await page
    .getByRole("heading", { name: "Dashboard" })
    .waitFor({ timeout: TIMEOUT });
  await malformedResponse;
  await page
    .getByText("reconnecting", { exact: true })
    .waitFor({ timeout: TIMEOUT });
  check("malformed frame triggers replay reconnect", malformedInjected);
  releaseRecovery();
  await page.getByText("live", { exact: true }).waitFor({ timeout: TIMEOUT });
  check("real browser authenticated and recovered stream is live", true);

  const title = `outage-reconnect-${Date.now()}`;
  const created = await api("/api/v1/actions", {
    method: "POST",
    body: JSON.stringify({
      title,
      description: "real Core outage reconnect check",
    }),
  });
  await page
    .getByText("ActionCreated", { exact: true })
    .waitFor({ timeout: TIMEOUT });
  check("live stream delivered the created event", Boolean(created.action?.id));

  await factory("stop");
  await page
    .getByText("reconnecting", { exact: true })
    .waitFor({ timeout: TIMEOUT });
  check("browser reports reconnecting while Core is down", true);
  check(
    "browser remains responsive during outage",
    (await page.evaluate(() => document.visibilityState)) === "visible",
  );

  const restartOutput = await factory("start");
  check(
    "CLI restarts Core on the same SQLite database",
    restartOutput.includes("Factory status: ready"),
  );
  await page.getByText("live", { exact: true }).waitFor({ timeout: TIMEOUT });
  await page
    .getByText("CoreStarted", { exact: true })
    .first()
    .waitFor({ timeout: TIMEOUT });

  const { events } = await api("/api/v1/events?after=0&limit=500");
  const seqs = events.map((event) => event.globalSeq);
  const createdEvents = events.filter(
    (event) =>
      event.eventType === "ActionCreated" && event.payload?.title === title,
  );
  const restarted = events.filter((event) => event.eventType === "CoreStarted");
  check(
    "replay is contiguous and duplicate-free",
    seqs.every((seq, index) => seq === index + 1),
    `seq=${seqs.join(",")}`,
  );
  check("SQLite action survived outage restart", createdEvents.length === 1);
  check(
    "CoreStarted replayed exactly once for restart",
    restarted.length === 2,
    `count=${restarted.length}`,
  );
  check(
    "Office rendered one created event",
    (await page.getByText("ActionCreated", { exact: true }).count()) === 1,
  );
  check(
    "Office rendered both boot events",
    (await page.getByText("CoreStarted", { exact: true }).count()) === 2,
  );
  check(
    "browser had no page errors",
    pageErrors.length === 0,
    pageErrors.join("; "),
  );

  result.ok = true;
} catch (error) {
  result.error = String(error).slice(0, 500);
  if (app) {
    result.diagnostics = {
      streamRequests,
      networkFailures,
      streamResponses,
      streamState: await pageRef
        .locator(".pill")
        .textContent()
        .catch(() => "unavailable"),
    };
  }
} finally {
  if (app && (await app.close())) {
    result.ok = false;
    result.error = "browser shutdown timed out";
  }
  try {
    await factory("stop");
  } catch (error) {
    result.ok = false;
    result.cleanupError = String(error).slice(0, 300);
  }
  console.log(JSON.stringify(result));
  if (!result.ok) process.exitCode = 1;
}
