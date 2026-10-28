// Real Chromium smoke for Agent Office and Campaign WorkGraph against Compose.
// Requires an already-running deterministic Compose stack and an operator token.
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "../office/e2e-browser/node_modules/playwright-core/index.mjs";

const base = process.env.FACTORY_OFFICE_URL || "http://127.0.0.1:8893";
const token = process.env.FACTORY_API_TOKEN;
const executablePath = process.env.CHROMIUM_EXE;
const evidenceDir =
  process.env.S4A_BROWSER_EVIDENCE_DIR || "/tmp/factory-agent-office-smoke";
if (!token) throw new Error("FACTORY_API_TOKEN is required");
if (!executablePath) throw new Error("CHROMIUM_EXE is required");
mkdirSync(evidenceDir, { recursive: true });

const launchArgs =
  process.env.FACTORY_BROWSER_NO_SANDBOX === "1"
    ? ["--no-sandbox", "--disable-dev-shm-usage"]
    : [];
const browser = await chromium.launch({
  headless: true,
  executablePath,
  ...(launchArgs.length > 0 ? { args: launchArgs } : {}),
});
const consoleErrors = [];
const pageErrors = [];
const notFound = [];
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });
  page.on("pageerror", (error) => pageErrors.push(String(error)));
  page.on("response", (response) => {
    if (response.status() === 404) notFound.push(response.url());
  });

  await page.goto(base);
  await page.getByPlaceholder("FACTORY_OPERATOR_TOKEN").fill(token);
  await page.getByRole("button", { name: "Unlock" }).click();
  await page.getByRole("heading", { name: "Overview", level: 1 }).waitFor();

  await page.getByRole("link", { name: "Agent Office", exact: true }).click();
  await page
    .getByRole("heading", { name: "Agent Office", exact: true, level: 1 })
    .waitFor();
  await page.getByTestId("agent-floor").waitFor();
  const implementer = page.getByRole("button", {
    name: /Inspect Implementer implementer:/,
  });
  await implementer.waitFor();
  await page.getByRole("button", { name: /Inspect Verifier verifier:/ }).waitFor();
  await page.getByRole("button", { name: /Inspect Reviewer reviewer:/ }).waitFor();
  await page.getByRole("button", { name: /Inspect Worker worker:/ }).waitFor();

  await implementer.click();
  const inspector = page.getByTestId("participant-inspector");
  await inspector.getByText("COMPLETED", { exact: true }).first().waitFor();
  await inspector.getByText("FINAL", { exact: true }).first().waitFor();
  await inspector
    .getByText("session_delete_verified_absent", { exact: true })
    .waitFor();
  await inspector
    .getByText("opencode/deepseek-v4.1-flash", { exact: false })
    .first()
    .waitFor();
  const toolTable = page.getByTestId("tool-event-table");
  await toolTable.getByText("write_file").first().waitFor();
  await toolTable.getByText("tool requested").first().waitFor();
  await toolTable.getByText("write applied").first().waitFor();
  await toolTable.getByText("provider content withheld").first().waitFor();
  assert.equal(
    await page.getByText("provider-sessions", { exact: false }).count(),
    0,
    "the projection must not render provider directories",
  );

  await page.getByRole("tab", { name: /Table/ }).click();
  await page.getByTestId("agent-table").waitFor();
  await page.getByRole("tab", { name: /Timeline/ }).click();
  await page.getByTestId("agent-timeline").waitFor();
  await page.getByRole("tab", { name: /Visual Office/ }).click();
  await page.getByTestId("agent-floor").waitFor();
  await page
    .getByTestId("agent-filters")
    .getByLabel("Campaign", { exact: true })
    .selectOption({ index: 1 });
  await page.getByTestId("agent-floor").waitFor();
  await page.getByRole("button", { name: /Inspect Implementer implementer:/ }).click();
  await page.getByTestId("participant-inspector").waitFor();
  const participantUrl = page.url();
  await page.reload();
  await page
    .getByRole("heading", { name: "Agent Office", exact: true, level: 1 })
    .waitFor();
  await page.getByTestId("participant-inspector").waitFor();
  await page.getByTestId("tool-event-table").getByText("write applied").first().waitFor();
  assert.equal(page.url(), participantUrl, "participant context must survive reload");

  await page
    .locator(".sidebar")
    .getByRole("link", { name: "Campaigns", exact: true })
    .click();
  await page.getByRole("heading", { name: "Campaigns", exact: true }).waitFor();
  await page
    .locator('tbody tr a[href^="#/campaigns/"]')
    .first()
    .click();
  await page.getByRole("heading", { name: /^Campaign /, level: 1 }).waitFor();
  await page.getByRole("tab", { name: /WorkGraph/ }).click();
  await page.getByRole("heading", { name: /WorkGraph \(/ }).waitFor();
  const nodes = page.locator('[data-testid^="workgraph-node-"]');
  const nodeCount = await nodes.count();
  if (nodeCount > 0) {
    await page.getByTestId("workgraph-table").waitFor();
    await nodes.first().click();
    await page.getByTestId("workgraph-inspector").waitFor();
    await page.waitForURL(/tab=workgraph&unit=/);
    const workGraphUrl = page.url();
    await page.reload();
    await page.getByTestId("workgraph-inspector").waitFor();
    assert.equal(page.url(), workGraphUrl, "WorkUnit selection must survive reload");
  } else {
    await page.getByText("No WorkGraph units").waitFor();
  }

  for (const width of [834, 390]) {
    await page.setViewportSize({ width, height: 900 });
    const viewport = await page.evaluate(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    }));
    assert.equal(
      viewport.scrollWidth,
      viewport.clientWidth,
      `Campaign WorkGraph must not overflow the document at ${width}px`,
    );
  }
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.screenshot({
    path: join(evidenceDir, "agent-office-workgraph.png"),
    fullPage: true,
  });

  const unexpectedNotFound = notFound.filter(
    (url) => !/\/core-api\/quality\/candidates\/[^/]+\/gate$/.test(url),
  );
  assert.deepEqual(
    unexpectedNotFound,
    [],
    `unexpected 404 responses: ${unexpectedNotFound.join("; ")}`,
  );
  const unexpectedConsole = consoleErrors.filter(
    (message) =>
      message !==
      "Failed to load resource: the server responded with a status of 404 (Not Found)",
  );
  assert.deepEqual(
    unexpectedConsole,
    [],
    `unexpected browser console errors: ${unexpectedConsole.join("; ")}`,
  );
  assert.deepEqual(pageErrors, [], `browser page errors: ${pageErrors.join("; ")}`);

  const summary = {
    ok: true,
    route: "/agents",
    participants: ["implementer", "verifier", "reviewer", "worker"],
    toolEvents: true,
    workGraphUnits: nodeCount,
    widths: [1440, 834, 390],
    evidence: join(evidenceDir, "agent-office-workgraph.png"),
  };
  writeFileSync(
    join(evidenceDir, "agent-office-workgraph.json"),
    `${JSON.stringify(summary, null, 2)}\n`,
  );
  console.log(JSON.stringify(summary));
} finally {
  await browser.close().catch(() => undefined);
}
