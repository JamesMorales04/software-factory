// S3-C real-browser journey: real Core + real Worker + stubbed Paseo CLI.
// Phase "success": empty Agents state, live session appearance, functional
// filter, RunDetail session/evidence projection, durable reload, malformed
// SSE frame reconnect, duplicate-free bounded timeline, no source leakage.
// Phase "orphan": historical Agents state after Core restart, orphaned
// binding warning, cancellation wording and event lineage.
//
// Env: BASE_URL, FACTORY_API_TOKEN, CHROMIUM_EXE, S3C_PHASE,
//      S3C_EVIDENCE_DIR, S3C_ORPHAN_RUN_ID, S3C_ORPHAN_SESSION_ID.
import { existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { launchBrowser, withTimeout } from "./harness.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const BASE = process.env.BASE_URL || "http://127.0.0.1:8476";
const TOKEN = process.env.FACTORY_API_TOKEN;
const EXE = process.env.CHROMIUM_EXE;
const PHASE = process.env.S3C_PHASE || "success";
const EVIDENCE_DIR =
  process.env.S3C_EVIDENCE_DIR || join(HERE, ".s3c-evidence");
const ORPHAN_RUN_ID = process.env.S3C_ORPHAN_RUN_ID || "";
const ORPHAN_SESSION_ID = process.env.S3C_ORPHAN_SESSION_ID || "";
const MULTI_RUN_ID = process.env.S3C_MULTI_RUN_ID || "";

if (!TOKEN) {
  console.log(
    JSON.stringify({ ok: false, error: "FACTORY_API_TOKEN required" }),
  );
  process.exit(1);
}
if (!EXE || !existsSync(EXE)) {
  console.log(
    JSON.stringify({ ok: false, error: "CHROMIUM_EXE must point to Chromium" }),
  );
  process.exit(1);
}
mkdirSync(EVIDENCE_DIR, { recursive: true });

let app;
let page;
const checks = [];
function check(name, pass, detail = "") {
  checks.push({ name, pass, ...(detail ? { detail } : {}) });
  if (!pass) {
    throw new Error(`${name}${detail ? `: ${detail}` : ""}`);
  }
}
function fail(step, error) {
  console.log(
    JSON.stringify({
      ok: false,
      phase: PHASE,
      step,
      error: String(error?.message ?? error),
      checks,
      evidence: EVIDENCE_DIR,
    }),
  );
  process.exit(1);
}

async function api(path, init = {}) {
  const response = await fetch(BASE + path, {
    ...init,
    headers: {
      Authorization: `Bearer ${TOKEN}`,
      "Content-Type": "application/json",
      ...init.headers,
    },
    signal: AbortSignal.timeout(20000),
  });
  if (!response.ok) {
    throw new Error(`API ${path} returned HTTP ${response.status}`);
  }
  return response.json();
}

async function waitFor(predicate, timeoutMs, label) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    if (await predicate()) return;
    if (Date.now() >= deadline) throw new Error(`timeout: ${label}`);
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
}

const shot = (name) =>
  withTimeout(
    page.screenshot({
      path: join(EVIDENCE_DIR, `s3c-${PHASE}-${name}.png`),
      timeout: 15000,
    }),
    20000,
    `screenshot ${name}`,
  );

async function assertTimeline(ledger, label) {
  const rendered = await page.locator(".events li").evaluateAll((items) =>
    items.map((item) => ({
      type: item.getAttribute("data-event-type"),
      seq: Number(item.getAttribute("data-global-seq")),
    })),
  );
  const expected = ledger.map((event) => ({
    type: event.eventType,
    seq: event.globalSeq,
  }));
  check(
    label,
    JSON.stringify(rendered) === JSON.stringify(expected),
    `rendered=${JSON.stringify(rendered)}`,
  );
}

async function openAgents() {
  await page.goto(BASE, { waitUntil: "domcontentloaded" });
  const gate = page.getByPlaceholder("FACTORY_API_TOKEN");
  if ((await gate.count()) > 0) {
    await gate.fill(TOKEN);
    await page.getByRole("button", { name: "Unlock" }).click();
  }
  await page.getByRole("heading", { name: "Dashboard" }).waitFor({
    timeout: 15000,
  });
  await page.getByRole("link", { name: "Agents", exact: true }).click();
  await page.getByRole("heading", { name: "Agents", exact: true }).waitFor({
    timeout: 15000,
  });
}

async function queueTask() {
  const body = {
    workloadKind: "paseo-task",
    workloadInput: {
      task: "Append a pure function multiply(a, b) with a JSDoc comment to add.js and export it alongside add (keep add unchanged).",
    },
    executionMode: "trusted-local",
    provider: "codex",
    model: "gpt-6-luna",
    paseoMode: "full-access",
    timeoutSecs: 600,
    maxAttempts: 1,
  };
  const created = await api("/api/v1/runs", {
    method: "POST",
    body: JSON.stringify(body),
  });
  return created.run.id;
}

async function terminalRun(runId, timeoutMs = 150000) {
  let run;
  await waitFor(
    async () => {
      run = (await api(`/api/v1/runs/${runId}`)).run;
      return ["succeeded", "failed", "cancelled"].includes(run.status);
    },
    timeoutMs,
    `run ${runId} terminal`,
  );
  return run;
}

async function successPhase() {
  await openAgents();
  await page
    .getByText("No Paseo agent sessions have been recorded yet", {
      exact: false,
    })
    .waitFor({ timeout: 15000 });
  check("empty Agents state is explicit", true);
  await shot("empty");

  const runId = await queueTask();
  const run = await terminalRun(runId);
  check("real Factory Run succeeded", run.status === "succeeded", run.status);
  const sessions = await api(`/api/v1/runs/${runId}/sessions`);
  const session = sessions.sessions[0];
  check("one durable session binding exists", sessions.sessions.length === 1);
  check("binding succeeded", session.status === "succeeded", session.status);
  check(
    "binding keeps the requested route",
    session.provider === "codex" &&
      session.model === "gpt-6-luna" &&
      session.mode === "full-access",
  );
  check("binding has real Paseo identity", Boolean(session.sessionId));
  check("binding records the worker", session.workerId === "local-1");

  const card = page.locator(".session-card").first();
  await card.waitFor({ timeout: 20000 });
  await card
    .locator(".session-card-head .pill.succeeded")
    .waitFor({ timeout: 15000 });
  const cardText = (await card.textContent()) || "";
  for (const expected of [
    "codex",
    "gpt-6-luna",
    "full-access",
    "TRUSTED_LOCAL",
    "local-1",
    "seq 1",
    "generation 1",
    runId.slice(0, 8),
    session.sessionId.slice(0, 18),
  ]) {
    check(`Agents card shows ${expected}`, cardText.includes(expected));
  }
  check(
    "Agents card shows succeeded binding",
    (await card.locator(".session-card-head .pill.succeeded").count()) === 1,
  );
  await shot("live-session");

  await page.getByLabel("Filter sessions by status").selectOption("succeeded");
  await waitFor(
    async () => (await page.locator(".session-card").count()) === 1,
    10000,
    "succeeded filter",
  );
  await page.getByLabel("Filter sessions by provider").selectOption("opencode");
  await page
    .getByText("No sessions match these filters.", { exact: true })
    .waitFor({ timeout: 10000 });
  check("filters are server-backed and explicit when empty", true);
  await page.getByLabel("Filter sessions by provider").selectOption("");
  await page.getByLabel("Filter sessions by status").selectOption("");
  await page.locator(".session-card").first().waitFor({ timeout: 10000 });

  await card
    .getByRole("link", { name: runId.slice(0, 8), exact: true })
    .click();
  await page
    .getByRole("heading", { name: new RegExp(`Run ${runId.slice(0, 8)}`) })
    .waitFor({ timeout: 15000 });
  await page
    .getByText("Session evidence (final summary)", { exact: true })
    .waitFor({ timeout: 15000 });

  for (const label of [
    "Requested route",
    "Paseo session (1)",
    "Observed route",
    "pass (exit 0)",
    "Source diff",
    "Cleanup / stop",
    "Confirmed by the Worker",
  ]) {
    await page.getByText(label).first().waitFor({
      timeout: 15000,
    });
  }
  check(
    "both test and format checks passed",
    (await page.getByText("pass (exit 0)", { exact: true }).count()) === 2,
  );
  for (const label of [
    "Run queued",
    "Worker leased the Run",
    "Attempt started",
    "Paseo session intended",
    "Paseo session active",
    "Paseo session succeeded",
    "Run completed",
  ]) {
    await page.getByText(label, { exact: true }).first().waitFor({
      timeout: 15000,
    });
  }

  const ledger = (await api(`/api/v1/runs/${runId}/events`)).events;
  const eventItems = page.locator(".events li");
  await waitFor(
    async () => (await eventItems.count()) === ledger.length,
    15000,
    "ledger event count",
  );
  await assertTimeline(
    ledger,
    "lifecycle timeline equals the durable ledger in order",
  );
  const html = await page.content();
  check("task prompt is not rendered", !html.includes("multiply(a, b)"));
  check("source excerpt field is not rendered", !html.includes("diffExcerpt"));
  await shot("run-detail");

  // Durable reload plus a real malformed-frame reconnect. The second stream
  // attempt is delayed so the visible Reconnecting state is observable.
  let streamAttempts = 0;
  await page.route("**/api/v1/events/stream**", async (route) => {
    streamAttempts += 1;
    if (streamAttempts === 1) {
      await route.fulfill({
        status: 200,
        contentType: "text/event-stream",
        body: "data: this-is-not-json\n\n",
      });
      return;
    }
    if (streamAttempts === 2) {
      await new Promise((resolve) => setTimeout(resolve, 2000));
    }
    await route.continue();
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page
    .getByText("Session evidence (final summary)", { exact: true })
    .waitFor({ timeout: 20000 });
  await page
    .getByText("Reconnecting", { exact: true })
    .first()
    .waitFor({ timeout: 10000 });
  await page
    .getByText("Connected", { exact: true })
    .first()
    .waitFor({ timeout: 20000 });
  await waitFor(
    async () => (await eventItems.count()) === ledger.length,
    20000,
    "reloaded ledger event count",
  );
  await assertTimeline(
    ledger,
    "reload reconstructs the same durable timeline without duplicates",
  );
  check(
    "malformed SSE frame triggers a visible reconnect",
    streamAttempts >= 2,
    `attempts=${streamAttempts}`,
  );
  await shot("reconnected");
  await page.unroute("**/api/v1/events/stream**");

  return {
    runId,
    sessionId: session.sessionId,
    eventCount: ledger.length,
    streamAttempts,
  };
}

async function multiPhase() {
  check("multi-attempt run id provided", Boolean(MULTI_RUN_ID));
  await openAgents();
  await page.goto(`${BASE}/#/runs/${MULTI_RUN_ID}`, {
    waitUntil: "domcontentloaded",
  });
  await page
    .getByRole("heading", {
      name: new RegExp(`Run ${MULTI_RUN_ID.slice(0, 8)}`),
    })
    .waitFor({ timeout: 15000 });
  await page
    .getByText("Paseo session (2)", { exact: true })
    .waitFor({ timeout: 15000 });
  const details = page.locator(".session-detail");
  await waitFor(
    async () => (await details.count()) === 2,
    15000,
    "two session details",
  );
  const text = (await page.locator("body").textContent()) || "";
  for (const expected of [
    "Attempt 1",
    "Attempt 2",
    "generation 1",
    "generation 2",
    "No final session summary",
  ]) {
    check(`multi-Attempt detail shows ${expected}`, text.includes(expected));
  }
  check(
    "each Attempt keeps exactly one binding outcome",
    (await details.locator(".session-card-head .pill.failed").count()) === 1 &&
      (await details.locator(".session-card-head .pill.succeeded").count()) ===
        1,
  );
  check(
    "only the successful Attempt has final evidence",
    (await page
      .getByText("Session evidence (final summary)", { exact: true })
      .count()) === 1,
  );
  const ledger = (await api(`/api/v1/runs/${MULTI_RUN_ID}/events`)).events;
  const eventItems = page.locator(".events li");
  await waitFor(
    async () => (await eventItems.count()) === ledger.length,
    15000,
    "multi ledger event count",
  );
  await assertTimeline(
    ledger,
    "multi-Attempt timeline equals the durable ledger in order",
  );
  await shot("multi-attempt");
  return { multiRunId: MULTI_RUN_ID, eventCount: ledger.length };
}

async function livePhase() {
  const runId = process.env.S3C_LIVE_RUN_ID || "";
  const sessionId = process.env.S3C_LIVE_SESSION_ID || "";
  check("live run id provided", Boolean(runId));
  check("live session id provided", Boolean(sessionId));

  await openAgents();
  await page.getByLabel("Filter sessions by status").selectOption("active");
  const card = page.locator(".session-card").first();
  await card.waitFor({ timeout: 20000 });
  await card
    .locator(".session-card-head .pill.active")
    .waitFor({ timeout: 15000 });
  const cardText = (await card.textContent()) || "";
  for (const expected of [
    "codex",
    "gpt-6-luna",
    "full-access",
    "TRUSTED_LOCAL",
    "active",
    "running",
    "local-1",
    runId.slice(0, 8),
    sessionId.slice(0, 18),
  ]) {
    check(`live Agents card shows ${expected}`, cardText.includes(expected));
  }
  await shot("live-active-session");

  await card
    .getByRole("link", { name: runId.slice(0, 8), exact: true })
    .click();
  await page
    .getByRole("heading", { name: new RegExp(`Run ${runId.slice(0, 8)}`) })
    .waitFor({ timeout: 15000 });
  await page
    .getByText("Paseo session (1)", { exact: true })
    .waitFor({ timeout: 15000 });
  await page
    .getByText("Session bound and non-terminal", { exact: false })
    .waitFor({ timeout: 15000 });
  const body = (await page.locator("body").textContent()) || "";
  for (const expected of [
    "active",
    "Not recorded in final evidence",
    "Paseo session intended",
    "Paseo session active",
  ]) {
    check(`live Run detail shows ${expected}`, body.includes(expected));
  }
  check(
    "live Run detail has no fabricated final summary",
    !body.includes("Session evidence (final summary)"),
  );
  check(
    "live Run detail has no fabricated success",
    !body.includes("Paseo session succeeded"),
  );
  check(
    "live Run detail has no fabricated cleanup confirmation",
    !body.includes("Confirmed by the Worker"),
  );
  await shot("live-active-run");

  const ledger = (await api(`/api/v1/runs/${runId}/events`)).events;
  const eventItems = page.locator(".events li");
  await waitFor(
    async () => (await eventItems.count()) === ledger.length,
    15000,
    "live ledger event count",
  );
  await assertTimeline(
    ledger,
    "live lifecycle timeline equals the durable ledger in order",
  );

  return {
    liveRunId: runId,
    liveSessionId: sessionId,
    eventCount: ledger.length,
  };
}

async function orphanPhase() {
  check("orphan run id provided", Boolean(ORPHAN_RUN_ID));
  check("orphan session id provided", Boolean(ORPHAN_SESSION_ID));
  await openAgents();
  await page.locator(".session-card").first().waitFor({ timeout: 15000 });
  check(
    "history survived the Core restart",
    (await page.locator(".session-card").count()) === 4,
  );

  await page.getByLabel("Filter sessions by status").selectOption("orphaned");
  const card = page.locator(".session-card").first();
  await card.waitFor({ timeout: 15000 });
  await card.locator(".pill.orphaned").waitFor({ timeout: 15000 });
  const cardText = (await card.textContent()) || "";
  for (const expected of [
    "orphaned",
    "cancelled",
    "External state unconfirmed",
    ORPHAN_SESSION_ID.slice(0, 18),
    ORPHAN_RUN_ID.slice(0, 8),
    "TRUSTED_LOCAL",
  ]) {
    check(`orphan card shows ${expected}`, cardText.includes(expected));
  }
  await shot("orphan-session");

  await card
    .getByRole("link", {
      name: ORPHAN_RUN_ID.slice(0, 8),
      exact: true,
    })
    .click();
  await page
    .getByRole("heading", {
      name: new RegExp(`Run ${ORPHAN_RUN_ID.slice(0, 8)}`),
    })
    .waitFor({ timeout: 15000 });
  await page
    .getByText("Paseo session (1)", { exact: true })
    .waitFor({ timeout: 15000 });
  const pageText = (await page.locator("body").textContent()) || "";
  for (const expected of [
    "cancelled",
    "orphaned",
    "External state unconfirmed",
    "Core cancellation or a superseded Attempt never claims the external session stopped",
    "No cleanup or stop confirmation is recorded",
    "Paseo session intended",
    "Paseo session active",
    "Paseo session orphaned",
    "Run cancelled",
  ]) {
    check(`orphan Run detail shows ${expected}`, pageText.includes(expected));
  }
  check(
    "Core cancellation never records a session stop",
    !pageText.includes("PaseoSessionCancelled"),
  );

  const ledger = (await api(`/api/v1/runs/${ORPHAN_RUN_ID}/events`)).events;
  const eventItems = page.locator(".events li");
  await waitFor(
    async () => (await eventItems.count()) === ledger.length,
    15000,
    "orphan ledger event count",
  );
  check(
    "orphan lifecycle timeline matches the durable ledger",
    (await eventItems.count()) === ledger.length,
  );
  await shot("orphan-run-detail");

  return {
    orphanRunId: ORPHAN_RUN_ID,
    eventCount: ledger.length,
  };
}

try {
  app = await launchBrowser(EXE, { timeoutMs: 60000 });
  page = app.page;
  const result =
    PHASE === "orphan"
      ? await orphanPhase()
      : PHASE === "live"
        ? await livePhase()
        : PHASE === "multi"
          ? await multiPhase()
          : await successPhase();
  console.log(
    JSON.stringify({
      ok: true,
      phase: PHASE,
      ...result,
      checks: checks.length,
      evidence: EVIDENCE_DIR,
    }),
  );
} catch (e) {
  await shot("failure").catch(() => {});
  fail("journey", e);
} finally {
  if (app && (await app.close().catch(() => true))) {
    fail("browser-close", "browser shutdown timed out; SIGKILL used");
  }
}
