import assert from "node:assert/strict";
import { chromium } from "../office/e2e-browser/node_modules/playwright-core/index.mjs";

const base = process.env.FACTORY_OFFICE_URL || "http://127.0.0.1:8893";
const token = process.env.FACTORY_API_TOKEN;
const executablePath = process.env.CHROMIUM_EXE;
if (!token || !executablePath) throw new Error("FACTORY_API_TOKEN and CHROMIUM_EXE are required");

const browser = await chromium.launch({ executablePath, headless: true, args: ["--no-sandbox"] });
const page = await browser.newPage({ viewport: { width: 1440, height: 1100 } });
const errors = [];
page.on("console", (message) => { if (message.type() === "error") errors.push(message.text()); });
try {
  await page.goto(base, { waitUntil: "domcontentloaded" });
  if (await page.locator(".gate").isVisible().catch(() => false)) {
    await page.locator('.gate input[type="password"]').fill(token);
    await page.getByRole("button", { name: "Unlock" }).click();
  }
  await page.getByRole("link", { name: "Settings", exact: true }).click();
  const card = page.locator("section.card").filter({ hasText: "OpenCode Go" });
  await card.waitFor();
  await card.locator('input[type="password"]').fill("dummy-ui-credential-1234");
  await card.getByLabel("Enabled").check();
  await card.getByLabel("Default route for Core model calls").check();
  await card.getByRole("button", { name: "Save configuration" }).click();
  await page.getByText(/Configuration saved/).waitFor({ timeout: 15000 });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.getByRole("link", { name: "Settings", exact: true }).click();
  const reloaded = page.locator("section.card").filter({ hasText: "OpenCode Go" });
  await reloaded.getByText("credential configured").waitFor({ timeout: 15000 });
  await reloaded.getByRole("button", { name: "Test connection" }).click();
  await reloaded.getByText(/^Test:/).waitFor({ timeout: 30000 });
  page.once("dialog", (dialog) => dialog.accept());
  await reloaded.getByRole("button", { name: "Revoke credential" }).click();
  await page.getByText(/Credential revoked/).waitFor({ timeout: 15000 });
  const finalState = await reloaded.innerText();
  assert(finalState.includes("not configured"), "credential was not revoked");
  assert.deepEqual(errors, [], `console errors: ${errors.join(" | ")}`);
  console.log(JSON.stringify({ status: "PASS", provider: "opencode-go", credentialRevoked: true, paidInference: false }));
} finally {
  await browser.close();
}
