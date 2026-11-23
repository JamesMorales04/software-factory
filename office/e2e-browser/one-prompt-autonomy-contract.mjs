// Pure contracts for the one-prompt autonomy lane.
//
// This module has no Playwright, network, filesystem or Core dependency. The
// browser lane and its constructive tests import the same helpers so the
// invariants are not a second, mirroring implementation.

const WORKFLOW_PATHS = [
  /^\/intake\/requests$/,
  /^\/intake\/requests\/[^/]+\/(assess|approve|subscription-admission|message|cancel)$/,
  /^\/agentic\/requests\/[^/]+\/(plan|participants|environment(\/review|\/recompile)?|spec(\/freeze|\/publish|\/refresh)?|plan-review|license-evidence-review|license-decision)$/,
  /^\/agentic\/requests\/[^/]+\/(findings\/[^/]+\/disposition|knowledge|skills\/[^/]+\/adopt)$/,
  /^\/agentic\/decisions$/,
  /^\/agentic\/decisions\/[^/]+\/resolve$/,
  /^\/campaigns$/,
  /^\/campaigns\/[^/]+\/(authorize|runs|cancel|repair)$/,
  /^\/runs\/[^/]+\/attempts$/,
  /^\/test-support\/subscription-dispatch-admissions$/,
];

// Setup calls are configuration, not workflow state. They are still recorded
// and allow-listed explicitly so an accidental workflow POST cannot hide among
// them.
const SETUP_MUTATION_ALLOWLIST = [
  /^\/settings\/providers\/[^/]+$/,
  /^\/settings\/runtime$/,
  /^\/execution-profiles$/,
  /^\/execution-profiles\/[^/]+\/default$/,
  /^\/projects\/[^/]+\/config$/,
];

export const HOME_LICENSE_SELECTORS = [
  "home-policy-license-authority",
  "home-licence-authority-state",
  "home-licence-pin",
  "home-licence-use-context",
  "home-licence-criterion",
  "home-licence-deadline",
  "home-licence-confirm",
  "home-licence-blocked",
  "home-licence-resume",
];

// Initial MISSING/wait-owner does not necessarily show the partial-recovery
// resume control; record it but do not require it for the first wait state.
export const REQUIRED_LICENSE_WAIT_SELECTORS = HOME_LICENSE_SELECTORS.filter(
  (selector) =>
    selector !== "home-licence-resume" && selector !== "home-licence-confirm",
);

export const REQUIRED_MODEL_PHASES = [
  "assessment",
  "planner",
  "environment",
  "spec",
  "license_evidence_review",
];

export const OPTIONAL_MODEL_PHASES = ["lead_stack_selection", "semantic_review"];

export function normalizeCorePath(value) {
  if (typeof value !== "string" || value.length === 0) return "";
  let path = value;
  try {
    const url = new URL(value, "http://127.0.0.1");
    path = url.pathname;
  } catch {
    // Already a path.
  }
  if (path.startsWith("/core-api")) path = path.slice("/core-api".length);
  return path.startsWith("/") ? path : `/${path}`;
}

export function isWorkflowPath(path) {
  const normalized = normalizeCorePath(path);
  return WORKFLOW_PATHS.some((pattern) => pattern.test(normalized));
}

export function isSetupMutation(path) {
  const normalized = normalizeCorePath(path);
  return SETUP_MUTATION_ALLOWLIST.some((pattern) => pattern.test(normalized));
}

export function isReadOnlyMethod(method) {
  return ["GET", "HEAD", "OPTIONS"].includes(String(method).toUpperCase());
}

/**
 * Validate the driver-originated Core calls.
 *
 * The driver may read anything for assertions and may perform only the
 * explicit configuration setup mutations. Any workflow mutation carrying the
 * driver marker is an API-seeding failure.
 */
export function validateDriverCalls(calls) {
  const failures = [];
  const setup = [];
  for (const call of calls) {
    const method = String(call.method || "").toUpperCase();
    const path = normalizeCorePath(call.path);
    if (isReadOnlyMethod(method)) continue;
    if (isWorkflowPath(path)) {
      failures.push(`driver workflow mutation ${method} ${path}`);
      continue;
    }
    if (!isSetupMutation(path)) {
      failures.push(`driver mutation outside setup allowlist ${method} ${path}`);
      continue;
    }
    setup.push(`${method} ${path}`);
  }
  if (failures.length > 0) {
    throw new Error(`one-prompt driver seeded workflow state: ${failures.join("; ")}`);
  }
  return { setup_calls: setup };
}

/**
 * Validate the page-originated Core mutations.
 *
 * The positive lane must create the request and enable the policy exactly once,
 * from the real page. Project registration is UI setup and is allowed once.
 * Every other workflow mutation is a failure, including test-support admission
 * seeding and manual assess/plan/approve/licence/decision/freeze/start.
 */
export function validatePageMutations(requests, { licenseDelegation = "none" } = {}) {
  if (!["none", "one"].includes(licenseDelegation)) {
    throw new Error(`invalid licenseDelegation policy ${licenseDelegation}`);
  }
  const counts = new Map();
  const failures = [];
  for (const request of requests) {
    const method = String(request.method || "").toUpperCase();
    const path = normalizeCorePath(request.path);
    if (isReadOnlyMethod(method)) continue;
    const key = `${method} ${path}`;
    counts.set(key, (counts.get(key) ?? 0) + 1);
    if (method === "POST" && path === "/auth/bootstrap") continue;
    if (method === "POST" && path === "/projects") continue; // UI project import
    if (method === "POST" && path === "/intake/requests") continue;
    if (method === "POST" && /^\/projects\/[^/]+\/execution-policy$/.test(path)) {
      continue;
    }
    const decisionCreate = method === "POST" && path === "/agentic/decisions";
    const decisionResolve =
      method === "POST" && /^\/agentic\/decisions\/[^/]+\/resolve$/.test(path);
    if (decisionCreate || decisionResolve) {
      if (licenseDelegation === "none") {
        failures.push(`page workflow mutation ${key}`);
      }
      continue;
    }
    if (isWorkflowPath(path)) {
      failures.push(`page workflow mutation ${key}`);
      continue;
    }
    // Unknown mutating routes fail closed. Setup is driver-owned and carries
    // the driver marker; a page-originated setup or arbitrary mutation is not
    // part of the accepted UI contract.
    failures.push(`unknown page mutation ${key}`);
  }
  if (failures.length > 0) {
    throw new Error(`one-prompt lane used manual workflow controls: ${failures.join("; ")}`);
  }
  const intake = counts.get("POST /intake/requests") ?? 0;
  const policy = [...counts.entries()]
    .filter(([key]) => /^POST \/projects\/[^/]+\/execution-policy$/.test(key))
    .reduce((total, [, value]) => total + value, 0);
  if (intake !== 1) {
    throw new Error(`expected exactly one page POST /intake/requests, observed ${intake}`);
  }
  if (policy !== 1) {
    throw new Error(
      `expected exactly one page policy confirmation, observed ${policy}`,
    );
  }
  const projectImports = counts.get("POST /projects") ?? 0;
  if (projectImports > 1) {
    throw new Error(`expected at most one UI project import, observed ${projectImports}`);
  }
  const decisionCreates = counts.get("POST /agentic/decisions") ?? 0;
  const decisionResolves = [...counts.entries()]
    .filter(([key]) => /^POST \/agentic\/decisions\/[^/]+\/resolve$/.test(key))
    .reduce((total, [, value]) => total + value, 0);
  if (licenseDelegation === "one") {
    if (decisionCreates !== 1 || decisionResolves !== 1) {
      throw new Error(
        `expected exactly one Owner decision create+resolve for the licence delegation, observed ${decisionCreates}/${decisionResolves}`,
      );
    }
  } else if (decisionCreates !== 0 || decisionResolves !== 0) {
    throw new Error(
      `licence delegation decision mutation is not allowed in this lane: ${decisionCreates}/${decisionResolves}`,
    );
  }
  return {
    intake_posts: intake,
    policy_posts: policy,
    project_imports: projectImports,
    license_delegation_creates: decisionCreates,
    license_delegation_resolves: decisionResolves,
  };
}

export function assertNoPreKnownRequestId(env) {
  const forbidden = Object.keys(env).filter(
    (key) =>
      /^FACTORY_.*(REQUEST|INTAKE|CAMPAIGN)_ID$/i.test(key) &&
      env[key] !== undefined &&
      String(env[key]).trim() !== "",
  );
  if (forbidden.length > 0) {
    throw new Error(
      `one-prompt lane must discover the request from the UI, not ${forbidden.join(", ")}`,
    );
  }
}

export function assertCountersBaseline(counters) {
  if (!counters || typeof counters !== "object") {
    throw new Error("fixture counters are missing");
  }
  for (const [phase, value] of Object.entries(counters)) {
    if (!Number.isInteger(value) || value < 0) {
      throw new Error(`fixture counter ${phase} is not a non-negative integer`);
    }
    if (value !== 0) {
      throw new Error(`fixture counter ${phase} is ${value} before the one prompt`);
    }
  }
}

/**
 * The positive lane may use each required phase exactly once. Stack selection
 * is conditional on the fixture project. Semantic review may run once per
 * required reviewer role; the contract only requires at least one. "other" and
 * any unknown phase must stay zero, and the fixture must report no errors.
 */
export function assertCounterDelta(before, after, { semanticReviewMinimum = 1 } = {}) {
  const phases = new Set([...Object.keys(before), ...Object.keys(after)]);
  const delta = {};
  for (const phase of phases) {
    const start = before[phase] ?? 0;
    const end = after[phase] ?? 0;
    delta[phase] = end - start;
    if (delta[phase] < 0) {
      throw new Error(`fixture counter ${phase} went backwards`);
    }
  }
  for (const phase of REQUIRED_MODEL_PHASES) {
    if ((delta[phase] ?? 0) !== 1) {
      throw new Error(
        `fixture phase ${phase} must execute exactly once; observed ${delta[phase] ?? 0}`,
      );
    }
  }
  for (const phase of OPTIONAL_MODEL_PHASES) {
    if (phase === "semantic_review") continue;
    if ((delta[phase] ?? 0) > 1) {
      throw new Error(`fixture phase ${phase} executed more than once`);
    }
  }
  if ((delta.semantic_review ?? 0) < semanticReviewMinimum) {
    throw new Error(
      `fixture semantic_review must execute at least ${semanticReviewMinimum} time(s); observed ${delta.semantic_review ?? 0}`,
    );
  }
  if ((delta.other ?? 0) !== 0) {
    throw new Error(`fixture classified ${delta.other} unexpected prompt(s) as other`);
  }
  const known = new Set([...REQUIRED_MODEL_PHASES, ...OPTIONAL_MODEL_PHASES, "other"]);
  for (const phase of phases) {
    if (!known.has(phase) && (delta[phase] ?? 0) !== 0) {
      throw new Error(`fixture executed unknown phase ${phase}`);
    }
  }
  return delta;
}

export function assertNoFixtureErrors(diagnostics) {
  const errors = diagnostics?.errors;
  if (!errors || Object.keys(errors).length === 0) return;
  throw new Error(`fixture reported model errors: ${JSON.stringify(errors)}`);
}

function reservationKey(reservation) {
  return [
    reservation.reservation_id,
    reservation.state,
    reservation.updated_at,
    reservation.input_tokens,
    reservation.output_tokens,
  ].join("|");
}

export function assertStableSnapshot(before, after) {
  const problems = [];
  const compare = (label, left, right) => {
    if (JSON.stringify(left) !== JSON.stringify(right)) {
      problems.push(`${label}: ${JSON.stringify(left)} != ${JSON.stringify(right)}`);
    }
  };
  compare("request id", before.ids.request_id, after.ids.request_id);
  compare("action id", before.ids.action_id, after.ids.action_id);
  compare("lead id", before.ids.lead_id, after.ids.lead_id);
  compare("campaign id", before.ids.campaign_id, after.ids.campaign_id);
  compare("run id", before.ids.run_id, after.ids.run_id);
  compare("attempt id", before.ids.attempt_id, after.ids.attempt_id);
  compare("candidate id", before.ids.candidate_id, after.ids.candidate_id);
  compare("candidate commit", before.ids.candidate_commit, after.ids.candidate_commit);
  compare("verifier receipt", before.ids.verifier_receipt, after.ids.verifier_receipt);
  compare("reviewer receipt", before.ids.reviewer_receipt, after.ids.reviewer_receipt);
  compare("gate", before.ids.gate, after.ids.gate);
  compare("plan hash", before.plan.plan_hash, after.plan.plan_hash);
  compare("graph sha256", before.plan.graph_sha256, after.plan.graph_sha256);
  compare("spec sha256", before.plan.spec_sha256, after.plan.spec_sha256);
  compare(
    "environment spec sha256",
    before.plan.environment_spec_sha256,
    after.plan.environment_spec_sha256,
  );
  compare("request state", before.request_state, after.request_state);
  compare("campaign status", before.campaign_status, after.campaign_status);
  compare("attempt count", before.attempt_count, after.attempt_count);
  compare("fixture counters", before.counters, after.counters);
  const beforeReservations = (before.reservations ?? []).map(reservationKey).sort();
  const afterReservations = (after.reservations ?? []).map(reservationKey).sort();
  compare("model reservations", beforeReservations, afterReservations);
  if (problems.length > 0) {
    throw new Error(`restart changed durable identity/state: ${problems.join("; ")}`);
  }
  return { stable: true, reservations: afterReservations.length };
}

export function assertNoManualControls(bodyText, labels) {
  const observed = labels.filter((label) => String(bodyText).includes(label));
  if (observed.length > 0) {
    throw new Error(`manual controls visible in AUTONOMOUS positive lane: ${observed.join(", ")}`);
  }
}

export function assertLeaseProjection(requestState, participants) {
  if (!Array.isArray(participants)) {
    throw new Error("participants projection is not a list");
  }
  const allowed = new Set(["LIVE", "EXPIRED", "RELEASED"]);
  const facts = [];
  for (const participant of participants) {
    const lease = String(participant.lease_state ?? "");
    const status = String(participant.status ?? "");
    if (!allowed.has(lease)) {
      throw new Error(`participant ${participant.participant_id ?? "?"} has invalid lease_state ${lease}`);
    }
    if (!status) {
      throw new Error(`participant ${participant.participant_id ?? "?"} has no separate status`);
    }
    if (requestState === "EXECUTING" && status === "RUNNING" && lease !== "LIVE") {
      throw new Error(
        `request EXECUTING and participant RUNNING but lease is ${lease}; unexplained lease/request contradiction`,
      );
    }
    if (requestState === "COMPLETED" && lease === "LIVE") {
      throw new Error(
        `request COMPLETED but participant ${participant.participant_id ?? "?"} still shows a LIVE lease`,
      );
    }
    facts.push({
      participant_id: participant.participant_id ?? null,
      role: participant.role ?? null,
      status,
      lease_state: lease,
      status_source: participant.status_source ?? null,
      observation_note: participant.observation_note ?? null,
      failure_reason: participant.failure_reason ?? null,
    });
  }
  return { request_state: requestState, participants: facts };
}

export function assertLicenseWaitOwner({
  projection,
  countersBefore,
  countersAfter,
  licenseDecisions,
  licenseReview,
  environment,
  spec,
  ownerDecisions,
  licenseAuthority,
}) {
  const state = String(projection?.state ?? "");
  if (!["BLOCKED", "WAITING_OWNER"].includes(state)) {
    throw new Error(`licence wait-owner lane expects BLOCKED/WAITING_OWNER, observed ${state}`);
  }
  const detail = String(projection?.stateDetail ?? projection?.state_detail ?? "");
  if (!/licen[cs]e gate/i.test(detail)) {
    throw new Error("projection detail does not name the licence gate");
  }
  if (!/owner delegation/i.test(detail)) {
    throw new Error("projection detail does not require the Owner licence delegation");
  }
  if (projection?.campaignId) {
    throw new Error("licence wait-owner lane created a Campaign");
  }
  const delta =
    (countersAfter?.license_evidence_review ?? 0) -
    (countersBefore?.license_evidence_review ?? 0);
  if (delta !== 0) {
    throw new Error(`licence wait-owner lane dispatched ${delta} licence model review(s)`);
  }
  if ((licenseDecisions ?? []).length !== 0) {
    throw new Error("licence wait-owner lane persisted a licence decision");
  }
  if (licenseReview?.available === true) {
    throw new Error("licence wait-owner lane produced an available licence review");
  }
  const environmentStatus = String(environment?.status ?? "");
  if (environmentStatus === "FROZEN" || environment?.specSha256) {
    throw new Error("licence wait-owner lane froze an environment");
  }
  if (spec?.frozen || spec?.spec?.spec_sha256) {
    throw new Error("licence wait-owner lane froze a specification");
  }
  const approvedLicenseDelegation = (ownerDecisions ?? []).find(
    (decision) =>
      decision.matter === "LICENSE_IDENTITY_EVIDENCE_DELEGATION" &&
      decision.status === "APPROVED" &&
      decision.selected_option === "APPROVE_DELEGATION",
  );
  if (approvedLicenseDelegation) {
    throw new Error("execution policy confirmation also created a licence delegation");
  }
  let authorityState = "MISSING_PROJECTION";
  if (licenseAuthority) {
    if (licenseAuthority.required !== true) {
      throw new Error("licence authority projection must be required:true");
    }
    authorityState = String(licenseAuthority.state ?? "UNKNOWN");
    if (authorityState === "APPROVED") {
      throw new Error("licence authority is APPROVED; use the positive lane, not wait-owner");
    }
    if (!["MISSING", "REVOKED", "EXPIRED", "MISMATCH", "UNKNOWN"].includes(authorityState)) {
      throw new Error(`licence wait-owner lane must not see authority state ${authorityState}`);
    }
    if (licenseAuthority.source !== "OWNER_DECISION") {
      throw new Error("licence authority source must be OWNER_DECISION");
    }
    // The standing Owner grant is project-scoped. Request/plan/receipt
    // identity is intentionally null before the exact Lead decision; do not
    // require future identity on the grant.
    const scope = licenseAuthority.scope ?? {};
    const project =
      licenseAuthority.project ??
      licenseAuthority.projectId ??
      scope.projectId ??
      scope.project_id;
    if (!project || (typeof project === "string" && project.trim() === "")) {
      throw new Error("licence authority grant is missing project");
    }
    for (const field of ["pin", "useContext", "economicCriterion"]) {
      if (
        typeof licenseAuthority[field] !== "string" ||
        licenseAuthority[field].trim() === ""
      ) {
        throw new Error(`licence authority grant is missing ${field}`);
      }
    }
    // Authority revision is NOT the execution-profile revision. Never accept
    // scope.profileRevision as a substitute; the two values are tested as
    // different below.
    const authorityRevision =
      licenseAuthority.authorityRevision ??
      licenseAuthority.delegationRevision ??
      licenseAuthority.revision;
    if (
      authorityRevision === null ||
      authorityRevision === undefined ||
      String(authorityRevision).trim() === ""
    ) {
      throw new Error(
        "licence authority grant is missing authorityRevision (scope.profileRevision is not authority revision)",
      );
    }
    if (
      typeof licenseAuthority.deadline !== "string" ||
      licenseAuthority.deadline.trim() === ""
    ) {
      throw new Error("licence authority grant is missing deadline");
    }
  }
  return {
    state,
    detail,
    license_authority_state: authorityState,
    license_model_calls: 0,
    license_decisions: 0,
    environment_status: environmentStatus || "MISSING",
    campaign_id: null,
    approved_license_delegation: false,
  };
}

export function assertFundingProjection(funding) {
  if (!funding || typeof funding !== "object") {
    throw new Error("funding projection is missing");
  }
  if (funding.provider !== "opencode-go") {
    throw new Error(`funding provider must be opencode-go, observed ${funding.provider}`);
  }
  if (funding.route !== "GO_SUBSCRIPTION_INCLUDED") {
    throw new Error(`funding route is not GO_SUBSCRIPTION_INCLUDED: ${funding.route}`);
  }
  const coreVerifiable =
    funding.coreVerifiable ?? funding.core_verifiable ?? null;
  if (coreVerifiable !== false) {
    throw new Error("funding projection must be coreVerifiable:false");
  }
  if (funding.admissible !== false) {
    throw new Error("funding projection must be admissible:false");
  }
  if (funding.entitlement !== "NOT_RECORDED") {
    throw new Error(`unexpected funding entitlement: ${funding.entitlement}`);
  }
  if (funding.quota?.state !== "UNKNOWN") {
    throw new Error(`unexpected funding quota state: ${funding.quota?.state}`);
  }
  const missing = funding.missingEvidence ?? funding.missing_evidence ?? [];
  for (const required of [
    "OWNER_CONFIRMED_EXTRA_USAGE_ASSUMPTION",
    "EXPLICIT_BOUNDED_SUBSCRIPTION_ADMISSION",
  ]) {
    if (!missing.includes(required)) {
      throw new Error(`funding projection is missing evidence marker ${required}`);
    }
  }
  return {
    provider: funding.provider,
    route: funding.route,
    enabled: funding.enabled,
    configured: funding.configured,
    core_verifiable: false,
    admissible: false,
    entitlement: funding.entitlement,
    quota: funding.quota,
    missing_evidence: missing,
  };
}

export function assertCampaignAuthorization(authorization) {
  if (!authorization) throw new Error("Campaign authorization is missing");
  const risk = authorization.riskClass ?? authorization.risk_class;
  if (risk !== "R0") {
    throw new Error(`Campaign risk class must be R0, observed ${risk}`);
  }
  if (authorization.provider !== "opencode-go") {
    throw new Error(`Campaign provider must be opencode-go, observed ${authorization.provider}`);
  }
  return {
    risk_class: risk,
    execution_mode: authorization.executionMode ?? authorization.execution_mode,
    provider: authorization.provider,
    model: authorization.model,
  };
}
