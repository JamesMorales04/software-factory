// S1 real-browser journey (playwright-core + Chromium).
// PHASE=1: gate -> unlock -> dashboard -> create -> detail -> start.
// PHASE=2 (after Core restart on the same DB): fresh browser replays the
// full ledger from the Dashboard stream and the persisted Action detail.
//
// Env: BASE_URL, FACTORY_API_TOKEN, CHROMIUM_EXE (path to a Chromium binary),
// PHASE=1|2. Evidence: .journey-evidence/*.png (gitignored).
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { launchBrowser, withTimeout } from "./harness.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const BASE = process.env.BASE_URL || "http://127.0.0.1:8471";
const TOKEN = process.env.FACTORY_API_TOKEN;
const PHASE = process.env.PHASE || "1";
const EXE = process.env.CHROMIUM_EXE;
const EVIDENCE_DIR =
  process.env.JOURNEY_EVIDENCE_DIR || join(HERE, ".journey-evidence");
const ACTION_FILE =
  process.env.JOURNEY_ACTION_FILE || join(HERE, ".journey-action.json");
if (!TOKEN) {
  console.log(
    JSON.stringify({
      phase: PHASE,
      ok: false,
      error: "FACTORY_API_TOKEN is required",
    }),
  );
  process.exit(1);
}
if (!EXE || !existsSync(EXE)) {
  console.log(
    JSON.stringify({
      phase: PHASE,
      ok: false,
      error: "CHROMIUM_EXE must point to a Chromium binary",
    }),
  );
  process.exit(1);
}
mkdirSync(EVIDENCE_DIR, { recursive: true });

let app;
try {
  // The harness owns the browser process from spawn, so a hung launch
  // binary is SIGKILLed on timeout instead of orphaned.
  app = await launchBrowser(EXE, { timeoutMs: 60000 });
} catch (e) {
  console.log(
    JSON.stringify({
      phase: PHASE,
      ok: false,
      error: `browser launch failed: ${e.message}`,
    }),
  );
  process.exit(1);
}
const { page, close } = app;
const notes = [];
const shot = (n) =>
  page.screenshot({
    path: join(EVIDENCE_DIR, `phase${PHASE}-${n}.png`),
    timeout: 15000,
  });

let journeyResult;
try {
  if (PHASE === "1") {
    await page.goto(BASE, { waitUntil: "networkidle" });
    await page
      .getByPlaceholder("FACTORY_API_TOKEN")
      .waitFor({ timeout: 10000 });
    notes.push("token gate rendered");
    await shot("gate");
    await page.getByPlaceholder("FACTORY_API_TOKEN").fill(TOKEN);
    await page.getByRole("button", { name: "Unlock" }).click();
    await page
      .getByRole("heading", { name: "Dashboard" })
      .waitFor({ timeout: 10000 });
    notes.push("unlock ok, dashboard rendered");
    await shot("dashboard");

    await page.getByRole("link", { name: "Actions", exact: true }).click();
    await page.getByRole("heading", { name: "New Action" }).waitFor();
    await page
      .getByPlaceholder("What should the Factory do?")
      .fill("Browser journey action");
    await page
      .getByPlaceholder("Acceptance sketch, context, constraints…")
      .fill("created through the real UI");
    await page.getByRole("button", { name: "Create Action" }).click();
    await page
      .getByText("Browser journey action")
      .first()
      .waitFor({ timeout: 10000 });
    notes.push("action created through UI and listed");
    await shot("actions");

    await page.getByRole("link", { name: "Browser journey action" }).click();
    await page.getByText("Browser journey action").first().waitFor();
    const detailUrl = page.url();
    const id = detailUrl.split("/").pop();
    writeFileSync(ACTION_FILE, JSON.stringify({ id, url: detailUrl }));
    await page.getByRole("button", { name: "Start" }).click();
    await page.getByText("in_progress").first().waitFor({ timeout: 10000 });
    notes.push(`detail ok, transitioned to in_progress (id=${id})`);
    await shot("detail");
  } else {
    const { id } = JSON.parse(readFileSync(ACTION_FILE, "utf8"));
    await page.goto(BASE, { waitUntil: "networkidle" });
    await page.getByPlaceholder("FACTORY_API_TOKEN").fill(TOKEN);
    await page.getByRole("button", { name: "Unlock" }).click();
    await page
      .getByRole("heading", { name: "Dashboard" })
      .waitFor({ timeout: 10000 });
    await page.goto(`${BASE}/#/`, { waitUntil: "networkidle" });
    await page.getByText("CoreStarted").first().waitFor({ timeout: 15000 });
    await page.getByText("ActionCreated").first().waitFor({ timeout: 15000 });
    await page
      .getByText("ActionTransitioned")
      .first()
      .waitFor({ timeout: 15000 });
    notes.push("dashboard stream replays full ledger incl. CoreStarted");
    await page.goto(`${BASE}/#/actions/${id}`, { waitUntil: "networkidle" });
    await page
      .getByText("Browser journey action")
      .first()
      .waitFor({ timeout: 10000 });
    await page.getByText("in_progress").first().waitFor({ timeout: 10000 });
    const body = await withTimeout(
      page.content(),
      15000,
      "page content timeout",
    );
    for (const needle of ["ActionCreated", "ActionTransitioned"]) {
      if (!body.includes(needle))
        throw new Error(`missing ledger evidence: ${needle}`);
    }
    notes.push(
      `post-restart: action ${id} + its ledger visible in fresh browser`,
    );
    await shot("post-restart");
  }
  journeyResult = { phase: PHASE, ok: true, notes };
} catch (e) {
  await shot("failure");
  journeyResult = {
    phase: PHASE,
    ok: false,
    error: String(e).slice(0, 500),
  };
} finally {
  // Bounded shutdown with SIGKILL fallback (see harness.mjs); a close
  // timeout is recorded as a failure so the verdict reflects it.
  if (await close()) {
    journeyResult = {
      phase: journeyResult?.phase ?? PHASE,
      ok: false,
      error: "browser shutdown timed out; SIGKILL used",
    };
  }
  console.log(JSON.stringify(journeyResult));
  if (!journeyResult.ok) process.exitCode = 1;
}
