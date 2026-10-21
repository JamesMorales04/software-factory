import test from "node:test";
import assert from "node:assert/strict";
import { add, subtract, multiply, divide } from "../../src/calc.js";

test("integer arithmetic baseline", () => {
  assert.equal(add(2, 3), 5);
  assert.equal(subtract(9, 4), 5);
  assert.equal(multiply(6, 7), 42);
  assert.equal(divide(8, 2), 4);
});

test("operations compose", () => {
  assert.equal(multiply(add(1, 2), subtract(10, 4)), 18);
});
