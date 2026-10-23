#!/usr/bin/env node
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
      response.writeHead(200, { "Content-Type": types[extname(file)] || "application/octet-stream" });
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

async function main() {
  const { server, url } = await serve();
  let browser;
  try {
    browser = await chromium.launch({ executablePath, headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage"] });
    const page = await browser.newPage();
    const errors = [];
    page.on("console", (message) => { if (message.type() === "error") errors.push(message.text()); });
    page.on("pageerror", (error) => errors.push(String(error)));
    await page.goto(url, { waitUntil: "load" });
    const text = await page.locator("#greeting").textContent();
    if (text !== "Hello, Factory!") throw new Error(`unexpected greeting: ${text}`);
    if (errors.length) throw new Error(`browser console errors: ${errors.join("; ")}`);
    console.log(JSON.stringify({ browser: "chromium", browserVersion: browser.version(), greeting: text, consoleErrors: 0 }));
  } finally {
    if (browser) await browser.close();
    await new Promise((resolve) => server.close(resolve));
  }
}

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
