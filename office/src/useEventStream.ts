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
  options: { enabled?: boolean; maxEvents?: number } = {},
) {
  const enabled = options.enabled ?? true;
  const maxEvents = options.maxEvents ?? 0;
  const [events, setEvents] = useState<FactoryEvent[]>([]);
  const [state, setState] = useState<StreamState>("connecting");
  const [lastMatchedSeq, setLastMatchedSeq] = useState(fromSeq);
  const cursorRef = useRef(fromSeq);
  const filterRef = useRef(filter);

  useEffect(() => {
    filterRef.current = filter;
  });

  useEffect(() => {
    if (!enabled) {
      setEvents([]);
      setState("connecting");
      setLastMatchedSeq(fromSeq);
      return;
    }
    const controller = new AbortController();
    const { signal } = controller;
    cursorRef.current = fromSeq;
    setEvents([]);
    setLastMatchedSeq(fromSeq);

    function append(e: FactoryEvent): void {
      // The ledger is gapless and strictly ordered, so the durable cursor is
      // also the deduplication boundary: replay/SSE overlap can only deliver
      // an event at or below it. This keeps client memory bounded without a
      // growing seen-set while preserving exactly-once UI updates.
      if (e.globalSeq <= cursorRef.current) {
        return;
      }
      cursorRef.current = e.globalSeq;
      const f = filterRef.current;
      if (!f || f(e)) {
        setLastMatchedSeq(e.globalSeq);
        setEvents((prev) => {
          const next = [...prev, e].sort((a, b) => a.globalSeq - b.globalSeq);
          return maxEvents > 0 && next.length > maxEvents
            ? next.slice(-maxEvents)
            : next;
        });
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
    //
    // A malformed frame fails the stream instead of being skipped: the
    // cursor must never cross an unprocessed sequence, so the hook
    // reconnects and re-establishes continuity from the last good cursor
    // through the validated replay endpoint.
    async function followStream(after: number): Promise<number> {
      const token = sessionStorage.getItem("factory-api-token");
      const res = await fetch(`/api/v1/events/stream?after=${after}`, {
        headers: token ? { Authorization: `Bearer ${token}` } : {},
        signal,
      });
      if (!res.ok || !res.body) {
        throw new Error(`stream HTTP ${res.status}`);
      }
      setState("live");
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
          if (parsed.malformed > 0) {
            throw new Error(
              `malformed event frame (${parsed.malformed} skipped)`,
            );
          }
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
      let backoff = 1000;
      // Initial backlog: retry until the API answers instead of dying in
      // `error` when the Core is briefly unreachable at mount. The error
      // state stays visible while retrying.
      for (;;) {
        try {
          after = await loadMissed(after);
          break;
        } catch {
          if (signal.aborted) {
            return;
          }
          setState("error");
          try {
            await sleepWithAbort(signal, backoff);
          } catch {
            return;
          }
          backoff = Math.min(backoff * 2, 10000);
        }
      }
      if (signal.aborted) {
        return;
      }
      backoff = 1000;
      while (!signal.aborted) {
        // Backoff resets only on progress (cursor advance): when the SSE
        // endpoint is down but replay answers, retries settle at the cap
        // instead of hammering every second forever.
        const before = after;
        try {
          after = await followStream(after);
          if (after !== before) {
            backoff = 1000;
          }
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
            if (after !== before) {
              backoff = 1000;
            }
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
  }, [fromSeq, enabled, maxEvents]);

  return { events, state, lastMatchedSeq };
}
