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
export const DEFAULT_ASSESSMENT_TIMEOUT_MS = 300_000;
export const DEFAULT_EXECUTION_TIMEOUT_MS = 1_800_000;
export const EXIT_REFUSED = 3;
export const EXIT_PAUSED = 4;
export const SUBSCRIPTION_POLICY = "SUBSCRIPTION_INCLUDED_ONLY";
export const VERIFICATION_SCHEMA = "factory.subscription-verification.v1";
const VERIFICATION_MAX_BYTES = 64 * 1024;
const DEFAULT_VERIFICATION_MAX_AGE_SECS = 86_400;
const SUBSCRIPTION_MODE = "GO_SUBSCRIPTION_INCLUDED";

const activeSecrets = new Set();

export class UatError extends Error {
  constructor(code, message, exitCode = 1) {
    super(message);
    this.name = "UatError";
    this.code = code;
    this.exitCode = exitCode;
  }
}

export class SubscriptionPausedError extends UatError {
  constructor(message, { stage = "EXECUTION", verification = null } = {}) {
    super("SUBSCRIPTION_QUOTA_EXHAUSTED", message, EXIT_PAUSED);
    this.name = "SubscriptionPausedError";
    this.stage = stage;
    this.verification = verification;
    this.paused = true;
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

export function refuseSuppliedSpendCap(env) {
  if (env.FACTORY_REAL_PROVIDER_SPEND_CAP_USD !== undefined) {
    throw refuse(
      "SPEND_CAP_NOT_APPLICABLE",
      "subscription-only mode does not accept a spend cap or positive USD authorization; remove FACTORY_REAL_PROVIDER_SPEND_CAP_USD",
    );
  }
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
  if (!["preflight", "assess", "execute", "resume"].includes(command)) {
    throw refuse(
      "USAGE",
      "usage: real-provider-uat.mjs <preflight|assess|execute --plan-hash SHA256|resume>",
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
  } else if (command === "resume" && argv.length !== 1) {
    throw refuse("USAGE", "resume accepts no arguments");
  }
  return { command, planHash };
}

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
  } else if (env.FACTORY_REAL_PROVIDER_OWNER_GO_AHEAD !== "1") {
    throw refuse(
      "OWNER_GO_AHEAD_MISSING",
      "a separate explicit Owner go-ahead is required before the first real request; set FACTORY_REAL_PROVIDER_OWNER_GO_AHEAD=1 after Console verification",
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
  if (accounting !== SUBSCRIPTION_MODE) {
    throw refuse(
      "PAID_MODE_DISABLED",
      "the current funding policy permits only GO_SUBSCRIPTION_INCLUDED; billable and pay-as-you-go modes are disabled before any request",
    );
  }
  refuseSuppliedSpendCap(env);
  const extraUsage = requireString(env, "FACTORY_REAL_PROVIDER_EXTRA_USAGE", {
    max: 64,
  });
  if (extraUsage !== "DISABLED") {
    throw refuse(
      "EXTRA_USAGE_NOT_DISABLED",
      "FACTORY_REAL_PROVIDER_EXTRA_USAGE must be DISABLED; enabled, unknown or unverifiable Extra Usage is refused before any request",
    );
  }

  const coreApiUrl = parseCoreUrl(
    requireString(env, "FACTORY_CORE_API_URL", { max: 512 }),
  );
  const evidenceDir = resolve(
    requireString(env, "FACTORY_REAL_PROVIDER_EVIDENCE_DIR", { max: 4096 }),
  );
  if (!isAbsolute(evidenceDir)) {
    throw refuse("EVIDENCE_DIR_INVALID", "evidence directory must be absolute");
  }
  const subscriptionEvidenceFile = resolve(
    requireString(env, "FACTORY_REAL_PROVIDER_SUBSCRIPTION_EVIDENCE_FILE", {
      max: 4096,
    }),
  );

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

  let verificationMaxAgeSecs = DEFAULT_VERIFICATION_MAX_AGE_SECS;
  if (env.FACTORY_REAL_PROVIDER_VERIFICATION_MAX_AGE_SECS !== undefined) {
    const parsed = Number(env.FACTORY_REAL_PROVIDER_VERIFICATION_MAX_AGE_SECS);
    if (!Number.isInteger(parsed) || parsed < 60 || parsed > 604_800) {
      throw refuse(
        "VERIFICATION_AGE_INVALID",
        "FACTORY_REAL_PROVIDER_VERIFICATION_MAX_AGE_SECS must be an integer between 60 and 604800",
      );
    }
    verificationMaxAgeSecs = parsed;
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
    fundingPolicy: SUBSCRIPTION_POLICY,
    mode: SUBSCRIPTION_MODE,
    coreApiUrl,
    evidenceDir,
    subscriptionEvidenceFile,
    verificationMaxAgeSecs,
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

const VERIFICATION_KEYS = new Set([
  "schema",
  "provider",
  "model",
  "route",
  "plan",
  "account_fingerprint",
  "source",
  "verification_method",
  "console_url",
  "observed_at",
  "valid_until",
  "entitlement",
  "extra_usage",
  "balance_fallback",
  "no_charge_route",
  "quota",
  "core_verifiable",
  "limitation",
  "rate_revision",
  "notes",
]);
const QUOTA_KEYS = new Set(["state", "remaining_usd", "resets_at"]);
const SUBSCRIPTION_PLANS = new Set(["Go", "Go Plus", "Go Teams"]);

function assertNoUnknownKeys(value, allowed, label) {
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) {
      throw refuse(
        "SUBSCRIPTION_VERIFICATION_INVALID",
        `${label} contains an unknown field; refusing to interpret it`,
      );
    }
  }
}

function boundedRegularFile(path, label, maxBytes) {
  let stat;
  try {
    stat = lstatSync(path);
  } catch {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_MISSING",
      `${label} is unavailable; owner-verified Console evidence is required before any request`,
    );
  }
  if (stat.isSymbolicLink() || !stat.isFile()) {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_INVALID",
      `${label} must be a regular non-symlink file`,
    );
  }
  if (process.platform !== "win32" && (stat.mode & 0o077) !== 0) {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_PERMISSIONS",
      `${label} permissions must not grant group/other access`,
    );
  }
  if (stat.size > maxBytes) {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_INVALID",
      `${label} exceeds its bounded size`,
    );
  }
  try {
    return readFileSync(path, "utf8");
  } catch {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_MISSING",
      `${label} could not be read`,
    );
  }
}

function parseTimestamp(value, label) {
  const parsed = Date.parse(value);
  if (!Number.isFinite(parsed)) {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_INVALID",
      `${label} must be an ISO-8601 timestamp`,
    );
  }
  return parsed;
}

/// Read the Owner-verified subscription evidence. This file is not a Core
/// verification receipt: the current Core/local preflight cannot inspect the
/// OpenCode Console. The limitation is recorded in every evidence projection.
export function loadSubscriptionVerification(gate) {
  const raw = boundedRegularFile(
    gate.subscriptionEvidenceFile,
    "FACTORY_REAL_PROVIDER_SUBSCRIPTION_EVIDENCE_FILE",
    VERIFICATION_MAX_BYTES,
  );
  let record;
  try {
    record = JSON.parse(raw);
  } catch {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_INVALID",
      "subscription evidence is not valid JSON",
    );
  }
  if (!record || typeof record !== "object" || Array.isArray(record)) {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_INVALID",
      "subscription evidence must be a JSON object",
    );
  }
  assertNoUnknownKeys(record, VERIFICATION_KEYS, "subscription evidence");
  if (record.schema !== VERIFICATION_SCHEMA) {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_INVALID",
      `subscription evidence schema must be ${VERIFICATION_SCHEMA}`,
    );
  }
  if (record.provider !== gate.provider || record.model !== gate.model) {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_BINDING_MISMATCH",
      "subscription evidence does not bind the authorized provider/model",
    );
  }
  if (record.route !== SUBSCRIPTION_MODE) {
    throw refuse(
      "PAID_MODE_DISABLED",
      "subscription evidence must record GO_SUBSCRIPTION_INCLUDED; paid routes are disabled",
    );
  }
  if (!SUBSCRIPTION_PLANS.has(record.plan)) {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_INVALID",
      "subscription plan must be Go, Go Plus or Go Teams",
    );
  }
  if (
    typeof record.account_fingerprint !== "string" ||
    !/^[A-Za-z0-9:_-]{8,128}$/.test(record.account_fingerprint)
  ) {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_INVALID",
      "subscription account fingerprint must be a bounded non-secret identifier",
    );
  }
  if (!["OWNER_CONSOLE", "CORE"].includes(record.source)) {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_INVALID",
      "subscription evidence source must be OWNER_CONSOLE or CORE",
    );
  }
  if (typeof record.core_verifiable !== "boolean") {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_INVALID",
      "subscription evidence must declare whether Core can verify it",
    );
  }
  if (record.source === "OWNER_CONSOLE") {
    if (record.core_verifiable !== false) {
      throw refuse(
        "SUBSCRIPTION_VERIFICATION_INVALID",
        "owner Console evidence must not claim Core verification",
      );
    }
    if (record.verification_method !== "OWNER_CONSOLE_INSPECTION") {
      throw refuse(
        "SUBSCRIPTION_VERIFICATION_INVALID",
        "owner Console evidence must use OWNER_CONSOLE_INSPECTION",
      );
    }
    if (
      typeof record.console_url !== "string" ||
      !record.console_url.startsWith("https://opencode.ai/console") ||
      typeof record.limitation !== "string" ||
      record.limitation.length < 8
    ) {
      throw refuse(
        "SUBSCRIPTION_VERIFICATION_INVALID",
        "owner Console evidence requires the Console URL and an explicit verification limitation",
      );
    }
  } else {
    if (record.core_verifiable !== true || record.verification_method !== "CORE_EXPORT") {
      throw refuse(
        "SUBSCRIPTION_VERIFICATION_INVALID",
        "Core-sourced evidence must declare CORE_EXPORT and core_verifiable=true",
      );
    }
  }
  if (
    record.entitlement !== "VERIFIED" ||
    record.extra_usage !== "OFF" ||
    record.balance_fallback !== "OFF" ||
    record.no_charge_route !== "VERIFIED"
  ) {
    throw refuse(
      "SUBSCRIPTION_ENTITLEMENT_UNVERIFIED",
      "subscription entitlement, Extra Usage OFF, balance fallback OFF and no-charge route VERIFIED are all required",
    );
  }
  if (
    record.rate_revision !== undefined &&
    (typeof record.rate_revision !== "string" ||
      record.rate_revision.length < 1 ||
      record.rate_revision.length > 512)
  ) {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_INVALID",
      "subscription rate_revision must be a bounded string",
    );
  }
  if (
    record.notes !== undefined &&
    (typeof record.notes !== "string" || record.notes.length > 2_000)
  ) {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_INVALID",
      "subscription notes must be a bounded string",
    );
  }
  if (!record.quota || typeof record.quota !== "object" || Array.isArray(record.quota)) {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_INVALID",
      "subscription evidence must include a quota observation",
    );
  }
  assertNoUnknownKeys(record.quota, QUOTA_KEYS, "subscription quota");
  if (!["AVAILABLE", "EXHAUSTED", "UNKNOWN"].includes(record.quota.state)) {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_INVALID",
      "subscription quota state must be AVAILABLE, EXHAUSTED or UNKNOWN",
    );
  }
  if (
    record.quota.remaining_usd !== undefined &&
    record.quota.remaining_usd !== null &&
    (typeof record.quota.remaining_usd !== "number" ||
      !Number.isFinite(record.quota.remaining_usd) ||
      record.quota.remaining_usd < 0)
  ) {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_INVALID",
      "subscription quota remaining_usd must be null or a non-negative number",
    );
  }
  if (record.quota.resets_at !== undefined && record.quota.resets_at !== null) {
    parseTimestamp(record.quota.resets_at, "subscription quota resets_at");
  }
  const observedMs = parseTimestamp(record.observed_at, "subscription observed_at");
  const validUntilMs = parseTimestamp(record.valid_until, "subscription valid_until");
  const now = Date.now();
  if (observedMs > now + 300_000) {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_INVALID",
      "subscription evidence observed_at is in the future",
    );
  }
  if (validUntilMs <= now) {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_STALE",
      "subscription evidence has expired; refresh the Owner Console verification",
    );
  }
  if (now - observedMs > gate.verificationMaxAgeSecs * 1_000) {
    throw refuse(
      "SUBSCRIPTION_VERIFICATION_STALE",
      "subscription evidence is older than the allowed verification age; refresh the Owner Console verification",
    );
  }
  if (record.quota.state === "EXHAUSTED") {
    throw new SubscriptionPausedError(
      "subscription quota is exhausted according to the Owner Console evidence; no dispatch was attempted",
      { stage: "PRE_DISPATCH", verification: record },
    );
  }
  if (
    record.quota.state === "AVAILABLE" &&
    typeof record.quota.remaining_usd === "number" &&
    record.quota.remaining_usd <= 0
  ) {
    throw new SubscriptionPausedError(
      "subscription quota reports zero remaining; no dispatch was attempted",
      { stage: "PRE_DISPATCH", verification: record },
    );
  }
  return {
    record,
    file: gate.subscriptionEvidenceFile,
    sha256: sha256(raw),
    observedMs,
    validUntilMs,
    ageMs: now - observedMs,
    limitation:
      record.source === "OWNER_CONSOLE"
        ? "CORE_AND_LOCAL_PREFLIGHT_CANNOT_VERIFY_CONSOLE_EXTRA_USAGE"
        : null,
  };
}

export function summarizeVerification(verification) {
  const record = verification.record;
  return {
    schema: record.schema,
    source: record.source,
    verification_method: record.verification_method,
    core_verifiable: record.core_verifiable,
    plan: record.plan,
    account_fingerprint: record.account_fingerprint,
    console_url: record.console_url ?? null,
    observed_at: record.observed_at,
    valid_until: record.valid_until,
    age_seconds: Math.floor(verification.ageMs / 1_000),
    entitlement: record.entitlement,
    extra_usage: record.extra_usage,
    balance_fallback: record.balance_fallback,
    no_charge_route: record.no_charge_route,
    quota: {
      state: record.quota.state,
      remaining_usd: record.quota.remaining_usd ?? null,
      resets_at: record.quota.resets_at ?? null,
    },
    rate_revision: record.rate_revision ?? null,
    limitation: verification.limitation,
    evidence_sha256: verification.sha256,
  };
}

export async function loadCoreSubscriptionVerification(client, gate) {
  const response = await client.get(
    `/funding/subscription/${encodeURIComponent(gate.provider)}`,
    { allowStatuses: [404, 405] },
  );
  if (response.status === 404 || response.status === 405) {
    throw new UatError(
      "CORE_V3_SUBSCRIPTION_ADMISSION_REQUIRED",
      "Core does not expose the V3 subscription funding verification; no Project or Request was created",
      1,
    );
  }
  const record = response.payload;
  assert(
    record && typeof record === "object" && !Array.isArray(record),
    "CORE_SUBSCRIPTION_VERIFICATION_INVALID",
    "Core returned no subscription funding verification object",
  );
  assert(
    record.provider === gate.provider &&
      record.model === gate.model &&
      record.route === SUBSCRIPTION_MODE,
    "CORE_SUBSCRIPTION_VERIFICATION_MISMATCH",
    "Core subscription verification does not bind the authorized provider/model/route",
  );
  assert(
    record.plan === gate.verification.record.plan &&
      record.account_fingerprint ===
        gate.verification.record.account_fingerprint,
    "SUBSCRIPTION_BINDING_CHANGED",
    "Core subscription verification differs from the Owner Console evidence",
  );
  assert(
    record.core_verifiable === true &&
      record.entitlement === "VERIFIED" &&
      record.extra_usage === "OFF" &&
      record.balance_fallback === "OFF" &&
      record.no_charge_route === "VERIFIED",
    "CORE_SUBSCRIPTION_ENTITLEMENT_UNVERIFIED",
    "Core subscription verification does not prove entitlement, Extra Usage OFF, balance fallback OFF and no-charge routing",
  );
  assert(
    record.quota &&
      ["AVAILABLE", "UNKNOWN", "EXHAUSTED"].includes(record.quota.state) &&
      (record.quota.remaining_usd === undefined ||
        record.quota.remaining_usd === null ||
        (typeof record.quota.remaining_usd === "number" &&
          Number.isFinite(record.quota.remaining_usd) &&
          record.quota.remaining_usd >= 0)),
    "CORE_SUBSCRIPTION_QUOTA_INVALID",
    "Core subscription verification has no valid quota observation",
  );
  if (record.quota.state === "EXHAUSTED") {
    throw new SubscriptionPausedError(
      "Core subscription verification reports quota exhausted; no dispatch was attempted",
      { stage: "PRE_DISPATCH", verification: record },
    );
  }
  if (
    record.quota.state === "AVAILABLE" &&
    typeof record.quota.remaining_usd === "number" &&
    record.quota.remaining_usd <= 0
  ) {
    throw new SubscriptionPausedError(
      "Core subscription verification reports zero remaining quota; no dispatch was attempted",
      { stage: "PRE_DISPATCH", verification: record },
    );
  }
  if (typeof record.valid_until === "string") {
    const validUntil = Date.parse(record.valid_until);
    if (Number.isFinite(validUntil) && validUntil <= Date.now()) {
      throw refuse(
        "SUBSCRIPTION_VERIFICATION_STALE",
        "Core subscription verification has expired; refresh the Owner Console/Core verification",
      );
    }
  }
  return {
    provider: record.provider,
    model: record.model,
    route: record.route,
    plan: record.plan,
    account_fingerprint: record.account_fingerprint,
    entitlement: record.entitlement,
    extra_usage: record.extra_usage,
    balance_fallback: record.balance_fallback,
    no_charge_route: record.no_charge_route,
    quota: {
      state: record.quota.state,
      remaining_usd: record.quota.remaining_usd ?? null,
      resets_at: record.quota.resets_at ?? null,
    },
    observed_at: record.observed_at ?? null,
    valid_until: record.valid_until ?? null,
    core_verifiable: true,
  };
}

export function isQuotaPausedPayload(payload) {
  const state = String(payload?.state ?? "");
  if (state !== "PAUSED" && state !== "PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED") {
    return false;
  }
  const reason = [
    payload?.pauseReason,
    payload?.stateDetail,
    payload?.reason,
    payload?.error,
  ]
    .filter((value) => typeof value === "string")
    .join(" ");
  return /SUBSCRIPTION_QUOTA_EXHAUSTED|QUOTA_EXHAUSTED/i.test(reason);
}

export function isQuotaError(error) {
  if (!(error instanceof UatError)) return false;
  const text = `${error.code} ${error.message}`;
  return /SUBSCRIPTION_QUOTA_EXHAUSTED|QUOTA_EXHAUSTED/i.test(text);
}

export function classifySubscriptionAdmissionError(error) {
  if (
    error instanceof UatError &&
    /INFERENCE_AUTHORIZATION_REQUIRED|INFERENCE_AUTHORIZATION_NOT_APPLICABLE/i.test(
      error.message,
    )
  ) {
    return new UatError(
      "CORE_V3_SUBSCRIPTION_ADMISSION_REQUIRED",
      "Core does not admit subscription-only inference without a positive monetary envelope; do not add a cap, select billable mode or fall back",
      1,
    );
  }
  return error;
}

function throwIfQuota(error, stage) {
  if (isQuotaError(error)) {
    throw new SubscriptionPausedError(error.message, { stage });
  }
  throw error;
}

export function readActualCharge(record, kind, id) {
  const known =
    record?.actual_charge_known === true ||
    record?.provider_reported_actual_charge_known === true;
  const rawMicros =
    record?.actual_charge_micros ??
    record?.provider_reported_actual_charge_micros;
  const billing = record?.billing_source ?? record?.billingSource;
  const normalizedBilling =
    typeof billing === "string" ? billing.toLowerCase() : null;
  const paidBilling =
    normalizedBilling === "balance" || normalizedBilling === "byok";

  if (known) {
    const micros = Number(rawMicros);
    if (!Number.isFinite(micros) || micros < 0) {
      return {
        observation: "UNKNOWN",
        micros: null,
        reason: "INVALID_ACTUAL_CHARGE_EVIDENCE",
        source: `${kind}:${id}`,
      };
    }
    if (micros > 0) {
      return {
        observation: "VERIFIED_POSITIVE",
        micros,
        provenance: "PROVIDER_REPORTED_ACTUAL_CHARGE",
        fallbackObserved: paidBilling,
        source: `${kind}:${id}`,
      };
    }
    if (paidBilling) {
      return {
        observation: "UNKNOWN",
        micros: 0,
        reason: "PAID_BILLING_SOURCE_OBSERVED",
        fallbackObserved: true,
        source: `${kind}:${id}`,
      };
    }
    return {
      observation: "VERIFIED_ZERO",
      micros: 0,
      provenance: "PROVIDER_REPORTED_ACTUAL_CHARGE",
      source: `${kind}:${id}`,
    };
  }

  if (paidBilling) {
    return {
      observation: "UNKNOWN",
      micros: null,
      reason: "PAID_BILLING_SOURCE_OBSERVED",
      fallbackObserved: true,
      source: `${kind}:${id}`,
    };
  }
  if (typeof billing === "string") {
    if (["go", "go-plus", "go-teams", "subscription", "free"].includes(normalizedBilling)) {
      return {
        observation: "VERIFIED_ZERO",
        micros: 0,
        provenance: `BILLING_SOURCE_${normalizedBilling.toUpperCase().replaceAll("-", "_")}`,
        source: `${kind}:${id}`,
      };
    }
  }
  return {
    observation: "UNKNOWN",
    micros: null,
    reason: "CORE_DOES_NOT_PERSIST_RAW_PROVIDER_CHARGE",
    source: `${kind}:${id}`,
  };
}

export function summarizeActualCharge(records) {
  const observations = records.map(({ record, kind, id }) =>
    readActualCharge(record, kind, id),
  );
  const positive = observations.find(
    (item) => item.observation === "VERIFIED_POSITIVE",
  );
  const fallback = observations.find((item) => item.fallbackObserved === true);
  if (positive) {
    return {
      observation: "VERIFIED_POSITIVE",
      micros: positive.micros,
      provenance: positive.provenance,
      fallback_observed: Boolean(fallback),
      observations,
    };
  }
  if (fallback) {
    return {
      observation: "UNKNOWN",
      micros: null,
      reason: "PAID_FALLBACK_OBSERVED_WITHOUT_AMOUNT",
      fallback_observed: true,
      observations,
    };
  }
  if (
    observations.length > 0 &&
    observations.every((item) => item.observation === "VERIFIED_ZERO")
  ) {
    return {
      observation: "VERIFIED_ZERO",
      micros: 0,
      provenance: "PROVIDER_REPORTED_OR_BILLING_SOURCE_VALUE",
      fallback_observed: false,
      observations,
    };
  }
  return {
    observation: "UNKNOWN",
    micros: null,
    reason: "CORE_DOES_NOT_PERSIST_RAW_PROVIDER_CHARGE",
    fallback_observed: false,
    observations,
  };
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
      if (error?.paused === true) throw error;
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
  const verification = gate.verification;
  assert(
    verification,
    "SUBSCRIPTION_VERIFICATION_UNAVAILABLE",
    "owner-verified subscription evidence was not loaded; no request was made",
  );
  const health = await client.get("/health");
  assert(health.status === 200 && health.payload?.healthy === true, "CORE_UNHEALTHY", "Core health is not healthy");

  const setup = await client.get("/setup/status");
  assert(
    setup.payload?.required === false,
    "SETUP_REQUIRED",
    "the disposable stack is not enrolled; use the prepare instructions and enroll the Owner device first",
  );

  const coreSubscription = await loadCoreSubscriptionVerification(client, gate);

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
      maxCostMicrosSemantics: "PROVIDER_SETTINGS_OBSERVATION_NOT_A_SPEND_AUTHORIZATION",
      inputMicrosPerMillion: provider.inputMicrosPerMillion,
      outputMicrosPerMillion: provider.outputMicrosPerMillion,
      revision: provider.revision ?? null,
      updatedAt: provider.updatedAt ?? null,
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
    subscription: {
      policy: SUBSCRIPTION_POLICY,
      mode: SUBSCRIPTION_MODE,
      positive_usd_authorization: "NOT_REQUIRED",
      spend_cap: "NOT_USED",
      verification: summarizeVerification(verification),
      core_verification: "AVAILABLE_V3_SUBSCRIPTION_BINDING",
      core_subscription: coreSubscription,
      limitation: verification.limitation,
    },
  };
}

function gateEcho(gate) {
  return {
    authorization_id: gate.authorization,
    funding_policy: gate.fundingPolicy,
    accounting: gate.accounting,
    extra_usage: gate.extraUsage,
    positive_usd_authorization: "NOT_REQUIRED",
    spend_cap: "NOT_USED",
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
    subscription_evidence_sha256: gate.verification?.sha256 ?? null,
    subscription_plan: gate.verification?.record?.plan ?? null,
    subscription_account_fingerprint:
      gate.verification?.record?.account_fingerprint ?? null,
    subscription_source: gate.verification?.record?.source ?? null,
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

const STATE_SCHEMA = "factory.real-provider-uat.state.v2";

function ensureStateAuthorization(state, gate) {
  assert(
    state.schema === STATE_SCHEMA,
    "STATE_VERSION_MISMATCH",
    "the existing state predates subscription-only funding; use a new evidence directory and authorization",
  );
  assert(
    state.authorization === gate.authorization,
    "STATE_AUTHORIZATION_MISMATCH",
    "the existing state belongs to a different authorization id; use a new evidence directory or a new authorization",
  );
  assert(
    state.funding?.policy === gate.fundingPolicy &&
      state.funding?.mode === gate.mode,
    "STATE_FUNDING_MISMATCH",
    "the existing state has a different funding policy; do not change funding mode silently",
  );
  assert(
    state.subscription?.provider === gate.provider &&
      state.subscription?.model === gate.model &&
      state.subscription?.plan === gate.verification.record.plan &&
      state.subscription?.account_fingerprint ===
        gate.verification.record.account_fingerprint,
    "SUBSCRIPTION_BINDING_CHANGED",
    "the subscription provider, model, plan or account differs from the frozen run; require explicit reauthorization",
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
    budget_semantics: "MODEL_PLAN_OBSERVATION_NOT_A_SPEND_AUTHORIZATION",
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

const INTAKE_TERMINAL = new Set([
  "AWAITING_APPROVAL",
  "PAUSED",
  "COMPLETED",
  "BLOCKED",
  "FAILED",
  "CANCELLED",
]);

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
    });
    const intake = intakeResponse.payload;
    assert(intake?.id && intake?.leadRequestId, "INTAKE_CREATE_FAILED", "Core did not return an intake request");

    state = {
      schema: STATE_SCHEMA,
      phase: "REQUEST_CREATED",
      authorization: gate.authorization,
      funding: {
        policy: gate.fundingPolicy,
        mode: gate.mode,
        positive_usd_authorization: "NOT_REQUIRED",
        spend_cap: "NOT_USED",
      },
      accounting: gate.accounting,
      extra_usage: gate.extraUsage,
      subscription: {
        provider: gate.provider,
        model: gate.model,
        plan: gate.verification.record.plan,
        account_fingerprint: gate.verification.record.account_fingerprint,
        evidence_sha256: gate.verification.sha256,
        source: gate.verification.record.source,
        core_verifiable: gate.verification.record.core_verifiable,
      },
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
        if (error.code !== "CORE_TRANSPORT") throwIfQuota(error, "ASSESSMENT");
        // The request may have reached Core. Do not retry; poll the durable
        // request state instead.
      }
    }
    projection = await waitFor(
      "real Lead assessment",
      async () => {
        const current = await client.get(`/intake/requests/${state.intake_request_id}`);
        if (isQuotaPausedPayload(current.payload)) {
          throw new SubscriptionPausedError(
            "Core paused the request with SUBSCRIPTION_QUOTA_EXHAUSTED",
            { stage: "ASSESSMENT", verification: gate.verification.record },
          );
        }
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
  state.assessment_spend_observation = result.spending?.status ?? null;
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
    actual_charge_micros: call.actual_charge_micros ?? null,
    actual_charge_known: call.actual_charge_known === true,
    provider_reported_actual_charge_micros:
      call.provider_reported_actual_charge_micros ?? null,
    provider_reported_actual_charge_known:
      call.provider_reported_actual_charge_known === true,
    billing_source: call.billing_source ?? call.billingSource ?? null,
    provider: call.provider,
    model: call.model,
    purpose: call.purpose,
    created_at: call.created_at,
  };
}

function buildFundingUsageEvidence(
  gate,
  preflight,
  startedAt,
  campaign,
  providerSessions,
  leadCalls,
  reviewCalls,
) {
  assert(
    campaign?.inference === null || campaign?.inference === undefined,
    "FUNDING_MODE_MISMATCH",
    "subscription-only execution returned a monetary inference envelope; Core V3 subscription admission is required and no envelope may be used as a spending cap",
  );

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

  const callInput = [...leadCalls, ...reviewCalls].reduce(
    (total, call) => total + numeric(call.input_tokens),
    0,
  );
  const callOutput = [...leadCalls, ...reviewCalls].reduce(
    (total, call) => total + numeric(call.output_tokens),
    0,
  );
  const sessionInput = providerSessions.reduce(
    (total, session) => total + numeric(session.input_tokens),
    0,
  );
  const sessionOutput = providerSessions.reduce(
    (total, session) => total + numeric(session.output_tokens),
    0,
  );
  const totalInput = callInput + sessionInput;
  const totalOutput = callOutput + sessionOutput;
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

  const configuredRateMicros = [...leadCalls, ...reviewCalls].reduce(
    (total, call) => total + numeric(call.cost_micros),
    0,
  );
  const opencodeMessageCostMicros = providerSessions.reduce(
    (total, session) => total + numeric(session.cost_micros),
    0,
  );
  const listRateMicros = configuredRateMicros + opencodeMessageCostMicros;

  const actualCharge = summarizeActualCharge([
    ...leadCalls.map((record) => ({
      record,
      kind: "model_call",
      id: record.reservation_id,
    })),
    ...reviewCalls.map((record) => ({
      record,
      kind: "model_call",
      id: record.reservation_id,
    })),
    ...providerSessions.map((record) => ({
      record,
      kind: "provider_session",
      id: record.session_record_id,
    })),
  ]);

  const rateRevision = {
    provider_settings_revision: preflight.provider.revision ?? null,
    provider_settings_updated_at: preflight.provider.updatedAt ?? null,
    configured_input_micros_per_million:
      preflight.provider.inputMicrosPerMillion ?? null,
    configured_output_micros_per_million:
      preflight.provider.outputMicrosPerMillion ?? null,
    schedule: isDeepSeekPeak(startedAt) ? "PEAK" : "OFF_PEAK",
    source: "CORE_PROVIDER_SETTINGS_AND_OPENCODE_GO_DOC_2026-10-29",
    owner_evidence_rate_revision:
      gate.verification.record.rate_revision ?? null,
  };

  const subscription = summarizeVerification(gate.verification);
  const unknown = [];
  if (actualCharge.observation === "UNKNOWN") {
    unknown.push("provider_reported_actual_charge");
  }
  if (subscription.quota.state === "UNKNOWN") {
    unknown.push("subscription_quota_remaining");
  }
  if (subscription.source === "OWNER_CONSOLE") {
    unknown.push("core_console_verification");
  }
  unknown.push("invoice");

  return {
    funding: {
      policy: SUBSCRIPTION_POLICY,
      mode: SUBSCRIPTION_MODE,
      positive_usd_authorization: "NOT_REQUIRED",
      spend_cap: "NOT_USED",
      monetary_envelope: "ABSENT",
      paid_mode: "PAID_MODE_DISABLED",
      fallback: "NONE",
    },
    subscription: {
      ...subscription,
      entitlement_observation:
        subscription.source === "CORE"
          ? "VERIFIED_CORE_EXPORT"
          : "VERIFIED_OWNER_CONSOLE",
      quota_observation: {
        state: subscription.quota.state,
        remaining_usd: subscription.quota.remaining_usd,
        resets_at: subscription.quota.resets_at,
        observed_at: subscription.observed_at,
        source: subscription.source,
      },
    },
    usage: {
      measured: {
        input_tokens: totalInput,
        output_tokens: totalOutput,
        total_tokens:
          [...leadCalls, ...reviewCalls].reduce(
            (total, call) => total + numeric(call.total_tokens),
            0,
          ) +
          providerSessions.reduce(
            (total, session) => total + numeric(session.total_tokens),
            0,
          ),
        max_input_tokens: gate.maxInputTokens,
        max_output_tokens: gate.maxOutputTokens,
      },
      list_rate_equivalent: {
        micros: listRateMicros,
        lead_reviewer_configured_rate_micros: configuredRateMicros,
        implementer_opencode_message_cost_micros: opencodeMessageCostMicros,
        rate_revision: rateRevision,
        provenance:
          "CORE_CONFIGURED_RATE_PLUS_OPENCODE_LOCAL_MESSAGE_COST_ESTIMATE",
        is_actual_charge: false,
        is_spending_cap: false,
      },
      provider_reported_actual_charge: actualCharge,
      unknown,
    },
    charge_incident:
      actualCharge.observation === "VERIFIED_POSITIVE" ||
      actualCharge.fallback_observed === true
        ? actualCharge
        : null,
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
  if (isQuotaPausedPayload(projection.payload)) {
    throw new SubscriptionPausedError(
      "Core reports SUBSCRIPTION_QUOTA_EXHAUSTED before approval",
      { stage: "EXECUTION", verification: gate.verification.record },
    );
  }
  assert(
    projection.payload?.state === "AWAITING_APPROVAL" ||
      projection.payload?.state === "EXECUTING" ||
      projection.payload?.state === "COMPLETED",
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
    state.plan_approval = gate.planApproval;
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
      });
    } catch (error) {
      throwIfQuota(error, "EXECUTION");
      // Approval is idempotent by key, but a transport error is ambiguous. Poll
      // the durable state; never send a blind retry.
      const current = await client.get(`/intake/requests/${state.intake_request_id}`).catch(() => null);
      if (isQuotaPausedPayload(current?.payload)) {
        throw new SubscriptionPausedError(
          "Core paused the approved request with SUBSCRIPTION_QUOTA_EXHAUSTED",
          { stage: "EXECUTION", verification: gate.verification.record },
        );
      }
      if (!["EXECUTING", "COMPLETED"].includes(current?.payload?.state)) throw error;
    }
  }

  state.phase = "EXECUTING";
  writeJson(statePath(gate), state);
  return finishExecution(client, gate, state, pre, startedAt, projection);
}

async function finishExecution(client, gate, state, pre, startedAt, projection) {
  const campaignId = await waitFor(
    "Campaign start",
    async () => {
      const current = await client.get(`/intake/requests/${state.intake_request_id}`);
      if (isQuotaPausedPayload(current.payload)) {
        state.campaign_id = current.payload?.campaignId ?? state.campaign_id ?? null;
        writeJson(statePath(gate), state);
        throw new SubscriptionPausedError(
          "Core paused the request with SUBSCRIPTION_QUOTA_EXHAUSTED",
          { stage: "EXECUTION", verification: gate.verification.record },
        );
      }
      return current.payload?.campaignId ?? null;
    },
    { timeoutMs: gate.timeoutMs, intervalMs: 2_000 },
  );
  state.campaign_id = campaignId;
  writeJson(statePath(gate), state);

  const campaign = await waitFor(
    "Campaign candidate",
    async () => {
      const requestState = await client.get(
        `/intake/requests/${state.intake_request_id}`,
      );
      if (isQuotaPausedPayload(requestState.payload)) {
        throw new SubscriptionPausedError(
          "Core paused the Campaign with SUBSCRIPTION_QUOTA_EXHAUSTED",
          { stage: "EXECUTION", verification: gate.verification.record },
        );
      }
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

  const fundingUsage = buildFundingUsageEvidence(
    gate,
    pre,
    startedAt,
    campaign,
    providerSessions,
    leadCalls,
    reviewCalls,
  );
  if (fundingUsage.charge_incident) {
    const incident = new UatError(
      "SUBSCRIPTION_CHARGE_INCIDENT",
      "a positive provider-reported charge or paid billing source was observed on a subscription-only run",
    );
    incident.fundingUsage = fundingUsage;
    throw incident;
  }

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
      schema: "factory.real-provider-uat.evidence.v2",
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
        funding_policy: gate.fundingPolicy,
        accounting: gate.accounting,
        extra_usage: gate.extraUsage,
        positive_usd_authorization: "NOT_REQUIRED",
        spend_cap: "NOT_USED",
        max_input_tokens: gate.maxInputTokens,
        max_output_tokens: gate.maxOutputTokens,
        max_turns: gate.maxTurns,
        provider: gate.provider,
        model: gate.model,
        plan_hash: state.plan_hash ?? gate.planHash,
        plan_approval: state.plan_approval ?? gate.planApproval,
        approval_rationale: state.approval_rationale,
        subscription_evidence_sha256: gate.verification.sha256,
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
          actual_charge: readActualCharge(
            implementer.session,
            "provider_session",
            state.attempt_id,
          ),
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
          cost_semantics:
            "OPENCODE_LOCAL_MESSAGE_COST_ESTIMATE_NOT_A_BALANCE_CHARGE",
          actual_charge: readActualCharge(
            session,
            "provider_session",
            session.session_record_id,
          ),
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
      ...fundingUsage,
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
        funding_policy: SUBSCRIPTION_POLICY,
        paid_fallback: "NONE",
        redispatch: "NONE",
        github_pr_created_or_merged: false,
        github_write: "NONE",
        isolation: "NOT_CLAIMED",
        independent_acceptance: "NOT_CLAIMED",
      },
      commands: {
        assess: "scripts/real-provider-uat.sh assess",
        execute: `scripts/real-provider-uat.sh execute --plan-hash ${state.plan_hash ?? gate.planHash}`,
        resume: "scripts/real-provider-uat.sh resume",
      },
      non_claims: [
        "Author-run UAT, not independent acceptance.",
        "No ISOLATED, SEC-01 or credential-confinement certification.",
        "Configured-rate and OpenCode message-cost micros are list-rate equivalents, not actual charges or spending caps.",
        "Provider-reported actual charge is unknown unless Core supplies explicit charge evidence.",
        "OpenCode Go allowance consumption is UNKNOWN to Core; the Owner Console observation is recorded but not Core-verified.",
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

function pausePath(gate) {
  return join(gate.evidenceDir, "real-provider-uat-paused.json");
}

function pauseEvidence(client, gate, error, { preDispatch = false } = {}) {
  ensureEvidenceDir(gate);
  let state = null;
  if (!preDispatch) {
    state = loadState(gate) ?? {
      schema: STATE_SCHEMA,
      authorization: gate.authorization,
    };
    state.phase = "PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED";
    state.pause_reason = "SUBSCRIPTION_QUOTA_EXHAUSTED";
    state.paused_at = nowIso();
    state.resume_stage = error.stage ?? "EXECUTION";
    state.recovery =
      "AUTHENTICATED_CORE_RESUME_AFTER_REFRESHED_QUOTA_EVIDENCE";
    writeJson(statePath(gate), state);
  }
  const verification = gate.verification
    ? summarizeVerification(gate.verification)
    : error?.verification
      ? {
          source: error.verification.source,
          plan: error.verification.plan,
          account_fingerprint: error.verification.account_fingerprint,
          entitlement: error.verification.entitlement,
          extra_usage: error.verification.extra_usage,
          balance_fallback: error.verification.balance_fallback,
          no_charge_route: error.verification.no_charge_route,
          quota: error.verification.quota,
          core_verifiable: error.verification.core_verifiable,
          limitation: error.verification.limitation ?? null,
        }
      : null;
  const evidence = redactDeep(
    {
      schema: "factory.real-provider-uat.paused.v1",
      result: "PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED",
      phase: "PAUSED",
      paused_at: nowIso(),
      recoverable: true,
      fallback: "NONE",
      redispatch: "NONE",
      pre_dispatch: preDispatch,
      gate: {
        ...gateEcho(gate),
        plan_hash: (state?.plan_hash ?? gate.planHash) || null,
      },
      state,
      subscription: verification,
      quota: verification?.quota ?? null,
      error: {
        code: error?.code ?? "SUBSCRIPTION_QUOTA_EXHAUSTED",
        message: error?.message ?? String(error),
      },
      resume_command: preDispatch
        ? "Re-run assess after refreshing the Owner Console verification"
        : "scripts/real-provider-uat.sh resume",
      boundary: {
        real_core_http_api: true,
        funding_policy: SUBSCRIPTION_POLICY,
        paid_fallback: "NONE",
        provider_or_model_fallback: "NONE",
        redispatch: "NONE",
        github_write: "NONE",
        isolation: "NOT_CLAIMED",
        independent_acceptance: "NOT_CLAIMED",
      },
    },
    client?.secrets ?? new Set(),
  );
  writeJson(pausePath(gate), evidence);
  return evidence;
}

/// Execute the approved plan. Any failure after this point preserves a
/// minimal, redacted failure record so an accounting or gate failure is never
/// hidden by an exception.
export async function execute(client, gate) {
  const state = loadState(gate);
  try {
    return await executeInner(client, gate);
  } catch (caught) {
    const error = classifySubscriptionAdmissionError(caught);
    if (error?.paused === true) {
      const evidence = pauseEvidence(client, gate, error);
      return {
        result: "PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED",
        paused: true,
        evidence,
        pausePath: pausePath(gate),
        gateDecision: null,
      };
    }
    const latestState = loadState(gate) ?? state;
    const failure = redactDeep(
      {
        schema: "factory.real-provider-uat.evidence.v2",
        result: `FAILED_${error?.code ?? "UNEXPECTED"}`,
        phase: "FAILED",
        finished_at: nowIso(),
        gate: {
          ...gateEcho(gate),
          plan_hash: gate.planHash,
          plan_approval: gate.planApproval,
        },
        state: latestState,
        error: {
          code: error?.code ?? "UNEXPECTED",
          message: error?.message ?? String(error),
        },
        ...(error?.fundingUsage ?? {}),
        boundary: {
          real_core_http_api: true,
          live_provider_inference: "CALLS_MAY_HAVE_BEEN_MADE",
          funding_policy: SUBSCRIPTION_POLICY,
          paid_fallback: "NONE",
          provider_or_model_fallback: "NONE",
          redispatch: "NONE",
          github_write: "NONE",
          isolation: "NOT_CLAIMED",
          independent_acceptance: "NOT_CLAIMED",
        },
        unknown: [
          "invoice",
          "provider_reported_actual_charge_when_absent",
        ],
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

/// Resume a quota-paused subscription run through an authenticated Core
/// operation. This never re-approves the plan, changes provider/model/funding,
/// or dispatches a replacement operation.
export async function resume(client, gate) {
  const state = loadState(gate);
  assert(state, "STATE_MISSING", "no paused state exists; run assess first");
  ensureStateAuthorization(state, gate);
  assert(
    state.phase === "PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED",
    "STATE_NOT_PAUSED",
    `the existing state is ${state.phase}; resume requires PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED`,
  );
  assert(
    state.resume_stage === "EXECUTION",
    "RESUME_NOT_SUPPORTED_FOR_STAGE",
    "quota exhaustion during assessment requires a new authorization; only execution resumes automatically",
  );
  assert(
    state.plan_hash && state.campaign_id,
    "RESUME_CONTEXT_MISSING",
    "the paused state has no frozen plan hash or Campaign identity",
  );
  if (gate.verification.record.quota.state === "EXHAUSTED") {
    const evidence = pauseEvidence(
      client,
      gate,
      new SubscriptionPausedError(
        "subscription quota is still exhausted according to refreshed Owner evidence",
        { stage: "EXECUTION", verification: gate.verification.record },
      ),
    );
    return {
      result: "PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED",
      paused: true,
      evidence,
      pausePath: pausePath(gate),
      gateDecision: null,
    };
  }

  let pre;
  try {
    pre = await preflight(client, gate);
  } catch (error) {
    if (error?.paused === true) {
      const evidence = pauseEvidence(client, gate, error);
      return {
        result: "PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED",
        paused: true,
        evidence,
        pausePath: pausePath(gate),
        gateDecision: null,
      };
    }
    throw error;
  }
  let projection = await client.get(`/intake/requests/${state.intake_request_id}`);
  assert(
    projection.payload?.plan?.plan_hash === state.plan_hash,
    "PLAN_HASH_MISMATCH",
    "Core's frozen plan hash does not match the paused run",
  );

  if (!["EXECUTING", "COMPLETED"].includes(projection.payload?.state)) {
    try {
      await client.post(`/intake/requests/${state.intake_request_id}/resume`, {
        expected_plan_hash: state.plan_hash,
        expected_campaign_id: state.campaign_id,
        subscription_evidence_sha256: gate.verification.sha256,
        idempotency_key: `${gate.authorization}:resume:${state.resume_count ?? 0}`,
      });
    } catch (error) {
      if (error?.code === "CORE_HTTP" && /-> 404|-> 405/.test(error.message)) {
        const blocked = new UatError(
          "CORE_V3_SUBSCRIPTION_RESUME_REQUIRED",
          "Core does not expose the subscription quota resume operation; no fallback or redispatch was attempted",
          1,
        );
        blocked.state = state;
        throw blocked;
      }
      throwIfQuota(error, "EXECUTION");
      const current = await client
        .get(`/intake/requests/${state.intake_request_id}`)
        .catch(() => null);
      if (isQuotaPausedPayload(current?.payload)) {
        const evidence = pauseEvidence(
          client,
          gate,
          new SubscriptionPausedError(
            "Core paused the resume with SUBSCRIPTION_QUOTA_EXHAUSTED",
            { stage: "EXECUTION", verification: gate.verification.record },
          ),
        );
        return {
          result: "PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED",
          paused: true,
          evidence,
          pausePath: pausePath(gate),
          gateDecision: null,
        };
      }
      if (!["EXECUTING", "COMPLETED"].includes(current?.payload?.state)) {
        throw error;
      }
    }
  }

  state.phase = "EXECUTING";
  state.resume_count = (state.resume_count ?? 0) + 1;
  state.resumed_at = nowIso();
  writeJson(statePath(gate), state);
  return finishExecution(client, gate, state, pre, new Date(), projection);
}

export async function main(argv = process.argv.slice(2), env = process.env) {
  const gate = parseGate(env, argv);
  try {
    gate.verification = loadSubscriptionVerification(gate);
  } catch (error) {
    if (error?.paused === true) {
      const evidence = pauseEvidence(null, gate, error, { preDispatch: true });
      printSummary("PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED", [
        "No dispatch was attempted.",
        `Paused evidence: ${pausePath(gate)}`,
        "Refresh the Owner Console quota/entitlement evidence, then re-run assess.",
      ]);
      process.exitCode = EXIT_PAUSED;
      return { result: "PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED", paused: true, evidence };
    }
    throw error;
  }
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
      let assessed;
      try {
        assessed = await assess(client, gate);
      } catch (error) {
        if (error?.paused === true) {
          const evidence = pauseEvidence(client, gate, error);
          printSummary("PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED", [
            "Assessment hit a quota pause; no retry or fallback was attempted.",
            `Paused evidence: ${pausePath(gate)}`,
            "A new authorization is required to re-assess after refresh.",
          ]);
          process.exitCode = EXIT_PAUSED;
          return {
            result: "PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED",
            paused: true,
            evidence,
          };
        }
        throw classifySubscriptionAdmissionError(error);
      }
      printSummary("AWAITING_PLAN_APPROVAL", [
        `Project: ${assessed.state.project_id}`,
        `Request: ${assessed.state.intake_request_id}`,
        `Plan hash: ${assessed.plan.plan_hash}`,
        `Plan file: ${planPath(gate)}`,
        `Next: review plan.json, then run execute --plan-hash ${assessed.plan.plan_hash}`,
      ]);
      return assessed;
    }
    const executed =
      gate.command === "resume"
        ? await resume(client, gate)
        : await execute(client, gate);
    if (executed.paused === true) {
      printSummary("PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED", [
        "Recoverable pause; no provider/model/funding fallback and no redispatch.",
        `Paused evidence: ${executed.pausePath}`,
        "Refresh the Owner Console quota/entitlement evidence, then run resume.",
      ]);
      process.exitCode = EXIT_PAUSED;
      return executed;
    }
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
