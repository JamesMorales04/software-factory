// Contract tests for the bounded subscription-only real-provider UAT harness.
//
// These tests do not contact OpenCode, a real Core, or any paid provider. They
// spawn the real driver against a local fake Core and prove:
//   1. live mode refuses before any network request without the explicit flag
//      and the complete subscription-only authorization envelope;
//   2. paid modes and spend caps are refused;
//   3. Extra Usage enabled, missing, stale or unverifiable evidence is refused;
//   4. the dry-run preflight is read-only and records no inference;
//   5. quota exhaustion is a durable PAUSED outcome that resumes without
//      re-approval, fallback or redispatch;
//   6. list-rate equivalents are separated from provider-reported actual charge;
//   7. the operator token and a credential-shaped response field never appear
//      in stdout, stderr or the evidence file.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import {
  chmodSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, before, test } from "node:test";

import {
  MODEL,
  PROVIDER,
  VERIFICATION_SCHEMA,
  readBillingSource,
  loadSubscriptionVerification,
  parseGate,
  readActualCharge,
  runFakeCoreSimulation,
  summarizeActualCharge,
} from "./real-provider-uat.mjs";

const DRIVER = new URL("./real-provider-uat.mjs", import.meta.url);
const TOKEN = "test-operator-token-0123456789abcdef0123456789abcdef";
const PROVIDER_CREDENTIAL = "provider-credential-marker-must-never-appear";
const TEMP_BASE = existsSync("/tmp/opencode") ? "/tmp/opencode" : tmpdir();
const ROOT = mkdtempSync(join(TEMP_BASE, "factory-rp-uat-test-"));
const PLAN_HASH = "f".repeat(64);
const COMMIT = "d".repeat(40);
const TREE = "e".repeat(40);

let verificationSequence = 0;

function cleanEnv(overrides) {
  const env = { ...process.env };
  for (const key of Object.keys(env)) {
    if (key.startsWith("FACTORY_")) delete env[key];
  }
  return { ...env, ...overrides };
}

function writeToken(mode = 0o600) {
  const path = join(ROOT, `operator-${mode.toString(8)}.token`);
  writeFileSync(path, `${TOKEN}\n`, { mode });
  chmodSync(path, mode);
  return path;
}

function validVerification(overrides = {}) {
  const now = Date.now();
  return {
    schema: VERIFICATION_SCHEMA,
    provider: PROVIDER,
    model: MODEL,
    route: "GO_SUBSCRIPTION_INCLUDED",
    plan: "Go",
    account_fingerprint: "sha256:test-owner-account",
    source: "OWNER_CONSOLE",
    verification_method: "OWNER_CONSOLE_INSPECTION",
    console_url: "https://opencode.ai/console",
    observed_at: new Date(now - 5 * 60_000).toISOString(),
    valid_until: new Date(now + 23 * 60 * 60_000).toISOString(),
    entitlement: "VERIFIED",
    extra_usage: "OFF",
    balance_fallback: "OFF",
    no_charge_route: "VERIFIED",
    quota: {
      state: "AVAILABLE",
      remaining_usd: null,
      resets_at: null,
    },
    core_verifiable: false,
    limitation:
      "CORE_AND_LOCAL_PREFLIGHT_CANNOT_VERIFY_CONSOLE_EXTRA_USAGE",
    rate_revision: "opencode-go-doc-2026-10-29",
    notes: "test fixture only; not a live Console observation",
    ...overrides,
  };
}

function writeVerification(overrides = {}) {
  verificationSequence += 1;
  const path = join(ROOT, `subscription-evidence-${verificationSequence}.json`);
  writeFileSync(path, `${JSON.stringify(validVerification(overrides), null, 2)}\n`, {
    mode: 0o600,
  });
  chmodSync(path, 0o600);
  return path;
}

function baseGate(port, overrides = {}) {
  return {
    FACTORY_REAL_PROVIDER_OWNER_GO_AHEAD: "1",
    FACTORY_REAL_PROVIDER_AUTHORIZATION: "test-owner-authorization",
    FACTORY_REAL_PROVIDER_ACCOUNTING: "GO_SUBSCRIPTION_INCLUDED",
    FACTORY_REAL_PROVIDER_EXTRA_USAGE: "DISABLED",
    FACTORY_REAL_PROVIDER_SUBSCRIPTION_EVIDENCE_FILE: writeVerification(),
    FACTORY_CORE_API_URL: `http://127.0.0.1:${port}/core-api`,
    FACTORY_REAL_PROVIDER_EVIDENCE_DIR: join(ROOT, "evidence"),
    FACTORY_OPERATOR_TOKEN_FILE: writeToken(),
    ...overrides,
  };
}

function runDriver(args, env, { simulation = false } = {}) {
  if (!simulation) {
    return spawnAndCollect(process.execPath, [DRIVER.pathname, ...args], env);
  }
  const source = `import { readFileSync } from "node:fs";
import { runFakeCoreSimulation } from ${JSON.stringify(DRIVER.href)};
const token = readFileSync(process.env.FACTORY_OPERATOR_TOKEN_FILE, "utf8").trim();
const calls = [];
const base = process.env.FACTORY_CORE_API_URL.replace(/\\/+$/, "");
const client = {
  simulationTransport: true,
  calls,
  secrets: new Set([token]),
  async request(method, path, body, options = {}) {
    let response;
    try {
      response = await fetch(base + path, {
        method,
        headers: { authorization: "Bearer " + token, ...(body === undefined ? {} : { "content-type": "application/json" }) },
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      });
    } catch (error) {
      throw Object.assign(new Error("fake Core transport failed"), { code: options.ambiguousCode ?? "CORE_TRANSPORT" });
    }
    const text = await response.text();
    let payload = null;
    if (text) { try { payload = JSON.parse(text); } catch { payload = text; } }
    calls.push({ method, path, status: response.status });
    if (!response.ok && !(options.allowStatuses ?? []).includes(response.status)) {
      const code = payload && typeof payload === "object" ? payload.code ?? payload.error : null;
      throw Object.assign(new Error("fake Core response"), { code: typeof code === "string" ? code : options.statusCode ?? "CORE_HTTP" });
    }
    return { status: response.status, payload };
  },
  get(path, options) { return this.request("GET", path, undefined, options); },
  post(path, body, options) { return this.request("POST", path, body, options); },
  put(path, body, options) { return this.request("PUT", path, body, options); },
};
runFakeCoreSimulation(${JSON.stringify(args)}, process.env, client).catch((error) => { const code = error?.code ?? "UNEXPECTED"; process.stderr.write("FAKE_CORE_SIMULATION=REFUSED code=" + code + " " + String(error?.message ?? error) + "\\n"); process.exitCode = error?.exitCode ?? 1; });`;
  return spawnAndCollect(process.execPath, ["--input-type=module", "-e", source], env);
}

function runSimulation(args, env) {
  return runDriver(args, env, { simulation: true });
}

function runMainWithSimulationOption(args, env) {
  const source = `import { main } from ${JSON.stringify(DRIVER.href)};\nmain(${JSON.stringify(args)}, process.env, { simulation: true }).catch((error) => { const code = error?.code ?? "UNEXPECTED"; process.stderr.write("REAL_PROVIDER_UAT=REFUSED code=" + code + " " + String(error?.message ?? error) + "\\n"); process.exitCode = error?.exitCode ?? 1; });`;
  return spawnAndCollect(process.execPath, ["--input-type=module", "-e", source], env);
}

function runWrapper(args, env) {
  return spawnAndCollect(
    "bash",
    [new URL("./real-provider-uat.sh", import.meta.url).pathname, ...args],
    env,
  );
}

function spawnAndCollect(command, args, env) {
  return new Promise((resolvePromise, rejectPromise) => {
    const child = spawn(command, args, {
      env: cleanEnv(env),
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      rejectPromise(new Error(`process timed out; stdout=${stdout} stderr=${stderr}`));
    }, 30_000);
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", (error) => {
      clearTimeout(timer);
      rejectPromise(error);
    });
    child.on("close", (status) => {
      clearTimeout(timer);
      resolvePromise({ status, stdout, stderr });
    });
  });
}

function assertRefused(result, code) {
  assert.notEqual(result.status, 0, `driver unexpectedly succeeded: ${result.stdout}`);
  assert.match(result.stderr, new RegExp(`code=${code}`), result.stderr);
  assert.doesNotMatch(result.stderr, new RegExp(TOKEN));
  assert.doesNotMatch(result.stdout, new RegExp(TOKEN));
}

let server;
let port;
let requests;
let healthMode = "ok";
let providerMaxTurns = 4;

const flow = {
  assessed: false,
  approved: false,
  quotaPauseAfterApproval: false,
  quotaRecovered: false,
  actualChargeKnown: true,
  actualChargeMicros: 0,
  billingSource: "go",
  approveCalls: 0,
  requestCalls: 0,
  resumeCalls: 0,
  projectCalls: 0,
  profileCalls: 0,
  defaultCalls: 0,
  policyCalls: 0,
  assessmentCalls: 0,
  providerCalls: 0,
  intakeBody: null,
  approveBody: null,
  resumeBody: null,
  githubWrite: false,
  admissionRejected: false,
  coreSubscriptionUnavailable: false,
};

function resetFlow(overrides = {}) {
  Object.assign(flow, {
    assessed: false,
    approved: false,
    quotaPauseAfterApproval: false,
    quotaRecovered: false,
    actualChargeKnown: true,
    actualChargeMicros: 0,
    billingSource: "go",
    approveCalls: 0,
    requestCalls: 0,
    resumeCalls: 0,
    projectCalls: 0,
    profileCalls: 0,
    defaultCalls: 0,
    policyCalls: 0,
    assessmentCalls: 0,
    providerCalls: 0,
    intakeBody: null,
    approveBody: null,
    resumeBody: null,
    githubWrite: false,
    admissionRejected: false,
    coreSubscriptionUnavailable: false,
    ...overrides,
  });
}

function flowProposal() {
  return {
    project_id: "project-1",
    action_id: "action-1",
    base_revision: COMMIT,
    project_config_hash: "b".repeat(64),
    objective:
      "Fix apply_discount in src/pricing.rs so the discount is applied exactly once, and add tests for zero, fractional, full and maximum-subtotal discounts.",
    class: "change",
    assumptions: [],
    risks: [],
    acceptance_criteria: [
      {
        id: "test",
        kind: "test",
        description: "cargo test",
        command: ["cargo", "test", "--quiet"],
        expectExit: 0,
        timeoutSecs: 120,
      },
    ],
    negative_cases: [
      {
        id: "negative",
        kind: "negative",
        description: "a double discount is rejected",
        command: ["grep", "-q", "double", "src/pricing.rs"],
        expectExit: 1,
        timeoutSecs: 30,
      },
    ],
    quality_profile: "generic",
    risk_class: "R0",
    budget: {
      maxInputTokens: 60000,
      maxOutputTokens: 12000,
      maxCostMicros: 2000000,
      maxWallClockSecs: 5400,
    },
    route: { provider: PROVIDER, model: MODEL, mode: "build" },
    max_repair_cycles: 0,
    max_wall_clock_secs: 5400,
    max_parallel: 1,
    max_sessions: 1,
    work_units: [
      {
        id: "unit-1",
        objective: "Fix the discount and add boundary tests.",
        depends_on: [],
        allowed_paths: ["src", "tests"],
      },
    ],
  };
}

function flowIntake() {
  const paused = flow.quotaPauseAfterApproval && !flow.quotaRecovered;
  const state = flow.approved
    ? paused
      ? "PAUSED"
      : "COMPLETED"
    : flow.assessed
      ? "AWAITING_APPROVAL"
      : "ASSESSING";
  return {
    id: "intake-1",
    projectId: "project-1",
    objective: flowProposal().objective,
    mode: "REVIEW_PLAN",
    state,
    stateDetail: paused
      ? "SUBSCRIPTION_QUOTA_EXHAUSTED"
      : flow.approved
        ? "campaign accepted"
        : "assessment complete",
    pauseReason: paused ? "SUBSCRIPTION_QUOTA_EXHAUSTED" : null,
    profile: {
      id: "profile-1",
      revisionId: "rev-1",
      revision: 1,
      name: "Real provider UAT test",
      contentHash: "c".repeat(64),
    },
    actionId: "action-1",
    delegationId: "delegation-1",
    leadRequestId: "lead-1",
    campaignId: flow.approved ? "campaign-1" : null,
    plan: flow.assessed
      ? {
          id: "plan-1",
          request_id: "lead-1",
          proposal_id: "proposal-1",
          plan_hash: PLAN_HASH,
          config_hash: "b".repeat(64),
          delegation_hash: "1".repeat(64),
          created_at: "2026-10-29T15:00:00Z",
        }
      : null,
    proposal: flow.assessed ? flowProposal() : null,
    assessment: flow.assessed ? { class: "change", estimated_effort: "small" } : null,
    spending: null,
    campaignStatus: flow.approved ? "AUTHORIZED" : null,
    gateOutcome: flow.approved && !paused ? "ACCEPTED" : null,
    updatedAt: "2026-10-29T15:00:00Z",
  };
}

function flowCampaign() {
  const paused = flow.quotaPauseAfterApproval && !flow.quotaRecovered;
  return {
    campaign: { id: "campaign-1", action_id: "action-1", status: paused ? "PAUSED" : "AUTHORIZED" },
    authorization: {
      provider: PROVIDER,
      model: MODEL,
      riskClass: "R0",
      executionMode: "TRUSTED_LOCAL_PROVIDER",
    },
    inference: null,
    tasks: paused
      ? []
      : [
          {
            task_id: "task-1",
            campaign_id: "campaign-1",
            request_id: "lead-1",
            unit_id: "unit-1",
            sequence: 1,
            status: "SUCCEEDED",
            base_commit: COMMIT,
            depends_on: [],
            run_id: "run-1",
            attempt_id: "attempt-1",
            candidate_id: "candidate-1",
            candidate_commit: COMMIT,
          },
        ],
    runs: paused ? [] : [{ id: "run-1", campaign_id: "campaign-1" }],
    attempts: paused ? [] : [{ id: "attempt-1", run_id: "run-1" }],
    candidates: paused
      ? []
      : [
          {
            id: "candidate-1",
            attempt_id: "attempt-1",
            commit_sha: COMMIT,
            tree_sha: TREE,
            implementer_id: "compose-worker",
          },
        ],
    events: [],
  };
}

function actualChargeFields() {
  return {
    actual_charge_micros: flow.actualChargeKnown ? flow.actualChargeMicros : null,
    actual_charge_known: flow.actualChargeKnown,
    billing_source: flow.billingSource,
  };
}

function flowLead() {
  return {
    request: { id: "lead-1", project_id: "project-1" },
    assessment: { id: "assessment-1", request_id: "lead-1", mechanism: "MODEL" },
    model_calls: [
      {
        reservation_id: "lead-reservation",
        role: "LEAD",
        principal_id: "operator-token",
        project_id: "project-1",
        objective_id: "lead-1",
        scope_id: "lead-1",
        provider: PROVIDER,
        model: MODEL,
        purpose: "lead_assessment",
        request_sha256: "a".repeat(64),
        input_tokens: 1000,
        output_tokens: 100,
        total_tokens: 1100,
        cost_micros: 150,
        max_cost_micros: 300000,
        usage_known: true,
        state: "SETTLED",
        error: "",
        created_at: "2026-10-29T15:00:00Z",
        updated_at: "2026-10-29T15:00:00Z",
        ...actualChargeFields(),
      },
    ],
  };
}

function flowReviewerCall() {
  return {
    reservation_id: "review-reservation",
    role: "SEMANTIC_REVIEWER",
    principal_id: "reviewer-1",
    project_id: "project-1",
    objective_id: "candidate-1",
    scope_id: "candidate-1",
    provider: PROVIDER,
    model: MODEL,
    purpose: "semantic_review",
    request_sha256: "2".repeat(64),
    input_tokens: 500,
    output_tokens: 50,
    total_tokens: 550,
    cost_micros: 75,
    max_cost_micros: 1000000,
    usage_known: true,
    state: "SETTLED",
    error: "",
    created_at: "2026-10-29T15:00:00Z",
    updated_at: "2026-10-29T15:00:00Z",
    ...actualChargeFields(),
  };
}

function flowProviderSession() {
  return {
    session_record_id: "session-1",
    attempt_id: "attempt-1",
    worker_id: "compose-worker",
    generation: 1,
    provider: PROVIDER,
    model: MODEL,
    provider_mode: "build",
    state: "COMPLETED",
    provider_session_id: "ses_real_123",
    provider_directory: "/tmp/provider-session",
    turn_id: "msg-1",
    input_tokens: 100,
    output_tokens: 50,
    total_tokens: 150,
    cost_micros: 1914,
    usage_known: true,
    cleanup_receipt: "session_delete_verified_absent",
    error: "",
    created_at: "2026-10-29T15:00:00Z",
    updated_at: "2026-10-29T15:00:00Z",
    ...actualChargeFields(),
  };
}

function flowReceipts() {
  return [
    {
      receipt_id: "verifier-1",
      candidate_id: "candidate-1",
      role: "verifier",
      principal_id: "verifier-1",
      action_spec_sha256: "3".repeat(64),
      commit_sha: COMMIT,
      tree_sha: TREE,
      result: "PASS",
      evidence: "EXECUTED",
      findings: [],
      mechanism: "DETERMINISTIC",
      model_provider: "",
      model_name: "",
      model_reservation_id: "",
      model_request_sha256: "",
      input_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
      cost_micros: 0,
      usage_known: false,
      received_at: "2026-10-29T15:00:00Z",
    },
    {
      receipt_id: "reviewer-1",
      candidate_id: "candidate-1",
      role: "reviewer",
      principal_id: "reviewer-1",
      action_spec_sha256: "3".repeat(64),
      commit_sha: COMMIT,
      tree_sha: TREE,
      result: "PASS",
      evidence: "EXECUTED",
      findings: [],
      mechanism: "MODEL",
      model_provider: PROVIDER,
      model_name: MODEL,
      model_reservation_id: "review-reservation",
      model_request_sha256: "2".repeat(64),
      input_tokens: 500,
      output_tokens: 50,
      total_tokens: 550,
      cost_micros: 75,
      usage_known: true,
      received_at: "2026-10-29T15:00:00Z",
    },
  ];
}

function readBody(request) {
  return new Promise((resolvePromise) => {
    let data = "";
    request.on("data", (chunk) => {
      data += chunk;
    });
    request.on("end", () => {
      if (!data) {
        resolvePromise(null);
        return;
      }
      try {
        resolvePromise(JSON.parse(data));
      } catch {
        resolvePromise(data);
      }
    });
  });
}

before(async () => {
  server = createServer(async (request, response) => {
    const body = await readBody(request);
    requests.push({
      method: request.method,
      url: request.url,
      authorization: request.headers.authorization ?? "",
      body,
    });
    const send = (status, value) => {
      response.writeHead(status, { "content-type": "application/json" });
      response.end(JSON.stringify(value));
    };
    const path = request.url.replace(/^\/core-api/, "").split("?")[0];
    if (request.method !== "GET" && /provider|models|inference|turn/i.test(path)) {
      flow.providerCalls += 1;
    }
    if (path === "/health") {
      if (healthMode === "echo-token") {
        send(401, {
          error: `rejected ${request.headers.authorization}`,
          credential: PROVIDER_CREDENTIAL,
        });
        return;
      }
      if (healthMode === "error-message-secret") {
        send(502, { error: `upstream rejected ${PROVIDER_CREDENTIAL}` });
        return;
      }
      if (healthMode === "error-plain-secret") {
        response.writeHead(502, { "content-type": "text/plain" });
        response.end(`upstream rejected ${PROVIDER_CREDENTIAL}`);
        return;
      }
      send(200, { healthy: true, version: "0.1.0" });
      return;
    }
    if (path === "/setup/status") {
      send(200, { required: false });
      return;
    }
    if (path === `/funding/subscription/${PROVIDER}` && request.method === "GET") {
      if (flow.coreSubscriptionUnavailable) {
        send(404, { error: "not found" });
        return;
      }
      const record = validVerification();
      send(200, {
        provider: PROVIDER,
        model: MODEL,
        route: "GO_SUBSCRIPTION_INCLUDED",
        plan: record.plan,
        account_fingerprint: record.account_fingerprint,
        entitlement: "VERIFIED",
        extra_usage: "OFF",
        balance_fallback: "OFF",
        no_charge_route: "VERIFIED",
        quota: record.quota,
        observed_at: record.observed_at,
        valid_until: record.valid_until,
        core_verifiable: true,
      });
      return;
    }
    if (path === "/settings/providers") {
      send(200, {
        providers: [
          {
            provider: PROVIDER,
            configured: true,
            enabled: true,
            isDefault: true,
            model: MODEL,
            baseUrl: "https://opencode.ai/inference/go/openai/v1",
            maxTurns: providerMaxTurns,
            maxInputTokens: 60000,
            maxOutputTokens: 12000,
            maxCostMicros: 2000000,
            inputMicrosPerMillion: 150000,
            outputMicrosPerMillion: 600000,
            revision: 7,
            updatedAt: "2026-10-29T15:00:00Z",
            credential: PROVIDER_CREDENTIAL,
          },
        ],
      });
      return;
    }
    if (path === `/settings/providers/${PROVIDER}/models`) {
      send(200, {
        provider: PROVIDER,
        status: "AVAILABLE",
        source: "OPENCODE_GO_CATALOG_URL",
        catalogUrl: "https://opencode.ai/inference/go/v1/models",
        models: [MODEL],
        model: MODEL,
        credentialSignal: "NOT_OBSERVED",
        detail: "1 model(s) discovered",
        discoveredAt: "2026-10-29T15:00:00Z",
        lastKnown: null,
        credential: PROVIDER_CREDENTIAL,
      });
      return;
    }
    if (path === "/settings/runtime") {
      send(200, {
        semanticReviewEnabled: true,
        semanticReviewMaxOutputTokens: 8192,
        updatedAt: "2026-10-29T15:00:00Z",
      });
      return;
    }
    if (path === "/workers") {
      send(200, [{ worker_id: "compose-worker", updated_at: "2026-10-29T15:00:00Z" }]);
      return;
    }
    if (path === "/projects" && request.method === "POST") {
      flow.projectCalls += 1;
      send(201, { id: "project-1", repository: "http://git-fixture:8080/repo.git" });
      return;
    }
    if (path === "/projects/project-1/config" && request.method === "GET") {
      send(200, {
        revision: 1,
        baseCommit: COMMIT,
        configHash: "b".repeat(64),
        allowedPaths: ["src", "tests"],
        active: true,
      });
      return;
    }
    if (path === "/projects/project-1/config" && request.method === "PUT") {
      send(200, {
        revision: 2,
        baseCommit: COMMIT,
        configHash: "b".repeat(64),
        allowedPaths: ["src", "tests"],
        capabilities: ["repository-edit", "generic-toolchain"],
        qualityProfile: "generic",
        active: true,
      });
      return;
    }
    if (path === "/execution-profiles" && request.method === "POST") {
      flow.profileCalls += 1;
      send(201, {
        id: "profile-1",
        revision: 1,
        revisionId: "rev-1",
        contentHash: "c".repeat(64),
        executable: true,
      });
      return;
    }
    if (path === "/execution-profiles/profile-1/default" && request.method === "POST") {
      flow.defaultCalls += 1;
      send(200, {});
      return;
    }
    if (path === "/projects/project-1/execution-policy" && request.method === "POST") {
      flow.policyCalls += 1;
      send(201, { id: "policy-1" });
      return;
    }
    if (path === "/intake/requests" && request.method === "POST") {
      flow.requestCalls += 1;
      flow.intakeBody = body;
      if (flow.admissionRejected) {
        send(409, { error: "INFERENCE_AUTHORIZATION_REQUIRED" });
        return;
      }
      send(201, {
        id: "intake-1",
        actionId: "action-1",
        leadRequestId: "lead-1",
        state: "SUBMITTED",
        mode: "REVIEW_PLAN",
        objective: flowProposal().objective,
      });
      return;
    }
    if (path === "/intake/requests/intake-1/assess" && request.method === "POST") {
      flow.assessmentCalls += 1;
      flow.assessed = true;
      send(200, flowIntake());
      return;
    }
    if (path === "/intake/requests/intake-1/approve" && request.method === "POST") {
      flow.approveCalls += 1;
      flow.approveBody = body;
      flow.approved = true;
      send(201, flowIntake());
      return;
    }
    if (path === "/intake/requests/intake-1/resume" && request.method === "POST") {
      flow.resumeCalls += 1;
      flow.resumeBody = body;
      flow.quotaRecovered = true;
      send(200, { ...flowIntake(), state: "EXECUTING" });
      return;
    }
    if (path === "/intake/requests/intake-1" && request.method === "GET") {
      send(200, flowIntake());
      return;
    }
    if (path === "/campaigns/campaign-1" && request.method === "GET") {
      send(200, flowCampaign());
      return;
    }
    if (path === "/quality/candidates/candidate-1/gate" && request.method === "GET") {
      send(200, { outcome: "ACCEPTED", reasons: [], candidateId: "candidate-1" });
      return;
    }
    if (path === "/quality/candidates/candidate-1/receipts" && request.method === "GET") {
      send(200, flowReceipts());
      return;
    }
    if (path === "/office/participants" && request.method === "GET") {
      send(200, {
        participants: [
          {
            role: "IMPLEMENTER",
            attempt_generation: 1,
            session: {
              state: "COMPLETED",
              worker_id: "compose-worker",
              provider_session_id: "ses_real_123",
              cleanup_receipt: "session_delete_verified_absent",
              input_tokens: 100,
              output_tokens: 50,
              cost_micros: 1914,
            },
          },
        ],
      });
      return;
    }
    if (path === "/provider/sessions" && request.method === "GET") {
      send(200, [flowProviderSession()]);
      return;
    }
    if (path === "/lead/requests/lead-1" && request.method === "GET") {
      send(200, flowLead());
      return;
    }
    if (path === "/model/calls" && request.method === "GET") {
      send(200, [flowReviewerCall()]);
      return;
    }
    if (
      path === "/campaigns/campaign-1/candidates/candidate-1/diff" &&
      request.method === "GET"
    ) {
      send(200, {
        changed_paths: ["src/pricing.rs", "tests/discounts.rs"],
        base_commit: COMMIT,
        candidate_commit: COMMIT,
      });
      return;
    }
    if (path === "/projects/project-1/github" && request.method === "GET") {
      send(200, { binding: null, sync_state: null });
      return;
    }
    if (path.includes("pull_request") || path.includes("pull-request")) {
      flow.githubWrite = true;
      send(500, { error: "the harness must never write a GitHub PR" });
      return;
    }
    send(404, { error: `unexpected ${request.method} ${path}` });
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  port = server.address().port;
});

after(() => {
  server.close();
  rmSync(ROOT, { recursive: true, force: true });
});

test("live mode refuses without FACTORY_REAL_PROVIDER_UAT=1 before any request", async () => {
  requests = [];
  const result = await runDriver(["assess"], baseGate(port));
  assertRefused(result, "LIVE_FLAG_MISSING");
  assert.equal(requests.length, 0, "the driver contacted Core before the live flag was present");
});

test("live mode refuses without the separate Owner go-ahead", async () => {
  requests = [];
  const env = baseGate(port);
  env.FACTORY_REAL_PROVIDER_UAT = "1";
  delete env.FACTORY_REAL_PROVIDER_OWNER_GO_AHEAD;
  const result = await runDriver(["assess"], env);
  assertRefused(result, "OWNER_GO_AHEAD_MISSING");
  assert.equal(requests.length, 0);
});

test("the shell entrypoint refuses a live phase before touching the stack", async () => {
  requests = [];
  const result = await runWrapper(["assess"], {});
  assert.equal(result.status, 3, result.stderr);
  assert.match(result.stderr, /FACTORY_REAL_PROVIDER_UAT=1 is required/);
  assert.equal(requests.length, 0);
});

test("live mode refuses without the authorization id before any request", async () => {
  requests = [];
  const env = baseGate(port);
  env.FACTORY_REAL_PROVIDER_UAT = "1";
  delete env.FACTORY_REAL_PROVIDER_AUTHORIZATION;
  const result = await runDriver(["assess"], env);
  assertRefused(result, "ENV_REQUIRED");
  assert.equal(requests.length, 0, "the driver contacted Core without an authorization id");
});

test("billable and pay-as-you-go modes are refused before any request", async () => {
  requests = [];
  for (const mode of ["GO_EXTRA_USAGE_BILLABLE", "PAY_AS_YOU_GO_API"]) {
    const env = baseGate(port, {
      FACTORY_REAL_PROVIDER_UAT: "1",
      FACTORY_REAL_PROVIDER_ACCOUNTING: mode,
      FACTORY_REAL_PROVIDER_EXTRA_USAGE:
        mode === "PAY_AS_YOU_GO_API" ? "NOT_APPLICABLE" : "ENABLED_WITHIN_CAP",
      FACTORY_REAL_PROVIDER_SPEND_CAP_USD: "2.00",
    });
    const result = await runDriver(["assess"], env);
    assertRefused(result, "PAID_MODE_DISABLED");
  }
  assert.equal(requests.length, 0);
});

test("a spend cap is not accepted in subscription-only mode", async () => {
  requests = [];
  const env = baseGate(port, {
    FACTORY_REAL_PROVIDER_UAT: "1",
    FACTORY_REAL_PROVIDER_SPEND_CAP_USD: "2.00",
  });
  const result = await runDriver(["assess"], env);
  assertRefused(result, "SPEND_CAP_NOT_APPLICABLE");
  assert.equal(requests.length, 0);
});

test("Extra Usage enabled or missing is refused before any request", async () => {
  requests = [];
  const enabled = baseGate(port, {
    FACTORY_REAL_PROVIDER_UAT: "1",
    FACTORY_REAL_PROVIDER_EXTRA_USAGE: "ENABLED_WITHIN_CAP",
  });
  assertRefused(await runDriver(["assess"], enabled), "EXTRA_USAGE_NOT_DISABLED");
  const missing = baseGate(port, { FACTORY_REAL_PROVIDER_UAT: "1" });
  delete missing.FACTORY_REAL_PROVIDER_EXTRA_USAGE;
  assertRefused(await runDriver(["assess"], missing), "ENV_REQUIRED");
  assert.equal(requests.length, 0);
});

test("missing subscription evidence is refused before any request", async () => {
  requests = [];
  const env = baseGate(port, {
    FACTORY_REAL_PROVIDER_UAT: "1",
    FACTORY_REAL_PROVIDER_SUBSCRIPTION_EVIDENCE_FILE: join(ROOT, "missing-evidence.json"),
  });
  const result = await runDriver(["assess"], env);
  assertRefused(result, "SUBSCRIPTION_VERIFICATION_MISSING");
  assert.equal(requests.length, 0);
});

test("stale subscription evidence is refused before any request", async () => {
  requests = [];
  const now = Date.now();
  const file = writeVerification({
    observed_at: new Date(now - 48 * 60 * 60_000).toISOString(),
    valid_until: new Date(now - 24 * 60 * 60_000).toISOString(),
  });
  const env = baseGate(port, {
    FACTORY_REAL_PROVIDER_UAT: "1",
    FACTORY_REAL_PROVIDER_SUBSCRIPTION_EVIDENCE_FILE: file,
  });
  const result = await runDriver(["assess"], env);
  assertRefused(result, "SUBSCRIPTION_VERIFICATION_STALE");
  assert.equal(requests.length, 0);
});

test("Extra Usage ON or unknown entitlement in evidence is refused", async () => {
  requests = [];
  for (const overrides of [
    { extra_usage: "ON" },
    { balance_fallback: "ON" },
    { entitlement: "UNKNOWN" },
    { no_charge_route: "UNKNOWN" },
  ]) {
    const env = baseGate(port, {
      FACTORY_REAL_PROVIDER_UAT: "1",
      FACTORY_REAL_PROVIDER_SUBSCRIPTION_EVIDENCE_FILE: writeVerification(overrides),
    });
    const result = await runDriver(["assess"], env);
    assertRefused(result, "SUBSCRIPTION_ENTITLEMENT_UNVERIFIED");
  }
  assert.equal(requests.length, 0);
});

test("execute refuses without the exact Owner plan approval attestation", async () => {
  requests = [];
  const env = baseGate(port, { FACTORY_REAL_PROVIDER_UAT: "1" });
  const result = await runDriver(["execute", "--plan-hash", "a".repeat(64)], env);
  assertRefused(result, "ENV_REQUIRED");
  assert.equal(requests.length, 0);
});

test("exhausted subscription quota pauses before any request", async () => {
  requests = [];
  const env = baseGate(port, {
    FACTORY_REAL_PROVIDER_UAT: "1",
    FACTORY_REAL_PROVIDER_SUBSCRIPTION_EVIDENCE_FILE: writeVerification({
      quota: { state: "EXHAUSTED", remaining_usd: 0, resets_at: null },
    }),
  });
  const result = await runDriver(["assess"], env);
  assert.equal(result.status, 4, result.stderr);
  assert.match(result.stdout, /PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED/);
  assert.equal(requests.length, 0);
  const paused = JSON.parse(
    readFileSync(join(env.FACTORY_REAL_PROVIDER_EVIDENCE_DIR, "real-provider-uat-paused.json"), "utf8"),
  );
  assert.equal(paused.result, "PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED");
  assert.equal(paused.recoverable, true);
  assert.equal(paused.pre_dispatch, true);
  assert.equal(paused.subscription.quota.state, "EXHAUSTED");
  assert.equal(paused.subscription.extra_usage, "OFF");
});

test("dry-run preflight is read-only and records no inference", async () => {
  requests = [];
  healthMode = "ok";
  const env = baseGate(port, { FACTORY_REAL_PROVIDER_DRY_RUN: "1" });
  const result = await runDriver(["preflight"], env);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(requests.length, 7);
  assert.ok(
    requests.every((request) => request.method === "GET"),
    `dry-run made a non-GET request: ${JSON.stringify(requests)}`,
  );
  const evidencePath = join(env.FACTORY_REAL_PROVIDER_EVIDENCE_DIR, "real-provider-uat-preflight.json");
  const evidence = JSON.parse(readFileSync(evidencePath, "utf8"));
  assert.equal(evidence.result, "DRY_RUN_READY");
  assert.equal(evidence.uat_status, "BLOCKED");
  assert.equal(evidence.preflight.request_admission.status, "NOT_VERIFIED");
  assert.equal(
    evidence.preflight.request_admission.reason,
    "CORE_HAS_NO_READ_ONLY_ATOMIC_REQUEST_ADMISSION_CONTRACT",
  );
  assert.equal(evidence.paid_inference, false);
  assert.equal(evidence.mutations, false);
  assert.equal(evidence.preflight.subscription.positive_usd_authorization, "NOT_REQUIRED");
  assert.equal(evidence.preflight.subscription.spend_cap, "NOT_USED");
  assert.equal(evidence.preflight.subscription.verification.extra_usage, "OFF");
  assert.equal(evidence.preflight.subscription.verification.core_verifiable, false);
  assert.match(evidence.preflight.subscription.verification.limitation, /CANNOT_VERIFY/);
  assert.equal(evidence.preflight.subscription.core_verification, "AVAILABLE_V3_SUBSCRIPTION_BINDING");
  assert.equal(evidence.preflight.subscription.core_subscription.core_verifiable, true);
  const serialized = JSON.stringify(evidence);
  assert.doesNotMatch(serialized, new RegExp(TOKEN));
  assert.doesNotMatch(serialized, new RegExp(PROVIDER_CREDENTIAL));
  assert.doesNotMatch(result.stdout, new RegExp(TOKEN));
  assert.doesNotMatch(result.stderr, new RegExp(TOKEN));
  assert.equal(statSync(evidencePath).mode & 0o077, 0);
});

test("preflight refuses a provider turn budget above the authorization", async () => {
  requests = [];
  providerMaxTurns = 8;
  healthMode = "ok";
  const env = baseGate(port, { FACTORY_REAL_PROVIDER_DRY_RUN: "1" });
  const result = await runDriver(["preflight"], env);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /PROVIDER_TURN_CEILING_EXCEEDED/);
  providerMaxTurns = 4;
});

test("a Core error that echoes the operator token is redacted", async () => {
  requests = [];
  healthMode = "echo-token";
  try {
    const env = baseGate(port, { FACTORY_REAL_PROVIDER_DRY_RUN: "1" });
    const result = await runDriver(["preflight"], env);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /CORE_HTTP/);
    assert.doesNotMatch(result.stderr, /rejected/);
    assert.doesNotMatch(result.stderr, new RegExp(TOKEN));
    assert.doesNotMatch(result.stdout, new RegExp(TOKEN));
    assert.doesNotMatch(result.stderr, new RegExp(PROVIDER_CREDENTIAL));
  } finally {
    healthMode = "ok";
  }
});

test("Core errors omit arbitrary JSON and non-JSON response bodies", async () => {
  try {
    for (const mode of ["error-message-secret", "error-plain-secret"]) {
      requests = [];
      healthMode = mode;
      const env = baseGate(port, { FACTORY_REAL_PROVIDER_DRY_RUN: "1" });
      const result = await runDriver(["preflight"], env);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /CORE_HTTP/);
      assert.doesNotMatch(result.stderr, /upstream rejected/);
      assert.doesNotMatch(result.stdout, new RegExp(PROVIDER_CREDENTIAL));
      assert.doesNotMatch(result.stderr, new RegExp(PROVIDER_CREDENTIAL));
    }
  } finally {
    healthMode = "ok";
  }
});

test("operator token files with group/other access are refused", async () => {
  requests = [];
  const env = baseGate(port, {
    FACTORY_REAL_PROVIDER_DRY_RUN: "1",
    FACTORY_OPERATOR_TOKEN_FILE: writeToken(0o644),
  });
  const result = await runDriver(["preflight"], env);
  assertRefused(result, "OPERATOR_TOKEN_PERMISSIONS");
  assert.equal(requests.length, 0);
});

test("plain HTTP is refused outside loopback", async () => {
  const env = baseGate(port, {
    FACTORY_REAL_PROVIDER_DRY_RUN: "1",
    FACTORY_CORE_API_URL: "http://core.example:8899/core-api",
  });
  const result = await runDriver(["preflight"], env);
  assertRefused(result, "CORE_URL_INSECURE");
});

test("the gate binds subscription-only funding without a USD cap", () => {
  const env = baseGate(port, { FACTORY_REAL_PROVIDER_UAT: "1" });
  const gate = parseGate(env, ["assess"]);
  assert.equal(gate.fundingPolicy, "SUBSCRIPTION_INCLUDED_ONLY");
  assert.equal(gate.accounting, "GO_SUBSCRIPTION_INCLUDED");
  assert.equal(gate.extraUsage, "DISABLED");
  assert.equal(gate.capMicros, undefined);
  assert.equal(gate.capUsd, undefined);
  assert.equal(gate.provider, PROVIDER);
  assert.equal(gate.model, MODEL);
  assert.equal(gate.baseUrl, "https://opencode.ai/inference/go/openai/v1");
});

test("a Core without subscription verification fails closed before creating a Project", async () => {
  requests = [];
  resetFlow({ coreSubscriptionUnavailable: true });
  const env = baseGate(port, {
    FACTORY_REAL_PROVIDER_UAT: "1",
    FACTORY_REAL_PROVIDER_EVIDENCE_DIR: join(ROOT, "core-unavailable-evidence"),
    FACTORY_REAL_PROVIDER_TIMEOUT_SECS: "120",
  });
  const result = await runDriver(["assess"], env);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /CORE_V3_SUBSCRIPTION_ADMISSION_REQUIRED/);
  assert.equal(flow.projectCalls, 0);
  assert.equal(flow.requestCalls, 0);
  assert.ok(requests.every((request) => !request.url.endsWith("/projects")));
});

test("fake-Core simulation requires an explicit fake transport and cannot default to CoreClient", async () => {
  requests = [];
  const env = baseGate(port, {
    FACTORY_REAL_PROVIDER_UAT: "1",
    FACTORY_REAL_PROVIDER_EVIDENCE_DIR: join(ROOT, "simulation-needs-fake-transport"),
  });
  await assert.rejects(
    runFakeCoreSimulation(["assess"], env),
    (error) => error.code === "FAKE_CORE_TRANSPORT_REQUIRED",
  );
  assert.equal(requests.length, 0);
  assert.ok(!existsSync(env.FACTORY_REAL_PROVIDER_EVIDENCE_DIR));
});

test("live main ignores simulation options and blocks before resource or inference calls", async () => {
  requests = [];
  resetFlow();
  const env = baseGate(port, {
    FACTORY_REAL_PROVIDER_UAT: "1",
    FACTORY_REAL_PROVIDER_EVIDENCE_DIR: join(ROOT, "admission-rejected-evidence"),
    FACTORY_REAL_PROVIDER_TIMEOUT_SECS: "120",
  });
  const result = await runMainWithSimulationOption(["assess"], env);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /REQUEST_ADMISSION_NOT_VERIFIED/);
  assert.doesNotMatch(result.stdout, /FAKE_CORE_SIMULATION/);
  assert.doesNotMatch(result.stderr, /FAKE_CORE_SIMULATION/);
  assert.equal(flow.projectCalls, 0);
  assert.equal(flow.profileCalls, 0);
  assert.equal(flow.defaultCalls, 0);
  assert.equal(flow.policyCalls, 0);
  assert.equal(flow.requestCalls, 0);
  assert.equal(flow.assessmentCalls, 0);
  assert.equal(flow.approveCalls, 0);
  assert.equal(flow.providerCalls, 0);
  assert.ok(!existsSync(join(env.FACTORY_REAL_PROVIDER_EVIDENCE_DIR, "real-provider-uat-state.json")));
  assert.ok(!existsSync(join(env.FACTORY_REAL_PROVIDER_EVIDENCE_DIR, "plan.json")));
  assert.equal(flow.intakeBody, null);
  assert.ok(
    requests.every(
      (request) =>
        request.method === "GET" &&
        !request.url.endsWith("/projects") &&
        !request.url.endsWith("/execution-profiles") &&
        !request.url.endsWith("/intake/requests"),
    ),
  );
});

test("fake-Core simulation exercises downstream flow without runtime acceptance", async () => {
  requests = [];
  resetFlow({ actualChargeKnown: true, actualChargeMicros: 0, billingSource: "go" });
  healthMode = "ok";
  const env = baseGate(port, {
    FACTORY_REAL_PROVIDER_UAT: "1",
    FACTORY_REAL_PROVIDER_PLAN_APPROVAL: "OWNER_REVIEWED",
    FACTORY_REAL_PROVIDER_EVIDENCE_DIR: join(ROOT, "flow-evidence"),
    FACTORY_REAL_PROVIDER_TIMEOUT_SECS: "120",
    FACTORY_REAL_PROVIDER_CANDIDATE_SHA: "a".repeat(40),
    FACTORY_REAL_PROVIDER_COMPOSE_PROJECT: "software-factory-rp-uat-test",
  });

  const assessed = await runSimulation(["assess"], env);
  assert.equal(assessed.status, 0, assessed.stderr);
  assert.match(assessed.stdout, /FAKE_CORE_SIMULATION=SIMULATED_AWAITING_PLAN_APPROVAL/);
  assert.equal(flow.intakeBody.authorized_micros, undefined, "subscription request must not carry a USD authorization");
  const statePath = join(env.FACTORY_REAL_PROVIDER_EVIDENCE_DIR, "real-provider-uat-state.json");
  const planPath = join(env.FACTORY_REAL_PROVIDER_EVIDENCE_DIR, "plan.json");
  const state = JSON.parse(readFileSync(statePath, "utf8"));
  const plan = JSON.parse(readFileSync(planPath, "utf8"));
  assert.equal(state.schema, "factory.real-provider-uat.state.v2");
  assert.equal(state.evidence_kind, "FAKE_CORE_SIMULATION");
  assert.equal(state.request_admission, "NOT_VERIFIED");
  assert.equal(plan.evidence_kind, "FAKE_CORE_SIMULATION");
  assert.equal(plan.request_admission, "NOT_VERIFIED");
  assert.equal(state.funding.policy, "SUBSCRIPTION_INCLUDED_ONLY");
  assert.equal(state.funding.positive_usd_authorization, "NOT_REQUIRED");
  assert.equal(state.funding.spend_cap, "NOT_USED");
  assert.equal(state.plan_hash, PLAN_HASH);
  assert.equal(plan.budget_semantics, "MODEL_PLAN_OBSERVATION_NOT_A_SPEND_AUTHORIZATION");
  assert.equal(flow.approved, false, "assess must not approve the plan");

  const wrongHash = await runSimulation(["execute", "--plan-hash", "a".repeat(64)], env);
  assert.notEqual(wrongHash.status, 0);
  assert.match(wrongHash.stderr, /PLAN_HASH_MISMATCH/);
  assert.equal(flow.approved, false);

  const executed = await runSimulation(["execute", "--plan-hash", PLAN_HASH], env);
  assert.equal(executed.status, 0, executed.stderr);
  assert.match(executed.stdout, /FAKE_CORE_SIMULATION=SIMULATED_ACCEPTED/);
  assert.equal(flow.approveBody.authorized_micros, undefined, "approval must not carry a USD authorization");
  assert.equal(flow.githubWrite, false);
  assert.ok(requests.every((request) => !request.url.includes("pull_request")));

  const evidence = JSON.parse(
    readFileSync(
      join(env.FACTORY_REAL_PROVIDER_EVIDENCE_DIR, "real-provider-uat-evidence.json"),
      "utf8",
    ),
  );
  assert.equal(evidence.result, "SIMULATED_ACCEPTED");
  assert.equal(evidence.evidence_kind, "FAKE_CORE_SIMULATION");
  assert.equal(evidence.request_admission, "NOT_VERIFIED");
  assert.equal(evidence.boundary.real_core_http_api, false);
  assert.equal(evidence.boundary.real_compose_stack, false);
  assert.equal(evidence.boundary.live_provider_inference, "NOT_PERFORMED_FAKE_CORE");
  assert.doesNotMatch(executed.stdout, /REAL_PROVIDER_UAT=(?:ACCEPTED|AWAITING_PLAN_APPROVAL)/);
  assert.equal(evidence.funding.policy, "SUBSCRIPTION_INCLUDED_ONLY");
  assert.equal(evidence.funding.positive_usd_authorization, "NOT_REQUIRED");
  assert.equal(evidence.funding.spend_cap, "NOT_USED");
  assert.equal(evidence.funding.monetary_envelope, "ABSENT");
  assert.equal(evidence.funding.paid_mode, "PAID_MODE_DISABLED");
  assert.equal(evidence.subscription.entitlement, "VERIFIED");
  assert.equal(evidence.subscription.extra_usage, "OFF");
  assert.equal(evidence.subscription.balance_fallback, "OFF");
  assert.equal(evidence.subscription.quota.state, "AVAILABLE");
  assert.ok(
    evidence.usage.provider_billing_source_observations.some(
      (source) => source.observation === "SUBSCRIPTION_INCLUDED",
    ),
    "subscription route source must remain separate from actual charge",
  );
  assert.equal(evidence.subscription.core_verifiable, false);
  assert.match(evidence.subscription.limitation, /CANNOT_VERIFY/);
  assert.equal(evidence.usage.measured.input_tokens, 1600);
  assert.equal(evidence.usage.measured.output_tokens, 200);
  assert.ok(evidence.usage.list_rate_equivalent.micros > 0);
  assert.equal(evidence.usage.list_rate_equivalent.is_actual_charge, false);
  assert.equal(evidence.usage.list_rate_equivalent.is_spending_cap, false);
  assert.equal(evidence.usage.list_rate_equivalent.rate_revision.provider_settings_revision, 7);
  assert.equal(evidence.usage.provider_reported_actual_charge.observation, "VERIFIED_ZERO");
  assert.equal(evidence.usage.provider_reported_actual_charge.micros, 0);
  assert.equal(evidence.ids.candidate_commit, COMMIT);
  assert.equal(evidence.receipts.reviewer.mechanism, "MODEL");
  assert.equal(evidence.quality_gate.outcome, "ACCEPTED");
  assert.equal(evidence.pr_preparation.status, "UNBOUND_READY");
  assert.equal(evidence.pr_preparation.write_attempted, false);
  assert.equal(evidence.boundary.paid_fallback, "NONE");
  assert.equal(evidence.boundary.redispatch, "NONE");
  const serialized = JSON.stringify(evidence);
  assert.doesNotMatch(serialized, new RegExp(TOKEN));
  assert.doesNotMatch(serialized, new RegExp(PROVIDER_CREDENTIAL));
  assert.doesNotMatch(executed.stdout, new RegExp(TOKEN));
  assert.doesNotMatch(executed.stderr, new RegExp(TOKEN));
  assert.equal(statSync(join(env.FACTORY_REAL_PROVIDER_EVIDENCE_DIR, "real-provider-uat-evidence.json")).mode & 0o077, 0);
});

test("unknown provider-reported actual charge is recorded as UNKNOWN, never zero", async () => {
  requests = [];
  resetFlow({ actualChargeKnown: false, actualChargeMicros: null, billingSource: null });
  const env = baseGate(port, {
    FACTORY_REAL_PROVIDER_UAT: "1",
    FACTORY_REAL_PROVIDER_PLAN_APPROVAL: "OWNER_REVIEWED",
    FACTORY_REAL_PROVIDER_EVIDENCE_DIR: join(ROOT, "unknown-charge-evidence"),
    FACTORY_REAL_PROVIDER_TIMEOUT_SECS: "120",
  });
  const assessed = await runSimulation(["assess"], env);
  assert.equal(assessed.status, 0, assessed.stderr);
  const executed = await runSimulation(["execute", "--plan-hash", PLAN_HASH], env);
  assert.equal(executed.status, 0, executed.stderr);
  const evidence = JSON.parse(
    readFileSync(join(env.FACTORY_REAL_PROVIDER_EVIDENCE_DIR, "real-provider-uat-evidence.json"), "utf8"),
  );
  assert.equal(evidence.usage.provider_reported_actual_charge.observation, "UNKNOWN");
  assert.equal(evidence.usage.provider_reported_actual_charge.micros, null);
  assert.ok(evidence.usage.unknown.includes("provider_reported_actual_charge"));
  assert.ok(evidence.usage.list_rate_equivalent.micros > 0);
});

test("a positive provider-reported charge blocks a subscription run", async () => {
  requests = [];
  resetFlow({
    actualChargeKnown: true,
    actualChargeMicros: 1914,
    billingSource: "balance",
  });
  const env = baseGate(port, {
    FACTORY_REAL_PROVIDER_UAT: "1",
    FACTORY_REAL_PROVIDER_PLAN_APPROVAL: "OWNER_REVIEWED",
    FACTORY_REAL_PROVIDER_EVIDENCE_DIR: join(ROOT, "charge-incident-evidence"),
    FACTORY_REAL_PROVIDER_TIMEOUT_SECS: "120",
  });
  const assessed = await runSimulation(["assess"], env);
  assert.equal(assessed.status, 0, assessed.stderr);
  const executed = await runSimulation(["execute", "--plan-hash", PLAN_HASH], env);
  assert.notEqual(executed.status, 0);
  assert.match(executed.stderr, /SUBSCRIPTION_CHARGE_INCIDENT/);
  const evidence = JSON.parse(
    readFileSync(join(env.FACTORY_REAL_PROVIDER_EVIDENCE_DIR, "real-provider-uat-evidence.json"), "utf8"),
  );
  assert.equal(evidence.result, "SIMULATED_FAILED_SUBSCRIPTION_CHARGE_INCIDENT");
  assert.equal(evidence.evidence_kind, "FAKE_CORE_SIMULATION");
  assert.equal(evidence.request_admission, "NOT_VERIFIED");
  assert.equal(evidence.usage.provider_reported_actual_charge.observation, "VERIFIED_POSITIVE");
  assert.ok(evidence.usage.provider_reported_actual_charge.fallback_observed);
  assert.equal(evidence.boundary.paid_fallback, "NONE");
});

test("quota exhaustion pauses and resumes without approval, fallback or redispatch", async () => {
  requests = [];
  resetFlow({ quotaPauseAfterApproval: true });
  const env = baseGate(port, {
    FACTORY_REAL_PROVIDER_UAT: "1",
    FACTORY_REAL_PROVIDER_PLAN_APPROVAL: "OWNER_REVIEWED",
    FACTORY_REAL_PROVIDER_EVIDENCE_DIR: join(ROOT, "quota-resume-evidence"),
    FACTORY_REAL_PROVIDER_TIMEOUT_SECS: "120",
  });
  const assessed = await runSimulation(["assess"], env);
  assert.equal(assessed.status, 0, assessed.stderr);
  assert.equal(flow.approveCalls, 0);
  assert.equal(flow.requestCalls, 1);

  const executed = await runSimulation(["execute", "--plan-hash", PLAN_HASH], env);
  assert.equal(executed.status, 4, executed.stderr);
  assert.match(executed.stdout, /PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED/);
  assert.equal(flow.approveCalls, 1);
  assert.equal(flow.requestCalls, 1);
  assert.equal(flow.resumeCalls, 0);
  const statePath = join(env.FACTORY_REAL_PROVIDER_EVIDENCE_DIR, "real-provider-uat-state.json");
  const pausedState = JSON.parse(readFileSync(statePath, "utf8"));
  assert.equal(pausedState.phase, "PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED");
  assert.equal(pausedState.resume_stage, "EXECUTION");
  assert.equal(pausedState.campaign_id, "campaign-1");
  assert.equal(pausedState.subscription.provider, PROVIDER);
  assert.equal(pausedState.subscription.model, MODEL);
  assert.equal(pausedState.funding.policy, "SUBSCRIPTION_INCLUDED_ONLY");
  assert.equal(pausedState.funding.positive_usd_authorization, "NOT_REQUIRED");
  const paused = JSON.parse(
    readFileSync(join(env.FACTORY_REAL_PROVIDER_EVIDENCE_DIR, "real-provider-uat-paused.json"), "utf8"),
  );
  assert.equal(paused.result, "SIMULATED_PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED");
  assert.equal(paused.evidence_kind, "FAKE_CORE_SIMULATION");
  assert.equal(paused.request_admission, "NOT_VERIFIED");
  assert.equal(paused.recoverable, true);
  assert.equal(paused.fallback, "NONE");
  assert.equal(paused.redispatch, "NONE");
  assert.equal(paused.state.provider, undefined);
  assert.equal(flow.githubWrite, false);

  const resumed = await runSimulation(["resume"], env);
  assert.equal(resumed.status, 0, resumed.stderr);
  assert.match(resumed.stdout, /FAKE_CORE_SIMULATION=SIMULATED_ACCEPTED/);
  assert.equal(flow.resumeCalls, 1);
  assert.equal(flow.approveCalls, 1, "resume must not re-approve the plan");
  assert.equal(flow.requestCalls, 1, "resume must not create another request");
  assert.equal(flow.projectCalls, 1, "resume must not create another project");
  assert.equal(flow.resumeBody.expected_plan_hash, PLAN_HASH);
  assert.equal(flow.resumeBody.subscription_evidence_sha256.length, 64);
  assert.equal(flow.resumeBody.expected_campaign_id, "campaign-1");
  assert.ok(
    requests.filter((request) => request.url.endsWith("/approve")).length === 1,
    "resume must not send a second approval",
  );
  assert.ok(
    requests.filter((request) => request.url.endsWith("/intake/requests")).length === 1,
    "resume must not create a second intake request",
  );
  const evidence = JSON.parse(
    readFileSync(join(env.FACTORY_REAL_PROVIDER_EVIDENCE_DIR, "real-provider-uat-evidence.json"), "utf8"),
  );
  assert.equal(evidence.result, "SIMULATED_ACCEPTED");
  assert.equal(evidence.evidence_kind, "FAKE_CORE_SIMULATION");
  assert.equal(evidence.request_admission, "NOT_VERIFIED");
  assert.equal(evidence.boundary.paid_fallback, "NONE");
  assert.equal(evidence.boundary.redispatch, "NONE");
});

test("list-rate and provider-reported actual charge are distinct observations", () => {
  const zero = readActualCharge(
    { actual_charge_known: true, actual_charge_micros: 0 },
    "provider_session",
    "zero",
  );
  const positive = readActualCharge(
    { actual_charge_known: true, actual_charge_micros: 1914 },
    "provider_session",
    "positive",
  );
  const unknown = readActualCharge({}, "provider_session", "unknown");
  const fallback = readActualCharge(
    { billing_source: "balance" },
    "model_call",
    "fallback",
  );
  assert.equal(zero.observation, "VERIFIED_ZERO");
  assert.equal(zero.provenance, "PROVIDER_REPORTED_ACTUAL_CHARGE");
  assert.equal(positive.observation, "VERIFIED_POSITIVE");
  assert.equal(unknown.observation, "UNKNOWN");
  assert.equal(fallback.fallbackObserved, true);
  assert.equal(
    readActualCharge(
      { actual_charge_known: true, actual_charge_micros: null },
      "model_call",
      "null-charge",
    ).observation,
    "UNKNOWN",
  );
  assert.equal(
    readActualCharge({ billing_source: "go" }, "model_call", "go-route").observation,
    "UNKNOWN",
  );
  assert.equal(
    readActualCharge({ billing_source: "go-plus" }, "model_call", "go-plus-route").observation,
    "UNKNOWN",
  );
  assert.equal(
    readActualCharge({ billing_source: "free" }, "model_call", "free-route").observation,
    "UNKNOWN",
  );
  assert.equal(
    readBillingSource({ billing_source: "go" }, "model_call", "go-route").observation,
    "SUBSCRIPTION_INCLUDED",
  );
  assert.equal(
    readBillingSource({ billing_source: "balance" }, "model_call", "balance-route").observation,
    "PAID_FALLBACK",
  );
  const summarizedZero = summarizeActualCharge([
      {
        record: { actual_charge_known: true, actual_charge_micros: 0 },
        kind: "x",
        id: "x",
      },
    ]);
  assert.equal(summarizedZero.observation, "VERIFIED_ZERO");
  assert.equal(summarizedZero.provenance, "EXPLICIT_PROVIDER_REPORTED_ACTUAL_CHARGE");
  assert.equal(
    summarizeActualCharge([{ record: {}, kind: "x", id: "x" }]).observation,
    "UNKNOWN",
  );
  assert.equal(
    summarizeActualCharge([
      {
        record: { actual_charge_known: true, actual_charge_micros: null },
        kind: "x",
        id: "null-charge",
      },
    ]).observation,
    "UNKNOWN",
  );
  assert.equal(
    summarizeActualCharge([
      { record: { billing_source: "go" }, kind: "x", id: "go-route" },
    ]).observation,
    "UNKNOWN",
  );
});

test("loadSubscriptionVerification loads the Owner Console limitation", () => {
  const env = baseGate(port, { FACTORY_REAL_PROVIDER_UAT: "1" });
  const gate = parseGate(env, ["assess"]);
  const verification = loadSubscriptionVerification(gate);
  assert.equal(verification.record.source, "OWNER_CONSOLE");
  assert.equal(verification.record.core_verifiable, false);
  assert.match(verification.limitation, /CANNOT_VERIFY/);
});
