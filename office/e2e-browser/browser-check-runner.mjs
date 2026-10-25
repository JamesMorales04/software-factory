#!/usr/bin/env node
// Fixed browser-check runner used by the Rust `factory-browser` capability.
//
// It is deliberately argument-driven: the Rust caller passes an authorized
// loopback URL, an assertion and disposable paths as argv. The only inherited
// environment is the explicit browser tooling allowlist applied by Rust.
//
// Exit codes:
//   0  assertion passed and evidence was written
//   1  assertion/check failed
//   77 browser tooling is unavailable (NOT_TESTED; never PASS)
import { mkdirSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname } from "node:path";

function fail(message, code = 1) {
  console.error(String(message ?? "browser check failed").slice(0, 500));
  process.exit(code);
}

function option(argv, name) {
  const index = argv.indexOf(`--${name}`);
  if (index === -1) return undefined;
  const value = argv[index + 1];
  if (value === undefined) fail(`missing value for --${name}`);
  return value;
}

function required(argv, name) {
  const value = option(argv, name);
  if (!value) fail(`missing required --${name}`);
  return value;
}

const number = (value, name, fallback) => {
  if (value === undefined) return fallback;
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed <= 0) fail(`invalid --${name}`);
  return parsed;
};

const argv = process.argv.slice(2);
const url = required(argv, "url");
const profile = required(argv, "profile");
const evidencePath = required(argv, "evidence");
const selector = option(argv, "selector");
const text = option(argv, "text");
const screenshot = option(argv, "screenshot");
const timeoutMs = number(option(argv, "timeout-ms"), "timeout-ms", 60000);

if (!selector && !text) fail("one of --selector or --text is required");
if (!/^http:\/\/127\.0\.0\.1:\d+\//.test(url)) {
  fail("browser check URL must be an authorized loopback endpoint");
}

const playwrightCore = process.env.FACTORY_PLAYWRIGHT_CORE;
if (!playwrightCore) {
  fail("browser tooling unavailable: FACTORY_PLAYWRIGHT_CORE is not set", 77);
}
let chromium;
try {
  ({ chromium } = createRequire(import.meta.url)(playwrightCore));
} catch (error) {
  fail(`browser tooling unavailable: ${error?.message ?? error}`, 77);
}
if (!chromium) {
  fail("browser tooling unavailable: playwright-core has no chromium", 77);
}

mkdirSync(dirname(evidencePath), { recursive: true, mode: 0o700 });
mkdirSync(profile, { recursive: true, mode: 0o700 });

const executablePath = process.env.CHROMIUM_EXE || undefined;
let consoleErrors = 0;
let failedRequests = 0;
let context;
let failure;

try {
  context = await chromium.launchPersistentContext(profile, {
    executablePath,
    headless: true,
    timeout: timeoutMs,
    args: [
      "--no-sandbox",
      "--disable-dev-shm-usage",
      "--no-first-run",
      "--no-default-browser-check",
      "--disable-background-networking",
    ],
  });
  context.setDefaultTimeout(timeoutMs);
  context.setDefaultNavigationTimeout(timeoutMs);
  const page = context.pages()[0] ?? (await context.newPage());
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors += 1;
  });
  page.on("pageerror", () => {
    consoleErrors += 1;
  });
  page.on("requestfailed", () => {
    failedRequests += 1;
  });
  await page.goto(url, { waitUntil: "load", timeout: timeoutMs });
  if (selector) {
    await page.locator(selector).first().waitFor({
      state: "visible",
      timeout: timeoutMs,
    });
  }
  if (text) {
    await page.waitForFunction(
      (expected) =>
        (document.body?.innerText ?? "").includes(expected),
      text,
      { timeout: timeoutMs },
    );
  }
  const title = (await page.title()).slice(0, 512);
  if (screenshot) {
    mkdirSync(dirname(screenshot), { recursive: true, mode: 0o700 });
    await page.screenshot({ path: screenshot, fullPage: true });
  }
  const evidence = {
    status: "PASSED",
    url: page.url().slice(0, 2048),
    title,
    consoleErrors,
    failedRequests,
    browserVersion: String(context.browser()?.version() ?? "unknown").slice(
      0,
      128,
    ),
    screenshot: screenshot ?? null,
    profileRemoved: false,
  };
  writeFileSync(evidencePath, `${JSON.stringify(evidence, null, 2)}\n`, {
    mode: 0o600,
  });
  process.stdout.write(`${JSON.stringify(evidence)}\n`);
} catch (error) {
  failure = error;
} finally {
  if (context) {
    try {
      await context.close();
    } catch {
      // Bounded shutdown is enforced by the Rust process-group kill.
    }
  }
}

if (failure) {
  console.error(String(failure?.message ?? failure).slice(0, 500));
  process.exitCode = 1;
}
