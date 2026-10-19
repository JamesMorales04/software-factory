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
import { closeBrowser, withTimeout } from "./harness.mjs";

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
const RUN = startedAt.toString(36);
// Unique titles per run so the exactly-once ledger check stays valid when
// the same scratch DB is reused across runs.
const TITLE_ONE = `Soak action one ${RUN}`;
const TITLE_TWO = `Soak action two ${RUN}`;
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
  // from an unmounted hook would show up here as steady growth.
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

async function settledSocketCount() {
  // Single samples of the idle pool are noisy (handshake transients,
  // lingering keep-alive); the median of three spaced samples is stable.
  const samples = [];
  for (let i = 0; i < 3; i++) {
    samples.push(chromeEstablishedStreams());
    await page.waitForTimeout(2000);
  }
  samples.sort((a, b) => a - b);
  return samples[1];
}

async function api(path, opts = {}) {
  // Bounded by default; a caller signal is composed with (not replaced
  // by) the timeout, so the override path can never go unbounded.
  const timeout = AbortSignal.timeout(15000);
  const signal = opts.signal ? AbortSignal.any([opts.signal, timeout]) : timeout;
  const init = { ...opts, signal };
  const res = await fetch(`${BASE}${path}`, {
    headers: { Authorization: `Bearer ${TOKEN}`, "Content-Type": "application/json" },
    ...init,
  });
  if (!res.ok) throw new Error(`API ${opts.method || "GET"} ${path}: ${res.status}`);
  return res.json();
}

let browser;
{
  // Launch failures must neither hang nor leak: bound the wait, and if a
  // late launch resolves after the timeout, close it instead of orphaning it.
  const pendingLaunch = chromium.launch({ executablePath: EXE });
  try {
    browser = await withTimeout(pendingLaunch, 60000, "browser launch timeout");
  } catch (e) {
    pendingLaunch.then(
      (late) => late.close().catch(() => {}),
      () => {},
    );
    console.log(JSON.stringify({ ok: false, error: `browser launch failed: ${e.message}` }));
    process.exit(1);
  }
}
const page = await withTimeout(browser.newPage(), 30000, "new page timeout").catch(
  async (e) => {
    // newPage failed after a successful launch: close the browser so it
    // cannot leak, then fail nonzero.
    await closeBrowser(browser);
    console.log(JSON.stringify({ ok: false, error: `new page failed: ${e.message}` }));
    process.exit(1);
  },
);
page.on("pageerror", (e) => failures.push(`pageerror: ${String(e).slice(0, 200)}`));
const shot = (n) =>
  page.screenshot({ path: join(HERE, ".soak-evidence", `soak-${n}.png`), timeout: 15000 });

async function responsive(label) {
  // NOTE: page.evaluate's second argument is the function argument, NOT a
  // timeout option — an unwrapped evaluate would hang forever on a frozen
  // renderer instead of failing. Race it against an explicit timer, and
  // clear the timer on success so probes never linger process exit.
  let timer;
  try {
    await Promise.race([
      page.evaluate(() => Date.now()),
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error("probe timeout")), 8000);
      }),
    ]);
    return true;
  } catch {
    check(`responsive (${label})`, false, "main thread stalled >8s");
    return false;
  } finally {
    clearTimeout(timer);
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
    body: JSON.stringify({ title: TITLE_ONE, description: "live-push probe" }),
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
  // Measure medians on the Dashboard in both cases: identical page state,
  // so only a real leak trends upward. Six cycles turn a +1/cycle leak
  // into an unmistakable +6 against ±1 idle-pool noise.
  const connsBeforeCycles = await settledSocketCount();
  for (let i = 0; i < 6; i++) {
    await page.getByRole("link", { name: "Actions", exact: true }).click();
    await page.getByRole("heading", { name: "New Action" }).waitFor();
    await page.getByRole("link", { name: "Dashboard", exact: true }).click();
    await page.getByRole("heading", { name: "Dashboard" }).waitFor();
    if (!(await responsive(`mount-cycle-${i}`))) break;
  }
  await page.getByRole("link", { name: "Actions", exact: true }).click();
  await page.getByRole("heading", { name: "New Action" }).waitFor();
  await page.getByPlaceholder("What should the Factory do?").fill(TITLE_TWO);
  await page.getByRole("button", { name: "Create Action" }).click();
  await page.getByText(TITLE_TWO).first().waitFor({ timeout: 10000 });
  note("action two created via UI after mount cycles");
  await page.getByRole("link", { name: "Dashboard", exact: true }).click();
  await page.getByRole("heading", { name: "Dashboard" }).waitFor();

  // Phase B2: filtered stream on Action Detail across a heartbeat.
  const listed = await api("/api/v1/actions");
  const two = listed.actions.find((a) => a.title === TITLE_TWO);
  if (!two) throw new Error("action two missing from ledger");
  await page.goto(`${BASE}/#/actions/${two.id}`, { waitUntil: "networkidle" });
  await page.getByText("Event timeline").waitFor({ timeout: 10000 });
  await page.getByText("ActionCreated").first().waitFor({ timeout: 10000 });
  note("detail timeline rendered for filtered stream");
  await page.waitForTimeout(25000);
  check("responsive on detail across heartbeat", await responsive("detail-heartbeat"));
  check(
    "detail timeline intact",
    (await page.getByText("ActionCreated").count()) >= 1,
  );
  await page.getByRole("link", { name: "Dashboard", exact: true }).click();
  await page.getByRole("heading", { name: "Dashboard" }).waitFor();

  // Phase C: background the tab across heartbeats (stream stays mounted), then return.
  const blank = await withTimeout(browser.newPage(), 30000, "background page timeout");
  await blank.goto("about:blank");
  await withTimeout(page.bringToFront(), 15000, "bringToFront timeout");
  note("app tab backgrounded");
  await page.waitForTimeout(25000);
  await withTimeout(page.bringToFront(), 15000, "bringToFront timeout");
  await withTimeout(blank.close(), 15000, "background page close timeout");
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
    titles.filter((t) => t === TITLE_ONE).length === 1 &&
      titles.filter((t) => t === TITLE_TWO).length === 1,
    titles.join(","),
  );

  // Connection accumulation: an un-aborted stream per unmounted hook would
  // grow Chromium-owned connections by one per cycle above. Compare
  // medians on the settled Dashboard; allow +1 of idle-pool noise.
  await page.getByRole("link", { name: "Dashboard", exact: true }).click();
  await page.getByRole("heading", { name: "Dashboard" }).waitFor();
  const connsAfterCycles = await settledSocketCount();
  check(
    "no connection accumulation across mount cycles",
    connsAfterCycles >= 0 && connsAfterCycles <= connsBeforeCycles + 1,
    `chromium established median before=${connsBeforeCycles} after=${connsAfterCycles}`,
  );
} catch (e) {
  await shot("failure").catch(() => undefined);
  failures.push(`harness error: ${String(e).slice(0, 200)}`);
} finally {
  // Bounded shutdown (see harness.mjs): a close timeout is a recorded
  // failure, and the summary is written only after the browser is gone,
  // so the verdict always reflects the shutdown outcome too.
  if (await closeBrowser(browser)) {
    failures.push("browser shutdown timed out; SIGKILL used");
  }
  const summary = { ok: failures.length === 0, elapsedSec: elapsed(), failures };
  writeFileSync(join(HERE, ".soak-evidence", "summary.json"), JSON.stringify(summary, null, 2));
  console.log(JSON.stringify(summary));
  if (failures.length > 0) process.exitCode = 1;
}
