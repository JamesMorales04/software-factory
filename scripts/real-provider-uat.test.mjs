// Contract tests for the bounded real-provider UAT harness.
//
// These tests do not contact OpenCode, a real Core, or any paid provider. They
// spawn the real driver against a local fake Core and prove:
//   1. live mode refuses before any network request without the explicit flag
//      and the complete authorization envelope;
//   2. the dry-run preflight is read-only and records no inference;
//   3. the operator token and a credential-shaped response field never appear
//      in stdout, stderr or the evidence file;
//   4. invalid gate combinations fail closed.

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
  parseGate,
  parseSpendCapMicros,
} from "./real-provider-uat.mjs";

const DRIVER = new URL("./real-provider-uat.mjs", import.meta.url);
const TOKEN = "test-operator-token-0123456789abcdef0123456789abcdef";
const PROVIDER_CREDENTIAL = "provider-credential-marker-must-never-appear";
const TEMP_BASE = existsSync("/tmp/opencode") ? "/tmp/opencode" : tmpdir();
const ROOT = mkdtempSync(join(TEMP_BASE, "factory-rp-uat-test-"));

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

function baseGate(port) {
  return {
    FACTORY_REAL_PROVIDER_AUTHORIZATION: "test-owner-authorization",
    FACTORY_REAL_PROVIDER_ACCOUNTING: "GO_SUBSCRIPTION_INCLUDED",
    FACTORY_REAL_PROVIDER_EXTRA_USAGE: "DISABLED",
    FACTORY_REAL_PROVIDER_SPEND_CAP_USD: "2.00",
    FACTORY_CORE_API_URL: `http://127.0.0.1:${port}/core-api`,
    FACTORY_REAL_PROVIDER_EVIDENCE_DIR: join(ROOT, "evidence"),
    FACTORY_OPERATOR_TOKEN_FILE: writeToken(),
  };
}

function runDriver(args, env) {
  return new Promise((resolvePromise, rejectPromise) => {
    const child = spawn(process.execPath, [DRIVER.pathname, ...args], {
      env: cleanEnv(env),
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      rejectPromise(new Error(`driver timed out; stdout=${stdout} stderr=${stderr}`));
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
    child.on("close", (status, signal) => {
      clearTimeout(timer);
      resolvePromise({ status, signal, stdout, stderr });
    });
  });
}

function assertRefused(result, code) {
  assert.notEqual(result.status, 0, `driver unexpectedly succeeded: ${result.stdout}`);
  assert.match(result.stderr, new RegExp(`code=${code}`), result.stderr);
  assert.doesNotMatch(result.stderr, new RegExp(TOKEN));
  assert.doesNotMatch(result.stdout, new RegExp(TOKEN));
}

function runWrapper(args, env) {
  return new Promise((resolvePromise, rejectPromise) => {
    const child = spawn("bash", [new URL("./real-provider-uat.sh", import.meta.url).pathname, ...args], {
      env: cleanEnv(env),
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      rejectPromise(new Error(`wrapper timed out; stdout=${stdout} stderr=${stderr}`));
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

let server;
let port;
let requests;
let healthMode = "ok";
let providerMaxTurns = 4;
const PLAN_HASH = "f".repeat(64);
const COMMIT = "d".repeat(40);
const TREE = "e".repeat(40);
const flow = {
  assessed: false,
  approved: false,
  githubWrite: false,
};

function resetFlow() {
  flow.assessed = false;
  flow.approved = false;
  flow.githubWrite = false;
}

function flowStatus() {
  return {
    envelopeId: "envelope-1",
    currency: "USD",
    state: "ACTIVE",
    authorizedMicros: 2_000_000,
    settledMicros: 1_234,
    reservedMicros: 0,
    ambiguousMicros: 0,
    availableMicros: 1_998_766,
    overrunMicros: 0,
    authorizationRevision: 1,
    billingBasis: "CONFIGURED_RATE",
    pricingProvenance: "test-fixture",
    unresolvedCount: 0,
    byRole: [],
    byProvider: [],
    asOf: "2026-10-29T15:00:00Z",
  };
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
  return {
    id: "intake-1",
    projectId: "project-1",
    objective: flowProposal().objective,
    mode: "REVIEW_PLAN",
    state: flow.approved
      ? "COMPLETED"
      : flow.assessed
        ? "AWAITING_APPROVAL"
        : "ASSESSING",
    stateDetail: flow.approved ? "campaign accepted" : "assessment complete",
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
    spending: { status: flowStatus() },
    campaignStatus: flow.approved ? "AUTHORIZED" : null,
    gateOutcome: flow.approved ? "ACCEPTED" : null,
    updatedAt: "2026-10-29T15:00:00Z",
  };
}

function flowCampaign() {
  return {
    campaign: { id: "campaign-1", action_id: "action-1", status: "AUTHORIZED" },
    authorization: { provider: PROVIDER, model: MODEL, riskClass: "R0" },
    inference: { envelope: { id: "envelope-1" }, status: flowStatus(), legacyEvidenceExcluded: true },
    tasks: [
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
    runs: [{ id: "run-1", campaign_id: "campaign-1" }],
    attempts: [{ id: "attempt-1", run_id: "run-1" }],
    candidates: [
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

before(async () => {
  server = createServer((request, response) => {
    requests.push({
      method: request.method,
      url: request.url,
      authorization: request.headers.authorization ?? "",
    });
    const send = (status, value) => {
      response.writeHead(status, { "content-type": "application/json" });
      response.end(JSON.stringify(value));
    };
    const path = request.url.replace(/^\/core-api/, "").split("?")[0];
    if (path === "/health") {
      if (healthMode === "echo-token") {
        send(401, {
          error: `rejected ${request.headers.authorization}`,
          credential: PROVIDER_CREDENTIAL,
        });
        return;
      }
      send(200, { healthy: true, version: "0.1.0" });
      return;
    }
    if (path === "/setup/status") {
      send(200, { required: false });
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
        modelAvailable: true,
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
      send(200, {});
      return;
    }
    if (path === "/projects/project-1/execution-policy" && request.method === "POST") {
      send(201, { id: "policy-1" });
      return;
    }
    if (path === "/intake/requests" && request.method === "POST") {
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
      flow.assessed = true;
      send(200, flowIntake());
      return;
    }
    if (path === "/intake/requests/intake-1/approve" && request.method === "POST") {
      flow.approved = true;
      send(201, flowIntake());
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
      send(200, [
        {
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
        },
      ]);
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

test("live mode refuses an incomplete accounting envelope before any request", async () => {
  requests = [];
  const env = baseGate(port);
  env.FACTORY_REAL_PROVIDER_UAT = "1";
  env.FACTORY_REAL_PROVIDER_EXTRA_USAGE = "NOT_APPLICABLE";
  const result = await runDriver(["assess"], env);
  assertRefused(result, "EXTRA_USAGE_INCONSISTENT");
  assert.equal(requests.length, 0);
});

test("execute refuses without the exact Owner plan approval attestation", async () => {
  requests = [];
  const env = baseGate(port);
  env.FACTORY_REAL_PROVIDER_UAT = "1";
  const result = await runDriver(
    ["execute", "--plan-hash", "a".repeat(64)],
    env,
  );
  assertRefused(result, "ENV_REQUIRED");
  assert.equal(requests.length, 0);
});

test("dry-run preflight is read-only and records no inference", async () => {
  requests = [];
  healthMode = "ok";
  const env = baseGate(port);
  env.FACTORY_REAL_PROVIDER_DRY_RUN = "1";
  const result = await runDriver(["preflight"], env);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(requests.length, 6);
  assert.ok(
    requests.every((request) => request.method === "GET"),
    `dry-run made a non-GET request: ${JSON.stringify(requests)}`,
  );
  const evidencePath = join(env.FACTORY_REAL_PROVIDER_EVIDENCE_DIR, "real-provider-uat-preflight.json");
  assert.ok(existsSync(evidencePath), "dry-run evidence was not written");
  const evidence = JSON.parse(readFileSync(evidencePath, "utf8"));
  assert.equal(evidence.result, "DRY_RUN_READY");
  assert.equal(evidence.paid_inference, false);
  assert.equal(evidence.mutations, false);
  assert.ok(evidence.preflight.catalog.modelAvailable);
  assert.deepEqual(evidence.preflight.catalog.models, [MODEL]);
  const serialized = JSON.stringify(evidence);
  assert.doesNotMatch(serialized, new RegExp(TOKEN));
  assert.doesNotMatch(serialized, new RegExp(PROVIDER_CREDENTIAL));
  assert.doesNotMatch(result.stdout, new RegExp(TOKEN));
  assert.doesNotMatch(result.stderr, new RegExp(TOKEN));
  assert.equal(statSync(evidencePath).mode & 0o077, 0, "evidence file must not be group/other readable");
});

test("preflight refuses a provider turn budget above the authorization", async () => {
  requests = [];
  providerMaxTurns = 8;
  healthMode = "ok";
  const env = baseGate(port);
  env.FACTORY_REAL_PROVIDER_DRY_RUN = "1";
  const result = await runDriver(["preflight"], env);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /PROVIDER_TURN_CEILING_EXCEEDED/);
  providerMaxTurns = 4;
});

test("a Core error that echoes the operator token is redacted", async () => {
  requests = [];
  healthMode = "echo-token";
  const env = baseGate(port);
  env.FACTORY_REAL_PROVIDER_DRY_RUN = "1";
  const result = await runDriver(["preflight"], env);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /\[REDACTED\]/, result.stderr);
  assert.doesNotMatch(result.stderr, new RegExp(TOKEN));
  assert.doesNotMatch(result.stdout, new RegExp(TOKEN));
  assert.doesNotMatch(result.stderr, new RegExp(PROVIDER_CREDENTIAL));
  healthMode = "ok";
});

test("a fake-Core contract run stops at plan approval and records ACCEPTED evidence", async () => {
  requests = [];
  resetFlow();
  healthMode = "ok";
  const env = baseGate(port);
  env.FACTORY_REAL_PROVIDER_UAT = "1";
  env.FACTORY_REAL_PROVIDER_PLAN_APPROVAL = "OWNER_REVIEWED";
  env.FACTORY_REAL_PROVIDER_EVIDENCE_DIR = join(ROOT, "flow-evidence");
  env.FACTORY_REAL_PROVIDER_TIMEOUT_SECS = "120";
  env.FACTORY_REAL_PROVIDER_CANDIDATE_SHA = "a".repeat(40);
  env.FACTORY_REAL_PROVIDER_COMPOSE_PROJECT = "software-factory-rp-uat-test";

  const assessed = await runDriver(["assess"], env);
  assert.equal(assessed.status, 0, assessed.stderr);
  assert.match(assessed.stdout, /REAL_PROVIDER_UAT=AWAITING_PLAN_APPROVAL/);
  const statePath = join(env.FACTORY_REAL_PROVIDER_EVIDENCE_DIR, "real-provider-uat-state.json");
  const planPath = join(env.FACTORY_REAL_PROVIDER_EVIDENCE_DIR, "plan.json");
  const state = JSON.parse(readFileSync(statePath, "utf8"));
  const plan = JSON.parse(readFileSync(planPath, "utf8"));
  assert.equal(state.phase, "AWAITING_PLAN_APPROVAL");
  assert.equal(state.plan_hash, PLAN_HASH);
  assert.equal(plan.plan_hash, PLAN_HASH);
  assert.equal(plan.route.provider, PROVIDER);
  assert.equal(plan.route.model, MODEL);
  assert.equal(flow.approved, false, "assess must not approve the plan");

  const wrongHash = await runDriver(["execute", "--plan-hash", "a".repeat(64)], env);
  assert.notEqual(wrongHash.status, 0);
  assert.match(wrongHash.stderr, /PLAN_HASH_MISMATCH/);
  assert.equal(flow.approved, false, "a mismatched plan hash must not approve the plan");

  const executed = await runDriver(["execute", "--plan-hash", PLAN_HASH], env);
  assert.equal(executed.status, 0, executed.stderr);
  assert.match(executed.stdout, /REAL_PROVIDER_UAT=ACCEPTED/);
  assert.equal(flow.approved, true);
  assert.equal(flow.githubWrite, false, "the harness attempted a GitHub PR write");
  assert.ok(
    requests.every((request) => !request.url.includes("pull_request")),
    "the harness called a GitHub PR endpoint",
  );

  const evidencePath = join(env.FACTORY_REAL_PROVIDER_EVIDENCE_DIR, "real-provider-uat-evidence.json");
  const evidence = JSON.parse(readFileSync(evidencePath, "utf8"));
  assert.equal(evidence.result, "ACCEPTED");
  assert.equal(evidence.runtime.candidate_sha, "a".repeat(40));
  assert.equal(evidence.runtime.compose_project, "software-factory-rp-uat-test");
  assert.deepEqual(evidence.runtime.compose_files, [
    "compose.yaml",
    "compose.real-provider-uat.yaml",
  ]);
  assert.equal(evidence.ids.candidate_commit, COMMIT);
  assert.equal(evidence.ids.candidate_tree, TREE);
  assert.equal(evidence.provider.route, `${PROVIDER}/${MODEL}`);
  assert.equal(evidence.provider.implementer.provider_session_id, "ses_real_123");
  assert.equal(evidence.provider.sessions[0].cleanup_receipt, "session_delete_verified_absent");
  assert.equal(evidence.receipts.verifier.result, "PASS");
  assert.equal(evidence.receipts.verifier.evidence, "EXECUTED");
  assert.equal(evidence.receipts.reviewer.result, "PASS");
  assert.equal(evidence.receipts.reviewer.mechanism, "MODEL");
  assert.equal(evidence.receipts.reviewer.model_provider, PROVIDER);
  assert.equal(evidence.receipts.reviewer.model_name, MODEL);
  assert.equal(evidence.quality_gate.outcome, "ACCEPTED");
  assert.equal(evidence.pr_preparation.status, "UNBOUND_READY");
  assert.equal(evidence.pr_preparation.write_attempted, false);
  assert.equal(evidence.accounting.envelope.settled_micros, 1234);
  assert.equal(evidence.accounting.provider_sessions.opencode_message_cost_micros, 1914);
  assert.match(evidence.accounting.provider_sessions.cost_semantics, /NOT_A_BALANCE_CHARGE/);
  assert.match(evidence.accounting.provider_sessions.allowance, /^UNKNOWN/);
  assert.deepEqual(evidence.candidate.changed_paths, ["src/pricing.rs", "tests/discounts.rs"]);
  assert.equal(evidence.boundary.github_write, "NONE");

  const serialized = JSON.stringify(evidence);
  assert.doesNotMatch(serialized, new RegExp(TOKEN));
  assert.doesNotMatch(serialized, new RegExp(PROVIDER_CREDENTIAL));
  assert.doesNotMatch(executed.stdout, new RegExp(TOKEN));
  assert.doesNotMatch(executed.stderr, new RegExp(TOKEN));
  assert.equal(statSync(evidencePath).mode & 0o077, 0);
});

test("operator token files with group/other access are refused", async () => {
  requests = [];
  const env = baseGate(port);
  env.FACTORY_REAL_PROVIDER_DRY_RUN = "1";
  env.FACTORY_OPERATOR_TOKEN_FILE = writeToken(0o644);
  const result = await runDriver(["preflight"], env);
  assertRefused(result, "OPERATOR_TOKEN_PERMISSIONS");
  assert.equal(requests.length, 0);
});

test("plain HTTP is refused outside loopback", async () => {
  const env = baseGate(port);
  env.FACTORY_REAL_PROVIDER_DRY_RUN = "1";
  env.FACTORY_CORE_API_URL = "http://core.example:8899/core-api";
  const result = await runDriver(["preflight"], env);
  assertRefused(result, "CORE_URL_INSECURE");
});

test("spend caps are exact and bounded", () => {
  assert.deepEqual(parseSpendCapMicros("2.00"), { micros: 2_000_000, usd: "2.00" });
  assert.deepEqual(parseSpendCapMicros("0.01"), { micros: 10_000, usd: "0.01" });
  for (const value of ["0", "0.00", "5.01", "2.001", "1e2", "-1", "02.00", "abc"]) {
    assert.throws(() => parseSpendCapMicros(value), undefined, value);
  }
});

test("the gate binds the exact provider, model and Go base URL", () => {
  const env = {
    FACTORY_REAL_PROVIDER_UAT: "1",
    FACTORY_REAL_PROVIDER_AUTHORIZATION: "owner-test",
    FACTORY_REAL_PROVIDER_ACCOUNTING: "GO_SUBSCRIPTION_INCLUDED",
    FACTORY_REAL_PROVIDER_EXTRA_USAGE: "DISABLED",
    FACTORY_REAL_PROVIDER_SPEND_CAP_USD: "1.00",
    FACTORY_CORE_API_URL: "https://factory.example/core-api",
    FACTORY_REAL_PROVIDER_EVIDENCE_DIR: join(ROOT, "evidence-gate"),
  };
  const gate = parseGate(env, ["assess"]);
  assert.equal(gate.provider, PROVIDER);
  assert.equal(gate.model, MODEL);
  assert.equal(gate.baseUrl, "https://opencode.ai/inference/go/openai/v1");
  assert.equal(gate.capMicros, 1_000_000);
  assert.throws(
    () =>
      parseGate(
        {
          ...env,
          FACTORY_REAL_PROVIDER_ACCOUNTING: "PAY_AS_YOU_GO_API",
          FACTORY_REAL_PROVIDER_EXTRA_USAGE: "DISABLED",
        },
        ["assess"],
      ),
    /PAY_AS_YOU_GO_API requires/,
  );
  assert.throws(
    () =>
      parseGate(
        {
          ...env,
          FACTORY_REAL_PROVIDER_EXTRA_USAGE: "ENABLED_WITHIN_CAP",
        },
        ["assess"],
      ),
    /GO_SUBSCRIPTION_INCLUDED requires Extra Usage DISABLED/,
  );
  assert.equal(
    parseGate(
      {
        ...env,
        FACTORY_REAL_PROVIDER_ACCOUNTING: "GO_EXTRA_USAGE_BILLABLE",
        FACTORY_REAL_PROVIDER_EXTRA_USAGE: "ENABLED_WITHIN_CAP",
      },
      ["assess"],
    ).accounting,
    "GO_EXTRA_USAGE_BILLABLE",
  );
});

