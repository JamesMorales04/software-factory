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
  assertFundingProjection,
  assertLeaseProjection,
  assertLicenseWaitOwner,
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
    license_delegation_creates: 0,
    license_delegation_resolves: 0,
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
  assert.throws(
    () =>
      validatePageMutations([
        { method: "POST", path: "/intake/requests" },
        { method: "POST", path: "/projects/project-1/execution-policy" },
        { method: "POST", path: "/agentic/decisions" },
      ]),
    /manual workflow controls/,
  );
  assert.throws(
    () =>
      validatePageMutations([
        { method: "POST", path: "/intake/requests" },
        { method: "POST", path: "/projects/project-1/execution-policy" },
        { method: "PUT", path: "/unknown/mutation" },
      ]),
    /unknown page mutation/,
  );
  assert.deepEqual(
    validatePageMutations(
      [
        { method: "POST", path: "/intake/requests" },
        { method: "POST", path: "/projects/project-1/execution-policy" },
        { method: "POST", path: "/agentic/decisions" },
        { method: "POST", path: "/agentic/decisions/decision-1/resolve" },
      ],
      { licenseDelegation: "one" },
    ).license_delegation_creates,
    1,
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

test("lease and request are separate; contradiction is rejected", () => {
  const runningLive = [
    {
      participant_id: "implementer:1",
      role: "IMPLEMENTER",
      status: "RUNNING",
      lease_state: "LIVE",
      status_source: "provider-session",
    },
  ];
  assert.equal(assertLeaseProjection("EXECUTING", runningLive).participants.length, 1);
  const runningReleased = structuredClone(runningLive);
  runningReleased[0].lease_state = "RELEASED";
  assert.throws(
    () => assertLeaseProjection("EXECUTING", runningReleased),
    /unexplained lease\/request contradiction/,
  );
  const completedReleased = structuredClone(runningReleased);
  completedReleased[0].status = "COMPLETED";
  assert.doesNotThrow(() => assertLeaseProjection("EXECUTING", completedReleased));
  const completedLive = structuredClone(runningLive);
  assert.throws(
    () => assertLeaseProjection("COMPLETED", completedLive),
    /still shows a LIVE lease/,
  );
  assert.throws(
    () => assertLeaseProjection("COMPLETED", [{ status: "COMPLETED", lease_state: "BROKEN" }]),
    /invalid lease_state/,
  );
});

test("licence wait-owner requires zero licence model/decision/freeze/Campaign", () => {
  const base = {
    projection: {
      state: "BLOCKED",
      stateDetail:
        "Autonomous preparation is blocked at the licence gate; an exact licence evidence review and decision for the current plan, identity receipt and approved Owner delegation are required.",
      campaignId: null,
    },
    countersBefore: { license_evidence_review: 0 },
    countersAfter: { license_evidence_review: 0 },
    licenseDecisions: [],
    licenseReview: { available: false },
    environment: { status: "CORE_VALIDATED", specSha256: null },
    spec: { frozen: null },
    ownerDecisions: [],
  };
  const result = assertLicenseWaitOwner(base);
  assert.equal(result.license_model_calls, 0);
  assert.equal(result.campaign_id, null);
  assert.equal(result.license_authority_state, "MISSING_PROJECTION");
  assert.equal(
    assertLicenseWaitOwner({
      ...base,
      licenseAuthority: {
        required: true,
        state: "MISSING",
        source: "OWNER_DECISION",
        project: "project-1",
        pin: "github:NixOS/nixpkgs/pin",
        useContext: "commercial-development",
        economicCriterion: "no-mandatory-fee-commercial-use",
        authorityRevision: 1,
        deadline: "2026-12-01T00:00:00Z",
        // Request/plan/receipt identity is not required on the standing grant.
        requestId: null,
        planSha256: null,
        receiptId: null,
      },
    }).license_authority_state,
    "MISSING",
  );
  assert.throws(
    () =>
      assertLicenseWaitOwner({
        ...base,
        licenseAuthority: {
          required: true,
          state: "APPROVED",
          source: "OWNER_DECISION",
        },
      }),
    /APPROVED; use the positive lane/,
  );
  assert.equal(
    assertLicenseWaitOwner({
      ...base,
      licenseAuthority: {
        required: true,
        state: "UNKNOWN",
        source: "OWNER_DECISION",
        project: "project-1",
        pin: "github:NixOS/nixpkgs/pin",
        useContext: "commercial-development",
        economicCriterion: "no-mandatory-fee-commercial-use",
        authorityRevision: 1,
        deadline: "2026-12-01T00:00:00Z",
      },
    }).license_authority_state,
    "UNKNOWN",
  );
  assert.throws(
    () =>
      assertLicenseWaitOwner({
        ...base,
        licenseAuthority: {
          required: true,
          state: "MISSING",
          source: "OWNER_DECISION",
          pin: "github:NixOS/nixpkgs/pin",
          useContext: "commercial-development",
          economicCriterion: "no-mandatory-fee-commercial-use",
          authorityRevision: 1,
          deadline: "2026-12-01T00:00:00Z",
        },
      }),
    /missing project/,
  );
  assert.doesNotThrow(() =>
    assertLicenseWaitOwner({
      ...base,
      licenseAuthority: {
        required: true,
        state: "MISSING",
        source: "OWNER_DECISION",
        scope: { projectId: "project-1", profileRevision: 7 },
        authorityRevision: 3,
        pin: "github:NixOS/nixpkgs/pin",
        useContext: "commercial-development",
        economicCriterion: "no-mandatory-fee-commercial-use",
        deadline: "2026-12-01T00:00:00Z",
      },
    }),
  );
  assert.throws(
    () =>
      assertLicenseWaitOwner({
        ...base,
        licenseAuthority: {
          required: true,
          state: "MISSING",
          source: "OWNER_DECISION",
          scope: { projectId: "project-1", profileRevision: 7 },
          pin: "github:NixOS/nixpkgs/pin",
          useContext: "commercial-development",
          economicCriterion: "no-mandatory-fee-commercial-use",
          deadline: "2026-12-01T00:00:00Z",
        },
      }),
    /scope.profileRevision is not authority revision/,
  );
  assert.throws(
    () =>
      assertLicenseWaitOwner({
        ...base,
        licenseAuthority: {
          required: true,
          state: "MISSING",
          source: "OWNER_DECISION",
          project: "project-1",
          pin: "github:NixOS/nixpkgs/pin",
          useContext: "commercial-development",
          economicCriterion: "no-mandatory-fee-commercial-use",
          revision: 1,
          deadline: "2026-12-01T00:00:00Z",
        },
      }),
    /no aliases/,
  );
  assert.throws(
    () =>
      assertLicenseWaitOwner({
        ...base,
        countersAfter: { license_evidence_review: 1 },
      }),
    /dispatched 1 licence model/,
  );
  assert.throws(
    () => assertLicenseWaitOwner({ ...base, projection: { ...base.projection, campaignId: "c" } }),
    /created a Campaign/,
  );
  assert.throws(
    () =>
      assertLicenseWaitOwner({
        ...base,
        ownerDecisions: [
          {
            matter: "LICENSE_IDENTITY_EVIDENCE_DELEGATION",
            status: "APPROVED",
            selected_option: "APPROVE_DELEGATION",
          },
        ],
      }),
    /also created a licence delegation/,
  );
  assert.throws(
    () =>
      assertLicenseWaitOwner({
        ...base,
        projection: { ...base.projection, stateDetail: "blocked" },
      }),
    /does not name the licence gate/,
  );
});

test("funding projection must be truthful, not just enabled", () => {
  const projection = {
    provider: "opencode-go",
    route: "GO_SUBSCRIPTION_INCLUDED",
    configured: true,
    enabled: true,
    coreVerifiable: false,
    admissible: false,
    entitlement: "NOT_RECORDED",
    quota: { state: "UNKNOWN", reason: "NO_PROVIDER_QUOTA_OBSERVATION_REQUIRED" },
    missingEvidence: [
      "OWNER_CONFIRMED_EXTRA_USAGE_ASSUMPTION",
      "EXPLICIT_BOUNDED_SUBSCRIPTION_ADMISSION",
    ],
  };
  const result = assertFundingProjection(projection);
  assert.equal(result.admissible, false);
  assert.equal(result.core_verifiable, false);
  assert.throws(
    () => assertFundingProjection({ ...projection, admissible: true }),
    /admissible:false/,
  );
  assert.throws(
    () => assertFundingProjection({ ...projection, coreVerifiable: true }),
    /coreVerifiable:false/,
  );
  assert.throws(
    () => assertFundingProjection({ ...projection, missingEvidence: [] }),
    /missing evidence marker/,
  );
  assert.throws(
    () => assertFundingProjection({ ...projection, entitlement: "VERIFIED" }),
    /unexpected funding entitlement/,
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
