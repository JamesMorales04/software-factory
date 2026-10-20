// S3-D recovery visibility journey: real Core projection + real Chromium.
// Env: BASE_URL, FACTORY_API_TOKEN, CHROMIUM_EXE, S3D_EVIDENCE_DIR,
//      S3D_CONFIRMED_RUN, S3D_CONFIRMED_SESSION,
//      S3D_BLOCKED_RUN, S3D_BLOCKED_SESSION,
//      S3D_PENDING_RUN, S3D_PENDING_SESSION.
import { existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { launchBrowser, withTimeout } from "./harness.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const BASE = process.env.BASE_URL || "http://127.0.0.1:8478";
const TOKEN = process.env.FACTORY_API_TOKEN;
const EXE = process.env.CHROMIUM_EXE;
const EVIDENCE = process.env.S3D_EVIDENCE_DIR || join(HERE, ".s3d-evidence");
const CONFIRMED_RUN = process.env.S3D_CONFIRMED_RUN || "";
const CONFIRMED_SESSION = process.env.S3D_CONFIRMED_SESSION || "";
const BLOCKED_RUN = process.env.S3D_BLOCKED_RUN || "";
const BLOCKED_SESSION = process.env.S3D_BLOCKED_SESSION || "";
const PENDING_RUN = process.env.S3D_PENDING_RUN || "";
const PENDING_SESSION = process.env.S3D_PENDING_SESSION || "";

const checks = [];
function check(name, pass, detail = "") {
  checks.push({ name, pass, ...(detail ? { detail } : {}) });
  if (!pass) throw new Error(`${name}${detail ? `: ${detail}` : ""}`);
}

async function cardFor(sessionId) {
  const code = page.locator(`code[title="${sessionId}"]`);
  await code.waitFor({ timeout: 15000 });
  return code.locator(
    "xpath=ancestor::article[contains(@class,'session-card')]",
  );
}

let app;
let page;
try {
  if (!TOKEN) throw new Error("FACTORY_API_TOKEN required");
  if (!EXE || !existsSync(EXE)) throw new Error("CHROMIUM_EXE required");
  for (const [name, value] of [
    ["S3D_CONFIRMED_RUN", CONFIRMED_RUN],
    ["S3D_CONFIRMED_SESSION", CONFIRMED_SESSION],
    ["S3D_BLOCKED_RUN", BLOCKED_RUN],
    ["S3D_BLOCKED_SESSION", BLOCKED_SESSION],
    ["S3D_PENDING_RUN", PENDING_RUN],
    ["S3D_PENDING_SESSION", PENDING_SESSION],
  ]) {
    if (!value) throw new Error(`${name} required`);
  }
  mkdirSync(EVIDENCE, { recursive: true });
  const launched = await launchBrowser(EXE, { headless: true });
  app = launched;
  page = launched.page;

  await page.goto(BASE, { waitUntil: "domcontentloaded" });
  const gate = page.getByPlaceholder("FACTORY_API_TOKEN");
  if ((await gate.count()) > 0) {
    await gate.fill(TOKEN);
    await page.getByRole("button", { name: "Unlock" }).click();
  }
  await page.getByRole("link", { name: "Agents", exact: true }).click();
  await page
    .getByRole("heading", { name: "Agents", exact: true })
    .waitFor({ timeout: 15000 });
  await page.getByLabel("Filter sessions by status").selectOption("orphaned");

  const confirmedCard = await cardFor(CONFIRMED_SESSION);
  const confirmedText = await confirmedCard.innerText();
  check(
    "confirmed recovery visible",
    confirmedText.includes("cleanup confirmed") &&
      confirmedText.includes("Cleanup confirmed"),
    confirmedText,
  );

  const blockedCard = await cardFor(BLOCKED_SESSION);
  const blockedText = await blockedCard.innerText();
  check(
    "blocked recovery visible with reason",
    blockedText.includes("cleanup blocked") && blockedText.includes("identity"),
    blockedText,
  );

  const pendingCard = await cardFor(PENDING_SESSION);
  const pendingText = await pendingCard.innerText();
  check(
    "pending recovery visible without a fabricated timestamp",
    pendingText.includes("cleanup pending") &&
      pendingText.includes("not observed yet") &&
      !pendingText.includes("0001"),
    pendingText,
  );

  await confirmedCard.locator("a").first().click();
  const confirmedDetailLocator = page
    .locator(".session-detail")
    .filter({ hasText: CONFIRMED_SESSION });
  await confirmedDetailLocator.waitFor({ timeout: 15000 });
  const confirmedDetail = await confirmedDetailLocator.innerText();
  check(
    "confirmed recovery survives Run detail",
    confirmedDetail.includes("cleanup confirmed") &&
      confirmedDetail.includes("not proof of exactly-once"),
    confirmedDetail,
  );
  await withTimeout(
    page.screenshot({
      path: join(EVIDENCE, "s3d-confirmed-run-detail.png"),
      timeout: 15000,
    }),
    20000,
    "screenshot confirmed",
  );

  await page.getByRole("link", { name: "Agents" }).click();
  await page.getByLabel("Filter sessions by status").selectOption("orphaned");
  const blockedAgain = await cardFor(BLOCKED_SESSION);
  await blockedAgain.locator("a").first().click();
  const blockedDetailLocator = page
    .locator(".session-detail")
    .filter({ hasText: BLOCKED_SESSION });
  await blockedDetailLocator.waitFor({ timeout: 15000 });
  const blockedDetail = await blockedDetailLocator.innerText();
  check(
    "blocked recovery survives Run detail",
    blockedDetail.includes("cleanup blocked") &&
      blockedDetail.includes("identity"),
    blockedDetail,
  );
  await withTimeout(
    page.screenshot({
      path: join(EVIDENCE, "s3d-blocked-run-detail.png"),
      timeout: 15000,
    }),
    20000,
    "screenshot blocked",
  );

  console.log(JSON.stringify({ ok: true, checks, evidence: EVIDENCE }));
} catch (error) {
  console.log(
    JSON.stringify({
      ok: false,
      error: String(error?.message ?? error),
      checks,
      evidence: EVIDENCE,
    }),
  );
  process.exitCode = 1;
} finally {
  if (app) await app.close();
}
