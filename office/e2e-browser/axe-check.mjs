// Real-browser accessibility scan for a running Factory Office.
//
// This is an automated floor, not a WCAG conformance claim. It intentionally
// scans the rendered page with axe-core; keyboard order, focus visibility,
// screen-reader output, 320px reflow and content quality remain manual checks
// in the ui-change-acceptance skill.

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import { launchBrowser } from "./harness.mjs";

const root = resolve(import.meta.dirname, "../..");
const url = process.env.FACTORY_OFFICE_URL;
const executablePath =
  process.env.CHROMIUM_EXE ||
  join(
    process.env.HOME || "",
    ".cache/ms-playwright/chromium-1248/chrome-linux64/chrome",
  );
const output = process.env.FACTORY_A11Y_OUTPUT
  ? resolve(process.env.FACTORY_A11Y_OUTPUT)
  : join(root, "target/qh/office-a11y-scan.json");

if (!url) {
  console.error(
    "FACTORY_OFFICE_URL is required (for example http://127.0.0.1:8471)",
  );
  process.exit(2);
}

const { page, close } = await launchBrowser(executablePath, {
  timeoutMs: 60_000,
  headless: true,
});
let result;
try {
  await page.goto(url, { waitUntil: "networkidle", timeout: 60_000 });
  result = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"])
    .analyze();
} finally {
  await close();
}

const blocking = result.violations.filter(
  (violation) =>
    violation.impact === "critical" || violation.impact === "serious",
);
const report = {
  schema: "factory-office-a11y-scan-v1",
  url,
  scannedAt: new Date().toISOString(),
  browser: "chromium",
  standard: "WCAG 2.2 A/AA (axe tags)",
  automatedOnly: true,
  limitations: [
    "Automated checks cover only a subset of WCAG.",
    "Keyboard order, focus visibility, screen-reader output, content quality and 320px reflow remain manual.",
  ],
  counts: {
    violations: result.violations.length,
    blocking: blocking.length,
  },
  blocking: blocking.map((violation) => ({
    id: violation.id,
    impact: violation.impact,
    help: violation.help,
    nodes: violation.nodes.length,
    targets: violation.nodes.slice(0, 10).map((node) => node.target),
  })),
  all: result.violations.map((violation) => ({
    id: violation.id,
    impact: violation.impact,
    help: violation.help,
    nodes: violation.nodes.length,
  })),
};
mkdirSync(dirname(output), { recursive: true });
writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
console.log(JSON.stringify(report, null, 2));
if (blocking.length > 0) {
  process.exitCode = 1;
}
