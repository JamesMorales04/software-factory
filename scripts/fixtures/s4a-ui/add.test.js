const test = require("node:test");
const assert = require("node:assert");
const { add } = require("./add.js");

test("add works", () => {
  assert.strictEqual(add(2, 3), 5);
});
