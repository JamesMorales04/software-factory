// S1 long-running soak: keeps Factory Office open for >=95s across many
// server heartbeats (~20s apart) plus background/foreground and mount
// cycles, and fails on ANY main-thread stall, page error, duplicate or
// missing event, or connection accumulation.
//
// Env: BASE_URL, FACTORY_API_TOKEN, CHROMIUM_EXE.
// Evidence: .soak-evidence/*.png + JSON summary on stdout (exit nonzero on failure).
import { chromium } from "playwright-core";
import { execSync } from "node:child_process";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "url";

const HERE = dirname(fileURLToPath(import.meta.url));
const BASE = process.env.BASE_URL;
const TOKEN = process.env.FACTORY_API_TOKEN;
const EXE = process.env.CHROMIUM_EXE;
const PORT = new URL(BASE).port;
if (!TOKEN || !EXE) {
  console.log(JSON.stringify({ ok: false, error: "FACTORY_API_TOKEN and CHROMIUM_EXE required" }));
  process.exit(1);
}
mkdirSync(join(HERE, ".soak-evidence"), { recursive: true });

const startedAt = Date.now();
const elapsed = () => Math.round((Date.now() - startedAt) / 1000);
const failures = [];
const note = (m) => console.log(`[t+${elapsed()}s] ${m}`);
const check = (name, cond, detail = "") => {
  console.log(`[${cond ? "PASS" : "FAIL"}] ${name}${cond ? "" : " — " + detail}`);
  if (!cond) failures.push(name);
};

function chromeEstablishedStreams() {
  // Count only Chromium-owned connections: the harness's own Node fetch
  // pool and generic idle keep-alive are not leaks. A leaked SSE stream
  // from an unmounted hook would show up here as growth.
  try {
    const out = execSync(`ss -tnp state established '( dport = :${PORT} )' 2>/dev/null`, {
      encoding: "utf8",
    });
    return out
      .split("\n")
      .filter((line) => /chrom/i.test(line))
      .length;
  } catch {
    return -1;
  }
}

async function api(path, opts = {}) {
  const res = await fetch(`${BASE}${path}`, {
    headers: { Authorization: `Bearer ${TOKEN}`, "Content-Type": "application/json" },
    ...opts,
  });
  if (!res.ok) throw new Error(`API ${opts.method || "GET"} ${path}: ${res.status}`);
  return res.json();
}

const browser = await chromium.launch({ executablePath: EXE });
const page = await browser.newPage();
page.on("pageerror", (e) => failures.push(`pageerror: ${String(e).slice(0, 200)}`));
const shot = (n) =>
  page.screenshot({ path: join(HERE, ".soak-evidence", `soak-${n}.png`), timeout: 15000 });

async function responsive(label) {
  try {
    await page.evaluate(() => Date.now(), { timeout: 8000 });
    return true;
  } catch {
    check(`responsive (${label})`, false, "main thread stalled >8s");
    return false;
  }
}

function ledgerIsClean(events) {
  const seqs = events.map((e) => e.globalSeq);
  const unique = new Set(seqs);
  return (
    unique.size === seqs.length && seqs.every((s, i) => i === 0 || s === seqs[i - 1] + 1)
  );
}

try {
  await page.goto(BASE, { waitUntil: "networkidle" });
  await page.getByPlaceholder("FACTORY_API_TOKEN").fill(TOKEN);
  await page.getByRole("button", { name: "Unlock" }).click();
  await page.getByRole("heading", { name: "Dashboard" }).waitFor({ timeout: 15000 });
  check("stream live on dashboard", await page.getByText("live").first().isVisible());
  await shot("start");

  // Phase A: stay on the Dashboard with the stream mounted. Create through
  // the API so the live push (not a poll) must deliver it mid-heartbeats.
  const created = await api("/api/v1/actions", {
    method: "POST",
    body: JSON.stringify({ title: "Soak action one", description: "live-push probe" }),
  });
  note(`action one created via API (${created.action.id.slice(0, 8)})`);
  await page.getByText("ActionCreated").first().waitFor({ timeout: 15000 });
  check("live push delivered action one", true);

  while (elapsed() < 40) {
    if (!(await responsive("phase-A heartbeat ride"))) break;
    await page.waitForTimeout(5000);
  }
  check("survived heartbeats to t+40s with mounted stream", elapsed() >= 40);

  // Phase B: mount/unmount cycles (navigation tears down and rebuilds streams).
  // Measure on the Dashboard in both cases: same state, so any growth is a leak.
  await page.waitForTimeout(3000);
  const connsBeforeCycles = chromeEstablishedStreams();
  for (let i = 0; i < 3; i++) {
    await page.getByRole("link", { name: "Actions", exact: true }).click();
    await page.getByRole("heading", { name: "New Action" }).waitFor();
    await page.getByRole("link", { name: "Dashboard", exact: true }).click();
    await page.getByRole("heading", { name: "Dashboard" }).waitFor();
    if (!(await responsive(`mount-cycle-${i}`))) break;
  }
  await page.getByRole("link", { name: "Actions", exact: true }).click();
  await page.getByRole("heading", { name: "New Action" }).waitFor();
  await page.getByPlaceholder("What should the Factory do?").fill("Soak action two");
  await page.getByRole("button", { name: "Create Action" }).click();
  await page.getByText("Soak action two").first().waitFor({ timeout: 10000 });
  note("action two created via UI after mount cycles");
  await page.getByRole("link", { name: "Dashboard", exact: true }).click();
  await page.getByRole("heading", { name: "Dashboard" }).waitFor();

  // Phase C: background the tab across heartbeats (stream stays mounted), then return.
  const blank = await browser.newPage();
  await blank.goto("about:blank");
  await blank.bringToFront();
  note("app tab backgrounded");
  await page.waitForTimeout(25000);
  await page.bringToFront();
  await blank.close();
  check("responsive after background/foreground", await responsive("post-background"));
  await page.getByRole("link", { name: "Dashboard", exact: true }).click();
  await page.getByRole("heading", { name: "Dashboard" }).waitFor();
  check("stream live after foreground", await page.getByText("live").first().isVisible());

  // Ride to >=95s total with probes.
  while (elapsed() < 95) {
    if (!(await responsive("final ride"))) break;
    await page.waitForTimeout(5000);
  }
  check("soak lasted >=95s", elapsed() >= 95, `elapsed=${elapsed()}s`);
  await shot("end");

  // Ledger integrity from the server side (exactly-once, no gaps).
  const { events } = await api("/api/v1/events?after=0&limit=500");
  check("ledger contiguous and duplicate-free", ledgerIsClean(events), `n=${events.length}`);
  const titles = events.map((e) => e.payload && e.payload.title).filter(Boolean);
  check(
    "both soak actions recorded exactly once",
    titles.filter((t) => t === "Soak action one").length === 1 &&
      titles.filter((t) => t === "Soak action two").length === 1,
    titles.join(","),
  );

  // Connection accumulation: an un-aborted stream per unmounted hook would
  // grow Chromium-owned connections by one per cycle above. Settle back on
  // the Dashboard (one live stream) and require no growth vs pre-cycles.
  await page.getByRole("link", { name: "Dashboard", exact: true }).click();
  await page.getByRole("heading", { name: "Dashboard" }).waitFor();
  await page.waitForTimeout(3000);
  const connsAfterCycles = chromeEstablishedStreams();
  check(
    "no connection accumulation across mount cycles",
    connsAfterCycles >= 0 && connsAfterCycles <= Math.max(1, connsBeforeCycles),
    `chromium established before=${connsBeforeCycles} after=${connsAfterCycles}`,
  );

  const summary = { ok: failures.length === 0, elapsedSec: elapsed(), failures };
  writeFileSync(join(HERE, ".soak-evidence", "summary.json"), JSON.stringify(summary, null, 2));
  console.log(JSON.stringify(summary));
  if (failures.length > 0) process.exitCode = 1;
} catch (e) {
  await shot("failure").catch(() => undefined);
  console.log(JSON.stringify({ ok: false, error: String(e).slice(0, 500), elapsedSec: elapsed() }));
  process.exitCode = 1;
} finally {
  await browser.close().catch(() => undefined);
}
