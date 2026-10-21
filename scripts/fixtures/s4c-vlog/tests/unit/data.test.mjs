import test from "node:test";
import assert from "node:assert/strict";
import { categories, entries } from "../../src/data.js";

test("sample entries are deterministic and categorized", () => {
  assert.equal(entries.length, 5);
  assert.deepEqual(
    entries.map((entry) => entry.category).sort(),
    ["Cooking", "Technology", "Technology", "Travel", "Travel"],
  );
  assert.deepEqual(categories, ["All", "Travel", "Technology", "Cooking", "Film"]);
});

test("every entry has a stable id and duration", () => {
  const ids = new Set(entries.map((entry) => entry.id));
  assert.equal(ids.size, entries.length);
  for (const entry of entries) {
    assert.match(entry.duration, /^\d{2}:\d{2}$/);
  }
});
