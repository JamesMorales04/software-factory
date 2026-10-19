// Pure SSE frame parser shared by the event-stream hook and its
// regression tests. It splits complete `\n\n`-terminated frames out of an
// incremental text buffer and decodes `data:` payloads as FactoryEvents.
//
// Structural guarantee: every loop iteration either consumes a complete
// frame (advancing the cursor) or stops at an incomplete tail. There is
// no code path — heartbeat-only frames, malformed frames, empty input —
// that can revisit the same buffer position, so the parser cannot spin.
import type { FactoryEvent } from "./api";

export interface ParsedChunk {
  events: FactoryEvent[];
  /** Unconsumed incomplete tail; feed it back with the next chunk. */
  carry: string;
  /** Frames with a `data:` line that failed to parse (skipped). */
  malformed: number;
  /** Frames without a `data:` line (`: ping` heartbeats, comments). */
  heartbeats: number;
}

export function parseEventChunk(buffer: string): ParsedChunk {
  const events: FactoryEvent[] = [];
  let malformed = 0;
  let heartbeats = 0;
  let rest = buffer;
  for (;;) {
    const boundary = rest.indexOf("\n\n");
    if (boundary < 0) {
      break;
    }
    const frame = rest.slice(0, boundary);
    rest = rest.slice(boundary + 2);
    const dataLine = frame.split("\n").find((line) => line.startsWith("data:"));
    if (!dataLine) {
      heartbeats += 1;
      continue;
    }
    try {
      events.push(
        JSON.parse(dataLine.slice("data:".length).trim()) as FactoryEvent,
      );
    } catch {
      malformed += 1;
    }
  }
  return { events, carry: rest, malformed, heartbeats };
}
