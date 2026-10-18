import { useEffect, useRef, useState } from "react";
import { api, type FactoryEvent } from "./api";

export type StreamState = "connecting" | "live" | "reconnecting" | "error";

/**
 * useEventStream tails the durable ledger: it first loads the missed suffix
 * via the replay endpoint from `fromSeq`, then follows the SSE stream and
 * resumes from the last observed globalSeq on every reconnect. The ledger
 * cursor — never wall-clock time — is the source of continuity, so a
 * reconnect yields neither gaps nor duplicates.
 */
export function useEventStream(
  fromSeq = 0,
  filter?: (e: FactoryEvent) => boolean,
) {
  const [events, setEvents] = useState<FactoryEvent[]>([]);
  const [state, setState] = useState<StreamState>("connecting");
  const cursorRef = useRef(fromSeq);
  const seenRef = useRef(new Set<number>());
  const filterRef = useRef(filter);
  filterRef.current = filter;

  useEffect(() => {
    let cancelled = false;
    let backoff = 1000;
    cursorRef.current = fromSeq;
    seenRef.current = new Set();
    setEvents([]);

    async function append(e: FactoryEvent) {
      if (seenRef.current.has(e.globalSeq)) {
        return;
      }
      seenRef.current.add(e.globalSeq);
      cursorRef.current = Math.max(cursorRef.current, e.globalSeq);
      const f = filterRef.current;
      if (!f || f(e)) {
        setEvents((prev) =>
          [...prev, e].sort((a, b) => a.globalSeq - b.globalSeq),
        );
      }
    }

    async function loadMissed(after: number): Promise<number> {
      const page = await api.listEvents(after);
      for (const e of page.events) {
        await append(e);
      }
      return page.nextAfter;
    }

    // Parse one SSE text stream with the session token in the header.
    // (EventSource cannot set Authorization headers, and the token must
    // not travel in the URL query string.)
    async function followStream(
      signal: AbortSignal,
      after: number,
    ): Promise<number> {
      const token = sessionStorage.getItem("factory-api-token");
      const res = await fetch(`/api/v1/events/stream?after=${after}`, {
        headers: token ? { Authorization: `Bearer ${token}` } : {},
        signal,
      });
      if (!res.ok || !res.body) {
        throw new Error(`stream HTTP ${res.status}`);
      }
      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";
      let next = after;
      for (;;) {
        const { done, value } = await reader.read();
        if (done) {
          break;
        }
        buffer += decoder.decode(value, { stream: true });
        let boundary = buffer.indexOf("\n\n");
        while (boundary >= 0) {
          const frame = buffer.slice(0, boundary);
          buffer = buffer.slice(boundary + 2);
          const dataLine = frame
            .split("\n")
            .find((line) => line.startsWith("data:"));
          if (!dataLine) {
            continue; // comment / heartbeat
          }
          try {
            const event = JSON.parse(dataLine.slice(5).trim()) as FactoryEvent;
            await append(event);
            next = Math.max(next, event.globalSeq);
          } catch {
            // Ignore a malformed frame; the cursor still resumes cleanly.
          }
          boundary = buffer.indexOf("\n\n");
        }
      }
      return next;
    }

    async function loop() {
      let after = cursorRef.current;
      try {
        after = await loadMissed(after);
      } catch {
        if (!cancelled) {
          setState("error");
        }
        return;
      }
      if (cancelled) {
        return;
      }
      setState("live");
      backoff = 1000;
      while (!cancelled) {
        const controller = new AbortController();
        try {
          after = await followStream(controller.signal, after);
          if (cancelled) {
            return;
          }
          setState("reconnecting");
        } catch {
          if (cancelled) {
            return;
          }
          setState("reconnecting");
          try {
            after = await loadMissed(after);
          } catch {
            // Stay in reconnecting; retry below with backoff.
          }
        }
        await new Promise((resolve) => setTimeout(resolve, backoff));
        backoff = Math.min(backoff * 2, 10000);
      }
    }

    void loop();
    return () => {
      cancelled = true;
    };
  }, [fromSeq]);

  return { events, state };
}
