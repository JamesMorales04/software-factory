import assert from "node:assert/strict";
import test from "node:test";
import { greeting } from "../../src/greeting.js";

test("greeting uses the requested name", () => {
  assert.equal(greeting("Factory"), "Hello, Factory!");
});

test("greeting rejects empty and non-string names", () => {
  assert.throws(() => greeting(""), TypeError);
  assert.throws(() => greeting(42), TypeError);
});
