// Constructive contracts for the one-prompt autonomy lane.
//
// These tests build synthetic call/snapshot inputs. They do not mirror a
// feature flag, do not need Playwright, Core, Compose or a browser, and they do
// not claim that the real lane ran.

import assert from "node:assert/strict";
import test from "node:test";

import {
  assertCampaignAuthorization,
  assertCounterDelta,
  assertCountersBaseline,
  assertNoFixtureErrors,
  assertNoManualControls,
  assertNoPreKnownRequestId,
  assertStableSnapshot,
  normalizeCorePath,
  validateDriverCalls,
  validatePageMutations,
} from "./one-prompt-autonomy-contract.mjs";

const snapshot = (overrides = {}) => ({
  ids: {
    request_id: "req-1",
    action_id: "action-1",
    lead_id: "lead-1",
    campaign_id: "campaign-1",
    run_id: "run-1",
    attempt_id: "attempt-1",
    candidate_id: "candidate-1",
    candidate_commit: "a".repeat(40),
    verifier_receipt: "verifier-1",
    reviewer_receipt: "reviewer-1",
    gate: "ACCEPTED",
  },
  plan: {
    plan_hash: "plan-hash",
    graph_sha256: "graph-sha",
    spec_sha256: "spec-sha",
    environment_spec_sha256: "env-spec-sha",
  },
  request_state: "COMPLETED",
  campaign_status: "AUTHORIZED",
  attempt_count: 1,
  counters: {
    assessment: 1,
    planner: 1,
    environment: 1,
    spec: 1,
    license_evidence_review: 1,
    semantic_review: 1,
    other: 0,
  },
  reservations: [
    {
      reservation_id: "reservation-1",
      state: "SETTLED",
      updated_at: "2026-11-23T00:00:00Z",
      input_tokens: 10,
      output_tokens: 5,
    },
  ],
  ...overrides,
});

test("normalizeCorePath removes origin and /core-api exactly once", () => {
  assert.equal(normalizeCorePath("http://127.0.0.1:1/core-api/intake/requests"), "/intake/requests");
  assert.equal(normalizeCorePath("/core-api/campaigns/x"), "/campaigns/x");
  assert.equal(normalizeCorePath("campaigns/x"), "/campaigns/x");
});

test("driver reads are allowed but workflow writes are rejected", () => {
  assert.doesNotThrow(() =>
    validateDriverCalls([
      { method: "GET", path: "/intake/requests/req-1" },
      { method: "GET", path: "/quality/candidates/candidate-1/receipts" },
      { method: "PUT", path: "/settings/providers/opencode-go" },
      { method: "PUT", path: "/projects/project-1/config" },
      { method: "POST", path: "/execution-profiles" },
      { method: "POST", path: "/execution-profiles/profile-1/default" },
    ]),
  );
  for (const path of [
    "/intake/requests",
    "/intake/requests/req-1/assess",
    "/intake/requests/req-1/approve",
    "/agentic/requests/req-1/plan",
    "/agentic/requests/req-1/environment/review",
    "/agentic/requests/req-1/license-decision",
    "/agentic/requests/req-1/spec/freeze",
    "/campaigns/campaign-1/authorize",
  ]) {
    assert.throws(
      () => validateDriverCalls([{ method: "POST", path }]),
      /seeded workflow state/,
      path,
    );
  }
});

test("driver mutation outside the setup allowlist is rejected", () => {
  assert.throws(
    () => validateDriverCalls([{ method: "POST", path: "/projects" }]),
    /outside setup allowlist/,
  );
});

test("page must own exactly one intake submit and one policy confirmation", () => {
  const requests = [
    { method: "POST", path: "/projects" },
    { method: "POST", path: "/intake/requests" },
    { method: "POST", path: "/projects/project-1/execution-policy" },
  ];
  assert.deepEqual(validatePageMutations(requests), {
    intake_posts: 1,
    policy_posts: 1,
    project_imports: 1,
  });
  assert.throws(
    () => validatePageMutations([{ method: "POST", path: "/intake/requests" }]),
    /policy confirmation/,
  );
  assert.throws(
    () =>
      validatePageMutations([
        { method: "POST", path: "/intake/requests" },
        { method: "POST", path: "/projects/project-1/execution-policy" },
        { method: "POST", path: "/intake/requests/req-1/assess" },
      ]),
    /manual workflow controls/,
  );
  assert.throws(
    () =>
      validatePageMutations([
        { method: "POST", path: "/intake/requests" },
        { method: "POST", path: "/intake/requests" },
        { method: "POST", path: "/projects/project-1/execution-policy" },
      ]),
    /exactly one page POST/,
  );
});

test("pre-known request identifiers are rejected", () => {
  assert.doesNotThrow(() => assertNoPreKnownRequestId({ FACTORY_UAT_PHASE: "run" }));
  assert.throws(
    () => assertNoPreKnownRequestId({ FACTORY_AUTONOMOUS_REQUEST_ID: "req-1" }),
    /discover the request from the UI/,
  );
  assert.throws(
    () => assertNoPreKnownRequestId({ FACTORY_INTAKE_REQUEST_ID: "req-1" }),
    /discover the request from the UI/,
  );
});

test("fixture counters must start at zero", () => {
  assert.doesNotThrow(() =>
    assertCountersBaseline({ assessment: 0, planner: 0, other: 0 }),
  );
  assert.throws(
    () => assertCountersBaseline({ assessment: 1 }),
    /before the one prompt/,
  );
});

test("positive counter delta requires each required phase exactly once", () => {
  const before = {
    assessment: 0,
    planner: 0,
    environment: 0,
    spec: 0,
    license_evidence_review: 0,
    semantic_review: 0,
    other: 0,
  };
  const after = {
    assessment: 1,
    planner: 1,
    environment: 1,
    spec: 1,
    license_evidence_review: 1,
    semantic_review: 2,
    other: 0,
  };
  assert.deepEqual(assertCounterDelta(before, after), {
    assessment: 1,
    planner: 1,
    environment: 1,
    spec: 1,
    license_evidence_review: 1,
    semantic_review: 2,
    other: 0,
  });
  assert.throws(
    () => assertCounterDelta(before, { ...after, license_evidence_review: 0 }),
    /license_evidence_review/,
  );
  assert.throws(
    () => assertCounterDelta(before, { ...after, other: 1 }),
    /unexpected prompt/,
  );
  assert.throws(
    () => assertCounterDelta(before, { ...after, semantic_review: 0 }),
    /semantic_review/,
  );
});

test("fixture errors fail the lane", () => {
  assert.doesNotThrow(() => assertNoFixtureErrors({ errors: {} }));
  assert.throws(
    () => assertNoFixtureErrors({ errors: { "req|planner": "bad identity" } }),
    /fixture reported model errors/,
  );
});

test("restart stability rejects a second model reservation or changed ID", () => {
  const before = snapshot();
  assert.doesNotThrow(() => assertStableSnapshot(before, structuredClone(before)));
  const extra = structuredClone(before);
  extra.reservations.push({
    reservation_id: "reservation-2",
    state: "SETTLED",
    updated_at: "2026-11-23T00:00:01Z",
    input_tokens: 1,
    output_tokens: 1,
  });
  assert.throws(() => assertStableSnapshot(before, extra), /model reservations/);
  const changed = structuredClone(before);
  changed.ids.campaign_id = "campaign-2";
  assert.throws(() => assertStableSnapshot(before, changed), /campaign id/);
  const counter = structuredClone(before);
  counter.counters.planner = 2;
  assert.throws(() => assertStableSnapshot(before, counter), /fixture counters/);
});

test("manual controls are rejected only when visible", () => {
  assert.doesNotThrow(() => assertNoManualControls("Autopilot owns these steps", ["Run assessment"]));
  assert.throws(
    () => assertNoManualControls("Run assessment", ["Run assessment"]),
    /manual controls visible/,
  );
});

test("campaign authorization requires exact R0 and the configured route", () => {
  assert.deepEqual(
    assertCampaignAuthorization({
      riskClass: "R0",
      executionMode: "TRUSTED_LOCAL_PROVIDER",
      provider: "opencode-go",
      model: "deepseek-v4.1-flash",
    }),
    {
      risk_class: "R0",
      execution_mode: "TRUSTED_LOCAL_PROVIDER",
      provider: "opencode-go",
      model: "deepseek-v4.1-flash",
    },
  );
  assert.throws(
    () => assertCampaignAuthorization({ riskClass: "R1", provider: "opencode-go" }),
    /must be R0/,
  );
  assert.throws(
    () => assertCampaignAuthorization({ riskClass: "R0", provider: "codex" }),
    /provider must be opencode-go/,
  );
});
