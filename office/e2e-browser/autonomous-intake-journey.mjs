// Stage 1 autonomous-intake browser journey (harness self-test, not independent
// acceptance).
//
// It observes exactly one request that the scenario harness already created with
// a single AUTONOMOUS `POST /intake/requests`, then asserts that Office shows
// Core-owned autopilot state without routine Director controls. It never calls
// planning or approval endpoints from the browser, and the owner-required flow
// is opt-in and separate.
//
// Authentication handoff: the Python driver exports Playwright `storageState`
// JSON from its already-parsed LWP cookie jar (`cookies.storage.json`). This
// journey never parses a second cookie format.
//
// Required environment:
//   FACTORY_OFFICE_URL        e.g. http://127.0.0.1:8765
//   FACTORY_STORAGE_STATE     storageState JSON written by the scenario driver
//   FACTORY_AUTONOMOUS_REQUEST_ID
// Optional:
//   FACTORY_OFFICE_UPSTREAM   e.g. http://core:8765. When set, the journey
//                             starts a verification-only loopback TCP proxy so
//                             the browser Origin/Host are loopback, which is the
//                             origin shape Core supports for http. This is a
//                             verification route, not certified product access.
//   CHROMIUM_EXE              Chromium binary (default from Playwright cache)
//   FACTORY_OWNER_REQUIRED=1  exercise the explicit owner-approval path
//   FACTORY_BROWSER_ARTIFACTS directory for screenshots/console logs
//
// The evidence is SIMULATED (fixture model) and must never be reported as live
// inference or independent verification.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { connect, createServer } from "node:net";
import { join } from "node:path";
import { launchBrowser } from "./harness.mjs";

const requestId = process.env.FACTORY_AUTONOMOUS_REQUEST_ID?.trim();
const storageStatePath = process.env.FACTORY_STORAGE_STATE?.trim();
const upstream = process.env.FACTORY_OFFICE_UPSTREAM?.trim();
const directUrl = (process.env.FACTORY_OFFICE_URL || "").trim();
const ownerRequired = process.env.FACTORY_OWNER_REQUIRED === "1";
const artifacts =
  process.env.FACTORY_BROWSER_ARTIFACTS?.trim() || "/tmp/autonomous-browser";
const chromium =
  process.env.CHROMIUM_EXE ||
  "/home/factory/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome";

const LABELS = {
  waitingOwner: [
    "Core has finished the planning materials and is waiting for the owner plan decision.",
    "Core terminó los materiales del plan y espera la decisión del owner.",
  ],
  autopilotOwns: [
    "Autopilot owns these deterministic steps in Core.",
    "El autopilot posee estos pasos deterministas en Core.",
  ],
  approvePlan: ["Approve plan", "Aprobar plan"],
  openCampaign: ["Open Campaign", "Abrir Campaña"],
  manualControls: [
    "Run assessment",
    "Ejecutar evaluación",
    "Approval rationale",
    "Justificación de la aprobación",
    "Approve frozen plan and start",
    "Aprobar el plan congelado e iniciar",
    "Enable governed execution",
    "Habilitar ejecución gobernada",
    "Open Director plan review (Compile plan / Record participant plan)",
    "Abrir la revisión del plan en el Director (Compilar plan / Registrar plan de participantes)",
  ],
  blockedHeading: ["Blocked request", "Solicitud bloqueada"],
  awaitingApproval: ["Awaiting approval", "Pendiente de aprobación"],
};

const findings = [];
const consoleErrors = [];

function fail(message) {
  throw new Error(message);
}

function record(check, detail) {
  findings.push({ check, status: "PASS", detail });
}

function assertAnyLabel(text, labels, description) {
  const match = labels.find((label) => text.includes(label));
  if (!match)
    fail(
      `${description}: none of ${labels.length} expected labels was visible`,
    );
  record(description, match);
}

function readStorageState(path) {
  let payload;
  try {
    payload = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    fail(`storageState is not readable JSON: ${error.message}`);
  }
  if (!payload || !Array.isArray(payload.cookies)) {
    fail("storageState is missing a cookies array");
  }
  if (payload.cookies.length === 0) fail("storageState has no cookies");
  for (const cookie of payload.cookies) {
    if (typeof cookie.name !== "string" || typeof cookie.value !== "string") {
      fail("storageState contains a malformed cookie");
    }
    if (typeof cookie.domain !== "string" || cookie.domain.length === 0) {
      fail("storageState cookie is missing a domain");
    }
  }
  return payload.cookies;
}

// Verification-only loopback proxy. Core's supported http Origin shape is a
// loopback authority equal to the Host; forwarding the request unchanged to the
// Office upstream keeps Host/Origin as 127.0.0.1:PORT and preserves auth exactly.
function startLoopbackProxy(targetUrl) {
  const target = new URL(targetUrl);
  const targetPort = Number(target.port || "80");
  return new Promise((resolve, reject) => {
    const server = createServer((client) => {
      const upstreamSocket = connect(targetPort, target.hostname);
      client.on("error", () => upstreamSocket.destroy());
      upstreamSocket.on("error", () => client.destroy());
      client.pipe(upstreamSocket);
      upstreamSocket.pipe(client);
    });
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      resolve({
        url: `http://127.0.0.1:${port}`,
        close: () =>
          new Promise((done) => {
            server.close(() => done());
          }),
      });
    });
  });
}

async function main() {
  if (!requestId) fail("FACTORY_AUTONOMOUS_REQUEST_ID is required");
  if (!storageStatePath) fail("FACTORY_STORAGE_STATE is required");
  if (process.env.FACTORY_COOKIE_JAR) {
    fail(
      "FACTORY_COOKIE_JAR is no longer part of the contract; use FACTORY_STORAGE_STATE",
    );
  }
  if (!directUrl && !upstream) {
    fail("set FACTORY_OFFICE_URL or FACTORY_OFFICE_UPSTREAM");
  }
  mkdirSync(artifacts, { recursive: true });

  const cookies = readStorageState(storageStatePath);
  const proxy = upstream ? await startLoopbackProxy(upstream) : null;
  const browser = await launchBrowser(chromium, { headless: true });
  try {
    await browser.browser.contexts()[0].addCookies(cookies);

    const page = browser.page;
    const officeUrl = (proxy ? proxy.url : directUrl).replace(/\/$/, "");
    page.on("console", (message) => {
      if (message.type() === "error") consoleErrors.push(message.text());
    });
    page.on("pageerror", (error) => consoleErrors.push(String(error)));

    await page.goto(`${officeUrl}/requests/${requestId}`, {
      waitUntil: "domcontentloaded",
    });
    await page
      .waitForLoadState("networkidle", { timeout: 30000 })
      .catch(() => {});

    const body = await page.locator("body").innerText();
    if (LABELS.blockedHeading.some((label) => body.includes(label))) {
      fail(
        "request is blocked; the autonomous positive expected preparation or ready state",
      );
    }
    if (ownerRequired) {
      assertAnyLabel(body, LABELS.waitingOwner, "waiting-for-owner paragraph");
      assertAnyLabel(
        body,
        LABELS.approvePlan,
        "explicit owner approve control",
      );
      record(
        "owner-required lane",
        "waiting-owner UI with explicit approval control",
      );
    } else {
      assertAnyLabel(
        [...LABELS.autopilotOwns, ...LABELS.openCampaign],
        "autopilot ownership or Open Campaign",
      );
      for (const label of LABELS.manualControls) {
        if (body.includes(label)) {
          fail(`routine manual control must be absent in AUTONOMOUS: ${label}`);
        }
      }
      record(
        "no routine manual controls",
        "no Director CTA or manual planning control",
      );
    }

    if (!ownerRequired) {
      const deadline = Date.now() + 60000;
      let reached = false;
      while (Date.now() < deadline) {
        const text = await page.locator("body").innerText();
        if (
          LABELS.openCampaign.some((label) => text.includes(label)) ||
          LABELS.waitingOwner.some((label) => text.includes(label))
        ) {
          reached = true;
          break;
        }
        await page.waitForTimeout(2000);
      }
      if (!reached)
        fail(
          "Office never showed Open Campaign or the waiting-owner paragraph",
        );
      record(
        "autopilot projection reached",
        "Core-backed state visible without manual refresh",
      );
    }

    for (const [width, height] of [
      [1440, 900],
      [834, 1112],
      [390, 844],
      [320, 720],
    ]) {
      await page.setViewportSize({ width, height });
      await page.waitForTimeout(300);
      const overflow = await page.evaluate(
        () =>
          document.documentElement.scrollWidth -
          document.documentElement.clientWidth,
      );
      if (overflow > 1)
        fail(`horizontal overflow ${overflow}px at ${width}x${height}`);
      record(`responsive ${width}x${height}`, "no horizontal overflow");
    }

    await page.setViewportSize({ width: 1440, height: 900 });
    await page.screenshot({
      path: join(artifacts, "autonomous-intake.png"),
      fullPage: false,
    });
    if (consoleErrors.length > 0) {
      fail(`browser console errors: ${consoleErrors.slice(0, 3).join(" | ")}`);
    }
    record("console", "no console errors");

    writeFileSync(
      join(artifacts, "autonomous-intake.json"),
      `${JSON.stringify(
        {
          requestId,
          ownerRequired,
          originRoute: upstream ? "loopback-proxy" : "direct",
          modelEvidence: "SIMULATED",
          liveInference: "NOT_TESTED",
          independentAcceptance: false,
          findings,
        },
        null,
        2,
      )}\n`,
    );
  } finally {
    const timedOut = await browser.close();
    if (proxy) await proxy.close();
    if (timedOut) fail("browser close timed out");
  }
}

main().catch((error) => {
  process.stderr.write(`autonomous-intake-journey: ${error.message}\n`);
  process.exitCode = 1;
});
