// Stage 1 autonomous-intake browser journey (harness self-test, not independent
// acceptance).
//
// It observes exactly one request that the scenario harness already created with
// a single AUTONOMOUS `POST /intake/requests`, then asserts that Office shows
// Core-owned autopilot state without routine Director controls. It never calls
// planning or approval endpoints from the browser, and the owner-required flow
// is opt-in and separate.
//
// Required environment:
//   FACTORY_OFFICE_URL        e.g. http://core:8765 (Compose campaign network)
//   FACTORY_COOKIE_JAR        Netscape cookie jar written by the scenario runner
//   FACTORY_AUTONOMOUS_REQUEST_ID
// Optional:
//   CHROMIUM_EXE              Chromium binary (default from Playwright cache)
//   FACTORY_OWNER_REQUIRED=1  exercise the explicit owner-approval path
//   FACTORY_BROWSER_ARTIFACTS directory for screenshots/console logs
//
// The evidence is SIMULATED (fixture model) and must never be reported as live
// inference or independent verification.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { launchBrowser } from "./harness.mjs";

const officeUrl = (
  process.env.FACTORY_OFFICE_URL || "http://core:8765"
).replace(/\/$/, "");
const requestId = process.env.FACTORY_AUTONOMOUS_REQUEST_ID?.trim();
const cookieJar = process.env.FACTORY_COOKIE_JAR?.trim();
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

function parseNetscapeCookies(raw, url) {
  const host = new URL(url).hostname;
  return raw
    .split("\n")
    .filter((line) => line.trim() && !line.startsWith("#"))
    .map((line) => line.split("\t"))
    .filter((parts) => parts.length >= 7)
    .filter((parts) => {
      const domain = parts[0].replace(/^\./, "");
      return host === domain || host.endsWith(`.${domain}`);
    })
    .map((parts) => ({
      name: parts[5],
      value: parts[6],
      domain: parts[0],
      path: parts[2] || "/",
      secure: parts[3] === "TRUE",
      httpOnly: parts[1] === "TRUE",
      expires: Number(parts[4]) > 0 ? Number(parts[4]) : -1,
    }));
}

async function main() {
  if (!requestId) fail("FACTORY_AUTONOMOUS_REQUEST_ID is required");
  if (!cookieJar) fail("FACTORY_COOKIE_JAR is required");
  mkdirSync(artifacts, { recursive: true });

  const browser = await launchBrowser(chromium, { headless: true });
  try {
    const cookies = parseNetscapeCookies(
      readFileSync(cookieJar, "utf8"),
      officeUrl,
    );
    if (cookies.length === 0)
      fail("cookie jar has no cookie for the Office origin");
    await browser.browser.contexts()[0].addCookies(cookies);

    const page = browser.page;
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
      // The owner-required lane may legitimately surface the Director review CTA.
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
      // Wait for either Open Campaign (ready) or the waiting-owner paragraph.
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

    // Responsive sanity: the same Core-backed state must remain readable.
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
    if (timedOut) fail("browser close timed out");
  }
}

main().catch((error) => {
  process.stderr.write(`autonomous-intake-journey: ${error.message}\n`);
  process.exitCode = 1;
});
