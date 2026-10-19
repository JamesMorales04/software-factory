import { useEffect, useRef, useState } from "react";
import { api, type FactoryEvent } from "./api";
import { parseEventChunk } from "./sse";

export type StreamState = "connecting" | "live" | "reconnecting" | "error";

/** Sleep that rejects promptly when the stream is torn down. */
function sleepWithAbort(signal: AbortSignal, ms: number): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) {
      reject(new DOMException("Aborted", "AbortError"));
      return;
    }
    const timer = setTimeout(() => {
      signal.removeEventListener("abort", onAbort);
      resolve();
    }, ms);
    const onAbort = () => {
      clearTimeout(timer);
      reject(new DOMException("Aborted", "AbortError"));
    };
    signal.addEventListener("abort", onAbort, { once: true });
  });
}

/**
 * useEventStream tails the durable ledger: it first loads the missed suffix
 * via the replay endpoint from `fromSeq`, then follows the SSE stream and
 * resumes from the last observed globalSeq on every reconnect. The ledger
 * cursor — never wall-clock time — is the source of continuity, so a
 * reconnect yields neither gaps nor duplicates.
 *
 * Lifecycle: one AbortController owns the whole effect. Unmount (including
 * React StrictMode remounts and in-app navigation) aborts the in-flight
 * fetch, releases the reader, clears the retry timer, and stops the loop —
 * no leaked connections, readers, or timers.
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

  useEffect(() => {
    filterRef.current = filter;
  });

  useEffect(() => {
    const controller = new AbortController();
    const { signal } = controller;
    cursorRef.current = fromSeq;
    seenRef.current = new Set();
    setEvents([]);

    function append(e: FactoryEvent): void {
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
      const page = await api.listEvents(after, 100, signal);
      for (const e of page.events) {
        append(e);
      }
      return page.nextAfter;
    }

    // Parse one SSE text stream with the session token in the header.
    // (EventSource cannot set Authorization headers, and the token must
    // not travel in the URL query string.)
    async function followStream(after: number): Promise<number> {
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
      let carry = "";
      let next = after;
      try {
        for (;;) {
          const { done, value } = await reader.read();
          if (done) {
            break;
          }
          carry += decoder.decode(value, { stream: true });
          const parsed = parseEventChunk(carry);
          carry = parsed.carry;
          for (const event of parsed.events) {
            append(event);
            next = Math.max(next, event.globalSeq);
          }
        }
      } finally {
        // Release the lock even when aborted mid-frame so the connection
        // and its server-side handler are reclaimed promptly.
        try {
          await reader.cancel();
        } catch {
          // Already closed by the remote end; nothing to cancel.
        }
        reader.releaseLock();
      }
      return next;
    }

    async function loop(): Promise<void> {
      let after = cursorRef.current;
      try {
        after = await loadMissed(after);
      } catch {
        if (signal.aborted) {
          return;
        }
        setState("error");
        return;
      }
      if (signal.aborted) {
        return;
      }
      setState("live");
      let backoff = 1000;
      while (!signal.aborted) {
        try {
          after = await followStream(after);
          backoff = 1000;
          if (signal.aborted) {
            return;
          }
          setState("reconnecting");
        } catch {
          if (signal.aborted) {
            return;
          }
          setState("reconnecting");
          try {
            after = await loadMissed(after);
            backoff = 1000;
          } catch {
            if (signal.aborted) {
              return;
            }
            // Stay in reconnecting; retry below with backoff.
          }
        }
        try {
          await sleepWithAbort(signal, backoff);
        } catch {
          return;
        }
        backoff = Math.min(backoff * 2, 10000);
      }
    }

    void loop();
    return () => {
      controller.abort();
    };
  }, [fromSeq]);

  return { events, state };
}
