// Regression tests for the SSE frame parser (office/src/sse.ts).
// Run: npm run test:parser  (compiles the actual TS source with esbuild,
// then executes these assertions with the node:test runner — no new deps).
// The heartbeat-only cases are the direct regression for the F1-S1
// main-thread freeze: the old inline parser never advanced past a
// comment-only frame and looped synchronously forever.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { parseEventChunk } from "./.sse.cjs";

const ev = (seq, title = "t") =>
  `event: factory\ndata: {"eventId":"e${seq}","eventType":"ActionCreated","schemaVersion":"1","aggregateType":"action","aggregateId":"a","sequence":${seq},"globalSeq":${seq},"occurredAt":"2026-10-19T00:00:00Z","recordedAt":"2026-10-19T00:00:00Z","correlationId":"c","causationId":"","actor":"local-operator","payload":{"title":"${title}"}}`;

describe("parseEventChunk", () => {
  it("terminates on heartbeat-only input and counts heartbeats", () => {
    const r = parseEventChunk(": ping\n\n: ping\n\n: ping\n\n");
    assert.equal(r.events.length, 0);
    assert.equal(r.heartbeats, 3);
    assert.equal(r.malformed, 0);
    assert.equal(r.carry, "");
  });

  it("survives a thousand heartbeats without hanging", () => {
    const r = parseEventChunk(": ping\n\n".repeat(1000));
    assert.equal(r.heartbeats, 1000);
    assert.equal(r.events.length, 0);
    assert.equal(r.carry, "");
  });

  it("holds an incomplete tail as carry and completes it later", () => {
    const first = parseEventChunk(`${ev(1).slice(0, 40)}`);
    assert.equal(first.events.length, 0);
    assert.ok(first.carry.length > 0);
    const second = parseEventChunk(first.carry + `${ev(1).slice(40)}\n\n`);
    assert.equal(second.events.length, 1);
    assert.equal(second.events[0].globalSeq, 1);
    assert.equal(second.carry, "");
  });

  it("handles mixed heartbeats, events and malformed frames in order", () => {
    const input = `: ping\n\n${ev(1)}\n\n: ping\n\nevent: factory\ndata: not-json\n\n${ev(2)}\n\n`;
    const r = parseEventChunk(input);
    assert.equal(r.heartbeats, 2);
    assert.equal(r.malformed, 1);
    assert.deepEqual(
      r.events.map((e) => e.globalSeq),
      [1, 2],
    );
    assert.equal(r.carry, "");
  });

  it("returns empty results for empty input", () => {
    assert.deepEqual(parseEventChunk(""), {
      events: [],
      carry: "",
      malformed: 0,
      heartbeats: 0,
    });
  });

  it("keeps unterminated garbage as carry instead of spinning", () => {
    const r = parseEventChunk("garbage without terminator");
    assert.equal(r.events.length, 0);
    assert.equal(r.carry, "garbage without terminator");
  });
});
