// Real Chromium acceptance for the delivery policy editor and Campaign delivery
// panel. Runs against an already running Office/Core candidate with a seeded
// Project and Campaign; it never invents Core state or reads a token file.
// This is implementer self-test evidence, never independent acceptance.
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { chromium } from "playwright-core";

const base = process.env.FACTORY_OFFICE_URL || "http://127.0.0.1:8893";
const projectId = process.env.FACTORY_DELIVERY_PROJECT_ID;
const campaignId = process.env.FACTORY_DELIVERY_CAMPAIGN_ID;
const enrollmentCode = process.env.FACTORY_BOOTSTRAP_CODE;
const executablePath =
  process.env.CHROMIUM_EXE || findChromium();

if (!projectId || !campaignId) {
  throw new Error(
    "FACTORY_DELIVERY_PROJECT_ID and FACTORY_DELIVERY_CAMPAIGN_ID are required",
  );
}

function findChromium() {
  const root = join(homedir(), ".cache/ms-playwright");
  if (!existsSync(root)) return undefined;
  const candidates = [
    "chrome-linux/chrome",
    "chrome-linux64/chrome",
    "chrome-linux/headless_shell",
  ];
  for (const name of candidates) {
    const path = join(root, name);
    if (existsSync(path)) return path;
  }
  return undefined;
}

const errors = [];
const browser = await chromium.launch({
  executablePath,
  headless: true,
  args: ["--no-sandbox", "--disable-dev-shm-usage"],
});
const context = await browser.newContext({
  viewport: { width: 1440, height: 1000 },
});
const page = await context.newPage();
page.on("pageerror", (error) => errors.push(String(error)));
page.on("console", (message) => {
  if (message.type() === "error") errors.push(message.text());
});

async function signIn() {
  await page.goto(base, { waitUntil: "domcontentloaded" });
  const heading = page.getByRole("heading", {
    name: /Set up this Factory|Recover local access/,
  });
  if (await heading.isVisible().catch(() => false)) {
    if (!enrollmentCode) {
      throw new Error("FACTORY_BOOTSTRAP_CODE is required for first-run setup");
    }
    await page
      .getByRole("textbox", { name: "One-time owner confirmation" })
      .fill(enrollmentCode);
    await page
      .getByRole("textbox", { name: "This device name" })
      .fill("Delivery policy E2E");
    await page.getByRole("button", { name: "Enroll this device" }).click();
  }
  await page.getByRole("heading", { name: "Home", level: 1 }).waitFor();
}

async function assertNoOverflow(label) {
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth > window.innerWidth,
  );
  assert.equal(overflow, false, `${label} must not overflow horizontally`);
}

try {
  await signIn();

  await page.goto(`${base}/#/projects/${projectId}`, {
    waitUntil: "domcontentloaded",
  });
  const editor = page.getByTestId("delivery-policy-editor");
  await editor.waitFor();
  await editor.getByRole("heading", { name: "Delivery policy" }).waitFor();
  const toggles = editor.getByRole("checkbox");
  assert.ok(
    (await toggles.count()) >= 3,
    "policy editor exposes risk-tiered, R1 opt-in and kill-switch controls",
  );

  for (const width of [1440, 834, 320]) {
    await page.setViewportSize({ width, height: 1000 });
    await editor.scrollIntoViewIfNeeded();
    await assertNoOverflow(`policy editor at ${width}px`);
  }

  await page.goto(`${base}/#/campaigns/${campaignId}`, {
    waitUntil: "domcontentloaded",
  });
  const panel = page.getByTestId("delivery-panel");
  await panel.waitFor();
  await panel.getByRole("heading", { name: "Delivery" }).waitFor();
  for (const width of [1440, 834, 320]) {
    await page.setViewportSize({ width, height: 1000 });
    await panel.scrollIntoViewIfNeeded();
    await assertNoOverflow(`delivery panel at ${width}px`);
  }

  await page.evaluate((key) => {
    window.localStorage.setItem(key, "es");
  }, "factory-office-locale");
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.getByTestId("delivery-panel").waitFor();
  await page
    .getByTestId("delivery-panel")
    .getByRole("heading", { name: "Entrega" })
    .waitFor();
  await assertNoOverflow("delivery panel at 320px in Spanish");
} finally {
  await browser.close();
}

assert.deepEqual(errors, [], "browser console and page errors must be empty");
console.log(
  JSON.stringify(
    {
      projectId,
      campaignId,
      widths: [1440, 834, 320],
      locales: ["en", "es"],
      errors,
    },
    null,
    2,
  ),
);
