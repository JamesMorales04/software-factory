// Unit tests for the shutdown fallback (office/e2e-browser/harness.mjs).
// Run: npm run test:close (node:test stdlib only, no browser needed).
// These pin the composition with a fake browser: a hanging close must
// time out, SIGKILL, and report true; a clean close must return false
// fast (no lingering timer); a rejecting close must fail safe.

import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { closeBrowser, withTimeout } from "./harness.mjs";

const tracked = () => {
  const calls = [];
  return {
    calls,
    browser: ({ hang = false, reject = false } = {}) => ({
      close: () => {
        calls.push("close");
        if (hang) return new Promise(() => {});
        if (reject) return Promise.reject(new Error("boom"));
        return Promise.resolve();
      },
      process: () => ({
        kill: (sig) => {
          calls.push(`kill:${sig}`);
        },
      }),
    }),
  };
};

describe("closeBrowser", () => {
  it("returns false fast on a clean close (timer cleared, no linger)", async () => {
    const t = tracked();
    const start = Date.now();
    assert.equal(await closeBrowser(t.browser(), 20000), false);
    assert.ok(
      Date.now() - start < 2000,
      "clean close must not wait out the timer",
    );
    assert.deepEqual(t.calls, ["close"]);
  });

  it("times out a hanging close, SIGKILLs, and reports true", async () => {
    const t = tracked();
    const start = Date.now();
    assert.equal(await closeBrowser(t.browser({ hang: true }), 100), true);
    const elapsed = Date.now() - start;
    assert.ok(elapsed >= 90 && elapsed < 5000, `elapsed=${elapsed}`);
    assert.deepEqual(t.calls, ["close", "kill:SIGKILL"]);
  });

  it("fails safe when close itself rejects", async () => {
    const t = tracked();
    assert.equal(await closeBrowser(t.browser({ reject: true }), 5000), true);
    assert.deepEqual(t.calls, ["close", "kill:SIGKILL"]);
  });
});

describe("withTimeout", () => {
  it("resolves with the value on success", async () => {
    assert.equal(await withTimeout(Promise.resolve(7), 1000, "nope"), 7);
  });

  it("rejects with the message on timeout", async () => {
    await assert.rejects(
      withTimeout(new Promise(() => {}), 50, "too slow"),
      /too slow/,
    );
  });
});
