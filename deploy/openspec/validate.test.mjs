// Deterministic contract tests for the OpenSpec validation job.
//
// The real Core endpoint, the real pinned image and a real Compose run are
// separate evidence lanes. These tests use a fake Core HTTP response and a fake
// `openspec` executable so the fetch, bounds, hash verification, workspace and
// exit-code contract can run without Node or OpenSpec on the host product path.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import {
  chmodSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const job = path.join(here, "validate.mjs");
const fixture = JSON.parse(
  readFileSync(path.join(here, "fixtures", "openspec-read-model.json"), "utf8"),
);
const token = "test-operator-token-0123456789abcdef";

function tempRoot() {
  return mkdtempSync(path.join(tmpdir(), "factory-openspec-job-"));
}

function writeToken(root) {
  const tokenFile = path.join(root, "operator.token");
  writeFileSync(tokenFile, `${token}\n`, { encoding: "utf8", mode: 0o600 });
  return tokenFile;
}

function fakeOpenSpec(root, { version = "1.16.1", failValidate = false } = {}) {
  const bin = path.join(root, "bin");
  mkdirSync(bin, { recursive: true });
  const executable = path.join(bin, "openspec");
  writeFileSync(
    executable,
    `#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "openspec ${version}"
  exit 0
fi
if [ "$1" = "validate" ]; then
  if [ "${failValidate ? "1" : "0"}" = "1" ]; then
    echo "missing scenario" >&2
    exit 1
  fi
  echo "$2: valid"
  exit 0
fi
echo "unexpected openspec invocation" >&2
exit 9
`,
    { encoding: "utf8", mode: 0o755 },
  );
  chmodSync(executable, 0o755);
  return bin;
}

async function startFakeCore({ payload = fixture, expectedToken = token } = {}) {
  const server = createServer((request, response) => {
    if (request.headers.authorization !== `Bearer ${expectedToken}`) {
      response.writeHead(401, { "content-type": "application/json" });
      response.end(JSON.stringify({ error: "unauthorized", code: "UNAUTHORIZED" }));
      return;
    }
    if (
      request.method === "GET" &&
      request.url === `/agentic/requests/${payload.requestId}/openspec`
    ) {
      const body = JSON.stringify(payload);
      response.writeHead(200, {
        "content-type": "application/json",
        "content-length": Buffer.byteLength(body),
      });
      response.end(body);
      return;
    }
    response.writeHead(404, { "content-type": "application/json" });
    response.end(JSON.stringify({ error: "not found", code: "NOT_FOUND" }));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address();
  return {
    url: `http://127.0.0.1:${port}`,
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}

function runJob({ coreUrl, root, bin, workspace, extraEnv = {} }) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [job], {
      env: {
        ...process.env,
        PATH: `${bin}${path.delimiter}${process.env.PATH ?? ""}`,
        FACTORY_CORE_URL: coreUrl,
        FACTORY_OPERATOR_TOKEN_FILE: path.join(root, "operator.token"),
        FACTORY_OPENSPEC_REQUEST_ID: fixture.requestId,
        FACTORY_OPENSPEC_WORKSPACE: workspace,
        ...extraEnv,
      },
    });
    let stdout = "";
    let stderr = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), 30_000);
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
    child.on("close", (status) => {
      clearTimeout(timer);
      try {
        const lines = stdout.trim().split("\n").filter(Boolean);
        assert.equal(
          lines.length,
          1,
          `expected one JSON result line, got: ${stdout}`,
        );
        resolve({
          completed: { status, stdout, stderr },
          result: JSON.parse(lines[0]),
        });
      } catch (error) {
        reject(error);
      }
    });
  });
}

test("validates a Core-rendered change and cleans its workspace", async () => {
  const root = tempRoot();
  const workspace = path.join(root, "workspace");
  const core = await startFakeCore();
  try {
    const bin = fakeOpenSpec(root);
    writeToken(root);
    const { completed, result } = await runJob({
      coreUrl: core.url,
      root,
      bin,
      workspace,
    });
    assert.equal(completed.status, 0, completed.stderr);
    assert.equal(result.status, "PASS");
    assert.equal(result.openspecVersion, "1.16.1");
    assert.equal(result.changeId, fixture.changeId);
    assert.equal(result.revision, fixture.revision);
    assert.equal(result.validated, true);
    assert.equal(result.command, `openspec validate ${fixture.changeId} --strict`);
    assert.equal(result.exitCode, 0);
    assert.deepEqual(result.checks, {
      coreResponse: true,
      versionPinned: true,
      artifactHashesMatch: true,
      openspecValidate: true,
      workspaceCleaned: true,
    });
    assert.deepEqual(
      result.artifactHashes,
      fixture.files.map((file) => ({ path: file.path, sha256: file.sha256 })),
    );
    assert.deepEqual(readdirSync(workspace), []);
    assert.equal(result.artifactContentSha256, fixture.artifactContentSha256);
  } finally {
    await core.close();
    rmSync(root, { recursive: true, force: true });
  }
});

test("returns a non-zero machine result when openspec validate fails", async () => {
  const root = tempRoot();
  const workspace = path.join(root, "workspace");
  const core = await startFakeCore();
  try {
    const bin = fakeOpenSpec(root, { failValidate: true });
    writeToken(root);
    const { completed, result } = await runJob({
      coreUrl: core.url,
      root,
      bin,
      workspace,
    });
    assert.equal(completed.status, 1);
    assert.equal(result.status, "FAIL");
    assert.equal(result.code, "OPENSPEC_VALIDATE_FAILED");
    assert.equal(result.checks.openspecValidate, false);
    assert.equal(result.checks.workspaceCleaned, true);
    assert.match(result.validation.stderr, /missing scenario/);
    assert.deepEqual(readdirSync(workspace), []);
  } finally {
    await core.close();
    rmSync(root, { recursive: true, force: true });
  }
});

test("rejects a response whose file does not match the Core manifest hash", async () => {
  const root = tempRoot();
  const workspace = path.join(root, "workspace");
  const tampered = structuredClone(fixture);
  tampered.files[0].content += "\n<!-- tampered -->\n";
  const core = await startFakeCore({ payload: tampered });
  try {
    const bin = fakeOpenSpec(root);
    writeToken(root);
    const { completed, result } = await runJob({
      coreUrl: core.url,
      root,
      bin,
      workspace,
    });
    assert.equal(completed.status, 1);
    assert.equal(result.status, "FAIL");
    assert.equal(result.code, "ARTIFACT_HASH_MISMATCH");
    assert.equal(result.checks.coreResponse, false);
    assert.equal(result.checks.openspecValidate, false);
    assert.equal(result.command, null);
  } finally {
    await core.close();
    rmSync(root, { recursive: true, force: true });
  }
});

test("surfaces a typed Core authorization failure without leaking the token", async () => {
  const root = tempRoot();
  const workspace = path.join(root, "workspace");
  const core = await startFakeCore({ expectedToken: "different-token" });
  try {
    const bin = fakeOpenSpec(root);
    writeToken(root);
    const { completed, result } = await runJob({
      coreUrl: core.url,
      root,
      bin,
      workspace,
    });
    assert.equal(completed.status, 1);
    assert.equal(result.code, "CORE_HTTP_ERROR");
    assert.match(result.error, /HTTP 401/);
    assert.equal(completed.stdout.includes(token), false);
    assert.equal(completed.stderr.includes(token), false);
  } finally {
    await core.close();
    rmSync(root, { recursive: true, force: true });
  }
});

test("rejects an unpinned OpenSpec version before validating", async () => {
  const root = tempRoot();
  const workspace = path.join(root, "workspace");
  const core = await startFakeCore();
  try {
    const bin = fakeOpenSpec(root, { version: "1.15.0" });
    writeToken(root);
    const { completed, result } = await runJob({
      coreUrl: core.url,
      root,
      bin,
      workspace,
    });
    assert.equal(completed.status, 1);
    assert.equal(result.code, "OPENSPEC_VERSION_MISMATCH");
    assert.equal(result.checks.versionPinned, false);
    assert.equal(result.command, null);
    assert.equal(result.checks.workspaceCleaned, true);
  } finally {
    await core.close();
    rmSync(root, { recursive: true, force: true });
  }
});
