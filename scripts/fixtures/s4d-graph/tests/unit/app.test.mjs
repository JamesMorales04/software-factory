import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

test("frontend wires greeting into the #greeting element", () => {
  const source = readFileSync(new URL("../../src/app.js", import.meta.url), "utf8");
  assert.match(source, /greeting\("Factory"\)/);
  assert.match(source, /#greeting/);
});
