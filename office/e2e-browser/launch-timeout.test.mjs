// Fault injection for the browser process ownership path. A deliberately
// hung executable and its child must both be killed when launch times out.

import assert from "node:assert/strict";
import { execSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, before, describe, it } from "node:test";
import { launchBrowser } from "./harness.mjs";

const MARKER = `harness-launch-probe-${process.pid}`;
const SCRIPT = join(tmpdir(), `${MARKER}.sh`);
const PID_FILE = join(tmpdir(), `${MARKER}.pid`);
const PATTERN = "harness-launch-probe-[0-9]+";
const previousPidFile = process.env.FACTORY_TEST_PID_FILE;

before(() => {
  writeFileSync(
    SCRIPT,
    '#!/bin/sh\nprintf "%s" "$$" > "$FACTORY_TEST_PID_FILE"\nsleep 30 & wait\n',
  );
  chmodSync(SCRIPT, 0o755);
  process.env.FACTORY_TEST_PID_FILE = PID_FILE;
});

after(() => {
  if (previousPidFile === undefined) delete process.env.FACTORY_TEST_PID_FILE;
  else process.env.FACTORY_TEST_PID_FILE = previousPidFile;
  if (existsSync(PID_FILE)) {
    const pid = Number(readFileSync(PID_FILE, "utf8"));
    if (Number.isInteger(pid) && pid > 1) {
      try {
        process.kill(-pid, "SIGKILL");
      } catch {
        // The process group is already gone.
      }
    }
  }
  rmSync(SCRIPT, { force: true });
  rmSync(PID_FILE, { force: true });
});

function probeAlive() {
  try {
    return (
      execSync(`pgrep -f '${PATTERN}' || true`, {
        encoding: "utf8",
      }).trim().length > 0
    );
  } catch {
    return false;
  }
}

describe("owned browser launch timeout", () => {
  it("returns nonzero promptly and kills the complete hung process group", async () => {
    const start = Date.now();
    await assert.rejects(
      launchBrowser(SCRIPT, { timeoutMs: 500 }),
      /devtools endpoint timeout/i,
    );
    const elapsed = Date.now() - start;
    assert.ok(elapsed < 5000, `failure must be bounded, elapsed=${elapsed}ms`);

    let alive = probeAlive();
    for (let i = 0; i < 10 && alive; i++) {
      await new Promise((resolve) => setTimeout(resolve, 100));
      alive = probeAlive();
    }
    assert.equal(
      alive,
      false,
      "hung browser and child process must be terminated",
    );
  });
});
