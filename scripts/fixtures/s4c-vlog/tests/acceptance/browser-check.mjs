#!/usr/bin/env node
// Protected browser acceptance oracle for the vlog fixture. The Implementer
// must not modify this file. It is executed by Factory's Independent
// Verifier from a clean read-only candidate checkout.
import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { createRequire } from "node:module";
import { extname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = fileURLToPath(new URL("../..", import.meta.url));
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
  ".css": "text/css; charset=utf-8",
};

async function serve() {
  const server = createServer(async (request, response) => {
    const raw = (request.url || "/").split("?")[0];
    if (raw === "/favicon.ico") return response.writeHead(204).end();
    const rel = raw === "/" ? "index.html" : raw.replace(/^\/+/, "");
    const file = join(HERE, rel);
    if (!file.startsWith(HERE)) return response.writeHead(403).end();
    try {
      const info = await stat(file);
      if (!info.isFile()) throw new Error("not a file");
      response.writeHead(200, {
        "Content-Type": types[extname(file)] || "application/octet-stream",
      });
      response.end(await readFile(file));
    } catch {
      response.writeHead(404).end();
    }
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  return { server, url: `http://127.0.0.1:${server.address().port}/` };
}

async function visibleTitles(page) {
  return page.locator("#entries .entry h2").allTextContents();
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

    if ((await visibleTitles(page)).length !== 5) {
      throw new Error("baseline: expected five visible entries");
    }
    await page.getByRole("button", { name: "Travel", exact: true }).click();
    await page.waitForFunction(
      () => document.querySelectorAll("#entries .entry").length === 2,
    );
    const travel = await visibleTitles(page);
    if (!travel.every((title) => title.length > 0)) {
      throw new Error("travel filter rendered empty titles");
    }
    await page.getByRole("button", { name: "All", exact: true }).click();
    if ((await visibleTitles(page)).length !== 5) {
      throw new Error("All filter did not restore five entries");
    }
    const emptyButton = page.getByRole("button", { name: "Cooking", exact: true });
    if ((await emptyButton.count()) !== 1) {
      throw new Error("Cooking category control is missing");
    }
    // Film is an authorized category with no sample entries, proving the
    // no-results state deterministically.
    const categories = await page
      .locator("#filters button")
      .allTextContents();
    if (!categories.includes("Film")) {
      throw new Error("missing empty category control");
    }
    await page.getByRole("button", { name: "Film", exact: true }).click();
    await page.waitForFunction(
      () => !document.querySelector("#empty-state").hidden,
    );
    if ((await page.locator("#entries .entry").count()) !== 0) {
      throw new Error("empty category still renders entries");
    }
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
