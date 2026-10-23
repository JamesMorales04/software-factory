// S4-D Office projection fixture. It intercepts every API request with
// deterministic data; it never contacts Core, Paseo or a model provider.
import { existsSync } from "node:fs";
import { launchBrowser } from "./harness.mjs";

const BASE = process.env.BASE_URL || "http://127.0.0.1:5173";
const EXE = process.env.CHROMIUM_EXE;
const TOKEN = "s4d-browser-fixture-token";
const REQUEST_ID = "lead-fixture-request";

const baseURL = new URL(BASE);
if (!(baseURL.hostname === "127.0.0.1" || baseURL.hostname === "localhost")) {
  throw new Error("S4-D fixture requires a loopback Office dev server");
}
if (!EXE || !existsSync(EXE)) {
  throw new Error("CHROMIUM_EXE must name an installed Chromium executable");
}

const readyRequest = {
  id: REQUEST_ID,
  projectId: "fixture-project",
  objective: "Add bounded search to the fixture app",
  delegationId: "fixture-delegation",
  status: "ready",
  authorizationState: "permitted",
  startMode: "awaiting_owner_or_autonomous_policy",
  delegationUsage: {
    delegationId: "fixture-delegation",
    reservedRequests: 1,
    reservedSessions: 4,
    reservedAttempts: 2,
  },
  assessment: {
    schemaRevision: 1,
    intent: "change",
    complexity: "small",
    evidenceRefs: ["repository-files"],
    uncertainty: ["Effort estimate has low confidence."],
    effortEstimate: {
      status: "estimated",
      unit: "person-hours",
      minPersonHours: 2,
      maxPersonHours: 5,
      assumptions: ["Existing search utilities can be reused."],
      unknownReason: "",
    },
    suggestedNextAction: "Review and start the frozen plan.",
  },
  modelObservation: {
    mechanism: "MODEL",
    provider: "fixture-provider",
    model: "fixture-model",
    mode: "build",
    elapsed: 1250000000,
    inputBytes: 1200,
    outputBytes: 900,
  },
  plan: {
    projectId: "fixture-project",
    actionId: "fixture-action",
    baseRevision: "a".repeat(40),
    objective: "Add bounded search to the fixture app",
    class: "feature",
    qualityProfile: "node-basic",
    provider: "fixture-provider",
    model: "fixture-model",
    paseoMode: "build",
    assumptions: ["Search is case-insensitive."],
    risks: ["Existing filters may interact with search."],
    acceptanceCriteria: [
      {
        id: "functional",
        kind: "functional",
        description: "Search narrows results.",
      },
    ],
    negativeCases: [
      {
        id: "empty",
        kind: "negative",
        description: "Empty query preserves results.",
      },
    ],
    riskClass: "R0",
    budget: { maxAttempts: 1, timeoutSecs: 120 },
    maxRepairCycles: 0,
    maxWallClockSecs: 600,
    maxParallel: 1,
    maxSessions: 4,
    workUnits: [
      {
        id: "backend",
        kind: "implementation",
        description: "Add the bounded search query.",
        allowedPaths: ["src/search.ts"],
      },
      {
        id: "ui",
        kind: "implementation",
        description: "Render search results in the existing view.",
        dependsOn: ["backend"],
        allowedPaths: ["src/view.ts"],
      },
    ],
  },
  planHash: "fixture-plan-hash",
  actionId: "fixture-action",
  actionSpecId: "fixture-spec",
  campaignId: "fixture-campaign",
  campaignStatus: "pending",
  createdAt: "2026-10-22T12:00:00Z",
  updatedAt: "2026-10-22T12:00:00Z",
};

let request = structuredClone(readyRequest);
const checks = [];
const apiCalls = [];
let page;
let closeBrowser;

function check(name, pass, detail = "") {
  checks.push({ name, pass, ...(detail ? { detail } : {}) });
  if (!pass) throw new Error(`${name}${detail ? `: ${detail}` : ""}`);
}

try {
  ({ page, close: closeBrowser } = await launchBrowser(EXE, {
    timeoutMs: 60000,
    headless: true,
  }));
  await page.route("**/api/v1/**", async (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname;
    const method = route.request().method();
    apiCalls.push(`${method} ${path}`);
    const headers = { "content-type": "application/json" };
    if (path === "/api/v1/projects" && method === "GET") {
      await route.fulfill({
        status: 200,
        headers,
        body: JSON.stringify({
          projects: [
            {
              id: "fixture-project",
              name: "Fixture Project",
              registrationStatus: "registered",
              status: "active",
              baseRefs: ["main"],
              qualityProfile: "node-basic",
              workspaceScope: ".",
              capabilities: ["repository-edit", "node-toolchain"],
              browserEndpoints: [],
              executionMode: "trusted-local",
              securityLabel: "trusted-local",
              configRevision: 1,
              configHash: "fixture-project-hash",
              createdAt: "2026-10-22T12:00:00Z",
              updatedAt: "2026-10-22T12:00:00Z",
            },
          ],
        }),
      });
      return;
    }
    if (path === "/api/v1/lead/requests" && method === "POST") {
      const input = route.request().postDataJSON();
      check(
        "submitted Project, objective and explicit delegation reach Core API boundary",
        input.projectId === "fixture-project" &&
          input.objective === "Find matching notes" &&
          input.delegationId === "fixture-delegation",
        JSON.stringify(input),
      );
      await route.fulfill({
        status: 201,
        headers,
        body: JSON.stringify({ request }),
      });
      return;
    }
    if (path === `/api/v1/lead/requests/${REQUEST_ID}` && method === "GET") {
      await route.fulfill({
        status: 200,
        headers,
        body: JSON.stringify({ request }),
      });
      return;
    }
    if (
      path === `/api/v1/lead/requests/${REQUEST_ID}/events` &&
      method === "GET"
    ) {
      await route.fulfill({
        status: 200,
        headers,
        body: JSON.stringify({
          events: [
            {
              eventId: "fixture-event-1",
              eventType: "LeadRequestCreated",
              schemaVersion: "1",
              aggregateType: "LeadRequest",
              aggregateId: REQUEST_ID,
              sequence: 1,
              globalSeq: 1,
              occurredAt: "2026-10-22T12:00:00Z",
              recordedAt: "2026-10-22T12:00:00Z",
              correlationId: "fixture-correlation",
              causationId: "fixture-causation",
              actor: "local-operator",
              payload: { requestId: REQUEST_ID },
            },
          ],
        }),
      });
      return;
    }
    if (
      path === `/api/v1/lead/requests/${REQUEST_ID}/start` &&
      method === "POST"
    ) {
      request = { ...request, status: "running", campaignStatus: "running" };
      await route.fulfill({
        status: 200,
        headers,
        body: JSON.stringify({ request }),
      });
      return;
    }
    await route.fulfill({
      status: 404,
      headers,
      body: JSON.stringify({
        error: {
          code: "not_found",
          message: `Unexpected fixture route ${method} ${path}`,
        },
      }),
    });
  });

  await page.goto(`${BASE}/#/lead`, { waitUntil: "domcontentloaded" });
  await page.getByPlaceholder("FACTORY_API_TOKEN").fill(TOKEN);
  await page.getByRole("button", { name: "Unlock" }).click();
  await page.getByRole("heading", { name: "Lead", exact: true }).waitFor();
  await page.getByTestId("lead-project").selectOption("fixture-project");
  await page.getByTestId("lead-objective").fill("Find matching notes");
  await page.getByTestId("lead-delegation").fill("fixture-delegation");
  await page.getByRole("button", { name: "Submit objective" }).click();

  await page.getByText(REQUEST_ID).waitFor();
  await page.getByText("LeadRequestCreated").waitFor();
  check(
    "server assessment and uncertainty are rendered",
    await page.getByText("Effort estimate has low confidence.").isVisible(),
  );
  const projectionText = await page.locator("body").innerText();
  check(
    "assessment effort estimate and actual model observation fields are rendered",
    projectionText.includes("Effort estimate unit") &&
      projectionText.includes("person-hours") &&
      projectionText.includes("1250000000") &&
      projectionText.includes("fixture-provider"),
  );
  check(
    "proposal risks and plan hash are rendered",
    (await page
      .getByText("Existing filters may interact with search.")
      .isVisible()) && (await page.getByText("fixture-plan-hash").isVisible()),
  );
  check(
    "frozen spec binding and quality profile are rendered for review",
    (await page.getByText("node-basic").isVisible()) &&
      (await page
        .getByText("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .isVisible()),
  );
  check(
    "Action, ActionSpec and Campaign links are rendered",
    (await page.getByText("fixture-action").isVisible()) &&
      (await page.getByText("fixture-spec").isVisible()) &&
      (await page.getByText("fixture-campaign").isVisible()),
  );
  check(
    "Core event history is rendered from its request-scoped API",
    await page.getByText("LeadRequestCreated").isVisible(),
  );
  check(
    "request identity is reloadable from URL",
    new URL(page.url()).hash.includes(`request=${REQUEST_ID}`),
    page.url(),
  );
  check(
    "ready authorized plan exposes explicit owner start action",
    await page
      .getByRole("button", { name: "Start authorized request" })
      .isVisible(),
  );

  await page.reload({ waitUntil: "domcontentloaded" });
  await page.getByText("fixture-plan-hash").waitFor();
  check(
    "reload retrieves the Core projection by URL request ID",
    apiCalls.includes(`GET /api/v1/lead/requests/${REQUEST_ID}`),
  );
  await page.getByRole("button", { name: "Start authorized request" }).click();
  await page.getByText("running", { exact: true }).waitFor();
  check(
    "owner start invokes the separate authenticated Campaign transition",
    apiCalls.includes(`POST /api/v1/lead/requests/${REQUEST_ID}/start`),
  );
  check(
    "fixture remained fully intercepted with no external runtime calls",
    apiCalls.every(
      (call) =>
        call.startsWith("GET /api/v1/projects") ||
        call.includes("/api/v1/lead/requests"),
    ),
    apiCalls.join(" | "),
  );
  console.log(JSON.stringify({ ok: true, checks }));
} catch (error) {
  console.log(JSON.stringify({ ok: false, checks, error: String(error) }));
  process.exitCode = 1;
} finally {
  if (closeBrowser) await closeBrowser();
}
