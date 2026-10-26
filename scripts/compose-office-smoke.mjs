import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { chromium } from "../office/e2e-browser/node_modules/playwright-core/index.mjs";

const base = process.env.FACTORY_OFFICE_URL || "http://127.0.0.1:8893";
const token = process.env.FACTORY_API_TOKEN;
const executablePath = process.env.CHROMIUM_EXE;
const evidence = process.env.S4A_BROWSER_EVIDENCE_DIR || "/tmp/factory-office-smoke";
if (!token) throw new Error("FACTORY_API_TOKEN is required");
if (!executablePath) throw new Error("CHROMIUM_EXE is required");
mkdirSync(evidence, { recursive: true });

const browser = await chromium.launch({
  executablePath,
  headless: true,
  args: ["--no-sandbox", "--disable-dev-shm-usage"],
});
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
const consoleErrors = [];
const failedRequests = [];
const notFound = [];
page.on("response", (response) => {
  if (response.status() === 404) notFound.push(response.url());
});
page.on("console", (message) => {
  if (message.type() === "error") consoleErrors.push(message.text());
});
page.on("requestfailed", (request) => {
  failedRequests.push(`${request.method()} ${request.url()} ${request.failure()?.errorText}`);
});
try {
  await page.goto(base, { waitUntil: "domcontentloaded" });
  const gate = page.locator(".gate");
  if (await gate.isVisible().catch(() => false)) {
    const heading = await gate.locator("h2").innerText();
    assert(!heading.toLowerCase().includes("first-run"), "deployment is still in first-run setup");
    await page.locator('.gate input[type="password"]').fill(token);
    await page.getByRole("button", { name: "Unlock" }).click();
  }
  await page.getByRole("heading", { name: /Dashboard|Factory Office/i }).first().waitFor({ timeout: 15000 });

  const nav = ["Settings", "Delegations", "Projects", "Actions", "Lead", "Campaigns"];
  for (const label of nav) {
    await page.getByRole("link", { name: label, exact: true }).waitFor({ timeout: 5000 });
  }

  await page.getByRole("link", { name: "Settings", exact: true }).click();
  await page.getByRole("heading", { name: /Providers & Models/i }).waitFor();
  await page.getByText("OpenCode Go", { exact: true }).waitFor();
  await page.getByText("Codex App Server", { exact: true }).waitFor();
  await page.getByText(/ChatGPT OAuth/i).first().waitFor();
  await page.getByRole("heading", { name: /Review & runtime policy/i }).waitFor();

  await page.getByRole("link", { name: "Projects", exact: true }).click();
  await page.getByRole("heading", { name: "Projects", exact: true }).waitFor();
  await page.getByRole("button", { name: "Configure" }).first().click();
  await page.getByText(/Project configuration/).waitFor();

  await page.getByRole("link", { name: "Actions", exact: true }).click();
  await page.getByRole("heading", { name: "Actions", exact: true }).waitFor();
  await page.getByText("Frozen checks").waitFor();
  const commandInput = page.locator('input[value*="provider candidate"]').first();
  await commandInput.waitFor();
  const objective = `compose browser smoke ${Date.now()}`;
  await page.getByTestId("action-objective").fill(objective);
  await page.getByRole("button", { name: "Create Action" }).click();
  try {
    await page.getByText(/Created Action/).waitFor({ timeout: 15000 });
  } catch (error) {
    const pageError = await page.locator(".error").first().innerText().catch(() => "no error text");
    await page.screenshot({ path: `${evidence}/action-failure.png`, fullPage: true });
    throw new Error(`Action creation failed: ${pageError}`);
  }

  await page.getByRole("link", { name: "Delegations", exact: true }).click();
  await page.getByRole("heading", { name: "Delegations", exact: true }).waitFor();
  await page.getByRole("button", { name: "Create delegation" }).waitFor();

  await page.screenshot({ path: `${evidence}/office-settings.png`, fullPage: true });
  assert.deepEqual(consoleErrors, [], `console errors: ${consoleErrors.join(" | ")}`);
  assert.deepEqual(failedRequests, [], `failed requests: ${failedRequests.join(" | ")}`);
  assert.deepEqual(notFound, [], `404 responses: ${notFound.join(" | ")}`);
  console.log(JSON.stringify({
    status: "PASS",
    base,
    actionObjective: objective,
    consoleErrors,
    failedRequests,
    notFound,
    screenshot: `${evidence}/office-settings.png`,
  }));
} finally {
  await browser.close();
}
