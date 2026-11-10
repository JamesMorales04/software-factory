#!/usr/bin/env node
// Bounded OpenSpec validation job for a Core-rendered change.
//
// This script runs only inside the pinned OpenSpec image. It fetches the latest
// durable projection from Core over the private control network, writes the
// rendered change into an isolated temporary workspace, verifies every hash
// locally, and runs the pinned `openspec validate <change-id> --strict`.
//
// Core remains authoritative. A PASS is validation evidence for plan review; it
// never accepts a candidate, closes a finding or grants delivery authority.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";

const RESULT_SCHEMA_VERSION = 1;
const PINNED_OPENSPEC_VERSION = "1.16.1";
const MAX_RESPONSE_BYTES = 8 * 1024 * 1024;
const MAX_FILE_BYTES = 512 * 1024;
const MAX_TOTAL_BYTES = 2 * 1024 * 1024;
const MAX_FILES = 64;
const MAX_OUTPUT_BYTES = 256 * 1024;
const MAX_CHANGE_ID_CHARS = 64;
const MAX_REQUEST_ID_CHARS = 200;
const FETCH_TIMEOUT_MS = 15_000;
const COMMAND_TIMEOUT_MS = 120_000;

class ValidationError extends Error {
  constructor(code, message) {
    super(message);
    this.code = code;
  }
}

const result = {
  schemaVersion: RESULT_SCHEMA_VERSION,
  status: "FAIL",
  requestId: process.env.FACTORY_OPENSPEC_REQUEST_ID ?? null,
  changeId: null,
  changePath: null,
  revision: null,
  validated: null,
  archived: null,
  openspecVersion: null,
  versionOutput: null,
  artifactContentSha256: null,
  artifactHashes: [],
  command: null,
  exitCode: null,
  checks: {
    coreResponse: false,
    versionPinned: false,
    artifactHashesMatch: false,
    openspecValidate: false,
    workspaceCleaned: false,
  },
  validation: {
    stdout: "",
    stderr: "",
  },
  error: null,
  code: null,
};

let workspace = null;

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function requireSha256(name, value) {
  if (
    typeof value !== "string" ||
    value.length !== 64 ||
    !/^[0-9a-f]{64}$/.test(value)
  ) {
    throw new ValidationError(
      "ARTIFACT_HASH_INVALID",
      `${name} must be a lowercase SHA-256`,
    );
  }
  return value;
}

function validateChangeId(value) {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length > MAX_CHANGE_ID_CHARS ||
    !/^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/.test(value) ||
    value.includes("--") ||
    value === "archive"
  ) {
    throw new ValidationError(
      "CHANGE_ID_INVALID",
      "change id must be a safe lowercase slug",
    );
  }
  return value;
}

function validateRelativePath(value) {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length > 512 ||
    value.startsWith("/") ||
    value.includes("\\") ||
    value.includes("\0") ||
    value.split("/").some((segment) => !segment || segment === "." || segment === "..")
  ) {
    throw new ValidationError(
      "ARTIFACT_PATH_INVALID",
      "artifact path must be a safe relative path",
    );
  }
  return value;
}

async function readBoundedBody(response) {
  const declared = Number(response.headers.get("content-length"));
  if (Number.isFinite(declared) && declared > MAX_RESPONSE_BYTES) {
    throw new ValidationError(
      "CORE_RESPONSE_TOO_LARGE",
      "Core response exceeds the byte bound",
    );
  }
  if (!response.body) {
    return "";
  }
  const reader = response.body.getReader();
  const chunks = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) {
      break;
    }
    total += value.byteLength;
    if (total > MAX_RESPONSE_BYTES) {
      await reader.cancel().catch(() => {});
      throw new ValidationError(
        "CORE_RESPONSE_TOO_LARGE",
        "Core response exceeds the byte bound",
      );
    }
    chunks.push(Buffer.from(value));
  }
  return Buffer.concat(chunks).toString("utf8");
}

function readOperatorToken() {
  const tokenFile =
    process.env.FACTORY_OPERATOR_TOKEN_FILE ?? "/config/operator.token";
  let token;
  try {
    token = readFileSync(tokenFile, "utf8").trim();
  } catch {
    throw new ValidationError(
      "TOKEN_UNREADABLE",
      "the operator token file is not readable",
    );
  }
  if (
    token.length === 0 ||
    token.length > 4096 ||
    /[\s\x00-\x1f]/.test(token)
  ) {
    throw new ValidationError(
      "TOKEN_INVALID",
      "the operator token file is empty or malformed",
    );
  }
  return token;
}

async function fetchProjection() {
  const requestId = process.env.FACTORY_OPENSPEC_REQUEST_ID ?? "";
  if (
    requestId.length === 0 ||
    requestId.length > MAX_REQUEST_ID_CHARS ||
    !/^[A-Za-z0-9._:-]+$/.test(requestId)
  ) {
    throw new ValidationError(
      "REQUEST_ID_INVALID",
      "FACTORY_OPENSPEC_REQUEST_ID must be a bounded request identifier",
    );
  }
  result.requestId = requestId;
  const coreUrl = (
    process.env.FACTORY_CORE_URL ?? "http://core:8765"
  ).replace(/\/+$/, "");
  if (!/^https?:\/\/[^/]+$/.test(coreUrl)) {
    throw new ValidationError(
      "CORE_URL_INVALID",
      "FACTORY_CORE_URL must be an http(s) origin",
    );
  }
  const token = readOperatorToken();
  const timeoutMs = Number(
    process.env.FACTORY_OPENSPEC_FETCH_TIMEOUT_MS ?? FETCH_TIMEOUT_MS,
  );
  const controller = new AbortController();
  const timer = setTimeout(
    () => controller.abort(),
    Number.isFinite(timeoutMs) ? timeoutMs : FETCH_TIMEOUT_MS,
  );
  let response;
  try {
    response = await fetch(
      `${coreUrl}/agentic/requests/${encodeURIComponent(requestId)}/openspec`,
      {
        method: "GET",
        headers: { authorization: `Bearer ${token}` },
        redirect: "error",
        signal: controller.signal,
      },
    );
  } catch (error) {
    throw new ValidationError(
      "CORE_UNREACHABLE",
      `Core request failed: ${error.name}`,
    );
  } finally {
    clearTimeout(timer);
  }
  const body = await readBoundedBody(response);
  if (!response.ok) {
    let code = "";
    try {
      const parsed = JSON.parse(body);
      if (typeof parsed.code === "string" && /^[A-Z_]{1,64}$/.test(parsed.code)) {
        code = ` (${parsed.code})`;
      }
    } catch {
      // The status is enough; never echo an unparsed body.
    }
    throw new ValidationError(
      "CORE_HTTP_ERROR",
      `Core returned HTTP ${response.status}${code}`,
    );
  }
  let payload;
  try {
    payload = JSON.parse(body);
  } catch {
    throw new ValidationError(
      "CORE_RESPONSE_INVALID",
      "Core response is not valid JSON",
    );
  }
  return { requestId, payload };
}

function validateProjection(payload, requestId) {
  if (
    payload === null ||
    typeof payload !== "object" ||
    Array.isArray(payload)
  ) {
    throw new ValidationError(
      "CORE_RESPONSE_INVALID",
      "Core response must be an object",
    );
  }
  if (payload.requestId !== requestId) {
    throw new ValidationError(
      "CORE_RESPONSE_MISMATCH",
      "Core response does not bind the requested request",
    );
  }
  const changeId = validateChangeId(payload.changeId);
  if (
    !Number.isInteger(payload.revision) ||
    payload.revision < 1 ||
    payload.revision > 2_147_483_647
  ) {
    throw new ValidationError(
      "PROJECTION_INVALID",
      "projection revision must be a positive integer",
    );
  }
  if (payload.validated !== true) {
    throw new ValidationError(
      "OPENSPEC_NOT_VALIDATED",
      "Core has not marked the projection as validated",
    );
  }
  if (
    payload.changePath !== `openspec/changes/${changeId}` ||
    typeof payload.archived !== "boolean"
  ) {
    throw new ValidationError(
      "PROJECTION_INVALID",
      "projection path or archived flag is invalid",
    );
  }
  const artifactContentSha256 = requireSha256(
    "artifactContentSha256",
    payload.artifactContentSha256,
  );
  const hashes = payload.hashes;
  if (hashes === null || typeof hashes !== "object") {
    throw new ValidationError(
      "PROJECTION_INVALID",
      "projection hashes are missing",
    );
  }
  requireSha256("proposalSha256", hashes.proposalSha256);
  requireSha256("specsSha256", hashes.specsSha256);
  requireSha256("tasksSha256", hashes.tasksSha256);
  if (
    typeof hashes.designSha256 !== "string" ||
    (hashes.designSha256 !== "" && !/^[0-9a-f]{64}$/.test(hashes.designSha256))
  ) {
    throw new ValidationError(
      "ARTIFACT_HASH_INVALID",
      "designSha256 must be empty or a lowercase SHA-256",
    );
  }

  const manifest = payload.manifest;
  if (
    manifest === null ||
    typeof manifest !== "object" ||
    manifest.changeId !== changeId ||
    manifest.revision !== payload.revision ||
    !Array.isArray(manifest.artifacts) ||
    manifest.artifacts.length === 0 ||
    manifest.artifacts.length > MAX_FILES
  ) {
    throw new ValidationError(
      "MANIFEST_INVALID",
      "manifest identity or artifact list is invalid",
    );
  }
  const manifestByPath = new Map();
  for (const entry of manifest.artifacts) {
    const artifactPath = validateRelativePath(entry?.path);
    requireSha256(`manifest hash for ${artifactPath}`, entry?.sha256);
    if (manifestByPath.has(artifactPath)) {
      throw new ValidationError(
        "MANIFEST_INVALID",
        "manifest contains a duplicate artifact path",
      );
    }
    manifestByPath.set(artifactPath, entry.sha256);
  }

  if (
    !Array.isArray(payload.files) ||
    payload.files.length === 0 ||
    payload.files.length > MAX_FILES ||
    payload.files.length !== manifest.artifacts.length
  ) {
    throw new ValidationError(
      "ARTIFACTS_INVALID",
      "rendered files do not match the manifest",
    );
  }
  const files = [];
  const filePaths = [];
  let totalBytes = 0;
  for (const file of payload.files) {
    const artifactPath = validateRelativePath(file?.path);
    if (
      typeof file.content !== "string" ||
      file.content.length === 0 ||
      Buffer.byteLength(file.content, "utf8") > MAX_FILE_BYTES
    ) {
      throw new ValidationError(
        "ARTIFACTS_INVALID",
        `artifact ${artifactPath} is empty or exceeds the byte bound`,
      );
    }
    totalBytes += Buffer.byteLength(file.content, "utf8");
    if (totalBytes > MAX_TOTAL_BYTES) {
      throw new ValidationError(
        "ARTIFACTS_INVALID",
        "rendered artifacts exceed the total byte bound",
      );
    }
    const computed = sha256(file.content, "utf8");
    const manifestHash = manifestByPath.get(artifactPath);
    if (computed !== file.sha256 || computed !== manifestHash) {
      throw new ValidationError(
        "ARTIFACT_HASH_MISMATCH",
        `artifact ${artifactPath} does not match its manifest hash`,
      );
    }
    filePaths.push(artifactPath);
    files.push({ path: artifactPath, sha256: computed, content: file.content });
  }
  const manifestPaths = [...manifestByPath.keys()];
  if (
    filePaths.length !== manifestPaths.length ||
    filePaths.some((artifactPath, index) => artifactPath !== manifestPaths[index])
  ) {
    throw new ValidationError(
      "ARTIFACTS_INVALID",
      "rendered files are not in deterministic manifest order",
    );
  }

  const specs = manifest.artifacts.filter((entry) =>
    entry.path.startsWith("specs/"),
  );
  if (specs.length === 0) {
    throw new ValidationError(
      "MANIFEST_INVALID",
      "manifest contains no specification artifact",
    );
  }
  const specsHasher = createHash("sha256");
  for (const spec of specs) {
    specsHasher.update(spec.path);
    specsHasher.update(Buffer.from([0]));
    specsHasher.update(spec.sha256);
    specsHasher.update(Buffer.from([0]));
  }
  const specsSha256 = specsHasher.digest("hex");
  if (
    hashes.proposalSha256 !== manifestByPath.get("proposal.md") ||
    hashes.tasksSha256 !== manifestByPath.get("tasks.md") ||
    hashes.designSha256 !== (manifestByPath.get("design.md") ?? "") ||
    hashes.specsSha256 !== specsSha256
  ) {
    throw new ValidationError(
      "ARTIFACT_HASH_MISMATCH",
      "projection hashes do not match the manifest",
    );
  }

  result.changeId = changeId;
  result.changePath = payload.changePath;
  result.revision = payload.revision;
  result.validated = payload.validated;
  result.archived = payload.archived;
  result.artifactContentSha256 = artifactContentSha256;
  result.artifactHashes = files.map(({ path: artifactPath, sha256: digest }) => ({
    path: artifactPath,
    sha256: digest,
  }));
  result.checks.coreResponse = true;
  result.checks.artifactHashesMatch = true;
  return { changeId, files };
}

function writeWorkspace(changeId, files) {
  const base = path.resolve(process.env.FACTORY_OPENSPEC_WORKSPACE ?? "/workspace");
  mkdirSync(base, { recursive: true });
  workspace = mkdtempSync(path.join(base, "openspec-"));
  const changeDir = path.join(workspace, "openspec", "changes", changeId);
  for (const file of files) {
    const target = path.resolve(changeDir, file.path);
    if (!target.startsWith(`${changeDir}${path.sep}`)) {
      throw new ValidationError(
        "ARTIFACT_PATH_INVALID",
        "artifact path escapes the change directory",
      );
    }
    mkdirSync(path.dirname(target), { recursive: true });
    writeFileSync(target, file.content, { encoding: "utf8", mode: 0o600 });
    const written = sha256(readFileSync(target));
    if (written !== file.sha256) {
      throw new ValidationError(
        "ARTIFACT_WRITE_MISMATCH",
        `artifact ${file.path} was not written exactly`,
      );
    }
  }
  return changeDir;
}

function runOpenSpec(args, cwd) {
  const completed = spawnSync("openspec", args, {
    cwd,
    encoding: "buffer",
    timeout: COMMAND_TIMEOUT_MS,
    maxBuffer: MAX_OUTPUT_BYTES,
    env: {
      ...process.env,
      OPENSPEC_NO_UPDATE_CHECK: "1",
      OPENSPEC_TELEMETRY: "0",
      NO_COLOR: "1",
      CI: "1",
    },
  });
  if (completed.error) {
    if (completed.error.code === "ETIMEDOUT") {
      throw new ValidationError(
        "OPENSPEC_TIMEOUT",
        `openspec ${args.join(" ")} exceeded its timeout`,
      );
    }
    if (completed.error.code === "ENOBUFS") {
      throw new ValidationError(
        "OPENSPEC_OUTPUT_TOO_LARGE",
        `openspec ${args.join(" ")} exceeded its output bound`,
      );
    }
    throw new ValidationError(
      "OPENSPEC_UNAVAILABLE",
      "the pinned openspec executable is not available",
    );
  }
  const stdout = (completed.stdout ?? Buffer.alloc(0)).toString("utf8").trim();
  const stderr = (completed.stderr ?? Buffer.alloc(0)).toString("utf8").trim();
  if (Buffer.byteLength(stdout) + Buffer.byteLength(stderr) > MAX_OUTPUT_BYTES) {
    throw new ValidationError(
      "OPENSPEC_OUTPUT_TOO_LARGE",
      "openspec output exceeded its byte bound",
    );
  }
  return { status: completed.status, stdout, stderr };
}

function verifyVersion(workspacePath) {
  const version = runOpenSpec(["--version"], workspacePath);
  result.versionOutput = version.stdout || version.stderr;
  if (version.status !== 0) {
    throw new ValidationError(
      "OPENSPEC_VERSION_UNAVAILABLE",
      "openspec did not report its version",
    );
  }
  const reported = version.stdout
    .split(/\s+/)
    .filter(Boolean)
    .at(-1)
    ?.replace(/^v/, "");
  result.openspecVersion = reported ?? null;
  if (reported !== PINNED_OPENSPEC_VERSION) {
    throw new ValidationError(
      "OPENSPEC_VERSION_MISMATCH",
      `expected OpenSpec ${PINNED_OPENSPEC_VERSION}, found ${reported ?? "unknown"}`,
    );
  }
  result.checks.versionPinned = true;
}

function validateChange(changeId, workspacePath) {
  result.command = `openspec validate ${changeId} --strict`;
  const validation = runOpenSpec(["validate", changeId, "--strict"], workspacePath);
  result.exitCode = validation.status;
  result.validation = {
    stdout: validation.stdout,
    stderr: validation.stderr,
  };
  if (validation.status !== 0) {
    throw new ValidationError(
      "OPENSPEC_VALIDATE_FAILED",
      `openspec validate exited ${validation.status}`,
    );
  }
  result.checks.openspecValidate = true;
}

function cleanupWorkspace() {
  if (!workspace) {
    return;
  }
  try {
    rmSync(workspace, { recursive: true, force: true });
    result.checks.workspaceCleaned = !existsSync(workspace);
    if (!result.checks.workspaceCleaned) {
      throw new Error("workspace still exists");
    }
  } catch {
    result.checks.workspaceCleaned = false;
    if (result.status === "PASS") {
      result.status = "FAIL";
      result.code = "WORKSPACE_CLEANUP_FAILED";
      result.error = "the isolated workspace could not be removed";
    }
  } finally {
    workspace = null;
  }
}

async function main() {
  const { requestId, payload } = await fetchProjection();
  const { changeId, files } = validateProjection(payload, requestId);
  const workspacePath = writeWorkspace(changeId, files);
  verifyVersion(workspacePath);
  validateChange(changeId, workspacePath);
  result.status = "PASS";
}

try {
  await main();
} catch (error) {
  result.status = "FAIL";
  result.code =
    error instanceof ValidationError ? error.code : "VALIDATION_JOB_FAILED";
  result.error =
    error instanceof ValidationError
      ? error.message
      : "the validation job failed unexpectedly";
} finally {
  cleanupWorkspace();
}

console.log(JSON.stringify(result));
process.exitCode = result.status === "PASS" ? 0 : 1;
