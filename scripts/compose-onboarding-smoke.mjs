import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { chromium } from "../office/e2e-browser/node_modules/playwright-core/index.mjs";

const base = process.env.FACTORY_OFFICE_URL || "http://127.0.0.1:8893";
const code = process.env.FACTORY_ENROLLMENT_CODE;
const executablePath = process.env.CHROMIUM_EXE;
const evidence = process.env.S4A_BROWSER_EVIDENCE_DIR || "/tmp/factory-onboarding-smoke";
if (!code) throw new Error("FACTORY_ENROLLMENT_CODE is required");
if (!executablePath) throw new Error("CHROMIUM_EXE is required");
mkdirSync(evidence, { recursive: true });

const browser = await chromium.launch({
  executablePath,
  headless: true,
  args: ["--no-sandbox", "--disable-dev-shm-usage"],
});
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
const consoleErrors = [];
const notFound = [];
page.on("console", (message) => {
  if (message.type() === "error") consoleErrors.push(message.text());
});
page.on("response", (response) => {
  if (response.status() === 404) notFound.push(response.url());
});
try {
  await page.goto(base, { waitUntil: "domcontentloaded" });
  await page.getByText(/Factory first-run setup/i).waitFor({ timeout: 15000 });
  await page.locator('.gate input[type="password"]').fill(code);
  await page.getByRole("button", { name: "Enroll owner" }).click();
  await page.getByRole("heading", { name: /Dashboard/i }).waitFor({ timeout: 15000 });

  await page.getByRole("link", { name: "Settings", exact: true }).click();
  await page.getByRole("heading", { name: /Providers & Models/i }).waitFor();
  await page.getByText("OpenCode Go", { exact: true }).waitFor();
  await page.getByText("Codex App Server", { exact: true }).waitFor();

  await page.getByRole("link", { name: "Projects", exact: true }).click();
  await page.getByTestId("project-repository").fill("https://github.com/rust-lang/rust-by-example.git");
  await page.getByRole("button", { name: "Register Project" }).click();
  await page.getByText(/Registered [0-9a-f]{8}/).waitFor({ timeout: 60000 });
  await page.getByRole("button", { name: "Configure" }).first().click();
  await page.getByText(/Project configuration/).waitFor();
  const pathInput = page.locator('input[placeholder="src, tests"]');
  await pathInput.fill("src");
  await page.getByLabel("repository-edit").check();
  await page.getByRole("button", { name: "Save project config" }).click();
  await page.getByText(/Project config revision/).waitFor({ timeout: 15000 });

  await page.getByRole("link", { name: "Actions", exact: true }).click();
  const objective = `onboarding smoke ${Date.now()}`;
  await page.getByTestId("action-objective").fill(objective);
  await page.getByRole("button", { name: "Create Action" }).click();
  await page.getByText(/Created Action/).waitFor({ timeout: 15000 });

  await page.getByRole("link", { name: "Delegations", exact: true }).click();
  await page.getByRole("button", { name: "Create delegation" }).click();
  await page.getByText(/Delegation .* created/).waitFor({ timeout: 15000 });

  await page.screenshot({ path: `${evidence}/onboarding-complete.png`, fullPage: true });
  assert.deepEqual(consoleErrors, [], `console errors: ${consoleErrors.join(" | ")}`);
  assert.deepEqual(notFound, [], `404 responses: ${notFound.join(" | ")}`);
  console.log(JSON.stringify({
    status: "PASS",
    base,
    onboarding: "office-first",
    project: "https://github.com/rust-lang/rust-by-example.git",
    actionObjective: objective,
    screenshot: `${evidence}/onboarding-complete.png`,
  }));
} finally {
  await browser.close();
}
