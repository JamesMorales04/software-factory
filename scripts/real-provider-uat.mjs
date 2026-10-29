#!/usr/bin/env node
// Bounded real-provider UAT driver for the Factory Compose product path.
//
// Default-closed. Live modes (`assess`, `execute`) refuse to make any request
// until the Owner authorization env is complete and the local Core preflight is
// clean. The driver never reads, prints or receives the OpenCode Go credential:
// the Owner stores it in Office/Core, and the Worker obtains it from Core.
//
// Modes:
//   preflight  read-only readiness check (`FACTORY_REAL_PROVIDER_DRY_RUN=1`)
//   assess     create Project/Request and run the real MODEL assessment
//   execute    approve one exact frozen plan and wait for the Gate
//
// Evidence is written as JSON with credentials and operator tokens redacted.
// No GitHub write is ever attempted.

import { createHash } from "node:crypto";
import {
  chmodSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const PROVIDER = "opencode-go";
export const MODEL = "deepseek-v4.1-flash";
export const PROVIDER_BASE_URL = "https://opencode.ai/inference/go/openai/v1";
export const MAX_SPEND_CAP_MICROS = 5_000_000;
export const DEFAULT_ASSESSMENT_TIMEOUT_MS = 300_000;
export const DEFAULT_EXECUTION_TIMEOUT_MS = 1_800_000;
export const EXIT_REFUSED = 3;

const activeSecrets = new Set();

export class UatError extends Error {
  constructor(code, message, exitCode = 1) {
    super(message);
    this.name = "UatError";
    this.code = code;
    this.exitCode = exitCode;
  }
}

function refuse(code, message) {
  return new UatError(code, message, EXIT_REFUSED);
}

function sleep(milliseconds) {
  return new Promise((resolvePromise) => setTimeout(resolvePromise, milliseconds));
}

function bounded(value, limit = 1_200) {
  return value.length <= limit ? value : `${value.slice(0, limit)}…`;
}

function requireString(env, name, { min = 1, max = 512, pattern } = {}) {
  const value = env[name];
  if (typeof value !== "string" || value.trim() !== value || value.length < min) {
    throw refuse("ENV_REQUIRED", `${name} is required`);
  }
  if (value.length > max) {
    throw refuse("ENV_INVALID", `${name} is too long`);
  }
  if (pattern && !pattern.test(value)) {
    throw refuse("ENV_INVALID", `${name} has an invalid format`);
  }
  return value;
}

export function parseSpendCapMicros(value) {
  if (typeof value !== "string" || value.length > 16) {
    throw refuse("SPEND_CAP_INVALID", "FACTORY_REAL_PROVIDER_SPEND_CAP_USD is required");
  }
  const match = /^(0|[1-9]\d{0,2})(?:\.(\d{1,2}))?$/.exec(value);
  if (!match) {
    throw refuse(
      "SPEND_CAP_INVALID",
      "FACTORY_REAL_PROVIDER_SPEND_CAP_USD must be a plain positive USD amount with at most two decimals",
    );
  }
  const dollars = Number(match[1]);
  const cents = Number((match[2] ?? "").padEnd(2, "0"));
  const micros = dollars * 1_000_000 + cents * 10_000;
  if (micros <= 0 || micros > MAX_SPEND_CAP_MICROS) {
    throw refuse(
      "SPEND_CAP_OUT_OF_BOUNDS",
      `FACTORY_REAL_PROVIDER_SPEND_CAP_USD must be greater than 0 and at most ${MAX_SPEND_CAP_MICROS / 1_000_000}`,
    );
  }
  return {
    micros,
    usd: `${dollars}.${String(cents).padStart(2, "0")}`,
  };
}

function parseCoreUrl(value) {
  let url;
  try {
    url = new URL(value);
  } catch {
    throw refuse("CORE_URL_INVALID", "FACTORY_CORE_API_URL must be a valid URL");
  }
  if (!["http:", "https:"].includes(url.protocol)) {
    throw refuse("CORE_URL_INVALID", "FACTORY_CORE_API_URL must use http or https");
  }
  if (url.username || url.password || url.search || url.hash) {
    throw refuse(
      "CORE_URL_INVALID",
      "FACTORY_CORE_API_URL must not contain credentials, a query or a fragment",
    );
  }
  const loopback = ["127.0.0.1", "localhost", "[::1]"].includes(url.hostname);
  if (url.protocol === "http:" && !loopback) {
    throw refuse(
      "CORE_URL_INSECURE",
      "plain-HTTP FACTORY_CORE_API_URL is allowed only on loopback",
    );
  }
  return `${url.origin}${url.pathname.replace(/\/+$/, "")}`;
}

function parseCommand(argv) {
  const command = argv[0];
  if (!["preflight", "assess", "execute"].includes(command)) {
    throw refuse(
      "USAGE",
      "usage: real-provider-uat.mjs <preflight|assess|execute --plan-hash SHA256>",
    );
  }
  let planHash = "";
  if (command === "execute") {
    const index = argv.indexOf("--plan-hash");
    if (index < 0 || typeof argv[index + 1] !== "string") {
      throw refuse("PLAN_HASH_REQUIRED", "execute requires --plan-hash <64-hex SHA-256>");
    }
    planHash = argv[index + 1].toLowerCase();
    if (!/^[0-9a-f]{64}$/.test(planHash)) {
      throw refuse("PLAN_HASH_INVALID", "execute --plan-hash must be a SHA-256 hex digest");
    }
    if (argv.length !== index + 2) {
      throw refuse("USAGE", "execute accepts only --plan-hash <SHA-256>");
    }
  }
  return { command, planHash };
}

const ACCOUNTING_MODES = new Set([
  "GO_SUBSCRIPTION_INCLUDED",
  "GO_EXTRA_USAGE_BILLABLE",
  "PAY_AS_YOU_GO_API",
]);
const EXTRA_USAGE_MODES = new Set([
  "DISABLED",
  "ENABLED_WITHIN_CAP",
  "NOT_APPLICABLE",
]);

export function parseGate(env, argv) {
  const { command, planHash } = parseCommand(argv);

  if (command === "preflight") {
    if (env.FACTORY_REAL_PROVIDER_DRY_RUN !== "1") {
      throw refuse(
        "DRY_RUN_FLAG_MISSING",
        "FACTORY_REAL_PROVIDER_DRY_RUN=1 is required for preflight; no request was made",
      );
    }
  } else if (env.FACTORY_REAL_PROVIDER_UAT !== "1") {
    throw refuse(
      "LIVE_FLAG_MISSING",
      "FACTORY_REAL_PROVIDER_UAT=1 is required for a live run; no request was made",
    );
  }

  const authorization = requireString(env, "FACTORY_REAL_PROVIDER_AUTHORIZATION", {
    min: 3,
    max: 128,
    pattern: /^[A-Za-z0-9][A-Za-z0-9._:-]*$/,
  });
  const accounting = requireString(env, "FACTORY_REAL_PROVIDER_ACCOUNTING", {
    max: 64,
  });
  if (!ACCOUNTING_MODES.has(accounting)) {
    throw refuse(
      "ACCOUNTING_INVALID",
      `FACTORY_REAL_PROVIDER_ACCOUNTING must be one of ${[...ACCOUNTING_MODES].join(", ")}`,
    );
  }
  const extraUsage = requireString(env, "FACTORY_REAL_PROVIDER_EXTRA_USAGE", {
    max: 64,
  });
  if (!EXTRA_USAGE_MODES.has(extraUsage)) {
    throw refuse(
      "EXTRA_USAGE_INVALID",
      `FACTORY_REAL_PROVIDER_EXTRA_USAGE must be one of ${[...EXTRA_USAGE_MODES].join(", ")}`,
    );
  }
  if (accounting === "PAY_AS_YOU_GO_API" && extraUsage !== "NOT_APPLICABLE") {
    throw refuse(
      "EXTRA_USAGE_INCONSISTENT",
      "PAY_AS_YOU_GO_API requires FACTORY_REAL_PROVIDER_EXTRA_USAGE=NOT_APPLICABLE",
    );
  }
  if (accounting !== "PAY_AS_YOU_GO_API" && extraUsage === "NOT_APPLICABLE") {
    throw refuse(
      "EXTRA_USAGE_INCONSISTENT",
      "Go subscription modes require DISABLED or ENABLED_WITHIN_CAP",
    );
  }
  if (accounting === "GO_SUBSCRIPTION_INCLUDED" && extraUsage !== "DISABLED") {
    throw refuse(
      "EXTRA_USAGE_INCONSISTENT",
      "GO_SUBSCRIPTION_INCLUDED requires Extra Usage DISABLED; use GO_EXTRA_USAGE_BILLABLE when balance fallback is possible",
    );
  }
  if (accounting === "GO_EXTRA_USAGE_BILLABLE" && extraUsage !== "ENABLED_WITHIN_CAP") {
    throw refuse(
      "EXTRA_USAGE_INCONSISTENT",
      "GO_EXTRA_USAGE_BILLABLE requires Extra Usage ENABLED_WITHIN_CAP",
    );
  }

  const cap = parseSpendCapMicros(env.FACTORY_REAL_PROVIDER_SPEND_CAP_USD);
  const coreApiUrl = parseCoreUrl(
    requireString(env, "FACTORY_CORE_API_URL", { max: 512 }),
  );
  const evidenceDir = resolve(
    requireString(env, "FACTORY_REAL_PROVIDER_EVIDENCE_DIR", { max: 4096 }),
  );
  if (!isAbsolute(evidenceDir)) {
    throw refuse("EVIDENCE_DIR_INVALID", "evidence directory must be absolute");
  }

  let timeoutSecs = DEFAULT_EXECUTION_TIMEOUT_MS / 1_000;
  if (env.FACTORY_REAL_PROVIDER_TIMEOUT_SECS !== undefined) {
    const parsed = Number(env.FACTORY_REAL_PROVIDER_TIMEOUT_SECS);
    if (!Number.isInteger(parsed) || parsed < 60 || parsed > 3_600) {
      throw refuse(
        "TIMEOUT_INVALID",
        "FACTORY_REAL_PROVIDER_TIMEOUT_SECS must be an integer between 60 and 3600",
      );
    }
    timeoutSecs = parsed;
  }

  const maxInputTokens = Number(env.FACTORY_REAL_PROVIDER_MAX_INPUT_TOKENS ?? "60000");
  const maxOutputTokens = Number(env.FACTORY_REAL_PROVIDER_MAX_OUTPUT_TOKENS ?? "12000");
  if (
    !Number.isInteger(maxInputTokens) ||
    maxInputTokens < 1 ||
    maxInputTokens > 1_000_000 ||
    !Number.isInteger(maxOutputTokens) ||
    maxOutputTokens < 1 ||
    maxOutputTokens > 100_000
  ) {
    throw refuse(
      "TOKEN_CEILING_INVALID",
      "FACTORY_REAL_PROVIDER_MAX_INPUT_TOKENS/MAX_OUTPUT_TOKENS are outside the supported bound",
    );
  }
  const maxTurns = Number(env.FACTORY_REAL_PROVIDER_MAX_TURNS ?? "4");
  if (!Number.isInteger(maxTurns) || maxTurns < 1 || maxTurns > 8) {
    throw refuse(
      "TURN_CEILING_INVALID",
      "FACTORY_REAL_PROVIDER_MAX_TURNS must be an integer between 1 and 8",
    );
  }

  let planApproval = "";
  if (command === "execute") {
    planApproval = requireString(env, "FACTORY_REAL_PROVIDER_PLAN_APPROVAL", {
      min: 13,
      max: 32,
    });
    if (planApproval !== "OWNER_REVIEWED") {
      throw refuse(
        "PLAN_APPROVAL_MISSING",
        "FACTORY_REAL_PROVIDER_PLAN_APPROVAL=OWNER_REVIEWED is required to execute",
      );
    }
  }

  const objective =
    env.FACTORY_REAL_PROVIDER_OBJECTIVE ??
    "Fix apply_discount in src/pricing.rs so the discount is applied exactly once, and add tests for zero, fractional, full and maximum-subtotal discounts. Do not change the public API.";
  if (
    typeof objective !== "string" ||
    objective.trim() !== objective ||
    objective.length < 20 ||
    objective.length > 2_000 ||
    objective.includes("\u0000")
  ) {
    throw refuse(
      "OBJECTIVE_INVALID",
      "FACTORY_REAL_PROVIDER_OBJECTIVE must be a bounded single-line objective",
    );
  }

  const repository =
    env.FACTORY_REAL_PROVIDER_REPOSITORY ?? "http://git-fixture:8080/repo.git";
  if (
    typeof repository !== "string" ||
    repository.length < 1 ||
    repository.length > 512 ||
    repository.includes("\u0000")
  ) {
    throw refuse("REPOSITORY_INVALID", "FACTORY_REAL_PROVIDER_REPOSITORY is invalid");
  }

  const candidateSha = env.FACTORY_REAL_PROVIDER_CANDIDATE_SHA ?? "";
  if (candidateSha && !/^[0-9a-f]{40}$/i.test(candidateSha)) {
    throw refuse(
      "CANDIDATE_SHA_INVALID",
      "FACTORY_REAL_PROVIDER_CANDIDATE_SHA must be a 40-hex Git SHA",
    );
  }
  const composeProject = env.FACTORY_REAL_PROVIDER_COMPOSE_PROJECT ?? "";
  const composeFiles =
    env.FACTORY_REAL_PROVIDER_COMPOSE_FILES ??
    "compose.yaml compose.real-provider-uat.yaml";

  return {
    command,
    planHash,
    authorization,
    accounting,
    extraUsage,
    capMicros: cap.micros,
    capUsd: cap.usd,
    coreApiUrl,
    evidenceDir,
    timeoutMs: timeoutSecs * 1_000,
    maxInputTokens,
    maxOutputTokens,
    maxTurns,
    planApproval,
    objective,
    repository,
    provider: PROVIDER,
    model: MODEL,
    baseUrl: PROVIDER_BASE_URL,
    candidateSha,
    composeProject,
    composeFiles,
  };
}

export function loadOperatorToken(env) {
  const file = env.FACTORY_OPERATOR_TOKEN_FILE;
  let token;
  if (typeof file === "string" && file.length > 0) {
    const path = resolve(file);
    let stat;
    try {
      stat = lstatSync(path);
    } catch {
      throw refuse("OPERATOR_TOKEN_UNAVAILABLE", "operator token file is unavailable");
    }
    if (stat.isSymbolicLink() || !stat.isFile()) {
      throw refuse(
        "OPERATOR_TOKEN_INVALID",
        "operator token file must be a regular non-symlink file",
      );
    }
    if (process.platform !== "win32" && (stat.mode & 0o077) !== 0) {
      throw refuse(
        "OPERATOR_TOKEN_PERMISSIONS",
        "operator token file permissions must not grant group/other access",
      );
    }
    try {
      token = readFileSync(path, "utf8").trim();
    } catch {
      throw refuse("OPERATOR_TOKEN_UNAVAILABLE", "operator token file could not be read");
    }
  } else if (typeof env.FACTORY_OPERATOR_TOKEN === "string") {
    token = env.FACTORY_OPERATOR_TOKEN.trim();
  }
  if (
    typeof token !== "string" ||
    token.length < 32 ||
    token.length > 8_192 ||
    /[\s\u0000-\u001f\u007f]/.test(token)
  ) {
    throw refuse(
      "OPERATOR_TOKEN_INVALID",
      "a local Core operator token of at least 32 bytes is required (file mode 0600 or environment)",
    );
  }
  return token;
}

export function redact(value, secrets) {
  let text = typeof value === "string" ? value : String(value);
  for (const secret of secrets) {
    if (secret) text = text.split(secret).join("[REDACTED]");
  }
  return text;
}

function redactDeep(value, secrets) {
  if (typeof value === "string") return redact(value, secrets);
  if (Array.isArray(value)) return value.map((item) => redactDeep(item, secrets));
  if (value && typeof value === "object") {
    const output = {};
    for (const [key, item] of Object.entries(value)) {
      output[key] = redactDeep(item, secrets);
    }
    return output;
  }
  return value;
}

const SENSITIVE_KEY = /(credential|token|secret|password|authorization|cookie)/i;

/// Error bodies are untrusted. Drop credential-shaped fields so a broken or
/// malicious upstream cannot make the harness log a secret it never received.
function sanitizeErrorPayload(value) {
  if (Array.isArray(value)) return value.map(sanitizeErrorPayload);
  if (value && typeof value === "object") {
    const output = {};
    for (const [key, item] of Object.entries(value)) {
      output[key] = SENSITIVE_KEY.test(key) ? "[REDACTED]" : sanitizeErrorPayload(item);
    }
    return output;
  }
  return value;
}

function writeJson(path, value) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
  try {
    chmodSync(path, 0o600);
  } catch {
    // Best-effort on filesystems without POSIX modes.
  }
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function ensureEvidenceDir(gate) {
  try {
    mkdirSync(gate.evidenceDir, { recursive: true, mode: 0o700 });
    const probe = join(gate.evidenceDir, ".write-probe");
    writeFileSync(probe, "ok\n", { mode: 0o600 });
    rmSync(probe, { force: true });
  } catch {
    throw new UatError(
      "EVIDENCE_DIR_UNAVAILABLE",
      "the evidence directory is not writable; no paid request was made",
      EXIT_REFUSED,
    );
  }
}

function nowIso() {
  return new Date().toISOString();
}

export function isDeepSeekPeak(date = new Date()) {
  const day = date.getUTCDay();
  if (day === 0 || day === 6) return false;
  const hour = date.getUTCHours();
  return (hour >= 1 && hour < 4) || (hour >= 6 && hour < 10);
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

export class CoreClient {
  constructor({ baseUrl, token, timeoutMs = 60_000 }) {
    this.baseUrl = baseUrl;
    this.token = token;
    this.timeoutMs = timeoutMs;
    this.secrets = new Set([token]);
    this.calls = [];
  }

  async request(method, path, body, options = {}) {
    const url = `${this.baseUrl}${path}`;
    const headers = {
      accept: "application/json",
      authorization: `Bearer ${this.token}`,
    };
    if (body !== undefined) headers["content-type"] = "application/json";
    const timeoutMs = options.timeoutMs ?? this.timeoutMs;
    let response;
    try {
      response = await fetch(url, {
        method,
        headers,
        body: body === undefined ? undefined : JSON.stringify(body),
        redirect: "error",
        signal: AbortSignal.timeout(timeoutMs),
      });
    } catch (error) {
      const safe = redact(error?.name === "TimeoutError" ? "request timed out" : String(error), this.secrets);
      throw new UatError(
        options.ambiguousCode ?? "CORE_TRANSPORT",
        `Core ${method} ${path} failed before a response: ${bounded(safe)}`,
        options.ambiguousExitCode ?? 1,
      );
    }
    const status = response.status;
    this.calls.push({ method, path, status });
    const text = await response.text().catch(() => "");
    let payload = null;
    if (text) {
      try {
        payload = JSON.parse(text);
      } catch {
        payload = text;
      }
    }
    const allowed = options.allowStatuses?.includes(status) ?? false;
    if (!response.ok && !allowed) {
      const sanitized = sanitizeErrorPayload(payload);
      const detail = bounded(
        redact(
          typeof sanitized === "string" ? sanitized : JSON.stringify(sanitized),
          this.secrets,
        ),
      );
      throw new UatError(
        options.statusCode ?? "CORE_HTTP",
        `Core ${method} ${path} -> ${status}: ${detail}`,
      );
    }
    return { status, payload };
  }

  get(path, options) {
    return this.request("GET", path, undefined, options);
  }

  post(path, body, options) {
    return this.request("POST", path, body, options);
  }

  put(path, body, options) {
    return this.request("PUT", path, body, options);
  }
}

function assert(condition, code, message) {
  if (!condition) throw new UatError(code, message);
}

export async function waitFor(label, operation, { timeoutMs, intervalMs = 2_000 }) {
  const deadline = Date.now() + timeoutMs;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const value = await operation();
      if (value) return value;
    } catch (error) {
      lastError = error;
    }
    await sleep(intervalMs);
  }
  throw new UatError(
    "TIMEOUT",
    `${label} timed out after ${timeoutMs}ms${lastError ? `: ${lastError.message}` : ""}`,
  );
}

function normalizeBaseUrl(value) {
  return String(value ?? "").replace(/\/+$/, "");
}

export async function preflight(client, gate) {
  const health = await client.get("/health");
  assert(health.status === 200 && health.payload?.healthy === true, "CORE_UNHEALTHY", "Core health is not healthy");

  const setup = await client.get("/setup/status");
  assert(
    setup.payload?.required === false,
    "SETUP_REQUIRED",
    "the disposable stack is not enrolled; use the prepare instructions and enroll the Owner device first",
  );

  const providers = await client.get("/settings/providers");
  const list = providers.payload?.providers;
  assert(Array.isArray(list), "PROVIDER_LIST_INVALID", "Core returned no provider list");
  const provider = list.find((item) => item?.provider === gate.provider);
  assert(provider, "PROVIDER_NOT_CONFIGURED", `${gate.provider} is not present in the provider catalog`);
  assert(provider.configured === true, "PROVIDER_CREDENTIAL_MISSING", `${gate.provider} has no stored credential`);
  assert(provider.enabled === true, "PROVIDER_DISABLED", `${gate.provider} is not enabled`);
  assert(provider.isDefault === true, "PROVIDER_NOT_DEFAULT", `${gate.provider} is not the default Core model route`);
  assert(
    provider.model === gate.model,
    "PROVIDER_MODEL_MISMATCH",
    `${gate.provider} is configured for ${provider.model}, not ${gate.model}`,
  );
  assert(
    normalizeBaseUrl(provider.baseUrl) === gate.baseUrl,
    "PROVIDER_BASE_URL_MISMATCH",
    `${gate.provider} is configured for ${provider.baseUrl}, not the pinned Go route`,
  );
  assert(
    Number(provider.maxTurns) <= gate.maxTurns,
    "PROVIDER_TURN_CEILING_EXCEEDED",
    `${gate.provider} allows ${provider.maxTurns} turns, above the authorized ${gate.maxTurns}`,
  );

  // Catalog discovery is a public read and performs no inference. It also
  // populates Core's process-local model cache, which the Worker assignment
  // requires before it will start a real provider session.
  const catalog = await client.get(`/settings/providers/${gate.provider}/models`, {
    timeoutMs: 30_000,
  });
  const catalogPayload = catalog.payload ?? {};
  assert(
    catalogPayload.status === "AVAILABLE",
    "PROVIDER_CATALOG_UNAVAILABLE",
    `${gate.provider} catalog is ${catalogPayload.status}: ${catalogPayload.detail ?? "no detail"}`,
  );
  assert(
    Array.isArray(catalogPayload.models) && catalogPayload.models.includes(gate.model),
    "PROVIDER_MODEL_UNAVAILABLE",
    `${gate.provider} catalog does not include ${gate.model}`,
  );
  assert(
    catalogPayload.credentialSignal !== "REJECTED",
    "PROVIDER_CREDENTIAL_REJECTED",
    `${gate.provider} catalog rejected the stored credential`,
  );

  const runtime = await client.get("/settings/runtime");
  assert(
    runtime.payload?.semanticReviewEnabled === true,
    "SEMANTIC_REVIEW_DISABLED",
    "semantic review is disabled; the independent reviewer would be DETERMINISTIC, not a real MODEL reviewer",
  );

  const workers = await client.get("/workers");
  assert(
    Array.isArray(workers.payload) && workers.payload.length > 0,
    "WORKER_UNAVAILABLE",
    "no Worker is registered for the disposable stack",
  );

  return {
    provider: {
      provider: provider.provider,
      configured: provider.configured === true,
      enabled: provider.enabled === true,
      isDefault: provider.isDefault === true,
      model: provider.model,
      baseUrl: provider.baseUrl,
      maxTurns: provider.maxTurns,
      maxInputTokens: provider.maxInputTokens,
      maxOutputTokens: provider.maxOutputTokens,
      maxCostMicros: provider.maxCostMicros,
      inputMicrosPerMillion: provider.inputMicrosPerMillion,
      outputMicrosPerMillion: provider.outputMicrosPerMillion,
    },
    catalog: {
      status: catalogPayload.status,
      source: catalogPayload.source,
      catalogUrl: catalogPayload.catalogUrl,
      modelAvailable:
        Array.isArray(catalogPayload.models) &&
        catalogPayload.models.includes(gate.model),
      credentialSignal: catalogPayload.credentialSignal,
      models: Array.isArray(catalogPayload.models)
        ? catalogPayload.models.filter((item) => item === gate.model)
        : [],
    },
    runtime: {
      semanticReviewEnabled: runtime.payload.semanticReviewEnabled === true,
      semanticReviewMaxOutputTokens: runtime.payload.semanticReviewMaxOutputTokens,
    },
    workers: workers.payload.map((worker) => worker.worker_id ?? worker.workerId ?? "unknown"),
  };
}

function gateEcho(gate) {
  return {
    authorization_id: gate.authorization,
    accounting: gate.accounting,
    extra_usage: gate.extraUsage,
    spend_cap_usd: gate.capUsd,
    spend_cap_micros: gate.capMicros,
    max_input_tokens: gate.maxInputTokens,
    max_output_tokens: gate.maxOutputTokens,
    max_turns: gate.maxTurns,
    provider: gate.provider,
    model: gate.model,
    base_url: gate.baseUrl,
    objective_sha256: sha256(gate.objective),
    repository: gate.repository,
    candidate_sha: gate.candidateSha || null,
    compose_project: gate.composeProject || null,
    compose_files: gate.composeFiles.split(/\s+/).filter(Boolean),
  };
}

function statePath(gate) {
  return join(gate.evidenceDir, "real-provider-uat-state.json");
}

function planPath(gate) {
  return join(gate.evidenceDir, "plan.json");
}

function evidencePath(gate) {
  return join(gate.evidenceDir, "real-provider-uat-evidence.json");
}

function loadState(gate) {
  try {
    return readJson(statePath(gate));
  } catch {
    return null;
  }
}

function ensureStateAuthorization(state, gate) {
  assert(
    state.authorization === gate.authorization,
    "STATE_AUTHORIZATION_MISMATCH",
    "the existing state belongs to a different authorization id; use a new evidence directory or a new authorization",
  );
  assert(
    state.spend_cap_micros === gate.capMicros,
    "STATE_CAP_MISMATCH",
    "the existing state has a different spend cap; do not widen an authorization silently",
  );
}

function safePlan(projection, gate) {
  const proposal = projection.proposal ?? {};
  return {
    schema: "factory.real-provider-uat.plan.v1",
    intake_request_id: projection.id,
    plan_hash: projection.plan?.plan_hash ?? null,
    objective: proposal.objective ?? projection.objective,
    class: proposal.class ?? null,
    risk_class: proposal.risk_class ?? null,
    route: proposal.route ?? { provider: gate.provider, model: gate.model, mode: "build" },
    budget: proposal.budget ?? null,
    quality_profile: proposal.quality_profile ?? null,
    assumptions: proposal.assumptions ?? [],
    risks: proposal.risks ?? [],
    acceptance_criteria: proposal.acceptance_criteria ?? [],
    negative_cases: proposal.negative_cases ?? [],
    work_units: Array.isArray(proposal.work_units)
      ? proposal.work_units.map((unit) => ({
          id: unit.id,
          objective: unit.objective,
          depends_on: unit.depends_on ?? [],
          allowed_paths: unit.allowed_paths ?? [],
        }))
      : [],
    frozen_at: projection.updatedAt ?? null,
  };
}

const INTAKE_TERMINAL = new Set(["AWAITING_APPROVAL", "COMPLETED", "BLOCKED", "FAILED", "CANCELLED"]);

export async function assess(client, gate) {
  const startedAt = new Date();
  ensureEvidenceDir(gate);
  const pre = await preflight(client, gate);
  const existing = loadState(gate);
  if (existing) {
    ensureStateAuthorization(existing, gate);
    if (existing.phase === "AWAITING_PLAN_APPROVAL") {
      const current = await client.get(`/intake/requests/${existing.intake_request_id}`);
      if (current.payload?.state === "AWAITING_APPROVAL") {
        return {
          result: "AWAITING_PLAN_APPROVAL",
          resumed: true,
          preflight: pre,
          state: existing,
          plan: safePlan(current.payload, gate),
        };
      }
      throw new UatError(
        "STATE_NOT_RESUMABLE",
        `the existing request is ${current.payload?.state}; use a new authorization id`,
      );
    }
    if (existing.phase !== "REQUEST_CREATED" && existing.phase !== "ASSESSING") {
      throw new UatError(
        "AUTHORIZATION_ALREADY_USED",
        `the existing state is ${existing.phase}; do not reuse one authorization for a second request`,
      );
    }
  }

  let state = existing;
  let projection;
  if (!state) {
    const projectResponse = await client.post("/projects", {
      repository: gate.repository,
      idempotency_key: `${gate.authorization}:project`,
    });
    const project = projectResponse.payload;
    assert(project?.id, "PROJECT_CREATE_FAILED", "Core did not return a Project id");

    const configBefore = await client.get(`/projects/${project.id}/config`);
    const configUpdated = await client.put(`/projects/${project.id}/config`, {
      expectedRevision: configBefore.payload.revision,
      allowedPaths: ["src", "tests"],
      capabilities: ["repository-edit", "generic-toolchain"],
      qualityProfile: "generic",
      active: true,
    });
    const projectConfig = configUpdated.payload;
    assert(projectConfig?.active === true, "PROJECT_CONFIG_FAILED", "Project configuration is not active");

    const role = { provider: gate.provider, model: gate.model, reasoning: "medium" };
    const profileResponse = await client.post("/execution-profiles", {
      name: `Real provider UAT ${gate.authorization}`,
      description: "Bounded real-provider Compose UAT profile",
      category: "BALANCED",
      roles: { lead: role, implementer: role, semanticReviewer: role },
      operations: {
        modelRequestTimeoutSecs: 300,
        toolCallTimeoutSecs: 600,
        maxRequestBytes: 2_097_152,
        maxResponseBytes: 2_097_152,
        maxGenerationTokens: 8_192,
        maxInFlightOperations: 2,
      },
      progress: {
        diagnoseAfterStagnantCheckpoints: 2,
        escalateAfterUnsuccessfulReplans: 1,
        observationTimeoutSecs: 300,
      },
      auto_start: { enabled: false, risk_classes: [] },
    });
    const profile = profileResponse.payload;
    assert(profile?.id, "PROFILE_CREATE_FAILED", "Core did not return an execution profile id");
    assert(profile.executable === true, "PROFILE_NOT_EXECUTABLE", "the execution profile is not executable");

    await client.post(`/execution-profiles/${profile.id}/default`, { scope: "global" });
    await client.post(`/projects/${project.id}/execution-policy`, {
      execution_profile_id: profile.id,
      expected_revision: profile.revision,
      expected_revision_hash: profile.contentHash,
      project_config_hash: projectConfig.configHash,
      acknowledge: true,
      idempotency_key: `${gate.authorization}:policy`,
    });

    const intakeResponse = await client.post("/intake/requests", {
      project_id: project.id,
      objective: gate.objective,
      execution_profile_id: profile.id,
      mode: "REVIEW_PLAN",
      idempotency_key: `${gate.authorization}:intake`,
      authorized_micros: gate.capMicros,
    });
    const intake = intakeResponse.payload;
    assert(intake?.id && intake?.leadRequestId, "INTAKE_CREATE_FAILED", "Core did not return an intake request");

    state = {
      schema: "factory.real-provider-uat.state.v1",
      phase: "REQUEST_CREATED",
      authorization: gate.authorization,
      accounting: gate.accounting,
      extra_usage: gate.extraUsage,
      spend_cap_usd: gate.capUsd,
      spend_cap_micros: gate.capMicros,
      started_at: startedAt.toISOString(),
      project_id: project.id,
      project_config_hash: projectConfig.configHash,
      project_base_commit: projectConfig.baseCommit,
      profile_id: profile.id,
      profile_revision_id: profile.revisionId,
      profile_revision: profile.revision,
      profile_hash: profile.contentHash,
      intake_request_id: intake.id,
      action_id: intake.actionId,
      lead_request_id: intake.leadRequestId,
      campaign_id: null,
      plan_hash: null,
    };
    writeJson(statePath(gate), state);
  }

  projection = await client.get(`/intake/requests/${state.intake_request_id}`);
  if (projection.payload?.state === "SUBMITTED" || projection.payload?.state === "ASSESSING") {
    if (state.phase === "REQUEST_CREATED") {
      state.phase = "ASSESSING";
      writeJson(statePath(gate), state);
      try {
        await client.post(`/intake/requests/${state.intake_request_id}/assess`, undefined, {
          timeoutMs: Math.min(gate.timeoutMs, DEFAULT_ASSESSMENT_TIMEOUT_MS),
        });
      } catch (error) {
        if (error.code !== "CORE_TRANSPORT") throw error;
        // The request may have reached Core. Do not retry; poll the durable
        // request state instead.
      }
    }
    projection = await waitFor(
      "real Lead assessment",
      async () => {
        const current = await client.get(`/intake/requests/${state.intake_request_id}`);
        return INTAKE_TERMINAL.has(current.payload?.state) ? current : null;
      },
      { timeoutMs: gate.timeoutMs, intervalMs: 2_000 },
    );
  }

  const result = projection.payload;
  assert(
    result?.state === "AWAITING_APPROVAL",
    "ASSESSMENT_NOT_READY",
    `intake request is ${result?.state}: ${result?.stateDetail ?? "no detail"}`,
  );
  const leadResponse = await client.get(`/lead/requests/${state.lead_request_id}`);
  assert(
    leadResponse.payload?.assessment?.mechanism === "MODEL",
    "LEAD_NOT_MODEL",
    `the Lead assessment mechanism is ${leadResponse.payload?.assessment?.mechanism}; a real MODEL assessment is required`,
  );
  const leadCalls = leadResponse.payload?.model_calls;
  assert(
    Array.isArray(leadCalls) &&
      leadCalls.some(
        (call) =>
          call.provider === gate.provider &&
          call.model === gate.model &&
          call.state === "SETTLED" &&
          call.usage_known === true,
      ),
    "LEAD_MODEL_CALL_MISSING",
    "no settled Lead model call used the authorized provider/model",
  );
  const planHash = result.plan?.plan_hash;
  assert(
    typeof planHash === "string" && /^[0-9a-f]{64}$/i.test(planHash),
    "PLAN_HASH_MISSING",
    "Core did not freeze a plan hash",
  );
  assert(
    result.proposal?.route?.provider === gate.provider &&
      result.proposal?.route?.model === gate.model,
    "PLAN_ROUTE_MISMATCH",
    `the frozen plan route is ${result.proposal?.route?.provider}/${result.proposal?.route?.model}, not the authorized route`,
  );
  assert(
    Number(result.proposal?.max_sessions) <= 1,
    "PLAN_SESSION_CEILING_EXCEEDED",
    `the frozen plan allows ${result.proposal?.max_sessions} sessions, above the authorized 1`,
  );
  assert(
    Number(result.proposal?.max_repair_cycles) === 0,
    "PLAN_REPAIR_CYCLE_NOT_AUTHORIZED",
    `the frozen plan allows ${result.proposal?.max_repair_cycles} repair cycles; this authorization allows none`,
  );

  state.phase = "AWAITING_PLAN_APPROVAL";
  state.plan_hash = planHash;
  state.assessed_at = nowIso();
  state.assessment_spend = result.spending?.status ?? null;
  writeJson(statePath(gate), state);
  const plan = safePlan(result, gate);
  writeJson(planPath(gate), plan);

  return {
    result: "AWAITING_PLAN_APPROVAL",
    resumed: Boolean(existing),
    preflight: pre,
    state,
    plan,
  };
}

function candidateTask(campaign) {
  const tasks = Array.isArray(campaign?.tasks) ? campaign.tasks : [];
  const candidates = Array.isArray(campaign?.candidates) ? campaign.candidates : [];
  return (
    tasks.find(
      (task) =>
        task?.status === "SUCCEEDED" &&
        candidates.some((candidate) => candidate?.id === task.candidate_id),
    ) ?? null
  );
}

function findReceipt(receipts, role) {
  return receipts.find((receipt) => receipt?.role === role) ?? null;
}

function numeric(value) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function safeModelCall(call) {
  return {
    reservation_id: call.reservation_id,
    role: call.role,
    state: call.state,
    usage_known: call.usage_known === true,
    input_tokens: numeric(call.input_tokens),
    output_tokens: numeric(call.output_tokens),
    total_tokens: numeric(call.total_tokens),
    cost_micros: numeric(call.cost_micros),
    max_cost_micros: numeric(call.max_cost_micros),
    provider: call.provider,
    model: call.model,
    purpose: call.purpose,
    created_at: call.created_at,
  };
}

function validateAccounting(gate, campaign, providerSessions, leadCalls, reviewCalls) {
  const status = campaign?.inference?.status;
  assert(status, "ACCOUNTING_UNAVAILABLE", "the Campaign has no inference envelope status");
  const settled = numeric(status.settledMicros);
  const reserved = numeric(status.reservedMicros);
  const ambiguous = numeric(status.ambiguousMicros);
  const overrun = numeric(status.overrunMicros);
  const unresolved = numeric(status.unresolvedCount);
  assert(settled <= gate.capMicros, "SPEND_CAP_EXCEEDED", `settled ${settled} exceeds the authorized ${gate.capMicros} micros`);
  assert(overrun === 0, "ACCOUNTING_OVERRUN", `the inference envelope overran by ${overrun} micros`);
  assert(ambiguous === 0, "ACCOUNTING_AMBIGUOUS", `${ambiguous} micros are ambiguous`);
  assert(unresolved === 0, "ACCOUNTING_UNRESOLVED", `${unresolved} inference operations are unresolved`);
  assert(reserved === 0, "ACCOUNTING_RESERVED", `${reserved} micros remain reserved after the Gate`);

  for (const call of [...leadCalls, ...reviewCalls]) {
    assert(call.state === "SETTLED", "MODEL_CALL_UNSETTLED", `model call ${call.reservation_id} is ${call.state}`);
    assert(call.usage_known === true, "MODEL_USAGE_UNKNOWN", `model call ${call.reservation_id} has unknown usage`);
    assert(call.provider === gate.provider && call.model === gate.model, "MODEL_ROUTE_MISMATCH", "a model call used an unauthorized route");
  }

  const completedSessions = providerSessions.filter((session) => session.state === "COMPLETED");
  assert(completedSessions.length >= 1, "PROVIDER_SESSION_MISSING", "no COMPLETED Implementer provider session was recorded");
  for (const session of providerSessions) {
    assert(session.provider === gate.provider && session.model === gate.model, "PROVIDER_SESSION_ROUTE_MISMATCH", "a provider session used an unauthorized route");
    assert(session.usage_known === true, "PROVIDER_USAGE_UNKNOWN", `provider session ${session.session_record_id} has unknown usage`);
    assert(
      typeof session.cleanup_receipt === "string" &&
        session.cleanup_receipt.length > 0 &&
        !session.cleanup_receipt.includes("unconfirmed"),
      "PROVIDER_CLEANUP_UNCONFIRMED",
      `provider session ${session.session_record_id} has no confirmed cleanup`,
    );
  }

  const totalInput =
    [...leadCalls, ...reviewCalls].reduce((total, call) => total + numeric(call.input_tokens), 0) +
    providerSessions.reduce((total, session) => total + numeric(session.input_tokens), 0);
  const totalOutput =
    [...leadCalls, ...reviewCalls].reduce((total, call) => total + numeric(call.output_tokens), 0) +
    providerSessions.reduce((total, session) => total + numeric(session.output_tokens), 0);
  assert(
    totalInput <= gate.maxInputTokens,
    "INPUT_TOKEN_CEILING_EXCEEDED",
    `reported input ${totalInput} exceeds the authorized ${gate.maxInputTokens}`,
  );
  assert(
    totalOutput <= gate.maxOutputTokens,
    "OUTPUT_TOKEN_CEILING_EXCEEDED",
    `reported output ${totalOutput} exceeds the authorized ${gate.maxOutputTokens}`,
  );

  return {
    basis: "CORE_CONFIGURED_RATE_AND_OPENCODE_MESSAGE_COST_ESTIMATES",
    tokens: {
      input: totalInput,
      output: totalOutput,
      max_input: gate.maxInputTokens,
      max_output: gate.maxOutputTokens,
    },
    envelope: {
      authorized_micros: numeric(status.authorizedMicros),
      settled_micros: settled,
      reserved_micros: reserved,
      ambiguous_micros: ambiguous,
      overrun_micros: overrun,
      available_micros: numeric(status.availableMicros),
      unresolved_count: unresolved,
      state: status.state,
      by_role: status.byRole ?? [],
      by_provider: status.byProvider ?? [],
    },
    model_calls: {
      configured_rate_micros: [...leadCalls, ...reviewCalls].reduce(
        (total, call) => total + numeric(call.cost_micros),
        0,
      ),
      lead_count: leadCalls.length,
      reviewer_count: reviewCalls.length,
    },
    provider_sessions: {
      opencode_message_cost_micros: providerSessions.reduce(
        (total, session) => total + numeric(session.cost_micros),
        0,
      ),
      cost_semantics:
        "OPENCODE_LOCAL_MESSAGE_COST_ESTIMATE_NOT_A_BALANCE_CHARGE_OR_INVOICE",
      allowance: "UNKNOWN_CORE_DOES_NOT_PERSIST_OPENCODE_ALLOWANCE",
      invoice: "UNKNOWN_NOT_AN_INVOICE",
    },
    console: {
      remaining_allowance_or_balance: "UNKNOWN_OWNER_MUST_READ_OPENCODE_CONSOLE",
      extra_usage: gate.extraUsage,
      accounting: gate.accounting,
    },
  };
}

async function executeInner(client, gate) {
  const startedAt = new Date();
  ensureEvidenceDir(gate);
  const state = loadState(gate);
  assert(state, "STATE_MISSING", "no assessment state exists; run assess first");
  ensureStateAuthorization(state, gate);
  assert(
    state.phase === "AWAITING_PLAN_APPROVAL" ||
      state.phase === "APPROVAL_SENT" ||
      state.phase === "EXECUTING",
    "STATE_NOT_EXECUTABLE",
    `the existing state is ${state.phase}; only an awaiting or executing state can continue`,
  );
  assert(
    state.plan_hash === gate.planHash,
    "PLAN_HASH_MISMATCH",
    "the command-line plan hash does not match the state file",
  );

  const pre = await preflight(client, gate);
  let projection = await client.get(`/intake/requests/${state.intake_request_id}`);
  assert(
    projection.payload?.state === "AWAITING_APPROVAL" || projection.payload?.state === "EXECUTING" || projection.payload?.state === "COMPLETED",
    "INTAKE_NOT_APPROVABLE",
    `intake request is ${projection.payload?.state}`,
  );
  assert(
    projection.payload?.plan?.plan_hash === gate.planHash,
    "PLAN_HASH_MISMATCH",
    "Core's frozen plan hash does not match the approved hash",
  );

  if (projection.payload.state === "AWAITING_APPROVAL") {
    state.phase = "APPROVAL_SENT";
    state.approval_rationale = `Owner reviewed the exact frozen plan ${gate.planHash} under authorization ${gate.authorization}.`;
    state.approved_at = nowIso();
    writeJson(statePath(gate), state);
    try {
      await client.post(`/intake/requests/${state.intake_request_id}/approve`, {
        expected_plan_hash: gate.planHash,
        expected_profile_revision_id: state.profile_revision_id,
        expected_profile_hash: state.profile_hash,
        rationale: state.approval_rationale,
        principal_label: "owner-real-provider-uat",
        idempotency_key: `${gate.authorization}:approve`,
        authorized_micros: gate.capMicros,
      });
    } catch (error) {
      // Approval is idempotent by key, but a transport error is ambiguous. Poll
      // the durable state; never send a blind retry.
      const current = await client.get(`/intake/requests/${state.intake_request_id}`).catch(() => null);
      if (!["EXECUTING", "COMPLETED"].includes(current?.payload?.state)) throw error;
    }
  }

  state.phase = "EXECUTING";
  writeJson(statePath(gate), state);

  const campaignId = await waitFor(
    "Campaign start",
    async () => {
      const current = await client.get(`/intake/requests/${state.intake_request_id}`);
      return current.payload?.campaignId ?? null;
    },
    { timeoutMs: gate.timeoutMs, intervalMs: 2_000 },
  );
  state.campaign_id = campaignId;
  writeJson(statePath(gate), state);

  const campaign = await waitFor(
    "Campaign candidate",
    async () => {
      const current = await client.get(`/campaigns/${campaignId}`);
      const detail = current.payload;
      const task = candidateTask(detail);
      return task ? detail : null;
    },
    { timeoutMs: gate.timeoutMs, intervalMs: 3_000 },
  );

  const task = candidateTask(campaign);
  assert(task, "CANDIDATE_MISSING", "the Campaign has no successful candidate task");
  const candidate = campaign.candidates.find((item) => item.id === task.candidate_id);
  assert(candidate, "CANDIDATE_MISSING", "the task candidate identity is absent");
  assert(
    typeof candidate.commit_sha === "string" && /^[0-9a-f]{40}$/i.test(candidate.commit_sha),
    "CANDIDATE_COMMIT_INVALID",
    "the candidate commit is not a 40-hex Git SHA",
  );
  state.candidate_id = candidate.id;
  state.candidate_commit = candidate.commit_sha;
  state.attempt_id = task.attempt_id;
  state.run_id = task.run_id ?? campaign.runs?.find((run) => run.id)?.id ?? null;
  writeJson(statePath(gate), state);

  const gateDecision = await waitFor(
    "deterministic Quality Gate",
    async () => {
      const current = await client.get(`/quality/candidates/${candidate.id}/gate`, {
        allowStatuses: [404],
      });
      if (current.status === 404) return null;
      return current.payload?.outcome && current.payload.outcome !== "PENDING" ? current.payload : null;
    },
    { timeoutMs: gate.timeoutMs, intervalMs: 3_000 },
  );
  state.last_gate = {
    outcome: gateDecision.outcome,
    reasons: gateDecision.reasons ?? [],
    candidate_id: candidate.id,
  };
  writeJson(statePath(gate), state);

  const receiptsResponse = await client.get(`/quality/candidates/${candidate.id}/receipts`);
  const receipts = Array.isArray(receiptsResponse.payload) ? receiptsResponse.payload : [];
  const verifier = findReceipt(receipts, "verifier");
  const reviewer = findReceipt(receipts, "reviewer");
  assert(verifier, "VERIFIER_RECEIPT_MISSING", "no independent verifier receipt exists");
  assert(reviewer, "REVIEWER_RECEIPT_MISSING", "no independent reviewer receipt exists");
  assert(verifier.result === "PASS", "VERIFIER_NOT_PASS", `verifier result is ${verifier.result}`);
  assert(verifier.evidence === "EXECUTED", "VERIFIER_NOT_EXECUTED", `verifier evidence is ${verifier.evidence}`);
  assert(
    verifier.commit_sha === candidate.commit_sha && verifier.tree_sha === candidate.tree_sha,
    "VERIFIER_BINDING_MISMATCH",
    "the verifier receipt is not bound to the exact candidate commit/tree",
  );
  assert(reviewer.result === "PASS", "REVIEWER_NOT_PASS", `reviewer result is ${reviewer.result}`);
  assert(reviewer.mechanism === "MODEL", "REVIEWER_NOT_MODEL", `reviewer mechanism is ${reviewer.mechanism}`);
  assert(
    reviewer.model_provider === gate.provider && reviewer.model_name === gate.model,
    "REVIEWER_ROUTE_MISMATCH",
    "the reviewer receipt does not name the authorized provider/model",
  );
  assert(reviewer.usage_known === true, "REVIEWER_USAGE_UNKNOWN", "the reviewer receipt has unknown usage");
  assert(
    reviewer.commit_sha === candidate.commit_sha && reviewer.tree_sha === candidate.tree_sha,
    "REVIEWER_BINDING_MISMATCH",
    "the reviewer receipt is not bound to the exact candidate commit/tree",
  );

  const participantsResponse = await client.get(
    `/office/participants?campaign_id=${encodeURIComponent(campaignId)}&limit=50`,
  );
  const participants = participantsResponse.payload?.participants ?? [];
  const implementer = participants.find((participant) => participant.role === "IMPLEMENTER");
  assert(implementer, "IMPLEMENTER_PARTICIPANT_MISSING", "the implementer participant is absent");
  assert(
    implementer.session?.state === "COMPLETED",
    "IMPLEMENTER_SESSION_NOT_COMPLETED",
    `implementer session is ${implementer.session?.state}`,
  );
  assert(
    typeof implementer.session?.provider_session_id === "string" &&
      implementer.session.provider_session_id.length > 0 &&
      !/fixture/i.test(implementer.session.provider_session_id),
    "IMPLEMENTER_NOT_REAL_PROVIDER",
    "the implementer session is a fixture or has no provider session id",
  );

  const sessionsResponse = await client.get("/provider/sessions");
  const allSessions = Array.isArray(sessionsResponse.payload) ? sessionsResponse.payload : [];
  const attemptIds = new Set(
    (campaign.attempts ?? []).map((attempt) => attempt.id).filter(Boolean),
  );
  const providerSessions = allSessions.filter((session) => attemptIds.has(session.attempt_id));
  assert(providerSessions.length >= 1, "PROVIDER_SESSION_MISSING", "no provider session belongs to this Campaign");

  const leadResponse = await client.get(`/lead/requests/${state.lead_request_id}`);
  const leadCalls = (leadResponse.payload?.model_calls ?? []).map(safeModelCall);
  const modelCallsResponse = await client.get("/model/calls?limit=500");
  const allModelCalls = Array.isArray(modelCallsResponse.payload) ? modelCallsResponse.payload : [];
  const reviewCalls = allModelCalls
    .filter(
      (call) =>
        call.role === "SEMANTIC_REVIEWER" &&
        (call.scope_id === candidate.id || call.objective_id === candidate.id),
    )
    .map(safeModelCall);
  assert(leadCalls.length >= 1, "LEAD_MODEL_CALL_MISSING", "no Lead model call was recorded");
  assert(reviewCalls.length >= 1, "REVIEWER_MODEL_CALL_MISSING", "no semantic reviewer model call was recorded");

  const accounting = validateAccounting(gate, campaign, providerSessions, leadCalls, reviewCalls);

  const diffResponse = await client.get(`/campaigns/${campaignId}/candidates/${candidate.id}/diff`);
  const diff = diffResponse.payload ?? {};
  const changedPaths = diff.changed_paths ?? diff.changedPaths ?? [];
  assert(Array.isArray(changedPaths) && changedPaths.length > 0, "CANDIDATE_DIFF_EMPTY", "the candidate diff has no changed paths");
  for (const path of changedPaths) {
    assert(
      typeof path === "string" && (path.startsWith("src/") || path.startsWith("tests/")),
      "CANDIDATE_PATH_OUTSIDE_SCOPE",
      `candidate changed a path outside src/ and tests/: ${path}`,
    );
  }

  const githubResponse = await client.get(`/projects/${state.project_id}/github`);
  const binding = githubResponse.payload?.binding ?? null;
  const prPreparation = binding
    ? {
        status: "BOUND_NOT_INSPECTED",
        write_attempted: false,
        note: "The project is bound to GitHub; this UAT never creates, merges or comments on a PR.",
      }
    : {
        status: "UNBOUND_READY",
        write_attempted: false,
        note: "The disposable project has no GitHub binding; the candidate commit/tree and diff are ready for separate PR authorization.",
      };

  const evidence = redactDeep(
    {
      schema: "factory.real-provider-uat.evidence.v1",
      result: gateDecision.outcome === "ACCEPTED" ? "ACCEPTED" : `GATE_${gateDecision.outcome}`,
      phase: "COMPLETE",
      started_at: startedAt.toISOString(),
      finished_at: nowIso(),
      peak_window_at_start: isDeepSeekPeak(startedAt),
      peak_window_at_finish: isDeepSeekPeak(new Date()),
      runtime: {
        candidate_sha: gate.candidateSha || null,
        compose_project: gate.composeProject || null,
        compose_files: gate.composeFiles.split(/\s+/).filter(Boolean),
      },
      preflight: pre,
      gate: {
        authorization_id: gate.authorization,
        accounting: gate.accounting,
        extra_usage: gate.extraUsage,
        spend_cap_usd: gate.capUsd,
        spend_cap_micros: gate.capMicros,
        max_input_tokens: gate.maxInputTokens,
        max_output_tokens: gate.maxOutputTokens,
        max_turns: gate.maxTurns,
        provider: gate.provider,
        model: gate.model,
        plan_hash: gate.planHash,
        plan_approval: gate.planApproval,
        approval_rationale: state.approval_rationale,
      },
      ids: {
        project: state.project_id,
        project_config_hash: state.project_config_hash,
        project_base_commit: state.project_base_commit,
        profile: state.profile_id,
        profile_revision_id: state.profile_revision_id,
        profile_revision: state.profile_revision,
        profile_hash: state.profile_hash,
        intake_request: state.intake_request_id,
        action: state.action_id,
        lead_request: state.lead_request_id,
        campaign: campaignId,
        run: state.run_id,
        attempt: state.attempt_id,
        candidate: candidate.id,
        candidate_commit: candidate.commit_sha,
        candidate_tree: candidate.tree_sha,
      },
      plan: safePlan(projection.payload ?? {}, gate),
      provider: {
        route: `${gate.provider}/${gate.model}`,
        implementer: {
          participant_worker_id: implementer.session.worker_id,
          provider_session_id: implementer.session.provider_session_id,
          cleanup_receipt: implementer.session.cleanup_receipt,
          state: implementer.session.state,
          usage: {
            input_tokens: numeric(implementer.session.input_tokens),
            output_tokens: numeric(implementer.session.output_tokens),
            cost_micros: numeric(implementer.session.cost_micros),
            cost_semantics:
              "OPENCODE_LOCAL_MESSAGE_COST_ESTIMATE_NOT_A_BALANCE_CHARGE",
          },
        },
        sessions: providerSessions.map((session) => ({
          session_record_id: session.session_record_id,
          provider_session_id: session.provider_session_id,
          provider: session.provider,
          model: session.model,
          mode: session.provider_mode,
          state: session.state,
          turn_id: session.turn_id,
          usage_known: session.usage_known === true,
          input_tokens: numeric(session.input_tokens),
          output_tokens: numeric(session.output_tokens),
          total_tokens: numeric(session.total_tokens),
          cost_micros: numeric(session.cost_micros),
          cleanup_receipt: session.cleanup_receipt,
        })),
      },
      model_calls: {
        lead: leadCalls,
        reviewer: reviewCalls,
      },
      receipts: {
        verifier: {
          receipt_id: verifier.receipt_id,
          principal_id: verifier.principal_id,
          result: verifier.result,
          evidence: verifier.evidence,
          commit_sha: verifier.commit_sha,
          tree_sha: verifier.tree_sha,
        },
        reviewer: {
          receipt_id: reviewer.receipt_id,
          principal_id: reviewer.principal_id,
          result: reviewer.result,
          mechanism: reviewer.mechanism,
          model_provider: reviewer.model_provider,
          model_name: reviewer.model_name,
          model_reservation_id: reviewer.model_reservation_id,
          usage_known: reviewer.usage_known === true,
          input_tokens: numeric(reviewer.input_tokens),
          output_tokens: numeric(reviewer.output_tokens),
          cost_micros: numeric(reviewer.cost_micros),
          commit_sha: reviewer.commit_sha,
          tree_sha: reviewer.tree_sha,
        },
      },
      quality_gate: {
        outcome: gateDecision.outcome,
        reasons: gateDecision.reasons ?? [],
        candidate_id: candidate.id,
      },
      accounting,
      candidate: {
        commit_sha: candidate.commit_sha,
        tree_sha: candidate.tree_sha,
        changed_paths: changedPaths,
        diff_base_commit: diff.base_commit ?? diff.baseCommit ?? null,
      },
      pr_preparation: prPreparation,
      boundary: {
        real_core_http_api: true,
        real_compose_stack: true,
        live_provider_inference: "CALLS_MADE",
        deterministic_model_fixture: false,
        github_pr_created_or_merged: false,
        github_write: "NONE",
        isolation: "NOT_CLAIMED",
        independent_acceptance: "NOT_CLAIMED",
      },
      commands: {
        assess: "scripts/real-provider-uat.sh assess",
        execute: `scripts/real-provider-uat.sh execute --plan-hash ${gate.planHash}`,
      },
      non_claims: [
        "Author-run UAT, not independent acceptance.",
        "No ISOLATED, SEC-01 or credential-confinement certification.",
        "Configured-rate micros are not an invoice.",
        "OpenCode Go allowance consumption is UNKNOWN to Core unless the Owner reads the Console.",
      ],
    },
    client.secrets,
  );
  writeJson(evidencePath(gate), evidence);

  return {
    result: evidence.result,
    evidence,
    evidencePath: evidencePath(gate),
    gateDecision,
  };
}

/// Execute the approved plan. Any failure after this point preserves a
/// minimal, redacted failure record so an accounting or gate failure is never
/// hidden by an exception.
export async function execute(client, gate) {
  const state = loadState(gate);
  try {
    return await executeInner(client, gate);
  } catch (error) {
    const failure = redactDeep(
      {
        schema: "factory.real-provider-uat.evidence.v1",
        result: `FAILED_${error?.code ?? "UNEXPECTED"}`,
        phase: "FAILED",
        finished_at: nowIso(),
        gate: {
          ...gateEcho(gate),
          plan_hash: gate.planHash,
          plan_approval: gate.planApproval,
        },
        state,
        error: {
          code: error?.code ?? "UNEXPECTED",
          message: error?.message ?? String(error),
        },
        boundary: {
          real_core_http_api: true,
          live_provider_inference: "CALLS_MAY_HAVE_BEEN_MADE",
          github_write: "NONE",
          isolation: "NOT_CLAIMED",
          independent_acceptance: "NOT_CLAIMED",
        },
        accounting: {
          status: "NOT_RECONCILED",
          note: "A failure before final reconciliation is not zero cost.",
        },
      },
      client.secrets,
    );
    writeJson(evidencePath(gate), failure);
    throw new UatError(
      error?.code ?? "EXECUTION_FAILED",
      `${error?.message ?? String(error)}; failure evidence at ${evidencePath(gate)}`,
      error?.exitCode ?? 1,
    );
  }
}

export async function main(argv = process.argv.slice(2), env = process.env) {
  const gate = parseGate(env, argv);
  const token = loadOperatorToken(env);
  activeSecrets.add(token);
  const client = new CoreClient({ baseUrl: gate.coreApiUrl, token });
  try {
    if (gate.command === "preflight") {
      ensureEvidenceDir(gate);
      const pre = await preflight(client, gate);
      const evidence = redactDeep(
        {
          schema: "factory.real-provider-uat.dry-run.v1",
          result: "DRY_RUN_READY",
          mode: "preflight",
          started_at: nowIso(),
          finished_at: nowIso(),
          ...gateEcho(gate),
          preflight: pre,
          requests: client.calls.map(({ method, path, status }) => ({ method, path, status })),
          paid_inference: false,
          mutations: false,
          non_claims: [
            "A catalog read is not inference evidence.",
            "No Project, Request, Campaign or provider session was created.",
          ],
        },
        client.secrets,
      );
      writeJson(join(gate.evidenceDir, "real-provider-uat-preflight.json"), evidence);
      printSummary("DRY_RUN_READY", [
        `Evidence: ${join(gate.evidenceDir, "real-provider-uat-preflight.json")}`,
        "No inference, Project, Request, Campaign or provider session was created.",
      ]);
      return { result: "DRY_RUN_READY", evidence };
    }
    if (gate.command === "assess") {
      const assessed = await assess(client, gate);
      printSummary("AWAITING_PLAN_APPROVAL", [
        `Project: ${assessed.state.project_id}`,
        `Request: ${assessed.state.intake_request_id}`,
        `Plan hash: ${assessed.plan.plan_hash}`,
        `Plan file: ${planPath(gate)}`,
        `Next: review plan.json, then run execute --plan-hash ${assessed.plan.plan_hash}`,
      ]);
      return assessed;
    }
    const executed = await execute(client, gate);
    printSummary(executed.result, [
      `Gate: ${executed.gateDecision.outcome}`,
      `Candidate: ${executed.evidence.ids.candidate_commit}`,
      `Evidence: ${executed.evidencePath}`,
    ]);
    if (executed.result !== "ACCEPTED") {
      process.exitCode = 1;
    }
    return executed;
  } finally {
    // Intentionally no token/header logging.
  }
}

function printSummary(result, lines) {
  process.stdout.write(`REAL_PROVIDER_UAT=${result}\n`);
  for (const line of lines) process.stdout.write(`${line}\n`);
}

const invokedDirectly =
  process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (invokedDirectly) {
  main().catch((error) => {
    const code = error?.code ?? "UNEXPECTED";
    const message = redact(error?.message ?? String(error), activeSecrets);
    process.stderr.write(`REAL_PROVIDER_UAT=REFUSED code=${code} ${bounded(message)}\n`);
    process.exit(typeof error?.exitCode === "number" ? error.exitCode : 1);
  });
}
