// S1 real-browser journey (playwright-core + Chromium).
// PHASE=1: gate -> unlock -> dashboard -> create -> detail -> start.
// PHASE=2 (after Core restart on the same DB): fresh browser replays the
// full ledger from the Dashboard stream and the persisted Action detail.
//
// Env: BASE_URL, FACTORY_API_TOKEN, CHROMIUM_EXE (path to a Chromium binary),
// PHASE=1|2. Evidence: .journey-evidence/*.png (gitignored).
import { chromium } from "playwright-core";
import { writeFileSync, readFileSync, mkdirSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const BASE = process.env.BASE_URL || "http://127.0.0.1:8471";
const TOKEN = process.env.FACTORY_API_TOKEN;
const PHASE = process.env.PHASE || "1";
const EXE = process.env.CHROMIUM_EXE;
if (!TOKEN) {
  console.log(JSON.stringify({ phase: PHASE, ok: false, error: "FACTORY_API_TOKEN is required" }));
  process.exit(1);
}
if (!EXE || !existsSync(EXE)) {
  console.log(
    JSON.stringify({ phase: PHASE, ok: false, error: "CHROMIUM_EXE must point to a Chromium binary" }),
  );
  process.exit(1);
}
mkdirSync(join(HERE, ".journey-evidence"), { recursive: true });

const browser = await chromium.launch({ executablePath: EXE });
const page = await browser.newPage();
const notes = [];
const shot = (n) =>
  page.screenshot({ path: join(HERE, ".journey-evidence", `phase${PHASE}-${n}.png`), timeout: 15000 });

try {
  if (PHASE === "1") {
    await page.goto(BASE, { waitUntil: "networkidle" });
    await page.getByPlaceholder("FACTORY_API_TOKEN").waitFor({ timeout: 10000 });
    notes.push("token gate rendered");
    await shot("gate");
    await page.getByPlaceholder("FACTORY_API_TOKEN").fill(TOKEN);
    await page.getByRole("button", { name: "Unlock" }).click();
    await page.getByRole("heading", { name: "Dashboard" }).waitFor({ timeout: 10000 });
    notes.push("unlock ok, dashboard rendered");
    await shot("dashboard");

    await page.getByRole("link", { name: "Actions", exact: true }).click();
    await page.getByRole("heading", { name: "New Action" }).waitFor();
    await page.getByPlaceholder("What should the Factory do?").fill("Browser journey action");
    await page
      .getByPlaceholder("Acceptance sketch, context, constraints…")
      .fill("created through the real UI");
    await page.getByRole("button", { name: "Create Action" }).click();
    await page.getByText("Browser journey action").first().waitFor({ timeout: 10000 });
    notes.push("action created through UI and listed");
    await shot("actions");

    await page.getByRole("link", { name: "Browser journey action" }).click();
    await page.getByText("Browser journey action").first().waitFor();
    const detailUrl = page.url();
    const id = detailUrl.split("/").pop();
    writeFileSync(join(HERE, ".journey-action.json"), JSON.stringify({ id, url: detailUrl }));
    await page.getByRole("button", { name: "Start" }).click();
    await page.getByText("in_progress").first().waitFor({ timeout: 10000 });
    notes.push(`detail ok, transitioned to in_progress (id=${id})`);
    await shot("detail");
  } else {
    const { id } = JSON.parse(readFileSync(join(HERE, ".journey-action.json"), "utf8"));
    await page.goto(BASE, { waitUntil: "networkidle" });
    await page.getByPlaceholder("FACTORY_API_TOKEN").fill(TOKEN);
    await page.getByRole("button", { name: "Unlock" }).click();
    await page.getByRole("heading", { name: "Dashboard" }).waitFor({ timeout: 10000 });
    await page.goto(`${BASE}/#/`, { waitUntil: "networkidle" });
    await page.getByText("CoreStarted").first().waitFor({ timeout: 15000 });
    await page.getByText("ActionCreated").first().waitFor({ timeout: 15000 });
    await page.getByText("ActionTransitioned").first().waitFor({ timeout: 15000 });
    notes.push("dashboard stream replays full ledger incl. CoreStarted");
    await page.goto(`${BASE}/#/actions/${id}`, { waitUntil: "networkidle" });
    await page.getByText("Browser journey action").first().waitFor({ timeout: 10000 });
    await page.getByText("in_progress").first().waitFor({ timeout: 10000 });
    const body = await page.content();
    for (const needle of ["ActionCreated", "ActionTransitioned"]) {
      if (!body.includes(needle)) throw new Error(`missing ledger evidence: ${needle}`);
    }
    notes.push(`post-restart: action ${id} + its ledger visible in fresh browser`);
    await shot("post-restart");
  }
  console.log(JSON.stringify({ phase: PHASE, ok: true, notes }));
} catch (e) {
  await shot("failure");
  console.log(JSON.stringify({ phase: PHASE, ok: false, error: String(e).slice(0, 500) }));
  process.exitCode = 1;
} finally {
  // Bounded shutdown: a frozen renderer stalls browser.close() exactly
  // like it stalls the page, so cap it and SIGKILL to guarantee exit.
  await Promise.race([
    browser.close(),
    new Promise((_, reject) =>
      setTimeout(() => reject(new Error("browser-close timeout")), 20000),
    ),
  ]).catch(() => {
    try {
      browser.process()?.kill("SIGKILL");
    } catch {
      // Already dead; nothing to kill.
    }
  });
}
