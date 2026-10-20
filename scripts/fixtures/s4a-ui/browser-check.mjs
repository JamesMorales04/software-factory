#!/usr/bin/env node
// Protected browser acceptance oracle for the F1-S4-A UI fixture. It is run
// by the Independent Verifier from a clean read-only candidate checkout. It
// starts a local static server, drives real Chromium, and fails on observable
// functional or browser errors. Only bounded metadata is printed.
import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, extname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const playwrightCore = process.env.FACTORY_PLAYWRIGHT_CORE;
const executablePath = process.env.CHROMIUM_EXE;
if (!playwrightCore || !executablePath) {
  console.error("browser tooling unavailable");
  process.exit(77);
}
const { chromium } = require(playwrightCore);

const types = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
};

async function serve() {
  const server = createServer(async (req, res) => {
    const raw = (req.url || "/").split("?")[0];
    if (raw === "/favicon.ico") {
      res.writeHead(204).end();
      return;
    }
    const rel = raw === "/" ? "index.html" : raw.replace(/^\/+/, "");
    const file = join(HERE, rel);
    if (!file.startsWith(HERE)) {
      res.writeHead(403).end();
      return;
    }
    try {
      const info = await stat(file);
      if (!info.isFile()) {
        res.writeHead(404).end();
        return;
      }
      const body = await readFile(file);
      res.writeHead(200, { "Content-Type": types[extname(file)] || "application/octet-stream" });
      res.end(body);
    } catch {
      res.writeHead(404).end();
    }
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const { port } = server.address();
  return { server, url: `http://127.0.0.1:${port}/` };
}

async function expectResult(page, want, label) {
  const deadline = Date.now() + 4000;
  for (;;) {
    const value = (await page.locator("#result").textContent())?.trim();
    if (value === want) return;
    if (Date.now() >= deadline) {
      throw new Error(`${label}: result ${value}, want ${want}`);
    }
    await page.waitForTimeout(100);
  }
}

async function main() {
  const { server, url } = await serve();
  let browser;
  try {
    browser = await chromium.launch({
      executablePath,
      headless: true,
      args: ["--no-sandbox", "--disable-dev-shm-usage"],
    });
    const context = await browser.newContext();
    const page = await context.newPage();
    const consoleErrors = [];
    const failedRequests = [];
    page.on("console", (message) => {
      if (message.type() === "error") consoleErrors.push(message.text());
    });
    page.on("pageerror", (error) => consoleErrors.push(String(error)));
    page.on("requestfailed", (request) => failedRequests.push(request.url()));
    await page.goto(url, { waitUntil: "load" });
    await page.fill("#a", "6");
    await page.fill("#b", "7");
    await page.selectOption("#op", "add");
    await page.click("#calc");
    await expectResult(page, "13", "add regression");
    await page.selectOption("#op", "multiply");
    await page.click("#calc");
    await expectResult(page, "42", "multiply acceptance");
    if (consoleErrors.length > 0) {
      throw new Error(`browser console errors: ${consoleErrors.length}`);
    }
    if (failedRequests.length > 0) {
      throw new Error(`failed requests: ${failedRequests.length}`);
    }
    console.log(
      JSON.stringify({
        browser: "chromium",
        browserVersion: browser.version(),
        consoleErrors: consoleErrors.length,
        failedRequests: failedRequests.length,
        userDataIsolated: true,
      }),
    );
  } finally {
    if (browser) await browser.close();
    server.close();
  }
}

main().catch((error) => {
  console.error(String(error?.message ?? error).slice(0, 300));
  process.exit(1);
});
