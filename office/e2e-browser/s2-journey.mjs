// S2 real-browser journey (playwright-core + Chromium).
// Unlock -> Runs -> queue synthetic Run -> succeeded -> Run detail
// (Attempts + event lineage) -> Workers (online). All state must come
// from the live Core; the UI renders snapshots, never fixtures.
//
// Env: BASE_URL, FACTORY_API_TOKEN, CHROMIUM_EXE (path to Chromium).
// Evidence: .s2-evidence/*.png (gitignored).
import { existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { launchBrowser, withTimeout } from "./harness.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const BASE = process.env.BASE_URL || "http://127.0.0.1:8471";
const TOKEN = process.env.FACTORY_API_TOKEN;
const EXE = process.env.CHROMIUM_EXE;
const EVIDENCE_DIR = process.env.S2_EVIDENCE_DIR || join(HERE, ".s2-evidence");

if (!TOKEN) {
  console.log(
    JSON.stringify({ ok: false, error: "FACTORY_API_TOKEN is required" }),
  );
  process.exit(1);
}
if (!EXE || !existsSync(EXE)) {
  console.log(
    JSON.stringify({
      ok: false,
      error: "CHROMIUM_EXE must point to a Chromium binary",
    }),
  );
  process.exit(1);
}
mkdirSync(EVIDENCE_DIR, { recursive: true });

const fail = (step, error) => {
  console.log(
    JSON.stringify({ ok: false, step, error: String(error?.message ?? error) }),
  );
  process.exit(1);
};

let app;
try {
  app = await launchBrowser(EXE, { timeoutMs: 60000 });
} catch (e) {
  fail("launch", e);
}
const { page, close } = app;
const shot = (n) =>
  page.screenshot({ path: join(EVIDENCE_DIR, `s2-${n}.png`), timeout: 15000 });

try {
  await withTimeout(
    page.goto(BASE, { waitUntil: "networkidle" }),
    30000,
    "goto timeout",
  );
  await withTimeout(
    page.getByPlaceholder("FACTORY_API_TOKEN").waitFor({ timeout: 10000 }),
    15000,
    "token gate timeout",
  );
  await shot("gate");
  await page.getByPlaceholder("FACTORY_API_TOKEN").fill(TOKEN);
  await page.getByRole("button", { name: "Unlock" }).click();
  await withTimeout(
    page
      .getByRole("heading", { name: "Dashboard", exact: true })
      .waitFor({ timeout: 10000 }),
    15000,
    "dashboard timeout",
  );
  await shot("dashboard");

  // Workers view: the registered local Worker must be online.
  await page.getByRole("link", { name: "Workers", exact: true }).click();
  await withTimeout(
    page
      .getByRole("heading", { name: "Workers", exact: true })
      .waitFor({ timeout: 10000 }),
    15000,
    "workers view timeout",
  );
  await withTimeout(
    page.getByText("local-1").first().waitFor({ timeout: 15000 }),
    20000,
    "worker row timeout",
  );
  await shot("workers");

  // Runs view: queue a synthetic Run through the product UI.
  await page.getByRole("link", { name: "Runs", exact: true }).click();
  await withTimeout(
    page
      .getByRole("heading", { name: "Runs", exact: true })
      .waitFor({ timeout: 10000 }),
    15000,
    "runs view timeout",
  );
  await page
    .getByPlaceholder("short marker echoed by the fixture")
    .fill("browser-proof");
  await page.getByRole("button", { name: "Queue Run" }).click();
  await withTimeout(
    page
      .getByText(/Queued Run/)
      .first()
      .waitFor({ timeout: 15000 }),
    20000,
    "queue confirmation timeout",
  );
  await shot("runs-queued");

  // The real Worker executes it: wait for a succeeded pill in the table.
  await page.getByRole("link", { name: "Runs", exact: true }).click();
  await withTimeout(
    (async () => {
      for (let i = 0; i < 30; i++) {
        const pills = await page.locator("table span.pill.succeeded").all();
        if (pills.length > 0) return;
        await new Promise((r) => setTimeout(r, 2000));
      }
      throw new Error("run did not succeed within 60s");
    })(),
    90000,
    "run completion timeout",
  );
  await shot("runs-succeeded");

  // Run detail: Attempts and the full event lineage render from the ledger.
  // The list is newest-first, so the just-queued run is the first body row.
  await page.locator("table tbody tr").first().locator("a").first().click();
  await withTimeout(
    page.getByRole("heading", { name: /Run / }).waitFor({ timeout: 10000 }),
    15000,
    "run detail timeout",
  );
  for (const label of [
    /Attempts \(\d+\)/,
    "Event timeline",
    "RunQueued",
    "RunLeased",
    "RunStarted",
    "RunCompleted",
  ]) {
    await withTimeout(
      page.getByText(label).first().waitFor({ timeout: 10000 }),
      15000,
      `detail content timeout: ${label}`,
    );
  }
  await shot("run-detail");

  console.log(JSON.stringify({ ok: true, evidence: EVIDENCE_DIR }));
} catch (e) {
  await shot("failure").catch(() => {});
  fail("journey", e);
} finally {
  // Bounded shutdown with SIGKILL fallback; a close timeout fails the run.
  if (await close().catch(() => true)) {
    fail("browser-close", "browser shutdown timed out; SIGKILL used");
  }
}
